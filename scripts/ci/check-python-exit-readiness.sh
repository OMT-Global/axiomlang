#!/usr/bin/env bash
set -euo pipefail

mode="text"
issue_state_file="${AXIOM_PYTHON_EXIT_ISSUE_STATES_FILE:-}"
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
    /^## Command And Runtime Matrix/ { in_matrix = 1; next }
    /^## / && in_matrix { in_matrix = 0 }
    in_matrix && $3 ~ /`blocked`/ { found = 1 }
    END { exit found ? 0 : 1 }
  ' docs/python-exit-parity-gate.md
}

legacy_invocation="python -m axi""om"
python_unittest="python -m unit""test"

if [[ -f docs/python-exit-parity-gate.md ]]; then
  add_check "parity_doc_present" "pass" "docs/python-exit-parity-gate.md exists"
else
  add_check "parity_doc_present" "fail" "docs/python-exit-parity-gate.md is missing"
fi

if [[ -f docs/python-exit-vm-disposition.md ]]; then
  add_check "vm_disposition_present" "pass" "docs/python-exit-vm-disposition.md exists"
else
  add_check "vm_disposition_present" "fail" "docs/python-exit-vm-disposition.md is missing"
fi

if [[ ! -f docs/python-exit-parity-gate.md ]]; then
  add_check "parity_matrix_unblocked" "fail" "Python exit parity matrix is unavailable because docs/python-exit-parity-gate.md is missing"
elif matrix_has_blocked_rows; then
  add_check "parity_matrix_unblocked" "fail" "Python exit parity matrix still contains blocked rows"
else
  add_check "parity_matrix_unblocked" "pass" "Python exit parity matrix has no blocked rows"
fi

doc_search_paths=()
for path in README.md docs scripts; do
  if [[ -e "$path" ]]; then
    doc_search_paths+=("$path")
  fi
done

if [[ "${#doc_search_paths[@]}" -gt 0 ]] && rg -n "$legacy_invocation" "${doc_search_paths[@]}" \
  --glob '*.md' \
  --glob '*.sh' \
  --glob '!docs/python-exit-parity-gate.md' \
  --glob '!docs/python-exit-vm-disposition.md' >/dev/null; then
  add_check "no_user_facing_python_cli" "fail" "user-facing docs still instruct users to run $legacy_invocation"
else
  add_check "no_user_facing_python_cli" "pass" "user-facing docs do not instruct users to run $legacy_invocation"
fi

ci_search_paths=()
for path in .github scripts Makefile project.bootstrap.yaml; do
  if [[ -e "$path" ]]; then
    ci_search_paths+=("$path")
  fi
done

if [[ "${#ci_search_paths[@]}" -gt 0 ]] && rg -n --hidden "$python_unittest" "${ci_search_paths[@]}" >/dev/null; then
  add_check "no_python_unittest_ci_gate" "fail" "CI still uses Python unittest as a language/runtime correctness gate"
else
  add_check "no_python_unittest_ci_gate" "pass" "CI does not use Python unittest as a language/runtime correctness gate"
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
  add_check "no_tracked_stage0_files" "fail" "Python stage0 source, tests, or packaging files are still tracked"
else
  add_check "no_tracked_stage0_files" "pass" "no Python stage0 source, tests, or packaging files are tracked"
fi

if [[ -f stage1/Cargo.toml ]]; then
  add_check "stage1_manifest_present" "pass" "stage1/Cargo.toml exists"
else
  add_check "stage1_manifest_present" "fail" "stage1/Cargo.toml is missing"
fi

for target in stage1-test stage1-conformance stage1-smoke docs-python-exit; do
  if has_make_target "$target"; then
    add_check "make_${target}" "pass" "Makefile exposes $target"
  else
    add_check "make_${target}" "fail" "Makefile does not expose $target"
  fi
done

if [[ -n "$issue_state_file" && ! -f "$issue_state_file" ]]; then
  add_check "python_exit_issue_state_source" "fail" "issue state file does not exist: $issue_state_file"
elif [[ -n "$issue_state_file" ]]; then
  add_check "python_exit_issue_state_source" "pass" "issue states loaded from $issue_state_file"
elif command -v gh >/dev/null 2>&1; then
  add_check "python_exit_issue_state_source" "pass" "issue states loaded from GitHub"
elif [[ "$require_issue_states" == true ]]; then
  add_check "python_exit_issue_state_source" "fail" "issue states are required but no --issue-state-file was provided and gh is unavailable"
else
  add_check "python_exit_issue_state_source" "pass" "issue states not checked; pass --require-issue-states for deletion PRs"
fi

for issue in 266 267 268 269 270 271; do
  issue_state=""
  if issue_state="$(read_issue_state "$issue")" && [[ -n "$issue_state" ]]; then
    if [[ "$issue_state" == "CLOSED" ]]; then
      add_check "python_exit_issue_${issue}_closed" "pass" "issue #$issue is CLOSED"
    else
      add_check "python_exit_issue_${issue}_closed" "fail" "issue #$issue is $issue_state"
    fi
  elif [[ "$require_issue_states" == true ]]; then
    add_check "python_exit_issue_${issue}_closed" "fail" "issue #$issue state is unavailable"
  fi
done

if [[ "$mode" == "json" ]]; then
  printf '{\n'
  printf '  "schema": "axiom.python_exit.readiness.v1",\n'
  printf '  "ready": %s,\n' "$(if [[ "${#failures[@]}" -eq 0 ]]; then echo true; else echo false; fi)"
  printf '  "checks": [\n'
  for index in "${!checks[@]}"; do
    IFS='|' read -r name status detail <<< "${checks[$index]}"
    comma=","
    if [[ "$index" -eq $((${#checks[@]} - 1)) ]]; then
      comma=""
    fi
    printf '    {"name":"%s","status":"%s","detail":"%s"}%s\n' "$name" "$status" "$detail" "$comma"
  done
  printf '  ]\n'
  printf '}\n'
else
  if [[ "${#failures[@]}" -eq 0 ]]; then
    echo "Python exit readiness: ready"
  else
    echo "Python exit readiness: blocked" >&2
  fi

  for check in "${checks[@]}"; do
    IFS='|' read -r name status detail <<< "$check"
    printf '%s %-36s %s\n' "$status" "$name" "$detail"
  done
fi

if [[ "${#failures[@]}" -gt 0 ]]; then
  exit 1
fi
