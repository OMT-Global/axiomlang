# Stage1 stdlib status

<!-- capability-ledger:v1 commands=28 stdlib_modules=34 stdlib_functions=299 capabilities=9 backend=cranelift -->

This page maps historical phase-c roadmap issues to the current compiler-owned
stdlib inventory. Issue numbers are historical evidence, not live issue-state
or production-qualification claims. The checked capability ledger owns the
current 34-module and 299-exported-function inventory.

## Landed bootstrap floor

| Issue | Current stage1 support | Still out of scope |
| --- | --- | --- |
| #232 generic collections | `std/collections.ax` has generic borrowed-slice helpers, and `std/string_builder.ax` now provides an owned string accumulator. | Growable `Vec<T>`, maps, sets, traits, and mutable-borrow-backed collection mutation. |
| #237 structured JSON | `std/json.ax` supports scalar parse/stringify, typed top-level object field extraction with `parse_field_*`, manual `field_*` / `object*` builders, and small JSON Schema object helpers. `std/serdes.ax` adds a native `Value` union with `to_json({string: Value})`, `stringify(Value)`, and `from_json(text)` for object/array/scalar round trips. | Derived struct encode/decode, streaming parse, full JSON Schema coverage, and macros. |
| #238 regex | `std/regex.ax` supports `is_match`, `find`, and `replace_all` over a deterministic NFA-state engine with anchors, `.`, `?`, `*`, `+`, escaped literals, and character classes/ranges. | Captures, alternation/grouping, Unicode character properties, and precompiled regex values. |
| #239 structured logging | `std/log.ax` supports deterministic JSON-line event formatting, levels, key-value attributes, and ambient stderr emission. | Host log sinks, replay buffers, filtering, and runtime logger configuration. |

## Historical roadmap rows with remaining breadth

| Issue | Current state | Remaining unqualified breadth |
| --- | --- | --- |
| #233 fs write-side | `std/fs.ax` exposes read plus create, write, append, replace, mkdir, and removal helpers. Reads require `fs`; mutations require `fs:write`. Direct-native runtime evidence covers bounded, rooted filesystem shapes. | Broader dynamic path/content shapes and production qualification remain outside the proven slice. |
| #234 net sockets | `std/net.ax` supports DNS resolution, HTTP client GET exists in `std/http.ax`, `std/net_tcp.ax` exposes blocking loopback TCP listener/stream handles with byte-slice and string read/write/close operations, `std/net_udp.ax` exposes loopback UDP bind/send/recv handles, and `std/async_net.ax` now supports raw TCP listener accept plus owned-string recv/send helpers for connection-per-task services. Raw socket bind and peer literals honor `[capabilities].net.hosts` and `[capabilities].net.ports`. | Borrowed-buffer async recv/send, readiness-based wakeups, and non-loopback service policy remain future runtime work. |
| #236 crypto | `std/crypto_hash.ax`, `std/crypto_mac.ax`, `std/crypto_rand.ax`, `std/crypto_aead.ax`, and `std/crypto_sign.ax` expose hashing, MAC, randomness, AEAD, and Ed25519 key generation/sign/verify; `std/crypto.ax` re-exports the landed surface. | Broader audited crypto coverage and production qualification remain outside the proven slice. |
| #240 richer testing | `axiomc test` discovers `*_test.ax`, golden stdout, assertion helpers, and `std/testing.ax` table/property/snapshot helpers; `axiomc bench` is the benchmark harness. | Richer randomized generation and benchmark CI policy remain future harness design work. |
| #97 HTTP server | `std/http.ax` includes `get`, loopback-only `listen`/`accept`/`route`/`respond`/`close`, blocking `serve_once(bind, body)`, and bounded `fixed_route` / `serve(bind, route, max_requests)` primitives behind `[capabilities].net`; `std/http_async.ax` adds async-gated bounded route serving. | Landed for the current stage1 route-shaped handler model; richer production lifecycle controls remain future runtime design work. |

## Verification handles

- `stage1/examples/stdlib_string_builder`
- `stage1/examples/stdlib_json`
- `stage1/examples/stdlib_serdes`
- `stage1/examples/stdlib_regex`
- `stage1/examples/stdlib_log`
- `cargo test --manifest-path stage1/Cargo.toml -p axiomc`
- `make stage1-smoke`
