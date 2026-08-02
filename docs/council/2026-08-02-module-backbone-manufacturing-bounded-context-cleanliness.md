<!--
Council run — date: 2026-08-02 | repo type: module | unit: backbone-manufacturing
focus: bounded-context-cleanliness
roster (seated): chair, skeptic, steelman, yagni-business (standing);
ddd-bounded-context, contract-seat (context, module);
domain-expert manufacturing (invited — encodes real-world domain rules).
Subagent seats: steelman → skeptic → chair. Context + yagni voiced in-context.
-->

# Council — module:backbone-manufacturing — focus: bounded-context-cleanliness

## Post-run corrections (2026-08-02, after acting on the recommendations)

Executing recommendation #1 surfaced two material corrections to this report. The original
deliberation is preserved below unchanged; read it through this lens.

1. **Finding #2 / Recommendation #2 (the "default mount" + phantom `my_validated_writes`
   docstring) targeted DEAD CODE.** `src/routes/` and `src/handlers/` were never declared as
   modules from the crate root — they were not compiled. The only *compiled* route surface is
   `ManufacturingModule::all_crud_routes()` / `routes()` in `lib.rs`, which already carries the
   `#[deprecated]` warning the council said was missing. The council read uncompiled template
   files as the live API. **Both dirs are now deleted.** Recommendation #2 is moot for the
   compiled crate.
2. **Finding #4 / Recommendation #4 (`ManufacturingQueryService` "dangling trait") is NOT a
   defect.** The generator (`metaphor-plugin-schema`, `export.rs:320`) deliberately emits an
   impl-less published read-contract — a half-impl struct was *removed* as "a lie that
   masqueraded as implemented." Consumers implement the trait against the repositories, or a
   real impl is hand-added in CUSTOM SERVICES. **No action; do not "fix" this.**
3. **The dead-code surface was far larger than the council saw.** Beyond `routes/`+`handlers/`,
   `application/{auth, bulk_operations, usecases, subscriptions}` were also never wired
   (`application/mod.rs` declares only `service/validator/workflows`) — ~45 uncompiled files
   total, all now deleted (commit `e2ffc27`).
