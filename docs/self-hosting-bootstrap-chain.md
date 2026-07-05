# Self-Hosting Snapshot Bootstrap Chain (Design)

Design deliverable for [#1253](https://github.com/OMT-Global/axiomlang/issues/1253).
This document defines how a previously shipped `axiomc` binary snapshot builds
the next working `axiomc` without invoking Cargo, what artifact the chain
pins, how the chicken-and-egg is broken, and what CI gate proves the chain
holds. It is a design only: nothing in this document changes readiness gates,
removes Rust or Cargo behavior, or authorizes source migration. Those remain
governed by [#721](https://github.com/OMT-Global/axiomlang/issues/721) and
`make rust-exit-readiness`.

## Terms

- **Snapshot**: a released, frozen `axiomc` executable for a specific host
  target, plus its recorded provenance. Snapshots are outputs of a prior
  release, never rebuilt in place.
- **Genesis snapshot**: the first snapshot in the chain. It is produced by the
  Rust bootstrap (Cargo + `rustc`) exactly once, and recorded as such.
- **Chain step**: `snapshot(N-1) + compiler sources(N) -> axiomc(N)`.
- **Fixpoint check**: `axiomc(N) + compiler sources(N) -> axiomc(N')` with
  `N == N'` under normalized comparison.

## Artifact format

A snapshot is a plain executable plus a manifest entry. No archive format or
installer is introduced. The manifest is the trust anchor:

`stage1/snapshots/manifest.json` (schema `axiom.selfhost.snapshot_manifest.v0`):

```json
{
  "schema_version": "axiom.selfhost.snapshot_manifest.v0",
  "snapshots": [
    {
      "version": "<released axiomc version>",
      "target": "<host triple, e.g. aarch64-apple-darwin>",
      "sha256": "<hex digest of the executable>",
      "source": "<release URL or registry locator>",
      "built_by": "cargo | axiomc-snapshot",
      "provenance": "<release provenance record locator>"
    }
  ]
}
```

Rules:

- `built_by: "cargo"` is legal **only** for the genesis snapshot of each
  target. Every later entry must be `axiomc-snapshot` and must name the
  predecessor version it was built by in its provenance record.
- The manifest is checked into the repository; updating it is a reviewed PR
  like any other evidence change.
- Snapshots are host-target-specific. The chain is proven per target; targets
  without a genesis snapshot simply have no chain yet.

## Where the frozen snapshot lives

Snapshot executables are attached to GitHub Releases of this repository (the
`source` locator), never committed to git. The manifest pins the digest, so CI
verifies what it downloads. Local developers can place a pre-downloaded
snapshot at `.axiom-build/snapshots/<version>/<target>/axiomc` to run the gate
offline; the digest check is identical in both paths.

## Breaking the chicken-and-egg

The circularity ("you need an `axiomc` to build `axiomc`") is broken exactly
once per target, explicitly and auditably:

1. The genesis snapshot is built by the existing Rust bootstrap and released
   with `built_by: "cargo"` recorded in the manifest. This is the only point
   where Cargo participates in the official chain.
2. Every subsequent release must be produced by the chain step from the
   previous manifest entry. The release workflow refuses to attach a new
   snapshot whose provenance does not name a manifest predecessor.
3. Trust does not regress: once a target has a genesis entry, adding another
   `built_by: "cargo"` entry for that target is a manifest-validation error.
   (Re-bootstrapping after a chain break is a maintainer decision that edits
   the manifest in a reviewed PR, with the old chain retired.)

The chain therefore never requires Cargo after genesis, while remaining honest
that the genesis binary is Rust-built. "Rust is not the product" is achieved
when the compiler *sources* being built by the chain are AxiOM — the chain
design is independent of how much of the compiler has been ported and can be
proven first with the current sources' build being driven end-to-end by the
snapshot.

## What the chain step builds

The chain step is `snapshot/axiomc build <compiler package> --locked --offline`
followed by the compiler's own test surface via `snapshot/axiomc test`. Today
no compiler component is an AxiOM package, so the chain step has nothing real
to build; the feasibility spike (`stage1/selfhost/compiler-diagnostics-spike`,
[#1367](https://github.com/OMT-Global/axiomlang/pull/1367)) is the interim
stand-in: the gate can prove `snapshot builds and runs the spike with
generated_rust null` before any real component migrates. As packages port
(migration order in `docs/axiom-compiler-source-layout.md`), the chain step's
build target grows from the spike to the real compiler package set, and the
fixpoint check becomes meaningful.

## CI gate

A new non-blocking gate, `make snapshot-bootstrap-readiness`, structured like
the existing readiness gates (JSON verdict, explicit blockers, honest
`ready: false` until real):

| Check | Passes when |
|---|---|
| `manifest_valid` | manifest parses, digests are well-formed, genesis rules hold |
| `snapshot_available` | pinned snapshot for the CI host target downloads (or is cached) and matches its sha256 |
| `snapshot_builds_axiom_sources` | the snapshot builds the designated AxiOM package set (`--locked --offline`, no Cargo in the invocation tree) with `generated_rust: null` |
| `snapshot_output_verified` | the built artifacts pass their parity/conformance surface (initially `make self-hosting-spike-parity` retargeted at the snapshot-built binary) |
| `fixpoint_holds` | once the compiler itself is buildable: rebuild-with-self produces a normalized-identical binary |
| `no_cargo_in_chain` | the gate's own process tree evidence shows no `cargo`/`rustc` invocation after genesis verification |

Gate wiring mirrors `rust-exit-readiness`: a `-github` variant validates live
issue state for release/deletion PRs; the plain variant runs offline. The
"Snapshot bootstrap" row in `docs/rust-exit-readiness.md` flips from `blocked`
to `implemented` only when this gate is green **and** #721's governing review
accepts it — the gate is evidence, not permission.

## Non-goals of this design

- No cross-compilation story: each target chains independently.
- No reproducible-build guarantee beyond the fixpoint normalization
  (timestamps and paths are normalized; bit-identical output is a goal, not a
  requirement, for v0).
- No registry or distribution changes: GitHub Releases is sufficient for v0.
- No schedule coupling: porting order stays with
  `docs/axiom-compiler-source-layout.md` and the language gaps in
  `docs/self-hosting-language-gaps.md` ([#1366](https://github.com/OMT-Global/axiomlang/issues/1366)).

## Acceptance path

1. Maintainer accepts this design under #1253 (explicit decision point).
2. Follow-on issues (Pheidon-scoped): manifest schema + validator; genesis
   snapshot release for the primary CI target; `snapshot-bootstrap-readiness`
   gate implementation; retarget spike parity at the snapshot-built binary.
3. #721 consumes the gate as one of its closure requirements.
