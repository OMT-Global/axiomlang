# Compiler HIR, Ownership, and Capability Package

This document defines the self-hosted package boundary for `compiler.hir`.
It is the contract for #940 and a child slice of the Rust-exit roadmap in
[AxiOM Compiler Source Layout and Self-Hosting Boundary](axiom-compiler-source-layout.md).

The current stage1 implementation is still Rust-hosted. This boundary keeps
typed source analysis, capability policy, ownership state, borrow state, and
property clause verdicts behind AxiOM package APIs so later migrations can
replace host implementation files without changing language semantics.

## Package Ownership

`compiler.hir` owns typed declarations after package graph, syntax, and
diagnostics have produced source units. It resolves imports and names, checks
types, evaluates package capability policy, evaluates ownership and borrow
state, reports property clause verdicts, exports public API records for docs,
and reports inferred capability use for agent inspection.

`compiler.hir` does not own package discovery, parser recovery, MIR lowering,
backend target selection, generated source projection, or artifact provenance.
Its boundary snapshot also keeps the package-level `must_not_own` surface
under the same Rust-capture validation as the owned package fields, so both
`owns` and `must_not_own` are treated as part of the reviewed contract surface.

## Package APIs

| API | Required inputs | Required outputs |
|---|---|---|
| `compiler.hir.build_package_hir(package_graph, syntax_units, options)` | Resolved package graph, parsed source units, diagnostics context, analysis options | Typed package HIR with declarations, imports, names, source spans, and unresolved diagnostic records. |
| `compiler.hir.resolve_names(hir_package, package_exports)` | Typed package HIR and dependency export records | Resolved name table, import visibility diagnostics, and source-correlated unresolved import/name diagnostics. |
| `compiler.hir.check_types(hir_package, name_table, options)` | HIR package, resolved names, type-check options | Typed expression/statement records, type diagnostics, and public declaration signatures. |
| `compiler.hir.evaluate_capability_policy(hir_package, manifest_policy)` | HIR package and manifest capability policy | Capability use records, allowed/denied verdicts, and capability diagnostics with source spans. |
| `compiler.hir.evaluate_ownership(hir_package, type_table)` | HIR package and typed values | Ownership state transitions, move diagnostics, and source-correlated owned-value hazards. |
| `compiler.hir.evaluate_borrow_state(hir_package, ownership_state)` | HIR package and ownership state | Borrow state transitions, conflicting-borrow diagnostics, and borrowed-return origin diagnostics. |
| `compiler.hir.evaluate_property_clauses(hir_package, type_table)` | HIR package and typed property clauses | Property clause verdicts, static failure diagnostics, and property totals for command envelopes. |
| `compiler.hir.export_public_api(hir_package)` | HIR package with resolved names and types | Public API symbols, signatures, source spans, module imports, and package capability surface. |
| `compiler.hir.infer_capability_use(hir_package)` | HIR package with typed calls and imports | Inferred capability uses grouped by package, symbol, source span, and policy verdict. |

## Analysis Input Contract

HIR analysis input must include:

- `package_graph`: resolved package identity, dependency edges, and package roots.
- `syntax_units`: parsed source units with concrete source spans.
- `diagnostics_context`: diagnostic envelope and stable source-span machinery.
- `manifest_capability_policy`: declared package capability policy.
- `source_span_index`: source location records used by diagnostics and agent inspection.

HIR analysis input must not require host implementation file names, host
package-manager metadata, generated source, backend artifacts, or backend
runtime diagnostics.

## Source-Correlated Diagnostics

Ownership, borrow, capability, and property diagnostics emitted by
`compiler.hir` must carry stable diagnostic kind/code data plus source
correlation. At minimum, diagnostics include a source path plus start line and
column when the failing source construct is known. Additional end-span fields
may be additive, but they are not required by this boundary.

## Property Fixture Preservation

The current compiler property corpus in `stage1/examples/compiler_properties`
is the fixture set for this boundary. The corpus must continue to expose typed
declarations, capability policy, ownership flow, and property clause behavior
through `make stage1-compiler-property-test`.

## Validation

The package boundary snapshot lives in
`stage1/compiler-contracts/snapshots/hir-ownership-capability.json` and
validates against
`stage1/compiler-contracts/schemas/axiom.compiler.hir_ownership_capability.v1.schema.json`.

Use these commands for #940:

```bash
make stage1-hir-boundary
make stage1-hir-boundary-test
make stage1-compiler-property-test
```
