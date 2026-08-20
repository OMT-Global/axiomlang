# Axiom package manifest

Stage1 packages use `axiom.toml` with a deterministic `axiom.lock` lockfile.
The `axiom.pkg` manifest format is no longer supported.

Package graph truth is AxiOM-owned: `axiom.toml`, `axiom.lock`, local source
files, workspace members, authenticated registry records, and exact cached or
vendored release bytes define package identity. Cargo remains the current
developer host for running the Rust stage1 compiler, but Cargo metadata is not
part of the package graph contract. See
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

`axiomc pkg graph <path> --json` prints the resolved package graph without
mutating manifests or lockfiles. Local path nodes retain their package roots;
registry nodes report the locked source, selected version, Package Trust
decision, cache or vendor disposition, dependency edges, and deterministic
resolver decisions. This makes the graph an inspection surface for why a
version was requested and selected rather than only a list of paths.

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
- `stage1/schemas/axiom-lockfile-v2.schema.json` describes the strict
  compatibility, registry, package, and dependency-edge records written to a
  registry-enabled `axiom.lock`.
- `stage1/schemas/axiom-package-resolution-v1.schema.json` describes selected
  packages, preserved path dependencies, dependency edges, and the ordered
  resolver trace emitted for agent inspection.
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

## Package Resolver v1

The bounded Package Resolver v1 contract is an executable static spike under
`make stage1-package-resolver`, `make stage1-package-graph-boundary`, and the
toolchain supply-chain gate. Those checks cover signed static registry
metadata, regular local files, and numeric-loopback fixtures; they do not
constitute the immutable exact-head supported-host receipt required for
runtime-complete promotion or the load, recovery, release-artifact, and
operational receipt required for production qualification. Public hosted
transport and edition selection also remain out of scope.

Package Resolver v1 preserves local path dependencies and adds one explicitly
configured static registry source. A root manifest names the registry and the
project-relative Package Trust policy files that authenticate it:

```toml
[registry]
name = "fixture"
index = "file:///absolute/path/to/registry/index.json"
trust_roots = "trust/roots.json"
expectation = "trust/expectation.json"
cache = ".axiom/cache"
vendor = "vendor"

[dependencies.local_util]
path = "deps/local-util"
version = "^0.4.0"

[dependencies.core]
registry = "fixture"
namespace = "axiom"
package = "core"
version = "^1.2.3"
```

`registry.name`, `registry.index`, `registry.trust_roots`, and
`registry.expectation` are required. `registry.cache` and `registry.vendor` are
optional project-relative roots and must not name the same path. A registry
dependency must name that configured registry, a portable lowercase namespace,
an optional published package name (defaulting to the dependency alias), and
an exact or caret release version. Path and registry sources are mutually
exclusive. Registry versions are strict `MAJOR.MINOR.PATCH` releases; wildcard,
prerelease, build-metadata, range, and tilde selectors are rejected.

The current transport is deliberately a local qualification surface:

- `file://` reads a regular, non-symlink local index or artifact.
- `http://` is limited to numeric loopback addresses for the checked local HTTP
  registry fixture.
- Redirects, transfer encoding, content encoding other than identity,
  ambiguous lengths, oversized headers/bodies, and truncated responses fail
  closed.
- Public HTTPS transport and a hosted registry service are not implemented by
  this resolver slice. A syntactically valid HTTPS index may be retained as
  forward-compatible manifest data, but the current fetch transport rejects it.

This is not an edition-selection mechanism. Resolver v1 records the current
compatibility and edition policy as evidence; it does not choose an edition for
a package.

### Resolution and trust boundary

The resolver first authenticates the exact Registry Index v2 bytes with
Package Trust v1. Only an `AuthenticatedRegistryCatalog` can supply candidates.
Its read-only catalog and release methods expose authenticated registry/source
identities, current root and index positions, transcript digests, signer IDs,
yank state, artifact digests, exact index bytes, and normalized paths for
`package.axp`, `axiom.toml`, `provenance.json`, and `package.axp.sig`. Resolver
code cannot construct or mutate authenticated release internals directly.

Resolution allows one version for each canonical
`(registry_identity, source_identity, namespace, package)` coordinate. It
orders coordinates lexically, tries matching strict release versions in
descending order, and uses authenticated release ID and target path lexical
order as the final tie-break. Duplicate coordinate/version records are
rejected. Exact and caret requirements, transitive edges, conflicts, yanks,
candidate attempts, backtracks, and trace events all use fixed deterministic
work budgets.

Each decision records the dependency alias, requested constraint, selected
coordinate, candidate disposition, and reason. An index signature failure,
expired or rolled-back metadata, replay, duplicate coordinate or target path,
source mismatch, incompatible constraints, work-budget exhaustion, or
ineligible yanked candidate fails closed before package bytes become graph
inputs. Fresh resolution never selects a yanked release. Locked replay may
retain one only when every exact Package Trust, digest, transcript, and cache or
vendor pin still verifies; `update` moves off it when a compatible non-yanked
release exists.

After candidate selection, the consumer fetches all four exact release
artifacts and calls full Package Trust verification. The authenticated manifest
is parsed from the verified bytes with `parse_manifest_exact`; it is never
silently reread from a mutable filesystem path. Transitive dependency
discovery therefore happens only after the containing release has been fully
authenticated.

### Lockfile v2

Any graph containing a registry dependency requires `axiom.lock` version 2.
Version 1 remains valid for path-only graphs. The v2 TOML contract contains:

- `[compatibility]`: `contract`, `compiler`, and the recorded
  `edition_policy`.
