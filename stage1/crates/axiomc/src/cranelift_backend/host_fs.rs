//! Direct-native i64 runtime lowering — filesystem (fs) capability group.
//! `fs_read`/`fs_write` intrinsic lowering, path-guard and audit helpers,
//! and the compile-time spike_fs_* scope resolvers. Extracted from
//! cranelift_backend.rs under the compiler-source decomposition ratchet
//! (#1254). Shared IR types and the recursive expr/stmt lowering hub stay
//! in the parent module and are visible here through `use super::*`.

use super::*;

pub(crate) struct I64FsReadPath {
    pub(crate) candidate: String,
    pub(crate) requested_len: usize,
}

pub(crate) fn lower_i64_fs_read_option_call_let_stmts(
    name: &str,
    inner: &Type,
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    if !matches!(inner, Type::String | Type::Str) {
        return None;
    }
    let path = i64_fs_read_path(expr, static_bindings)?;
    let file_len = i64_fs_read_file_len_expr(&path.candidate, path.requested_len, static_bindings)?;
    lower_i64_string_option_len_call_let_stmts(name, file_len, locals, local_indexes)
}

pub(crate) fn i64_fs_read_path(expr: &Expr, static_bindings: &I64StaticBindings) -> Option<I64FsReadPath> {
    if static_bindings.has_fs_write_calls {
        return None;
    }
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if name != "fs_read"
        && name != "read_file"
        && name != "std_fs_read_file"
        && !static_bindings.fs_read_wrappers.contains(name)
    {
        return None;
    }
    let [path] = args.as_slice() else {
        return None;
    };
    let package_root = static_bindings.package_root.as_deref()?;
    let fs_root = static_bindings.fs_root.as_deref()?;
    let path = i64_string_text(path, static_bindings)?;
    let requested_len = path.len();
    spike_fs_existing_candidate_for_scope(package_root, fs_root, &path).map(|path| I64FsReadPath {
        candidate: path.display().to_string(),
        requested_len,
    })
}

pub(crate) fn i64_fs_read_file_len_expr(
    path: &str,
    path_len: usize,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if static_bindings.has_fs_write_calls {
        return None;
    }
    let fs_root = static_bindings.fs_root.as_deref()?;
    let path = Path::new(path);
    let guarded = i64_runtime_fs_guard_expr(
        fs_root,
        path,
        i64_fs_runtime_parent_fallback(path)?.as_path(),
        CraneliftI64Expr::FileLen {
            path: path.display().to_string(),
            max_bytes: SPIKE_MAX_FS_READ_BYTES,
        },
    )?;
    i64_audited_fs_expr_with_success(
        "fs_read",
        path_len,
        None,
        guarded,
        static_bindings,
        CraneliftI64AuditSuccess::NonNegative,
    )
}

pub(crate) fn i64_stmts_have_fs_write_call(stmts: &[Stmt], static_bindings: &I64StaticBindings) -> bool {
    stmts
        .iter()
        .any(|stmt| i64_stmt_has_fs_write_call(stmt, static_bindings))
}

pub(crate) fn i64_stmt_has_fs_write_call(stmt: &Stmt, static_bindings: &I64StaticBindings) -> bool {
    match stmt {
        Stmt::Let { expr, .. }
        | Stmt::Print { expr, .. }
        | Stmt::Panic { message: expr, .. }
        | Stmt::Defer { expr, .. }
        | Stmt::Return { expr, .. } => i64_expr_has_fs_write_call(expr, static_bindings),
        Stmt::Assign { target, expr, .. } => {
            i64_expr_has_fs_write_call(target, static_bindings)
                || i64_expr_has_fs_write_call(expr, static_bindings)
        }
        Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            i64_expr_has_fs_write_call(cond, static_bindings)
                || i64_stmts_have_fs_write_call(then_block, static_bindings)
                || else_block
                    .as_ref()
                    .is_some_and(|stmts| i64_stmts_have_fs_write_call(stmts, static_bindings))
        }
        Stmt::While { cond, body, .. } => {
            i64_expr_has_fs_write_call(cond, static_bindings)
                || i64_stmts_have_fs_write_call(body, static_bindings)
        }
        Stmt::Match { expr, arms, .. } => {
            i64_expr_has_fs_write_call(expr, static_bindings)
                || arms
                    .iter()
                    .any(|arm| i64_stmts_have_fs_write_call(&arm.body, static_bindings))
        }
    }
}

