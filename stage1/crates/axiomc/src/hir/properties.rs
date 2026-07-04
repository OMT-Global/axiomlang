use crate::diagnostics::Diagnostic;
use crate::syntax;

use super::expressions::static_bool_value;
use super::model::{CompareOp, Expr, LiteralValue, LogicOp, Param, SourceSpan, Stmt, Type};

pub(super) fn validate_property_signature(
    function: &syntax::Function,
    return_ty: &Type,
) -> Result<(), Diagnostic> {
    if function.type_params.is_empty() && function.params.len() == 1 && return_ty == &Type::Bool {
        return Ok(());
    }
    if !function.type_params.is_empty() {
        return Err(Diagnostic::new(
            "property",
            format!(
                "property function {:?} cannot be generic in phase H.1",
                function.source_name
            ),
        )
        .with_path(function.path.clone())
        .with_span(function.line, function.column));
    }
    if function.params.len() != 1 {
        return Err(Diagnostic::new(
            "property",
            format!(
                "property function {:?} must declare exactly one input parameter",
                function.source_name
            ),
        )
        .with_path(function.path.clone())
        .with_span(function.line, function.column));
    }
    Err(Diagnostic::new(
        "property",
        format!(
            "property function {:?} must return bool",
            function.source_name
        ),
    )
    .with_path(function.path.clone())
    .with_span(function.line, function.column))
}

pub(super) fn validate_property_verdict(
    function: &syntax::Function,
    params: &[Param],
    body: &[Stmt],
) -> Result<(), Diagnostic> {
    let Some(input) = params.first() else {
        return Ok(());
    };
    if let Some((span, expr)) = property_failing_return(body) {
        return Err(Diagnostic::new(
            "property",
            format!(
                "property {:?} failed for {}",
                function.source_name,
                property_sample_input(input)
            ),
        )
        .with_code("property_failed")
        .with_path(function.path.clone())
        .with_span(span.line, span.column)
        .with_help(format!(
            "the property return expression is statically false: {}",
            property_expr_summary(expr)
        )));
    }
    Ok(())
}

fn property_failing_return(stmts: &[Stmt]) -> Option<(SourceSpan, &Expr)> {
    for stmt in stmts {
        match stmt {
            Stmt::Return { expr, span, .. } if property_bool_value(expr) == Some(false) => {
                return Some((*span, expr));
            }
            Stmt::If {
                then_block,
                else_block,
                ..
            } => {
                if let Some(failure) = property_failing_return(then_block) {
                    return Some(failure);
                }
                if let Some(else_block) = else_block
                    && let Some(failure) = property_failing_return(else_block)
                {
                    return Some(failure);
                }
            }
            Stmt::Match { arms, .. } => {
                for arm in arms {
                    if let Some(failure) = property_failing_return(&arm.body) {
                        return Some(failure);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn property_bool_value(expr: &Expr) -> Option<bool> {
    match expr {
        Expr::Literal {
            value: LiteralValue::Bool(value),
            ..
        } => Some(*value),
        Expr::BinaryCompare { op, lhs, rhs, .. } if lhs == rhs => Some(match op {
            CompareOp::Eq | CompareOp::Le | CompareOp::Ge => true,
            CompareOp::Ne | CompareOp::Lt | CompareOp::Gt => false,
        }),
        Expr::BinaryCompare { .. } => static_bool_value(expr),
        Expr::BinaryLogic { op, lhs, rhs, .. } => {
            let lhs = property_bool_value(lhs)?;
            let rhs = property_bool_value(rhs)?;
            Some(match op {
                LogicOp::And => lhs && rhs,
                LogicOp::Or => lhs || rhs,
            })
        }
        _ => None,
    }
}

fn property_sample_input(param: &Param) -> String {
    format!("{} = {}", param.name, property_sample_value(&param.ty))
}

fn property_sample_value(ty: &Type) -> String {
    match ty {
        Type::Int | Type::Numeric(_) => String::from("0"),
        Type::Bool => String::from("false"),
        Type::String | Type::Str => String::from("\"\""),
        Type::Array(_, _) | Type::Slice(_) | Type::MutSlice(_) => String::from("[]"),
        Type::Option(_) => String::from("None"),
        Type::Tuple(elements) if elements.is_empty() => String::from("()"),
        Type::Tuple(elements) => format!(
            "({})",
            elements
                .iter()
                .map(property_sample_value)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => format!("<sample {other}>"),
    }
}

fn property_expr_summary(expr: &Expr) -> String {
    match expr {
        Expr::Literal {
            value: LiteralValue::Bool(value),
            ..
        } => value.to_string(),
        Expr::BinaryCompare { op, lhs, rhs, .. } if lhs == rhs => {
            format!("<input> {} <input>", op.lexeme())
        }
        Expr::BinaryLogic { op, .. } => format!("boolean expression using {}", op.lexeme()),
        _ => String::from("boolean expression"),
    }
}
