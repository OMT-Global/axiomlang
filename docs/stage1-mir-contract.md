# Stage1 typed MIR contract

The stage1 compiler lowers syntax to HIR and then to typed MIR (axiomc::mir) before Rust code generation. MIR is the stable internal contract used by compiler passes, diagnostics, snapshots, and downstream tooling that need to inspect the compiler understanding of an Axiom program.

## Contract shape

A MIR Program contains package path metadata plus typed declarations and top-level statements:

- structs: names and typed fields.
- enums: names, variants, ordered payload types, and named payload labels when present.
- statics: name, declared type, and lowered initializer expression.
- functions: canonical name, source name, source path, typed parameters, typed return, body, async/extern metadata, and definition span.
- stmts: top-level lowered statements.

Every expression variant that can be consumed by later passes carries or computes a Type through Expr::ty(). The MIR type algebra covers primitive values, owned strings, borrowed strings, structs, enums, pointers, slices, options/results, tuples, maps, arrays, async/task handles, channels, select results, and function types.

## Required lowering coverage

The typed MIR contract explicitly covers the constructs needed by the stage1 roadmap slice:

- Locals: Stmt::Let { name, ty, expr, span } preserves the declared/resolved local type and source span.
- Moves: ownership-moving projections lower as typed expressions such as Expr::FieldAccess assigned into typed Stmt::Let bindings; ownership legality remains enforced before MIR consumers run.
- Borrows: borrowed views lower to typed slice/pointer forms (Type::Slice, Type::MutSlice, Type::Ptr, Type::MutPtr) with borrow-producing expressions such as Expr::Slice and Expr::StringBorrow.
- Calls: Expr::Call { name, args, ty } records the resolved callable name, lowered arguments, and result type.
- Branches: Stmt::If and Stmt::While carry typed condition expressions and recursively lowered statement blocks.
- Match: Stmt::Match carries the typed scrutinee expression plus ordered MatchArm entries, variant names, payload bindings, named/positional payload mode, ignored-payload state, and lowered arm bodies.
- Return: Stmt::Return { expr, span } carries the typed returned expression and source span.

## Serialization contract

MIR derives serde::Serialize and is covered by normalized JSON snapshots in stage1/crates/axiomc/tests/mir_snapshots. Snapshot tests protect the JSON shape of representative examples. Unit tests in axiomc::mir additionally assert that locals, moves, borrows, calls, branches, match, and return all lower into typed MIR variants with stable serialized shapes.

MIR currently serializes Rust enum variants using serde externally tagged representation. Consumers should treat that representation as the stage1 inspection format until a versioned public schema is introduced.
