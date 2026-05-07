# Stage1 Quality Thresholds

Stage1 CRAP threshold calibration is intentionally proposal-only. Run:

```bash
make stage1-crap-thresholds
```

The command scans Rust-owned stage1 sources, estimates function complexity, and
emits `axiom.stage1.crap-threshold-proposal.v1` JSON with a warning threshold,
hotspot list, and proposed blocking policy. Coverage defaults to zero until
coverage artifacts are wired into extended validation, so the output should be
used to calibrate hotspots rather than fail CI.

Blocking remains opt-in through
`scripts/ci/propose-stage1-crap-thresholds.py --enforce` after coverage inputs
and runner baselines are stable.
