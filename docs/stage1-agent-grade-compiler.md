# Stage1 Agent-Grade Compiler Plan

This doc is the implementation spec for turning `stage1/` into Axiom's first
workable compiler for agent use. `docs/stage1.md` stays as the shorter status
and slice summary; this file is the detailed execution contract for future work.

## Current baseline

AG0 is the current entry floor and must remain intact before any downstream work starts.

- Stage1 already has a real `axiomc` CLI with `new`, `check`, `build`, `run`,
  `test`, and `caps`.
- The backend is still generated Rust plus `rustc`. That is acceptable for the
  agent-grade milestone as long as the public workflow is fully `axiomc`-driven.
- The new backend-selection seam is preparatory plumbing only; it does not yet
  satisfy or close #105 on its own.
- The current language floor includes multi-file modules, structs, enums,
  arrays, maps, tuples, borrowed slices, `Option<T>`, `Result<T, E>`, and the
  ownership/bootstrap work captured by `stage1/examples/borrowed_shapes`.
- The required Rust-only verification gate remains:
  - `make stage1-test`
  - `make stage1-conformance`
  - `make stage1-smoke`

Entry rule:

- Every AG1+ branch must start from a commit that includes the borrowed-projection
  baseline and the `borrowed_shapes` example.

## Definition of agent-grade compiler

The first workable-compiler bar is **agent-grade**, not direct-native parity.

To count as agent-grade:

- Stage1 must provide a complete end-user workflow through `axiomc` for
  building and running stage1 programs.
- The required public commands at this bar are:
  - `axiomc new`
  - `axiomc check`
  - `axiomc build`
  - `axiomc run`
  - `axiomc test`
  - `axiomc caps`
- The generated-Rust backend remains an internal implementation detail and is
  acceptable for this milestone.
- The compiler must support real Axiom packages for three proof workloads:
  - multi-package CLI
  - queue-style worker
  - small HTTP service
- All three workloads must use capability-gated stage1 stdlib/runtime APIs.
- JSON diagnostics are required on `check`, `build`, `test`, and `caps`.

Not required for the agent-grade bar:

- replacing generated Rust with a direct backend
- `fmt`, `bench`, `doc`, `publish`, registry publishing, or LSP
- trait bounds, macros, higher-kinded abstractions, or user `unsafe`

## Milestones

### AG0: Baseline freeze and entry criteria

Status: landed.

Deliverables:

- Borrowed slices remain valid inside named structs and enum payloads.
- `stage1/examples/borrowed_shapes` stays in the checked-in example set.
- `make stage1-test`, `make stage1-conformance`, and `make stage1-smoke` cover the current language gate.
- `docs/stage1.md` remains the short status page and links to this doc.

Acceptance:

- No AG1+ work may remove or weaken the borrowed-projection regressions.
- Stage1 baseline behavior is proven by the existing Rust suite plus both repo-wide gates.

### AG1: Finish ownership and borrowing

Goal: replace the remaining bootstrap ownership special cases with a stable lexical borrow model.

Status: in progress. AG1.1 is landed.

Work packages:

