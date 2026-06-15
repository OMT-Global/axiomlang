# Direct Native Runtime ABI v0

This document defines the first contract for runtime values and host-facing
runtime services that a direct native backend must support before generated
Rust can stop carrying the broad stage1 runtime surface.

The contract is backend-neutral. It describes Axiom values, capabilities, and
effects; it does not make generated Rust, Rust types, Cargo, or `rustc` part of
the semantic model.

Machine-readable contract:
`stage1/runtime-abi/direct-native-v0.json`

Validation:

```bash
make stage1-direct-native-runtime-abi
```

Focused evidence gate:

```bash
make stage1-direct-native-runtime-abi-evidence
```

The evidence gate validates the machine-readable ABI contract, runs the
Cranelift backend evidence suite that backs the current `partial` and
denial-evidence rows, and verifies the `axiomc run/test --backend cranelift`
command paths can execute without generated-Rust artifacts. It is intentionally
not a readiness claim while rows remain `partial` or `blocked`.

## Contract Shape

The v0 contract has two groups:

- `value_features`: runtime representations the direct native backend must
  carry across function calls, aggregate projections, returns, pattern matches,
  and owned/borrowed access.
- `capability_shims`: backend-neutral runtime entrypoints for host services
  that are currently implemented through the generated-Rust runtime path.

Every row has one of these statuses:

- `implemented`: direct native builds support the row for the supported stage1
  surface through runtime entrypoints or backend-emitted codegen, and the row
  names `runtime_evidence`.
- `partial`: direct native builds support a narrower subset and the row names
  the remaining issue.
- `blocked`: direct native builds do not yet support the row broadly enough for
  Rust exit.

Rows that are not `implemented` must name at least one blocker issue. Rows that
are `implemented` must not name blockers. Compiler-side Cranelift spike
evaluation can be recorded as `evidence` on a partial row, but it does not by
itself satisfy `runtime_evidence` or prove runtime support.

## Required Value Features

The direct native backend must support:

- numeric scalars across signed, unsigned, and floating-width forms;
- booleans and strings;
- fixed arrays and borrowed slices;
- maps and map lookup helpers;
- tuples;
- `Option<T>` and `Result<T, E>`;
- enums with tuple and named payloads;
- structs and field projections;
- owned values and move-state preserving calls.

## Required Capability Shims

The direct native backend must provide runtime entrypoints for the supported
stage1 capability and stdlib surface:

- filesystem read and write operations scoped by the package manifest;
- network DNS, TCP, UDP, HTTP client, HTTP server, and async HTTP service
  operations;
- process status execution;
- environment reads with manifest allowlists;
- clock and sleep operations;
- crypto hash, MAC, random, signature, and AEAD helpers;
- FFI calls;
- async task, channel, timeout, and scheduler operations;
- JSON and serdes operations;
- regex matching and replacement;
- sync primitives;
- logging and stderr/stdin/stdout helpers.

Capability denials must remain backend-neutral: a denied host service must fail
at check time, or through the same documented manifest policy, before the direct
native backend attempts lowering or native execution.

## Current Status

The checked-in contract is intentionally not ready. It records compiler-side
Cranelift/direct-native spike evidence and keeps the affected runtime rows
partial until real runtime entrypoints or backend-emitted codegen land. This
lets future backend slices update the contract as runtime support lands without
pretending the spike already proves direct-native runtime coverage.

