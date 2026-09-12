//! Unbuild golden cases: the source-done guard, the over-remaining guard, the reversal's estates
//! and its mirror post (Dr Raw Σ · Cr FG Σ), idempotent re-execute, and that the source work
//! order is NEVER touched.

mod common;

use backbone_manufacturing::application::service::manufacturing_events::LoggingSink;
use backbone_manufacturing::application::service::manufacturing_write_service::{
    ManufacturingError, ManufacturingWriteService, NewBom, NewBomItem, NewJobCard, NewUnbuild,
    NewWorkOrder,
};
use common::*;
use rust_decimal::Decimal;
use uuid::Uuid;

/// A done work order of `qty` FG whose component costs 100/unit and whose job card costs 30,
/// fully received into `inv`'s finished estate. Value locked in FG = qty × 103.
/// Returns (svc, pool, wo, fg_item, comp, accounts).
async fn done_wo(
    qty: &str,
    inv: &FakeInventory,
) -> (ManufacturingWriteService, sqlx::PgPool, Uuid, Uuid, Uuid, WoAccounts) {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let sink = LoggingSink;
    let fg_item = Uuid::new_v4();
    let comp = Uuid::new_v4();
    let acc = wo_accounts(&pool).await;
    inv.stock(comp, qty, "100"); // exactly the BoM's requirement (1/unit)

    let bom = svc
        .create_bom(NewBom {
            item_id: fg_item,
            bom_code: format!("BOM-{}", &Uuid::new_v4().to_string()[..8]),
            quantity: dec("1"),
            uom: None,
            items: vec![NewBomItem { item_id: comp, quantity: dec("1"), rate: dec("100"), is_phantom: false }],
            operations: vec![],
        })
        .await
        .unwrap();
    let wo = svc
        .create_work_order(NewWorkOrder {
            work_order_number: format!("WO-{}", &Uuid::new_v4().to_string()[..8]),
            item_id: fg_item,
            bom_id: bom,
            product_category_id: None,
            quantity: dec(qty),
            wip_warehouse_id: Some(Uuid::new_v4()),
            fg_warehouse_id: Some(Uuid::new_v4()),
            wip_account_id: Some(acc.wip),
            fg_account_id: Some(acc.fg),
            raw_material_account_id: Some(acc.raw),
            conversion_cost_account_id: Some(acc.conversion),
        })
        .await
        .unwrap();
    svc.confirm_work_order(wo, &sink).await.unwrap();
    let gl = GlAdapter::new(pool.clone());
    svc.consume_materials(wo, Uuid::new_v4(), inv, &gl, &sink).await.unwrap();
    let jc = svc
        .add_job_card(NewJobCard {
            work_order_id: wo,
            operation_id: Uuid::new_v4(),
            workstation_id: Uuid::new_v4(),
            total_time_mins: dec("30"),
            hour_rate: dec("60"), // 30min × 60/h = 30 operating
        })
        .await
        .unwrap();
    svc.complete_job_card(jc, &gl, &sink).await.unwrap();
    let out = svc.receive_finished(wo, rcv(dec(qty)), inv, &gl, &sink).await.unwrap();
    assert!(out.completed, "the source order must be fully produced");
    (svc, pool, wo, fg_item, comp, acc)
}

