//! Typing for the read-only filesystem intrinsics: `fs_read` and the metadata
//! queries `fs_file_exists` and `fs_file_size` behind `std/fs.ax`'s
//! `file_exists` and `file_size` (#1582).

use super::*;

/// Lowers a one-path read-only filesystem intrinsic. All three require the
/// `fs` capability and take one `string` path. `fs_read` yields
/// `Option<string>`, `fs_file_exists` `bool` and `fs_file_size` `int`.
pub(super) fn lower_fs_read_intrinsic(
    name: &str,
    args: &[syntax::Expr],
    line: usize,
    column: usize,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Expr, Diagnostic> {
    require_capability(ctx.capabilities, CapabilityKind::Fs, name, line, column)?;
    if args.len() != 1 {
        return Err(Diagnostic::new(
            "type",
            format!("{name} expects 1 argument, got {}", args.len()),
        )
        .with_span(line, column));
    }
    let lowered = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
    if lowered.ty() != &Type::String {
        return Err(Diagnostic::new(
            "type",
            format!("{name} expects a string argument, got {}", lowered.ty()),
        )
        .with_span(args[0].line(), args[0].column()));
    }
    move_lowered_value(&lowered, env)?;
    let ty = match name {
        "fs_file_exists" => Type::Bool,
        "fs_file_size" => Type::Int,
        _ => Type::Option(Box::new(Type::String)),
    };
    Ok(Expr::Call {
        span: SourceSpan::point(line, column),
        name: name.to_string(),
        args: vec![lowered],
        ty,
    })
}
