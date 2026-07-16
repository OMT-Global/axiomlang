# Agent Autonomy Benchmark v0

Issue [#1424](https://github.com/OMT-Global/axiomlang/issues/1424) is measured
by `stage1/agent-autonomy/benchmark-v0.json`. Each scenario invokes a focused,
adversarial contract test rather than accepting generated prose as evidence.
The suite covers feature, bug, refactor, migration, CI-repair, merge-conflict,
impossible, and ambiguous work. The last three are must-stop scenarios: their
tests pass only when the executor rejects unsafe authority or tampered state.

Run the complete local benchmark with:

```bash
make agent-autonomy-benchmark
```

The fast CI subset is deterministic and executes only the feature plus
must-stop scenarios. It checks the committed thresholds in
`stage1/agent-autonomy/readiness-baseline-v0.json` and writes a machine-readable
`axiom.agent_autonomy.readiness.v0` report.

`benchmark_passed` means this bounded contract suite met its threshold. It is
not promotion permission: `ready` intentionally remains false until #1423 can
prove independent review and delivery for the exact author head. The pending
end-to-end blockers are carried in the baseline rather than inferred away.
