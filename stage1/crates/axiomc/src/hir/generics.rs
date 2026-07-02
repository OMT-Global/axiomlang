use super::{
    is_async_runtime_type, monomorphized_function_name, monomorphized_type_name,
    preserves_intrinsic_type_args,
};
use crate::diagnostics::Diagnostic;
use crate::syntax;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GenericInstantiation {
    name: String,
    type_args: Vec<syntax::TypeName>,
}

const MAX_GENERIC_INSTANTIATION_EXPANSIONS: usize = 256;

fn infer_generic_call_type_args(
    program: &syntax::Program,
    generic_functions: &HashMap<String, syntax::Function>,
) -> Result<syntax::Program, Diagnostic> {
    let mut inferred = program.clone();
    inferred.functions = program
        .functions
        .iter()
        .map(|function| infer_generic_calls_in_function(function, generic_functions))
        .collect::<Result<Vec<_>, _>>()?;
    let mut env = HashMap::new();
    inferred.stmts =
        infer_generic_calls_in_stmts(&program.stmts, &mut env, None, generic_functions)?;
    Ok(inferred)
}

fn infer_generic_calls_in_function(
    function: &syntax::Function,
    generic_functions: &HashMap<String, syntax::Function>,
) -> Result<syntax::Function, Diagnostic> {
    let mut env = HashMap::new();
    for param in &function.params {
        env.insert(param.name.clone(), param.ty.clone());
    }
    let mut inferred = function.clone();
    inferred.body = infer_generic_calls_in_stmts(
        &function.body,
        &mut env,
        Some(&function.return_ty),
        generic_functions,
    )?;
    Ok(inferred)
}

fn infer_generic_calls_in_stmts(
    stmts: &[syntax::Stmt],
    env: &mut HashMap<String, syntax::TypeName>,
    return_ty: Option<&syntax::TypeName>,
    generic_functions: &HashMap<String, syntax::Function>,
) -> Result<Vec<syntax::Stmt>, Diagnostic> {
    let mut inferred = Vec::new();
    for stmt in stmts {
        inferred.push(infer_generic_calls_in_stmt(
            stmt,
            env,
            return_ty,
            generic_functions,
        )?);
    }
    Ok(inferred)
}

