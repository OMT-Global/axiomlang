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

Checked-in example smoke:

```bash
make stage1-direct-native-example-smoke
```

The evidence gate validates the machine-readable ABI contract, runs the
Cranelift backend evidence suite that backs the current `partial` and
denial-evidence rows, and verifies the `axiomc run/test --backend cranelift`
command paths can execute without generated-Rust artifacts. It is intentionally
not a readiness claim while rows remain `partial` or `blocked`.

The example smoke runs a bounded subset of checked-in value and stdlib examples
through `check`, `build --backend cranelift`, and `run --backend cranelift`, and
asserts the build/run JSON reports `generated_rust: null`. The current set
covers 53 deterministic examples across scalar/aggregate values, borrowed
shapes, generic aggregates, modules/packages/workspaces, governance/service
fixtures, property fixtures, workspace-only package selection, outcome/result
helpers, JSON value and serdes helpers, LSP/doc/testing helpers, plus async,
CLI's no-argument path, collections, crypto hash/MAC, env allowlisted and
unrestricted-migration reads, encoding, fs read/write, HTTP's closed-port
client path, io, JSON, logging, process-status missing-binary handling, regex,
sync, string builder, and time. It is direct-native example evidence for #1001,
not a
replacement for full
`stage1-smoke` parity; examples that still require broader capability policy or
runtime parity remain outside this smoke target.

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
returns, and casts, unsigned `u8`/`u16`/`u32`/`u64`/`usize` helper arguments,
locals, returns, and casts, explicit narrow integer casts with unsigned
truncation and signed extension, typed narrow integer arithmetic and helper
return values cast back to their source width, high-bit unsigned local values,
unsigned comparison predicates, and unsigned division, immediate tuple-literal
scalar indexing,
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

