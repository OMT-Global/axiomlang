#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo_root/scripts/ci/run-compiler-property-checks.sh"

if grep -Eq 'mktemp .*[.]XXXXXX[.]' "$script"; then
  echo "compiler property checks must use BSD-compatible mktemp templates" >&2
  exit 1
fi

harness_tmp="$(mktemp -d)"
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
    else
      printf '{"backend":"cranelift","ok":true,"cases":[{"name":"fake_property","generated_rust":null}]}'
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

if PATH="$fake_bin:$PATH" TMPDIR="$run_tmp" AXIOM_FAKE_CARGO_JSON=invalid bash "$script" >/dev/null 2>&1; then
  echo "compiler property checks must fail on invalid JSON" >&2
  exit 1
fi

if find "$run_tmp" -maxdepth 1 -name 'axiom-compiler-property-cranelift*' -print -quit | grep -q .; then
  echo "compiler property checks must remove temporary report directories after JSON validation failure" >&2
  find "$run_tmp" -maxdepth 1 -name 'axiom-compiler-property-cranelift*' -print >&2
  exit 1
fi

report_log="$harness_tmp/report-paths.txt"
: >"$report_log"
PATH="$fake_bin:$PATH" TMPDIR="$run_tmp" AXIOM_FAKE_CARGO_REPORT_LOG="$report_log" bash "$script" &
pid_one=$!
PATH="$fake_bin:$PATH" TMPDIR="$run_tmp" AXIOM_FAKE_CARGO_REPORT_LOG="$report_log" bash "$script" &
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
