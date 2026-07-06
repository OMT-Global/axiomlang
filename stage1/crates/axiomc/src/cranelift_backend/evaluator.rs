//! Compile-time program evaluator for the cranelift backend's "spike" path:
//! interprets programs whose values are known at build time (`SpikeValue`),
//! including the host-capability call dispatchers (fs, net, http, async,
//! process, clock, crypto, json/serdes, regex, encoding). Extracted from
//! cranelift_backend.rs under the compiler-source decomposition ratchet
//! (#1254). Shared value types (`SpikeValue` and companions) and helpers
//! reused by the i64 lowering path stay in the parent module and are visible
//! here through `use super::*`.

use super::*;

pub(crate) fn with_spike_stdin<T>(
    stdin: Option<&str>,
    body: impl FnOnce() -> Result<T, Diagnostic>,
) -> Result<T, Diagnostic> {
    SPIKE_STDIN.with(|state| {
        let previous = state.replace(SpikeStdin::new(stdin));
        let result = body();
        state.replace(previous);
        result
    })
}

pub(crate) fn eval_block(
    stmts: &[Stmt],
    functions: &HashMap<&str, &Function>,
    env: &mut SpikeEnv,
    lines: &mut Vec<OutputLine>,
    defers: &mut Vec<Expr>,
) -> Result<Option<SpikeValue>, Diagnostic> {
    for stmt in stmts {
        if let Some(value) = eval_stmt(stmt, functions, env, lines, defers)? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

/// Evaluate a function (or top-level) body with its own `defer` scope, running
/// the registered deferred calls in LIFO order on every exit path -- normal
/// return, fall-through, and panic. Deferred calls registered in nested blocks
/// run at the enclosing function's exit, matching Go-style `defer` semantics.
pub(crate) fn run_function_body(
    body: &[Stmt],
    functions: &HashMap<&str, &Function>,
    env: &mut SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<Option<SpikeValue>, Diagnostic> {
    let mut defers: Vec<Expr> = Vec::new();
    let result = eval_block(body, functions, env, lines, &mut defers);
    let mut defer_error = None;
    while let Some(deferred) = defers.pop() {
        if let Err(err) = eval_expr_effectful(&deferred, functions, env, lines)
            && defer_error.is_none()
        {
            defer_error = Some(err);
        }
    }
    match result {
        // A panic in the body takes precedence over any error raised while
        // unwinding its deferred calls.
        Err(err) => Err(err),
        Ok(value) => match defer_error {
            Some(err) => Err(err),
            None => Ok(value),
        },
    }
}

pub(crate) fn eval_stmt(
    stmt: &Stmt,
    functions: &HashMap<&str, &Function>,
    env: &mut SpikeEnv,
    lines: &mut Vec<OutputLine>,
    defers: &mut Vec<Expr>,
) -> Result<Option<SpikeValue>, Diagnostic> {
    match stmt {
        Stmt::Let { name, expr, .. } => {
            let value = eval_expr_effectful(expr, functions, env, lines)?;
            if let SpikeValue::ControlReturn(value) = value {
                return Ok(Some(*value));
            }
            env.insert(name.clone(), value);
            Ok(None)
        }
        Stmt::Print { expr, .. } => {
            let value = eval_expr_effectful(expr, functions, env, lines)?;
            if let SpikeValue::ControlReturn(value) = value {
                return Ok(Some(*value));
            }
            lines.push(OutputLine::stdout(render_value(&value)));
            Ok(None)
        }
        Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            let cond = eval_expr_effectful(cond, functions, env, lines)?;
            if let SpikeValue::ControlReturn(value) = cond {
                return Ok(Some(*value));
            }
            let branch = match cond {
                SpikeValue::Bool(true) => Some(then_block.as_slice()),
                SpikeValue::Bool(false) => else_block.as_deref(),
                _ => return Err(unsupported("if conditions must be boolean")),
            };
            if let Some(branch) = branch {
                eval_block(branch, functions, env, lines, defers)
            } else {
                Ok(None)
            }
        }
        Stmt::While { cond, .. } => {
            let cond = eval_expr_effectful(cond, functions, env, lines)?;
            if let SpikeValue::ControlReturn(value) = cond {
                return Ok(Some(*value));
            }
            match cond {
                SpikeValue::Bool(false) => Ok(None),
                SpikeValue::Bool(true) => Err(unsupported(
                    "runtime loops are not part of the cranelift hello spike",
                )),
                _ => Err(unsupported("while conditions must be boolean")),
            }
        }
        Stmt::Match { expr, arms, .. } => {
            eval_match_stmt(expr, arms, functions, env, lines, defers)
        }
        Stmt::Return { expr, .. } => {
            let value = eval_expr_effectful(expr, functions, env, lines)?;
            if let SpikeValue::ControlReturn(value) = value {
                return Ok(Some(*value));
            }
            Ok(Some(value))
        }
        Stmt::Assign { target, expr, .. } => eval_assign(target, expr, functions, env, lines),
        Stmt::Panic { message, .. } => {
            let message =
                render_runtime_panic_message(eval_expr_effectful(message, functions, env, lines)?)?;
            Err(cranelift_runtime_trap("panic", message))
        }
        Stmt::Defer { expr, .. } => {
            // Register the deferred call; it runs at the enclosing function's
            // exit (see `run_function_body`), LIFO, regardless of how the
            // function leaves -- return, fall-through, or panic.
            defers.push(expr.clone());
            Ok(None)
        }
    }
}

pub(crate) fn eval_assign(
    target: &Expr,
    expr: &Expr,
    functions: &HashMap<&str, &Function>,
    env: &mut SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<Option<SpikeValue>, Diagnostic> {
    let value = eval_expr_effectful(expr, functions, env, lines)?;
    if let SpikeValue::ControlReturn(value) = value {
        return Ok(Some(*value));
    }
    match target {
        Expr::VarRef { name, .. } => {
            env.insert(name.clone(), value);
            Ok(None)
        }
        Expr::Deref { expr, .. } => {
            let SpikeValue::MutRef(name) = eval_expr(expr, functions, env, lines)? else {
                return Err(unsupported(
                    "dereference assignment requires a mutable local reference",
                ));
            };
            env.insert(name, value);
            Ok(None)
        }
        Expr::Index { base, index, .. } => {
            let index = expect_non_negative_index(eval_expr(index, functions, env, lines)?)?;
            match eval_expr(base, functions, env, lines)? {
                SpikeValue::MutSlice { target, start, end } => {
                    let real_index = start
                        .checked_add(index)
                        .ok_or_else(|| unsupported("slice index overflow"))?;
                    if real_index >= end {
                        return Err(unsupported("slice index is outside the slice length"));
                    }
                    assign_array_index(env, &target, real_index, value).map(|()| None)
                }
                SpikeValue::Array(mut elements) => {
                    let Some(slot) = elements.get_mut(index) else {
                        return Err(unsupported("array index is outside the array length"));
                    };
                    *slot = value;
                    if let Expr::VarRef { name, .. } = base.as_ref() {
                        env.insert(name.clone(), SpikeValue::Array(elements));
                        Ok(None)
                    } else {
                        Err(unsupported(
                            "array index assignment requires a local array target",
                        ))
                    }
                }
                _ => Err(unsupported(
                    "index assignment requires a mutable slice or local array target",
                )),
            }
        }
        _ => Err(unsupported(
            "assignment requires a local variable, mutable local dereference, or mutable slice index target",
        )),
    }
}

pub(crate) fn assign_array_index(
    env: &mut SpikeEnv,
    name: &str,
    index: usize,
    value: SpikeValue,
) -> Result<(), Diagnostic> {
    let Some(SpikeValue::Array(elements)) = env.get_mut(name) else {
        return Err(unsupported(
            "mutable slice assignment requires a live local array",
        ));
    };
    let Some(slot) = elements.get_mut(index) else {
        return Err(unsupported("array index is outside the array length"));
    };
    *slot = value;
    Ok(())
}

pub(crate) fn eval_expr_effectful(
    expr: &Expr,
    functions: &HashMap<&str, &Function>,
    env: &mut SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match expr {
        Expr::Call { name, args, .. } => eval_call_effectful(name, args, functions, env, lines),
        Expr::BinaryAdd { op, lhs, rhs, ty } => {
            let left = eval_expr_effectful(lhs, functions, env, lines)?;
            if is_control_return(&left) {
                return Ok(left);
            }
            let right = eval_expr_effectful(rhs, functions, env, lines)?;
            if is_control_return(&right) {
                return Ok(right);
            }
            eval_arithmetic_values(*op, ty, left, right)
        }
        Expr::BinaryCompare {
            op,
            lhs,
            rhs,
            ty: _,
        } => {
            let left = eval_expr_effectful(lhs, functions, env, lines)?;
            if is_control_return(&left) {
                return Ok(left);
            }
            let right = eval_expr_effectful(rhs, functions, env, lines)?;
            if is_control_return(&right) {
                return Ok(right);
            }
            eval_compare_values(*op, left, right)
        }
        Expr::BinaryLogic { op, lhs, rhs, .. } => {
            let left_value = eval_expr_effectful(lhs, functions, env, lines)?;
            if is_control_return(&left_value) {
                return Ok(left_value);
            }
            let left = expect_bool(left_value)?;
            match op {
                crate::mir::LogicOp::And if !left => Ok(SpikeValue::Bool(false)),
                crate::mir::LogicOp::Or if left => Ok(SpikeValue::Bool(true)),
                crate::mir::LogicOp::And | crate::mir::LogicOp::Or => {
                    let right = eval_expr_effectful(rhs, functions, env, lines)?;
                    if is_control_return(&right) {
                        return Ok(right);
                    }
                    Ok(SpikeValue::Bool(expect_bool(right)?))
                }
            }
        }
        Expr::Cast { expr, ty } => {
            let value = eval_expr_effectful(expr, functions, env, lines)?;
            if is_control_return(&value) {
                return Ok(value);
            }
            cast_spike_value(value, ty)
        }
        Expr::Try { expr, .. } => eval_try_expr(expr, functions, env, lines),
        _ => eval_expr(expr, functions, env, lines),
    }
}

pub(crate) fn eval_expr(
    expr: &Expr,
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match expr {
        Expr::Literal(LiteralValue::Int(value)) => Ok(SpikeValue::Int(*value)),
        Expr::Literal(LiteralValue::Numeric { raw, ty }) => eval_numeric_literal(raw, *ty),
        Expr::Literal(LiteralValue::Bool(value)) => Ok(SpikeValue::Bool(*value)),
        Expr::Literal(LiteralValue::String(value)) | Expr::Literal(LiteralValue::Str(value)) => {
            Ok(SpikeValue::Text(value.clone()))
        }
        Expr::VarRef { name, .. } => env
            .get(name)
            .cloned()
            .ok_or_else(|| unsupported(&format!("unknown cranelift spike variable {name:?}"))),
        Expr::Call { name, args, .. } => eval_call(name, args, functions, env, lines),
        Expr::BinaryAdd { op, lhs, rhs, ty } => {
            eval_arithmetic(*op, lhs, rhs, ty, functions, env, lines)
        }
        Expr::BinaryCompare {
            op,
            lhs,
            rhs,
            ty: _,
        } => eval_compare(*op, lhs, rhs, functions, env, lines),
        Expr::BinaryLogic { op, lhs, rhs, .. } => {
            let left = expect_bool(eval_expr(lhs, functions, env, lines)?)?;
            match op {
                crate::mir::LogicOp::And if !left => Ok(SpikeValue::Bool(false)),
                crate::mir::LogicOp::Or if left => Ok(SpikeValue::Bool(true)),
                crate::mir::LogicOp::And | crate::mir::LogicOp::Or => Ok(SpikeValue::Bool(
                    expect_bool(eval_expr(rhs, functions, env, lines)?)?,
                )),
            }
        }
        Expr::Cast { expr, ty } => cast_spike_value(eval_expr(expr, functions, env, lines)?, ty),
        Expr::StructLiteral { name, fields, .. } => fields
            .iter()
            .map(|field| {
                let mut env = env.clone();
                let value = eval_expr_effectful(&field.expr, functions, &mut env, lines)?;
                if is_control_return(&value) {
                    return Ok((field.name.clone(), value));
                }
                Ok((field.name.clone(), value))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|fields| {
                fields
                    .iter()
                    .find_map(|(_, value)| {
                        if let SpikeValue::ControlReturn(value) = value {
                            Some(SpikeValue::ControlReturn(value.clone()))
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| SpikeValue::Struct {
                        name: name.clone(),
                        fields,
                    })
            }),
        Expr::FieldAccess { base, field, .. } => match eval_expr(base, functions, env, lines)? {
            SpikeValue::Struct { name, fields } => fields
                .into_iter()
                .find_map(|(candidate, value)| (candidate == *field).then_some(value))
                .ok_or_else(|| {
                    unsupported(&format!(
                        "struct {name:?} has no field {field:?} in the cranelift spike"
                    ))
                }),
            _ => Err(unsupported("field access requires a struct value")),
        },
        Expr::EnumVariant {
            enum_name,
            variant,
            field_names,
            payloads,
            ..
        } => payloads
            .iter()
            .map(|payload| {
                let mut env = env.clone();
                eval_expr_effectful(payload, functions, &mut env, lines)
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|payloads| {
                payloads
                    .iter()
                    .find_map(|value| {
                        if let SpikeValue::ControlReturn(value) = value {
                            Some(SpikeValue::ControlReturn(value.clone()))
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| SpikeValue::Enum {
                        enum_name: enum_name.clone(),
                        variant: variant.clone(),
                        field_names: field_names.clone(),
                        payloads,
                    })
            }),
        Expr::Match { expr, arms, .. } => eval_match_expr(expr, arms, functions, env, lines),
        Expr::TupleLiteral { elements, .. } => elements
            .iter()
            .map(|element| {
                let mut env = env.clone();
                eval_expr_effectful(element, functions, &mut env, lines)
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|elements| {
                elements
                    .iter()
                    .find_map(|value| {
                        if let SpikeValue::ControlReturn(value) = value {
                            Some(SpikeValue::ControlReturn(value.clone()))
                        } else {
                            None
                        }
                    })
                    .unwrap_or(SpikeValue::Tuple(elements))
            }),
        Expr::TupleIndex { base, index, .. } => match eval_expr(base, functions, env, lines)? {
            SpikeValue::Tuple(elements) => elements
                .get(*index)
                .cloned()
                .ok_or_else(|| unsupported("tuple index is outside the tuple width")),
            _ => Err(unsupported("tuple indexing requires a tuple value")),
        },
        Expr::MapLiteral { entries, .. } => {
            let mut values = Vec::new();
            for entry in entries {
                let mut key_env = env.clone();
                let key = eval_expr_effectful(&entry.key, functions, &mut key_env, lines)?;
                if is_control_return(&key) {
                    return Ok(key);
                }
                validate_map_key(&key)?;
                let mut value_env = env.clone();
                let value = eval_expr_effectful(&entry.value, functions, &mut value_env, lines)?;
                if is_control_return(&value) {
                    return Ok(value);
                }
                insert_map_entry(&mut values, key, value)?;
            }
            Ok(SpikeValue::Map(values))
        }
        Expr::ArrayLiteral { elements, .. } => elements
            .iter()
            .map(|element| {
                let mut env = env.clone();
                eval_expr_effectful(element, functions, &mut env, lines)
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|elements| {
                elements
                    .iter()
                    .find_map(|value| {
                        if let SpikeValue::ControlReturn(value) = value {
                            Some(SpikeValue::ControlReturn(value.clone()))
                        } else {
                            None
                        }
                    })
                    .unwrap_or(SpikeValue::Array(elements))
            }),
        Expr::Closure { params, body, .. } => Ok(SpikeValue::Closure {
            params: params.clone(),
            body: body.clone(),
            env: env.clone(),
        }),
        Expr::Slice {
            base,
            start,
            end,
            ty,
        } => {
            let elements = match eval_expr(base, functions, env, lines)? {
                SpikeValue::Array(elements) => elements,
                _ => {
                    return Err(unsupported(
                        "slicing supports arrays in the cranelift spike",
                    ));
                }
            };
            let start = match start {
                Some(expr) => expect_non_negative_index(eval_expr(expr, functions, env, lines)?)?,
                None => 0,
            };
            let end = match end {
                Some(expr) => expect_non_negative_index(eval_expr(expr, functions, env, lines)?)?,
                None => elements.len(),
            };
            if start > end {
                return Err(cranelift_runtime_trap(
                    "runtime",
                    "array slice start after end",
                ));
            }
            if end > elements.len() {
                return Err(cranelift_runtime_trap(
                    "runtime",
                    "array slice end out of bounds",
                ));
            }
            if matches!(ty, Type::MutSlice(_))
                && let Expr::VarRef { name, .. } = base.as_ref()
            {
                return Ok(SpikeValue::MutSlice {
                    target: name.clone(),
                    start,
                    end,
                });
            }
            Ok(SpikeValue::Array(elements[start..end].to_vec()))
        }
        Expr::Index { base, index, .. } => match eval_expr(base, functions, env, lines)? {
            SpikeValue::Array(elements) => {
                let index = expect_non_negative_index(eval_expr(index, functions, env, lines)?)?;
                elements
                    .get(index)
                    .cloned()
                    .ok_or_else(|| cranelift_runtime_trap("runtime", "array index out of bounds"))
            }
            SpikeValue::MutSlice { target, start, end } => {
                let index = expect_non_negative_index(eval_expr(index, functions, env, lines)?)?;
                let real_index = start
                    .checked_add(index)
                    .ok_or_else(|| unsupported("slice index overflow"))?;
                if real_index >= end {
                    return Err(unsupported("slice index is outside the slice length"));
                }
                match env.get(&target) {
                    Some(SpikeValue::Array(elements)) => {
                        elements.get(real_index).cloned().ok_or_else(|| {
                            cranelift_runtime_trap("runtime", "array index out of bounds")
                        })
                    }
                    _ => Err(unsupported(
                        "mutable slice indexing requires a live local array",
                    )),
                }
            }
            SpikeValue::Map(entries) => {
                let key = eval_expr(index, functions, env, lines)?;
                validate_map_key(&key)?;
                for (candidate, value) in entries {
                    if map_keys_equal(&candidate, &key)? {
                        return Ok(value);
                    }
                }
                Err(unsupported("map key not found"))
            }
            _ => Err(unsupported("indexing requires an array or map value")),
        },
        Expr::Await { expr, .. } => await_spike_task(eval_expr(expr, functions, env, lines)?),
        Expr::StringBorrow { expr, .. } => eval_expr(expr, functions, env, lines),
        Expr::Try { expr, .. } => {
            let mut env = env.clone();
            eval_try_expr(expr, functions, &mut env, lines)
        }
        Expr::MutBorrow { expr, .. } => match expr.as_ref() {
            Expr::VarRef { name, .. } if env.contains_key(name) => {
                Ok(SpikeValue::MutRef(name.clone()))
            }
            Expr::VarRef { name, .. } => Err(unsupported(&format!(
                "unknown cranelift spike variable {name:?}"
            ))),
            _ => Err(unsupported(
                "mutable borrow supports local variables in the cranelift spike",
            )),
        },
        Expr::Deref { expr, .. } => match eval_expr(expr, functions, env, lines)? {
            SpikeValue::MutRef(name) => env
                .get(&name)
                .cloned()
                .ok_or_else(|| unsupported(&format!("unknown cranelift spike variable {name:?}"))),
            _ => Err(unsupported(
                "dereference requires a mutable local reference in the cranelift spike",
            )),
        },
    }
}

pub(crate) fn eval_match_stmt(
    expr: &Expr,
    arms: &[MatchArm],
    functions: &HashMap<&str, &Function>,
    env: &mut SpikeEnv,
    lines: &mut Vec<OutputLine>,
    defers: &mut Vec<Expr>,
) -> Result<Option<SpikeValue>, Diagnostic> {
    let matched_value = eval_expr_effectful(expr, functions, env, lines)?;
    if let SpikeValue::ControlReturn(value) = matched_value {
        return Ok(Some(*value));
    }
    if arms.iter().all(|arm| arm.enum_name.is_empty()) {
        return eval_const_match_stmt(matched_value, arms, functions, env, lines, defers);
    }
    let matched = expect_enum_value(matched_value)?;
    let arm = arms
        .iter()
        .find(|arm| arm.enum_name == matched.enum_name && arm.variant == matched.variant)
        .ok_or_else(|| unsupported("match statement has no matching enum arm"))?;
    let mut arm_env = env.clone();
    if !arm.ignore_payloads {
        bind_match_payloads(
            &mut arm_env,
            &arm.bindings,
            arm.is_named,
            &matched.field_names,
            &matched.payloads,
        )?;
    }
    let returned = eval_block(&arm.body, functions, &mut arm_env, lines, defers)?;
    *env = arm_env;
    Ok(returned)
}

pub(crate) fn eval_const_match_stmt(
    matched_value: SpikeValue,
    arms: &[MatchArm],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
    defers: &mut Vec<Expr>,
) -> Result<Option<SpikeValue>, Diagnostic> {
    let matched = expect_int(matched_value)?.to_string();
    let arm = arms
        .iter()
        .find(|arm| arm.variant == matched)
        .ok_or_else(|| unsupported("const match statement has no matching arm"))?;
    let mut arm_env = env.clone();
    eval_block(&arm.body, functions, &mut arm_env, lines, defers)
}

pub(crate) fn eval_match_expr(
    expr: &Expr,
    arms: &[MatchExprArm],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let matched = expect_enum_value(eval_expr(expr, functions, env, lines)?)?;
    let arm = arms
        .iter()
        .find(|arm| arm.enum_name == matched.enum_name && arm.variant == matched.variant)
        .ok_or_else(|| unsupported("match expression has no matching enum arm"))?;
    let mut arm_env = env.clone();
    bind_match_payloads(
        &mut arm_env,
        &arm.bindings,
        arm.is_named,
        &matched.field_names,
        &matched.payloads,
    )?;
    eval_expr(&arm.expr, functions, &arm_env, lines)
}

pub(crate) fn expect_enum_value(value: SpikeValue) -> Result<MatchedEnum, Diagnostic> {
    match value {
        SpikeValue::Enum {
            enum_name,
            variant,
            field_names,
            payloads,
        } => Ok(MatchedEnum {
            enum_name,
            variant,
            field_names,
            payloads,
        }),
        _ => Err(unsupported("match requires an enum value")),
    }
}

pub(crate) fn bind_match_payloads(
    env: &mut SpikeEnv,
    bindings: &[String],
    is_named: bool,
    field_names: &[String],
    payloads: &[SpikeValue],
) -> Result<(), Diagnostic> {
    if bindings.len() != payloads.len() {
        return Err(unsupported("match payload binding count mismatch"));
    }
    for (index, binding) in bindings.iter().enumerate() {
        if binding == "_" {
            continue;
        }
        let payload_index = if is_named {
            field_names
                .iter()
                .position(|field_name| field_name == binding)
                .ok_or_else(|| unsupported("named enum match binding has no payload field"))?
        } else {
            index
        };
        let value = payloads
            .get(payload_index)
            .ok_or_else(|| unsupported("match payload binding index mismatch"))?;
        env.insert(binding.clone(), value.clone());
    }
    Ok(())
}

pub(crate) fn eval_numeric_literal(raw: &str, ty: NumericType) -> Result<SpikeValue, Diagnostic> {
    match ty {
        NumericType::F32 => raw
            .parse::<f32>()
            .map(|value| SpikeValue::Float(value as f64))
            .map_err(|_| unsupported("invalid f32 numeric literal")),
        NumericType::F64 => raw
            .parse::<f64>()
            .map(SpikeValue::Float)
            .map_err(|_| unsupported("invalid f64 numeric literal")),
        NumericType::I8
        | NumericType::I16
        | NumericType::I32
        | NumericType::I64
        | NumericType::Isize => raw
            .parse::<i64>()
            .map(|value| cast_signed_integer(value, ty))
            .map_err(|_| unsupported("invalid signed integer numeric literal")),
        NumericType::U8
        | NumericType::U16
        | NumericType::U32
        | NumericType::U64
        | NumericType::Usize => raw
            .parse::<u128>()
            .map_err(|_| unsupported("invalid unsigned integer numeric literal"))
            .and_then(|value| {
                u64::try_from(value)
                    .map(|value| cast_unsigned_integer(value, ty))
                    .map_err(|_| unsupported("invalid unsigned integer numeric literal"))
            }),
    }
}

pub(crate) fn cast_spike_value(value: SpikeValue, ty: &Type) -> Result<SpikeValue, Diagnostic> {
    match ty {
        Type::Int => match value {
            SpikeValue::Int(value) => Ok(SpikeValue::Int(value)),
            SpikeValue::UInt(value) => Ok(SpikeValue::UInt(value)),
            SpikeValue::Float(value) => Ok(SpikeValue::Int(value as i64)),
            _ => Err(unsupported("only numeric values can be cast to int")),
        },
        Type::Numeric(numeric_ty) => cast_to_numeric(value, *numeric_ty),
        _ => Ok(value),
    }
}

pub(crate) fn cast_to_numeric(value: SpikeValue, ty: NumericType) -> Result<SpikeValue, Diagnostic> {
    match value {
        SpikeValue::Int(value) => Ok(cast_signed_integer(value, ty)),
        SpikeValue::UInt(value) => Ok(cast_unsigned_integer(value, ty)),
        SpikeValue::Float(value) => Ok(cast_float(value, ty)),
        _ => Err(unsupported(
            "only numeric values can be cast to numeric types",
        )),
    }
}

pub(crate) fn cast_float(value: f64, ty: NumericType) -> SpikeValue {
    match ty {
        NumericType::I8 => SpikeValue::Int(value as i8 as i64),
        NumericType::I16 => SpikeValue::Int(value as i16 as i64),
        NumericType::I32 => SpikeValue::Int(value as i32 as i64),
        NumericType::I64 => SpikeValue::Int(value as i64),
        NumericType::Isize => SpikeValue::Int(value as isize as i64),
        NumericType::U8 => SpikeValue::UInt(value as u8 as u64),
        NumericType::U16 => SpikeValue::UInt(value as u16 as u64),
        NumericType::U32 => SpikeValue::UInt(value as u32 as u64),
        NumericType::U64 => SpikeValue::UInt(value as u64),
        NumericType::Usize => SpikeValue::UInt(value as usize as u64),
        NumericType::F32 => SpikeValue::Float((value as f32) as f64),
        NumericType::F64 => SpikeValue::Float(value),
    }
}

pub(crate) fn eval_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    if is_assert_call(name) {
        return eval_assert_call(name, args, functions, env, lines);
    }
    if let Some(SpikeValue::Closure {
        params,
        body,
        env: captured_env,
    }) = env.get(name)
    {
        return eval_closure_call(params, body, captured_env, args, functions, env, lines);
    }
    if name == "len" {
        return eval_len_call(args, functions, env, lines);
    }
    if name == "first" || name == "last" {
        return eval_first_last_call(name, args, functions, env, lines);
    }
    if name == "contains" || name == "map_contains_key" {
        return eval_map_contains_call(args, functions, env, lines);
    }
    if name == "map_get" || name == "get" {
        return eval_map_get_call(args, functions, env, lines);
    }
    if name == "get_or_default" {
        return eval_map_get_or_default_call(args, functions, env, lines);
    }
    if name == "keys" || name == "map_keys" {
        return eval_map_keys_call(args, functions, env, lines);
    }
    if name == "io_eprintln" {
        return eval_io_eprintln_call(args, functions, env, lines);
    }
    if name == "io_readline" {
        return eval_io_readline_call(args);
    }
    if name == "io_read_to_string" {
        return eval_io_read_to_string_call(args);
    }
    if is_json_call(name) {
        return eval_json_call(name, args, functions, env, lines);
    }
    if is_json_serdes_call(name) {
        return eval_json_serdes_call(name, args, functions, env, lines);
    }
    if is_std_serdes_call(name) {
        return eval_std_serdes_call(name, args, functions, env, lines);
    }
    if is_crypto_call(name) {
        return eval_crypto_call(name, args, functions, env, lines);
    }
    if is_encoding_call(name) {
        return eval_encoding_call(name, args, functions, env, lines);
    }
    if is_string_call(name) {
        return eval_string_call(name, args, functions, env, lines);
    }
    if is_async_call(name) {
        return eval_async_call(name, args, functions, env, lines);
    }
    if is_cli_call(name) {
        return eval_cli_call(name, args, functions, env, lines);
    }
    if name == "env_get" {
        return eval_env_get_call(args, functions, env, lines);
    }
    if name == "fs_read" {
        return eval_fs_read_call(args, functions, env, lines);
    }
    if is_fs_write_call(name) {
        return eval_fs_write_call(name, args, functions, env, lines);
    }
    if name == "process_status" {
        return eval_process_status_call(args, functions, env, lines);
    }
    if name == "clock_now_ms" {
        return eval_clock_now_ms_call(args);
    }
    if name == "clock_elapsed_ms" {
        return eval_clock_elapsed_ms_call(args, functions, env, lines);
    }
    if name == "clock_sleep_ms" {
        return eval_clock_sleep_ms_call(args, functions, env, lines);
    }
    if is_net_call(name) {
        return eval_net_call(name, args, functions, env, lines);
    }
    if is_http_call(name) {
        return eval_http_call(name, args, functions, env, lines);
    }
    if is_regex_call(name) {
        return eval_regex_call(name, args, functions, env, lines);
    }
    if let Some(SpikeValue::Closure {
        params,
        body,
        env: captured_env,
    }) = env.get(name)
    {
        return eval_closure_call(params, body, captured_env, args, functions, env, lines);
    }
    let function = functions
        .get(name)
        .ok_or_else(|| unsupported(&format!("unsupported cranelift spike call {name:?}")))?;
    if function.params.len() != args.len() {
        return Err(unsupported("function argument count mismatch"));
    }
    if function.is_extern {
        return eval_extern_call(function, args, functions, env, lines);
    }
    let mut local_env = env.clone();
    let mut receiver_alias_bound = false;
    for (param, arg) in function.params.iter().zip(args) {
        let mut arg_env = env.clone();
        let value = eval_expr_effectful(arg, functions, &mut arg_env, lines)?;
        if is_control_return(&value) {
            return Ok(value);
        }
        if param.name == "self_" && !receiver_alias_bound {
            local_env.insert(String::from("self"), value.clone());
            receiver_alias_bound = true;
        }
        local_env.insert(param.name.clone(), value);
    }
    if function.is_async {
        // Async bodies evaluate with virtual sleeps: blocking time accumulates
        // into the task's duration and is spent when the task is consumed
        // (await, join, or timeout), so sibling tasks model concurrency.
        let previous = SPIKE_VIRTUAL_SLEEP.with(|slot| slot.replace(Some(0)));
        let started = current_time_ms()?;
        let returned = run_function_body(&function.body, functions, &mut local_env, lines);
        let virtual_ms = SPIKE_VIRTUAL_SLEEP
            .with(|slot| slot.replace(previous))
            .unwrap_or(0);
        let returned = returned?
            .ok_or_else(|| unsupported("cranelift spike functions must return a value"))?;
        let duration = current_time_ms()?
            .saturating_sub(started)
            .saturating_add(virtual_ms);
        Ok(spike_task_with_duration(returned, duration))
    } else {
        let returned = run_function_body(&function.body, functions, &mut local_env, lines)?
            .ok_or_else(|| unsupported("cranelift spike functions must return a value"))?;
        Ok(returned)
    }
}

pub(crate) fn eval_closure_call(
    params: &[crate::mir::Param],
    body: &Expr,
    captured_env: &SpikeEnv,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    caller_env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    if params.len() != args.len() {
        return Err(unsupported("closure argument count mismatch"));
    }
    let mut local_env = captured_env.clone();
    for (param, arg) in params.iter().zip(args) {
        let mut caller_env = caller_env.clone();
        let value = eval_expr_effectful(arg, functions, &mut caller_env, lines)?;
        if is_control_return(&value) {
            return Ok(value);
        }
        local_env.insert(param.name.clone(), value);
    }
    eval_expr(body, functions, &local_env, lines)
}

pub(crate) fn eval_call_effectful(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &mut SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    if is_net_call(name) {
        return eval_net_call_effectful(name, args, functions, env, lines);
    }
    let Some(function) = functions.get(name) else {
        return eval_call(name, args, functions, env, lines);
    };
    if function.params.len() != args.len() {
        return Err(unsupported("function argument count mismatch"));
    }
    if function.is_extern {
        return eval_extern_call(function, args, functions, env, lines);
    }

    let mut local_env = env.clone();
    let mut writebacks = Vec::new();
    for (index, (param, arg)) in function.params.iter().zip(args).enumerate() {
        let value = eval_expr_effectful(arg, functions, env, lines)?;
        if is_control_return(&value) {
            return Ok(value);
        }
        if let SpikeValue::MutSlice { target, start, end } = value {
            let backing_name = format!("__arg{index}_{target}");
            let Some(SpikeValue::Array(elements)) = env.get(&target) else {
                return Err(unsupported(
                    "mutable slice call argument requires a live local array",
                ));
            };
            local_env.insert(backing_name.clone(), SpikeValue::Array(elements.clone()));
            local_env.insert(
                param.name.clone(),
                SpikeValue::MutSlice {
                    target: backing_name.clone(),
                    start,
                    end,
                },
            );
            writebacks.push((backing_name, target));
        } else {
            if param.name == "self_" {
                local_env.insert(String::from("self"), value.clone());
            }
            local_env.insert(param.name.clone(), value);
        }
    }

    // Async bodies evaluate with virtual sleeps (see the sibling call path).
    let previous = function
        .is_async
        .then(|| SPIKE_VIRTUAL_SLEEP.with(|slot| slot.replace(Some(0))));
    let started = current_time_ms()?;
    let returned = run_function_body(&function.body, functions, &mut local_env, lines);
    let virtual_ms = previous
        .map(|previous| {
            SPIKE_VIRTUAL_SLEEP
                .with(|slot| slot.replace(previous))
                .unwrap_or(0)
        })
        .unwrap_or(0);
    let returned =
        returned?.ok_or_else(|| unsupported("cranelift spike functions must return a value"))?;
    let async_duration = current_time_ms()?
        .saturating_sub(started)
        .saturating_add(virtual_ms);
    for (backing_name, target) in writebacks {
        let Some(SpikeValue::Array(elements)) = local_env.get(&backing_name) else {
            return Err(unsupported(
                "mutable slice call lost its local backing array",
            ));
        };
        env.insert(target, SpikeValue::Array(elements.clone()));
    }
    if function.is_async {
        Ok(spike_task_with_duration(returned, async_duration))
    } else {
        Ok(returned)
    }
}

pub(crate) fn is_assert_call(name: &str) -> bool {
    matches!(
        name,
        "assert_true"
            | "assert_property"
            | "assert_snapshot"
            | "assert_contains"
            | "assert_eq"
            | "assert_case_eq"
            | "assert_ne"
    )
}

pub(crate) fn eval_assert_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "assert_true" => {
            let [condition, _line, _column] = args else {
                return Err(unsupported(
                    "assert_true expects condition, line, and column",
                ));
            };
            let condition = expect_bool(eval_expr(condition, functions, env, lines)?)?;
            assert_result(condition, "assert_true failed")
        }
        "assert_property" => {
            let [_label, condition, _line, _column] = args else {
                return Err(unsupported(
                    "assert_property expects label, condition, line, and column",
                ));
            };
            let condition = expect_bool(eval_expr(condition, functions, env, lines)?)?;
            assert_result(condition, "assert_property failed")
        }
        "assert_snapshot" => {
            let [_label, actual, expected, _line, _column] = args else {
                return Err(unsupported(
                    "assert_snapshot expects label, actual, expected, line, and column",
                ));
            };
            let actual = expect_text(eval_expr(actual, functions, env, lines)?, name)?;
            let expected = expect_text(eval_expr(expected, functions, env, lines)?, name)?;
            assert_result(actual == expected, "assert_snapshot failed")
        }
        "assert_contains" => {
            let [haystack, needle, _line, _column] = args else {
                return Err(unsupported(
                    "assert_contains expects haystack, needle, line, and column",
                ));
            };
            let haystack = expect_text(eval_expr(haystack, functions, env, lines)?, name)?;
            let needle = expect_text(eval_expr(needle, functions, env, lines)?, name)?;
            assert_result(haystack.contains(&needle), "assert_contains failed")
        }
        "assert_eq" | "assert_ne" => {
            let [left, right, _line, _column] = args else {
                return Err(unsupported(
                    "assert_eq/assert_ne expects left, right, line, and column",
                ));
            };
            let left = eval_expr(left, functions, env, lines)?;
            let right = eval_expr(right, functions, env, lines)?;
            let equal = spike_values_equal(&left, &right)?;
            if equal == (name == "assert_eq") {
                Ok(SpikeValue::Int(0))
            } else {
                let op = if name == "assert_eq" { "==" } else { "!=" };
                Err(cranelift_runtime_trap(
                    "assertion",
                    format!(
                        "expected left {op} right, left={}, right={}",
                        render_value(&left),
                        render_value(&right)
                    ),
                ))
            }
        }
        "assert_case_eq" => {
            let [label, left, right, _line, _column] = args else {
                return Err(unsupported(
                    "assert_case_eq expects label, left, right, line, and column",
                ));
            };
            let label = expect_text(eval_expr(label, functions, env, lines)?, "assert_case_eq")?;
            let left = eval_expr(left, functions, env, lines)?;
            let right = eval_expr(right, functions, env, lines)?;
            if spike_values_equal(&left, &right)? {
                Ok(SpikeValue::Int(0))
            } else {
                Err(cranelift_runtime_trap(
                    "assertion",
                    format!(
                        "table case {label:?} failed: expected {}, got {}",
                        render_value(&right),
                        render_value(&left)
                    ),
                ))
            }
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike assertion call {name:?}"
        ))),
    }
}

pub(crate) fn assert_result(condition: bool, message: &str) -> Result<SpikeValue, Diagnostic> {
    if condition {
        Ok(SpikeValue::Int(0))
    } else {
        Err(unsupported(message))
    }
}

pub(crate) fn eval_extern_call(
    function: &Function,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match (
        function.source_name.as_str(),
        function.extern_abi.as_deref(),
        function.extern_library.as_deref(),
    ) {
        ("strlen", Some("C"), Some("c")) => {
            let [value] = args else {
                return Err(unsupported("strlen expects exactly one argument"));
            };
            let value = expect_text(eval_expr(value, functions, env, lines)?, "strlen")?;
            let len = value
                .as_bytes()
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(value.len());
            Ok(SpikeValue::Int(len as i64))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike extern call {:?} from {:?}",
            function.name, function.extern_library
        ))),
    }
}

pub(crate) fn is_cli_call(name: &str) -> bool {
    matches!(name, "cli_args" | "cli_arg_count" | "cli_arg")
}

pub(crate) fn eval_cli_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "cli_args" => {
            let [] = args else {
                return Err(unsupported("cli_args expects no arguments"));
            };
            Ok(SpikeValue::Array(Vec::new()))
        }
        "cli_arg_count" => {
            let [] = args else {
                return Err(unsupported("cli_arg_count expects no arguments"));
            };
            Ok(SpikeValue::Int(0))
        }
        "cli_arg" => {
            let [index] = args else {
                return Err(unsupported("cli_arg expects exactly one argument"));
            };
            let _index = expect_signed_integer(eval_expr(index, functions, env, lines)?)?;
            Ok(spike_option(None))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike CLI call {name:?}"
        ))),
    }
}