The `string` row now has narrow direct-native runtime evidence for `len(...)`
over string literals, string statics initialized from literals, and string locals
initialized from known literal or static string values. Known-text
`string_clone(...)`, `string_trim(...)`, and `string_trim_start(...)` results can
also feed this same direct-native path, and `string_clone(...)` can pass through
supported runtime string length projection locals. Pure helper calls with known
string arguments can now fold string helper parameters and returns into the same
direct-native length, comparison, and `string_starts_with(...)` condition paths
without generated Rust when the helper body is a direct return, pure local
`let` bindings followed by a return, a pure final `if` whose branches return,
or a pure match-return expression or final match statement over known enum values
such as `Option<string>`, including known string projections from tuple indexes
and struct fields, plus direct indexes into known map literals. String length is
represented as a byte-length projection local, matching the generated-Rust
backend and Cranelift spike `.len()` semantics, and can feed direct-native
integer locals, comparisons, helper calls, runtime branch-local string
projection `let`s, and process exit status without generated Rust.
String concatenation length also lowers for supported string length projection
inputs by adding the operand byte lengths without materializing the concatenated
runtime string.
Literal-, static-, local-, and known-string-call-backed `string_starts_with(...)`
predicates and known-text string comparisons now also lower directly to native
boolean conditions, including bool locals, helper returns, composed branch
conditions, and process exit status without generated Rust. Known-input
`string_strip_prefix(...)`, `string_strip_suffix(...)`, and `string_line_at(...)`
calls can also lower direct `match` expressions by selecting the `Some` or
`None` arm at compile time and binding the `Some` payload as a known string fact
for that arm, including static scalar `string_line_at(...)` indexes. Known-text
`encoding_url_component_encode(...)`,
`encoding_path_segment_encode(...)`, `encoding_url_query_pair_encode(...)`, and
`encoding_path_join_segment(...)` calls can feed the same direct-native string
length and comparison path, and known-input `encoding_url_component_decode(...)`
can lower direct `Option<string>` matches by compile-time arm selection.
Imported public `std/encoding.ax` wrappers now alias those same known-input
encode, decode, query-pair, and path-join lowering paths.
Known-input `crypto_sha256(...)`, `crypto_hmac_sha256(...)`, and
`crypto_hmac_sha512(...)` calls can also feed direct-native string length and
comparison paths after normal front-end crypto capability checks. Supported
runtime scalar/bool string-projection inputs, directly or through
`string_clone(...)` over a projection local, can feed crypto hash/MAC length
projections without materializing a general runtime string value. Known-input
`crypto_constant_time_eq(...)` over known string values can also feed
direct-native boolean conditions after normal front-end crypto capability
checks. Imported public `std/crypto_hash.ax` and `std/crypto_mac.ax` hash, HMAC,
verify, and constant-time equality wrappers now alias those same known-input
direct-native paths in runtime-exit programs. The row
remains partial because direct-native codegen still does not provide a general
string ABI, general runtime string parameters or returns, allocation or mutation
behavior, non-literal string storage, general Option-string payload storage or
helper ABI, broad string, encoding, or crypto string intrinsic lowering, or
host-boundary representation.

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
projected element locals. Inline scalar and bool fixed-array literals also
support narrow in-bounds dynamic indexes by selecting across lowered literal
elements. Scalar and bool fixed-array-returning helpers now lower across
direct-native function-call boundaries as one return slot per element, with
caller-side projection locals populated from the multi-slot return; this covers
literal array returns, local array binding returns, forwarded array parameters,
and branch-selected array returns. The same
projected element-slot representation now covers fixed-array payloads inside
narrow `Option<[int; 2]>` and `Result<[int; 2], [int; 2]>` construction,
tag/payload matches, helper parameters, helper returns, and forwarded helper
values. Existing fixed-array locals can now be reassigned from fixed-array
helper returns using the same element-slot ABI, including inside runtime loop
blocks. Fixed-array `len`, `first`, and `last` over scalar and bool element
arrays also lower through the same projected element-slot representation for
local arrays, inline literals, helper parameters, and helper-returned arrays
feeding a direct-native process exit status. The row remains partial because
direct-native codegen still does not provide a general array ABI, array storage
for non-scalar elements, full dynamic indexing semantics, bounds diagnostics,
or a complete aggregate value passing contract.

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
bindings, and helpers forwarding tuple parameters. Existing tuple locals can
now be reassigned from tuple helper returns using the same tuple-element ABI,
including inside runtime loop blocks. The row remains partial because
direct-native codegen still does not provide a general tuple ABI, tuple storage
for non-scalar elements, tuple return expressions beyond the scalar/bool local,
literal, and parameter slice, or a complete aggregate value passing contract.

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
helpers forwarding struct parameters. The same declared-field slot
representation now backs narrow `Option<Step>` and `Result<Step, Step>` struct
payload construction, matching, helper parameters, helper returns, forwarded
helper values, and inline `Some(Step { ... })`/`None` and
`Ok(Step { ... })`/`Err(Step { ... })` helper arguments. Existing struct locals
can now be reassigned from struct helper returns using the declared-field slot
ABI, including inside runtime branch blocks. The row remains partial because
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
tag/payload-slot representation now covers `Option<[int; 2]>` construction,
matching, helper parameters, helper returns, forwarded helper values, and inline
`Some([..])`/`None` helper arguments. The same representation now covers narrow
`Option<Step>` struct payload construction, expression and statement matches,
helper parameters, helper returns, forwarded helper values, and inline
`Some(Step { ... })`/`None` helper arguments using declared field-order payload
slots. Existing narrow `Option<Step>` locals can now be reassigned from option
helper returns using the same tag/payload slots, including inside runtime branch
blocks. The direct-native path also has narrow evidence for nested
`Option<Option<int>>` construction, reassignment, matching, helper parameters,
helper returns, forwarded helper values, and inline `Some(Some(...))`,
`Some(None)`, and outer `None` helper arguments using nested tag/payload slots.
The same nested slot representation now has narrow evidence for
`Option<Result<int, int>>` construction, reassignment, matching, helper
parameters, helper returns, forwarded helper values, and inline
`Some(Ok(...))`, `Some(Err(...))`, and outer `None` helper arguments.
The row remains partial because direct-native codegen still does not provide a
general `Option<T>` ABI across broader payload shapes, deeper nested option or
result values, or broad aggregate storage.

The first executable guard for this boundary is a Cranelift regression that
builds a package using `std/fs.ax` without the `fs` capability and verifies the
public capability denial appears before any Cranelift unsupported-feature
diagnostic.

The `fs.read` row now has partial Cranelift evidence for `std/fs.ax`
`read_file` on present and missing filesystem names, plus denial evidence that a
package without the `fs` capability fails before backend lowering. The
direct-native i64 path now also lowers literal-path `fs_read(...)` calls and the
public `std/fs.ax` `read_file(...)` wrapper into native process exit status by
selecting `Option<string>` match arms at compile time for present and missing
package-root-scoped files, using the same canonicalized `fs_root`, file-only,
UTF-8, and maximum-size guard as the Cranelift spike. Full runtime-time
filesystem access, write-side filesystem wrappers, manifest policy parity,
runtime filesystem binding, and audit parity remain open under #1001.

