use super::*;

#[derive(Debug, Clone)]
pub(super) struct MatchArmInput {
    pub(super) variant: String,
    pub(super) bindings: Vec<String>,
    pub(super) is_named: bool,
    pub(super) ignore_payloads: bool,
    pub(super) body: Vec<syntax::Stmt>,
    pub(super) line: usize,
    pub(super) column: usize,
}

#[derive(Debug, Clone)]
pub(super) struct MatchExprArmInput {
    variant: String,
    bindings: Vec<String>,
    is_named: bool,
    expr: syntax::Expr,
    line: usize,
    column: usize,
}

impl From<&syntax::MatchArm> for MatchArmInput {
    fn from(arm: &syntax::MatchArm) -> Self {
        Self {
            variant: arm.variant.clone(),
            bindings: arm.bindings.clone(),
            is_named: arm.is_named,
            ignore_payloads: false,
            body: arm.body.clone(),
            line: arm.line,
            column: arm.column,
        }
    }
}

impl From<&syntax::MatchExprArm> for MatchExprArmInput {
    fn from(arm: &syntax::MatchExprArm) -> Self {
        Self {
            variant: arm.variant.clone(),
            bindings: arm.bindings.clone(),
            is_named: arm.is_named,
            expr: arm.expr.clone(),
            line: arm.line,
            column: arm.column,
        }
    }
}

