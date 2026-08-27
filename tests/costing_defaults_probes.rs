//! The account-resolution chain probes: per-order override → category costing default → LOUD
//! MissingAccount. No hardcoded fallback exists anywhere — an unresolvable account refuses.

mod common;

use backbone_manufacturing::application::service::manufacturing_events::LoggingSink;
use backbone_manufacturing::application::service::manufacturing_write_service::{
    ManufacturingError, ManufacturingWriteService, NewBom, NewBomItem, NewWorkOrder,
};
use common::*;
use uuid::Uuid;

async fn draft_wo(
    svc: &ManufacturingWriteService,
    pool: &sqlx::PgPool,
    company: Uuid,
    category: Option<Uuid>,
    overrides: Option<WoAccounts>,
) -> Uuid {
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
    let _ = pool;
    svc.create_work_order(NewWorkOrder {
        company_id: company,
        work_order_number: format!("WO-{}", &Uuid::new_v4().to_string()[..8]),
        item_id: fg_item,
        bom_id: bom,
        product_category_id: category,
        quantity: dec("1"),
        wip_warehouse_id: Some(Uuid::new_v4()),
        fg_warehouse_id: Some(Uuid::new_v4()),
        wip_account_id: overrides.as_ref().map(|a| a.wip),
        fg_account_id: overrides.as_ref().map(|a| a.fg),
        raw_material_account_id: overrides.as_ref().map(|a| a.raw),
        conversion_cost_account_id: overrides.as_ref().map(|a| a.conversion),
    })
    .await
    .unwrap()
}

/// CD-1 — no override AND no category default → MissingAccount, LOUD. Never a hardcoded fallback.
#[tokio::test]
async fn cd1_missing_account_loud() {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let company = Uuid::new_v4();
    let sink = LoggingSink;
    let wo = draft_wo(&svc, &pool, company, None, None).await;
    svc.confirm_work_order(wo, &sink).await.unwrap();
    let inv = FakeInventory::new();
    inv.stock(item_of(&pool, wo).await, "10", "10");
    let gl = CountingGl::new();
    let err = svc.consume_materials(wo, Uuid::new_v4(), &inv, &gl, &sink).await.unwrap_err();
    assert!(
        matches!(err, ManufacturingError::MissingAccount(_)),
        "unresolvable account is LOUD, got {err:?}"
    );
    // Nothing was consumed: the guard fired before the port call.
    assert_eq!(inv.on_hand(item_of(&pool, wo).await), dec("10"));
}

async fn item_of(pool: &sqlx::PgPool, wo: Uuid) -> Uuid {
    // The WO's FG item — but the CONSUMED component is what needs stock; fetch the requirement.
    let comp: Uuid = sqlx::query_scalar(
        "SELECT item_id FROM manufacturing.work_order_items WHERE work_order_id=$1 LIMIT 1",
    )
    .bind(wo)
    .fetch_one(pool)
    .await
    .unwrap();
    comp
}

/// CD-2 — the category's costing defaults fill every unset override.
#[tokio::test]
async fn cd2_category_defaults_fill() {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let company = Uuid::new_v4();
    let sink = LoggingSink;
    let category = Uuid::new_v4();
    let dw = account(&pool, company, "1410-DWIP", "asset", "inventory", "debit").await;
    let dr = account(&pool, company, "1400-DRAW", "asset", "inventory", "debit").await;
    seed_costing_defaults(&pool, company, category, Some(dw), None, Some(dr), None, None, None, None, None).await;

    let wo = draft_wo(&svc, &pool, company, Some(category), None).await;
    svc.confirm_work_order(wo, &sink).await.unwrap();
    let comp = item_of(&pool, wo).await;
    let inv = FakeInventory::new();
    inv.stock(comp, "10", "10");
    let gl = GlAdapter::new(pool.clone());
    let out = svc.consume_materials(wo, Uuid::new_v4(), &inv, &gl, &sink).await.unwrap();
    assert_eq!(out.raw_material_value, dec("10.00"));
    // The DEFAULTS carried the post: default-WIP debited, default-RAW credited.
    assert_eq!(balance(&pool, dw).await, dec("10.00"));
    assert_eq!(balance(&pool, dr).await, dec("-10.00"));
}

/// CD-3 — a per-order override beats the category default (distinct account carries the post).
#[tokio::test]
async fn cd3_override_wins_over_default() {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let company = Uuid::new_v4();
    let sink = LoggingSink;
    let category = Uuid::new_v4();
    let dw = account(&pool, company, "1410-DWIP", "asset", "inventory", "debit").await;
    let dr = account(&pool, company, "1400-DRAW", "asset", "inventory", "debit").await;
    seed_costing_defaults(&pool, company, category, Some(dw), None, Some(dr), None, None, None, None, None).await;
    let over = wo_accounts(&pool, company).await; // 1410-WIP / 1400-RAW — distinct ids
    let over_wip = over.wip;
    let over_raw = over.raw;

    let wo = draft_wo(&svc, &pool, company, Some(category), Some(over)).await;
    svc.confirm_work_order(wo, &sink).await.unwrap();
    let comp = item_of(&pool, wo).await;
    let inv = FakeInventory::new();
    inv.stock(comp, "10", "10");
    let gl = GlAdapter::new(pool.clone());
    svc.consume_materials(wo, Uuid::new_v4(), &inv, &gl, &sink).await.unwrap();
    // The OVERRIDES carried the post; the defaults stayed untouched.
    assert_eq!(balance(&pool, over_wip).await, dec("10.00"));
    assert_eq!(balance(&pool, over_raw).await, dec("-10.00"));
    assert_eq!(balance(&pool, dw).await, dec("0.00"), "default not used");
    assert_eq!(balance(&pool, dr).await, dec("0.00"), "default not used");
}

/// CD-4 — a category with a defaults row that leaves the account NULL is still LOUD (a seeded
/// row is not a resolution).
#[tokio::test]
async fn cd4_null_default_still_loud() {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let company = Uuid::new_v4();
    let sink = LoggingSink;
    let category = Uuid::new_v4();
    seed_costing_defaults(&pool, company, category, None, None, None, None, None, None, None, None).await;
    let wo = draft_wo(&svc, &pool, company, Some(category), None).await;
    svc.confirm_work_order(wo, &sink).await.unwrap();
    let inv = FakeInventory::new();
    inv.stock(item_of(&pool, wo).await, "10", "10");
    let gl = CountingGl::new();
    let err = svc.consume_materials(wo, Uuid::new_v4(), &inv, &gl, &sink).await.unwrap_err();
    assert!(matches!(err, ManufacturingError::MissingAccount(_)), "NULL default ≠ resolution");
}