The DNS row now has partial Cranelift evidence: the spike builds and runs a
`std/net.ax` package resolving `localhost` through host DNS without generated
Rust and returns the public `Option<string>` shape. The direct-native i64 path
now also lowers known-host `net_resolve(...)` calls and the public `std/net.ax`
`resolve(...)` wrapper into native process exit status by selecting
`Option<string>` match arms at compile time for `localhost`. Packages without
the `net` capability still fail before backend lowering. Full runtime-time DNS
policy, non-loopback coverage, resolver portability, and audit parity remain
open under #1001.

The TCP row now has partial Cranelift evidence: the spike builds and runs
`std/net.ax` `tcp_listen_loopback_once(...)` over `127.0.0.1` without generated
Rust and returns a loopback port. The spike now also builds and runs
`std/async_net.ax` loopback TCP `listen`, `local_port`, `accept`, `recv_text`,
`send_text`, `close`, `close_listener`, and paired `tcp_dial` flows without
generated Rust. The direct-native i64 path now also lowers known-response
`net_tcp_listen_loopback_once(...)` and public `std/net.ax`
`tcp_listen_loopback_once(...)` calls into native process exit status by
selecting `Option<int>` match arms at compile time for successful loopback
binds. Packages without the `net` capability still fail before backend
lowering. General runtime-time TCP socket lifecycle APIs, non-loopback policy
coverage, timeout parity, and audit parity remain open under #1001.

The UDP row now has partial Cranelift evidence: the spike builds and runs
`std/net.ax` `udp_bind_loopback_once(...)` over `127.0.0.1` without generated
Rust and returns a loopback port. The direct-native i64 path now also lowers
known-response `net_udp_bind_loopback_once(...)` and public `std/net.ax`
`udp_bind_loopback_once(...)` calls into native process exit status by selecting
`Option<int>` match arms at compile time for successful loopback binds. Packages
without the `net` capability still fail before backend lowering. Paired
dynamic-port send/recv coverage, full UDP socket lifecycle APIs, non-loopback
policy coverage, timeout parity, and audit parity remain open under #1001.

The filesystem write row now has partial Cranelift evidence: the spike
evaluates `std/fs.ax` write helpers over configured `fs_root`-scoped literal
paths during compilation and emits the resulting output, covering `mkdir_all`,
`write_file`, `append_file`, readback, `replace_file`, `create_file`,
`remove_file`, and `remove_dir`. It also covers `fs_root` scoping and preserves
the public manifest-policy denial for a package with `fs = true` and
`"fs:write" = false` that calls `std/fs.ax` `write_file(...)`. The
direct-native i64 path now also lowers literal-path `mkdir_all`, `write_file`,
`append_file`, `replace_file`, `create_file`, `remove_file`, and `remove_dir`
calls, including public `std/fs.ax` wrappers, into native process exit status
without generated Rust by executing the same `fs_root`-scoped candidate checks
and status-code convention used by the spike. Full runtime-time filesystem
writes, atomic replace parity, TOCTOU hardening, and audit parity remain open
under #1001.

The direct-native crypto hash slice is still marked partial: the Cranelift
spike can build and run `std/crypto_hash.ax` `sha256(...)` without generated
Rust, and crypto capability denials still happen before backend lowering. The
direct-native i64 path now also lowers known-input `crypto_sha256(...)` string
results and imported public `std/crypto_hash.ax` `sha256(...)` wrapper results
into length and comparison conditions that can feed a native process exit
status without generated Rust. Supported runtime string-projection inputs can
also feed fixed SHA-256 hex length projections directly or through
`string_clone(...)` over a projection local without materializing a general
runtime string value.
Random, signature, AEAD, dynamic runtime hash execution, and broader crypto
audit parity remain open.

