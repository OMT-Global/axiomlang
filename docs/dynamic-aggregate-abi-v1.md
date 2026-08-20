# Dynamic Aggregate ABI v1

Issue [#1439](https://github.com/OMT-Global/axiomlang/issues/1439) requires
runtime-origin non-Copy aggregates to cross calls, returns, mutation, storage,
and cleanup without compiler-known contents. This document defines the
target-neutral ABI that backends must eventually implement and separately
records the smaller direct-native floor that can be executed today.

The machine-readable sources are:

- `stage1/compiler-contracts/schemas/axiom.dynamic_aggregate_abi.v1.schema.json`
- `stage1/compiler-contracts/snapshots/dynamic-aggregate-abi-v1.json`
- `stage1/compiler-contracts/fixtures/dynamic-aggregate-abi-v1/`

This tranche is partial and does not close #1439. The layout and ownership
models are normative contract tests, not executable runtime proof. Runtime-
origin owned strings, dynamic non-Copy storage, owned field projection,
recursive cleanup, and retirement of static projection paths remain blocked.

## Deterministic layout

Every layout record is qualified by target triple, ABI profile, pointer width,
and byte order. Struct fields use declaration order; tuple and array elements
use ascending index order. For each field, the backend computes
`offset = align_up(current_offset, field_alignment)`. Aggregate alignment is
the maximum field alignment, and final size is `align_up(end, alignment)`.
Arrays use `stride = align_up(element_size, element_alignment)` and reject
size arithmetic that exceeds the target address range. Inspection records are
bounded to 4,096 materialized fields so a compact untrusted type model cannot
force unbounded memory allocation.

Enum tags encode stable source variant ordinals in the smallest unsigned width
from 1, 2, 4, or 8 bytes. The payload size and alignment are the maxima across
all variants. The payload begins at
`align_up(discriminant_width, payload_alignment)`; total size is the aligned
end of that payload. Enum field offsets are aggregate-relative, including the
payload offset. Every variant records its source ordinal and complete
field-offset list; the summary offset list remains the widest-payload
compatibility view. `Option` and `Result` use the same rule. Padding is zeroed
and has no AxiOM-observable meaning.

Arguments and results use one closed selection rule. A layout is direct when
its size is no more than two pointer words and its alignment is no greater
than one pointer word. Direct arguments and returns use `direct_value`.
Larger or over-aligned arguments use `indirect_pointer`; returns use
`caller_provided_storage`. There is no backend-specific discretionary branch.

The target-layout fixtures record target triple, ABI profile, pointer width,
byte order, size, alignment, discriminant width and offset, payload offset,
field offsets, argument passing, and return passing. The checker recomputes
those records from type models and rejects fabricated records or arithmetic
overflow. The accepted v1 target profiles are the exact canonical pairs
`x86_64-unknown-linux-gnu` / `sysv64-v1` and `aarch64-apple-darwin` /
`aapcs64-v1`; pointer width and byte order must match the selected profile.
Dedicated fixtures exercise tuple, option, and result layout instead of merely
listing those kinds in the contract vocabulary.

## Ownership and lifetime

The target-neutral transition model makes cleanup obligations explicit. Move
transfers one obligation and invalidates the source. Borrow preserves the
owner and blocks conflicting move, mutation, or drop for its extent. Clone
creates an independent obligation. Drop consumes one live obligation; a
second drop is rejected. Early exit discharges live obligations in reverse
creation order.

Executable compiler cases validate move-after-move and borrow-conflict
diagnostics. The transition model validates clone independence, exactly-once
discharge, double-drop rejection, and early-exit order. That model is not
executable runtime proof of owned aggregate clones or destructors.

Recursive owned cleanup, early-return owned cleanup, panic/unwind cleanup,
cancellation cleanup, and a runtime double-drop diagnostic remain a target gap.
The current backend must fail closed instead of claiming those paths. The v1
contract intentionally makes no production claim for unwind or cancellation.

## Current direct-native floor

The current evidence tier remains `static_spike` / `partial`. The executable
fixture compiler builds a real Cranelift binary from a scalar aggregate that
crosses three helper boundaries, verifies that no generated host source was
emitted, and validates the binary's exact exit code and output. Separate
compiler fixtures validate structured ownership and fail-closed build
diagnostics.

This evidence proves bounded scalar and boolean projection only. It does not
prove the physical layout model is used by the backend for runtime-origin
non-Copy values. Unsupported paths must report
`backend.runtime_lowering_required` and emit neither a generated-host fallback
nor a binary.

## Trusted validation

The checker accepts `--root` (also `--checkout-root`) so the trusted base-pinned
CI script can treat the pull-request checkout strictly as data. Checker
self-tests continue to run from the trusted script checkout. Fast Checks never
passes `--execute`; activating executable PR CI must wait until this checker is
present on the trusted base branch. The local executable proof still requires a
governed digest for every program tree, compiles the selected checkout in an
isolated target directory, and copies governed programs to a temporary
directory before building them.

Run the focused checks with:

```bash
python3 scripts/ci/test-check-dynamic-aggregate-abi-v1.py
python3 scripts/ci/check-dynamic-aggregate-abi-v1.py --execute --json
bash scripts/ci/test-pr-fast-ci-workflow.sh
python3 scripts/ci/extract-public-contract-v1.py --check
```
