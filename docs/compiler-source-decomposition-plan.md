# Compiler Source Decomposition Plan

This plan turns the self-hosting source-layout boundary in
[AxiOM Compiler Source Layout and Self-Hosting Boundary](axiom-compiler-source-layout.md)
into a measurable migration plan for the largest Rust-hosted compiler files.
It is the first remediation slice for #1162 and supports the Rust-exit
bootstrap path by making compiler source ownership smaller, reviewable, and
package-aligned before AxiOM-owned packages replace the Rust host.

The plan does not make Rust module paths canonical. Rust files are migration
references only; the future ownership boundary remains the AxiOM package map in
`docs/axiom-compiler-source-layout.md`.

## Measurement

Generate the current advisory report with:

```bash
make stage1-compiler-source-monoliths
```

Validate that this plan still covers the current largest files with:

```bash
make stage1-compiler-source-monoliths-test
```

The report records:

- total hand-written Rust lines under `stage1/crates/axiomc/src`;
- the largest single compiler file;
- the total line count for the top files;
- the top-file share of the compiler source tree;
- the self-hosted package boundary each large file must move toward.

This is now a ratcheted gate. The target is that the largest files and
top-seven file share move downward release over release as child extraction PRs
land. The current ceilings are the maximum allowed values; extraction PRs should
lower the relevant ceiling in this plan when they remove lines from a tracked
monolith.

The `lib.rs` test-module extraction lowered the absolute top-seven source line
count. It also raised the top-seven share because test code left
`stage1/crates/axiomc/src`, reducing the source denominator. The HIR generic
analysis extraction then split generic inference and monomorphization into a
tracked `compiler.hir` module. The HIR model extraction then moved public HIR
types and type-display helpers into `stage1/crates/axiomc/src/hir/model.rs`,
and the HIR type-lowering extraction moved syntax-to-HIR literal, type, and
operator lowering into `stage1/crates/axiomc/src/hir/types.rs`. The HIR
definitions extraction then moved type-name collection, aggregate definitions,
trait type-use validation, and recursive aggregate checks into
`stage1/crates/axiomc/src/hir/definitions.rs`. The HIR signature extraction
then moved function/method signature collection, trait impl signature
validation, and HIR symbol-name resolution into
`stage1/crates/axiomc/src/hir/signatures.rs`. The HIR capability extraction
then moved FFI validation, capability checks, network/process allowlist
validation, and capability-focused tests into
`stage1/crates/axiomc/src/hir/capabilities.rs`. The HIR expression typing
extraction then moved numeric type predicates, method-owner typing, string
borrow coercion, binary-add typing, and static expression value helpers into
`stage1/crates/axiomc/src/hir/expressions.rs`. The HIR ownership extraction
then moved move/projection checks, borrow-region origin tracing, borrowed-slice
type detection, and active borrow counters into
`stage1/crates/axiomc/src/hir/ownership.rs`. The HIR property extraction then
moved property signature validation, static verdict detection, and property
diagnostic sample/help text into `stage1/crates/axiomc/src/hir/properties.rs`,
lowering both absolute top-file lines and share. The HIR reachability
extraction then moved stdlib reachability and call-graph discovery into
`stage1/crates/axiomc/src/hir/reachability.rs`, further lowering the main HIR
facade without making the Rust helper layout canonical. The HIR diagnostic
recovery extraction then moved primary/related diagnostic selection, flattening,
and deterministic sorting into `stage1/crates/axiomc/src/hir/diagnostics.rs`.
The HIR symbol extraction then moved monomorphized symbol naming and async or
collection intrinsic classification into `stage1/crates/axiomc/src/hir/symbols.rs`.
The HIR boundary-test extraction then moved the inline HIR lowering regression
module out to `stage1/crates/axiomc/tests/hir_unit.rs`, keeping the private
module boundary while shrinking the Rust-hosted HIR facade.
The HIR source-location extraction then moved syntax statement/expression span
accessors into `stage1/crates/axiomc/src/hir/source_locations.rs`, further
shrinking the facade while keeping span logic inside the `compiler.hir`
boundary.
The HIR control-flow extraction then moved return-flow analysis into
`stage1/crates/axiomc/src/hir/control_flow.rs`, keeping block return
classification inside the `compiler.hir` boundary while further shrinking the
facade.
The HIR const-array extraction then moved const integer evaluation and const
array length validation into `stage1/crates/axiomc/src/hir/const_arrays.rs`,
keeping compile-time array shape checks inside the `compiler.hir` boundary.
The HIR match-lowering extraction then moved enum/const match statement and
match expression lowering into `stage1/crates/axiomc/src/hir/matches.rs`,
keeping pattern validation and match-arm borrow handling inside the
`compiler.hir` boundary.
The HIR variant-constructor extraction then moved enum variant resolution and
positional/named payload constructor lowering into
`stage1/crates/axiomc/src/hir/variants.rs`, keeping variant payload validation
inside the `compiler.hir` boundary.
The HIR async-runtime extraction then moved async runtime intrinsic capability
and type lowering into `stage1/crates/axiomc/src/hir/async_runtime.rs`,
keeping async capability validation inside the `compiler.hir` boundary.
The HIR map-intrinsic extraction then moved map lookup/key/default intrinsic
type and ownership lowering into `stage1/crates/axiomc/src/hir/maps.rs`,
keeping collection intrinsic validation inside the `compiler.hir` boundary.
The HIR const-function extraction then moved const function body and expression
validation into `stage1/crates/axiomc/src/hir/const_functions.rs`, keeping
compile-time function restrictions inside the `compiler.hir` boundary.
The direct-native runtime-serving stack then raised the native backend baseline
before this ratchet merged; the ceilings below reflect that post-merge snapshot
so future backend growth must be paid down or accompanied by an explicit
ratchet update.

