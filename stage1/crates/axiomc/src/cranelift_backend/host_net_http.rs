//! Direct-native i64 runtime lowering — host_net_http group.
//! Extracted from cranelift_backend.rs under the compiler-source
//! decomposition ratchet (#1254). Shared IR types and helpers stay in
//! the parent module and are visible here through `use super::*`.

use super::*;

pub(crate) fn populate_i64_http_static_bindings(program: &Program, static_bindings: &mut I64StaticBindings) {
    static_bindings.http_shim_wrappers = program
        .functions
        .iter()
        .filter(|function| function.path == "<stdlib>/http.ax")
        .map(|function| function.name.clone())
        .collect();
    static_bindings.http_get_wrappers = program
        .functions
        .iter()
        .filter(|function| function.path == "<stdlib>/http.ax" && function.source_name == "get")
        .map(|function| function.name.clone())
        .collect();
    static_bindings.http_serve_once_wrappers = program
        .functions
        .iter()
        .filter(|function| {
            function.path == "<stdlib>/http.ax" && function.source_name == "serve_once"
        })
        .map(|function| function.name.clone())
        .collect();
    static_bindings.http_listen_wrappers = program
        .functions
        .iter()
        .filter(|function| function.path == "<stdlib>/http.ax" && function.source_name == "listen")
        .map(|function| function.name.clone())
        .collect();
    static_bindings.http_local_port_wrappers = program
        .functions
        .iter()
        .filter(|function| {
            function.path == "<stdlib>/http.ax" && function.source_name == "local_port"
        })
        .map(|function| function.name.clone())
        .collect();
    static_bindings.http_accept_wrappers = program
        .functions
        .iter()
        .filter(|function| function.path == "<stdlib>/http.ax" && function.source_name == "accept")
        .map(|function| function.name.clone())
        .collect();
    static_bindings.http_respond_wrappers = program
        .functions
        .iter()
        .filter(|function| function.path == "<stdlib>/http.ax" && function.source_name == "respond")
        .map(|function| function.name.clone())
        .collect();
    static_bindings.http_close_wrappers = program
        .functions
        .iter()
        .filter(|function| function.path == "<stdlib>/http.ax" && function.source_name == "close")
        .map(|function| function.name.clone())
        .collect();
}

/// When `expr` is an http serve call whose static bind address is not
/// loopback-only, return the structured runtime error line the server reports
/// on stderr before refusing to serve.
pub(crate) fn i64_http_non_loopback_bind_diag(
    expr: &Expr,
    static_bindings: &I64StaticBindings,
) -> Option<String> {
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    let bind = if is_i64_http_serve_once_name(name, static_bindings) {
        let [bind, _] = args.as_slice() else {
            return None;
        };
        bind
    } else if is_i64_http_serve_route_name(name) {
        let [bind, _, _, _] = args.as_slice() else {
            return None;
        };
        bind
    } else {
        return None;
    };
    let bind = i64_string_text(bind, static_bindings)?;
    if http_parse_loopback_bind(&bind).is_some() {
        return None;
    }
    Some(String::from(HTTP_NON_LOOPBACK_BIND_DIAG))
}

pub(crate) fn lower_i64_net_option_call_let_stmts(
    name: &str,
    inner: &Type,
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    if !matches!(inner, Type::String | Type::Str) {
        return None;
    }
    let host = i64_net_resolve_host(expr, static_bindings)?;
    let net_len = i64_net_resolve_len_expr(&host, static_bindings)?;
    lower_i64_string_option_len_call_let_stmts(name, net_len, locals, local_indexes)
}

