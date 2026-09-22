#!/usr/bin/env python3
"""Validate Compiler Native Backend and Runtime v1 without authorizing cutover."""

from __future__ import annotations

import argparse
import json
import os
import stat
import re
import sys
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
MAX_READ_BYTES = 1024 * 1024
SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.compiler_native_backend_runtime.v1.schema.json")
SNAPSHOT = Path("stage1/compiler-contracts/snapshots/compiler-native-backend-runtime-v1.json")
PRODUCTION_READINESS = Path("docs/production-language-readiness.json")
MIR_BACKEND = Path("stage1/compiler-contracts/snapshots/mir-backend.json")
RUNTIME_LIFECYCLE = Path("stage1/compiler-contracts/snapshots/runtime-lifecycle-v1.json")
PROVIDER_ABI = Path("stage1/compiler-contracts/snapshots/provider-abi-v1.json")
RUNTIME_ABI_DOC = Path("docs/direct-native-runtime-abi-v0.md")
ARTIFACT_PLAN_DOC = Path("docs/artifact-plan-v0.md")
TARGET_INTERFACE_DOC = Path("docs/backend-target-interface-v0.md")

DEPENDENCIES = [1254, 1425, 1426, 1427, 1434, 1436, 1437, 1438, 1439, 1440, 1453, 1455, 1472, 1476, 1477, 1478, 1479]
READINESS_DEPENDENCIES = [1472, 1478, 1479, 1455, 1453]
SEMANTIC_INPUTS = ["artifact_contract", "command_options", "executable_mir", "lifecycle_contract", "provider_contract", "runtime_abi", "source_provenance", "target_contract", "target_support_evidence"]
BACKEND_STAGES = ["artifact_emission", "backend_dispatch", "debug_provenance", "evidence_linking", "feature_effect_validation", "link_planning", "runtime_abi_adaptation", "runtime_lowering", "target_selection"]
ARTIFACT_OUTPUTS = ["backend_execution_receipt", "capability_evidence", "debug_provenance", "link_plan", "native_binary", "native_object", "runtime_shim_requirements", "unsupported_diagnostics"]
UNSUPPORTED_DIMENSIONS = ["artifact_kind", "effect_kind", "evidence_requirement", "lifecycle_operation", "provider_requirement", "runtime_abi_row", "target_feature", "type_feature"]
RUNTIME_SENSITIVITY = {"artifact_reuse": "same_artifact_digest", "runtime_input_sets": 2, "output_relation": "distinct_semantic_outputs_for_distinct_inputs", "rebuilds_between_inputs": 0, "runtime_origin_required": True}
FORBIDDEN_RUNTIME_AUTHORITY = ["clock", "crypto", "environment", "filesystem", "network", "process", "randomness", "runtime_authority"]
BUILD_PURITY = {"forbidden_runtime_authority": FORBIDDEN_RUNTIME_AUTHORITY, "runtime_effect_boundary": "effects_execute_only_after_artifact_start", "execution_mode": "native_runtime", "lowering_mode": "runtime_lowered", "generated_host_projection_required": False}
LIFECYCLE_EXITS = ["cancellation", "error", "normal_return", "panic_trap"]
EVIDENCE_FIELDS = ["artifact_digest", "artifact_plan_digest", "capability_evidence", "debug_provenance_digest", "diagnostic_ids", "executable_mir_digest", "lifecycle_contract_version", "link_plan_digest", "native_object_digest", "package_id", "provider_identity", "runtime_abi_version", "runtime_input_digest", "source_provenance", "target_contract_id", "target_support_receipt"]
DIAGNOSTICS = ["backend.native_artifact_plan_invalid", "backend.native_contract_version_mismatch", "backend.native_runtime_lowering_required", "backend.native_unsupported_semantics", "self_host.native_backend_dependencies_incomplete", "self_host.native_backend_human_cutover_required", "self_host.native_backend_runtime_not_qualified"]
PROHIBITED_SHORTCUTS = ["build_time_program_execution", "compiler_evaluator_runtime", "fixture_specific_output", "generated_host_projection_required", "missing_unsupported_diagnostic", "provider_specific_semantic_contract", "rebuild_between_runtime_inputs", "target_specific_public_semantics", "unversioned_runtime_abi"]
TARGET_GAPS = ["all_supported_targets", "axiom_owned_native_package", "build_purity", "debug_provenance", "executable_mir", "legacy_independence", "lifecycle_ownership", "native_object_link", "provider_parity", "runtime_abi_completeness", "runtime_input_sensitivity", "unsupported_fail_closed"]
REQUIRED_PROOFS = ["all_supported_targets", "axiom_owned_native_package", "build_purity", "debug_provenance", "executable_mir_runtime", "legacy_human_cutover_approval", "legacy_independence", "lifecycle_ownership", "native_object_link", "provider_parity", "runtime_abi_completeness", "runtime_input_sensitivity", "unsupported_fail_closed"]
COMPLETION_FIELDS = ["axiom_owned_native_package_present", "executable_mir_complete", "runtime_abi_complete", "lifecycle_ownership_complete", "build_purity_proven", "runtime_input_sensitivity_proven", "unsupported_fail_closed_proven", "native_object_link_proven", "debug_provenance_proven", "provider_parity_proven", "all_supported_targets_proven", "legacy_independence_proven"]
FIXTURE_NAMES = ["all-supported-targets", "axiom-owned-native-package", "bootstrap-artifact-plan", "bootstrap-direct-native-subset", "bootstrap-lifecycle-contract", "bootstrap-mir-backend-contract", "bootstrap-provider-contract", "bootstrap-runtime-abi-contract", "build-purity", "debug-provenance", "executable-mir", "legacy-independence", "lifecycle-ownership", "native-object-link", "provider-parity", "runtime-abi-completeness", "runtime-input-sensitivity", "unsupported-fail-closed"]
BOOTSTRAP_FIXTURES = {"bootstrap-artifact-plan", "bootstrap-direct-native-subset", "bootstrap-lifecycle-contract", "bootstrap-mir-backend-contract", "bootstrap-provider-contract", "bootstrap-runtime-abi-contract"}
RUNTIME_FIXTURES = {"all-supported-targets", "build-purity", "debug-provenance", "executable-mir", "lifecycle-ownership", "provider-parity", "runtime-abi-completeness", "runtime-input-sensitivity"}
SOURCE_MARKERS = {
    MIR_BACKEND: ["compiler.backend.native", "axiom://target/stage1-direct-native"],
    RUNTIME_LIFECYCLE: ["axiom.runtime_lifecycle.v1"],
    PROVIDER_ABI: ["axiom.provider-abi.v1"],
    RUNTIME_ABI_DOC: ["# Direct Native Runtime ABI v0"],
    ARTIFACT_PLAN_DOC: ["# Artifact Plan v0"],
    TARGET_INTERFACE_DOC: ["## Target Contract", "native_binary"],
}
HOST_CAPTURE_TERMS = ("cargo", "cranelift", "rust", "rustc", "std::", "vec<")

