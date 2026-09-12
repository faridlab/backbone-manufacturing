//! Execution: the WIP consume + receive seam (hand-authored, user-owned).
//!
//! An `impl ManufacturingWriteService` chunk over the vocabulary in
//! [`super::manufacturing_write_service`]. A Work Order's value flows through WIP in two balanced
//! posts here (the third — `operate` — lives in [`super::manufacturing_job_card`]):
//!   consume  Dr WIP · Cr Raw-Material Stock   (materials issued; value from real inventory)
//!   receive  Dr Finished-Goods · Cr WIP        (FG = raw + operating − byproduct shares, + interim)
//! Each post is transition-gated (the status advance is the once-only guard) and carries a stable
//! idempotency key, so a retry never double-charges WIP. Manufacturing owns no stock or ledger: it
//! drives inventory (InventoryPort) for the physical moves + valuation and emits the posts (GlPostSink).
//!
//! The receive path splits the batch's value across byproduct legs by their BoM cost shares (the FG
//! line keeps the remainder, so rounding residue lands nowhere), carries the subcontract extra-cost
//! leg against the interim account, and honours the valuation posture: `average` receives at the
//! actual WIP-derived cost; `standard` receives at the pinned standard price and posts the gap to
//! the cost-variance account — the standard price is NEVER recomputed from actuals.
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on `WorkOrderRepository`
//! / `WorkOrderItemRepository`, whose gate methods take THIS service's transaction so the consume /
//! receive commits as one unit (the once-only guard).
//!
//! Tenancy (ADR-0029): the module is tenant-agnostic — no company argument exists on any verb
//! here. Transactions relay the ambient org scope (the composing service sets it per request);
//! pool reads ride the caller-scoped helpers undecorated. Wire payloads (inventory issues,
//! receipts, GL posts, events) keep a legacy company twin filled from the ambient scope's echo
//! for consumers that still read a tenant off the wire.

use rust_decimal::Decimal;
use uuid::Uuid;

use super::manufacturing_events::*;
use super::manufacturing_gl::{AccountingPostEnvelope, GlPostLine, GlPostSink};
use super::manufacturing_ports::{CostPosture, FinishedReceipt, InventoryPort, IssueLine, MaterialIssue, ReceiptByproduct};
use super::manufacturing_write_service::{
    legacy_company_echo, money, relay_ambient_scope, ConsumeOutcome, ManufacturingError,
    ManufacturingWriteService, ReceiveFinishedOrder, ReceiveOutcome,
};

