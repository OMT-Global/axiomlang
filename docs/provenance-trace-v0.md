# Intent-to-Artifact Provenance v0

`axiomc build` emits a bounded provenance document at `dist/.axiom/provenance.json` for each built package. The document links the package, source modules, source functions, and backend-specific build artifacts with stable `axiom://` ids.

This is an inspection contract for agents and operators. It does not define semantic meaning for Axiom programs and does not make generated Rust the semantic contract.

## Provenance file

The v0 provenance file uses `schema_version = "axiom.provenance.v0"` and records:

- `package`: the package node.
- `nodes`: package, source, and function nodes.
- `artifacts`: backend-specific build artifacts with content hashes. Generated-Rust builds include a `rust_source` artifact and a `native_binary` artifact. Direct-native builds may emit native artifacts without a `rust_source` artifact.
- `relationships`: `declares` edges from package to source and source to functions, plus `emits` edges from sources and functions to artifacts.

Source spans are best-effort parser spans from the stage1 source model. Artifact paths are package-relative where possible.

## Trace command

`axiomc trace <path> --json` reads `<path>/dist/.axiom/provenance.json` and emits the full trace report.

`axiomc trace <axiom-id> --json` reads `./dist/.axiom/provenance.json` and filters the report to relationships touching that node or artifact id.

If the provenance file is missing, run `axiomc build <path>` first.

## GitHub delivery edge

Issue-to-PR traceability is tracked separately from build provenance. The
advisory report described in [Issue-to-PR Traceability v0](issue-pr-traceability.md)
links GitHub issues to pull requests, changed files, and coarse semantic hints.
Build provenance still owns source-to-artifact relationships inside a package.

## Rust capture risk

The only Rust-specific artifact in v0 is `rust_source`, and it is emitted only when the generated-Rust backend produced that artifact. Direct-native provenance must be valid without a generated Rust artifact. Rust source remains an implementation artifact; source functions and package nodes remain Axiom-level provenance nodes.
