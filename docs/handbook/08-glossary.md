<!-- Reader: All · Mode: Reference -->
# Glossary — ubiquitous language

One term, one meaning, used everywhere in this handbook and in the code. When a term here names a
type or file, that name is exact. If you find a doc using a different word for one of these, the doc
is the bug.

### Aggregate / Entity
A domain object with identity and a lifecycle, defined by one `schema/models/<name>.model.yaml`.
This module's eight: `Workstation`, `Operation`, `Bom`, `BomItem`, `BomOperation`, `WorkOrder`,
`WorkOrderItem`, `JobCard`. Each is generated into `src/domain/entity/<name>.rs` with a strongly-typed
id, a builder, `apply_patch`, and audit accessors.

### Application layer
The use-case layer (`src/application/`): services and DTOs. Depends on the domain; knows nothing
about HTTP or SQL.

### Audit metadata
The `metadata` JSONB field (`created_at`, `updated_at`, `deleted_at`, `created_by`, `updated_by`,
`deleted_by`) added when `config.audit: true`. Timestamps are set by a Postgres trigger; the `*_by`
actor fields are logical FKs to `sapiens.User.id`.

### `BackboneCrudHandler`
The `backbone-core` type that produces an Axum `Router` with all **twelve** CRUD endpoints for an
entity. Invoked as `BackboneCrudHandler::<…>::routes(service, "/collection")`. You never hand-write
these routes.

### Bounded context
The single business domain a module owns. One module = one bounded context. A module never edits
another's schema; it references other modules by logical FK.

### `all_crud_routes()`
The method on `ManufacturingModule` ([`src/lib.rs`](../../src/lib.rs)) that merges the eight
entities' `BackboneCrudHandler` routers into one `Router` — the **full unguarded** CRUD surface (no
domain validation). Mount a **guarded** composition (read routes open, writes behind
validation/auth) for production; the deprecated `routes()` alias mounts the same unguarded surface.
The `/api/v1` prefix is added separately by `routes::get_routes`, not by `all_crud_routes()`.

### Composition root
[`src/lib.rs`](../../src/lib.rs) — the `ManufacturingModule` struct and `ManufacturingModuleBuilder`.
Wires each of the eight services to its repository and composes the routers. The one place allowed to
depend on every layer. (Not `src/module.rs`, which is dead, uncompiled skeleton code — no `mod`
statement declares it.)

### CUSTOM marker
A `// <<< CUSTOM … // END CUSTOM` region inside a generated file. Content between the markers
survives regeneration. Spelling varies per file (`// <<< CUSTOM METHODS START >>>`, `// <<< CUSTOM
DTOs`, …) — match what is already there.

### DTO (Data Transfer Object)
A wire-shape struct in `src/application/dto/`. Per entity: `Create…Dto`, `Update…Dto`, `Patch…Dto`,
`…ResponseDto`, `…SummaryDto`, `…ListResponseDto`. Serialized `camelCase`. Generated, with
`From`/`Apply` conversions to and from the entity.

### Domain layer
The innermost layer (`src/domain/`): entities, value objects, enums, invariants, and repository
**traits** (ports). Depends on nothing.

### Generation targets
The 31 kinds of artifact `metaphor schema schema generate` can emit (`rust`, `sql`, `dto`,
`handler`, `repository`, `service`, `proto`, `openapi`, …). `--target all` (default) emits the lot;
a comma-separated subset emits part.

### `GenericCrudRepository` / `GenericCrudService`
The `backbone-orm` / `backbone-core` generics that carry all standard CRUD. A module's repository is
a **newtype** over `GenericCrudRepository<Entity, SoftDelete>`; its service is a **type alias** over
`GenericCrudService<Entity, CreateDto, UpdateDto, Repository>`. Inherited, never re-implemented.

### Infrastructure layer
The adapter layer (`src/infrastructure/`): repository implementations, cache, messaging, jobs.
Depends on domain and application.

### Logical foreign key
A cross-module reference declared with `@foreign_key(module.Type.field)` (e.g.
`@foreign_key(sapiens.User.id)`). It documents the relationship and is *not* enforced by a database
constraint, so modules stay independently deployable.

