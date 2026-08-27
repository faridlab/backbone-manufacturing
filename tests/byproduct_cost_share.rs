//! Byproduct cost-share split + the subcontract DoD numbers:
//!   components 6,000 + PO 2,500  →  Dr FG 8,500 · Cr WIP 6,000 · Cr Interim 2,500
//! Byproduct legs slice T × cost_share; the FG line keeps the remainder; Σ shares > 100 is LOUD.

mod common;

use backbone_manufacturing::application::service::manufacturing_events::LoggingSink;
use backbone_manufacturing::application::service::manufacturing_gl::GlPostSink;
use backbone_manufacturing::application::service::manufacturing_ports::CostPosture;
use backbone_manufacturing::application::service::manufacturing_write_service::{
    ManufacturingError, ManufacturingWriteService, NewBom, NewBomItem, NewWorkOrder,
    ReceiveByproductLine, ReceiveFinishedOrder,
};
use backbone_manufacturing::infrastructure::persistence::{BomByproductRepository, NewBomByproductRow};
use common::*;
use uuid::Uuid;

/// A confirmed WO for `qty` units whose single component costs `qty × 500` to consume, with real
/// accounts and stocked inventory. Returns (svc, pool, company, wo, comp, accounts).
async fn wo_base(qty: &str) -> (ManufacturingWriteService, sqlx::PgPool, Uuid, Uuid, Uuid, WoAccounts) {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let sink = LoggingSink;
    let company = Uuid::new_v4();
    let fg_item = Uuid::new_v4();
    let comp = Uuid::new_v4();
    let acc = wo_accounts(&pool, company).await;
    let inv = FakeInventory::new();
    inv.stock(comp, "1000", "500");
    let _ = inv;

    let bom = svc
        .create_bom(NewBom {
            company_id: company,
            item_id: fg_item,
            bom_code: format!("BOM-{}", &Uuid::new_v4().to_string()[..8]),
            // One component per FG unit at rate 500 — the whole order consumes qty × 500
            // (explosion = per-unit quantity × order quantity / BoM output quantity).
            quantity: dec("1"),
            uom: None,
            items: vec![NewBomItem { item_id: comp, quantity: dec("1"), rate: dec("500"), is_phantom: false }],
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
            wip_account_id: Some(acc.wip),
            fg_account_id: Some(acc.fg),
            raw_material_account_id: Some(acc.raw),
            conversion_cost_account_id: Some(acc.conversion),
        })
        .await
        .unwrap();
    svc.confirm_work_order(wo, &sink).await.unwrap();
    (svc, pool, company, wo, comp, acc)
}

/// Consume the order's materials. `gl` receives the consume post — tests that assert
/// REAL-ledger WIP netting pass the `GlAdapter`; value-only probes pass a `CountingGl`.
async fn consumed(svc: &ManufacturingWriteService, wo: Uuid, comp: Uuid, gl: &dyn GlPostSink) -> FakeInventory {
    let inv = FakeInventory::new();
    inv.stock(comp, "1000", "500");
    let sink = LoggingSink;
    svc.consume_materials(wo, Uuid::new_v4(), &inv, gl, &sink).await.unwrap();
    inv
}

