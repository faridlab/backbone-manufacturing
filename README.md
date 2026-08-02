# backbone-manufacturing

A Backbone/Metaphor **domain module** (bounded context) for discrete manufacturing —
*what to build* (Bill of Materials) and *the act of building* (Work Orders). A library
crate (`[lib]` only) consumed by `backend-service` projects; wires its services via
`ManufacturingModule::builder()`.

Manufacturing **owns no stock and no ledger**. It defines cost (BOMs), drives
`backbone-inventory` for the physical moves + valuation, and emits the WIP/FG postings
through `backbone-accounting`. Cross-module ids are logical FKs.

## The domain

The model is bipartitioned along the real seam (mirrored in `schema/models/`):

**Product definition** — *what to build*; master data, posts **no GL**.
- **Workstation** — a machine/work-center with an hourly conversion-cost rate.
- **Operation** — a named production step (cut, assemble, …), optionally on a default workstation.
- **Bom** — the recipe for one manufactured item: its component materials (`BomItem`) and the
  operations that convert them (`BomOperation`). Cost rolls up:
  `raw_material_cost (Σ items) + operating_cost (Σ operations) = total_cost`.
- **BomItem** — one component line; a *phantom* sub-assembly is exploded through to its own
  BOM at release, never stocked.
- **BomOperation** — one operation line (drives operating cost; hour rate snapshotted).

**Execution** — *the act of building*; transactional, **emits GL**.
- **WorkOrder** — an order to produce `qty` of an item against a BOM. Lifecycle:
  `draft → released → in_process → completed` (`stopped` halts). Releasing explodes the BOM
  into required materials (`WorkOrderItem`).
- **WorkOrderItem** — a required material exploded from the BOM, with consumption tracking.
- **JobCard** — a shop-floor record of an operation run; completing it charges conversion
  cost to WIP (`open → completed`).

Status enums: `WorkOrderStatus`, `JobCardStatus`.

## The WIP / FG costing seam (the core invariant)

A Work Order's value flows through WIP in **three balanced posts**, so **WIP nets to zero on
completion**:

| Step   | Posting                          | Meaning                                  |
|--------|----------------------------------|------------------------------------------|
| consume  | Dr WIP · Cr Raw-Material Stock | materials issued to WIP (valued by inventory) |
| operate  | Dr WIP · Cr Conversion-Applied  | job-card labour/overhead                  |
| receive  | Dr Finished-Goods · Cr WIP      | FG = raw + operating                       |

Each post is transition-gated (the status advance is the once-only guard) and keyed by a
stable idempotency key, so a retry never double-charges WIP. This lives in
`ManufacturingWriteService` (`src/application/service/manufacturing_*.rs`), proven by the
golden cases and the plant-to-produce seam test against the real accounting ledger.

## HTTP surface — two paths, deliberately distinct

1. **Unguarded generic CRUD** — `ManufacturingModule::all_crud_routes()` mounts 12 endpoints
   per entity with **no** domain validation. It can create invalid rows or flip a WorkOrder to
   `completed` with zero GL posts (stranded WIP). Use it only for **reads, trusted/admin, or
   seeding**. (`routes()` is a deprecated alias for the same unguarded surface.)
2. **Validated write commands** — [`write_api`](src/write_api.rs) forwards the WIP-costing
   transitions to `ManufacturingWriteService`:

   ```rust
   use backbone_manufacturing::{ManufacturingModule, write_api};
   use std::sync::Arc;

   let m = ManufacturingModule::builder().with_database(pool.clone()).build()?;

   // The validated command router (release → consume → operate → receive):
   let deps = write_api::ManufacturingWriteDeps {
       write_service: m.write_service(),
       inventory: Arc::new(my_inventory_adapter), // real InventoryPort over backbone-inventory
       gl:        Arc::new(my_gl_adapter),        // real GlPostSink over backbone-accounting
       events:    Arc::new(LoggingSink),          // or your bus sink
   };
   let app = Router::new()
       .merge(m.all_crud_routes())                                         // reads / admin
       .merge(write_api::create_manufacturing_write_routes().with_state(deps));
   ```

   The ports are **caller-supplied per call** — a no-op `GlPostSink` compiles and looks done but
   silently drops the WIP postings (WIP leaks). Always supply real adapters from the composing
   service.

## Quick start

```bash
metaphor schema schema validate          # check schema YAML
metaphor dev test                         # run tests (DB-gated suites need DATABASE_URL)
metaphor migration run                    # apply migrations
```

## Schema is the single source of truth

`schema/models/*.model.yaml` defines every entity; code is regenerated from it.

- `bom.model.yaml` — Workstation, Operation, Bom, BomItem, BomOperation.
- `work_order.model.yaml` — WorkOrder, WorkOrderItem, JobCard + `WorkOrderStatus`/`JobCardStatus`.
- `index.model.yaml` — module/schema identity, shared types, and the `generators` config.

**Regeneration preserves only `// <<< CUSTOM … // END CUSTOM` blocks.** Custom logic goes in
`*_custom.rs` siblings (e.g. the `manufacturing_*.rs` write-service chunks) or inside CUSTOM
markers — never hand-edit generated code outside them.

### Generator config

`index.model.yaml` disables generators manufacturing doesn't wire:

```yaml
generators:
  disabled:
    - graphql
    - grpc
    - proto
    - auth               # manufacturing wires only service/validator/workflows
    - bulk_operations    #   + inline route composition (all_crud_routes / write_api)
    - usecases
    - routes_composer    # src/routes/ composition — not used (compose inline instead)
    - handlers_module    # src/handlers/ AppState — not used
```

(These per-entity layers are opt-in at the generator source via `layers: true`; manufacturing
doesn't wire them, so they're disabled. `specification` is kept — manufacturing wires it.)

## Project layout

```
schema/models/          # SSoT — bom.model.yaml, work_order.model.yaml, index.model.yaml
migrations/             # tables + enums + company RLS + audit triggers
src/
├── lib.rs              # ManufacturingModule + builder + all_crud_routes() + write_service()
├── write_api.rs        # validated command router + ManufacturingWriteDeps
├── domain/             # entities, repositories (ports), events, specifications, policies
├── application/
│   ├── service/        # GenericCrudService aliases + manufacturing_write_service.rs (+ chunks:
│   │                   #   bom_definition, work_order, execution, job_card, gl, events, ports)
│   ├── validator/
│   └── workflows/
├── infrastructure/     # persistence (GenericCrudRepository newtypes), event_store, integration
├── presentation/       # http handlers (BackboneCrudHandler), dto, versioning
├── exports/            # public read contract (ManufacturingQueryService trait)
└── seeders/
tests/                  # manufacturing_golden_cases, plant_to_produce_seam, integrity_probes, integration
```

## Further reading

- `docs/FSD.md`, `docs/PRD.md` — functional + product spec.
- `docs/handbook/` — glossary, maintainer guide, schema architecture.
- `docs/adr/` — the manufacturing boundary and the WIP job-order costing seam.
- `docs/council/` — decision records (e.g. the bounded-context-cleanliness review).
