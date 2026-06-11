#!/usr/bin/env bash
set -euo pipefail

mode="text"
issue_state_file="${AXIOM_RUST_EXIT_ISSUE_STATES_FILE:-}"
require_issue_states=false

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --json)
      mode="json"
      shift
      ;;
    --issue-state-file)
      if [[ -z "${2:-}" ]]; then
        echo "missing value for --issue-state-file" >&2
        exit 2
      fi
      issue_state_file="$2"
      shift 2
      ;;
    --require-issue-states)
      require_issue_states=true
      shift
      ;;
    *)
      echo "usage: $0 [--json] [--issue-state-file <path>] [--require-issue-states]" >&2
      exit 2
      ;;
  esac
done

checks=()
failures=()

json_escape() {
  python3 -c 'import json, sys; print(json.dumps(sys.stdin.read()))'
}

add_check() {
  local name="$1"
  local status="$2"
  local detail="$3"

  checks+=("$name|$status|$detail")

  if [[ "$status" != "pass" ]]; then
    failures+=("$name: $detail")
  fi
}

has_make_target() {
  local target="$1"
  grep -Eq "^${target}[[:space:]]*:" Makefile
}

issue_state_from_file() {
  local issue="$1"
  local file="$2"
  awk -v issue="$issue" '
    $1 == issue { print toupper($2); found = 1; exit }
    END { exit found ? 0 : 1 }
  ' "$file"
}

issue_state_from_github() {
  local issue="$1"
  gh issue view "$issue" --json state --jq '.state' 2>/dev/null | tr '[:lower:]' '[:upper:]'
}

read_issue_state() {
  local issue="$1"
  if [[ -n "$issue_state_file" ]]; then
    issue_state_from_file "$issue" "$issue_state_file"
    return
  fi
  if command -v gh >/dev/null 2>&1; then
    issue_state_from_github "$issue"
    return
  fi
  return 1
}

matrix_has_blocked_rows() {
  awk -F '|' '
    /^## Backend Matrix/ || /^## Bootstrap Matrix/ { in_matrix = 1; next }
    /^## / && in_matrix { in_matrix = 0 }
    in_matrix && $4 ~ /`blocked`/ { found = 1 }
    END { exit found ? 0 : 1 }
  ' docs/rust-exit-readiness.md
}

blocking_issues_from_manifest() {
  python3 - <<'PY'
import json

with open("docs/rust-exit-readiness.json", encoding="utf-8") as handle:
    payload = json.load(handle)

for entry in payload.get("blockingIssues", []):
    print(entry["issue"])
PY
}

if [[ -f docs/rust-exit-readiness.md ]]; then
  add_check "readiness_doc_present" "pass" "docs/rust-exit-readiness.md exists"
else
  add_check "readiness_doc_present" "fail" "docs/rust-exit-readiness.md is missing"
fi

if [[ -f docs/rust-exit-readiness.json ]]; then
  add_check "readiness_manifest_present" "pass" "docs/rust-exit-readiness.json exists"
else
  add_check "readiness_manifest_present" "fail" "docs/rust-exit-readiness.json is missing"
fi

if [[ ! -f docs/rust-exit-readiness.md ]]; then
  add_check "readiness_matrix_unblocked" "fail" "Rust exit readiness matrix is unavailable because docs/rust-exit-readiness.md is missing"
elif matrix_has_blocked_rows; then
  add_check "readiness_matrix_unblocked" "fail" "Rust exit readiness matrix still contains blocked rows"
else
  add_check "readiness_matrix_unblocked" "pass" "Rust exit readiness matrix has no blocked rows"
fi

if [[ -f docs/rust-exit-readiness.json ]]; then
  python3 - <<'PY'
import json
import sys

with open("docs/rust-exit-readiness.json", encoding="utf-8") as handle:
    payload = json.load(handle)

if payload.get("schemaVersion") != 1:
    print("schemaVersion must be 1", file=sys.stderr)
    sys.exit(1)
