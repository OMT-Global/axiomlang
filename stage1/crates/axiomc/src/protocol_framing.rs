use std::fmt;
use std::io::{self, BufRead};

pub(crate) const MAX_HEADER_LINE_BYTES: usize = 8 * 1024;
pub(crate) const MAX_HEADER_BYTES: usize = 32 * 1024;
pub(crate) const MAX_HEADER_COUNT: usize = 64;
pub(crate) const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FrameReadError {
    HeaderRead(String),
    HeaderLineTooLarge,
    HeadersTooLarge,
    TooManyHeaders,
    HeaderAllocationFailed,
    TruncatedHeaders,
    MalformedHeader,
    MissingContentLength,
    DuplicateContentLength,
    MalformedContentLength,
    ContentLengthOverflow,
    BodyTooLarge { declared: usize },
    BodyAllocationFailed,
    TruncatedBody { expected: usize },
    BodyRead(String),
    InvalidUtf8Body,
}

impl fmt::Display for FrameReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HeaderRead(error) => write!(formatter, "failed to read header: {error}"),
            Self::HeaderLineTooLarge => write!(
                formatter,
                "header line exceeds {MAX_HEADER_LINE_BYTES} byte limit"
            ),
            Self::HeadersTooLarge => {
                write!(formatter, "headers exceed {MAX_HEADER_BYTES} byte limit")
            }
            Self::TooManyHeaders => {
                write!(formatter, "header count exceeds {MAX_HEADER_COUNT}")
            }
            Self::HeaderAllocationFailed => write!(formatter, "failed to allocate header buffer"),
            Self::TruncatedHeaders => write!(formatter, "truncated headers"),
            Self::MalformedHeader => write!(formatter, "malformed header line"),
            Self::MissingContentLength => write!(formatter, "missing Content-Length header"),
            Self::DuplicateContentLength => write!(formatter, "duplicate Content-Length header"),
            Self::MalformedContentLength => write!(formatter, "malformed Content-Length header"),
            Self::ContentLengthOverflow => {
                write!(formatter, "Content-Length header overflows usize")
            }
            Self::BodyTooLarge { declared } => write!(
                formatter,
                "Content-Length {declared} exceeds {MAX_BODY_BYTES} byte limit"
            ),
            Self::BodyAllocationFailed => write!(formatter, "failed to allocate body buffer"),
            Self::TruncatedBody { expected } => {
                write!(formatter, "truncated body: expected {expected} bytes")
            }
            Self::BodyRead(error) => write!(formatter, "failed to read body: {error}"),
            Self::InvalidUtf8Body => write!(formatter, "body is not UTF-8"),
        }
    }
}

enum HeaderLine {
    Eof,
    Bytes(Vec<u8>),
}

pub(crate) fn read_message<R>(input: &mut R) -> Result<Option<String>, FrameReadError>
where
    R: BufRead,
{
    let mut header_bytes = 0usize;
    let mut header_count = 0usize;
    let mut content_length = None;

    loop {
        let mut line = match read_header_line(input, &mut header_bytes)? {
            HeaderLine::Eof if header_bytes == 0 => return Ok(None),
            HeaderLine::Eof => return Err(FrameReadError::TruncatedHeaders),
            HeaderLine::Bytes(line) => line,
        };

        while line
            .last()
            .is_some_and(|byte| matches!(byte, b'\r' | b'\n'))
        {
            line.pop();
        }
        if line.is_empty() {
            break;
        }

        header_count = header_count
            .checked_add(1)
            .ok_or(FrameReadError::TooManyHeaders)?;
        if header_count > MAX_HEADER_COUNT {
            return Err(FrameReadError::TooManyHeaders);
        }

        let line = std::str::from_utf8(&line).map_err(|_| FrameReadError::MalformedHeader)?;
        let (name, value) = line
            .split_once(':')
            .ok_or(FrameReadError::MalformedHeader)?;
        if name.trim().is_empty() {
            return Err(FrameReadError::MalformedHeader);
        }
        if !name.trim().eq_ignore_ascii_case("Content-Length") {
            continue;
        }
        if content_length.is_some() {
            return Err(FrameReadError::DuplicateContentLength);
        }
        content_length = Some(parse_content_length(value)?);
    }

    let length = content_length.ok_or(FrameReadError::MissingContentLength)?;
    if length > MAX_BODY_BYTES {
        return Err(FrameReadError::BodyTooLarge { declared: length });
    }

    let mut body = Vec::new();
    body.try_reserve_exact(length)
        .map_err(|_| FrameReadError::BodyAllocationFailed)?;
    body.resize(length, 0);
    match input.read_exact(&mut body) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(FrameReadError::TruncatedBody { expected: length });
        }
        Err(error) => return Err(FrameReadError::BodyRead(error.to_string())),
    }

    String::from_utf8(body)
        .map(Some)
        .map_err(|_| FrameReadError::InvalidUtf8Body)
}

fn read_header_line<R>(
    input: &mut R,
    header_bytes: &mut usize,
) -> Result<HeaderLine, FrameReadError>
where
    R: BufRead,
{
    let mut line = Vec::new();
    loop {
        let buffer = input
            .fill_buf()
            .map_err(|error| FrameReadError::HeaderRead(error.to_string()))?;
        if buffer.is_empty() {
            return if line.is_empty() {
                Ok(HeaderLine::Eof)
            } else {
                Err(FrameReadError::TruncatedHeaders)
            };
        }

        let bytes_to_take = buffer
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(buffer.len(), |newline| newline + 1);
        let line_length = line
            .len()
            .checked_add(bytes_to_take)
            .ok_or(FrameReadError::HeaderLineTooLarge)?;
        if line_length > MAX_HEADER_LINE_BYTES {
            return Err(FrameReadError::HeaderLineTooLarge);
        }
        let total_length = header_bytes
            .checked_add(bytes_to_take)
            .ok_or(FrameReadError::HeadersTooLarge)?;
        if total_length > MAX_HEADER_BYTES {
            return Err(FrameReadError::HeadersTooLarge);
        }

        line.try_reserve_exact(bytes_to_take)
            .map_err(|_| FrameReadError::HeaderAllocationFailed)?;
        line.extend_from_slice(&buffer[..bytes_to_take]);
        input.consume(bytes_to_take);
        *header_bytes = total_length;

        if line.last() == Some(&b'\n') {
            return Ok(HeaderLine::Bytes(line));
        }
    }
}

fn parse_content_length(value: &str) -> Result<usize, FrameReadError> {
    let value = value.trim();
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(FrameReadError::MalformedContentLength);
    }

    value.bytes().try_fold(0usize, |length, byte| {
        length
            .checked_mul(10)
            .and_then(|length| length.checked_add(usize::from(byte - b'0')))
            .ok_or(FrameReadError::ContentLengthOverflow)
    })
}
