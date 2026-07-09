# Self-Hosting Language Gaps

Seed checklist for the self-hosting language-readiness surface, produced by the
compiler.diagnostics feasibility spike under
[#1253](https://github.com/OMT-Global/axiomlang/issues/1253). Each entry is a
language or backend feature the spike needed, had to design around, or has since
unblocked for the stage1 AxiOM surface. The shipped rows preserve the original
spike diagnostic as historical evidence; the open rows still describe current
compiler gaps.

The spike itself lives at `stage1/selfhost/compiler-diagnostics-spike` and is
validated by `scripts/ci/run-self-hosting-spike-parity.sh`. It proves the
classification/rendering half of `compiler.diagnostics` can be authored in
AxiOM and run through the direct-native backend with `generated_rust: null`.
The suggestion-distance half (`edit_distance`, `closest_name`,
`message_with_suggestion`) is blocked by the gaps marked **blocking** below.

Umbrella tracking issue: [#1366](https://github.com/OMT-Global/axiomlang/issues/1366).

| # | Gap | Diagnostic observed | Spike impact | Workaround used | Severity for compiler rewrite |
|---|---|---|---|---|---|
| 1 | ~~No comment syntax of any kind~~ **Shipped**: full-line and trailing `//` comments (see `stage1/conformance/pass/line_comments`) | previously: `stage1 bootstrap currently supports top-level import, const, static, ...` | Spike sources now carry boundary comments without extra preprocessing. | `//` inside string literals remains source text, not a comment delimiter. | Closed. |
| 2 | ~~No character/byte access on strings~~ **Partially shipped**: `string_byte_at(value, index): Option<int>` (UTF-8 byte, `None` out of bounds; see `stage1/conformance/pass/string_byte_at`) | previously: `index expects an array or map value, got string` | Byte-level lexing is now expressible; `edit_distance` additionally needs gap 9. | Residual: substring/slice, split, and code-point iteration remain open; byte loops on direct-native also interact with gap 9. | Was High; residual Medium (code-point APIs also wait on gap 7). |
| 3 | ~~No string-contains primitive~~ **Shipped**: `string_contains(text, needle): bool` for literal-safe substring checks. | previously: `undefined function "string_contains"` | `stable_diagnostic_code` branches can now use direct substring checks. | `std/regex` remains available for pattern matching. | Was Medium; residual Low (case-folding and locale-aware search are out of scope). |
| 4 | ~~No general int-to-string conversion~~ **Shipped**: `int_to_string(value): string` mirrors the existing integer formatting intrinsic (see `stage1/conformance/pass/int_to_string`). | previously: `operator '+' expects matching numeric or string operands, got string and int`; `undefined function "int_to_string"` | `path:line:column` rendering can now use the direct intrinsic. | `json_stringify_int` remains as the JSON-oriented spelling. | Closed. |
| 5 | **Partially shipped**: string locals can be reassigned when the direct-native backend can keep the value as known static text | previously: `assignment target must be a scalar local, dereference a mutable reference, or index a mutable slice, got string` | Static accumulator-style string building is now expressible for compiler diagnostics fixtures. | Residual: runtime string mutation still needs a string value ABI beyond static text tracking; `std/string_builder` remains the heavier workaround. | Was Medium; residual Medium. |
| 6 | No borrowed string parameters; non-Copy arguments move per call | `invalid identifier "&string"`; `use of moved value "message"` | Every reuse of a string parameter across calls needs an explicit copy. | `string_clone(message)` at each use site. | Medium: verbose but sound. |
| 7 | No runtime-sized array allocation | n/a (no allocation form exists; array literals only) | **Blocking** for DP scratch rows sized by input length. | Caller-provided fixed-capacity literal arrays. | High for algorithmic compiler internals. |
| 8 | Borrowed slices of non-Copy element types cannot be indexed | `borrowed slice indexing requires a Copy element type, got string` | Candidate lists for `closest_name` cannot be passed as `&[string]`. | Not used; suggestion ranking deferred. | Medium. |
| 9 | ~~Direct-native backend rejects runtime loops with slice writes / match accumulators~~ **Partially shipped**: loop bodies now lower index writes through local mutable-slice aliases, option-int match accumulation (`Some(expr)` and `string_byte_at` on static strings), and the full Levenshtein DP (see `stage1/conformance/pass/runtime_loop_bodies` and `stage1/selfhost/compiler-diagnostics-distance-spike`) | previously: `runtime loops are not part of the cranelift hello spike` | `edit_distance` now runs native with Rust parity. | Residual: `&mut`/`&[T]`/`string` function parameters still do not lower (write-through ABI missing), so loops over caller-provided data must use by-value array params; one binary cannot yet mix compile-time-evaluated string surfaces with runtime loops. | Was High; residual Medium-High (parameter ABI is the next boundary). |
| 10 | No `for` iteration protocol | existing conformance fixture `for_loop_requires_iteration_protocol` | Style only. | `while` + index. | Low. |
| 11 | No mutable array bindings; element writes only through `&mut [T]` function parameters | `assignment target must be a scalar local, dereference a mutable reference, or index a mutable slice, got int` | All in-place mutation must be factored into helper functions. | Write-through helper pattern (`fn f(values: &mut [int], ...)`). | Medium. |

## Reading the severity column

- **High** entries gate the next migration steps (`compiler.syntax` is migration
  order 2 and is text-processing throughout).
- **Medium** entries have sound workarounds but multiply source size and review
  surface; they should become language features before multi-package ports.
- Gap 9 is a backend gap, not a language gap: the HIR accepts the program and
  `axiomc check` passes. It belongs to the direct-native hardening track
  (see the fixed-array notes in `stage1/runtime-abi/direct-native-v0.json`)
  but is listed here because self-hosting cannot proceed past toy slices
  without it.

## Relationship to gates

`make self-hosting-language-readiness` currently reports `ready: false` on the
`compiler_command_surface` and `compiler_scale_rewrite_fixture` rows. This
checklist is evidence for why: the spike shows the *expressible* subset is real
but narrow. The readiness gate must not be marked ready until at least the
High entries have governing feature issues with merged implementations or
explicit deferrals.
