# Axiom kernel

This kernel note describes the supported Rust `axiomc` path.

## Values

- Scalars: `int`, `string`, and `bool`.
- Aggregates: structs, enums, tuples, arrays, maps, borrowed slices,
  `Option<T>`, and `Result<T, E>`.
- Conditions are bool-only.
- Comparisons produce booleans.
- `print` renders booleans as `true` / `false`.

## Statements

- `import "<path>"` for package-local, dependency-prefixed, or `std/` modules.
- `pub const`, `const`, `pub type`, and `type` declarations.
- `pub struct`, `struct`, `pub enum`, and `enum` declarations.
- `pub fn` and `fn` declarations.
- `let <ident>: <type> = <expr>` and inferred `let <ident> = <expr>`.
- `print <expr>`, `return <expr>`, `if` / `else`, `while`, and statement-level
  `match`.

## Execution

- Packages are checked, built, run, and tested through `axiomc`.
- `axiomc build` currently generates Rust and invokes `rustc` to produce a native binary.
- The backend selection surface is preparatory seam work for later native-backend expansion; today only `generated-rust` is implemented, so this is not completion of #105 (part of #105).
- `axiomc test` discovers `src/**/*_test.ax` entrypoints and compares stdout
  with sibling `*.stdout` files when present.
- `axiomc check --json`, `build --json`, `test --json`, and `caps --json` emit
  the versioned `axiom.stage1.v1` schema envelope.

## Capabilities

Runtime capabilities are declared in `axiom.toml` and enforced before native
execution. Current capability classes are `clock`, `env`, `fs`, `net`,
`process`, and `crypto`.

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- caps stage1/examples/hello --json
```

The `caps` command reports the package capability surface in machine-readable
form. Standard library wrappers preserve manifest enforcement for importing
packages.
