# Formatter edit contract

`axiomc fmt --json` identifies its command-specific output with
`schema: "stage1/schemas/axiom-format-edit-v1.schema.json"`. The checked JSON
Schema is the public contract for formatter reports and their edits.

Each edit retains the bootstrap formatter's `action`, one-based `line`,
`before`, and `after` fields for operator readability. Automation should replay
the precise fields instead:

- `start_byte` is the inclusive UTF-8 byte offset in the original file.
- `end_byte` is the exclusive UTF-8 byte offset in the original file.
- `replacement` is the exact text that replaces that half-open range.

All offsets in a file refer to the same original source. Apply edits in
descending `start_byte` order so earlier replacements cannot invalidate later
offsets. Zero-width ranges are insertions and an empty replacement is a
deletion. Offsets always fall on UTF-8 character boundaries.

## Current scope and remaining formatter v1 work

This contract makes the existing whitespace normalizer replayable. It covers
tab expansion, trailing whitespace, repeated or trailing blank lines, CRLF
normalization, and the required final newline. It does not close #1460.

The remaining formatter v1 work is syntax-aware canonical layout for the full
language, comment and macro preservation, import ordering, malformed-source
recovery, stdin and range formatting, parse/check and semantic-digest
preservation, complete-corpus two-pass idempotence, LSP formatting transcripts,
and compiler-scale performance bounds.
