//! Borrow-check support for stage1 lowering.
//!
//! HIR lowering still performs the syntax-to-HIR walk, but ownership-specific
//! borrow state transitions, lifetime/origin classification, and diagnostics live
//! behind this module boundary so the current lowering-time checks use the same
//! API that a later standalone pass can consume.

use crate::diagnostics::Diagnostic;
use crate::hir::{EnumDef, StructDef, Type};
use std::collections::{HashMap, HashSet};

pub(crate) const LOOP_MOVE_OUTER_NON_COPY: &str = "loop_move_outer_non_copy";
pub(crate) const BORROW_RETURN_REQUIRES_PARAM_ORIGIN: &str = "borrow_return_requires_param_origin";
pub(crate) const MOVE_WHILE_BORROWED: &str = "move_while_borrowed";
pub(crate) const USE_AFTER_MOVE: &str = "use_after_move";
pub(crate) const SHARED_BORROW_WHILE_MUTABLE_LIVE: &str = "shared_borrow_while_mutable_live";
pub(crate) const MUTABLE_BORROW_WHILE_MUTABLE_LIVE: &str = "mutable_borrow_while_mutable_live";
pub(crate) const MUTABLE_BORROW_WHILE_SHARED_LIVE: &str = "mutable_borrow_while_shared_live";

/// Borrow-checker's read-only view of the lowered type universe.
///
/// Keeping this wrapper local to `borrowck` prevents ownership analysis from
/// reaching through MIR lowering details as the pass grows.
pub(crate) struct BorrowIr<'a> {
    structs: &'a HashMap<String, StructDef>,
    enums: &'a HashMap<String, EnumDef>,
}

impl<'a> BorrowIr<'a> {
    pub(crate) fn new(
        structs: &'a HashMap<String, StructDef>,
        enums: &'a HashMap<String, EnumDef>,
    ) -> Self {
        Self { structs, enums }
    }

    fn contains_borrowed_slice_type(&self, ty: &Type) -> bool {
        contains_borrowed_slice_type_inner(
            ty,
            self.structs,
            self.enums,
            &mut HashSet::new(),
            &mut HashSet::new(),
        )
    }

    fn contains_mut_borrowed_slice_type(&self, ty: &Type) -> bool {
        contains_mut_borrowed_slice_type_inner(
            ty,
            self.structs,
            self.enums,
            &mut HashSet::new(),
            &mut HashSet::new(),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceSpan {
    pub(crate) line: usize,
    pub(crate) column: usize,
}

impl SourceSpan {
    pub(crate) const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct BorrowState {
    pub(crate) active_shared_or_mutable: usize,
    pub(crate) active_mutable: usize,
}

impl BorrowState {
    pub(crate) fn begin_borrow(
        &mut self,
        owner_name: &str,
        requested: BorrowKind,
        span: SourceSpan,
    ) -> Result<(), Diagnostic> {
        if let Some(diagnostic) = borrow_conflict_error(
            owner_name,
            requested,
            self.active_shared_or_mutable,
            self.active_mutable,
            span,
        ) {
            return Err(diagnostic);
        }
        self.active_shared_or_mutable += 1;
        if matches!(requested, BorrowKind::Mutable) {
            self.active_mutable += 1;
        }
        Ok(())
    }

    pub(crate) fn end_borrow(&mut self, kind: BorrowKind) {
        self.active_shared_or_mutable = self.active_shared_or_mutable.saturating_sub(1);
        if matches!(kind, BorrowKind::Mutable) {
            self.active_mutable = self.active_mutable.saturating_sub(1);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BorrowKind {
    Shared,
    Mutable,
}

pub(crate) fn ownership_error(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("ownership", message).with_code(code)
}

pub(crate) fn borrow_conflict_error(
    owner_name: &str,
    requested: BorrowKind,
    active_shared: usize,
    active_mutable: usize,
    span: SourceSpan,
) -> Option<Diagnostic> {
    match requested {
        BorrowKind::Shared if active_mutable > 0 => Some(
            ownership_error(
                SHARED_BORROW_WHILE_MUTABLE_LIVE,
                format!(
                    "cannot create shared borrow of value {owner_name:?} while a mutable borrow is still live"
                ),
            )
            .with_span(span.line, span.column),
        ),
        BorrowKind::Mutable if active_mutable > 0 => Some(
            ownership_error(
                MUTABLE_BORROW_WHILE_MUTABLE_LIVE,
                format!(
                    "cannot create mutable borrow of value {owner_name:?} while another mutable borrow is still live"
                ),
            )
            .with_span(span.line, span.column),
        ),
        BorrowKind::Mutable if active_shared > 0 => Some(
            ownership_error(
                MUTABLE_BORROW_WHILE_SHARED_LIVE,
                format!(
                    "cannot create mutable borrow of value {owner_name:?} while a shared borrow is still live"
                ),
            )
            .with_help("drop the shared borrow before creating a mutable borrow")
            .with_span(span.line, span.column),
        ),
        _ => None,
    }
}

pub(crate) fn classify_borrow_return(
    params: &[Type],
    return_ty: &Type,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    line: usize,
    column: usize,
) -> Result<Vec<usize>, Diagnostic> {
    let ir = BorrowIr::new(structs, enums);
    if !ir.contains_borrowed_slice_type(return_ty) {
        return Ok(Vec::new());
    }
    let matches = params
        .iter()
        .enumerate()
        .filter_map(|(index, ty)| ir.contains_borrowed_slice_type(ty).then_some(index))
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return Err(Diagnostic::new(
            "type",
            "borrowed return functions must take at least one borrowed parameter in stage1",
        )
        .with_span(line, column));
    }
    Ok(matches)
}

pub(crate) fn borrow_kind_for_type(
    ty: &Type,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> Option<BorrowKind> {
    let ir = BorrowIr::new(structs, enums);
    if ir.contains_mut_borrowed_slice_type(ty) {
        Some(BorrowKind::Mutable)
    } else if ir.contains_borrowed_slice_type(ty) {
        Some(BorrowKind::Shared)
    } else {
        None
    }
}

pub(crate) fn contains_borrowed_slice_type(
    ty: &Type,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> bool {
    BorrowIr::new(structs, enums).contains_borrowed_slice_type(ty)
}

fn contains_borrowed_slice_type_inner(
    ty: &Type,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    visiting_structs: &mut HashSet<String>,
    visiting_enums: &mut HashSet<String>,
) -> bool {
    match ty {
        Type::Slice(_) | Type::MutSlice(_) => true,
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
        Type::Array(inner)
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
        Type::Error | Type::Int | Type::Bool | Type::String | Type::Ptr(_) | Type::MutPtr(_) => {
            false
        }
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
        Type::MutSlice(_) => true,
        Type::Slice(_)
        | Type::Error
        | Type::Int
        | Type::Bool
        | Type::String
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
        Type::Array(inner)
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
