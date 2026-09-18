//! Validated manufacturing write routes — the compiled, invariant-enforcing command surface.
//!
//! `ManufacturingModule::all_crud_routes()` (in [`crate`]) mounts *unguarded* generic CRUD on every
//! entity: it can flip a Work Order to `done` with zero GL posts, zero material consumption, and
//! a stranded non-zero WIP balance. That bypasses every manufacturing invariant.
//!
//! This module is the honest counterpart: a command router that forwards to
//! [`ManufacturingWriteService`] — confirm → consume → operate → receive (and the unbuild / repair /
//! workcenter verbs) — so each state transition emits the WIP/FG postings it must, and WIP nets to
//! zero on completion.
//!
//! No compatibility aliases exist: the old `release` route is GONE, replaced by `confirm` (the
//! state vocabulary renamed with it). A stale client gets a 404, not a silently-different verb.
//!
//! Tenancy (ADR-0029): the module is tenant-agnostic — no request body carries a company id.
//! Scoping is installed by the composing service's tenancy decorator from the authenticated
//! principal's org scope; these handlers forward ids and payloads only.
//!
//! ## The ports are YOURS to supply
//!
//! Manufacturing owns no stock or ledger ([`InventoryPort`] lives in `backbone-inventory`,
//! [`GlPostSink`] in `backbone-accounting`). Those adapters are cross-context dependencies a `module`
//! crate does not own, so they are **not** auto-wired — you supply them via
//! [`ManufacturingWriteDeps`]. A no-op `GlPostSink` compiles and looks done but silently drops the WIP
//! postings; always supply real adapters from the composing `backend-service`.
//!
//! ## Assembly
//!
//! ```ignore
//! use backbone_manufacturing::{ManufacturingModule, write_api};
//! use std::sync::Arc;
//!
//! let m = ManufacturingModule::builder().with_database(pool.clone()).build()?;
//! let deps = write_api::ManufacturingWriteDeps {
//!     write_service: m.write_service(),
//!     inventory: Arc::new(my_inventory_adapter),  // real InventoryPort over backbone-inventory
//!     gl:        Arc::new(my_gl_adapter),         // real GlPostSink over backbone-accounting
//!     events:    Arc::new(LoggingSink),           // or your bus sink
//! };
//! // Reads (generic) + validated writes (this router):
//! let app = Router::new()
//!     .merge(m.all_crud_routes())                                  // trusted/admin/reads
//!     .merge(write_api::create_manufacturing_write_routes().with_state(deps));
//! ```

#![allow(unused_imports)]

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::application::service::{
    manufacturing_events::ManufacturingEventSink,
    manufacturing_gl::GlPostSink,
    manufacturing_ports::{CostPosture, InventoryPort},
    manufacturing_write_service::{
        ManufacturingError, ManufacturingWriteService, NewBom, NewBomItem, NewBomOperation,
        NewJobCard, NewProductivity, NewRepairOrder, NewRepairPart, NewUnbuild, NewWorkOrder,
        ReceiveByproductLine, ReceiveFinishedOrder,
    },
};
use crate::domain::entity::{RepairLineType, ReservationState};