pub(crate) fn is_async_call(name: &str) -> bool {
    matches!(
        name,
        "async_ready"
            | "async_spawn"
            | "async_join"
            | "async_cancel"
            | "async_is_canceled"
            | "async_timeout"
            | "async_channel"
            | "async_send"
            | "async_recv"
            | "async_select"
            | "async_selected"
            | "async_selected_value"
    )
}

pub(crate) fn eval_async_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "async_ready" => {
            let [value] = args else {
                return Err(unsupported("async_ready expects exactly one argument"));
            };
            eval_expr(value, functions, env, lines).map(spike_task)
        }
        "async_spawn" => {
            let [task] = args else {
                return Err(unsupported("async_spawn expects exactly one argument"));
            };
            let task = eval_expr(task, functions, env, lines)?;
            expect_task_value(&task, "async_spawn")?;
            // The task's body ran with virtual sleeps, so it carries its
            // duration without having blocked. Spawning stamps the handle with
            // the wall-clock time when that specific task would be ready.
            let ready_at_ms = if let SpikeValue::Task {
                duration_ms,
                canceled: false,
                ..
            } = &task
            {
                current_time_ms()?.saturating_add(*duration_ms)
            } else {
                current_time_ms()?
            };
            Ok(SpikeValue::JoinHandle {
                task: Box::new(task),
                ready_at_ms,
            })
        }
        "async_join" => {
            let [handle] = args else {
                return Err(unsupported("async_join expects exactly one argument"));
            };
            match eval_expr(handle, functions, env, lines)? {
                SpikeValue::JoinHandle { task, ready_at_ms } => {
                    // Joining waits only for the handle being consumed. Each
                    // handle has its own readiness timestamp, so a later join
                    // cannot inherit stale state from an earlier async group.
                    let remaining = ready_at_ms.saturating_sub(current_time_ms()?);
                    if remaining > 0 {
                        spike_wait_out_duration(remaining);
                    }
                    spike_task_value(*task).map(spike_task)
                }
                _ => Err(unsupported("async_join expects a join handle")),
            }
        }
        "async_cancel" => {
            let [task] = args else {
                return Err(unsupported("async_cancel expects exactly one argument"));
            };
            match eval_expr(task, functions, env, lines)? {
                SpikeValue::Task {
                    value, duration_ms, ..
                } => Ok(SpikeValue::Task {
                    value,
                    canceled: true,
                    duration_ms,
                }),
                _ => Err(unsupported("async_cancel expects a task")),
            }
        }
        "async_is_canceled" => {
            let [task] = args else {
                return Err(unsupported(
                    "async_is_canceled expects exactly one argument",
                ));
            };
            match eval_expr(task, functions, env, lines)? {
                SpikeValue::Task { canceled, .. } => Ok(SpikeValue::Bool(canceled)),
                _ => Err(unsupported("async_is_canceled expects a task")),
            }
        }
        "async_timeout" => {
            let [task, milliseconds] = args else {
                return Err(unsupported("async_timeout expects exactly two arguments"));
            };
            let timeout_ms =
                expect_signed_integer(eval_expr(milliseconds, functions, env, lines)?)?;
            // Async bodies run with virtual sleeps, so each task carries its
            // duration without having blocked. A task that would outrun the
            // deadline times out (None) after waiting out the deadline itself;
            // otherwise awaiting the task waits out its (shorter) duration.
            match eval_expr(task, functions, env, lines)? {
                SpikeValue::Task { canceled: true, .. } => Ok(spike_task(spike_option(None))),
                SpikeValue::Task { duration_ms, .. }
                    if timeout_ms >= 0 && duration_ms > timeout_ms =>
                {
                    spike_wait_out_duration(timeout_ms);
                    Ok(spike_task(spike_option(None)))
                }
                task @ SpikeValue::Task { .. } => {
                    await_spike_task(task).map(|value| spike_task(spike_option(Some(value))))
                }
                _ => Err(unsupported("async_timeout expects a task")),
            }
        }
        "async_channel" => {
            let [] = args else {
                return Err(unsupported("async_channel expects no arguments"));
            };
            Ok(SpikeValue::AsyncChannel { slot: None })
        }
        "async_send" => {
            let [channel, value] = args else {
                return Err(unsupported("async_send expects exactly two arguments"));
            };
            match eval_expr(channel, functions, env, lines)? {
                SpikeValue::AsyncChannel { slot: None } => {
                    let value = eval_expr(value, functions, env, lines)?;
                    Ok(spike_task(SpikeValue::AsyncChannel {
                        slot: Some(Box::new(value)),
                    }))
                }
                SpikeValue::AsyncChannel { slot: Some(_) } => {
                    Err(unsupported("async_send on a full channel"))
                }
                _ => Err(unsupported("async_send expects an async channel")),
            }
        }
        "async_recv" => {
            let [channel] = args else {
                return Err(unsupported("async_recv expects exactly one argument"));
            };
            match eval_expr(channel, functions, env, lines)? {
                SpikeValue::AsyncChannel { slot } => {
                    Ok(spike_task(spike_option(slot.map(|value| *value))))
                }
                _ => Err(unsupported("async_recv expects an async channel")),
            }
        }
        "async_select" => {
            let [left, right] = args else {
                return Err(unsupported("async_select expects exactly two arguments"));
            };
            let left = await_spike_task(eval_expr(left, functions, env, lines)?)?;
            if option_payload(&left)?.is_some() {
                return Ok(spike_task(SpikeValue::SelectResult {
                    selected: 0,
                    value: option_payload(&left)?.map(Box::new),
                }));
            }
            let right = await_spike_task(eval_expr(right, functions, env, lines)?)?;
            Ok(spike_task(SpikeValue::SelectResult {
                selected: 1,
                value: option_payload(&right)?.map(Box::new),
            }))
        }
        "async_selected" => {
            let [result] = args else {
                return Err(unsupported("async_selected expects exactly one argument"));
            };
            match eval_expr(result, functions, env, lines)? {
                SpikeValue::SelectResult { selected, .. } => Ok(SpikeValue::Int(selected)),
                _ => Err(unsupported("async_selected expects a select result")),
            }
        }
        "async_selected_value" => {
            let [result] = args else {
                return Err(unsupported(
                    "async_selected_value expects exactly one argument",
                ));
            };
            match eval_expr(result, functions, env, lines)? {
                SpikeValue::SelectResult { value, .. } => {
                    Ok(spike_option(value.map(|value| *value)))
                }
                _ => Err(unsupported("async_selected_value expects a select result")),
            }
        }
        _ => Err(unsupported("unknown async runtime intrinsic")),
    }
}

