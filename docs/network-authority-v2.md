# Runtime network authority v2

Network Authority v2 is a target-neutral, contract-only definition for issue
#1447. It separates DNS resolution, outbound connect, inbound listen, and
accepted-peer authority. It does not authorize a runtime transport
implementation.

Rules are evaluated against the requested and resolved endpoint, host pattern,
IP/CIDR, port or range, interface, and Unix socket. Loopback is the safe
default for listen and accepted-peer authority. Dynamic endpoints are denied
unless each resolution is validated; DNS rebinding is revalidated before use.

Audit decisions expose the requested endpoint, resolved endpoint, direction,
governing rule, and decision. Credentials, query values, and authorization
headers are redacted.

Verify the contract and deterministic fixtures:

```bash
python3 scripts/ci/check-network-authority-v2.py --json
python3 scripts/ci/test-check-network-authority-v2.py
```

Manifest, effect, Intent IR, reactor, provider, and runtime enforcement changes
remain follow-on work for the dependent issues.
