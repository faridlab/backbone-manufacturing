# ADR-004: Unbuild, repair, and the subcontract mint — plain records, port legs, zero Cargo edges

Status: accepted · Date: 2026-08-27

## Context

Three surfaces the legacy module did not have: reversing a finished batch into its components,
repairing a broken item with part legs, and manufacturing that a supplier performs. The wrong
shape for all three is a mirrored work-order lifecycle — unbuilding has nothing to schedule,
repairs are not production, and a subcontract order is authored by a purchase, not by an operator.

## Decisions

**Unbuild** is a plain 2-state record (`draft → done`) against a **done** work order. The move is
ONE port call, `reverse_production`: FG leaves the finished estate and every component returns at
its share of the recovered value; the GL mirrors it exactly (`Dr Raw Σ · Cr FG Σ`). The source work
order is never written — it stays done, accumulators untouched; "how much has been unbuilt" is
summed from done unbuilds. Guards: `UnbuildSourceNotDone`, and `UnbuildOverRemaining` once the done
unbuilds have eaten the produced quantity (never a silent clip).

**Repair** is a verb chain `draft → (validate) confirmed → (start) under_repair → (end) done |
cancel` over three part-leg kinds (`add`/`remove`/`recycle`). No leg moves stock until `end_repair`,
which executes EVERY leg in one pass — each through `InventoryPort::execute_repair_leg`, idempotent
on `repair-leg:{repair}:{index}` — so a cancel before end needs no move cancellation, and a failed
leg leaves the order under_repair with the already-moved legs safe to retry. The GL is one grouped
balanced post: add `Dr Repair-Expense · Cr Raw`; remove `Dr Inventory-Loss · Cr Raw` (a scrap posts
to the loss ACCOUNT — no quarantine Location row exists anywhere in the family); recycle
`Dr Raw · Cr Repair-Expense`. `validate` probes add-leg availability read-only through the port
(advisory-checked, authority-held). `done` is uncancelable — its legs have moved real stock. Repair
billing is not this module's surface.

**Subcontract** orders are minted by buying's receipt EVENT (a serialized contract — zero Cargo
edge): when `order_kind == "subcontract"`, manufacturing mints the hidden MO once per purchase
order (`subcontract_mo_links` unique is the replay backstop; replays return the same id). The mint
is atomic — confirmed WO + exploded requirements + link row in ONE transaction; there is no
orphan-draft window — and the item's active BoM must be `bom_type='subcontract'` or the receipt is
an authoring defect, LOUD. The minted MO lands directly `confirmed`, numbered by the purchase
reference, warehouses NULL for the operator; inventory/ledger value rides the ordinary
consume/receive verbs (extra cost through the interim account). A non-subcontract receipt event
never mints (`SubcontractKindMismatch`). Zero buying writes, zero SVL.

## Consequences

- No mirrored lifecycle anywhere: three small state machines instead of one overloaded one.
- Port surface stays at five methods: issue_to_wip, receive_finished, reverse_production,
  execute_repair_leg, check_repair_availability (the probe is the one addition; the leg executor
  keeps repair from needing per-line inventory semantics).
- Double-delivery of the same purchase order cannot double-mint (link unique loses the race).

Anchors: `manufacturing_unbuild.rs`, `manufacturing_repair.rs`, `manufacturing_subcontract.rs`,
`manufacturing_ports.rs`, `tests/unbuild_golden_cases.rs`, `tests/repair_lifecycle_probes.rs`,
`tests/subcontract_golden_cases.rs`.
