#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import io
import os
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

SCRIPT = Path(__file__).with_name("run-mutation-rust-smoke.py")
SPEC = importlib.util.spec_from_file_location("mutation_rust_smoke", SCRIPT)
assert SPEC is not None
assert SPEC.loader is not None
mutation_rust_smoke = importlib.util.module_from_spec(SPEC)
sys.modules["mutation_rust_smoke"] = mutation_rust_smoke
SPEC.loader.exec_module(mutation_rust_smoke)


class MutationRustSmokeTests(unittest.TestCase):
    def mutant(self, path: Path, find: str = "alpha", replace: str = "omega"):
        return mutation_rust_smoke.Mutant(
            name="fixture_mutant",
            area="fixture",
            file=path,
            find=find,
            replace=replace,
            test_filter="fixture_test",
        )

    def result(
        self,
        mutant,
        outcome,
        *,
        clock=lambda: 0.0,
        total=10.0,
    ):
        original_root = mutation_rust_smoke.REPO_ROOT
        mutation_rust_smoke.REPO_ROOT = mutant.file.parent
        outcomes = iter(
            [
                mutation_rust_smoke.TestOutcome("passed", 0, 0.5),
                outcome,
            ]
        )
        try:
            return mutation_rust_smoke.run_profile(
                [mutant],
                head_sha="1" * 40,
                per_mutant_budget=5.0,
                total_budget=total,
                fail_on_survivors=True,
                test_runner=lambda _filter, _timeout: next(outcomes),
                clock=clock,
            )
        finally:
            mutation_rust_smoke.REPO_ROOT = original_root

    def test_unique_anchor_is_restored_after_killed_mutant(self) -> None:
        with tempfile.TemporaryDirectory() as temp_name:
            path = Path(temp_name) / "sample.rs"
            path.write_text("alpha beta\n")
            report = self.result(
                self.mutant(path),
                mutation_rust_smoke.TestOutcome("failed", 1, 3.0),
            )
            self.assertEqual("alpha beta\n", path.read_text())
            self.assertEqual("killed", report["mutants"][0]["status"])
            self.assertEqual("passed", report["status"])

    def test_survivor_is_blocking_and_has_stable_reproducer(self) -> None:
        with tempfile.TemporaryDirectory() as temp_name:
            path = Path(temp_name) / "sample.rs"
            path.write_text("alpha\n")
            report = self.result(
                self.mutant(path),
                mutation_rust_smoke.TestOutcome("passed", 0, 1.0),
            )
            self.assertEqual(1, report["summary"]["survived"])
            self.assertEqual(1, report["summary"]["blocking"])
            self.assertIn("--mutant fixture_mutant", report["survivors"][0]["reproducer"])
            self.assertIn("--expected-head " + "1" * 40, report["survivors"][0]["reproducer"])

    def test_timeout_is_fail_closed_and_restores_source(self) -> None:
        with tempfile.TemporaryDirectory() as temp_name:
            path = Path(temp_name) / "sample.rs"
            path.write_text("alpha\n")
            report = self.result(
                self.mutant(path),
                mutation_rust_smoke.TestOutcome("timeout", None, 5000.0),
            )
            self.assertEqual("timeout", report["mutants"][0]["status"])
            self.assertEqual(1, report["summary"]["blocking"])
            self.assertEqual("alpha\n", path.read_text())

    def test_total_budget_exhaustion_skips_mutation(self) -> None:
        with tempfile.TemporaryDirectory() as temp_name:
            path = Path(temp_name) / "sample.rs"
            path.write_text("alpha\n")
            ticks = iter([0.0, 11.0])
            report = self.result(
                self.mutant(path),
                mutation_rust_smoke.TestOutcome("failed", 1, 1.0),
                clock=lambda: next(ticks),
                total=10.0,
            )
            self.assertEqual("budget_exhausted", report["mutants"][0]["status"])
            self.assertEqual("alpha\n", path.read_text())

    def test_baseline_time_counts_against_per_mutant_budget(self) -> None:
        with tempfile.TemporaryDirectory() as temp_name:
            path = Path(temp_name) / "sample.rs"
            path.write_text("alpha\n")
            ticks = iter([0.0, 0.0, 6.0])
            report = self.result(
                self.mutant(path),
                mutation_rust_smoke.TestOutcome("failed", 1, 1.0),
                clock=lambda: next(ticks),
                total=10.0,
            )
            self.assertEqual("budget_exhausted", report["mutants"][0]["status"])
            self.assertIn(
                "per-mutant budget exhausted by baseline",
                report["mutants"][0]["stderr_tail"],
            )
            self.assertEqual("alpha\n", path.read_text())

    def test_missing_duplicate_and_stale_anchors_are_distinct(self) -> None:
        cases = [
            ("beta\n", "missing_anchor"),
            ("alpha alpha\n", "duplicate_anchor"),
            ("omega\n", "stale_anchor"),
        ]
        with tempfile.TemporaryDirectory() as temp_name:
            for index, (source, expected) in enumerate(cases):
                path = Path(temp_name) / f"sample-{index}.rs"
                path.write_text(source)
                report = self.result(
                    self.mutant(path),
                    mutation_rust_smoke.TestOutcome("failed", 1, 1.0),
                )
                self.assertEqual(expected, report["mutants"][0]["status"])
                self.assertEqual(source, path.read_text())

    def test_run_test_classifies_subprocess_timeout(self) -> None:
        with mock.patch.object(
            mutation_rust_smoke,
            "run_command",
            return_value=mutation_rust_smoke.TestOutcome(
                "timeout", None, 1000.0, "partial", ""
            ),
        ):
            outcome = mutation_rust_smoke.run_test("fixture", 1.0)
        self.assertEqual("timeout", outcome.status)
        self.assertEqual("partial", outcome.stdout)

    def test_timeout_terminates_and_reaps_the_process_group(self) -> None:
        process = mock.Mock()
        process.pid = 4242
        process.returncode = -mutation_rust_smoke.signal.SIGTERM
        process.communicate.side_effect = [
            subprocess.TimeoutExpired(["cargo"], 1.0),
            ("partial", "terminated"),
        ]
        with (
            mock.patch.object(subprocess, "Popen", return_value=process),
            mock.patch.object(mutation_rust_smoke.os, "name", "posix"),
            mock.patch.object(mutation_rust_smoke.os, "killpg") as killpg,
        ):
            outcome = mutation_rust_smoke.run_command(
                ["cargo"], cwd=Path.cwd(), timeout_seconds=1.0
            )
        self.assertEqual("timeout", outcome.status)
        self.assertEqual(
            [
                mock.call(4242, mutation_rust_smoke.signal.SIGTERM),
                mock.call(4242, 0),
                mock.call(4242, mutation_rust_smoke.signal.SIGKILL),
            ],
            killpg.call_args_list,
        )
        self.assertEqual(2, process.communicate.call_count)

    def test_interrupt_at_spawn_ownership_transfer_cleans_up_child(self) -> None:
        process = mock.Mock()
        with (
            mock.patch.object(subprocess, "Popen", return_value=process),
            mock.patch.object(
                mutation_rust_smoke,
                "block_interrupts",
                return_value={mutation_rust_smoke.signal.SIGTERM},
            ),
            mock.patch.object(
                mutation_rust_smoke,
                "restore_interrupt_mask",
                side_effect=mutation_rust_smoke.MutationInterrupted("cancelled"),
            ),
            mock.patch.object(
                mutation_rust_smoke, "terminate_process_group"
            ) as terminate,
            self.assertRaises(mutation_rust_smoke.MutationInterrupted),
        ):
            mutation_rust_smoke.run_command(
                ["cargo"], cwd=Path.cwd(), timeout_seconds=1.0
            )
        terminate.assert_called_once_with(process)

    def test_termination_sequence_defers_interrupt_delivery(self) -> None:
        process = mock.Mock()
        process.pid = 4242
        process.communicate.return_value = ("", "")
        marker = {mutation_rust_smoke.signal.SIGTERM}
        with (
            mock.patch.object(
                mutation_rust_smoke, "block_interrupts", return_value=marker
            ) as block,
            mock.patch.object(
                mutation_rust_smoke, "restore_interrupt_mask"
            ) as restore,
            mock.patch.object(mutation_rust_smoke.os, "name", "posix"),
            mock.patch.object(
                mutation_rust_smoke.os,
                "killpg",
                side_effect=[None, ProcessLookupError],
            ),
        ):
            mutation_rust_smoke.terminate_process_group(process)
        block.assert_called_once_with()
        restore.assert_called_once_with(marker)

    @unittest.skipUnless(sys.platform != "win32", "requires POSIX process groups")
    def test_timeout_terminates_spawned_descendants(self) -> None:
        with tempfile.TemporaryDirectory() as temp_name:
            marker = Path(temp_name) / "child-terminated"
            child_code = (
                "import pathlib,signal,sys,time\n"
                f"marker=pathlib.Path({str(marker)!r})\n"
                "def stop(*_args):\n"
                " marker.write_text('terminated')\n"
                " raise SystemExit(0)\n"
                "signal.signal(signal.SIGTERM, stop)\n"
                "marker.write_text('ready')\n"
                "time.sleep(30)\n"
            )
            parent_code = (
                "import pathlib,subprocess,sys,time\n"
                f"marker=pathlib.Path({str(marker)!r})\n"
                f"subprocess.Popen([sys.executable, '-c', {child_code!r}])\n"
                "deadline=time.monotonic()+5\n"
                "while not marker.exists() and time.monotonic()<deadline:\n"
                " time.sleep(0.01)\n"
                "time.sleep(30)\n"
            )
            outcome = mutation_rust_smoke.run_command(
                [sys.executable, "-c", parent_code],
                cwd=Path(temp_name),
                timeout_seconds=1.0,
            )
            self.assertEqual("timeout", outcome.status)
            self.assertEqual("terminated", marker.read_text())

    @unittest.skipUnless(sys.platform != "win32", "requires POSIX process groups")
    def test_timeout_force_kills_term_ignoring_closed_pipe_descendant(self) -> None:
        with tempfile.TemporaryDirectory() as temp_name:
            pid_file = Path(temp_name) / "child.pid"
            child_code = (
                "import os,pathlib,signal,time\n"
                "signal.signal(signal.SIGTERM, signal.SIG_IGN)\n"
                f"pathlib.Path({str(pid_file)!r}).write_text(str(os.getpid()))\n"
                "time.sleep(30)\n"
            )
            parent_code = (
                "import pathlib,subprocess,sys,time\n"
                f"pid_file=pathlib.Path({str(pid_file)!r})\n"
                "subprocess.Popen("
                f"[sys.executable, '-c', {child_code!r}], "
                "stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)\n"
                "deadline=time.monotonic()+5\n"
                "while not pid_file.exists() and time.monotonic()<deadline:\n"
                " time.sleep(0.01)\n"
                "time.sleep(30)\n"
            )
            outcome = mutation_rust_smoke.run_command(
                [sys.executable, "-c", parent_code],
                cwd=Path(temp_name),
                timeout_seconds=1.0,
            )
            self.assertEqual("timeout", outcome.status)
            child_pid = int(pid_file.read_text())
            deadline = time.monotonic() + 2.0
            while time.monotonic() < deadline:
                try:
                    os.kill(child_pid, 0)
                except ProcessLookupError:
                    break
                time.sleep(0.05)
            else:
                self.fail(f"TERM-ignoring descendant {child_pid} survived timeout")

    def test_run_test_targets_only_the_library_test_harness(self) -> None:
        commands = []

        def passing(command, **_kwargs):
            commands.append(command)
            return mutation_rust_smoke.TestOutcome(
                "passed",
                0,
                1.0,
                "test tests::fixture ... ok\n",
                "",
            )

        with mock.patch.object(mutation_rust_smoke, "run_command", side_effect=passing):
            outcome = mutation_rust_smoke.run_test("fixture", 1.0)
        self.assertEqual("passed", outcome.status)
        self.assertIn("--lib", commands[0])
        self.assertIn("--locked", commands[0])

    def test_nonzero_without_focused_test_failure_is_execution_error(self) -> None:
        with mock.patch.object(
            mutation_rust_smoke,
            "run_command",
            return_value=mutation_rust_smoke.TestOutcome(
                "failed", 101, 1.0, "", "compiler failed"
            ),
        ):
            outcome = mutation_rust_smoke.run_test("fixture", 1.0)
        self.assertEqual("execution_error", outcome.status)
        self.assertIn("did not report an assertion failure", outcome.stderr)

    def test_focused_test_assertion_failure_is_a_kill_candidate(self) -> None:
        with mock.patch.object(
            mutation_rust_smoke,
            "run_command",
            return_value=mutation_rust_smoke.TestOutcome(
                "failed",
                101,
                1.0,
                "test tests::fixture ... FAILED\n",
                "",
            ),
        ):
            outcome = mutation_rust_smoke.run_test("fixture", 1.0)
        self.assertEqual("failed", outcome.status)

    def test_interruption_restores_mutated_source(self) -> None:
        with tempfile.TemporaryDirectory() as temp_name:
            path = Path(temp_name) / "sample.rs"
            path.write_text("alpha\n")
            original_root = mutation_rust_smoke.REPO_ROOT
            mutation_rust_smoke.REPO_ROOT = path.parent
            try:
                with self.assertRaises(mutation_rust_smoke.MutationInterrupted):
                    mutation_rust_smoke.run_mutant(
                        self.mutant(path),
                        head_sha="1" * 40,
                        timeout_seconds=1.0,
                        total_limited=False,
                        per_mutant_budget=5.0,
                        total_budget=10.0,
                        baseline_duration_ms=0.5,
                        test_runner=lambda _filter, _timeout: (_ for _ in ()).throw(
                            mutation_rust_smoke.MutationInterrupted("cancelled")
                        ),
                    )
            finally:
                mutation_rust_smoke.REPO_ROOT = original_root
            self.assertEqual("alpha\n", path.read_text())

    def test_interruption_during_mutation_write_is_guarded_and_restored(self) -> None:
        with tempfile.TemporaryDirectory() as temp_name:
            path = Path(temp_name) / "sample.rs"
            path.write_text("alpha\n")
            original_root = mutation_rust_smoke.REPO_ROOT
            mutation_rust_smoke.REPO_ROOT = path.parent
            original_write_text = Path.write_text

            def interrupted_write(target, content, *args, **kwargs):
                if target == path and content == "omega\n":
                    original_write_text(target, "partial", encoding="utf-8")
                    raise mutation_rust_smoke.MutationInterrupted("cancelled")
                return original_write_text(target, content, *args, **kwargs)

            try:
                with (
                    mock.patch.object(Path, "write_text", new=interrupted_write),
                    self.assertRaises(mutation_rust_smoke.MutationInterrupted),
                ):
                    mutation_rust_smoke.run_mutant(
                        self.mutant(path),
                        head_sha="1" * 40,
                        timeout_seconds=1.0,
                        total_limited=False,
                        per_mutant_budget=5.0,
                        total_budget=10.0,
                        baseline_duration_ms=0.5,
                    )
            finally:
                mutation_rust_smoke.REPO_ROOT = original_root
            self.assertEqual("alpha\n", path.read_text())

    def test_failing_baseline_is_not_counted_as_a_killed_mutant(self) -> None:
        with tempfile.TemporaryDirectory() as temp_name:
            path = Path(temp_name) / "sample.rs"
            path.write_text("alpha\n")
            original_root = mutation_rust_smoke.REPO_ROOT
            mutation_rust_smoke.REPO_ROOT = path.parent
            try:
                report = mutation_rust_smoke.run_profile(
                    [self.mutant(path)],
                    head_sha="1" * 40,
                    per_mutant_budget=5.0,
                    total_budget=10.0,
                    fail_on_survivors=True,
                    test_runner=lambda _filter, _timeout: (
                        mutation_rust_smoke.TestOutcome("failed", 101, 1.0)
                    ),
                    clock=lambda: 0.0,
                )
            finally:
                mutation_rust_smoke.REPO_ROOT = original_root
            self.assertEqual("baseline_failure", report["mutants"][0]["status"])
            self.assertEqual("baseline", report["mutants"][0]["phase"])
            self.assertEqual(0, report["summary"]["killed"])
            self.assertEqual(1, report["summary"]["blocking"])
            self.assertEqual("alpha\n", path.read_text())

    def test_infrastructure_error_exit_takes_precedence_over_survivor_exit(self) -> None:
        survivor = {
            "name": "survivor",
            "area": "fixture",
            "file": "sample.rs",
            "test_filter": "fixture",
            "status": "survived",
            "reproducer": "reproduce survivor",
        }
        timeout = {
            "name": "timeout",
            "area": "fixture",
            "file": "sample.rs",
            "test_filter": "fixture",
            "status": "timeout",
            "reproducer": "reproduce timeout",
        }
        report = mutation_rust_smoke.build_report(
            head_sha="1" * 40,
            per_mutant_budget=5.0,
            total_budget=10.0,
            results=[survivor, timeout],
            fail_on_survivors=True,
        )
        self.assertEqual(2, mutation_rust_smoke.report_exit_code(report))

    def test_dirty_tracked_checkout_is_rejected(self) -> None:
        completed = subprocess.CompletedProcess(
            ["git"], 0, " M stage1/crates/axiomc/src/hir.rs\n", ""
        )
        with mock.patch.object(
            mutation_rust_smoke.subprocess, "run", return_value=completed
        ):
            with self.assertRaisesRegex(RuntimeError, "must match HEAD"):
                mutation_rust_smoke.require_clean_tracked_tree(Path.cwd())

    def test_argument_budgets_must_be_positive_and_finite(self) -> None:
        for value in ("0", "-1", "nan", "inf"):
            with (
                self.assertRaises(SystemExit),
                mock.patch("sys.stderr", new=io.StringIO()),
            ):
                mutation_rust_smoke.parse_args(["--per-mutant-budget-seconds", value])

    def test_expected_head_must_be_an_exact_lowercase_commit(self) -> None:
        for value in ("abc", "A" * 40, "1" * 39, "1" * 41):
            with (
                self.assertRaises(SystemExit),
                mock.patch("sys.stderr", new=io.StringIO()),
            ):
                mutation_rust_smoke.parse_args(["--expected-head", value])

    def test_profile_covers_expected_areas_and_governing_issue(self) -> None:
        areas = {mutant.area for mutant in mutation_rust_smoke.MUTANTS}
        self.assertEqual(areas, {"parser", "hir", "mir", "codegen"})
        self.assertEqual(1463, mutation_rust_smoke.GOVERNING_ISSUE["number"])
        self.assertEqual(
            "https://github.com/OMT-Global/axiomlang/issues/1463",
            mutation_rust_smoke.GOVERNING_ISSUE["url"],
        )


if __name__ == "__main__":
    unittest.main()
