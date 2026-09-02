use super::*;
pub(crate) fn lower_i64_unicode_scalar_count_intrinsic_expr(
    name: &str,
    args: &[Expr],
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if name != "string_scalar_count" {
        return None;
    }
    let [text] = args else {
        return None;
    };
    if !i64_expr_is_stdin_text_source(text, static_bindings) {
        return None;
    }
    Some(CraneliftI64Expr::StdinScalarCount {
        max_bytes: I64_STDIN_BUFFER_BYTES,
    })
}

pub(crate) fn i64_expr_is_io_read_to_string_call(expr: &Expr, static_bindings: &I64StaticBindings) -> bool {
    matches!(
        expr,
        Expr::Call { name, args, .. }
            if args.is_empty() && is_i64_io_read_to_string_name(name, static_bindings)
    )
}

pub(crate) fn i64_expr_is_stdin_text_source(expr: &Expr, static_bindings: &I64StaticBindings) -> bool {
    match expr {
        Expr::Call { .. } => i64_expr_is_io_read_to_string_call(expr, static_bindings),
        Expr::StringBorrow { expr: inner, .. } => {
            i64_expr_is_stdin_text_source(inner, static_bindings)
        }
        Expr::VarRef { name, .. } => static_bindings.stdin_text_bindings.contains(name),
        _ => false,
    }
}

pub(crate) fn i64_stdin_text_binding_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::VarRef { name, .. } => Some(name),
        Expr::StringBorrow { expr: inner, .. } => i64_stdin_text_binding_name(inner),
        _ => None,
    }
}

#[derive(Default)]
pub(crate) struct I64StdinTextUsage {
    pub(crate) len_uses: usize,
    pub(crate) scalar_uses: usize,
}

pub(crate) fn i64_scan_stdin_text_usage(name: &str, stmts: &[Stmt]) -> I64StdinTextUsage {
    let mut usage = I64StdinTextUsage::default();
    for stmt in stmts {
        i64_scan_stdin_text_usage_stmt(name, stmt, &mut usage, 0);
    }
    usage
}

pub(crate) fn i64_scan_stdin_text_usage_stmt(
    name: &str,
    stmt: &Stmt,
    usage: &mut I64StdinTextUsage,
    depth: usize,
) {
    if depth > 8 {
        return;
    }
    match stmt {
        Stmt::Let { expr, .. }
        | Stmt::Return { expr, .. }
        | Stmt::Print { expr, .. }
        | Stmt::Defer { expr, .. } => i64_scan_stdin_text_usage_expr(name, expr, usage, depth + 1),
        Stmt::Assign { target, expr, .. } => {
            i64_scan_stdin_text_usage_expr(name, target, usage, depth + 1);
            i64_scan_stdin_text_usage_expr(name, expr, usage, depth + 1);
        }
        Stmt::Panic { message, .. } => {
            i64_scan_stdin_text_usage_expr(name, message, usage, depth + 1)
        }
        Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            i64_scan_stdin_text_usage_expr(name, cond, usage, depth + 1);
            for inner in then_block {
                i64_scan_stdin_text_usage_stmt(name, inner, usage, depth + 1);
            }
            if let Some(else_block) = else_block {
                for inner in else_block {
                    i64_scan_stdin_text_usage_stmt(name, inner, usage, depth + 1);
                }
            }
        }
        Stmt::While { cond, body, .. } => {
            i64_scan_stdin_text_usage_expr(name, cond, usage, depth + 1);
            for inner in body {
                i64_scan_stdin_text_usage_stmt(name, inner, usage, depth + 1);
            }
        }
        Stmt::Match { expr, arms, .. } => {
            i64_scan_stdin_text_usage_expr(name, expr, usage, depth + 1);
            for arm in arms {
                for inner in &arm.body {
                    i64_scan_stdin_text_usage_stmt(name, inner, usage, depth + 1);
                }
            }
        }
        Stmt::Break { .. } | Stmt::Continue { .. } => {}
    }
}

pub(crate) fn i64_scan_stdin_text_usage_expr(
    name: &str,
    expr: &Expr,
    usage: &mut I64StdinTextUsage,
    depth: usize,
) {
    if depth > 12 {
        return;
    }
    match expr {
        Expr::Call {
            name: call_name,
            args,
            ..
        } => {
            if let [first, ..] = args.as_slice()
                && i64_stdin_text_binding_name(first) == Some(name)
            {
                match call_name.as_str() {
                    "len" => usage.len_uses += 1,
                    "string_scalar_count" | "string_scalar_at" => usage.scalar_uses += 1,
                    _ => {}
                }
            }
            for arg in args {
                i64_scan_stdin_text_usage_expr(name, arg, usage, depth + 1);
            }
        }
        Expr::BinaryAdd { lhs, rhs, .. }
        | Expr::BinaryCompare { lhs, rhs, .. }
        | Expr::BinaryLogic { lhs, rhs, .. } => {
            i64_scan_stdin_text_usage_expr(name, lhs, usage, depth + 1);
            i64_scan_stdin_text_usage_expr(name, rhs, usage, depth + 1);
        }
        Expr::StringBorrow { expr: inner, .. } | Expr::Cast { expr: inner, .. } => {
            i64_scan_stdin_text_usage_expr(name, inner, usage, depth + 1)
        }
        Expr::Index { base, index, .. } => {
            i64_scan_stdin_text_usage_expr(name, base, usage, depth + 1);
            i64_scan_stdin_text_usage_expr(name, index, usage, depth + 1);
        }
        Expr::FieldAccess { base, .. } | Expr::TupleIndex { base, .. } => {
            i64_scan_stdin_text_usage_expr(name, base, usage, depth + 1)
        }
        Expr::ArrayLiteral { elements, .. } | Expr::TupleLiteral { elements, .. } => {
            for element in elements {
                i64_scan_stdin_text_usage_expr(name, element, usage, depth + 1);
            }
        }
        Expr::MapLiteral { entries, .. } => {
            for entry in entries {
                i64_scan_stdin_text_usage_expr(name, &entry.key, usage, depth + 1);
                i64_scan_stdin_text_usage_expr(name, &entry.value, usage, depth + 1);
            }
        }
        Expr::EnumVariant { payloads, .. } => {
            for payload in payloads {
                i64_scan_stdin_text_usage_expr(name, payload, usage, depth + 1);
            }
        }
        Expr::StructLiteral { fields, .. } => {
            for field in fields {
                i64_scan_stdin_text_usage_expr(name, &field.expr, usage, depth + 1);
            }
        }
        Expr::Match {
            expr: inner, arms, ..
        } => {
            i64_scan_stdin_text_usage_expr(name, inner, usage, depth + 1);
            for arm in arms {
                i64_scan_stdin_text_usage_expr(name, &arm.expr, usage, depth + 1);
            }
        }
        _ => {}
    }
}

