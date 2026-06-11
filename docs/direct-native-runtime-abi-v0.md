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

## Contract Shape

The v0 contract has two groups:

- `value_features`: runtime representations the direct native backend must
  carry across function calls, aggregate projections, returns, pattern matches,
  and owned/borrowed access.
- `capability_shims`: backend-neutral runtime entrypoints for host services
  that are currently implemented through the generated-Rust runtime path.

Every row has one of these statuses:

- `implemented`: direct native builds support the row for the supported stage1
  surface.
- `partial`: direct native builds support a narrower subset and the row names
  the remaining issue.
- `blocked`: direct native builds do not yet support the row broadly enough for
  Rust exit.

Rows that are not `implemented` must name at least one blocker issue.
Compiler-side Cranelift spike evaluation can be recorded as evidence on a
blocked runtime-shim row, but it does not by itself reclassify that row as
runtime support.

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

The checked-in contract is intentionally not ready. It records the current
Cranelift/direct-native spike as partial and points the blocked runtime rows at
the Rust-exit implementation issues. This lets future backend slices update the
contract as runtime shims land without editing the generated-Rust target
contract.

The first executable guard for this boundary is a Cranelift regression that
builds a package using `std/fs.ax` without the `fs` capability and verifies the
public capability denial appears before any Cranelift unsupported-feature
diagnostic.

<<<<<<< HEAD
The `fs.read` and `env.read` rows now have partial Cranelift evidence for
`std/fs.ax` `read_file` and `std/env.ax` `get_env` on present and missing
filesystem or environment names, plus denial evidence that packages without the
matching capability fail before backend lowering. Full runtime-time lookup,
manifest allowlist parity, and audit parity remain open under #928.

The UDP row is still blocked for positive direct-native runtime execution, but
now has denial evidence: a package that calls `std/net.ax`
`udp_bind_loopback_once(...)` without the `net` capability must receive the
public manifest-policy denial before any backend-specific lowering diagnostic.

The TCP row is still blocked for positive direct-native runtime execution, but
now has denial evidence: a package that calls `std/net.ax`
`tcp_listen_loopback_once(...)` without the `net` capability must receive the
public manifest-policy denial before any backend-specific lowering diagnostic.

The filesystem write row remains blocked for direct-native runtime support, but
now has positive compiler-side spike evidence: the Cranelift spike evaluates
`std/fs.ax` write helpers over configured `fs_root`-scoped literal paths during
compilation and emits the resulting output, covering `mkdir_all`, `write_file`,
`append_file`, readback, `replace_file`, `create_file`, `remove_file`, and
`remove_dir`. A package with `fs = true` and `"fs:write" = false` that calls
`std/fs.ax` `write_file(...)` must still receive the public manifest-policy
denial before any backend-specific lowering diagnostic. Full runtime-time
filesystem writes, atomic replace parity, TOCTOU hardening, and audit parity
remain open under #928.

The DNS resolve row is still blocked for positive direct-native runtime
execution, but now has denial evidence: a package that calls `std/net.ax`
`resolve(...)` without the `net` capability must receive the public
manifest-policy denial before any backend-specific lowering diagnostic.

The direct-native crypto hash slice is still marked partial: the Cranelift
spike can build and run `std/crypto_hash.ax` `sha256(...)` without generated
Rust, and crypto capability denials still happen before backend lowering.
Random, signature, AEAD, and broader crypto audit parity remain blocked.

The direct-native crypto MAC slice is now marked partial: the Cranelift spike
can build and run `std/crypto_mac.ax` HMAC-SHA256, HMAC-SHA512, verification
helpers, string constant-time equality, and byte-slice constant-time equality
without generated Rust. A package without the `crypto` capability fails before
backend lowering. Runtime audit parity and broader crypto host-service coverage
remain blocked under #928.

The HTTP client row remains blocked for positive direct-native runtime support:
the Cranelift spike does not yet lower `std/http.ax` `get(...)` into a native
host-service entrypoint. The current evidence proves only that denied `net`
capability use fails through the manifest policy before Cranelift lowering or
native execution.

The process status row remains blocked for direct-native runtime support, but
now has positive compiler-side spike evidence: the Cranelift spike evaluates
`std/process.ax` `run_status(...)` for literal, allowlisted deterministic
commands during compilation and emits their exit statuses without generated
Rust. Denied `process` capability use still fails through the manifest policy
before Cranelift lowering or native execution. Full runtime-time process
execution, argument handling, audit parity, and host-process policy coverage
remain open under #928.

