//! Tenancy posture probes (ADR-0029), schema + session level. The module is tenant-agnostic by
//! design: org scoping is installed by the COMPOSING service's tenancy decorator, never declared
//! here. These probes pin that posture from the outside, so a regression (a smuggled company
//! column, a module-declared isolation policy) fails loudly.
//!
//! The main connection is the owner (superuser), so RLS visibility cannot be exercised there —
//! what CAN be probed is the schema shape plus the default-denial the fence produces for a
//! non-owner. The non-owner legs connect as the NOBYPASSRLS `mfg_app` role (see `app_dsn`;
//! provision it on the scratch DB with `CREATE ROLE mfg_app LOGIN PASSWORD 'mfg_app' NOBYPASSRLS`
//! plus GRANT USAGE on the schema and per-table privileges).
//!
//! Legs (numbered, one per probe):
//!  TP-1  every manufacturing table carries the RLS fence flags: ENABLEd + FORCEd;
//!  TP-2  the module ships ZERO tenancy policies (isolation belongs to the composing service's
//!        decorator — a module policy would fence undecorated module tests out of their own data);
//!  TP-3  no company_id column survives on any manufacturing table (the strip held);
//!  TP-4  no org_unit_id column exists either — the module SQL law: hand-written module SQL never
//!        references the decorator's column, so undecorated tests see plain tables;
//!  TP-5  undecorated writes still work as the owner (master data + transaction rows insert with
//!        no tenant argument at all);
//!  TP-6  a non-owner NOBYPASSRLS session is default-denied regardless of the legacy company
//!        variable, while the owner still sees the seeded rows (the denial is the fence, not an
//!        empty database), and its INSERT is refused by row-level security.

mod common;

use common::pool;
use sqlx::{Connection, PgConnection, Row};
use uuid::Uuid;

fn app_dsn() -> String {
    std::env::var("MFG_APP_DSN").unwrap_or_else(|_| {
        "postgres://mfg_app:mfg_app@127.0.0.1:5433/backbone_manufacturing".into()
    })
}

/// Every table the module owns (the full census — a new table must join this fence).
const FENCED_TABLES: &[&str] = &[
    "bom_byproducts",
    "bom_items",
    "bom_operations",
    "bom_subcontractors",
    "boms",
    "category_costing_defaults",
    "job_cards",
    "operations",
    "repair_orders",
    "repair_parts",
    "repair_tags",
    "subcontract_mo_links",
    "unbuild_orders",
    "work_order_items",
    "work_orders",
    "workstation_losses",
    "workstation_productivity",
    "workstations",
];

fn err_text(e: &sqlx::Error) -> String {
    match e {
        sqlx::Error::Database(d) => d.message().to_string(),
        other => other.to_string(),
    }
}

/// TP-1 — every manufacturing table carries the fence flags: RLS ENABLEd and FORCEd (the owner is
/// fenced too, so a decorator installed by the composing service governs every session).
#[tokio::test]
async fn tp1_rls_enabled_and_forced_on_every_table() {
    let pool = pool().await;
    for t in FENCED_TABLES {
        let q = format!(
            "SELECT relrowsecurity, relforcerowsecurity FROM pg_class WHERE oid = 'manufacturing.{t}'::regclass"
        );
        let row = sqlx::query(&q).fetch_one(&pool).await.expect("pg_class");
        assert_eq!(
            (row.get::<bool, _>(0), row.get::<bool, _>(1)),
            (true, true),
            "RLS must be enabled + forced on manufacturing.{t}"
        );
    }
}

/// TP-2 — the module declares NO tenancy policy. Under ADR-0029, isolation belongs to the
/// composing service's decorator; with the flags forced and zero policies, every NOBYPASSRLS
/// session is default-denied until that decorator admits it.
#[tokio::test]
async fn tp2_zero_module_tenancy_policies() {
    let pool = pool().await;
    let policies: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_policies WHERE schemaname = 'manufacturing'",
    )
    .fetch_one(&pool)
    .await
    .expect("pg_policies");
    assert_eq!(policies, 0, "the module declares no tenancy policy");
}

/// TP-3 — no company_id column survives the strip on any manufacturing table.
#[tokio::test]
async fn tp3_no_company_columns_anywhere() {
    let pool = pool().await;
    let company_cols: i64 = sqlx::query_scalar(
        r#"SELECT count(*) FROM information_schema.columns
            WHERE table_schema = 'manufacturing' AND column_name = 'company_id'"#,
    )
    .fetch_one(&pool)
    .await
    .expect("information_schema");
    assert_eq!(company_cols, 0, "no company_id column survives the strip");
}

