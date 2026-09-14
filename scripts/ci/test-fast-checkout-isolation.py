#!/usr/bin/env python3
"""Run trusted readers against PR-only corruptions, never compile Rust."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class ReaderIsolationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory(prefix="axiom PR isolation ")
        cls.addClassCleanup(cls.temp.cleanup)
        cls.head = Path(cls.temp.name) / "head checkout"
        shutil.copytree(ROOT, cls.head, ignore=shutil.ignore_patterns(
            '.git', 'target', '__pycache__', '.trusted-ci', 'node_modules'))
        cls.bin = Path(cls.temp.name) / 'bin'
        cls.bin.mkdir()
        # The shell wrapper's compiler call is outside this data-reader test.
        cargo = cls.bin / 'cargo'
        cargo.write_text('#!/bin/sh\nexit 0\n')
        cargo.chmod(0o755)

    def invoke(self, command, head):
        env = dict(os.environ, AXIOM_CHECKOUT_PATH=str(head),
                   PATH=str(self.bin) + os.pathsep + os.environ['PATH'])
        return subprocess.run(command, cwd=ROOT, env=env, text=True,
                              stdout=subprocess.PIPE, stderr=subprocess.STDOUT)

    def check_reader(self, script, data, args=(), shell=False):
        command = ['bash' if shell else sys.executable,
                   str(ROOT / 'scripts/ci' / script), *args]
        path = self.head / data
        original = path.read_bytes()
        good = self.invoke(command, self.head)
        self.assertEqual(good.returncode, 0, good.stdout)
        try:
            value = json.loads(original)
            value['schema_version'] = 'invalid.PR-only.schema'
            path.write_text(json.dumps(value))
            bad = self.invoke(command, self.head)
            self.assertNotEqual(bad.returncode, 0, f'{script} ignored PR corruption: {bad.stdout}')
            trusted = self.invoke(command, ROOT)
            self.assertEqual(trusted.returncode, 0, trusted.stdout)
        finally:
            path.write_bytes(original)

    def test_provider(self):
        self.check_reader('check-provider-abi-v1.py',
                          'stage1/compiler-contracts/snapshots/provider-abi-v1.json')

    def test_stdlib(self):
        self.check_reader('check-stdlib-catalog.py',
                          'stage1/compiler-contracts/snapshots/stdlib-catalog.json')

    def test_semantic_mir(self):
        self.check_reader('check-semantic-mir-v1.py',
                          'stage1/compiler-contracts/snapshots/semantic-mir-v1.json')

    def test_lifecycle(self):
        self.check_reader('check-runtime-lifecycle-v1.py',
                          'stage1/compiler-contracts/snapshots/runtime-lifecycle-v1.json')

    def test_autonomy(self):
        self.check_reader('run-agent-autonomy-benchmark.py',
                          'stage1/agent-autonomy/benchmark-v0.json', ('--validate-only',))

    def test_compatibility_wrapper(self):
        self.check_reader('test-check-compatibility-v1.sh',
                          'stage1/compatibility/policy-v1.json', shell=True)

    def test_lane_exports_absolute_data_and_target_paths(self):
        source = (ROOT / 'scripts/ci/run-fast-checks.sh').read_text()
        prefix = source.split('bash "$script_repo_root/scripts/ci/check-python-exit-docs.sh"')[0]
        with tempfile.TemporaryDirectory() as temp:
            runner = Path(temp) / 'prefix.sh'
            runner.write_text(prefix + '\npython3 -c \'import os; print(os.environ["AXIOM_CHECKOUT_PATH"]); print(os.environ["CARGO_TARGET_DIR"])\'\n')
            env = dict(os.environ, AXIOM_CHECKOUT_PATH=str(self.head), CARGO_TARGET_DIR='relative-target')
            result = subprocess.run(['bash', str(runner)], env=env, text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.splitlines(), [str(self.head), str(self.head / 'relative-target')])
            env['AXIOM_CHECKOUT_PATH'] = str(Path(temp) / 'absent')
            bad = subprocess.run(['bash', str(runner)], env=env, text=True, capture_output=True)
            self.assertNotEqual(bad.returncode, 0)
            self.assertIn('must identify an Axiom checkout', bad.stderr)

    def test_package_graph_remains_cwd_relative(self):
        result = subprocess.run([sys.executable, str(ROOT / 'scripts/ci/check-package-graph-boundary.py'), '--json'],
                                cwd=self.head, text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
