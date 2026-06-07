# Compiler Diagnostics and Syntax Boundary

`compiler.diagnostics` and `compiler.syntax` are the first self-hosted compiler
packages to freeze because every later package migration depends on stable
source parsing and stable error reporting.

The current implementation still lives in Rust under `diagnostics.rs`,
`diagnostic_catalog.rs`, `syntax.rs`, and LSP diagnostic adapters. Those files
are scaffolding. They do not define the public contract for diagnostics or
syntax once AxiOM-owned packages replace them.

## Package APIs

`compiler.syntax` owns these entrypoints:

- `lex_source(source, path)`: tokenize source text while preserving comments as
  trivia boundaries for source-span recovery. The v1 public contract does not
  expose Rust token names.
- `parse_program(source, path, options)`: parse one source file into package
  syntax nodes and macro expansion records.
- `parse_program_with_recovery(source, path, options)`: return the first
  diagnostic plus related recovered diagnostics when top-level recovery can
  continue.
- `parse_macro_definitions(source, path, options)`: collect top-level `macro`
  and compatibility `macro_rules!` definitions.
- `expand_macros(program, options)`: expand the stage1 macro subset and emit
  deterministic macro expansion records.

`compiler.diagnostics` owns these entrypoints:

- `diagnostic(kind, message, span, code, repair, related)`: construct the
  stable diagnostic envelope.
- `normalize_code(kind, message, explicit_code)`: derive the public diagnostic
  code when a diagnostic did not supply one explicitly.
- `explain(code)`: return catalog metadata for stable diagnostic codes.
- `to_lsp_range(diagnostic)`: translate source spans into protocol ranges
  without changing the compiler diagnostic envelope.

## Diagnostic Envelope

The v1 diagnostic envelope is AxiOM-owned and keeps these fields stable:

- `kind`: broad category such as `parse`, `type`, `ownership`, `capability`,
  `manifest`, `import`, `control`, `build`, or `codegen`.
- `code`: optional stable machine-readable code. Parser diagnostics must use
  the `parse.*` namespace; implementation helper names are not valid codes.
- `message`: human-readable diagnostic message.
- `path`: source path when known.
- `line` and `column`: one-based start position.
- `end_line` and `end_column`: optional one-based end position. These are
  additive and must not replace the start-position compatibility contract.
- `related`: optional additional diagnostics discovered through recovery.
- `repair`: optional structured repair metadata when a future repair package
  produces safe edit hints.

Parser recovery must preserve the first diagnostic as the primary failure and
attach subsequent recovered top-level diagnostics under `related`.

## Syntax Contract

The syntax package owns source concepts, not Rust parser helper names. Public
syntax terms are the grammar concepts in [Axiom grammar](grammar.md), including
program, item, macro item, import item, declaration, statement, expression,
type, pattern, block, and macro expansion.

Macro expansion records are part of the syntax contract because command JSON,
inspection, and future repair tooling need to map expanded source back to call
sites. The stable v1 record includes:

- `macro_name`
- `call_site.path`
- `call_site.line`
- `call_site.column`
- `expanded_line_start`
- `expanded_line_end`

## Rust Capture Rules

- Do not expose Rust enum variant names, struct names, module paths, or parser
  helper function names as stable diagnostic codes.
- Do not define AxiOM syntax in terms of Rust token enums.
- Do not require Cargo, `rustc`, or generated Rust to parse source or explain a
  diagnostic.
- Keep LSP ranges as protocol adapters over the diagnostic envelope, not the
  canonical diagnostic representation.

## Fixture and Validation

The checked contract fixture lives at:

- `stage1/compiler-contracts/schemas/axiom.compiler.diagnostics_syntax.v1.schema.json`
- `stage1/compiler-contracts/snapshots/diagnostics-syntax.json`

The local validator is:

```bash
make stage1-diagnostics-syntax-boundary
```

It checks that the snapshot satisfies the declared schema, the referenced
`check --json` fixtures exist, parser diagnostics keep start and end spans,
stable parse codes remain in the public `parse.*` namespace, and the macro
expansion fixture keeps source-correlated call-site fields.
