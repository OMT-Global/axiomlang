# I/O Reactor v1 evidence contract

Issue #1446 now has a target-neutral evidence contract for readiness-based
sockets, timers, signals, cancellation wakeups, partial I/O, backpressure, and
deterministic resource closure. This slice does not claim that a runtime reactor
or portable target adapters are implemented.

The current TCP APIs are blocking. The `std/async_net.ax` wrappers call those
blocking operations from task-shaped functions; that is not proof of
nonblocking readiness, cancellation, or freedom from thread-per-connection
execution. The current implementation therefore remains `syntax_only` with
`runtime_backed`, `nonblocking_io`, `portable_adapters`, and
`thread_per_connection_free` all set to `false`.
The capability ledger's generic schema row remains `static_spike` for
base-checker compatibility; it records that the schema is checked and does not
override the contract snapshot's explicit `syntax_only` implementation tier.

The `axiom.io_reactor.v1` contract requires one runtime readiness model for TCP
listeners and streams, UDP sockets, monotonic timers, signals, and cancellation
wakeups. Every operation generation must identify its resource, readiness
interest, deadline, cancellation owner, buffer bound, target, and adapter.
Unavailable runtime evidence is represented as `null_with_reason`, never as a
synthetic ready state.

Promotion requires:

- bounded queues and buffers with explicit producer backpressure;
- partial read/write byte counts and zero-progress retry rules;
- deterministic cancellation races where late readiness cannot revive a
  terminal operation generation;
- idempotent closure and no readiness delivery after terminal close;
- supported-host adapter evidence for kqueue and epoll without exposing those
  adapter names or handles as language semantics; and
- fairness, slow-peer, descriptor-exhaustion, cancellation-race, and leak-free
  shutdown tests without a thread-per-connection requirement.

Validate the offline contract and its deterministic fixtures with:

```bash
make stage1-io-reactor-v1
make stage1-io-reactor-v1-test
```

Runtime completion remains blocked on #1425, #1426, #1436, #1438, and #1445.
Existing blocking socket and task APIs remain available while those foundations
land, but they do not satisfy this contract.
