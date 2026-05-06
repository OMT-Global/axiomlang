# Axiom package manifest

Stage1 packages use `axiom.toml` with a deterministic `axiom.lock` lockfile.
The `axiom.pkg` manifest format is no longer supported.

## Common Commands

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/hello --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/hello --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/hello
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/modules --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- caps stage1/examples/hello --json
>>>>>>> origin/codex/issue-406-collection-lookup
>>>>>>> origin/codex/issue-383-new-templates
>>>>>>> origin/codex/agent-g-regex
>>>>>>> origin/codex/agent-f-fs
>>>>>>> origin/codex/agent-i-language-slice
>>>>>>> origin/codex/issue-387-capability-validation
>>>>>>> origin/codex/issue-395-effective-fs-roots
>>>>>>> origin/codex/worker-h-issue-413
>>>>>>> origin/codex/worker-j-issue-362
>>>>>>> origin/codex/worker-j-issue-363
>>>>>>> origin/codex/issue-369-check-fixtures
>>>>>>> origin/codex/issue-370-command-fixtures
>>>>>>> origin/codex/issue-418-schema-metadata
>>>>>>> origin/codex/issue-422-comparison-gate
>>>>>>> origin/codex/issue-425-crap-thresholds
>>>>>>> origin/codex/issue-423-mutation-smoke
>>>>>>> origin/codex/issue-424-survivor-report
>>>>>>> origin/codex/issue-409-proof-cli
>>>>>>> origin/codex/issue-410-proof-worker
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- publish stage1/examples/hello --registry-dir ./registry/packages --signing-key dev-key
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- registry-index ./registry/packages --base-url https://packages.example.test --out ./registry/index.json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- registry-validate ./registry/index.json
```

## Manifest Shape

The current stage1 examples document the supported manifest surface:

- `stage1/examples/hello`: single-package baseline.
- `stage1/examples/modules`: package-local modules and discovered tests.
- `stage1/examples/packages`: local path dependencies.
- `stage1/examples/workspace`: package-root workspace members.
- `stage1/examples/workspace_only`: workspace-only roots with
  `--package` selection.
- `stage1/examples/capabilities`: manifest-gated runtime capabilities.

<<<<<<< HEAD
`axiomc caps <package> --json` reports the declared capability surface. When
filesystem access is enabled, the `fs` capability includes the manifest-relative
`configured_root` and canonical `effective_root` so operators can inspect the
actual package-local filesystem boundary before build or run.
Local path dependencies may declare a bounded version constraint:

```toml
[dependencies]
core = { path = "deps/core", version = "^0.1.0" }
```

Stage1 currently accepts `*`, exact `MAJOR.MINOR.PATCH`, and caret
`^MAJOR.MINOR.PATCH` constraints. The compiler validates the constraint against
the dependency package's `[package].version` while loading the local package
graph and fails deterministically when the versions are incompatible.
=======
## Publish Contract

Remote publishing is not implemented in stage1, but manifests can now declare
the package metadata that future registry tooling will inspect:

```toml
[publish]
registry = "https://registry.example.test/index"
checksum = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = ["src", "axiom.toml", "axiom.lock"]
```

Package identity still comes from `[package].name` and `[package].version`.
`[publish].registry` is validated as an `https://` or `file:` registry source,
`[publish].checksum` must use `sha256:<64 hex characters>`, and include entries
must be relative paths without parent traversal. These fields define the
manifest contract only; `axiomc` does not publish, upload, or contact a remote
registry.

See [stage1.md](stage1.md) for the current compiler, package, and capability
contract.
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
=======

## Editor Schemas

Checked-in editor and agent metadata lives under `stage1/schemas/`:

- `stage1/schemas/axiom.toml.schema.json` describes the decoded `axiom.toml`
  manifest shape for TOML-aware editors.
- `stage1/schemas/axiom.stage1.v1.schema.json` describes the shared JSON
  envelope emitted by `axiomc check`, `build`, `test`, and `caps` with
  `--json`.

These schemas are intentionally metadata for editor completion, validation, and
agent contract discovery. The compiler remains the source of truth for semantic
checks such as dependency graph validity, capability enforcement, and source
analysis.
=======
=======
=======
=======
=======
=======

## Publish and Static Registry Groundwork

`axiomc publish` packs a checked stage1 package into a deterministic `package.axp`, writes an `axiom-signature-v1` sidecar, and copies `axiom.toml` plus `axiom.lock` into a local registry tree at `<packages>/<name>/<version>/`. The command validates the lockfile first and refuses to replace an existing release unless `--allow-overwrite` is passed.

`axiomc registry-index` builds a static JSON index from package release folders laid out as
`<packages>/<name>/<version>/axiom.toml`. Each release may include:

- `package.axp` plus `package.axp.sig` for signed package artifacts
- `axiom-registry.toml` with `yanked = true` and optional `yank_reason`

The generated index records per-release capability manifests, archive/signature URLs,
and yanked status so a simple static host can serve lockfile-friendly package metadata. This is publish and registry-index groundwork for a future hosted registry service, not the hosted service itself.
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
and yanked status so a simple static host can serve lockfile-friendly package metadata. This is registry-index groundwork for a future hosted registry service, not the hosted service itself.
## Registry And Publish Contract

The local manifest contract reserves the package-registry surface without
implementing remote publishing yet. Today, `axiomc` accepts local path
dependencies only:

```toml
[dependencies]
core = { path = "deps/core" }
```

Package identity is the pair in `[package]`:

```toml
[package]
name = "agent-worker"
version = "0.1.0"
```

Future registry packages will need stable source and integrity metadata:

- Package identity: `package.name` plus `package.version`.
- Registry source: a named registry or URL source for non-local packages.
- Checksums: content-addressed package archives, expected to use a tagged form
  such as `sha256:<hex>`.
- Publish metadata: include/exclude rules, target registry, archive checksum,
  and provenance or signature references.

Those fields are intentionally reserved. Until `axiomc publish` and registry
resolution exist, manifests must not contain `[registry]`, `[publish]`,
`package.checksum`, `package.registry`, `package.source`, or dependency
`version`/`checksum`/`registry`/`source` fields. The parser rejects them instead
of silently treating a registry package as a local package.
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
