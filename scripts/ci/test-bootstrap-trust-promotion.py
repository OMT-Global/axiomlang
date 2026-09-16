#!/usr/bin/env python3
"""Fail-closed authorization and non-vacuous wiring controls, no network."""
import copy
import importlib.util
from pathlib import Path
import subprocess
import textwrap
import unittest

ROOT=Path(__file__).resolve().parents[2]
SCRIPT=ROOT/"scripts/ci/bootstrap-trust-promotion.py"
spec=importlib.util.spec_from_file_location("promotion", SCRIPT)
m=importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
HEAD="a"*40

def fixture():
    live={"number":1686,"state":"open","head":{"sha":HEAD,"ref":m.BRANCH,"repo":{"full_name":m.REPOSITORY}},"base":{"sha":m.BASE,"ref":"main","repo":{"full_name":m.REPOSITORY}}}
    return {"number":1686,"pull_request":copy.deepcopy(live)},live,[{"state":"approved","user":{"login":"jmcte"},"environments":[{"id":m.STAGE_ID}]}]

def contract(workflow, source):
    # Parse a deterministic generated inline block, not arbitrary YAML values.
    marker="          python3 - <<'PYTHON'\n"
    block=workflow.split(marker,1)[1].split("          PYTHON\n",1)[0]
    if block != textwrap.indent(source, "          "):
        raise ValueError("inline authorization differs from tested source")
    job=workflow.split("  fast-checks:\n",1)[1].split("  full-lib-suite:\n",1)[0]
    for token in ["    environment: stage\n", "APPROVED_HEAD: ${{ vars.AXIOM_BOOTSTRAP_1686_HEAD }}", "ref: ${{ github.event.pull_request.base.sha }}\n          path: .trusted-ci", "ref: "+m.REPAIR+"\n          path: .approved-ci\n          persist-credentials: false", "if: steps.bootstrap.outputs.repair_ref != ''", '[[ "$(git -C .approved-ci rev-parse HEAD)" == "$APPROVED_REPAIR" ]]', '[[ "$APPROVED_REPAIR" == '+m.REPAIR+' ]]', 'AXIOM_CHECKOUT_PATH="$GITHUB_WORKSPACE" bash .approved-ci/scripts/ci/run-fast-checks.sh', 'AXIOM_CHECKOUT_PATH="$GITHUB_WORKSPACE" bash .trusted-ci/scripts/ci/run-fast-checks.sh']:
        if token not in job: raise ValueError("bootstrap wiring missing: "+token)

class PromotionTests(unittest.TestCase):
    def test_positive_and_ordinary_base(self):
        e,l,a=fixture(); self.assertEqual(m.select(e,l,HEAD,a),m.REPAIR)
        l["number"]=1687;e["number"]=1687;e["pull_request"]["number"]=1687
        self.assertEqual(m.select(e,l,"",[]),"")
    def test_mutation_matrix(self):
        changes=[("missing variable",lambda e,l,a:None,""),("wrong variable",lambda e,l,a:None,"b"*40),("mutable ref",lambda e,l,a:None,"main"),("no approval",lambda e,l,a:a.clear(),HEAD),("wrong reviewer",lambda e,l,a:a[0]["user"].update(login="pheidon"),HEAD),("rejected",lambda e,l,a:a[0].update(state="rejected"),HEAD),("wrong environment",lambda e,l,a:a[0]["environments"][0].update(id=0),HEAD),("stale head",lambda e,l,a:l["head"].update(sha="b"*40),HEAD),("base advance",lambda e,l,a:[x["base"].update(sha="b"*40) for x in [l,e["pull_request"]]],HEAD),("branch",lambda e,l,a:[x["head"].update(ref="attacker") for x in [l,e["pull_request"]]],HEAD),("fork",lambda e,l,a:[x["head"]["repo"].update(full_name="other/axiomlang") for x in [l,e["pull_request"]]],HEAD),("closed",lambda e,l,a:l.update(state="closed"),HEAD),("wrong pr",lambda e,l,a:l.update(number=2),HEAD)]
        for name,change,head in changes:
            with self.subTest(name=name):
                e,l,a=fixture();change(e,l,a)
                with self.assertRaises(ValueError):m.select(e,l,head,a)
    def test_inline_and_wiring_mutations(self):
        w=(ROOT/".github/workflows/pr-fast-ci.yml").read_text();s=SCRIPT.read_text();contract(w,s)
        for before,after in [("    environment: stage\n",""),("ref: "+m.REPAIR,"ref: main"),("path: .trusted-ci","path: .wrong-ci"),("persist-credentials: false","persist-credentials: true"),("APPROVED_HEAD: ${{ vars.AXIOM_BOOTSTRAP_1686_HEAD }}","APPROVED_HEAD: ${{ github.event.pull_request.head.sha }}"),("return REPAIR  # Never", "return approved_head  # Never")]:
            self.assertIn(before,w)
            with self.subTest(before=before),self.assertRaises(ValueError):contract(w.replace(before,after),s)
    def test_original_other_jobs_byte_identical(self):
        old=subprocess.check_output(["git","show",m.REPAIR+":.github/workflows/pr-fast-ci.yml"],cwd=ROOT,text=True)
        new=(ROOT/".github/workflows/pr-fast-ci.yml").read_text()
        self.assertEqual(old.split("  fast-checks:\n")[0],new.split("  fast-checks:\n")[0])
        self.assertEqual(old.split("  full-lib-suite:\n",1)[1],new.split("  full-lib-suite:\n",1)[1])
    def test_authorization_not_optimized_away(self):
        self.assertNotIn("assert ",SCRIPT.read_text())

if __name__=="__main__":unittest.main()