# Trusted semantic pins remain authoritative when checkout schemas are weakened.
QUALIFICATION = {'fixture_scaffolding_only': True,
 'backend_dispatch_authorized': False,
 'semantic_cutover_authorized': False,
 'legacy_retirement_authorized': False,
 'dependencies_must_be_runtime_complete': True,
 'human_cutover_issue': 1479,
 'required_proofs': ['all_supported_targets',
                     'axiom_owned_native_package',
                     'build_purity',
                     'debug_provenance',
                     'executable_mir_runtime',
                     'legacy_human_cutover_approval',
                     'legacy_independence',
                     'lifecycle_ownership',
                     'native_object_link',
                     'provider_parity',
                     'runtime_abi_completeness',
                     'runtime_input_sensitivity',
                     'unsupported_fail_closed'],
 'readiness_promotable': False,
 'review_policy': 'independent_acceptance_and_human_cutover_review_required',
 'fail_closed_diagnostic': 'self_host.native_backend_runtime_not_qualified'}
BOOTSTRAP_EVIDENCE = ['docs/artifact-plan-v0.md',
 'docs/backend-target-interface-v0.md',
 'docs/compiler-mir-backend-packages.md',
 'docs/direct-native-runtime-abi-v0.md',
 'docs/provider-abi-v1.md',
 'docs/runtime-lifecycle-abi-v1.md',
 'stage1/compiler-contracts/snapshots/mir-backend.json',
 'stage1/compiler-contracts/snapshots/provider-abi-v1.json',
 'stage1/compiler-contracts/snapshots/runtime-lifecycle-v1.json']
