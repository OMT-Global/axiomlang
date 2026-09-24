# Executable MIR v0 inspection slice

`axiomc::project::executable_mir_packages` exposes the first explicit,
backend-neutral executable MIR boundary for inspected packages.  The model is
deliberately small while the existing Cranelift compatibility lowering remains
the production build path.

The v0 slice represents:

- scalar `int` and `bool` values;
- scalar parameters, locals, assignments, arithmetic, comparisons, and logic;
- direct scalar function calls;
- runtime stdin length through `read_to_string()`;
- basic blocks with terminal returns and conditional branches; and
- source spans on functions, blocks, instructions, and terminators.

Callers receive `executable_mir.unsupported` when a package is outside this
slice.  The API never returns a partial program and does not alter normal build
output.  This makes the model suitable for snapshots and backend migration
work while preserving the existing compatibility path.

The next implementation slice can make Cranelift consume this model directly
after the explicit block and effect contracts have independent backend
coverage.
