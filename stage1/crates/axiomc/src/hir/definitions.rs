use super::model::{
    EnumDef, EnumVariantDef, StructDef, StructField, TraitDef, TraitMethodDef, Type,
};
use super::types::lower_type;
use crate::diagnostics::Diagnostic;
use crate::syntax;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub(super) struct VariantInfo {
    pub(super) enum_name: String,
    pub(super) payload_tys: Vec<Type>,
    pub(super) payload_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum AggregateRef {
    Struct(String),
    Enum(String),
}

pub(super) fn validate_recursive_type_cycles(
    program: &syntax::Program,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    syntax_structs: &HashMap<String, syntax::StructDecl>,
    syntax_enums: &HashMap<String, ()>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
    consts: &HashMap<String, syntax::ConstDecl>,
) -> Result<(), Diagnostic> {
    for struct_decl in &program.structs {
        let owner = AggregateRef::Struct(struct_decl.name.clone());
        for field in &struct_decl.fields {
            let ty = lower_type(
                &field.ty,
                syntax_structs,
                syntax_enums,
                aliases,
                consts,
                field.line,
                field.column,
            )?;
            if type_has_unboxed_recursive_path(&ty, &owner, structs, enums, &mut HashSet::new()) {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "recursive field {:?} in struct {:?} requires indirection; unboxed recursive types are not supported",
                        field.name, struct_decl.name
                    ),
                )
                .with_span(field.line, field.column));
            }
        }
    }

    for enum_decl in &program.enums {
        let owner = AggregateRef::Enum(enum_decl.name.clone());
        for variant in &enum_decl.variants {
            for payload_ty in &variant.payload_tys {
                let ty = lower_type(
                    payload_ty,
                    syntax_structs,
                    syntax_enums,
                    aliases,
                    consts,
                    variant.line,
                    variant.column,
                )?;
                if type_has_unboxed_recursive_path(&ty, &owner, structs, enums, &mut HashSet::new())
                {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "recursive payload variant {:?} in enum {:?} requires indirection; unboxed recursive types are not supported",
                            variant.name, enum_decl.name
                        ),
                    )
                    .with_span(variant.line, variant.column));
                }
            }
        }
    }

    Ok(())
}

fn type_has_unboxed_recursive_path(
    ty: &Type,
    owner: &AggregateRef,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    visiting: &mut HashSet<AggregateRef>,
) -> bool {
    match ty {
        Type::Error
        | Type::Never
        | Type::Int
        | Type::Numeric(_)
        | Type::Bool
        | Type::String
        | Type::Str
        | Type::Ptr(_)
        | Type::MutPtr(_)
        | Type::MutRef(_) => false,
        Type::Struct(name) => {
            let current = AggregateRef::Struct(name.clone());
            if &current == owner {
                return true;
            }
            if !visiting.insert(current.clone()) {
                return false;
            }
            let result = structs
                .get(name)
                .map(|struct_def| {
                    struct_def.fields.iter().any(|field| {
                        type_has_unboxed_recursive_path(&field.ty, owner, structs, enums, visiting)
                    })
                })
                .unwrap_or(false);
            visiting.remove(&current);
            result
        }
        Type::Enum(name) => {
            let current = AggregateRef::Enum(name.clone());
            if &current == owner {
                return true;
            }
            if !visiting.insert(current.clone()) {
                return false;
            }
            let result = enums
                .get(name)
                .map(|enum_def| {
                    enum_def.variants.iter().any(|variant| {
                        variant.payload_tys.iter().any(|payload_ty| {
                            type_has_unboxed_recursive_path(
                                payload_ty, owner, structs, enums, visiting,
                            )
                        })
                    })
                })
                .unwrap_or(false);
            visiting.remove(&current);
            result
        }
        Type::Slice(_)
        | Type::MutSlice(_)
        | Type::Map(_, _)
        | Type::Array(_, _)
        | Type::Fn(_, _) => false,
        Type::Option(inner)
        | Type::Task(inner)
        | Type::JoinHandle(inner)
        | Type::AsyncChannel(inner)
        | Type::SelectResult(inner) => {
            type_has_unboxed_recursive_path(inner, owner, structs, enums, visiting)
        }
        Type::Result(ok, err) => {
            type_has_unboxed_recursive_path(ok, owner, structs, enums, visiting)
                || type_has_unboxed_recursive_path(err, owner, structs, enums, visiting)
        }
        Type::Tuple(elements) => elements.iter().any(|element| {
            type_has_unboxed_recursive_path(element, owner, structs, enums, visiting)
        }),
    }
}

