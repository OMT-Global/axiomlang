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

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> crate::mir::SourceSpan {
        crate::mir::SourceSpan { line: 1, column: 1 }
    }

    fn stdin_read() -> Expr {
        Expr::Call {
            name: String::from("io_read_to_string"),
            args: Vec::new(),
            ty: Type::String,
        }
    }

    fn text_ref() -> Expr {
        Expr::VarRef {
            name: String::from("text"),
            ty: Type::String,
        }
    }

    fn call(name: &str, args: Vec<Expr>, ty: Type) -> Expr {
        Expr::Call {
            name: String::from(name),
            args,
            ty,
        }
    }

    #[test]
    fn lowers_only_single_stdin_scalar_count_argument() {
        let mut bindings = I64StaticBindings::default();
        bindings.stdin_text_bindings.insert(String::from("text"));

        assert!(i64_expr_is_io_read_to_string_call(&stdin_read(), &bindings));
        assert!(i64_expr_is_stdin_text_source(&text_ref(), &bindings));
        assert!(i64_expr_is_stdin_text_source(
            &Expr::StringBorrow {
                expr: Box::new(text_ref()),
                ty: Type::Str,
            },
            &bindings,
        ));
        assert_eq!(
            lower_i64_unicode_scalar_count_intrinsic_expr(
                "string_scalar_count",
                &[text_ref()],
                &bindings,
            ),
            Some(CraneliftI64Expr::StdinScalarCount {
                max_bytes: I64_STDIN_BUFFER_BYTES,
            })
        );
        assert_eq!(
            lower_i64_unicode_scalar_count_intrinsic_expr(
                "string_scalar_count",
                &[Expr::Literal(LiteralValue::String(String::from("static")))],
                &bindings,
            ),
            None
        );
        assert_eq!(
            lower_i64_unicode_scalar_count_intrinsic_expr(
                "string_scalar_at",
                &[text_ref()],
                &bindings,
            ),
            None
        );
    }

    #[test]
    fn scanner_counts_unicode_and_length_uses_through_nested_control_flow() {
        let scalar_count = call("string_scalar_count", vec![text_ref()], Type::Int);
        let scalar_at = call(
            "string_scalar_at",
            vec![Expr::StringBorrow {
                expr: Box::new(text_ref()),
                ty: Type::Str,
            }, Expr::Literal(LiteralValue::Int(1))],
            Type::Option(Box::new(Type::String)),
        );
        let len = call("len", vec![text_ref()], Type::Int);
        let nested = Expr::BinaryLogic {
            op: LogicOp::And,
            lhs: Box::new(Expr::BinaryCompare {
                op: CompareOp::Gt,
                lhs: Box::new(Expr::BinaryAdd {
                    op: ArithmeticOp::Add,
                    lhs: Box::new(scalar_count.clone()),
                    rhs: Box::new(Expr::Literal(LiteralValue::Int(0))),
                    ty: Type::Int,
                }),
                rhs: Box::new(Expr::Literal(LiteralValue::Int(0))),
                ty: Type::Bool,
            }),
            rhs: Box::new(Expr::Literal(LiteralValue::Bool(true))),
            ty: Type::Bool,
        };
        let stmts = vec![
            Stmt::Let {
                name: String::from("value"),
                ty: Type::Int,
                expr: scalar_count.clone(),
                span: span(),
            },
            Stmt::Assign {
                target: Expr::Index {
                    base: Box::new(Expr::ArrayLiteral {
                        elements: vec![Expr::Literal(LiteralValue::Int(0))],
                        ty: Type::Array(Box::new(Type::Int), Some(1)),
                    }),
                    index: Box::new(Expr::Literal(LiteralValue::Int(0))),
                    ty: Type::Int,
                },
                expr: nested,
                span: span(),
            },
            Stmt::Defer {
                expr: Expr::Cast {
                    expr: Box::new(len.clone()),
                    ty: Type::Int,
                },
                span: span(),
            },
            Stmt::If {
                cond: Expr::Literal(LiteralValue::Bool(true)),
                then_block: vec![Stmt::Print {
                    expr: scalar_at,
                    span: span(),
                }],
                else_block: Some(vec![Stmt::Panic {
                    message: len.clone(),
                    span: span(),
                }]),
                span: span(),
            },
            Stmt::While {
                cond: Expr::Literal(LiteralValue::Bool(false)),
                body: vec![Stmt::Match {
                    expr: Expr::TupleIndex {
                        base: Box::new(Expr::TupleLiteral {
                            elements: vec![text_ref()],
                            ty: Type::Tuple(vec![Type::String]),
                        }),
                        index: 0,
                        ty: Type::String,
                    },
                    arms: vec![crate::mir::MatchArm {
                        enum_name: String::from("Option"),
                        variant: String::from("Some"),
                        bindings: Vec::new(),
                        is_named: false,
                        ignore_payloads: true,
                        body: vec![Stmt::Return {
                            expr: Expr::Match {
                                expr: Box::new(Expr::EnumVariant {
                                    enum_name: String::from("Option"),
                                    variant: String::from("Some"),
                                    field_names: Vec::new(),
                                    payloads: vec![text_ref()],
                                    ty: Type::Option(Box::new(Type::String)),
                                }),
                                arms: vec![crate::mir::MatchExprArm {
                                    enum_name: String::from("Option"),
                                    variant: String::from("Some"),
                                    bindings: Vec::new(),
                                    is_named: false,
                                    expr: len,
                                }],
                                ty: Type::Int,
                            },
                            span: span(),
                        }],
                    }],
                    span: span(),
                }],
                span: span(),
            },
            Stmt::Break { span: span() },
            Stmt::Continue { span: span() },
        ];

        let usage = i64_scan_stdin_text_usage("text", &stmts);
        assert_eq!(usage.scalar_uses, 3);
        assert_eq!(usage.len_uses, 3);
    }
}

