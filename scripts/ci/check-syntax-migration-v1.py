#!/usr/bin/env python3
"""Validate the compiler.syntax migration v1 target and current evidence floor."""

from __future__ import annotations

import argparse
import json
import os
import re
import stat
import sys
from pathlib import Path, PurePosixPath
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = Path("stage1/compiler-contracts/schemas/axiom.compiler.syntax_migration.v1.schema.json")
SNAPSHOT = Path("stage1/compiler-contracts/snapshots/syntax-migration-v1.json")
FIXTURE_ROOT = Path("stage1/compiler-contracts/fixtures/syntax-migration-v1")
SYNTAX_SOURCE = Path("stage1/crates/axiomc/src/syntax.rs")
SYNTAX_TESTS = Path("stage1/crates/axiomc/tests/support/lib_unit.rs")
FIXTURE_TESTS = Path("stage1/crates/axiomc/tests/syntax_migration_v1.rs")
MAX_EVIDENCE_BYTES = 1_048_576

ENTRYPOINTS = ["expand_macros", "lex_source", "parse_macro_definitions", "parse_program", "parse_program_with_recovery"]
KINDS = ["block", "declaration", "expression", "import", "item", "macro", "pattern", "program", "statement", "type"]
INSPECTION_FIELDS = ["comments", "diagnostics", "macro_expansions", "node_id", "node_kind", "recovered", "source_identity", "span", "syntax_owner"]
ENTRY_GATES = [
    ("compiler_scale_runtime_proof", 1427),
    ("diagnostics_contract_qualification", 1473),
    ("maintainer_cutover_approval", 1468),
    ("parent_runtime_prerequisites", 1468),
]
TARGET_GAPS = [
    "axiom_owned_package",
    "canonical_axiom_node_identity",
    "differential_coexistence",
    "doc_comment_trivia_ownership",
    "fuzz_corpus_execution",
    "line_comment_trivia_ownership",
    "macro_limit_ceiling_enforcement",
    "macro_stable_failure_codes",
    "recovered_node_emission",
    "runtime_same_binary_ab",
    "rust_path_disable",
    "unicode_span_semantics",
]
CUTOVER_PROOFS = {
    "axiom_owned_package": "axiom_owned_syntax_package_qualified",
    "canonical_axiom_node_identity": "canonical_node_identity_collision_vectors_pass",
    "differential_coexistence": "differential_corpus_matches",
    "doc_comment_trivia_ownership": "doc_comment_trivia_parity_passes",
    "fuzz_corpus_execution": "fuzz_corpus_qualification_passes",
    "line_comment_trivia_ownership": "line_comment_trivia_parity_passes",
    "macro_limit_ceiling_enforcement": "macro_limit_ceiling_vectors_pass",
    "macro_stable_failure_codes": "macro_failure_codes_match",
    "recovered_node_emission": "recovered_nodes_and_diagnostics_match",
    "runtime_same_binary_ab": "axiom_package_executes_same_binary_runtime_ab",
    "rust_path_disable": "rust_syntax_path_disabled",
    "unicode_span_semantics": "unicode_span_vectors_match",
}
FIXTURES = {
    "bootstrap-conformance": ("conformance", "bootstrap_pass"),
    "bootstrap-line-comment-coordinates": ("comment", "bootstrap_pass"),
    "bootstrap-macro-byte-limit": ("macro", "bootstrap_pass"),
    "bootstrap-macro-invocation-limit": ("macro", "bootstrap_pass"),
    "bootstrap-macro-provenance": ("macro", "bootstrap_pass"),
    "bootstrap-macro-recursion-limit": ("macro", "bootstrap_pass"),
    "bootstrap-node-identity": ("node_identity", "bootstrap_pass"),
    "bootstrap-recovery-diagnostics": ("recovery", "bootstrap_pass"),
    "differential-coexistence": ("differential", "target_gap"),
    "doc-comment-trivia-ownership": ("comment", "target_gap"),
    "fuzz-corpus": ("fuzz", "target_gap"),
    "line-comment-trivia-ownership": ("comment", "target_gap"),
    "node-identity-parity": ("node_identity", "target_gap"),
    "recovered-node-emission": ("recovery", "target_gap"),
    "runtime-same-binary-ab": ("runtime_input", "target_gap"),
    "rust-path-disable": ("ownership", "target_gap"),
    "unicode-span-semantics": ("span", "target_gap"),
}
EXPECTED_DOCUMENT_FIELDS = {
    "schema_version",
    "id",
    "category",
    "status",
    "input",
    "expected",
    "evidence",
    "qualification",
}
EXPECTED_RESULT_FIELDS = {
    "exit_outcome",
    "compiler_diagnostics",
    "recovered_nodes",
    "spans",
    "macro_provenance",
    "node_identities",
}