pub(super) fn collect_struct_definitions(
    structs: &[syntax::StructDecl],
    enums: &HashMap<String, ()>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
    consts: &HashMap<String, syntax::ConstDecl>,
) -> Result<HashMap<String, StructDef>, Diagnostic> {
    let mut names = HashMap::new();
    for struct_decl in structs {
        if enums.contains_key(&struct_decl.name) {
            return Err(Diagnostic::new(
                "type",
                format!("duplicate type name {:?}", struct_decl.name),
            )
            .with_span(struct_decl.line, struct_decl.column));
        }
        if names
            .insert(struct_decl.name.clone(), struct_decl.clone())
            .is_some()
        {
            return Err(Diagnostic::new(
                "type",
                format!("duplicate struct {:?}", struct_decl.name),
            )
            .with_span(struct_decl.line, struct_decl.column));
        }
    }

    let mut lowered = HashMap::new();
    for struct_decl in structs {
        let mut fields = Vec::new();
        let mut seen = HashMap::new();
        for field in &struct_decl.fields {
            if seen.insert(field.name.clone(), ()).is_some() {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "duplicate field {:?} in struct {:?}",
                        field.name, struct_decl.name
                    ),
                )
                .with_span(field.line, field.column));
            }
            let ty = lower_type(
                &field.ty,
                &names,
                enums,
                aliases,
                consts,
                field.line,
                field.column,
            )?;
            fields.push(StructField {
                name: field.name.clone(),
                ty,
            });
        }
        lowered.insert(
            struct_decl.name.clone(),
            StructDef {
                name: struct_decl.name.clone(),
                fields,
            },
        );
    }
    Ok(lowered)
}

pub(super) fn collect_trait_definitions(
    traits: &[syntax::TraitDecl],
    structs: &HashMap<String, syntax::StructDecl>,
    enums: &HashMap<String, ()>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
) -> Result<HashMap<String, TraitDef>, Diagnostic> {
    let mut names = HashMap::new();
    for trait_decl in traits {
        if structs.contains_key(&trait_decl.name)
            || enums.contains_key(&trait_decl.name)
            || aliases.contains_key(&trait_decl.name)
        {
            return Err(Diagnostic::new(
                "type",
                format!("duplicate type name {:?}", trait_decl.name),
            )
            .with_span(trait_decl.line, trait_decl.column));
        }
        if names.insert(trait_decl.name.clone(), ()).is_some() {
            return Err(
                Diagnostic::new("type", format!("duplicate trait {:?}", trait_decl.name))
                    .with_span(trait_decl.line, trait_decl.column),
            );
        }
    }

    // First pass: validate all trait type references now that we have the complete trait names map
    for trait_decl in traits {
        for method in &trait_decl.methods {
            for param in &method.params {
                validate_trait_type_use_in_namespace(&param.ty, &names, param.line, param.column)?;
            }
            validate_trait_type_use_in_namespace(
                &method.return_ty,
                &names,
                method.line,
                method.column,
            )?;
        }
    }

    let mut lowered = HashMap::new();
    for trait_decl in traits {
        let mut seen_methods = HashMap::new();
        let methods = trait_decl
            .methods
            .iter()
            .map(|method| {
                if seen_methods.insert(method.name.clone(), ()).is_some() {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "duplicate method {:?} in trait {:?}",
                            method.name, trait_decl.name
                        ),
                    )
                    .with_span(method.line, method.column));
                }
                Ok(TraitMethodDef {
                    name: method.name.clone(),
                    params: method.params.iter().map(|param| param.ty.clone()).collect(),
                    return_ty: method.return_ty.clone(),
                    has_self: method.has_self,
                })
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        lowered.insert(
            trait_decl.name.clone(),
            TraitDef {
                name: trait_decl.name.clone(),
                methods,
            },
        );
    }
    Ok(lowered)
}

