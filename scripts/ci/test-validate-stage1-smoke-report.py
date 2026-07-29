#!/usr/bin/env python3
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VALIDATOR = ROOT / "scripts/ci/validate-stage1-smoke-report.py"


def direct_lowering(*, static_folds: bool = False) -> dict[str, object]:
    return {
        "schema_version": "axiom.build-lowering-evidence.v1",
        "execution_mode": "direct_native_runtime",
        "lowering_mode": (
            "direct_native_runtime_with_static_folds"
            if static_folds
            else "direct_native_runtime"
        ),
        "direct_native_runtime": True,
        "known_value_static_folds": static_folds,
        "legacy_fallback_attempted": False,
    }


def blocked_lowering() -> dict[str, object]:
    return {
        "schema_version": "axiom.build-lowering-evidence.v1",
        "execution_mode": "not_produced",
        "lowering_mode": "runtime_lowering_required",
        "direct_native_runtime": False,
        "known_value_static_folds": False,
        "legacy_fallback_attempted": True,
    }


def direct_case(name: str) -> dict[str, object]:
    return {
        "name": name,
        "ok": True,
        "binary": f"/tmp/{name.replace('/', '-')}",
        "generated_rust": None,
        "lowering": direct_lowering(),
        "error": None,
    }


def blocked_case(name: str) -> dict[str, object]:
    return {
        "name": name,
        "ok": False,
        "binary": None,
        "generated_rust": None,
        "lowering": blocked_lowering(),
        "error": {"code": "backend.runtime_lowering_required"},
    }


def bounded_static_case(name: str) -> dict[str, object]:
    return {
        "name": name,
        "ok": True,
        "binary": f"/tmp/{name.replace('/', '-')}",
        "generated_rust": None,
        "lowering": {
            "schema_version": "axiom.build-lowering-evidence.v1",
            "execution_mode": "bounded_static_output",
            "lowering_mode": "bounded_static_output",
            "direct_native_runtime": False,
            "known_value_static_folds": True,
            "legacy_fallback_attempted": False,
        },
        "error": None,
    }


