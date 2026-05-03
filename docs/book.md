# The Axiom Book

This book is the tutorial path for the Rust-only stage1 toolchain. It teaches
the current supported Axiom surface from first package to agent-oriented
programs, while clearly marking future language work as roadmap material.

## 1. Install And Run

Clone the repo and use the Rust bootstrap compiler:

```bash
git clone https://github.com/OMT-Global/axiom.git
cd axiom
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/hello --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/hello
```

## 2. A First Program

```axiom
fn greet(name: string): string {
    return "hello, " + name
}

print greet("axiom")
```

A package is described by `axiom.toml`, locked by `axiom.lock`, and built with
`axiomc build`.

## 3. Packages And Modules

Use package-local imports for multi-file programs:

```axiom
import "math.ax"

print add(20, 22)
```

See `stage1/examples/modules`, `stage1/examples/packages`, and
`stage1/examples/workspace` for the current package graph.

## 4. Types

Stage1 currently supports scalar types, structs, enums, tuples, arrays, maps,
borrowed slices, `Option<T>`, and `Result<T, E>`. Generic structs, generic enums,
and generic functions require explicit type arguments today.

## 5. Control Flow

Use `if` / `else`, `while`, `return`, and statement-level `match`. Match
exhaustiveness is checked for enum variants.

## 6. Capabilities

Runtime effects are manifest-gated. A package that imports `std/fs.ax`,
`std/net.ax`, `std/process.ax`, `std/env.ax`, `std/time.ax`,
`std/crypto_hash.ax`, or `std/crypto_mac.ax` must declare the matching
capability in `axiom.toml`.

Inspect capabilities with:

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- caps stage1/examples/capabilities --json
```

## 7. Tests And Conformance

Package tests are `src/*_test.ax` files and can use sibling stdout golden files. `std/testing.ax` adds table-case, property, and snapshot assertion helpers for richer package tests.

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/modules --json
make stage1-conformance
```

## 8. Tooling

The bootstrap toolchain includes formatting, docs, benchmarks, and a scratch
REPL:

```bash
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- fmt stage1/examples/hello --check
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- doc stage1/examples/hello
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- bench stage1/examples/benchmarks --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- repl
```

## 9. Roadmap

The roadmap lives in `docs/roadmap.md` and issue #264. The largest remaining
gaps are traits, mutable references, richer diagnostics, real async I/O,
publisher/registry flows, LSP support, direct native codegen, and agent-native
typed tool calls.
