#!/usr/bin/env python3
import importlib.util
import json
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

try:
    import jsonschema
except ModuleNotFoundError:
    jsonschema = None

ROOT = Path(__file__).resolve().parents[2]
RUNNER = ROOT / "scripts/ci/run-toolchain-qualification.py"
SHA = "0123456789abcdef0123456789abcdef01234567"
BASE_SHA = "89abcdef0123456789abcdef0123456789abcdef"
SPEC = importlib.util.spec_from_file_location("toolchain_qualification", RUNNER)
assert SPEC is not None
assert SPEC.loader is not None
toolchain_qualification = importlib.util.module_from_spec(SPEC)
sys.modules["toolchain_qualification"] = toolchain_qualification
SPEC.loader.exec_module(toolchain_qualification)


class QualificationTests(unittest.TestCase):
    def run_plan(self, checks, repo_root=ROOT, base_sha=None):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        base = Path(temp.name)
        plan = base / "plan.json"
        output = base / "out"
        plan.write_text(json.dumps({"checks": checks}), encoding="utf-8")
        command = [
            sys.executable, str(RUNNER), "--repo-root", str(repo_root), "--output-dir", str(output),
            "--plan", str(plan), "--head-sha", SHA, "--target", "fixture-target",
            "--trigger", "fixture", "--fixture-duration-ms", "7",
        ]
        if base_sha is not None:
            command.extend(["--base-sha", base_sha])
        result = subprocess.run(
            command,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        payload = json.loads((output / "toolchain-qualification.json").read_text())
        return result, payload, output

    def fixture_repo(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        root = Path(temp.name)
        schema_dir = root / "stage1/schemas"
        schema_dir.mkdir(parents=True)
        shutil.copy2(
            ROOT / "stage1/schemas/axiom-toolchain-qualification-v0.schema.json",
            schema_dir / "axiom-toolchain-qualification-v0.schema.json",
        )
        return root

    def test_default_quality_gate_is_exact_head_bound_and_preserves_evidence(self):
        quality = next(
            check
            for check in toolchain_qualification.DEFAULT_CHECKS
            if check["id"] == "stage1_quality_gate"
        )
        self.assertIn(
            '--expected-head "$AXIOM_QUALIFICATION_HEAD_SHA"',
            quality["command"],
        )
        self.assertEqual(["cargo-llvm-cov"], quality["requiredTools"])
        self.assertEqual(
            [
                ".axiom-build/reports/stage1-coverage.lcov",
                ".axiom-build/reports/stage1-quality-report.json",
            ],
            quality["artifactPaths"],
        )

    def test_default_plan_contains_bounded_blocking_mutation_smoke(self):
        check = next(
            check
            for check in toolchain_qualification.DEFAULT_CHECKS
            if check["id"] == "mutation_quality_smoke"
        )
        self.assertIn("--fail-on-survivors", check["command"])
        self.assertIn("--per-mutant-budget-seconds 90", check["command"])
        self.assertIn("--total-budget-seconds 300", check["command"])
        self.assertIn(
            '--expected-head "$AXIOM_QUALIFICATION_HEAD_SHA"',
            check["command"],
        )
        self.assertEqual(
            [".axiom-build/reports/mutation-rust-smoke.json"],
            check["artifactPaths"],
        )

    def test_default_plan_isolates_generated_artifacts_and_requires_go(self):
        direct_native = next(
            check
            for check in toolchain_qualification.DEFAULT_CHECKS
            if check["id"] == "direct_native_abi"
        )
        self.assertIn(
            "CARGO_TARGET_DIR=stage1/target/direct-native-runtime-abi",
            direct_native["command"],
        )

        benchmark = next(
            check
            for check in toolchain_qualification.DEFAULT_CHECKS
            if check["id"] == "benchmark_comparison"
        )
        self.assertEqual(["go"], benchmark["requiredTools"])

        parser_fuzz = next(
            check
            for check in toolchain_qualification.DEFAULT_CHECKS
            if check["id"] == "parser_fuzz_smoke"
        )
        self.assertIn("--cases 64", parser_fuzz["command"])
        self.assertIn("--timeout-ms 2000", parser_fuzz["command"])
        self.assertIn(
            '--expected-head "$AXIOM_QUALIFICATION_HEAD_SHA"',
            parser_fuzz["command"],
        )
        self.assertEqual(
            [".axiom-build/reports/stage1-parser-fuzz.json"],
            parser_fuzz["artifactPaths"],
        )

    def test_default_plan_exercises_all_cargo_targets_without_include_only_helpers(self):
        full_suite = next(
            check
            for check in toolchain_qualification.DEFAULT_CHECKS
            if check["id"] == "full_crate_integration"
        )
        full_tokens = shlex.split(full_suite["command"])
        self.assertIn("--all-targets", full_tokens)
        self.assertNotIn("--test", full_tokens)
        tests_dir = ROOT / "stage1/crates/axiomc/tests"
        self.assertFalse((tests_dir / "hir_unit.rs").exists())
        self.assertFalse((tests_dir / "lib_unit.rs").exists())
        self.assertTrue((tests_dir / "support/hir_unit.rs").is_file())
        self.assertTrue((tests_dir / "support/lib_unit.rs").is_file())

        lsp = next(
            check
            for check in toolchain_qualification.DEFAULT_CHECKS
            if check["id"] == "lsp_protocol_smoke"
        )
        lsp_tokens = shlex.split(lsp["command"].split("&&", 1)[0])
        self.assertIn("--lib", lsp_tokens)
        self.assertIn("--test", lsp_tokens)
        self.assertEqual("lsp_stdio", lsp_tokens[lsp_tokens.index("--test") + 1])
        self.assertNotIn("hir_unit", lsp_tokens)
        self.assertNotIn("lib_unit", lsp_tokens)

    def test_declared_artifact_is_copied_and_listed_without_cargo(self):
        root = self.fixture_repo()
        checks = [{
            "id": "mutation_quality_smoke",
            "command": (
                "mkdir -p generated && "
                "printf '{\"schema\":\"fixture\"}\\n' > generated/mutation.json"
            ),
            "artifactPaths": ["generated/mutation.json"],
        }]
        result, payload, output = self.run_plan(checks, repo_root=root)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(
            {"schema": "fixture"},
            json.loads((output / "mutation.json").read_text(encoding="utf-8")),
        )
        self.assertEqual(
            ["mutation_quality_smoke.log", "mutation.json"],
            payload["checks"][0]["artifacts"],
        )
        self.assertIn("mutation.json", payload["artifactPaths"])

    def test_success_without_declared_artifact_fails_closed(self):
        root = self.fixture_repo()
        result, payload, _ = self.run_plan([{
            "id": "mutation_quality_smoke",
            "command": "printf pass",
            "artifactPaths": ["generated/missing.json"],
        }], repo_root=root)
        self.assertEqual(1, result.returncode)
        self.assertEqual("failed", payload["status"])
        self.assertEqual("product_failure", payload["failureClass"])
        self.assertEqual("product_failure", payload["checks"][0]["failureClass"])

    def test_failed_check_preserves_the_report_it_produced(self):
        root = self.fixture_repo()
        result, payload, output = self.run_plan([{
            "id": "mutation_quality_smoke",
            "command": (
                "mkdir -p generated && "
                "printf '{\"status\":\"failed\"}\\n' > generated/mutation.json; "
                "exit 3"
            ),
            "artifactPaths": ["generated/mutation.json"],
        }], repo_root=root)
        self.assertEqual(1, result.returncode)
        self.assertEqual(3, payload["checks"][0]["exitCode"])
        self.assertEqual(
            {"status": "failed"},
            json.loads((output / "mutation.json").read_text(encoding="utf-8")),
        )
        self.assertIn("mutation.json", payload["checks"][0]["artifacts"])

    def test_preexisting_report_is_not_reused_when_check_does_not_regenerate_it(self):
        root = self.fixture_repo()
        generated = root / "generated"
        generated.mkdir()
        (generated / "mutation.json").write_text(
            '{"status":"stale"}\n', encoding="utf-8"
        )
        result, payload, output = self.run_plan([{
            "id": "mutation_quality_smoke",
            "command": "printf pass",
            "artifactPaths": ["generated/mutation.json"],
        }], repo_root=root)
        self.assertEqual(1, result.returncode)
        self.assertEqual("product_failure", payload["checks"][0]["failureClass"])
        self.assertEqual(
            ["mutation_quality_smoke.log"],
            payload["checks"][0]["artifacts"],
        )
        self.assertFalse((output / "mutation.json").exists())
        self.assertIn(
            "stale declared artifacts not regenerated",
            (output / "mutation_quality_smoke.log").read_text(encoding="utf-8"),
        )

    def test_success_is_deterministic_and_exact(self):
        checks = [{"id": "full_crate_integration", "command": "printf pass"}]
        first, payload, _ = self.run_plan(checks)
        second, repeated, _ = self.run_plan(checks)
        self.assertEqual(0, first.returncode)
        self.assertEqual(0, second.returncode)
        self.assertEqual(payload, repeated)
        self.assertEqual(SHA, payload["headSha"])
        self.assertEqual("fixture-target", payload["target"])
        self.assertEqual("fixture", payload["trigger"])
        self.assertEqual(7, payload["durationMs"])
        self.assertEqual("none", payload["failureClass"])

    def test_emitted_evidence_validates_against_the_real_schema(self):
        if jsonschema is None:
            self.skipTest("optional Python jsonschema package is unavailable")
        checks = [{"id": "full_crate_integration", "command": "printf pass"}]
        result, payload, _ = self.run_plan(checks)
        self.assertEqual(0, result.returncode, result.stderr)
        schema = json.loads(
            (
                ROOT
                / "stage1/schemas/axiom-toolchain-qualification-v0.schema.json"
            ).read_text(encoding="utf-8")
        )
        jsonschema.Draft202012Validator(schema).validate(payload)

    def test_dependency_free_evidence_validator_rejects_unknown_and_wrong_types(self):
        checks = [{"id": "full_crate_integration", "command": "printf pass"}]
        result, payload, _ = self.run_plan(checks)
        self.assertEqual(0, result.returncode, result.stderr)
        schema = (
            ROOT
            / "stage1/schemas/axiom-toolchain-qualification-v0.schema.json"
        )
        toolchain_qualification.validate_qualification_evidence(payload, schema)

        unknown = json.loads(json.dumps(payload))
        unknown["unexpected"] = True
        with self.assertRaisesRegex(ValueError, "unknown"):
            toolchain_qualification.validate_qualification_evidence(
                unknown, schema
            )

        wrong_duration = json.loads(json.dumps(payload))
        wrong_duration["durationMs"] = True
        with self.assertRaisesRegex(ValueError, "nonnegative integer"):
            toolchain_qualification.validate_qualification_evidence(
                wrong_duration, schema
            )

        impossible = json.loads(json.dumps(payload))
        impossible["failureClass"] = "product_failure"
        with self.assertRaisesRegex(ValueError, "cannot contain blockers"):
            toolchain_qualification.validate_qualification_evidence(
                impossible, schema
            )

    def test_check_receives_the_bound_qualification_head(self):
        result, payload, _ = self.run_plan([{
            "id": "head_binding",
            "command": (
                f'test "$AXIOM_QUALIFICATION_HEAD_SHA" = "{SHA}"'
            ),
        }])
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual("passed", payload["checks"][0]["status"])

    def test_check_receives_the_optional_comparison_base(self):
        result, payload, _ = self.run_plan(
            [{
                "id": "base_binding",
                "command": (
                    f'test "$AXIOM_QUALIFICATION_BASE_SHA" = "{BASE_SHA}"'
                ),
            }],
            base_sha=BASE_SHA,
        )
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual("passed", payload["checks"][0]["status"])

    def test_check_receives_empty_comparison_base_when_omitted(self):
        result, payload, _ = self.run_plan([{
            "id": "base_binding",
            "command": 'test -z "$AXIOM_QUALIFICATION_BASE_SHA"',
        }])
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual("passed", payload["checks"][0]["status"])

    def test_product_failure_is_not_infrastructure_failure(self):
        result, payload, _ = self.run_plan([{"id": "conformance", "command": "exit 3"}])
        self.assertEqual(1, result.returncode)
        self.assertEqual("product_failure", payload["failureClass"])
        self.assertEqual(3, payload["checks"][0]["exitCode"])

    def test_missing_tool_is_infrastructure_failure(self):
        result, payload, _ = self.run_plan([{"id": "supply_chain", "command": "axiom-tool-that-does-not-exist"}])
        self.assertEqual(1, result.returncode)
        self.assertEqual("infrastructure_failure", payload["failureClass"])
        self.assertEqual("infrastructure_failure", payload["checks"][0]["failureClass"])

    def test_declared_missing_infrastructure_is_a_skip(self):
        result, payload, _ = self.run_plan([{
            "id": "supply_chain", "command": "exit 9",
            "requiredTools": ["axiom-tool-that-does-not-exist"],
        }])
        self.assertEqual(1, result.returncode)
        self.assertEqual("skipped", payload["status"])
        self.assertEqual("infrastructure_skip", payload["failureClass"])
        self.assertEqual("skipped", payload["checks"][0]["status"])

    def test_rejects_non_exact_head_before_running_checks(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        base = Path(temp.name)
        plan = base / "plan.json"
        marker = base / "must-not-exist"
        plan.write_text(json.dumps({"checks": [{
            "id": "conformance", "command": f"touch {marker}"
        }]}), encoding="utf-8")
        result = subprocess.run([
            sys.executable, str(RUNNER), "--repo-root", str(ROOT),
            "--output-dir", str(base / "out"), "--plan", str(plan),
            "--head-sha", "not-a-sha", "--target", "fixture-target",
        ], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
        self.assertNotEqual(0, result.returncode)
        self.assertFalse(marker.exists())

    def test_rejects_non_exact_base_before_running_checks(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        base = Path(temp.name)
        plan = base / "plan.json"
        marker = base / "must-not-exist"
        plan.write_text(json.dumps({"checks": [{
            "id": "conformance", "command": f"touch {marker}"
        }]}), encoding="utf-8")
        result = subprocess.run([
            sys.executable, str(RUNNER), "--repo-root", str(ROOT),
            "--output-dir", str(base / "out"), "--plan", str(plan),
            "--head-sha", SHA, "--base-sha", "not-a-sha",
            "--target", "fixture-target",
        ], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
        self.assertNotEqual(0, result.returncode)
        self.assertIn("--base-sha must be the exact", result.stderr)
        self.assertFalse(marker.exists())

    def test_rejects_artifact_parent_traversal_before_running_checks(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        base = Path(temp.name)
        marker = base / "must-not-exist"
        plan = base / "plan.json"
        plan.write_text(json.dumps({"checks": [{
            "id": "mutation_quality_smoke",
            "command": f"touch {marker}",
            "artifactPaths": ["../outside.json"],
        }]}), encoding="utf-8")
        result = subprocess.run([
            sys.executable, str(RUNNER), "--repo-root", str(ROOT),
            "--output-dir", str(base / "out"), "--plan", str(plan),
            "--head-sha", SHA, "--target", "fixture-target",
        ], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
        self.assertNotEqual(0, result.returncode)
        self.assertFalse(marker.exists())

    def test_rejects_artifact_output_name_collisions_before_running_checks(self):
        root = self.fixture_repo()
        checks = [
            {
                "id": "first",
                "command": "printf first",
                "artifactPaths": ["one/report.json"],
            },
            {
                "id": "second",
                "command": "printf second",
                "artifactPaths": ["two/report.json"],
            },
        ]
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        plan = Path(temp.name) / "plan.json"
        output = Path(temp.name) / "out"
        plan.write_text(json.dumps({"checks": checks}), encoding="utf-8")
        result = subprocess.run([
            sys.executable, str(RUNNER), "--repo-root", str(root),
            "--output-dir", str(output), "--plan", str(plan),
            "--head-sha", SHA, "--target", "fixture-target",
        ], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
        self.assertNotEqual(0, result.returncode)
        self.assertFalse((output / "first.log").exists())
        self.assertFalse((output / "second.log").exists())


if __name__ == "__main__":
    unittest.main()
