//! Job-card open + completion (hand-authored, user-owned).
//!
//! An `impl ManufacturingWriteService` chunk over the vocabulary in
//! [`super::manufacturing_write_service`]. A job card records an operation run against a Work Order;
//! completing it charges the operation's conversion cost to WIP (`Dr WIP · Cr Conversion-Applied`),
//! gated open → completed (the once-only charge, idempotent on retry). This is the third of the three
//! balanced WIP posts (consume + operate + receive nets WIP to zero on completion).
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on `JobCardRepository`
//! / `WorkOrderRepository`, whose gate methods take THIS service's transaction so the once-only guard
//! commits with the cost accumulation.

use backbone_orm::company_scope;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::infrastructure::persistence::NewJobCardRow;

use super::manufacturing_events::*;
use super::manufacturing_gl::{AccountingPostEnvelope, GlPostLine, GlPostSink};
use super::manufacturing_write_service::{
    money, sixty, ManufacturingError, ManufacturingWriteService, NewJobCard,
};

impl ManufacturingWriteService {
    /// Record a job card for an operation run (open).
    pub async fn add_job_card(&self, j: NewJobCard) -> Result<Uuid, ManufacturingError> {
        if j.total_time_mins < Decimal::ZERO || j.hour_rate < Decimal::ZERO {
            return Err(ManufacturingError::Invalid("bad time/rate".into()));
        }
        let id = Uuid::new_v4();
        let cost = money(j.total_time_mins / sixty() * j.hour_rate);
        // RLS scope (ADR-0008): company on the DTO — scope the insert so it passes the WITH CHECK fence.
        company_scope::with_company_scope(
            Some(j.company_id),
            self.job_cards.insert_open(&self.pool, &NewJobCardRow {
                id,
                company_id: j.company_id,
                work_order_id: j.work_order_id,
                operation_id: j.operation_id,
                workstation_id: j.workstation_id,
                total_time_mins: j.total_time_mins,
                hour_rate: j.hour_rate,
                operating_cost: cost,
            }),
        )
        .await?;
        Ok(id)
    }

    /// Complete a job card: charge its conversion cost to WIP (`Dr WIP · Cr Conversion-Applied`).
    /// Gated open → completed (the once-only charge, idempotent on retry).
    pub async fn complete_job_card(
        &self,
        job_card_id: Uuid,
        gl: &dyn GlPostSink,
        sink: &dyn ManufacturingEventSink,
    ) -> Result<Decimal, ManufacturingError> {
        // RLS scope (ADR-0008), ID-only pattern — see `release_work_order`: the join is fenced by the
        // request-dedicated connection; the transaction below binds the job card's own company.
        let jc = self.job_cards.find_completion_source(&self.pool, job_card_id).await?
            .ok_or(ManufacturingError::NotFound("job card"))?;
        if jc.status == "completed" {
            return Ok(jc.operating_cost);
        }
        if jc.work_order_status != "in_process" && jc.work_order_status != "released" {
            return Err(ManufacturingError::InvalidState("work order not open for operations"));
        }
        let company_id: Uuid = jc.company_id;
        let wo_id: Uuid = jc.work_order_id;
        let cost: Decimal = jc.operating_cost;
        let wip = jc.wip_account_id.ok_or(ManufacturingError::MissingAccount("wip"))?;
        let conv = jc
            .conversion_cost_account_id
            .ok_or(ManufacturingError::MissingAccount("conversion_cost"))?;

        if cost > Decimal::ZERO {
            let env = AccountingPostEnvelope {
                idempotency_key: format!("operate:{job_card_id}"),
                company_id,
                branch_id: None,
                source_type: "manufacturing".into(),
                // The job card id is already a distinct voucher id.
                source_id: job_card_id,
                source_reference: Some(jc.work_order_number.clone()),
                posting_date: chrono::Utc::now().date_naive(),
                currency: "IDR".into(),
                posting_type: "original".into(),
                description: Some("conversion cost to WIP".into()),
                lines: vec![
                    GlPostLine::debit(wip, cost).with_description("WIP"),
                    GlPostLine::credit(conv, cost).with_description("Conversion applied"),
                ],
            };
            self.post(gl, &env).await?;
        }

        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company_id).await?;
        let moved = self.job_cards.gate_complete(&mut tx, job_card_id).await?;
        if moved != 1 {
            tx.rollback().await?;
            return Ok(cost); // already completed concurrently; post deduped
        }
        if cost > Decimal::ZERO {
            self.work_orders.add_operating_cost(&mut tx, wo_id, cost).await?;
        }
        tx.commit().await?;
        sink.publish(&ManufacturingEvent::ConversionCharged(ConversionCharged {
            job_card_id,
            work_order_id: wo_id,
            company_id,
            operating_cost: cost,
        }));
        Ok(cost)
    }
}
