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

**Total: 25 sub-issues** (24 child issues + this index).

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
