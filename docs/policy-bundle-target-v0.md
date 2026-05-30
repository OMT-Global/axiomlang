# Policy Bundle Target v0

Policy Bundle Target v0 projects the package capability surface and effect
graph into a deterministic JSON allowlist. The bundle is an artifact for
downstream gates such as CI policy checks, runtime allowlists, or OPA adapters;
it does not enforce policy by itself and does not change compile-time
capability checks.

## Target Contract

```json
{
  "id": "axiom://target/stage1-policy-bundle-v0",
  "class": "policy_bundle",
  "description": "Stage 1 policy bundle generator for manifest capabilities and effect allowlists.",
  "status": "experimental",
  "input_node_kinds": ["Package", "Capability", "Effect"],
  "supported_effect_kinds": [
    "clock.now",
    "clock.sleep",
    "crypto.hash",
    "crypto.mac",
    "env.read",
    "fs.read",
    "fs.write",
    "network.dns.resolve",
    "network.http.get",
    "network.tcp.bind",
    "network.tcp.connect",
    "network.udp.send",
    "process.status"
  ],
  "supported_type_features": [],
  "artifact_outputs": [
    {
      "id": "axiom://package/<package>/artifact/policy-bundle",
      "kind": "policy_bundle",
      "path": "dist/policy-bundle.json",
      "generated_from": ["axiom://package/<package>"],
      "status": "planned"
    }
  ],
  "evidence_requirements": ["unit_test", "fixture"],
  "unsupported_feature_diagnostics": []
}
```

## Command

```bash
axiomc generate policy <path> --out dist/policy-bundle.json --json
```

Relative `--out` paths are resolved inside the package path. The command emits
a JSON report with the target contract, generated artifact record,
`allowed_effect_kinds`, and observed effect records.

## Projection Rules

The allowlist is derived from manifest capabilities:

- `clock` permits `clock.now` and `clock.sleep`.
- `env` permits `env.read`.
- `fs` permits `fs.read`.
- `fs:write` permits `fs.write`.
- `net` permits `network.dns.resolve`, `network.http.get`,
  `network.tcp.bind`, `network.tcp.connect`, and `network.udp.send`.
- `process` permits `process.status`.
- `crypto` permits `crypto.hash` and `crypto.mac`.

Capabilities with no effect kind in the current effect model, such as `ffi` and
`async`, remain represented in the `capabilities` section but do not add
effect allowlist entries.

Observed effects come from `axiomc inspect effects`. They let downstream policy
checks compare what a package uses against what the manifest permits without
changing `axiomc caps` or compiler enforcement.

## Drift Detection

The bundle is byte-deterministic. Removing a capability from `axiom.toml`
removes its effect kinds from `allowed_effect_kinds` when the bundle is
regenerated, so policy drift is visible in a normal artifact diff.

## Artifact Plan

`axiomc inspect artifacts <path> --json` reports
`<out_dir>/policy-bundle.json` as a `policy_bundle` target artifact. Its status
is `planned` before generation and `generated` once the file exists.

## Boundaries

Policy Bundle Target v0 emits a neutral JSON allowlist only. It does not add an
OPA/Rego renderer, runtime policy enforcement, new capability flags, or
signed/attested bundles.
