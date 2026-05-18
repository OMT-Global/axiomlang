use crate::hir;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Program {
    pub path: String,
    pub structs: Vec<StructDef>,
    pub enums: Vec<EnumDef>,
    pub statics: Vec<StaticDef>,
    pub functions: Vec<Function>,
    pub stmts: Vec<Stmt>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<StructField>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StructField {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<EnumVariantDef>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EnumVariantDef {
    pub name: String,
    pub payload_tys: Vec<Type>,
    pub payload_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StaticDef {
    pub name: String,
    pub ty: Type,
    pub expr: Expr,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub source_name: String,
    pub path: String,
    pub params: Vec<Param>,
    pub return_ty: Type,
    pub body: Vec<Stmt>,
    pub is_async: bool,
    pub is_extern: bool,
    pub extern_abi: Option<String>,
    pub extern_library: Option<String>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct SourceSpan {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum Stmt {
    Let {
        name: String,
        ty: Type,
        expr: Expr,
        span: SourceSpan,
    },
    Assign {
        target: Expr,
        expr: Expr,
        span: SourceSpan,
    },
    Print {
        expr: Expr,
        span: SourceSpan,
    },
    Panic {
        message: Expr,
        span: SourceSpan,
    },
    Defer {
        expr: Expr,
        span: SourceSpan,
    },
    If {
        cond: Expr,
        then_block: Vec<Stmt>,
        else_block: Option<Vec<Stmt>>,
        span: SourceSpan,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
        span: SourceSpan,
    },
    Match {
        expr: Expr,
        arms: Vec<MatchArm>,
        span: SourceSpan,
    },
    Return {
        expr: Expr,
        span: SourceSpan,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MatchArm {
    pub enum_name: String,
    pub variant: String,
    pub bindings: Vec<String>,
    pub is_named: bool,
    pub ignore_payloads: bool,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MapEntry {
    pub key: Expr,
    pub value: Expr,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum Expr {
    Literal(LiteralValue),
    VarRef {
        name: String,
        ty: Type,
    },
    Call {
        name: String,
        args: Vec<Expr>,
        ty: Type,
    },
    BinaryAdd {
        op: ArithmeticOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        ty: Type,
    },
    BinaryCompare {
        op: CompareOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        ty: Type,
    },
    Cast {
        expr: Box<Expr>,
        ty: Type,
    },
    MutBorrow {
        expr: Box<Expr>,
        ty: Type,
    },
    Deref {
        expr: Box<Expr>,
        ty: Type,
    },
    Try {
        expr: Box<Expr>,
        ty: Type,
    },
    Await {
        expr: Box<Expr>,
        ty: Type,
    },
    StructLiteral {
        name: String,
        fields: Vec<StructFieldValue>,
        ty: Type,
    },
    FieldAccess {
        base: Box<Expr>,
        field: String,
        ty: Type,
    },
    TupleLiteral {
        elements: Vec<Expr>,
        ty: Type,
    },
    TupleIndex {
        base: Box<Expr>,
        index: usize,
        ty: Type,
    },
    MapLiteral {
        entries: Vec<MapEntry>,
        ty: Type,
    },
    EnumVariant {
        enum_name: String,
        variant: String,
        field_names: Vec<String>,
        payloads: Vec<Expr>,
        ty: Type,
    },
    ArrayLiteral {
        elements: Vec<Expr>,
        ty: Type,
    },
    Closure {
        params: Vec<Param>,
        body: Box<Expr>,
        ty: Type,
    },
    Slice {
        base: Box<Expr>,
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        ty: Type,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        ty: Type,
    },
    StringBorrow {
        expr: Box<Expr>,
        ty: Type,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum LiteralValue {
    Int(i64),
    Numeric {
        raw: String,
        ty: crate::syntax::NumericType,
    },
    Bool(bool),
    String(String),
    Str(String),
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum ArithmeticOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum Type {
    Int,
    Numeric(crate::syntax::NumericType),
    Bool,
    String,
    Str,
    Struct(String),
    Enum(String),
    Ptr(Box<Type>),
    MutPtr(Box<Type>),
    MutRef(Box<Type>),
    Slice(Box<Type>),
    MutSlice(Box<Type>),
    Option(Box<Type>),
    Result(Box<Type>, Box<Type>),
    Tuple(Vec<Type>),
    Map(Box<Type>, Box<Type>),
    Array(Box<Type>, Option<usize>),
    Task(Box<Type>),
    JoinHandle(Box<Type>),
    AsyncChannel(Box<Type>),
    SelectResult(Box<Type>),
    Fn(Vec<Type>, Box<Type>),
}

impl Type {
    pub fn is_copy(&self) -> bool {
        match self {
            Type::Int
            | Type::Numeric(_)
            | Type::Bool
            | Type::Str
            | Type::Ptr(_)
            | Type::MutPtr(_)
            | Type::Slice(_) => true,
            Type::MutRef(_) | Type::MutSlice(_) => false,
            Type::Option(inner) => inner.is_copy(),
            Type::Result(ok, err) => ok.is_copy() && err.is_copy(),
            Type::Tuple(elements) => elements.iter().all(Type::is_copy),
            Type::String
            | Type::Struct(_)
            | Type::Enum(_)
            | Type::Map(_, _)
            | Type::Array(_, _)
            | Type::Task(_)
            | Type::JoinHandle(_)
            | Type::AsyncChannel(_)
            | Type::SelectResult(_)
            | Type::Fn(_, _) => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StructFieldValue {
    pub name: String,
    pub expr: Expr,
}

pub fn lower(program: &hir::Program) -> Program {
    Program {
        path: program.path.clone(),
        structs: program.structs.iter().map(lower_struct).collect(),
        enums: program.enums.iter().map(lower_enum).collect(),
        statics: program.statics.iter().map(lower_static).collect(),
        functions: program.functions.iter().map(lower_function).collect(),
        stmts: program.stmts.iter().map(lower_stmt).collect(),
    }
}

fn lower_static(static_def: &hir::StaticDef) -> StaticDef {
    StaticDef {
        name: static_def.name.clone(),
        ty: lower_type(&static_def.ty),
        expr: lower_expr(&static_def.expr),
    }
}

impl Program {
    pub fn statement_count(&self) -> usize {
        self.functions
            .iter()
            .map(|function| function.body.iter().map(count_stmt).sum::<usize>())
            .sum::<usize>()
            + self.stmts.iter().map(count_stmt).sum::<usize>()
    }
}

impl Expr {
    pub fn ty(&self) -> Type {
        match self {
            Expr::Literal(LiteralValue::Int(_)) => Type::Int,
            Expr::Literal(LiteralValue::Numeric { ty, .. }) => Type::Numeric(*ty),
            Expr::Literal(LiteralValue::Bool(_)) => Type::Bool,
            Expr::Literal(LiteralValue::String(_)) => Type::String,
            Expr::Literal(LiteralValue::Str(_)) => Type::Str,
            Expr::VarRef { ty, .. } => ty.clone(),
            Expr::Call { ty, .. } => ty.clone(),
            Expr::BinaryAdd { ty, .. } => ty.clone(),
            Expr::BinaryCompare { ty, .. } => ty.clone(),
            Expr::Cast { ty, .. } => ty.clone(),
            Expr::Try { ty, .. } => ty.clone(),
            Expr::Await { ty, .. } => ty.clone(),
            Expr::StructLiteral { ty, .. } => ty.clone(),
            Expr::FieldAccess { ty, .. } => ty.clone(),
            Expr::TupleLiteral { ty, .. } => ty.clone(),
            Expr::TupleIndex { ty, .. } => ty.clone(),
            Expr::MapLiteral { ty, .. } => ty.clone(),
            Expr::EnumVariant { ty, .. } => ty.clone(),
            Expr::ArrayLiteral { ty, .. } => ty.clone(),
            Expr::Closure { ty, .. } => ty.clone(),
            Expr::Slice { ty, .. } => ty.clone(),
            Expr::Index { ty, .. } => ty.clone(),
            Expr::StringBorrow { ty, .. } => ty.clone(),
            Expr::MutBorrow { ty, .. } => ty.clone(),
            Expr::Deref { ty, .. } => ty.clone(),
        }
    }
}

fn count_stmt(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::Let { .. }
        | Stmt::Assign { .. }
        | Stmt::Print { .. }
        | Stmt::Panic { .. }
        | Stmt::Defer { .. }
        | Stmt::Return { .. } => 1,
        Stmt::If {
            then_block,
            else_block,
            ..
        } => {
            1 + then_block.iter().map(count_stmt).sum::<usize>()
                + else_block
                    .as_ref()
                    .map(|block| block.iter().map(count_stmt).sum::<usize>())
                    .unwrap_or(0)
        }
        Stmt::While { body, .. } => 1 + body.iter().map(count_stmt).sum::<usize>(),
        Stmt::Match { arms, .. } => {
            1 + arms
                .iter()
                .map(|arm| arm.body.iter().map(count_stmt).sum::<usize>())
                .sum::<usize>()
        }
    }
}

fn lower_function(function: &hir::Function) -> Function {
    Function {
        name: function.name.clone(),
        source_name: function.source_name.clone(),
        path: function.path.clone(),
        params: function.params.iter().map(lower_param).collect(),
        return_ty: lower_type(&function.return_ty),
        body: function.body.iter().map(lower_stmt).collect(),
        is_async: function.is_async,
        is_extern: function.is_extern,
        extern_abi: function.extern_abi.clone(),
        extern_library: function.extern_library.clone(),
        line: function.line,
        column: function.column,
    }
}

fn lower_struct(struct_def: &hir::StructDef) -> StructDef {
    StructDef {
        name: struct_def.name.clone(),
        fields: struct_def.fields.iter().map(lower_struct_field).collect(),
    }
}

fn lower_enum(enum_def: &hir::EnumDef) -> EnumDef {
    EnumDef {
        name: enum_def.name.clone(),
        variants: enum_def.variants.iter().map(lower_enum_variant).collect(),
    }
}

fn lower_enum_variant(variant: &hir::EnumVariantDef) -> EnumVariantDef {
    EnumVariantDef {
        name: variant.name.clone(),
        payload_tys: variant.payload_tys.iter().map(lower_type).collect(),
        payload_names: variant.payload_names.clone(),
    }
}

fn lower_struct_field(field: &hir::StructField) -> StructField {
    StructField {
        name: field.name.clone(),
        ty: lower_type(&field.ty),
    }
}

fn lower_param(param: &hir::Param) -> Param {
    Param {
        name: param.name.clone(),
        ty: lower_type(&param.ty),
    }
}

fn lower_stmt(stmt: &hir::Stmt) -> Stmt {
    match stmt {
        hir::Stmt::Let {
            name,
            ty,
            expr,
            span,
        } => Stmt::Let {
            name: name.clone(),
            ty: lower_type(ty),
            expr: lower_expr(expr),
            span: lower_source_span(span),
        },
        hir::Stmt::Assign { target, expr, span } => Stmt::Assign {
            target: lower_expr(target),
            expr: lower_expr(expr),
            span: lower_source_span(span),
        },
        hir::Stmt::Print { expr, span } => Stmt::Print {
            expr: lower_expr(expr),
            span: lower_source_span(span),
        },
        hir::Stmt::Panic { message, span } => Stmt::Panic {
            message: lower_expr(message),
            span: lower_source_span(span),
        },
        hir::Stmt::Defer { expr, span } => Stmt::Defer {
            expr: lower_expr(expr),
            span: lower_source_span(span),
        },
        hir::Stmt::If {
            cond,
            then_block,
            else_block,
            span,
        } => Stmt::If {
            cond: lower_expr(cond),
            then_block: then_block.iter().map(lower_stmt).collect(),
            else_block: else_block
                .as_ref()
                .map(|block| block.iter().map(lower_stmt).collect()),
            span: lower_source_span(span),
        },
        hir::Stmt::While { cond, body, span } => Stmt::While {
            cond: lower_expr(cond),
            body: body.iter().map(lower_stmt).collect(),
            span: lower_source_span(span),
        },
        hir::Stmt::Match { expr, arms, span } => Stmt::Match {
            expr: lower_expr(expr),
            arms: arms
                .iter()
                .map(|arm| MatchArm {
                    enum_name: arm.enum_name.clone(),
                    variant: arm.variant.clone(),
                    bindings: arm.bindings.clone(),
                    is_named: arm.is_named,
                    ignore_payloads: arm.ignore_payloads,
                    body: arm.body.iter().map(lower_stmt).collect(),
                })
                .collect(),
            span: lower_source_span(span),
        },
        hir::Stmt::Return { expr, span } => Stmt::Return {
            expr: lower_expr(expr),
            span: lower_source_span(span),
        },
    }
}

fn lower_source_span(span: &hir::SourceSpan) -> SourceSpan {
    SourceSpan {
        line: span.line,
        column: span.column,
    }
}

fn lower_expr(expr: &hir::Expr) -> Expr {
    match expr {
        hir::Expr::Literal { ty, value } => Expr::Literal(match value {
            hir::LiteralValue::Int(value) => LiteralValue::Int(*value),
            hir::LiteralValue::Numeric { raw, ty } => LiteralValue::Numeric {
                raw: raw.clone(),
                ty: *ty,
            },
            hir::LiteralValue::Bool(value) => LiteralValue::Bool(*value),
            hir::LiteralValue::String(value) => {
                if matches!(ty, hir::Type::Str) {
                    LiteralValue::Str(value.clone())
                } else {
                    LiteralValue::String(value.clone())
                }
            }
        }),
        hir::Expr::VarRef { name, ty } => Expr::VarRef {
            name: name.clone(),
            ty: lower_type(ty),
        },
        hir::Expr::Call { name, args, ty, .. } => Expr::Call {
            name: name.clone(),
            args: args.iter().map(lower_expr).collect(),
            ty: lower_type(ty),
        },
        hir::Expr::BinaryAdd { op, lhs, rhs, ty } => Expr::BinaryAdd {
            op: lower_arithmetic_op(*op),
            lhs: Box::new(lower_expr(lhs)),
            rhs: Box::new(lower_expr(rhs)),
            ty: lower_type(ty),
        },
        hir::Expr::BinaryCompare { op, lhs, rhs, ty } => Expr::BinaryCompare {
            op: lower_compare_op(*op),
            lhs: Box::new(lower_expr(lhs)),
            rhs: Box::new(lower_expr(rhs)),
            ty: lower_type(ty),
        },
        hir::Expr::Cast { expr, ty } => Expr::Cast {
            expr: Box::new(lower_expr(expr)),
            ty: lower_type(ty),
        },
        hir::Expr::MutBorrow { expr, ty } => Expr::MutBorrow {
            expr: Box::new(lower_expr(expr)),
            ty: lower_type(ty),
        },
        hir::Expr::Deref { expr, ty } => Expr::Deref {
            expr: Box::new(lower_expr(expr)),
            ty: lower_type(ty),
        },
        hir::Expr::Try { expr, ty } => Expr::Try {
            expr: Box::new(lower_expr(expr)),
            ty: lower_type(ty),
        },
        hir::Expr::Await { expr, ty } => Expr::Await {
            expr: Box::new(lower_expr(expr)),
            ty: lower_type(ty),
        },
        hir::Expr::StructLiteral { name, fields, ty } => Expr::StructLiteral {
            name: name.clone(),
            fields: fields
                .iter()
                .map(|field| StructFieldValue {
                    name: field.name.clone(),
                    expr: lower_expr(&field.expr),
                })
                .collect(),
            ty: lower_type(ty),
        },
        hir::Expr::FieldAccess { base, field, ty } => Expr::FieldAccess {
            base: Box::new(lower_expr(base)),
            field: field.clone(),
            ty: lower_type(ty),
        },
        hir::Expr::TupleLiteral { elements, ty } => Expr::TupleLiteral {
            elements: elements.iter().map(lower_expr).collect(),
            ty: lower_type(ty),
        },
        hir::Expr::TupleIndex { base, index, ty } => Expr::TupleIndex {
            base: Box::new(lower_expr(base)),
            index: *index,
            ty: lower_type(ty),
        },
        hir::Expr::MapLiteral { entries, ty } => Expr::MapLiteral {
            entries: entries
                .iter()
                .map(|entry| MapEntry {
                    key: lower_expr(&entry.key),
                    value: lower_expr(&entry.value),
                })
                .collect(),
            ty: lower_type(ty),
        },
        hir::Expr::EnumVariant {
            enum_name,
            variant,
            field_names,
            payloads,
            ty,
        } => Expr::EnumVariant {
            enum_name: enum_name.clone(),
            variant: variant.clone(),
            field_names: field_names.clone(),
            payloads: payloads.iter().map(lower_expr).collect(),
            ty: lower_type(ty),
        },
        hir::Expr::ArrayLiteral { elements, ty } => Expr::ArrayLiteral {
            elements: elements.iter().map(lower_expr).collect(),
            ty: lower_type(ty),
        },
        hir::Expr::Closure { params, body, ty } => Expr::Closure {
            params: params.iter().map(lower_param).collect(),
            body: Box::new(lower_expr(body)),
            ty: lower_type(ty),
        },
        hir::Expr::Slice {
            base,
            start,
            end,
            ty,
        } => Expr::Slice {
            base: Box::new(lower_expr(base)),
            start: start.as_ref().map(|expr| Box::new(lower_expr(expr))),
            end: end.as_ref().map(|expr| Box::new(lower_expr(expr))),
            ty: lower_type(ty),
        },
        hir::Expr::Index { base, index, ty } => Expr::Index {
            base: Box::new(lower_expr(base)),
            index: Box::new(lower_expr(index)),
            ty: lower_type(ty),
        },
        hir::Expr::StringBorrow { expr, ty } => Expr::StringBorrow {
            expr: Box::new(lower_expr(expr)),
            ty: lower_type(ty),
        },
    }
}

fn lower_type(ty: &hir::Type) -> Type {
    match ty {
        hir::Type::Error => unreachable!("type-error sentinel must not reach MIR lowering"),
        hir::Type::Int => Type::Int,
        hir::Type::Numeric(numeric) => Type::Numeric(*numeric),
        hir::Type::Bool => Type::Bool,
        hir::Type::String => Type::String,
        hir::Type::Str => Type::Str,
        hir::Type::Struct(name) => Type::Struct(name.clone()),
        hir::Type::Enum(name) => Type::Enum(name.clone()),
        hir::Type::Ptr(inner) => Type::Ptr(Box::new(lower_type(inner))),
        hir::Type::MutPtr(inner) => Type::MutPtr(Box::new(lower_type(inner))),
        hir::Type::MutRef(inner) => Type::MutRef(Box::new(lower_type(inner))),
        hir::Type::Slice(inner) => Type::Slice(Box::new(lower_type(inner))),
        hir::Type::MutSlice(inner) => Type::MutSlice(Box::new(lower_type(inner))),
        hir::Type::Option(inner) => Type::Option(Box::new(lower_type(inner))),
        hir::Type::Result(ok, err) => {
            Type::Result(Box::new(lower_type(ok)), Box::new(lower_type(err)))
        }
        hir::Type::Tuple(elements) => Type::Tuple(elements.iter().map(lower_type).collect()),
        hir::Type::Map(key, value) => {
            Type::Map(Box::new(lower_type(key)), Box::new(lower_type(value)))
        }
        hir::Type::Array(inner, len) => Type::Array(Box::new(lower_type(inner)), *len),
        hir::Type::Task(inner) => Type::Task(Box::new(lower_type(inner))),
        hir::Type::JoinHandle(inner) => Type::JoinHandle(Box::new(lower_type(inner))),
        hir::Type::AsyncChannel(inner) => Type::AsyncChannel(Box::new(lower_type(inner))),
        hir::Type::SelectResult(inner) => Type::SelectResult(Box::new(lower_type(inner))),
        hir::Type::Fn(params, return_ty) => Type::Fn(
            params.iter().map(lower_type).collect(),
            Box::new(lower_type(return_ty)),
        ),
    }
}

fn lower_compare_op(op: hir::CompareOp) -> CompareOp {
    match op {
        hir::CompareOp::Eq => CompareOp::Eq,
        hir::CompareOp::Ne => CompareOp::Ne,
        hir::CompareOp::Lt => CompareOp::Lt,
        hir::CompareOp::Le => CompareOp::Le,
        hir::CompareOp::Gt => CompareOp::Gt,
        hir::CompareOp::Ge => CompareOp::Ge,
    }
}

fn lower_arithmetic_op(op: hir::ArithmeticOp) -> ArithmeticOp {
    match op {
        hir::ArithmeticOp::Add => ArithmeticOp::Add,
        hir::ArithmeticOp::Sub => ArithmeticOp::Sub,
        hir::ArithmeticOp::Mul => ArithmeticOp::Mul,
        hir::ArithmeticOp::Div => ArithmeticOp::Div,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir;
    use crate::syntax;
    use serde_json::json;
    use std::path::Path;

    fn lower_source(source: &str) -> Program {
        let parsed =
            syntax::parse_program(source, Path::new("main.ax")).expect("parse MIR fixture");
        let hir = hir::lower(&parsed).expect("lower HIR fixture");
        lower(&hir)
    }

    #[test]
    fn mir_contract_covers_locals_moves_borrows_calls_branches_match_and_return() {
        let program = lower_source(
            r#"
struct Pair {
name: string
values: [int]
}

enum Choice {
Picked(int)
Skipped
}

fn tail(values: &[int]): &[int] {
return values[1:]
}

fn choose(flag: bool): Choice {
if flag {
return Picked(7)
}
return Skipped
}

let pair: Pair = Pair { name: "alpha", values: [1, 2, 3] }
let moved_values: [int] = pair.values
let borrowed: &[int] = tail(moved_values[:])
match choose(true) {
Picked(value) {
print value
}
Skipped {
print len(borrowed)
}
}
print pair.name
"#,
        );

        assert_eq!(
            program.structs[0].fields[1].ty,
            Type::Array(Box::new(Type::Int), None)
        );
        assert_eq!(program.enums[0].variants[0].payload_tys, vec![Type::Int]);

        let tail_fn = program
            .functions
            .iter()
            .find(|function| function.source_name == "tail")
            .expect("tail function lowered");
        assert_eq!(tail_fn.params[0].ty, Type::Slice(Box::new(Type::Int)));
        assert_eq!(tail_fn.return_ty, Type::Slice(Box::new(Type::Int)));
        assert!(matches!(
            tail_fn.body.first(),
            Some(Stmt::Return {
                expr: Expr::Slice { .. },
                ..
            })
        ));

        let choose_fn = program
            .functions
            .iter()
            .find(|function| function.source_name == "choose")
            .expect("choose function lowered");
        assert!(matches!(choose_fn.body.first(), Some(Stmt::If { .. })));
        assert!(matches!(
            choose_fn.body.last(),
            Some(Stmt::Return {
                expr: Expr::EnumVariant { .. },
                ..
            })
        ));

        assert!(
            matches!(program.stmts[0], Stmt::Let { ref name, ty: Type::Struct(ref struct_name), expr: Expr::StructLiteral { .. }, .. } if name == "pair" && struct_name == "Pair")
        );
        assert!(
            matches!(program.stmts[1], Stmt::Let { ref name, ty: Type::Array(_, None), expr: Expr::FieldAccess { .. }, .. } if name == "moved_values")
        );
        assert!(
            matches!(program.stmts[2], Stmt::Let { ref name, ty: Type::Slice(_), expr: Expr::Call { .. }, .. } if name == "borrowed")
        );
        assert!(
            matches!(program.stmts[3], Stmt::Match { ref expr, ref arms, .. } if matches!(expr, Expr::Call { name, ty: Type::Enum(enum_name), .. } if name.contains("choose") && enum_name == "Choice") && arms.len() == 2)
        );
    }

    #[test]
    fn mir_contract_serializes_stable_typed_shapes() {
        let program = lower_source(
            r#"
enum Choice {
Picked(int)
Skipped
}

fn choose(flag: bool): Choice {
if flag {
return Picked(7)
}
return Skipped
}

match choose(true) {
Picked(value) {
print value
}
Skipped {
print 0
}
}
"#,
        );
        let value = serde_json::to_value(&program).expect("serialize MIR");
        assert_eq!(
            value["enums"][0]["variants"][0]["payload_tys"],
            json!(["Int"])
        );
        assert_eq!(
            value["functions"][0]["return_ty"],
            json!({ "Enum": "Choice" })
        );
        assert_eq!(
            value["functions"][0]["body"][0]["If"]["cond"]["VarRef"]["ty"],
            json!("Bool")
        );
        assert_eq!(
            value["stmts"][0]["Match"]["expr"]["Call"]["ty"],
            json!({ "Enum": "Choice" })
        );
        assert_eq!(
            value["stmts"][0]["Match"]["arms"][0]["bindings"],
            json!(["value"])
        );
    }
}