pub(crate) fn lower_i64_net_option_match_value_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let Expr::Match {
        expr: matched,
        arms,
        ty,
    } = expr
    else {
        return None;
    };
    if !is_i64_exit_type(ty) {
        return None;
    }
    let Type::Option(inner) = matched.ty() else {
        return None;
    };
    if !matches!(inner.as_ref(), Type::String | Type::Str) {
        return None;
    }
    let host = i64_net_resolve_host(matched, static_bindings)?;
    let (some_arm, none_arm) = i64_option_match_arms(arms)?;
    let binding = some_arm
        .bindings
        .first()
        .filter(|binding| binding.as_str() != "_");
    let net_len = i64_net_resolve_len_expr(&host, static_bindings)?;
    let then_result = lower_i64_net_some_arm_expr(
        &some_arm.expr,
        binding.map(String::as_str),
        &host,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let else_result = lower_i64_return_value_expr(
        &none_arm.expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    Some(CraneliftI64Expr::Select {
        cond: Box::new(CraneliftI64Condition::Compare(CraneliftI64Compare {
            op: CraneliftI64CompareOp::Ge,
            lhs: net_len,
            rhs: CraneliftI64Expr::Literal(0),
        })),
        then_result: Box::new(then_result),
        else_result: Box::new(else_result),
    })
}

pub(crate) fn lower_i64_net_some_arm_expr(
    expr: &Expr,
    binding: Option<&str>,
    host: &I64NetResolveHost,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if let Some(binding) = binding
        && let Expr::Call { name, args, .. } = expr
        && name == "len"
        && let [Expr::VarRef { name, .. }] = args.as_slice()
        && name == binding
    {
        return i64_net_resolve_len_expr(host, static_bindings);
    }
    lower_i64_return_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )
}

pub(crate) fn i64_net_resolve_host(
    expr: &Expr,
    static_bindings: &I64StaticBindings,
) -> Option<I64NetResolveHost> {
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if !is_i64_net_resolve_name(name, static_bindings) {
        return None;
    }
    let [host] = args.as_slice() else {
        return None;
    };
    let host = i64_string_text(host, static_bindings)?;
    Some(I64NetResolveHost {
        resolved_len: i64::try_from(
            i64_net_resolve_text_for_bindings(&host, static_bindings)?.len(),
        )
        .ok()?,
        host,
    })
}

pub(crate) fn i64_net_resolve_len_expr(
    host: &I64NetResolveHost,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    i64_audited_net_expr(
        "net_resolve",
        host.host.len(),
        CraneliftI64Expr::NetResolveLen {
            host: host.host.clone(),
            resolved_len: host.resolved_len,
        },
        static_bindings,
        CraneliftI64AuditSuccess::NonNegative,
    )
}

pub(crate) fn i64_net_resolve_text_for_bindings(
    host: &str,
    static_bindings: &I64StaticBindings,
) -> Option<String> {
    i64_net_resolve_text(host).or_else(|| {
        if !static_bindings.net_unrestricted && !static_bindings.net_allowed_hosts.contains(host) {
            return None;
        }
        (host, 0)
            .to_socket_addrs()
            .ok()
            .and_then(|mut addrs| addrs.next())
            .map(|addr| addr.ip().to_string())
    })
}

