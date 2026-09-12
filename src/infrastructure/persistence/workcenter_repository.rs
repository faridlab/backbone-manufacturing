//! Repository for WorkstationLoss / WorkstationProductivity entities
//!
//! Hand-written (declared under `user_owned` in `metaphor.codegen.yaml`). Productivity rows are
//! the OEE ledger: each row books a stretch of time on a workstation against a loss bucket
//! (productive / availability / performance / quality). Duration is READ-SIDE ONLY — it is
//! computed as `date_end − date_start` at query time; no duration column exists to drift, and
//! NO cron ever aggregates it (the module runs zero scheduled jobs — the OEE report is computed
//! on demand over the window the caller asks for).

use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use backbone_orm::{company_scope, org_scope};

use crate::domain::entity::{WorkstationLoss, WorkstationProductivity};

/// Table name for WorkstationLoss entities
pub const LOSSES_TABLE: &str = "manufacturing.workstation_losses";
/// Table name for WorkstationProductivity entities
pub const PRODUCTIVITY_TABLE: &str = "manufacturing.workstation_productivity";

/// Repository for the workcenter family.
pub struct WorkcenterRepository {
    losses: backbone_orm::GenericCrudRepository<WorkstationLoss, backbone_orm::SoftDelete>,
    productivity: backbone_orm::GenericCrudRepository<WorkstationProductivity, backbone_orm::SoftDelete>,
}

impl WorkcenterRepository {
    /// Create a new repository instance.
    pub fn new(pool: PgPool) -> Self {
        Self {
            losses: backbone_orm::GenericCrudRepository::new(pool.clone(), LOSSES_TABLE),
            productivity: backbone_orm::GenericCrudRepository::new(pool, PRODUCTIVITY_TABLE),
        }
    }

    /// Access the loss-table CRUD repository.
    pub fn losses(&self) -> &backbone_orm::GenericCrudRepository<WorkstationLoss, backbone_orm::SoftDelete> {
        &self.losses
    }

    /// Access the productivity-table CRUD repository.
    pub fn productivity(&self) -> &backbone_orm::GenericCrudRepository<WorkstationProductivity, backbone_orm::SoftDelete> {
        &self.productivity
    }
}

/// The exact row a loss insert writes.
pub struct NewWorkstationLossRow<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub loss_type: crate::domain::entity::LossType,
}

/// The exact row a productivity insert writes. `date_end` may be NULL — an open stretch counts
/// up to NOW() on the read side (a station still down contributes its downtime so far).
pub struct NewWorkstationProductivityRow {
    pub id: Uuid,
    pub workstation_id: Uuid,
    pub job_card_id: Option<Uuid>,
    pub loss_id: Uuid,
    pub date_start: DateTime<Utc>,
    pub date_end: Option<DateTime<Utc>>,
    pub description: Option<String>,
}

/// Seconds booked in one loss bucket over the OEE window.
pub struct OeeBucketRow {
    pub loss_type: String,
    pub seconds: f64,
}

impl WorkcenterRepository {
    /// Insert a loss reason — a pool write riding `org_scope::execute_scoped`, which binds the
    /// ambient org scope (the composing service sets it per request); undecorated (module tests)
    /// the insert runs plain (ADR-0029).
    pub async fn insert_loss(
        &self,
        pool: &PgPool,
        l: &NewWorkstationLossRow<'_>,
    ) -> Result<(), sqlx::Error> {
        org_scope::execute_scoped(
            pool,
            sqlx::query(
                r#"INSERT INTO manufacturing.workstation_losses (id, name, loss_type)
                   VALUES ($1,$2,$3)"#,
            )
            .bind(l.id).bind(l.name).bind(l.loss_type),
        )
        .await?;
        Ok(())
    }

    /// Find a loss reason by name — ID-only (ADR-0029): the module declares no tenant axis; under
    /// the composed decorator the org fence's scope union (subtree ∪ tenant root) decides which
    /// losses resolve, and the decorator's re-declared (org unit, name) NULLS NOT DISTINCT unique
    /// keeps the one-namespace-per-unit shape.
    pub async fn find_loss_by_name(
        &self,
        pool: &PgPool,
        name: &str,
    ) -> Result<Option<Uuid>, sqlx::Error> {
        company_scope::fetch_optional_scalar_scoped(
            pool,
            sqlx::query_scalar(
                r#"SELECT id FROM manufacturing.workstation_losses
                   WHERE name=$1
                     AND (metadata->>'deleted_at') IS NULL"#,
            )
            .bind(name),
        ).await
    }

    /// Insert a productivity row — a pool write riding `org_scope::execute_scoped`
    /// (ADR-0029; see `insert_loss`).
    pub async fn insert_productivity(
        &self,
        pool: &PgPool,
        p: &NewWorkstationProductivityRow,
    ) -> Result<(), sqlx::Error> {
        org_scope::execute_scoped(
            pool,
            sqlx::query(
                r#"INSERT INTO manufacturing.workstation_productivity
                     (id, workstation_id, job_card_id, loss_id,
                      date_start, date_end, description)
                   VALUES ($1,$2,$3,$4,$5,$6,$7)"#,
            )
            .bind(p.id).bind(p.workstation_id).bind(p.job_card_id)
            .bind(p.loss_id).bind(p.date_start).bind(p.date_end).bind(p.description.clone()),
        )
        .await?;
        Ok(())
    }

    /// Aggregate every loss bucket's seconds for one workstation over [from, to].
    ///
    /// A row overlaps the window when its [date_start, COALESCE(date_end, NOW())] interval
    /// intersects it; the window CLAMPS the counted seconds (a stretch that started before the
    /// window only contributes the part inside it). Computed at read time — no stored aggregate,
    /// no cron.
    pub async fn oee_buckets(
        &self,
        pool: &PgPool,
        workstation_id: Uuid,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<OeeBucketRow>, sqlx::Error> {
        let rows = company_scope::fetch_all_rows_scoped(
            pool,
            sqlx::query(
                r#"SELECT l.loss_type::text AS loss_type,
                          SUM(EXTRACT(EPOCH FROM (
                              LEAST(COALESCE(p.date_end, NOW()), $3::timestamptz)
                              - GREATEST(p.date_start, $2::timestamptz)
                          )))::float8 AS seconds
                   FROM manufacturing.workstation_productivity p
                   JOIN manufacturing.workstation_losses l ON l.id = p.loss_id
                   WHERE p.workstation_id=$1
                     AND COALESCE(p.date_end, NOW()) > $2::timestamptz
                     AND p.date_start < $3::timestamptz
                     AND (p.metadata->>'deleted_at') IS NULL
                   GROUP BY 1"#,
            )
            .bind(workstation_id)
            .bind(from)
            .bind(to),
        ).await?;
        Ok(rows
            .into_iter()
            .map(|r| OeeBucketRow {
                loss_type: r.get("loss_type"),
                seconds: r.get("seconds"),
            })
            .collect())
    }
}
