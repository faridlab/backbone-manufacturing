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

use crate::infrastructure::persistence::{NewBomItemRow, NewBomOperationRow, NewBomRow};

use super::manufacturing_write_service::{
    is_dup, money, sixty, ManufacturingError, ManufacturingWriteService, NewBom,
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
            company_id: b.company_id,
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
}
