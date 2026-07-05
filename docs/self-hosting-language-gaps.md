# Self-Hosting Language Gaps

Seed checklist for the self-hosting language-readiness surface, produced by the
compiler.diagnostics feasibility spike under
[#1253](https://github.com/OMT-Global/axiomlang/issues/1253). Each entry is a
language or backend feature the spike needed (or had to design around) that the
stage1 AxiOM surface does not provide today. Every gap below was reproduced
against the current compiler; the quoted text is the verbatim diagnostic.

The spike itself lives at `stage1/selfhost/compiler-diagnostics-spike` and is
validated by `scripts/ci/run-self-hosting-spike-parity.sh`. It proves the
classification/rendering half of `compiler.diagnostics` can be authored in
AxiOM and run through the direct-native backend with `generated_rust: null`.
The suggestion-distance half (`edit_distance`, `closest_name`,
`message_with_suggestion`) is blocked by the gaps marked **blocking** below.

Umbrella tracking issue: [#1366](https://github.com/OMT-Global/axiomlang/issues/1366).

| # | Gap | Diagnostic observed | Spike impact | Workaround used | Severity for compiler rewrite |
|---|---|---|---|---|---|
| 1 | No comment syntax of any kind | `stage1 bootstrap currently supports top-level import, const, static, type, struct, enum, fn, let, print, panic, defer, if/else, while, and match statements` | Spike sources cannot carry doc or boundary comments. | None; comments removed. | High: compiler-scale sources need documentation. |
| 2 | No character/byte access, substring, split, or char iteration on strings | `index expects an array or map value, got string`; `slice expects an array or slice value, got string` | **Blocking** for `edit_distance` and any lexer work (migration order 2). | Corpus-level: callers must pre-split text into char-code int arrays. | High: blocks `compiler.syntax` entirely. |
| 3 | No string-contains primitive | `undefined function "string_contains"` | Needed by every `stable_diagnostic_code` branch. | `std/regex` `is_match` with literal-safe needles. | Medium: workaround is adequate but subtle (needles must be regex-safe). |
| 4 | No general int-to-string conversion | `operator '+' expects matching numeric or string operands, got string and int`; `undefined function "int_to_string"` | Needed for `path:line:column` rendering. | `json_stringify_int`. | Medium: workaround exact for integers. |
| 5 | String locals cannot be reassigned (only scalar int/bool locals) | `assignment target must be a scalar local, dereference a mutable reference, or index a mutable slice, got string` | Prevents accumulator-style string building in loops. | Expression composition; `std/string_builder` exists for heavier cases. | Medium. |
| 6 | No borrowed string parameters; non-Copy arguments move per call | `invalid identifier "&string"`; `use of moved value "message"` | Every reuse of a string parameter across calls needs an explicit copy. | `string_clone(message)` at each use site. | Medium: verbose but sound. |
| 7 | No runtime-sized array allocation | n/a (no allocation form exists; array literals only) | **Blocking** for DP scratch rows sized by input length. | Caller-provided fixed-capacity literal arrays. | High for algorithmic compiler internals. |
| 8 | Borrowed slices of non-Copy element types cannot be indexed | `borrowed slice indexing requires a Copy element type, got string` | Candidate lists for `closest_name` cannot be passed as `&[string]`. | Not used; suggestion ranking deferred. | Medium. |
| 9 | Direct-native backend rejects runtime loops that write through `&mut` slice parameters | `unsupported by --backend cranelift spike: runtime loops are not part of the cranelift hello spike` | **Blocking**: HIR accepts the `edit_distance` DP loop but the supported backend cannot lower it. | None; function excluded from the spike. | High: this is the sharpest HIR-vs-backend split found; also relevant to the `compiler_scale_rewrite_fixture` readiness row. |
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
