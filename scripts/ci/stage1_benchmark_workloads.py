#!/usr/bin/env python3
"""Load the canonical stage1 native-reference benchmark workload set."""

from __future__ import annotations

import json
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path
from typing import Any

SCHEMA_VERSION = "axiom.stage1.benchmark-workloads.v1"


@dataclass(frozen=True)
class Workload:
    name: str
    kind: str
    project: Path
    reference: Path
    expected_lowering_mode: str


def load_workloads(repo_root: Path) -> list[Workload]:
    manifest_path = repo_root / "stage1/benchmarks/workloads.json"
    payload = json.loads(manifest_path.read_text(encoding="utf-8"))
    if payload.get("schema_version") != SCHEMA_VERSION:
        raise ValueError(
            f"{manifest_path} must use schema_version {SCHEMA_VERSION!r}"
        )
    rows = payload.get("workloads")
    if not isinstance(rows, list) or not rows:
        raise ValueError(f"{manifest_path} must declare at least one workload")

    workloads: list[Workload] = []
    seen: set[str] = set()
    for row in rows:
        if not isinstance(row, dict):
            raise ValueError(f"{manifest_path} contains a non-object workload")
        name = row.get("name")
        kind = row.get("kind")
        project = row.get("project")
        reference = row.get("reference")
        expected_lowering_mode = row.get("expected_lowering_mode")
        if not all(
            isinstance(value, str) and value
            for value in (name, kind, project, reference, expected_lowering_mode)
        ):
            raise ValueError(f"{manifest_path} contains an incomplete workload")
        if name in seen:
            raise ValueError(f"{manifest_path} repeats workload {name!r}")
        if kind not in {"compute", "io", "concurrency"}:
            raise ValueError(f"{manifest_path} uses unknown workload kind {kind!r}")
        if expected_lowering_mode not in {
            "direct_native_runtime",
            "bounded_static_output",
        }:
            raise ValueError(
                f"{manifest_path} uses unknown expected lowering mode "
                f"{expected_lowering_mode!r}"
            )
        project_path = _repo_relative_path(repo_root, project, "project")
        reference_path = _repo_relative_path(repo_root, reference, "reference")
        if not (project_path / "axiom.toml").is_file():
            raise ValueError(f"missing workload manifest: {project_path / 'axiom.toml'}")
        for source in ("main.go", "main.rs"):
            if not (reference_path / source).is_file():
                raise ValueError(f"missing reference source: {reference_path / source}")
        seen.add(name)
        workloads.append(
            Workload(
                name,
                kind,
                project_path,
                reference_path,
                expected_lowering_mode,
            )
        )
    return workloads


def _repo_relative_path(repo_root: Path, value: str, label: str) -> Path:
    path = Path(value)
    if path.is_absolute() or ".." in path.parts:
        raise ValueError(f"workload {label} must be a repository-relative path")
    resolved = (repo_root / path).resolve()
    try:
        resolved.relative_to(repo_root.resolve())
    except ValueError as error:
        raise ValueError(f"workload {label} escapes the repository") from error
    return resolved


def build_payload_matches_expected_lowering(
    workload: Workload, payload: Mapping[str, Any]
) -> bool:
    lowering = payload.get("lowering")
    if not isinstance(lowering, dict):
        return False
    expected_direct = workload.expected_lowering_mode == "direct_native_runtime"
    expected_static = workload.expected_lowering_mode == "bounded_static_output"
    return (
        payload.get("ok") is True
        and isinstance(payload.get("binary"), str)
        and bool(payload["binary"])
        and payload.get("generated_rust") is None
        and lowering.get("lowering_mode") == workload.expected_lowering_mode
        and lowering.get("execution_mode") == workload.expected_lowering_mode
        and lowering.get("direct_native_runtime") is expected_direct
        and lowering.get("known_value_static_folds") is expected_static
        and lowering.get("legacy_fallback_attempted") is False
    )


def semantic_outputs_match(
    outputs: Mapping[str, tuple[int, str]],
) -> bool:
    expected_runtimes = {"axiom", "go", "rust"}
    return set(outputs) == expected_runtimes and len(set(outputs.values())) == 1
