# Target Support Evidence Manifest — Refs #1455, supersedes #1642

This directory is the committed exact-head target-support evidence bundle for
this PR. It is the only content added between the tested code head
`ff5e549ea3fe238204c1e12a34a39606eb80e412` (H1d) and this commit (H2).

## Authority and revised contract

- Governing issue: #1455 (prove Linux x86-64 and macOS arm64 native toolchains).
- Binding review: Athena CHANGES_REQUESTED 2026-08-20T18:06:33Z on original head
  `48766171daf3f34ed0c3e1fd013f38c32c4e8ba0`: a main-only target job cannot provide
  pre-merge exact-head evidence; require an approval-gated exact-PR-SHA path or
  explicitly change to post-merge evidence, then resolve conflicts and refresh CI.
- Narrow gateway-owner decision (JT, 2026-09-25T23:16Z): an explicitly reviewed
  specialized macOS evidence lane solely to produce exact-head target-support
  evidence for #1455/#1642, while ordinary CI remains hosted `ubuntu-24.04` and no
  new spend. Not a blanket runner-policy change; no self-hosted runner registration;
  no macOS CI jobs.
- Revised contract implemented here (see `docs/target-support-v1.md`):
  - **Linux x86-64**: pre-merge assurance = full ordinary hosted CI at the exact
    head (PR `CI Gate` on ubuntu-24.04, plus the recorded dispatch runs below).
    The authoritative `axiom.target_support_evidence.v1` artifact is produced
    continuously post-merge by the `target-support-evidence` job on `main`
    pushes/nightly — the binding review's explicitly sanctioned post-merge option —
    AND an approval-gated pre-merge exact-PR-SHA path is live: `workflow_dispatch`
    with `evidence_pr_sha` (write-access-gated, fail-closed 40-hex validation)
    produces the same artifact for any exact PR head on demand.
  - **macOS arm64**: pre-merge exact-head evidence is produced only by the
    specialized gateway-managed node lane (below), never by a CI job.

## Pins and lineage (all verified live)

| Pin | SHA | Note |
| --- | --- | --- |
| Base main at authoring | `6078b898e92db5e9b1deb85f79b8f764a96a86bc` | live-verified before branching |
| Original #1642 head | `48766171daf3f34ed0c3e1fd013f38c32c4e8ba0` | author jmcte; UNTOUCHED |
| Original #1642 base | `b3149c5e9bf10a4a244b0d89c6e6cd804b47ae3f` | |
| H1 | `91ec57036c32e9873149281c582d0c413beb9dbc` | cherry-pick reauthor of 4876617 onto base; all 8 semantic conflicts resolved preserving fail-closed semantics; **author John McChesney TenEyck Jr <59268465+jmcte@users.noreply.github.com> preserved**, committer pheidon |
| H1b | `f5ae3590f9d59a61d11f8e54a1cee2f82bdb44c0` | fix: job-level env may not reference the `runner` context (GitHub dispatch 422) |
| H1c | `4f2e329f18f251f1dd806a0309370bc8fcad04e1` | fix: evidence CLI must not dirty its own exact-head checkout (`__pycache__`); + subprocess regression test |
| H1d | `ff5e549ea3fe238204c1e12a34a39606eb80e412` | fix: drop stale `RUST_VERSION: 1.90.0` pin (locked deps MSRV 1.93.0: cranelift 0.132 / wasmtime-internal-core 45); workflow test fails closed on reintroduction |
| H2 | this commit | H1d + this evidence directory only |

## Binding verification (reviewer commands)

```sh
# 1. This commit adds ONLY this evidence directory:
git diff --name-only ff5e549ea3fe238204c1e12a34a39606eb80e412 HEAD
git diff ff5e549ea3fe238204c1e12a34a39606eb80e412 HEAD -- . ':(exclude)evidence/target-support-1642' # must be empty

# 2. File integrity:
cd evidence/target-support-1642 && sha256sum -c CHECKSUMS.sha256

# 3. Artifact self-validation (fail-closed schema + cross-field checks):
python3 scripts/ci/run-target-support-evidence-v1.py validate --evidence evidence/target-support-1642/macos-arm64.json

# 4. Artifact binds to the exact tested head and tree:
python3 - <<'PY'
import json, subprocess, hashlib
ev = json.load(open("evidence/target-support-1642/macos-arm64.json"))
head = "ff5e549ea3fe238204c1e12a34a39606eb80e412"
assert ev["head_sha"] == head
lock = subprocess.run(["git","show",f"{head}:stage1/Cargo.lock"],capture_output=True,check=True).stdout
assert ev["toolchain"]["cargo_lock_sha256"] == hashlib.sha256(lock).hexdigest()
epoch = subprocess.run(["git","show","-s","--format=%ct",head],capture_output=True,text=True,check=True).stdout.strip()
assert ev["toolchain"]["source_date_epoch"] == int(epoch)
assert ev["evidence_status"] == "passed" and all(c["status"]=="passed" for c in ev["checks"])
print("binding OK")
PY
```

