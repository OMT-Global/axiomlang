# Stage1 Mutation Smoke

`make mutation-rust-smoke` runs a bounded mutation profile across the Rust
bootstrap compiler. The profile mutates one checked source string or mapping in
each stage1 area and runs one focused Rust test for that area:

- parser: `syntax.rs`
- HIR/type checks: `hir.rs`
- MIR lowering: `mir.rs`
- generated Rust/codegen: `codegen.rs`

The target writes `.axiom-build/reports/mutation-rust-smoke.json` with schema
`axiom.stage1.mutation-smoke.v1`. Each mutant is reported as `killed` when the
focused test fails under the mutation, or `survived` when the test still passes.
Survivors are recorded for follow-up but do not fail the make target until the
profile is promoted with `--fail-on-survivors`.

Extended toolchain qualification promotes the same four-mutant profile to a
required quality gate:

```text
python3 scripts/ci/run-mutation-rust-smoke.py \
  --fail-on-survivors \
  --per-mutant-budget-seconds 90 \
  --total-budget-seconds 300 \
  --output .axiom-build/reports/mutation-rust-smoke.json
```

The per-mutant and total budgets keep the extended lane bounded. Every focused
library test first passes against the unmodified checkout during one baseline
preflight, which also warms the shared Cargo target before any source file is
changed. Baselines count against the total profile budget but not a mutant's
own execution window. A baseline failure is blocking and cannot be counted as
a killed mutant. Cargo uses the locked workspace and a mutation is credited as
killed only when the named focused test actually reports an assertion failure;
compiler, signal, harness, or zero-test failures block as execution errors
instead. Any surviving mutant also fails qualification.

Qualification binds the command to its exact checkout SHA, requires tracked
files to match that commit before and after the run, and terminates the complete
Cargo process group when a budget expires. Interrupt handling restores the
active source mutation before producing a failed report. The qualification
runner copies the JSON report to its output directory as
`mutation-rust-smoke.json` and lists it with the mutation log and
`toolchain-qualification.json`, so the extended-validation artifact upload
preserves both the summary evidence and the detailed mutant results. This
required check runs only in the extended qualification suite; the fast
pull-request lane is unchanged.
