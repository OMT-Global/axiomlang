use crate::diagnostics::Diagnostic;
use crate::syntax;
use std::collections::{HashMap, HashSet};

pub(super) fn declared_array_len(
    ty: &syntax::TypeName,
    consts: &HashMap<String, syntax::ConstDecl>,
    line: usize,
    column: usize,
) -> Result<Option<usize>, Diagnostic> {
    match ty {
        syntax::TypeName::Array(_, Some(raw)) => {
            let value = resolve_const_array_len(raw.trim(), consts, line, column)?;
            if value < 0 {
                return Err(Diagnostic::new("type", "array length must be non-negative")
                    .with_span(line, column));
            }
            Ok(Some(value as usize))
        }
        _ => Ok(None),
    }
}

pub(super) fn resolve_const_array_len(
    raw: &str,
    consts: &HashMap<String, syntax::ConstDecl>,
    line: usize,
    column: usize,
) -> Result<i64, Diagnostic> {
    if let Ok(value) = raw.parse::<i64>() {
        return Ok(value);
    }
    let Some(const_decl) = consts.get(raw) else {
        return Err(Diagnostic::new(
            "type",
            format!("array length {raw:?} must be a known int const/static expression"),
        )
        .with_span(line, column));
    };
    eval_const_int_expr(&const_decl.expr).ok_or_else(|| {
        Diagnostic::new(
            "type",
            format!(
                "array length const {:?} must evaluate to int",
                const_decl.name
            ),
        )
        .with_span(const_decl.line, const_decl.column)
    })
}

pub(super) fn eval_const_int_expr(expr: &syntax::Expr) -> Option<i64> {
    match expr {
        syntax::Expr::Literal(syntax::Literal::Int(value)) => Some(*value),
        syntax::Expr::BinaryAdd { op, lhs, rhs, .. } => match op {
            syntax::ArithmeticOp::Add => {
                Some(eval_const_int_expr(lhs)? + eval_const_int_expr(rhs)?)
            }
            syntax::ArithmeticOp::Sub => {
                Some(eval_const_int_expr(lhs)? - eval_const_int_expr(rhs)?)
            }
            syntax::ArithmeticOp::Mul => {
                Some(eval_const_int_expr(lhs)? * eval_const_int_expr(rhs)?)
            }
            syntax::ArithmeticOp::Div => {
                Some(eval_const_int_expr(lhs)? / eval_const_int_expr(rhs)?)
            }
        },
        _ => None,
    }
}

pub(super) fn resolve_const_int_decls(
    consts: &HashMap<String, syntax::ConstDecl>,
    functions: &HashMap<String, syntax::Function>,
) -> Result<HashMap<String, syntax::ConstDecl>, Diagnostic> {
    let mut resolved = consts.clone();
    let mut values = HashMap::new();
    for name in consts.keys() {
        let mut resolving = HashSet::new();
        match eval_const_int_decl(name, consts, functions, &mut values, &mut resolving)? {
            Some(value) => {
                if let Some(decl) = resolved.get_mut(name) {
                    decl.expr = syntax::Expr::Literal(syntax::Literal::Int(value));
                }
            }
            None => {
                let decl = &consts[name];
                if matches!(decl.ty, syntax::TypeName::Int)
                    && const_int_expr_contains_call(&decl.expr)
                {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "const {:?} requires a pure const fn integer expression",
                            decl.name
                        ),
                    )
                    .with_span(decl.line, decl.column));
                }
            }
        }
    }
    Ok(resolved)
}

fn eval_const_int_decl(
    name: &str,
    consts: &HashMap<String, syntax::ConstDecl>,
    functions: &HashMap<String, syntax::Function>,
    values: &mut HashMap<String, i64>,
    resolving: &mut HashSet<String>,
) -> Result<Option<i64>, Diagnostic> {
    if let Some(value) = values.get(name) {
        return Ok(Some(*value));
    }
    let Some(decl) = consts.get(name) else {
        return Ok(None);
    };
    if !resolving.insert(name.to_string()) {
        return Err(Diagnostic::new(
            "type",
            format!("const {name:?} has a recursive initializer"),
        )
        .with_span(decl.line, decl.column));
    }
    let mut locals = HashMap::new();
    let value = eval_const_int_expr_resolved(
        &decl.expr,
        consts,
        functions,
        values,
        resolving,
        &mut locals,
    )?;
    resolving.remove(name);
    if let Some(value) = value {
        values.insert(name.to_string(), value);
    }
    Ok(value)
}

