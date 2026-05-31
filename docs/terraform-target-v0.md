# Terraform/OpenTofu Target v0

Terraform/OpenTofu Target v0 projects declared runtime surfaces into a
provider-neutral HCL module. It is an artifact target: it does not run
`terraform`, `tofu`, providers, state backends, or cloud credentials.

## Declaration Surface

The v0 runtime surface is derived from existing stage1 semantics:

- manifest capability declarations and allowlists
- observed effects from `axiomc inspect effects`

The generator currently recognizes network, environment, process, and
filesystem-write surfaces. Network allowlists come from
`[capabilities].net.hosts` and `[capabilities].net.ports`; environment and
process surfaces come from their manifest allowlists. Observed effect records
tie those declarations back to actual stdlib/runtime calls.

## Target Contract

```json
{
  "id": "axiom://target/stage1-terraform-module-v0",
  "class": "terraform_module",
  "description": "Stage 1 OpenTofu-compatible module generator for declared runtime surfaces.",
  "status": "experimental",
  "input_node_kinds": ["Package", "Capability", "Effect", "RuntimeSurface"],
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
      "id": "axiom://package/<package>/artifact/terraform-module",
      "kind": "terraform_module",
      "path": "dist/terraform/main.tf",
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
axiomc generate terraform <path> --out dist/terraform --json
```

The command writes `main.tf` when at least one runtime surface is declared or
observed. The JSON report includes the target contract, generated artifact
record, derived runtime surfaces, a byte-change flag, and advisory diagnostics.

If no runtime surfaces are present, the command succeeds, reports a planned
artifact with a diagnostic, and does not create a module file. This keeps
"nothing to provision" as an explicit no-output state instead of an empty
module.

## Projection Rules

The generated module is provider-neutral and OpenTofu-compatible:

- it pins only the Terraform language version with `required_version`
- it records the Axiom package id and target id
- it exposes `local.axiom_runtime_surfaces`
- it exports `output "axiom_runtime_surfaces"` for downstream adapters

Provider-specific modules can consume this output later, but v0 intentionally
does not guess cloud resources from generic capability declarations.

## Boundaries

Terraform/OpenTofu Target v0 does not manage state, authenticate to providers,
declare cloud resources, or expose infrastructure that is not justified by the
package capability/effect surface.
