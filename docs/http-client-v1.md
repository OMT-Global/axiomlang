# HTTP client v1 contract

Issue #1448 defines the contract boundary for a future HTTP client. This
document and its schema are preparatory artifacts: they do not claim that
runtime transport, TLS, cancellation, or dynamic request lowering is complete.

The canonical contract is
`stage1/compiler-contracts/schemas/axiom.runtime_http_client.v1.schema.json`,
with the accepted shape in
`stage1/compiler-contracts/snapshots/http-client-v1.json`.

The contract requires:

- capability-approved `http`/`https` authorities and deterministic method,
  path, query, and header handling;
- byte-oriented request and response bodies, with UTF-8 validation only for
  the text representation;
- bounded request (1 MiB), response (8 MiB), and header envelopes;
- status validation before body allocation, rejection of conflicting
  `Content-Length` values, rejection of truncated framed bodies, and an
  effective `final_url` after approved redirect processing;
- structured errors with a stable `code`, `phase`, `message`, and
  code-specific, bounded `details` object for status, framing, TLS,
  cancellation, and transport failures;
- redirect denial by default and verified system-root or pinned TLS policy;
- explicit cancellation outcomes with one terminal result and no post-cancel
  body delivery;
- evidence for authority, network scope, TLS verification, request identity,
  and runtime origin.

The checker is deliberately offline and standard-library-only:

```bash
python3 scripts/ci/check-http-client-v1.py --json
python3 scripts/ci/test-check-http-client-v1.py
```

The negative fixtures cover malformed status, conflicting lengths, oversized
bodies, unsupported security policy, and an invalid cancellation/error shape.
Transport implementation remains dependent on the reactor, network authority,
text/value ABI, and structured-concurrency work tracked by #1446, #1447,
#1441, #1425, #1426, and #1445.
