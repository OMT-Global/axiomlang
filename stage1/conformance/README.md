# Stage1 Conformance

Run the Rust-owned conformance corpus with:

```sh
make stage1-conformance
```

Packages under `pass/` are executable fixtures. Each package is a complete
stage1 project with `axiom.toml`, `axiom.lock`, source, and
`expected-output.txt`. The conformance runner compiles each discovered
`src/**/*_test.ax` target through the Rust path, executes the generated binary,
and compares stdout to the package-level expected output.

Current executable fixtures cover:

- `legacy_core_programs`: migrated golden-program coverage for integer
  addition, bools, `if/else`, `while false`, string concat/escapes, array
  indexing, and array length.
- `functions_across_modules`: function calls and return values imported from a
  sibling module.
- `struct_field_access`: struct construction, field access, and passing a
  struct through a function.
- `outcome_control_flow`: `Option` and `Result` construction plus `match`
  control flow.
- `collection_operations`: standard collection helpers over arrays and
  borrowed slices.
- `package_local_modules`: nested package-local module imports that execute
  successfully.
- `package_visibility`: `pub(pkg)` items imported across sibling modules within
  the same package.
- `comparison_strict_typing`: Axiom-owned comparison fixture for explicit
  scalar typing and bool-only control flow.
- `comparison_package_imports`: Axiom-owned comparison fixture for package-local
  imports and typed function boundaries.

Packages under `fail/` are compile-fail fixtures. Each package is a complete
stage1 project with `axiom.toml`, `axiom.lock`, source, and
`expected-error.json`. The conformance runner checks the diagnostic kind, code,
message, relative path, line, and column.

Current compile-fail fixtures cover:

- `mutable_borrow_while_shared_live`: ownership diagnostics for conflicting
  mutable and shared borrows.
- `ownership_use_after_move`: ownership diagnostics for reading a moved value.
- `panic_rejects_unreachable_statement`: control diagnostics for statements
  that appear after `panic(...)` in the same block.
- `panic_rejects_multiple_arguments`: type diagnostics for `panic(...)` when
  the call supplies more than one message argument.
- `panic_requires_single_argument`: type diagnostics for `panic(...)` when the
  call arity is not exactly one argument.
- `panic_requires_string_argument`: type diagnostics for `panic(...)` when the
  message is not a `string`.
- `panic_rejects_type_arguments`: type diagnostics for `panic(...)` when the
  statement incorrectly supplies type arguments.
- `result_ok_without_context`: type diagnostics for `Ok(...)` without an
  expected `Result<T, E>` context.
- `stdlib_clock_without_capability`: capability diagnostics for clock
  intrinsics without the manifest opt-in.
- `package_visibility_dependency_boundary`: import diagnostics for `pub(pkg)`
  items that are referenced across a dependency package boundary.
- `recursive_struct_without_indirection`: type diagnostics for direct
  self-recursive struct fields without an indirection boundary.
- `recursive_mutual_struct_without_indirection`: type diagnostics for
  mutually recursive struct fields without an indirection boundary.
- `recursive_struct_enum_without_indirection`: type diagnostics for recursive
  struct-enum cycles without an indirection boundary.
- `recursive_enum_without_indirection`: type diagnostics for direct
  self-recursive enum payloads without an indirection boundary.
- `match_guard_not_supported`: parse diagnostics for unsupported `if` guard
  clauses on match arms.
- `named_nested_match_pattern_not_supported`: parse diagnostics for
  unsupported nested destructuring inside named match patterns.
- `nested_match_pattern_not_supported`: parse diagnostics for unsupported
  nested destructuring inside match patterns.
- `comparison_owned_resource_move`: Axiom-owned comparison fixture for
  predictable ownership diagnostics when a non-copy value is used after move.
- `comparison_predictable_diagnostic`: Axiom-owned comparison fixture for a
  stable type diagnostic with exact source location.
