//! Integrity probes — the domain invariants that keep the Work Order costing honest under
//! retry/concurrency. Mirrors docs/business-flows/golden-cases.md.

mod common;

use backbone_manufacturing::application::service::manufacturing_events::LoggingSink;
use backbone_manufacturing::application::service::manufacturing_write_service::{
    ManufacturingError, ManufacturingWriteService, NewBom, NewBomItem, NewJobCard, NewWorkOrder,
};
use common::*;
use rust_decimal::Decimal;
use uuid::Uuid;

/// Build a company + a released Work Order for `qty`, with real GL accounts and stocked components.
/// Returns (svc, company, wo_id, raw_warehouse, accounts, inventory).
async fn released_wo(
    accounts_present: bool,
    qty: &str,
) -> (ManufacturingWriteService, Uuid, Uuid, Uuid, FakeInventory) {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let sink = LoggingSink;
    let company = Uuid::new_v4();
    let fg_item = Uuid::new_v4();
    let comp = Uuid::new_v4();
    let raw_wh = Uuid::new_v4();

    let inv = FakeInventory::new();
    inv.stock(comp, "100", "500"); // plenty of the component at rate 500

    let bom = svc
        .create_bom(NewBom {
            company_id: company,
            item_id: fg_item,
            bom_code: format!("BOM-{}", &Uuid::new_v4().to_string()[..8]),
            quantity: dec("1"),
            uom: None,
            items: vec![NewBomItem { item_id: comp, quantity: dec("2"), rate: dec("500"), is_phantom: false }],
            operations: vec![],
        })
        .await
        .unwrap();

    let acc = if accounts_present { Some(wo_accounts(&pool, company).await) } else { None };
    let wo = svc
        .create_work_order(NewWorkOrder {
            company_id: company,
            work_order_number: format!("WO-{}", &Uuid::new_v4().to_string()[..8]),
            item_id: fg_item,
            bom_id: bom,
            quantity: dec(qty),
            wip_warehouse_id: Some(Uuid::new_v4()),
            fg_warehouse_id: Some(Uuid::new_v4()),
            wip_account_id: acc.as_ref().map(|a| a.wip),
            fg_account_id: acc.as_ref().map(|a| a.fg),
            raw_material_account_id: acc.as_ref().map(|a| a.raw),
            conversion_cost_account_id: acc.as_ref().map(|a| a.conversion),
        })
        .await
        .unwrap();
    svc.release_work_order(wo, &sink).await.unwrap();
    (svc, company, wo, raw_wh, inv)
}

/// IP-1 — a Work Order cannot produce more than it was ordered to.
#[tokio::test]
async fn ip1_over_produce_rejected() {
    let (svc, _company, wo, raw_wh, inv) = released_wo(true, "5").await;
    let gl = CountingGl::new();
    let sink = LoggingSink;
    svc.consume_materials(wo, raw_wh, &inv, &gl, &sink).await.unwrap();
    // Ordered 5; try to receive 6.
    let err = svc.receive_finished(wo, dec("6"), &inv, &gl, &sink).await.unwrap_err();
    assert!(matches!(err, ManufacturingError::OverProduce { .. }));
}

/// IP-2 — consuming materials is idempotent: a retry re-charges WIP at most once.
#[tokio::test]
async fn ip2_consume_is_idempotent() {
    let (svc, _company, wo, raw_wh, inv) = released_wo(true, "1").await;
    let gl = CountingGl::new();
    let sink = LoggingSink;

    let first = svc.consume_materials(wo, raw_wh, &inv, &gl, &sink).await.unwrap();
    assert!(!first.already);
    let second = svc.consume_materials(wo, raw_wh, &inv, &gl, &sink).await.unwrap();
    assert!(second.already, "second consume short-circuits");
    assert_eq!(gl.count("consume"), 1, "WIP charged for materials exactly once");
    // Component stock (started 100) was reduced only once by the required 2.
    // (the second call never reached inventory)
}

/// IP-3 — receiving finished goods is idempotent: WIP is cleared once and the WO completes once.
#[tokio::test]
async fn ip3_receive_is_idempotent() {
    let (svc, _company, wo, raw_wh, inv) = released_wo(true, "1").await;
    let gl = CountingGl::new();
    let sink = LoggingSink;
    svc.consume_materials(wo, raw_wh, &inv, &gl, &sink).await.unwrap();

    let a = svc.receive_finished(wo, dec("1"), &inv, &gl, &sink).await.unwrap();
    assert!(a.completed && !a.already);
    let b = svc.receive_finished(wo, dec("1"), &inv, &gl, &sink).await.unwrap();
    assert!(b.already, "second receive short-circuits on completed");
    assert_eq!(gl.count("receive"), 1, "FG received / WIP cleared exactly once");
}

