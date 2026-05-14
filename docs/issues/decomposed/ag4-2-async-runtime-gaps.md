---
title: "AG4.2 follow-up: real timers, host-thread scheduling, blocking wakeups"
labels: [stage1, area:runtime, lane:daedalus]
parent: null
---

This issue tracks the explicit AG4.2 gaps called out in `docs/stage1-agent-grade-compiler.md`:

> Stage1 still does not provide host-thread scheduling, blocking wakeups, or real timers.

Today the async runtime is deterministic and single-threaded. To serve real workloads (#97 HTTP server, #234-d async sockets, future agent runtimes), it needs:

## Scope

1. **Host-thread scheduling** — a small thread pool that drives `Task<T>` work in parallel. Configurable pool size via `[runtime].max_threads` in `axiom.toml`; default is `num_cpus`.
2. **Blocking wakeups** — task suspension that releases the worker thread back to the pool until a wakeup is delivered (channel send, IO readiness, timer fire).
3. **Real timers** — the existing `axiom_async_timeout` is already real-time-bounded (PR #573), but `clock_sleep_ms` blocks the current thread. Wire `clock_sleep_ms` and `async_timeout` through a single timer wheel.

## Acceptance

- Benchmark fixture: 1,000 simultaneous `async_sleep(100ms)` tasks complete in roughly 100ms wall time, not 100,000ms.
- An HTTP service (#609 once it lands) serves two concurrent requests on a single binary without spawning host threads explicitly.
- The runtime sections of `docs/stage1.md` describe the new behavior.

## Working rules

- Keep the deterministic mode available behind a build flag for replay / record use cases (`docs/roadmap.md` notes "Built-in record/replay determinism for agent runs").
- Do not introduce new external crates beyond what's already in `stage1/Cargo.toml`; if a thread pool is needed, hand-roll a minimal one or pull in a vetted small dep (e.g., `crossbeam-deque`).
