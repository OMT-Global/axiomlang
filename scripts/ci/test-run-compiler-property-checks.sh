#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/run-compiler-property-checks.sh"

if ! awk '
  /^keep_outputs_writable\(\) \{$/ {
    if (getline != 1 || $0 != "  trap - EXIT HUP INT TERM") {
      exit 1
    }
    found=1
    exit
  }
  END { exit found ? 0 : 1 }
' "$script"; then
  echo "keep_outputs_writable must clear inherited traps before starting its loop" >&2
  exit 1
fi

if grep -Eq 'mktemp .*[.]XXXXXX[.]' "$script"; then
  echo "compiler property checks must use BSD-compatible mktemp templates" >&2
  exit 1
fi

if [[ "${GITHUB_ACTIONS:-}" == "true" && -n "${RUNNER_TEMP:-}" ]]; then
  # The self-hosted runner may reclaim nested /tmp paths while a long-running
  # Actions job is still using them. Keep the simulated checkout and its
  # report path under the checked-out workspace for the same reason as the
  # production property-check script.
  harness_tmp="$(mktemp -d "${repo_root%/}/.ci-property-checks.XXXXXX")"
else
  harness_tmp="$(mktemp -d)"
fi
cleanup() {
  rm -rf "$harness_tmp"
}
trap cleanup EXIT

fake_bin="$harness_tmp/bin"
mkdir -p "$fake_bin"
cat >"$fake_bin/cargo" <<'SH'
#!/usr/bin/env bash
set -euo pipefail

mode=""
for arg in "$@"; do
  case "$arg" in
    check|test)
      mode="$arg"
      ;;
  esac
done

case "$mode" in
  check)
    exit 0
    ;;
  test)
    if [[ -n "${AXIOM_FAKE_CARGO_REPORT_LOG:-}" && -e "/proc/$$/fd/1" ]]; then
      readlink "/proc/$$/fd/1" >>"$AXIOM_FAKE_CARGO_REPORT_LOG" 2>/dev/null || true
    fi
    if [[ "${AXIOM_FAKE_CARGO_JSON:-valid}" == "invalid" ]]; then
      printf '{"backend":"cranelift","ok":false'
    elif [[ "${AXIOM_FAKE_CARGO_JSON:-valid}" == "missing-binary" ]]; then
      printf '{"backend":"cranelift","ok":true,"cases":[{"kind":"property","name":"fake_property","generated_rust":null,"duration_ms":1,"binary":null}]}'
    elif [[ "${AXIOM_FAKE_CARGO_JSON:-valid}" == "lowering-tolerated" ]]; then
      printf '{"backend":"cranelift","ok":false,"cases":[{"kind":"property","name":"fake_property","generated_rust":null,"duration_ms":1,"binary":null,"ok":false,"error":{"code":"backend.runtime_lowering_required"},"lowering":{"schema_version":"axiom.build-lowering-evidence.v1","lowering_mode":"runtime_lowering_required","execution_mode":"not_produced"}}]}'
    elif [[ "${AXIOM_FAKE_CARGO_JSON:-valid}" == "lowering-no-evidence" ]]; then
      printf '{"backend":"cranelift","ok":false,"cases":[{"kind":"property","name":"fake_property","generated_rust":null,"duration_ms":1,"binary":null,"ok":false,"error":{"code":"backend.runtime_lowering_required"}}]}'
    else
      printf '{"backend":"cranelift","ok":true,"cases":[{"kind":"property","name":"fake_property","generated_rust":null,"duration_ms":1,"binary":"/fake/property-bin"}]}'
    fi
    ;;
  *)
    echo "fake cargo expected check or test mode: $*" >&2
    exit 1
    ;;
esac
SH
chmod +x "$fake_bin/cargo"

run_tmp="$harness_tmp/run-tmp"
mkdir -p "$run_tmp"
PATH="$fake_bin:$PATH" TMPDIR="$run_tmp" bash "$script"

if find "$run_tmp" -maxdepth 1 -name 'axiom-compiler-property-cranelift*' -print -quit | grep -q .; then
  echo "compiler property checks must remove temporary report directories after success" >&2
  find "$run_tmp" -maxdepth 1 -name 'axiom-compiler-property-cranelift*' -print >&2
  exit 1
fi

action_report_root="$harness_tmp/action-checkout"
mkdir -p "$action_report_root/stage1/examples/compiler_properties/src"
for index in $(seq 1 100); do
  printf 'property fn fake_%s() { }\n' "$index" >>"$action_report_root/stage1/examples/compiler_properties/src/main.ax"
