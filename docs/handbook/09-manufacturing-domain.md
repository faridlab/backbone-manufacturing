<!-- Reader: Maintainer + App developer · Mode: Explanation → How-to -->
# The manufacturing domain

This is the page that explains *why the module exists*. The generated CRUD (previous pages) is
table-stakes; the domain lives in the hand-authored service files under
[`application/service/`](../../src/application/service/) and the schema models. Read this to
understand what a BOM and a Work Order mean here, how cost rolls up, how the Work Order lifecycle
drives inventory and the ledger, and the one invariant everything protects: **WIP nets to zero.**

Authoritative sources this page narrates (don't duplicate — link out):
[BRD](../BRD.md) (rules BR-1…BR-6), [PRD](../PRD.md) (scope/non-goals),
[FSD](../FSD.md) (entities/state machines/seams),
[ADR-001](../adr/ADR-001-manufacturing-boundary-and-wip-seam.md) (the boundary + WIP seam),
[ADR-002](../adr/ADR-002-hybrid-state-convergence.md) (the state vocabulary),
[ADR-003](../adr/ADR-003-wip-costing-and-cost-share.md) (job-order costing + byproduct split),
[ADR-004](../adr/ADR-004-unbuild-repair-subcontract.md) (the reversal / repair / subcontract surfaces),
[ADR-005](../adr/ADR-005-shared-master-data-and-guards.md) (the shared_blank fence + DB guards),
[business flows](../business-flows/README.md) + [golden cases](../business-flows/golden-cases.md)
(the numeric oracle).

## The one split: master data vs execution

The domain divides cleanly in two, mirrored by the schema models.

| | **Product definition** (master data) | **Execution** (transactional) |
|---|---|---|
| Schema | [`bom.model.yaml`](../../schema/models/bom.model.yaml) | [`work_order.model.yaml`](../../schema/models/work_order.model.yaml) |
| Entities | `Workstation`, `Operation`, `Bom`, `BomItem`, `BomOperation`, `BomByproduct`, `BomSubcontractor`, `WorkstationLoss` | `WorkOrder`, `WorkOrderItem`, `JobCard`, `UnbuildOrder`, `RepairOrder`/`RepairPart`/`RepairTag`, `WorkstationProductivity`, `SubcontractMoLink` |
| Answers | **WHAT** to build (and in what *way*: normal / kit / subcontract) | The **ACT** of building — and of un-building and repairing |
| GL | Posts **no** ledger — only *defines* cost | Emits the WIP/FG postings and the reversal/repair posts |

A `Bom` is the recipe for one manufactured item: its component materials (`BomItem`), the
operations that convert them (`BomOperation`, each run on a `Workstation` at an hourly rate), its
optional byproduct legs (`BomByproduct` — co-products that come off the line with the main item),
and — for subcontract recipes — its supplier legs (`BomSubcontractor`). A `WorkOrder` produces
`quantity` of that item against a `Bom`, and *its* lifecycle is what turns raw materials + labour
into finished-goods value in the books. Manufacturing **owns no stock and no ledger** — it drives
inventory for the physical moves and emits balanced postings through a seam (ADR-001).

## Master data: staged, typed, versioned

`ManufacturingWriteService::create_bom` computes the cost roll-up (BR-1):

- `raw_material_cost` = Σ over components of `money(quantity × rate)`
- `operating_cost` = Σ over operations of `money(time_in_mins / 60 × hour_rate)`
- `total_cost` = `raw_material_cost + operating_cost`

Money is IDR, 2dp, half-away-from-zero (`money()` in the write service). Validation: a BOM needs a
positive base quantity and **≥ 1 component**, else `ManufacturingError::Invalid`. Lines are inserted
in the same transaction as the parent BOM.

Two staging dimensions sit on every BOM (ADR-002/ADR-005):

- **`version`** (default 1): one live BOM per `(company, item, version)` — a second hand-authored v1
  for the same item collides LOUDLY; stage the old revision to v2 and the slot frees.
- **`bom_type`** ∈ `normal | kit | subcontract` (default `normal`): a **kit** never gets its own work
  order (it explodes through to components at demand time), and a **subcontract** BOM's orders are
  minted only by the subcontract receipt event — both refusals happen at `confirm`, LOUD, before any
  requirement row exists.

Master data is **shared_blank** (ADR-0014 in the workspace index): `company_id` is nullable on
workstations, operations, and the BOM family — a NULL row is *shared* master data visible to every
company session, and the uniques are `NULLS NOT DISTINCT` so the shared namespace stays
collision-free. Company-owned rows win over shared rows (company-owned first ordering).

## Execution: the hybrid Work Order lifecycle

A Work Order carries a **6-state hybrid** vocabulary (ADR-002) — three *direct* writes and three
*derived* states:

```
draft ──confirm──▶ confirmed ──cancel──▶ cancel   (cancel is sticky, terminal)
                       │
        consume ───────┼─▶ progress        (derived by the consume gate)
                       │        │
        receive ◀──────┴────────┤
          partial ─▶ to_close   (derived by the receive accumulator)
          full    ─▶ done       (derived by the receive accumulator)
```

Availability lives ONLY in the read-side `reservation_state` projection
(`waiting | confirmed | assigned`) — no field named `availability` exists anywhere in the module.

### 1. `create_work_order` → `draft`
Inserts the WO with its target `quantity`, its `bom_id`, the logical-FK **warehouse ids** (WIP, FG),
optional per-order **GL account overrides**, and a `product_category_id` snapshot that selects the
costing defaults (below). A draft's `item_id` may still be corrected; after confirm the database
itself refuses a re-point (the F11 trigger, ADR-005).

### 2. `confirm_work_order` → `confirmed`
Explodes the BOM into `WorkOrderItem` rows — the required materials —
`required_qty = component.qty × WO.qty / BOM.qty` (BR-2). A **phantom** component
(`BomItem.is_phantom`) is never issued: `explode_bom` recurses *through* it to its own BOM's
components, with a depth cap guarding against a mis-authored cycle. The `draft → confirmed`
update is the gate; on success it emits `WorkOrderConfirmed`.

### 3. `consume_materials` → `progress` — **post #1**
Issues the outstanding required materials to WIP:

1. Drive `InventoryPort::issue_to_wip` — removes the components and returns their
   **moving-average value** (inventory's number, not the BOM's).
2. Post **`Dr WIP · Cr Raw-Material Stock`** for that value via `GlPostSink`.
3. Gate `confirmed → progress` (the gate *derives* progress — no verb writes it directly), add the
   value to `raw_material_cost`, bump each line's `consumed_qty`.

Requires the WIP + raw-material accounts — resolved through the chain below, `MissingAccount`
LOUD *before* any post or stock move. Idempotent: a second call short-circuits, WIP charged once.
Emits `MaterialsConsumed`.

### 4. `add_job_card` + `complete_job_card` — **post #2 (repeatable per card)**
A Job Card carries the shop-floor 5-state vocabulary `ready → progress → done | cancel | blocked`.
`add_job_card` records a `ready` card; `start_job_card` moves it to `progress` (the WO must be
confirmed or progress); `complete_job_card` charges its cost to WIP: post
**`Dr WIP · Cr Conversion-Applied`**, then gate `progress → done` and add to the WO's
`operating_cost`. Requires the WIP + conversion accounts. Idempotent: charged once even if completed
twice. Emits `ConversionCharged`. Many job cards may run over the life of one WO.

### 5. `receive_finished` → `to_close` / `done` — **post #3**
Receives finished goods (plus any byproduct legs, plus optional subcontract extra cost) at cost:

1. **Value the batch.** `Average` posture (default): the prorated share of accumulated WIP
   (`raw_material_cost + operating_cost` + any `extra_cost`); on a **full** receipt, clear all
   remaining WIP so it nets exactly to zero (no rounding residue). `Standard` posture: FG at
   `qty × standard_unit_price` — the pinned price is never recomputed — and the gap to actual posts
   to the **cost-variance account** as a plug (favourable gap credits, unfavourable debits).
2. **Split the byproducts.** Each byproduct leg's value = batch total × its BoM row's
   `cost_share / 100`; the FG line keeps the remainder. A byproduct with no BoM row is refused, and
   a family whose shares sum past 100 is `CostShareOverflow` — LOUD (ADR-003).
3. Drive `InventoryPort::receive_finished` — the FG and every byproduct enter stock at their values.
4. Post **`Dr Finished-Goods (+ byproduct legs) · Cr WIP (+ Cr Subcontract-Interim for extra cost)`**.
5. Gate the produced-quantity accumulator; derive `to_close` on a partial batch, `done` on the last.

Bounded by the ordered quantity — over-producing is `OverProduce`, LOUD. Idempotent per cumulative
produced qty. Emits `FinishedGoodsReceived`, plus `WorkOrderCompleted` on the last receipt.

## The account-resolution chain (one rule, three hops)

Every costing verb resolves its accounts the same way (ADR-003):

```
per-order override  →  category costing default  →  LOUD MissingAccount
```

`category_costing_defaults` holds, per `(company, product_category)`, the WIP / FG / raw /
conversion / subcontract-interim / cost-variance / inventory-loss / repair-expense accounts. A
seeded-but-NULL column is NOT a resolution — the refusal stays LOUD. No hardcoded fallback exists
anywhere in the module.

## The invariant: WIP nets to zero

Across the three posts, everything charged **into** WIP is credited back **out** to finished goods:

```
consume   Dr WIP            Cr Raw-Material Stock     (raw value)
operate   Dr WIP            Cr Conversion-Applied     (conversion value)
receive   Dr Finished-Goods Cr WIP                    (raw + conversion + extra)
──────────────────────────────────────────────────────────────────
          WIP debits  ==  WIP credits   ⇒  WIP = 0 on full receipt
```

This is textbook job-order costing and the seam's one **provable** invariant — proven end-to-end
against the real accounting ledger in `tests/wip_netting.rs` (consume `Dr WIP 6,000 · Cr Raw 6,000`,
operate `Dr WIP 60 · Cr Conv 60`, receive `Dr FG 6,060 · Cr WIP 6,060`; **WIP = 0**). Turn
manufacturing off and the GL still balances — it only *adds* WIP/FG value, reversibly (ADR-001).
The subcontract DoD numbers (`tests/byproduct_cost_share.rs`): components 6,000 + purchase extra
2,500 → `Dr FG 8,500 · Cr WIP 6,000 · Cr Interim 2,500`.

## Unbuild: reversing a done order (ADR-004)

An `UnbuildOrder` is a plain 2-state record (`draft → done`) that reverses FINISHED goods of a
**done** work order back into components. It never creates a reverse work order and never writes
`work_orders` — the source stays `done`, its accumulators untouched; how much of its output has
been unbuilt is answered by summing done unbuilds. Guards, both LOUD: the source must be `done`
(`UnbuildSourceNotDone` — unbuilding a half-produced order would strand WIP), and the quantity may
not exceed what remains after every other done unbuild (`UnbuildOverRemaining` — never a silent
clip). The move is ONE port call (`reverse_production`: FG out, components back at their share of
the recovered value); the GL mirrors it exactly — `Dr Raw Σ · Cr FG Σ` — so no value is created or
destroyed.

## Repair: three part legs, one grouped post (ADR-004)

A `RepairOrder` fixes a broken item with three kinds of part leg (`add` / `remove` / `recycle`),
verb-driven: `draft → (validate) confirmed → (start) under_repair → (end) done | cancel`. NO leg
moves stock until `end_repair` — which executes EVERY leg in one pass through
`InventoryPort::execute_repair_leg`, so a cancel before end needs no move cancellation. `validate`
probes every add-leg's availability read-only through the port (advisory-checked,
authority-held — the authoritative refusal is the leg execution itself). The GL is ONE grouped
balanced post:

```
add      Dr Repair-Expense  · Cr Raw-Material Stock   (part consumed into the repair)
remove   Dr Inventory-Loss  · Cr Raw-Material Stock   (part scrapped — to the LOSS ACCOUNT,
                                                       never a quarantine location)
recycle  Dr Raw-Material    · Cr Repair-Expense       (part recovered back into stock)
```

`done` is uncancelable — its legs have moved real stock. Repair *billing* is not this module's
surface (no fees exist anywhere in the family).

## Subcontract: the hidden MO behind a purchase receipt (ADR-004)

When buying emits a receipt event whose `order_kind` is `subcontract`, manufacturing mints the
hidden manufacturing order — **once per purchase order** (the `subcontract_mo_links` row is the
replay backstop; a replayed event returns the same WO id). The mint is atomic: `insert_confirmed` +
the exploded requirements + the link row land in ONE transaction — there is no orphan-draft window.
The item's ACTIVE BoM must be `bom_type='subcontract'` (anything else is an authoring defect,
LOUD); the minted MO lands directly `confirmed`, numbered by the purchase reference, with warehouses
left NULL for the operator to set. Zero buying writes, zero SVL — the inventory/ledger moves ride
the ordinary consume/receive verbs. `SubcontractKindMismatch` is the guard that keeps a plain goods
receipt from ever minting production orders.