The `numeric.scalars` row now has the first narrow `runtime_evidence`: the
`axiomc` Cranelift build path can lower zero-argument `main(): int` and
`main(): i64`, `main(): i32`, and `main(): u32` entrypoints with straight-line
literal/local `int` and `i64` expressions, integer typed numeric literals cast
to `int`, `int` values round-tripped through `i64`, local initializer
expressions, helper `int -> int` and `int/i64` boundary functions, simple
`int`, `i64`, signed `i8`/`i16`/`i32` and `isize` helper arguments, locals,
returns, and casts, narrow unsigned `u8`/`u16`/`u32` helper arguments, locals,
returns, and casts, explicit narrow integer casts with unsigned truncation and
signed extension, typed narrow integer arithmetic and helper return values cast
back to their source width, immediate tuple-literal scalar indexing,
scalar projection from local tuple bindings, immediate array-literal scalar
indexing with literal indexes, scalar projection from local fixed-array
bindings, immediate struct-literal scalar field access, scalar projection from
local struct bindings, and explicitly cast typed-integer static values,
multi-argument helper calls, recursive helper-to-helper calls, and nested i64
arithmetic into Cranelift functions, calls, locals, and
add/sub/mul/signed-div arithmetic instructions. It also
lowers source-level `while` loops with scalar numeric and bool local assignment,
scoped runtime scalar `let` declarations, scoped runtime tuple, fixed-array,
and struct scalar projection `let` declarations, and scalar-projection aggregate
reassignment in loop bodies, after loops, and final return branches, and nested
branch statements into Cranelift loop, branch, return-block, and assignment
instructions in both the entrypoint and helper functions, including helper
function loop bodies with scoped runtime `let` declarations, then returns the
computed value as the process exit status at runtime without generated Rust.
The same path now has narrow boolean runtime
evidence for signed i64 comparisons, bool local bindings backed by i64 slots,
simple bool static values, and boolean literals composed with `&&`/`||` driving
an `if` branch whose arms return direct-native i64 expressions. It also covers a
zero-argument
`main(): bool` that calls bool-returning helpers and maps the returned condition
to process exit status `1` for true and `0` for false. Bool helper parameters
are encoded through the i64 helper ABI for literal bools, static bools, dynamic
bool locals, and comparison expressions. Bool-returning helpers can return
condition expressions directly, use bool parameters in branch conditions, and
cover final `if` branches whose arms return bool expressions. It also covers
boolean equality/inequality between dynamic bool expressions, local/static bool
values, and boolean literals in conditions, plus immediate tuple-literal bool
indexing, bool projection from local tuple bindings, immediate array-literal
bool indexing, bool projection from local fixed-array bindings, immediate
struct-literal bool field access, and bool projection from local struct
bindings for bool locals, helper returns, and boolean conditions. The backend
crate has narrow object-link evidence for composed `&&`/`||` comparison conditions,
condition-to-i64 value lowering for helper-call arguments, and bool local
assignment through a branch inside a loop after a scoped runtime bool `let`.
Both rows remain partial because that runtime path does not yet cover the full
supported scalar, function-call, control-flow, or boolean surface.

The `array.fixed` row now has narrow direct-native runtime evidence for
immediate array-literal scalar indexing with literal indexes and scalar
projection from local fixed-array bindings, including runtime-scope loop-body
bindings and reassignment of scalar-projection array locals. Numeric
projections can feed `int` and typed integer locals; boolean projections can
feed bool locals, helper return conditions, and composed boolean conditions.
Fixed-size scalar and bool array helper parameters lower across direct-native
function-call boundaries as one ABI slot per element for local array values and
inline array literals. Local and helper-parameter fixed-array scalar and bool
projections also support narrow in-bounds dynamic indexes by selecting across
projected element locals. Scalar and bool fixed-array-returning helpers now
lower across direct-native function-call boundaries as one return slot per
element, with caller-side projection locals populated from the multi-slot
return; this covers literal array returns, local array binding returns,
forwarded array parameters, and branch-selected array returns. The same
projected element-slot representation now covers fixed-array payloads inside
narrow local `Option<[int; 2]>` construction and tag/payload matches. The row
remains partial because direct-native codegen still does not provide a general
array ABI, array storage for non-scalar elements, full dynamic indexing
semantics, bounds diagnostics, or a complete aggregate value passing contract.