- `roots`: sorted unique package IDs for every entry package whose dependency
  closure forms the graph, including virtual-workspace members.
- `[[registry]]`: manifest-local `name`/`source`, authenticated
  `registry_identity`/`source_identity`, exact `trust_roots_sha256` and
  `expectation_sha256`,
  `current_root_version`/`current_root_sequence`/
  `current_root_transcript_sha256`, exact `index_sha256`/
  `index_transcript_sha256`, `index_generation`/`index_sequence`,
  `index_snapshot_id`, and sorted unique `index_signer_key_ids`.
- `[[package]]`: required `id`, `name`, `version`, `source`, and
  `compatibility`. Registry records additionally require `registry`,
  `namespace`, `archive_sha256`, `archive_length`, `manifest_sha256`,
  `provenance_sha256`, `package_signature_sha256`, `publisher_identity`,
  `verification_sha256`, sorted unique `signer_key_ids`, exact `cache_key`, and
  `yanked_at_resolution`; path records must omit every registry evidence field.
- `[[edge]]`: from/to package IDs, dependency alias, requested constraint,
  path or registry source kind, and one closed stable reason:
  `root_path_constraint`, `transitive_path_constraint`,
  `highest_compatible`, `exact_locked_replay`, or
  `trusted_yanked_locked_replay`.

Parsing is strict and rejects unknown fields, duplicate identities, malformed
versions or digests, and inconsistent graph edges. Lockfile replacement is an
atomic write. `--locked` never performs resolution or changes the file; a
missing, v1, stale, or altered lock for a registry graph is an error.

The Rust boundary preserves version identity instead of decoding into a common
lossy struct: `parse_lockfile_exact` and `load_lockfile` return
`ParsedLockfile::V1` or `ParsedLockfile::V2`; `render_lockfile_v2`,
`validate_lockfile_v2`, and `write_lockfile_v2_atomic` own v2 persistence;
`validate_lockfile_version_for_manifest` enforces the registry/v2 gate; and
`canonical_path_package_id`/`canonical_registry_package_id` define stable node
IDs.

### Cache, offline mode, and vendoring

Registry packages enter the build graph only through verified immutable
material. Archives are limited to 64 MiB, 4,096 files, 16 MiB per file,
1,024-byte paths, and 64 path components. Absolute or parent paths, duplicate
entries, symlinks, unsupported entry types, and extraction outside the
transaction root are rejected.

The content-addressed store shares immutable archive content by authenticated
archive digest: the exact blob lives under `blobs/sha256/<archive-digest>` and
the extracted tree under
`trees/axiom-package-extractor-v1/sha256/<archive-digest>`. Package Trust and
tree-integrity evidence lives under
`evidence/sha256/<archive-digest>/<evidence-identity>/`, where the evidence
identity binds the exact registry-index and verification-document digests.
Atomic admission records live under
`commits/sha256/<archive-digest>/<registry-index-digest>/<evidence-identity>.json`.
The `axiom.package_tree_integrity.v1` record binds the extractor version and a
path-sorted list of every file length and SHA-256 digest. Cache hits are
selected by the lock's exact archive, registry-index, and verification digests
and revalidated against the complete evidence and commit records; a matching
directory name is not sufficient proof.

`--locked --offline` performs no registry request and has no network fallback.
It succeeds only when every locked release can be reconstructed from intact
cache or vendor material and still passes the locked digest, identity, trust,
and compatibility checks. Missing files, modified bytes, symlinks, traversal,
duplicate archive paths, or stale trust/index pins fail closed.

Vendoring copies the same verified immutable package material into a
project-controlled snapshot under
`snapshots/sha256/<vendor-manifest-digest>/`, then atomically replaces
`CURRENT`. Within one snapshot, packages sharing an archive reuse
`packages/sha256/<archive-digest>/{archive,tree}` while exact Package Trust
evidence and commits remain isolated by evidence identity. Its canonical
`axiom.vendor_manifest.v1` sorts package identities and binds every content
key, archive digest, registry-index digest, verification digest, evidence
identity, and tree-manifest digest. A vendor tree is not a trust bypass:
locked and offline consumers reverify it exactly as they reverify the shared
cache. Local path dependencies remain paths and are never copied into the
registry store or vendor tree.

The stable operator surface is:

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- pkg fetch <path> --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- pkg update <path> --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- pkg update <path> --package core --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- pkg vendor <path> --out vendor --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- pkg graph <path> --json
```

`fetch` preserves every selection in an existing valid v2 lock while
authenticating and caching its exact pins. For a registry graph with no lock or
only v1, it performs the initial deterministic resolution and atomically writes
v2. `update` explicitly re-resolves and atomically replaces v2; `--package`
unlocks only that direct dependency while freezing all other selections, and
fails with a deterministic conflict when the requested change needs a broader
update. `vendor` materializes the locked graph at the configured vendor root or
`--out` override. Machine-readable reports include the deterministic resolver
trace, trust decision, cache/vendor disposition, and resulting package graph.

The issue-level qualification gate is:

```bash
make stage1-package-resolver
```

It covers exact/caret and transitive resolution, conflicts and yanks, local
HTTP fixture fetch, Package Trust tamper/replay rejection, content-addressed
cache admission, locked-offline and vendor round trips, package-graph
inspection, and supply-chain integration. Public hosted-registry operation
remains outside this gate.

## Publish metadata

Package identity remains the `[package]` name/version pair. `[publish]` is
optional metadata consumed by the separate local publication flow:

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

Resolver v1 does not turn `[publish]` into an upload client and does not
implement a public hosted registry.
