//! The hand-authored manufacturing write path (user-owned; survives regen).
//!
//! Product definition: `create_bom` rolls up component + operation cost. Execution: a Work Order's
//! value flows through WIP in three balanced posts and nets WIP to ZERO on completion —
//!   consume  Dr WIP · Cr Raw-Material Stock   (materials issued; value from real inventory)
//!   operate  Dr WIP · Cr Conversion-Applied   (job-card labour/overhead)
//!   receive  Dr Finished-Goods · Cr WIP        (FG = raw + operating)
//! Each post is transition-gated (the status advance is the once-only guard) and carries a stable
//! idempotency key, so a retry never double-charges WIP. Manufacturing owns no stock or ledger: it
//! drives inventory (InventoryPort) for the physical moves + valuation and emits the posts (GlPostSink).
//! Money is IDR, 2dp, half-away-from-zero.

use backbone_orm::company_scope;
use rust_decimal::{Decimal, RoundingStrategy};
use sqlx::PgPool;
use uuid::Uuid;

use crate::infrastructure::persistence::{
    BomItemRepository, BomOperationRepository, BomRepository, JobCardRepository, NewBomItemRow,
    NewBomOperationRow, NewBomRow, NewJobCardRow, NewWorkOrderItemRow, NewWorkOrderRow,
    WorkOrderItemRepository, WorkOrderRepository, WorkOrderRow,
};

use super::manufacturing_events::*;
use super::manufacturing_gl::{AccountingPostEnvelope, GlPostLine, GlPostSink};
use super::manufacturing_ports::{FinishedReceipt, InventoryPort, IssueLine, MaterialIssue};

fn money(v: Decimal) -> Decimal {
    v.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
}

fn sixty() -> Decimal {
    Decimal::from(60)
}

#[derive(Debug, thiserror::Error)]
pub enum ManufacturingError {
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
    #[error("not found: {0}")]
    NotFound(&'static str),
    #[error("invalid state: {0}")]
    InvalidState(&'static str),
    #[error("missing account: {0}")]
    MissingAccount(&'static str),
    #[error("over-produce: producing {producing} would exceed the {ordered} ordered")]
    OverProduce { producing: Decimal, ordered: Decimal },
    #[error("inventory rejected: {0}")]
    Inventory(String),
    #[error("gl rejected: {0}")]
    Gl(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("duplicate number: {0}")]
    DuplicateNumber(String),
}

// ---- request DTOs ----------------------------------------------------------------------------

pub struct NewBomItem {
    pub item_id: Uuid,
    pub quantity: Decimal,
    pub rate: Decimal,
    /// A phantom sub-assembly — exploded through to its own BOM's components at release, never issued.
    pub is_phantom: bool,
}
pub struct NewBomOperation {
    pub operation_id: Uuid,
    pub workstation_id: Uuid,
    pub time_in_mins: Decimal,
    pub hour_rate: Decimal,
}
pub struct NewBom {
    pub company_id: Uuid,
    pub item_id: Uuid,
    pub bom_code: String,
    pub quantity: Decimal,
    pub uom: Option<String>,
    pub items: Vec<NewBomItem>,
    pub operations: Vec<NewBomOperation>,
}

pub struct NewWorkOrder {
    pub company_id: Uuid,
    pub work_order_number: String,
    pub item_id: Uuid,
    pub bom_id: Uuid,
    pub quantity: Decimal,
    pub wip_warehouse_id: Option<Uuid>,
    pub fg_warehouse_id: Option<Uuid>,
    pub wip_account_id: Option<Uuid>,
    pub fg_account_id: Option<Uuid>,
    pub raw_material_account_id: Option<Uuid>,
    pub conversion_cost_account_id: Option<Uuid>,
}

pub struct NewJobCard {
    pub company_id: Uuid,
    pub work_order_id: Uuid,
    pub operation_id: Uuid,
    pub workstation_id: Uuid,
    pub total_time_mins: Decimal,
    pub hour_rate: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConsumeOutcome {
    pub raw_material_value: Decimal,
    pub already: bool,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ReceiveOutcome {
    pub finished_value: Decimal,
    pub completed: bool,
    pub already: bool,
}

pub struct ManufacturingWriteService {
    pool: PgPool,
    boms: BomRepository,
    bom_items: BomItemRepository,
    bom_operations: BomOperationRepository,
    work_orders: WorkOrderRepository,
    work_order_items: WorkOrderItemRepository,
    job_cards: JobCardRepository,
}

impl ManufacturingWriteService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            boms: BomRepository::new(pool.clone()),
            bom_items: BomItemRepository::new(pool.clone()),
            bom_operations: BomOperationRepository::new(pool.clone()),
            work_orders: WorkOrderRepository::new(pool.clone()),
            work_order_items: WorkOrderItemRepository::new(pool.clone()),
            job_cards: JobCardRepository::new(pool.clone()),
            pool,
        }
    }

    // ---- product definition -----------------------------------------------------------------

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

    // ---- execution --------------------------------------------------------------------------

    /// Create a draft Work Order.
    pub async fn create_work_order(&self, o: NewWorkOrder) -> Result<Uuid, ManufacturingError> {
        if o.quantity <= Decimal::ZERO {
            return Err(ManufacturingError::Invalid("work order quantity must be positive".into()));
        }
        let id = Uuid::new_v4();
        // RLS scope (ADR-0008): company on the DTO — scope the insert so it passes the WITH CHECK fence.
        let r = company_scope::with_company_scope(
            Some(o.company_id),
            self.work_orders.insert_draft(&self.pool, &NewWorkOrderRow {
                id,
                company_id: o.company_id,
                work_order_number: &o.work_order_number,
                item_id: o.item_id,
                bom_id: o.bom_id,
                quantity: o.quantity,
                wip_warehouse_id: o.wip_warehouse_id,
                fg_warehouse_id: o.fg_warehouse_id,
                wip_account_id: o.wip_account_id,
                fg_account_id: o.fg_account_id,
                raw_material_account_id: o.raw_material_account_id,
                conversion_cost_account_id: o.conversion_cost_account_id,
            }),
        )
        .await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { ManufacturingError::DuplicateNumber(o.work_order_number) } else { e.into() });
        }
        Ok(id)
    }