pub(crate) fn i64_stmt_has_fs_read_call(stmt: &Stmt, static_bindings: &I64StaticBindings) -> bool {
    match stmt {
        Stmt::Let { expr, .. }
        | Stmt::Print { expr, .. }
        | Stmt::Panic { message: expr, .. }
        | Stmt::Defer { expr, .. }
        | Stmt::Return { expr, .. } => i64_expr_has_fs_read_call(expr, static_bindings),
        Stmt::Assign { target, expr, .. } => {
            i64_expr_has_fs_read_call(target, static_bindings)
                || i64_expr_has_fs_read_call(expr, static_bindings)
        }
        Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            i64_expr_has_fs_read_call(cond, static_bindings)
                || then_block
                    .iter()
                    .any(|stmt| i64_stmt_has_fs_read_call(stmt, static_bindings))
                || else_block.as_ref().is_some_and(|stmts| {
                    stmts
                        .iter()
                        .any(|stmt| i64_stmt_has_fs_read_call(stmt, static_bindings))
                })
        }
        Stmt::While { cond, body, .. } => {
            i64_expr_has_fs_read_call(cond, static_bindings)
                || body
                    .iter()
                    .any(|stmt| i64_stmt_has_fs_read_call(stmt, static_bindings))
        }
        Stmt::Match { expr, arms, .. } => {
            i64_expr_has_fs_read_call(expr, static_bindings)
                || arms.iter().any(|arm| {
                    arm.body
                        .iter()
                        .any(|stmt| i64_stmt_has_fs_read_call(stmt, static_bindings))
                })
        }
    }
}

pub(crate) fn i64_expr_has_fs_write_call(expr: &Expr, static_bindings: &I64StaticBindings) -> bool {
    match expr {
        Expr::Call { name, args, .. } => {
            i64_fs_write_intrinsic_name(name, static_bindings).is_some()
                || args
                    .iter()
                    .any(|arg| i64_expr_has_fs_write_call(arg, static_bindings))
        }
        Expr::BinaryAdd { lhs, rhs, .. }
        | Expr::BinaryCompare { lhs, rhs, .. }
        | Expr::BinaryLogic { lhs, rhs, .. }
        | Expr::Index {
            base: lhs,
            index: rhs,
            ..
        } => {
            i64_expr_has_fs_write_call(lhs, static_bindings)
                || i64_expr_has_fs_write_call(rhs, static_bindings)
        }
        Expr::Cast { expr, .. }
        | Expr::MutBorrow { expr, .. }
        | Expr::Deref { expr, .. }
        | Expr::Try { expr, .. }
        | Expr::Await { expr, .. }
        | Expr::FieldAccess { base: expr, .. }
        | Expr::TupleIndex { base: expr, .. }
        | Expr::StringBorrow { expr, .. } => i64_expr_has_fs_write_call(expr, static_bindings),
        Expr::StructLiteral { fields, .. } => fields
            .iter()
            .any(|field| i64_expr_has_fs_write_call(&field.expr, static_bindings)),
        Expr::TupleLiteral { elements, .. } | Expr::ArrayLiteral { elements, .. } => elements
            .iter()
            .any(|element| i64_expr_has_fs_write_call(element, static_bindings)),
        Expr::MapLiteral { entries, .. } => entries.iter().any(|entry| {
            i64_expr_has_fs_write_call(&entry.key, static_bindings)
                || i64_expr_has_fs_write_call(&entry.value, static_bindings)
        }),
        Expr::EnumVariant { payloads, .. } => payloads
            .iter()
            .any(|payload| i64_expr_has_fs_write_call(payload, static_bindings)),
        Expr::Closure { body, .. } => i64_expr_has_fs_write_call(body, static_bindings),
        Expr::Slice {
            base, start, end, ..
        } => {
            i64_expr_has_fs_write_call(base, static_bindings)
                || start
                    .as_ref()
                    .is_some_and(|expr| i64_expr_has_fs_write_call(expr, static_bindings))
                || end
                    .as_ref()
                    .is_some_and(|expr| i64_expr_has_fs_write_call(expr, static_bindings))
        }
        Expr::Match { expr, arms, .. } => {
            i64_expr_has_fs_write_call(expr, static_bindings)
                || arms
                    .iter()
                    .any(|arm| i64_expr_has_fs_write_call(&arm.expr, static_bindings))
        }
        Expr::Literal(_) | Expr::VarRef { .. } => false,
    }
}

