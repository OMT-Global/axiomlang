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

Refresh the committed calibration baseline only after maintainers agree the
runner, workload set, and observed medians are stable enough to ratchet. Keep
baseline changes in review so tolerance movement is visible.
