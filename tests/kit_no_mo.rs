//! A kit BoM NEVER gets its own work order — it explodes through to components at demand time —
//! and a subcontract BoM's orders are minted ONLY by the receipt event. Both refusals are LOUD,
//! at confirm, before any requirement row is written.

mod common;

use backbone_manufacturing::application::service::manufacturing_events::LoggingSink;
use backbone_manufacturing::application::service::manufacturing_write_service::{
    ManufacturingError, ManufacturingWriteService, NewBom, NewBomItem, NewWorkOrder,
};
use common::*;
use uuid::Uuid;

async fn wo_on_typed_bom(bom_type: &str) -> (ManufacturingWriteService, sqlx::PgPool, Uuid) {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let item = Uuid::new_v4();
    let bom = svc
        .create_bom(NewBom {
            item_id: item,
            bom_code: format!("BOM-{}", &Uuid::new_v4().to_string()[..8]),
            quantity: dec("1"),
            uom: None,
            items: vec![NewBomItem { item_id: Uuid::new_v4(), quantity: dec("1"), rate: dec("10"), is_phantom: false }],
            operations: vec![],
        })
        .await
        .unwrap();
    sqlx::query("UPDATE manufacturing.boms SET bom_type=$2::bom_type WHERE id=$1")
        .bind(bom)
        .bind(bom_type)
        .execute(&pool)
        .await
        .unwrap();
    let wo = svc
        .create_work_order(NewWorkOrder {
            work_order_number: format!("WO-{}", &Uuid::new_v4().to_string()[..8]),
            item_id: item,
            bom_id: bom,
            product_category_id: None,
            quantity: dec("1"),
            wip_warehouse_id: None,
            fg_warehouse_id: None,
            wip_account_id: None,
            fg_account_id: None,
            raw_material_account_id: None,
            conversion_cost_account_id: None,
        })
        .await
        .unwrap();
    (svc, pool, wo)
}

/// KIT-1 — confirming a work order on a kit BoM is refused LOUDLY and writes NO requirements.
#[tokio::test]
async fn kit1_confirm_refused() {
    let (svc, pool, wo) = wo_on_typed_bom("kit").await;
    let sink = LoggingSink;
    let err = svc.confirm_work_order(wo, &sink).await.unwrap_err();
    match &err {
        ManufacturingError::Invalid(msg) => assert!(
            msg.contains("kit"),
            "the refusal names the kit rule, got: {msg}"
        ),
        other => panic!("expected Invalid, got {other:?}"),
    }
    let status: String = sqlx::query_scalar("SELECT status::text FROM manufacturing.work_orders WHERE id=$1")
        .bind(wo)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "draft", "refused before any state move");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manufacturing.work_order_items WHERE work_order_id=$1")
        .bind(wo)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0, "no requirement was exploded");
}

/// KIT-2 — a subcontract BoM's orders are minted ONLY by the receipt event: hand-confirm is
/// refused LOUDLY too.
#[tokio::test]
async fn kit2_subcontract_confirm_refused() {
    let (svc, _pool, wo) = wo_on_typed_bom("subcontract").await;
    let sink = LoggingSink;
    let err = svc.confirm_work_order(wo, &sink).await.unwrap_err();
    match &err {
        ManufacturingError::Invalid(msg) => assert!(
            msg.contains("subcontract"),
            "the refusal names the subcontract rule, got: {msg}"
        ),
        other => panic!("expected Invalid, got {other:?}"),
    }
}
