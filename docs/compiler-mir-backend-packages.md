# Compiler MIR and Backend Packages

This document defines the self-hosted package boundary for `compiler.mir`,
`compiler.backend.contracts`, `compiler.backend.generated_rust`, and
`compiler.backend.native`. It is the contract for #939 and a child slice of
the Rust-exit roadmap in [AxiOM Compiler Source Layout and Self-Hosting
Boundary](axiom-compiler-source-layout.md).

The current stage1 implementation is still Rust-hosted, and generated Rust
remains a compatibility backend. This boundary keeps generated Rust isolated as
one target implementation while direct native backend inputs, diagnostics, and
evidence flow through package APIs that do not require `rust_source`.

## Package Ownership

`compiler.mir` owns the lowered executable compiler IR after package graph,
syntax, diagnostics, and HIR analysis have succeeded. It exposes typed packages,
functions, declarations, statements, expressions, intrinsic calls, test
entrypoints, and source spans to backend packages. It is an implementation IR,
not the canonical Intent IR.

`compiler.backend.contracts` owns target selection and target contracts. It
declares target classes, supported type features, supported effect kinds,
artifact kinds, required evidence, and unsupported-feature diagnostics.

`compiler.backend.generated_rust` owns the legacy generated-Rust projection.
It may emit `rust_source` and invoke `rustc` while the bootstrap host exists,
but no self-hosted compiler package may depend on generated Rust as semantic
truth.

`compiler.backend.native` owns direct MIR-to-native lowering, native runtime ABI
shims, native binary artifacts, native backend diagnostics, and direct-native
evidence records. Its diagnostics and evidence must stand without a generated
Rust source path.

## Package APIs

| Package | API | Required inputs | Required outputs |
|---|---|---|---|
| `compiler.mir` | `compiler.mir.lower_package(package_graph, hir_package, options)` | Resolved package graph, typed HIR package, diagnostics context, lowering options | `MirPackage` with typed declarations, functions, statements, expressions, intrinsic calls, test entrypoints, source spans, and backend capability requirements. |
| `compiler.mir` | `compiler.mir.export_backend_input(mir_package, target_query)` | `MirPackage`, target class, feature/effect query | Backend input record with MIR version, package id, module set, entrypoints, required type features, required effect kinds, and source-span index. |
| `compiler.backend.contracts` | `compiler.backend.contracts.select_target(mir_input, requested_target, options)` | Backend input, requested target class, debug/locked/offline options | Selected target contract, artifact plan, unsupported-feature diagnostics, and evidence requirements. |
| `compiler.backend.generated_rust` | `compiler.backend.generated_rust.emit_source(mir_input, target_contract, options)` | Backend input and `rust_source` target contract | Generated Rust compatibility artifact, source map, generated-Rust diagnostics, and downstream `rustc` metadata. |
| `compiler.backend.native` | `compiler.backend.native.lower_to_object(mir_input, target_contract, runtime_abi, options)` | Backend input, `native_binary` target contract, direct native runtime ABI | Native object/binary plan, runtime shim requirements, native diagnostics, and direct-native evidence records. |
| `compiler.backend.native` | `compiler.backend.native.explain_unsupported(mir_input, target_contract)` | Backend input and target contract | Axiom diagnostic envelope records for unsupported type features, effect kinds, artifact outputs, or missing runtime ABI rows. |

## Backend Input Contract

A backend input record must include:

- `mir_version`: stable MIR package contract version.
- `package_id`: stable package identity from `compiler.package_graph`.
- `entrypoints`: functions, tests, and exported runtime entries available to a target.
- `type_features`: declarative type features required by the MIR package.
- `effect_kinds`: backend-neutral effect kinds required by the package.
- `artifact_kinds`: artifact classes requested by command or package metadata.
- `source_spans`: source-span lookup records for diagnostics and evidence.
- `evidence_hooks`: build, run, test, debug, and provenance evidence slots.

Backend input must not include parser helper names, HIR checker internals, Rust
module paths, Cargo metadata, `rustc` command lines, or generated Rust source as
required input. A target may return those values only as target-local
compatibility metadata.

## Direct Native Evidence Rules

Direct-native evidence may reference native object files, native binary paths,
runtime ABI rows, unsupported-feature diagnostics, debug sidecars, and
`axiom://` source/artifact ids. It must not require `rust_source`,
`generated_rust`, Cargo metadata, or `rustc` output to prove direct-native
behavior.

Generated Rust evidence remains valid for the legacy
`compiler.backend.generated_rust` package only. Direct-native parity can compare
against generated Rust during migration, but the comparison is optional
evidence, not the direct-native contract.

## Validation

The package boundary snapshot lives in
`stage1/compiler-contracts/snapshots/mir-backend.json` and validates against
`stage1/compiler-contracts/schemas/axiom.compiler.mir_backend.v1.schema.json`.

Use these commands for #939:

```bash
make stage1-mir-backend-boundary
make stage1-mir-backend-boundary-test
make stage1-direct-native-runtime-abi-test
cargo test --manifest-path stage1/Cargo.toml -p axiomc --test cranelift_backend
```

