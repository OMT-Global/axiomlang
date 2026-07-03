use super::model::{Expr, LiteralValue, Type};
use super::signatures::FunctionSig;
use super::{Binding, LowerContext, NetBindingOrigin};
use crate::diagnostics::Diagnostic;
use crate::manifest::{CapabilityConfig, CapabilityKind};
use crate::syntax;
use std::collections::HashMap;

pub(super) fn validate_ffi_signature(
    function: &syntax::Function,
    return_ty: &Type,
) -> Result<(), Diagnostic> {
    validate_ffi_type(return_ty, function.line, function.column)?;
    for param in &function.params {
        validate_ffi_type_name(&param.ty, param.line, param.column)?;
    }
    Ok(())
}

fn validate_ffi_type_name(
    ty: &syntax::TypeName,
    line: usize,
    column: usize,
) -> Result<(), Diagnostic> {
    match ty {
        syntax::TypeName::Int
        | syntax::TypeName::Numeric(_)
        | syntax::TypeName::Bool
        | syntax::TypeName::String
        | syntax::TypeName::Str => Ok(()),
        syntax::TypeName::Ptr(inner) | syntax::TypeName::MutPtr(inner) => {
            validate_ffi_type_name(inner, line, column)
        }
        _ => Err(Diagnostic::new(
            "type",
            "FFI signatures only support int, bool, string, ptr<T>, and mutptr<T> in stage1",
        )
        .with_span(line, column)),
    }
}

fn validate_ffi_type(ty: &Type, line: usize, column: usize) -> Result<(), Diagnostic> {
    match ty {
        Type::Int | Type::Numeric(_) | Type::Bool | Type::String | Type::Str => Ok(()),
        Type::Ptr(inner) | Type::MutPtr(inner) => validate_ffi_type(inner, line, column),
        _ => Err(Diagnostic::new(
            "type",
            "FFI signatures only support int, bool, string, ptr<T>, and mutptr<T> in stage1",
        )
        .with_span(line, column)),
    }
}

pub(super) fn validate_net_host_allowlist_hir(
    capabilities: &CapabilityConfig,
    intrinsic_name: &str,
    host: &Expr,
    line: usize,
    column: usize,
    allow_dynamic_host: bool,
) -> Result<(), Diagnostic> {
    if capabilities.net_hosts.is_empty() {
        return match host {
            Expr::Literal {
                value: LiteralValue::String(_),
                ..
            } => Ok(()),
            _ if allow_dynamic_host => Ok(()),
            _ => Err(Diagnostic::new(
                "capability",
                format!(
                    "call to {intrinsic_name:?} requires a string literal when [capabilities].net hosts are unrestricted"
                ),
            )
            .with_span(line, column)),
        };
    }
    match host {
        Expr::Literal {
            value: LiteralValue::String(value),
            ..
        } if capabilities
            .net_hosts
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(value)) => Ok(()),
        Expr::Literal {
            value: LiteralValue::String(value),
            ..
        } => Err(Diagnostic::new(
            "capability",
            format!(
                "call to {intrinsic_name:?} requires [capabilities].net.hosts to include {value:?}"
            ),
        )
        .with_span(line, column)),
        _ if allow_dynamic_host => Ok(()),
        _ => Err(Diagnostic::new(
            "capability",
            format!(
                "call to {intrinsic_name:?} requires a string literal listed in [capabilities].net.hosts"
            ),
        )
        .with_span(line, column)),
    }
}