Precomputed values: `cargo_lock_sha256 =
e52cd0aa97e40d882a58d3561d5a67f1cd8487a4cbf79bd1addde815d6d72b2f`,
`source_date_epoch = 1790386641`.

## macOS arm64 binding evidence (specialized lane at H1d)

- Node: JMCTE Macbook (OpenClaw node `11ed57bd733267c468297b12aaa9afb6c67ff75a6cf71644eb8a3b05ba166873`),
  Mac16,5, hw.ncpu 16, macOS 27.0 (26A428), Darwin 27.0.0 arm64 (T6041),
  hostname JMCTE-Macbook.local, user johnteneyckjr. — full capture in
  `node-identity.txt` (sha256 `5f7f74eb845836b3d7bdc1be893953e405bba3e15e0ebbbd609a174fefdfff18`).
- Toolchain: rustc 1.93.1 (01f6ddf75 2026-02-11), host `aarch64-apple-darwin`;
  cargo 1.93.1; Python 3.9.6; git 2.54.0 (Apple Git-157); bash 3.2.57; Apple clang 21.
- Run: START `2026-09-26T01:42:54Z` → END `2026-09-26T01:44:14Z`, RC=0 (80 s);
  log `macos-run.log` (sha256 `c5a9c118b2e3435e8e947c9d43d41292b52ea9c825c7a647fbda460a58d3efb8`).
- Artifact: `macos-arm64.json` sha256
  `093823cb87c43d42be1d50b04ee5e7b4079dd21539192017f4d56e68d4b42239`;
  `evidence_status=passed`; 8/8 checks passed (debug-build, doctor-report,
  host-identity, native-smoke, proof-workloads, release-build, target-contract,
  unsupported-target); qualification stays `partial` with `host_evidence=true`,
  `cross_compilation=false`, `release_qualification=false` (no overclaim).
  Binaries: axiomc debug 67,455,000 B (sha256 `1bf7d150…`), release 19,258,080 B
  (sha256 `52dec2b7…`), Mach-O arm64 magic/cpu-type verified by the script, with
  binary identity re-verified before and after each compiler invocation.
- Isolation & hygiene (read-mostly compliance): fresh `mktemp -d` clone
  (`/tmp/axiom-1642-evidence-h1d.RW9art`), detached checkout at H1d; preflight
  `git status --porcelain=v1 --untracked-files=all --ignored=matching` empty;
  `cargo fetch --locked --offline` green (pre-existing `~/.cargo` registry cache
  only — no installs, no host/config changes, no benchmarks); build/run fully
  offline (`CARGO_NET_OFFLINE=true`, `AXIOM_REGISTRY_NETWORK_DISABLED=1`);
  `PYTHONDONTWRITEBYTECODE=1`; evidence output written outside the worktree;
  `caffeinate -i` against idle sleep; post-run tracked state clean at `ff5e549e`.
- Duration mechanics (disclosure): the run reuses the warm cargo build cache at the
  script's deterministic default `CARGO_TARGET_DIR`
  (`/tmp/axiom-target-evidence-macos-arm64`), created by the superseded H1b
  rehearsal run (directory birthtime `2026-09-25T20:00:19` local, verified). No
  sccache (not installed) and no `~/.cargo/config`. Rust sources are identical
  across H1b/H1c/H1d (the fixes touch only workflow/CI-python files), so cargo
  fingerprints hit while doctor, native-smoke, proof workloads, and the
  unsupported-target negative all executed live. Timings: H1b ≈100 s (cold
  directory creation — anomalously fast for a cold tree; disclosed for scrutiny;
  superseded), H1c 76 s, H1d 80 s (warm, mechanically consistent). Source binding
  is cache-independent: exact-head checkout verification, lockfile digest at start
  and end, tracked-state recheck, and binary identity reverification are enforced
  by the fail-closed script itself.
- Lane history (transparency): rehearsal runs at H1b (passed; artifact preserved
  in the author checkpoint `reports/axiom-1642-reauthor-20260925/evidence/macos-lane-h1b/`)
  and H1c (passed; superseded by the H1d head move) preceded the binding run. A
  first H1d attempt (START 01:39:13Z) was killed by an OpenClaw companion-app
  disconnection before producing output (log has START without RC/END; forensics:
  tree clean, zero byproducts); it was relaunched detached — the binding run
  above. Superseded rehearsal artifacts are deliberately NOT part of the binding
  set.

