use crate::diagnostics::Diagnostic;
use std::io::BufRead;

pub(crate) const MAX_HEADER_BYTES: usize = 64 * 1024;
pub(crate) const MAX_HEADER_LINE_BYTES: usize = 8 * 1024;
pub(crate) const MAX_HEADER_COUNT: usize = 64;
pub(crate) const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn read_message<R>(input: &mut R, protocol: &str) -> Result<Option<String>, Diagnostic>
where
    R: BufRead,
{
    let Some(first_line) = read_header_line(input, protocol)? else {
        return Ok(None);
    };
    let mut header_bytes = first_line.len();
    let mut header_count = 0usize;
    let mut content_length = None;

    let mut line = Some(first_line);
    while let Some(raw_line) = line.take() {
        if header_bytes > MAX_HEADER_BYTES {
            return Err(frame_error(
                protocol,
                "headers_oversized",
                format!("{protocol} headers exceed the {MAX_HEADER_BYTES}-byte limit"),
            ));
        }
        let trimmed = trim_header_line(&raw_line, protocol)?;
        if trimmed.is_empty() {
            break;
        }
        header_count = header_count.saturating_add(1);
        if header_count > MAX_HEADER_COUNT {
            return Err(frame_error(
                protocol,
                "too_many_headers",
                format!("{protocol} message exceeds the {MAX_HEADER_COUNT}-header limit"),
            ));
        }
        let (name, value) = trimmed.split_once(':').ok_or_else(|| {
            frame_error(
                protocol,
                "malformed_header",
                format!("malformed {protocol} header"),
            )
        })?;
        if name.trim().is_empty() {
            return Err(frame_error(
                protocol,
                "malformed_header",
                format!("malformed {protocol} header name"),
            ));
        }
        if name.trim().eq_ignore_ascii_case("Content-Length") {
            if content_length.is_some() {
                return Err(frame_error(
                    protocol,
                    "duplicate_content_length",
                    format!("duplicate Content-Length header in {protocol} message"),
                ));
            }
            let value = value.trim();
            let length = value.parse::<u64>().map_err(|_| {
                frame_error(
                    protocol,
                    "invalid_content_length",
                    format!("invalid Content-Length header in {protocol} message"),
                )
            })?;
            if length > MAX_BODY_BYTES as u64 {
                return Err(frame_error(
                    protocol,
                    "body_oversized",
                    format!("{protocol} body exceeds the {MAX_BODY_BYTES}-byte limit"),
                ));
            }
            content_length = Some(length as usize);
        }

        line = read_header_line(input, protocol)?;
        let Some(next_line) = line.as_ref() else {
            return Err(frame_error(
                protocol,
                "truncated_headers",
                format!("truncated {protocol} header block"),
            ));
        };
        header_bytes = header_bytes.saturating_add(next_line.len());
        if header_bytes > MAX_HEADER_BYTES {
            return Err(frame_error(
                protocol,
                "headers_oversized",
                format!("{protocol} headers exceed the {MAX_HEADER_BYTES}-byte limit"),
            ));
        }
    }

    let length = content_length.ok_or_else(|| {
        frame_error(
            protocol,
            "missing_content_length",
            format!("missing Content-Length header in {protocol} message"),
        )
    })?;
    let mut body = Vec::new();
    body.try_reserve_exact(length).map_err(|_| {
        frame_error(
            protocol,
            "body_allocation_failed",
            format!("unable to reserve {length} bytes for {protocol} body"),
        )
    })?;
    body.resize(length, 0);
    input.read_exact(&mut body).map_err(|err| {
        frame_error(
            protocol,
            "truncated_body",
            format!("failed to read {protocol} body: {err}"),
        )
    })?;
    String::from_utf8(body).map(Some).map_err(|err| {
        frame_error(
            protocol,
            "invalid_utf8",
            format!("{protocol} body is not UTF-8: {err}"),
        )
    })
}

fn read_header_line<R>(input: &mut R, protocol: &str) -> Result<Option<Vec<u8>>, Diagnostic>
where
    R: BufRead,
{
    let mut line = Vec::new();
    loop {
        let buffer = input.fill_buf().map_err(|err| {
            frame_error(
                protocol,
                "header_read",
                format!("failed to read {protocol} header: {err}"),
            )
        })?;
        if buffer.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            return Err(frame_error(
                protocol,
                "truncated_header",
                format!("truncated {protocol} header line"),
            ));
        }
        let newline = buffer.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(buffer.len(), |index| index + 1);
        if line.len().saturating_add(take) > MAX_HEADER_LINE_BYTES {
            return Err(frame_error(
                protocol,
                "header_line_oversized",
                format!("{protocol} header line exceeds the {MAX_HEADER_LINE_BYTES}-byte limit"),
            ));
        }
        line.extend_from_slice(&buffer[..take]);
        input.consume(take);
        if newline.is_some() {
            return Ok(Some(line));
        }
    }
}

