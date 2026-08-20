# Compatibility v1

Compatibility v1 makes AxiOM's public policy and source-derived contract
inventory explicit and machine-comparable. It does not claim that a historical
compiler release exists: the repository currently describes compiler `0.1.0`,
has no release tags, and has no immutable previous-compiler qualification.

## Policy

`stage1/compatibility/policy-v1.json` is the checked policy instance. It is
validated by `axiom-compatibility-policy-v1.schema.json` and records:

- the current compiler version and support range (`0.1.0` exactly), plus the
  honest absence of a qualified previous compiler;
- pre-1.0 SemVer rules for stable, experimental, and deprecated surfaces;
- at least 180 days and two supported editions before removal, with removal
  deferred to a major contract version and requiring a semantic replacement
  and migration action;
- separate compatibility directions for schema producers and consumers;
- stable JSON CLI output versus explicitly non-byte-stable human text;
- package, lockfile, artifact, language, stdlib, CLI, ABI, and schema evolution
  rules; and
- a ban on exposing Rust physical layout, crate paths, enum discriminants,
  pointer width, alignment, or Serde encodings as logical AxiOM ABI contracts.

The edition lifecycle is the closed ordered set `experimental`, `supported`,
`deprecated`, and `removed`. The support matrix is also closed: it contains
exactly the current compiler row and the honest unavailable-previous-compiler
row. Canonical comparison uses the separately frozen
`accepted-baseline/policy.json` as the old policy and reports both old and new
policy versions. Policy semantic drift must increase both the policy version
and the public contract version.

Edition `2026` is experimental and policy-only. A manifest edition selector is
not implemented. The manifest parser rejects unknown root and nested keys,
including a future `edition` field, so an unsupported edition cannot be silently
accepted. `axiomc migrate` is a plan generator, not an edition selector.

## Source-derived public contract

`scripts/ci/extract-public-contract-v1.py` derives the current contract from
the governed sources listed in `source-inventory-v1.json`. Each surface carries
sorted source paths, roles, selectors, and SHA-256 provenance. The required
surface kinds are `language`, `stdlib`, `cli`, `package`, `abi`, `schema`, and
`artifact`; compiler identity and its exact current/minimum/maximum range live
at the contract's top level rather than as a duplicate surface.

The extraction covers:

- governed diagnostic-syntax and HIR-ownership snapshots for a deliberately
  marked partial language signature;
- every stdlib module, symbol, signature, effect, binding, and binding kind;
- nested CLI command paths from the governed CLI inventory, including package
  subcommands; command-graph coverage is governed while flag coverage remains
  explicitly experimental and partial;
- the complete manifest schema plus the mechanically checked parser contract
  for exact test kinds, fail-closed per-test capabilities, and canonical
  dependency versions;
- the canonical lockfile fixture and all published JSON schemas;
- a governed target-neutral ABI meaning contract whose IDs and capabilities
  must exactly match the runtime-readiness inventory; and
- the target-neutral artifact envelope.

The ABI semantic digest contains only logical AxiOM value and capability
meaning. Readiness status, evidence, blockers, notes, target names, and
implementation vocabulary are provenance and parity inputs, not ABI meaning,
so their ordering or wording cannot create false semantic drift.

Rust implementation names are never public IDs. Rust-capture checks inspect
surface signatures, logical ABI meanings, and migration actions and reject
Rust, rustc, Cargo, Cranelift, generated-Rust, Serde, crate, `repr`, `Vec`,
native-layout, discriminant, pointer-width, alignment, and crate-path
vocabulary. AxiOM language concepts such as `Option` and `Result` remain valid.

The current corpus is under `stage1/compatibility/fixtures/current/` and
includes metadata, manifest, lockfile, source, schema, artifact, and generated
contract. `accepted-baseline/` is the frozen, complete source-contract ratchet
used by canonical CI, including its historical policy snapshot; it is not
release history or a previous compiler.
`previous-contract-fixture/` remains sparse synthetic checker input and is not
used as the canonical ratchet.

