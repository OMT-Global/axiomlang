use super::signatures::FunctionSig;
use crate::diagnostics::Diagnostic;
use crate::syntax;
use std::collections::HashMap;

pub(super) fn validate_const_function_body(
    function: &syntax::Function,
    functions: &HashMap<String, FunctionSig>,
) -> Result<(), Diagnostic> {
    if !function.is_const {
        return Ok(());
    }
    if function.is_async {
        return Err(Diagnostic::new(
            "type",
            format!("const fn {:?} cannot be async", function.name),
        )
        .with_span(function.line, function.column));
    }
    if function.is_extern {
        return Err(Diagnostic::new(
            "type",
            format!("const fn {:?} cannot be extern", function.name),
        )
        .with_span(function.line, function.column));
    }
    for stmt in &function.body {
        validate_const_function_stmt(function, stmt, functions)?;
    }
    Ok(())
}

fn validate_const_function_stmt(
    function: &syntax::Function,
    stmt: &syntax::Stmt,
    functions: &HashMap<String, FunctionSig>,
) -> Result<(), Diagnostic> {
    match stmt {
        syntax::Stmt::Let { expr, .. } | syntax::Stmt::Return { expr, .. } => {
            validate_const_function_expr(function, expr, functions)
        }
        syntax::Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            validate_const_function_expr(function, cond, functions)?;
            for stmt in then_block {
                validate_const_function_stmt(function, stmt, functions)?;
            }
            if let Some(else_block) = else_block {
                for stmt in else_block {
                    validate_const_function_stmt(function, stmt, functions)?;
                }
            }
            Ok(())
        }
        _ => Err(Diagnostic::new(
            "type",
            format!(
                "const fn {:?} only supports let, if/else, and return statements in stage1",
                function.name
            ),
        )
        .with_span(stmt.line(), stmt.column())),
    }
}

fn validate_const_function_expr(
    function: &syntax::Function,
    expr: &syntax::Expr,
    functions: &HashMap<String, FunctionSig>,
) -> Result<(), Diagnostic> {
    match expr {
        syntax::Expr::Literal(_) | syntax::Expr::VarRef { .. } => Ok(()),
        syntax::Expr::BinaryAdd { lhs, rhs, .. } | syntax::Expr::BinaryCompare { lhs, rhs, .. } => {
            validate_const_function_expr(function, lhs, functions)?;
            validate_const_function_expr(function, rhs, functions)
        }
        syntax::Expr::Call { name, args, .. } => {
            let Some(signature) = functions.get(name) else {
                return Err(const_function_call_error(function, name, expr));
            };
            if !signature.is_const || signature.is_extern {
                return Err(const_function_call_error(function, name, expr));
            }
            for arg in args {
                validate_const_function_expr(function, arg, functions)?;
            }
            Ok(())
        }
        _ => Err(Diagnostic::new(
            "type",
            format!(
                "const fn {:?} only supports literals, variables, arithmetic/comparison expressions, and calls to other const fn in stage1",
                function.name
            ),
        )
        .with_span(expr.line(), expr.column())),
    }
}

fn const_function_call_error(
    function: &syntax::Function,
    callee: &str,
    expr: &syntax::Expr,
) -> Diagnostic {
    Diagnostic::new(
        "type",
        format!(
            "const fn {:?} can only call other const fn; {callee:?} is a host runtime or non-const call",
            function.name
        ),
    )
    .with_span(expr.line(), expr.column())
}
