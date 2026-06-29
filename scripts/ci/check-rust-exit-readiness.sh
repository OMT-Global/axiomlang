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
  python3 -c 'import json, sys; payload = json.load(sys.stdin); incomplete = payload.get("incomplete_rows", []); blocked = payload.get("blocked_rows", []); errors = payload.get("errors", []); by_group = payload.get("incomplete_rows_by_group", {}); value_count = len(by_group.get("value_features", [])); capability_count = len(by_group.get("capability_shims", [])); issues = ", ".join("#%s" % issue for issue in payload.get("blocker_issues", [])); print("errors: " + ", ".join(map(str, errors)) if errors else ("%d incomplete rows (%d value, %d capability), %d blocked rows; blocker issues: %s" % (len(incomplete), value_count, capability_count, len(blocked), issues) if incomplete or blocked else "contract status: %s" % payload.get("contract_status")))'
}

closed_blocking_issues_from_manifest() {
  local issue
  local issue_state

  while IFS= read -r issue; do
    issue_state=""
    if issue_state="$(read_issue_state "$issue")" && [[ -n "$issue_state" ]]; then
      if [[ "$issue_state" == "CLOSED" ]]; then
        printf '%s\n' "$issue"
      fi
    fi
  done < <(blocking_issues_from_manifest)
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

generated_rust_cli_gate_report() {
  python3 - <<'PY'
from pathlib import Path

main_rs = Path("stage1/crates/axiomc/src/main.rs")
codegen_rs = Path("stage1/crates/axiomc/src/codegen.rs")
if not main_rs.is_file() or not codegen_rs.is_file():
    missing = [str(path) for path in (main_rs, codegen_rs) if not path.is_file()]
    print("generated_rust_cli_gate|fail|missing source file(s): " + ", ".join(missing))
else:
    main_source = main_rs.read_text(encoding="utf-8")
    codegen_source = codegen_rs.read_text(encoding="utf-8")
    failures = []
    if "AXIOM_ENABLE_GENERATED_RUST_COMPAT" in main_source:
        failures.append("temporary compatibility environment variable remains")
    if "validate_cli_backend" in main_source:
        failures.append("post-parse generated-rust CLI validator remains")
    if '"generated-rust" => Ok(Self::GeneratedRust)' in codegen_source:
        failures.append("generated-rust still parses as a CLI backend")
    if "compatibility backends: generated-rust" in codegen_source:
        failures.append("CLI parser still advertises generated-rust compatibility")
    if failures:
        print(
            "generated_rust_cli_gate|fail|generated-rust CLI removal is incomplete: "
            + ", ".join(failures)
        )
    else:
        print("generated_rust_cli_gate|pass|generated-rust is removed from CLI backend parsing")
PY
}

generated_rust_contract_gate_report() {
  python3 - <<'PY'
import json
from pathlib import Path

paths = {
    "build_success": Path("stage1/json-fixtures/build/success.json"),
    "test_filter_success": Path("stage1/json-fixtures/test/filter-success.json"),
    "test_failure": Path("stage1/json-fixtures/test/failure.json"),
    "artifact_schema": Path("stage1/schemas/axiom-artifacts-v0.schema.json"),
}
missing = [str(path) for path in paths.values() if not path.is_file()]
if missing:
    print("generated_rust_contract_gate|fail|missing contract file(s): " + ", ".join(missing))
else:
    failures = []

    build_success = json.loads(paths["build_success"].read_text(encoding="utf-8"))
    if build_success.get("backend") != "cranelift":
        failures.append("build success fixture is not cranelift")
    if build_success.get("generated_rust") is not None:
        failures.append("build success fixture reports generated_rust")
    cache_key = build_success.get("cache_key", {})
    if "generated_rust_hash" in cache_key:
        failures.append("build cache contract still exposes generated_rust_hash")
    if "backend_input_hash" not in cache_key:
        failures.append("build cache contract must expose backend_input_hash")
    for package in build_success.get("packages", []):
        if package.get("backend") != "cranelift":
            failures.append("package build fixture is not cranelift")
        if package.get("generated_rust") is not None:
            failures.append("package build fixture reports generated_rust")
        package_cache = package.get("cache_key", {})
        if "generated_rust_hash" in package_cache:
            failures.append("package cache contract still exposes generated_rust_hash")

    for key in ("test_filter_success", "test_failure"):
        payload = json.loads(paths[key].read_text(encoding="utf-8"))
        for case in payload.get("cases", []):
            if case.get("generated_rust") is not None:
                failures.append(f"{paths[key]} reports generated_rust for test case")

    artifact_schema = json.loads(paths["artifact_schema"].read_text(encoding="utf-8"))
    artifact_kinds = set(
        artifact_schema.get("$defs", {})
        .get("artifact", {})
        .get("properties", {})
        .get("kind", {})
        .get("enum", [])
    )
    if "generated_rust" in artifact_kinds:
        failures.append("artifact schema still allows generated_rust as a planned kind")
    if "legacy_generated_rust" not in artifact_kinds:
        failures.append("artifact schema must classify stale files as legacy_generated_rust")

    if failures:
        print(
            "generated_rust_contract_gate|fail|generated-rust contract removal is incomplete: "
            + ", ".join(dict.fromkeys(failures))
        )
    else:
        print("generated_rust_contract_gate|pass|command fixtures and artifact schema no longer model generated Rust as supported output")
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

abi_report=""
abi_ready=false
if abi_report="$(direct_native_runtime_abi_report 2>/dev/null)" && [[ -n "$abi_report" ]]; then
  if printf '%s' "$abi_report" | direct_native_runtime_abi_ready; then
    abi_ready=true
  fi
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

blocking_entries = payload.get("blockingIssues", [])
if not isinstance(blocking_entries, list) or not blocking_entries:
    print("blockingIssues must be a non-empty list", file=sys.stderr)
    sys.exit(1)

issues = []
for index, entry in enumerate(blocking_entries):
    if not isinstance(entry, dict):
        print(f"blockingIssues[{index}] must be an object", file=sys.stderr)
        sys.exit(1)
    issue = entry.get("issue")
    if not isinstance(issue, int):
        print(f"blockingIssues[{index}].issue must be an integer", file=sys.stderr)
        sys.exit(1)
    if not entry.get("lane"):
        print(f"blockingIssues[{index}].lane must be non-empty", file=sys.stderr)
        sys.exit(1)
    if not entry.get("check"):
        print(f"blockingIssues[{index}].check must be non-empty", file=sys.stderr)
        sys.exit(1)
    issues.append(issue)

if payload["finalBootstrapIssue"] in issues:
    print("finalBootstrapIssue must not also be listed as a blocker", file=sys.stderr)
    sys.exit(1)

required = {731, 1191}
missing = sorted(required - set(issues))
if missing:
    print("missing required blocking issues: " + ", ".join(f"#{issue}" for issue in missing), file=sys.stderr)
    sys.exit(1)
unexpected = sorted(set(issues) - required)
if unexpected:
    print("unexpected stale blocking issues: " + ", ".join(f"#{issue}" for issue in unexpected), file=sys.stderr)
    sys.exit(1)
if len(set(issues)) != len(issues):
    print("blocking issue list contains duplicates", file=sys.stderr)
    sys.exit(1)

with open("stage1/runtime-abi/direct-native-v0.json", encoding="utf-8") as handle:
    contract = json.load(handle)

abi_blockers = set()
for group in ("value_features", "capability_shims"):
    for row in contract.get(group, []):
        if row.get("status") != "implemented":
            abi_blockers.update(row.get("blockers", []))

missing_abi_blockers = sorted(abi_blockers - set(issues))
if missing_abi_blockers:
    print(
        "ABI blocker issues missing from readiness manifest: "
        + ", ".join(f"#{issue}" for issue in missing_abi_blockers),
        file=sys.stderr,
    )
    sys.exit(1)
PY
  add_check "readiness_manifest_valid" "pass" "docs/rust-exit-readiness.json has schema-valid live blockers and covers ABI blockers"
else
  add_check "readiness_manifest_valid" "fail" "docs/rust-exit-readiness.json cannot be validated"
fi

if [[ ! -f docs/rust-exit-readiness.json ]]; then
  add_check "readiness_blockers_closed" "fail" "Rust exit readiness manifest is unavailable"
elif all_blocking_issues_closed; then
  add_check "readiness_blockers_closed" "pass" "All blocking issues listed in docs/rust-exit-readiness.json are CLOSED"
else
  add_check "readiness_blockers_closed" "fail" "One or more blocking issues listed in docs/rust-exit-readiness.json are not CLOSED"
fi

if [[ "$abi_ready" != true && -f docs/rust-exit-readiness.json ]]; then
  closed_blocking_issues=()
  while IFS= read -r closed_issue; do
    closed_blocking_issues+=("$closed_issue")
  done < <(closed_blocking_issues_from_manifest)
  if [[ "${#closed_blocking_issues[@]}" -gt 0 ]]; then
    closed_issue_detail="$(printf '#%s, ' "${closed_blocking_issues[@]}")"
    closed_issue_detail="${closed_issue_detail%, }"
    add_check "readiness_blockers_live_when_not_ready" "fail" "closed blockers cannot represent remaining Rust-exit work while the ABI is not ready: ${closed_issue_detail}"
  else
    add_check "readiness_blockers_live_when_not_ready" "pass" "listed blockers remain live while the ABI is not ready"
  fi
else
  add_check "readiness_blockers_live_when_not_ready" "pass" "ABI is ready or no readiness manifest is present"
fi

if [[ -n "$abi_report" ]]; then
  if [[ "$abi_ready" == true ]]; then
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

while IFS='|' read -r gate_name gate_status gate_detail; do
  add_check "$gate_name" "$gate_status" "$gate_detail"
done < <(generated_rust_cli_gate_report)

while IFS='|' read -r gate_name gate_status gate_detail; do
  add_check "$gate_name" "$gate_status" "$gate_detail"
done < <(generated_rust_contract_gate_report)

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