The cranelift intrinsics extraction then moved the pure runtime-intrinsic
implementations (JSON scalar parse/stringify, the stage1-safe regex engine,
percent encoding, and the crypto primitives) into
`stage1/crates/axiomc/src/cranelift_backend/intrinsics.rs`, lowering the
`cranelift_backend.rs` ceiling below its pre-language-slice level. The small
ceiling raises for `codegen.rs`, `hir.rs`, `hir/matches.rs`, `hir/variants.rs`,
`main.rs`, `project.rs`, and `syntax.rs` record feature growth from merged
work (#1355-#1376 era) that landed while the ratchet was still advisory, which landed while
the ratchet was still advisory; the ratchet now runs in the fast PR lane via
`run-fast-checks.sh`, so future growth fails CI unless the ceiling change is
explicit in the same PR.

## Current Top Files

Snapshot from 2026-07-02:

| Rank | Current Rust file | Lines | Target package boundary | First extraction slice |
| ---: | --- | ---: | --- | --- |
| 1 | `stage1/crates/axiomc/src/cranelift_backend.rs` | 27,815 | `compiler.backend.native` | Pure runtime-intrinsic implementations now live in `stage1/crates/axiomc/src/cranelift_backend/intrinsics.rs`; continue splitting by runtime ABI groups: scalar/aggregate value features, capability shims, host imports, object emission, unsupported diagnostics, and evidence helpers. |
| 2 | `stage1/crates/axiomc/src/project.rs` | 11,250 | `compiler.package_graph`, `compiler.commands`, `compiler.evidence` | Split manifest/workspace loading, command orchestration, provenance/debug records, and build artifact planning along package ownership. |
| 3 | `stage1/crates/axiomc/src/main.rs` | 10,695 | `compiler.commands` | Move command parsing, JSON envelope construction, check/build/run/test/doc/trace orchestration, and exit handling behind `docs/compiler-command-lsp-packages.md` APIs. |
| 4 | `stage1/crates/axiomc/src/codegen.rs` | 7,882 | `compiler.backend.generated_rust`, `compiler.backend.contracts` | Isolate generated-Rust compatibility emission from backend target selection and unsupported-feature contracts. |
| 5 | `stage1/crates/axiomc/src/syntax.rs` | 6,324 | `compiler.syntax`, `compiler.diagnostics` | Split lexer/parser, parse recovery, source spans, macros, and syntax diagnostics behind the syntax boundary. |
| 6 | `stage1/crates/axiomc/src/hir.rs` | 5,842 | `compiler.hir` | Generic inference and monomorphization now live in `stage1/crates/axiomc/src/hir/generics.rs`; public HIR model types now live in `stage1/crates/axiomc/src/hir/model.rs`; syntax-to-HIR type/literal lowering now lives in `stage1/crates/axiomc/src/hir/types.rs`; type-name, aggregate, and trait-use definition checks now live in `stage1/crates/axiomc/src/hir/definitions.rs`; function/method signatures and trait impl signature validation now live in `stage1/crates/axiomc/src/hir/signatures.rs`; capability analysis now lives in `stage1/crates/axiomc/src/hir/capabilities.rs`; expression typing helpers now live in `stage1/crates/axiomc/src/hir/expressions.rs`; ownership and borrow-state helpers now live in `stage1/crates/axiomc/src/hir/ownership.rs`; property clause checks now live in `stage1/crates/axiomc/src/hir/properties.rs`; reachability/call-graph discovery now lives in `stage1/crates/axiomc/src/hir/reachability.rs`; diagnostic recovery helpers now live in `stage1/crates/axiomc/src/hir/diagnostics.rs`; monomorphized symbol and intrinsic helpers now live in `stage1/crates/axiomc/src/hir/symbols.rs`; source-location helpers now live in `stage1/crates/axiomc/src/hir/source_locations.rs`; return-flow analysis now lives in `stage1/crates/axiomc/src/hir/control_flow.rs`; const-array length validation now lives in `stage1/crates/axiomc/src/hir/const_arrays.rs`; const-function validation now lives in `stage1/crates/axiomc/src/hir/const_functions.rs`; match lowering now lives in `stage1/crates/axiomc/src/hir/matches.rs`; enum variant constructor helpers now live in `stage1/crates/axiomc/src/hir/variants.rs`; async runtime intrinsic lowering now lives in `stage1/crates/axiomc/src/hir/async_runtime.rs`; map intrinsic lowering now lives in `stage1/crates/axiomc/src/hir/maps.rs`; HIR boundary regression tests now live in `stage1/crates/axiomc/tests/hir_unit.rs`; continue splitting remaining HIR helper clusters behind the package APIs in `docs/compiler-hir-ownership-capability.md`. |
| 7 | `stage1/crates/axiomc/src/hir/generics.rs` | 4,208 | `compiler.hir` | Keep generic call inference, trait-bound validation, aggregate monomorphization, and generic call rewriting isolated from the main HIR lowering facade. |

## Ratchet Ceilings

These ceilings are consumed by
`scripts/ci/report-compiler-source-monoliths.py --check-ratchet`. A PR that
adds lines above any ceiling fails `make stage1-compiler-source-monoliths`.
When an extraction PR shrinks a tracked monolith or top-file share, lower the
matching ceiling in this table in the same PR.

| Tracked item | Ceiling |
| --- | ---: |
| `summary.top_file_line_share` | 0.8237 |
| `summary.top_file_lines` | 74316 |
| `stage1/crates/axiomc/src/cranelift_backend.rs` | 27815 |
| `stage1/crates/axiomc/src/cranelift_backend/intrinsics.rs` | 917 |
| `stage1/crates/axiomc/src/hir.rs` | 5848 |
| `stage1/crates/axiomc/src/project.rs` | 11396 |
| `stage1/crates/axiomc/src/main.rs` | 10755 |
| `stage1/crates/axiomc/src/codegen.rs` | 7897 |
| `stage1/crates/axiomc/src/syntax.rs` | 6372 |
| `stage1/crates/axiomc/src/hir/async_runtime.rs` | 188 |
| `stage1/crates/axiomc/src/hir/capabilities.rs` | 773 |
| `stage1/crates/axiomc/src/hir/const_arrays.rs` | 330 |
| `stage1/crates/axiomc/src/hir/const_functions.rs` | 117 |
| `stage1/crates/axiomc/src/hir/control_flow.rs` | 36 |
| `stage1/crates/axiomc/src/hir/definitions.rs` | 684 |
| `stage1/crates/axiomc/src/hir/diagnostics.rs` | 28 |
| `stage1/crates/axiomc/src/hir/expressions.rs` | 205 |
| `stage1/crates/axiomc/src/hir/generics.rs` | 4208 |
| `stage1/crates/axiomc/src/hir/maps.rs` | 124 |
| `stage1/crates/axiomc/src/hir/matches.rs` | 737 |
| `stage1/crates/axiomc/src/hir/model.rs` | 607 |
| `stage1/crates/axiomc/src/hir/ownership.rs` | 1129 |
| `stage1/crates/axiomc/src/hir/properties.rs` | 167 |
| `stage1/crates/axiomc/src/hir/reachability.rs` | 161 |
| `stage1/crates/axiomc/src/hir/signatures.rs` | 471 |
| `stage1/crates/axiomc/src/hir/source_locations.rs` | 89 |
| `stage1/crates/axiomc/src/hir/symbols.rs` | 137 |
| `stage1/crates/axiomc/src/hir/types.rs` | 241 |
| `stage1/crates/axiomc/src/hir/variants.rs` | 188 |
| `stage1/crates/axiomc/src/registry.rs` | 2234 |
| `stage1/crates/axiomc/src/lib.rs` | 21 |

## Extraction Order

1. `compiler.backend.native`: start with helpers that are already aligned to
   `stage1/runtime-abi/direct-native-v0.json` rows. Each extraction should keep
   `make stage1-direct-native-runtime-abi-test` passing.
2. `compiler.backend.contracts`: move target selection and unsupported-feature
   contracts out of generated-Rust code before the final generated-Rust removal
   gate.
3. `compiler.hir`: generic inference/monomorphization, public HIR model types,
   syntax-to-HIR type/literal lowering, and type/aggregate definition collection
   are split; function/method signatures, trait impl signature validation,
   capability analysis, expression typing helpers, ownership/borrow helpers,
   property checks, reachability/call-graph discovery, diagnostic recovery
   helpers, monomorphized symbol/intrinsic helpers, source-location helpers,
   return-flow analysis, const-array validation, const-function validation,
   match lowering, enum variant constructor lowering, async runtime intrinsic
   lowering, and map intrinsic lowering are split; continue with remaining HIR
   helper clusters.
4. `compiler.commands` and `compiler.package_graph`: separate command envelopes
   from package loading so the snapshot bootstrap can invoke package APIs
   without Cargo assumptions.
5. `compiler.syntax` and `compiler.diagnostics`: keep public syntax and
   diagnostic fixtures stable while implementation files shrink.

## PR Rules

- Each extraction PR must cite the target AxiOM package boundary, not only the
  Rust module being split.
- Each PR must preserve the existing command JSON envelopes or list the
  intentional envelope delta.
- Each PR must run the package boundary command named in
  `docs/axiom-compiler-source-layout.md`.
- Direct-native backend extractions must also run
  `make stage1-direct-native-runtime-abi-test`.
- Generated-Rust compatibility extractions must not make `rust_source`
  required evidence for direct-native behavior.

## Rust Capture Check

This plan is about migration mechanics only. It does not define Axiom semantics
in terms of Rust files, Rust modules, Cargo, or Cranelift internals. AxiOM
package names and backend-neutral contracts remain the durable self-hosting
boundary.
