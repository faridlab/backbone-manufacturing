//! BoM versioning + staging: create_bom pins version 1 and bom_type 'normal'; revisions stage by
//! version bump and coexist per item. The one-live-BoM-per-(item, version) slot is installed by
//! the composing service's tenancy decorator as an org-scoped unique (ADR-0029) — the module
//! declares no such unique, so undecorated module tests must NOT see collisions.

mod common;

use backbone_manufacturing::application::service::manufacturing_events::LoggingSink;
use backbone_manufacturing::application::service::manufacturing_write_service::{
    ManufacturingWriteService, NewBom, NewBomItem, NewWorkOrder,
};
use common::*;
use uuid::Uuid;

async fn bom_for(svc: &ManufacturingWriteService, item: Uuid, code: &str) -> Uuid {
    svc.create_bom(NewBom {
        item_id: item,
        bom_code: code.into(),
        quantity: dec("1"),
        uom: None,
        items: vec![NewBomItem { item_id: Uuid::new_v4(), quantity: dec("1"), rate: dec("10"), is_phantom: false }],
        operations: vec![],
    })
    .await
    .unwrap()
}

/// BV-1 — a fresh BoM lands at version 1, bom_type 'normal'.
#[tokio::test]
async fn bv1_create_pins_v1_normal() {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let item = Uuid::new_v4();
    let bom = bom_for(&svc, item, &format!("BOM-{}", &Uuid::new_v4().to_string()[..8])).await;
    let (version, bom_type): (i32, String) = sqlx::query_as(
        "SELECT version, bom_type::text FROM manufacturing.boms WHERE id=$1",
    )
    .bind(bom)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(version, 1);
    assert_eq!(bom_type, "normal");
}

/// BV-2 — the module declares no (item, version) slot unique: undecorated, a second v1 for the
/// same item inserts. This is deliberate (ADR-0029) — the collision guard is the composing
/// service's decorator (org-scoped unique), probed in the composition's suite, never here.
#[tokio::test]
async fn bv2_undecorated_second_v1_inserts_the_decorator_owns_the_slot() {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let item = Uuid::new_v4();
    bom_for(&svc, item, &format!("BOM-{}", &Uuid::new_v4().to_string()[..8])).await;
    let second = svc
        .create_bom(NewBom {
            item_id: item,
            bom_code: format!("BOM-{}", &Uuid::new_v4().to_string()[..8]),
            quantity: dec("1"),
            uom: None,
            items: vec![NewBomItem { item_id: Uuid::new_v4(), quantity: dec("1"), rate: dec("10"), is_phantom: false }],
            operations: vec![],
        })
        .await;
    assert!(
        matches!(second, Ok(_)),
        "no module unique holds the (item, version) slot — got {:?}",
        second.err()
    );
}

/// BV-3 — revisions stage by version bump and coexist per item.
#[tokio::test]
async fn bv3_version_bump_coexists() {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let item = Uuid::new_v4();
    let v1 = bom_for(&svc, item, &format!("BOM-{}", &Uuid::new_v4().to_string()[..8])).await;
    sqlx::query("UPDATE manufacturing.boms SET version=2 WHERE id=$1")
        .bind(v1)
        .execute(&pool)
        .await
        .unwrap();
    let v1b = bom_for(&svc, item, &format!("BOM-{}", &Uuid::new_v4().to_string()[..8])).await;
    let versions: Vec<i32> = sqlx::query_scalar("SELECT version FROM manufacturing.boms WHERE item_id=$1 ORDER BY version")
        .bind(item)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(versions, vec![1, 2], "both revisions coexist");
    let _ = v1b;
}

/// BV-4 — the stage is queryable per work-order consumption: a kit/subcontract BoM never reaches
/// confirm (covered in kit_no_mo / subcontract_golden_cases) — here the plain contract that a
/// NORMAL BoM's work order confirms fine at v1.
#[tokio::test]
async fn bv4_normal_bom_confirms() {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let sink = LoggingSink;
    let item = Uuid::new_v4();
    let bom = bom_for(&svc, item, &format!("BOM-{}", &Uuid::new_v4().to_string()[..8])).await;
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
    svc.confirm_work_order(wo, &sink).await.expect("a normal BoM confirms");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manufacturing.work_order_items WHERE work_order_id=$1")
        .bind(wo)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "the explosion wrote its requirement");
}
