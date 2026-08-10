#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

write_fixture() {
  local fixture="$1"
  local case_name="${2:-valid}"
  mkdir -p "$fixture/artifacts" "$fixture/provenance"

  python3 - "$fixture" "$case_name" <<'PY'
import hashlib
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
case = sys.argv[2]
source = "OMT-Global/axiomlang"
target = "x86_64-unknown-linux-gnu"
head = hashlib.sha1(b"axiomlang snapshot fixture source head").hexdigest()
chain_id = f"{source}@{target}"

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def write_json(path, payload):
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

artifacts = []
for sequence, contents in enumerate((b"axiomc genesis fixture\n", b"axiomc self-hosted fixture\n")):
    artifact = root / "artifacts" / f"axiomc-{sequence}"
    artifact.write_bytes(contents)
    artifacts.append(artifact)

entries = []
for sequence, artifact in enumerate(artifacts):
    snapshot_id = f"{chain_id}@{sequence}"
    artifact_sha = digest(artifact)
    predecessor = f"{chain_id}@{sequence - 1}" if sequence else None
    predecessor_sha = digest(artifacts[sequence - 1]) if sequence else None
    if sequence == 0:
        execution = {
            "snapshot_id": snapshot_id,
            "target": target,
            "source": source,
            "source_head_sha": head,
            "predecessor": None,
            "predecessor_artifact_sha256": None,
            "version": f"0.0.0-fixture-{sequence}",
            "command": ["cargo", "build", "--locked", "--offline"],
            "processes": ["cargo", "rustc"],
            "offline": True,
            "network_access": False,
            "generated_rust": None,
            "output": {"status": "pass", "artifact_sha256": artifact_sha, "divergent": False},
            "fixpoint": {
                "status": "not_applicable",
                "normalized_equal": None,
                "first_sha256": None,
                "second_sha256": None,
            },
        }
        built_by = "cargo"
    else:
        execution = {
            "snapshot_id": snapshot_id,
            "target": target,
            "source": source,
            "source_head_sha": head,
            "predecessor": predecessor,
            "predecessor_artifact_sha256": predecessor_sha,
            "version": f"0.0.0-fixture-{sequence}",
            "command": ["axiomc-snapshot", "build", "compiler", "--locked", "--offline"],
            "processes": ["axiomc-snapshot"],
            "offline": True,
            "network_access": False,
            "generated_rust": None,
            "output": {"status": "pass", "artifact_sha256": artifact_sha, "divergent": False},
            "fixpoint": {
                "status": "pass",
                "normalized_equal": True,
                "first_sha256": artifact_sha,
                "second_sha256": artifact_sha,
            },
        }
        built_by = "axiomc-snapshot"
    provenance = {
        "schema_version": "axiom.selfhost.snapshot_provenance.v0",
        "snapshot_id": snapshot_id,
        "chain_id": chain_id,
        "sequence": sequence,
        "version": f"0.0.0-fixture-{sequence}",
        "target": target,
        "source": source,
        "source_head_sha": head,
        "built_by": built_by,
        "predecessor": predecessor,
        "predecessor_artifact_sha256": predecessor_sha,
        "artifact_sha256": artifact_sha,
        "execution": execution,
    }
    provenance_path = root / "provenance" / f"snapshot-{sequence}.json"
    write_json(provenance_path, provenance)
    entries.append({
        "snapshot_id": snapshot_id,
        "chain_id": chain_id,
        "sequence": sequence,
        "version": f"0.0.0-fixture-{sequence}",
        "target": target,
        "sha256": artifact_sha,
        "artifact_path": str(artifact),
        "source": source,
        "source_head_sha": head,
        "built_by": built_by,
        "predecessor": predecessor,
        "provenance": str(provenance_path),
        "provenance_sha256": digest(provenance_path),
    })

if case == "forged":
    provenance_path = pathlib.Path(entries[1]["provenance"])
    payload = json.loads(provenance_path.read_text(encoding="utf-8"))
    payload["source_head_sha"] = hashlib.sha1(b"forged source head").hexdigest()
    payload["execution"]["source_head_sha"] = payload["source_head_sha"]
    write_json(provenance_path, payload)
    entries[1]["provenance_sha256"] = digest(provenance_path)
elif case == "nonexistent":
    entries[1]["artifact_path"] = str(root / "artifacts" / "does-not-exist")
elif case == "zero-digest":
    entries[1]["sha256"] = "0" * 64
    provenance_path = pathlib.Path(entries[1]["provenance"])
    payload = json.loads(provenance_path.read_text(encoding="utf-8"))
    payload["artifact_sha256"] = "0" * 64
    payload["execution"]["output"]["artifact_sha256"] = "0" * 64
    payload["execution"]["fixpoint"]["first_sha256"] = "0" * 64
    payload["execution"]["fixpoint"]["second_sha256"] = "0" * 64
    write_json(provenance_path, payload)
    entries[1]["provenance_sha256"] = digest(provenance_path)
elif case == "cargo-rustc":
    provenance_path = pathlib.Path(entries[1]["provenance"])
    payload = json.loads(provenance_path.read_text(encoding="utf-8"))
    payload["execution"]["command"] = ["cargo", "build", "--locked", "--offline"]
    payload["execution"]["processes"] = ["cargo", "rustc"]
    write_json(provenance_path, payload)
    entries[1]["provenance_sha256"] = digest(provenance_path)
elif case == "divergent-output":
    provenance_path = pathlib.Path(entries[1]["provenance"])
    payload = json.loads(provenance_path.read_text(encoding="utf-8"))
    payload["execution"]["output"]["status"] = "fail"
    payload["execution"]["output"]["divergent"] = True
    write_json(provenance_path, payload)
    entries[1]["provenance_sha256"] = digest(provenance_path)
elif case == "fixpoint":
    provenance_path = pathlib.Path(entries[1]["provenance"])
    payload = json.loads(provenance_path.read_text(encoding="utf-8"))
    payload["execution"]["fixpoint"]["status"] = "fail"
    payload["execution"]["fixpoint"]["normalized_equal"] = False
    payload["execution"]["fixpoint"]["second_sha256"] = hashlib.sha256(b"divergent rebuild").hexdigest()
    write_json(provenance_path, payload)
    entries[1]["provenance_sha256"] = digest(provenance_path)
elif case == "continuity":
    entries[1]["predecessor"] = f"{chain_id}@9"

readiness = {
    "schemaVersion": 1,
    "schema": "axiom.self_hosting.snapshot_bootstrap_readiness.v0",
    "snapshotManifest": str(root / "snapshots.json"),
    "rows": [
        {
            "id": "offline_chain",
            "requirement": "offline chain evidence is valid",
            "status": "implemented",
            "governingIssue": 1575,
            "validatingCommand": "make snapshot-bootstrap-readiness",
        }
    ],
}
write_json(root / "snapshots.json", {"schema_version": "axiom.selfhost.snapshot_manifest.v0", "snapshots": entries})
write_json(root / "readiness.json", readiness)
PY
}

