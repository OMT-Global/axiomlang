#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
workflow="$repo_root/.github/workflows/extended-validation.yml"
fixture="$repo_root/scripts/ci/fixtures/extended-validation-routing.json"

python3 - "$workflow" "$fixture" <<'PY'
import fnmatch
import json
import pathlib
import re
import sys

workflow_path = pathlib.Path(sys.argv[1])
fixture_path = pathlib.Path(sys.argv[2])
workflow = workflow_path.read_text(encoding="utf-8")
fixture = json.loads(fixture_path.read_text(encoding="utf-8"))
errors = []

if fixture.get("schema_version") != "axiom.ci.extended_validation_routing_fixture.v1":
    errors.append("routing fixture has an unsupported schema_version")

if not re.search(r"^  push:\n    branches: \[main\]$", workflow, re.MULTILINE):
    errors.append("extended validation must select pushes to main")
if not re.search(r"^  schedule:\n    - cron: '[^']+'$", workflow, re.MULTILINE):
    errors.append("extended validation must retain a nightly schedule")
if not re.search(r"^  workflow_dispatch:$", workflow, re.MULTILINE):
    errors.append("extended validation must remain manually dispatchable")

extended_match = re.search(
    r"^            extended:\n(?P<body>(?:^              - .+\n)+)",
    workflow,
    re.MULTILINE,
)
if extended_match is None:
    errors.append("extended path filter is missing or malformed")
    patterns = []
else:
    patterns = re.findall(r"^              - '([^']+)'$", extended_match.group("body"), re.MULTILINE)

def matches(path: str) -> bool:
    return any(fnmatch.fnmatchcase(path, pattern) for pattern in patterns)

for case in fixture.get("cases", []):
    path = case["path"]
    expected = case["extended"]
    actual = matches(path)
    if actual != expected:
        errors.append(
            f"routing mismatch for {path}: expected extended={expected}, got {actual}; "
            f"reason: {case['reason']}"
        )

if not matches("Makefile"):
    errors.append("Makefile changes must route to extended qualification")

preset_match = re.search(
    r"- name: Run full suite for nightly or manual invocations\n"
    r"(?P<body>.*?)(?=\n      - (?:name:|uses:))",
    workflow,
    re.DOTALL,
)
if preset_match is None:
    errors.append("nightly/manual full-suite preset is missing")
else:
    preset = preset_match.group("body")
    if "if: github.event_name != 'push'" not in preset:
        errors.append("nightly/manual preset must select every non-push invocation")
    for output in ("app=true", "ci=true", "extended=true"):
        if output not in preset:
            errors.append(f"nightly/manual preset must emit {output}")

jobs_section = workflow.split("\njobs:\n", 1)
if len(jobs_section) != 2:
    errors.append("workflow jobs section is missing")
    jobs = []
else:
    jobs = re.findall(
        r"^  ([a-z][a-z0-9-]+):\n(?P<body>.*?)(?=^  [a-z][a-z0-9-]+:|\Z)",
        jobs_section[1],
        re.MULTILINE | re.DOTALL,
    )
expected_runner = "runs-on: ubuntu-24.04"
for job_name, body in jobs:
    if expected_runner not in body:
        errors.append(f"job {job_name} must remain on the standard ubuntu-24.04 hosted runner")

fast_job = next((body for name, body in jobs if name == "fast-checks"), "")
rust_setup = "uses: dtolnay/rust-toolchain@29eef336d9b2848a0b548edc03f92a220660cdb8"
fast_entrypoint = "run: bash scripts/ci/run-fast-checks.sh"
if rust_setup not in fast_job or fast_entrypoint not in fast_job:
    errors.append("fast-checks must provision Rust before running Cargo-dependent checks")
elif fast_job.index(rust_setup) > fast_job.index(fast_entrypoint):
    errors.append("fast-checks Rust setup must precede the fast-check entrypoint")

extended_job = next((body for name, body in jobs if name == "extended-checks"), "")
if "needs.changes.outputs.extended == 'true'" not in extended_job:
    errors.append("extended-checks must consume the extended selection output")
if "bash scripts/ci/run-extended-validation.sh" not in extended_job:
    errors.append("extended-checks must invoke the extended validation entrypoint")
if "- name: Summarize qualification evidence" not in extended_job:
    errors.append("extended-checks must summarize qualification evidence")
if "scripts/ci/report-toolchain-qualification.py" not in extended_job:
    errors.append("extended-checks must invoke the metadata-only qualification reporter")
if "--expected-head-sha '${{ github.sha }}'" not in extended_job:
    errors.append("extended-checks must bind the qualification summary to the workflow head")
