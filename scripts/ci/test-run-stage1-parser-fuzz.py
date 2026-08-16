#!/usr/bin/env python3
"""Hermetic tests for the deterministic parser fuzz smoke profile."""

from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/ci/run-stage1-parser-fuzz.py"
SPEC = importlib.util.spec_from_file_location("stage1_parser_fuzz", SCRIPT)
assert SPEC and SPEC.loader
fuzz = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = fuzz
SPEC.loader.exec_module(fuzz)


class ParserFuzzTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.corpus = self.root / "corpus"
        self.corpus.mkdir()
        (self.corpus / "a.ax").write_text("print 1\n", encoding="utf-8")
        (self.corpus / "b.ax").write_text("match 1 {}\n", encoding="utf-8")
        self.fake = self.root / "axiomc"
        self.fake.write_text(
            "#!/usr/bin/env python3\n"
            "import json, pathlib, sys\n"
            "target = pathlib.Path(sys.argv[-1])\n"
            "source = (target / 'src/main.ax').read_text() if target.is_dir() else target.read_text()\n"
            "ok = '}' not in source\n"
            "print(json.dumps({'schema_version': 'axiom.stage1.v1', 'command': 'parse', 'ok': ok}))\n"
            "raise SystemExit(0 if ok else 1)\n",
            encoding="utf-8",
        )
        self.fake.chmod(0o755)
        subprocess.run(["git", "init", "-q"], cwd=self.root, check=True)
        subprocess.run(["git", "config", "user.name", "fuzz-test"], cwd=self.root, check=True)
        subprocess.run(
            ["git", "config", "user.email", "fuzz-test@example.invalid"],
            cwd=self.root,
            check=True,
        )
        subprocess.run(["git", "add", "."], cwd=self.root, check=True)
        subprocess.run(
            ["git", "-c", "commit.gpgsign=false", "commit", "-m", "fixture"],
            cwd=self.root,
            check=True,
            stdout=subprocess.DEVNULL,
        )

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def args(self, output: Path, *, seed: int = 9) -> object:
        return type(
            "Args",
            (),
            {
                "axiomc": self.fake,
                "corpus": self.corpus,
                "seed": seed,
                "cases": 12,
                "timeout_ms": 2000,
                "budget_seconds": 10.0,
                "expected_head": None,
                "output": output,
            },
        )()

    def test_profile_is_deterministic_and_reports_replay_metadata(self) -> None:
        first = self.root / "first/report.json"
        second = self.root / "second/report.json"
        with patch.object(fuzz, "repo_root", return_value=self.root):
            first_report, first_status = fuzz.run_profile(self.args(first))
            second_report, second_status = fuzz.run_profile(self.args(second))
        self.assertEqual(0, first_status)
        self.assertEqual(0, second_status)
        normalize = lambda report: [
            {key: value for key, value in case.items() if key != "duration_ms"}
            for case in report["cases"]
        ]
        self.assertEqual(normalize(first_report), normalize(second_report))
        self.assertEqual(first_report["corpus_sha256"], second_report["corpus_sha256"])
        self.assertEqual(12, first_report["cases_executed"])
        self.assertEqual("passed", first_report["status"])
        self.assertEqual("axiom.stage1.parser_fuzz.v1", first_report["schema_version"])
        self.assertTrue(all(case["status"] in {"accepted", "diagnostic"} for case in first_report["cases"]))

    def test_parser_classifies_timeout_and_invalid_output(self) -> None:
        timeout_fake = self.root / "timeout-axiomc"
        timeout_fake.write_text(
            "#!/usr/bin/env python3\nimport time\ntime.sleep(1)\n", encoding="utf-8"
        )
        timeout_fake.chmod(0o755)
        result = fuzz.run_parser(timeout_fake, "print 1", 10, self.root)
        self.assertEqual("timeout", result.status)

        invalid_fake = self.root / "invalid-axiomc"
        invalid_fake.write_text(
            "#!/usr/bin/env python3\nprint('not json')\n", encoding="utf-8"
        )
        invalid_fake.chmod(0o755)
        result = fuzz.run_parser(invalid_fake, "print 1", 1000, self.root)
        self.assertEqual("invalid_output", result.status)

    def test_failure_reproducer_is_smaller_when_failure_survives(self) -> None:
        crash_fake = self.root / "crash-axiomc"
        crash_fake.write_text(
            "#!/usr/bin/env python3\nimport os\nos.kill(os.getpid(), 6)\n", encoding="utf-8"
        )
        crash_fake.chmod(0o755)
        source = "line one\nline two\nline three\n"
        result = fuzz.minimize_failure(
            crash_fake,
            source,
            "crash",
            1000,
            self.root,
            fuzz.time.monotonic() + 5,
        )
        self.assertLess(len(result), len(source))


if __name__ == "__main__":
    raise SystemExit(unittest.main())
