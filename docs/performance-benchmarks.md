# Performance Benchmarks

<!-- capability-ledger:v1 commands=30 stdlib_modules=34 stdlib_functions=299 capabilities=9 backend=cranelift -->

The `axiomc bench` harness discovers `*_bench.ax` files and executes each
entrypoint for every warmup and measured iteration. It emits per-sample timing,
median, p95, sample variance, and the allocation count when a portable runtime
counter is available.

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- bench stage1/examples/benchmarks --json
```

Use the versioned baseline schema at
`stage1/schemas/axiom-benchmark-baseline-v1.schema.json` to reject median
regressions. The checked-in fixture uses
`stage1/benchmarks/baselines/axiomc-bench-v1.json`; callers choose a threshold
explicitly for their runner class.

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- bench stage1/examples/benchmarks \
  --baseline stage1/benchmarks/baselines/axiomc-bench-v1.json \
  --max-regression-percent 20 --json
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

## Cranelift evidence

The first direct-object backend slice records an advisory hello-world baseline at
`stage1/benchmarks/cranelift-hello-baseline.json`. Cranelift is now the supported
CLI backend, but benchmark availability is not production qualification. The
capability ledger keeps the backend at `direct_runtime` / `partial`, while the
runtime-ABI contract records the narrower implemented shapes.
