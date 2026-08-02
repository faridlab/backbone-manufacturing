//! Validated manufacturing write routes — the compiled, invariant-enforcing command surface.
//!
//! `ManufacturingModule::all_crud_routes()` (in [`crate`]) mounts *unguarded* generic CRUD on every
//! entity: it can flip a Work Order to `completed` with zero GL posts, zero material consumption, and
//! a stranded non-zero WIP balance. That bypasses every manufacturing invariant.
//!
//! This module is the honest counterpart: a command router that forwards to
//! [`ManufacturingWriteService`] — release → consume → operate → receive — so each state transition
//! emits the WIP/FG postings it must, and WIP nets to zero on completion.
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
    routing::post,
    Json, Router,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::application::service::{
    manufacturing_events::ManufacturingEventSink,
    manufacturing_gl::GlPostSink,
    manufacturing_ports::InventoryPort,
    manufacturing_write_service::{ManufacturingError, ManufacturingWriteService, NewJobCard},
};

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
        // Work-order lifecycle: draft → released → in_process → completed.
        .route("/work-orders/:id/release", post(release_work_order))
        .route("/work-orders/:id/consume", post(consume_materials))
        .route("/work-orders/:id/job-cards", post(add_job_card))
        .route("/work-orders/:id/receive", post(receive_finished))
        // Job-card completion charges conversion cost to WIP (operate).
        .route("/job-cards/:id/complete", post(complete_job_card))
}

// ---------------------------------------------------------------------------
// Handlers — thin forwarders; ManufacturingWriteService does all the work.
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ReleaseResponse {
    pub work_order_id: Uuid,
    pub released: bool,
}

/// Release a draft Work Order: explode its BOM into required materials (draft → released).
pub async fn release_work_order(
    State(deps): State<ManufacturingWriteDeps>,
    Path(id): Path<Uuid>,
) -> Result<Json<ReleaseResponse>, (StatusCode, String)> {
    deps.write_service
        .release_work_order(id, &*deps.events)
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(ReleaseResponse {
        work_order_id: id,
        released: true,
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

/// Issue all required materials to WIP (released → in_process): Dr WIP · Cr Raw-Material Stock.
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
    /// Tenant owner of the work order. In production prefer the authenticated principal's company,
    /// not a request body field — kept here as the reference shape.
    pub company_id: Uuid,
    pub operation_id: Uuid,
    pub workstation_id: Uuid,
    pub total_time_mins: Decimal,
    pub hour_rate: Decimal,
}

#[derive(Serialize)]
pub struct JobCardResponse {
    pub job_card_id: Uuid,
}

/// Open a job card for an operation run against this work order.
pub async fn add_job_card(
    State(deps): State<ManufacturingWriteDeps>,
    Path(work_order_id): Path<Uuid>,
    Json(body): Json<AddJobCardBody>,
) -> Result<Json<JobCardResponse>, (StatusCode, String)> {
    let id = deps
        .write_service
        .add_job_card(NewJobCard {
            company_id: body.company_id,
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
pub struct CompleteResponse {
    /// Conversion cost charged to WIP (time/60 × hour rate).
    pub operating_cost: Decimal,
}

/// Complete a job card: charge conversion cost to WIP — Dr WIP · Cr Conversion-Applied.
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

#[derive(Deserialize)]
pub struct ReceiveBody {
    /// Quantity to receive as finished goods (bounded by the ordered quantity).
    pub quantity: Decimal,
}

#[derive(Serialize)]
pub struct ReceiveResponse {
    pub finished_value: Decimal,
    /// true if the work order is now fully produced (WIP nets to zero).
    pub completed: bool,
    /// true if a concurrent receive already won this quantity (idempotent).
    pub already: bool,
}

/// Receive finished goods into stock at cost (Dr Finished-Goods · Cr WIP); completes the WO on full
/// receipt, clearing WIP to zero.
pub async fn receive_finished(
    State(deps): State<ManufacturingWriteDeps>,
    Path(id): Path<Uuid>,
    Json(body): Json<ReceiveBody>,
) -> Result<Json<ReceiveResponse>, (StatusCode, String)> {
    let out = deps
        .write_service
        .receive_finished(id, body.quantity, &*deps.inventory, &*deps.gl, &*deps.events)
        .await
        .map_err(map_mfg_error)?;
    Ok(Json(ReceiveResponse {
        finished_value: out.finished_value,
        completed: out.completed,
        already: out.already,
    }))
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

fn map_mfg_error(e: ManufacturingError) -> (StatusCode, String) {
    use ManufacturingError::*;
    let status = match &e {
        NotFound(_) => StatusCode::NOT_FOUND,
        InvalidState(_) | OverProduce { .. } | DuplicateNumber(_) => StatusCode::CONFLICT,
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
