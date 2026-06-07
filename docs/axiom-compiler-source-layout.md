# AxiOM Compiler Source Layout and Self-Hosting Boundary

This document maps the current Rust-hosted `stage1/axiomc` implementation to
the AxiOM packages that will own the self-hosted compiler. It is the boundary
contract for #930 and a prerequisite for source migration: implementation PRs
may use this map to move behavior package by package, but they must not redefine
Axiom semantics around Rust modules, Cargo, generated Rust, or `rustc`.

The current Rust compiler remains the supported bootstrap host until the
snapshot release chain proves that a previously released `axiomc` can build the
next `axiomc` without Cargo. The package names below describe the self-hosted
compiler architecture, not the current implementation language.

## Boundary Rules

- AxiOM package names define compiler ownership boundaries; Rust file names are
  migration references only.
- Public compiler APIs are expressed in terms of packages, source text,
  manifests, semantic nodes, diagnostics, evidence, and artifacts.
- Backend-specific artifacts such as `rust_source` or Cranelift object files do
  not define the semantic contract.
- Migration slices must preserve the JSON command envelopes and diagnostics
  used by agents before deleting Rust code.
- Every package migration needs a fixture or command listed in this document
  before it can replace the Rust-hosted implementation.

## Package Map

| Migration order | Future AxiOM package | Current Rust sources | Boundary | Owner lane | Validation command | Rust capture risk |
|---:|---|---|---|---|---|---|
| 1 | `compiler.diagnostics` | `diagnostics.rs`, `diagnostic_catalog.rs`, LSP diagnostic adapters in `lsp.rs` | Diagnostic envelope, codes, source spans, recovery hints, and target diagnostics. | Pheidon | `make stage1-diagnostics-syntax-boundary`, `cargo test --manifest-path stage1/Cargo.toml -p axiomc diagnostic` | Medium: keep spans and codes AxiOM-neutral; do not encode Rust enum names as public codes. |
| 2 | `compiler.syntax` | `syntax.rs` | Lexer, parser, comments, macro expansion records, parse recovery, and concrete source spans. | Daedalus | `make stage1-diagnostics-syntax-boundary`, `cargo test --manifest-path stage1/Cargo.toml -p axiomc parse_program` | Low: grammar terms are AxiOM source concepts; avoid Rust token helper names in public grammar. |
| 3 | `compiler.package_graph` | `manifest.rs`, `lockfile.rs`, package graph portions of `project.rs`, `registry.rs` | Manifest loading, workspace membership, lockfile validation, dependency graph, registry index integrity. | Pheidon | `make stage1-package-graph-boundary`, `make supply-chain`, and `cargo test --manifest-path stage1/Cargo.toml -p axiomc lockfile` | Medium: Cargo metadata may remain test scaffolding only; official package graph must be AxiOM manifest and lockfile based. |
| 4 | `compiler.hir` | `hir.rs`, `borrowck.rs` | Typed declarations, name resolution, imports, capability checks, ownership and borrow validation, property clauses. | Daedalus | `make stage1-compiler-property-test` | High: do not define concepts by Rust lifetimes, traits, `Option`, `Result`, or borrow checker implementation shortcuts. |
| 5 | `compiler.mir` | `mir.rs` | Lowered executable compiler IR, control flow, intrinsic calls, test/property entrypoints, backend input. | Daedalus | `cargo test --manifest-path stage1/Cargo.toml -p axiomc mir` | Medium: MIR is an implementation layer, not Intent IR; keep semantic graph mapping explicit. |
| 6 | `compiler.stdlib` | `stdlib.rs`, `stage1/stdlib/std/*.ax` | Embedded standard library modules, capability-gated stdlib surfaces, stdlib property fixtures. | Daedalus | `make stage1-stdlib-test` | Medium: Rust intrinsics are backend shims; stdlib contracts must be described in AxiOM terms. |
| 7 | `compiler.backend.contracts` | `backend-target-interface-v0.md`, `direct-native-runtime-abi-v0.md`, backend selection in `codegen.rs` and `project.rs` | Target selection, supported feature/effect declarations, artifact kinds, unsupported-feature diagnostics. | Pheidon | `make stage1-direct-native-runtime-abi-test` | High: generated Rust and Cranelift are target implementations, not canonical semantics. |
| 8 | `compiler.backend.generated_rust` | `codegen.rs` | Legacy generated-Rust projection and `rustc` invocation compatibility. | Daedalus | `cargo test --manifest-path stage1/Cargo.toml -p axiomc render_rust` | High: this package is legacy-only and must not be a dependency of self-hosted semantics. |
| 9 | `compiler.backend.native` | `cranelift_backend.rs`, direct-native tests | Direct MIR-to-native lowering, native ABI shims, native binary artifacts, native backend diagnostics. | Daedalus | `cargo test --manifest-path stage1/Cargo.toml -p axiomc --test cranelift_backend` | High: Cranelift details must stay backend-local; direct-native evidence must not require generated Rust. |
| 10 | `compiler.evidence` | provenance/debug/reporting portions of `project.rs`, `stage1-debug-map.md`, `provenance-trace-v0.md` | Build provenance, debug sidecars, evidence records, trace output, source-to-artifact relationships. | Pheidon | `cargo test --manifest-path stage1/Cargo.toml -p axiomc provenance` | Medium: evidence may reference Rust artifacts only when the selected backend emitted them. |
| 11 | `compiler.commands` | `main.rs`, command orchestration in `project.rs`, `new_project.rs`, `json_contract.rs` | `axiomc` command dispatch, JSON envelopes, starter project generation, build/run/test/doc/check/caps/trace flows. | Pheidon | `cargo test --manifest-path stage1/Cargo.toml -p axiomc json_contract` | Medium: command contracts must not require Cargo except on the temporary developer path. |
| 12 | `compiler.services.lsp` | `lsp.rs`, `dap.rs`, `stage1-lsp.md` | LSP/DAP protocol handling, document state, diagnostics publication, future completion and navigation APIs. | Daedalus | `cargo test --manifest-path stage1/Cargo.toml -p axiomc lsp` | Medium: protocol structs can mirror LSP JSON, but compiler analysis must call package APIs instead of Rust internals. |

