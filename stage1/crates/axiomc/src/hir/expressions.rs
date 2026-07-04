use std::collections::HashMap;

use crate::diagnostics::Diagnostic;
use crate::syntax;

use super::model::{CompareOp, Expr, LiteralValue, LogicOp, Type};
use super::types::lower_arithmetic_op;
use super::{Binding, LowerContext, lower_expr};

pub(super) fn is_castable_numeric(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Numeric(_))
}

pub(super) fn is_ordered_numeric(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Numeric(_))
}

fn is_addable_numeric(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Numeric(_))
}

pub(super) fn numeric_method_return_ty(receiver: &Type, method: &str) -> Option<Type> {
    let is_integer = match receiver {
        Type::Int => true,
        Type::Numeric(numeric) => {
            !matches!(numeric, syntax::NumericType::F32 | syntax::NumericType::F64)
        }
        _ => false,
    };
    if !is_integer {
        return None;
    }
    match method {
        "wrapping_add" | "wrapping_sub" | "wrapping_mul" | "wrapping_div" | "wrapping_rem" => {
            Some(receiver.clone())
        }
        "checked_add" | "checked_sub" | "checked_mul" | "checked_div" | "checked_rem" => {
            Some(Type::Option(Box::new(receiver.clone())))
        }
        "saturating_add" | "saturating_sub" | "saturating_mul" => Some(receiver.clone()),
        _ => None,
    }
}

pub(super) fn method_owner_name(ty: &Type) -> Option<&str> {
    match ty {
        Type::Struct(name) | Type::Enum(name) => Some(name.as_str()),
        _ => None,
    }
}

pub(super) fn is_string_like_type(ty: &Type) -> bool {
    matches!(ty, Type::String | Type::Str)
}

fn coerce_expr_to_expected(
    expr: Expr,
    expected: Option<&Type>,
    allow_temporary_string_borrow: bool,
) -> Result<Expr, Diagnostic> {
    match expected {
        Some(Type::Str) if expr.ty() == &Type::String => {
            if !allow_temporary_string_borrow && !is_stable_string_borrow_owner(&expr) {
                return Err(Diagnostic::new(
                    "ownership",
                    "cannot borrow a temporary String as &str; bind the String to a local first",
                ));
            }
            Ok(Expr::StringBorrow {
                expr: Box::new(expr),
                ty: Type::Str,
            })
        }
        _ => Ok(expr),
    }
}

fn is_stable_string_borrow_owner(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::VarRef { .. } | Expr::FieldAccess { .. } | Expr::TupleIndex { .. }
    )
}

pub(super) fn coerce_lowered_expr_to_expected(
    lowered: Expr,
    expected: Option<&Type>,
) -> Result<Expr, Diagnostic> {
    coerce_expr_to_expected(lowered, expected, false)
}

pub(super) fn lower_call_arg_with_expected(
    expr: &syntax::Expr,
    expected: Option<&Type>,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
    allow_temporary_string_borrow: bool,
) -> Result<Expr, Diagnostic> {
    super::lower_expr_with_expected_inner(expr, expected, env, ctx).and_then(|lowered| {
        coerce_expr_to_expected(lowered, expected, allow_temporary_string_borrow)
    })
}

pub(super) fn lower_binary_add_chain(
    expr: &syntax::Expr,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Expr, Diagnostic> {
    let mut current = expr;
    let mut pending = Vec::new();
    while let syntax::Expr::BinaryAdd {
        op,
        lhs,
        rhs,
        line,
        column,
    } = current
    {
        pending.push((*op, rhs.as_ref(), *line, *column));
        current = lhs.as_ref();
    }

    let mut lowered = lower_expr(current, env, ctx)?;
    for (op, rhs, line, column) in pending.into_iter().rev() {
        let lowered_rhs = lower_expr(rhs, env, ctx)?;
        let lhs_ty = lowered.ty().clone();
        let rhs_ty = lowered_rhs.ty().clone();
        let result_ty = if lhs_ty == rhs_ty && is_addable_numeric(&lhs_ty) {
            lhs_ty.clone()
        } else if op == syntax::ArithmeticOp::Add
            && is_string_like_type(&lhs_ty)
            && is_string_like_type(&rhs_ty)
        {
            Type::String
        } else {
            return Err(
                Diagnostic::new(
                    "type",
                    format!(
                        "operator '{}' expects matching numeric or string operands, got {lhs_ty} and {rhs_ty}",
                        op.lexeme()
                    ),
                )
                .with_span(line, column),
            );
        };
        lowered = Expr::BinaryAdd {
            op: lower_arithmetic_op(op),
            lhs: Box::new(lowered),
            rhs: Box::new(lowered_rhs),
            ty: result_ty,
        };
    }
    Ok(lowered)
}

pub(super) fn static_bool_value(expr: &Expr) -> Option<bool> {
    match expr {
        Expr::Literal {
            value: LiteralValue::Bool(value),
            ..
        } => Some(*value),
        Expr::BinaryCompare { op, lhs, rhs, .. } => {
            let lhs = literal_value(lhs)?;
            let rhs = literal_value(rhs)?;
            Some(match (lhs, rhs) {
                (LiteralValue::Int(lhs), LiteralValue::Int(rhs)) => match op {
                    CompareOp::Eq => lhs == rhs,
                    CompareOp::Ne => lhs != rhs,
                    CompareOp::Lt => lhs < rhs,
                    CompareOp::Le => lhs <= rhs,
                    CompareOp::Gt => lhs > rhs,
                    CompareOp::Ge => lhs >= rhs,
                },
                (LiteralValue::Bool(lhs), LiteralValue::Bool(rhs)) => match op {
                    CompareOp::Eq => lhs == rhs,
                    CompareOp::Ne => lhs != rhs,
                    _ => return None,
                },
                (LiteralValue::String(lhs), LiteralValue::String(rhs)) => match op {
                    CompareOp::Eq => lhs == rhs,
                    CompareOp::Ne => lhs != rhs,
                    _ => return None,
                },
                _ => return None,
            })
        }
        Expr::BinaryLogic { op, lhs, rhs, .. } => {
            let lhs = static_bool_value(lhs)?;
            let rhs = static_bool_value(rhs)?;
            Some(match op {
                LogicOp::And => lhs && rhs,
                LogicOp::Or => lhs || rhs,
            })
        }
        _ => None,
    }
}

fn literal_value(expr: &Expr) -> Option<&LiteralValue> {
    match expr {
        Expr::Literal { value, .. } => Some(value),
        _ => None,
    }
}
