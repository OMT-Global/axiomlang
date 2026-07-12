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
for exact_head_fragment in ("--head-sha '${{ github.sha }}'", "--trigger '${{ github.event_name }}'"):
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