pub(crate) fn lower_i64_http_server_intrinsic_expr(
    name: &str,
    args: &[Expr],
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if is_i64_http_listen_name(name, static_bindings) {
        let [bind] = args else {
            return None;
        };
        let bind = i64_string_text(bind, static_bindings)?;
        let addr = http_parse_loopback_bind(&bind)?;
        return Some(CraneliftI64Expr::HttpServerListen { port: addr.port() });
    }
    if is_i64_http_local_port_name(name, static_bindings) {
        let [server] = args else {
            return None;
        };
        if let Expr::VarRef { name, .. } = server {
            return static_bindings
                .http_server_ports
                .get(name)
                .map(|port| CraneliftI64Expr::Literal(i64::from(*port)));
        }
        return None;
    }
    if is_i64_http_accept_name(name, static_bindings) {
        let [server] = args else {
            return None;
        };
        return Some(CraneliftI64Expr::HttpServerAccept {
            server: Box::new(lower_i64_expr(
                server,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?),
        });
    }
    if is_i64_http_close_name(name, static_bindings) {
        let [server] = args else {
            return None;
        };
        return Some(CraneliftI64Expr::HttpServerClose {
            server: Box::new(lower_i64_expr(
                server,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?),
        });
    }
    None
}

pub(crate) fn lower_i64_http_server_listen_local(
    name: &str,
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    static_bindings: &mut I64StaticBindings,
) -> Option<()> {
    let Expr::Call {
        name: call_name,
        args,
        ..
    } = expr
    else {
        return None;
    };
    if !is_i64_http_listen_name(call_name, static_bindings) {
        return None;
    }
    let [bind] = args.as_slice() else {
        return None;
    };
    let bind = i64_string_text(bind, static_bindings)?;
    let addr = http_parse_loopback_bind(&bind)?;
    let local = local_indexes.len();
    local_indexes.insert(name.to_string(), local);
    locals.push(CraneliftI64Expr::HttpServerListen { port: addr.port() });
    static_bindings
        .http_server_ports
        .insert(name.to_string(), addr.port());
    Some(())
}

pub(crate) fn lower_i64_http_request_stream_expr(
    request: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    match request {
        Expr::VarRef {
            name,
            ty: Type::Struct(_),
        } => local_indexes
            .get(i64_struct_projection_key(name, "stream").as_str())
            .copied()
            .map(CraneliftI64Expr::Local),
        Expr::FieldAccess { base, field, .. } if field == "stream" => lower_i64_expr(
            request,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        ),
        _ => lower_i64_expr(
            request,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        ),
    }
}

pub(crate) fn i64_audited_net_expr(
    intrinsic: &str,
    host_len: usize,
    result: CraneliftI64Expr,
    static_bindings: &I64StaticBindings,
    success: CraneliftI64AuditSuccess,
) -> Option<CraneliftI64Expr> {
    let package = static_bindings.package_root.as_deref()?;
    Some(CraneliftI64Expr::AuditNet {
        intrinsic: intrinsic.to_string(),
        package: package.display().to_string(),
        host_len,
        success,
        result: Box::new(result),
    })
}

pub(crate) fn is_i64_std_http_shim_wrapper(function: &Function) -> bool {
    // std/http.ax functions are thin wrappers over runtime-only host
    // intrinsics (listen/accept/serve/respond/get/local_port/close). Their
    // calls are handled at call sites (e.g. serve_once -> HttpServeOnce), so
    // they must not be lowered as i64 helper functions -- their bodies do not
    // lower and would otherwise fail the whole-program i64 path.
    function.path == "<stdlib>/http.ax"
}

pub(crate) fn is_i64_std_net_shim_wrapper(function: &Function) -> bool {
    matches!(
        (function.path.as_str(), function.source_name.as_str()),
        (
            "<stdlib>/net.ax",
            "resolve"
                | "tcp_listen_loopback_once"
                | "tcp_dial"
                | "udp_bind_loopback_once"
                | "udp_send_recv"
        )
    )
}

pub(crate) fn is_i64_std_net_wrapper(function: &Function, source_name: &str) -> bool {
    function.path == "<stdlib>/net.ax" && function.source_name == source_name
}

pub(crate) fn is_i64_net_resolve_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    matches!(name, "net_resolve" | "resolve" | "std_net_resolve")
        || static_bindings.net_resolve_wrappers.contains(name)
}

pub(crate) fn is_i64_net_tcp_loopback_once_name(name: &str) -> bool {
    matches!(
        name,
        "net_tcp_listen_loopback_once"
            | "tcp_listen_loopback_once"
            | "std_net_tcp_listen_loopback_once"
    )
}

pub(crate) fn is_i64_net_udp_loopback_once_name(name: &str) -> bool {
    matches!(
        name,
        "net_udp_bind_loopback_once" | "udp_bind_loopback_once" | "std_net_udp_bind_loopback_once"
    )
}

pub(crate) fn is_i64_http_get_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    matches!(name, "http_get" | "get" | "std_http_get")
        || static_bindings.http_get_wrappers.contains(name)
}

pub(crate) fn is_i64_http_serve_once_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    matches!(
        name,
        "http_serve_once" | "serve_once" | "std_http_serve_once"
    ) || static_bindings.http_serve_once_wrappers.contains(name)
}

pub(crate) fn is_i64_http_serve_route_name(name: &str) -> bool {
    name == "http_serve_route"
}

pub(crate) fn is_i64_http_listen_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    matches!(name, "http_server_listen" | "listen" | "std_http_listen")
        || static_bindings.http_listen_wrappers.contains(name)
}

