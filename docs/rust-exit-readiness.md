# Rust Exit Readiness

This matrix defines the technical bar for making Rust and Cargo unnecessary for
the supported Axiom toolchain.

The current supported implementation remains the Rust-hosted `stage1/axiomc`
compiler with generated-Rust backend support. Rust exit is complete only when
the official check, build, run, test, documentation, LSP, and release paths can
operate from AxiOM-owned sources and direct native artifacts without requiring
Cargo, generated Rust, or `rustc`.

Final Rust bootstrap issue: [#721](https://github.com/OMT-Global/axiomlang/issues/721)

## Readiness Command

`make rust-exit-readiness` runs the non-blocking local readiness check:

```bash
make rust-exit-readiness
```

It emits `axiom.rust_exit.readiness.v1` JSON and fails while blocker issues in
`docs/rust-exit-readiness.json` are open or unavailable, or while the
machine-readable direct-native runtime ABI reports `ready: false`. Deletion or
release-chain PRs can require live GitHub state:

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
| Direct native runtime ABI | Supported values, ownership shapes, stdlib calls, and capability host calls lower through backend-neutral direct-native runtime entrypoints. | Implemented and checked by `scripts/ci/check-direct-native-runtime-abi.py`; default-backend, LSP, and final Rust-removal gates remain separate blockers. | [#1124](https://github.com/OMT-Global/axiomlang/issues/1124) |
| Direct native diagnostics and evidence | Direct native builds preserve source diagnostics, provenance, debug manifests, and operator evidence without generated Rust. | Implemented for the Cranelift direct-native spike; broader coverage remains gated by default-backend blockers. | [#929](https://github.com/OMT-Global/axiomlang/issues/929) |
| Default backend | `axiomc build` defaults to direct native output and no longer invokes `rustc` for supported broad builds. | Host/native builds default to the direct-native Cranelift backend; targeted builds still keep generated Rust for compatibility until #1191 is complete. | [#1191](https://github.com/OMT-Global/axiomlang/issues/1191) |
| Generated-Rust removal | The generated-Rust backend and `--backend rust` compatibility path are removed after a release with direct native as default. | Generated Rust remains present as the targeted-build compatibility backend and must leave the supported toolchain before Rust exit completes. | [#1191](https://github.com/OMT-Global/axiomlang/issues/1191) |

## Bootstrap Matrix

| Surface | Required state | Current disposition | Governing issue |
| --- | --- | --- | --- |
| AxiOM compiler source layout | Parser, checker, lowering, MIR, backend selection, diagnostics, packages, manifests, lockfiles, and command dispatch have AxiOM package boundaries. | Implemented as [AxiOM Compiler Source Layout and Self-Hosting Boundary](axiom-compiler-source-layout.md); source migration remains owned by child issues. | [#930](https://github.com/OMT-Global/axiomlang/issues/930) |
| Snapshot bootstrap | A previously shipped `axiomc` snapshot builds the next working `axiomc` binary without invoking Cargo. | `blocked` | [#931](https://github.com/OMT-Global/axiomlang/issues/931) |
| Final readiness gate | The Rust-exit command proves supported workflows, release builds, tests, docs, and LSP no longer require Rust-only infrastructure. | Implemented as `make rust-exit-readiness`; the gate still fails until the rows above and below are complete. | [#932](https://github.com/OMT-Global/axiomlang/issues/932) |
| Compiler verification | Compiler-internal coverage is expressed in AxiOM property form instead of Rust-only tests. | `blocked` | [#562](https://github.com/OMT-Global/axiomlang/issues/562) |
| Documentation generator | `axiomc doc` and structured/Markdown output are produced by AxiOM-owned code. | `blocked` | [#563](https://github.com/OMT-Global/axiomlang/issues/563) |
| LSP server | `axiomc lsp` runs an AxiOM-owned LSP server and protocol stack. | `blocked` | [#564](https://github.com/OMT-Global/axiomlang/issues/564) |

## Closure Rules

- The direct native backend may replace generated Rust only after the backend
  matrix has no incomplete rows (`partial` or `blocked`).
- A direct-native runtime ABI row may be marked `implemented` only when it has
  runtime-entrypoint or backend-emitted codegen evidence; compiler-side
  Cranelift spike evaluation alone is not sufficient.
- #721 may close only after the backend matrix and bootstrap matrix have no
  incomplete rows.
- Generated Rust may remain as a targeted-build compatibility backend while
  #1191 is open, but Rust exit may not complete until it is removed from the
  supported toolchain.
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
