#!/usr/bin/env bash
set -euo pipefail

decision_doc="docs/python-exit-vm-disposition.md"
parity_doc="docs/python-exit-parity-gate.md"
readiness_doc="docs/python-exit-deletion-readiness.json"

if [[ ! -f "$decision_doc" ]]; then
  echo "missing $decision_doc" >&2
  exit 1
fi

if [[ ! -f "$parity_doc" ]]; then
  echo "missing $parity_doc" >&2
  exit 1
fi

if [[ ! -f "$readiness_doc" ]]; then
  echo "missing $readiness_doc" >&2
  exit 1
fi

required_patterns=(
  "Parity gate: [Python Exit Parity Gate](python-exit-parity-gate.md)"
  "Python interpreter | Retire"
  "Python bytecode compiler | Retire"
  "Python bytecode format | Preserve only as historical material"
  "Python bytecode VM | Retire"
  "Python disassembler | Retire"
  "There will be no Rust port of the Python bytecode interpreter or VM"
  "Legacy module command | Disposition"
  '`check` | Use `axiomc check <package>`'
  '`compile` | Use `axiomc build <package>`'
  '`interp` | Retire'
  '`vm` | Retire with the bytecode VM'
  '`repl` | Retire'
  '`pkg init` | Use `axiomc new <path>`'
  '`pkg build` | Use `axiomc build <package>`'
  '`pkg check` | Use `axiomc check <package>`'
  '`pkg run` | Use `axiomc run <package>`'
  'package tests | Use `axiomc test <package>`'
  '`pkg clean` | Retire'
  '`pkg manifest` | Retire as a separate command'
  '`host list` | Retire'
  '`host describe` | Retire'
)

for pattern in "${required_patterns[@]}"; do
  if ! grep -Fq "$pattern" "$decision_doc"; then
    echo "missing Python exit decision text: $pattern" >&2
    exit 1
  fi
done

required_parity_patterns=(
  "Final deletion issue: [#272](https://github.com/OMT-Global/axiom/issues/272)"
  "The final Python deletion issue is blocked until"
  "| Python-facing surface | Status | Rust-only gate or disposition |"
  '| `check` | `ported` | `axiomc check <package>`'
  '| `interp` | `retired` |'
  '| `compile` | `replaced` | `axiomc build <package>`'
  '| `vm` | `retired` |'
  '| `repl` | `retired` |'
  '| `pkg init` | `replaced` | `axiomc new <path>`'
  '| `pkg build` | `ported` | `axiomc build <package>`'
  '| `pkg check` | `ported` | `axiomc check <package>`'
  '| `pkg run` | `ported` | `axiomc run <package>`'
  '| package tests | `replaced` | `axiomc test <package>`'
  '| `pkg clean` | `retired` |'
  '| `pkg manifest` | `replaced` |'
  '| `host list` | `retired` |'
  '| `host describe` | `retired` |'
  '| Python bytecode VM | `retired` |'
  '| Python host builtins namespace | `replaced` |'
  '| Python test suite | `replaced` |'
  'There are no `blocked` rows in the current matrix.'
)

for pattern in "${required_parity_patterns[@]}"; do
  if ! grep -Fq "$pattern" "$parity_doc"; then
    echo "missing Python exit parity text: $pattern" >&2
    exit 1
  fi
done

quickstart_doc="README.md"

if [[ ! -f "$quickstart_doc" ]]; then
  echo "missing $quickstart_doc" >&2
  exit 1
fi

quickstart_block="$(awk '
  /^## Quickstart$/ { in_quickstart = 1; next }
  /^## / && in_quickstart { in_quickstart = 0 }
  in_quickstart { print }
' "$quickstart_doc")"

if [[ -z "$quickstart_block" ]]; then
  echo "README quickstart is missing" >&2
  exit 1
fi

if ! grep -Fq "cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/hello --json" <<< "$quickstart_block"; then
  echo "README quickstart must use the Rust axiomc check workflow" >&2
  exit 1
fi

if ! grep -Fq "cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/hello" <<< "$quickstart_block"; then
  echo "README quickstart must use the Rust axiomc run workflow" >&2
  exit 1
fi

if grep -Eiq '(^|[^[:alpha:]])(python|stage0)([^[:alpha:]]|$)' <<< "$quickstart_block"; then
  echo "README quickstart must not route users to Python stage0" >&2
  exit 1
fi

python3 - "$readiness_doc" <<'PY'
import json, os, sys, urllib.error, urllib.request
readiness_doc = sys.argv[1]
required_issues = [266, 267, 268, 269, 270, 271]
with open(readiness_doc, encoding="utf-8") as handle:
    readiness = json.load(handle)
