//! State-convergence probes — the hybrid 6-state vocabulary: three direct writes (confirm /
//! mark-done-by-receive / cancel) and the derived states (progress by consume; to_close / done by
//! the receive accumulator). Sticky cancel; no field named `availability` anywhere.

mod common;

use backbone_manufacturing::application::service::manufacturing_events::LoggingSink;
use backbone_manufacturing::application::service::manufacturing_write_service::{
    ManufacturingError, ManufacturingWriteService, NewBom, NewBomItem, NewJobCard, NewWorkOrder,
};
use common::*;
use uuid::Uuid;

async fn wo_at(qty: &str) -> (ManufacturingWriteService, sqlx::PgPool, Uuid, Uuid, Uuid) {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let company = Uuid::new_v4();
    let fg_item = Uuid::new_v4();
    let comp = Uuid::new_v4();
    let acc = wo_accounts(&pool, company).await;
    let inv = FakeInventory::new();
    inv.stock(comp, "1000", "10");
    let _ = inv;
    let _ = acc;
    let bom = svc
        .create_bom(NewBom {
            company_id: company,
            item_id: fg_item,
            bom_code: format!("BOM-{}", &Uuid::new_v4().to_string()[..8]),
            quantity: dec("1"),
            uom: None,
            items: vec![NewBomItem { item_id: comp, quantity: dec("1"), rate: dec("10"), is_phantom: false }],
            operations: vec![],
        })
        .await
        .unwrap();
    let wo = svc
        .create_work_order(NewWorkOrder {
            company_id: company,
            work_order_number: format!("WO-{}", &Uuid::new_v4().to_string()[..8]),
            item_id: fg_item,
            bom_id: bom,
            product_category_id: None,
            quantity: dec(qty),
            wip_warehouse_id: Some(Uuid::new_v4()),
            fg_warehouse_id: Some(Uuid::new_v4()),
            wip_account_id: None,
            fg_account_id: None,
            raw_material_account_id: None,
            conversion_cost_account_id: None,
        })
        .await
        .unwrap();
    (svc, pool, wo, company, comp)
}

