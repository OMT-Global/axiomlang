# Stage1 Language Issue Disposition

<!-- capability-ledger:v1 commands=29 stdlib_modules=34 stdlib_functions=299 capabilities=9 backend=cranelift -->

This document records the historical disposition for the broad Phase A language
issues opened as #216 through #225. It is an evidence guide, not current live
issue state or production closure: stage1 remains the Rust bootstrap compiler
described in [Stage1 bootstrap](stage1.md), and the detailed execution contract
remains [Stage1 Agent-Grade Compiler Plan](stage1-agent-grade-compiler.md).

The checked capability ledger owns current surface and evidence-tier facts.
Rows below preserve prior issue decisions as historical evidence only.

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
- Top-level scalar `const` declarations, named constants in match patterns,
  const-sized array lengths, and a purity-fenced `const fn` subset have landed.
  Module-scope `static` declarations cover scalar values, strings, and small
  tuples; broader compile-time evaluation remains outside the current contract.

## Closure Matrix

| Issue | Disposition | Rationale |
| --- | --- | --- |
| [#216](https://github.com/OMT-Global/axiomlang/issues/216) Traits / interfaces | Partial static surface; dynamic dispatch remains unsupported | Trait declarations, explicit bounds, local impls, bounded method calls, and the `std/traits.ax` `Eq` seed have landed. Coherence breadth, blanket impls, associated items, supertraits, and `dyn` dispatch remain outside the evidenced stage1 slice. |
| [#217](https://github.com/OMT-Global/axiomlang/issues/217) Generic type inference | Historical deferral; not in the qualified surface | The compiler deliberately uses explicit type arguments and monomorphized generics. Inference depends on a constraint model and eventual trait bounds, so any future work needs a focused inference contract after the trait decision. |
| [#218](https://github.com/OMT-Global/axiomlang/issues/218) Mutable references | Historical AG1 follow-up; full model unqualified | The current compiler has borrowed slices and some mutable-slice plumbing, but not the full exposed `&mut T` aliasing and lifetime contract described by the historical issue. |
| [#219](https://github.com/OMT-Global/axiomlang/issues/219) Explicit lifetimes | Partial implementation landed | Stage1 now accepts explicit lifetime annotations on borrowed slice and mutable borrowed slice function signatures, preserving them through project rewriting and using the return lifetime to restrict which borrowed parameters may feed a borrowed return. Broader lifetime parameters on aggregate declarations, traits, and the final syntax-versus-elision policy still need RFC follow-up. |
| [#220](https://github.com/OMT-Global/axiomlang/issues/220) Full numeric tower | Implementation slices landed | The numeric tower has been split into independently testable issue slices covering signed widths, unsigned widths, floats, explicit cross-width casts, and literal suffixes. `docs/stage1.md` records overflow and floating-point behavior, while focused unit tests and conformance fixtures cover the shipped surface. Keep future numeric work as separate follow-up issues instead of reopening the broad umbrella. |
| [#221](https://github.com/OMT-Global/axiomlang/issues/221) Owned `String` and borrowed `&str` | Historical deferral; string-view model unqualified | Stage1 has owned `string` and borrowed slices, but not a user-facing string view model. Any future work depends on the lifetime and reference decisions and must not be treated as qualified by this historical issue. |
| [#222](https://github.com/OMT-Global/axiomlang/issues/222) Const / static evaluation | Partial implementation landed | Top-level scalar `const`, imported public constants, const-sized array lengths, named match-pattern constants, and a purity-fenced `const fn` subset have landed. Module-scope `static` covers scalar values, strings, and small tuples; broader evaluation and aggregate statics remain outside the evidenced slice. |
| [#223](https://github.com/OMT-Global/axiomlang/issues/223) Declarative macros | Implemented for stage1 subset | Stage1 now parses top-level `macro` and compatibility `macro_rules!` definitions, expands macros before type-check, records expansion metadata in `axiomc check --json`, exposes macro symbols to inspection, applies introduced-local hygiene for macro-owned `let` bindings, supports repeated expression captures, and bounds recursive expansion with `--macro-recursion-limit`. Proc macros and token-tree rewriting remain out of scope. |
| [#224](https://github.com/OMT-Global/axiomlang/issues/224) Operator overloading | Unsupported beyond built-in operators | Stage1 has static trait declarations/bounds/impls and built-in lowering for arithmetic, comparisons, and indexing, but operators do not resolve through trait impls. Coherence and overload-resolution semantics require their own schema and evidence before this can move beyond `unsupported`. |
| [#225](https://github.com/OMT-Global/axiomlang/issues/225) Flow-sensitive type narrowing | Historical deferral; broader narrowing unqualified | Stage1 already narrows within match payload bindings for the supported statement-level `match` shape, but guard-driven flow analysis and impossible-arm warnings remain a broader type-system feature needing smaller contracts after predicate-purity and diagnostics policy are defined. |
| [#226](https://github.com/OMT-Global/axiomlang/issues/226) Dedicated borrow-check pass | Historical preparation; dedicated pass unqualified | `borrowck.rs` now owns shared borrow classification helpers, diagnostic codes, and a read-only type view, but HIR lowering still drives the ownership walk and binding state. A distinct borrow-check pass/IR, explicit region/lifetime step, and span-aware conflicting-borrow diagnostics remain outside the qualified surface. |

## Follow-Up Rule

When one of these broad items becomes implementation-ready, split it into one
or more issue-backed slices with:

- the stage1 milestone it advances;
- a concrete parser, HIR, MIR, codegen, or diagnostic surface;
- compile-pass and compile-fail coverage expectations; and
- the exact example or conformance fixture that proves the behavior.
