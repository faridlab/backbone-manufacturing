//! Work-order creation + release (hand-authored, user-owned).
//!
//! An `impl ManufacturingWriteService` chunk over the vocabulary in
//! [`super::manufacturing_write_service`]: open a draft Work Order, then release it by exploding its
//! BOM into required materials (recursing through phantom sub-assemblies to their own components) and
//! drafting → releasing in one transaction. The explosion is deterministic + static — no MRP.
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on `WorkOrderRepository`
//! / `WorkOrderItemRepository` / `BomRepository` / `BomItemRepository`, whose methods take THIS
//! service's transaction (or the request-dedicated connection) so the release is fenced + atomic.

use backbone_orm::company_scope;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::infrastructure::persistence::{NewWorkOrderItemRow, NewWorkOrderRow};

use super::manufacturing_events::*;
use super::manufacturing_write_service::{
    is_dup, ManufacturingError, ManufacturingWriteService, NewWorkOrder,
};

impl ManufacturingWriteService {
    /// Create a draft Work Order.
    pub async fn create_work_order(&self, o: NewWorkOrder) -> Result<Uuid, ManufacturingError> {
        if o.quantity <= Decimal::ZERO {
            return Err(ManufacturingError::Invalid("work order quantity must be positive".into()));
        }
        let id = Uuid::new_v4();
        // RLS scope (ADR-0008): company on the DTO — scope the insert so it passes the WITH CHECK fence.
        let r = company_scope::with_company_scope(
            Some(o.company_id),
            self.work_orders.insert_draft(&self.pool, &NewWorkOrderRow {
                id,
                company_id: o.company_id,
                work_order_number: &o.work_order_number,
                item_id: o.item_id,
                bom_id: o.bom_id,
                quantity: o.quantity,
                wip_warehouse_id: o.wip_warehouse_id,
                fg_warehouse_id: o.fg_warehouse_id,
                wip_account_id: o.wip_account_id,
                fg_account_id: o.fg_account_id,
                raw_material_account_id: o.raw_material_account_id,
                conversion_cost_account_id: o.conversion_cost_account_id,
            }),
        )
        .await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { ManufacturingError::DuplicateNumber(o.work_order_number) } else { e.into() });
        }
        Ok(id)
    }

    /// Release a draft Work Order: explode its BOM into required materials, draft → released.
    pub async fn release_work_order(
        &self,
        wo_id: Uuid,
        sink: &dyn ManufacturingEventSink,
    ) -> Result<(), ManufacturingError> {
        // RLS scope (ADR-0008), ID-only pattern: identified by the work-order id alone, so there is no
        // company to bind before the transaction opens. Read the header first through the scoped helper
        // (it rides the REQUEST-dedicated connection carrying the caller's `app.company_id`, so another
        // company's work order simply isn't found), then bind ITS company onto the transaction below.
        // The once-only guard is unaffected: it remains the in-transaction draft→released gate.
        let wo = self.work_orders.find_release_source(&self.pool, wo_id).await?
            .ok_or(ManufacturingError::NotFound("work order"))?;
        if wo.status != "draft" {
            return Err(ManufacturingError::InvalidState("work order is not draft"));
        }
        let company_id: Uuid = wo.company_id;
        let item_id: Uuid = wo.item_id;
        let bom_id: Uuid = wo.bom_id;
        let wo_qty: Decimal = wo.quantity;

        // Explode the BOM into required materials, recursing THROUGH phantom sub-assemblies to their
        // own components (a phantom is never stocked). Deterministic + static — no MRP.
        let mut required: Vec<(Uuid, Decimal, Decimal)> = Vec::new();
        self.explode_bom(company_id, bom_id, wo_qty, 0, &mut required).await?;

        // Gate the explosion on the draft→released transition (once-only).
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company_id).await?;
        let moved = self.work_orders.gate_release(&mut tx, wo_id).await?;
        if moved != 1 {
            return Err(ManufacturingError::InvalidState("work order is not draft"));
        }

        for (item, qty, rate) in &required {
            self.work_order_items.insert_requirement(&mut tx, &NewWorkOrderItemRow {
                id: Uuid::new_v4(),
                company_id,
                work_order_id: wo_id,
                item_id: *item,
                required_qty: *qty,
                rate: *rate,
            }).await?;
        }
        tx.commit().await?;
        sink.publish(&ManufacturingEvent::WorkOrderReleased(WorkOrderReleased {
            work_order_id: wo_id,
            company_id,
            item_id,
            quantity: wo_qty,
        }));
        Ok(())
    }

    /// Recursively flatten a BOM into leaf material requirements for `want_units` of its output.
    /// A **phantom** component is never issued — it is exploded through to its own BOM's components
    /// (resolved by the phantom item's active BOM). Real components accumulate as `(item, qty, rate)`.
    /// A depth cap guards against a mis-authored phantom cycle.
    fn explode_bom<'a>(
        &'a self,
        company_id: Uuid,
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
            // RLS scope (ADR-0008): the company is on the parameter — bind it around each read so the
            // explosion is fenced even when driven by a non-request caller (job / event subscriber).
            let base: Decimal = company_scope::with_company_scope(
                Some(company_id),
                self.boms.fetch_output_quantity(&self.pool, bom_id),
            )
            .await?
            .ok_or(ManufacturingError::NotFound("bom"))?;

            let comps = company_scope::with_company_scope(
                Some(company_id),
                self.bom_items.list_components(&self.pool, bom_id),
            )
            .await?;

            for c in &comps {
                let needed = c.quantity * want_units / base;
                if c.is_phantom {
                    // Resolve the phantom item's own BOM (default first) and explode through it.
                    let child_bom: Uuid = company_scope::with_company_scope(
                        Some(company_id),
                        self.boms.find_active_bom_for_item(&self.pool, company_id, c.item_id),
                    )
                    .await?
                    .ok_or(ManufacturingError::Invalid("phantom component has no BOM".into()))?;
                    self.explode_bom(company_id, child_bom, needed, depth + 1, out).await?;
                } else {
                    out.push((c.item_id, needed, c.rate));
                }
            }
            Ok(())
        })
    }
}
