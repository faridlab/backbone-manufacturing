//! The hand-authored manufacturing write path (user-owned; survives regen).
//!
//! Product definition: `create_bom` rolls up component + operation cost. Execution: a Work Order's
//! value flows through WIP in three balanced posts and nets WIP to ZERO on completion —
//!   consume  Dr WIP · Cr Raw-Material Stock   (materials issued; value from real inventory)
//!   operate  Dr WIP · Cr Conversion-Applied   (job-card labour/overhead)
//!   receive  Dr Finished-Goods · Cr WIP        (FG = raw + operating)
//! Each post is transition-gated (the status advance is the once-only guard) and carries a stable
//! idempotency key, so a retry never double-charges WIP. Manufacturing owns no stock or ledger: it
//! drives inventory (InventoryPort) for the physical moves + valuation and emits the posts (GlPostSink).
//! Money is IDR, 2dp, half-away-from-zero.
//!
//! **This file is the hub:** it holds the module's vocabulary (input structs, outcomes, errors) and
//! the shared helpers (`post`, `received_value`, `load_wo`). The write surface is chunked into focused
//! siblings, each an `impl ManufacturingWriteService` block over these same types:
//!
//! - [`super::manufacturing_bom_definition`] — roll up component + operation cost (`create_bom`).
//! - [`super::manufacturing_work_order`] — create + release a Work Order (BOM explosion).
//! - [`super::manufacturing_execution`] — the WIP seam: `consume_materials` / `receive_finished`.
//! - [`super::manufacturing_job_card`] — job-card open + completion (conversion cost to WIP).

use rust_decimal::{Decimal, RoundingStrategy};
use sqlx::PgPool;
use uuid::Uuid;

use crate::infrastructure::persistence::{
    BomItemRepository, BomOperationRepository, BomRepository, JobCardRepository,
    WorkOrderItemRepository, WorkOrderRepository, WorkOrderRow,
};

use super::manufacturing_gl::{AccountingPostEnvelope, GlPostSink};

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
}

// ---- request DTOs ----------------------------------------------------------------------------

pub struct NewBomItem {
    pub item_id: Uuid,
    pub quantity: Decimal,
    pub rate: Decimal,
    /// A phantom sub-assembly — exploded through to its own BOM's components at release, never issued.
    pub is_phantom: bool,
}
pub struct NewBomOperation {
    pub operation_id: Uuid,
    pub workstation_id: Uuid,
    pub time_in_mins: Decimal,
    pub hour_rate: Decimal,
}
pub struct NewBom {
    pub company_id: Uuid,
    pub item_id: Uuid,
    pub bom_code: String,
    pub quantity: Decimal,
    pub uom: Option<String>,
    pub items: Vec<NewBomItem>,
    pub operations: Vec<NewBomOperation>,
}

pub struct NewWorkOrder {
    pub company_id: Uuid,
    pub work_order_number: String,
    pub item_id: Uuid,
    pub bom_id: Uuid,
    pub quantity: Decimal,
    pub wip_warehouse_id: Option<Uuid>,
    pub fg_warehouse_id: Option<Uuid>,
    pub wip_account_id: Option<Uuid>,
    pub fg_account_id: Option<Uuid>,
    pub raw_material_account_id: Option<Uuid>,
    pub conversion_cost_account_id: Option<Uuid>,
}

pub struct NewJobCard {
    pub company_id: Uuid,
    pub work_order_id: Uuid,
    pub operation_id: Uuid,
    pub workstation_id: Uuid,
    pub total_time_mins: Decimal,
    pub hour_rate: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConsumeOutcome {
    pub raw_material_value: Decimal,
    pub already: bool,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ReceiveOutcome {
    pub finished_value: Decimal,
    pub completed: bool,
    pub already: bool,
}

pub struct ManufacturingWriteService {
    pub(super) pool: PgPool,
    pub(super) boms: BomRepository,
    pub(super) bom_items: BomItemRepository,
    pub(super) bom_operations: BomOperationRepository,
    pub(super) work_orders: WorkOrderRepository,
    pub(super) work_order_items: WorkOrderItemRepository,
    pub(super) job_cards: JobCardRepository,
}

impl ManufacturingWriteService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            boms: BomRepository::new(pool.clone()),
            bom_items: BomItemRepository::new(pool.clone()),
            bom_operations: BomOperationRepository::new(pool.clone()),
            work_orders: WorkOrderRepository::new(pool.clone()),
            work_order_items: WorkOrderItemRepository::new(pool.clone()),
            job_cards: JobCardRepository::new(pool.clone()),
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
        // RLS scope (ADR-0008), ID-only pattern: no company argument — the read rides the
        // request-dedicated connection, so RLS fences it to the caller's tenant. Callers that are
        // EVENT-driven (not on a request) must wrap the call in
        // `with_company_scope(Some(event.company_id))` or this read fails closed.
        self.work_orders.load(&self.pool, wo_id).await?
            .ok_or(ManufacturingError::NotFound("work order"))
    }
}

pub(super) fn is_dup(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505"))
}
