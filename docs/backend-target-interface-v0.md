# Backend Target Interface v0

Axiom is a semantic intent layer above implementation languages and artifact
formats (see [vision.md](vision.md) and
[positioning/implementation-languages.md](positioning/implementation-languages.md)).
Every backend that consumes the Axiom semantic graph and emits an artifact is a
*target*. This document defines the target interface so future backends do not
become ad hoc and no single target becomes Axiom's identity.

Target interface v0 is descriptive. It does not change any backend behavior.
The direct-native Cranelift backend and the generated-Rust compatibility
backend are both describable using this schema. New non-Rust artifact targets
must declare their contracts using the same shape before they ship.

The first implemented non-source artifact target is
[OpenAPI Target v0](openapi-target-v0.md), which emits an `openapi_spec`
artifact from HTTP-serving semantic routes.
[Policy Bundle Target v0](policy-bundle-target-v0.md) emits a
`policy_bundle` artifact from manifest capabilities and effect records.
[SQL Migration Target v0](sql-migration-target-v0.md) emits
`sql_migration` artifacts from declared schema structs and schema axioms.
[Terraform/OpenTofu Target v0](terraform-target-v0.md) emits a
`terraform_module` artifact from capability and runtime-effect surfaces.
[Runbook Target v0](runbook-target-v0.md) emits a `runbook` artifact from
capability, effect, evidence, and artifact inspection records.

## Where It Fits

```text
Axiom semantic graph / Intent IR
            │
            ▼
    Target contract (this doc)
            │
            ▼
Targets: native_binary / rust_source / cranelift / zero_source / go_source /
typescript_source / python_source / openapi_spec / sql_migration /
terraform_module / policy_bundle / documentation / runbook
```

The semantic graph defines *what must be true and what must be produced*.
Targets define *how a specific artifact is realized*. The boundary is the
target interface: the input semantic node kinds, the effect kinds the target
supports, the type features it understands, the artifacts it emits, the
evidence it can record, and the diagnostics it raises for features it does not
support.

This split preserves the
[Rust Bootstrap Boundary](rust-bootstrap-boundary.md): generated Rust is a
backend projection, not Axiom's vocabulary, and the same constraint applies to
every target listed below.

## Target Classes

A target class is the kind of artifact the backend produces. The v0 set is:

| Class | Description |
|---|---|
| `native_binary` | Architecture-specific executable or library produced directly by an Axiom-owned native backend. |
| `rust_source` | Generated Rust source that is compiled by `rustc` into a host binary. Stage1 keeps this as an explicit compatibility backend while direct-native output is the default. |
| `zero_source` | Generated Zero source. Future agent-friendly systems target. |
| `go_source` | Generated Go source for services and tooling. |
| `typescript_source` | Generated TypeScript source for clients, services, and tooling. |
| `python_source` | Generated Python source for tooling and automation surfaces. |
| `openapi_spec` | OpenAPI / AsyncAPI / gRPC contract document derived from semantic capabilities. |
| `sql_migration` | SQL migration script generated from declared schema or invariants. |
| `terraform_module` | Terraform / OpenTofu module generated from declared infrastructure intent. |
| `policy_bundle` | Policy or capability allowlist artifact (e.g., OPA, manifest gates). |
| `documentation` | Markdown / HTML documentation derived from semantic nodes. |
| `runbook` | Operator runbook derived from declared operations and evidence. |

Target classes are not exclusive: one package may declare multiple target
contracts and emit multiple artifacts from the same semantic graph.

## Target Contract

A target contract describes what a single backend implementation accepts from
the semantic graph and what it produces. v0 contract fields:

- `id`: stable target identifier, for example
  `axiom://target/stage1-generated-rust`.