async fn status(pool: &sqlx::PgPool, wo: Uuid) -> String {
    sqlx::query_scalar("SELECT status::text FROM manufacturing.work_orders WHERE id=$1")
        .bind(wo)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn jc_status(pool: &sqlx::PgPool, id: Uuid) -> String {
    sqlx::query_scalar::<_, String>("SELECT status::text FROM manufacturing.job_cards WHERE id=$1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// SC-1 — a new work order is `draft` and consumes nothing.
#[tokio::test]
async fn sc1_new_wo_is_draft() {
    let (svc, pool, wo, _c, _i) = wo_at("2").await;
    let _ = &svc;
    assert_eq!(status(&pool, wo).await, "draft");
}

/// SC-2 — confirm is a direct write: draft → confirmed; a repeat is LOUD.
#[tokio::test]
async fn sc2_confirm_direct_write() {
    let (svc, pool, wo, _c, _i) = wo_at("2").await;
    let sink = LoggingSink;
    svc.confirm_work_order(wo, &sink).await.unwrap();
    assert_eq!(status(&pool, wo).await, "confirmed");
    let err = svc.confirm_work_order(wo, &sink).await.unwrap_err();
    assert!(matches!(err, ManufacturingError::InvalidState(_)));
}

/// SC-3 — progress is DERIVED by the consume gate; a partial receipt derives to_close; the full
/// one derives done. No verb writes progress/to_close/done directly.
#[tokio::test]
async fn sc3_derived_states() {
    let (svc, pool, wo, company, comp) = wo_at("4").await;
    let sink = LoggingSink;
    let acc = wo_accounts(&pool, company).await;
    let inv = FakeInventory::new();
    inv.stock(comp, "1000", "10");
    let gl = CountingGl::new();
    // Put accounts on the order? The order was created without overrides — set them via defaults
    // seeding on a category? Simpler: this WO was created without accounts; patch them on.
    sqlx::query("UPDATE manufacturing.work_orders SET wip_account_id=$2, fg_account_id=$3, raw_material_account_id=$4 WHERE id=$1")
        .bind(wo).bind(acc.wip).bind(acc.fg).bind(acc.raw)
        .execute(&pool).await.unwrap();

    svc.confirm_work_order(wo, &sink).await.unwrap();
    assert_eq!(status(&pool, wo).await, "confirmed");

    svc.consume_materials(wo, Uuid::new_v4(), &inv, &gl, &sink).await.unwrap();
    assert_eq!(status(&pool, wo).await, "progress", "progress is derived by consume");

    svc.receive_finished(wo, rcv(dec("2")), &inv, &gl, &sink).await.unwrap();
    assert_eq!(status(&pool, wo).await, "to_close", "partial receipt derives to_close");

    svc.receive_finished(wo, rcv(dec("2")), &inv, &gl, &sink).await.unwrap();
    assert_eq!(status(&pool, wo).await, "done", "full receipt derives done");
}

/// SC-4 — cancel is direct-write and STICKY: draft|confirmed only, terminal, repeat is LOUD.
#[tokio::test]
async fn sc4_cancel_sticky() {
    let (svc, pool, wo, _c, _i) = wo_at("2").await;
    let sink = LoggingSink;
    svc.cancel_work_order(wo, &sink).await.unwrap();
    assert_eq!(status(&pool, wo).await, "cancel");
    let err = svc.cancel_work_order(wo, &sink).await.unwrap_err();
    assert!(matches!(err, ManufacturingError::InvalidState(_)), "cancel is sticky");
}

/// SC-5 — an order that has consumed (progress) or produced (to_close/done) can NEVER cancel.
#[tokio::test]
async fn sc5_cancel_refused_after_wip() {
    let (svc, pool, wo, company, comp) = wo_at("2").await;
    let sink = LoggingSink;
    let acc = wo_accounts(&pool, company).await;
    let inv = FakeInventory::new();
    inv.stock(comp, "1000", "10");
    let gl = CountingGl::new();
    sqlx::query("UPDATE manufacturing.work_orders SET wip_account_id=$2, fg_account_id=$3, raw_material_account_id=$4 WHERE id=$1")
        .bind(wo).bind(acc.wip).bind(acc.fg).bind(acc.raw)
        .execute(&pool).await.unwrap();
    svc.confirm_work_order(wo, &sink).await.unwrap();
    svc.consume_materials(wo, Uuid::new_v4(), &inv, &gl, &sink).await.unwrap();
    let err = svc.cancel_work_order(wo, &sink).await.unwrap_err();
    assert!(matches!(err, ManufacturingError::InvalidState(_)), "progress cannot cancel");
}

/// SC-6 — job cards: fresh is `ready`, start derives progress, complete derives done once, and
/// cancel is refused on a done card. The parent order must be confirmed before floor work starts.
#[tokio::test]
async fn sc6_job_card_states() {
    let (svc, pool, wo, company, _comp) = wo_at("2").await;
    let sink = LoggingSink;
    let acc = wo_accounts(&pool, company).await;
    // Completion charges conversion cost to WIP — wire the accounts the card will resolve.
    sqlx::query("UPDATE manufacturing.work_orders SET wip_account_id=$2, conversion_cost_account_id=$3 WHERE id=$1")
        .bind(wo).bind(acc.wip).bind(acc.conversion)
        .execute(&pool).await.unwrap();
    svc.confirm_work_order(wo, &sink).await.unwrap();
    let jc = svc
        .add_job_card(NewJobCard {
            company_id: svc_job_card_company(&pool, wo).await,
            work_order_id: wo,
            operation_id: Uuid::new_v4(),
            workstation_id: Uuid::new_v4(),
            total_time_mins: dec("30"),
            hour_rate: dec("60"),
        })
        .await
        .unwrap();
    assert_eq!(jc_status(&pool, jc).await, "ready", "a new job card starts ready");
    svc.start_job_card(jc).await.unwrap();
    assert_eq!(jc_status(&pool, jc).await, "progress");
    let gl = CountingGl::new();
    svc.complete_job_card(jc, &gl, &sink).await.unwrap();
    svc.complete_job_card(jc, &gl, &sink).await.unwrap(); // idempotent repeat
    assert_eq!(jc_status(&pool, jc).await, "done");
    assert_eq!(gl.count("operate"), 1, "conversion charged exactly once");
    let err = svc.cancel_job_card(jc).await.unwrap_err();
    assert!(matches!(err, ManufacturingError::InvalidState(_)), "done card cannot cancel");
}

async fn svc_job_card_company(pool: &sqlx::PgPool, wo: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT company_id FROM manufacturing.work_orders WHERE id=$1")
        .bind(wo)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// SC-7 — availability lives ONLY in reservation_state; no column named `availability` exists.
#[tokio::test]
async fn sc7_no_availability_column() {
    let pool = pool().await;
    let has_availability: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_schema='manufacturing' AND column_name='availability')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!has_availability, "no field named availability exists anywhere");
    // And the projection column exists with the reserved vocabulary.
    let has_projection: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_schema='manufacturing' AND table_name='work_orders' AND column_name='reservation_state')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(has_projection);
}
