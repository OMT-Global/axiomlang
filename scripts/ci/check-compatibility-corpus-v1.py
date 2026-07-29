#!/usr/bin/env python3
"""Validate both Compatibility v1 corpora and their honest qualification boundary."""

from __future__ import annotations

import argparse
import importlib.util
import json
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "stage1/compatibility/fixtures"
POLICY = ROOT / "stage1/compatibility/policy-v1.json"
CHECKER = ROOT / "scripts/ci/check-compatibility-v1.py"
EXTRACTOR = ROOT / "scripts/ci/extract-public-contract-v1.py"
CORPORA = {
    "current": ("current_source_snapshot", "workspace_source"),
    "accepted-baseline": ("accepted_source_baseline", "source_contract_only"),
    "previous-contract-fixture": ("previous_contract_fixture", "no_compiler_association"),
}
QUALIFICATION = {
    "current": "Source and fixture integrity only; this corpus is not evidence of a published or previous compiler.",
    "accepted-baseline": "Frozen full-surface source-contract ratchet only; this corpus is not release history or evidence of a previous compiler.",
    "previous-contract-fixture": "Synthetic prior contract input for deterministic semantic diff tests only; it is not a released or previously supported AxiOM compiler.",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixtures-root", type=Path, default=FIXTURES)
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def load_object(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"cannot read {path}: {error}") from error
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain an object")
    return value


def checker_module() -> Any:
    spec = importlib.util.spec_from_file_location("axiom_compatibility_checker", CHECKER)
    if spec is None or spec.loader is None:
        raise ValueError("cannot load Compatibility v1 checker")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def validate_artifact(payload: dict[str, Any], label: str) -> None:
    expected = {
        "schema_version",
        "ok",
        "command",
        "project",
        "package",
        "artifacts",
    }
    if set(payload) != expected:
        raise ValueError(f"{label} artifact envelope fields drifted")
    if (
        payload["schema_version"] != "axiom.artifacts.v0"
        or payload["command"] != "inspect artifacts"
        or payload["ok"] is not True
    ):
        raise ValueError(f"{label} artifact envelope has invalid identity")
    artifacts = payload.get("artifacts")
    if not isinstance(artifacts, list) or not artifacts:
        raise ValueError(f"{label} artifact envelope must contain an artifact")
    for artifact in artifacts:
        if not isinstance(artifact, dict):
            raise ValueError(f"{label} artifact must be an object")
        if set(artifact) != {"id", "kind", "path", "generated_from", "status"}:
            raise ValueError(f"{label} artifact fields drifted")
        if not str(artifact["id"]).startswith("axiom://package/"):
            raise ValueError(f"{label} artifact must use an AxiOM package identity")


def validate_corpus(
    fixtures_root: Path,
    name: str,
    expected_role: str,
    expected_compiler_status: str,
    checker: Any,
    policy: dict[str, Any],
) -> dict[str, Any]:
    directory = fixtures_root / name
    required = [
        "metadata.json",
        "contract.json",
        "axiom.toml",
        "axiom.lock",
        "src/main.ax",
        "schema/representative.schema.json",
        "artifacts/representative.json",
    ]
    if name == "accepted-baseline":
        required.append("policy.json")
    missing = [relative for relative in required if not (directory / relative).is_file()]
    if missing:
        raise ValueError(f"{name} corpus missing required files: {', '.join(missing)}")

    metadata = load_object(directory / "metadata.json")
    expected_metadata_keys = {
        "schema_version",
        "role",
        "compiler",
        "edition",
        "contract",
        "qualification",
    }
    if set(metadata) != expected_metadata_keys:
        raise ValueError(f"{name} metadata fields drifted")
    if metadata.get("schema_version") != "axiom.compatibility_corpus.v1":
        raise ValueError(f"{name} metadata has an unsupported schema_version")
    if metadata.get("role") != expected_role:
        raise ValueError(f"{name} corpus role must be {expected_role}")
    compiler = metadata.get("compiler")
    if not isinstance(compiler, dict) or set(compiler) != {
        "status",
        "version",
        "released",
        "qualified_previous",
    }:
        raise ValueError(f"{name} compiler metadata fields drifted")
    if compiler.get("status") != expected_compiler_status:
        raise ValueError(f"{name} compiler status must be {expected_compiler_status}")
    if compiler.get("released") is not False or compiler.get("qualified_previous") is not False:
        raise ValueError(f"{name} must not claim a released or qualified previous compiler")
    edition_metadata = metadata.get("edition")
    if not isinstance(edition_metadata, dict) or set(edition_metadata) != {
        "id",
        "policy_status",
        "manifest_selection",
    }:
        raise ValueError(f"{name} edition metadata fields drifted")
    if metadata.get("contract") != "contract.json":
        raise ValueError(f"{name} metadata contract must be contract.json")
    if metadata.get("qualification") != QUALIFICATION[name]:
        raise ValueError(f"{name} qualification disclaimer drifted")
    if name == "previous-contract-fixture":
        if compiler.get("version") is not None:
            raise ValueError("previous contract fixture must not claim a compiler or release")

    manifest = tomllib.loads((directory / "axiom.toml").read_text(encoding="utf-8"))
    package = manifest.get("package")
    if not isinstance(package, dict) or set(package) != {"name", "version"}:
        raise ValueError(f"{name} corpus manifest must have only package name/version")
    if "edition" in manifest or "edition" in package:
        raise ValueError(f"{name} corpus must not pretend manifest edition selection exists")
    lock = tomllib.loads((directory / "axiom.lock").read_text(encoding="utf-8"))
    lock_packages = lock.get("package")
    if lock.get("version") != 1 or not isinstance(lock_packages, list) or len(lock_packages) != 1:
        raise ValueError(f"{name} corpus lockfile must use format 1 with one package")
    locked = lock_packages[0]
    if (
        not isinstance(locked, dict)
        or locked.get("name") != package.get("name")
        or locked.get("version") != package.get("version")
        or locked.get("source") != "path"
    ):
        raise ValueError(f"{name} corpus manifest and lockfile disagree")

    source = (directory / "src/main.ax").read_text(encoding="utf-8")
    if "fn main(): int" not in source:
        raise ValueError(f"{name} corpus source must expose an AxiOM main function")
    representative_schema = load_object(directory / "schema/representative.schema.json")
    if (
        not isinstance(representative_schema.get("$id"), str)
        or representative_schema.get("additionalProperties") is not False
        or not isinstance(representative_schema.get("required"), list)
    ):
        raise ValueError(f"{name} representative schema is not closed and identified")
    validate_artifact(load_object(directory / "artifacts/representative.json"), name)

    selected_policy = (
        load_object(directory / "policy.json")
        if name == "accepted-baseline"
        else policy
    )
    checker.validate_policy(selected_policy)
    contract = load_object(directory / metadata["contract"])
    checker.validate_contract(
        contract,
        name,
        selected_policy,
        current_policy=name in {"current", "accepted-baseline"},
    )
    expected_snapshot = (
        "axiom://compatibility/current-source-contract"
        if name == "current"
        else (
            "axiom://compatibility/accepted-source-baseline-v1"
            if name == "accepted-baseline"
            else "axiom://compatibility/previous-contract-fixture"
        )
    )
    if contract.get("snapshot_id") != expected_snapshot:
        raise ValueError(f"{name} contract has the wrong fixture identity")
    current_edition = selected_policy["editions"]["current"]
    edition_rows = {
        row["id"]: row
        for row in selected_policy["editions"]["supported"]
        if isinstance(row, dict)
    }
    if (
        edition_metadata.get("id") != current_edition
        or edition_metadata.get("id") != contract["edition"]["id"]
        or edition_metadata.get("policy_status") != edition_rows[current_edition]["status"]
        or edition_metadata.get("policy_status") != contract["edition"]["status"]
        or edition_metadata.get("manifest_selection") != "unavailable"
    ):
        raise ValueError(f"{name} edition metadata contradicts policy or contract")
    if name == "current":
        cargo = tomllib.loads((ROOT / "stage1/Cargo.toml").read_text(encoding="utf-8"))
        cargo_version = cargo["workspace"]["package"]["version"]
        if (
            compiler.get("version") != cargo_version
            or compiler.get("version") != selected_policy["compiler_support"]["current"]
            or compiler.get("version") != contract["compiler"]["current"]
            or compiler.get("released")
            != selected_policy["release_state"]["published_compiler_releases"]
        ):
            raise ValueError(f"{name} compiler metadata contradicts Cargo, policy, or contract")
    elif name == "accepted-baseline":
        if (
            compiler.get("version") != selected_policy["compiler_support"]["current"]
            or compiler.get("version") != contract["compiler"]["current"]
            or compiler.get("released")
            != selected_policy["release_state"]["published_compiler_releases"]
        ):
            raise ValueError(
                "accepted-baseline compiler metadata contradicts its frozen policy or contract"
            )
    return {
        "contract_version": contract["contract_version"],
        "files": len(required),
        "role": metadata["role"],
    }


def main() -> int:
    args = parse_args()
    try:
        checker = checker_module()
        policy = load_object(POLICY)
        checker.validate_policy(policy)
        corpora = {
            name: validate_corpus(
                args.fixtures_root,
                name,
                expected_role,
                expected_compiler_status,
                checker,
                policy,
            )
            for name, (expected_role, expected_compiler_status) in CORPORA.items()
        }
        if args.fixtures_root.resolve() == FIXTURES.resolve():
            extraction = subprocess.run(
                [sys.executable, str(EXTRACTOR), "--check"],
                cwd=ROOT,
                text=True,
                capture_output=True,
                check=False,
            )
            if extraction.returncode != 0:
                raise ValueError(
                    "current contract is not byte-identical to source extraction: "
                    + extraction.stdout
                    + extraction.stderr
                )
        report = {
            "corpora": corpora,
            "ok": True,
            "previous_compiler_qualified": False,
            "schema_version": "axiom.compatibility_corpus_check.v1",
        }
    except (OSError, ValueError, tomllib.TOMLDecodeError) as error:
        report = {
            "error": str(error),
            "ok": False,
            "schema_version": "axiom.compatibility_corpus_check.v1",
        }
        print(json.dumps(report, indent=2, sort_keys=True) if args.json else f"compatibility corpus: fail\n- {error}")
        return 1
    print(json.dumps(report, indent=2, sort_keys=True) if args.json else "compatibility corpus: pass")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
