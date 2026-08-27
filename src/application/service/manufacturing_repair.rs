//! Repair: the repair verb chain (hand-authored, user-owned).
//!
//! An `impl ManufacturingWriteService` chunk over the vocabulary in
//! [`super::manufacturing_write_service`]. A repair order fixes a broken item with three kinds of
//! part leg, hand-set and verb-driven: draft → confirmed (validate — part availability checked
//! through the inventory port) → under_repair (start; auto-confirms a draft) → done (end) | cancel.
//!
//! NO leg moves stock until `end_repair` — which executes EVERY leg in one pass, each through
//! [`InventoryPort::execute_repair_leg`], so a cancel before end needs no move cancellation. The
//! legs' GL, one grouped balanced post:
//!   add      Dr Repair-Expense · Cr Raw-Material Stock   (part consumed into the repair)
//!   remove   Dr Inventory-Loss · Cr Raw-Material Stock   (part scrapped — to the LOSS ACCOUNT,
//!                                                         never a Location row; no quarantine
//!                                                         location exists anywhere in the family)
//!   recycle  Dr Raw-Material Stock · Cr Repair-Expense   (part recovered back into stock)
//!
//! `done` is uncancelable — the legs have moved real stock. NO fees exist anywhere in the family:
//! repair billing is not this module's surface.
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on `RepairRepository`.

use backbone_orm::company_scope;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::infrastructure::persistence::{NewRepairOrderRow, NewRepairPartRow};

use super::manufacturing_events::*;
use super::manufacturing_gl::{AccountingPostEnvelope, GlPostLine, GlPostSink};
use super::manufacturing_ports::{InventoryPort, RepairAvailability, RepairLeg};
use super::manufacturing_write_service::{
    money, is_dup, ManufacturingError, ManufacturingWriteService, NewRepairOrder,
    RepairEndOutcome,
};

