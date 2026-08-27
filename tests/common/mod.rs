//! Shared test helpers: a live pool, a real-accounting GL adapter, ledger seeding/balances, and a
//! faithful in-test `InventoryPort` (valued stock with insufficient-stock rejection + conservation).
//! Every test uses fresh random ids so rows never collide across parallel tests.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use backbone_accounting::application::service::posting_service::{
    PostingLine, PostingRequest, PostingService,
};
use backbone_accounting::infrastructure::persistence::SqlxPostingRepository;
use backbone_manufacturing::application::service::manufacturing_gl::{
    AccountingPostEnvelope, GlPostAck, GlPostRejected, GlPostSink,
};
use backbone_manufacturing::application::service::manufacturing_ports::{
    CostPosture, FinishedReceipt, InventoryPort, InventoryRejected, IssueAck, IssuedLineValue,
    MaterialIssue, RepairAvailability, RepairLeg, UnbuildReversal,
};
use backbone_manufacturing::application::service::manufacturing_write_service::ReceiveFinishedOrder;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

pub fn dburl() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5433/backbone_manufacturing".into())
}
pub async fn pool() -> PgPool {
    PgPool::connect(&dburl()).await.expect("connect")
}
pub fn dec(s: &str) -> Decimal {
    s.parse().unwrap()
}
pub fn today() -> chrono::NaiveDate {
    chrono::Utc::now().date_naive()
}
/// A minimal average-posture receipt request (no byproducts, no extra cost).
pub fn rcv(q: Decimal) -> ReceiveFinishedOrder {
    ReceiveFinishedOrder {
        produced_qty: q,
        byproducts: vec![],
        extra_cost: None,
        cost_posture: CostPosture::default(),
        standard_unit_price: None,
    }
}

/// Seed a detail account and return its id. `atype`/`normal` are the accounting enum values.
/// Idempotent per (company, account number): a code already seeded for the company returns the
/// existing account's id instead of colliding on the unique index.
pub async fn account(
    pool: &PgPool,
    company: Uuid,
    code: &str,
    atype: &str,
    subtype: &str,
    normal: &str,
) -> Uuid {
    let id = Uuid::new_v4();
    let inserted = sqlx::query(
        r#"INSERT INTO accounting.accounts
             (id, company_id, account_number, account_code, name, account_type, account_subtype,
              normal_balance, is_header, is_detail, status)
           VALUES ($1,$2,$3,$4,$5,$6::account_type,$7::account_subtype,$8::normal_balance,
                   false,true,'active'::account_status)"#,
    )
    .bind(id)
    .bind(company)
    .bind(code)
    .bind(code)
    .bind(code)
    .bind(atype)
    .bind(subtype)
    .bind(normal)
    .execute(pool)
    .await;
    match inserted {
        Ok(_) => id,
        Err(e) if matches!(&e, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505")) => {
            sqlx::query_scalar(
                "SELECT id FROM accounting.accounts WHERE company_id=$1 AND account_number=$2",
            )
            .bind(company)
            .bind(code)
            .fetch_one(pool)
            .await
            .expect("load already-seeded account")
        }
        Err(e) => panic!("seed account: {e}"),
    }
}

/// Ledger balance (debit − credit) for an account.
pub async fn balance(pool: &PgPool, account: Uuid) -> Decimal {
    sqlx::query_scalar(
        "SELECT COALESCE(SUM(debit_amount),0) - COALESCE(SUM(credit_amount),0)
         FROM accounting.ledgers WHERE account_id=$1",
    )
    .bind(account)
    .fetch_one(pool)
    .await
    .expect("balance")
}

/// The four GL accounts a work order needs.
pub struct WoAccounts {
    pub wip: Uuid,
    pub fg: Uuid,
    pub raw: Uuid,
    pub conversion: Uuid,
}
pub async fn wo_accounts(pool: &PgPool, company: Uuid) -> WoAccounts {
    WoAccounts {
        wip: account(pool, company, "1410-WIP", "asset", "inventory", "debit").await,
        fg: account(pool, company, "1420-FG", "asset", "inventory", "debit").await,
        raw: account(pool, company, "1400-RAW", "asset", "inventory", "debit").await,
        conversion: account(pool, company, "5100-CONV", "expense", "operating_expense", "credit").await,
    }
}