class ContractError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ContractError(message)


def relative_parts(value: str | Path, label: str) -> tuple[str, ...]:
    text = value.as_posix() if isinstance(value, Path) else value
    require(isinstance(text, str) and bool(text), f"{label} path must be a non-empty string")
    require("\\" not in text and "\x00" not in text, f"{label} path uses a forbidden separator")
    path = PurePosixPath(text)
    require(not path.is_absolute(), f"{label} path must be relative: {text}")
    require(path.parts and all(part not in {"", ".", ".."} for part in path.parts), f"{label} path escapes checkout: {text}")
    return path.parts


def secure_open(root: Path, value: str | Path, label: str) -> tuple[int, os.stat_result]:
    parts = relative_parts(value, label)
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    directory_flags = flags | getattr(os, "O_DIRECTORY", 0)
    descriptors: list[int] = []
    try:
        current = os.open(root, directory_flags)
        descriptors.append(current)
        for part in parts[:-1]:
            current = os.open(part, directory_flags, dir_fd=current)
            descriptors.append(current)
        result = os.open(parts[-1], flags, dir_fd=current)
        metadata = os.fstat(result)
        for descriptor in reversed(descriptors):
            os.close(descriptor)
        return result, metadata
    except (OSError, ValueError) as error:
        for descriptor in reversed(descriptors):
            try:
                os.close(descriptor)
            except OSError:
                pass
        raise ContractError(f"{label} path is unavailable or unsafe: {value}: {error}") from error


def secure_kind(root: Path, value: str | Path, label: str) -> str:
    descriptor, metadata = secure_open(root, value, label)
    os.close(descriptor)
    require(stat.S_ISREG(metadata.st_mode), f"{label} must be a regular file: {value}")
    require(metadata.st_size <= MAX_EVIDENCE_BYTES, f"{label} exceeds {MAX_EVIDENCE_BYTES} bytes: {value}")
    return "file"


def secure_read_text(root: Path, value: str | Path, label: str) -> str:
    descriptor, metadata = secure_open(root, value, label)
    try:
        require(stat.S_ISREG(metadata.st_mode), f"{label} must be a regular file: {value}")
        require(metadata.st_size <= MAX_EVIDENCE_BYTES, f"{label} exceeds {MAX_EVIDENCE_BYTES} bytes: {value}")
        chunks: list[bytes] = []
        remaining = MAX_EVIDENCE_BYTES + 1
        while remaining:
            chunk = os.read(descriptor, min(65536, remaining))
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        payload = b"".join(chunks)
        require(len(payload) <= MAX_EVIDENCE_BYTES, f"{label} exceeds {MAX_EVIDENCE_BYTES} bytes: {value}")
        try:
            return payload.decode("utf-8")
        except UnicodeDecodeError as error:
            raise ContractError(f"{label} is not UTF-8: {value}") from error
    finally:
        os.close(descriptor)


