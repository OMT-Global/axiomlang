# Path to Rust Independence

Status: routing document. Version 0.2, 2026-09-07. Refs #1566.

This document sequences the work needed for AxiOM to stop requiring Rust.
[`production-language-roadmap.md`](production-language-roadmap.md) and
[`roadmap-status.md`](roadmap-status.md) remain the roadmap of record;
[`rust-exit-readiness.md`](rust-exit-readiness.md) defines the backend-exit gate;
[`self-hosting-language-gaps.md`](self-hosting-language-gaps.md) records measured
language gaps; [`axiom-compiler-source-layout.md`](axiom-compiler-source-layout.md)
defines package migration order. Live issue prerequisites control dispatch.

## 1. Source inventory and actual ownership

The original #1566 audit measured the source tree on 2026-08-09. Its counts are
reproducible at this document's introducing commit, `fa43076c518b93a4c533dac89cd645b87ccf59cb`.
The refreshed baseline is main commit `27f551804a2ca3fcff6d0ca81e701c83fd5a536b`.

| Inventory | Original audit | Refreshed baseline |
| --- | ---: | ---: |
| All tracked Rust files under `stage1/` | 178,341 lines | 181,636 lines |
| Rust in the two compiler crate source trees | 135,963 lines | 139,084 lines |
| Tracked `.ax` files under `stage1/selfhost/` | 381 lines | 381 lines |
| Spike share of all tracked Rust plus spike source | 0.21% | 0.21% |
| Spike share of compiler source-tree Rust plus spike source | 0.28% | 0.27% |

The compiler source-tree inventory includes `stage1/crates/axiomc/src/` and
`stage1/crates/axiomc-backend-cranelift/src/`. It excludes separate integration
test trees, but includes inline tests, comments, and blank lines. The all-Rust
inventory includes separate tests as well. Both count newline bytes, like
`wc -l`, in Git blobs; ignored build output, dependencies, and local untracked
files cannot inflate the denominator. A share is `100 * ax / (rust + ax)`.
The earlier 624,150-line / 0.06% claim was incorrect.

Reproduce either column from any checkout containing its commit:

```sh
python3 - fa43076c518b93a4c533dac89cd645b87ccf59cb <<'PY'
import subprocess
import sys

ref = sys.argv[1]  # use 27f551804a2ca3fcff6d0ca81e701c83fd5a536b for the refresh
paths = subprocess.check_output(
    ["git", "ls-tree", "-r", "--name-only", ref, "stage1/"], text=True
).splitlines()

def lines(selected):
    return sum(subprocess.check_output(["git", "show", f"{ref}:{p}"]).count(b"\n")
               for p in selected)

rust = lines(p for p in paths if p.endswith(".rs"))
compiler = lines(p for p in paths if p.endswith(".rs") and p.startswith((
    "stage1/crates/axiomc/src/", "stage1/crates/axiomc-backend-cranelift/src/")))
ax = lines(p for p in paths if p.startswith("stage1/selfhost/") and p.endswith(".ax"))
print(f"tracked Rust: {rust}; compiler source-tree Rust: {compiler}; spike AxiOM: {ax}")
print(f"all-source spike share: {100 * ax / (rust + ax):.2f}%")
print(f"compiler-tree spike share: {100 * ax / (compiler + ax):.2f}%")
PY
```

These are **source inventory ratios, not a host-exit completion percentage**.
The 381 lines belong to `compiler-diagnostics-spike` (316) and
`compiler-diagnostics-distance-spike` (65). Running a spike through the
direct-native backend with `generated_rust: null` proves that bounded path;
it does not prove that official compiler commands dispatch through AxiOM.
Actual ownership requires dispatch/provenance evidence that the official
`check/build/run/test/doc/lsp` paths no longer require the Rust implementation.
The source inventory is supporting evidence for that gate.

## 2. Three distinct exits

| Exit | Meaning | Gate |
| --- | --- | --- |
| **Backend exit** | Supported user programs build without generated Rust or `rustc` | `make rust-exit-readiness`, #721 |
| **Host exit** | Official compiler paths execute an AxiOM compiler instead of the Rust implementation | #1468 and children |
| **Bootstrap exit** | The source-to-compiler chain needs no Cargo | #1428, snapshot bootstrap |

Backend execution support does not establish host or bootstrap exit. The
2026-08-09 audit found three implemented, two partial, and ten blocked
self-hosting readiness rows; its snapshot-bootstrap rows were blocked. Rerun
the gates for current state rather than treating those audit counts as live.

## 3. The executable foundation before the string/slice ABI

The missing write-through string/slice ABI blocks `closest_name` and
`message_with_suggestion` in `compiler-diagnostics-spike`. Runtime-sized
allocation also blocks practical lexer, symbol-table, and IR-builder work.
These are high-priority language gaps, but the string/slice leaf is not the
first dispatchable task.

As corrected in #1566, #1426 depends on #1425, #1436, and #1438; #1425 also
depends on #1438. Dispatching `#1426 -> #1425` would invert the prerequisites.
The dependency-safe opening is:

```text
#1436 executable MIR/native foundation   #1438 lifecycle/ownership foundation
                 \                         /
                  +------> #1425 runtime-sized collections
                                  |
                                  v
                           #1426 string/slice ABI
                                  |
                                  v
                remaining runtime prerequisites -> #1427 compiler-scale proof
                                  |
                                  v
                       #1468 migration entry gate
                                  |
                                  v
                    compiler package migration -> #1428 bootstrap
```

Check each live issue before dispatch. Where broad runtime contracts depend on
one another, factor the smallest independently closeable foundation slice;
do not dispatch an implementation against a dependency cycle.

