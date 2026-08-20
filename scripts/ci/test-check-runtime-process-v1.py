#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from collections.abc import Callable
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-runtime-process-v1.py"
spec = importlib.util.spec_from_file_location("check_runtime_process_v1", CHECKER)
assert spec and spec.loader
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


def write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def copy_contract_root(temporary: str) -> Path:
    root = Path(temporary)
    paths = [checker.SCHEMA, checker.SNAPSHOT, *checker.IMPLEMENTATION_EVIDENCE]
    for path in paths:
        (root / path).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / path, root / path)
    fixture_root = root / checker.FIXTURES
    fixture_root.mkdir(parents=True, exist_ok=True)
    for name in checker.FIXTURE_SPECS:
        fixture_path = checker.FIXTURES / f"{name}.json"
        shutil.copy2(ROOT / fixture_path, root / fixture_path)
    return root


def mutate_snapshot(root: Path, mutation: Callable[[dict[str, Any]], None]) -> None:
    snapshot = checker.load(root / checker.SNAPSHOT)
    mutation(snapshot)
    write_json(root / checker.SNAPSHOT, snapshot)


def mutate_fixture(root: Path, name: str, mutation: Callable[[dict[str, Any]], None]) -> None:
    path = root / checker.FIXTURES / f"{name}.json"
    fixture = checker.load(path)
    mutation(fixture)
    write_json(path, fixture)


