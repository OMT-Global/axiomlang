# Axiom Style Guide

This document defines the canonical source style for checked-in `.ax` files.
Use the formatter as the default enforcement path, and treat this guide as the
readable statement of the layout it is meant to preserve.

## Formatting

Run the formatter before review:

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- fmt <path>
```

Use `--check` in CI or before pushing:

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- fmt stage1/examples/hello --check
```

The formatter currently enforces the bootstrap-safe rules that are stable across
the parser and examples:

- Unix newlines.
- No trailing whitespace.
- Tabs expanded to four spaces.
- A single final newline.
- No repeated blank-line runs.

## Core style rules

- Use spaces, never tabs in checked-in source.
- Keep keywords lowercase (`fn`, `let`, `match`, `return`, `pub`).
- Put a single space after commas and around infix operators.
- Put the opening `{` on the same line as `fn`, `if`, `while`, `match`,
  `struct`, and `enum` headers.
- Prefer one statement per line.
- End files with a trailing newline.

## Source shape

- Prefer package-local modules over large `main.ax` files.
- Keep public declarations small and documented with `///` when they are meant
  to appear in generated docs.
- Use explicit type annotations for public constants, function parameters, and
  return values.
- Keep capability use visible in `axiom.toml`; do not hide filesystem, network,
  process, environment, clock, or crypto behavior behind helper modules without
  documenting it.
- Prefer environment allowlists such as `env = ["LOG_LEVEL"]`. If migration work
  must use `env_unrestricted = true`, include `unsafe_rationale` in the same
  `[capabilities]` table so reviewers can audit the unsafe grant.

## Imports

- Group imports at the top of the file.
- Keep one `import` per line.
- Sort imports lexicographically within a group.
- Leave one blank line between the import block and the first item or statement.

```axiom
import "core/banner.ax"
import "core/math.ax"

print banner("hello", label())
```

## Functions and control flow

- Use concise names that describe the value or operation.
- Add explicit type annotations where the language requires them; do not add
  redundant commentary around obvious types.
- Keep short function signatures on one line when they fit.
- Break after the header only when the signature becomes hard to scan.

```axiom
fn banner(name: string): string {
    return "hello " + name
}

if ready {
    print banner("axiom")
} else {
    print "not ready"
}

match result {
    Some(value) {
        print value
    }
    None {
        print "missing"
    }
}
```

## Data declarations

- Keep struct and enum fields one per line.
- Prefer trailing comments on their own line above the declaration instead of at
  the end of a field line.
- Use compact inline literals only when the whole value is still easy to read.
- Expand literals across lines when they grow beyond a short handful of fields
  or arguments.

```axiom
struct Pipeline {
    name: string
    steps: int
    ready: bool
}

let pipeline: Pipeline = Pipeline {
    name: "stage1",
    steps: 3,
    ready: true,
}
```

## Comments and docs

- Use `#` comments sparingly for intent, invariants, or temporary limitations.
- Prefer explaining why a constraint exists instead of narrating the code.
- Keep comments updated when behavior changes.
- Use doc comments on public APIs when they should appear in generated docs.

```axiom
/// Returns the display name for a job.
pub fn label(name: string): string {
    return "job:" + name
}
```

Then generate docs with:

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- doc <package>
```

## Tests and checked-in examples

- Put runnable package tests in `src/*_test.ax`.
- Add sibling `*.stdout` files for golden-output checks.
- Add compile-fail language coverage under `stage1/conformance/fail/`.
- Apply this style to checked-in examples, conformance fixtures, RFC snippets,
  and README/docs code blocks.
- When existing fixtures use older formatting, clean them opportunistically in
  the same change only if the diff stays easy to review.
- Do not mix formatting-only churn into unrelated feature work.
