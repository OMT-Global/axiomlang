# Python Exit Conformance

Stage1 owns the Rust-run conformance corpus at `stage1/conformance`.

Run it with:

```sh
make stage1-conformance
```

## Fixture Layout

Executable fixtures live under `stage1/conformance/pass`. Each fixture is a
complete package and includes `expected-output.txt`; the runner compiles and
executes the generated native binary, then checks stdout.

Compile-fail fixtures live under `stage1/conformance/fail`. Each fixture is a
complete package and includes `expected-error.json`; the runner checks the
diagnostic kind, code, exact message, relative source path, line, and column.

## Python Test Module Migration

| Retired Python test module | Stage1 destination or disposition |
| --- | --- |
| `tests/test_conformance.py` | Replaced by `make stage1-conformance` and the checked-in `stage1/conformance/{pass,fail}` corpus. |
| `tests/test_cli_runtime.py` | Runtime execution parity moved to `axiomc test` and `axiomc run` coverage under `stage1/conformance`, `stage1/examples`, and Rust project tests. REPL, interpreter, bytecode compiler, and VM checks are retired by `docs/python-exit-vm-disposition.md`. |
| `tests/test_cli_packages.py` | Replaced by Rust stage1 package/workspace tests and `stage1/examples/{packages,workspace,workspace_only}`. Python `pkg` subcommands are retired by `docs/python-exit-vm-disposition.md`. |
| `tests/test_errors_core.py` | Replaced by Rust checker tests plus compile-fail conformance fixtures for stable public diagnostics, including `stage1/conformance/fail/parser_type_return_mismatch` for migrated parser/type return checking (#367). |
| `tests/test_errors_imports.py` | Replaced by Rust import/package tests and compile-fail conformance fixtures for supported stage1 import behavior. Python import aliases and namespace-qualified calls are retired because stage1 rejects them. |
| `tests/test_errors_runtime.py` | Runtime behavior that remains supported is covered by stage1 generated-native execution tests. Python bytecode, VM, interpreter, and host builtin internals are retired. |
| `tests/test_bytecode.py` | Retired. The Python bytecode format, decoder, and VM are not supported compatibility targets after Python exit. |
| `tests/test_intops.py` | Retired with Python integer helper internals; stage1 does not currently expose a division operator contract. |
| `tests/test_loader.py` | Replaced by Rust stage1 import/package resolution tests and conformance import fixtures. Python loader internals are retired. |
| `tests/test_semantic_plan.py` | Retired as Python implementation-internal coverage; supported semantic behavior moved to Rust checker tests and conformance fixtures. |
| `tests/test_detect_secrets_script.py` | Kept outside language conformance; the current shell gate is `scripts/check-detect-secrets.sh`. |

## Golden Program Migration

| Retired golden program group | Stage1 destination or disposition |
| --- | --- |
| `arith`, `bool_values`, `if_else`, `string_concat`, `string_escape`, `array_basic`, `array_len` | Ported into `stage1/conformance/pass/legacy_core_programs`. |
| `fn_basic`, `fn_recursive`, `array_fn`, `array_strings`, `array_while`, `vars` | Covered by existing stage1 examples, Rust project tests, and conformance fixtures for functions, arrays, and while execution; `stage1/conformance/pass/parser_type_slice` adds the #367 parser/type migration slice for aliases, borrowed slices, enum payload patterns, and typed aggregate literals. |
| `assign`, `assign_outer`, `scopes`, `fn_scope`, `expr_stmt`, `let_infer`, `array_empty`, `array_neg_index`, `div`, `while_sum` | Either covered by the Rust checker/runtime suite or obsolete where stage1 intentionally differs, such as no standalone expression statement contract, no empty array literal support, no mutable assignment statement contract, and no current division operator contract. |
| `array_push`, `array_set`, `string_builtins`, `math_builtins`, `fn_host_abs`, `fn_host_math_abs`, `fn_host_version` | Retired with Python host builtin APIs. Stage1 uses explicit stdlib modules and compiler-known intrinsics instead of the Python `host.*` namespace. |
| `fn_closure_capture`, `fn_closure_recursive`, `fn_closure_shadow`, `fn_nested`, `fn_first_class_basic`, `fn_first_class_param`, `fn_first_class_reassign` | Retired as stage0-only function model coverage. Stage1 does not support closures, nested functions, or first-class function values. |
| `for_basic`, `for_nested` | Retired until a Rust-owned `for` statement exists; current stage1 iteration coverage uses `while`, arrays, and borrowed-slice helpers. |

## Current Stage1 Coverage

Executable conformance fixtures currently cover migrated legacy core programs,
functions across modules, nested package-local imports, struct field access,
`Option`/`Result` control flow, and standard collection helpers over arrays and
borrowed slices. Issue #367 adds `parser_type_slice` and `parser_type_return_mismatch` as the first narrow parser/type migration slice linked to this Python-exit tracker.

Compile-fail fixtures currently cover use-after-move ownership, mutable borrow
while a shared borrow is live, `Ok(...)` without a `Result<T, E>` context, and
clock stdlib access without the required capability.
