---
parent: 218
title: "AG1.2d: mutable borrows on struct/tuple projections"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:daedalus, status:ready-for-agent]
depends_on: [218-a-mutable-borrowed-locals]
---

Part of #218 / AG1.2. Permit `&mut value.field` and `&mut tuple.0` while honoring the projection-disjoint rules from #330.

## Scope

- Two non-overlapping projection mutable borrows of the same aggregate are allowed (`&mut pair.first` and `&mut pair.second`).
- Overlapping projection mutable borrows are rejected with a clear diagnostic that names both projections.
- Whole-value mutable borrow remains conservative: it invalidates every projection-rooted shared or mutable borrow.

## Acceptance

- Pass fixture: simultaneous `&mut pair.first` and `&mut pair.second`.
- Fail fixture: simultaneous `&mut pair.first` and `&mut pair.first`.
- Fail fixture: `&mut pair` while a projection borrow is live.

## Depends on

- 218-a (mutable locals).
- Built on the projection-disjoint facts in the #328 / #330 chain.
