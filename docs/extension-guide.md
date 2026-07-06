# Extension guide — backbone-manufacturing

## Public / stable surface
- **The GL-posting port.** `GlPostSink` + `AccountingPostEnvelope` (`manufacturing_gl.rs`) — the WIP/FG
  posts manufacturing emits. A composing service implements it over accounting's `PostingService`.
- **The inventory port.** `InventoryPort` (`issue_to_wip` / `receive_finished`, `manufacturing_ports.rs`)
  — how manufacturing moves + values stock. Wire it to backbone-inventory.
- **Write verbs.** `ManufacturingWriteService::{create_bom, create_work_order, release_work_order,
  consume_materials, add_job_card, complete_job_card, receive_finished}` — hand-authored, survive regen.
- **Domain events.** `ManufacturingEvent` {released, consumed, conversion-charged, FG-received, completed}
  via `ManufacturingEventSink`. Subscribe for a production dashboard / costing analytics.
- **The 12 generated CRUD endpoints** per entity (author BOMs / workstations / operations).

## Boundaries
- Manufacturing posts WIP/FG only — never route a revenue/AR post through it.
- Cross-module ids are logical FKs; manufacturing never imports accounting/inventory/catalog.
- A new GL producer must be registered in accounting's `PostingSourceType` enum (as `manufacturing` is).

## How to…
- **Run a Work Order:** `create_bom` → `create_work_order` → `release_work_order` → `consume_materials`
  (drives inventory + posts WIP) → `add_job_card`/`complete_job_card` (conversion) → `receive_finished`
  (FG in, WIP clears). WIP nets to zero at completion.
