#!/usr/bin/env python3
"""Exercise real fast-check startup; stop before workloads with a PATH spy."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument("--script", type=Path, default=Path(__file__).resolve().with_name("run-fast-checks.sh"))
args = parser.parse_args()
script = args.script.resolve()
repo = script.parents[2]
with tempfile.TemporaryDirectory(prefix="axiom-target-isolation-") as tmp:
    root = Path(tmp)
    spies = root / "bin"
    spies.mkdir()
    git = spies / "git"
    git.write_text('#!/bin/sh\nprintf called > "$AXIOM_TEST_GIT_CALL"\n[ "$*" = "rev-parse --verify HEAD" ] || exit 96\nprintf "%s\\n" "$AXIOM_TEST_COMMIT"\n')
    bash = spies / "bash"
    bash.write_text('#!/bin/sh\n[ "$1" = "$AXIOM_TEST_FIRST_CHECK" ] || exit 97\nprintf "%s\\n" "$CARGO_TARGET_DIR" > "$AXIOM_TEST_CAPTURE"\nexit 77\n')
    git.chmod(0o700)
    bash.chmod(0o700)

    def selected_target(label, commit, override=None):
        capture = root / (label + ".capture")
        git_call = root / (label + ".git")
        env = dict(os.environ)
        env.pop("CARGO_TARGET_DIR", None)
        env.update(PATH=str(spies) + os.pathsep + env["PATH"],
                   RUNNER_TEMP=str(root / "runner-temp"),
                   AXIOM_CHECKOUT_PATH=str(repo),
                   AXIOM_TEST_FIRST_CHECK=str(repo / "scripts/ci/check-python-exit-docs.sh"),
                   AXIOM_TEST_COMMIT=commit, AXIOM_TEST_GIT_CALL=str(git_call),
                   AXIOM_TEST_CAPTURE=str(capture))
        if override is not None:
            env["CARGO_TARGET_DIR"] = str(override)
        result = subprocess.run(["/bin/bash", str(script)], env=env, cwd=repo,
                                capture_output=True, text=True, timeout=15)
        assert result.returncode == 77, (label, result.returncode, result.stderr)
        target = Path(capture.read_text().strip())
        assert target.is_dir(), (label, "target was not created")
        if override is not None:
            assert target == override, "explicit CARGO_TARGET_DIR was not preserved"
            assert not git_call.exists(), "explicit target unnecessarily queried Git"
        else:
            assert target.is_relative_to(root / "runner-temp")
            assert git_call.exists(), "default target did not bind Git HEAD"
        return target

    first = selected_target("first", "1" * 40)
    second = selected_target("second", "2" * 40)
    repeat = selected_target("repeat", "1" * 40)
    assert first != second, "different commits reused a stale default target"
    assert first == repeat, "same commit did not select a deterministic target"
    selected_target("override", "3" * 40, root / "explicit-target")
print("fast-check target isolation: real startup controls passed")
