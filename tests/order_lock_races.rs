//! Verbs racing on one work order take turns under its lock: a consume and a cancel never both
//! succeed, and two receives never share an idempotency key, so every receipt reaches stock.

mod common;
use backbone_manufacturing::application::service::manufacturing_events::LoggingSink;
use backbone_manufacturing::application::service::manufacturing_write_service::{
    ManufacturingWriteService, NewBom, NewBomItem, NewWorkOrder,
};
use common::*;
use uuid::Uuid;

/// A confirmed order for `qty` units of a one-component BoM, its component in stock.
async fn confirmed_order(svc: &ManufacturingWriteService, inv: &FakeInventory, qty: &str) -> (Uuid, Uuid, Uuid) {
    let pool = pool().await;
    let fg_item = Uuid::new_v4();
    let comp = Uuid::new_v4();
    let acc = wo_accounts(&pool).await;
    inv.stock(comp, "1000", "500");
    let bom = svc
        .create_bom(NewBom {
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
    svc.confirm_work_order(wo, &LoggingSink).await.unwrap();
    (wo, fg_item, comp)
}

async fn status_of(wo: Uuid) -> String {
    sqlx::query_scalar("SELECT status::text FROM manufacturing.work_orders WHERE id = $1")
        .bind(wo)
        .fetch_one(&pool().await)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_consume_and_a_cancel_racing_never_both_succeed() {
    let svc = ManufacturingWriteService::new(pool().await);
    for _ in 0..10 {
        let inv = FakeInventory::new();
        let gl = CountingGl::new();
        let (wo, _fg, comp) = confirmed_order(&svc, &inv, "1").await;
        let (consumed, cancelled) = tokio::join!(
            svc.consume_materials(wo, Uuid::new_v4(), &inv, &gl, &LoggingSink),
            svc.cancel_work_order(wo, &LoggingSink),
        );
        let consumed_ok = consumed.as_ref().is_ok_and(|o| !o.already);
        assert!(consumed_ok != cancelled.is_ok(), "exactly one wins: {consumed:?} / {cancelled:?}");
        if cancelled.is_ok() {
            assert_eq!(status_of(wo).await, "cancel");
            assert_eq!(gl.count("consume"), 0, "a cancelled order carries no WIP debit");
            assert_eq!(inv.on_hand(comp), dec("1000"), "and issued no stock");
        } else {
            assert_eq!(status_of(wo).await, "progress");
        }
    }
}

#[tokio::test]
async fn two_receives_racing_both_reach_stock() {
    let svc = ManufacturingWriteService::new(pool().await);
    for _ in 0..10 {
        let inv = FakeInventory::new();
        let gl = CountingGl::new();
        let (wo, fg, _comp) = confirmed_order(&svc, &inv, "2").await;
        svc.consume_materials(wo, Uuid::new_v4(), &inv, &gl, &LoggingSink).await.unwrap();
        let (a, b) = tokio::join!(
            svc.receive_finished(wo, rcv(dec("1")), &inv, &gl, &LoggingSink),
            svc.receive_finished(wo, rcv(dec("1")), &inv, &gl, &LoggingSink),
        );
        a.unwrap();
        b.unwrap();
        assert_eq!(inv.finished_qty(fg), dec("2"), "both receipts reached stock");
        assert_eq!(gl.count("receive"), 2, "each receipt posted once");
        assert_eq!(status_of(wo).await, "done");
    }
}