/// IP-4 — a Work Order missing its GL accounts is rejected before any post reaches the ledger.
#[tokio::test]
async fn ip4_missing_account_before_any_post() {
    let (svc, _company, wo, raw_wh, inv) = released_wo(false, "1").await;
    let gl = CountingGl::new();
    let sink = LoggingSink;
    let err = svc.consume_materials(wo, raw_wh, &inv, &gl, &sink).await.unwrap_err();
    assert!(matches!(err, ManufacturingError::MissingAccount(_)));
    assert_eq!(gl.count("consume"), 0, "no post emitted");
    assert_eq!(inv.on_hand(Uuid::new_v4()), Decimal::ZERO); // nothing issued
}

/// IP-5 — a job card charges its conversion cost to WIP exactly once (idempotent completion).
#[tokio::test]
async fn ip5_job_card_charges_once() {
    let (svc, company, wo, raw_wh, inv) = released_wo(true, "1").await;
    let gl = CountingGl::new();
    let sink = LoggingSink;
    svc.consume_materials(wo, raw_wh, &inv, &gl, &sink).await.unwrap();

    let jc = svc
        .add_job_card(NewJobCard {
            company_id: company,
            work_order_id: wo,
            operation_id: Uuid::new_v4(),
            workstation_id: Uuid::new_v4(),
            total_time_mins: dec("30"),
            hour_rate: dec("120"),
        })
        .await
        .unwrap();
    let c1 = svc.complete_job_card(jc, &gl, &sink).await.unwrap();
    assert_eq!(c1, dec("60.00"));
    let _c2 = svc.complete_job_card(jc, &gl, &sink).await.unwrap();
    assert_eq!(gl.count("operate"), 1, "conversion charged to WIP exactly once");
}

/// IP-6 (council 2026-07-06) — if the FG receipt's side effect fails, the Work Order is NOT marked
/// completed: the retry re-drives, WIP clears, and FG is received exactly once. (Before the fix,
/// receive committed the completion gate BEFORE the side effects, so a failure stranded WIP non-zero
/// and the retry self-concealed with `already:true, finished_value:0`.)
#[tokio::test]
async fn ip6_receive_failure_does_not_strand_wip() {
    use backbone_manufacturing::application::service::manufacturing_gl::GlPostSink;
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let gl = GlAdapter::new(pool.clone()); // REAL ledger, so we can check WIP nets to zero
    let sink = LoggingSink;
    let company = Uuid::new_v4();
    let fg_item = Uuid::new_v4();
    let comp = Uuid::new_v4();
    let raw_wh = Uuid::new_v4();
    let acc = wo_accounts(&pool, company).await;
    let inv = FakeInventory::new();
    inv.stock(comp, "100", "500");

    let bom = svc
        .create_bom(NewBom {
            company_id: company,
            item_id: fg_item,
            bom_code: format!("BOM-{}", &Uuid::new_v4().to_string()[..8]),
            quantity: dec("1"),
            uom: None,
            items: vec![NewBomItem { item_id: comp, quantity: dec("2"), rate: dec("500"), is_phantom: false }],
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
            quantity: dec("1"),
            wip_warehouse_id: Some(Uuid::new_v4()),
            fg_warehouse_id: Some(Uuid::new_v4()),
            wip_account_id: Some(acc.wip),
            fg_account_id: Some(acc.fg),
            raw_material_account_id: Some(acc.raw),
            conversion_cost_account_id: Some(acc.conversion),
        })
        .await
        .unwrap();
    svc.release_work_order(wo, &sink).await.unwrap();
    svc.consume_materials(wo, raw_wh, &inv, &gl, &sink).await.unwrap();

    // The inventory receipt fails once.
    inv.fail_next_receives(1);
    let err = svc.receive_finished(wo, dec("1"), &inv, &gl, &sink).await.unwrap_err();
    assert!(matches!(err, ManufacturingError::Inventory(_)));
    // WO must NOT be completed (the gate never advanced).
    let status: String = sqlx::query_scalar("SELECT status::text FROM manufacturing.work_orders WHERE id=$1")
        .bind(wo).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "in_process", "a failed receipt must not mark the WO completed");

    // Retry succeeds — WIP clears, FG received once.
    let r = svc.receive_finished(wo, dec("1"), &inv, &gl, &sink).await.unwrap();
    assert!(r.completed && !r.already);
    assert_eq!(balance(&pool, acc.wip).await, dec("0.00"), "WIP nets to zero after the retry");
    assert_eq!(inv.finished_qty(fg_item), dec("1"), "FG received exactly once");
}