pub(crate) fn is_i64_http_local_port_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    matches!(
        name,
        "http_server_local_port" | "local_port" | "std_http_local_port"
    ) || static_bindings.http_local_port_wrappers.contains(name)
}

pub(crate) fn is_i64_http_accept_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    matches!(name, "http_server_accept" | "accept" | "std_http_accept")
        || static_bindings.http_accept_wrappers.contains(name)
}

pub(crate) fn is_i64_http_respond_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    matches!(name, "http_response_write" | "respond" | "std_http_respond")
        || static_bindings.http_respond_wrappers.contains(name)
}

pub(crate) fn is_i64_http_close_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    matches!(name, "http_server_close" | "close" | "std_http_close")
        || static_bindings.http_close_wrappers.contains(name)
}

pub(crate) fn net_timeout(timeout_ms: i64) -> std::time::Duration {
    std::time::Duration::from_millis(timeout_ms.clamp(1, 30_000) as u64)
}

pub(crate) fn net_tcp_listen_loopback_once(response: String, timeout: std::time::Duration) -> Option<i64> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).ok()?;
    listener.set_nonblocking(true).ok()?;
    let port = listener.local_addr().ok()?.port();
    std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match listener.accept() {
                Ok((mut stream, _peer)) => {
                    let _ = stream.set_read_timeout(Some(timeout));
                    let _ = stream.set_write_timeout(Some(timeout));
                    let mut total_read = 0usize;
                    let mut buf = [0u8; 4096];
                    loop {
                        match stream.read(&mut buf) {
                            Ok(0) => break,
                            Ok(read) => {
                                total_read = total_read.saturating_add(read);
                                if total_read >= 65_536 {
                                    break;
                                }
                            }
                            Err(err)
                                if matches!(
                                    err.kind(),
                                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                                ) =>
                            {
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                    break;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    });
    Some(i64::from(port))
}

pub(crate) fn net_udp_bind_loopback_once(response: String, timeout: std::time::Duration) -> Option<i64> {
    let socket = std::net::UdpSocket::bind(("127.0.0.1", 0)).ok()?;
    socket.set_read_timeout(Some(timeout)).ok()?;
    socket.set_write_timeout(Some(timeout)).ok()?;
    let port = socket.local_addr().ok()?.port();
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        if let Ok((_n, peer)) = socket.recv_from(&mut buf) {
            let _ = socket.send_to(response.as_bytes(), peer);
        }
    });
    Some(i64::from(port))
}

pub(crate) fn spike_http_servers() -> &'static Mutex<HashMap<i64, SpikeHttpServer>> {
    SPIKE_HTTP_SERVERS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn spike_http_requests() -> &'static Mutex<HashMap<i64, SpikeHttpRequest>> {
    SPIKE_HTTP_REQUESTS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn spike_http_next_handle() -> i64 {
    SPIKE_HTTP_NEXT_HANDLE.fetch_add(1, Ordering::Relaxed)
}

pub(crate) fn http_get(url: &str) -> Option<String> {
    let (scheme, host, port, path) = http_split_url(url)?;
    if scheme != "http" {
        return None;
    }
    let host = http_strip_crlf(host);
    let path = http_strip_crlf(path);
    if host.is_empty() || path.is_empty() {
        return None;
    }
    let request = http_request(&host, &path);
    let addrs = resolve_public_socket_addrs(host.as_str(), port)?;
    let mut stream = None;
    for addr in addrs {
        if let Ok(candidate) =
            std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(5))
        {
            stream = Some(candidate);
            break;
        }
    }
    let mut stream = stream?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok()?;
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(5)))
        .ok()?;
    stream.write_all(request.as_bytes()).ok()?;
    http_read_response(&mut stream)
}

pub(crate) fn resolve_public_socket_addrs(host: &str, port: u16) -> Option<Vec<std::net::SocketAddr>> {
    let addrs: Vec<std::net::SocketAddr> = (host, port).to_socket_addrs().ok()?.collect();
    if addrs.is_empty() || addrs.iter().any(|addr| is_blocked_network_ip(addr.ip())) {
        return None;
    }
    Some(addrs)
}

