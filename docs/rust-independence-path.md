# Path to Rust Independence

Status: routing document. Version 0.1, 2026-08-09.

This document answers one question: **in what order must work land for AxiOM to
stop needing Rust, and what is not on that path?**

It does not replace existing authority. [`production-language-roadmap.md`](production-language-roadmap.md)
and [`roadmap-status.md`](roadmap-status.md) remain the roadmap of record;
[`rust-exit-readiness.md`](rust-exit-readiness.md) defines the backend-exit gate;
[`self-hosting-language-gaps.md`](self-hosting-language-gaps.md) records the
measured language gaps; [`axiom-compiler-source-layout.md`](axiom-compiler-source-layout.md)
defines the package migration order. This document sequences them and states
what is on the critical path versus what is adjacent work.

## 1. The one number

Progress toward Rust independence is one ratio: **compiler source that AxiOM
owns, over total compiler source.**

| Measure | Value (2026-08-09) |
| --- | --- |
| Rust under `stage1/**/*.rs` | 624,150 lines |
| AxiOM-owned compiler source under `stage1/selfhost/` | 381 lines |
| **AxiOM-owned share** | **0.06%** |

The 381 lines are two spikes: `compiler-diagnostics-spike` (316) and
`compiler-diagnostics-distance-spike` (65). Both run through the direct-native
backend with `generated_rust: null`, which is real evidence — and both are
incomplete, for the reason in section 3.

Everything else in this document explains how that ratio moves.

## 2. Three distinct exits, often conflated

| Exit | Meaning | Gate | State |
| --- | --- | --- | --- |
| **Backend exit** | Supported user programs build without generated Rust or `rustc` | `make rust-exit-readiness`, #721 | Structurally shipped; readiness manifest currently broken (#1565) |
| **Host exit** | The compiler itself is written in AxiOM | #1468 and children | 0.06% |
| **Bootstrap exit** | The chain from source to compiler needs no Cargo | #1428, snapshot bootstrap | All rows blocked |

Backend exit is largely done. **Host exit has barely started, and it is the one
people mean by "getting off Rust."** Bootstrap exit is downstream of host exit
and cannot be attempted first.

## 3. The actual bottleneck

Host exit is not blocked by effort, planning, or decomposition. It is blocked by
a single missing ABI.

From [`self-hosting-language-gaps.md`](self-hosting-language-gaps.md), gap 9
residual:

> `&mut`/`&[T]`/`string` function parameters still do not lower (write-through
> ABI missing), so loops over caller-provided data must use by-value array
> params.

**You cannot pass a string or a slice to a function and have it run natively.**
That is why `closest_name` and `message_with_suggestion` are unfinished in
`compiler-diagnostics-spike` — the leaf-most, purest, most trivially portable
package in the entire migration order. Nothing compiler-shaped can be written
until this lands.

Second, gap 7: no runtime-sized allocation. Scratch buffers must be
caller-provided fixed-capacity literals. No lexer, symbol table, or IR builder
survives that constraint.

These are #1426 and #1425. **They are the highest-leverage issues in the
repository.**

The dependency chain is short and unforgiving:

```
#1426 (string/slice parameter ABI)
#1425 (runtime-sized collections)
        └─> compiler-diagnostics-spike completes
                └─> #1427 (compiler-scale proof: one built binary, many source inputs)
                        └─> #1468 entry gate opens
                                └─> #1473, #1471, #1469, ... (migration order)
                                        └─> #1428 (snapshot bootstrap)
                                                └─> #721 (Rust bootstrap retired)
```

## 4. Why nothing has moved

`#1468`'s entry gate is correct and explicit: no migration child may claim
completion until build purity is green, executable MIR / lifecycle / ownership
rows are runtime-complete, #1425, #1426, #1476 and #1477 are complete, and
#1427 proves a compiler-scale package. None of that holds.