class RuntimeProcessContractTests(unittest.TestCase):
    def assert_contract_error(self, reason: str, operation: Callable[[], Any]) -> None:
        with self.assertRaises(checker.ContractError) as caught:
            operation()
        self.assertIn(reason, str(caught.exception))

    def test_contract_and_fixtures_pass(self) -> None:
        self.assertEqual(
            checker.validate_contract(ROOT),
            {
                "schema": "axiom.runtime_process.v1",
                "ok": True,
                "fixtures": 9,
                "authority_dimensions": 8,
                "resource_limits": 4,
            },
        )

    def test_cli_is_deterministic(self) -> None:
        first = subprocess.run([sys.executable, str(CHECKER), "--json"], check=True, capture_output=True, text=True)
        second = subprocess.run([sys.executable, str(CHECKER), "--json"], check=True, capture_output=True, text=True)
        self.assertEqual(first.stdout, second.stdout)
        self.assertEqual(
            json.loads(first.stdout),
            {
                "schema": "axiom.runtime_process.v1",
                "ok": True,
                "fixtures": 9,
                "authority_dimensions": 8,
                "resource_limits": 4,
            },
        )

    def test_snapshot_rejects_implicit_shell_with_schema_reason(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)
            mutate_snapshot(root, lambda snapshot: snapshot["command"].update({"implicit_shell": True}))
            self.assert_contract_error("$.command.implicit_shell: const mismatch", lambda: checker.validate_contract(root))

    def test_public_schema_accepts_a_qualified_runtime_complete_implementation(self) -> None:
        schema = checker.load(ROOT / checker.SCHEMA)
        snapshot = checker.load(ROOT / checker.SNAPSHOT)
        snapshot["implementation"] = {
            "tier": "runtime_complete",
            "structured_api": "implemented",
            "legacy_entrypoint": "run_status(command: string): int",
            "legacy_semantics": "compatibility wrapper over the structured runtime process API",
            "evidence": ["axiom://runtime-process/evidence/qualified"],
            "blockers": [],
        }
        checker.validate_schema(snapshot, schema)

    def test_public_schema_rejects_an_incomplete_runtime_complete_claim(self) -> None:
        schema = checker.load(ROOT / checker.SCHEMA)
        snapshot = checker.load(ROOT / checker.SNAPSHOT)
        snapshot["implementation"]["tier"] = "runtime_complete"
        self.assert_contract_error(
            "$.implementation: expected exactly one matching schema",
            lambda: checker.validate_schema(snapshot, schema),
        )

    def test_public_schema_pins_stable_process_semantics(self) -> None:
        mutations = (
            lambda snapshot: snapshot["command"]["fields"].__setitem__(0, "arbitrary"),
            lambda snapshot: snapshot["audit"]["redacted_fields"].__setitem__(0, "not_secret"),
            lambda snapshot: snapshot["resource_limits"]["supported"][0].update({"maximum": 1}),
            lambda snapshot: snapshot["bounds"].update({"max_argv_entries": 1}),
            lambda snapshot: snapshot["inspection_fields"].__setitem__(0, "arbitrary"),
        )
        schema = checker.load(ROOT / checker.SCHEMA)
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                snapshot = checker.load(ROOT / checker.SNAPSHOT)
                mutation(snapshot)
                with self.assertRaises(checker.ContractError):
                    checker.validate_schema(snapshot, schema)

    def test_snapshot_rejects_host_capture_term_at_semantic_boundary(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)

            def mutate(snapshot: dict[str, Any]) -> None:
                snapshot["fixtures"][0]["asserts"][0] += " through Rust"

            mutate_snapshot(root, mutate)
            self.assert_contract_error("leaks host capture terms: rust", lambda: checker.validate_contract(root))

    def test_lifecycle_rejects_transition_outside_exact_set(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)

            def mutate(snapshot: dict[str, Any]) -> None:
                snapshot["lifecycle"]["transitions"][2] = "running->running"

            mutate_snapshot(root, mutate)
            self.assert_contract_error("lifecycle.transitions[2]", lambda: checker.validate_contract(root))

    def test_lifecycle_rejects_unknown_transition_endpoint(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)

            def mutate(snapshot: dict[str, Any]) -> None:
                snapshot["lifecycle"]["transitions"][2] = "running->unknown"

            mutate_snapshot(root, mutate)
            self.assert_contract_error("$.lifecycle.transitions[2]: enum mismatch", lambda: checker.validate_contract(root))

    def test_timeout_fixture_requires_real_cancellation_path(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)
            mutate_fixture(root, "timeout-cancellation", lambda fixture: fixture.pop("cancellation"))
            self.assert_contract_error(
                "fixture.timeout-cancellation: missing fields cancellation",
                lambda: checker.validate_contract(root),
            )

    def test_authority_fixture_requires_allow_and_deny_for_each_dimension(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)

            def mutate(fixture: dict[str, Any]) -> None:
                fixture["checks"][-1]["allow"]["decision"] = "denied"

            mutate_fixture(root, "capability-denied", mutate)
            self.assert_contract_error(
                "fixture.capability-denied.checks[7].allow.decision: expected 'allowed'",
                lambda: checker.validate_contract(root),
            )

    def test_authority_fixture_requires_runtime_owned_reap_after_denial(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)

            def mutate(fixture: dict[str, Any]) -> None:
                fixture["runtime_denial"]["runtime_supervision"]["child_reaped"] = False

            mutate_fixture(root, "capability-denied", mutate)
            self.assert_contract_error(
                "fixture.capability-denied.runtime_denial.runtime_supervision.child_reaped: expected True",
                lambda: checker.validate_contract(root),
            )

    def test_lifecycle_requires_graceful_exit_transitions(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)

            def mutate(snapshot: dict[str, Any]) -> None:
                snapshot["lifecycle"]["transitions"].remove("timed_out->exited")
                snapshot["lifecycle"]["transitions"].append("timed_out->signaled")

            mutate_snapshot(root, mutate)
            self.assert_contract_error("duplicate items", lambda: checker.validate_contract(root))

    def test_schema_requires_initiating_outcome_preservation(self) -> None:
        schema = checker.load(ROOT / checker.SCHEMA)
        snapshot = checker.load(ROOT / checker.SNAPSHOT)
        snapshot["lifecycle"]["initiating_outcome_preservation"]["timed_out"]["outcome"] = "signal"
        self.assert_contract_error(
            "$.lifecycle.initiating_outcome_preservation.timed_out.outcome: const mismatch",
            lambda: checker.validate_schema(snapshot, schema),
        )

    def test_schema_requires_structured_runtime_cleanup(self) -> None:
        schema = checker.load(ROOT / checker.SCHEMA)
        snapshot = checker.load(ROOT / checker.SNAPSHOT)
        snapshot["authority"]["runtime_cleanup"]["abandonment"].pop("child_reaped")
        self.assert_contract_error(
            "$.authority.runtime_cleanup.abandonment: missing child_reaped",
            lambda: checker.validate_schema(snapshot, schema),
        )

    def test_resource_limit_contract_rejects_default_drift(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)

            def mutate(snapshot: dict[str, Any]) -> None:
                snapshot["resource_limits"]["supported"][2]["default"] = 63

            mutate_snapshot(root, mutate)
            self.assert_contract_error(
                "$.resource_limits.supported: const mismatch",
                lambda: checker.validate_contract(root),
            )

    def test_resource_limit_fixture_rejects_wrong_outcome(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)

            def mutate(fixture: dict[str, Any]) -> None:
                fixture["exceeded"]["outcome"] = "signal"

            mutate_fixture(root, "resource-limits", mutate)
            self.assert_contract_error(
                "fixture.resource-limits.exceeded.outcome: expected 'resource_limit_exceeded'",
                lambda: checker.validate_contract(root),
            )

    def test_fixture_metadata_rejects_kind_drift(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)

            def mutate(snapshot: dict[str, Any]) -> None:
                snapshot["fixtures"][4]["kind"] = "positive"

            mutate_snapshot(root, mutate)
            self.assert_contract_error("fixtures[4].kind: expected 'negative'", lambda: checker.validate_contract(root))

    def test_fixture_metadata_rejects_assertion_drift(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)

            def mutate(snapshot: dict[str, Any]) -> None:
                snapshot["fixtures"][4]["asserts"][0] = "limits exist"

            mutate_snapshot(root, mutate)
            self.assert_contract_error("fixtures[4].asserts[0]", lambda: checker.validate_contract(root))

    def test_large_output_fixture_validates_terminal_outcome(self) -> None:
        snapshot = checker.load(ROOT / checker.SNAPSHOT)
        fixture = checker.load(ROOT / checker.FIXTURES / "large-output.json")
        fixture["outcome"] = "signal"
        self.assert_contract_error(
            "fixture.large-output.outcome: expected 'output_limit_exceeded'",
            lambda: checker.validate_fixture("large-output", fixture, snapshot),
        )

    def test_snapshot_rejects_implementation_evidence_set_drift(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)

            def mutate(snapshot: dict[str, Any]) -> None:
                snapshot["implementation"]["evidence"][-1] = "docs/process-v1.md"

            mutate_snapshot(root, mutate)
            self.assert_contract_error("implementation.evidence[4]", lambda: checker.validate_contract(root))

    def test_temporary_root_evidence_mutation_reaches_content_assertion(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)
            path = root / checker.IMPLEMENTATION_EVIDENCE[1]
            text = path.read_text(encoding="utf-8").replace(
                "std::process::Command::new(program)",
                "std::process::Command::new(replaced_program)",
            )
            path.write_text(text, encoding="utf-8")
            self.assert_contract_error(
                "generated-native legacy evidence no longer uses one executable value",
                lambda: checker.validate_contract(root),
            )

    def test_migration_compatibility_is_pinned(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)
            mutate_snapshot(root, lambda snapshot: snapshot["migration"].update({"compatibility": "automatic"}))
            self.assert_contract_error("migration.compatibility", lambda: checker.validate_contract(root))

    def test_migration_dependencies_are_exact(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)

            def mutate(snapshot: dict[str, Any]) -> None:
                snapshot["migration"]["dependencies"][-1] = 9999

            mutate_snapshot(root, mutate)
            self.assert_contract_error("migration.dependencies[5]: expected 1477", lambda: checker.validate_contract(root))

    def test_temporary_root_rejects_extra_fixture_file(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = copy_contract_root(temporary)
            write_json(root / checker.FIXTURES / "extra.json", {})
            self.assert_contract_error(
                "fixture files must exactly match the pinned fixture set",
                lambda: checker.validate_contract(root),
            )


if __name__ == "__main__":
    unittest.main()
