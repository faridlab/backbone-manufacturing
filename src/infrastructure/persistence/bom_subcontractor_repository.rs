//! Repository for BomSubcontractor entities
//!
//! Hand-written (declared under `user_owned` in `metaphor.codegen.yaml`). The subcontractor list
//! is READ-ONLY master data on a BoM of type `subcontract` — it records WHO may produce the item
//! externally; the send-to-subcontractor flow itself is deferred (no component moves to the
//! partner, no receipt-driven closing).

use anyhow::Result;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use backbone_orm::company_scope;

use crate::domain::entity::BomSubcontractor;

/// Table name for BomSubcontractor entities
pub const TABLE_NAME: &str = "manufacturing.bom_subcontractors";

/// Repository for BomSubcontractor entities.
pub struct BomSubcontractorRepository(
    backbone_orm::GenericCrudRepository<BomSubcontractor, backbone_orm::SoftDelete>,
);

impl std::ops::Deref for BomSubcontractorRepository {
    type Target = backbone_orm::GenericCrudRepository<BomSubcontractor, backbone_orm::SoftDelete>;
    fn deref(&self) -> &Self::Target { &self.0 }
}

impl BomSubcontractorRepository {
    /// Create a new repository instance.
    pub fn new(pool: PgPool) -> Self {
        Self(backbone_orm::GenericCrudRepository::new(pool, TABLE_NAME))
    }
}

/// The exact row a BoM-subcontractor insert writes. One row per (bom, partner) — the partial
/// unique index enforces it at the DB.
pub struct NewBomSubcontractorRow {
    pub id: Uuid,
    pub company_id: Option<Uuid>,
    pub bom_id: Uuid,
    pub partner_id: Uuid,
}

impl BomSubcontractorRepository {
    /// Insert a subcontractor link. Takes the CALLER'S connection; the caller binds the company
    /// on it (`bind_company_on`) — don't re-bind.
    pub async fn insert_subcontractor(
        &self,
        conn: &mut sqlx::PgConnection,
        s: &NewBomSubcontractorRow,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"INSERT INTO manufacturing.bom_subcontractors (id, company_id, bom_id, partner_id)
               VALUES ($1,$2,$3,$4)"#,
        )
        .bind(s.id).bind(s.company_id).bind(s.bom_id).bind(s.partner_id)
        .execute(conn)
        .await?;
        Ok(())
    }

    /// Every partner registered on a BoM (the subcontract receipt path resolves the BoM by type,
    /// not by partner — the list is informational read-side data).
    pub async fn partners_for_bom(
        &self,
        pool: &PgPool,
        bom_id: Uuid,
    ) -> Result<Vec<Uuid>, sqlx::Error> {
        let rows = company_scope::fetch_all_rows_scoped(
            pool,
            sqlx::query(
                r#"SELECT partner_id FROM manufacturing.bom_subcontractors
                   WHERE bom_id=$1 AND (metadata->>'deleted_at') IS NULL"#,
            )
            .bind(bom_id),
        ).await?;
        Ok(rows.into_iter().map(|r| r.get("partner_id")).collect())
    }
}

// The generated per-entity service stack (BomSubcontractorService) type-aliases over this
// repository; the trait impl it requires rides this macro, same as every other hand-written
// repository here.
backbone_core::impl_crud_repository!(BomSubcontractorRepository, BomSubcontractor, soft_delete);