impl ManufacturingWriteService {
    /// Open a draft repair order with its part lines. Nothing moves yet — legs execute only at
    /// [`Self::end_repair`].
    pub async fn create_repair_order(&self, o: NewRepairOrder) -> Result<Uuid, ManufacturingError> {
        if o.quantity <= Decimal::ZERO {
            return Err(ManufacturingError::Invalid("repair quantity must be positive".into()));
        }
        for p in &o.parts {
            if p.quantity <= Decimal::ZERO {
                return Err(ManufacturingError::Invalid("repair part quantity must be positive".into()));
            }
            if p.rate < Decimal::ZERO {
                return Err(ManufacturingError::Invalid("repair part rate must not be negative".into()));
            }
        }
        let id = Uuid::new_v4();
        // RLS scope (ADR-0008): company on the DTO — scope the inserts so they pass the WITH CHECK fence.
        let r = company_scope::with_company_scope(
            Some(o.company_id),
            self.repairs.insert_repair_order(&self.pool, &NewRepairOrderRow {
                id,
                company_id: o.company_id,
                repair_number: &o.repair_number,
                item_id: o.item_id,
                product_category_id: o.product_category_id,
                quantity: o.quantity,
            }),
        )
        .await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { ManufacturingError::DuplicateNumber(o.repair_number) } else { e.into() });
        }
        for p in &o.parts {
            company_scope::with_company_scope(
                Some(o.company_id),
                self.repairs.insert_repair_part(&self.pool, &NewRepairPartRow {
                    id: Uuid::new_v4(),
                    company_id: o.company_id,
                    repair_order_id: id,
                    item_id: p.item_id,
                    warehouse_id: p.warehouse_id,
                    line_type: p.line_type,
                    quantity: p.quantity,
                    rate: p.rate,
                }),
            )
            .await?;
        }
        Ok(id)
    }

    /// Validate a draft: every ADD leg's part must be on hand (probed read-only through the
    /// inventory port) — a shortfall is LOUD, the draft stays draft. Remove/recycle legs draw
    /// nothing from stock and are not probed. Then draft → confirmed.
    ///
    /// The probe is advisory-checked, authority-held: the authoritative refusal is the
    /// end-of-repair leg execution itself (which will fail loudly if stock left in between).
    pub async fn validate_repair(
        &self,
        repair_id: Uuid,
        inventory: &dyn InventoryPort,
    ) -> Result<(), ManufacturingError> {
        let o = self.repairs.find_order(&self.pool, repair_id).await?
            .ok_or(ManufacturingError::NotFound("repair order"))?;
        if o.status != "draft" {
            return Err(ManufacturingError::RepairInvalidState("repair order is not draft"));
        }
        let parts = company_scope::with_company_scope(
            Some(o.company_id),
            self.repairs.find_parts(&self.pool, repair_id),
        )
        .await?;
        for p in &parts {
            if p.line_type == "add" {
                inventory
                    .check_repair_availability(&RepairAvailability {
                        company_id: o.company_id,
                        item_id: p.item_id,
                        warehouse_id: p.warehouse_id,
                        quantity: p.quantity,
                    })
                    .await
                    .map_err(|r| ManufacturingError::Inventory(r.code))?;
            }
        }
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, o.company_id).await?;
        let moved = self.repairs.gate_validate(&mut tx, repair_id).await?;
        if moved != 1 {
            tx.rollback().await?;
            return Err(ManufacturingError::RepairInvalidState("repair order is not draft"));
        }
        tx.commit().await?;
        Ok(())
    }

    /// Start the repair: under_repair. A DRAFT is auto-confirmed first — the operator starting
    /// work has implicitly accepted part availability.
    pub async fn start_repair(&self, repair_id: Uuid) -> Result<(), ManufacturingError> {
        let o = self.repairs.find_order(&self.pool, repair_id).await?
            .ok_or(ManufacturingError::NotFound("repair order"))?;
        match o.status.as_str() {
            "draft" | "confirmed" => {}
            "under_repair" => return Ok(()), // idempotent
            "done" => return Err(ManufacturingError::RepairInvalidState(
                "repair order is done — it cannot be started",
            )),
            "cancel" => return Err(ManufacturingError::RepairInvalidState(
                "repair order is cancelled",
            )),
            _ => return Err(ManufacturingError::RepairInvalidState("repair order state does not allow start")),
        }
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, o.company_id).await?;
        self.repairs.gate_auto_confirm(&mut tx, repair_id).await?;
        let moved = self.repairs.gate_start(&mut tx, repair_id).await?;
        if moved != 1 {
            tx.rollback().await?;
            return Err(ManufacturingError::RepairInvalidState("repair order could not start"));
        }
        tx.commit().await?;
        Ok(())
    }

    /// End the repair: execute EVERY part leg in one pass and post the grouped legs post —
    /// once-only (the under_repair → done gate is the guard; each leg's port call is idempotent
    /// on `repair-leg:{repair}:{index}`).
    ///
    /// If any leg fails, nothing is gated: the executed legs' idempotency keys make the retry
    /// safe (already-moved legs no-op), the failed one re-attempts, and the order stays
    /// under_repair — LOUD, never a silent partial settlement.
    pub async fn end_repair(
        &self,
        repair_id: Uuid,
        inventory: &dyn InventoryPort,
        gl: &dyn GlPostSink,
        sink: &dyn ManufacturingEventSink,
    ) -> Result<RepairEndOutcome, ManufacturingError> {
        let o = self.repairs.find_order(&self.pool, repair_id).await?
            .ok_or(ManufacturingError::NotFound("repair order"))?;
        if o.status == "done" {
            return Ok(RepairEndOutcome {
                parts_moved: 0,
                repair_expense: Decimal::ZERO,
                inventory_loss: Decimal::ZERO,
                recovered_value: Decimal::ZERO,
            });
        }
        if o.status != "under_repair" {
            return Err(ManufacturingError::RepairInvalidState(
                "repair order is not under repair — start it first",
            ));
        }
        let parts = company_scope::with_company_scope(
            Some(o.company_id),
            self.repairs.find_parts(&self.pool, repair_id),
        )
        .await?;

        // Accounts resolve through the same chain as every other costing path: category default
        // → LOUD MissingAccount (a repair carries no per-order account overrides).
        let repair_expense_acct = self
            .resolve_account("repair_expense", None, o.company_id, o.product_category_id, |d| d.repair_expense_account_id)
            .await?;
        let inventory_loss_acct = self
            .resolve_account("inventory_loss", None, o.company_id, o.product_category_id, |d| d.inventory_loss_account_id)
            .await?;
        let raw_acct = self
            .resolve_account("raw_material", None, o.company_id, o.product_category_id, |d| d.raw_material_account_id)
            .await?;

        // SIDE EFFECTS BEFORE THE GATE: every leg through the port, idempotent per leg key.
        let mut inventory_loss = Decimal::ZERO; // Σ(remove)
        let mut add_total = Decimal::ZERO;
        let mut recycle_total = Decimal::ZERO;
        for (idx, p) in parts.iter().enumerate() {
            let value = money(p.quantity * p.rate);
            let key = format!("repair-leg:{repair_id}:{idx}");
            inventory
                .execute_repair_leg(&RepairLeg {
                    company_id: o.company_id,
                    repair_order_id: repair_id,
                    item_id: p.item_id,
                    warehouse_id: p.warehouse_id,
                    line_type: p.line_type.clone(),
                    quantity: p.quantity,
                    rate: p.rate,
                    idempotency_key: key,
                })
                .await
                .map_err(|r| ManufacturingError::Inventory(r.code))?;
            match p.line_type.as_str() {
                "add" => { add_total += value; }
                "remove" => { inventory_loss += value; }
                "recycle" => { recycle_total += value; }
                _ => {}
            }
        }
        let repair_expense = add_total + recycle_total; // Σ(add) + Σ(recycle) against the expense account
        let recovered_value = recycle_total; // Σ(recycle)

        // The grouped legs post — balanced by construction (each leg is a same-amount Dr/Cr pair):
        //   Dr Repair-Expense Σ(add) · Dr Inventory-Loss Σ(remove) · Dr Raw Σ(recycle)
        //   Cr Raw Σ(add)+Σ(remove) · Cr Repair-Expense Σ(recycle)
        if repair_expense + inventory_loss > Decimal::ZERO {
            let mut lines = Vec::new();
            if add_total > Decimal::ZERO {
                lines.push(GlPostLine::debit(repair_expense_acct, add_total).with_description("Repair parts consumed"));
            }
            if inventory_loss > Decimal::ZERO {
                lines.push(GlPostLine::debit(inventory_loss_acct, inventory_loss).with_description("Scrapped repair parts"));
            }
            if recycle_total > Decimal::ZERO {
                lines.push(GlPostLine::debit(raw_acct, recycle_total).with_description("Recovered repair parts"));
            }
            if add_total + inventory_loss > Decimal::ZERO {
                lines.push(GlPostLine::credit(raw_acct, add_total + inventory_loss).with_description("Raw material stock"));
            }
            if recycle_total > Decimal::ZERO {
                lines.push(GlPostLine::credit(repair_expense_acct, recycle_total).with_description("Repair part recovery"));
            }
            let env = AccountingPostEnvelope {
                idempotency_key: format!("repair:{repair_id}"),
                company_id: o.company_id,
                branch_id: None,
                source_type: "manufacturing".into(),
                source_id: Uuid::new_v5(&repair_id, b"manufacturing:repair"),
                source_reference: Some(repair_id.to_string()),
                posting_date: chrono::Utc::now().date_naive(),
                currency: "IDR".into(),
                posting_type: "original".into(),
                description: Some("repair part legs".into()),
                lines,
            };
            self.post(gl, &env).await?;
        }

        // THE GATE, last: under_repair → done (once-only, terminal).
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, o.company_id).await?;
        let moved = self.repairs.gate_end(&mut tx, repair_id).await?;
        if moved != 1 {
            tx.rollback().await?;
            return Ok(RepairEndOutcome {
                parts_moved: parts.len(),
                repair_expense,
                inventory_loss,
                recovered_value,
            });
        }
        tx.commit().await?;

        let outcome = RepairEndOutcome {
            parts_moved: parts.len(),
            repair_expense,
            inventory_loss,
            recovered_value,
        };
        sink.publish(&ManufacturingEvent::RepairCompleted(RepairCompleted {
            repair_order_id: repair_id,
            company_id: o.company_id,
            item_id: o.item_id,
            repair_expense,
            inventory_loss,
            recovered_value,
        }));
        Ok(outcome)
    }

    /// Cancel a repair: draft|confirmed|under_repair → cancel (terminal).
    ///
    /// Cancel BEFORE end needs no move cancellation — legs only ever move at `end`. A `done`
    /// repair has moved real stock; it is uncancelable, LOUDLY.
    pub async fn cancel_repair(&self, repair_id: Uuid) -> Result<(), ManufacturingError> {
        let o = self.repairs.find_order(&self.pool, repair_id).await?
            .ok_or(ManufacturingError::NotFound("repair order"))?;
        match o.status.as_str() {
            "draft" | "confirmed" | "under_repair" => {}
            "done" => return Err(ManufacturingError::RepairInvalidState(
                "repair order is done — its part legs have moved real stock and cannot be cancelled",
            )),
            "cancel" => return Err(ManufacturingError::RepairInvalidState("repair order is already cancelled")),
            _ => return Err(ManufacturingError::RepairInvalidState("repair order state does not allow cancel")),
        }
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, o.company_id).await?;
        let moved = self.repairs.gate_cancel(&mut tx, repair_id).await?;
        if moved != 1 {
            tx.rollback().await?;
            return Err(ManufacturingError::RepairInvalidState(
                "repair order has left the cancellable states",
            ));
        }
        tx.commit().await?;
        Ok(())
    }

    /// Create (or reuse) a tenant-scoped repair tag — the family's sole DB-level guard is the
    /// unique (company, name); a duplicate name is a LOUD domain error, not an overwrite.
    pub async fn create_repair_tag(&self, company_id: Uuid, name: String) -> Result<Uuid, ManufacturingError> {
        if name.trim().is_empty() {
            return Err(ManufacturingError::Invalid("repair tag name must not be empty".into()));
        }
        if self.repairs.find_tag_by_name(&self.pool, company_id, &name).await?.is_some() {
            return Err(ManufacturingError::Invalid(format!("repair tag '{name}' already exists")));
        }
        let id = Uuid::new_v4();
        let r = company_scope::with_company_scope(
            Some(company_id),
            self.repairs.insert_tag(&self.pool, id, company_id, &name),
        )
        .await;
        if let Err(e) = r {
            return Err(if is_dup(&e) {
                ManufacturingError::Invalid(format!("repair tag '{name}' already exists"))
            } else {
                e.into()
            });
        }
        Ok(id)
    }
}
