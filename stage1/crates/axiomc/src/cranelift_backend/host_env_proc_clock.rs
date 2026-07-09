//! Environment, process, and clock i64-lowering support for the native Cranelift backend.
//!
//! Extracted from `cranelift_backend.rs` under issue #1254.  All functions are
//! `pub(crate)` because they are called back into from sibling modules (evaluator,
//! host_* peers) through the parent's `use super::*` re-export.

use super::*;

pub(crate) fn lower_i64_env_option_match_stmt(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: HashMap<String, usize>,
    local_conditions: HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    allow_stdio_effects: bool,
) -> Option<CraneliftI64Stmt> {
    let Stmt::Match { expr, arms, .. } = stmt else {
        return None;
    };
    let Type::Option(inner) = expr.ty() else {
        return None;
    };
    if !matches!(inner.as_ref(), Type::String | Type::Str) {
        return None;
    }
    let key = i64_env_get_key(expr, static_bindings)?;
    let env_len = i64_env_len_expr(&key, static_bindings)?;
    let (some_arm, none_arm) = i64_option_stmt_match_arms(arms)?;
    let mut some_static_bindings = static_bindings.clone();
    if !some_arm.ignore_payloads
        && let Some(binding) = some_arm.bindings.first()
        && binding != "_"
    {
        // `Some(value) { print value }`: emit the environment variable's runtime
        // string directly. The i64 value model only carries lengths, so the
        // general payload path below cannot materialize the string.
        if allow_stdio_effects && i64_env_option_prints_binding_verbatim(&some_arm.body, binding) {
            return Some(CraneliftI64Stmt::If {
                cond: CraneliftI64Condition::Compare(CraneliftI64Compare {
                    op: CraneliftI64CompareOp::Ge,
                    lhs: env_len,
                    rhs: CraneliftI64Expr::Literal(0),
                }),
                then_body: vec![CraneliftI64Stmt::WriteEnvValue {
                    stream: OutputStream::Stdout,
                    key: key.clone(),
                    append_newline: true,
                }],
                else_body: lower_i64_runtime_stmts(
                    &none_arm.body,
                    locals,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                    allow_stdio_effects,
                )?,
            });
        }
        if !i64_env_option_payload_uses_len_only(&some_arm.body, binding) {
            return None;
        }
        some_static_bindings
            .values
            .insert(binding.clone(), env_len.clone());
    }
    Some(CraneliftI64Stmt::If {
        cond: CraneliftI64Condition::Compare(CraneliftI64Compare {
            op: CraneliftI64CompareOp::Ge,
            lhs: env_len,
            rhs: CraneliftI64Expr::Literal(0),
        }),
        then_body: lower_i64_runtime_stmts(
            &some_arm.body,
            locals,
            local_indexes.clone(),
            local_conditions.clone(),
            helper_signatures,
            &some_static_bindings,
            allow_stdio_effects,
        )?,
        else_body: lower_i64_runtime_stmts(
            &none_arm.body,
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
            allow_stdio_effects,
        )?,
    })
}

/// True when the `Some` arm body is exactly `print <binding>` -- i.e. it prints
/// the environment value verbatim. This is lowered to a `WriteEnvValue` runtime
/// write instead of the length-only payload path.
pub(crate) fn i64_env_option_prints_binding_verbatim(stmts: &[Stmt], binding: &str) -> bool {
    matches!(
        stmts,
        [Stmt::Print {
            expr: Expr::VarRef { name, .. },
            ..
        }] if name == binding
    )
}

pub(crate) fn i64_env_option_payload_uses_len_only(stmts: &[Stmt], binding: &str) -> bool {
    stmts
        .iter()
        .all(|stmt| i64_env_option_stmt_uses_payload_len_only(stmt, binding))
}

