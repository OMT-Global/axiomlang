#!/usr/bin/env python3
"""Trust-boundary tests using ephemeral real Ed25519 keys, no model/API calls."""
import base64
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "gate", Path(__file__).with_name("cross-model-review-gate.py"))
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)


class CrossModelGateTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.root = Path(cls.tmp.name)
        for role in ("author", "review", "unknown"):
            subprocess.run(["openssl", "genpkey", "-algorithm", "ED25519", "-out",
                            str(cls.root / (role + ".key"))], check=True,
                           capture_output=True)
            subprocess.run(["openssl", "pkey", "-in", str(cls.root / (role + ".key")),
                            "-pubout", "-out", str(cls.root / (role + ".pub"))],
                           check=True, capture_output=True)

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def setUp(self):
        self.context = {"repository": "OMT-Global/axiomlang", "pr": 100,
                        "base": "a" * 40, "head": "b" * 40, "commits": ["b" * 40]}
        self.policy = {"version": 1,
                       "routes": {"openai": "qwen", "qwen": "claude", "claude": "openai"},
                       "models": {"openai/astra-test": "openai", "openai/sol-test": "openai",
                                  "qwencloud/qwen-test": "qwen", "anthropic/claude-test": "claude"},
                       "reviewer_ready": {"qwen": True, "claude": True, "openai": True},
                       "keys": {r: {"role": r, "public_key": r + ".pub"}
                                for r in ("author", "review")}}
        self.evidence = self.root / "evidence"
        self.evidence.write_bytes(b"test review and execution evidence\n")
        self.author = {"version": 1, "kind": "author", "context": self.context,
                       "run_id": "author-run", "changes": {
                           "b" * 40: [{"provider": "openai", "model": "astra-test"}]}}
        self.review = {"version": 1, "kind": "review", "context": self.context,
                       "run_id": "review-run", "identity": {
                           "provider": "qwencloud", "model": "qwen-test"},
                       "verdict": "APPROVE", "complete": True, "blocking_findings": [],
                       "source_modified": False,
                       "evidence_sha256": hashlib.sha256(self.evidence.read_bytes()).hexdigest()}

    def sign(self, payload, role, key=None):
        message = self.root / "message"
        signature = self.root / "signature"
        message.write_bytes(gate.canonical(payload))
        subprocess.run(["openssl", "pkeyutl", "-sign", "-inkey",
                        str(self.root / ((key or role) + ".key")), "-rawin",
                        "-in", str(message), "-out", str(signature)],
                       check=True, capture_output=True)
        return {"payload": copy.deepcopy(payload), "key_id": role,
                "signature": base64.b64encode(signature.read_bytes()).decode()}

    def evaluate(self, author=None, review=None):
        return gate.evaluate(self.policy, self.root, self.context,
                             author or self.sign(self.author, "author"),
                             review or self.sign(self.review, "review"), self.evidence)

    def test_valid_cross_model_and_all_routes(self):
        identities = [{"provider": "openai", "model": "astra-test"},
                      {"provider": "qwencloud", "model": "qwen-test"},
                      {"provider": "anthropic", "model": "claude-test"}]
        for i in range(3):
            with self.subTest(i=i):
                self.author["changes"]["b" * 40] = [identities[i]]
                self.review["identity"] = identities[(i + 1) % 3]
                self.assertEqual(self.evaluate()["decision"], "PASS")

    def test_example_policy_verified_runtime_routes_still_disabled(self):
        example = gate.load(Path(__file__).resolve().parents[2] /
                            "docs/bootstrap/cross-model-review-policy.example.json")
        identities = [
            {"provider": "openai", "model": "gpt-6-astra"},
            {"provider": "bailian-token-plan", "model": "qwen3.8-max"},
            {"provider": "claude-cli", "model": "claude-sonnet-4-6"},
        ]
        families = [gate.model_family(identity, example) for identity in identities]
        self.assertEqual(families, ["openai", "qwen", "claude"])
        for index, family in enumerate(families):
            self.assertEqual(example["routes"][family], families[(index + 1) % 3])
            self.assertIs(example["reviewer_ready"][family], False)
        with self.assertRaisesRegex(gate.Rejected, "unknown model"):
            gate.model_family({"provider": "anthropic", "model": "claude-sonnet-4-6"}, example)

    def test_signed_payload_tampering(self):
        envelope = self.sign(self.review, "review")
        envelope["payload"]["verdict"] = "REQUEST_CHANGES"
        with self.assertRaisesRegex(gate.Rejected, "signature verification"):
            self.evaluate(review=envelope)

    def test_untrusted_signer(self):
        with self.assertRaisesRegex(gate.Rejected, "signature verification"):
            self.evaluate(review=self.sign(self.review, "review", key="unknown"))

    def test_wrong_signing_role(self):
        with self.assertRaisesRegex(gate.Rejected, "wrong or unknown role"):
            self.evaluate(review=self.sign(self.review, "author"))

    def test_same_key_under_different_role_names(self):
        self.policy["keys"]["review"]["public_key"] = "author.pub"
        with self.assertRaisesRegex(gate.Rejected, "share a signing key"):
            self.evaluate(review=self.sign(self.review, "review", key="author"))

    def test_same_family_different_model(self):
        self.review["identity"] = {"provider": "openai", "model": "sol-test"}
        with self.assertRaisesRegex(gate.Rejected, "author model family"):
            self.evaluate()

    def test_different_but_wrong_reviewer(self):
        self.review["identity"] = {"provider": "anthropic", "model": "claude-test"}
        with self.assertRaisesRegex(gate.Rejected, "designated reviewer"):
            self.evaluate()

    def test_unknown_or_spoofed_model(self):
        for model in ("unknown", "qwen-test", ""):
            with self.subTest(model=model):
                self.author["changes"]["b" * 40] = [{"provider": "openai", "model": model}]
                with self.assertRaises(gate.Rejected):
                    self.evaluate()

    def test_receipt_replay_against_changed_context(self):
        for key, value in (("repository", "other/repo"), ("pr", 101),
                           ("head", "c" * 40), ("base", "c" * 40),
                           ("commits", ["c" * 40, "b" * 40])):
            with self.subTest(key=key):
                payload = copy.deepcopy(self.review)
                payload["context"][key] = value
                with self.assertRaisesRegex(gate.Rejected, "does not match"):
                    self.evaluate(review=self.sign(payload, "review"))

    def test_missing_commit_or_contributor_provenance(self):
        for changes in ({}, {"c" * 40: self.author["changes"]["b" * 40]}, {"b" * 40: []}):
            with self.subTest(changes=changes):
                self.author["changes"] = changes
                with self.assertRaises(gate.Rejected):
                    self.evaluate()

    def test_mixed_authors_block_ambiguous_routing(self):
        self.author["changes"]["b" * 40].append({"provider": "anthropic", "model": "claude-test"})
        with self.assertRaisesRegex(gate.Rejected, "mixed-author routing"):
            self.evaluate()

    def test_reviewer_was_one_of_multiple_authors(self):
        self.author["changes"]["b" * 40].append(self.review["identity"])
        with self.assertRaisesRegex(gate.Rejected, "author model family"):
            self.evaluate()

    def test_rejection_incomplete_findings_edits_and_reused_run(self):
        for key, value in (("verdict", "REQUEST_CHANGES"), ("complete", False),
                           ("blocking_findings", ["bug"]), ("source_modified", True),
                           ("run_id", "author-run"), ("run_id", "")):
            with self.subTest(key=key):
                payload = dict(self.review, **{key: value})
                with self.assertRaises(gate.Rejected):
                    self.evaluate(review=self.sign(payload, "review"))

    def test_missing_review_fields(self):
        for key in ("identity", "verdict", "complete", "blocking_findings", "source_modified", "evidence_sha256"):
            with self.subTest(key=key):
                payload = dict(self.review)
                del payload[key]
                with self.assertRaises(gate.Rejected):
                    self.evaluate(review=self.sign(payload, "review"))

    def test_unavailable_reviewer_and_no_fallback(self):
        self.policy["reviewer_ready"]["qwen"] = False
        with self.assertRaisesRegex(gate.Rejected, "access/usage not verified"):
            self.evaluate()

    def test_evidence_tampering(self):
        self.evidence.write_bytes(b"different evidence")
        with self.assertRaisesRegex(gate.Rejected, "bundle digest mismatch"):
            self.evaluate()

    def test_duplicate_json_keys_and_nonfinite_values(self):
        path = self.root / "invalid.json"
        for value in ('{"verdict":"APPROVE","verdict":"REJECT"}', '{"v":NaN}'):
            path.write_text(value)
            with self.assertRaises(gate.Rejected):
                gate.load(path)

    def test_cli_success_and_failure_exit_codes(self):
        args = ["python3", str(Path(gate.__file__))]
        for name, value in (("policy", self.policy), ("context", self.context),
                            ("author", self.sign(self.author, "author")),
                            ("review", self.sign(self.review, "review"))):
            path = self.root / (name + ".json")
            path.write_bytes(gate.canonical(value))
            args.extend(["--" + name, str(path)])
        args.extend(["--evidence", str(self.evidence)])
        good = subprocess.run(args, capture_output=True, text=True)
        self.assertEqual(good.returncode, 0, good.stdout + good.stderr)
        self.assertEqual(json.loads(good.stdout)["decision"], "PASS")
        (self.root / "review.json").write_text("{}")
        bad = subprocess.run(args, capture_output=True, text=True)
        self.assertEqual(bad.returncode, 1)
        self.assertEqual(json.loads(bad.stdout)["decision"], "BLOCK")


if __name__ == "__main__":
    unittest.main()
