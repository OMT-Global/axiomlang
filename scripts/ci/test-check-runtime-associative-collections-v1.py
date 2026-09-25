#!/usr/bin/env python3
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-runtime-associative-collections-v1.py"


def run(root: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(root / "scripts/ci/check-runtime-associative-collections-v1.py")],
        cwd=root,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def main() -> None:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory) / "repo"
        shutil.copytree(ROOT / "stage1", root / "stage1")
        (root / "scripts/ci").mkdir(parents=True)
        shutil.copy2(CHECKER, root / "scripts/ci/check-runtime-associative-collections-v1.py")
        if run(root).returncode != 0:
            raise SystemExit("valid associative collections contract was rejected")

        schema_path = root / "stage1/compiler-contracts/schemas/axiom.runtime_associative_collections.v1.schema.json"
        original_schema = schema_path.read_bytes()
        schema = json.loads(original_schema)
        del schema["properties"]["keys"]["properties"]["equality"]["properties"]["tuple"]["const"]
        schema_path.write_text(json.dumps(schema))
        if run(root).returncode == 0:
            raise SystemExit("weakened nested published-schema constraint was accepted")
        schema_path.write_bytes(original_schema)

        path = root / "stage1/compiler-contracts/snapshots/runtime-associative-collections-v1.json"
        original_snapshot = path.read_bytes()
        # Test both snapshot drift and coordinated schema/snapshot weakening:
        # the checker must anchor the required inspection/model contract itself.
        for case in ("inspection-field", "inspection-fixture", "model-comparison-fixture"):
            for coupled in (False, True):
                value = json.loads(original_snapshot)
                schema = json.loads(original_schema)
                if case == "inspection-field":
                    value["inspection_fields"].remove("resource_authority")
                    field = "inspection_fields"
                else:
                    kind = "inspection" if case == "inspection-fixture" else "model-comparison"
                    value["fixtures"] = [row for row in value["fixtures"] if row["kind"] != kind]
                    field = "fixtures"
                if coupled:
                    schema["properties"][field].update(
                        const=value[field], minItems=len(value[field]), maxItems=len(value[field])
                    )
                schema_path.write_text(json.dumps(schema))
                path.write_text(json.dumps(value))
                result = run(root)
                if result.returncode == 0:
                    raise SystemExit(f"{case} drift accepted (coupled={coupled})")
                if field not in result.stderr:
                    raise SystemExit(f"{case} rejected for an unrelated reason: {result.stderr}")
        schema_path.write_bytes(original_schema)
        path.write_bytes(original_snapshot)
        if run(root).returncode != 0:
            raise SystemExit("restored inspection/model contract fixture failed")


        value = json.loads(path.read_text())
        value["keys"]["accepted"] = ["primitive"]
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("incomplete key-shape coverage was accepted")

        value = json.loads((ROOT / "stage1/compiler-contracts/snapshots/runtime-associative-collections-v1.json").read_text())
        value["resources"]["collision"] = "unbounded"
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("unbounded collision handling was accepted")

        value = json.loads((ROOT / "stage1/compiler-contracts/snapshots/runtime-associative-collections-v1.json").read_text())
        value["fixtures"][9]["kind"] = "positive"
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("missing adversarial fixture classification was accepted")

        value = json.loads((ROOT / "stage1/compiler-contracts/snapshots/runtime-associative-collections-v1.json").read_text())
        del value["keys"]["equality"]["tuple"]
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("missing nested schema constraint was accepted")

        value = json.loads((ROOT / "stage1/compiler-contracts/snapshots/runtime-associative-collections-v1.json").read_text())
        value["hashing"]["host_seed"] = "Rust HashMap detail"
        path.write_text(json.dumps(value))
        if run(root).returncode == 0:
            raise SystemExit("host-specific associative collection terms were accepted")
    print("Runtime Associative Collections v1 checker tests passed")


if __name__ == "__main__":
    main()
