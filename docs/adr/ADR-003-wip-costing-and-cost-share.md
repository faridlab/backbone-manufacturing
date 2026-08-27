# ADR-003: WIP job-order costing, the account chain, and the byproduct cost-share split

Status: accepted · Date: 2026-08-27 · Extends ADR-001 §2

## Context

ADR-001 fixed the seam (manufacturing owns no ledger, posts balanced envelopes) and the invariant
(WIP nets to zero). Left open: where accounts come from when the order does not carry them, how a
receipt valued under a *standard* price reconciles to actual, and how co-products share a batch's
cost.

## Decisions

**One resolution chain, three hops, no fallback.** Every costing verb resolves accounts as:
per-order override → `category_costing_defaults` row for the order's `product_category_id` snapshot
→ `MissingAccount`, LOUD, before any post or stock move. A seeded-but-NULL column is not a
resolution. No hardcoded account exists anywhere in the module.

**Job-order costing, unchanged in shape:** consume `Dr WIP · Cr Raw`, operate `Dr WIP · Cr
Conversion`, receive `Dr FG · Cr WIP`; on a full receipt the FG value is the WIP remainder (not a
recomputed prorate) so WIP nets to exactly zero.

**Byproduct cost-share split.** A receipt may carry byproduct legs; each leg's value is
`batch_total × cost_share / 100` from its BoM byproduct row, and the FG line keeps the remainder.
A byproduct with no BoM row is refused (it would have no share); a family whose shares exceed 100
is `CostShareOverflow`. The total is never re-derived — Σ shares ≤ 100 and the FG remainder make the
post balanced by construction.

**Cost postures.** `Average` (default) values FG from accumulated WIP + any subcontract extra cost
(`Dr FG · Cr WIP · Cr Subcontract-Interim`). `Standard` values FG at `qty × standard_unit_price`
— the pinned price is never recomputed — and posts the gap to the cost-variance account as a plug:
favourable gap (standard > actual) credits variance, unfavourable debits it. A nonzero gap with no
variance account resolvable is `MissingAccount`, LOUD.

## Consequences

- Composers can run with zero per-order account bookkeeping (one defaults row per category) while
  keeping every refusal loud.
- The subcontract DoD numbers fall out: components 6,000 + purchase extra 2,500 →
  `Dr FG 8,500 · Cr WIP 6,000 · Cr Interim 2,500`.
- Co-product accounting is deterministic from the recipe (share) + the batch total (actuals) — no
  allocation run, no rounding-order dependence in the split itself.

Anchors: `manufacturing_write_service.rs` (`resolve_account`), `manufacturing_execution.rs`,
`tests/costing_defaults_probes.rs`, `tests/byproduct_cost_share.rs`, `tests/wip_netting.rs`.
