# Intent IR / Semantic Graph v0

Intent IR is Axiom's canonical agent-facing semantic graph. It describes what a
package means, which effects it may perform, which invariants it must preserve,
which evidence supports it, and which artifacts it plans or emits.

Intent IR is not the AST, HIR, or MIR:

- The AST preserves source syntax shape.
- HIR is the compiler's typed lowering layer.
- MIR is the compiler's backend-oriented intermediate form.
- Intent IR is the durable semantic contract agents inspect, transform,
  validate, repair, and trace across generated artifacts.

The stage1 compiler emits Intent IR for real packages and workspaces with
`axiomc inspect intent <path> --json`. The emitted document is the canonical
semantic input for inspection, semantic diff, verification, repair planning,
and artifact planning. Consumers should not reconstruct competing partial
graphs from compiler implementation details.

## Envelope

Every Intent IR document uses:

- `schema_version`: `axiom.intent_ir.v0`
- `graph_id`: stable graph identifier
- `package`: root package node id
- `provenance`: deterministic source inputs and their digests
- `nodes`: semantic graph nodes
- `edges`: semantic graph relationships
- `diagnostics`: explicit, node-linked completeness diagnostics

Arrays and object members use deterministic ordering. The command does not add
timestamps or machine-specific absolute paths, so repeated runs over unchanged
inputs are byte-stable.

## Node Types

- `Package`: package-level semantic root.
- `Module`: source or generated module boundary.
- `Type`: declared or referenced type.
- `Function`: callable behavior.
- `Capability`: semantic or manifest capability.
- `Effect`: observable operation against a resource or runtime surface.
- `Axiom`: invariant or truth the package must preserve.
- `Evidence`: proof record, placeholder, or validation requirement.
- `Artifact`: generated, planned, or verified output.
- `Decision`: recorded design or policy decision.
- `Dependency`: package, module, or external dependency.
- `RuntimeSurface`: runtime API, stdlib module, or host surface.

## Edge Types

- `declares`: a package or module declares a semantic node.
- `uses`: a node uses another node.
- `requires`: a node requires a capability, evidence item, dependency, or
  runtime surface.
- `preserves`: a capability, function, or artifact preserves an axiom.
- `allows_effect`: a capability allows an effect.
- `emits`: a node emits an effect or artifact.
- `verified_by`: a node is verified by evidence.
- `generated_from`: an artifact is generated from a semantic node.
- `depends_on`: a node depends on another node.
- `implements`: a target artifact implements a semantic node.
- `violates`: a diagnostic, evidence item, or decision records a violation.

## Stable IDs

Stable IDs are URI-like and should not depend on Rust implementation details:

```text
axiom://package/<package-name>
axiom://package/<package-name>/module/<module-name>
axiom://package/<package-name>/capability/<capability-name>
axiom://package/<package-name>/axiom/<axiom-name>
```

Additional node kinds should extend the same package-rooted convention, for
example:

```text
axiom://package/<package-name>/function/<function-name>
axiom://package/<package-name>/artifact/<artifact-name>
axiom://package/<package-name>/evidence/<evidence-name>
```

IDs describe Axiom concepts. Compiler-host names such as Rust structs, Cargo
packages, backend implementation types, and code-generator internals are not
part of this contract.

## Provenance and diagnostics

`provenance.source_digest` is a digest of the normalized semantic inputs.
`provenance.inputs` records each package-relative source input, its owning
package node, and its digest. `path_policy` is fixed to `package_relative` so
documents do not capture checkout or machine paths.

Diagnostics make incomplete semantic coverage visible. An unsupported or
partially represented node family produces a stable diagnostic code instead of
being silently omitted. Every diagnostic has at least one `node_ids` entry;
optional `node_kind` and `source_span` fields narrow the affected contract.
An empty diagnostics array means the emitter found no known completeness gap.

Every artifact is itself an `Artifact` node and must have a `generated_from` or
`implements` edge to the semantic node it traces to. The same node identity is
used by artifact inspection and planning.

## CLI

Emit a graph for a package or workspace:

```bash
axiomc inspect intent stage1/examples/agent_native_authorize --json
axiomc inspect intent stage1/examples/workspace --json
```

The first command exercises capabilities, effects, axioms, evidence, and
artifacts. The workspace fixture exercises package dependencies and modules
across multiple member packages. Both outputs validate against
`stage1/schemas/axiom-intent-ir-v0.schema.json`.

## V0 Fixture

The smoke fixture lives at:

```text
stage1/examples/intent_ir_smoke/intent-ir.json
```

It includes one package, one module, one function, one manifest capability, one
effect edge, one evidence placeholder, one artifact placeholder, deterministic
input provenance, and an empty completeness diagnostic set. It is validated by
`stage1/schemas/axiom-intent-ir-v0.schema.json`.

## Boundaries

Intent IR remains distinct from:

- proof solving
- code generation from Intent IR
- runtime execution

Those systems may consume or contribute nodes and evidence without defining the
graph contract.

## Related Schemas

- [Backend Target Interface v0](backend-target-interface-v0.md) and
  `stage1/schemas/axiom-target-v0.schema.json` describe the contract a
  backend declares against the semantic graph defined here.
