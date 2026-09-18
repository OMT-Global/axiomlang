#!/usr/bin/env python3
"""Fail-closed authorization and non-vacuous wiring controls, no network."""
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import textwrap
import tempfile
import unittest
import urllib.error
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/ci/provider-abi-trust-promotion.py"
spec = importlib.util.spec_from_file_location("promotion", SCRIPT)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
# The authorized maintenance head is deliberately distinct from the executable
# repair source; the guard must never select this event head for execution.
HEAD = "f" * 40
EXPRESSION = "$" + "{{"


def fixture():
    live = {"number": m.PR_NUMBER, "state": "open",
            "head": {"sha": HEAD, "ref": m.BRANCH, "repo": {"full_name": m.REPOSITORY}},
            "base": {"sha": m.BASE, "ref": "main", "repo": {"full_name": m.REPOSITORY}}}
    return {"number": m.PR_NUMBER, "pull_request": copy.deepcopy(live)}, live, [
        {"state": "approved", "user": {"login": "jmcte"}, "environments": [{"id": m.STAGE_ID}]}
    ]


def contract(workflow, source):
    marker = "          python3 - <<'PYTHON'\n"
    block = workflow.split(marker, 1)[1].split("          PYTHON\n", 1)[0]
    if block != textwrap.indent(source, "          "):
        raise ValueError("inline authorization differs from tested source")
    if m.RETAINED_SOURCE_REF != "ci-source/axiom-copied-checkers-67b9b136":
        raise ValueError("unexpected retained source reference")
    job = workflow.split("  fast-checks:\n", 1)[1].split("  full-lib-suite:\n", 1)[0]
    for token in [
        "    environment: stage\n",
        "APPROVED_HEAD: " + EXPRESSION + " vars.AXIOM_PROVIDER_ABI_1698_HEAD }}",
        "ref: " + EXPRESSION + " github.event.pull_request.base.sha }}\n          path: .trusted-ci",
        "ref: " + m.REPAIR + "\n          path: .approved-ci\n          persist-credentials: false",
        "if: steps.provider_abi.outputs.repair_ref != ''",
        '[[ "$APPROVED_REPAIR" == ' + m.REPAIR + ' ]]',
        '[[ "$(git -C .approved-ci rev-parse HEAD)" == "$APPROVED_REPAIR" ]]',
        'AXIOM_CHECKOUT_PATH="$GITHUB_WORKSPACE" bash .approved-ci/scripts/ci/run-fast-checks.sh',
        'AXIOM_CHECKOUT_PATH="$GITHUB_WORKSPACE" bash .trusted-ci/scripts/ci/run-fast-checks.sh',
    ]:
        if token not in job:
            raise ValueError("provider ABI maintenance wiring missing: " + token)


class PromotionTests(unittest.TestCase):
    def test_positive_and_ordinary_base(self):
        event, live, approvals = fixture()
        self.assertEqual(m.select(event, live, HEAD, approvals), m.REPAIR)
        live["number"] = 1699
        event["number"] = event["pull_request"]["number"] = 1699
        self.assertEqual(m.select(event, live, "", []), "")

    def test_mutation_matrix(self):
        changes = [
            ("missing variable", lambda e, l, a: None, ""),
            ("wrong variable", lambda e, l, a: None, "b" * 40),
            ("mutable ref", lambda e, l, a: None, "main"),
            ("no approval", lambda e, l, a: a.clear(), HEAD),
            ("wrong reviewer", lambda e, l, a: a[0]["user"].update(login="pheidon"), HEAD),
            ("rejected", lambda e, l, a: a[0].update(state="rejected"), HEAD),
            ("wrong environment", lambda e, l, a: a[0]["environments"][0].update(id=0), HEAD),
            ("stale head", lambda e, l, a: l["head"].update(sha="b" * 40), HEAD),
            ("base advance", lambda e, l, a: [x["base"].update(sha="b" * 40) for x in [l, e["pull_request"]]], HEAD),
            ("branch", lambda e, l, a: [x["head"].update(ref="attacker") for x in [l, e["pull_request"]]], HEAD),
            ("fork", lambda e, l, a: [x["head"]["repo"].update(full_name="other/axiomlang") for x in [l, e["pull_request"]]], HEAD),
            ("closed", lambda e, l, a: l.update(state="closed"), HEAD),
            ("wrong PR", lambda e, l, a: l.update(number=2), HEAD),
        ]
        for name, change, approved_head in changes:
            with self.subTest(name=name):
                event, live, approvals = fixture()
                change(event, live, approvals)
                with self.assertRaises(ValueError):
                    m.select(event, live, approved_head, approvals)

    def test_inline_and_wiring_mutations(self):
        workflow = (ROOT / ".github/workflows/pr-fast-ci.yml").read_text()
        source = SCRIPT.read_text()
        contract(workflow, source)
        approved = "APPROVED_HEAD: " + EXPRESSION + " vars.AXIOM_PROVIDER_ABI_1698_HEAD }}"
        untrusted = "APPROVED_HEAD: " + EXPRESSION + " github.event.pull_request.head.sha }}"
        for before, after in [
            ("    environment: stage\n", ""),
            ("ref: " + m.REPAIR, "ref: main"),
            ("path: .trusted-ci", "path: .wrong-ci"),
            ("persist-credentials: false", "persist-credentials: true"),
            (approved, untrusted),
            ("return REPAIR", "return approved_head"),
        ]:
            self.assertIn(before, workflow)
            with self.subTest(before=before), self.assertRaises(ValueError):
                contract(workflow.replace(before, after), source)

    def test_authorization_not_optimized_away(self):
        self.assertNotIn("assert ", SCRIPT.read_text())

    def test_api_error_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            event = Path(directory) / "event.json"
            event.write_text(json.dumps({"number": m.PR_NUMBER}))
            output = Path(directory) / "output"
            with mock.patch.dict(m.os.environ, {
                "GITHUB_REPOSITORY": m.REPOSITORY,
                "GITHUB_EVENT_NAME": "pull_request",
                "GITHUB_EVENT_PATH": str(event),
                "GITHUB_RUN_ID": "1",
                "GITHUB_OUTPUT": str(output),
                "GH_TOKEN": "test-token",
            }, clear=False), mock.patch.object(
                m.urllib.request, "urlopen", side_effect=urllib.error.URLError("offline")
            ):
                with self.assertRaises(urllib.error.URLError):
                    m.main()

    def test_other_jobs_are_base_identical(self):
        workflow = (ROOT / ".github/workflows/pr-fast-ci.yml").read_text()
        self.assertEqual(
            hashlib.sha256(workflow.split("  fast-checks:\n")[0].encode()).hexdigest(),
            "814f6a5adcf12b77ef9d191899af9b085f435822a724a332a404726628e51330",
        )
        self.assertEqual(
            hashlib.sha256(workflow.split("  full-lib-suite:\n", 1)[1].encode()).hexdigest(),
            "a93bdb0a7e0a26902689f799910f50c87c02b596f206b785ee43d0d478c04269",
        )


if __name__ == "__main__":
    unittest.main()