def load(root: Path, value: str | Path, label: str) -> Any:
    try:
        return json.loads(secure_read_text(root, value, label))
    except json.JSONDecodeError as error:
        raise ContractError(f"unable to parse {label}: {value}: {error}") from error


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
    if isinstance(left, list) or isinstance(right, list):
        return (
            isinstance(left, list)
            and isinstance(right, list)
            and len(left) == len(right)
            and all(json_equal(left_item, right_item) for left_item, right_item in zip(left, right))
        )
    if isinstance(left, dict) or isinstance(right, dict):
        return (
            isinstance(left, dict)
            and isinstance(right, dict)
            and set(left) == set(right)
            and all(json_equal(left[field], right[field]) for field in left)
        )
    return value_kind(left) == value_kind(right) and left == right


def validate_schema(value: Any, schema: dict[str, Any], path: str, root: dict[str, Any]) -> None:
    if "$ref" in schema:
        prefix = "#/$defs/"
        reference = schema["$ref"]
        require(reference.startswith(prefix), f"{path}: unsupported schema reference")
        name = reference[len(prefix):]
        require(name in root.get("$defs", {}), f"{path}: unknown schema reference")
        validate_schema(value, root["$defs"][name], path, root)
        return
    if "const" in schema:
        require(json_equal(value, schema["const"]), f"{path}: const mismatch")
    if "enum" in schema:
        require(any(json_equal(value, candidate) for candidate in schema["enum"]), f"{path}: enum mismatch")
    expected = schema.get("type")
    if expected:
        require(value_kind(value) == expected, f"{path}: expected {expected}")
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
        if "items" in schema:
            for index, item in enumerate(value):
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


def validate_evidence_reference(root: Path, value: str, label: str) -> None:
    require(isinstance(value, str) and value, f"{label} evidence must be a string")
    path_text, separator, anchor = value.partition("::")
    if separator:
        require(bool(anchor), f"{label} evidence anchor is empty: {value}")
        source = secure_read_text(root, path_text, f"{label} anchored evidence")
        require(anchor in source, f"{label} evidence anchor is missing: {value}")
    else:
        secure_kind(root, path_text, f"{label} evidence")


def validate_fixture_document(root: Path, name: str, metadata: dict[str, Any]) -> dict[str, Any]:
    expected_category, expected_status = FIXTURES[name]
    expected_path = FIXTURE_ROOT / f"{name}.json"
    require(metadata["fixture_file"] == expected_path.as_posix(), f"fixture path drifted for {name}")
    document = load(root, expected_path, f"fixture {name}")
    require(isinstance(document, dict) and set(document) == EXPECTED_DOCUMENT_FIELDS, f"fixture {name} fields drifted")
    require(document["schema_version"] == "axiom.compiler.syntax_fixture.v1", f"fixture schema drifted for {name}")
    require(document["id"] == f"syntax-migration-v1/{name}", f"fixture id drifted for {name}")
    require(document["category"] == expected_category and metadata["category"] == expected_category, f"fixture category drifted for {name}")
    require(document["status"] == expected_status and metadata["status"] == expected_status, f"fixture status drifted for {name}")
    require(isinstance(document["input"], dict) and document["input"], f"fixture input is empty for {name}")
    require(isinstance(document["qualification"], dict) and document["qualification"], f"fixture qualification is empty for {name}")
    result = document["expected"]
    require(isinstance(result, dict) and set(result) == EXPECTED_RESULT_FIELDS, f"fixture result fields drifted for {name}")
    for field in EXPECTED_RESULT_FIELDS - {"exit_outcome"}:
        require(isinstance(result[field], list), f"fixture {name} {field} must be an array")
    if expected_status == "bootstrap_pass":
        require(result["exit_outcome"] != "not_executed_target_gap", f"bootstrap fixture {name} is not executable")
        require(metadata["runtime_origin"] is False and metadata["blocks_cutover"] is False, f"bootstrap fixture metadata drifted for {name}")
    else:
        require(result["exit_outcome"] == "not_executed_target_gap", f"target gap {name} overclaims execution")
        require(metadata["runtime_origin"] is True and metadata["blocks_cutover"] is True, f"target-gap metadata drifted for {name}")
    evidence = document["evidence"]
    require(isinstance(evidence, list) and evidence, f"fixture {name} evidence is empty")
    for reference in evidence:
        validate_evidence_reference(root, reference, f"fixture {name}")
    return document


