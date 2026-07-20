#!/usr/bin/env python3
import json, shutil, subprocess, sys, tempfile
from pathlib import Path
R=Path(__file__).resolve().parents[2]
def run(root): return subprocess.run([sys.executable,str(root/"scripts/ci/check-provider-abi-v1.py")],cwd=root,capture_output=True).returncode
with tempfile.TemporaryDirectory() as d:
 repo=Path(d)/"repo"; shutil.copytree(R/"stage1",repo/"stage1"); (repo/"scripts/ci").mkdir(parents=True); shutil.copy2(R/"scripts/ci/check-provider-abi-v1.py",repo/"scripts/ci/check-provider-abi-v1.py")
 if run(repo): raise SystemExit("valid contract rejected")
 p=repo/"stage1/compiler-contracts/snapshots/provider-abi-v1.json"; x=json.loads(p.read_text()); x["safe_surface"]["forbidden"].remove("raw_pointer"); p.write_text(json.dumps(x))
 if not run(repo): raise SystemExit("raw pointer escape accepted")
 x=json.loads((R/"stage1/compiler-contracts/snapshots/provider-abi-v1.json").read_text()); x["loading"]["search_paths"]["ambient_lookup"]="allowed"; p.write_text(json.dumps(x))
 if not run(repo): raise SystemExit("ambient search path accepted")
print("Provider ABI v1 checker tests passed")