- `class`: one of the target classes above.
- `description`: short human-readable summary.
- `status`: `experimental`, `supported`, or `deprecated`.
- `input_node_kinds`: semantic node kinds the target consumes
  (`Package`, `Module`, `Type`, `Function`, `Capability`, `Effect`, `Axiom`,
  `Evidence`, `Artifact`, `Decision`, `Dependency`, `RuntimeSurface`).
  These match `nodeKind` in the Intent IR schema.
- `supported_effect_kinds`: effect identifiers the target can emit safely,
  using the namespaced form from the effect model (for example
  `network.tcp.bind`, `clock.now`).
- `supported_type_features`: declarative tags such as `numeric.signed`,
  `numeric.unsigned`, `numeric.float`, `aggregate.struct`, `aggregate.enum`,
  `borrow.mutable`, `async.task`, `generics.bounded`.
- `artifact_outputs`: artifact records the target plans to emit, matching the
  Artifact node shape used in the Intent IR (`id`, `kind`, `path`,
  `generated_from`, `status`).
- `evidence_requirements`: evidence kinds required before a target may declare
  itself shipped for a given package (for example `unit_test`, `conformance`,
  `capability_denial_test`).
- `unsupported_feature_diagnostics`: structured diagnostics emitted when the
  semantic graph requires features the target does not implement.

## Diagnostics

Targets must not silently drop semantic content. When a backend cannot lower a
semantic node, it must emit a diagnostic record using the existing diagnostic
envelope. v0 fields specific to target diagnostics:

- `target_id`: the contract id that produced the diagnostic.
- `unsupported_kind`: which contract slot was violated
  (`input_node_kind`, `effect_kind`, `type_feature`, `artifact_output`,
  `evidence_requirement`).
- `semantic_node`: the offending node id.

Diagnostics are advisory at the target interface layer in v0. They become
blocking only when the artifact plan or evidence model declares the affected
artifact required.

## Mapping The Generated-Rust Backend

The current `axiomc build` pipeline lowers Axiom source to MIR, projects MIR
into generated Rust, and invokes `rustc`. In target-contract terms:

```json
{
  "id": "axiom://target/stage1-generated-rust",
  "class": "rust_source",
  "description": "Stage 1 generated-Rust backend compiled by rustc into a native binary.",
  "status": "supported",
  "input_node_kinds": ["Package", "Module", "Type", "Function", "Capability", "Effect"],
  "supported_effect_kinds": [
    "clock.now",
    "clock.sleep",
    "env.read",
    "fs.read",
    "fs.write",
    "network.dns.resolve",
    "network.http.get",
    "network.tcp.bind",
    "network.tcp.connect",
    "network.udp.send",
    "process.status",
    "crypto.hash",
    "crypto.mac",
    "crypto.rand",
    "crypto.sign"
  ],
  "supported_type_features": [
    "numeric.signed",
    "numeric.unsigned",
    "numeric.float",
    "aggregate.struct",
    "aggregate.enum",
    "borrow.mutable",
    "async.task"
  ],
  "artifact_outputs": [
    {
      "id": "axiom://target/stage1-generated-rust/artifact/source",
      "kind": "rust_source",
      "path": "target/axiomc/<package>.rs",
      "generated_from": ["axiom://package/<package>"],
      "status": "planned"
    }
  ],
  "evidence_requirements": ["unit_test", "conformance"],
  "unsupported_feature_diagnostics": []
}
```

The `rust_source` class is intentional: the generated Rust is the artifact the
target produces. The host binary is a downstream consequence of `rustc`, not
the target's primary output. Future direct native backends drop the
intermediate Rust source entirely.

## Mapping The Direct Native Backend Roadmap

The direct native backend roadmap (umbrella #105, with Cranelift integration,
MIR-to-native lowering, default-replacement, and generated-Rust removal slices)
maps to a separate `native_binary` target contract:

