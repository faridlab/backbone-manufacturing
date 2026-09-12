//! Work-order creation + confirm + cancel (hand-authored, user-owned).
//!
//! An `impl ManufacturingWriteService` chunk over the vocabulary in
//! [`super::manufacturing_write_service`]: open a draft Work Order, then confirm it by exploding its
//! BOM into required materials (recursing through phantom sub-assemblies to their own components) and
//! drafting → confirming in one transaction. The explosion is deterministic + static — no MRP.
//!
//! Confirm refuses kit and subcontract BoMs LOUDLY: a phantom kit NEVER gets its own work order (it
//! explodes through to components at demand time), and a subcontract order is minted only by the
//! subcontract receipt event (hidden, directly in `confirmed`).
//!
//! Cancel is a direct write from draft|confirmed only: an order that has consumed materials
//! (progress) or received goods (to_close/done) carries WIP and stock a state flip cannot unwind —
//! the refusal is LOUD, never a silent unwind.
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on `WorkOrderRepository`
//! / `WorkOrderItemRepository` / `BomRepository` / `BomItemRepository`, whose methods take THIS
//! service's transaction (or the request-dedicated connection) so the confirm is atomic.
//!
//! Tenancy (ADR-0029): the module is tenant-agnostic — no company argument exists on any verb
//! here. Transactions relay the ambient org scope (the composing service sets it per request);
//! pool reads ride the caller-scoped helpers undecorated. Event payloads keep a legacy company
//! twin filled from the ambient scope's echo.

use rust_decimal::Decimal;
use uuid::Uuid;

use crate::infrastructure::persistence::{NewWorkOrderItemRow, NewWorkOrderRow};

use super::manufacturing_events::*;
use super::manufacturing_write_service::{
    is_dup, legacy_company_echo, relay_ambient_scope, ManufacturingError, ManufacturingWriteService,
    NewWorkOrder,
};