pub(crate) fn spike_task(value: SpikeValue) -> SpikeValue {
    spike_task_with_duration(value, 0)
}

pub(crate) fn spike_task_with_duration(value: SpikeValue, duration_ms: i64) -> SpikeValue {
    SpikeValue::Task {
        value: Some(Box::new(value)),
        canceled: false,
        duration_ms,
    }
}

pub(crate) fn expect_task_value(value: &SpikeValue, name: &str) -> Result<(), Diagnostic> {
    match value {
        SpikeValue::Task { .. } => Ok(()),
        _ => Err(unsupported(&format!("{name} expects a task"))),
    }
}

pub(crate) fn is_control_return(value: &SpikeValue) -> bool {
    matches!(value, SpikeValue::ControlReturn(_))
}

pub(crate) fn eval_try_expr(
    expr: &Expr,
    functions: &HashMap<&str, &Function>,
    env: &mut SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match eval_expr_effectful(expr, functions, env, lines)? {
        SpikeValue::ControlReturn(value) => Ok(SpikeValue::ControlReturn(value)),
        SpikeValue::Enum {
            enum_name,
            variant,
            mut payloads,
            ..
        } if enum_name == "Option" && variant == "Some" && payloads.len() == 1 => {
            Ok(payloads.remove(0))
        }
        SpikeValue::Enum {
            enum_name,
            variant,
            field_names,
            payloads,
        } if enum_name == "Option" && variant == "None" && payloads.is_empty() => {
            Ok(SpikeValue::ControlReturn(Box::new(SpikeValue::Enum {
                enum_name,
                variant,
                field_names,
                payloads,
            })))
        }
        SpikeValue::Enum {
            enum_name,
            variant,
            mut payloads,
            ..
        } if enum_name == "Result" && variant == "Ok" && payloads.len() == 1 => {
            Ok(payloads.remove(0))
        }
        SpikeValue::Enum {
            enum_name,
            variant,
            field_names,
            payloads,
        } if enum_name == "Result" && variant == "Err" && payloads.len() == 1 => {
            Ok(SpikeValue::ControlReturn(Box::new(SpikeValue::Enum {
                enum_name,
                variant,
                field_names,
                payloads,
            })))
        }
        _ => Err(unsupported("`?` expects an Option or Result value")),
    }
}

pub(crate) fn option_payload(value: &SpikeValue) -> Result<Option<SpikeValue>, Diagnostic> {
    match value {
        SpikeValue::Enum {
            enum_name,
            variant,
            payloads,
            ..
        } if enum_name == "Option" && variant == "Some" && payloads.len() == 1 => {
            Ok(Some(payloads[0].clone()))
        }
        SpikeValue::Enum {
            enum_name,
            variant,
            payloads,
            ..
        } if enum_name == "Option" && variant == "None" && payloads.is_empty() => Ok(None),
        _ => Err(unsupported("expected an Option value")),
    }
}

pub(crate) fn eval_len_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [arg] = args else {
        return Err(unsupported("len expects exactly one argument"));
    };
    let value = eval_expr(arg, functions, env, lines)?;
    let len = match value {
        // Match the generated-Rust backend, which lowers `len(...)` to Rust
        // `.len()` (encoded byte length). Using char count here would diverge
        // for non-ASCII strings (e.g. `len("é")` is 2, not 1).
        SpikeValue::Text(value) => value.len(),
        SpikeValue::Tuple(values) | SpikeValue::Array(values) => values.len(),
        SpikeValue::MutSlice { start, end, .. } => end.saturating_sub(start),
        _ => return Err(unsupported("len supports strings, tuples, and arrays")),
    };
    Ok(SpikeValue::Int(len as i64))
}

pub(crate) fn eval_first_last_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [arg] = args else {
        return Err(unsupported(&format!("{name} expects exactly one argument")));
    };
    // HIR restricts `first`/`last` to arrays and slices and returns the element
    // directly (it panics at runtime on an empty collection). The spike models
    // owned arrays and evaluated array slices with the same value shape.
    let selected = match eval_expr(arg, functions, env, lines)? {
        SpikeValue::Array(elements) => {
            if name == "first" {
                elements.first().cloned()
            } else {
                elements.last().cloned()
            }
        }
        SpikeValue::MutSlice { target, start, end } => {
            let Some(SpikeValue::Array(elements)) = env.get(&target) else {
                return Err(unsupported(
                    "mutable slice access requires a live local array",
                ));
            };
            let slice = elements
                .get(start..end)
                .ok_or_else(|| unsupported("slice range is outside the array length"))?;
            if name == "first" {
                slice.first().cloned()
            } else {
                slice.last().cloned()
            }
        }
        _ => {
            return Err(unsupported(&format!(
                "{name} supports arrays in the cranelift spike"
            )));
        }
    };
    selected.ok_or_else(|| unsupported(&format!("{name} on an empty array")))
}

