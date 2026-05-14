---
parent: 564
title: "Phase-L.2: `std/serdes.ax` — JSON serialization stdlib"
labels: [roadmap, area:lang, lane:daedalus, risk:high, status:needs-human-approval, phase-l]
depends_on: [561-i3-property-fn-first-class]
---

Part of #564. Provide `to_json` / `from_json` in AxiOM so that the LSP server (and any other AxiOM tooling) can speak JSON without a Rust dependency.

## Scope

- `std/serdes.ax` exposes `fn to_json(value: Map<string, Value>): string` and `fn from_json(text: string): Result<Value, ParseError>`.
- `Value` is an AxiOM-native discriminated union covering null / bool / int / float / string / array / object.
- Compiles through `axiomc check → build → run` and ships a parity property test set with the existing Rust JSON contract snapshots.

## Acceptance

- Round-trip property: `from_json(to_json(v)) == Ok(v)` holds for all `Value` shapes the strategy table can generate.
- Parser handles surrogate-pair escapes, scientific-notation numbers, and trailing whitespace exactly as `serde_json` does for the same inputs (parity fixture).

## Depends on

- Phase-I.3 (property runner).

## Out of scope

- Streaming JSON / pretty-printing options — follow-ups.
