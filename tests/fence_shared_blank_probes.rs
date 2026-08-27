//! shared_blank fence probes (ADR-0014), schema level. The test connection is a superuser, so RLS
//! visibility cannot be exercised here — what CAN be probed is the fence's schema shape: shared
//! (company-NULL) master-data rows insert cleanly, the shared namespace is collision-free
//! (NULLS NOT DISTINCT uniques), company-owned and shared rows coexist, and the STRICT tables
//! still refuse a NULL company.

mod common;

use common::*;
use uuid::Uuid;

async fn insert_loss(pool: &sqlx::PgPool, company: Option<Uuid>, name: &str, loss_type: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO manufacturing.workstation_losses (id, company_id, name, loss_type)
           VALUES ($1, $2, $3, $4::loss_type)"#,
    )
    .bind(Uuid::new_v4())
    .bind(company)
    .bind(name)
    .bind(loss_type)
    .execute(pool)
    .await
    .map(|_| ())
}

async fn insert_bom(
    pool: &sqlx::PgPool,
    company: Option<Uuid>,
    item: Uuid,
    code: &str,
    version: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO manufacturing.boms (id, company_id, item_id, bom_code, version)
           VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(Uuid::new_v4())
    .bind(company)
    .bind(item)
    .bind(code)
    .bind(version)
    .execute(pool)
    .await
    .map(|_| ())
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    e.to_string().contains("duplicate key value violates unique constraint")
}

/// FB-1 — shared workstation losses (company NULL) insert cleanly; a duplicate shared name
/// collides (the NULLS NOT DISTINCT unique holds the shared namespace single).
#[tokio::test]
async fn fb1_shared_losses_single_namespace() {
    let pool = pool().await;
    let name = format!("setup-{}", &Uuid::new_v4().to_string()[..8]);
    insert_loss(&pool, None, &name, "productive").await.expect("shared insert ok");
    let err = insert_loss(&pool, None, &name, "productive")
        .await
        .expect_err("two shared losses with the same name must collide");
    assert!(is_unique_violation(&err), "got: {err}");
    // A DIFFERENT shared name is fine.
    insert_loss(&pool, None, &format!("{name}-b"), "availability").await.expect("distinct shared name ok");
}

/// FB-2 — same-company loss names collide as before; company-owned and shared names coexist.
#[tokio::test]
async fn fb2_company_and_shared_coexist() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let name = format!("shift-{}", &Uuid::new_v4().to_string()[..8]);
    insert_loss(&pool, Some(company), &name, "productive").await.expect("company-owned ok");
    insert_loss(&pool, None, &name, "productive").await.expect("shared with the same name coexists");
    // The company's own namespace stays single.
    let err = insert_loss(&pool, Some(company), &name, "quality")
        .await
        .expect_err("same-company duplicate collides");
    assert!(is_unique_violation(&err), "got: {err}");
}

/// FB-3 — shared BoMs: one shared row per (item, version); a second shared row at the same
/// version collides, the next version is a new slot.
#[tokio::test]
async fn fb3_shared_bom_version_slots() {
    let pool = pool().await;
    let item = Uuid::new_v4();
    insert_bom(&pool, None, item, "SHARED-A", 1).await.expect("shared bom v1 ok");
    let err = insert_bom(&pool, None, item, "SHARED-B", 1)
        .await
        .expect_err("two shared boms at the same (item, version) must collide");
    assert!(is_unique_violation(&err), "got: {err}");
    insert_bom(&pool, None, item, "SHARED-C", 2).await.expect("version 2 is a new shared slot");
}

/// FB-4 — a company's BoM and the shared BoM occupy DISTINCT slots at the same (item, version).
#[tokio::test]
async fn fb4_company_and_shared_bom_distinct_slots() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let item = Uuid::new_v4();
    insert_bom(&pool, Some(company), item, "OWN-V1", 1).await.expect("company slot");
    insert_bom(&pool, None, item, "SHARED-V1", 1).await.expect("shared slot at the same (item, version)");
    // Two companies each carry their own v1 too — three rows, one shared.
    let other = Uuid::new_v4();
    insert_bom(&pool, Some(other), item, "OTHER-V1", 1).await.expect("second company slot");
}

/// FB-5 — the STRICT tables never went nullable: work_orders.company_id is still NOT NULL.
#[tokio::test]
async fn fb5_strict_tables_stay_strict() {
    let pool = pool().await;
    let nullable: bool = sqlx::query_scalar(
        "SELECT is_nullable='YES' FROM information_schema.columns
          WHERE table_schema='manufacturing' AND table_name='work_orders' AND column_name='company_id'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!nullable, "work_orders.company_id must stay NOT NULL (strict posture)");
    // And the fence actually refuses: a NULL-company work order cannot exist.
    let err = sqlx::query(
        r#"INSERT INTO manufacturing.work_orders (id, company_id, work_order_number, item_id, bom_id, quantity)
           VALUES ($1, NULL, 'NEVER', $2, $3, 1)"#,
    )
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .execute(&pool)
    .await
    .expect_err("strict tables refuse a NULL company");
    assert!(err.to_string().contains("null value"), "got: {err}");
}
