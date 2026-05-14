---
parent: 218
title: "AG1.2b: mutable borrowed slices"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:daedalus, status:ready-for-agent]
depends_on: [218-a-mutable-borrowed-locals]
---

Part of #218 / AG1.2. Extend mutable references to `&mut [T]` slice borrows of locally-owned arrays / vectors.

## Scope

- `let s: &mut [int] = &mut arr[..]` typechecks and borrow-checks.
- Element write via `s[i] = …` lowers correctly through codegen.
- Borrow checker forbids overlapping mutable slice borrows and forbids a shared `&[T]` borrow over the same region while the mutable slice borrow is live.

## Acceptance

- Pass fixture: in-place transformation of an `[int]` via `&mut [int]`.
- Fail fixture: two overlapping `&mut [int]` borrows.
- Fail fixture: `&mut [int]` plus `&[int]` over the same array.

## Depends on

- 218-a (mutable local borrows).

## Out of scope

- Slice-of-slice splits (`split_at_mut`) — separate follow-up.
- Mutable slice across call boundaries — 218-c.
