# Performance Benchmarks

<!-- capability-ledger:v1 commands=31 stdlib_modules=34 stdlib_functions=305 capabilities=9 backend=cranelift -->

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

Transient benchmark failures can be retried without losing the signal that the
entrypoint was flaky. Pass `--retries N` to retry each warmup or sample up to
`N` times; JSON reports include the number of retries actually consumed and a
`flaky` flag for every benchmark. A run that exhausts its retries remains a
failure, so retries never weaken the baseline or exit-status gate.

## Advisory Go/Rust/Axiom comparison gate
This closes the local benchmark-suite foundation. Extended validation also runs
`make stage1-bench-gate`, which measures three representative executable
stage1 build workloads (`hello`, `stdlib_time`, and `stdlib_sync`) against
checked-in Go/Rust reference programs. The canonical workload paths live in
`stage1/benchmarks/workloads.json`, so the blocking and advisory comparison
reports cannot silently select different programs.
The blocking gate also requires each Axiom build to preserve its declared
lowering mode and requires exact exit-status and stdout parity with both native
references; build-time comparisons cannot pass on semantically different
programs.

`stdlib_time` exercises direct-native clock I/O. `stdlib_sync` exercises
ownership-shaped mutex, once-cell, and single-slot channel compilation and is
kept in the historical `concurrency` build-budget category. Its current Axiom
evidence is bounded static output; it is not scheduler, thread, blocking, or
dynamic-channel runtime proof. Broad capability and async examples remain
honestly fail-closed and are therefore not executable benchmark fixtures.

The existing benchmark gate still owns hard failures for obvious cold-build and
warm-cache regressions against the checked-in native reference builds. The newer
committed calibration-baseline comparison is deliberately non-blocking: it
compares current `axiomc build` medians to
`stage1/benchmarks/baselines/stage1-build-median.json` with a 35% tolerance and prints
`PASS`/`WARN` diagnostics, but WARN results exit successfully so CI can collect
calibration data without blocking unrelated PRs.

- cold and warm Axiom build time versus Go/Rust reference build medians
- run time medians for each produced executable
- binary size for Axiom, Go, and Rust outputs
- JSON diagnostic quality from a failing conformance fixture
- capability manifest coverage from `axiomc caps --json`
- advisory regression warnings against the committed calibration baseline
- normalized clean-vs-warm artifact equivalence, including relative paths,
  normalized metadata, byte sizes, and SHA-256 content hashes

```bash
python3 scripts/ci/check-stage1-benchmarks.py --json-out stage1/target/stage1-comparison-report.json
```

The default policy is `advisory-nonblocking`; advisory limit findings are
reported but do not fail PRs. Maintainers can opt into blocking behavior later
with `--enforce` once representative workloads and thresholds are stable.

The comparison gate also builds each workload once after removing its `dist`
directory and once with the resulting cache warm. It records the relative path,
normalized metadata size, and SHA-256 hash of every output file. Absolute paths
inside textual provenance/cache metadata are replaced with an artifact-root
marker. Timing and cache-hit counters are intentionally excluded from the
equivalence key; lowering evidence and the normalized output manifest must
still match exactly. A missing, added, or changed output is a blocking
artifact-equivalence failure.

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
samples and medians for each workload. It also includes a
`benchmark_entrypoints` section containing the execution-backed
`axiom.stage1.bench.v1` report for `stage1/examples/benchmarks`. The harness
fails if the benchmark command returns malformed output, discovers no entrypoint,
or reports a failed entrypoint. The default fixed examples are `hello`,
`stdlib_time`, and `stdlib_sync`; callers can invoke the underlying script
directly to change the round count, workload list, or output path:

```bash
python3 scripts/ci/run-stage1-bench.py --rounds 5 hello stdlib_time stdlib_sync
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
