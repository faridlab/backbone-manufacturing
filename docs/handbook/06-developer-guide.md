<!-- Reader: App developer · Mode: Tutorial → How-to -->
# Developer Guide

Get from a checkout to a running `manufacturing` module with its eight entities and twelve REST
endpoints each. The tutorial part holds your hand once; the recipes assume you know your way around.
For the *domain* — what BOMs and Work Orders mean and how the WIP posts work — read
[The manufacturing domain](09-manufacturing-domain.md) after the quickstart.

Commands here were run against `metaphor 0.2.0`. Where the top-level [README](../../README.md)
shows a `backbone-schema`/`backbone` command, use the `metaphor` form below — those are the ones
that work today.

## Prerequisites

- **Rust** (2021 edition toolchain) and **Cargo**.
- The **`metaphor`** CLI on your `PATH` (`metaphor --version` → `metaphor 0.2.0` or newer).
- A reachable **PostgreSQL** instance.

## Install

`backbone-manufacturing` is a **library crate**, not a runnable service — you either work inside this
module or depend on it from a `backend-service`.

```bash
# Work inside the module:
cd backbone-manufacturing

# The backbone-* framework crates are GIT dependencies pinned to branch = "main" — no path
# fix-up needed. For a release build, pin them to a tag/rev instead (see the Technology page).
```

> The README's "fix dependency paths" step is stale — the deps are git, not path. Leave them, or
> pin to a tag.

## Quickstart — prove the toolchain end to end

Point at a database, run the migrations, and run the tests.

```bash
# From the module directory:
export DATABASE_URL="postgresql://root:password@localhost:5432/skeletondb"

# 1. Validate the manufacturing schema (index + bom + work_order models).
metaphor schema schema validate

# 2. Apply the migrations (enums + the eight entity tables + audit triggers).
metaphor migration run

# 3. Run the module's tests (golden cases, integrity probes, the plant-to-produce seam).
metaphor dev test
```

Expected: validation passes; migrations report the `manufacturing` schema and its tables
(`workstations`, `operations`, `boms`, `bom_items`, `bom_operations`, `work_orders`,
`work_order_items`, `job_cards`) created; the test run is green.

To see the HTTP surface, compose the module into a service and `metaphor dev serve`, then create a
workstation (the simplest master-data entity — no line children):

```bash
curl -s -X POST localhost:8080/api/v1/workstations \
  -H 'content-type: application/json' \
  -d '{"companyId":"00000000-0000-0000-0000-000000000001","workstationName":"CNC-1","hourRate":120.00,"isActive":true}'
# → 201 { "id": "…", "workstationName": "CNC-1", "hourRate": …, "isActive": true,
#         "metadata": { "createdAt": "…" } }
```

