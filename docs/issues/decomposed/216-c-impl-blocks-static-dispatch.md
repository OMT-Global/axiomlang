---
parent: 216
title: "Traits C: impl blocks for static dispatch"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:daedalus]
depends_on: [216-b-explicit-generic-bounds]
---

Part of #216. Add `impl <Trait> for <Type> { … }` blocks that supply method bodies; resolve calls through monomorphization.

## Scope

- Parser accepts `impl Trait for Type { fn method(...) { … } }`.
- Each method body is type-checked against the trait signature.
- Bounded-generic calls monomorphize: each instantiation of the generic gets its own copy with the right `impl` baked in.
- Coherence is intentionally minimal at this stage — at most one impl per `(Trait, Type)` pair in the build; duplicates are rejected with a deterministic diagnostic.

## Acceptance

- Pass fixture: `impl Display for int { … }` + a generic `fn render<T: Display>(...)` instantiated with `int`.
- Fail fixture: two `impl Display for int` blocks in the same package.

## Depends on

- 216-b (bounded generics).

## Out of scope

- Dynamic dispatch (`dyn Trait`) — follow-up.
- Blanket impls — follow-up.
- Orphan rules across packages — follow-up.
