# Cross-model review gate

Tracking issue: #1678. **Staged evaluator, not enabled merge enforcement.**

JT requires a model family different from every implementation contributor to
review and approve the code. Opening a PR through another account does not change
model authorship. Initial routes are OpenAI/Astra → Qwen, Qwen → Claude, and
Claude → OpenAI/Astra. Missing designated reviewers block; do not substitute one
silently. Multiple author families whose routes disagree need an explicit policy
decision, not an automatic fallback. Human/unknown authorship also needs explicit
policy/provenance; this version does not invent a model for it.

## Reuse and scope

Use the existing Codex, Claude Code, and OpenCode clients for supported model
execution. Existing reviewer projects such as
[J-Bot](https://github.com/pgup-ai/jbot-review-action) provide model selection and
PR review, but do not by themselves establish authenticated, complete author
provenance or enforce this project's author-to-reviewer routing. The addition
here is only a local policy/evidence evaluator; it is not a replacement model
client, credential broker, scheduler, or a deployed GitHub App.

## Trust boundary

`scripts/ci/cross-model-review-gate.py` verifies two Ed25519-signed receipts using
role-specific trusted public keys. The author and review signing keys must differ.
Both receipts bind the repository, PR number, current base SHA, current head SHA,
and complete ordered PR commit list. Author evidence covers every listed commit
and every contributing model for that commit. The reviewer must be outside all
author families and match the designated route. Only a complete APPROVE without
blocking findings or source changes passes. The saved evidence bundle's SHA-256
must match the signed digest. No model calls or GitHub writes are performed.

**Signatures authenticate broker assertions, not model truth by themselves.**
Trusted execution adapters must record actual provider/model IDs and run IDs from
execution, aggregate all contributing runs, and refuse unknown or contradictory
identity. Do not generate a valid-looking receipt from a model's self-description,
a PR label, a supplied JSON verdict, or user-authored PR text. A general-purpose
"sign arbitrary JSON" endpoint would defeat this design and is not supplied.

Policy, public keys, live context, evaluator code, and publishing credentials must
come from a trusted deployment outside the PR's writable tree. The example policy
in this repository is NOT a production trust root. Never execute this checker from
an untrusted PR while exposing signing or publishing credentials. An attacker able
to replace the evaluator or its policy can approve anything; this CLI alone does
not prevent a privileged orchestrator from doing so. Issuer key custody, execution
adapters, and the restricted GitHub publisher remain required integration work.

## Receipt contract

An envelope has exactly `payload`, `signature` (strict base64), and `key_id`.
Sign the UTF-8 canonical JSON bytes produced by the evaluator's `canonical()`:
sorted keys, compact separators, ASCII escapes, and no non-finite numbers.
Duplicate JSON keys are rejected on load. Keys must be Ed25519 public PEM files.

Every payload contains `version: 1`, `kind` (`author` or `review`), a nonempty
trusted `run_id`, and `context`:

```json
{
  "repository": "OMT-Global/axiomlang",
  "pr": 123,
  "base": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "head": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
  "commits": ["bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"]
}
```

The author payload adds `changes`, a map of **every** context commit SHA to a
nonempty list of `{ "provider": "...", "model": "..." }` identities. Multiple
contributing models must all be listed, including later fix-up authors. Trusted
adapters must aggregate this provenance; completeness cannot be discovered from
ordinary Git commits alone. Unknown historical authorship blocks.

The review payload adds:

```json
{
  "identity": {"provider": "qwencloud", "model": "qwen3.8-max"},
  "verdict": "APPROVE",
  "complete": true,
  "blocking_findings": [],
  "source_modified": false,
  "evidence_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
}
```

If a reviewer edits source it becomes an author; discard the review and collect a
new review from an eligible model. Preserve findings across retries. Rate limits,
auth failures, unknown model versions, and unsupported subscription usage block
execution; do not fall back to a paid API or another model automatically.

## Running and testing

Requires Python 3 and OpenSSL with Ed25519 `pkeyutl -rawin` support (OpenSSL 3 is
tested). The normal Linux shell-safe runner provides OpenSSL; no pip dependencies
are added. The Mac's system LibreSSL is not claimed to be supported.

```sh
python3 scripts/ci/test-cross-model-review-gate.py -v
python3 /trusted/cross-model-review-gate.py \
  --policy /trusted/policy.json \
  --context /trusted/live-pr-context.json \
  --author /trusted/author-receipt.json \
  --review /trusted/review-receipt.json \
  --evidence /trusted/review-evidence.tar
```

The CLI outputs JSON, exits 0 for PASS and nonzero for BLOCK/errors. The unit tests
generate temporary signing keys and never contact a model or GitHub. They are
wired into Fast Checks as evaluator regression tests, **not** a passing live
cross-model review. No production signing keys are generated by this change.

## Activation requirements (not yet fulfilled)

1. Prove supported authenticated author/reviewer clients and resolve exact IDs.
   The example lists the verified OpenClaw routes for Astra, Qwen 3.8, and
   Claude Sonnet 4.6. All `reviewer_ready` values intentionally start false: live
   access alone does not establish a trusted deployment or permitted unattended use.
2. Deploy isolated trusted execution receipt issuers. Keep keys out of worker
   sandboxes. Maintain complete per-commit author provenance and review history.
3. Deploy a restricted publisher with GitHub check-write/review authority. Capture
   live repository/PR/base/head/all paginated commits from GitHub, not PR input.
   Fetch them again immediately before publication and publish only for that head;
   a changed context must rerun review. Retain receipts and evidence immutably.
4. The publisher may report `cross-model-review` success **only** for a verified
   PASS and an actually recorded authorized review. Never emit neutral or skipped
   results for missing provenance, routing/auth failures, or absent review.
5. Prove a real PR end to end and a changed-head rejection. Then add the required
   check with its expected GitHub App as source, preserving existing CI Gate,
   code-owner review, stale-review dismissal, and admin enforcement. Register a
   trusted base/synchronize event handler so base/head changes invalidate the
   result. Do not rely solely on the CLI's one-time context snapshot.
6. Publish transparent review identity/model/run/evidence details. Auto-merge only
   after all existing gates and the cross-model gate pass; verify MERGED afterward.

Until this is proven, leave branch protection unchanged. This source change does
not deploy a watcher, activate a required status, or authorize a paid API.

## Subscription/access findings (2026-09-13)

- Direct-host ChatGPT/Astra and Qwen 3.8 Max both returned live responses through
  OpenClaw. Their runtime IDs are `openai/gpt-6-astra` and
  `bailian-token-plan/qwen3.8-max`. Use actual execution metadata, not the display
  alias or a model's self-identification, when issuing receipts.
- Direct-host Claude Code is authenticated to Claude Pro. The native OpenClaw
  route `claude-cli/claude-sonnet-4-6` returned a live response without API-key
  fallback. The generic Anthropic API route did not use this subscription login;
  it is not an interchangeable authenticated route.
- QwenCloud explicitly documents [OpenClaw integration](https://docs.qwencloud.com/developer-guides/clients-and-developer-tools/openclaw).
  User-initiated interactive agent review is the configured operating mode. The
  [Personal FAQ](https://docs.qwencloud.com/token-plan/personal/token-plan-personal-faq)
  and [Team FAQ](https://docs.qwencloud.com/token-plan/team/token-plan-team-faq)
  restrict unattended/background use. Do not start Qwen reviews from cron or PR
  event handlers, disguise scheduled work as interactive, or silently substitute
  a paid API. A user-requested review can produce evidence for a separate,
  non-model validation/publishing gate.
- Provider credentials remain outside worker checkouts. Claude credentials are
  owned by Claude Code; Qwen uses an OpenClaw SecretRef. These access proofs do
  not make the example policy a production trust root.
- [Claude authentication](https://code.claude.com/docs/en/authentication) documents
  subscription OAuth. [OpenAI authentication](https://learn.chatgpt.com/docs/auth)
  distinguishes subscription access from separate API billing.

This task does not resolve runtime authority issue #1574; it avoids treating the
same kind of caller-supplied assertions as independent review evidence.