The four fields (`companyId`, `workstationName`, `hourRate`, `isActive`) are exactly
`CreateWorkstationDto` — all required. Note the JSON is **camelCase** even though the Rust and SQL
are snake_case (the generated `#[serde(rename_all = "camelCase")]`; snake_case aliases are also
accepted). The `/api/v1`
prefix comes from the `routes::get_routes` composer; the module's own `all_crud_routes()` mounts the
same endpoints **unprefixed and unguarded** (see [Architecture §4](04-architecture.md#the-twelve-endpoints-for-free)).

> **Generic CRUD ≠ the domain path.** `POST /api/v1/boms` writes whatever `totalCost` you send — it
> does **not** roll up cost or run the WIP lifecycle. Cost roll-up and the Work Order verbs live in
> `ManufacturingWriteService`, not the generated handlers. See
> [The manufacturing domain](09-manufacturing-domain.md).

## Recipe — add a new entity to this module

Say you want a `Vendor` entity alongside the eight that ship.

```bash
# 1. Add the schema model. Either scaffold a stub…
metaphor make entity Vendor --module manufacturing
#    …or copy an existing model (e.g. schema/models/bom.model.yaml) → vendor.model.yaml, edit it,
#    then add `- vendor.model.yaml` under `imports:` in schema/models/index.model.yaml.

# 2. Validate, generate, migrate.
metaphor schema schema validate
metaphor schema schema generate --target all --force
metaphor migration generate Vendor manufacturing
metaphor migration run

# 3. Wire the service into ManufacturingModule in src/lib.rs (see the Maintainer Guide, step 6),
#    then test.
metaphor dev test
```

(`manufacturing` is the module name — auto-detected from the current directory when omitted, but
passing it explicitly is clearer in scripts.)

## Key concepts

Five ideas carry you the rest of the way. One line each; the linked page explains *why*.

- **Schema YAML is the source of truth.** You edit [`schema/models/*.model.yaml`](../schema/RULE_FORMAT_MODELS.md);
  the entity, DTOs, migration, repository, service, handler, and routes are generated from it.
  ([Philosophy](01-philosophy.md).)
- **A module is a library, not a service.** It has no `main.rs`. A `backend-service` composes it
  via `ManufacturingModule::builder().with_database(pool).build()?` and mounts its router —
  `module.all_crud_routes()` for the raw surface, or `routes::get_routes(&module)` for the
  `/api/v1`-prefixed form. Prefer a guarded composition for production.
  ([Architecture](04-architecture.md).)
- **Twelve endpoints come free per entity.** `BackboneCrudHandler` gives list / create / get /
  update / patch / soft_delete / restore / empty_trash / bulk_create / upsert / find_by_id /
  list_deleted, mounted under `/api/v1/<collection>`.
- **CRUD is inherited, not written.** `Service = GenericCrudService<…>` is a type alias;
  `Repository` is a newtype over `GenericCrudRepository`. You add methods, never a fresh `impl`.
  ([ADR-0002](adr/adr-0002-generic-crud.md).)
- **Custom code survives regeneration** if it sits in `// <<< CUSTOM` markers, `*_custom.rs` files,
  or a `user_owned` path. Anything else is overwritten by `generate --force`.
  ([ADR-0003](adr/adr-0003-custom-markers.md).)

## Recipes

### How do I add another entity to this module?

Follow the golden path in the [Maintainer Guide → Adding a new entity](05-maintainer-guide.md#adding-a-new-entity-the-golden-path),
or the recipe above. In short: add the `.model.yaml`, add it to `index.model.yaml` `imports:`,
`validate`, `generate`, `migration generate`, `migration run`, then register the service in
`ManufacturingModule` in `src/lib.rs`.

### How do I add a business rule (e.g. "a Work Order can't be received before it is released")?

Write it in a hand-authored write-service file, not in a generated `…_service.rs` alias — exactly as
`manufacturing_write_service.rs` does. Domain rules gate on status transitions:

```rust
// application/service/manufacturing_write_service.rs (the real file)
pub async fn receive_finished(&self, wo_id: Uuid, /* … */) -> Result<ReceiveOutcome, ManufacturingError> {
    let wo = self.load_wo(wo_id).await?;
    if wo.status != "in_process" {
        return Err(ManufacturingError::InvalidState("work order has no WIP to receive"));
    }
    // … drive inventory, post Dr FG · Cr WIP, gate the produced-qty advance …
}
```

Declare the file's `mod`/`pub use` in `application/service/mod.rs` under a `// <<< CUSTOM` marker so
regen keeps it. See [The manufacturing domain](09-manufacturing-domain.md) for the full write path.

### How do I add a non-CRUD endpoint?

Don't edit the generated handler. Add a handler fn in a `*_custom.rs`, compose it in `routes/`
beside the `BackboneCrudHandler` merge, and protect the file with a `user_owned` glob. Full steps:
[Maintainer Guide → Adding a non-CRUD endpoint](05-maintainer-guide.md#adding-a-non-crud-endpoint).

### How do I reference a user (or another module's entity)?

By **logical foreign key**, declared in the schema — never by copying the table in. This module
already does this for audit actors (and for its accounting/inventory/catalog references):

```yaml
# schema/models/index.model.yaml
external_imports:
  - module: sapiens
    types: [User]
# …
created_by:
  type: uuid?
  attributes: ["@foreign_key(sapiens.User.id)"]
```

### How do I seed sample data?

Edit the seeder in `src/seeders/`, then:

```bash
metaphor migration seed manufacturing          # run Rust seeders
metaphor migration generate-seeds manufacturing  # emit SQL seed files
```

## Configuration

Defaults live in [`config/application.yml`](../../config/application.yml); override per environment
and at runtime.

| Option | Default | When to change |
|--------|---------|----------------|
| `server.host` | `0.0.0.0` | Bind to a specific interface. |
| `server.port` | `8080` | Port conflicts / multi-service hosts. |
| `database.url` | `postgresql://root:password@localhost:5432/skeletondb` | **Always** in real deployments — override with the `DATABASE_URL` env var, which takes precedence. |
| `database.max_connections` | `10` | Tune to your Postgres pool budget. |
| `logging.level` | `info` | `debug`/`trace` when diagnosing; `warn` in noisy prod. |

Layered files: `application.yml` (base) → `application-dev.yml` / `application-prod.yml`
(overrides). `DATABASE_URL` in the environment always wins over the YAML.

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `backbone-schema: command not found` | Following the stale README | Use `metaphor schema schema …`. `backbone-schema` is not a separate binary here. |
| `metaphor migration run` can't connect | `DATABASE_URL` unset or Postgres down | `export DATABASE_URL=postgresql://…`; confirm Postgres is reachable. |
| My custom method vanished after regen | Code sat outside a protected region | Move it inside a `// <<< CUSTOM` marker, a `*_custom.rs` file, or a `user_owned` glob ([Maintainer Guide](05-maintainer-guide.md#regen-safety--the-rules-that-keep-your-logic-alive)). |
| New endpoint returns 404 | Route not composed, or service not registered | Merge the route in `routes/mod.rs`; register the service field in `ManufacturingModule` (`src/lib.rs`). |
| Endpoint 404s at `/api/v1/…` but works at `/…` | Mounted via `all_crud_routes()` (unprefixed) not `routes::get_routes` (which nests under `/api/v1`) | Pick the composer that matches the prefix you expect ([Architecture §4](04-architecture.md#the-twelve-endpoints-for-free)). |
| `type WorkOrderStatus not found` after adding an enum variant | Migration not regenerated / not applied | Regenerate, `metaphor migration generate`, `metaphor migration run`. |
| Schema change ignored | Edited generated Rust instead of the YAML | Revert the Rust, edit `schema/models/*.model.yaml`, regenerate. |
| JSON field names look wrong (`created_at` vs `createdAt`) | Expecting snake_case on the wire | DTOs are `camelCase` by design; snake_case is DB/Rust only. |

---

Next: [Contributing](07-contributing.md) to send a change back, or the
[Glossary](08-glossary.md) to pin down a term.