pub(super) fn lower_match_stmt(
    expr: &syntax::Expr,
    arms: Vec<MatchArmInput>,
    line: usize,
    column: usize,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Stmt, Diagnostic> {
    let lowered_expr = lower_expr(expr, env, ctx)?;
    let match_borrowed_owners = expr_borrowed_owners(&lowered_expr, env, ctx);
    let match_borrow_kind = borrow_kind_for_type(lowered_expr.ty(), ctx.structs, ctx.enums);
    let reuse_existing_match_binding =
        matches!(lowered_expr, Expr::VarRef { .. }) && !match_borrowed_owners.is_empty();
    if let Some(borrow_kind) = match_borrow_kind
        && !reuse_existing_match_binding
    {
        increment_active_borrows(&match_borrowed_owners, env, borrow_kind, line, column)?;
    }
    if matches!(lowered_expr, Expr::VarRef { .. }) && !lowered_expr.ty().is_copy() {
        move_lowered_owner_value(&lowered_expr, env)?;
    }
    let Some((enum_name, variant_defs)) = match_variants(lowered_expr.ty(), ctx) else {
        return lower_const_match_stmt(
            lowered_expr,
            arms,
            line,
            column,
            env,
            ctx,
            match_borrow_kind,
            match_borrowed_owners,
            reuse_existing_match_binding,
        );
    };
    let before = env.clone();
    let mut seen = HashMap::new();
    let mut lowered_arms = Vec::new();
    let mut arm_states = Vec::new();
    let mut ignored_body_cache: HashMap<String, (Vec<Stmt>, HashMap<String, Binding>, bool)> =
        HashMap::new();
    for arm in arms {
        let variant_def = variant_defs
            .iter()
            .find(|variant| variant.name == arm.variant)
            .ok_or_else(|| {
                Diagnostic::new(
                    "type",
                    message_with_suggestion(
                        format!("enum {enum_name:?} has no variant {:?}", arm.variant),
                        &arm.variant,
                        variant_defs.iter().map(|variant| variant.name.as_str()),
                    ),
                )
                .with_span(arm.line, arm.column)
            })?;
        if seen.insert(arm.variant.clone(), ()).is_some() {
            return Err(
                Diagnostic::new("type", format!("duplicate match arm {:?}", arm.variant))
                    .with_span(arm.line, arm.column),
            );
        }
        let mut arm_env = before.clone();
        let binding_tys = if arm.ignore_payloads {
            if !arm.bindings.is_empty() {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "match arm {:?} cannot both ignore payloads and bind names",
                        arm.variant
                    ),
                )
                .with_span(arm.line, arm.column));
            }
            Vec::new()
        } else if arm.is_named {
            if variant_def.payload_names.is_empty() {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "match arm {:?} uses named bindings, but variant {:?} is positional",
                        arm.variant, arm.variant
                    ),
                )
                .with_span(arm.line, arm.column));
            }
            if arm.bindings.len() != variant_def.payload_names.len() {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "match arm {:?} expects {} named bindings, got {}",
                        arm.variant,
                        variant_def.payload_names.len(),
                        arm.bindings.len()
                    ),
                )
                .with_span(arm.line, arm.column));
            } else {
                let mut seen_named = HashMap::new();
                let mut payload_tys = Vec::new();
                for binding in &arm.bindings {
                    let Some(position) = variant_def
                        .payload_names
                        .iter()
                        .position(|name| name == binding)
                    else {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "match arm {:?} has no named payload {:?}",
                                arm.variant, binding
                            ),
                        )
                        .with_span(arm.line, arm.column));
                    };
                    if seen_named.insert(binding.clone(), ()).is_some() {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "match arm {:?} repeats named payload {:?}",
                                arm.variant, binding
                            ),
                        )
                        .with_span(arm.line, arm.column));
                    }
                    payload_tys.push(variant_def.payload_tys[position].clone());
                }
                payload_tys
            }
        } else {
            if !variant_def.payload_names.is_empty() {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "match arm {:?} must use named bindings for variant {:?}",
                        arm.variant, arm.variant
                    ),
                )
                .with_span(arm.line, arm.column));
            }
            if arm.bindings.len() != variant_def.payload_tys.len() {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "match arm {:?} expects {} bindings, got {}",
                        arm.variant,
                        variant_def.payload_tys.len(),
                        arm.bindings.len()
                    ),
                )
                .with_span(arm.line, arm.column));
            } else {
                variant_def.payload_tys.clone()
            }
        };
        let mut arm_borrow_region_facts = Vec::new();
        for (binding_index, (binding, payload_ty)) in
            arm.bindings.iter().zip(binding_tys.iter()).enumerate()
        {
            let payload_index = if arm.is_named {
                variant_def
                    .payload_names
                    .iter()
                    .position(|name| name == binding)
                    .expect("named match binding already validated")
            } else {
                binding_index
            };
            if ctx.functions.contains_key(binding) {
                return Err(Diagnostic::new(
                    "type",
                    format!("match binding {binding:?} conflicts with a function name"),
                )
                .with_span(arm.line, arm.column));
            }
            if arm_env.contains_key(binding) {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "match binding {binding:?} reuses an existing name in the current scope"
                    ),
                )
                .with_span(arm.line, arm.column));
            }
            let borrowed_owners = match_binding_borrowed_owners(
                &lowered_expr,
                &arm.variant,
                binding,
                binding_index,
                payload_ty,
                &before,
                ctx,
            );
            arm_borrow_region_facts.extend(borrow_region_facts_for_enum_payload_binding(
                binding,
                payload_ty,
                &borrowed_owners,
                &lowered_expr,
                &arm.variant,
                payload_index,
            ));
            arm_env.insert(
                binding.clone(),
                Binding {
                    ty: payload_ty.clone(),
                    moved: false,
                    moved_projections: HashSet::new(),
                    borrow_kind: borrow_kind_for_type(payload_ty, ctx.structs, ctx.enums),
                    borrow_origin: match_binding_borrow_origin(
                        &lowered_expr,
                        &arm.variant,
                        binding,
                        binding_index,
                        payload_ty,
                        &before,
                        ctx,
                    ),
                    net_origin: None,
                    borrowed_owners,
                    active_borrow_count: 0,
                    active_mut_borrow_count: 0,
                    active_borrows: HashMap::new(),
                },
            );
        }
        let (body, after, returns) = if arm.ignore_payloads && arm.bindings.is_empty() {
            let cache_key = format!("{:?}", arm.body);
            if let Some((body, after, returns)) = ignored_body_cache.get(&cache_key) {
                (body.clone(), after.clone(), *returns)
            } else {
                let lowered = lower_block(&arm.body, &mut arm_env, ctx)?;
                ignored_body_cache.insert(cache_key, lowered.clone());
                lowered
            }
        } else {
            lower_block(&arm.body, &mut arm_env, ctx)?
        };
        lowered_arms.push(MatchArm {
            enum_name: enum_name.clone(),
            variant: arm.variant.clone(),
            bindings: arm.bindings.clone(),
            is_named: arm.is_named,
            ignore_payloads: arm.ignore_payloads,
            borrow_region_facts: arm_borrow_region_facts,
            body,
        });
        arm_states.push((after, returns));
    }
    let missing = variant_defs
        .iter()
        .filter(|variant| !seen.contains_key(&variant.name))
        .map(|variant| variant.name.clone())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(Diagnostic::new(
            "type",
            format!(
                "match on {:?} is not exhaustive; missing {}",
                enum_name,
                missing.join(", ")
            ),
        )
        .with_span(line, column));
    }
    merge_match_state(env, &before, &arm_states);
    if let Some(borrow_kind) = match_borrow_kind
        && !reuse_existing_match_binding
    {
        release_active_borrow_owners(&match_borrowed_owners, env, borrow_kind);
    }
    Ok(Stmt::Match {
        expr: lowered_expr,
        arms: lowered_arms,
        span: SourceSpan::point(line, column),
    })
}

