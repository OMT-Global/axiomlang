# Decomposed Issue Bodies

This directory holds copy-paste-ready GitHub issue bodies for the next decomposition wave. They were generated when the GitHub MCP server was unavailable; once it's back, the maintainer (or a future agent session) can paste each file into `gh issue create` or the web UI and attach as a sub-issue of its parent.

The first decomposition wave (sub-issues for #326, #97, #328, #330, #332, #101) was filed live as #603 – #622 and is **not** repeated here.

## Wave 2 contents

| Parent | Children (files in this directory) | Count |
|---|---|---|
| #560 Phase-H first self-hosted test | `560-h1-…`, `560-h2-…`, `560-h3-…` | 3 |
| #561 Phase-I compiler test suite in AxiOM | `561-i1-…`, `561-i2-…`, `561-i3-…` | 3 |
| #562 Phase-J compiler internals in AxiOM | `562-j1-…`, `562-j2-…`, `562-j3-…` | 3 |
| #563 Phase-K doc generator in AxiOM | `563-k1-…`, `563-k2-…`, `563-k3-…` | 3 |
| #564 Phase-L LSP server in AxiOM | `564-l1-…`, `564-l2-…`, `564-l3-…` | 3 |
| #105 Direct native backend (post-AG5) | `105-a-…`, `105-b-…`, `105-c-…`, `105-d-…` | 4 |
| #223 Declarative macros | `223-a-…`, `223-b-…`, `223-c-…`, `223-d-…` | 4 |
| #230 DWARF debug info | `230-a-…`, `230-b-…` | 2 |

**Wave-2 total: 24 child issues.**

## Wave 3 contents

Feature-gap decompositions discovered by auditing `docs/roadmap.md`, `docs/stage1-agent-grade-compiler.md`, `docs/stage1-stdlib-status.md`, and `docs/stage1-language-issue-disposition.md`, plus session-discovered code-health items.

| Parent | Children (files in this directory) | Count |
|---|---|---|
| #218 mutable references (AG1.2) | `218-a-…` through `218-e-…` | 5 |
| #220 full numeric tower | `220-a-…` through `220-f-…` | 6 |
| #222 const / static remaining | `222-a-…` through `222-d-…` | 4 |
| #234 net sockets | `234-a-…` through `234-d-…` | 4 |
| #236 crypto primitives | `236-a-…` through `236-e-…` | 5 |
| #216 traits (per RFC 0001) | `216-a-…` through `216-d-…` | 4 |
| AG1.4 stable ownership diagnostics | `ag1-4-stable-ownership-diagnostics.md` | 1 |
| AG4.2 async runtime gaps | `ag4-2-async-runtime-gaps.md` | 1 |
| stdlib: stdin readline | `stdlib-io-stdin-readline.md` | 1 |
| Code health: native-test gating | `code-health-native-test-gating.md` | 1 |

**Wave-3 total: 32 child / standalone issues.**

## Wave 1 (already on GitHub)

Filed live as #603 – #622: 20 sub-issues for #326, #97, #328, #330, #332, #101. Not duplicated in this directory.

## Grand total

44 GitHub-filed + 56 ready-to-file in this directory = **100 granular work items** carved out of the ~12 umbrella issues.

## File format

Each file begins with a YAML frontmatter block:

```yaml
---
parent: 560
title: ...
labels: [...]
depends_on: [...]   # optional, by file basename
---
```

The body below `---` is the issue text. When filing:

1. Use the `title` for the GitHub issue title.
2. Use everything below the closing `---` as the body.
3. Apply `labels` exactly as listed.
4. After creation, attach as a sub-issue of `parent` via the GitHub UI or `gh` CLI sub-issue support.

## Working rules (apply to every child)

- Keep the slice narrow and issue-backed.
- Prefer Rust stage1 coverage and machine-readable diagnostics / contracts.
- Do not weaken existing stage1 AG0–AG5 gates or Python-exit blockers.
