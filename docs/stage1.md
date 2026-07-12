# Stage1 bootstrap

<!-- capability-ledger:v1 commands=29 stdlib_modules=34 stdlib_functions=299 capabilities=9 backend=cranelift -->

The Rust bootstrap compiler in `stage1/` is the supported Axiom toolchain.
The Python `stage0` interpreter, bytecode compiler, bytecode format, bytecode
VM, and disassembler are not supported execution surfaces; see
[Python Exit Parity Gate](python-exit-parity-gate.md) and
[Python Exit VM Disposition](python-exit-vm-disposition.md).

## Current bootstrap scope

The checked [capability ledger](../stage1/compiler-contracts/snapshots/capability-ledger.json)
is the source of truth for the current language, command, package, backend,
standard-library, runtime-ABI, and schema inventories. It is generated from
compiler-owned tables and validated by
`python3 scripts/ci/check-capability-ledger.py --check-docs --json`.

The current inventory contains 28 CLI commands, 34 synthetic standard-library
modules with 299 exported functions, and 9 manifest capability kinds. Cranelift
is the only supported CLI backend. Those counts describe discovered surfaces,
not production qualification: the ledger currently records zero
`production_qualified` rows and preserves narrower `direct_runtime`,
`static_spike`, `scaffold`, and `unsupported` evidence tiers.

Filesystem read and write wrappers, Ed25519 helpers, and closure syntax have
landed. Their individual runtime breadth remains classified conservatively in
the ledger and direct-native runtime ABI contract rather than inferred from the
presence of a parser node or wrapper. Closed bootstrap issues are historical
evidence only; they do not establish present production closure.