FIXTURE_EVIDENCE = {'all-supported-targets': {'category': 'target',
                           'evidence': ['docs/compiler-native-backend-runtime-v1.md::Target and '
                                        'provider parity']},
 'axiom-owned-native-package': {'category': 'boundary',
                                'evidence': ['docs/production-language-readiness.json::compiler_native_backend_source']},
 'bootstrap-artifact-plan': {'category': 'artifact',
                             'evidence': ['docs/artifact-plan-v0.md::# Artifact Plan v0']},
 'bootstrap-direct-native-subset': {'category': 'runtime',
                                    'evidence': ['stage1/compiler-contracts/snapshots/mir-backend.json::axiom://target/stage1-direct-native']},
 'bootstrap-lifecycle-contract': {'category': 'lifecycle',
                                  'evidence': ['stage1/compiler-contracts/snapshots/runtime-lifecycle-v1.json::axiom.runtime_lifecycle.v1']},
 'bootstrap-mir-backend-contract': {'category': 'boundary',
                                    'evidence': ['stage1/compiler-contracts/snapshots/mir-backend.json::compiler.backend.native']},
 'bootstrap-provider-contract': {'category': 'provider',
                                 'evidence': ['stage1/compiler-contracts/snapshots/provider-abi-v1.json::axiom.provider-abi.v1']},
 'bootstrap-runtime-abi-contract': {'category': 'runtime',
                                    'evidence': ['docs/direct-native-runtime-abi-v0.md::# Direct '
                                                 'Native Runtime ABI v0']},
 'build-purity': {'category': 'purity',
                  'evidence': ['docs/compiler-native-backend-runtime-v1.md::Build purity and '
                               'runtime sensitivity']},
 'debug-provenance': {'category': 'debug',
                      'evidence': ['docs/compiler-native-backend-runtime-v1.md::Lifecycle, '
                                   'debugging, and provenance']},
 'executable-mir': {'category': 'runtime',
                    'evidence': ['docs/production-language-readiness.json::executable_mir']},
 'legacy-independence': {'category': 'dependency',
                         'evidence': ['docs/compiler-native-backend-runtime-v1.md::Dispatch and '
                                      'human cutover gate']},
 'lifecycle-ownership': {'category': 'lifecycle',
                         'evidence': ['docs/compiler-native-backend-runtime-v1.md::Lifecycle, '
                                      'debugging, and provenance']},
 'native-object-link': {'category': 'artifact',
                        'evidence': ['docs/compiler-native-backend-runtime-v1.md::Input and output '
                                     'contract']},
 'provider-parity': {'category': 'parity',
                     'evidence': ['docs/compiler-native-backend-runtime-v1.md::Target and provider '
                                  'parity']},
 'runtime-abi-completeness': {'category': 'runtime',
                              'evidence': ['docs/direct-native-runtime-abi-v0.md::# Direct Native '
                                           'Runtime ABI v0']},
 'runtime-input-sensitivity': {'category': 'runtime',
                               'evidence': ['docs/compiler-native-backend-runtime-v1.md::Build '
                                            'purity and runtime sensitivity']},
 'unsupported-fail-closed': {'category': 'target',
                             'evidence': ['docs/compiler-native-backend-runtime-v1.md::Fail-closed '
                                          'unsupported behavior']}}


class ContractError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ContractError(message)


