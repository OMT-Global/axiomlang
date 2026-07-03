use super::*;

pub(super) fn resolve_variant<'a>(
    name: &str,
    expected: Option<&Type>,
    ctx: &'a LowerContext<'_>,
    line: usize,
    column: usize,
) -> Result<Option<&'a VariantInfo>, Diagnostic> {
    let Some(candidates) = ctx.variants.get(name) else {
        return Ok(None);
    };
    if let Some(Type::Enum(expected_enum)) = expected {
        return Ok(candidates
            .iter()
            .find(|variant| &variant.enum_name == expected_enum));
    }
    if candidates.len() == 1 {
        return Ok(candidates.first());
    }
    Err(Diagnostic::new(
        "type",
        format!("enum variant {name:?} is ambiguous without an expected enum type"),
    )
    .with_span(line, column))
}

pub(super) fn lower_variant_constructor(
    name: &str,
    args: &[syntax::Expr],
    line: usize,
    column: usize,
    variant: &VariantInfo,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Expr, Diagnostic> {
    if !variant.payload_names.is_empty() {
        return Err(Diagnostic::new(
            "type",
            format!("enum variant {name:?} requires named payload fields"),
        )
        .with_span(line, column));
    }
    if variant.payload_tys.is_empty() {
        return Err(Diagnostic::new(
            "type",
            format!("enum variant {name:?} does not take arguments"),
        )
        .with_span(line, column));
    }
    if args.len() != variant.payload_tys.len() {
        return Err(Diagnostic::new(
            "type",
            format!(
                "enum variant {name:?} expects {} arguments, got {}",
                variant.payload_tys.len(),
                args.len()
            ),
        )
        .with_span(line, column));
    }
    let mut lowered_payloads = Vec::new();
    for (arg, expected) in args.iter().zip(variant.payload_tys.iter()) {
        let lowered = lower_call_arg_with_expected(arg, Some(expected), env, ctx, false)?;
        if !type_assignable_to(lowered.ty(), expected) {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "enum variant {name:?} expects payload type {expected}, got {}",
                    lowered.ty()
                ),
            )
            .with_span(arg.line(), arg.column()));
        }
        if !expected.is_copy() {
            move_lowered_value(&lowered, env)?;
        }
        lowered_payloads.push(lowered);
    }
    Ok(Expr::EnumVariant {
        enum_name: variant.enum_name.clone(),
        variant: name.to_string(),
        field_names: Vec::new(),
        payloads: lowered_payloads,
        ty: Type::Enum(variant.enum_name.clone()),
    })
}

pub(super) fn lower_named_variant_constructor(
    name: &str,
    fields: &[syntax::StructFieldValue],
    line: usize,
    column: usize,
    variant: &VariantInfo,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Expr, Diagnostic> {
    let mut lowered_fields = HashMap::new();
    for field in fields {
        let Some(position) = variant
            .payload_names
            .iter()
            .position(|payload_name| payload_name == &field.name)
        else {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "enum variant {name:?} has no named payload {:?}",
                    field.name
                ),
            )
            .with_span(field.line, field.column));
        };
        if lowered_fields.contains_key(&field.name) {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "duplicate named payload {:?} in enum variant literal {name:?}",
                    field.name
                ),
            )
            .with_span(field.line, field.column));
        }
        let expected = &variant.payload_tys[position];
        let lowered = lower_expr_with_expected(&field.expr, Some(expected), env, ctx)?;
        if !type_assignable_to(lowered.ty(), expected) {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "enum variant {name:?} payload {:?} expects {expected}, got {}",
                    field.name,
                    lowered.ty()
                ),
            )
            .with_span(field.line, field.column));
        }
        if !expected.is_copy() {
            move_lowered_owner_value(&lowered, env)?;
        }
        lowered_fields.insert(field.name.clone(), lowered);
    }
    let mut ordered_payloads = Vec::new();
    for payload_name in &variant.payload_names {
        let lowered = lowered_fields.remove(payload_name).ok_or_else(|| {
            Diagnostic::new(
                "type",
                format!(
                    "enum variant literal {name:?} is missing named payload {:?}",
                    payload_name
                ),
            )
            .with_span(line, column)
        })?;
        ordered_payloads.push(lowered);
    }
    Ok(Expr::EnumVariant {
        enum_name: variant.enum_name.clone(),
        variant: name.to_string(),
        field_names: variant.payload_names.clone(),
        payloads: ordered_payloads,
        ty: Type::Enum(variant.enum_name.clone()),
    })
}
