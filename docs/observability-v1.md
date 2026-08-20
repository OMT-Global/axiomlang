# Runtime observability v1

Issue #1451 now has an executable, target-neutral Rust runtime core in
`stage1/crates/axiomc/src/runtime_observability.rs`. This is a partial delivery,
not issue closure. It does not claim that AxiOM `std/log.ax` is wired to the
runtime, that trace context crosses tasks or host calls, or that metrics and
external exporters exist.

The canonical schema and snapshot are:

- `stage1/compiler-contracts/schemas/axiom.runtime_observability.v1.schema.json`
- `stage1/compiler-contracts/schemas/axiom.runtime_observability_evidence.v1.schema.json`
- `stage1/compiler-contracts/snapshots/runtime-observability-v1.json`

The contract establishes a bounded event envelope, deterministic levels and
labels, a 64 KiB event limit, a 32-field limit, and a maximum of 1,000 values
per label. Secrets are redacted before any sink receives an event. Runtime
correlation fields (`request_id`, `trace_id`, `span_id`, and `runtime_origin`)
are trusted context: the host fixes `runtime_origin` when constructing the
runtime, and event callers cannot overwrite it or the issued identifiers.

The executable slice provides:

- runtime-updatable global and exact-target level filters;
- bounded dynamic fields for null, boolean, signed, unsigned, finite float,
  and explicitly public text values;
- explicit public message templates and redaction-by-default for sensitive
  values, secret-shaped keys, and error messages;
- runtime-issued request, trace, and span identifiers that reject contexts
  created by another runtime instance;
- count- and byte-bounded queues with deterministic `drop_newest` and
  `drop_oldest` behavior;
- exporter-neutral JSON-line sinks that receive only sanitized bytes;
- latched, sanitized sink failures and counters for accepted, filtered,
  rejected, dropped, written, and failed events;
- stop-intake, ordered drain, flush, and terminal shutdown states. A sink error
  or elapsed deadline is `failed`, never `flushed`;
- schema-validated event, receipt, inspection, drain, and shutdown evidence.

The synchronous timeout check is honest but not preemptive: if a sink blocks
inside its write or flush implementation, the current runtime has no structured
concurrency primitive with which to interrupt it. The deadline is evaluated
after each sink operation returns.

Validate the contract without network access:

```bash
make stage1-runtime-observability-v1
```

Trusted PR CI executes checker and self-test code from the base-pinned
`.trusted-ci` checkout and supplies the PR checkout only through explicit
`--root`. Every checker read is descriptor-relative, no-follow, nonblocking,
regular-file-only, strict UTF-8, and capped at 1 MiB. The PR that first adds
this checker hardening cannot make its own new checker authoritative; the
split-checkout gate becomes authoritative after the checker lands on the
trusted base. The same base checkout owns a standalone Rust harness, builds
the PR's `axiomc` library without running PR-defined tests, links the trusted
harness against that fresh library, and requires real redaction, correlation,
bounded delivery, and terminal drain-before-flush behavior.

Remaining issue blockers are exact:

- `std/log.ax` still formats and writes directly instead of calling this
  bounded runtime, and user code has no runtime filter/configuration surface;
- task, HTTP, process/provider, SQLite, and database context propagation waits
  on #1445, #1449, #1444, #1452, and the corresponding runtime integrations;
- baggage trust rules and cross-boundary correlation transcripts are absent;
- counters, gauges, histograms, and bounded label-cardinality storage are not
  implemented;
- rotation, file ownership/permissions, OTLP or other exporter adapters, and
  exporter retry policy are absent;
- shutdown cannot preempt a blocking sink until structured concurrency and
  cancellation are available;
- direct-native and generated-Rust user-program receipts are absent.

The value/collection and serialization dependencies remain tracked by #1425,
#1426, #1450, and #1476. This slice is `Refs #1451`; it does not close #1451.
