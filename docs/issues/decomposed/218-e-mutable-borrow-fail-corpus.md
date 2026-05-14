---
parent: 218
title: "AG1.2e: compile-fail corpus for mutable borrow rules"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:ares, status:ready-for-agent]
depends_on: [218-a-mutable-borrowed-locals, 218-b-mutable-borrowed-slices, 218-c-mutable-borrows-call-boundary, 218-d-mutable-projection-borrows]
---

Part of #218 / AG1.2. Lock in the mutable-borrow rules with a conformance corpus that codifies the negative cases. Closes AG1.4 for the mutable-borrow chapter.

## Scope

- `stage1/conformance/fail/mutable_borrow_double_mut` — two `&mut T` over the same place.
- `stage1/conformance/fail/mutable_borrow_mut_plus_shared` — `&mut T` + `&T` overlap.
- `stage1/conformance/fail/mutable_borrow_use_after_borrow` — read original while `&mut T` is live.
- `stage1/conformance/fail/mutable_borrow_overlapping_slice_mut` — overlapping `&mut [T]` ranges.
- `stage1/conformance/fail/mutable_borrow_whole_then_projection` — `&mut pair` then `&mut pair.first`.
- Each fixture has a deterministic `expected-error.json` with a stable error kind / code.

## Acceptance

- All five fixtures fail with the expected diagnostics.
- `tests::conformance_corpus_reports_stable_results` is updated to match.
- Stable diagnostic codes are documented in `docs/stage1.md` under the ownership section.

## Depends on

- All four prior AG1.2 slices.