- `AG1.1`: unknown-branch and loop join handling — **landed**
  - Moving an outer non-`Copy` value inside a `while` body is now a compile
    error ("cannot move non-copy value … inside loop body — value would not be
    available on subsequent iterations").
  - Post-loop ownership state preserves pre-loop moved flags since the body may
    execute zero times.
  - Dead-branch pruning for statically false `if` / `while` conditions is
    preserved unchanged.
  - `if` / `else` branch merge retains OR semantics (moved in either branch →
    moved after the `if`), which is sound for the current bootstrap scope.
  - Covered by four new Rust tests:
    `check_project_rejects_moving_outer_string_inside_while_body`,
    `check_project_allows_copy_move_inside_while_body`,
    `check_project_allows_use_after_while_when_body_does_not_move`,
    `check_project_allows_local_string_move_inside_while_body`.
- `AG1.2`: mutable borrows
  - Start with borrowed locals and borrowed slices.
  - Reject double mutable borrow and mutable-plus-shared aliasing.
- `AG1.3`: projection-sensitive ownership — **landed**
  - Non-`Copy` struct field access and static tuple indexing now move only the named projection and leave sibling projections available.
  - Whole-value use after a partial move remains rejected, and call lowering respects projected non-`Copy` arguments.
  - Match payload bindings continue to lower as independent owned bindings so moving one non-`Copy` payload binding does not invalidate sibling payload bindings.
- `AG1.4`: diagnostics and failure corpus
  - Add stable ownership error kinds in JSON diagnostics.
  - Lock a compile-fail suite for move-after-use, invalid returned borrows,
    conflicting borrows, and loop/control-flow hazards.

Acceptance:

- Ownership is no longer described as bootstrap-only in docs.
- The Rust regression suite includes a dedicated ownership compile-fail corpus.
- Stage1 has at least one checked-in ownership-heavy example that passes through `axiomc build` and `axiomc run`.

### AG2: Minimum generic abstraction layer

Goal: add the smallest generic system needed for agent/service code.

Work packages:

- `AG2.1`: monomorphized generic functions
  - Support generic utility functions over existing stage1 types.
  - Require explicit type arguments when inference is ambiguous.
- `AG2.2`: generic structs and enums
  - Support generic wrappers over arrays, maps, slices, `Option<T>`, and `Result<T, E>`.
  - Keep codegen monomorphized.
- `AG2.3`: borrow-generic interaction rules (**landed**)
  - Make borrowed data legal inside generic signatures and generic wrappers.
  - Add compile-fail coverage for mismatched instantiations, unconstrained type
    parameters, and borrowed generic return misuse.

Deliberate exclusions:

- no trait bounds
- no methods
- no higher-kinded abstractions
- no macros
- no requirement for user-defined closures at this milestone

Acceptance:

- Stage1 examples can express generic wrappers and utility helpers directly in
  the current compiler.
- Generic borrow behavior is covered by both positive and compile-fail tests.

### AG3: Package graph, module rules, and capability enforcement

Goal: make stage1 usable across real multi-package codebases.

Status: complete for the current stage1 bootstrap contract.

- `AG3.1` local path dependency graphs, package-root workspace members, and root lockfile validation are landed.
- `AG3.2` now rejects import aliases, re-exports, and namespace-qualified calls with explicit parser diagnostics.
- `AG3.3` now denies capability-gated compiler-known intrinsics across all six manifest flags: `fs_read(...)`, `net_resolve(...)`, `process_status(...)`, `env_get(...)`, `clock_now_ms()`, `clock_elapsed_ms(...)`, `clock_sleep_ms(...)`, and `crypto_sha256(...)`.
- Workspace-only manifests are now accepted at the root, and `axiomc check/build/run/test -p <package>` can target a concrete workspace member when the root has no `[package]` section.

Work packages:

- `AG3.1`: dependencies and workspaces
  - Accept local path dependency entries in `axiom.toml` and support package-root workspace membership with relative local members.
  - Validate `axiom.lock` against the resolved graph.
- `AG3.2`: stable module/import rules
  - Lock the import model for package-local modules plus dependency imports.
  - Reject unsupported aliasing, re-exports, and namespace-qualified calls explicitly rather than implicitly.
- `AG3.3`: capability enforcement
  - Move capability handling from metadata-only to compile/build/run enforcement.
  - Keep new stage1 runtime entrypoints capability-aware by default instead of allowing metadata-only drift.
  - Capability-denied programs must fail before native execution.

Acceptance:

- `axiomc check/build/run` works on a workspace with at least one dependency edge.
- `axiom.lock` participates in deterministic builds and is validated in CI.
- Capability-denied code fails consistently with machine-readable diagnostics.

### AG4: Service-grade runtime surface

Goal: provide the minimum runtime and stdlib needed for agents, workers, and small services.

Status: in progress. AG4.1 has been kicked off with the synthetic stdlib
plumbing, and every stage1 capability-gated intrinsic now has a matching
thin-wrapper stdlib module: `std/time.ax`, `std/env.ax`, `std/fs.ax`,
`std/net.ax`, `std/process.ax`, `std/crypto_hash.ax`, and
`std/crypto_mac.ax`. AG4.1 now also
includes `std/http.ax`, a blocking HTTP/1.0 client for `http://` and
`https://` URLs on top of a new `http_get` intrinsic that shares the `net`
capability with `std/net.ax` and demonstrates that the `std.*` surface is not
limited to one wrapper per capability. It also includes
`std/io.ax`, the first stdlib module not tied to a capability flag, which
wraps a new ungated `io_eprintln` intrinsic and establishes the "ambient
stdio" precedent alongside the existing `print` statement. `std/json.ax`
adds ungated scalar/string JSON helpers, `std/collections.ax` adds generic
borrowed-slice helpers on top of AG2 generic functions, `std/sync.ax` provides
ownership-shaped synchronization values, and `std/async.ax` exposes the
deterministic AG4.2 task/channel runtime. AG4.4 capability-aware integration
for the currently landed stdlib/runtime surface is now complete. AG4.3 remains
open: `std/http.ax::serve_once` and `std/http.ax::serve` are intermediate
loopback-only blocking service primitives that provide smoke coverage for
bind/accept/route/respond and bounded lifecycle behavior, but they are not the
full async-runtime listen/accept/respond service surface required by #97.


Work packages:

- `AG4.1`: stdlib surface
  - Synthetic stdlib infrastructure: `import "std/<module>.ax"` is resolved by
    the compiler against an in-crate source table under a `<stdlib>` sentinel
    package root. Wrappers call existing intrinsics; capability enforcement
    still runs against the **importing** package's manifest via
    `hir::lower_with_capabilities`, so stdlib imports stay transparent to the
    capability model.
  - `std.time` — **landed** as `std/time.ax` exposing `Duration`, `Instant`,
    `duration_ms(ms): Duration`, `now_ms(): int`, `now(): Instant`,
    `elapsed_ms(start): int`, and `sleep(duration): int` on top of the existing
    `clock_now_ms` intrinsic and the new `clock_elapsed_ms` / `clock_sleep_ms`
    intrinsics. Covered by
    `stage1/examples/stdlib_time` and three Rust tests
    (`stage1_project_imports_synthetic_stdlib_time_module`,
    `stage1_project_rejects_stdlib_time_without_clock_capability`,
    `stage1_project_rejects_unknown_stdlib_module`).
  - `std.env` — **landed** as `std/env.ax` exposing
    `get_env(key: string): Option<string>` on top of the existing `env_get`
    intrinsic. Covered by `stage1/examples/stdlib_env` and two Rust tests
    (`stage1_project_imports_synthetic_stdlib_env_module`,
    `stage1_project_rejects_stdlib_env_without_env_capability`).
  - `std.fs` — **landed** as `std/fs.ax` exposing
    `read_file(path: string): Option<string>` on top of the existing `fs_read`
    intrinsic. The generated helper treats relative paths as package-relative,
    restricts access to the package root by default or `[capabilities]
    fs_root = "<relative package path>"`, canonicalizes requested files to deny
    traversal and symlink escapes, and rejects reads larger than 64 MiB. Covered
    by `stage1/examples/stdlib_fs` and Rust tests
    (`stage1_project_imports_synthetic_stdlib_fs_module`,
    `stage1_project_rejects_stdlib_fs_without_fs_capability`,
    `build_project_scopes_fs_read_to_manifest_root`).
  - `std.net` — **landed** (extension beyond the original AG4.1 list to close
    the capability/wrapper symmetry) as `std/net.ax` exposing
    `resolve(host: string): Option<string>` on top of the existing
    `net_resolve` intrinsic. Covered by `stage1/examples/stdlib_net` and two
    Rust tests (`stage1_project_imports_synthetic_stdlib_net_module`,
    `stage1_project_rejects_stdlib_net_without_net_capability`).
  - `std.process` — **landed** as `std/process.ax` exposing
    `run_status(command: string): int` on top of the existing `process_status`
    intrinsic. Covered by `stage1/examples/stdlib_process` and two Rust tests
    (`stage1_project_imports_synthetic_stdlib_process_module`,
    `stage1_project_rejects_stdlib_process_without_process_capability`).
  - `std.crypto.hash` — **landed** as `std/crypto_hash.ax` (stage1 uses a flat
    filename to avoid cross-platform path separator issues in the virtual
    stdlib table) exposing `sha256(input: string): string` on top of the
    existing `crypto_sha256` intrinsic. Covered by
    `stage1/examples/stdlib_crypto_hash` and two Rust tests
    (`stage1_project_imports_synthetic_stdlib_crypto_hash_module`,
    `stage1_project_rejects_stdlib_crypto_hash_without_crypto_capability`).
  - `std.crypto.mac` — **partial** as `std/crypto_mac.ax` exposing
    `hmac_sha256(key: string, message: string): string` and
    `constant_time_eq(left: string, right: string): bool` on top of
    capability-gated compiler intrinsics. Covered by
    `stage1/examples/stdlib_crypto_mac` and two Rust tests
    (`stage1_project_imports_synthetic_stdlib_crypto_mac_module`,
    `stage1_project_rejects_stdlib_crypto_mac_without_crypto_capability`).
  - `std.io` — **landed** as `std/io.ax` exposing
    `eprintln(text: string): int` on top of a new ungated `io_eprintln`
    intrinsic that writes a line to stderr and returns the number of bytes
    written (`-1` on error). This is the first stdlib module not tied to a
    capability flag: stderr output is ambient, matching `print`'s ungated
    statement form, so the wrapper does not call `require_capability` and the
    importing package needs no manifest opt-in. Covered by
    `stage1/examples/stdlib_io` and one Rust test
    (`stage1_project_imports_synthetic_stdlib_io_module`). There is no
    companion denial test because `std.io` has no capability to withhold.
  - `std.json` — **landed** as `std/json.ax` exposing scalar/string JSON
    parsing and serialisation helpers on top of the ungated `json_parse_*` and
    `json_stringify_*` intrinsics, plus manual `field_*` / `object*` builders
    for deterministic object encoding. Covered by `stage1/examples/stdlib_json`
    and two Rust tests (`stage1_project_imports_synthetic_stdlib_json_module`,
    `stage1_project_rejects_stdlib_json_with_wrong_argument_type`).
  - `std.http` — **partially landed** as `std/http.ax` exposing
    `get(url: string): Option<string>`, the intermediate loopback-only
    `serve_once(bind: string, body: string): bool` smoke primitive, and the
    route-shaped helpers `route(path: string, body: string): HttpRoute`,
    `respond(body: string): HttpRoute`, and
    `serve(bind: string, selected_route: HttpRoute, max_requests: int): bool`
    on top of the `http_get`, `http_serve_once`, and loopback-only
    `http_serve_route` intrinsics. The client path implements a blocking
    HTTP/1.0 fetch for `http://` and `https://` URLs in the generated Rust
    runtime; TLS failures return `None` and emit a structured `net`
    diagnostic. The server path is intentionally narrow: it accepts only
    loopback bind addresses, serves plain-text HTTP/1.0 responses, and exits
    after one request for `serve_once` or after the bounded `max_requests`
    count for `serve`. Both intrinsics share the existing `net` capability
    because any code that can open a raw TCP socket could implement HTTP
    itself, so a separate `http` manifest flag would not add meaningful
    isolation in stage1. Covered by `stage1/examples/stdlib_http` and Rust
    tests (`stage1_project_imports_synthetic_stdlib_http_module`,
    `stage1_stdlib_http_get_supports_https_urls`,
    `stage1_stdlib_http_reports_tls_diagnostics`,
    `stage1_stdlib_http_service_serves_one_request`,
    `stage1_stdlib_http_service_rejects_non_loopback_bind`,
    `stage1_stdlib_http_routed_service_rejects_non_loopback_bind`,
    `stage1_stdlib_http_service_routes_multiple_requests`,
    `stage1_project_rejects_stdlib_http_without_net_capability`, and
    `stage1_project_rejects_stdlib_http_service_without_net_capability`).
    This remains an intermediate #97 slice: it covers simple request routing,
    response helpers, bounded lifecycle behavior, loopback bind enforcement,
    and native threaded request fan-out, but the final async-runtime
    listen/accept/respond API remains AG4.3 work.

  - `std.collections` — **landed** as `std/collections.ax` exposing generic
    borrowed-slice helpers (`count`, `is_empty`, `has_items`, `skip`, `take`,
    and `window`) on top of AG2 generic functions plus existing collection
    primitives. Covered by `stage1/examples/stdlib_collections` and one Rust
    test (`stage1_project_imports_synthetic_stdlib_collections_module`).
  - `std.string_builder` — **landed** as `std/string_builder.ax` exposing
    `StringBuilder`, `builder`, `from_string`, `push_str`, `push_line`, and
    `finish` as a pure owned string accumulator. This is not a growable generic
    vector or map substitute. Covered by `stage1/examples/stdlib_string_builder`
    and one Rust test
    (`stage1_project_imports_synthetic_stdlib_string_builder_module`).
  - `std.log` — **landed** as `std/log.ax` exposing deterministic JSON-line
    event formatting, levels, key-value attributes, and ambient stderr logging.
    It deliberately does not add host log sinks, runtime filtering, or replay
    buffers. Covered by `stage1/examples/stdlib_log` and one Rust test
    (`stage1_project_imports_synthetic_stdlib_log_module`).
  - `std.sync` — **landed** as `std/sync.ax` exposing ownership-shaped
    primitives (`Mutex`, `MutexGuard`, `Once`, and `Channel`) implemented in
    Axiom without host-thread capabilities. The stage1 channel is single-slot
    and nonblocking. Covered by `stage1/examples/stdlib_sync` and one Rust test
    (`stage1_project_imports_synthetic_stdlib_sync_module`).
- `AG4.2`: async runtime — **landed for deterministic stage1 execution** with
  `async fn`, `await`, `Task<T>`, `JoinHandle<T>`, `AsyncChannel<T>`,
  cancellation, timeouts, and `select` exposed by `std/async.ax`. Stage1 still
  does not provide host-thread scheduling, blocking wakeups, or real timers.
  Covered by `stage1/examples/stdlib_async` and one Rust integration test
  (`stage1_project_supports_async_runtime_surface`).
- `AG4.3`: HTTP service support — **partial / intermediate** via the loopback-only blocking `std/http.ax::serve_once(bind, body)` smoke primitive and bounded `std/http.ax::serve(bind, route, max_requests)` helper. This does **not** close #97; full service support still needs the async-runtime listen/accept/respond API shape, richer lifecycle controls, and acceptance coverage.

- `AG4.4`: capability-aware integration
  - **landed for the current stdlib/runtime surface**: compiler-known
    intrinsics enforce all six manifest flags, stdlib wrappers preserve that
    enforcement against the importing package's manifest, capability-denied
    programs fail before native execution, and the Rust suite covers both
    per-wrapper denial paths and cross-package capability interactions
    (`dependency_package_must_enable_its_own_capabilities`).

Acceptance:

- Stage1 can build and run a small HTTP service, not just scripts and workers.
- File I/O, JSON, process execution, HTTP client/server, async coordination, and
  cancellation are covered by stage1 integration tests.

### AG5: Agent-grade compiler closure

Goal: make the stage1 public workflow complete enough to call the compiler workable.

Work packages:

- `AG5.1`: `axiomc test`
  - Stabilize the public stage1 test command for package/workspace-level test
    execution and carry it from bootstrap source discovery plus golden-output
    assertions to the agent-grade proof workloads.
- `AG5.2`: stable JSON contract
  - Lock JSON diagnostics for `check`, `build`, `test`, and `caps`.
- `AG5.3`: proof workload fixtures
  - Add checked-in end-to-end examples for:
    - multi-package CLI
    - queue-style worker
    - small HTTP service
- `AG5.4`: CI closure
  - Treat the three proof workloads as blocking acceptance tests in CI.

Agent-grade closure bar:

- A multi-package CLI builds and runs under `axiomc`.
- A queue-style worker builds and runs under `axiomc`.
- A small HTTP service builds and runs under `axiomc`.
- All three use stage1 capability-gated APIs.
- The user-facing workflow for those stage1 programs stays within `axiomc`.

## Public interfaces and contracts

- Manifest contract remains `axiom.toml` plus `axiom.lock`.
- The agent-grade milestone does not promise a direct native backend.
- `axiomc test` is part of the required public surface before AG5 closes.
- JSON diagnostics on `check`, `build`, `test`, and `caps` are part of the public contract at AG5.

## Working rules for agents

- One AG work package per PR. Do not combine ownership, generics, package-graph,
  runtime, and backend work in the same change.
- AG2 work starts only after AG1 ownership behavior is stable enough to represent
  borrowed data inside generic signatures without new bootstrap exceptions.
- AG4 work depends on AG3 capability enforcement. Do not ship stdlib modules
  that bypass capability checks.
- AG5 closure work depends on AG3 and AG4 being functional enough to support the
  CLI, worker, and HTTP-service fixtures.
- Keep the Rust-only verification gate green:
  - `make stage1-test`
  - `make stage1-conformance`
  - `make stage1-smoke`

## Post-threshold follow-ons

After AG5 closes, the next compiler track is:

- replace generated-Rust codegen with a direct native backend
- add `fmt`, `bench`, `doc`, `publish`, and LSP
- keep benchmark gates against simple Go and Rust references green
