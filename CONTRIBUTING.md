# Contributing

Axiom is intentionally small, test-driven, and stage1-first. Keep changes tight,
specified, and backed by executable validation.

## Ground rules

- Keep the language kernel small and specified in `docs/kernel.md`.
- Use the RFC process in `docs/rfcs/` for language-level or runtime-level
  contract changes.
- Format `.ax` source with `axiomc fmt`; one canonical style keeps examples,
  docs, and fixtures reviewable.
- Add features only with:
  - a spec update when behavior changes, and
  - at least one Rust-run conformance, package, or crate test under `stage1/`.
- Treat Rust `stage1/` as the supported compiler/runtime path.
- Treat Python `stage0` as retired historical context, not a supported
  development target.

## Local setup

1. Clone the repo and enter it.
2. Enable local hooks:
   ```bash
   git config core.hooksPath .githooks
   ```
3. Confirm the Rust toolchain is available:
   ```bash
   cargo --version
   rustc --version
   ```
4. Run the default validation once before making changes:
   ```bash
   make test
   make smoke
   ```

If you use the devcontainer or managed bootstrap profiles, read
`docs/bootstrap/onboarding.md` for the repo governance and environment setup
that must stay aligned with the project.

## Where to work

- `stage1/crates/axiomc/`: parser, checker, ownership, codegen, CLI, and tests.
- `stage1/examples/`: runnable package fixtures and smoke coverage.
- `stage1/conformance/`: pass/fail corpus for executable and diagnostic checks.
- `docs/`: grammar, kernel, stage1 status, roadmap, and RFCs.

## Style expectations

For Axiom source, examples, and snippets, follow `docs/style.md`.
Use `axiomc fmt` to check or apply that canonical layout before review.

## Validation matrix

Use the smallest validation set that proves your change, and expand when the
change crosses compiler or language boundaries.

```bash
# Default docs + Rust test lane
make test

# Executable example smoke lane
make smoke

# Full Rust-owned conformance corpus
make stage1-conformance

# Rust crate tests only
make stage1-test

# Supply-chain checks, signed npm package verification (when a `package-lock.json` exists), offline lockfile verification, and SBOM emission
make supply-chain
```

## Source style

The canonical style is documented in `docs/style.md`. Use:

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- fmt stage1/examples/hello --check
```

Run without `--check` to rewrite files.

## Docs and benchmarks

Source comments that start with `///` are included by `axiomc doc`:

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- doc stage1/examples/hello
```

Benchmark entrypoints use the `*_bench.ax` suffix and run through `axiomc bench`:

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- bench stage1/examples/benchmarks --json
```

Richer package-test fixtures use naming conventions the test runner reports in
JSON: `*_table_test.ax` for table-driven cases, `*_property.ax` or
`*_property_test.ax` for bounded deterministic property-style samples, and
`*_snapshot_test.ax` / `*_golden_test.ax` with sibling `*.stdout` files for
snapshot/golden checks. Use `axiomc test --include-benchmarks` when a package's
`*_bench.ax` entrypoints should also compile and execute once as a smoke gate.

## Bootstrap discipline

Treat the repo as a staged bootstrap:

- Rust `stage1/` is the supported compiler and runtime path.
- Language behavior should be proven with Rust crate tests, `stage1/conformance`,
  and `axiomc test` package fixtures.

### Ownership and borrowing changes

If you touch ownership, borrowing, projection rules, or diagnostics:

- run `make stage1-test`;
- run `make stage1-conformance`;
- review `stage1/examples/borrowed_shapes`; and
- update or add fixtures under
  `stage1/crates/axiomc/tests/ownership_failures/` when a diagnostic contract
  changes.

The ownership-focused checked-in corpus currently lives in:

- `stage1/examples/borrowed_shapes`
- `stage1/crates/axiomc/tests/ownership_failures/`
- relevant compile-fail fixtures under `stage1/conformance/fail/`

## RFCs and G1 proposals

Open an RFC before implementation when a change affects syntax, type-system
rules, capability semantics, package/runtime contracts, or long-lived stdlib
APIs.

Use this flow:

1. Open or link a GitHub issue describing the problem, user impact, and affected
   compiler/runtime area.
2. Copy `docs/rfcs/0000-template.md` to a new numbered draft such as
   `docs/rfcs/0001-short-title.md`.
3. Mark the RFC as `Status: Draft` and link the governing issue.
4. Open a docs-only PR for the RFC plus directly supporting spec text.
5. Land implementation only after the RFC is accepted.

For roadmap-grade work such as G1 ownership/borrowing evolution, the RFC should
also call out:

- which roadmap item or stage goal it advances;
- expected parser/checker/HIR/codegen/runtime impact;
- conformance, crate tests, and example coverage to add; and
- compatibility or migration risks for existing packages.

See `docs/rfcs/README.md` for the acceptance bar and file conventions.

## Pull requests

Please use the generated PR template in `.github/PULL_REQUEST_TEMPLATE.md` and fill each section with concrete content. The required headings are:

- `## Summary`
- `## Governing Issue`
- `## Validation`
- `## Bootstrap Governance`
- `## Notes`

In particular, make sure the PR body clearly links or closes the governing issue with an accepted closing/reference form such as `Closes #262`, `Fixes #262`, `Resolves OMT-Global/axiom#262`, or a full GitHub issue URL, and records the validation you actually ran so the required `Validate PR Description` and `CI Gate` checks can pass.

Older pull requests may still pass a temporary legacy fallback when they link an issue and include a short prose summary; that fallback also accepts qualified issue references and full GitHub issue URLs, but new pull requests should use the structured template above.

Please include:

- what changed;
- why it changed;
- how it was tested; and
- any follow-up risk, limitation, or RFC linkage reviewers should know.

Keep PRs reviewable. Separate formatting churn, large refactors, and behavior
changes unless they are inseparable.
