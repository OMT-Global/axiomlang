---
parent: 230
title: "DWARF B: direct DWARF emission via the native backend"
labels: [phase-b, area:runtime, lane:daedalus]
depends_on: [105-b-mir-to-native, 230-a-sourcemap-layer]
---

Part of #230. Once the direct native backend (#105) is producing object files, emit DWARF debug info directly that maps back to AxiOM source — no Rust sourcemap intermediary.

## Scope

- Emit DWARF compile-units, line-info tables, and function records for every AxiOM function in the build.
- Line tables reference `.ax` source paths and line numbers directly.
- Variable scope DIEs cover let-bound locals (best-effort for the first pass).

## Acceptance

- `lldb` `b <function>` resolves AxiOM symbols and steps line-by-line through `.ax` source.
- `gdb` `info locals` shows let-bound locals with their AxiOM types.

## Depends on

- 105-b-mir-to-native (the direct backend handles full surface).
- 230-a-sourcemap-layer (interim coverage stays available until DWARF is at parity).

## Out of scope

- Symbol mangling stability across releases — separate spec.
