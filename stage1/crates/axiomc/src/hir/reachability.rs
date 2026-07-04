use crate::syntax;
use std::collections::{HashMap, HashSet, VecDeque};

pub(super) fn reachable_function_names(program: &syntax::Program) -> HashSet<String> {
    let functions_by_name: HashMap<&str, &syntax::Function> = program
        .functions
        .iter()
        .map(|function| (function.name.as_str(), function))
        .collect();
    let mut reachable = HashSet::new();
    let mut queue = VecDeque::new();
    for stmt in &program.stmts {
        collect_stmt_calls(stmt, &mut queue);
    }
    while let Some(name) = queue.pop_front() {
        if !reachable.insert(name.clone()) {
            continue;
        }
        if let Some(function) = functions_by_name.get(name.as_str()) {
            for stmt in &function.body {
                collect_stmt_calls(stmt, &mut queue);
            }
        }
    }
    reachable
}

fn collect_stmt_calls(stmt: &syntax::Stmt, calls: &mut VecDeque<String>) {
    match stmt {
        syntax::Stmt::Let { expr, .. }
        | syntax::Stmt::Print { expr, .. }
        | syntax::Stmt::Panic { expr, .. }
        | syntax::Stmt::Defer { expr, .. }
        | syntax::Stmt::Return { expr, .. } => collect_expr_calls(expr, calls),
        syntax::Stmt::Assign { target, expr, .. } => {
            collect_expr_calls(target, calls);
            collect_expr_calls(expr, calls);
        }
        syntax::Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            collect_expr_calls(cond, calls);
            for stmt in then_block {
                collect_stmt_calls(stmt, calls);
            }
            if let Some(else_block) = else_block {
                for stmt in else_block {
                    collect_stmt_calls(stmt, calls);
                }
            }
        }
        syntax::Stmt::IfLet {
            expr,
            then_block,
            else_block,
            ..
        } => {
            collect_expr_calls(expr, calls);
            for stmt in then_block {
                collect_stmt_calls(stmt, calls);
            }
            if let Some(else_block) = else_block {
                for stmt in else_block {
                    collect_stmt_calls(stmt, calls);
                }
            }
        }
        syntax::Stmt::While { cond, body, .. } => {
            collect_expr_calls(cond, calls);
            for stmt in body {
                collect_stmt_calls(stmt, calls);
            }
        }
        syntax::Stmt::Match { expr, arms, .. } => {
            collect_expr_calls(expr, calls);
            for arm in arms {
                for stmt in &arm.body {
                    collect_stmt_calls(stmt, calls);
                }
            }
        }
    }
}

fn collect_expr_calls(expr: &syntax::Expr, calls: &mut VecDeque<String>) {
    match expr {
        syntax::Expr::Literal(_) | syntax::Expr::VarRef { .. } => {}
        syntax::Expr::Call { name, args, .. } => {
            calls.push_back(name.clone());
            for arg in args {
                collect_expr_calls(arg, calls);
            }
        }
        syntax::Expr::MethodCall { base, args, .. } => {
            collect_expr_calls(base, calls);
            for arg in args {
                collect_expr_calls(arg, calls);
            }
        }
        syntax::Expr::BinaryAdd { lhs, rhs, .. }
        | syntax::Expr::BinaryCompare { lhs, rhs, .. }
        | syntax::Expr::BinaryLogic { lhs, rhs, .. } => {
            collect_expr_calls(lhs, calls);
            collect_expr_calls(rhs, calls);
        }
        syntax::Expr::Try { expr, .. }
        | syntax::Expr::Await { expr, .. }
        | syntax::Expr::Cast { expr, .. }
        | syntax::Expr::MutBorrow { expr, .. }
        | syntax::Expr::Deref { expr, .. } => {
            collect_expr_calls(expr, calls);
        }
        syntax::Expr::StructLiteral { fields, .. } => {
            for field in fields {
                collect_expr_calls(&field.expr, calls);
            }
        }
        syntax::Expr::FieldAccess { base, .. } | syntax::Expr::TupleIndex { base, .. } => {
            collect_expr_calls(base, calls);
        }
        syntax::Expr::TupleLiteral { elements, .. }
        | syntax::Expr::ArrayLiteral { elements, .. } => {
            for element in elements {
                collect_expr_calls(element, calls);
            }
        }
        syntax::Expr::MapLiteral { entries, .. } => {
            for entry in entries {
                collect_expr_calls(&entry.key, calls);
                collect_expr_calls(&entry.value, calls);
            }
        }
        syntax::Expr::Slice {
            base, start, end, ..
        } => {
            collect_expr_calls(base, calls);
            if let Some(start) = start {
                collect_expr_calls(start, calls);
            }
            if let Some(end) = end {
                collect_expr_calls(end, calls);
            }
        }
        syntax::Expr::Index { base, index, .. } => {
            collect_expr_calls(base, calls);
            collect_expr_calls(index, calls);
        }
        syntax::Expr::Closure { body, .. } => {
            collect_expr_calls(body, calls);
        }
        syntax::Expr::Match { expr, arms, .. } => {
            collect_expr_calls(expr, calls);
            for arm in arms {
                collect_expr_calls(&arm.expr, calls);
            }
        }
    }
}
