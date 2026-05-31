# Operator Runbook: runbook-service

## Package

- Package: `runbook-service`
- Version: `0.1.0`
- Package node: `axiom://package/runbook-service`
- Build entry: `src/main.ax`
- Target: `axiom://target/stage1-runbook-v0`
- Artifact: `dist/runbook.md`

## Capability Gates

| Capability | Enabled | Allowed Values | Unsafe | Owner | Rationale |
|---|---|---|---|---|---|
| `fs` | no | - | - | - | - |
| `fs:write` | no | - | - | - | - |
| `net` | no | - | - | - | - |
| `process` | no | - | - | - | - |
| `env` | yes | RUNBOOK_MODE | - | - | - |
| `clock` | yes | - | - | - | - |
| `crypto` | no | - | - | - | - |
| `ffi` | no | - | - | - | - |
| `async` | no | - | - | - | - |

## Semantic Capabilities

### DescribeOperatorMode

- Node: `axiom://semantic/capability/DescribeOperatorMode`
- Source: `stage1/examples/runbook_service/src/main.ax`:15:1
- Inputs: `mode: string`
- Declared effects: `read OperatorEnv`, `read RuntimeClock`
- Backing evidence: `RunbookSmokeTest`

## Observed Runtime Effects

| Effect | Operation | Resource | Capability Gate | Source | Evidence |
|---|---|---|---|---|---|
| `env.read` | `read` | RUNBOOK_MODE | `env` | `stage1/examples/runbook_service/src/main.ax`:25:24 | axiom://package/runbook-service/evidence/runbook-smoke (passing) |
| `clock.now` | `read` | * | `clock` | `stage1/examples/runbook_service/src/main.ax`:26:15 | axiom://package/runbook-service/evidence/runbook-smoke (passing) |

## Evidence

- Validation status: `passing`
- Summary: 1 passing, 0 failing, 0 missing, 1 provided

| Evidence | Type | Status | Target | Path |
|---|---|---|---|---|
| `axiom://package/runbook-service/evidence/runbook-smoke` | `unit_test` | `passing` | `axiom://package/runbook-service` | src/main_test.ax |

## Artifacts

| Kind | Status | Source | Path |
|---|---|---|---|
| `build_entry` | `generated` | `configured` | `src/main.ax` |
| `build_output` | `generated` | `available` | `dist/runbook.md` |
| `build_output_dir` | `generated` | `configured` | `dist` |
| `generated_rust` | `planned` | `configured` | `dist/runbook-service.generated.rs` |
| `lockfile` | `generated` | `configured` | `axiom.lock` |
| `manifest` | `generated` | `configured` | `axiom.toml` |
| `native_binary` | `planned` | `configured` | `dist/runbook-service` |
| `openapi_spec` | `planned` | `target_contract` | `dist/openapi.json` |
| `policy_bundle` | `planned` | `target_contract` | `dist/policy-bundle.json` |
| `runbook` | `generated` | `target_contract` | `dist/runbook.md` |
| `test_entry` | `generated` | `configured` | `src/main_test.ax` |

## Unsupported Feature Diagnostics

- None.