pub(crate) fn i64_env_option_stmt_uses_payload_len_only(stmt: &Stmt, binding: &str) -> bool {
    match stmt {
        Stmt::Let { expr, .. }
        | Stmt::Assign { expr, .. }
        | Stmt::Print { expr, .. }
        | Stmt::Return { expr, .. }
        | Stmt::Defer { expr, .. } => i64_env_option_expr_uses_payload_len_only(expr, binding),
        Stmt::Panic { message, .. } => i64_env_option_expr_uses_payload_len_only(message, binding),
        Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            i64_env_option_expr_uses_payload_len_only(cond, binding)
                && i64_env_option_payload_uses_len_only(then_block, binding)
                && else_block
                    .as_ref()
                    .is_none_or(|block| i64_env_option_payload_uses_len_only(block, binding))
        }
        Stmt::While { cond, body, .. } => {
            i64_env_option_expr_uses_payload_len_only(cond, binding)
                && i64_env_option_payload_uses_len_only(body, binding)
        }
        Stmt::Match { expr, arms, .. } => {
            i64_env_option_expr_uses_payload_len_only(expr, binding)
                && arms
                    .iter()
                    .all(|arm| i64_env_option_payload_uses_len_only(&arm.body, binding))
        }
    }
}

pub(crate) fn i64_env_option_expr_uses_payload_len_only(expr: &Expr, binding: &str) -> bool {
    match expr {
        Expr::VarRef { name, .. } => name != binding,
        Expr::Call { name, args, .. } if name == "len" => {
            if let [Expr::VarRef { name, .. }] = args.as_slice()
                && name == binding
            {
                return true;
            }
            args.iter()
                .all(|arg| i64_env_option_expr_uses_payload_len_only(arg, binding))
        }
        Expr::Call { args, .. } => args
            .iter()
            .all(|arg| i64_env_option_expr_uses_payload_len_only(arg, binding)),
        Expr::BinaryAdd { lhs, rhs, .. }
        | Expr::BinaryCompare { lhs, rhs, .. }
        | Expr::BinaryLogic { lhs, rhs, .. } => {
            i64_env_option_expr_uses_payload_len_only(lhs, binding)
                && i64_env_option_expr_uses_payload_len_only(rhs, binding)
        }
        Expr::Cast { expr, .. }
        | Expr::MutBorrow { expr, .. }
        | Expr::Deref { expr, .. }
        | Expr::Try { expr, .. }
        | Expr::Await { expr, .. }
        | Expr::StringBorrow { expr, .. } => {
            i64_env_option_expr_uses_payload_len_only(expr, binding)
        }
        Expr::StructLiteral { fields, .. } => fields
            .iter()
            .all(|field| i64_env_option_expr_uses_payload_len_only(&field.expr, binding)),
        Expr::FieldAccess { base, .. } => i64_env_option_expr_uses_payload_len_only(base, binding),
        Expr::TupleLiteral { elements, .. } | Expr::ArrayLiteral { elements, .. } => elements
            .iter()
            .all(|element| i64_env_option_expr_uses_payload_len_only(element, binding)),
        Expr::TupleIndex { base, .. } => i64_env_option_expr_uses_payload_len_only(base, binding),
        Expr::MapLiteral { entries, .. } => entries.iter().all(|entry| {
            i64_env_option_expr_uses_payload_len_only(&entry.key, binding)
                && i64_env_option_expr_uses_payload_len_only(&entry.value, binding)
        }),
        Expr::EnumVariant { payloads, .. } => payloads
            .iter()
            .all(|payload| i64_env_option_expr_uses_payload_len_only(payload, binding)),
        Expr::Closure { body, .. } => i64_env_option_expr_uses_payload_len_only(body, binding),
        Expr::Slice {
            base, start, end, ..
        } => {
            i64_env_option_expr_uses_payload_len_only(base, binding)
                && start
                    .as_ref()
                    .is_none_or(|expr| i64_env_option_expr_uses_payload_len_only(expr, binding))
                && end
                    .as_ref()
                    .is_none_or(|expr| i64_env_option_expr_uses_payload_len_only(expr, binding))
        }
        Expr::Index { base, index, .. } => {
            i64_env_option_expr_uses_payload_len_only(base, binding)
                && i64_env_option_expr_uses_payload_len_only(index, binding)
        }
        Expr::Match { expr, arms, .. } => {
            i64_env_option_expr_uses_payload_len_only(expr, binding)
                && arms
                    .iter()
                    .all(|arm| i64_env_option_expr_uses_payload_len_only(&arm.expr, binding))
        }
        Expr::Literal(_) => true,
    }
}

