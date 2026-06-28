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
- the top-file share of the compiler source tree;
- the self-hosted package boundary each large file must move toward.

This is advisory evidence, not a release blocker. The trend target is that the
largest file and top-seven file share move downward release over release as
child extraction PRs land.

## Current Top Files

Snapshot from 2026-06-21:

| Rank | Current Rust file | Lines | Target package boundary | First extraction slice |
| ---: | --- | ---: | --- | --- |
| 1 | `stage1/crates/axiomc/src/cranelift_backend.rs` | 21,536 | `compiler.backend.native` | Split direct-native lowering by runtime ABI groups: scalar/aggregate value features, capability shims, host imports, object emission, unsupported diagnostics, and evidence helpers. |
| 2 | `stage1/crates/axiomc/src/hir.rs` | 16,758 | `compiler.hir` | Split name resolution, type checking, capability analysis, ownership/borrow validation, property clauses, and HIR diagnostics behind the package APIs in `docs/compiler-hir-ownership-capability.md`. |
| 3 | `stage1/crates/axiomc/src/lib.rs` | 14,684 | compiler package facade | Reduce to package exports and shared test scaffolding while moving implementation logic into package-owned modules. |
| 4 | `stage1/crates/axiomc/src/main.rs` | 9,936 | `compiler.commands` | Move command parsing, JSON envelope construction, check/build/run/test/doc/trace orchestration, and exit handling behind `docs/compiler-command-lsp-packages.md` APIs. |
| 5 | `stage1/crates/axiomc/src/project.rs` | 8,684 | `compiler.package_graph`, `compiler.commands`, `compiler.evidence` | Split manifest/workspace loading, command orchestration, provenance/debug records, and build artifact planning along package ownership. |
| 6 | `stage1/crates/axiomc/src/codegen.rs` | 7,772 | `compiler.backend.generated_rust`, `compiler.backend.contracts` | Isolate generated-Rust compatibility emission from backend target selection and unsupported-feature contracts. |
| 7 | `stage1/crates/axiomc/src/syntax.rs` | 6,324 | `compiler.syntax`, `compiler.diagnostics` | Split lexer/parser, parse recovery, source spans, macros, and syntax diagnostics behind the syntax boundary. |

## Extraction Order

1. `compiler.backend.native`: start with helpers that are already aligned to
   `stage1/runtime-abi/direct-native-v0.json` rows. Each extraction should keep
   `make stage1-direct-native-runtime-abi-test` passing.
2. `compiler.backend.contracts`: move target selection and unsupported-feature
   contracts out of generated-Rust code before the final generated-Rust removal
   gate.
3. `compiler.hir`: split resolution, typing, capability, ownership, and
   property checks in that order so diagnostics stay stable.
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