fn lower_const_match_stmt(
    lowered_expr: Expr,
    arms: Vec<MatchArmInput>,
    line: usize,
    column: usize,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
    match_borrow_kind: Option<BorrowKind>,
    match_borrowed_owners: HashSet<BorrowedOwner>,
    reuse_existing_match_binding: bool,
) -> Result<Stmt, Diagnostic> {
    if lowered_expr.ty() != &Type::Int {
        return Err(Diagnostic::new(
            "type",
            format!(
                "match expects an enum-like value or int const patterns, got {}",
                lowered_expr.ty()
            ),
        )
        .with_span(line, column));
    }
    let before = env.clone();
    let mut seen = HashMap::new();
    let mut lowered_arms = Vec::new();
    let mut arm_states = Vec::new();
    for arm in arms {
        if arm.is_named || !arm.bindings.is_empty() || arm.ignore_payloads {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "const match arm {:?} cannot bind payload values",
                    arm.variant
                ),
            )
            .with_span(arm.line, arm.column));
        }
        let value = resolve_const_match_int_pattern(&arm, ctx)?;
        if seen.insert(value, ()).is_some() {
            return Err(
                Diagnostic::new("type", format!("duplicate match arm {:?}", arm.variant))
                    .with_span(arm.line, arm.column),
            );
        }
        let mut arm_env = before.clone();
        let (body, after, returns) = lower_block(&arm.body, &mut arm_env, ctx)?;
        lowered_arms.push(MatchArm {
            enum_name: String::new(),
            variant: value.to_string(),
            bindings: Vec::new(),
            is_named: false,
            ignore_payloads: false,
            borrow_region_facts: Vec::new(),
            body,
        });
        arm_states.push((after, returns));
    }
    merge_match_state(env, &before, &arm_states);
    if let Some(borrow_kind) = match_borrow_kind
        && !reuse_existing_match_binding
    {
        release_active_borrow_owners(&match_borrowed_owners, env, borrow_kind);
    }
    Ok(Stmt::Match {
        expr: lowered_expr,
        arms: lowered_arms,
        span: SourceSpan::point(line, column),
    })
}

