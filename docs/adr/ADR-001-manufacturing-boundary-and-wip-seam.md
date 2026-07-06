# ADR-001 — Manufacturing's boundary and the WIP job-order costing seam

Status: accepted · 2026-07-06 · Tier 3 (Manufacturing pillar; a GL producer)

## Context

An SMB that makes things needs finished-goods cost in the books. ERPNext bakes the WIP/FG postings
into Stock Entry `on_submit`. We want manufacturing to be a **separate GL producer** that emits through
the same `AccountingPost` contract the Financials pillar defined (brief §5) — owning no ledger, no stock.

## Decision

1. **Manufacturing owns the WIP/FG posts; inventory owns the stock; accounting owns the ledger.**
   A Work Order's value flows through WIP in **three balanced posts** — consume (`Dr WIP · Cr Raw`),
   operate (`Dr WIP · Cr Conversion`), receive (`Dr FG · Cr WIP`) — so **WIP nets to zero** on completion
   and finished goods carry raw + operating cost. This is textbook job-order costing, and "WIP nets to
   zero" is the seam's one provable invariant (analogous to POS's "A/R nets to zero").

2. **Material valuation comes from inventory, not the BOM.** `consume_materials` drives the
   `InventoryPort` to issue components and returns their moving-average value; the BOM's component rates
   are for the *planning* roll-up only. So FG cost is inventory's real number.

3. **Emit through `GlPostSink`; zero normal Cargo edge.** Manufacturing serializes an
   `AccountingPostEnvelope` (`source_type='manufacturing'`); a composing service / the seam test maps it
   to accounting's `PostingService`. Registered manufacturing in accounting's `PostingSourceType` enum
   (the one cross-module change a new producer needs).

4. **Each post is a distinct voucher.** `posting_type='original'` for all three; distinctness comes from
   `source_id` (consume/receive derived from the WO, operate = the job card), so accounting's dedup
   `(company, source_type, source_id, posting_type)` is the retry backstop behind the transition gates.

5. **Bounded + idempotent, side-effects-before-gate.** Receipt is bounded by the ordered quantity (no
   over-produce). Every verb performs its **idempotent side effects first** (inventory issue/receive —
   keyed by an `idempotency_key` so a repeat never moves stock twice — then the GL post, deduped on the
   derived `source_id`) and only **then** commits its status-transition gate (consume: released→in_process,
   operate: open→completed, receive: →completed). So a crash/error mid-saga leaves the WO in its prior
   state and the retry re-drives to completion — WIP never strands non-zero (maturity council 2026-07-06:
   `receive_finished` originally committed the completion gate *before* the side effects, stranding WIP on
   a failed receipt — proven by revert, IP-6).

## Consequences

- Turn manufacturing off and the GL still balances — it only *adds* WIP/FG value, reversibly.
- Proven end-to-end (`tests/plant_to_produce_seam.rs`) and survives a full regen (§5).
- **Phantom sub-assemblies** (KEEP, completeness council 2026-07-06): a `BomItem.is_phantom` line is exploded THROUGH to its own BOM's components at release (static, no MRP) — a phantom is never stocked or issued (MGC-4, proven-by-revert).

## Parking lot (each with a gate)
- **Real inventory Manufacture Stock Entry** — today the `InventoryPort` is driven by an in-test adapter
  (inventory has no consume-components-and-produce-a-different-item primitive yet). Gate: inventory
  ships a Manufacture Stock Entry; then wire the port to it.
- **FG standard vs actual cost** — MVP uses actual (raw consumed + job-card operating). Gate: a merchant
  wants standard costing + variance.
- **Scrap / short-close** — "WIP nets to zero" holds on a *full* receipt; a WO that yields less than
  ordered (real shop-floor loss) cannot reach `completed` and its residual WIP strands. Gate: a
  `close_short` verb that writes the shortfall to a scrap/variance account. Flagged by the completeness
  council (2026-07-06) as the day-one caveat on the headline invariant.
- **Partial-receipt proration nuance**, **Production Plan / MRP**, **routing master**, **subcontracting**,
  **capacity scheduling** — deferred (PRD non-goals).
