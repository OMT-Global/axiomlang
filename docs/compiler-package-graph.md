# Compiler Package Graph Boundary

`compiler.package_graph` is the AxiOM-owned package identity and dependency
surface for the self-hosted compiler path. It resolves packages from
`axiom.toml`, `axiom.lock`, source files, authenticated static-registry
metadata, and verified cache or vendor material. Cargo is developer scaffolding
for the current Rust-hosted compiler only; Cargo metadata is not package truth.

## Contract

The package graph accepts these inputs:

- Root package directory.
- Root `axiom.toml`.
- Root `axiom.lock`.
- Local workspace member manifests.
- Local path dependency manifests.
- Exact authenticated Registry Index v2 bytes, trust roots, and verification
  expectation named by the root manifest.
- Exact registry package archive, manifest, provenance, and signature bytes
  admitted through Package Trust v1.
- Content-addressed cache or vendor evidence matching lockfile v2.
- Package source files reachable from each manifest entrypoint and imports.

It produces a stable `axiom.compiler.package_graph.v1` envelope with:

- The root path, manifest path, and lockfile path.
- One package node for every locked root, workspace, local dependency, and
  registry dependency package.
- Package identity from `[package].name`, `[package].version`, and the locked
  source string.
- Workspace membership and dependency edges from `axiom.toml` and lockfile v2.
- Build entrypoint and output directory from `[build]`.
- Lockfile integrity data matching the checked-in `axiom.lock`.
- For registry nodes, the requested/selected version, authenticated source and
  signer evidence, yank-at-resolution state, cache/vendor disposition, and
  stable resolver decision.
- Hash inputs needed by build caches, release evidence, and snapshot bootstrap.

The graph must not read `Cargo.toml`, `Cargo.lock`, `cargo metadata`, or
`stage1/Cargo.lock` to decide AxiOM package identity. The Rust implementation may
call Rust code to compute the graph during the bootstrap period, but the
observable contract must stay expressible in AxiOM package terms.

`axiomc pkg graph --json` exposes the materialized compiler view through the
separate `axiom.compiler.package_graph.runtime.v1` contract. That runtime
contract preserves canonical package IDs, locked sources, resolver edge
decisions, lockfile evidence, Package Trust signer evidence, and cache/vendor
materialization identities without pretending to be the static boundary
envelope above.

## Package Nodes

Each package node has:

- `name`: the manifest package name.
- `version`: the manifest package version.
- `source`: the lockfile source, such as `path` or `path:members/core`.
- `root`: the package root relative to the repository root.
- `manifest`: the package manifest relative to the repository root.
- `lockfile`: the lockfile used for that package graph evaluation.
- `entry`: the manifest build entrypoint relative to the package root.
- `out_dir`: the manifest build output directory relative to the package root.
- `workspace_members`: local workspace members declared by that package.
- `local_dependencies`: dependency name/path edges declared by that package.
- Registry dependency data, when applicable: canonical registry/source
  identity, namespace, selected version, archive digest, publisher/signers,
  trust decision, and cache or vendor evidence.
- Resolver decisions: requested constraint, explicit selected version and
  package ID, source kind, stable reason, and candidate/yank disposition.

The root package appears first. Remaining package nodes are sorted by locked
source and then package name so independent implementations can compare graphs
deterministically. Resolver coordinates are canonical
`(registry_identity, source_identity, namespace, package)` tuples sorted
lexically. Candidate versions use strict release SemVer descending, with
release ID and target path as lexical tie-breakers; duplicate
coordinate/version entries are rejected. One selected version is allowed per
canonical coordinate.

## Lockfile Integrity

`axiom.lock` remains the package graph integrity source. Version 1 is the
path-only format. A graph containing registry dependencies requires version 2,
whose compatibility, explicit sorted root IDs, registry, package, and edge
records bind the full selection and trust evidence. Explicit roots preserve
virtual and multi-member workspace entry sets without inventing dependency
edges. Official builds must reject missing, malformed, version-inappropriate,
or stale lockfiles in locked/offline modes before source lowering or backend
selection. A graph fixture is valid only when its roots, package identities,
and edges exactly match the decoded lockfile.

Fresh resolution excludes yanked releases. A locked graph may replay a release
that was subsequently yanked only when every exact digest, signer, trust-root,
index-transcript, and release pin still verifies; an update moves away from it
when a compatible non-yanked release exists. Conflict, yank, work-budget, and
backtrack outcomes are deterministic and appear in the resolver trace.

`--locked --offline` is a strict no-network mode. Every registry node must be
recoverable from intact content-addressed cache or vendor evidence and must
reverify against lockfile v2. There is no registry fallback and no
authenticate-then-reread path. Once materialized, registry manifests and
reachable `.ax` sources are consumed from the exact authenticated archive bytes
carried into a bounded in-memory compiler view; later cache or vendor path
mutation cannot change compiler input.

The current Rust-hosted compiler already hashes the lockfile into build cache
keys. The self-hosted graph must preserve that behavior: cache keys and release
evidence are invalid unless they bind the manifest hash, lockfile hash, and
source hashes for the same package graph.

## Release-Chain Boundary

For #931, a previously released `axiomc` snapshot must be able to read this
contract and build the next compiler without invoking Cargo. Until that release
chain exists:

- Cargo may run local developer commands that host the stage1 compiler.
- Cargo-vet and `stage1/Cargo.lock` may remain part of the temporary
  Rust-hosted supply-chain gate.
- Official package identity must still come from `axiom.toml` and `axiom.lock`.

The release-chain evidence for package loading must include:

- The package graph fixture validated by
  `make stage1-package-graph-boundary`.
- Package Resolver v1 fixtures validated by `make stage1-package-resolver`,
  including exact/caret/transitive selection, conflict/yank/tamper/replay,
  fetch-to-cache, locked-offline, vendor, and package-graph round trips.
- A successful lockfile validation path for root, workspace, local dependency,
  and registry dependency packages.
- Build cache evidence that includes the lockfile hash.
- Supply-chain output for any host tooling used by the temporary bootstrap
  compiler.

## Fixture

The checked fixture lives at:

- `stage1/compiler-contracts/schemas/axiom.compiler.package_graph.v1.schema.json`
- `stage1/compiler-contracts/snapshots/package-graph.json`
- `stage1/compiler-contracts/schemas/axiom.compiler.package_graph.runtime.v1.schema.json`
- `stage1/compiler-contracts/snapshots/package-graph-runtime.json`
- `stage1/schemas/axiom-lockfile-v2.schema.json`
- `stage1/schemas/axiom-package-resolution-v1.schema.json`
- `stage1/package-resolver/fixtures/lockfile-v2.json`
- `stage1/package-resolver/fixtures/resolution-v1.json`

The local validator is:

```bash
make stage1-package-graph-boundary
make stage1-package-resolver
```

The boundary target validates the schema envelope, compares fixture package
identity with `stage1/examples/workspace/axiom.lock`, checks the fixture against
`stage1/examples/workspace/axiom.toml`, rejects Cargo-derived fields inside the
graph output, and validates the registry-manifest, lockfile-v2, and ordered
resolution-trace fixtures. The resolver target executes the local
static/loopback registry, authenticated cache/vendor, locked-offline, and
package-graph round trips. Neither target claims a public hosted registry or
package edition selection.
