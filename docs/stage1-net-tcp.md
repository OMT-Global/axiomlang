# Stage1 TCP sockets

Issue: #735

Stage1 exposes blocking TCP listener and stream handles through `std/net_tcp.ax`.
The module requires `[capabilities].net = true`.

```axiom
import "std/net_tcp.ax"

let listener: TcpListener = listen("127.0.0.1:0")
let port: int = local_port(listener)
```

The raw host intrinsics use integer handles, while the stdlib publishes aliases
named `TcpListener` and `TcpStream` so user code does not pass unlabelled handles
through the public API.

## API

- `listen(bind: string): TcpListener`
- `local_port(listener: TcpListener): int`
- `accept(listener: TcpListener): TcpStream`
- `read(stream: TcpStream, buf: &mut [u8]): int`
- `read_string(stream: TcpStream, max_bytes: int): string`
- `write(stream: TcpStream, buf: &[u8]): int`
- `write_string(stream: TcpStream, message: string): int`
- `close(stream: TcpStream): int`
- `close_listener(listener: TcpListener): int`

`listen` currently accepts loopback bind addresses such as `127.0.0.1:0`,
`[::1]:0`, and `localhost:0`. Non-loopback binds are rejected by the generated
runtime. When `[capabilities].net.hosts` or `[capabilities].net.ports` is set,
bind literals must match those allowlists.

The raw byte-slice `read` and `write` calls are blocking and remain the
low-level API for caller-owned buffers. `std/async_net.ax` exposes
`listen`, `accept`, `recv_text`, and `send_text` as `Task`-returning helpers for
connection-per-task services; the text helpers use owned strings so spawned
tasks do not carry borrowed buffers across host-thread boundaries. See
`stage1/examples/stdlib_net_tcp_async` for a two-client loopback echo fixture.