/// ACL: manufacturing's serialized envelope → accounting's PostingRequest against the REAL ledger.
pub struct GlAdapter {
    pub svc: PostingService,
}
impl GlAdapter {
    pub fn new(pool: PgPool) -> Self {
        Self { svc: PostingService::new(Arc::new(SqlxPostingRepository::new(pool))) }
    }
}
#[async_trait::async_trait]
impl GlPostSink for GlAdapter {
    async fn post(&self, e: &AccountingPostEnvelope) -> Result<GlPostAck, GlPostRejected> {
        let mut r = PostingRequest::original(e.company_id, &e.source_type, e.source_id, e.posting_date);
        r.source_reference = e.source_reference.clone();
        r.posting_type = e.posting_type.clone();
        r.lines = e
            .lines
            .iter()
            .map(|l| PostingLine {
                account_id: l.account_id,
                debit: l.debit,
                credit: l.credit,
                party_type: l.party_type.clone(),
                party_id: l.party_id,
                cost_center_id: None,
                project_id: None,
                department_id: None,
                description: l.description.clone(),
            })
            .collect();
        match self.svc.post(r, None).await {
            Ok(x) => Ok(GlPostAck { post_id: x.post_id, journal_id: x.journal_id, idempotent_reuse: x.idempotent_reuse }),
            Err(x) => Err(GlPostRejected { code: x.code().to_string(), message: x.to_string() }),
        }
    }
}

/// A counting GL sink — records each post's idempotency_key so tests can assert how many posts of a
/// given KIND (consume/operate/receive, keyed by the idempotency_key prefix) reached the ledger.
#[derive(Clone, Default)]
pub struct CountingGl {
    pub keys: Arc<Mutex<Vec<String>>>, // idempotency_key of each post
}
impl CountingGl {
    pub fn new() -> Self {
        Self::default()
    }
    /// How many posts whose idempotency_key starts with `kind` (e.g. "consume", "operate", "receive").
    pub fn count(&self, kind: &str) -> usize {
        self.keys.lock().unwrap().iter().filter(|k| k.starts_with(kind)).count()
    }
}
#[async_trait::async_trait]
impl GlPostSink for CountingGl {
    async fn post(&self, e: &AccountingPostEnvelope) -> Result<GlPostAck, GlPostRejected> {
        self.keys.lock().unwrap().push(e.idempotency_key.clone());
        Ok(GlPostAck { post_id: Uuid::new_v4(), journal_id: Uuid::new_v4(), idempotent_reuse: false })
    }
}

