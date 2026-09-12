//! The hand-authored manufacturing write path (user-owned; survives regen).
//!
//! Product definition: `create_bom` rolls up component + operation cost. Execution: a Work Order's
//! value flows through WIP in three balanced posts and nets WIP to ZERO on completion —
//!   consume  Dr WIP · Cr Raw-Material Stock   (materials issued; value from real inventory)
//!   operate  Dr WIP · Cr Conversion-Applied   (job-card labour/overhead)
//!   receive  Dr Finished-Goods · Cr WIP        (FG = raw + operating, minus byproduct shares)
//! Each post is transition-gated (the status advance is the once-only guard) and carries a stable
//! idempotency key, so a retry never double-charges WIP. Manufacturing owns no stock or ledger: it
//! drives inventory (InventoryPort) for the physical moves + valuation and emits the posts (GlPostSink).
//! Money is IDR, 2dp, half-away-from-zero.
//!
//! **This file is the hub:** it holds the module's vocabulary (input structs, outcomes, errors) and
//! the shared helpers (`post`, `received_value`, `load_wo`, `resolve_account`). The write surface
//! is chunked into focused siblings, each an `impl ManufacturingWriteService` block over these
//! same types:
//!
//! - [`super::manufacturing_bom_definition`] — roll up component + operation cost (`create_bom`).
//! - [`super::manufacturing_work_order`] — create + confirm a Work Order (BOM explosion) + cancel.
//! - [`super::manufacturing_execution`] — the WIP seam: `consume_materials` / `receive_finished`
//!   (byproduct legs, extra-cost leg, posture-aware standard costing).
//! - [`super::manufacturing_job_card`] — job-card start + completion (conversion cost to WIP) + cancel.
//! - [`super::manufacturing_unbuild`] — disassemble a done order's output back to components.
//! - [`super::manufacturing_repair`] — the repair verb chain (validate / start / end / cancel).
//! - [`super::manufacturing_workcenter`] — productivity recording + the on-demand OEE report.
//! - [`super::manufacturing_subcontract`] — the subcontract receipt event → hidden work order mint.
//!
//! Account resolution order is ALWAYS: per-order override → category costing default → LOUD
//! `MissingAccount`. A hardcoded fallback account would silently post to someone else's ledger
//! and is never used.
//!
//! Tenancy (ADR-0029): the module is tenant-agnostic. Writes bind the ambient org scope (set per
//! request by the composing service) — never a module-side tenant value; pool reads ride the
//! caller-scoped helpers. Cross-module payload fields that still carry a company id are legacy
//! twins, filled from the ambient scope's company echo, never keyed on by a statement here.

use backbone_orm::org_scope;
use rust_decimal::{Decimal, RoundingStrategy};
use sqlx::PgPool;
use uuid::Uuid;

use crate::infrastructure::persistence::{
    BomItemRepository, BomOperationRepository, BomRepository, BomByproductRepository,
    BomSubcontractorRepository, CostingDefaultsAccounts, CostingDefaultsRepository,
    JobCardRepository, RepairRepository, SubcontractLinkRepository, UnbuildRepository,
    WorkcenterRepository, WorkOrderItemRepository, WorkOrderRepository, WorkOrderRow,
};

use super::manufacturing_gl::{AccountingPostEnvelope, GlPostSink};

/// The ambient org scope's legacy company echo — the acting org unit, which the spine mirrored
/// from the historical company id verbatim. Cross-module wire fields that still carry a tenant
/// (the GL envelope, the inventory issues, the outbox fence, the event payloads) are filled from
/// it; nothing in this module keys a statement on it. `Uuid::nil()` when no scope is bound
/// (undecorated module tests, jobs).
pub(crate) fn legacy_company_echo() -> Uuid {
    org_scope::current_org_scope()
        .and_then(|s| s.legacy_company_id())
        .unwrap_or(Uuid::nil())
}

/// Re-bind the caller's ambient org scope onto a transaction this service opened itself — the
/// scope is task-local and a fresh pool transaction carries none of it. With no ambient scope
/// (standalone deployment, jobs) the transaction stays plain: the module is tenant-agnostic and
/// the composed decorator owns isolation.
pub(crate) async fn relay_ambient_scope(conn: &mut sqlx::PgConnection) -> Result<(), sqlx::Error> {
    if let Some(scope) = org_scope::current_org_scope() {
        org_scope::bind_org_scope_on(conn, &scope).await?;
    }
    Ok(())
}

