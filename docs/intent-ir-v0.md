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

The stage1 compiler does not emit full Intent IR yet. This v0 document and
schema define the first stable shape; complete real-package emission is tracked
by [#1418](https://github.com/OMT-Global/axiomlang/issues/1418) so inspection,
verification, repair, artifact planning, and autonomous execution can consume
one canonical graph.

## Envelope

Every Intent IR document uses:

- `schema_version`: `axiom.intent_ir.v0`
- `graph_id`: stable graph identifier
- `package`: root package node id
- `nodes`: semantic graph nodes
- `edges`: semantic graph relationships

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

## V0 Fixture

The smoke fixture lives at:

```text
stage1/examples/intent_ir_smoke/intent-ir.json
```

It includes one package, one module, one function, one manifest capability, one
effect edge, one evidence placeholder, and one artifact placeholder. It is
validated by `stage1/schemas/axiom-intent-ir-v0.schema.json`.

## Boundaries

Intent IR v0 does not require:

- parser integration
- proof solving
- code generation from Intent IR
- runtime execution

Those behaviors should be added by later issues after the schema and smoke
fixture are stable.

## Related Schemas

- [Backend Target Interface v0](backend-target-interface-v0.md) and
  `stage1/schemas/axiom-target-v0.schema.json` describe the contract a
  backend declares against the semantic graph defined here.
