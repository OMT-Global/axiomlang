//! Ownership facts follow nearest-loop edges, independently of function returns.
use super::*;
use std::cell::RefCell;
use std::rc::Rc;

type Environment = HashMap<String, Binding>;
type ArmResult = (Vec<Stmt>, Environment, bool);
pub(super) type LoopEdges = Rc<RefCell<Vec<(EdgeKind, Environment)>>>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EdgeKind {
    Break,
    Continue,
}

pub(super) fn edge_mark(ctx: &LowerContext<'_>) -> usize {
    ctx.loop_edges.as_ref().map_or(0, |edges| edges.borrow().len())
}

pub(super) fn capture_edge(ctx: &LowerContext<'_>, env: &Environment, kind: EdgeKind) {
    if let Some(edges) = &ctx.loop_edges {
        edges.borrow_mut().push((kind, env.clone()));
    }
}

pub(super) fn release_block_edges(
    ctx: &LowerContext<'_>,
    start: usize,
    scope_names: &HashSet<String>,
) {
    if let Some(edges) = &ctx.loop_edges {
        for (_, env) in &mut edges.borrow_mut()[start..] {
            release_scope_borrows(env, scope_names);
        }
    }
}

pub(super) fn release_match_edges(
    ctx: &LowerContext<'_>,
    start: usize,
    before: &Environment,
    borrow_kind: Option<BorrowKind>,
    borrowed_owners: &HashSet<BorrowedOwner>,
    reuse_existing_binding: bool,
) {
    if let Some(edges) = &ctx.loop_edges {
        for (_, env) in &mut edges.borrow_mut()[start..] {
            // Payload names alias the match acquisition; they do not acquire
            // another borrow. Block-local acquisitions were released already.
            env.retain(|name, _| before.contains_key(name));
            if let Some(kind) = borrow_kind {
                if !reuse_existing_binding {
                    release_active_borrow_owners(borrowed_owners, env, kind);
                }
            }
        }
    }
}

pub(super) fn lower_match_arm_body(
    arm: &MatchArmInput,
    env: &mut Environment,
    ctx: &LowerContext<'_>,
    cache: &mut HashMap<String, ArmResult>,
) -> Result<ArmResult, Diagnostic> {
    // Cached HIR has no captured ownership edges. Keep the non-loop fast path.
    if ctx.loop_edges.is_none() && arm.ignore_payloads && arm.bindings.is_empty() {
        let key = format!("{:?}", arm.body);
        if let Some(lowered) = cache.get(&key) {
            return Ok(lowered.clone());
        }
        let lowered = lower_block(&arm.body, env, ctx)?;
        cache.insert(key, lowered.clone());
        Ok(lowered)
    } else {
        lower_block(&arm.body, env, ctx)
    }
}

pub(super) fn lower_while(
    cond: &syntax::Expr,
    body: &[syntax::Stmt],
    line: usize,
    column: usize,
    env: &mut Environment,
    ctx: &LowerContext<'_>,
    depth_ctx: &LowerContext<'_>,
) -> Result<Stmt, Diagnostic> {
    let header = env.clone();
    let lowered_cond = lower_expr(cond, env, ctx)?;
    if lowered_cond.ty() != &Type::Bool {
        return Err(Diagnostic::new(
            "type",
            format!("while condition expects bool, got {}", lowered_cond.ty()),
        ).with_span(line, column));
    }
    if static_bool_value(&lowered_cond) == Some(false) {
        return Ok(Stmt::While {
            cond: lowered_cond,
            body: Vec::new(),
            span: SourceSpan::point(line, column),
        });
    }
    // The condition executes even on a zero-body exit. A backedge, however,
    // must preserve availability from before that condition was evaluated.
    let body_entry = env.clone();
    let edges = Rc::new(RefCell::new(Vec::new()));
    // The caller (hir driver) already incremented loop_depth on depth_ctx.
    let mut loop_ctx = depth_ctx.clone();
    loop_ctx.loop_edges = Some(Rc::clone(&edges));
    let mut body_env = body_entry.clone();
    let (body, body_after, body_terminates) = lower_block(body, &mut body_env, &loop_ctx)?;
    let edges = edges.borrow();
    for (kind, after) in edges.iter() {
        if *kind == EdgeKind::Continue {
            validate_backedge(&header, after, line, column)?;
        }
    }
    if !body_terminates {
        validate_backedge(&header, &body_after, line, column)?;
    }
    *env = body_entry;
    // Function return/panic states never enter this collector. Valid backedges
    // cannot add moves, but can contribute conservative live-borrow facts.
    for (_, after) in edges.iter() {
        join_loop_exit(env, after);
    }
    if !body_terminates {
        join_loop_exit(env, &body_after);
    }
    Ok(Stmt::While {
        cond: lowered_cond,
        body,
        span: SourceSpan::point(line, column),
    })
}

fn validate_backedge(
    header: &Environment,
    after: &Environment,
    line: usize,
    column: usize,
) -> Result<(), Diagnostic> {
    // Sort diagnostics independently of randomized HashMap iteration.
    let mut names = header.keys().collect::<Vec<_>>();
    names.sort();
    for name in names {
        let before = &header[name];
        if before.moved || before.ty.is_copy() {
            continue;
        }
        if let Some(binding) = after.get(name) {
            if binding.moved || !binding.moved_projections.is_subset(&before.moved_projections) {
                return Err(ownership_error(
                    OWNERSHIP_LOOP_MOVE_OUTER_NON_COPY,
                    format!(
                        "cannot move non-copy value `{name}` inside loop body or condition — \
                         value would not be available on subsequent iterations"
                    ),
                ).with_span(line, column));
            }
        }
    }
    Ok(())
}

fn join_loop_exit(env: &mut Environment, after: &Environment) {
    for (name, binding) in env {
        if let Some(exit) = after.get(name) {
            binding.moved |= exit.moved;
            binding.moved_projections.extend(exit.moved_projections.iter().cloned());
            binding.active_borrow_count = binding.active_borrow_count.max(exit.active_borrow_count);
            binding.active_mut_borrow_count = binding.active_mut_borrow_count.max(exit.active_mut_borrow_count);
            for (projection, state) in &exit.active_borrows {
                let current = binding.active_borrows.entry(projection.clone()).or_default();
                current.active_shared_or_mutable = current.active_shared_or_mutable.max(state.active_shared_or_mutable);
                current.active_mutable = current.active_mutable.max(state.active_mutable);
            }
        }
    }
}
