# Stage1 Supply Chain Gate

The stage1 supply-chain gate is the repo-local `make supply-chain` target and
the matching `Toolchain Supply Chain` GitHub Actions workflow. It is the closure
surface for lockfile integrity, signed package verification when applicable,
offline dependency verification, reproducible release build inputs, and SBOM
emission.

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

## Runner Contract

The workflow installs Node.js only when `package-lock.json` exists. That keeps
the signed-package verification path active for npm work while avoiding unused
Node tool extraction on self-hosted runners for the current Rust-only stage1
graph.

Hosted registry service ownership, external trust-root operation, and package
authenticity beyond the local static-publish sidecar remain outside this gate
and are tracked separately by the hosted-registry roadmap issue.
