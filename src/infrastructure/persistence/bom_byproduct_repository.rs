//! Repository for BomByproduct entities
//!
//! Hand-written (declared under `user_owned` in `metaphor.codegen.yaml`). A byproduct row is
//! master data riding its BoM: cost_share is the PERCENT of the batch's input value the
//! byproduct carries off (0–100, CHECKed in DDL; the authoring path refuses a family whose
//! shares sum above 100 BEFORE any receipt can mint legs from it).

use anyhow::Result;
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use backbone_orm::company_scope;

use crate::domain::entity::BomByproduct;

/// Table name for BomByproduct entities
pub const TABLE_NAME: &str = "manufacturing.bom_byproducts";

/// Repository for BomByproduct entities.
pub struct BomByproductRepository(
    backbone_orm::GenericCrudRepository<BomByproduct, backbone_orm::SoftDelete>,
);

impl std::ops::Deref for BomByproductRepository {
    type Target = backbone_orm::GenericCrudRepository<BomByproduct, backbone_orm::SoftDelete>;
    fn deref(&self) -> &Self::Target { &self.0 }
}

impl BomByproductRepository {
    /// Create a new repository instance.
    pub fn new(pool: PgPool) -> Self {
        Self(backbone_orm::GenericCrudRepository::new(pool, TABLE_NAME))
    }
}

/// The exact row a byproduct insert writes.
pub struct NewBomByproductRow {
    pub id: Uuid,
    pub company_id: Option<Uuid>,
    pub bom_id: Uuid,
    pub item_id: Uuid,
    pub product_category_id: Option<Uuid>,
    pub quantity: Decimal,
    pub cost_share: Decimal,
}

/// A byproduct leg the receipt path splits value across.
pub struct BomByproductRow {
    pub item_id: Uuid,
    pub product_category_id: Option<Uuid>,
    pub quantity: Decimal,
    pub cost_share: Decimal,
}

impl BomByproductRepository {
    /// Insert a byproduct row.
    ///
    /// Takes the CALLER'S connection so the byproduct lands atomically with the BoM header it
    /// rides; the caller binds the company on it (`bind_company_on`) — don't re-bind.
    pub async fn insert_byproduct(
        &self,
        conn: &mut sqlx::PgConnection,
        b: &NewBomByproductRow,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"INSERT INTO manufacturing.bom_byproducts
                 (id, company_id, bom_id, item_id, product_category_id, quantity, cost_share)
               VALUES ($1,$2,$3,$4,$5,$6,$7)"#,
        )
        .bind(b.id).bind(b.company_id).bind(b.bom_id).bind(b.item_id)
        .bind(b.product_category_id).bind(b.quantity).bind(b.cost_share)
        .execute(conn)
        .await?;
        Ok(())
    }

    /// Read every byproduct leg of a BoM, oldest first (the receipt path mints legs in a stable
    /// order so the value split is deterministic).
    pub async fn find_by_bom(
        &self,
        pool: &PgPool,
        bom_id: Uuid,
    ) -> Result<Vec<BomByproductRow>, sqlx::Error> {
        let rows = company_scope::fetch_all_rows_scoped(
            pool,
            sqlx::query(
                r#"SELECT item_id, product_category_id, quantity, cost_share
                   FROM manufacturing.bom_byproducts
                   WHERE bom_id=$1 AND (metadata->>'deleted_at') IS NULL
                   ORDER BY (metadata->>'created_at') ASC, id ASC"#,
            )
            .bind(bom_id),
        ).await?;
        Ok(rows
            .into_iter()
            .map(|r| BomByproductRow {
                item_id: r.get("item_id"),
                product_category_id: r.get("product_category_id"),
                quantity: r.get("quantity"),
                cost_share: r.get("cost_share"),
            })
            .collect())
    }
}

// The generated per-entity service stack (BomByproductService) type-aliases over this repository;
// the trait impl it requires rides this macro, same as every other hand-written repository here.
backbone_core::impl_crud_repository!(BomByproductRepository, BomByproduct, soft_delete);
