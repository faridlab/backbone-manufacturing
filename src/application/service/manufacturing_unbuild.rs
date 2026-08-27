//! Unbuild: disassemble a done order's output back into components (hand-authored, user-owned).
//!
//! An `impl ManufacturingWriteService` chunk over the vocabulary in
//! [`super::manufacturing_write_service`]. An unbuild order is a plain 2-state record (draft →
//! done) that reverses FINISHED goods of a DONE work order back into components. It NEVER creates
//! a reverse work order — there is nothing to schedule, only stock to move back — and it NEVER
//! writes `manufacturing.work_orders` (the source order stays `done`, its accumulators untouched;
//! how much of its output has been unbuilt is answered by summing done unbuilds, which is exactly
//! what [`UnbuildRepository::find_execute_source`] does).
//!
//! The reversal is ONE port call ([`InventoryPort::reverse_production`]): the FG line leaves stock
//! and every component returns at its share of the recovered value. The GL post mirrors it —
//! `Dr Raw-Material Stock Σ · Cr Finished-Goods Σ` (same Σ, so the reversal moves value between
//! estates without creating any).
//!
//! Guards, both LOUD: the source work order must be `done` ([`ManufacturingError::UnbuildSourceNotDone`]
//! — unbuilding a half-produced order would strand WIP), and the requested quantity may not exceed
//! what remains producible after every OTHER done unbuild of the same source
//! ([`ManufacturingError::UnbuildOverRemaining`] — never a silent clip).
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on `UnbuildRepository`
//! / `WorkOrderRepository` / `WorkOrderItemRepository`.

use backbone_orm::company_scope;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::infrastructure::persistence::NewUnbuildRow;

use super::manufacturing_events::*;
use super::manufacturing_gl::{AccountingPostEnvelope, GlPostLine, GlPostSink};
use super::manufacturing_ports::{InventoryPort, IssueLine, UnbuildReversal};
use super::manufacturing_write_service::{
    money, is_dup, ManufacturingError, ManufacturingWriteService, NewUnbuild, UnbuildOutcome,
};