## Linux x86-64 evidence (ordinary hosted ubuntu-24.04 CI only)

Full details in `linux-ci-record.md`. Summary:

- Run 2 — approval-gated exact-PR-SHA dispatch at H1c: run `36207965390`
  (`workflow_dispatch`, `evidence_pr_sha=4f2e329f…`). **Fast Checks SUCCESS** and
  **Extended Checks SUCCESS** (complete qualification suite) on hosted
  `ubuntu-24.04` at the exact head. The `Target Support Evidence` job failed
  solely on the stale inherited `RUST_VERSION: '1.90.0'` pin versus the locked
  dependency MSRV (every cargo invocation exited 101: "rustc 1.90.0 is not
  supported … cranelift-* requires rustc 1.93.0"); fixed in H1d, a workflow-only
  change (no product-code delta H1c→H1d). The job's fail-closed `failed` evidence
  JSON was still produced and uploaded (honest record). The gate failed as
  designed because the evidence job failed.
- Run 1 — dispatch at H1b: run `36206756482`. Fast Checks SUCCESS; evidence job
  failed on the CLI's own `__pycache__` write tripping its (correct) purity gate;
  fixed in H1c with a regression test; run cancelled to conserve the CI budget
  once root-caused. Zero-job parse-failure run `36206489859` (H1 push) recorded
  the `runner`-context expression rejection fixed by H1b.
- Run 3 — PR CI at H2 (this head): the required `CI Gate` fast suite on hosted
  ubuntu-24.04, triggered by opening this PR; id and terminal result are recorded
  in the PR conversation and the author checkpoint (this file is committed before
  that run exists). This is the pre-merge Linux exact-head assurance under the
  revised contract.
- CI budget: 3 runs consumed in total (two dispatches + this PR's CI), within the
  ≤3 cap; therefore no additional dispatch was spent on a pre-merge Linux artifact
  JSON at H2. The artifact contract is post-merge-continuous on `main` plus the
  maintainer-dispatchable approval-gated exact-SHA path (exercised end-to-end by
  runs 1–2 through checkout/validation/fetch/purity stages; the sole failure
  causes were found, fixed, and regression-tested).

## Provenance and runtimes (disclosure)

- Author runtime: `qwen-token-plan/qwen3.8-max` (high reasoning) under the recorded
  OpenAI-provider 401 outage routing. The gateway owner restarted the gateway
  ~2026-09-26T01:21Z mid-task; one node exec was interrupted, reconciled by
  forensics, and cleanly relaunched (no duplicated work).
- Commit attribution: H1 preserves jmcte authorship (verify:
  `git log --format='%h %an <%ae> | committer %cn' 6078b89..HEAD`). H1b/H1c/H1d/H2
  are pheidon-authored fix/evidence commits.
- Independent review: `anthropic/claude-opus-5` (Qwen→Claude mapping) on the exact
  H2 head/base/tree/diff including this manifest; verdict recorded in the author
  checkpoint and reported to the merge owner. Qwen self-review is not independent.
- Merge ownership: maintainer/parent lane. No auto-merge enabled by the author;
  fallback merge-readiness policy recorded in the PR body.

## Compliance statements

- No self-hosted GitHub runner was registered or configured; no macOS jobs exist
  in any CI workflow; `.github` runner policy and `AGENTS.md` are untouched;
  ordinary CI remains hosted `ubuntu-24.04`.
- No branch-protection, repository-config, or scheduler changes; no force-push;
  the original #1642 branch/head is untouched.
- No new spend: hosted CI within budget, the existing gateway-managed macOS node,
  and a pre-existing local docker image for author-side Rust validation only.
- No weakened assertions or edited frozen fixtures: `accepted-baseline`,
  `previous-current`, and historical `before-*` fixtures are byte-identical (the
  compatibility suite re-asserts their SHA-256s and passed); the reauthoring added
  assertions (surface version pins, RUST_VERSION-absence guard, bytecode-pollution
  regression test).

## Files in this directory

- `EVIDENCE.md` — this manifest
- `macos-arm64.json` — binding macOS arm64 evidence artifact (H1d, passed)
- `macos-run.log` — binding lane run log (START/RC/END, UTC)
- `node-identity.txt` — binding lane node identity capture
- `commands.txt` — exact commands used for the lane and dispatches
- `linux-ci-record.md` — hosted-CI run records with failure/fix disclosure
- `CHECKSUMS.sha256` — SHA-256 of every file in this directory except itself
