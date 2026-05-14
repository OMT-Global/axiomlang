---
parent: 218
title: "AG1.2c: mutable borrows across call boundaries"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:daedalus, status:ready-for-agent]
depends_on: [218-b-mutable-borrowed-slices]
---

Part of #218 / AG1.2. Allow `&mut T` and `&mut [T]` to appear as function parameter types and propagate borrow facts across calls.

## Scope

- Function signatures accept `param: &mut T` and `param: &mut [T]`.
- Caller-side borrow check: the argument's borrow becomes inactive at the caller for the lifetime of the call.
- Callee-side borrow check: the parameter is a fresh borrow that obeys the local rules.
- Conformance pass fixture: helper function takes `&mut [int]` and increments each element.

## Acceptance

- Pass fixture builds and runs with the expected output.
- Caller cannot read or write the value while the call is in flight (compile error).
- Existing call-boundary tests remain green.

## Depends on

- 218-a, 218-b.

## Out of scope

- Returning mutable borrows from functions — covered by the #332 lifetime elision work.
