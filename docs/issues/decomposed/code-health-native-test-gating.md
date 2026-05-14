---
title: "Code health: gate native-runtime test cases behind a feature flag"
labels: [stage1, area:infra, lane:hephaestus]
parent: null
---

During the May 2026 review-cleanup session, every PR's local `cargo test --lib` hit the same 10 deterministic failures on a sandboxed dev host that lacks a system linker:

- `build_project_emits_native_binary_with_const_sized_arrays`
- `build_project_rejects_non_int_const_sized_array_lengths`
- `build_project_resolves_const_sized_arrays_inside_function_bodies`
- `checked_in_proof_http_service_requires_net_capability_for_server`
- `checked_in_proof_http_service_serves_local_request_response`
- `env_allowlist_scopes_generated_env_get`
- `conformance_corpus_reports_stable_results`
- `run_project_tests_supports_local_http_fixture_runner`
- `stage1_project_supports_local_path_dependencies`
- `stage1_project_supports_async_runtime_surface`

All ten try to `axiomc build → axiomc run` a native binary. They pass in CI (which has `cc` / `rustc` linker tooling) but fail anywhere else, producing scary panics that look like real test failures to new contributors.

## Scope

- Add a Cargo feature `run-native-tests` (default off) in `stage1/crates/axiomc/Cargo.toml`.
- Gate the affected tests with `#[cfg_attr(not(feature = "run-native-tests"), ignore)]` so they're listed-but-skipped when the feature is off.
- CI invocations switch to `cargo test --features run-native-tests` (Makefile + workflow files).
- Documentation: `docs/stage1.md` "running tests locally" section explains the gate.
- Optional: a `cargo test --lib` smoke run in this repo's PR CI confirms the non-gated tests pass on hosts without a linker, catching gating regressions.

## Acceptance

- `cargo test --manifest-path stage1/Cargo.toml -p axiomc` on a sandbox without `cc` / `rust-lld` passes with zero failures (10 tests show as ignored).
- `make stage1-test` still runs the native tests because it enables the feature.

## Working rules

- The intent is **discoverability**, not weakening coverage. CI behavior must not change.