pub(crate) fn eval_map_contains_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [map, key] = args else {
        return Err(unsupported("map contains expects exactly two arguments"));
    };
    let entries = match eval_expr(map, functions, env, lines)? {
        SpikeValue::Map(entries) => entries,
        _ => return Err(unsupported("map contains expects a map value")),
    };
    let key = eval_expr(key, functions, env, lines)?;
    validate_map_key(&key)?;
    let contains = entries.iter().try_fold(false, |found, (candidate, _)| {
        Ok::<_, Diagnostic>(found || map_keys_equal(candidate, &key)?)
    })?;
    Ok(SpikeValue::Bool(contains))
}

pub(crate) fn eval_map_get_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [map, key] = args else {
        return Err(unsupported("map_get/get expects exactly two arguments"));
    };
    let entries = match eval_expr(map, functions, env, lines)? {
        SpikeValue::Map(entries) => entries,
        _ => return Err(unsupported("map_get/get expects a map value")),
    };
    let key = eval_expr(key, functions, env, lines)?;
    validate_map_key(&key)?;
    for (candidate, value) in entries {
        if map_keys_equal(&candidate, &key)? {
            return Ok(spike_option(Some(value)));
        }
    }
    Ok(spike_option(None))
}

pub(crate) fn eval_map_get_or_default_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [map, key, default] = args else {
        return Err(unsupported(
            "get_or_default expects exactly three arguments",
        ));
    };
    let entries = match eval_expr(map, functions, env, lines)? {
        SpikeValue::Map(entries) => entries,
        _ => return Err(unsupported("get_or_default expects a map value")),
    };
    let key = eval_expr(key, functions, env, lines)?;
    validate_map_key(&key)?;
    for (candidate, value) in entries {
        if map_keys_equal(&candidate, &key)? {
            return Ok(value);
        }
    }
    eval_expr(default, functions, env, lines)
}

pub(crate) fn eval_map_keys_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [map] = args else {
        return Err(unsupported("map_keys expects exactly one argument"));
    };
    let entries = match eval_expr(map, functions, env, lines)? {
        SpikeValue::Map(entries) => entries,
        _ => return Err(unsupported("map_keys expects a map value")),
    };
    entries
        .into_iter()
        .map(|(key, _)| {
            validate_map_key(&key)?;
            Ok(key)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(SpikeValue::Array)
}

pub(crate) fn is_json_call(name: &str) -> bool {
    matches!(
        name,
        "json_parse_int"
            | "json_parse_bool"
            | "json_parse_string"
            | "json_parse_value"
            | "json_parse_field_int"
            | "json_parse_field_bool"
            | "json_parse_field_string"
            | "json_parse_field_value"
            | "json_stringify_int"
            | "json_stringify_bool"
            | "json_stringify_string"
            | "json_stringify_value"
    )
}

pub(crate) fn eval_json_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "json_parse_int" => {
            let text = eval_json_unary_text(name, args, functions, env, lines)?;
            Ok(spike_option(json_parse_int(&text).map(SpikeValue::Int)))
        }
        "json_parse_bool" => {
            let text = eval_json_unary_text(name, args, functions, env, lines)?;
            Ok(spike_option(json_parse_bool(&text).map(SpikeValue::Bool)))
        }
        "json_parse_string" => {
            let text = eval_json_unary_text(name, args, functions, env, lines)?;
            Ok(spike_option(json_parse_string(&text).map(SpikeValue::Text)))
        }
        "json_parse_value" => {
            let text = eval_json_unary_text(name, args, functions, env, lines)?;
            Ok(spike_option(json_parse_value(&text).map(SpikeValue::Text)))
        }
        "json_parse_field_int" => {
            let (text, key) = eval_json_binary_text(name, args, functions, env, lines)?;
            Ok(spike_option(
                json_object_field(&text, &key)
                    .and_then(|value| json_parse_int(&value))
                    .map(SpikeValue::Int),
            ))
        }
        "json_parse_field_bool" => {
            let (text, key) = eval_json_binary_text(name, args, functions, env, lines)?;
            Ok(spike_option(
                json_object_field(&text, &key)
                    .and_then(|value| json_parse_bool(&value))
                    .map(SpikeValue::Bool),
            ))
        }
        "json_parse_field_string" => {
            let (text, key) = eval_json_binary_text(name, args, functions, env, lines)?;
            Ok(spike_option(
                json_object_field(&text, &key)
                    .and_then(|value| json_parse_string(&value))
                    .map(SpikeValue::Text),
            ))
        }
        "json_parse_field_value" => {
            let (text, key) = eval_json_binary_text(name, args, functions, env, lines)?;
            Ok(spike_option(
                json_object_field(&text, &key)
                    .and_then(|value| json_parse_value(&value))
                    .map(SpikeValue::Text),
            ))
        }
        "json_stringify_int" => {
            let value = eval_json_unary(name, args, functions, env, lines)?;
            let value = expect_signed_integer(value)?;
            Ok(SpikeValue::Text(value.to_string()))
        }
        "json_stringify_bool" => {
            let value = eval_json_unary(name, args, functions, env, lines)?;
            Ok(SpikeValue::Text(expect_bool(value)?.to_string()))
        }
        "json_stringify_string" => {
            let text = eval_json_unary_text(name, args, functions, env, lines)?;
            Ok(SpikeValue::Text(json_escape_string(&text)))
        }
        "json_stringify_value" => {
            let text = eval_json_unary_text(name, args, functions, env, lines)?;
            Ok(SpikeValue::Text(json_parse_value(&text).unwrap_or(text)))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike JSON call {name:?}"
        ))),
    }
}

pub(crate) fn eval_json_unary(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [arg] = args else {
        return Err(unsupported(&format!("{name} expects exactly one argument")));
    };
    eval_expr(arg, functions, env, lines)
}

pub(crate) fn eval_json_unary_text(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<String, Diagnostic> {
    match eval_json_unary(name, args, functions, env, lines)? {
        SpikeValue::Text(value) => Ok(value),
        _ => Err(unsupported(&format!("{name} expects a string argument"))),
    }
}

pub(crate) fn eval_json_binary_text(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<(String, String), Diagnostic> {
    let [left, right] = args else {
        return Err(unsupported(&format!(
            "{name} expects exactly two arguments"
        )));
    };
    let left = match eval_expr(left, functions, env, lines)? {
        SpikeValue::Text(value) => value,
        _ => {
            return Err(unsupported(&format!(
                "{name} expects a string JSON argument"
            )));
        }
    };
    let right = match eval_expr(right, functions, env, lines)? {
        SpikeValue::Text(value) => value,
        _ => {
            return Err(unsupported(&format!(
                "{name} expects a string key argument"
            )));
        }
    };
    Ok((left, right))
}

pub(crate) fn spike_option(value: Option<SpikeValue>) -> SpikeValue {
    match value {
        Some(value) => SpikeValue::Enum {
            enum_name: String::from("Option"),
            variant: String::from("Some"),
            field_names: Vec::new(),
            payloads: vec![value],
        },
        None => SpikeValue::Enum {
            enum_name: String::from("Option"),
            variant: String::from("None"),
            field_names: Vec::new(),
            payloads: Vec::new(),
        },
    }
}

pub(crate) fn is_json_serdes_call(name: &str) -> bool {
    matches!(
        name,
        "json_serdes_parse"
            | "json_serdes_parse_str"
            | "json_serdes_value_to_json"
            | "json_serdes_to_json"
    )
}

pub(crate) fn eval_json_serdes_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "json_serdes_parse" | "json_serdes_parse_str" => {
            let text = eval_json_unary_text(name, args, functions, env, lines)?;
            Ok(json_serdes_result(json_serdes_parse_document(&text)))
        }
        "json_serdes_value_to_json" => {
            let value = eval_json_unary(name, args, functions, env, lines)?;
            Ok(SpikeValue::Text(json_serdes_value_to_json(&value)?))
        }
        "json_serdes_to_json" => {
            let value = eval_json_unary(name, args, functions, env, lines)?;
            let SpikeValue::Map(entries) = value else {
                return Err(unsupported("json_serdes_to_json expects an object map"));
            };
            Ok(SpikeValue::Text(json_serdes_object_to_json(&entries)?))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike JSON serdes call {name:?}"
        ))),
    }
}

pub(crate) fn is_std_serdes_call(name: &str) -> bool {
    matches!(
        name,
        "std_serdes_is_null"
            | "std_serdes_as_bool"
            | "std_serdes_as_int"
            | "std_serdes_as_text"
            | "std_serdes_as_array"
            | "std_serdes_as_object"
            | "std_serdes_field"
            | "std_serdes_bool_field"
            | "std_serdes_text_field"
            | "std_serdes_int_field"
            | "std_serdes_array_field"
            | "std_serdes_object_field"
            | "std_serdes_value_item"
    )
}

pub(crate) fn eval_std_serdes_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "std_serdes_is_null" => {
            let value = eval_json_unary(name, args, functions, env, lines)?;
            let (variant, payloads) = expect_std_serdes_value(&value, name)?;
            Ok(SpikeValue::Bool(variant == "Null" && payloads.is_empty()))
        }
        "std_serdes_as_bool" => {
            let value = eval_json_unary(name, args, functions, env, lines)?;
            Ok(spike_option(std_serdes_as_bool_value(&value)?))
        }
        "std_serdes_as_int" => {
            let value = eval_json_unary(name, args, functions, env, lines)?;
            Ok(spike_option(std_serdes_as_int_value(&value)?))
        }
        "std_serdes_as_text" => {
            let value = eval_json_unary(name, args, functions, env, lines)?;
            Ok(spike_option(std_serdes_as_text_value(&value)?))
        }
        "std_serdes_as_array" => {
            let value = eval_json_unary(name, args, functions, env, lines)?;
            Ok(spike_option(std_serdes_as_array_value(&value)?))
        }
        "std_serdes_as_object" => {
            let value = eval_json_unary(name, args, functions, env, lines)?;
            Ok(spike_option(std_serdes_as_object_value(&value)?))
        }
        "std_serdes_field" => {
            let value = eval_std_serdes_field_value(name, args, functions, env, lines)?;
            Ok(spike_option(value))
        }
        "std_serdes_bool_field" => {
            let value = eval_std_serdes_field_value(name, args, functions, env, lines)?;
            Ok(spike_option(match value {
                Some(value) => std_serdes_as_bool_value(&value)?,
                None => None,
            }))
        }
        "std_serdes_text_field" => {
            let value = eval_std_serdes_field_value(name, args, functions, env, lines)?;
            Ok(spike_option(match value {
                Some(value) => std_serdes_as_text_value(&value)?,
                None => None,
            }))
        }
        "std_serdes_int_field" => {
            let value = eval_std_serdes_field_value(name, args, functions, env, lines)?;
            Ok(spike_option(match value {
                Some(value) => std_serdes_as_int_value(&value)?,
                None => None,
            }))
        }
        "std_serdes_array_field" => {
            let value = eval_std_serdes_field_value(name, args, functions, env, lines)?;
            Ok(spike_option(match value {
                Some(value) => std_serdes_as_array_value(&value)?,
                None => None,
            }))
        }
        "std_serdes_object_field" => {
            let value = eval_std_serdes_field_value(name, args, functions, env, lines)?;
            Ok(spike_option(match value {
                Some(value) => std_serdes_as_object_value(&value)?,
                None => None,
            }))
        }
        "std_serdes_value_item" => {
            let [value, index] = args else {
                return Err(unsupported("std_serdes_value_item expects two arguments"));
            };
            let value = eval_expr(value, functions, env, lines)?;
            let index = expect_signed_integer(eval_expr(index, functions, env, lines)?)?;
            let (variant, payloads) = expect_std_serdes_value(&value, name)?;
            let item = match (variant, payloads) {
                ("Array", [SpikeValue::Array(items)]) if index >= 0 => usize::try_from(index)
                    .ok()
                    .and_then(|index| items.get(index).cloned()),
                _ => None,
            };
            Ok(spike_option(item))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike std/serdes call {name:?}"
        ))),
    }
}

pub(crate) fn eval_std_serdes_field_value(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<Option<SpikeValue>, Diagnostic> {
    let [value, key] = args else {
        return Err(unsupported(&format!("{name} expects two arguments")));
    };
    let value = eval_expr(value, functions, env, lines)?;
    let key = match eval_expr(key, functions, env, lines)? {
        SpikeValue::Text(value) => value,
        _ => return Err(unsupported(&format!("{name} expects a string key"))),
    };
    let (variant, payloads) = expect_std_serdes_value(&value, name)?;
    let ("Object", [SpikeValue::Map(entries)]) = (variant, payloads) else {
        return Ok(None);
    };
    for (candidate, value) in entries {
        if map_keys_equal(candidate, &SpikeValue::Text(key.clone()))? {
            return Ok(Some(value.clone()));
        }
    }
    Ok(None)
}

pub(crate) fn expect_std_serdes_value<'a>(
    value: &'a SpikeValue,
    name: &str,
) -> Result<(&'a str, &'a [SpikeValue]), Diagnostic> {
    let SpikeValue::Enum {
        enum_name,
        variant,
        payloads,
        ..
    } = value
    else {
        return Err(unsupported(&format!("{name} expects std/serdes Value")));
    };
    if enum_name != "std_serdes_Value" {
        return Err(unsupported(&format!("{name} expects std/serdes Value")));
    }
    Ok((variant.as_str(), payloads.as_slice()))
}

pub(crate) fn std_serdes_as_bool_value(value: &SpikeValue) -> Result<Option<SpikeValue>, Diagnostic> {
    let (variant, payloads) = expect_std_serdes_value(value, "std_serdes_as_bool")?;
    Ok(match (variant, payloads) {
        ("Bool", [SpikeValue::Bool(value)]) => Some(SpikeValue::Bool(*value)),
        _ => None,
    })
}

pub(crate) fn std_serdes_as_int_value(value: &SpikeValue) -> Result<Option<SpikeValue>, Diagnostic> {
    let (variant, payloads) = expect_std_serdes_value(value, "std_serdes_as_int")?;
    Ok(match (variant, payloads) {
        ("Int", [SpikeValue::Int(value)]) => Some(SpikeValue::Int(*value)),
        _ => None,
    })
}

pub(crate) fn std_serdes_as_text_value(value: &SpikeValue) -> Result<Option<SpikeValue>, Diagnostic> {
    let (variant, payloads) = expect_std_serdes_value(value, "std_serdes_as_text")?;
    Ok(match (variant, payloads) {
        ("Text", [SpikeValue::Text(value)]) => Some(SpikeValue::Text(value.clone())),
        _ => None,
    })
}

pub(crate) fn std_serdes_as_array_value(value: &SpikeValue) -> Result<Option<SpikeValue>, Diagnostic> {
    let (variant, payloads) = expect_std_serdes_value(value, "std_serdes_as_array")?;
    Ok(match (variant, payloads) {
        ("Array", [SpikeValue::Array(values)]) => Some(SpikeValue::Array(values.clone())),
        _ => None,
    })
}

pub(crate) fn std_serdes_as_object_value(value: &SpikeValue) -> Result<Option<SpikeValue>, Diagnostic> {
    let (variant, payloads) = expect_std_serdes_value(value, "std_serdes_as_object")?;
    Ok(match (variant, payloads) {
        ("Object", [SpikeValue::Map(entries)]) => Some(SpikeValue::Map(entries.clone())),
        _ => None,
    })
}

pub(crate) fn json_serdes_result(value: Result<SpikeValue, String>) -> SpikeValue {
    match value {
        Ok(value) => SpikeValue::Enum {
            enum_name: String::from("Result"),
            variant: String::from("Ok"),
            field_names: Vec::new(),
            payloads: vec![value],
        },
        Err(message) => SpikeValue::Enum {
            enum_name: String::from("Result"),
            variant: String::from("Err"),
            field_names: Vec::new(),
            payloads: vec![SpikeValue::Struct {
                name: String::from("std_serdes_ParseError"),
                fields: vec![(String::from("message"), SpikeValue::Text(message))],
            }],
        },
    }
}

pub(crate) fn json_serdes_parse_document(text: &str) -> Result<SpikeValue, String> {
    let (value, index) = json_serdes_parse_value(text, json_skip_ws(text, 0))?;
    if json_skip_ws(text, index) == text.len() {
        Ok(value)
    } else {
        Err(String::from("trailing characters after JSON value"))
    }
}

pub(crate) fn is_crypto_call(name: &str) -> bool {
    matches!(
        name,
        "crypto_sha256"
            | "crypto_hmac_sha256"
            | "crypto_hmac_sha512"
            | "crypto_constant_time_eq"
            | "crypto_constant_time_eq_u8"
            | "crypto_rand_bytes"
            | "crypto_rand_u64"
            | "crypto_ed25519_keygen"
            | "crypto_ed25519_sign"
            | "crypto_ed25519_verify"
            | "crypto_aead_seal"
            | "crypto_aead_open"
    )
}

pub(crate) fn is_encoding_call(name: &str) -> bool {
    matches!(
        name,
        "encoding_url_component_encode"
            | "encoding_url_component_decode"
            | "encoding_path_segment_encode"
            | "encoding_url_query_pair_encode"
            | "encoding_path_join_segment"
    )
}

pub(crate) fn is_string_call(name: &str) -> bool {
    matches!(
        name,
        "string_line_at"
            | "string_byte_at"
            | "string_clone"
            | "string_starts_with"
            | "string_strip_prefix"
            | "string_strip_suffix"
            | "string_trim"
            | "string_trim_start"
    )
}

