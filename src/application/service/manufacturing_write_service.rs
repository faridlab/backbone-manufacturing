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

use rust_decimal::{Decimal, RoundingStrategy};
use sqlx::{PgPool, Row};
use uuid::Uuid;

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
}

impl ManufacturingWriteService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
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
        let mut tx = self.pool.begin().await?;
        let ins = sqlx::query(
            r#"INSERT INTO manufacturing.boms
                 (id, company_id, item_id, bom_code, quantity, uom, currency,
                  raw_material_cost, operating_cost, total_cost, is_active, is_default)
               VALUES ($1,$2,$3,$4,$5,$6,'IDR',$7,$8,$9,true,false)"#,
        )
        .bind(id).bind(b.company_id).bind(b.item_id).bind(&b.bom_code).bind(b.quantity)
        .bind(&b.uom).bind(raw).bind(operating).bind(total)
        .execute(&mut *tx).await;
        if let Err(e) = ins {
            return Err(if is_dup(&e) { ManufacturingError::DuplicateNumber(b.bom_code) } else { e.into() });
        }
        for it in &b.items {
            sqlx::query(
                r#"INSERT INTO manufacturing.bom_items (id, bom_id, item_id, quantity, rate, amount, is_phantom)
                   VALUES ($1,$2,$3,$4,$5,$6,$7)"#,
            )
            .bind(Uuid::new_v4()).bind(id).bind(it.item_id).bind(it.quantity).bind(it.rate)
            .bind(money(it.quantity * it.rate)).bind(it.is_phantom)
            .execute(&mut *tx).await?;
        }
        for op in &b.operations {
            sqlx::query(
                r#"INSERT INTO manufacturing.bom_operations
                     (id, bom_id, operation_id, workstation_id, time_in_mins, hour_rate, operating_cost)
                   VALUES ($1,$2,$3,$4,$5,$6,$7)"#,
            )
            .bind(Uuid::new_v4()).bind(id).bind(op.operation_id).bind(op.workstation_id)
            .bind(op.time_in_mins).bind(op.hour_rate).bind(money(op.time_in_mins / sixty() * op.hour_rate))
            .execute(&mut *tx).await?;
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
        let r = sqlx::query(
            r#"INSERT INTO manufacturing.work_orders
                 (id, company_id, work_order_number, item_id, bom_id, quantity, status,
                  wip_warehouse_id, fg_warehouse_id, wip_account_id, fg_account_id,
                  raw_material_account_id, conversion_cost_account_id)
               VALUES ($1,$2,$3,$4,$5,$6,'draft'::work_order_status,$7,$8,$9,$10,$11,$12)"#,
        )
        .bind(id).bind(o.company_id).bind(&o.work_order_number).bind(o.item_id).bind(o.bom_id)
        .bind(o.quantity).bind(o.wip_warehouse_id).bind(o.fg_warehouse_id).bind(o.wip_account_id)
        .bind(o.fg_account_id).bind(o.raw_material_account_id).bind(o.conversion_cost_account_id)
        .execute(&self.pool).await;
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
        let mut tx = self.pool.begin().await?;
        let wo = sqlx::query(
            r#"SELECT company_id, item_id, bom_id, quantity, status::text AS status
               FROM manufacturing.work_orders WHERE id=$1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(wo_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ManufacturingError::NotFound("work order"))?;
        let status: String = wo.get("status");
        if status != "draft" {
            return Err(ManufacturingError::InvalidState("work order is not draft"));
        }
        let company_id: Uuid = wo.get("company_id");
        let item_id: Uuid = wo.get("item_id");
        let bom_id: Uuid = wo.get("bom_id");
        let wo_qty: Decimal = wo.get("quantity");

        // Explode the BOM into required materials, recursing THROUGH phantom sub-assemblies to their
        // own components (a phantom is never stocked). Deterministic + static — no MRP.
        let mut required: Vec<(Uuid, Decimal, Decimal)> = Vec::new();
        self.explode_bom(company_id, bom_id, wo_qty, 0, &mut required).await?;

        // Gate the explosion on the draft→released transition (once-only).
        let moved = sqlx::query(
            r#"UPDATE manufacturing.work_orders SET status='released'::work_order_status
               WHERE id=$1 AND status='draft'::work_order_status"#,
        )
        .bind(wo_id)
        .execute(&mut *tx)
        .await?;
        if moved.rows_affected() != 1 {
            return Err(ManufacturingError::InvalidState("work order is not draft"));
        }

        for (item, qty, rate) in &required {
            sqlx::query(
                r#"INSERT INTO manufacturing.work_order_items
                     (id, work_order_id, item_id, required_qty, consumed_qty, rate)
                   VALUES ($1,$2,$3,$4,0,$5)"#,
            )
            .bind(Uuid::new_v4()).bind(wo_id).bind(item).bind(qty).bind(rate)
            .execute(&mut *tx)
            .await?;
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
            let base: Decimal = sqlx::query_scalar(
                r#"SELECT quantity FROM manufacturing.boms WHERE id=$1 AND (metadata->>'deleted_at') IS NULL"#,
            )
            .bind(bom_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(ManufacturingError::NotFound("bom"))?;

            let comps = sqlx::query(
                r#"SELECT item_id, quantity, rate, is_phantom FROM manufacturing.bom_items WHERE bom_id=$1"#,
            )
            .bind(bom_id)
            .fetch_all(&self.pool)
            .await?;

            for c in &comps {
                let cqty: Decimal = c.get("quantity");
                let needed = cqty * want_units / base;
                if c.get::<bool, _>("is_phantom") {
                    // Resolve the phantom item's own BOM (default first) and explode through it.
                    let phantom_item: Uuid = c.get("item_id");
                    let child_bom: Uuid = sqlx::query_scalar(
                        r#"SELECT id FROM manufacturing.boms
                           WHERE company_id=$1 AND item_id=$2 AND is_active=true
                             AND (metadata->>'deleted_at') IS NULL
                           ORDER BY is_default DESC, (metadata->>'created_at') ASC LIMIT 1"#,
                    )
                    .bind(company_id)
                    .bind(phantom_item)
                    .fetch_optional(&self.pool)
                    .await?
                    .ok_or(ManufacturingError::Invalid("phantom component has no BOM".into()))?;
                    self.explode_bom(company_id, child_bom, needed, depth + 1, out).await?;
                } else {
                    out.push((c.get("item_id"), needed, c.get("rate")));
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

        let items = sqlx::query(
            r#"SELECT id, item_id, required_qty, consumed_qty FROM manufacturing.work_order_items
               WHERE work_order_id=$1"#,
        )
        .bind(wo_id)
        .fetch_all(&self.pool)
        .await?;
        let mut lines = Vec::new();
        for it in &items {
            let req: Decimal = it.get("required_qty");
            let done: Decimal = it.get("consumed_qty");
            let remaining = req - done;
            if remaining > Decimal::ZERO {
                lines.push((it.get::<Uuid, _>("id"), it.get::<Uuid, _>("item_id"), remaining));
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
        let moved = sqlx::query(
            r#"UPDATE manufacturing.work_orders
               SET status='in_process'::work_order_status, raw_material_cost = raw_material_cost + $2
               WHERE id=$1 AND status='released'::work_order_status"#,
        )
        .bind(wo_id)
        .bind(raw_value)
        .execute(&mut *tx)
        .await?;
        if moved.rows_affected() != 1 {
            // Someone else consumed concurrently — the post deduped; don't double-book state.
            tx.rollback().await?;
            let now = self.load_wo(wo_id).await?;
            return Ok(ConsumeOutcome { raw_material_value: now.raw_material_cost, already: true });
        }
        for (line_id, _item, qty) in &lines {
            sqlx::query(
                r#"UPDATE manufacturing.work_order_items SET consumed_qty = consumed_qty + $2 WHERE id=$1"#,
            )
            .bind(line_id)
            .bind(qty)
            .execute(&mut *tx)
            .await?;
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
        sqlx::query(
            r#"INSERT INTO manufacturing.job_cards
                 (id, company_id, work_order_id, operation_id, workstation_id, total_time_mins,
                  hour_rate, operating_cost, status)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'open'::job_card_status)"#,
        )
        .bind(id).bind(j.company_id).bind(j.work_order_id).bind(j.operation_id).bind(j.workstation_id)
        .bind(j.total_time_mins).bind(j.hour_rate).bind(cost)
        .execute(&self.pool)
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
        let jc = sqlx::query(
            r#"SELECT j.company_id, j.work_order_id, j.operating_cost, j.status::text AS status,
                      w.wip_account_id, w.conversion_cost_account_id, w.work_order_number, w.status::text AS wo_status
               FROM manufacturing.job_cards j
               JOIN manufacturing.work_orders w ON w.id = j.work_order_id
               WHERE j.id=$1 AND (j.metadata->>'deleted_at') IS NULL"#,
        )
        .bind(job_card_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(ManufacturingError::NotFound("job card"))?;
        let status: String = jc.get("status");
        if status == "completed" {
            return Ok(jc.get("operating_cost"));
        }
        let wo_status: String = jc.get("wo_status");
        if wo_status != "in_process" && wo_status != "released" {
            return Err(ManufacturingError::InvalidState("work order not open for operations"));
        }
        let company_id: Uuid = jc.get("company_id");
        let wo_id: Uuid = jc.get("work_order_id");
        let cost: Decimal = jc.get("operating_cost");
        let wip = jc.get::<Option<Uuid>, _>("wip_account_id").ok_or(ManufacturingError::MissingAccount("wip"))?;
        let conv = jc
            .get::<Option<Uuid>, _>("conversion_cost_account_id")
            .ok_or(ManufacturingError::MissingAccount("conversion_cost"))?;

        if cost > Decimal::ZERO {
            let env = AccountingPostEnvelope {
                idempotency_key: format!("operate:{job_card_id}"),
                company_id,
                branch_id: None,
                source_type: "manufacturing".into(),
                // The job card id is already a distinct voucher id.
                source_id: job_card_id,
                source_reference: Some(jc.get::<String, _>("work_order_number")),
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
        let moved = sqlx::query(
            r#"UPDATE manufacturing.job_cards SET status='completed'::job_card_status
               WHERE id=$1 AND status='open'::job_card_status"#,
        )
        .bind(job_card_id)
        .execute(&mut *tx)
        .await?;
        if moved.rows_affected() != 1 {
            tx.rollback().await?;
            return Ok(cost); // already completed concurrently; post deduped
        }
        if cost > Decimal::ZERO {
            sqlx::query(
                r#"UPDATE manufacturing.work_orders SET operating_cost = operating_cost + $2 WHERE id=$1"#,
            )
            .bind(wo_id)
            .bind(cost)
            .execute(&mut *tx)
            .await?;
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
        let moved = sqlx::query(
            r#"UPDATE manufacturing.work_orders
               SET produced_qty = produced_qty + $2,
                   status = CASE WHEN produced_qty + $2 >= quantity THEN 'completed'::work_order_status
                                 ELSE status END
               WHERE id=$1 AND status='in_process'::work_order_status AND produced_qty + $2 <= quantity"#,
        )
        .bind(wo_id)
        .bind(produced_qty)
        .execute(&mut *tx)
        .await?;
        if moved.rows_affected() != 1 {
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

    async fn load_wo(&self, wo_id: Uuid) -> Result<WoRow, ManufacturingError> {
        let r = sqlx::query(
            r#"SELECT company_id, work_order_number, item_id, quantity, produced_qty,
                      status::text AS status, raw_material_cost, operating_cost,
                      wip_warehouse_id, fg_warehouse_id, wip_account_id, fg_account_id,
                      raw_material_account_id, conversion_cost_account_id
               FROM manufacturing.work_orders WHERE id=$1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(wo_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(ManufacturingError::NotFound("work order"))?;
        Ok(WoRow {
            company_id: r.get("company_id"),
            work_order_number: r.get("work_order_number"),
            item_id: r.get("item_id"),
            quantity: r.get("quantity"),
            produced_qty: r.get("produced_qty"),
            status: r.get("status"),
            raw_material_cost: r.get("raw_material_cost"),
            operating_cost: r.get("operating_cost"),
            wip_warehouse_id: r.get("wip_warehouse_id"),
            fg_warehouse_id: r.get("fg_warehouse_id"),
            wip_account_id: r.get("wip_account_id"),
            fg_account_id: r.get("fg_account_id"),
            raw_material_account_id: r.get("raw_material_account_id"),
            conversion_cost_account_id: r.get("conversion_cost_account_id"),
        })
    }
}

struct WoRow {
    company_id: Uuid,
    work_order_number: String,
    item_id: Uuid,
    quantity: Decimal,
    produced_qty: Decimal,
    status: String,
    raw_material_cost: Decimal,
    operating_cost: Decimal,
    wip_warehouse_id: Option<Uuid>,
    fg_warehouse_id: Option<Uuid>,
    wip_account_id: Option<Uuid>,
    fg_account_id: Option<Uuid>,
    raw_material_account_id: Option<Uuid>,
    conversion_cost_account_id: Option<Uuid>,
}

fn is_dup(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505"))
}
