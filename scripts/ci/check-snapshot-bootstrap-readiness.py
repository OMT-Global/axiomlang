#!/usr/bin/env python3
"""Validate the self-hosting snapshot chain and its offline evidence."""

import argparse
import hashlib
import json
import re
from pathlib import Path


SCHEMA = "axiom.self_hosting.snapshot_bootstrap_readiness.v0"
DEFAULT_SNAPSHOT_SCHEMA_PATH = Path(
    "stage1/schemas/axiom-selfhost-snapshot-manifest-v0.schema.json"
)
DEFAULT_PROVENANCE_SCHEMA_PATH = Path(
    "stage1/schemas/axiom-selfhost-snapshot-provenance-v0.schema.json"
)
VALID_STATUSES = {"implemented", "partial", "blocked"}
HEX_SHA256 = re.compile(r"^[0-9a-f]{64}$")
HEX_SHA1 = re.compile(r"^[0-9a-f]{40}$")
COMMAND_TOOL = re.compile(r"(^|[^a-z0-9])(cargo|rustc)([^a-z0-9]|$)", re.IGNORECASE)


def check(name, status, detail):
    return {"name": name, "status": status, "detail": detail}


def load_json(path):
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def validate_schema_node(value, schema, path, defs):
    if "$ref" in schema:
        prefix = "#/$defs/"
        ref = schema["$ref"]
        if not ref.startswith(prefix):
            raise ValueError(f"{path} uses unsupported schema ref {ref!r}")
        name = ref[len(prefix) :]
        if name not in defs:
            raise ValueError(f"{path} references unknown schema def {name!r}")
        validate_schema_node(value, defs[name], path, defs)
        return

    if "const" in schema and value != schema["const"]:
        raise ValueError(f"{path} must equal {schema['const']!r}")
    if "enum" in schema and value not in schema["enum"]:
        raise ValueError(f"{path} must be one of {schema['enum']!r}")

    expected_type = schema.get("type")
    expected_types = (
        expected_type if isinstance(expected_type, list) else [expected_type]
    )
    if expected_type is not None:
        type_matches = {
            "object": isinstance(value, dict),
            "array": isinstance(value, list),
            "string": isinstance(value, str),
            "integer": isinstance(value, int) and not isinstance(value, bool),
            "boolean": isinstance(value, bool),
            "null": value is None,
        }
        if not any(type_matches.get(item, False) for item in expected_types):
            raise ValueError(f"{path} must be of type {expected_types!r}")

    if isinstance(value, dict) and "object" in expected_types:
        required = set(schema.get("required", []))
        missing = sorted(required - set(value))
        if missing:
            raise ValueError(f"{path} missing required fields: {', '.join(missing)}")
        properties = schema.get("properties", {})
        if schema.get("additionalProperties") is False:
            extra = sorted(set(value) - set(properties))
            if extra:
                raise ValueError(f"{path} has unexpected fields: {', '.join(extra)}")
        for key, nested in value.items():
            if key in properties:
                validate_schema_node(nested, properties[key], f"{path}.{key}", defs)
    elif isinstance(value, list) and "array" in expected_types:
        minimum = schema.get("minItems")
        if minimum is not None and len(value) < minimum:
            raise ValueError(f"{path} must contain at least {minimum} items")
        item_schema = schema.get("items")
        if item_schema:
            for index, item in enumerate(value):
                validate_schema_node(item, item_schema, f"{path}[{index}]", defs)
    elif isinstance(value, str) and "string" in expected_types:
        if "minLength" in schema and len(value) < schema["minLength"]:
            raise ValueError(f"{path} must not be empty")
        if "pattern" in schema and not re.fullmatch(schema["pattern"], value):
            raise ValueError(f"{path} must match pattern {schema['pattern']!r}")
    elif "integer" in expected_types and isinstance(value, int):
        if "minimum" in schema and value < schema["minimum"]:
            raise ValueError(f"{path} must be at least {schema['minimum']}")


def validate_against_schema(value, schema):
    validate_schema_node(value, schema, "$", schema.get("$defs", {}))


def is_zero_digest(value):
    return isinstance(value, str) and HEX_SHA256.fullmatch(value) and set(value) == {"0"}


def is_zero_head(value):
    return isinstance(value, str) and HEX_SHA1.fullmatch(value) and set(value) == {"0"}


