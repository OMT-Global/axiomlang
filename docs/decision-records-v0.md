# Decision Records v0

Decision Records v0 make checked-in design and policy decisions visible to the
semantic graph. They complement RFCs and prose docs; they do not replace the
human review process.

## File Layout

Packages may add JSON decision records under:

```text
decisions/*.json
```

Each record has:

- `id`: stable decision identifier.
- `title`: human-readable decision name.
- `status`: `proposed`, `accepted`, `superseded`, or `rejected`.
- `context`: why the decision exists.
- `decision`: the chosen policy or design.
- `governs`: semantic graph edges from the decision to the nodes it constrains.

## Graph Mapping

`axiomc inspect graph <path> --json` loads decision records as `decision`
nodes. Their `status` stays visible on the node so `superseded` records are
distinguishable from `accepted` records.

Supported `governs[].relationship` values are:

- `preserves`
- `violates`
- `depends_on`

Targets are regular `axiom://...` semantic node ids, such as an axiom,
capability, or artifact node.

## Schema

Decision records are described by
`stage1/schemas/axiom-decision-record-v0.schema.json`.

## Boundaries

Decision Records v0 are declarative metadata. They do not enforce that every
change cites a decision and do not auto-generate decisions from PRs or code.
