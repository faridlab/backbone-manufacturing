//! Repository for SubcontractMoLink entities
//!
//! Hand-written (declared under `user_owned` in `metaphor.codegen.yaml`). The link table is the
//! subcontract receipt path's IDEMPOTENCY BACKSTOP: one hidden work order per
//! (company, purchase order). A replayed receipt event finds the existing link and returns the
//! work order it minted — no second MO, no second cost leg.

use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use backbone_orm::company_scope;

use crate::domain::entity::SubcontractMoLink;

/// Table name for SubcontractMoLink entities
pub const TABLE_NAME: &str = "manufacturing.subcontract_mo_links";

/// Repository for SubcontractMoLink entities.
pub struct SubcontractLinkRepository(
    backbone_orm::GenericCrudRepository<SubcontractMoLink, backbone_orm::SoftDelete>,
);

impl std::ops::Deref for SubcontractLinkRepository {
    type Target = backbone_orm::GenericCrudRepository<SubcontractMoLink, backbone_orm::SoftDelete>;
    fn deref(&self) -> &Self::Target { &self.0 }
}

impl SubcontractLinkRepository {
    /// Create a new repository instance.
    pub fn new(pool: PgPool) -> Self {
        Self(backbone_orm::GenericCrudRepository::new(pool, TABLE_NAME))
    }
}

/// The exact row a link insert writes.
pub struct NewSubcontractLinkRow {
    pub id: Uuid,
    pub company_id: Uuid,
    pub purchase_order_id: Uuid,
    pub work_order_id: Uuid,
}

impl SubcontractLinkRepository {
    /// Insert a link. Takes the CALLER'S connection so the link, the minted work order and the
    /// link's INSERT commit as ONE unit — a link without its work order (or the reverse) would
    /// break the replay backstop. The caller binds the company on it (`bind_company_on`) —
    /// don't re-bind. Returns the raw `sqlx::Error` so a concurrent mint surfaces as a unique
    /// violation the caller can re-read.
    pub async fn insert_link(
        &self,
        conn: &mut sqlx::PgConnection,
        l: &NewSubcontractLinkRow,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"INSERT INTO manufacturing.subcontract_mo_links
                 (id, company_id, purchase_order_id, work_order_id)
               VALUES ($1,$2,$3,$4)"#,
        )
        .bind(l.id).bind(l.company_id).bind(l.purchase_order_id).bind(l.work_order_id)
        .execute(conn)
        .await?;
        Ok(())
    }

    /// Find the work order a purchase order already minted, if any — the replay path's first
    /// stop (`find_by_purchase_order`).
    pub async fn find_by_purchase_order(
        &self,
        pool: &PgPool,
        company_id: Uuid,
        purchase_order_id: Uuid,
    ) -> Result<Option<Uuid>, sqlx::Error> {
        company_scope::fetch_optional_scalar_scoped(
            pool,
            sqlx::query_scalar(
                r#"SELECT work_order_id FROM manufacturing.subcontract_mo_links
                   WHERE company_id=$1 AND purchase_order_id=$2
                     AND (metadata->>'deleted_at') IS NULL"#,
            )
            .bind(company_id)
            .bind(purchase_order_id),
        ).await
    }
}