fn infer_generic_calls_in_stmt(
    stmt: &syntax::Stmt,
    env: &mut HashMap<String, syntax::TypeName>,
    return_ty: Option<&syntax::TypeName>,
    generic_functions: &HashMap<String, syntax::Function>,
) -> Result<syntax::Stmt, Diagnostic> {
    Ok(match stmt {
        syntax::Stmt::Let {
            name,
            ty,
            expr,
            line,
            column,
        } => {
            let expr = infer_generic_calls_in_expr(expr, Some(ty), env, generic_functions)?;
            env.insert(name.clone(), ty.clone());
            syntax::Stmt::Let {
                name: name.clone(),
                ty: ty.clone(),
                expr,
                line: *line,
                column: *column,
            }
        }
        syntax::Stmt::Print { expr, line, column } => syntax::Stmt::Print {
            expr: infer_generic_calls_in_expr(expr, None, env, generic_functions)?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Assign {
            target,
            expr,
            line,
            column,
        } => syntax::Stmt::Assign {
            target: infer_generic_calls_in_expr(target, None, env, generic_functions)?,
            expr: infer_generic_calls_in_expr(expr, None, env, generic_functions)?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Panic { expr, line, column } => syntax::Stmt::Panic {
            expr: infer_generic_calls_in_expr(
                expr,
                Some(&syntax::TypeName::String),
                env,
                generic_functions,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Defer { expr, line, column } => syntax::Stmt::Defer {
            expr: infer_generic_calls_in_expr(expr, None, env, generic_functions)?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::If {
            cond,
            then_block,
            else_block,
            line,
            column,
        } => {
            let mut then_env = env.clone();
            let mut else_env = env.clone();
            syntax::Stmt::If {
                cond: infer_generic_calls_in_expr(
                    cond,
                    Some(&syntax::TypeName::Bool),
                    env,
                    generic_functions,
                )?,
                then_block: infer_generic_calls_in_stmts(
                    then_block,
                    &mut then_env,
                    return_ty,
                    generic_functions,
                )?,
                else_block: else_block
                    .as_ref()
                    .map(|block| {
                        infer_generic_calls_in_stmts(
                            block,
                            &mut else_env,
                            return_ty,
                            generic_functions,
                        )
                    })
                    .transpose()?,
                line: *line,
                column: *column,
            }
        }
        syntax::Stmt::While {
            cond,
            body,
            line,
            column,
        } => {
            let mut body_env = env.clone();
            syntax::Stmt::While {
                cond: infer_generic_calls_in_expr(
                    cond,
                    Some(&syntax::TypeName::Bool),
                    env,
                    generic_functions,
                )?,
                body: infer_generic_calls_in_stmts(
                    body,
                    &mut body_env,
                    return_ty,
                    generic_functions,
                )?,
                line: *line,
                column: *column,
            }
        }
        syntax::Stmt::IfLet {
            variant,
            bindings,
            is_named,
            expr,
            then_block,
            else_block,
            line,
            column,
        } => {
            let mut then_env = env.clone();
            let mut else_env = env.clone();
            syntax::Stmt::IfLet {
                variant: variant.clone(),
                bindings: bindings.clone(),
                is_named: *is_named,
                expr: infer_generic_calls_in_expr(expr, None, env, generic_functions)?,
                then_block: infer_generic_calls_in_stmts(
                    then_block,
                    &mut then_env,
                    return_ty,
                    generic_functions,
                )?,
                else_block: else_block
                    .as_ref()
                    .map(|block| {
                        infer_generic_calls_in_stmts(
                            block,
                            &mut else_env,
                            return_ty,
                            generic_functions,
                        )
                    })
                    .transpose()?,
                line: *line,
                column: *column,
            }
        }
        syntax::Stmt::Match {
            expr,
            arms,
            line,
            column,
        } => syntax::Stmt::Match {
            expr: infer_generic_calls_in_expr(expr, None, env, generic_functions)?,
            arms: arms
                .iter()
                .map(|arm| {
                    let mut arm_env = env.clone();
                    Ok(syntax::MatchArm {
                        variant: arm.variant.clone(),
                        bindings: arm.bindings.clone(),
                        is_named: arm.is_named,
                        body: infer_generic_calls_in_stmts(
                            &arm.body,
                            &mut arm_env,
                            return_ty,
                            generic_functions,
                        )?,
                        line: arm.line,
                        column: arm.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Return { expr, line, column } => syntax::Stmt::Return {
            expr: infer_generic_calls_in_expr(expr, return_ty, env, generic_functions)?,
            line: *line,
            column: *column,
        },
    })
}

fn infer_generic_calls_in_expr(
    expr: &syntax::Expr,
    expected: Option<&syntax::TypeName>,
    env: &HashMap<String, syntax::TypeName>,
    generic_functions: &HashMap<String, syntax::Function>,
) -> Result<syntax::Expr, Diagnostic> {
    Ok(match expr {
        syntax::Expr::Call {
            name,
            type_args,
            args,
            line,
            column,
        } => {
            let mut type_args = type_args.clone();
            if type_args.is_empty() {
                if let Some(template) = generic_functions.get(name) {
                    type_args = infer_type_args_for_call(
                        template,
                        args,
                        expected,
                        env,
                        generic_functions,
                        *line,
                        *column,
                    )?;
                }
            }
            let param_expected = generic_functions.get(name).and_then(|template| {
                if type_args.len() == template.type_params.len() {
                    Some(generic_type_bindings(template, &type_args).ok()?)
                } else {
                    None
                }
            });
            let args = args
                .iter()
                .enumerate()
                .map(|(index, arg)| {
                    let expected_arg = generic_functions
                        .get(name)
                        .and_then(|template| template.params.get(index))
                        .and_then(|param| {
                            param_expected
                                .as_ref()
                                .map(|bindings| substitute_type_name(&param.ty, bindings))
                        });
                    infer_generic_calls_in_expr(arg, expected_arg.as_ref(), env, generic_functions)
                })
                .collect::<Result<Vec<_>, _>>()?;
            syntax::Expr::Call {
                name: name.clone(),
                type_args,
                args,
                line: *line,
                column: *column,
            }
        }
        syntax::Expr::MethodCall {
            base,
            method,
            type_args,
            args,
            line,
            column,
        } => syntax::Expr::MethodCall {
            base: Box::new(infer_generic_calls_in_expr(
                base,
                None,
                env,
                generic_functions,
            )?),
            method: method.clone(),
            type_args: type_args.clone(),
            args: args
                .iter()
                .map(|arg| infer_generic_calls_in_expr(arg, None, env, generic_functions))
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::BinaryAdd {
            op,
            lhs,
            rhs,
            line,
            column,
        } => syntax::Expr::BinaryAdd {
            op: *op,
            lhs: Box::new(infer_generic_calls_in_expr(
                lhs,
                expected,
                env,
                generic_functions,
            )?),
            rhs: Box::new(infer_generic_calls_in_expr(
                rhs,
                expected,
                env,
                generic_functions,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::BinaryCompare {
            op,
            lhs,
            rhs,
            line,
            column,
        } => syntax::Expr::BinaryCompare {
            op: *op,
            lhs: Box::new(infer_generic_calls_in_expr(
                lhs,
                None,
                env,
                generic_functions,
            )?),
            rhs: Box::new(infer_generic_calls_in_expr(
                rhs,
                None,
                env,
                generic_functions,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::BinaryLogic {
            op,
            lhs,
            rhs,
            line,
            column,
        } => syntax::Expr::BinaryLogic {
            op: *op,
            lhs: Box::new(infer_generic_calls_in_expr(
                lhs,
                Some(&syntax::TypeName::Bool),
                env,
                generic_functions,
            )?),
            rhs: Box::new(infer_generic_calls_in_expr(
                rhs,
                Some(&syntax::TypeName::Bool),
                env,
                generic_functions,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Cast {
            expr,
            ty,
            line,
            column,
        } => syntax::Expr::Cast {
            expr: Box::new(infer_generic_calls_in_expr(
                expr,
                Some(ty),
                env,
                generic_functions,
            )?),
            ty: ty.clone(),
            line: *line,
            column: *column,
        },
        syntax::Expr::Try { expr, line, column } => syntax::Expr::Try {
            expr: Box::new(infer_generic_calls_in_expr(
                expr,
                None,
                env,
                generic_functions,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::MutBorrow { expr, line, column } => syntax::Expr::MutBorrow {
            expr: Box::new(infer_generic_calls_in_expr(
                expr,
                None,
                env,
                generic_functions,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Deref { expr, line, column } => syntax::Expr::Deref {
            expr: Box::new(infer_generic_calls_in_expr(
                expr,
                None,
                env,
                generic_functions,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Await { expr, line, column } => syntax::Expr::Await {
            expr: Box::new(infer_generic_calls_in_expr(
                expr,
                None,
                env,
                generic_functions,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::StructLiteral {
            name,
            type_args,
            fields,
            line,
            column,
        } => syntax::Expr::StructLiteral {
            name: name.clone(),
            type_args: type_args.clone(),
            fields: fields
                .iter()
                .map(|field| {
                    Ok(syntax::StructFieldValue {
                        name: field.name.clone(),
                        expr: infer_generic_calls_in_expr(
                            &field.expr,
                            None,
                            env,
                            generic_functions,
                        )?,
                        line: field.line,
                        column: field.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::FieldAccess {
            base,
            field,
            line,
            column,
        } => syntax::Expr::FieldAccess {
            base: Box::new(infer_generic_calls_in_expr(
                base,
                None,
                env,
                generic_functions,
            )?),
            field: field.clone(),
            line: *line,
            column: *column,
        },
        syntax::Expr::TupleLiteral {
            elements,
            line,
            column,
        } => syntax::Expr::TupleLiteral {
            elements: elements
                .iter()
                .map(|element| infer_generic_calls_in_expr(element, None, env, generic_functions))
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::TupleIndex {
            base,
            index,
            line,
            column,
        } => syntax::Expr::TupleIndex {
            base: Box::new(infer_generic_calls_in_expr(
                base,
                None,
                env,
                generic_functions,
            )?),
            index: *index,
            line: *line,
            column: *column,
        },
        syntax::Expr::MapLiteral {
            entries,
            line,
            column,
        } => syntax::Expr::MapLiteral {
            entries: entries
                .iter()
                .map(|entry| {
                    Ok(syntax::MapEntry {
                        key: infer_generic_calls_in_expr(&entry.key, None, env, generic_functions)?,
                        value: infer_generic_calls_in_expr(
                            &entry.value,
                            None,
                            env,
                            generic_functions,
                        )?,
                        line: entry.line,
                        column: entry.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::ArrayLiteral {
            elements,
            line,
            column,
        } => syntax::Expr::ArrayLiteral {
            elements: elements
                .iter()
                .map(|element| infer_generic_calls_in_expr(element, None, env, generic_functions))
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::Slice {
            base,
            start,
            end,
            line,
            column,
        } => syntax::Expr::Slice {
            base: Box::new(infer_generic_calls_in_expr(
                base,
                None,
                env,
                generic_functions,
            )?),
            start: start
                .as_ref()
                .map(|expr| {
                    infer_generic_calls_in_expr(
                        expr,
                        Some(&syntax::TypeName::Int),
                        env,
                        generic_functions,
                    )
                    .map(Box::new)
                })
                .transpose()?,
            end: end
                .as_ref()
                .map(|expr| {
                    infer_generic_calls_in_expr(
                        expr,
                        Some(&syntax::TypeName::Int),
                        env,
                        generic_functions,
                    )
                    .map(Box::new)
                })
                .transpose()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::Index {
            base,
            index,
            line,
            column,
        } => syntax::Expr::Index {
            base: Box::new(infer_generic_calls_in_expr(
                base,
                None,
                env,
                generic_functions,
            )?),
            index: Box::new(infer_generic_calls_in_expr(
                index,
                Some(&syntax::TypeName::Int),
                env,
                generic_functions,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Closure {
            params,
            body,
            line,
            column,
        } => syntax::Expr::Closure {
            params: params.clone(),
            body: Box::new(infer_generic_calls_in_expr(
                body,
                expected,
                env,
                generic_functions,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Match {
            expr,
            arms,
            line,
            column,
        } => syntax::Expr::Match {
            expr: Box::new(infer_generic_calls_in_expr(
                expr,
                None,
                env,
                generic_functions,
            )?),
            arms: arms
                .iter()
                .map(|arm| {
                    Ok(syntax::MatchExprArm {
                        variant: arm.variant.clone(),
                        bindings: arm.bindings.clone(),
                        is_named: arm.is_named,
                        expr: infer_generic_calls_in_expr(
                            &arm.expr,
                            expected,
                            env,
                            generic_functions,
                        )?,
                        line: arm.line,
                        column: arm.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::Literal(_) | syntax::Expr::VarRef { .. } => expr.clone(),
    })
}

fn infer_type_args_for_call(
    template: &syntax::Function,
    args: &[syntax::Expr],
    expected: Option<&syntax::TypeName>,
    env: &HashMap<String, syntax::TypeName>,
    generic_functions: &HashMap<String, syntax::Function>,
    line: usize,
    column: usize,
) -> Result<Vec<syntax::TypeName>, Diagnostic> {
    let mut bindings = HashMap::new();
    let type_params = template.type_params.iter().cloned().collect::<HashSet<_>>();
    for (index, (param, arg)) in template.params.iter().zip(args).enumerate() {
        if let Some(arg_ty) = infer_expr_type_name(arg, None, env, generic_functions) {
            unify_generic_type_name(
                &param.ty,
                &arg_ty,
                &type_params,
                &mut bindings,
                line,
                column,
            )
            .map_err(|error| {
                Diagnostic::new(
                    "type",
                    format!(
                        "generic function {:?} argument {} constraint failed: {}",
                        template.name,
                        index + 1,
                        error.message
                    ),
                )
                .with_span(line, column)
            })?;
        }
    }
    if let Some(expected) = expected {
        unify_generic_type_name(
            &template.return_ty,
            expected,
            &type_params,
            &mut bindings,
            line,
            column,
        )
        .map_err(|error| {
            Diagnostic::new(
                "type",
                format!(
                    "generic function {:?} return constraint failed: {}",
                    template.name, error.message
                ),
            )
            .with_span(line, column)
        })?;
    }
    template
        .type_params
        .iter()
        .map(|param| {
            bindings.get(param).cloned().ok_or_else(|| {
                Diagnostic::new(
                    "type",
                    format!(
                        "generic function {:?} could not infer type parameter {:?}",
                        template.name, param
                    ),
                )
                .with_span(line, column)
            })
        })
        .collect()
}

fn infer_expr_type_name(
    expr: &syntax::Expr,
    expected: Option<&syntax::TypeName>,
    env: &HashMap<String, syntax::TypeName>,
    generic_functions: &HashMap<String, syntax::Function>,
) -> Option<syntax::TypeName> {
    match expr {
        syntax::Expr::Literal(syntax::Literal::Int(_)) => Some(syntax::TypeName::Int),
        syntax::Expr::Literal(syntax::Literal::Bool(_)) => Some(syntax::TypeName::Bool),
        syntax::Expr::Literal(syntax::Literal::String(_)) => Some(syntax::TypeName::String),
        syntax::Expr::VarRef { name, .. } => {
            if (name == "None" || name == "Some" || name == "Ok" || name == "Err")
                && expected.is_some()
            {
                expected.cloned()
            } else {
                env.get(name).cloned()
            }
        }
        syntax::Expr::Call {
            name,
            type_args,
            args,
            ..
        } => {
            if let Some(template) = generic_functions.get(name) {
                let inferred_args = if type_args.is_empty() {
                    infer_type_args_for_call(template, args, expected, env, generic_functions, 0, 0)
                        .ok()?
                } else {
                    type_args.clone()
                };
                let bindings = generic_type_bindings(template, &inferred_args).ok()?;
                Some(substitute_type_name(&template.return_ty, &bindings))
            } else {
                expected.cloned()
            }
        }
        syntax::Expr::ArrayLiteral { elements, .. } => elements
            .first()
            .and_then(|element| infer_expr_type_name(element, None, env, generic_functions))
            .map(|inner| syntax::TypeName::Array(Box::new(inner), None)),
        syntax::Expr::Slice { base, .. } => {
            match infer_expr_type_name(base, None, env, generic_functions)? {
                syntax::TypeName::Array(inner, _)
                | syntax::TypeName::Slice(inner)
                | syntax::TypeName::MutSlice(inner) => Some(syntax::TypeName::Slice(inner)),
                other => Some(other),
            }
        }
        syntax::Expr::Index { base, .. } => {
            match infer_expr_type_name(base, None, env, generic_functions)? {
                syntax::TypeName::Array(inner, _)
                | syntax::TypeName::Slice(inner)
                | syntax::TypeName::MutSlice(inner) => Some(*inner),
                syntax::TypeName::Map(_, value) => Some(*value),
                _ => None,
            }
        }
        syntax::Expr::TupleLiteral { elements, .. } => elements
            .iter()
            .map(|element| infer_expr_type_name(element, None, env, generic_functions))
            .collect::<Option<Vec<_>>>()
            .map(syntax::TypeName::Tuple),
        syntax::Expr::Try { expr, .. } | syntax::Expr::Await { expr, .. } => {
            infer_expr_type_name(expr, expected, env, generic_functions)
        }
        _ => expected.cloned(),
    }
}

fn generic_constraint_mismatch(
    pattern: &syntax::TypeName,
    actual: &syntax::TypeName,
    line: usize,
    column: usize,
) -> Diagnostic {
    Diagnostic::new(
        "type",
        format!("expected generic constraint {pattern:?}, got {actual:?}"),
    )
    .with_span(line, column)
}

fn contains_generic_type_param(ty: &syntax::TypeName, type_params: &HashSet<String>) -> bool {
    match ty {
        syntax::TypeName::Named(name, args) => {
            (args.is_empty() && type_params.contains(name))
                || args
                    .iter()
                    .any(|arg| contains_generic_type_param(arg, type_params))
        }
        syntax::TypeName::Ptr(inner)
        | syntax::TypeName::MutPtr(inner)
        | syntax::TypeName::MutRef(inner)
        | syntax::TypeName::Slice(inner)
        | syntax::TypeName::MutSlice(inner)
        | syntax::TypeName::LifetimeSlice(_, inner)
        | syntax::TypeName::LifetimeMutSlice(_, inner)
        | syntax::TypeName::Option(inner)
        | syntax::TypeName::Array(inner, _) => contains_generic_type_param(inner, type_params),
        syntax::TypeName::Result(ok, err) | syntax::TypeName::Map(ok, err) => {
            contains_generic_type_param(ok, type_params)
                || contains_generic_type_param(err, type_params)
        }
        syntax::TypeName::Tuple(elements) => elements
            .iter()
            .any(|element| contains_generic_type_param(element, type_params)),
        syntax::TypeName::Fn(params, return_ty) => {
            params
                .iter()
                .any(|param| contains_generic_type_param(param, type_params))
                || contains_generic_type_param(return_ty, type_params)
        }
        syntax::TypeName::Int
        | syntax::TypeName::Numeric(_)
        | syntax::TypeName::Bool
        | syntax::TypeName::String
        | syntax::TypeName::Str => false,
    }
}

fn unify_generic_type_name(
    pattern: &syntax::TypeName,
    actual: &syntax::TypeName,
    type_params: &HashSet<String>,
    bindings: &mut HashMap<String, syntax::TypeName>,
    line: usize,
    column: usize,
) -> Result<(), Diagnostic> {
    match pattern {
        syntax::TypeName::Named(name, args) if args.is_empty() && type_params.contains(name) => {
            if let Some(bound) = bindings.get(name) {
                if bound != actual {
                    return Err(Diagnostic::new(
                        "type",
                        format!("generic type parameter {name:?} inferred as both {bound:?} and {actual:?}"),
                    ).with_span(line, column));
                }
            } else {
                bindings.insert(name.clone(), actual.clone());
            }
            Ok(())
        }
        syntax::TypeName::Named(lhs_name, lhs_args) => match actual {
            syntax::TypeName::Named(rhs_name, rhs_args)
                if lhs_name == rhs_name && lhs_args.len() == rhs_args.len() =>
            {
                for (lhs, rhs) in lhs_args.iter().zip(rhs_args) {
                    unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)?;
                }
                Ok(())
            }
            _ if contains_generic_type_param(pattern, type_params) => {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            }
            _ => Ok(()),
        },
        syntax::TypeName::Ptr(lhs) => {
            if let syntax::TypeName::Ptr(rhs) = actual {
                unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::MutPtr(lhs) => {
            if let syntax::TypeName::MutPtr(rhs) = actual {
                unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::MutRef(lhs) => {
            if let syntax::TypeName::MutRef(rhs) = actual {
                unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::Slice(lhs) => {
            if let syntax::TypeName::Slice(rhs) = actual {
                unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::MutSlice(lhs) => {
            if let syntax::TypeName::MutSlice(rhs) = actual {
                unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::LifetimeSlice(_, lhs) => {
            if let syntax::TypeName::LifetimeSlice(_, rhs) = actual {
                unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::LifetimeMutSlice(_, lhs) => {
            if let syntax::TypeName::LifetimeMutSlice(_, rhs) = actual {
                unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::Option(lhs) => {
            if let syntax::TypeName::Option(rhs) = actual {
                unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::Result(lhs_ok, lhs_err) => {
            if let syntax::TypeName::Result(rhs_ok, rhs_err) = actual {
                unify_generic_type_name(lhs_ok, rhs_ok, type_params, bindings, line, column)?;
                unify_generic_type_name(lhs_err, rhs_err, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::Tuple(lhs) => {
            if let syntax::TypeName::Tuple(rhs) = actual {
                if lhs.len() != rhs.len() && contains_generic_type_param(pattern, type_params) {
                    return Err(generic_constraint_mismatch(pattern, actual, line, column));
                }
                for (lhs, rhs) in lhs.iter().zip(rhs) {
                    unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)?;
                }
                Ok(())
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::Map(lhs_key, lhs_value) => {
            if let syntax::TypeName::Map(rhs_key, rhs_value) = actual {
                unify_generic_type_name(lhs_key, rhs_key, type_params, bindings, line, column)?;
                unify_generic_type_name(lhs_value, rhs_value, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::Array(lhs, _) => {
            if let syntax::TypeName::Array(rhs, _) = actual {
                unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::Fn(lhs_params, lhs_return) => {
            if let syntax::TypeName::Fn(rhs_params, rhs_return) = actual {
                if lhs_params.len() != rhs_params.len()
                    && contains_generic_type_param(pattern, type_params)
                {
                    return Err(generic_constraint_mismatch(pattern, actual, line, column));
                }
                for (lhs, rhs) in lhs_params.iter().zip(rhs_params) {
                    unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)?;
                }
                unify_generic_type_name(lhs_return, rhs_return, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::Int
        | syntax::TypeName::Numeric(_)
        | syntax::TypeName::Bool
        | syntax::TypeName::String
        | syntax::TypeName::Str => Ok(()),
    }
}

fn validate_syntax_trait_bounds(program: &syntax::Program) -> Result<(), Diagnostic> {
    let trait_names = program
        .traits
        .iter()
        .map(|trait_decl| trait_decl.name.clone())
        .collect::<HashSet<_>>();
    for type_alias in &program.type_aliases {
        validate_syntax_type_param_bounds(&type_alias.type_param_bounds, &trait_names)?;
    }
    for struct_decl in &program.structs {
        validate_syntax_type_param_bounds(&struct_decl.type_param_bounds, &trait_names)?;
    }
    for enum_decl in &program.enums {
        validate_syntax_type_param_bounds(&enum_decl.type_param_bounds, &trait_names)?;
    }
    for function in &program.functions {
        validate_syntax_type_param_bounds(&function.type_param_bounds, &trait_names)?;
    }
    Ok(())
}

fn validate_syntax_type_param_bounds(
    bounds: &[syntax::TypeParamBound],
    trait_names: &HashSet<String>,
) -> Result<(), Diagnostic> {
    for bound in bounds {
        for trait_name in &bound.traits {
            if !trait_names.contains(trait_name) {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "unknown trait bound {trait_name:?} on type parameter {:?}",
                        bound.param
                    ),
                )
                .with_span(bound.line, bound.column));
            }
        }
    }
    Ok(())
}

fn collect_syntax_trait_impl_pairs(functions: &[syntax::Function]) -> HashSet<(String, String)> {
    functions
        .iter()
        .filter_map(|function| {
            Some((
                function.impl_trait.as_ref()?.clone(),
                function.impl_target.as_ref()?.clone(),
            ))
        })
        .collect()
}

fn collect_syntax_trait_method_names(
    traits: &[syntax::TraitDecl],
) -> HashMap<String, HashSet<String>> {
    traits
        .iter()
        .map(|trait_decl| {
            (
                trait_decl.name.clone(),
                trait_decl
                    .methods
                    .iter()
                    .map(|method| method.name.clone())
                    .collect(),
            )
        })
        .collect()
}

fn validate_generic_function_trait_method_calls(
    function: &syntax::Function,
    trait_methods: &HashMap<String, HashSet<String>>,
    structs: &HashMap<String, syntax::StructDecl>,
) -> Result<(), Diagnostic> {
    let type_params = function.type_params.iter().cloned().collect::<HashSet<_>>();
    let param_bounds = function
        .type_param_bounds
        .iter()
        .map(|bound| (bound.param.clone(), bound.traits.clone()))
        .collect::<HashMap<_, _>>();
    let mut env = HashMap::new();
    for param in &function.params {
        if contains_generic_type_param(&param.ty, &type_params) {
            env.insert(param.name.clone(), param.ty.clone());
        }
    }
    validate_generic_trait_method_calls_in_stmts(
        &function.body,
        env,
        &type_params,
        &param_bounds,
        trait_methods,
        structs,
    )
}

fn generic_type_param_binding(
    ty: &syntax::TypeName,
    type_params: &HashSet<String>,
) -> Option<String> {
    if let syntax::TypeName::Named(name, args) = ty
        && args.is_empty()
        && type_params.contains(name)
    {
        return Some(name.clone());
    }
    None
}

fn validate_generic_trait_method_calls_in_stmts(
    stmts: &[syntax::Stmt],
    mut env: HashMap<String, syntax::TypeName>,
    type_params: &HashSet<String>,
    param_bounds: &HashMap<String, Vec<String>>,
    trait_methods: &HashMap<String, HashSet<String>>,
    structs: &HashMap<String, syntax::StructDecl>,
) -> Result<(), Diagnostic> {
    for stmt in stmts {
        match stmt {
            syntax::Stmt::Let { name, ty, expr, .. } => {
                validate_generic_trait_method_calls_in_expr(
                    expr,
                    &env,
                    type_params,
                    param_bounds,
                    trait_methods,
                    structs,
                )?;
                if contains_generic_type_param(ty, type_params) {
                    env.insert(name.clone(), ty.clone());
                } else {
                    env.remove(name);
                }
            }
            syntax::Stmt::Assign { expr, .. }
            | syntax::Stmt::Print { expr, .. }
            | syntax::Stmt::Panic { expr, .. }
            | syntax::Stmt::Defer { expr, .. }
            | syntax::Stmt::Return { expr, .. } => {
                validate_generic_trait_method_calls_in_expr(
                    expr,
                    &env,
                    type_params,
                    param_bounds,
                    trait_methods,
                    structs,
                )?;
            }
            syntax::Stmt::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                validate_generic_trait_method_calls_in_expr(
                    cond,
                    &env,
                    type_params,
                    param_bounds,
                    trait_methods,
                    structs,
                )?;
                validate_generic_trait_method_calls_in_stmts(
                    then_block,
                    env.clone(),
                    type_params,
                    param_bounds,
                    trait_methods,
                    structs,
                )?;
                if let Some(else_block) = else_block {
                    validate_generic_trait_method_calls_in_stmts(
                        else_block,
                        env.clone(),
                        type_params,
                        param_bounds,
                        trait_methods,
                        structs,
                    )?;
                }
            }
            syntax::Stmt::IfLet {
                expr,
                then_block,
                else_block,
                ..
            } => {
                validate_generic_trait_method_calls_in_expr(
                    expr,
                    &env,
                    type_params,
                    param_bounds,
                    trait_methods,
                    structs,
                )?;
                validate_generic_trait_method_calls_in_stmts(
                    then_block,
                    env.clone(),
                    type_params,
                    param_bounds,
                    trait_methods,
                    structs,
                )?;
                if let Some(else_block) = else_block {
                    validate_generic_trait_method_calls_in_stmts(
                        else_block,
                        env.clone(),
                        type_params,
                        param_bounds,
                        trait_methods,
                        structs,
                    )?;
                }
            }
            syntax::Stmt::While { cond, body, .. } => {
                validate_generic_trait_method_calls_in_expr(
                    cond,
                    &env,
                    type_params,
                    param_bounds,
                    trait_methods,
                    structs,
                )?;
                validate_generic_trait_method_calls_in_stmts(
                    body,
                    env.clone(),
                    type_params,
                    param_bounds,
                    trait_methods,
                    structs,
                )?;
            }
            syntax::Stmt::Match { expr, arms, .. } => {
                validate_generic_trait_method_calls_in_expr(
                    expr,
                    &env,
                    type_params,
                    param_bounds,
                    trait_methods,
                    structs,
                )?;
                for arm in arms {
                    validate_generic_trait_method_calls_in_stmts(
                        &arm.body,
                        env.clone(),
                        type_params,
                        param_bounds,
                        trait_methods,
                        structs,
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn validate_generic_trait_method_calls_in_expr(
    expr: &syntax::Expr,
    env: &HashMap<String, syntax::TypeName>,
    type_params: &HashSet<String>,
    param_bounds: &HashMap<String, Vec<String>>,
    trait_methods: &HashMap<String, HashSet<String>>,
    structs: &HashMap<String, syntax::StructDecl>,
) -> Result<(), Diagnostic> {
    if let syntax::Expr::MethodCall {
        base,
        method,
        args: _,
        line,
        column,
        ..
    } = expr
        && let Some(type_param) = generic_type_param_for_expr(base, env, type_params, structs)
    {
        let available = param_bounds
            .get(&type_param)
            .into_iter()
            .flatten()
            .any(|trait_name| {
                trait_methods
                    .get(trait_name)
                    .is_some_and(|methods| methods.contains(method))
            });
        if !available {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "method call {method:?} on generic parameter {type_param:?} requires an explicit trait bound"
                ),
            )
            .with_span(*line, *column));
        }
    }
    for child in syntax_expr_children(expr) {
        validate_generic_trait_method_calls_in_expr(
            child,
            env,
            type_params,
            param_bounds,
            trait_methods,
            structs,
        )?;
    }
    Ok(())
}

fn generic_type_param_for_expr(
    expr: &syntax::Expr,
    env: &HashMap<String, syntax::TypeName>,
    type_params: &HashSet<String>,
    structs: &HashMap<String, syntax::StructDecl>,
) -> Option<String> {
    let ty = generic_type_for_expr(expr, env, structs)?;
    generic_type_param_binding(&ty, type_params)
}

fn generic_type_for_expr(
    expr: &syntax::Expr,
    env: &HashMap<String, syntax::TypeName>,
    structs: &HashMap<String, syntax::StructDecl>,
) -> Option<syntax::TypeName> {
    match expr {
        syntax::Expr::VarRef { name, .. } => env.get(name).cloned(),
        syntax::Expr::FieldAccess { base, field, .. } => {
            let base_ty = generic_type_for_expr(base, env, structs)?;
            generic_field_type(&base_ty, field, structs)
        }
        syntax::Expr::TupleIndex { base, index, .. } => {
            let base_ty = generic_type_for_expr(base, env, structs)?;
            match base_ty {
                syntax::TypeName::Tuple(elements) => elements.get(*index).cloned(),
                _ => None,
            }
        }
        syntax::Expr::Deref { expr, .. }
        | syntax::Expr::MutBorrow { expr, .. }
        | syntax::Expr::Try { expr, .. }
        | syntax::Expr::Await { expr, .. }
        | syntax::Expr::Cast { expr, .. } => generic_type_for_expr(expr, env, structs),
        _ => None,
    }
}

fn generic_field_type(
    base_ty: &syntax::TypeName,
    field_name: &str,
    structs: &HashMap<String, syntax::StructDecl>,
) -> Option<syntax::TypeName> {
    let syntax::TypeName::Named(name, args) = base_ty else {
        return None;
    };
    let struct_decl = structs.get(name)?;
    if struct_decl.type_params.len() != args.len() {
        return None;
    }
    let type_bindings = struct_decl
        .type_params
        .iter()
        .cloned()
        .zip(args.iter().cloned())
        .collect::<HashMap<_, _>>();
    let field = struct_decl
        .fields
        .iter()
        .find(|field| field.name == field_name)?;
    Some(substitute_type_name(&field.ty, &type_bindings))
}

fn syntax_expr_children(expr: &syntax::Expr) -> Vec<&syntax::Expr> {
    match expr {
        syntax::Expr::Call { args, .. } => args.iter().collect(),
        syntax::Expr::MethodCall { base, args, .. } => {
            let mut children = vec![base.as_ref()];
            children.extend(args.iter());
            children
        }
        syntax::Expr::BinaryAdd { lhs, rhs, .. }
        | syntax::Expr::BinaryCompare { lhs, rhs, .. }
        | syntax::Expr::BinaryLogic { lhs, rhs, .. } => vec![lhs.as_ref(), rhs.as_ref()],
        syntax::Expr::Cast { expr, .. }
        | syntax::Expr::Try { expr, .. }
        | syntax::Expr::MutBorrow { expr, .. }
        | syntax::Expr::Deref { expr, .. }
        | syntax::Expr::Await { expr, .. }
        | syntax::Expr::FieldAccess { base: expr, .. }
        | syntax::Expr::TupleIndex { base: expr, .. } => vec![expr.as_ref()],
        syntax::Expr::StructLiteral { fields, .. } => {
            fields.iter().map(|field| &field.expr).collect()
        }
        syntax::Expr::TupleLiteral { elements, .. }
        | syntax::Expr::ArrayLiteral { elements, .. } => elements.iter().collect(),
        syntax::Expr::MapLiteral { entries, .. } => entries
            .iter()
            .flat_map(|entry| [&entry.key, &entry.value])
            .collect(),
        syntax::Expr::Closure { body, .. } => vec![body.as_ref()],
        syntax::Expr::Slice {
            base, start, end, ..
        } => {
            let mut children = vec![base.as_ref()];
            if let Some(start) = start {
                children.push(start.as_ref());
            }
            if let Some(end) = end {
                children.push(end.as_ref());
            }
            children
        }
        syntax::Expr::Index { base, index, .. } => vec![base.as_ref(), index.as_ref()],
        syntax::Expr::Match { expr, arms, .. } => {
            let mut children = vec![expr.as_ref()];
            children.extend(arms.iter().map(|arm| &arm.expr));
            children
        }
        syntax::Expr::Literal(_) | syntax::Expr::VarRef { .. } => Vec::new(),
    }
}

fn validate_generic_trait_bounds(
    template: &syntax::Function,
    type_args: &[syntax::TypeName],
    trait_impls: &HashSet<(String, String)>,
) -> Result<(), Diagnostic> {
    validate_type_param_trait_bounds(
        &template.name,
        &template.type_params,
        &template.type_param_bounds,
        type_args,
        trait_impls,
        template.line,
        template.column,
    )
}

fn validate_type_param_trait_bounds(
    owner: &str,
    type_params: &[String],
    type_param_bounds: &[syntax::TypeParamBound],
    type_args: &[syntax::TypeName],
    trait_impls: &HashSet<(String, String)>,
    line: usize,
    column: usize,
) -> Result<(), Diagnostic> {
    if type_param_bounds.is_empty() {
        return Ok(());
    }
    let type_bindings = generic_decl_type_bindings(owner, type_params, type_args, line, column)?;
    for bound in type_param_bounds {
        let Some(type_arg) = type_bindings.get(&bound.param) else {
            continue;
        };
        let Some(type_name) = simple_trait_impl_type_name(type_arg) else {
            return Err(trait_bound_not_satisfied(
                type_arg,
                &bound.traits[0],
                line,
                column,
            ));
        };
        for trait_name in &bound.traits {
            if !trait_impls.contains(&(trait_name.clone(), type_name.clone())) {
                return Err(trait_bound_not_satisfied(
                    type_arg, trait_name, line, column,
                ));
            }
        }
    }
    Ok(())
}

fn simple_trait_impl_type_name(ty: &syntax::TypeName) -> Option<String> {
    match ty {
        syntax::TypeName::Named(name, args) if args.is_empty() => Some(name.clone()),
        _ => None,
    }
}

fn trait_bound_not_satisfied(
    ty: &syntax::TypeName,
    trait_name: &str,
    line: usize,
    column: usize,
) -> Diagnostic {
    Diagnostic::new(
        "type",
        format!("trait bound not satisfied: type {ty:?} does not implement {trait_name:?}"),
    )
    .with_span(line, column)
}

pub(super) fn monomorphize_program(
    program: &syntax::Program,
) -> Result<syntax::Program, Diagnostic> {
    validate_syntax_trait_bounds(program)?;
    let trait_impls = collect_syntax_trait_impl_pairs(&program.functions);
    let trait_methods = collect_syntax_trait_method_names(&program.traits);
    let syntax_structs = program
        .structs
        .iter()
        .map(|struct_decl| (struct_decl.name.clone(), struct_decl.clone()))
        .collect::<HashMap<_, _>>();
    let mut generic_functions = HashMap::new();
    let mut seen_function_names = HashSet::new();

    for function in &program.functions {
        if function.impl_target.is_none() && !seen_function_names.insert(function.name.clone()) {
            return Err(
                Diagnostic::new("type", format!("duplicate function {:?}", function.name))
                    .with_span(function.line, function.column),
            );
        }
        if !function.type_params.is_empty() {
            validate_generic_function(function)?;
            validate_generic_function_trait_method_calls(
                function,
                &trait_methods,
                &syntax_structs,
            )?;
            generic_functions.insert(function.name.clone(), function.clone());
        }
    }

    let program = infer_generic_call_type_args(program, &generic_functions)?;
    let mut generic_functions = HashMap::new();
    let mut concrete_functions = Vec::new();
    for function in &program.functions {
        if function.type_params.is_empty() {
            concrete_functions.push(function.clone());
        } else {
            generic_functions.insert(function.name.clone(), function.clone());
        }
    }

    let mut queue = VecDeque::new();
    let mut queued = HashSet::new();
    let mut lowered_functions = Vec::new();
    for function in &concrete_functions {
        lowered_functions.push(rewrite_function_generic_calls(
            function,
            &HashMap::new(),
            &generic_functions,
            &mut queue,
            &mut queued,
        )?);
    }
    let stmts = program
        .stmts
        .iter()
        .map(|stmt| {
            rewrite_stmt_generic_calls(
                stmt,
                &HashMap::new(),
                &generic_functions,
                &mut queue,
                &mut queued,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut emitted = HashSet::new();
    while let Some(instantiation) = queue.pop_front() {
        if emitted.len() >= MAX_GENERIC_INSTANTIATION_EXPANSIONS {
            return Err(generic_instantiation_limit_diagnostic(&instantiation));
        }
        if !emitted.insert(instantiation.clone()) {
            continue;
        }
        let template = generic_functions
            .get(&instantiation.name)
            .expect("queued generic instantiations must reference templates");
        validate_generic_trait_bounds(template, &instantiation.type_args, &trait_impls)?;
        let type_bindings = generic_type_bindings(template, &instantiation.type_args)?;
        let mut function = template.clone();
        function.name = monomorphized_function_name(&template.name, &instantiation.type_args);
        function.type_params = Vec::new();
        function.type_param_bounds = Vec::new();
        function.params = template
            .params
            .iter()
            .map(|param| syntax::Param {
                name: param.name.clone(),
                ty: substitute_type_name(&param.ty, &type_bindings),
                line: param.line,
                column: param.column,
            })
            .collect();
        function.return_ty = substitute_type_name(&template.return_ty, &type_bindings);
        function.body = template
            .body
            .iter()
            .map(|stmt| {
                rewrite_stmt_generic_calls(
                    stmt,
                    &type_bindings,
                    &generic_functions,
                    &mut queue,
                    &mut queued,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        lowered_functions.push(function);
    }

    monomorphize_aggregates(
        syntax::Program {
            path: program.path.clone(),
            imports: program.imports.clone(),
            macros: program.macros.clone(),
            macro_expansions: program.macro_expansions.clone(),
            axioms: program.axioms.clone(),
            semantic_capabilities: program.semantic_capabilities.clone(),
            evidence: program.evidence.clone(),
            consts: program.consts.clone(),
            type_aliases: program.type_aliases.clone(),
            structs: program.structs.clone(),
            enums: program.enums.clone(),
            traits: program.traits.clone(),
            functions: lowered_functions,
            stmts,
        },
        &trait_impls,
    )
}

fn monomorphize_aggregates(
    program: syntax::Program,
    trait_impls: &HashSet<(String, String)>,
) -> Result<syntax::Program, Diagnostic> {
    let mut generic_structs = HashMap::new();
    let mut concrete_structs = Vec::new();
    let mut seen_struct_names = HashSet::new();
    for struct_decl in &program.structs {
        if !seen_struct_names.insert(struct_decl.name.clone()) {
            return Err(Diagnostic::new(
                "type",
                format!("duplicate struct {:?}", struct_decl.name),
            )
            .with_span(struct_decl.line, struct_decl.column));
        }
        if struct_decl.type_params.is_empty() {
            concrete_structs.push(struct_decl.clone());
        } else {
            validate_generic_struct(struct_decl)?;
            generic_structs.insert(struct_decl.name.clone(), struct_decl.clone());
        }
    }

    let mut generic_enums = HashMap::new();
    let mut concrete_enums = Vec::new();
    let mut seen_enum_names = HashSet::new();
    for enum_decl in &program.enums {
        if !seen_enum_names.insert(enum_decl.name.clone()) {
            return Err(
                Diagnostic::new("type", format!("duplicate enum {:?}", enum_decl.name))
                    .with_span(enum_decl.line, enum_decl.column),
            );
        }
        if enum_decl.type_params.is_empty() {
            concrete_enums.push(enum_decl.clone());
        } else {
            validate_generic_enum(enum_decl)?;
            generic_enums.insert(enum_decl.name.clone(), enum_decl.clone());
        }
    }

    let mut queue = VecDeque::new();
    let mut queued = HashSet::new();
    let mut type_aliases = Vec::new();
    for alias in &program.type_aliases {
        type_aliases.push(syntax::TypeAliasDecl {
            name: alias.name.clone(),
            type_params: alias.type_params.clone(),
            type_param_bounds: alias.type_param_bounds.clone(),
            ty: rewrite_aggregate_type_name(
                &alias.ty,
                &generic_structs,
                &generic_enums,
                &mut queue,
                &mut queued,
                alias.line,
                alias.column,
            )?,
            visibility: alias.visibility,
            line: alias.line,
            column: alias.column,
        });
    }
    let consts = program
        .consts
        .iter()
        .map(|constant| {
            Ok(syntax::ConstDecl {
                name: constant.name.clone(),
                ty: rewrite_aggregate_type_name(
                    &constant.ty,
                    &generic_structs,
                    &generic_enums,
                    &mut queue,
                    &mut queued,
                    constant.line,
                    constant.column,
                )?,
                expr: rewrite_expr_aggregate_types(
                    &constant.expr,
                    &generic_structs,
                    &generic_enums,
                    &mut queue,
                    &mut queued,
                )?,
                is_static: constant.is_static,
                visibility: constant.visibility,
                line: constant.line,
                column: constant.column,
            })
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    let structs = concrete_structs
        .iter()
        .map(|struct_decl| {
            rewrite_struct_decl_aggregate_types(
                struct_decl,
                &HashMap::new(),
                &generic_structs,
                &generic_enums,
                &mut queue,
                &mut queued,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let enums = concrete_enums
        .iter()
        .map(|enum_decl| {
            rewrite_enum_decl_aggregate_types(
                enum_decl,
                &HashMap::new(),
                &generic_structs,
                &generic_enums,
                &mut queue,
                &mut queued,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let traits = program.traits.clone();
    let functions = program
        .functions
        .iter()
        .map(|function| {
            rewrite_function_aggregate_types(
                function,
                &generic_structs,
                &generic_enums,
                &mut queue,
                &mut queued,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let stmts = program
        .stmts
        .iter()
        .map(|stmt| {
            rewrite_stmt_aggregate_types(
                stmt,
                &generic_structs,
                &generic_enums,
                &mut queue,
                &mut queued,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut structs = structs;
    let mut enums = enums;
    let mut emitted = HashSet::new();
    while let Some(instantiation) = queue.pop_front() {
        if emitted.len() >= MAX_GENERIC_INSTANTIATION_EXPANSIONS {
            return Err(generic_instantiation_limit_diagnostic(&instantiation));
        }
        if !emitted.insert(instantiation.clone()) {
            continue;
        }
        if let Some(template) = generic_structs.get(&instantiation.name) {
            validate_type_param_trait_bounds(
                &template.name,
                &template.type_params,
                &template.type_param_bounds,
                &instantiation.type_args,
                trait_impls,
                template.line,
                template.column,
            )?;
            let type_bindings = generic_decl_type_bindings(
                &template.name,
                &template.type_params,
                &instantiation.type_args,
                template.line,
                template.column,
            )?;
            let mut lowered = rewrite_struct_decl_aggregate_types(
                template,
                &type_bindings,
                &generic_structs,
                &generic_enums,
                &mut queue,
                &mut queued,
            )?;
            lowered.name = monomorphized_type_name(&template.name, &instantiation.type_args);
            lowered.type_params = Vec::new();
            lowered.type_param_bounds = Vec::new();
            structs.push(lowered);
            continue;
        }
        if let Some(template) = generic_enums.get(&instantiation.name) {
            validate_type_param_trait_bounds(
                &template.name,
                &template.type_params,
                &template.type_param_bounds,
                &instantiation.type_args,
                trait_impls,
                template.line,
                template.column,
            )?;
            let type_bindings = generic_decl_type_bindings(
                &template.name,
                &template.type_params,
                &instantiation.type_args,
                template.line,
                template.column,
            )?;
            let mut lowered = rewrite_enum_decl_aggregate_types(
                template,
                &type_bindings,
                &generic_structs,
                &generic_enums,
                &mut queue,
                &mut queued,
            )?;
            lowered.name = monomorphized_type_name(&template.name, &instantiation.type_args);
            lowered.type_params = Vec::new();
            lowered.type_param_bounds = Vec::new();
            enums.push(lowered);
        }
    }

    Ok(syntax::Program {
        path: program.path,
        imports: program.imports,
        macros: program.macros,
        macro_expansions: program.macro_expansions,
        axioms: program.axioms,
        semantic_capabilities: program.semantic_capabilities,
        evidence: program.evidence,
        consts,
        type_aliases,
        structs,
        enums,
        traits,
        functions,
        stmts,
    })
}

fn generic_instantiation_limit_diagnostic(instantiation: &GenericInstantiation) -> Diagnostic {
    Diagnostic::new(
        "type",
        format!(
            "generic instantiation resource limit exceeded while expanding {:?}; generic expansion is bounded to prevent runaway recursive instantiations",
            instantiation.name
        ),
    )
    .with_code("generic_instantiation_limit")
}

fn validate_generic_function(function: &syntax::Function) -> Result<(), Diagnostic> {
    let mut constrained = HashSet::new();
    for param in &function.params {
        collect_type_params(&param.ty, &function.type_params, &mut constrained);
    }
    collect_type_params(&function.return_ty, &function.type_params, &mut constrained);
    for type_param in &function.type_params {
        if !constrained.contains(type_param) {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "generic function {:?} has unconstrained type parameter {:?}",
                    function.name, type_param
                ),
            )
            .with_span(function.line, function.column));
        }
    }
    Ok(())
}

fn validate_generic_struct(struct_decl: &syntax::StructDecl) -> Result<(), Diagnostic> {
    let mut constrained = HashSet::new();
    for field in &struct_decl.fields {
        collect_type_params(&field.ty, &struct_decl.type_params, &mut constrained);
    }
    validate_all_type_params_constrained(
        "struct",
        &struct_decl.name,
        &struct_decl.type_params,
        &constrained,
        struct_decl.line,
        struct_decl.column,
    )
}

fn validate_generic_enum(enum_decl: &syntax::EnumDecl) -> Result<(), Diagnostic> {
    let mut constrained = HashSet::new();
    for variant in &enum_decl.variants {
        for ty in &variant.payload_tys {
            collect_type_params(ty, &enum_decl.type_params, &mut constrained);
        }
    }
    validate_all_type_params_constrained(
        "enum",
        &enum_decl.name,
        &enum_decl.type_params,
        &constrained,
        enum_decl.line,
        enum_decl.column,
    )
}

fn validate_all_type_params_constrained(
    kind: &str,
    name: &str,
    type_params: &[String],
    constrained: &HashSet<String>,
    line: usize,
    column: usize,
) -> Result<(), Diagnostic> {
    for type_param in type_params {
        if !constrained.contains(type_param) {
            return Err(Diagnostic::new(
                "type",
                format!("generic {kind} {name:?} has unconstrained type parameter {type_param:?}"),
            )
            .with_span(line, column));
        }
    }
    Ok(())
}

fn collect_type_params(ty: &syntax::TypeName, type_params: &[String], found: &mut HashSet<String>) {
    match ty {
        syntax::TypeName::Named(name, args)
            if args.is_empty() && type_params.iter().any(|param| param == name) =>
        {
            found.insert(name.clone());
        }
        syntax::TypeName::Named(_, args) => {
            for arg in args {
                collect_type_params(arg, type_params, found);
            }
        }
        syntax::TypeName::Ptr(inner)
        | syntax::TypeName::MutPtr(inner)
        | syntax::TypeName::MutRef(inner)
        | syntax::TypeName::Slice(inner)
        | syntax::TypeName::MutSlice(inner)
        | syntax::TypeName::LifetimeSlice(_, inner)
        | syntax::TypeName::LifetimeMutSlice(_, inner)
        | syntax::TypeName::Option(inner)
        | syntax::TypeName::Array(inner, _) => collect_type_params(inner, type_params, found),
        syntax::TypeName::Result(ok, err) | syntax::TypeName::Map(ok, err) => {
            collect_type_params(ok, type_params, found);
            collect_type_params(err, type_params, found);
        }
        syntax::TypeName::Fn(params, return_ty) => {
            for param in params {
                collect_type_params(param, type_params, found);
            }
            collect_type_params(return_ty, type_params, found);
        }
        syntax::TypeName::Tuple(elements) => {
            for element in elements {
                collect_type_params(element, type_params, found);
            }
        }
        syntax::TypeName::Int
        | syntax::TypeName::Numeric(_)
        | syntax::TypeName::Bool
        | syntax::TypeName::String
        | syntax::TypeName::Str => {}
    }
}

fn generic_type_bindings(
    function: &syntax::Function,
    type_args: &[syntax::TypeName],
) -> Result<HashMap<String, syntax::TypeName>, Diagnostic> {
    if type_args.len() != function.type_params.len() {
        return Err(Diagnostic::new(
            "type",
            format!(
                "generic function {:?} expects {} type arguments, got {}",
                function.name,
                function.type_params.len(),
                type_args.len()
            ),
        )
        .with_span(function.line, function.column));
    }
    Ok(function
        .type_params
        .iter()
        .cloned()
        .zip(type_args.iter().cloned())
        .collect())
}

fn generic_decl_type_bindings(
    name: &str,
    type_params: &[String],
    type_args: &[syntax::TypeName],
    line: usize,
    column: usize,
) -> Result<HashMap<String, syntax::TypeName>, Diagnostic> {
    if type_args.len() != type_params.len() {
        return Err(Diagnostic::new(
            "type",
            format!(
                "generic type {:?} expects {} type arguments, got {}",
                name,
                type_params.len(),
                type_args.len()
            ),
        )
        .with_span(line, column));
    }
    Ok(type_params
        .iter()
        .cloned()
        .zip(type_args.iter().cloned())
        .collect())
}

fn rewrite_struct_decl_aggregate_types(
    struct_decl: &syntax::StructDecl,
    type_bindings: &HashMap<String, syntax::TypeName>,
    generic_structs: &HashMap<String, syntax::StructDecl>,
    generic_enums: &HashMap<String, syntax::EnumDecl>,
    queue: &mut VecDeque<GenericInstantiation>,
    queued: &mut HashSet<GenericInstantiation>,
) -> Result<syntax::StructDecl, Diagnostic> {
    Ok(syntax::StructDecl {
        name: struct_decl.name.clone(),
        type_params: struct_decl.type_params.clone(),
        type_param_bounds: struct_decl.type_param_bounds.clone(),
        fields: struct_decl
            .fields
            .iter()
            .map(|field| {
                Ok(syntax::StructField {
                    name: field.name.clone(),
                    ty: rewrite_aggregate_type_name(
                        &substitute_type_name(&field.ty, type_bindings),
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                        field.line,
                        field.column,
                    )?,
                    line: field.line,
                    column: field.column,
                })
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?,
        visibility: struct_decl.visibility,
        line: struct_decl.line,
        column: struct_decl.column,
    })
}

fn rewrite_enum_decl_aggregate_types(
    enum_decl: &syntax::EnumDecl,
    type_bindings: &HashMap<String, syntax::TypeName>,
    generic_structs: &HashMap<String, syntax::StructDecl>,
    generic_enums: &HashMap<String, syntax::EnumDecl>,
    queue: &mut VecDeque<GenericInstantiation>,
    queued: &mut HashSet<GenericInstantiation>,
) -> Result<syntax::EnumDecl, Diagnostic> {
    Ok(syntax::EnumDecl {
        name: enum_decl.name.clone(),
        type_params: enum_decl.type_params.clone(),
        type_param_bounds: enum_decl.type_param_bounds.clone(),
        variants: enum_decl
            .variants
            .iter()
            .map(|variant| {
                Ok(syntax::EnumVariantDecl {
                    name: variant.name.clone(),
                    payload_tys: variant
                        .payload_tys
                        .iter()
                        .map(|ty| {
                            rewrite_aggregate_type_name(
                                &substitute_type_name(ty, type_bindings),
                                generic_structs,
                                generic_enums,
                                queue,
                                queued,
                                variant.line,
                                variant.column,
                            )
                        })
                        .collect::<Result<Vec<_>, Diagnostic>>()?,
                    payload_names: variant.payload_names.clone(),
                    line: variant.line,
                    column: variant.column,
                })
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?,
        visibility: enum_decl.visibility,
        line: enum_decl.line,
        column: enum_decl.column,
    })
}

fn rewrite_function_aggregate_types(
    function: &syntax::Function,
    generic_structs: &HashMap<String, syntax::StructDecl>,
    generic_enums: &HashMap<String, syntax::EnumDecl>,
    queue: &mut VecDeque<GenericInstantiation>,
    queued: &mut HashSet<GenericInstantiation>,
) -> Result<syntax::Function, Diagnostic> {
    Ok(syntax::Function {
        name: function.name.clone(),
        source_name: function.source_name.clone(),
        path: function.path.clone(),
        type_params: function.type_params.clone(),
        type_param_bounds: function.type_param_bounds.clone(),
        params: function
            .params
            .iter()
            .map(|param| {
                Ok(syntax::Param {
                    name: param.name.clone(),
                    ty: rewrite_aggregate_type_name(
                        &param.ty,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                        param.line,
                        param.column,
                    )?,
                    line: param.line,
                    column: param.column,
                })
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?,
        return_ty: rewrite_aggregate_type_name(
            &function.return_ty,
            generic_structs,
            generic_enums,
            queue,
            queued,
            function.line,
            function.column,
        )?,
        body: function
            .body
            .iter()
            .map(|stmt| {
                rewrite_stmt_aggregate_types(stmt, generic_structs, generic_enums, queue, queued)
            })
            .collect::<Result<Vec<_>, _>>()?,
        is_property: function.is_property,
        is_const: function.is_const,
        is_async: function.is_async,
        is_extern: function.is_extern,
        extern_abi: function.extern_abi.clone(),
        extern_library: function.extern_library.clone(),
        visibility: function.visibility,
        receiver: function.receiver,
        impl_target: function.impl_target.clone(),
        impl_trait: function.impl_trait.clone(),
        line: function.line,
        column: function.column,
    })
}

fn rewrite_aggregate_type_name(
    ty: &syntax::TypeName,
    generic_structs: &HashMap<String, syntax::StructDecl>,
    generic_enums: &HashMap<String, syntax::EnumDecl>,
    queue: &mut VecDeque<GenericInstantiation>,
    queued: &mut HashSet<GenericInstantiation>,
    line: usize,
    column: usize,
) -> Result<syntax::TypeName, Diagnostic> {
    Ok(match ty {
        syntax::TypeName::Named(name, args) => {
            let args = args
                .iter()
                .map(|arg| {
                    rewrite_aggregate_type_name(
                        arg,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                        line,
                        column,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            if is_async_runtime_type(name) {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("async runtime type {name:?} expects 1 type argument"),
                    )
                    .with_span(line, column));
                }
                return Ok(syntax::TypeName::Named(name.clone(), args));
            }
            let is_generic = generic_structs.contains_key(name) || generic_enums.contains_key(name);
            if args.is_empty() {
                if is_generic {
                    return Err(Diagnostic::new(
                        "type",
                        format!("generic type {name:?} requires explicit type arguments"),
                    )
                    .with_span(line, column));
                }
                syntax::TypeName::Named(name.clone(), Vec::new())
            } else {
                let type_params = generic_structs
                    .get(name)
                    .map(|decl| decl.type_params.as_slice())
                    .or_else(|| {
                        generic_enums
                            .get(name)
                            .map(|decl| decl.type_params.as_slice())
                    })
                    .ok_or_else(|| {
                        Diagnostic::new("type", format!("type {name:?} is not generic"))
                            .with_span(line, column)
                    })?;
                generic_decl_type_bindings(name, type_params, &args, line, column)?;
                let instantiation = GenericInstantiation {
                    name: name.clone(),
                    type_args: args.clone(),
                };
                if queued.insert(instantiation.clone()) {
                    queue.push_back(instantiation);
                }
                syntax::TypeName::Named(monomorphized_type_name(name, &args), Vec::new())
            }
        }
        syntax::TypeName::Ptr(inner) => {
            syntax::TypeName::Ptr(Box::new(rewrite_aggregate_type_name(
                inner,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?))
        }
        syntax::TypeName::MutPtr(inner) => {
            syntax::TypeName::MutPtr(Box::new(rewrite_aggregate_type_name(
                inner,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?))
        }
        syntax::TypeName::MutRef(inner) => {
            syntax::TypeName::MutRef(Box::new(rewrite_aggregate_type_name(
                inner,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?))
        }
        syntax::TypeName::Slice(inner) => {
            syntax::TypeName::Slice(Box::new(rewrite_aggregate_type_name(
                inner,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?))
        }
        syntax::TypeName::MutSlice(inner) => {
            syntax::TypeName::MutSlice(Box::new(rewrite_aggregate_type_name(
                inner,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?))
        }
        syntax::TypeName::LifetimeSlice(lifetime, inner) => syntax::TypeName::LifetimeSlice(
            lifetime.clone(),
            Box::new(rewrite_aggregate_type_name(
                inner,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?),
        ),
        syntax::TypeName::LifetimeMutSlice(lifetime, inner) => syntax::TypeName::LifetimeMutSlice(
            lifetime.clone(),
            Box::new(rewrite_aggregate_type_name(
                inner,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?),
        ),
        syntax::TypeName::Option(inner) => {
            syntax::TypeName::Option(Box::new(rewrite_aggregate_type_name(
                inner,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?))
        }
        syntax::TypeName::Result(ok, err) => syntax::TypeName::Result(
            Box::new(rewrite_aggregate_type_name(
                ok,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?),
            Box::new(rewrite_aggregate_type_name(
                err,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?),
        ),
        syntax::TypeName::Tuple(elements) => syntax::TypeName::Tuple(
            elements
                .iter()
                .map(|element| {
                    rewrite_aggregate_type_name(
                        element,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                        line,
                        column,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        syntax::TypeName::Map(key, value) => syntax::TypeName::Map(
            Box::new(rewrite_aggregate_type_name(
                key,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?),
            Box::new(rewrite_aggregate_type_name(
                value,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?),
        ),
        syntax::TypeName::Array(inner, len) => syntax::TypeName::Array(
            Box::new(rewrite_aggregate_type_name(
                inner,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?),
            len.clone(),
        ),
        syntax::TypeName::Fn(params, return_ty) => syntax::TypeName::Fn(
            params
                .iter()
                .map(|param| {
                    rewrite_aggregate_type_name(
                        param,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                        line,
                        column,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            Box::new(rewrite_aggregate_type_name(
                return_ty,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?),
        ),
        syntax::TypeName::Int => syntax::TypeName::Int,
        syntax::TypeName::Numeric(numeric) => syntax::TypeName::Numeric(*numeric),
        syntax::TypeName::Bool => syntax::TypeName::Bool,
        syntax::TypeName::String => syntax::TypeName::String,
        syntax::TypeName::Str => syntax::TypeName::Str,
    })
}

fn rewrite_function_generic_calls(
    function: &syntax::Function,
    type_bindings: &HashMap<String, syntax::TypeName>,
    generic_functions: &HashMap<String, syntax::Function>,
    queue: &mut VecDeque<GenericInstantiation>,
    queued: &mut HashSet<GenericInstantiation>,
) -> Result<syntax::Function, Diagnostic> {
    let mut rewritten = function.clone();
    rewritten.body = function
        .body
        .iter()
        .map(|stmt| {
            rewrite_stmt_generic_calls(stmt, type_bindings, generic_functions, queue, queued)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rewritten)
}

fn rewrite_stmt_aggregate_types(
    stmt: &syntax::Stmt,
    generic_structs: &HashMap<String, syntax::StructDecl>,
    generic_enums: &HashMap<String, syntax::EnumDecl>,
    queue: &mut VecDeque<GenericInstantiation>,
    queued: &mut HashSet<GenericInstantiation>,
) -> Result<syntax::Stmt, Diagnostic> {
    Ok(match stmt {
        syntax::Stmt::Let {
            name,
            ty,
            expr,
            line,
            column,
        } => syntax::Stmt::Let {
            name: name.clone(),
            ty: rewrite_aggregate_type_name(
                ty,
                generic_structs,
                generic_enums,
                queue,
                queued,
                *line,
                *column,
            )?,
            expr: rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Print { expr, line, column } => syntax::Stmt::Print {
            expr: rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Assign {
            target,
            expr,
            line,
            column,
        } => syntax::Stmt::Assign {
            target: rewrite_expr_aggregate_types(
                target,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?,
            expr: rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Panic { expr, line, column } => syntax::Stmt::Panic {
            expr: rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Defer { expr, line, column } => syntax::Stmt::Defer {
            expr: rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::If {
            cond,
            then_block,
            else_block,
            line,
            column,
        } => syntax::Stmt::If {
            cond: rewrite_expr_aggregate_types(
                cond,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?,
            then_block: then_block
                .iter()
                .map(|stmt| {
                    rewrite_stmt_aggregate_types(
                        stmt,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            else_block: else_block
                .as_ref()
                .map(|block| {
                    block
                        .iter()
                        .map(|stmt| {
                            rewrite_stmt_aggregate_types(
                                stmt,
                                generic_structs,
                                generic_enums,
                                queue,
                                queued,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::IfLet {
            variant,
            bindings,
            is_named,
            expr,
            then_block,
            else_block,
            line,
            column,
        } => syntax::Stmt::IfLet {
            variant: variant.clone(),
            bindings: bindings.clone(),
            is_named: *is_named,
            expr: rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?,
            then_block: then_block
                .iter()
                .map(|stmt| {
                    rewrite_stmt_aggregate_types(
                        stmt,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            else_block: else_block
                .as_ref()
                .map(|block| {
                    block
                        .iter()
                        .map(|stmt| {
                            rewrite_stmt_aggregate_types(
                                stmt,
                                generic_structs,
                                generic_enums,
                                queue,
                                queued,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::While {
            cond,
            body,
            line,
            column,
        } => syntax::Stmt::While {
            cond: rewrite_expr_aggregate_types(
                cond,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?,
            body: body
                .iter()
                .map(|stmt| {
                    rewrite_stmt_aggregate_types(
                        stmt,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Match {
            expr,
            arms,
            line,
            column,
        } => syntax::Stmt::Match {
            expr: rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?,
            arms: arms
                .iter()
                .map(|arm| {
                    Ok(syntax::MatchArm {
                        variant: arm.variant.clone(),
                        bindings: arm.bindings.clone(),
                        is_named: arm.is_named,
                        body: arm
                            .body
                            .iter()
                            .map(|stmt| {
                                rewrite_stmt_aggregate_types(
                                    stmt,
                                    generic_structs,
                                    generic_enums,
                                    queue,
                                    queued,
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                        line: arm.line,
                        column: arm.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Return { expr, line, column } => syntax::Stmt::Return {
            expr: rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
    })
}

fn rewrite_expr_aggregate_types(
    expr: &syntax::Expr,
    generic_structs: &HashMap<String, syntax::StructDecl>,
    generic_enums: &HashMap<String, syntax::EnumDecl>,
    queue: &mut VecDeque<GenericInstantiation>,
    queued: &mut HashSet<GenericInstantiation>,
) -> Result<syntax::Expr, Diagnostic> {
    Ok(match expr {
        syntax::Expr::Literal(_) | syntax::Expr::VarRef { .. } => expr.clone(),
        syntax::Expr::Call {
            name,
            type_args,
            args,
            line,
            column,
        } => syntax::Expr::Call {
            name: name.clone(),
            type_args: type_args
                .iter()
                .map(|type_arg| {
                    rewrite_aggregate_type_name(
                        type_arg,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                        *line,
                        *column,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            args: args
                .iter()
                .map(|arg| {
                    rewrite_expr_aggregate_types(arg, generic_structs, generic_enums, queue, queued)
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::MethodCall {
            base,
            method,
            type_args,
            args,
            line,
            column,
        } => syntax::Expr::MethodCall {
            base: Box::new(rewrite_expr_aggregate_types(
                base,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            method: method.clone(),
            type_args: type_args
                .iter()
                .map(|type_arg| {
                    rewrite_aggregate_type_name(
                        type_arg,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                        *line,
                        *column,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            args: args
                .iter()
                .map(|arg| {
                    rewrite_expr_aggregate_types(arg, generic_structs, generic_enums, queue, queued)
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::BinaryAdd {
            op,
            lhs,
            rhs,
            line,
            column,
        } => syntax::Expr::BinaryAdd {
            op: *op,
            lhs: Box::new(rewrite_expr_aggregate_types(
                lhs,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            rhs: Box::new(rewrite_expr_aggregate_types(
                rhs,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::BinaryCompare {
            op,
            lhs,
            rhs,
            line,
            column,
        } => syntax::Expr::BinaryCompare {
            op: *op,
            lhs: Box::new(rewrite_expr_aggregate_types(
                lhs,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            rhs: Box::new(rewrite_expr_aggregate_types(
                rhs,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::BinaryLogic {
            op,
            lhs,
            rhs,
            line,
            column,
        } => syntax::Expr::BinaryLogic {
            op: *op,
            lhs: Box::new(rewrite_expr_aggregate_types(
                lhs,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            rhs: Box::new(rewrite_expr_aggregate_types(
                rhs,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Cast {
            expr,
            ty,
            line,
            column,
        } => syntax::Expr::Cast {
            expr: Box::new(rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            ty: rewrite_aggregate_type_name(
                ty,
                generic_structs,
                generic_enums,
                queue,
                queued,
                *line,
                *column,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Expr::Try { expr, line, column } => syntax::Expr::Try {
            expr: Box::new(rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::MutBorrow { expr, line, column } => syntax::Expr::MutBorrow {
            expr: Box::new(rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Deref { expr, line, column } => syntax::Expr::Deref {
            expr: Box::new(rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Await { expr, line, column } => syntax::Expr::Await {
            expr: Box::new(rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::StructLiteral {
            name,
            type_args,
            fields,
            line,
            column,
        } => {
            let rewritten_type_args = type_args
                .iter()
                .map(|type_arg| {
                    rewrite_aggregate_type_name(
                        type_arg,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                        *line,
                        *column,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            let rewritten_name = if !rewritten_type_args.is_empty() {
                let type_params = generic_structs
                    .get(name)
                    .map(|decl| decl.type_params.as_slice())
                    .or_else(|| {
                        generic_enums
                            .get(name)
                            .map(|decl| decl.type_params.as_slice())
                    })
                    .ok_or_else(|| {
                        Diagnostic::new("type", format!("type {name:?} is not generic"))
                            .with_span(*line, *column)
                    })?;
                generic_decl_type_bindings(
                    name,
                    type_params,
                    &rewritten_type_args,
                    *line,
                    *column,
                )?;
                let instantiation = GenericInstantiation {
                    name: name.clone(),
                    type_args: rewritten_type_args.clone(),
                };
                if queued.insert(instantiation.clone()) {
                    queue.push_back(instantiation);
                }
                monomorphized_type_name(name, &rewritten_type_args)
            } else {
                name.clone()
            };
            syntax::Expr::StructLiteral {
                name: rewritten_name,
                type_args: Vec::new(),
                fields: fields
                    .iter()
                    .map(|field| {
                        Ok(syntax::StructFieldValue {
                            name: field.name.clone(),
                            expr: rewrite_expr_aggregate_types(
                                &field.expr,
                                generic_structs,
                                generic_enums,
                                queue,
                                queued,
                            )?,
                            line: field.line,
                            column: field.column,
                        })
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?,
                line: *line,
                column: *column,
            }
        }
        syntax::Expr::FieldAccess {
            base,
            field,
            line,
            column,
        } => syntax::Expr::FieldAccess {
            base: Box::new(rewrite_expr_aggregate_types(
                base,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            field: field.clone(),
            line: *line,
            column: *column,
        },
        syntax::Expr::TupleLiteral {
            elements,
            line,
            column,
        } => syntax::Expr::TupleLiteral {
            elements: elements
                .iter()
                .map(|element| {
                    rewrite_expr_aggregate_types(
                        element,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::TupleIndex {
            base,
            index,
            line,
            column,
        } => syntax::Expr::TupleIndex {
            base: Box::new(rewrite_expr_aggregate_types(
                base,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            index: *index,
            line: *line,
            column: *column,
        },
        syntax::Expr::MapLiteral {
            entries,
            line,
            column,
        } => syntax::Expr::MapLiteral {
            entries: entries
                .iter()
                .map(|entry| {
                    Ok(syntax::MapEntry {
                        key: rewrite_expr_aggregate_types(
                            &entry.key,
                            generic_structs,
                            generic_enums,
                            queue,
                            queued,
                        )?,
                        value: rewrite_expr_aggregate_types(
                            &entry.value,
                            generic_structs,
                            generic_enums,
                            queue,
                            queued,
                        )?,
                        line: entry.line,
                        column: entry.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::ArrayLiteral {
            elements,
            line,
            column,
        } => syntax::Expr::ArrayLiteral {
            elements: elements
                .iter()
                .map(|element| {
                    rewrite_expr_aggregate_types(
                        element,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::Slice {
            base,
            start,
            end,
            line,
            column,
        } => syntax::Expr::Slice {
            base: Box::new(rewrite_expr_aggregate_types(
                base,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            start: start
                .as_ref()
                .map(|expr| {
                    rewrite_expr_aggregate_types(
                        expr,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                    )
                    .map(Box::new)
                })
                .transpose()?,
            end: end
                .as_ref()
                .map(|expr| {
                    rewrite_expr_aggregate_types(
                        expr,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                    )
                    .map(Box::new)
                })
                .transpose()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::Index {
            base,
            index,
            line,
            column,
        } => syntax::Expr::Index {
            base: Box::new(rewrite_expr_aggregate_types(
                base,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            index: Box::new(rewrite_expr_aggregate_types(
                index,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Closure {
            params,
            body,
            line,
            column,
        } => syntax::Expr::Closure {
            params: params
                .iter()
                .map(|param| {
                    Ok(syntax::Param {
                        name: param.name.clone(),
                        ty: rewrite_aggregate_type_name(
                            &param.ty,
                            generic_structs,
                            generic_enums,
                            queue,
                            queued,
                            param.line,
                            param.column,
                        )?,
                        line: param.line,
                        column: param.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            body: Box::new(rewrite_expr_aggregate_types(
                body,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Match {
            expr,
            arms,
            line,
            column,
        } => syntax::Expr::Match {
            expr: Box::new(rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            arms: arms
                .iter()
                .map(|arm| {
                    Ok(syntax::MatchExprArm {
                        variant: arm.variant.clone(),
                        bindings: arm.bindings.clone(),
                        is_named: arm.is_named,
                        expr: rewrite_expr_aggregate_types(
                            &arm.expr,
                            generic_structs,
                            generic_enums,
                            queue,
                            queued,
                        )?,
                        line: arm.line,
                        column: arm.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
    })
}

fn rewrite_stmt_generic_calls(
    stmt: &syntax::Stmt,
    type_bindings: &HashMap<String, syntax::TypeName>,
    generic_functions: &HashMap<String, syntax::Function>,
    queue: &mut VecDeque<GenericInstantiation>,
    queued: &mut HashSet<GenericInstantiation>,
) -> Result<syntax::Stmt, Diagnostic> {
    Ok(match stmt {
        syntax::Stmt::Let {
            name,
            ty,
            expr,
            line,
            column,
        } => syntax::Stmt::Let {
            name: name.clone(),
            ty: substitute_type_name(ty, type_bindings),
            expr: rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Print { expr, line, column } => syntax::Stmt::Print {
            expr: rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Assign {
            target,
            expr,
            line,
            column,
        } => syntax::Stmt::Assign {
            target: rewrite_expr_generic_calls(
                target,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?,
            expr: rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Panic { expr, line, column } => syntax::Stmt::Panic {
            expr: match expr {
                syntax::Expr::Call {
                    name,
                    type_args,
                    args,
                    line,
                    column,
                } if name == "panic" => syntax::Expr::Call {
                    name: name.clone(),
                    type_args: type_args
                        .iter()
                        .map(|type_arg| substitute_type_name(type_arg, type_bindings))
                        .collect(),
                    args: args
                        .iter()
                        .map(|arg| {
                            rewrite_expr_generic_calls(
                                arg,
                                type_bindings,
                                generic_functions,
                                queue,
                                queued,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    line: *line,
                    column: *column,
                },
                _ => rewrite_expr_generic_calls(
                    expr,
                    type_bindings,
                    generic_functions,
                    queue,
                    queued,
                )?,
            },
            line: *line,
            column: *column,
        },
        syntax::Stmt::Defer { expr, line, column } => syntax::Stmt::Defer {
            expr: rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::If {
            cond,
            then_block,
            else_block,
            line,
            column,
        } => syntax::Stmt::If {
            cond: rewrite_expr_generic_calls(
                cond,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?,
            then_block: then_block
                .iter()
                .map(|stmt| {
                    rewrite_stmt_generic_calls(
                        stmt,
                        type_bindings,
                        generic_functions,
                        queue,
                        queued,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            else_block: else_block
                .as_ref()
                .map(|block| {
                    block
                        .iter()
                        .map(|stmt| {
                            rewrite_stmt_generic_calls(
                                stmt,
                                type_bindings,
                                generic_functions,
                                queue,
                                queued,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::IfLet {
            variant,
            bindings,
            is_named,
            expr,
            then_block,
            else_block,
            line,
            column,
        } => syntax::Stmt::IfLet {
            variant: variant.clone(),
            bindings: bindings.clone(),
            is_named: *is_named,
            expr: rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?,
            then_block: then_block
                .iter()
                .map(|stmt| {
                    rewrite_stmt_generic_calls(
                        stmt,
                        type_bindings,
                        generic_functions,
                        queue,
                        queued,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            else_block: else_block
                .as_ref()
                .map(|block| {
                    block
                        .iter()
                        .map(|stmt| {
                            rewrite_stmt_generic_calls(
                                stmt,
                                type_bindings,
                                generic_functions,
                                queue,
                                queued,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::While {
            cond,
            body,
            line,
            column,
        } => syntax::Stmt::While {
            cond: rewrite_expr_generic_calls(
                cond,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?,
            body: body
                .iter()
                .map(|stmt| {
                    rewrite_stmt_generic_calls(
                        stmt,
                        type_bindings,
                        generic_functions,
                        queue,
                        queued,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Match {
            expr,
            arms,
            line,
            column,
        } => syntax::Stmt::Match {
            expr: rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?,
            arms: arms
                .iter()
                .map(|arm| {
                    Ok(syntax::MatchArm {
                        variant: arm.variant.clone(),
                        bindings: arm.bindings.clone(),
                        is_named: arm.is_named,
                        body: arm
                            .body
                            .iter()
                            .map(|stmt| {
                                rewrite_stmt_generic_calls(
                                    stmt,
                                    type_bindings,
                                    generic_functions,
                                    queue,
                                    queued,
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                        line: arm.line,
                        column: arm.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Return { expr, line, column } => syntax::Stmt::Return {
            expr: rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
    })
}

fn rewrite_expr_generic_calls(
    expr: &syntax::Expr,
    type_bindings: &HashMap<String, syntax::TypeName>,
    generic_functions: &HashMap<String, syntax::Function>,
    queue: &mut VecDeque<GenericInstantiation>,
    queued: &mut HashSet<GenericInstantiation>,
) -> Result<syntax::Expr, Diagnostic> {
    Ok(match expr {
        syntax::Expr::Literal(_) | syntax::Expr::VarRef { .. } => expr.clone(),
        syntax::Expr::Call {
            name,
            type_args,
            args,
            line,
            column,
        } => {
            let args = args
                .iter()
                .map(|arg| {
                    rewrite_expr_generic_calls(arg, type_bindings, generic_functions, queue, queued)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let type_args = type_args
                .iter()
                .map(|type_arg| substitute_type_name(type_arg, type_bindings))
                .collect::<Vec<_>>();
            let name = if let Some(template) = generic_functions.get(name) {
                if type_args.is_empty() {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "generic function {:?} requires explicit type arguments",
                            name
                        ),
                    )
                    .with_span(*line, *column));
                }
                generic_type_bindings(template, &type_args)?;
                let instantiation = GenericInstantiation {
                    name: name.clone(),
                    type_args: type_args.clone(),
                };
                if queued.insert(instantiation.clone()) {
                    queue.push_back(instantiation);
                }
                monomorphized_function_name(name, &type_args)
            } else {
                if !type_args.is_empty() && !preserves_intrinsic_type_args(name) {
                    return Err(Diagnostic::new(
                        "type",
                        format!("function {:?} is not generic", name),
                    )
                    .with_span(*line, *column));
                }
                name.clone()
            };
            let keep_type_args = preserves_intrinsic_type_args(name.as_str());
            syntax::Expr::Call {
                name,
                type_args: if keep_type_args {
                    type_args
                } else {
                    Vec::new()
                },
                args,
                line: *line,
                column: *column,
            }
        }
        syntax::Expr::MethodCall {
            base,
            method,
            type_args,
            args,
            line,
            column,
        } => syntax::Expr::MethodCall {
            base: Box::new(rewrite_expr_generic_calls(
                base,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            method: method.clone(),
            type_args: type_args
                .iter()
                .map(|type_arg| substitute_type_name(type_arg, type_bindings))
                .collect(),
            args: args
                .iter()
                .map(|arg| {
                    rewrite_expr_generic_calls(arg, type_bindings, generic_functions, queue, queued)
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::BinaryAdd {
            op,
            lhs,
            rhs,
            line,
            column,
        } => syntax::Expr::BinaryAdd {
            op: *op,
            lhs: Box::new(rewrite_expr_generic_calls(
                lhs,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            rhs: Box::new(rewrite_expr_generic_calls(
                rhs,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::BinaryCompare {
            op,
            lhs,
            rhs,
            line,
            column,
        } => syntax::Expr::BinaryCompare {
            op: *op,
            lhs: Box::new(rewrite_expr_generic_calls(
                lhs,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            rhs: Box::new(rewrite_expr_generic_calls(
                rhs,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::BinaryLogic {
            op,
            lhs,
            rhs,
            line,
            column,
        } => syntax::Expr::BinaryLogic {
            op: *op,
            lhs: Box::new(rewrite_expr_generic_calls(
                lhs,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            rhs: Box::new(rewrite_expr_generic_calls(
                rhs,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Cast {
            expr,
            ty,
            line,
            column,
        } => syntax::Expr::Cast {
            expr: Box::new(rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            ty: substitute_type_name(ty, type_bindings),
            line: *line,
            column: *column,
        },
        syntax::Expr::Try { expr, line, column } => syntax::Expr::Try {
            expr: Box::new(rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::MutBorrow { expr, line, column } => syntax::Expr::MutBorrow {
            expr: Box::new(rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Deref { expr, line, column } => syntax::Expr::Deref {
            expr: Box::new(rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Await { expr, line, column } => syntax::Expr::Await {
            expr: Box::new(rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::StructLiteral {
            name,
            type_args,
            fields,
            line,
            column,
        } => syntax::Expr::StructLiteral {
            name: name.clone(),
            type_args: type_args.clone(),
            fields: fields
                .iter()
                .map(|field| {
                    Ok(syntax::StructFieldValue {
                        name: field.name.clone(),
                        expr: rewrite_expr_generic_calls(
                            &field.expr,
                            type_bindings,
                            generic_functions,
                            queue,
                            queued,
                        )?,
                        line: field.line,
                        column: field.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::FieldAccess {
            base,
            field,
            line,
            column,
        } => syntax::Expr::FieldAccess {
            base: Box::new(rewrite_expr_generic_calls(
                base,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            field: field.clone(),
            line: *line,
            column: *column,
        },
        syntax::Expr::TupleLiteral {
            elements,
            line,
            column,
        } => syntax::Expr::TupleLiteral {
            elements: elements
                .iter()
                .map(|element| {
                    rewrite_expr_generic_calls(
                        element,
                        type_bindings,
                        generic_functions,
                        queue,
                        queued,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::TupleIndex {
            base,
            index,
            line,
            column,
        } => syntax::Expr::TupleIndex {
            base: Box::new(rewrite_expr_generic_calls(
                base,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            index: *index,
            line: *line,
            column: *column,
        },
        syntax::Expr::MapLiteral {
            entries,
            line,
            column,
        } => syntax::Expr::MapLiteral {
            entries: entries
                .iter()
                .map(|entry| {
                    Ok(syntax::MapEntry {
                        key: rewrite_expr_generic_calls(
                            &entry.key,
                            type_bindings,
                            generic_functions,
                            queue,
                            queued,
                        )?,
                        value: rewrite_expr_generic_calls(
                            &entry.value,
                            type_bindings,
                            generic_functions,
                            queue,
                            queued,
                        )?,
                        line: entry.line,
                        column: entry.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::ArrayLiteral {
            elements,
            line,
            column,
        } => syntax::Expr::ArrayLiteral {
            elements: elements
                .iter()
                .map(|element| {
                    rewrite_expr_generic_calls(
                        element,
                        type_bindings,
                        generic_functions,
                        queue,
                        queued,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::Slice {
            base,
            start,
            end,
            line,
            column,
        } => syntax::Expr::Slice {
            base: Box::new(rewrite_expr_generic_calls(
                base,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            start: start
                .as_ref()
                .map(|expr| {
                    rewrite_expr_generic_calls(
                        expr,
                        type_bindings,
                        generic_functions,
                        queue,
                        queued,
                    )
                    .map(Box::new)
                })
                .transpose()?,
            end: end
                .as_ref()
                .map(|expr| {
                    rewrite_expr_generic_calls(
                        expr,
                        type_bindings,
                        generic_functions,
                        queue,
                        queued,
                    )
                    .map(Box::new)
                })
                .transpose()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::Index {
            base,
            index,
            line,
            column,
        } => syntax::Expr::Index {
            base: Box::new(rewrite_expr_generic_calls(
                base,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            index: Box::new(rewrite_expr_generic_calls(
                index,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Closure {
            params,
            body,
            line,
            column,
        } => syntax::Expr::Closure {
            params: params
                .iter()
                .map(|param| syntax::Param {
                    name: param.name.clone(),
                    ty: substitute_type_name(&param.ty, type_bindings),
                    line: param.line,
                    column: param.column,
                })
                .collect(),
            body: Box::new(rewrite_expr_generic_calls(
                body,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Match {
            expr,
            arms,
            line,
            column,
        } => syntax::Expr::Match {
            expr: Box::new(rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            arms: arms
                .iter()
                .map(|arm| {
                    Ok(syntax::MatchExprArm {
                        variant: arm.variant.clone(),
                        bindings: arm.bindings.clone(),
                        is_named: arm.is_named,
                        expr: rewrite_expr_generic_calls(
                            &arm.expr,
                            type_bindings,
                            generic_functions,
                            queue,
                            queued,
                        )?,
                        line: arm.line,
                        column: arm.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
    })
}

fn substitute_type_name(
    ty: &syntax::TypeName,
    type_bindings: &HashMap<String, syntax::TypeName>,
) -> syntax::TypeName {
    match ty {
        syntax::TypeName::Named(name, args) if args.is_empty() => type_bindings
            .get(name)
            .cloned()
            .unwrap_or_else(|| syntax::TypeName::Named(name.clone(), Vec::new())),
        syntax::TypeName::Named(name, args) => syntax::TypeName::Named(
            name.clone(),
            args.iter()
                .map(|arg| substitute_type_name(arg, type_bindings))
                .collect(),
        ),
        syntax::TypeName::Ptr(inner) => {
            syntax::TypeName::Ptr(Box::new(substitute_type_name(inner, type_bindings)))
        }
        syntax::TypeName::MutPtr(inner) => {
            syntax::TypeName::MutPtr(Box::new(substitute_type_name(inner, type_bindings)))
        }
        syntax::TypeName::MutRef(inner) => {
            syntax::TypeName::MutRef(Box::new(substitute_type_name(inner, type_bindings)))
        }
        syntax::TypeName::Slice(inner) => {
            syntax::TypeName::Slice(Box::new(substitute_type_name(inner, type_bindings)))
        }
        syntax::TypeName::MutSlice(inner) => {
            syntax::TypeName::MutSlice(Box::new(substitute_type_name(inner, type_bindings)))
        }
        syntax::TypeName::LifetimeSlice(lifetime, inner) => syntax::TypeName::LifetimeSlice(
            lifetime.clone(),
            Box::new(substitute_type_name(inner, type_bindings)),
        ),
        syntax::TypeName::LifetimeMutSlice(lifetime, inner) => syntax::TypeName::LifetimeMutSlice(
            lifetime.clone(),
            Box::new(substitute_type_name(inner, type_bindings)),
        ),
        syntax::TypeName::Option(inner) => {
            syntax::TypeName::Option(Box::new(substitute_type_name(inner, type_bindings)))
        }
        syntax::TypeName::Result(ok, err) => syntax::TypeName::Result(
            Box::new(substitute_type_name(ok, type_bindings)),
            Box::new(substitute_type_name(err, type_bindings)),
        ),
        syntax::TypeName::Tuple(elements) => syntax::TypeName::Tuple(
            elements
                .iter()
                .map(|element| substitute_type_name(element, type_bindings))
                .collect(),
        ),
        syntax::TypeName::Map(key, value) => syntax::TypeName::Map(
            Box::new(substitute_type_name(key, type_bindings)),
            Box::new(substitute_type_name(value, type_bindings)),
        ),
        syntax::TypeName::Array(inner, len) => syntax::TypeName::Array(
            Box::new(substitute_type_name(inner, type_bindings)),
            len.clone(),
        ),
        syntax::TypeName::Fn(params, return_ty) => syntax::TypeName::Fn(
            params
                .iter()
                .map(|param| substitute_type_name(param, type_bindings))
                .collect(),
            Box::new(substitute_type_name(return_ty, type_bindings)),
        ),
        syntax::TypeName::Int => syntax::TypeName::Int,
        syntax::TypeName::Numeric(numeric) => syntax::TypeName::Numeric(*numeric),
        syntax::TypeName::Bool => syntax::TypeName::Bool,
        syntax::TypeName::String => syntax::TypeName::String,
        syntax::TypeName::Str => syntax::TypeName::Str,
    }
}
