//! Synthetic stage1 standard library.
//!
//! The AG4.1 milestone introduces a `std.*` surface exposed through the normal
//! `import "std/<module>.ax"` syntax. The compiler materialises a synthetic
//! package under the sentinel path [`STDLIB_ROOT`] whose sources live in a
//! compile-time table instead of the filesystem. Each stdlib module is a thin
//! wrapper around existing capability-gated intrinsics, so capability
//! enforcement continues to run against the importing package's manifest via
//! `hir::lower_with_capabilities`.
//!
//! Today this provides thirty stdlib modules. The capability-gated
//! wrappers include the six manifest capability classes:
//!
//! * `std/time.ax` — `Duration`, `Instant`, `now_ms()`, `now()`,
//!   `elapsed_ms(start)`, and `sleep(duration)` on top of `clock_now_ms`,
//!   `clock_elapsed_ms`, and `clock_sleep_ms` (clock).
//! * `std/env.ax` — `get_env(key)` on top of `env_get` (env).
//! * `std/fs.ax` — `read_file(path)` on top of `fs_read` (fs) plus write-side helpers behind `fs:write`.
//! * `std/net.ax` — `resolve(host)` on top of `net_resolve`, plus a bounded
//!   loopback-only TCP/UDP socket floor on top of `net_tcp_*` and `net_udp_*`
//!   intrinsics (net).
//! * `std/net_tcp.ax` — dedicated TCP wrappers over blocking listener/stream
//!   host intrinsics plus the current bounded loopback helpers (net).
//! * `std/net_udp.ax` — dedicated UDP socket wrappers plus loopback helpers on
//!   top of `net_udp_*` intrinsics (net).
//! * `std/process.ax` — `run_status(command)` on top of `process_status`
//!   (process).
//! * `std/crypto_hash.ax` — `sha256(input)` on top of `crypto_sha256` (crypto).
//!   (This is the stage1 spelling of the `std.crypto.hash` module from the
//!   AG4.1 plan; stage1 uses a flat filename to avoid cross-platform path
//!   separator issues in the virtual stdlib table.)
//! * `std/crypto_mac.ax` — `hmac_sha256(key, message)`,
//!   `hmac_sha512(key, message)`, `verify_sha256(tag, key, message)`,
//!   `verify_sha512(tag, key, message)`, `constant_time_eq(left, right)`,
//!   and `constant_time_eq_u8(left, right)` on top of `crypto_hmac_*` and
//!   `crypto_constant_time_eq*` (crypto).
//! * `std/crypto_rand.ax` — `random_bytes(n)` and `random_u64()` on top of
//!   `crypto_rand_*` intrinsics (crypto).
//! * `std/crypto_aead.ax` — typed AEAD algorithm wrappers for AES-GCM and
//!   ChaCha20-Poly1305 on top of `crypto_aead_*` intrinsics (crypto).
//! * `std/crypto_sign.ax` — Ed25519 key generation, signing, and verification
//!   on top of `crypto_ed25519_*` intrinsics (crypto).
//! * `std/crypto.ax` — umbrella re-export module for the stage1 crypto hash
//!   MAC, random, AEAD, and Ed25519 helpers.
//!
//! Additional modules share existing capability classes with peer wrappers,
//! demonstrating that the `std.*` surface is not limited to one wrapper per
//! capability:
//!
//! * `std/http.ax` — `get(url)`, explicit loopback-only server handles
//!   (`listen`, `accept`, `route`, `respond`, and `close`), and route-shaped
//!   compatibility helpers on top of the `http_*` intrinsics. `std/http_async.ax`
//!   carries async route serving so plain HTTP imports do not require the async
//!   capability. HTTP shares the `net` capability surface because any code that
//!   can open a raw TCP socket could implement HTTP itself, so a separate `http`
//!   manifest flag would not add meaningful isolation in stage1. The stage1
//!   client supports both http:// and https:// URLs; the server helpers bind
//!   loopback-only sockets and serve HTTP/1.0 responses.