pub(super) fn collect_type_names(
    structs: &[syntax::StructDecl],
    enums: &[syntax::EnumDecl],
    aliases: &[syntax::TypeAliasDecl],
) -> Result<
    (
        HashMap<String, syntax::StructDecl>,
        HashMap<String, ()>,
        HashMap<String, syntax::TypeAliasDecl>,
    ),
    Diagnostic,
> {
    let mut struct_names = HashMap::new();
    for struct_decl in structs {
        if struct_names
            .insert(struct_decl.name.clone(), struct_decl.clone())
            .is_some()
        {
            return Err(Diagnostic::new(
                "type",
                format!("duplicate struct {:?}", struct_decl.name),
            )
            .with_span(struct_decl.line, struct_decl.column));
        }
    }
    let mut enum_names = HashMap::new();
    for enum_decl in enums {
        if struct_names.contains_key(&enum_decl.name) {
            return Err(Diagnostic::new(
                "type",
                format!("duplicate type name {:?}", enum_decl.name),
            )
            .with_span(enum_decl.line, enum_decl.column));
        }
        if enum_names.insert(enum_decl.name.clone(), ()).is_some() {
            return Err(
                Diagnostic::new("type", format!("duplicate enum {:?}", enum_decl.name))
                    .with_span(enum_decl.line, enum_decl.column),
            );
        }
    }
    let mut alias_names = HashMap::new();
    for type_alias in aliases {
        if struct_names.contains_key(&type_alias.name) || enum_names.contains_key(&type_alias.name)
        {
            return Err(Diagnostic::new(
                "type",
                format!("duplicate type name {:?}", type_alias.name),
            )
            .with_span(type_alias.line, type_alias.column));
        }
        if alias_names
            .insert(type_alias.name.clone(), type_alias.clone())
            .is_some()
        {
            return Err(Diagnostic::new(
                "type",
                format!("duplicate type alias {:?}", type_alias.name),
            )
            .with_span(type_alias.line, type_alias.column));
        }
    }
    Ok((struct_names, enum_names, alias_names))
}

pub(super) fn validate_trait_type_uses_in_program(
    program: &syntax::Program,
    traits: &HashMap<String, TraitDef>,
) -> Result<(), Diagnostic> {
    for type_alias in &program.type_aliases {
        validate_trait_type_use(&type_alias.ty, traits, type_alias.line, type_alias.column)?;
    }
    for struct_decl in &program.structs {
        for field in &struct_decl.fields {
            validate_trait_type_use(&field.ty, traits, field.line, field.column)?;
        }
    }
    for enum_decl in &program.enums {
        for variant in &enum_decl.variants {
            for ty in &variant.payload_tys {
                validate_trait_type_use(ty, traits, variant.line, variant.column)?;
            }
        }
    }
    for constant in &program.consts {
        validate_trait_type_use(&constant.ty, traits, constant.line, constant.column)?;
    }
    for trait_decl in &program.traits {
        for method in &trait_decl.methods {
            for param in &method.params {
                validate_trait_type_use(&param.ty, traits, param.line, param.column)?;
            }
            validate_trait_type_use(&method.return_ty, traits, method.line, method.column)?;
        }
    }
    for function in &program.functions {
        for param in &function.params {
            validate_trait_type_use(&param.ty, traits, param.line, param.column)?;
        }
        validate_trait_type_use(&function.return_ty, traits, function.line, function.column)?;
        validate_trait_type_uses_in_stmts(&function.body, traits)?;
    }
    validate_trait_type_uses_in_stmts(&program.stmts, traits)
}

