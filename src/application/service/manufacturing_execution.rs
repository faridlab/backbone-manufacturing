//! Execution: the WIP consume + receive seam (hand-authored, user-owned).
//!
//! An `impl ManufacturingWriteService` chunk over the vocabulary in
//! [`super::manufacturing_write_service`]. A Work Order's value flows through WIP in two balanced
//! posts here (the third — `operate` — lives in [`super::manufacturing_job_card`]):
//!   consume  Dr WIP · Cr Raw-Material Stock   (materials issued; value from real inventory)
//!   receive  Dr Finished-Goods · Cr WIP        (FG = raw + operating)
//! Each post is transition-gated (the status advance is the once-only guard) and carries a stable
//! idempotency key, so a retry never double-charges WIP. Manufacturing owns no stock or ledger: it
//! drives inventory (InventoryPort) for the physical moves + valuation and emits the posts (GlPostSink).
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on `WorkOrderRepository`
//! / `WorkOrderItemRepository`, whose gate methods take THIS service's transaction so the consume /
//! receive commits as one unit (the once-only guard).

use backbone_orm::company_scope;
use rust_decimal::Decimal;
use uuid::Uuid;

use super::manufacturing_events::*;
use super::manufacturing_gl::{AccountingPostEnvelope, GlPostLine, GlPostSink};
use super::manufacturing_ports::{FinishedReceipt, InventoryPort, IssueLine, MaterialIssue};
use super::manufacturing_write_service::{
    money, ConsumeOutcome, ManufacturingError, ManufacturingWriteService, ReceiveOutcome,
};

