# Stage1 Supply Chain Gate

The stage1 supply-chain gate is the repo-local `make supply-chain` target and
the matching `Toolchain Supply Chain` GitHub Actions workflow. It is the closure
surface for lockfile integrity, signed package verification when applicable,
offline dependency verification, reproducible release build inputs, and SBOM
emission.

Package Resolver v1 has an additional issue-level qualification target:

```bash
make stage1-package-resolver
```

Run both targets for resolver changes. The resolver target validates AxiOM
lockfile v2 and authenticated package material; `make supply-chain` continues
to validate the temporary Rust-hosted implementation and its Cargo dependency
closure.

This gate verifies the temporary Rust-hosted toolchain. AxiOM package identity
is still defined by `axiom.toml` and `axiom.lock`, not Cargo metadata. The
package graph boundary fixture is checked separately with:

```bash
make stage1-package-graph-boundary
```

## Local Command

```bash
make supply-chain
```

The target runs `scripts/ci/run-toolchain-supply-chain.sh`.

## Verified Surface

- `cargo fetch --manifest-path stage1/Cargo.toml --locked` proves dependency
  resolution does not drift outside `stage1/Cargo.lock`.
- `cargo metadata --manifest-path stage1/Cargo.toml --format-version 1 --locked
  --offline` proves the locked graph can be inspected without network access.
- `cargo vet --manifest-path stage1/Cargo.toml --locked --frozen` enforces the
  pinned cargo-vet policy and imports under `stage1/supply-chain/`.
- When the repository root has `package-lock.json`, the gate runs
  `npm ci --ignore-scripts --no-audit --no-fund` followed by
  `npm audit signatures` so signed npm packages are verified without lifecycle
  script execution.
- The release build runs with `SOURCE_DATE_EPOCH` and a
  `--remap-path-prefix` `RUSTFLAGS` entry so build metadata does not depend on
  the runner's absolute checkout path or wall clock.
- `scripts/ci/emit-stage1-sbom.py` emits an SPDX JSON document at
  `stage1/target/sbom/stage1.spdx.json`, and CI uploads that file as the
  `stage1-sbom` artifact.
- `make stage1-package-trust-contract` and its regression target validate the
  RFC 8032 Ed25519 + SHA-256 transcript, threshold trust/root and index
  metadata, package publication floors, exact current-index/offline pins,
  provenance bindings, and negative vectors. The
  `publish` and `registry-index` commands consume protected Ed25519 seed files
  through repeatable `--signing-key-file` arguments; `registry-validate` and
  `registry-serve` consume only public trust inputs and perform full artifact
  verification. Legacy HMAC sidecars are rejected. See
  [Package Trust v1 Contract](package-trust-v1.md).
- `make stage1-package-graph-boundary` proves the self-hosting package graph
  fixture is derived from `axiom.toml` and `axiom.lock` and rejects
  Cargo-derived graph outputs.
- `make stage1-package-resolver` proves exact/caret and transitive selection,
  deterministic conflict/yank behavior, trusted local fixture fetch,
  content-addressed cache admission, locked-offline and vendor round trips,
  resolver traces, and package-graph trust/cache decisions.

## Package Resolver Supply-Chain Boundary

Resolver candidates come only from an `AuthenticatedRegistryCatalog` produced
from exact Registry Index v2 bytes. Index, root-transition, expiry, rollback,
replay, identity, duplicate-coordinate, and duplicate-target failures are
rejected before selection. Selected package bytes then pass full Package Trust
verification before manifest parsing or extraction.

The current registry transport is intentionally limited to regular
non-symlink `file://` inputs and numeric-loopback `http://` fixtures. Redirects,
chunked/transfer encoding, non-identity content encoding, ambiguous lengths,
oversized or truncated responses, and non-loopback HTTP hosts fail closed.
Public HTTPS transport and hosted registry operation are outside the v1 local
qualification surface.

The package archive and store enforce these bounds:

- archive size: 64 MiB;
- individual file size: 16 MiB;
- file count: 4,096;
- archive path: 1,024 bytes and 64 components;
- no absolute paths, `..`, duplicates, symlinks, unsupported entry kinds, or
  extraction outside the transaction root.

The content-addressed layout is:

```text
blobs/sha256/<archive-digest>
trees/axiom-package-extractor-v1/sha256/<archive-digest>/
evidence/sha256/<archive-digest>/<evidence-identity>/
  manifest
  provenance
  signature
  registry-index
  verification
  integrity.json
commits/sha256/<archive-digest>/<registry-index-digest>/<evidence-identity>.json
```

`integrity.json` is an `axiom.package_tree_integrity.v1` record binding the
extractor version, archive digest, and a path-sorted list of extracted
file lengths and SHA-256 digests. Installation writes a temporary transaction,
verifies the complete tree, and publishes the commit marker last. Offline cache
use selects the exact archive, registry-index, and verification digests from
lockfile v2, then verifies the blob, tree, isolated evidence identity, and
commit again; directory presence is never treated as trust.

Vendor snapshots use
`snapshots/sha256/<vendor-manifest-digest>/packages/sha256/<archive-digest>/`
plus a canonical `axiom.vendor_manifest.v1` and an atomically replaced
`CURRENT` pointer. Shared archive/tree bytes remain keyed by archive digest;
per-verification evidence and commits are isolated below `evidence/` and
`commits/` by the evidence identity. The manifest sorts package identities and
binds each content key, archive digest, registry-index digest, verification
digest, evidence identity, and tree-manifest digest. Vendor material is
reverified against lockfile v2 and Package Trust evidence in offline mode.
Local path dependencies are preserved as paths rather than copied into the
registry store.

Publication is reader-aware: `CURRENT` replacement and snapshot reclamation
share an atomic lifecycle lock, while locked consumers hold active-reader
leases for the duration of their snapshot traversal. The current snapshot and
leased older snapshots are retained; other completed snapshots are reclaimed
with deterministic lifecycle evidence. A verified snapshot published before a
process failure is adopted on the next matching run, so recovery never needs
to copy the package content again.

Fresh resolution excludes yanked releases. A locked replay may retain a newly
yanked release only when every exact trust, digest, transcript, and compatibility
pin still verifies. This narrow replay rule supports reproducibility without
allowing a fresh resolver to select a yanked package.

## Runner Contract

The workflow installs Node.js only when `package-lock.json` exists. That keeps
the signed-package verification path active for npm work while avoiding unused
Node tool extraction on self-hosted runners for the current Rust-only stage1
graph.

Hosted registry service ownership and external trust-root operation remain
outside this gate and are tracked separately by the hosted-registry roadmap
issue. The local static registry verifies package authenticity and serves only
an immutable in-memory snapshot of already verified bytes. Package Resolver v1
does not claim public registry availability, external network transport, or
edition selection.