summary_marker = "- name: Summarize qualification evidence"
qualification_upload_marker = "- name: Upload qualification evidence"
summary_index = extended_job.find(summary_marker)
upload_index = extended_job.find(qualification_upload_marker)
if summary_index < 0 or upload_index < 0 or summary_index > upload_index:
    errors.append("extended-checks must summarize qualification evidence before uploading artifacts")
if "- name: Collect readiness reports" not in extended_job:
    errors.append("extended-checks must collect readiness reports")
if "run_report rust-exit-readiness" not in extended_job:
    errors.append("extended-checks must execute the Rust-exit readiness checker")
if "run_report self-hosting-language-readiness" not in extended_job:
    errors.append("extended-checks must execute the self-hosting readiness checker")
if "run_report snapshot-bootstrap-readiness" not in extended_job:
    errors.append("extended-checks must execute the snapshot-bootstrap readiness checker")
if "- name: Summarize qualification evidence" not in extended_job:
    errors.append("extended-checks must summarize qualification evidence")
if "scripts/ci/report-toolchain-qualification.py" not in extended_job:
    errors.append("extended-checks must invoke the metadata-only qualification reporter")
if "--expected-head-sha '${{ github.sha }}'" not in extended_job:
    errors.append("extended-checks must bind the qualification summary to the workflow head")
summary_marker = "- name: Summarize qualification evidence"
qualification_upload_marker = "- name: Upload qualification evidence"
summary_index = extended_job.find(summary_marker)
upload_index = extended_job.find(qualification_upload_marker)
if summary_index < 0 or upload_index < 0 or summary_index > upload_index:
    errors.append("extended-checks must summarize qualification evidence before uploading artifacts")
if "- name: Upload readiness reports" not in extended_job or "path: artifacts/readiness" not in extended_job:
    errors.append("extended-checks must upload readiness reports even after failures")

job_preamble = extended_job.split("\n    steps:\n", 1)[0]
if re.search(r"\$\{\{\s*runner\s*(?:\.|\[)", job_preamble):
    errors.append("extended-checks must not use the runner context before step execution")
if "fetch-depth: 0" not in extended_job:
    errors.append("extended-checks must fetch full history for quality baseline ancestry")
if (
    "AXIOM_QUALIFICATION_BASE_SHA: "
    "${{ github.event_name == 'push' && github.event.before || '' }}"
    not in extended_job
):
    errors.append(
        "extended-checks must bind push qualification to github.event.before "
        "and leave other triggers unbased"
    )
if "components: llvm-tools-preview" not in extended_job:
    errors.append("extended-checks must provision llvm-tools-preview")
if "actions/setup-go@924ae3a1cded613372ab5595356fb5720e22ba16" not in extended_job:
    errors.append("extended-checks must provision Go with the pinned setup action")
if "go-version: ${{ env.GO_VERSION }}" not in extended_job:
    errors.append("extended-checks must use the repository-pinned Go version")
if not re.search(r"^  GO_VERSION: '1\.26\.5'$", workflow, re.MULTILINE):
    errors.append("extended validation must pin Go 1.26.5")
if 'required_version="0.8.5"' not in extended_job:
    errors.append("extended-checks must pin cargo-llvm-cov 0.8.5")
if 'cargo install cargo-llvm-cov --version "$required_version" --locked --force' not in extended_job:
    errors.append("extended-checks must repair a missing or mismatched cargo-llvm-cov")
for exact_head_fragment in (
    "--head-sha '${{ github.sha }}'",
    "--target '${{ runner.os }}-${{ runner.arch }}'",
    "--trigger '${{ github.event_name }}'",
):
    if exact_head_fragment not in extended_job:
        errors.append(f"extended-checks must pass exact qualification provenance: {exact_head_fragment}")
if "if: always()" not in extended_job or "actions/upload-artifact@" not in extended_job:
    errors.append("extended-checks must upload qualification evidence even after failures")
if "timeout-minutes: 120" not in extended_job:
    errors.append("extended-checks must allow the complete product qualification suite to finish")

# Compiler setup is needed before any native reference or coverage-tool build.
for name, body, prerequisite in (
    ("fast-checks", fast_job, "      - name: Run fast checks"),
    ("extended-checks", extended_job, "      - name: Ensure pinned coverage tooling"),
):
    setup = re.search(r"^      - name: Ensure C compiler availability\n(?P<body>.*?)(?=^      - |\Z)", body, re.MULTILINE | re.DOTALL)
    if setup is None or prerequisite not in body or setup.start() >= body.index(prerequisite):
        errors.append(f"{name} must provision a C compiler before native work")
    elif 'install -y --no-install-recommends gcc libc6-dev' not in setup.group("body") or 'exit 1' not in setup.group("body"):
        errors.append(f"{name} compiler provisioning must retain installation and fail-closed checks")

