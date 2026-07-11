# Build lowering evidence

`axiomc build --json` reports a `lowering` object conforming to
`stage1/schemas/axiom-build-lowering-evidence-v1.schema.json`. The evidence is
also stored in the version 2 build cache and repeated for each package, so an
agent can distinguish how an existing artifact was produced without executing
it.

The execution and lowering modes have deliberately narrow meanings:

- `direct_native_runtime` means the emitted binary executes direct-native
  runtime lowering.
- `direct_native_runtime_with_static_folds` is the hybrid direct-native mode:
  the binary executes at runtime but contains compiler-proven known-value
  folds.
- `bounded_static_output` means an effect-free, bounded program was reduced to
  deterministic output. It is not runtime-lowering evidence.
- `generated_rust_compatibility` identifies the explicit compatibility backend.
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