```json
{
  "id": "axiom://target/stage1-direct-native",
  "class": "native_binary",
  "description": "Direct MIR-to-native backend with no generated-Rust intermediate.",
  "status": "experimental",
  "input_node_kinds": ["Package", "Module", "Type", "Function", "Capability", "Effect"],
  "supported_effect_kinds": [
    "clock.now",
    "clock.sleep",
    "env.read",
    "fs.read",
    "fs.write",
    "network.dns.resolve",
    "network.tcp.bind",
    "network.tcp.connect",
    "process.status",
    "crypto.hash",
    "crypto.rand"
  ],
  "supported_type_features": [
    "numeric.signed",
    "numeric.unsigned",
    "aggregate.struct",
    "aggregate.enum"
  ],
  "artifact_outputs": [
    {
      "id": "axiom://target/stage1-direct-native/artifact/binary",
      "kind": "native_binary",
      "path": "target/axiomc-native/<package>",
      "generated_from": ["axiom://package/<package>"],
      "status": "planned"
    }
  ],
  "evidence_requirements": ["unit_test", "conformance"],
  "unsupported_feature_diagnostics": [
    {
      "target_id": "axiom://target/stage1-direct-native",
      "unsupported_kind": "effect_kind",
      "semantic_node": "<placeholder>",
      "message": "network.http.get is not yet implemented on the direct native backend."
    }
  ]
}
```

The two targets coexist during the migration. Each roadmap slice for the
direct native backend can be tracked against the `supported_effect_kinds`,
`supported_type_features`, and `unsupported_feature_diagnostics` slots in
this contract, not by editing the generated-Rust contract. Runtime value and
host-service requirements for the direct native backend are tracked separately
in [Direct Native Runtime ABI v0](direct-native-runtime-abi-v0.md).

## Future Target Sketches

These sketches are not part of stage1 work. They illustrate that the schema is
expressive enough for non-source targets:

- `openapi_spec`: implemented for stage1 as
  [OpenAPI Target v0](openapi-target-v0.md). Input node kinds include
  `Package`, `Module`, `Function`, `Capability`, `Effect`, and `Type`; effect
  kinds describe `network.http.*` and `network.tcp.bind`; artifacts are JSON
  OpenAPI 3.1 documents.
- `sql_migration`: implemented for stage1 as
  [SQL Migration Target v0](sql-migration-target-v0.md). Input node kinds
  include `Package`, `Module`, `Type`, and `Axiom`; artifacts are deterministic
  PostgreSQL-compatible forward and rollback SQL files plus a schema snapshot.
- `terraform_module`: implemented for stage1 as
  [Terraform/OpenTofu Target v0](terraform-target-v0.md). Input node kinds
  include `Package`, `Capability`, `Effect`, and `RuntimeSurface`; artifacts are
  provider-neutral HCL modules.
- `policy_bundle`: implemented for stage1 as
  [Policy Bundle Target v0](policy-bundle-target-v0.md). Input node kinds
  include `Package`, `Capability`, and `Effect`; artifacts are deterministic
  JSON policy files consumed by allowlist gates.
- `runbook`: implemented for stage1 as [Runbook Target v0](runbook-target-v0.md).
  Input node kinds include `Package`, `Capability`, `Effect`,
  `RuntimeSurface`, `Evidence`, and `Artifact`; artifacts are deterministic
  Markdown operator runbooks.

These sketches share the same contract shape. Adding them does not require
inventing a new schema.

## Schema

The machine-readable shape lives at
`stage1/schemas/axiom-target-v0.schema.json`. It accepts both single contracts
and target manifests that list multiple contracts so a package can declare all
the artifacts it plans to produce from one semantic graph.

## Boundaries

Target interface v0 does not require:

- A new backend implementation.
- Renaming or restructuring the generated-Rust compatibility pipeline.
- Implementing Zero, Go, TypeScript, or provider-specific infrastructure
  generation.
- Runtime sandboxing or new capability gates.

Those behaviors should be added by later issues after the contract shape is
stable and at least one backend is described by it in tree.
