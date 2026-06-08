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

The borrowed-slice row has partial direct-native evidence: the Cranelift spike
now evaluates array-backed borrowed slices through `len`, `first`, `last`,
indexing, and function returns. Broader borrowed-slice aliasing and host ABI
coverage remain tracked by issue #928.

## Rust Capture Check

This ABI describes Axiom runtime values and host-service effects. Rust may
remain the current implementation host while this contract is incomplete, but
Rust spelling and generated-Rust helper internals do not define the contract.
