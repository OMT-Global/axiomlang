# Production Language Roadmap

This roadmap defines the work required for AxiOM to become a dependable
language platform for command-line tools, long-running workers, HTTP services,
and its own compiler. The governing umbrella is
[#1432](https://github.com/OMT-Global/axiomlang/issues/1432).

The target is pragmatic Rust/Go-class capability, not literal feature parity.
A production-capable AxiOM release must let a team build, test, distribute,
operate, and evolve representative software without a mandatory Rust or Go
escape hatch. Rust, Cranelift, operating-system APIs, and native libraries are
implementation adapters; they do not define AxiOM semantics.

This track complements, but does not replace:

- [self-hosting](rust-exit-readiness.md), governed by #721;
- [unattended agent coding](autonomous-agent-roadmap.md), governed by #1417;
- complete real-package Intent IR, governed by #1418.

## Critical Runtime Correction

The direct-native status surface is narrower than the command names and green
ABI summary imply. When narrow native lowering does not recognize a program,
`compile_cranelift_hello_spike` can fall back to `collect_output_program`.
That path evaluates AxiOM code in the compiler process and emits a binary that
replays collected output.

The audit reproduced this with the checked `proof_cli` workload:

1. build with `AXIOM_STAGE1_CLI_MODE=build-time`;
2. run the emitted binary with `AXIOM_STAGE1_CLI_MODE=run-time`;
3. observe that the binary still reports `build-time`.

The worker proof can also write its declared result file during compilation.
The evaluator includes environment, filesystem, process, network, HTTP,
clock, randomness, crypto, async, and other host dispatchers. This is both a
semantic bug and a security boundary violation: runtime authority must never
silently become build-time authority.

Therefore:

- `generated_rust: null` proves only that generated Rust was bypassed;
- a green static or known-input spike does not prove runtime execution;
- #1434 is the first product and self-hosting blocker;
- #1427 cannot certify a compiler-scale workload until one built binary
  processes different runtime source inputs without rebuilding.

## Evidence Tiers

Every readiness row uses one of four evidence tiers:

| Tier | Meaning |
| --- | --- |
| `syntax_only` | The compiler parses, checks, or describes the shape. |
| `static_spike` | The behavior is proven only for compiler-known values or the compile-time evaluator. |
| `runtime_complete` | A built binary handles values and effects that originate after build. |
| `production_qualified` | Runtime behavior passes supported-host, load, recovery, security, release, and operational gates. |

A lower tier cannot satisfy a higher-tier row. Capability ledgers and future
issues must use these terms instead of the unqualified word “implemented.”

The machine-readable source of status is
[`production-language-readiness.json`](production-language-readiness.json).
Run its offline validator with:

```bash
make production-language-readiness
```

The command is expected to report `ready: false` until the required rows have
real evidence. Release or final-gate work may additionally require live issue
state:

```bash
make production-language-readiness-github
```

## Current Baseline

### Strong foundations

- The front end supports numeric widths, structs, enums, tuples, arrays, maps,
  borrowed and mutable slices, generic functions and aggregates, static traits,
  closures, `Option`, `Result`, `?`, assignment, `defer`, async syntax, packages,
  capabilities, macros, and property/evidence surfaces.
- Syntax, HIR, MIR-shaped data, direct-native object emission, package/workspace
  graphs, diagnostics, JSON contracts, local publishing, inspection, LSP/DAP
  scaffolds, build caches, and provenance are present.
- The conformance corpus contains substantial passing and compile-fail coverage,
  and the compiler source decomposition ratchet is green.
- Local CLI, worker, and HTTP proof packages are useful fixtures once their
  runtime claims are strengthened.

### Capability boundaries that remain

- The supported native backend is host-only and many non-scalar paths are
  shape-specific or static-known.
- Current MIR is not yet the complete executable CFG/value/effect/lifecycle
  contract needed by all backends.
- Runtime allocation, general string/slice calls, non-Copy aggregates, and
  deterministic cleanup remain incomplete. Runtime growable associative
  collections and their hashing/iteration contract are not implemented.
- Ownership is mostly checked while lowering HIR rather than by one complete
  MIR control-flow/resource pass.
- The async, synchronization, network, HTTP, JSON, FFI, formatter, LSP, DAP,
  documentation, benchmark, package, and release surfaces are bootstrap-grade.
- No executable persistence package, public dependency resolver, installable
  release, or production workload qualification gate exists.

## Dependency-Ordered Execution

### Wave 0 — restore semantic truth

| Issue | Outcome | Dispatch rule |
| --- | --- | --- |
| #1433 | Checked roadmap, manifest, schema, and validator | This roadmap PR. |
| #1434 | Builds are effect-pure; unsupported runtime lowering fails closed | First implementation task; blocks all higher runtime claims. |
| #1435 | Generated capability ledger and documentation drift gate | May proceed in parallel with #1434. |
| #1437 | Axiom-neutral executable MIR v1 contract | Human design approval before implementation. |
| #1436 | First HIR → MIR → native runtime-complete vertical slice | After #1437 and #1434. |

### Wave 1 — native value and ownership foundation

| Issue | Outcome | Dependencies |
| --- | --- | --- |
| #1438 | Allocation, ownership, drop, and resource lifecycle ABI | MIR design; human approval. |
| #1425 | Runtime-sized sequence/vector allocation and growth | #1434, #1437, #1438. |
| #1426 | Runtime string and slice calls, returns, aliases, and cleanup | #1434, #1437, #1438, coordinated with #1425. |
| #1439 | Dynamic non-Copy aggregates across calls, returns, and storage | #1438, #1425, #1426, #1436. |
| #1440 | Dedicated MIR move, borrow, drop, and resource analysis | #1437-#1439. |
| #1476 | Runtime maps/sets, equality/hashing, deterministic iteration, and collision bounds | Lifecycle, sequences, and ownership; human collection-contract approval. |

### Wave 2 — serious CLI and worker runtime

| Issue | Outcome | Dependencies |
| --- | --- | --- |
| #1441 | UTF-8 text, slicing, split, lines, scalar iteration, conversion | Runtime strings and collections. |
| #1442 | Iteration protocol, `for`, `break`, and `continue` | Collections, MIR, ownership. |
| #1477 | Running-program argv/env/stdin/stdout/stderr/cwd/exit ABI | Build purity and runtime value/lifecycle ABIs. |
| #1443 | Paths, metadata, traversal, binary I/O, atomic and temporary resources | Build purity and lifecycle/value ABIs. |
| #1444 | Argv-safe child processes, pipes, signals, terminal, and cancellation | Program host ABI, values, structured concurrency. |
| #1445 | Real scheduler, cancellation, channels, backpressure, and synchronization | Lifecycle/value/MIR foundation; human concurrency design. |

### Wave 3 — service and persistence runtime

| Issue | Outcome | Dependencies |
| --- | --- | --- |
| #1446 | Readiness reactor with cancellation and portable adapters | Structured concurrency and value ABIs. |
| #1447 | Connect/listen-separated network authority and dynamic endpoints | Reactor and Intent IR; human security policy. |
| #1448 | Structured HTTP client with TLS, headers, bodies, limits, and cancellation | Text, values, reactor, network policy. |
| #1449 | Long-running HTTP/1.1 server, dynamic handlers, backpressure, graceful drain | Text, values, concurrency, reactor, network policy. |
| #1450 | Runtime dynamic JSON and typed codecs with limits/error paths | Dynamic aggregates and text. |
| #1453 | Safe versioned provider ABI over opaque handles and buffers | Lifecycle/value ABIs and existing target contracts; human design. |
| #1452 | Capability-scoped SQLite, prepared statements, rows, and transactions | Filesystem, provider, values, text. |
| #1451 | Logs, traces, metrics, propagation, redaction, and shutdown flushing | Runtime values, concurrency, codecs. |

### Wave 4 — productize the toolchain

| Issue | Outcome | Dependencies |
| --- | --- | --- |
| #1454 | Real extended/product qualification CI | #1430 and capability truth. |
| #1455 | Supported Linux x86-64 and macOS arm64 native target matrix | CI and runtime foundations. |
| #1481 | Runtime-origin hash/MAC/entropy/AEAD/signatures over vetted providers | Values, provider ABI, supported targets; human security policy. |
| #1457 | Editions, SemVer, public API/ABI, package, CLI, and schema compatibility | Intent IR and roadmap policy; human approval. |
| #1456 | Reproducible signed compiler releases and no-Cargo install | CI, targets, compatibility; human publication. |
| #1458 | Asymmetric publisher signatures, trust roots, rotation, and revocation | Compatibility policy. |
| #1459 | Registry resolver, content-addressed cache, lockfile v2, and vendoring | Package trust and compatibility. |
| #1460 | Syntax-aware idempotent formatter | Current syntax contract. |
| #1461 | Persistent package-aware LSP navigation and completion | Intent IR and CI qualification. |
| #1462 | Typed public API docs, search, links, and doctests | Intent IR and formatter. |
| #1463 | Fuzzing, coverage, mutation, and complexity ratchets | CI qualification. |
| #1464 | Real property cases, shrinking, and benchmark entrypoint execution | Runtime values and build purity. |
| #1465 | Build profiles, optimization, module cache, and safe parallelism | Executable MIR, targets, compatibility. |
| #1466 | Native AxiOM DWARF, real DAP execution, and profiling | Executable MIR and target support. |

### Wave 5 — qualify applications and self-host the compiler

| Issue | Outcome | Dependencies |
| --- | --- | --- |
| #1467 | Final production CLI, worker, and HTTP API load/recovery gate | Required Wave 0-4 rows. |
| #1427 | Runtime-complete compiler-scale AxiOM package and commands | Build purity, MIR, values, ownership. |
| #1468 | AxiOM compiler source-migration umbrella | #1254 decomposition and #1427 entry gate. |
| #1473 | AxiOM-owned diagnostics | #1427 entry evidence; #1468 is the open parent, not a close-before-start dependency. |
| #1471 | AxiOM-owned lexer/parser/macros/syntax | Migrated diagnostics. |
| #1469 | AxiOM-owned package graph/manifests/lockfiles/source loading | Diagnostics/syntax and package contracts. |
| #1470 | AxiOM-owned HIR/typing/capabilities/ownership | Front-end packages and ownership foundation. |
| #1472 | AxiOM-owned MIR/verification/backend planning | Migrated HIR and executable MIR. |
| #1478 | AxiOM-owned compiler stdlib catalog and intrinsic/provider bindings | Migrated MIR plus qualified runtime/stdlib contracts. |
| #1479 | AxiOM-owned backend contracts and legacy generated-Rust retirement | Migrated MIR/stdlib, targets, compatibility; human cutover. |
| #1474 | AxiOM-owned native backend/runtime | Migrated MIR/stdlib/backend contracts, target, provider, and runtime contracts. |
| #1475 | AxiOM-owned commands/evidence/doc/LSP services | All migrated compiler packages. |
| #1428 | Genesis snapshot, offline chain, no Cargo after genesis, fixpoint | Migrated compiler and release infrastructure. |
| #721 | Final host-exit decision | Exact candidate evidence; human only. |

## Immediate Agent Dispatch

The safe first queue is:

1. #1434 — effect-pure builds and fail-closed runtime lowering;
2. #1435 — canonical capability truth and documentation reconciliation;
3. #1437 — Pheidon-led MIR v1 design;
4. #1438 — Pheidon-led lifecycle ABI design;
5. #1454 — extended validation design after #1430;
6. #1460 and #1463 — bounded tooling/quality work that does not claim runtime
   readiness.

Do not dispatch #1427, #1468 children, snapshot, release publication, external
network authority, provider ABI, structured-concurrency policy, or final
production qualification past their human and dependency gates.

## Agent Execution Contract

Each implementation issue and PR must state:

- lane and autonomy class;
- exact dependencies and entry evidence;
- semantic node/schema changes;
- fixtures and positive/negative/runtime evidence;
- validation commands and expected outcomes;
- immutable promotion evidence recording exact head, target, command result,
  runtime inputs, and artifact/input digests;
- Rust-capture risk;
- agent-facing inspection impact;
- rollback/coexistence plan for compiler migration;
- whether evidence is syntax-only, static-spike, runtime-complete, or
  production-qualified.

An author agent may not approve its own PR. No agent may widen capability
authority, publish releases, change trust roots, expose external listeners, or
declare production/self-host completion without the governing human gate.

`make production-language-readiness-validate` checks ledger/schema structure,
paths, dependency cycles, and semantic invariants without pretending the open
roadmap is complete. The ordinary and GitHub readiness commands remain red
until required rows are implemented. A row's existence and command string are
not execution proof; tier promotion must land the receipt above and pass the
governing CI qualification gate.

## Deferred Beyond the First Production Release

The following are useful but are not prerequisites for the first qualified
CLI/worker/API release:

- HTTP/2, HTTP/3, WebSockets, QUIC, and broad non-HTTP protocols;
- PostgreSQL and multiple database/provider ecosystems after SQLite v1;
- full Unicode normalization and locale services beyond the accepted text v1;
- regex captures/full Unicode regex;
- dynamic traits, associated types, higher-kinded types, and advanced
  inference unless a proof workload demonstrates a hard need;
- broad unsafe FFI outside the safe provider ABI;
- WASM, Windows, mobile, and embedded targets until their target rows have
  explicit owners and executable evidence;
- public hosted registry service ergonomics after trusted resolution works.

These remain roadmap candidates, not unbounded invitations for worker agents.