done
PATH="$fake_bin:$PATH" TMPDIR="$run_tmp" GITHUB_ACTIONS=true RUNNER_TEMP="$harness_tmp/runner-temp" AXIOM_CHECKOUT_PATH="$action_report_root" bash "$script" >/dev/null
if find "$action_report_root" -maxdepth 1 -name 'axiom-compiler-property-cranelift*' -print -quit | grep -q .; then
  echo "Actions-mode compiler property checks must remove workspace report directories after success" >&2
  find "$action_report_root" -maxdepth 1 -name 'axiom-compiler-property-cranelift*' -print >&2
  exit 1
fi

if PATH="$fake_bin:$PATH" TMPDIR="$run_tmp" AXIOM_FAKE_CARGO_JSON=invalid bash "$script" >/dev/null 2>&1; then
  echo "compiler property checks must fail on invalid JSON" >&2
  exit 1
fi

if PATH="$fake_bin:$PATH" TMPDIR="$run_tmp" AXIOM_FAKE_CARGO_JSON=missing-binary bash "$script" >/dev/null 2>&1; then
  echo "compiler property checks must fail when a property case was not executed" >&2
  exit 1
fi

if ! PATH="$fake_bin:$PATH" TMPDIR="$run_tmp" AXIOM_FAKE_CARGO_JSON=lowering-tolerated bash "$script" >/dev/null 2>&1; then
  echo "compiler property checks must accept tolerated unexecuted cases with bounded lowering evidence" >&2
  exit 1
fi

if PATH="$fake_bin:$PATH" TMPDIR="$run_tmp" AXIOM_FAKE_CARGO_JSON=lowering-no-evidence bash "$script" >/dev/null 2>&1; then
  echo "compiler property checks must fail tolerated failures that lack bounded lowering evidence" >&2
  exit 1
fi

if find "$run_tmp" -maxdepth 1 -name 'axiom-compiler-property-cranelift*' -print -quit | grep -q .; then
  echo "compiler property checks must remove temporary report directories after JSON validation failure" >&2
  find "$run_tmp" -maxdepth 1 -name 'axiom-compiler-property-cranelift*' -print >&2
  exit 1
fi

report_log="$harness_tmp/report-paths.txt"
: >"$report_log"

# Parallel invocations must not share a checkout: the script mutates
# stage1/examples/compiler_properties/dist (rm -rf + mkdir + chmod fixer),
# so two concurrent instances racing on one checkout flake the fast-checks
# lane with transient rm/exec failures and missing report payloads.
parallel_checkouts=()
for slot in one two; do
  iso_checkout="$harness_tmp/parallel-checkout-$slot"
  mkdir -p "$iso_checkout/stage1/examples"
  cp -a "$repo_root/stage1/examples/compiler_properties" \
    "$iso_checkout/stage1/examples/compiler_properties"
  rm -rf "$iso_checkout/stage1/examples/compiler_properties/dist"
  parallel_checkouts+=("$iso_checkout")
done

PATH="$fake_bin:$PATH" TMPDIR="$run_tmp" AXIOM_CHECKOUT_PATH="${parallel_checkouts[0]}" AXIOM_FAKE_CARGO_REPORT_LOG="$report_log" bash "$script" &
pid_one=$!
PATH="$fake_bin:$PATH" TMPDIR="$run_tmp" AXIOM_CHECKOUT_PATH="${parallel_checkouts[1]}" AXIOM_FAKE_CARGO_REPORT_LOG="$report_log" bash "$script" &
pid_two=$!
wait "$pid_one"
wait "$pid_two"

if [[ -s "$report_log" ]]; then
  report_count="$(sort -u "$report_log" | wc -l | tr -d '[:space:]')"
  if [[ "$report_count" != "2" ]]; then
    echo "parallel compiler property checks must use distinct report paths" >&2
    cat "$report_log" >&2
    exit 1
  fi
else
  grep -Fq 'mktemp -d "${report_parent%/}/axiom-compiler-property-cranelift.XXXXXX"' "$script" || {
    echo "compiler property checks must allocate one temporary report directory per invocation" >&2
    exit 1
  }
fi

if find "$run_tmp" -maxdepth 1 -name 'axiom-compiler-property-cranelift*' -print -quit | grep -q .; then
  echo "parallel compiler property checks must clean temporary report directories" >&2
  find "$run_tmp" -maxdepth 1 -name 'axiom-compiler-property-cranelift*' -print >&2
  exit 1
fi

echo "run-compiler-property-checks regression cases passed"
