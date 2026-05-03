#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source_script="$repo_root/scripts/ci/check-python-exit-docs.sh"

if [[ ! -f "$source_script" ]]; then
  echo "missing source script: $source_script" >&2
  exit 1
fi

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
legacy_invocation="python -m axi""om"
python_unittest="python -m unit""test"

assert_success() {
  local case_name="$1"
  local status="$2"
  local output_path="$3"

  if [[ "$status" -ne 0 ]]; then
    echo "$case_name: expected success" >&2
    cat "$output_path" >&2
    exit 1
  fi
}

assert_failure_contains() {
  local case_name="$1"
  local status="$2"
  local output_path="$3"
  local expected="$4"

  if [[ "$status" -eq 0 ]]; then
    echo "$case_name: expected failure" >&2
    exit 1
  fi

  if ! grep -Fq "$expected" "$output_path"; then
    echo "$case_name: missing expected output: $expected" >&2
    cat "$output_path" >&2
    exit 1
  fi
}

assert_output_contains() {
  local case_name="$1"
  local output_path="$2"
  local expected="$3"

  if ! grep -Fq "$expected" "$output_path"; then
    echo "$case_name: missing expected output: $expected" >&2
    cat "$output_path" >&2
    exit 1
  fi
}

setup_case_repo() {
  local case_dir="$1"

  mkdir -p "$case_dir/docs" "$case_dir/scripts/ci"
  cp "$source_script" "$case_dir/scripts/ci/check-python-exit-docs.sh"
  cp "$repo_root/README.md" "$case_dir/README.md"
  cp "$repo_root/docs/python-exit-vm-disposition.md" "$case_dir/docs/python-exit-vm-disposition.md"
  cp "$repo_root/docs/python-exit-parity-gate.md" "$case_dir/docs/python-exit-parity-gate.md"
  : > "$case_dir/Makefile"
  : > "$case_dir/project.bootstrap.yaml"

  (
    cd "$case_dir"
    git init -q
    git config user.name "Ares"
    git config user.email "ares@example.com"
    git config commit.gpgsign false
    git add docs scripts
    git commit -q -m "fixture"
  )
}

run_case() {
  local case_name="$1"
  local expected_status="$2"
  local expected_text="${3:-}"
  local expected_detail="${4:-}"
  local case_dir="$tmpdir/$case_name"
  local output_path="$tmpdir/$case_name.out"
  local status=0

  setup_case_repo "$case_dir"

  case "$case_name" in
    rejects_missing_decision_doc)
      rm -f "$case_dir/docs/python-exit-vm-disposition.md"
      ;;
    rejects_missing_parity_doc)
      rm -f "$case_dir/docs/python-exit-parity-gate.md"
      ;;
    excluded_docs_allow_legacy_strings)
      printf '%s\n' "$legacy_invocation" >> "$case_dir/docs/python-exit-vm-disposition.md"
      printf '%s\n' "$legacy_invocation" >> "$case_dir/docs/python-exit-parity-gate.md"
      ;;
    rejects_legacy_invocation_in_user_docs)
      printf '\n%s\n' "$legacy_invocation" >> "$case_dir/README.md"
      ;;
    rejects_legacy_invocation_in_docs_tree)
      printf '%s\n' "$legacy_invocation" > "$case_dir/docs/getting-started.md"
      ;;
    rejects_legacy_invocation_in_scripts_tree)
      printf '%s\n' "$legacy_invocation" > "$case_dir/scripts/ci/legacy-run.sh"
      ;;
    rejects_missing_readme_quickstart)
      awk '
        /^## Quickstart$/ { skip = 1; next }
        /^## / && skip { skip = 0 }
        !skip { print }
      ' "$case_dir/README.md" > "$case_dir/README.md.tmp"
      mv "$case_dir/README.md.tmp" "$case_dir/README.md"
      ;;
    rejects_python_readme_quickstart)
      cat > "$case_dir/README.md" <<README
# Axiom

## Quickstart

\`\`\`bash
$legacy_invocation check examples/hello.ax
\`\`\`

## Useful Commands

\`\`\`bash
make stage1-test
\`\`\`
README
      ;;
    rejects_incomplete_rust_readme_quickstart)
      cat > "$case_dir/README.md" <<'README'
# Axiom

## Quickstart

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/hello --json
```

## Useful Commands