fn eval_const_int_expr_resolved(
    expr: &syntax::Expr,
    consts: &HashMap<String, syntax::ConstDecl>,
    functions: &HashMap<String, syntax::Function>,
    values: &mut HashMap<String, i64>,
    resolving: &mut HashSet<String>,
    locals: &mut HashMap<String, i64>,
) -> Result<Option<i64>, Diagnostic> {
    match expr {
        syntax::Expr::Literal(syntax::Literal::Int(value)) => Ok(Some(*value)),
        syntax::Expr::VarRef { name, .. } => {
            if let Some(value) = locals.get(name) {
                return Ok(Some(*value));
            }
            eval_const_int_decl(name, consts, functions, values, resolving)
        }
        syntax::Expr::BinaryAdd { op, lhs, rhs, .. } => {
            let lhs =
                eval_const_int_expr_resolved(lhs, consts, functions, values, resolving, locals)?;
            let rhs =
                eval_const_int_expr_resolved(rhs, consts, functions, values, resolving, locals)?;
            Ok(match (lhs, rhs) {
                (Some(lhs), Some(rhs)) => Some(match op {
                    syntax::ArithmeticOp::Add => lhs + rhs,
                    syntax::ArithmeticOp::Sub => lhs - rhs,
                    syntax::ArithmeticOp::Mul => lhs * rhs,
                    syntax::ArithmeticOp::Div => lhs / rhs,
                }),
                _ => None,
            })
        }
        syntax::Expr::Call { name, args, .. } => {
            let Some(function) = functions.get(name) else {
                return Ok(None);
            };
            if !function.is_const || function.is_extern || function.params.len() != args.len() {
                return Ok(None);
            }
            let mut function_locals = HashMap::new();
            for (param, arg) in function.params.iter().zip(args.iter()) {
                let Some(value) = eval_const_int_expr_resolved(
                    arg, consts, functions, values, resolving, locals,
                )?
                else {
                    return Ok(None);
                };
                function_locals.insert(param.name.clone(), value);
            }
            eval_const_int_block(
                &function.body,
                consts,
                functions,
                values,
                resolving,
                &mut function_locals,
            )
        }
        _ => Ok(None),
    }
}

fn eval_const_int_block(
    body: &[syntax::Stmt],
    consts: &HashMap<String, syntax::ConstDecl>,
    functions: &HashMap<String, syntax::Function>,
    values: &mut HashMap<String, i64>,
    resolving: &mut HashSet<String>,
    locals: &mut HashMap<String, i64>,
) -> Result<Option<i64>, Diagnostic> {
    for stmt in body {
        match stmt {
            syntax::Stmt::Let { name, expr, .. } => {
                let Some(value) = eval_const_int_expr_resolved(
                    expr, consts, functions, values, resolving, locals,
                )?
                else {
                    return Ok(None);
                };
                locals.insert(name.clone(), value);
            }
            syntax::Stmt::Return { expr, .. } => {
                return eval_const_int_expr_resolved(
                    expr, consts, functions, values, resolving, locals,
                );
            }
            _ => return Ok(None),
        }
    }
    Ok(None)
}

fn const_int_expr_contains_call(expr: &syntax::Expr) -> bool {
    match expr {
        syntax::Expr::Call { .. } => true,
        syntax::Expr::BinaryAdd { lhs, rhs, .. } | syntax::Expr::BinaryCompare { lhs, rhs, .. } => {
            const_int_expr_contains_call(lhs) || const_int_expr_contains_call(rhs)
        }
        _ => false,
    }
}

pub(super) fn validate_const_array_lengths_in_program(
    program: &syntax::Program,
    consts: &HashMap<String, syntax::ConstDecl>,
) -> Result<(), Diagnostic> {
    for struct_decl in &program.structs {
        for field in &struct_decl.fields {
            validate_const_array_lengths_in_type(&field.ty, consts, field.line, field.column)?;
        }
    }
    for enum_decl in &program.enums {
        for variant in &enum_decl.variants {
            for payload_ty in &variant.payload_tys {
                validate_const_array_lengths_in_type(
                    payload_ty,
                    consts,
                    variant.line,
                    variant.column,
                )?;
            }
        }
    }
    for alias in &program.type_aliases {
        validate_const_array_lengths_in_type(&alias.ty, consts, alias.line, alias.column)?;
    }
    for function in &program.functions {
        validate_const_array_lengths_in_type(
            &function.return_ty,
            consts,
            function.line,
            function.column,
        )?;
        for param in &function.params {
            validate_const_array_lengths_in_type(&param.ty, consts, param.line, param.column)?;
        }
    }
    for stmt in &program.stmts {
        if let syntax::Stmt::Let {
            ty, line, column, ..
        } = stmt
        {
            validate_const_array_lengths_in_type(ty, consts, *line, *column)?;
        }
    }
    Ok(())
}

fn validate_const_array_lengths_in_type(
    ty: &syntax::TypeName,
    consts: &HashMap<String, syntax::ConstDecl>,
    line: usize,
    column: usize,
) -> Result<(), Diagnostic> {
    match ty {
        syntax::TypeName::Array(inner, len) => {
            if let Some(raw) = len {
                let value = resolve_const_array_len(raw.trim(), consts, line, column)?;
                if value < 0 {
                    return Err(Diagnostic::new("type", "array length must be non-negative")
                        .with_span(line, column));
                }
            }
            validate_const_array_lengths_in_type(inner, consts, line, column)
        }
        syntax::TypeName::Slice(inner)
        | syntax::TypeName::MutSlice(inner)
        | syntax::TypeName::Option(inner) => {
            validate_const_array_lengths_in_type(inner, consts, line, column)
        }
        syntax::TypeName::Result(ok, err) | syntax::TypeName::Map(ok, err) => {
            validate_const_array_lengths_in_type(ok, consts, line, column)?;
            validate_const_array_lengths_in_type(err, consts, line, column)
        }
        syntax::TypeName::Tuple(elements) => {
            for element in elements {
                validate_const_array_lengths_in_type(element, consts, line, column)?;
            }
            Ok(())
        }
        syntax::TypeName::Named(_, args) => {
            for arg in args {
                validate_const_array_lengths_in_type(arg, consts, line, column)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
