//! BOM definition (hand-authored, user-owned).
//!
//! An `impl ManufacturingWriteService` chunk over the vocabulary in
//! [`super::manufacturing_write_service`]: validate the inputs, roll up component + operation cost
//! (IDR 2dp), and write the BOM header + its component + operation lines as ONE unit of work.
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on `BomRepository` /
//! `BomItemRepository` / `BomOperationRepository`, whose insert methods take THIS service's
//! transaction so a BOM is never half-authored.
//!
//! Tenancy (ADR-0029): the module is tenant-agnostic — no company argument exists on any verb
//! here. Transactions relay the ambient org scope (the composing service sets it per request);
//! pool reads ride the caller-scoped helpers undecorated.

use rust_decimal::Decimal;
use uuid::Uuid;

use crate::infrastructure::persistence::{
    NewBomByproductRow, NewBomItemRow, NewBomOperationRow, NewBomRow, NewBomSubcontractorRow,
};

use super::manufacturing_write_service::{
    is_dup, money, relay_ambient_scope, sixty, ManufacturingError, ManufacturingWriteService,
    NewBom, NewBomByproduct,
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
        let mut tx = self.pool.begin().await?;
        // The ambient org scope — every insert below rides the composed decorator's fence
        // (ADR-0029). Undecorated, the tx stays plain.
        relay_ambient_scope(&mut tx).await?;
        let ins = self.boms.insert_bom(&mut tx, &NewBomRow {
            id,
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
    pub async fn add_bom_byproduct(&self, b: NewBomByproduct) -> Result<Uuid, ManufacturingError> {
        if b.quantity <= Decimal::ZERO {
            return Err(ManufacturingError::Invalid("byproduct quantity must be positive".into()));
        }
        if b.cost_share < Decimal::ZERO || b.cost_share > Decimal::from(100) {
            return Err(ManufacturingError::Invalid(
                "byproduct cost share must be between 0 and 100".into(),
            ));
        }
        // The parent BoM must be visible to the authoring caller; under the composed decorator
        // the org fence decides (ADR-0029).
        let _parent = self
            .boms
            .fetch_bom_type(&self.pool, b.bom_id)
            .await?
            .ok_or(ManufacturingError::NotFound("bom"))?;

        // Authoring-time sum guard: existing family + this leg may not exceed the whole batch.
        let family = self.bom_byproducts.find_by_bom(&self.pool, b.bom_id).await?;
        let mut share_sum = b.cost_share;
        for leg in &family {
            share_sum += leg.cost_share;
        }
        if share_sum > Decimal::from(100) {
            return Err(ManufacturingError::CostShareOverflow { sum: share_sum });
        }

        let id = Uuid::new_v4();
        let mut tx = self.pool.begin().await?;
        // The ambient org scope — the byproduct lands behind the composed decorator's fence
        // (ADR-0029). Undecorated, the tx stays plain.
        relay_ambient_scope(&mut tx).await?;
        self.bom_byproducts
            .insert_byproduct(&mut tx, &NewBomByproductRow {
                id,
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
    /// may produce the recipe externally — this module has no partner write surface). The
    /// (bom, partner) pair is unique at the DB.
    pub async fn add_bom_subcontractor(
        &self,
        bom_id: Uuid,
        partner_id: Uuid,
    ) -> Result<Uuid, ManufacturingError> {
        let _parent = self
            .boms
            .fetch_bom_type(&self.pool, bom_id)
            .await?
            .ok_or(ManufacturingError::NotFound("bom"))?;
        let id = Uuid::new_v4();
        let mut tx = self.pool.begin().await?;
        // The ambient org scope — the link lands behind the composed decorator's fence
        // (ADR-0029). Undecorated, the tx stays plain.
        relay_ambient_scope(&mut tx).await?;
        let ins = self
            .bom_subcontractors
            .insert_subcontractor(&mut tx, &NewBomSubcontractorRow {
                id,
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
        bom_id: Uuid,
    ) -> Result<Vec<Uuid>, ManufacturingError> {
        Ok(self.bom_subcontractors.partners_for_bom(&self.pool, bom_id).await?)
    }
}
