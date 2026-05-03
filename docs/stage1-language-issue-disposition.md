# Stage1 Language Issue Disposition

This document records the current disposition for the broad Phase A language
issues opened as #216 through #225. It is intentionally a status and closure
guide, not an implementation claim: stage1 remains the Rust bootstrap compiler
described in [Stage1 bootstrap](stage1.md), and the detailed execution contract
remains [Stage1 Agent-Grade Compiler Plan](stage1-agent-grade-compiler.md).

The agent-grade compiler bar is service and agent usability through `axiomc`,
not Rust parity. Large language parity tracks should stay open until they have
either a shipped implementation or an issue-backed implementation slice with
testable acceptance criteria. Status documentation can narrow scope, but it is
not sufficient closure evidence by itself.

## Current Evidence

- Stage1 already supports explicit generic functions, generic structs and
  enums, borrowed slices, aggregate borrowed returns, owned `string`, scalar
  `int` / `bool`, top-level scalar `const`, statement-level `match`,
  `Option<T>` / `Result<T, E>`, package-local imports, local dependency graphs,
  capability-gated stdlib modules, and generated-Rust native builds.
- Stage1 still has no trait system, no generic type inference at call sites, no
  exposed lifetime parameter syntax, no `String` / `&str` split, no declarative
  macro expander, and no operator overloading protocol.
- Mutable borrowed slices exist in the current internal type/codegen surface,
  but the language still does not expose a complete `&mut T` reference model,
  reborrowing, or full lifetime checking.
- Top-level scalar `const` declarations with compile-time evaluation have
  landed, but `static`, `const fn`, const-sized arrays, and named constants in
  match-pattern positions remain outside the current bootstrap contract.

## Closure Matrix

| Issue | Disposition | Rationale |
| --- | --- | --- |
| [#216](https://github.com/OMT-Global/axiom/issues/216) Traits / interfaces | Keep open until split or implemented | The issue bundles trait declarations, bounded generics, `dyn` dispatch, coherence, blanket impls, and stdlib traits. That is a Rust-parity epic, not an active AG0-AG5 stage1 slice; closure needs either implementation or an owner-approved split into independently testable work. |
| [#217](https://github.com/OMT-Global/axiom/issues/217) Generic type inference | Keep open until scoped | The current AG2 contract deliberately uses explicit type arguments and monomorphized generics. Inference depends on a constraint model and eventual trait bounds, so implementation should wait for a focused inference RFC after the trait decision. |
| [#218](https://github.com/OMT-Global/axiom/issues/218) Mutable references | Keep open as active AG1 follow-up | This maps to AG1.2. The current compiler has borrowed slices and some mutable-slice plumbing, but not the full exposed `&mut T` aliasing and lifetime contract requested by the issue. |
| [#219](https://github.com/OMT-Global/axiom/issues/219) Explicit lifetimes | Keep open pending RFC | Stage1 currently tracks borrow provenance for the supported borrowed-slice shapes without exposed lifetime syntax. The issue itself names an open design question; implementation should wait for the syntax-versus-elision choice to be decided by RFC. |
| [#220](https://github.com/OMT-Global/axiom/issues/220) Full numeric tower | Keep open or split | The current language still only exposes scalar `int` and `bool` as first-class numerics. This is real product work, but the issue should be split before implementation into integer widths, float support, casts, overflow policy, and literal suffixes. |
| [#221](https://github.com/OMT-Global/axiom/issues/221) Owned `String` and borrowed `&str` | Keep open or split after lifetime policy | Stage1 has owned `string` and borrowed slices, but not a user-facing string view model. This depends on the lifetime and reference decisions, so it should remain deferred rather than be treated as complete. |
| [#222](https://github.com/OMT-Global/axiom/issues/222) Const / static evaluation | Keep open as partially landed | Top-level scalar `const` evaluation has landed, including local and imported public constants. The issue acceptance also requires const-sized arrays and match arms, and the scope includes `static` and `const fn`, so the current evidence is partial only. |
| [#223](https://github.com/OMT-Global/axiom/issues/223) Declarative macros | Keep open pending RFC | Macros are explicitly beyond the agent-grade compiler bar. They need an RFC covering hygiene and expansion boundaries before implementation issues are useful. |
| [#224](https://github.com/OMT-Global/axiom/issues/224) Operator overloading | Blocked on #216 traits RFC/implementation | This issue asks for operator traits (`Add`, `Sub`, `Mul`, `Div`, `Eq`, `Ord`, and `Index`). Stage1 currently has built-in operator lowering for scalar/string addition, scalar comparisons, tuple indexing, array indexing, and map indexing, but no trait declaration, trait bound, impl-for-trait, coherence, or overload-resolution surface. Implementing this now would either hard-code ad hoc operator methods or pre-commit the trait design before #216 is settled, so it should remain open until the traits RFC/implementation supplies a stable semantic surface to test against. |
| [#225](https://github.com/OMT-Global/axiom/issues/225) Flow-sensitive type narrowing | Keep open until scoped | Stage1 already narrows within match payload bindings for the supported statement-level `match` shape, but the requested guard-driven flow analysis and impossible-arm warnings are a broader type-system feature. Split into smaller issues after predicate purity and diagnostics policy are defined. |

## Follow-Up Rule

When one of these broad items becomes implementation-ready, split it into one
or more issue-backed slices with:

- the stage1 milestone it advances;
- a concrete parser, HIR, MIR, codegen, or diagnostic surface;
- compile-pass and compile-fail coverage expectations; and
- the exact example or conformance fixture that proves the behavior.
