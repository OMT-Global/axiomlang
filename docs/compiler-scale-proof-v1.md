# Compiler-Scale Runtime Proof v1

Compiler-Scale Runtime Proof v1 defines the evidence required before a
multi-package AxiOM compiler workload may be reported as runtime-complete. It is
a qualification contract and fixture scaffold for #1427. It does not authorize
the workload while the runtime dependencies remain incomplete, and it is not
permission to remove the bootstrap compiler.

The machine-readable target and current evidence floor are stored in
`stage1/compiler-contracts/snapshots/compiler-scale-proof-v1.json` and
validated against
`stage1/compiler-contracts/schemas/axiom.compiler_scale_proof.v1.schema.json`.
Run the contract and mutation gates with:

```bash
make stage1-compiler-scale-proof-v1
make stage1-compiler-scale-proof-v1-test
```

These gates deliberately keep readiness blocked at `syntax_only`. They accept
the current source-layout, boundary, small-spike, and direct-native subset
evidence while rejecting any attempt to treat that evidence as a compiler-scale
runtime proof.

## Workload boundary

The proof workload is a checked multi-package AxiOM program whose package roles
cover diagnostics, syntax, package graph, HIR, MIR, standard-library bindings,
backend contracts, native backend planning, evidence, commands, and LSP
services. It loads source, tokenizes and parses, traverses a package graph,
performs typed checks, emits diagnostics, plans MIR/backend work, dispatches
commands, and links evidence to outputs.

The six required command surfaces are `check`, `build`, `run`, `test`, `doc`,
and `lsp`. The workload path may not invoke an external toolchain process or
route through the legacy generated-host compatibility package.

## Material scale floor

A fixture-only sample cannot satisfy this proof. The checked workload must
contain at least eight packages, twenty source files, two thousand nonblank
AxiOM source lines, eighty functions, one thousand semantic nodes, twelve
package-dependency edges, and all six command surfaces. Counts are taken from
the same source and semantic evidence graph used by the build; generated files,
duplicated padding, comments-only padding, and unreachable fixture copies do
not count.

The scale report records the exact source digests and count method. Reviewers
must be able to reproduce every count from the checked package set.

## Build once and run many

The workload is built once. The same artifact digest then processes at least
two source trees supplied after the build. Source A and source B have distinct
input digests and must produce correspondingly distinct diagnostics, semantic
summaries, or artifact plans without a rebuild between runs.

Runtime inputs include source files, argv, stdin, cwd, scoped environment, and
prior command artifacts. Build-time evaluation, fixture-specific output,
compiler-known source substitution, and static-output replay cannot satisfy the
proof. The evidence reports `native_runtime` execution and `runtime_lowered`
lowering, with generated host source absent.

## Build purity and runtime state

Compilation cannot execute filesystem, environment, process, network, clock,
randomness, cryptographic, or other runtime authority to manufacture emitted
output. Those effects may execute only after the artifact starts and only under
the program's declared capability policy.

Symbol tables, dependency tables, and work sets use runtime-allocated maps and
sets with deterministic iteration. Fixed literal maps, compile-time tables, and
fixture-shaped switch logic are prohibited. Dynamic values, resources, borrows,
moves, and drops must satisfy the lifecycle and ownership contracts on normal,
error, early-exit, and cancellation paths.

## Evidence graph

One target-neutral evidence graph links package identity, source provenance,
runtime input digest, runtime origin, command surface, MIR digest, lowering
mode, backend plan, capability evidence, target support, output digest, and
exact artifact digest. Every command report references the same graph and
artifact where reuse is required.

The graph must prove that no compiler evaluator, static-output replay,
generated host source, rebuild-between-inputs, or external toolchain process
entered the workload path. Missing or contradictory evidence fails closed.

## Parity and inspection

Representative diagnostics, package decisions, semantic summaries, command
envelopes, and artifact plans match either the current compiler or an explicitly
versioned target-neutral semantic contract. Differences require a versioned
transition; undocumented drift is not parity.

Inspection exposes package and command ownership, runtime origin, source and
input digests, semantic and MIR identities, capabilities, lowering mode,
backend plan, target, output digest, artifact digest, scale counts, and every
remaining blocker. It does not expose backend-local addresses, container
layouts, or bootstrap implementation names as language truth.

## Dispatch and promotion gate

Only fixture and checker scaffolding is authorized while any dependency remains
below `runtime_complete`. The executable workload must not be dispatched from
this contract slice. A later implementation may change that state only with
the complete dependency matrix, all required proofs, and independent acceptance
review.

Current diagnostics and small self-hosting spikes are useful feasibility
evidence, and direct-native subset reports already exclude generated host
source. They do not meet the scale floor, the source A/B artifact-reuse proof,
runtime map/set and host-ABI requirements, or the six-command package proof.
Readiness therefore remains blocked and non-promotable. The stable cutover
diagnostic is `self_host.compiler_scale_proof_not_qualified`.
