#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
workflow="$repo_root/.github/workflows/pr-fast-ci.yml"
fast_checks_script="$repo_root/scripts/ci/run-fast-checks.sh"

if [[ ! -f "$workflow" ]]; then
  echo "missing workflow: $workflow" >&2
  exit 1
fi

if [[ ! -f "$fast_checks_script" ]]; then
  echo "missing fast checks script: $fast_checks_script" >&2
  exit 1
fi

section="$({
  awk '
    /^  validate-pr-description:$/ { in_job=1; print; next }
    in_job && /^  [A-Za-z0-9_-]+:$/ { exit }
    in_job { print }
  ' "$workflow"
})"

if [[ -z "$section" ]]; then
  echo "validate-pr-description job is missing from pr-fast-ci workflow" >&2
  exit 1
fi

checkout_line=$(printf '%s\n' "$section" | nl -ba | { grep 'actions/checkout@' || true; } | head -n1 | awk '{print $1}')
inline_run_line=$(printf '%s\n' "$section" | nl -ba | { grep -E 'run:[[:space:]]*\|' || true; } | head -n1 | awk '{print $1}')
script_run_line=$(printf '%s\n' "$section" | nl -ba | { grep 'bash scripts/ci/validate-pr-description.sh' || true; } | head -n1 | awk '{print $1}')
pr_body_env_line=$(printf '%s\n' "$section" | nl -ba | { grep -F 'PR_BODY: ${{ github.event.pull_request.body }}' || true; } | head -n1 | awk '{print $1}')
ci_gate_needs_validate=$(awk '
  /^  ci-gate:$/ { in_job=1; next }
  in_job && /^  [A-Za-z0-9_-]+:$/ { exit }
  in_job && /validate-pr-description/ { found=1 }
  END { if (found) print "yes" }
' "$workflow")
ci_gate_section="$({
  awk '
    /^  ci-gate:$/ { in_job=1; print; next }
    in_job && /^  [A-Za-z0-9_-]+:$/ { exit }
    in_job { print }
  ' "$workflow"
})"
ci_gate_checkout_line=$(printf '%s\n' "$ci_gate_section" | nl -ba | { grep 'actions/checkout@' || true; } | head -n1 | awk '{print $1}')
ci_gate_run_line=$(printf '%s\n' "$ci_gate_section" | nl -ba | { grep 'bash scripts/ci/check-pr-fast-ci-gate.sh' || true; } | head -n1 | awk '{print $1}')
ci_gate_fork_validate_line=$(printf '%s\n' "$ci_gate_section" | nl -ba | { grep 'bash scripts/ci/validate-pr-description.sh' || true; } | head -n1 | awk '{print $1}')
ci_gate_base_ref_line=$(printf '%s\n' "$ci_gate_section" | nl -ba | { grep -F 'ref: ${{ github.event.pull_request.base.sha }}' || true; } | head -n1 | awk '{print $1}')
ci_gate_head_ref=$(printf '%s\n' "$ci_gate_section" | grep -F 'github.event.pull_request.head.sha' || true)
benchmark_gate_reference=$(grep -nE 'check-stage1-benchmarks\.py|stage1-comparison-report\.json' "$workflow" || true)
runtime_abi_status_check=$(grep -nF 'scripts/ci/render-direct-native-runtime-abi-status.py' "$fast_checks_script" || true)

if [[ -n "$checkout_line" ]]; then
  echo "validate-pr-description job must not checkout untrusted pull request code" >&2
  exit 1
fi

if [[ -z "$inline_run_line" ]]; then
  echo "validate-pr-description job must keep PR description validation inline" >&2
  exit 1
fi

if [[ -n "$script_run_line" ]]; then
  echo "validate-pr-description job must not execute repository validation scripts" >&2
  exit 1
fi

if [[ -z "$pr_body_env_line" ]]; then
  echo "validate-pr-description job must pass pull_request.body into PR_BODY" >&2
  exit 1
fi

if [[ "$ci_gate_needs_validate" != "yes" ]]; then
  echo "ci-gate must depend on validate-pr-description so PR body failures block the workflow" >&2
  exit 1
fi

if ! grep -q 'IS_FORK_PR:' <<<"$ci_gate_section"; then
  echo "ci-gate must expose IS_FORK_PR to fail skipped fork validation jobs" >&2
  exit 1
fi

if ! grep -q 'TRUSTED_FORK_PR_DESCRIPTION_VALIDATED:' <<<"$ci_gate_section"; then
  echo "ci-gate must mark trusted fork PR description validation before accepting a skipped branch job" >&2
  exit 1
fi

if [[ -z "$ci_gate_checkout_line" ]]; then
  echo "ci-gate must checkout the repo before running the gate helper" >&2
  exit 1
fi

if [[ -z "$ci_gate_run_line" ]]; then
  echo "ci-gate must delegate result policy to scripts/ci/check-pr-fast-ci-gate.sh" >&2
  exit 1
fi

if (( ci_gate_checkout_line >= ci_gate_run_line )); then
  echo "ci-gate must checkout the repo before running the gate helper" >&2
  exit 1
fi

if [[ -z "$ci_gate_fork_validate_line" ]]; then
  echo "ci-gate must validate fork PR descriptions from the trusted base checkout" >&2
  exit 1
fi

if (( ci_gate_checkout_line >= ci_gate_fork_validate_line || ci_gate_fork_validate_line >= ci_gate_run_line )); then
  echo "ci-gate must validate fork PR descriptions after trusted checkout and before the gate helper" >&2
  exit 1
fi

if [[ -z "$ci_gate_base_ref_line" || -n "$ci_gate_head_ref" ]]; then
  echo "ci-gate must checkout the trusted base SHA before running repository scripts on self-hosted runners" >&2
  exit 1
fi

RESULTS='changes=success fast-checks=skipped validate-pr-description=skipped validate-secrets=skipped' \
  IS_FORK_PR=false \
  bash "$repo_root/scripts/ci/check-pr-fast-ci-gate.sh" >/dev/null

if RESULTS='changes=success fast-checks=skipped validate-pr-description=skipped validate-secrets=skipped' \
  IS_FORK_PR=true \
  TRUSTED_FORK_PR_DESCRIPTION_VALIDATED=false \
  bash "$repo_root/scripts/ci/check-pr-fast-ci-gate.sh" >/dev/null 2>&1; then
  echo "ci-gate must not accept skipped fork PR description validation without the trusted-base validation step" >&2
  exit 1
fi

if ! RESULTS='changes=success fast-checks=skipped validate-pr-description=skipped validate-secrets=skipped' \
  IS_FORK_PR=true \
  TRUSTED_FORK_PR_DESCRIPTION_VALIDATED=true \
  bash "$repo_root/scripts/ci/check-pr-fast-ci-gate.sh" >/dev/null 2>&1; then
  echo "ci-gate must allow fork PR branch validation jobs after PR description was validated from the trusted base checkout" >&2
  exit 1
fi

if [[ -n "$benchmark_gate_reference" ]]; then
  echo "pr-fast-ci must not run the Stage 1 comparison benchmark gate; keep it in extended, nightly, or manual validation" >&2
  printf '%s\n' "$benchmark_gate_reference" >&2
  exit 1
fi

if [[ -z "$runtime_abi_status_check" ]]; then
  echo "run-fast-checks must validate that the direct-native runtime ABI status block matches the JSON contract" >&2
  exit 1
fi

echo "pr-fast-ci workflow validation passed"
