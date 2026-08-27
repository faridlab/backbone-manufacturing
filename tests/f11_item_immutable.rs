//! F11 backstop probe: a work order's item is immutable once the order leaves draft. The trigger
//! is the DB-level guard beneath the application layer — repointing production at another item
//! after confirm would strand consumed components against the wrong product.

mod common;

use backbone_manufacturing::application::service::manufacturing_events::LoggingSink;
use backbone_manufacturing::application::service::manufacturing_write_service::{
    ManufacturingWriteService, NewBom, NewBomItem, NewWorkOrder,
};
use common::*;
use uuid::Uuid;

async fn draft_wo(svc: &ManufacturingWriteService, company: Uuid) -> Uuid {
    let fg_item = Uuid::new_v4();
    let comp = Uuid::new_v4();
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
    svc.create_work_order(NewWorkOrder {
        company_id: company,
        work_order_number: format!("WO-{}", &Uuid::new_v4().to_string()[..8]),
        item_id: fg_item,
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
    .unwrap()
}

async fn wo_item(pool: &sqlx::PgPool, wo: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT item_id FROM manufacturing.work_orders WHERE id=$1")
        .bind(wo)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// F11-1 — while DRAFT the item may still be corrected (the order has consumed nothing).
#[tokio::test]
async fn f11_draft_item_mutable() {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let company = Uuid::new_v4();
    let wo = draft_wo(&svc, company).await;
    let replacement = Uuid::new_v4();
    let n = sqlx::query("UPDATE manufacturing.work_orders SET item_id=$2 WHERE id=$1")
        .bind(wo)
        .bind(replacement)
        .execute(&pool)
        .await
        .unwrap()
        .rows_affected();
    assert_eq!(n, 1, "draft item correction is allowed");
    assert_eq!(wo_item(&pool, wo).await, replacement);
}

/// F11-2 — after CONFIRM the database refuses the change loudly (P0001 raised exception).
#[tokio::test]
async fn f11_confirmed_item_immutable() {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let company = Uuid::new_v4();
    let sink = LoggingSink;
    let wo = draft_wo(&svc, company).await;
    svc.confirm_work_order(wo, &sink).await.unwrap();
    let err = sqlx::query("UPDATE manufacturing.work_orders SET item_id=$2 WHERE id=$1")
        .bind(wo)
        .bind(Uuid::new_v4())
        .execute(&pool)
        .await
        .expect_err("the trigger must raise");
    let msg = err.to_string();
    assert!(msg.contains("immutable"), "a LOUD refusal naming the rule, got: {msg}");
    // The row kept its original item.
    let original: Uuid = sqlx::query_scalar(
        "SELECT item_id FROM manufacturing.boms WHERE id=(SELECT bom_id FROM manufacturing.work_orders WHERE id=$1)",
    )
    .bind(wo)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(wo_item(&pool, wo).await, original, "item unchanged after the refusal");
}
