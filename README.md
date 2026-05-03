# Axiom

Axiom is a small experimental programming language. The supported toolchain is
the Rust bootstrap compiler in `stage1/`.

Python `stage0` and its bytecode VM are not supported execution paths; see
[docs/python-exit-vm-disposition.md](docs/python-exit-vm-disposition.md) and
[docs/python-exit-parity-gate.md](docs/python-exit-parity-gate.md).

## Current Status

Axiom currently supports a Rust-only `axiomc` workflow with:

- `axiom.toml` and `axiom.lock` package manifests.
- Package-local modules, local path dependencies, and workspace member
  selection.
- Native builds through generated Rust and `rustc`.
- `check`, `build`, `run`, `test`, and capability inspection commands.
- A stage1 conformance corpus under `stage1/conformance`.
- Synthetic standard library modules under the `std/` import prefix.

Use `cargo run --manifest-path stage1/Cargo.toml -p axiomc -- ...` or the Make
targets below.

## Example

```axiom
fn greet(name: string): string {
  return "hello, " + name
}

let ready: bool = true

if ready {
  print greet("axiom")
}
```

## Quickstart

```bash
# Clone
git clone https://github.com/OMT-Global/axiom.git
cd axiom

# Check a package
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/hello --json

# Build a native binary
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/hello --json

# Inspect build cache and compile timing
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/hello --timings

# Run a package
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/hello

# Run discovered package tests
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/modules --json

# Inspect capability requirements
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- caps stage1/examples/hello --json

# Publish to a local static registry tree and build/validate its index
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- publish stage1/examples/hello --registry-dir ./registry/packages --signing-key dev-key
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- registry-index ./registry/packages --base-url https://packages.example.test --out ./registry/index.json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- registry-validate ./registry/index.json

# Format source, generate docs, and run benchmark entrypoints
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- fmt stage1/examples/hello --check
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- doc stage1/examples/hello
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- bench stage1/examples/benchmarks --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/stdlib_testing --include-benchmarks --json
cargo run --manifest-path stage1/Cargo.toml -p axiomc -- lsp
```

## Useful Commands

```bash
# Rust crate tests
make stage1-test

# Rust conformance corpus
make stage1-conformance

# Rust smoke workload
make stage1-smoke

# Default local validation
make test
make smoke
```

## Language Snapshot

The current stage1 compiler supports top-level imports, functions, constants,
structs, enums, tuple types, arrays, maps, borrowed slices, `Option<T>`,
`Result<T, E>`, statement-level `match`, `if` / `else`, `while`, `return`,
`print`, scalar comparisons, and `+` on `int` and `string`.

Stage1 also enforces the current capability-gated runtime surface for `clock`,
`env`, `fs`, `net`, `process`, and `crypto`, with stdlib wrappers in
`std/time.ax`, `std/env.ax`, `std/fs.ax`, `std/net.ax`, `std/process.ax`,
`std/crypto_hash.ax`, and `std/crypto_mac.ax`. Additional ungated or
shared-capability wrappers live in
`std/io.ax`, `std/json.ax`, `std/collections.ax`, `std/sync.ax`,
`std/async.ax`, and `std/http.ax`.
The `std/net.ax` socket floor is deliberately bounded to one-shot loopback TCP
and UDP helpers under `[capabilities].net` so examples and tests stay
deterministic and avoid external network access.

See [docs/grammar.md](docs/grammar.md), [docs/kernel.md](docs/kernel.md),
[docs/stage1.md](docs/stage1.md), and
[docs/stage1-lsp.md](docs/stage1-lsp.md) for more detail.
Start with [docs/book.md](docs/book.md) for the tutorial path and
[docs/style.md](docs/style.md) for canonical source style.
See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution workflow and validation
expectations.

## Repo Map

- `stage1/crates/axiomc/`: Rust compiler, CLI, manifest, diagnostics, HIR/MIR,
  stdlib, and the current generated-Rust backend; native backend expansion
  beyond generated Rust remains future work, and this backend plumbing is only preparatory
  groundwork (part of #105).
- `stage1/examples/`: checked-in package examples for language, package,
  workspace, stdlib, and capability behavior.
- `stage1/conformance/`: Rust-run pass/fail conformance fixtures.
- `docs/`: language, package, bootstrap, and roadmap documentation.
- `scripts/ci/`: local and CI validation entrypoints.
