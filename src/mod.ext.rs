// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

// Hand-written validated command surface (`src/write_api.rs` is declared under
// `user_owned` in metaphor.codegen.yaml). Declared inside this marker so schema
// regeneration preserves the declaration alongside the generated module list.
pub mod write_api;
// The hand-owned request-pool shim (the composing service's tenant pool
// resolution): a generated-tree declaration the regenerator drops, so it
// lives in the preserved block (#447 cause-2 class).
pub mod request_pool;
