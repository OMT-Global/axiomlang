---
parent: 223
title: "Declarative macros A: pattern-matching macro definition syntax"
labels: [phase-a, area:lang, lane:daedalus]
---

Part of #223. Add the surface syntax for declarative (`macro_rules!`-style) macros and parse-time recognition. No expansion yet.

## Scope

- New top-level form: `macro <name> { (<pattern>) => (<body>) ; (<pattern>) => (<body>) ; … }`.
- Pattern fragments cover `$ident`, `$expr`, `$ty`, and repetition (`$( … )*`).
- Parser produces a `syntax::MacroDecl` node; HIR lowering treats every macro use as a stub `Expr::MacroCall` that the type checker rejects until 223-d wires expansion in.

## Acceptance

- A macro declaration parses without error and is visible in `axiomc check --json` symbol output.
- A program that uses a macro fails check with `macro expansion is not yet implemented` (deterministic, replaced when 223-d lands).
- Negative fixtures for malformed macro syntax.

## Out of scope

- Hygiene — 223-b.
- Recursive expansion — 223-c.
- Wiring into `axiomc check` — 223-d.