pub(crate) fn i64_expr_has_fs_read_call(expr: &Expr, static_bindings: &I64StaticBindings) -> bool {
    match expr {
        Expr::Call { name, args, .. } => {
            matches!(name.as_str(), "fs_read" | "read_file" | "std_fs_read_file")
                || static_bindings.fs_read_wrappers.contains(name)
                || args
                    .iter()
                    .any(|arg| i64_expr_has_fs_read_call(arg, static_bindings))
        }
        Expr::BinaryAdd { lhs, rhs, .. }
        | Expr::BinaryCompare { lhs, rhs, .. }
        | Expr::BinaryLogic { lhs, rhs, .. }
        | Expr::Index {
            base: lhs,
            index: rhs,
            ..
        } => {
            i64_expr_has_fs_read_call(lhs, static_bindings)
                || i64_expr_has_fs_read_call(rhs, static_bindings)
        }
        Expr::Cast { expr, .. }
        | Expr::MutBorrow { expr, .. }
        | Expr::Deref { expr, .. }
        | Expr::Try { expr, .. }
        | Expr::Await { expr, .. }
        | Expr::FieldAccess { base: expr, .. }
        | Expr::TupleIndex { base: expr, .. }
        | Expr::StringBorrow { expr, .. } => i64_expr_has_fs_read_call(expr, static_bindings),
        Expr::StructLiteral { fields, .. } => fields
            .iter()
            .any(|field| i64_expr_has_fs_read_call(&field.expr, static_bindings)),
        Expr::TupleLiteral { elements, .. } | Expr::ArrayLiteral { elements, .. } => elements
            .iter()
            .any(|element| i64_expr_has_fs_read_call(element, static_bindings)),
        Expr::MapLiteral { entries, .. } => entries.iter().any(|entry| {
            i64_expr_has_fs_read_call(&entry.key, static_bindings)
                || i64_expr_has_fs_read_call(&entry.value, static_bindings)
        }),
        Expr::EnumVariant { payloads, .. } => payloads
            .iter()
            .any(|payload| i64_expr_has_fs_read_call(payload, static_bindings)),
        Expr::Closure { body, .. } => i64_expr_has_fs_read_call(body, static_bindings),
        Expr::Slice {
            base, start, end, ..
        } => {
            i64_expr_has_fs_read_call(base, static_bindings)
                || start
                    .as_ref()
                    .is_some_and(|expr| i64_expr_has_fs_read_call(expr, static_bindings))
                || end
                    .as_ref()
                    .is_some_and(|expr| i64_expr_has_fs_read_call(expr, static_bindings))
        }
        Expr::Match { expr, arms, .. } => {
            i64_expr_has_fs_read_call(expr, static_bindings)
                || arms
                    .iter()
                    .any(|arm| i64_expr_has_fs_read_call(&arm.expr, static_bindings))
        }
        Expr::Literal(_) | Expr::VarRef { .. } => false,
    }
}

