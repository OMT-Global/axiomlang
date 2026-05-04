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
- Stage1 now ships a synthetic standard library surface under the `std/` import prefix with sixteen landed modules. Capability-gated surfaces cover the six capability classes: `std/time.ax` exposes `Duration`, `Instant`, `duration_ms(ms): Duration`, `now_ms(): int`, `now(): Instant`, `elapsed_ms(start): int`, and `sleep(duration): int` on top of `clock_now_ms`, `clock_elapsed_ms`, and `clock_sleep_ms`; `std/env.ax` exposes `get_env(key: string): Option<string>` on top of `env_get`, `std/fs.ax` exposes `read_file(path: string): Option<string>` on top of `fs_read`, `std/net.ax` exposes `resolve(host: string): Option<string>` on top of `net_resolve` plus a bounded loopback-only socket floor (`tcp_listen_loopback_once`, `tcp_dial`, `udp_bind_loopback_once`, and `udp_send_recv`), `std/process.ax` exposes `run_status(command: string): int` on top of `process_status`, and `std/crypto_hash.ax` (the stage1 spelling of `std.crypto.hash`) exposes `sha256(input: string): string` on top of `crypto_sha256`. The crypto surface also includes `std/crypto_mac.ax` for `hmac_sha256(key, message): string` plus `constant_time_eq(left, right): bool`; both require `[capabilities].crypto = true`. Each gated module requires the importing package to declare the matching capability (`clock`, `env`, `fs`, `net`, `process`, or `crypto`); environment access is scoped with `env = ["PORT", "LOG_LEVEL"]`, and `env_get` returns `None` for names outside that manifest allowlist. The legacy `env = true` form remains temporarily available but emits a check warning because it grants unrestricted process environment access; `env_unrestricted = true` is the explicit migration escape hatch and is reported as unsafe in capability output. The seventh module, `std/http.ax`, shares the `net` capability surface with `std/net.ax` and exposes `get(url): Option<string>` plus intermediate loopback-only server primitives: `serve_once(bind, body): bool` and a route-shaped `serve(bind, route(path, body), max_requests): bool` helper on top of the `http_get`, `http_serve_once`, and `http_serve_route` intrinsics. The client path implements blocking HTTP/1.0 for `http://` and `https://` URLs; the server path can bind only loopback sockets, route simple GET/HEAD paths to plain-text responses, handle a bounded request lifecycle, and fan accepted requests out to native worker threads. This is still an intermediate AG4.3/#97 slice rather than the final async-runtime listen/accept/respond API. Ungated modules now cover `std/io.ax`, `std/json.ax`, `std/collections.ax`, `std/string_builder.ax`, `std/log.ax`, `std/sync.ax`, `std/async.ax`, and `std/regex.ax`. `std/regex.ax` exposes `is_match(pattern, text): bool`, `find(pattern, text): Option<string>`, and `replace_all(pattern, text, replacement): string` over a deterministic NFA-state engine supporting anchors, `.`, `?`, `*`, `+`, escaped literals, and character classes/ranges without backtracking.

