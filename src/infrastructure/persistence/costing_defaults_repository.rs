//! Repository for CategoryCostingDefaults entities
//!
//! Hand-written (declared under `user_owned` in `metaphor.codegen.yaml`). One row per
//! (org unit, product category) under the composed fence holding the WIP / FG / raw-material /
//! conversion / subcontract-interim / cost-variance / inventory-loss / repair-expense accounts
//! the costing paths fall back to when a work order's explicit overrides are unset. The
//! resolution order is ALWAYS: per-order override → category default → LOUD MissingAccount. A
//! hardcoded fallback account would silently post to someone else's ledger and is never used.

use anyhow::Result;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use backbone_orm::{company_scope, org_scope};

use crate::domain::entity::CategoryCostingDefaults;

/// Table name for CategoryCostingDefaults entities
pub const TABLE_NAME: &str = "manufacturing.category_costing_defaults";

/// Repository for CategoryCostingDefaults entities.
pub struct CostingDefaultsRepository(
    backbone_orm::GenericCrudRepository<CategoryCostingDefaults, backbone_orm::SoftDelete>,
);

impl std::ops::Deref for CostingDefaultsRepository {
    type Target = backbone_orm::GenericCrudRepository<CategoryCostingDefaults, backbone_orm::SoftDelete>;
    fn deref(&self) -> &Self::Target { &self.0 }
}

impl CostingDefaultsRepository {
    /// Create a new repository instance.
    pub fn new(pool: PgPool) -> Self {
        Self(backbone_orm::GenericCrudRepository::new(pool, TABLE_NAME))
    }
}

/// The exact row a costing-defaults insert writes. Every account is optional — an unset column
/// is a HOLE the resolution path reports loudly, not a zero to post against.
pub struct NewCostingDefaultsRow {
    pub id: Uuid,
    pub product_category_id: Uuid,
    pub wip_account_id: Option<Uuid>,
    pub fg_account_id: Option<Uuid>,
    pub raw_material_account_id: Option<Uuid>,
    pub conversion_cost_account_id: Option<Uuid>,
    pub subcontract_interim_account_id: Option<Uuid>,
    pub cost_variance_account_id: Option<Uuid>,
    pub inventory_loss_account_id: Option<Uuid>,
    pub repair_expense_account_id: Option<Uuid>,
}

/// A defaults row's accounts, exactly as resolved.
pub struct CostingDefaultsAccounts {
    pub wip_account_id: Option<Uuid>,
    pub fg_account_id: Option<Uuid>,
    pub raw_material_account_id: Option<Uuid>,
    pub conversion_cost_account_id: Option<Uuid>,
    pub subcontract_interim_account_id: Option<Uuid>,
    pub cost_variance_account_id: Option<Uuid>,
    pub inventory_loss_account_id: Option<Uuid>,
    pub repair_expense_account_id: Option<Uuid>,
}

impl CostingDefaultsRepository {
    /// Insert a defaults row — a pool write riding `org_scope::execute_scoped`, which binds the
    /// ambient org scope (the composing service sets it per request); undecorated (module tests)
    /// the insert runs plain (ADR-0029).
    ///
    /// Returns the raw `sqlx::Error` deliberately: the caller inspects it for a unique violation
    /// to turn a duplicate (org unit, category) row into a domain error.
    pub async fn insert_defaults(
        &self,
        pool: &PgPool,
        d: &NewCostingDefaultsRow,
    ) -> Result<(), sqlx::Error> {
        org_scope::execute_scoped(
            pool,
            sqlx::query(
                r#"INSERT INTO manufacturing.category_costing_defaults
                     (id, product_category_id,
                      wip_account_id, fg_account_id, raw_material_account_id,
                      conversion_cost_account_id, subcontract_interim_account_id,
                      cost_variance_account_id, inventory_loss_account_id, repair_expense_account_id)
                   VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)"#,
            )
            .bind(d.id).bind(d.product_category_id)
            .bind(d.wip_account_id).bind(d.fg_account_id).bind(d.raw_material_account_id)
            .bind(d.conversion_cost_account_id).bind(d.subcontract_interim_account_id)
            .bind(d.cost_variance_account_id).bind(d.inventory_loss_account_id)
            .bind(d.repair_expense_account_id),
        )
        .await?;
        Ok(())
    }

    /// Resolve the category's default accounts — ID-only (ADR-0029): the read rides the
    /// request-dedicated connection, so under the composed decorator the org fence scopes it to
    /// the caller's unit, and the decorator's re-declared (org unit, category) unique guarantees
    /// at most one live row per unit.
    pub async fn find(
        &self,
        pool: &PgPool,
        product_category_id: Uuid,
    ) -> Result<Option<CostingDefaultsAccounts>, sqlx::Error> {
        let row = company_scope::fetch_optional_row_scoped(
            pool,
            sqlx::query(
                r#"SELECT wip_account_id, fg_account_id, raw_material_account_id,
                          conversion_cost_account_id, subcontract_interim_account_id,
                          cost_variance_account_id, inventory_loss_account_id,
                          repair_expense_account_id
                   FROM manufacturing.category_costing_defaults
                   WHERE product_category_id=$1
                     AND (metadata->>'deleted_at') IS NULL"#,
            )
            .bind(product_category_id),
        ).await?;
        Ok(row.map(|r| CostingDefaultsAccounts {
            wip_account_id: r.get("wip_account_id"),
            fg_account_id: r.get("fg_account_id"),
            raw_material_account_id: r.get("raw_material_account_id"),
            conversion_cost_account_id: r.get("conversion_cost_account_id"),
            subcontract_interim_account_id: r.get("subcontract_interim_account_id"),
            cost_variance_account_id: r.get("cost_variance_account_id"),
            inventory_loss_account_id: r.get("inventory_loss_account_id"),
            repair_expense_account_id: r.get("repair_expense_account_id"),
        }))
    }
}