pub(crate) fn lower_i64_fs_write_intrinsic_expr(
    name: &str,
    args: &[Expr],
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let name = i64_fs_write_intrinsic_name(name, static_bindings)?;
    if name == "fs_write" {
        let (path, content) = i64_fs_path_content(args, static_bindings)?;
        let path_len = path.len();
        let content_len = content.len();
        if content.len() > SPIKE_MAX_FS_WRITE_BYTES {
            return i64_audited_fs_expr(
                name,
                path_len,
                Some(content_len),
                CraneliftI64Expr::Literal(-1),
                static_bindings,
            );
        }
        let package_root = static_bindings.package_root.as_deref()?;
        let fs_root = static_bindings.fs_root.as_deref()?;
        let Some(candidate) =
            spike_fs_write_candidate_for_scope(package_root, fs_root, &path, true)
        else {
            return i64_audited_fs_expr(
                name,
                path_len,
                Some(content_len),
                CraneliftI64Expr::Literal(-1),
                static_bindings,
            );
        };
        return i64_audited_fs_expr(
            name,
            path_len,
            Some(content_len),
            i64_runtime_fs_guard_expr(
                fs_root,
                &candidate,
                i64_fs_runtime_parent_fallback(&candidate)?.as_path(),
                CraneliftI64Expr::WriteFile {
                    path: candidate.display().to_string(),
                    content,
                },
            )?,
            static_bindings,
        );
    }
    if name == "fs_append" {
        let (path, content) = i64_fs_path_content(args, static_bindings)?;
        let path_len = path.len();
        let content_len = content.len();
        if content.len() > SPIKE_MAX_FS_WRITE_BYTES {
            return i64_audited_fs_expr(
                name,
                path_len,
                Some(content_len),
                CraneliftI64Expr::Literal(-1),
                static_bindings,
            );
        }
        let package_root = static_bindings.package_root.as_deref()?;
        let fs_root = static_bindings.fs_root.as_deref()?;
        let Some(candidate) =
            spike_fs_write_candidate_for_scope(package_root, fs_root, &path, true)
        else {
            return i64_audited_fs_expr(
                name,
                path_len,
                Some(content_len),
                CraneliftI64Expr::Literal(-1),
                static_bindings,
            );
        };
        return i64_audited_fs_expr(
            name,
            path_len,
            Some(content_len),
            i64_runtime_fs_guard_expr(
                fs_root,
                &candidate,
                i64_fs_runtime_parent_fallback(&candidate)?.as_path(),
                CraneliftI64Expr::AppendFile {
                    path: candidate.display().to_string(),
                    content,
                },
            )?,
            static_bindings,
        );
    }
    if name == "fs_replace" {
        let (path, content) = i64_fs_path_content(args, static_bindings)?;
        let path_len = path.len();
        let content_len = content.len();
        if content.len() > SPIKE_MAX_FS_WRITE_BYTES {
            return i64_audited_fs_expr(
                name,
                path_len,
                Some(content_len),
                CraneliftI64Expr::Literal(-1),
                static_bindings,
            );
        }
        let package_root = static_bindings.package_root.as_deref()?;
        let fs_root = static_bindings.fs_root.as_deref()?;
        let Some(candidate) =
            spike_fs_write_candidate_for_scope(package_root, fs_root, &path, true)
        else {
            return i64_audited_fs_expr(
                name,
                path_len,
                Some(content_len),
                CraneliftI64Expr::Literal(-1),
                static_bindings,
            );
        };
        let Some(parent) = candidate.parent() else {
            return i64_audited_fs_expr(
                name,
                path_len,
                Some(content_len),
                CraneliftI64Expr::Literal(-1),
                static_bindings,
            );
        };
        let Some(file_name) = candidate.file_name() else {
            return i64_audited_fs_expr(
                name,
                path_len,
                Some(content_len),
                CraneliftI64Expr::Literal(-1),
                static_bindings,
            );
        };
        let temp_path = parent.join(format!(
            ".{}.axiom-replace.tmp",
            file_name.to_string_lossy()
        ));
        return i64_audited_fs_expr(
            name,
            path_len,
            Some(content_len),
            i64_runtime_fs_guard_expr(
                fs_root,
                &candidate,
                parent,
                CraneliftI64Expr::ReplaceFile {
                    path: candidate.display().to_string(),
                    temp_path: temp_path.display().to_string(),
                    content,
                },
            )?,
            static_bindings,
        );
    }
    if name == "fs_create" {
        let path = i64_fs_path(args, static_bindings)?;
        let path_len = path.len();
        let package_root = static_bindings.package_root.as_deref()?;
        let fs_root = static_bindings.fs_root.as_deref()?;
        let Some(candidate) =
            spike_fs_write_candidate_for_scope(package_root, fs_root, &path, true)
        else {
            return i64_audited_fs_expr(
                name,
                path_len,
                None,
                CraneliftI64Expr::Literal(-1),
                static_bindings,
            );
        };
        return i64_audited_fs_expr(
            name,
            path_len,
            None,
            i64_runtime_fs_guard_expr(
                fs_root,
                &candidate,
                i64_fs_runtime_parent_fallback(&candidate)?.as_path(),
                CraneliftI64Expr::CreateFile {
                    path: candidate.display().to_string(),
                },
            )?,
            static_bindings,
        );
    }
    if name == "fs_mkdir" {
        let path = i64_fs_path(args, static_bindings)?;
        let path_len = path.len();
        let package_root = static_bindings.package_root.as_deref()?;
        let fs_root = static_bindings.fs_root.as_deref()?;
        let Some(candidate) =
            spike_fs_write_candidate_for_scope(package_root, fs_root, &path, false)
        else {
            return i64_audited_fs_expr(
                name,
                path_len,
                None,
                CraneliftI64Expr::Literal(-1),
                static_bindings,
            );
        };
        return i64_audited_fs_expr(
            name,
            path_len,
            None,
            i64_runtime_fs_guard_expr(
                fs_root,
                &candidate,
                i64_fs_runtime_parent_fallback(&candidate)?.as_path(),
                CraneliftI64Expr::MakeDir {
                    path: candidate.display().to_string(),
                },
            )?,
            static_bindings,
        );
    }
    if name == "fs_mkdir_all" {
        let path = i64_fs_path(args, static_bindings)?;
        let path_len = path.len();
        let package_root = static_bindings.package_root.as_deref()?;
        let fs_root = static_bindings.fs_root.as_deref()?;
        let Some(candidate) =
            spike_fs_write_candidate_for_scope(package_root, fs_root, &path, true)
        else {
            return i64_audited_fs_expr(
                name,
                path_len,
                None,
                CraneliftI64Expr::Literal(-1),
                static_bindings,
            );
        };
        return i64_audited_fs_expr(
            name,
            path_len,
            None,
            i64_runtime_fs_guard_expr(
                fs_root,
                &candidate,
                i64_fs_runtime_existing_fallback(&candidate)?.as_path(),
                CraneliftI64Expr::MakeDirAll {
                    path: candidate.display().to_string(),
                },
            )?,
            static_bindings,
        );
    }
    if name == "fs_remove_file" {
        let path = i64_fs_path(args, static_bindings)?;
        let path_len = path.len();
        let package_root = static_bindings.package_root.as_deref()?;
        let fs_root = static_bindings.fs_root.as_deref()?;
        let Some(candidate) =
            spike_fs_write_candidate_for_scope(package_root, fs_root, &path, false)
        else {
            return i64_audited_fs_expr(
                name,
                path_len,
                None,
                CraneliftI64Expr::Literal(-1),
                static_bindings,
            );
        };
        return i64_audited_fs_expr(
            name,
            path_len,
            None,
            i64_runtime_fs_guard_expr(
                fs_root,
                &candidate,
                i64_fs_runtime_parent_fallback(&candidate)?.as_path(),
                CraneliftI64Expr::RemoveFile {
                    path: candidate.display().to_string(),
                },
            )?,
            static_bindings,
        );
    }
    if name == "fs_remove_dir" {
        let path = i64_fs_path(args, static_bindings)?;
        let path_len = path.len();
        let package_root = static_bindings.package_root.as_deref()?;
        let fs_root = static_bindings.fs_root.as_deref()?;
        let Some(candidate) =
            spike_fs_write_candidate_for_scope(package_root, fs_root, &path, false)
        else {
            return i64_audited_fs_expr(
                name,
                path_len,
                None,
                CraneliftI64Expr::Literal(-1),
                static_bindings,
            );
        };
        return i64_audited_fs_expr(
            name,
            path_len,
            None,
            i64_runtime_fs_guard_expr(
                fs_root,
                &candidate,
                i64_fs_runtime_parent_fallback(&candidate)?.as_path(),
                CraneliftI64Expr::RemoveDir {
                    path: candidate.display().to_string(),
                },
            )?,
            static_bindings,
        );
    }
    Some(CraneliftI64Expr::Literal(i64_fs_write_result(
        name,
        args,
        static_bindings,
    )?))
}

