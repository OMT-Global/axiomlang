#!/usr/bin/env python3
"""Hermetic regressions for policy-bound Compatibility v1 reporting."""

from __future__ import annotations

import copy
import importlib.util
import json
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-compatibility-v1.py"
EXTRACTOR = ROOT / "scripts/ci/extract-public-contract-v1.py"
SCENARIO = ROOT / "stage1/compatibility/fixtures/migration-plan-scenario"
OLD = SCENARIO / "old.json"
NEW = SCENARIO / "new.json"
POLICY = SCENARIO / "policy.json"
CURRENT_POLICY = ROOT / "stage1/compatibility/policy-v1.json"
BASELINE = ROOT / "stage1/compatibility/fixtures/accepted-baseline/contract.json"
BASELINE_POLICY = ROOT / "stage1/compatibility/fixtures/accepted-baseline/policy.json"
CURRENT = ROOT / "stage1/compatibility/fixtures/current/contract.json"


def load(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    assert isinstance(value, dict)
    return value


def write(path: Path, value: dict[str, Any]) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def run(
    old: Path,
    new: Path,
    *,
    policy: Path = POLICY,
    old_policy: Path | None = None,
) -> subprocess.CompletedProcess[str]:
    command = [
            sys.executable,
            str(CHECKER),
            "--old",
            str(old),
            "--new",
            str(new),
            "--policy",
            str(policy),
            "--json",
        ]
    if old_policy is not None:
        command.extend(["--old-policy", str(old_policy)])
    return subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )


def expect_failure(
    directory: Path,
    old_payload: dict[str, Any],
    new_payload: dict[str, Any],
    message: str,
    *,
    policy: Path = POLICY,
    old_policy: Path | None = None,
) -> None:
    old = directory / "old.json"
    new = directory / "new.json"
    write(old, old_payload)
    write(new, new_payload)
    result = run(old, new, policy=policy, old_policy=old_policy)
    assert result.returncode != 0, result.stdout + result.stderr
    assert message in result.stdout, result.stdout
    failure = json.loads(result.stdout)
    assert set(failure) == {"command", "error", "ok", "schema_version"}
    assert failure["ok"] is False


def surface(contract: dict[str, Any], identifier: str) -> dict[str, Any]:
    return next(item for item in contract["surfaces"] if item["id"] == identifier)