pub(super) fn money(v: Decimal) -> Decimal {
    v.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
}

pub(super) fn sixty() -> Decimal {
    Decimal::from(60)
}

#[derive(Debug, thiserror::Error)]
pub enum ManufacturingError {
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
    #[error("not found: {0}")]
    NotFound(&'static str),
    #[error("invalid state: {0}")]
    InvalidState(&'static str),
    #[error("missing account: {0}")]
    MissingAccount(&'static str),
    #[error("over-produce: producing {producing} would exceed the {ordered} ordered")]
    OverProduce { producing: Decimal, ordered: Decimal },
    #[error("inventory rejected: {0}")]
    Inventory(String),
    #[error("gl rejected: {0}")]
    Gl(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("duplicate number: {0}")]
    DuplicateNumber(String),
    #[error("unbuild source not done: the source work order must be fully produced before any of its output is unbuilt")]
    UnbuildSourceNotDone,
    #[error("unbuild over remaining: unbuilding {requested} would exceed the {remaining} producible quantity left on the source order")]
    UnbuildOverRemaining { requested: Decimal, remaining: Decimal },
    #[error("cost share overflow: byproduct cost shares sum to {sum}% — the family may not carry off more than the whole batch")]
    CostShareOverflow { sum: Decimal },
    #[error("subcontract kind mismatch: the receipt event's order_kind is '{0}', not subcontract — refusing to mint a manufacturing order")]
    SubcontractKindMismatch(String),
    #[error("repair invalid state: {0}")]
    RepairInvalidState(&'static str),
}

// ---- request DTOs ----------------------------------------------------------------------------

pub struct NewBomItem {
    pub item_id: Uuid,
    pub quantity: Decimal,
    pub rate: Decimal,
    /// A phantom sub-assembly — exploded through to its own BOM's components at confirm, never issued.
    pub is_phantom: bool,
}
pub struct NewBomOperation {
    pub operation_id: Uuid,
    pub workstation_id: Uuid,
    pub time_in_mins: Decimal,
    pub hour_rate: Decimal,
}
pub struct NewBom {
    pub item_id: Uuid,
    pub bom_code: String,
    pub quantity: Decimal,
    pub uom: Option<String>,
    pub items: Vec<NewBomItem>,
    pub operations: Vec<NewBomOperation>,
}
/// A byproduct leg authored onto a BoM. The share sum across the family is guarded at authoring
/// (`add_bom_byproduct` refuses a family whose shares would exceed 100) — never clamped.
pub struct NewBomByproduct {
    pub bom_id: Uuid,
    pub item_id: Uuid,
    /// Category whose costing defaults supply the byproduct's stock account (caller-supplied).
    pub product_category_id: Option<Uuid>,
    pub quantity: Decimal,
    pub cost_share: Decimal,
}

pub struct NewWorkOrder {
    pub work_order_number: String,
    pub item_id: Uuid,
    pub bom_id: Uuid,
    pub quantity: Decimal,
    /// The item's product category at creation — selects the costing defaults that fill any
    /// unset account override.
    pub product_category_id: Option<Uuid>,
    pub wip_warehouse_id: Option<Uuid>,
    pub fg_warehouse_id: Option<Uuid>,
    pub wip_account_id: Option<Uuid>,
    pub fg_account_id: Option<Uuid>,
    pub raw_material_account_id: Option<Uuid>,
    pub conversion_cost_account_id: Option<Uuid>,
}

pub struct NewJobCard {
    pub work_order_id: Uuid,
    pub operation_id: Uuid,
    pub workstation_id: Uuid,
    pub total_time_mins: Decimal,
    pub hour_rate: Decimal,
}

pub struct NewUnbuild {
    pub unbuild_number: String,
    pub work_order_id: Uuid,
    pub item_id: Uuid,
    pub quantity: Decimal,
}

/// One byproduct leg of a finished-goods receipt — the actual quantity harvested this batch.
/// Its VALUE is not taken from here: the BoM byproduct row's `cost_share` slices it off the
/// batch total (FG keeps the remainder), so a byproduct with no BoM row is refused LOUDLY.
pub struct ReceiveByproductLine {
    pub item_id: Uuid,
    pub quantity: Decimal,
}

/// The receipt request: how much FG came off the line this batch, plus its byproduct legs,
/// any subcontract extra cost, and the valuation posture for the FG line.
pub struct ReceiveFinishedOrder {
    pub produced_qty: Decimal,
    pub byproducts: Vec<ReceiveByproductLine>,
    /// The subcontract extra-cost leg (PO quoted value prorated by receipt share); None/zero for
    /// in-house orders. Posts on the credit side against the subcontract-interim account.
    pub extra_cost: Option<Decimal>,
    /// Valuation posture of the FG line: average (actual, WIP-derived — the default) or standard
    /// (pinned price; the gap posts to the cost-variance account, never a silent recompute).
    pub cost_posture: super::manufacturing_ports::CostPosture,
    /// Required by the Standard posture; ignored by Average.
    pub standard_unit_price: Option<Decimal>,
}

pub struct NewRepairOrder {
    pub repair_number: String,
    pub item_id: Uuid,
    /// The item's category at creation — selects the costing defaults that resolve the
    /// repair-expense / inventory-loss / raw accounts at end-of-repair.
    pub product_category_id: Option<Uuid>,
    pub quantity: Decimal,
    pub parts: Vec<NewRepairPart>,
}

pub struct NewRepairPart {
    pub item_id: Uuid,
    pub warehouse_id: Option<Uuid>,
    pub line_type: crate::domain::entity::RepairLineType,
    pub quantity: Decimal,
    pub rate: Decimal,
}

pub struct NewWorkstationLoss {
    pub name: String,
    pub loss_type: crate::domain::entity::LossType,
}

pub struct NewProductivity {
    pub workstation_id: Uuid,
    pub job_card_id: Option<Uuid>,
    pub loss_name: String,
    pub date_start: chrono::DateTime<chrono::Utc>,
    pub date_end: Option<chrono::DateTime<chrono::Utc>>,
    pub description: Option<String>,
}

pub struct NewCostingDefaults {
    pub product_category_id: Uuid,
    pub wip_account_id: Option<Uuid>,
    pub fg_account_id: Option<Uuid>,
    pub raw_material_account_id: Option<Uuid>,
    pub conversion_cost_account_id: Option<Uuid>,
    pub subcontract_interim_account_id: Option<Uuid>,
    pub cost_variance_account_id: Option<Uuid>,
    pub inventory_loss_account_id: Option<Uuid>,
    pub repair_expense_account_id: Option<Uuid>,
}

// ---- outcomes --------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ConsumeOutcome {
    pub raw_material_value: Decimal,
    pub already: bool,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ReceiveOutcome {
    pub finished_value: Decimal,
    /// byproduct legs minted by this receipt: (item, value carried off)
    pub byproduct_value: Decimal,
    /// the subcontract extra-cost leg value (zero outside subcontract orders)
    pub extra_cost: Decimal,
    pub completed: bool,
    pub already: bool,
}
#[derive(Debug, Clone, PartialEq)]
pub struct UnbuildOutcome {
    pub reversed_value: Decimal,
    pub already: bool,
}
#[derive(Debug, Clone, PartialEq)]
pub struct RepairEndOutcome {
    pub parts_moved: usize,
    pub repair_expense: Decimal,
    pub inventory_loss: Decimal,
    pub recovered_value: Decimal,
}

/// The on-demand OEE report over a window. Every component is a RATIO in [0, 1]
/// (multiply by 100 at the presentation edge); each is (total − respective losses) / total.
#[derive(Debug, Clone, PartialEq)]
pub struct OeeReport {
    pub availability: Decimal,
    pub performance: Decimal,
    pub quality: Decimal,
    pub oee: Decimal,
    /// Total booked seconds over the window (the denominator).
    pub total_seconds: Decimal,
}

pub struct ManufacturingWriteService {
    pub(super) pool: PgPool,
    pub(super) boms: BomRepository,
    pub(super) bom_items: BomItemRepository,
    pub(super) bom_operations: BomOperationRepository,
    pub(super) bom_byproducts: BomByproductRepository,
    pub(super) bom_subcontractors: BomSubcontractorRepository,
    pub(super) work_orders: WorkOrderRepository,
    pub(super) work_order_items: WorkOrderItemRepository,
    pub(super) job_cards: JobCardRepository,
    pub(super) unbuilds: UnbuildRepository,
    pub(super) repairs: RepairRepository,
    pub(super) workcenter: WorkcenterRepository,
    pub(super) costing_defaults: CostingDefaultsRepository,
    pub(super) subcontract_links: SubcontractLinkRepository,
}

impl ManufacturingWriteService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            boms: BomRepository::new(pool.clone()),
            bom_items: BomItemRepository::new(pool.clone()),
            bom_operations: BomOperationRepository::new(pool.clone()),
            bom_byproducts: BomByproductRepository::new(pool.clone()),
            bom_subcontractors: BomSubcontractorRepository::new(pool.clone()),
            work_orders: WorkOrderRepository::new(pool.clone()),
            work_order_items: WorkOrderItemRepository::new(pool.clone()),
            job_cards: JobCardRepository::new(pool.clone()),
            unbuilds: UnbuildRepository::new(pool.clone()),
            repairs: RepairRepository::new(pool.clone()),
            workcenter: WorkcenterRepository::new(pool.clone()),
            costing_defaults: CostingDefaultsRepository::new(pool.clone()),
            subcontract_links: SubcontractLinkRepository::new(pool.clone()),
            pool,
        }
    }

    // ---- shared helpers -----------------------------------------------------------------------

    pub(super) async fn post(
        &self,
        gl: &dyn GlPostSink,
        env: &AccountingPostEnvelope,
    ) -> Result<(), ManufacturingError> {
        if !env.is_balanced() {
            return Err(ManufacturingError::Invalid("unbalanced posting".into()));
        }
        gl.post(env).await.map_err(|r| ManufacturingError::Gl(r.code))?;
        Ok(())
    }

    pub(super) async fn received_value(&self, wo_id: Uuid) -> Result<Decimal, ManufacturingError> {
        // Value already received = accumulated FG value for prior partial receipts.
        // Tracked as: (raw+operating) * produced_qty/quantity at the time of each receipt; on the final
        // receipt we clear the residue. Recompute from produced_qty for a single-receipt MVP.
        let wo = self.load_wo(wo_id).await?;
        if wo.produced_qty <= Decimal::ZERO {
            return Ok(Decimal::ZERO);
        }
        Ok(money((wo.raw_material_cost + wo.operating_cost) * wo.produced_qty / wo.quantity))
    }

    pub(super) async fn load_wo(&self, wo_id: Uuid) -> Result<WorkOrderRow, ManufacturingError> {
        // ID-only pattern: no tenant argument — the read rides the request-dedicated connection,
        // so the composed decorator's org fence scopes it to the caller's unit (ADR-0029).
        // Undecorated (module tests) the read runs plain.
        self.work_orders.load(&self.pool, wo_id).await?
            .ok_or(ManufacturingError::NotFound("work order"))
    }

    /// THE account-resolution chain: per-order override → category costing default → LOUD
    /// `MissingAccount`. Never a hardcoded fallback — a silent default account posts to someone
    /// else's ledger. The category-default lookup is ID-only: under the composed decorator the
    /// org fence scopes it to the caller's unit (ADR-0029).
    pub(super) async fn resolve_account(
        &self,
        label: &'static str,
        order_override: Option<Uuid>,
        product_category_id: Option<Uuid>,
        pick: fn(&CostingDefaultsAccounts) -> Option<Uuid>,
    ) -> Result<Uuid, ManufacturingError> {
        if let Some(account) = order_override {
            return Ok(account);
        }
        if let Some(category) = product_category_id {
            if let Some(defaults) = self
                .costing_defaults
                .find(&self.pool, category)
                .await?
            {
                if let Some(account) = pick(&defaults) {
                    return Ok(account);
                }
            }
        }
        Err(ManufacturingError::MissingAccount(label))
    }
}

pub(super) fn is_dup(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505"))
}
