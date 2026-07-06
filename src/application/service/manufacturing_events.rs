//! Manufacturing domain events (hand-authored, user-owned) — the public extension surface.
//!
//! The Work Order lifecycle publishes these as it charges and clears WIP. A consumer (costing
//! analytics, a production dashboard) subscribes without calling back into manufacturing.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A Work Order was released — its BOM was exploded into required materials.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkOrderReleased {
    pub work_order_id: Uuid,
    pub company_id: Uuid,
    pub item_id: Uuid,
    pub quantity: Decimal,
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

/// The manufacturing domain-event union.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ManufacturingEvent {
    WorkOrderReleased(WorkOrderReleased),
    MaterialsConsumed(MaterialsConsumed),
    ConversionCharged(ConversionCharged),
    FinishedGoodsReceived(FinishedGoodsReceived),
    WorkOrderCompleted(WorkOrderCompleted),
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
