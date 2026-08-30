# Structured Concurrency v1

Structured Concurrency v1 is the target-neutral contract for task trees,
cancellation, deadlines, bounded channels, and fair waiting. The machine-
readable contract is
`stage1/compiler-contracts/snapshots/runtime-concurrency-v1.json`; validate it
with:

```bash
make stage1-runtime-concurrency-v1
python3 scripts/ci/test-check-structured-concurrency-v1.py
```

This slice establishes semantic inputs and evidence requirements. It does not
claim that the current compiler already provides the production scheduler,
readiness reactor, or network lifecycle described by issues #1445–#1449.

## Task trees

Every spawned task belongs to a parent task scope. A child must be explicitly
joined or joined as part of scope exit; detached children are rejected unless a
future contract explicitly grants that authority. A task handle is an
obligation, not a copyable host token. A deadline cancels the affected subtree
and then waits for its cleanup before reporting timeout.

Cancellation is cooperative and propagates from a cancelled parent to all
descendants. Cancellation is observable at waits and other documented
safepoints; it does not silently abandon child cleanup. Child count, in-flight
work, task depth, wait duration, and channel capacity are bounded resources.

## Channels and select

Channels are bounded multi-producer, multi-consumer queues with an explicit
positive capacity. Values are enqueued in FIFO order. A full channel applies
backpressure or returns cancellation; it never grows without a declared
budget. Closing is a single-discharge operation that wakes waiters. Receivers
drain buffered values before observing the closed result, while later sends
produce `concurrency.channel_closed`.

`select` waits for one of its arms, including cancellation and an optional
deadline. When several arms are ready, selection rotates the starting arm so a
permanently-ready arm cannot starve the others. An empty select must wait or
time out rather than spin.

## Cleanup and evidence

Normal return, early return, error return, panic/unwind, and cancellation all
run deferred work in last-in-first-out order, signal descendants, join them,
and only then release the parent scope. Every task and channel obligation is
discharged exactly once. Diagnostics carry source path, line, and column;
inspection exposes task state, deadlines, budgets, waiter counts, wakeup
reasons, join obligations, and provenance without exposing host handles,
capability secrets, raw thread identities, or scheduler addresses.

The fixture includes positive coverage for joins, nested scopes, cancellation,
timeouts, channel order/drain/backpressure, and select fairness. Negative
coverage includes detached children, join timeouts, sends after close, budget
exhaustion, and use after close.

## Migration boundary

The contract consumes Semantic MIR v1 operations and remains independent of a
particular scheduler, operating-system readiness API, networking framework, or
host representation. Runtime implementation slices must separately prove
cooperative cancellation, fairness, backpressure, shutdown, and resource
cleanup against these rules before the readiness state can advance.
