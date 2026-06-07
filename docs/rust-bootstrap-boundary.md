# Rust Bootstrap Boundary

Rust is the current implementation host for `stage1/axiomc`. It is a useful
bootstrap language and a useful backend projection, but it must not define
Axiom semantics.

## Boundary Rules

- Rust may shape implementation strategy, local data structures, tests, and
  generated backend code.
- Rust must not be the explanation for Axiom semantic concepts.
- Generated Rust is a backend projection, not canonical truth.
- HIR and MIR are compiler implementation layers. They may inform the
  agent-facing semantic graph, but they are not automatically the Intent IR.
- Future non-Rust targets must not require semantic rewrites.

## Anti-Capture Checks

Every new semantic concept should be explainable without relying on:

- Rust lifetimes
- Rust traits
- Cargo
- `Result` or `Option` spelling
- Serde struct layout
- generated Rust internals

If a behavior is backend-specific, document it as backend-specific. If a concept
is semantic, document it in Axiom-neutral language first, then map it to Rust
implementation details as needed.

## PR Checklist

Use this Rust capture check when a PR changes language semantics, semantic IR,
capabilities, effects, evidence, artifacts, or backend contracts:

- [ ] This change does not define Axiom semantics in Rust-specific terms.
- [ ] Any new semantic concept is documented in Axiom-neutral language.
- [ ] Backend-specific behavior is marked as backend-specific.

## Related Docs

- [Axiom Vision](vision.md)
- [Compiler Package Graph Boundary](compiler-package-graph.md)
- [Compiler Command and LSP Packages](compiler-command-lsp-packages.md)
- [Implementation Language Positioning](positioning/implementation-languages.md)
