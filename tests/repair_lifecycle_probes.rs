//! Repair lifecycle probes: draft → validate (part availability probed through the port, a
//! shortfall LOUD) → start (auto-confirms a draft) → end (every leg moves in one pass + the
//! grouped legs post) → done is uncancelable; cancel before end moves nothing.

mod common;

use backbone_manufacturing::application::service::manufacturing_events::LoggingSink;
use backbone_manufacturing::application::service::manufacturing_write_service::{
    ManufacturingError, ManufacturingWriteService, NewRepairOrder, NewRepairPart,
};
use backbone_manufacturing::domain::entity::RepairLineType;
use common::*;
use uuid::Uuid;

/// Seed the repair accounts through the category chain (a repair carries NO per-order overrides)
/// and return (svc, pool, category, repair_expense, inventory_loss, raw accounts).
async fn repair_base() -> (ManufacturingWriteService, sqlx::PgPool, Uuid, Uuid, Uuid, Uuid) {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let category = Uuid::new_v4();
    let expense = account(&pool, "6100-REPX", "expense", "operating_expense", "debit").await;
    let loss = account(&pool, "6200-INVLOSS", "expense", "operating_expense", "debit").await;
    let raw = account(&pool, "1400-RAWX", "asset", "inventory", "debit").await;
    seed_costing_defaults(&pool, category, None, None, Some(raw), None, None, None, Some(loss), Some(expense)).await;
    (svc, pool, category, expense, loss, raw)
}

fn new_repair(category: Uuid, broken: Uuid, parts: Vec<NewRepairPart>) -> NewRepairOrder {
    NewRepairOrder {
        repair_number: format!("RP-{}", &Uuid::new_v4().to_string()[..8]),
        item_id: broken,
        product_category_id: Some(category),
        quantity: dec("1"),
        parts,
    }
}

