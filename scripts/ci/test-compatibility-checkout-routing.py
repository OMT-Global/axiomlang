#!/usr/bin/env python3
"""Verify the trusted compatibility harness compiles PR-head source, not base."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("test-check-compatibility-v1.sh")


class CheckoutRoutingTests(unittest.TestCase):
    def run_harness(self, checkout, cargo_status=0):
        with tempfile.TemporaryDirectory(prefix="compatibility routing ") as temp:
            root = Path(temp)
            trusted = root / "trusted base"
            scripts = trusted / "scripts" / "ci"
            scripts.mkdir(parents=True)
            script = scripts / SCRIPT.name
            shutil.copyfile(SCRIPT, script)
            head = root / "PR head"
            head.mkdir()
            bin_dir = root / "bin"
            bin_dir.mkdir()
            trace = root / "trace.jsonl"
            # External-command fakes record the actual shell invocation. No Rust
            # compiler is needed to test this checkout-selection boundary.
            for name in ("python3", "cargo"):
                fake = bin_dir / name
                fake.write_text(
                    "#!" + os.path.realpath(sys.executable) + "\n"
                    "import json, os, sys\n"
                    "from pathlib import Path\n"
                    "with open(os.environ['ROUTING_TRACE'], 'a') as out:\n"
                    " out.write(json.dumps({'tool': Path(sys.argv[0]).name, "
                    "'args': sys.argv[1:], 'cwd': os.getcwd()}) + '\\n')\n"
                    "sys.exit(int(os.environ['CARGO_STATUS']) if "
                    "Path(sys.argv[0]).name == 'cargo' else 0)\n"
                )
                fake.chmod(0o755)
            env = dict(os.environ, PATH=str(bin_dir) + os.pathsep + os.environ['PATH'],
                       ROUTING_TRACE=str(trace), CARGO_STATUS=str(cargo_status))
            env.pop("AXIOM_CHECKOUT_PATH", None)
            if checkout is not None:
                env["AXIOM_CHECKOUT_PATH"] = str(head) if checkout else ""
            result = subprocess.run(["bash", str(script)], env=env, cwd=root,
                                    text=True, capture_output=True)
            calls = [json.loads(line) for line in trace.read_text().splitlines()]
            self.assertEqual([call['tool'] for call in calls], ['python3'] * 3 + ['cargo'])
            self.assertTrue(all(call['cwd'] == str(trusted) for call in calls))
            self.assertEqual([call['args'] for call in calls[:3]], [
                ['scripts/ci/test-check-compatibility-v1.py'],
                ['scripts/ci/test-check-compatibility-corpus-v1.py'],
                ['scripts/ci/check-compatibility-corpus-v1.py', '--json'],
            ])
            expected = head if checkout else trusted
            self.assertEqual(calls[-1]['args'], [
                'test', '--manifest-path', str(expected / 'stage1' / 'Cargo.toml'),
                '-p', 'axiomc', '--test', 'compatibility_v1', '--test', 'migration_plan_cli',
            ])
            self.assertEqual(result.returncode, cargo_status, result.stderr)

    def test_head_selected_with_spaces(self):
        self.run_harness(True)

    def test_local_default(self):
        self.run_harness(None)

    def test_empty_override_defaults_to_script_checkout(self):
        self.run_harness(False)

    def test_cargo_failure_is_not_swallowed(self):
        self.run_harness(True, cargo_status=17)


if __name__ == '__main__':
    unittest.main()
