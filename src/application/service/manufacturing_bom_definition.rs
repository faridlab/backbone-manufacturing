//! BOM definition (hand-authored, user-owned).
//!
//! An `impl ManufacturingWriteService` chunk over the vocabulary in
//! [`super::manufacturing_write_service`]: validate the inputs, roll up component + operation cost
//! (IDR 2dp), and write the BOM header + its component + operation lines as ONE unit of work.
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on `BomRepository` /
//! `BomItemRepository` / `BomOperationRepository`, whose insert methods take THIS service's
//! transaction so a BOM is never half-authored.

use backbone_orm::company_scope;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::infrastructure::persistence::{
    NewBomByproductRow, NewBomItemRow, NewBomOperationRow, NewBomRow, NewBomSubcontractorRow,
};

use super::manufacturing_write_service::{
    is_dup, money, sixty, ManufacturingError, ManufacturingWriteService, NewBom, NewBomByproduct,
};

impl ManufacturingWriteService {
    /// Create a BOM with its component + operation lines, rolling up the cost.
    pub async fn create_bom(&self, b: NewBom) -> Result<Uuid, ManufacturingError> {
        if b.quantity <= Decimal::ZERO {
            return Err(ManufacturingError::Invalid("bom quantity must be positive".into()));
        }
        if b.items.is_empty() {
            return Err(ManufacturingError::Invalid("a bom needs at least one component".into()));
        }
        let mut raw = Decimal::ZERO;
        for it in &b.items {
            if it.quantity <= Decimal::ZERO || it.rate < Decimal::ZERO {
                return Err(ManufacturingError::Invalid("bad component qty/rate".into()));
            }
            raw += money(it.quantity * it.rate);
        }
        let mut operating = Decimal::ZERO;
        for op in &b.operations {
            operating += money(op.time_in_mins / sixty() * op.hour_rate);
        }
        let total = raw + operating;
        let id = Uuid::new_v4();
        // RLS scope (ADR-0008): company on the DTO — bind it onto the transaction so every insert below
        // passes the WITH CHECK fence. The explicit `company_id` bind stays as defense-in-depth.
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, b.company_id).await?;
        let ins = self.boms.insert_bom(&mut tx, &NewBomRow {
            id,
            // create_bom authors a COMPANY-owned BoM; shared master data is seeded via the
            // repository directly (company_id NULL — the shared_blank fence, ADR-0014).
            company_id: Some(b.company_id),
            item_id: b.item_id,
            bom_code: &b.bom_code,
            quantity: b.quantity,
            uom: b.uom.as_deref(),
            raw_material_cost: raw,
            operating_cost: operating,
            total_cost: total,
        }).await;
        if let Err(e) = ins {
            return Err(if is_dup(&e) { ManufacturingError::DuplicateNumber(b.bom_code) } else { e.into() });
        }
        for it in &b.items {
            self.bom_items.insert_component(&mut tx, &NewBomItemRow {
                id: Uuid::new_v4(),
                company_id: b.company_id,
                bom_id: id,
                item_id: it.item_id,
                quantity: it.quantity,
                rate: it.rate,
                amount: money(it.quantity * it.rate),
                is_phantom: it.is_phantom,
            }).await?;
        }
        for op in &b.operations {
            self.bom_operations.insert_operation(&mut tx, &NewBomOperationRow {
                id: Uuid::new_v4(),
                company_id: b.company_id,
                bom_id: id,
                operation_id: op.operation_id,
                workstation_id: op.workstation_id,
                time_in_mins: op.time_in_mins,
                hour_rate: op.hour_rate,
                operating_cost: money(op.time_in_mins / sixty() * op.hour_rate),
            }).await?;
        }
        tx.commit().await?;
        Ok(id)
    }

    /// Author a byproduct leg onto a BoM, guarding the family's cost-share sum at authoring
    /// time: the legs together may not carry off more than the whole batch, so a family whose
    /// shares would sum above 100 is refused LOUD (`CostShareOverflow`) — never clamped. The
    /// row-level bound (0 <= cost_share <= 100) is backstopped by the column CHECK.
    ///
    /// The row's `company_id` mirrors the parent BoM (NULL for a shared recipe), keeping the
    /// byproduct inside the BoM's fence posture; the authoring session is company-scoped.
    pub async fn add_bom_byproduct(&self, b: NewBomByproduct) -> Result<Uuid, ManufacturingError> {
        if b.quantity <= Decimal::ZERO {
            return Err(ManufacturingError::Invalid("byproduct quantity must be positive".into()));
        }
        if b.cost_share < Decimal::ZERO || b.cost_share > Decimal::from(100) {
            return Err(ManufacturingError::Invalid(
                "byproduct cost share must be between 0 and 100".into(),
            ));
        }
        // The parent BoM decides the row's fence posture (company-owned vs shared master data).
        let bom_company = company_scope::with_company_scope(
            Some(b.company_id),
            self.boms.fetch_company_id(&self.pool, b.bom_id),
        )
        .await?
        .ok_or(ManufacturingError::NotFound("bom"))?;

        // Authoring-time sum guard: existing family + this leg may not exceed the whole batch.
        let family = company_scope::with_company_scope(
            Some(b.company_id),
            self.bom_byproducts.find_by_bom(&self.pool, b.bom_id),
        )
        .await?;
        let mut share_sum = b.cost_share;
        for leg in &family {
            share_sum += leg.cost_share;
        }
        if share_sum > Decimal::from(100) {
            return Err(ManufacturingError::CostShareOverflow { sum: share_sum });
        }

        let id = Uuid::new_v4();
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, b.company_id).await?;
        self.bom_byproducts
            .insert_byproduct(&mut tx, &NewBomByproductRow {
                id,
                company_id: bom_company,
                bom_id: b.bom_id,
                item_id: b.item_id,
                product_category_id: b.product_category_id,
                quantity: b.quantity,
                cost_share: b.cost_share,
            })
            .await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Register a partner as a subcontractor on a BoM (read-only reference data: it records WHO
    /// may produce the recipe externally — this module has no partner write surface). The row's
    /// `company_id` mirrors the parent BoM; the (bom, partner) pair is unique at the DB.
    pub async fn add_bom_subcontractor(
        &self,
        company_id: Uuid,
        bom_id: Uuid,
        partner_id: Uuid,
    ) -> Result<Uuid, ManufacturingError> {
        let bom_company = company_scope::with_company_scope(
            Some(company_id),
            self.boms.fetch_company_id(&self.pool, bom_id),
        )
        .await?
        .ok_or(ManufacturingError::NotFound("bom"))?;
        let id = Uuid::new_v4();
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company_id).await?;
        let ins = self
            .bom_subcontractors
            .insert_subcontractor(&mut tx, &NewBomSubcontractorRow {
                id,
                company_id: bom_company,
                bom_id,
                partner_id,
            })
            .await;
        if let Err(e) = ins {
            return Err(if is_dup(&e) {
                ManufacturingError::Invalid("partner is already a subcontractor on this bom".into())
            } else {
                e.into()
            });
        }
        tx.commit().await?;
        Ok(id)
    }

    /// The partners registered as subcontractors on a BoM (informational read side).
    pub async fn list_bom_subcontractors(
        &self,
        company_id: Uuid,
        bom_id: Uuid,
    ) -> Result<Vec<Uuid>, ManufacturingError> {
        Ok(company_scope::with_company_scope(
            Some(company_id),
            self.bom_subcontractors.partners_for_bom(&self.pool, bom_id),
        )
        .await?)
    }
}