The `tuple` row now has narrow direct-native runtime evidence for immediate
tuple-literal scalar indexing and scalar projection from local tuple bindings.
This includes runtime-scope loop-body bindings. Numeric projections can feed
`int` and typed integer locals; boolean projections can feed bool locals, helper
return conditions, and composed boolean conditions. It also covers reassignment
of scalar-projection tuple locals. Scalar and bool tuple helper parameters lower
across direct-native function-call boundaries as one ABI slot per element for
local tuple values and inline tuple literals. Scalar and bool tuple-returning
helpers now lower across direct-native function-call boundaries as one return
slot per tuple element, with caller-side projection locals populated from the
multi-slot return; this includes helpers whose final return is selected by
branch blocks with branch-local scalar values, helpers returning local tuple
bindings, and helpers forwarding tuple parameters. The row remains partial
because direct-native codegen still does not provide a general tuple ABI, tuple
storage for non-scalar elements, tuple return expressions beyond the
scalar/bool local, literal, and parameter slice, or a complete aggregate value
passing contract.

The `struct.field` row now has narrow direct-native runtime evidence for
immediate struct-literal scalar field access and scalar projection from local
struct bindings, including runtime-scope loop-body bindings. Numeric fields can
feed `int` and typed integer locals; boolean fields can feed bool locals, helper
return conditions, and composed boolean conditions. It also covers reassignment
of scalar-projection struct locals. Scalar and bool struct helper parameters
lower across direct-native function-call boundaries as one ABI slot per field
in declared field order for local struct values and inline struct literal
arguments. Scalar and bool struct-returning helpers now lower across
direct-native function-call boundaries as one return slot per declared field,
with caller-side projection locals populated from the multi-slot return; this
includes helpers whose final return is selected by branch blocks with
branch-local scalar values, helpers returning local struct bindings, and
helpers forwarding struct parameters. The row remains partial because
direct-native codegen still does not provide a general struct ABI, struct
storage for non-scalar fields, owned field projection, field mutation, struct
return expressions beyond the scalar/bool local, literal, and parameter slice,
or a complete aggregate value passing contract.

The `option` row now has narrow direct-native runtime evidence for local
`Option<int>` and `Option<bool>` construction represented as tag/payload locals,
scalar option reassignment in loop bodies, value-producing `match` expressions
over `Some(payload)` and `None` arms that feed scalar and bool locals before
returning, and `match` statements that assign scalar and bool locals from
`Some`/`None` arms. Scalar `Option<int>` and `Option<bool>` helper parameters
lower across direct-native function-call boundaries as explicit tag/payload ABI
slots for local option values and inline `Some`/`None` arguments. The
direct-native path also has narrow evidence for `Option<(int, bool)>`
construction, reassignment, matching, helper parameters, and helper returns for
local values, forwarded local or parameter values, and inline `Some((...))`/`None`
arguments represented as a tag plus multiple payload slots. The same
tag/payload-slot representation now covers local `Option<[int; 2]>`
construction and matching for inline `Some([..])`/`None` values. The row remains
partial because direct-native codegen still does not provide a general
`Option<T>` ABI across broader payload shapes, nested option values, helper ABI
coverage for array payloads, or broad aggregate storage.

The first executable guard for this boundary is a Cranelift regression that
builds a package using `std/fs.ax` without the `fs` capability and verifies the
public capability denial appears before any Cranelift unsupported-feature
diagnostic.

The `fs.read` row now has partial Cranelift evidence for `std/fs.ax`
`read_file` on present and missing filesystem names, plus denial evidence that a
package without the `fs` capability fails before backend lowering. Full
runtime-time filesystem access, manifest policy parity, and audit parity remain
open under #1001.

The DNS row now has partial Cranelift evidence: the spike builds and runs a
`std/net.ax` package resolving `localhost` through host DNS without generated
Rust and returns the public `Option<string>` shape. Packages without the `net`
capability still fail before backend lowering. Full runtime-time DNS policy,
non-loopback coverage, resolver portability, and audit parity remain open under
#1001.

The TCP row now has partial Cranelift evidence: the spike builds and runs
`std/net.ax` `tcp_listen_loopback_once(...)` over `127.0.0.1` without generated
Rust and returns a loopback port. Packages without the `net` capability still
fail before backend lowering. Paired dynamic-port dial coverage, full TCP
socket lifecycle APIs, non-loopback policy coverage, timeout parity, and audit
parity remain open under #1001.

