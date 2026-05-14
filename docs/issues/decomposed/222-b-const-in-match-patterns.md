---
parent: 222
title: "Const/static B: named constants in match-pattern positions"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:daedalus]
---

Part of #222. Permit a top-level `const` to appear as a literal pattern in a match arm.

## Scope

- `const READY: int = 7; match status { READY => … }` resolves the pattern via name lookup.
- Lowercase names are still rejected to keep enum-variant disambiguation clear.
- Imported public constants behave the same way.

## Acceptance

- Pass fixture: match on an int against a const.
- Fail fixture: match arm `read_y => …` is rejected as either an unknown variant or an attempt to use a lowercase identifier as a pattern.

## Depends on

- None (works against the existing `const` scalar floor).