/// Resolve the per-element local slots backing an array, slice, or mutable
/// slice binding so runtime-index writes can target them. Mirrors the element
/// resolution used by the dynamic-index read paths.
pub(crate) fn lower_i64_env_option_match_value_expr(
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
    let key = i64_env_get_key(matched, static_bindings)?;
    let (some_arm, none_arm) = i64_option_match_arms(arms)?;
    let binding = some_arm
        .bindings
        .first()
        .filter(|binding| binding.as_str() != "_");
    let env_len = i64_env_len_expr(&key, static_bindings)?;
    let then_result = lower_i64_env_some_arm_expr(
        &some_arm.expr,
        binding.map(String::as_str),
        &key,
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
            lhs: env_len,
            rhs: CraneliftI64Expr::Literal(0),
        })),
        then_result: Box::new(then_result),
        else_result: Box::new(else_result),
    })
}

pub(crate) fn lower_i64_env_some_arm_expr(
    expr: &Expr,
    binding: Option<&str>,
    key: &str,
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
        return i64_env_len_expr(key, static_bindings);
    }
    lower_i64_return_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )
}

pub(crate) fn i64_env_len_expr(key: &str, static_bindings: &I64StaticBindings) -> Option<CraneliftI64Expr> {
    let result =
        if static_bindings.env_unrestricted || static_bindings.env_allowed_names.contains(key) {
            CraneliftI64Expr::EnvLen {
                key: key.to_string(),
            }
        } else {
            CraneliftI64Expr::Literal(-1)
        };
    i64_audited_env_expr(
        "env_get",
        key.len(),
        result,
        static_bindings,
        CraneliftI64AuditSuccess::NonNegative,
    )
}

pub(crate) fn i64_env_get_key(expr: &Expr, static_bindings: &I64StaticBindings) -> Option<String> {
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if name != "env_get" && !static_bindings.env_get_wrappers.contains(name) {
        return None;
    }
    let [key] = args.as_slice() else {
        return None;
    };
    i64_string_text(key, static_bindings)
}