The borrowed-slice row has partial direct-native evidence: the Cranelift spike
now evaluates array-backed borrowed slices through `len`, `first`, `last`,
indexing, and function returns. Broader borrowed-slice aliasing and host ABI
coverage remain tracked by issue #928.

The `env.read` row now has partial Cranelift evidence for `std/env.ax`
`get_env` on present and missing environment names, plus denial evidence that a
package without the `env` capability fails before backend lowering. Full
=======
The `env.read` row now has compiler-side Cranelift spike evidence for
`std/env.ax` `get_env` on present and missing environment names, plus denial
evidence that a package without the `env` capability fails before backend
lowering. This does not claim direct native runtime execution yet; full
>>>>>>> 96bafc73 (Clarify direct-native runtime evidence)
runtime-time lookup, manifest allowlist parity, and audit parity remain open
under #928.

The sync-primitives row has partial direct-native evidence: the Cranelift spike
now evaluates ownership-shaped `std/sync.ax` mutex, once, and channel wrappers
and emits the expected native output. Concurrent execution, blocking behavior,
and host runtime synchronization remain tracked by issue #928.

The `Result<T, E>` row has partial direct-native evidence: the Cranelift spike
now builds and runs a package importing `std/outcome.ax`, using result
predicates, fallback unwrap helpers, direct match arms over `Result<T, E>`
values, scalar payloads, string errors, and struct payloads. Broader runtime
ABI and capability-shim coverage remain tracked by issue #928.

The owned move-state row has partial direct-native evidence: the Cranelift
spike builds and runs projection-sensitive owned field moves while preserving
access to disjoint sibling projections. Broader move-state, lifetime, and host
ABI coverage remain tracked by issue #928.

The logging/stdio row has partial direct-native evidence: the Cranelift spike
now evaluates `std/io.ax` stderr writes and emits the resulting stdout and
stderr streams from the native binary. Stdin reads, `std/log.ax` wrappers, and
broader streaming/runtime buffering remain tracked by issue #928.

The `clock.now_sleep` row now has partial Cranelift evidence for `std/time.ax`
`now_ms`, `now`, `elapsed_ms`, and zero-duration `sleep`, plus guards that a
package without the `clock` capability fails before backend lowering and that
nonzero sleep fails fast instead of ever reaching host sleep during
compiler-side spike evaluation. The spike intentionally keeps the supported
sleep shape limited to zero-duration calls until the real runtime clock path
lands. Full runtime-time clock/sleep execution, timer scheduling, async clock
integration, and audit parity remain open under #928.

The sync-primitives row has partial direct-native evidence: the Cranelift spike
now evaluates ownership-shaped `std/sync.ax` mutex, once, and channel wrappers
and emits the expected native output. Concurrent execution, blocking behavior,
and host runtime synchronization remain tracked by issue #928.

The direct-native JSON/serdes slice is still marked partial: the Cranelift
spike can build and run `std/json.ax` scalar parse/stringify helpers, first-class
`JsonValue` string wrapping, object field extraction, and value normalization
without generated Rust. Full `std/serdes.ax` object graph parsing, schema
validation, and richer JSON value modeling remain blocked.

The regex row is partial: direct native builds cover `std/regex.ax` matching,
finding, and replacement for the stage1-safe NFA subset. The replacement
coverage includes an anchored `replace_all` regression so `^` patterns only
replace the original leading match. Full regex syntax and broader conformance
remain tracked by #928.

The `clock.now_sleep` row now has partial Cranelift evidence for `std/time.ax`
`now_ms`, `now`, `elapsed_ms`, and zero-duration `sleep`, plus guards that a
package without the `clock` capability fails before backend lowering and that
nonzero sleep fails fast instead of ever reaching host sleep during
compiler-side spike evaluation. The spike intentionally keeps the supported
sleep shape limited to zero-duration calls until the real runtime clock path
lands. Full runtime-time clock/sleep execution, timer scheduling, async clock
integration, and audit parity remain open under #928.

The direct-native JSON/serdes slice is still marked partial: the Cranelift
spike can build and run `std/json.ax` scalar parse/stringify helpers, first-class
`JsonValue` string wrapping, object field extraction, and value normalization
without generated Rust. Full `std/serdes.ax` object graph parsing, schema
validation, and richer JSON value modeling remain blocked.


## Rust Capture Check

This ABI describes Axiom runtime values and host-service effects. Rust may
remain the current implementation host while this contract is incomplete, but
Rust spelling and generated-Rust helper internals do not define the contract.
