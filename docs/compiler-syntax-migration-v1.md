# Compiler syntax migration v1

Issue [#1471](https://github.com/OMT-Global/axiomlang/issues/1471) defines the
target for moving lexing, parsing, comments, macros, recovery, spans, and
syntax inspection into an AxiOM-owned `compiler.syntax` package. The target is
normative; the current evidence remains a Rust-bootstrap `syntax_only` floor
and does not authorize cutover.

The machine-readable contract is:

- `stage1/compiler-contracts/schemas/axiom.compiler.syntax_migration.v1.schema.json`
- `stage1/compiler-contracts/snapshots/syntax-migration-v1.json`
- `stage1/compiler-contracts/fixtures/syntax-migration-v1/*.json`

## Entry gates

Syntax migration cannot enter cutover from syntax evidence alone. Issue #1427
must first provide one reviewed compiler-scale binary that processes distinct
runtime source inputs. Issue #1468 must separately record that all parent
runtime prerequisites are satisfied and that a maintainer approved cutover for
the exact qualified artifact SHA-256. Issue #1473 must independently qualify
the diagnostics contract and exact structured diagnostic parity. All four
gates are explicit blocked machine gates;
none can be inferred from fixture labels or an otherwise green checker.

## Span semantics

Spans use zero-normalization-free offsets into the exact input bytes: starts
are inclusive, ends are exclusive, and offsets count UTF-8 bytes. Lines and
columns are one-based; columns count Unicode scalar values. A tab counts as one
scalar and is not expanded to a display width. LF and CRLF each count as one
line break. Malformed UTF-8 is rejected before lexing with
`source.invalid_utf8` and the first invalid byte offset.

The Unicode fixture freezes non-ASCII identifiers and emoji, tabs, CRLF, and
malformed-byte vectors. This is target semantics, not current bootstrap proof;
the dedicated cross-backend corpus remains blocked.

## Canonical node identity

Canonical IDs use
`axiom://syntax/sha256/{source_digest}/{origin}/{kind}/{ordinal}`. The source
digest is lower-case SHA-256 of the exact input bytes. Kinds are lower-snake
ASCII, ordinals are base-10 without leading zeroes, and allocation follows
source-order depth-first preorder within each source origin.

Source nodes use the `source` origin. Macro nodes use
`macro-{call_site_source_ordinal}` and expansion preorder. Recovered nodes use
`recovered` at the first skipped token and occupy the corresponding source
preorder slot. Synthetic nodes use
`synthetic-{owning_origin}-{owning_kind}-{owning_ordinal}` and a deterministic
per-owner construction ordinal, so equal ordinals from source, macro, and
recovery origins cannot produce the same synthetic origin. The full
source-digest/origin/kind/ordinal tuple must be unique. Host addresses,
randomized hashes, Rust module paths, and Rust type names are prohibited. The
collision/parity vectors remain target gaps; bootstrap IDs are stable only in
their existing noncanonical `path:line:column:kind:name` form.

## Recovery contract

Recovery emits ordered `compiler.diagnostics` records by source span and then
emitter order. A qualified implementation also emits structured recovered
nodes carrying node identity, node kind, skipped byte range, span, and linked
diagnostic IDs. Resynchronization is limited to item, declaration, statement,
or block boundaries. The bootstrap fixtures execute exact ordered diagnostic
records, but the Rust bootstrap returns no recovered nodes; recovered-node
emission is therefore still a target gap.

## Macro bounds and provenance

Macro expansion preserves definition and call-site spans and uses separate,
inclusive budgets:

| Budget | Default | Ceiling | Unit | Scope | Target failure code |
| --- | ---: | ---: | --- | --- | --- |
| Recursion | 64 | 1,024 | expansion depth | root invocation | `parse.macro_recursion_limit` |
| Expanded bytes | 16,777,216 | 67,108,864 | UTF-8 bytes | source parse | `parse.macro_expanded_bytes_limit` |
| Invocations | 8,192 | 65,536 | expanded invocations | source parse | `parse.macro_invocation_limit` |

Configuration errors take precedence, followed by invocation, expanded-byte,
and recursion failures. Structured bootstrap fixtures execute independent
recursion, byte, and invocation limits plus exact provenance. The bootstrap
still normalizes those failures to `parse.invalid_syntax` and does not enforce
the target ceilings, so stable target codes and ceiling enforcement remain
blocked rather than being inferred from option names.

## Trivia ownership

The target owns line and documentation comments as retained syntax trivia with
source spans and attachment identity. The current bootstrap strips line-comment
text before parsing while preserving line coordinates. That coordinate proof
is useful but is not trivia ownership. Line-comment and doc-comment ownership
therefore remain separate target gaps.

## Same-binary runtime A/B

Runtime qualification requires two source inputs created after the candidate
artifact digest is recorded. The same exact artifact SHA-256 must process A and
B, producing distinct expected syntax or diagnostic envelopes. The artifact
digest is checked before and after each run, both input digests must be absent
from the artifact bytes, and an ordered run log binds artifact, input, and
output digests. Because no AxiOM-owned parser exists, the fixture records this
as a precise target gap and contains no fabricated artifact digest.

## Fuzz qualification

Promotion requires a checked seed manifest with SHA-256 integrity, deterministic
mutation seed 1471, a 1 MiB per-case input limit, and a 1,000 ms per-case time
limit. Oracles reject crashes, nontermination, nondeterministic diagnostics,
and out-of-checkout reads. Promotion requires zero crashes/timeouts, stable
diagnostic envelopes, and AxiOM/Rust seed parity. The seed manifest is
intentionally empty today, so fuzz coverage remains blocked.

## Differential and cutover proof

The AxiOM and Rust parsers must produce byte-identical normalized inspection
envelopes for the same artifact-bound corpus. A qualified package must then
disable the Rust syntax path without changing command behavior and prove the
rollback path. Cutover additionally requires the #1427 compiler-scale proof,
the #1473 diagnostics gate, both #1468 gates, and explicit proof for every
declared target gap: package ownership, runtime A/B, Unicode and identity
vectors, comment trivia, fuzzing, macro ceilings and stable failure codes,
recovered-node parity, differential coexistence, and Rust-path disablement.
Maintainer approval remains bound to the exact artifact. Failure remains
closed with `self_host.syntax_cutover_not_qualified`.

## Current floor

The current branch has executable structured bootstrap fixtures for the
conformance corpus, line-comment coordinate preservation, three independent
macro limits, macro provenance, repeated-parse bootstrap IDs, and ordered
recovery diagnostics. It does not have an AxiOM-owned syntax package,
same-binary runtime A/B, canonical IDs, recovered nodes, retained comment
trivia, a fuzz corpus, Unicode parity, differential coexistence, Rust-disable
proof, target macro codes/ceilings, or cutover approval. Readiness therefore
remains `syntax_only` / `blocked`.

## Validation

Run the focused checks with:

```bash
python3 scripts/ci/test-check-syntax-migration-v1.py
python3 scripts/ci/check-syntax-migration-v1.py --root "$PWD" --json
cargo test --manifest-path stage1/Cargo.toml -p axiomc --test syntax_migration_v1
make stage1-diagnostics-syntax-boundary
make stage1-diagnostics-syntax-boundary-test
make stage1-conformance
```
