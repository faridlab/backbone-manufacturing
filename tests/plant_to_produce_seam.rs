//! The plant-to-produce seam, end-to-end: **manufacturing → the REAL backbone-accounting ledger**
//! (with materials valued through the inventory port). A Work Order's value flows through WIP in three
//! balanced posts — consume (Dr WIP · Cr Raw), operate (Dr WIP · Cr Conversion), receive (Dr FG · Cr
//! WIP) — so once the WO is fully received **WIP nets to ZERO**, raw stock is credited, conversion is
//! applied, and finished goods carry raw + operating cost. Manufacturing owns no ledger — it emits the
//! posts through `GlPostSink` (mapped to accounting's `PostingService`) and drives inventory through
//! `InventoryPort`. The shipped library has ZERO normal Cargo edge to accounting (dev-dep only here).

mod common;

use backbone_manufacturing::application::service::manufacturing_events::LoggingSink;
use backbone_manufacturing::application::service::manufacturing_write_service::{
    ManufacturingWriteService, NewBom, NewBomItem, NewJobCard, NewWorkOrder,
};
use common::*;
use rust_decimal::Decimal;
use std::sync::Arc;
use uuid::Uuid;

/// PTPSEAM-1 — a full Work Order (consume → operate → receive) nets WIP to zero across the real ledger.
#[tokio::test]
async fn ptpseam1_wip_nets_to_zero() {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let gl = Arc::new(GlAdapter::new(pool.clone()));
    let sink = LoggingSink;
    let company = Uuid::new_v4();
    let fg_item = Uuid::new_v4();
    let (comp_x, comp_y) = (Uuid::new_v4(), Uuid::new_v4());
    let raw_wh = Uuid::new_v4();
    let acc = wo_accounts(&pool, company).await;

    // Real inventory valuation: X @ 500, Y @ 250, both well stocked.
    let inv = FakeInventory::new();
    inv.stock(comp_x, "100", "500");
    inv.stock(comp_y, "100", "250");

    // BOM for 1 FG: 2 × X + 4 × Y.
    let bom = svc
        .create_bom(NewBom {
            company_id: company,
            item_id: fg_item,
            bom_code: format!("BOM-{}", &Uuid::new_v4().to_string()[..8]),
            quantity: dec("1"),
            uom: None,
            items: vec![
                NewBomItem { item_id: comp_x, quantity: dec("2"), rate: dec("500"), is_phantom: false },
                NewBomItem { item_id: comp_y, quantity: dec("4"), rate: dec("250"), is_phantom: false },
            ],
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

    // 1) consume: value = 2×500 + 4×250 = 2000. Dr WIP · Cr Raw.
    let c = svc.consume_materials(wo, raw_wh, &*inv_arc(&inv), &*gl, &sink).await.unwrap();
    assert_eq!(c.raw_material_value, dec("2000.00"));

    // 2) operate: 30 min @ 120/hr = 60. Dr WIP · Cr Conversion.
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
    assert_eq!(svc.complete_job_card(jc, &*gl, &sink).await.unwrap(), dec("60.00"));

    // 3) receive: FG value = raw 2000 + operating 60 = 2060. Dr FG · Cr WIP.
    let r = svc.receive_finished(wo, dec("1"), &*inv_arc(&inv), &*gl, &sink).await.unwrap();
    assert_eq!(r.finished_value, dec("2060.00"));
    assert!(r.completed);

    // --- The ledger: WIP nets to ZERO; raw credited; conversion applied; FG carries the full cost. ---
    assert_eq!(balance(&pool, acc.wip).await, dec("0.00"), "WIP nets to zero on completion");
    assert_eq!(balance(&pool, acc.raw).await, dec("-2000.00"), "raw-material stock credited");
    assert_eq!(balance(&pool, acc.conversion).await, dec("-60.00"), "conversion applied");
    assert_eq!(balance(&pool, acc.fg).await, dec("2060.00"), "finished goods = raw + operating");

    // --- Inventory: components consumed, finished goods received at cost. ---
    assert_eq!(inv.on_hand(comp_x), dec("98"), "2 of X issued");
    assert_eq!(inv.on_hand(comp_y), dec("96"), "4 of Y issued");
    assert_eq!(inv.finished_qty(fg_item), dec("1"));
    assert_eq!(inv.finished_value(fg_item), dec("2060.00"));
}

/// Borrow a `FakeInventory` as `&dyn InventoryPort` (it holds Arc-shared interior state, so clones
/// observe the same stock).
fn inv_arc(
    inv: &FakeInventory,
) -> Box<dyn backbone_manufacturing::application::service::manufacturing_ports::InventoryPort> {
    Box::new(inv.clone())
}