def relative_components(relative: os.PathLike[str] | str) -> tuple[str, list[str]]:
    try:
        text = os.fspath(relative)
    except TypeError as error:
        raise ContractError(f"checkout path is not path-like: {relative!r}") from error
    require(isinstance(text, str), "checkout paths must be text")
    require("\x00" not in text, f"unsafe checkout path contains NUL: {text!r}")
    windows = PureWindowsPath(text)
    require(not PurePosixPath(text).is_absolute(), f"absolute checkout path is forbidden: {text!r}")
    require(not windows.is_absolute() and not windows.drive and not windows.root, f"Windows absolute checkout path is forbidden: {text!r}")
    require("\\" not in text, f"Windows checkout path separators are forbidden: {text!r}")
    components = text.split("/")
    require(all(component not in {"", ".", ".."} for component in components), f"unsafe checkout path component: {text!r}")
    return text, components


def safe_read_bytes(root: Path, relative: os.PathLike[str] | str) -> bytes:
    text, components = relative_components(relative)
    nofollow = getattr(os, "O_NOFOLLOW", None)
    nonblock = getattr(os, "O_NONBLOCK", None)
    directory = getattr(os, "O_DIRECTORY", None)
    require(nofollow is not None and nonblock is not None and directory is not None, "secure descriptor-relative reads are unsupported on this platform")
    common = os.O_RDONLY | nofollow | nonblock | getattr(os, "O_CLOEXEC", 0)
    dir_fd = -1
    file_fd = -1
    try:
        dir_fd = os.open(os.fspath(root), common | directory)
        require(stat.S_ISDIR(os.fstat(dir_fd).st_mode), f"checkout root is not a directory: {root}")
        for component in components[:-1]:
            next_fd = os.open(component, common | directory, dir_fd=dir_fd)
            require(stat.S_ISDIR(os.fstat(next_fd).st_mode), f"checkout path component is not a directory: {text!r}")
            os.close(dir_fd)
            dir_fd = next_fd
        file_fd = os.open(components[-1], common, dir_fd=dir_fd)
        metadata = os.fstat(file_fd)
        require(stat.S_ISREG(metadata.st_mode), f"checkout path is not a regular file: {text!r}")
        require(metadata.st_size <= MAX_READ_BYTES, f"checkout file exceeds {MAX_READ_BYTES} bytes: {text!r}")
        chunks: list[bytes] = []
        remaining = MAX_READ_BYTES + 1
        while remaining:
            chunk = os.read(file_fd, min(65536, remaining))
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        payload = b"".join(chunks)
        require(len(payload) <= MAX_READ_BYTES, f"checkout file exceeds {MAX_READ_BYTES} bytes while reading: {text!r}")
        return payload
    except ContractError:
        raise
    except (OSError, ValueError) as error:
        raise ContractError(f"unable to read checkout file {text!r}: {error}") from error
    finally:
        if file_fd >= 0:
            try:
                os.close(file_fd)
            except OSError:
                pass
        if dir_fd >= 0:
            try:
                os.close(dir_fd)
            except OSError:
                pass


def safe_read_text(root: Path, relative: os.PathLike[str] | str) -> str:
    payload = safe_read_bytes(root, relative)
    try:
        return payload.decode("utf-8", errors="strict")
    except UnicodeDecodeError as error:
        raise ContractError(f"checkout file is not valid UTF-8: {os.fspath(relative)!r}") from error


def load(root: Path, relative: os.PathLike[str] | str) -> Any:
    try:
        return json.loads(safe_read_text(root, relative))
    except json.JSONDecodeError as error:
        raise ContractError(f"unable to parse checkout JSON {os.fspath(relative)!r}: {error}") from error


def value_kind(value: Any) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, int):
        return "integer"
    if isinstance(value, float):
        return "number"
    if isinstance(value, str):
        return "string"
    if isinstance(value, list):
        return "array"
    return "object"


