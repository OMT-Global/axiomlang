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

This closes the local benchmark-suite foundation. The extended validation
benchmark gate also compares the current stage1 build medians against the
committed calibration baseline at
`stage1/benchmarks/baselines/stage1-build-median.json`. That comparison is
reported as a non-blocking warning with a documented tolerance while runner
variance is being measured; the existing benchmark gate still owns hard failures
for obvious cold-build and warm-cache regressions against the checked-in Go and
Rust reference builds.
