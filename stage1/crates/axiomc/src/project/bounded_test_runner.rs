use super::{
    RunLimits, command_for_build_output, normalize_http_fixture_path, parse_http_fixture_bind,
};
use std::io::ErrorKind;
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

pub(super) struct BoundedCommandOutput {
    pub(super) status: std::process::ExitStatus,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
}

struct BoundedChildProcess {
    child: std::process::Child,
    stdout_reader: Option<thread::JoinHandle<io::Result<Vec<u8>>>>,
    stderr_reader: Option<thread::JoinHandle<io::Result<Vec<u8>>>>,
    stdin_writer: Option<thread::JoinHandle<io::Result<()>>>,
    output_bytes: Arc<AtomicUsize>,
    max_output_bytes: usize,
}

pub(super) fn run_test_command(
    command: &mut Command,
    limits: Option<RunLimits>,
) -> io::Result<std::process::Output> {
    match limits {
        Some(limits) => run_bounded_command(command, limits).map(|output| std::process::Output {
            status: output.status,
            stdout: output.stdout,
            stderr: output.stderr,
        }),
        None => command.output(),
    }
}

pub(super) fn run_bounded_command(
    command: &mut Command,
    limits: RunLimits,
) -> io::Result<BoundedCommandOutput> {
    run_bounded_command_with_stdin(command, limits, None)
}

pub(super) fn run_bounded_command_with_stdin(
    command: &mut Command,
    limits: RunLimits,
    stdin: Option<&[u8]>,
) -> io::Result<BoundedCommandOutput> {
    let process = spawn_bounded_child(command, limits, stdin)?;
    finish_bounded_child(process, limits.timeout)
}

pub(super) fn run_command_with_stdin(
    mut command: Command,
    stdin: &str,
    limits: Option<RunLimits>,
) -> io::Result<std::process::Output> {
    if let Some(limits) = limits {
        let output = run_bounded_command_with_stdin(&mut command, limits, Some(stdin.as_bytes()))?;
        return Ok(std::process::Output {
            status: output.status,
            stdout: output.stdout,
            stderr: output.stderr,
        });
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    if let Some(mut input) = child.stdin.take() {
        input.write_all(stdin.as_bytes())?;
    }
    child.wait_with_output()
}

fn spawn_bounded_child(
    command: &mut Command,
    limits: RunLimits,
    stdin: Option<&[u8]>,
) -> io::Result<BoundedChildProcess> {
    command
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_bounded_command(command, limits);
    let mut child = command.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("bounded command stdout pipe unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("bounded command stderr pipe unavailable"))?;
    let output_bytes = Arc::new(AtomicUsize::new(0));
    let stdout_reader =
        capture_bounded_stream(stdout, limits.max_output_bytes, Arc::clone(&output_bytes));
    let stderr_reader =
        capture_bounded_stream(stderr, limits.max_output_bytes, Arc::clone(&output_bytes));
    let stdin_writer = if let Some(input) = stdin {
        let mut pipe = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("bounded command stdin pipe unavailable"))?;
        let input = input.to_vec();
        Some(thread::spawn(move || pipe.write_all(&input)))
    } else {
        None
    };
    Ok(BoundedChildProcess {
        child,
        stdout_reader: Some(stdout_reader),
        stderr_reader: Some(stderr_reader),
        stdin_writer,
        output_bytes,
        max_output_bytes: limits.max_output_bytes,
    })
}

fn finish_bounded_child(
    mut process: BoundedChildProcess,
    timeout: Duration,
) -> io::Result<BoundedCommandOutput> {
    let deadline = Instant::now() + timeout;
    let status = loop {
        if process.output_bytes.load(Ordering::Relaxed) > process.max_output_bytes {
            stop_bounded_child(&mut process);
            return Err(io::Error::other(format!(
                "output exceeded the {} byte limit",
                process.max_output_bytes
            )));
        }
        if let Some(status) = process.child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            stop_bounded_child(&mut process);
            return Err(io::Error::new(
                ErrorKind::TimedOut,
                format!("execution exceeded the {:?} timeout", timeout),
            ));
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = join_bounded_stream(
        process
            .stdout_reader
            .take()
            .ok_or_else(|| io::Error::other("bounded command stdout reader unavailable"))?,
    )?;
    let stderr = join_bounded_stream(
        process
            .stderr_reader
            .take()
            .ok_or_else(|| io::Error::other("bounded command stderr reader unavailable"))?,
    )?;
    if let Some(writer) = process.stdin_writer.take() {
        writer
            .join()
            .map_err(|_| io::Error::other("bounded command stdin writer panicked"))??;
    }
    if process.output_bytes.load(Ordering::Relaxed) > process.max_output_bytes {
        return Err(io::Error::other(format!(
            "output exceeded the {} byte limit",
            process.max_output_bytes
        )));
    }
    Ok(BoundedCommandOutput {
        status,
        stdout,
        stderr,
    })
}

fn stop_bounded_child(process: &mut BoundedChildProcess) {
    terminate_bounded_child(&mut process.child);
    if let Some(reader) = process.stdout_reader.take() {
        let _ = reader.join();
    }
    if let Some(reader) = process.stderr_reader.take() {
        let _ = reader.join();
    }
    if let Some(writer) = process.stdin_writer.take() {
        let _ = writer.join();
    }
}

fn capture_bounded_stream<R>(
    mut stream: R,
    limit: usize,
    output_bytes: Arc<AtomicUsize>,
) -> thread::JoinHandle<io::Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut captured = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let read = stream.read(&mut buffer)?;
            if read == 0 {
                return Ok(captured);
            }
            let previous = output_bytes.fetch_add(read, Ordering::Relaxed);
            if previous < limit {
                let remaining = limit - previous;
                captured.extend_from_slice(&buffer[..read.min(remaining)]);
            }
        }
    })
}