pub(super) fn validate_http_get_net_allowlist_hir(
    capabilities: &CapabilityConfig,
    intrinsic_name: &str,
    url: &Expr,
    line: usize,
    column: usize,
    allow_dynamic_url: bool,
) -> Result<(), Diagnostic> {
    match url {
        Expr::Literal {
            value: LiteralValue::String(value),
            ..
        } => {
            let Some((_scheme, host, port, _path)) = split_http_url_literal(value) else {
                return Err(Diagnostic::new(
                    "capability",
                    format!("{intrinsic_name:?} requires a static http:// or https:// URL literal"),
                )
                .with_span(line, column));
            };
            if !capabilities.net_hosts.is_empty()
                && !capabilities
                    .net_hosts
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(host))
            {
                return Err(Diagnostic::new(
                    "capability",
                    format!(
                        "call to {intrinsic_name:?} requires [capabilities].net.hosts to include {host:?}"
                    ),
                )
                .with_span(line, column));
            }
            if !capabilities.net_ports.is_empty() && !capabilities.net_ports.contains(&port) {
                return Err(Diagnostic::new(
                    "capability",
                    format!(
                        "call to {intrinsic_name:?} requires [capabilities].net.ports to include {port}"
                    ),
                )
                .with_span(line, column));
            }
            Ok(())
        }
        _ if allow_dynamic_url => Ok(()),
        _ if capabilities.net_hosts.is_empty() && capabilities.net_ports.is_empty() => {
            Err(Diagnostic::new(
                "capability",
                format!(
                    "call to {intrinsic_name:?} requires a static URL literal when [capabilities].net is unrestricted"
                ),
            )
            .with_span(line, column))
        }
        _ => Err(Diagnostic::new(
            "capability",
            format!(
                "call to {intrinsic_name:?} requires a static URL literal when [capabilities].net host or port allowlists are configured"
            ),
        )
        .with_span(line, column)),
    }
}

fn split_http_url_literal(url: &str) -> Option<(&str, &str, u16, &str)> {
    let (scheme, rest, default_port) = if let Some(rest) = url.strip_prefix("http://") {
        ("http", rest, 80)
    } else if let Some(rest) = url.strip_prefix("https://") {
        ("https", rest, 443)
    } else {
        return None;
    };
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if authority.is_empty() {
        return None;
    }
    let (host, port) = if let Some((host, port)) = authority.rsplit_once(':') {
        if host.is_empty() {
            return None;
        }
        (host, port.parse::<u16>().ok()?)
    } else {
        (authority, default_port)
    };
    let path = if path.is_empty() { "/" } else { path };
    Some((scheme, host, port, path))
}

pub(super) fn validate_net_socket_allowlist_hir(
    capabilities: &CapabilityConfig,
    intrinsic_name: &str,
    socket_addr: &Expr,
    line: usize,
    column: usize,
    allow_dynamic_socket: bool,
) -> Result<(), Diagnostic> {
    match socket_addr {
        Expr::Literal {
            value: LiteralValue::String(value),
            ..
        } => {
            let Some((host, port)) = split_socket_addr_literal(value) else {
                return Err(Diagnostic::new(
                    "capability",
                    format!(
                        "call to {intrinsic_name:?} requires a static host:port literal"
                    ),
                )
                .with_span(line, column));
            };
            if !capabilities.net_hosts.is_empty()
                && !capabilities
                    .net_hosts
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(host))
            {
                return Err(Diagnostic::new(
                    "capability",
                    format!(
                        "call to {intrinsic_name:?} requires [capabilities].net.hosts to include {host:?}"
                    ),
                )
                .with_span(line, column));
            }
            if !capabilities.net_ports.is_empty() && !capabilities.net_ports.contains(&port) {
                return Err(Diagnostic::new(
                    "capability",
                    format!(
                        "call to {intrinsic_name:?} requires [capabilities].net.ports to include {port}"
                    ),
                )
                .with_span(line, column));
            }
            Ok(())
        }
        _ if allow_dynamic_socket => Ok(()),
        _ if capabilities.net_hosts.is_empty() && capabilities.net_ports.is_empty() => {
            Err(Diagnostic::new(
                "capability",
                format!(
                    "call to {intrinsic_name:?} requires a static host:port literal when [capabilities].net is unrestricted"
                ),
            )
            .with_span(line, column))
        }
        _ => Err(Diagnostic::new(
            "capability",
            format!(
                "call to {intrinsic_name:?} requires a static host:port literal when [capabilities].net host or port allowlists are configured"
            ),
        )
        .with_span(line, column)),
    }
}

fn split_socket_addr_literal(value: &str) -> Option<(&str, u16)> {
    if let Some(rest) = value.strip_prefix('[') {
        let (host, port) = rest.split_once("]:")?;
        return Some((host, port.parse::<u16>().ok()?));
    }
    let (host, port) = value.rsplit_once(':')?;
    if host.is_empty() {
        return None;
    }
    Some((host, port.parse::<u16>().ok()?))
}