impl ManufacturingWriteService {
    /// Issue all required materials to WIP: drive inventory (value from moving-average) + post
    /// `Dr WIP · Cr Raw-Material Stock`. Gated released → in_process (the once-only consume).
    pub async fn consume_materials(
        &self,
        wo_id: Uuid,
        raw_warehouse_id: Uuid,
        inventory: &dyn InventoryPort,
        gl: &dyn GlPostSink,
        sink: &dyn ManufacturingEventSink,
    ) -> Result<ConsumeOutcome, ManufacturingError> {
        let wo = self.load_wo(wo_id).await?;
        if wo.status == "in_process" || wo.status == "completed" {
            return Ok(ConsumeOutcome { raw_material_value: wo.raw_material_cost, already: true });
        }
        if wo.status != "released" {
            return Err(ManufacturingError::InvalidState("work order is not released"));
        }
        let wip = wo.wip_account_id.ok_or(ManufacturingError::MissingAccount("wip"))?;
        let raw_acct = wo.raw_material_account_id.ok_or(ManufacturingError::MissingAccount("raw_material"))?;

        // RLS scope (ADR-0008): `load_wo` read the work order (fenced by the request connection), so its
        // company is known here — bind it explicitly for the line read and the transaction below.
        let items = company_scope::with_company_scope(
            Some(wo.company_id),
            self.work_order_items.list_requirements(&self.pool, wo_id),
        )
        .await?;
        let mut lines = Vec::new();
        for it in &items {
            let remaining = it.required_qty - it.consumed_qty;
            if remaining > Decimal::ZERO {
                lines.push((it.id, it.item_id, remaining));
            }
        }
        if lines.is_empty() {
            return Ok(ConsumeOutcome { raw_material_value: wo.raw_material_cost, already: true });
        }

        // Drive real inventory to remove the components and tell us what they were worth.
        let issue = MaterialIssue {
            company_id: wo.company_id,
            work_order_id: wo_id,
            warehouse_id: raw_warehouse_id,
            idempotency_key: format!("consume:{wo_id}"),
            lines: lines.iter().map(|(_, item, qty)| IssueLine { item_id: *item, quantity: *qty }).collect(),
        };
        let ack = inventory
            .issue_to_wip(&issue)
            .await
            .map_err(|r| ManufacturingError::Inventory(r.code))?;
        let raw_value = money(ack.total_value);

        // Emit the consume post: Dr WIP · Cr Raw-Material Stock.
        let env = AccountingPostEnvelope {
            idempotency_key: format!("consume:{wo_id}"),
            company_id: wo.company_id,
            branch_id: None,
            source_type: "manufacturing".into(),
            // Each manufacturing post is a distinct voucher; accounting dedups on (company, source_type,
            // source_id, posting_type='original'), so derive a stable source_id per post kind.
            source_id: Uuid::new_v5(&wo_id, b"manufacturing:consume"),
            source_reference: Some(wo.work_order_number.clone()),
            posting_date: chrono::Utc::now().date_naive(),
            currency: "IDR".into(),
            posting_type: "original".into(),
            description: Some("material issue to WIP".into()),
            lines: vec![
                GlPostLine::debit(wip, raw_value).with_description("WIP"),
                GlPostLine::credit(raw_acct, raw_value).with_description("Raw material stock"),
            ],
        };
        self.post(gl, &env).await?;

        // Record consumption + advance state, gated on released → in_process.
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, wo.company_id).await?;
        let moved = self.work_orders.gate_consume(&mut tx, wo_id, raw_value).await?;
        if moved != 1 {
            // Someone else consumed concurrently — the post deduped; don't double-book state.
            tx.rollback().await?;
            let now = self.load_wo(wo_id).await?;
            return Ok(ConsumeOutcome { raw_material_value: now.raw_material_cost, already: true });
        }
        for (line_id, _item, qty) in &lines {
            self.work_order_items.add_consumed_qty(&mut tx, *line_id, *qty).await?;
        }
        tx.commit().await?;
        sink.publish(&ManufacturingEvent::MaterialsConsumed(MaterialsConsumed {
            work_order_id: wo_id,
            company_id: wo.company_id,
            raw_material_value: raw_value,
        }));
        Ok(ConsumeOutcome { raw_material_value: raw_value, already: false })
    }

    /// Receive finished goods into stock at cost (raw + operating, prorated) and post
    /// `Dr Finished-Goods · Cr WIP`. Bounded by the ordered quantity; on full receipt WIP nets to zero
    /// and the WO completes. Gated on the produced-quantity advance (idempotent per completion).
    pub async fn receive_finished(
        &self,
        wo_id: Uuid,
        produced_qty: Decimal,
        inventory: &dyn InventoryPort,
        gl: &dyn GlPostSink,
        sink: &dyn ManufacturingEventSink,
    ) -> Result<ReceiveOutcome, ManufacturingError> {
        if produced_qty <= Decimal::ZERO {
            return Err(ManufacturingError::Invalid("produced qty must be positive".into()));
        }
        let wo = self.load_wo(wo_id).await?;
        if wo.status == "completed" {
            return Ok(ReceiveOutcome { finished_value: Decimal::ZERO, completed: true, already: true });
        }
        if wo.status != "in_process" {
            return Err(ManufacturingError::InvalidState("work order has no WIP to receive"));
        }
        let remaining = wo.quantity - wo.produced_qty;
        if produced_qty > remaining {
            return Err(ManufacturingError::OverProduce { producing: produced_qty, ordered: wo.quantity });
        }
        let fg = wo.fg_account_id.ok_or(ManufacturingError::MissingAccount("finished_goods"))?;
        let wip = wo.wip_account_id.ok_or(ManufacturingError::MissingAccount("wip"))?;
        let fg_wh = wo.fg_warehouse_id.ok_or(ManufacturingError::MissingAccount("fg_warehouse"))?;

        // Value of this receipt = prorated share of accumulated WIP (raw + operating).
        let wip_total = wo.raw_material_cost + wo.operating_cost;
        let is_full = produced_qty == remaining;
        let value = if is_full {
            // Clear all remaining WIP so it nets to zero, avoiding rounding residue.
            money(wip_total) - self.received_value(wo_id).await?
        } else {
            money(wip_total * produced_qty / wo.quantity)
        };

        // SIDE EFFECTS BEFORE THE GATE (mirrors consume/operate) — both are idempotent, keyed by the
        // cumulative produced qty, so a crash/error before the gate commits is safe to retry: the WO
        // is still `in_process`, the retry re-drives, and WIP always clears. Committing the completion
        // marker BEFORE the receipt would strand WIP non-zero if a side effect failed (council 2026-07-06).
        let cumulative = money(wo.produced_qty + produced_qty);
        let dedup = format!("receive:{wo_id}:{cumulative}");

        // 1) Drive inventory to receive the FG at this value (idempotent per `dedup`).
        inventory
            .receive_finished(&FinishedReceipt {
                company_id: wo.company_id,
                work_order_id: wo_id,
                warehouse_id: fg_wh,
                item_id: wo.item_id,
                quantity: produced_qty,
                value,
                idempotency_key: dedup.clone(),
            })
            .await
            .map_err(|r| ManufacturingError::Inventory(r.code))?;

        // 2) Post Dr Finished-Goods · Cr WIP (idempotent on the derived source_id).
        let env = AccountingPostEnvelope {
            idempotency_key: dedup.clone(),
            company_id: wo.company_id,
            branch_id: None,
            source_type: "manufacturing".into(),
            source_id: Uuid::new_v5(&wo_id, format!("manufacturing:{dedup}").as_bytes()),
            source_reference: Some(wo.work_order_number.clone()),
            posting_date: chrono::Utc::now().date_naive(),
            currency: "IDR".into(),
            posting_type: "original".into(),
            description: Some("finished goods receipt".into()),
            lines: vec![
                GlPostLine::debit(fg, value).with_description("Finished goods"),
                GlPostLine::credit(wip, value).with_description("WIP"),
            ],
        };
        self.post(gl, &env).await?;

        // 3) THE GATE, last: advance produced qty / complete. Concurrent double-receive → one wins;
        //    the loser's (idempotent) side effects were harmless dups.
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, wo.company_id).await?;
        let moved = self.work_orders.gate_receive(&mut tx, wo_id, produced_qty).await?;
        if moved != 1 {
            // Another receive won the race (its side effects deduped ours) — not an error.
            tx.rollback().await?;
            return Ok(ReceiveOutcome { finished_value: value, completed: true, already: true });
        }
        tx.commit().await?;

        let completed = wo.produced_qty + produced_qty >= wo.quantity;
        sink.publish(&ManufacturingEvent::FinishedGoodsReceived(FinishedGoodsReceived {
            work_order_id: wo_id,
            company_id: wo.company_id,
            item_id: wo.item_id,
            produced_qty,
            finished_value: value,
        }));
        if completed {
            sink.publish(&ManufacturingEvent::WorkOrderCompleted(WorkOrderCompleted {
                work_order_id: wo_id,
                company_id: wo.company_id,
                total_cost: money(wo.raw_material_cost + wo.operating_cost),
            }));
        }
        Ok(ReceiveOutcome { finished_value: value, completed, already: false })
    }
}
