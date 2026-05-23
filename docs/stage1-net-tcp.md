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
- `write(stream: TcpStream, buf: &[u8]): int`
- `close(stream: TcpStream): int`
- `close_listener(listener: TcpListener): int`

`listen` currently accepts loopback bind addresses such as `127.0.0.1:0`,
`[::1]:0`, and `localhost:0`. Non-loopback binds are rejected by the generated
runtime. Host and port allowlist policy for raw TCP and UDP sockets is tracked by
#737.

The calls are blocking. Async integration beyond spawning an async task around a
blocking call remains tracked by #738.