## Workcenter: OEE as a pure read (zero crons)

Productivity losses are named master data (`WorkstationLoss`: `productive | availability |
performance | quality` — shared or company-owned). `WorkstationProductivity` rows book stretches of
workstation time against them; duration is **read-side computed** (`date_end − date_start`, an open
stretch counts up to now). `workstation_oee` reports the four ratios over any window — each
`(total − respective losses) / total`, `oee = a × p × q`, with the window CLAMPED at both edges (a
stretch straddling the edge contributes only its inside part). The module declares
**`scheduled_jobs: {}`** — no cron, no stored aggregate, nothing scheduled. A window with zero
booked seconds reports all ratios 0 (a vacuous 1.0 would overstate an unmeasured station).

## Why the postings are safe under retry

Two mechanisms, in the write services and ADR-001 §4–§5:

- **Distinct-voucher dedup.** Every post is `posting_type = "original"`; distinctness is the derived
  `source_id` (a v5 UUID namespaced on the aggregate id) and the envelope's `idempotency_key`
  (`consume:{wo}`, `receive:{wo}:{cumulative}`, `operate:{job_card}`, `unbuild:{id}`,
  `repair-leg:{repair}:{index}`, `repair:{id}`). Accounting dedups on
  `(company, source_type, source_id, posting_type)`; the inventory port dedups on the key — so a
  replayed envelope or a re-executed leg is a no-op.
