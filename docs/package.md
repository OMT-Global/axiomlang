# Axiom package manifest

Stage1 packages use `axiom.toml` with a deterministic `axiom.lock` lockfile.
The `axiom.pkg` manifest format is no longer supported.

Package graph truth is AxiOM-owned: `axiom.toml`, `axiom.lock`, local source
files, workspace members, and future registry integrity records define package
identity. Cargo remains the current developer host for running the Rust stage1
compiler, but Cargo metadata is not part of the package graph contract. See
[Compiler Package Graph Boundary](compiler-package-graph.md).

## Common Commands

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/hello --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/hello --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/hello
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/modules --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- caps stage1/examples/hello --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- pkg graph stage1/examples/workspace_only --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- registry-validate ./registry/index.json --packages-dir ./registry/packages --trust-roots ./registry/trust-roots.json --expectation ./registry/verification-request.json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- registry-serve ./registry/packages --index ./registry/index.json --trust-roots ./registry/trust-roots.json --expectation ./registry/verification-request.json --addr 127.0.0.1:8080
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
- `stage1/schemas/axiom-package-signature-v1.schema.json` describes package
  signing sidecar payloads.
- `stage1/schemas/axiom-trust-roots-v1.schema.json` describes trusted signing
  roots and root status.
- `stage1/schemas/axiom-registry-index-v2.schema.json` describes static registry
  index records and package provenance links.
- `stage1/schemas/axiom-package-verification-expectation-v1.schema.json` describes
  deterministic package verification expectations, including accepted key IDs.
- `stage1/schemas/axiom-package-verification-v1.schema.json` describes package
  verification decisions and trust statuses.
- `stage1/compiler-contracts/schemas/axiom.compiler.package_graph.v1.schema.json`
  describes the self-hosting package graph contract used by
  `compiler.package_graph`. See
  [Compiler Package Graph Boundary](compiler-package-graph.md).

These schemas are intentionally metadata for editor completion, validation, and
agent contract discovery. The compiler remains the source of truth for semantic
checks such as dependency graph validity, capability enforcement, and source
analysis.

## Package Trust Contract

`make stage1-package-trust-contract` validates the Ed25519 + SHA-256 package
trust contract, including canonical transcript bytes, signed root/index
thresholds, identity and provenance bindings, offline pins, and
positive/negative verification vectors. The regression target is
`make stage1-package-trust-contract-test`.

See [Package Trust v1 Contract](package-trust-v1.md) for the binary transcript,
trust-root and index model, stable reason codes, and official specifications.

## Asymmetric Publish and Static Registry Flow

`axiomc publish` packs a checked stage1 package into a deterministic
`package.axp`, binds the exact archive, manifest, provenance, publisher, and
registry plus immutable index publication-floor coordinates in a Package Trust
signature envelope, and writes the release atomically under
`<packages>/<namespace>/<name>/<version>/`. Publication validates the lockfile
and refuses to replace a release unless `--allow-overwrite` is passed.

`--index-generation` and `--index-sequence` are not an exact forever-snapshot
binding. They are signed not-before floors: an authenticated current index must
meet or exceed each component, so later index generations and sequences can
continue carrying the same immutable package envelope. A current index below
either floor is rejected. The current signed index and the offline expectation's
index generation, sequence, and transcript digest remain exact matches.

Both publisher commands accept repeatable `--signing-key-file` flags. Each file
contains an Ed25519 seed and must be protected as publisher-only secret
material; consumers never receive a signing key. Supply enough distinct,
authorized keys to satisfy the thresholds in the verification expectation.
For example:

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- publish stage1/examples/hello \
  --registry-dir ./registry/packages \
  --namespace axiom \
  --registry-identity axiom-registry-production \
  --source-identity registry:axiom-production \
  --publisher-identity https://publishers.example/foundation \
  --index-generation 42 \
  --index-sequence 1042 \
  --provenance ./registry/hello-provenance.json \
  --trust-roots ./registry/trust-roots.json \
  --expectation ./registry/verification-request.json \
  --signing-key-file ./secrets/publisher-a.seed \
  --signing-key-file ./secrets/publisher-b.seed

cargo run --manifest-path stage1/Cargo.toml -p axiomc -- registry-index ./registry/packages \
  --registry-identity axiom-registry-production \
  --source-identity registry:axiom-production \
  --generation 42 \
  --sequence 1042 \
  --issued-at 2026-07-29T10:00:00Z \
  --expires-at 2026-08-29T10:00:00Z \
  --snapshot-id snapshot-42 \
  --metadata-path index.json \
  --previous-snapshot-sha256 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa \
  --trust-roots ./registry/trust-roots.json \
  --expectation ./registry/verification-request.json \
  --signing-key-file ./secrets/registry-a.seed \
  --signing-key-file ./secrets/registry-b.seed \
  --out ./registry/index.json
```

Index generation includes only releases that pass full Package Trust
verification, then signs the v2 index with its registry-index role. Legacy
`axiom-hmac-sha256-v1` sidecars and unsigned v1 indexes are rejected; there is
no HMAC downgrade path.

Consumers provide only the signed index, release directory, public trust roots,
and verification expectation:

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- registry-validate ./registry/index.json \
  --packages-dir ./registry/packages \
  --trust-roots ./registry/trust-roots.json \
  --expectation ./registry/verification-request.json

cargo run --manifest-path stage1/Cargo.toml -p axiomc -- registry-serve ./registry/packages \
  --index ./registry/index.json \
  --trust-roots ./registry/trust-roots.json \
  --expectation ./registry/verification-request.json \
  --addr 127.0.0.1:8080
```

`registry-validate` verifies the signed index and every exact archive, manifest,
provenance statement, and package signature. `registry-serve` performs the same
full verification before binding its listener, captures the verified index and
release bytes in memory, and serves that immutable snapshot. Later filesystem
changes cannot alter the bytes being served.

The read-only server exposes:

- `/index.json` and `/` as the verified signed index
- `/<namespace>/<package>/<version>/axiom.toml`
- `/<namespace>/<package>/<version>/provenance.json`
- `/<namespace>/<package>/<version>/package.axp`
- `/<namespace>/<package>/<version>/package.axp.sig`

Pass `--base-url` when the registry is behind a proxy or stable hostname;
otherwise it derives a local URL from the bound address. Uploads remain a
separate `axiomc publish` operation.

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