pub(super) fn lower_match_expr(
    expr: &syntax::Expr,
    arms: Vec<MatchExprArmInput>,
    expected: Option<&Type>,
    line: usize,
    column: usize,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Expr, Diagnostic> {
    let lowered_expr = lower_expr(expr, env, ctx)?;
    let Some((enum_name, variant_defs)) = match_variants(lowered_expr.ty(), ctx) else {
        return Err(Diagnostic::new(
            "type",
            format!(
                "match expression expects an enum-like value, got {}",
                lowered_expr.ty()
            ),
        )
        .with_span(line, column));
    };
    let match_borrowed_owners = expr_borrowed_owners(&lowered_expr, env, ctx);
    let match_borrow_kind = borrow_kind_for_type(lowered_expr.ty(), ctx.structs, ctx.enums);
    let reuse_existing_match_binding =
        matches!(lowered_expr, Expr::VarRef { .. }) && !match_borrowed_owners.is_empty();
    if let Some(borrow_kind) = match_borrow_kind
        && !reuse_existing_match_binding
    {
        increment_active_borrows(&match_borrowed_owners, env, borrow_kind, line, column)?;
    }
    if matches!(lowered_expr, Expr::VarRef { .. }) && !lowered_expr.ty().is_copy() {
        move_lowered_owner_value(&lowered_expr, env)?;
    }

    let before = env.clone();
    let mut seen = HashMap::new();
    let mut lowered_arms = Vec::new();
    let mut arm_states = Vec::new();
    let mut result_ty = expected.cloned();
    for arm in arms {
        let variant_def = variant_defs
            .iter()
            .find(|variant| variant.name == arm.variant)
            .ok_or_else(|| {
                Diagnostic::new(
                    "type",
                    message_with_suggestion(
                        format!("enum {enum_name:?} has no variant {:?}", arm.variant),
                        &arm.variant,
                        variant_defs.iter().map(|variant| variant.name.as_str()),
                    ),
                )
                .with_span(arm.line, arm.column)
            })?;
        if seen.insert(arm.variant.clone(), ()).is_some() {
            return Err(
                Diagnostic::new("type", format!("duplicate match arm {:?}", arm.variant))
                    .with_span(arm.line, arm.column),
            );
        }
        let mut arm_env = before.clone();
        let binding_tys = match_match_arm_binding_types(
            &arm.variant,
            &arm.bindings,
            arm.is_named,
            variant_def,
            arm.line,
            arm.column,
        )?;
        for (binding_index, (binding, payload_ty)) in
            arm.bindings.iter().zip(binding_tys.iter()).enumerate()
        {
            if ctx.functions.contains_key(binding) {
                return Err(Diagnostic::new(
                    "type",
                    format!("match binding {binding:?} conflicts with a function name"),
                )
                .with_span(arm.line, arm.column));
            }
            if arm_env.contains_key(binding) {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "match binding {binding:?} reuses an existing name in the current scope"
                    ),
                )
                .with_span(arm.line, arm.column));
            }
            arm_env.insert(
                binding.clone(),
                Binding {
                    ty: payload_ty.clone(),
                    moved: false,
                    moved_projections: HashSet::new(),
                    borrow_kind: borrow_kind_for_type(payload_ty, ctx.structs, ctx.enums),
                    borrow_origin: match_binding_borrow_origin(
                        &lowered_expr,
                        &arm.variant,
                        binding,
                        binding_index,
                        payload_ty,
                        &before,
                        ctx,
                    ),
                    net_origin: None,
                    borrowed_owners: match_binding_borrowed_owners(
                        &lowered_expr,
                        &arm.variant,
                        binding,
                        binding_index,
                        payload_ty,
                        &before,
                        ctx,
                    ),
                    active_borrow_count: 0,
                    active_mut_borrow_count: 0,
                    active_borrows: HashMap::new(),
                },
            );
        }
        let lowered_arm_expr =
            lower_expr_with_expected(&arm.expr, result_ty.as_ref(), &mut arm_env, ctx)?;
        if let Some(expected_ty) = result_ty.as_ref() {
            if let Some(unified) = unify_types(expected_ty, lowered_arm_expr.ty()) {
                result_ty = Some(unified);
            } else if !type_assignable_to(lowered_arm_expr.ty(), expected_ty) {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "match expression arm {:?} expects {expected_ty}, got {}",
                        arm.variant,
                        lowered_arm_expr.ty()
                    ),
                )
                .with_span(arm.line, arm.column));
            }
        } else {
            result_ty = Some(lowered_arm_expr.ty().clone());
        }
        if !lowered_arm_expr.ty().is_copy() {
            move_lowered_owner_value(&lowered_arm_expr, &mut arm_env)?;
        }
        lowered_arms.push(MatchExprArm {
            enum_name: enum_name.clone(),
            variant: arm.variant,
            bindings: arm.bindings,
            is_named: arm.is_named,
            expr: lowered_arm_expr,
        });
        arm_states.push((arm_env, false));
    }
    let missing = variant_defs
        .iter()
        .filter(|variant| !seen.contains_key(&variant.name))
        .map(|variant| variant.name.clone())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(Diagnostic::new(
            "type",
            format!(
                "match on {:?} is not exhaustive; missing {}",
                enum_name,
                missing.join(", ")
            ),
        )
        .with_span(line, column));
    }
    merge_match_state(env, &before, &arm_states);
    if let Some(borrow_kind) = match_borrow_kind
        && !reuse_existing_match_binding
    {
        release_active_borrow_owners(&match_borrowed_owners, env, borrow_kind);
    }
    Ok(Expr::Match {
        expr: Box::new(lowered_expr),
        arms: lowered_arms,
        ty: result_ty.unwrap_or(Type::Error),
    })
}