def resolve_evidence_path(manifest_path, raw_path):
    path = Path(raw_path)
    return path if path.is_absolute() else manifest_path.parent / path


def file_sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def validate_evidence_file(path, expected_digest, label):
    if not path.is_file() or path.is_symlink():
        return None, check(label, "fail", f"{path} is missing or not a regular file")
    try:
        actual_digest = file_sha256(path)
    except OSError as error:
        return None, check(label, "fail", f"cannot read {path}: {error}")
    if actual_digest != expected_digest:
        return None, check(
            label,
            "fail",
            f"{path} digest {actual_digest} does not match pinned {expected_digest}",
        )
    try:
        payload = load_json(path)
    except (OSError, json.JSONDecodeError) as error:
        return None, check(label, "fail", f"{path} is not valid JSON: {error}")
    return payload, check(label, "pass", f"{path} exists and matches its pinned digest")


def canonical_chain_id(snapshot):
    return f"{snapshot.get('source')}@{snapshot.get('target')}"


def canonical_snapshot_id(snapshot):
    return f"{snapshot.get('chain_id')}@{snapshot.get('sequence')}"


def command_mentions_cargo_or_rustc(execution):
    commands = execution.get("command", []) + execution.get("processes", [])
    return any(COMMAND_TOOL.search(str(command)) for command in commands)


