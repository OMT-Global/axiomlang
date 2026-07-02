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
`stage1/crates/axiomc/src/hir/capabilities.rs`, lowering both absolute top-file
lines and share.

## Current Top Files

Snapshot from 2026-07-02:

| Rank | Current Rust file | Lines | Target package boundary | First extraction slice |
| ---: | --- | ---: | --- | --- |
| 1 | `stage1/crates/axiomc/src/cranelift_backend.rs` | 27,994 | `compiler.backend.native` | Split direct-native lowering by runtime ABI groups: scalar/aggregate value features, capability shims, host imports, object emission, unsupported diagnostics, and evidence helpers. |
| 2 | `stage1/crates/axiomc/src/project.rs` | 10,812 | `compiler.package_graph`, `compiler.commands`, `compiler.evidence` | Split manifest/workspace loading, command orchestration, provenance/debug records, and build artifact planning along package ownership. |
| 3 | `stage1/crates/axiomc/src/hir.rs` | 10,001 | `compiler.hir` | Generic inference and monomorphization now live in `stage1/crates/axiomc/src/hir/generics.rs`; public HIR model types now live in `stage1/crates/axiomc/src/hir/model.rs`; syntax-to-HIR type/literal lowering now lives in `stage1/crates/axiomc/src/hir/types.rs`; type-name, aggregate, and trait-use definition checks now live in `stage1/crates/axiomc/src/hir/definitions.rs`; function/method signatures and trait impl signature validation now live in `stage1/crates/axiomc/src/hir/signatures.rs`; capability analysis now lives in `stage1/crates/axiomc/src/hir/capabilities.rs`; next split expression typing, ownership/borrow validation, property clauses, and HIR diagnostics behind the package APIs in `docs/compiler-hir-ownership-capability.md`. |
| 4 | `stage1/crates/axiomc/src/main.rs` | 10,678 | `compiler.commands` | Move command parsing, JSON envelope construction, check/build/run/test/doc/trace orchestration, and exit handling behind `docs/compiler-command-lsp-packages.md` APIs. |
| 5 | `stage1/crates/axiomc/src/codegen.rs` | 7,804 | `compiler.backend.generated_rust`, `compiler.backend.contracts` | Isolate generated-Rust compatibility emission from backend target selection and unsupported-feature contracts. |
| 6 | `stage1/crates/axiomc/src/syntax.rs` | 6,324 | `compiler.syntax`, `compiler.diagnostics` | Split lexer/parser, parse recovery, source spans, macros, and syntax diagnostics behind the syntax boundary. |
| 7 | `stage1/crates/axiomc/src/hir/generics.rs` | 4,205 | `compiler.hir` | Keep generic call inference, trait-bound validation, aggregate monomorphization, and generic call rewriting isolated from the main HIR lowering facade. |

## Ratchet Ceilings

These ceilings are consumed by
`scripts/ci/report-compiler-source-monoliths.py --check-ratchet`. A PR that
adds lines above any ceiling fails `make stage1-compiler-source-monoliths`.
When an extraction PR shrinks a tracked monolith or top-file share, lower the
matching ceiling in this table in the same PR.

| Tracked item | Ceiling |
| --- | ---: |
| `summary.top_file_line_share` | 0.8736 |
| `summary.top_file_lines` | 77818 |
| `stage1/crates/axiomc/src/cranelift_backend.rs` | 27994 |
| `stage1/crates/axiomc/src/hir.rs` | 10001 |
| `stage1/crates/axiomc/src/project.rs` | 10812 |
| `stage1/crates/axiomc/src/main.rs` | 10678 |
| `stage1/crates/axiomc/src/codegen.rs` | 7804 |
| `stage1/crates/axiomc/src/syntax.rs` | 6324 |
| `stage1/crates/axiomc/src/hir/capabilities.rs` | 773 |
| `stage1/crates/axiomc/src/hir/definitions.rs` | 684 |
| `stage1/crates/axiomc/src/hir/generics.rs` | 4205 |
| `stage1/crates/axiomc/src/hir/model.rs` | 607 |
| `stage1/crates/axiomc/src/hir/signatures.rs` | 471 |
| `stage1/crates/axiomc/src/hir/types.rs` | 241 |
| `stage1/crates/axiomc/src/registry.rs` | 2159 |
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
   are split; function/method signatures, trait impl signature validation, and
   capability analysis are split; continue with expression typing, ownership,
   and property checks in that order so diagnostics stay stable.
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