The direct-native crypto MAC slice is now marked partial: the Cranelift spike
can build and run `std/crypto_mac.ax` HMAC-SHA256, HMAC-SHA512, verification
helpers, string constant-time equality, and byte-slice constant-time equality
without generated Rust. A package without the `crypto` capability fails before
backend lowering. The direct-native i64 path now also lowers known-input
`crypto_hmac_sha256(...)` and `crypto_hmac_sha512(...)` string results into
length and comparison conditions that can feed a native process exit status
without generated Rust. Supported runtime string-projection inputs can also feed
fixed HMAC hex length projections directly or through `string_clone(...)` over
a projection local without materializing a general runtime string value. Known-input
`crypto_constant_time_eq(...)` over known string values lowers into native
boolean conditions. It also lowers
`crypto_constant_time_eq_u8(...)` over narrow fixed-array/static-slice `u8`
inputs into native boolean conditions. Imported public `std/crypto_mac.ax`
wrappers for `hmac_sha256(...)`, `hmac_sha512(...)`,
`constant_time_eq(...)`, `constant_time_eq_u8(...)`, `verify_sha256(...)`, and
`verify_sha512(...)` now alias those same known-input direct-native paths in a
runtime-exit program without generated Rust. Runtime audit parity, dynamic
runtime MAC execution, general byte-slice runtime equality, and broader crypto
host-service coverage remain blocked under #1001.

The direct-native crypto random slice is now marked partial: the Cranelift
spike can build and run `std/crypto_rand.ax` `random_bytes(...)` and
`random_u64()` through a Unix OS-random source without generated Rust, while
preserving the generated-Rust helper's `0..=65536` byte length cap. The
direct-native i64 path now also lowers public `std/crypto_rand.ax`
`random_u64()` into native process exit status through the same Unix OS-random
source. It also lowers `len(random_bytes(n))` for literal and static scalar
nonnegative lengths up to the stage1 65,536 byte cap into native process exit
status without materializing a general byte-array value. A package without the
`crypto` capability still fails before backend lowering. Direct-native
`random_bytes(...)` byte storage and contents, portable entropy source parity,
deterministic test hooks, and runtime audit parity remain open under #1001.

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
and fetches a local one-shot HTTP response without generated Rust. The
direct-native i64 path now also lowers known-url `http_get(...)` and public
`std/http.ax` `get(...)` calls into native process exit status by selecting
`Option<string>` match arms at compile time for local HTTP responses. Packages
without the `net` capability still fail before backend lowering. HTTPS,
nonlocal HTTP policy coverage, redirects, richer response handling, timeout
parity, and audit parity remain open under #1001.

The HTTP server row now has partial Cranelift evidence: the spike builds and
runs loopback HTTP server entrypoints without generated Rust, covering
`http_server_listen`, `http_server_local_port`, `http_server_accept`,
`http_request_method`, `http_request_path`, `http_request_body`,
`http_response_write`, and `http_server_close` over a one-request HTTP/1.0
fixture. The direct-native i64 path now also lowers known-bind
`http_serve_once(...)`, `http_serve_route(...)`, and public `std/http.ax`
`serve_once(...)` calls into native process exit status by selecting bool
branches at compile time for local HTTP responses, including a two-request
routed fixture. Packages without the `net` capability still fail before backend
lowering. Non-loopback policy coverage, richer response metadata, timeout
parity, and audit parity remain open under #1001.

The async HTTP server row now has partial Cranelift evidence: the spike builds
and runs `http_async_serve_route` over a loopback server handle without
generated Rust, returns a `Task<bool>`, and serves a one-request HTTP/1.0 route
fixture. It also proves the async gate separately: with `net` present and
`async` missing, `std/http_async.ax` `async_serve_route(...)` must fail through
the public `async` capability denial before backend lowering. Real
scheduler-backed serving, concurrent clients, cancellation, timeout parity,
non-loopback policy coverage, and audit parity remain open under #1001.

The process status row now has partial direct-native evidence: the Cranelift
spike builds and runs `std/process.ax` `run_status(...)` for literal,
allowlisted deterministic commands and the checked-in missing-binary sentinel
through compiler-side spike evaluation and emits their exit statuses without
generated Rust. The direct-native i64 path also lowers literal
`process_status(...)` calls and the `std/process.ax` `run_status(...)` wrapper
for deterministic `/usr/bin/true`, `/usr/bin/false`, and
`__axiom_stage1_missing_binary__` commands into native process exit status
without generated Rust. Denied `process` capability use still fails through the
manifest policy before Cranelift lowering or native execution. Full
runtime-time process execution, argument handling, audit parity, and
host-process policy coverage remain open under #1001.