/// A faithful in-test inventory: valued stock, insufficient-stock rejection, **idempotent** issue/receive
/// (a repeated `idempotency_key` never moves stock twice), plus an optional one-shot receive failure to
/// exercise the crash-between-side-effect-and-gate window. Also implements the reversal / repair-leg /
/// availability surface, and records byproduct + extra-cost legs for assertions.
#[derive(Clone, Default)]
pub struct FakeInventory {
    /// item_id → (on_hand_qty, valuation_rate)
    pub raw: Arc<Mutex<HashMap<Uuid, (Decimal, Decimal)>>>,
    /// item_id → received finished (qty, total_value) — FG AND byproducts land here
    pub finished: Arc<Mutex<HashMap<Uuid, (Decimal, Decimal)>>>,
    /// idempotency_key → prior IssueAck (dedup)
    issued: Arc<Mutex<HashMap<String, IssueAck>>>,
    /// idempotency_keys of receipts/reversals/legs already applied
    received: Arc<Mutex<std::collections::HashSet<String>>>,
    /// when > 0, the next N `receive_finished` calls fail transiently (then succeed)
    fail_receive: Arc<Mutex<u32>>,
    /// every extra-cost leg carried by receipts, in order
    pub extra_costs: Arc<Mutex<Vec<Decimal>>>,
    /// executed repair legs (line_type, item_id, quantity, rate), in order
    pub repair_legs: Arc<Mutex<Vec<(String, Uuid, Decimal, Decimal)>>>,
}
impl FakeInventory {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn stock(&self, item: Uuid, qty: &str, rate: &str) {
        self.raw.lock().unwrap().insert(item, (dec(qty), dec(rate)));
    }
    /// Make the next `n` receive_finished calls fail (simulating a transient inventory error).
    pub fn fail_next_receives(&self, n: u32) {
        *self.fail_receive.lock().unwrap() = n;
    }
    pub fn on_hand(&self, item: Uuid) -> Decimal {
        self.raw.lock().unwrap().get(&item).map(|(q, _)| *q).unwrap_or(Decimal::ZERO)
    }
    pub fn finished_qty(&self, item: Uuid) -> Decimal {
        self.finished.lock().unwrap().get(&item).map(|(q, _)| *q).unwrap_or(Decimal::ZERO)
    }
    pub fn finished_value(&self, item: Uuid) -> Decimal {
        self.finished.lock().unwrap().get(&item).map(|(_, v)| *v).unwrap_or(Decimal::ZERO)
    }
}
#[async_trait::async_trait]
impl InventoryPort for FakeInventory {
    async fn issue_to_wip(&self, req: &MaterialIssue) -> Result<IssueAck, InventoryRejected> {
        // Idempotent: a repeated key returns the prior result WITHOUT moving stock again.
        if let Some(prior) = self.issued.lock().unwrap().get(&req.idempotency_key) {
            return Ok(prior.clone());
        }
        let mut raw = self.raw.lock().unwrap();
        for l in &req.lines {
            let (qty, _) = raw.get(&l.item_id).copied().unwrap_or((Decimal::ZERO, Decimal::ZERO));
            if l.quantity > qty {
                return Err(InventoryRejected { code: "insufficient_stock".into(), message: format!("{}", l.item_id) });
            }
        }
        let mut lines = Vec::new();
        let mut total = Decimal::ZERO;
        for l in &req.lines {
            let (qty, rate) = *raw.get(&l.item_id).unwrap();
            raw.insert(l.item_id, (qty - l.quantity, rate));
            let value = (l.quantity * rate).round_dp(2);
            total += value;
            lines.push(IssuedLineValue { item_id: l.item_id, quantity: l.quantity, rate, value });
        }
        let ack = IssueAck { total_value: total, lines };
        self.issued.lock().unwrap().insert(req.idempotency_key.clone(), ack.clone());
        Ok(ack)
    }
    async fn receive_finished(&self, req: &FinishedReceipt) -> Result<(), InventoryRejected> {
        {
            let mut f = self.fail_receive.lock().unwrap();
            if *f > 0 {
                *f -= 1;
                return Err(InventoryRejected { code: "transient".into(), message: "simulated".into() });
            }
        }
        // Idempotent: a repeated key is a no-op.
        if !self.received.lock().unwrap().insert(req.idempotency_key.clone()) {
            return Ok(());
        }
        let mut fin = self.finished.lock().unwrap();
        let e = fin.entry(req.item_id).or_insert((Decimal::ZERO, Decimal::ZERO));
        e.0 += req.quantity;
        e.1 += req.value;
        // Byproducts land in the finished estate too, each at its split value.
        for b in &req.byproducts {
            let be = fin.entry(b.item_id).or_insert((Decimal::ZERO, Decimal::ZERO));
            be.0 += b.quantity;
            be.1 += b.value;
        }
        if let Some(extra) = req.extra_cost {
            if extra > Decimal::ZERO {
                self.extra_costs.lock().unwrap().push(extra);
            }
        }
        Ok(())
    }
    async fn reverse_production(&self, req: &UnbuildReversal) -> Result<(), InventoryRejected> {
        if !self.received.lock().unwrap().insert(req.idempotency_key.clone()) {
            return Ok(());
        }
        let mut fin = self.finished.lock().unwrap();
        let (qty, value) = fin.get(&req.item_id).copied().unwrap_or((Decimal::ZERO, Decimal::ZERO));
        if req.quantity > qty {
            return Err(InventoryRejected {
                code: "insufficient_stock".into(),
                message: format!("finished stock of {} is {}", req.item_id, qty),
            });
        }
        // Reverse off at the estate's average rate; the recovered value rides the components.
        let _rate = if qty > Decimal::ZERO { value / qty } else { Decimal::ZERO };
        fin.insert(req.item_id, (qty - req.quantity, value - req.value));
        drop(fin);
        // Components return to raw stock: quantity back, value spread by the request's shares.
        let mut raw = self.raw.lock().unwrap();
        let total_comp_qty: Decimal = req.components.iter().map(|c| c.quantity).sum();
        for c in &req.components {
            let (q, r) = raw.get(&c.item_id).copied().unwrap_or((Decimal::ZERO, Decimal::ZERO));
            let share = if total_comp_qty > Decimal::ZERO {
                req.value * c.quantity / total_comp_qty
            } else {
                Decimal::ZERO
            };
            let new_rate = if q + c.quantity > Decimal::ZERO {
                (r * q + share) / (q + c.quantity)
            } else {
                r
            };
            raw.insert(c.item_id, (q + c.quantity, new_rate));
        }
        Ok(())
    }
    async fn execute_repair_leg(&self, req: &RepairLeg) -> Result<(), InventoryRejected> {
        // Idempotent per key — but a key is claimed ONLY when the leg actually moves stock: a
        // failed attempt must not consume the key, or the ruled retry would silently no-op.
        if self.received.lock().unwrap().contains(&req.idempotency_key) {
            return Ok(());
        }
        let mut raw = self.raw.lock().unwrap();
        match req.line_type.as_str() {
            "add" | "remove" => {
                let (qty, rate) = raw.get(&req.item_id).copied().unwrap_or((Decimal::ZERO, Decimal::ZERO));
                if req.quantity > qty {
                    return Err(InventoryRejected {
                        code: "insufficient_stock".into(),
                        message: format!("stock of {} is {}", req.item_id, qty),
                    });
                }
                raw.insert(req.item_id, (qty - req.quantity, rate));
            }
            "recycle" => {
                let (qty, rate) = raw.get(&req.item_id).copied().unwrap_or((Decimal::ZERO, Decimal::ZERO));
                raw.insert(req.item_id, (qty + req.quantity, rate));
            }
            other => {
                return Err(InventoryRejected { code: "bad_line_type".into(), message: other.into() });
            }
        }
        drop(raw);
        self.received.lock().unwrap().insert(req.idempotency_key.clone());
        self.repair_legs
            .lock()
            .unwrap()
            .push((req.line_type.clone(), req.item_id, req.quantity, req.rate));
        Ok(())
    }
    async fn check_repair_availability(&self, req: &RepairAvailability) -> Result<(), InventoryRejected> {
        let qty = self
            .raw
            .lock()
            .unwrap()
            .get(&req.item_id)
            .map(|(q, _)| *q)
            .unwrap_or(Decimal::ZERO);
        if req.quantity > qty {
            return Err(InventoryRejected {
                code: "insufficient_stock".into(),
                message: format!("{} short of {}", qty, req.quantity),
            });
        }
        Ok(())
    }
}

/// Seed a category's costing-defaults row (the account-resolution chain's middle hop).
#[allow(clippy::too_many_arguments)]
pub async fn seed_costing_defaults(
    pool: &PgPool,
    company: Uuid,
    category: Uuid,
    wip: Option<Uuid>,
    fg: Option<Uuid>,
    raw: Option<Uuid>,
    conversion: Option<Uuid>,
    interim: Option<Uuid>,
    variance: Option<Uuid>,
    inventory_loss: Option<Uuid>,
    repair_expense: Option<Uuid>,
) {
    use backbone_manufacturing::infrastructure::persistence::{CostingDefaultsRepository, NewCostingDefaultsRow};
    CostingDefaultsRepository::new(pool.clone())
        .insert_defaults(
            pool,
            &NewCostingDefaultsRow {
                id: Uuid::new_v4(),
                company_id: company,
                product_category_id: category,
                wip_account_id: wip,
                fg_account_id: fg,
                raw_material_account_id: raw,
                conversion_cost_account_id: conversion,
                subcontract_interim_account_id: interim,
                cost_variance_account_id: variance,
                inventory_loss_account_id: inventory_loss,
                repair_expense_account_id: repair_expense,
            },
        )
        .await
        .expect("seed costing defaults");
}