### `metaphor`
The workspace CLI (v0.2.0) that orchestrates the projects and dispatches to plugins
(`metaphor-schema`, `metaphor-codegen`, `metaphor-dev`). Prefer it over raw `cargo`/`sqlx`. Note:
the standalone `backbone-schema` binary the README mentions is **not** installed; use `metaphor
schema schema …`.

### Module
A **library crate** owning one bounded context in 4-layer DDD, schema-driven. `[lib]` only — no
`main.rs`. Composed into a `backend-service`; never run alone. This repo *is* one: the
`manufacturing` module (`ManufacturingModule`).

### Own schema (per module)
Each module gets its own Postgres schema (`schema: manufacturing` in `index.model.yaml`). Migrations
`CREATE SCHEMA <module>` and qualify tables as `<module>.<table>`, so modules never collide on a
table name.

### Port / Adapter
The DDD names for the two repository types per entity (e.g. `BomRepository`): the **port** is the
domain-layer `trait` (the contract); the **adapter** is the infrastructure-layer `struct` (the
Postgres implementation). The term also names the module's outward seams — `InventoryPort` and
`GlPostSink` are ports the *composing service* adapts to real inventory/accounting.

### Presentation layer
The transport layer (`src/presentation/`, `src/routes/`): HTTP handlers, route composition, and
optionally gRPC/GraphQL. Depends on the application layer.

### Regeneration (regen)
Re-running `metaphor schema schema generate … --force` to rebuild all downstream code from the
schema. Overwrites everything **outside** a protected region (CUSTOM markers, `*_custom.rs`,
`user_owned` globs).

### Schema (the SSoT)
`schema/models/*.model.yaml` — the single source of truth. Every entity struct, DTO, migration,
repository, service, handler, and route is generated from it. Not to be confused with the *Postgres
schema* (the per-module namespace).

### Soft delete
Marking a row deleted (`metadata.deleted_at` set) instead of removing it, enabled by
`config.soft_delete: true`. Backs the `soft_delete` / `restore` / `empty_trash` / `list_deleted`
endpoints.

### Twelve endpoints
The standard CRUD surface every entity gets from `BackboneCrudHandler`: `list`, `create`, `get`,
`update`, `patch`, `soft_delete`, `restore`, `empty_trash`, `bulk_create`, `upsert`, `find_by_id`,
`list_deleted`.

### `user_owned`
The `metaphor.codegen.yaml` key listing glob paths the generator skips wholesale — never reads,
merges, or deletes. This module protects `tests/features/**` and `docs/**` (this handbook lives
under one of them). Hand-authored service files add themselves here or use the marker mechanism.

---

## Manufacturing domain terms

The ubiquitous language of the `manufacturing` bounded context. Sourced from the schema headers
([`bom.model.yaml`](../../schema/models/bom.model.yaml),
[`work_order.model.yaml`](../../schema/models/work_order.model.yaml)), [BRD](../BRD.md), and
[ADR-001](../adr/ADR-001-manufacturing-boundary-and-wip-seam.md). See
[The manufacturing domain](09-manufacturing-domain.md) for the narrative.

### Master data vs execution
The domain's central split. **Master data** (`Bom`, `BomItem`, `BomOperation`, `Workstation`,
`Operation`) *defines* what to build and rolls up its planned cost — it posts **no** GL. **Execution**
(`WorkOrder`, `WorkOrderItem`, `JobCard`) is the *act* of building — it emits the WIP/FG postings.

### BOM (Bill of Materials) — `Bom`
The recipe for one manufactured item: its component materials (`BomItem`) and the operations that
convert them (`BomOperation`). Carries the cost roll-up (`raw_material_cost` + `operating_cost` =
`total_cost`) per its base `quantity`. Unique per `(company_id, bom_code)`.

### BOM Item — `BomItem`
One component line of a BOM: `quantity` × `rate` = `amount`. If `is_phantom`, it is a sub-assembly
that is **never stocked** — at Work Order release it is exploded *through* to its own BOM's
components rather than issued as a material.

### BOM Operation — `BomOperation`
One operation line of a BOM: `time_in_mins / 60 × hour_rate = operating_cost`. The `hour_rate` is
snapshotted from the `Workstation` it runs on. Drives the BOM's `operating_cost`.

### Workstation
A machine/work-center with an `hour_rate` (conversion cost per hour — labour + overhead). Supplies
the rate to BOM operations and job cards.

