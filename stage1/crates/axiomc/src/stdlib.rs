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
//! Today this provides twelve stdlib modules. Six are thin wrappers over
//! single-intrinsic capability-gated surfaces, one per capability class:
//!
//! * `std/time.ax` — `Duration`, `Instant`, `now_ms()`, `now()`,
//!   `elapsed_ms(start)`, and `sleep(duration)` on top of `clock_now_ms`,
//!   `clock_elapsed_ms`, and `clock_sleep_ms` (clock).
//! * `std/env.ax` — `get_env(key)` on top of `env_get` (env).
//! * `std/fs.ax` — `read_file(path)` on top of `fs_read` (fs).
//! * `std/net.ax` — `resolve(host)` on top of `net_resolve` (net).
//! * `std/process.ax` — `run_status(command)` on top of `process_status`
//!   (process).
//! * `std/crypto_hash.ax` — `sha256(input)` on top of `crypto_sha256` (crypto).
//!   (This is the stage1 spelling of the `std.crypto.hash` module from the
//!   AG4.1 plan; stage1 uses a flat filename to avoid cross-platform path
//!   separator issues in the virtual stdlib table.)
//!
//! The seventh module shares an existing capability class with a peer
//! wrapper, demonstrating that the `std.*` surface is not limited to one
//! wrapper per capability:
//!
//! * `std/http.ax` — `get(url)` and `serve_once(bind, body)` on top of the new
//!   `http_get` and `http_serve_once` intrinsics. HTTP shares the `net`
//!   capability surface because any code that can open a raw TCP socket could
//!   implement HTTP itself, so a separate `http` manifest flag would not add
//!   meaningful isolation in stage1. The stage1 client supports both http://
//!   and https:// URLs; the server serves one blocking HTTP/1.0 response and
//!   then exits.
//!
//! The eighth, ninth, tenth, eleventh, and twelfth modules are stdlib surfaces not tied to a
//! capability flag, matching the ambient status of the `print` statement:
//!
//! * `std/io.ax` — `eprintln(text)` on top of the new ungated `io_eprintln`
//!   intrinsic (writes a line to stderr and returns bytes written).
//! * `std/json.ax` — scalar/string JSON parsing and serialization helpers on
//!   top of new ungated `json_parse_*` / `json_stringify_*` intrinsics.
//! * `std/collections.ax` — generic borrowed-slice helpers built on the
//!   existing polymorphic collection primitives and AG2 generic functions.
//! * `std/sync.ax` — ownership-shaped synchronization primitives implemented
//!   in Axiom: move-only mutex guards, one-shot cells, and single-slot
//!   nonblocking channels.
//! * `std/async.ax` — deterministic task, join, channel, timeout,
//!   cancellation, and select wrappers over the stage1 async runtime values.

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
/// the stdlib import prefix. Keeping stage1 stdlib sources in-tree as `&str`
/// avoids any filesystem lookup and keeps the bootstrap hermetic.
const STDLIB_SOURCES: &[(&str, &str)] = &[
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
        "pub fn read_file(path: string): Option<string> {\nreturn fs_read(path)\n}\n",
    ),
    (
        "net.ax",
        "pub fn resolve(host: string): Option<string> {\nreturn net_resolve(host)\n}\n",
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
        "io.ax",
        "pub fn eprintln(text: string): int {\nreturn io_eprintln(text)\n}\n",
    ),
    (
        "json.ax",
        "pub fn parse_int(text: string): Option<int> {\nreturn json_parse_int(text)\n}\n\
pub fn parse_bool(text: string): Option<bool> {\nreturn json_parse_bool(text)\n}\n\
pub fn parse_string(text: string): Option<string> {\nreturn json_parse_string(text)\n}\n\
pub fn stringify_int(value: int): string {\nreturn json_stringify_int(value)\n}\n\
pub fn stringify_bool(value: bool): string {\nreturn json_stringify_bool(value)\n}\n\
pub fn stringify_string(value: string): string {\nreturn json_stringify_string(value)\n}\n",
    ),
    (
        "collections.ax",
        "pub fn count<T>(values: &[T]): int {\nreturn len(values)\n}\n\
pub fn is_empty<T>(values: &[T]): bool {\nreturn len(values) == 0\n}\n\
pub fn has_items<T>(values: &[T]): bool {\nreturn len(values) > 0\n}\n\
pub fn skip<T>(values: &[T], count: int): &[T] {\nreturn values[count:]\n}\n\
pub fn take<T>(values: &[T], count: int): &[T] {\nreturn values[:count]\n}\n\
pub fn window<T>(values: &[T], start: int, end: int): &[T] {\nreturn values[start:end]\n}\n",
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
        "http.ax",
        "pub fn get(url: string): Option<string> {\nreturn http_get(url)\n}\n\
pub fn serve_once(bind: string, body: string): bool {\nreturn http_serve_once(bind, body)\n}\n",
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