pub(super) fn validate_net_port_allowlist_hir(
    capabilities: &CapabilityConfig,
    intrinsic_name: &str,
    port: &Expr,
    line: usize,
    column: usize,
    allow_dynamic_port: bool,
) -> Result<(), Diagnostic> {
    if capabilities.net_ports.is_empty() {
        return match port {
            Expr::Literal {
                value: LiteralValue::Int(value),
                ..
            } if u16::try_from(*value).is_ok() => Ok(()),
            Expr::Literal {
                value: LiteralValue::Int(value),
                ..
            } => Err(Diagnostic::new(
                "capability",
                format!("call to {intrinsic_name:?} requires a valid u16 port, got {value}"),
            )
            .with_span(line, column)),
            _ if allow_dynamic_port => Ok(()),
            _ => Err(Diagnostic::new(
                "capability",
                format!(
                    "call to {intrinsic_name:?} requires an integer literal when [capabilities].net ports are unrestricted"
                ),
            )
            .with_span(line, column)),
        };
    }
    match port {
        Expr::Literal {
            value: LiteralValue::Int(value),
            ..
        } if u16::try_from(*value)
            .ok()
            .is_some_and(|value| capabilities.net_ports.contains(&value)) => Ok(()),
        Expr::Literal {
            value: LiteralValue::Int(value),
            ..
        } => Err(Diagnostic::new(
            "capability",
            format!(
                "call to {intrinsic_name:?} requires [capabilities].net.ports to include {value}"
            ),
        )
        .with_span(line, column)),
        _ if allow_dynamic_port => Ok(()),
        _ => Err(Diagnostic::new(
            "capability",
            format!(
                "call to {intrinsic_name:?} requires an integer literal listed in [capabilities].net.ports"
            ),
        )
        .with_span(line, column)),
    }
}

pub(super) fn validate_stdlib_network_wrapper_call_hir(
    _ctx: &LowerContext<'_>,
    capabilities: &CapabilityConfig,
    env: &HashMap<String, Binding>,
    function_name: &str,
    signature: &FunctionSig,
    args: &[Expr],
    line: usize,
    column: usize,
) -> Result<(), Diagnostic> {
    let diagnostic_name = if signature.source_path.starts_with("<stdlib>/") {
        signature.source_name.as_str()
    } else {
        function_name
    };
    match (
        signature.source_path.as_str(),
        signature.source_name.as_str(),
    ) {
        ("<stdlib>/net.ax", "resolve") => validate_net_host_allowlist_hir(
            capabilities,
            diagnostic_name,
            &args[0],
            line,
            column,
            false,
        ),
        ("<stdlib>/net.ax", "tcp_dial")
        | ("<stdlib>/net.ax", "udp_send_recv")
        | ("<stdlib>/net_tcp.ax", "dial")
        | ("<stdlib>/net_udp.ax", "send_recv")
        | ("<stdlib>/async_net.ax", "tcp_dial")
        | ("<stdlib>/async_net.ax", "udp_send_recv") => {
            validate_net_host_allowlist_hir(
                capabilities,
                diagnostic_name,
                &args[0],
                line,
                column,
                false,
            )?;
            validate_net_port_allowlist_hir(
                capabilities,
                diagnostic_name,
                &args[1],
                line,
                column,
                // A dynamic peer port is fine when ports are unrestricted (there
                // is no allowlist to check a literal against); a loopback
                // listener port is also always allowed. When ports are
                // restricted, `validate_net_port_allowlist_hir` still requires a
                // literal drawn from the allowlist.
                is_loopback_listener_port_expr(&args[1], env) || capabilities.net_ports.is_empty(),
            )
        }
        ("<stdlib>/net_tcp.ax", "listen")
        | ("<stdlib>/net_udp.ax", "bind")
        | ("<stdlib>/async_net.ax", "listen") => validate_net_socket_allowlist_hir(
            capabilities,
            diagnostic_name,
            &args[0],
            line,
            column,
            net_socket_allowlist_is_unrestricted(capabilities),
        ),
        ("<stdlib>/net_udp.ax", "send_to") => validate_net_socket_allowlist_hir(
            capabilities,
            diagnostic_name,
            &args[2],
            line,
            column,
            net_socket_allowlist_is_unrestricted(capabilities),
        ),
        ("<stdlib>/http.ax", "get") => validate_http_get_net_allowlist_hir(
            capabilities,
            diagnostic_name,
            &args[0],
            line,
            column,
            false,
        ),
        ("<stdlib>/http.ax", "listen")
        | ("<stdlib>/http.ax", "serve")
        | ("<stdlib>/http.ax", "serve_once") => validate_net_socket_allowlist_hir(
            capabilities,
            diagnostic_name,
            &args[0],
            line,
            column,
            false,
        ),
        _ => Ok(()),
    }
}

