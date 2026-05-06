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

The stage1 comparison report is intentionally non-blocking at first. It builds
and runs equivalent Axiom, Go, and Rust workloads, then emits machine-readable
JSON covering:

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
variance is being measured.
variance is being measured; the existing benchmark gate still owns hard failures
for obvious cold-build and warm-cache regressions against the checked-in Go and
Rust reference builds.
This closes the local benchmark-suite foundation. Go and Rust reference
comparisons should be layered on top of this harness in CI once representative
workloads are stable enough to treat as performance policy.
