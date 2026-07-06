//! Golden cases — the numeric oracle for BOM cost roll-up + work-order explosion.
//! Money is IDR (2dp, half-away-from-zero). Mirrors docs/business-flows/golden-cases.md.

mod common;

use backbone_manufacturing::application::service::manufacturing_events::LoggingSink;
use backbone_manufacturing::application::service::manufacturing_write_service::{
    ManufacturingError, ManufacturingWriteService, NewBom, NewBomItem, NewBomOperation, NewWorkOrder,
};
use common::*;
use rust_decimal::Decimal;
use uuid::Uuid;

async fn a_bom(svc: &ManufacturingWriteService, company: Uuid, item: Uuid, code: &str) -> Uuid {
    svc.create_bom(NewBom {
        company_id: company,
        item_id: item,
        bom_code: code.into(),
        quantity: dec("1"),
        uom: Some("unit".into()),
        // 10 × 500 = 5,000 + 4 × 250 = 1,000 → raw 6,000
        items: vec![
            NewBomItem { item_id: Uuid::new_v4(), quantity: dec("10"), rate: dec("500"), is_phantom: false },
            NewBomItem { item_id: Uuid::new_v4(), quantity: dec("4"), rate: dec("250"), is_phantom: false },
        ],
        // 30 min @ 120/hr = 60 + 15 min @ 80/hr = 20 → operating 80
        operations: vec![
            NewBomOperation { operation_id: Uuid::new_v4(), workstation_id: Uuid::new_v4(), time_in_mins: dec("30"), hour_rate: dec("120") },
            NewBomOperation { operation_id: Uuid::new_v4(), workstation_id: Uuid::new_v4(), time_in_mins: dec("15"), hour_rate: dec("80") },
        ],
    })
    .await
    .unwrap()
}

/// MGC-1 — BOM cost rolls up raw (Σ components) + operating (Σ time/60 × rate) = total.
#[tokio::test]
async fn mgc1_bom_cost_rollup() {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let bom = a_bom(&svc, company, item, &format!("BOM-{}", &Uuid::new_v4().to_string()[..8])).await;

    let row = sqlx::query_as::<_, (Decimal, Decimal, Decimal)>(
        "SELECT raw_material_cost, operating_cost, total_cost FROM manufacturing.boms WHERE id=$1",
    )
    .bind(bom)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, dec("6000.00"), "raw = 5,000 + 1,000");
    assert_eq!(row.1, dec("80.00"), "operating = 60 + 20");
    assert_eq!(row.2, dec("6080.00"), "total = raw + operating");
}

/// MGC-2 — releasing a work order explodes the BOM into required materials, scaled by WO qty / BOM qty.
#[tokio::test]
async fn mgc2_work_order_explosion() {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let sink = LoggingSink;
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let comp_a = Uuid::new_v4();
    let bom = svc
        .create_bom(NewBom {
            company_id: company,
            item_id: item,
            bom_code: format!("BOM-{}", &Uuid::new_v4().to_string()[..8]),
            quantity: dec("1"),
            uom: None,
            items: vec![NewBomItem { item_id: comp_a, quantity: dec("2"), rate: dec("100"), is_phantom: false }],
            operations: vec![],
        })
        .await
        .unwrap();

    let wo = svc
        .create_work_order(NewWorkOrder {
            company_id: company,
            work_order_number: format!("WO-{}", &Uuid::new_v4().to_string()[..8]),
            item_id: item,
            bom_id: bom,
            quantity: dec("5"), // build 5 against a BOM that yields 1
            wip_warehouse_id: None,
            fg_warehouse_id: None,
            wip_account_id: None,
            fg_account_id: None,
            raw_material_account_id: None,
            conversion_cost_account_id: None,
        })
        .await
        .unwrap();
    svc.release_work_order(wo, &sink).await.unwrap();

    // required = component qty (2) × WO qty (5) / BOM qty (1) = 10
    let required: Decimal = sqlx::query_scalar(
        "SELECT required_qty FROM manufacturing.work_order_items WHERE work_order_id=$1 AND item_id=$2",
    )
    .bind(wo)
    .bind(comp_a)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(required, dec("10.0000"));
}