fn match_match_arm_binding_types(
    variant: &str,
    bindings: &[String],
    is_named: bool,
    variant_def: &EnumVariantDef,
    line: usize,
    column: usize,
) -> Result<Vec<Type>, Diagnostic> {
    if is_named {
        if variant_def.payload_names.is_empty() {
            return Err(Diagnostic::new(
                "type",
                format!("match arm {variant:?} uses named bindings, but variant {variant:?} is positional"),
            )
            .with_span(line, column));
        }
        if bindings.len() != variant_def.payload_names.len() {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "match arm {variant:?} expects {} named bindings, got {}",
                    variant_def.payload_names.len(),
                    bindings.len()
                ),
            )
            .with_span(line, column));
        }
        let mut seen_named = HashMap::new();
        let mut payload_tys = Vec::new();
        for binding in bindings {
            let Some(position) = variant_def
                .payload_names
                .iter()
                .position(|name| name == binding)
            else {
                return Err(Diagnostic::new(
                    "type",
                    format!("match arm {variant:?} has no named payload {binding:?}"),
                )
                .with_span(line, column));
            };
            if seen_named.insert(binding.clone(), ()).is_some() {
                return Err(Diagnostic::new(
                    "type",
                    format!("match arm {variant:?} repeats named payload {binding:?}"),
                )
                .with_span(line, column));
            }
            payload_tys.push(variant_def.payload_tys[position].clone());
        }
        Ok(payload_tys)
    } else {
        if !variant_def.payload_names.is_empty() {
            return Err(Diagnostic::new(
                "type",
                format!("match arm {variant:?} must use named bindings for variant {variant:?}"),
            )
            .with_span(line, column));
        }
        if bindings.len() != variant_def.payload_tys.len() {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "match arm {variant:?} expects {} bindings, got {}",
                    variant_def.payload_tys.len(),
                    bindings.len()
                ),
            )
            .with_span(line, column));
        }
        Ok(variant_def.payload_tys.clone())
    }
}

fn resolve_const_match_int_pattern(
    arm: &MatchArmInput,
    ctx: &LowerContext<'_>,
) -> Result<i64, Diagnostic> {
    if let Ok(value) = arm.variant.parse::<i64>() {
        return Ok(value);
    }
    if !starts_with_ascii_uppercase(&arm.variant) {
        return Err(Diagnostic::new(
            "type",
            format!(
                "match pattern {:?} must name an uppercase int const",
                arm.variant
            ),
        )
        .with_span(arm.line, arm.column));
    }
    let Some(const_decl) = ctx.consts.get(&arm.variant) else {
        return Err(Diagnostic::new(
            "type",
            format!(
                "match pattern {:?} must name a known int const",
                arm.variant
            ),
        )
        .with_span(arm.line, arm.column));
    };
    eval_const_int_expr(&const_decl.expr).ok_or_else(|| {
        Diagnostic::new(
            "type",
            format!(
                "match pattern const {:?} must evaluate to int",
                const_decl.name
            ),
        )
        .with_span(const_decl.line, const_decl.column)
    })
}

fn starts_with_ascii_uppercase(value: &str) -> bool {
    value
        .chars()
        .next()
        .map(|ch| ch.is_ascii_uppercase())
        .unwrap_or(false)
}

pub(super) fn match_variants(
    ty: &Type,
    ctx: &LowerContext<'_>,
) -> Option<(String, Vec<EnumVariantDef>)> {
    match ty {
        Type::Enum(enum_name) => ctx
            .enums
            .get(enum_name)
            .map(|enum_def| (enum_name.clone(), enum_def.variants.clone())),
        Type::Option(inner) => Some((
            String::from("Option"),
            vec![
                EnumVariantDef {
                    name: String::from("Some"),
                    payload_tys: vec![inner.as_ref().clone()],
                    payload_names: Vec::new(),
                },
                EnumVariantDef {
                    name: String::from("None"),
                    payload_tys: Vec::new(),
                    payload_names: Vec::new(),
                },
            ],
        )),
        Type::Result(ok, err) => Some((
            String::from("Result"),
            vec![
                EnumVariantDef {
                    name: String::from("Ok"),
                    payload_tys: vec![ok.as_ref().clone()],
                    payload_names: Vec::new(),
                },
                EnumVariantDef {
                    name: String::from("Err"),
                    payload_tys: vec![err.as_ref().clone()],
                    payload_names: Vec::new(),
                },
            ],
        )),
        _ => None,
    }
}
