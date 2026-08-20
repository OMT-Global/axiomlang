//! Bounded registry transport for local files and loopback HTTP fixtures.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const DEFAULT_MAX_REGISTRY_BODY_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PACKAGE_ARCHIVE_BODY_BYTES: usize = 64 * 1024 * 1024;
pub const DEFAULT_MAX_HTTP_HEADER_BYTES: usize = 64 * 1024;
pub const MAX_REGISTRY_URL_BYTES: usize = 4_096;
pub const DEFAULT_REGISTRY_OPERATION_TIMEOUT: Duration = Duration::from_secs(60);
pub const REGISTRY_NETWORK_DISABLED_ENV: &str = "AXIOM_REGISTRY_NETWORK_DISABLED";

#[derive(Clone, Debug)]
pub struct RegistryClient {
    max_body_bytes: usize,
    max_header_bytes: usize,
    timeout: Duration,
    operation_timeout: Duration,
    network_disabled: bool,
}

impl Default for RegistryClient {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_REGISTRY_BODY_BYTES)
    }
}

impl RegistryClient {
    pub fn new(max_body_bytes: usize) -> Self {
        Self {
            max_body_bytes,
            max_header_bytes: DEFAULT_MAX_HTTP_HEADER_BYTES,
            timeout: Duration::from_secs(5),
            operation_timeout: DEFAULT_REGISTRY_OPERATION_TIMEOUT,
            network_disabled: registry_network_disabled(),
        }
    }

    pub fn with_limits(max_body_bytes: usize, max_header_bytes: usize, timeout: Duration) -> Self {
        Self {
            max_body_bytes,
            max_header_bytes,
            timeout,
            operation_timeout: DEFAULT_REGISTRY_OPERATION_TIMEOUT,
            network_disabled: registry_network_disabled(),
        }
    }

    pub fn with_network_disabled(mut self) -> Self {
        self.network_disabled = true;
        self
    }

    /// Set the maximum wall-clock time shared by a package operation's
    /// registry fetches. Individual requests remain capped by `timeout`.
    pub fn with_operation_timeout(mut self, timeout: Duration) -> Self {
        self.operation_timeout = timeout;
        self
    }

    pub const fn max_body_bytes(&self) -> usize {
        self.max_body_bytes
    }

    /// Create the absolute deadline to share across all registry fetches in
    /// one package operation.
    pub fn operation_deadline(&self) -> Result<Instant, RegistryClientError> {
        Instant::now()
            .checked_add(self.operation_timeout)
            .ok_or_else(|| RegistryClientError::Io {
                context: "configure registry operation deadline".to_owned(),
                message: "registry operation timeout exceeds the platform clock range".to_owned(),
            })
    }

    /// Fetch a registry index, trust document, manifest, or other bounded
    /// metadata body. The client's configured document limit applies.
    pub fn fetch(&self, url: &str) -> Result<Vec<u8>, RegistryClientError> {
        self.fetch_bounded(url, self.max_body_bytes)
    }

    /// Fetch metadata while honoring an absolute deadline shared by the
    /// surrounding package operation. The request still has the client's
    /// shorter per-request timeout when that expires first.
    pub fn fetch_until(
        &self,
        url: &str,
        deadline: Instant,
    ) -> Result<Vec<u8>, RegistryClientError> {
        self.fetch_bounded_until(url, self.max_body_bytes, deadline)
    }

    /// Fetch a package archive using the package-format contract's independent
    /// 64 MiB ceiling. Callers must opt into this larger allowance explicitly;
    /// metadata fetches continue to use [`Self::fetch`] and its smaller limit.
    pub fn fetch_package_archive(&self, url: &str) -> Result<Vec<u8>, RegistryClientError> {
        self.fetch_bounded(url, MAX_PACKAGE_ARCHIVE_BODY_BYTES)
    }

    /// Fetch a package archive while honoring an absolute operation deadline.
    pub fn fetch_package_archive_until(
        &self,
        url: &str,
        deadline: Instant,
    ) -> Result<Vec<u8>, RegistryClientError> {
        self.fetch_bounded_until(url, MAX_PACKAGE_ARCHIVE_BODY_BYTES, deadline)
    }

    fn fetch_bounded(
        &self,
        url: &str,
        max_body_bytes: usize,
    ) -> Result<Vec<u8>, RegistryClientError> {
        self.fetch_bounded_until(url, max_body_bytes, self.operation_deadline()?)
    }

