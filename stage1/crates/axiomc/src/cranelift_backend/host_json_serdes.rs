//! JSON / serdes i64-lowering support for the native Cranelift backend.
//!
//! Extracted from `cranelift_backend.rs` under issue #1254.  All functions are
//! `pub(crate)` because they are called back into from sibling modules (evaluator,
//! host_* peers) through the parent's `use super::*` re-export.

use super::*;
pub(crate) fn i64_json_safe_string_len_key(name: &str) -> String {
    format!("{name}$json_safe_len")
}

pub(crate) fn is_i64_std_json_wrapper(function: &Function, source_name: &str) -> bool {
    function.path == "<stdlib>/json.ax" && function.source_name == source_name
}

pub(crate) fn is_i64_json_parse_int_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "json_parse_int" || static_bindings.json_parse_int_wrappers.contains(name)
}

pub(crate) fn is_i64_json_parse_bool_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "json_parse_bool" || static_bindings.json_parse_bool_wrappers.contains(name)
}

pub(crate) fn is_i64_json_parse_string_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "json_parse_string" || static_bindings.json_parse_string_wrappers.contains(name)
}

pub(crate) fn is_i64_json_parse_field_int_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "json_parse_field_int" || static_bindings.json_parse_field_int_wrappers.contains(name)
}

pub(crate) fn is_i64_json_parse_field_bool_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "json_parse_field_bool"
        || static_bindings
            .json_parse_field_bool_wrappers
            .contains(name)
}

pub(crate) fn is_i64_json_parse_field_string_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "json_parse_field_string"
        || static_bindings
            .json_parse_field_string_wrappers
            .contains(name)
}

pub(crate) fn is_i64_json_stringify_int_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "json_stringify_int" || static_bindings.json_stringify_int_wrappers.contains(name)
}

pub(crate) fn is_i64_json_stringify_bool_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "json_stringify_bool" || static_bindings.json_stringify_bool_wrappers.contains(name)
}

pub(crate) fn is_i64_json_stringify_string_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "json_stringify_string"
        || static_bindings
            .json_stringify_string_wrappers
            .contains(name)
}

pub(crate) fn lower_i64_json_stringify_string_line_stmts(
    expr: &Expr,
    stream: OutputStream,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    lower_i64_json_stringify_string_line_stmts_with_written(
        expr,
        stream,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )
    .map(|(stmts, _)| stmts)
}

pub(crate) fn lower_i64_json_stringify_string_line_stmts_with_written(
    expr: &Expr,
    stream: OutputStream,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<(Vec<CraneliftI64Stmt>, CraneliftI64Expr)> {
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if !is_i64_json_stringify_string_name(name, static_bindings) {
        return None;
    }
    let [value] = args.as_slice() else {
        return None;
    };
    let mut stmts = lower_i64_log_json_string_value_stmts(
        value,
        stream,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    stmts.push(CraneliftI64Stmt::WriteLine {
        stream,
        text: String::new(),
    });
    Some((
        stmts,
        CraneliftI64Expr::Binary {
            op: CraneliftI64BinaryOp::Add,
            lhs: Box::new(lower_i64_json_escaped_string_len_expr(
                value,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?),
            rhs: Box::new(CraneliftI64Expr::Literal(1)),
        },
    ))
}

pub(crate) fn lower_i64_json_escaped_string_len_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if let Some(value) = i64_string_text(expr, static_bindings) {
        return Some(CraneliftI64Expr::Literal(
            json_escape_string(&value).len() as i64
        ));
    }
    if let Expr::Call { name, args, .. } = expr {
        if is_i64_json_stringify_string_name(name, static_bindings) {
            let [text] = args.as_slice() else {
                return None;
            };
            return Some(CraneliftI64Expr::Binary {
                op: CraneliftI64BinaryOp::Add,
                lhs: Box::new(lower_i64_json_safe_string_len_expr(
                    text,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )?),
                rhs: Box::new(CraneliftI64Expr::Literal(4)),
            });
        }
    }
    lower_i64_map_key_array_string_index_mapped_i64_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
        |value| json_escape_string(value).len() as i64,
    )
    .or_else(|| {
        Some(CraneliftI64Expr::Binary {
            op: CraneliftI64BinaryOp::Add,
            lhs: Box::new(lower_i64_json_safe_string_len_expr(
                expr,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?),
            rhs: Box::new(CraneliftI64Expr::Literal(2)),
        })
    })
}

pub(crate) fn lower_i64_json_safe_string_len_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    match expr {
        Expr::Call { name, args, .. } if is_i64_json_stringify_int_name(name, static_bindings) => {
            let [value] = args.as_slice() else {
                return None;
            };
            let value = lower_i64_expr(
                value,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            Some(i64_decimal_string_len_expr(value))
        }
        Expr::Call { name, args, .. } if is_i64_json_stringify_bool_name(name, static_bindings) => {
            let [value] = args.as_slice() else {
                return None;
            };
            Some(CraneliftI64Expr::Select {
                cond: Box::new(lower_i64_condition(
                    value,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )?),
                then_result: Box::new(CraneliftI64Expr::Literal(4)),
                else_result: Box::new(CraneliftI64Expr::Literal(5)),
            })
        }
        Expr::VarRef {
            name,
            ty: Type::String | Type::Str,
        } => {
            if let Some(local) = local_indexes.get(i64_json_safe_string_len_key(name).as_str()) {
                return Some(CraneliftI64Expr::Local(*local));
            }
            if let Some(local) = local_indexes.get(i64_printable_i64_string_key(name).as_str()) {
                return Some(i64_decimal_string_len_expr(CraneliftI64Expr::Local(*local)));
            }
            local_conditions
                .get(i64_printable_bool_string_key(name).as_str())
                .cloned()
                .map(|cond| CraneliftI64Expr::Select {
                    cond: Box::new(cond),
                    then_result: Box::new(CraneliftI64Expr::Literal(4)),
                    else_result: Box::new(CraneliftI64Expr::Literal(5)),
                })
        }
        Expr::BinaryAdd {
            op: ArithmeticOp::Add,
            lhs,
            rhs,
            ty: Type::String | Type::Str,
        } => Some(CraneliftI64Expr::Binary {
            op: CraneliftI64BinaryOp::Add,
            lhs: Box::new(lower_i64_json_safe_string_len_expr(
                lhs,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?),
            rhs: Box::new(lower_i64_json_safe_string_len_expr(
                rhs,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?),
        }),
        _ => None,
    }
}
