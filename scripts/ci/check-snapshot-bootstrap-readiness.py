#!/usr/bin/env python3
import argparse
import json
from pathlib import Path

SCHEMA = "axiom.self_hosting.snapshot_bootstrap_readiness.v0"
SNAPSHOT_SCHEMA = "axiom.selfhost.snapshot_manifest.v0"
VALID_STATUSES = {"implemented", "partial", "blocked"}


def check(name, status, detail):
    return {"name": name, "status": status, "detail": detail}


def load_json(path):
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def validate_snapshot_manifest(path):
    checks = []
    if not path.is_file():
        return [check("snapshot_manifest_present", "fail", f"{path} is missing")], []
    checks.append(check("snapshot_manifest_present", "pass", f"{path} exists"))
    try:
        payload = load_json(path)
    except json.JSONDecodeError as error:
        return checks + [check("snapshot_manifest_json", "fail", str(error))], []
    checks.append(check("snapshot_manifest_json", "pass", "snapshot manifest is valid JSON"))
    checks.append(check(
        "snapshot_manifest_schema",
        "pass" if payload.get("schema_version") == SNAPSHOT_SCHEMA else "fail",
        f"schema_version is {payload.get('schema_version')!r}",
    ))
    snapshots = payload.get("snapshots")
    if not isinstance(snapshots, list):
        checks.append(check("snapshot_manifest_snapshots", "fail", "snapshots must be an array"))
        return checks, []
    checks.append(check("snapshot_manifest_snapshots", "pass", f"manifest contains {len(snapshots)} snapshots"))
    cargo_targets = set()
    for index, snapshot in enumerate(snapshots):
        name = f"snapshot_manifest_entry_{index}"
        if not isinstance(snapshot, dict):
            checks.append(check(name, "fail", "snapshot entry must be an object"))
            continue
        missing = [field for field in ["version", "target", "sha256", "source", "built_by", "provenance"] if not snapshot.get(field)]
        if missing:
            checks.append(check(name, "fail", "missing fields: " + ", ".join(missing)))
            continue
        if snapshot["built_by"] not in {"cargo", "axiomc-snapshot"}:
            checks.append(check(name, "fail", "built_by must be cargo or axiomc-snapshot"))
            continue
        if snapshot["built_by"] == "cargo":
            if snapshot["target"] in cargo_targets:
                checks.append(check(name, "fail", f"multiple cargo genesis snapshots for {snapshot['target']}"))
                continue
            cargo_targets.add(snapshot["target"])
        checks.append(check(name, "pass", f"{snapshot['version']} for {snapshot['target']} is well formed"))
    return checks, snapshots


def main():
    parser = argparse.ArgumentParser(description="Check snapshot bootstrap readiness.")
    parser.add_argument("--json", action="store_true", help="emit JSON output")
    parser.add_argument("--manifest", default="docs/snapshot-bootstrap-readiness.json")
    parser.add_argument("--snapshot-manifest")
    args = parser.parse_args()

    manifest_path = Path(args.manifest)
    checks = []
    if not manifest_path.is_file():
        payload = {}
        checks.append(check("snapshot_readiness_manifest_present", "fail", f"{manifest_path} is missing"))
    else:
        checks.append(check("snapshot_readiness_manifest_present", "pass", f"{manifest_path} exists"))
        try:
            payload = load_json(manifest_path)
            checks.append(check("snapshot_readiness_manifest_json", "pass", "readiness manifest is valid JSON"))
        except json.JSONDecodeError as error:
            payload = {}
            checks.append(check("snapshot_readiness_manifest_json", "fail", str(error)))

    checks.append(check("snapshot_readiness_schema", "pass" if payload.get("schema") == SCHEMA else "fail", f"manifest schema is {payload.get('schema')!r}"))
    rows = payload.get("rows", []) if isinstance(payload.get("rows"), list) else []
    checks.append(check("snapshot_readiness_rows_present", "pass" if rows else "fail", f"manifest contains {len(rows)} rows"))
    for row in rows:
        row_id = row.get("id", "unknown")
        status = row.get("status")
        if status not in VALID_STATUSES:
            checks.append(check(f"snapshot_readiness_row_{row_id}", "fail", f"invalid status {status!r}"))
        elif status == "implemented" and not row.get("validatingCommand"):
            checks.append(check(f"snapshot_readiness_row_{row_id}", "fail", "implemented rows require validatingCommand"))
        else:
            checks.append(check(f"snapshot_readiness_row_{row_id}", "pass", f"row status is {status}"))

    snapshot_manifest = Path(args.snapshot_manifest or payload.get("snapshotManifest", "stage1/snapshots/manifest.json"))
    snapshot_checks, snapshots = validate_snapshot_manifest(snapshot_manifest)
    checks.extend(snapshot_checks)
    checks.append(check("snapshot_available", "pass" if snapshots else "fail", "at least one snapshot is pinned" if snapshots else "no snapshot is pinned yet"))

    all_rows_implemented = bool(rows) and all(row.get("status") == "implemented" for row in rows)
    ready = bool(all_rows_implemented and snapshots and all(item["status"] == "pass" for item in checks))
    output = {
        "schema": SCHEMA,
        "ready": ready,
        "snapshot_manifest": str(snapshot_manifest),
        "checks": checks,
        "rows": [{"id": row.get("id"), "status": row.get("status"), "governing_issue": row.get("governingIssue"), "blocker_issues": row.get("blockerIssues", []), "validating_command": row.get("validatingCommand")} for row in rows],
    }
    if args.json:
        print(json.dumps(output, indent=2, sort_keys=True))
    elif ready:
        print("Snapshot bootstrap readiness: ready")
    else:
        print("Snapshot bootstrap readiness: blocked")
    return 0 if ready else 1


if __name__ == "__main__":
    raise SystemExit(main())