class SmokeReportValidatorTests(unittest.TestCase):
    def validate(
        self,
        payload: dict[str, object],
        *,
        command: str,
        expectation: str,
        successes: tuple[str, ...] = (),
        bounded_static: tuple[str, ...] = (),
        blocked: tuple[str, ...] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as temp:
            report = Path(temp) / "report.json"
            report.write_text(json.dumps(payload), encoding="utf-8")
            args = [
                sys.executable,
                str(VALIDATOR),
                "--report",
                str(report),
                "--command",
                command,
                "--project",
                "stage1/examples/fixture",
                "--expect",
                expectation,
            ]
            for name in successes:
                args.extend(["--expected-success-case", name])
            for name in bounded_static:
                args.extend(["--expected-bounded-static-case", name])
            if blocked is not None:
                for name in blocked:
                    args.extend(["--expected-blocked-case", name])
            return subprocess.run(
                args,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )

    def test_accepts_exact_direct_native_build_tuple(self):
        lowering = direct_lowering(static_folds=True)
        payload = {
            "backend": "cranelift",
            "ok": True,
            "generated_rust": None,
            "lowering": lowering,
            "packages": [
                {
                    "package_root": "/tmp/project",
                    "generated_rust": None,
                    "lowering": lowering,
                }
            ],
        }
        result = self.validate(
            payload, command="build", expectation="direct-native"
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_rejects_bounded_static_output_as_direct_native(self):
        bounded = {
            "schema_version": "axiom.build-lowering-evidence.v1",
            "execution_mode": "bounded_static_output",
            "lowering_mode": "bounded_static_output",
            "direct_native_runtime": False,
            "known_value_static_folds": True,
            "legacy_fallback_attempted": False,
        }
        payload = {
            "backend": "cranelift",
            "ok": True,
            "generated_rust": None,
            "lowering": bounded,
            "packages": [],
        }
        result = self.validate(
            payload, command="build", expectation="direct-native"
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("contradictory direct-native evidence", result.stderr)

        accepted = self.validate(
            payload, command="build", expectation="bounded-static"
        )
        self.assertEqual(accepted.returncode, 0, accepted.stderr)

    def test_accepts_exact_blocked_build_tuple(self):
        payload = {
            "ok": False,
            "error": {"code": "backend.runtime_lowering_required"},
            "lowering": blocked_lowering(),
        }
        result = self.validate(payload, command="build", expectation="blocked")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_successful_test_requires_a_nonempty_passing_case_list(self):
        empty = {
            "backend": "cranelift",
            "ok": True,
            "generated_rust": None,
            "cases": [],
        }
        result = self.validate(
            empty, command="test", expectation="direct-native"
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("at least one test case", result.stderr)

        failed = {**empty, "cases": [blocked_case("src/regressed_test")]}
        result = self.validate(
            failed, command="test", expectation="direct-native"
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must pass", result.stderr)

    def test_rejects_contradictory_blocked_tuple_and_generated_rust(self):
        lowering = blocked_lowering()
        lowering["direct_native_runtime"] = True
        payload = {
            "ok": False,
            "generated_rust": "/tmp/generated.rs",
            "error": {"code": "backend.runtime_lowering_required"},
            "lowering": lowering,
        }
        result = self.validate(payload, command="build", expectation="blocked")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("emitted generated Rust", result.stderr)

    def test_rejects_binary_for_not_produced_build_and_case(self):
        payload = {
            "ok": False,
            "binary": "/tmp/stale-binary",
            "generated_rust": None,
            "error": {"code": "backend.runtime_lowering_required"},
            "lowering": blocked_lowering(),
        }
        result = self.validate(payload, command="build", expectation="blocked")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("advertised a binary", result.stderr)

        payload = {
            "backend": "cranelift",
            "ok": False,
            "cases": [
                {
                    **blocked_case("src/main_test"),
                    "binary": "/tmp/stale-test-binary",
                }
            ],
        }
        result = self.validate(
            payload,
            command="test",
            expectation="blocked",
            blocked=("src/main_test",),
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("advertised a binary", result.stderr)

    def test_mixed_suite_preserves_exact_supported_and_blocked_cases(self):
        successes = ("src/json_bench", "src/json_snapshot_test")
        blocked = ("src/property_a", "src/property_b")
        payload = {
            "backend": "cranelift",
            "ok": False,
            "cases": [
                *(direct_case(name) for name in successes),
                *(blocked_case(name) for name in blocked),
            ],
        }
        result = self.validate(
            payload,
            command="test",
            expectation="blocked",
            successes=successes,
            blocked=blocked,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

        regressed = {
            **payload,
            "cases": [
                blocked_case("src/json_bench"),
                direct_case("src/json_snapshot_test"),
                *(blocked_case(name) for name in blocked),
            ],
        }
        result = self.validate(
            regressed,
            command="test",
            expectation="blocked",
            successes=successes,
            blocked=blocked,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("changed direct-native cases", result.stderr)

    def test_mixed_suite_distinguishes_duplicate_names_by_lowering_mode(self):
        payload = {
            "backend": "cranelift",
            "ok": False,
            "cases": [
                blocked_case("src/main_test"),
                bounded_static_case("src/main_test"),
            ],
        }
        result = self.validate(
            payload,
            command="test",
            expectation="blocked",
            bounded_static=("src/main_test",),
            blocked=("src/main_test",),
        )
        self.assertEqual(result.returncode, 0, result.stderr)

        duplicate = {
            **payload,
            "cases": [
                *payload["cases"],
                bounded_static_case("src/main_test"),
            ],
        }
        result = self.validate(
            duplicate,
            command="test",
            expectation="blocked",
            bounded_static=("src/main_test",),
            blocked=("src/main_test",),
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("changed bounded-static cases", result.stderr)


if __name__ == "__main__":
    unittest.main()
