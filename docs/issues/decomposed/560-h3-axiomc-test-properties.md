---
parent: 560
title: "Phase-H.3: `axiomc test --properties` runner flag"
labels: [area:tooling, roadmap, lane:daedalus, risk:high, status:needs-human-approval, phase-h]
depends_on: [560-h1-property-clause, 560-h2-std-testing]
---

Part of #560. Add `--properties` to `axiomc test` so it discovers `property fn` clauses, iterates the input domain, and reports failing inputs.

## Scope

- `axiomc test --properties <path>` walks the package's `.ax` sources, locates every `property fn` declaration, and runs each one.
- For each property, generate or iterate inputs from a small built-in strategy table (int / bool / string / [int]); fail loudly when the property's input type isn't yet supported by the strategy table.
- Report results through the existing `axiom.stage1.v1` JSON envelope (a new `properties` field per the schema rules).
- Exit non-zero when any property fails; print the failing input and the property's source span.

## Acceptance

- `axiomc test --properties stage1/examples/property_smoke` reports `1/1 properties passed` for a passing property.
- The same command on a deliberately broken property reports the exact failing input.
- The public v1 schema is updated to declare the new fields (or the property report uses its own schema_version).

## Depends on

- Phase-H.1 (property clause) and Phase-H.2 (std/testing.ax).

## Out of scope

- Shrinking / minimizing failing inputs — separate enhancement.
- Replacing `cargo test` for the stdlib — that's Phase-I.