So **zero PRs against #1468–#1479 in a month is the expected outcome, not
neglect.** The migration issues are correctly gated and genuinely unstartable.

Two things follow, and both are fixable today:

1. **Routing defect.** Eight gated migration children (#1469–#1475, #1478) carry
   `status:ready-for-agent` and `state:ready-for-implementation` — labels
   identical to the nine genuinely-startable prerequisites. Nothing in the label
   vocabulary distinguishes "critical path" from "gated behind work nobody has
   started." Worker dispatch cannot tell them apart.

2. **Attention defect.** Every issue on the critical path — #1425, #1426, #1436,
   #1438, #1439, #1441, #1476, #1477, #1427 — has exactly one comment, the
   planning comment from creation. Meanwhile 63 PRs merged in 30 days into
   stdlib helpers, package trust, the resolver, and CI evidence. That work is
   real, and some of it is on the path (#1525 text helpers, #1534 loop control,
   #1539/#1540 argv and cwd). But the four hard backend items went untouched,
   which is exactly why `runtime_complete` rows moved from 2 to 2.

This is selection bias toward tractable work. The periphery lands as
`syntax_only` and `static_spike` rows; only the backend items convert rows to
`runtime_complete`.

## 5. The ordered path

### Track A — critical path, strictly serial at the head

| Order | Issue | Unblocks |
| --- | --- | --- |
| A1 | **#1426** direct-native string and slice parameter ABI | everything |
| A2 | **#1425** runtime-sized collections | everything algorithmic |
| A3 | **#1436** executable MIR — first runtime-complete control-flow slice | the runtime tier |
| A4 | **#1438** runtime lifecycle ABI (allocation, ownership, drop, handles) | #1439, #1440 |
| A5 | **#1439** dynamic non-Copy aggregates across calls, returns, storage | compiler data shapes |
| A6 | **#1476** associative collections, hashing, deterministic iteration | symbol tables |
| A7 | **#1441** text v1 — UTF-8 validation, slicing, split, lines, scalars | lexer |
| A8 | **#1427** compiler-scale proof: one binary, many runtime source inputs | #1468 entry gate |

A1 and A2 are the head and are near-serial. A3–A5 are one backend workstream and
should be staffed as one. A6 and A7 can proceed in parallel with A3–A5 once A1
and A2 land.

### Track B — parallel, no dependency on Track A

| Issue | Why it is parallel |
| --- | --- |
| #1477 program host ABI v1 (argv, env, streams, cwd, exit) | Host surface, independent of value ABI; already partly landed by #1539/#1540 |
| #1455 target support v1 (Linux x86-64, macOS arm64) | Toolchain proof, no language dependency |
| #1440 ownership v1 (MIR borrow, move, drop analysis) | Can be conservative-first; a compiler can be written with always-copy semantics and tightened later |
| #1442 language control v1 (iteration protocol, `for`) | Gap 10 is severity **Low** — `while` + index is a sound workaround. Style, not blocker. |

### Track C — migration, gated on A8

Follow the order in [`axiom-compiler-source-layout.md`](axiom-compiler-source-layout.md).
Do not reorder; each step is the previous step's test corpus.

`compiler.diagnostics` (#1473) → `compiler.syntax` (#1471) →
`compiler.package_graph` (#1469) → `compiler.hir` (#1470) →
`compiler.mir` (#1472) → `compiler.stdlib` (#1478) →
backend contracts and generated-Rust retirement (#1479) →
`compiler.backend.native` (#1474) → evidence, commands, and LSP services (#1475)

Then #1428 snapshot bootstrap, then #721.

### Not on the path

Named explicitly so they are not mistaken for progress toward host exit:
SQLite (#1452), HTTP client and server (#1448, #1449), observability (#1451),
serialization (#1450), structured concurrency (#1445), network capabilities v2
(#1447), the I/O reactor (#1446), and provider ABI (#1453). These are
**production-language** work under #1432. They matter for shipping a usable
language. They do not move the 0.06%.

## 6. Performance is a gate, not a follow-up

An agent-native language puts the compiler inside the agent's inner loop. When a
model emits a change and waits on `axiomc check` before emitting the next one,
**compile latency multiplies against agent throughput.** At current model
speeds, a multi-second check turns an agent that could iterate continuously into
one that idles. This is not a polish concern; it is the difference between the
language being usable by its intended audience and not.

Two distinct budgets, both currently unmeasured as gates:

**Agent-loop latency** — `axiomc check` and warm incremental `build` on a
compiler-scale package. This is the number that governs agent throughput. There
is no gate on it today.

**Generated-code performance** — #1465 states plainly that native optimization
is "effectively opt-level 0." A self-hosted compiler compiled at opt-level 0 by
a compiler that is itself opt-level 0 compounds: stage2 build times will be the
first place this becomes intolerable, and that arrives exactly when Track C
starts.

The machinery already exists and is not wired to anything binding. `axiomc bench`
emits median, p95, variance, and allocation counts against
`stage1/schemas/axiom-benchmark-baseline-v1.schema.json`, but
`check-stage1-benchmarks.py` records
`"committed_baseline_comparison": "advisory-nonblocking"`.

Required actions:

1. Add an agent-loop latency budget to the acceptance criteria of #1436, #1425,
   and #1426 — every runtime-tier issue should state its latency cost, measured.
2. Promote #1465 (profiles, optimization, module-level incremental cache, safe
   parallelism) from adjacent work to **Track B**, scheduled to land before
   Track C begins.
3. Convert the benchmark comparison from advisory to blocking on a defined
   regression threshold, per runner class.
4. Record a stage2 build-time budget in #1427's acceptance criteria. If the
   compiler-scale proof cannot build itself in a tolerable time, the migration
   order is unaffordable regardless of correctness.

## 7. One decision to make explicitly: port or rewrite

[`axiom-compiler-source-layout.md`](axiom-compiler-source-layout.md) maps each
package to specific Rust files (`syntax.rs`, `hir.rs`, `mir.rs`,
`cranelift_backend.rs`). Read literally, that is a **translation** of 624,150
lines. At any plausible velocity, translation does not finish.

The alternative is the classic bootstrap move: write a stage2 in AxiOM that
compiles only the subset of AxiOM needed to compile itself, prove the fixpoint,
and let the Rust compiler's remaining surface be retired feature by feature
rather than line by line. That artifact is perhaps a tenth the size, and it makes
#1428's fixpoint requirement reachable.

The migration order in Track C is correct either way — it is the dependency
order of the packages. What differs is the acceptance bar for each step:
behavioural parity with the Rust implementation (port), or sufficiency to
compile the language subset (rewrite).

**This should be a recorded decision, not an implication of a layout table.**
Track C's cost estimate depends entirely on the answer.

## 8. Definition of done

Host exit is complete when all of the following hold:

- `make self-hosting-language-readiness` reports every row implemented.
- `make rust-exit-readiness` runs, passes, and has a manifest whose blockers are
  all open issues that actually exist (#1565).
- #1427 proves one built AxiOM binary handles different runtime source inputs.
- Every package in the migration order is AxiOM-owned, meaning the official
  command path executes AxiOM at runtime and the Rust implementation is not
  required for that surface.
- `make snapshot-bootstrap-readiness` reports `snapshot_output_verified`,
  `fixpoint_holds`, and `no_cargo_in_chain`.
- The AxiOM-owned share in section 1 is the compiler.

## 9. How this is measured

Today, badly. The readiness gates are the right instruments and **no CI lane
executes any of them** — only their self-tests run, and `make rust-exit-readiness`
currently fails outright on a stale blocker manifest. That is #1565, and it
should be fixed before this document's numbers are trusted for steering.

Once fixed, publish the section 1 ratio with every extended-validation run. One
number, moving or not moving, is harder to argue with than a 52-row ledger.
