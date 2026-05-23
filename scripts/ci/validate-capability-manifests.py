#!/usr/bin/env python3
"""Validate Axiom capability manifest tables with CI-cheap checks."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - exercised on Python < 3.11.
    import tomli as tomllib


BOOL_KEYS = {
    "fs",
    "fs:write",
    "process",
    "env_unrestricted",
    "clock",
    "crypto",
    "ffi",
    "async",
}
KNOWN_KEYS = BOOL_KEYS | {"fs_root", "env", "net", "unsafe_rationale"}


def iter_manifests(root: Path) -> list[Path]:
    return sorted(
        path
        for path in root.rglob("axiom.toml")
        if ".axiom-build" not in path.parts
        and ".git" not in path.parts
        and ".worktrees" not in path.parts
    )


def validate_manifest(path: Path) -> list[str]:
    errors: list[str] = []
    try:
        capabilities = read_capabilities_table(path)
    except (OSError, ValueError) as exc:
        return [f"{path}: failed to parse capability table: {exc}"]

    if capabilities is None:
        return errors

    for key, value in capabilities.items():
        if key not in KNOWN_KEYS:
            errors.append(f"{path}: unknown [capabilities] key {key!r}")
            continue
        if key in BOOL_KEYS and not isinstance(value, bool):
            errors.append(f"{path}: [capabilities].{key} must be a boolean")
        elif key == "fs_root":
            validate_fs_root(path, value, errors)
        elif key == "env":
            validate_env(path, value, errors)
        elif key == "net":
            validate_net(path, value, errors)
        elif key == "unsafe_rationale" and (not isinstance(value, str) or not value.strip()):
            errors.append(f"{path}: [capabilities].unsafe_rationale must be a non-empty string")
    return errors


def read_capabilities_table(path: Path) -> dict[str, object] | None:
    try:
        manifest = tomllib.loads(path.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as exc:
        raise ValueError(str(exc)) from exc

    capabilities = manifest.get("capabilities")
    if capabilities is None:
        return None
    if not isinstance(capabilities, dict):
        raise ValueError("[capabilities] must be a table")
    return capabilities


def validate_fs_root(path: Path, value: object, errors: list[str]) -> None:
    if not isinstance(value, str) or not value.strip():
        errors.append(f"{path}: [capabilities].fs_root must be a non-empty string")
        return
    candidate = Path(value)
    if candidate.is_absolute():
        errors.append(f"{path}: [capabilities].fs_root must be relative")
    if ".." in candidate.parts:
        errors.append(f"{path}: [capabilities].fs_root must not use parent traversal")


def validate_env(path: Path, value: object, errors: list[str]) -> None:
    if isinstance(value, bool):
        return
    if not isinstance(value, list):
        errors.append(f"{path}: [capabilities].env must be a boolean or string list")
        return

    seen: set[str] = set()
    for index, item in enumerate(value):
        field = f"[capabilities].env[{index}]"
        if not isinstance(item, str) or not item.strip():
            errors.append(f"{path}: {field} must be a non-empty string")
            continue
        if "=" in item:
            errors.append(f"{path}: {field} must be a variable name, not NAME=value")
        if item in seen:
            errors.append(f"{path}: duplicate environment allowlist entry at {field}")
        seen.add(item)


def validate_net(path: Path, value: object, errors: list[str]) -> None:
    if isinstance(value, bool):
        return
    if not isinstance(value, dict):
        errors.append(f"{path}: [capabilities].net must be a boolean or allowlist table")
        return

    for key in value:
        if key not in {"hosts", "ports"}:
            errors.append(f"{path}: unknown [capabilities].net key {key!r}")

    hosts = value.get("hosts", [])
    if hosts is not None:
        if not isinstance(hosts, list):
            errors.append(f"{path}: [capabilities].net.hosts must be a string list")
        else:
            seen_hosts: set[str] = set()
            for index, item in enumerate(hosts):
                field = f"[capabilities].net.hosts[{index}]"
                if not isinstance(item, str) or not item.strip():
                    errors.append(f"{path}: {field} must be a non-empty string")
                    continue
                normalized = item.strip().lower()
                if normalized in seen_hosts:
                    errors.append(f"{path}: duplicate network host allowlist entry at {field}")
                seen_hosts.add(normalized)

    ports = value.get("ports", [])
    if ports is not None:
        if not isinstance(ports, list):
            errors.append(f"{path}: [capabilities].net.ports must be an integer list")
        else:
            seen_ports: set[int] = set()
            for index, item in enumerate(ports):
                field = f"[capabilities].net.ports[{index}]"
                if not isinstance(item, int) or isinstance(item, bool):
                    errors.append(f"{path}: {field} must be an integer")
                    continue
                if item < 1 or item > 65535:
                    errors.append(f"{path}: {field} must be between 1 and 65535")
                    continue
                if item in seen_ports:
                    errors.append(f"{path}: duplicate network port allowlist entry at {field}")
                seen_ports.add(item)

    if hosts == [] and ports == []:
        errors.append(
            f"{path}: [capabilities].net must include at least one host or port, or use false"
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path.cwd())
    args = parser.parse_args()

    root = args.root.resolve()
    errors: list[str] = []
    for manifest in iter_manifests(root):
        errors.extend(validate_manifest(manifest))

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1

    print(f"validated capability manifests under {root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
