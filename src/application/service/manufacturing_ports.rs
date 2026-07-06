//! Manufacturing's inventory seam (hand-authored, user-owned).
//!
//! Manufacturing owns no stock. It drives `backbone-inventory` to move quantity and to VALUE the
//! materials it consumes — the moving-average cost of the issued components is inventory's number,
//! not a made-up one. Manufacturing holds only the `InventoryPort` trait; a composing service (and
//! the seam test) wires it over the real inventory write path. **Zero normal Cargo edge** to inventory.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A request to issue components out of a raw warehouse into WIP.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MaterialIssue {
    pub company_id: Uuid,
    pub work_order_id: Uuid,
    pub warehouse_id: Uuid,
    /// Stable dedup key. A retry of the SAME issue (a crash/error mid-saga) must NOT move stock twice —
    /// the inventory implementation returns the prior result for a repeated key.
    pub idempotency_key: String,
    pub lines: Vec<IssueLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IssueLine {
    pub item_id: Uuid,
    pub quantity: Decimal,
}

/// The valued result of an issue — inventory reports the rate + value it removed for each line.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IssueAck {
    pub total_value: Decimal,
    pub lines: Vec<IssuedLineValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IssuedLineValue {
    pub item_id: Uuid,
    pub quantity: Decimal,
    pub rate: Decimal,
    pub value: Decimal,
}

/// A request to receive finished goods into an FG warehouse at the computed unit cost.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FinishedReceipt {
    pub company_id: Uuid,
    pub work_order_id: Uuid,
    pub warehouse_id: Uuid,
    pub item_id: Uuid,
    pub quantity: Decimal,
    /// Total value of the received quantity (raw consumed + operating, prorated).
    pub value: Decimal,
    /// Stable dedup key. A retry of the SAME receipt must NOT add stock twice.
    pub idempotency_key: String,
}

/// Inventory's rejection of an issue/receipt (e.g. insufficient stock).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InventoryRejected {
    pub code: String,
    pub message: String,
}

/// The inventory seam manufacturing drives.
#[async_trait::async_trait]
pub trait InventoryPort: Send + Sync {
    /// Remove components from stock (into WIP) and return what they were worth.
    async fn issue_to_wip(&self, req: &MaterialIssue) -> Result<IssueAck, InventoryRejected>;
    /// Add finished goods to stock at the given value.
    async fn receive_finished(&self, req: &FinishedReceipt) -> Result<(), InventoryRejected>;
}
