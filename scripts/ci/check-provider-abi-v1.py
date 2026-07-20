#!/usr/bin/env python3
"""Validate the Provider ABI v1 security contract."""
import json, shutil, subprocess, sys, tempfile
from pathlib import Path
R=Path(__file__).resolve().parents[2]; S=R/"stage1/compiler-contracts/schemas/axiom.provider-abi.v1.schema.json"; V=R/"stage1/compiler-contracts/snapshots/provider-abi-v1.json"; C=R/"stage1/compiler-contracts/fixtures/provider-abi-v1/reference-provider.c"
def need(x,m):
 if not x: print(m,file=sys.stderr); raise SystemExit(1)
s=json.loads(S.read_text()); v=json.loads(V.read_text())
need(s["type"]=="object" and s["additionalProperties"] is False,"schema envelope drift")
need(set(s["required"])==set(v),"schema required surface drift")
need((v["schema_version"],v["contract"],v["issue"])==("axiom.provider-abi.v1","runtime.provider_abi",1453),"identity drift")
need(v["negotiation"]["entrypoint"]=="axiom_provider_v1" and v["negotiation"]["incompatible"]=="fail_closed_before_call","negotiation drift")
need(v["safe_surface"]["forbidden"]==["raw_pointer","address","allocator_callback","retained_callback","unbounded_length"],"raw-pointer escape accepted")
need(v["handles"]["representation"]=="nonzero_provider_scoped_generation_tagged_u64" and v["handles"]["invalid"]=="fail_closed_without_dispatch","handle drift")
need(v["buffers"]["retention"]=="forbidden_after_boundary" and v["buffers"]["safe_return"]=="copy_then_release_or_release_on_failure","buffer lifetime drift")
need(v["operations"]["capability"]=="declared_and_checked_before_dispatch" and v["operations"]["fault"]=="quarantine_provider_invalidate_handles","operation safety drift")
need(v["loading"]["search_paths"]=={"host_default":"denied","relative":"denied","ambient_lookup":"denied","unsigned":"denied"},"library search policy drift")
need(v["audit"]["forbidden"]==["buffer_contents","text_contents","paths","credentials","addresses","raw_handles"],"audit leak")
ids={x["id"] for x in v["fixtures"]}; need(len(ids)==10 and "c-reference-descriptor-provider" in ids and "owned-buffer-release-leak" in ids,"fixture drift")
cc=shutil.which("cc"); need(cc,"C compiler unavailable for reference fixture")
with tempfile.TemporaryDirectory() as d:
 p=subprocess.run([cc,"-std=c11","-Wall","-Wextra","-Werror","-c",str(C),"-o",str(Path(d)/"provider.o")],capture_output=True,text=True); need(p.returncode==0,"C reference fixture failed: "+p.stderr)
print(json.dumps({"schema":v["schema_version"],"ok":True,"fixtures":len(ids),"c_fixture":"compiled"}))