/// BP-1 — the subcontract DoD: consume 6,000, extra cost 2,500 → FG 8,500, WIP cleared,
/// interim credited 2,500. Real ledger.
#[tokio::test]
async fn bp1_subcontract_dod_numbers() {
    let (svc, pool, company, wo, comp, acc) = wo_base("12").await; // 12 × 500 = 6,000
    let interim = account(&pool, company, "2200-INTERIM", "liability", "current_liability", "credit").await;
    let gl = GlAdapter::new(pool.clone());
    let inv = consumed(&svc, wo, comp, &gl).await;
    let sink = LoggingSink;

    // The WO carries overrides for wip/fg/raw but NOT interim — the interim default resolves
    // through the category chain, so point the order at a seeded category.
    sqlx::query("UPDATE manufacturing.work_orders SET product_category_id=$2 WHERE id=$1")
        .bind(wo)
        .bind(seed_interim_category(&pool, company, Some(interim)).await)
        .execute(&pool)
        .await
        .unwrap();

    let out = svc
        .receive_finished(
            wo,
            ReceiveFinishedOrder {
                produced_qty: dec("12"),
                byproducts: vec![],
                extra_cost: Some(dec("2500")),
                cost_posture: CostPosture::Average,
                standard_unit_price: None,
            },
            &inv,
            &gl,
            &sink,
        )
        .await
        .unwrap();
    assert_eq!(out.finished_value, dec("8500.00"), "FG = components 6,000 + PO 2,500");
    assert_eq!(out.extra_cost, dec("2500.00"));
    assert!(out.completed);

    // GL: Dr FG 8,500 · Cr WIP 6,000 · Cr Interim 2,500; WIP nets to zero.
    assert_eq!(balance(&pool, acc.fg).await, dec("8500.00"));
    assert_eq!(balance(&pool, acc.wip).await, dec("0.00"));
    let interim_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM accounting.accounts WHERE company_id=$1 AND account_code='2200-INTERIM'",
    )
    .bind(company)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(balance(&pool, interim_id).await, dec("-2500.00"), "interim credited the PO value");
}

/// Seed a category whose defaults row carries only the interim account; returns the category id.
async fn seed_interim_category(pool: &sqlx::PgPool, company: Uuid, interim: Option<Uuid>) -> Uuid {
    let category = Uuid::new_v4();
    seed_costing_defaults(
        pool,
        company,
        category,
        None,
        None,
        None,
        None,
        interim,
        None,
        None,
        None,
    )
    .await;
    category
}

/// Attach a shared (company-NULL) byproduct leg to a BoM — master data, seeded directly.
async fn seed_byproduct(pool: &sqlx::PgPool, bom_id: Uuid, item: Uuid, qty: &str, share: &str) {
    let repo = BomByproductRepository::new(pool.clone());
    let mut conn = pool.acquire().await.unwrap();
    repo.insert_byproduct(
        &mut conn,
        &NewBomByproductRow {
            id: Uuid::new_v4(),
            company_id: None,
            bom_id,
            item_id: item,
            product_category_id: None,
            quantity: dec(qty),
            cost_share: dec(share),
        },
    )
    .await
    .unwrap();
}

/// BP-2 — byproduct legs slice T by cost_share; the FG line keeps the remainder exactly.
#[tokio::test]
async fn bp2_cost_share_split() {
    let (svc, pool, _company, wo, comp, _acc) = wo_base("10").await; // T = 10 × 500 = 5,000
    let byproduct_item = Uuid::new_v4();
    let bom_id: Uuid = sqlx::query_scalar("SELECT bom_id FROM manufacturing.work_orders WHERE id=$1")
        .bind(wo)
        .fetch_one(&pool)
        .await
        .unwrap();
    seed_byproduct(&pool, bom_id, byproduct_item, "2", "30").await;

    let gl = CountingGl::new();
    let inv = consumed(&svc, wo, comp, &gl).await;
    let sink = LoggingSink;
    let out = svc
        .receive_finished(
            wo,
            ReceiveFinishedOrder {
                produced_qty: dec("10"),
                byproducts: vec![ReceiveByproductLine { item_id: byproduct_item, quantity: dec("2") }],
                extra_cost: None,
                cost_posture: CostPosture::Average,
                standard_unit_price: None,
            },
            &inv,
            &gl,
            &sink,
        )
        .await
        .unwrap();
    // T = 5,000 → byproduct 30% = 1,500; FG keeps the remainder 3,500.
    assert_eq!(out.byproduct_value, dec("1500.00"));
    assert_eq!(out.finished_value, dec("3500.00"), "FG keeps the remainder");
    assert_eq!(inv.finished_qty(byproduct_item), dec("2"));
    assert_eq!(inv.finished_value(byproduct_item), dec("1500.00"));
}

