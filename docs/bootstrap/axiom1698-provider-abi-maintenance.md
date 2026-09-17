# Proposed single-use Provider ABI CI trust promotion

LOCAL AND UNACTIVATED. This document is not permission to publish, mutate GitHub,
approve a deployment, rerun CI, or merge. It requires renewed exact-content review
and JT's separate authority.

The base-only executable-CI rule remains the default. The sole proposed exception is
for OMT-Global/axiomlang PR1698, branch pheidon/provider-abi-negative-env, base
4facdeb6358720e10f9187b38a4921f5c9091c16, and reviewed repair
a26544991d4e9377ded6613cf983d3999d5584ea. The repair is the only executable source;
PR head remains data through AXIOM_CHECKOUT_PATH.

Activation would require a retained immutable source ref
ci-source/axiom-provider-abi-a2654499 at the repair SHA, an exact stage variable
AXIOM_PROVIDER_ABI_1698_HEAD set only after publication/read-back, and a normal
jmcte stage approval record for environment id 13536211154. The guard rejects API
errors, missing/rejected/bypass-only approval, stale head/base/ref/repository,
wrong PR, missing/wrong variable, fork, or any non-fixed source. Other PRs execute
ordinary base-pinned scripts, although Fast Checks await stage approval while this
temporary mechanism exists.

JT must obtain the real newly-created run after a separately authorized publication
and use Review deployments to approve stage; no run URL exists before then. All
original Fast Checks commands, CI Gate, code-owner review, protection, and normal
head-guarded merge rules remain required. This mechanism does not itself defend
against malicious workflow mutation beyond platform protections and renewed review.

Before merge, reject/cancel the run, remove only the task variable under authority,
retain evidence/source, and feature-branch-revert if needed. After merge, remove the
variable, retain source until a normal reviewed cleanup removes this mechanism, prove
the exact reverse restores the PR1698 repair tree, then prove ordinary base-only CI.
Fresh automatic merged-main ExtendedValidation remains required before issue1697
closure.
