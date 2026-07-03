use super::*;

pub(super) fn lower_map_lookup_intrinsic(
    name: &str,
    type_args: &[syntax::TypeName],
    args: &[syntax::Expr],
    line: usize,
    column: usize,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Expr, Diagnostic> {
    if !type_args.is_empty() && type_args.len() != 2 {
        return Err(Diagnostic::new(
            "type",
            format!(
                "{name} expects 0 or 2 type arguments, got {}",
                type_args.len()
            ),
        )
        .with_span(line, column));
    }
    let expected_args = if matches!(name, "map_keys" | "keys") {
        1
    } else if name == "get_or_default" {
        3
    } else {
        2
    };
    if args.len() != expected_args {
        return Err(Diagnostic::new(
            "type",
            format!(
                "{name} expects {expected_args} arguments, got {}",
                args.len()
            ),
        )
        .with_span(line, column));
    }
    let lowered_map = lower_expr(&args[0], env, ctx)?;
    let Type::Map(key_ty, value_ty) = lowered_map.ty() else {
        return Err(Diagnostic::new(
            "type",
            format!("{name} expects a map value, got {}", lowered_map.ty()),
        )
        .with_span(args[0].line(), args[0].column()));
    };
    let key_ty = (*key_ty.clone()).clone();
    let value_ty = (*value_ty.clone()).clone();
    if let [expected_key, expected_value] = type_args {
        let expected_key = lower_type(
            expected_key,
            ctx.structs,
            ctx.enums,
            ctx.aliases,
            ctx.consts,
            line,
            column,
        )?;
        let expected_value = lower_type(
            expected_value,
            ctx.structs,
            ctx.enums,
            ctx.aliases,
            ctx.consts,
            line,
            column,
        )?;
        if expected_key != key_ty || expected_value != value_ty {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "{name} type arguments expect {{{expected_key}: {expected_value}}}, got {}",
                    lowered_map.ty()
                ),
            )
            .with_span(line, column));
        }
    }
    if matches!(name, "map_keys" | "keys") {
        move_lowered_value(&lowered_map, env)?;
        return Ok(Expr::Call {
            span: SourceSpan::point(line, column),
            name: name.to_string(),
            args: vec![lowered_map],
            ty: Type::Array(Box::new(key_ty), None),
        });
    }
    let lowered_key = lower_expr_with_expected(&args[1], Some(&key_ty), env, ctx)?;
    if lowered_key.ty() != &key_ty {
        return Err(Diagnostic::new(
            "type",
            format!("{name} expects key type {key_ty}, got {}", lowered_key.ty()),
        )
        .with_span(args[1].line(), args[1].column()));
    }
    let mut lowered_args = vec![lowered_map, lowered_key];
    if name == "get_or_default" {
        let lowered_default = lower_expr_with_expected(&args[2], Some(&value_ty), env, ctx)?;
        if lowered_default.ty() != &value_ty {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "{name} expects default type {value_ty}, got {}",
                    lowered_default.ty()
                ),
            )
            .with_span(args[2].line(), args[2].column()));
        }
        move_lowered_value(&lowered_default, env)?;
        lowered_args.push(lowered_default);
    }
    move_lowered_value(&lowered_args[0], env)?;
    move_lowered_value(&lowered_args[1], env)?;
    Ok(Expr::Call {
        span: SourceSpan::point(line, column),
        name: name.to_string(),
        args: lowered_args,
        ty: match name {
            "map_get" | "get" => Type::Option(Box::new(value_ty)),
            "get_or_default" => value_ty,
            _ => Type::Bool,
        },
    })
}
