#!/usr/bin/env python3
"""Hermetic regression tests for the bounded stage1 quality gate."""

from __future__ import annotations

import importlib.util
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from types import SimpleNamespace

try:
    import jsonschema
except ModuleNotFoundError:
    jsonschema = None


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/ci/run-stage1-quality-gate.py"
SPEC = importlib.util.spec_from_file_location("stage1_quality_gate", SCRIPT)
assert SPEC and SPEC.loader
quality = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = quality
SPEC.loader.exec_module(quality)


def command(root: Path, *arguments: str) -> str:
    completed = subprocess.run(
        list(arguments),
        cwd=root,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    )
    return completed.stdout.strip()


def lcov_for(root: Path, hits: dict[int, int], *, absolute: bool = True) -> str:
    source = root / "stage1/crates/axiomc/src/lib.rs"
    display = str(source) if absolute else "stage1/crates/axiomc/src/lib.rs"
    covered = sum(value > 0 for value in hits.values())
    lines = [f"SF:{display}"]
    lines.extend(f"DA:{line},{value}" for line, value in sorted(hits.items()))
    lines.extend(
        [
            f"LF:{len(hits)}",
            f"LH:{covered}",
            "end_of_record",
        ]
    )
    return "\n".join(lines)


class FakeProcessRunner:
    def __init__(
        self,
        lcov: str,
        *,
        version: str = "cargo-llvm-cov 0.8.5",
        version_status: str = "passed",
        coverage_status: str = "passed",
    ) -> None:
        self.lcov = lcov
        self.version = version
        self.version_status = version_status
        self.coverage_status = coverage_status
        self.calls: list[tuple[list[str], dict[str, str] | None]] = []

    def __call__(
        self,
        invocation: list[str],
        *,
        cwd: Path,
        timeout_seconds: float,
        env: dict[str, str] | None = None,
    ) -> quality.ProcessOutcome:
        self.calls.append((list(invocation), env))
        if invocation == ["cargo", "llvm-cov", "--version"]:
            return quality.ProcessOutcome(
                self.version_status,
                0 if self.version_status == "passed" else None,
                self.version if self.version_status == "passed" else "",
                "" if self.version_status == "passed" else "not found",
                0.01,
            )
        if invocation == ["rustc", "-vV"]:
            return quality.ProcessOutcome(
                "passed", 0, "rustc 1.97.1\nhost: fixture-target\n", "", 0.01
            )
        self.assert_coverage_command(invocation, cwd, env)
        if self.coverage_status == "passed":
            output = Path(invocation[invocation.index("--output-path") + 1])
            output.write_text(self.lcov, encoding="utf-8")
            return quality.ProcessOutcome("passed", 0, "", "", 0.01)
        return quality.ProcessOutcome(
            self.coverage_status,
            None if self.coverage_status == "timeout" else 1,
            "",
            "fixture coverage failure",
            0.01,
        )

    @staticmethod
    def assert_coverage_command(
        invocation: list[str], cwd: Path, env: dict[str, str] | None
    ) -> None:
        expected = [
            "cargo",
            "llvm-cov",
            "--manifest-path",
            str(cwd / "stage1/Cargo.toml"),
            "-p",
            "axiomc",
            "--lib",
            "--bin",
            "axiomc",
            "--locked",
            "--ignore-filename-regex",
            "rustlib/src/rust/",
            "--lcov",
        ]
        if invocation[: len(expected)] != expected:
            raise AssertionError(f"unexpected coverage command: {invocation}")
        if "--no-clean" in invocation:
            raise AssertionError("quality coverage must use a clean llvm-cov run")
        if invocation[-3:] != [
            "--test-threads=1",
            "--skip",
            quality.SKIPPED_TEST,
        ]:
            raise AssertionError(f"unexpected test arguments: {invocation[-3:]}")
        if env is None or env.get("RUST_MIN_STACK") != "8388608":
            raise AssertionError("coverage command lacks required RUST_MIN_STACK")


class QualityGateTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        (self.root / "stage1/crates/axiomc/src").mkdir(parents=True)
        (self.root / "stage1/quality").mkdir(parents=True)
        (self.root / "stage1/schemas").mkdir(parents=True)
        (self.root / "scripts/ci").mkdir(parents=True)
        (self.root / "stage1/Cargo.toml").write_text(
            "[workspace]\nmembers = [\"crates/axiomc\"]\n", encoding="utf-8"
        )
        self.source = self.root / "stage1/crates/axiomc/src/lib.rs"
        self.source.write_text(
            "pub fn stable(value: i32) -> i32 {\n"
            "    let one = 1;\n"
            "    let two = 2;\n"
            "    let three = 3;\n"
            "    let four = 4;\n"
            "    let five = 5;\n"
            "    let six = 6;\n"
            "    value + one + two + three + four + five + six\n"
            "}\n",
            encoding="utf-8",
        )
        command(self.root, "git", "init", "-q")
        command(self.root, "git", "config", "user.name", "Quality Fixture")
        command(
            self.root, "git", "config", "user.email", "quality-fixture@example.invalid"
        )
        command(self.root, "git", "add", ".")
        command(
            self.root,
            "git",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "fixture base",
        )
        self.base = command(self.root, "git", "rev-parse", "HEAD")

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def write_policy(self) -> None:
        payload = {
            "schemaVersion": "axiom.quality_policy.v1",
            "globalLineCoverageFloor": {
                "numerator": 3,
                "denominator": 5,
            },
            "changedLineCoverageFloor": {
                "numerator": 3,
                "denominator": 5,
            },
        }
        (self.root / quality.DEFAULT_POLICY).write_text(
            json.dumps(payload, indent=2) + "\n", encoding="utf-8"
        )

    def commit_head(self, message: str = "fixture head") -> str:
        command(self.root, "git", "add", ".")
        command(
            self.root,
            "git",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            message,
        )
        return command(self.root, "git", "rev-parse", "HEAD")

    def args(
        self, head: str, comparison: str | None = None
    ) -> SimpleNamespace:
        return SimpleNamespace(
            repo_root=self.root,
            expected_head=head,
            comparison_head=comparison,
            policy=Path(quality.DEFAULT_POLICY),
            lcov_output=Path(quality.DEFAULT_LCOV_OUTPUT),
            output=Path(quality.DEFAULT_REPORT_OUTPUT),
            budget_seconds=10.0,
        )

    def report(self) -> dict:
        return json.loads(
            (self.root / quality.DEFAULT_REPORT_OUTPUT).read_text(encoding="utf-8")
        )

    def run_fixture(
        self,
        lcov: str,
        *,
        fake: FakeProcessRunner | None = None,
        comparison: str | None = None,
    ) -> tuple[int, dict, FakeProcessRunner]:
        head = command(self.root, "git", "rev-parse", "HEAD")
        runner = fake or FakeProcessRunner(lcov)
        result = quality.execute_gate(
            self.args(head, comparison), process_runner=runner
        )
        return result, self.report(), runner

    def test_passes_and_publishes_normalized_exact_head_evidence(self) -> None:
        self.write_policy()
        head = self.commit_head()
        result, report, runner = self.run_fixture(
            lcov_for(self.root, {line: 1 for line in range(1, 10)})
        )
        self.assertEqual(result, 0)
        self.assertEqual(report["status"], "passed")
        self.assertEqual(report["headSha"], head)
        self.assertIsNone(report["baseSha"])
        self.assertEqual(report["target"], "fixture-target")
        self.assertEqual(report["coverage"]["changed"]["status"], "not_applicable")
        normalized = (self.root / quality.DEFAULT_LCOV_OUTPUT).read_text()
        self.assertIn("SF:stage1/crates/axiomc/src/lib.rs\n", normalized)
        self.assertTrue(normalized.endswith("end_of_record\n"))
        self.assertEqual(len(runner.calls), 3)
        if jsonschema is not None:
            schema = json.loads(
                (
                    ROOT / "stage1/schemas/axiom-quality-report-v1.schema.json"
                ).read_text()
            )
            jsonschema.Draft202012Validator(schema).validate(report)

    def test_empty_qualification_base_env_means_no_comparison(self) -> None:
        previous = os.environ.get("AXIOM_QUALIFICATION_BASE_SHA")
        os.environ["AXIOM_QUALIFICATION_BASE_SHA"] = ""
        try:
            parsed = quality.parse_args(
                ["--expected-head", "0" * 40]
            )
        finally:
            if previous is None:
                os.environ.pop("AXIOM_QUALIFICATION_BASE_SHA", None)
            else:
                os.environ["AXIOM_QUALIFICATION_BASE_SHA"] = previous
        self.assertIsNone(parsed.comparison_head)

    def test_global_regression_is_a_quality_failure(self) -> None:
        self.write_policy()
        self.commit_head()
        result, report, _ = self.run_fixture(
            lcov_for(self.root, {line: int(line <= 4) for line in range(1, 10)})
        )
        self.assertEqual(result, 1)
        self.assertEqual(report["failureClass"], "quality")
        self.assertTrue((self.root / quality.DEFAULT_LCOV_OUTPUT).is_file())
        self.assertIn(
            "global_coverage_regression",
            {item["code"] for item in report["findings"]},
        )

    def test_changed_line_floor_fails_on_uncovered_added_executable_lines(self) -> None:
        self.source.write_text(
            self.source.read_text()
            .replace("    value +", "    let uncovered = 0;\n    value + uncovered +"),
            encoding="utf-8",
        )
        self.write_policy()
        self.commit_head()
        hits = {line: 1 for line in range(1, 11)}
        hits[8] = 0
        hits[9] = 0
        result, report, _ = self.run_fixture(
            lcov_for(self.root, hits), comparison=self.base
        )
        self.assertEqual(result, 1)
        self.assertEqual(report["coverage"]["changed"]["status"], "failed")
        self.assertIn(
            "changed_coverage_regression",
            {item["code"] for item in report["findings"]},
        )

    def test_explicit_comparison_reports_passing_changed_coverage(self) -> None:
        self.source.write_text(
            self.source.read_text().replace(
                "    value +",
                "    let covered = 7;\n    value + covered +",
            ),
            encoding="utf-8",
        )
        self.write_policy()
        self.commit_head()
        result, report, _ = self.run_fixture(
            lcov_for(self.root, {line: 1 for line in range(1, 11)}),
            comparison=self.base,
        )
        self.assertEqual(result, 0)
        self.assertEqual(report["baseSha"], self.base)
        self.assertEqual(report["coverage"]["changed"]["status"], "passed")
        self.assertNotIn("crap", report)

    def test_lcov_rejects_malformed_truncated_duplicate_and_escape(self) -> None:
        valid = lcov_for(self.root, {1: 1})
        cases = {
            "malformed": valid.replace("DA:1,1", "DA:nope"),
            "truncated": valid.removesuffix("end_of_record"),
            "duplicate_da": valid.replace("DA:1,1", "DA:1,1\nDA:1,1"),
            "duplicate_source": valid + "\n" + valid,
            "path_escape": valid.replace(
                f"SF:{self.source}", "SF:../../outside.rs"
            ),
        }
        for name, payload in cases.items():
            with self.subTest(name=name):
                path = self.root / f"{name}.lcov"
                path.write_text(payload, encoding="utf-8")
                with self.assertRaises(quality.GateError):
                    quality.parse_lcov(path, self.root)

    def test_missing_and_wrong_tool_versions_emit_valid_failure_reports(self) -> None:
        self.write_policy()
        self.commit_head()
        source_lcov = lcov_for(self.root, {line: 1 for line in range(1, 10)})
        stale_lcov = self.root / quality.DEFAULT_LCOV_OUTPUT
        stale_lcov.parent.mkdir(parents=True)
        stale_lcov.write_text("stale", encoding="utf-8")
        missing = FakeProcessRunner(source_lcov, version_status="execution_error")
        result, report, _ = self.run_fixture(source_lcov, fake=missing)
        self.assertEqual(result, 1)
        self.assertEqual(report["findings"][0]["code"], "tool_missing")
        self.assertIsNone(report["artifacts"]["lcov"])
        self.assertFalse(stale_lcov.exists())

        stale_lcov.write_text("stale again", encoding="utf-8")
        wrong = FakeProcessRunner(source_lcov, version="cargo-llvm-cov 0.8.4")
        result, report, _ = self.run_fixture(source_lcov, fake=wrong)
        self.assertEqual(result, 1)
        self.assertEqual(report["findings"][0]["code"], "tool_version_mismatch")
        self.assertFalse(stale_lcov.exists())

    def test_wrong_and_dirty_heads_fail_before_tool_execution(self) -> None:
        self.write_policy()
        head = self.commit_head()
        fake = FakeProcessRunner(lcov_for(self.root, {1: 1}))
        stale_lcov = self.root / quality.DEFAULT_LCOV_OUTPUT
        stale_lcov.parent.mkdir(parents=True)
        stale_lcov.write_text("stale", encoding="utf-8")
        args = self.args("0" * 40)
        self.assertEqual(quality.execute_gate(args, process_runner=fake), 1)
        self.assertEqual(self.report()["findings"][0]["code"], "wrong_head")
        self.assertEqual(fake.calls, [])
        self.assertFalse(stale_lcov.exists())

        self.source.write_text(self.source.read_text() + "// dirty\n")
        stale_lcov.write_text("stale again", encoding="utf-8")
        args.expected_head = head
        self.assertEqual(quality.execute_gate(args, process_runner=fake), 1)
        self.assertEqual(self.report()["findings"][0]["code"], "dirty_checkout")
        self.assertEqual(fake.calls, [])
        self.assertFalse(stale_lcov.exists())

    def test_tracked_report_target_is_never_overwritten(self) -> None:
        self.write_policy()
        tracked_report = self.root / "tracked-report.json"
        original = b"maintainer-owned tracked bytes\n"
        tracked_report.write_bytes(original)
        head = self.commit_head()
        fake = FakeProcessRunner(lcov_for(self.root, {1: 1}))
        args = self.args(head)
        args.output = Path("tracked-report.json")
        self.assertEqual(quality.execute_gate(args, process_runner=fake), 1)
        self.assertEqual(tracked_report.read_bytes(), original)
        self.assertEqual(fake.calls, [])

    def test_nonignored_untracked_input_fails_cleanliness(self) -> None:
        self.write_policy()
        head = self.commit_head()
        unexpected = self.root / "stage1/crates/axiomc/src/untracked.rs"
        unexpected.write_text("pub fn surprise() {}\n", encoding="utf-8")
        fake = FakeProcessRunner(lcov_for(self.root, {1: 1}))
        self.assertEqual(
            quality.execute_gate(self.args(head), process_runner=fake),
            1,
        )
        self.assertEqual(self.report()["findings"][0]["code"], "dirty_checkout")
        self.assertEqual(fake.calls, [])

    def test_concurrent_head_move_publishes_no_lcov(self) -> None:
        self.write_policy()
        head = self.commit_head()
        source_lcov = lcov_for(
            self.root, {line: 1 for line in range(1, 10)}
        )

        class HeadMovingRunner(FakeProcessRunner):
            def __call__(runner_self, invocation, **kwargs):
                outcome = super(HeadMovingRunner, runner_self).__call__(
                    invocation, **kwargs
                )
                if "--output-path" in invocation:
                    command(
                        self.root,
                        "git",
                        "-c",
                        "commit.gpgsign=false",
                        "commit",
                        "--allow-empty",
                        "-qm",
                        "concurrent head move",
                    )
                return outcome

        result = quality.execute_gate(
            self.args(head), process_runner=HeadMovingRunner(source_lcov)
        )
        report = self.report()
        self.assertEqual(result, 1)
        self.assertIn(
            "head_changed", {item["code"] for item in report["findings"]}
        )
        self.assertEqual(report["failureClass"], "provenance")
        self.assertIsNone(report["artifacts"]["lcov"])
        self.assertFalse((self.root / quality.DEFAULT_LCOV_OUTPUT).exists())

    @unittest.skipUnless(os.name == "posix", "signal cancellation requires POSIX")
    def test_sigterm_cancels_and_reaps_coverage_process_group(self) -> None:
        self.write_policy()
        head = self.commit_head()
        pid_directory = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, pid_directory)
        pid_file = pid_directory / "descendant.pid"
        source_lcov = lcov_for(self.root, {1: 1})
        ordinary = FakeProcessRunner(source_lcov)

        def cancellation_runner(invocation, **kwargs):
            if "--output-path" not in invocation:
                return ordinary(invocation, **kwargs)
            shell = (
                "trap '' TERM; "
                "(trap '' TERM; while :; do sleep 1; done) & "
                f"echo $! > {shlex_quote(pid_file)}; "
                "while :; do sleep 1; done"
            )
            timer = threading.Timer(
                0.15, lambda: os.kill(os.getpid(), signal.SIGTERM)
            )
            timer.start()
            try:
                return quality.run_process(
                    ["sh", "-c", shell],
                    cwd=kwargs["cwd"],
                    timeout_seconds=5.0,
                    env=kwargs.get("env"),
                )
            finally:
                timer.join()

        previous = signal.getsignal(signal.SIGTERM)

        def prior_handler(_signum, _frame):
            raise AssertionError("prior handler ran during quality measurement")

        signal.signal(signal.SIGTERM, prior_handler)
        try:
            result = quality.execute_gate(
                self.args(head), process_runner=cancellation_runner
            )
            self.assertIs(signal.getsignal(signal.SIGTERM), prior_handler)
        finally:
            signal.signal(signal.SIGTERM, previous)
        self.assertEqual(result, 1)
        self.assertEqual(
            self.report()["findings"][0]["code"], "quality_gate_cancelled"
        )
        descendant = int(pid_file.read_text().strip())
        deadline = time.monotonic() + 1.0
        while time.monotonic() < deadline:
            try:
                os.kill(descendant, 0)
            except ProcessLookupError:
                break
            time.sleep(0.01)
        else:
            self.fail(f"descendant process {descendant} survived cancellation")
        self.assertFalse((self.root / quality.DEFAULT_LCOV_OUTPUT).exists())

    @unittest.skipUnless(
        hasattr(signal, "pthread_sigmask"),
        "publication masking requires POSIX signal masks",
    )
    def test_sigterm_during_publication_preserves_consistent_artifacts(self) -> None:
        self.write_policy()
        head = self.commit_head()
        source_lcov = lcov_for(self.root, {line: 1 for line in range(1, 10)})
        prior_handler = signal.getsignal(signal.SIGTERM)
        report_states_seen_by_handler: list[bool] = []
        original_atomic_write = quality.atomic_write_text
        report_path = (self.root / quality.DEFAULT_REPORT_OUTPUT).resolve()

        def observe_signal(_signum, _frame):
            report_states_seen_by_handler.append(report_path.is_file())

        def interrupt_report_publication(path, contents):
            if path == report_path:
                os.kill(os.getpid(), signal.SIGTERM)
            original_atomic_write(path, contents)

        signal.signal(signal.SIGTERM, observe_signal)
        quality.atomic_write_text = interrupt_report_publication
        try:
            result = quality.execute_gate(
                self.args(head),
                process_runner=FakeProcessRunner(source_lcov),
            )
            self.assertIs(signal.getsignal(signal.SIGTERM), observe_signal)
        finally:
            quality.atomic_write_text = original_atomic_write
            signal.signal(signal.SIGTERM, prior_handler)

        self.assertEqual(result, 0)
        self.assertEqual(report_states_seen_by_handler, [True])
        self.assertTrue(report_path.is_file())
        self.assertTrue(
            (self.root / quality.DEFAULT_LCOV_OUTPUT).is_file()
        )
        report = self.report()
        self.assertEqual(report["status"], "passed")
        self.assertIsInstance(report["artifacts"]["lcov"], str)

    def test_unavailable_and_nonancestor_comparisons_fail_closed(self) -> None:
        self.write_policy()
        self.commit_head()
        source_lcov = lcov_for(self.root, {1: 1})
        result, report, _ = self.run_fixture(
            source_lcov, comparison="f" * 40
        )
        self.assertEqual(result, 1)
        self.assertEqual(
            report["findings"][0]["code"], "comparison_unavailable"
        )

        other = tempfile.TemporaryDirectory()
        self.addCleanup(other.cleanup)
        other_root = Path(other.name)
        command(other_root, "git", "init", "-q")
        command(other_root, "git", "config", "user.name", "Other")
        command(other_root, "git", "config", "user.email", "other@example.invalid")
        (other_root / "other").write_text("other")
        command(other_root, "git", "add", ".")
        command(
            other_root,
            "git",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "other",
        )
        other_head = command(other_root, "git", "rev-parse", "HEAD")
        command(self.root, "git", "fetch", "-q", str(other_root), other_head)
        result, report, _ = self.run_fixture(
            source_lcov, comparison=other_head
        )
        self.assertEqual(result, 1)
        self.assertEqual(
            report["findings"][0]["code"], "comparison_not_ancestor"
        )

    def test_coverage_timeout_removes_stale_outputs(self) -> None:
        self.write_policy()
        self.commit_head()
        lcov_path = self.root / quality.DEFAULT_LCOV_OUTPUT
        lcov_path.parent.mkdir(parents=True)
        lcov_path.write_text("stale", encoding="utf-8")
        timeout = FakeProcessRunner(
            lcov_for(self.root, {1: 1}), coverage_status="timeout"
        )
        result, report, _ = self.run_fixture(timeout.lcov, fake=timeout)
        self.assertEqual(result, 1)
        self.assertEqual(report["findings"][0]["code"], "coverage_timeout")
        self.assertFalse(lcov_path.exists())

    @unittest.skipUnless(os.name == "posix", "process-group cleanup requires POSIX")
    def test_timeout_kills_term_ignoring_descendant(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            pid_file = Path(directory) / "descendant.pid"
            script = (
                "trap '' TERM; "
                "(trap '' TERM; while :; do sleep 1; done) & "
                f"echo $! > {shlex_quote(pid_file)}; "
                "while :; do sleep 1; done"
            )
            outcome = quality.run_process(
                ["sh", "-c", script],
                cwd=Path(directory),
                timeout_seconds=0.1,
            )
            self.assertEqual(outcome.status, "timeout")
            descendant = int(pid_file.read_text().strip())
            with self.assertRaises(ProcessLookupError):
                os.kill(descendant, 0)

    def test_report_schema_and_producer_reject_contradictory_states(self) -> None:
        self.write_policy()
        self.commit_head()
        result, report, _ = self.run_fixture(
            lcov_for(self.root, {line: 1 for line in range(1, 10)})
        )
        self.assertEqual(result, 0)
        self.assertNotIn("crap", report)

        impossible_counts = json.loads(json.dumps(report))
        impossible_counts["coverage"]["global"]["coveredLines"] = 10
        impossible_counts["coverage"]["global"]["totalLines"] = 9
        with self.assertRaisesRegex(
            quality.GateError, "impossible line counts"
        ):
            quality.validate_report_semantics(impossible_counts)

        below_floor_pass = json.loads(json.dumps(report))
        below_floor_pass["coverage"]["global"]["coveredLines"] = 1
        below_floor_pass["coverage"]["global"]["totalLines"] = 5
        with self.assertRaisesRegex(
            quality.GateError, "status contradicts its floor"
        ):
            quality.validate_report_semantics(below_floor_pass)

        passed_without_lcov = json.loads(json.dumps(report))
        passed_without_lcov["artifacts"]["lcov"] = None
        with self.assertRaisesRegex(
            quality.GateError, "contradictory state"
        ):
            quality.validate_report_semantics(passed_without_lcov)

        reversed_span = json.loads(json.dumps(report))
        reversed_span["status"] = "failed"
        reversed_span["failureClass"] = "quality"
        reversed_span["findings"] = [
            quality.finding(
                code="fixture_failure",
                message="fixture",
                reproducer_command=report["reproducer"],
                semantic_area_name="compiler.stage1",
                path=quality.SOURCE_PREFIX,
                start_line=1,
                end_line=1,
            )
        ]
        reversed_span["findings"][0]["startLine"] = 9
        reversed_span["findings"][0]["endLine"] = 3
        with self.assertRaisesRegex(quality.GateError, "source span"):
            quality.validate_report_semantics(reversed_span)

        contradictory_pass = json.loads(json.dumps(report))
        contradictory_pass["failureClass"] = "quality"
        if jsonschema is not None:
            schema = json.loads(
                (
                    ROOT / "stage1/schemas/axiom-quality-report-v1.schema.json"
                ).read_text()
            )
            validator = jsonschema.Draft202012Validator(schema)
            with self.assertRaises(jsonschema.ValidationError):
                validator.validate(contradictory_pass)
            with self.assertRaises(jsonschema.ValidationError):
                validator.validate(passed_without_lcov)
            contradictory_failure = json.loads(json.dumps(report))
            contradictory_failure["status"] = "failed"
            contradictory_failure["failureClass"] = "quality"
            with self.assertRaises(jsonschema.ValidationError):
                validator.validate(contradictory_failure)

    def test_checked_in_policy_and_schemas_are_valid(self) -> None:
        policy = json.loads(
            (ROOT / "stage1/quality/quality-policy-v1.json").read_text()
        )
        policy_schema = json.loads(
            (
                ROOT
                / "stage1/schemas/axiom-quality-policy-v1.schema.json"
            ).read_text()
        )
        if jsonschema is not None:
            jsonschema.Draft202012Validator(policy_schema).validate(policy)
        self.assertEqual(policy["schemaVersion"], "axiom.quality_policy.v1")
        self.assertEqual(
            policy["globalLineCoverageFloor"],
            {"numerator": 3, "denominator": 5},
        )
        self.assertEqual(
            policy["changedLineCoverageFloor"],
            {"numerator": 3, "denominator": 5},
        )


def shlex_quote(path: Path) -> str:
    import shlex

    return shlex.quote(str(path))


if __name__ == "__main__":
    unittest.main()