def validate_fixture_semantics(documents: dict[str, dict[str, Any]]) -> None:
    conformance = documents["bootstrap-conformance"]
    require(
        conformance["input"]
        == {
            "pass_root": "stage1/conformance/pass",
            "fail_root": "stage1/conformance/fail",
            "supplied_after_build": False,
        }
        and conformance["expected"]["exit_outcome"] == "corpus_passes",
        "bootstrap conformance fixture drifted",
    )
    recovery = documents["bootstrap-recovery-diagnostics"]["expected"]
    require(
        [(item["code"], item["line"], item["column"]) for item in recovery["compiler_diagnostics"]]
        == [("parse.invalid_syntax", 1, 1), ("parse.missing_token", 2, 1), ("parse.unexpected_token", 4, 1)],
        "bootstrap recovery diagnostic order drifted",
    )
    require(recovery["recovered_nodes"] == [], "bootstrap must not claim recovered-node emission")

    provenance = documents["bootstrap-macro-provenance"]["expected"]["macro_provenance"]
    require(provenance == [{"macro_name": "ping", "depth": 1, "definition_span": {"path": "main.ax", "line": 1, "column": 7}, "call_span": {"path": "main.ax", "line": 5, "column": 1}}], "bootstrap macro provenance drifted")

    identity = documents["bootstrap-node-identity"]["expected"]["node_identities"]
    require(identity and identity[0]["id"] == "modules/nested.ax:1:1:enum:ResultKind", "bootstrap node identity drifted")
    require(identity[0]["canonical_axiom_id"] is False, "bootstrap identity must not be called canonical")

    line_comment = documents["bootstrap-line-comment-coordinates"]
    require(line_comment["qualification"]["comments_retained_as_trivia"] is False, "bootstrap line comments are stripped, not retained")
    require([(span["line"], span["column"]) for span in line_comment["expected"]["spans"]] == [(2, 1), (3, 1)], "line-comment coordinate proof drifted")

    limits = {
        "bootstrap-macro-byte-limit": ("macro_expansion_byte_limit", 96, "expanded source budget of 96 bytes"),
        "bootstrap-macro-invocation-limit": ("macro_expansion_invocation_limit", 1, "invocation budget of 1"),
        "bootstrap-macro-recursion-limit": ("macro_recursion_limit", 3, "bounded depth of 3"),
    }
    for name, (option, value, message) in limits.items():
        document = documents[name]
        require(document["qualification"]["option"] == option and document["qualification"]["value"] == value, f"macro option drifted for {name}")
        diagnostic = document["expected"]["compiler_diagnostics"]
        require(len(diagnostic) == 1 and diagnostic[0]["code"] == "parse.invalid_syntax" and message in diagnostic[0]["message"], f"macro diagnostic drifted for {name}")

    runtime = documents["runtime-same-binary-ab"]
    runtime_input = runtime["input"]
    runs = runtime_input["inputs_supplied_after_build"]
    require(runtime_input["artifact_sha256"] is None and runtime_input["built_before_inputs_exist"] is True, "runtime A/B must remain an unproved target gap")
    require([run["id"] for run in runs] == ["A", "B"] and runs[0]["source"] != runs[1]["source"], "runtime A/B inputs drifted")
    require(runtime["qualification"]["artifact_identity"] == "one exact sha256 for A and B", "runtime A/B artifact binding drifted")
    require(len(runtime["qualification"]["anti_static_replay"]) == 4, "runtime A/B anti-replay proof is incomplete")
    require(
        runtime["expected"]["compiler_diagnostics"]
        == [
            {"input": "A", "records": []},
            {
                "input": "B",
                "records": [
                    {
                        "kind": "parse",
                        "code": "parse.missing_token",
                        "line": 1,
                        "column": 1,
                    }
                ],
            },
        ],
        "runtime A/B expected envelopes drifted",
    )

    fuzz = documents["fuzz-corpus"]
    require(fuzz["input"]["seed_manifest"] == [], "fuzz corpus must remain a gap until deterministic seeds exist")
    require(fuzz["input"]["deterministic_mutation_seed"] == 1471, "fuzz mutation seed drifted")
    require(fuzz["qualification"]["per_case_byte_limit"] == 1048576 and fuzz["qualification"]["per_case_time_ms"] == 1000, "fuzz resource bounds drifted")
    require(fuzz["qualification"]["oracles"] == ["deterministic_diagnostics", "no_crash", "no_nontermination", "no_out_of_checkout_read"], "fuzz oracles drifted")

    unicode_fixture = documents["unicode-span-semantics"]
    require([vector["id"] for vector in unicode_fixture["input"]["vectors"]] == ["unicode-scalar", "tab", "crlf", "malformed"], "Unicode vector inventory drifted")
    require(unicode_fixture["qualification"] == {"offset_unit": "utf8_byte", "column_unit": "unicode_scalar", "tabs": "one scalar", "crlf": "one line break", "malformed_utf8": "reject before lexing"}, "Unicode span semantics drifted")
    require(
        unicode_fixture["expected"]["compiler_diagnostics"]
        == [{"vector": "malformed", "kind": "source", "code": "source.invalid_utf8", "byte_offset": 6}],
        "malformed UTF-8 vector drifted",
    )
    require(
        unicode_fixture["expected"]["spans"]
        == [
            {"vector": "unicode-scalar", "token": "café", "start_byte": 4, "end_byte": 9, "line": 1, "column": 5, "end_column": 9},
            {"vector": "unicode-scalar", "token": "🙂", "start_byte": 21, "end_byte": 25, "line": 1, "column": 21, "end_column": 22},
            {"vector": "tab", "token": "print", "start_byte": 1, "end_byte": 6, "line": 1, "column": 2, "end_column": 7},
            {"vector": "crlf", "token": "print", "occurrence": 2, "start_byte": 9, "end_byte": 14, "line": 2, "column": 1, "end_column": 6},
        ],
        "Unicode span vectors drifted",
    )

    require(
        documents["node-identity-parity"]["expected"]["node_identities"]
        == [
            {"origin": "source", "kind": "program", "ordinal": 0},
            {"origin": "macro-2", "kind": "expression", "ordinal": 0},
            {"origin": "recovered", "kind": "declaration", "ordinal": 4},
            {"origin": "synthetic-source-program-0", "kind": "eof", "ordinal": 0},
        ],
        "node identity origin vectors drifted",
    )
    recovered = documents["recovered-node-emission"]["expected"]
    require(
        recovered["compiler_diagnostics"]
        == [{"kind": "parse", "code": "parse.missing_token", "message": "let binding is missing ':'", "path": "runtime/recovery.ax", "line": 1, "column": 1}]
        and recovered["recovered_nodes"]
        == [
            {
                "node_id": "axiom://syntax/sha256/a251f5bacc41ef22fe5b41b8158dba7638614346d380ebb677252349e01237b8/recovered/declaration/1",
                "node_kind": "declaration",
                "skipped_start_byte": 0,
                "skipped_end_byte": 18,
                "diagnostic_ids": [0],
                "span": {"path": "runtime/recovery.ax", "start_byte": 0, "end_byte": 18, "line": 1, "column": 1},
            }
        ]
        and recovered["spans"] == [{"kind": "recovered_declaration", "line": 1, "column": 1}]
        and recovered["node_identities"]
        == [
            {
                "id": "axiom://syntax/sha256/a251f5bacc41ef22fe5b41b8158dba7638614346d380ebb677252349e01237b8/recovered/declaration/1",
                "origin": "recovered",
                "kind": "declaration",
                "ordinal": 1,
            }
        ],
        "target recovered-node fixture drifted",
    )
    require(documents["line-comment-trivia-ownership"]["qualification"]["bootstrap_behavior"] == "text stripped while line coordinates survive", "line-comment ownership gap drifted")
    require(documents["differential-coexistence"]["qualification"]["artifact_identity_required"] is True, "differential proof must bind an artifact")