/// Caller-supplied dependencies for the validated write router.
///
/// The write service comes from [`crate::ManufacturingModule::write_service`]; the three ports are
/// the composing service's real adapters (manufacturing owns neither stock nor ledger).
#[derive(Clone)]
pub struct ManufacturingWriteDeps {
    pub write_service: Arc<ManufacturingWriteService>,
    pub inventory: Arc<dyn InventoryPort>,
    pub gl: Arc<dyn GlPostSink>,
    pub events: Arc<dyn ManufacturingEventSink>,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// The validated manufacturing command router. State is [`ManufacturingWriteDeps`] (caller-supplied);
/// mount via `.with_state(deps)`. Routes are the WIP-costing transitions the generic CRUD surface
/// must not allow directly.
pub fn create_manufacturing_write_routes() -> Router<ManufacturingWriteDeps> {
    Router::new()
        // Authoring: a bill of materials, then the order raised against it. These forward to the
        // same validated write service the lifecycle verbs use — they are here because without
        // them the lifecycle has nothing to act on: a work order could be confirmed, consumed and
        // received, and never raised.
        .route("/boms", post(create_bom))
        .route("/work-orders", post(create_work_order))
        // Work-order lifecycle: draft → confirmed → progress → to_close|done, cancel from draft|confirmed.
        .route("/work-orders/:id/confirm", post(confirm_work_order))
        .route("/work-orders/:id/cancel", post(cancel_work_order))
        .route("/work-orders/:id/reservation", post(write_reservation))
        .route("/work-orders/:id/consume", post(consume_materials))
        .route("/work-orders/:id/job-cards", post(add_job_card))
        .route("/work-orders/:id/receive", post(receive_finished))
        // Job-card lifecycle: start (ready|blocked → progress), completion charges WIP, cancel.
        .route("/job-cards/:id/start", post(start_job_card))
        .route("/job-cards/:id/complete", post(complete_job_card))
        .route("/job-cards/:id/cancel", post(cancel_job_card))
        // Unbuild: open a draft, then execute the reversal (source WO must be done).
        .route("/unbuilds", post(create_unbuild))
        .route("/unbuilds/:id/execute", post(execute_unbuild))
        // Repair: open with parts, validate (availability probe), start, end (legs move), cancel.
        .route("/repairs", post(create_repair))
        .route("/repairs/:id/validate", post(validate_repair))
        .route("/repairs/:id/start", post(start_repair))
        .route("/repairs/:id/end", post(end_repair))
        .route("/repairs/:id/cancel", post(cancel_repair))
        // Workcenter: book productivity stretches; OEE is a pure read over a window.
        .route("/workstations/:id/productivity", post(record_productivity))
        .route("/workstations/:id/oee", get(workstation_oee))
}

// ---------------------------------------------------------------------------
// Handlers — thin forwarders; ManufacturingWriteService does all the work.
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBomItemBody {
    pub item_id: Uuid,
    pub quantity: Decimal,
    #[serde(default)]
    pub rate: Decimal,
    /// A phantom sub-assembly is exploded through to its own BOM's components at confirm,
    /// never issued as itself.
    #[serde(default)]
    pub is_phantom: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBomOperationBody {
    pub operation_id: Uuid,
    pub workstation_id: Uuid,
    pub time_in_mins: Decimal,
    pub hour_rate: Decimal,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBomBody {
    pub item_id: Uuid,
    pub bom_code: String,
    pub quantity: Decimal,
    #[serde(default)]
    pub uom: Option<String>,
    pub items: Vec<CreateBomItemBody>,
    #[serde(default)]
    pub operations: Vec<CreateBomOperationBody>,
}

#[derive(Serialize)]
pub struct BomResponse {
    pub bom_id: Uuid,
}

/// Author a bill of materials. The service refuses a BoM with no components and a
/// non-positive quantity; both are invariants, not shapes the caller may choose.
pub async fn create_bom(
    State(deps): State<ManufacturingWriteDeps>,
    Json(body): Json<CreateBomBody>,
) -> Result<Json<BomResponse>, (StatusCode, String)> {
    let id = deps
        .write_service
        .create_bom(NewBom {
            item_id: body.item_id,
            bom_code: body.bom_code,
            quantity: body.quantity,
            uom: body.uom,
            items: body
                .items
                .into_iter()
                .map(|i| NewBomItem {
                    item_id: i.item_id,
                    quantity: i.quantity,
                    rate: i.rate,
                    is_phantom: i.is_phantom,
                })
                .collect(),
            operations: body
                .operations
                .into_iter()
                .map(|o| NewBomOperation {
                    operation_id: o.operation_id,
                    workstation_id: o.workstation_id,
                    time_in_mins: o.time_in_mins,
                    hour_rate: o.hour_rate,
                })
                .collect(),
        })
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(BomResponse { bom_id: id }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkOrderBody {
    pub work_order_number: String,
    pub item_id: Uuid,
    pub bom_id: Uuid,
    pub quantity: Decimal,
    /// The item's product category, which selects the costing defaults that fill any account
    /// left unset below. Supplying neither leaves the order with no costing accounts, and the
    /// first valuation leg refuses rather than guessing one.
    #[serde(default)]
    pub product_category_id: Option<Uuid>,
    #[serde(default)]
    pub wip_warehouse_id: Option<Uuid>,
    #[serde(default)]
    pub fg_warehouse_id: Option<Uuid>,
    #[serde(default)]
    pub wip_account_id: Option<Uuid>,
    #[serde(default)]
    pub fg_account_id: Option<Uuid>,
    #[serde(default)]
    pub raw_material_account_id: Option<Uuid>,
    #[serde(default)]
    pub conversion_cost_account_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct WorkOrderResponse {
    pub work_order_id: Uuid,
}

/// Raise a draft work order against a bill of materials.
pub async fn create_work_order(
    State(deps): State<ManufacturingWriteDeps>,
    Json(body): Json<CreateWorkOrderBody>,
) -> Result<Json<WorkOrderResponse>, (StatusCode, String)> {
    let id = deps
        .write_service
        .create_work_order(NewWorkOrder {
            work_order_number: body.work_order_number,
            item_id: body.item_id,
            bom_id: body.bom_id,
            quantity: body.quantity,
            product_category_id: body.product_category_id,
            wip_warehouse_id: body.wip_warehouse_id,
            fg_warehouse_id: body.fg_warehouse_id,
            wip_account_id: body.wip_account_id,
            fg_account_id: body.fg_account_id,
            raw_material_account_id: body.raw_material_account_id,
            conversion_cost_account_id: body.conversion_cost_account_id,
        })
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(WorkOrderResponse { work_order_id: id }))
}

#[derive(Serialize)]
pub struct ConfirmResponse {
    pub work_order_id: Uuid,
    pub confirmed: bool,
}

/// Confirm a draft Work Order: explode its BOM into required materials (draft → confirmed).
/// Kit and subcontract BoMs are refused LOUDLY — they never get a hand-minted work order.
pub async fn confirm_work_order(
    State(deps): State<ManufacturingWriteDeps>,
    Path(id): Path<Uuid>,
) -> Result<Json<ConfirmResponse>, (StatusCode, String)> {
    deps.write_service
        .confirm_work_order(id, &*deps.events)
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(ConfirmResponse {
        work_order_id: id,
        confirmed: true,
    }))
}

#[derive(Serialize)]
pub struct CancelResponse {
    pub work_order_id: Uuid,
    pub cancelled: bool,
}

/// Cancel a Work Order from draft|confirmed only — an order carrying WIP or stock is refused LOUDLY.
pub async fn cancel_work_order(
    State(deps): State<ManufacturingWriteDeps>,
    Path(id): Path<Uuid>,
) -> Result<Json<CancelResponse>, (StatusCode, String)> {
    deps.write_service
        .cancel_work_order(id, &*deps.events)
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(CancelResponse {
        work_order_id: id,
        cancelled: true,
    }))
}

#[derive(Deserialize)]
pub struct ReservationBody {
    /// waiting | confirmed | assigned — the inventory-side projection of component availability.
    pub state: ReservationState,
}

#[derive(Serialize)]
pub struct ReservationResponse {
    pub work_order_id: Uuid,
    pub reservation_state: ReservationState,
}

/// Write the reservation_state projection — the ONLY surface that may write it. Availability is
/// expressed ONLY through this projection; no field named `availability` exists in the module.
pub async fn write_reservation(
    State(deps): State<ManufacturingWriteDeps>,
    Path(id): Path<Uuid>,
    Json(body): Json<ReservationBody>,
) -> Result<Json<ReservationResponse>, (StatusCode, String)> {
    deps.write_service
        .write_reservation(id, body.state)
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(ReservationResponse {
        work_order_id: id,
        reservation_state: body.state,
    }))
}

#[derive(Deserialize)]
pub struct ConsumeBody {
    /// Raw-material warehouse to issue components from.
    pub raw_warehouse_id: Uuid,
}

#[derive(Serialize)]
pub struct ConsumeResponse {
    pub raw_material_value: Decimal,
    /// true if the work order was already consumed (idempotent retry — no second issue).
    pub already_consumed: bool,
}

/// Issue all required materials to WIP (confirmed → progress): Dr WIP · Cr Raw-Material Stock.
pub async fn consume_materials(
    State(deps): State<ManufacturingWriteDeps>,
    Path(id): Path<Uuid>,
    Json(body): Json<ConsumeBody>,
) -> Result<Json<ConsumeResponse>, (StatusCode, String)> {
    let out = deps
        .write_service
        .consume_materials(id, body.raw_warehouse_id, &*deps.inventory, &*deps.gl, &*deps.events)
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(ConsumeResponse {
        raw_material_value: out.raw_material_value,
        already_consumed: out.already,
    }))
}

#[derive(Deserialize)]
pub struct AddJobCardBody {
    pub operation_id: Uuid,
    pub workstation_id: Uuid,
    pub total_time_mins: Decimal,
    pub hour_rate: Decimal,
}

#[derive(Serialize)]
pub struct JobCardResponse {
    pub job_card_id: Uuid,
}

/// Open a job card for an operation run against this work order (starts `ready`).
pub async fn add_job_card(
    State(deps): State<ManufacturingWriteDeps>,
    Path(work_order_id): Path<Uuid>,
    Json(body): Json<AddJobCardBody>,
) -> Result<Json<JobCardResponse>, (StatusCode, String)> {
    let id = deps
        .write_service
        .add_job_card(NewJobCard {
            work_order_id,
            operation_id: body.operation_id,
            workstation_id: body.workstation_id,
            total_time_mins: body.total_time_mins,
            hour_rate: body.hour_rate,
        })
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(JobCardResponse { job_card_id: id }))
}

