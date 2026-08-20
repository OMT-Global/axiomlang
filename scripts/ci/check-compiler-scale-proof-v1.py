#!/usr/bin/env python3
"""Validate Compiler-Scale Runtime Proof v1 and its blocked scaffold floor."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.compiler_scale_proof.v1.schema.json")
SNAPSHOT = Path("stage1/compiler-contracts/snapshots/compiler-scale-proof-v1.json")
PRODUCTION_READINESS = Path("docs/production-language-readiness.json")
SELF_HOSTING_READINESS = Path("docs/self-hosting-language-readiness.json")
SOURCE_LAYOUT = Path("docs/axiom-compiler-source-layout.md")
COMMAND_SNAPSHOT = Path("stage1/compiler-contracts/snapshots/command-lsp.json")
BUILD_SNAPSHOT = Path("stage1/compiler-contracts/snapshots/build.json")
DIAGNOSTICS_SPIKE = Path("stage1/selfhost/compiler-diagnostics-spike/axiom.toml")
DISTANCE_SPIKE = Path("stage1/selfhost/compiler-diagnostics-distance-spike/axiom.toml")
SPIKE_PARITY = Path("scripts/ci/run-self-hosting-spike-parity.sh")

DEPENDENCIES = [1425, 1426, 1434, 1436, 1437, 1438, 1439, 1440, 1476, 1477]
PACKAGE_ROLES = ["compiler.backend.contracts", "compiler.backend.native", "compiler.commands", "compiler.diagnostics", "compiler.evidence", "compiler.hir", "compiler.mir", "compiler.package_graph", "compiler.services.lsp", "compiler.stdlib", "compiler.syntax"]
COMMAND_SURFACES = ["build", "check", "doc", "lsp", "run", "test"]
PIPELINE_STAGES = ["backend_planning", "command_dispatch", "diagnostics", "evidence_linking", "hir_typing", "mir_planning", "package_graph", "source_loading", "syntax_analysis"]
RUNTIME_ORIGINS = ["argv", "cwd", "environment", "filesystem_source", "prior_command_artifact", "stdin"]
SCALE_FLOOR = {"minimum_packages": 8, "minimum_source_files": 20, "minimum_axiom_lines": 2000, "minimum_functions": 80, "minimum_semantic_nodes": 1000, "minimum_dependency_edges": 12, "minimum_command_surfaces": 6, "minimum_runtime_inputs": 2, "fixture_only_shortcut_forbidden": True}
RUNTIME_SENSITIVITY = {"artifact_reuse": "same_artifact_digest", "input_pair": "two_post_build_source_trees", "output_relation": "distinct_semantic_outputs_for_distinct_inputs", "rebuilds_between_inputs": 0, "runtime_origin_required": True}
FORBIDDEN_BUILD_EFFECTS = ["clock", "crypto", "environment", "filesystem", "network", "process", "randomness", "runtime_authority"]
BUILD_PURITY = {"forbidden_build_effects": FORBIDDEN_BUILD_EFFECTS, "runtime_effect_boundary": "effects_execute_only_after_artifact_start", "execution_mode": "native_runtime", "lowering_mode": "runtime_lowered", "generated_host_source": "absent"}
TABLE_KINDS = ["dependency_table", "symbol_table", "work_set"]
ASSOCIATIVE_STATE = {"table_kinds": TABLE_KINDS, "runtime_allocated": True, "fixed_literal_tables_allowed": False, "deterministic_iteration_required": True}
EVIDENCE_FIELDS = ["artifact_digest", "backend_plan", "capability_evidence", "command_surface", "input_digest", "lowering_mode", "mir_digest", "output_digest", "package_id", "runtime_origin", "source_provenance", "target_support"]
DIAGNOSTICS = ["backend.compiler_scale_runtime_lowering_required", "self_host.compiler_scale_dependencies_incomplete", "self_host.compiler_scale_fixture_only", "self_host.compiler_scale_proof_not_qualified", "self_host.compiler_scale_shortcut_detected"]
PROHIBITED_FALLBACKS = ["build_time_effect_execution", "compiler_evaluator_runtime", "fixed_literal_associative_tables", "fixture_specific_output", "generated_host_source", "rebuild_between_inputs", "static_output_replay", "toolchain_process_in_workload_path"]
TARGET_GAPS = ["all_boundary_gates", "all_supported_targets", "build_effect_purity", "compiler_scale_package", "evidence_graph", "executable_mir_runtime", "generated_host_source_absence", "lifecycle_ownership", "parity", "program_host_abi", "runtime_maps_sets", "runtime_source_ab", "same_artifact_reuse", "scale_floor", "six_command_surfaces"]
REQUIRED_PROOFS = ["all_boundary_gates", "all_supported_targets", "build_effect_purity", "compiler_scale_package", "evidence_graph", "executable_mir_runtime", "generated_host_source_absence", "lifecycle_ownership", "parity_or_versioned_contract", "program_host_abi", "runtime_maps_sets", "runtime_source_ab", "same_artifact_reuse", "scale_floor", "six_command_surfaces"]
COMPLETION_FIELDS = ["compiler_scale_package_present", "scale_floor_met", "six_command_surfaces_proven", "runtime_source_ab_proven", "same_artifact_reuse_proven", "runtime_maps_sets_proven", "program_host_abi_proven", "build_effect_purity_proven", "executable_mir_runtime_proven", "lifecycle_ownership_proven", "generated_host_source_absent_proven", "parity_proven", "evidence_graph_proven", "all_supported_targets_proven", "all_boundary_gates_proven"]
FIXTURE_NAMES = ["all-boundary-gates", "artifact-reuse", "bootstrap-boundary-contracts", "bootstrap-command-contract", "bootstrap-diagnostics-spike", "bootstrap-direct-native-subset", "bootstrap-source-layout", "build-effect-purity", "compiler-scale-package", "evidence-graph", "executable-mir", "generated-host-source-absence", "lifecycle-ownership", "parity", "program-host-abi", "runtime-map-set", "runtime-source-a-b", "scale-floor", "six-command-surfaces", "supported-targets"]
BOOTSTRAP_FIXTURES = {"bootstrap-boundary-contracts", "bootstrap-command-contract", "bootstrap-diagnostics-spike", "bootstrap-direct-native-subset", "bootstrap-source-layout"}
RUNTIME_FIXTURES = {"artifact-reuse", "build-effect-purity", "compiler-scale-package", "evidence-graph", "executable-mir", "lifecycle-ownership", "parity", "program-host-abi", "runtime-map-set", "runtime-source-a-b", "six-command-surfaces", "supported-targets"}
SOURCE_MARKERS = {
    SOURCE_LAYOUT: ["## Package Map", "compiler.diagnostics", "compiler.backend.native", "compiler.services.lsp"],
    COMMAND_SNAPSHOT: ["compiler.commands", "compiler.services.lsp"],
    BUILD_SNAPSHOT: ["\"generated_rust\": null"],
    DIAGNOSTICS_SPIKE: ["compiler-diagnostics-spike"],
    DISTANCE_SPIKE: ["compiler-diagnostics-distance-spike"],
    SPIKE_PARITY: ["generated_rust null", "self-hosting spike parity passed"],
}
HOST_CAPTURE_TERMS = ("cargo", "cranelift", "rust", "rustc", "std::", "vec<")


class ContractError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ContractError(message)


def load(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"unable to load {path}: {error}") from error


def value_kind(value: Any) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, int):
        return "integer"
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
    path_text, separator, anchor = value.partition("::")
    relative = Path(path_text)
    require(path_text != "" and not relative.is_absolute(), f"{label} evidence path must be repository-relative: {value}")
    require(".." not in relative.parts, f"{label} evidence path cannot traverse outside the repository: {value}")
    resolved_root = root.resolve()
    path = resolved_root / relative
    cursor = resolved_root
    for part in relative.parts:
        cursor /= part
        require(not cursor.is_symlink(), f"{label} evidence path cannot contain symlinks: {value}")
    try:
        path.resolve().relative_to(resolved_root)
    except ValueError as error:
        raise ContractError(f"{label} evidence path escapes the repository: {value}") from error
    require(path.is_file(), f"{label} evidence is missing: {value}")
    if separator:
        require(anchor in path.read_text(encoding="utf-8"), f"{label} evidence anchor is missing: {value}")


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
            require(term not in lowered, f"compiler-scale semantic contract captures host term: {term}")


def readiness_row(payload: dict[str, Any], row_id: str) -> dict[str, Any]:
    rows = {item["id"]: item for item in payload["rows"]}
    require(row_id in rows, f"{row_id} readiness row missing")
    return rows[row_id]


def validate_readiness(root: Path) -> None:
    required_evidence = {"docs/compiler-scale-proof-v1.md", "stage1/compiler-contracts/snapshots/compiler-scale-proof-v1.json"}
    production = readiness_row(load(root / PRODUCTION_READINESS), "compiler_scale_runtime_proof")
    require(production["governingIssue"] == 1427, "production governing issue drifted")
    require(production["currentTier"] == "syntax_only" and production["status"] == "blocked", "production readiness overclaims compiler-scale proof")
    require(production["blockerIssues"] == [1427], "production blocker drifted")
    require(production["dependencies"] == DEPENDENCIES, "production dependencies drifted")
    require(required_evidence <= set(production["evidence"]), "production compiler-scale evidence missing")
    require("make stage1-compiler-scale-proof-v1-test" in production["validatingCommand"], "production mutation gate missing")
    require("make stage1-compiler-scale-proof-v1" in production["validatingCommand"], "production contract gate missing")
    self_hosting = load(root / SELF_HOSTING_READINESS)
    command = readiness_row(self_hosting, "compiler_command_surface")
    rewrite = readiness_row(self_hosting, "compiler_scale_rewrite_fixture")
    require(command["status"] == "blocked" and command["directNativeStatus"] == "partial", "command readiness overclaims compiler-scale proof")
    require(rewrite["status"] == "blocked" and rewrite["directNativeStatus"] == "not_applicable", "rewrite fixture readiness overclaims compiler-scale proof")
    for label, item in (("command", command), ("rewrite", rewrite)):
        require(item["blockerIssues"] == [1427], f"{label} blocker drifted")
        require(required_evidence <= set(item["evidence"]), f"{label} compiler-scale evidence missing")
        require("make stage1-compiler-scale-proof-v1-test" in item["validatingCommand"], f"{label} mutation gate missing")


def validate_contract(root: Path) -> dict[str, Any]:
    schema, snapshot = load(root / SCHEMA), load(root / SNAPSHOT)
    require(schema.get("$id", "").endswith("axiom.compiler_scale_proof.v1.schema.json"), "schema id drifted")
    validate_schema(snapshot, schema, "$", schema)
    require((snapshot["schema_version"], snapshot["contract"], snapshot["issue"]) == ("axiom.compiler_scale_proof.v1", "self_hosting.compiler_scale_runtime_proof", 1427), "contract identity drifted")
    require(snapshot["dependency_issues"] == DEPENDENCIES, "dependency inventory drifted")
    target = snapshot["target_contract"]
    exact = {
        "package_roles": PACKAGE_ROLES,
        "command_surfaces": COMMAND_SURFACES,
        "pipeline_stages": PIPELINE_STAGES,
        "runtime_origins": RUNTIME_ORIGINS,
        "evidence_graph_fields": EVIDENCE_FIELDS,
        "diagnostics": DIAGNOSTICS,
        "prohibited_fallbacks": PROHIBITED_FALLBACKS,
    }
    for label, expected in exact.items():
        require(target[label] == expected, f"target {label} drifted")
        require_sorted_unique(target[label], f"target {label}")
    require(target["scale_floor"] == SCALE_FLOOR, "material scale floor drifted")
    require(target["runtime_sensitivity"] == RUNTIME_SENSITIVITY, "runtime sensitivity drifted")
    require(target["build_purity"] == BUILD_PURITY, "build purity contract drifted")
    require_sorted_unique(target["build_purity"]["forbidden_build_effects"], "build effect denials")
    require(target["associative_state"] == ASSOCIATIVE_STATE, "associative state contract drifted")
    require_sorted_unique(target["associative_state"]["table_kinds"], "associative table kinds")
    reject_host_capture(schema)
    reject_host_capture({"target_contract": target, "implementation_owner": snapshot["current_floor"]["implementation_owner"], "qualification": snapshot["qualification"]})
    floor = snapshot["current_floor"]
    require((floor["tier"], floor["status"], floor["implementation_owner"]) == ("syntax_only", "blocked", "legacy_bootstrap"), "current floor identity drifted")
    positive = ["source_layout_contract_present", "diagnostics_spike_present", "direct_native_subset_present", "boundary_contracts_present", "command_contract_present"]
    require(all(floor[field] is True for field in positive), "bootstrap compiler-scale evidence disappeared")
    require(all(floor[field] is False for field in COMPLETION_FIELDS), "current floor overclaims compiler-scale proof")
    require(floor["target_gaps"] == TARGET_GAPS, "target gap inventory drifted")
    require_sorted_unique(floor["bootstrap_evidence"], "bootstrap evidence")
    for value in floor["bootstrap_evidence"]:
        validate_evidence(root, value, "bootstrap floor")
    qualification = snapshot["qualification"]
    require(qualification["fixture_scaffolding_only"] is True, "scaffold boundary drifted")
    require(qualification["workload_dispatch_authorized"] is False, "workload dispatch cannot be authorized by this slice")
    require(qualification["dependencies_must_be_runtime_complete"] is True, "dependency runtime-complete gate disappeared")
    require(qualification["required_proofs"] == REQUIRED_PROOFS, "qualification proofs drifted")
    require(qualification["readiness_promotable"] is False, "scaffold slice cannot promote readiness")
    fixtures = snapshot["fixtures"]
    names = [fixture["id"].rsplit("/", 1)[-1] for fixture in fixtures]
    require(names == FIXTURE_NAMES, "fixture inventory must be complete and sorted")
    for fixture, name in zip(fixtures, names, strict=True):
        bootstrap = name in BOOTSTRAP_FIXTURES
        require(fixture["status"] == ("bootstrap_pass" if bootstrap else "target_gap"), f"fixture status drifted for {name}")
        require(fixture["runtime_origin"] is (name in RUNTIME_FIXTURES), f"fixture runtime origin drifted for {name}")
        require(fixture["blocks_readiness"] is (not bootstrap), f"fixture readiness drifted for {name}")
        for value in fixture["evidence"]:
            validate_evidence(root, value, f"fixture {name}")
    for path, markers in SOURCE_MARKERS.items():
        source = (root / path).read_text(encoding="utf-8")
        for marker in markers:
            require(marker in source, f"bootstrap compiler-scale marker missing in {path}: {marker}")
    validate_readiness(root)
    return {"schema": snapshot["schema_version"], "ok": True, "fixtures": len(fixtures), "bootstrap_pass": len(BOOTSTRAP_FIXTURES), "target_gaps": len(fixtures) - len(BOOTSTRAP_FIXTURES), "workload_dispatch_authorized": qualification["workload_dispatch_authorized"], "readiness_promotable": qualification["readiness_promotable"]}


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    try:
        result = validate_contract(args.root)
    except (ContractError, KeyError, TypeError, AttributeError) as error:
        print(json.dumps({"error": str(error), "ok": False}, sort_keys=True) if args.json else f"compiler-scale-proof-v1: {error}", file=sys.stdout if args.json else sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True) if args.json else "compiler-scale-proof-v1: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
