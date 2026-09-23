# Process v1 contract

Issue #1444 is represented by a portable, offline Process v1 contract and
fixture corpus. This tranche does not claim that the structured process API,
streaming pipes, cancellation, resource limits, signals, or terminal control
are implemented.

The canonical schema and snapshot are:

- `stage1/compiler-contracts/schemas/axiom.runtime_process.v1.schema.json`
- `stage1/compiler-contracts/snapshots/runtime-process-v1.json`

The published schema separates stable Process v1 semantics from mutable
qualification evidence. It can represent a fully qualified runtime-complete
implementation, while the checked snapshot remains the current fail-closed
`static_spike` floor.

Process v1 always separates the executable identity from its ordered UTF-8
arguments. It never performs implicit shell parsing. The working directory,
environment inheritance and keys, stdio modes, process control, signals, and
terminal access are independent deny-by-default authority dimensions. A
denied decision is audited before its protected operation and does not record
argument, environment, or stdin values. The allow/deny fixture covers all
eight dimensions; spawn-preflight denial cannot start a child, and runtime
control denial cannot apply its requested effect. That denial applies only to
caller-directed control: runtime-owned supervision remains mandatory, closes
pipes, and reaps every child. If the caller abandons a handle, the runtime
performs bounded graceful-then-forced termination and reap without requiring
caller authority.
The published schema represents those guarantees as closed structured fields:
runtime ownership, supervision after denial, a grace period no greater than
the contract's 5,000 ms maximum, pipe closure, forced termination when still
running, and child reap are all mandatory for every conforming document.

Captured stdin, stdout, and stderr are bounded. Output overflow terminates the
child and returns `process.output_limit_exceeded` with at most the configured
retained bytes. Timeouts use a monotonic clock; timeout or cancellation asks
for graceful termination, waits a bounded grace period, and then forces
termination. Exit codes, signals, timeout, cancellation, spawn failure, and
output or resource-limit overflow remain distinct outcomes. The exact state
transition set is:

- `spawning -> running` or `spawning -> spawn_failed`
- `running -> exited`, `running -> signaled`, `running -> timed_out`, or
  `running -> cancelled`
- `timed_out -> exited` or `timed_out -> signaled`, and `cancelled -> exited`
  or `cancelled -> signaled`, while preserving the initiating timeout or
  cancellation outcome. A normal exit represents a child that honored the
  graceful termination request before force was required.

The schema enumerates this exact ten-transition set and separately binds both
terminal states to the initiating `timeout` or `cancelled` outcome, so an exit
code or signal cannot replace that initiating outcome in a conforming claim.

Process v1 supports four portable resource-limit names. Every default and
maximum is finite, and `process_control` authority may lower a default or set a
lower per-request ceiling but may not remove a bound.

| Limit | Unit | Minimum | Default | Maximum |
| --- | --- | ---: | ---: | ---: |
| `cpu_time_ms` | milliseconds | 1 | 30,000 | 300,000 |
| `memory_bytes` | bytes | 1,048,576 | 268,435,456 | 1,073,741,824 |
| `open_files` | count | 3 | 64 | 1,024 |
| `subprocesses` | count | 0 | 1 | 64 |

Invalid, unauthorized, or unsupported limits fail before spawn with
`process.resource_limit_invalid`, `process.capability_denied`, or
`process.resource_limit_unsupported`. Exceeding an effective limit terminates
and cleans up the child with the distinct
`process.resource_limit_exceeded` outcome and diagnostic.

The existing `run_status(command: string): int` helper is legacy evidence, not
the Process v1 API. Generated-native execution uses one exact executable value,
and the POSIX direct-native path uses `execv` rather than shell parsing.
Windows direct-native execution currently passes command text to `system`, so
it reparses shell syntax and is explicitly non-qualifying legacy evidence; it
does not establish argv-safe Process v1 behavior. The legacy helper still lacks
argv, explicit environment/cwd policy, pipes, timeouts, cancellation, terminal
state, and portable Windows parity.
The snapshot pins the exact five-file legacy evidence set, and the checker
validates the relevant executable, stdlib binding, manifest authority, and
direct-native readiness content in each file.

Validate the contract without network access:

```bash
make stage1-runtime-process-v1
make stage1-runtime-process-v1-test
```

Runtime implementation remains dependent on build-effect purity,
collection/string, lifecycle, structured-concurrency, and program-host work
tracked by #1434, #1425, #1426, #1438, #1445, and #1477. Until those gates
permit a real structured API, the rollback
and compatibility boundary is the current legacy helper: it does not
implicitly opt into Process v1, and Process v1 remains `static_spike`.