- **Side-effects-before-gate.** Each verb runs its idempotent side effects (inventory move, then the
  GL post) **before** committing its status-transition gate. A crash between them leaves the order
  in its prior state; the retry re-drives and the keys dedup.

## How to run a Work Order (the happy path)

All through `ManufacturingWriteService` with the composing service supplying the `InventoryPort` and
`GlPostSink` adapters:

```text
create_bom            → recipe + cost roll-up (version 1, bom_type normal)
create_work_order     → draft, with warehouses + (optional) account overrides
confirm_work_order    → confirmed; BOM exploded into required materials
consume_materials     → progress; Dr WIP · Cr Raw          (drives inventory)
add_job_card
start_job_card
complete_job_card     → Dr WIP · Cr Conversion            (per card)
receive_finished      → to_close/done; Dr FG · Cr WIP     (drives inventory; WIP = 0)
```

Unbuilding, repairing, and the subcontract receipt event follow their own verbs above. Subscribe to
`ManufacturingEvent` (`WorkOrderConfirmed`, `MaterialsConsumed`, `ConversionCharged`,
`FinishedGoodsReceived`, `WorkOrderCompleted`, `WorkOrderCancelled`, `UnbuildExecuted`,
`RepairCompleted`, `SubcontractMoMinted`) via `ManufacturingEventSink` for a production dashboard or
costing analytics.

## Boundaries (what this module will not do)

- **Posts WIP/FG-family only** — never route a revenue/AR post through it.
- **No Cargo edge** to accounting/inventory/catalog/buying — cross-module ids are logical FKs; the
  ports (and the serialized subcontract receipt event) are the only contracts.
- **Deferred** (PRD non-goals): Production Plan / MPS / MRP, capacity scheduling, component-to-
  subcontractor moves inside subcontract depth, receipt-validation closing, routing as a reusable
  master, scrap/short-close.

## Where each claim is anchored

| Claim | Source |
|-------|--------|
| Cost roll-up formulas | `create_bom` + `bom.model.yaml` header |
| Version/type staging + the kit/subcontract refusals | `bom.model.yaml` + `confirm_work_order` + `tests/bom_version_stage.rs` / `tests/kit_no_mo.rs` |
| Hybrid states, sticky cancel, reservation_state | ADR-002 + `tests/state_convergence_probes.rs` |
| Three postings, WIP=0 | `consume_materials` / `complete_job_card` / `receive_finished` + `tests/wip_netting.rs` |
| Account chain, byproduct split, variance plug | ADR-003 + `tests/costing_defaults_probes.rs` / `tests/byproduct_cost_share.rs` |
| Unbuild / repair / subcontract | ADR-004 + `tests/unbuild_golden_cases.rs` / `tests/repair_lifecycle_probes.rs` / `tests/subcontract_golden_cases.rs` |
| OEE math + zero crons | `workstation_oee` + `tests/workcenter_oee.rs` / `tests/zero_crons.rs` |
| shared_blank fence, F11, strict posture | ADR-005 + `tests/fence_shared_blank_probes.rs` / `tests/f11_item_immutable.rs` |
| Idempotency + gates | write-service transition updates + [ADR-001](../adr/ADR-001-manufacturing-boundary-and-wip-seam.md) §4–§5 |
| The numbers | [golden cases](../business-flows/golden-cases.md) (MGC / IP / PTPSEAM) |

---

Related: [Architecture §4b](04-architecture.md#4b-data--control-flow--the-wip-lifecycle) traces the
lifecycle as a sequence diagram · [Glossary → Manufacturing domain terms](08-glossary.md#manufacturing-domain-terms) ·
[Extension guide](../extension-guide.md) for the stable integration surface.