    /// Release a draft Work Order: explode its BOM into required materials, draft → released.
    pub async fn release_work_order(
        &self,
        wo_id: Uuid,
        sink: &dyn ManufacturingEventSink,
    ) -> Result<(), ManufacturingError> {
        // RLS scope (ADR-0008), ID-only pattern: identified by the work-order id alone, so there is no
        // company to bind before the transaction opens. Read the header first through the scoped helper
        // (it rides the REQUEST-dedicated connection carrying the caller's `app.company_id`, so another
        // company's work order simply isn't found), then bind ITS company onto the transaction below.
        // The once-only guard is unaffected: it remains the in-transaction draft→released gate.
        let wo = self.work_orders.find_release_source(&self.pool, wo_id).await?
            .ok_or(ManufacturingError::NotFound("work order"))?;
        if wo.status != "draft" {
            return Err(ManufacturingError::InvalidState("work order is not draft"));
        }
        let company_id: Uuid = wo.company_id;
        let item_id: Uuid = wo.item_id;
        let bom_id: Uuid = wo.bom_id;
        let wo_qty: Decimal = wo.quantity;

        // Explode the BOM into required materials, recursing THROUGH phantom sub-assemblies to their
        // own components (a phantom is never stocked). Deterministic + static — no MRP.
        let mut required: Vec<(Uuid, Decimal, Decimal)> = Vec::new();
        self.explode_bom(company_id, bom_id, wo_qty, 0, &mut required).await?;

        // Gate the explosion on the draft→released transition (once-only).
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company_id).await?;
        let moved = self.work_orders.gate_release(&mut tx, wo_id).await?;
        if moved != 1 {
            return Err(ManufacturingError::InvalidState("work order is not draft"));
        }

