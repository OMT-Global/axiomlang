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