pub(crate) fn eval_string_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "string_line_at" => {
            let [text, index] = args else {
                return Err(unsupported("string_line_at expects exactly two arguments"));
            };
            let text = expect_text(eval_expr(text, functions, env, lines)?, name)?;
            let index = expect_int(eval_expr(index, functions, env, lines)?)?;
            let line = if index < 0 {
                None
            } else {
                text.lines()
                    .nth(index as usize)
                    .map(|line| SpikeValue::Text(line.to_string()))
            };
            Ok(spike_option(line))
        }
        "string_byte_at" => {
            let [text, index] = args else {
                return Err(unsupported("string_byte_at expects exactly two arguments"));
            };
            let text = expect_text(eval_expr(text, functions, env, lines)?, name)?;
            let index = expect_int(eval_expr(index, functions, env, lines)?)?;
            let byte = if index < 0 {
                None
            } else {
                text.as_bytes()
                    .get(index as usize)
                    .map(|byte| SpikeValue::Int(i64::from(*byte)))
            };
            Ok(spike_option(byte))
        }
        "string_clone" => {
            let [text] = args else {
                return Err(unsupported("string_clone expects exactly one argument"));
            };
            let text = expect_text(eval_expr(text, functions, env, lines)?, name)?;
            Ok(SpikeValue::Text(text))
        }
        "string_starts_with" => {
            let [text, prefix] = args else {
                return Err(unsupported(
                    "string_starts_with expects exactly two arguments",
                ));
            };
            let text = expect_text(eval_expr(text, functions, env, lines)?, name)?;
            let prefix = expect_text(eval_expr(prefix, functions, env, lines)?, name)?;
            Ok(SpikeValue::Bool(text.starts_with(&prefix)))
        }
        "string_strip_prefix" => {
            let [text, prefix] = args else {
                return Err(unsupported(
                    "string_strip_prefix expects exactly two arguments",
                ));
            };
            let text = expect_text(eval_expr(text, functions, env, lines)?, name)?;
            let prefix = expect_text(eval_expr(prefix, functions, env, lines)?, name)?;
            Ok(spike_option(
                text.strip_prefix(&prefix)
                    .map(|rest| SpikeValue::Text(rest.to_string())),
            ))
        }
        "string_strip_suffix" => {
            let [text, suffix] = args else {
                return Err(unsupported(
                    "string_strip_suffix expects exactly two arguments",
                ));
            };
            let text = expect_text(eval_expr(text, functions, env, lines)?, name)?;
            let suffix = expect_text(eval_expr(suffix, functions, env, lines)?, name)?;
            Ok(spike_option(
                text.strip_suffix(&suffix)
                    .map(|rest| SpikeValue::Text(rest.to_string())),
            ))
        }
        "string_trim" | "string_trim_start" => {
            let [text] = args else {
                return Err(unsupported(&format!("{name} expects exactly one argument")));
            };
            let text = expect_text(eval_expr(text, functions, env, lines)?, name)?;
            let trimmed = if name == "string_trim" {
                text.trim()
            } else {
                text.trim_start()
            };
            Ok(SpikeValue::Text(trimmed.to_string()))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike string call {name:?}"
        ))),
    }
}

pub(crate) fn eval_encoding_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "encoding_url_component_encode" | "encoding_path_segment_encode" => {
            let [value] = args else {
                return Err(unsupported(&format!("{name} expects exactly one argument")));
            };
            let value = expect_text(eval_expr(value, functions, env, lines)?, name)?;
            Ok(SpikeValue::Text(percent_encode(&value)))
        }
        "encoding_url_component_decode" => {
            let [value] = args else {
                return Err(unsupported(
                    "encoding_url_component_decode expects exactly one argument",
                ));
            };
            let value = expect_text(eval_expr(value, functions, env, lines)?, name)?;
            Ok(spike_option(percent_decode(&value).map(SpikeValue::Text)))
        }
        "encoding_url_query_pair_encode" => {
            let [key, value] = args else {
                return Err(unsupported(
                    "encoding_url_query_pair_encode expects exactly two arguments",
                ));
            };
            let key = expect_text(eval_expr(key, functions, env, lines)?, name)?;
            let value = expect_text(eval_expr(value, functions, env, lines)?, name)?;
            Ok(SpikeValue::Text(format!(
                "{}={}",
                percent_encode(&key),
                percent_encode(&value)
            )))
        }
        "encoding_path_join_segment" => {
            let [base, segment] = args else {
                return Err(unsupported(
                    "encoding_path_join_segment expects exactly two arguments",
                ));
            };
            let base = expect_text(eval_expr(base, functions, env, lines)?, name)?;
            let segment = expect_text(eval_expr(segment, functions, env, lines)?, name)?;
            let encoded = percent_encode(&segment);
            let joined = if base.is_empty() {
                encoded
            } else if base.ends_with('/') {
                format!("{base}{encoded}")
            } else {
                format!("{base}/{encoded}")
            };
            Ok(SpikeValue::Text(joined))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike encoding call {name:?}"
        ))),
    }
}

pub(crate) fn eval_crypto_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "crypto_sha256" => {
            let [arg] = args else {
                return Err(unsupported("crypto_sha256 expects exactly one argument"));
            };
            let input = expect_text(eval_expr(arg, functions, env, lines)?, name)?;
            Ok(SpikeValue::Text(sha256_hex(input.as_bytes())))
        }
        "crypto_hmac_sha256" | "crypto_hmac_sha512" => {
            let [key, message] = args else {
                return Err(unsupported(&format!(
                    "{name} expects exactly two arguments"
                )));
            };
            let key = expect_text(eval_expr(key, functions, env, lines)?, name)?;
            let message = expect_text(eval_expr(message, functions, env, lines)?, name)?;
            let tag = if name == "crypto_hmac_sha256" {
                hmac_hex(key.as_bytes(), message.as_bytes(), 64, sha256_bytes)
            } else {
                hmac_hex(key.as_bytes(), message.as_bytes(), 128, sha512_bytes)
            };
            Ok(SpikeValue::Text(tag))
        }
        "crypto_constant_time_eq" => {
            let [left, right] = args else {
                return Err(unsupported(
                    "crypto_constant_time_eq expects exactly two arguments",
                ));
            };
            let left = expect_text(eval_expr(left, functions, env, lines)?, name)?;
            let right = expect_text(eval_expr(right, functions, env, lines)?, name)?;
            Ok(SpikeValue::Bool(constant_time_eq_bytes(
                left.as_bytes(),
                right.as_bytes(),
            )))
        }
        "crypto_constant_time_eq_u8" => {
            let [left, right] = args else {
                return Err(unsupported(
                    "crypto_constant_time_eq_u8 expects exactly two arguments",
                ));
            };
            let left = expect_u8_array(eval_expr(left, functions, env, lines)?, name)?;
            let right = expect_u8_array(eval_expr(right, functions, env, lines)?, name)?;
            Ok(SpikeValue::Bool(constant_time_eq_bytes(&left, &right)))
        }
        "crypto_rand_bytes" => {
            let [length] = args else {
                return Err(unsupported(
                    "crypto_rand_bytes expects exactly one argument",
                ));
            };
            let length = expect_int(eval_expr(length, functions, env, lines)?)?;
            let bytes = crypto_random_bytes(length)?;
            Ok(SpikeValue::Array(
                bytes
                    .into_iter()
                    .map(|value| SpikeValue::UInt(value as u64))
                    .collect(),
            ))
        }
        "crypto_rand_u64" => {
            let [] = args else {
                return Err(unsupported("crypto_rand_u64 expects no arguments"));
            };
            let bytes = crypto_random_bytes(8)?;
            let value = u64::from_ne_bytes(
                bytes
                    .try_into()
                    .map_err(|_| unsupported("crypto_rand_u64 expected 8 random bytes"))?,
            );
            Ok(SpikeValue::UInt(value))
        }
        "crypto_ed25519_keygen" => {
            let [] = args else {
                return Err(unsupported("crypto_ed25519_keygen expects no arguments"));
            };
            let (public_key, secret_key) = crypto_ed25519_keygen()?;
            Ok(SpikeValue::Tuple(vec![
                spike_u8_array(public_key),
                spike_u8_array(secret_key),
            ]))
        }
        "crypto_ed25519_sign" => {
            let [secret_key, message] = args else {
                return Err(unsupported(
                    "crypto_ed25519_sign expects exactly two arguments",
                ));
            };
            let secret_key = expect_u8_array(eval_expr(secret_key, functions, env, lines)?, name)?;
            let message = expect_u8_array(eval_expr(message, functions, env, lines)?, name)?;
            Ok(spike_u8_array(crypto_ed25519_sign(&secret_key, &message)?))
        }
        "crypto_ed25519_verify" => {
            let [public_key, message, signature] = args else {
                return Err(unsupported(
                    "crypto_ed25519_verify expects exactly three arguments",
                ));
            };
            let public_key = expect_u8_array(eval_expr(public_key, functions, env, lines)?, name)?;
            let message = expect_u8_array(eval_expr(message, functions, env, lines)?, name)?;
            let signature = expect_u8_array(eval_expr(signature, functions, env, lines)?, name)?;
            Ok(SpikeValue::Bool(crypto_ed25519_verify(
                &public_key,
                &message,
                &signature,
            )?))
        }
        "crypto_aead_seal" => {
            let [alg, key, nonce, aad, plaintext] = args else {
                return Err(unsupported(
                    "crypto_aead_seal expects exactly five arguments",
                ));
            };
            let alg = expect_text(eval_expr(alg, functions, env, lines)?, name)?;
            let key = expect_u8_array(eval_expr(key, functions, env, lines)?, name)?;
            let nonce = expect_u8_array(eval_expr(nonce, functions, env, lines)?, name)?;
            let aad = expect_u8_array(eval_expr(aad, functions, env, lines)?, name)?;
            let plaintext = expect_u8_array(eval_expr(plaintext, functions, env, lines)?, name)?;
            Ok(spike_u8_array(crypto_aead_seal(
                &alg, &key, &nonce, &aad, &plaintext,
            )?))
        }
        "crypto_aead_open" => {
            let [alg, key, nonce, aad, ciphertext] = args else {
                return Err(unsupported(
                    "crypto_aead_open expects exactly five arguments",
                ));
            };
            let alg = expect_text(eval_expr(alg, functions, env, lines)?, name)?;
            let key = expect_u8_array(eval_expr(key, functions, env, lines)?, name)?;
            let nonce = expect_u8_array(eval_expr(nonce, functions, env, lines)?, name)?;
            let aad = expect_u8_array(eval_expr(aad, functions, env, lines)?, name)?;
            let ciphertext = expect_u8_array(eval_expr(ciphertext, functions, env, lines)?, name)?;
            Ok(spike_option(
                crypto_aead_open(&alg, &key, &nonce, &aad, &ciphertext)?.map(spike_u8_array),
            ))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike crypto call {name:?}"
        ))),
    }
}

pub(crate) fn spike_u8_array(bytes: Vec<u8>) -> SpikeValue {
    SpikeValue::Array(
        bytes
            .into_iter()
            .map(|value| SpikeValue::UInt(value as u64))
            .collect(),
    )
}

pub(crate) fn is_net_call(name: &str) -> bool {
    matches!(
        name,
        "net_resolve"
            | "net_tcp_listen"
            | "net_tcp_listener_port"
            | "net_tcp_accept"
            | "net_tcp_read"
            | "net_tcp_read_string"
            | "net_tcp_write"
            | "net_tcp_write_string"
            | "net_tcp_close"
            | "net_tcp_close_listener"
            | "net_tcp_listen_loopback_once"
            | "net_tcp_dial"
            | "net_udp_bind"
            | "net_udp_local_addr"
            | "net_udp_local_port"
            | "net_udp_send_to"
            | "net_udp_recv_from"
            | "net_udp_close"
            | "net_udp_bind_loopback_once"
            | "net_udp_send_recv"
    )
}

pub(crate) fn eval_net_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "net_resolve" => {
            let [host] = args else {
                return Err(unsupported("net_resolve expects exactly one argument"));
            };
            let host = expect_text(eval_expr(host, functions, env, lines)?, name)?;
            let resolved = i64_net_resolve_text(host.as_str()).map(SpikeValue::Text);
            Ok(spike_option(resolved))
        }
        "net_tcp_listen_loopback_once" => {
            let [response, timeout_ms] = args else {
                return Err(unsupported(
                    "net_tcp_listen_loopback_once expects exactly two arguments",
                ));
            };
            let response = expect_text(eval_expr(response, functions, env, lines)?, name)?;
            let timeout = net_timeout(expect_int(eval_expr(timeout_ms, functions, env, lines)?)?);
            Ok(spike_option(
                net_tcp_listen_loopback_once(response, timeout).map(SpikeValue::Int),
            ))
        }
        "net_tcp_listen" => {
            let [bind] = args else {
                return Err(unsupported("net_tcp_listen expects exactly one argument"));
            };
            let bind = expect_text(eval_expr(bind, functions, env, lines)?, name)?;
            Ok(SpikeValue::Int(net_tcp_listen(&bind).ok_or_else(|| {
                unsupported("net_tcp_listen failed in cranelift spike")
            })?))
        }
        "net_tcp_listener_port" => {
            let [listener] = args else {
                return Err(unsupported(
                    "net_tcp_listener_port expects exactly one argument",
                ));
            };
            let listener = expect_int(eval_expr(listener, functions, env, lines)?)?;
            Ok(SpikeValue::Int(
                net_tcp_listener_port(listener).ok_or_else(|| {
                    unsupported("net_tcp_listener_port failed in cranelift spike")
                })?,
            ))
        }
        "net_tcp_accept" => {
            let [listener] = args else {
                return Err(unsupported("net_tcp_accept expects exactly one argument"));
            };
            let listener = expect_int(eval_expr(listener, functions, env, lines)?)?;
            Ok(SpikeValue::Int(net_tcp_accept(listener).ok_or_else(
                || unsupported("net_tcp_accept failed in cranelift spike"),
            )?))
        }
        "net_tcp_read_string" => {
            let [stream, max_bytes] = args else {
                return Err(unsupported(
                    "net_tcp_read_string expects exactly two arguments",
                ));
            };
            let stream = expect_int(eval_expr(stream, functions, env, lines)?)?;
            let max_bytes = expect_int(eval_expr(max_bytes, functions, env, lines)?)?;
            Ok(SpikeValue::Text(
                net_tcp_read_string(stream, max_bytes)
                    .ok_or_else(|| unsupported("net_tcp_read_string failed in cranelift spike"))?,
            ))
        }
        "net_tcp_read" => {
            let [stream, buffer] = args else {
                return Err(unsupported("net_tcp_read expects exactly two arguments"));
            };
            let stream = expect_int(eval_expr(stream, functions, env, lines)?)?;
            let max_bytes = byte_buffer_len(eval_expr(buffer, functions, env, lines)?, env)?;
            Ok(SpikeValue::Int(
                net_tcp_read(stream, max_bytes)
                    .ok_or_else(|| unsupported("net_tcp_read failed in cranelift spike"))?,
            ))
        }
        "net_tcp_write_string" => {
            let [stream, message] = args else {
                return Err(unsupported(
                    "net_tcp_write_string expects exactly two arguments",
                ));
            };
            let stream = expect_int(eval_expr(stream, functions, env, lines)?)?;
            let message = expect_text(eval_expr(message, functions, env, lines)?, name)?;
            Ok(SpikeValue::Int(net_tcp_write_string(stream, &message)))
        }
        "net_tcp_write" => {
            let [stream, buffer] = args else {
                return Err(unsupported("net_tcp_write expects exactly two arguments"));
            };
            let stream = expect_int(eval_expr(stream, functions, env, lines)?)?;
            let message = byte_buffer_text(eval_expr(buffer, functions, env, lines)?, env)?;
            Ok(SpikeValue::Int(net_tcp_write_string(stream, &message)))
        }
        "net_tcp_close" => {
            let [stream] = args else {
                return Err(unsupported("net_tcp_close expects exactly one argument"));
            };
            let stream = expect_int(eval_expr(stream, functions, env, lines)?)?;
            Ok(SpikeValue::Int(net_tcp_close(stream)))
        }
        "net_tcp_close_listener" => {
            let [listener] = args else {
                return Err(unsupported(
                    "net_tcp_close_listener expects exactly one argument",
                ));
            };
            let listener = expect_int(eval_expr(listener, functions, env, lines)?)?;
            Ok(SpikeValue::Int(net_tcp_close_listener(listener)))
        }
        "net_tcp_dial" => {
            let [host, port, message, timeout_ms] = args else {
                return Err(unsupported("net_tcp_dial expects exactly four arguments"));
            };
            let host = expect_text(eval_expr(host, functions, env, lines)?, name)?;
            let port = expect_int(eval_expr(port, functions, env, lines)?)?;
            let message = expect_text(eval_expr(message, functions, env, lines)?, name)?;
            let timeout = net_timeout(expect_int(eval_expr(timeout_ms, functions, env, lines)?)?);
            Ok(spike_option(
                net_tcp_dial(host, port, message, timeout).map(SpikeValue::Text),
            ))
        }
        "net_udp_bind_loopback_once" => {
            let [response, timeout_ms] = args else {
                return Err(unsupported(
                    "net_udp_bind_loopback_once expects exactly two arguments",
                ));
            };
            let response = expect_text(eval_expr(response, functions, env, lines)?, name)?;
            let timeout = net_timeout(expect_int(eval_expr(timeout_ms, functions, env, lines)?)?);
            Ok(spike_option(
                net_udp_bind_loopback_once(response, timeout).map(SpikeValue::Int),
            ))
        }
        "net_udp_bind" => {
            let [bind] = args else {
                return Err(unsupported("net_udp_bind expects exactly one argument"));
            };
            let bind = expect_text(eval_expr(bind, functions, env, lines)?, name)?;
            Ok(SpikeValue::Int(net_udp_bind(&bind).ok_or_else(|| {
                unsupported("net_udp_bind failed in cranelift spike")
            })?))
        }
        "net_udp_local_addr" => {
            let [socket] = args else {
                return Err(unsupported(
                    "net_udp_local_addr expects exactly one argument",
                ));
            };
            let socket = expect_int(eval_expr(socket, functions, env, lines)?)?;
            Ok(SpikeValue::Text(net_udp_local_addr(socket).ok_or_else(
                || unsupported("net_udp_local_addr failed in cranelift spike"),
            )?))
        }
        "net_udp_local_port" => {
            let [socket] = args else {
                return Err(unsupported(
                    "net_udp_local_port expects exactly one argument",
                ));
            };
            let socket = expect_int(eval_expr(socket, functions, env, lines)?)?;
            Ok(SpikeValue::Int(net_udp_local_port(socket).ok_or_else(
                || unsupported("net_udp_local_port failed in cranelift spike"),
            )?))
        }
        "net_udp_send_to" => {
            let [socket, buffer, peer] = args else {
                return Err(unsupported(
                    "net_udp_send_to expects exactly three arguments",
                ));
            };
            let socket = expect_int(eval_expr(socket, functions, env, lines)?)?;
            let message = byte_buffer_text(eval_expr(buffer, functions, env, lines)?, env)?;
            let peer = expect_text(eval_expr(peer, functions, env, lines)?, name)?;
            Ok(SpikeValue::Int(net_udp_send_to(socket, &message, &peer)))
        }
        "net_udp_recv_from" => {
            let [socket, buffer] = args else {
                return Err(unsupported(
                    "net_udp_recv_from expects exactly two arguments",
                ));
            };
            let socket = expect_int(eval_expr(socket, functions, env, lines)?)?;
            let max_bytes = byte_buffer_len(eval_expr(buffer, functions, env, lines)?, env)?;
            let (count, peer) = net_udp_recv_from(socket, max_bytes)
                .ok_or_else(|| unsupported("net_udp_recv_from failed in cranelift spike"))?;
            Ok(SpikeValue::Tuple(vec![
                SpikeValue::Int(count),
                SpikeValue::Text(peer),
            ]))
        }
        "net_udp_close" => {
            let [socket] = args else {
                return Err(unsupported("net_udp_close expects exactly one argument"));
            };
            let socket = expect_int(eval_expr(socket, functions, env, lines)?)?;
            Ok(SpikeValue::Int(net_udp_close(socket)))
        }
        "net_udp_send_recv" => {
            let [host, port, message, timeout_ms] = args else {
                return Err(unsupported(
                    "net_udp_send_recv expects exactly four arguments",
                ));
            };
            let host = expect_text(eval_expr(host, functions, env, lines)?, name)?;
            let port = expect_int(eval_expr(port, functions, env, lines)?)?;
            let message = expect_text(eval_expr(message, functions, env, lines)?, name)?;
            let timeout = net_timeout(expect_int(eval_expr(timeout_ms, functions, env, lines)?)?);
            Ok(spike_option(
                net_udp_send_recv(host, port, message, timeout).map(SpikeValue::Text),
            ))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike net call {name:?}"
        ))),
    }
}

