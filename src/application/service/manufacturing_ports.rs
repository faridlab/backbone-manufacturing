//! Manufacturing's inventory seam (hand-authored, user-owned).
//!
//! Manufacturing owns no stock. It drives `backbone-inventory` to move quantity and to VALUE the
//! materials it consumes — the moving-average cost of the issued components is inventory's number,
//! not a made-up one. Manufacturing holds only the `InventoryPort` trait; a composing service (and
//! the seam test) wires it over the real inventory write path. **Zero normal Cargo edge** to inventory.
//!
//! Byproducts ride the finished receipt (their own item + quantity + the value the cost-share split
//! carried off), the unbuild path reverses a done order's output through `reverse_production`, and
//! repair part legs move through `execute_repair_leg` — every call idempotent by its key, so a
//! retry of the same saga step never moves stock twice.

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

/// One byproduct leg of a finished receipt: its own item, received quantity, and the value the
/// cost-share split carried off the batch (the FG line keeps the remainder).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReceiptByproduct {
    pub item_id: Uuid,
    pub quantity: Decimal,
    pub value: Decimal,
}

/// How the FG line is valued on receipt.
///
/// `average` (the default) receives at the actual WIP-derived cost — the estate's average moves.
/// `standard` receives at the item's standard price WITHOUT re-deriving it and posts any gap to
/// the cost-variance account (a plug; a nonzero plug with no variance account configured is a
/// LOUD MissingAccount, never a silent recompute).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum CostPosture {
    #[default]
    Average,
    Standard,
}

/// A request to receive finished goods into an FG warehouse at the computed unit cost.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FinishedReceipt {
    pub company_id: Uuid,
    pub work_order_id: Uuid,
    pub warehouse_id: Uuid,
    pub item_id: Uuid,
    pub quantity: Decimal,
    /// Total value of the received quantity — actual WIP-derived (average posture) or the
    /// standard-price total (standard posture).
    pub value: Decimal,
    /// Byproduct legs received alongside the FG line, each already valued by the cost-share split.
    pub byproducts: Vec<ReceiptByproduct>,
    /// The subcontract extra-cost leg (PO quoted value prorated by receipt share); zero/None for
    /// in-house orders. Posted against the subcontract-interim account on the credit side.
    pub extra_cost: Option<Decimal>,
    /// Valuation posture of the FG line.
    pub cost_posture: CostPosture,
    /// Standard unit price — required by the Standard posture, ignored by Average.
    pub standard_unit_price: Option<Decimal>,
    /// Stable dedup key. A retry of the SAME receipt must NOT add stock twice.
    pub idempotency_key: String,
}

/// A request to reverse a done order's finished goods back into components (an unbuild).
///
/// One port call moves EVERYTHING: the FG line OUT of stock and the component lines back IN at
/// the reversal's recovered value — never a reverse work order.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnbuildReversal {
    pub company_id: Uuid,
    pub unbuild_order_id: Uuid,
    pub source_work_order_id: Uuid,
    /// Warehouse the finished good leaves.
    pub fg_warehouse_id: Uuid,
    pub item_id: Uuid,
    pub quantity: Decimal,
    /// Value reversed off the finished-good estate.
    pub value: Decimal,
    /// Warehouse the components return to.
    pub raw_warehouse_id: Uuid,
    /// Component lines and the value recovered into each.
    pub components: Vec<IssueLine>,
    /// Stable dedup key. A retry of the SAME unbuild must NOT move stock twice.
    pub idempotency_key: String,
}

/// What one repair part leg does to stock (the port mirrors the line_type).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepairLeg {
    pub company_id: Uuid,
    pub repair_order_id: Uuid,
    pub item_id: Uuid,
    pub warehouse_id: Option<Uuid>,
    /// "add" issues the part out of stock into the repair; "remove" writes it off to the
    /// inventory-loss account; "recycle" receives it back into stock.
    pub line_type: String,
    pub quantity: Decimal,
    pub rate: Decimal,
    /// Stable dedup key. A retry of the SAME leg must NOT move stock twice.
    pub idempotency_key: String,
}

/// A read-only availability probe for a repair "add" leg — validate asks the inventory side
/// whether the part is on hand BEFORE the order is confirmed. Remove/recycle legs draw nothing
/// from stock and are never probed. On shortfall the port returns its rejection with the
/// shortfall detail in `message`; no stock moves.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepairAvailability {
    pub company_id: Uuid,
    pub item_id: Uuid,
    pub warehouse_id: Option<Uuid>,
    pub quantity: Decimal,
}

/// Inventory's rejection of an issue/receipt/reversal/leg (e.g. insufficient stock).
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
    /// Add finished goods (and byproduct legs) to stock at the given values.
    async fn receive_finished(&self, req: &FinishedReceipt) -> Result<(), InventoryRejected>;
    /// Reverse a done order's output back into components (an unbuild) — FG out, components in.
    async fn reverse_production(&self, req: &UnbuildReversal) -> Result<(), InventoryRejected>;
    /// Execute one repair part leg (add / remove / recycle), idempotent by its key.
    async fn execute_repair_leg(&self, req: &RepairLeg) -> Result<(), InventoryRejected>;
    /// Read-only on-hand probe for a repair "add" leg (validate's availability check).
    async fn check_repair_availability(&self, req: &RepairAvailability) -> Result<(), InventoryRejected>;
}
