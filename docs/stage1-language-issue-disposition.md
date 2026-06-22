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
  `int` / `bool`, first-class signed, unsigned, and floating numeric widths,
  explicit numeric casts, suffixed numeric literals, top-level scalar `const`,
  statement-level `match`, `Option<T>` / `Result<T, E>`, package-local imports,
  local dependency graphs, capability-gated stdlib modules, and generated-Rust
  native builds.
- Stage1 still has no trait system, no generic type inference at call sites, no
  exposed lifetime parameter syntax, no `String` / `&str` split, and no
  operator overloading protocol. Declarative macro support is now limited to the
  stage1 `macro` / `macro_rules!` subset documented in [Grammar](grammar.md).
- Mutable borrowed slices exist in the current internal type/codegen surface,
  but the language still does not expose a complete `&mut T` reference model,
  reborrowing, or full lifetime checking.
- Top-level scalar `const` declarations with compile-time evaluation have
  landed, including const-sized array type lengths backed by int constants, and
  module-scope `static` declarations now cover scalar values, strings, and
  small tuples. `const fn` and named constants in match-pattern positions
  remain outside the current bootstrap contract.

## Closure Matrix

| Issue | Disposition | Rationale |
| --- | --- | --- |
| [#216](https://github.com/OMT-Global/axiomlang/issues/216) Traits / interfaces | Keep open until split or implemented | The issue bundles trait declarations, bounded generics, `dyn` dispatch, coherence, blanket impls, and stdlib traits. That is a Rust-parity epic, not an active AG0-AG5 stage1 slice; closure needs either implementation or an owner-approved split into independently testable work. Draft RFC `docs/rfcs/0001-traits-bounded-generics.md` now proposes that split, with static bounded generics before `dyn` dispatch. |
| [#217](https://github.com/OMT-Global/axiomlang/issues/217) Generic type inference | Keep open until scoped | The current AG2 contract deliberately uses explicit type arguments and monomorphized generics. Inference depends on a constraint model and eventual trait bounds, so implementation should wait for a focused inference RFC after the trait decision. |
| [#218](https://github.com/OMT-Global/axiomlang/issues/218) Mutable references | Keep open as active AG1 follow-up | This maps to AG1.2. The current compiler has borrowed slices and some mutable-slice plumbing, but not the full exposed `&mut T` aliasing and lifetime contract requested by the issue. |
| [#219](https://github.com/OMT-Global/axiomlang/issues/219) Explicit lifetimes | Partial implementation landed | Stage1 now accepts explicit lifetime annotations on borrowed slice and mutable borrowed slice function signatures, preserving them through project rewriting and using the return lifetime to restrict which borrowed parameters may feed a borrowed return. Broader lifetime parameters on aggregate declarations, traits, and the final syntax-versus-elision policy still need RFC follow-up. |
| [#220](https://github.com/OMT-Global/axiomlang/issues/220) Full numeric tower | Implementation slices landed | The numeric tower has been split into independently testable issue slices covering signed widths, unsigned widths, floats, explicit cross-width casts, and literal suffixes. `docs/stage1.md` records overflow and floating-point behavior, while focused unit tests and conformance fixtures cover the shipped surface. Keep future numeric work as separate follow-up issues instead of reopening the broad umbrella. |
| [#221](https://github.com/OMT-Global/axiomlang/issues/221) Owned `String` and borrowed `&str` | Keep open or split after lifetime policy | Stage1 has owned `string` and borrowed slices, but not a user-facing string view model. This depends on the lifetime and reference decisions, so it should remain deferred rather than be treated as complete. |
| [#222](https://github.com/OMT-Global/axiomlang/issues/222) Const / static evaluation | Keep open as partially landed | Top-level scalar `const` evaluation has landed, including local and imported public constants plus const-sized array lengths. Module-scope `static` now covers scalar values, strings, and small tuples. Match-pattern constants and `const fn` remain outside the current bootstrap contract, so the current evidence is partial only. |
| [#223](https://github.com/OMT-Global/axiomlang/issues/223) Declarative macros | Implemented for stage1 subset | Stage1 now parses top-level `macro` and compatibility `macro_rules!` definitions, expands macros before type-check, records expansion metadata in `axiomc check --json`, exposes macro symbols to inspection, applies introduced-local hygiene for macro-owned `let` bindings, supports repeated expression captures, and bounds recursive expansion with `--macro-recursion-limit`. Proc macros and token-tree rewriting remain out of scope. |
| [#224](https://github.com/OMT-Global/axiomlang/issues/224) Operator overloading | Blocked on #216 traits RFC/implementation | This issue asks for operator traits (`Add`, `Sub`, `Mul`, `Div`, `Eq`, `Ord`, and `Index`). Stage1 currently has built-in operator lowering for scalar/string addition, scalar comparisons, tuple indexing, array indexing, and map indexing, but no trait declaration, trait bound, impl-for-trait, coherence, or overload-resolution surface. Implementing this now would either hard-code ad hoc operator methods or pre-commit the trait design before #216 is settled, so it should remain open until the traits RFC/implementation supplies a stable semantic surface to test against. |
| [#225](https://github.com/OMT-Global/axiomlang/issues/225) Flow-sensitive type narrowing | Keep open until scoped | Stage1 already narrows within match payload bindings for the supported statement-level `match` shape, but the requested guard-driven flow analysis and impossible-arm warnings are a broader type-system feature. Split into smaller issues after predicate purity and diagnostics policy are defined. |
| [#226](https://github.com/OMT-Global/axiomlang/issues/226) Dedicated borrow-check pass | Keep open as partially prepared | `borrowck.rs` now owns shared borrow classification helpers, diagnostic codes, and a read-only type view, but HIR lowering still drives the ownership walk and binding state. Closing this issue still requires a distinct borrow-check pass/IR, an explicit region/lifetime step, and span-aware conflicting-borrow diagnostics from that pass. |

## Follow-Up Rule

When one of these broad items becomes implementation-ready, split it into one
or more issue-backed slices with:

- the stage1 milestone it advances;
- a concrete parser, HIR, MIR, codegen, or diagnostic surface;
- compile-pass and compile-fail coverage expectations; and
- the exact example or conformance fixture that proves the behavior.
