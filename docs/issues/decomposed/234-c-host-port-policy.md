---
parent: 234
title: "Net sockets C: per-host:port capability policy"
labels: [stage1, area:stdlib, security, lane:daedalus]
depends_on: [234-a-tcp-listener-stream, 234-b-udp-send-recv]
---

Part of #234. Reuse the existing `net.hosts` / `net.ports` manifest allowlists for the new TCP / UDP primitives so that capability gating extends beyond DNS and HTTP.

## Scope

- TCP listener `bind` and stream `write` peer are checked against `net.hosts` and `net.ports` exactly like `std/net.ax::resolve` does today.
- UDP `send_to` peer and `bind` address are checked the same way.
- Diagnostic: clearly says which allowlist (hosts vs ports) blocked the call.

## Acceptance

- Negative fixture: `net.hosts = ["127.0.0.1"]` allows local listen but rejects an external peer.
- Negative fixture: `net.ports = [8080]` allows port 8080 but rejects port 9090.

## Depends on

- 234-a, 234-b.