### Operation
A named production step (e.g. cut, assemble), optionally on a `default_workstation_id`.

### Work Order — `WorkOrder`
An order to manufacture `quantity` of an item against a `Bom`. Its lifecycle
(`draft → released → in_process → completed`, plus `stopped`) emits the three WIP posts and
accumulates `raw_material_cost` + `operating_cost`. Carries logical-FK warehouse ids (WIP/FG) and
four GL account ids (WIP, FG, raw-material, conversion-cost).

### Work Order Item — `WorkOrderItem`
A required material exploded from the BOM at release: `required_qty = component.qty × WO.qty / BOM.qty`.
Tracks `consumed_qty` (bounded by `required_qty`) as materials are issued to WIP.

### Job Card — `JobCard`
A shop-floor record of an operation run for a Work Order (`open → completed`). On completion its
`operating_cost` (`total_time_mins / 60 × hour_rate`) is charged to WIP **once**.

### WIP (Work-In-Progress)
The costing account a Work Order's value passes through. Debited by consume (materials) and operate
(conversion), credited by receive (finished goods). On a full receipt, **WIP nets to zero** — the
seam's one provable invariant.

### The three postings (consume / operate / receive)
The balanced GL posts a Work Order emits, all `source_type = "manufacturing"`,
`posting_type = "original"`:
- **consume** — `Dr WIP · Cr Raw-Material Stock` (materials issued; value from inventory).
- **operate** — `Dr WIP · Cr Conversion-Applied` (job-card labour/overhead).
- **receive** — `Dr Finished-Goods · Cr WIP` (FG = raw + operating).

### Cost roll-up
The BOM's planning cost: `raw_material_cost` = Σ component `amount`; `operating_cost` = Σ operation
cost; `total_cost` = the sum. Computed in `ManufacturingWriteService::create_bom`. Money is IDR,
2dp, half-away-from-zero. Distinct from *actual* FG cost, which comes from real consumed value.

### Conversion cost
Labour + overhead applied to WIP via job cards (`time / 60 × hour_rate`), credited to a
Conversion-Applied account. The "operating" half of a finished good's cost.

### AccountingPost seam (`GlPostSink` / `AccountingPostEnvelope`)
The outbound port through which manufacturing emits its balanced postings. It serializes an
`AccountingPostEnvelope` (`source_type = "manufacturing"`); a composing service maps it to
accounting's `PostingService`. The module has **zero normal Cargo edge** to accounting.
([`manufacturing_gl.rs`](../../src/application/service/manufacturing_gl.rs).)

### `InventoryPort`
The seam manufacturing drives to move + value stock: `issue_to_wip` (removes components, returns
moving-average value) and `receive_finished` (adds FG at the computed value). Manufacturing owns no
stock; a composing service wires this to `backbone-inventory`.
([`manufacturing_ports.rs`](../../src/application/service/manufacturing_ports.rs).)

### Logical FK to accounting / inventory / catalog / organization
Cross-module ids carried on manufacturing rows with `@exclude_from_foreign_key_check` — documented
references, not DB constraints: `item_id → catalog.Product`, `*_warehouse_id → inventory.Warehouse`,
`*_account_id → accounting.Account`, `company_id → organization.Company`. Keeps modules
independently deployable.

### `ManufacturingModule`
The module's composition root struct ([`src/lib.rs`](../../src/lib.rs)). Built via
`ManufacturingModule::builder().with_database(pool).build()?`; holds the eight `Arc<…Service>`s and
exposes `all_crud_routes()`.

### `ManufacturingWriteService`
The hand-authored (user-owned, regen-safe) write path
([`manufacturing_write_service.rs`](../../src/application/service/manufacturing_write_service.rs))
that carries all domain logic: `create_bom`, `create_work_order`, `release_work_order`,
`consume_materials`, `add_job_card`, `complete_job_card`, `receive_finished`. Not one of the eight
generated CRUD aliases.

### Side-effects-before-gate
The idempotency discipline of every write verb: perform the idempotent side effects (inventory move,
GL post — each keyed so a retry never repeats) **first**, then commit the status-transition gate. A
crash mid-saga leaves the Work Order in its prior state and a retry re-drives; WIP never strands
non-zero. ([ADR-001](../adr/ADR-001-manufacturing-boundary-and-wip-seam.md) §5.)