async fn wo_status(pool: &sqlx::PgPool, wo: Uuid) -> String {
    sqlx::query_scalar("SELECT status::text FROM manufacturing.work_orders WHERE id=$1")
        .bind(wo)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// UG-1 — the source work order must be DONE: a confirmed-but-unproduced order is refused LOUDLY.
#[tokio::test]
async fn ug1_source_not_done_refused() {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let sink = LoggingSink;
    let fg_item = Uuid::new_v4();
    let comp = Uuid::new_v4();
    let inv = FakeInventory::new();
    inv.stock(comp, "1000", "100");
    let bom = svc
        .create_bom(NewBom {
            item_id: fg_item,
            bom_code: format!("BOM-{}", &Uuid::new_v4().to_string()[..8]),
            quantity: dec("1"),
            uom: None,
            items: vec![NewBomItem { item_id: comp, quantity: dec("1"), rate: dec("100"), is_phantom: false }],
            operations: vec![],
        })
        .await
        .unwrap();
    let wo = svc
        .create_work_order(NewWorkOrder {
            work_order_number: format!("WO-{}", &Uuid::new_v4().to_string()[..8]),
            item_id: fg_item,
            bom_id: bom,
            product_category_id: None,
            quantity: dec("10"),
            wip_warehouse_id: Some(Uuid::new_v4()),
            fg_warehouse_id: Some(Uuid::new_v4()),
            wip_account_id: None,
            fg_account_id: None,
            raw_material_account_id: None,
            conversion_cost_account_id: None,
        })
        .await
        .unwrap();
    svc.confirm_work_order(wo, &sink).await.unwrap(); // confirmed, NOT done

    let unbuild = svc
        .create_unbuild(NewUnbuild {
            unbuild_number: format!("UB-{}", &Uuid::new_v4().to_string()[..8]),
            work_order_id: wo,
            item_id: fg_item,
            quantity: dec("1"),
        })
        .await
        .unwrap();
    let gl = CountingGl::new();
    let err = svc
        .execute_unbuild(unbuild, Uuid::new_v4(), &inv, &gl, &sink)
        .await
        .unwrap_err();
    assert!(
        matches!(err, ManufacturingError::UnbuildSourceNotDone),
        "a not-done source is refused LOUDLY, got {err:?}"
    );
    // And the draft stays re-executable: the guard did not consume it.
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status::text FROM manufacturing.unbuild_orders WHERE id=$1")
            .bind(unbuild)
            .fetch_one(&pool)
            .await
            .unwrap(),
        "draft"
    );
}

/// UG-2 — the requested quantity may not exceed what remains after other done unbuilds.
#[tokio::test]
async fn ug2_over_remaining_refused() {
    let inv = FakeInventory::new();
    let (svc, pool, wo, fg_item, _comp, _acc) = done_wo("10", &inv).await;
    let sink = LoggingSink;
    let gl = GlAdapter::new(pool.clone());
    let raw_wh = Uuid::new_v4();

    // Over the whole batch → LOUD.
    let too_much = svc
        .create_unbuild(NewUnbuild {
            unbuild_number: format!("UB-{}", &Uuid::new_v4().to_string()[..8]),
            work_order_id: wo,
            item_id: fg_item,
            quantity: dec("11"),
        })
        .await
        .unwrap();
    let err = svc.execute_unbuild(too_much, raw_wh, &inv, &gl, &sink).await.unwrap_err();
    assert!(
        matches!(&err, ManufacturingError::UnbuildOverRemaining { requested, remaining }
            if *requested == dec("11") && *remaining == dec("10")),
        "over-remaining is LOUD, got {err:?}"
    );

    // 6 unbuilt → only 4 remain; asking for 5 is LOUD.
    let first = svc
        .create_unbuild(NewUnbuild {
            unbuild_number: format!("UB-{}", &Uuid::new_v4().to_string()[..8]),
            work_order_id: wo,
            item_id: fg_item,
            quantity: dec("6"),
        })
        .await
        .unwrap();
    svc.execute_unbuild(first, raw_wh, &inv, &gl, &sink).await.unwrap();
    let second = svc
        .create_unbuild(NewUnbuild {
            unbuild_number: format!("UB-{}", &Uuid::new_v4().to_string()[..8]),
            work_order_id: wo,
            item_id: fg_item,
            quantity: dec("5"),
        })
        .await
        .unwrap();
    let err = svc.execute_unbuild(second, raw_wh, &inv, &gl, &sink).await.unwrap_err();
    assert!(
        matches!(&err, ManufacturingError::UnbuildOverRemaining { remaining, .. } if *remaining == dec("4")),
        "remaining accounts for done unbuilds, got {err:?}"
    );
}

