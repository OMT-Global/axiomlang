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

Fixtures may also declare explicit `[[tests]]` entries in `axiom.toml`.
Manifest test entries support `name`, `entry`, `stdout`, `expected_error`,
`capabilities`, and `package` metadata. `axiomc test --json` reports those
contracts on each discovered case so agents can inspect the fixture intent
without reading sidecar files first.

Current executable fixtures cover:

- `legacy_core_programs`: migrated golden-program coverage for integer
  addition, bools, `if/else`, `while false`, string concat/escapes, array
  indexing, and array length.
- `functions_across_modules`: function calls and return values imported from a
  sibling module.
- `struct_field_access`: struct construction, field access, and passing a
  struct through a function.
- `outcome_control_flow`: `Option` and `Result` construction plus `match`
  and `if let` control flow, including ignored fallback payloads.
- `collection_operations`: standard collection helpers over arrays and
  borrowed slices.
- `ownership_borrowing`: borrow-safe parameter, aggregate, borrowed-return,
  projection move, and dependency-boundary execution.

- `comparison_package_imports`: Axiom-owned Go/Rust-style comparison fixture
  for explicit package imports and cross-module function calls.
- `comparison_package_resources`: Axiom-owned Go/Rust-style comparison fixture
  for explicit package imports, strict struct typing, owned resource transfer,
  borrowed slices, and machine-checkable result output.
- `comparison_strict_typing`: Axiom-owned Go/Rust-style comparison fixture
  proving strict struct field typing in passing programs.
- `package_local_modules`: nested package-local module imports that execute
  successfully.
- `package_visibility`: `pub(pkg)` items imported across sibling modules within
  the same package.
- `type_system_aggregates`: typed aggregate coverage for generic wrappers,
  structs, enums, tuples, arrays, maps, `Option`, and `Result`.
- `parser_type_slice`: migrated parser/type coverage for type aliases,
  borrowed slices, enum payload patterns, typed aggregate literals, and
  return-type checking through parsed control flow.
- `runtime_negative_diagnostics`: executable negative runtime coverage for
  structured `panic(...)` diagnostics and array bounds runtime diagnostics.
- `declarative_macros`: macro keyword syntax, nested expansion, repeated
  expression captures, and introduced-local hygiene before type-check.

Packages under `fail/` are compile-fail fixtures. Each package is a complete
stage1 project with `axiom.toml`, `axiom.lock`, source, and
`expected-error.json`. The conformance runner checks the diagnostic kind, code,
message, relative path, line, and column.

Current compile-fail fixtures cover:

- `closure_move_captured_non_copy`: ownership diagnostics for `fn` closures
  whose body consumes a captured non-copy value.
- `closure_captures_function_callee`: ownership diagnostics for closures
  that capture a function-valued callee and move it into a later closure.
- `import_cycle`: import diagnostics for circular module references.
- `import_duplicate_export`: import diagnostics for colliding public exports
  from sibling modules.
- `import_missing_module`: import diagnostics for missing package-local modules.
- `import_path_escape`: import diagnostics for parent-directory traversal.
- `import_reserved_namespace`: import diagnostics for incomplete `std`
  namespace imports.
- `import_unsupported_alias`: parse diagnostics for unsupported import aliases.
- `mutable_borrow_while_shared_live`: ownership diagnostics for conflicting
  mutable and shared borrows.
- `comparison_owned_resource_move`: Axiom-owned Go/Rust-style comparison
  diagnostic for an owned resource consumed by a function and then reused.
- `comparison_predictable_diagnostic`: Axiom-owned Go/Rust-style comparison
  diagnostic for predictable error message shape.
- `comparison_strict_type_mismatch`: Axiom-owned Go/Rust-style comparison
  diagnostic for strict struct field typing.
- `parser_type_return_mismatch`: migrated parser/type diagnostic coverage for
  return expressions whose parsed value type disagrees with the declared
  function return type.
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
- `generic_struct_constructor_extra_type_args`: type diagnostics tied to the
  constructor path for explicit generic struct constructor type arguments with
  invalid arity.
- `generic_struct_constructor_missing_type_args`: type diagnostics tied to the
  constructor path when a generic struct constructor spelling supplies too few
  explicit type arguments.
- `generic_struct_constructor_mismatched_type_args`: type diagnostics for generic
  struct constructors whose payload does not match the contextual type argument.
- `generic_tuple_enum_constructor_type_args`: type diagnostics for tuple enum
  constructors that incorrectly supply explicit type arguments; named-payload
  enum constructors are covered through contextual generic validation because
  the current named-literal surface has no valid explicit `Variant<T> { ... }`
  form that reaches HIR lowering.
- `generic_named_enum_constructor_missing_type_args`: type diagnostics tied to
  the named-payload constructor path when the current syntax surface sees an
  explicitly generic variant constructor spelling.
- `generic_named_enum_constructor_mismatched_type_args`: type diagnostics for
  named-payload enum constructors whose payload does not match the contextual
  type argument.
- `result_ok_without_context`: type diagnostics for `Ok(...)` without an
  expected `Result<T, E>` context.
- `stdlib_clock_without_capability`: capability diagnostics for clock
  intrinsics without the manifest opt-in.
- `stdlib_fs_write_without_capability`: capability diagnostics for write-side
  filesystem helpers without the manifest opt-in.
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
- `macro_missing_argument`: parse diagnostics for a macro call that omits a
  required captured argument.
- `macro_recursion_limit`: parse diagnostics for recursive declarative macro
  expansion bounded by the default recursion limit.
- `named_nested_match_pattern_not_supported`: parse diagnostics for
  unsupported nested destructuring inside named match patterns.
- `nested_match_pattern_not_supported`: parse diagnostics for unsupported
  nested destructuring inside match patterns.
- `comparison_owned_resource_move`: Axiom-owned comparison fixture for
  predictable ownership diagnostics when a non-copy value is used after move.
- `comparison_predictable_diagnostic`: Axiom-owned comparison fixture for a
  stable type diagnostic with exact source location.
