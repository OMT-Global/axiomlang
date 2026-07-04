use crate::syntax;

pub(super) fn monomorphized_function_name(name: &str, type_args: &[syntax::TypeName]) -> String {
    monomorphized_name(name, type_args)
}

pub(super) fn monomorphized_type_name(name: &str, type_args: &[syntax::TypeName]) -> String {
    monomorphized_name(name, type_args)
}

fn monomorphized_name(name: &str, type_args: &[syntax::TypeName]) -> String {
    let suffix = type_args
        .iter()
        .map(type_name_monomorph_suffix)
        .collect::<Vec<_>>()
        .join("__");
    format!("{name}__{suffix}")
}

pub(super) fn is_async_runtime_type(name: &str) -> bool {
    matches!(
        name,
        "Task" | "JoinHandle" | "AsyncChannel" | "SelectResult"
    )
}

pub(super) fn is_async_runtime_intrinsic(name: &str) -> bool {
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

pub(super) fn preserves_intrinsic_type_args(name: &str) -> bool {
    is_async_runtime_intrinsic(name)
        || matches!(
            name,
            "map_get"
                | "map_contains_key"
                | "map_keys"
                | "contains"
                | "get"
                | "get_or_default"
                | "keys"
        )
}

fn type_name_monomorph_suffix(ty: &syntax::TypeName) -> String {
    match ty {
        syntax::TypeName::Int => String::from("int"),
        syntax::TypeName::Numeric(numeric) => numeric.as_str().to_string(),
        syntax::TypeName::Bool => String::from("bool"),
        syntax::TypeName::String => String::from("string"),
        syntax::TypeName::Str => String::from("str"),
        syntax::TypeName::Named(name, args) if args.is_empty() => name.clone(),
        syntax::TypeName::Named(name, args) => monomorphized_type_name(name, args),
        syntax::TypeName::Ptr(inner) => format!("ptr_{}", type_name_monomorph_suffix(inner)),
        syntax::TypeName::MutPtr(inner) => {
            format!("mutptr_{}", type_name_monomorph_suffix(inner))
        }
        syntax::TypeName::MutRef(inner) => {
            format!("mutref_{}", type_name_monomorph_suffix(inner))
        }
        syntax::TypeName::Slice(inner) => format!("slice_{}", type_name_monomorph_suffix(inner)),
        syntax::TypeName::MutSlice(inner) => {
            format!("mutslice_{}", type_name_monomorph_suffix(inner))
        }
        syntax::TypeName::LifetimeSlice(lifetime, inner) => format!(
            "lslice_{}_{}",
            sanitize_symbol_suffix(lifetime),
            type_name_monomorph_suffix(inner)
        ),
        syntax::TypeName::LifetimeMutSlice(lifetime, inner) => format!(
            "lmutslice_{}_{}",
            sanitize_symbol_suffix(lifetime),
            type_name_monomorph_suffix(inner)
        ),
        syntax::TypeName::Option(inner) => {
            format!("option_{}", type_name_monomorph_suffix(inner))
        }
        syntax::TypeName::Result(ok, err) => format!(
            "result_{}_{}",
            type_name_monomorph_suffix(ok),
            type_name_monomorph_suffix(err)
        ),
        syntax::TypeName::Tuple(elements) => format!(
            "tuple_{}",
            elements
                .iter()
                .map(type_name_monomorph_suffix)
                .collect::<Vec<_>>()
                .join("_")
        ),
        syntax::TypeName::Map(key, value) => format!(
            "map_{}_{}",
            type_name_monomorph_suffix(key),
            type_name_monomorph_suffix(value)
        ),
        syntax::TypeName::Array(inner, len) => match len {
            Some(len) => format!(
                "array_{}_{}",
                type_name_monomorph_suffix(inner),
                sanitize_symbol_suffix(len)
            ),
            None => format!("array_{}", type_name_monomorph_suffix(inner)),
        },
        syntax::TypeName::Fn(params, return_ty) => format!(
            "fn_{}_{}",
            params
                .iter()
                .map(type_name_monomorph_suffix)
                .collect::<Vec<_>>()
                .join("_"),
            type_name_monomorph_suffix(return_ty)
        ),
    }
}

fn sanitize_symbol_suffix(raw: &str) -> String {
    raw.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}
