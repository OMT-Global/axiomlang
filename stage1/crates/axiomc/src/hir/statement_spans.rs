//! Gives every statement-level diagnostic a source location (#1786).
//!
//! Many HIR checks, ownership moves among them, build a diagnostic without a
//! span because they only see an expression or a binding. The innermost
//! statement that was being lowered is always known, so a diagnostic that
//! leaves a statement without a line takes that statement's line and column.

use super::{Binding, LowerContext, Stmt, lower_stmt_unspanned};
use crate::diagnostics::Diagnostic;
use crate::syntax;
use std::collections::HashMap;

pub(super) fn lower_stmt(
    stmt: &syntax::Stmt,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Stmt, Diagnostic> {
    lower_stmt_unspanned(stmt, env, ctx).map_err(|error| {
        if error.line.is_some() {
            return error;
        }
        error.with_span(stmt.line(), stmt.column())
    })
}