impl ManufacturingWriteService {
    /// Issue all required materials to WIP: drive inventory (value from moving-average) + post
    /// `Dr WIP · Cr Raw-Material Stock`. The gate derives confirmed → progress (the once-only
    /// consume — `progress` means "materials have been issued", there is no separate start verb).
    pub async fn consume_materials(
        &self,
        wo_id: Uuid,
        raw_warehouse_id: Uuid,
        inventory: &dyn InventoryPort,
        gl: &dyn GlPostSink,
        sink: &dyn ManufacturingEventSink,
    ) -> Result<ConsumeOutcome, ManufacturingError> {
        let wo = self.load_wo(wo_id).await?;
        if wo.status == "progress" || wo.status == "done" {
            return Ok(ConsumeOutcome { raw_material_value: wo.raw_material_cost, already: true });
        }
        if wo.status != "confirmed" {
            return Err(ManufacturingError::InvalidState("work order is not confirmed"));
        }
        let wip = self
            .resolve_account("wip", wo.wip_account_id, wo.product_category_id, |d| d.wip_account_id)
            .await?;
        let raw_acct = self
            .resolve_account("raw_material", wo.raw_material_account_id, wo.product_category_id, |d| {
                d.raw_material_account_id
            })
            .await?;

        // The line read rides the caller-scoped helper (ADR-0029): under the composed decorator
        // the org fence scopes the requirements to the caller's unit on the request-dedicated
        // connection; undecorated (module tests) the read runs plain.
        let items = self.work_order_items.list_requirements(&self.pool, wo_id).await?;
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

        // Legacy company twin (ADR-0029): filled from the ambient org scope's company echo for the
        // wire consumers below (inventory issue, GL post, event) that still read a tenant off the
        // wire. No module statement keys on it.
        let legacy_company = legacy_company_echo();

        // Drive real inventory to remove the components and tell us what they were worth.
        let issue = MaterialIssue {
            company_id: legacy_company,
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
            company_id: legacy_company,
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

        // Record consumption + advance state, gated on confirmed → progress.
        let mut tx = self.pool.begin().await?;
        // The ambient org scope — the consume gate rides the composed decorator's fence
        // (ADR-0029). Undecorated, the tx stays plain.
        relay_ambient_scope(&mut tx).await?;
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
            company_id: legacy_company,
            raw_material_value: raw_value,
        }));
        Ok(ConsumeOutcome { raw_material_value: raw_value, already: false })
    }

    /// Receive finished goods into stock and post the receipt. Gated on the produced-quantity
    /// advance (idempotent per cumulative receipt); on full receipt WIP nets to zero and the WO
    /// derives `done`.
    ///
    /// The batch's total value T splits three ways:
    ///   - byproduct legs: T × cost_share / 100 each (their BoM rows carry the share; the FG line
    ///     keeps the REMAINDER, so the split always sums to T exactly);
    ///   - the FG line: the remainder;
    ///   - under the standard posture T is `qty × standard_unit_price` (pinned, never recomputed)
    ///     and the gap to the actual WIP+extra posts to the cost-variance account as a plug.
    /// The subcontract extra cost (PO quoted value prorated by receipt share) posts on the credit
    /// side against the interim account.
    pub async fn receive_finished(
        &self,
        wo_id: Uuid,
        order: ReceiveFinishedOrder,
        inventory: &dyn InventoryPort,
        gl: &dyn GlPostSink,
        sink: &dyn ManufacturingEventSink,
    ) -> Result<ReceiveOutcome, ManufacturingError> {
        let produced_qty = order.produced_qty;
        if produced_qty <= Decimal::ZERO {
            return Err(ManufacturingError::Invalid("produced qty must be positive".into()));
        }
        let wo = self.load_wo(wo_id).await?;
        if wo.status == "done" {
            return Ok(ReceiveOutcome {
                finished_value: Decimal::ZERO,
                byproduct_value: Decimal::ZERO,
                extra_cost: Decimal::ZERO,
                completed: true,
                already: true,
            });
        }
        // A partial receipt leaves the order `to_close` — it stays receivable so the
        // accumulator can complete the order on a later receipt (only done refuses).
        if wo.status != "progress" && wo.status != "to_close" {
            return Err(ManufacturingError::InvalidState("work order has no WIP to receive"));
        }
        let remaining = wo.quantity - wo.produced_qty;
        if produced_qty > remaining {
            return Err(ManufacturingError::OverProduce { producing: produced_qty, ordered: wo.quantity });
        }
        let fg = self
            .resolve_account("finished_goods", wo.fg_account_id, wo.product_category_id, |d| d.fg_account_id)
            .await?;
        let wip = self
            .resolve_account("wip", wo.wip_account_id, wo.product_category_id, |d| d.wip_account_id)
            .await?;
        let fg_wh = wo.fg_warehouse_id.ok_or(ManufacturingError::MissingAccount("fg_warehouse"))?;
        let extra = order.extra_cost.unwrap_or(Decimal::ZERO);
        let interim = if extra > Decimal::ZERO {
            Some(
                self.resolve_account(
                    "subcontract_interim",
                    None,
                    wo.product_category_id,
                    |d| d.subcontract_interim_account_id,
                )
                .await?,
            )
        } else {
            None
        };

        // BoM byproduct legs + their cost shares; the family may not carry off more than the batch.
        // The read rides the caller-scoped helper (ADR-0029) — the fence decides under composition.
        let bom_byproducts = self.bom_byproducts.find_by_bom(&self.pool, wo.bom_id).await?;
        let mut share_sum = Decimal::ZERO;
        for b in &bom_byproducts {
            share_sum += b.cost_share;
        }
        if share_sum > Decimal::from(100) {
            return Err(ManufacturingError::CostShareOverflow { sum: share_sum });
        }

        // Value of this receipt's actual WIP = prorated share of accumulated (raw + operating).
        let wip_total = wo.raw_material_cost + wo.operating_cost;
        let is_full = produced_qty == remaining;
        let wip_value = if is_full {
            // Clear all remaining WIP so it nets to zero, avoiding rounding residue.
            money(wip_total) - self.received_value(wo_id).await?
        } else {
            money(wip_total * produced_qty / wo.quantity)
        };

        // The receipt's total value under the chosen posture.
        let total = match order.cost_posture {
            CostPosture::Average => wip_value + extra,
            CostPosture::Standard => {
                let std_unit = order.standard_unit_price.ok_or_else(|| {
                    ManufacturingError::Invalid("standard posture requires a standard unit price".into())
                })?;
                money(produced_qty * std_unit)
            }
        };

        // Split the total across the byproduct legs by cost share; FG keeps the remainder.
        let mut legs: Vec<(Uuid, Uuid, Decimal, Decimal)> = Vec::new(); // (item, account, qty, value)
        let mut byproduct_value = Decimal::ZERO;
        for line in &order.byproducts {
            let bom_leg = bom_byproducts
                .iter()
                .find(|b| b.item_id == line.item_id)
                .ok_or_else(|| ManufacturingError::Invalid("byproduct line has no BoM cost share".into()))?;
            let value = money(total * bom_leg.cost_share / Decimal::from(100));
            // The byproduct lands in finished stock beside the FG, so its stock account rides the
            // same resolution chain (per-order override first, per ADR-003); the byproduct row's
            // own category — when authored — is a more specific category hop than the order's
            // snapshot, so it takes precedence within that hop.
            let account = self
                .resolve_account(
                    "finished_goods",
                    wo.fg_account_id,
                    bom_leg.product_category_id.or(wo.product_category_id),
                    |d| d.fg_account_id,
                )
                .await?;
            byproduct_value += value;
            legs.push((line.item_id, account, line.quantity, value));
        }
        let fg_value = total - byproduct_value; // remainder — the split sums to `total` exactly

        // Standard-posture plug: the gap between the pinned standard total and the actual costs.
        let mut variance_line: Option<(Uuid, Decimal, bool)> = None; // (account, amount, is_debit)
        if order.cost_posture == CostPosture::Standard {
            let diff = total - (wip_value + extra);
            if diff != Decimal::ZERO {
                let variance = self
                    .resolve_account(
                        "cost_variance",
                        None,
                        wo.product_category_id,
                        |d| d.cost_variance_account_id,
                    )
                    .await?;
                // diff > 0: production came in under standard → CREDIT variance (favourable).
                // diff < 0: over standard → DEBIT variance (unfavourable).
                variance_line = Some((
                    variance,
                    diff.abs(),
                    diff < Decimal::ZERO,
                ));
            }
        }

        // Legacy company twin (ADR-0029): filled from the ambient org scope's company echo for the
        // wire consumers below (receipt, GL post, events) that still read a tenant off the wire.
        // No module statement keys on it.
        let legacy_company = legacy_company_echo();

        // SIDE EFFECTS BEFORE THE GATE (mirrors consume/operate) — both are idempotent, keyed by the
        // cumulative produced qty, so a crash/error before the gate commits is safe to retry: the WO
        // is still `progress`, the retry re-drives, and WIP always clears. Committing the completion
        // marker BEFORE the receipt would strand WIP non-zero if a side effect failed (council 2026-07-06).
        let cumulative = money(wo.produced_qty + produced_qty);
        let dedup = format!("receive:{wo_id}:{cumulative}");

        // 1) Drive inventory to receive the FG + byproduct legs at these values (idempotent per `dedup`).
        inventory
            .receive_finished(&FinishedReceipt {
                company_id: legacy_company,
                work_order_id: wo_id,
                warehouse_id: fg_wh,
                item_id: wo.item_id,
                quantity: produced_qty,
                value: fg_value,
                byproducts: legs
                    .iter()
                    .map(|(item, _acct, qty, value)| ReceiptByproduct {
                        item_id: *item,
                        quantity: *qty,
                        value: *value,
                    })
                    .collect(),
                extra_cost: if extra > Decimal::ZERO { Some(extra) } else { None },
                cost_posture: order.cost_posture,
                standard_unit_price: order.standard_unit_price,
                idempotency_key: dedup.clone(),
            })
            .await
            .map_err(|r| ManufacturingError::Inventory(r.code))?;

        // 2) Post the receipt: Dr FG (+ byproduct legs) (+ variance plug) · Cr WIP (+ interim).
        let mut lines = vec![GlPostLine::debit(fg, fg_value).with_description("Finished goods")];
        for (item, account, _qty, value) in &legs {
            lines.push(GlPostLine::debit(*account, *value).with_description("Byproduct"));
            let _ = item;
        }
        if let Some((account, amount, is_debit)) = variance_line {
            if is_debit {
                lines.push(GlPostLine::debit(account, amount).with_description("Cost variance (unfavourable)"));
            } else {
                lines.push(GlPostLine::credit(account, amount).with_description("Cost variance (favourable)"));
            }
        }
        lines.push(GlPostLine::credit(wip, wip_value).with_description("WIP"));
        if let Some(interim) = interim {
            lines.push(GlPostLine::credit(interim, extra).with_description("Subcontract interim"));
        }
        let env = AccountingPostEnvelope {
            idempotency_key: dedup.clone(),
            company_id: legacy_company,
            branch_id: None,
            source_type: "manufacturing".into(),
            source_id: Uuid::new_v5(&wo_id, format!("manufacturing:{dedup}").as_bytes()),
            source_reference: Some(wo.work_order_number.clone()),
            posting_date: chrono::Utc::now().date_naive(),
            currency: "IDR".into(),
            posting_type: "original".into(),
            description: Some("finished goods receipt".into()),
            lines,
        };
        self.post(gl, &env).await?;

        // 3) THE GATE, last: advance produced qty / derive to_close|done. Concurrent double-receive →
        //    one wins; the loser's (idempotent) side effects were harmless dups.
        let mut tx = self.pool.begin().await?;
        // The ambient org scope — the receive gate rides the composed decorator's fence
        // (ADR-0029). Undecorated, the tx stays plain.
        relay_ambient_scope(&mut tx).await?;
        let moved = self.work_orders.gate_receive(&mut tx, wo_id, produced_qty).await?;
        if moved != 1 {
            // Another receive won the race (its side effects deduped ours) — not an error.
            tx.rollback().await?;
            return Ok(ReceiveOutcome {
                finished_value: fg_value,
                byproduct_value,
                extra_cost: extra,
                completed: true,
                already: true,
            });
        }
        tx.commit().await?;

        let completed = wo.produced_qty + produced_qty >= wo.quantity;
        sink.publish(&ManufacturingEvent::FinishedGoodsReceived(FinishedGoodsReceived {
            work_order_id: wo_id,
            company_id: legacy_company,
            item_id: wo.item_id,
            produced_qty,
            finished_value: fg_value,
        }));
        if completed {
            sink.publish(&ManufacturingEvent::WorkOrderCompleted(WorkOrderCompleted {
                work_order_id: wo_id,
                company_id: legacy_company,
                total_cost: money(wo.raw_material_cost + wo.operating_cost),
            }));
        }
        Ok(ReceiveOutcome {
            finished_value: fg_value,
            byproduct_value,
            extra_cost: extra,
            completed,
            already: false,
        })
    }
}