The regex row now has partial direct-native evidence: the Cranelift spike covers
`std/regex.ax` `is_match`, `find`, and `replace_all` for the stage1-safe NFA
subset without generated Rust, including anchored replacement behavior. The
direct-native i64 path now also lowers known-input `regex_is_match(...)`
conditions, known-input `regex_replace_all(...)` string results, and known-input
`regex_find(...)` direct `Option<string>` matches into native process exit status
without generated Rust. Imported public `std/regex.ax` `is_match`, `find`, and
`replace_all` wrappers now alias that same direct-native known-input lowering.
Broader regex syntax, dynamic runtime regex execution, capture groups,
replacement expansion semantics, and conformance coverage remain open under
#1001.

The string row has partial direct-native evidence: the Cranelift spike now
builds and runs pure string intrinsics including `string_clone`,
`string_starts_with`, `string_strip_prefix`, `string_strip_suffix`,
`string_trim`, `string_trim_start`, and `string_line_at` without generated
Rust. It also builds and runs `std/string_builder.ax` owned string accumulator
helpers and `std/encoding.ax` percent encode/decode helpers, query-pair
encoding, and path segment joining without generated Rust. Known-text encoding
helpers now also feed narrow direct-native string length/comparison lowering,
known-input `string_line_at(...)` also accepts static scalar indexes, and
known-input percent decode can feed direct `Option<string>` matches without
generated Rust. Pure known-text helper calls can now fold direct-return,
local-let-return, final-if-return, match-return, and final-match-statement string
helper arguments and returns, including tuple-index and struct-field string
projections and direct map-index string projections over known map literals,
into direct-native length, comparison, and `string_starts_with(...)` conditions
without generated Rust.
Imported public `std/string_builder.ax` builder, seed, push,
line-push, and finish wrappers now alias known text facts that can feed
direct-native string comparisons, length projections, and process exit status
without generated Rust. Broader string ABI coverage, allocation behavior,
general runtime string parameters and returns, non-literal storage, and
host-boundary representation remain tracked by issue #1001.

The borrowed-slice row has partial direct-native evidence: the Cranelift spike
evaluates array-backed borrowed slices through `len`, `first`, `last`, indexing,
and function returns. The direct-native runtime path now also lowers narrow
static-range fixed-array slices using literal or static scalar bounds such as
`values[1:]`, `values[START:]`, `values[:2]`, and `values[:END]` through `len`,
`first`, and `last` for scalar and bool elements by projecting the underlying
fixed-array slots, including helper-parameter arrays feeding a direct-native
process exit status. Static-range fixed-array slices also support narrow literal
and dynamic indexing over the sliced window through the same projection slots,
including pre-runtime slice locals that alias the projected fixed-array slots.
Broader borrowed-slice aliasing, dynamic slice bounds, slice returns, and host
ABI coverage remain tracked by issue #1001.

The map lookup row has partial direct-native evidence: the Cranelift spike now
builds and runs direct map indexing, `get`, `get_or_default`,
`map_contains_key`, `map_keys`, and the public `std/collections.ax` `contains`,
`get`, `get_or_default`, and `keys` helpers for string and integer key/value
shapes without generated Rust. The direct-native i64 path now also lowers
inline-map-literal `get_or_default(...)` over scalar/string keys and
i64-compatible values into native process exit status, including default
fallback and duplicate-key replacement behavior. Inline-map-literal
`map_contains_key(...)` over scalar/string keys now also lowers into native
boolean conditions that can feed direct-native process exit status.
Inline-map-literal `get(...)` over scalar/string keys and scalar integer,
boolean, or known string values can now feed direct
`Option<int>`/`Option<bool>`/`Option<string>` match expressions into native
process exit status. Integer and boolean lookup results can also feed local
`Option<int>`/`Option<bool>` tag/payload bindings that are matched later in the
same direct-native body, and known string lookup results can feed pre-runtime
local `Option<string>` facts that are matched later in the same body.
Pre-runtime local map bindings initialized from inline map literals can feed the
same `get_or_default`, `map_contains_key`, and `get` lowering, and
`len(keys(...))`/`len(map_keys(...))` can count static map keys without
materializing a runtime key array. Static scalar integer and boolean keys can
also feed inline and pre-runtime map lookup, contains, and get-or-default
lowering. Imported public `std/collections.ax` `contains`, `get`,
`get_or_default`, and `keys` map wrappers now alias the same direct-native i64
lowering for static string/int map-local cases. Literal indexes into static
string key arrays can also feed known string length
lowering, and non-literal scalar indexes into those static string key arrays can
select among known key byte lengths. Dynamic key-array value projection locals
whose index is derived from a prior collection predicate local can also feed
equality/inequality predicates, `string_starts_with(...)` predicates, and
`string_trim(...)`/`string_trim_start(...)` length projections for
direct-native process exit status. Trimmed dynamic key-array projection locals
can also feed `string_starts_with(...)` predicates without materializing runtime
strings. Direct indexes into known map literals can also feed known string facts
for helper returns, length projections, and `string_starts_with(...)`
conditions.
Broader map ownership, runtime map storage, general payload lookup bindings,
runtime key array value projection, and host-boundary representation remain
tracked by issue #1001.

