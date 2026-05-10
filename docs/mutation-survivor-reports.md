# Mutation Survivor Reports

`scripts/ci/render-mutation-survivor-report.py` turns a mutation smoke JSON
artifact into Markdown that can be pasted into a GitHub issue comment.

```bash
python3 scripts/ci/render-mutation-survivor-report.py \
  --input .axiom-build/reports/mutation-rust-smoke.json \
  --output .axiom-build/reports/mutation-survivors.md
```

The report groups survivors by source file and test function, keeps output
ordering stable, and recommends focused follow-up fixture names. A zero-survivor
input still produces a short successful report so automation can attach the
same artifact shape every time.
