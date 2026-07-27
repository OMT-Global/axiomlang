# Stage1 CRAP metrics

CRAP combines cyclomatic complexity and executable-line coverage:

```text
CRAP = complexity^2 * (1 - coverage)^3 + complexity
```

`scripts/ci/propose-stage1-crap-thresholds.py` is an advisory inspection tool.
It uses a dependency-free lexical scan to approximate Rust function boundaries
and control-flow complexity, reads optional LCOV data, and reports measured
hotspots. It is not a Rust parser, so raw strings, generated source text,
conditional compilation, and target-specific code can affect its estimates.

Missing executable records remain explicitly unmeasured; they are never
converted into fabricated zero coverage.

```bash
python3 scripts/ci/propose-stage1-crap-thresholds.py \
  --lcov .axiom-build/reports/stage1-coverage.lcov
```

The historical `stage1-crap-proposal` target is useful for exploration, but its
threshold is not qualification policy. CRAP estimates do not affect the
exact-head quality verdict. The only blocking thresholds in this slice are the
fixed 60% global and changed executable-line coverage floors described in
`docs/stage1-quality-thresholds.md`.

Before CRAP can become blocking, #1463 still requires a Rust-syntax-aware
metric, target-specific calibration, stable function identities, and reviewed
non-increasing ratchets. This advisory slice Refs #1463; it does not close it.
