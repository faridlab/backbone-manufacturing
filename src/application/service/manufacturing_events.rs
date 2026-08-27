//! Manufacturing domain events (hand-authored, user-owned) — the public extension surface.
//!
//! The Work Order lifecycle publishes these as it charges and clears WIP. A consumer (costing
//! analytics, a production dashboard) subscribes without calling back into manufacturing.
//!
//! State vocabulary note: the confirm verb (and its `WorkOrderConfirmed` event) replaced the old
//! release wording — `confirmed` is one of the three hand-gated direct writes; `progress` /
//! `to_close` / `done` are derived by the consume / receive gates.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A Work Order was confirmed — its BOM was exploded into required materials.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkOrderConfirmed {
    pub work_order_id: Uuid,
    pub company_id: Uuid,
    pub item_id: Uuid,
    pub quantity: Decimal,
}

/// A Work Order was cancelled (from draft or confirmed — before any WIP existed).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkOrderCancelled {
    pub work_order_id: Uuid,
    pub company_id: Uuid,
}

/// Materials were issued to WIP (the consume post: Dr WIP · Cr Raw-Material Stock).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MaterialsConsumed {
    pub work_order_id: Uuid,
    pub company_id: Uuid,
    pub raw_material_value: Decimal,
}

/// A job card's conversion cost was charged to WIP (Dr WIP · Cr Conversion-Applied).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversionCharged {
    pub job_card_id: Uuid,
    pub work_order_id: Uuid,
    pub company_id: Uuid,
    pub operating_cost: Decimal,
}

/// Finished goods were received (the receive post: Dr Finished-Goods · Cr WIP).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FinishedGoodsReceived {
    pub work_order_id: Uuid,
    pub company_id: Uuid,
    pub item_id: Uuid,
    pub produced_qty: Decimal,
    pub finished_value: Decimal,
}

/// A Work Order was fully produced (WIP cleared to zero).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkOrderCompleted {
    pub work_order_id: Uuid,
    pub company_id: Uuid,
    pub total_cost: Decimal,
}

/// A done order's output was disassembled back into components (the unbuild reversal).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnbuildExecuted {
    pub unbuild_order_id: Uuid,
    pub source_work_order_id: Uuid,
    pub company_id: Uuid,
    pub item_id: Uuid,
    pub quantity: Decimal,
    pub reversed_value: Decimal,
}

/// A repair order ended — its part legs settled and the asset's stock restored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepairCompleted {
    pub repair_order_id: Uuid,
    pub company_id: Uuid,
    pub item_id: Uuid,
    pub repair_expense: Decimal,
    pub inventory_loss: Decimal,
    pub recovered_value: Decimal,
}

/// A subcontract receipt minted its hidden manufacturing order (directly in `confirmed`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubcontractMoMinted {
    pub work_order_id: Uuid,
    pub company_id: Uuid,
    pub purchase_order_id: Uuid,
    pub supplier_id: Uuid,
    pub item_id: Uuid,
    pub quantity: Decimal,
}

/// The manufacturing domain-event union.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ManufacturingEvent {
    WorkOrderConfirmed(WorkOrderConfirmed),
    WorkOrderCancelled(WorkOrderCancelled),
    MaterialsConsumed(MaterialsConsumed),
    ConversionCharged(ConversionCharged),
    FinishedGoodsReceived(FinishedGoodsReceived),
    WorkOrderCompleted(WorkOrderCompleted),
    UnbuildExecuted(UnbuildExecuted),
    RepairCompleted(RepairCompleted),
    SubcontractMoMinted(SubcontractMoMinted),
}

/// Sink the write path publishes to. A consuming service supplies its own (bus, outbox, …).
pub trait ManufacturingEventSink: Send + Sync {
    fn publish(&self, event: &ManufacturingEvent);
}

/// A no-op/logging sink for tests and single-process composition.
#[derive(Debug, Default, Clone)]
pub struct LoggingSink;

impl ManufacturingEventSink for LoggingSink {
    fn publish(&self, event: &ManufacturingEvent) {
        tracing::info!(?event, "manufacturing event");
    }
}