    fn fetch_bounded_until(
        &self,
        url: &str,
        max_body_bytes: usize,
        operation_deadline: Instant,
    ) -> Result<Vec<u8>, RegistryClientError> {
        let deadline = self.request_deadline(operation_deadline)?;
        if url.len() > MAX_REGISTRY_URL_BYTES || !url.is_ascii() {
            return Err(RegistryClientError::InvalidUrl(
                "registry URL is non-ASCII or exceeds 4096 bytes".to_owned(),
            ));
        }
        if url.starts_with("https://") {
            return Err(RegistryClientError::UnsupportedScheme("https".to_owned()));
        }
        if let Some(path) = url.strip_prefix("file://") {
            return self.fetch_file(parse_file_path(path)?, max_body_bytes, deadline);
        }
        if let Some(endpoint) = url.strip_prefix("http://") {
            if self.network_disabled {
                return Err(RegistryClientError::NetworkDisabled);
            }
            return self.fetch_http(parse_http_endpoint(endpoint)?, max_body_bytes, deadline);
        }
        let scheme = url.split_once(':').map_or("", |(scheme, _)| scheme);
        Err(RegistryClientError::UnsupportedScheme(scheme.to_owned()))
    }

    fn request_deadline(
        &self,
        operation_deadline: Instant,
    ) -> Result<Instant, RegistryClientError> {
        let request_deadline =
            Instant::now()
                .checked_add(self.timeout)
                .ok_or_else(|| RegistryClientError::Io {
                    context: "configure loopback registry deadline".to_owned(),
                    message: "registry timeout exceeds the platform clock range".to_owned(),
                })?;
        if operation_deadline <= Instant::now() {
            return Err(deadline_error("start registry fetch"));
        }
        Ok(operation_deadline.min(request_deadline))
    }

    fn fetch_file(
        &self,
        path: PathBuf,
        max_body_bytes: usize,
        deadline: Instant,
    ) -> Result<Vec<u8>, RegistryClientError> {
        let file = open_registry_file(&path)?;
        let metadata = file
            .metadata()
            .map_err(|error| io_error("inspect opened registry file", error))?;
        if !metadata.is_file() {
            return Err(RegistryClientError::InvalidFile(
                "file registry endpoint must be a regular non-symlink file".to_owned(),
            ));
        }
        if metadata.len() > max_body_bytes as u64 {
            return Err(RegistryClientError::ResponseTooLarge {
                limit: max_body_bytes,
                declared: Some(metadata.len()),
            });
        }
        ensure_deadline(deadline, "read registry file")?;
        let bytes = read_bounded(file, max_body_bytes)?;
        ensure_deadline(deadline, "read registry file")?;
        Ok(bytes)
    }

