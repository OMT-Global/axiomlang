---
parent: 230
title: "DWARF A: sourcemap layer over rustc output (interim)"
labels: [phase-b, area:runtime, lane:daedalus]
---

Part of #230. While the generated-Rust backend is still default, emit a sidecar sourcemap so that lldb/gdb can attribute frames back to `.ax` source.

## Scope

- During codegen, write a `<binary>.axiom.map` JSON file alongside the build output containing `{ generated_rust_line → ax_path, ax_line, ax_column }` entries.
- Update `axiomc build --json` to include a `debug_map` field pointing at the sourcemap path.
- Document how to feed the sourcemap into common debuggers (lldb script, gdb python).

## Acceptance

- A debugger session attached to a built binary shows the AxiOM line for the current PC.
- Snapshot test compares the sourcemap JSON for a fixture against a golden file.

## Out of scope

- Direct DWARF emission — 230-b.
