# ADR-002: Hybrid state convergence — direct writes and derived states

Status: accepted · Date: 2026-08-27 · Supersedes the 5-state hand-gated lifecycle of ADR-001's era

## Context

The legacy `work_order_status` (`draft/released/in_process/completed/stopped`) mixed two different
kinds of transition in one enum: operator *decisions* (release, stop) and *accumulated facts*
(materials have moved, output exists). `stopped` was unreachable in code and in every gate. Job
cards carried a 2-state `open/completed` vocabulary too coarse for the shop floor. Every consumer
had to reconstruct "how far along is this order?" from accumulators anyway.

## Decision

The work-order state vocabulary becomes a **hybrid** with exactly three direct writes and three
derived states:

- **Direct writes (verbs):** `confirm` (draft → confirmed — the once-only explosion gate),
  `cancel` (draft|confirmed → cancel — sticky, terminal; refused once ANY consume or receive has
  happened, because real stock has moved), and done-by-receive (below).
- **Derived (by gates on accumulators):** `progress` is derived by the consume gate; `to_close` and
  `done` are derived by the receive accumulator's CASE (`produced_qty ≥ quantity` → done, partial →
  to_close). No verb writes progress/to_close/done directly.

Job cards converge to the shop-floor 5-state `ready/progress/done/cancel/blocked`
(`open → ready`, `completed → done` at migration; the default is `ready`).

Availability is a READ-SIDE projection only: `work_orders.reservation_state`
(`waiting/confirmed/assigned`). **No field named `availability` exists anywhere in the module** —
availability as a stored boolean was the old design's most misleading field, because it answered
neither "reserved by whom" nor "pickable now".

The migration replaces both enums (Postgres cannot drop values), maps
`released → confirmed`, `in_process → progress`, `completed → done`, and **raises** if any row
carries `stopped` — dirty data refuses loudly, never a silent remap.

## Consequences

- Status is always answerable from the row alone: an operator decision is a direct verb; everything
  else is what has physically happened.
- Cancel-after-WIP is structurally impossible (state guard), not just discouraged.
- The write API exposes no `release` route and no state-writing verb for the derived states; the
  receive/consume gates are their only authors.

Anchors: `migrations/20260901110000_work_order_state_convergence.up.sql`,
`tests/state_convergence_probes.rs`, handbook §"Execution".
