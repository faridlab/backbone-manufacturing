//! Job-card lifecycle (hand-authored, user-owned).
//!
//! An `impl ManufacturingWriteService` chunk over the vocabulary in
//! [`super::manufacturing_write_service`]. A job card records an operation run against a Work Order;
//! completing it charges the operation's conversion cost to WIP (`Dr WIP · Cr Conversion-Applied`),
//! gated once-only on the completion (idempotent on retry, key `operate:{job_card_id}`). This is the
//! second of the balanced WIP posts (consume + operate + receive nets WIP to zero on completion).
//!
//! The card's own states are: `ready` (fresh, or reservation-projected back), `blocked` (waiting on
//! component reservations — a projection flip, not a hand write), `progress` (started on the floor),
//! `done`, `cancel`. Starting from `blocked` is allowed: the floor may begin the moment parts
//! physically arrive, without waiting for the projection.
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
    /// Record a job card for an operation run — it starts `ready` (the reservation projection may
    /// flip it to `blocked` until components are reserved).
    pub async fn add_job_card(&self, j: NewJobCard) -> Result<Uuid, ManufacturingError> {
        if j.total_time_mins < Decimal::ZERO || j.hour_rate < Decimal::ZERO {
            return Err(ManufacturingError::Invalid("bad time/rate".into()));
        }
        let id = Uuid::new_v4();
        let cost = money(j.total_time_mins / sixty() * j.hour_rate);
        // RLS scope (ADR-0008): company on the DTO — scope the insert so it passes the WITH CHECK fence.
        company_scope::with_company_scope(
            Some(j.company_id),
            self.job_cards.insert_ready(&self.pool, &NewJobCardRow {
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

    /// Start a job card on the floor: ready|blocked → progress.
    ///
    /// Starting from `blocked` is allowed — parts arriving physically is enough; the card does not
    /// wait for the reservation projection to flip it back to ready. The parent work order must be
    /// confirmed (or already in progress) — a draft/cancelled order has no floor work.
    pub async fn start_job_card(&self, job_card_id: Uuid) -> Result<(), ManufacturingError> {
        let jc = self.job_cards.find_completion_source(&self.pool, job_card_id).await?
            .ok_or(ManufacturingError::NotFound("job card"))?;
        match jc.status.as_str() {
            "ready" | "blocked" => {}
            "progress" => return Ok(()), // already started — idempotent
            "done" => return Err(ManufacturingError::InvalidState("job card is already done")),
            "cancel" => return Err(ManufacturingError::InvalidState("job card is cancelled")),
            _ => return Err(ManufacturingError::InvalidState("job card state does not allow start")),
        }
        if jc.work_order_status != "confirmed" && jc.work_order_status != "progress" {
            return Err(ManufacturingError::InvalidState(
                "work order is not open for operations (confirm it first)",
            ));
        }
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, jc.company_id).await?;
        let moved = self.job_cards.gate_start(&mut tx, job_card_id).await?;
        if moved != 1 {
            tx.rollback().await?;
            return Ok(()); // a concurrent start won — same end state
        }
        tx.commit().await?;
        Ok(())
    }

    /// Complete a job card: charge its conversion cost to WIP (`Dr WIP · Cr Conversion-Applied`).
    /// Once-only — the gate refuses a done/cancelled card, and the post is idempotent on
    /// `operate:{job_card_id}`.
    pub async fn complete_job_card(
        &self,
        job_card_id: Uuid,
        gl: &dyn GlPostSink,
        sink: &dyn ManufacturingEventSink,
    ) -> Result<Decimal, ManufacturingError> {
        // RLS scope (ADR-0008), ID-only pattern — see `confirm_work_order`: the join is fenced by the
        // request-dedicated connection; the transaction below binds the job card's own company.
        let jc = self.job_cards.find_completion_source(&self.pool, job_card_id).await?
            .ok_or(ManufacturingError::NotFound("job card"))?;
        if jc.status == "done" {
            return Ok(jc.operating_cost);
        }
        if jc.status == "cancel" {
            return Err(ManufacturingError::InvalidState("job card is cancelled"));
        }
        if jc.work_order_status != "confirmed" && jc.work_order_status != "progress" {
            return Err(ManufacturingError::InvalidState("work order not open for operations"));
        }
        let company_id: Uuid = jc.company_id;
        let wo_id: Uuid = jc.work_order_id;
        let cost: Decimal = jc.operating_cost;
        let wip = self
            .resolve_account(
                "wip",
                jc.wip_account_id,
                company_id,
                jc.product_category_id,
                |d| d.wip_account_id,
            )
            .await?;
        let conv = self
            .resolve_account(
                "conversion_cost",
                jc.conversion_cost_account_id,
                company_id,
                jc.product_category_id,
                |d| d.conversion_cost_account_id,
            )
            .await?;

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

    /// Cancel a job card: ready|blocked|progress → cancel (terminal, sticky).
    ///
    /// A `done` card has already charged conversion cost to WIP — cancelling it would strand the
    /// charge, so the refusal is LOUD.
    pub async fn cancel_job_card(&self, job_card_id: Uuid) -> Result<(), ManufacturingError> {
        let jc = self.job_cards.find_completion_source(&self.pool, job_card_id).await?
            .ok_or(ManufacturingError::NotFound("job card"))?;
        match jc.status.as_str() {
            "ready" | "blocked" | "progress" => {}
            "done" => return Err(ManufacturingError::InvalidState(
                "job card is done — its conversion cost is already in WIP and cannot be cancelled",
            )),
            "cancel" => return Err(ManufacturingError::InvalidState("job card is already cancelled")),
            _ => return Err(ManufacturingError::InvalidState("job card state does not allow cancel")),
        }
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, jc.company_id).await?;
        let moved = self.job_cards.gate_cancel(&mut tx, job_card_id).await?;
        if moved != 1 {
            tx.rollback().await?;
            return Err(ManufacturingError::InvalidState(
                "job card has left the cancellable states",
            ));
        }
        tx.commit().await?;
        Ok(())
    }
}