pub(crate) fn i64_audited_fs_expr(
    intrinsic: &str,
    path_len: usize,
    content_len: Option<usize>,
    result: CraneliftI64Expr,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    i64_audited_fs_expr_with_success(
        intrinsic,
        path_len,
        content_len,
        result,
        static_bindings,
        CraneliftI64AuditSuccess::ExitZero,
    )
}

pub(crate) fn i64_audited_fs_expr_with_success(
    intrinsic: &str,
    path_len: usize,
    content_len: Option<usize>,
    result: CraneliftI64Expr,
    static_bindings: &I64StaticBindings,
    success: CraneliftI64AuditSuccess,
) -> Option<CraneliftI64Expr> {
    let package = static_bindings.package_root.as_deref()?;
    Some(CraneliftI64Expr::AuditFs {
        intrinsic: intrinsic.to_string(),
        package: package.display().to_string(),
        path_len,
        content_len,
        success,
        result: Box::new(result),
    })
}

pub(crate) fn i64_runtime_fs_guard_expr(
    fs_root: &Path,
    path: &Path,
    fallback_path: &Path,
    result: CraneliftI64Expr,
) -> Option<CraneliftI64Expr> {
    let root = std::fs::canonicalize(fs_root).ok()?;
    Some(CraneliftI64Expr::RuntimeFsGuard {
        root: root.display().to_string(),
        path: path.display().to_string(),
        fallback_path: fallback_path.display().to_string(),
        result: Box::new(result),
    })
}

