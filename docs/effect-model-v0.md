# Effect Model v0

Effect Model v0 describes observable operations a package may perform through
stage1 stdlib and runtime surfaces. It complements manifest capabilities:
capabilities are coarse gates, while effects are inspectable semantic records
with source spans and resources.

## Command

```bash
axiomc inspect effects <path> --json
```

The command is read-only. It parses package source and emits effect nodes for
known calls without changing `axiomc caps` behavior.

## Fields

Each effect record contains:

- `id`: stable package-scoped effect id.
- `kind`: semantic effect kind.
- `resource`: literal resource when available, or a placeholder for dynamic
  expressions.
- `operation`: operation performed on the resource.
- `capability_gate`: manifest capability required by the runtime surface.
- `source_span`: file, line, and column where the effect call appears.
- `policy`: v0 allowlist booleans for host and port policy.

## V0 Kinds

The initial map covers the current stdlib/runtime surfaces:

- `clock.now`
- `clock.sleep`
- `env.read`
- `fs.read`
- `fs.write`
- `network.dns.resolve`
- `network.http.get`
- `network.tcp.bind`
- `network.tcp.connect`
- `network.udp.send`
- `process.status`
- `crypto.hash`
- `crypto.mac`

Future work can add `crypto.rand`, `crypto.sign`, and richer policy evaluation
as those runtime surfaces land.
