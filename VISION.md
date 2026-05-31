# Axiom Vision

Version: 0.2

Axiom is an agent-native typed intent layer and semantic construction system. It should let humans and agents describe software in terms of intent, capabilities, effects, invariants, required evidence, and generated artifacts instead of only editing files and hoping the reason for the system survives.

The current product is the Rust-hosted `stage1` compiler and `axiomc` workflow. Rust is the bootstrap implementation and generated-Rust backend, not the identity of the language. The long-term identity is the semantic graph and Intent IR that agents can inspect, verify, and project into code, tests, docs, policies, schemas, runbooks, and service contracts.

## Who It Serves

- Agent operators who need stable semantic context before changing code.
- Language contributors building the first practical `axiomc` compiler and conformance corpus.
- Future backend authors who need a target contract that prevents Rust, native code, OpenAPI, SQL, policy bundles, or docs from becoming ad hoc projections.

## Current Product Boundary

- Supported path: `stage1/` and the `axiomc` commands for `new`, `check`, `build`, `run`, `test`, `caps`, package manifests, conformance fixtures, generated Rust, and capability inspection.
- Semantic path: schema-first, fixture-backed increments such as target contracts, evidence reports, artifact plans, effect models, and semantic declarations.
- Deprecated path: Python `stage0` and bytecode VM execution are documentation and migration references only.

## Product Principles

- Semantics define the contract; backend implementation details only explain how a target realizes that contract.
- Every new semantic primitive needs a schema or schema delta, fixture coverage, and an inspection path before runtime behavior depends on it.
- Diagnostics and evidence must be structured enough for agents to attach them to PRs without scraping terminal logs.
- Capability and effect boundaries are part of the language, not optional policy afterthoughts.
- The compiler should become agent-grade before it becomes ambitious: complete package workflows and clear JSON diagnostics matter more than direct-native parity.

## Near-Term Direction

- Finish the agent-grade compiler floor: ownership/borrowing, package workflows, capability-gated stdlib/runtime APIs, and three proof workloads.
- Make `axiomc` inspection commands useful for agent repair: evidence, artifacts, semantic nodes, unsupported target features, and safe edit surfaces.
- Keep backend target contracts explicit as generated Rust, direct native, and non-code artifact targets evolve.
- Preserve the conformance corpus and fast gates as the minimum trust surface for every language slice.

## Non-Goals

- Axiom is not a generic task manager, YAML workflow engine, natural-language executor, or Rust replacement.
- Do not introduce semantic-layer behavior as undocumented compiler magic.
- Do not let generated Rust or any future target define Axiom's vocabulary.
- Do not revive Python VM execution as a supported product path.
