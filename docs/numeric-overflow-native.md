# Direct-native signed addition overflow

This document records the supported signed addition slice of [issue #1659](https://github.com/OMT-Global/axiomlang/issues/1659). It applies the existing [Stage1 numeric overflow policy](stage1.md#numeric-overflow-policy) to the direct-native Cranelift path; it does not close the broader numeric-operations contract.

## Contract

Ambient signed integer `+` reports a structured runtime diagnostic on overflow in debug builds and wraps in release builds. This build-mode distinction is retained intentionally for compatibility with the documented Stage1 contract. Programs that require profile-independent arithmetic should use explicit numeric helpers where their backend lowering is supported.

The full-width signed types `int` (the `i64` alias), `i64`, and `isize` now receive the same debug check as the narrower signed types. A signed-overflow instruction detects the full-width carry across the sign boundary before execution can continue with a wrapped value. A range check of the already-wrapped result cannot detect that condition.

Overflow diagnostics retain the existing JSON-line shape:

```json
{"kind":"runtime","message":"numeric overflow: i64 addition"}
```

`int` and `i64` use `i64` in the message; `isize` uses `isize`. The process terminates unsuccessfully after writing the diagnostic. The operating-system signal or numeric exit code is not part of this contract.

Unsigned addition remains wrapping in both build modes. This repair does not change explicit wrapping, checked, or saturating methods.

## Runtime evidence

The integration test `cranelift_numeric_overflow` builds each program explicitly with `--backend cranelift --json` in both debug and release configurations. It requires `execution_mode = direct_native_runtime`, `generated_rust = null`, and no static-fold or evaluator-fallback evidence.

Each built binary runs twice with different stdin. The byte count from `std/io.ax` supplies the runtime operand, so the compiler cannot substitute a known overflow result. The tests cover both `MAX + count` and `MIN + (0 - count)` for `int`, `i64`, and `isize`, with `i8` and `i32` regression controls. The matrix compiles 20 native artifacts and executes 40 runtime cases.

| Mode | Runtime input | Arithmetic result | Process result | stderr |
| --- | --- | --- | --- | --- |
| Debug | Empty stdin | Boundary value, no overflow | Exit 0 | Empty |
| Release | Empty stdin | Boundary value, no overflow | Exit 0 | Empty |
| Debug | One byte | Signed addition overflows | Unsuccessful termination | Structured numeric-overflow diagnostic |
| Release | One byte | Wraps to the opposite signed bound | Sentinel exit 42 | Empty |

The matrix passed on macOS arm64 on 2026-09-07: 20 native builds and 40 binary executions. The sentinel is test instrumentation confirming the wrapped value, not a language runtime exit code. All cases have empty stdout.

Run the native matrix and the existing narrow-width regression with:

```sh
cargo test --manifest-path stage1/Cargo.toml -p axiomc --test cranelift_numeric_overflow --locked --offline
cargo test --manifest-path stage1/Cargo.toml -p axiomc --lib --locked --offline tests::stage1_numeric_overflow_policy_checks_signed_debug_and_wraps_release -- --exact
```

## Remaining issue scope

Issue #1659 remains open for an explicit cross-backend contract and runtime matrix for subtraction, multiplication, unary negation, division edge cases, and explicit wrapping/checked operations. Those operations must not be assigned new semantics merely by extrapolating this addition repair. Broader profile-policy changes require a separate language decision.
