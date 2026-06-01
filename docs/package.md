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
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- publish stage1/examples/hello --registry-dir ./registry/packages --signing-key dev-key
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- pkg graph stage1/examples/workspace_only --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- registry-index ./registry/packages --base-url https://packages.example.test --out ./registry/index.json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- registry-validate ./registry/index.json --packages-dir ./registry/packages --signing-key dev-key
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- registry-serve ./registry/packages --addr 127.0.0.1:8080 --base-url http://127.0.0.1:8080
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

`axiomc caps <package> --json` reports the declared capability surface. When
filesystem access is enabled, the `fs` capability includes the manifest-relative
`configured_root` and canonical `effective_root` so operators can inspect the
actual package-local filesystem boundary before build or run.

`axiomc pkg graph <path> --json` prints the resolved local package graph without
mutating manifests or lockfiles. The JSON lists each package root, package
identity, workspace members, local dependencies, build entrypoint, capabilities,
and whether that package's `axiom.lock` is current or stale.

Local path dependencies may declare a bounded version constraint:

```toml
[dependencies]
core = { path = "deps/core", version = "^0.1.0" }
```

Stage1 currently accepts `*`, exact `MAJOR.MINOR.PATCH`, and caret
`^MAJOR.MINOR.PATCH` constraints. The compiler validates the constraint against
the dependency package's `[package].version` while loading the local package
graph and fails deterministically when the versions are incompatible.

## Editor Schemas

Checked-in editor and agent metadata lives under `stage1/schemas/`:

- `stage1/schemas/axiom.toml.schema.json` describes the decoded `axiom.toml`
  manifest shape for TOML-aware editors.
- `stage1/schemas/axiom.stage1.v1.schema.json` describes the shared JSON
  envelope emitted by `axiomc check`, `build`, `test`, and `caps` with
  `--json`.
- `stage1/schemas/axiom-intent-ir-v0.schema.json` describes the first
  agent-facing Intent IR / semantic graph contract. See
  [intent-ir-v0.md](intent-ir-v0.md).

These schemas are intentionally metadata for editor completion, validation, and
agent contract discovery. The compiler remains the source of truth for semantic
checks such as dependency graph validity, capability enforcement, and source
analysis.

## Publish and Static Registry Groundwork

`axiomc publish` packs a checked stage1 package into a deterministic `package.axp`, writes an `axiom-integrity-v1` sidecar bound to a required `--signing-key`, and copies `axiom.toml` plus `axiom.lock` into a local registry tree at `<packages>/<name>/<version>/`. The command validates the lockfile first and refuses to replace an existing release unless `--allow-overwrite` is passed. The sidecar is a tamper-detection integrity tag, not a cryptographic signature; the stage1 registry does not yet provide authenticity proof.

`axiomc registry-index` builds a static JSON index from package release folders laid out as
`<packages>/<name>/<version>/axiom.toml`. Each release may include:

- `package.axp` plus `package.axp.sig` for signed package artifacts
- `axiom-registry.toml` with `yanked = true` and optional `yank_reason`

The generated index records per-release capability manifests, archive/signature URLs,
and yanked status so a simple static host can serve lockfile-friendly package metadata. `axiomc registry-validate` checks the index contract by default; when passed `--packages-dir` and `--signing-key`, it also reads every indexed local archive plus sidecar and rejects tampered archives or mismatched integrity keys.

`axiomc registry-serve <packages-dir>` starts a small read-only HTTP registry for that same release tree. It serves:

- `/index.json` and `/` as a freshly rendered registry index
- `/<package>/<version>/axiom.toml`
- `/<package>/<version>/axiom.lock`
- `/<package>/<version>/package.axp`
- `/<package>/<version>/package.axp.sig`

The server rebuilds and validates the index before serving package files, so malformed manifests, mismatched archive sidecars, unsafe path segments, or invalid yank metadata fail before artifacts are exposed. Pass `--base-url` when the registry is behind a proxy or a stable hostname; otherwise the server derives a local `http://host:port` base URL from the bound address. The hosted stage1 registry remains read-only: package uploads still happen through `axiomc publish`, and the `.sig` sidecar is still an integrity tag rather than cryptographic authenticity proof.

## Registry And Publish Contract

The local manifest contract exposes publish metadata for future registry tooling while keeping dependency resolution local-only. Today, `axiomc` accepts local path dependencies and rejects registry dependency selectors:

```toml
[dependencies]
core = { path = "deps/core" }
```

Package identity is the pair in `[package]`. Publish metadata is optional and declarative only:

```toml
[package]
name = "agent-worker"
version = "0.1.0"

[publish]
registry = "https://registry.example.test/index"
checksum = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = ["src/**", "axiom.toml", "axiom.lock"]
exclude = ["dist/**"]
```

Future registry packages will need stable source and integrity metadata:

- Package identity: `package.name` plus `package.version`.
- Registry source: a named registry or URL source for non-local packages.
- Checksums: content-addressed package archives, expected to use a tagged form
  such as `sha256:<hex>`.
- Publish metadata: include/exclude rules, target registry, archive checksum,
  and provenance or signature references.

Those registry fields are intentionally reserved. Until registry resolution
exists, manifests must not contain root `[registry]`, `package.checksum`,
`package.registry`, `package.source`, or dependency
`checksum`/`registry`/`source` fields. Local dependency `version` constraints
are accepted only with a local `path` and are validated against the dependency
package version. The parser rejects reserved registry fields instead
of silently treating a registry package as a local package. `[publish]` is
accepted only as metadata; it does not make `axiomc` contact or upload to a
remote registry.