    fn fetch_http(
        &self,
        endpoint: HttpEndpoint,
        max_body_bytes: usize,
        deadline: Instant,
    ) -> Result<Vec<u8>, RegistryClientError> {
        let connect_timeout = remaining_http_time(deadline, "connect to loopback registry")?;
        let mut stream = TcpStream::connect_timeout(&endpoint.address, connect_timeout)
            .map_err(|error| io_error("connect to loopback registry", error))?;
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nAccept: application/json, application/octet-stream\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
            endpoint.target, endpoint.host_header
        );
        write_all_before_deadline(
            &mut stream,
            request.as_bytes(),
            deadline,
            "write registry request",
        )?;
        self.read_http_response(stream, max_body_bytes, deadline)
    }

    fn read_http_response(
        &self,
        mut stream: TcpStream,
        max_body_bytes: usize,
        deadline: Instant,
    ) -> Result<Vec<u8>, RegistryClientError> {
        let mut received = Vec::new();
        let header_end = loop {
            if let Some(offset) = find_bytes(&received, b"\r\n\r\n") {
                break offset + 4;
            }
            if received.len() >= self.max_header_bytes {
                return Err(RegistryClientError::HeadersTooLarge {
                    limit: self.max_header_bytes,
                });
            }
            let remaining = self.max_header_bytes - received.len();
            let mut buffer = [0_u8; 8 * 1024];
            let capacity = buffer.len();
            let read = read_before_deadline(
                &mut stream,
                &mut buffer[..remaining.min(capacity)],
                deadline,
                "read registry response headers",
            )?;
            if read == 0 {
                return Err(RegistryClientError::TruncatedResponse {
                    expected: None,
                    received: received.len(),
                });
            }
            received.extend_from_slice(&buffer[..read]);
        };
        let headers = std::str::from_utf8(&received[..header_end])
            .map_err(|_| RegistryClientError::MalformedResponse("headers are not UTF-8".into()))?;
        let parsed = parse_response_headers(headers)?;
        if (300..400).contains(&parsed.status) {
            return Err(RegistryClientError::Redirect(parsed.status));
        }
        if parsed.status != 200 {
            return Err(RegistryClientError::HttpStatus(parsed.status));
        }
        if parsed.transfer_encoding {
            return Err(RegistryClientError::UnsupportedTransferEncoding);
        }
        let content_length = parsed
            .content_length
            .ok_or(RegistryClientError::AmbiguousLength)?;
        if content_length > max_body_bytes as u64 {
            return Err(RegistryClientError::ResponseTooLarge {
                limit: max_body_bytes,
                declared: Some(content_length),
            });
        }
        let expected =
            usize::try_from(content_length).map_err(|_| RegistryClientError::ResponseTooLarge {
                limit: max_body_bytes,
                declared: Some(content_length),
            })?;
        let mut body = received[header_end..].to_vec();
        if body.len() > expected {
            return Err(RegistryClientError::AmbiguousLength);
        }
        while body.len() < expected {
            let mut buffer = [0_u8; 8 * 1024];
            let remaining = expected - body.len();
            let capacity = buffer.len();
            let read = read_before_deadline(
                &mut stream,
                &mut buffer[..remaining.min(capacity)],
                deadline,
                "read registry response body",
            )?;
            if read == 0 {
                return Err(RegistryClientError::TruncatedResponse {
                    expected: Some(expected),
                    received: body.len(),
                });
            }
            body.extend_from_slice(&buffer[..read]);
        }
        // Content-Length is the HTTP message framing boundary. Once exactly
        // that many bytes have arrived the response is complete; waiting for
        // EOF would let a keep-alive or delayed-close peer consume a fresh
        // per-read timeout after an otherwise successful bounded fetch.
        Ok(body)
    }
}

fn registry_network_disabled() -> bool {
    std::env::var_os(REGISTRY_NETWORK_DISABLED_ENV).is_some_and(|value| value == "1")
}

fn deadline_error(context: &str) -> RegistryClientError {
    RegistryClientError::Io {
        context: context.to_owned(),
        message: "registry operation deadline elapsed".to_owned(),
    }
}

fn ensure_deadline(deadline: Instant, context: &str) -> Result<(), RegistryClientError> {
    if deadline <= Instant::now() {
        return Err(deadline_error(context));
    }
    Ok(())
}

fn remaining_http_time(deadline: Instant, context: &str) -> Result<Duration, RegistryClientError> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| deadline_error(context))
}

fn read_before_deadline(
    stream: &mut TcpStream,
    buffer: &mut [u8],
    deadline: Instant,
    context: &str,
) -> Result<usize, RegistryClientError> {
    let remaining = remaining_http_time(deadline, context)?;
    stream
        .set_read_timeout(Some(remaining))
        .map_err(|error| io_error("configure registry read deadline", error))?;
    stream
        .read(buffer)
        .map_err(|error| io_error(context, error))
}

fn write_all_before_deadline(
    stream: &mut TcpStream,
    mut bytes: &[u8],
    deadline: Instant,
    context: &str,
) -> Result<(), RegistryClientError> {
    while !bytes.is_empty() {
        let remaining = remaining_http_time(deadline, context)?;
        stream
            .set_write_timeout(Some(remaining))
            .map_err(|error| io_error("configure registry write deadline", error))?;
        let written = stream
            .write(bytes)
            .map_err(|error| io_error(context, error))?;
        if written == 0 {
            return Err(io_error(
                context,
                std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "failed to write complete registry request",
                ),
            ));
        }
        bytes = &bytes[written..];
    }
    Ok(())
}

#[cfg(unix)]
fn open_registry_file(path: &Path) -> Result<File, RegistryClientError> {
    use std::os::unix::fs::OpenOptionsExt;

    let file = OpenOptions::new()
        .read(true)
        // O_NOFOLLOW closes the stat-then-open race for the final path
        // component. O_NONBLOCK prevents a swapped FIFO from blocking before
        // descriptor metadata rejects it, and O_CLOEXEC avoids descriptor
        // inheritance into compiler subprocesses.
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| {
            if error.raw_os_error() == Some(libc::ELOOP) {
                RegistryClientError::InvalidFile(
                    "file registry endpoint must not be a symlink".to_owned(),
                )
            } else {
                io_error("open registry file without following symlinks", error)
            }
        })?;
    validate_opened_registry_file(file)
}

