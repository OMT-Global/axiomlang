# Evidence Model v0

Evidence Model v0 is the first machine-readable contract for objective package
completion signals. It turns manifest test targets into semantic evidence
records that agents and PR gates can inspect without scraping terminal output.

## Command

```bash
axiomc evidence <path> --json
```

The command uses the existing stage1 test runner. It returns exit zero when it
successfully emits an evidence report, even when one or more evidence items are
`failing` or `missing`.

## Evidence Types

V0 supports:

- `unit_test`
- `property_test`
- `conformance_fixture`
- `capability_denial_test`
- `golden_output`
- `schema_validation`
- `security_fixture`
- `benchmark_baseline`
- `manual_review`
- `risk_note`

The stage1 implementation maps manifest test targets as follows:

- `unit` and `table` tests become `unit_test`
- `property` tests become `property_test`
- `snapshot` tests become `golden_output`
- `benchmark` tests become `benchmark_baseline`

## Evidence Status

Evidence status values are:

- `required`
- `provided`
- `passing`
- `failing`
- `missing`
- `waived`

When no manifest tests are discovered, the report includes a missing
`unit_test` placeholder so the absence of evidence is explicit.

## Relationship To PR Gates

This command does not replace CI. It gives agents a stable semantic report they
can attach to PR descriptions and later connect to property tests, capability
denial fixtures, artifact verification, and repository policy checks.
