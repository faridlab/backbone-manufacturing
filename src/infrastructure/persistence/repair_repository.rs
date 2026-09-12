//! Repository for RepairOrder / RepairPart / RepairTag entities
//!
//! Hand-written (declared under `user_owned` in `metaphor.codegen.yaml`). Holds every
//! hand-written repair SQL statement: the draft insert, the part-line insert, and the four verb
//! gates (validate / start / end / cancel). `end` is the once-only guard on the part legs — all
//! legs move in ONE pass inside that transaction (4-layer rule: services orchestrate and own the
//! unit of work, repositories hold the SQL).
//!
//! The states are hand-set and verb-driven: draft → confirmed (validate, availability checked
//! through the inventory port) → under_repair (start) → done (end). Cancel is terminal; done is
//! uncancelable. No fees exist anywhere in the family, and no quarantine location: a `remove`
//! leg leaves for the inventory-LOSS ACCOUNT, never a Location row.

use anyhow::Result;
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use backbone_orm::{company_scope, org_scope};

use crate::domain::entity::{RepairOrder, RepairPart, RepairTag};

/// Table name for RepairOrder entities
pub const REPAIR_ORDERS_TABLE: &str = "manufacturing.repair_orders";
/// Table name for RepairPart entities
pub const REPAIR_PARTS_TABLE: &str = "manufacturing.repair_parts";
/// Table name for RepairTag entities
pub const REPAIR_TAGS_TABLE: &str = "manufacturing.repair_tags";

/// Repository for the repair family. One type owns all three tables: parts and tags only ever
/// travel with their order.
pub struct RepairRepository {
    orders: backbone_orm::GenericCrudRepository<RepairOrder, backbone_orm::SoftDelete>,
    parts: backbone_orm::GenericCrudRepository<RepairPart, backbone_orm::SoftDelete>,
    tags: backbone_orm::GenericCrudRepository<RepairTag, backbone_orm::SoftDelete>,
}

impl RepairRepository {
    /// Create a new repository instance.
    pub fn new(pool: PgPool) -> Self {
        Self {
            orders: backbone_orm::GenericCrudRepository::new(pool.clone(), REPAIR_ORDERS_TABLE),
            parts: backbone_orm::GenericCrudRepository::new(pool.clone(), REPAIR_PARTS_TABLE),
            tags: backbone_orm::GenericCrudRepository::new(pool, REPAIR_TAGS_TABLE),
        }
    }

    /// Access the order-table CRUD repository.
    pub fn orders(&self) -> &backbone_orm::GenericCrudRepository<RepairOrder, backbone_orm::SoftDelete> {
        &self.orders
    }

    /// Access the part-table CRUD repository.
    pub fn parts(&self) -> &backbone_orm::GenericCrudRepository<RepairPart, backbone_orm::SoftDelete> {
        &self.parts
    }

    /// Access the tag-table CRUD repository.
    pub fn tags(&self) -> &backbone_orm::GenericCrudRepository<RepairTag, backbone_orm::SoftDelete> {
        &self.tags
    }
}

/// The exact row a repair-order insert writes. `status` is not a parameter — the statement pins
/// `'draft'::repair_status`.
pub struct NewRepairOrderRow<'a> {
    pub id: Uuid,
    pub repair_number: &'a str,
    pub item_id: Uuid,
    pub product_category_id: Option<Uuid>,
    pub quantity: Decimal,
}

/// The exact row a repair-part insert writes. `line_type` is passed — add/remove/recycle is an
/// authoring decision, not a derived one.
pub struct NewRepairPartRow {
    pub id: Uuid,
    pub repair_order_id: Uuid,
    pub item_id: Uuid,
    pub warehouse_id: Option<Uuid>,
    pub line_type: crate::domain::entity::RepairLineType,
    pub quantity: Decimal,
    pub rate: Decimal,
}

/// A repair-order header the verbs read.
pub struct RepairOrderRow {
    pub item_id: Uuid,
    pub product_category_id: Option<Uuid>,
    pub quantity: Decimal,
    pub status: String,
}

/// One part leg as the end-of-repair pass consumes it.
pub struct RepairPartRow {
    pub item_id: Uuid,
    pub warehouse_id: Option<Uuid>,
    pub line_type: String,
    pub quantity: Decimal,
    pub rate: Decimal,
}