def validate_contract(root: Path) -> dict[str, Any]:
    try:
        root = root.resolve(strict=True)
    except OSError as error:
        raise ContractError(f"checkout root is unavailable: {root}: {error}") from error
    require(root.is_dir(), f"checkout root must be a directory: {root}")
    schema = load(root, SCHEMA, "syntax schema")
    snapshot = load(root, SNAPSHOT, "syntax snapshot")
    require(schema.get("$id", "").endswith("axiom.compiler.syntax_migration.v1.schema.json"), "schema id drifted")
    validate_schema(snapshot, schema, "$", schema)
    require((snapshot["schema_version"], snapshot["contract"], snapshot["issue"], snapshot["parent_issue"]) == ("axiom.compiler.syntax_migration.v1", "compiler.syntax", 1471, 1468), "contract identity drifted")
    require(snapshot["dependency_issues"] == [1427, 1468, 1473], "syntax migration dependencies must include #1427, #1468, and #1473")
    gates = snapshot["entry_gates"]
    require([(gate["id"], gate["issue"]) for gate in gates] == ENTRY_GATES, "syntax migration entry gates drifted")
    require(all(gate["status"] == "blocked" and gate["required_proof"] for gate in gates), "entry gates must fail closed")

    target = snapshot["target_contract"]
    require(target["entrypoints"] == ENTRYPOINTS, "syntax entrypoints drifted")
    require(target["syntax_kinds"] == KINDS, "syntax kinds drifted")
    require(target["trivia"] == ["doc_comment", "line_comment"], "syntax trivia contract drifted")
    require(target["spans"] == {
        "offset_unit": "utf8_byte", "line_base": 1, "column_base": 1,
        "column_unit": "unicode_scalar", "tab_policy": "one_unicode_scalar_no_display_expansion",
        "line_endings": "lf_and_crlf_each_count_as_one_line_break",
        "malformed_utf8": "reject_before_lexing:source.invalid_utf8",
        "start_inclusive": True, "end_exclusive": True, "source_identity_required": True,
    }, "span semantics drifted")
    identity = target["node_identity"]
    require(identity["scheme"] == "axiom://syntax/sha256/{source_digest}/{origin}/{kind}/{ordinal}", "canonical node identity scheme drifted")
    require(identity["source_identity"] == "sha256_of_exact_input_bytes_lower_hex", "node source identity drifted")
    require(
        identity["origin_rules"]
        == [
            "macro_generated=macro-{call_site_source_ordinal} with expansion preorder ordinal",
            "recovered=recovered at the first skipped token and occupies the source preorder slot",
            "source=source with one global source preorder ordinal",
            "synthetic=synthetic-{owning_origin}-{owning_kind}-{owning_ordinal} with per-owner deterministic construction ordinal",
        ],
        "node origin rules drifted",
    )
    require_sorted_unique(identity["prohibited_inputs"], "node identity prohibited inputs")
    require(target["recovery"]["recovered_node_required"] is True, "target recovery must emit recovered nodes")
    require_sorted_unique(target["recovery"]["recovered_node_fields"], "recovered node fields")
    require_sorted_unique(target["recovery"]["resynchronization_points"], "recovery points")
    require(target["macros"]["limit_precedence"] == "configuration_then_invocations_then_expanded_bytes_then_recursion", "macro limit precedence drifted")
    for key, expected in {"recursion": (64, 1024, "parse.macro_recursion_limit"), "expanded_bytes": (16777216, 67108864, "parse.macro_expanded_bytes_limit"), "invocations": (8192, 65536, "parse.macro_invocation_limit")}.items():
        limit = target["macros"][key]
        require((limit["default"], limit["ceiling"], limit["failure_code"]) == expected, f"macro {key} limit drifted")
        require(limit["default"] <= limit["ceiling"] and limit["boundary"] == "inclusive", f"macro {key} boundary drifted")
    require(target["inspection_fields"] == INSPECTION_FIELDS, "syntax inspection fields drifted")

    floor = snapshot["current_floor"]
    require_sorted_unique(floor["bootstrap_evidence"], "bootstrap evidence")
    for evidence in floor["bootstrap_evidence"]:
        validate_evidence_reference(root, evidence, "bootstrap floor")
    require(floor["target_gaps"] == TARGET_GAPS, "syntax target gaps drifted")
    require(not any([floor["axiom_package_present"], floor["runtime_origin_source_proven"], floor["rust_path_disable_proven"], floor["differential_parity_present"], floor["canonical_axiom_node_ids"]]), "current floor overclaims AxiOM syntax migration")
    require(not list(root.glob("stage1/selfhost/compiler-syntax*/axiom.toml")), "AxiOM syntax package appeared; update the migration floor")

    cutover = snapshot["cutover"]
    require(cutover["permitted"] is False, "syntax cutover cannot be permitted by this planning slice")
    require(cutover["required_entry_gates"] == [gate[0] for gate in ENTRY_GATES], "cutover omits an entry gate")
    require_sorted_unique(cutover["required_proofs"], "cutover proofs")
    require(set(CUTOVER_PROOFS) == set(TARGET_GAPS), "cutover proof mapping omits a target gap")
    require(
        cutover["required_proofs"] == sorted(CUTOVER_PROOFS.values()),
        "cutover omits proof for a blocking target gap",
    )

    fixtures = snapshot["fixtures"]
    names = [fixture["id"].rsplit("/", 1)[-1] for fixture in fixtures]
    require(names == sorted(FIXTURES), "fixture inventory must be complete and sorted")
    documents = {name: validate_fixture_document(root, name, fixture) for name, fixture in zip(names, fixtures, strict=True)}
    validate_fixture_semantics(documents)

    source = secure_read_text(root, SYNTAX_SOURCE, "bootstrap syntax source")
    tests = secure_read_text(root, SYNTAX_TESTS, "bootstrap syntax tests")
    fixture_tests = secure_read_text(root, FIXTURE_TESTS, "syntax fixture tests")
    for marker in ["pub fn parse_program_with_recovery", "fn strip_line_comments", "pub fn stable_id", "macro_recursion_limit", "macro_expansion_byte_limit", "macro_expansion_invocation_limit"]:
        require(marker in source, f"bootstrap syntax evidence marker missing: {marker}")
    for marker in ["parser_bounds_total_declarative_macro_expansion_size", "parser_honors_macro_recursion_limit_option", "parser_recovery_reports_stable_top_level_errors"]:
        require(marker in tests, f"bootstrap syntax test marker missing: {marker}")
    for marker in ["bootstrap_recovery_diagnostics_match_fixture", "bootstrap_macro_limits_match_fixtures", "bootstrap_macro_provenance_matches_fixture", "bootstrap_node_identity_matches_fixture", "bootstrap_line_comment_coordinates_match_fixture"]:
        require(marker in fixture_tests, f"structured fixture test marker missing: {marker}")
    require("axiom://syntax/sha256/" not in source, "bootstrap source now exposes canonical AxiOM node IDs; update the floor")

    bootstrap_count = sum(status == "bootstrap_pass" for _, status in FIXTURES.values())
    return {
        "schema": snapshot["schema_version"],
        "ok": True,
        "fixtures": len(fixtures),
        "bootstrap_pass": bootstrap_count,
        "target_gaps": len(floor["target_gaps"]),
        "cutover_permitted": cutover["permitted"],
    }


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", "--checkout-root", dest="root", type=Path, default=ROOT)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    try:
        result = validate_contract(args.root)
    except (ContractError, KeyError, TypeError, AttributeError, IndexError) as error:
        if args.json:
            print(json.dumps({"error": str(error), "ok": False}, sort_keys=True))
        else:
            print(f"syntax-migration-v1: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True) if args.json else "syntax-migration-v1: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
