//! The module's extension (hand-written; ADR-0031): what the module adds beside its
//! generated services. The generated module and builder carry `ModuleExt` and
//! `ModuleBuilderExt` and dereference to them, so their fields read as `module.field`.

#[allow(unused_imports)]
use super::*;

/// State the module adds; read through the generated module's `Deref`.
pub struct ModuleExt {
    pub(crate) manufacturing_write_service:
        Arc<crate::application::service::ManufacturingWriteService>,
}

/// State the builder adds.
pub struct ModuleBuilderExt {
}

impl Default for ModuleBuilderExt {
    fn default() -> Self {
        Self {
        }
    }
}

impl ModuleBuilderExt {
    /// Build the extension's state from what the generated build made.
    #[allow(unused_variables, clippy::redundant_clone)]
    pub(crate) fn build(self, parts: &ModuleParts<'_>) -> anyhow::Result<ModuleExt> {
        let db_pool = parts.db_pool.clone();
        // The validated execution engine. It shares this module's pool (same RLS connection
        // story as the generic services); the GL/inventory ports are supplied per call by the
        // composing service via `write_api::ManufacturingWriteDeps`.
        let manufacturing_write_service = Arc::new(
            crate::application::service::ManufacturingWriteService::new(db_pool.clone()),
        );
        Ok(ModuleExt {
            manufacturing_write_service,
        })
    }
}

impl crate::ThisModule {
    /// The validated manufacturing execution engine (release → consume → operate → receive),
    /// which rolls up BOM cost and emits the three WIP/FG GL posts so a Work Order's WIP nets to
    /// zero on completion.
    ///
    /// This is **not** mounted by [`Self::all_crud_routes`] — that exposes *unguarded* generic CRUD
    /// that bypasses every manufacturing invariant (a `PATCH status→completed` there posts no GL).
    /// To serve validated writes over HTTP, compose the command router from [`write_api`]:
    ///
    /// ```ignore
    /// let m = ManufacturingModule::builder().with_database(pool).build()?;
    /// let deps = write_api::ManufacturingWriteDeps {
    ///     write_service: m.write_service(),
    ///     inventory: Arc::new(my_inventory_adapter),   // you own stock — supply a real InventoryPort
    ///     gl:        Arc::new(my_gl_adapter),          // you own the ledger — supply a real GlPostSink
    ///     events:    Arc::new(LoggingSink),            // or your bus sink
    /// };
    /// let app = Router::new()
    ///     .merge(/* your read routes */)
    ///     .merge(write_api::create_manufacturing_write_routes().with_state(deps));
    /// ```
    ///
    /// The ports are caller-supplied per call: a no-op `GlPostSink` silently drops the WIP postings
    /// (it compiles, it looks done, WIP leaks) — supply real adapters from the composing service.
    pub fn write_service(
        &self,
    ) -> Arc<crate::application::service::ManufacturingWriteService> {
        self.manufacturing_write_service.clone()
    }
}