#[cfg(windows)]
fn open_registry_file(path: &Path) -> Result<File, RegistryClientError> {
    use std::os::windows::fs::OpenOptionsExt;

    // FILE_FLAG_OPEN_REPARSE_POINT opens the final reparse point itself rather
    // than following it. Descriptor metadata below then rejects symlinks,
    // directories, devices, and other non-regular endpoints.
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|error| io_error("open registry file without following reparse points", error))?;
    validate_opened_registry_file(file)
}

#[cfg(not(any(unix, windows)))]
fn open_registry_file(_path: &Path) -> Result<File, RegistryClientError> {
    // std does not expose a portable no-follow open. Refuse file transport on
    // an unsupported target instead of reintroducing a check-then-open race.
    Err(RegistryClientError::InvalidFile(
        "file registry endpoints require descriptor-level no-follow support on this platform"
            .to_owned(),
    ))
}

fn validate_opened_registry_file(file: File) -> Result<File, RegistryClientError> {
    let metadata = file
        .metadata()
        .map_err(|error| io_error("inspect opened registry file", error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(RegistryClientError::InvalidFile(
            "file registry endpoint must be a regular non-symlink file".to_owned(),
        ));
    }
    Ok(file)
}

/// Resolve an authenticated catalog's relative artifact path against the
/// directory containing its index URL.
///
/// Artifact paths are deliberately narrower than general URLs. They must be
/// normalized ASCII relative paths and cannot contain escaping, query,
/// fragment, backslash, control, empty, or traversal components.
pub fn resolve_registry_artifact_url(
    index_url: &str,
    relative_path: &str,
) -> Result<String, RegistryClientError> {
    validate_relative_artifact_path(relative_path)?;
    if index_url.len() > MAX_REGISTRY_URL_BYTES || !index_url.is_ascii() {
        return Err(RegistryClientError::InvalidUrl(
            "registry URL is non-ASCII or exceeds 4096 bytes".to_owned(),
        ));
    }
    let resolved = if let Some(value) = index_url.strip_prefix("file://") {
        let index_path = parse_file_path(value)?;
        validate_normalized_index_path(value)?;
        let directory = index_path.parent().ok_or_else(|| {
            RegistryClientError::InvalidUrl(
                "file registry index URL has no containing directory".to_owned(),
            )
        })?;
        format!("file://{}", directory.join(relative_path).display())
    } else if let Some(value) = index_url.strip_prefix("http://") {
        let endpoint = parse_http_endpoint(value)?;
        validate_normalized_index_path(&endpoint.target)?;
        let directory_end = endpoint.target.rfind('/').ok_or_else(|| {
            RegistryClientError::InvalidUrl(
                "HTTP registry index URL has no containing directory".to_owned(),
            )
        })?;
        let directory = &endpoint.target[..=directory_end];
        format!(
            "http://{}{}{}",
            endpoint.host_header, directory, relative_path
        )
    } else if index_url.starts_with("https://") {
        return Err(RegistryClientError::UnsupportedScheme("https".to_owned()));
    } else {
        let scheme = index_url.split_once(':').map_or("", |(scheme, _)| scheme);
        return Err(RegistryClientError::UnsupportedScheme(scheme.to_owned()));
    };
    if resolved.len() > MAX_REGISTRY_URL_BYTES {
        return Err(RegistryClientError::InvalidUrl(
            "resolved registry URL exceeds 4096 bytes".to_owned(),
        ));
    }
    Ok(resolved)
}

fn validate_normalized_index_path(value: &str) -> Result<(), RegistryClientError> {
    if !value.starts_with('/')
        || value.starts_with("//")
        || value.contains(['\\', '%'])
        || value
            .trim_start_matches('/')
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Err(RegistryClientError::InvalidUrl(
            "registry index path must be normalized and unescaped".to_owned(),
        ));
    }
    Ok(())
}

fn validate_relative_artifact_path(value: &str) -> Result<(), RegistryClientError> {
    if value.is_empty()
        || !value.is_ascii()
        || value.starts_with('/')
        || value.contains(['\\', '?', '#', '%'])
        || value.bytes().any(|byte| byte.is_ascii_control())
        || value
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Err(RegistryClientError::InvalidUrl(
            "artifact path must be a normalized unescaped ASCII relative path".to_owned(),
        ));
    }
    Ok(())
}