```bash
make stage1-test
```
README
      ;;
    rejects_blocked_parity_rows)
      awk '
        {
          if (!inserted && $0 == "There are no `blocked` rows in the current matrix.") {
            print "| synthetic blocked case | `blocked` | Linked child issue is still open. |"
            print ""
            inserted = 1
          }
          print
        }
      ' "$case_dir/docs/python-exit-parity-gate.md" > "$case_dir/docs/python-exit-parity-gate.md.tmp"
      mv "$case_dir/docs/python-exit-parity-gate.md.tmp" "$case_dir/docs/python-exit-parity-gate.md"
      ;;
    rejects_python_unittest_gate)
      mkdir -p "$case_dir/.github/workflows"
      printf '%s\n' "$python_unittest" > "$case_dir/.github/workflows/pr-fast-ci.yml"
      ;;
    rejects_python_unittest_gate_in_makefile)
      printf '%s\n' "$python_unittest" > "$case_dir/Makefile"
      ;;
    rejects_python_unittest_gate_in_bootstrap_config)
      printf '%s\n' "$python_unittest" > "$case_dir/project.bootstrap.yaml"
      ;;
    rejects_tracked_stage0_files)
      mkdir -p "$case_dir/axiom"
      printf '%s\n' 'print("legacy")' > "$case_dir/axiom/legacy.py"
      ;;
    rejects_tracked_stage0_tests)
      mkdir -p "$case_dir/tests"
      printf '%s\n' 'print("legacy test")' > "$case_dir/tests/test_legacy.py"
      ;;
    rejects_tracked_stage0_pyproject)
      printf '%s\n' '[project]' > "$case_dir/pyproject.toml"
      ;;
    rejects_tracked_stage0_python_version)
      printf '%s\n' '3.12.0' > "$case_dir/.python-version"
      ;;
    rejects_tracked_stage0_requirements)
      printf '%s\n' 'pytest==8.0.0' > "$case_dir/requirements.txt"
      ;;
    rejects_tracked_stage0_requirements_in)
      printf '%s\n' 'pytest==8.0.0' > "$case_dir/requirements.in"
      ;;
    rejects_tracked_stage0_requirements_lockfile)
      printf '%s\n' 'pytest==8.0.0' > "$case_dir/requirements-dev.txt"
      ;;
    rejects_tracked_stage0_pipfile)
      printf '%s\n' '[[source]]' > "$case_dir/Pipfile"
      ;;
    rejects_tracked_stage0_pipfile_lock)
      printf '%s\n' '{}' > "$case_dir/Pipfile.lock"
      ;;
    rejects_tracked_stage0_poetry_lock)
      printf '%s\n' '# lock' > "$case_dir/poetry.lock"
      ;;
    rejects_tracked_stage0_setup_cfg)
      printf '%s\n' '[metadata]' > "$case_dir/setup.cfg"
      ;;
    rejects_tracked_stage0_setup_py)
      printf '%s\n' 'from setuptools import setup' > "$case_dir/setup.py"
      ;;
    rejects_tracked_stage0_tox_ini)
      printf '%s\n' '[tox]' > "$case_dir/tox.ini"
      ;;
    *)
      echo "unknown case: $case_name" >&2
      exit 1
      ;;
  esac

  (
    cd "$case_dir"
    git add .
    set +e
    bash scripts/ci/check-python-exit-docs.sh >"$output_path" 2>&1
    status=$?
    set -e
    echo "$status" > "$tmpdir/$case_name.status"
  )

  status="$(cat "$tmpdir/$case_name.status")"

  if [[ "$expected_status" == "success" ]]; then
    assert_success "$case_name" "$status" "$output_path"
  else
    assert_failure_contains "$case_name" "$status" "$output_path" "$expected_text"

    if [[ -n "$expected_detail" ]]; then
      assert_output_contains "$case_name" "$output_path" "$expected_detail"
    fi
  fi
}

run_case rejects_missing_decision_doc failure "missing docs/python-exit-vm-disposition.md"
run_case rejects_missing_parity_doc failure "missing docs/python-exit-parity-gate.md"
run_case excluded_docs_allow_legacy_strings success
run_case rejects_legacy_invocation_in_user_docs failure "user-facing docs still instruct users to run $legacy_invocation"
run_case rejects_legacy_invocation_in_docs_tree failure "user-facing docs still instruct users to run $legacy_invocation"
run_case rejects_legacy_invocation_in_scripts_tree failure "user-facing docs still instruct users to run $legacy_invocation"
run_case rejects_missing_readme_quickstart failure "README quickstart is missing"
run_case rejects_python_readme_quickstart failure "README quickstart must use the Rust axiomc check workflow"
run_case rejects_incomplete_rust_readme_quickstart failure "README quickstart must use the Rust axiomc run workflow"
run_case rejects_blocked_parity_rows failure "Python exit parity matrix has blocked rows"
run_case rejects_python_unittest_gate failure "CI still uses Python unittest as a language/runtime correctness gate"
run_case rejects_python_unittest_gate_in_makefile failure "CI still uses Python unittest as a language/runtime correctness gate"
run_case rejects_python_unittest_gate_in_bootstrap_config failure "CI still uses Python unittest as a language/runtime correctness gate"
run_case rejects_tracked_stage0_files failure "Python stage0 source, tests, or packaging files are still tracked" "axiom/legacy.py"
run_case rejects_tracked_stage0_tests failure "Python stage0 source, tests, or packaging files are still tracked" "tests/test_legacy.py"
run_case rejects_tracked_stage0_pyproject failure "Python stage0 source, tests, or packaging files are still tracked" "pyproject.toml"
run_case rejects_tracked_stage0_python_version failure "Python stage0 source, tests, or packaging files are still tracked" ".python-version"
run_case rejects_tracked_stage0_requirements failure "Python stage0 source, tests, or packaging files are still tracked" "requirements.txt"
run_case rejects_tracked_stage0_requirements_in failure "Python stage0 source, tests, or packaging files are still tracked" "requirements.in"
run_case rejects_tracked_stage0_requirements_lockfile failure "Python stage0 source, tests, or packaging files are still tracked" "requirements-dev.txt"
run_case rejects_tracked_stage0_pipfile failure "Python stage0 source, tests, or packaging files are still tracked" "Pipfile"
run_case rejects_tracked_stage0_pipfile_lock failure "Python stage0 source, tests, or packaging files are still tracked" "Pipfile.lock"
run_case rejects_tracked_stage0_poetry_lock failure "Python stage0 source, tests, or packaging files are still tracked" "poetry.lock"
run_case rejects_tracked_stage0_setup_cfg failure "Python stage0 source, tests, or packaging files are still tracked" "setup.cfg"
run_case rejects_tracked_stage0_setup_py failure "Python stage0 source, tests, or packaging files are still tracked" "setup.py"
run_case rejects_tracked_stage0_tox_ini failure "Python stage0 source, tests, or packaging files are still tracked" "tox.ini"

echo "check-python-exit-docs regression cases passed"
