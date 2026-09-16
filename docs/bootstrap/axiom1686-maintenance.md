# Proposed single-use CI trust promotion (#1685/#1686)

**UNACTIVATED: requires explicit maintainer authorization of this narrow trust-policy exception, independent exact-content review, and protected environment approval.** This document is not that authorization.

The original #1543 rule in `pr-fast-ci.yml` says the executable CI checkout "must stay pinned to the PR *base*". That rule remains the default. The proposal adds one independently reviewed, non-head source: immutable repair `d551df9273e9ed58227384dd5984e7a0513c178a`, only for PR1686 on base `080e9daa2c6daf73eac77d56e42f71cb19085876` and branch `recovery/trusted-base-bootstrap-20260916`.

## Preconditions and execution

1. Independently review BOTH the four-file repair and this complete maintenance commit. Do not adopt timed-out Qwen work as a verdict. Confirm live protection still requires normal CI Gate, code owners, conversation resolution, strict base synchronization and admin enforcement.
2. JT explicitly authorizes this exact maintenance commit's temporary #1543 exception. Only then publish it as a fast-forward addition to #1686. Publishing triggers ordinary CI, with Fast Checks held by `environment: stage`; it does not authorize execution.
3. Retain `ci-source/axiom-bootstrap-d551df92` at exactly `d551df9273e9ed58227384dd5984e7a0513c178a` so squash/branch cleanup cannot strand the immutable source. No write to main.
4. After reviewing the new full PR SHA, JT authorizes setting **stage environment** variable `AXIOM_BOOTSTRAP_1686_HEAD` to that exact 40-hex SHA (not a branch name, not a repo/global variable). Inspect for and preserve any existing value rather than overwrite blindly. The operational command is `gh variable set AXIOM_BOOTSTRAP_1686_HEAD --repo OMT-Global/axiomlang --env stage --body <reviewed-full-head>`.
5. JT normally approves the matching pending **stage** deployment for that run in GitHub. No admin bypass. The inline preflight verifies the approval history contains a normal jmcte approval for stage13536211154 and no rejection, the live PR matches the event, the authorized exact head matches, and repository/base/ref/number are the fixed tuple. Missing approval, wrong head or base, forks, bypass-only histories and rejections fail closed. API failures fail the job.
6. Fast Checks executes all original checks using only the immutable reviewed repair as CI code. PR head remains data via AXIOM_CHECKOUT_PATH. The base checkout, full native tests, ABI checks, secret scan, PR description and CI Gate aggregator remain unchanged; failures propagate normally. No alternate CI Gate, status injection, or skipped failing test is introduced.
7. Reverify genuine required check results, exact-head independent review, eligible code-owner approval and all usual protection requirements, then perform normal squash merge with an exact-head guard. No admin merge or direct-main write.

## Scope, residual risk and removal

Environment approval is real trust promotion: the reviewer must inspect the actual workflow commit rather than approve solely from the environment name. A repository writer can propose malicious workflow changes, but they are not authorized by this plan; never approve a different head. This is why the workflow is not published/activated automatically under ordinary CI-repair authority. Existing stage admin-bypass capability is NOT used; the approval API record is independently required by the job.

All Fast Checks jobs require stage approval while this temporary workflow is installed. For any PR other than1686, executable CI remains base-only; the special repair cannot be selected. The old-base check means the exception is unusable after main advances. This intentionally favors fail-closed behavior over a general emergency mechanism. It does not solve any pre-existing public-runner exposure outside this change's scope.

After verified merge, delete the stage authorization variable (restore a prior value only if separately authorized), retain the source ref until reviewed removal lands, and prepare a normal reviewed cleanup PR reverting ONLY this maintenance commit while retaining the fixture repair. The repaired main now supplies the ordinary trusted CI, so cleanup has no bootstrap dependency. All normal CI/code-owner gates remain. This restores unattended base-only Fast Checks; do not remove the environment gate unreviewed.

## Abort / rollback

Before merge: reject the pending stage deployment, cancel the bootstrap run, delete only this task's stage variable, and add a normal revert of the maintenance commit to the feature branch if desired. Leave the original repair and all failed logs intact. No main/protection/runner mutation is needed. Head changes invalidate authorization automatically. A rejected stage record requires a new appropriately approved run, never bypassing the recorded rejection.

After merge: remove the temporary authorization and use the reviewed cleanup PR. Do not reset main, relax protection, or revert the fixture fix merely to clean up maintenance wiring. Reuse the passing native repair evidence only where the exact code is unchanged; fresh ordinary CI must validate the final merged state.