def json_equal(left: Any, right: Any) -> bool:
    if value_kind(left) != value_kind(right):
        return False
    if isinstance(left, dict):
        return set(left) == set(right) and all(json_equal(left[key], right[key]) for key in left)
    if isinstance(left, list):
        return len(left) == len(right) and all(json_equal(a, b) for a, b in zip(left, right, strict=True))
    return left == right


def validate_schema(value: Any, schema: dict[str, Any], path: str, root: dict[str, Any]) -> None:
    if "$ref" in schema:
        reference = schema["$ref"]
        require(reference.startswith("#/$defs/"), f"{path}: unsupported schema reference")
        name = reference.removeprefix("#/$defs/")
        require(name in root.get("$defs", {}), f"{path}: unknown schema reference")
        validate_schema(value, root["$defs"][name], path, root)
        return
    if "const" in schema:
        require(json_equal(value, schema["const"]), f"{path}: const mismatch")
    if "enum" in schema:
        require(any(json_equal(value, candidate) for candidate in schema["enum"]), f"{path}: enum mismatch")
    if schema.get("type"):
        require(value_kind(value) == schema["type"], f"{path}: expected {schema['type']}")
    if isinstance(value, dict):
        properties = schema.get("properties", {})
        for field in schema.get("required", []):
            require(field in value, f"{path}: missing {field}")
        if schema.get("additionalProperties") is False:
            require(not (set(value) - set(properties)), f"{path}: unknown fields")
        for field, nested in value.items():
            if field in properties:
                validate_schema(nested, properties[field], f"{path}.{field}", root)
    if isinstance(value, list):
        require(len(value) >= schema.get("minItems", 0), f"{path}: too few items")
        for index, item in enumerate(value):
            if "items" in schema:
                validate_schema(item, schema["items"], f"{path}[{index}]", root)
    if isinstance(value, str):
        require(len(value) >= schema.get("minLength", 0), f"{path}: empty string")
        if "pattern" in schema:
            require(re.search(schema["pattern"], value) is not None, f"{path}: pattern mismatch")
    if isinstance(value, int) and not isinstance(value, bool) and "minimum" in schema:
        require(value >= schema["minimum"], f"{path}: below minimum")


def require_sorted_unique(values: list[str], label: str) -> None:
    require(values == sorted(set(values)), f"{label} must be sorted and unique")


def evidence_path(value: str) -> Path:
    return Path(value.split("::", 1)[0])


def validate_evidence(root: Path, value: str, label: str) -> None:
    require(isinstance(value, str), f"{label} evidence must be text")
    path_text, separator, anchor = value.partition("::")
    text = safe_read_text(root, path_text)
    if separator:
        require(bool(anchor), f"{label} evidence anchor must not be empty: {value}")
        require(anchor in text, f"{label} evidence anchor is missing: {value}")


def strings(value: Any) -> list[str]:
    if isinstance(value, str):
        return [value]
    if isinstance(value, list):
        return [item for nested in value for item in strings(nested)]
    if isinstance(value, dict):
        return [item for nested in value.values() for item in strings(nested)]
    return []


def reject_host_capture(value: Any) -> None:
    for text in strings(value):
        lowered = text.lower()
        for term in HOST_CAPTURE_TERMS:
            require(term not in lowered, f"native backend semantic contract captures host term: {term}")


def readiness_row(payload: dict[str, Any], row_id: str) -> dict[str, Any]:
    rows = {item["id"]: item for item in payload["rows"]}
    require(row_id in rows, f"{row_id} readiness row missing")
    return rows[row_id]


def validate_readiness(root: Path) -> None:
    required_evidence = {"docs/compiler-native-backend-runtime-v1.md", "stage1/compiler-contracts/snapshots/compiler-native-backend-runtime-v1.json"}
    row = readiness_row(load(root, PRODUCTION_READINESS), "compiler_native_backend_source")
    require(json_equal(row["governingIssue"], 1474), "production governing issue drifted")
    require(row["currentTier"] == "syntax_only" and row["status"] == "blocked", "production readiness overclaims native backend")
    require(json_equal(row["blockerIssues"], [1474]), "production blocker drifted")
    require(json_equal(row["dependencies"], READINESS_DEPENDENCIES), "production direct dependencies drifted")
    require(required_evidence <= set(row["evidence"]), "production native backend evidence missing")
    require("make stage1-compiler-native-backend-runtime-v1-test" in row["validatingCommand"], "production mutation gate missing")
    require("make stage1-compiler-native-backend-runtime-v1" in row["validatingCommand"], "production contract gate missing")