The UDP row now has partial Cranelift evidence: the spike builds and runs
`std/net.ax` `udp_bind_loopback_once(...)` over `127.0.0.1` without generated
Rust and returns a loopback port. Packages without the `net` capability still
fail before backend lowering. Paired dynamic-port send/recv coverage, full UDP
socket lifecycle APIs, non-loopback policy coverage, timeout parity, and audit
parity remain open under #1001.

The filesystem write row now has partial Cranelift evidence: the spike
evaluates `std/fs.ax` write helpers over configured `fs_root`-scoped literal
paths during compilation and emits the resulting output, covering `mkdir_all`,
`write_file`, `append_file`, readback, `replace_file`, `create_file`,
`remove_file`, and `remove_dir`. It also covers `fs_root` scoping and preserves
the public manifest-policy denial for a package with `fs = true` and
`"fs:write" = false` that calls `std/fs.ax` `write_file(...)`. Full
runtime-time filesystem writes, atomic replace parity, TOCTOU hardening, and
audit parity remain open under #1001.

The direct-native crypto hash slice is still marked partial: the Cranelift
spike can build and run `std/crypto_hash.ax` `sha256(...)` without generated
Rust, and crypto capability denials still happen before backend lowering.
Random, signature, AEAD, and broader crypto audit parity remain open.

The direct-native crypto MAC slice is now marked partial: the Cranelift spike
can build and run `std/crypto_mac.ax` HMAC-SHA256, HMAC-SHA512, verification
helpers, string constant-time equality, and byte-slice constant-time equality
without generated Rust. A package without the `crypto` capability fails before
backend lowering. Runtime audit parity and broader crypto host-service coverage
remain blocked under #1001.

The direct-native crypto random slice is now marked partial: the Cranelift
spike can build and run `std/crypto_rand.ax` `random_bytes(...)` and
`random_u64()` through a Unix OS-random source without generated Rust, while
preserving the generated-Rust helper's `0..=65536` byte length cap. A package
without the `crypto` capability still fails before backend lowering. Portable
entropy source parity, deterministic test hooks, and runtime audit parity
remain open under #1001.

The direct-native crypto signature slice is now marked partial: the Cranelift
spike builds and runs `std/crypto_sign.ax` Ed25519 key generation, signing, and
verification without generated Rust by dynamically loading the host libcrypto
EVP provider for real cryptographic operations. Packages without the `crypto`
capability still fail before backend lowering. Runtime-integrated crypto
provider selection, deterministic test hooks, audit parity, and non-Unix support
remain open under #1001.

The direct-native crypto AEAD slice is now marked partial: the Cranelift spike
builds and runs `std/crypto_aead.ax` AES-256-GCM seal/open without generated
Rust through a dynamically loaded host OpenSSL EVP provider. Packages without
the `crypto` capability still fail before backend lowering. Runtime-integrated
crypto provider selection, broader algorithm coverage, deterministic test
hooks, audit parity, and non-Unix support remain open under #1001.

The HTTP client row now has partial Cranelift evidence: the spike builds
`std/http.ax` `get(...)` against a static allowlisted `http://127.0.0.1` URL
and fetches a local one-shot HTTP response without generated Rust. Packages
without the `net` capability still fail before backend lowering. HTTPS,
nonlocal HTTP policy coverage, redirects, richer response handling, timeout
parity, and audit parity remain open under #1001.

The HTTP server row now has partial Cranelift evidence: the spike builds and
runs loopback HTTP server entrypoints without generated Rust, covering
`http_server_listen`, `http_server_local_port`, `http_server_accept`,
`http_request_method`, `http_request_path`, `http_request_body`,
`http_response_write`, and `http_server_close` over a one-request HTTP/1.0
fixture. Packages without the `net` capability still fail before backend
lowering. Route helpers, multi-request serving, non-loopback policy coverage,
richer response metadata, timeout parity, and audit parity remain open under
#1001.