/// BP-3 — a byproduct family carrying more than the whole batch is refused LOUDLY.
#[tokio::test]
async fn bp3_cost_share_overflow_loud() {
    let (svc, pool, _company, wo, comp, _acc) = wo_base("4").await;
    let bp_a = Uuid::new_v4();
    let bp_b = Uuid::new_v4();
    let bom_id: Uuid = sqlx::query_scalar("SELECT bom_id FROM manufacturing.work_orders WHERE id=$1")
        .bind(wo)
        .fetch_one(&pool)
        .await
        .unwrap();
    seed_byproduct(&pool, bom_id, bp_a, "1", "60").await;
    seed_byproduct(&pool, bom_id, bp_b, "1", "60").await;

    let gl = CountingGl::new();
    let inv = consumed(&svc, wo, comp, &gl).await;
    let sink = LoggingSink;
    let err = svc
        .receive_finished(
            wo,
            ReceiveFinishedOrder {
                produced_qty: dec("4"),
                byproducts: vec![
                    ReceiveByproductLine { item_id: bp_a, quantity: dec("1") },
                    ReceiveByproductLine { item_id: bp_b, quantity: dec("1") },
                ],
                extra_cost: None,
                cost_posture: CostPosture::Average,
                standard_unit_price: None,
            },
            &inv,
            &gl,
            &sink,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, ManufacturingError::CostShareOverflow { sum } if sum == dec("120")),
        "Σ shares > 100 is LOUD, got {err:?}"
    );
}

/// BP-4 — a byproduct line with NO BoM row (no cost share) is refused LOUDLY.
#[tokio::test]
async fn bp4_unknown_byproduct_refused() {
    let (svc, _pool, _company, wo, comp, _acc) = wo_base("4").await;
    let gl = CountingGl::new();
    let inv = consumed(&svc, wo, comp, &gl).await;
    let sink = LoggingSink;
    let err = svc
        .receive_finished(
            wo,
            ReceiveFinishedOrder {
                produced_qty: dec("4"),
                byproducts: vec![ReceiveByproductLine { item_id: Uuid::new_v4(), quantity: dec("1") }],
                extra_cost: None,
                cost_posture: CostPosture::Average,
                standard_unit_price: None,
            },
            &inv,
            &gl,
            &sink,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ManufacturingError::Invalid(_)), "no BoM share → LOUD");
}

/// BP-5 — standard posture: FG at qty × standard price (never recomputed), the gap posts to the
/// cost-variance account as a plug; a nonzero plug with no variance account is LOUD.
#[tokio::test]
async fn bp5_standard_posture_variance_plug() {
    let (svc, pool, company, wo, comp, acc) = wo_base("10").await; // actual 5,000
    let variance = account(&pool, company, "9300-VAR", "expense", "operating_expense", "debit").await;
    let category = Uuid::new_v4();
    seed_costing_defaults(
        &pool,
        company,
        category,
        None,
        None,
        None,
        None,
        None,
        Some(variance),
        None,
        None,
    )
    .await;
    sqlx::query("UPDATE manufacturing.work_orders SET product_category_id=$2 WHERE id=$1")
        .bind(wo)
        .bind(category)
        .execute(&pool)
        .await
        .unwrap();

    let gl = GlAdapter::new(pool.clone());
    let inv = consumed(&svc, wo, comp, &gl).await;
    let sink = LoggingSink;
    // Standard 600/unit × 10 = 6,000 vs actual 5,000 → favourable plug 1,000 (credit variance).
    let out = svc
        .receive_finished(
            wo,
            ReceiveFinishedOrder {
                produced_qty: dec("10"),
                byproducts: vec![],
                extra_cost: None,
                cost_posture: CostPosture::Standard,
                standard_unit_price: Some(dec("600")),
            },
            &inv,
            &gl,
            &sink,
        )
        .await
        .unwrap();
    assert_eq!(out.finished_value, dec("6000.00"), "FG at the pinned standard price");
    assert_eq!(balance(&pool, acc.fg).await, dec("6000.00"));
    assert_eq!(balance(&pool, acc.wip).await, dec("0.00"));
    assert_eq!(
        balance(&pool, variance).await,
        dec("-1000.00"),
        "favourable gap credits variance; the standard price is never recomputed"
    );
}