pub(crate) fn i64_fs_runtime_parent_fallback(path: &Path) -> Option<PathBuf> {
    path.parent().map(Path::to_path_buf)
}

pub(crate) fn i64_fs_runtime_existing_fallback(path: &Path) -> Option<PathBuf> {
    let mut candidate = path;
    while !candidate.exists() {
        candidate = candidate.parent()?;
    }
    Some(candidate.to_path_buf())
}

pub(crate) fn is_i64_std_fs_read_wrapper(function: &Function) -> bool {
    function.path == "<stdlib>/fs.ax" && function.source_name == "read_file"
}

pub(crate) fn i64_std_fs_write_intrinsic(function: &Function) -> Option<&'static str> {
    if function.path != "<stdlib>/fs.ax" {
        return None;
    }
    match function.source_name.as_str() {
        "write_file" => Some("fs_write"),
        "create_file" => Some("fs_create"),
        "append_file" => Some("fs_append"),
        "mkdir" => Some("fs_mkdir"),
        "mkdir_all" => Some("fs_mkdir_all"),
        "remove_file" => Some("fs_remove_file"),
        "remove_dir" => Some("fs_remove_dir"),
        "replace_file" => Some("fs_replace"),
        _ => None,
    }
}

pub(crate) fn i64_fs_write_intrinsic_name<'a>(
    name: &'a str,
    static_bindings: &'a I64StaticBindings,
) -> Option<&'a str> {
    match name {
        "fs_write" | "fs_create" | "fs_append" | "fs_mkdir" | "fs_mkdir_all" | "fs_remove_file"
        | "fs_remove_dir" | "fs_replace" => Some(name),
        "write_file" | "std_fs_write_file" => Some("fs_write"),
        "create_file" | "std_fs_create_file" => Some("fs_create"),
        "append_file" | "std_fs_append_file" => Some("fs_append"),
        "mkdir" | "std_fs_mkdir" => Some("fs_mkdir"),
        "mkdir_all" | "std_fs_mkdir_all" => Some("fs_mkdir_all"),
        "remove_file" | "std_fs_remove_file" => Some("fs_remove_file"),
        "remove_dir" | "std_fs_remove_dir" => Some("fs_remove_dir"),
        "replace_file" | "std_fs_replace_file" => Some("fs_replace"),
        _ => static_bindings
            .fs_write_wrappers
            .get(name)
            .map(String::as_str),
    }
}

