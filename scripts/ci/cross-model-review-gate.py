#!/usr/bin/env python3
"""Evaluate signed, commit-bound cross-model receipts. Never calls a model or GitHub.

Policy, public keys and live context MUST come from a trusted publisher, not a PR.
See docs/bootstrap/cross-model-review.md for the still-required rollout boundary.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile


class Rejected(ValueError):
    pass


def require(condition, reason):
    if not condition:
        raise Rejected(reason)


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=True, allow_nan=False).encode()


def load(path):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result
    def constant(_):
        raise Rejected("non-finite JSON number")
    data = Path(path).read_bytes()
    require(len(data) <= 1024 * 1024, "input exceeds 1 MiB")
    return json.loads(data, object_pairs_hook=pairs, parse_constant=constant)


def public_key_bytes(path):
    result = subprocess.run(
        ["openssl", "pkey", "-pubin", "-in", str(path), "-outform", "DER"],
        capture_output=True, timeout=10, check=False)
    require(result.returncode == 0, "invalid trusted public key")
    require(len(result.stdout) == 44 and
            result.stdout.startswith(bytes.fromhex("302a300506032b6570032100")),
            "trusted key must be Ed25519")
    return result.stdout


def verify(envelope, role, policy, policy_dir):
    require(isinstance(envelope, dict) and set(envelope) ==
            {"payload", "signature", "key_id"}, "invalid receipt envelope")
    key_id = envelope["key_id"]
    require(isinstance(key_id, str), "invalid key identifier")
    key = policy["keys"].get(key_id)
    require(isinstance(key, dict) and key.get("role") == role,
            "receipt signed by wrong or unknown role")
    key_path = (policy_dir / key["public_key"]).resolve()
    require(key_path.is_file(), "trusted public key unavailable")
    signature = base64.b64decode(envelope["signature"], validate=True)
    # Fixed Ed25519 signature length; never accept a different signature scheme.
    require(len(signature) == 64, "invalid Ed25519 signature length")
    payload = envelope["payload"]
    require(isinstance(payload, dict), "invalid receipt payload")
    with tempfile.TemporaryDirectory(prefix="cross-model-verify-") as tmp:
        message = Path(tmp) / "message"
        sig = Path(tmp) / "signature"
        message.write_bytes(canonical(payload))
        sig.write_bytes(signature)
        result = subprocess.run(
            ["openssl", "pkeyutl", "-verify", "-pubin", "-inkey", str(key_path),
             "-rawin", "-in", str(message), "-sigfile", str(sig)],
            capture_output=True, timeout=10, check=False)
        require(result.returncode == 0, "receipt signature verification failed")
    require(type(payload.get("version")) is int and payload["version"] == 1 and
            payload.get("kind") == role,
            "unsupported receipt version or kind")
    return payload, public_key_bytes(key_path)


def model_family(identity, policy):
    require(isinstance(identity, dict) and set(identity) == {"provider", "model"},
            "model identity must contain exact provider and model IDs")
    require(all(isinstance(v, str) and v for v in identity.values()),
            "empty model identity")
    name = identity["provider"] + "/" + identity["model"]
    family = policy["models"].get(name)
    require(isinstance(family, str) and family in policy["routes"],
            "unknown model identity: " + name)
    return family


def validate_context(context):
    require(isinstance(context, dict) and set(context) ==
            {"repository", "pr", "base", "head", "commits"}, "invalid live context")
    require(isinstance(context["repository"], str) and
            re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", context["repository"]),
            "invalid repository")
    require(type(context["pr"]) is int and context["pr"] > 0, "invalid PR number")
    commits = context["commits"]
    require(isinstance(commits, list) and commits, "missing live commit list")
    for sha in [context["base"], context["head"], *commits]:
        require(isinstance(sha, str) and re.fullmatch(r"[0-9a-f]{40}", sha),
                "invalid commit SHA")
    require(len(set(commits)) == len(commits) and context["head"] in commits,
            "duplicate commits or head missing from commit list")


def evaluate(policy, policy_dir, context, author_envelope, review_envelope, evidence_path):
    require(isinstance(policy, dict) and type(policy.get("version")) is int and
            policy["version"] == 1, "unsupported policy version")
    for field in ("keys", "routes", "models", "reviewer_ready"):
        require(isinstance(policy.get(field), dict), "invalid policy " + field)
    require(all(isinstance(k, str) and isinstance(v, str)
                for k, v in policy["routes"].items()), "invalid policy routes")
    validate_context(context)
    author, author_key = verify(author_envelope, "author", policy, policy_dir)
    review, review_key = verify(review_envelope, "review", policy, policy_dir)
    require(author_key != review_key, "author and reviewer share a signing key")
    for receipt in (author, review):
        require(receipt.get("context") == context, "receipt does not match live PR")
        require(isinstance(receipt.get("run_id"), str) and receipt["run_id"],
                "missing trusted execution run ID")
    require(author["run_id"] != review["run_id"], "review reused author execution")
    changes = author.get("changes")
    require(isinstance(changes, dict) and set(changes) == set(context["commits"]),
            "author provenance must cover every live PR commit")
    families = set()
    for identities in changes.values():
        require(isinstance(identities, list) and identities,
                "missing author model provenance")
        families.update(model_family(identity, policy) for identity in identities)
    reviewer = model_family(review.get("identity"), policy)
    require(reviewer not in families, "author model family cannot approve")
    targets = {policy["routes"][family] for family in families}
    require(len(targets) == 1, "mixed-author routing needs explicit policy")
    require(reviewer in targets, "designated reviewer model family required")
    require(review.get("verdict") == "APPROVE", "review did not approve")
    require(review.get("complete") is True, "review incomplete")
    require(review.get("blocking_findings") == [], "unresolved review findings")
    require(review.get("source_modified") is False, "reviewer changed source")
    evidence = review.get("evidence_sha256")
    require(isinstance(evidence, str) and re.fullmatch(r"[0-9a-f]{64}", evidence),
            "missing evidence bundle digest")
    digest = hashlib.sha256()
    with Path(evidence_path).open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    require(digest.hexdigest() == evidence, "evidence bundle digest mismatch")
    require(policy.get("reviewer_ready", {}).get(reviewer) is True,
            "designated reviewer access/usage not verified")
    return {"decision": "PASS", "context": context,
            "author_families": sorted(families), "reviewer_family": reviewer,
            "review_model": review["identity"], "review_run_id": review["run_id"],
            "evidence_sha256": evidence,
            "author_receipt_sha256": hashlib.sha256(canonical(author)).hexdigest(),
            "review_receipt_sha256": hashlib.sha256(canonical(review)).hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("policy", "context", "author", "review", "evidence"):
        parser.add_argument("--" + name, required=True, type=Path)
    args = parser.parse_args()
    try:
        result = evaluate(load(args.policy), args.policy.resolve().parent,
                          load(args.context), load(args.author), load(args.review),
                          args.evidence)
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError) as error:
        print(json.dumps({"decision": "BLOCK", "reason": str(error)}))
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