fn is_stdlib_net_host_wrapper(ctx: &LowerContext<'_>) -> bool {
    ctx.current_path == "<stdlib>/net.ax" && ctx.current_function.as_deref() == Some("resolve")
}

pub(super) fn stdlib_dynamic_net_host_allowed(
    _capabilities: &CapabilityConfig,
    ctx: &LowerContext<'_>,
) -> bool {
    is_stdlib_net_host_wrapper(ctx)
}

fn is_stdlib_net_peer_wrapper(ctx: &LowerContext<'_>) -> bool {
    matches!(
        (ctx.current_path, ctx.current_function.as_deref()),
        ("<stdlib>/net.ax", Some("tcp_dial"))
            | ("<stdlib>/net.ax", Some("udp_send_recv"))
            | ("<stdlib>/net_tcp.ax", Some("dial"))
            | ("<stdlib>/net_udp.ax", Some("send_recv"))
            | ("<stdlib>/async_net.ax", Some("tcp_dial"))
            | ("<stdlib>/async_net.ax", Some("udp_send_recv"))
    )
}

pub(super) fn stdlib_dynamic_net_peer_host_allowed(
    _capabilities: &CapabilityConfig,
    ctx: &LowerContext<'_>,
) -> bool {
    is_stdlib_net_peer_wrapper(ctx)
}

pub(super) fn stdlib_dynamic_net_peer_port_allowed(
    _capabilities: &CapabilityConfig,
    ctx: &LowerContext<'_>,
) -> bool {
    is_stdlib_net_peer_wrapper(ctx)
}

fn is_stdlib_net_socket_wrapper(ctx: &LowerContext<'_>) -> bool {
    matches!(
        (ctx.current_path, ctx.current_function.as_deref()),
        ("<stdlib>/net_tcp.ax", Some("listen"))
            | ("<stdlib>/net_udp.ax", Some("bind"))
            | ("<stdlib>/net_udp.ax", Some("send_to"))
            | ("<stdlib>/async_net.ax", Some("listen"))
    )
}

fn net_socket_allowlist_is_unrestricted(capabilities: &CapabilityConfig) -> bool {
    capabilities.net_hosts.is_empty() && capabilities.net_ports.is_empty()
}

pub(super) fn stdlib_dynamic_net_socket_allowed(
    _capabilities: &CapabilityConfig,
    ctx: &LowerContext<'_>,
) -> bool {
    is_stdlib_net_socket_wrapper(ctx)
}

pub(super) fn is_stdlib_http_get_wrapper(ctx: &LowerContext<'_>) -> bool {
    ctx.current_path == "<stdlib>/http.ax" && ctx.current_function.as_deref() == Some("get")
}

fn is_stdlib_http_socket_wrapper(ctx: &LowerContext<'_>) -> bool {
    matches!(
        (ctx.current_path, ctx.current_function.as_deref()),
        ("<stdlib>/http.ax", Some("listen"))
            | ("<stdlib>/http.ax", Some("serve"))
            | ("<stdlib>/http.ax", Some("serve_once"))
    )
}

pub(super) fn stdlib_dynamic_http_socket_allowed(
    _capabilities: &CapabilityConfig,
    ctx: &LowerContext<'_>,
) -> bool {
    is_stdlib_http_socket_wrapper(ctx)
}