def validate_contract(root: Path) -> dict[str, Any]:
    schema, snapshot = load(root, SCHEMA), load(root, SNAPSHOT)
    require(schema.get("$id", "").endswith("axiom.compiler_native_backend_runtime.v1.schema.json"), "schema id drifted")
    require(isinstance(snapshot, dict) and set(snapshot) == {'qualification', 'current_floor', 'target_contract', 'contract', 'fixtures', 'issue', 'dependency_issues', 'schema_version'}, "snapshot fields drifted")
    validate_schema(snapshot, schema, "$", schema)
    require(json_equal([snapshot["schema_version"], snapshot["contract"], snapshot["issue"]], ["axiom.compiler_native_backend_runtime.v1", "self_hosting.compiler_native_backend_runtime", 1474]), "contract identity drifted")
    require(json_equal(snapshot["dependency_issues"], DEPENDENCIES), "dependency inventory drifted")
    target = snapshot["target_contract"]
    exact = {
        "semantic_inputs": SEMANTIC_INPUTS,
        "backend_stages": BACKEND_STAGES,
        "artifact_outputs": ARTIFACT_OUTPUTS,
        "unsupported_dimensions": UNSUPPORTED_DIMENSIONS,
        "lifecycle_exits": LIFECYCLE_EXITS,
        "evidence_graph_fields": EVIDENCE_FIELDS,
        "diagnostics": DIAGNOSTICS,
        "prohibited_shortcuts": PROHIBITED_SHORTCUTS,
    }
    require(isinstance(target, dict) and set(target) == set(exact) | {"runtime_sensitivity", "build_purity"}, "target fields drifted")
    for label, expected in exact.items():
        require(json_equal(target[label], expected), f"target {label} drifted")
        require_sorted_unique(target[label], f"target {label}")
    require(json_equal(target["runtime_sensitivity"], RUNTIME_SENSITIVITY), "runtime sensitivity drifted")
    require(json_equal(target["build_purity"], BUILD_PURITY), "build purity contract drifted")
    require_sorted_unique(target["build_purity"]["forbidden_runtime_authority"], "build purity denials")
    reject_host_capture(schema)
    reject_host_capture({"target_contract": target, "implementation_owner": snapshot["current_floor"]["implementation_owner"], "qualification": snapshot["qualification"]})
    floor = snapshot["current_floor"]
    require(isinstance(floor, dict) and set(floor) == {'status', 'direct_native_subset_present', 'tier', 'mir_backend_contract_present', 'debug_provenance_proven', 'all_supported_targets_proven', 'axiom_owned_native_package_present', 'lifecycle_contract_present', 'native_object_link_proven', 'runtime_abi_complete', 'unsupported_fail_closed_proven', 'target_gaps', 'runtime_input_sensitivity_proven', 'implementation_owner', 'legacy_independence_proven', 'build_purity_proven', 'lifecycle_ownership_complete', 'runtime_abi_contract_present', 'bootstrap_evidence', 'artifact_plan_present', 'executable_mir_complete', 'provider_contract_present', 'provider_parity_proven'}, "current floor fields drifted")
    require(json_equal(floor["bootstrap_evidence"], BOOTSTRAP_EVIDENCE), "bootstrap evidence inventory drifted")
    require((floor["tier"], floor["status"], floor["implementation_owner"]) == ("syntax_only", "blocked", "legacy_bootstrap"), "current floor identity drifted")
    positive = ["mir_backend_contract_present", "direct_native_subset_present", "runtime_abi_contract_present", "lifecycle_contract_present", "provider_contract_present", "artifact_plan_present"]
    require(all(floor[field] is True for field in positive), "bootstrap native backend evidence disappeared")
    require(all(floor[field] is False for field in COMPLETION_FIELDS), "current floor overclaims native backend completion")
    require(floor["target_gaps"] == TARGET_GAPS, "target gap inventory drifted")
    require_sorted_unique(floor["bootstrap_evidence"], "bootstrap evidence")
    for value in floor["bootstrap_evidence"]:
        validate_evidence(root, value, "bootstrap floor")
    qualification = snapshot["qualification"]
    require(json_equal(qualification, QUALIFICATION), "qualification semantics drifted")
    require(qualification["fixture_scaffolding_only"] is True, "scaffold boundary drifted")
    require(qualification["backend_dispatch_authorized"] is False, "backend dispatch cannot be authorized by this slice")
    require(qualification["semantic_cutover_authorized"] is False, "semantic cutover cannot be authorized by this slice")
    require(qualification["legacy_retirement_authorized"] is False, "legacy retirement cannot be authorized by this slice")
    require(qualification["dependencies_must_be_runtime_complete"] is True, "dependency runtime-complete gate disappeared")
    require(qualification["human_cutover_issue"] == 1479, "human cutover issue drifted")
    require(qualification["required_proofs"] == REQUIRED_PROOFS, "qualification proofs drifted")
    require(qualification["readiness_promotable"] is False, "scaffold slice cannot promote readiness")
    fixtures = snapshot["fixtures"]
    require(isinstance(fixtures, list) and all(isinstance(f, dict) and set(f) == {"id", "category", "status", "runtime_origin", "blocks_readiness", "scenario", "evidence"} for f in fixtures), "fixture fields drifted")
    names = [fixture["id"].rsplit("/", 1)[-1] for fixture in fixtures]
    require(names == FIXTURE_NAMES, "fixture inventory must be complete and sorted")
    for fixture, name in zip(fixtures, names, strict=True):
        require(fixture["id"] == "compiler-native-backend-runtime-v1/" + name, "fixture identity drifted")
        require(isinstance(fixture["scenario"], str) and bool(fixture["scenario"].strip()), f"fixture scenario missing: {name}")
        require(json_equal({key: fixture[key] for key in ("category", "evidence")}, FIXTURE_EVIDENCE[name]), f"fixture evidence binding drifted: {name}")
        bootstrap = name in BOOTSTRAP_FIXTURES
        require(fixture["status"] == ("bootstrap_pass" if bootstrap else "target_gap"), f"fixture status drifted for {name}")
        require(fixture["runtime_origin"] is (name in RUNTIME_FIXTURES), f"fixture runtime origin drifted for {name}")
        require(fixture["blocks_readiness"] is (not bootstrap), f"fixture readiness drifted for {name}")
        for value in fixture["evidence"]:
            validate_evidence(root, value, f"fixture {name}")
    for path, markers in SOURCE_MARKERS.items():
        source = safe_read_text(root, path)
        for marker in markers:
            require(marker in source, f"bootstrap native backend marker missing in {path}: {marker}")
    validate_readiness(root)
    return {"schema": snapshot["schema_version"], "ok": True, "fixtures": len(fixtures), "bootstrap_pass": len(BOOTSTRAP_FIXTURES), "target_gaps": len(fixtures) - len(BOOTSTRAP_FIXTURES), "backend_dispatch_authorized": qualification["backend_dispatch_authorized"], "semantic_cutover_authorized": qualification["semantic_cutover_authorized"], "legacy_retirement_authorized": qualification["legacy_retirement_authorized"], "readiness_promotable": qualification["readiness_promotable"]}


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    try:
        result = validate_contract(args.root)
    except (ContractError, KeyError, TypeError, AttributeError) as error:
        print(json.dumps({"error": str(error), "ok": False}, sort_keys=True) if args.json else f"compiler-native-backend-runtime-v1: {error}", file=sys.stdout if args.json else sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True) if args.json else "compiler-native-backend-runtime-v1: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
