#!/usr/bin/env python3
"""Exercise the public smoke entrypoints with a deterministic fake compiler."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
TABLE = ROOT / "scripts/ci/stage1-smoke-expectations.json"


def evidence(expectation):
    mode = {"direct-native": "direct_native_runtime", "bounded-static": "bounded_static_output",
            "blocked": "runtime_lowering_required"}[expectation]
    return {"schema_version": "axiom.build-lowering-evidence.v1", "lowering_mode": mode,
            "execution_mode": "not_produced" if expectation == "blocked" else mode,
            "direct_native_runtime": expectation == "direct-native",
            "known_value_static_folds": expectation == "bounded-static",
            "legacy_fallback_attempted": expectation == "blocked"}


def fake_cargo():
    args = sys.argv[1:]
    command, project = args[args.index("--") + 1:][:2]
    example = Path(project).name
    row = next(row for row in json.loads(TABLE.read_text()) if row["example"] == example)
    name = row.get("runtime_env", {}).get("name")
    with open(os.environ["SMOKE_TEST_LOG"], "a") as log:
        log.write(json.dumps({"kind": "cargo", "args": args, "command": command,
                              "example": example, "env": os.environ.get(name) if name else None}) + "\n")
    if command == "check":
        print(json.dumps({"ok": True}))
        return
    if command == "run":
        print("fake program")
        return
    expected = row[command]
    fault = os.environ.get("SMOKE_TEST_FAULT", "")
    if fault == f"success-{command}" and example == "capabilities":
        expected = "direct-native"
    if fault == f"block-env-{command}" and example == "stdlib_env":
        expected = "blocked"
    report = {"ok": expected != "blocked", "backend": "cranelift", "lowering": evidence(expected)}
    error = {"code": "backend.runtime_lowering_required"}
    if fault == f"diagnostic-{command}" and example == "capabilities":
        error = {"code": "backend.unrelated_failure"}
    if expected == "blocked":
        report["error"] = error
    if command == "build" and expected != "blocked":
        binary = Path(os.environ["SMOKE_TEST_LOG"]).parent / (example + "-binary")
        output = repr("none") if fault == "stale-env" else f"os.environ.get({name!r}, 'none')"
        binary.write_text("#!/usr/bin/env python3\nimport json, os\n"
                          f"with open({os.environ['SMOKE_TEST_LOG']!r}, 'a') as log:\n"
                          f" log.write(json.dumps({{'kind':'binary','path':{str(binary)!r},'env':os.environ.get({name!r})}})+'\\n')\n"
                          f"print({output})\n")
        binary.chmod(0o755)
        report["binary"] = str(binary)
    if command == "test":
        groups = [(expected, ["src/smoke_test"])] if expected != "blocked" else [
            ("direct-native", row.get("expected_success_cases", [])),
            ("bounded-static", row.get("expected_bounded_static_cases", [])),
            ("blocked", row.get("expected_blocked_cases", ["src/blocked_test"]))]
        report["cases"] = [{"name": case, "ok": mode != "blocked", "lowering": evidence(mode),
                            **({"error": error} if mode == "blocked" else {})}
                           for mode, names in groups for case in names]
        if fault == "missing-case" and example == "stdlib_testing":
            report["cases"] = report["cases"][1:]
    print(json.dumps(report))
    raise SystemExit(0 if report["ok"] else 1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lane", required=True, choices=("basic", "stdlib"))
    lane = parser.parse_args().lane
    rows = json.loads(TABLE.read_text())
    by_example = {row["example"]: row for row in rows}
    assert len(by_example) == len(rows), "expectation table repeats an example"
    for example, mode in (("capabilities", "blocked"), ("stdlib_env", "direct-native")):
        assert by_example[example]["build"] == mode and by_example[example]["test"] == mode, example
    makefile = (ROOT / "Makefile").read_text()
    recipe = re.search(r"^stage1-smoke:[^\n]*\n((?:[\t ].*\n|\n)*)", makefile, re.M).group(1)
    assert "$(MAKE) stage1-basic-smoke" in recipe and "$(MAKE) stage1-stdlib-smoke" in recipe
    assert "examples/capabilities" not in recipe, "aggregate bypasses shared capabilities contract"
    fast_checks = (ROOT / "scripts/ci/run-fast-checks.sh").read_text()
    linker_gate = fast_checks.index('if [[ -z "$rust_linker" ]]')
    for smoke_lane in ("basic", "stdlib"):
        assert fast_checks.index(f"bash scripts/ci/run-stage1-{smoke_lane}-smoke.sh") > linker_gate
    with tempfile.TemporaryDirectory(prefix="axiom-smoke-contract-") as directory:
        temp = Path(directory)
        cargo = temp / "cargo"
        cargo.write_text(f"#!/usr/bin/env python3\nimport runpy\nrunpy.run_path({str(Path(__file__).resolve())!r}, run_name='__fake_cargo__')\n")
        cargo.chmod(0o755)
        log = temp / "calls.jsonl"
        env = {**os.environ, "PATH": str(temp) + os.pathsep + os.environ.get("PATH", ""),
               "SMOKE_TEST_LOG": str(log), "SMOKE_TEST_FAULT": ""}
        for row in rows:
            if "runtime_env" in row:
                env[row["runtime_env"]["name"]] = "inherited-smoke-test-value"

        def invoke(fault="", arguments=None):
            log.write_text("")
            result = subprocess.run(arguments or ["bash", f"scripts/ci/run-stage1-{lane}-smoke.sh",
                                                  "--report-dir", str(temp / "reports")],
                                    cwd=ROOT, env={**env, "SMOKE_TEST_FAULT": fault},
                                    text=True, capture_output=True, timeout=60)
            calls = [json.loads(line) for line in log.read_text().splitlines()]
            return result, calls

        result, calls = invoke()
        assert result.returncode == 0, result.stdout + result.stderr
        cargo_calls = [call for call in calls if call["kind"] == "cargo"]
        selected = [row for row in rows if row["lane"] == lane]
        assert {call["example"] for call in cargo_calls} == {row["example"] for row in selected}
        for row in selected:
            actual = [call for call in cargo_calls if call["example"] == row["example"]]
            commands = [call["command"] for call in actual]
            required = ["check", "build"] + (["test"] if "test" in row else [])
            for command in required:
                assert commands.count(command) == 1, (row["example"], command, commands)
            if row["build"] == "blocked":
                assert "run" not in commands, row["example"]
            elif "runtime_env" not in row:
                assert commands.count("run") == 1, row["example"]
            for call in actual:
                args = call["args"]
                assert args[:6] == ["run", "--manifest-path", "stage1/Cargo.toml", "-p", "axiomc", "--"]
                if call["command"] != "check":
                    assert args[args.index("--backend") + 1] == "cranelift", args
                if row.get("package") and call["command"] in ("build", "run"):
                    assert args[args.index("--package") + 1] == row["package"], args
                if call["command"] == "test":
                    assert all(arg in args for arg in row.get("test_args", [])), args
                    if "runtime_env" in row:
                        assert call["env"] is None, "environment test must exercise missing variable"
            if "runtime_env" in row:
                executions = [call for call in calls if call["kind"] == "binary"]
                assert [call["env"] for call in executions] == [None, *row["runtime_env"]["values"]]
                assert len({call["path"] for call in executions}) == 1, "must reuse the built binary"
                build_index = next(i for i, call in enumerate(calls) if call.get("example") == row["example"] and call.get("command") == "build")
                run_index = next(i for i, call in enumerate(calls) if call.get("example") == row["example"] and call.get("command") == "run")
                binary_indices = [i for i, call in enumerate(calls) if call["kind"] == "binary"]
                assert build_index < min(binary_indices) <= max(binary_indices) < run_index, "prove the build artifact before run can replace it"

        faults = (["success-build", "success-test", "diagnostic-build", "diagnostic-test"]
                  if lane == "basic" else ["block-env-build", "block-env-test", "stale-env", "missing-case"])
        for fault in faults:
            result, _ = invoke(fault)
            assert result.returncode != 0, f"smoke accepted injected {fault}"
            assert result.stdout.strip() or result.stderr.strip(), f"silent failure: {fault}"
        runner = [sys.executable, "scripts/ci/run-stage1-smoke.py", "--report-dir", str(temp / "reports")]
        result, calls = invoke(arguments=runner + ["--help"])
        assert result.returncode == 0 and "--lane" in result.stdout and not calls
        result, calls = invoke(arguments=runner + ["--lane", "unknown"])
        assert result.returncode != 0 and "lane" in result.stderr and not calls
        result, calls = invoke(arguments=runner)
        assert result.returncode != 0 and "lane" in result.stderr and not calls, result.stderr
    print(f"stage1 {lane} smoke behavioral contract passed ({1 + len(faults)} scenarios)")


if __name__ == "__fake_cargo__":
    fake_cargo()
elif __name__ == "__main__":
    main()