fn read_bounded(reader: impl Read, limit: usize) -> Result<Vec<u8>, RegistryClientError> {
    let take_limit = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::new();
    reader
        .take(take_limit)
        .read_to_end(&mut bytes)
        .map_err(|error| io_error("read registry file", error))?;
    if bytes.len() > limit {
        return Err(RegistryClientError::ResponseTooLarge {
            limit,
            declared: None,
        });
    }
    Ok(bytes)
}

fn parse_file_path(value: &str) -> Result<PathBuf, RegistryClientError> {
    if !value.starts_with('/')
        || value.contains(['?', '#', '%'])
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(RegistryClientError::InvalidUrl(
            "file URL must contain one unescaped absolute path".to_owned(),
        ));
    }
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err(RegistryClientError::InvalidUrl(
            "file URL path must be absolute".to_owned(),
        ));
    }
    Ok(path.to_owned())
}

#[derive(Clone, Debug)]
struct HttpEndpoint {
    address: SocketAddr,
    host_header: String,
    target: String,
}

fn parse_http_endpoint(value: &str) -> Result<HttpEndpoint, RegistryClientError> {
    if value.contains(['#', '\r', '\n', ' ', '\t']) {
        return Err(RegistryClientError::InvalidUrl(
            "HTTP registry URL contains forbidden characters".to_owned(),
        ));
    }
    let (authority, target) = value
        .split_once('/')
        .map_or((value, "/".to_owned()), |(authority, path)| {
            (authority, format!("/{path}"))
        });
    if authority.is_empty() || authority.contains('@') || target.contains('?') {
        return Err(RegistryClientError::InvalidUrl(
            "HTTP registry URL authority or target is invalid".to_owned(),
        ));
    }
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let close = rest
            .find(']')
            .ok_or_else(|| RegistryClientError::InvalidUrl("unterminated IPv6 host".to_owned()))?;
        let host = &rest[..close];
        let suffix = &rest[close + 1..];
        let port = if suffix.is_empty() {
            80
        } else {
            suffix
                .strip_prefix(':')
                .ok_or_else(|| {
                    RegistryClientError::InvalidUrl("invalid IPv6 authority".to_owned())
                })?
                .parse()
                .map_err(|_| RegistryClientError::InvalidUrl("invalid HTTP port".to_owned()))?
        };
        (host, port)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if host.contains(':') {
            return Err(RegistryClientError::InvalidUrl(
                "IPv6 hosts must use brackets".to_owned(),
            ));
        }
        let port = port
            .parse()
            .map_err(|_| RegistryClientError::InvalidUrl("invalid HTTP port".to_owned()))?;
        (host, port)
    } else {
        (authority, 80)
    };
    let ip: IpAddr = host.parse().map_err(|_| {
        RegistryClientError::NonLoopbackHost(
            "HTTP registry host must be a numeric loopback address".to_owned(),
        )
    })?;
    if !ip.is_loopback() {
        return Err(RegistryClientError::NonLoopbackHost(host.to_owned()));
    }
    let address = SocketAddr::new(ip, port);
    let host_header = if ip.is_ipv6() {
        format!("[{ip}]:{port}")
    } else {
        format!("{ip}:{port}")
    };
    Ok(HttpEndpoint {
        address,
        host_header,
        target,
    })
}

#[derive(Debug)]
struct ParsedHeaders {
    status: u16,
    content_length: Option<u64>,
    transfer_encoding: bool,
}