if readiness.get("finalDeletionIssue") != 272:
    print("Python deletion readiness checklist must name final deletion issue #272", file=sys.stderr); sys.exit(1)
items = readiness.get("blockingIssues")
if not isinstance(items, list):
    print("Python deletion readiness checklist must contain blockingIssues", file=sys.stderr); sys.exit(1)
seen = []
for item in items:
    if not isinstance(item, dict):
        print("Python deletion readiness checklist entries must be objects", file=sys.stderr); sys.exit(1)
    issue = item.get("issue"); check = item.get("check")
    if issue not in required_issues or not isinstance(check, str) or not check:
        print("Python deletion readiness checklist entries must cover #266-#271 with checks", file=sys.stderr); sys.exit(1)
    seen.append(issue)
if sorted(seen) != required_issues:
    print("Python deletion readiness checklist must cover exactly issues #266 through #271", file=sys.stderr); sys.exit(1)
states_json = os.environ.get("AXIOM_PYTHON_EXIT_ISSUE_STATES_JSON")
if states_json:
    states = {int(k): str(v).lower() for k, v in json.loads(states_json).items()}
else:
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN") or ""
    headers = {
        "Accept": "application/vnd.github+json",
        "User-Agent": "axiom-python-exit-readiness-check",
    }
    if token:
        headers["Authorization"] = f"Bearer {token}"
    states = {}
    try:
        for issue in required_issues:
            req = urllib.request.Request(
                f"https://api.github.com/repos/OMT-Global/axiom/issues/{issue}",
                headers=headers,
            )
            with urllib.request.urlopen(req, timeout=20) as response:
                item = json.load(response)
            states[issue] = str(item.get("state", "")).lower()
    except (urllib.error.URLError, TimeoutError) as exc:
        print(f"unable to verify Python deletion blocker issue states: {exc}", file=sys.stderr); sys.exit(1)
missing = [issue for issue in required_issues if issue not in states]
if missing:
    print("unable to verify Python deletion blocker issue states: missing " + ", ".join(f"#{issue}" for issue in missing), file=sys.stderr); sys.exit(1)
open_blockers = [issue for issue in required_issues if states[issue] != "closed"]
if open_blockers:
    print("Python deletion blocked by open readiness issues: " + ", ".join(f"#{issue}" for issue in open_blockers), file=sys.stderr); sys.exit(1)
PY

if awk -F '|' '
  /^## Command And Runtime Matrix/ { in_matrix = 1; next }
  /^## / && in_matrix { in_matrix = 0 }
  in_matrix && $3 ~ /`blocked`/ { found = 1 }
  END { exit found ? 0 : 1 }
' "$parity_doc"; then
  echo "Python exit parity matrix has blocked rows" >&2
  exit 1
fi
legacy_invocation="python -m axi""om"
doc_search_paths=()

for path in README.md docs scripts; do
  if [[ -e "$path" ]]; then
    doc_search_paths+=("$path")
  fi
done

if [[ "${#doc_search_paths[@]}" -gt 0 ]] && grep -RInF --include='*.md' --include='*.sh' \
  --exclude='python-exit-parity-gate.md' \
  --exclude='python-exit-vm-disposition.md' \
  "$legacy_invocation" "${doc_search_paths[@]}"; then
  echo "user-facing docs still instruct users to run $legacy_invocation" >&2
  exit 1
fi

python_unittest="python -m unit""test"
ci_search_paths=()

for path in .github scripts Makefile project.bootstrap.yaml; do
  if [[ -e "$path" ]]; then
    ci_search_paths+=("$path")
  fi
done

if [[ "${#ci_search_paths[@]}" -gt 0 ]] && grep -RInF "$python_unittest" "${ci_search_paths[@]}"; then
  echo "CI still uses Python unittest as a language/runtime correctness gate" >&2
  exit 1
fi

stage0_pathspecs=(
  ':(glob)axiom/**'
  ':(glob)tests/**'
  ':(glob)requirements*.in'
  ':(glob)requirements*.txt'
  '.python-version'
  'Pipfile'
  'Pipfile.lock'
  'poetry.lock'
  'pyproject.toml'
  'setup.cfg'
  'setup.py'
  'tox.ini'
)

tracked_stage0_files="$(git ls-files -- "${stage0_pathspecs[@]}")"

if [[ -n "$tracked_stage0_files" ]]; then
  echo "Python stage0 source, tests, or packaging files are still tracked" >&2
  printf '%s\n' "$tracked_stage0_files" >&2
  exit 1
fi
