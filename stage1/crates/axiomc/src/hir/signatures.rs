use super::model::{EnumDef, StructDef, TraitDef, TraitMethodDef, Type};
use super::types::lower_type;
use crate::borrowck;
use crate::diagnostics::Diagnostic;
use crate::syntax;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub(super) struct FunctionSig {
    pub(super) source_name: String,
    pub(super) source_path: String,
    pub(super) params: Vec<Type>,
    pub(super) return_ty: Type,
    pub(super) borrow_return_params: Vec<usize>,
    pub(super) is_extern: bool,
    pub(super) is_const: bool,
}

#[derive(Debug, Clone)]
pub(super) struct MethodSig {
    pub(super) function_name: String,
    pub(super) params: Vec<Type>,
    pub(super) return_ty: Type,
    pub(super) borrow_return_params: Vec<usize>,
    pub(super) has_self: bool,
}

pub(super) fn function_symbol_name(function: &syntax::Function) -> String {
    match &function.impl_target {
        Some(target) => format!("{target}__{}", function.name),
        None => function.name.clone(),
    }
}

fn substitute_self_type_name(
    ty: &syntax::TypeName,
    self_ty: &syntax::TypeName,
) -> syntax::TypeName {
    match ty {
        syntax::TypeName::Named(name, args) if name == "Self" && args.is_empty() => self_ty.clone(),
        syntax::TypeName::Named(name, args) => syntax::TypeName::Named(
            name.clone(),
            args.iter()
                .map(|arg| substitute_self_type_name(arg, self_ty))
                .collect(),
        ),
        syntax::TypeName::Ptr(inner) => {
            syntax::TypeName::Ptr(Box::new(substitute_self_type_name(inner, self_ty)))
        }
        syntax::TypeName::MutPtr(inner) => {
            syntax::TypeName::MutPtr(Box::new(substitute_self_type_name(inner, self_ty)))
        }
        syntax::TypeName::MutRef(inner) => {
            syntax::TypeName::MutRef(Box::new(substitute_self_type_name(inner, self_ty)))
        }
        syntax::TypeName::Slice(inner) => {
            syntax::TypeName::Slice(Box::new(substitute_self_type_name(inner, self_ty)))
        }
        syntax::TypeName::MutSlice(inner) => {
            syntax::TypeName::MutSlice(Box::new(substitute_self_type_name(inner, self_ty)))
        }
        syntax::TypeName::LifetimeSlice(lifetime, inner) => syntax::TypeName::LifetimeSlice(
            lifetime.clone(),
            Box::new(substitute_self_type_name(inner, self_ty)),
        ),
        syntax::TypeName::LifetimeMutSlice(lifetime, inner) => syntax::TypeName::LifetimeMutSlice(
            lifetime.clone(),
            Box::new(substitute_self_type_name(inner, self_ty)),
        ),
        syntax::TypeName::Option(inner) => {
            syntax::TypeName::Option(Box::new(substitute_self_type_name(inner, self_ty)))
        }
        syntax::TypeName::Result(ok, err) => syntax::TypeName::Result(
            Box::new(substitute_self_type_name(ok, self_ty)),
            Box::new(substitute_self_type_name(err, self_ty)),
        ),
        syntax::TypeName::Tuple(elements) => syntax::TypeName::Tuple(
            elements
                .iter()
                .map(|element| substitute_self_type_name(element, self_ty))
                .collect(),
        ),
        syntax::TypeName::Map(key, value) => syntax::TypeName::Map(
            Box::new(substitute_self_type_name(key, self_ty)),
            Box::new(substitute_self_type_name(value, self_ty)),
        ),
        syntax::TypeName::Array(inner, len) => syntax::TypeName::Array(
            Box::new(substitute_self_type_name(inner, self_ty)),
            len.clone(),
        ),
        syntax::TypeName::Fn(params, return_ty) => syntax::TypeName::Fn(
            params
                .iter()
                .map(|param| substitute_self_type_name(param, self_ty))
                .collect(),
            Box::new(substitute_self_type_name(return_ty, self_ty)),
        ),
        syntax::TypeName::Int => syntax::TypeName::Int,
        syntax::TypeName::Numeric(numeric) => syntax::TypeName::Numeric(*numeric),
        syntax::TypeName::Bool => syntax::TypeName::Bool,
        syntax::TypeName::String => syntax::TypeName::String,
        syntax::TypeName::Str => syntax::TypeName::Str,
    }
}

