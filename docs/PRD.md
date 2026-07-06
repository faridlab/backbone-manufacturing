# PRD — backbone-manufacturing

> Product Requirements. Tier 3 · Manufacturing pillar (a GL producer). Date: 2026-07-06.

## Why this module exists

An Indonesia SMB that *makes* things needs to know what a finished good costs and to have that cost
land in the books. `backbone-manufacturing` owns the **product definition** (what to build — BOM,
operations) and the **execution** (the act of building — Work Order, Job Card), and it emits the
**WIP/FG valuation postings** that turn raw materials + labour into finished-goods inventory value.

It **owns no stock and no ledger.** It drives `backbone-inventory` for the physical material moves +
valuation and emits balanced postings through the same `AccountingPost` seam the Financials pillar
defined — so turning manufacturing off leaves the GL still balanced. It is the **6th GL producer**.

## Scope (the SMB minimum — brief §4)

- **Static BOM** (single & multi-level via components + operations) with a cost roll-up.
- **Manual Work Order** against a BOM: release (explode required materials) → consume → operate →
  receive finished goods.
- **Job Card** shop-floor time → conversion cost charged to WIP.
- **WIP job-order costing**: three balanced posts (consume `Dr WIP·Cr Raw`, operate `Dr WIP·Cr
  Conversion`, receive `Dr FG·Cr WIP`) so **WIP nets to zero** on completion and finished goods carry
  raw + operating cost. Region-neutral IDR.

## Non-goals / deferred (brief §4, with reason)

- **Production Plan / MPS / MRP** and **finite/capacity scheduling** — manual Work Orders first.
- **Subcontracting depth** — a flag, not a flow.
- **BOM mass-rebuild tooling**, **multi-level explosion automation**, **routing as a reusable master**
  (operations are inline on the BOM for the MVP).
- **Partial/over- reporting nuances** beyond the bounded receipt.

## Success criteria

- A Work Order runs consume → operate → receive and the **GL WIP account nets to exactly zero**
  (proven: `tests/plant_to_produce_seam.rs` against the real ledger).
- No Work Order over-produces; each post is emitted at most once under retry (integrity probes).
- Zero normal Cargo edge to accounting/inventory — the envelope + ports are the only contract.
