# Compiler Native Backend and Runtime v1

Compiler Native Backend and Runtime v1 defines the target-neutral evidence
required before `compiler.backend.native` may be reported as an AxiOM-owned,
runtime-complete backend package. It is a contract and fixture scaffold for
#1474. It does not authorize backend dispatch, generated-host-path retirement,
or semantic cutover while the dependency gates remain incomplete.

The machine-readable target and current evidence floor live in
`stage1/compiler-contracts/snapshots/compiler-native-backend-runtime-v1.json`
and validate against
`stage1/compiler-contracts/schemas/axiom.compiler_native_backend_runtime.v1.schema.json`.
Run the contract and mutation gates with:

```bash
make stage1-compiler-native-backend-runtime-v1
make stage1-compiler-native-backend-runtime-v1-test
```

These gates keep the production-readiness row at `syntax_only` and `blocked`.
They recognize existing bootstrap contracts and direct-native subset evidence,
but reject claims that the AxiOM-owned backend package or full runtime path is
already qualified.

## Ownership boundary

`compiler.backend.native` consumes executable MIR plus versioned target,
provider, lifecycle, runtime-ABI, and artifact contracts. Its semantic
responsibilities are target selection enforcement, unsupported-feature
rejection, runtime lowering, native object emission, link planning, debug and
source provenance, runtime ABI adaptation, artifact publication, and evidence
linking.

Public behavior is described by AxiOM contracts. Provider-specific data,
bootstrap module paths, physical layouts, and toolchain command lines remain
adapter-local implementation detail. A native backend may use a qualified
provider implementation, but that provider cannot redefine type, effect,
lifecycle, artifact, diagnostic, or target semantics.

## Input and output contract

The required inputs are executable MIR, the selected target contract, runtime
ABI rows, lifecycle and ownership requirements, provider requirements, the
artifact plan, target support evidence, source provenance, and command options.
Every input carries a version or digest suitable for exact evidence linking.

The outputs are native object and binary artifacts, a link plan, runtime shim
requirements, unsupported-feature diagnostics, debug/source provenance,
capability evidence, and a target-neutral backend execution receipt. Missing
inputs, unsupported semantics, or contradictory evidence fail before backend
execution and cannot produce a successful artifact receipt.

## Build purity and runtime sensitivity

Compilation may inspect source, MIR, declared capabilities, target metadata,
and versioned provider metadata. It may not execute the program's runtime
filesystem, environment, network, process, clock, randomness, cryptographic,
or other authority to manufacture emitted output.

One built artifact must respond to runtime inputs supplied after the build.
The same artifact digest is exercised with at least two runtime input sets, and
the resulting semantic output or effect transcript must differ when the inputs
differ. Rebuilding, compiler evaluation, fixture-shaped output, or static
replay does not establish runtime lowering.

## Fail-closed unsupported behavior

Unsupported type features, effects, lifecycle operations, runtime ABI rows,
provider requirements, target features, artifact kinds, and evidence
requirements are rejected before emission. Diagnostics identify the semantic
node, requested target, unsupported contract dimension, required contract
version, and evidence gap without exposing provider-local representation.

An unsupported path cannot silently switch to a compatibility projection,
erase an effect, omit lifecycle behavior, or publish a partial artifact as
successful. The stable qualification diagnostic is
`self_host.native_backend_runtime_not_qualified`.

## Target and provider parity

Every supported production target runs the same semantic corpus and publishes
the same target-neutral receipt shape. Platform-specific bytes may differ, but
diagnostics, exit behavior, runtime effects, lifecycle outcomes, capability
decisions, source provenance, and artifact intent remain equivalent.

Provider parity is measured against versioned contracts and semantic results,
not provider-internal data structures. An accepted provider boundary must be
versioned, fail closed, and emit enough evidence to prove the exact provider,
target, runtime ABI, MIR input, object output, and linked artifact.

## Lifecycle, debugging, and provenance

Dynamic values, resources, borrows, moves, clones, and drops obey the lifecycle
and ownership contracts on normal return, error, early exit, panic/trap, and
cancellation paths. Runtime shims cannot leak, double-release, or silently
change ownership across the provider boundary.

Debug and provenance evidence links source identities and spans through MIR,
backend decisions, object sections, link plans, and final artifacts. Evidence
contains stable semantic identities rather than backend-local addresses or
physical layout as language truth.

## Evidence graph

One receipt graph links package identity, source and MIR digests, target
contract, feature/effect decisions, runtime ABI rows, lifecycle requirements,
provider identity, artifact plan, object digest, link-plan digest, debug
provenance, final artifact digest, capability evidence, diagnostics, and the
exact supported-target result.

Successful evidence proves build purity, executable runtime lowering,
runtime-input sensitivity, lifecycle correctness, unsupported-feature
fail-closed behavior, provider parity, and target-matrix completion. Missing or
contradictory nodes fail closed.

## Dispatch and human cutover gate

This slice authorizes fixture and checker preparation only. Backend dispatch
and readiness promotion remain disabled until #1427, executable MIR, lifecycle,
ownership, dynamic values, program-host ABI, compiler stdlib, target support,
provider contracts, and the AxiOM-owned backend contract are runtime-complete.

Issue #1479 retains the human-only decision to retire or quarantine the legacy
generated-host path. Nothing in this contract permits deletion, release-graph
removal, default-target replacement, or cutover. A later implementation must
provide all required proofs plus independent review and the recorded #1479
human approval before changing those states.
