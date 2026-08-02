# Stage1 Supply Chain Gate

The stage1 supply-chain gate is the repo-local `make supply-chain` target and
the matching `Toolchain Supply Chain` GitHub Actions workflow. It is the closure
surface for lockfile integrity, signed package verification when applicable,
offline dependency verification, reproducible release build inputs, and SBOM
emission.

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

## Runner Contract

The workflow installs Node.js only when `package-lock.json` exists. That keeps
the signed-package verification path active for npm work while avoiding unused
Node tool extraction on self-hosted runners for the current Rust-only stage1
graph.

Hosted registry service ownership and external trust-root operation remain
outside this gate and are tracked separately by the hosted-registry roadmap
issue. The local static registry verifies package authenticity and serves only
an immutable in-memory snapshot of already verified bytes.