impl ManufacturingWriteService {
    /// Create a draft Work Order.
    pub async fn create_work_order(&self, o: NewWorkOrder) -> Result<Uuid, ManufacturingError> {
        if o.quantity <= Decimal::ZERO {
            return Err(ManufacturingError::Invalid("work order quantity must be positive".into()));
        }
        let id = Uuid::new_v4();
        // The pool insert rides `org_scope::execute_scoped` — the ambient org scope (the
        // composing service sets it per request) makes the decorator's fence see it (ADR-0029).
        let r = self.work_orders.insert_draft(&self.pool, &NewWorkOrderRow {
            id,
            work_order_number: &o.work_order_number,
            item_id: o.item_id,
            bom_id: o.bom_id,
            quantity: o.quantity,
            product_category_id: o.product_category_id,
            wip_warehouse_id: o.wip_warehouse_id,
            fg_warehouse_id: o.fg_warehouse_id,
            wip_account_id: o.wip_account_id,
            fg_account_id: o.fg_account_id,
            raw_material_account_id: o.raw_material_account_id,
            conversion_cost_account_id: o.conversion_cost_account_id,
        })
        .await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { ManufacturingError::DuplicateNumber(o.work_order_number) } else { e.into() });
        }
        Ok(id)
    }

    /// Confirm a draft Work Order: explode its BOM into required materials, draft → confirmed.
    pub async fn confirm_work_order(
        &self,
        wo_id: Uuid,
        sink: &dyn ManufacturingEventSink,
    ) -> Result<(), ManufacturingError> {
        // ID-only pattern (ADR-0029): identified by the work-order id alone. Read the header
        // first through the scoped helper (it rides the REQUEST-dedicated connection — under the
        // composed decorator the org fence hides another unit's work order). The once-only guard
        // is unaffected: it remains the in-transaction draft→confirmed gate.
        let wo = self.work_orders.find_confirm_source(&self.pool, wo_id).await?
            .ok_or(ManufacturingError::NotFound("work order"))?;
        if wo.status != "draft" {
            return Err(ManufacturingError::InvalidState("work order is not draft"));
        }
        let item_id: Uuid = wo.item_id;
        let bom_id: Uuid = wo.bom_id;
        let wo_qty: Decimal = wo.quantity;

        // A kit NEVER mints a work order (it explodes through to components at demand time), and a
        // subcontract BoM's orders are minted ONLY by the receipt event — never by hand.
        let bom_type = self.boms.fetch_bom_type(&self.pool, bom_id).await?
            .ok_or(ManufacturingError::NotFound("bom"))?;
        if bom_type == "kit" {
            return Err(ManufacturingError::Invalid(
                "a kit BoM never gets its own work order — it explodes through to components at demand time".into(),
            ));
        }
        if bom_type == "subcontract" {
            return Err(ManufacturingError::Invalid(
                "a subcontract BoM's work orders are minted only by the subcontract receipt event".into(),
            ));
        }

        // Explode the BOM into required materials, recursing THROUGH phantom sub-assemblies to their
        // own components (a phantom is never stocked). Deterministic + static — no MRP.
        let mut required: Vec<(Uuid, Decimal, Decimal)> = Vec::new();
        self.explode_bom(bom_id, wo_qty, 0, &mut required).await?;

        // Gate the explosion on the draft→confirmed transition (once-only).
        let mut tx = self.pool.begin().await?;
        // The ambient org scope — the confirm tx rides the composed decorator's fence
        // (ADR-0029). Undecorated, the tx stays plain.
        relay_ambient_scope(&mut tx).await?;
        let moved = self.work_orders.gate_confirm(&mut tx, wo_id).await?;
        if moved != 1 {
            return Err(ManufacturingError::InvalidState("work order is not draft"));
        }

        for (item, qty, rate) in &required {
            self.work_order_items.insert_requirement(&mut tx, &NewWorkOrderItemRow {
                id: Uuid::new_v4(),
                work_order_id: wo_id,
                item_id: *item,
                required_qty: *qty,
                rate: *rate,
            }).await?;
        }
        tx.commit().await?;
        sink.publish(&ManufacturingEvent::WorkOrderConfirmed(WorkOrderConfirmed {
            work_order_id: wo_id,
            company_id: legacy_company_echo(),
            item_id,
            quantity: wo_qty,
        }));
        Ok(())
    }

    /// Cancel a Work Order: draft|confirmed → cancel (direct write, terminal, sticky).
    ///
    /// An order that has consumed materials (progress) or received goods (to_close/done) carries
    /// WIP and stock that a state flip cannot unwind — the refusal is LOUD (InvalidState), never
    /// a silent unwind. Done is terminal for the same reason.
    pub async fn cancel_work_order(
        &self,
        wo_id: Uuid,
        sink: &dyn ManufacturingEventSink,
    ) -> Result<(), ManufacturingError> {
        let wo = self.work_orders.find_confirm_source(&self.pool, wo_id).await?
            .ok_or(ManufacturingError::NotFound("work order"))?;
        match wo.status.as_str() {
            "draft" | "confirmed" => {}
            "progress" => return Err(ManufacturingError::InvalidState(
                "work order is in progress — materials already consumed to WIP cannot be cancelled, only completed",
            )),
            "to_close" | "done" => return Err(ManufacturingError::InvalidState(
                "work order has produced goods — it can no longer be cancelled",
            )),
            "cancel" => return Err(ManufacturingError::InvalidState("work order is already cancelled")),
            _ => return Err(ManufacturingError::InvalidState("work order state does not allow cancel")),
        }

        let mut tx = self.pool.begin().await?;
        // The ambient org scope — the cancel tx rides the composed decorator's fence
        // (ADR-0029). Undecorated, the tx stays plain.
        relay_ambient_scope(&mut tx).await?;
        let moved = self.work_orders.gate_cancel(&mut tx, wo_id).await?;
        if moved != 1 {
            return Err(ManufacturingError::InvalidState(
                "work order has left the cancellable states (draft/confirmed)",
            ));
        }
        tx.commit().await?;
        sink.publish(&ManufacturingEvent::WorkOrderCancelled(WorkOrderCancelled {
            work_order_id: wo_id,
            company_id: legacy_company_echo(),
        }));
        Ok(())
    }

    /// Write the reservation_state projection (waiting | confirmed | assigned) — the ONLY surface
    /// that may write it. Availability is expressed ONLY here: no field named `availability`
    /// exists anywhere in the module, work-order verbs never read this column into a decision,
    /// and job-card `blocked` is derived from it on the read side.
    pub async fn write_reservation(
        &self,
        wo_id: Uuid,
        state: crate::domain::entity::ReservationState,
    ) -> Result<(), ManufacturingError> {
        // Idempotent by value: rewriting the same state is a no-op.
        self.work_orders.write_reservation_state(&self.pool, wo_id, state).await?;
        Ok(())
    }

    /// Recursively flatten a BOM into leaf material requirements for `want_units` of its output.
    /// A **phantom** component is never issued — it is exploded through to its own BOM's components
    /// (resolved by the phantom item's active BOM). Real components accumulate as `(item, qty, rate)`.
    /// A depth cap guards against a mis-authored phantom cycle.
    pub(super) fn explode_bom<'a>(
        &'a self,
        bom_id: Uuid,
        want_units: Decimal,
        depth: u32,
        out: &'a mut Vec<(Uuid, Decimal, Decimal)>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ManufacturingError>> + Send + 'a>>
    {
        Box::pin(async move {
            if depth > 8 {
                return Err(ManufacturingError::Invalid("phantom BOM nesting too deep (cycle?)".into()));
            }
            // ID-only (ADR-0029): each read rides the caller-scoped helper — under the composed
            // decorator the org fence scopes the explosion to the caller's unit; undecorated
            // (module tests) the reads run plain.
            let base: Decimal = self
                .boms
                .fetch_output_quantity(&self.pool, bom_id)
                .await?
                .ok_or(ManufacturingError::NotFound("bom"))?;

            let comps = self.bom_items.list_components(&self.pool, bom_id).await?;

            for c in &comps {
                let needed = c.quantity * want_units / base;
                if c.is_phantom {
                    // Resolve the phantom item's own BOM (default first) and explode through it.
                    let child_bom: Uuid = self
                        .boms
                        .find_active_bom_for_item(&self.pool, c.item_id)
                        .await?
                        .ok_or(ManufacturingError::Invalid("phantom component has no BOM".into()))?;
                    self.explode_bom(child_bom, needed, depth + 1, out).await?;
                } else {
                    out.push((c.item_id, needed, c.rate));
                }
            }
            Ok(())
        })
    }
}
