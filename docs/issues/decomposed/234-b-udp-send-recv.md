---
parent: 234
title: "Net sockets B: UDP send/recv primitives"
labels: [stage1, area:stdlib, lane:daedalus]
---

Part of #234. Add UDP socket primitives and an `std/net_udp.ax` wrapper.

## Scope

- Host intrinsics: `net_udp_bind(bind: string): UdpSocket`, `net_udp_send_to(socket, buf: &[u8], peer: string): int`, `net_udp_recv_from(socket, buf: &mut [u8]): (int, string)`.
- `std/net_udp.ax` wraps the intrinsics under `net` capability.

## Acceptance

- Pass fixture: send and receive a single datagram on a localhost UDP socket.
- Capability denial test as in 234-a.

## Out of scope

- Multicast — separate follow-up.
- Per-host:port allowlist policy — 234-c.