## 4. Routing work toward executable evidence

#1468's entry gate requires build purity, executable MIR/lifecycle/ownership
at the required runtime tier, #1425, #1426, #1476, #1477, and #1427's
compiler-scale proof. A contract, schema, or static spike is insufficient to
close a runtime prerequisite.

The original #1566 audit recorded 63 merged PRs over roughly 30 days while
`runtime_complete` remained at two rows. It also found gated migration leaves
sharing ready labels with executable prerequisites. Those are historical audit
findings, not current PR or label counts. The operational response is to check
live dependencies, distinguish a gated child from a ready foundation slice,
and give every executable blocker an owner and observable acceptance evidence.

## 5. Ordered implementation and migration

### Track A — executable foundation

The waves below follow #1566's corrected routing. Issue contracts take
precedence if their dependencies change.

| Wave | Work | Acceptance needed before proceeding |
| --- | --- | --- |
| A1 | #1436 executable MIR/native foundation and #1438 lifecycle foundation | Runtime behavior for the independently scoped foundation slices |
| A2 | #1425 runtime-sized collections | A1 prerequisites and bounded allocation behavior |
| A3 | #1426 string/slice parameter ABI | #1425, #1436, and #1438 prerequisites |
| A4 | #1439 dynamic aggregates, #1441 text, #1477 program host ABI | Each issue's own prerequisites; parallel only where independent |
| A5 | #1440 ownership, then #1476 associative collections | Ownership and collection prerequisites with runtime evidence |
| A6 | #1427 compiler-scale proof | One built binary handling multiple runtime source inputs |
| A7 | #1468 entry gate, then #1473 diagnostics migration | All migration-entry predicates, not just A6 |

### Parallel work

#1455 target support and #1465 profiles/optimization/cache work can advance
alongside the foundation where their own contracts permit. #1442 iteration
control has a bounded `while` plus index workaround for early compiler slices.
Neither parallel status nor a workaround waives the final issue acceptance
criteria. #1477 belongs in the dependency-checked runtime wave above, rather
than being assumed independent of all value-ABI work.

### Track C — compiler package migration

Follow [`axiom-compiler-source-layout.md`](axiom-compiler-source-layout.md),
subject to #1468's entry gate:

`compiler.diagnostics` (#1473) -> `compiler.syntax` (#1471) ->
`compiler.package_graph` (#1469) -> `compiler.hir` (#1470) ->
`compiler.mir` (#1472) -> `compiler.stdlib` (#1478) ->
backend contracts and generated-Rust retirement (#1479) ->
`compiler.backend.native` (#1474) -> evidence, commands, and LSP services (#1475).

Then prove #1428 snapshot bootstrap and the final #721 exit contract.

### Production-language work and the final gate

SQLite, HTTP, observability, serialization, structured concurrency, networking,
the I/O reactor, and provider ABI support the production-language roadmap.
They do not by themselves transfer compiler ownership to AxiOM. However, #1566
records a remaining scope decision: #721's Rust-exit gate includes capability
rows such as networking, async, signatures, and AEAD. The maintainer must either
retain them as final host-exit blockers or explicitly decouple them. This
routing document does not silently waive capabilities required by that gate.

## 6. Performance acceptance

Compile latency belongs in an agent's edit/check loop. Runtime-tier issues
should report cold and warm `axiomc check`/`build` latency on representative
compiler-scale inputs, alongside runtime correctness. #1465's optimization
and incremental-cache work should progress before migration becomes the
compiler's main execution path.

The existing `axiomc bench` evidence and
`stage1/schemas/axiom-benchmark-baseline-v1.schema.json` provide measurement
structure. Turning an advisory comparison into a blocking threshold requires
an explicit budget per runner class and a stage2 build-time budget in #1427.
This document proposes that policy; it does not claim those budgets are enforced.

## 7. Record the port-versus-bootstrap decision

The source layout maps packages to Rust implementation files. The original
inventory contains 135,963 lines in the two compiler source trees, or 178,341
tracked Rust lines when separate tests and other stage1 Rust are included.
Neither count is an estimate of how much code must be translated.

One approach ports behavior toward parity with the Rust implementation. Another
first writes a stage2 that compiles the subset of AxiOM needed to compile itself,
proves the fixpoint, and expands or retires remaining surfaces explicitly.
The dependency order applies to either approach, but the acceptance bar and
cost differ. Record that decision before assigning package-sized migrations;
a source-layout table does not resolve it.

## 8. Definition of done

Host and bootstrap exit require executable evidence:

- The required self-hosting and Rust-exit readiness rows pass.
- #1427 proves one built AxiOM binary handles different runtime source inputs.
- Official compiler command paths execute AxiOM at runtime and no longer need
  the corresponding Rust implementation, verified through dispatch/provenance.
- #1428 proves verified snapshot output, a fixpoint, and no Cargo in the chain.
- The final #721 capability scope is explicit and all retained blockers pass.

Publish source inventories beside these results, with pinned revisions and
stable path definitions. Source growth or a passing spike alone cannot close
an ownership gate.

## 9. Measurement in CI

The 2026-08-09 audit found readiness gates unexecuted by CI and a stale Rust-exit
blocker manifest (#1565). Main has since received readiness repairs through
#1569. Validate current workflow execution and gate output; do not reuse the
old failure as a current status assertion.

Run `make self-hosting-language-readiness`, `make rust-exit-readiness`, and
`make snapshot-bootstrap-readiness` for their respective evidence. Report
failed readiness separately from a checker regression. Extended validation
should publish both the pinned source inventory and official-path ownership
evidence so dispatch decisions follow executable progress.