The `env.read` row now has partial Cranelift evidence for `std/env.ax`
`get_env` on present and missing environment names without generated Rust, plus
denial evidence that a package without the `env` capability fails before
backend lowering. The direct-native i64 path now also lowers literal-key
`env_get(...)` calls and the public `std/env.ax` `get_env(...)` wrapper into
native process exit status by selecting `Option<string>` match arms at compile
time for present and missing test environment names. Full runtime-time lookup,
manifest allowlist parity, runtime environment binding, and audit parity remain
open under #1001.

The FFI call row now has partial direct-native evidence: the spike builds and
runs a narrow C ABI `extern fn strlen(value: string): int from "c"` fixture
without generated Rust, using the source-level extern declaration. The
direct-native i64 path also lowers that same narrow `strlen` declaration for
supported literal and string-projection inputs into native process exit status
without generated Rust. A package with an `extern fn` declaration and no `ffi`
capability must still receive its public manifest-policy denial before any
Cranelift-specific lowering diagnostic. Broad dynamic symbol loading, pointer
and mutable-pointer ABI shapes, non-string arguments, ownership safety, platform
library resolution, and audit parity remain open under #1001.

The async runtime row now has partial Cranelift evidence for `std/async.ax`
`ready`, `await`, `spawn`, `join`, `cancel`, `is_canceled`, `timeout`,
single-slot channel `send`/`recv`, `select`, `selected`, and `selected_value`
without generated Rust. The spike now also builds and runs the
`std/async_net.ax` loopback TCP example through async `listen`, `accept`,
`recv_text`, `send_text`, `tcp_dial`, and `join` flows without generated Rust.
The direct-native i64 path now also lowers deterministic public `std/async.ax`
`ready`, `await`, `spawn`/`join` over ready tasks, and `cancel`/`is_canceled`
over known tasks into native process exit status without generated Rust. A
package importing `std/async.ax` with no `async` capability must still receive
the public manifest-policy denial before backend lowering. Full scheduler,
timer, channel/select runtime storage, blocking, wakeup, cancellation, and audit
parity remain open under #1001.

The sync-primitives row has partial direct-native evidence: the Cranelift spike
now evaluates ownership-shaped `std/sync.ax` mutex, once, and channel wrappers
and emits the expected native output. The direct-native i64 path now also lowers
public `std/sync.ax` `mutex(...)`, `lock(...)`, `replace(...)`, and
`into_inner(...)` wrappers over a scalar `int` payload into native process exit
status without generated Rust. It also lowers public `std/sync.ax`
`once_with(...)`, `once(...)`, `once_is_set(...)`, and `once_take(...)` wrappers
over scalar `int`/`bool` payloads when the one-shot cell value is compile-time
known, including pre-runtime `Once` locals, letting present and missing once
cells feed direct-native process exit status without generated Rust. It also
lowers public `std/sync.ax`
`channel(...)`, `send(...)`, and `try_recv(...)` wrappers for compile-time-known
single-slot `int`/`bool` payloads, including pre-runtime channel locals, so
present and missing channel receives can feed direct-native process exit status
without generated Rust. Concurrent execution, blocking behavior, dynamic channel
or once state after runtime scalar lowering, and host runtime synchronization
remain tracked by issue #1001.

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
local or parameter values for the `Result<(int, bool), int>` narrow slice. The
direct-native path also has narrow evidence for `Result<[int; 2], [int; 2]>`
construction, matching, helper parameters, helper returns, forwarded helper
values, and inline `Ok([..])`/`Err([..])` helper arguments represented as a tag
plus fixed-array payload slots. It also covers narrow `Result<Step, Step>`
struct payload construction, expression and statement matches, helper
parameters, helper returns, forwarded helper values, and inline
`Ok(Step { ... })`/`Err(Step { ... })` helper arguments using declared
field-order payload slots. Existing narrow `Result<Step, Step>` locals can now
be reassigned from result helper returns using the same tag/payload slots,
including inside runtime branch blocks. The nested option payload slice now also
has narrow direct-native evidence for `Result<Option<int>, int>` construction,
reassignment, matching, helper parameters, helper returns, forwarded helper
values, and inline `Ok(Some(...))`, `Ok(None)`, and `Err(...)` helper arguments.
The recursive result payload slice now also has narrow evidence for
`Result<Result<int, int>, int>` construction, reassignment, matching, helper
parameters, helper returns, forwarded helper values, and inline `Ok(Ok(...))`,
`Ok(Err(...))`, and outer `Err(...)` helper arguments.
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
struct payload variants. Existing narrow custom enum locals can now be
reassigned from enum helper returns using the same tag/payload slots, including
inside runtime branch blocks. The same representation now has narrow evidence
for positional custom enum payloads carrying nested `Option<Result<int, int>>`
and `Result<Option<int>, int>` values, including runtime-scope literal
construction, reassignment, value-producing matches, helper returns, forwarded
helper values, and inline nested variant arguments. Broader enum ABI support,
deeper nested payload shapes, and aggregate payload storage beyond the evidenced
slices remain tracked by issue #1001.

