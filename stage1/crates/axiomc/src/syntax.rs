use crate::diagnostics::Diagnostic;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
pub struct AstNodeId(pub String);

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    pub file: String,
    pub line: usize,
    pub column: usize,
}

impl SourceSpan {
    pub fn new(file: impl Into<String>, line: usize, column: usize) -> Self {
        Self {
            file: file.into(),
            line,
            column,
        }
    }

    pub fn stable_id(&self, kind: &str, name: Option<&str>) -> AstNodeId {
        let suffix = name.unwrap_or("");
        AstNodeId(format!(
            "{}:{}:{}:{}:{}",
            self.file, self.line, self.column, kind, suffix
        ))
    }
}

/// Parser-owned AST for a single stage1 source file.
///
/// This layer records syntax only: imports are raw module references, names are
/// raw source spellings, and type annotations are `TypeName` syntax trees.
/// Visibility, duplicate-symbol checks, imported symbol availability, concrete
/// type resolution, and ownership/borrow validation are intentionally deferred
/// to project flattening and HIR lowering.

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Program {
    pub path: String,
    pub imports: Vec<Import>,
    pub macros: Vec<MacroDecl>,
    pub macro_expansions: Vec<MacroExpansion>,
    pub axioms: Vec<AxiomDecl>,
    pub semantic_capabilities: Vec<SemanticCapabilityDecl>,
    pub evidence: Vec<EvidenceDecl>,
    pub consts: Vec<ConstDecl>,
    pub type_aliases: Vec<TypeAliasDecl>,
    pub structs: Vec<StructDecl>,
    pub enums: Vec<EnumDecl>,
    pub traits: Vec<TraitDecl>,
    pub functions: Vec<Function>,
    pub stmts: Vec<Stmt>,
}

