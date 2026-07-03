use super::*;

pub(super) fn lower_async_runtime_intrinsic(
    name: &str,
    type_args: &[syntax::TypeName],
    args: &[syntax::Expr],
    line: usize,
    column: usize,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Expr, Diagnostic> {
    require_capability(ctx.capabilities, CapabilityKind::Async, name, line, column)?;
    if type_args.len() != 1 {
        return Err(Diagnostic::new(
            "type",
            format!(
                "{name} expects 1 explicit type argument, got {}",
                type_args.len()
            ),
        )
        .with_span(line, column));
    }
    let value_ty = lower_type(
        &type_args[0],
        ctx.structs,
        ctx.enums,
        ctx.aliases,
        ctx.consts,
        line,
        column,
    )?;
    let expected_len = match name {
        "async_channel" => 0,
        "async_ready"
        | "async_is_canceled"
        | "async_spawn"
        | "async_join"
        | "async_cancel"
        | "async_recv"
        | "async_selected"
        | "async_selected_value" => 1,
        "async_timeout" | "async_send" | "async_select" => 2,
        _ => unreachable!("async runtime intrinsic checked before lowering"),
    };
    if args.len() != expected_len {
        return Err(Diagnostic::new(
            "type",
            format!(
                "{name} expects {expected_len} arguments, got {}",
                args.len()
            ),
        )
        .with_span(line, column));
    }

    let lowered_args = match name {
        "async_ready" => {
            let value = lower_expr_with_expected(&args[0], Some(&value_ty), env, ctx)?;
            if value.ty() != &value_ty {
                return Err(Diagnostic::new(
                    "type",
                    format!("{name} expects value type {value_ty}, got {}", value.ty()),
                )
                .with_span(args[0].line(), args[0].column()));
            }
            move_lowered_value(&value, env)?;
            vec![value]
        }
        "async_spawn" | "async_cancel" | "async_is_canceled" => {
            let expected = Type::Task(Box::new(value_ty.clone()));
            let task = lower_expr_with_expected(&args[0], Some(&expected), env, ctx)?;
            if task.ty() != &expected {
                return Err(Diagnostic::new(
                    "type",
                    format!("{name} expects {expected}, got {}", task.ty()),
                )
                .with_span(args[0].line(), args[0].column()));
            }
            move_lowered_value(&task, env)?;
            vec![task]
        }
        "async_join" => {
            let expected = Type::JoinHandle(Box::new(value_ty.clone()));
            let handle = lower_expr_with_expected(&args[0], Some(&expected), env, ctx)?;
            if handle.ty() != &expected {
                return Err(Diagnostic::new(
                    "type",
                    format!("{name} expects {expected}, got {}", handle.ty()),
                )
                .with_span(args[0].line(), args[0].column()));
            }
            move_lowered_value(&handle, env)?;
            vec![handle]
        }
        "async_timeout" => {
            let expected = Type::Task(Box::new(value_ty.clone()));
            let task = lower_expr_with_expected(&args[0], Some(&expected), env, ctx)?;
            let ms = lower_expr_with_expected(&args[1], Some(&Type::Int), env, ctx)?;
            if task.ty() != &expected || ms.ty() != &Type::Int {
                return Err(Diagnostic::new(
                    "type",
                    format!("{name} expects Task<{value_ty}> and int arguments"),
                )
                .with_span(line, column));
            }
            move_lowered_value(&task, env)?;
            vec![task, ms]
        }
        "async_channel" => Vec::new(),
        "async_send" => {
            let channel_ty = Type::AsyncChannel(Box::new(value_ty.clone()));
            let channel = lower_expr_with_expected(&args[0], Some(&channel_ty), env, ctx)?;
            let value = lower_expr_with_expected(&args[1], Some(&value_ty), env, ctx)?;
            if channel.ty() != &channel_ty || value.ty() != &value_ty {
                return Err(Diagnostic::new(
                    "type",
                    format!("{name} expects AsyncChannel<{value_ty}> and {value_ty} arguments"),
                )
                .with_span(line, column));
            }
            move_lowered_value(&channel, env)?;
            move_lowered_value(&value, env)?;
            vec![channel, value]
        }
        "async_recv" => {
            let channel_ty = Type::AsyncChannel(Box::new(value_ty.clone()));
            let channel = lower_expr_with_expected(&args[0], Some(&channel_ty), env, ctx)?;
            if channel.ty() != &channel_ty {
                return Err(Diagnostic::new(
                    "type",
                    format!("{name} expects {channel_ty}, got {}", channel.ty()),
                )
                .with_span(args[0].line(), args[0].column()));
            }
            move_lowered_value(&channel, env)?;
            vec![channel]
        }
        "async_select" => {
            let task_ty = Type::Task(Box::new(Type::Option(Box::new(value_ty.clone()))));
            let left = lower_expr_with_expected(&args[0], Some(&task_ty), env, ctx)?;
            let right = lower_expr_with_expected(&args[1], Some(&task_ty), env, ctx)?;
            if left.ty() != &task_ty || right.ty() != &task_ty {
                return Err(Diagnostic::new(
                    "type",
                    format!("{name} expects two {task_ty} arguments"),
                )
                .with_span(line, column));
            }
            move_lowered_value(&left, env)?;
            move_lowered_value(&right, env)?;
            vec![left, right]
        }
        "async_selected" | "async_selected_value" => {
            let result_ty = Type::SelectResult(Box::new(value_ty.clone()));
            let result = lower_expr_with_expected(&args[0], Some(&result_ty), env, ctx)?;
            if result.ty() != &result_ty {
                return Err(Diagnostic::new(
                    "type",
                    format!("{name} expects {result_ty}, got {}", result.ty()),
                )
                .with_span(args[0].line(), args[0].column()));
            }
            move_lowered_value(&result, env)?;
            vec![result]
        }
        _ => unreachable!("async runtime intrinsic checked before lowering"),
    };

    let ty = match name {
        "async_ready" | "async_cancel" | "async_join" => Type::Task(Box::new(value_ty)),
        "async_spawn" => Type::JoinHandle(Box::new(value_ty)),
        "async_is_canceled" => Type::Bool,
        "async_timeout" => Type::Task(Box::new(Type::Option(Box::new(value_ty)))),
        "async_channel" => Type::AsyncChannel(Box::new(value_ty)),
        "async_send" => Type::Task(Box::new(Type::AsyncChannel(Box::new(value_ty)))),
        "async_recv" => Type::Task(Box::new(Type::Option(Box::new(value_ty)))),
        "async_select" => Type::Task(Box::new(Type::SelectResult(Box::new(value_ty)))),
        "async_selected" => Type::Int,
        "async_selected_value" => Type::Option(Box::new(value_ty)),
        _ => unreachable!("async runtime intrinsic checked before lowering"),
    };
    Ok(Expr::Call {
        span: SourceSpan::point(line, column),
        name: name.to_string(),
        args: lowered_args,
        ty,
    })
}