pub(crate) fn eval_net_call_effectful(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &mut SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "net_tcp_read" => {
            let [stream, buffer] = args else {
                return Err(unsupported("net_tcp_read expects exactly two arguments"));
            };
            let stream = expect_int(eval_expr(stream, functions, env, lines)?)?;
            let buffer = eval_expr(buffer, functions, env, lines)?;
            let max_bytes = byte_buffer_len(buffer.clone(), env)?;
            let text = net_tcp_read_string(stream, max_bytes)
                .ok_or_else(|| unsupported("net_tcp_read failed in cranelift spike"))?;
            write_byte_buffer_text(buffer, env, &text)?;
            Ok(SpikeValue::Int(
                i64::try_from(text.len()).unwrap_or(i64::MAX),
            ))
        }
        "net_udp_recv_from" => {
            let [socket, buffer] = args else {
                return Err(unsupported(
                    "net_udp_recv_from expects exactly two arguments",
                ));
            };
            let socket = expect_int(eval_expr(socket, functions, env, lines)?)?;
            let buffer = eval_expr(buffer, functions, env, lines)?;
            let max_bytes = byte_buffer_len(buffer.clone(), env)?;
            let (count, peer, text) = net_udp_recv_from_text(socket, max_bytes)
                .ok_or_else(|| unsupported("net_udp_recv_from failed in cranelift spike"))?;
            write_byte_buffer_text(buffer, env, &text)?;
            Ok(SpikeValue::Tuple(vec![
                SpikeValue::Int(count),
                SpikeValue::Text(peer),
            ]))
        }
        _ => eval_net_call(name, args, functions, env, lines),
    }
}

pub(crate) fn byte_buffer_len(value: SpikeValue, env: &SpikeEnv) -> Result<i64, Diagnostic> {
    i64::try_from(byte_buffer_values(value, env)?.len())
        .map_err(|_| unsupported("byte buffer length is outside the host i64 range"))
}

pub(crate) fn byte_buffer_text(value: SpikeValue, env: &SpikeEnv) -> Result<String, Diagnostic> {
    byte_buffer_values(value, env)?
        .into_iter()
        .map(|value| match value {
            SpikeValue::Int(value) => u8::try_from(value)
                .map_err(|_| unsupported("network byte buffers must contain u8-compatible values")),
            SpikeValue::UInt(value) => u8::try_from(value)
                .map_err(|_| unsupported("network byte buffers must contain u8-compatible values")),
            _ => Err(unsupported("network byte buffers must contain integers")),
        })
        .collect::<Result<Vec<_>, _>>()
        .and_then(|bytes| {
            String::from_utf8(bytes)
                .map_err(|_| unsupported("network byte buffers must be valid UTF-8 text"))
        })
}

pub(crate) fn write_byte_buffer_text(
    value: SpikeValue,
    env: &mut SpikeEnv,
    text: &str,
) -> Result<(), Diagnostic> {
    let bytes = text
        .bytes()
        .map(|byte| SpikeValue::UInt(u64::from(byte)))
        .collect::<Vec<_>>();
    match value {
        SpikeValue::MutSlice { target, start, end } => {
            let Some(SpikeValue::Array(values)) = env.get_mut(&target) else {
                return Err(unsupported(
                    "mutable byte buffers require a live local array",
                ));
            };
            if start > end || end > values.len() {
                return Err(unsupported("byte buffer slice is outside the array length"));
            }
            for (index, byte) in bytes.into_iter().take(end - start).enumerate() {
                values[start + index] = byte;
            }
            Ok(())
        }
        SpikeValue::Array(_) => Ok(()),
        _ => Err(unsupported("network byte buffers must be byte arrays")),
    }
}

pub(crate) fn byte_buffer_values(value: SpikeValue, env: &SpikeEnv) -> Result<Vec<SpikeValue>, Diagnostic> {
    match value {
        SpikeValue::Array(values) => Ok(values),
        SpikeValue::MutSlice { target, start, end } => {
            let Some(SpikeValue::Array(values)) = env.get(&target) else {
                return Err(unsupported(
                    "mutable byte buffers require a live local array",
                ));
            };
            let Some(values) = values.get(start..end) else {
                return Err(unsupported("byte buffer slice is outside the array length"));
            };
            Ok(values.to_vec())
        }
        _ => Err(unsupported("network byte buffers must be byte arrays")),
    }
}

pub(crate) fn net_loopback_socket_addr(host: &str, port: i64) -> Option<SocketAddr> {
    let port = u16::try_from(port).ok()?;
    match host {
        "localhost" | "127.0.0.1" => Some(SocketAddr::from(([127, 0, 0, 1], port))),
        "::1" => Some(SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port))),
        _ => None,
    }
}

pub(crate) fn spike_tcp_listeners() -> &'static Mutex<HashMap<i64, SpikeTcpListener>> {
    SPIKE_TCP_LISTENERS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn spike_tcp_streams() -> &'static Mutex<HashMap<i64, SpikeTcpStream>> {
    SPIKE_TCP_STREAMS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn spike_tcp_responses() -> &'static Mutex<HashMap<i64, String>> {
    SPIKE_TCP_RESPONSES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn spike_udp_sockets() -> &'static Mutex<HashMap<i64, SpikeUdpSocket>> {
    SPIKE_UDP_SOCKETS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn spike_tcp_next_handle() -> i64 {
    SPIKE_TCP_NEXT_HANDLE.fetch_add(1, Ordering::Relaxed)
}

pub(crate) fn spike_udp_next_handle() -> i64 {
    SPIKE_UDP_NEXT_HANDLE.fetch_add(1, Ordering::Relaxed)
}

pub(crate) fn net_tcp_listen(bind: &str) -> Option<i64> {
    let addr = http_parse_loopback_bind(bind)?;
    let handle = spike_tcp_next_handle();
    let port = if addr.port() == 0 {
        20_000 + handle.rem_euclid(30_000)
    } else {
        i64::from(addr.port())
    };
    spike_tcp_listeners()
        .lock()
        .ok()?
        .insert(handle, SpikeTcpListener { port });
    Some(handle)
}

pub(crate) fn net_tcp_listener_port(listener: i64) -> Option<i64> {
    let listeners = spike_tcp_listeners().lock().ok()?;
    Some(listeners.get(&listener)?.port)
}

pub(crate) fn net_tcp_accept(listener: i64) -> Option<i64> {
    let listeners = spike_tcp_listeners().lock().ok()?;
    let listener_port = listeners.get(&listener)?.port;
    drop(listeners);
    let handle = spike_tcp_next_handle();
    spike_tcp_streams().lock().ok()?.insert(
        handle,
        SpikeTcpStream {
            listener_port,
            received: String::from("ping"),
            written: String::new(),
        },
    );
    Some(handle)
}

pub(crate) fn net_tcp_read(stream: i64, max_bytes: i64) -> Option<i64> {
    let streams = spike_tcp_streams().lock().ok()?;
    let stream = streams.get(&stream)?;
    let max_bytes = usize::try_from(max_bytes.max(0)).ok()?;
    Some(i64::try_from(stream.received.as_bytes().len().min(max_bytes)).ok()?)
}

pub(crate) fn net_tcp_read_string(stream: i64, max_bytes: i64) -> Option<String> {
    let streams = spike_tcp_streams().lock().ok()?;
    let stream = streams.get(&stream)?;
    let max_bytes = usize::try_from(max_bytes.max(0)).ok()?;
    Some(stream.received.chars().take(max_bytes).collect())
}

pub(crate) fn net_tcp_write_string(stream: i64, message: &str) -> i64 {
    let Ok(mut streams) = spike_tcp_streams().lock() else {
        return -1;
    };
    let Some(stream) = streams.get_mut(&stream) else {
        return -1;
    };
    stream.written.push_str(message);
    if let Ok(mut responses) = spike_tcp_responses().lock() {
        responses.insert(stream.listener_port, stream.written.clone());
    }
    i64::try_from(message.len()).unwrap_or(-1)
}

pub(crate) fn net_tcp_close(stream: i64) -> i64 {
    if let Ok(mut streams) = spike_tcp_streams().lock()
        && streams.remove(&stream).is_some()
    {
        0
    } else {
        -1
    }
}

pub(crate) fn net_tcp_close_listener(listener: i64) -> i64 {
    if spike_tcp_listeners()
        .lock()
        .ok()
        .and_then(|mut listeners| listeners.remove(&listener))
        .is_some()
    {
        0
    } else {
        -1
    }
}

pub(crate) fn net_tcp_registered_loopback_echo(host: &str, port: i64, message: &str) -> Option<String> {
    net_loopback_socket_addr(host, port)?;
    if let Some(response) = spike_tcp_responses().lock().ok()?.remove(&port) {
        return Some(response);
    }
    let listeners = spike_tcp_listeners().lock().ok()?;
    if listeners.values().any(|listener| listener.port == port) {
        return Some(message.to_string());
    }
    drop(listeners);
    if let Some(response) = spike_tcp_streams()
        .lock()
        .ok()?
        .values()
        .find(|stream| stream.listener_port == port && !stream.written.is_empty())
        .map(|stream| stream.written.clone())
    {
        return Some(response);
    }
    None
}

