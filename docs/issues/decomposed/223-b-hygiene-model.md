---
parent: 223
title: "Declarative macros B: hygiene model"
labels: [phase-a, area:lang, lane:daedalus]
depends_on: [223-a-pattern-syntax]
---

Part of #223. Implement the capture / shadowing rules so that a macro's interior identifiers don't accidentally bind to the call-site environment.

## Scope

- Each macro expansion produces a fresh `syntax_context` so that a `let` inside the macro body doesn't shadow a same-named binding at the call site.
- Captured identifiers (passed as `$ident` fragments) resolve in the call-site environment as expected.
- Diagnostics for hygiene violations point at both the macro definition and the call site.

## Acceptance

- Conformance pass fixture: a macro that introduces a local `let x = 0` doesn't shadow the caller's `x`.
- Conformance fail fixture: a macro that returns `$x + 1` requires the caller to provide `$x`.

## Depends on

- 223-a (pattern syntax).

## Out of scope

- Recursive macro depth cap — 223-c.
