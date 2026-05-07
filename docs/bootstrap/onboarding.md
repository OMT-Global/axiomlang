# Bootstrap Onboarding

## Repo Governance

- Confirm the repository exists at `OMT-Global/axiom`.
- Confirm branch protection or rulesets on `main` require one approval and code owner review.
- Confirm branch protection points at the `CI Gate` status.
- Confirm `delete branch on merge` and `allow auto-merge` are enabled.
- Confirm `.github/PULL_REQUEST_TEMPLATE.md`, `CONTRIBUTING.md`, and the `Validate PR Description` job stay aligned when PR guidance changes.

## Pull Request Expectations

- Prefer the generated PR template with these headings:
  - `## Summary`
  - `## Governing Issue`
  - `## Validation`
  - `## Bootstrap Governance`
  - `## Notes`
- Make sure the PR body honestly links or closes the governing issue with an accepted form such as `Refs #262`, `Part of #262`, `Closes #262`, `Fixes #262`, `Resolves OMT-Global/axiom#262`, or a full GitHub issue URL. Use closing language only when the PR actually completes the issue.
- Record the local validation you actually ran so `Validate PR Description` and `CI Gate` can pass cleanly.
- Older pull requests may still pass a temporary legacy fallback when they link an issue and include a short prose summary; that fallback also accepts qualified issue references and full GitHub issue URLs, but new pull requests should use the structured template above.
- Keep required checks aligned to `CI Gate`; optional review-automation lanes should stay non-required.
- Capability manifest changes are checked by `scripts/ci/validate-capability-manifests.sh`
  in the fast lane; update that validator when new `[capabilities]` keys become
  part of the supported manifest contract.

## Environments

- `dev`: open by default for rapid iteration.
- `stage`: one reviewer required and self-review blocked.
- `prod`: one reviewer required, self-review blocked, deployments limited to `main`.

## Runner Policy

- Shell-safe jobs may use `[self-hosted, synology, shell-only, public]`.
- Docker, service-container, browser, and `container:` workloads stay on GitHub-hosted runners.
- Keep PR checks cheap. Add heavy validation to `scripts/ci/run-extended-validation.sh` instead of the PR lane.

## Home Profiles

- Run `project-bootstrap apply home --manifest ./project.bootstrap.yaml` after reviewing the bundled profile content.
- The bootstrap manages portable Codex and Claude assets only. Auth, sessions, caches, and machine-local state stay unmanaged.

## Claude Setup

- First-party Claude web sessions should use `bash scripts/claude-cloud/setup.sh` in `claude.ai/code`.
- Interactive Claude work is prepared through `.devcontainer/devcontainer.json`.
- GitHub-hosted Claude automation lives in `.github/workflows/claude.yml` and is intentionally separate from the required PR checks.
- Finish GitHub-side auth by running `/install-github-app` in Claude Code or adding `ANTHROPIC_API_KEY` as a repo secret.