impl RepairRepository {
    /// Insert a draft repair order — a pool write riding `org_scope::execute_scoped`, which binds
    /// the ambient org scope (the composing service sets it per request); undecorated (module
    /// tests) the insert runs plain (ADR-0029).
    ///
    /// Returns the raw `sqlx::Error` deliberately: the caller inspects it for a unique violation
    /// to turn a duplicate repair number into a domain error.
    pub async fn insert_repair_order(
        &self,
        pool: &PgPool,
        o: &NewRepairOrderRow<'_>,
    ) -> Result<(), sqlx::Error> {
        org_scope::execute_scoped(
            pool,
            sqlx::query(
                r#"INSERT INTO manufacturing.repair_orders
                     (id, repair_number, item_id, product_category_id, quantity, status)
                   VALUES ($1,$2,$3,$4,$5,'draft'::repair_status)"#,
            )
            .bind(o.id).bind(o.repair_number)
            .bind(o.item_id).bind(o.product_category_id).bind(o.quantity),
        )
        .await?;
        Ok(())
    }

    /// Insert a repair part line — a pool write riding `org_scope::execute_scoped`
    /// (ADR-0029; see `insert_repair_order`).
    pub async fn insert_repair_part(
        &self,
        pool: &PgPool,
        p: &NewRepairPartRow,
    ) -> Result<(), sqlx::Error> {
        org_scope::execute_scoped(
            pool,
            sqlx::query(
                r#"INSERT INTO manufacturing.repair_parts
                     (id, repair_order_id, item_id, warehouse_id, line_type,
                      quantity, rate)
                   VALUES ($1,$2,$3,$4,$5,$6,$7)"#,
            )
            .bind(p.id).bind(p.repair_order_id).bind(p.item_id)
            .bind(p.warehouse_id).bind(p.line_type).bind(p.quantity).bind(p.rate),
        )
        .await?;
        Ok(())
    }

    /// Read a repair-order header. `status` is read as `::text` so an unknown state fails a
    /// check rather than panicking in a decode.
    pub async fn find_order(
        &self,
        pool: &PgPool,
        repair_id: Uuid,
    ) -> Result<Option<RepairOrderRow>, sqlx::Error> {
        let row = company_scope::fetch_optional_row_scoped(
            pool,
            sqlx::query(
                r#"SELECT item_id, product_category_id, quantity, status::text AS status
                   FROM manufacturing.repair_orders
                   WHERE id=$1 AND (metadata->>'deleted_at') IS NULL"#,
            )
            .bind(repair_id),
        ).await?;
        Ok(row.map(|r| RepairOrderRow {
            item_id: r.get("item_id"),
            product_category_id: r.get("product_category_id"),
            quantity: r.get("quantity"),
            status: r.get("status"),
        }))
    }

    /// Read every part leg of a repair order — the end-of-repair pass executes them ALL in one
    /// transaction; there is no per-leg partial state.
    pub async fn find_parts(
        &self,
        pool: &PgPool,
        repair_id: Uuid,
    ) -> Result<Vec<RepairPartRow>, sqlx::Error> {
        let rows = company_scope::fetch_all_rows_scoped(
            pool,
            sqlx::query(
                r#"SELECT item_id, warehouse_id, line_type::text AS line_type, quantity, rate
                   FROM manufacturing.repair_parts
                   WHERE repair_order_id=$1 AND (metadata->>'deleted_at') IS NULL
                   ORDER BY (metadata->>'created_at') ASC, id ASC"#,
            )
            .bind(repair_id),
        ).await?;
        Ok(rows
            .into_iter()
            .map(|r| RepairPartRow {
                item_id: r.get("item_id"),
                warehouse_id: r.get("warehouse_id"),
                line_type: r.get("line_type"),
                quantity: r.get("quantity"),
                rate: r.get("rate"),
            })
            .collect())
    }

    /// THE VALIDATE GATE: draft → confirmed. Returns rows affected (1 = this caller won).
    pub async fn gate_validate(
        &self,
        conn: &mut sqlx::PgConnection,
        repair_id: Uuid,
    ) -> Result<u64, sqlx::Error> {
        let done = sqlx::query(
            r#"UPDATE manufacturing.repair_orders SET status='confirmed'::repair_status
               WHERE id=$1 AND status='draft'::repair_status"#,
        )
        .bind(repair_id)
        .execute(conn)
        .await?;
        Ok(done.rows_affected())
    }

