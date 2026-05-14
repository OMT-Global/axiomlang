---
parent: 564
title: "Phase-L.3: `axiomc lsp` driver is AxiOM-only"
labels: [roadmap, area:lang, lane:daedalus, risk:high, status:needs-human-approval, phase-l]
depends_on: [564-l1-std-lsp-ax, 564-l2-std-serdes-ax]
---

Part of #564. Wire up `axiomc lsp` as a thin entrypoint that compiles `std/lsp.ax` to a native binary and re-runs `axiomc check` on file events.

## Scope

- `axiomc lsp` shells out to (or links in) the AxiOM-compiled LSP loop.
- File-open events trigger `axiomc check`; the resulting JSON envelope is translated into LSP diagnostics.
- File-change events debounce check invocations and emit `textDocument/publishDiagnostics` per package.
- Rust is no longer involved in the LSP path.

## Acceptance

- A reference LSP client (an in-tree test harness) opens a `.ax` file with a deliberate type error and observes the diagnostic appear within a deterministic timeout.
- `axiomc lsp` exits cleanly on `shutdown` + `exit` messages.

## Depends on

- L.1 (LSP stdlib) and L.2 (serdes stdlib).

## Out of scope

- Editor extensions / VS Code / Helix configs — separate work.
