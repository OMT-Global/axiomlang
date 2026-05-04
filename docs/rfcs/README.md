# Axiom RFCs

RFCs are the design record for language-level and runtime-level changes whose
tradeoffs should be reviewed before implementation. They are required when a
change alters syntax, type-system rules, capability semantics, package/runtime
contracts, or long-lived standard library APIs.

Small bug fixes, docs-only clarifications, test-only additions, and mechanical
refactors do not need an RFC unless they change a public contract.

## Numbering

- RFC numbers are four digits and monotonically increasing.
- Draft filenames use `NNNN-short-title.md`.
- `0000-template.md` is the template and is never assigned to a proposal.
- Reserve a number only when a draft PR is opened.

## Review Flow

1. Open a GitHub issue that states the problem, user impact, and affected
   compiler/runtime area.
2. Copy `0000-template.md` to `docs/rfcs/NNNN-short-title.md`.
3. Mark the RFC `Status: Draft` and link the governing issue.
4. Open a PR containing only the RFC and directly supporting docs.
5. Review the RFC against the acceptance bar below.
6. After approval, update the RFC status to `Accepted` and merge.
7. Implementation PRs must link the accepted RFC and include executable
   validation for the behavior they land.
8. If implementation proves the design wrong, update the RFC in a follow-up PR
   or replace it with a superseding RFC.

Rejected or withdrawn RFCs stay in the tree with their final status so design
history remains durable.

## Acceptance Bar

An RFC is ready to accept when it:

- states the user-facing problem and non-goals;
- specifies syntax or API contracts precisely enough to test;
- identifies parser, checker, HIR/MIR, codegen, runtime, stdlib, docs, and CI
  surfaces touched by the design;
- calls out compatibility and migration impact;
- describes security, capability, determinism, and host-boundary implications;
- names the conformance fixtures, Rust tests, examples, or smoke targets that
  will prove the feature;
- records alternatives considered and why they were not chosen.

## Initial Queue

The first RFCs should cover the largest unresolved language and runtime choices:

- traits and bounded generics, drafted in `0001-traits-bounded-generics.md` and tracked by issue #216;
- explicit lifetime and scope syntax, tracked by issue #219;
- type-level effect and capability modeling, tracked by issue #252;
- the async runtime and host scheduler model, tracked by issue #231.