pub(crate) fn is_i64_std_fs_shim_wrapper(function: &Function) -> bool {
    matches!(
        (function.path.as_str(), function.source_name.as_str()),
        (
            "<stdlib>/fs.ax",
            "read_file"
                | "write_file"
                | "create_file"
                | "append_file"
                | "mkdir"
                | "mkdir_all"
                | "remove_file"
                | "remove_dir"
                | "replace_file"
        )
    )
}

pub(crate) fn spike_fs_read_text_for_scope(package_root: &Path, fs_root: &Path, path: &str) -> Option<String> {
    let candidate = spike_fs_existing_candidate_for_scope(package_root, fs_root, path)?;
    let metadata = std::fs::metadata(&candidate).ok()?;
    if !metadata.is_file() || metadata.len() > SPIKE_MAX_FS_READ_BYTES {
        return None;
    }
    let file = std::fs::File::open(&candidate).ok()?;
    let mut reader = file.take(SPIKE_MAX_FS_READ_BYTES + 1);
    let mut content = String::new();
    if reader.read_to_string(&mut content).is_err()
        || content.len() as u64 > SPIKE_MAX_FS_READ_BYTES
    {
        return None;
    }
    Some(content)
}

pub(crate) fn i64_fs_write_result(
    name: &str,
    args: &[Expr],
    static_bindings: &I64StaticBindings,
) -> Option<i64> {
    let package_root = static_bindings.package_root.as_deref()?;
    let fs_root = static_bindings.fs_root.as_deref()?;
    match name {
        "fs_write" => {
            let (path, content) = i64_fs_path_content(args, static_bindings)?;
            if content.len() > SPIKE_MAX_FS_WRITE_BYTES {
                return Some(-1);
            }
            Some(
                spike_fs_write_candidate_for_scope(package_root, fs_root, &path, false)
                    .and_then(|candidate| std::fs::write(candidate, content).ok())
                    .map(|()| 0)
                    .unwrap_or(-1),
            )
        }
        "fs_create" => {
            let path = i64_fs_path(args, static_bindings)?;
            Some(
                spike_fs_write_candidate_for_scope(package_root, fs_root, &path, false)
                    .and_then(|candidate| {
                        std::fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(candidate)
                            .ok()
                    })
                    .map(|_| 0)
                    .unwrap_or(-1),
            )
        }
        "fs_append" => {
            let (path, content) = i64_fs_path_content(args, static_bindings)?;
            if content.len() > SPIKE_MAX_FS_WRITE_BYTES {
                return Some(-1);
            }
            Some(
                spike_fs_write_candidate_for_scope(package_root, fs_root, &path, false)
                    .and_then(|candidate| {
                        let mut file = std::fs::OpenOptions::new()
                            .append(true)
                            .create(true)
                            .open(candidate)
                            .ok()?;
                        std::io::Write::write_all(&mut file, content.as_bytes()).ok()
                    })
                    .map(|()| 0)
                    .unwrap_or(-1),
            )
        }
        "fs_mkdir" => {
            let path = i64_fs_path(args, static_bindings)?;
            Some(
                spike_fs_write_candidate_for_scope(package_root, fs_root, &path, false)
                    .and_then(|candidate| std::fs::create_dir(candidate).ok())
                    .map(|()| 0)
                    .unwrap_or(-1),
            )
        }
        "fs_mkdir_all" => {
            let path = i64_fs_path(args, static_bindings)?;
            Some(
                spike_fs_write_candidate_for_scope(package_root, fs_root, &path, true)
                    .and_then(|candidate| std::fs::create_dir_all(candidate).ok())
                    .map(|()| 0)
                    .unwrap_or(-1),
            )
        }
        "fs_remove_file" => {
            let path = i64_fs_path(args, static_bindings)?;
            Some(
                spike_fs_existing_candidate_for_scope(package_root, fs_root, &path)
                    .and_then(|candidate| {
                        std::fs::metadata(&candidate)
                            .ok()
                            .filter(|metadata| metadata.is_file())?;
                        std::fs::remove_file(candidate).ok()
                    })
                    .map(|()| 0)
                    .unwrap_or(-1),
            )
        }
        "fs_remove_dir" => {
            let path = i64_fs_path(args, static_bindings)?;
            Some(
                spike_fs_existing_candidate_for_scope(package_root, fs_root, &path)
                    .and_then(|candidate| {
                        std::fs::metadata(&candidate)
                            .ok()
                            .filter(|metadata| metadata.is_dir())?;
                        std::fs::remove_dir(candidate).ok()
                    })
                    .map(|()| 0)
                    .unwrap_or(-1),
            )
        }
        "fs_replace" => {
            let (path, content) = i64_fs_path_content(args, static_bindings)?;
            if content.len() > SPIKE_MAX_FS_WRITE_BYTES {
                return Some(-1);
            }
            Some(
                spike_fs_write_candidate_for_scope(package_root, fs_root, &path, false)
                    .and_then(|candidate| std::fs::write(candidate, content).ok())
                    .map(|()| 0)
                    .unwrap_or(-1),
            )
        }
        _ => None,
    }
}

