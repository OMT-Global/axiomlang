# Build lowering evidence

`axiomc build --json` reports a `lowering` object conforming to
`stage1/schemas/axiom-build-lowering-evidence-v1.schema.json`. The evidence is
also stored in the version 2 build cache and repeated for each package, so an
agent can distinguish how an existing artifact was produced without executing
it.

## Execution-mode matrix

This is the canonical matrix for the supported Cranelift CLI path. The examples
below describe the current native host build; inspect each new program rather
than inferring its mode from similar syntax or an imported module.

| Outcome | Representative package | `lowering.execution_mode` | `lowering.lowering_mode` | What the artifact proves |
| --- | --- | --- | --- | --- |
| Direct runtime | `stage1/examples/hello`, `stage1/examples/stdlib_env` | `direct_native_runtime` | `direct_native_runtime` or `direct_native_runtime_with_static_folds` | The emitted binary executes the supported native lowering. The hybrid mode also contains known-value folds. |
| Bounded static output | `stage1/examples/arrays`, `stage1/examples/maps` | `bounded_static_output` | `bounded_static_output` | The compiler evaluated a bounded, effect-free program and emitted an artifact that replays its output. This does not prove runtime arrays or maps. |
| Blocked | `stage1/examples/capabilities` | `not_produced` | `runtime_lowering_required` | No executable was produced. The command fails with `error.code = backend.runtime_lowering_required`. |

Inspect the complete structured envelope, including on failure:

```sh
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/stdlib_env --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/arrays --json
# Expected nonzero exit; the JSON explains the unsupported lowering.
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/capabilities --json
```

For runtime-sensitive evidence, build `stdlib_env` once and execute the emitted
`binary` path with `__AXIOM_STAGE1_MISSING__=first`, then with
`__AXIOM_STAGE1_MISSING__=second`. Its output must change without rebuilding.
Successful builds, native machine code, and `generated_rust: null` alone do not
distinguish the first two rows.

The three AG5 fixtures (`proof_cli`, `proof_worker`, `proof_http_service`) remain
blocked in the current proof harness. Their expected failures do not establish
an executable milestone; the proof contract decision remains open in
[#1657](https://github.com/OMT-Global/axiomlang/issues/1657).

## Field meanings

The execution and lowering modes have deliberately narrow meanings:

- `direct_native_runtime` means the emitted binary executes direct-native
  runtime lowering.
- `direct_native_runtime_with_static_folds` is the hybrid direct-native mode:
  the binary executes at runtime but contains compiler-proven known-value
  folds.
- `bounded_static_output` means an effect-free, bounded program was reduced to
  deterministic output. It is not runtime-lowering evidence.
- `generated_rust_compatibility` identifies internal legacy compatibility
  evidence. It is not a supported CLI backend selection.
- `runtime_lowering_required` is emitted on a fail-closed native build.
  Its execution mode is `not_produced` because no executable artifact exists.

`legacy_fallback_attempted: true` has one precise interpretation: native
lowering did not accept the program, selection reached the former legacy
evaluator fallback, and the compiler blocked that selection before evaluator
execution. It does not mean that the evaluator ran. Other build failures omit
lowering evidence because they did not establish that boundary.

Validate the schema and deterministic success/failure fixtures with:

```sh
cargo test --manifest-path stage1/Cargo.toml -p axiomc --test json_command_fixtures
```
