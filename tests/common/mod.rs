//! Shared test helpers: a live pool, a real-accounting GL adapter, ledger seeding/balances, and a
//! faithful in-test `InventoryPort` (valued stock with insufficient-stock rejection + conservation).
//! Every test uses fresh random ids so rows never collide across parallel tests.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use backbone_accounting::application::service::posting_service::{
    PostingLine, PostingRequest, PostingService,
};
use backbone_manufacturing::application::service::manufacturing_gl::{
    AccountingPostEnvelope, GlPostAck, GlPostRejected, GlPostSink,
};
use backbone_manufacturing::application::service::manufacturing_ports::{
    FinishedReceipt, InventoryPort, InventoryRejected, IssueAck, IssuedLineValue, MaterialIssue,
};
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

/// Seed a detail account and return its id. `atype`/`normal` are the accounting enum values.
pub async fn account(
    pool: &PgPool,
    company: Uuid,
    code: &str,
    atype: &str,
    subtype: &str,
    normal: &str,
) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
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
    .await
    .expect("seed account");
    id
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
        Self { svc: PostingService::new(pool) }
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
/// exercise the crash-between-side-effect-and-gate window.
#[derive(Clone, Default)]
pub struct FakeInventory {
    /// item_id → (on_hand_qty, valuation_rate)
    pub raw: Arc<Mutex<HashMap<Uuid, (Decimal, Decimal)>>>,
    /// item_id → received finished (qty, total_value)
    pub finished: Arc<Mutex<HashMap<Uuid, (Decimal, Decimal)>>>,
    /// idempotency_key → prior IssueAck (dedup)
    issued: Arc<Mutex<HashMap<String, IssueAck>>>,
    /// idempotency_keys of receipts already applied
    received: Arc<Mutex<std::collections::HashSet<String>>>,
    /// when > 0, the next N `receive_finished` calls fail transiently (then succeed)
    fail_receive: Arc<Mutex<u32>>,
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
        Ok(())
    }
}