/// MGC-3 — validation: a BOM needs components; a work order needs a positive quantity.
#[tokio::test]
async fn mgc3_validation() {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());

    let empty = svc
        .create_bom(NewBom {
            company_id: company,
            item_id: item,
            bom_code: "BAD".into(),
            quantity: dec("1"),
            uom: None,
            items: vec![],
            operations: vec![],
        })
        .await;
    assert!(matches!(empty, Err(ManufacturingError::Invalid(_))));

    let bom = a_bom(&svc, company, item, &format!("BOM-{}", &Uuid::new_v4().to_string()[..8])).await;
    let bad_qty = svc
        .create_work_order(NewWorkOrder {
            company_id: company,
            work_order_number: "WO-BAD".into(),
            item_id: item,
            bom_id: bom,
            quantity: dec("0"),
            wip_warehouse_id: None,
            fg_warehouse_id: None,
            wip_account_id: None,
            fg_account_id: None,
            raw_material_account_id: None,
            conversion_cost_account_id: None,
        })
        .await;
    assert!(matches!(bad_qty, Err(ManufacturingError::Invalid(_))));
}

/// MGC-4 (completeness council 2026-07-06) — a PHANTOM sub-assembly is exploded THROUGH to its own
/// BOM's components at release, not issued as a material. A "Chair" whose BOM lists a phantom "Frame"
/// (= 4 legs + 8 dowels) → releasing a WO for 2 chairs requires legs/dowels (the phantom's children),
/// never the Frame item itself.
#[tokio::test]
async fn mgc4_phantom_bom_explodes_through() {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let sink = LoggingSink;
    let company = Uuid::new_v4();
    let (chair, frame, leg, dowel, cushion) =
        (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());

    // The phantom Frame's own BOM: 4 legs + 8 dowels (per 1 frame).
    svc.create_bom(NewBom {
        company_id: company, item_id: frame, bom_code: format!("BOM-F-{}", &Uuid::new_v4().to_string()[..8]),
        quantity: dec("1"), uom: None,
        items: vec![
            NewBomItem { item_id: leg, quantity: dec("4"), rate: dec("1000"), is_phantom: false },
            NewBomItem { item_id: dowel, quantity: dec("8"), rate: dec("50"), is_phantom: false },
        ],
        operations: vec![],
    }).await.unwrap();

    // The Chair BOM: 1 phantom Frame + 1 real Cushion (per 1 chair).
    let chair_bom = svc.create_bom(NewBom {
        company_id: company, item_id: chair, bom_code: format!("BOM-C-{}", &Uuid::new_v4().to_string()[..8]),
        quantity: dec("1"), uom: None,
        items: vec![
            NewBomItem { item_id: frame, quantity: dec("1"), rate: dec("0"), is_phantom: true },
            NewBomItem { item_id: cushion, quantity: dec("1"), rate: dec("30000"), is_phantom: false },
        ],
        operations: vec![],
    }).await.unwrap();

    let wo = svc.create_work_order(NewWorkOrder {
        company_id: company, work_order_number: format!("WO-{}", &Uuid::new_v4().to_string()[..8]),
        item_id: chair, bom_id: chair_bom, quantity: dec("2"),
        wip_warehouse_id: None, fg_warehouse_id: None, wip_account_id: None, fg_account_id: None,
        raw_material_account_id: None, conversion_cost_account_id: None,
    }).await.unwrap();
    svc.release_work_order(wo, &sink).await.unwrap();

    let req = |item: Uuid| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, Decimal>("SELECT COALESCE(SUM(required_qty),0) FROM manufacturing.work_order_items WHERE work_order_id=$1 AND item_id=$2")
                .bind(wo).bind(item).fetch_one(&pool).await.unwrap()
        }
    };
    // 2 chairs → 2 frames → legs 4×2=8, dowels 8×2=16; cushion 1×2=2. Frame itself: NONE.
    assert_eq!(req(leg).await, dec("8.0000"), "phantom exploded to legs");
    assert_eq!(req(dowel).await, dec("16.0000"), "phantom exploded to dowels");
    assert_eq!(req(cushion).await, dec("2.0000"), "real component required directly");
    assert_eq!(req(frame).await, dec("0"), "the phantom item itself is NEVER required (never stocked)");
}