fn join_bounded_stream(reader: thread::JoinHandle<io::Result<Vec<u8>>>) -> io::Result<Vec<u8>> {
    reader
        .join()
        .map_err(|_| io::Error::other("bounded command output reader panicked"))?
}

#[cfg(unix)]
fn configure_bounded_command(command: &mut Command, limits: RunLimits) {
    use std::os::unix::process::CommandExt;

    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            set_bounded_rlimit(limits.max_cpu_seconds, |rlimit| {
                libc::setrlimit(libc::RLIMIT_CPU, rlimit)
            })?;
            set_bounded_rlimit(limits.max_file_bytes, |rlimit| {
                libc::setrlimit(libc::RLIMIT_FSIZE, rlimit)
            })?;
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn configure_bounded_command(_command: &mut Command, _limits: RunLimits) {}

#[cfg(unix)]
fn set_bounded_rlimit(
    limit: u64,
    set_limit: impl FnOnce(*const libc::rlimit) -> libc::c_int,
) -> io::Result<()> {
    let limits = libc::rlimit {
        rlim_cur: limit as libc::rlim_t,
        rlim_max: limit as libc::rlim_t,
    };
    if set_limit(&limits) != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn terminate_bounded_child(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let process_group = -(child.id() as i32);
        unsafe {
            libc::kill(process_group, libc::SIGTERM);
        }
        let grace_deadline = Instant::now() + Duration::from_millis(100);
        while Instant::now() < grace_deadline {
            if child.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        unsafe {
            libc::kill(process_group, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    let _ = child.kill();
    let _ = child.wait();
}

pub(super) fn run_bounded_http_fixture_case(
    binary: &std::path::Path,
    build_output_dir: &std::path::Path,
    test: &crate::manifest::TestTarget,
    limits: RunLimits,
) -> io::Result<std::process::Output> {
    let fixture = test.http.as_ref().expect("http fixture present");
    let (target_addr, injected_bind) = if let Some(bind) = &fixture.bind {
        (parse_http_fixture_bind(bind)?, None)
    } else {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
        let target_addr = listener.local_addr()?;
        drop(listener);
        let bind = target_addr.to_string();
        (target_addr, Some(bind))
    };
    let path = normalize_http_fixture_path(&fixture.path)?;

    let mut command = command_for_build_output(binary, build_output_dir)?;
    if let Some(bind) = injected_bind {
        command.env("AXIOM_TEST_BIND", bind);
    }
    let mut process = spawn_bounded_child(&mut command, limits, None)?;
    let deadline = Instant::now() + limits.timeout;
    let mut stream = loop {
        if Instant::now() >= deadline {
            stop_bounded_child(&mut process);
            return Err(io::Error::new(
                ErrorKind::TimedOut,
                format!(
                    "http fixture service never became ready within {:?}",
                    limits.timeout
                ),
            ));
        }
        if process.child.try_wait()?.is_some() {
            let output = finish_bounded_child(process, bounded_remaining(deadline))?;
            return Err(io::Error::other(format!(
                "http fixture service exited before becoming ready: {}",
                output.status
            )));
        }
        match std::net::TcpStream::connect(target_addr) {
            Ok(stream) => break stream,
            Err(err) => {
                let _ = err;
                thread::sleep(Duration::from_millis(25).min(bounded_remaining(deadline)));
            }
        }
    };
    let socket_timeout = bounded_remaining(deadline);
    if socket_timeout.is_zero() {
        stop_bounded_child(&mut process);
        return Err(io::Error::new(
            ErrorKind::TimedOut,
            "http fixture request exceeded its execution timeout",
        ));
    }
    if let Err(err) = stream
        .set_read_timeout(Some(socket_timeout))
        .and_then(|_| stream.set_write_timeout(Some(socket_timeout)))
        .and_then(|_| {
            stream.write_all(format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
        })
    {
        stop_bounded_child(&mut process);
        return Err(err);
    }
    let response =
        match read_bounded_http_response_until(&mut stream, limits.max_output_bytes, deadline) {
            Ok(response) => response,
            Err(err) => {
                stop_bounded_child(&mut process);
                return Err(err);
            }
        };
    let response = String::from_utf8_lossy(&response);
    let body = response.split("\r\n\r\n").nth(1).unwrap_or("");
    if body != fixture.expected_body {
        stop_bounded_child(&mut process);
        return Err(io::Error::other(format!(
            "http response body expected {:?}, got {} bytes",
            fixture.expected_body,
            body.len()
        )));
    }

    let output = finish_bounded_child(process, bounded_remaining(deadline))?;
    Ok(std::process::Output {
        status: output.status,
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

fn bounded_remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

#[cfg(test)]
fn read_bounded_http_response(
    stream: &mut std::net::TcpStream,
    max_bytes: usize,
) -> io::Result<Vec<u8>> {
    read_bounded_http_response_with_deadline(stream, max_bytes, None)
}

fn read_bounded_http_response_until(
    stream: &mut std::net::TcpStream,
    max_bytes: usize,
    deadline: Instant,
) -> io::Result<Vec<u8>> {
    read_bounded_http_response_with_deadline(stream, max_bytes, Some(deadline))
}

fn read_bounded_http_response_with_deadline(
    stream: &mut std::net::TcpStream,
    max_bytes: usize,
    deadline: Option<Instant>,
) -> io::Result<Vec<u8>> {
    let mut response = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        if let Some(deadline) = deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    ErrorKind::TimedOut,
                    "http response exceeded its execution timeout",
                ));
            }
            stream.set_read_timeout(Some(remaining))?;
        }
        let read = match stream.read(&mut buffer) {
            Err(err)
                if deadline.is_some()
                    && matches!(err.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) =>
            {
                return Err(io::Error::new(
                    ErrorKind::TimedOut,
                    "http response exceeded its execution timeout",
                ));
            }
            result => result?,
        };
        if read == 0 {
            return Ok(response);
        }
        if response.len().saturating_add(read) > max_bytes {
            return Err(io::Error::other(format!(
                "http response exceeded the {} byte limit",
                max_bytes
            )));
        }
        response.extend_from_slice(&buffer[..read]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    #[cfg(unix)]
    #[test]
    fn bounded_test_command_enforces_benchmark_timeout() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 2"]);
        let error = run_test_command(
            &mut command,
            Some(RunLimits::benchmark(Duration::from_millis(50))),
        )
        .expect_err("sleeping command should exceed benchmark timeout");
        assert_eq!(error.kind(), ErrorKind::TimedOut);
    }

    #[cfg(unix)]
    #[test]
    fn bounded_stdin_fixture_drains_output_while_writing_input() {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "dd if=/dev/zero bs=65536 count=32 2>/dev/null; cat >/dev/null",
        ]);
        let input = "x".repeat(2 * 1024 * 1024);
        let output = run_command_with_stdin(
            command,
            &input,
            Some(RunLimits {
                timeout: Duration::from_secs(2),
                max_output_bytes: 3 * 1024 * 1024,
                max_file_bytes: 8 * 1024 * 1024,
                max_cpu_seconds: 3,
            }),
        )
        .expect("bounded stdin fixture should not deadlock");
        assert!(output.status.success());
        assert_eq!(output.stdout.len(), 2 * 1024 * 1024);
    }

    #[test]
    fn bounded_http_response_rejects_oversized_payloads() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind response server");
        let address = listener.local_addr().expect("response server address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept response client");
            let body = vec![b'x'; 128];
            let mut response = b"HTTP/1.0 200 OK\r\nContent-Length: 128\r\n\r\n".to_vec();
            response.extend_from_slice(&body);
            stream
                .write_all(&response)
                .expect("write oversized response");
        });
        let mut stream = std::net::TcpStream::connect(address).expect("connect response server");
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("set response timeout");
        let error = read_bounded_http_response(&mut stream, 64)
            .expect_err("oversized response should be rejected");
        assert!(error.to_string().contains("exceeded the 64 byte limit"));
        server.join().expect("response server thread");
    }

    #[test]
    fn bounded_http_response_observes_socket_read_deadline() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind idle server");
        let address = listener.local_addr().expect("idle server address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept idle client");
            stream
                .write_all(b"HTTP/1.0 200 OK\r\n\r\npartial")
                .expect("write partial response");
            thread::sleep(Duration::from_millis(200));
        });
        let mut stream = std::net::TcpStream::connect(address).expect("connect idle server");
        stream
            .set_read_timeout(Some(Duration::from_millis(50)))
            .expect("set idle response timeout");
        let error = read_bounded_http_response(&mut stream, 1024)
            .expect_err("idle response should hit the socket deadline");
        assert!(matches!(
            error.kind(),
            ErrorKind::TimedOut | ErrorKind::WouldBlock
        ));
        server.join().expect("idle server thread");
    }

    #[cfg(unix)]
    #[test]
    fn bounded_http_fixture_enforces_timeout_and_kills_child_group() {
        let build_output_dir = tempdir().expect("build output directory");
        let binary = build_output_dir.path().join("fixture.sh");
        let marker = build_output_dir.path().join("fixture.sh.finished");
        fs::write(
            &binary,
            b"#!/bin/sh\nsleep 1\nprintf done > \"$0.finished\"\n",
        )
        .expect("write fixture child");
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))
            .expect("make fixture child executable");

        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind fixture server");
        let address = listener.local_addr().expect("fixture server address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept fixture client");
            stream
                .write_all(b"HTTP/1.0 200 OK\r\n\r\npartial")
                .expect("write partial fixture response");
            thread::sleep(Duration::from_millis(200));
        });
        let test = crate::manifest::TestTarget {
            name: "bounded-http-timeout".to_string(),
            entry: "fixture.ax".to_string(),
            stdin: None,
            stdout: None,
            stderr: None,
            kind: crate::manifest::TestKind::Unit,
            expected_error: None,
            http: Some(crate::manifest::HttpTestFixture {
                bind: Some(address.to_string()),
                path: "/health".to_string(),
                expected_body: "ok".to_string(),
            }),
            capabilities: Vec::new(),
            package: None,
        };
        let error = run_bounded_http_fixture_case(
            &binary,
            build_output_dir.path(),
            &test,
            RunLimits {
                timeout: Duration::from_millis(50),
                max_output_bytes: 1024,
                max_file_bytes: 8 * 1024 * 1024,
                max_cpu_seconds: 1,
            },
        )
        .expect_err("bounded HTTP fixture should hit its global timeout");
        assert_eq!(error.kind(), ErrorKind::TimedOut);
        server.join().expect("fixture server thread");
        thread::sleep(Duration::from_millis(150));
        assert!(
            !marker.exists(),
            "timed-out fixture child should not finish"
        );
    }
}
