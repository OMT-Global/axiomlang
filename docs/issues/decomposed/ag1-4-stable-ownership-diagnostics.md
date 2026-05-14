---
title: "AG1.4: stable ownership error kinds in JSON diagnostics"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:daedalus, status:ready-for-agent]
parent: null
---

This issue captures the **AG1.4 work package** from `docs/stage1-agent-grade-compiler.md`, which currently has no dedicated GitHub issue. It is a child of the AG1 ownership milestone, but the AG1 umbrella issue numbers (#324–#332) only cover AG1.1–AG1.3.

## Goal

Stabilize the JSON diagnostic surface for ownership errors so downstream tooling (LSPs, agents) can rely on stable error codes when an ownership rule is violated.

## Scope

- Define stable `kind` / `code` pairs for the existing ownership diagnostics: `move-after-use`, `borrow-after-move`, `aliasing-mut-and-shared`, `double-mut`, `borrow-escapes-scope`, `mutate-shared-borrow`.
- Codes appear in the `axiom.stage1.v1` JSON envelope's `error.code` field.
- Update `docs/stage1.md` ownership section to list the codes.
- Lock a compile-fail corpus under `stage1/conformance/fail/ownership_*` that covers each code (some already exist; this issue closes the gap).

## Acceptance

- `axiomc check --json` on each fail fixture emits the expected `code` value byte-for-byte.
- Removing or renaming a code is rejected by a new contract test.
- Doc page lists every code with a one-line explanation and a sample diagnostic.

## Acceptance documents

- `docs/stage1.md` ownership section updated.
- `docs/stage1-agent-grade-compiler.md` updates the AG1.4 status to `landed` and links here.

## Working rules

- Do not invent new ownership rules. This is purely a stabilization pass over the existing rules.
- Codes should follow the existing `AX-OWN-NNN` shape if one exists, or `AX-BORROW-NNN` if a separate namespace is preferred — pick one and document it.