fn parse_response_headers(value: &str) -> Result<ParsedHeaders, RegistryClientError> {
    let mut lines = value
        .strip_suffix("\r\n\r\n")
        .ok_or_else(|| RegistryClientError::MalformedResponse("missing header terminator".into()))?
        .split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| RegistryClientError::MalformedResponse("missing status line".into()))?;
    let mut status_parts = status_line.split_whitespace();
    let version = status_parts.next().unwrap_or_default();
    let status: u16 = status_parts
        .next()
        .ok_or_else(|| RegistryClientError::MalformedResponse("missing status".into()))?
        .parse()
        .map_err(|_| RegistryClientError::MalformedResponse("invalid status".into()))?;
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") || status_parts.next().is_none() {
        return Err(RegistryClientError::MalformedResponse(
            "invalid HTTP status line".to_owned(),
        ));
    }
    let mut content_length = None;
    let mut transfer_encoding = false;
    for line in lines {
        if line.is_empty() || line.starts_with([' ', '\t']) {
            return Err(RegistryClientError::MalformedResponse(
                "empty or folded HTTP header".to_owned(),
            ));
        }
        let (name, raw_value) = line.split_once(':').ok_or_else(|| {
            RegistryClientError::MalformedResponse("header lacks colon".to_owned())
        })?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(RegistryClientError::MalformedResponse(
                "invalid HTTP header name".to_owned(),
            ));
        }
        let header_value = raw_value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some()
                || header_value.is_empty()
                || !header_value.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(RegistryClientError::AmbiguousLength);
            }
            content_length = Some(
                header_value
                    .parse()
                    .map_err(|_| RegistryClientError::AmbiguousLength)?,
            );
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            transfer_encoding = true;
        } else if name.eq_ignore_ascii_case("content-encoding")
            && !header_value.eq_ignore_ascii_case("identity")
        {
            return Err(RegistryClientError::UnsupportedContentEncoding(
                header_value.to_owned(),
            ));
        }
    }
    Ok(ParsedHeaders {
        status,
        content_length,
        transfer_encoding,
    })
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn io_error(context: &str, error: std::io::Error) -> RegistryClientError {
    RegistryClientError::Io {
        context: context.to_owned(),
        message: error.to_string(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryClientError {
    UnsupportedScheme(String),
    NetworkDisabled,
    InvalidUrl(String),
    NonLoopbackHost(String),
    InvalidFile(String),
    Io {
        context: String,
        message: String,
    },
    HeadersTooLarge {
        limit: usize,
    },
    ResponseTooLarge {
        limit: usize,
        declared: Option<u64>,
    },
    TruncatedResponse {
        expected: Option<usize>,
        received: usize,
    },
    Redirect(u16),
    HttpStatus(u16),
    UnsupportedTransferEncoding,
    UnsupportedContentEncoding(String),
    AmbiguousLength,
    MalformedResponse(String),
}

impl fmt::Display for RegistryClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedScheme(scheme) => {
                write!(formatter, "unsupported registry URL scheme {scheme:?}")
            }
            Self::NetworkDisabled => formatter.write_str("registry network access is disabled"),
            Self::InvalidUrl(message)
            | Self::InvalidFile(message)
            | Self::MalformedResponse(message) => formatter.write_str(message),
            Self::NonLoopbackHost(host) => {
                write!(formatter, "HTTP registry host is not loopback: {host}")
            }
            Self::Io { context, message } => write!(formatter, "{context}: {message}"),
            Self::HeadersTooLarge { limit } => {
                write!(formatter, "registry response headers exceed {limit} bytes")
            }
            Self::ResponseTooLarge { limit, declared } => {
                write!(
                    formatter,
                    "registry response exceeds {limit} bytes (declared {declared:?})"
                )
            }
            Self::TruncatedResponse { expected, received } => write!(
                formatter,
                "registry response truncated (expected {expected:?}, received {received})"
            ),
            Self::Redirect(status) => {
                write!(
                    formatter,
                    "registry redirects are forbidden (HTTP {status})"
                )
            }
            Self::HttpStatus(status) => write!(formatter, "registry returned HTTP {status}"),
            Self::UnsupportedTransferEncoding => {
                formatter.write_str("registry transfer encodings are forbidden")
            }
            Self::UnsupportedContentEncoding(encoding) => {
                write!(
                    formatter,
                    "unsupported registry content encoding {encoding:?}"
                )
            }
            Self::AmbiguousLength => {
                formatter.write_str("registry response has ambiguous body length")
            }
        }
    }
}

