use crate::syntax;

impl syntax::Stmt {
    pub(super) fn line(&self) -> usize {
        match self {
            syntax::Stmt::Let { line, .. }
            | syntax::Stmt::Print { line, .. }
            | syntax::Stmt::Panic { line, .. }
            | syntax::Stmt::Defer { line, .. }
            | syntax::Stmt::If { line, .. }
            | syntax::Stmt::IfLet { line, .. }
            | syntax::Stmt::While { line, .. }
            | syntax::Stmt::Match { line, .. }
            | syntax::Stmt::Assign { line, .. }
            | syntax::Stmt::Return { line, .. } => *line,
        }
    }

    pub(super) fn column(&self) -> usize {
        match self {
            syntax::Stmt::Let { column, .. }
            | syntax::Stmt::Print { column, .. }
            | syntax::Stmt::Panic { column, .. }
            | syntax::Stmt::Defer { column, .. }
            | syntax::Stmt::If { column, .. }
            | syntax::Stmt::IfLet { column, .. }
            | syntax::Stmt::While { column, .. }
            | syntax::Stmt::Match { column, .. }
            | syntax::Stmt::Assign { column, .. }
            | syntax::Stmt::Return { column, .. } => *column,
        }
    }
}

impl syntax::Expr {
    pub(super) fn line(&self) -> usize {
        match self {
            syntax::Expr::Literal(_) => 1,
            syntax::Expr::VarRef { line, .. }
            | syntax::Expr::Call { line, .. }
            | syntax::Expr::MethodCall { line, .. }
            | syntax::Expr::BinaryAdd { line, .. }
            | syntax::Expr::BinaryCompare { line, .. }
            | syntax::Expr::BinaryLogic { line, .. }
            | syntax::Expr::Cast { line, .. }
            | syntax::Expr::Try { line, .. }
            | syntax::Expr::Await { line, .. }
            | syntax::Expr::StructLiteral { line, .. }
            | syntax::Expr::FieldAccess { line, .. }
            | syntax::Expr::TupleLiteral { line, .. }
            | syntax::Expr::TupleIndex { line, .. }
            | syntax::Expr::MapLiteral { line, .. }
            | syntax::Expr::ArrayLiteral { line, .. }
            | syntax::Expr::Slice { line, .. }
            | syntax::Expr::Index { line, .. }
            | syntax::Expr::MutBorrow { line, .. }
            | syntax::Expr::Deref { line, .. }
            | syntax::Expr::Closure { line, .. }
            | syntax::Expr::Match { line, .. } => *line,
        }
    }

    pub(super) fn column(&self) -> usize {
        match self {
            syntax::Expr::Literal(_) => 1,
            syntax::Expr::VarRef { column, .. }
            | syntax::Expr::Call { column, .. }
            | syntax::Expr::MethodCall { column, .. }
            | syntax::Expr::BinaryAdd { column, .. }
            | syntax::Expr::BinaryCompare { column, .. }
            | syntax::Expr::BinaryLogic { column, .. }
            | syntax::Expr::Cast { column, .. }
            | syntax::Expr::Try { column, .. }
            | syntax::Expr::Await { column, .. }
            | syntax::Expr::StructLiteral { column, .. }
            | syntax::Expr::FieldAccess { column, .. }
            | syntax::Expr::TupleLiteral { column, .. }
            | syntax::Expr::TupleIndex { column, .. }
            | syntax::Expr::MapLiteral { column, .. }
            | syntax::Expr::ArrayLiteral { column, .. }
            | syntax::Expr::Slice { column, .. }
            | syntax::Expr::Index { column, .. }
            | syntax::Expr::MutBorrow { column, .. }
            | syntax::Expr::Deref { column, .. }
            | syntax::Expr::Closure { column, .. }
            | syntax::Expr::Match { column, .. } => *column,
        }
    }
}
