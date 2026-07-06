# BRD — backbone-manufacturing

> Business Requirements & Rules. Tier 3 · Manufacturing pillar (GL producer). Date: 2026-07-06.
> Pairs with `docs/business-flows/golden-cases.md`.

## Documents
Workstation (hourly conversion rate) · Operation (a production step) · Bom (+ BomItem + BomOperation)
(the recipe + cost roll-up) · WorkOrder (+ WorkOrderItem) (the build; emits WIP/FG posts) · JobCard
(shop-floor time → conversion cost).

## Business rules

**BR-1 (BOM cost roll-up).** `raw_material_cost = Σ (component.qty × rate)`; `operating_cost =
Σ (op.time_in_mins / 60 × hour_rate)`; `total_cost = raw + operating`. Money IDR, 2dp, half-away-from-zero.
A BOM needs ≥ 1 component (→ `invalid`).

**BR-2 (release — explode, through phantoms).** `release_work_order` explodes the BOM into WorkOrderItems:
`required_qty = component.qty × WO.qty / BOM.qty`, recursing THROUGH any `is_phantom` component to its own
BOM's components (a phantom is never stocked / issued) — static multi-level, no MRP. Gated `draft →
released` (once-only).

**BR-3 (consume — the WIP charge).** `consume_materials` issues the required materials to WIP via the
`InventoryPort` (value = inventory's moving-average, not a made-up number) and posts **`Dr WIP · Cr
Raw-Material Stock`** for that value. Gated `released → in_process`; idempotent (a retry short-circuits,
WIP charged once). Requires the WIP + raw accounts (→ `missing_account`, before any post or stock move).

**BR-4 (operate — conversion cost).** `complete_job_card` charges a job card's
`operating_cost = time/60 × hour_rate` to WIP: **`Dr WIP · Cr Conversion-Applied`**. Gated `open →
completed`; idempotent (charged once). Requires the WIP + conversion accounts.

**BR-5 (receive — FG, WIP clears).** `receive_finished` values the receipt as the prorated share of
accumulated WIP (`raw + operating`), drives the `InventoryPort` to receive the FG at that value, and
posts **`Dr Finished-Goods · Cr WIP`**. Bounded by the ordered quantity (→ `over_produce`); on full
receipt **WIP nets to zero** and the WO completes. Gated on the produced-qty advance (idempotent).

**BR-6 (the three posts / distinct vouchers).** All posts are `posting_type='original'`,
`source_type='manufacturing'`, each with a **distinct** `source_id` (consume/receive derived from the
WO, operate = the job card) so accounting's dedup `(company, source_type, source_id, posting_type)` is
the retry backstop behind the transition gates. Manufacturing is registered in accounting's
`PostingSourceType` enum.

## Events
`WorkOrderReleased`, `MaterialsConsumed`, `ConversionCharged`, `FinishedGoodsReceived`,
`WorkOrderCompleted`.

## Deferred (with reason)
Production Plan / MPS / MRP, capacity scheduling, subcontracting, BOM mass-rebuild, reusable Routing
master, multi-level explosion automation. See PRD non-goals.
