# Zero, Rust, Go, and Axiom's Layer

Axiom should not be framed as "a better Rust" or "another Zero." It occupies a
different layer:

```text
Human / agent intent
        ↓
Axiom semantic graph / Intent IR
        ↓
Targets: Rust / Cranelift / Zero / Go / TS / SQL / OpenAPI / Terraform / policy
        ↓
Runtime systems and artifacts
```

Axiom defines what must be true and what must be produced. Implementation
languages define how the resulting artifact runs.

## Rust

Rust is the current bootstrap host and generated-source backend. It is valuable
for building `axiomc`, testing the stage1 language, and projecting native
workloads through `rustc`. Rust should remain a target implementation detail,
not the vocabulary for Axiom semantics.

## Cranelift And Native Backends

A direct native backend can reduce dependence on generated Rust and make Axiom
execution more direct. Native backend work should still consume backend-neutral
semantic data and report unsupported features as target diagnostics.

## Zero

Zero is positioned as an agent-friendly programming language. Axiom may later
target Zero if it becomes useful, but adopting Zero as a backend would not make
Zero the source of Axiom semantics.

## Go, TypeScript, And Python

Application and service targets such as Go, TypeScript, and Python are useful
when the desired artifact is a service, library, integration, or automation
surface. They should be target projections from semantic graph nodes.

## SQL, OpenAPI, Terraform, And Policy

Some artifacts are not general-purpose code. SQL migrations, OpenAPI specs,
Terraform/OpenTofu modules, policy bundles, documentation, and runbooks should
be first-class target outputs when the semantic graph requires them.

## Position

Axiom is the semantic intent layer above implementation languages and artifact
formats. It should preserve typed intent, effects, invariants, evidence, and
provenance across all targets.
