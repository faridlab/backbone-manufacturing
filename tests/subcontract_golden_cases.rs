//! Subcontract receipt golden cases: kind mismatch refused LOUDLY; the mint lands 'confirmed'
//! with exploded requirements numbered by the purchase reference; a replayed event returns the
//! SAME work order id (the link row is the backstop); a non-subcontract BoM is an authoring
//! defect, LOUD. Zero buying writes, zero SVL.

mod common;

use backbone_manufacturing::application::service::manufacturing_events::LoggingSink;
use backbone_manufacturing::application::service::manufacturing_subcontract::{
    SubcontractReceiptEvent, SubcontractReceiptLine,
};
use backbone_manufacturing::application::service::manufacturing_write_service::{
    ManufacturingError, ManufacturingWriteService, NewBom, NewBomItem,
};
use common::*;
use rust_decimal::Decimal;
use uuid::Uuid;

async fn svc_bom(company: Uuid, item: Uuid, comp: Uuid) -> (ManufacturingWriteService, sqlx::PgPool, Uuid) {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let bom = svc
        .create_bom(NewBom {
            company_id: company,
            item_id: item,
            bom_code: format!("BOM-{}", &Uuid::new_v4().to_string()[..8]),
            quantity: dec("1"),
            uom: None,
            items: vec![NewBomItem { item_id: comp, quantity: dec("2"), rate: dec("150"), is_phantom: false }],
            operations: vec![],
        })
        .await
        .unwrap();
    (svc, pool, bom)
}

fn event(company: Uuid, order: Uuid, item: Uuid, qty: &str, reference: Option<&str>) -> SubcontractReceiptEvent {
    SubcontractReceiptEvent {
        order_id: order,
        company_id: company,
        supplier_id: Uuid::new_v4(),
        order_kind: "subcontract".into(),
        currency: "IDR".into(),
        reference: reference.map(|r| r.into()),
        lines: vec![SubcontractReceiptLine { item_id: item, quantity: dec(qty), rate: dec("5000") }],
    }
}

/// SCB-1 — a goods receipt whose order_kind is not `subcontract` NEVER mints a work order.
#[tokio::test]
async fn scb1_kind_mismatch_loud() {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let company = Uuid::new_v4();
    let item = Uuid::new_v4();
    let mut e = event(company, Uuid::new_v4(), item, "5", None);
    e.order_kind = "goods".into();
    let sink = LoggingSink;
    let err = svc.handle_subcontract_receipt(&e, &sink).await.unwrap_err();
    assert!(
        matches!(err, ManufacturingError::SubcontractKindMismatch(ref k) if k == "goods"),
        "kind mismatch is LOUD, got {err:?}"
    );
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manufacturing.work_orders WHERE company_id=$1")
        .bind(company)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0, "no MO was minted");
}

/// SCB-2 — the mint: a confirmed MO numbered by the purchase reference, requirements exploded
/// from the subcontract BoM, warehouses left for the operator to set.
#[tokio::test]
async fn scb2_mint_confirmed_exploded() {
    let company = Uuid::new_v4();
    let item = Uuid::new_v4();
    let comp = Uuid::new_v4();
    let (svc, pool, bom) = svc_bom(company, item, comp).await;
    sqlx::query("UPDATE manufacturing.boms SET bom_type='subcontract'::bom_type WHERE id=$1")
        .bind(bom)
        .execute(&pool)
        .await
        .unwrap();
    let po = Uuid::new_v4();
    let sink = LoggingSink;
    let wo = svc
        .handle_subcontract_receipt(&event(company, po, item, "5", Some("PO/SC-1042")), &sink)
        .await
        .unwrap();

    let (status, number, qty): (String, String, Decimal) = sqlx::query_as(
        "SELECT status::text, work_order_number, quantity FROM manufacturing.work_orders WHERE id=$1",
    )
    .bind(wo)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "confirmed", "the hidden MO is minted directly confirmed");
    assert_eq!(number, "PO/SC-1042", "numbered by the purchase reference");
    assert_eq!(qty, dec("5"));
    // The BoM's 2-per exploded to the receipt quantity.
    let (req_item, req_qty): (Uuid, Decimal) = sqlx::query_as(
        "SELECT item_id, required_qty FROM manufacturing.work_order_items WHERE work_order_id=$1",
    )
    .bind(wo)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((req_item, req_qty), (comp, dec("10")));
    // The link row exists — one MO per purchase order.
    let linked: Uuid = sqlx::query_scalar(
        "SELECT work_order_id FROM manufacturing.subcontract_mo_links WHERE company_id=$1 AND purchase_order_id=$2",
    )
    .bind(company)
    .bind(po)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(linked, wo);
}

/// SCB-3 — replay: the same purchase order's event again returns the SAME work order id and
/// mints nothing new (a second, DIFFERENT order for the same item still mints its own).
#[tokio::test]
async fn scb3_replay_returns_same_id() {
    let company = Uuid::new_v4();
    let item = Uuid::new_v4();
    let comp = Uuid::new_v4();
    let (svc, pool, bom) = svc_bom(company, item, comp).await;
    sqlx::query("UPDATE manufacturing.boms SET bom_type='subcontract'::bom_type WHERE id=$1")
        .bind(bom)
        .execute(&pool)
        .await
        .unwrap();
    let po = Uuid::new_v4();
    let sink = LoggingSink;
    let first = svc.handle_subcontract_receipt(&event(company, po, item, "3", None), &sink).await.unwrap();
    let replay = svc.handle_subcontract_receipt(&event(company, po, item, "3", None), &sink).await.unwrap();
    assert_eq!(first, replay, "the link row is the replay backstop");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manufacturing.work_orders WHERE company_id=$1")
        .bind(company)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "no second MO");
    // A different purchase order for the same item mints its own MO.
    let other_po = svc.handle_subcontract_receipt(&event(company, Uuid::new_v4(), item, "2", None), &sink).await.unwrap();
    assert_ne!(other_po, first);
}

/// SCB-4 — the item's active BoM must be bom_type='subcontract'; a normal BoM on a subcontract
/// receipt is an authoring defect, LOUD.
#[tokio::test]
async fn scb4_normal_bom_refused() {
    let company = Uuid::new_v4();
    let item = Uuid::new_v4();
    let comp = Uuid::new_v4();
    let (svc, _pool, _bom) = svc_bom(company, item, comp).await; // left 'normal'
    let sink = LoggingSink;
    let err = svc
        .handle_subcontract_receipt(&event(company, Uuid::new_v4(), item, "5", None), &sink)
        .await
        .unwrap_err();
    match &err {
        ManufacturingError::Invalid(msg) => assert!(
            msg.contains("bom_type"),
            "the refusal names the authoring defect, got: {msg}"
        ),
        other => panic!("expected Invalid, got {other:?}"),
    }
    // And an event with NO lines never reaches the BoM at all.
    let mut e = event(company, Uuid::new_v4(), item, "5", None);
    e.lines = vec![];
    let err = svc.handle_subcontract_receipt(&e, &sink).await.unwrap_err();
    assert!(matches!(err, ManufacturingError::Invalid(_)), "empty event refused");
}
