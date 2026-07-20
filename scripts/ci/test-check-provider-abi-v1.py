#!/usr/bin/env python3
"""Negative coverage for every Provider ABI v1 contract rule and fixture."""
import copy
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

R = Path(__file__).resolve().parents[2]
CHECKER = "scripts/ci/check-provider-abi-v1.py"

def run(root):
    return subprocess.run([sys.executable, str(root / CHECKER), "--target", "test-target"], cwd=root, capture_output=True, text=True).returncode

def mutate(path, value):
    def apply(document):
        cursor = document
        for key in path[:-1]: cursor = cursor[key]
        cursor[path[-1]] = value
    return lambda document, schema: apply(document)

CASES = {
    "schema-envelope": lambda v, s: s.update({"additionalProperties": True}),
    "schema-required": lambda v, s: s["required"].remove("audit"),
    "version-incompatible": mutate(("negotiation", "incompatible"), "allow_call"),
    "missing-symbol": lambda v, s: v["negotiation"]["required_symbols"].pop(),
    "untrusted-library": mutate(("loading", "trust"), "optional"),
    "capability-denied": mutate(("operations", "capability"), "declared_only"),
    "null-invalid-stale-handle": mutate(("handles", "invalid"), "dispatch_anyway"),
    "borrowed-buffer-retention": mutate(("buffers", "retention"), "allowed"),
    "provider-fault-quarantine": mutate(("operations", "fault"), "continue"),
    "owned-buffer-release-leak": mutate(("buffers", "safe_return"), "copy_without_release"),
    "cancel-drain": mutate(("operations", "cancellation"), "teardown_immediately"),
    "safe-surface": lambda v, s: v["safe_surface"]["forbidden"].remove("raw_pointer"),
    "feature-discovery": mutate(("negotiation", "features"), "anything"),
    "handle-validation": lambda v, s: v["handles"]["validation"].remove("generation"),
    "buffer-limits": mutate(("buffers", "limits"), "unchecked"),
    "error-shape": lambda v, s: v["errors"]["shape"].remove("message"),
    "error-kinds": lambda v, s: v["errors"]["kinds"].remove("provider_leak"),
    "search-path": mutate(("loading", "search_paths", "ambient_lookup"), "allowed"),
    "missing-symbol-before-code": mutate(("loading", "resolution"), "after_provider_code"),
    "capability-audit": lambda v, s: v["audit"]["forbidden"].remove("credentials"),
    "fixture-id": mutate(("fixtures", 1, "id"), "unverified-version-fixture"),
}

with tempfile.TemporaryDirectory() as directory:
    repo = Path(directory) / "repo"
    shutil.copytree(R / "stage1", repo / "stage1")
    (repo / "scripts/ci").mkdir(parents=True)
    shutil.copy2(R / CHECKER, repo / CHECKER)
    if run(repo): raise SystemExit("valid contract rejected")
    original_contract = json.loads((repo / "stage1/compiler-contracts/snapshots/provider-abi-v1.json").read_text())
    original_schema = json.loads((repo / "stage1/compiler-contracts/schemas/axiom.provider-abi.v1.schema.json").read_text())
    for name, change in CASES.items():
        contract, schema = copy.deepcopy(original_contract), copy.deepcopy(original_schema)
        change(contract, schema)
        (repo / "stage1/compiler-contracts/snapshots/provider-abi-v1.json").write_text(json.dumps(contract))
        (repo / "stage1/compiler-contracts/schemas/axiom.provider-abi.v1.schema.json").write_text(json.dumps(schema))
        if not run(repo): raise SystemExit(f"{name} accepted")
print(f"Provider ABI v1 checker tests passed ({len(CASES)} negative cases)")
