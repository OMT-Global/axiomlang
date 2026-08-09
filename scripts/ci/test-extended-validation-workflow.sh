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
expected_runner = "runs-on: ['self-hosted', 'linux', 'shell-only', 'public']"
for job_name, body in jobs:
    if expected_runner not in body:
        errors.append(f"job {job_name} must remain on the shell-safe public runner pool")

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

if errors:
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    raise SystemExit(1)
PY

echo "extended-validation workflow routing contract passed"