//!
//! The remaining modules are stdlib surfaces not tied to a
//! capability flag, matching the ambient status of the `print` statement:
//!
//! * `std/traits.ax` — the static-dispatch seed trait `Eq`.
//! * `std/io.ax` — `eprintln(text)`, `readline()`, and `read_to_string()`
//!   on top of the new ungated `io_*` intrinsics.
//! * `std/json.ax` — scalar/string JSON parsing plus first-class `JsonValue`
//!   parsing, composition, nested field lookup, and serialization helpers on
//!   top of ungated `json_parse_*` / `json_stringify_*` intrinsics.
//! * `std/serdes.ax` — an Axiom `Value` union plus map-to-JSON and JSON-to-value
//!   helpers on top of ungated `json_serdes_*` bootstrap intrinsics.
//! * `std/collections.ax` — generic borrowed-slice helpers built on the
//!   existing polymorphic collection primitives and AG2 generic functions.
//! * `std/string_builder.ax` — an owned string accumulator implemented with
//!   stage1 strings.
//! * `std/log.ax` — deterministic JSON-line logging helpers over ambient
//!   stderr.
//! * `std/sync.ax` — ownership-shaped synchronization primitives implemented
//!   in Axiom: move-only mutex guards, one-shot cells, and single-slot
//!   nonblocking channels.
//! * `std/async.ax` — task, join, channel, timeout, cancellation, and select
//!   wrappers over the stage1 async runtime values.
//! * `std/async_time.ax` and `std/async_net.ax` — async task wrappers around
//!   `std/time` timer and `std/net` loopback socket primitives.
//! * `std/http_async.ax` — async task wrapper around bounded HTTP route serving.
//! * `std/regex.ax` — linear-time regular-expression helpers (`is_match`,
//!   `find`, `replace_all`) over a stage1-safe NFA engine.
//! * `std/testing.ax` — table-case, property, and snapshot assertion helpers
//!   layered over the bootstrap test intrinsics.
//! * `std/outcome.ax` — generic `Option<T>` / `Result<T, E>` predicates and
//!   fallback unwrap helpers implemented in Axiom.
//! * `std/encoding.ax` — URL query and path percent-encoding helpers.
//! * `std/cli.ax` — access to process arguments forwarded by `axiomc run`.

use std::path::{Path, PathBuf};

/// Sentinel path component used as the synthetic stdlib package root.
pub(crate) const STDLIB_ROOT: &str = "<stdlib>";

/// Import-prefix that selects the synthetic stdlib package.
pub(crate) const STDLIB_IMPORT_PREFIX: &str = "std";

/// Package name used for the synthetic stdlib manifest.
pub(crate) const STDLIB_PACKAGE_NAME: &str = "std";

/// Package version used for the synthetic stdlib manifest.
pub(crate) const STDLIB_PACKAGE_VERSION: &str = "0.0.0";