    /// THE START GATE: confirmed → under_repair. `start_repair` auto-confirms a draft first, so
    /// the arrival state here is confirmed either way. Returns rows affected.
    pub async fn gate_start(
        &self,
        conn: &mut sqlx::PgConnection,
        repair_id: Uuid,
    ) -> Result<u64, sqlx::Error> {
        let done = sqlx::query(
            r#"UPDATE manufacturing.repair_orders SET status='under_repair'::repair_status
               WHERE id=$1 AND status='confirmed'::repair_status"#,
        )
        .bind(repair_id)
        .execute(conn)
        .await?;
        Ok(done.rows_affected())
    }

    /// Auto-confirm a draft (the start verb's rider: a repair may be started straight from
    /// draft — availability was implicitly accepted by the operator).
    pub async fn gate_auto_confirm(
        &self,
        conn: &mut sqlx::PgConnection,
        repair_id: Uuid,
    ) -> Result<u64, sqlx::Error> {
        let done = sqlx::query(
            r#"UPDATE manufacturing.repair_orders SET status='confirmed'::repair_status
               WHERE id=$1 AND status='draft'::repair_status"#,
        )
        .bind(repair_id)
        .execute(conn)
        .await?;
        Ok(done.rows_affected())
    }

    /// THE END GATE: under_repair → done. Returns rows affected (1 = this caller won; 0 = the
    /// repair was already ended and the part legs deduped).
    ///
    /// The `status='under_repair'` predicate is the once-only guard on the part legs: every leg
    /// (add / remove / recycle) executes in the SAME transaction as this gate, or none does.
    /// `done` is uncancelable — the legs have moved real stock.
    pub async fn gate_end(
        &self,
        conn: &mut sqlx::PgConnection,
        repair_id: Uuid,
    ) -> Result<u64, sqlx::Error> {
        let done = sqlx::query(
            r#"UPDATE manufacturing.repair_orders SET status='done'::repair_status
               WHERE id=$1 AND status='under_repair'::repair_status"#,
        )
        .bind(repair_id)
        .execute(conn)
        .await?;
        Ok(done.rows_affected())
    }

    /// THE CANCEL GATE: draft|confirmed|under_repair → cancel. Returns rows affected (1 = this
    /// caller won; 0 = the repair was done — uncancelable, the service turns the 0 into a LOUD
    /// InvalidState).
    ///
    /// A cancel before the end gate needs NO move cancellation: the legs only ever move at `end`.
    pub async fn gate_cancel(
        &self,
        conn: &mut sqlx::PgConnection,
        repair_id: Uuid,
    ) -> Result<u64, sqlx::Error> {
        let done = sqlx::query(
            r#"UPDATE manufacturing.repair_orders SET status='cancel'::repair_status
               WHERE id=$1 AND status IN ('draft'::repair_status, 'confirmed'::repair_status,
                                          'under_repair'::repair_status)"#,
        )
        .bind(repair_id)
        .execute(conn)
        .await?;
        Ok(done.rows_affected())
    }

    /// Find a tag by name — ID-only (ADR-0029): the module declares no tenant axis; under the
    /// composed decorator the org fence scopes the lookup to the caller's unit and the decorator's
    /// re-declared (org unit, name) unique keeps one namespace per unit.
    pub async fn find_tag_by_name(
        &self,
        pool: &PgPool,
        name: &str,
    ) -> Result<Option<Uuid>, sqlx::Error> {
        company_scope::fetch_optional_scalar_scoped(
            pool,
            sqlx::query_scalar(
                r#"SELECT id FROM manufacturing.repair_tags
                   WHERE name=$1 AND (metadata->>'deleted_at') IS NULL"#,
            )
            .bind(name),
        ).await
    }

    /// Insert a tag — a pool write riding `org_scope::execute_scoped` (ADR-0029). Returns the raw
    /// `sqlx::Error` so the caller can turn the org-scoped name unique violation into a domain
    /// error.
    pub async fn insert_tag(
        &self,
        pool: &PgPool,
        id: Uuid,
        name: &str,
    ) -> Result<(), sqlx::Error> {
        org_scope::execute_scoped(
            pool,
            sqlx::query(
                r#"INSERT INTO manufacturing.repair_tags (id, name)
                   VALUES ($1,$2)"#,
            )
            .bind(id).bind(name),
        )
        .await?;
        Ok(())
    }
}