pub(super) fn collect_function_signatures(
    functions: &[syntax::Function],
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
    consts: &HashMap<String, syntax::ConstDecl>,
) -> Result<HashMap<String, FunctionSig>, Diagnostic> {
    let mut signatures = HashMap::new();
    for function in functions {
        let return_ty = lower_type(
            &function.return_ty,
            structs,
            enums,
            aliases,
            consts,
            function.line,
            function.column,
        )?;
        let mut params = Vec::new();
        if let Some(target_name) = &function.impl_target {
            let self_ty = lower_type(
                &syntax::TypeName::Named(target_name.clone(), Vec::new()),
                structs,
                enums,
                aliases,
                consts,
                function.line,
                function.column,
            )?;
            if function.receiver.is_some() {
                params.push(self_ty);
            }
        }
        for param in &function.params {
            params.push(lower_type(
                &param.ty,
                structs,
                enums,
                aliases,
                consts,
                param.line,
                param.column,
            )?);
        }
        let signature_return_ty = if function.is_async {
            Type::Task(Box::new(return_ty.clone()))
        } else {
            return_ty.clone()
        };
        let borrow_return_params = classify_borrow_return(
            &params,
            &signature_return_ty,
            structs,
            enums,
            function.line,
            function.column,
        )?;
        if signatures
            .insert(
                function_symbol_name(function),
                FunctionSig {
                    source_name: function.source_name.clone(),
                    source_path: function.path.clone(),
                    params,
                    return_ty: signature_return_ty,
                    borrow_return_params,
                    is_extern: function.is_extern,
                    is_const: function.is_const,
                },
            )
            .is_some()
        {
            let message = if let Some(target) = &function.impl_target {
                format!(
                    "duplicate impl method {:?} for {:?}",
                    function.source_name, target
                )
            } else {
                format!("duplicate function {:?}", function.name)
            };
            return Err(Diagnostic::new("type", message).with_span(function.line, function.column));
        }
    }
    Ok(signatures)
}

pub(super) fn collect_method_signatures(
    functions: &[syntax::Function],
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
    consts: &HashMap<String, syntax::ConstDecl>,
) -> Result<HashMap<String, HashMap<String, MethodSig>>, Diagnostic> {
    let mut methods: HashMap<String, HashMap<String, MethodSig>> = HashMap::new();
    for function in functions {
        let Some(target_name) = &function.impl_target else {
            continue;
        };
        if !function.type_params.is_empty() {
            return Err(Diagnostic::new(
                "type",
                format!("impl method {:?} cannot be generic yet", function.name),
            )
            .with_span(function.line, function.column));
        }
        let target_ty = lower_type(
            &syntax::TypeName::Named(target_name.clone(), Vec::new()),
            structs,
            enums,
            aliases,
            consts,
            function.line,
            function.column,
        )?;
        if !matches!(target_ty, Type::Struct(_) | Type::Enum(_)) {
            return Err(Diagnostic::new(
                "type",
                format!("impl target {:?} must be a struct or enum", target_name),
            )
            .with_span(function.line, function.column));
        }
        let return_ty = lower_type(
            &function.return_ty,
            structs,
            enums,
            aliases,
            consts,
            function.line,
            function.column,
        )?;
        let mut params = Vec::new();
        if function.receiver.is_some() {
            params.push(target_ty.clone());
        }
        for param in &function.params {
            params.push(lower_type(
                &param.ty,
                structs,
                enums,
                aliases,
                consts,
                param.line,
                param.column,
            )?);
        }
        let return_ty = if function.is_async {
            Type::Task(Box::new(return_ty))
        } else {
            return_ty
        };
        let borrow_return_params = classify_borrow_return(
            &params,
            &return_ty,
            structs,
            enums,
            function.line,
            function.column,
        )?;
        let method = MethodSig {
            function_name: function_symbol_name(function),
            params,
            return_ty,
            borrow_return_params,
            has_self: function.receiver.is_some(),
        };
        let entry = methods.entry(target_name.clone()).or_default();
        if entry.insert(function.source_name.clone(), method).is_some() {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "duplicate impl method {:?} for {:?}",
                    function.name, target_name
                ),
            )
            .with_span(function.line, function.column));
        }
    }
    Ok(methods)
}