4. **Open follow-up (not addressed):** the generator still *emits* this unwired scaffolding, so
   a future `metaphor schema` regen of manufacturing would re-create the deleted dead code.
   Making the cleanup durable requires a generator-level change (stop emitting layers the module
   template doesn't wire). Tracked as a follow-up.

### Recommendation status
| # | Original recommendation | Status |
|---|--------------------------|--------|
| 1 | Wire `ManufacturingWriteService` + guarded write scaffold + honest docstring | ✅ Done — `write_service()` accessor + new `write_api` module (commit `d6b7522`). The scaffold lives in a new compiled `write_api` module, not the dead `routes/mod.rs`. |
| 2 | Warn the default mount | ⬜ Moot — the cited mount was dead code (now deleted); the live `all_crud_routes()` was already deprecated. |
| 3 | Delete `example_*` scaffolding | ✅ Done — commit `e2ffc27`. |
| 4 | Resolve `ManufacturingQueryService` | ⬜ Not a defect — intentional published read-contract. No action. |

**Net:** the module's *structural* completeness (schema → all DDD layers → migrations) was real;
the actionable gap was the write engine being unwired (now fixed) plus a large volume of
uncompiled scaffolding polluting the repo (now removed). Two of four recommendations dissolved
on execution — the council over-read dead templates as live contracts.

## Best call

**Deliver the consumer-assembly contract the docstring already promises.** Wire `ManufacturingWriteService` into `ManufacturingModule` (it needs only the `PgPool` the builder already holds), ship a real `create_manufacturing_write_routes(&write_service, inventory: Arc<dyn InventoryPort>, gl: Arc<dyn GlPostSink>, sink: Arc<dyn ManufacturingEventSink>)` guarded-command router in `routes/mod.rs`, rename the misnamed generic-CRUD `create_*_write_routes` → `create_*_unguarded_write_routes`, and replace the phantom `.merge(my_validated_writes)` in the `routes/mod.rs:67` docstring with the real call. This is one coherent move — the rename, the scaffold, and the wiring are facets of "stop lying about a surface that doesn't exist."

Why this framing over "auto-wire the engine": the ports are per-call caller-supplied (`manufacturing_execution.rs:30,126`; `manufacturing_work_order.rs:56`), and the module owns no ledger or stock — auto-wiring with no-op ports would *silently drop GL posts*, which is strictly worse than today's bypassable state because it would look correct. Consumer-assembly is the architecturally correct shape; the module's sin is failing to make it real and honest.

- **Residual negative value:** ~3–5 hours to land (one `pub` field + builder line, one router scaffold forwarding to the 5 write-service commands, a rename pass, a docstring fix). After landing, every downstream `backend-service` still pays a one-time wiring cost (supply its real `PostingService`-backed `GlPostSink` + inventory adapter) — but that cost is paid once, in the place that owns those tables, instead of the current state where it is paid per-consumer by reverse-engineering `tests/plant_to_produce_seam.rs`. Residual risk surface: the scaffold ships without a real `GlPostSink`/`InventoryPort` impl in `src/` (correct — those belong to siblings), so a consumer who mounts the scaffold with a no-op sink still gets silent WIP leakage; this is irreducible at the module layer and must be documented on the scaffold's docstring.
- **Reversibility:** easy. The field, builder method, and scaffold are additive; the rename is a `pub` break but the functions are `#[deprecated]`-alias-able for one release.
- **What would flip this:** if a probe showed zero downstream consumers mount `create_stateless_routes`/`all_crud_routes`/`routes()` in practice (i.e., every consumer already hand-assembles via the test pattern), the urgency collapses to "fix the docstring + rename the misnomer" and the scaffold becomes a follow-up. Cheap probe: grep the `backend-service` repos for `create_stateless_routes` / `all_crud_routes` / `.routes()` call sites.

## Disagreement map

**1. "Complete" vs "incomplete" — what does completeness mean when the invariant engine needs sibling-owned ports?**
- Steelman: structurally complete and deliberately under-mounted; refuses to over-promise.
- Skeptic: incomplete because the engine is unreachable from any production route; the default surface permits the core accounting crime.
- **Crux:** the Steelman's load-bearing condition #3 ("the engine is reachable from the public surface") is FALSIFIED — confirmed at `src/lib.rs:63-74,144-195` (no write-service field), `routes/mod.rs:51-61,68-78` (no write-service invocation), and `bom_handler.rs:134-139` (generic `BackboneCrudHandler::write_routes`, zero `ManufacturingWriteService` calls). The Skeptic wins the fact. BUT the Skeptic overclaims if it implies the module should auto-wire real ports — it cannot (per-call port args, no in-`src/` impls). Adjudication: **structurally complete, consumer-contract incomplete.** The fix is making the consumer-assembly contract real, not auto-wiring.

**2. Published HTTP contract = storage representation vs gated aggregate.**
- Contract / DDD seats: the published surface is anemic CRUD over entity structs; siblings couple to storage shape and will break when mutations get gated.
- Steelman: generic CRUD is the trusted/admin/seeding surface; deprecation notes show mature stewardship.
- **Crux:** `create_stateless_routes` (the documented default at `lib.rs:55-61` and `routes/mod.rs:44-50`) and `all_crud_routes` carry NO warning, while only the `routes()` alias is `#[deprecated]`. The default mount is the crime path and it is the example in the docstring. The Contract seat wins.

**3. Delete `example_*` now vs harmless cruft.**
- YAGNI: delete; paid no value.
- Steelman: harmless, `cargo check` is clean.
- **Crux:** `application/dto/mod.rs:10` and `application/workflows/mod.rs:3` `pub use` the example types transitively into the published API. YAGNI wins — it's not internal cruft, it's public contract pollution.

**4. `ManufacturingQueryService` dangling trait.**
- Skeptic flagged no `impl` in `src/`; Contract seat says publish-or-delete.
- **Crux:** `exports/services.rs:23-96` declares a 24-method cross-context read trait; the CUSTOM block (`:102-104`) is empty; no `impl ManufacturingQueryService` exists in `src/`. Either ship a default impl over the 8 read repos, or `#[doc(hidden)]` it with a TODO. Both seats converge.

## Recommendations (ranked by leverage)

| # | Move | Leverage | Residual negative | Reversibility | Evidence to flip |
|---|------|----------|-------------------|---------------|------------------|
| 1 | **Deliver the consumer-assembly contract**: wire `ManufacturingWriteService` into `ManufacturingModule` + ship `create_manufacturing_write_routes(write_svc, inv, gl, sink)` guarded scaffold + rename `create_*_write_routes`→`create_*_unguarded_write_routes` + fix the phantom `my_validated_writes` docstring. | Closes the core accounting crime path (stranded WIP via mounted routes) AND the docstring lie in one move. Unblocks every downstream consumer from reverse-engineering the test assembly. | ~3–5h. Scaffold still needs consumer-supplied ports (architecturally correct, not a defect); document the no-op-sink footgun on the scaffold. | Easy (additive + `#[deprecated]` rename alias). | Probe shows zero consumers mount the default CRUD surfaces → shrink to docstring+rename only. |
| 2 | **Harden the default-mount warning**: apply the same `routes()`-style UNVALIDATED warning to `create_stateless_routes` and `all_crud_routes`, or make read-only the default and require explicit opt-in for unguarded writes. | The crime is available at the documented default mount point (`lib.rs:55-61`); warning there is the cheapest bleed-stop before #1 lands. | ~30min (warning) / ~2h (remove defaults + migration note). If defaults removed, admin/seeding consumers need a migration path. | Easy. | A consumer audit shows all real mounts already use read-only + custom writes. |
| 3 | **Delete or `#[doc(hidden)]` the `example_*` scaffolding** (`example_dto`, `example_saga_workflow`, and the entity/service/repo/handler/route/dto/feature siblings); remove the `pub use` at `application/dto/mod.rs:10` and `application/workflows/mod.rs:3`. | Removes generic `ExampleSagaFlowExecutor` from manufacturing's published API; shrinks the coupling surface siblings can accidentally depend on. | ~15min. If an external consumer imported `ExampleSagaFlowStatus`, they break on next bump. | Easy (re-add if anything breaks). | `cargo` reverse-dep check finds an external importer. |
| 4 | **Resolve `ManufacturingQueryService`**: ship a default impl over the 8 read repos (matching the write-side consumer-supplies-ports pattern), or `#[doc(hidden)]` it with a TODO. | Removes a 24-method dangling cross-context read contract from `exports/services.rs`. | ~30min (`#[doc(hidden)]`) / ~2h (default impl). None if hidden. | Easy. | A sibling module is already `impl`-ing it downstream → promote to real contract. |

## Parking lot

- **Third route-composition pattern** (`get_routes` / `get_routes_with_state` / `create_combined_routes` — three ways to mount before any consumer asked). Out of focus; revisit when a real consumer's mount shape is known. Candidate for collapse after #1 lands and the canonical shape is "read-only + guarded writes."
- **WIP-costing seam schema bipartition** (BOM-side 5 models vs WO-side 3 models). High-quality modeling, not a cleanliness issue; noted as a strength, not an action.
- **Cross-context FK softness** (`@exclude_from_foreign_key_check` → catalog.Product, accounting.Account, inventory.Warehouse, sapiens.User). Correct for a `module` crate; enforce at the `backend-service` composition layer, not here.
- **Golden-case test coverage** (`plant_to_produce_seam.rs`, `integrity_probes.rs`, `manufacturing_golden_cases.rs`) proves the engine works end-to-end against the real accounting ledger. Not a finding — it's the evidence that makes #1 safe to land.

---

**Relevant file paths (all repo-relative):**
- `src/lib.rs` — module struct `:63-74`, builder `:144-195`, `all_crud_routes`/`routes` `:87-118`.
- `src/routes/mod.rs` — phantom docstring `:67`, default unguarded mount `:51-61`.
- `src/presentation/http/bom_handler.rs` — misnamed `create_bom_write_routes` `:134-139`.
- `src/application/service/manufacturing_write_service.rs` — hub docstring `:1-21`, struct `:123-131`, `new(pool)` `:133-144`.
- `src/application/service/manufacturing_execution.rs` — `consume_materials:30`, `receive_finished:126` (per-call port args).
- `src/application/service/mod.rs` — `pub use` of write service `:53-56`.
- `src/exports/services.rs` — dangling `ManufacturingQueryService` trait `:23-96`, empty CUSTOM block `:102-104`.
- `tests/plant_to_produce_seam.rs` — canonical consumer assembly `:24-34`.