fn validate_trait_type_uses_in_stmts(
    stmts: &[syntax::Stmt],
    traits: &HashMap<String, TraitDef>,
) -> Result<(), Diagnostic> {
    for stmt in stmts {
        match stmt {
            syntax::Stmt::Let {
                ty, line, column, ..
            } => validate_trait_type_use(ty, traits, *line, *column)?,
            syntax::Stmt::If {
                then_block,
                else_block,
                ..
            } => {
                validate_trait_type_uses_in_stmts(then_block, traits)?;
                if let Some(else_block) = else_block {
                    validate_trait_type_uses_in_stmts(else_block, traits)?;
                }
            }
            syntax::Stmt::IfLet {
                then_block,
                else_block,
                ..
            } => {
                validate_trait_type_uses_in_stmts(then_block, traits)?;
                if let Some(else_block) = else_block {
                    validate_trait_type_uses_in_stmts(else_block, traits)?;
                }
            }
            syntax::Stmt::While { body, .. } => {
                validate_trait_type_uses_in_stmts(body, traits)?;
            }
            syntax::Stmt::Match { arms, .. } => {
                for arm in arms {
                    validate_trait_type_uses_in_stmts(&arm.body, traits)?;
                }
            }
            syntax::Stmt::Print { .. }
            | syntax::Stmt::Assign { .. }
            | syntax::Stmt::Panic { .. }
            | syntax::Stmt::Defer { .. }
            | syntax::Stmt::Return { .. } => {}
        }
    }
    Ok(())
}

fn validate_trait_type_use(
    ty: &syntax::TypeName,
    traits: &HashMap<String, TraitDef>,
    line: usize,
    column: usize,
) -> Result<(), Diagnostic> {
    let trait_names = traits
        .keys()
        .map(|name| (name.clone(), ()))
        .collect::<HashMap<_, _>>();
    validate_trait_type_use_in_namespace(ty, &trait_names, line, column)
}