The async HTTP server row now has partial Cranelift evidence: the spike builds
and runs `http_async_serve_route` over a loopback server handle without
generated Rust, returns a `Task<bool>`, and serves a one-request HTTP/1.0 route
fixture. It also proves the async gate separately: with `net` present and
`async` missing, `std/http_async.ax` `async_serve_route(...)` must fail through
the public `async` capability denial before backend lowering. Real
scheduler-backed serving, concurrent clients, cancellation, timeout parity,
non-loopback policy coverage, and audit parity remain open under #1001.

The process status row now has partial compiler-side spike evidence: the
Cranelift spike builds and runs `std/process.ax` `run_status(...)` for literal,
allowlisted deterministic commands through compiler-side spike evaluation and
emits their exit statuses without generated Rust. Denied `process` capability
use still fails through the manifest policy before Cranelift lowering or native
execution. Full runtime-time process execution, argument handling, audit parity,
and host-process policy coverage remain open under #1001.

The string row has partial direct-native evidence: the Cranelift spike now
builds and runs pure string intrinsics including `string_clone`,
`string_starts_with`, `string_strip_prefix`, `string_strip_suffix`,
`string_trim`, `string_trim_start`, and `string_line_at` without generated
Rust. It also builds and runs `std/string_builder.ax` owned string accumulator
helpers and `std/encoding.ax` percent encode/decode helpers, query-pair
encoding, and path segment joining without generated Rust. Broader string ABI
coverage, allocation behavior, and host-boundary representation remain tracked
by issue #1001.

The borrowed-slice row has partial direct-native evidence: the Cranelift spike
now evaluates array-backed borrowed slices through `len`, `first`, `last`,
indexing, and function returns. Broader borrowed-slice aliasing and host ABI
coverage remain tracked by issue #1001.

The map lookup row has partial direct-native evidence: the Cranelift spike now
builds and runs direct map indexing, `get`, `get_or_default`,
`map_contains_key`, `map_keys`, and the public `std/collections.ax` `contains`,
`get`, `get_or_default`, and `keys` helpers for string and integer key/value
shapes without generated Rust. Broader map ownership and host-boundary
representation remain tracked by issue #1001.

The `env.read` row now has partial Cranelift evidence for `std/env.ax`
`get_env` on present and missing environment names without generated Rust, plus
denial evidence that a package without the `env` capability fails before
backend lowering. Full runtime-time lookup, manifest allowlist parity, and
audit parity remain open under #1001.

The FFI call row now has partial Cranelift evidence: the spike builds and runs
a narrow C ABI `extern fn strlen(value: string): int from "c"` fixture without
generated Rust, using the source-level extern declaration. A package with an
`extern fn` declaration and no `ffi` capability must still receive its public
manifest-policy denial before any Cranelift-specific lowering diagnostic. Broad
dynamic symbol loading, pointer and mutable-pointer ABI shapes, non-string
arguments, ownership safety, platform library resolution, and audit parity
remain open under #1001.

The async runtime row now has partial Cranelift evidence for `std/async.ax`
`ready`, `await`, `spawn`, `join`, `cancel`, `is_canceled`, `timeout`,
single-slot channel `send`/`recv`, `select`, `selected`, and `selected_value`
without generated Rust. A package importing `std/async.ax` with no `async`
capability must still receive the public manifest-policy denial before backend
lowering. Full scheduler, timer, blocking, wakeup, cancellation, and audit
parity remain open under #1001.

The sync-primitives row has partial direct-native evidence: the Cranelift spike
now evaluates ownership-shaped `std/sync.ax` mutex, once, and channel wrappers
and emits the expected native output. Concurrent execution, blocking behavior,
and host runtime synchronization remain tracked by issue #1001.