#[derive(Serialize)]
pub struct StartJobCardResponse {
    pub job_card_id: Uuid,
    pub started: bool,
}

/// Start a job card on the floor (ready|blocked → progress). Starting from `blocked` is allowed —
/// parts arriving physically is enough.
pub async fn start_job_card(
    State(deps): State<ManufacturingWriteDeps>,
    Path(id): Path<Uuid>,
) -> Result<Json<StartJobCardResponse>, (StatusCode, String)> {
    deps.write_service
        .start_job_card(id)
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(StartJobCardResponse { job_card_id: id, started: true }))
}

#[derive(Serialize)]
pub struct CompleteResponse {
    /// Conversion cost charged to WIP (time/60 × hour rate).
    pub operating_cost: Decimal,
}

/// Complete a job card: charge its conversion cost to WIP — Dr WIP · Cr Conversion-Applied.
pub async fn complete_job_card(
    State(deps): State<ManufacturingWriteDeps>,
    Path(id): Path<Uuid>,
) -> Result<Json<CompleteResponse>, (StatusCode, String)> {
    let cost = deps
        .write_service
        .complete_job_card(id, &*deps.gl, &*deps.events)
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(CompleteResponse { operating_cost: cost }))
}

#[derive(Serialize)]
pub struct CancelJobCardResponse {
    pub job_card_id: Uuid,
    pub cancelled: bool,
}

