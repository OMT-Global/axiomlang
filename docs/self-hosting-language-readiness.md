# Self-Hosting Language Readiness

This checklist defines the minimum AxiOM language and direct-native backend
surface required before the `axiomc` compiler rewrite can move from planning to
implementation.

It is intentionally stricter than "the examples pass." A row is ready only when
it has checked evidence, a validating command, and no open blocker issue. The
machine-readable source is
[`docs/self-hosting-language-readiness.json`](self-hosting-language-readiness.json);
this page explains how to read that manifest.

## Readiness Command

Run the local gate with:

```bash
make self-hosting-language-readiness
```

It emits `axiom.self_hosting.language_readiness.v0` JSON and fails while any
required row is not `implemented` or while row evidence is missing. For release
or rewrite-decision PRs, require live issue state so open blocker issues also
keep the gate red:

```bash
make self-hosting-language-readiness-github
```

The self-hosting gate is a prerequisite for the Rust bootstrap exit in
[#721](https://github.com/OMT-Global/axiomlang/issues/721) and the broader
self-hosting track in [#565](https://github.com/OMT-Global/axiomlang/issues/565).
It does not authorize removing Rust by itself; it answers only whether the
language and backend surface are adequate to start the compiler rewrite.

## Current Matrix

| Row | Required surface | Current status | Governing issue |
| --- | --- | --- | --- |
| `error_handling_try` | Option/Result propagation and diagnostics for recoverable compiler errors. | Implemented for current stage1 and direct-native evidence. | [#1256](https://github.com/OMT-Global/axiomlang/issues/1256) |
| `compiler_data_shapes` | Scalars, tuples, arrays, maps, structs, enums, `Option`, `Result`, and ownership-sensitive aggregate movement. | Implemented for the current checklist scope. | [#1251](https://github.com/OMT-Global/axiomlang/issues/1251) |
| `generics_traits_static_dispatch` | Explicit generics and static trait-bounded dispatch for reusable typed helpers. | Implemented for static dispatch; dynamic dispatch is intentionally not required by this row. | [#216](https://github.com/OMT-Global/axiomlang/issues/216) |
| `strings_diagnostics_and_text` | String building, byte inspection, substring search, JSON, regex, logging, and deterministic text processing for lexer/parser/diagnostics work. | Implemented for the current checklist scope; the diagnostics spike uses direct `string_contains` evidence. | [#1256](https://github.com/OMT-Global/axiomlang/issues/1256) |
| `host_io_capabilities` | Scoped filesystem, environment, clock, process, networking, crypto, JSON, and regex surfaces through capability-gated stdlib modules. | Implemented for scoped direct-native stdlib use. | [#1251](https://github.com/OMT-Global/axiomlang/issues/1251) |
| `packages_modules_and_workspace` | Package-local modules, local dependencies, workspaces, and deterministic lockfile validation. | Implemented as a language/package surface; source migration remains separate. | [#721](https://github.com/OMT-Global/axiomlang/issues/721) |
| `compiler_command_surface` | AxiOM-owned check/build/run/test/doc/LSP-facing command packages. | Blocked until the final bootstrap track proves the compiler command surface can be owned by AxiOM packages. | [#721](https://github.com/OMT-Global/axiomlang/issues/721) |
| `compiler_scale_rewrite_fixture` | A compiler-scale AxiOM package that exercises parser, checker, diagnostics, package graph, and codegen-style flow before rewrite begins. | Blocked until the final Rust bootstrap track produces that proof workload. | [#721](https://github.com/OMT-Global/axiomlang/issues/721) |

## Closure Rules

- Do not start the compiler rewrite merely because small language examples pass.
- A row may be `implemented` only when its evidence paths exist and its
  validating command is concrete.
- A `blocked` or `partial` row must name at least one GitHub issue.
- Direct-native backend status must be explicit for every row, even when the row
  is documentation or rewrite-proof oriented.
- This checklist must fail until `compiler_command_surface` and
  `compiler_scale_rewrite_fixture` are backed by executable evidence.

## Rust Capture Check

This gate describes AxiOM language capability in AxiOM-neutral terms. Rust,
Cargo, and `rustc` may appear as current validation infrastructure, but they are
not the semantic contract for the language rows.
