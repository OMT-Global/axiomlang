use std::collections::{HashMap, HashSet};

use crate::borrowck;
use crate::diagnostics::Diagnostic;

use super::model::{
    BorrowRegionFact, BorrowRegionOrigin, BorrowRegionProjection, BorrowRegionScope,
    BorrowRegionSource, EnumDef, Expr, StructDef, Type,
};
use super::{
    Binding, LowerContext, OWNERSHIP_MOVE_WHILE_BORROWED, OWNERSHIP_USE_AFTER_MOVE,
    is_scalar_local_assignment_type, ownership_error,
};

pub(super) type BorrowKind = borrowck::BorrowKind;
pub(super) type ProjectionPath = Vec<ProjectionSegment>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct BorrowedOwner {
    pub(super) name: String,
    pub(super) projection: ProjectionPath,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum ProjectionSegment {
    Field(String),
    TupleIndex(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum BorrowOrigin {
    Param(String),
    Local,
}

pub(super) fn move_lowered_value(
    expr: &Expr,
    env: &mut HashMap<String, Binding>,
) -> Result<(), Diagnostic> {
    let Expr::VarRef { name, .. } = expr else {
        return Ok(());
    };
    mark_projection_moved(name, Vec::new(), env)
}

pub(super) fn move_lowered_owner_value(
    expr: &Expr,
    env: &mut HashMap<String, Binding>,
) -> Result<(), Diagnostic> {
    let Some((name, projection)) = ownership_projection(expr) else {
        return Ok(());
    };
    mark_projection_moved(name, projection, env)
}

fn mark_projection_moved(
    name: &str,
    projection: ProjectionPath,
    env: &mut HashMap<String, Binding>,
) -> Result<(), Diagnostic> {
    let binding = env.get_mut(name).ok_or_else(|| {
        Diagnostic::new(
            "type",
            format!("internal error: missing binding for moved value {name:?}"),
        )
    })?;
    if moved_projection_conflicts_with_active_borrow(binding, &projection) {
        return Err(ownership_error(
            OWNERSHIP_MOVE_WHILE_BORROWED,
            format!(
                "cannot move value {:?} while borrowed slices are still live",
                format_projected_name(name, &projection)
            ),
        ));
    }
    if projection_is_unavailable(binding, &projection) {
        return Err(ownership_error(
            OWNERSHIP_USE_AFTER_MOVE,
            format!(
                "use of moved value {:?}",
                format_projected_name(name, &projection)
            ),
        ));
    }
    if projection.is_empty() {
        binding.moved = true;
    } else {
        binding.moved_projections.insert(projection);
    }
    Ok(())
}

fn moved_projection_conflicts_with_active_borrow(
    binding: &Binding,
    projection: &[ProjectionSegment],
) -> bool {
    if binding.active_borrow_count == 0 {
        return false;
    }
    if projection.is_empty() || binding.active_borrows.is_empty() {
        return true;
    }
    binding
        .active_borrows
        .keys()
        .any(|active_projection| projection_conflicts(active_projection, projection))
}

pub(super) fn ensure_lowered_projection_traversable(
    expr: &Expr,
    env: &HashMap<String, Binding>,
) -> Result<(), Diagnostic> {
    let Some((name, projection)) = ownership_projection(expr) else {
        return Ok(());
    };
    let Some(binding) = env.get(name) else {
        return Ok(());
    };
    if projection_has_moved_ancestor(binding, &projection) {
        return Err(ownership_error(
            OWNERSHIP_USE_AFTER_MOVE,
            format!(
                "use of moved value {:?}",
                format_projected_name(name, &projection)
            ),
        ));
    }
    Ok(())
}

pub(super) fn ownership_projection(expr: &Expr) -> Option<(&str, ProjectionPath)> {
    match expr {
        Expr::VarRef { name, .. } => Some((name, Vec::new())),
        Expr::Try { expr, .. } | Expr::Await { expr, .. } => ownership_projection(expr),
        Expr::FieldAccess { base, field, .. } => {
            let (name, mut path) = ownership_projection(base)?;
            path.push(ProjectionSegment::Field(field.clone()));
            Some((name, path))
        }
        Expr::TupleIndex { base, index, .. } => {
            let (name, mut path) = ownership_projection(base)?;
            path.push(ProjectionSegment::TupleIndex(*index));
            Some((name, path))
        }
        Expr::Index { base, .. } => ownership_projection(base),
        _ => None,
    }
}

fn projection_is_unavailable(binding: &Binding, projection: &[ProjectionSegment]) -> bool {
    binding.moved
        || binding
            .moved_projections
            .iter()
            .any(|moved| projection_conflicts(moved, projection))
}

fn projection_has_moved_ancestor(binding: &Binding, projection: &[ProjectionSegment]) -> bool {
    binding.moved
        || binding
            .moved_projections
            .iter()
            .any(|moved| is_projection_prefix(moved, projection))
}

fn projection_conflicts(left: &[ProjectionSegment], right: &[ProjectionSegment]) -> bool {
    is_projection_prefix(left, right) || is_projection_prefix(right, left)
}

fn is_projection_prefix(prefix: &[ProjectionSegment], projection: &[ProjectionSegment]) -> bool {
    prefix.len() <= projection.len()
        && prefix
            .iter()
            .zip(projection.iter())
            .all(|(left, right)| left == right)
}

fn format_projected_name(name: &str, projection: &[ProjectionSegment]) -> String {
    let mut out = name.to_string();
    for segment in projection {
        match segment {
            ProjectionSegment::Field(field) => {
                out.push('.');
                out.push_str(field);
            }
            ProjectionSegment::TupleIndex(index) => {
                out.push('.');
                out.push_str(&index.to_string());
            }
        }
    }
    out
}

pub(super) fn is_borrowable_slice_base(expr: &Expr) -> bool {
    match expr {
        Expr::VarRef { .. } => true,
        Expr::FieldAccess { base, .. } => is_borrowable_slice_base(base),
        Expr::TupleIndex { base, .. } => is_borrowable_slice_base(base),
        Expr::Slice { .. } => true,
        Expr::Call {
            ty: Type::Array(element, Some(_)),
            ..
        } => is_scalar_local_assignment_type(element),
        _ => false,
    }
}

pub(super) fn is_supported_helper_call_array_index_base(expr: &Expr) -> bool {
    match expr {
        Expr::Call {
            ty: Type::Array(element, Some(_)),
            ..
        } => is_scalar_local_assignment_type(element),
        _ => true,
    }
}

pub(super) fn binding_borrow_origin(
    ty: &Type,
    param_name: Option<&str>,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> Option<BorrowOrigin> {
    if !contains_borrowed_slice_type(ty, structs, enums) {
        return None;
    }
    Some(match param_name {
        Some(name) => BorrowOrigin::Param(name.to_string()),
        None => BorrowOrigin::Local,
    })
}

pub(super) fn binding_borrow_origin_from_expr(
    ty: &Type,
    expr: &Expr,
    env: &HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Option<BorrowOrigin> {
    if !contains_borrowed_slice_type(ty, ctx.structs, ctx.enums) {
        return None;
    }
    expr_borrow_origin(expr, env, ctx)
}

pub(super) fn binding_borrowed_owners_from_expr(
    ty: &Type,
    expr: &Expr,
    env: &HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> HashSet<BorrowedOwner> {
    if !contains_borrowed_slice_type(ty, ctx.structs, ctx.enums) {
        return HashSet::new();
    }
    expr_borrowed_owners(expr, env, ctx)
}

pub(super) fn borrow_region_facts_for_binding(
    binding: &str,
    ty: &Type,
    borrowed_owners: &HashSet<BorrowedOwner>,
) -> Vec<BorrowRegionFact> {
    if !matches!(ty, Type::Slice(_) | Type::MutSlice(_)) {
        return Vec::new();
    }
    let mut owners = borrowed_owners.iter().collect::<Vec<_>>();
    owners.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| format!("{:?}", left.projection).cmp(&format!("{:?}", right.projection)))
    });
    owners
        .into_iter()
        .map(|owner| BorrowRegionFact {
            binding: binding.to_string(),
            origin: BorrowRegionOrigin::from(owner),
            scope: BorrowRegionScope::Binding(binding.to_string()),
            source: BorrowRegionSource::Direct,
        })
        .collect()
}

pub(super) fn borrow_region_facts_for_enum_payload_binding(
    binding: &str,
    payload_ty: &Type,
    borrowed_owners: &HashSet<BorrowedOwner>,
    matched_expr: &Expr,
    variant: &str,
    payload_index: usize,
) -> Vec<BorrowRegionFact> {
    let Some(enum_origin) =
        owned_borrow_owner(matched_expr).map(|owner| BorrowRegionOrigin::from(&owner))
    else {
        return Vec::new();
    };
    let mut facts = borrow_region_facts_for_binding(binding, payload_ty, borrowed_owners);
    for fact in &mut facts {
        fact.source = BorrowRegionSource::EnumPayload {
            enum_origin: enum_origin.clone(),
            variant: variant.to_string(),
            payload_index,
        };
    }
    facts
}

pub(super) fn borrow_region_facts_for_return_expr(
    function: &str,
    expr: &Expr,
    env: &HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Vec<BorrowRegionFact> {
    let mut facts = Vec::new();
    collect_return_borrow_region_facts(function, expr, Vec::new(), env, ctx, &mut facts);
    facts
}

fn collect_return_borrow_region_facts(
    function: &str,
    expr: &Expr,
    projection: Vec<BorrowRegionProjection>,
    env: &HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
    facts: &mut Vec<BorrowRegionFact>,
) {
    if !contains_borrowed_slice_type(expr.ty(), ctx.structs, ctx.enums) {
        return;
    }
    match expr {
        Expr::TupleLiteral { elements, .. } => {
            for (index, element) in elements.iter().enumerate() {
                let mut child_projection = projection.clone();
                child_projection.push(BorrowRegionProjection::TupleIndex(index));
                collect_return_borrow_region_facts(
                    function,
                    element,
                    child_projection,
                    env,
                    ctx,
                    facts,
                );
            }
        }
        Expr::StructLiteral { fields, .. } => {
            for field in fields {
                let mut child_projection = projection.clone();
                child_projection.push(BorrowRegionProjection::Field(field.name.clone()));
                collect_return_borrow_region_facts(
                    function,
                    &field.expr,
                    child_projection,
                    env,
                    ctx,
                    facts,
                );
            }
        }
        Expr::EnumVariant {
            field_names,
            payloads,
            ..
        } => {
            for (index, payload) in payloads.iter().enumerate() {
                let mut child_projection = projection.clone();
                if let Some(field_name) = field_names.get(index).filter(|name| !name.is_empty()) {
                    child_projection.push(BorrowRegionProjection::Field(field_name.clone()));
                } else {
                    child_projection.push(BorrowRegionProjection::TupleIndex(index));
                }
                collect_return_borrow_region_facts(
                    function,
                    payload,
                    child_projection,
                    env,
                    ctx,
                    facts,
                );
            }
        }
        _ => {
            let mut owners = expr_borrowed_owners(expr, env, ctx);
            if owners.is_empty()
                && let Some(BorrowOrigin::Param(origin)) = expr_borrow_origin(expr, env, ctx)
            {
                owners.insert(BorrowedOwner {
                    name: origin,
                    projection: Vec::new(),
                });
            }
            let mut owners = owners.into_iter().collect::<Vec<_>>();
            owners.sort_by(|left, right| {
                left.name.cmp(&right.name).then_with(|| {
                    format!("{:?}", left.projection).cmp(&format!("{:?}", right.projection))
                })
            });
            facts.extend(owners.iter().map(|owner| BorrowRegionFact {
                binding: "return".to_string(),
                origin: BorrowRegionOrigin::from(owner),
                scope: BorrowRegionScope::Return {
                    function: function.to_string(),
                    projection: projection.clone(),
                },
                source: BorrowRegionSource::AggregateReturn,
            }));
        }
    }
}

impl From<&BorrowedOwner> for BorrowRegionOrigin {
    fn from(owner: &BorrowedOwner) -> Self {
        Self {
            name: owner.name.clone(),
            projection: owner
                .projection
                .iter()
                .map(BorrowRegionProjection::from)
                .collect(),
        }
    }
}

impl From<&ProjectionSegment> for BorrowRegionProjection {
    fn from(segment: &ProjectionSegment) -> Self {
        match segment {
            ProjectionSegment::Field(field) => Self::Field(field.clone()),
            ProjectionSegment::TupleIndex(index) => Self::TupleIndex(*index),
        }
    }
}

pub(super) fn expr_borrow_origin(
    expr: &Expr,
    env: &HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Option<BorrowOrigin> {
    if !contains_borrowed_slice_type(expr.ty(), ctx.structs, ctx.enums) {
        return None;
    }
    match expr {
        Expr::VarRef { name, .. } => env
            .get(name)
            .and_then(|binding| binding.borrow_origin.clone()),
        Expr::Slice { base, .. } => match base.ty() {
            Type::Slice(_) | Type::MutSlice(_) => expr_borrow_origin(base, env, ctx),
            Type::Array(_, _) => Some(BorrowOrigin::Local),
            _ => Some(BorrowOrigin::Local),
        },
        Expr::MutBorrow { .. } => Some(BorrowOrigin::Local),
        Expr::Deref { expr, .. } => expr_borrow_origin(expr, env, ctx),
        Expr::Call { name, args, .. } => ctx
            .functions
            .get(name)
            .map(|signature| {
                merge_borrow_origins(
                    signature
                        .borrow_return_params
                        .iter()
                        .map(|index| expr_borrow_origin(&args[*index], env, ctx)),
                )
            })
            .flatten(),
        Expr::TupleLiteral { elements, .. } => merge_borrow_origins(
            elements
                .iter()
                .map(|element| expr_borrow_origin(element, env, ctx)),
        ),
        Expr::Try { expr, .. } | Expr::Await { expr, .. } | Expr::Cast { expr, .. } => {
            expr_borrow_origin(expr, env, ctx)
        }
        Expr::TupleIndex { base, .. } => expr_borrow_origin(base, env, ctx),
        Expr::MapLiteral { entries, .. } => {
            merge_borrow_origins(entries.iter().flat_map(|entry| {
                [
                    expr_borrow_origin(&entry.key, env, ctx),
                    expr_borrow_origin(&entry.value, env, ctx),
                ]
            }))
        }
        Expr::EnumVariant { payloads, .. } => merge_borrow_origins(
            payloads
                .iter()
                .map(|payload| expr_borrow_origin(payload, env, ctx)),
        ),
        Expr::FieldAccess { base, .. } => expr_borrow_origin(base, env, ctx),
        Expr::ArrayLiteral { elements, .. } => merge_borrow_origins(
            elements
                .iter()
                .map(|element| expr_borrow_origin(element, env, ctx)),
        ),
        Expr::StructLiteral { fields, .. } => merge_borrow_origins(
            fields
                .iter()
                .map(|field| expr_borrow_origin(&field.expr, env, ctx)),
        ),
        Expr::Index { base, .. } => expr_borrow_origin(base, env, ctx),
        Expr::Closure { .. } => None,
        Expr::Match { arms, .. } => merge_borrow_origins(
            arms.iter()
                .map(|arm| expr_borrow_origin(&arm.expr, env, ctx)),
        ),
        Expr::Literal { .. }
        | Expr::BinaryAdd { .. }
        | Expr::BinaryCompare { .. }
        | Expr::BinaryLogic { .. } => None,
        Expr::StringBorrow { expr, .. } => expr_borrow_origin(expr, env, ctx)
            .or_else(|| owned_borrow_owner(expr).map(|_| BorrowOrigin::Local)),
    }
}

fn merge_borrow_origins<I>(origins: I) -> Option<BorrowOrigin>
where
    I: IntoIterator<Item = Option<BorrowOrigin>>,
{
    let mut merged = None;
    for origin in origins.into_iter().flatten() {
        match &merged {
            None => merged = Some(origin),
            Some(existing) if existing == &origin => {}
            Some(_) => return Some(BorrowOrigin::Local),
        }
    }
    merged
}

fn match_binding_payload_expr<'a>(
    matched_expr: &'a Expr,
    variant_name: &str,
    binding_name: &str,
    binding_index: usize,
) -> Option<&'a Expr> {
    let Expr::EnumVariant {
        variant,
        field_names,
        payloads,
        ..
    } = matched_expr
    else {
        return None;
    };
    if variant != variant_name {
        return None;
    }
    if field_names.is_empty() {
        return payloads.get(binding_index);
    }
    field_names
        .iter()
        .position(|field_name| field_name == binding_name)
        .and_then(|index| payloads.get(index))
}

pub(super) fn match_binding_borrow_origin(
    matched_expr: &Expr,
    variant_name: &str,
    binding_name: &str,
    binding_index: usize,
    payload_ty: &Type,
    env: &HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Option<BorrowOrigin> {
    if !contains_borrowed_slice_type(payload_ty, ctx.structs, ctx.enums) {
        return None;
    }
    if let Some(payload_expr) =
        match_binding_payload_expr(matched_expr, variant_name, binding_name, binding_index)
    {
        return expr_borrow_origin(payload_expr, env, ctx);
    }
    expr_borrow_origin(matched_expr, env, ctx)
}

pub(super) fn match_binding_borrowed_owners(
    matched_expr: &Expr,
    variant_name: &str,
    binding_name: &str,
    binding_index: usize,
    payload_ty: &Type,
    env: &HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> HashSet<BorrowedOwner> {
    if !contains_borrowed_slice_type(payload_ty, ctx.structs, ctx.enums) {
        return HashSet::new();
    }
    if let Some(payload_expr) =
        match_binding_payload_expr(matched_expr, variant_name, binding_name, binding_index)
    {
        return expr_borrowed_owners(payload_expr, env, ctx);
    }
    expr_borrowed_owners(matched_expr, env, ctx)
}

pub(super) fn expr_borrowed_owners(
    expr: &Expr,
    env: &HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> HashSet<BorrowedOwner> {
    if !contains_borrowed_slice_type(expr.ty(), ctx.structs, ctx.enums) {
        return HashSet::new();
    }
    match expr {
        Expr::VarRef { name, .. } => env
            .get(name)
            .map(|binding| binding.borrowed_owners.clone())
            .unwrap_or_default(),
        Expr::Slice { base, .. } => match base.ty() {
            Type::Slice(_) | Type::MutSlice(_) => expr_borrowed_owners(base, env, ctx),
            Type::Array(_, _) => owned_borrow_owner(base).into_iter().collect(),
            _ => HashSet::new(),
        },
        Expr::MutBorrow { expr, .. } => owned_borrow_owner(expr).into_iter().collect(),
        Expr::Deref { expr, .. } => expr_borrowed_owners(expr, env, ctx),
        Expr::Call { name, args, .. } => ctx
            .functions
            .get(name)
            .map(|signature| {
                let mut owners = HashSet::new();
                for index in &signature.borrow_return_params {
                    owners.extend(expr_borrowed_owners(&args[*index], env, ctx));
                }
                owners
            })
            .unwrap_or_default(),
        Expr::TupleLiteral { elements, .. } => collect_expr_borrowed_owners(elements, env, ctx),
        Expr::Try { expr, .. } | Expr::Await { expr, .. } | Expr::Cast { expr, .. } => {
            expr_borrowed_owners(expr, env, ctx)
        }
        Expr::TupleIndex { base, .. } => expr_borrowed_owners(base, env, ctx),
        Expr::MapLiteral { entries, .. } => {
            let mut owners = HashSet::new();
            for entry in entries {
                owners.extend(expr_borrowed_owners(&entry.key, env, ctx));
                owners.extend(expr_borrowed_owners(&entry.value, env, ctx));
            }
            owners
        }
        Expr::ArrayLiteral { elements, .. } => collect_expr_borrowed_owners(elements, env, ctx),
        Expr::EnumVariant { payloads, .. } => collect_expr_borrowed_owners(payloads, env, ctx),
        Expr::FieldAccess { base, .. } => expr_borrowed_owners(base, env, ctx),
        Expr::Index { base, .. } => expr_borrowed_owners(base, env, ctx),
        Expr::Closure { .. } => HashSet::new(),
        Expr::Match { arms, .. } => {
            let mut owners = HashSet::new();
            for arm in arms {
                owners.extend(expr_borrowed_owners(&arm.expr, env, ctx));
            }
            owners
        }
        Expr::Literal { .. }
        | Expr::BinaryAdd { .. }
        | Expr::BinaryCompare { .. }
        | Expr::BinaryLogic { .. } => HashSet::new(),
        Expr::StringBorrow { expr, .. } => owned_borrow_owner(expr).into_iter().collect(),
        Expr::StructLiteral { fields, .. } => {
            let mut owners = HashSet::new();
            for field in fields {
                owners.extend(expr_borrowed_owners(&field.expr, env, ctx));
            }
            owners
        }
    }
}

fn collect_expr_borrowed_owners(
    exprs: &[Expr],
    env: &HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> HashSet<BorrowedOwner> {
    let mut owners = HashSet::new();
    for expr in exprs {
        owners.extend(expr_borrowed_owners(expr, env, ctx));
    }
    owners
}

fn owned_borrow_owner(expr: &Expr) -> Option<BorrowedOwner> {
    let (name, projection) = ownership_projection(expr)?;
    if matches!(expr.ty(), Type::Slice(_) | Type::MutSlice(_)) {
        return None;
    }
    Some(BorrowedOwner {
        name: name.to_string(),
        projection,
    })
}

pub(super) fn contains_borrowed_slice_type(
    ty: &Type,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> bool {
    contains_borrowed_slice_type_inner(ty, structs, enums, &mut HashSet::new(), &mut HashSet::new())
}

fn contains_borrowed_slice_type_inner(
    ty: &Type,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    visiting_structs: &mut HashSet<String>,
    visiting_enums: &mut HashSet<String>,
) -> bool {
    match ty {
        Type::Slice(_) | Type::MutSlice(_) | Type::MutRef(_) | Type::Str => true,
        Type::Option(inner) => contains_borrowed_slice_type_inner(
            inner,
            structs,
            enums,
            visiting_structs,
            visiting_enums,
        ),
        Type::Result(ok, err) => {
            contains_borrowed_slice_type_inner(ok, structs, enums, visiting_structs, visiting_enums)
                || contains_borrowed_slice_type_inner(
                    err,
                    structs,
                    enums,
                    visiting_structs,
                    visiting_enums,
                )
        }
        Type::Tuple(elements) => elements.iter().any(|element| {
            contains_borrowed_slice_type_inner(
                element,
                structs,
                enums,
                visiting_structs,
                visiting_enums,
            )
        }),
        Type::Map(key, value) => {
            contains_borrowed_slice_type_inner(
                key,
                structs,
                enums,
                visiting_structs,
                visiting_enums,
            ) || contains_borrowed_slice_type_inner(
                value,
                structs,
                enums,
                visiting_structs,
                visiting_enums,
            )
        }
        Type::Array(inner, _)
        | Type::Task(inner)
        | Type::JoinHandle(inner)
        | Type::AsyncChannel(inner)
        | Type::SelectResult(inner) => contains_borrowed_slice_type_inner(
            inner,
            structs,
            enums,
            visiting_structs,
            visiting_enums,
        ),
        Type::Fn(params, return_ty) => {
            params.iter().any(|param| {
                contains_borrowed_slice_type_inner(
                    param,
                    structs,
                    enums,
                    visiting_structs,
                    visiting_enums,
                )
            }) || contains_borrowed_slice_type_inner(
                return_ty,
                structs,
                enums,
                visiting_structs,
                visiting_enums,
            )
        }
        Type::Struct(name) => {
            if !visiting_structs.insert(name.clone()) {
                return false;
            }
            let contains = structs.get(name).is_some_and(|struct_def| {
                struct_def.fields.iter().any(|field| {
                    contains_borrowed_slice_type_inner(
                        &field.ty,
                        structs,
                        enums,
                        visiting_structs,
                        visiting_enums,
                    )
                })
            });
            visiting_structs.remove(name);
            contains
        }
        Type::Enum(name) => {
            if !visiting_enums.insert(name.clone()) {
                return false;
            }
            let contains = enums.get(name).is_some_and(|enum_def| {
                enum_def.variants.iter().any(|variant| {
                    variant.payload_tys.iter().any(|payload_ty| {
                        contains_borrowed_slice_type_inner(
                            payload_ty,
                            structs,
                            enums,
                            visiting_structs,
                            visiting_enums,
                        )
                    })
                })
            });
            visiting_enums.remove(name);
            contains
        }
        Type::Error
        | Type::Never
        | Type::Int
        | Type::Numeric(_)
        | Type::Bool
        | Type::String
        | Type::Ptr(_)
        | Type::MutPtr(_) => false,
    }
}

fn contains_mut_borrowed_slice_type_inner(
    ty: &Type,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    visiting_structs: &mut HashSet<String>,
    visiting_enums: &mut HashSet<String>,
) -> bool {
    match ty {
        Type::MutSlice(_) | Type::MutRef(_) => true,
        Type::Error
        | Type::Never
        | Type::Slice(_)
        | Type::Int
        | Type::Numeric(_)
        | Type::Bool
        | Type::String
        | Type::Str
        | Type::Ptr(_)
        | Type::MutPtr(_) => false,
        Type::Option(inner) => contains_mut_borrowed_slice_type_inner(
            inner,
            structs,
            enums,
            visiting_structs,
            visiting_enums,
        ),
        Type::Result(ok, err) => {
            contains_mut_borrowed_slice_type_inner(
                ok,
                structs,
                enums,
                visiting_structs,
                visiting_enums,
            ) || contains_mut_borrowed_slice_type_inner(
                err,
                structs,
                enums,
                visiting_structs,
                visiting_enums,
            )
        }
        Type::Tuple(elements) => elements.iter().any(|element| {
            contains_mut_borrowed_slice_type_inner(
                element,
                structs,
                enums,
                visiting_structs,
                visiting_enums,
            )
        }),
        Type::Map(key, value) => {
            contains_mut_borrowed_slice_type_inner(
                key,
                structs,
                enums,
                visiting_structs,
                visiting_enums,
            ) || contains_mut_borrowed_slice_type_inner(
                value,
                structs,
                enums,
                visiting_structs,
                visiting_enums,
            )
        }
        Type::Array(inner, _)
        | Type::Task(inner)
        | Type::JoinHandle(inner)
        | Type::AsyncChannel(inner)
        | Type::SelectResult(inner) => contains_mut_borrowed_slice_type_inner(
            inner,
            structs,
            enums,
            visiting_structs,
            visiting_enums,
        ),
        Type::Fn(params, return_ty) => {
            params.iter().any(|param| {
                contains_mut_borrowed_slice_type_inner(
                    param,
                    structs,
                    enums,
                    visiting_structs,
                    visiting_enums,
                )
            }) || contains_mut_borrowed_slice_type_inner(
                return_ty,
                structs,
                enums,
                visiting_structs,
                visiting_enums,
            )
        }
        Type::Struct(name) => {
            if !visiting_structs.insert(name.clone()) {
                return false;
            }
            let contains = structs.get(name).is_some_and(|struct_def| {
                struct_def.fields.iter().any(|field| {
                    contains_mut_borrowed_slice_type_inner(
                        &field.ty,
                        structs,
                        enums,
                        visiting_structs,
                        visiting_enums,
                    )
                })
            });
            visiting_structs.remove(name);
            contains
        }
        Type::Enum(name) => {
            if !visiting_enums.insert(name.clone()) {
                return false;
            }
            let contains = enums.get(name).is_some_and(|enum_def| {
                enum_def.variants.iter().any(|variant| {
                    variant.payload_tys.iter().any(|payload_ty| {
                        contains_mut_borrowed_slice_type_inner(
                            payload_ty,
                            structs,
                            enums,
                            visiting_structs,
                            visiting_enums,
                        )
                    })
                })
            });
            visiting_enums.remove(name);
            contains
        }
    }
}

pub(super) fn borrow_kind_for_type(
    ty: &Type,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> Option<BorrowKind> {
    borrowck::borrow_kind_for_type(ty, structs, enums)
}

pub(super) fn increment_active_borrows(
    owner_names: &HashSet<BorrowedOwner>,
    env: &mut HashMap<String, Binding>,
    borrow_kind: BorrowKind,
    line: usize,
    column: usize,
) -> Result<(), Diagnostic> {
    for owner in owner_names {
        let binding = env.get_mut(&owner.name).ok_or_else(|| {
            Diagnostic::new(
                "type",
                format!("internal error: missing borrow owner {:?}", owner.name),
            )
        })?;
        let mut conflicting = borrowck::BorrowState::default();
        for (active_projection, state) in &binding.active_borrows {
            if projection_conflicts(active_projection, &owner.projection) {
                conflicting.active_shared_or_mutable += state.active_shared_or_mutable;
                conflicting.active_mutable += state.active_mutable;
            }
        }
        if binding.active_borrow_count > 0 && binding.active_borrows.is_empty() {
            conflicting.active_shared_or_mutable = binding.active_borrow_count;
            conflicting.active_mutable = binding.active_mut_borrow_count;
        }
        let projected_name = format_projected_name(&owner.name, &owner.projection);
        conflicting.begin_borrow(
            &projected_name,
            borrow_kind,
            borrowck::SourceSpan::new(line, column),
        )?;
        let mut state = borrowck::BorrowState {
            active_shared_or_mutable: binding
                .active_borrows
                .get(&owner.projection)
                .map(|state| state.active_shared_or_mutable)
                .unwrap_or_default(),
            active_mutable: binding
                .active_borrows
                .get(&owner.projection)
                .map(|state| state.active_mutable)
                .unwrap_or_default(),
        };
        state.active_shared_or_mutable += 1;
        if matches!(borrow_kind, BorrowKind::Mutable) {
            state.active_mutable += 1;
        }
        binding
            .active_borrows
            .insert(owner.projection.clone(), state);
        binding.active_borrow_count += 1;
        if matches!(borrow_kind, BorrowKind::Mutable) {
            binding.active_mut_borrow_count += 1;
        }
    }
    Ok(())
}

pub(super) fn record_temporary_borrows(
    expr: &Expr,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
    temporary_borrows: &mut Vec<(HashSet<BorrowedOwner>, BorrowKind)>,
) -> Result<(), Diagnostic> {
    let owners = expr_borrowed_owners(expr, env, ctx);
    let Some(borrow_kind) = borrow_kind_for_type(expr.ty(), ctx.structs, ctx.enums) else {
        return Ok(());
    };
    increment_active_borrows(&owners, env, borrow_kind, 0, 0)?;
    temporary_borrows.push((owners, borrow_kind));
    Ok(())
}

pub(super) fn release_temporary_borrows(
    temporary_borrows: &[(HashSet<BorrowedOwner>, BorrowKind)],
    env: &mut HashMap<String, Binding>,
) {
    for (owner_names, borrow_kind) in temporary_borrows.iter().rev() {
        release_active_borrow_owners(owner_names, env, *borrow_kind);
    }
}

pub(super) fn release_active_borrow_owners(
    owner_names: &HashSet<BorrowedOwner>,
    env: &mut HashMap<String, Binding>,
    borrow_kind: BorrowKind,
) {
    for owner in owner_names {
        decrement_active_borrow(owner, env, borrow_kind);
    }
}

fn decrement_active_borrow(
    owner: &BorrowedOwner,
    env: &mut HashMap<String, Binding>,
    borrow_kind: BorrowKind,
) {
    let Some(binding) = env.get_mut(&owner.name) else {
        return;
    };
    let mut remove_projection = false;
    if let Some(state) = binding.active_borrows.get_mut(&owner.projection) {
        state.active_shared_or_mutable = state.active_shared_or_mutable.saturating_sub(1);
        if matches!(borrow_kind, BorrowKind::Mutable) {
            state.active_mutable = state.active_mutable.saturating_sub(1);
        }
        remove_projection = state.active_shared_or_mutable == 0 && state.active_mutable == 0;
    }
    if remove_projection {
        binding.active_borrows.remove(&owner.projection);
    }
    binding.active_borrow_count = binding.active_borrow_count.saturating_sub(1);
    if matches!(borrow_kind, BorrowKind::Mutable) {
        binding.active_mut_borrow_count = binding.active_mut_borrow_count.saturating_sub(1);
    }
}

pub(super) fn release_scope_borrows(
    env: &mut HashMap<String, Binding>,
    scope_names: &HashSet<String>,
) {
    let released = env
        .keys()
        .filter(|name| !scope_names.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    for name in &released {
        let (owner_names, borrow_kind) = env
            .get(name)
            .map(|binding| {
                (
                    binding.borrowed_owners.iter().cloned().collect::<Vec<_>>(),
                    binding.borrow_kind,
                )
            })
            .unwrap_or_default();
        let Some(borrow_kind) = borrow_kind else {
            continue;
        };
        for owner in owner_names {
            decrement_active_borrow(&owner, env, borrow_kind);
        }
    }
    for name in released {
        env.remove(&name);
    }
}

pub(super) fn merge_borrow_count(
    before: usize,
    then_returns: bool,
    then_after: Option<usize>,
    else_returns: bool,
    else_after: Option<usize>,
) -> usize {
    let then_count = if then_returns {
        before
    } else {
        then_after.unwrap_or(before)
    };
    let else_count = if else_returns {
        before
    } else {
        else_after.unwrap_or(before)
    };
    then_count.max(else_count)
}