async fn status(pool: &sqlx::PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT status::text FROM manufacturing.repair_orders WHERE id=$1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// RP-1 — validate probes every ADD leg's availability through the port: a shortfall is LOUD and
/// the draft stays draft; a stocked draft validates to confirmed.
#[tokio::test]
async fn rp1_validate_availability() {
    let (svc, pool, category, _e, _l, _r) = repair_base().await;
    let broken = Uuid::new_v4();
    let part = Uuid::new_v4();
    let inv = FakeInventory::new();
    let id = svc
        .create_repair_order(new_repair(
            category,
            broken,
            vec![NewRepairPart {
                item_id: part,
                warehouse_id: None,
                line_type: RepairLineType::Add,
                quantity: dec("3"),
                rate: dec("100"),
            }],
        ))
        .await
        .unwrap();

    // No stock at all → shortfall LOUD, still draft.
    inv.stock(part, "2", "100");
    let err = svc.validate_repair(id, &inv).await.unwrap_err();
    assert!(
        matches!(err, ManufacturingError::Inventory(ref m) if m.contains("insufficient_stock")),
        "a shortfall is LOUD through the port, got {err:?}"
    );
    assert_eq!(status(&pool, id).await, "draft");

    // Stocked → validated to confirmed.
    inv.stock(part, "10", "100");
    svc.validate_repair(id, &inv).await.unwrap();
    assert_eq!(status(&pool, id).await, "confirmed");

    // Re-validating a confirmed order is LOUD (draft only).
    let err = svc.validate_repair(id, &inv).await.unwrap_err();
    assert!(matches!(err, ManufacturingError::RepairInvalidState(_)));
}

/// RP-2 — start auto-confirms a draft; end executes every leg and posts the grouped post:
///   add 2×100 + remove 1×50 + recycle 1×80
///   Dr Repair-Expense 200 (add) · Dr Inventory-Loss 50 (remove) · Dr Raw 80 (recycle)
///   Cr Raw 250 (add+remove) · Cr Repair-Expense 80 (recycle)
#[tokio::test]
async fn rp2_end_moves_legs_and_posts() {
    let (svc, pool, category, expense, loss, raw) = repair_base().await;
    let broken = Uuid::new_v4();
    let part_a = Uuid::new_v4();
    let part_b = Uuid::new_v4();
    let part_c = Uuid::new_v4();
    let inv = FakeInventory::new();
    inv.stock(part_a, "10", "100"); // add: 2 × 100
    inv.stock(part_b, "10", "50"); // remove: 1 × 50
    inv.stock(part_c, "0", "80"); // recycle: 1 × 80 (returns to stock, needs nothing)
    let id = svc
        .create_repair_order(new_repair(
            category,
            broken,
            vec![
                NewRepairPart { item_id: part_a, warehouse_id: None, line_type: RepairLineType::Add, quantity: dec("2"), rate: dec("100") },
                NewRepairPart { item_id: part_b, warehouse_id: None, line_type: RepairLineType::Remove, quantity: dec("1"), rate: dec("50") },
                NewRepairPart { item_id: part_c, warehouse_id: None, line_type: RepairLineType::Recycle, quantity: dec("1"), rate: dec("80") },
            ],
        ))
        .await
        .unwrap();

    // START straight from draft — auto-confirm is implied by the operator starting work.
    svc.start_repair(id).await.unwrap();
    assert_eq!(status(&pool, id).await, "under_repair");

    let sink = LoggingSink;
    let gl = GlAdapter::new(pool.clone());
    let out = svc.end_repair(id, &inv, &gl, &sink).await.unwrap();
    assert_eq!(out.parts_moved, 3);
    assert_eq!(out.repair_expense, dec("280.00"), "Σ(add) + Σ(recycle)");
    assert_eq!(out.inventory_loss, dec("50.00"), "Σ(remove)");
    assert_eq!(out.recovered_value, dec("80.00"), "Σ(recycle)");

    // Stock: add and remove drew down, recycle returned.
    assert_eq!(inv.on_hand(part_a), dec("8"));
    assert_eq!(inv.on_hand(part_b), dec("9"));
    assert_eq!(inv.on_hand(part_c), dec("1"));
    // Grouped GL: Dr Expense 200 · Dr Loss 50 · Dr Raw 80 · Cr Raw 250 · Cr Expense 80.
    assert_eq!(balance(&pool, expense).await, dec("120.00"), "200 − 80");
    assert_eq!(balance(&pool, loss).await, dec("50.00"));
    assert_eq!(balance(&pool, raw).await, dec("-170.00"), "80 − 250");
    assert_eq!(status(&pool, id).await, "done");

    // Re-ending a done repair is an idempotent no-op (no second leg pass, no second post).
    let again = svc.end_repair(id, &inv, &gl, &sink).await.unwrap();
    assert_eq!(again.parts_moved, 0);
    assert_eq!(balance(&pool, raw).await, dec("-170.00"), "unchanged by the retry");
}

/// RP-3 — end moves EVERY leg in one pass or nothing is gated: a leg that cannot draw stock
/// leaves the order under_repair, LOUD, and the already-moved legs are idempotent on retry.
#[tokio::test]
async fn rp3_leg_failure_loud_not_gated() {
    let (svc, pool, category, _e, _l, _r) = repair_base().await;
    let broken = Uuid::new_v4();
    let stocked = Uuid::new_v4();
    let missing = Uuid::new_v4();
    let inv = FakeInventory::new();
    inv.stock(stocked, "10", "100");
    inv.stock(missing, "0", "100"); // the second leg cannot draw
    let id = svc
        .create_repair_order(new_repair(
            category,
            broken,
            vec![
                NewRepairPart { item_id: stocked, warehouse_id: None, line_type: RepairLineType::Add, quantity: dec("2"), rate: dec("100") },
                NewRepairPart { item_id: missing, warehouse_id: None, line_type: RepairLineType::Add, quantity: dec("5"), rate: dec("100") },
            ],
        ))
        .await
        .unwrap();
    svc.start_repair(id).await.unwrap();
    let sink = LoggingSink;
    let gl = CountingGl::new();
    let err = svc.end_repair(id, &inv, &gl, &sink).await.unwrap_err();
    assert!(
        matches!(err, ManufacturingError::Inventory(ref c) if c == "insufficient_stock"),
        "the failed leg is LOUD, got {err:?}"
    );
    assert_eq!(status(&pool, id).await, "under_repair", "not gated");
    assert_eq!(inv.on_hand(stocked), dec("8"), "the first leg DID move (idempotent on retry)");
    // Restock and retry: the first leg must not move twice.
    inv.stock(missing, "10", "100");
    svc.end_repair(id, &inv, &gl, &sink).await.unwrap();
    assert_eq!(inv.on_hand(stocked), dec("8"), "idempotent per-leg keys");
    assert_eq!(inv.on_hand(missing), dec("5"));
    assert_eq!(status(&pool, id).await, "done");
}

/// RP-4 — done is uncancelable (its legs moved real stock); cancel BEFORE end moves nothing and
/// needs no move cancellation.
#[tokio::test]
async fn rp4_cancel_rules() {
    let (svc, pool, category, _e, _l, _r) = repair_base().await;
    let broken = Uuid::new_v4();
    let part = Uuid::new_v4();
    let inv = FakeInventory::new();
    inv.stock(part, "10", "100");
    let id = svc
        .create_repair_order(new_repair(
            category,
            broken,
            vec![NewRepairPart { item_id: part, warehouse_id: None, line_type: RepairLineType::Add, quantity: dec("2"), rate: dec("100") }],
        ))
        .await
        .unwrap();
    svc.start_repair(id).await.unwrap();
    svc.cancel_repair(id).await.unwrap(); // under_repair → cancel, nothing had moved
    assert_eq!(status(&pool, id).await, "cancel");
    assert_eq!(inv.on_hand(part), dec("10"), "no leg ever moved");
    let err = svc.cancel_repair(id).await.unwrap_err();
    assert!(matches!(err, ManufacturingError::RepairInvalidState(_)), "already cancelled");

    // A DONE repair refuses cancel LOUDLY.
    let id2 = svc
        .create_repair_order(new_repair(
            category,
            broken,
            vec![NewRepairPart { item_id: part, warehouse_id: None, line_type: RepairLineType::Add, quantity: dec("1"), rate: dec("100") }],
        ))
        .await
        .unwrap();
    let sink = LoggingSink;
    let gl = CountingGl::new();
    svc.start_repair(id2).await.unwrap();
    svc.end_repair(id2, &inv, &gl, &sink).await.unwrap();
    let err = svc.cancel_repair(id2).await.unwrap_err();
    assert!(matches!(err, ManufacturingError::RepairInvalidState(_)), "done is uncancelable");
    assert_eq!(status(&pool, id2).await, "done");
}

/// RP-5 — repair tags are master data with a single module-level namespace: the service refuses a
/// duplicate name LOUDLY. The DB-level one-namespace-per-org-unit guard is the (org unit, name)
/// unique installed by the composing service's tenancy decorator (ADR-0029), so this leg asserts
/// only the module's own read-then-insert refusal — never an org-scoped collision.
#[tokio::test]
async fn rp5_tag_duplicate_loud() {
    let (svc, _pool, _c, _e, _l, _r) = repair_base().await;
    let name = format!("warranty-{}", &Uuid::new_v4().to_string()[..8]);
    svc.create_repair_tag(name.clone()).await.unwrap();
    let err = svc.create_repair_tag(name).await.unwrap_err();
    assert!(matches!(err, ManufacturingError::Invalid(_)), "duplicate tag is LOUD");
}