        for (item, qty, rate) in &required {
            self.work_order_items.insert_requirement(&mut tx, &NewWorkOrderItemRow {
                id: Uuid::new_v4(),
                work_order_id: wo_id,
                item_id: *item,
                required_qty: *qty,
                rate: *rate,
            }).await?;
        }
        tx.commit().await?;
        sink.publish(&ManufacturingEvent::WorkOrderReleased(WorkOrderReleased {
            work_order_id: wo_id,
            company_id,
            item_id,
            quantity: wo_qty,
        }));
        Ok(())
    }

    /// Recursively flatten a BOM into leaf material requirements for `want_units` of its output.
    /// A **phantom** component is never issued — it is exploded through to its own BOM's components
    /// (resolved by the phantom item's active BOM). Real components accumulate as `(item, qty, rate)`.
    /// A depth cap guards against a mis-authored phantom cycle.
    fn explode_bom<'a>(
        &'a self,
        company_id: Uuid,
        bom_id: Uuid,
        want_units: Decimal,
        depth: u32,
        out: &'a mut Vec<(Uuid, Decimal, Decimal)>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ManufacturingError>> + Send + 'a>>
    {
        Box::pin(async move {
            if depth > 8 {
                return Err(ManufacturingError::Invalid("phantom BOM nesting too deep (cycle?)".into()));
            }
            // RLS scope (ADR-0008): the company is on the parameter — bind it around each read so the
            // explosion is fenced even when driven by a non-request caller (job / event subscriber).
            let base: Decimal = company_scope::with_company_scope(
                Some(company_id),
                self.boms.fetch_output_quantity(&self.pool, bom_id),
            )
            .await?
            .ok_or(ManufacturingError::NotFound("bom"))?;

            let comps = company_scope::with_company_scope(
                Some(company_id),
                self.bom_items.list_components(&self.pool, bom_id),
            )
            .await?;

            for c in &comps {
                let needed = c.quantity * want_units / base;
                if c.is_phantom {
                    // Resolve the phantom item's own BOM (default first) and explode through it.
                    let child_bom: Uuid = company_scope::with_company_scope(
                        Some(company_id),
                        self.boms.find_active_bom_for_item(&self.pool, company_id, c.item_id),
                    )
                    .await?
                    .ok_or(ManufacturingError::Invalid("phantom component has no BOM".into()))?;
                    self.explode_bom(company_id, child_bom, needed, depth + 1, out).await?;
                } else {
                    out.push((c.item_id, needed, c.rate));
                }
            }
            Ok(())
        })
    }

    /// Issue all required materials to WIP: drive inventory (value from moving-average) + post
    /// `Dr WIP · Cr Raw-Material Stock`. Gated released → in_process (the once-only consume).
    pub async fn consume_materials(
        &self,
        wo_id: Uuid,
        raw_warehouse_id: Uuid,
        inventory: &dyn InventoryPort,
        gl: &dyn GlPostSink,
        sink: &dyn ManufacturingEventSink,
    ) -> Result<ConsumeOutcome, ManufacturingError> {
        let wo = self.load_wo(wo_id).await?;
        if wo.status == "in_process" || wo.status == "completed" {
            return Ok(ConsumeOutcome { raw_material_value: wo.raw_material_cost, already: true });
        }
        if wo.status != "released" {
            return Err(ManufacturingError::InvalidState("work order is not released"));
        }
        let wip = wo.wip_account_id.ok_or(ManufacturingError::MissingAccount("wip"))?;
        let raw_acct = wo.raw_material_account_id.ok_or(ManufacturingError::MissingAccount("raw_material"))?;

        // RLS scope (ADR-0008): `load_wo` read the work order (fenced by the request connection), so its
        // company is known here — bind it explicitly for the line read and the transaction below.
        let items = company_scope::with_company_scope(
            Some(wo.company_id),
            self.work_order_items.list_requirements(&self.pool, wo_id),
        )
        .await?;
        let mut lines = Vec::new();
        for it in &items {
            let remaining = it.required_qty - it.consumed_qty;
            if remaining > Decimal::ZERO {
                lines.push((it.id, it.item_id, remaining));
            }
        }
        if lines.is_empty() {
            return Ok(ConsumeOutcome { raw_material_value: wo.raw_material_cost, already: true });
        }

        // Drive real inventory to remove the components and tell us what they were worth.
        let issue = MaterialIssue {
            company_id: wo.company_id,
            work_order_id: wo_id,
            warehouse_id: raw_warehouse_id,
            idempotency_key: format!("consume:{wo_id}"),
            lines: lines.iter().map(|(_, item, qty)| IssueLine { item_id: *item, quantity: *qty }).collect(),
        };
        let ack = inventory
            .issue_to_wip(&issue)
            .await
            .map_err(|r| ManufacturingError::Inventory(r.code))?;
        let raw_value = money(ack.total_value);

        // Emit the consume post: Dr WIP · Cr Raw-Material Stock.
        let env = AccountingPostEnvelope {
            idempotency_key: format!("consume:{wo_id}"),
            company_id: wo.company_id,
            branch_id: None,
            source_type: "manufacturing".into(),
            // Each manufacturing post is a distinct voucher; accounting dedups on (company, source_type,
            // source_id, posting_type='original'), so derive a stable source_id per post kind.
            source_id: Uuid::new_v5(&wo_id, b"manufacturing:consume"),
            source_reference: Some(wo.work_order_number.clone()),
            posting_date: chrono::Utc::now().date_naive(),
            currency: "IDR".into(),
            posting_type: "original".into(),
            description: Some("material issue to WIP".into()),
            lines: vec![
                GlPostLine::debit(wip, raw_value).with_description("WIP"),
                GlPostLine::credit(raw_acct, raw_value).with_description("Raw material stock"),
            ],
        };
        self.post(gl, &env).await?;

        // Record consumption + advance state, gated on released → in_process.
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, wo.company_id).await?;
        let moved = self.work_orders.gate_consume(&mut tx, wo_id, raw_value).await?;
        if moved != 1 {
            // Someone else consumed concurrently — the post deduped; don't double-book state.
            tx.rollback().await?;
            let now = self.load_wo(wo_id).await?;
            return Ok(ConsumeOutcome { raw_material_value: now.raw_material_cost, already: true });
        }
        for (line_id, _item, qty) in &lines {
            self.work_order_items.add_consumed_qty(&mut tx, *line_id, *qty).await?;
        }
        tx.commit().await?;
        sink.publish(&ManufacturingEvent::MaterialsConsumed(MaterialsConsumed {
            work_order_id: wo_id,
            company_id: wo.company_id,
            raw_material_value: raw_value,
        }));
        Ok(ConsumeOutcome { raw_material_value: raw_value, already: false })
    }

    /// Record a job card for an operation run (open).
    pub async fn add_job_card(&self, j: NewJobCard) -> Result<Uuid, ManufacturingError> {
        if j.total_time_mins < Decimal::ZERO || j.hour_rate < Decimal::ZERO {
            return Err(ManufacturingError::Invalid("bad time/rate".into()));
        }
        let id = Uuid::new_v4();
        let cost = money(j.total_time_mins / sixty() * j.hour_rate);
        // RLS scope (ADR-0008): company on the DTO — scope the insert so it passes the WITH CHECK fence.
        company_scope::with_company_scope(
            Some(j.company_id),
            self.job_cards.insert_open(&self.pool, &NewJobCardRow {
                id,
                company_id: j.company_id,
                work_order_id: j.work_order_id,
                operation_id: j.operation_id,
                workstation_id: j.workstation_id,
                total_time_mins: j.total_time_mins,
                hour_rate: j.hour_rate,
                operating_cost: cost,
            }),
        )
        .await?;
        Ok(id)
    }

    /// Complete a job card: charge its conversion cost to WIP (`Dr WIP · Cr Conversion-Applied`).
    /// Gated open → completed (the once-only charge, idempotent on retry).
    pub async fn complete_job_card(
        &self,
        job_card_id: Uuid,
        gl: &dyn GlPostSink,
        sink: &dyn ManufacturingEventSink,
    ) -> Result<Decimal, ManufacturingError> {
        // RLS scope (ADR-0008), ID-only pattern — see `release_work_order`: the join is fenced by the
        // request-dedicated connection; the transaction below binds the job card's own company.
        let jc = self.job_cards.find_completion_source(&self.pool, job_card_id).await?
            .ok_or(ManufacturingError::NotFound("job card"))?;
        if jc.status == "completed" {
            return Ok(jc.operating_cost);
        }
        if jc.work_order_status != "in_process" && jc.work_order_status != "released" {
            return Err(ManufacturingError::InvalidState("work order not open for operations"));
        }
        let company_id: Uuid = jc.company_id;
        let wo_id: Uuid = jc.work_order_id;
        let cost: Decimal = jc.operating_cost;
        let wip = jc.wip_account_id.ok_or(ManufacturingError::MissingAccount("wip"))?;
        let conv = jc
            .conversion_cost_account_id
            .ok_or(ManufacturingError::MissingAccount("conversion_cost"))?;

        if cost > Decimal::ZERO {
            let env = AccountingPostEnvelope {
                idempotency_key: format!("operate:{job_card_id}"),
                company_id,
                branch_id: None,
                source_type: "manufacturing".into(),
                // The job card id is already a distinct voucher id.
                source_id: job_card_id,
                source_reference: Some(jc.work_order_number.clone()),
                posting_date: chrono::Utc::now().date_naive(),
                currency: "IDR".into(),
                posting_type: "original".into(),
                description: Some("conversion cost to WIP".into()),
                lines: vec![
                    GlPostLine::debit(wip, cost).with_description("WIP"),
                    GlPostLine::credit(conv, cost).with_description("Conversion applied"),
                ],
            };
            self.post(gl, &env).await?;
        }

        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company_id).await?;
        let moved = self.job_cards.gate_complete(&mut tx, job_card_id).await?;
        if moved != 1 {
            tx.rollback().await?;
            return Ok(cost); // already completed concurrently; post deduped
        }
        if cost > Decimal::ZERO {
            self.work_orders.add_operating_cost(&mut tx, wo_id, cost).await?;
        }
        tx.commit().await?;
        sink.publish(&ManufacturingEvent::ConversionCharged(ConversionCharged {
            job_card_id,
            work_order_id: wo_id,
            company_id,
            operating_cost: cost,
        }));
        Ok(cost)
    }

    /// Receive finished goods into stock at cost (raw + operating, prorated) and post
    /// `Dr Finished-Goods · Cr WIP`. Bounded by the ordered quantity; on full receipt WIP nets to zero
    /// and the WO completes. Gated on the produced-quantity advance (idempotent per completion).
    pub async fn receive_finished(
        &self,
        wo_id: Uuid,
        produced_qty: Decimal,
        inventory: &dyn InventoryPort,
        gl: &dyn GlPostSink,
        sink: &dyn ManufacturingEventSink,
    ) -> Result<ReceiveOutcome, ManufacturingError> {
        if produced_qty <= Decimal::ZERO {
            return Err(ManufacturingError::Invalid("produced qty must be positive".into()));
        }
        let wo = self.load_wo(wo_id).await?;
        if wo.status == "completed" {
            return Ok(ReceiveOutcome { finished_value: Decimal::ZERO, completed: true, already: true });
        }
        if wo.status != "in_process" {
            return Err(ManufacturingError::InvalidState("work order has no WIP to receive"));
        }
        let remaining = wo.quantity - wo.produced_qty;
        if produced_qty > remaining {
            return Err(ManufacturingError::OverProduce { producing: produced_qty, ordered: wo.quantity });
        }
        let fg = wo.fg_account_id.ok_or(ManufacturingError::MissingAccount("finished_goods"))?;
        let wip = wo.wip_account_id.ok_or(ManufacturingError::MissingAccount("wip"))?;
        let fg_wh = wo.fg_warehouse_id.ok_or(ManufacturingError::MissingAccount("fg_warehouse"))?;

        // Value of this receipt = prorated share of accumulated WIP (raw + operating).
        let wip_total = wo.raw_material_cost + wo.operating_cost;
        let is_full = produced_qty == remaining;
        let value = if is_full {
            // Clear all remaining WIP so it nets to zero, avoiding rounding residue.
            money(wip_total) - self.received_value(wo_id).await?
        } else {
            money(wip_total * produced_qty / wo.quantity)
        };

        // SIDE EFFECTS BEFORE THE GATE (mirrors consume/operate) — both are idempotent, keyed by the
        // cumulative produced qty, so a crash/error before the gate commits is safe to retry: the WO
        // is still `in_process`, the retry re-drives, and WIP always clears. Committing the completion
        // marker BEFORE the receipt would strand WIP non-zero if a side effect failed (council 2026-07-06).
        let cumulative = money(wo.produced_qty + produced_qty);
        let dedup = format!("receive:{wo_id}:{cumulative}");

        // 1) Drive inventory to receive the FG at this value (idempotent per `dedup`).
        inventory
            .receive_finished(&FinishedReceipt {
                company_id: wo.company_id,
                work_order_id: wo_id,
                warehouse_id: fg_wh,
                item_id: wo.item_id,
                quantity: produced_qty,
                value,
                idempotency_key: dedup.clone(),
            })
            .await
            .map_err(|r| ManufacturingError::Inventory(r.code))?;

        // 2) Post Dr Finished-Goods · Cr WIP (idempotent on the derived source_id).
        let env = AccountingPostEnvelope {
            idempotency_key: dedup.clone(),
            company_id: wo.company_id,
            branch_id: None,
            source_type: "manufacturing".into(),
            source_id: Uuid::new_v5(&wo_id, format!("manufacturing:{dedup}").as_bytes()),
            source_reference: Some(wo.work_order_number.clone()),
            posting_date: chrono::Utc::now().date_naive(),
            currency: "IDR".into(),
            posting_type: "original".into(),
            description: Some("finished goods receipt".into()),
            lines: vec![
                GlPostLine::debit(fg, value).with_description("Finished goods"),
                GlPostLine::credit(wip, value).with_description("WIP"),
            ],
        };
        self.post(gl, &env).await?;

        // 3) THE GATE, last: advance produced qty / complete. Concurrent double-receive → one wins;
        //    the loser's (idempotent) side effects were harmless dups.
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, wo.company_id).await?;
        let moved = self.work_orders.gate_receive(&mut tx, wo_id, produced_qty).await?;
        if moved != 1 {
            // Another receive won the race (its side effects deduped ours) — not an error.
            tx.rollback().await?;
            return Ok(ReceiveOutcome { finished_value: value, completed: true, already: true });
        }
        tx.commit().await?;

        let completed = wo.produced_qty + produced_qty >= wo.quantity;
        sink.publish(&ManufacturingEvent::FinishedGoodsReceived(FinishedGoodsReceived {
            work_order_id: wo_id,
            company_id: wo.company_id,
            item_id: wo.item_id,
            produced_qty,
            finished_value: value,
        }));
        if completed {
            sink.publish(&ManufacturingEvent::WorkOrderCompleted(WorkOrderCompleted {
                work_order_id: wo_id,
                company_id: wo.company_id,
                total_cost: money(wo.raw_material_cost + wo.operating_cost),
            }));
        }
        Ok(ReceiveOutcome { finished_value: value, completed, already: false })
    }

    // ---- helpers ----------------------------------------------------------------------------

    async fn post(&self, gl: &dyn GlPostSink, env: &AccountingPostEnvelope) -> Result<(), ManufacturingError> {
        if !env.is_balanced() {
            return Err(ManufacturingError::Invalid("unbalanced posting".into()));
        }
        gl.post(env).await.map_err(|r| ManufacturingError::Gl(r.code))?;
        Ok(())
    }

    async fn received_value(&self, wo_id: Uuid) -> Result<Decimal, ManufacturingError> {
        // Value already received = accumulated FG value for prior partial receipts.
        // Tracked as: (raw+operating) * produced_qty/quantity at the time of each receipt; on the final
        // receipt we clear the residue. Recompute from produced_qty for a single-receipt MVP.
        let wo = self.load_wo(wo_id).await?;
        if wo.produced_qty <= Decimal::ZERO {
            return Ok(Decimal::ZERO);
        }
        Ok(money((wo.raw_material_cost + wo.operating_cost) * wo.produced_qty / wo.quantity))
    }

    async fn load_wo(&self, wo_id: Uuid) -> Result<WorkOrderRow, ManufacturingError> {
        // RLS scope (ADR-0008), ID-only pattern: no company argument — the read rides the
        // request-dedicated connection, so RLS fences it to the caller's tenant. Callers that are
        // EVENT-driven (not on a request) must wrap the call in
        // `with_company_scope(Some(event.company_id))` or this read fails closed.
        self.work_orders.load(&self.pool, wo_id).await?
            .ok_or(ManufacturingError::NotFound("work order"))
    }
}

fn is_dup(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505"))
}
