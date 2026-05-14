---
parent: 218
title: "AG1.2a: mutable borrowed locals"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:daedalus, status:ready-for-agent]
---

Part of #218 / AG1.2. Add a first-class `&mut T` local binding for non-`Copy` values held in let-bindings. This is the smallest viable slice of mutable references — no slice payloads, no call boundaries, no projections.

## Scope

- Parser accepts `let r: &mut T = &mut value`.
- HIR records the borrow with `mutability = Mut` and the lexical region of the let-binding.
- Borrow checker forbids using `value` directly (read or write) while `r` is live.
- Reassignment through `*r = …` works for the supported scalar / aggregate types.
- Conformance pass fixture: write-through-mutable-local.
- Conformance fail fixture: use-after-borrow.

## Acceptance

- `axiomc check` accepts the pass fixture and rejects the fail fixture with a stable diagnostic.
- No regression in any existing borrow-check test.

## Out of scope

- Mutable borrows on slice payloads — AG1.2b.
- Mutable borrows passed at call boundaries — AG1.2c.
- Field/projection-rooted mutable borrows — AG1.2d.
- Double-mutable / mutable+shared rejection diagnostics — AG1.2e.

## Working rules

- Keep the slice narrow; do not pre-implement region/lifetime annotations.
- All cascade matches on `hir::BorrowKind` must compile cleanly.
