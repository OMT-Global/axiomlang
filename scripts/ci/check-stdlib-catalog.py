#!/usr/bin/env python3
import argparse, hashlib, json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
LEDGER=ROOT/"stage1/compiler-contracts/snapshots/capability-ledger.json"
SNAPSHOT=ROOT/"stage1/compiler-contracts/snapshots/stdlib-catalog.json"
SCHEMA=ROOT/"stage1/compiler-contracts/schemas/axiom.compiler.stdlib_catalog.v1.schema.json"
ROLLBACK="stage1/crates/axiomc/src/stdlib.rs remains the bootstrap loader until the qualified compiler path consumes this catalog."
def build(ledger):
 modules=[]
 for row in sorted(ledger["stdlib"],key=lambda x:x["module"]):
  caps=sorted(row["capabilities"]); effect="pure" if not caps else "capability:"+",".join(caps)
  modules.append({"name":row["module"],"capabilities":caps,"source_identity":"embedded+source:stage1/crates/axiomc/src/stdlib.rs","symbols":[{"name":s,"effect":effect,"binding":f"axiom://provider/stage1-v1/{row['module'].removeprefix('std/').removesuffix('.ax')}/{s}","binding_kind":"provider_contract"} for s in sorted(row["functions"])]})
 material={"catalog_version":"1.0.0","modules":modules}
 return {"schema_version":"axiom.compiler.stdlib_catalog.v1","contract":"compiler.stdlib","catalog_version":"1.0.0","source":"stage1/compiler-contracts/snapshots/capability-ledger.json","modules":modules,"release_digest":hashlib.sha256(json.dumps(material,sort_keys=True,separators=(",",":")).encode()).hexdigest(),"rollback_boundary":ROLLBACK}
p=argparse.ArgumentParser();p.add_argument("--write",action="store_true");p.add_argument("--json",action="store_true");a=p.parse_args()
ledger=json.loads(LEDGER.read_text()); expected=build(ledger)
if a.write: SNAPSHOT.write_text(json.dumps(expected,indent=2)+"\n")
catalog=json.loads(SNAPSHOT.read_text()); schema=json.loads(SCHEMA.read_text())
assert catalog==expected,"stdlib catalog drift; regenerate with --write"
assert schema["title"]=="AxiOM compiler standard-library catalog" and set(catalog)==set(schema["properties"])
for m in catalog["modules"]:
 assert m["source_identity"].startswith("embedded+source:")
 for s in m["symbols"]: assert s["binding_kind"]=="provider_contract" and s["binding"].startswith("axiom://provider/") and "rust" not in s["binding"].lower()
out={"ok":True,"modules":len(catalog["modules"]),"symbols":sum(len(m["symbols"]) for m in catalog["modules"]),"release_digest":catalog["release_digest"]}
print(json.dumps(out,sort_keys=True) if a.json else out)
