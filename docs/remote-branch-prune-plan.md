# Remote branch prune plan

Issue #1164 uses `scripts/ci/remote-branch-prune-plan.py` to produce a
deterministic branch-to-SHA-to-PR manifest before any remote branch deletion.
The planner is read-only. A `candidate` is eligible for maintainer review; it
is not approval to delete.

The live planner queries every `refs/heads/*` ref and its associated pull
requests through GitHub GraphQL:

```bash
python3 scripts/ci/remote-branch-prune-plan.py --json
```

The report preserves the default branch, protected branches, configured
historical prefixes, and every branch with an open pull request. Branches
without a pull request, with an unknown PR state, or whose current SHA differs
from every terminal PR head are held for manual review. Only an unprotected
branch at the exact head of a merged or closed PR can become a candidate.

Before deletion, a maintainer must:

1. Review each candidate and explicitly approve the exact branch and SHA.
2. Re-run the planner immediately before mutation.
3. Confirm the branch still has the approved SHA and no open pull request.
4. Delete only the approved names, then re-run the planner and record the new
   remote branch count.

Use `--preserve-prefix` to add project-specific historical branch families.
The built-in conservative prefixes are `archive/`, `historical/`, `hotfix/`,
and `release/`.

Hermetic fixture validation does not require GitHub access:

```bash
python3 scripts/ci/test-remote-branch-prune-plan.py
```

## Review snapshot: 2026-07-29

The live repository had 90 remote branches: 17 exact-head candidates, 68 held
for manual review, and 5 preserved automatically. Automatic head deletion was
enabled and `main` was the only protected branch reported by GitHub. This table
is evidence for review, not deletion authority; every row must be regenerated
and matched by SHA immediately before an approved prune.

| Branch | Exact SHA | Associated terminal PR |
| --- | --- | --- |
| `codex/issue-780-effect-model` | `fa100fc44a2f1f3f85eb5b82b9ebf13e5e8033b9` | [#806](https://github.com/OMT-Global/axiomlang/pull/806) merged |
| `codex/issue-783-artifact-plan` | `14ce8c97eaa525ad0945a2f6c460b1fcc75dc99d` | [#804](https://github.com/OMT-Global/axiomlang/pull/804) merged |
| `codex/issue-784-repair-plan` | `9a1b413af3f9f65b557287773709daefc52986fe` | [#807](https://github.com/OMT-Global/axiomlang/pull/807) merged |
| `codex/rust-exit-runtime-serve` | `11317d5dc794ee66e47a1f0e80905593095947bf` | [#1290](https://github.com/OMT-Global/axiomlang/pull/1290) closed |
| `codex/rust-exit-unsupported-runtime-triage` | `673d6277a381e365576dbe35290e4193f57df737` | [#1263](https://github.com/OMT-Global/axiomlang/pull/1263) closed |
| `daedalus/1204-cranelift-duplicate-arms` | `15ca7b24e9f3f01debb5406ce83f0fdf9b43ef73` | [#1413](https://github.com/OMT-Global/axiomlang/pull/1413) closed |
| `daedalus/issue-111` | `41406c86a8265a17965669a845f3559a79b2aaec` | [#496](https://github.com/OMT-Global/axiomlang/pull/496) closed |
| `daedalus/issue-153` | `43429d54735b09c87f2fa8a77e0d875875943a96` | [#188](https://github.com/OMT-Global/axiomlang/pull/188) merged |
| `daedalus/issue-218` | `b4d9f7d22a56f6938a222ab2ed89f95dc25425cc` | [#493](https://github.com/OMT-Global/axiomlang/pull/493) closed |
| `daedalus/issue-219` | `68384126d18f8a0386c6e0f386936514fdf813e7` | [#492](https://github.com/OMT-Global/axiomlang/pull/492) closed |
| `daedalus/issue-220` | `4c6ae9b673a4a4fb1f95668f5ecee5ad43783d90` | [#491](https://github.com/OMT-Global/axiomlang/pull/491) closed |
| `daedalus/issue-221` | `30c50f6df601c8dc05278e99e5767631f4d9779a` | [#490](https://github.com/OMT-Global/axiomlang/pull/490) closed |
| `daedalus/issue-225` | `fdc344ba225b554d0fbafedf13b91ba9e120b4aa` | [#486](https://github.com/OMT-Global/axiomlang/pull/486) closed |
| `daedalus/issue-226` | `4de9442a589e1dbf42fabd7a37a5dec0dc5cbc97` | [#485](https://github.com/OMT-Global/axiomlang/pull/485) closed |
| `daedalus/issue-231` | `3f91e6af526dce52095ddcf55fb7eddd46a1f9cf` | [#482](https://github.com/OMT-Global/axiomlang/pull/482) closed |
| `daedalus/issue-88` | `76eacda191cc1fb7ce8480f4685a1c3b7dd1b345` | [#200](https://github.com/OMT-Global/axiomlang/pull/200) merged |
| `jmcte/codex/bootstrap-axiom` | `48c3fe5245d7d9720ee734950bf37650fc665272` | [#73](https://github.com/OMT-Global/axiomlang/pull/73) closed |