def validate_snapshot_manifest(path, schema_path, provenance_schema_path):
    checks = []
    if not path.is_file():
        return [check("snapshot_manifest_present", "fail", f"{path} is missing")], []
    checks.append(check("snapshot_manifest_present", "pass", f"{path} exists"))
    try:
        payload = load_json(path)
    except (OSError, json.JSONDecodeError) as error:
        return checks + [check("snapshot_manifest_json", "fail", str(error))], []
    checks.append(check("snapshot_manifest_json", "pass", "snapshot manifest is valid JSON"))

    schema_valid = False
    if not schema_path.is_file():
        checks.append(check("snapshot_manifest_schema", "fail", f"{schema_path} is missing"))
    else:
        try:
            schema = load_json(schema_path)
            validate_against_schema(payload, schema)
            schema_valid = True
            checks.append(check("snapshot_manifest_schema", "pass", f"{path} matches {schema_path}"))
        except (OSError, json.JSONDecodeError, ValueError) as error:
            checks.append(check("snapshot_manifest_schema", "fail", str(error)))

    snapshots = payload.get("snapshots")
    if not isinstance(snapshots, list):
        checks.append(check("snapshot_manifest_snapshots", "fail", "snapshots must be an array"))
        return checks, []
    checks.append(check("snapshot_manifest_snapshots", "pass", f"manifest contains {len(snapshots)} snapshots"))
    if not schema_valid:
        return checks, snapshots

    by_id = {}
    by_target = {}
    for index, snapshot in enumerate(snapshots):
        name = f"snapshot_manifest_entry_{index}"
        if not isinstance(snapshot, dict):
            checks.append(check(name, "fail", "snapshot entry must be an object"))
            continue
        snapshot_id = snapshot.get("snapshot_id", f"entry-{index}")
        entry_errors = []
        if snapshot.get("sha256") and is_zero_digest(snapshot["sha256"]):
            entry_errors.append("artifact sha256 must not be all zeroes")
        if snapshot.get("source_head_sha") and is_zero_head(snapshot["source_head_sha"]):
            entry_errors.append("source head sha must not be all zeroes")
        if snapshot.get("chain_id") != canonical_chain_id(snapshot):
            entry_errors.append("chain_id does not bind the exact source and target")
        if snapshot.get("snapshot_id") != canonical_snapshot_id(snapshot):
            entry_errors.append("snapshot_id does not bind the chain and sequence")
        if snapshot.get("snapshot_id") in by_id:
            entry_errors.append("snapshot_id is duplicated")
        else:
            by_id[snapshot.get("snapshot_id")] = snapshot
        target_key = snapshot.get("target")
        identity = (snapshot.get("source"), snapshot.get("chain_id"))
        if target_key in by_target and by_target[target_key] != identity:
            entry_errors.append("target is associated with multiple chain identities")
        else:
            by_target[target_key] = identity
        if snapshot.get("sequence") == 0:
            if snapshot.get("built_by") != "cargo":
                entry_errors.append("sequence zero must be the cargo genesis")
            if snapshot.get("predecessor") is not None:
                entry_errors.append("genesis snapshot must not have a predecessor")
        elif snapshot.get("built_by") != "axiomc-snapshot":
            entry_errors.append("non-genesis snapshots must be built_by axiomc-snapshot")
        if snapshot.get("sequence", 0) > 0 and not snapshot.get("predecessor"):
            entry_errors.append("non-genesis snapshot must name a predecessor")
        if entry_errors:
            checks.append(check(name, "fail", "; ".join(entry_errors)))
        else:
            checks.append(check(name, "pass", f"{snapshot_id} has a valid manifest identity"))

    valid_entries = [item for item in snapshots if isinstance(item, dict)]
    for target in sorted({item.get("target") for item in valid_entries}):
        target_entries = sorted(
            [item for item in valid_entries if item.get("target") == target],
            key=lambda item: item.get("sequence", -1),
        )
        sequences = [item.get("sequence") for item in target_entries]
        expected = list(range(len(target_entries)))
        if sequences != expected:
            checks.append(
                check(
                    f"snapshot_chain_order_{target}",
                    "fail",
                    f"target chain sequences {sequences!r} are not contiguous from zero",
                )
            )
        else:
            checks.append(check(f"snapshot_chain_order_{target}", "pass", f"{target} chain is ordered"))

        for snapshot in target_entries:
            sequence = snapshot.get("sequence")
            expected_predecessor = None
            if isinstance(sequence, int) and sequence > 0:
                previous = next(
                    (item for item in target_entries if item.get("sequence") == sequence - 1),
                    None,
                )
                expected_predecessor = previous.get("snapshot_id") if previous else "missing"
            actual_predecessor = snapshot.get("predecessor")
            if actual_predecessor != expected_predecessor:
                checks.append(
                    check(
                        f"snapshot_predecessor_{snapshot.get('snapshot_id', 'unknown')}",
                        "fail",
                        f"predecessor {actual_predecessor!r} does not equal {expected_predecessor!r}",
                    )
                )
            else:
                checks.append(
                    check(
                        f"snapshot_predecessor_{snapshot.get('snapshot_id', 'unknown')}",
                        "pass",
                        "predecessor is continuous for the target chain",
                    )
                )

    if not valid_entries:
        return checks, snapshots

    provenance_schema = None
    if not provenance_schema_path.is_file():
        checks.append(check("snapshot_provenance_schema", "fail", f"{provenance_schema_path} is missing"))
    else:
        try:
            provenance_schema = load_json(provenance_schema_path)
            checks.append(check("snapshot_provenance_schema", "pass", f"loaded {provenance_schema_path}"))
        except (OSError, json.JSONDecodeError) as error:
            checks.append(check("snapshot_provenance_schema", "fail", str(error)))

    post_genesis_cargo_failures = []
    for index, snapshot in enumerate(valid_entries):
        snapshot_id = snapshot.get("snapshot_id", f"entry-{index}")
        prefix = f"snapshot_{snapshot_id}"
        artifact_path = resolve_evidence_path(path, snapshot.get("artifact_path", ""))
        if not artifact_path.is_file() or artifact_path.is_symlink():
            checks.append(check(f"{prefix}_artifact_present", "fail", f"{artifact_path} is missing or not a regular file"))
        elif is_zero_digest(snapshot.get("sha256")):
            checks.append(check(f"{prefix}_artifact_digest", "fail", "artifact digest must not be all zeroes"))
        else:
            try:
                actual = file_sha256(artifact_path)
            except OSError as error:
                checks.append(check(f"{prefix}_artifact_digest", "fail", str(error)))
            else:
                checks.append(
                    check(
                        f"{prefix}_artifact_digest",
                        "pass" if actual == snapshot.get("sha256") else "fail",
                        f"artifact digest is {actual}, pinned {snapshot.get('sha256')}",
                    )
                )

        provenance_path = resolve_evidence_path(path, snapshot.get("provenance", ""))
        provenance, provenance_check = validate_evidence_file(
            provenance_path, snapshot.get("provenance_sha256", ""), f"{prefix}_provenance"
        )
        checks.append(provenance_check)
        if provenance is None:
            continue
        if provenance_schema is not None:
            try:
                validate_against_schema(provenance, provenance_schema)
                checks.append(check(f"{prefix}_provenance_schema", "pass", "provenance evidence matches its schema"))
            except ValueError as error:
                checks.append(check(f"{prefix}_provenance_schema", "fail", str(error)))
                continue

        binding_fields = [
            "snapshot_id",
            "chain_id",
            "sequence",
            "version",
            "target",
            "source",
            "source_head_sha",
            "built_by",
            "predecessor",
        ]
        binding_errors = [
            f"{field} does not match manifest"
            for field in binding_fields
            if provenance.get(field) != snapshot.get(field)
        ]
        if provenance.get("artifact_sha256") != snapshot.get("sha256"):
            binding_errors.append("artifact_sha256 does not match manifest sha256")
        if is_zero_digest(provenance.get("artifact_sha256")):
            binding_errors.append("provenance artifact digest must not be all zeroes")
        if binding_errors:
            checks.append(check(f"{prefix}_binding", "fail", "; ".join(binding_errors)))
        else:
            checks.append(check(f"{prefix}_binding", "pass", "source, head, identity, predecessor, and artifact are bound"))

        predecessor = snapshot.get("predecessor")
        predecessor_entry = next((item for item in valid_entries if item.get("snapshot_id") == predecessor), None)
        expected_predecessor_digest = predecessor_entry.get("sha256") if predecessor_entry else None
        if provenance.get("predecessor_artifact_sha256") != expected_predecessor_digest:
            checks.append(
                check(
                    f"{prefix}_predecessor_artifact",
                    "fail",
                    "provenance predecessor artifact digest is not continuous",
                )
            )
        elif is_zero_digest(provenance.get("predecessor_artifact_sha256")):
            checks.append(check(f"{prefix}_predecessor_artifact", "fail", "predecessor digest must not be all zeroes"))
        else:
            checks.append(check(f"{prefix}_predecessor_artifact", "pass", "predecessor artifact digest is continuous"))

        execution = provenance.get("execution", {})
        execution_binding = [
            field
            for field in [
                "snapshot_id",
                "target",
                "source",
                "source_head_sha",
                "predecessor",
                "version",
            ]
            if execution.get(field) != snapshot.get(field)
        ]
        if execution_binding:
            checks.append(check(f"{prefix}_execution_binding", "fail", "execution evidence does not bind " + ", ".join(execution_binding)))
        else:
            checks.append(check(f"{prefix}_execution_binding", "pass", "execution evidence binds source and target identity"))

        if execution.get("predecessor_artifact_sha256") != expected_predecessor_digest:
            checks.append(
                check(
                    f"{prefix}_execution_predecessor_artifact",
                    "fail",
                    "execution predecessor artifact digest is not continuous",
                )
            )
        else:
            checks.append(
                check(
                    f"{prefix}_execution_predecessor_artifact",
                    "pass",
                    "execution predecessor artifact digest is continuous",
                )
            )

        offline_ok = (
            execution.get("offline") is True
            and execution.get("network_access") is False
            and "--offline" in " ".join(execution.get("command", []))
            and "--locked" in " ".join(execution.get("command", []))
        )
        checks.append(
            check(
                f"{prefix}_offline_execution",
                "pass" if offline_ok else "fail",
                "execution is explicitly locked and offline" if offline_ok else "execution lacks locked offline evidence",
            )
        )

        cargo_rustc = command_mentions_cargo_or_rustc(execution)
        allowed_genesis_bootstrap = snapshot.get("sequence") == 0 and snapshot.get("built_by") == "cargo"
        no_cargo_ok = not cargo_rustc or allowed_genesis_bootstrap
        if snapshot.get("sequence", 0) > 0 and not no_cargo_ok:
            post_genesis_cargo_failures.append(snapshot_id)
        checks.append(
            check(
                f"{prefix}_no_cargo_rustc",
                "pass" if no_cargo_ok else "fail",
                "Cargo/rustc is limited to the genesis bootstrap" if no_cargo_ok else "post-genesis execution invokes Cargo or rustc",
            )
        )

        output = execution.get("output", {})
        output_ok = (
            output.get("status") == "pass"
            and output.get("divergent") is False
            and output.get("artifact_sha256") == snapshot.get("sha256")
        )
        checks.append(
            check(
                f"{prefix}_output",
                "pass" if output_ok else "fail",
                "output is verified against the content digest" if output_ok else "output is missing, divergent, or digest-mismatched",
            )
        )

        fixpoint = execution.get("fixpoint", {})
        sequence = snapshot.get("sequence")
        if sequence == 0:
            fixpoint_ok = (
                fixpoint.get("status") == "not_applicable"
                and fixpoint.get("normalized_equal") is None
                and fixpoint.get("first_sha256") is None
                and fixpoint.get("second_sha256") is None
            )
        else:
            fixpoint_ok = (
                fixpoint.get("status") == "pass"
                and fixpoint.get("normalized_equal") is True
                and fixpoint.get("first_sha256") == snapshot.get("sha256")
                and fixpoint.get("second_sha256") == snapshot.get("sha256")
                and not is_zero_digest(fixpoint.get("first_sha256"))
            )
        checks.append(
            check(
                f"{prefix}_fixpoint",
                "pass" if fixpoint_ok else "fail",
                "fixpoint evidence is valid" if fixpoint_ok else "fixpoint is absent, divergent, or failed",
            )
        )

    checks.append(
        check(
            "snapshot_no_cargo_after_genesis",
            "fail" if post_genesis_cargo_failures else "pass",
            (
                "post-genesis entries invoke Cargo or rustc: "
                + ", ".join(post_genesis_cargo_failures)
                if post_genesis_cargo_failures
                else "post-genesis entries contain no Cargo or rustc invocation"
            ),
        )
    )
    return checks, snapshots