pub(super) fn net_binding_origin_from_expr(
    expr: &Expr,
    env: &HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Option<NetBindingOrigin> {
    match expr {
        Expr::Await { expr, .. } => net_binding_origin_from_expr(expr, env, ctx),
        Expr::Call { name, args, .. } if is_tcp_listener_call_name(name, ctx) => {
            if args
                .first()
                .and_then(static_string_literal)
                .is_some_and(is_loopback_ephemeral_bind)
            {
                Some(NetBindingOrigin::LoopbackTcpListener)
            } else {
                None
            }
        }
        Expr::Call { name, args, .. } if is_tcp_listener_port_call_name(name, ctx) => {
            let Some(Expr::VarRef {
                name: listener_name,
                ..
            }) = args.first()
            else {
                return None;
            };
            env.get(listener_name)
                .and_then(|binding| binding.net_origin.as_ref())
                .filter(|origin| matches!(origin, NetBindingOrigin::LoopbackTcpListener))
                .map(|_| NetBindingOrigin::LoopbackTcpListenerPort)
        }
        _ => None,
    }
}

fn static_string_literal(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Literal {
            value: LiteralValue::String(value),
            ..
        } => Some(value.as_str()),
        _ => None,
    }
}

fn is_loopback_ephemeral_bind(value: &str) -> bool {
    split_socket_addr_literal(value)
        .is_some_and(|(host, port)| port == 0 && matches!(host, "127.0.0.1" | "localhost" | "::1"))
}

fn is_tcp_listener_call_name(name: &str, ctx: &LowerContext<'_>) -> bool {
    if name == "net_tcp_listen" {
        return true;
    }
    ctx.functions.get(name).is_some_and(|signature| {
        matches!(
            (
                signature.source_path.as_str(),
                signature.source_name.as_str()
            ),
            ("<stdlib>/net_tcp.ax", "listen") | ("<stdlib>/async_net.ax", "listen")
        )
    })
}

fn is_tcp_listener_port_call_name(name: &str, ctx: &LowerContext<'_>) -> bool {
    if name == "net_tcp_listener_port" {
        return true;
    }
    ctx.functions.get(name).is_some_and(|signature| {
        matches!(
            (
                signature.source_path.as_str(),
                signature.source_name.as_str()
            ),
            ("<stdlib>/net_tcp.ax", "local_port") | ("<stdlib>/async_net.ax", "local_port")
        )
    })
}

fn is_loopback_listener_port_expr(expr: &Expr, env: &HashMap<String, Binding>) -> bool {
    let Expr::VarRef { name, .. } = expr else {
        return false;
    };
    env.get(name)
        .and_then(|binding| binding.net_origin.as_ref())
        .is_some_and(|origin| matches!(origin, NetBindingOrigin::LoopbackTcpListenerPort))
}

pub(super) fn validate_process_command_allowlist_hir(
    capabilities: &CapabilityConfig,
    ctx: &LowerContext<'_>,
    command: &Expr,
    line: usize,
    column: usize,
    allow_dynamic_command: bool,
) -> Result<(), Diagnostic> {
    let command_value = hir_known_string(command, ctx);
    let Some(allowed_commands) = capabilities.process_commands.allowed_commands() else {
        if allow_dynamic_command {
            return Ok(());
        }
        return if command_value.is_some() {
            Ok(())
        } else {
            Err(Diagnostic::new(
                "capability",
                "call to \"process_status\" requires a string literal when [capabilities].process is unrestricted",
            )
            .with_span(line, column))
        };
    };
    if allowed_commands.is_empty() {
        return Err(Diagnostic::new(
            "capability",
            "internal error: process command allowlist must not be empty",
        )
        .with_span(line, column));
    }
    match command_value {
        Some(value) if allowed_commands.iter().any(|allowed| allowed == &value) => Ok(()),
        Some(value) => Err(Diagnostic::new(
            "capability",
            format!(
                "call to \"process_status\" requires [capabilities].process to include {value:?}"
            ),
        )
        .with_span(line, column)),
        _ => Err(Diagnostic::new(
            "capability",
            "call to \"process_status\" requires a string literal listed in [capabilities].process",
        )
        .with_span(line, column)),
    }
}

