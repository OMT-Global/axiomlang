# Semantic Declarations v0

Stage1 supports metadata-only semantic declarations for agent inspection. These declarations make capabilities and axioms traceable, but they do not execute, authorize runtime behavior, or prove invariants.

## Axioms

```axiom
axiom NoNegativeBalance {
  scope Authorization
  severity fatal
  description "Authorization must never produce a negative available balance."
}
```

An `axiom` names an invariant that agents and future verification tools can reference. In v0, `assert` text is preserved as metadata and is not formally solved.

## Semantic Capabilities

```axiom
capability AuthorizePayment {
  input account: AccountRef
  input amount: Money

  effects {
    read AccountStore
    write AuthorizationStore
    emit PaymentAuthorized
  }

  preserves NoNegativeBalance
  requires evidence PaymentAuthorizationEvidence
}
```

Semantic capabilities differ from manifest capabilities. Manifest capabilities gate host/runtime surfaces such as `fs`, `net`, and `crypto`. Semantic capabilities describe system intent, inputs, effects, invariants, and evidence for agents.

## Evidence

```axiom
evidence PaymentAuthorizationEvidence {
  description "Request evidence used for authorization review."
}
```

Evidence declarations are optional in v0. If a package declares evidence nodes, capability evidence references are validated against them.

## Inspection

`axiomc inspect graph <path> --json` emits semantic nodes and edges:

- `axiom` nodes for invariant declarations.
- `capability` nodes for semantic capability declarations.
- `evidence` nodes for evidence declarations.
- `preserves` edges from capabilities to axioms.
- `requires_evidence` edges from capabilities to evidence.

## Validation

`axiomc check` rejects duplicate axiom, capability, and evidence names. It also rejects `preserves` references to missing axioms.
