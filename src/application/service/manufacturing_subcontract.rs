//! Subcontract: the receipt event → hidden manufacturing order mint (hand-authored, user-owned).
//!
//! An `impl ManufacturingWriteService` chunk over the vocabulary in
//! [`super::manufacturing_write_service`]. Buying publishes a receipt event when a purchase order
//! line is receipted; when — and only when — that order's `order_kind` is `subcontract`, the event
//! mints the hidden manufacturing order here: directly in `confirmed` (there is nothing to
//! schedule — the supplier already has the work), numbered by the PO reference, with the
//! subcontract BoM's components exploded as its material requirements (the materials the customer
//! supplies to the supplier).
//!
//! Zero Cargo edge to buying: the event is a serialized DTO (mirroring buying's receipt envelope
//! shape), delivered by the composing service's relay. Manufacturing NEVER writes anything on the
//! buying side, and NEVER writes stock-valuation layers — inventory moves ride the normal
//! consume/receive verbs on the minted order.
//!
//! Idempotency backstop: the link table's unique `(company_id, purchase_order_id)`. A replayed
//! receipt finds the existing link and returns the work order it minted — no second MO, no second
//! cost leg. Concurrent double-delivery collapses onto the same constraint inside the mint
//! transaction.
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on
//! `WorkOrderRepository` / `BomRepository` / `SubcontractLinkRepository`.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::infrastructure::persistence::{NewSubcontractLinkRow, NewWorkOrderItemRow, NewWorkOrderRow};

use super::manufacturing_events::*;
use super::manufacturing_write_service::{ManufacturingError, ManufacturingWriteService};

/// One receipt line of a subcontract purchase order.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubcontractReceiptLine {
    pub item_id: Uuid,
    pub quantity: Decimal,
    /// Quoted unit rate — the subcontract cost the receive path carries as its extra-cost leg.
    pub rate: Decimal,
}

/// The subcontract receipt event — mirrors buying's receipt envelope (serialized contract; zero
/// Cargo edge). Any field buying adds later is ignored here by serde's default tolerance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubcontractReceiptEvent {
    pub order_id: Uuid,
    pub company_id: Uuid,
    pub supplier_id: Uuid,
    /// Only `subcontract` mints a manufacturing order — every other kind is refused LOUDLY.
    pub order_kind: String,
    pub currency: String,
    pub reference: Option<String>,
    pub lines: Vec<SubcontractReceiptLine>,
}

impl ManufacturingWriteService {
    /// Handle a purchase-receipt event: mint the hidden subcontract MO, once per purchase order.
    ///
    /// Returns the minted (or replayed) work order id. REFUSES LOUDLY when `order_kind` is not
    /// `subcontract` ([`ManufacturingError::SubcontractKindMismatch`]) — a goods receipt must
    /// never silently mint production orders.
    pub async fn handle_subcontract_receipt(
        &self,
        event: &SubcontractReceiptEvent,
        sink: &dyn ManufacturingEventSink,
    ) -> Result<Uuid, ManufacturingError> {
        if event.order_kind != "subcontract" {
            return Err(ManufacturingError::SubcontractKindMismatch(event.order_kind.clone()));
        }
        // The replay backstop: one MO per (company, purchase order) — return the existing mint.
        if let Some(existing) = backbone_orm::company_scope::with_company_scope(
            Some(event.company_id),
            self.subcontract_links.find_by_purchase_order(&self.pool, event.company_id, event.order_id),
        )
        .await?
        {
            return Ok(existing);
        }
        let first = event
            .lines
            .first()
            .ok_or(ManufacturingError::Invalid("subcontract receipt event has no lines".into()))?;
        let quantity: Decimal = event
            .lines
            .iter()
            .filter(|l| l.item_id == first.item_id)
            .map(|l| l.quantity)
            .sum();
        if quantity <= Decimal::ZERO {
            return Err(ManufacturingError::Invalid("subcontract receipt quantity must be positive".into()));
        }

        // Resolve the item's ACTIVE BoM and insist it is a subcontract BoM — anything else means
        // the supplier's item has no subcontract recipe, which is an authoring defect, LOUD.
        let bom_id = backbone_orm::company_scope::with_company_scope(
            Some(event.company_id),
            self.boms.find_active_bom_for_item(&self.pool, event.company_id, first.item_id),
        )
        .await?
        .ok_or(ManufacturingError::Invalid(
            "subcontract receipt item has no active BoM — author one with bom_type=subcontract".into(),
        ))?;
        let bom_type = backbone_orm::company_scope::with_company_scope(
            Some(event.company_id),
            self.boms.fetch_bom_type(&self.pool, bom_id),
        )
        .await?
        .unwrap_or_else(|| "normal".into());
        if bom_type != "subcontract" {
            return Err(ManufacturingError::Invalid(
                "subcontract receipt item's active BoM is not bom_type=subcontract".into(),
            ));
        }

        // Explode the subcontract BoM: the customer-supplied materials become the MO's requirements
        // so the normal consume/receive verbs drive them (the DoD subcontract receipt = components
        // + PO value → FG, exactly the receive path's extra-cost leg).
        let mut required: Vec<(Uuid, Decimal, Decimal)> = Vec::new();
        self.explode_bom(event.company_id, bom_id, quantity, 0, &mut required).await?;

        // The hidden MO: numbered by the PO reference, directly in `confirmed`, with its link row —
        // ONE transaction. A concurrent double-delivery loses on the link's unique constraint and
        // re-reads the winner's link.
        let wo_id = Uuid::new_v4();
        let number = event
            .reference
            .clone()
            .unwrap_or_else(|| event.order_id.to_string());
        let mut tx = self.pool.begin().await?;
        backbone_orm::company_scope::bind_company_on(&mut tx, event.company_id).await?;
        let mint = self.work_orders.insert_confirmed(&mut tx, &NewWorkOrderRow {
            id: wo_id,
            company_id: event.company_id,
            work_order_number: &number,
            item_id: first.item_id,
            bom_id,
            quantity,
            product_category_id: None,
            wip_warehouse_id: None,
            fg_warehouse_id: None,
            wip_account_id: None,
            fg_account_id: None,
            raw_material_account_id: None,
            conversion_cost_account_id: None,
        })
        .await;
        if let Err(e) = mint {
            tx.rollback().await?;
            return Err(ManufacturingError::Db(e));
        }
        for (item, qty, rate) in &required {
            self.work_order_items.insert_requirement(&mut tx, &NewWorkOrderItemRow {
                id: Uuid::new_v4(),
                company_id: event.company_id,
                work_order_id: wo_id,
                item_id: *item,
                required_qty: *qty,
                rate: *rate,
            }).await?;
        }
        self.subcontract_links.insert_link(&mut tx, &NewSubcontractLinkRow {
            id: Uuid::new_v4(),
            company_id: event.company_id,
            purchase_order_id: event.order_id,
            work_order_id: wo_id,
        }).await?;
        tx.commit().await?;

        sink.publish(&ManufacturingEvent::SubcontractMoMinted(SubcontractMoMinted {
            work_order_id: wo_id,
            company_id: event.company_id,
            purchase_order_id: event.order_id,
            supplier_id: event.supplier_id,
            item_id: first.item_id,
            quantity,
        }));
        Ok(wo_id)
    }
}