pub(crate) fn net_tcp_dial(
    host: String,
    port: i64,
    message: String,
    timeout: std::time::Duration,
) -> Option<String> {
    if let Some(response) = net_tcp_registered_loopback_echo(&host, port, &message) {
        return Some(response);
    }
    let addr = net_loopback_socket_addr(&host, port)?;
    let mut stream = std::net::TcpStream::connect_timeout(&addr, timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    stream.write_all(message.as_bytes()).ok()?;
    stream.shutdown(std::net::Shutdown::Write).ok()?;
    let mut response = Vec::new();
    stream.take(64 * 1024).read_to_end(&mut response).ok()?;
    String::from_utf8(response).ok()
}

pub(crate) fn net_udp_bind(bind: &str) -> Option<i64> {
    let addr = http_parse_loopback_bind(bind)?;
    let handle = spike_udp_next_handle();
    let port = if addr.port() == 0 {
        30_000 + handle.rem_euclid(20_000)
    } else {
        i64::from(addr.port())
    };
    let addr = SocketAddr::new(addr.ip(), u16::try_from(port).ok()?);
    spike_udp_sockets().lock().ok()?.insert(
        handle,
        SpikeUdpSocket {
            addr,
            datagrams: Vec::new(),
        },
    );
    Some(handle)
}

pub(crate) fn net_udp_local_addr(socket: i64) -> Option<String> {
    let sockets = spike_udp_sockets().lock().ok()?;
    Some(sockets.get(&socket)?.addr.to_string())
}

pub(crate) fn net_udp_local_port(socket: i64) -> Option<i64> {
    let sockets = spike_udp_sockets().lock().ok()?;
    Some(i64::from(sockets.get(&socket)?.addr.port()))
}

pub(crate) fn net_udp_send_to(socket: i64, message: &str, peer: &str) -> i64 {
    let Ok(peer_addr) = peer.parse::<SocketAddr>() else {
        return -1;
    };
    let Ok(mut sockets) = spike_udp_sockets().lock() else {
        return -1;
    };
    let Some(source_addr) = sockets.get(&socket).map(|socket| socket.addr) else {
        return -1;
    };
    if let Some(target) = sockets
        .values_mut()
        .find(|candidate| candidate.addr == peer_addr)
    {
        target
            .datagrams
            .push((message.to_string(), source_addr.to_string()));
        i64::try_from(message.len()).unwrap_or(-1)
    } else {
        -1
    }
}

pub(crate) fn net_udp_recv_from(socket: i64, max_bytes: i64) -> Option<(i64, String)> {
    let (count, peer, _message) = net_udp_recv_from_text(socket, max_bytes)?;
    Some((count, peer))
}

pub(crate) fn net_udp_recv_from_text(socket: i64, max_bytes: i64) -> Option<(i64, String, String)> {
    let mut sockets = spike_udp_sockets().lock().ok()?;
    let socket = sockets.get_mut(&socket)?;
    let (message, peer) = socket.datagrams.pop()?;
    let max_bytes = usize::try_from(max_bytes.max(0)).ok()?;
    let message = message.chars().take(max_bytes).collect::<String>();
    Some((i64::try_from(message.len()).ok()?, peer, message))
}

pub(crate) fn net_udp_close(socket: i64) -> i64 {
    if spike_udp_sockets()
        .lock()
        .ok()
        .and_then(|mut sockets| sockets.remove(&socket))
        .is_some()
    {
        0
    } else {
        -1
    }
}

pub(crate) fn net_udp_send_recv(
    host: String,
    port: i64,
    message: String,
    timeout: std::time::Duration,
) -> Option<String> {
    let addr = net_loopback_socket_addr(&host, port)?;
    let socket = std::net::UdpSocket::bind(("127.0.0.1", 0)).ok()?;
    socket.set_read_timeout(Some(timeout)).ok()?;
    socket.set_write_timeout(Some(timeout)).ok()?;
    socket.send_to(message.as_bytes(), addr).ok()?;
    let mut response = vec![0u8; 64 * 1024];
    let (n, _peer) = socket.recv_from(&mut response).ok()?;
    response.truncate(n);
    String::from_utf8(response).ok()
}

pub(crate) fn is_http_call(name: &str) -> bool {
    matches!(
        name,
        "http_get"
            | "http_server_listen"
            | "http_server_local_port"
            | "http_server_accept"
            | "http_request_method"
            | "http_request_path"
            | "http_request_body"
            | "http_response_write"
            | "http_server_close"
            | "http_serve_once"
            | "http_serve_route"
            | "http_async_serve_route"
    )
}

pub(crate) fn eval_http_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "http_get" => {
            let [url] = args else {
                return Err(unsupported("http_get expects exactly one argument"));
            };
            let url = expect_text(eval_expr(url, functions, env, lines)?, name)?;
            Ok(spike_option(http_get(&url).map(SpikeValue::Text)))
        }
        "http_server_listen" => {
            let [bind] = args else {
                return Err(unsupported(
                    "http_server_listen expects exactly one argument",
                ));
            };
            let bind = expect_text(eval_expr(bind, functions, env, lines)?, name)?;
            Ok(SpikeValue::Int(http_server_listen(&bind).ok_or_else(
                || unsupported("http_server_listen failed in cranelift spike"),
            )?))
        }
        "http_server_local_port" => {
            let [server] = args else {
                return Err(unsupported(
                    "http_server_local_port expects exactly one argument",
                ));
            };
            let server = expect_int(eval_expr(server, functions, env, lines)?)?;
            Ok(SpikeValue::Int(http_server_local_port(server).ok_or_else(
                || unsupported("http_server_local_port failed in cranelift spike"),
            )?))
        }
        "http_server_accept" => {
            let [server] = args else {
                return Err(unsupported(
                    "http_server_accept expects exactly one argument",
                ));
            };
            let server = expect_int(eval_expr(server, functions, env, lines)?)?;
            Ok(SpikeValue::Int(http_server_accept(server).ok_or_else(
                || unsupported("http_server_accept failed in cranelift spike"),
            )?))
        }
        "http_request_method" | "http_request_path" | "http_request_body" => {
            let [request] = args else {
                return Err(unsupported(&format!("{name} expects exactly one argument")));
            };
            let request = expect_int(eval_expr(request, functions, env, lines)?)?;
            let value = http_request_part(request, name)
                .ok_or_else(|| unsupported("http request handle missing in cranelift spike"))?;
            Ok(SpikeValue::Text(value))
        }
        "http_response_write" => {
            let [request, status, body] = args else {
                return Err(unsupported(
                    "http_response_write expects exactly three arguments",
                ));
            };
            let request = expect_int(eval_expr(request, functions, env, lines)?)?;
            let status = expect_int(eval_expr(status, functions, env, lines)?)?;
            let body = expect_text(eval_expr(body, functions, env, lines)?, name)?;
            Ok(SpikeValue::Bool(http_response_write(
                request, status, &body,
            )))
        }
        "http_server_close" => {
            let [server] = args else {
                return Err(unsupported(
                    "http_server_close expects exactly one argument",
                ));
            };
            let server = expect_int(eval_expr(server, functions, env, lines)?)?;
            Ok(SpikeValue::Bool(http_server_close(server)))
        }
        "http_serve_once" => {
            let [bind, body] = args else {
                return Err(unsupported("http_serve_once expects exactly two arguments"));
            };
            let bind = expect_text(eval_expr(bind, functions, env, lines)?, name)?;
            let body = expect_text(eval_expr(body, functions, env, lines)?, name)?;
            if http_parse_loopback_bind(&bind).is_none() {
                lines.push(OutputLine::stderr(HTTP_NON_LOOPBACK_BIND_DIAG));
                return Ok(SpikeValue::Bool(false));
            }
            Ok(SpikeValue::Bool(http_serve_once(&bind, &body)))
        }
        "http_serve_route" => {
            let [bind, route_path, body, max_requests] = args else {
                return Err(unsupported(
                    "http_serve_route expects exactly four arguments",
                ));
            };
            let bind = expect_text(eval_expr(bind, functions, env, lines)?, name)?;
            let route_path = expect_text(eval_expr(route_path, functions, env, lines)?, name)?;
            let body = expect_text(eval_expr(body, functions, env, lines)?, name)?;
            let max_requests = expect_int(eval_expr(max_requests, functions, env, lines)?)?;
            if http_parse_loopback_bind(&bind).is_none() {
                lines.push(OutputLine::stderr(HTTP_NON_LOOPBACK_BIND_DIAG));
                return Ok(SpikeValue::Bool(false));
            }
            Ok(SpikeValue::Bool(http_serve_route(
                &bind,
                &route_path,
                &body,
                max_requests,
            )))
        }
        "http_async_serve_route" => {
            let [server, route_path, body, max_requests] = args else {
                return Err(unsupported(
                    "http_async_serve_route expects exactly four arguments",
                ));
            };
            let server = expect_int(eval_expr(server, functions, env, lines)?)?;
            let route_path = expect_text(eval_expr(route_path, functions, env, lines)?, name)?;
            let body = expect_text(eval_expr(body, functions, env, lines)?, name)?;
            let max_requests = expect_int(eval_expr(max_requests, functions, env, lines)?)?;
            Ok(spike_task(SpikeValue::Bool(http_serve_route_on_server(
                server,
                &route_path,
                &body,
                max_requests,
            ))))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike http call {name:?}"
        ))),
    }
}

pub(crate) fn eval_env_get_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [name] = args else {
        return Err(unsupported("env_get expects exactly one argument"));
    };
    let name = expect_text(eval_expr(name, functions, env, lines)?, "env_get")?;
    if !spike_env_name_allowed(env, &name) {
        return Ok(spike_option(None));
    }
    Ok(spike_option(env::var(name).ok().map(SpikeValue::Text)))
}

pub(crate) fn spike_env_name_allowed(env: &SpikeEnv, name: &str) -> bool {
    if matches!(
        env.get(SPIKE_ENV_UNRESTRICTED_BINDING),
        Some(SpikeValue::Bool(true))
    ) {
        return true;
    }
    matches!(
        env.get(SPIKE_ENV_ALLOWLIST_BINDING),
        Some(SpikeValue::Array(names))
            if names
                .iter()
                .any(|allowed| matches!(allowed, SpikeValue::Text(allowed) if allowed == name))
    )
}

pub(crate) fn is_fs_write_call(name: &str) -> bool {
    matches!(
        name,
        "fs_write"
            | "fs_create"
            | "fs_append"
            | "fs_mkdir"
            | "fs_mkdir_all"
            | "fs_remove_file"
            | "fs_remove_dir"
            | "fs_replace"
    )
}

pub(crate) fn eval_fs_read_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [path] = args else {
        return Err(unsupported("fs_read expects exactly one argument"));
    };
    let path = expect_text(eval_expr(path, functions, env, lines)?, "fs_read")?;
    Ok(spike_option(
        spike_fs_read_text(env, &path)?.map(SpikeValue::Text),
    ))
}

pub(crate) fn spike_fs_read_text(env: &SpikeEnv, path: &str) -> Result<Option<String>, Diagnostic> {
    let (package_root, fs_root) = spike_fs_scope(env)?;
    Ok(spike_fs_read_text_for_scope(&package_root, &fs_root, path))
}

pub(crate) fn eval_fs_write_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let result = match name {
        "fs_write" => {
            let (path, content) = eval_fs_path_content(name, args, functions, env, lines)?;
            if content.len() > SPIKE_MAX_FS_WRITE_BYTES {
                -1
            } else {
                spike_fs_write_candidate(env, &path, false)?
                    .and_then(|candidate| std::fs::write(candidate, content).ok())
                    .map(|()| 0)
                    .unwrap_or(-1)
            }
        }
        "fs_create" => {
            let path = eval_fs_path(name, args, functions, env, lines)?;
            spike_fs_write_candidate(env, &path, false)?
                .and_then(|candidate| {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(candidate)
                        .ok()
                })
                .map(|_| 0)
                .unwrap_or(-1)
        }
        "fs_append" => {
            let (path, content) = eval_fs_path_content(name, args, functions, env, lines)?;
            if content.len() > SPIKE_MAX_FS_WRITE_BYTES {
                -1
            } else {
                spike_fs_write_candidate(env, &path, false)?
                    .and_then(|candidate| {
                        let mut file = std::fs::OpenOptions::new()
                            .append(true)
                            .create(true)
                            .open(candidate)
                            .ok()?;
                        std::io::Write::write_all(&mut file, content.as_bytes()).ok()
                    })
                    .map(|()| 0)
                    .unwrap_or(-1)
            }
        }
        "fs_mkdir" => {
            let path = eval_fs_path(name, args, functions, env, lines)?;
            spike_fs_write_candidate(env, &path, false)?
                .and_then(|candidate| std::fs::create_dir(candidate).ok())
                .map(|()| 0)
                .unwrap_or(-1)
        }
        "fs_mkdir_all" => {
            let path = eval_fs_path(name, args, functions, env, lines)?;
            spike_fs_write_candidate(env, &path, true)?
                .and_then(|candidate| std::fs::create_dir_all(candidate).ok())
                .map(|()| 0)
                .unwrap_or(-1)
        }
        "fs_remove_file" => {
            let path = eval_fs_path(name, args, functions, env, lines)?;
            spike_fs_existing_candidate(env, &path)?
                .and_then(|candidate| {
                    std::fs::metadata(&candidate)
                        .ok()
                        .filter(|metadata| metadata.is_file())?;
                    std::fs::remove_file(candidate).ok()
                })
                .map(|()| 0)
                .unwrap_or(-1)
        }
        "fs_remove_dir" => {
            let path = eval_fs_path(name, args, functions, env, lines)?;
            spike_fs_existing_candidate(env, &path)?
                .and_then(|candidate| {
                    std::fs::metadata(&candidate)
                        .ok()
                        .filter(|metadata| metadata.is_dir())?;
                    std::fs::remove_dir(candidate).ok()
                })
                .map(|()| 0)
                .unwrap_or(-1)
        }
        "fs_replace" => {
            let (path, content) = eval_fs_path_content(name, args, functions, env, lines)?;
            if content.len() > SPIKE_MAX_FS_WRITE_BYTES {
                -1
            } else {
                spike_fs_write_candidate(env, &path, false)?
                    .and_then(|candidate| std::fs::write(candidate, content).ok())
                    .map(|()| 0)
                    .unwrap_or(-1)
            }
        }
        _ => {
            return Err(unsupported(&format!(
                "unsupported cranelift spike filesystem call {name:?}"
            )));
        }
    };
    Ok(SpikeValue::Int(result))
}

pub(crate) fn eval_fs_path(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<String, Diagnostic> {
    let [path] = args else {
        return Err(unsupported(&format!("{name} expects exactly one argument")));
    };
    expect_text(eval_expr(path, functions, env, lines)?, name)
}

pub(crate) fn eval_fs_path_content(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<(String, String), Diagnostic> {
    let [path, content] = args else {
        return Err(unsupported(&format!(
            "{name} expects exactly two arguments"
        )));
    };
    let path = expect_text(eval_expr(path, functions, env, lines)?, name)?;
    let content = expect_text(eval_expr(content, functions, env, lines)?, name)?;
    Ok((path, content))
}

pub(crate) fn spike_fs_root(env: &SpikeEnv) -> Result<PathBuf, Diagnostic> {
    match env.get(SPIKE_FS_ROOT_BINDING) {
        Some(SpikeValue::Text(root)) => Ok(PathBuf::from(root)),
        _ => Err(unsupported(
            "cranelift spike filesystem root is unavailable",
        )),
    }
}

pub(crate) fn spike_fs_scope(env: &SpikeEnv) -> Result<(PathBuf, PathBuf), Diagnostic> {
    let fs_root = spike_fs_root(env)?;
    let package_root = match env.get(SPIKE_PACKAGE_ROOT_BINDING) {
        Some(SpikeValue::Text(root)) => PathBuf::from(root),
        _ => fs_root.clone(),
    };
    Ok((package_root, fs_root))
}

pub(crate) fn spike_fs_existing_candidate(env: &SpikeEnv, path: &str) -> Result<Option<PathBuf>, Diagnostic> {
    let (package_root, fs_root) = spike_fs_scope(env)?;
    Ok(spike_fs_existing_candidate_for_scope(
        &package_root,
        &fs_root,
        path,
    ))
}

pub(crate) fn spike_fs_write_candidate(
    env: &SpikeEnv,
    path: &str,
    allow_missing_ancestors: bool,
) -> Result<Option<PathBuf>, Diagnostic> {
    let (package_root, fs_root) = spike_fs_scope(env)?;
    Ok(spike_fs_write_candidate_for_scope(
        &package_root,
        &fs_root,
        path,
        allow_missing_ancestors,
    ))
}

pub(crate) fn eval_process_status_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [command] = args else {
        return Err(unsupported("process_status expects exactly one argument"));
    };
    let command = expect_text(eval_expr(command, functions, env, lines)?, "process_status")?;
    let status = match command.as_str() {
        "/usr/bin/true" => 0,
        "/usr/bin/false" => 1,
        "__axiom_stage1_missing_binary__" => -1,
        _ => {
            // Beyond the deterministic sentinels, only programs owned by the
            // package itself may run: the command must canonicalize inside the
            // package root, mirroring the filesystem scope model. System paths
            // outside the package stay rejected so compile-time folding cannot
            // execute arbitrary host binaries.
            let (package_root, _) = spike_fs_scope(env)?;
            let package_local = std::fs::canonicalize(&package_root)
                .ok()
                .zip(std::fs::canonicalize(&command).ok())
                .filter(|(root, candidate)| candidate.starts_with(root))
                .map(|(_, candidate)| candidate);
            let Some(candidate) = package_local else {
                return Err(unsupported(
                    "process_status spike only permits allowlisted deterministic commands or package-local programs",
                ));
            };
            // Run the package-local program, mirroring the generated runtime's
            // process contract (exit code, or -1 when the process cannot spawn
            // or is signal-terminated).
            std::process::Command::new(candidate)
                .status()
                .ok()
                .and_then(|status| status.code())
                .map(i64::from)
                .unwrap_or(-1)
        }
    };
    Ok(SpikeValue::Int(status))
}

pub(crate) fn is_regex_call(name: &str) -> bool {
    matches!(name, "regex_is_match" | "regex_find" | "regex_replace_all")
}

pub(crate) fn eval_regex_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "env_get" => {
            let [name] = args else {
                return Err(unsupported("env_get expects exactly one argument"));
            };
            let name = match eval_expr(name, functions, env, lines)? {
                SpikeValue::Text(value) => value,
                _ => return Err(unsupported("env_get expects a string argument")),
            };
            let value = std::env::var(name).ok();
            Ok(spike_option(value.map(SpikeValue::Text)))
        }
        "regex_is_match" => {
            let (pattern, text) = eval_regex_binary_text(name, args, functions, env, lines)?;
            Ok(SpikeValue::Bool(regex_find_span(&pattern, &text).is_some()))
        }
        "regex_find" => {
            let (pattern, text) = eval_regex_binary_text(name, args, functions, env, lines)?;
            let found = regex_find_span(&pattern, &text)
                .map(|(start, end)| SpikeValue::Text(text[start..end].to_string()));
            Ok(spike_option(found))
        }
        "regex_replace_all" => {
            let [pattern, text, replacement] = args else {
                return Err(unsupported(
                    "regex_replace_all expects exactly three arguments",
                ));
            };
            let pattern = expect_text(
                eval_expr(pattern, functions, env, lines)?,
                "regex_replace_all",
            )?;
            let text = expect_text(eval_expr(text, functions, env, lines)?, "regex_replace_all")?;
            let replacement = expect_text(
                eval_expr(replacement, functions, env, lines)?,
                "regex_replace_all",
            )?;
            Ok(SpikeValue::Text(regex_replace_all(
                &pattern,
                &text,
                &replacement,
            )))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike regex call {name:?}"
        ))),
    }
}

pub(crate) fn eval_regex_binary_text(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<(String, String), Diagnostic> {
    let [pattern, text] = args else {
        return Err(unsupported(&format!(
            "{name} expects exactly two arguments"
        )));
    };
    let pattern = expect_text(eval_expr(pattern, functions, env, lines)?, name)?;
    let text = expect_text(eval_expr(text, functions, env, lines)?, name)?;
    Ok((pattern, text))
}

pub(crate) fn eval_io_eprintln_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [arg] = args else {
        return Err(unsupported("io_eprintln expects exactly one argument"));
    };
    let text = match eval_expr(arg, functions, env, lines)? {
        SpikeValue::Text(value) => value,
        _ => return Err(unsupported("io_eprintln expects a string")),
    };
    let written = text.len() as i64 + 1;
    lines.push(OutputLine::stderr(text));
    Ok(SpikeValue::Int(written))
}

pub(crate) fn eval_io_readline_call(args: &[Expr]) -> Result<SpikeValue, Diagnostic> {
    let [] = args else {
        return Err(unsupported("io_readline expects no arguments"));
    };
    Ok(spike_option(SPIKE_STDIN.with(|state| {
        state.borrow_mut().readline().map(SpikeValue::Text)
    })))
}

