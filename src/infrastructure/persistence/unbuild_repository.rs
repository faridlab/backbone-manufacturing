//! Repository for UnbuildOrder entities
//!
//! Hand-written (declared under `user_owned` in `metaphor.codegen.yaml`). Holds every
//! hand-written unbuild SQL statement: the draft insert and the execute gate that is the
//! once-only guard on the reverse stock move (4-layer rule: services orchestrate and own the
//! unit of work, repositories hold the SQL).
//!
//! An unbuild order is a plain 2-state record (draft → done) that reverses FINISHED goods of a
//! DONE work order back into components via the InventoryPort. It never creates a reverse work
//! order — there is nothing to schedule, only stock to move back.

use anyhow::Result;
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use backbone_orm::{company_scope, org_scope};

use crate::domain::entity::UnbuildOrder;

/// Table name for UnbuildOrder entities
pub const TABLE_NAME: &str = "manufacturing.unbuild_orders";

/// Repository for UnbuildOrder entities.
pub struct UnbuildRepository(
    backbone_orm::GenericCrudRepository<UnbuildOrder, backbone_orm::SoftDelete>,
);

impl std::ops::Deref for UnbuildRepository {
    type Target = backbone_orm::GenericCrudRepository<UnbuildOrder, backbone_orm::SoftDelete>;
    fn deref(&self) -> &Self::Target { &self.0 }
}

impl UnbuildRepository {
    /// Create a new repository instance.
    pub fn new(pool: PgPool) -> Self {
        Self(backbone_orm::GenericCrudRepository::new(pool, TABLE_NAME))
    }
}

/// The exact row an unbuild insert writes. `status` is not a parameter — the statement pins
/// `'draft'::unbuild_status` so a new unbuild can only ever start as a draft.
pub struct NewUnbuildRow<'a> {
    pub id: Uuid,
    pub unbuild_number: &'a str,
    pub work_order_id: Uuid,
    pub item_id: Uuid,
    pub quantity: Decimal,
}

/// Everything `execute_unbuild` needs to decide and to reverse, in ONE round trip: the unbuild's
/// own state, its source work order's state and produced quantity, and how much of that
/// production has ALREADY been unbuilt by other done unbuilds (this order excluded — a draft
/// re-executed after other unbuilds must count only against what remains).
pub struct UnbuildExecuteRow {
    pub work_order_id: Uuid,
    pub item_id: Uuid,
    pub quantity: Decimal,
    pub status: String,
    pub work_order_status: String,
    pub produced_qty: Decimal,
    pub already_unbuilt: Decimal,
}

impl UnbuildRepository {
    /// Insert a draft unbuild order — a pool write riding `org_scope::execute_scoped`, which
    /// binds the ambient org scope (the composing service sets it per request); undecorated
    /// (module tests) the insert runs plain (ADR-0029).
    ///
    /// Returns the raw `sqlx::Error` deliberately: the caller inspects it for a unique violation
    /// to turn a duplicate unbuild number into a domain error.
    pub async fn insert_draft(
        &self,
        pool: &PgPool,
        u: &NewUnbuildRow<'_>,
    ) -> Result<(), sqlx::Error> {
        org_scope::execute_scoped(
            pool,
            sqlx::query(
                r#"INSERT INTO manufacturing.unbuild_orders
                     (id, unbuild_number, work_order_id, item_id, quantity, status)
                   VALUES ($1,$2,$3,$4,$5,'draft'::unbuild_status)"#,
            )
            .bind(u.id).bind(u.unbuild_number)
            .bind(u.work_order_id).bind(u.item_id).bind(u.quantity),
        )
        .await?;
        Ok(())
    }

    /// Read everything `execute_unbuild` needs. The remaining producible quantity is
    /// `produced_qty − already_unbuilt`; the service refuses a quantity above it LOUDLY
    /// (UnbuildOverRemaining) rather than clipping silently.
    pub async fn find_execute_source(
        &self,
        pool: &PgPool,
        unbuild_id: Uuid,
    ) -> Result<Option<UnbuildExecuteRow>, sqlx::Error> {
        let row = company_scope::fetch_optional_row_scoped(
            pool,
            sqlx::query(
                r#"SELECT u.work_order_id, u.item_id, u.quantity,
                          u.status::text AS status,
                          w.status::text AS wo_status, w.produced_qty,
                          COALESCE((SELECT SUM(x.quantity) FROM manufacturing.unbuild_orders x
                                    WHERE x.work_order_id = u.work_order_id
                                      AND x.status = 'done' AND x.id <> u.id), 0) AS already_unbuilt
                   FROM manufacturing.unbuild_orders u
                   JOIN manufacturing.work_orders w ON w.id = u.work_order_id
                   WHERE u.id=$1 AND (u.metadata->>'deleted_at') IS NULL"#,
            )
            .bind(unbuild_id),
        ).await?;
        Ok(row.map(|r| UnbuildExecuteRow {
            work_order_id: r.get("work_order_id"),
            item_id: r.get("item_id"),
            quantity: r.get("quantity"),
            status: r.get("status"),
            work_order_status: r.get("wo_status"),
            produced_qty: r.get("produced_qty"),
            already_unbuilt: r.get("already_unbuilt"),
        }))
    }

    /// THE EXECUTE GATE: draft → done. Returns rows affected (1 = this caller won; 0 = it was
    /// executed concurrently and the reversal deduped).
    ///
    /// The `status='draft'` predicate is the once-only guard on the reverse stock move and its GL
    /// reversal. Takes the CALLER'S connection so the gate, the port reversal and the GL post
    /// commit as ONE unit; the caller relays the ambient org scope on it (`relay_ambient_scope`)
    /// — don't re-bind (ADR-0029).
    pub async fn gate_execute(
        &self,
        conn: &mut sqlx::PgConnection,
        unbuild_id: Uuid,
    ) -> Result<u64, sqlx::Error> {
        let done = sqlx::query(
            r#"UPDATE manufacturing.unbuild_orders SET status='done'::unbuild_status
               WHERE id=$1 AND status='draft'::unbuild_status"#,
        )
        .bind(unbuild_id)
        .execute(conn)
        .await?;
        Ok(done.rows_affected())
    }
}