fn hir_known_string(expr: &Expr, ctx: &LowerContext<'_>) -> Option<String> {
    match expr {
        Expr::Literal {
            value: LiteralValue::String(value),
            ..
        } => Some(value.clone()),
        Expr::VarRef { name, ty } if matches!(ty, Type::String | Type::Str) => {
            let const_decl = ctx.consts.get(name)?;
            if !matches!(
                const_decl.ty,
                syntax::TypeName::String | syntax::TypeName::Str
            ) {
                return None;
            }
            match &const_decl.expr {
                syntax::Expr::Literal(syntax::Literal::String(value)) => Some(value.clone()),
                _ => None,
            }
        }
        _ => None,
    }
}

pub(super) fn is_stdlib_process_wrapper(ctx: &LowerContext<'_>) -> bool {
    ctx.current_path == "<stdlib>/process.ax"
        && ctx.current_function.as_deref() == Some("run_status")
}

#[cfg(test)]
mod tests {
    use super::super::lower_with_capabilities;
    use crate::manifest::{CapabilityConfig, ProcessCommandAllowlist, ProcessCommandPolicy};
    use crate::syntax;
    use std::path::Path;

    #[test]
    fn process_allowlist_empty_shape_fails_closed_for_dynamic_command() {
        let parsed = syntax::parse_program(
            "let command: string = \"/bin/true\"\nprint process_status(command)\n",
            Path::new("main.ax"),
        )
        .expect("parse process status program");
        let capabilities = CapabilityConfig {
            process: true,
            process_commands: ProcessCommandPolicy::Allowlist(
                ProcessCommandAllowlist::new_unchecked_for_test(Vec::new()),
            ),
            ..CapabilityConfig::default()
        };

        let error = lower_with_capabilities(&parsed, &capabilities)
            .expect_err("empty process allowlist shape should fail closed");

        assert_eq!(error.kind, "capability");
        assert!(
            error
                .message
                .contains("internal error: process command allowlist must not be empty"),
            "unexpected diagnostic: {error:?}"
        );
    }

    #[test]
    fn http_get_rejects_dynamic_url_when_net_is_unrestricted() {
        let parsed = syntax::parse_program(
            "let url: string = \"http://127.0.0.1:1/health\"\nprint http_get(url)\n",
            Path::new("main.ax"),
        )
        .expect("parse http get program");
        let capabilities = CapabilityConfig {
            net: true,
            ..CapabilityConfig::default()
        };

        let error = lower_with_capabilities(&parsed, &capabilities)
            .expect_err("dynamic http target should fail closed");

        assert!(
            error
                .message
                .contains("requires a static URL literal when [capabilities].net is unrestricted"),
            "unexpected diagnostic: {error:?}"
        );
    }

    #[test]
    fn net_tcp_dial_rejects_dynamic_port_when_net_is_unrestricted() {
        let parsed = syntax::parse_program(
            "let port: int = 8080\nprint net_tcp_dial(\"example.com\", port, \"ping\", 1000)\n",
            Path::new("main.ax"),
        )
        .expect("parse tcp dial program");
        let capabilities = CapabilityConfig {
            net: true,
            ..CapabilityConfig::default()
        };

        let error = lower_with_capabilities(&parsed, &capabilities)
            .expect_err("dynamic network port should fail closed");

        assert!(
            error.message.contains(
                "requires an integer literal when [capabilities].net ports are unrestricted"
            ),
            "unexpected diagnostic: {error:?}"
        );
    }
}

pub(super) fn require_capability(
    capabilities: &CapabilityConfig,
    kind: CapabilityKind,
    intrinsic_name: &str,
    line: usize,
    column: usize,
) -> Result<(), Diagnostic> {
    if capabilities.enabled(kind) {
        return Ok(());
    }
    let requirement = if kind == CapabilityKind::Env {
        String::from("[capabilities].env = [\"NAME\"] or env_unrestricted = true")
    } else {
        format!("[capabilities].{} = true", kind.name())
    };
    Err(Diagnostic::new(
        "capability",
        format!("call to {intrinsic_name:?} requires {requirement}"),
    )
    .with_span(line, column))
}