/// Cancel a job card (ready|blocked|progress → cancel). A done card is refused LOUDLY — its
/// conversion cost is already in WIP.
pub async fn cancel_job_card(
    State(deps): State<ManufacturingWriteDeps>,
    Path(id): Path<Uuid>,
) -> Result<Json<CancelJobCardResponse>, (StatusCode, String)> {
    deps.write_service
        .cancel_job_card(id)
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(CancelJobCardResponse { job_card_id: id, cancelled: true }))
}

#[derive(Deserialize)]
pub struct ReceiveByproductBody {
    pub item_id: Uuid,
    /// Actual byproduct quantity harvested this batch; the VALUE comes from the BoM cost share.
    pub quantity: Decimal,
}

#[derive(Deserialize)]
pub struct ReceiveBody {
    /// Quantity to receive as finished goods (bounded by the ordered quantity).
    pub quantity: Decimal,
    /// Byproduct legs harvested alongside; each must have a BoM byproduct row (its cost share
    /// slices the value; the FG line keeps the remainder).
    #[serde(default)]
    pub byproducts: Vec<ReceiveByproductBody>,
    /// The subcontract extra-cost leg (PO quoted value prorated by receipt share); omit for
    /// in-house orders.
    pub extra_cost: Option<Decimal>,
    /// average (default — actual WIP-derived) or standard (pinned price; the gap posts to the
    /// cost-variance account, never a silent recompute).
    #[serde(default)]
    pub cost_posture: CostPosture,
    /// Required by the standard posture; ignored by average.
    pub standard_unit_price: Option<Decimal>,
}

