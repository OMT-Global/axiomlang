use crate::syntax;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Program {
    pub path: String,
    pub structs: Vec<StructDef>,
    pub enums: Vec<EnumDef>,
    pub traits: Vec<TraitDef>,
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
pub struct TraitDef {
    pub name: String,
    pub methods: Vec<TraitMethodDef>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TraitMethodDef {
    pub name: String,
    pub params: Vec<syntax::TypeName>,
    pub return_ty: syntax::TypeName,
    pub has_self: bool,
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
    pub is_property: bool,
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
    pub end_line: usize,
    pub end_column: usize,
}

impl SourceSpan {
    pub const fn point(line: usize, column: usize) -> Self {
        Self {
            line,
            column,
            end_line: line,
            end_column: column + 1,
        }
    }

    pub const fn range(line: usize, column: usize, end_line: usize, end_column: usize) -> Self {
        Self {
            line,
            column,
            end_line,
            end_column,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum Stmt {
    Let {
        name: String,
        ty: Type,
        expr: Expr,
        #[serde(skip)]
        borrow_region_facts: Vec<BorrowRegionFact>,
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
        #[serde(skip)]
        borrow_region_facts: Vec<BorrowRegionFact>,
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
    #[serde(skip)]
    pub borrow_region_facts: Vec<BorrowRegionFact>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MapEntry {
    pub key: Expr,
    pub value: Expr,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum Expr {
    Literal {
        ty: Type,
        value: LiteralValue,
    },
    VarRef {
        name: String,
        ty: Type,
    },
    Call {
        name: String,
        args: Vec<Expr>,
        ty: Type,
        span: SourceSpan,
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
    BinaryLogic {
        op: LogicOp,
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
    Match {
        expr: Box<Expr>,
        arms: Vec<MatchExprArm>,
        ty: Type,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MatchExprArm {
    pub enum_name: String,
    pub variant: String,
    pub bindings: Vec<String>,
    pub is_named: bool,
    pub expr: Expr,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BorrowRegionFact {
    pub binding: String,
    pub origin: BorrowRegionOrigin,
    pub scope: BorrowRegionScope,
    pub source: BorrowRegionSource,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BorrowRegionOrigin {
    pub name: String,
    pub projection: Vec<BorrowRegionProjection>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum BorrowRegionProjection {
    Field(String),
    TupleIndex(usize),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum BorrowRegionScope {
    Binding(String),
    Return {
        function: String,
        projection: Vec<BorrowRegionProjection>,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum BorrowRegionSource {
    Direct,
    EnumPayload {
        enum_origin: BorrowRegionOrigin,
        variant: String,
        payload_index: usize,
    },
    AggregateReturn,
}

#[derive(Debug, Clone, Serialize, Eq)]
pub enum Type {
    Error,
    Never,
    Int,
    Numeric(syntax::NumericType),
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

impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Type::Error, Type::Error)
            | (Type::Never, Type::Never)
            | (Type::Int, Type::Int)
            | (Type::Bool, Type::Bool)
            | (Type::String, Type::String)
            | (Type::Str, Type::Str) => true,
            (Type::Numeric(lhs), Type::Numeric(rhs)) => lhs == rhs,
            (Type::Struct(lhs), Type::Struct(rhs)) | (Type::Enum(lhs), Type::Enum(rhs)) => {
                lhs == rhs
            }
            (Type::Ptr(lhs), Type::Ptr(rhs))
            | (Type::MutPtr(lhs), Type::MutPtr(rhs))
            | (Type::MutRef(lhs), Type::MutRef(rhs))
            | (Type::Slice(lhs), Type::Slice(rhs))
            | (Type::MutSlice(lhs), Type::MutSlice(rhs))
            | (Type::Option(lhs), Type::Option(rhs))
            | (Type::Task(lhs), Type::Task(rhs))
            | (Type::JoinHandle(lhs), Type::JoinHandle(rhs))
            | (Type::AsyncChannel(lhs), Type::AsyncChannel(rhs))
            | (Type::SelectResult(lhs), Type::SelectResult(rhs)) => lhs == rhs,
            (Type::Result(lhs_ok, lhs_err), Type::Result(rhs_ok, rhs_err))
            | (Type::Map(lhs_ok, lhs_err), Type::Map(rhs_ok, rhs_err)) => {
                lhs_ok == rhs_ok && lhs_err == rhs_err
            }
            (Type::Tuple(lhs), Type::Tuple(rhs)) => lhs == rhs,
            (Type::Array(lhs_inner, lhs_len), Type::Array(rhs_inner, rhs_len)) => {
                lhs_inner == rhs_inner && lhs_len == rhs_len
            }
            (Type::Fn(lhs_params, lhs_return), Type::Fn(rhs_params, rhs_return)) => {
                lhs_params == rhs_params && lhs_return == rhs_return
            }
            _ => false,
        }
    }
}

pub(super) fn type_assignable_to(actual: &Type, expected: &Type) -> bool {
    match (actual, expected) {
        (Type::Never, _) | (_, Type::Error) | (Type::Error, _) => true,
        (Type::Array(actual_inner, actual_len), Type::Array(expected_inner, expected_len)) => {
            type_assignable_to(actual_inner, expected_inner)
                && match expected_len {
                    Some(len) => actual_len == &Some(*len),
                    None => true,
                }
        }
        (Type::Ptr(actual_inner), Type::Ptr(expected_inner))
        | (Type::MutPtr(actual_inner), Type::MutPtr(expected_inner))
        | (Type::MutRef(actual_inner), Type::MutRef(expected_inner))
        | (Type::Slice(actual_inner), Type::Slice(expected_inner))
        | (Type::MutSlice(actual_inner), Type::MutSlice(expected_inner))
        | (Type::Option(actual_inner), Type::Option(expected_inner))
        | (Type::Task(actual_inner), Type::Task(expected_inner))
        | (Type::JoinHandle(actual_inner), Type::JoinHandle(expected_inner))
        | (Type::AsyncChannel(actual_inner), Type::AsyncChannel(expected_inner))
        | (Type::SelectResult(actual_inner), Type::SelectResult(expected_inner)) => {
            type_assignable_to(actual_inner, expected_inner)
        }
        (Type::Result(actual_ok, actual_err), Type::Result(expected_ok, expected_err))
        | (Type::Map(actual_ok, actual_err), Type::Map(expected_ok, expected_err)) => {
            type_assignable_to(actual_ok, expected_ok)
                && type_assignable_to(actual_err, expected_err)
        }
        (Type::Tuple(actual), Type::Tuple(expected)) => {
            actual.len() == expected.len()
                && actual
                    .iter()
                    .zip(expected.iter())
                    .all(|(actual, expected)| type_assignable_to(actual, expected))
        }
        (Type::Fn(actual_params, actual_return), Type::Fn(expected_params, expected_return)) => {
            actual_params.len() == expected_params.len()
                && actual_params
                    .iter()
                    .zip(expected_params.iter())
                    .all(|(actual, expected)| type_assignable_to(actual, expected))
                && type_assignable_to(actual_return, expected_return)
        }
        _ => unify_types(actual, expected).is_some_and(|ty| ty == *expected),
    }
}

pub(crate) fn unify_types(left: &Type, right: &Type) -> Option<Type> {
    match (left, right) {
        (Type::Never, Type::Never) => Some(Type::Never),
        (Type::Never, other) | (other, Type::Never) => Some(other.clone()),
        (Type::Error, other) | (other, Type::Error) => Some(other.clone()),
        _ if left == right => Some(left.clone()),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum LiteralValue {
    Int(i64),
    Numeric {
        raw: String,
        ty: syntax::NumericType,
    },
    Bool(bool),
    String(String),
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
pub enum LogicOp {
    And,
    Or,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum ArithmeticOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StructFieldValue {
    pub name: String,
    pub expr: Expr,
}

impl Expr {
    pub fn ty(&self) -> &Type {
        match self {
            Expr::Literal { ty, .. } => ty,
            Expr::VarRef { ty, .. } => ty,
            Expr::Call { ty, .. } => ty,
            Expr::BinaryAdd { ty, .. } => ty,
            Expr::BinaryCompare { ty, .. } => ty,
            Expr::BinaryLogic { ty, .. } => ty,
            Expr::Cast { ty, .. } => ty,
            Expr::MutBorrow { ty, .. } => ty,
            Expr::Deref { ty, .. } => ty,
            Expr::Try { ty, .. } => ty,
            Expr::Await { ty, .. } => ty,
            Expr::StructLiteral { ty, .. } => ty,
            Expr::FieldAccess { ty, .. } => ty,
            Expr::TupleLiteral { ty, .. } => ty,
            Expr::TupleIndex { ty, .. } => ty,
            Expr::MapLiteral { ty, .. } => ty,
            Expr::EnumVariant { ty, .. } => ty,
            Expr::ArrayLiteral { ty, .. } => ty,
            Expr::Closure { ty, .. } => ty,
            Expr::Slice { ty, .. } => ty,
            Expr::Index { ty, .. } => ty,
            Expr::StringBorrow { ty, .. } => ty,
            Expr::Match { ty, .. } => ty,
        }
    }
}

impl CompareOp {
    pub fn lexeme(self) -> &'static str {
        match self {
            CompareOp::Eq => "==",
            CompareOp::Ne => "!=",
            CompareOp::Lt => "<",
            CompareOp::Le => "<=",
            CompareOp::Gt => ">",
            CompareOp::Ge => ">=",
        }
    }
}

impl LogicOp {
    pub fn lexeme(self) -> &'static str {
        match self {
            LogicOp::And => "&&",
            LogicOp::Or => "||",
        }
    }
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Error => write!(f, "<type-error>"),
            Type::Never => write!(f, "!"),
            Type::Int => write!(f, "int"),
            Type::Numeric(numeric) => write!(f, "{}", numeric.as_str()),
            Type::Bool => write!(f, "bool"),
            Type::String => write!(f, "string"),
            Type::Str => write!(f, "&str"),
            Type::Struct(name) => write!(f, "{name}"),
            Type::Enum(name) => write!(f, "{name}"),
            Type::Ptr(inner) => write!(f, "ptr<{inner}>"),
            Type::MutPtr(inner) => write!(f, "mutptr<{inner}>"),
            Type::MutRef(inner) => write!(f, "&mut {inner}"),
            Type::Slice(inner) => write!(f, "&[{inner}]"),
            Type::MutSlice(inner) => write!(f, "&mut [{inner}]"),
            Type::Option(inner) => write!(f, "Option<{inner}>"),
            Type::Result(ok, err) => write!(f, "Result<{ok}, {err}>"),
            Type::Tuple(elements) => write!(
                f,
                "({})",
                elements
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Type::Map(key, value) => write!(f, "{{{key}: {value}}}"),
            Type::Array(inner, Some(len)) => write!(f, "[{inner}; {len}]"),
            Type::Array(inner, None) => write!(f, "[{inner}]"),
            Type::Task(inner) => write!(f, "Task<{inner}>"),
            Type::JoinHandle(inner) => write!(f, "JoinHandle<{inner}>"),
            Type::AsyncChannel(inner) => write!(f, "AsyncChannel<{inner}>"),
            Type::SelectResult(inner) => write!(f, "SelectResult<{inner}>"),
            Type::Fn(params, return_ty) => write!(
                f,
                "fn({}): {return_ty}",
                params
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}