run_valid_fixture() {
  local fixture="$tmpdir/valid"
  write_fixture "$fixture"
  python3 scripts/ci/check-snapshot-bootstrap-readiness.py --json \
    --manifest "$fixture/readiness.json" --snapshot-manifest "$fixture/snapshots.json" > "$fixture/result.json"
  python3 - "$fixture/result.json" <<'PY'
import json
import sys
payload = json.load(open(sys.argv[1], encoding="utf-8"))
assert payload["ready"] is True, payload
assert not any(item["status"] == "fail" for item in payload["checks"])
PY
}

run_blocked_fixture() {
  local case_name="$1"
  local expected="$2"
  local fixture="$tmpdir/$case_name"
  write_fixture "$fixture" "$case_name"
  if python3 scripts/ci/check-snapshot-bootstrap-readiness.py --json \
    --manifest "$fixture/readiness.json" --snapshot-manifest "$fixture/snapshots.json" > "$fixture/result.json"; then
    echo "expected $case_name fixture to be rejected" >&2
    exit 1
  fi
  python3 - "$fixture/result.json" "$expected" <<'PY'
import json
import sys
payload = json.load(open(sys.argv[1], encoding="utf-8"))
expected = sys.argv[2]
assert payload["ready"] is False, payload
assert any(item["name"] == expected and item["status"] == "fail" for item in payload["checks"]), payload["checks"]
PY
}

run_valid_fixture
run_blocked_fixture forged 'snapshot_OMT-Global/axiomlang@x86_64-unknown-linux-gnu@1_binding'
run_blocked_fixture nonexistent 'snapshot_OMT-Global/axiomlang@x86_64-unknown-linux-gnu@1_artifact_present'
run_blocked_fixture zero-digest 'snapshot_OMT-Global/axiomlang@x86_64-unknown-linux-gnu@1_artifact_digest'
run_blocked_fixture cargo-rustc 'snapshot_OMT-Global/axiomlang@x86_64-unknown-linux-gnu@1_no_cargo_rustc'
run_blocked_fixture divergent-output 'snapshot_OMT-Global/axiomlang@x86_64-unknown-linux-gnu@1_output'
run_blocked_fixture fixpoint 'snapshot_OMT-Global/axiomlang@x86_64-unknown-linux-gnu@1_fixpoint'
run_blocked_fixture continuity 'snapshot_predecessor_OMT-Global/axiomlang@x86_64-unknown-linux-gnu@1'

echo "check-snapshot-bootstrap-readiness realistic offline fixtures passed"