pub(crate) fn is_blocked_network_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(addr) => {
            let octets = addr.octets();
            addr.is_private()
                || addr.is_loopback()
                || addr.is_link_local()
                || addr.is_unspecified()
                || addr.is_broadcast()
                || addr.is_multicast()
                || octets[0] == 0
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
                || (octets[0] == 198 && (18..=19).contains(&octets[1]))
                || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
                || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113)
        }
        std::net::IpAddr::V6(addr) => {
            if let Some(mapped) = addr.to_ipv4_mapped() {
                return is_blocked_network_ip(std::net::IpAddr::V4(mapped));
            }
            let segments = addr.segments();
            addr.is_loopback()
                || addr.is_unspecified()
                || addr.is_multicast()
                || (segments[0] & 0xfe00) == 0xfc00
                || (segments[0] & 0xffc0) == 0xfe80
                || (segments[0] == 0x2001 && segments[1] == 0x0db8)
        }
    }
}

pub(crate) fn http_strip_crlf(value: &str) -> String {
    value
        .chars()
        .filter(|ch| *ch != '\r' && *ch != '\n')
        .collect()
}

pub(crate) fn http_split_url(url: &str) -> Option<(&str, &str, u16, &str)> {
    let (scheme, rest, default_port) = if let Some(rest) = url.strip_prefix("http://") {
        ("http", rest, 80u16)
    } else if let Some(rest) = url.strip_prefix("https://") {
        ("https", rest, 443u16)
    } else {
        return None;
    };
    let (host_port, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    if host_port.is_empty() {
        return None;
    }
    let (host, port) = match host_port.rfind(':') {
        Some(index) => {
            let parsed = host_port[index + 1..].parse().ok()?;
            (&host_port[..index], parsed)
        }
        None => (host_port, default_port),
    };
    if host.is_empty() {
        return None;
    }
    Some((scheme, host, port, path))
}

pub(crate) fn http_request(host: &str, path: &str) -> String {
    format!(
        "GET {} HTTP/1.0\r\nHost: {}\r\nUser-Agent: axiom-stage1/0.1\r\nConnection: close\r\n\r\n",
        path, host
    )
}

pub(crate) fn http_read_response<R: Read>(reader: &mut R) -> Option<String> {
    const MAX_HEADER_BYTES: usize = 64 * 1024;
    const MAX_BODY_BYTES: usize = 1024 * 1024;
    let mut raw = Vec::new();
    let mut body_start = None;
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
        if body_start.is_none() {
            if let Some(separator) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                if separator > MAX_HEADER_BYTES {
                    return None;
                }
                body_start = Some(separator + 4);
            } else if raw.len() > MAX_HEADER_BYTES {
                return None;
            }
        }
        if let Some(start) = body_start {
            if raw.len().saturating_sub(start) > MAX_BODY_BYTES {
                return None;
            }
        }
    }
    let body_start = body_start?;
    let header_end = body_start - 4;
    let head = &raw[..header_end];
    let body = &raw[body_start..];
    let status_line_end = head
        .iter()
        .position(|byte| *byte == b'\r')
        .unwrap_or(head.len());
    let status_line = std::str::from_utf8(&head[..status_line_end]).ok()?;
    let mut parts = status_line.splitn(3, ' ');
    let _version = parts.next()?;
    let status_code: u16 = parts.next()?.parse().ok()?;
    if !(200..300).contains(&status_code) {
        return None;
    }
    String::from_utf8(body.to_vec()).ok()
}

pub(crate) fn http_server_listen(bind: &str) -> Option<i64> {
    let addr = http_parse_loopback_bind(bind)?;
    let listener = TcpListener::bind(addr).ok()?;
    listener.set_nonblocking(true).ok()?;
    let handle = spike_http_next_handle();
    spike_http_servers()
        .lock()
        .ok()?
        .insert(handle, SpikeHttpServer { listener });
    Some(handle)
}

pub(crate) fn http_server_local_port(server: i64) -> Option<i64> {
    let servers = spike_http_servers().lock().ok()?;
    let server = servers.get(&server)?;
    Some(i64::from(server.listener.local_addr().ok()?.port()))
}

