---
parent: 560
title: "Phase-H.1: `property` clause in the type system"
labels: [area:stdlib, roadmap, lane:daedalus, risk:high, status:needs-human-approval, phase-h]
---

Part of #560. Add a `property` block to the type checker so that AxiOM can describe properties as a first-class language construct rather than a Rust library.

## Scope

- Recognize `property fn <name>(input: T): bool { … }` at parse time.
- Type-check the body in a scope that binds `input` and any free variables.
- The body must currently return `bool` (or be a chain of `assert_true` / `==` / `&&` / `||` / function-call expressions whose final value reduces to `bool`).
- `axiomc check` reports property errors with the offending failing input formatted in the existing diagnostic envelope.

## Acceptance

```axiom
// Type-checked at compile time
property fn reverse_double_returns_original(input: [int]): bool {
  assert_true(reverse(reverse(input)) == input)
}
```

- `axiomc check` accepts the program above.
- Replacing `==` with `!=` produces a deterministic compile-time property error that names the failing input shape.
- Conformance fixtures cover one passing property and one failing property.

## Out of scope

- `axiomc test --properties` runner — covered by H.3.
- `std/testing.ax` stdlib module — covered by H.2.
- Property generation / shrinking strategies — separate follow-up.
