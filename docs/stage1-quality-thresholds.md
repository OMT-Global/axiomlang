# Stage1 quality evidence

The extended qualification lane produces exact-head compiler coverage evidence:

```bash
make stage1-quality-gate
```

The command runs the `axiomc` library and CLI unit-test targets with
`cargo-llvm-cov 0.8.5`, locked dependencies, one test thread, and a bounded
runtime. The CLI test that recursively invokes the toolchain is excluded from
the instrumented profile because instrumentation changes that nested
invocation; the ordinary qualification suite still runs it.

Every run must freshly produce both durable artifacts:

- `.axiom-build/reports/stage1-coverage.lcov`
- `.axiom-build/reports/stage1-quality-report.json`

The LCOV paths are repository-relative. The JSON report binds the measurement
to the exact checkout head, comparison commit, target, tool version, semantic
area, source span, reproducer, and issue #1463. Qualification rejects a dirty
checkout, a head mismatch, an unavailable comparison commit, or a comparison
commit that is not an ancestor of the measured head.

For pushes to `main`, extended validation compares the pushed head with
`github.event.before`. Nightly, manual, and local runs may omit a comparison
commit; their changed-line result is then not applicable while the global floor
still applies. Full Git history is available to the extended job so supplied
ancestry claims can be verified.

## Blocking coverage floors

This slice enforces two fixed coverage floors:

- global executable-line coverage under `stage1/crates/axiomc/src` must be at
  least 60%;
- executable lines added since the comparison commit must be at least 60%
  covered.

The checker compares exact integer ratios rather than rounded percentages.
When a comparison contains no added executable lines, changed-line coverage is
reported as not applicable instead of inventing a percentage.

These values are initial cross-target coverage floors. They are not a claim
that the current coverage level can never decrease, and they are not a
target-calibrated non-increasing ratchet. Per-target evidence and
non-increasing ratchets remain follow-up work under #1463.

## Advisory complexity evidence

A separate helper can combine lexical estimates from Rust source with the same
LCOV data. Its CRAP figures are useful for locating review hotspots, but they do
not affect the qualification verdict. A Rust-syntax-aware complexity metric,
target calibration, and reviewed non-increasing function ratchets are still
required before complexity can become a blocking policy.

The extended workflow provisions `llvm-tools-preview` and pins
`cargo-llvm-cov 0.8.5`. A local environment must provide the same tool and
compatible `llvm-cov`/`llvm-profdata` binaries.

## Deterministic parser fuzz smoke

The extended qualification suite also runs a bounded parser/recovery profile:

```bash
make stage1-parser-fuzz
```

The runner mutates the versioned corpus under
`stage1/fuzz/parser-corpus` from a fixed seed, invokes the real
`axiomc parse --json` entrypoint, and allows either a successful parse or a
structured diagnostic. Timeouts, signals, and malformed JSON are failures.
Each case
records its derived seed, corpus entry, source digest, duration, and bounded
reproducer path; crash/timeout reproducers are minimized with a fixed attempt
budget. The report is `axiom.stage1.parser_fuzz.v1` and is uploaded as part of
toolchain qualification. Re-running with the same seed and corpus reproduces
the same case inputs and case IDs.

This is the first parser/recovery fuzz slice for #1463. Manifest, lockfile,
archive, JSON-RPC, HIR/MIR, codec, and network-framing fuzz profiles plus
versioned crash-corpus promotion remain follow-up work.

The dependency-free checker tests do not invoke Cargo:

```bash
make stage1-quality-gate-test
make stage1-crap-thresholds-test
```

This remains a partial slice that Refs #1463. Per-target non-increasing
coverage and complexity ratchets, broader nightly budgets, and unified
cross-area failure evidence remain.
