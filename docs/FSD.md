# FSD — backbone-manufacturing

> Functional Spec. Tier 3 · Manufacturing pillar (GL producer). Date: 2026-07-06.

## Entities (schema/models/*.model.yaml — SSoT)
Workstation (`hour_rate`) · Operation (`default_workstation_id`) · Bom (`quantity`, cost roll-up) +
BomItem (`quantity`/`rate`/`amount`/`is_phantom`) + BomOperation (`time_in_mins`/`hour_rate`/`operating_cost`) ·
WorkOrder (`quantity`, `produced_qty`, `status`, cost accumulators, WIP/FG warehouses + 4 GL accounts)
+ WorkOrderItem (`required_qty`/`consumed_qty`) · JobCard (`total_time_mins`/`hour_rate`/`operating_cost`,
`status`). Cross-module ids are logical FKs (`@exclude_from_foreign_key_check`): item→catalog,
warehouses→inventory, accounts→accounting, company→organization.

## Services (application/service — hand-authored, user_owned)
- `ManufacturingWriteService` — `create_bom` (roll-up), `create_work_order`, `release_work_order`
  (explode), `consume_materials` (issue → `Dr WIP·Cr Raw`), `add_job_card` + `complete_job_card`
  (`Dr WIP·Cr Conversion`), `receive_finished` (`Dr FG·Cr WIP`, WIP nets to zero, bounded).
- `manufacturing_gl` — the outbound `GlPostSink` + `AccountingPostEnvelope` (source_type
  "manufacturing"); zero normal Cargo edge to accounting.
- `manufacturing_ports` — `InventoryPort` (`issue_to_wip` returns valued issue; `receive_finished`);
  zero normal edge to inventory.
- `manufacturing_events` — `ManufacturingEvent` union + sink.

## State machines
- WorkOrder: `draft → released → in_process → completed` (+ `stopped`). Each transition is the once-only
  gate for its post.
- JobCard: `open → completed` (conversion charged on completion).

## Integration seams
- **Plant-to-produce (proven, marquee):** manufacturing emits three balanced posts through `GlPostSink`
  into the REAL accounting ledger; **WIP nets to zero** on completion (`tests/plant_to_produce_seam.rs`).
  Material valuation flows from the `InventoryPort`. ADR-001, `scripts/plant_to_produce_seam_roundtrip.sh`.
- **Inbound (future):** a Production Plan producing Work Orders; a real inventory Manufacture Stock Entry.

## Test oracle
`manufacturing_golden_cases` (4: BOM roll-up, WO explosion, validation, **MGC-4 phantom explodes through**), `integrity_probes` (6:
over-produce bounded, consume idempotent, receive idempotent, missing-account-before-post, job-card
charged once), `plant_to_produce_seam` (1: WIP nets to zero across manufacturing + REAL accounting +
inventory port) + §5 round-trip. **11 tests.**