The current source contract is version `0.5.0` with 68 surfaces. Its changes
from the byte-frozen 52-surface `0.1.0` accepted baseline include the five
Package Trust v1 schemas, six additive base-contract schemas (Provider ABI,
runtime observability, Semantic MIR, runtime lifecycle, target support, and persistent LSP), two quality
schemas (quality policy and quality report), and three package-resolver schemas.
The existing CLI, manifest, lockfile, `axiom.toml` schema, and stage1
JSON-envelope schema surfaces also carry their governed package-resolver
changes. Per-surface versions remain `0.1.0` for unchanged surfaces and are
`0.2.0` for the additive schema surfaces. The breaking stdlib authority
transition publishes catalog version `2.0.0` and stdlib catalog schema surface
version `0.3.0`; consumers must adopt AxiOM-owned per-symbol effects before
reading the refreshed catalog. The CLI surface is version `0.3.0`.

Existing command invocations require no changes. Operators adopting registry
dependencies run `axiomc pkg fetch` to create the v2 lockfile and verified
cache, use `axiomc pkg update` for explicit re-resolution, and run
`axiomc pkg vendor` before cache-independent locked offline builds. Standalone
Package Trust verification remains available through `axiomc pkg verify` with
the exact artifact and trust metadata paths plus `--json`.

Verify source derivation and the corpus with:

```bash
python3 scripts/ci/extract-public-contract-v1.py --check
python3 scripts/ci/check-compatibility-corpus-v1.py --json
```

## Compatibility reports

Run the checker against the corpus contracts:

```bash
python3 scripts/ci/check-compatibility-v1.py \
  --old stage1/compatibility/fixtures/accepted-baseline/contract.json \
  --old-policy stage1/compatibility/fixtures/accepted-baseline/policy.json \
  --new stage1/compatibility/fixtures/current/contract.json \
  --json
```

The success report records exact old and new policy and contract versions plus
exact old and new compiler current/minimum/maximum versions. Its changes are
deterministically ordered. Signature, kind, version, stability, policy,
edition, or support-range drift is classified as breaking, deprecated,
additive, or compatible according to the applicable historical and current
policies.

Semantic drift must increment the contract version. Stable post-1.0 breaking
drift requires a major version; pre-1.0 breaking drift requires at least a
minor version. A deprecated surface freezes its signature and carries a
replacement and action. A removal is legal only after prior deprecation and
must record an action, replacement, `removed_in` equal to the new contract
version, and `removed_on` no earlier than `remove_after`. Migration metadata is
a closed graph: every deprecated replacement exists in the same contract,
every removed surface has exactly one migration entry, no fabricated entries
are allowed, and the replacement must survive in the new contract.

Malformed snapshots and reports fail closed. JSON failures still conform to
the published report schema through its explicit success/failure union.

## Plan-only migration scenario

The separate `migration-plan-scenario/` is synthetic, plan-only input. It keeps
the compiler and experimental edition unchanged while exercising stable
breaking drift, deprecation, and replacement:

```bash
python3 scripts/ci/check-compatibility-v1.py \
  --old stage1/compatibility/fixtures/migration-plan-scenario/old.json \
  --new stage1/compatibility/fixtures/migration-plan-scenario/new.json \
  --policy stage1/compatibility/fixtures/migration-plan-scenario/policy.json \
  --json

cargo run --manifest-path stage1/Cargo.toml -p axiomc -- \
  migrate \
  --report stage1/json-fixtures/migration-plan/success.report.json \
  --json
```

`axiomc migrate` validates successful report consistency and emits
`axiom.migration_plan.v1`. It never rewrites source, resolves packages,
publishes releases, or changes compatibility policy.

## Remaining qualification

This source-derived policy and corpus are necessary but not sufficient to close
issue `#1457`. A later qualification lane must build or obtain two immutable
compiler versions, run both against the same corpus, publish exact old/new
compiler and contract identities, and validate forward/backward outcomes. Until
that evidence exists, readiness remains `partial`.