## Commands

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- new /tmp/axiom-cli --template cli
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- new /tmp/axiom-worker --template worker
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- new /tmp/axiom-service --template service
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/hello --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/hello --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/hello --timings
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/hello --debug
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/hello --locked --offline
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/hello
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/hello --json
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
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- doctor stage1/examples/hello --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- inspect symbols stage1/examples/modules --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- inspect graph stage1/examples/modules --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- explain use_after_move --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- fmt stage1/examples/hello --check
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- doc stage1/examples/hello
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- doc --md stage1/examples/hello
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- doc stage1/examples/hello --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- bench stage1/examples/benchmarks --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- mutation-report .axiom-build/reports/mutation-rust-smoke.json --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- lsp
```

`axiomc doc --json` emits the same API extraction pass as a versioned machine
contract, including generated Markdown/HTML paths, `functions` and `types`
views, documented symbols, comments, signatures, simple example notes,
declaration kind/visibility, and package capability descriptors when the input
path is a package root. The output validates against
`stage1/schemas/axiom-doc-v0.schema.json` as well as the shared
`axiom.stage1.v1` envelope. `axiomc doc --md <project>` writes Markdown into
`<project>/dist/docs/index.md` unless `--out-dir` is provided.
`std/doc.ax` now defines the AxiOM-side doc item contract and Markdown renderer;
source extraction and `axiomc doc` driver integration remain bootstrap-hosted
until Phase-K.1 can follow Phase-J.3.
`std/lsp.ax` defines the AxiOM-side JSON-RPC/LSP message contract for
initialize, document-change, diagnostic, completion, shutdown, and exit flows;
the Stage1 Rust stdio loop still owns process I/O until the Phase-L self-hosting
dependencies are satisfied.

`axiomc new` defaults to the `cli` starter and also accepts `--template worker`
and `--template service`. Each starter writes `axiom.toml`, `axiom.lock`,
`src/main.ax`, `src/main_test.ax`, and `src/main_test.stdout`; the generated
project is expected to pass `axiomc check`, `axiomc build`, and `axiomc test`
without manual edits.

`axiomc test` discovers `src/**/*_test.ax` entrypoints by default, builds each test
as a native artifact, executes it, and compares stdout against a sibling
`*.stdout` golden file when present. A sibling `*.stderr` file can also pin the
expected stderr stream, and manifest-declared `[[tests]]` entries support inline
`stdin`, `stdout`, and `stderr` expectations. JSON output includes actual and expected
streams on failing cases so agents can distinguish stdout golden drift from
runtime diagnostics. Tests can also use the built-in assertion helpers
`assert_eq`, `assert_ne`, `assert_true`, and `assert_contains`; they return `0`
on success so they fit in the current statement-only bootstrap surface via
ordinary `let` bindings, and they abort the test with a source location plus
expected/actual detail on failure. For richer stdlib-oriented coverage,
`import "std/testing.ax"` exposes table-case helpers (`table_int` /
`table_bool` / `table_string`), a named `property(name, holds)` helper for
QuickCheck-style sampled checks expressed as deterministic loops or fixtures,
and `snapshot(name, actual, expected)` for inline golden assertions. Projects
that need explicit naming or inline expectations can still declare `[[tests]]`
entries in `axiom.toml`, including `kind = "unit"`, `"table"`, `"property"`,
`"snapshot"`, or `"benchmark"` for JSON reporting. Discovery classifies
`*_table_test.ax`, `*_property.ax`, `*_property_test.ax`, `*_snapshot_test.ax`,
and `*_golden_test.ax` as richer fixture kinds while preserving the ordinary
`*_test.ax` lane. Benchmark entrypoints remain owned by `axiomc bench`, but
`axiomc test --include-benchmarks` can smoke-run discovered `*_bench.ax`
fixtures once so benchmark code participates in functional gates. The command
also accepts `--filter <pattern>` to run a subset of discovered tests by test
name or entry path. `axiomc test --properties` narrows discovery to property
fixtures and prints an explicit `N/N properties passed` summary for Phase-H
property gates. `axiomc check --properties` first performs the normal type,
ownership, capability, and manifest checks, then runs the same property-only
fixture gate so property failures block a check command before build artifacts
are accepted. `axiomc check --properties --backend generated-rust` is rejected
by the CLI parser and is no longer a compatibility escape hatch. The
`std/testing.ax` helper surface is backed by
`stage1/stdlib/std/testing.ax` and embedded into the virtual stdlib at compiler
build time. The checked-in stdlib testing package now carries a 12-property
suite across the deterministic stdlib surfaces and is exercised by
`scripts/ci/run-stdlib-property-checks.sh`; that script also runs the checked-in
`stage1/examples/stdlib_*` AxiOM tests so CI covers stdlib behavior through
`axiomc test`. `make stage1-test` also runs `make stage1-stdlib-test` and
`make stage1-compiler-property-test`, so local stage1 verification records the
same property summaries before proof workloads. Phase-J compiler-internal
property migration has a seed package at `stage1/examples/compiler_properties`;
`make stage1-compiler-property-test` runs its type-system, ownership,
capability-policy, and property-clause fixtures through both `axiomc
check --properties` and `axiomc test --properties` on the direct-native
Cranelift backend while the full 100-property suite remains tracked by #717.
The default CLI summary prints
`passed` / `failed` / `skipped` counts. `axiomc test --list` exposes the same
discovery pass without building or running the tests; text output emits package,
test name, and entry path columns, while `--json` adds stable package
membership plus golden-output and compile-fail markers for automation.
Workspace-only roots are supported as long as build/run commands select a
concrete member package with `-p/--package`.

## JSON contract

`axiomc check --json`, `build --json`, `run --json`, `test --json`,
`caps --json`, and `mutation-report --json` all now emit the versioned schema
envelope `schema_version = "axiom.stage1.v1"`.
The checked-in compiler JSON schema is
`stage1/schemas/axiom.stage1.v1.schema.json`; the manifest editor schema is
`stage1/schemas/axiom.toml.schema.json`.
The first agent-facing Intent IR / semantic graph schema is
`stage1/schemas/axiom-intent-ir-v0.schema.json`; see
[intent-ir-v0.md](intent-ir-v0.md).
`axiomc inspect intent <path> --json` emits that canonical, deterministic
package or workspace graph with package-relative provenance and explicit
node-linked completeness diagnostics. Its output is the dedicated
`axiom.intent_ir.v0` document rather than the shared command envelope described
below.
Successful payloads always include `ok` and `command`; project-scoped payloads
also include `project`.
`axiomc run --json` captures the selected backend, native binary exit code,
result enum, duration, stdout, stderr, forwarded args, selected package, and
optional generated-Rust artifact path without changing the default
human-readable streaming behavior. `axiomc test --json` likewise reports the
selected backend and keeps per-case generated-Rust artifact paths optional so
direct-native test runs can report only their native binaries.
`axiomc inspect artifacts` reports direct-native package binaries and target
artifacts as the planned build surface. It does not plan generated Rust for
packages; stale `.generated.rs` files in `dist/` are classified as
`legacy_generated_rust` compatibility outputs.
`axiomc mutation-report --json` reports survivor counts and grouped recommended
fixtures in the same versioned envelope used by the other command contracts.
The self-hosted command and LSP package boundary is frozen in
`docs/compiler-command-lsp-packages.md`, with the validation snapshot at
`stage1/compiler-contracts/snapshots/command-lsp.json`. Use
`make stage1-command-lsp-boundary` to verify that command dispatch, JSON
envelopes, and LSP service flows stay package-oriented while the Rust bootstrap
host remains the temporary developer path.
The HIR ownership and capability package boundary is frozen in
`docs/compiler-hir-ownership-capability.md`, with the validation snapshot at
`stage1/compiler-contracts/snapshots/hir-ownership-capability.json`. Use
`make stage1-hir-boundary` to verify that typed declarations, name resolution,
capability policy, ownership and borrow state, property clauses, and
agent-facing HIR inspection stay package-oriented while the Rust bootstrap host
remains temporary.
The MIR/backend package boundary is frozen in
`docs/compiler-mir-backend-packages.md`, with the validation snapshot at
`stage1/compiler-contracts/snapshots/mir-backend.json`. Use
`make stage1-mir-backend-boundary` to verify that MIR-to-target inputs,
backend target contracts, generated-Rust compatibility, and direct-native
evidence stay separate before the Rust-hosted backend path is removed.
`axiomc test --json` additionally reports `filter`, `properties_only`,
property totals, and per-run/per-case `duration_ms` plus `passed` / `failed` /
`skipped`. Build payloads report the
requested Rust target triple when `--target <triple>` is used and report
`debug: true` when `axiomc build --debug` requests an unoptimized debuginfo build

with generated source-position markers. Build JSON carries both `cache_key`
metadata with the cache schema version, compiler key, target, debug mode,
manifest hash, lockfile hash, generated Rust hash, and per-source hashes used
for incremental cache validation, plus a smaller `metadata` object for
requested/resolved target, debug mode, package lockfile, lockfile hash, and
aggregate source hash inspection. Debug builds report `debug_map`, a JSON
sidecar that maps generated Rust statement lines back to Axiom file/line/column
positions, plus `debug_manifest`, a JSON sidecar that binds the native binary
hash, generated Rust hash, backend-native debug settings, source file hashes,
and mapping counts for debugger/tooling consumers. See
`docs/stage1-debug-map.md` for the LLDB/GDB sidecar translation workflow.
`axiomc build --timings` prints total build time, cache hit/miss counts, and
per-package compile timing/cache status for the incremental generated-Rust
cache.
Build payloads also expose persisted lowering evidence that distinguishes
direct-native runtime execution, hybrid runtime binaries with known-value
static folds, bounded effect-free static output, and fail-closed legacy
fallback selection. See [Build lowering evidence](build-lowering-evidence.md)
for the schema and the exact meaning of `legacy_fallback_attempted`.
Parser diagnostics now preserve additional recovered top-level parse errors in
the error payload's `related` array when possible, so editor tooling can show
more than the first syntax error without waiting for full checker recovery.
Diagnostics may include additive `end_line` and `end_column` fields when the
compiler can report a full source range; existing `line` and `column` start
positions remain the compatibility contract.
`axiomc doctor --json` reports local `rustc` and `cargo` availability, the host
target triple, lockfile status, package/workspace graph summary, manifest
capabilities, and known unsupported feature buckets for agent preflight checks.
`axiomc inspect symbols --json` emits exported package source symbols with
source spans, signatures, module imports, and directly inferred intrinsic
capability use for agent indexing.
`axiomc inspect graph --json` emits package metadata, lockfile resolution,
package-local module imports, stdlib module names, detected local import cycles,
and import errors for agent dependency-graph checks.
Stable diagnostic codes can be queried with `axiomc explain <code>` in text or
JSON form; the current catalog covers the stable ownership codes emitted by the
stage1 checker plus the structured generated-Rust backend failure codes.

## Numeric Overflow Policy

Stage1 follows explicit, reviewable numeric overflow semantics. In debug builds,
ambient signed integer `+` checks overflow and reports a runtime diagnostic such
as `numeric overflow: i32 addition`; in release builds the same signed operation
wraps. Ambient unsigned integer `+` wraps in both debug and release builds.
Floating-point `+` follows the target platform's IEEE behavior.

Use numeric helper methods when overflow behavior is part of the program
contract: `wrapping_add` wraps, `checked_add` returns `None` on overflow, and
`saturating_add` clamps at the type bounds. These helpers are available on the
supported integer widths and should be preferred over ambient arithmetic when
reviewers need to see the intended overflow behavior at the call site.

Stable diagnostic codes:

| Code | Meaning | Sample diagnostic |
| --- | --- | --- |
| `use_after_move` | A non-`Copy` value was moved into another binding or call and then used again. | `use of moved value "greeting"`. |
| `move_while_borrowed` | An owned collection root was moved while a live borrowed slice still referenced it. | `cannot move value "values" while borrowed slices are still live`. |
| `loop_move_outer_non_copy` | A loop body moved a non-`Copy` value declared outside the loop. | `cannot move non-copy value "name" declared outside the loop inside a while body`. |
| `borrow_return_requires_param_origin` | A function returned a borrowed value that was not derived from a borrowed parameter. | `returning borrowed values requires data derived from one of the borrowed parameters in stage1`. |
| `borrow_return_origin_ambiguous` | A function returned a borrowed value while more than one borrowed parameter could be the origin. | `cannot infer which parameter the returned borrow originates from`. |
| `mutable_borrow_while_shared_live` | A mutable borrowed slice was created while a shared borrowed slice of the same owner was live. | `cannot create mutable borrow of value "values" while a shared borrow is still live`. |
| `shared_borrow_while_mutable_live` | A shared borrowed slice was created while a mutable borrowed slice of the same owner was live. | `cannot create shared borrow of value "values" while a mutable borrow is still live`. |
| `mutable_borrow_while_mutable_live` | A mutable borrowed slice was created while another mutable borrowed slice of the same owner was live. | `cannot create mutable borrow of value "values" while another mutable borrow is still live`. |
| `closure_move_captured_non_copy` | A closure body moved a captured non-`Copy` value. | `closure cannot move captured non-copy value`. |
| `closure_borrowed_slice_return` | A closure returned a borrowed slice whose lifetime cannot be tied to a safe parameter origin. | `closure fn values cannot return borrowed slice types in stage1`. |
| `generated_rust_compilation_failed` | rustc rejected generated Rust while building a stage1 artifact. | `generated Rust compilation failed`. |
| `ICE-001` | An invalid compiler-internal shape reached generated-Rust codegen. | `internal compiler error while rendering generated Rust`. |

Common non-ownership diagnostic codes:

| Code | Meaning |
| --- | --- |
| `type.mismatch` | A value does not match the type required by a binding, call, or return site. |
| `type.invalid` | A type-system rule failed outside a simple expected-versus-actual mismatch. |
| `parse.unexpected_token` | The parser found a token that is invalid in the current grammar slot. |
| `parse.invalid_syntax` | Source does not fit a supported stage1 grammar form. |
| `parse.missing_token` | A declaration or expression is incomplete because a required token is missing. |
| `parse.unsupported_syntax` | Source uses a syntactic form that stage1 deliberately does not implement yet. |
| `manifest.invalid_capability` | `axiom.toml` declares a capability with an unsupported shape. |
| `manifest.bad_dependency_path` | A dependency or workspace member path is empty, missing, or escapes the workspace boundary. |
| `import.unresolved` | A package or dependency import path could not be resolved to source. |
| `import.cycle` | A package-local import graph loops back to a module already being loaded. |
| `import.invalid` | An import violates stage1 visibility, namespace, or boundary rules. |
| `capability.denied` | Source attempted to use a host capability that the manifest does not allow. |
| `control.missing_return` | A non-unit function can reach a control-flow path without returning a value. |
| `control.unreachable_statement` | A statement follows a terminating control-flow operation in a currently unsupported shape. |
| `control.invalid` | A branch, loop, or terminating statement violates the current control-flow contract. |
| `codegen.internal` | A checked construct reached a backend path without a valid lowering. |

Checked-in `check --json` contract fixtures live under
`stage1/json-fixtures/check/` and cover success, parse, type, ownership, and
capability-denial payloads.
The self-hosted diagnostics and syntax boundary fixture lives at
`stage1/compiler-contracts/snapshots/diagnostics-syntax.json` and is validated
with `make stage1-diagnostics-syntax-boundary`.
Checked-in `build`, `test`, and `caps` JSON contract fixtures live under
`stage1/json-fixtures/` and cover Cranelift direct-native success payloads,
targeted-build no-fallback failures, build failures, test filters, duration
fields, failing cases, and unsafe capability state.


## Current gaps

The compiler-owned inventory is broader than its qualified behavior. The
capability ledger deliberately separates surface presence from evidence depth:
no row is currently production-qualified.

- Language: closure expressions, static trait dispatch, mutable references, and
  explicit lifetime slices are present, but the ledger classifies syntax rows
  as static spikes. Dynamic trait dispatch remains unsupported, and ownership
  checking remains narrower than a general borrow checker.
- Packages: local packages, path dependencies, workspaces, lockfile validation,
  and local registry publication shapes are implemented as bootstrap/static
  evidence. Remote registry resolution remains unsupported.
- Runtime and stdlib: 34 modules and 299 exported functions are compiler-owned
  surfaces. Direct-runtime rows identify where native execution evidence exists;
  module rows stay partial because evidence covers bounded shapes rather than
  every legal input and composition.
- Backends and targets: Cranelift is the only supported CLI backend and remains
  partial at the full-language level. Generated Rust is internal-only legacy
  source, and `wasm32-wasip1` remains unsupported by the direct-native path.
- Tooling: native Axiom DWARF, the general borrow-check pass, and dynamic trait
  dispatch are scaffolded or unsupported as recorded in the ledger.

Closed bootstrap issues and older milestone prose are retained only as
historical evidence. Current claims must come from the checked ledger, runtime
ABI contract, and production-language readiness gate.

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
- `std/async_net.ax` exposes async task wrappers for the bounded TCP/UDP socket floor and raw TCP listener accept plus owned-string recv/send helpers. `stage1/examples/stdlib_net_tcp_async` proves a connection-per-task loopback echo fixture with two clients, and the module remains subject to the importing package's `[capabilities] net` flag.
- `stage1/examples/stdlib_process` extends AG4.1 with `import "std/process.ax"`, bringing `run_status(command)` into scope and staying subject to the importing package's `[capabilities] process` flag.
- `stage1/examples/stdlib_crypto_hash` extends AG4.1 with `import "std/crypto_hash.ax"`, bringing `sha256(input)` into scope and staying subject to the importing package's `[capabilities] crypto` flag.
- `stage1/examples/stdlib_crypto_mac` extends AG4.1 with `import "std/crypto_mac.ax"`, bringing `hmac_sha256(key, message)`, `hmac_sha512(key, message)`, `verify_sha256(tag, key, message)`, `verify_sha512(tag, key, message)`, `constant_time_eq(left, right)`, and `constant_time_eq_u8(left, right)` into scope and staying subject to the importing package's `[capabilities] crypto` flag. `std/crypto.ax` is the umbrella import for the landed hash, MAC, and constant-time comparison helpers.
- `stage1/examples/stdlib_crypto_random`, `stage1/examples/stdlib_crypto_signature`, and `stage1/examples/stdlib_crypto_aead` extend AG4.1 with checked-in stdlib examples for `random_bytes(...)`/`random_u64()`, Ed25519 signing and verification, and AES-256-GCM seal/open round-trips. They stay subject to the importing package's `[capabilities] crypto` flag and are part of the focused direct-native example smoke.
- `stage1/examples/stdlib_io` extends AG4.1 with `import "std/io.ax"`, bringing `eprintln(text)`, `readline()`, and `read_to_string()` into scope without any capability opt-in — `std/io.ax` is the first stdlib module not tied to a capability flag, matching the ambient status of the `print` statement.
- `stage1/examples/stdlib_json` extends AG4.1 with `import "std/json.ax"`, bringing ungated scalar/string JSON parsing and serialization helpers into scope without waiting for AG2 generics or a first-class JSON value type. It is part of the focused direct-native example smoke.
- `stage1/examples/stdlib_serdes` adds `import "std/serdes.ax"` with a native `Value` union, `to_json`, `stringify`, and `from_json` coverage for object maps, arrays, scalars, surrogate-pair string escapes, scientific-notation numbers, and trailing whitespace.
- `stage1/examples/stdlib_outcome` proves the public outcome/result helper surface and is part of the focused direct-native example smoke.
- `stage1/examples/stdlib_regex` extends AG4.1 with `import "std/regex.ax"`, bringing ungated linear-time `is_match`, `find`, and `replace_all` helpers into scope for agent-safe text matching. It is part of the focused direct-native example smoke.
- `stage1/examples/stdlib_collections` extends AG4.1 with `import "std/collections.ax"`, bringing generic borrowed-slice helpers (`count`, `is_empty`, `has_items`, `skip`, `take`, and `window`) into scope without any capability opt-in. It is part of the focused direct-native example smoke, along with `stage1/examples/stdlib_collection_lookup`.
- `stage1/examples/stdlib_string_builder` extends AG4.1 with `import "std/string_builder.ax"`, bringing an owned string accumulator into scope without claiming growable generic vectors or hash maps. It is part of the focused direct-native example smoke.
- `stage1/examples/stdlib_log` extends AG4.1 with `import "std/log.ax"`, bringing deterministic JSON-line event formatting and stderr logging into scope without host logging sinks or replay buffers.
- `stage1/examples/stdlib_http` extends AG4.1 with `import "std/http.ax"`, bringing `get(url)`, loopback-only `listen`/`accept`/`respond`, `serve_once(bind, body)`, and route-shaped `fixed_route(path, body)` / `serve(bind, route, max_requests)` primitives into scope on top of blocking HTTP client/server helpers. It shares the importing package's `[capabilities] net` flag with `std/net.ax`; the checked-in example keeps its smoke deterministic by exercising the closed-port client path, while the Rust integration suite covers listener handles, the single-request server path, routed path, async-gated route serving, and bind-policy rejection.

- `stage1/examples/proof_cli` closes the first AG5.3 proof workload with a multi-package CLI fixture that pulls command and render helpers from separate local packages while staying fully inside the `axiomc` workflow and exercising capability-gated `std/env.ax` and `std/time.ax`.
- `stage1/examples/proof_worker` closes the queue-style AG5.3 proof workload with a deterministic worker fixture built on `std/async.ax`, `std/env.ax`, and `std/time.ax`.
- `stage1/examples/proof_http_service` is a checked-in HTTP-shaped response fixture that routes request metadata from `std/env.ax`, stamps liveness with `std/time.ax`, and renders the response body through `std/json.ax`; it remains the small-service AG5.3 workload on top of the landed AG4.3/#97 server surface.

- `stage1/examples/stdlib_http` extends AG4.1 with `import "std/http.ax"`, bringing `get(url)` into scope on top of a new blocking HTTP/1.0 client for `http://` and `https://` URLs; it shares the importing package's `[capabilities] net` flag with `std/net.ax` and keeps its smoke deterministic by pointing at a closed local port so the `None` branch always fires.
- `stage1/examples/proof_cli` closes the first AG5.3 proof workload with a multi-package CLI fixture that pulls command and render helpers from separate local packages while staying fully inside the `axiomc` workflow and exercising capability-gated `std/env.ax` and `std/time.ax`.
- `stage1/examples/proof_worker` closes the queue-style AG5.3 proof workload with a deterministic worker fixture built on `std/async.ax`, `std/env.ax`, and `std/time.ax`.
- `stage1/examples/proof_http_service` closes the small-service AG5.3 proof workload with a checked-in HTTP response fixture that routes request metadata from `std/env.ax`, stamps liveness with `std/time.ax`, and renders the response body through `std/json.ax`.

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
  the stdlib `axiomc test --properties` gate and the AG5 proof-workload tests
  for `stage1/examples/proof_cli`, `stage1/examples/proof_worker`, and
  `stage1/examples/proof_http_service`, while `make stage1-smoke` carries their
  blocking build/run acceptance path.
- Local `cargo test --manifest-path stage1/Cargo.toml -p axiomc` keeps native
  runtime tests listed but ignored by default so sandboxed contributor hosts
  without linker tooling still get a clean compiler test signal. Use
  `cargo test --manifest-path stage1/Cargo.toml -p axiomc --features run-native-tests`
  or `make stage1-test` when the host has the native toolchain and should run
  the full build/run coverage.

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

## Reference

- [Typed MIR contract](stage1-mir-contract.md)
