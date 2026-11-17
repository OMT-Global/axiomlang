//! `registry-serve` accept-loop resilience tests (issue #1780).

use super::super::serve::{
    MAX_REGISTRY_REQUEST_HEAD_BYTES, RegistryServeCounts, registry_request_head_end,
    serve_registry_listener,
};
use super::*;

/// Runs the accept loop on an ephemeral port while `client` talks to it,
/// then sends one well-formed request. The loop stops after `answered`
/// responses, counting the follow-up.
fn serve_then_follow_up(
    answered: usize,
    io_timeout: std::time::Duration,
    client: impl FnOnce(std::net::SocketAddr) + Send,
) -> (RegistryServeCounts, String) {
    let dir = tempdir().unwrap();
    let (fixture, index, registry) = publish_and_index(dir.path());
    let context =
        RegistryServeContext::new(&registry, &index, &fixture.roots, &fixture.expectation)
            .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::scope(|scope| {
        let server = scope
            .spawn(|| serve_registry_listener(&listener, &context, Some(answered), io_timeout));
        let follow_up = scope.spawn(move || {
            client(addr);
            let mut stream = TcpStream::connect(addr).unwrap();
            stream
                .write_all(b"GET /index.json HTTP/1.1\r\nhost: registry\r\n\r\n")
                .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            response
        });
        let response = follow_up.join().unwrap();
        (server.join().unwrap().unwrap(), response)
    })
}

fn read_response(stream: &mut TcpStream) -> String {
    let mut response = String::new();
    // A reset after the response is still a readable response.
    let _ = stream.read_to_string(&mut response);
    response
}

const SHORT_SERVE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(200);

#[test]
fn serve_survives_an_idle_connection_and_answers_the_next_request() {
    let (counts, response) = serve_then_follow_up(1, SHORT_SERVE_TIMEOUT, |addr| {
        let mut idle = TcpStream::connect(addr).unwrap();
        // Hold the connection open, silent, until the server gives up on it.
        assert_eq!(read_response(&mut idle), "");
    });
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert_eq!(counts.requests, 1);
    assert_eq!(counts.dropped_connections, 1);
}

#[test]
fn serve_survives_a_client_that_connects_and_closes_immediately() {
    let (counts, response) = serve_then_follow_up(1, SHORT_SERVE_TIMEOUT, |addr| {
        drop(TcpStream::connect(addr).unwrap());
    });
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert_eq!(counts.requests, 1);
    assert_eq!(counts.dropped_connections, 1);
}

#[test]
fn serve_answers_consecutive_connections() {
    // Send a request and read the response in full, so the server's write
    // has completed, then answer a second connection.
    let (counts, response) = serve_then_follow_up(2, SHORT_SERVE_TIMEOUT, |addr| {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .write_all(b"HEAD /index.json HTTP/1.1\r\n\r\n")
            .unwrap();
        assert!(read_response(&mut stream).starts_with("HTTP/1.1 200 OK"));
    });
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert_eq!(
        counts,
        RegistryServeCounts {
            requests: 2,
            dropped_connections: 0
        }
    );
}

#[test]
fn serve_reads_a_request_head_split_across_segments() {
    let dir = tempdir().unwrap();
    let (fixture, index, registry) = publish_and_index(dir.path());
    let context =
        RegistryServeContext::new(&registry, &index, &fixture.roots, &fixture.expectation)
            .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (counts, response) = std::thread::scope(|scope| {
        let server = scope.spawn(|| {
            serve_registry_listener(
                &listener,
                &context,
                Some(1),
                std::time::Duration::from_secs(5),
            )
        });
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.set_nodelay(true).unwrap();
        for part in [
            &b"GET /axiom/core/1.2.3/pack"[..],
            b"age.axp HTT",
            b"P/1.1\r\nhost: registry\r\n",
            b"\r\n",
        ] {
            stream.write_all(part).unwrap();
            stream.flush().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(40));
        }
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        (server.join().unwrap().unwrap(), response)
    });
    let archive = fs::read(registry.join("axiom/core/1.2.3/package.axp")).unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200 OK"));
    assert!(response.ends_with(&archive));
    assert_eq!(
        counts,
        RegistryServeCounts {
            requests: 1,
            dropped_connections: 0
        }
    );
}

#[test]
fn serve_rejects_an_oversize_request_head_and_keeps_serving() {
    let (counts, response) =
        serve_then_follow_up(2, std::time::Duration::from_secs(5), |addr| {
            let mut stream = TcpStream::connect(addr).unwrap();
            let mut head = b"GET /index.json HTTP/1.1\r\nx-padding: ".to_vec();
            head.resize(MAX_REGISTRY_REQUEST_HEAD_BYTES, b'a');
            stream.write_all(&head).unwrap();
            let rejected = read_response(&mut stream);
            assert!(
                rejected.starts_with("HTTP/1.1 431 Request Header Fields Too Large"),
                "{rejected}"
            );
        });
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert_eq!(
        counts,
        RegistryServeCounts {
            requests: 2,
            dropped_connections: 0
        }
    );
}

#[test]
fn serve_times_out_a_partial_request_head_with_408() {
    let (counts, response) = serve_then_follow_up(2, SHORT_SERVE_TIMEOUT, |addr| {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.write_all(b"GET /index.json HTT").unwrap();
        let stalled = read_response(&mut stream);
        assert!(
            stalled.starts_with("HTTP/1.1 408 Request Timeout"),
            "{stalled}"
        );
    });
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert_eq!(
        counts,
        RegistryServeCounts {
            requests: 2,
            dropped_connections: 0
        }
    );
}

#[test]
fn request_head_end_accepts_crlf_and_bare_lf_terminators() {
    assert_eq!(
        registry_request_head_end(b"GET / HTTP/1.1\r\n\r\nbody"),
        Some(18)
    );
    assert_eq!(
        registry_request_head_end(b"GET / HTTP/1.1\n\nbody"),
        Some(16)
    );
    assert_eq!(
        registry_request_head_end(b"GET / HTTP/1.1\r\nhost: a\r\n"),
        None
    );
    assert_eq!(registry_request_head_end(b""), None);
}
