---
parent: 234
title: "Net sockets D: async integration"
labels: [stage1, area:stdlib, area:runtime, lane:daedalus]
depends_on: [234-a-tcp-listener-stream, 234-c-host-port-policy]
---

Part of #234. Make the new TCP/UDP primitives cooperate with the AG4.2 async runtime so a single binary can serve concurrent connections without spawning host threads.

## Scope

- `async fn async_accept(listener: TcpListener): Task<TcpStream>` and `async fn async_recv(stream: TcpStream, buf: &mut [u8]): Task<int>` yield to the scheduler rather than blocking the host thread.
- A connection-per-task pattern is demonstrated in `stage1/examples/stdlib_net_tcp_async`.
- Mirrors the design of #609 (HTTP server async integration) — share helpers where possible.

## Acceptance

- Pass fixture: two concurrent TCP connections to a single accepting binary echo bytes in deterministic order.
- Performance benchmark fixture records baseline accept latency.

## Depends on

- 234-a, 234-c. Closely related to AG4.3c (#609) HTTP server async integration.
