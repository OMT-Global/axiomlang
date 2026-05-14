---
parent: 564
title: "Phase-L.1: `std/lsp.ax` — LSP protocol stdlib"
labels: [roadmap, area:lang, lane:daedalus, risk:high, status:needs-human-approval, phase-l]
depends_on: [562-j3-no-rust-bootstrap, 564-l2-std-serdes-ax]
---

Part of #564. Implement the Language Server Protocol in AxiOM so that `axiomc lsp` does not depend on Rust crates.

## Scope

- `std/lsp.ax` reads JSON-RPC 2.0 frames from stdin and writes them to stdout.
- Supports `initialize`, `shutdown`, `exit`, `textDocument/didOpen`, `textDocument/didChange`, `textDocument/publishDiagnostics`, and `completion`.
- Diagnostics are surfaced from `axiomc check` invocations and translated into LSP `Diagnostic` records.

## Acceptance

- `axiomc lsp` (from L.3) exchanges an `initialize` / `initialized` handshake with a real LSP client (vscode-languageclient, helix, etc.) and reports diagnostics on file open.
- Property tests for the JSON-RPC framing live alongside `std/lsp.ax`.

## Depends on

- Phase-L.2 (`std/serdes.ax` must exist).
- Phase-J.3 (Rust bootstrap removed).

## Out of scope

- Hover / goto-definition / refactor — independent follow-ups.