The `json.serdes` row has expanded partial direct-native evidence: the
Cranelift spike now builds and runs `std/json.ax` scalar/object helpers and
`std/serdes.ax` `Value` object-map construction, nested JSON object/array
parsing, typed field accessors, value indexing, stringify, and parse-error
reporting without generated Rust. The checked-in `stdlib_serdes` example now
also runs through the direct-native example smoke, including deep `Value`
equality and `std/testing.ax` assertion helpers without generated Rust. The
direct-native i64 path now also lowers
known-input `json_stringify_*` string results, including literal and static
scalar/bool stringify inputs. Runtime scalar/bool `json_stringify_*` calls can
also feed direct-native string length projections directly or through
`string_clone(...)` over a projection local without materializing a general
runtime string value, including string projection `let`s scoped inside runtime
branches and `len(left + right)` over supported projection inputs.
`json_stringify_string(...)` can also lower a quoted byte-length projection for
supported JSON-safe scalar/bool stringify locals, including branch-scoped
projection `let`s, by adding the enclosing quote bytes without materializing a
runtime string. `json_stringify_value(...)` over those formatted scalar and
bool JSON strings can preserve the raw formatted value for length projections
and native output passthroughs without materializing or reparsing a general
runtime JSON value. Imported public `std/json.ax`
`stringify_value(value_int(...))`, `stringify_value(value_bool(...))`, and
`stringify_value(value_string(...))` wrappers now preserve those raw formatted
scalar values for direct-native length projections and stdout/stderr output
without generated Rust. Imported public `std/json.ax` `field_int(...)`,
`field_bool(...)`, and `field_string(...)` wrappers over supported runtime
scalar/formatted-string values now lower to direct-native JSON field length
projections and stdout/stderr output without generated Rust.
Known-input `json_parse_string` and
`json_parse_value` direct `Option<string>` matches, known-input
`json_parse_field_string`/`json_parse_field_value` direct `Option<string>`
matches, and known-input `json_parse_int`, `json_parse_bool`,
`json_parse_field_int`, and `json_parse_field_bool` direct scalar option matches
into native process exit status without generated Rust. Schema validation,
dynamic runtime JSON parsing, and broader JSON value modeling remain tracked by
issue #1001. Imported public `std/json.ax` scalar parse/stringify wrappers for
`parse_int(...)`, `parse_bool(...)`, `parse_string(...)`,
`parse_field_int(...)`, `parse_field_bool(...)`, `parse_field_string(...)`,
`stringify_int(...)`, `stringify_bool(...)`, and `stringify_string(...)` now
alias those same direct-native paths in runtime-exit programs. Imported public
`std/serdes.ax` known-input `to_json(...)`, `stringify(...)`,
`from_json_str(...)`, `as_text(...)`, and `parse_error_message(...)` wrapper
paths now also feed direct-native known string comparisons, length projections,
`Result` matches, `Option` matches, and process exit status without generated
Rust for literal `Value`/object-map and literal JSON inputs. Broader dynamic
runtime JSON parsing, broad `std/serdes` `Value` storage, `JsonValue` wrapper
construction, and schema helper coverage remain tracked by issue #1001.

