# Compiler Command and LSP Packages

This document defines the self-hosted package boundary for `compiler.commands`
and `compiler.services.lsp`. It is the contract for #938 and a child slice of
the Rust-exit roadmap in [AxiOM Compiler Source Layout and Self-Hosting
Boundary](axiom-compiler-source-layout.md).

The current implementation is still hosted by the Rust `stage1/axiomc`
binary. The package APIs below define the self-hosted compiler ownership
surface that future AxiOM packages must provide. Rust modules, Cargo metadata,
Clap structs, and generated Rust backend helpers are implementation details,
not command or service contracts.

## Package Ownership

`compiler.commands` owns user-facing command dispatch and stable machine
envelopes. It receives source roots, selected package names, command options,
and forwarded runtime arguments, then delegates analysis and artifact work to
compiler packages instead of reaching into host-language internals.

`compiler.services.lsp` owns JSON-RPC framing, bounded document state, LSP
lifecycle handling, diagnostic publication, and future editor semantic
features. It delegates parsing, package graph discovery, HIR analysis,
diagnostic normalization, and evidence lookup to package APIs.

## Command APIs

| Command | API | Required package calls | Stable output |
|---|---|---|---|
| `axiomc check` | `compiler.commands.check_package(root, package, options)` | `compiler.package_graph.load_workspace`, `compiler.syntax.parse_program`, `compiler.hir.analyze`, `compiler.diagnostics.diagnostic` | `axiom.stage1.v1` check envelope with diagnostics, package records, capability records, warnings, optional macro expansions, and optional debug symbols. |
| `axiomc build` | `compiler.commands.build_package(root, package, target, debug, locked, offline)` | `compiler.package_graph.resolve_locked`, `compiler.hir.analyze`, `compiler.mir.lower_package`, `compiler.backend.contracts.select_target`, `compiler.evidence.record_build` | `axiom.stage1.v1` build envelope with artifact plan, selected backend, build metadata, cache metadata, provenance, and optional debug sidecars. |
| `axiomc run` | `compiler.commands.run_artifact(root, package, args, options)` | `compiler.commands.build_package`, `compiler.evidence.record_run` | Text streaming by default; `axiom.stage1.v1` run envelope with selected backend, exit code, stdout, stderr, result, forwarded args, selected package, and optional generated-Rust artifact when JSON is requested. |
| `axiomc test` | `compiler.commands.test_package(root, package, filter, options)` | `compiler.package_graph.load_workspace`, `compiler.commands.build_package`, `compiler.evidence.record_test` | `axiom.stage1.v1` test envelope with selected backend, discovered tests, per-case results, property totals, golden stream evidence, duration, filter metadata, and optional per-case generated-Rust artifacts. |
| `axiomc doc` | `compiler.commands.render_docs(root, package, options)` | `compiler.package_graph.load_workspace`, `compiler.hir.export_public_api`, `compiler.evidence.record_artifact` | Versioned doc JSON plus Markdown/HTML artifact paths, symbol extraction records, source comments, signatures, and package capabilities. |
| `axiomc caps` | `compiler.commands.describe_capabilities(root, package, options)` | `compiler.package_graph.load_workspace`, `compiler.hir.infer_capability_use` | `axiom.stage1.v1` caps envelope with manifest capability policy, inferred uses, unsafe escape hatches, owners, and rationales. |
| `axiomc trace` | `compiler.evidence.trace(root, query)` | `compiler.package_graph.load_workspace`, `compiler.evidence.trace_graph` | `axiom.trace.v0` provenance graph with stable `axiom://` node IDs, artifacts, and relationships. |

The command APIs deliberately exclude Cargo and `rustc` from official
self-hosted command behavior. Cargo remains allowed only for the temporary
developer path while the Rust bootstrap host and generated-Rust backend remain
checked in.

## LSP Service APIs

| Flow | API | Required package calls | Stable protocol |
|---|---|---|---|
| Stdio server | `compiler.services.lsp.serve_stdio()` | `compiler.services.lsp.initialize`, document handlers, shutdown handlers | LSP messages framed with `Content-Length` headers and JSON-RPC 2.0 bodies. |
| Initialize | `compiler.services.lsp.initialize(params)` | package capability registry for advertised features | JSON-RPC response with bounded text document sync and additive future capabilities. |
| Open document | `compiler.services.lsp.open_document(uri, version, text)` | `compiler.syntax.parse_program`, `compiler.diagnostics.diagnostic` | `textDocument/publishDiagnostics` notification for the opened document. |
| Change document | `compiler.services.lsp.change_document(uri, version, text)` | `compiler.syntax.parse_program`, `compiler.package_graph.resolve_document`, `compiler.hir.analyze` when package context is available | Updated `textDocument/publishDiagnostics` notification. |
| Publish diagnostics | `compiler.services.lsp.publish_diagnostics(uri, diagnostics)` | `compiler.diagnostics.to_lsp_range`, diagnostic code normalization, and `compiler.evidence.lookup_related` when related evidence is available | LSP diagnostic array preserving source ranges, messages, severity, stable codes, and additive related context. |
| Shutdown | `compiler.services.lsp.shutdown()` | service state drain only | JSON-RPC shutdown response with `null` result. |
| Exit | `compiler.services.lsp.exit()` | service state drain only | Clean process exit after shutdown or bounded best-effort exit otherwise. |

Future completion, hover, definition, and semantic-token APIs must flow through
the package graph, syntax, HIR, diagnostics, and evidence packages. They must
not inspect Rust-only parser, checker, or LSP module internals as their public
contract.

## Stable Envelope Rules

- Commands keep their human-readable behavior unless `--json` is requested.
- JSON command payloads keep stable schema names and `command` labels while
  package implementations migrate.
- LSP framing remains standard `Content-Length` delimited JSON-RPC messages.
- Protocol payloads may add fields only when existing clients can ignore them.
- Backend-specific paths such as generated Rust files are compatibility
  metadata, not requirements for direct-native or self-hosted command behavior.

## Validation

The package boundary snapshot lives in
`stage1/compiler-contracts/snapshots/command-lsp.json` and validates against
`stage1/compiler-contracts/schemas/axiom.compiler.command_lsp.v1.schema.json`.

Use these commands for #938:

```bash
make stage1-command-lsp-boundary
make stage1-command-lsp-boundary-test
cargo test --manifest-path stage1/Cargo.toml -p axiomc json_contract
cargo test --manifest-path stage1/Cargo.toml -p axiomc lsp
```