fn trim_header_line<'a>(line: &'a [u8], protocol: &str) -> Result<&'a str, Diagnostic> {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    std::str::from_utf8(line).map_err(|err| {
        frame_error(
            protocol,
            "invalid_header",
            format!("{protocol} header is not UTF-8: {err}"),
        )
    })
}

fn frame_error(protocol: &str, code: &str, message: String) -> Diagnostic {
    Diagnostic::new(protocol, message).with_code(format!("{protocol}.frame.{code}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn read(input: &str, protocol: &str) -> Result<Option<String>, Diagnostic> {
        read_message(&mut Cursor::new(input.as_bytes()), protocol)
    }

    #[test]
    fn accepts_valid_lsp_and_dap_frames() {
        for protocol in ["lsp", "dap"] {
            let body = "{\"jsonrpc\":\"2.0\"}";
            let input = format!("X-Test: ok\r\nContent-Length: {}\r\n\r\n{body}", body.len());
            assert_eq!(read(&input, protocol).unwrap(), Some(body.to_owned()));
        }
    }

    #[test]
    fn rejects_duplicate_missing_malformed_and_overflow_lengths() {
        for (input, code) in [
            (
                "Content-Length: 1\r\nContent-Length: 1\r\n\r\na",
                "duplicate_content_length",
            ),
            ("X-Test: ok\r\n\r\n", "missing_content_length"),
            ("Content-Length: nope\r\n\r\n", "invalid_content_length"),
            (
                "Content-Length: 18446744073709551616\r\n\r\n",
                "invalid_content_length",
            ),
            ("Content-Length: 1\r\n\r\n", "truncated_body"),
        ] {
            let error = read(input, "lsp").expect_err("invalid frame must fail");
            assert_eq!(
                error.code.as_deref(),
                Some(format!("lsp.frame.{code}").as_str())
            );
        }
    }

    #[test]
    fn rejects_malformed_oversized_and_truncated_frames_before_body_allocation() {
        for protocol in ["lsp", "dap"] {
            let malformed = read("not-a-header\r\n\r\n", protocol)
                .expect_err("malformed header");
            assert_eq!(
                malformed.code.as_deref(),
                Some(format!("{protocol}.frame.malformed_header").as_str())
            );

            let huge = format!("Content-Length: {}\r\n\r\n", MAX_BODY_BYTES + 1);
            let huge = read(&huge, protocol).expect_err("oversized body");
            assert_eq!(
                huge.code.as_deref(),
                Some(format!("{protocol}.frame.body_oversized").as_str())
            );
        }

        let mut many_headers = String::new();
        for index in 0..=MAX_HEADER_COUNT {
            many_headers.push_str(&format!("X-Test-{index}: ok\r\n"));
        }
        many_headers.push_str("Content-Length: 0\r\n\r\n");
        let many_headers = read(&many_headers, "dap").expect_err("too many headers");
        assert_eq!(
            many_headers.code.as_deref(),
            Some("dap.frame.too_many_headers")
        );

        let long_header = format!("{}\r\n", "x".repeat(MAX_HEADER_LINE_BYTES));
        let long_header = read(&long_header, "lsp").expect_err("oversized header line");
        assert_eq!(
            long_header.code.as_deref(),
            Some("lsp.frame.header_line_oversized")
        );

        let body = "Content-Length: 4\r\n\r\nabc";
        let truncated = read(body, "lsp").expect_err("truncated body");
        assert_eq!(truncated.code.as_deref(), Some("lsp.frame.truncated_body"));
    }


    fn endpoint(protocol: &str, input: &[u8]) -> (Result<(), Diagnostic>, Vec<u8>) {
        let mut output = Vec::new();
        let result = match protocol {
            "lsp" => crate::lsp::serve_stdio(Cursor::new(input), &mut output),
            "dap" => crate::dap::run_stdio(Cursor::new(input), &mut output),
            _ => unreachable!("test protocol"),
        };
        (result, output)
    }

    #[test]
    fn both_endpoints_reject_invalid_frames_before_dispatch() {
        let cases = [
            (b"Content-Length: 1\r\nContent-Length: 1\r\n\r\na".to_vec(), "duplicate_content_length"),
            (b"X: a\r\n\r\n".to_vec(), "missing_content_length"),
            (b"Content-Length: nope\r\n\r\n".to_vec(), "invalid_content_length"),
            (b"Content-Length: 18446744073709551616\r\n\r\n".to_vec(), "invalid_content_length"),
            (b"not-a-header\r\n\r\n".to_vec(), "malformed_header"),
            (b"Content-Length: 4\r\n\r\nabc".to_vec(), "truncated_body"),
            (b"Content-Length: 4\r\n".to_vec(), "truncated_headers"),
            (b"Content-Length: 4".to_vec(), "truncated_header"),
            (b"Content-Length: 1\r\n\r\n\xff".to_vec(), "invalid_utf8"),
            (format!("Content-Length: {}\r\n\r\n", MAX_BODY_BYTES + 1).into_bytes(), "body_oversized"),
        ];
        for protocol in ["lsp", "dap"] {
            for (input, code) in &cases {
                let (result, output) = endpoint(protocol, input);
                let error = result.expect_err("invalid endpoint frame must fail");
                assert_eq!(error.code.as_deref(), Some(format!("{protocol}.frame.{code}").as_str()));
                assert!(output.is_empty(), "invalid framing must not dispatch a message");
            }
        }
    }

    #[test]
    fn aggregate_header_bound_is_enforced_at_both_endpoints() {
        let mut frame = b"Content-Length: 0\r\n".to_vec();
        while frame.len() + 2 < MAX_HEADER_BYTES {
            let bytes = (MAX_HEADER_BYTES - frame.len() - 2).min(MAX_HEADER_LINE_BYTES);
            assert!(bytes >= 5);
            frame.extend_from_slice(b"X: ");
            frame.extend(std::iter::repeat_n(b'x', bytes - 5));
            frame.extend_from_slice(b"\r\n");
        }
        frame.extend_from_slice(b"\r\n");
        assert_eq!(frame.len(), MAX_HEADER_BYTES);
        for protocol in ["lsp", "dap"] {
            assert_eq!(read_message(&mut Cursor::new(&frame), protocol).unwrap(), Some(String::new()));
            let mut excessive = frame.clone();
            excessive.splice(excessive.len()-2..excessive.len()-2, b"X: x\r\n".iter().copied());
            let (result, output) = endpoint(protocol, &excessive);
            assert_eq!(result.unwrap_err().code.as_deref(), Some(format!("{protocol}.frame.headers_oversized").as_str()));
            assert!(output.is_empty());
        }
    }

    #[test]
    fn line_and_count_limits_reach_both_endpoints() {
        let oversized_line = format!("X: {}\r\n", "x".repeat(MAX_HEADER_LINE_BYTES));
        let too_many = format!("{}Content-Length: 0\r\n\r\n", "X: x\r\n".repeat(MAX_HEADER_COUNT));
        for protocol in ["lsp", "dap"] {
            for (input, code) in [(&oversized_line, "header_line_oversized"), (&too_many, "too_many_headers")] {
                let (result, output) = endpoint(protocol, input.as_bytes());
                assert_eq!(result.unwrap_err().code.as_deref(), Some(format!("{protocol}.frame.{code}").as_str()));
                assert!(output.is_empty());
            }
        }
    }

    #[test]
    fn valid_shutdown_stops_before_trailing_malformed_frame() {
        for (protocol, body) in [
            ("lsp", r#"{"jsonrpc":"2.0","method":"exit"}"#),
            ("dap", r#"{"seq":1,"type":"request","command":"disconnect"}"#),
        ] {
            let input = format!("Content-Length: {}\r\n\r\n{body}not-a-header\r\n\r\n", body.len());
            endpoint(protocol, input.as_bytes()).0.expect("shutdown must stop before trailing data");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn oversized_endpoints_under_memory_limit() {
        const MARKER: &str = "AXIOM_BOUNDED_FRAME_MEMORY_ASSERTIONS_PASSED";
        if std::env::var_os("AXIOM_FRAME_MEMORY_CHILD").is_some() {
            for protocol in ["lsp", "dap"] {
                let (result, output) = endpoint(protocol, b"Content-Length: 1073741824\r\n\r\n");
                assert_eq!(result.unwrap_err().code.as_deref(), Some(format!("{protocol}.frame.body_oversized").as_str()));
                assert!(output.is_empty());
            }
            println!("{MARKER}");
            return;
        }
        let output = std::process::Command::new("/bin/sh")
            .args(["-c", "ulimit -v 524288 || exit 1; exec \"$1\" --exact framed_protocol::tests::oversized_endpoints_under_memory_limit --nocapture", "axiom-frame-memory"])
            .arg(std::env::current_exe().expect("test executable"))
            .env("AXIOM_FRAME_MEMORY_CHILD", "1")
            .output().expect("run memory-limited child");
        assert!(output.status.success(), "memory-limited child failed: {:?}\n{}\n{}", output.status, String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert!(String::from_utf8_lossy(&output.stdout).contains(MARKER), "child ran no memory assertions");
    }
}