/// UG-3 — the reversal moves estates and posts the mirror: Dr Raw 412 · Cr FG 412 for a 4/10
/// share of a 1,030 batch; the components return to raw stock.
#[tokio::test]
async fn ug3_reversal_estates_and_post() {
    let inv = FakeInventory::new();
    let (svc, pool, wo, fg_item, comp, acc) = done_wo("10", &inv).await;
    let sink = LoggingSink;
    let gl = GlAdapter::new(pool.clone());
    let raw_wh = Uuid::new_v4();
    let before_fg = balance(&pool, acc.fg).await; // 1,030
    let before_raw = balance(&pool, acc.raw).await; // -1,000
    assert_eq!(before_fg, dec("1030.00"));
    assert_eq!(before_raw, dec("-1000.00"));

    let unbuild = svc
        .create_unbuild(NewUnbuild {
            unbuild_number: format!("UB-{}", &Uuid::new_v4().to_string()[..8]),
            work_order_id: wo,
            item_id: fg_item,
            quantity: dec("4"),
        })
        .await
        .unwrap();
    let out = svc.execute_unbuild(unbuild, raw_wh, &inv, &gl, &sink).await.unwrap();
    assert!(!out.already);
    assert_eq!(out.reversed_value, dec("412.00"), "4/10 share of (1,000 + 30)");

    // GL mirror: raw debited 412, fg credited 412 — no value created or destroyed.
    assert_eq!(balance(&pool, acc.raw).await, dec("-588.00"));
    assert_eq!(balance(&pool, acc.fg).await, dec("618.00"));
    assert_eq!(balance(&pool, acc.wip).await, dec("0.00"), "WIP stays netted");
    // Stock: 4 FG leave the finished estate, 4 components return to raw.
    assert_eq!(inv.finished_qty(fg_item), dec("6"));
    assert_eq!(inv.on_hand(comp), dec("4"), "components returned to raw stock");
}

/// UG-4 — re-executing a done unbuild is an idempotent no-op: no second reversal, no second post.
#[tokio::test]
async fn ug4_reexecute_idempotent() {
    let inv = FakeInventory::new();
    let (svc, pool, wo, fg_item, _comp, acc) = done_wo("10", &inv).await;
    let sink = LoggingSink;
    let gl = GlAdapter::new(pool.clone());
    let unbuild = svc
        .create_unbuild(NewUnbuild {
            unbuild_number: format!("UB-{}", &Uuid::new_v4().to_string()[..8]),
            work_order_id: wo,
            item_id: fg_item,
            quantity: dec("4"),
        })
        .await
        .unwrap();
    let raw_wh = Uuid::new_v4();
    svc.execute_unbuild(unbuild, raw_wh, &inv, &gl, &sink).await.unwrap();
    let again = svc.execute_unbuild(unbuild, raw_wh, &inv, &gl, &sink).await.unwrap();
    assert!(again.already, "a done unbuild short-circuits");
    assert_eq!(again.reversed_value, dec("0.00"));
    assert_eq!(balance(&pool, acc.fg).await, dec("618.00"), "unchanged by the retry");
    assert_eq!(balance(&pool, acc.raw).await, dec("-588.00"));
}

/// UG-5 — the source work order is NEVER written: it stays done with its accumulators untouched.
#[tokio::test]
async fn ug5_source_work_order_untouched() {
    let inv = FakeInventory::new();
    let (svc, pool, wo, fg_item, _comp, _acc) = done_wo("10", &inv).await;
    let sink = LoggingSink;
    let gl = GlAdapter::new(pool.clone());
    let (produced, raw_cost, op_cost): (Decimal, Decimal, Decimal) = sqlx::query_as(
        "SELECT produced_qty, raw_material_cost, operating_cost FROM manufacturing.work_orders WHERE id=$1",
    )
    .bind(wo)
    .fetch_one(&pool)
    .await
    .unwrap();
    let unbuild = svc
        .create_unbuild(NewUnbuild {
            unbuild_number: format!("UB-{}", &Uuid::new_v4().to_string()[..8]),
            work_order_id: wo,
            item_id: fg_item,
            quantity: dec("4"),
        })
        .await
        .unwrap();
    svc.execute_unbuild(unbuild, Uuid::new_v4(), &inv, &gl, &sink).await.unwrap();
    assert_eq!(wo_status(&pool, wo).await, "done", "the source stays done");
    let (p2, r2, o2): (Decimal, Decimal, Decimal) = sqlx::query_as(
        "SELECT produced_qty, raw_material_cost, operating_cost FROM manufacturing.work_orders WHERE id=$1",
    )
    .bind(wo)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((p2, r2, o2), (produced, raw_cost, op_cost), "accumulators untouched");
}
