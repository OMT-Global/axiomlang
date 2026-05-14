---
parent: 216
title: "Traits A: trait declaration syntax (parse + HIR only)"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:daedalus]
---

Part of #216. Follows the static-dispatch slice in `docs/rfcs/0001-traits-bounded-generics.md`. Adds the trait declaration syntax to the parser and HIR. No bounded generics, no impl blocks, no dispatch yet.

## Scope

- New top-level form: `trait <Name> { fn <method>(...): <return>; … }`.
- Parser produces a `syntax::TraitDecl`; HIR lowers to a `hir::TraitDecl` with method signatures.
- Type checker rejects any use of trait names in type positions until 216-b lands.

## Acceptance

- Pass fixture: a `trait Display { fn render(self): string }` declaration parses.
- Negative fixture: using the trait name as a type — rejected with `trait dispatch is not yet implemented`.

## Out of scope

- Bounded generics — 216-b.
- Impl blocks — 216-c.
- Dynamic dispatch — out of the RFC's first slice.