pub(crate) fn lower_i64_clock_intrinsic_expr(
    name: &str,
    args: &[Expr],
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let milliseconds = match name {
        "clock_now_ms" => {
            let [] = args else {
                return None;
            };
            return Some(CraneliftI64Expr::ClockNowMs);
        }
        name if is_i64_time_now_ms_name(name, static_bindings) => {
            let [] = args else {
                return None;
            };
            return Some(CraneliftI64Expr::ClockNowMs);
        }
        "clock_elapsed_ms" => {
            let [start] = args else {
                return None;
            };
            return Some(CraneliftI64Expr::ClockElapsedMs {
                start: Box::new(lower_i64_expr(
                    start,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )?),
            });
        }
        name if is_i64_time_elapsed_ms_name(name, static_bindings) => {
            let [start] = args else {
                return None;
            };
            return Some(CraneliftI64Expr::ClockElapsedMs {
                start: Box::new(lower_i64_instant_ms_expr(
                    start,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )?),
            });
        }
        "clock_sleep_ms" => {
            let [milliseconds] = args else {
                return None;
            };
            lower_i64_expr(
                milliseconds,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?
        }
        name if is_i64_time_sleep_name(name, static_bindings) => {
            let [duration] = args else {
                return None;
            };
            lower_i64_duration_ms_expr(
                duration,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?
        }
        _ => return None,
    };
    i64_audited_clock_expr(
        "clock_sleep_ms",
        "milliseconds",
        CraneliftI64Expr::SleepMs {
            milliseconds: Box::new(milliseconds),
        },
        static_bindings,
        CraneliftI64AuditSuccess::ExitZero,
    )
}

pub(crate) fn lower_i64_duration_ms_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    match expr {
        Expr::VarRef { name, .. } => local_indexes
            .get(i64_struct_projection_key(name, "ms").as_str())
            .copied()
            .map(CraneliftI64Expr::Local),
        Expr::Call { name, args, .. } if is_i64_time_duration_ms_name(name, static_bindings) => {
            let [milliseconds] = args.as_slice() else {
                return None;
            };
            lower_i64_expr(
                milliseconds,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        Expr::StructLiteral { fields, .. } => fields
            .iter()
            .find(|field| field.name == "ms")
            .and_then(|field| {
                lower_i64_expr(
                    &field.expr,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )
            }),
        _ => None,
    }
}

pub(crate) fn lower_i64_instant_ms_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    match expr {
        Expr::VarRef { name, .. } => local_indexes
            .get(i64_struct_projection_key(name, "ms").as_str())
            .copied()
            .map(CraneliftI64Expr::Local),
        Expr::StructLiteral { fields, .. } => fields
            .iter()
            .find(|field| field.name == "ms")
            .and_then(|field| {
                lower_i64_expr(
                    &field.expr,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )
            }),
        Expr::Call { name, args, .. } if is_i64_time_now_name(name, static_bindings) => {
            if args.is_empty() {
                Some(CraneliftI64Expr::ClockNowMs)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(crate) fn lower_i64_process_intrinsic_expr(
    name: &str,
    args: &[Expr],
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if name != "process_status" && !static_bindings.process_status_wrappers.contains(name) {
        return None;
    }
    let [command] = args else {
        return None;
    };
    let command = i64_string_text(command, static_bindings)?;
    match command.as_str() {
        "/usr/bin/true" | "/usr/bin/false" | "__axiom_stage1_missing_binary__" => {
            i64_audited_process_expr(
                "process_status",
                command.len(),
                CraneliftI64Expr::ProcessStatus { command },
                static_bindings,
                CraneliftI64AuditSuccess::NonNegative,
            )
        }
        _ => None,
    }
}

pub(crate) fn i64_audited_env_expr(
    intrinsic: &str,
    key_len: usize,
    result: CraneliftI64Expr,
    static_bindings: &I64StaticBindings,
    success: CraneliftI64AuditSuccess,
) -> Option<CraneliftI64Expr> {
    let package = static_bindings.package_root.as_deref()?;
    Some(CraneliftI64Expr::AuditEnv {
        intrinsic: intrinsic.to_string(),
        package: package.display().to_string(),
        key_len,
        success,
        result: Box::new(result),
    })
}

pub(crate) fn i64_audited_process_expr(
    intrinsic: &str,
    command_len: usize,
    result: CraneliftI64Expr,
    static_bindings: &I64StaticBindings,
    success: CraneliftI64AuditSuccess,
) -> Option<CraneliftI64Expr> {
    let package = static_bindings.package_root.as_deref()?;
    Some(CraneliftI64Expr::AuditProcess {
        intrinsic: intrinsic.to_string(),
        package: package.display().to_string(),
        command_len,
        success,
        result: Box::new(result),
    })
}

pub(crate) fn i64_audited_clock_expr(
    intrinsic: &str,
    arg_name: &str,
    result: CraneliftI64Expr,
    static_bindings: &I64StaticBindings,
    success: CraneliftI64AuditSuccess,
) -> Option<CraneliftI64Expr> {
    let package = static_bindings.package_root.as_deref()?;
    Some(CraneliftI64Expr::AuditClock {
        intrinsic: intrinsic.to_string(),
        package: package.display().to_string(),
        arg_name: arg_name.to_string(),
        success,
        result: Box::new(result),
    })
}
