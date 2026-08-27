# ADR-005: shared_blank master data and the database-level guards

Status: accepted · Date: 2026-08-27 · Implements the workspace's ADR-0014 posture map

## Context

Manufacturing master data (workstations, operations, the BOM family, productivity-loss reasons) is
authored once and shared across companies in the Odoo style: a NULL `company_id` row is *shared*,
visible to every company session; a company-owned row shadows it. Meanwhile the transaction tables
(work orders, job cards, unbuilds, repairs, productivity rows) must stay strictly company-fenced.
Two silent-failure classes needed closing at the database level: a post-confirm re-point of a work
order's item, and any scheduled aggregate that could drift.

## Decisions

**shared_blank fence.** `company_id` became nullable on `workstations`, `operations`, `boms`,
`bom_items`, `bom_operations` (and is nullable-by-design on `workstation_losses` and
`bom_byproducts`). The RLS policies were recreated with an `OR company_id IS NULL` arm in the SAME
migration as the `DROP NOT NULL` — a session that applied one without the other would either see
shared rows it must not see, or could not insert shared rows at all. Shared-namespace uniques are
recreated `NULLS NOT DISTINCT` (PG 15+) so two shared rows with the same code collide — with plain
partial uniques, NULL ≢ NULL and the shared namespace would be collision-free by accident.
Company-owned rows win resolution (the `ORDER BY (company_id IS NULL) ASC` arm).

**Strict tables stay strict.** `work_orders.company_id` (and every other transaction table's)
remains NOT NULL — the fence probes assert this at the schema level, because the test connection is
a superuser and RLS visibility cannot be exercised in-suite.

**F11 backstop.** A `BEFORE UPDATE` trigger refuses any change to `work_orders.item_id` once the
order has left draft — a silent re-point would strand already-consumed components against the wrong
product and desynchronise the order quantity from the stock-move estate. The application layer is
not the only guard.

**Zero crons.** The hooks manifest pins `scheduled_jobs: {}` explicitly, no scheduler dependency
exists, and no migration seeds scheduled rows. Every aggregate is either verb-driven or — like the
OEE ratios — computed on read over window-clamped time buckets.

## Consequences

- Master data can be seeded shared and specialized per company without a copy per tenant.
- The unique constraints hold the shared namespace honest even for a superuser seeding path.
- A DB-level immutability guard survives application bugs, raw SQL, and future verbs alike.

Anchors: `migrations/20260901110200_fence_shared_blank_master_data.up.sql`,
`migrations/20260901110100_f11_item_immutable_trigger.up.sql`,
`schema/hooks/manufacturing.hook.yaml`, `tests/fence_shared_blank_probes.rs`,
`tests/f11_item_immutable.rs`, `tests/zero_crons.rs`.
