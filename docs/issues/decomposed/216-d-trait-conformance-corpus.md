---
parent: 216
title: "Traits D: conformance corpus for static-dispatch traits"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:ares]
depends_on: [216-c-impl-blocks-static-dispatch]
---

Part of #216. Lock in the static-dispatch trait behavior with positive and negative conformance fixtures.

## Scope

- `stage1/conformance/pass/trait_display_int`: int impls `Display`, generic `render` instantiated with int.
- `stage1/conformance/pass/trait_multibound`: function with `<T: A + B>` instantiated.
- `stage1/conformance/fail/trait_missing_impl`: generic called with a type that doesn't impl the bound.
- `stage1/conformance/fail/trait_duplicate_impl`: two impls of the same `(Trait, Type)` pair.
- Update `tests::conformance_corpus_reports_stable_results`.

## Depends on

- 216-c.