pub(crate) fn http_server_accept(server: i64) -> Option<i64> {
    let listener = {
        let servers = spike_http_servers().lock().ok()?;
        servers.get(&server)?.listener.try_clone().ok()?
    };
    let request = http_accept_request(&listener)?;
    let handle = spike_http_next_handle();
    spike_http_requests().lock().ok()?.insert(handle, request);
    Some(handle)
}

pub(crate) fn http_request_part(request: i64, name: &str) -> Option<String> {
    let requests = spike_http_requests().lock().ok()?;
    let request = requests.get(&request)?;
    match name {
        "http_request_method" => Some(request.method.clone()),
        "http_request_path" => Some(request.path.clone()),
        "http_request_body" => Some(request.body.clone()),
        _ => None,
    }
}

pub(crate) fn http_response_write(request: i64, status: i64, body: &str) -> bool {
    let Some(mut request) = spike_http_requests()
        .lock()
        .ok()
        .and_then(|mut requests| requests.remove(&request))
    else {
        return false;
    };
    let response = http_response(status, body);
    request.stream.write_all(response.as_bytes()).is_ok() && request.stream.flush().is_ok()
}

pub(crate) fn http_server_close(server: i64) -> bool {
    spike_http_servers()
        .lock()
        .ok()
        .and_then(|mut servers| servers.remove(&server))
        .is_some()
}

pub(crate) fn i64_http_ok_response(body: &str) -> String {
    i64_http_response_with_status("200 OK", body)
}

pub(crate) fn i64_http_not_found_response() -> String {
    i64_http_response_with_status("404 Not Found", "not found")
}

pub(crate) fn i64_http_response(status: i64, body: &str) -> String {
    i64_http_response_with_status(&i64_http_status_line(status), body)
}

pub(crate) fn i64_http_status_line(status: i64) -> String {
    match status {
        200 => String::from("200 OK"),
        201 => String::from("201 Created"),
        202 => String::from("202 Accepted"),
        204 => String::from("204 No Content"),
        400 => String::from("400 Bad Request"),
        404 => String::from("404 Not Found"),
        405 => String::from("405 Method Not Allowed"),
        500 => String::from("500 Internal Server Error"),
        code if (100..=999).contains(&code) => format!("{code} OK"),
        _ => String::from("500 Internal Server Error"),
    }
}