/// TP-4 — no org_unit_id column exists either: the module SQL law. The composing service's
/// decorator adds it; hand-written module SQL never references it, so undecorated module tests
/// run against plain tables.
#[tokio::test]
async fn tp4_no_org_unit_columns_either() {
    let pool = pool().await;
    let org_cols: i64 = sqlx::query_scalar(
        r#"SELECT count(*) FROM information_schema.columns
            WHERE table_schema = 'manufacturing' AND column_name = 'org_unit_id'"#,
    )
    .fetch_one(&pool)
    .await
    .expect("information_schema");
    assert_eq!(org_cols, 0, "the decorator's org column must not appear in the module schema");
}

/// TP-5 — undecorated writes work as the owner: master data (a workstation loss, a BoM) and a
/// transaction row (a repair tag) insert with no tenant argument at all. This is the shape every
/// module test relies on.
#[tokio::test]
async fn tp5_undecorated_owner_writes_work() {
    let pool = pool().await;
    sqlx::query(
        r#"INSERT INTO manufacturing.workstation_losses (id, name, loss_type)
           VALUES ($1, $2, 'productive'::loss_type)"#,
    )
    .bind(Uuid::new_v4())
    .bind(format!("posture-{}", &Uuid::new_v4().to_string()[..8]))
    .execute(&pool)
    .await
    .expect("undecorated master-data insert");

    sqlx::query(
        r#"INSERT INTO manufacturing.boms (id, item_id, bom_code, quantity)
           VALUES ($1, $2, $3, 1)"#,
    )
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(format!("BOM-POSTURE-{}", &Uuid::new_v4().to_string()[..8]))
    .execute(&pool)
    .await
    .expect("undecorated bom insert");

    sqlx::query(
        r#"INSERT INTO manufacturing.repair_tags (id, name)
           VALUES ($1, $2)"#,
    )
    .bind(Uuid::new_v4())
    .bind(format!("tag-{}", &Uuid::new_v4().to_string()[..8]))
    .execute(&pool)
    .await
    .expect("undecorated transaction-row insert");
}

/// TP-6 — a non-owner NOBYPASSRLS session is default-denied regardless of the legacy company
/// variable, while the owner still sees the seeded rows (the denial is the fence, not an empty
/// database), and its INSERT is refused by row-level security.
#[tokio::test]
async fn tp6_app_role_default_denied_even_with_legacy_variable() {
    let pool = pool().await;

    // Seed one master-data row as the owner so the denial leg below is meaningful.
    sqlx::query(
        r#"INSERT INTO manufacturing.workstation_losses (id, name, loss_type)
           VALUES ($1, $2, 'productive'::loss_type)"#,
    )
    .bind(Uuid::new_v4())
    .bind(format!("posture-denial-{}", &Uuid::new_v4().to_string()[..8]))
    .execute(&pool)
    .await
    .expect("owner seed");

    let mut app = PgConnection::connect(&app_dsn()).await.expect("app-role connect");
    sqlx::query("SELECT set_config('app.company_id', $1, false)")
        .bind(Uuid::new_v4().to_string())
        .execute(&mut app)
        .await
        .expect("set legacy variable");

    let losses: i64 = sqlx::query_scalar("SELECT count(*) FROM manufacturing.workstation_losses")
        .fetch_one(&mut app)
        .await
        .expect("loss count as the app role");
    assert_eq!(losses, 0, "no policy admits the app role, even with the legacy variable set");

    let owner_losses: i64 = sqlx::query_scalar("SELECT count(*) FROM manufacturing.workstation_losses")
        .fetch_one(&pool)
        .await
        .expect("loss count as owner");
    assert!(owner_losses >= 1, "the owner sees the seeded rows (got {owner_losses})");

    let insert = sqlx::query(
        r#"INSERT INTO manufacturing.workstation_losses (id, name, loss_type)
           VALUES ($1, $2, 'productive'::loss_type)"#,
    )
    .bind(Uuid::new_v4())
    .bind("posture refusal")
    .execute(&mut app)
    .await;
    match insert {
        Err(e) => assert!(
            err_text(&e).contains("row-level security") || err_text(&e).contains("policy"),
            "wrong error: {}",
            err_text(&e)
        ),
        Ok(_) => panic!("a default-denied role must not insert"),
    }
}
