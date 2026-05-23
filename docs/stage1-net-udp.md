# Stage1 UDP sockets

Issue: #736

Stage1 exposes blocking UDP socket handles through `std/net_udp.ax`. The module
requires `[capabilities].net = true`.

```axiom
import "std/net_udp.ax"

let socket: UdpSocket = bind("127.0.0.1:0")
let peer: string = local_addr(socket)
```

The raw host intrinsics use integer handles, while the stdlib publishes an
`UdpSocket` alias so user code does not pass unlabelled handles through the
public API.

## API

- `bind(bind: string): UdpSocket`
- `local_addr(socket: UdpSocket): string`
- `local_port(socket: UdpSocket): int`
- `send_to(socket: UdpSocket, buf: &[u8], peer: string): int`
- `recv_from(socket: UdpSocket, buf: &mut [u8]): (int, string)`
- `close(socket: UdpSocket): int`

`bind` and `send_to` currently accept loopback addresses such as `127.0.0.1:0`,
`[::1]:0`, and `localhost:0`. Non-loopback addresses are rejected by the
generated runtime. Host and port allowlist policy for raw TCP and UDP sockets is
tracked by #737.