pub(super) fn validate_trait_impls(
    functions: &[syntax::Function],
    traits: &HashMap<String, TraitDef>,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
    consts: &HashMap<String, syntax::ConstDecl>,
    methods: &HashMap<String, HashMap<String, MethodSig>>,
) -> Result<(), Diagnostic> {
    let mut impls: HashMap<(String, String), Vec<&syntax::Function>> = HashMap::new();
    for function in functions {
        let Some(trait_name) = &function.impl_trait else {
            continue;
        };
        let Some(target_name) = &function.impl_target else {
            continue;
        };
        if !traits.contains_key(trait_name) {
            return Err(Diagnostic::new(
                "type",
                format!("impl references unknown trait {trait_name:?}"),
            )
            .with_span(function.line, function.column));
        }
        let target_ty = lower_type(
            &syntax::TypeName::Named(target_name.clone(), Vec::new()),
            structs,
            enums,
            aliases,
            consts,
            function.line,
            function.column,
        )?;
        if !matches!(target_ty, Type::Struct(_) | Type::Enum(_)) {
            return Err(Diagnostic::new(
                "type",
                format!("impl target {target_name:?} must be a local struct or enum"),
            )
            .with_span(function.line, function.column));
        }
        impls
            .entry((trait_name.clone(), target_name.clone()))
            .or_default()
            .push(function);
    }

    for ((trait_name, target_name), impl_functions) in impls {
        let trait_def = traits
            .get(&trait_name)
            .expect("trait impl keys are validated while grouping");
        let method_sigs = methods.get(&target_name).ok_or_else(|| {
            Diagnostic::new(
                "type",
                format!("impl {trait_name} for {target_name} does not define any methods"),
            )
            .with_span(impl_functions[0].line, impl_functions[0].column)
        })?;
        for required in &trait_def.methods {
            let implementation = impl_functions
                .iter()
                .find(|function| function.source_name == required.name)
                .ok_or_else(|| {
                    Diagnostic::new(
                        "type",
                        format!(
                            "impl {trait_name} for {target_name} is missing required method {:?}",
                            required.name
                        ),
                    )
                    .with_span(impl_functions[0].line, impl_functions[0].column)
                })?;
            validate_trait_method_signature(
                required,
                implementation,
                &trait_name,
                &target_name,
                structs,
                enums,
                aliases,
                consts,
                method_sigs,
            )?;
        }
    }
    Ok(())
}

fn validate_trait_method_signature(
    required: &TraitMethodDef,
    implementation: &syntax::Function,
    trait_name: &str,
    target_name: &str,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
    consts: &HashMap<String, syntax::ConstDecl>,
    method_sigs: &HashMap<String, MethodSig>,
) -> Result<(), Diagnostic> {
    if required.has_self != implementation.receiver.is_some() {
        return Err(Diagnostic::new(
            "type",
            format!(
                "impl {trait_name} for {target_name} method {:?} has an incompatible self receiver",
                required.name
            ),
        )
        .with_span(implementation.line, implementation.column));
    }
    if required.params.len() != implementation.params.len() {
        return Err(Diagnostic::new(
            "type",
            format!(
                "impl {trait_name} for {target_name} method {:?} expects {} parameters, got {}",
                required.name,
                required.params.len(),
                implementation.params.len()
            ),
        )
        .with_span(implementation.line, implementation.column));
    }
    let target_ty = syntax::TypeName::Named(target_name.to_string(), Vec::new());
    for (expected, actual) in required.params.iter().zip(&implementation.params) {
        let expected = lower_type(
            &substitute_self_type_name(expected, &target_ty),
            structs,
            enums,
            aliases,
            consts,
            actual.line,
            actual.column,
        )?;
        let actual = lower_type(
            &actual.ty,
            structs,
            enums,
            aliases,
            consts,
            actual.line,
            actual.column,
        )?;
        if expected != actual {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "impl {trait_name} for {target_name} method {:?} has parameter type {}, expected {}",
                    required.name, actual, expected
                ),
            )
            .with_span(implementation.line, implementation.column));
        }
    }
    let expected_return = lower_type(
        &substitute_self_type_name(&required.return_ty, &target_ty),
        structs,
        enums,
        aliases,
        consts,
        implementation.line,
        implementation.column,
    )?;
    let method_sig = method_sigs
        .get(&implementation.source_name)
        .expect("method signature should be collected before trait impl validation");
    if expected_return != method_sig.return_ty {
        return Err(Diagnostic::new(
            "type",
            format!(
                "impl {trait_name} for {target_name} method {:?} returns {}, expected {}",
                required.name, method_sig.return_ty, expected_return
            ),
        )
        .with_span(implementation.line, implementation.column));
    }
    Ok(())
}

fn classify_borrow_return(
    params: &[Type],
    return_ty: &Type,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    line: usize,
    column: usize,
) -> Result<Vec<usize>, Diagnostic> {
    borrowck::classify_borrow_return(params, return_ty, structs, enums, line, column)
}