# Required qualification must have the same pinned supply-chain tool available.
vet_setup = re.search(r"^      - name: Ensure cargo-vet\n(?P<body>.*?)(?=^      - |\Z)", extended_job, re.MULTILINE | re.DOTALL)
if vet_setup is None or vet_setup.start() >= extended_job.index("      - name: Run extended validation"):
    errors.append("extended-checks must provision cargo-vet before qualification")
else:
    source = (workflow_path.parent / "toolchain-supply-chain.yml").read_text(encoding="utf-8")
    canonical = re.search(r"^      - name: Ensure cargo-vet\n(?P<body>.*?)(?=^      - |\Z)", source, re.MULTILINE | re.DOTALL)
    if canonical is None or canonical.group("body").strip() != vet_setup.group("body").strip():
        errors.append("extended-checks cargo-vet setup must match pinned supply-chain provisioning")

target_job = next((body for name, body in jobs if name == "target-support-evidence"), "")
if not target_job:
    errors.append("target-support-evidence job is missing")
for fragment in (
    "name: Target Support Evidence (linux-x86-64)",
    "runs-on: ubuntu-24.04",
    "uses: dtolnay/rust-toolchain@29eef336d9b2848a0b548edc03f92a220660cdb8",
    "cargo fetch --locked --manifest-path stage1/Cargo.toml",
    "python3 scripts/ci/run-target-support-evidence-v1.py run",
    "--expected-target 'x86_64-unknown-linux-gnu'",
    "--head-sha '${{ github.event.inputs.evidence_pr_sha || github.sha }}'",
    "--trigger '${{ github.event_name }}'",
    "--runner-labels-json '[\"ubuntu-24.04\"]'",
    "--output 'artifacts/target-support/linux-x86-64.json'",
    "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
    "ref: ${{ github.event.inputs.evidence_pr_sha || github.sha }}",
):
    if fragment not in target_job:
        errors.append(f"target-support-evidence is missing required contract: {fragment}")
if "needs.changes.outputs.extended == 'true'" not in target_job:
    errors.append("target-support-evidence must consume the extended selection output")
if "github.repository == 'OMT-Global/axiomlang'" not in target_job:
    errors.append("target-support-evidence must be bound to the governed repository")
if "github.ref == 'refs/heads/main'" not in target_job:
    errors.append("continuous target evidence must remain bound to the protected main ref")
if "github.event_name == 'workflow_dispatch'" not in target_job:
    errors.append("pre-merge target evidence must require an explicit manual dispatch")
if "github.event.inputs.evidence_pr_sha != ''" not in target_job:
    errors.append("manual dispatches must pin an exact evidence_pr_sha to produce pre-merge evidence")
if "^[0-9a-f]{40}$" not in target_job:
    errors.append("evidence_pr_sha validation must fail closed on malformed input")
if "if: always()" not in target_job:
    errors.append("target-support-evidence must upload partial evidence after failures")
validate_marker = "- name: Validate approval-gated evidence dispatch input"
checkout_marker = "uses: actions/checkout@"
produce_marker = "- name: Produce exact-head target evidence"
upload_marker = "- name: Upload target support evidence"
if (
    validate_marker not in target_job
    or checkout_marker not in target_job
    or produce_marker not in target_job
    or upload_marker not in target_job
):
    errors.append("target-support-evidence must validate the dispatch input, check out the exact head, produce evidence, and upload artifacts")
elif not (
    target_job.index(validate_marker)
    < target_job.index(checkout_marker)
    < target_job.index(produce_marker)
    < target_job.index(upload_marker)
):
    errors.append("target-support-evidence steps must run in validation, checkout, evidence, upload order")
if "RUST_VERSION" in workflow:
    errors.append("extended validation must not reintroduce a stale RUST_VERSION pin below the locked dependency MSRV")
if "evidence_pr_sha:" not in workflow:
    errors.append("workflow_dispatch must declare the evidence_pr_sha input")
# Fail-closed runner policy: hosted ubuntu-24.04 only. macOS arm64 evidence
# comes from the specialized node lane documented in docs/target-support-v1.md,
# never from a CI job or a self-hosted pool.
if "self-hosted" in workflow:
    errors.append("extended validation must not schedule self-hosted runners; ordinary CI stays on hosted ubuntu-24.04")
for forbidden in ("aarch64-apple-darwin", "macos-arm64", "matrix.runner", "fromJSON(matrix"):
    if forbidden in workflow:
        errors.append(f"macOS target evidence must stay out of CI: forbidden fragment {forbidden}")

gate_job = next((body for name, body in jobs if name == "extended-validation-gate"), "")
if "- target-support-evidence" not in gate_job:
    errors.append("extended validation gate must depend on target-support-evidence")
if "target-support-evidence=${{ needs.target-support-evidence.result }}" not in gate_job:
    errors.append("extended validation gate must inspect target-support-evidence")

if errors:
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    raise SystemExit(1)
PY

echo "extended-validation workflow routing contract passed"
