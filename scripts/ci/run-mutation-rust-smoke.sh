#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
manifest_path="$repo_root/stage1/Cargo.toml"
output_dir="${MUTATION_RUST_SMOKE_OUTPUT_DIR:-$repo_root/stage1/target/mutation-rust-smoke}"
summary_path="$output_dir/survivors.json"

files=(
  "$repo_root/stage1/crates/axiomc/src/syntax.rs"
  "$repo_root/stage1/crates/axiomc/src/hir.rs"
  "$repo_root/stage1/crates/axiomc/src/mir.rs"
  "$repo_root/stage1/crates/axiomc/src/codegen.rs"
)

if ! command -v cargo-mutants >/dev/null 2>&1; then
  echo "cargo-mutants is required for mutation-rust-smoke" >&2
  echo "install with: cargo install cargo-mutants --locked" >&2
  exit 127
fi

mkdir -p "$output_dir"
rm -rf "$output_dir/cargo-mutants"

args=(
  mutants
  --manifest-path "$manifest_path"
  --package axiomc
  --output "$output_dir/cargo-mutants"
  --timeout "${MUTATION_RUST_SMOKE_TIMEOUT:-60}"
  --jobs "${MUTATION_RUST_SMOKE_JOBS:-2}"
)

for file in "${files[@]}"; do
  if [[ ! -f "$file" ]]; then
    echo "missing mutation smoke input: $file" >&2
    exit 1
  fi
  args+=(--file "$file")
done

set +e
cargo "${args[@]}"
status=$?
set -e

python3 - "$output_dir/cargo-mutants" "$summary_path" "$status" "${files[@]#$repo_root/}" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path

run_dir = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
status = int(sys.argv[3])
scoped_files = list(sys.argv[4:])

survivor_outcomes = {"missed", "timeout"}
survivors: list[dict[str, object]] = []
outcome_counts: dict[str, int] = {}

outcomes_path = run_dir / "outcomes.json"
if outcomes_path.exists():
    data = json.loads(outcomes_path.read_text(encoding="utf-8"))
    entries = data.values() if isinstance(data, dict) else data
    for entry in entries:
        if not isinstance(entry, dict):
            continue
        outcome = str(entry.get("outcome") or entry.get("result") or "unknown")
        outcome_counts[outcome] = outcome_counts.get(outcome, 0) + 1
        if outcome in survivor_outcomes:
            survivors.append(
                {
                    "outcome": outcome,
                    "file": entry.get("file") or entry.get("src") or entry.get("path"),
                    "line": entry.get("line"),
                    "mutant": entry.get("mutant") or entry.get("name") or entry.get("description"),
                }
            )
else:
    # Older cargo-mutants releases may not write outcomes.json. Preserve an
    # explicit machine-readable summary instead of silently losing the run.
    outcome_counts["unknown"] = 0

summary = {
    "schema": "axiom.mutation-rust-smoke.survivors.v1",
    "status": status,
    "scope": {
        "package": "axiomc",
        "files": scoped_files,
    },
    "runDir": str(run_dir),
    "outcomeCounts": outcome_counts,
    "survivorOutcomes": sorted(survivor_outcomes),
    "survivors": survivors,
}
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"wrote mutation smoke survivor summary: {summary_path}")
print(json.dumps({"survivors": len(survivors), "status": status}, sort_keys=True))
PY

# cargo-mutants returns 2 when mutants survive. For this smoke profile,
# survivors are recorded as follow-up diagnostics rather than weakening existing
# stage1 gates, so only infrastructure/tool failures block the target.
case "$status" in
  0)
    exit 0
    ;;
  2)
    echo "cargo-mutants reported surviving mutants; recorded diagnostics in $summary_path" >&2
    exit 0
    ;;
  *)
    echo "cargo-mutants failed with infrastructure/tool status $status" >&2
    exit "$status"
    ;;
esac
