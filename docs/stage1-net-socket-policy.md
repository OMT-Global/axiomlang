# Stage1 raw socket policy

Issue: #737

Raw TCP and UDP socket helpers share the existing `[capabilities].net` host and
port allowlists.

```toml
[capabilities]
net = { hosts = ["127.0.0.1"], ports = [8080] }
```

When either allowlist is configured, socket bind and peer arguments must be
static `host:port` string literals that are present in the configured allowlist.
Diagnostics name the blocked allowlist:

- `[capabilities].net.hosts` for host mismatches.
- `[capabilities].net.ports` for port mismatches.

Runtime checks use the same allowlists for generated programs, so direct
intrinsic calls and stdlib wrappers get the same policy enforcement.