pub(crate) fn i64_fs_path(args: &[Expr], static_bindings: &I64StaticBindings) -> Option<String> {
    let [path] = args else {
        return None;
    };
    i64_string_text(path, static_bindings)
}

pub(crate) fn i64_fs_path_content(
    args: &[Expr],
    static_bindings: &I64StaticBindings,
) -> Option<(String, String)> {
    let [path, content] = args else {
        return None;
    };
    Some((
        i64_string_text(path, static_bindings)?,
        i64_string_text(content, static_bindings)?,
    ))
}

pub(crate) fn spike_fs_existing_candidate_for_scope(
    package_root: &Path,
    fs_root: &Path,
    path: &str,
) -> Option<PathBuf> {
    let canonical_root = std::fs::canonicalize(fs_root).ok()?;
    for candidate in spike_fs_join_candidates(package_root, fs_root, path)? {
        let Ok(canonical_candidate) = std::fs::canonicalize(candidate) else {
            continue;
        };
        if canonical_candidate.starts_with(&canonical_root) {
            return Some(canonical_candidate);
        }
    }
    None
}

pub(crate) fn spike_fs_write_candidate_for_scope(
    package_root: &Path,
    fs_root: &Path,
    path: &str,
    allow_missing_ancestors: bool,
) -> Option<PathBuf> {
    let candidate = spike_fs_join_candidate(package_root, path)?;
    let Ok(canonical_root) = std::fs::canonicalize(fs_root) else {
        return None;
    };
    if let Ok(canonical_candidate) = std::fs::canonicalize(&candidate) {
        return canonical_candidate
            .starts_with(canonical_root)
            .then_some(canonical_candidate);
    }
    if matches!(
        std::fs::symlink_metadata(&candidate),
        Ok(metadata) if metadata.file_type().is_symlink()
    ) {
        return None;
    }
    let parent = candidate.parent()?;
    if !allow_missing_ancestors {
        let Ok(canonical_parent) = std::fs::canonicalize(parent) else {
            return None;
        };
        if !canonical_parent.starts_with(&canonical_root) {
            return None;
        }
        let file_name = candidate.file_name()?;
        return Some(canonical_parent.join(file_name));
    }
    let mut ancestor = parent;
    while !ancestor.exists() {
        let parent = ancestor.parent()?;
        ancestor = parent;
    }
    let Ok(canonical_ancestor) = std::fs::canonicalize(ancestor) else {
        return None;
    };
    canonical_ancestor
        .starts_with(canonical_root)
        .then_some(candidate)
}

pub(crate) fn spike_fs_join_candidate(package_root: &Path, path: &str) -> Option<PathBuf> {
    spike_fs_join_candidates(package_root, package_root, path)?
        .into_iter()
        .next()
}

pub(crate) fn spike_fs_join_candidates(
    package_root: &Path,
    fs_root: &Path,
    path: &str,
) -> Option<Vec<PathBuf>> {
    let requested = Path::new(path);
    if requested.as_os_str().is_empty()
        || requested
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    let primary = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        package_root.join(requested)
    };
    let mut candidates = vec![primary];
    if !requested.is_absolute() && fs_root != package_root {
        candidates.push(fs_root.join(requested));
    }
    Some(candidates)
}