The `Result<T, E>` row has partial direct-native evidence: the Cranelift spike
now builds and runs a package importing `std/outcome.ax`, using result
predicates, fallback unwrap helpers, direct match arms over `Result<T, E>`
values, scalar payloads, string errors, and struct payloads. The direct-native
runtime path now also has narrow evidence for local `Result<int, int>`,
`Result<bool, bool>`, `Result<int, bool>`, and `Result<bool, int>` `Ok` and
`Err` construction and reassignment, plus typed numeric `Result<i32, u32>`
`Result<i64, u16>`, and `Result<u8, i8>` `Ok`/`Err` construction and
reassignment, represented as tag/payload locals and value-producing `match`
expressions over `Ok(payload)` and `Err(error)` that can feed scalar or bool
locals and the process exit status. It also covers `match` statements that
assign scalar and bool locals from `Ok`/`Err` arms. Those Result helper
parameters lower across direct-native function-call boundaries as explicit
tag/payload ABI slots for local values and inline `Ok`/`Err` arguments without
generated Rust. The direct-native path also has narrow evidence for
`Result<(int, bool), int>` and `Result<(int, bool), (int, bool)>` `Ok`/`Err`
construction, reassignment, matching, and helper parameters for local values and
inline `Ok`/`Err` arguments represented as a tag plus multiple payload slots.
That same tag/payload representation now covers helper returns and forwarded
local or parameter values for the `Result<(int, bool), int>` narrow slice.
Broader Result ABI support, the full numeric-width matrix, additional aggregate
payload shapes, and capability-shim coverage remain tracked by issue #1001.

The `enum.payload` row now has narrow direct-native runtime evidence for local
custom enum construction, reassignment, value-producing matches, and statement
matches over scalar/bool positional and named payload variants, represented as a
tag plus payload slots and returned as process exit status without generated
Rust. The same tag/payload-slot representation now covers narrow scalar tuple
and scalar struct payload storage, matching, and helper parameters for named
custom enum payloads such as `(int, bool)` and `Step { value: int, enabled:
bool }`. Scalar/bool custom enum helper parameters lower across direct-native
function-call boundaries as explicit tag/payload ABI slots for local values and
inline variant arguments. Narrow custom enum helper returns and forwarded local
or parameter values also lower through the same tag/payload slots for scalar
struct payload variants. Broader enum ABI support and aggregate payload storage
beyond scalar tuples/structs remain tracked by issue #1001.

The `json.serdes` row has expanded partial direct-native evidence: the
Cranelift spike now builds and runs `std/json.ax` scalar/object helpers and
`std/serdes.ax` `Value` object-map construction, nested JSON object/array
parsing, typed field accessors, value indexing, stringify, and parse-error
reporting without generated Rust. Schema validation and broader JSON value
modeling remain tracked by issue #1001.

The owned move-state row has partial direct-native evidence: the Cranelift
spike builds and runs projection-sensitive owned field moves while preserving
access to disjoint sibling projections. Broader move-state, lifetime, and host
ABI coverage remain tracked by issue #1001.

The logging/stdio row has partial direct-native evidence: the Cranelift spike
now evaluates `std/io.ax` stderr writes and `std/log.ax` structured event
formatting plus `info_attrs` stderr emission, then emits the resulting stdout
and stderr streams from the native binary. Stdin reads and broader
streaming/runtime buffering remain tracked by issue #1001.

The `clock.now_sleep` row now has partial Cranelift evidence for `std/time.ax`
`now_ms`, `now`, `elapsed_ms`, and zero-duration `sleep`, plus guards that a
package without the `clock` capability fails before backend lowering and that
nonzero sleep fails fast instead of ever reaching host sleep during
compiler-side spike evaluation. The spike intentionally keeps the supported
sleep shape limited to zero-duration calls until the real runtime clock path
lands. Full runtime-time clock/sleep execution, timer scheduling, async clock
integration, and audit parity remain open under #1001.



## Rust Capture Check

This ABI describes Axiom runtime values and host-service effects. Rust may
remain the current implementation host while this contract is incomplete, but
Rust spelling and generated-Rust helper internals do not define the contract.
