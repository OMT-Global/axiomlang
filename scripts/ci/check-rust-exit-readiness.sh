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

blocking_issues_from_manifest() {
  python3 - <<'PY'
import json

with open("docs/rust-exit-readiness.json", encoding="utf-8") as handle:
    payload = json.load(handle)

for entry in payload.get("blockingIssues", []):
    print(entry["issue"])
PY
}

all_blocking_issues_closed() {
  local issue_state
  local issue

  if [[ ! -f docs/rust-exit-readiness.json ]]; then
    return 1
  fi

  while IFS= read -r issue; do
    issue_state=""
    if ! issue_state="$(read_issue_state "$issue")" || [[ -z "$issue_state" ]]; then
      return 1
    fi
    if [[ "$issue_state" != "CLOSED" ]]; then
      return 1
    fi
  done < <(blocking_issues_from_manifest)
}

direct_native_runtime_abi_report() {
  if [[ ! -f scripts/ci/check-direct-native-runtime-abi.py ]]; then
    return 1
  fi
  python3 scripts/ci/check-direct-native-runtime-abi.py --json
}

direct_native_runtime_abi_ready() {
  python3 -c 'import json, sys; payload = json.load(sys.stdin); sys.exit(0 if payload.get("ready") is True else 1)'
}

direct_native_runtime_abi_detail() {
  python3 -c 'import json, sys; payload = json.load(sys.stdin); incomplete = payload.get("incomplete_rows", []); blocked = payload.get("blocked_rows", []); errors = payload.get("errors", []); issues = ", ".join("#%s" % issue for issue in payload.get("blocker_issues", [])); print("errors: " + ", ".join(map(str, errors)) if errors else ("%d incomplete rows, %d blocked rows; blocker issues: %s" % (len(incomplete), len(blocked), issues) if incomplete or blocked else "contract status: %s" % payload.get("contract_status")))'
}

self_hosted_boundary_report() {
  python3 - <<'PY'
import json
from pathlib import Path

checks = []

command_lsp = Path("stage1/compiler-contracts/snapshots/command-lsp.json")
if not command_lsp.is_file():
    checks.append(("command_lsp_release_boundary", "fail", "command/LSP boundary snapshot is missing"))
else:
    payload = json.loads(command_lsp.read_text(encoding="utf-8"))
    release = payload.get("official_release", {})
    failures = []
    if release.get("requires_cargo") is not False:
        failures.append("requires_cargo must be false")
    if release.get("requires_rustc") is not False:
        failures.append("requires_rustc must be false")
    if release.get("temporary_developer_path") is not True:
        failures.append("temporary_developer_path must remain explicit while Rust host exists")
    if failures:
        checks.append(("command_lsp_release_boundary", "fail", "; ".join(failures)))
    else:
        checks.append(("command_lsp_release_boundary", "pass", "official command/LSP boundary excludes Cargo and rustc"))

mir_backend = Path("stage1/compiler-contracts/snapshots/mir-backend.json")
if not mir_backend.is_file():
    checks.append(("mir_backend_direct_native_boundary", "fail", "MIR/backend boundary snapshot is missing"))
else:
    payload = json.loads(mir_backend.read_text(encoding="utf-8"))
    targets = {target.get("id"): target for target in payload.get("targets", [])}
    native = targets.get("axiom://target/stage1-direct-native")
    failures = []
    if not native:
        failures.append("direct-native target is missing")
    else:
        forbidden = {"rust_source", "generated_rust", "cargo_metadata", "rustc_output"}
        if set(native.get("must_not_require", [])) != forbidden:
            failures.append("direct-native must_not_require set changed")
        if "rust_source" in native.get("primary_artifacts", []):
            failures.append("direct-native primary artifacts must not include rust_source")
        if "rust_source" in native.get("required_evidence", []):
            failures.append("direct-native evidence must not require rust_source")
        if "runtime_abi" not in native.get("required_evidence", []):
            failures.append("direct-native evidence must include runtime_abi")
    if failures:
        checks.append(("mir_backend_direct_native_boundary", "fail", "; ".join(failures)))
    else:
        checks.append(("mir_backend_direct_native_boundary", "pass", "direct-native target contract excludes generated Rust, Cargo, and rustc"))

for name, status, detail in checks:
    print(f"{name}|{status}|{detail}")
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

if [[ ! -f docs/rust-exit-readiness.json ]]; then
  add_check "readiness_blockers_closed" "fail" "Rust exit readiness manifest is unavailable"
elif all_blocking_issues_closed; then
  add_check "readiness_blockers_closed" "pass" "All blocking issues listed in docs/rust-exit-readiness.json are CLOSED"
else
  add_check "readiness_blockers_closed" "fail" "One or more blocking issues listed in docs/rust-exit-readiness.json are not CLOSED"
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
required = {562, 563, 564, 693, 694, 927, 929, 930, 931}
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

abi_report=""
if abi_report="$(direct_native_runtime_abi_report 2>/dev/null)" && [[ -n "$abi_report" ]]; then
  if printf '%s' "$abi_report" | direct_native_runtime_abi_ready; then
    add_check "direct_native_runtime_abi_ready" "pass" "Direct native runtime ABI reports ready"
  else
    abi_detail="$(printf '%s' "$abi_report" | direct_native_runtime_abi_detail)"
    add_check "direct_native_runtime_abi_ready" "fail" "Direct native runtime ABI is not ready: $abi_detail"
  fi
else
  add_check "direct_native_runtime_abi_ready" "fail" "Direct native runtime ABI report is unavailable"
fi

while IFS='|' read -r boundary_name boundary_status boundary_detail; do
  add_check "$boundary_name" "$boundary_status" "$boundary_detail"
done < <(self_hosted_boundary_report)

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
