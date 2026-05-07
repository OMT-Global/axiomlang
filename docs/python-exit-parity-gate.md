# Python Exit Parity Gate

Status: accepted

Parent issue: [#265](https://github.com/OMT-Global/axiom/issues/265)

Governing issue: [#266](https://github.com/OMT-Global/axiom/issues/266)

Final deletion issue: [#272](https://github.com/OMT-Global/axiom/issues/272)

This matrix defines the technical bar for treating Rust `stage1` as the sole
supported Axiom implementation. The final Python deletion issue is blocked until
this matrix has no `blocked` rows. Any future `blocked` row must link a child
issue in the row.

`make python-exit-readiness` is the machine-checkable deletion readiness gate.
It emits `axiom.python_exit.readiness.v1` JSON and fails if the parity matrix is
blocked, stage0 files are still tracked, CI still uses Python unittest as a
runtime correctness gate, user docs still advertise `python -m axiom`, or the
Rust-owned Makefile gates are missing.

Deletion PRs must use the strict issue-state variant:

```bash
make python-exit-readiness-github
```

That variant fails unless the issue states are available and Python-exit blocker
issues [#266](https://github.com/OMT-Global/axiom/issues/266) through
[#271](https://github.com/OMT-Global/axiom/issues/271) are all closed. Offline
automation can provide the same contract with
`scripts/ci/check-python-exit-readiness.sh --issue-state-file <path>
--require-issue-states`, where the state file contains one `<issue> <state>`
pair per line.

The verification snapshot for this matrix is:

- Legacy module help: no current Python package is checked in here; legacy module
  commands are inventoried below from the Python-exit issue set and disposition
  docs.
- `Makefile`: local validation is Rust-only through `docs-python-exit`,
  `stage1-test`, `stage1-conformance`, and `stage1-smoke`.
- `pyproject.toml`, `axiom/`, `tests/`: not present in this worktree.
- `docs/`: Python `stage0` is described only as retired, historical, or
  migration context.
- `stage1 axiomc`: supports `new`, `check`, `build`, `run`, `test`, and `caps`.

## Status Vocabulary

| Status | Meaning |
| --- | --- |
| `ported` | Rust `stage1` provides the same supported user-facing workflow or runtime behavior directly. |
| `replaced` | Rust `stage1` provides a different supported workflow for the user need. |
| `retired` | The Python surface is intentionally dropped and is not required for Python deletion. |
| `blocked` | Python deletion cannot proceed until the linked child issue lands and this row is reclassified. |

## Command And Runtime Matrix

| Python-facing surface | Status | Rust-only gate or disposition |
| --- | --- | --- |
| legacy module help | `replaced` | Supported command discovery is `axiomc --help`; the Rust command lists `new`, `check`, `build`, `run`, `test`, and `caps`. |
| `check` | `ported` | `axiomc check <package>` checks a stage1 package or workspace member; `--json` preserves a machine-readable diagnostic path. |
| `interp` | `retired` | There is no supported interpreter mode after Python exit; execute through `axiomc run <package>`. |
| `compile` | `replaced` | `axiomc build <package>` owns lowering, generated Rust emission, debug-map output when requested, and native binary creation. |
| `vm` | `retired` | The Python bytecode VM is not a compatibility target; runtime behavior is proven by Rust-owned tests and native execution. |
| `repl` | `retired` | No REPL is required for the Rust-only gate. A future REPL would be new Rust-owned product work, not Python-exit parity. |
| `pkg init` | `replaced` | `axiomc new <path>` creates `axiom.toml`, `axiom.lock`, and starter source. |
| `pkg build` | `ported` | `axiomc build <package>` builds packages and selected workspace members. |
| `pkg check` | `ported` | `axiomc check <package>` checks packages and selected workspace members. |
| `pkg run` | `ported` | `axiomc run <package>` builds and executes the generated native binary and returns its process status. |
| package tests | `replaced` | `axiomc test <package>` discovers, builds, runs, filters, and reports package test entrypoints; `--json` is the stable automation surface. |
| `pkg clean` | `retired` | Stage1 artifacts are ordinary package output under the manifest `out_dir`; remove that directory when a clean tree is needed. |
| `pkg manifest` | `replaced` | `axiom.toml` and `axiom.lock` are the supported metadata surfaces; `axiomc caps <package> --json` reports capability metadata. |
| `host list` | `retired` | Python host discovery is not part of the Rust-supported execution path. |
| `host describe` | `retired` | Future host or target inspection must be Rust-owned and tied to native build targets, not Python stage0 hosts. |
| Python bytecode compiler | `retired` | Rust lowering and generated-native builds replace bytecode compilation; no Rust bytecode compiler is required. |
| Python bytecode format | `retired` | Preserve only as historical material if retained at all; it is not a compatibility target. |
| Python bytecode VM | `retired` | No Rust port is required; supported runtime behavior must be covered by Rust tests, conformance fixtures, or generated-native execution. |
| Python disassembler | `retired` | Future inspection tools should target Rust-owned IR, generated Rust, debug maps, or a direct backend. |
| Python host builtins namespace | `replaced` | Stage1 uses explicit `std/` wrapper modules and compiler-known intrinsics with manifest capabilities instead of Python `host.*`. |
| Python package and loader internals | `replaced` | Stage1 package, workspace, local dependency, and import behavior is owned by Rust project tests and conformance fixtures. |
| Python test suite | `replaced` | Rust crate tests, `stage1/conformance`, `stage1/examples`, `make stage1-test`, `make stage1-conformance`, and `make stage1-smoke` are the supported regression gates. |

There are no `blocked` rows in the current matrix.

## Preserved Rust-Owned Behavior

Python deletion may proceed only while these Rust-owned behaviors stay covered:

- CLI help and command exits for `axiomc new`, `check`, `build`, `run`, `test`,
  and `caps`.
- Package manifests, lockfiles, local path dependencies, package-local modules,
  workspace member selection, and package test discovery.
- Machine-readable JSON success and error output for supported automation paths.
- Generated-native execution for supported stage1 programs, including stdout,
  stderr where exposed by stdlib, and process exit propagation.
- Stable public diagnostics covered by Rust checker tests or compile-fail
  conformance fixtures.
- Current stage1 language snapshot: imports, functions, constants, structs,
  enums, tuples, arrays, maps, borrowed slices, `Option<T>`, `Result<T, E>`,
  statement-level `match`, `if` / `else`, `while`, `return`, `print`, scalar
  comparisons, and `+` on `int` and `string`.
- Capability-gated runtime behavior for `clock`, `env`, `fs`, `net`, `process`,
  and `crypto`, plus the checked-in `std/` wrapper modules.

Coverage ownership is split across:

- [Python Exit Conformance](python-exit-conformance.md)
- [Python Exit VM Disposition](python-exit-vm-disposition.md)
- `stage1/crates/axiomc` Rust tests
- `stage1/conformance`
- `stage1/examples`

## Explicitly Dropped

The following are intentionally not part of the Rust-only parity bar:

- Python interpreter execution.
- Python bytecode compilation, bytecode decoding, bytecode VM execution, and
  bytecode disassembly.
- Python REPL behavior.
- Python `host.*` discovery and host builtin APIs.
- Python implementation-internal loader, semantic plan, and integer helper
  internals.
- Stage0-only language behavior listed as retired in
  [Python Exit Conformance](python-exit-conformance.md), including closures,
  nested functions, first-class function values, Python import aliases,
  namespace-qualified calls, and retired mutable array helper APIs.

## Deletion Rule

[#272](https://github.com/OMT-Global/axiom/issues/272) may delete Python
implementation files only after this matrix still has no `blocked` rows at PR
time, `make test` passes, and user-facing docs continue to avoid presenting
Python `stage0` as a supported execution path.
