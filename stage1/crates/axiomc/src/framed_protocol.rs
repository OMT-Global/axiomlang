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
}