- The pipeline is already split into syntax -> HIR -> MIR -> native build.
- `axiomc build` emits a native binary by default, or a `.wasm` artifact for `--target wasm32` / `--target wasm32-wasi`, by generating Rust and invoking `rustc`.
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
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/hello --target wasm32
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/hello
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/modules --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/workspace --filter core --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/packages --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/workspace --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/workspace_only --package workspace-app --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/workspace_only --package workspace-app
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/workspace_only --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/capabilities --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/proof_cli --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/proof_worker --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- caps stage1/examples/hello --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- fmt stage1/examples/hello --check
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- doc stage1/examples/hello
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- bench stage1/examples/benchmarks --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- lsp
```

`axiomc test` discovers `src/**/*_test.ax` entrypoints by default, builds each test
as a native artifact, executes it, and compares stdout against a sibling
`*.stdout` golden file when present. Tests can also use the built-in assertion
helpers `assert_eq`, `assert_ne`, `assert_true`, and `assert_contains`; they
return `0` on success so they fit in the current statement-only bootstrap
surface via ordinary `let` bindings, and they abort the test with a source
location plus expected/actual detail on failure. For richer stdlib-oriented
coverage, `import "std/testing.ax"` exposes table-case helpers
(`table_int` / `table_bool` / `table_string`), a named `property(name, holds)`
helper for QuickCheck-style sampled checks expressed as deterministic loops or
fixtures, and `snapshot(name, actual, expected)` for inline golden assertions.
Projects that need explicit naming or inline expectations can still declare
`[[tests]]` entries in `axiom.toml`, including `kind = "unit"`, `"table"`,
`"property"`, `"snapshot"`, or `"benchmark"` for JSON reporting. Discovery
classifies `*_table_test.ax`, `*_property.ax`, `*_property_test.ax`,
`*_snapshot_test.ax`, and `*_golden_test.ax` as richer fixture kinds while
preserving the ordinary `*_test.ax` lane. Benchmark entrypoints remain owned by
`axiomc bench`, but `axiomc test --include-benchmarks` can smoke-run discovered
`*_bench.ax` fixtures once so benchmark code participates in functional gates.
The command also accepts `--filter <pattern>` to run a subset of discovered
tests by test name or entry path, and the default CLI summary prints `passed` /
`failed` / `skipped` counts. Workspace-only roots are supported as long as
build/run commands select a concrete member package with `-p/--package`.

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
Parser diagnostics now preserve additional recovered top-level parse errors in
the error payload's `related` array when possible, so editor tooling can show
more than the first syntax error without waiting for full checker recovery.

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
- `axiomc publish` now validates the lockfile and stages a deterministic signed archive into a local static-registry tree for `axiomc registry-index`; there is still no hosted registry service, version resolution, trust-root management, or offline package verification beyond this bootstrap shape.

### Runtime and standard library gaps

- The stdlib surface now covers every stage1 capability-gated intrinsic with a thin wrapper module (`std/time.ax`, `std/env.ax`, `std/fs.ax`, `std/net.ax`, `std/process.ax`, `std/crypto_hash.ax`, `std/crypto_mac.ax`), plus `std/http.ax` (capability-gated HTTP client helper `http_get`, intermediate loopback-only blocking single-request server helper `http_serve_once`, and bounded route-based helper `http_serve_route` sharing the existing `net` surface), `std/io.ax` (first ungated stdlib module, `eprintln` on top of the new `io_eprintln` intrinsic), `std/json.ax` (ungated scalar/string JSON helpers plus manual object/field builders), `std/collections.ax` (generic borrowed-slice helpers built on AG2 generic functions), `std/string_builder.ax` (owned string accumulator), `std/log.ax` (deterministic JSON-line logging over stderr), `std/sync.ax` (ownership-shaped mutex guards, one-shot cells, and single-slot nonblocking channels), `std/async.ax` (deterministic task, join, channel, cancellation, timeout, and select wrappers), and `std/regex.ax` (linear-time matching helpers for common regex constructs). The `net` socket floor is intentionally loopback-only in stage1: the one-shot TCP and UDP listen helpers bind `127.0.0.1:0`, dial/send helpers reject non-loopback targets, HTTP service helpers reject non-loopback bind addresses, payloads are bounded to 64 KiB, and timeouts are clamped to 1-30000 ms. The `fs` capability is scoped: `fs_read` resolves relative paths from the package root, bounds them to the package root by default or `[capabilities] fs_root = "<relative package path>"`, canonicalizes targets to reject traversal and symlink escapes, and refuses files larger than 64 MiB.

- Capability-aware integration is now in place for the current stage1 runtime surface: compiler-known intrinsics enforce all six manifest flags, stdlib wrappers preserve that enforcement against the importing package's manifest, capability-denied programs fail before native execution, and the Rust suite covers cross-package capability interactions (`dependency_package_must_enable_its_own_capabilities`) plus per-wrapper denial paths.
- No host-thread scheduler, blocking channel wakeups, real timers, or service-grade I/O surface exists.

### Backend and tooling gaps

- Native builds still work by generating Rust and invoking `rustc`; there is no Cranelift backend yet.
- The backend-selection surface is only preparatory backend plumbing for later native-backend expansion; today `generated-rust` is the only implemented backend, so this branch is part of #105 rather than closure for it.
- Generated-Rust builds now use a persistent per-artifact cache keyed by
  compiler version, target, debug mode, manifest/lockfile hash, rendered Rust,
  module source hashes, and dependency imports. Cache hits skip `rustc`, cache
  misses repair stale generated Rust or binary artifacts, and `--timings`
  exposes the hit/miss counts plus per-package compile time.
- `axiomc build --debug` now asks `rustc` for debuginfo, disables optimization,
  emits generated Rust source markers, and writes a JSON source-map sidecar for
  Axiom file/line/column positions; full Axiom-native debugger stepping remains
  a direct-backend follow-on.
- `axiomc fmt`, `axiomc bench`, `axiomc doc`, the stage1 scratch `repl`, and a
  bounded `axiomc lsp` analyzer now exist as bootstrap-grade toolchain
  commands. The LSP endpoint currently serves compiler-backed diagnostics over
  JSON-RPC stdio; hover, goto-definition, completion, rename, code actions, and
  full package-graph analysis remain open. See [Stage1 LSP analyzer](stage1-lsp.md).
  Publisher, full LSP, and debugger surfaces remain open.
- Diagnostics are still intentionally minimal: useful JSON now includes stable ownership codes and top-level parser recovery, but checker recovery, span quality, and note richness are still limited.
- Extended validation now carries a small performance regression gate: stage1 `axiomc build` is benchmarked across representative compute (`hello`), I/O/capability (`capabilities`), and concurrency (`stdlib_async`) workloads against checked-in Go and Rust reference builds, with separate cold-build and warm-cache budget multipliers to catch obvious compiler-path regressions without making PR fast CI noisy.

## Execution plan

The detailed execution spec for turning stage1 into the first workable compiler now
lives in [docs/stage1-agent-grade-compiler.md](stage1-agent-grade-compiler.md).
The broad Phase A language issue disposition for #216 through #225 is tracked in
[Stage1 Language Issue Disposition](stage1-language-issue-disposition.md).

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
- `stage1/examples/stdlib_net` extends AG4.1 with `import "std/net.ax"`, bringing `resolve(host)`, one-shot TCP loopback listen/dial, and one-shot UDP loopback bind/send-recv into scope while staying subject to the importing package's `[capabilities] net` flag.
- `stage1/examples/stdlib_process` extends AG4.1 with `import "std/process.ax"`, bringing `run_status(command)` into scope and staying subject to the importing package's `[capabilities] process` flag.
- `stage1/examples/stdlib_crypto_hash` extends AG4.1 with `import "std/crypto_hash.ax"`, bringing `sha256(input)` into scope and staying subject to the importing package's `[capabilities] crypto` flag.
- `stage1/examples/stdlib_crypto_mac` extends AG4.1 with `import "std/crypto_mac.ax"`, bringing `hmac_sha256(key, message)` and `constant_time_eq(left, right)` into scope and staying subject to the importing package's `[capabilities] crypto` flag.
- `stage1/examples/stdlib_io` extends AG4.1 with `import "std/io.ax"`, bringing `eprintln(text)` into scope without any capability opt-in — `std/io.ax` is the first stdlib module not tied to a capability flag, matching the ambient status of the `print` statement.
- `stage1/examples/stdlib_json` extends AG4.1 with `import "std/json.ax"`, bringing ungated scalar/string JSON parsing and serialization helpers into scope without waiting for AG2 generics or a first-class JSON value type.
- `stage1/examples/stdlib_regex` extends AG4.1 with `import "std/regex.ax"`, bringing ungated linear-time `is_match`, `find`, and `replace_all` helpers into scope for agent-safe text matching.
- `stage1/examples/stdlib_collections` extends AG4.1 with `import "std/collections.ax"`, bringing generic borrowed-slice helpers (`count`, `is_empty`, `has_items`, `skip`, `take`, and `window`) into scope without any capability opt-in.
- `stage1/examples/stdlib_string_builder` extends AG4.1 with `import "std/string_builder.ax"`, bringing an owned string accumulator into scope without claiming growable generic vectors or hash maps.
- `stage1/examples/stdlib_log` extends AG4.1 with `import "std/log.ax"`, bringing deterministic JSON-line event formatting and stderr logging into scope without host logging sinks or replay buffers.
- `stage1/examples/stdlib_http` extends AG4.1 with `import "std/http.ax"`, bringing `get(url)`, loopback-only `serve_once(bind, body)`, and route-shaped `route(path, body)` / `serve(bind, route, max_requests)` primitives into scope on top of blocking HTTP client/server helpers. It shares the importing package's `[capabilities] net` flag with `std/net.ax`; the checked-in example keeps its smoke deterministic by exercising the closed-port client path, while the Rust integration suite covers the single-request server path, routed path, and bind-policy rejection. This is not the full #97 HTTP service surface.

- `stage1/examples/proof_cli` closes the first AG5.3 proof workload with a multi-package CLI fixture that pulls command and render helpers from separate local packages while staying fully inside the `axiomc` workflow and exercising capability-gated `std/env.ax` and `std/time.ax`.
- `stage1/examples/proof_worker` closes the queue-style AG5.3 proof workload with a deterministic worker fixture built on `std/async.ax`, `std/env.ax`, and `std/time.ax`.
- `stage1/examples/proof_http_service` is a checked-in HTTP-shaped response fixture that routes request metadata from `std/env.ax`, stamps liveness with `std/time.ax`, and renders the response body through `std/json.ax`; it remains a fixture for the small-service AG5.3 workload while AG4.3/#97 continues toward a fuller async-runtime listen/accept/respond server API.

- `stage1/examples/arrays`, `stage1/examples/maps`, `stage1/examples/tuples`,
  and `stage1/examples/structs` cover the current structured-data floor.
- `stage1/examples/slices`, `stage1/examples/borrowed_shapes`, `stage1/examples/enums`,
  and `stage1/examples/outcomes` cover the current borrow-aware and enum/result floor.
- `stage1/examples/generic_aggregates` covers monomorphized generic wrappers and borrowed generic utility helpers over arrays, maps, slices, `Option<T>`, `Result<T, E>`, and user-defined enum payloads.
- `stage1/examples/benchmarks` provides the first checked-in benchmark suite
  fixture for `axiomc bench`; the Go/Rust comparison gate remains a later CI
  policy layer on top of the harness.
- `stage1/examples/proof_cli` and `stage1/examples/proof_worker` provide the
  first two AG5 proof-workload fixtures. The CLI fixture proves a multi-package
  Axiom program, while the worker fixture proves deterministic queue-style async
  processing. The small HTTP service fixture remains blocked on server-side HTTP
  support.
- `make stage1-test`, `make stage1-conformance`, and `make stage1-smoke` now
  cover the checked-in stage1 language gate. `make stage1-test` also carries
  the AG5 proof-workload tests for `stage1/examples/proof_cli` and
  `stage1/examples/proof_worker`, while `make stage1-smoke` carries their
  blocking build/run acceptance path. The small HTTP service proof remains
  blocked on server-side HTTP support.

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
