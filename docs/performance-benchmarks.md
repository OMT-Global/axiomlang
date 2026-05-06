# Performance Benchmarks

The first benchmark harness is `axiomc bench`. It discovers `*_bench.ax` files,
runs warmup iterations, runs measured iterations, and emits median and p95 wall
time statistics.

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- bench stage1/examples/benchmarks --json
```

The checked-in fixture package lives at `stage1/examples/benchmarks`.

`axiomc bench` remains the measurement path. For PR and smoke validation, the
test harness can also compile and execute benchmark entrypoints once without
collecting timing data:

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/stdlib_testing --include-benchmarks --json
```

## Advisory Go/Rust/Axiom comparison gate
This closes the local benchmark-suite foundation. Extended validation also runs
`make stage1-bench-gate`, which measures three representative stage1 build
workloads (`hello`, `capabilities`, and `stdlib_async`) against checked-in
Go/Rust reference programs.

The existing benchmark gate still owns hard failures for obvious cold-build and
warm-cache regressions against the checked-in native reference builds. The newer
committed calibration-baseline comparison is deliberately non-blocking: it
compares current `axiomc build` medians to
`stage1/benchmarks/stage1-build-baseline.json` with a 35% tolerance and prints
`PASS`/`WARN` diagnostics, but WARN results exit successfully so CI can collect
calibration data without blocking unrelated PRs.

- cold and warm Axiom build time versus Go/Rust reference build medians
- run time medians for each produced executable
- binary size for Axiom, Go, and Rust outputs
- JSON diagnostic quality from a failing conformance fixture
- capability manifest coverage from `axiomc caps --json`
- advisory regression warnings against the committed calibration baseline

```bash
python3 scripts/ci/check-stage1-benchmarks.py --json-out stage1/target/stage1-comparison-report.json
```

The default policy is `advisory-nonblocking`; advisory limit findings are
reported but do not fail PRs. Maintainers can opt into blocking behavior later
with `--enforce` once representative workloads and thresholds are stable.

The extended validation gate also compares the current stage1 build medians
against the committed calibration baseline at
`stage1/benchmarks/baselines/stage1-build-median.json`. That comparison is
reported as a non-blocking warning with a documented tolerance while runner
<<<<<<< HEAD
variance is being measured.
variance is being measured; the existing benchmark gate still owns hard failures
for obvious cold-build and warm-cache regressions against the checked-in Go and
Rust reference builds.
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
>>>>>>> origin/codex/agent-i-language-slice
>>>>>>> origin/codex/issue-395-effective-fs-roots
>>>>>>> origin/codex/worker-j-issue-362
>>>>>>> origin/codex/issue-425-crap-thresholds
The stage1 baseline harness wraps fixed checked-in examples and records parser,
checker, build, and run timings as JSON:
variance is being measured; the existing benchmark gate still owns hard failures
for obvious cold-build and warm-cache regressions against the checked-in Go and
Rust reference builds.

## Stage1 baseline harness

`make stage1-bench` records parser, check, build, and run wall-clock timings for
fixed checked-in example packages and writes a generated JSON report to
`stage1/benchmarks/generated/stage1-bench.json`. The generated path is ignored so
normal smoke/validation runs do not mutate the checked-in timing baseline.

```bash
make stage1-bench
```

By default the report is written to
`.axiom-build/reports/stage1-bench.json` using schema
`axiom.stage1.bench-harness.v1`. It is a local artifact, not a committed
baseline. Use `scripts/ci/check-stage1-benchmarks.py` for the separate
non-blocking comparison gate against Go/Rust reference workloads.
>>>>>>> origin/codex/worker-c-issue-361
This closes the local benchmark-suite foundation. Go and Rust reference
comparisons should be layered on top of this harness in CI once representative
workloads are stable enough to treat as performance policy.
=======
=======
=======
=======
Refresh the committed calibration baseline only after maintainers agree the
runner, workload set, and observed medians are stable enough to ratchet. Keep
baseline changes in review so tolerance movement is visible.
=======
The report uses schema `axiom.stage1.benchmark_harness.v1` and includes per-step
samples and medians for each workload. The default fixed examples are `hello`,
`capabilities`, and `modules`; callers can invoke the underlying script directly
to change the round count, workload list, or output path:

```bash
python3 scripts/ci/run-stage1-bench.py --rounds 5 hello modules
```

To intentionally refresh the tracked baseline at
`stage1/benchmarks/stage1-baseline.json`, use the explicit update target:

```bash
make stage1-bench-update-baseline
```

The parser timing is backed by `axiomc parse`, a parse-only command that validates
the primary package entrypoint and emits the same machine-readable stage1 JSON
contract shape as the other compiler commands.