/// Compile-time table of stdlib module sources keyed by their path relative to
/// the stdlib import prefix. The bootstrap remains hermetic because the compiler
/// embeds sources at compile time; Phase-H modules may live as `.ax` files and
/// enter this table through `include_str!`.
const STDLIB_SOURCES: &[(&str, &str)] = &[
    (
        "traits.ax",
        "pub trait Eq {\nfn eq(self, other: Self): bool\n}\n",
    ),
    (
        "time.ax",
        "pub struct Duration {\nms: int\n}\n\
pub struct Instant {\nms: int\n}\n\
pub fn duration_ms(ms: int): Duration {\nreturn Duration { ms: ms }\n}\n\
pub fn now_ms(): int {\nreturn clock_now_ms()\n}\n\
pub fn now(): Instant {\nreturn Instant { ms: clock_now_ms() }\n}\n\
pub fn elapsed_ms(start: Instant): int {\nreturn clock_elapsed_ms(start.ms)\n}\n\
pub fn sleep(duration: Duration): int {\nreturn clock_sleep_ms(duration.ms)\n}\n",
    ),
    (
        "env.ax",
        "pub fn get_env(key: string): Option<string> {\nreturn env_get(key)\n}\n",
    ),
    (
        "fs.ax",
        "pub fn read_file(path: string): Option<string> {
return fs_read(path)
}
\
pub fn write_file(path: string, content: string): int {
return fs_write(path, content)
}
\
pub fn create_file(path: string): int {
return fs_create(path)
}
\
pub fn append_file(path: string, content: string): int {
return fs_append(path, content)
}
\
pub fn mkdir(path: string): int {
return fs_mkdir(path)
}
\
pub fn mkdir_all(path: string): int {
return fs_mkdir_all(path)
}
\
pub fn remove_file(path: string): int {
return fs_remove_file(path)
}
\
pub fn remove_dir(path: string): int {
return fs_remove_dir(path)
}
\
pub fn replace_file(path: string, content: string): int {
return fs_replace(path, content)
}
",
    ),
    (
        "net.ax",
        "pub fn resolve(host: string): Option<string> {\nreturn net_resolve(host)\n}\n\
pub fn tcp_listen_loopback_once(response: string, timeout_ms: int): Option<int> {\nreturn net_tcp_listen_loopback_once(response, timeout_ms)\n}\n\
pub fn tcp_dial(host: string, port: int, message: string, timeout_ms: int): Option<string> {\nreturn net_tcp_dial(host, port, message, timeout_ms)\n}\n\
pub fn udp_bind_loopback_once(response: string, timeout_ms: int): Option<int> {\nreturn net_udp_bind_loopback_once(response, timeout_ms)\n}\n\
pub fn udp_send_recv(host: string, port: int, message: string, timeout_ms: int): Option<string> {\nreturn net_udp_send_recv(host, port, message, timeout_ms)\n}\n",
    ),
    (
        "net_tcp.ax",
        "pub type TcpListener = int\n\
pub type TcpStream = int\n\
pub fn listen(bind: string): TcpListener {\nreturn net_tcp_listen(bind)\n}\n\
pub fn local_port(listener: TcpListener): int {\nreturn net_tcp_listener_port(listener)\n}\n\
pub fn accept(listener: TcpListener): TcpStream {\nreturn net_tcp_accept(listener)\n}\n\
pub fn read(stream: TcpStream, buf: &mut [u8]): int {\nreturn net_tcp_read(stream, buf)\n}\n\
pub fn read_string(stream: TcpStream, max_bytes: int): string {\nreturn net_tcp_read_string(stream, max_bytes)\n}\n\
pub fn write(stream: TcpStream, buf: &[u8]): int {\nreturn net_tcp_write(stream, buf)\n}\n\
pub fn write_string(stream: TcpStream, message: string): int {\nreturn net_tcp_write_string(stream, message)\n}\n\
pub fn close(stream: TcpStream): int {\nreturn net_tcp_close(stream)\n}\n\
pub fn close_listener(listener: TcpListener): int {\nreturn net_tcp_close_listener(listener)\n}\n\
pub fn listen_loopback_once(response: string, timeout_ms: int): Option<int> {\nreturn net_tcp_listen_loopback_once(response, timeout_ms)\n}\n\
pub fn dial(host: string, port: int, message: string, timeout_ms: int): Option<string> {\nreturn net_tcp_dial(host, port, message, timeout_ms)\n}\n",
    ),
    (
        "net_udp.ax",
        "pub type UdpSocket = int\n\
pub fn bind(bind: string): UdpSocket {\nreturn net_udp_bind(bind)\n}\n\
pub fn local_addr(socket: UdpSocket): string {\nreturn net_udp_local_addr(socket)\n}\n\
pub fn local_port(socket: UdpSocket): int {\nreturn net_udp_local_port(socket)\n}\n\
pub fn send_to(socket: UdpSocket, buf: &[u8], peer: string): int {\nreturn net_udp_send_to(socket, buf, peer)\n}\n\
pub fn recv_from(socket: UdpSocket, buf: &mut [u8]): (int, string) {\nreturn net_udp_recv_from(socket, buf)\n}\n\
pub fn close(socket: UdpSocket): int {\nreturn net_udp_close(socket)\n}\n\
pub fn bind_loopback_once(response: string, timeout_ms: int): Option<int> {\nreturn net_udp_bind_loopback_once(response, timeout_ms)\n}\n\
pub fn send_recv(host: string, port: int, message: string, timeout_ms: int): Option<string> {\nreturn net_udp_send_recv(host, port, message, timeout_ms)\n}\n",
    ),
    (
        "process.ax",
        "pub fn run_status(command: string): int {\nreturn process_status(command)\n}\n",
    ),
    (
        "crypto_hash.ax",
        "pub fn sha256(input: string): string {\nreturn crypto_sha256(input)\n}\n",
    ),
    (
        "crypto_mac.ax",
        "pub fn hmac_sha256(key: string, message: string): string {\nreturn crypto_hmac_sha256(key, message)\n}\n\
pub fn hmac_sha512(key: string, message: string): string {\nreturn crypto_hmac_sha512(key, message)\n}\n\
pub fn constant_time_eq(left: string, right: string): bool {\nreturn crypto_constant_time_eq(left, right)\n}\n\
pub fn constant_time_eq_u8(left: &[u8], right: &[u8]): bool {\nreturn crypto_constant_time_eq_u8(left, right)\n}\n\
pub fn verify_sha256(tag: string, key: string, message: string): bool {\nreturn constant_time_eq(tag, hmac_sha256(key, message))\n}\n\
pub fn verify_sha512(tag: string, key: string, message: string): bool {\nreturn constant_time_eq(tag, hmac_sha512(key, message))\n}\n",
    ),
    (
        "crypto_rand.ax",
        "pub fn random_bytes(n: int): [u8] {\nreturn crypto_rand_bytes(n)\n}\n\
pub fn random_u64(): u64 {\nreturn crypto_rand_u64()\n}\n",
    ),
    (
        "crypto_aead.ax",
        "pub enum AeadAlgorithm {\nAes128Gcm\nAes256Gcm\nChaCha20Poly1305\n}\n\
pub fn aead_algorithm_name(alg: AeadAlgorithm): string {\nmatch alg {\nAes128Gcm {\nreturn \"AES-128-GCM\"\n}\nAes256Gcm {\nreturn \"AES-256-GCM\"\n}\nChaCha20Poly1305 {\nreturn \"CHACHA20-POLY1305\"\n}\n}\n}\n\
pub fn aead_seal(alg: AeadAlgorithm, key: &[u8], nonce: &[u8], aad: &[u8], plaintext: &[u8]): [u8] {\nreturn crypto_aead_seal(aead_algorithm_name(alg), key, nonce, aad, plaintext)\n}\n\
pub fn aead_open(alg: AeadAlgorithm, key: &[u8], nonce: &[u8], aad: &[u8], ciphertext: &[u8]): Option<[u8]> {\nreturn crypto_aead_open(aead_algorithm_name(alg), key, nonce, aad, ciphertext)\n}\n",
    ),
    (
        "crypto_sign.ax",
        "pub fn ed25519_keygen(): ([u8], [u8]) {\nreturn crypto_ed25519_keygen()\n}\n\
pub fn ed25519_sign(secret_key: &[u8], message: &[u8]): [u8] {\nreturn crypto_ed25519_sign(secret_key, message)\n}\n\
pub fn ed25519_verify(public_key: &[u8], message: &[u8], signature: &[u8]): bool {\nreturn crypto_ed25519_verify(public_key, message, signature)\n}\n",
    ),
    (
        "crypto.ax",
        "pub enum AeadAlgorithm {\nAes128Gcm\nAes256Gcm\nChaCha20Poly1305\n}\n\
pub fn aead_algorithm_name(alg: AeadAlgorithm): string {\nmatch alg {\nAes128Gcm {\nreturn \"AES-128-GCM\"\n}\nAes256Gcm {\nreturn \"AES-256-GCM\"\n}\nChaCha20Poly1305 {\nreturn \"CHACHA20-POLY1305\"\n}\n}\n}\n\
pub fn sha256(input: string): string {\nreturn crypto_sha256(input)\n}\n\
pub fn hmac_sha256(key: string, message: string): string {\nreturn crypto_hmac_sha256(key, message)\n}\n\
pub fn hmac_sha512(key: string, message: string): string {\nreturn crypto_hmac_sha512(key, message)\n}\n\
pub fn constant_time_eq(left: string, right: string): bool {\nreturn crypto_constant_time_eq(left, right)\n}\n\
pub fn constant_time_eq_u8(left: &[u8], right: &[u8]): bool {\nreturn crypto_constant_time_eq_u8(left, right)\n}\n\
pub fn verify_sha256(tag: string, key: string, message: string): bool {\nreturn constant_time_eq(tag, hmac_sha256(key, message))\n}\n\
pub fn verify_sha512(tag: string, key: string, message: string): bool {\nreturn constant_time_eq(tag, hmac_sha512(key, message))\n}\n\
pub fn random_bytes(n: int): [u8] {\nreturn crypto_rand_bytes(n)\n}\n\
pub fn random_u64(): u64 {\nreturn crypto_rand_u64()\n}\n\
pub fn aead_seal(alg: AeadAlgorithm, key: &[u8], nonce: &[u8], aad: &[u8], plaintext: &[u8]): [u8] {\nreturn crypto_aead_seal(aead_algorithm_name(alg), key, nonce, aad, plaintext)\n}\n\
pub fn aead_open(alg: AeadAlgorithm, key: &[u8], nonce: &[u8], aad: &[u8], ciphertext: &[u8]): Option<[u8]> {\nreturn crypto_aead_open(aead_algorithm_name(alg), key, nonce, aad, ciphertext)\n}\n\
pub fn ed25519_keygen(): ([u8], [u8]) {\nreturn crypto_ed25519_keygen()\n}\n\
pub fn ed25519_sign(secret_key: &[u8], message: &[u8]): [u8] {\nreturn crypto_ed25519_sign(secret_key, message)\n}\n\
pub fn ed25519_verify(public_key: &[u8], message: &[u8], signature: &[u8]): bool {\nreturn crypto_ed25519_verify(public_key, message, signature)\n}\n",
    ),
    (
        "io.ax",
        "pub fn eprintln(text: string): int {\nreturn io_eprintln(text)\n}\n\
pub fn readline(): Option<string> {\nreturn io_readline()\n}\n\
pub fn read_to_string(): string {\nreturn io_read_to_string()\n}\n",
    ),
    (
        "json.ax",
        "pub struct JsonValue {\nsource: string\n}\n\
pub fn parse_value(text: string): Option<JsonValue> {\nmatch json_parse_value(text) {\nSome(source) {\nreturn Some(JsonValue { source: source })\n}\nNone {\nreturn None\n}\n}\n}\n\
pub fn stringify_value(value: JsonValue): string {\nreturn json_stringify_value(value.source)\n}\n\
pub fn parse_int(text: string): Option<int> {\nreturn json_parse_int(text)\n}\n\
pub fn parse_bool(text: string): Option<bool> {\nreturn json_parse_bool(text)\n}\n\
pub fn parse_string(text: string): Option<string> {\nreturn json_parse_string(text)\n}\n\
pub fn parse_field_int(text: string, key: string): Option<int> {\nreturn json_parse_field_int(text, key)\n}\n\
pub fn parse_field_bool(text: string, key: string): Option<bool> {\nreturn json_parse_field_bool(text, key)\n}\n\
pub fn parse_field_string(text: string, key: string): Option<string> {\nreturn json_parse_field_string(text, key)\n}\n\
pub fn parse_field_value(value: JsonValue, key: string): Option<JsonValue> {\nmatch json_parse_field_value(value.source, key) {\nSome(source) {\nreturn Some(JsonValue { source: source })\n}\nNone {\nreturn None\n}\n}\n}\n\
pub fn stringify_int(value: int): string {\nreturn json_stringify_int(value)\n}\n\
pub fn stringify_bool(value: bool): string {\nreturn json_stringify_bool(value)\n}\n\
pub fn stringify_string(value: string): string {\nreturn json_stringify_string(value)\n}\n\
pub fn value_string(value: string): JsonValue {\nreturn JsonValue { source: json_stringify_string(value) }\n}\n\
pub fn value_int(value: int): JsonValue {\nreturn JsonValue { source: json_stringify_int(value) }\n}\n\
pub fn value_bool(value: bool): JsonValue {\nreturn JsonValue { source: json_stringify_bool(value) }\n}\n\
pub fn field_string(key: string, value: string): string {\nreturn json_stringify_string(key) + \":\" + json_stringify_string(value)\n}\n\
pub fn field_int(key: string, value: int): string {\nreturn json_stringify_string(key) + \":\" + json_stringify_int(value)\n}\n\
pub fn field_bool(key: string, value: bool): string {\nreturn json_stringify_string(key) + \":\" + json_stringify_bool(value)\n}\n\
pub fn field_value(key: string, value: JsonValue): string {\nreturn json_stringify_string(key) + \":\" + json_stringify_value(value.source)\n}\n\
pub fn schema_field_string(key: string): string {\nreturn json_stringify_string(key) + \":{\\\"type\\\":\\\"string\\\"}\"\n}\n\
pub fn schema_field_int(key: string): string {\nreturn json_stringify_string(key) + \":{\\\"type\\\":\\\"integer\\\"}\"\n}\n\
pub fn schema_field_bool(key: string): string {\nreturn json_stringify_string(key) + \":{\\\"type\\\":\\\"boolean\\\"}\"\n}\n\
pub fn schema_object1(field: string): string {\nreturn \"{\\\"type\\\":\\\"object\\\",\\\"properties\\\":{\" + field + \"}}\"\n}\n\
pub fn schema_object2(first_field: string, second_field: string): string {\nreturn \"{\\\"type\\\":\\\"object\\\",\\\"properties\\\":{\" + first_field + \",\" + second_field + \"}}\"\n}\n\
pub fn schema_object3(first_field: string, second_field: string, third_field: string): string {\nreturn \"{\\\"type\\\":\\\"object\\\",\\\"properties\\\":{\" + first_field + \",\" + second_field + \",\" + third_field + \"}}\"\n}\n\
pub fn object1(field: string): string {\nreturn \"{\" + field + \"}\"\n}\n\
pub fn object2(first_field: string, second_field: string): string {\nreturn \"{\" + first_field + \",\" + second_field + \"}\"\n}\n\
pub fn object3(first_field: string, second_field: string, third_field: string): string {\nreturn \"{\" + first_field + \",\" + second_field + \",\" + third_field + \"}\"\n}\n\
pub fn value_object1(field: string): JsonValue {\nreturn JsonValue { source: object1(field) }\n}\n\
pub fn value_object2(first_field: string, second_field: string): JsonValue {\nreturn JsonValue { source: object2(first_field, second_field) }\n}\n\
pub fn value_object3(first_field: string, second_field: string, third_field: string): JsonValue {\nreturn JsonValue { source: object3(first_field, second_field, third_field) }\n}\n\
pub fn array1(first: JsonValue): JsonValue {\nreturn JsonValue { source: \"[\" + json_stringify_value(first.source) + \"]\" }\n}\n\
pub fn array2(first: JsonValue, second: JsonValue): JsonValue {\nreturn JsonValue { source: \"[\" + json_stringify_value(first.source) + \",\" + json_stringify_value(second.source) + \"]\" }\n}\n\
pub fn array3(first: JsonValue, second: JsonValue, third: JsonValue): JsonValue {\nreturn JsonValue { source: \"[\" + json_stringify_value(first.source) + \",\" + json_stringify_value(second.source) + \",\" + json_stringify_value(third.source) + \"]\" }\n}\n",
    ),
    ("serdes.ax", include_str!("../../../stdlib/std/serdes.ax")),
    (
        "collections.ax",
        "pub fn count<T>(values: &[T]): int {\nreturn len(values)\n}\n\
pub fn is_empty<T>(values: &[T]): bool {\nreturn len(values) == 0\n}\n\
pub fn has_items<T>(values: &[T]): bool {\nreturn len(values) > 0\n}\n\
pub fn count_mut<T>(values: &mut [T]): int {\nreturn len(values)\n}\n\
pub fn skip<T>(values: &[T], count: int): &[T] {\nreturn values[count:]\n}\n\
pub fn take<T>(values: &[T], count: int): &[T] {\nreturn values[:count]\n}\n\
pub fn window<T>(values: &[T], start: int, end: int): &[T] {\nreturn values[start:end]\n}\n\
pub fn contains<K, V>(values: {K: V}, key: K): bool {\nreturn map_contains_key<K, V>(values, key)\n}\n\
pub fn get<K, V>(values: {K: V}, key: K): Option<V> {\nreturn map_get<K, V>(values, key)\n}\n\
pub fn get_or_default<K, V>(values: {K: V}, key: K, default: V): V {\nmatch map_get<K, V>(values, key) {\nSome(value) {\nreturn value\n}\nNone {\nreturn default\n}\n}\n}\n\
pub fn keys<K, V>(values: {K: V}): [K] {\nreturn map_keys<K, V>(values)\n}\n",
    ),
    (
        "string_builder.ax",
        "pub struct StringBuilder {\nvalue: string\n}\n\
pub fn builder(): StringBuilder {\nreturn StringBuilder { value: \"\" }\n}\n\
pub fn from_string(value: string): StringBuilder {\nreturn StringBuilder { value: value }\n}\n\
pub fn push_str(builder: StringBuilder, text: string): StringBuilder {\nreturn StringBuilder { value: builder.value + text }\n}\n\
pub fn push_line(builder: StringBuilder, text: string): StringBuilder {\nreturn StringBuilder { value: builder.value + text + \"\\n\" }\n}\n\
pub fn finish(builder: StringBuilder): string {\nreturn builder.value\n}\n",
    ),
    (
        "log.ax",
        "pub fn field_string(key: string, value: string): string {\nreturn json_stringify_string(key) + \":\" + json_stringify_string(value)\n}\n\
pub fn field_int(key: string, value: int): string {\nreturn json_stringify_string(key) + \":\" + json_stringify_int(value)\n}\n\
pub fn field_bool(key: string, value: bool): string {\nreturn json_stringify_string(key) + \":\" + json_stringify_bool(value)\n}\n\
pub fn fields2(first_field: string, second_field: string): string {\nreturn first_field + \",\" + second_field\n}\n\
pub fn fields3(first_field: string, second_field: string, third_field: string): string {\nreturn first_field + \",\" + second_field + \",\" + third_field\n}\n\
pub fn event(level: string, message: string, attributes: string): string {\nreturn \"{\\\"level\\\":\" + json_stringify_string(level) + \",\\\"message\\\":\" + json_stringify_string(message) + \",\\\"attributes\\\":{\" + attributes + \"}}\"\n}\n\
pub fn debug(message: string): int {\nreturn io_eprintln(event(\"debug\", message, \"\"))\n}\n\
pub fn info(message: string): int {\nreturn io_eprintln(event(\"info\", message, \"\"))\n}\n\
pub fn warn(message: string): int {\nreturn io_eprintln(event(\"warn\", message, \"\"))\n}\n\
pub fn error(message: string): int {\nreturn io_eprintln(event(\"error\", message, \"\"))\n}\n\
pub fn info_attrs(message: string, attributes: string): int {\nreturn io_eprintln(event(\"info\", message, attributes))\n}\n",
    ),
    (
        "sync.ax",
        "pub struct Mutex<T> {\nvalue: T\n}\n\
pub struct MutexGuard<T> {\nvalue: T\n}\n\
pub struct Once<T> {\nvalue: Option<T>\n}\n\
pub struct Channel<T> {\nslot: Option<T>\n}\n\
pub fn mutex<T>(value: T): Mutex<T> {\nreturn Mutex { value: value }\n}\n\
pub fn lock<T>(mutex: Mutex<T>): MutexGuard<T> {\nreturn MutexGuard { value: mutex.value }\n}\n\
pub fn replace<T>(_guard: MutexGuard<T>, value: T): Mutex<T> {\nreturn Mutex { value: value }\n}\n\
pub fn into_inner<T>(guard: MutexGuard<T>): T {\nreturn guard.value\n}\n\
pub fn once<T>(value: Option<T>): Once<T> {\nreturn Once { value: value }\n}\n\
pub fn once_with<T>(value: T): Once<T> {\nreturn Once { value: Some(value) }\n}\n\
pub fn once_is_set<T>(cell: Once<T>): bool {\nmatch cell.value {\nSome(_value) {\nreturn true\n}\nNone {\nreturn false\n}\n}\n}\n\
pub fn once_take<T>(cell: Once<T>): Option<T> {\nreturn cell.value\n}\n\
pub fn channel<T>(slot: Option<T>): Channel<T> {\nreturn Channel { slot: slot }\n}\n\
pub fn send<T>(_channel: Channel<T>, value: T): Channel<T> {\nreturn Channel { slot: Some(value) }\n}\n\
pub fn try_recv<T>(channel: Channel<T>): Option<T> {\nreturn channel.slot\n}\n",
    ),
    (
        "async.ax",
        "pub fn ready<T>(value: T): Task<T> {\nreturn async_ready<T>(value)\n}\n\
pub fn spawn<T>(task: Task<T>): JoinHandle<T> {\nreturn async_spawn<T>(task)\n}\n\
pub fn join<T>(handle: JoinHandle<T>): Task<T> {\nreturn async_join<T>(handle)\n}\n\
pub fn cancel<T>(task: Task<T>): Task<T> {\nreturn async_cancel<T>(task)\n}\n\
pub fn is_canceled<T>(task: Task<T>): bool {\nreturn async_is_canceled<T>(task)\n}\n\
pub fn timeout<T>(task: Task<T>, milliseconds: int): Task<Option<T>> {\nreturn async_timeout<T>(task, milliseconds)\n}\n\
pub fn channel<T>(): AsyncChannel<T> {\nreturn async_channel<T>()\n}\n\
pub fn send<T>(channel: AsyncChannel<T>, value: T): Task<AsyncChannel<T>> {\nreturn async_send<T>(channel, value)\n}\n\
pub fn recv<T>(channel: AsyncChannel<T>): Task<Option<T>> {\nreturn async_recv<T>(channel)\n}\n\
pub fn select<T>(left: Task<Option<T>>, right: Task<Option<T>>): Task<SelectResult<T>> {\nreturn async_select<T>(left, right)\n}\n\
pub fn selected<T>(result: SelectResult<T>): int {\nreturn async_selected<T>(result)\n}\n\
pub fn selected_value<T>(result: SelectResult<T>): Option<T> {\nreturn async_selected_value<T>(result)\n}\n",
    ),
    (
        "async_time.ax",
        "pub async fn sleep_ms(milliseconds: int): int {\nreturn clock_sleep_ms(milliseconds)\n}\n\
pub async fn sleep_duration_ms(milliseconds: int): int {\nreturn clock_sleep_ms(milliseconds)\n}\n",
    ),
    (
        "async_net.ax",
        "pub type TcpListener = int\n\
pub type TcpStream = int\n\
pub type UdpSocket = int\n\
pub async fn listen(bind: string): TcpListener {\nreturn net_tcp_listen(bind)\n}\n\
pub fn local_port(listener: TcpListener): int {\nreturn net_tcp_listener_port(listener)\n}\n\
pub async fn accept(listener: TcpListener): TcpStream {\nreturn net_tcp_accept(listener)\n}\n\
pub async fn recv_text(stream: TcpStream, max_bytes: int): string {\nreturn net_tcp_read_string(stream, max_bytes)\n}\n\
pub async fn send_text(stream: TcpStream, message: string): int {\nreturn net_tcp_write_string(stream, message)\n}\n\
pub fn close(stream: TcpStream): int {\nreturn net_tcp_close(stream)\n}\n\
pub fn close_listener(listener: TcpListener): int {\nreturn net_tcp_close_listener(listener)\n}\n\
pub async fn tcp_listen_loopback_once(response: string, timeout_ms: int): Option<int> {\nreturn net_tcp_listen_loopback_once(response, timeout_ms)\n}\n\
pub async fn tcp_dial(host: string, port: int, message: string, timeout_ms: int): Option<string> {\nreturn net_tcp_dial(host, port, message, timeout_ms)\n}\n\
pub async fn udp_bind_loopback_once(response: string, timeout_ms: int): Option<int> {\nreturn net_udp_bind_loopback_once(response, timeout_ms)\n}\n\
pub async fn udp_send_recv(host: string, port: int, message: string, timeout_ms: int): Option<string> {\nreturn net_udp_send_recv(host, port, message, timeout_ms)\n}\n",
    ),
    ("testing.ax", include_str!("../../../stdlib/std/testing.ax")),
    (
        "http.ax",
        "pub type Server = int
pub struct HttpHeader {
name: string
value: string
}
pub struct HttpRequest {
stream: int
method: string
path: string
body: string
}
pub struct HttpResponse {
status: int
body: string
headers: [HttpHeader]
}
pub struct HttpRoute {
path: string
response: HttpResponse
}
pub fn get(url: string): Option<string> {
return http_get(url)
}
pub fn listen(bind: string): Server {
return http_server_listen(bind)
}
pub fn local_port(server: Server): int {
return http_server_local_port(server)
}
pub fn accept(server: Server): HttpRequest {
let stream: int = http_server_accept(server)
return HttpRequest { stream: stream, method: http_request_method(stream), path: http_request_path(stream), body: http_request_body(stream) }
}
pub fn header(name: string, value: string): HttpHeader {
return HttpHeader { name: name, value: value }
}
pub fn request(method: string, path: string, body: string): HttpRequest {
return HttpRequest { stream: -1, method: method, path: path, body: body }
}
pub fn response(status: int, body: string, headers: [HttpHeader]): HttpResponse {
return HttpResponse { status: status, body: body, headers: headers }
}
pub fn text_response(status: int, body: string): HttpResponse {
return response(status, body, [header(\"content-type\", \"text/plain; charset=utf-8\")])
}
pub fn route(request: HttpRequest): string {
return request.path
}
pub fn fixed_route(path: string, body: string): HttpRoute {
return HttpRoute { path: path, response: text_response(200, body) }
}
pub fn route_response(path: string, selected_response: HttpResponse): HttpRoute {
return HttpRoute { path: path, response: selected_response }
}
pub fn respond(request: HttpRequest, status: int, body: string): bool {
return http_response_write(request.stream, status, body)
}
pub fn serve(bind: string, selected_route: HttpRoute, max_requests: int): bool {
return http_serve_route(bind, selected_route.path, selected_route.response.body, max_requests)
}
pub fn serve_once(bind: string, body: string): bool {
return http_serve_once(bind, body)
}
pub fn close(server: Server): bool {
return http_server_close(server)
}
",
    ),
    (
        "http_async.ax",
        "pub fn async_serve_route(server: int, path: string, body: string, max_requests: int): Task<bool> {\nreturn http_async_serve_route(server, path, body, max_requests)\n}\n",
    ),
    (
        "regex.ax",
        "pub fn is_match(pattern: string, text: string): bool {\nreturn regex_is_match(pattern, text)\n}\n\
pub fn find(pattern: string, text: string): Option<string> {\nreturn regex_find(pattern, text)\n}\n\
pub fn replace_all(pattern: string, text: string, replacement: string): string {\nreturn regex_replace_all(pattern, text, replacement)\n}\n",
    ),
    (
        "encoding.ax",
        "pub fn url_component_encode(value: string): string {
return encoding_url_component_encode(value)
}
pub fn url_component_decode(value: string): Option<string> {
return encoding_url_component_decode(value)
}
pub fn path_segment_encode(value: string): string {
return encoding_path_segment_encode(value)
}
pub fn query_pair_encode(name: string, value: string): string {
return encoding_url_query_pair_encode(name, value)
}
pub fn path_join_segment(base: string, segment: string): string {
return encoding_path_join_segment(base, segment)
}
",
    ),
    (
        "outcome.ax",
        "pub fn option_is_some<T>(value: Option<T>): bool {\nmatch value {\nSome(_inner) {\nreturn true\n}\nNone {\nreturn false\n}\n}\n}\n\
pub fn option_is_none<T>(value: Option<T>): bool {\nmatch value {\nSome(_inner) {\nreturn false\n}\nNone {\nreturn true\n}\n}\n}\n\
pub fn option_unwrap_or<T>(value: Option<T>, fallback: T): T {\nmatch value {\nSome(inner) {\nreturn inner\n}\nNone {\nreturn fallback\n}\n}\n}\n\
pub fn result_is_ok<T, E>(value: Result<T, E>): bool {\nmatch value {\nOk(_inner) {\nreturn true\n}\nErr(_error) {\nreturn false\n}\n}\n}\n\
pub fn result_is_err<T, E>(value: Result<T, E>): bool {\nmatch value {\nOk(_inner) {\nreturn false\n}\nErr(_error) {\nreturn true\n}\n}\n}\n\
pub fn result_unwrap_or<T, E>(value: Result<T, E>, fallback: T): T {\nmatch value {\nOk(inner) {\nreturn inner\n}\nErr(_error) {\nreturn fallback\n}\n}\n}\n",
    ),
    (
        "cli.ax",
        "pub fn args(): [string] {\nreturn cli_args()\n}\n\
pub fn arg_count(): int {\nreturn cli_arg_count()\n}\n\
pub fn arg(index: int): Option<string> {\nreturn cli_arg(index)\n}\n",
    ),
];

pub(crate) fn stdlib_root() -> PathBuf {
    PathBuf::from(STDLIB_ROOT)
}

pub(crate) fn is_stdlib_path(path: &Path) -> bool {
    path.starts_with(Path::new(STDLIB_ROOT))
}

/// Returns the virtual module path for `module_relative`, e.g.
/// `"time.ax"` -> `<stdlib>/time.ax`.
pub(crate) fn stdlib_source_path(module_relative: &str) -> PathBuf {
    PathBuf::from(STDLIB_ROOT).join(module_relative)
}

/// Returns the embedded source for a virtual stdlib path, or `None` if the
/// path does not correspond to a known stdlib module.
pub(crate) fn stdlib_source_for(path: &Path) -> Option<&'static str> {
    let relative = path.strip_prefix(Path::new(STDLIB_ROOT)).ok()?;
    let key = relative.to_str()?;
    STDLIB_SOURCES
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, source)| *source)
}

/// Returns the virtual module key (e.g. `"time.ax"`) used by a stdlib import
/// remainder. `import_remainder` is the portion of the user-visible import
/// path that follows the `std/` prefix (e.g. `"time.ax"`).
pub(crate) fn stdlib_has_module(import_remainder: &Path) -> bool {
    let Some(key) = import_remainder.to_str() else {
        return false;
    };
    STDLIB_SOURCES.iter().any(|(name, _)| *name == key)
}

pub(crate) fn stdlib_module_names() -> impl Iterator<Item = &'static str> {
    STDLIB_SOURCES.iter().map(|(name, _)| *name)
}
