# Stage1 smoke example contract

`make stage1-smoke` runs the basic and standard-library example lanes through
`scripts/ci/run-stage1-smoke.py`. Their shared source of truth is
`scripts/ci/stage1-smoke-expectations.json`; the shell entry points and their
behavioral contract tests consume that same table.

Each row declares the example's build expectation and, where present, its test
expectation. Builds always follow a successful `axiomc check`. Successful builds
must also run successfully. A missing `test` field means the example's standalone
test suite is outside that lane; it does not imply that the suite passes.

The supported expectations are:

| Expectation | Required evidence |
| --- | --- |
| `direct-native` | Successful command, Cranelift backend, versioned direct-native lowering evidence, and no generated Rust. |
| `bounded-static` | Successful command and versioned bounded-static lowering evidence. This is coverage of the current static subset, not runtime execution proof. |
| `blocked` | Failing command, no advertised binary, versioned blocked-lowering evidence, and the structured diagnostic `backend.runtime_lowering_required`. |

Test reports must contain cases. Mixed suites declare the exact successful and
blocked case names in the table; an unrelated diagnostic cannot satisfy an
expected failure.

`capabilities` must continue to fail closed for both build and test while its
filesystem and cryptography operations lack runtime lowering. Its checked-in
stdout fixture is the desired future behavior, not permission to bypass the
current block. Adding that lowering requires changing the table and supplying
runtime-sensitive execution evidence.

`stdlib_env` must build and test with direct-native lowering. After building it,
the runner executes the same binary with its allowed environment variable absent,
then with two different values. Exact stdout must track each runtime input. The
runner removes that variable from the fixture's build/test environment so a
caller's shell cannot change the deterministic test expectation.

The smoke target also retains the proof-workload runner and the `caps`, `fmt`,
`doc`, and benchmark command checks. Those command-specific contracts remain in
their existing runners; they are not additional basic/stdlib table rows.

Run the checks with:

```sh
bash scripts/ci/test-run-stage1-basic-smoke.sh
bash scripts/ci/test-run-stage1-stdlib-smoke.sh
python3 scripts/ci/test-validate-stage1-smoke-report.py
make stage1-smoke
```

The fast-check lane runs the two script contract tests and the report validator
tests. These exercise actual runner dispatch using a fake compiler,
including unexpected success, wrong diagnostics, unexpected blocking, and
compile-time-captured environment output. Full smoke execution supplies the real
compiler evidence. After its existing native-linker prerequisite, the fast-check
lane also runs both real example lanes so compiler changes cannot silently leave
the shared expectations stale. Build/check/test envelopes and environment stdout evidence are
retained under `.axiom-build/reports/stage1-smoke/`; either lane accepts
`--report-dir` to select another evidence directory.
