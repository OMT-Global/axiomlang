# Compiler stdlib catalog authority

The `compiler.stdlib` catalog has one semantic authority:
`stage1/compiler-contracts/sources/stdlib-catalog-authority-v1.json`.
Catalog version 2.0.0 owns each module's aggregate capability requirements,
per-symbol defaults and overrides, and provider namespace. The checked catalog
snapshot combines that authority with public signatures parsed from the
embedded AxiOM module sources.

The capability ledger is downstream parity evidence. It must contain the same
modules, functions, and capability sets, but it no longer supplies the catalog
semantics. This direction matters: changing a generated ledger row cannot
silently redefine a standard-library effect or provider binding.

## Authority chain

For every module, `scripts/ci/check-stdlib-catalog.py` requires all of the
following:

1. The authority contract has exactly one deterministically ordered module row
   with sorted, unique capabilities, closed per-symbol policy when a module has
   mixed effects, and a unique AxiOM provider namespace.
2. The bootstrap loader exposes AxiOM source for that module, and its parsed
   public signatures exactly match the capability-ledger inventory.
3. Per-symbol effects aggregate exactly to the module capability set, and that
   set matches the ledger. The ledger is rejected when it adds, removes, or
   changes a capability.
4. Every public symbol receives a stable provider-contract identifier beneath
   the module's declared namespace. Bindings cannot contain host-language
   names or collide with another symbol.
5. The catalog checker parses the bounded bootstrap runtime enforcement table
   and compares every catalog symbol effect while that table remains the
   loader.
6. The checked snapshot, schema, module source digests, and release digest all
   agree byte-for-byte with the regenerated catalog.

The authority document is deliberately small. Function signatures remain
AxiOM source facts, while symbol inventory remains a checked parity edge. This
avoids duplicating 304 signatures in a handwritten metadata file without
letting Rust tables or generated evidence own capability semantics.

## Bootstrap and rollback boundary

`stage1/crates/axiomc/src/stdlib.rs` remains the bootstrap source loader until
issue #1436 qualifies an executable catalog consumer. It is not the authority
for capability, effect, or provider identity. The current catalog therefore
remains a `static_spike`, and issue #1478 stays open while these larger gates
remain:

- externalize the remaining inline AxiOM module text from the Rust loader;
- qualify intrinsic/provider execution against the runtime contracts;
- pass positive, denial, target, lifecycle, and runtime-origin native proofs;
- disable Rust registration for the qualified compiler path with a tested
  rollback switch.

## Validation

Run:

```sh
python3 scripts/ci/check-stdlib-catalog.py --json
python3 scripts/ci/test-check-stdlib-catalog.py
make stage1-stdlib-test
```

The first command reports both the semantic authority digest and catalog
release digest. The regression suite mutates the authority/ledger edge,
provider namespaces, per-symbol effects, deterministic ordering, closed shape,
source selection, and checked catalog fields to prove those boundaries fail
closed.
