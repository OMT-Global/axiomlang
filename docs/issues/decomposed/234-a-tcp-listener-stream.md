---
parent: 234
title: "Net sockets A: TCP listener and stream primitives"
labels: [stage1, area:stdlib, lane:daedalus]
---

Part of #234. Add raw TCP listener / stream primitives to the host runtime and a thin `std/net_tcp.ax` wrapper.

## Scope

- Host intrinsics: `net_tcp_listen(bind: string): TcpListener`, `net_tcp_accept(listener: TcpListener): TcpStream`, `net_tcp_read(stream: TcpStream, buf: &mut [u8]): int`, `net_tcp_write(stream: TcpStream, buf: &[u8]): int`, `net_tcp_close(stream)` / `net_tcp_close_listener(listener)`.
- `std/net_tcp.ax` exposes typed wrappers that call the intrinsics under the `net` capability.
- All operations are blocking for now; async wrappers come in 234-d.

## Acceptance

- Pass fixture: a small echo server using `std/net_tcp.ax` accepts one connection and echoes a byte buffer.
- Capability denial test: missing `net` rejects the import deterministically.

## Out of scope

- UDP — 234-b.
- Per-host:port allowlist policy — 234-c.
- Async runtime integration — 234-d.
