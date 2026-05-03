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

This closes the local benchmark-suite foundation. Go and Rust reference
comparisons should be layered on top of this harness in CI once representative
workloads are stable enough to treat as performance policy.
