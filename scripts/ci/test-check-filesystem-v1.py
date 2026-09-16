#!/usr/bin/env python3
"""Hermetic regressions for the Filesystem v1 evidence checker."""

from __future__ import annotations

import copy
import importlib.util
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-filesystem-v1.py"


def run(root: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(CHECKER), "--root", str(root)],
        cwd=root,
        text=True,
        capture_output=True,
        check=False,
    )


def write(path: Path, value: dict[str, object]) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def expect_failure(root: Path, message: str) -> None:
    result = run(root)
    if result.returncode == 0:
        raise SystemExit(message)


def checker_module() -> object:
    spec = importlib.util.spec_from_file_location("filesystem_v1_checker", CHECKER)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main() -> None:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory) / "repo"
        shutil.copytree(ROOT / "stage1", root / "stage1")
        (root / "docs/rfcs").mkdir(parents=True)
        shutil.copy2(ROOT / "docs/stage1.md", root / "docs/stage1.md")
        shutil.copy2(
            ROOT / "docs/rfcs/0002-write-capability-boundary.md",
            root / "docs/rfcs/0002-write-capability-boundary.md",
        )
        shutil.copy2(ROOT / "docs/filesystem-v1.md", root / "docs/filesystem-v1.md")
        shutil.copy2(
            ROOT / "docs/production-language-readiness.json",
            root / "docs/production-language-readiness.json",
        )
        (root / "scripts/ci").mkdir(parents=True)
        shutil.copy2(
            ROOT / "scripts/ci/run-filesystem-v1-behavioral-tests.sh",
            root / "scripts/ci/run-filesystem-v1-behavioral-tests.sh",
        )

        valid = run(root)
        if valid.returncode != 0:
            raise SystemExit(f"valid Filesystem v1 evidence was rejected: {valid.stderr}")

        snapshot_path = root / "stage1/compiler-contracts/snapshots/filesystem-v1.json"
        original_snapshot = json.loads(snapshot_path.read_text(encoding="utf-8"))
        schema_path = root / "stage1/compiler-contracts/schemas/axiom.filesystem.v1.schema.json"
        original_schema = json.loads(schema_path.read_text(encoding="utf-8"))

        value = json.loads(snapshot_path.read_text(encoding="utf-8"))
        value["path_model"]["required_operations"] = list(
            reversed(value["path_model"]["required_operations"])
        )
        write(snapshot_path, value)
        expect_failure(root, "unordered path operations were accepted")

        value = json.loads(json.dumps(original_snapshot))
        value["authority"]["required_kinds"] = value["authority"]["required_kinds"][:-1]
        write(snapshot_path, value)
        expect_failure(root, "incomplete authority partition was accepted")

        value = copy.deepcopy(original_snapshot)
        value["authority"]["operation_requirements"]["read_dir"] = "metadata"
        write(snapshot_path, value)
        expect_failure(root, "wrong read_dir authority was accepted")

        value = json.loads(json.dumps(original_snapshot))
        value["implementation"]["atomic_replace"] = True
        write(snapshot_path, value)
        expect_failure(root, "unqualified atomic replacement was accepted")

        value = copy.deepcopy(original_snapshot)
        value["implementation"]["runtime_effects_only"] = True
        write(snapshot_path, value)
        expect_failure(root, "effectful compile-time filesystem fallback was hidden")

        value = copy.deepcopy(original_snapshot)
        value["implementation"].update(
            {
                "tier": "runtime_complete",
                "status": "qualified",
                "blockers": [],
                "scoped_text_io": True,
                "root_scoped_metadata": True,
                "root_scoped_write": True,
                "typed_paths": True,
                "binary_handles": True,
                "deterministic_traversal": True,
                "atomic_replace": True,
                "secure_temporary_resources": True,
                "runtime_effects_only": True,
                "descriptor_anchored_replace": True,
                "pathname_operations_toctou_safe": True,
            }
        )
        for reference in value["fixtures"]:
            reference["evidence"] = "runtime"
        checker = checker_module()
        checker.validate_schema_node(value, original_schema, "$", original_schema["$defs"])
        write(snapshot_path, value)
        expect_failure(root, "promotion-capable schema bypassed current evidence pin")

        value = json.loads(json.dumps(original_snapshot))
        value["unexpected"] = True
        write(snapshot_path, value)
        expect_failure(root, "unknown snapshot fields were accepted")

        write(snapshot_path, original_snapshot)
        fixture_path = root / "stage1/compiler-contracts/fixtures/filesystem-v1/symlink-swap.json"
        fixture = json.loads(fixture_path.read_text(encoding="utf-8"))
        fixture["assertions"] = ["root escape denied"]
        write(fixture_path, fixture)
        expect_failure(root, "underspecified symlink-race fixture was accepted")

        shutil.copy2(
            ROOT / "stage1/compiler-contracts/fixtures/filesystem-v1/symlink-swap.json",
            fixture_path,
        )
        fixture_path = root / "stage1/compiler-contracts/fixtures/filesystem-v1/authority-denials.json"
        fixture = json.loads(fixture_path.read_text(encoding="utf-8"))
        fixture["cases"][0]["host_io_performed"] = True
        write(fixture_path, fixture)
        expect_failure(root, "authority denial performed host I/O")

        shutil.copy2(
            ROOT / "stage1/compiler-contracts/fixtures/filesystem-v1/authority-denials.json",
            fixture_path,
        )
        fixture_path = (
            root
            / "stage1/compiler-contracts/fixtures/filesystem-v1/partial-binary-write.json"
        )
        fixture = json.loads(fixture_path.read_text(encoding="utf-8"))
        fixture["authorities"] = ["read"]
        write(fixture_path, fixture)
        expect_failure(root, "binary write was certified with read authority")

        shutil.copy2(
            ROOT
            / "stage1/compiler-contracts/fixtures/filesystem-v1/partial-binary-write.json",
            fixture_path,
        )
        fixture = json.loads(fixture_path.read_text(encoding="utf-8"))
        fixture["completed_bytes"] = 9
        write(fixture_path, fixture)
        expect_failure(root, "invalid partial-write byte accounting was accepted")

        fixture = json.loads(
            (ROOT / "stage1/compiler-contracts/fixtures/filesystem-v1/partial-binary-write.json").read_text(
                encoding="utf-8"
            )
        )
        fixture["requested_bytes"] = 1048577
        fixture["remaining_bytes"] = 1048574
        write(fixture_path, fixture)
        expect_failure(root, "positive partial I/O above the published ceiling was accepted")

        shutil.copy2(
            ROOT
            / "stage1/compiler-contracts/fixtures/filesystem-v1/partial-binary-write.json",
            fixture_path,
        )
        fixture_path = root / "stage1/compiler-contracts/fixtures/filesystem-v1/oversize-io.json"
        fixture = json.loads(fixture_path.read_text(encoding="utf-8"))
        fixture["requested_bytes"] = fixture["maximum_request_bytes"]
        write(fixture_path, fixture)
        expect_failure(root, "at-limit request was presented as oversize evidence")

        shutil.copy2(
            ROOT / "stage1/compiler-contracts/fixtures/filesystem-v1/oversize-io.json",
            fixture_path,
        )
        fixture_path = (
            root
            / "stage1/compiler-contracts/fixtures/filesystem-v1/deterministic-directory-order.json"
        )
        fixture = json.loads(fixture_path.read_text(encoding="utf-8"))
        fixture["expected_order"] = list(reversed(fixture["expected_order"]))
        write(fixture_path, fixture)
        expect_failure(root, "host-dependent directory ordering was accepted")

        shutil.copy2(
            ROOT
            / "stage1/compiler-contracts/fixtures/filesystem-v1/deterministic-directory-order.json",
            fixture_path,
        )
        fixture_path = root / "stage1/compiler-contracts/fixtures/filesystem-v1/atomic-replace.json"
        fixture = json.loads(fixture_path.read_text(encoding="utf-8"))
        fixture["phase_outcomes"][-1]["destination"] = "old"
        write(fixture_path, fixture)
        expect_failure(root, "post-commit directory-sync failure preserved the old destination")

        shutil.copy2(
            ROOT / "stage1/compiler-contracts/fixtures/filesystem-v1/atomic-replace.json",
            fixture_path,
        )
        (root / "stage1/compiler-contracts/fixtures/filesystem-v1/unbounded-io.json").unlink()
        expect_failure(root, "missing unbounded-I/O fixture was accepted")

        shutil.copy2(
            ROOT / "stage1/compiler-contracts/fixtures/filesystem-v1/unbounded-io.json",
            root / "stage1/compiler-contracts/fixtures/filesystem-v1/unbounded-io.json",
        )
        readiness_path = root / "docs/production-language-readiness.json"
        readiness = json.loads(readiness_path.read_text(encoding="utf-8"))
        row = next(item for item in readiness["rows"] if item["id"] == "filesystem_resources")
        row["currentTier"] = "runtime_complete"
        row["status"] = "complete"
        write(readiness_path, readiness)
        expect_failure(root, "readiness promotion without executable proof was accepted")

        shutil.copy2(
            ROOT / "docs/production-language-readiness.json",
            readiness_path,
        )
        ledger_path = root / "stage1/compiler-contracts/snapshots/capability-ledger.json"
        ledger = json.loads(ledger_path.read_text(encoding="utf-8"))
        row = next(
            item
            for item in ledger["schemas"]
            if item["name"].endswith("axiom.filesystem.v1.schema.json")
        )
        row["evidenceTier"] = "runtime_complete"
        write(ledger_path, ledger)
        expect_failure(root, "capability-ledger promotion without proof was accepted")

        shutil.copy2(
            ROOT / "stage1/compiler-contracts/snapshots/capability-ledger.json",
            ledger_path,
        )
        doc_path = root / "docs/filesystem-v1.md"
        doc_path.write_text(
            doc_path.read_text(encoding="utf-8").replace(
                "not descriptor-anchored", "fully descriptor-anchored"
            ),
            encoding="utf-8",
        )
        expect_failure(root, "TOCTOU limitation was erased from documentation")

        shutil.copy2(ROOT / "docs/filesystem-v1.md", doc_path)
        runner_path = root / "scripts/ci/run-filesystem-v1-behavioral-tests.sh"
        runner_path.write_text(
            runner_path.read_text(encoding="utf-8").replace(
                "cranelift_backend_denies_fs_", "filesystem_denials_removed"
            ),
            encoding="utf-8",
        )
        expect_failure(root, "executable current-backend denial checks were removed")

        shutil.copy2(
            ROOT / "scripts/ci/run-filesystem-v1-behavioral-tests.sh",
            runner_path,
        )
        doc_path.replace(root / "docs/filesystem-v1.backup.md")
        doc_path.symlink_to("filesystem-v1.backup.md")
        expect_failure(root, "symlinked PR-head evidence was accepted")

    print("Filesystem v1 checker tests passed")


if __name__ == "__main__":
    main()
