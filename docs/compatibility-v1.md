# Compatibility v1

Compatibility v1 makes public AxiOM contracts explicit and machine-comparable.
It is deliberately target-neutral: Rust layouts, enums, Serde encodings, crate
versions, and provider errors are implementation details, never contract IDs.

## Public contract snapshot

`axiom.public_contract.v1` inventories a versioned public surface. Every entry
has an AxiOM ID, kind, semantic version, stability, and signature. The required
kinds are `compiler`, `language`, `stdlib`, `cli`, `package`, `abi`, `schema`,
and `artifact`; an implementation cannot omit a category merely because it has
not yet been fully qualified. The compiler support range is compared as the
`compiler` surface so a narrowed supported range is visible as breaking drift.

An edition has a four-digit ID and is experimental, supported, or deprecated.
Changing editions is breaking and requires a migration action. Deprecated
editions and surfaces require both a migration action and a replacement AxiOM
ID. Compiler support is expressed as an inclusive SemVer range, independently
of package or runtime-provider versions. The new snapshot's optional
`migrations` object maps removed surface IDs to their required migration action;
this makes a removal auditable even though the removed surface cannot appear in
the new `surfaces` list.

## Compatibility report

Run the checker against an old and new public-contract snapshot:

```bash
python3 scripts/ci/check-compatibility-v1.py \
  --old stage1/examples/compatibility_v1/old.json \
  --new stage1/examples/compatibility_v1/current.json --json
```

It emits `axiom.compatibility_report.v1`. The report is deterministic and sorts
breaking, deprecated, additive, then compatible entries by surface kind and
ID. A changed signature, kind, downgraded version, or major-version increase is
breaking. A newly deprecated surface is reported separately and must carry a
migration action. Additions and compatible minor changes remain visible rather
than being inferred from source-text diffs.

The checker fails closed when contract snapshots are malformed, a public ID is
duplicated, a removed surface lacks an old-contract migration action, or a new
breaking/deprecated surface lacks one. It does not claim that downstream
packages have migrated: a future package resolver and edition command must
consume this report before #1457 can close.

## Migration policy

- Stable public surfaces use SemVer: a major change is breaking; additions use
  a minor version; compatible corrections use a patch version.
- Experimental surfaces are still recorded and diffed, but their stability
  explicitly limits compatibility promises.
- Deprecation retains the surface through its documented edition window and
  names a semantic replacement, never a Rust type or an implementation path.
- Schemas and artifacts version their envelope IDs and appear in the same
  report as language and CLI changes, so generated-output drift cannot be
  hidden from release review.

The schemas are
`stage1/schemas/axiom-public-contract-v1.schema.json` and
`stage1/schemas/axiom-compatibility-report-v1.schema.json`.
