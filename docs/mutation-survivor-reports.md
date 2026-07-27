# Mutation Survivor Reports

`scripts/ci/render-mutation-survivor-report.py` turns a mutation smoke JSON
artifact into Markdown that can be pasted into a GitHub issue comment.

```bash
python3 scripts/ci/render-mutation-survivor-report.py \
  --input .axiom-build/reports/mutation-rust-smoke.json \
  --output .axiom-build/reports/mutation-survivors.md
```

The report shows the overall qualification status, blocking count, fatal error,
and each nonzero blocking outcome count before grouping survivors by source
file and test function. Survivor ordering remains stable, and each survivor
gets a focused follow-up fixture recommendation. When the source report carries
an exact-head reproducer, the rendered survivor or blocking detail keeps that
command visible for the next operator.

A zero-survivor report is only described as needing no follow-up when it has no
blocking signal. Reports with a `failed` overall status, a fatal error, or
`baseline_failure`, `timeout`, `budget_exhausted`, anchor, execution, or raw
`failed` outcomes remain visibly blocked even when they contain no survivor
fixtures. Legacy reports without the newer status and summary fields remain
readable; blocking counts are inferred from their mutant entries when possible.
