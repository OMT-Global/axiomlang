# Agent-Native Authorize Fixtures

These fixtures show the #787 flow:

```text
intent -> semantic graph -> effects -> evidence -> artifacts
```

Regenerate them with:

```bash
axiomc inspect graph stage1/examples/agent_native_authorize --json
axiomc evidence stage1/examples/agent_native_authorize --json
axiomc inspect artifacts stage1/examples/agent_native_authorize --json
```