pub(crate) fn i64_http_response_with_status(status: &str, body: &str) -> String {
    // Mirror the generated runtime's axiom_http_response_with_status.
    format!(
        "HTTP/1.0 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

pub(crate) fn http_serve_once(bind: &str, body: &str) -> bool {
    let Some(server) = http_server_listen(bind) else {
        return false;
    };
    let result = http_server_accept(server)
        .map(|request| http_response_write(request, 200, body))
        .unwrap_or(false);
    let _ = http_server_close(server);
    result
}

pub(crate) fn http_serve_route(bind: &str, route_path: &str, body: &str, max_requests: i64) -> bool {
    let Some(server) = http_server_listen(bind) else {
        return false;
    };
    let result = http_serve_route_on_server(server, route_path, body, max_requests);
    let _ = http_server_close(server);
    result
}

pub(crate) fn http_serve_route_on_server(
    server: i64,
    route_path: &str,
    body: &str,
    max_requests: i64,
) -> bool {
    if max_requests <= 0 {
        return false;
    }
    let route_path = http_strip_crlf(route_path);
    if route_path.is_empty() {
        return false;
    }
    let mut served = 0i64;
    while served < max_requests {
        let Some(request) = http_server_accept(server) else {
            return false;
        };
        let Some(path) = http_request_part(request, "http_request_path") else {
            return false;
        };
        let matched = path == route_path;
        let status = if matched { 200 } else { 404 };
        let response_body = if matched { body } else { "not found" };
        if !http_response_write(request, status, response_body) {
            return false;
        }
        served += 1;
    }
    true
}

pub(crate) fn http_parse_loopback_bind(bind: &str) -> Option<SocketAddr> {
    let addr = bind.parse::<SocketAddr>().ok().or_else(|| {
        let (host, port) = bind.rsplit_once(':')?;
        if host != "localhost" {
            return None;
        }
        let port = port.parse::<u16>().ok()?;
        Some(SocketAddr::from(([127, 0, 0, 1], port)))
    })?;
    if !addr.ip().is_loopback() {
        return None;
    }
    Some(addr)
}

pub(crate) fn http_accept_request(listener: &TcpListener) -> Option<SpikeHttpRequest> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match listener.accept() {
            Ok((mut stream, _peer)) => {
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .ok()?;
                stream
                    .set_write_timeout(Some(std::time::Duration::from_secs(2)))
                    .ok()?;
                let (method, path, body) = http_read_request(&mut stream)?;
                return Some(SpikeHttpRequest {
                    stream,
                    method,
                    path,
                    body,
                });
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(_) => return None,
        }
    }
}

pub(crate) fn http_read_request<R: Read>(reader: &mut R) -> Option<(String, String, String)> {
    const MAX_HEADER_BYTES: usize = 64 * 1024;
    const MAX_BODY_BYTES: usize = 1024 * 1024;
    let mut raw = Vec::new();
    let mut header_end = None;
    let mut content_length = 0usize;
    let mut buf = [0u8; 4096];
    loop {
        let n = reader.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
        if header_end.is_none() {
            if let Some(separator) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                if separator > MAX_HEADER_BYTES {
                    return None;
                }
                header_end = Some(separator + 4);
                let headers = std::str::from_utf8(&raw[..separator]).ok()?;
                content_length = http_content_length(headers)?;
                if content_length > MAX_BODY_BYTES {
                    return None;
                }
            } else if raw.len() > MAX_HEADER_BYTES {
                return None;
            }
        }
        if let Some(end) = header_end {
            if raw.len().saturating_sub(end) >= content_length {
                break;
            }
        }
    }
    let header_end = header_end?;
    let header = std::str::from_utf8(&raw[..header_end - 4]).ok()?;
    let (method, path) = http_request_line(header)?;
    let body_end = header_end.checked_add(content_length)?;
    let body = String::from_utf8(raw.get(header_end..body_end)?.to_vec()).ok()?;
    Some((method, path, body))
}

pub(crate) fn http_content_length(headers: &str) -> Option<usize> {
    for line in headers.lines().skip(1) {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            return value.trim().parse().ok();
        }
    }
    Some(0)
}

pub(crate) fn http_request_line(headers: &str) -> Option<(String, String)> {
    let line = headers.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = http_strip_crlf(parts.next()?);
    let path = http_strip_crlf(parts.next()?);
    if method.is_empty() || path.is_empty() {
        return None;
    }
    Some((method, path))
}

pub(crate) fn http_response(status: i64, body: &str) -> String {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    };
    format!(
        "HTTP/1.0 {} {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        reason,
        body.len(),
        body
    )
}

pub(crate) fn i64_net_resolve_text(host: &str) -> Option<String> {
    if host == "localhost" {
        return Some("127.0.0.1".to_string());
    }
    let addrs: Vec<std::net::SocketAddr> = (host, 0).to_socket_addrs().ok()?.collect();
    if addrs.is_empty()
        || addrs
            .iter()
            .any(|addr| i64_is_blocked_network_ip(addr.ip()))
    {
        return None;
    }
    addrs.into_iter().next().map(|addr| addr.ip().to_string())
}

pub(crate) fn i64_is_blocked_network_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(addr) => {
            let octets = addr.octets();
            addr.is_private()
                || addr.is_loopback()
                || addr.is_link_local()
                || addr.is_unspecified()
                || addr.is_broadcast()
                || addr.is_multicast()
                || octets[0] == 0
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
                || (octets[0] == 198 && (18..=19).contains(&octets[1]))
                || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
                || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113)
        }
        std::net::IpAddr::V6(addr) => {
            if let Some(mapped) = addr.to_ipv4_mapped() {
                return i64_is_blocked_network_ip(std::net::IpAddr::V4(mapped));
            }
            let segments = addr.segments();
            addr.is_loopback()
                || addr.is_unspecified()
                || addr.is_multicast()
                || (segments[0] & 0xfe00) == 0xfc00
                || (segments[0] & 0xffc0) == 0xfe80
                || (segments[0] == 0x2001 && segments[1] == 0x0db8)
        }
    }
}

