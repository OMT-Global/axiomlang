use super::model::{ArithmeticOp, CompareOp, Expr, LiteralValue, LogicOp, Type};
use super::{const_arrays::resolve_const_array_len, is_async_runtime_type};
use crate::diagnostics::Diagnostic;
use crate::syntax;
use std::collections::{HashMap, HashSet};

pub(super) fn lower_literal(literal: &syntax::Literal, expected: Option<&Type>) -> Expr {
    match literal {
        syntax::Literal::Int(value) => Expr::Literal {
            ty: Type::Int,
            value: LiteralValue::Int(*value),
        },
        syntax::Literal::Numeric { raw, ty } => Expr::Literal {
            ty: Type::Numeric(*ty),
            value: LiteralValue::Numeric {
                raw: raw.clone(),
                ty: *ty,
            },
        },
        syntax::Literal::Bool(value) => Expr::Literal {
            ty: Type::Bool,
            value: LiteralValue::Bool(*value),
        },
        syntax::Literal::String(value) => Expr::Literal {
            ty: if matches!(expected, Some(Type::Str)) {
                Type::Str
            } else {
                Type::String
            },
            value: LiteralValue::String(value.clone()),
        },
    }
}

pub(super) fn lower_type<T, U>(
    ty: &syntax::TypeName,
    structs: &HashMap<String, T>,
    enums: &HashMap<String, U>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
    consts: &HashMap<String, syntax::ConstDecl>,
    line: usize,
    column: usize,
) -> Result<Type, Diagnostic> {
    let mut resolving = HashSet::new();
    lower_type_inner(
        ty,
        structs,
        enums,
        aliases,
        consts,
        &mut resolving,
        line,
        column,
    )
}

