# Stage1 bootstrap

The Rust bootstrap compiler in `stage1/` is now the supported Axiom toolchain.
The Python `stage0` interpreter, bytecode compiler, bytecode format, bytecode
VM, and disassembler are not supported execution surfaces; see
[Python Exit Parity Gate](python-exit-parity-gate.md) and
[Python Exit VM Disposition](python-exit-vm-disposition.md).

## Current bootstrap scope

The Rust compiler is intentionally small in this bootstrap slice:

- `axiom.toml` and `axiom.lock` are the new manifest and lockfile pair.
- Supported source subset is top-level `import`, `pub const`, `const`, `pub type`, `type`, `pub struct`, `struct`, `pub enum`, `enum`, `pub fn`, `fn`, `let`, `print`, `panic`, `if` / `else`, `while`, statement-level `match`, `return`, variables, bare enum variants, tuple-style enum constructors, named-payload enum constructors, payload-binding match arms, named-payload match arms, `Option<T>`, `Result<T, E>`, `Some`, `None`, `Ok`, `Err`, postfix `?` error propagation on `Option<T>` / `Result<T, E>`, the built-in polymorphic collection helpers `len(...)`, `first(...)`, and `last(...)`, function calls, named struct types, named enum types, generic struct and enum definitions with explicit type arguments, transparent type aliases, scalar `const` declarations with compile-time evaluation, tuple types, tuple literals, tuple indexing, map types, map literals, map indexing, array types, array literals, array indexing, borrowed array slice expressions, borrowed slice types, borrowed slices stored inside named structs and enum payloads, borrowed-return aggregates backed by one or more borrowed parameters, struct literals, field access, `+` on `int`/`string`, and scalar comparisons.
- Stage1 now ships a synthetic standard library surface under the `std/` import prefix with ten landed modules. Six are capability-gated surfaces, one per capability class: `std/time.ax` exposes `Duration`, `Instant`, `duration_ms(ms): Duration`, `now_ms(): int`, `now(): Instant`, `elapsed_ms(start): int`, and `sleep(duration): int` on top of `clock_now_ms`, `clock_elapsed_ms`, and `clock_sleep_ms`; `std/env.ax` exposes `get_env(key: string): Option<string>` on top of `env_get`, `std/fs.ax` exposes `read_file(path: string): Option<string>` on top of `fs_read`, `std/net.ax` exposes `resolve(host: string): Option<string>` on top of `net_resolve`, `std/process.ax` exposes `run_status(command: string): int` on top of `process_status`, and `std/crypto_hash.ax` (the stage1 spelling of `std.crypto.hash`) exposes `sha256(input: string): string` on top of `crypto_sha256`. Each of those six requires the importing package to declare the matching capability (`clock`, `env`, `fs`, `net`, `process`, or `crypto`); environment access is scoped with `env = ["PORT", "LOG_LEVEL"]`, and `env_get` returns `None` for names outside that manifest allowlist. The legacy `env = true` form remains temporarily available but emits a check warning because it grants unrestricted process environment access; `env_unrestricted = true` is the explicit migration escape hatch and is reported as unsafe in capability output. The seventh module, `std/http.ax`, shares the `net` capability surface with `std/net.ax` and exposes `get(url: string): Option<string>` plus `serve_once(bind: string, body: string): bool` on top of `http_get` and `http_serve_once` intrinsics. The client path implements a blocking HTTP/1.0 client for `http://` and `https://` URLs; the server path binds a socket, serves one plain-text `200 OK` response, and exits. The eighth module, `std/io.ax`, is the first stdlib surface not tied to a capability flag: it exposes `eprintln(text: string): int` on top of a new ungated `io_eprintln` intrinsic that writes a line to stderr and returns the number of bytes written (`-1` on error), matching the ambient status of the `print` statement. The ninth module, `std/json.ax`, is also ungated and exposes a string-based scalar JSON floor: `parse_int`, `parse_bool`, `parse_string`, `stringify_int`, `stringify_bool`, and `stringify_string`. The tenth module, `std/collections.ax`, adds generic borrowed-slice helpers on top of the existing polymorphic collection primitives.
- The pipeline is already split into syntax -> HIR -> MIR -> native build.
- `axiomc build` emits a native binary by generating a Rust file and invoking `rustc`.
- A bootstrap ownership rule is active: non-`Copy` values move on binding and call boundaries, non-`Copy` struct field access and static tuple indexing now move only the named projection while keeping sibling projections available, non-`Copy` map indexing and array indexing still conservatively move the indexed owner projection, branch-local moves conservatively propagate after `if` and `match`, statically false `if` / `while` branches are now ignored instead of poisoning later ownership state, moving an outer non-`Copy` value inside a `while` body is rejected because the value would not be available on subsequent iterations, post-loop ownership state preserves the pre-loop state since the loop body may execute zero times, and live borrowed slices now block moving their owned collection roots until the borrow scope ends, including when those borrows are wrapped in local tuples, named structs, enum payloads, `Option` / `Result` values, passed through sibling expression evaluation, or introduced by temporary `match` expressions.