fn validate_trait_type_use_in_namespace(
    ty: &syntax::TypeName,
    trait_names: &HashMap<String, ()>,
    line: usize,
    column: usize,
) -> Result<(), Diagnostic> {
    match ty {
        syntax::TypeName::Named(name, args) => {
            if trait_names.contains_key(name) {
                return Err(Diagnostic::new(
                    "type",
                    format!("trait dispatch is not yet implemented for trait {name:?}"),
                )
                .with_span(line, column));
            }
            for arg in args {
                validate_trait_type_use_in_namespace(arg, trait_names, line, column)?;
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
        | syntax::TypeName::Array(inner, _) => {
            validate_trait_type_use_in_namespace(inner, trait_names, line, column)?;
        }
        syntax::TypeName::Result(ok, err) | syntax::TypeName::Map(ok, err) => {
            validate_trait_type_use_in_namespace(ok, trait_names, line, column)?;
            validate_trait_type_use_in_namespace(err, trait_names, line, column)?;
        }
        syntax::TypeName::Tuple(elements) => {
            for element in elements {
                validate_trait_type_use_in_namespace(element, trait_names, line, column)?;
            }
        }
        syntax::TypeName::Fn(params, return_ty) => {
            for param in params {
                validate_trait_type_use_in_namespace(param, trait_names, line, column)?;
            }
            validate_trait_type_use_in_namespace(return_ty, trait_names, line, column)?;
        }
        syntax::TypeName::Int
        | syntax::TypeName::Numeric(_)
        | syntax::TypeName::Bool
        | syntax::TypeName::String
        | syntax::TypeName::Str => {}
    }
    Ok(())
}

pub(super) fn collect_enum_definitions(
    enums: &[syntax::EnumDecl],
    structs: &HashMap<String, syntax::StructDecl>,
    enum_names: &HashMap<String, ()>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
    consts: &HashMap<String, syntax::ConstDecl>,
) -> Result<(HashMap<String, EnumDef>, HashMap<String, Vec<VariantInfo>>), Diagnostic> {
    let mut lowered = HashMap::new();
    let mut variants = HashMap::new();
    for enum_decl in enums {
        if enum_decl.variants.is_empty() {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "enum {:?} must declare at least one variant",
                    enum_decl.name
                ),
            )
            .with_span(enum_decl.line, enum_decl.column));
        }
        let mut seen = HashMap::new();
        let mut lowered_variants = Vec::new();
        for variant in &enum_decl.variants {
            if seen.insert(variant.name.clone(), ()).is_some() {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "duplicate variant {:?} in enum {:?}",
                        variant.name, enum_decl.name
                    ),
                )
                .with_span(variant.line, variant.column));
            }
            variants
                .entry(variant.name.clone())
                .or_insert_with(Vec::new)
                .push(VariantInfo {
                    enum_name: enum_decl.name.clone(),
                    payload_tys: variant
                        .payload_tys
                        .iter()
                        .map(|ty| {
                            lower_type(
                                ty,
                                structs,
                                enum_names,
                                aliases,
                                consts,
                                variant.line,
                                variant.column,
                            )
                        })
                        .collect::<Result<Vec<_>, Diagnostic>>()?,
                    payload_names: variant.payload_names.clone(),
                });
            let payload_tys = variant
                .payload_tys
                .iter()
                .map(|ty| {
                    lower_type(
                        ty,
                        structs,
                        enum_names,
                        aliases,
                        consts,
                        variant.line,
                        variant.column,
                    )
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            if !variant.payload_names.is_empty() && variant.payload_names.len() != payload_tys.len()
            {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "internal error: enum variant {:?} has mismatched named payload metadata",
                        variant.name
                    ),
                )
                .with_span(variant.line, variant.column));
            }
            let mut seen_payload_names = HashMap::new();
            for payload_name in &variant.payload_names {
                if seen_payload_names
                    .insert(payload_name.clone(), ())
                    .is_some()
                {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "duplicate payload field {:?} in enum variant {:?}",
                            payload_name, variant.name
                        ),
                    )
                    .with_span(variant.line, variant.column));
                }
            }
            lowered_variants.push(EnumVariantDef {
                name: variant.name.clone(),
                payload_tys,
                payload_names: variant.payload_names.clone(),
            });
        }
        lowered.insert(
            enum_decl.name.clone(),
            EnumDef {
                name: enum_decl.name.clone(),
                variants: lowered_variants,
            },
        );
    }
    Ok((lowered, variants))
}

pub(super) fn validate_trait_bounds_in_program(
    program: &syntax::Program,
    traits: &HashMap<String, TraitDef>,
) -> Result<(), Diagnostic> {
    for type_alias in &program.type_aliases {
        validate_type_param_bounds(&type_alias.type_param_bounds, traits)?;
    }
    for struct_decl in &program.structs {
        validate_type_param_bounds(&struct_decl.type_param_bounds, traits)?;
    }
    for enum_decl in &program.enums {
        validate_type_param_bounds(&enum_decl.type_param_bounds, traits)?;
    }
    for function in &program.functions {
        validate_type_param_bounds(&function.type_param_bounds, traits)?;
    }
    Ok(())
}

fn validate_type_param_bounds(
    bounds: &[syntax::TypeParamBound],
    traits: &HashMap<String, TraitDef>,
) -> Result<(), Diagnostic> {
    for bound in bounds {
        for trait_name in &bound.traits {
            if !traits.contains_key(trait_name) {
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