#[derive(Serialize)]
pub struct ReceiveResponse {
    /// Value carried onto the FG line (the remainder after the byproduct shares).
    pub finished_value: Decimal,
    /// Total value carried off by the byproduct legs.
    pub byproduct_value: Decimal,
    /// The subcontract extra-cost leg value.
    pub extra_cost: Decimal,
    /// true if the work order is now fully produced (WIP nets to zero).
    pub completed: bool,
    /// true if a concurrent receive already won this quantity (idempotent).
    pub already: bool,
}

/// Receive finished goods (and byproduct legs) into stock: Dr FG (+ byproducts) · Cr WIP
/// (+ subcontract interim); completes the WO on full receipt, clearing WIP to zero.
pub async fn receive_finished(
    State(deps): State<ManufacturingWriteDeps>,
    Path(id): Path<Uuid>,
    Json(body): Json<ReceiveBody>,
) -> Result<Json<ReceiveResponse>, (StatusCode, String)> {
    let out = deps
        .write_service
        .receive_finished(
            id,
            ReceiveFinishedOrder {
                produced_qty: body.quantity,
                byproducts: body
                    .byproducts
                    .into_iter()
                    .map(|b| ReceiveByproductLine { item_id: b.item_id, quantity: b.quantity })
                    .collect(),
                extra_cost: body.extra_cost,
                cost_posture: body.cost_posture,
                standard_unit_price: body.standard_unit_price,
            },
            &*deps.inventory,
            &*deps.gl,
            &*deps.events,
        )
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(ReceiveResponse {
        finished_value: out.finished_value,
        byproduct_value: out.byproduct_value,
        extra_cost: out.extra_cost,
        completed: out.completed,
        already: out.already,
    }))
}

// ---------------------------------------------------------------------------
// Unbuild
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateUnbuildBody {
    pub unbuild_number: String,
    pub work_order_id: Uuid,
    pub item_id: Uuid,
    pub quantity: Decimal,
}

#[derive(Serialize)]
pub struct UnbuildResponse {
    pub unbuild_order_id: Uuid,
}

/// Open a draft unbuild order against a (presumably done) work order.
pub async fn create_unbuild(
    State(deps): State<ManufacturingWriteDeps>,
    Json(body): Json<CreateUnbuildBody>,
) -> Result<Json<UnbuildResponse>, (StatusCode, String)> {
    let id = deps
        .write_service
        .create_unbuild(NewUnbuild {
            unbuild_number: body.unbuild_number,
            work_order_id: body.work_order_id,
            item_id: body.item_id,
            quantity: body.quantity,
        })
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(UnbuildResponse { unbuild_order_id: id }))
}

#[derive(Deserialize)]
pub struct ExecuteUnbuildBody {
    /// Warehouse the components return to.
    pub raw_warehouse_id: Uuid,
}

#[derive(Serialize)]
pub struct ExecuteUnbuildResponse {
    pub reversed_value: Decimal,
    pub already: bool,
}

