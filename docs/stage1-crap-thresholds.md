# Stage1 CRAP threshold proposal

This is a report-only quality proposal for Rust stage1 hotspots. It does **not**
participate in PR or extended CI gates until maintainers explicitly opt in.

## Metric

CRAP combines complexity and coverage:

```text
CRAP = complexity^2 * (1 - coverage)^3 + complexity
```

The proposal script is dependency-free so it can run in the existing shell-only
CI shape. It estimates function complexity from Rust control-flow tokens and can
fold in line coverage from an optional LCOV report. When no coverage report is
available, functions are ranked as if uncovered so the output is conservative,
but still report-only.

## Proposed thresholds

Use these bands for the future stage1 hotspot gate:

| Band | CRAP score | Meaning |
| --- | ---: | --- |
| watch | `>= 30` | Needs review before growing or touching the function. |
| warn | `>= 60` | Should be split, simplified, or covered before expansion. |
| critical | `>= 100` | Should block new/changed hotspots once CI enforcement is enabled. |

Initial enforcement should be a ratchet: report every hotspot, but fail only new
or changed functions over the accepted threshold. A full-baseline cleanup gate
should be a separate explicit decision because current stage1 has large compiler
hotspots that predate this proposal.

## Current generated proposal

Run:

```bash
make stage1-crap-proposal
```

This writes `stage1/quality/crap-threshold-proposal.json` with:

- report-only status and `ciBlocking: false`
- the proposed threshold bands above
- observed bootstrap thresholds from the current hotspot distribution
- the top stage1 Rust hotspots for review

Optional LCOV input can be supplied directly:

```bash
python3 scripts/ci/propose-stage1-crap-thresholds.py \
  --lcov path/to/lcov.info \
  --output stage1/quality/crap-threshold-proposal.json
```

Do not wire `--enforce` into CI until the proposal is explicitly accepted.
