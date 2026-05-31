# Runbook Target v0

Runbook Target v0 projects the package capability surface, semantic
capabilities, observed runtime effects, evidence records, artifact plan, and
diagnostics into a deterministic Markdown operator runbook.

## Target Contract

```json
{
  "id": "axiom://target/stage1-runbook-v0",
  "class": "runbook",
  "description": "Stage 1 operator runbook generator for capabilities, effects, evidence, and artifacts.",
  "status": "experimental",
  "input_node_kinds": [
    "Package",
    "Capability",
    "Effect",
    "RuntimeSurface",
    "Evidence",
    "Artifact"
  ],
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
    "crypto.mac"
  ],
  "supported_type_features": [],
  "artifact_outputs": [
    {
      "id": "axiom://package/<package>/artifact/operator-runbook",
      "kind": "runbook",
      "path": "dist/runbook.md",
      "generated_from": ["axiom://package/<package>"],
      "status": "generated"
    }
  ],
  "evidence_requirements": ["unit_test", "fixture"],
  "unsupported_feature_diagnostics": []
}
```

## Command

```bash
axiomc generate runbook <path> --out dist/runbook.md --json
```

The command writes a Markdown document and emits a JSON report containing the
same target contract shape used by other target generators.

## Projection Rules

The runbook includes:

- package identity, build entry, target id, and runbook artifact path
- every manifest capability gate, including allowlists, owners, rationale, and
  unsafe rationale
- semantic `capability` declarations, their declared effects, preserved axioms,
  and required evidence records
- observed runtime effects from `axiomc inspect effects`
- evidence summary and evidence records from `axiomc evidence`
- planned and generated artifacts from `axiomc inspect artifacts`
- advisory diagnostics from `axiomc check`

Semantic capabilities without a `requires evidence` clause are explicitly
flagged as missing evidence. Runtime effects link back to the package evidence
records available to the generator; missing evidence remains visible as the
standard evidence model placeholder.

## Artifact Plan

`axiomc inspect artifacts <path> --json` includes `<out_dir>/runbook.md` as a
`runbook` target artifact. Its status is `planned` before generation and
`generated` after the file exists.

## Non-Goals

Runbook Target v0 does not pull live operational metrics, configure alerting,
or run incident-response automation. It is an inspectable projection of the
existing semantic and evidence surfaces.
