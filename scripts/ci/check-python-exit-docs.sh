#!/usr/bin/env bash
set -euo pipefail

decision_doc="docs/python-exit-vm-disposition.md"
parity_doc="docs/python-exit-parity-gate.md"

if [[ ! -f "$decision_doc" ]]; then
  echo "missing $decision_doc" >&2
  exit 1
fi

if [[ ! -f "$parity_doc" ]]; then
  echo "missing $parity_doc" >&2
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

if [[ "${#doc_search_paths[@]}" -gt 0 ]] && rg -n "$legacy_invocation" "${doc_search_paths[@]}" \
  --glob '*.md' \
  --glob '*.sh' \
  --glob '!docs/python-exit-parity-gate.md' \
  --glob '!docs/python-exit-vm-disposition.md'; then
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

if [[ "${#ci_search_paths[@]}" -gt 0 ]] && rg -n --hidden "$python_unittest" "${ci_search_paths[@]}"; then
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