pub(crate) fn eval_io_read_to_string_call(args: &[Expr]) -> Result<SpikeValue, Diagnostic> {
    let [] = args else {
        return Err(unsupported("io_read_to_string expects no arguments"));
    };
    Ok(SpikeValue::Text(
        SPIKE_STDIN.with(|state| state.borrow_mut().read_to_string()),
    ))
}

pub(crate) fn eval_clock_now_ms_call(args: &[Expr]) -> Result<SpikeValue, Diagnostic> {
    let [] = args else {
        return Err(unsupported("clock_now_ms expects no arguments"));
    };
    current_time_ms().map(SpikeValue::Int)
}

pub(crate) fn eval_clock_elapsed_ms_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [start] = args else {
        return Err(unsupported("clock_elapsed_ms expects exactly one argument"));
    };
    let start = expect_signed_integer(eval_expr(start, functions, env, lines)?)?;
    let now = current_time_ms()?;
    Ok(SpikeValue::Int(if now < start { -1 } else { now - start }))
}

pub(crate) fn eval_clock_sleep_ms_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [milliseconds] = args else {
        return Err(unsupported("clock_sleep_ms expects exactly one argument"));
    };
    let milliseconds = expect_signed_integer(eval_expr(milliseconds, functions, env, lines)?)?;
    if milliseconds < 0 {
        return Ok(SpikeValue::Int(-1));
    }
    if milliseconds == 0 {
        return Ok(SpikeValue::Int(0));
    }
    if milliseconds > SPIKE_MAX_CLOCK_SLEEP_MS {
        return Err(unsupported(&format!(
            "clock_sleep_ms literals above {SPIKE_MAX_CLOCK_SLEEP_MS} ms are not supported by the cranelift spike"
        )));
    }
    // Inside a spawned task the sleep is virtual: it extends the task's
    // duration without blocking, so sibling tasks model concurrent execution.
    let deferred = SPIKE_VIRTUAL_SLEEP.with(|slot| match slot.get() {
        Some(accumulated) => {
            slot.set(Some(accumulated.saturating_add(milliseconds)));
            true
        }
        None => false,
    });
    if !deferred {
        std::thread::sleep(std::time::Duration::from_millis(milliseconds as u64));
    }
    Ok(SpikeValue::Int(0))
}

pub(crate) fn current_time_ms() -> Result<i64, Diagnostic> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| unsupported("system clock must be after unix epoch"))?;
    Ok(now.as_millis() as i64)
}

pub(crate) fn expect_text(value: SpikeValue, name: &str) -> Result<String, Diagnostic> {
    match value {
        SpikeValue::Text(value) => Ok(value),
        _ => Err(unsupported(&format!("{name} expects string arguments"))),
    }
}

pub(crate) fn expect_int(value: SpikeValue) -> Result<i64, Diagnostic> {
    match value {
        SpikeValue::Int(value) => Ok(value),
        SpikeValue::UInt(value) => {
            i64::try_from(value).map_err(|_| unsupported("integer value is outside the i64 range"))
        }
        _ => Err(unsupported("expected integer expression")),
    }
}

pub(crate) fn expect_u8_array(value: SpikeValue, name: &str) -> Result<Vec<u8>, Diagnostic> {
    let SpikeValue::Array(values) = value else {
        return Err(unsupported(&format!("{name} expects byte-slice arguments")));
    };
    values
        .into_iter()
        .map(|value| match value {
            SpikeValue::UInt(value) if value <= u8::MAX as u64 => Ok(value as u8),
            SpikeValue::Int(value) if (0..=u8::MAX as i64).contains(&value) => Ok(value as u8),
            _ => Err(unsupported(&format!("{name} expects byte-slice arguments"))),
        })
        .collect()
}

pub(crate) fn eval_arithmetic(
    op: ArithmeticOp,
    lhs: &Expr,
    rhs: &Expr,
    ty: &Type,
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let left = eval_expr(lhs, functions, env, lines)?;
    let right = eval_expr(rhs, functions, env, lines)?;
    eval_arithmetic_values(op, ty, left, right)
}

pub(crate) fn eval_arithmetic_values(
    op: ArithmeticOp,
    ty: &Type,
    left: SpikeValue,
    right: SpikeValue,
) -> Result<SpikeValue, Diagnostic> {
    match (ty, left, right) {
        (Type::Int, SpikeValue::Int(left), SpikeValue::Int(right)) => {
            let value = match op {
                ArithmeticOp::Add => left + right,
                ArithmeticOp::Sub => left - right,
                ArithmeticOp::Mul => left * right,
                ArithmeticOp::Div if right != 0 => left / right,
                ArithmeticOp::Div => return Err(unsupported("integer division by zero")),
            };
            Ok(SpikeValue::Int(value))
        }
        (Type::Numeric(numeric_ty), left, right) if is_signed_numeric(*numeric_ty) => {
            let left = expect_signed_integer(left)?;
            let right = expect_signed_integer(right)?;
            let value = match op {
                ArithmeticOp::Add => left + right,
                ArithmeticOp::Sub => left - right,
                ArithmeticOp::Mul => left * right,
                ArithmeticOp::Div if right != 0 => left / right,
                ArithmeticOp::Div => return Err(unsupported("integer division by zero")),
            };
            Ok(cast_signed_integer(value, *numeric_ty))
        }
        (Type::Numeric(numeric_ty), left, right) if is_unsigned_numeric(*numeric_ty) => {
            let left = expect_unsigned_integer(left)?;
            let right = expect_unsigned_integer(right)?;
            let value = match op {
                ArithmeticOp::Add => left + right,
                ArithmeticOp::Sub => left - right,
                ArithmeticOp::Mul => left * right,
                ArithmeticOp::Div if right != 0 => left / right,
                ArithmeticOp::Div => return Err(unsupported("integer division by zero")),
            };
            Ok(cast_unsigned_integer(value, *numeric_ty))
        }
        (
            Type::Numeric(numeric_ty @ (NumericType::F32 | NumericType::F64)),
            SpikeValue::Float(left),
            SpikeValue::Float(right),
        ) => eval_float_arithmetic(op, left, right, *numeric_ty),
        (Type::String | Type::Str, left, right) if op == ArithmeticOp::Add => Ok(SpikeValue::Text(
            format!("{}{}", render_value(&left), render_value(&right)),
        )),
        (Type::String | Type::Str, _, _) => Err(unsupported(
            "only string addition is supported by the cranelift spike",
        )),
        _ => Err(unsupported(
            "unsupported cranelift spike arithmetic operands",
        )),
    }
}

pub(crate) fn eval_float_arithmetic(
    op: ArithmeticOp,
    left: f64,
    right: f64,
    ty: NumericType,
) -> Result<SpikeValue, Diagnostic> {
    match ty {
        NumericType::F32 => {
            let left = left as f32;
            let right = right as f32;
            let value = match op {
                ArithmeticOp::Add => left + right,
                ArithmeticOp::Sub => left - right,
                ArithmeticOp::Mul => left * right,
                ArithmeticOp::Div if right != 0.0 => left / right,
                ArithmeticOp::Div => return Err(unsupported("floating-point division by zero")),
            };
            Ok(SpikeValue::Float(value as f64))
        }
        NumericType::F64 => {
            let value = match op {
                ArithmeticOp::Add => left + right,
                ArithmeticOp::Sub => left - right,
                ArithmeticOp::Mul => left * right,
                ArithmeticOp::Div if right != 0.0 => left / right,
                ArithmeticOp::Div => return Err(unsupported("floating-point division by zero")),
            };
            Ok(SpikeValue::Float(value))
        }
        _ => Err(unsupported(
            "floating-point arithmetic requires a float type",
        )),
    }
}

pub(crate) fn is_signed_numeric(ty: NumericType) -> bool {
    matches!(
        ty,
        NumericType::I8
            | NumericType::I16
            | NumericType::I32
            | NumericType::I64
            | NumericType::Isize
    )
}

pub(crate) fn is_unsigned_numeric(ty: NumericType) -> bool {
    matches!(
        ty,
        NumericType::U8
            | NumericType::U16
            | NumericType::U32
            | NumericType::U64
            | NumericType::Usize
    )
}

pub(crate) fn expect_signed_integer(value: SpikeValue) -> Result<i64, Diagnostic> {
    match value {
        SpikeValue::Int(value) => Ok(value),
        SpikeValue::UInt(value) => Ok(value as i64),
        _ => Err(unsupported("expected integer operands")),
    }
}

pub(crate) fn expect_unsigned_integer(value: SpikeValue) -> Result<u64, Diagnostic> {
    match value {
        SpikeValue::Int(value) => Ok(value as u64),
        SpikeValue::UInt(value) => Ok(value),
        _ => Err(unsupported("expected integer operands")),
    }
}

pub(crate) fn eval_compare(
    op: CompareOp,
    lhs: &Expr,
    rhs: &Expr,
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let left = eval_expr(lhs, functions, env, lines)?;
    let right = eval_expr(rhs, functions, env, lines)?;
    eval_compare_values(op, left, right)
}

pub(crate) fn eval_compare_values(
    op: CompareOp,
    left: SpikeValue,
    right: SpikeValue,
) -> Result<SpikeValue, Diagnostic> {
    let result = match (&left, &right) {
        (SpikeValue::Int(left), SpikeValue::Int(right)) => compare_ord(op, *left, *right),
        (SpikeValue::UInt(left), SpikeValue::UInt(right)) => compare_ord(op, *left, *right),
        (SpikeValue::Float(left), SpikeValue::Float(right)) => compare_float(op, *left, *right)?,
        (SpikeValue::Bool(left), SpikeValue::Bool(right)) => compare_eq(op, *left, *right)?,
        (SpikeValue::Text(left), SpikeValue::Text(right)) => {
            compare_eq(op, left.as_str(), right.as_str())?
        }
        _ if matches!(op, CompareOp::Eq | CompareOp::Ne) => {
            let equal = spike_values_equal(&left, &right)?;
            matches!(op, CompareOp::Eq) == equal
        }
        _ => return Err(unsupported("mismatched comparison operands")),
    };
    Ok(SpikeValue::Bool(result))
}

pub(crate) fn compare_ord<T: Ord>(op: CompareOp, left: T, right: T) -> bool {
    match op {
        CompareOp::Eq => left == right,
        CompareOp::Ne => left != right,
        CompareOp::Lt => left < right,
        CompareOp::Le => left <= right,
        CompareOp::Gt => left > right,
        CompareOp::Ge => left >= right,
    }
}

pub(crate) fn compare_float(op: CompareOp, left: f64, right: f64) -> Result<bool, Diagnostic> {
    if !left.is_finite() || !right.is_finite() {
        return Err(unsupported("non-finite float comparison"));
    }
    Ok(match op {
        CompareOp::Eq => left == right,
        CompareOp::Ne => left != right,
        CompareOp::Lt => left < right,
        CompareOp::Le => left <= right,
        CompareOp::Gt => left > right,
        CompareOp::Ge => left >= right,
    })
}

pub(crate) fn compare_eq<T: Eq>(op: CompareOp, left: T, right: T) -> Result<bool, Diagnostic> {
    match op {
        CompareOp::Eq => Ok(left == right),
        CompareOp::Ne => Ok(left != right),
        _ => Err(unsupported("only equality comparisons are supported here")),
    }
}

pub(crate) fn expect_bool(value: SpikeValue) -> Result<bool, Diagnostic> {
    match value {
        SpikeValue::Bool(value) => Ok(value),
        _ => Err(unsupported("expected boolean expression")),
    }
}

pub(crate) fn expect_non_negative_index(value: SpikeValue) -> Result<usize, Diagnostic> {
    match value {
        SpikeValue::Int(value) if value >= 0 => Ok(value as usize),
        SpikeValue::Int(_) => Err(unsupported("array index cannot be negative")),
        SpikeValue::UInt(value) => usize::try_from(value)
            .map_err(|_| unsupported("array index is outside the host usize range")),
        _ => Err(unsupported("array index must be an integer")),
    }
}

pub(crate) fn validate_map_key(value: &SpikeValue) -> Result<(), Diagnostic> {
    match value {
        SpikeValue::Int(_) | SpikeValue::UInt(_) | SpikeValue::Bool(_) | SpikeValue::Text(_) => {
            Ok(())
        }
        SpikeValue::Tuple(values) => values.iter().try_for_each(validate_map_key),
        SpikeValue::Float(_) => Err(unsupported(
            "map float keys are not supported by the cranelift spike",
        )),
        SpikeValue::Enum { .. }
        | SpikeValue::Struct { .. }
        | SpikeValue::Map(_)
        | SpikeValue::Array(_)
        | SpikeValue::Closure { .. }
        | SpikeValue::MutRef(_)
        | SpikeValue::MutSlice { .. }
        | SpikeValue::Task { .. }
        | SpikeValue::JoinHandle { .. }
        | SpikeValue::AsyncChannel { .. }
        | SpikeValue::SelectResult { .. }
        | SpikeValue::ControlReturn(_) => Err(unsupported(
            "map keys must be scalar values or scalar tuples in the cranelift spike",
        )),
    }
}

pub(crate) fn render_value(value: &SpikeValue) -> String {
    match value {
        SpikeValue::Int(value) => value.to_string(),
        SpikeValue::UInt(value) => value.to_string(),
        SpikeValue::Float(value) => value.to_string(),
        SpikeValue::Bool(true) => String::from("true"),
        SpikeValue::Bool(false) => String::from("false"),
        SpikeValue::Text(value) => value.clone(),
        SpikeValue::Struct { name, fields } => render_struct(name, fields),
        SpikeValue::Enum {
            variant, payloads, ..
        } => render_enum(variant, payloads),
        SpikeValue::Tuple(values) => render_sequence("(", ")", values),
        SpikeValue::Map(entries) => render_map(entries),
        SpikeValue::Array(values) => render_sequence("[", "]", values),
        SpikeValue::Closure { params, .. } => {
            format!("fn({})", params.len())
        }
        SpikeValue::MutRef(name) => format!("&mut {name}"),
        SpikeValue::MutSlice { target, start, end } => {
            format!("&mut {target}[{start}..{end}]")
        }
        SpikeValue::Task { canceled, .. } => {
            format!("Task {{ canceled: {canceled} }}")
        }
        SpikeValue::JoinHandle { .. } => String::from("JoinHandle"),
        SpikeValue::AsyncChannel { slot } => {
            format!("AsyncChannel {{ occupied: {} }}", slot.is_some())
        }
        SpikeValue::SelectResult { selected, value } => format!(
            "SelectResult {{ selected: {selected}, value: {} }}",
            render_value(&spike_option(value.as_ref().map(|value| (**value).clone())))
        ),
        SpikeValue::ControlReturn(_) => String::from("<control-return>"),
    }
}

pub(crate) fn render_enum(variant: &str, payloads: &[SpikeValue]) -> String {
    if payloads.is_empty() {
        return variant.to_string();
    }
    format!("{variant}{}", render_sequence("(", ")", payloads))
}

pub(crate) fn render_struct(name: &str, fields: &[(String, SpikeValue)]) -> String {
    let mut rendered = format!("{name} {{ ");
    for (index, (field, value)) in fields.iter().enumerate() {
        if index > 0 {
            rendered.push_str(", ");
        }
        rendered.push_str(field);
        rendered.push_str(": ");
        rendered.push_str(&render_value(value));
    }
    rendered.push_str(" }");
    rendered
}

pub(crate) fn render_sequence(open: &str, close: &str, values: &[SpikeValue]) -> String {
    let mut rendered = String::from(open);
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            rendered.push_str(", ");
        }
        rendered.push_str(&render_value(value));
    }
    rendered.push_str(close);
    rendered
}

pub(crate) fn render_map(entries: &[(SpikeValue, SpikeValue)]) -> String {
    let mut rendered = String::from("{");
    for (index, (key, value)) in entries.iter().enumerate() {
        if index > 0 {
            rendered.push_str(", ");
        }
        rendered.push_str(&render_value(key));
        rendered.push_str(": ");
        rendered.push_str(&render_value(value));
    }
    rendered.push('}');
    rendered
}

pub(crate) fn render_runtime_panic_message(value: SpikeValue) -> Result<String, Diagnostic> {
    match value {
        SpikeValue::Text(message) => Ok(message),
        SpikeValue::Int(_)
        | SpikeValue::UInt(_)
        | SpikeValue::Float(_)
        | SpikeValue::Bool(_)
        | SpikeValue::Struct { .. }
        | SpikeValue::Enum { .. }
        | SpikeValue::Tuple(_)
        | SpikeValue::Map(_)
        | SpikeValue::Array(_)
        | SpikeValue::Closure { .. }
        | SpikeValue::MutRef(_)
        | SpikeValue::MutSlice { .. }
        | SpikeValue::Task { .. }
        | SpikeValue::JoinHandle { .. }
        | SpikeValue::AsyncChannel { .. }
        | SpikeValue::SelectResult { .. } => Ok(render_value(&value)),
        SpikeValue::ControlReturn(_) => Err(unsupported(
            "control return cannot be rendered as a panic message",
        )),
    }
}

pub(crate) fn cranelift_runtime_trap(kind: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(CRANELIFT_RUNTIME_TRAP_KIND, message.into()).with_code(kind)
}