The owned move-state row has partial direct-native evidence: the Cranelift
spike builds and runs projection-sensitive owned field moves while preserving
access to disjoint sibling projections. Broader move-state, lifetime, and host
ABI coverage remain tracked by issue #1001.

The logging/stdio row has partial direct-native evidence: the Cranelift spike
now evaluates `std/io.ax` stderr writes and `std/log.ax` structured event
formatting plus `info_attrs` stderr emission, then emits the resulting stdout
and stderr streams from the native binary. The direct-native i64 path now also
lowers deterministic public `std/log.ax` formatting wrappers for field
construction, field-list joining, and event rendering into known string facts
that can feed comparisons, length projections, and native process exit status
without generated Rust. It also lowers known-string public `std/io.ax`
`eprintln` calls inside direct-native i64 `main` functions into native stderr
writes while preserving the newline-inclusive byte-count return value and
`generated_rust` null. Public `std/io.ax` `eprintln` calls over runtime
`std/json.ax` `stringify_int` and `stringify_bool` results also lower into
native stderr integer/boolean writes while preserving the newline-inclusive
byte-count return value without generated Rust. Known public `std/log.ax`
`info_attrs` calls now lower into native stderr writes from direct-native i64
`main` functions for known message and attributes strings, preserving the
structured JSON line and byte-count return value without generated Rust. Known
pure `print` expressions for string, integer, and boolean values now lower from
direct-native i64 `main` functions into native stdout writes without generated
Rust. Public `std/json.ax` `stringify_int` and `stringify_bool` results can
also feed `print` statements as native stdout integer/boolean writes without
generated Rust, and runtime-computed boolean `print` expressions now lower into
conditional native stdout writes without generated Rust. Runtime-computed
signed and unsigned integer `print` expressions for currently supported
direct-native scalar widths now lower through native decimal formatters and
stdout writes without generated Rust. `string_clone` over those formatted
integer and boolean strings, including through local string bindings, preserves
the same native stdout/stderr lowering. Public `std/json.ax`
`stringify_string` over those formatted integer and boolean strings, including
through local string bindings and `string_clone`, also lowers into native
JSON-quoted stdout/stderr writes while preserving `eprintln` byte counts.
Intrinsic `json_stringify_value` over those formatted scalar JSON strings
preserves the same native stdout/stderr lowering for raw JSON scalar output.
Imported public `std/json.ax` `stringify_value(value_int(...))`,
`stringify_value(value_bool(...))`, and `stringify_value(value_string(...))`
wrappers preserve the same native stdout/stderr lowering and `eprintln` byte
counts for raw JSON scalar output. Public `std/json.ax` `field_int(...)`,
`field_bool(...)`, and `field_string(...)` wrappers over supported runtime
scalar/formatted-string values also preserve native stdout/stderr lowering and
`eprintln` byte counts for JSON field output.
Stdin reads, dynamic string stdout/stderr text beyond these formatted scalar
passthroughs, broader numeric formatting beyond supported integer lines,
dynamic log inputs, and broader streaming/runtime buffering remain tracked by
issue #1001.

The `clock.now_sleep` row now has partial Cranelift evidence for `std/time.ax`
`now_ms`, `now`, `elapsed_ms`, and zero-duration `sleep`, plus guards that a
package without the `clock` capability fails before backend lowering and that
nonzero sleep fails fast instead of ever reaching host sleep during
compiler-side spike evaluation. The direct-native i64 path now also lowers
literal and static scalar `clock_sleep_ms(...)` nonpositive durations through
entrypoint and helper functions to a native process exit status without
generated Rust. Imported public `std/time.ax` `sleep(duration_ms(...))`
wrappers now alias that same deterministic path for literal and static scalar
zero-duration and negative-duration calls in runtime-exit programs. The
supported sleep shape remains limited to compile-time-known nonpositive
durations until the real runtime clock path lands. Full runtime-time
clock/sleep execution, timer scheduling, async clock integration, nonzero
sleep, and audit parity remain open under #1001.



## Rust Capture Check

This ABI describes Axiom runtime values and host-service effects. Rust may
remain the current implementation host while this contract is incomplete, but
Rust spelling and generated-Rust helper internals do not define the contract.