fn lower_type_inner<T, U>(
    ty: &syntax::TypeName,
    structs: &HashMap<String, T>,
    enums: &HashMap<String, U>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
    consts: &HashMap<String, syntax::ConstDecl>,
    resolving: &mut HashSet<String>,
    line: usize,
    column: usize,
) -> Result<Type, Diagnostic> {
    match ty {
        syntax::TypeName::Int => Ok(Type::Int),
        syntax::TypeName::Numeric(numeric) => Ok(Type::Numeric(*numeric)),
        syntax::TypeName::Bool => Ok(Type::Bool),
        syntax::TypeName::String => Ok(Type::String),
        syntax::TypeName::Str => Ok(Type::Str),
        syntax::TypeName::Named(name, args) => {
            if is_async_runtime_type(name) {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("async runtime type {name:?} expects 1 type argument"),
                    )
                    .with_span(line, column));
                }
                let inner = Box::new(lower_type_inner(
                    &args[0], structs, enums, aliases, consts, resolving, line, column,
                )?);
                return Ok(match name.as_str() {
                    "Task" => Type::Task(inner),
                    "JoinHandle" => Type::JoinHandle(inner),
                    "AsyncChannel" => Type::AsyncChannel(inner),
                    "SelectResult" => Type::SelectResult(inner),
                    _ => unreachable!("async runtime type checked above"),
                });
            }
            if !args.is_empty() {
                return Err(Diagnostic::new(
                    "type",
                    format!("generic type {name:?} was not monomorphized"),
                )
                .with_span(line, column));
            }
            if structs.contains_key(name) {
                return Ok(Type::Struct(name.clone()));
            }
            if enums.contains_key(name) {
                return Ok(Type::Enum(name.clone()));
            }
            if let Some(type_alias) = aliases.get(name) {
                if !resolving.insert(name.clone()) {
                    return Err(Diagnostic::new(
                        "type",
                        format!("type alias {name:?} is recursive"),
                    )
                    .with_span(type_alias.line, type_alias.column));
                }
                let lowered = lower_type_inner(
                    &type_alias.ty,
                    structs,
                    enums,
                    aliases,
                    consts,
                    resolving,
                    type_alias.line,
                    type_alias.column,
                );
                resolving.remove(name);
                return lowered;
            }
            Err(Diagnostic::new("type", format!("unknown type {name:?}")).with_span(line, column))
        }
        syntax::TypeName::Ptr(inner) => Ok(Type::Ptr(Box::new(lower_type_inner(
            inner, structs, enums, aliases, consts, resolving, line, column,
        )?))),
        syntax::TypeName::MutPtr(inner) => Ok(Type::MutPtr(Box::new(lower_type_inner(
            inner, structs, enums, aliases, consts, resolving, line, column,
        )?))),
        syntax::TypeName::MutRef(inner) => Ok(Type::MutRef(Box::new(lower_type_inner(
            inner, structs, enums, aliases, consts, resolving, line, column,
        )?))),
        syntax::TypeName::Slice(inner) | syntax::TypeName::LifetimeSlice(_, inner) => {
            Ok(Type::Slice(Box::new(lower_type_inner(
                inner, structs, enums, aliases, consts, resolving, line, column,
            )?)))
        }
        syntax::TypeName::MutSlice(inner) | syntax::TypeName::LifetimeMutSlice(_, inner) => {
            Ok(Type::MutSlice(Box::new(lower_type_inner(
                inner, structs, enums, aliases, consts, resolving, line, column,
            )?)))
        }
        syntax::TypeName::Option(inner) => Ok(Type::Option(Box::new(lower_type_inner(
            inner, structs, enums, aliases, consts, resolving, line, column,
        )?))),
        syntax::TypeName::Result(ok, err) => Ok(Type::Result(
            Box::new(lower_type_inner(
                ok, structs, enums, aliases, consts, resolving, line, column,
            )?),
            Box::new(lower_type_inner(
                err, structs, enums, aliases, consts, resolving, line, column,
            )?),
        )),
        syntax::TypeName::Tuple(elements) => Ok(Type::Tuple(
            elements
                .iter()
                .map(|element| {
                    lower_type_inner(
                        element, structs, enums, aliases, consts, resolving, line, column,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
        )),
        syntax::TypeName::Map(key, value) => {
            let key = lower_type_inner(
                key, structs, enums, aliases, consts, resolving, line, column,
            )?;
            if !key.supports_map_key() {
                return Err(Diagnostic::new(
                    "type",
                    format!("map key type {key} is not supported"),
                )
                .with_span(line, column));
            }
            Ok(Type::Map(
                Box::new(key),
                Box::new(lower_type_inner(
                    value, structs, enums, aliases, consts, resolving, line, column,
                )?),
            ))
        }
        syntax::TypeName::Array(inner, len) => {
            let len = len
                .as_ref()
                .map(|raw| resolve_const_array_len(raw.trim(), consts, line, column))
                .transpose()?
                .map(|value| value as usize);
            Ok(Type::Array(
                Box::new(lower_type_inner(
                    inner, structs, enums, aliases, consts, resolving, line, column,
                )?),
                len,
            ))
        }
        syntax::TypeName::Fn(params, return_ty) => Ok(Type::Fn(
            params
                .iter()
                .map(|param| {
                    lower_type_inner(
                        param, structs, enums, aliases, consts, resolving, line, column,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            Box::new(lower_type_inner(
                return_ty, structs, enums, aliases, consts, resolving, line, column,
            )?),
        )),
    }
}

pub(super) fn lower_compare_op(op: syntax::CompareOp) -> CompareOp {
    match op {
        syntax::CompareOp::Eq => CompareOp::Eq,
        syntax::CompareOp::Ne => CompareOp::Ne,
        syntax::CompareOp::Lt => CompareOp::Lt,
        syntax::CompareOp::Le => CompareOp::Le,
        syntax::CompareOp::Gt => CompareOp::Gt,
        syntax::CompareOp::Ge => CompareOp::Ge,
    }
}

pub(super) fn lower_logic_op(op: syntax::LogicOp) -> LogicOp {
    match op {
        syntax::LogicOp::And => LogicOp::And,
        syntax::LogicOp::Or => LogicOp::Or,
    }
}

pub(super) fn lower_arithmetic_op(op: syntax::ArithmeticOp) -> ArithmeticOp {
    match op {
        syntax::ArithmeticOp::Add => ArithmeticOp::Add,
        syntax::ArithmeticOp::Sub => ArithmeticOp::Sub,
        syntax::ArithmeticOp::Mul => ArithmeticOp::Mul,
        syntax::ArithmeticOp::Div => ArithmeticOp::Div,
    }
}