This is not the final backend architecture. It is the smallest executable
version of the native compiler path that can build a native hello-world and
carry the 1.0 package model.

## Commands

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/hello --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/hello --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/hello --timings
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/hello --debug
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/hello --target "$(rustc -vV | sed -n 's/^host: //p')"
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/hello
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/modules --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/workspace --filter core --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/packages --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/workspace --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/workspace_only --package workspace-app --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/workspace_only --package workspace-app
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/workspace_only --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/capabilities --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- caps stage1/examples/hello --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- fmt stage1/examples/hello --check
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- doc stage1/examples/hello
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- bench stage1/examples/benchmarks --json
```

`axiomc test` discovers `src/**/*_test.ax` entrypoints by default, builds each test
as a native artifact, executes it, and compares stdout against a sibling
`*.stdout` golden file when present. Tests can also use the built-in assertion
helpers `assert_eq`, `assert_ne`, `assert_true`, and `assert_contains`; they
return `0` on success so they fit in the current statement-only bootstrap
surface via ordinary `let` bindings, and they abort the test with a source
location plus expected/actual detail on failure. Projects that need explicit
naming or inline expectations can still declare `[[tests]]` entries in
`axiom.toml`. The command now also accepts `--filter <pattern>` to run a subset
of discovered tests by test name or entry path, and the default CLI summary now
prints `passed` / `failed` / `skipped` counts. Workspace-only roots are now
supported as long as build/run commands select a concrete member package with
`-p/--package`.

## JSON contract

`axiomc check --json`, `build --json`, `test --json`, and `caps --json` all now
emit the versioned schema envelope `schema_version = "axiom.stage1.v1"`.
Successful payloads always include `ok`, `command`, and `project`, while
`axiomc test --json` additionally reports `filter` and per-run/per-case
`duration_ms` plus `passed` / `failed` / `skipped`. Build payloads report the
requested Rust target triple when `--target <triple>` is used and report
`debug: true` when `axiomc build --debug` requests an unoptimized debuginfo build
with generated source-position markers. Debug builds also report `debug_map`,
a JSON sidecar that maps generated Rust statement lines back to Axiom
file/line/column positions. `axiomc build --timings` prints total build time,
cache hit/miss counts, and per-package compile timing/cache status for the
incremental generated-Rust cache.

## Current gaps

The current bootstrap is enough to prove the split and native artifact path, but it is
still far from the stated 1.0 target for service and agent workloads.

### Language surface gaps

- Modules are now limited to package-local path imports plus direct `pub type`, `pub struct`, `pub enum`, and `pub fn` exports only.
- Structs, tuples, tuple-style enum payloads, named-payload enum variants, `Option<T>`, `Result<T, E>`, maps, arrays, borrowed slice types, borrowed array slice expressions, borrowed slices stored inside named structs and enum payloads, borrowed-return aggregates backed by one or more borrowed parameters, field access, tuple indexing, map indexing, array indexing, exhaustive statement-level `match`, monomorphized generic functions, generic structs, generic enums, and the built-in collection helpers `len(...)`, `first(...)`, and `last(...)` now exist, but there is still no general borrow system.
- No inferred generic function, struct, or enum type arguments.
- No methods, trait-style interfaces, or closures. `async fn` and `await` exist for stage1 `Task<T>` values, but the runtime is deterministic and does not yet provide host-thread scheduling.
- Rebinding and shadowing are intentionally rejected today to keep the bootstrap scope small.

### Type and ownership gaps

- Ownership now has a stable current-stage contract for all non-`Copy` stage1 values, including shared and mutable borrowed-slice conflicts, loop-body move rejection, and stable machine-readable ownership error codes in `--json` diagnostics, but it is still intentionally narrower than a full Rust-style borrow checker.
- AG1.1 loop-join handling is now landed: moving an outer non-`Copy` value inside a `while` body is a compile error, and post-loop ownership state preserves the pre-loop state since the body may execute zero times. Dead-branch pruning for statically false conditions is preserved.
- Borrowed slices can now flow through direct `&[T]` returns, named structs, enum payloads, and aggregate return types like `Option<&[T]>` or tuples when they are derived from one or more borrowed parameters, `Option` / `Result` match bindings preserve enough borrow provenance to return those borrowed payloads again, conservative call summaries now keep borrowed-return provenance alive across multiple borrowed parameters, statically false control-flow is now skipped instead of contaminating move state, and live borrowed slices now block later owner moves until their scope ends even when those borrows are stored inside local aggregate wrappers, named structs, enum payloads, or temporary `match` / call expressions, but there are still no general borrows, mutable borrows, lifetime checks, or precise path-sensitive borrow narrowing beyond constant conditions.
- Exhaustiveness checking now exists for statement-level enum `match`, but there is still no typed error propagation and no control-flow-sensitive ownership diagnostics beyond simple branches.
- A dedicated checked-in ownership compile-fail corpus now lives under `stage1/crates/axiomc/tests/ownership_failures`, covering move-after-use, invalid borrowed returns, conflicting borrows, and loop/control-flow hazards. The checked-in ownership-heavy proof point remains `stage1/examples/borrowed_shapes`, and it stays in `make stage1-smoke`.

### Package and build graph gaps

- `axiom.toml` and `axiom.lock` now support deterministic local path dependency graphs, package-root workspace members, workspace-only roots, and `-p/--package` selection for member-targeted build/run/test flows.
- The current import model is still intentionally small: package-local relative path imports plus dependency-prefixed imports like `core/math.ax`, direct `pub struct` / `pub enum` / `pub fn` exports only, and explicit parser diagnostics for unsupported aliases, re-exports, and namespace-qualified calls.
- There is no package registry flow, no version resolution, and no offline lockfile validation beyond the bootstrap lockfile shape.

### Runtime and standard library gaps

- The stdlib surface now covers every stage1 capability-gated intrinsic with a thin wrapper module (`std/time.ax`, `std/env.ax`, `std/fs.ax`, `std/net.ax`, `std/process.ax`, `std/crypto_hash.ax`), plus `std/http.ax` (capability-gated HTTP client/server helpers `http_get` and `http_serve_once` sharing the existing `net` surface), `std/io.ax` (first ungated stdlib module, `eprintln` on top of the new `io_eprintln` intrinsic), `std/json.ax` (ungated scalar/string JSON helpers), `std/collections.ax` (generic borrowed-slice helpers built on AG2 generic functions), `std/sync.ax` (ownership-shaped mutex guards, one-shot cells, and single-slot nonblocking channels), and `std/async.ax` (deterministic task, join, channel, cancellation, timeout, and select wrappers). The `fs` capability is scoped: `fs_read` resolves relative paths from the package root, bounds them to the package root by default or `[capabilities] fs_root = "<relative package path>"`, canonicalizes targets to reject traversal and symlink escapes, and refuses files larger than 64 MiB.
- Capability-aware integration is now in place for the current stage1 runtime surface: compiler-known intrinsics enforce all six manifest flags, stdlib wrappers preserve that enforcement against the importing package's manifest, capability-denied programs fail before native execution, and the Rust suite covers cross-package capability interactions (`dependency_package_must_enable_its_own_capabilities`) plus per-wrapper denial paths.
- No host-thread scheduler, blocking channel wakeups, real timers, or service-grade I/O surface exists.

### Backend and tooling gaps

- Native builds still work by generating Rust and invoking `rustc`; there is no Cranelift backend yet.
- Generated-Rust builds now use a persistent per-artifact cache keyed by
  compiler version, target, debug mode, manifest/lockfile hash, rendered Rust,
  module source hashes, and dependency imports. Cache hits skip `rustc`, cache
  misses repair stale generated Rust or binary artifacts, and `--timings`
  exposes the hit/miss counts plus per-package compile time.
- `axiomc build --debug` now asks `rustc` for debuginfo, disables optimization,
  emits generated Rust source markers, and writes a JSON source-map sidecar for
  Axiom file/line/column positions; full Axiom-native debugger stepping remains
  a direct-backend follow-on.
- `axiomc fmt`, `axiomc bench`, `axiomc doc`, and the stage1 scratch `repl`
  now exist as bootstrap-grade toolchain commands. Publisher, full LSP, and
  debugger surfaces remain open.
- Diagnostics are still intentionally minimal: useful JSON now includes stable ownership codes, but span quality and note richness are still limited.
- Extended validation now carries a small performance regression gate: stage1 `axiomc build` is benchmarked across representative compute (`hello`), I/O/capability (`capabilities`), and concurrency (`stdlib_async`) workloads against checked-in Go and Rust reference builds, with separate cold-build and warm-cache budget multipliers to catch obvious compiler-path regressions without making PR fast CI noisy.

## Execution plan

The detailed execution spec for turning stage1 into the first workable compiler now
lives in [docs/stage1-agent-grade-compiler.md](stage1-agent-grade-compiler.md).

Current proof points:

- `stage1/examples/hello` remains the single-file callable baseline.
- `stage1/examples/modules` proves the multi-file package baseline and the new
  `axiomc test` discovery flow.
- `stage1/examples/packages` proves the local path dependency baseline and root-package lockfile validation.
- `stage1/examples/workspace` proves the package-root workspace-member baseline and workspace-aware root lockfile validation.
- `stage1/examples/workspace_only` proves workspace-only manifests plus `-p/--package` selection for member-targeted build/run while preserving workspace-wide test discovery.
- `stage1/examples/capabilities` proves the capability-gated fs/net/env/clock/crypto path, while the Rust suite covers the remaining process intrinsic contract.
- `stage1/examples/stdlib_time` proves the AG4.1 synthetic stdlib surface: `import "std/time.ax"` brings `Duration`, `Instant`, `duration_ms()`, `now_ms()`, `now()`, `elapsed_ms()`, and `sleep()` into scope and remains subject to the importing package's `[capabilities] clock` flag. Sleep returns `0` after a successful non-negative millisecond duration and `-1` for negative durations.
- `stage1/examples/stdlib_env` extends AG4.1 with `import "std/env.ax"`, bringing `get_env(key)` into scope and staying subject to the importing package's `[capabilities] env = ["NAME"]` allowlist.
- `stage1/examples/stdlib_fs` extends AG4.1 with `import "std/fs.ax"`, bringing `read_file(path)` into scope and staying subject to the importing package's `[capabilities] fs` flag.
- `stage1/examples/stdlib_net` extends AG4.1 with `import "std/net.ax"`, bringing `resolve(host)` into scope and staying subject to the importing package's `[capabilities] net` flag.
- `stage1/examples/stdlib_process` extends AG4.1 with `import "std/process.ax"`, bringing `run_status(command)` into scope and staying subject to the importing package's `[capabilities] process` flag.
- `stage1/examples/stdlib_crypto_hash` extends AG4.1 with `import "std/crypto_hash.ax"`, bringing `sha256(input)` into scope and staying subject to the importing package's `[capabilities] crypto` flag.
- `stage1/examples/stdlib_io` extends AG4.1 with `import "std/io.ax"`, bringing `eprintln(text)` into scope without any capability opt-in — `std/io.ax` is the first stdlib module not tied to a capability flag, matching the ambient status of the `print` statement.
- `stage1/examples/stdlib_json` extends AG4.1 with `import "std/json.ax"`, bringing ungated scalar/string JSON parsing and serialization helpers into scope without waiting for AG2 generics or a first-class JSON value type.
- `stage1/examples/stdlib_collections` extends AG4.1 with `import "std/collections.ax"`, bringing generic borrowed-slice helpers (`count`, `is_empty`, `has_items`, `skip`, `take`, and `window`) into scope without any capability opt-in.
- `stage1/examples/stdlib_http` extends AG4.1/AG4.3 with `import "std/http.ax"`, bringing `get(url)` and `serve_once(bind, body)` into scope on top of blocking HTTP client/server helpers. The checked-in example keeps its smoke deterministic by exercising the closed-port client path; the Rust integration suite covers the single-request server path.
- `stage1/examples/arrays`, `stage1/examples/maps`, `stage1/examples/tuples`,
  and `stage1/examples/structs` cover the current structured-data floor.
- `stage1/examples/slices`, `stage1/examples/borrowed_shapes`, `stage1/examples/enums`,
  and `stage1/examples/outcomes` cover the current borrow-aware and enum/result floor.
- `stage1/examples/generic_aggregates` covers monomorphized generic wrappers and borrowed generic utility helpers over arrays, maps, slices, `Option<T>`, `Result<T, E>`, and user-defined enum payloads.
- `stage1/examples/benchmarks` provides the first checked-in benchmark suite
  fixture for `axiomc bench`; the Go/Rust comparison gate remains a later CI
  policy layer on top of the harness.
- `make stage1-test`, `make stage1-conformance`, and `make stage1-smoke` now cover the checked-in stage1 language gate.

Agent-grade compiler milestone summary:

- `AG0`: freeze the current borrowed-projection baseline as the stage1 entry floor.
- `AG1`: finish ownership and borrowing.
- `AG2`: add the minimum generic abstraction layer.
- `AG3`: add package graph support, stable module rules, and real capability enforcement.
- `AG4`: add the stdlib, async runtime, and HTTP-service-capable runtime surface.
- `AG5`: expand `axiomc test` plus the CLI/worker/service fixtures that close
  the first agent-grade compiler bar.

Important bar definition:

- The first workable-compiler bar is **agent-grade**, not direct-native parity.
- Generated-Rust codegen remains acceptable at that bar as long as the public
  workflow is fully `axiomc`-driven.
- The required proof workloads are a multi-package CLI, a queue-style worker,
  and a small HTTP service.

## Working rules for future stage1 work

- Keep the Rust-only verification gate green: `make stage1-test`,
  `make stage1-conformance`, and `make stage1-smoke`.
- Land stage1 slices in small, reviewable increments; do not combine data-model work, ownership work, and backend replacement in one change.
- Prefer compile-fail tests for language rule changes before broad end-to-end examples.