/// Execute an unbuild: reverse the finished goods back into components (source WO must be `done`;
/// quantity bounded by what remains producible after other done unbuilds).
pub async fn execute_unbuild(
    State(deps): State<ManufacturingWriteDeps>,
    Path(id): Path<Uuid>,
    Json(body): Json<ExecuteUnbuildBody>,
) -> Result<Json<ExecuteUnbuildResponse>, (StatusCode, String)> {
    let out = deps
        .write_service
        .execute_unbuild(id, body.raw_warehouse_id, &*deps.inventory, &*deps.gl, &*deps.events)
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(ExecuteUnbuildResponse {
        reversed_value: out.reversed_value,
        already: out.already,
    }))
}

// ---------------------------------------------------------------------------
// Repair
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct RepairPartBody {
    pub item_id: Uuid,
    pub warehouse_id: Option<Uuid>,
    /// add = consumed into the repair; remove = scrapped to the inventory-loss account;
    /// recycle = recovered back into stock.
    pub line_type: RepairLineType,
    pub quantity: Decimal,
    pub rate: Decimal,
}

#[derive(Deserialize)]
pub struct CreateRepairBody {
    pub repair_number: String,
    pub item_id: Uuid,
    /// The item's category at creation — selects the costing defaults that resolve the
    /// repair-expense / inventory-loss / raw accounts at end-of-repair.
    pub product_category_id: Option<Uuid>,
    pub quantity: Decimal,
    #[serde(default)]
    pub parts: Vec<RepairPartBody>,
}

/// Open a draft repair order with its part lines. Nothing moves until `end`.
pub async fn create_repair(
    State(deps): State<ManufacturingWriteDeps>,
    Json(body): Json<CreateRepairBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let id = deps
        .write_service
        .create_repair_order(NewRepairOrder {
            repair_number: body.repair_number,
            item_id: body.item_id,
            product_category_id: body.product_category_id,
            quantity: body.quantity,
            parts: body
                .parts
                .into_iter()
                .map(|p| NewRepairPart {
                    item_id: p.item_id,
                    warehouse_id: p.warehouse_id,
                    line_type: p.line_type,
                    quantity: p.quantity,
                    rate: p.rate,
                })
                .collect(),
        })
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(serde_json::json!({ "repair_order_id": id })))
}

#[derive(Serialize)]
pub struct RepairVerbResponse {
    pub repair_order_id: Uuid,
}

/// Validate a draft: every ADD leg's part must be on hand (read-only probe through the inventory
/// port) — a shortfall is LOUD. Then draft → confirmed.
pub async fn validate_repair(
    State(deps): State<ManufacturingWriteDeps>,
    Path(id): Path<Uuid>,
) -> Result<Json<RepairVerbResponse>, (StatusCode, String)> {
    deps.write_service
        .validate_repair(id, &*deps.inventory)
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(RepairVerbResponse { repair_order_id: id }))
}

/// Start the repair (a draft is auto-confirmed first): → under_repair.
pub async fn start_repair(
    State(deps): State<ManufacturingWriteDeps>,
    Path(id): Path<Uuid>,
) -> Result<Json<RepairVerbResponse>, (StatusCode, String)> {
    deps.write_service
        .start_repair(id)
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(RepairVerbResponse { repair_order_id: id }))
}

#[derive(Serialize)]
pub struct EndRepairResponse {
    pub repair_order_id: Uuid,
    pub parts_moved: usize,
    pub repair_expense: Decimal,
    pub inventory_loss: Decimal,
    pub recovered_value: Decimal,
}

/// End the repair: EVERY part leg moves in one pass (add → repair expense, remove → inventory
/// loss, recycle → back to stock), with the grouped GL post. Once-only; `done` is uncancelable.
pub async fn end_repair(
    State(deps): State<ManufacturingWriteDeps>,
    Path(id): Path<Uuid>,
) -> Result<Json<EndRepairResponse>, (StatusCode, String)> {
    let out = deps
        .write_service
        .end_repair(id, &*deps.inventory, &*deps.gl, &*deps.events)
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(EndRepairResponse {
        repair_order_id: id,
        parts_moved: out.parts_moved,
        repair_expense: out.repair_expense,
        inventory_loss: out.inventory_loss,
        recovered_value: out.recovered_value,
    }))
}

