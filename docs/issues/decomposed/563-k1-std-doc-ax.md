---
parent: 563
title: "Phase-K.1: `std/doc.ax` — documentation stdlib"
labels: [area:stdlib, roadmap, risk:low, status:needs-human-approval, lane:hermes, phase-k]
depends_on: [562-j3-no-rust-bootstrap]
---

Part of #563. Replace the Rust-implemented doc generator with an AxiOM module that reads `.ax` source and emits Markdown / HTML.

## Scope

- `std/doc.ax` exposes `extract_doc_comments(path: string): [DocItem]` and `render_markdown(items: [DocItem]): string`.
- `///` doc comments are parsed from `.ax` source files at the function, type, and module level.
- `axiomc doc` compiles `std/doc.ax` and uses it to render documentation for the requested package.

## Acceptance

- `axiomc doc stage1/examples/hello` produces the same Markdown output as the current Rust-side implementation.
- Snapshot test compares output bytes against a golden fixture.

## Depends on

- Phase-J.3 (the toolchain runs without Rust).

## Out of scope

- The JSON output mode — K.2.
- The Markdown output flag wiring — K.3.
