# Runtime observability v1 contract

Issue #1451 is currently represented by an offline contract and evidence
fixture set. This tranche does not claim runtime logging sinks, tracing
propagation, metrics export, or shutdown flushing are implemented.

The canonical schema and snapshot are:

- `stage1/compiler-contracts/schemas/axiom.runtime_observability.v1.schema.json`
- `stage1/compiler-contracts/snapshots/runtime-observability-v1.json`

The contract establishes a bounded event envelope, deterministic levels and
labels, a 64 KiB event limit, a 32-field limit, and a maximum of 1,000 values
per label. Secrets are redacted before any sink receives an event. Runtime
correlation fields (`request_id`, `trace_id`, `span_id`, and `runtime_origin`)
are trusted context and cannot be overwritten by callers.

Sinks have bounded queues and an explicit backpressure policy. Sink failure,
dropped-event counts, and shutdown timeout are observable states. Shutdown must
stop intake, drain accepted events, flush sinks, and only then report `flushed`;
failure or timeout cannot be reported as success.

Validate the contract without network access:

```bash
python3 scripts/ci/check-runtime-observability-v1.py --json
python3 scripts/ci/test-check-runtime-observability-v1.py
```

The runtime implementation remains dependent on the value/collection,
concurrency, serialization, HTTP, provider, and database contracts tracked by
#1425, #1426, #1445, #1448, #1450, #1453, and #1452.