def main():
    parser = argparse.ArgumentParser(description="Check snapshot bootstrap readiness.")
    parser.add_argument("--json", action="store_true", help="emit JSON output")
    parser.add_argument("--manifest", default="docs/snapshot-bootstrap-readiness.json")
    parser.add_argument("--snapshot-manifest")
    parser.add_argument("--snapshot-schema", default=str(DEFAULT_SNAPSHOT_SCHEMA_PATH))
    parser.add_argument("--provenance-schema", default=str(DEFAULT_PROVENANCE_SCHEMA_PATH))
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
        except (OSError, json.JSONDecodeError) as error:
            payload = {}
            checks.append(check("snapshot_readiness_manifest_json", "fail", str(error)))

    checks.append(check("snapshot_readiness_schema", "pass" if payload.get("schema") == SCHEMA else "fail", f"manifest schema is {payload.get('schema')!r}"))
    rows = payload.get("rows", []) if isinstance(payload.get("rows"), list) else []
    checks.append(check("snapshot_readiness_rows_present", "pass" if rows else "fail", f"manifest contains {len(rows)} rows"))
    for index, row in enumerate(rows):
        if not isinstance(row, dict):
            checks.append(check(f"snapshot_readiness_row_{index}", "fail", "readiness row must be an object"))
            continue
        row_id = row.get("id", "unknown")
        status = row.get("status")
        if status not in VALID_STATUSES:
            checks.append(check(f"snapshot_readiness_row_{row_id}", "fail", f"invalid status {status!r}"))
        elif status == "implemented" and not row.get("validatingCommand"):
            checks.append(check(f"snapshot_readiness_row_{row_id}", "fail", "implemented rows require validatingCommand"))
        else:
            checks.append(check(f"snapshot_readiness_row_{row_id}", "pass", f"row status is {status}"))

    snapshot_manifest = Path(args.snapshot_manifest or payload.get("snapshotManifest", "stage1/snapshots/manifest.json"))
    snapshot_checks, snapshots = validate_snapshot_manifest(
        snapshot_manifest, Path(args.snapshot_schema), Path(args.provenance_schema)
    )
    checks.extend(snapshot_checks)
    checks.append(check("snapshot_available", "pass" if snapshots else "fail", "at least one snapshot is pinned" if snapshots else "no snapshot is pinned yet"))

    all_rows_implemented = bool(rows) and all(
        isinstance(row, dict) and row.get("status") == "implemented" for row in rows
    )
    ready = bool(all_rows_implemented and snapshots and all(item["status"] == "pass" for item in checks))
    output = {
        "schema": SCHEMA,
        "ready": ready,
        "snapshot_manifest": str(snapshot_manifest),
        "checks": checks,
        "rows": [
            {
                "id": row.get("id") if isinstance(row, dict) else None,
                "status": row.get("status") if isinstance(row, dict) else None,
                "governing_issue": row.get("governingIssue") if isinstance(row, dict) else None,
                "blocker_issues": row.get("blockerIssues", []) if isinstance(row, dict) else [],
                "validating_command": row.get("validatingCommand") if isinstance(row, dict) else None,
            }
            for row in rows
        ],
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
