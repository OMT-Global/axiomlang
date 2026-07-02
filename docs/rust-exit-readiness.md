# Rust Exit Readiness

This matrix defines the technical bar for making Rust and Cargo unnecessary for
the supported Axiom toolchain.

The current implementation still includes the Rust-hosted `stage1/axiomc`
compiler, but generated Rust is no longer a supported backend path. Rust exit is
complete only when the official check, build, run, test, documentation, LSP, and
release paths can operate from AxiOM-owned sources and direct native artifacts
without requiring Cargo, generated Rust, or `rustc`.

Final Rust bootstrap issue: [#721](https://github.com/OMT-Global/axiomlang/issues/721)

## Readiness Command

`make rust-exit-readiness` runs the non-blocking local readiness check:

```bash
make rust-exit-readiness
```

It emits `axiom.rust_exit.readiness.v1` JSON and fails while blocker issues in
`docs/rust-exit-readiness.json` are open or unavailable, while the
machine-readable direct-native runtime ABI reports `ready: false`, or while the
source-level Rust-capture gates still find supported toolchain paths owned by
Rust-only drivers. Deletion or release-chain PRs can require live GitHub state:

```bash
make rust-exit-readiness-github
```

The readiness gate is an evidence surface, not permission to remove Rust by
itself. It uses the manifest, the direct-native ABI contract, self-hosted
command/MIR boundary fixtures, and live issue state; this Markdown page is
descriptive evidence only. Closing #721 also requires the governing issues and
review gates to be satisfied.

## Backend Matrix

| Surface | Required state | Current disposition | Governing issue |
| --- | --- | --- | --- |
| Direct native parity matrix | Every supported stage1 surface has a direct-native status row and linked blocker when incomplete. | Implemented as the checked runtime ABI matrix; no runtime ABI rows remain incomplete. | [#927](https://github.com/OMT-Global/axiomlang/issues/927) |
| Direct native runtime ABI | Supported values, ownership shapes, stdlib calls, and capability host calls lower through backend-neutral direct-native runtime entrypoints. | Implemented and checked by `scripts/ci/check-direct-native-runtime-abi.py`; LSP/tooling and final bootstrap gates remain separate blockers. | [#1124](https://github.com/OMT-Global/axiomlang/issues/1124) |
| Direct native diagnostics and evidence | Direct native builds preserve source diagnostics, provenance, debug manifests, and operator evidence without generated Rust. | Implemented for the Cranelift direct-native spike; broader coverage remains gated by default-backend blockers. | [#929](https://github.com/OMT-Global/axiomlang/issues/929) |
| Full lib-suite and backend parity gate | The full `axiomc --lib --features run-native-tests` suite is triaged, environment-gated cases are separated, direct-native failures are repaired or explicitly linked to blockers, and parity evidence is green before final Rust removal. | Blocked by [#1255](https://github.com/OMT-Global/axiomlang/issues/1255). The current triage is checked by `make stage1-full-lib-triage`; it records 1 known full-suite failure (an environment-gated proof workload needing loopback networking and the wasm32-wasip1 target) and keeps the gap visible without pretending the suite is green. | [#1255](https://github.com/OMT-Global/axiomlang/issues/1255) |
| Default backend | `axiomc build` defaults to direct native output and no longer invokes `rustc` for supported broad builds. | Host/native builds default to the direct-native Cranelift backend; default targeted builds fail closed instead of falling back to generated Rust, and extended conformance now runs on Cranelift with `generated_rust: null`. | [#1191](https://github.com/OMT-Global/axiomlang/issues/1191) |
| Generated-Rust removal | The generated-Rust backend and `--backend rust` compatibility path are removed after a release with direct native as default. | Completed for the supported toolchain in #1191. The CLI parser no longer accepts `--backend generated-rust` or the old `--backend rust` transition alias, targeted builds fail closed instead of using generated Rust, and command/schema fixtures no longer model generated Rust as supported output. | [#1191](https://github.com/OMT-Global/axiomlang/issues/1191) |

## Bootstrap Matrix

| Surface | Required state | Current disposition | Governing issue |
| --- | --- | --- | --- |
| AxiOM compiler source layout | Parser, checker, lowering, MIR, backend selection, diagnostics, packages, manifests, lockfiles, and command dispatch have AxiOM package boundaries. | Implemented as [AxiOM Compiler Source Layout and Self-Hosting Boundary](axiom-compiler-source-layout.md); final source migration remains governed by the Rust bootstrap gate. | [#721](https://github.com/OMT-Global/axiomlang/issues/721) |
| Snapshot bootstrap | A previously shipped `axiomc` snapshot builds the next working `axiomc` binary without invoking Cargo. | `blocked` until the final Rust bootstrap removal gate is satisfied. | [#721](https://github.com/OMT-Global/axiomlang/issues/721) |
| Final readiness gate | The Rust-exit command proves supported workflows, release builds, tests, docs, and LSP no longer require Rust-only infrastructure. | Implemented as `make rust-exit-readiness`; the gate still fails until the live blockers in `docs/rust-exit-readiness.json` are closed and ABI, boundary, generated-Rust, and LSP driver-ownership checks pass. | [#721](https://github.com/OMT-Global/axiomlang/issues/721) |
| Compiler verification | Compiler-internal coverage is expressed in AxiOM property form instead of Rust-only tests. | Shipped through the property-test gate; remaining Rust-bootstrap release-chain work stays with #721. | [#721](https://github.com/OMT-Global/axiomlang/issues/721) |
| Documentation generator and LSP | `axiomc doc`, structured/Markdown output, and `axiomc lsp` protocol handling are produced by AxiOM-owned code. | The LSP stdio harness exists, but the readiness gate keeps failing while `axiomc lsp` still dispatches through the Rust-hosted stdio/message loop. | [#731](https://github.com/OMT-Global/axiomlang/issues/731) |

## Closure Rules

- The direct native backend may replace generated Rust only after the backend
  matrix has no incomplete rows (`partial` or `blocked`).
- Final Rust removal also requires the full lib-suite and backend parity gate in
  #1255 to be green; direct-native ABI readiness alone is not enough while
  `cargo test --manifest-path stage1/Cargo.toml -p axiomc --lib --features
  run-native-tests` has known untriaged or unrepaired failures.
- A direct-native runtime ABI row may be marked `implemented` only when it has
  runtime-entrypoint or backend-emitted codegen evidence; compiler-side
  Cranelift spike evaluation alone is not sufficient.
- #721 may close only after the backend matrix and bootstrap matrix have no
  incomplete rows.
- Generated Rust must stay outside the supported toolchain; regressions that
  reintroduce it as a CLI backend, targeted-build fallback, command fixture, or
  release artifact must fail the readiness gate.
- Cargo may remain as a developer convenience while #931 is being proven, but it
  may not be required by the official release-chain path.
- Any new blocked row must name a GitHub issue in
  `docs/rust-exit-readiness.json`.
- #932 tracks creation of this gate. After #932 closes, the gate must keep
  failing only on the remaining Rust-exit blockers listed in
  `docs/rust-exit-readiness.json`.

## Rust Capture Check

This gate tracks implementation dependencies only. It does not define Axiom
semantics in Rust terms. Direct native, generated Rust, Cargo, and snapshot
bootstrap details are backend or release-chain implementation concerns.
