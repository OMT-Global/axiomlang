//! Accept loop and request-head reader for `axiomc registry-serve`.
//!
//! Nothing a client does is fatal to the server: per-connection I/O errors
//! are counted and logged, and only a listener-level failure ends the loop.

use super::{
    RegistryServeContext, registry_error, registry_http_error, registry_http_response,
    write_registry_http_response,
};
use crate::diagnostics::Diagnostic;
use std::io::Read;
use std::net::{TcpListener, TcpStream};

pub(super) const MAX_REGISTRY_REQUEST_HEAD_BYTES: usize = 16 * 1024;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct RegistryServeCounts {
    pub(super) requests: usize,
    pub(super) dropped_connections: usize,
}

/// Outcome of one accepted connection. Nothing a client does is fatal to the
/// server; only a listener-level failure ends the accept loop.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum RegistryConnectionOutcome {
    /// A complete HTTP response (of any status) was written.
    Answered,
    /// The connection was abandoned without a complete response.
    Dropped(String),
}

/// Serves connections one at a time until the listener fails or
/// `max_requests` requests have been answered.
///
/// Each connection gets at most `io_timeout` to deliver its request head and
/// `io_timeout` per write, which bounds how long an idle or stalled client can
/// delay the next one.
pub(super) fn serve_registry_listener(
    listener: &TcpListener,
    context: &RegistryServeContext,
    max_requests: Option<usize>,
    io_timeout: std::time::Duration,
) -> Result<RegistryServeCounts, Diagnostic> {
    let mut counts = RegistryServeCounts::default();
    loop {
        let (mut stream, peer) = match listener.accept() {
            Ok(accepted) => accepted,
            Err(error) if registry_accept_error_is_transient(&error) => {
                counts.dropped_connections += 1;
                eprintln!("registry-serve: dropped connection before accept completed: {error}");
                continue;
            }
            Err(error) => {
                return Err(registry_error(
                    None,
                    format!("failed to accept request: {error}"),
                ));
            }
        };
        match serve_registry_stream(context, &mut stream, io_timeout) {
            RegistryConnectionOutcome::Answered => {
                counts.requests += 1;
                if max_requests.is_some_and(|max| counts.requests >= max) {
                    return Ok(counts);
                }
            }
            RegistryConnectionOutcome::Dropped(reason) => {
                counts.dropped_connections += 1;
                eprintln!("registry-serve: dropped connection from {peer}: {reason}");
            }
        }
    }
}

fn registry_accept_error_is_transient(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::TimedOut
    )
}

pub(super) fn serve_registry_stream(
    context: &RegistryServeContext,
    stream: &mut TcpStream,
    io_timeout: std::time::Duration,
) -> RegistryConnectionOutcome {
    if let Err(error) = stream.set_write_timeout(Some(io_timeout)) {
        return RegistryConnectionOutcome::Dropped(format!("failed to set write timeout: {error}"));
    }
    let response = match read_registry_request_head(stream, io_timeout) {
        RegistryRequestHead::Complete(head) => {
            registry_http_response(context, &String::from_utf8_lossy(&head))
        }
        RegistryRequestHead::TooLarge => registry_http_error(
            "431 Request Header Fields Too Large",
            "request head too large",
        ),
        RegistryRequestHead::Incomplete => {
            registry_http_error("408 Request Timeout", "incomplete request head")
        }
        RegistryRequestHead::Silent => {
            return RegistryConnectionOutcome::Dropped("client sent no request".to_owned());
        }
        RegistryRequestHead::Failed(error) => {
            return RegistryConnectionOutcome::Dropped(format!("failed to read request: {error}"));
        }
    };
    match write_registry_http_response(stream, &response) {
        Ok(()) => RegistryConnectionOutcome::Answered,
        Err(error) => {
            RegistryConnectionOutcome::Dropped(format!("failed to write response: {error}"))
        }
    }
}

pub(super) enum RegistryRequestHead {
    /// The bytes up to and including the blank line that ends the head.
    Complete(Vec<u8>),
    /// No head terminator within `MAX_REGISTRY_REQUEST_HEAD_BYTES`.
    TooLarge,
    /// Some bytes arrived, but the head did not finish before EOF or the deadline.
    Incomplete,
    /// The client connected and sent nothing before EOF or the deadline.
    Silent,
    Failed(std::io::Error),
}

/// Reads one request head, across as many TCP segments as it takes, under a
/// byte cap and a single deadline for the whole head.
pub(super) fn read_registry_request_head(
    stream: &mut TcpStream,
    timeout: std::time::Duration,
) -> RegistryRequestHead {
    let deadline = std::time::Instant::now() + timeout;
    let mut head = Vec::new();
    let mut buffer = [0_u8; 4 * 1024];
    loop {
        if let Some(end) = registry_request_head_end(&head) {
            head.truncate(end);
            return RegistryRequestHead::Complete(head);
        }
        if head.len() >= MAX_REGISTRY_REQUEST_HEAD_BYTES {
            return RegistryRequestHead::TooLarge;
        }
        let unfinished = |head: &[u8]| {
            if head.is_empty() {
                RegistryRequestHead::Silent
            } else {
                RegistryRequestHead::Incomplete
            }
        };
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return unfinished(&head);
        }
        if let Err(error) = stream.set_read_timeout(Some(remaining)) {
            return RegistryRequestHead::Failed(error);
        }
        let room = (MAX_REGISTRY_REQUEST_HEAD_BYTES - head.len()).min(buffer.len());
        match stream.read(&mut buffer[..room]) {
            Ok(0) => return unfinished(&head),
            Ok(length) => head.extend_from_slice(&buffer[..length]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return unfinished(&head);
            }
            Err(error) => return RegistryRequestHead::Failed(error),
        }
    }
}

/// Index just past the blank line that terminates an HTTP request head.
pub(super) fn registry_request_head_end(head: &[u8]) -> Option<usize> {
    let crlf = head
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|at| at + 4);
    let lf = head
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|at| at + 2);
    match (crlf, lf) {
        (Some(crlf), Some(lf)) => Some(crlf.min(lf)),
        (found, None) | (None, found) => found,
    }
}
