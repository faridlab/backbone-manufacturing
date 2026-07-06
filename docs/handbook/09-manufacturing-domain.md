<!-- Reader: Maintainer + App developer · Mode: Explanation → How-to -->
# The manufacturing domain

This is the page that explains *why the module exists*. The generated CRUD (previous pages) is
table-stakes; the domain lives in one hand-authored file,
[`application/service/manufacturing_write_service.rs`](../../src/application/service/manufacturing_write_service.rs),
and two schema models. Read this to understand what a BOM and a Work Order mean here, how cost rolls
up, how the Work Order lifecycle drives inventory and the ledger, and the one invariant everything
protects: **WIP nets to zero.**

Authoritative sources this page narrates (don't duplicate — link out):
[BRD](../BRD.md) (rules BR-1…BR-6), [PRD](../PRD.md) (scope/non-goals),
[FSD](../FSD.md) (entities/state machines/seams),
[ADR-001](../adr/ADR-001-manufacturing-boundary-and-wip-seam.md) (the boundary + WIP seam),
[business flows](../business-flows/README.md) + [golden cases](../business-flows/golden-cases.md)
(the numeric oracle).

## The one split: master data vs execution

The domain divides cleanly in two, mirrored by the two schema models.

| | **Product definition** (master data) | **Execution** (transactional) |
|---|---|---|
| Schema | [`bom.model.yaml`](../../schema/models/bom.model.yaml) | [`work_order.model.yaml`](../../schema/models/work_order.model.yaml) |
| Entities | `Workstation`, `Operation`, `Bom`, `BomItem`, `BomOperation` | `WorkOrder`, `WorkOrderItem`, `JobCard` |
| Answers | **WHAT** to build | The **ACT** of building |
| GL | Posts **no** ledger — only *defines* cost | Emits the three WIP/FG postings |

A `Bom` is the recipe for one manufactured item: its component materials (`BomItem`) and the
operations that convert them (`BomOperation`, each run on a `Workstation` at an hourly rate). A
`WorkOrder` produces `quantity` of that item against a `Bom`, and *its* lifecycle is what turns raw
materials + labour into finished-goods value in the books. Manufacturing **owns no stock and no
ledger** — it drives inventory for the physical moves and emits balanced postings through a seam
(ADR-001).

## Master data: the cost roll-up

A BOM defines cost per its base `quantity`. `ManufacturingWriteService::create_bom` computes
(BR-1, `bom.model.yaml` header):

- `raw_material_cost` = Σ over components of `money(quantity × rate)`
- `operating_cost` = Σ over operations of `money(time_in_mins / 60 × hour_rate)`
- `total_cost` = `raw_material_cost + operating_cost`

Money is IDR, 2dp, half-away-from-zero (`money()` in the write service). Validation: a BOM needs a
positive base quantity and **≥ 1 component**, else `ManufacturingError::Invalid` (golden case MGC-3).
Component/operation lines are inserted in the same transaction as the parent BOM.

> This roll-up is a **planning** number. The *actual* finished-goods cost comes later from the value
> inventory reports at consume time — not from these BOM rates (ADR-001 §2).

## Execution: the Work Order lifecycle

A Work Order moves `draft → released → in_process → completed` (plus `stopped`), and **each
transition is the once-only gate for a posting** (`work_order.model.yaml` enums; FSD state machines).
A Job Card moves `open → completed`. The verbs, in order:

### 1. `create_work_order` → `draft`
Inserts the WO with its target `quantity`, its `bom_id`, and the logical-FK **warehouse ids** (WIP,
FG) and **GL account ids** (WIP, FG, raw-material, conversion-cost) it will post against. Quantity
must be positive.

### 2. `release_work_order` → `released`
Explodes the BOM into `WorkOrderItem` rows — the required materials —
`required_qty = component.qty × WO.qty / BOM.qty` (BR-2, MGC-2). A **phantom** component
(`BomItem.is_phantom`) is never issued: `explode_bom` recurses *through* it to its own BOM's
components (resolved by the phantom item's active/default BOM), with a depth cap guarding against a
mis-authored cycle (MGC-4). The `draft → released` update is the gate; on success it emits
`WorkOrderReleased`.

### 3. `consume_materials` → `in_process` — **post #1**
Issues the outstanding required materials to WIP:

1. Drive `InventoryPort::issue_to_wip` — removes the components from the raw warehouse and returns
   their **moving-average value** (inventory's number, not the BOM's).
2. Post **`Dr WIP · Cr Raw-Material Stock`** for that value via `GlPostSink`.
3. Gate `released → in_process`, add the value to `raw_material_cost`, and bump each line's
   `consumed_qty`.

Requires the WIP + raw-material accounts (`missing_account` *before* any post or stock move — IP-4).
Idempotent: a second call short-circuits, WIP charged once (IP-2). Emits `MaterialsConsumed`.

### 4. `add_job_card` + `complete_job_card` — **post #2 (repeatable per card)**
`add_job_card` records an `open` job card with `operating_cost = time / 60 × hour_rate`.
`complete_job_card` charges that cost to WIP: post **`Dr WIP · Cr Conversion-Applied`**, then gate
`open → completed` and add to the WO's `operating_cost`. Requires the WIP + conversion accounts.
Idempotent: charged once even if completed twice (IP-5). Emits `ConversionCharged`. Many job cards
may run over the life of one WO.

### 5. `receive_finished` → `completed` — **post #3**
Receives finished goods into stock at cost:

1. Value this receipt as the prorated share of accumulated WIP (`raw_material_cost + operating_cost`);
   on a **full** receipt, clear all remaining WIP so it nets exactly to zero (avoiding rounding
   residue).
2. Drive `InventoryPort::receive_finished` — add the FG at that value.
3. Post **`Dr Finished-Goods · Cr WIP`**.
4. Gate the produced-quantity advance; complete the WO when `produced_qty ≥ quantity`.

Bounded by the ordered quantity — over-producing is `OverProduce` (IP-1). Idempotent per cumulative
produced qty (IP-3). Emits `FinishedGoodsReceived`, plus `WorkOrderCompleted` on the last receipt.

## The invariant: WIP nets to zero

Across the three posts, everything charged **into** WIP is credited back **out** to finished goods:

```
consume   Dr WIP            Cr Raw-Material Stock     (raw value)
operate   Dr WIP            Cr Conversion-Applied     (conversion value)
receive   Dr Finished-Goods Cr WIP                    (raw + conversion)
──────────────────────────────────────────────────────────────────
          WIP debits  ==  WIP credits   ⇒  WIP = 0 on full receipt
```

This is textbook job-order costing and the seam's one **provable** invariant — proven end-to-end
against the real accounting ledger in `tests/plant_to_produce_seam.rs` (golden case PTPSEAM-1:
consume `Dr WIP 2,000 · Cr Raw 2,000`, operate `Dr WIP 60 · Cr Conv 60`, receive
`Dr FG 2,060 · Cr WIP 2,060`; **WIP = 0**). Turn manufacturing off and the GL still balances — it
only *adds* WIP/FG value, reversibly (ADR-001).

> **Day-one caveat (ADR-001 parking lot).** "WIP nets to zero" holds on a **full** receipt. A WO that
> yields less than ordered (real shop-floor scrap) cannot reach `completed`, and its residual WIP
> strands until a future `close_short` verb writes the shortfall to a scrap/variance account. Don't
> document scrap handling as if it exists — it is deferred.

## Why the postings are safe under retry

Two mechanisms, both in the write service and ADR-001 §4–§5:

- **Distinct-voucher dedup.** Every post is `posting_type = "original"`; distinctness is the derived
  `source_id` (consume/receive: a v5 UUID from the WO; operate: the job-card id). Accounting dedups on
  `(company, source_type, source_id, posting_type)`, so a replayed envelope is a no-op.
- **Side-effects-before-gate.** Each verb runs its idempotent side effects (inventory move keyed by
  `idempotency_key`, then the GL post) **before** committing its status-transition gate. A crash
  between them leaves the WO in its prior state; the retry re-drives and the keys dedup. `receive_finished`
  originally committed the gate *first* and could strand WIP non-zero on a failed receipt — corrected,
  proven by revert (ADR-001 §5).

## How to run a Work Order (the happy path)

From the [extension guide](../extension-guide.md), all through `ManufacturingWriteService` with the
composing service supplying the `InventoryPort` and `GlPostSink` adapters:

```text
create_bom            → recipe + cost roll-up
create_work_order     → draft, with warehouse + GL account ids
release_work_order    → released; BOM exploded into required materials
consume_materials     → in_process; Dr WIP · Cr Raw       (drives inventory)
add_job_card
complete_job_card     → Dr WIP · Cr Conversion            (per card)
receive_finished      → completed; Dr FG · Cr WIP; WIP = 0 (drives inventory)
```

Subscribe to `ManufacturingEvent` (`WorkOrderReleased`, `MaterialsConsumed`, `ConversionCharged`,
`FinishedGoodsReceived`, `WorkOrderCompleted`) via `ManufacturingEventSink` for a production
dashboard or costing analytics.

## Boundaries (what this module will not do)

- **Posts WIP/FG only** — never route a revenue/AR post through it.
- **No Cargo edge** to accounting/inventory/catalog — cross-module ids are logical FKs; the ports are
  the only contract.
- **Deferred** (PRD non-goals): Production Plan / MPS / MRP, capacity scheduling, subcontracting depth,
  routing as a reusable master, standard-vs-actual costing, scrap/short-close, partial-receipt
  proration nuance.

## Where each claim is anchored

| Claim | Source |
|-------|--------|
| Cost roll-up formulas | `create_bom` + `bom.model.yaml` header |
| Explosion + phantoms | `release_work_order` / `explode_bom` + `BomItem.is_phantom` |
| Three postings, WIP=0 | `consume_materials` / `complete_job_card` / `receive_finished` + `work_order.model.yaml` header |
| Idempotency + gates | write-service transition updates + [ADR-001](../adr/ADR-001-manufacturing-boundary-and-wip-seam.md) §4–§5 |
| The numbers | [golden cases](../business-flows/golden-cases.md) (MGC / IP / PTPSEAM) |

---

Related: [Architecture §4b](04-architecture.md#4b-data--control-flow--the-wip-lifecycle) traces the
lifecycle as a sequence diagram · [Glossary → Manufacturing domain terms](08-glossary.md#manufacturing-domain-terms) ·
[Extension guide](../extension-guide.md) for the stable integration surface.
