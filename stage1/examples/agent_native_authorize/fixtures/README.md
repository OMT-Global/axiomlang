# Agent-Native Authorize Fixtures

These fixtures show the #787 flow:

```text
intent -> semantic graph -> effects -> evidence -> artifacts
```

Intent IR is emitted live from this package rather than maintained as a second
hand-written snapshot. The contract test runs the command twice, validates both
the schema and byte stability, and verifies that every artifact and diagnostic
traces to an emitted semantic node:

```bash
axiomc inspect intent stage1/examples/agent_native_authorize --json
```

The same test uses `stage1/examples/workspace` as the multi-package fixture so
the contract covers workspace member packages and dependencies without cloning
source into a fixture-only tree.

Regenerate them with:

```bash
axiomc inspect graph stage1/examples/agent_native_authorize --json
axiomc evidence stage1/examples/agent_native_authorize --json
axiomc inspect artifacts stage1/examples/agent_native_authorize --json
```