if payload.get("finalBootstrapIssue") != 721:
    print("finalBootstrapIssue must be 721", file=sys.stderr)
    sys.exit(1)
issues = [entry.get("issue") for entry in payload.get("blockingIssues", [])]
required = {562, 563, 564, 693, 694, 927, 928, 929, 930, 931}
missing = sorted(required - set(issues))
if missing:
    print("missing required blocking issues: " + ", ".join(f"#{issue}" for issue in missing), file=sys.stderr)
    sys.exit(1)
if len(set(issues)) != len(issues):
    print("blocking issue list contains duplicates", file=sys.stderr)
    sys.exit(1)
PY
  add_check "readiness_manifest_valid" "pass" "docs/rust-exit-readiness.json has the required schema and blockers"
else
  add_check "readiness_manifest_valid" "fail" "docs/rust-exit-readiness.json cannot be validated"
fi

for target in rust-exit-readiness rust-exit-readiness-github rust-exit-readiness-test; do
  if has_make_target "$target"; then
    add_check "make_${target}" "pass" "Makefile exposes $target"
  else
    add_check "make_${target}" "fail" "Makefile does not expose $target"
  fi
done

if [[ -n "$issue_state_file" && ! -f "$issue_state_file" ]]; then
  add_check "rust_exit_issue_state_source" "fail" "issue state file does not exist: $issue_state_file"
elif [[ -n "$issue_state_file" ]]; then
  add_check "rust_exit_issue_state_source" "pass" "issue states loaded from $issue_state_file"
elif command -v gh >/dev/null 2>&1; then
  add_check "rust_exit_issue_state_source" "pass" "issue states loaded from GitHub"
elif [[ "$require_issue_states" == true ]]; then
  add_check "rust_exit_issue_state_source" "fail" "issue states are required but no --issue-state-file was provided and gh is unavailable"
else
  add_check "rust_exit_issue_state_source" "pass" "issue states not checked; pass --require-issue-states for deletion PRs"
fi

if [[ -f docs/rust-exit-readiness.json ]]; then
  while IFS= read -r issue; do
    issue_state=""
    if issue_state="$(read_issue_state "$issue")" && [[ -n "$issue_state" ]]; then
      if [[ "$issue_state" == "CLOSED" ]]; then
        add_check "rust_exit_issue_${issue}_closed" "pass" "issue #$issue is CLOSED"
      else
        add_check "rust_exit_issue_${issue}_closed" "fail" "issue #$issue is $issue_state"
      fi
    elif [[ "$require_issue_states" == true ]]; then
      add_check "rust_exit_issue_${issue}_closed" "fail" "issue #$issue state is unavailable"
    fi
  done < <(blocking_issues_from_manifest)
fi

if [[ "$mode" == "json" ]]; then
  printf '{\n'
  printf '  "schema": "axiom.rust_exit.readiness.v1",\n'
  printf '  "ready": %s,\n' "$(if [[ "${#failures[@]}" -eq 0 ]]; then echo true; else echo false; fi)"
  printf '  "checks": [\n'
  for index in "${!checks[@]}"; do
    IFS='|' read -r name status detail <<< "${checks[$index]}"
    comma=","
    if [[ "$index" -eq $((${#checks[@]} - 1)) ]]; then
      comma=""
    fi
    escaped_detail="$(printf '%s' "$detail" | json_escape)"
    printf '    {"name":"%s","status":"%s","detail":%s}%s\n' "$name" "$status" "$escaped_detail" "$comma"
  done
  printf '  ]\n'
  printf '}\n'
else
  if [[ "${#failures[@]}" -eq 0 ]]; then
    echo "Rust exit readiness: ready"
  else
    echo "Rust exit readiness: blocked" >&2
  fi

  for check in "${checks[@]}"; do
    IFS='|' read -r name status detail <<< "$check"
    printf '%s %-36s %s\n' "$status" "$name" "$detail"
  done
fi

if [[ "${#failures[@]}" -gt 0 ]]; then
  exit 1
fi