## Public Self-Hosted Compiler API

The self-hosted compiler API is package-oriented. The CLI may keep its current
surface, but command handlers should delegate to these AxiOM package functions:

| Command | Self-hosted API | Required outputs |
|---|---|---|
| `axiomc check` | `compiler.commands.check_package(root, options)` | diagnostics, capability records, optional exports/debug symbols, `axiom.stage1.v1` JSON envelope |
| `axiomc build` | `compiler.commands.build_package(root, target, debug, locked, offline)` | artifact plan, selected backend contract, provenance, optional debug sidecars, build metadata |
| `axiomc run` | `compiler.commands.run_artifact(root, args, options)` | exit code, stdout/stderr capture when JSON is requested, selected package metadata |
| `axiomc test` | `compiler.commands.test_package(root, filter, options)` | discovered tests, per-case result, property totals, golden stream evidence |
| `axiomc doc` | `compiler.commands.render_docs(root, options)` | documentation artifacts, API extraction records, source symbol evidence |
| `axiomc lsp` | `compiler.services.lsp.serve_stdio()` | JSON-RPC/LSP lifecycle, document diagnostics, future semantic features through compiler package APIs |
| `axiomc trace` | `compiler.evidence.trace(root, query)` | source/function/package/artifact graph filtered by stable `axiom://` ids |

The API deliberately excludes Cargo and `rustc`. Those tools remain allowed for
the temporary developer path and generated-Rust backend compatibility, but the
official self-hosted API must be callable from an `axiomc` snapshot.

## Migration Order

1. Freeze diagnostics and syntax packages first so later migrations can report
   equivalent errors.
2. Move package graph and HIR ownership/capability checks before MIR so source
   analysis is independent of backend selection.
3. Move MIR and stdlib contracts after HIR fixtures prove command behavior.
4. Keep generated-Rust as a legacy backend package while direct-native coverage
   catches up.
5. Move command dispatch only after package APIs expose the JSON envelope data
   without reaching into Rust-specific structs.
6. Move LSP after package graph and diagnostics are self-hosted, so editor
   analysis uses the same compiler APIs as `axiomc check`.
7. Delete Cargo/Rust from the official release lane only after the snapshot
   bootstrap gate and Rust-exit readiness gate pass.

## Allowed Rust Scaffolding

The following Rust implementation details may remain during migration:

- Temporary host modules under `stage1/crates/axiomc/src`.
- Generated-Rust backend support for compatibility and fixture comparison.
- Rust unit tests that prove parity while an AxiOM package is being introduced.
- Cargo-driven developer commands outside the official release chain.
- Cranelift integration details inside `compiler.backend.native`.

The following must not leak into the semantic contract:

- Rust lifetime names, borrow checker internals, trait implementation details,
  or Serde layout as explanations for AxiOM concepts.
- Cargo metadata as the source of package truth.
- `rust_source` as a required artifact for direct-native builds.
- `rustc` as a required step for official self-hosted compiler builds.
- Rust module paths as stable agent-facing inspection identifiers.

## Child Implementation Slices

The following child issues own source migration slices:

- #936 Package graph and lockfile package: move manifest/workspace/lockfile
  behavior behind `compiler.package_graph`.
- #937 Diagnostics and syntax package freeze: migrate parser and diagnostic
  fixtures behind `compiler.diagnostics` and `compiler.syntax` APIs. See
  [Compiler Diagnostics and Syntax Boundary](compiler-diagnostics-syntax.md).
- #938 Command and LSP package split: route CLI and LSP through package APIs
  instead of Rust module internals.
- #939 MIR and backend contract package: expose MIR-to-target inputs without
  generated-Rust assumptions.
- #940 HIR and ownership package: migrate typed declarations, capability checks,
  ownership, borrow, and property-clause behavior behind `compiler.hir`.

Each child issue must cite this document, name the package boundary it owns,
and list the validation command from the package map.