def extractor_module() -> Any:
    spec = importlib.util.spec_from_file_location("axiom_contract_extractor", EXTRACTOR)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main() -> int:
    first = run(OLD, NEW)
    second = run(OLD, NEW)
    assert first.returncode == 0, first.stdout + first.stderr
    assert first.stdout == second.stdout, "compatibility report must be byte-deterministic"
    relative = subprocess.run(
        [
            sys.executable,
            str(CHECKER.relative_to(ROOT)),
            "--old",
            str(OLD.relative_to(ROOT)),
            "--new",
            str(NEW.relative_to(ROOT)),
            "--policy",
            str(POLICY.relative_to(ROOT)),
            "--json",
        ],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    alternate_cwd = subprocess.run(
        [
            sys.executable,
            str(CHECKER),
            "--old",
            str(OLD),
            "--new",
            str(NEW),
            "--policy",
            str(POLICY),
            "--json",
        ],
        cwd=SCENARIO,
        text=True,
        capture_output=True,
        check=False,
    )
    assert relative.returncode == 0 and alternate_cwd.returncode == 0
    assert (
        first.stdout == relative.stdout == alternate_cwd.stdout
    ), "report bytes must depend on contract identities, not path spelling or cwd"
    report = json.loads(first.stdout)
    assert report["old"] == "axiom://compatibility/migration-plan-scenario/old"
    assert report["new"] == "axiom://compatibility/migration-plan-scenario/new"
    assert report["contracts"] == {"old": "1.0.0", "new": "2.0.0"}
    assert report["policies"] == {
        "old": "1.0.0",
        "new": "1.0.0",
        "severity": "compatible",
        "migration": None,
    }
    assert report["compiler"] == {
        "old": {"current": "0.1.0", "minimum": "0.1.0", "maximum": "0.1.0"},
        "new": {"current": "0.1.0", "minimum": "0.1.0", "maximum": "0.1.0"},
        "severity": "compatible",
        "migration": None,
    }
    assert report["summary"] == {
        "additive": 3,
        "breaking": 1,
        "compatible": 0,
        "deprecated": 1,
    }
    assert [
        (item["surface_kind"], item["severity"]) for item in report["changes"]
    ] == [
        ("cli", "breaking"),
        ("stdlib", "deprecated"),
        ("language", "additive"),
        ("stdlib", "additive"),
        ("schema", "additive"),
    ]

    extraction = subprocess.run(
        [sys.executable, str(EXTRACTOR), "--check"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    assert extraction.returncode == 0, extraction.stdout + extraction.stderr

    baseline_payload = load(BASELINE)
    current_payload = load(CURRENT)
    baseline_ids = [item["id"] for item in baseline_payload["surfaces"]]
    current_ids = [item["id"] for item in current_payload["surfaces"]]
    new_package_trust_ids = {
        "axiom://schema/axiom-package-signature-v1",
        "axiom://schema/axiom-package-verification-expectation-v1",
        "axiom://schema/axiom-package-verification-v1",
        "axiom://schema/axiom-registry-index-v2",
        "axiom://schema/axiom-trust-roots-v1",
    }
    new_main_schema_ids = {
        "axiom://schema/axiom.lsp.v1",
        "axiom://schema/axiom.provider-abi.v1",
        "axiom://schema/axiom.runtime_http_server.v1",
        "axiom://schema/axiom.runtime_observability.v1",
        "axiom://schema/axiom.runtime_lifecycle.v1",
        "axiom://schema/axiom.semantic_mir.v1",
    }
    new_target_support_schema_ids = {
        "axiom://schema/axiom-target-support-v1",
    }
    new_quality_schema_ids = {
        "axiom://schema/axiom-quality-policy-v1",
        "axiom://schema/axiom-quality-report-v1",
    }
    new_package_resolver_ids = {
        "axiom://schema/axiom.compiler.package_graph.runtime.v1",
        "axiom://schema/axiom-lockfile-v2",
        "axiom://schema/axiom-package-resolution-v1",
    }
    new_public_schema_ids = (
        new_package_trust_ids
        | new_main_schema_ids
        | new_quality_schema_ids
        | new_target_support_schema_ids
    )
    new_schema_ids = (
        new_package_trust_ids
        | new_main_schema_ids
        | new_package_resolver_ids
        | new_target_support_schema_ids
    )
    modified_schema_ids = {
        "axiom://schema/axiom-build-lowering-evidence-v1",
        "axiom://schema/axiom.stage1.command",
        "axiom://schema/axiom.stage1.v1",
    }
    assert len(baseline_ids) == 52, "accepted baseline must remain the frozen 52-surface ratchet"
    assert len(current_ids) == 69, "current contract must include package trust, quality, HTTP server v1, Provider ABI, runtime observability, Semantic MIR, runtime lifecycle, target support, persistent LSP, and package resolver schemas"
    assert set(baseline_ids) < set(current_ids)
    assert set(current_ids) - set(baseline_ids) == new_public_schema_ids | new_package_resolver_ids
    assert current_payload["contract_version"] == "0.5.0"
    current_cli = surface(current_payload, "axiom://cli/axiomc")
    assert current_cli["version"] == "0.3.0"
    current_stage1_schema = surface(current_payload, "axiom://schema/axiom.stage1.v1")
    assert current_stage1_schema["version"] == "0.2.0"
    compatibility_doc = (ROOT / "docs/compatibility-v1.md").read_text(encoding="utf-8")
    assert f"current source contract is version `{current_payload['contract_version']}` with {len(current_ids)} surfaces" in compatibility_doc
    assert f"CLI surface is version `{current_cli['version']}`" in compatibility_doc
    current_commands = (
        current_cli["signature"].split("; ", maxsplit=1)[0].split("=")[1].split(",")
    )
    assert {"pkg fetch", "pkg update", "pkg vendor", "pkg verify"} <= set(current_commands)
    canonical = run(
        BASELINE,
        CURRENT,
        policy=CURRENT_POLICY,
        old_policy=BASELINE_POLICY,
    )
    assert canonical.returncode == 0, canonical.stdout + canonical.stderr
    canonical_report = json.loads(canonical.stdout)
    assert canonical_report["summary"] == {
        "additive": 17,
        "breaking": 9,
        "compatible": 0,
        "deprecated": 0,
    }
    expected_changed_ids = new_public_schema_ids | new_package_resolver_ids | modified_schema_ids | {
        "axiom://cli/axiomc",
        "axiom://package/lockfile",
        "axiom://package/manifest",
        "axiom://schema/axiom.compiler.stdlib_catalog.v1",
        "axiom://schema/axiom.toml",
        "axiom://stdlib/catalog",
    }
    assert {
        item["surface_id"] for item in canonical_report["changes"]
    } == expected_changed_ids

    with tempfile.TemporaryDirectory() as directory:
        temporary = Path(directory)
        previous_current = copy.deepcopy(current_payload)
        previous_current["contract_version"] = "0.4.0"
        previous_current["surfaces"] = [
            item
            for item in previous_current["surfaces"]
            if item["id"] != "axiom://schema/axiom.runtime_http_server.v1"
        ]
        previous_path = temporary / "previous-current.json"
        write(previous_path, previous_current)

        ratchet = run(previous_path, CURRENT, policy=CURRENT_POLICY, old_policy=CURRENT_POLICY)
        assert ratchet.returncode == 0, ratchet.stdout + ratchet.stderr
        ratchet_report = json.loads(ratchet.stdout)
        assert ratchet_report["summary"] == {
            "additive": 1,
            "breaking": 0,
            "compatible": 0,
            "deprecated": 0,
        }
        assert ratchet_report["changes"][0]["surface_id"] == "axiom://schema/axiom.runtime_http_server.v1"

        unbumped_current = copy.deepcopy(current_payload)
        unbumped_current["contract_version"] = "0.4.0"
        unbumped_path = temporary / "unbumped-current.json"
        write(unbumped_path, unbumped_current)
        unbumped = run(previous_path, unbumped_path, policy=CURRENT_POLICY, old_policy=CURRENT_POLICY)
        assert unbumped.returncode != 0
        assert "semantic drift requires an increased new.contract_version" in unbumped.stdout
    assert all(
        item["change"] == "added"
        and item["severity"] == "additive"
        and item["surface_kind"] == "schema"
        for item in canonical_report["changes"]
        if item["surface_id"] in new_schema_ids
    )
    cli_change = next(
        item
        for item in canonical_report["changes"]
        if item["surface_id"] == "axiom://cli/axiomc"
    )
    assert cli_change["change"] == "modified"
    assert cli_change["severity"] == "breaking"
    assert cli_change["surface_kind"] == "cli"
    assert cli_change["migration"] == (
        "Existing command invocations require no changes. To adopt registry dependencies, "
        "run axiomc pkg fetch to create the v2 lock and verified cache, use axiomc pkg "
        "update for explicit re-resolution, and run axiomc pkg vendor before "
        "cache-independent locked offline builds. Package Trust v1 remains available "
        "through axiomc pkg verify with exact artifact and trust metadata paths plus "
        "--json."
    )
    for identifier in modified_schema_ids:
        schema_change = next(
            item for item in canonical_report["changes"] if item["surface_id"] == identifier
        )
        assert schema_change["change"] == "modified"
        assert schema_change["severity"] == "breaking"
        assert schema_change["migration"]
    compiler_schema_ids = {
        f"axiom://schema/{path.name.removesuffix('.schema.json')}"
        for path in (ROOT / "stage1/compiler-contracts/schemas").glob("*.schema.json")
    }
    assert compiler_schema_ids <= set(current_ids)
    assert compiler_schema_ids - set(baseline_ids) == (
        new_main_schema_ids - {"axiom://schema/axiom.lsp.v1"}
    ) | {
        "axiom://schema/axiom.compiler.package_graph.runtime.v1"
    }
    with tempfile.TemporaryDirectory() as temporary:
        directory = Path(temporary)
        for identifier in baseline_ids:
            mutated = copy.deepcopy(current_payload)
            mutated_surface = surface(mutated, identifier)
            mutated_surface["signature"] += "; unversioned_mutation=true"
            mutated_surface["version"] = surface(baseline_payload, identifier)["version"]
            expect_failure(
                directory,
                baseline_payload,
                mutated,
                f"changed public surface {identifier} must increase its version",
                policy=CURRENT_POLICY,
            )

        compiler_old = copy.deepcopy(baseline_payload)
        compiler_new = copy.deepcopy(baseline_payload)
        compiler_new["contract_version"] = "0.2.0"
        compiler_new["policy_version"] = "1.1.0"
        compiler_new["compiler"].update(
            {
                "current": "0.2.0",
                "minimum": "0.2.0",
                "maximum": "0.2.0",
                "migration": {
                    "action": "Install compiler 0.2.0.",
                    "replacement": "axiom://compiler/0.2.0",
                },
            }
        )
        compiler_policy = load(CURRENT_POLICY)
        compiler_policy["policy_version"] = "1.1.0"
        compiler_policy["transition"] = {
            "from": "1.0.0",
            "severity": "breaking",
            "migration": "Upgrade projects to the new compiler support line.",
        }
        compiler_policy["compiler_support"].update(
            {
                "current": "0.2.0",
                "minimum": "0.2.0",
                "maximum": "0.2.0",
                "maintenance_line": "0.2.x",
            }
        )
        next(
            row
            for row in compiler_policy["support_matrix"]
            if row["status"] == "current"
        )["compiler"] = "0.2.0"
        compiler_policy_path = directory / "compiler-policy.json"
        write(compiler_policy_path, compiler_policy)
        compiler_old_path = directory / "compiler-old.json"
        compiler_new_path = directory / "compiler-new.json"
        write(compiler_old_path, compiler_old)
        write(compiler_new_path, compiler_new)
        compiler_result = run(
            compiler_old_path,
            compiler_new_path,
            policy=compiler_policy_path,
            old_policy=BASELINE_POLICY,
        )
        assert compiler_result.returncode == 0, compiler_result.stdout
        compiler_report = json.loads(compiler_result.stdout)
        assert compiler_report["policies"] == {
            "old": "1.0.0",
            "new": "1.1.0",
            "severity": "breaking",
            "migration": "Upgrade projects to the new compiler support line.",
        }
        assert compiler_report["compiler"]["severity"] == "breaking"
        assert compiler_report["changes"] == []
        missing_compiler_action = copy.deepcopy(compiler_new)
        missing_compiler_action["compiler"].pop("migration")
        expect_failure(
            directory,
            compiler_old,
            missing_compiler_action,
            "narrowed compiler support range requires new.compiler.migration",
            policy=compiler_policy_path,
            old_policy=BASELINE_POLICY,
        )

        unversioned_policy = copy.deepcopy(compiler_policy)
        unversioned_policy["policy_version"] = "1.0.0"
        unversioned_policy_path = directory / "unversioned-policy.json"
        write(unversioned_policy_path, unversioned_policy)
        unversioned_contract = copy.deepcopy(compiler_new)
        unversioned_contract["policy_version"] = "1.0.0"
        expect_failure(
            directory,
            compiler_old,
            unversioned_contract,
            "policy semantic drift requires an increased new policy_version",
            policy=unversioned_policy_path,
            old_policy=BASELINE_POLICY,
        )

        rollback_policy = copy.deepcopy(compiler_policy)
        rollback_policy["policy_version"] = "0.9.0"
        rollback_policy_path = directory / "rollback-policy.json"
        write(rollback_policy_path, rollback_policy)
        rollback_contract = copy.deepcopy(compiler_new)
        rollback_contract["policy_version"] = "0.9.0"
        expect_failure(
            directory,
            compiler_old,
            rollback_contract,
            "new policy_version must not be lower than old policy_version",
            policy=rollback_policy_path,
            old_policy=BASELINE_POLICY,
        )

        unbumped_contract = copy.deepcopy(compiler_new)
        unbumped_contract["contract_version"] = compiler_old["contract_version"]
        expect_failure(
            directory,
            compiler_old,
            unbumped_contract,
            "semantic drift requires an increased new.contract_version",
            policy=compiler_policy_path,
            old_policy=BASELINE_POLICY,
        )

    extractor = extractor_module()
    abi = load(ROOT / "stage1/compatibility/abi-surface-v1.json")
    readiness = load(ROOT / "stage1/runtime-abi/direct-native-v0.json")
    abi_digest = extractor.semantic_digest(
        extractor.abi_semantic_projection(abi, readiness)
    )
    readiness_mutations = [
        lambda value: next(
            item for item in value["capability_shims"] if item["id"] == "fs.read"
        ).update({"notes": "Changed Rust and Cranelift readiness prose."}),
        lambda value: next(
            item for item in value["capability_shims"] if item["id"] == "fs.read"
        ).update({"evidence": ["changed/readiness/evidence"]}),
        lambda value: next(
            item for item in value["capability_shims"] if item["id"] == "fs.read"
        ).update({"status": "partial"}),
        lambda value: next(
            item
            for item in value["capability_shims"]
            if item["id"] == "network.http.async_server"
        )["blockers"].reverse(),
    ]
    for mutate in readiness_mutations:
        mutated = copy.deepcopy(readiness)
        mutate(mutated)
        assert (
            extractor.semantic_digest(extractor.abi_semantic_projection(abi, mutated))
            == abi_digest
        ), "readiness prose, evidence, status, and blocker order must not change semantic ABI"

    logical_meaning = copy.deepcopy(abi)
    next(
        item
        for item in logical_meaning["capability_shims"]
        if item["id"] == "fs.read"
    )["logical_semantics"] += " Reads preserve deterministic absence."
    assert (
        extractor.semantic_digest(
            extractor.abi_semantic_projection(logical_meaning, readiness)
        )
        != abi_digest
    ), "logical ABI meaning drift must change the digest"

    logical_capability = copy.deepcopy(abi)
    readiness_capability = copy.deepcopy(readiness)
    next(
        item
        for item in logical_capability["capability_shims"]
        if item["id"] == "fs.read"
    )["capability"] = "net"
    next(
        item
        for item in readiness_capability["capability_shims"]
        if item["id"] == "fs.read"
    )["capability"] = "net"
    assert (
        extractor.semantic_digest(
            extractor.abi_semantic_projection(
                logical_capability,
                readiness_capability,
            )
        )
        != abi_digest
    ), "logical ABI capability drift must change the digest"

    mismatched_readiness = copy.deepcopy(readiness)
    next(
        item
        for item in mismatched_readiness["capability_shims"]
        if item["id"] == "fs.read"
    )["capability"] = "net"
    try:
        extractor.abi_semantic_projection(abi, mismatched_readiness)
    except ValueError as error:
        assert "contradict the logical ABI contract" in str(error)
    else:
        raise AssertionError("readiness capability mismatch must fail ABI parity")

    captured_abi = copy.deepcopy(abi)
    captured_abi["value_features"][0]["logical_semantics"] = "Cranelift enum layout"
    try:
        extractor.abi_semantic_projection(captured_abi, readiness)
    except ValueError as error:
        assert "captures a Rust implementation detail" in str(error)
    else:
        raise AssertionError("logical ABI meanings must reject implementation capture")

    manifest_schema = load(ROOT / "stage1/schemas/axiom.toml.schema.json")
    manifest_parser_contract = load(
        ROOT / "stage1/compatibility/manifest-parser-contract-v1.json"
    )
    manifest_projection = extractor.manifest_semantic_projection(
        manifest_schema,
        manifest_parser_contract,
    )
    manifest_digest = extractor.semantic_digest(manifest_projection)
    changed_parser_contract = copy.deepcopy(manifest_parser_contract)
    changed_parser_contract["test_kinds"].append("integration")
    changed_schema = copy.deepcopy(manifest_schema)
    changed_schema["properties"]["tests"]["items"]["properties"]["kind"][
        "enum"
    ].append("integration")
    for changed in (
        extractor.manifest_semantic_projection(changed_schema, changed_parser_contract),
        {
            **manifest_projection,
            "schema": {
                **manifest_projection["schema"],
                "x-parser-parity-mutation": True,
            },
        },
    ):
        assert (
            extractor.semantic_digest(changed) != manifest_digest
        ), "manifest parser or schema drift must change the semantic digest"

    artifact = load(ROOT / "stage1/schemas/axiom-artifacts-v0.schema.json")
    artifact_digest = extractor.semantic_digest(
        extractor.artifact_semantic_projection(artifact)
    )
    legacy_only = copy.deepcopy(artifact)
    kinds = legacy_only["$defs"]["artifact"]["properties"]["kind"]["enum"]
    kinds[kinds.index("legacy_generated_rust")] = "legacy_generated_host_source"
    assert (
        extractor.semantic_digest(extractor.artifact_semantic_projection(legacy_only))
        == artifact_digest
    ), "legacy-only artifact drift must stay outside target-neutral compatibility"
    for mutate in (
        lambda value: value["$defs"]["artifact"]["properties"]["kind"]["enum"].append(
            "target_neutral_new_kind"
        ),
        lambda value: value["$defs"]["artifact"]["required"].append("digest"),
        lambda value: value["$defs"]["artifact"]["properties"].update(
            {"digest": {"type": "string"}}
        ),
    ):
        changed = copy.deepcopy(artifact)
        mutate(changed)
        assert (
            extractor.semantic_digest(extractor.artifact_semantic_projection(changed))
            != artifact_digest
        ), "target-neutral artifact semantic drift must change the digest"

    old_payload = load(OLD)
    new_payload = load(NEW)
    with tempfile.TemporaryDirectory() as temporary:
        directory = Path(temporary)

        missing_kind = copy.deepcopy(new_payload)
        missing_kind["surfaces"] = [
            item for item in missing_kind["surfaces"] if item["kind"] != "artifact"
        ]
        expect_failure(
            directory,
            old_payload,
            missing_kind,
            "new missing required surface kinds: artifact",
        )

        missing_source = copy.deepcopy(new_payload)
        surface(missing_source, "axiom://cli/check")["sources"] = []
        expect_failure(
            directory,
            old_payload,
            missing_source,
            "new.surfaces[3].sources must be a non-empty array",
        )

        unordered = copy.deepcopy(new_payload)
        unordered["surfaces"][0], unordered["surfaces"][1] = (
            unordered["surfaces"][1],
            unordered["surfaces"][0],
        )
        expect_failure(
            directory,
            old_payload,
            unordered,
            "must be deterministically sorted by kind and id",
        )

        unchanged_contract = copy.deepcopy(new_payload)
        unchanged_contract["contract_version"] = old_payload["contract_version"]
        expect_failure(
            directory,
            old_payload,
            unchanged_contract,
            "semantic drift requires an increased new.contract_version",
        )

        bad_signature_bump = copy.deepcopy(new_payload)
        changed_cli = surface(bad_signature_bump, "axiom://cli/check")
        changed_cli["version"] = "1.1.0"
        bad_signature_bump["contract_version"] = "1.1.0"
        expect_failure(
            directory,
            old_payload,
            bad_signature_bump,
            "stable signature change axiom://cli/check requires a major bump",
        )

        compiler_drift = copy.deepcopy(new_payload)
        compiler_drift["compiler"]["current"] = "9.9.9"
        compiler_drift["compiler"]["minimum"] = "9.9.9"
        compiler_drift["compiler"]["maximum"] = "9.9.9"
        expect_failure(
            directory,
            old_payload,
            compiler_drift,
            "new.compiler.current must match selected policy.compiler_support.current",
        )

        edition_old = copy.deepcopy(old_payload)
        edition_old["edition"]["status"] = "supported"
        edition_old_policy = load(POLICY)
        next(
            row
            for row in edition_old_policy["editions"]["supported"]
            if row["id"] == edition_old["edition"]["id"]
        )["status"] = "supported"
        edition_old_policy_path = directory / "edition-old-policy.json"
        write(edition_old_policy_path, edition_old_policy)
        edition_new = copy.deepcopy(old_payload)
        edition_new["snapshot_id"] = "axiom://compatibility/edition-regression/new"
        edition_new["contract_version"] = "1.1.0"
        edition_new["policy_version"] = "1.1.0"
        edition_new_policy = load(POLICY)
        edition_new_policy["policy_version"] = "1.1.0"
        edition_new_policy["transition"] = {
            "from": "1.0.0",
            "severity": "breaking",
            "migration": "Migrate away from the regressed edition policy.",
        }
        edition_new_policy_path = directory / "edition-new-policy.json"
        write(edition_new_policy_path, edition_new_policy)
        expect_failure(
            directory,
            edition_old,
            edition_new,
            "edition 2026 cannot regress from supported to experimental",
            policy=edition_new_policy_path,
            old_policy=edition_old_policy_path,
        )

        for forbidden in (
            "Serde payload layout",
            "repr(C) enum",
            "repr (C) enum",
            "#[repr(align(8))]",
            "Vec<u8>",
            "native host layout",
            "host_layout",
            "enum discriminant",
            "memory alignment",
            "pointer_width=64",
            "target_pointer_width=64",
            "align=8",
            'extern "C" fn',
            "usize",
            "isize",
            "compiler::private_type",
            "compiler :: private_type",
        ):
            capture = copy.deepcopy(new_payload)
            target = surface(capture, "axiom://cli/check")
            target["signature"] = forbidden
            expect_failure(directory, old_payload, capture, "captures a Rust implementation detail")
            try:
                extractor.surface(
                    "axiom://test/capture",
                    "language",
                    "1.0.0",
                    forbidden,
                    [],
                )
            except ValueError as error:
                assert "captures a Rust implementation detail" in str(error)
            else:
                raise AssertionError(f"extractor accepted Rust capture {forbidden!r}")

            migration_capture = copy.deepcopy(new_payload)
            surface(migration_capture, "axiom://cli/check")["migration"]["action"] = forbidden
            expect_failure(
                directory,
                old_payload,
                migration_capture,
                "migration.action captures a Rust implementation detail",
            )

        extractor.surface(
            "axiom://test/option-result",
            "language",
            "1.0.0",
            "Option<Result<Text, Error>>",
            [],
        )
        logical_old = copy.deepcopy(old_payload)
        logical_new = copy.deepcopy(old_payload)
        logical_new["snapshot_id"] = "axiom://compatibility/logical-axiom-types/new"
        for contract in (logical_old, logical_new):
            surface(contract, "axiom://language/loop")["signature"] = (
                "Option<Result<Text, Error>>"
            )
        old_path = directory / "logical-old.json"
        new_path = directory / "logical-new.json"
        write(old_path, logical_old)
        write(new_path, logical_new)
        logical = run(old_path, new_path)
        assert logical.returncode == 0, logical.stdout + logical.stderr

        source_old = copy.deepcopy(old_payload)
        source_new = copy.deepcopy(old_payload)
        source_new["snapshot_id"] = "axiom://compatibility/provenance-only/new"
        surface(source_new, "axiom://cli/check")["sources"][0]["sha256"] = "f" * 64
        old_path = directory / "source-old.json"
        new_path = directory / "source-new.json"
        write(old_path, source_old)
        write(new_path, source_new)
        result = run(old_path, new_path)
        assert result.returncode == 0, result.stdout + result.stderr
        assert json.loads(result.stdout)["changes"] == []

        removal_old = copy.deepcopy(new_payload)
        surface(removal_old, "axiom://stdlib/text/lines")["version"] = "9.0.0"
        removal_new = copy.deepcopy(new_payload)
        removal_new["snapshot_id"] = "axiom://compatibility/removal/new"
        removal_new["contract_version"] = "3.0.0"
        removal_new["surfaces"] = [
            item
            for item in removal_new["surfaces"]
            if item["id"] != "axiom://stdlib/text/lines"
        ]
        removal_new["migrations"] = {
            "axiom://stdlib/text/lines": {
                "action": "Use text.split_lines.",
                "removed_in": "3.0.0",
                "removed_on": "2026-07-01",
                "replacement": "axiom://stdlib/text/split-lines",
            }
        }
        old_path = directory / "removal-old.json"
        new_path = directory / "removal-new.json"
        write(old_path, removal_old)
        write(new_path, removal_new)
        result = run(old_path, new_path)
        assert result.returncode == 0, result.stdout + result.stderr
        assert any(item["change"] == "removed" for item in json.loads(result.stdout)["changes"])

        dangling_deprecation = copy.deepcopy(new_payload)
        deprecated_lines = surface(
            dangling_deprecation,
            "axiom://stdlib/text/lines",
        )
        deprecated_lines["replacement"] = "axiom://stdlib/text/missing"
        deprecated_lines["migration"]["replacement"] = "axiom://stdlib/text/missing"
        expect_failure(
            directory,
            old_payload,
            dangling_deprecation,
            "replacement must name a different surviving surface",
        )
        self_replacement = copy.deepcopy(new_payload)
        deprecated_lines = surface(
            self_replacement,
            "axiom://stdlib/text/lines",
        )
        deprecated_lines["replacement"] = deprecated_lines["id"]
        deprecated_lines["migration"]["replacement"] = deprecated_lines["id"]
        expect_failure(
            directory,
            old_payload,
            self_replacement,
            "replacement must name a different surviving surface",
        )
        cyclic_replacements = copy.deepcopy(new_payload)
        cycle_lines = surface(cyclic_replacements, "axiom://stdlib/text/lines")
        cycle_split = surface(cyclic_replacements, "axiom://stdlib/text/split-lines")
        cycle_split.update(
            {
                "stability": "deprecated",
                "replacement": cycle_lines["id"],
                "migration": {
                    "action": "Use text.lines.",
                    "replacement": cycle_lines["id"],
                },
                "deprecation": copy.deepcopy(cycle_lines["deprecation"]),
            }
        )
        expect_failure(
            directory,
            old_payload,
            cyclic_replacements,
            "replacement graph contains a cycle",
        )
        blank_action = copy.deepcopy(new_payload)
        surface(blank_action, "axiom://cli/check")["migration"]["action"] = "   "
        expect_failure(
            directory,
            old_payload,
            blank_action,
            "must be a non-empty string",
        )

        fabricated_migration = copy.deepcopy(new_payload)
        fabricated_migration["migrations"] = {
            "axiom://stdlib/text/split-lines": {
                "action": "Unused migration.",
                "removed_in": "2.0.0",
                "removed_on": "2026-07-01",
                "replacement": "axiom://language/loop",
            }
        }
        expect_failure(
            directory,
            old_payload,
            fabricated_migration,
            "new.migrations keys must exactly equal removed public surfaces",
        )

        missing_migration = copy.deepcopy(removal_new)
        missing_migration["migrations"] = {}
        expect_failure(
            directory,
            removal_old,
            missing_migration,
            "new.migrations keys must exactly equal removed public surfaces",
        )

        missing_replacement_target_old = copy.deepcopy(removal_old)
        replacement_surface = surface(
            missing_replacement_target_old,
            "axiom://stdlib/text/split-lines",
        )
        replacement_surface.update(
            {
                "stability": "deprecated",
                "replacement": "axiom://language/loop",
                "migration": {
                    "action": "Use the language loop surface.",
                    "replacement": "axiom://language/loop",
                },
                "deprecation": {
                    "announced_on": "2026-01-01",
                    "remove_after": "2026-07-01",
                    "supported_editions": ["2026", "2027"],
                },
            }
        )
        missing_replacement_target_new = copy.deepcopy(removal_new)
        missing_replacement_target_new["surfaces"] = [
            item
            for item in missing_replacement_target_new["surfaces"]
            if item["id"] != "axiom://stdlib/text/split-lines"
        ]
        surviving_stdlib = copy.deepcopy(
            surface(removal_old, "axiom://stdlib/text/split-lines")
        )
        surviving_stdlib.update(
            {
                "id": "axiom://stdlib/text/survivor",
                "stability": "stable",
            }
        )
        for field in ("replacement", "migration", "deprecation"):
            surviving_stdlib.pop(field, None)
        missing_replacement_target_new["surfaces"].insert(1, surviving_stdlib)
        missing_replacement_target_new["migrations"][
            "axiom://stdlib/text/split-lines"
        ] = {
            "action": "Use the language loop surface.",
            "removed_in": "3.0.0",
            "removed_on": "2026-07-01",
            "replacement": "axiom://language/loop",
        }
        expect_failure(
            directory,
            missing_replacement_target_old,
            missing_replacement_target_new,
            "replacement must survive in the new contract",
        )

        non_major_removal = copy.deepcopy(removal_new)
        non_major_removal["contract_version"] = "2.1.0"
        non_major_removal["migrations"]["axiom://stdlib/text/lines"][
            "removed_in"
        ] = "2.1.0"
        expect_failure(
            directory,
            removal_old,
            non_major_removal,
            "requires a major contract_version bump",
        )

        early_removal = copy.deepcopy(removal_new)
        early_removal["migrations"]["axiom://stdlib/text/lines"]["removed_on"] = "2026-06-30"
        expect_failure(
            directory,
            removal_old,
            early_removal,
            "cannot be removed before remove_after",
        )
        wrong_release = copy.deepcopy(removal_new)
        wrong_release["migrations"]["axiom://stdlib/text/lines"]["removed_in"] = "9.0.0"
        expect_failure(
            directory,
            removal_old,
            wrong_release,
            "removed_in must equal new.contract_version",
        )

        current_policy_result = run(
            OLD,
            NEW,
            policy=CURRENT_POLICY,
            old_policy=CURRENT_POLICY,
        )
        assert current_policy_result.returncode != 0
        assert (
            "supported_editions are absent from selected policy history"
            in current_policy_result.stdout
        )

        bad_policy = load(POLICY)
        bad_policy["compiler_support"]["maintenance_line"] = "8.8.x"
        policy_path = directory / "bad-policy.json"
        write(policy_path, bad_policy)
        expect_failure(
            directory,
            old_payload,
            new_payload,
            "policy compiler maintenance_line must be 0.1.x",
            policy=policy_path,
        )
        bad_policy = load(POLICY)
        current_row = next(
            row for row in bad_policy["support_matrix"] if row["status"] == "current"
        )
        current_row["compiler"] = "9.9.9"
        write(policy_path, bad_policy)
        expect_failure(
            directory,
            old_payload,
            new_payload,
            "support_matrix must exactly model",
            policy=policy_path,
        )
        for mutate in (
            lambda value: value["support_matrix"][1].update({"status": "previous"}),
            lambda value: value["support_matrix"][1].update({"compiler": "0.0.9"}),
            lambda value: value["support_matrix"][1].update({"edition": "2099"}),
        ):
            bad_policy = load(POLICY)
            mutate(bad_policy)
            write(policy_path, bad_policy)
            expect_failure(
                directory,
                old_payload,
                new_payload,
                "support_matrix must exactly model",
                policy=policy_path,
            )
        for mutate in (
            lambda value: value.update({"unexpected": True}),
            lambda value: value["compiler_support"].update({"unexpected": True}),
            lambda value: value["release_state"].update({"pre_1_0_rule": ""}),
            lambda value: value["evolution"]["language"].update({"identity": ""}),
            lambda value: value.update({"policy_version": "01.0.0"}),
            lambda value: value["editions"].update(
                {"lifecycle": ["experimental", "supported", "deprecated"]}
            ),
            lambda value: value["editions"].update(
                {
                    "lifecycle": [
                        "supported",
                        "experimental",
                        "deprecated",
                        "removed",
                    ]
                }
            ),
            lambda value: value["editions"].update(
                {
                    "lifecycle": [
                        "experimental",
                        "supported",
                        "deprecated",
                        "removed",
                        "future",
                    ]
                }
            ),
        ):
            bad_policy = load(POLICY)
            mutate(bad_policy)
            write(policy_path, bad_policy)
            expect_failure(
                directory,
                old_payload,
                new_payload,
                "policy schema violation",
                policy=policy_path,
            )
        for mutate in (
            lambda value: value["editions"].update(
                {"supported": list(reversed(value["editions"]["supported"]))}
            ),
            lambda value: value["editions"]["supported"].append(
                {
                    **copy.deepcopy(value["editions"]["supported"][0]),
                    "status": "supported",
                }
            ),
        ):
            bad_policy = load(POLICY)
            mutate(bad_policy)
            write(policy_path, bad_policy)
            expect_failure(
                directory,
                old_payload,
                new_payload,
                "policy.editions.supported IDs must be unique and deterministically sorted",
                policy=policy_path,
            )

    print("compatibility v1 regression cases passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
