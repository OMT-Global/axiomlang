# Artifact Plan v0

Artifact Plan v0 is the read-only contract for outputs a stage1 package can
produce. It lets agents inspect the output surface without running a build,
test, doc, or benchmark command.

## Command

```bash
axiomc inspect artifacts <path> --json
```

The command emits an `axiom.artifacts.v0` envelope containing artifact records
derived from `axiom.toml`.

## Artifact Records

Each artifact has:

- `id`: stable semantic identifier for the package artifact.
- `kind`: output category, such as `native_binary`,
  `legacy_generated_rust`,
  `test_report`, `benchmark_report`, or `docs`.
- `path`: package-relative output path.
- `generated_from`: source node ids that explain where the artifact came from.
- `status`: `planned` when the file is not present, or `generated` when the
  expected output path already exists.

`verified` is reserved for a later evidence-aware command that can prove the
artifact was produced by a passing build, test, doc, or benchmark run.

## Current Mapping

For a buildable package, the artifact plan includes:

- the native binary at the manifest build output directory,
- the OpenAPI target artifact at `<out_dir>/openapi.json`,
- the policy bundle target artifact at `<out_dir>/policy-bundle.json`,
- the SQL migration target artifacts at `<out_dir>/001_schema_forward.sql`,
  `<out_dir>/001_schema_rollback.sql`, and `<out_dir>/schema.snapshot.json`,
- the Terraform/OpenTofu module target artifact at `<out_dir>/main.tf`,
- the runbook target artifact at `<out_dir>/runbook.md`,
- docs at `docs/axiom/index.md`,
- one test report for each manifest test target,
- one benchmark report for each manifest benchmark target.

The plan no longer includes `generated_rust` as a supported artifact kind. If
an older output directory still contains a stale `.generated.rs` file, the
inspect command reports it as `legacy_generated_rust` so tools can see the
compatibility artifact without treating it as current build output.

`openapi_spec` records move from `planned` to `generated` after
`axiomc generate openapi <path> --out <out_dir>/openapi.json` writes the
document. `policy_bundle` records move from `planned` to `generated` after
`axiomc generate policy <path> --out <out_dir>/policy-bundle.json` writes the
allowlist. `runbook` records move from `planned` to `generated` after
`axiomc generate runbook <path> --out <out_dir>/runbook.md` writes the
operator document. `sql_migration` records move after
`axiomc generate sql <path> --out <out_dir>` writes the migration pair and
snapshot. `terraform_module` records move after
`axiomc generate terraform <path> --out <out_dir>` writes `main.tf`; packages
with no declared runtime surface intentionally keep that artifact planned and
produce no module file. The inspect command does not generate files and does
not replace `axiomc build`, `axiomc test`, `axiomc doc`, `axiomc bench`, or
target generators.
