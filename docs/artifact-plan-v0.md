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
- `kind`: output category, such as `native_binary`, `generated_rust`,
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
- generated Rust beside the binary,
- docs at `docs/axiom/index.md`,
- one test report for each manifest test target,
- one benchmark report for each manifest benchmark target.

The command does not generate files and does not replace `axiomc build`,
`axiomc test`, `axiomc doc`, or `axiomc bench`.
