# Manufacturing — Golden Cases (the numeric oracle)

Mirrors `tests/manufacturing_golden_cases.rs`, `tests/integrity_probes.rs`, and the seam in
`tests/plant_to_produce_seam.rs`. Money is exact IDR (2dp, half-away-from-zero).

## Write path (`tests/manufacturing_golden_cases.rs`)
| Case | Input | Expected |
|------|-------|----------|
| **MGC-1** | BOM: 2×10@500 + ... ; ops 30min@120 + 15min@80 | raw `6,000`, operating `80`, total `6,080`. |
| **MGC-2** | WO qty 5 vs BOM qty 1, component qty 2 | required `10`. |
| **MGC-3** | empty BOM / WO qty 0 | `invalid`. |
| **MGC-4** (council) | Chair BOM with a PHANTOM Frame (=4 legs+8 dowels) + real Cushion; WO qty 2 | required legs `8`, dowels `16`, cushion `2`; the Frame item itself **never** required (exploded through). |

## Integrity probes (`tests/integrity_probes.rs`)
| Case | Input | Expected |
|------|-------|----------|
| **IP-1** | receive 6 on a WO of 5 | `over_produce`. |
| **IP-2** | consume twice | 2nd short-circuits; WIP charged **once**. |
| **IP-3** | receive full twice | 2nd short-circuits on completed; FG received **once**. |
| **IP-4** | consume with no GL accounts | `missing_account`; **no post, no stock move**. |
| **IP-5** | complete a job card twice | conversion charged to WIP **once**. |

## Plant-to-produce seam (`tests/plant_to_produce_seam.rs` + `scripts/plant_to_produce_seam_roundtrip.sh`)
| Case | Input | Expected |
|------|-------|----------|
| **PTPSEAM-1** | BOM 2×X@500 + 4×Y@250; WO qty 1; job card 30min@120; receive 1 | consume `Dr WIP 2,000·Cr Raw 2,000`; operate `Dr WIP 60·Cr Conv 60`; receive `Dr FG 2,060·Cr WIP 2,060`. **WIP = 0**, Raw −2,000, Conv −60, FG +2,060. Inventory: X 100→98, Y 100→96, FG +1 @ 2,060. Zero normal Cargo edge. |
| **§5 round-trip** | regen `--force`, re-run | seam files byte-identical; all green. |

## Conventions
- Manufacturing posts **WIP/FG only** — a Work Order's three posts net WIP to zero on completion.
- Material value comes from the inventory port (moving-average), not the BOM roll-up.
- Bounded (no over-produce) + transition-gated idempotent posts.
