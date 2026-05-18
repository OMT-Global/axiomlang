# Axiom Vision

Axiom is an agent-native typed intent layer and semantic construction system.
Its purpose is to let humans and agents describe systems in terms of typed
intent, effects, invariants, required evidence, and generated artifacts.

## Problem

Agents can edit files, run tests, and repair compiler errors, but file editing
alone loses the reason a system exists. The durable interface needs to answer:

- What is this package intended to provide?
- Which capabilities and effects are allowed?
- Which invariants must hold?
- Which evidence proves the implementation?
- Which artifacts should be produced, verified, or regenerated?

Compiler layers such as syntax trees, HIR, MIR, and backend codegen are
necessary, but they are not enough as the agent-facing source of truth.

## Thesis

Axiom should be the semantic layer above implementation targets. Source files
describe typed intent. The compiler, verifiers, generators, and backends project
that intent into artifacts while preserving traceable evidence.

## Architecture Layers

- Axiom language/source: the human-readable syntax checked into the package.
- Axiom semantic graph / Intent IR: the canonical graph of packages, modules,
  types, functions, capabilities, effects, axioms, evidence, decisions,
  dependencies, runtime surfaces, and artifacts.
- Axiom compiler: the implementation that parses, lowers, validates, and
  prepares semantic data for targets.
- Axiom generators: tools that project semantic nodes into code, tests, docs,
  policies, schemas, runbooks, or service contracts.
- Axiom verifiers: tools that validate type rules, capability gates, effect
  policies, invariants, evidence, and generated artifacts.
- Axiom runtime/backend targets: Rust, native code, service languages, OpenAPI,
  SQL, Terraform/OpenTofu, policy bundles, and documentation outputs.

## Core Primitives

- `axiom`: a named invariant or truth the system must preserve.
- `capability`: a semantic unit of intent with inputs, effects, guarantees,
  evidence, and artifacts.
- `effect`: an observable operation against a resource or runtime surface.
- `evidence`: objective proof such as tests, conformance fixtures, schema
  validation, security fixtures, benchmark baselines, or review records.
- `artifact`: an output such as a binary, source projection, schema, API
  contract, migration, policy bundle, document, or runbook.
- `intent IR`: the stable representation agents inspect and transform.
- `semantic graph`: the connected graph of intent nodes and relationships.
- `target backend`: a projection from semantic graph nodes to executable or
  operational artifacts.
- `agent repair`: a structured plan that ties diagnostics, affected semantic
  nodes, safe edit surfaces, and required evidence together.

## Non-Goals

- Axiom is not a Rust replacement.
- Axiom is not merely a compiler.
- Axiom is not YAML-as-programming.
- Axiom is not natural language execution.
- Axiom is not bound to generated Rust.

## Bootstrap Path

The stage1 Rust compiler remains the supported implementation path while the
semantic layer is introduced. New semantic primitives should land schema-first,
fixture-backed, and inspectable before they become runtime behavior. Backend
work should treat Rust and native execution as target implementations, not as
canonical semantics.

## Related Docs

- [Rust Bootstrap Boundary](rust-bootstrap-boundary.md)
- [Implementation Language Positioning](positioning/implementation-languages.md)
- [Roadmap](roadmap.md)
- [Stage1 Agent-Grade Compiler](stage1-agent-grade-compiler.md)