impl ManufacturingWriteService {
    /// Open a draft unbuild order against a (presumably done) work order. The source's doneness
    /// is checked at EXECUTE, not here — a draft may be authored while production finishes.
    pub async fn create_unbuild(&self, o: NewUnbuild) -> Result<Uuid, ManufacturingError> {
        if o.quantity <= Decimal::ZERO {
            return Err(ManufacturingError::Invalid("unbuild quantity must be positive".into()));
        }
        let id = Uuid::new_v4();
        // RLS scope (ADR-0008): company on the DTO — scope the insert so it passes the WITH CHECK fence.
        let r = company_scope::with_company_scope(
            Some(o.company_id),
            self.unbuilds.insert_draft(&self.pool, &NewUnbuildRow {
                id,
                company_id: o.company_id,
                unbuild_number: &o.unbuild_number,
                work_order_id: o.work_order_id,
                item_id: o.item_id,
                quantity: o.quantity,
            }),
        )
        .await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { ManufacturingError::DuplicateNumber(o.unbuild_number) } else { e.into() });
        }
        Ok(id)
    }

    /// Execute an unbuild: reverse the finished goods back into components.
    ///
    /// Side effects before the gate (the established ordering): the port call and the GL post are
    /// idempotent on `unbuild:{id}`, and the draft→done gate is the once-only guard — a crash
    /// between them leaves a re-executable draft, never a double reversal.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_unbuild(
        &self,
        unbuild_id: Uuid,
        raw_warehouse_id: Uuid,
        inventory: &dyn InventoryPort,
        gl: &dyn GlPostSink,
        sink: &dyn ManufacturingEventSink,
    ) -> Result<UnbuildOutcome, ManufacturingError> {
        let u = self.unbuilds.find_execute_source(&self.pool, unbuild_id).await?
            .ok_or(ManufacturingError::NotFound("unbuild order"))?;
        if u.status == "done" {
            return Ok(UnbuildOutcome { reversed_value: Decimal::ZERO, already: true });
        }
        if u.status != "draft" {
            return Err(ManufacturingError::InvalidState("unbuild order is not draft"));
        }
        // The source must be FULLY produced: unbuilding a half-produced order would reverse goods
        // whose WIP is still open — refused LOUDLY, never a partial reversal.
        if u.work_order_status != "done" {
            return Err(ManufacturingError::UnbuildSourceNotDone);
        }
        let remaining = u.produced_qty - u.already_unbuilt;
        if u.quantity > remaining {
            return Err(ManufacturingError::UnbuildOverRemaining {
                requested: u.quantity,
                remaining,
            });
        }

        // The source order's own projection (estate value + warehouses + accounts). Scoped read:
        // the unbuild's company was read above, bind it for this second hop.
        let wo = company_scope::with_company_scope(Some(u.company_id), self.load_wo(u.work_order_id)).await?;
        if wo.status != "done" {
            return Err(ManufacturingError::UnbuildSourceNotDone);
        }
        let fg_wh = wo.fg_warehouse_id
            .ok_or(ManufacturingError::MissingAccount("fg_warehouse"))?;
        let fg_acct = self
            .resolve_account("finished_goods", wo.fg_account_id, wo.company_id, wo.product_category_id, |d| d.fg_account_id)
            .await?;
        let raw_acct = self
            .resolve_account("raw_material", wo.raw_material_account_id, wo.company_id, wo.product_category_id, |d| d.raw_material_account_id)
            .await?;

        // Components back = the order's ACTUAL per-unit consumption, prorated by the unbuilt share.
        let items = company_scope::with_company_scope(
            Some(u.company_id),
            self.work_order_items.list_requirements(&self.pool, u.work_order_id),
        )
        .await?;
        let share = if u.produced_qty > Decimal::ZERO { u.quantity / u.produced_qty } else { Decimal::ZERO };
        let components: Vec<IssueLine> = items
            .iter()
            .filter(|i| i.consumed_qty > Decimal::ZERO)
            .map(|i| IssueLine {
                item_id: i.item_id,
                quantity: money(i.consumed_qty * share),
            })
            .filter(|l| l.quantity > Decimal::ZERO)
            .collect();

        // The value reversed off the FG estate: this share of the order's job-order cost. The GL
        // mirrors the stock move exactly (Dr Raw Σ · Cr FG Σ), so no value is created or destroyed.
        let reversed_value = money((wo.raw_material_cost + wo.operating_cost) * share);

        // SIDE EFFECTS BEFORE THE GATE — idempotent on `unbuild:{id}`.
        let dedup = format!("unbuild:{unbuild_id}");
        inventory
            .reverse_production(&UnbuildReversal {
                company_id: u.company_id,
                unbuild_order_id: unbuild_id,
                source_work_order_id: u.work_order_id,
                fg_warehouse_id: fg_wh,
                item_id: u.item_id,
                quantity: u.quantity,
                value: reversed_value,
                raw_warehouse_id,
                components: components.clone(),
                idempotency_key: dedup.clone(),
            })
            .await
            .map_err(|r| ManufacturingError::Inventory(r.code))?;

        let env = AccountingPostEnvelope {
            idempotency_key: dedup.clone(),
            company_id: u.company_id,
            branch_id: None,
            source_type: "manufacturing".into(),
            source_id: Uuid::new_v5(&unbuild_id, b"manufacturing:unbuild"),
            source_reference: Some(wo.work_order_number.clone()),
            posting_date: chrono::Utc::now().date_naive(),
            currency: "IDR".into(),
            posting_type: "original".into(),
            description: Some("unbuild reversal".into()),
            lines: vec![
                GlPostLine::debit(raw_acct, reversed_value).with_description("Raw material stock"),
                GlPostLine::credit(fg_acct, reversed_value).with_description("Finished goods"),
            ],
        };
        self.post(gl, &env).await?;

        // THE GATE, last: draft → done (the once-only guard on the reversal).
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, u.company_id).await?;
        let moved = self.unbuilds.gate_execute(&mut tx, unbuild_id).await?;
        if moved != 1 {
            tx.rollback().await?;
            return Ok(UnbuildOutcome { reversed_value, already: true });
        }
        tx.commit().await?;

        sink.publish(&ManufacturingEvent::UnbuildExecuted(UnbuildExecuted {
            unbuild_order_id: unbuild_id,
            source_work_order_id: u.work_order_id,
            company_id: u.company_id,
            item_id: u.item_id,
            quantity: u.quantity,
            reversed_value,
        }));
        Ok(UnbuildOutcome { reversed_value, already: false })
    }
}