pub const DEFAULT_MACRO_RECURSION_LIMIT: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseOptions {
    pub macro_recursion_limit: usize,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            macro_recursion_limit: DEFAULT_MACRO_RECURSION_LIMIT,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum ReceiverKind {
    Value,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Module,
    Package,
    Public,
}

impl Visibility {
    pub fn is_public(self) -> bool {
        matches!(self, Self::Public)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Import {
    pub path: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MacroDecl {
    pub name: String,
    pub style: MacroStyle,
    pub params: Vec<String>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MacroStyle {
    Macro,
    MacroRules,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MacroExpansion {
    pub macro_name: String,
    pub call_site: MacroSpan,
    pub definition_site: MacroSpan,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MacroSpan {
    pub path: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AxiomDecl {
    pub name: String,
    pub scope: Option<String>,
    pub severity: Option<String>,
    pub description: Option<String>,
    pub assertion: Option<String>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SemanticCapabilityDecl {
    pub name: String,
    pub inputs: Vec<CapabilityInput>,
    pub effects: Vec<CapabilityEffect>,
    pub preserves: Vec<SemanticReference>,
    pub evidence: Vec<SemanticReference>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CapabilityInput {
    pub name: String,
    pub ty: TypeName,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CapabilityEffect {
    pub kind: String,
    pub target: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SemanticReference {
    pub name: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EvidenceDecl {
    pub name: String,
    pub description: Option<String>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub source_name: String,
    pub path: String,
    pub type_params: Vec<String>,
    pub params: Vec<Param>,
    pub return_ty: TypeName,
    pub body: Vec<Stmt>,
    pub is_const: bool,
    pub is_async: bool,
    pub is_extern: bool,
    pub extern_abi: Option<String>,
    pub extern_library: Option<String>,
    pub visibility: Visibility,
    pub receiver: Option<ReceiverKind>,
    pub impl_target: Option<String>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ConstDecl {
    pub name: String,
    pub ty: TypeName,
    pub expr: Expr,
    pub is_static: bool,
    pub visibility: Visibility,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TypeAliasDecl {
    pub name: String,
    pub ty: TypeName,
    pub visibility: Visibility,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StructDecl {
    pub name: String,
    pub type_params: Vec<String>,
    pub fields: Vec<StructField>,
    pub visibility: Visibility,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StructField {
    pub name: String,
    pub ty: TypeName,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EnumDecl {
    pub name: String,
    pub type_params: Vec<String>,
    pub variants: Vec<EnumVariantDecl>,
    pub visibility: Visibility,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EnumVariantDecl {
    pub name: String,
    pub payload_tys: Vec<TypeName>,
    pub payload_names: Vec<String>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TraitDecl {
    pub name: String,
    pub methods: Vec<TraitMethodDecl>,
    pub visibility: Visibility,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TraitMethodDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_ty: TypeName,
    pub has_self: bool,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub ty: TypeName,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum Stmt {
    Let {
        name: String,
        ty: TypeName,
        expr: Expr,
        line: usize,
        column: usize,
    },
    Assign {
        target: Expr,
        expr: Expr,
        line: usize,
        column: usize,
    },
    Print {
        expr: Expr,
        line: usize,
        column: usize,
    },
    Panic {
        expr: Expr,
        line: usize,
        column: usize,
    },
    Defer {
        expr: Expr,
        line: usize,
        column: usize,
    },
    If {
        cond: Expr,
        then_block: Vec<Stmt>,
        else_block: Option<Vec<Stmt>>,
        line: usize,
        column: usize,
    },
    IfLet {
        variant: String,
        bindings: Vec<String>,
        is_named: bool,
        expr: Expr,
        then_block: Vec<Stmt>,
        else_block: Option<Vec<Stmt>>,
        line: usize,
        column: usize,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
        line: usize,
        column: usize,
    },
    Match {
        expr: Expr,
        arms: Vec<MatchArm>,
        line: usize,
        column: usize,
    },
    Return {
        expr: Expr,
        line: usize,
        column: usize,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MatchArm {
    pub variant: String,
    pub bindings: Vec<String>,
    pub is_named: bool,
    pub body: Vec<Stmt>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MatchExprArm {
    pub variant: String,
    pub bindings: Vec<String>,
    pub is_named: bool,
    pub expr: Expr,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Hash)]
pub enum NumericType {
    I8,
    I16,
    I32,
    I64,
    Isize,
    U8,
    U16,
    U32,
    U64,
    Usize,
    F32,
    F64,
}

impl NumericType {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "i8" => Some(Self::I8),
            "i16" => Some(Self::I16),
            "i32" => Some(Self::I32),
            "i64" => Some(Self::I64),
            "isize" => Some(Self::Isize),
            "u8" => Some(Self::U8),
            "u16" => Some(Self::U16),
            "u32" => Some(Self::U32),
            "u64" => Some(Self::U64),
            "usize" => Some(Self::Usize),
            "f32" => Some(Self::F32),
            "f64" => Some(Self::F64),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::Isize => "isize",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::Usize => "usize",
            Self::F32 => "f32",
            Self::F64 => "f64",
        }
    }

    pub fn is_float(self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
pub enum TypeName {
    Int,
    Numeric(NumericType),
    Bool,
    String,
    Str,
    Named(String, Vec<TypeName>),
    Ptr(Box<TypeName>),
    MutPtr(Box<TypeName>),
    MutRef(Box<TypeName>),
    Slice(Box<TypeName>),
    MutSlice(Box<TypeName>),
    LifetimeSlice(String, Box<TypeName>),
    LifetimeMutSlice(String, Box<TypeName>),
    Option(Box<TypeName>),
    Result(Box<TypeName>, Box<TypeName>),
    Tuple(Vec<TypeName>),
    Map(Box<TypeName>, Box<TypeName>),
    Array(Box<TypeName>, Option<String>),
    Fn(Vec<TypeName>, Box<TypeName>),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum Expr {
    Literal(Literal),
    VarRef {
        name: String,
        line: usize,
        column: usize,
    },
    Call {
        name: String,
        type_args: Vec<TypeName>,
        args: Vec<Expr>,
        line: usize,
        column: usize,
    },
    MethodCall {
        base: Box<Expr>,
        method: String,
        type_args: Vec<TypeName>,
        args: Vec<Expr>,
        line: usize,
        column: usize,
    },
    BinaryAdd {
        op: ArithmeticOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        line: usize,
        column: usize,
    },
    BinaryCompare {
        op: CompareOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        line: usize,
        column: usize,
    },
    Cast {
        expr: Box<Expr>,
        ty: TypeName,
        line: usize,
        column: usize,
    },
    MutBorrow {
        expr: Box<Expr>,
        line: usize,
        column: usize,
    },
    Deref {
        expr: Box<Expr>,
        line: usize,
        column: usize,
    },
    Try {
        expr: Box<Expr>,
        line: usize,
        column: usize,
    },
    Await {
        expr: Box<Expr>,
        line: usize,
        column: usize,
    },
    StructLiteral {
        name: String,
        type_args: Vec<TypeName>,
        fields: Vec<StructFieldValue>,
        line: usize,
        column: usize,
    },
    FieldAccess {
        base: Box<Expr>,
        field: String,
        line: usize,
        column: usize,
    },
    TupleLiteral {
        elements: Vec<Expr>,
        line: usize,
        column: usize,
    },
    TupleIndex {
        base: Box<Expr>,
        index: usize,
        line: usize,
        column: usize,
    },
    MapLiteral {
        entries: Vec<MapEntry>,
        line: usize,
        column: usize,
    },
    ArrayLiteral {
        elements: Vec<Expr>,
        line: usize,
        column: usize,
    },
    Slice {
        base: Box<Expr>,
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        line: usize,
        column: usize,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        line: usize,
        column: usize,
    },
    Closure {
        params: Vec<Param>,
        body: Box<Expr>,
        line: usize,
        column: usize,
    },
    Match {
        expr: Box<Expr>,
        arms: Vec<MatchExprArm>,
        line: usize,
        column: usize,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StructFieldValue {
    pub name: String,
    pub expr: Expr,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MapEntry {
    pub key: Expr,
    pub value: Expr,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum Literal {
    Int(i64),
    Numeric { raw: String, ty: NumericType },
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
pub enum ArithmeticOp {
    Add,
    Sub,
    Mul,
    Div,
}

impl Import {
    pub fn span_in(&self, file: &str) -> SourceSpan {
        SourceSpan::new(file, self.line, self.column)
    }

    pub fn stable_id_in(&self, file: &str) -> AstNodeId {
        self.span_in(file).stable_id("import", Some(&self.path))
    }
}

impl Function {
    pub fn span(&self) -> SourceSpan {
        SourceSpan::new(&self.path, self.line, self.column)
    }
    pub fn stable_id(&self) -> AstNodeId {
        self.span().stable_id("function", Some(&self.source_name))
    }
}

impl ConstDecl {
    pub fn span_in(&self, file: &str) -> SourceSpan {
        SourceSpan::new(file, self.line, self.column)
    }
    pub fn stable_id_in(&self, file: &str) -> AstNodeId {
        self.span_in(file).stable_id(
            if self.is_static { "static" } else { "const" },
            Some(&self.name),
        )
    }
}

impl TypeAliasDecl {
    pub fn span_in(&self, file: &str) -> SourceSpan {
        SourceSpan::new(file, self.line, self.column)
    }
    pub fn stable_id_in(&self, file: &str) -> AstNodeId {
        self.span_in(file).stable_id("type_alias", Some(&self.name))
    }
}

impl StructDecl {
    pub fn span_in(&self, file: &str) -> SourceSpan {
        SourceSpan::new(file, self.line, self.column)
    }
    pub fn stable_id_in(&self, file: &str) -> AstNodeId {
        self.span_in(file).stable_id("struct", Some(&self.name))
    }
}

impl EnumDecl {
    pub fn span_in(&self, file: &str) -> SourceSpan {
        SourceSpan::new(file, self.line, self.column)
    }
    pub fn stable_id_in(&self, file: &str) -> AstNodeId {
        self.span_in(file).stable_id("enum", Some(&self.name))
    }
}

impl TraitDecl {
    pub fn span_in(&self, file: &str) -> SourceSpan {
        SourceSpan::new(file, self.line, self.column)
    }
    pub fn stable_id_in(&self, file: &str) -> AstNodeId {
        self.span_in(file).stable_id("trait", Some(&self.name))
    }
}

impl Stmt {
    pub fn span_in(&self, file: &str) -> SourceSpan {
        let (line, column) = match self {
            Stmt::Let { line, column, .. }
            | Stmt::Assign { line, column, .. }
            | Stmt::Print { line, column, .. }
            | Stmt::Panic { line, column, .. }
            | Stmt::Defer { line, column, .. }
            | Stmt::If { line, column, .. }
            | Stmt::IfLet { line, column, .. }
            | Stmt::While { line, column, .. }
            | Stmt::Match { line, column, .. }
            | Stmt::Return { line, column, .. } => (*line, *column),
        };
        SourceSpan::new(file, line, column)
    }

    pub fn stable_id_in(&self, file: &str) -> AstNodeId {
        self.span_in(file).stable_id("stmt", None)
    }
}

impl Expr {
    pub fn span_in(&self, file: &str) -> Option<SourceSpan> {
        let (line, column) = match self {
            Expr::Literal(_) => return None,
            Expr::VarRef { line, column, .. }
            | Expr::Call { line, column, .. }
            | Expr::MethodCall { line, column, .. }
            | Expr::BinaryAdd { line, column, .. }
            | Expr::BinaryCompare { line, column, .. }
            | Expr::Cast { line, column, .. }
            | Expr::MutBorrow { line, column, .. }
            | Expr::Deref { line, column, .. }
            | Expr::Try { line, column, .. }
            | Expr::Await { line, column, .. }
            | Expr::StructLiteral { line, column, .. }
            | Expr::FieldAccess { line, column, .. }
            | Expr::TupleLiteral { line, column, .. }
            | Expr::TupleIndex { line, column, .. }
            | Expr::MapLiteral { line, column, .. }
            | Expr::ArrayLiteral { line, column, .. }
            | Expr::Slice { line, column, .. }
            | Expr::Index { line, column, .. }
            | Expr::Closure { line, column, .. }
            | Expr::Match { line, column, .. } => (*line, *column),
        };
        Some(SourceSpan::new(file, line, column))
    }

    pub fn stable_id_in(&self, file: &str) -> Option<AstNodeId> {
        self.span_in(file).map(|span| span.stable_id("expr", None))
    }
}

#[derive(Debug, Clone)]
struct MacroRule {
    name: String,
    style: MacroStyle,
    arms: Vec<MacroArm>,
    line: usize,
    column: usize,
}

#[derive(Debug, Clone)]
struct MacroArm {
    params: Vec<MacroParam>,
    template: String,
}

#[derive(Debug, Clone)]
enum MacroParam {
    Single(String),
    Repeat {
        name: String,
        separator: Option<char>,
    },
}

impl MacroParam {
    fn name(&self) -> &str {
        match self {
            MacroParam::Single(name) | MacroParam::Repeat { name, .. } => name,
        }
    }
}

struct ExpandedSource {
    source: String,
    macros: Vec<MacroDecl>,
    expansions: Vec<MacroExpansion>,
}

#[derive(Clone)]
struct ExpandedSourceLine {
    text: String,
    original_line: usize,
}

pub fn parse_program(source: &str, path: &Path) -> Result<Program, Diagnostic> {
    parse_program_with_recovery(source, path).map_err(|mut diagnostics| {
        let mut first = diagnostics.remove(0);
        first.related = diagnostics;
        first
    })
}

pub fn parse_program_with_recovery(source: &str, path: &Path) -> Result<Program, Vec<Diagnostic>> {
    parse_program_with_options_and_recovery(source, path, &ParseOptions::default())
}

pub fn parse_program_with_options(
    source: &str,
    path: &Path,
    options: &ParseOptions,
) -> Result<Program, Diagnostic> {
    parse_program_with_options_and_recovery(source, path, options).map_err(|mut diagnostics| {
        let mut first = diagnostics.remove(0);
        first.related = diagnostics;
        first
    })
}

pub fn parse_program_with_options_and_recovery(
    source: &str,
    path: &Path,
    options: &ParseOptions,
) -> Result<Program, Vec<Diagnostic>> {
    let expanded_source = expand_declarative_macros(source, path, options)?;
    let lines: Vec<&str> = expanded_source.source.lines().collect();
    let mut index = 0;
    let mut imports = Vec::new();
    let mut axioms = Vec::new();
    let mut semantic_capabilities = Vec::new();
    let mut evidence = Vec::new();
    let mut consts = Vec::new();
    let mut type_aliases = Vec::new();
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut traits = Vec::new();
    let mut functions = Vec::new();
    let mut stmts = Vec::new();
    let mut diagnostics = Vec::new();
    while index < lines.len() {
        let line_no = index + 1;
        let trimmed = lines[index].trim();
        if trimmed.is_empty() {
            index += 1;
            continue;
        }
        match trimmed {
            "}" => {
                diagnostics.push(
                    Diagnostic::new("parse", "unexpected closing brace")
                        .with_path(path.display().to_string())
                        .with_span(line_no, 1),
                );
                index += 1;
                continue;
            }
            "} else {" | "else {" => {
                diagnostics.push(
                    Diagnostic::new("parse", "unexpected else block")
                        .with_path(path.display().to_string())
                        .with_span(line_no, 1),
                );
                index += 1;
                continue;
            }
            _ => {}
        }
        match parse_import(trimmed, path, line_no) {
            Ok(Some(import)) => {
                imports.push(import);
                index += 1;
                continue;
            }
            Ok(None) => {}
            Err(error) => {
                diagnostics.push(error);
                index += 1;
                continue;
            }
        }
        if trimmed.starts_with("pub import ")
            || trimmed.starts_with("pub(pkg) import ")
            || trimmed.starts_with("pub use ")
            || trimmed.starts_with("pub(pkg) use ")
            || trimmed.starts_with("export ")
        {
            diagnostics.push(
                Diagnostic::new(
                    "parse",
                    "stage1 bootstrap does not support re-exports; expose public symbols from their defining module instead",
                )
                .with_path(path.display().to_string())
                .with_span(line_no, 1),
            );
            index += 1;
            continue;
        }
        if trimmed.starts_with("axiom ") {
            let start_index = index;
            match parse_axiom_decl(&lines, &mut index, path) {
                Ok(decl) => axioms.push(decl),
                Err(error) => {
                    diagnostics.push(error);
                    index = start_index;
                    synchronize_top_level(&lines, &mut index);
                }
            }
            continue;
        }
        if trimmed.starts_with("capability ") {
            let start_index = index;
            match parse_semantic_capability_decl(&lines, &mut index, path) {
                Ok(decl) => semantic_capabilities.push(decl),
                Err(error) => {
                    diagnostics.push(error);
                    index = start_index;
                    synchronize_top_level(&lines, &mut index);
                }
            }
            continue;
        }
        if trimmed.starts_with("evidence ") {
            let start_index = index;
            match parse_evidence_decl(&lines, &mut index, path) {
                Ok(decl) => evidence.push(decl),
                Err(error) => {
                    diagnostics.push(error);
                    index = start_index;
                    synchronize_top_level(&lines, &mut index);
                }
            }
            continue;
        }
        if trimmed.starts_with("fn ")
            || trimmed.starts_with("const fn ")
            || trimmed.starts_with("async fn ")
            || trimmed.starts_with("extern fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub const fn ")
            || trimmed.starts_with("pub async fn ")
            || trimmed.starts_with("pub extern fn ")
            || trimmed.starts_with("pub(pkg) fn ")
            || trimmed.starts_with("pub(pkg) const fn ")
            || trimmed.starts_with("pub(pkg) async fn ")
            || trimmed.starts_with("pub(pkg) extern fn ")
        {
            let start_index = index;
            match parse_function(&lines, &mut index, path) {
                Ok(function) => functions.push(function),
                Err(error) => {
                    push_diagnostic_with_related(&mut diagnostics, error);
                    index = start_index;
                    synchronize_top_level(&lines, &mut index);
                }
            }
            continue;
        }
        if trimmed.starts_with("const ")
            || trimmed.starts_with("pub const ")
            || trimmed.starts_with("pub(pkg) const ")
            || trimmed.starts_with("static ")
            || trimmed.starts_with("pub static ")
            || trimmed.starts_with("pub(pkg) static ")
        {
            match parse_const_or_static_decl(trimmed, path, line_no) {
                Ok(const_decl) => consts.push(const_decl),
                Err(error) => diagnostics.push(error),
            }
            index += 1;
            continue;
        }
        if trimmed.starts_with("type ")
            || trimmed.starts_with("pub type ")
            || trimmed.starts_with("pub(pkg) type ")
        {
            match parse_type_alias(trimmed, path, line_no) {
                Ok(type_alias) => type_aliases.push(type_alias),
                Err(error) => diagnostics.push(error),
            }
            index += 1;
            continue;
        }
        if trimmed.starts_with("struct ")
            || trimmed.starts_with("pub struct ")
            || trimmed.starts_with("pub(pkg) struct ")
        {
            let start_index = index;
            match parse_struct(&lines, &mut index, path) {
                Ok(struct_decl) => structs.push(struct_decl),
                Err(error) => {
                    push_diagnostic_with_related(&mut diagnostics, error);
                    index = start_index;
                    synchronize_top_level(&lines, &mut index);
                }
            }
            continue;
        }
        if trimmed.starts_with("enum ")
            || trimmed.starts_with("pub enum ")
            || trimmed.starts_with("pub(pkg) enum ")
        {
            let start_index = index;
            match parse_enum(&lines, &mut index, path) {
                Ok(enum_decl) => enums.push(enum_decl),
                Err(error) => {
                    push_diagnostic_with_related(&mut diagnostics, error);
                    index = start_index;
                    synchronize_top_level(&lines, &mut index);
                }
            }
            continue;
        }
        if trimmed.starts_with("trait ")
            || trimmed.starts_with("pub trait ")
            || trimmed.starts_with("pub(pkg) trait ")
        {
            let start_index = index;
            match parse_trait(&lines, &mut index, path) {
                Ok(trait_decl) => traits.push(trait_decl),
                Err(error) => {
                    push_diagnostic_with_related(&mut diagnostics, error);
                    index = start_index;
                    synchronize_top_level(&lines, &mut index);
                }
            }
            continue;
        }
        if trimmed.starts_with("impl ") {
            let start_index = index;
            match parse_impl(&lines, &mut index, path) {
                Ok(methods) => functions.extend(methods),
                Err(error) => {
                    push_diagnostic_with_related(&mut diagnostics, error);
                    index = start_index;
                    synchronize_top_level(&lines, &mut index);
                }
            }
            continue;
        }
        let start_index = index;
        match parse_stmt(&lines, &mut index, path, false) {
            Ok(stmt) => stmts.push(stmt),
            Err(error) => {
                push_diagnostic_with_related(&mut diagnostics, error);
                index = start_index;
                synchronize_top_level(&lines, &mut index);
            }
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    Ok(Program {
        path: path.display().to_string(),
        imports,
        macros: expanded_source.macros,
        macro_expansions: expanded_source.expansions,
        axioms,
        semantic_capabilities,
        evidence,
        consts,
        type_aliases,
        structs,
        enums,
        traits,
        functions,
        stmts,
    })
}

fn expand_declarative_macros(
    source: &str,
    path: &Path,
    options: &ParseOptions,
) -> Result<ExpandedSource, Vec<Diagnostic>> {
    if options.macro_recursion_limit == 0 {
        return Err(vec![
            Diagnostic::new("parse", "macro recursion limit must be at least 1")
                .with_path(path.display().to_string())
                .with_span(1, 1),
        ]);
    }
    let (macros, mut expanded_lines) = collect_macro_rules(source, path)?;
    let declarations = macro_declarations(&macros);
    if macros.is_empty() {
        return Ok(ExpandedSource {
            source: expanded_lines_to_source(&expanded_lines),
            macros: declarations,
            expansions: Vec::new(),
        });
    }
    let mut expansions = Vec::new();
    let mut invocation_chain = Vec::new();
    for pass in 0..options.macro_recursion_limit {
        let pass_depth = pass + 1;
        let (next, line_expansions) =
            expand_macro_invocations_once(&expanded_lines, &macros, path, pass_depth)?;
        let changed = !line_expansions.is_empty();
        for expansion in &line_expansions {
            invocation_chain.push(format!("{}!", expansion.macro_name));
        }
        expansions.extend(line_expansions);
        expanded_lines = next;
        if !changed {
            return Ok(ExpandedSource {
                source: expanded_lines_to_source(&expanded_lines),
                macros: declarations,
                expansions,
            });
        }
    }
    if source_has_macro_invocation(&expanded_lines, &macros) {
        let chain = if invocation_chain.is_empty() {
            String::from("<unknown>")
        } else {
            invocation_chain
                .iter()
                .rev()
                .take(8)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join(" -> ")
        };
        return Err(vec![Diagnostic::new(
            "parse",
            format!(
                "declarative macro expansion exceeded bounded depth of {}; invocation chain: {chain}",
                options.macro_recursion_limit
            ),
        )
        .with_path(path.display().to_string())
        .with_span(1, 1)]);
    }
    Ok(ExpandedSource {
        source: expanded_lines_to_source(&expanded_lines),
        macros: declarations,
        expansions,
    })
}

fn source_has_macro_invocation(
    source: &[ExpandedSourceLine],
    macros: &std::collections::HashMap<String, MacroRule>,
) -> bool {
    source.iter().any(|line| {
        macros.values().any(|rule| {
            let needle = format!("{}!(", rule.name);
            find_macro_invocation(&line.text, &needle).is_some()
        })
    })
}

fn expanded_lines_to_source(lines: &[ExpandedSourceLine]) -> String {
    lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect_macro_rules(
    source: &str,
    path: &Path,
) -> Result<
    (
        std::collections::HashMap<String, MacroRule>,
        Vec<ExpandedSourceLine>,
    ),
    Vec<Diagnostic>,
> {
    let lines: Vec<&str> = source.lines().collect();
    let mut macros = std::collections::HashMap::new();
    let mut kept = Vec::new();
    let mut index = 0usize;
    let mut top_level_depth = 0i32;
    while index < lines.len() {
        let trimmed = lines[index].trim_start();
        if !starts_macro_definition(trimmed) {
            kept.push(ExpandedSourceLine {
                text: lines[index].to_string(),
                original_line: index + 1,
            });
            top_level_depth += brace_delta(lines[index]);
            index += 1;
            continue;
        }
        let start_line = index + 1;
        let style = macro_style_for_definition(trimmed).expect("recognized macro definition");
        if top_level_depth != 0 {
            return Err(vec![
                Diagnostic::new(
                    "parse",
                    "declarative macro definitions are only supported at top level",
                )
                .with_path(path.display().to_string())
                .with_span(start_line, 1),
            ]);
        }
        let mut definition = String::new();
        let mut depth = 0i32;
        loop {
            let line = lines.get(index).ok_or_else(|| {
                vec![
                    Diagnostic::new("parse", "unterminated declarative macro definition")
                        .with_path(path.display().to_string())
                        .with_span(start_line, 1),
                ]
            })?;
            if !definition.is_empty() {
                definition.push('\n');
            }
            definition.push_str(line);
            depth += brace_delta(line);
            index += 1;
            if depth == 0 && definition.contains('{') {
                break;
            }
        }
        let rule = parse_macro_rule(&definition, style, path, start_line)?;
        if macros.insert(rule.name.clone(), rule).is_some() {
            return Err(vec![
                Diagnostic::new("parse", "duplicate declarative macro definition")
                    .with_path(path.display().to_string())
                    .with_span(start_line, 1),
            ]);
        }
    }
    Ok((macros, kept))
}

fn starts_macro_definition(trimmed: &str) -> bool {
    macro_style_for_definition(trimmed).is_some()
}

fn macro_style_for_definition(trimmed: &str) -> Option<MacroStyle> {
    if trimmed.starts_with("macro_rules! ") {
        Some(MacroStyle::MacroRules)
    } else if trimmed.starts_with("macro ") {
        Some(MacroStyle::Macro)
    } else {
        None
    }
}

fn macro_declarations(macros: &std::collections::HashMap<String, MacroRule>) -> Vec<MacroDecl> {
    let mut declarations = macros
        .values()
        .map(|rule| {
            let params = rule
                .arms
                .iter()
                .flat_map(|arm| arm.params.iter().map(|param| param.name().to_string()))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            MacroDecl {
                name: rule.name.clone(),
                style: rule.style,
                params,
                line: rule.line,
                column: rule.column,
            }
        })
        .collect::<Vec<_>>();
    declarations.sort_by(|left, right| left.line.cmp(&right.line).then(left.name.cmp(&right.name)));
    declarations
}

fn parse_macro_rule(
    definition: &str,
    style: MacroStyle,
    path: &Path,
    line_no: usize,
) -> Result<MacroRule, Vec<Diagnostic>> {
    let trimmed = definition.trim();
    let (rest, name_column) = match style {
        MacroStyle::MacroRules => {
            let Some(rest) = trimmed.strip_prefix("macro_rules! ") else {
                return Err(vec![
                    Diagnostic::new("parse", "invalid macro_rules! definition")
                        .with_path(path.display().to_string())
                        .with_span(line_no, 1),
                ]);
            };
            (rest, 14)
        }
        MacroStyle::Macro => {
            let Some(rest) = trimmed.strip_prefix("macro ") else {
                return Err(vec![
                    Diagnostic::new("parse", "invalid macro definition")
                        .with_path(path.display().to_string())
                        .with_span(line_no, 1),
                ]);
            };
            (rest, 7)
        }
    };
    let Some(open_brace) = find_top_level_char(rest, '{') else {
        return Err(vec![
            Diagnostic::new("parse", "declarative macro definition is missing '{'")
                .with_path(path.display().to_string())
                .with_span(line_no, 1),
        ]);
    };
    let name = rest[..open_brace].trim();
    validate_ident(name, path, line_no, name_column).map_err(|error| vec![error])?;
    let Some(close_brace) = find_matching_brace(rest, open_brace) else {
        return Err(vec![
            Diagnostic::new(
                "parse",
                "declarative macro definition is missing closing '}'",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1),
        ]);
    };
    if !rest[close_brace + 1..].trim().is_empty() {
        return Err(vec![
            Diagnostic::new(
                "parse",
                "unexpected tokens after declarative macro definition",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, close_brace + 1),
        ]);
    }
    let body = rest[open_brace + 1..close_brace].trim();
    let mut arms = Vec::new();
    for arm in split_top_level(body, ';') {
        let arm = arm.trim();
        if arm.is_empty() {
            continue;
        }
        arms.push(parse_macro_arm(arm, path, line_no)?);
    }
    if arms.is_empty() {
        return Err(vec![
            Diagnostic::new(
                "parse",
                "declarative macro definition must contain at least one arm",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1),
        ]);
    }
    Ok(MacroRule {
        name: name.to_string(),
        style,
        arms,
        line: line_no,
        column: name_column,
    })
}

fn parse_macro_arm(arm: &str, path: &Path, line_no: usize) -> Result<MacroArm, Vec<Diagnostic>> {
    let Some(arrow) = arm.find("=>") else {
        return Err(vec![
            Diagnostic::new("parse", "declarative macro arm must contain `=>`")
                .with_path(path.display().to_string())
                .with_span(line_no, 1),
        ]);
    };
    let pattern = arm[..arrow].trim();
    let template = arm[arrow + 2..].trim();
    if !pattern.starts_with('(')
        || !pattern.ends_with(')')
        || find_matching_paren(pattern, 0) != Some(pattern.len() - 1)
    {
        return Err(vec![
            Diagnostic::new(
                "parse",
                "declarative macro pattern must use `($name:fragment, ...)` syntax",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1),
        ]);
    }
    let Some(template) = strip_macro_template_delimiters(template) else {
        return Err(vec![
            Diagnostic::new(
                "parse",
                "declarative macro expansion must be enclosed in braces or parentheses",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1),
        ]);
    };
    let params = parse_macro_params(pattern, path, line_no)?;
    Ok(MacroArm {
        params,
        template: template.trim_matches('\n').to_string(),
    })
}

fn strip_macro_template_delimiters(template: &str) -> Option<&str> {
    if template.starts_with('{')
        && template.ends_with('}')
        && find_matching_brace(template, 0) == Some(template.len() - 1)
    {
        Some(&template[1..template.len() - 1])
    } else if template.starts_with('(')
        && template.ends_with(')')
        && find_matching_paren(template, 0) == Some(template.len() - 1)
    {
        Some(&template[1..template.len() - 1])
    } else {
        None
    }
}

fn parse_macro_params(
    pattern: &str,
    path: &Path,
    line_no: usize,
) -> Result<Vec<MacroParam>, Vec<Diagnostic>> {
    let params_raw = pattern[1..pattern.len() - 1].trim();
    if params_raw.is_empty() {
        return Ok(Vec::new());
    }
    if params_raw.contains("$(") {
        return parse_repetition_macro_param(params_raw, path, line_no).map(|param| vec![param]);
    }
    let mut params = Vec::new();
    for part in split_top_level(params_raw, ',') {
        let part = part.trim();
        let Some(part) = part.strip_prefix('$') else {
            return Err(vec![
                Diagnostic::new("parse", "macro parameter must start with `$`")
                    .with_path(path.display().to_string())
                    .with_span(line_no, 1),
            ]);
        };
        let name = part
            .split_once(':')
            .map(|(name, _)| name)
            .unwrap_or(part)
            .trim();
        validate_ident(name, path, line_no, 1).map_err(|error| vec![error])?;
        if params
            .iter()
            .any(|existing: &MacroParam| existing.name() == name)
        {
            return Err(vec![
                Diagnostic::new("parse", format!("duplicate macro parameter {name:?}"))
                    .with_path(path.display().to_string())
                    .with_span(line_no, 1),
            ]);
        }
        params.push(MacroParam::Single(name.to_string()));
    }
    Ok(params)
}

fn parse_repetition_macro_param(
    raw: &str,
    path: &Path,
    line_no: usize,
) -> Result<MacroParam, Vec<Diagnostic>> {
    let Some(start) = raw.find("$(") else {
        return Err(vec![
            Diagnostic::new(
                "parse",
                "macro repetition must use `$($name:fragment)*` syntax",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1),
        ]);
    };
    if raw[..start].trim().is_empty() {
        let open = start + 1;
        let Some(close) = find_matching_paren(raw, open) else {
            return Err(vec![
                Diagnostic::new("parse", "macro repetition is missing closing `)`")
                    .with_path(path.display().to_string())
                    .with_span(line_no, 1),
            ]);
        };
        if !raw[close + 1..].trim().ends_with('*') {
            return Err(vec![
                Diagnostic::new("parse", "macro repetition must end with `*`")
                    .with_path(path.display().to_string())
                    .with_span(line_no, 1),
            ]);
        }
        let separator = raw[close + 1..]
            .trim()
            .strip_suffix('*')
            .and_then(|prefix| prefix.trim().chars().next());
        let inner = raw[open + 1..close].trim();
        let Some(inner) = inner.strip_prefix('$') else {
            return Err(vec![
                Diagnostic::new("parse", "macro repetition parameter must start with `$`")
                    .with_path(path.display().to_string())
                    .with_span(line_no, 1),
            ]);
        };
        let name = inner
            .split_once(':')
            .map(|(name, _)| name)
            .unwrap_or(inner)
            .trim();
        validate_ident(name, path, line_no, 1).map_err(|error| vec![error])?;
        return Ok(MacroParam::Repeat {
            name: name.to_string(),
            separator,
        });
    }
    Err(vec![
        Diagnostic::new(
            "parse",
            "stage1 macro repetition supports a single repeated parameter per arm",
        )
        .with_path(path.display().to_string())
        .with_span(line_no, 1),
    ])
}

fn expand_macro_invocations_once(
    source: &[ExpandedSourceLine],
    macros: &std::collections::HashMap<String, MacroRule>,
    path: &Path,
    depth: usize,
) -> Result<(Vec<ExpandedSourceLine>, Vec<MacroExpansion>), Vec<Diagnostic>> {
    let mut expansions = Vec::new();
    let mut output = Vec::new();
    for line in source {
        let (expanded, line_expansion) =
            expand_macro_line_once(&line.text, macros, path, line.original_line, depth)?;
        if let Some(expansion) = line_expansion {
            expansions.push(expansion);
        }
        output.extend(expanded);
    }
    Ok((output, expansions))
}

fn expand_macro_line_once(
    line: &str,
    macros: &std::collections::HashMap<String, MacroRule>,
    path: &Path,
    line_no: usize,
    depth: usize,
) -> Result<(Vec<ExpandedSourceLine>, Option<MacroExpansion>), Vec<Diagnostic>> {
    let mut first_match: Option<(usize, usize, &MacroRule)> = None;
    for rule in macros.values() {
        let needle = format!("{}!(", rule.name);
        if let Some(start) = find_macro_invocation(line, &needle) {
            let open = start + rule.name.len() + 1;
            if let Some(close) = find_matching_paren(line, open) {
                if first_match.is_none_or(|(existing, _, _)| start < existing) {
                    first_match = Some((start, close, rule));
                }
            } else {
                return Err(vec![
                    Diagnostic::new("parse", "macro invocation is missing closing ')'")
                        .with_path(path.display().to_string())
                        .with_span(line_no, start + 1),
                ]);
            }
        }
    }
    let Some((start, close, rule)) = first_match else {
        return Ok((
            vec![ExpandedSourceLine {
                text: line.to_string(),
                original_line: line_no,
            }],
            None,
        ));
    };
    let args_raw = &line[start + rule.name.len() + 2..close];
    let args: Vec<&str> = if args_raw.trim().is_empty() {
        Vec::new()
    } else {
        split_top_level(args_raw, ',')
            .into_iter()
            .map(str::trim)
            .collect()
    };
    let Some(arm) = select_macro_arm(rule, &args) else {
        return Err(vec![
            Diagnostic::new(
                "parse",
                format!(
                    "macro {}! does not have a matching arm for {} argument(s)",
                    rule.name,
                    args.len()
                ),
            )
            .with_path(path.display().to_string())
            .with_span(line_no, start + 1),
        ]);
    };
    let hygiene_prefix = format!("__axiom_macro_{}_{}_{}_", rule.name, line_no, start + 1);
    let template = rename_introduced_bindings(&arm.template, &arm.params, &hygiene_prefix);
    let expansion = render_macro_expansion(&template, &arm.params, &args);
    let before = &line[..start];
    let after = &line[close + 1..];
    let invocation_is_statement = before.trim().is_empty() && after.trim().is_empty();
    let metadata = MacroExpansion {
        macro_name: rule.name.clone(),
        call_site: MacroSpan {
            path: path.display().to_string(),
            line: line_no,
            column: start + 1,
        },
        definition_site: MacroSpan {
            path: path.display().to_string(),
            line: rule.line,
            column: rule.column,
        },
        depth,
    };
    if invocation_is_statement {
        let indent = before;
        let lines = expansion
            .lines()
            .map(|expanded_line| {
                let text = if expanded_line.trim().is_empty() {
                    String::new()
                } else {
                    format!("{indent}{}", expanded_line.trim())
                };
                ExpandedSourceLine {
                    text,
                    original_line: line_no,
                }
            })
            .collect();
        return Ok((lines, Some(metadata)));
    }
    if expansion.lines().count() > 1 {
        return Err(vec![
            Diagnostic::new(
                "parse",
                format!(
                    "multi-line macro {}! can only be invoked as a full statement",
                    rule.name
                ),
            )
            .with_path(path.display().to_string())
            .with_span(line_no, start + 1),
        ]);
    }
    Ok((
        vec![ExpandedSourceLine {
            text: format!("{}{}{}", before, expansion.trim(), after),
            original_line: line_no,
        }],
        Some(metadata),
    ))
}

fn select_macro_arm<'a>(rule: &'a MacroRule, args: &[&str]) -> Option<&'a MacroArm> {
    rule.arms.iter().find(|arm| {
        if arm.params.len() == 1 && matches!(arm.params[0], MacroParam::Repeat { .. }) {
            true
        } else {
            arm.params.len() == args.len()
        }
    })
}

fn find_macro_invocation(line: &str, needle: &str) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '#' => return None,
            _ => {
                if line[index..].starts_with(needle)
                    && line[..index]
                        .chars()
                        .next_back()
                        .is_none_or(|previous| !is_identifier_char(previous))
                {
                    return Some(index);
                }
            }
        }
    }
    None
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn render_macro_expansion(template: &str, params: &[MacroParam], args: &[&str]) -> String {
    if let Some(MacroParam::Repeat { name, separator }) = params
        .iter()
        .find(|param| matches!(param, MacroParam::Repeat { .. }))
    {
        return render_repetition_macro_expansion(template, name, *separator, args);
    }
    let mut output = String::new();
    let mut chars = template.char_indices().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some((index, ch)) = chars.next() {
        if in_string {
            output.push(ch);
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => {
                in_string = true;
                output.push(ch);
                continue;
            }
            '#' => {
                output.push_str(&template[index..]);
                break;
            }
            '$' => {}
            _ => {
                output.push(ch);
                continue;
            }
        }
        let name_start = index + ch.len_utf8();
        let Some((_, first)) = chars.peek().copied() else {
            output.push(ch);
            continue;
        };
        if !(first.is_ascii_alphabetic() || first == '_') {
            output.push(ch);
            continue;
        }
        let mut name_end = name_start;
        while let Some((next_index, next_ch)) = chars.peek().copied() {
            if is_identifier_char(next_ch) {
                name_end = next_index + next_ch.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        let name = &template[name_start..name_end];
        if let Some(position) = params.iter().position(|param| param.name() == name) {
            output.push_str(args[position]);
        } else {
            output.push('$');
            output.push_str(name);
        }
    }
    output
}

fn render_repetition_macro_expansion(
    template: &str,
    name: &str,
    separator: Option<char>,
    args: &[&str],
) -> String {
    let mut output = template.to_string();
    let joined = args.join(&separator.map(|ch| ch.to_string()).unwrap_or_default());
    let compact_comma = format!("$(${name}),*");
    let spaced_comma = format!("$(${name}), *");
    let compact = format!("$(${name})*");
    output = output.replace(&compact_comma, &joined);
    output = output.replace(&spaced_comma, &joined);
    output.replace(&compact, &joined)
}

fn rename_introduced_bindings(
    template: &str,
    params: &[MacroParam],
    hygiene_prefix: &str,
) -> String {
    let param_names = params
        .iter()
        .map(MacroParam::name)
        .collect::<std::collections::BTreeSet<_>>();
    let mut introduced = std::collections::BTreeSet::new();
    for line in template.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("let ") else {
            continue;
        };
        let name = rest
            .chars()
            .take_while(|ch| is_identifier_char(*ch))
            .collect::<String>();
        if !name.is_empty() && !param_names.contains(name.as_str()) {
            introduced.insert(name);
        }
    }
    let mut renamed = template.to_string();
    for name in introduced {
        renamed = replace_identifier_outside_strings(
            &renamed,
            &name,
            &format!("{hygiene_prefix}{name}"),
            true,
        );
    }
    renamed
}

fn replace_identifier_outside_strings(
    source: &str,
    needle: &str,
    replacement: &str,
    skip_macro_parameter: bool,
) -> String {
    let mut output = String::new();
    let mut index = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    while index < source.len() {
        let ch = source[index..].chars().next().expect("valid char boundary");
        if in_string {
            output.push(ch);
            index += ch.len_utf8();
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            output.push(ch);
            index += ch.len_utf8();
            continue;
        }
        if ch == '#' {
            output.push_str(&source[index..]);
            break;
        }
        if source[index..].starts_with(needle)
            && source[..index]
                .chars()
                .next_back()
                .is_none_or(|previous| !is_identifier_char(previous))
            && source[index + needle.len()..]
                .chars()
                .next()
                .is_none_or(|next| !is_identifier_char(next))
            && (!skip_macro_parameter
                || source[..index]
                    .chars()
                    .next_back()
                    .is_none_or(|previous| previous != '$'))
        {
            output.push_str(replacement);
            index += needle.len();
            continue;
        }
        output.push(ch);
        index += ch.len_utf8();
    }
    output
}

fn synchronize_top_level(lines: &[&str], index: &mut usize) {
    if *index >= lines.len() {
        return;
    }
    let mut depth = brace_delta(lines[*index]);
    *index += 1;
    while *index < lines.len() && depth > 0 {
        if depth == 1 && is_top_level_recovery_anchor(lines[*index]) {
            return;
        }
        depth += brace_delta(lines[*index]);
        *index += 1;
    }
}

fn is_top_level_recovery_anchor(line: &str) -> bool {
    if line.trim_start() != line {
        return false;
    }
    let trimmed = line.trim();
    trimmed.starts_with("import ")
        || trimmed.starts_with("pub import ")
        || trimmed.starts_with("pub(pkg) import ")
        || trimmed.starts_with("pub use ")
        || trimmed.starts_with("pub(pkg) use ")
        || trimmed.starts_with("export ")
        || trimmed.starts_with("fn ")
        || trimmed.starts_with("async fn ")
        || trimmed.starts_with("extern fn ")
        || trimmed.starts_with("pub fn ")
        || trimmed.starts_with("pub async fn ")
        || trimmed.starts_with("pub extern fn ")
        || trimmed.starts_with("pub(pkg) fn ")
        || trimmed.starts_with("pub(pkg) async fn ")
        || trimmed.starts_with("pub(pkg) extern fn ")
        || trimmed.starts_with("const ")
        || trimmed.starts_with("pub const ")
        || trimmed.starts_with("pub(pkg) const ")
        || trimmed.starts_with("static ")
        || trimmed.starts_with("pub static ")
        || trimmed.starts_with("pub(pkg) static ")
        || trimmed.starts_with("type ")
        || trimmed.starts_with("pub type ")
        || trimmed.starts_with("pub(pkg) type ")
        || trimmed.starts_with("struct ")
        || trimmed.starts_with("pub struct ")
        || trimmed.starts_with("pub(pkg) struct ")
        || trimmed.starts_with("enum ")
        || trimmed.starts_with("pub enum ")
        || trimmed.starts_with("pub(pkg) enum ")
        || trimmed.starts_with("trait ")
        || trimmed.starts_with("pub trait ")
        || trimmed.starts_with("pub(pkg) trait ")
        || trimmed.starts_with("impl ")
}

fn brace_delta(line: &str) -> i32 {
    let mut delta = 0;
    let mut in_string = false;
    let mut escaped = false;
    for ch in line.chars() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '#' => break,
            '{' => delta += 1,
            '}' => delta -= 1,
            _ => {}
        }
    }
    delta
}

fn parse_import(trimmed: &str, path: &Path, line_no: usize) -> Result<Option<Import>, Diagnostic> {
    let Some(rest) = trimmed.strip_prefix("import ") else {
        return Ok(None);
    };
    if let Some((import_path, alias)) = rest.split_once(" as ")
        && serde_json::from_str::<String>(import_path.trim()).is_ok()
    {
        let message = if alias.trim().is_empty() {
            "stage1 bootstrap does not support import aliases"
        } else {
            "stage1 bootstrap does not support import aliases; import exported symbols directly"
        };
        let column = trimmed.find(" as ").map(|index| index + 2).unwrap_or(1);
        return Err(Diagnostic::new("parse", message)
            .with_path(path.display().to_string())
            .with_span(line_no, column));
    }
    let import_path = serde_json::from_str::<String>(rest).map_err(|_| {
        Diagnostic::new("parse", "import must use a quoted relative path")
            .with_path(path.display().to_string())
            .with_span(line_no, 1)
    })?;
    Ok(Some(Import {
        path: import_path,
        line: line_no,
        column: 1,
    }))
}

fn parse_stmt_list(
    lines: &[&str],
    index: &mut usize,
    path: &Path,
) -> Result<Vec<Stmt>, Diagnostic> {
    let mut stmts = Vec::new();
    while *index < lines.len() {
        let line_no = *index + 1;
        let trimmed = lines[*index].trim();
        if trimmed.is_empty() {
            *index += 1;
            continue;
        }
        if trimmed == "}" {
            *index += 1;
            return Ok(stmts);
        }
        if trimmed == "} else {" {
            return Ok(stmts);
        }
        if trimmed == "else {" {
            return Err(Diagnostic::new("parse", "unexpected else block")
                .with_path(path.display().to_string())
                .with_span(line_no, 1));
        }
        if trimmed.starts_with("import ") {
            return Err(Diagnostic::new(
                "parse",
                "stage1 bootstrap only supports imports at the top level",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
        }
        if trimmed.starts_with("pub import ")
            || trimmed.starts_with("pub use ")
            || trimmed.starts_with("export ")
        {
            return Err(Diagnostic::new(
                "parse",
                "stage1 bootstrap does not support re-exports inside blocks",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
        }
        if trimmed.starts_with("fn ")
            || trimmed.starts_with("async fn ")
            || trimmed.starts_with("extern fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub async fn ")
            || trimmed.starts_with("pub extern fn ")
        {
            return Err(Diagnostic::new(
                "parse",
                "stage1 bootstrap only supports top-level function declarations",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
        }
        if trimmed.starts_with("const ")
            || trimmed.starts_with("pub const ")
            || trimmed.starts_with("static ")
            || trimmed.starts_with("pub static ")
            || trimmed.starts_with("pub(pkg) static ")
        {
            return Err(Diagnostic::new(
                "parse",
                "stage1 bootstrap only supports top-level const/static declarations",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
        }
        if trimmed.starts_with("type ") || trimmed.starts_with("pub type ") {
            return Err(Diagnostic::new(
                "parse",
                "stage1 bootstrap only supports top-level type alias declarations",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
        }
        if trimmed.starts_with("struct ") || trimmed.starts_with("pub struct ") {
            return Err(Diagnostic::new(
                "parse",
                "stage1 bootstrap only supports top-level struct declarations",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
        }
        if trimmed.starts_with("enum ") || trimmed.starts_with("pub enum ") {
            return Err(Diagnostic::new(
                "parse",
                "stage1 bootstrap only supports top-level enum declarations",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
        }
        if trimmed.starts_with("trait ")
            || trimmed.starts_with("pub trait ")
            || trimmed.starts_with("pub(pkg) trait ")
        {
            return Err(Diagnostic::new(
                "parse",
                "stage1 bootstrap only supports top-level trait declarations",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
        }
        match parse_stmt(lines, index, path, true) {
            Ok(stmt) => stmts.push(stmt),
            Err(error) => {
                let mut diagnostics = vec![error];
                let failed_index = *index;
                synchronize_statement(lines, index);
                while *index < lines.len() {
                    let trimmed = lines[*index].trim();
                    if trimmed.is_empty() {
                        *index += 1;
                        continue;
                    }
                    if trimmed == "}" || trimmed == "} else {" {
                        break;
                    }
                    match parse_stmt(lines, index, path, true) {
                        Ok(stmt) => stmts.push(stmt),
                        Err(error) => {
                            diagnostics.push(error);
                            let before = *index;
                            synchronize_statement(lines, index);
                            if *index == before {
                                *index += 1;
                            }
                        }
                    }
                }
                let mut first = diagnostics.remove(0);
                first.related = diagnostics;
                if *index == failed_index {
                    *index += 1;
                }
                return Err(first);
            }
        }
    }
    Err(Diagnostic::new("parse", "missing closing brace for block")
        .with_path(path.display().to_string())
        .with_span(lines.len().max(1), 1))
}

fn push_diagnostic_with_related(diagnostics: &mut Vec<Diagnostic>, mut diagnostic: Diagnostic) {
    diagnostics.push(Diagnostic {
        related: Vec::new(),
        ..diagnostic.clone()
    });
    diagnostics.append(&mut diagnostic.related);
}

fn synchronize_statement(lines: &[&str], index: &mut usize) {
    if *index >= lines.len() {
        return;
    }
    let trimmed = lines[*index].trim();
    if trimmed.ends_with('{') {
        synchronize_nested_block(lines, index);
    } else {
        *index += 1;
    }
}

fn synchronize_nested_block(lines: &[&str], index: &mut usize) {
    let mut depth = 0usize;
    while *index < lines.len() {
        let trimmed = lines[*index].trim();
        if trimmed.ends_with('{') {
            depth += 1;
        }
        if trimmed == "}" || trimmed == "} else {" {
            if depth == 0 {
                return;
            }
            depth -= 1;
            *index += 1;
            if depth == 0 {
                return;
            }
            continue;
        }
        *index += 1;
    }
}

fn parse_stmt(
    lines: &[&str],
    index: &mut usize,
    path: &Path,
    in_block: bool,
) -> Result<Stmt, Diagnostic> {
    let line_no = *index + 1;
    let trimmed = lines[*index].trim();
    if trimmed.starts_with("for ") {
        return Err(Diagnostic::new(
            "parse",
            "stage1 bootstrap does not support `for` loops yet; use `while`-based iteration until the iteration protocol lands",
        )
        .with_path(path.display().to_string())
        .with_span(line_no, 1));
    }
    if trimmed.starts_with("if ") {
        return parse_if_stmt(lines, index, path);
    }
    if trimmed.starts_with("while ") {
        return parse_while_stmt(lines, index, path);
    }
    if trimmed.starts_with("match ") {
        return parse_match_stmt(lines, index, path);
    }
    if let Some(rest) = trimmed.strip_prefix("let ") {
        if let Some((combined, next)) = collect_multiline_let_match(lines, *index, path)? {
            let stmt = parse_let_stmt(&combined, path, line_no)?;
            *index = next;
            return Ok(stmt);
        }
        let stmt = parse_let_stmt(rest, path, line_no)?;
        *index += 1;
        return Ok(stmt);
    }
    if !trimmed.starts_with("print ")
        && !trimmed.starts_with("panic ")
        && !trimmed.starts_with("return ")
        && let Some(equals) = find_top_level_char(trimmed, '=')
    {
        let target_raw = trimmed[..equals].trim();
        if !target_raw.starts_with('*') && !target_raw.contains('[') {
            return Err(Diagnostic::new(
                "parse",
                if in_block {
                    "stage1 bootstrap currently supports let, print, panic, defer, if/else, while, match, and return statements inside blocks"
                } else {
                    "stage1 bootstrap currently supports top-level import, const, static, type, struct, enum, fn, let, print, panic, defer, if/else, while, and match statements"
                },
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
        }
        let expr_raw = trimmed[equals + 1..].trim();
        if target_raw.is_empty() || expr_raw.is_empty() {
            return Err(
                Diagnostic::new("parse", "assignment must use `target = value` syntax")
                    .with_path(path.display().to_string())
                    .with_span(line_no, 1),
            );
        }
        let target = parse_expr(target_raw, path, line_no, 1)?;
        let expr = parse_expr(expr_raw, path, line_no, equals + 2)?;
        *index += 1;
        return Ok(Stmt::Assign {
            target,
            expr,
            line: line_no,
            column: 1,
        });
    }
    if let Some(rest) = trimmed.strip_prefix("print ") {
        let expr = parse_expr(rest, path, line_no, 7)?;
        *index += 1;
        return Ok(Stmt::Print {
            expr,
            line: line_no,
            column: 1,
        });
    }
    if trimmed == "panic"
        || trimmed.starts_with("panic(")
        || trimmed.starts_with("panic<")
        || trimmed
            .strip_prefix("panic")
            .is_some_and(|rest| rest.starts_with(char::is_whitespace))
    {
        let expr = parse_expr(trimmed, path, line_no, 1)?;
        *index += 1;
        return Ok(Stmt::Panic {
            expr,
            line: line_no,
            column: 1,
        });
    }
    if let Some(rest) = trimmed.strip_prefix("defer ") {
        let expr = parse_expr(rest, path, line_no, 7)?;
        *index += 1;
        return Ok(Stmt::Defer {
            expr,
            line: line_no,
            column: 1,
        });
    }
    if let Some(rest) = trimmed.strip_prefix("return ") {
        let expr = parse_expr(rest, path, line_no, 8)?;
        *index += 1;
        return Ok(Stmt::Return {
            expr,
            line: line_no,
            column: 1,
        });
    }
    if let Some(equals) = find_top_level_char(trimmed, '=') {
        let target_raw = trimmed[..equals].trim();
        let expr_raw = trimmed[equals + 1..].trim();
        if target_raw.is_empty() || expr_raw.is_empty() {
            return Err(
                Diagnostic::new("parse", "assignment must use `target = value` syntax")
                    .with_path(path.display().to_string())
                    .with_span(line_no, 1),
            );
        }
        let target = parse_expr(target_raw, path, line_no, 1)?;
        let expr = parse_expr(expr_raw, path, line_no, equals + 2)?;
        *index += 1;
        return Ok(Stmt::Assign {
            target,
            expr,
            line: line_no,
            column: 1,
        });
    }
    let message = if in_block {
        "stage1 bootstrap currently supports let, print, panic, defer, if/else, while, match, and return statements inside blocks"
    } else {
        "stage1 bootstrap currently supports top-level import, const, static, type, struct, enum, fn, let, print, panic, defer, if/else, while, and match statements"
    };
    Err(Diagnostic::new("parse", message)
        .with_path(path.display().to_string())
        .with_span(line_no, 1))
}

fn parse_function(lines: &[&str], index: &mut usize, path: &Path) -> Result<Function, Diagnostic> {
    parse_function_in_context(lines, index, path, None)
}

fn parse_function_in_context(
    lines: &[&str],
    index: &mut usize,
    path: &Path,
    impl_target: Option<&str>,
) -> Result<Function, Diagnostic> {
    let line_no = *index + 1;
    let trimmed = lines[*index].trim();
    let (visibility, rest, visibility_column) = parse_visibility_prefix(trimmed);
    let (is_const, rest, const_column) = if let Some(rest) = rest.strip_prefix("const ") {
        (true, rest, visibility_column + 6)
    } else {
        (false, rest, visibility_column)
    };
    let (is_async, is_extern, header, fn_column) =
        if let Some(rest) = rest.strip_prefix("async fn ") {
            if is_const {
                return Err(Diagnostic::new("parse", "const functions cannot be async")
                    .with_path(path.display().to_string())
                    .with_span(line_no, const_column));
            }
            (true, false, rest, visibility_column + 6)
        } else if let Some(rest) = rest.strip_prefix("extern fn ") {
            if is_const {
                return Err(Diagnostic::new("parse", "const functions cannot be extern")
                    .with_path(path.display().to_string())
                    .with_span(line_no, const_column));
            }
            (false, true, rest, visibility_column + 7)
        } else {
            let rest = rest.strip_prefix("fn ").ok_or_else(|| {
                Diagnostic::new("parse", "invalid function declaration")
                    .with_path(path.display().to_string())
                    .with_span(line_no, 1)
            })?;
            (false, false, rest, const_column)
        };
    let open_paren = find_top_level_char(header, '(').ok_or_else(|| {
        Diagnostic::new("parse", "function declaration is missing '('")
            .with_path(path.display().to_string())
            .with_span(line_no, 1)
    })?;
    let close_paren = find_matching_paren(header, open_paren).ok_or_else(|| {
        Diagnostic::new("parse", "function declaration is missing ')'")
            .with_path(path.display().to_string())
            .with_span(line_no, 1)
    })?;
    let name_text = header[..open_paren].trim();
    let (name, type_params) = parse_function_name(name_text, path, line_no, fn_column + 3)?;
    let (receiver, params) = parse_params(
        &header[open_paren + 1..close_paren],
        path,
        line_no,
        impl_target.is_some(),
    )?;
    let after_paren = header[close_paren + 1..].trim();
    let after_colon = after_paren.strip_prefix(':').ok_or_else(|| {
        Diagnostic::new("parse", "function declaration must include a return type")
            .with_path(path.display().to_string())
            .with_span(line_no, close_paren + 2)
    })?;
    if is_extern {
        let (return_text, extern_library) = after_colon.rsplit_once(" from ").ok_or_else(|| {
            Diagnostic::new(
                "parse",
                "extern function declaration must use `extern fn name(args): type from \"lib\"` syntax",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1)
        })?;
        let return_ty = parse_type_name(return_text.trim(), path, line_no, 1)?;
        let extern_library =
            serde_json::from_str::<String>(extern_library.trim()).map_err(|_| {
                Diagnostic::new("parse", "extern function library must be a quoted string")
                    .with_path(path.display().to_string())
                    .with_span(line_no, 1)
            })?;
        *index += 1;
        return Ok(Function {
            name: name.to_string(),
            source_name: name.to_string(),
            path: path.display().to_string(),
            type_params,
            params,
            return_ty,
            body: Vec::new(),
            is_const,
            is_async,
            is_extern,
            extern_abi: Some(String::from("C")),
            extern_library: Some(extern_library),
            visibility,
            receiver,
            impl_target: impl_target.map(str::to_string),
            line: line_no,
            column: 1,
        });
    }
    let return_text = after_colon
        .strip_suffix('{')
        .map(str::trim)
        .ok_or_else(|| {
            Diagnostic::new(
                "parse",
                "function declaration must use `fn name(args): type {` syntax",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1)
        })?;
    let return_ty = parse_type_name(return_text, path, line_no, 1)?;
    *index += 1;
    let body = parse_stmt_list(lines, index, path)?;
    Ok(Function {
        name: name.to_string(),
        source_name: name.to_string(),
        path: path.display().to_string(),
        type_params,
        params,
        return_ty,
        body,
        is_const,
        is_async,
        is_extern,
        extern_abi: None,
        extern_library: None,
        visibility,
        receiver,
        impl_target: impl_target.map(str::to_string),
        line: line_no,
        column: 1,
    })
}

fn parse_type_alias(
    trimmed: &str,
    path: &Path,
    line_no: usize,
) -> Result<TypeAliasDecl, Diagnostic> {
    let (visibility, rest, visibility_column) = parse_visibility_prefix(trimmed);
    let header = if let Some(rest) = rest.strip_prefix("type ") {
        rest
    } else {
        let _ = rest.strip_prefix("type ").ok_or_else(|| {
            Diagnostic::new("parse", "invalid type alias declaration")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
        unreachable!()
    };
    let equals = find_top_level_char(header, '=').ok_or_else(|| {
        Diagnostic::new("parse", "type alias declaration is missing '='")
            .with_path(path.display().to_string())
            .with_span(line_no, 1)
    })?;
    let name = header[..equals].trim();
    validate_ident(name, path, line_no, visibility_column + 5)?;
    let target = header[equals + 1..].trim();
    if target.is_empty() {
        return Err(
            Diagnostic::new("parse", "type alias is missing a target type")
                .with_path(path.display().to_string())
                .with_span(line_no, equals + 2),
        );
    }
    let ty = parse_type_name(target, path, line_no, equals + 2)?;
    Ok(TypeAliasDecl {
        name: name.to_string(),
        ty,
        visibility,
        line: line_no,
        column: 1,
    })
}

fn parse_const_or_static_decl(
    trimmed: &str,
    path: &Path,
    line_no: usize,
) -> Result<ConstDecl, Diagnostic> {
    let (visibility, rest, visibility_column) = parse_visibility_prefix(trimmed);
    let (keyword, header) = if let Some(rest) = rest.strip_prefix("const ") {
        ("const", rest)
    } else if let Some(rest) = rest.strip_prefix("static ") {
        ("static", rest)
    } else {
        return Err(Diagnostic::new("parse", "invalid const/static declaration")
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
    };
    let column = visibility_column + keyword.len() + 1;
    let colon = find_top_level_char(header, ':').ok_or_else(|| {
        Diagnostic::new("parse", "const/static declaration is missing ':'")
            .with_path(path.display().to_string())
            .with_span(line_no, column)
    })?;
    let equals = find_top_level_char(header, '=').ok_or_else(|| {
        Diagnostic::new("parse", "const/static declaration is missing '='")
            .with_path(path.display().to_string())
            .with_span(line_no, column)
    })?;
    if equals <= colon {
        return Err(Diagnostic::new(
            "parse",
            "const/static declaration must use `const NAME: Type = expr` or `static NAME: Type = expr` syntax",
        )
        .with_path(path.display().to_string())
        .with_span(line_no, column));
    }
    let name = header[..colon].trim();
    validate_ident(name, path, line_no, column)?;
    let ty_text = header[colon + 1..equals].trim();
    if ty_text.is_empty() {
        return Err(
            Diagnostic::new("parse", "const/static declaration is missing a type")
                .with_path(path.display().to_string())
                .with_span(line_no, column + colon + 1),
        );
    }
    let expr_text = header[equals + 1..].trim();
    if expr_text.is_empty() {
        return Err(Diagnostic::new(
            "parse",
            "const/static declaration is missing an initializer",
        )
        .with_path(path.display().to_string())
        .with_span(line_no, column + equals + 1));
    }
    Ok(ConstDecl {
        name: name.to_string(),
        ty: parse_type_name(ty_text, path, line_no, column + colon + 2)?,
        expr: parse_expr(expr_text, path, line_no, column + equals + 2)?,
        is_static: keyword == "static",
        visibility,
        line: line_no,
        column: 1,
    })
}

fn parse_axiom_decl(
    lines: &[&str],
    index: &mut usize,
    path: &Path,
) -> Result<AxiomDecl, Diagnostic> {
    let decl_line = *index + 1;
    let name = semantic_block_name(lines[*index].trim(), "axiom", path, decl_line)?;
    *index += 1;
    let mut scope = None;
    let mut severity = None;
    let mut description = None;
    let mut assertion = None;
    while *index < lines.len() {
        let line_no = *index + 1;
        let trimmed = lines[*index].trim();
        if trimmed.is_empty() {
            *index += 1;
            continue;
        }
        if trimmed == "}" {
            *index += 1;
            return Ok(AxiomDecl {
                name,
                scope,
                severity,
                description,
                assertion,
                line: decl_line,
                column: 1,
            });
        }
        if let Some(value) = trimmed.strip_prefix("scope ") {
            let value = value.trim();
            validate_ident(value, path, line_no, 7)?;
            scope = Some(value.to_string());
        } else if let Some(value) = trimmed.strip_prefix("severity ") {
            let value = value.trim();
            validate_ident(value, path, line_no, 10)?;
            severity = Some(value.to_string());
        } else if let Some(value) = trimmed.strip_prefix("description ") {
            description = Some(parse_semantic_string(value.trim(), path, line_no, 13)?);
        } else if let Some(value) = trimmed.strip_prefix("assert ") {
            let value = value.trim();
            if value.is_empty() {
                return Err(Diagnostic::new("parse", "axiom assert clause is empty")
                    .with_path(path.display().to_string())
                    .with_span(line_no, 8));
            }
            assertion = Some(value.to_string());
        } else {
            return Err(Diagnostic::new(
                "parse",
                "axiom declarations support scope, severity, description, and assert clauses",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
        }
        *index += 1;
    }
    Err(Diagnostic::new("parse", "missing closing brace for axiom")
        .with_path(path.display().to_string())
        .with_span(decl_line, 1))
}

fn parse_semantic_capability_decl(
    lines: &[&str],
    index: &mut usize,
    path: &Path,
) -> Result<SemanticCapabilityDecl, Diagnostic> {
    let decl_line = *index + 1;
    let name = semantic_block_name(lines[*index].trim(), "capability", path, decl_line)?;
    *index += 1;
    let mut inputs = Vec::new();
    let mut effects = Vec::new();
    let mut preserves = Vec::new();
    let mut evidence = Vec::new();
    while *index < lines.len() {
        let line_no = *index + 1;
        let trimmed = lines[*index].trim();
        if trimmed.is_empty() {
            *index += 1;
            continue;
        }
        if trimmed == "}" {
            *index += 1;
            return Ok(SemanticCapabilityDecl {
                name,
                inputs,
                effects,
                preserves,
                evidence,
                line: decl_line,
                column: 1,
            });
        }
        if let Some(value) = trimmed.strip_prefix("input ") {
            inputs.push(parse_capability_input(value.trim(), path, line_no)?);
            *index += 1;
            continue;
        }
        if trimmed == "effects {" {
            *index += 1;
            effects.extend(parse_capability_effects(lines, index, path)?);
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("preserves ") {
            preserves.push(parse_semantic_reference(value.trim(), path, line_no, 11)?);
            *index += 1;
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("requires evidence ") {
            evidence.push(parse_semantic_reference(value.trim(), path, line_no, 18)?);
            *index += 1;
            continue;
        }
        return Err(Diagnostic::new(
            "parse",
            "capability declarations support input, effects, preserves, and requires evidence clauses",
        )
        .with_path(path.display().to_string())
        .with_span(line_no, 1));
    }
    Err(
        Diagnostic::new("parse", "missing closing brace for capability")
            .with_path(path.display().to_string())
            .with_span(decl_line, 1),
    )
}

fn parse_evidence_decl(
    lines: &[&str],
    index: &mut usize,
    path: &Path,
) -> Result<EvidenceDecl, Diagnostic> {
    let decl_line = *index + 1;
    let name = semantic_block_name(lines[*index].trim(), "evidence", path, decl_line)?;
    *index += 1;
    let mut description = None;
    while *index < lines.len() {
        let line_no = *index + 1;
        let trimmed = lines[*index].trim();
        if trimmed.is_empty() {
            *index += 1;
            continue;
        }
        if trimmed == "}" {
            *index += 1;
            return Ok(EvidenceDecl {
                name,
                description,
                line: decl_line,
                column: 1,
            });
        }
        if let Some(value) = trimmed.strip_prefix("description ") {
            description = Some(parse_semantic_string(value.trim(), path, line_no, 13)?);
            *index += 1;
            continue;
        }
        return Err(Diagnostic::new(
            "parse",
            "evidence declarations support only description clauses in v0",
        )
        .with_path(path.display().to_string())
        .with_span(line_no, 1));
    }
    Err(
        Diagnostic::new("parse", "missing closing brace for evidence")
            .with_path(path.display().to_string())
            .with_span(decl_line, 1),
    )
}

fn semantic_block_name(
    trimmed: &str,
    keyword: &'static str,
    path: &Path,
    line_no: usize,
) -> Result<String, Diagnostic> {
    let prefix = format!("{keyword} ");
    let name = trimmed
        .strip_prefix(&prefix)
        .and_then(|rest| rest.strip_suffix('{'))
        .map(str::trim)
        .ok_or_else(|| {
            Diagnostic::new(
                "parse",
                format!("{keyword} declaration must use `{keyword} Name {{` syntax"),
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1)
        })?;
    validate_ident(name, path, line_no, keyword.len() + 2)?;
    Ok(name.to_string())
}

fn parse_capability_input(
    raw: &str,
    path: &Path,
    line_no: usize,
) -> Result<CapabilityInput, Diagnostic> {
    let colon = find_top_level_char(raw, ':').ok_or_else(|| {
        Diagnostic::new("parse", "capability input is missing ':'")
            .with_path(path.display().to_string())
            .with_span(line_no, 7)
    })?;
    let name = raw[..colon].trim();
    validate_ident(name, path, line_no, 7)?;
    Ok(CapabilityInput {
        name: name.to_string(),
        ty: parse_type_name(raw[colon + 1..].trim(), path, line_no, colon + 8)?,
        line: line_no,
        column: 1,
    })
}

fn parse_capability_effects(
    lines: &[&str],
    index: &mut usize,
    path: &Path,
) -> Result<Vec<CapabilityEffect>, Diagnostic> {
    let mut effects = Vec::new();
    while *index < lines.len() {
        let line_no = *index + 1;
        let trimmed = lines[*index].trim();
        if trimmed.is_empty() {
            *index += 1;
            continue;
        }
        if trimmed == "}" {
            *index += 1;
            return Ok(effects);
        }
        let Some((kind, target)) = trimmed.split_once(' ') else {
            return Err(Diagnostic::new(
                "parse",
                "capability effect must use `read Target`, `write Target`, or `emit Target`",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
        };
        if !matches!(kind, "read" | "write" | "emit") {
            return Err(Diagnostic::new(
                "parse",
                "capability effect kind must be read, write, or emit",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, 1));
        }
        let target = target.trim();
        validate_ident(target, path, line_no, kind.len() + 2)?;
        effects.push(CapabilityEffect {
            kind: kind.to_string(),
            target: target.to_string(),
            line: line_no,
            column: 1,
        });
        *index += 1;
    }
    Err(
        Diagnostic::new("parse", "missing closing brace for capability effects")
            .with_path(path.display().to_string())
            .with_span(lines.len().max(1), 1),
    )
}

fn parse_semantic_reference(
    value: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<SemanticReference, Diagnostic> {
    validate_ident(value, path, line_no, column)?;
    Ok(SemanticReference {
        name: value.to_string(),
        line: line_no,
        column,
    })
}

fn parse_semantic_string(
    value: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<String, Diagnostic> {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .map(str::to_string)
        .ok_or_else(|| {
            Diagnostic::new("parse", "semantic description must be a quoted string")
                .with_path(path.display().to_string())
                .with_span(line_no, column)
        })
}

fn parse_struct(lines: &[&str], index: &mut usize, path: &Path) -> Result<StructDecl, Diagnostic> {
    let line_no = *index + 1;
    let trimmed = lines[*index].trim();
    let (visibility, rest, visibility_column) = parse_visibility_prefix(trimmed);
    let header = if let Some(rest) = rest.strip_prefix("struct ") {
        rest
    } else {
        let _ = rest.strip_prefix("struct ").ok_or_else(|| {
            Diagnostic::new("parse", "invalid struct declaration")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
        unreachable!()
    };
    let name_text = header.strip_suffix('{').map(str::trim).ok_or_else(|| {
        Diagnostic::new(
            "parse",
            "struct declaration must use `struct Name {` syntax",
        )
        .with_path(path.display().to_string())
        .with_span(line_no, 1)
    })?;
    let (name, type_params) =
        parse_decl_name(name_text, "struct", path, line_no, visibility_column + 7)?;
    *index += 1;
    let fields = parse_struct_fields(lines, index, path)?;
    Ok(StructDecl {
        name: name.to_string(),
        type_params,
        fields,
        visibility,
        line: line_no,
        column: 1,
    })
}

fn parse_struct_fields(
    lines: &[&str],
    index: &mut usize,
    path: &Path,
) -> Result<Vec<StructField>, Diagnostic> {
    let mut fields = Vec::new();
    while *index < lines.len() {
        let line_no = *index + 1;
        let trimmed = lines[*index].trim();
        if trimmed.is_empty() {
            *index += 1;
            continue;
        }
        if trimmed == "}" {
            *index += 1;
            return Ok(fields);
        }
        let colon = find_top_level_char(trimmed, ':').ok_or_else(|| {
            Diagnostic::new("parse", "struct field is missing ':'")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
        let name = trimmed[..colon].trim();
        validate_ident(name, path, line_no, 1)?;
        let ty = parse_type_name(trimmed[colon + 1..].trim(), path, line_no, colon + 2)?;
        fields.push(StructField {
            name: name.to_string(),
            ty,
            line: line_no,
            column: 1,
        });
        *index += 1;
    }
    Err(Diagnostic::new("parse", "missing closing brace for struct")
        .with_path(path.display().to_string())
        .with_span(lines.len().max(1), 1))
}

fn parse_impl(lines: &[&str], index: &mut usize, path: &Path) -> Result<Vec<Function>, Diagnostic> {
    let line_no = *index + 1;
    let trimmed = lines[*index].trim();
    let target = trimmed
        .strip_prefix("impl ")
        .and_then(|rest| rest.strip_suffix('{'))
        .map(str::trim)
        .ok_or_else(|| {
            Diagnostic::new("parse", "impl block must use `impl Type {` syntax")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
    validate_ident(target, path, line_no, 6)?;
    *index += 1;
    let mut methods = Vec::new();
    while *index < lines.len() {
        skip_blank_lines(lines, index);
        if *index >= lines.len() {
            break;
        }
        let trimmed = lines[*index].trim();
        if trimmed == "}" {
            *index += 1;
            return Ok(methods);
        }
        if trimmed.starts_with("fn ")
            || trimmed.starts_with("const fn ")
            || trimmed.starts_with("async fn ")
            || trimmed.starts_with("extern fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub const fn ")
            || trimmed.starts_with("pub async fn ")
            || trimmed.starts_with("pub extern fn ")
            || trimmed.starts_with("pub(pkg) fn ")
            || trimmed.starts_with("pub(pkg) const fn ")
            || trimmed.starts_with("pub(pkg) async fn ")
            || trimmed.starts_with("pub(pkg) extern fn ")
        {
            methods.push(parse_function_in_context(lines, index, path, Some(target))?);
            continue;
        }
        return Err(Diagnostic::new(
            "parse",
            "impl blocks may only contain function declarations",
        )
        .with_path(path.display().to_string())
        .with_span(*index + 1, 1));
    }
    Err(
        Diagnostic::new("parse", "impl block is missing closing '}'")
            .with_path(path.display().to_string())
            .with_span(line_no, 1),
    )
}

fn parse_trait(lines: &[&str], index: &mut usize, path: &Path) -> Result<TraitDecl, Diagnostic> {
    let line_no = *index + 1;
    let trimmed = lines[*index].trim();
    let (visibility, rest, visibility_column) = parse_visibility_prefix(trimmed);
    let header = rest.strip_prefix("trait ").ok_or_else(|| {
        Diagnostic::new("parse", "invalid trait declaration")
            .with_path(path.display().to_string())
            .with_span(line_no, 1)
    })?;
    let name = header.strip_suffix('{').map(str::trim).ok_or_else(|| {
        Diagnostic::new("parse", "trait declaration must use `trait Name {` syntax")
            .with_path(path.display().to_string())
            .with_span(line_no, 1)
    })?;
    validate_ident(name, path, line_no, visibility_column + 7)?;
    *index += 1;
    let methods = parse_trait_methods(lines, index, path)?;
    Ok(TraitDecl {
        name: name.to_string(),
        methods,
        visibility,
        line: line_no,
        column: 1,
    })
}

fn parse_trait_methods(
    lines: &[&str],
    index: &mut usize,
    path: &Path,
) -> Result<Vec<TraitMethodDecl>, Diagnostic> {
    let mut methods = Vec::new();
    while *index < lines.len() {
        let line_no = *index + 1;
        let trimmed = lines[*index].trim();
        if trimmed.is_empty() {
            *index += 1;
            continue;
        }
        if trimmed == "}" {
            *index += 1;
            return Ok(methods);
        }
        let header = trimmed
            .strip_prefix("fn ")
            .and_then(|raw| raw.strip_suffix(';').or(Some(raw)))
            .ok_or_else(|| {
                Diagnostic::new(
                    "parse",
                    "trait declarations may only contain method signatures",
                )
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
            })?;
        let open_paren = find_top_level_char(header, '(').ok_or_else(|| {
            Diagnostic::new("parse", "trait method declaration is missing '('")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
        let close_paren = find_matching_paren(header, open_paren).ok_or_else(|| {
            Diagnostic::new("parse", "trait method declaration is missing ')'")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
        let name = header[..open_paren].trim();
        validate_ident(name, path, line_no, 4)?;
        let (receiver, params) =
            parse_params(&header[open_paren + 1..close_paren], path, line_no, true)?;
        let after_paren = header[close_paren + 1..].trim();
        let return_text = after_paren
            .strip_prefix(':')
            .map(str::trim)
            .ok_or_else(|| {
                Diagnostic::new(
                    "parse",
                    "trait method declaration must include a return type",
                )
                .with_path(path.display().to_string())
                .with_span(line_no, close_paren + 2)
            })?;
        methods.push(TraitMethodDecl {
            name: name.to_string(),
            params,
            return_ty: parse_type_name(return_text, path, line_no, close_paren + 3)?,
            has_self: receiver.is_some(),
            line: line_no,
            column: 1,
        });
        *index += 1;
    }
    Err(
        Diagnostic::new("parse", "trait declaration is missing closing '}'")
            .with_path(path.display().to_string())
            .with_span(lines.len().max(1), 1),
    )
}

fn parse_enum(lines: &[&str], index: &mut usize, path: &Path) -> Result<EnumDecl, Diagnostic> {
    let line_no = *index + 1;
    let trimmed = lines[*index].trim();
    let (visibility, rest, visibility_column) = parse_visibility_prefix(trimmed);
    let header = if let Some(rest) = rest.strip_prefix("enum ") {
        rest
    } else {
        let _ = rest.strip_prefix("enum ").ok_or_else(|| {
            Diagnostic::new("parse", "invalid enum declaration")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
        unreachable!()
    };
    let name_text = header.strip_suffix('{').map(str::trim).ok_or_else(|| {
        Diagnostic::new("parse", "enum declaration must use `enum Name {` syntax")
            .with_path(path.display().to_string())
            .with_span(line_no, 1)
    })?;
    let (name, type_params) =
        parse_decl_name(name_text, "enum", path, line_no, visibility_column + 5)?;
    *index += 1;
    let variants = parse_enum_variants(lines, index, path)?;
    Ok(EnumDecl {
        name: name.to_string(),
        type_params,
        variants,
        visibility,
        line: line_no,
        column: 1,
    })
}

fn parse_visibility_prefix(trimmed: &str) -> (Visibility, &str, usize) {
    if let Some(rest) = trimmed.strip_prefix("pub(pkg) ") {
        (Visibility::Package, rest, 10)
    } else if let Some(rest) = trimmed.strip_prefix("pub ") {
        (Visibility::Public, rest, 5)
    } else {
        (Visibility::Module, trimmed, 1)
    }
}

fn parse_enum_variants(
    lines: &[&str],
    index: &mut usize,
    path: &Path,
) -> Result<Vec<EnumVariantDecl>, Diagnostic> {
    let mut variants = Vec::new();
    while *index < lines.len() {
        let line_no = *index + 1;
        let trimmed = lines[*index].trim();
        if trimmed.is_empty() {
            *index += 1;
            continue;
        }
        if trimmed == "}" {
            *index += 1;
            return Ok(variants);
        }
        let (name, payload_tys, payload_names) = if trimmed.ends_with('}')
            && let Some(open_brace) = find_top_level_char(trimmed, '{')
            && matches!(find_matching_brace(trimmed, open_brace), Some(close) if close == trimmed.len() - 1)
        {
            let name = trimmed[..open_brace].trim();
            validate_ident(name, path, line_no, 1)?;
            let payload_raw = trimmed[open_brace + 1..trimmed.len() - 1].trim();
            let fields = parse_named_enum_payload_fields(
                payload_raw,
                path,
                line_no,
                open_brace + 2,
                "enum variant payload field",
            )?;
            (
                name.to_string(),
                fields.iter().map(|field| field.1.clone()).collect(),
                fields.into_iter().map(|field| field.0).collect(),
            )
        } else if trimmed.ends_with(')')
            && let Some(open_paren) = find_top_level_char(trimmed, '(')
            && matches!(find_matching_paren(trimmed, open_paren), Some(close) if close == trimmed.len() - 1)
        {
            let name = trimmed[..open_paren].trim();
            validate_ident(name, path, line_no, 1)?;
            let payload_raw = trimmed[open_paren + 1..trimmed.len() - 1].trim();
            if payload_raw.is_empty() {
                return Err(
                    Diagnostic::new("parse", "enum variant payload type is empty")
                        .with_path(path.display().to_string())
                        .with_span(line_no, open_paren + 2),
                );
            }
            let payload_tys = split_top_level_type(payload_raw, ',')
                .into_iter()
                .map(|ty| {
                    let ty = ty.trim();
                    if ty.is_empty() {
                        return Err(
                            Diagnostic::new("parse", "enum variant payload type is empty")
                                .with_path(path.display().to_string())
                                .with_span(line_no, open_paren + 2),
                        );
                    }
                    parse_type_name(ty, path, line_no, open_paren + 2)
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            (name.to_string(), payload_tys, Vec::new())
        } else {
            validate_ident(trimmed, path, line_no, 1)?;
            (trimmed.to_string(), Vec::new(), Vec::new())
        };
        variants.push(EnumVariantDecl {
            name,
            payload_tys,
            payload_names,
            line: line_no,
            column: 1,
        });
        *index += 1;
    }
    Err(Diagnostic::new("parse", "missing closing brace for enum")
        .with_path(path.display().to_string())
        .with_span(lines.len().max(1), 1))
}

fn parse_params(
    raw: &str,
    path: &Path,
    line_no: usize,
    allow_self: bool,
) -> Result<(Option<ReceiverKind>, Vec<Param>), Diagnostic> {
    if raw.trim().is_empty() {
        return Ok((None, Vec::new()));
    }
    let mut receiver = None;
    let mut params = Vec::new();
    for (index, param_text) in split_top_level_type(raw, ',').into_iter().enumerate() {
        let param_text = param_text.trim();
        if param_text == "self" {
            if !allow_self {
                return Err(Diagnostic::new(
                    "parse",
                    "self parameter is only allowed inside impl methods",
                )
                .with_path(path.display().to_string())
                .with_span(line_no, 1));
            }
            if index != 0 {
                return Err(Diagnostic::new(
                    "parse",
                    "self parameter must be the first parameter in an impl method",
                )
                .with_path(path.display().to_string())
                .with_span(line_no, 1));
            }
            receiver = Some(ReceiverKind::Value);
            continue;
        }
        let colon = find_top_level_char(param_text, ':').ok_or_else(|| {
            Diagnostic::new("parse", "function parameter is missing ':'")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
        let name = param_text[..colon].trim();
        validate_ident(name, path, line_no, 1)?;
        let ty = parse_type_name(param_text[colon + 1..].trim(), path, line_no, 1)?;
        params.push(Param {
            name: name.to_string(),
            ty,
            line: line_no,
            column: 1,
        });
    }
    Ok((receiver, params))
}

fn parse_if_stmt(lines: &[&str], index: &mut usize, path: &Path) -> Result<Stmt, Diagnostic> {
    let line_no = *index + 1;
    let trimmed = lines[*index].trim();
    let cond_raw = trimmed
        .strip_prefix("if ")
        .and_then(|raw| raw.strip_suffix('{'))
        .map(str::trim)
        .ok_or_else(|| {
            Diagnostic::new("parse", "if statement must use `if <expr> {` syntax")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
    if let Some(pattern_raw) = cond_raw.strip_prefix("let ") {
        return parse_if_let_stmt(lines, index, path, line_no, pattern_raw);
    }
    let cond = parse_expr(cond_raw, path, line_no, 4)?;
    *index += 1;
    let then_block = parse_stmt_list(lines, index, path)?;
    skip_blank_lines(lines, index);
    let else_block = if *index < lines.len() {
        match lines[*index].trim() {
            "} else {" => {
                *index += 1;
                Some(parse_stmt_list(lines, index, path)?)
            }
            "else {" => {
                *index += 1;
                Some(parse_stmt_list(lines, index, path)?)
            }
            _ => None,
        }
    } else {
        None
    };
    Ok(Stmt::If {
        cond,
        then_block,
        else_block,
        line: line_no,
        column: 1,
    })
}

fn parse_if_let_stmt(
    lines: &[&str],
    index: &mut usize,
    path: &Path,
    line_no: usize,
    pattern_raw: &str,
) -> Result<Stmt, Diagnostic> {
    let equals = find_top_level_char(pattern_raw, '=').ok_or_else(|| {
        Diagnostic::new(
            "parse",
            "if let statement must use `if let <Variant>(...) = <expr> {` syntax",
        )
        .with_path(path.display().to_string())
        .with_span(line_no, 4)
    })?;
    let pattern = pattern_raw[..equals].trim();
    let expr_raw = pattern_raw[equals + 1..].trim();
    if pattern.is_empty() || expr_raw.is_empty() {
        return Err(Diagnostic::new(
            "parse",
            "if let statement must include both a pattern and an expression",
        )
        .with_path(path.display().to_string())
        .with_span(line_no, 4));
    }
    let (variant, bindings, is_named) = parse_match_pattern(pattern, path, line_no)?;
    let expr = parse_expr(expr_raw, path, line_no, 4 + equals + 1)?;
    *index += 1;
    let then_block = parse_stmt_list(lines, index, path)?;
    skip_blank_lines(lines, index);
    let else_block = if *index < lines.len() {
        match lines[*index].trim() {
            "} else {" => {
                *index += 1;
                Some(parse_stmt_list(lines, index, path)?)
            }
            "else {" => {
                *index += 1;
                Some(parse_stmt_list(lines, index, path)?)
            }
            _ => None,
        }
    } else {
        None
    };
    Ok(Stmt::IfLet {
        variant,
        bindings,
        is_named,
        expr,
        then_block,
        else_block,
        line: line_no,
        column: 1,
    })
}

fn parse_match_pattern(
    pattern: &str,
    path: &Path,
    line_no: usize,
) -> Result<(String, Vec<String>, bool), Diagnostic> {
    if pattern.starts_with('(') {
        return Err(
            Diagnostic::new("parse", "nested match patterns are not supported yet")
                .with_path(path.display().to_string())
                .with_span(line_no, 1),
        );
    }
    if pattern.ends_with('}')
        && let Some(open_brace) = find_top_level_char(pattern, '{')
        && matches!(find_matching_brace(pattern, open_brace), Some(close) if close == pattern.len() - 1)
    {
        let name = pattern[..open_brace].trim();
        validate_ident(name, path, line_no, 1)?;
        let bindings_raw = &pattern[open_brace + 1..pattern.len() - 1];
        let bindings = parse_match_bindings(bindings_raw, open_brace, path, line_no)?;
        Ok((name.to_string(), bindings, true))
    } else if pattern.ends_with(')')
        && let Some(open_paren) = find_top_level_char(pattern, '(')
        && matches!(find_matching_paren(pattern, open_paren), Some(close) if close == pattern.len() - 1)
    {
        let name = pattern[..open_paren].trim();
        validate_ident(name, path, line_no, 1)?;
        let bindings_raw = &pattern[open_paren + 1..pattern.len() - 1];
        let bindings = parse_match_bindings(bindings_raw, open_paren, path, line_no)?;
        Ok((name.to_string(), bindings, false))
    } else {
        validate_ident(pattern, path, line_no, 1)?;
        Ok((pattern.to_string(), Vec::new(), false))
    }
}

fn parse_match_bindings(
    bindings_raw: &str,
    open_delim: usize,
    path: &Path,
    line_no: usize,
) -> Result<Vec<String>, Diagnostic> {
    if bindings_raw.trim().is_empty() {
        return Err(Diagnostic::new("parse", "match arm binding is empty")
            .with_path(path.display().to_string())
            .with_span(line_no, open_delim + 2));
    }
    split_top_level_with_offsets(bindings_raw, ',')
        .into_iter()
        .map(|(binding_offset, raw_binding)| {
            let binding = raw_binding.trim();
            let leading_ws = raw_binding
                .len()
                .saturating_sub(raw_binding.trim_start().len());
            let binding_column = open_delim + 2 + binding_offset + leading_ws;
            if binding.is_empty() {
                return Err(Diagnostic::new("parse", "match arm binding is empty")
                    .with_path(path.display().to_string())
                    .with_span(line_no, binding_column));
            }
            if let Some(nested_offset) = find_nested_match_pattern_offset(binding) {
                return Err(Diagnostic::new(
                    "parse",
                    "nested match patterns are not supported yet",
                )
                .with_path(path.display().to_string())
                .with_span(line_no, binding_column + nested_offset));
            }
            validate_ident(binding, path, line_no, binding_column)?;
            Ok(binding.to_string())
        })
        .collect()
}

fn parse_while_stmt(lines: &[&str], index: &mut usize, path: &Path) -> Result<Stmt, Diagnostic> {
    let line_no = *index + 1;
    let trimmed = lines[*index].trim();
    let cond_raw = trimmed
        .strip_prefix("while ")
        .and_then(|raw| raw.strip_suffix('{'))
        .map(str::trim)
        .ok_or_else(|| {
            Diagnostic::new("parse", "while statement must use `while <expr> {` syntax")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
    let cond = parse_expr(cond_raw, path, line_no, 7)?;
    *index += 1;
    let body = parse_stmt_list(lines, index, path)?;
    Ok(Stmt::While {
        cond,
        body,
        line: line_no,
        column: 1,
    })
}

fn parse_match_stmt(lines: &[&str], index: &mut usize, path: &Path) -> Result<Stmt, Diagnostic> {
    let line_no = *index + 1;
    let trimmed = lines[*index].trim();
    let expr_raw = trimmed
        .strip_prefix("match ")
        .and_then(|raw| raw.strip_suffix('{'))
        .map(str::trim)
        .ok_or_else(|| {
            Diagnostic::new("parse", "match statement must use `match <expr> {` syntax")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
    let expr = parse_expr(expr_raw, path, line_no, 7)?;
    *index += 1;
    let arms = parse_match_arms(lines, index, path)?;
    Ok(Stmt::Match {
        expr,
        arms,
        line: line_no,
        column: 1,
    })
}

fn parse_match_arms(
    lines: &[&str],
    index: &mut usize,
    path: &Path,
) -> Result<Vec<MatchArm>, Diagnostic> {
    let mut arms = Vec::new();
    while *index < lines.len() {
        let line_no = *index + 1;
        let trimmed = lines[*index].trim();
        if trimmed.is_empty() {
            *index += 1;
            continue;
        }
        if trimmed == "}" {
            *index += 1;
            return Ok(arms);
        }
        let variant = trimmed.strip_suffix('{').map(str::trim).ok_or_else(|| {
            Diagnostic::new("parse", "match arm must use `Variant {` syntax")
                .with_path(path.display().to_string())
                .with_span(line_no, 1)
        })?;
        if let Some(column) = find_top_level_keyword(variant, "if") {
            return Err(
                Diagnostic::new("parse", "match arm guards are not supported yet")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column + 1),
            );
        }
        if variant.starts_with('(') {
            return Err(
                Diagnostic::new("parse", "nested match patterns are not supported yet")
                    .with_path(path.display().to_string())
                    .with_span(line_no, 1),
            );
        }
        let (variant, bindings, is_named) = if variant.ends_with('}')
            && let Some(open_brace) = find_top_level_char(variant, '{')
            && matches!(find_matching_brace(variant, open_brace), Some(close) if close == variant.len() - 1)
        {
            let name = variant[..open_brace].trim();
            validate_ident(name, path, line_no, 1)?;
            let bindings_raw = &variant[open_brace + 1..variant.len() - 1];
            if bindings_raw.trim().is_empty() {
                return Err(Diagnostic::new("parse", "match arm binding is empty")
                    .with_path(path.display().to_string())
                    .with_span(line_no, open_brace + 2));
            }
            let bindings = split_top_level_with_offsets(bindings_raw, ',')
                .into_iter()
                .map(|(binding_offset, raw_binding)| {
                    let binding = raw_binding.trim();
                    let leading_ws = raw_binding
                        .len()
                        .saturating_sub(raw_binding.trim_start().len());
                    let binding_column = open_brace + 2 + binding_offset + leading_ws;
                    if binding.is_empty() {
                        return Err(Diagnostic::new("parse", "match arm binding is empty")
                            .with_path(path.display().to_string())
                            .with_span(line_no, binding_column));
                    }
                    if let Some(nested_offset) = find_nested_match_pattern_offset(binding) {
                        return Err(Diagnostic::new(
                            "parse",
                            "nested match patterns are not supported yet",
                        )
                        .with_path(path.display().to_string())
                        .with_span(line_no, binding_column + nested_offset));
                    }
                    validate_ident(binding, path, line_no, binding_column)?;
                    Ok(binding.to_string())
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            (name.to_string(), bindings, true)
        } else if variant.ends_with(')')
            && let Some(open_paren) = find_top_level_char(variant, '(')
            && matches!(find_matching_paren(variant, open_paren), Some(close) if close == variant.len() - 1)
        {
            let name = variant[..open_paren].trim();
            validate_ident(name, path, line_no, 1)?;
            let bindings_raw = &variant[open_paren + 1..variant.len() - 1];
            if bindings_raw.trim().is_empty() {
                return Err(Diagnostic::new("parse", "match arm binding is empty")
                    .with_path(path.display().to_string())
                    .with_span(line_no, open_paren + 2));
            }
            let bindings = split_top_level_with_offsets(bindings_raw, ',')
                .into_iter()
                .map(|(binding_offset, raw_binding)| {
                    let binding = raw_binding.trim();
                    let leading_ws = raw_binding
                        .len()
                        .saturating_sub(raw_binding.trim_start().len());
                    let binding_column = open_paren + 2 + binding_offset + leading_ws;
                    if binding.is_empty() {
                        return Err(Diagnostic::new("parse", "match arm binding is empty")
                            .with_path(path.display().to_string())
                            .with_span(line_no, binding_column));
                    }
                    if let Some(nested_offset) = find_nested_match_pattern_offset(binding) {
                        return Err(Diagnostic::new(
                            "parse",
                            "nested match patterns are not supported yet",
                        )
                        .with_path(path.display().to_string())
                        .with_span(line_no, binding_column + nested_offset));
                    }
                    validate_ident(binding, path, line_no, binding_column)?;
                    Ok(binding.to_string())
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            (name.to_string(), bindings, false)
        } else {
            validate_ident(variant, path, line_no, 1)?;
            (variant.to_string(), Vec::new(), false)
        };
        *index += 1;
        let body = parse_stmt_list(lines, index, path)?;
        arms.push(MatchArm {
            variant,
            bindings,
            is_named,
            body,
            line: line_no,
            column: 1,
        });
    }
    Err(Diagnostic::new("parse", "missing closing brace for match")
        .with_path(path.display().to_string())
        .with_span(lines.len().max(1), 1))
}

fn parse_match_expr(
    raw: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<Expr, Diagnostic> {
    let body_open = find_top_level_char(raw, '{').ok_or_else(|| {
        Diagnostic::new(
            "parse",
            "match expression must use `match <expr> { ... }` syntax",
        )
        .with_path(path.display().to_string())
        .with_span(line_no, column)
    })?;
    if !matches!(find_matching_brace(raw, body_open), Some(close) if close == raw.len() - 1) {
        return Err(
            Diagnostic::new("parse", "match expression body is incomplete")
                .with_path(path.display().to_string())
                .with_span(line_no, column),
        );
    }
    let expr_raw = raw["match ".len()..body_open].trim();
    if expr_raw.is_empty() {
        return Err(
            Diagnostic::new("parse", "match expression is missing a scrutinee")
                .with_path(path.display().to_string())
                .with_span(line_no, column),
        );
    }
    let inner = &raw[body_open + 1..raw.len() - 1];
    let mut arms = Vec::new();
    for (arm_offset, arm_raw) in split_match_expr_arms(inner) {
        let arm_raw = arm_raw.trim();
        if arm_raw.is_empty() {
            continue;
        }
        let arm_source_offset = body_open + 1 + arm_offset;
        let (arm_line, arm_column) =
            source_position_for_offset(line_no, column, raw, arm_source_offset);
        let Some(arrow) = find_top_level_arrow(arm_raw) else {
            return Err(Diagnostic::new(
                "parse",
                "match expression arm must use `Pattern => expr` syntax",
            )
            .with_path(path.display().to_string())
            .with_span(arm_line, arm_column));
        };
        let pattern = arm_raw[..arrow].trim();
        let expr_raw = &arm_raw[arrow + 2..];
        let expr_leading_ws = expr_raw.len().saturating_sub(expr_raw.trim_start().len());
        let expr_offset = arm_source_offset + arrow + 2 + expr_leading_ws;
        let (expr_line, expr_column) =
            source_position_for_offset(line_no, column, raw, expr_offset);
        let expr_text = expr_raw.trim();
        if expr_text.is_empty() {
            return Err(Diagnostic::new(
                "parse",
                "match expression arm is missing a result expression",
            )
            .with_path(path.display().to_string())
            .with_span(expr_line, expr_column));
        }
        let (variant, bindings, is_named) = parse_match_pattern(pattern, path, arm_line)?;
        arms.push(MatchExprArm {
            variant,
            bindings,
            is_named,
            expr: parse_expr(expr_text, path, expr_line, expr_column)?,
            line: arm_line,
            column: arm_column,
        });
    }
    if arms.is_empty() {
        let (body_line, body_column) =
            source_position_for_offset(line_no, column, raw, body_open + 1);
        return Err(
            Diagnostic::new("parse", "match expression must contain at least one arm")
                .with_path(path.display().to_string())
                .with_span(body_line, body_column),
        );
    }
    Ok(Expr::Match {
        expr: Box::new(parse_expr(
            expr_raw,
            path,
            line_no,
            column + "match ".len(),
        )?),
        arms,
        line: line_no,
        column,
    })
}

fn split_match_expr_arms(inner: &str) -> Vec<(usize, String)> {
    if !inner.contains('\n') {
        return split_top_level_with_offsets(inner, ',')
            .into_iter()
            .map(|(offset, raw)| (offset, raw.to_string()))
            .collect();
    }

    let mut arms = Vec::new();
    let mut offset = 0usize;
    for line in inner.split_inclusive('\n') {
        let raw_line = line.trim_end_matches('\n');
        let leading_ws = raw_line.len().saturating_sub(raw_line.trim_start().len());
        let arm = raw_line.trim().trim_end_matches(',').trim_end();
        if !arm.is_empty() {
            arms.push((offset + leading_ws, arm.to_string()));
        }
        offset += line.len();
    }
    arms
}

fn let_match_expr_complete(rest: &str) -> bool {
    let Some(eq) = find_top_level_char(rest, '=') else {
        return false;
    };
    let expr_text = rest[eq + 1..].trim();
    if !expr_text.starts_with("match ") {
        return false;
    }
    let Some(body_open) = find_top_level_char(expr_text, '{') else {
        return false;
    };
    matches!(
        find_matching_brace(expr_text, body_open),
        Some(close) if expr_text[close + 1..].trim().is_empty()
    )
}

fn let_match_expr_needs_more(rest: &str) -> bool {
    let Some(eq) = find_top_level_char(rest, '=') else {
        return false;
    };
    let expr_text = rest[eq + 1..].trim();
    expr_text.starts_with("match ")
        && find_top_level_char(expr_text, '{').is_some()
        && !let_match_expr_complete(rest)
}

fn collect_multiline_let_match(
    lines: &[&str],
    index: usize,
    path: &Path,
) -> Result<Option<(String, usize)>, Diagnostic> {
    let line_no = index + 1;
    let Some(rest) = lines[index].trim().strip_prefix("let ") else {
        return Ok(None);
    };
    if !let_match_expr_needs_more(rest) {
        return Ok(None);
    }

    let mut combined = rest.to_string();
    let mut next = index + 1;
    while next < lines.len() {
        combined.push('\n');
        combined.push_str(lines[next].trim());
        next += 1;
        if let_match_expr_complete(&combined) {
            return Ok(Some((combined, next)));
        }
    }

    Err(
        Diagnostic::new("parse", "match expression body is incomplete")
            .with_path(path.display().to_string())
            .with_span(line_no, 1),
    )
}

fn parse_let_stmt(rest: &str, path: &Path, line_no: usize) -> Result<Stmt, Diagnostic> {
    let colon = find_top_level_char(rest, ':').ok_or_else(|| {
        Diagnostic::new("parse", "let binding is missing ':'")
            .with_path(path.display().to_string())
            .with_span(line_no, 1)
    })?;
    let name = rest[..colon].trim();
    validate_ident(name, path, line_no, 5)?;
    let after_colon = &rest[colon + 1..];
    let eq = find_top_level_char(after_colon, '=').ok_or_else(|| {
        Diagnostic::new("parse", "let binding is missing '='")
            .with_path(path.display().to_string())
            .with_span(line_no, colon + 2)
    })?;
    let type_text = after_colon[..eq].trim();
    let expr_text = after_colon[eq + 1..].trim();
    let ty = parse_type_name(type_text, path, line_no, colon + 2)?;
    let expr = parse_expr(expr_text, path, line_no, colon + eq + 3)?;
    Ok(Stmt::Let {
        name: name.to_string(),
        ty,
        expr,
        line: line_no,
        column: 1,
    })
}

fn parse_type_name(
    raw: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<TypeName, Diagnostic> {
    if raw.starts_with("fn(") {
        let open = 2;
        let close = find_matching_paren(raw, open).ok_or_else(|| {
            Diagnostic::new("parse", "function type must use `fn(args): return` syntax")
                .with_path(path.display().to_string())
                .with_span(line_no, column)
        })?;
        let after = raw[close + 1..].trim_start();
        let Some(return_raw) = after.strip_prefix(':') else {
            return Err(
                Diagnostic::new("parse", "function type is missing `: return`")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column + close + 1),
            );
        };
        let params_raw = raw[open + 1..close].trim();
        let mut params = Vec::new();
        if !params_raw.is_empty() {
            for param_raw in split_top_level_type(params_raw, ',') {
                params.push(parse_type_name(
                    param_raw.trim(),
                    path,
                    line_no,
                    column + open + 1,
                )?);
            }
        }
        let return_ty = parse_type_name(return_raw.trim(), path, line_no, column + close + 2)?;
        return Ok(TypeName::Fn(params, Box::new(return_ty)));
    }
    if raw.starts_with("&mut [")
        && raw.ends_with(']')
        && matches!(find_matching_square(raw, 5), Some(close) if close == raw.len() - 1)
    {
        let inner = raw[6..raw.len() - 1].trim();
        if inner.is_empty() {
            return Err(
                Diagnostic::new("parse", "mutable slice type is missing an inner type")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(TypeName::MutSlice(Box::new(parse_type_name(
            inner,
            path,
            line_no,
            column + 6,
        )?)));
    }
    if let Some(inner) = raw.strip_prefix("&mut ") {
        let inner = inner.trim();
        if inner.is_empty() {
            return Err(Diagnostic::new(
                "parse",
                "mutable reference type is missing an inner type",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, column));
        }
        return Ok(TypeName::MutRef(Box::new(parse_type_name(
            inner,
            path,
            line_no,
            column + 5,
        )?)));
    }
    if raw.starts_with("&[")
        && raw.ends_with(']')
        && matches!(find_matching_square(raw, 1), Some(close) if close == raw.len() - 1)
    {
        let inner = raw[2..raw.len() - 1].trim();
        if inner.is_empty() {
            return Err(
                Diagnostic::new("parse", "slice type is missing an inner type")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(TypeName::Slice(Box::new(parse_type_name(
            inner,
            path,
            line_no,
            column + 2,
        )?)));
    }
    if raw.starts_with("Option<")
        && raw.ends_with('>')
        && matches!(find_matching_angle(raw, 6), Some(close) if close == raw.len() - 1)
    {
        let inner = raw[7..raw.len() - 1].trim();
        if inner.is_empty() {
            return Err(
                Diagnostic::new("parse", "Option type is missing an inner type")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(TypeName::Option(Box::new(parse_type_name(
            inner,
            path,
            line_no,
            column + 7,
        )?)));
    }
    if raw.starts_with("Result<")
        && raw.ends_with('>')
        && matches!(find_matching_angle(raw, 6), Some(close) if close == raw.len() - 1)
    {
        let inner = raw[7..raw.len() - 1].trim();
        let parts = split_top_level_type(inner, ',');
        if parts.len() != 2 {
            return Err(
                Diagnostic::new("parse", "Result type must use `Result<ok, err>` syntax")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        let ok_raw = parts[0].trim();
        let err_raw = parts[1].trim();
        if ok_raw.is_empty() || err_raw.is_empty() {
            return Err(
                Diagnostic::new("parse", "Result type is missing an ok or error type")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(TypeName::Result(
            Box::new(parse_type_name(ok_raw, path, line_no, column + 7)?),
            Box::new(parse_type_name(err_raw, path, line_no, column + 7)?),
        ));
    }
    if raw.starts_with('{')
        && raw.ends_with('}')
        && matches!(find_matching_brace(raw, 0), Some(close) if close == raw.len() - 1)
    {
        let inner = raw[1..raw.len() - 1].trim();
        if inner.is_empty() {
            return Err(Diagnostic::new("parse", "map type is empty")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        let colon = find_top_level_char(inner, ':').ok_or_else(|| {
            Diagnostic::new("parse", "map type must use `{key: value}` syntax")
                .with_path(path.display().to_string())
                .with_span(line_no, column)
        })?;
        let key_raw = inner[..colon].trim();
        let value_raw = inner[colon + 1..].trim();
        if key_raw.is_empty() || value_raw.is_empty() {
            return Err(
                Diagnostic::new("parse", "map type is missing a key or value type")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(TypeName::Map(
            Box::new(parse_type_name(key_raw, path, line_no, column + 1)?),
            Box::new(parse_type_name(
                value_raw,
                path,
                line_no,
                column + colon + 2,
            )?),
        ));
    }
    if is_wrapped_in_parens(raw) {
        let inner = raw[1..raw.len() - 1].trim();
        if inner.is_empty() {
            return Err(Diagnostic::new("parse", "tuple type is empty")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        let items = split_top_level_type(inner, ',');
        if items.len() > 1 {
            return Ok(TypeName::Tuple(parse_tuple_type_names(
                inner,
                path,
                line_no,
                column + 1,
            )?));
        }
        return parse_type_name(inner, path, line_no, column + 1);
    }
    if raw.starts_with('[')
        && raw.ends_with(']')
        && matches!(find_matching_square(raw, 0), Some(close) if close == raw.len() - 1)
    {
        let inner = raw[1..raw.len() - 1].trim();
        if inner.is_empty() {
            return Err(
                Diagnostic::new("parse", "array type is missing an element type")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        let (element_raw, len_raw) = if let Some(semi) = find_top_level_char(inner, ';') {
            let element_raw = inner[..semi].trim();
            let len_raw = inner[semi + 1..].trim();
            if element_raw.is_empty() {
                return Err(
                    Diagnostic::new("parse", "array type is missing an element type")
                        .with_path(path.display().to_string())
                        .with_span(line_no, column + 1),
                );
            }
            if len_raw.is_empty() {
                return Err(
                    Diagnostic::new("parse", "array type is missing a length expression")
                        .with_path(path.display().to_string())
                        .with_span(line_no, column + semi + 2),
                );
            }
            parse_expr(len_raw, path, line_no, column + semi + 2)?;
            (element_raw, Some(len_raw.to_string()))
        } else {
            (inner, None)
        };
        return Ok(TypeName::Array(
            Box::new(parse_type_name(element_raw, path, line_no, column + 1)?),
            len_raw,
        ));
    }
    if let Some(open_angle) = find_top_level_char(raw, '<') {
        if !raw.ends_with('>')
            || !matches!(find_matching_angle(raw, open_angle), Some(close) if close == raw.len() - 1)
        {
            return Err(
                Diagnostic::new("parse", "generic types must use `Name<type>` syntax")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column + open_angle),
            );
        }
        let name = raw[..open_angle].trim();
        validate_ident(name, path, line_no, column)?;
        let args_raw = raw[open_angle + 1..raw.len() - 1].trim();
        if args_raw.is_empty() {
            return Err(
                Diagnostic::new("parse", "generic type is missing type arguments")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column + open_angle + 1),
            );
        }
        let mut type_args = Vec::new();
        for arg in split_top_level_type(args_raw, ',') {
            type_args.push(parse_type_name(
                arg.trim(),
                path,
                line_no,
                column + open_angle + 1,
            )?);
        }
        if name == "ptr" {
            if type_args.len() != 1 {
                return Err(
                    Diagnostic::new("parse", "ptr types must use `ptr<type>` syntax")
                        .with_path(path.display().to_string())
                        .with_span(line_no, column),
                );
            }
            return Ok(TypeName::Ptr(Box::new(type_args.remove(0))));
        }
        if name == "mutptr" {
            if type_args.len() != 1 {
                return Err(Diagnostic::new(
                    "parse",
                    "mutptr types must use `mutptr<type>` syntax",
                )
                .with_path(path.display().to_string())
                .with_span(line_no, column));
            }
            return Ok(TypeName::MutPtr(Box::new(type_args.remove(0))));
        }
        return Ok(TypeName::Named(name.to_string(), type_args));
    }
    match raw {
        "int" => Ok(TypeName::Int),
        "bool" => Ok(TypeName::Bool),
        "string" | "String" => Ok(TypeName::String),
        "&str" => Ok(TypeName::Str),
        name => {
            if let Some(numeric) = NumericType::parse(name) {
                return Ok(TypeName::Numeric(numeric));
            }
            validate_ident(raw, path, line_no, column)?;
            Ok(TypeName::Named(raw.to_string(), Vec::new()))
        }
    }
}

fn source_position_for_offset(
    line_no: usize,
    column: usize,
    raw: &str,
    offset: usize,
) -> (usize, usize) {
    let mut line = line_no;
    let mut current_column = column;
    for ch in raw[..offset.min(raw.len())].chars() {
        if ch == '\n' {
            line += 1;
            current_column = 1;
        } else {
            current_column += 1;
        }
    }
    (line, current_column)
}

fn parse_expr(raw: &str, path: &Path, line_no: usize, column: usize) -> Result<Expr, Diagnostic> {
    let raw = raw.trim();
    if raw.starts_with('|') {
        return parse_term(raw, path, line_no, column);
    }
    if raw.starts_with("match ") {
        return parse_match_expr(raw, path, line_no, column);
    }
    if let Some(split_index) = find_top_level_as(raw) {
        let lhs_raw = raw[..split_index].trim();
        let ty_raw = raw[split_index + 4..].trim();
        if lhs_raw.is_empty() || ty_raw.is_empty() {
            return Err(
                Diagnostic::new("parse", "cast expression must use `expr as Type` syntax")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(Expr::Cast {
            expr: Box::new(parse_expr(lhs_raw, path, line_no, column)?),
            ty: parse_type_name(ty_raw, path, line_no, column + split_index + 5)?,
            line: line_no,
            column,
        });
    }
    if let Some((op, split_index)) = find_compare_operator(raw) {
        let lhs_raw = raw[..split_index].trim();
        let rhs_offset = split_index + op.lexeme().len();
        let rhs_raw = raw[rhs_offset..].trim();
        if lhs_raw.is_empty() || rhs_raw.is_empty() {
            return Err(
                Diagnostic::new("parse", "comparison expression is incomplete")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(Expr::BinaryCompare {
            op,
            lhs: Box::new(parse_add(lhs_raw, path, line_no, column)?),
            rhs: Box::new(parse_add(rhs_raw, path, line_no, column)?),
            line: line_no,
            column,
        });
    }
    parse_add(raw, path, line_no, column)
}

fn parse_add(raw: &str, path: &Path, line_no: usize, column: usize) -> Result<Expr, Diagnostic> {
    let terms =
        split_top_level_arithmetic(raw, &[('+', ArithmeticOp::Add), ('-', ArithmeticOp::Sub)]);
    if terms.len() > 1 {
        let mut expr = parse_mul(terms[0].0.trim(), path, line_no, column)?;
        for (term, op) in &terms[1..] {
            let rhs = parse_mul(term.trim(), path, line_no, column)?;
            expr = Expr::BinaryAdd {
                op: *op,
                lhs: Box::new(expr),
                rhs: Box::new(rhs),
                line: line_no,
                column,
            };
        }
        return Ok(expr);
    }
    parse_mul(raw.trim(), path, line_no, column)
}

fn parse_mul(raw: &str, path: &Path, line_no: usize, column: usize) -> Result<Expr, Diagnostic> {
    let terms =
        split_top_level_arithmetic(raw, &[('*', ArithmeticOp::Mul), ('/', ArithmeticOp::Div)]);
    if terms.len() > 1 {
        let mut expr = parse_term(terms[0].0.trim(), path, line_no, column)?;
        for (term, op) in &terms[1..] {
            let rhs = parse_term(term.trim(), path, line_no, column)?;
            expr = Expr::BinaryAdd {
                op: *op,
                lhs: Box::new(expr),
                rhs: Box::new(rhs),
                line: line_no,
                column,
            };
        }
        return Ok(expr);
    }
    parse_term(raw.trim(), path, line_no, column)
}

fn parse_term(raw: &str, path: &Path, line_no: usize, column: usize) -> Result<Expr, Diagnostic> {
    if let Some(closure) = parse_closure_expr(raw, path, line_no, column)? {
        return Ok(closure);
    }
    if raw.is_empty() {
        return Err(Diagnostic::new("parse", "expression is empty")
            .with_path(path.display().to_string())
            .with_span(line_no, column));
    }

    if let Some(literal) = parse_numeric_literal(raw) {
        return Ok(Expr::Literal(literal));
    }
    if looks_like_invalid_numeric_literal(raw) {
        return Err(
            Diagnostic::new("parse", format!("invalid numeric literal {raw:?}"))
                .with_path(path.display().to_string())
                .with_span(line_no, column),
        );
    }
    if raw.ends_with('?') {
        let inner = raw[..raw.len() - 1].trim_end();
        if inner.is_empty() {
            return Err(
                Diagnostic::new("parse", "try expression is missing an operand")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(Expr::Try {
            expr: Box::new(parse_term(inner, path, line_no, column)?),
            line: line_no,
            column: column + inner.len(),
        });
    }
    if let Some(inner) = raw.strip_prefix("await ") {
        let inner = inner.trim();
        if inner.is_empty() {
            return Err(
                Diagnostic::new("parse", "await expression is missing an operand")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(Expr::Await {
            expr: Box::new(parse_term(inner, path, line_no, column + 6)?),
            line: line_no,
            column,
        });
    }
    if let Some(dot) = find_last_top_level_char(raw, '.') {
        let base_raw = raw[..dot].trim();
        let field = raw[dot + 1..].trim();
        if base_raw.is_empty() || field.is_empty() {
            return Err(Diagnostic::new("parse", "field access is incomplete")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        if field.ends_with(')')
            && let Some(open_paren) = find_top_level_char(field, '(')
            && matches!(find_matching_paren(field, open_paren), Some(close) if close == field.len() - 1)
        {
            let method_text = field[..open_paren].trim();
            let (method, type_args) =
                parse_call_name(method_text, path, line_no, column + dot + 1)?;
            validate_ident(method, path, line_no, column + dot + 1)?;
            let args = parse_call_args(
                &field[open_paren + 1..field.len() - 1],
                path,
                line_no,
                column + dot + 1,
            )?;
            return Ok(Expr::MethodCall {
                base: Box::new(parse_term(base_raw, path, line_no, column)?),
                method: method.to_string(),
                type_args,
                args,
                line: line_no,
                column,
            });
        }
        if field.chars().all(|ch| ch.is_ascii_digit()) {
            let index = field.parse::<usize>().map_err(|_| {
                Diagnostic::new("parse", format!("invalid tuple index {field:?}"))
                    .with_path(path.display().to_string())
                    .with_span(line_no, column + dot + 1)
            })?;
            return Ok(Expr::TupleIndex {
                base: Box::new(parse_term(base_raw, path, line_no, column)?),
                index,
                line: line_no,
                column,
            });
        }
        validate_ident(field, path, line_no, column + dot + 1)?;
        return Ok(Expr::FieldAccess {
            base: Box::new(parse_term(base_raw, path, line_no, column)?),
            field: field.to_string(),
            line: line_no,
            column,
        });
    }
    if raw.ends_with(']')
        && let Some(open_bracket) = find_last_top_level_char(raw, '[')
        && matches!(find_matching_square(raw, open_bracket), Some(close) if close == raw.len() - 1)
    {
        let base_raw = raw[..open_bracket].trim();
        let index_raw = raw[open_bracket + 1..raw.len() - 1].trim();
        if base_raw.is_empty() {
            // This is an array literal, handled below.
        } else if let Some(colon) = find_top_level_char(index_raw, ':') {
            let start_raw = index_raw[..colon].trim();
            let end_raw = index_raw[colon + 1..].trim();
            return Ok(Expr::Slice {
                base: Box::new(parse_term(base_raw, path, line_no, column)?),
                start: if start_raw.is_empty() {
                    None
                } else {
                    Some(Box::new(parse_expr(
                        start_raw,
                        path,
                        line_no,
                        column + open_bracket + 1,
                    )?))
                },
                end: if end_raw.is_empty() {
                    None
                } else {
                    Some(Box::new(parse_expr(
                        end_raw,
                        path,
                        line_no,
                        column + open_bracket + colon + 2,
                    )?))
                },
                line: line_no,
                column,
            });
        } else if index_raw.is_empty() {
            return Err(Diagnostic::new("parse", "array index is incomplete")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        } else {
            return Ok(Expr::Index {
                base: Box::new(parse_term(base_raw, path, line_no, column)?),
                index: Box::new(parse_expr(
                    index_raw,
                    path,
                    line_no,
                    column + open_bracket + 1,
                )?),
                line: line_no,
                column,
            });
        }
    }
    if is_wrapped_in_parens(raw) {
        let inner = raw[1..raw.len() - 1].trim();
        if split_top_level(inner, ',').len() > 1 {
            return Ok(Expr::TupleLiteral {
                elements: parse_tuple_literal_elements(inner, path, line_no, column + 1)?,
                line: line_no,
                column,
            });
        }
        return parse_expr(&raw[1..raw.len() - 1], path, line_no, column + 1);
    }
    if raw.starts_with('{')
        && raw.ends_with('}')
        && matches!(find_matching_brace(raw, 0), Some(close) if close == raw.len() - 1)
    {
        return Ok(Expr::MapLiteral {
            entries: parse_map_literal_entries(&raw[1..raw.len() - 1], path, line_no, column + 1)?,
            line: line_no,
            column,
        });
    }
    if raw.starts_with('[')
        && raw.ends_with(']')
        && matches!(find_matching_square(raw, 0), Some(close) if close == raw.len() - 1)
    {
        return Ok(Expr::ArrayLiteral {
            elements: parse_array_literal_elements(&raw[1..raw.len() - 1], path, line_no, column)?,
            line: line_no,
            column,
        });
    }
    if raw.ends_with('}')
        && let Some(open_brace) = find_top_level_char(raw, '{')
        && matches!(find_matching_brace(raw, open_brace), Some(close) if close == raw.len() - 1)
    {
        let target = raw[..open_brace].trim();
        if !target.is_empty() {
            let (name, type_args) = parse_call_name(target, path, line_no, column)?;
            validate_ident(name, path, line_no, column)?;
            let fields = parse_struct_literal_fields(
                &raw[open_brace + 1..raw.len() - 1],
                path,
                line_no,
                column,
            )?;
            return Ok(Expr::StructLiteral {
                name: name.to_string(),
                type_args,
                fields,
                line: line_no,
                column,
            });
        }
    }
    if raw.starts_with('"') {
        let parsed = serde_json::from_str::<String>(raw).map_err(|_| {
            Diagnostic::new("parse", "invalid string literal")
                .with_path(path.display().to_string())
                .with_span(line_no, column)
        })?;
        return Ok(Expr::Literal(Literal::String(parsed)));
    }
    if raw == "true" {
        return Ok(Expr::Literal(Literal::Bool(true)));
    }
    if raw == "false" {
        return Ok(Expr::Literal(Literal::Bool(false)));
    }
    if let Ok(value) = raw.parse::<i64>() {
        return Ok(Expr::Literal(Literal::Int(value)));
    }
    if let Some(inner) = raw.strip_prefix("&mut ") {
        let inner = inner.trim();
        if inner.is_empty() {
            return Err(
                Diagnostic::new("parse", "mutable borrow expression is missing a target")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(Expr::MutBorrow {
            expr: Box::new(parse_expr(inner, path, line_no, column + 5)?),
            line: line_no,
            column,
        });
    }
    if let Some(inner) = raw.strip_prefix('*') {
        let inner = inner.trim();
        if inner.is_empty() {
            return Err(
                Diagnostic::new("parse", "dereference expression is missing a target")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column),
            );
        }
        return Ok(Expr::Deref {
            expr: Box::new(parse_expr(inner, path, line_no, column + 1)?),
            line: line_no,
            column,
        });
    }
    if raw.ends_with(')')
        && let Some(open_paren) = find_top_level_char(raw, '(')
    {
        let name_text = raw[..open_paren].trim();
        if !name_text.is_empty() {
            let (name, type_args) = parse_call_name(name_text, path, line_no, column)?;
            validate_ident(name, path, line_no, column)?;
            let args = parse_call_args(&raw[open_paren + 1..raw.len() - 1], path, line_no, column)?;
            return Ok(Expr::Call {
                name: name.to_string(),
                type_args,
                args,
                line: line_no,
                column,
            });
        }
    }
    validate_ident(raw, path, line_no, column)?;
    Ok(Expr::VarRef {
        name: raw.to_string(),
        line: line_no,
        column,
    })
}

const NUMERIC_LITERAL_SUFFIXES: &[&str] = &[
    "isize", "usize", "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64",
];

fn parse_numeric_literal(raw: &str) -> Option<Literal> {
    for suffix in NUMERIC_LITERAL_SUFFIXES {
        let Some(number) = raw.strip_suffix(*suffix) else {
            continue;
        };
        if number.is_empty() || number == "." {
            return None;
        }
        let ty = NumericType::parse(suffix)?;
        if numeric_literal_fits(number, ty) {
            return Some(Literal::Numeric {
                raw: number.to_string(),
                ty,
            });
        }
    }
    None
}

fn looks_like_invalid_numeric_literal(raw: &str) -> bool {
    NUMERIC_LITERAL_SUFFIXES.iter().any(|suffix| {
        let Some(number) = raw.strip_suffix(suffix) else {
            return false;
        };
        !number.is_empty()
            && (number.parse::<f64>().is_ok()
                || number
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_digit() || ch == '-' || ch == '.'))
    })
}

fn numeric_literal_fits(number: &str, ty: NumericType) -> bool {
    match ty {
        NumericType::F32 => float_literal_fits_f32(number),
        NumericType::F64 => float_literal_fits_f64(number),
        NumericType::I8 => integer_literal_in_range(number, i8::MIN as i128, i8::MAX as i128),
        NumericType::I16 => integer_literal_in_range(number, i16::MIN as i128, i16::MAX as i128),
        NumericType::I32 => integer_literal_in_range(number, i32::MIN as i128, i32::MAX as i128),
        NumericType::I64 | NumericType::Isize => {
            integer_literal_in_range(number, i64::MIN as i128, i64::MAX as i128)
        }
        NumericType::U8 => unsigned_integer_literal_in_range(number, u8::MAX as u128),
        NumericType::U16 => unsigned_integer_literal_in_range(number, u16::MAX as u128),
        NumericType::U32 => unsigned_integer_literal_in_range(number, u32::MAX as u128),
        NumericType::U64 | NumericType::Usize => {
            unsigned_integer_literal_in_range(number, u64::MAX as u128)
        }
    }
}

fn float_literal_has_rust_number_shape(number: &str) -> bool {
    let unsigned = number.strip_prefix('-').unwrap_or(number);
    let Some(first) = unsigned.chars().next() else {
        return false;
    };
    if !first.is_ascii_digit() {
        return false;
    }
    if !unsigned.chars().any(|ch| ch.is_ascii_digit()) {
        return false;
    }
    if unsigned
        .chars()
        .any(|ch| !(ch.is_ascii_digit() || matches!(ch, '_' | '.' | 'e' | 'E' | '+' | '-')))
    {
        return false;
    }
    true
}

fn float_literal_fits_f32(number: &str) -> bool {
    float_literal_has_rust_number_shape(number) && number.parse::<f32>().is_ok_and(f32::is_finite)
}

fn float_literal_fits_f64(number: &str) -> bool {
    float_literal_has_rust_number_shape(number) && number.parse::<f64>().is_ok_and(f64::is_finite)
}

fn integer_literal_in_range(number: &str, min: i128, max: i128) -> bool {
    number
        .parse::<i128>()
        .is_ok_and(|value| value >= min && value <= max)
}

fn unsigned_integer_literal_in_range(number: &str, max: u128) -> bool {
    if number.starts_with('-') {
        return false;
    }
    number.parse::<u128>().is_ok_and(|value| value <= max)
}

fn find_top_level_as(raw: &str) -> Option<usize> {
    let mut paren = 0usize;
    let mut square = 0usize;
    let mut brace = 0usize;
    let mut angle = 0usize;
    let bytes = raw.as_bytes();
    let mut index = 0usize;
    while index + 4 <= bytes.len() {
        match bytes[index] as char {
            '(' => paren += 1,
            ')' => paren = paren.saturating_sub(1),
            '[' => square += 1,
            ']' => square = square.saturating_sub(1),
            '{' => brace += 1,
            '}' => brace = brace.saturating_sub(1),
            '<' => angle += 1,
            '>' => angle = angle.saturating_sub(1),
            _ => {}
        }
        if paren == 0 && square == 0 && brace == 0 && angle == 0 && raw[index..].starts_with(" as ")
        {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn parse_closure_expr(
    raw: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<Option<Expr>, Diagnostic> {
    if !raw.starts_with('|') {
        return Ok(None);
    }
    let Some(close_bar) = find_closure_param_bar(raw) else {
        return Err(
            Diagnostic::new("parse", "closure parameters must be closed with `|`")
                .with_path(path.display().to_string())
                .with_span(line_no, column),
        );
    };
    let params_raw = raw[1..close_bar].trim();
    let body_raw = raw[close_bar + 1..].trim();
    if body_raw.is_empty() {
        return Err(Diagnostic::new("parse", "closure body is empty")
            .with_path(path.display().to_string())
            .with_span(line_no, column + close_bar + 1));
    }
    let mut params = Vec::new();
    if !params_raw.is_empty() {
        for param_text in split_top_level(params_raw, ',') {
            let colon = find_top_level_char(param_text, ':').ok_or_else(|| {
                Diagnostic::new("parse", "closure parameter must use `name: type` syntax")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column + 1)
            })?;
            let name = param_text[..colon].trim();
            validate_ident(name, path, line_no, column + 1)?;
            let ty = parse_type_name(
                param_text[colon + 1..].trim(),
                path,
                line_no,
                column + colon + 2,
            )?;
            params.push(Param {
                name: name.to_string(),
                ty,
                line: line_no,
                column: column + 1,
            });
        }
    }
    let body_text = if body_raw.starts_with('{')
        && body_raw.ends_with('}')
        && matches!(find_matching_brace(body_raw, 0), Some(close) if close == body_raw.len() - 1)
    {
        body_raw[1..body_raw.len() - 1].trim()
    } else {
        body_raw
    };
    let body = parse_expr(body_text, path, line_no, column + close_bar + 1)?;
    Ok(Some(Expr::Closure {
        params,
        body: Box::new(body),
        line: line_no,
        column,
    }))
}

fn find_closure_param_bar(raw: &str) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in raw.char_indices().skip(1) {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            continue;
        }
        if ch == '|' {
            return Some(index);
        }
    }
    None
}

fn parse_function_name<'a>(
    raw: &'a str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<(&'a str, Vec<String>), Diagnostic> {
    parse_decl_name(raw, "function", path, line_no, column)
}

fn parse_decl_name<'a>(
    raw: &'a str,
    kind: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<(&'a str, Vec<String>), Diagnostic> {
    if let Some(open_angle) = find_top_level_char(raw, '<') {
        if !raw.ends_with('>')
            || !matches!(find_matching_angle(raw, open_angle), Some(close) if close == raw.len() - 1)
        {
            return Err(Diagnostic::new(
                "parse",
                format!("generic {kind} declarations must use `name<T>` syntax"),
            )
            .with_path(path.display().to_string())
            .with_span(line_no, column + open_angle));
        }
        let name = raw[..open_angle].trim();
        validate_ident(name, path, line_no, column)?;
        let params_raw = raw[open_angle + 1..raw.len() - 1].trim();
        if params_raw.is_empty() {
            return Err(Diagnostic::new(
                "parse",
                format!("generic {kind} is missing type parameters"),
            )
            .with_path(path.display().to_string())
            .with_span(line_no, column + open_angle + 1));
        }
        let mut params = Vec::new();
        for param in split_top_level_type(params_raw, ',') {
            let param = param.trim();
            validate_ident(param, path, line_no, column + open_angle + 1)?;
            if params.iter().any(|existing| existing == param) {
                return Err(Diagnostic::new(
                    "parse",
                    format!("duplicate type parameter {param:?}"),
                )
                .with_path(path.display().to_string())
                .with_span(line_no, column + open_angle + 1));
            }
            params.push(param.to_string());
        }
        return Ok((name, params));
    }
    validate_ident(raw, path, line_no, column)?;
    Ok((raw, Vec::new()))
}

fn parse_call_name<'a>(
    raw: &'a str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<(&'a str, Vec<TypeName>), Diagnostic> {
    if let Some(open_angle) = find_top_level_char(raw, '<') {
        if !raw.ends_with('>')
            || !matches!(find_matching_angle(raw, open_angle), Some(close) if close == raw.len() - 1)
        {
            return Err(Diagnostic::new(
                "parse",
                "generic calls must use `name<type>(args)` syntax",
            )
            .with_path(path.display().to_string())
            .with_span(line_no, column + open_angle));
        }
        let name = raw[..open_angle].trim();
        validate_ident(name, path, line_no, column)?;
        let args_raw = raw[open_angle + 1..raw.len() - 1].trim();
        if args_raw.is_empty() {
            return Err(
                Diagnostic::new("parse", "generic call is missing type arguments")
                    .with_path(path.display().to_string())
                    .with_span(line_no, column + open_angle + 1),
            );
        }
        let mut type_args = Vec::new();
        for arg in split_top_level_type(args_raw, ',') {
            type_args.push(parse_type_name(
                arg.trim(),
                path,
                line_no,
                column + open_angle + 1,
            )?);
        }
        return Ok((name, type_args));
    }
    Ok((raw, Vec::new()))
}

fn parse_array_literal_elements(
    raw: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<Vec<Expr>, Diagnostic> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut elements = Vec::new();
    for element_text in split_top_level_type(raw, ',') {
        let element_text = element_text.trim();
        if element_text.is_empty() {
            return Err(Diagnostic::new("parse", "array literal element is empty")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        elements.push(parse_expr(element_text, path, line_no, column)?);
    }
    Ok(elements)
}

fn parse_tuple_type_names(
    raw: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<Vec<TypeName>, Diagnostic> {
    let mut elements = Vec::new();
    for element_text in split_top_level(raw, ',') {
        let element_text = element_text.trim();
        if element_text.is_empty() {
            return Err(Diagnostic::new("parse", "tuple type element is empty")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        elements.push(parse_type_name(element_text, path, line_no, column)?);
    }
    Ok(elements)
}

fn parse_tuple_literal_elements(
    raw: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<Vec<Expr>, Diagnostic> {
    let mut elements = Vec::new();
    for element_text in split_top_level(raw, ',') {
        let element_text = element_text.trim();
        if element_text.is_empty() {
            return Err(Diagnostic::new("parse", "tuple literal element is empty")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        elements.push(parse_expr(element_text, path, line_no, column)?);
    }
    Ok(elements)
}

fn parse_map_literal_entries(
    raw: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<Vec<MapEntry>, Diagnostic> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for entry_text in split_top_level(raw, ',') {
        let entry_text = entry_text.trim();
        if entry_text.is_empty() {
            return Err(Diagnostic::new("parse", "map literal entry is empty")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        let colon = find_top_level_char(entry_text, ':').ok_or_else(|| {
            Diagnostic::new("parse", "map literal entry is missing ':'")
                .with_path(path.display().to_string())
                .with_span(line_no, column)
        })?;
        let key = parse_expr(entry_text[..colon].trim(), path, line_no, column)?;
        let value = parse_expr(
            entry_text[colon + 1..].trim(),
            path,
            line_no,
            column + colon + 1,
        )?;
        entries.push(MapEntry {
            key,
            value,
            line: line_no,
            column,
        });
    }
    Ok(entries)
}

fn parse_struct_literal_fields(
    raw: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<Vec<StructFieldValue>, Diagnostic> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut fields = Vec::new();
    for field_text in split_top_level_type(raw, ',') {
        let field_text = field_text.trim();
        if field_text.is_empty() {
            return Err(Diagnostic::new("parse", "struct literal field is empty")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        let colon = find_top_level_char(field_text, ':').ok_or_else(|| {
            Diagnostic::new("parse", "struct literal field is missing ':'")
                .with_path(path.display().to_string())
                .with_span(line_no, column)
        })?;
        let name = field_text[..colon].trim();
        validate_ident(name, path, line_no, column)?;
        let expr = parse_expr(
            field_text[colon + 1..].trim(),
            path,
            line_no,
            column + colon + 1,
        )?;
        fields.push(StructFieldValue {
            name: name.to_string(),
            expr,
            line: line_no,
            column,
        });
    }
    Ok(fields)
}

fn parse_named_enum_payload_fields(
    raw: &str,
    path: &Path,
    line_no: usize,
    column: usize,
    context: &str,
) -> Result<Vec<(String, TypeName)>, Diagnostic> {
    if raw.trim().is_empty() {
        return Err(Diagnostic::new("parse", format!("{context} is empty"))
            .with_path(path.display().to_string())
            .with_span(line_no, column));
    }
    let mut fields = Vec::new();
    for field_text in split_top_level(raw, ',') {
        let field_text = field_text.trim();
        if field_text.is_empty() {
            return Err(Diagnostic::new("parse", format!("{context} is empty"))
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        let colon = find_top_level_char(field_text, ':').ok_or_else(|| {
            Diagnostic::new("parse", format!("{context} is missing ':'"))
                .with_path(path.display().to_string())
                .with_span(line_no, column)
        })?;
        let name = field_text[..colon].trim();
        validate_ident(name, path, line_no, column)?;
        let ty = parse_type_name(
            field_text[colon + 1..].trim(),
            path,
            line_no,
            column + colon + 1,
        )?;
        fields.push((name.to_string(), ty));
    }
    Ok(fields)
}

fn parse_call_args(
    raw: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<Vec<Expr>, Diagnostic> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut args = Vec::new();
    for arg_text in split_top_level(raw, ',') {
        let arg_text = arg_text.trim();
        if arg_text.is_empty() {
            return Err(Diagnostic::new("parse", "call argument is empty")
                .with_path(path.display().to_string())
                .with_span(line_no, column));
        }
        args.push(parse_expr(arg_text, path, line_no, column)?);
    }
    Ok(args)
}

fn validate_ident(
    value: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Result<(), Diagnostic> {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(Diagnostic::new("parse", "identifier is empty")
            .with_path(path.display().to_string())
            .with_span(line_no, column));
    };
    if !(first.is_ascii_alphabetic() || first == '_')
        || !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Err(
            Diagnostic::new("parse", format!("invalid identifier {value:?}"))
                .with_path(path.display().to_string())
                .with_span(line_no, column),
        );
    }
    Ok(())
}

fn find_nested_match_pattern_offset(raw: &str) -> Option<usize> {
    ['(', '{', '[', ':']
        .into_iter()
        .filter_map(|ch| find_top_level_char(raw, ch))
        .min()
}

fn split_top_level(raw: &str, delimiter: char) -> Vec<&str> {
    split_top_level_with_offsets(raw, delimiter)
        .into_iter()
        .map(|(_, part)| part)
        .collect()
}

fn split_top_level_with_offsets(raw: &str, delimiter: char) -> Vec<(usize, &str)> {
    let mut parts = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut start = 0;
    for (index, ch) in raw.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            _ if ch == delimiter && paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                parts.push((start, &raw[start..index]));
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push((start, &raw[start..]));
    parts
}

fn split_top_level_arithmetic<'a>(
    raw: &'a str,
    operators: &[(char, ArithmeticOp)],
) -> Vec<(&'a str, ArithmeticOp)> {
    let mut parts = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut start = 0;
    let mut next_op = ArithmeticOp::Add;
    let chars: Vec<(usize, char)> = raw.char_indices().collect();
    for (cursor, (index, ch)) in chars.iter().copied().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            _ if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                if ch == '-' && is_unary_minus(raw, cursor, &chars) {
                    continue;
                }
                if ch == '*' && is_unary_deref(raw, cursor, &chars) {
                    continue;
                }
                if let Some((_, op)) = operators.iter().find(|(candidate, _)| *candidate == ch) {
                    parts.push((&raw[start..index], next_op));
                    start = index + ch.len_utf8();
                    next_op = *op;
                }
            }
            _ => {}
        }
    }
    parts.push((&raw[start..], next_op));
    parts
}

fn is_unary_minus(raw: &str, cursor: usize, chars: &[(usize, char)]) -> bool {
    if cursor == 0 {
        return true;
    }
    let before = raw[..chars[cursor].0].trim_end();
    if before.ends_with('e') || before.ends_with('E') {
        return true;
    }
    matches!(
        before.chars().next_back(),
        None | Some('(' | '[' | '{' | ',' | ':' | '=' | '<' | '>' | '+' | '-' | '*' | '/')
    )
}

fn is_unary_deref(raw: &str, cursor: usize, chars: &[(usize, char)]) -> bool {
    if cursor == 0 {
        return true;
    }
    let before = raw[..chars[cursor].0].trim_end();
    matches!(
        before.chars().next_back(),
        None | Some('(' | '[' | '{' | ',' | ':' | '=' | '<' | '>' | '+' | '-' | '*' | '/')
    )
}

fn find_top_level_keyword(raw: &str, keyword: &str) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut angle_depth = 0usize;
    let chars: Vec<(usize, char)> = raw.char_indices().collect();
    for cursor in 0..chars.len() {
        let (index, ch) = chars[cursor];
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '<' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                angle_depth += 1;
            }
            '>' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                angle_depth = angle_depth.saturating_sub(1);
            }
            _ => {}
        }
        if paren_depth != 0 || brace_depth != 0 || bracket_depth != 0 || angle_depth != 0 {
            continue;
        }
        if !raw[index..].starts_with(keyword) {
            continue;
        }
        let before = if cursor == 0 {
            None
        } else {
            Some(chars[cursor - 1].1)
        };
        let after_index = index + keyword.len();
        let after = raw[after_index..].chars().next();
        if before.is_none_or(|ch| ch.is_ascii_whitespace())
            && after.is_none_or(|ch| ch.is_ascii_whitespace())
        {
            return Some(index);
        }
    }
    None
}

fn split_top_level_type(raw: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut angle_depth = 0usize;
    let mut start = 0;
    for (index, ch) in raw.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            _ if ch == delimiter
                && paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && angle_depth == 0 =>
            {
                parts.push(&raw[start..index]);
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&raw[start..]);
    parts
}

fn find_top_level_char(raw: &str, target: char) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut angle_depth = 0usize;
    for (index, ch) in raw.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if angle_depth > 0 {
            match ch {
                '<' => angle_depth += 1,
                '>' => angle_depth = angle_depth.saturating_sub(1),
                _ => {}
            }
            continue;
        }
        match ch {
            '(' => {
                if target == '(' && paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 {
                    return Some(index);
                }
                paren_depth += 1;
            }
            ')' => {
                if target == ')' && paren_depth == 1 && brace_depth == 0 && bracket_depth == 0 {
                    return Some(index);
                }
                paren_depth = paren_depth.saturating_sub(1);
            }
            '{' => {
                if target == '{' && paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 {
                    return Some(index);
                }
                brace_depth += 1;
            }
            '}' => {
                if target == '}' && paren_depth == 0 && brace_depth == 1 && bracket_depth == 0 {
                    return Some(index);
                }
                brace_depth = brace_depth.saturating_sub(1);
            }
            '[' => {
                if target == '['
                    && paren_depth == 0
                    && brace_depth == 0
                    && bracket_depth == 0
                    && angle_depth == 0
                {
                    return Some(index);
                }
                bracket_depth += 1;
            }
            ']' => {
                if target == ']'
                    && paren_depth == 0
                    && brace_depth == 0
                    && bracket_depth == 1
                    && angle_depth == 0
                {
                    return Some(index);
                }
                bracket_depth = bracket_depth.saturating_sub(1);
            }
            '<' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                if target == '<' {
                    return Some(index);
                }
                angle_depth += 1;
            }
            '>' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                if target == '>' {
                    return Some(index);
                }
                angle_depth = angle_depth.saturating_sub(1);
            }
            _ if ch == target
                && paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && angle_depth == 0 =>
            {
                return Some(index);
            }
            _ => {}
        }
    }
    None
}

fn find_top_level_arrow(raw: &str) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut chars = raw.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '=' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                if chars.peek().is_some_and(|(_, next)| *next == '>') {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_matching_paren(raw: &str, open_index: usize) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    let mut paren_depth = 0usize;
    for (index, ch) in raw
        .char_indices()
        .skip_while(|(index, _)| *index < open_index)
    {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '(' => paren_depth += 1,
            ')' => {
                paren_depth = paren_depth.saturating_sub(1);
                if paren_depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_matching_angle(raw: &str, open_index: usize) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    let mut angle_depth = 0usize;
    for (index, ch) in raw
        .char_indices()
        .skip_while(|(index, _)| *index < open_index)
    {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '<' => angle_depth += 1,
            '>' => {
                angle_depth = angle_depth.saturating_sub(1);
                if angle_depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_matching_brace(raw: &str, open_index: usize) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    let mut brace_depth = 0usize;
    let mut paren_depth = 0usize;
    for (index, ch) in raw
        .char_indices()
        .skip_while(|(index, _)| *index < open_index)
    {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' if paren_depth == 0 => brace_depth += 1,
            '}' if paren_depth == 0 => {
                brace_depth = brace_depth.saturating_sub(1);
                if brace_depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_matching_square(raw: &str, open_index: usize) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    for (index, ch) in raw
        .char_indices()
        .skip_while(|(index, _)| *index < open_index)
    {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' if paren_depth == 0 && brace_depth == 0 => bracket_depth += 1,
            ']' if paren_depth == 0 && brace_depth == 0 => {
                bracket_depth = bracket_depth.saturating_sub(1);
                if bracket_depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_last_top_level_char(raw: &str, target: char) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut found = None;
    for (index, ch) in raw.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => {
                if target == '[' && paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 {
                    found = Some(index);
                }
                bracket_depth += 1;
            }
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            _ if ch == target && paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                found = Some(index)
            }
            _ => {}
        }
    }
    found
}

fn is_wrapped_in_parens(raw: &str) -> bool {
    raw.starts_with('(')
        && raw.ends_with(')')
        && matches!(find_matching_paren(raw, 0), Some(close) if close == raw.len() - 1)
}

fn find_compare_operator(raw: &str) -> Option<(CompareOp, usize)> {
    let mut in_string = false;
    let mut escaped = false;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let chars: Vec<(usize, char)> = raw.char_indices().collect();
    let mut cursor = 0;
    while cursor < chars.len() {
        let (index, ch) = chars[cursor];
        if escaped {
            escaped = false;
            cursor += 1;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            cursor += 1;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            cursor += 1;
            continue;
        }
        if in_string {
            cursor += 1;
            continue;
        }
        match ch {
            '(' => {
                paren_depth += 1;
                cursor += 1;
                continue;
            }
            ')' => {
                paren_depth = paren_depth.saturating_sub(1);
                cursor += 1;
                continue;
            }
            '{' => {
                brace_depth += 1;
                cursor += 1;
                continue;
            }
            '}' => {
                brace_depth = brace_depth.saturating_sub(1);
                cursor += 1;
                continue;
            }
            '[' => {
                bracket_depth += 1;
                cursor += 1;
                continue;
            }
            ']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                cursor += 1;
                continue;
            }
            _ => {}
        }
        if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 {
            if ch == '<'
                && let Some(close_angle) = find_matching_angle(raw, index)
                && matches!(
                    raw[close_angle + 1..].trim_start().chars().next(),
                    Some('(' | '{')
                )
            {
                cursor += 1;
                while cursor < chars.len() && chars[cursor].0 <= close_angle {
                    cursor += 1;
                }
                continue;
            }
            if let Some((_, next)) = chars.get(cursor + 1) {
                match (ch, *next) {
                    ('=', '=') => return Some((CompareOp::Eq, index)),
                    ('!', '=') => return Some((CompareOp::Ne, index)),
                    ('<', '=') => return Some((CompareOp::Le, index)),
                    ('>', '=') => return Some((CompareOp::Ge, index)),
                    _ => {}
                }
            }
            match ch {
                '<' => return Some((CompareOp::Lt, index)),
                '>' => return Some((CompareOp::Gt, index)),
                _ => {}
            }
        }
        cursor += 1;
    }
    None
}

fn skip_blank_lines(lines: &[&str], index: &mut usize) {
    while *index < lines.len() && lines[*index].trim().is_empty() {
        *index += 1;
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

impl ArithmeticOp {
    pub fn lexeme(self) -> &'static str {
        match self {
            ArithmeticOp::Add => "+",
            ArithmeticOp::Sub => "-",
            ArithmeticOp::Mul => "*",
            ArithmeticOp::Div => "/",
        }
    }
}

#[cfg(test)]
mod ast_identity_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn top_level_ast_items_have_stable_file_spans_and_ids() {
        let source = r#"
import "./nested.ax"
pub struct Agent {
    id: int
}
const LIMIT: int = 7
fn run(): int {
    let value: int = LIMIT
    return value
}
"#;
        let program = parse_program(source, Path::new("root.ax")).expect("parse program");

        let import = &program.imports[0];
        assert_eq!(import.line, 2);
        assert_eq!(import.column, 1);
        assert_eq!(
            import.span_in(&program.path),
            SourceSpan::new("root.ax", 2, 1)
        );
        assert_eq!(
            import.stable_id_in(&program.path),
            AstNodeId("root.ax:2:1:import:./nested.ax".to_string())
        );

        let structure = &program.structs[0];
        assert_eq!(
            structure.span_in(&program.path),
            SourceSpan::new("root.ax", 3, 1)
        );
        assert_eq!(
            structure.stable_id_in(&program.path),
            AstNodeId("root.ax:3:1:struct:Agent".to_string())
        );

        let constant = &program.consts[0];
        assert_eq!(
            constant.span_in(&program.path),
            SourceSpan::new("root.ax", 6, 1)
        );
        assert_eq!(
            constant.stable_id_in(&program.path),
            AstNodeId("root.ax:6:1:const:LIMIT".to_string())
        );

        let function = &program.functions[0];
        assert_eq!(function.span(), SourceSpan::new("root.ax", 7, 1));
        assert_eq!(
            function.stable_id(),
            AstNodeId("root.ax:7:1:function:run".to_string())
        );
        assert_eq!(
            function.body[0].span_in(&program.path),
            SourceSpan::new("root.ax", 8, 1)
        );
    }

    #[test]
    fn nested_module_item_ids_are_stable_across_repeated_parses() {
        let source = r#"
pub enum ResultKind {
    Ok(int)
}
"#;
        let first = parse_program(source, Path::new("modules/nested.ax")).expect("first parse");
        let second = parse_program(source, Path::new("modules/nested.ax")).expect("second parse");

        assert_eq!(
            first.enums[0].span_in(&first.path),
            SourceSpan::new("modules/nested.ax", 2, 1)
        );
        assert_eq!(
            first.enums[0].stable_id_in(&first.path),
            AstNodeId("modules/nested.ax:2:1:enum:ResultKind".to_string())
        );
        assert_eq!(
            first.enums[0].stable_id_in(&first.path),
            second.enums[0].stable_id_in(&second.path)
        );
    }
}
