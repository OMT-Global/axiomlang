# Native Debugging v1 evidence contract

Issue #1466 now has a target-neutral evidence contract for native AxiOM DWARF,
process-backed Debug Adapter Protocol behavior, and symbolized profiling. This
slice does not claim that those runtime capabilities are implemented.

The current `axiomc dap` endpoint is a source simulator. Clients must opt in
with `mode: "source-simulator"`. Its line-range checks are not native
breakpoint installation, so breakpoint responses keep `verified: false` and
report source resolution separately as `axiomSourceResolved`.

`axiom/debugStatus` and the initialize response expose the distinct, closed
`axiom.native_debug_status.v1` status envelope. The larger
`axiom.native_debugging.v1` schema governs this evidence contract and is never
used to label the runtime payload. Until real target-bound evidence exists,
`processBacked`, `nativeAxiomDwarf`, and `profileSymbolization` remain false;
binary digest and target remain unavailable; and the status names the open
executable-MIR and target-evidence blockers. Source simulation may expose a
SHA-256 source generation after launch, but that is not a binary identity.

The normative evidence schema can represent either the current static spike or
a fully production-qualified process-backed implementation. The separate live
status schema remains deliberately closed to the current source simulator so
an unavailable runtime cannot report future qualification fields early.

Thread, stack, scope, and variable observations succeed only while an
explicitly launched source simulator is stopped. Before launch and after
termination the adapter returns failed DAP responses without synthetic
observation bodies.

Native Debugging v1 can be promoted only when both supported hosts provide:

- native AxiOM line tables, symbols, stack frames, and representative locals;
- scripted LLDB or GDB breakpoint, step, backtrace, locals, and source proof;
- real DAP launch or attach with verified runtime controls and observations;
- symbolized profile samples bound to binary digest, source generation, and
  target.

Debug manifests and sidecar maps remain supplemental. They cannot prove native
DWARF, process control, native breakpoint installation, or profile
symbolization by themselves.

Validate the offline contract and the negative fixtures with:

```bash
make stage1-native-debugging-v1
make stage1-native-debugging-v1-test
cargo test --manifest-path stage1/Cargo.toml -p axiomc dap --lib
```

The runtime completion lane remains blocked on #1436 and #1455. The accepted
MIR/lifecycle and compatibility/source-span contracts from #1437 and #1457
remain dependencies. Readiness stays partial until real binary and debugger
evidence exists for every supported target.