impl std::error::Error for RegistryClientError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn serve_once(response: Vec<u8>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let address = listener.local_addr().expect("listener address");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("read timeout");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request);
            stream.write_all(&response).expect("write fixture response");
        });
        (format!("http://{address}/index.json"), handle)
    }

    fn fetch_response(response: &[u8], limit: usize) -> Result<Vec<u8>, RegistryClientError> {
        let (url, handle) = serve_once(response.to_vec());
        let result = RegistryClient::with_limits(limit, 4096, Duration::from_secs(2)).fetch(&url);
        handle.join().expect("server joins");
        result
    }

    fn fetch_archive_response(response: &[u8]) -> Result<Vec<u8>, RegistryClientError> {
        let (url, handle) = serve_once(response.to_vec());
        let result = RegistryClient::with_limits(4, 4096, Duration::from_secs(2))
            .fetch_package_archive(&url);
        handle.join().expect("server joins");
        result
    }

    #[test]
    fn loopback_http_requires_exact_content_length_and_no_redirects() {
        assert_eq!(
            fetch_response(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello", 16)
                .expect("bounded response"),
            b"hello"
        );
        assert!(matches!(
            fetch_response(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nabc", 16),
            Err(RegistryClientError::TruncatedResponse { .. })
        ));
        assert!(matches!(
            fetch_response(
                b"HTTP/1.1 302 Found\r\nLocation: /other\r\nContent-Length: 0\r\n\r\n",
                16
            ),
            Err(RegistryClientError::Redirect(302))
        ));
    }

    #[test]
    fn loopback_http_finishes_at_content_length_without_waiting_for_close() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let address = listener.local_addr().expect("listener address");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("read timeout");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello")
                .expect("write complete framed response");
            thread::sleep(Duration::from_millis(250));
        });
        let url = format!("http://{address}/index.json");

        let body = RegistryClient::with_limits(16, 4096, Duration::from_millis(75))
            .fetch(&url)
            .expect("Content-Length completes the response before delayed close");
        assert_eq!(body, b"hello");
        handle.join().expect("server joins");
    }

    #[test]
    fn loopback_http_uses_one_absolute_deadline_across_slow_reads() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let address = listener.local_addr().expect("listener address");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("read timeout");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request);
            if stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 32\r\n\r\n")
                .is_err()
            {
                return;
            }
            for byte in b"0123456789abcdef0123456789abcdef" {
                if stream.write_all(std::slice::from_ref(byte)).is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(30));
            }
        });
        let url = format!("http://{address}/index.json");
        let started_at = Instant::now();

        let result = RegistryClient::with_limits(64, 4096, Duration::from_millis(120)).fetch(&url);
        let elapsed = started_at.elapsed();
        assert!(
            matches!(result, Err(RegistryClientError::Io { .. })),
            "slow trickle must exceed the single fetch deadline: {result:?}"
        );
        assert!(
            elapsed < Duration::from_millis(500),
            "absolute deadline must not restart for each byte: {elapsed:?}"
        );
        handle.join().expect("server joins");
    }

    #[test]
    fn expired_shared_operation_deadline_fails_before_connecting() {
        let deadline = Instant::now() - Duration::from_millis(1);
        let result = RegistryClient::new(64).fetch_until("http://127.0.0.1:1/index.json", deadline);

        assert!(
            matches!(result, Err(RegistryClientError::Io { ref message, .. }) if message.contains("operation deadline")),
            "expired operation deadline must fail closed before transport: {result:?}"
        );
    }

    #[test]
    fn loopback_http_rejects_oversize_chunked_and_ambiguous_lengths() {
        assert!(matches!(
            fetch_response(b"HTTP/1.1 200 OK\r\nContent-Length: 17\r\n\r\n", 16),
            Err(RegistryClientError::ResponseTooLarge { .. })
        ));
        assert!(matches!(
            fetch_response(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
                16
            ),
            Err(RegistryClientError::UnsupportedTransferEncoding)
        ));
        assert!(matches!(
            fetch_response(
                b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n",
                16
            ),
            Err(RegistryClientError::AmbiguousLength)
        ));
    }

    #[test]
    fn package_archive_fetch_uses_its_explicit_64_mib_contract_limit() {
        assert_eq!(MAX_PACKAGE_ARCHIVE_BODY_BYTES, 64 * 1024 * 1024);
        assert_eq!(
            fetch_archive_response(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello")
                .expect("archive allowance exceeds document limit"),
            b"hello"
        );
        let oversized = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
            MAX_PACKAGE_ARCHIVE_BODY_BYTES + 1
        );
        assert_eq!(
            fetch_archive_response(oversized.as_bytes()),
            Err(RegistryClientError::ResponseTooLarge {
                limit: MAX_PACKAGE_ARCHIVE_BODY_BYTES,
                declared: Some((MAX_PACKAGE_ARCHIVE_BODY_BYTES + 1) as u64),
            })
        );
    }

    #[test]
    fn transport_rejects_https_and_non_loopback_http_deterministically() {
        let client = RegistryClient::default();
        assert_eq!(
            client.fetch("https://127.0.0.1/index.json"),
            Err(RegistryClientError::UnsupportedScheme("https".to_owned()))
        );
        assert!(matches!(
            client.fetch("http://192.0.2.1/index.json"),
            Err(RegistryClientError::NonLoopbackHost(_))
        ));
        assert!(matches!(
            client.fetch("http://localhost/index.json"),
            Err(RegistryClientError::NonLoopbackHost(_))
        ));
    }

    #[test]
    fn network_disabled_mode_rejects_loopback_http_before_connecting_but_keeps_file_replay() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("index.json");
        std::fs::write(&path, b"offline").expect("write file registry fixture");
        let client = RegistryClient::default().with_network_disabled();

        assert_eq!(
            client.fetch("http://127.0.0.1:1/index.json"),
            Err(RegistryClientError::NetworkDisabled)
        );
        assert_eq!(
            client
                .fetch(&format!("file://{}", path.display()))
                .expect("local file replay remains available"),
            b"offline"
        );
    }

    #[test]
    fn artifact_urls_resolve_from_index_directory_without_escape_syntax() {
        assert_eq!(
            resolve_registry_artifact_url(
                "http://127.0.0.1:8080/catalog/index.json",
                "releases/a.json"
            )
            .expect("loopback join"),
            "http://127.0.0.1:8080/catalog/releases/a.json"
        );
        assert_eq!(
            resolve_registry_artifact_url("file:///tmp/axiom/catalog.json", "releases/a.json")
                .expect("file join"),
            "file:///tmp/axiom/releases/a.json"
        );
        for invalid in [
            "",
            "/absolute",
            "../escape",
            "a/../escape",
            "./a",
            "a//b",
            r"a\b",
            "a?query",
            "a#fragment",
            "a%2fb",
            "a\nb",
        ] {
            assert!(
                resolve_registry_artifact_url("http://127.0.0.1:8080/catalog/index.json", invalid)
                    .is_err(),
                "{invalid:?} must fail"
            );
        }
        assert_eq!(
            resolve_registry_artifact_url("https://127.0.0.1/index.json", "a.json"),
            Err(RegistryClientError::UnsupportedScheme("https".to_owned()))
        );
        for invalid_base in [
            "http://127.0.0.1:8080/catalog/../index.json",
            "http://127.0.0.1:8080/catalog/%69ndex.json",
            r"http://127.0.0.1:8080/catalog\index.json",
            "file:///tmp/catalog/../index.json",
        ] {
            assert!(
                resolve_registry_artifact_url(invalid_base, "a.json").is_err(),
                "{invalid_base:?} must fail"
            );
        }
    }

    #[test]
    fn file_transport_is_bounded_and_rejects_symlinks() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("index.json");
        std::fs::write(&path, b"hello").expect("write fixture");
        let url = format!("file://{}", path.display());
        assert_eq!(
            RegistryClient::new(5).fetch(&url).expect("read exact file"),
            b"hello"
        );
        assert!(matches!(
            RegistryClient::new(4).fetch(&url),
            Err(RegistryClientError::ResponseTooLarge { .. })
        ));
        assert_eq!(
            RegistryClient::new(4)
                .fetch_package_archive(&url)
                .expect("archive fetch has an explicit larger allowance"),
            b"hello"
        );

        #[cfg(unix)]
        {
            let link = directory.path().join("link.json");
            std::os::unix::fs::symlink(&path, &link).expect("create symlink");
            assert!(matches!(
                RegistryClient::new(16).fetch(&format!("file://{}", link.display())),
                Err(RegistryClientError::InvalidFile(_))
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn file_transport_reads_the_opened_descriptor_not_a_replaced_path() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("package.axp");
        let old_path = directory.path().join("opened-package.axp");
        let replacement = directory.path().join("replacement.axp");
        std::fs::write(&path, b"authenticated").expect("write authenticated bytes");
        std::fs::write(&replacement, b"replacement").expect("write replacement bytes");

        let opened = open_registry_file(&path).expect("open anchored descriptor");
        std::fs::rename(&path, &old_path).expect("preserve opened inode");
        std::fs::rename(&replacement, &path).expect("replace pathname");

        assert_eq!(
            read_bounded(opened, 32).expect("read already-opened descriptor"),
            b"authenticated"
        );
        assert_eq!(
            std::fs::read(&path).expect("read replacement pathname"),
            b"replacement"
        );
    }
}
