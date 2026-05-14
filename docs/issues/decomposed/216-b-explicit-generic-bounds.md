---
parent: 216
title: "Traits B: explicit generic bounds on functions"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:daedalus]
depends_on: [216-a-trait-declaration-syntax]
---

Part of #216. Allow function signatures to declare `fn name<T: TraitName>(...)`. Bounded generics resolve to static dispatch only.

## Scope

- Parser accepts `<T: Trait>` and `<T: TraitA + TraitB>` bound syntax.
- Type checker records the bound on the generic parameter; uses it to validate that the type argument satisfies the trait (which it must, until 216-c lands, by having a previously declared impl — which 216-c will introduce).
- For now, only built-in trait stubs are usable; user impl blocks come in 216-c.

## Acceptance

- A generic function `fn render<T: Display>(value: T): string { value.render() }` typechecks against a built-in stub impl.
- Calling `render(42)` with no `Display` for `int` fails with a stable `trait bound not satisfied` diagnostic.

## Depends on

- 216-a.