/// Cancel a repair (draft|confirmed|under_repair → cancel). A done repair is refused LOUDLY.
pub async fn cancel_repair(
    State(deps): State<ManufacturingWriteDeps>,
    Path(id): Path<Uuid>,
) -> Result<Json<RepairVerbResponse>, (StatusCode, String)> {
    deps.write_service
        .cancel_repair(id)
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(RepairVerbResponse { repair_order_id: id }))
}

// ---------------------------------------------------------------------------
// Workcenter: productivity + OEE
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ProductivityBody {
    /// Named loss reason (must exist; its classification feeds the OEE buckets).
    pub loss_name: String,
    pub job_card_id: Option<Uuid>,
    pub date_start: chrono::DateTime<chrono::Utc>,
    /// May be null — an open stretch counts up to now on the read side.
    pub date_end: Option<chrono::DateTime<chrono::Utc>>,
    pub description: Option<String>,
}

#[derive(Serialize)]
pub struct ProductivityResponse {
    pub productivity_id: Uuid,
}

/// Book a stretch of workstation time against a loss reason. Duration is read-side only — no
/// stored column, no cron.
pub async fn record_productivity(
    State(deps): State<ManufacturingWriteDeps>,
    Path(workstation_id): Path<Uuid>,
    Json(body): Json<ProductivityBody>,
) -> Result<Json<ProductivityResponse>, (StatusCode, String)> {
    let id = deps
        .write_service
        .record_productivity(NewProductivity {
            workstation_id,
            job_card_id: body.job_card_id,
            loss_name: body.loss_name,
            date_start: body.date_start,
            date_end: body.date_end,
            description: body.description,
        })
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(ProductivityResponse { productivity_id: id }))
}

#[derive(Deserialize)]
pub struct OeeQuery {
    pub from: chrono::DateTime<chrono::Utc>,
    pub to: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize)]
pub struct OeeResponse {
    /// Ratios in [0, 1] — multiply by 100 at the presentation edge.
    pub availability: Decimal,
    pub performance: Decimal,
    pub quality: Decimal,
    pub oee: Decimal,
    pub total_seconds: Decimal,
}

/// The on-demand OEE report over a window — a pure read; nothing stored, nothing scheduled.
pub async fn workstation_oee(
    State(deps): State<ManufacturingWriteDeps>,
    Path(workstation_id): Path<Uuid>,
    axum::extract::Query(q): axum::extract::Query<OeeQuery>,
) -> Result<Json<OeeResponse>, (StatusCode, String)> {
    let r = deps
        .write_service
        .workstation_oee(workstation_id, q.from, q.to)
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(OeeResponse {
        availability: r.availability,
        performance: r.performance,
        quality: r.quality,
        oee: r.oee,
        total_seconds: r.total_seconds,
    }))
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

fn map_mfg_error(e: ManufacturingError) -> (StatusCode, String) {
    use ManufacturingError::*;
    let status = match &e {
        NotFound(_) => StatusCode::NOT_FOUND,
        InvalidState(_)
        | RepairInvalidState(_)
        | OverProduce { .. }
        | DuplicateNumber(_)
        | UnbuildSourceNotDone
        | UnbuildOverRemaining { .. }
        | CostShareOverflow { .. }
        | SubcontractKindMismatch(_) => StatusCode::CONFLICT,
        Invalid(_) => StatusCode::BAD_REQUEST,
        Inventory(_) => StatusCode::UNPROCESSABLE_ENTITY,
        // MissingAccount / Gl / Db are server/config faults, not client-correctable.
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    // Don't leak raw sqlx::Error text to clients.
    let msg = match &e {
        Db(_) => "internal error".to_string(),
        _ => e.to_string(),
    };
    (status, msg)
}
