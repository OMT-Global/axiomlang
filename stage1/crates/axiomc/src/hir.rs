use crate::diagnostics::{Diagnostic, message_with_suggestion};
use crate::manifest::{CapabilityConfig, CapabilityKind};
use crate::syntax;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Program {
    pub path: String,
    pub structs: Vec<StructDef>,
    pub enums: Vec<EnumDef>,
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
    },
    BinaryAdd {
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
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum Type {
    Error,
    Int,
    Bool,
    String,
    Struct(String),
    Enum(String),
    Ptr(Box<Type>),
    MutPtr(Box<Type>),
    Slice(Box<Type>),
    MutSlice(Box<Type>),
    Option(Box<Type>),
    Result(Box<Type>, Box<Type>),
    Tuple(Vec<Type>),
    Map(Box<Type>, Box<Type>),
    Array(Box<Type>),
    Task(Box<Type>),
    JoinHandle(Box<Type>),
    AsyncChannel(Box<Type>),
    SelectResult(Box<Type>),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum LiteralValue {
    Int(i64),
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StructFieldValue {
    pub name: String,
    pub expr: Expr,
}

#[derive(Debug, Clone)]
struct Binding {
    ty: Type,
    moved: bool,
    moved_projections: HashSet<ProjectionPath>,
    borrow_kind: Option<BorrowKind>,
    borrow_origin: Option<BorrowOrigin>,
    borrowed_owners: HashSet<String>,
    active_borrow_count: usize,
    active_mut_borrow_count: usize,
}

type ProjectionPath = Vec<ProjectionSegment>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ProjectionSegment {
    Field(String),
    TupleIndex(usize),
}

#[derive(Debug, Clone)]
struct FunctionSig {
    params: Vec<Type>,
    return_ty: Type,
    borrow_return_params: Vec<usize>,
    is_extern: bool,
}

#[derive(Debug, Clone)]
struct MethodSig {
    function_name: String,
    params: Vec<Type>,
    return_ty: Type,
    has_self: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BorrowOrigin {
    Param(String),
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BorrowKind {
    Shared,
    Mutable,
}

struct LowerContext<'a> {
    structs: &'a HashMap<String, StructDef>,
    enums: &'a HashMap<String, EnumDef>,
    aliases: &'a HashMap<String, syntax::TypeAliasDecl>,
    variants: &'a HashMap<String, Vec<VariantInfo>>,
    functions: &'a HashMap<String, FunctionSig>,
    methods: &'a HashMap<String, HashMap<String, MethodSig>>,
    capabilities: &'a CapabilityConfig,
    current_return: Option<Type>,
    current_borrow_return_params: HashSet<String>,
}

#[derive(Debug, Clone)]
struct VariantInfo {
    enum_name: String,
    payload_tys: Vec<Type>,
    payload_names: Vec<String>,
}

const OWNERSHIP_LOOP_MOVE_OUTER_NON_COPY: &str = "loop_move_outer_non_copy";
const OWNERSHIP_BORROW_RETURN_REQUIRES_PARAM_ORIGIN: &str = "borrow_return_requires_param_origin";
const OWNERSHIP_MOVE_WHILE_BORROWED: &str = "move_while_borrowed";
const OWNERSHIP_USE_AFTER_MOVE: &str = "use_after_move";
const OWNERSHIP_SHARED_BORROW_WHILE_MUTABLE_LIVE: &str = "shared_borrow_while_mutable_live";
const OWNERSHIP_MUTABLE_BORROW_WHILE_MUTABLE_LIVE: &str = "mutable_borrow_while_mutable_live";
const OWNERSHIP_MUTABLE_BORROW_WHILE_SHARED_LIVE: &str = "mutable_borrow_while_shared_live";

fn function_symbol_name(function: &syntax::Function) -> String {
    match &function.impl_target {
        Some(target) => format!("{target}__{}", function.name),
        None => function.name.clone(),
    }
}

fn method_owner_name(ty: &Type) -> Option<&str> {
    match ty {
        Type::Struct(name) | Type::Enum(name) => Some(name.as_str()),
        _ => None,
    }
}

pub fn lower(program: &syntax::Program) -> Result<Program, Diagnostic> {
    let capabilities = CapabilityConfig::default();
    lower_with_capabilities(program, &capabilities)
}

pub fn lower_with_capabilities(
    program: &syntax::Program,
    capabilities: &CapabilityConfig,
) -> Result<Program, Diagnostic> {
    lower_with_capabilities_recovery(program, capabilities).map_err(primary_diagnostic)
}

pub fn lower_with_capabilities_recovery(
    program: &syntax::Program,
    capabilities: &CapabilityConfig,
) -> Result<Program, Vec<Diagnostic>> {
    lower_with_capabilities_impl(program, capabilities, true)
}

fn lower_with_capabilities_impl(
    program: &syntax::Program,
    capabilities: &CapabilityConfig,
    recover: bool,
) -> Result<Program, Vec<Diagnostic>> {
    let program = monomorphize_program(program).map_err(single_diagnostic)?;
    let (struct_names, enum_names, aliases) =
        collect_type_names(&program.structs, &program.enums, &program.type_aliases)
            .map_err(single_diagnostic)?;
    let (enums, variants) =
        collect_enum_definitions(&program.enums, &struct_names, &enum_names, &aliases)
            .map_err(single_diagnostic)?;
    let structs = collect_struct_definitions(&program.structs, &enum_names, &aliases)
        .map_err(single_diagnostic)?;
    validate_recursive_type_cycles(
        &program,
        &structs,
        &enums,
        &struct_names,
        &enum_names,
        &aliases,
    )
    .map_err(single_diagnostic)?;
    let functions = collect_function_signatures(&program.functions, &structs, &enums, &aliases)
        .map_err(single_diagnostic)?;
    let methods = collect_method_signatures(&program.functions, &structs, &enums, &aliases)
        .map_err(single_diagnostic)?;
    let mut diagnostics = Vec::new();
    let mut lowered_structs = structs.values().cloned().collect::<Vec<_>>();
    lowered_structs.sort_by(|lhs, rhs| lhs.name.cmp(&rhs.name));
    let mut lowered_enums = enums.values().cloned().collect::<Vec<_>>();
    lowered_enums.sort_by(|lhs, rhs| lhs.name.cmp(&rhs.name));
    let reachable_functions = reachable_function_names(&program);
    let mut lowered_functions = Vec::new();
    for function in &program.functions {
        if function.name.starts_with("std_fs_") && !reachable_functions.contains(&function.name) {
            continue;
        }
        match lower_function(
            function,
            &structs,
            &enums,
            &aliases,
            &variants,
            &functions,
            &methods,
            capabilities,
        ) {
            Ok(function) => lowered_functions.push(function),
            Err(error) if recover => append_diagnostic(&mut diagnostics, error),
            Err(error) => return Err(single_diagnostic(error)),
        }
    }
    let ctx = LowerContext {
        structs: &structs,
        enums: &enums,
        aliases: &aliases,
        variants: &variants,
        functions: &functions,
        methods: &methods,
        capabilities,
        current_return: None,
        current_borrow_return_params: HashSet::new(),
    };
    let mut env = HashMap::new();
    let stmts = if recover {
        let (stmts, mut block_diagnostics, _) =
            lower_block_recovering(&program.stmts, &mut env, &ctx);
        diagnostics.append(&mut block_diagnostics);
        stmts
    } else {
        lower_block(&program.stmts, &mut env, &ctx)
            .map_err(single_diagnostic)?
            .0
    };
    if !diagnostics.is_empty() {
        sort_diagnostics(&mut diagnostics);
        return Err(diagnostics);
    }
    Ok(Program {
        path: program.path.clone(),
        structs: lowered_structs,
        enums: lowered_enums,
        functions: lowered_functions,
        stmts,
    })
}

fn primary_diagnostic(mut diagnostics: Vec<Diagnostic>) -> Diagnostic {
    sort_diagnostics(&mut diagnostics);
    let mut first = diagnostics.remove(0);
    first.related = diagnostics;
    first
}

fn single_diagnostic(diagnostic: Diagnostic) -> Vec<Diagnostic> {
    vec![diagnostic]
}

fn append_diagnostic(diagnostics: &mut Vec<Diagnostic>, mut diagnostic: Diagnostic) {
    diagnostics.append(&mut diagnostic.related);
    diagnostics.push(diagnostic);
}

fn sort_diagnostics(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| left.column.cmp(&right.column))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.message.cmp(&right.message))
    });
}

fn reachable_function_names(program: &syntax::Program) -> HashSet<String> {
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
        syntax::Expr::BinaryAdd { lhs, rhs, .. } | syntax::Expr::BinaryCompare { lhs, rhs, .. } => {
            collect_expr_calls(lhs, calls);
            collect_expr_calls(rhs, calls);
        }
        syntax::Expr::Try { expr, .. } | syntax::Expr::Await { expr, .. } => {
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
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum AggregateRef {
    Struct(String),
    Enum(String),
}

fn validate_recursive_type_cycles(
    program: &syntax::Program,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    syntax_structs: &HashMap<String, syntax::StructDecl>,
    syntax_enums: &HashMap<String, ()>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
) -> Result<(), Diagnostic> {
    for struct_decl in &program.structs {
        let owner = AggregateRef::Struct(struct_decl.name.clone());
        for field in &struct_decl.fields {
            let ty = lower_type(
                &field.ty,
                syntax_structs,
                syntax_enums,
                aliases,
                field.line,
                field.column,
            )?;
            if type_has_unboxed_recursive_path(&ty, &owner, structs, enums, &mut HashSet::new()) {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "recursive field {:?} in struct {:?} requires indirection; unboxed recursive types are not supported",
                        field.name, struct_decl.name
                    ),
                )
                .with_span(field.line, field.column));
            }
        }
    }

    for enum_decl in &program.enums {
        let owner = AggregateRef::Enum(enum_decl.name.clone());
        for variant in &enum_decl.variants {
            for payload_ty in &variant.payload_tys {
                let ty = lower_type(
                    payload_ty,
                    syntax_structs,
                    syntax_enums,
                    aliases,
                    variant.line,
                    variant.column,
                )?;
                if type_has_unboxed_recursive_path(&ty, &owner, structs, enums, &mut HashSet::new())
                {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "recursive payload variant {:?} in enum {:?} requires indirection; unboxed recursive types are not supported",
                            variant.name, enum_decl.name
                        ),
                    )
                    .with_span(variant.line, variant.column));
                }
            }
        }
    }

    Ok(())
}

fn type_has_unboxed_recursive_path(
    ty: &Type,
    owner: &AggregateRef,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    visiting: &mut HashSet<AggregateRef>,
) -> bool {
    match ty {
        Type::Error | Type::Int | Type::Bool | Type::String | Type::Ptr(_) | Type::MutPtr(_) => {
            false
        }
        Type::Struct(name) => {
            let current = AggregateRef::Struct(name.clone());
            if &current == owner {
                return true;
            }
            if !visiting.insert(current.clone()) {
                return false;
            }
            let result = structs
                .get(name)
                .map(|struct_def| {
                    struct_def.fields.iter().any(|field| {
                        type_has_unboxed_recursive_path(&field.ty, owner, structs, enums, visiting)
                    })
                })
                .unwrap_or(false);
            visiting.remove(&current);
            result
        }
        Type::Enum(name) => {
            let current = AggregateRef::Enum(name.clone());
            if &current == owner {
                return true;
            }
            if !visiting.insert(current.clone()) {
                return false;
            }
            let result = enums
                .get(name)
                .map(|enum_def| {
                    enum_def.variants.iter().any(|variant| {
                        variant.payload_tys.iter().any(|payload_ty| {
                            type_has_unboxed_recursive_path(
                                payload_ty, owner, structs, enums, visiting,
                            )
                        })
                    })
                })
                .unwrap_or(false);
            visiting.remove(&current);
            result
        }
        Type::Slice(_) | Type::MutSlice(_) | Type::Map(_, _) | Type::Array(_) => false,
        Type::Option(inner)
        | Type::Task(inner)
        | Type::JoinHandle(inner)
        | Type::AsyncChannel(inner)
        | Type::SelectResult(inner) => {
            type_has_unboxed_recursive_path(inner, owner, structs, enums, visiting)
        }
        Type::Result(ok, err) => {
            type_has_unboxed_recursive_path(ok, owner, structs, enums, visiting)
                || type_has_unboxed_recursive_path(err, owner, structs, enums, visiting)
        }
        Type::Tuple(elements) => elements.iter().any(|element| {
            type_has_unboxed_recursive_path(element, owner, structs, enums, visiting)
        }),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GenericInstantiation {
    name: String,
    type_args: Vec<syntax::TypeName>,
}

fn infer_generic_call_type_args(
    program: &syntax::Program,
    generic_functions: &HashMap<String, syntax::Function>,
) -> Result<syntax::Program, Diagnostic> {
    let mut inferred = program.clone();
    inferred.functions = program
        .functions
        .iter()
        .map(|function| infer_generic_calls_in_function(function, generic_functions))
        .collect::<Result<Vec<_>, _>>()?;
    let mut env = HashMap::new();
    inferred.stmts =
        infer_generic_calls_in_stmts(&program.stmts, &mut env, None, generic_functions)?;
    Ok(inferred)
}

fn infer_generic_calls_in_function(
    function: &syntax::Function,
    generic_functions: &HashMap<String, syntax::Function>,
) -> Result<syntax::Function, Diagnostic> {
    let mut env = HashMap::new();
    for param in &function.params {
        env.insert(param.name.clone(), param.ty.clone());
    }
    let mut inferred = function.clone();
    inferred.body = infer_generic_calls_in_stmts(
        &function.body,
        &mut env,
        Some(&function.return_ty),
        generic_functions,
    )?;
    Ok(inferred)
}

fn infer_generic_calls_in_stmts(
    stmts: &[syntax::Stmt],
    env: &mut HashMap<String, syntax::TypeName>,
    return_ty: Option<&syntax::TypeName>,
    generic_functions: &HashMap<String, syntax::Function>,
) -> Result<Vec<syntax::Stmt>, Diagnostic> {
    let mut inferred = Vec::new();
    for stmt in stmts {
        inferred.push(infer_generic_calls_in_stmt(
            stmt,
            env,
            return_ty,
            generic_functions,
        )?);
    }
    Ok(inferred)
}

fn infer_generic_calls_in_stmt(
    stmt: &syntax::Stmt,
    env: &mut HashMap<String, syntax::TypeName>,
    return_ty: Option<&syntax::TypeName>,
    generic_functions: &HashMap<String, syntax::Function>,
) -> Result<syntax::Stmt, Diagnostic> {
    Ok(match stmt {
        syntax::Stmt::Let {
            name,
            ty,
            expr,
            line,
            column,
        } => {
            let expr = infer_generic_calls_in_expr(expr, Some(ty), env, generic_functions)?;
            env.insert(name.clone(), ty.clone());
            syntax::Stmt::Let {
                name: name.clone(),
                ty: ty.clone(),
                expr,
                line: *line,
                column: *column,
            }
        }
        syntax::Stmt::Print { expr, line, column } => syntax::Stmt::Print {
            expr: infer_generic_calls_in_expr(expr, None, env, generic_functions)?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Panic { expr, line, column } => syntax::Stmt::Panic {
            expr: infer_generic_calls_in_expr(
                expr,
                Some(&syntax::TypeName::String),
                env,
                generic_functions,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Defer { expr, line, column } => syntax::Stmt::Defer {
            expr: infer_generic_calls_in_expr(expr, None, env, generic_functions)?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::If {
            cond,
            then_block,
            else_block,
            line,
            column,
        } => {
            let mut then_env = env.clone();
            let mut else_env = env.clone();
            syntax::Stmt::If {
                cond: infer_generic_calls_in_expr(
                    cond,
                    Some(&syntax::TypeName::Bool),
                    env,
                    generic_functions,
                )?,
                then_block: infer_generic_calls_in_stmts(
                    then_block,
                    &mut then_env,
                    return_ty,
                    generic_functions,
                )?,
                else_block: else_block
                    .as_ref()
                    .map(|block| {
                        infer_generic_calls_in_stmts(
                            block,
                            &mut else_env,
                            return_ty,
                            generic_functions,
                        )
                    })
                    .transpose()?,
                line: *line,
                column: *column,
            }
        }
        syntax::Stmt::While {
            cond,
            body,
            line,
            column,
        } => {
            let mut body_env = env.clone();
            syntax::Stmt::While {
                cond: infer_generic_calls_in_expr(
                    cond,
                    Some(&syntax::TypeName::Bool),
                    env,
                    generic_functions,
                )?,
                body: infer_generic_calls_in_stmts(
                    body,
                    &mut body_env,
                    return_ty,
                    generic_functions,
                )?,
                line: *line,
                column: *column,
            }
        }
        syntax::Stmt::Match {
            expr,
            arms,
            line,
            column,
        } => syntax::Stmt::Match {
            expr: infer_generic_calls_in_expr(expr, None, env, generic_functions)?,
            arms: arms
                .iter()
                .map(|arm| {
                    let mut arm_env = env.clone();
                    Ok(syntax::MatchArm {
                        variant: arm.variant.clone(),
                        bindings: arm.bindings.clone(),
                        is_named: arm.is_named,
                        body: infer_generic_calls_in_stmts(
                            &arm.body,
                            &mut arm_env,
                            return_ty,
                            generic_functions,
                        )?,
                        line: arm.line,
                        column: arm.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Return { expr, line, column } => syntax::Stmt::Return {
            expr: infer_generic_calls_in_expr(expr, return_ty, env, generic_functions)?,
            line: *line,
            column: *column,
        },
    })
}

fn infer_generic_calls_in_expr(
    expr: &syntax::Expr,
    expected: Option<&syntax::TypeName>,
    env: &HashMap<String, syntax::TypeName>,
    generic_functions: &HashMap<String, syntax::Function>,
) -> Result<syntax::Expr, Diagnostic> {
    Ok(match expr {
        syntax::Expr::Call {
            name,
            type_args,
            args,
            line,
            column,
        } => {
            let mut type_args = type_args.clone();
            if type_args.is_empty() {
                if let Some(template) = generic_functions.get(name) {
                    type_args = infer_type_args_for_call(
                        template,
                        args,
                        expected,
                        env,
                        generic_functions,
                        *line,
                        *column,
                    )?;
                }
            }
            let param_expected = generic_functions.get(name).and_then(|template| {
                if type_args.len() == template.type_params.len() {
                    Some(generic_type_bindings(template, &type_args).ok()?)
                } else {
                    None
                }
            });
            let args = args
                .iter()
                .enumerate()
                .map(|(index, arg)| {
                    let expected_arg = generic_functions
                        .get(name)
                        .and_then(|template| template.params.get(index))
                        .and_then(|param| {
                            param_expected
                                .as_ref()
                                .map(|bindings| substitute_type_name(&param.ty, bindings))
                        });
                    infer_generic_calls_in_expr(arg, expected_arg.as_ref(), env, generic_functions)
                })
                .collect::<Result<Vec<_>, _>>()?;
            syntax::Expr::Call {
                name: name.clone(),
                type_args,
                args,
                line: *line,
                column: *column,
            }
        }
        syntax::Expr::MethodCall {
            base,
            method,
            type_args,
            args,
            line,
            column,
        } => syntax::Expr::MethodCall {
            base: Box::new(infer_generic_calls_in_expr(
                base,
                None,
                env,
                generic_functions,
            )?),
            method: method.clone(),
            type_args: type_args.clone(),
            args: args
                .iter()
                .map(|arg| infer_generic_calls_in_expr(arg, None, env, generic_functions))
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::BinaryAdd {
            lhs,
            rhs,
            line,
            column,
        } => syntax::Expr::BinaryAdd {
            lhs: Box::new(infer_generic_calls_in_expr(
                lhs,
                expected,
                env,
                generic_functions,
            )?),
            rhs: Box::new(infer_generic_calls_in_expr(
                rhs,
                expected,
                env,
                generic_functions,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::BinaryCompare {
            op,
            lhs,
            rhs,
            line,
            column,
        } => syntax::Expr::BinaryCompare {
            op: *op,
            lhs: Box::new(infer_generic_calls_in_expr(
                lhs,
                None,
                env,
                generic_functions,
            )?),
            rhs: Box::new(infer_generic_calls_in_expr(
                rhs,
                None,
                env,
                generic_functions,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Try { expr, line, column } => syntax::Expr::Try {
            expr: Box::new(infer_generic_calls_in_expr(
                expr,
                None,
                env,
                generic_functions,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Await { expr, line, column } => syntax::Expr::Await {
            expr: Box::new(infer_generic_calls_in_expr(
                expr,
                None,
                env,
                generic_functions,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::StructLiteral {
            name,
            fields,
            line,
            column,
        } => syntax::Expr::StructLiteral {
            name: name.clone(),
            fields: fields
                .iter()
                .map(|field| {
                    Ok(syntax::StructFieldValue {
                        name: field.name.clone(),
                        expr: infer_generic_calls_in_expr(
                            &field.expr,
                            None,
                            env,
                            generic_functions,
                        )?,
                        line: field.line,
                        column: field.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::FieldAccess {
            base,
            field,
            line,
            column,
        } => syntax::Expr::FieldAccess {
            base: Box::new(infer_generic_calls_in_expr(
                base,
                None,
                env,
                generic_functions,
            )?),
            field: field.clone(),
            line: *line,
            column: *column,
        },
        syntax::Expr::TupleLiteral {
            elements,
            line,
            column,
        } => syntax::Expr::TupleLiteral {
            elements: elements
                .iter()
                .map(|element| infer_generic_calls_in_expr(element, None, env, generic_functions))
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::TupleIndex {
            base,
            index,
            line,
            column,
        } => syntax::Expr::TupleIndex {
            base: Box::new(infer_generic_calls_in_expr(
                base,
                None,
                env,
                generic_functions,
            )?),
            index: *index,
            line: *line,
            column: *column,
        },
        syntax::Expr::MapLiteral {
            entries,
            line,
            column,
        } => syntax::Expr::MapLiteral {
            entries: entries
                .iter()
                .map(|entry| {
                    Ok(syntax::MapEntry {
                        key: infer_generic_calls_in_expr(&entry.key, None, env, generic_functions)?,
                        value: infer_generic_calls_in_expr(
                            &entry.value,
                            None,
                            env,
                            generic_functions,
                        )?,
                        line: entry.line,
                        column: entry.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::ArrayLiteral {
            elements,
            line,
            column,
        } => syntax::Expr::ArrayLiteral {
            elements: elements
                .iter()
                .map(|element| infer_generic_calls_in_expr(element, None, env, generic_functions))
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::Slice {
            base,
            start,
            end,
            line,
            column,
        } => syntax::Expr::Slice {
            base: Box::new(infer_generic_calls_in_expr(
                base,
                None,
                env,
                generic_functions,
            )?),
            start: start
                .as_ref()
                .map(|expr| {
                    infer_generic_calls_in_expr(
                        expr,
                        Some(&syntax::TypeName::Int),
                        env,
                        generic_functions,
                    )
                    .map(Box::new)
                })
                .transpose()?,
            end: end
                .as_ref()
                .map(|expr| {
                    infer_generic_calls_in_expr(
                        expr,
                        Some(&syntax::TypeName::Int),
                        env,
                        generic_functions,
                    )
                    .map(Box::new)
                })
                .transpose()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::Index {
            base,
            index,
            line,
            column,
        } => syntax::Expr::Index {
            base: Box::new(infer_generic_calls_in_expr(
                base,
                None,
                env,
                generic_functions,
            )?),
            index: Box::new(infer_generic_calls_in_expr(
                index,
                Some(&syntax::TypeName::Int),
                env,
                generic_functions,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Literal(_) | syntax::Expr::VarRef { .. } => expr.clone(),
    })
}

fn infer_type_args_for_call(
    template: &syntax::Function,
    args: &[syntax::Expr],
    expected: Option<&syntax::TypeName>,
    env: &HashMap<String, syntax::TypeName>,
    generic_functions: &HashMap<String, syntax::Function>,
    line: usize,
    column: usize,
) -> Result<Vec<syntax::TypeName>, Diagnostic> {
    let mut bindings = HashMap::new();
    let type_params = template.type_params.iter().cloned().collect::<HashSet<_>>();
    for (index, (param, arg)) in template.params.iter().zip(args).enumerate() {
        if let Some(arg_ty) = infer_expr_type_name(arg, None, env, generic_functions) {
            unify_generic_type_name(
                &param.ty,
                &arg_ty,
                &type_params,
                &mut bindings,
                line,
                column,
            )
            .map_err(|error| {
                Diagnostic::new(
                    "type",
                    format!(
                        "generic function {:?} argument {} constraint failed: {}",
                        template.name,
                        index + 1,
                        error.message
                    ),
                )
                .with_span(line, column)
            })?;
        }
    }
    if let Some(expected) = expected {
        unify_generic_type_name(
            &template.return_ty,
            expected,
            &type_params,
            &mut bindings,
            line,
            column,
        )
        .map_err(|error| {
            Diagnostic::new(
                "type",
                format!(
                    "generic function {:?} return constraint failed: {}",
                    template.name, error.message
                ),
            )
            .with_span(line, column)
        })?;
    }
    template
        .type_params
        .iter()
        .map(|param| {
            bindings.get(param).cloned().ok_or_else(|| {
                Diagnostic::new(
                    "type",
                    format!(
                        "generic function {:?} could not infer type parameter {:?}",
                        template.name, param
                    ),
                )
                .with_span(line, column)
            })
        })
        .collect()
}

fn infer_expr_type_name(
    expr: &syntax::Expr,
    expected: Option<&syntax::TypeName>,
    env: &HashMap<String, syntax::TypeName>,
    generic_functions: &HashMap<String, syntax::Function>,
) -> Option<syntax::TypeName> {
    match expr {
        syntax::Expr::Literal(syntax::Literal::Int(_)) => Some(syntax::TypeName::Int),
        syntax::Expr::Literal(syntax::Literal::Bool(_)) => Some(syntax::TypeName::Bool),
        syntax::Expr::Literal(syntax::Literal::String(_)) => Some(syntax::TypeName::String),
        syntax::Expr::VarRef { name, .. } => {
            if (name == "None" || name == "Some" || name == "Ok" || name == "Err")
                && expected.is_some()
            {
                expected.cloned()
            } else {
                env.get(name).cloned()
            }
        }
        syntax::Expr::Call {
            name,
            type_args,
            args,
            ..
        } => {
            if let Some(template) = generic_functions.get(name) {
                let inferred_args = if type_args.is_empty() {
                    infer_type_args_for_call(template, args, expected, env, generic_functions, 0, 0)
                        .ok()?
                } else {
                    type_args.clone()
                };
                let bindings = generic_type_bindings(template, &inferred_args).ok()?;
                Some(substitute_type_name(&template.return_ty, &bindings))
            } else {
                expected.cloned()
            }
        }
        syntax::Expr::ArrayLiteral { elements, .. } => elements
            .first()
            .and_then(|element| infer_expr_type_name(element, None, env, generic_functions))
            .map(|inner| syntax::TypeName::Array(Box::new(inner))),
        syntax::Expr::Slice { base, .. } => {
            match infer_expr_type_name(base, None, env, generic_functions)? {
                syntax::TypeName::Array(inner)
                | syntax::TypeName::Slice(inner)
                | syntax::TypeName::MutSlice(inner) => Some(syntax::TypeName::Slice(inner)),
                other => Some(other),
            }
        }
        syntax::Expr::Index { base, .. } => {
            match infer_expr_type_name(base, None, env, generic_functions)? {
                syntax::TypeName::Array(inner)
                | syntax::TypeName::Slice(inner)
                | syntax::TypeName::MutSlice(inner) => Some(*inner),
                syntax::TypeName::Map(_, value) => Some(*value),
                _ => None,
            }
        }
        syntax::Expr::TupleLiteral { elements, .. } => elements
            .iter()
            .map(|element| infer_expr_type_name(element, None, env, generic_functions))
            .collect::<Option<Vec<_>>>()
            .map(syntax::TypeName::Tuple),
        syntax::Expr::Try { expr, .. } | syntax::Expr::Await { expr, .. } => {
            infer_expr_type_name(expr, expected, env, generic_functions)
        }
        _ => expected.cloned(),
    }
}

fn generic_constraint_mismatch(
    pattern: &syntax::TypeName,
    actual: &syntax::TypeName,
    line: usize,
    column: usize,
) -> Diagnostic {
    Diagnostic::new(
        "type",
        format!("expected generic constraint {pattern:?}, got {actual:?}"),
    )
    .with_span(line, column)
}

fn contains_generic_type_param(ty: &syntax::TypeName, type_params: &HashSet<String>) -> bool {
    match ty {
        syntax::TypeName::Named(name, args) => {
            (args.is_empty() && type_params.contains(name))
                || args
                    .iter()
                    .any(|arg| contains_generic_type_param(arg, type_params))
        }
        syntax::TypeName::Ptr(inner)
        | syntax::TypeName::MutPtr(inner)
        | syntax::TypeName::Slice(inner)
        | syntax::TypeName::MutSlice(inner)
        | syntax::TypeName::Option(inner)
        | syntax::TypeName::Array(inner) => contains_generic_type_param(inner, type_params),
        syntax::TypeName::Result(ok, err) | syntax::TypeName::Map(ok, err) => {
            contains_generic_type_param(ok, type_params)
                || contains_generic_type_param(err, type_params)
        }
        syntax::TypeName::Tuple(elements) => elements
            .iter()
            .any(|element| contains_generic_type_param(element, type_params)),
        syntax::TypeName::Int | syntax::TypeName::Bool | syntax::TypeName::String => false,
    }
}

fn unify_generic_type_name(
    pattern: &syntax::TypeName,
    actual: &syntax::TypeName,
    type_params: &HashSet<String>,
    bindings: &mut HashMap<String, syntax::TypeName>,
    line: usize,
    column: usize,
) -> Result<(), Diagnostic> {
    match pattern {
        syntax::TypeName::Named(name, args) if args.is_empty() && type_params.contains(name) => {
            if let Some(bound) = bindings.get(name) {
                if bound != actual {
                    return Err(Diagnostic::new(
                        "type",
                        format!("generic type parameter {name:?} inferred as both {bound:?} and {actual:?}"),
                    ).with_span(line, column));
                }
            } else {
                bindings.insert(name.clone(), actual.clone());
            }
            Ok(())
        }
        syntax::TypeName::Named(lhs_name, lhs_args) => match actual {
            syntax::TypeName::Named(rhs_name, rhs_args)
                if lhs_name == rhs_name && lhs_args.len() == rhs_args.len() =>
            {
                for (lhs, rhs) in lhs_args.iter().zip(rhs_args) {
                    unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)?;
                }
                Ok(())
            }
            _ if contains_generic_type_param(pattern, type_params) => {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            }
            _ => Ok(()),
        },
        syntax::TypeName::Ptr(lhs) => {
            if let syntax::TypeName::Ptr(rhs) = actual {
                unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::MutPtr(lhs) => {
            if let syntax::TypeName::MutPtr(rhs) = actual {
                unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::Slice(lhs) => {
            if let syntax::TypeName::Slice(rhs) = actual {
                unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::MutSlice(lhs) => {
            if let syntax::TypeName::MutSlice(rhs) = actual {
                unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::Option(lhs) => {
            if let syntax::TypeName::Option(rhs) = actual {
                unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::Result(lhs_ok, lhs_err) => {
            if let syntax::TypeName::Result(rhs_ok, rhs_err) = actual {
                unify_generic_type_name(lhs_ok, rhs_ok, type_params, bindings, line, column)?;
                unify_generic_type_name(lhs_err, rhs_err, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::Tuple(lhs) => {
            if let syntax::TypeName::Tuple(rhs) = actual {
                if lhs.len() != rhs.len() && contains_generic_type_param(pattern, type_params) {
                    return Err(generic_constraint_mismatch(pattern, actual, line, column));
                }
                for (lhs, rhs) in lhs.iter().zip(rhs) {
                    unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)?;
                }
                Ok(())
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::Map(lhs_key, lhs_value) => {
            if let syntax::TypeName::Map(rhs_key, rhs_value) = actual {
                unify_generic_type_name(lhs_key, rhs_key, type_params, bindings, line, column)?;
                unify_generic_type_name(lhs_value, rhs_value, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::Array(lhs) => {
            if let syntax::TypeName::Array(rhs) = actual {
                unify_generic_type_name(lhs, rhs, type_params, bindings, line, column)
            } else if contains_generic_type_param(pattern, type_params) {
                Err(generic_constraint_mismatch(pattern, actual, line, column))
            } else {
                Ok(())
            }
        }
        syntax::TypeName::Int | syntax::TypeName::Bool | syntax::TypeName::String => Ok(()),
    }
}

fn monomorphize_program(program: &syntax::Program) -> Result<syntax::Program, Diagnostic> {
    let mut generic_functions = HashMap::new();
    let mut seen_function_names = HashSet::new();

    for function in &program.functions {
        if !seen_function_names.insert(function.name.clone()) {
            return Err(
                Diagnostic::new("type", format!("duplicate function {:?}", function.name))
                    .with_span(function.line, function.column),
            );
        }
        if !function.type_params.is_empty() {
            validate_generic_function(function)?;
            generic_functions.insert(function.name.clone(), function.clone());
        }
    }

    let program = infer_generic_call_type_args(program, &generic_functions)?;
    let mut generic_functions = HashMap::new();
    let mut concrete_functions = Vec::new();
    for function in &program.functions {
        if function.type_params.is_empty() {
            concrete_functions.push(function.clone());
        } else {
            generic_functions.insert(function.name.clone(), function.clone());
        }
    }

    let mut queue = VecDeque::new();
    let mut queued = HashSet::new();
    let mut lowered_functions = Vec::new();
    for function in &concrete_functions {
        lowered_functions.push(rewrite_function_generic_calls(
            function,
            &HashMap::new(),
            &generic_functions,
            &mut queue,
            &mut queued,
        )?);
    }
    let stmts = program
        .stmts
        .iter()
        .map(|stmt| {
            rewrite_stmt_generic_calls(
                stmt,
                &HashMap::new(),
                &generic_functions,
                &mut queue,
                &mut queued,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut emitted = HashSet::new();
    while let Some(instantiation) = queue.pop_front() {
        if !emitted.insert(instantiation.clone()) {
            continue;
        }
        let template = generic_functions
            .get(&instantiation.name)
            .expect("queued generic instantiations must reference templates");
        let type_bindings = generic_type_bindings(template, &instantiation.type_args)?;
        let mut function = template.clone();
        function.name = monomorphized_function_name(&template.name, &instantiation.type_args);
        function.type_params = Vec::new();
        function.params = template
            .params
            .iter()
            .map(|param| syntax::Param {
                name: param.name.clone(),
                ty: substitute_type_name(&param.ty, &type_bindings),
                line: param.line,
                column: param.column,
            })
            .collect();
        function.return_ty = substitute_type_name(&template.return_ty, &type_bindings);
        function.body = template
            .body
            .iter()
            .map(|stmt| {
                rewrite_stmt_generic_calls(
                    stmt,
                    &type_bindings,
                    &generic_functions,
                    &mut queue,
                    &mut queued,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        lowered_functions.push(function);
    }

    monomorphize_aggregates(syntax::Program {
        path: program.path.clone(),
        imports: program.imports.clone(),
        consts: program.consts.clone(),
        type_aliases: program.type_aliases.clone(),
        structs: program.structs.clone(),
        enums: program.enums.clone(),
        functions: lowered_functions,
        stmts,
    })
}

fn monomorphize_aggregates(program: syntax::Program) -> Result<syntax::Program, Diagnostic> {
    let mut generic_structs = HashMap::new();
    let mut concrete_structs = Vec::new();
    let mut seen_struct_names = HashSet::new();
    for struct_decl in &program.structs {
        if !seen_struct_names.insert(struct_decl.name.clone()) {
            return Err(Diagnostic::new(
                "type",
                format!("duplicate struct {:?}", struct_decl.name),
            )
            .with_span(struct_decl.line, struct_decl.column));
        }
        if struct_decl.type_params.is_empty() {
            concrete_structs.push(struct_decl.clone());
        } else {
            validate_generic_struct(struct_decl)?;
            generic_structs.insert(struct_decl.name.clone(), struct_decl.clone());
        }
    }

    let mut generic_enums = HashMap::new();
    let mut concrete_enums = Vec::new();
    let mut seen_enum_names = HashSet::new();
    for enum_decl in &program.enums {
        if !seen_enum_names.insert(enum_decl.name.clone()) {
            return Err(
                Diagnostic::new("type", format!("duplicate enum {:?}", enum_decl.name))
                    .with_span(enum_decl.line, enum_decl.column),
            );
        }
        if enum_decl.type_params.is_empty() {
            concrete_enums.push(enum_decl.clone());
        } else {
            validate_generic_enum(enum_decl)?;
            generic_enums.insert(enum_decl.name.clone(), enum_decl.clone());
        }
    }

    let mut queue = VecDeque::new();
    let mut queued = HashSet::new();
    let mut type_aliases = Vec::new();
    for alias in &program.type_aliases {
        type_aliases.push(syntax::TypeAliasDecl {
            name: alias.name.clone(),
            ty: rewrite_aggregate_type_name(
                &alias.ty,
                &generic_structs,
                &generic_enums,
                &mut queue,
                &mut queued,
                alias.line,
                alias.column,
            )?,
            visibility: alias.visibility,
            line: alias.line,
            column: alias.column,
        });
    }
    let consts = program
        .consts
        .iter()
        .map(|constant| {
            Ok(syntax::ConstDecl {
                name: constant.name.clone(),
                ty: rewrite_aggregate_type_name(
                    &constant.ty,
                    &generic_structs,
                    &generic_enums,
                    &mut queue,
                    &mut queued,
                    constant.line,
                    constant.column,
                )?,
                expr: rewrite_expr_aggregate_types(
                    &constant.expr,
                    &generic_structs,
                    &generic_enums,
                    &mut queue,
                    &mut queued,
                )?,
                visibility: constant.visibility,
                line: constant.line,
                column: constant.column,
            })
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    let structs = concrete_structs
        .iter()
        .map(|struct_decl| {
            rewrite_struct_decl_aggregate_types(
                struct_decl,
                &HashMap::new(),
                &generic_structs,
                &generic_enums,
                &mut queue,
                &mut queued,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let enums = concrete_enums
        .iter()
        .map(|enum_decl| {
            rewrite_enum_decl_aggregate_types(
                enum_decl,
                &HashMap::new(),
                &generic_structs,
                &generic_enums,
                &mut queue,
                &mut queued,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let functions = program
        .functions
        .iter()
        .map(|function| {
            rewrite_function_aggregate_types(
                function,
                &generic_structs,
                &generic_enums,
                &mut queue,
                &mut queued,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let stmts = program
        .stmts
        .iter()
        .map(|stmt| {
            rewrite_stmt_aggregate_types(
                stmt,
                &generic_structs,
                &generic_enums,
                &mut queue,
                &mut queued,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut structs = structs;
    let mut enums = enums;
    let mut emitted = HashSet::new();
    while let Some(instantiation) = queue.pop_front() {
        if !emitted.insert(instantiation.clone()) {
            continue;
        }
        if let Some(template) = generic_structs.get(&instantiation.name) {
            let type_bindings = generic_decl_type_bindings(
                &template.name,
                &template.type_params,
                &instantiation.type_args,
                template.line,
                template.column,
            )?;
            let mut lowered = rewrite_struct_decl_aggregate_types(
                template,
                &type_bindings,
                &generic_structs,
                &generic_enums,
                &mut queue,
                &mut queued,
            )?;
            lowered.name = monomorphized_type_name(&template.name, &instantiation.type_args);
            lowered.type_params = Vec::new();
            structs.push(lowered);
            continue;
        }
        if let Some(template) = generic_enums.get(&instantiation.name) {
            let type_bindings = generic_decl_type_bindings(
                &template.name,
                &template.type_params,
                &instantiation.type_args,
                template.line,
                template.column,
            )?;
            let mut lowered = rewrite_enum_decl_aggregate_types(
                template,
                &type_bindings,
                &generic_structs,
                &generic_enums,
                &mut queue,
                &mut queued,
            )?;
            lowered.name = monomorphized_type_name(&template.name, &instantiation.type_args);
            lowered.type_params = Vec::new();
            enums.push(lowered);
        }
    }

    Ok(syntax::Program {
        path: program.path,
        imports: program.imports,
        consts,
        type_aliases,
        structs,
        enums,
        functions,
        stmts,
    })
}

fn validate_generic_function(function: &syntax::Function) -> Result<(), Diagnostic> {
    let mut constrained = HashSet::new();
    for param in &function.params {
        collect_type_params(&param.ty, &function.type_params, &mut constrained);
    }
    collect_type_params(&function.return_ty, &function.type_params, &mut constrained);
    for type_param in &function.type_params {
        if !constrained.contains(type_param) {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "generic function {:?} has unconstrained type parameter {:?}",
                    function.name, type_param
                ),
            )
            .with_span(function.line, function.column));
        }
    }
    Ok(())
}

fn validate_generic_struct(struct_decl: &syntax::StructDecl) -> Result<(), Diagnostic> {
    let mut constrained = HashSet::new();
    for field in &struct_decl.fields {
        collect_type_params(&field.ty, &struct_decl.type_params, &mut constrained);
    }
    validate_all_type_params_constrained(
        "struct",
        &struct_decl.name,
        &struct_decl.type_params,
        &constrained,
        struct_decl.line,
        struct_decl.column,
    )
}

fn validate_generic_enum(enum_decl: &syntax::EnumDecl) -> Result<(), Diagnostic> {
    let mut constrained = HashSet::new();
    for variant in &enum_decl.variants {
        for ty in &variant.payload_tys {
            collect_type_params(ty, &enum_decl.type_params, &mut constrained);
        }
    }
    validate_all_type_params_constrained(
        "enum",
        &enum_decl.name,
        &enum_decl.type_params,
        &constrained,
        enum_decl.line,
        enum_decl.column,
    )
}

fn validate_all_type_params_constrained(
    kind: &str,
    name: &str,
    type_params: &[String],
    constrained: &HashSet<String>,
    line: usize,
    column: usize,
) -> Result<(), Diagnostic> {
    for type_param in type_params {
        if !constrained.contains(type_param) {
            return Err(Diagnostic::new(
                "type",
                format!("generic {kind} {name:?} has unconstrained type parameter {type_param:?}"),
            )
            .with_span(line, column));
        }
    }
    Ok(())
}

fn collect_type_params(ty: &syntax::TypeName, type_params: &[String], found: &mut HashSet<String>) {
    match ty {
        syntax::TypeName::Named(name, args)
            if args.is_empty() && type_params.iter().any(|param| param == name) =>
        {
            found.insert(name.clone());
        }
        syntax::TypeName::Named(_, args) => {
            for arg in args {
                collect_type_params(arg, type_params, found);
            }
        }
        syntax::TypeName::Ptr(inner)
        | syntax::TypeName::MutPtr(inner)
        | syntax::TypeName::Slice(inner)
        | syntax::TypeName::MutSlice(inner)
        | syntax::TypeName::Option(inner)
        | syntax::TypeName::Array(inner) => collect_type_params(inner, type_params, found),
        syntax::TypeName::Result(ok, err) | syntax::TypeName::Map(ok, err) => {
            collect_type_params(ok, type_params, found);
            collect_type_params(err, type_params, found);
        }
        syntax::TypeName::Tuple(elements) => {
            for element in elements {
                collect_type_params(element, type_params, found);
            }
        }
        syntax::TypeName::Int | syntax::TypeName::Bool | syntax::TypeName::String => {}
    }
}

fn generic_type_bindings(
    function: &syntax::Function,
    type_args: &[syntax::TypeName],
) -> Result<HashMap<String, syntax::TypeName>, Diagnostic> {
    if type_args.len() != function.type_params.len() {
        return Err(Diagnostic::new(
            "type",
            format!(
                "generic function {:?} expects {} type arguments, got {}",
                function.name,
                function.type_params.len(),
                type_args.len()
            ),
        )
        .with_span(function.line, function.column));
    }
    Ok(function
        .type_params
        .iter()
        .cloned()
        .zip(type_args.iter().cloned())
        .collect())
}

fn generic_decl_type_bindings(
    name: &str,
    type_params: &[String],
    type_args: &[syntax::TypeName],
    line: usize,
    column: usize,
) -> Result<HashMap<String, syntax::TypeName>, Diagnostic> {
    if type_args.len() != type_params.len() {
        return Err(Diagnostic::new(
            "type",
            format!(
                "generic type {:?} expects {} type arguments, got {}",
                name,
                type_params.len(),
                type_args.len()
            ),
        )
        .with_span(line, column));
    }
    Ok(type_params
        .iter()
        .cloned()
        .zip(type_args.iter().cloned())
        .collect())
}

fn rewrite_struct_decl_aggregate_types(
    struct_decl: &syntax::StructDecl,
    type_bindings: &HashMap<String, syntax::TypeName>,
    generic_structs: &HashMap<String, syntax::StructDecl>,
    generic_enums: &HashMap<String, syntax::EnumDecl>,
    queue: &mut VecDeque<GenericInstantiation>,
    queued: &mut HashSet<GenericInstantiation>,
) -> Result<syntax::StructDecl, Diagnostic> {
    Ok(syntax::StructDecl {
        name: struct_decl.name.clone(),
        type_params: struct_decl.type_params.clone(),
        fields: struct_decl
            .fields
            .iter()
            .map(|field| {
                Ok(syntax::StructField {
                    name: field.name.clone(),
                    ty: rewrite_aggregate_type_name(
                        &substitute_type_name(&field.ty, type_bindings),
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                        field.line,
                        field.column,
                    )?,
                    line: field.line,
                    column: field.column,
                })
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?,
        visibility: struct_decl.visibility,
        line: struct_decl.line,
        column: struct_decl.column,
    })
}

fn rewrite_enum_decl_aggregate_types(
    enum_decl: &syntax::EnumDecl,
    type_bindings: &HashMap<String, syntax::TypeName>,
    generic_structs: &HashMap<String, syntax::StructDecl>,
    generic_enums: &HashMap<String, syntax::EnumDecl>,
    queue: &mut VecDeque<GenericInstantiation>,
    queued: &mut HashSet<GenericInstantiation>,
) -> Result<syntax::EnumDecl, Diagnostic> {
    Ok(syntax::EnumDecl {
        name: enum_decl.name.clone(),
        type_params: enum_decl.type_params.clone(),
        variants: enum_decl
            .variants
            .iter()
            .map(|variant| {
                Ok(syntax::EnumVariantDecl {
                    name: variant.name.clone(),
                    payload_tys: variant
                        .payload_tys
                        .iter()
                        .map(|ty| {
                            rewrite_aggregate_type_name(
                                &substitute_type_name(ty, type_bindings),
                                generic_structs,
                                generic_enums,
                                queue,
                                queued,
                                variant.line,
                                variant.column,
                            )
                        })
                        .collect::<Result<Vec<_>, Diagnostic>>()?,
                    payload_names: variant.payload_names.clone(),
                    line: variant.line,
                    column: variant.column,
                })
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?,
        visibility: enum_decl.visibility,
        line: enum_decl.line,
        column: enum_decl.column,
    })
}

fn rewrite_function_aggregate_types(
    function: &syntax::Function,
    generic_structs: &HashMap<String, syntax::StructDecl>,
    generic_enums: &HashMap<String, syntax::EnumDecl>,
    queue: &mut VecDeque<GenericInstantiation>,
    queued: &mut HashSet<GenericInstantiation>,
) -> Result<syntax::Function, Diagnostic> {
    Ok(syntax::Function {
        name: function.name.clone(),
        source_name: function.source_name.clone(),
        path: function.path.clone(),
        type_params: function.type_params.clone(),
        params: function
            .params
            .iter()
            .map(|param| {
                Ok(syntax::Param {
                    name: param.name.clone(),
                    ty: rewrite_aggregate_type_name(
                        &param.ty,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                        param.line,
                        param.column,
                    )?,
                    line: param.line,
                    column: param.column,
                })
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?,
        return_ty: rewrite_aggregate_type_name(
            &function.return_ty,
            generic_structs,
            generic_enums,
            queue,
            queued,
            function.line,
            function.column,
        )?,
        body: function
            .body
            .iter()
            .map(|stmt| {
                rewrite_stmt_aggregate_types(stmt, generic_structs, generic_enums, queue, queued)
            })
            .collect::<Result<Vec<_>, _>>()?,
        is_async: function.is_async,
        is_extern: function.is_extern,
        extern_abi: function.extern_abi.clone(),
        extern_library: function.extern_library.clone(),
        visibility: function.visibility,
        receiver: function.receiver,
        impl_target: function.impl_target.clone(),
        line: function.line,
        column: function.column,
    })
}

fn rewrite_aggregate_type_name(
    ty: &syntax::TypeName,
    generic_structs: &HashMap<String, syntax::StructDecl>,
    generic_enums: &HashMap<String, syntax::EnumDecl>,
    queue: &mut VecDeque<GenericInstantiation>,
    queued: &mut HashSet<GenericInstantiation>,
    line: usize,
    column: usize,
) -> Result<syntax::TypeName, Diagnostic> {
    Ok(match ty {
        syntax::TypeName::Named(name, args) => {
            let args = args
                .iter()
                .map(|arg| {
                    rewrite_aggregate_type_name(
                        arg,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                        line,
                        column,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            if is_async_runtime_type(name) {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("async runtime type {name:?} expects 1 type argument"),
                    )
                    .with_span(line, column));
                }
                return Ok(syntax::TypeName::Named(name.clone(), args));
            }
            let is_generic = generic_structs.contains_key(name) || generic_enums.contains_key(name);
            if args.is_empty() {
                if is_generic {
                    return Err(Diagnostic::new(
                        "type",
                        format!("generic type {name:?} requires explicit type arguments"),
                    )
                    .with_span(line, column));
                }
                syntax::TypeName::Named(name.clone(), Vec::new())
            } else {
                let type_params = generic_structs
                    .get(name)
                    .map(|decl| decl.type_params.as_slice())
                    .or_else(|| {
                        generic_enums
                            .get(name)
                            .map(|decl| decl.type_params.as_slice())
                    })
                    .ok_or_else(|| {
                        Diagnostic::new("type", format!("type {name:?} is not generic"))
                            .with_span(line, column)
                    })?;
                generic_decl_type_bindings(name, type_params, &args, line, column)?;
                let instantiation = GenericInstantiation {
                    name: name.clone(),
                    type_args: args.clone(),
                };
                if queued.insert(instantiation.clone()) {
                    queue.push_back(instantiation);
                }
                syntax::TypeName::Named(monomorphized_type_name(name, &args), Vec::new())
            }
        }
        syntax::TypeName::Ptr(inner) => {
            syntax::TypeName::Ptr(Box::new(rewrite_aggregate_type_name(
                inner,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?))
        }
        syntax::TypeName::MutPtr(inner) => {
            syntax::TypeName::MutPtr(Box::new(rewrite_aggregate_type_name(
                inner,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?))
        }
        syntax::TypeName::Slice(inner) => {
            syntax::TypeName::Slice(Box::new(rewrite_aggregate_type_name(
                inner,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?))
        }
        syntax::TypeName::MutSlice(inner) => {
            syntax::TypeName::MutSlice(Box::new(rewrite_aggregate_type_name(
                inner,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?))
        }
        syntax::TypeName::Option(inner) => {
            syntax::TypeName::Option(Box::new(rewrite_aggregate_type_name(
                inner,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?))
        }
        syntax::TypeName::Result(ok, err) => syntax::TypeName::Result(
            Box::new(rewrite_aggregate_type_name(
                ok,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?),
            Box::new(rewrite_aggregate_type_name(
                err,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?),
        ),
        syntax::TypeName::Tuple(elements) => syntax::TypeName::Tuple(
            elements
                .iter()
                .map(|element| {
                    rewrite_aggregate_type_name(
                        element,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                        line,
                        column,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        syntax::TypeName::Map(key, value) => syntax::TypeName::Map(
            Box::new(rewrite_aggregate_type_name(
                key,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?),
            Box::new(rewrite_aggregate_type_name(
                value,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?),
        ),
        syntax::TypeName::Array(inner) => {
            syntax::TypeName::Array(Box::new(rewrite_aggregate_type_name(
                inner,
                generic_structs,
                generic_enums,
                queue,
                queued,
                line,
                column,
            )?))
        }
        syntax::TypeName::Int => syntax::TypeName::Int,
        syntax::TypeName::Bool => syntax::TypeName::Bool,
        syntax::TypeName::String => syntax::TypeName::String,
    })
}

fn rewrite_function_generic_calls(
    function: &syntax::Function,
    type_bindings: &HashMap<String, syntax::TypeName>,
    generic_functions: &HashMap<String, syntax::Function>,
    queue: &mut VecDeque<GenericInstantiation>,
    queued: &mut HashSet<GenericInstantiation>,
) -> Result<syntax::Function, Diagnostic> {
    let mut rewritten = function.clone();
    rewritten.body = function
        .body
        .iter()
        .map(|stmt| {
            rewrite_stmt_generic_calls(stmt, type_bindings, generic_functions, queue, queued)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rewritten)
}

fn rewrite_stmt_aggregate_types(
    stmt: &syntax::Stmt,
    generic_structs: &HashMap<String, syntax::StructDecl>,
    generic_enums: &HashMap<String, syntax::EnumDecl>,
    queue: &mut VecDeque<GenericInstantiation>,
    queued: &mut HashSet<GenericInstantiation>,
) -> Result<syntax::Stmt, Diagnostic> {
    Ok(match stmt {
        syntax::Stmt::Let {
            name,
            ty,
            expr,
            line,
            column,
        } => syntax::Stmt::Let {
            name: name.clone(),
            ty: rewrite_aggregate_type_name(
                ty,
                generic_structs,
                generic_enums,
                queue,
                queued,
                *line,
                *column,
            )?,
            expr: rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Print { expr, line, column } => syntax::Stmt::Print {
            expr: rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Panic { expr, line, column } => syntax::Stmt::Panic {
            expr: rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Defer { expr, line, column } => syntax::Stmt::Defer {
            expr: rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::If {
            cond,
            then_block,
            else_block,
            line,
            column,
        } => syntax::Stmt::If {
            cond: rewrite_expr_aggregate_types(
                cond,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?,
            then_block: then_block
                .iter()
                .map(|stmt| {
                    rewrite_stmt_aggregate_types(
                        stmt,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            else_block: else_block
                .as_ref()
                .map(|block| {
                    block
                        .iter()
                        .map(|stmt| {
                            rewrite_stmt_aggregate_types(
                                stmt,
                                generic_structs,
                                generic_enums,
                                queue,
                                queued,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::While {
            cond,
            body,
            line,
            column,
        } => syntax::Stmt::While {
            cond: rewrite_expr_aggregate_types(
                cond,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?,
            body: body
                .iter()
                .map(|stmt| {
                    rewrite_stmt_aggregate_types(
                        stmt,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Match {
            expr,
            arms,
            line,
            column,
        } => syntax::Stmt::Match {
            expr: rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?,
            arms: arms
                .iter()
                .map(|arm| {
                    Ok(syntax::MatchArm {
                        variant: arm.variant.clone(),
                        bindings: arm.bindings.clone(),
                        is_named: arm.is_named,
                        body: arm
                            .body
                            .iter()
                            .map(|stmt| {
                                rewrite_stmt_aggregate_types(
                                    stmt,
                                    generic_structs,
                                    generic_enums,
                                    queue,
                                    queued,
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                        line: arm.line,
                        column: arm.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Return { expr, line, column } => syntax::Stmt::Return {
            expr: rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
    })
}

fn rewrite_expr_aggregate_types(
    expr: &syntax::Expr,
    generic_structs: &HashMap<String, syntax::StructDecl>,
    generic_enums: &HashMap<String, syntax::EnumDecl>,
    queue: &mut VecDeque<GenericInstantiation>,
    queued: &mut HashSet<GenericInstantiation>,
) -> Result<syntax::Expr, Diagnostic> {
    Ok(match expr {
        syntax::Expr::Literal(_) | syntax::Expr::VarRef { .. } => expr.clone(),
        syntax::Expr::Call {
            name,
            type_args,
            args,
            line,
            column,
        } => syntax::Expr::Call {
            name: name.clone(),
            type_args: type_args
                .iter()
                .map(|type_arg| {
                    rewrite_aggregate_type_name(
                        type_arg,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                        *line,
                        *column,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            args: args
                .iter()
                .map(|arg| {
                    rewrite_expr_aggregate_types(arg, generic_structs, generic_enums, queue, queued)
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::MethodCall {
            base,
            method,
            type_args,
            args,
            line,
            column,
        } => syntax::Expr::MethodCall {
            base: Box::new(rewrite_expr_aggregate_types(
                base,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            method: method.clone(),
            type_args: type_args
                .iter()
                .map(|type_arg| {
                    rewrite_aggregate_type_name(
                        type_arg,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                        *line,
                        *column,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            args: args
                .iter()
                .map(|arg| {
                    rewrite_expr_aggregate_types(arg, generic_structs, generic_enums, queue, queued)
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::BinaryAdd {
            lhs,
            rhs,
            line,
            column,
        } => syntax::Expr::BinaryAdd {
            lhs: Box::new(rewrite_expr_aggregate_types(
                lhs,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            rhs: Box::new(rewrite_expr_aggregate_types(
                rhs,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::BinaryCompare {
            op,
            lhs,
            rhs,
            line,
            column,
        } => syntax::Expr::BinaryCompare {
            op: *op,
            lhs: Box::new(rewrite_expr_aggregate_types(
                lhs,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            rhs: Box::new(rewrite_expr_aggregate_types(
                rhs,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Try { expr, line, column } => syntax::Expr::Try {
            expr: Box::new(rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Await { expr, line, column } => syntax::Expr::Await {
            expr: Box::new(rewrite_expr_aggregate_types(
                expr,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::StructLiteral {
            name,
            fields,
            line,
            column,
        } => syntax::Expr::StructLiteral {
            name: name.clone(),
            fields: fields
                .iter()
                .map(|field| {
                    Ok(syntax::StructFieldValue {
                        name: field.name.clone(),
                        expr: rewrite_expr_aggregate_types(
                            &field.expr,
                            generic_structs,
                            generic_enums,
                            queue,
                            queued,
                        )?,
                        line: field.line,
                        column: field.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::FieldAccess {
            base,
            field,
            line,
            column,
        } => syntax::Expr::FieldAccess {
            base: Box::new(rewrite_expr_aggregate_types(
                base,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            field: field.clone(),
            line: *line,
            column: *column,
        },
        syntax::Expr::TupleLiteral {
            elements,
            line,
            column,
        } => syntax::Expr::TupleLiteral {
            elements: elements
                .iter()
                .map(|element| {
                    rewrite_expr_aggregate_types(
                        element,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::TupleIndex {
            base,
            index,
            line,
            column,
        } => syntax::Expr::TupleIndex {
            base: Box::new(rewrite_expr_aggregate_types(
                base,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            index: *index,
            line: *line,
            column: *column,
        },
        syntax::Expr::MapLiteral {
            entries,
            line,
            column,
        } => syntax::Expr::MapLiteral {
            entries: entries
                .iter()
                .map(|entry| {
                    Ok(syntax::MapEntry {
                        key: rewrite_expr_aggregate_types(
                            &entry.key,
                            generic_structs,
                            generic_enums,
                            queue,
                            queued,
                        )?,
                        value: rewrite_expr_aggregate_types(
                            &entry.value,
                            generic_structs,
                            generic_enums,
                            queue,
                            queued,
                        )?,
                        line: entry.line,
                        column: entry.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::ArrayLiteral {
            elements,
            line,
            column,
        } => syntax::Expr::ArrayLiteral {
            elements: elements
                .iter()
                .map(|element| {
                    rewrite_expr_aggregate_types(
                        element,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::Slice {
            base,
            start,
            end,
            line,
            column,
        } => syntax::Expr::Slice {
            base: Box::new(rewrite_expr_aggregate_types(
                base,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            start: start
                .as_ref()
                .map(|expr| {
                    rewrite_expr_aggregate_types(
                        expr,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                    )
                    .map(Box::new)
                })
                .transpose()?,
            end: end
                .as_ref()
                .map(|expr| {
                    rewrite_expr_aggregate_types(
                        expr,
                        generic_structs,
                        generic_enums,
                        queue,
                        queued,
                    )
                    .map(Box::new)
                })
                .transpose()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::Index {
            base,
            index,
            line,
            column,
        } => syntax::Expr::Index {
            base: Box::new(rewrite_expr_aggregate_types(
                base,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            index: Box::new(rewrite_expr_aggregate_types(
                index,
                generic_structs,
                generic_enums,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
    })
}

fn rewrite_stmt_generic_calls(
    stmt: &syntax::Stmt,
    type_bindings: &HashMap<String, syntax::TypeName>,
    generic_functions: &HashMap<String, syntax::Function>,
    queue: &mut VecDeque<GenericInstantiation>,
    queued: &mut HashSet<GenericInstantiation>,
) -> Result<syntax::Stmt, Diagnostic> {
    Ok(match stmt {
        syntax::Stmt::Let {
            name,
            ty,
            expr,
            line,
            column,
        } => syntax::Stmt::Let {
            name: name.clone(),
            ty: substitute_type_name(ty, type_bindings),
            expr: rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Print { expr, line, column } => syntax::Stmt::Print {
            expr: rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Panic { expr, line, column } => syntax::Stmt::Panic {
            expr: match expr {
                syntax::Expr::Call {
                    name,
                    type_args,
                    args,
                    line,
                    column,
                } if name == "panic" => syntax::Expr::Call {
                    name: name.clone(),
                    type_args: type_args
                        .iter()
                        .map(|type_arg| substitute_type_name(type_arg, type_bindings))
                        .collect(),
                    args: args
                        .iter()
                        .map(|arg| {
                            rewrite_expr_generic_calls(
                                arg,
                                type_bindings,
                                generic_functions,
                                queue,
                                queued,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    line: *line,
                    column: *column,
                },
                _ => rewrite_expr_generic_calls(
                    expr,
                    type_bindings,
                    generic_functions,
                    queue,
                    queued,
                )?,
            },
            line: *line,
            column: *column,
        },
        syntax::Stmt::Defer { expr, line, column } => syntax::Stmt::Defer {
            expr: rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::If {
            cond,
            then_block,
            else_block,
            line,
            column,
        } => syntax::Stmt::If {
            cond: rewrite_expr_generic_calls(
                cond,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?,
            then_block: then_block
                .iter()
                .map(|stmt| {
                    rewrite_stmt_generic_calls(
                        stmt,
                        type_bindings,
                        generic_functions,
                        queue,
                        queued,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            else_block: else_block
                .as_ref()
                .map(|block| {
                    block
                        .iter()
                        .map(|stmt| {
                            rewrite_stmt_generic_calls(
                                stmt,
                                type_bindings,
                                generic_functions,
                                queue,
                                queued,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::While {
            cond,
            body,
            line,
            column,
        } => syntax::Stmt::While {
            cond: rewrite_expr_generic_calls(
                cond,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?,
            body: body
                .iter()
                .map(|stmt| {
                    rewrite_stmt_generic_calls(
                        stmt,
                        type_bindings,
                        generic_functions,
                        queue,
                        queued,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Match {
            expr,
            arms,
            line,
            column,
        } => syntax::Stmt::Match {
            expr: rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?,
            arms: arms
                .iter()
                .map(|arm| {
                    Ok(syntax::MatchArm {
                        variant: arm.variant.clone(),
                        bindings: arm.bindings.clone(),
                        is_named: arm.is_named,
                        body: arm
                            .body
                            .iter()
                            .map(|stmt| {
                                rewrite_stmt_generic_calls(
                                    stmt,
                                    type_bindings,
                                    generic_functions,
                                    queue,
                                    queued,
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                        line: arm.line,
                        column: arm.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Return { expr, line, column } => syntax::Stmt::Return {
            expr: rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?,
            line: *line,
            column: *column,
        },
    })
}

fn rewrite_expr_generic_calls(
    expr: &syntax::Expr,
    type_bindings: &HashMap<String, syntax::TypeName>,
    generic_functions: &HashMap<String, syntax::Function>,
    queue: &mut VecDeque<GenericInstantiation>,
    queued: &mut HashSet<GenericInstantiation>,
) -> Result<syntax::Expr, Diagnostic> {
    Ok(match expr {
        syntax::Expr::Literal(_) | syntax::Expr::VarRef { .. } => expr.clone(),
        syntax::Expr::Call {
            name,
            type_args,
            args,
            line,
            column,
        } => {
            let args = args
                .iter()
                .map(|arg| {
                    rewrite_expr_generic_calls(arg, type_bindings, generic_functions, queue, queued)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let type_args = type_args
                .iter()
                .map(|type_arg| substitute_type_name(type_arg, type_bindings))
                .collect::<Vec<_>>();
            let name = if let Some(template) = generic_functions.get(name) {
                if type_args.is_empty() {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "generic function {:?} requires explicit type arguments",
                            name
                        ),
                    )
                    .with_span(*line, *column));
                }
                generic_type_bindings(template, &type_args)?;
                let instantiation = GenericInstantiation {
                    name: name.clone(),
                    type_args: type_args.clone(),
                };
                if queued.insert(instantiation.clone()) {
                    queue.push_back(instantiation);
                }
                monomorphized_function_name(name, &type_args)
            } else {
                if !type_args.is_empty() && !is_async_runtime_intrinsic(name) {
                    return Err(Diagnostic::new(
                        "type",
                        format!("function {:?} is not generic", name),
                    )
                    .with_span(*line, *column));
                }
                name.clone()
            };
            let keep_type_args = is_async_runtime_intrinsic(name.as_str());
            syntax::Expr::Call {
                name,
                type_args: if keep_type_args {
                    type_args
                } else {
                    Vec::new()
                },
                args,
                line: *line,
                column: *column,
            }
        }
        syntax::Expr::MethodCall {
            base,
            method,
            type_args,
            args,
            line,
            column,
        } => syntax::Expr::MethodCall {
            base: Box::new(rewrite_expr_generic_calls(
                base,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            method: method.clone(),
            type_args: type_args
                .iter()
                .map(|type_arg| substitute_type_name(type_arg, type_bindings))
                .collect(),
            args: args
                .iter()
                .map(|arg| {
                    rewrite_expr_generic_calls(arg, type_bindings, generic_functions, queue, queued)
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::BinaryAdd {
            lhs,
            rhs,
            line,
            column,
        } => syntax::Expr::BinaryAdd {
            lhs: Box::new(rewrite_expr_generic_calls(
                lhs,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            rhs: Box::new(rewrite_expr_generic_calls(
                rhs,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::BinaryCompare {
            op,
            lhs,
            rhs,
            line,
            column,
        } => syntax::Expr::BinaryCompare {
            op: *op,
            lhs: Box::new(rewrite_expr_generic_calls(
                lhs,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            rhs: Box::new(rewrite_expr_generic_calls(
                rhs,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Try { expr, line, column } => syntax::Expr::Try {
            expr: Box::new(rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Await { expr, line, column } => syntax::Expr::Await {
            expr: Box::new(rewrite_expr_generic_calls(
                expr,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::StructLiteral {
            name,
            fields,
            line,
            column,
        } => syntax::Expr::StructLiteral {
            name: name.clone(),
            fields: fields
                .iter()
                .map(|field| {
                    Ok(syntax::StructFieldValue {
                        name: field.name.clone(),
                        expr: rewrite_expr_generic_calls(
                            &field.expr,
                            type_bindings,
                            generic_functions,
                            queue,
                            queued,
                        )?,
                        line: field.line,
                        column: field.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::FieldAccess {
            base,
            field,
            line,
            column,
        } => syntax::Expr::FieldAccess {
            base: Box::new(rewrite_expr_generic_calls(
                base,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            field: field.clone(),
            line: *line,
            column: *column,
        },
        syntax::Expr::TupleLiteral {
            elements,
            line,
            column,
        } => syntax::Expr::TupleLiteral {
            elements: elements
                .iter()
                .map(|element| {
                    rewrite_expr_generic_calls(
                        element,
                        type_bindings,
                        generic_functions,
                        queue,
                        queued,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::TupleIndex {
            base,
            index,
            line,
            column,
        } => syntax::Expr::TupleIndex {
            base: Box::new(rewrite_expr_generic_calls(
                base,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            index: *index,
            line: *line,
            column: *column,
        },
        syntax::Expr::MapLiteral {
            entries,
            line,
            column,
        } => syntax::Expr::MapLiteral {
            entries: entries
                .iter()
                .map(|entry| {
                    Ok(syntax::MapEntry {
                        key: rewrite_expr_generic_calls(
                            &entry.key,
                            type_bindings,
                            generic_functions,
                            queue,
                            queued,
                        )?,
                        value: rewrite_expr_generic_calls(
                            &entry.value,
                            type_bindings,
                            generic_functions,
                            queue,
                            queued,
                        )?,
                        line: entry.line,
                        column: entry.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::ArrayLiteral {
            elements,
            line,
            column,
        } => syntax::Expr::ArrayLiteral {
            elements: elements
                .iter()
                .map(|element| {
                    rewrite_expr_generic_calls(
                        element,
                        type_bindings,
                        generic_functions,
                        queue,
                        queued,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::Slice {
            base,
            start,
            end,
            line,
            column,
        } => syntax::Expr::Slice {
            base: Box::new(rewrite_expr_generic_calls(
                base,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            start: start
                .as_ref()
                .map(|expr| {
                    rewrite_expr_generic_calls(
                        expr,
                        type_bindings,
                        generic_functions,
                        queue,
                        queued,
                    )
                    .map(Box::new)
                })
                .transpose()?,
            end: end
                .as_ref()
                .map(|expr| {
                    rewrite_expr_generic_calls(
                        expr,
                        type_bindings,
                        generic_functions,
                        queue,
                        queued,
                    )
                    .map(Box::new)
                })
                .transpose()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::Index {
            base,
            index,
            line,
            column,
        } => syntax::Expr::Index {
            base: Box::new(rewrite_expr_generic_calls(
                base,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            index: Box::new(rewrite_expr_generic_calls(
                index,
                type_bindings,
                generic_functions,
                queue,
                queued,
            )?),
            line: *line,
            column: *column,
        },
    })
}

fn substitute_type_name(
    ty: &syntax::TypeName,
    type_bindings: &HashMap<String, syntax::TypeName>,
) -> syntax::TypeName {
    match ty {
        syntax::TypeName::Named(name, args) if args.is_empty() => type_bindings
            .get(name)
            .cloned()
            .unwrap_or_else(|| syntax::TypeName::Named(name.clone(), Vec::new())),
        syntax::TypeName::Named(name, args) => syntax::TypeName::Named(
            name.clone(),
            args.iter()
                .map(|arg| substitute_type_name(arg, type_bindings))
                .collect(),
        ),
        syntax::TypeName::Ptr(inner) => {
            syntax::TypeName::Ptr(Box::new(substitute_type_name(inner, type_bindings)))
        }
        syntax::TypeName::MutPtr(inner) => {
            syntax::TypeName::MutPtr(Box::new(substitute_type_name(inner, type_bindings)))
        }
        syntax::TypeName::Slice(inner) => {
            syntax::TypeName::Slice(Box::new(substitute_type_name(inner, type_bindings)))
        }
        syntax::TypeName::MutSlice(inner) => {
            syntax::TypeName::MutSlice(Box::new(substitute_type_name(inner, type_bindings)))
        }
        syntax::TypeName::Option(inner) => {
            syntax::TypeName::Option(Box::new(substitute_type_name(inner, type_bindings)))
        }
        syntax::TypeName::Result(ok, err) => syntax::TypeName::Result(
            Box::new(substitute_type_name(ok, type_bindings)),
            Box::new(substitute_type_name(err, type_bindings)),
        ),
        syntax::TypeName::Tuple(elements) => syntax::TypeName::Tuple(
            elements
                .iter()
                .map(|element| substitute_type_name(element, type_bindings))
                .collect(),
        ),
        syntax::TypeName::Map(key, value) => syntax::TypeName::Map(
            Box::new(substitute_type_name(key, type_bindings)),
            Box::new(substitute_type_name(value, type_bindings)),
        ),
        syntax::TypeName::Array(inner) => {
            syntax::TypeName::Array(Box::new(substitute_type_name(inner, type_bindings)))
        }
        syntax::TypeName::Int => syntax::TypeName::Int,
        syntax::TypeName::Bool => syntax::TypeName::Bool,
        syntax::TypeName::String => syntax::TypeName::String,
    }
}

fn monomorphized_function_name(name: &str, type_args: &[syntax::TypeName]) -> String {
    monomorphized_name(name, type_args)
}

fn monomorphized_type_name(name: &str, type_args: &[syntax::TypeName]) -> String {
    monomorphized_name(name, type_args)
}

fn monomorphized_name(name: &str, type_args: &[syntax::TypeName]) -> String {
    let suffix = type_args
        .iter()
        .map(type_name_monomorph_suffix)
        .collect::<Vec<_>>()
        .join("__");
    format!("{name}__{suffix}")
}

fn is_async_runtime_type(name: &str) -> bool {
    matches!(
        name,
        "Task" | "JoinHandle" | "AsyncChannel" | "SelectResult"
    )
}

fn is_async_runtime_intrinsic(name: &str) -> bool {
    matches!(
        name,
        "async_ready"
            | "async_spawn"
            | "async_join"
            | "async_cancel"
            | "async_is_canceled"
            | "async_timeout"
            | "async_channel"
            | "async_send"
            | "async_recv"
            | "async_select"
            | "async_selected"
            | "async_selected_value"
    )
}

fn type_name_monomorph_suffix(ty: &syntax::TypeName) -> String {
    match ty {
        syntax::TypeName::Int => String::from("int"),
        syntax::TypeName::Bool => String::from("bool"),
        syntax::TypeName::String => String::from("string"),
        syntax::TypeName::Named(name, args) if args.is_empty() => name.clone(),
        syntax::TypeName::Named(name, args) => monomorphized_type_name(name, args),
        syntax::TypeName::Ptr(inner) => format!("ptr_{}", type_name_monomorph_suffix(inner)),
        syntax::TypeName::MutPtr(inner) => {
            format!("mutptr_{}", type_name_monomorph_suffix(inner))
        }
        syntax::TypeName::Slice(inner) => format!("slice_{}", type_name_monomorph_suffix(inner)),
        syntax::TypeName::MutSlice(inner) => {
            format!("mutslice_{}", type_name_monomorph_suffix(inner))
        }
        syntax::TypeName::Option(inner) => {
            format!("option_{}", type_name_monomorph_suffix(inner))
        }
        syntax::TypeName::Result(ok, err) => format!(
            "result_{}_{}",
            type_name_monomorph_suffix(ok),
            type_name_monomorph_suffix(err)
        ),
        syntax::TypeName::Tuple(elements) => format!(
            "tuple_{}",
            elements
                .iter()
                .map(type_name_monomorph_suffix)
                .collect::<Vec<_>>()
                .join("_")
        ),
        syntax::TypeName::Map(key, value) => format!(
            "map_{}_{}",
            type_name_monomorph_suffix(key),
            type_name_monomorph_suffix(value)
        ),
        syntax::TypeName::Array(inner) => format!("array_{}", type_name_monomorph_suffix(inner)),
    }
}

fn ownership_error(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("ownership", message).with_code(code)
}

impl Type {
    fn is_error(&self) -> bool {
        matches!(self, Type::Error)
    }

    pub fn is_copy(&self) -> bool {
        match self {
            Type::Error
            | Type::Int
            | Type::Bool
            | Type::Ptr(_)
            | Type::MutPtr(_)
            | Type::Slice(_) => true,
            Type::MutSlice(_) => false,
            Type::Option(inner) => inner.is_copy(),
            Type::Result(ok, err) => ok.is_copy() && err.is_copy(),
            Type::Tuple(elements) => elements.iter().all(Type::is_copy),
            Type::String
            | Type::Struct(_)
            | Type::Enum(_)
            | Type::Map(_, _)
            | Type::Array(_)
            | Type::Task(_)
            | Type::JoinHandle(_)
            | Type::AsyncChannel(_)
            | Type::SelectResult(_) => false,
        }
    }

    fn supports_map_key(&self) -> bool {
        match self {
            Type::Int | Type::Bool | Type::String => true,
            Type::Tuple(elements) => elements.iter().all(Type::supports_map_key),
            Type::Error
            | Type::Struct(_)
            | Type::Enum(_)
            | Type::Ptr(_)
            | Type::MutPtr(_)
            | Type::Slice(_)
            | Type::MutSlice(_)
            | Type::Option(_)
            | Type::Result(_, _)
            | Type::Map(_, _)
            | Type::Array(_)
            | Type::Task(_)
            | Type::JoinHandle(_)
            | Type::AsyncChannel(_)
            | Type::SelectResult(_) => false,
        }
    }
}

fn collect_struct_definitions(
    structs: &[syntax::StructDecl],
    enums: &HashMap<String, ()>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
) -> Result<HashMap<String, StructDef>, Diagnostic> {
    let mut names = HashMap::new();
    for struct_decl in structs {
        if enums.contains_key(&struct_decl.name) {
            return Err(Diagnostic::new(
                "type",
                format!("duplicate type name {:?}", struct_decl.name),
            )
            .with_span(struct_decl.line, struct_decl.column));
        }
        if names
            .insert(struct_decl.name.clone(), struct_decl.clone())
            .is_some()
        {
            return Err(Diagnostic::new(
                "type",
                format!("duplicate struct {:?}", struct_decl.name),
            )
            .with_span(struct_decl.line, struct_decl.column));
        }
    }

    let mut lowered = HashMap::new();
    for struct_decl in structs {
        let mut fields = Vec::new();
        let mut seen = HashMap::new();
        for field in &struct_decl.fields {
            if seen.insert(field.name.clone(), ()).is_some() {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "duplicate field {:?} in struct {:?}",
                        field.name, struct_decl.name
                    ),
                )
                .with_span(field.line, field.column));
            }
            let ty = lower_type(&field.ty, &names, enums, aliases, field.line, field.column)?;
            fields.push(StructField {
                name: field.name.clone(),
                ty,
            });
        }
        lowered.insert(
            struct_decl.name.clone(),
            StructDef {
                name: struct_decl.name.clone(),
                fields,
            },
        );
    }
    Ok(lowered)
}

fn collect_type_names(
    structs: &[syntax::StructDecl],
    enums: &[syntax::EnumDecl],
    aliases: &[syntax::TypeAliasDecl],
) -> Result<
    (
        HashMap<String, syntax::StructDecl>,
        HashMap<String, ()>,
        HashMap<String, syntax::TypeAliasDecl>,
    ),
    Diagnostic,
> {
    let mut struct_names = HashMap::new();
    for struct_decl in structs {
        if struct_names
            .insert(struct_decl.name.clone(), struct_decl.clone())
            .is_some()
        {
            return Err(Diagnostic::new(
                "type",
                format!("duplicate struct {:?}", struct_decl.name),
            )
            .with_span(struct_decl.line, struct_decl.column));
        }
    }
    let mut enum_names = HashMap::new();
    for enum_decl in enums {
        if struct_names.contains_key(&enum_decl.name) {
            return Err(Diagnostic::new(
                "type",
                format!("duplicate type name {:?}", enum_decl.name),
            )
            .with_span(enum_decl.line, enum_decl.column));
        }
        if enum_names.insert(enum_decl.name.clone(), ()).is_some() {
            return Err(
                Diagnostic::new("type", format!("duplicate enum {:?}", enum_decl.name))
                    .with_span(enum_decl.line, enum_decl.column),
            );
        }
    }
    let mut alias_names = HashMap::new();
    for type_alias in aliases {
        if struct_names.contains_key(&type_alias.name) || enum_names.contains_key(&type_alias.name)
        {
            return Err(Diagnostic::new(
                "type",
                format!("duplicate type name {:?}", type_alias.name),
            )
            .with_span(type_alias.line, type_alias.column));
        }
        if alias_names
            .insert(type_alias.name.clone(), type_alias.clone())
            .is_some()
        {
            return Err(Diagnostic::new(
                "type",
                format!("duplicate type alias {:?}", type_alias.name),
            )
            .with_span(type_alias.line, type_alias.column));
        }
    }
    Ok((struct_names, enum_names, alias_names))
}

fn collect_enum_definitions(
    enums: &[syntax::EnumDecl],
    structs: &HashMap<String, syntax::StructDecl>,
    enum_names: &HashMap<String, ()>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
) -> Result<(HashMap<String, EnumDef>, HashMap<String, Vec<VariantInfo>>), Diagnostic> {
    let mut lowered = HashMap::new();
    let mut variants = HashMap::new();
    for enum_decl in enums {
        if enum_decl.variants.is_empty() {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "enum {:?} must declare at least one variant",
                    enum_decl.name
                ),
            )
            .with_span(enum_decl.line, enum_decl.column));
        }
        let mut seen = HashMap::new();
        let mut lowered_variants = Vec::new();
        for variant in &enum_decl.variants {
            if seen.insert(variant.name.clone(), ()).is_some() {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "duplicate variant {:?} in enum {:?}",
                        variant.name, enum_decl.name
                    ),
                )
                .with_span(variant.line, variant.column));
            }
            variants
                .entry(variant.name.clone())
                .or_insert_with(Vec::new)
                .push(VariantInfo {
                    enum_name: enum_decl.name.clone(),
                    payload_tys: variant
                        .payload_tys
                        .iter()
                        .map(|ty| {
                            lower_type(
                                ty,
                                structs,
                                enum_names,
                                aliases,
                                variant.line,
                                variant.column,
                            )
                        })
                        .collect::<Result<Vec<_>, Diagnostic>>()?,
                    payload_names: variant.payload_names.clone(),
                });
            let payload_tys = variant
                .payload_tys
                .iter()
                .map(|ty| {
                    lower_type(
                        ty,
                        structs,
                        enum_names,
                        aliases,
                        variant.line,
                        variant.column,
                    )
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            if !variant.payload_names.is_empty() && variant.payload_names.len() != payload_tys.len()
            {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "internal error: enum variant {:?} has mismatched named payload metadata",
                        variant.name
                    ),
                )
                .with_span(variant.line, variant.column));
            }
            let mut seen_payload_names = HashMap::new();
            for payload_name in &variant.payload_names {
                if seen_payload_names
                    .insert(payload_name.clone(), ())
                    .is_some()
                {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "duplicate payload field {:?} in enum variant {:?}",
                            payload_name, variant.name
                        ),
                    )
                    .with_span(variant.line, variant.column));
                }
            }
            lowered_variants.push(EnumVariantDef {
                name: variant.name.clone(),
                payload_tys,
                payload_names: variant.payload_names.clone(),
            });
        }
        lowered.insert(
            enum_decl.name.clone(),
            EnumDef {
                name: enum_decl.name.clone(),
                variants: lowered_variants,
            },
        );
    }
    Ok((lowered, variants))
}

fn collect_function_signatures(
    functions: &[syntax::Function],
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
) -> Result<HashMap<String, FunctionSig>, Diagnostic> {
    let mut signatures = HashMap::new();
    for function in functions {
        let return_ty = lower_type(
            &function.return_ty,
            structs,
            enums,
            aliases,
            function.line,
            function.column,
        )?;
        let mut params = Vec::new();
        if let Some(target_name) = &function.impl_target {
            let self_ty = lower_type(
                &syntax::TypeName::Named(target_name.clone(), Vec::new()),
                structs,
                enums,
                aliases,
                function.line,
                function.column,
            )?;
            if function.receiver.is_some() {
                params.push(self_ty);
            }
        }
        for param in &function.params {
            params.push(lower_type(
                &param.ty,
                structs,
                enums,
                aliases,
                param.line,
                param.column,
            )?);
        }
        let signature_return_ty = if function.is_async {
            Type::Task(Box::new(return_ty.clone()))
        } else {
            return_ty.clone()
        };
        let borrow_return_params = classify_borrow_return(
            &params,
            &signature_return_ty,
            structs,
            enums,
            function.line,
            function.column,
        )?;
        if signatures
            .insert(
                function_symbol_name(function),
                FunctionSig {
                    params,
                    return_ty: signature_return_ty,
                    borrow_return_params,
                    is_extern: function.is_extern,
                },
            )
            .is_some()
        {
            return Err(
                Diagnostic::new("type", format!("duplicate function {:?}", function.name))
                    .with_span(function.line, function.column),
            );
        }
    }
    Ok(signatures)
}

fn collect_method_signatures(
    functions: &[syntax::Function],
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
) -> Result<HashMap<String, HashMap<String, MethodSig>>, Diagnostic> {
    let mut methods: HashMap<String, HashMap<String, MethodSig>> = HashMap::new();
    for function in functions {
        let Some(target_name) = &function.impl_target else {
            continue;
        };
        if !function.type_params.is_empty() {
            return Err(Diagnostic::new(
                "type",
                format!("impl method {:?} cannot be generic yet", function.name),
            )
            .with_span(function.line, function.column));
        }
        let target_ty = lower_type(
            &syntax::TypeName::Named(target_name.clone(), Vec::new()),
            structs,
            enums,
            aliases,
            function.line,
            function.column,
        )?;
        if !matches!(target_ty, Type::Struct(_) | Type::Enum(_)) {
            return Err(Diagnostic::new(
                "type",
                format!("impl target {:?} must be a struct or enum", target_name),
            )
            .with_span(function.line, function.column));
        }
        let return_ty = lower_type(
            &function.return_ty,
            structs,
            enums,
            aliases,
            function.line,
            function.column,
        )?;
        let mut params = Vec::new();
        if function.receiver.is_some() {
            params.push(target_ty.clone());
        }
        for param in &function.params {
            params.push(lower_type(
                &param.ty,
                structs,
                enums,
                aliases,
                param.line,
                param.column,
            )?);
        }
        let return_ty = if function.is_async {
            Type::Task(Box::new(return_ty))
        } else {
            return_ty
        };
        let method = MethodSig {
            function_name: function_symbol_name(function),
            params,
            return_ty,
            has_self: function.receiver.is_some(),
        };
        let entry = methods.entry(target_name.clone()).or_default();
        if entry.insert(function.source_name.clone(), method).is_some() {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "duplicate impl method {:?} for {:?}",
                    function.name, target_name
                ),
            )
            .with_span(function.line, function.column));
        }
    }
    Ok(methods)
}

fn lower_function(
    function: &syntax::Function,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
    variants: &HashMap<String, Vec<VariantInfo>>,
    functions: &HashMap<String, FunctionSig>,
    methods: &HashMap<String, HashMap<String, MethodSig>>,
    capabilities: &CapabilityConfig,
) -> Result<Function, Diagnostic> {
    let return_ty = lower_type(
        &function.return_ty,
        structs,
        enums,
        aliases,
        function.line,
        function.column,
    )?;
    if function.is_extern {
        if function.is_async {
            return Err(Diagnostic::new(
                "type",
                format!("extern function {:?} cannot be async", function.name),
            )
            .with_span(function.line, function.column));
        }
        if !function.type_params.is_empty() {
            return Err(Diagnostic::new(
                "type",
                format!("extern function {:?} cannot be generic", function.name),
            )
            .with_span(function.line, function.column));
        }
        validate_ffi_signature(function, &return_ty)?;
    }
    let symbol_name = function_symbol_name(function);
    let signature = functions
        .get(&symbol_name)
        .expect("function signatures collected before lowering");
    let mut env: HashMap<String, Binding> = HashMap::new();
    let mut params = Vec::new();
    if let Some(target_name) = &function.impl_target
        && function.receiver.is_some()
    {
        let ty = lower_type(
            &syntax::TypeName::Named(target_name.clone(), Vec::new()),
            structs,
            enums,
            aliases,
            function.line,
            function.column,
        )?;
        env.insert(
            String::from("self"),
            Binding {
                ty: ty.clone(),
                moved: false,
                moved_projections: HashSet::new(),
                borrow_kind: borrow_kind_for_type(&ty, structs, enums),
                borrow_origin: binding_borrow_origin(&ty, Some("self"), structs, enums),
                borrowed_owners: HashSet::new(),
                active_borrow_count: 0,
                active_mut_borrow_count: 0,
            },
        );
        params.push(Param {
            name: String::from("self_"),
            ty,
        });
    }
    for param in &function.params {
        if functions.contains_key(&param.name) {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "binding name {:?} conflicts with a function name",
                    param.name
                ),
            )
            .with_span(param.line, param.column));
        }
        if env.contains_key(&param.name) {
            return Err(
                Diagnostic::new("type", format!("duplicate parameter {:?}", param.name))
                    .with_span(param.line, param.column),
            );
        }
        let ty = lower_type(&param.ty, structs, enums, aliases, param.line, param.column)?;
        env.insert(
            param.name.clone(),
            Binding {
                ty: ty.clone(),
                moved: false,
                moved_projections: HashSet::new(),
                borrow_kind: borrow_kind_for_type(&ty, structs, enums),
                borrow_origin: binding_borrow_origin(&ty, Some(&param.name), structs, enums),
                borrowed_owners: HashSet::new(),
                active_borrow_count: 0,
                active_mut_borrow_count: 0,
            },
        );
        params.push(Param {
            name: param.name.clone(),
            ty,
        });
    }
    let ctx = LowerContext {
        structs,
        enums,
        aliases,
        variants,
        functions,
        methods,
        capabilities,
        current_return: Some(return_ty.clone()),
        current_borrow_return_params: signature
            .borrow_return_params
            .iter()
            .filter_map(|index| params.get(*index).map(|param| param.name.clone()))
            .collect(),
    };
    let (body, _, guaranteed_return) = if function.is_extern {
        (Vec::new(), env.clone(), true)
    } else {
        let (body, diagnostics, guaranteed_return) =
            lower_block_recovering(&function.body, &mut env, &ctx);
        if !diagnostics.is_empty() {
            return Err(primary_diagnostic(diagnostics));
        }
        (body, env.clone(), guaranteed_return)
    };
    if !guaranteed_return {
        return Err(Diagnostic::new(
            "control",
            format!(
                "function {:?} does not return along all paths",
                function.name
            ),
        )
        .with_span(function.line, function.column));
    }
    Ok(Function {
        name: symbol_name,
        source_name: function.source_name.clone(),
        path: function.path.clone(),
        params,
        return_ty: if function.is_async {
            Type::Task(Box::new(return_ty))
        } else {
            return_ty
        },
        body,
        is_async: function.is_async,
        is_extern: function.is_extern,
        extern_abi: function.extern_abi.clone(),
        extern_library: function.extern_library.clone(),
        line: function.line,
        column: function.column,
    })
}

fn lower_block(
    block: &[syntax::Stmt],
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<(Vec<Stmt>, HashMap<String, Binding>, bool), Diagnostic> {
    let scope_names = env.keys().cloned().collect::<HashSet<_>>();
    let mut lowered = Vec::new();
    let mut guaranteed_return = false;
    for stmt in block {
        if guaranteed_return {
            return Err(Diagnostic::new(
                "control",
                "unreachable statements after a terminating control-flow statement are not yet supported in stage1",
            )
            .with_span(stmt.line(), stmt.column()));
        }
        let lowered_stmt = lower_stmt(stmt, env, ctx)?;
        guaranteed_return = lowered_stmt.always_returns();
        lowered.push(lowered_stmt);
    }
    let mut after = env.clone();
    release_scope_borrows(&mut after, &scope_names);
    Ok((lowered, after, guaranteed_return))
}

fn lower_block_recovering(
    block: &[syntax::Stmt],
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> (Vec<Stmt>, Vec<Diagnostic>, bool) {
    let scope_names = env.keys().cloned().collect::<HashSet<_>>();
    let mut lowered = Vec::new();
    let mut diagnostics = Vec::new();
    let mut guaranteed_return = false;
    for stmt in block {
        if guaranteed_return {
            diagnostics.push(
                Diagnostic::new(
                    "control",
                    "unreachable statements after a terminating control-flow statement are not yet supported in stage1",
                )
                .with_span(stmt.line(), stmt.column()),
            );
            continue;
        }
        let mut candidate_env = env.clone();
        match lower_stmt(stmt, &mut candidate_env, ctx) {
            Ok(lowered_stmt) => {
                guaranteed_return = lowered_stmt.always_returns();
                *env = candidate_env;
                lowered.push(lowered_stmt);
            }
            Err(error) => {
                insert_type_error_binding_for_failed_stmt(stmt, env);
                diagnostics.push(error);
            }
        }
    }
    release_scope_borrows(env, &scope_names);
    (lowered, diagnostics, guaranteed_return)
}

fn insert_type_error_binding_for_failed_stmt(
    stmt: &syntax::Stmt,
    env: &mut HashMap<String, Binding>,
) {
    if let syntax::Stmt::Let { name, .. } = stmt {
        env.entry(name.clone()).or_insert_with(|| Binding {
            ty: Type::Error,
            moved: false,
            moved_projections: HashSet::new(),
            borrow_kind: None,
            borrow_origin: None,
            borrowed_owners: HashSet::new(),
            active_borrow_count: 0,
            active_mut_borrow_count: 0,
        });
    }
}

fn lower_stmt(
    stmt: &syntax::Stmt,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Stmt, Diagnostic> {
    match stmt {
        syntax::Stmt::Let {
            name,
            ty,
            expr,
            line,
            column,
        } => {
            if ctx.functions.contains_key(name) {
                return Err(Diagnostic::new(
                    "type",
                    format!("binding name {name:?} conflicts with a function name"),
                )
                .with_span(*line, *column));
            }
            if env.contains_key(name) {
                return Err(Diagnostic::new(
                    "type",
                    format!("rebinding existing name {name:?} is not yet supported in stage1"),
                )
                .with_span(*line, *column));
            }
            let expected = lower_type(ty, ctx.structs, ctx.enums, ctx.aliases, *line, *column)?;
            let lowered_expr = lower_expr_with_expected(expr, Some(&expected), env, ctx)?;
            let actual = lowered_expr.ty().clone();
            if actual != expected && !actual.is_error() && !expected.is_error() {
                return Err(Diagnostic::new(
                    "type",
                    format!("let binding {name:?} expects {expected}, got {actual}"),
                )
                .with_span(*line, *column));
            }
            let borrowed_owners =
                binding_borrowed_owners_from_expr(&expected, &lowered_expr, env, ctx);
            if let Some(borrow_kind) = borrow_kind_for_type(&expected, ctx.structs, ctx.enums) {
                increment_active_borrows(&borrowed_owners, env, borrow_kind, *line, *column)?;
            }
            if !actual.is_copy() {
                move_lowered_value(&lowered_expr, env)?;
            }
            env.insert(
                name.clone(),
                Binding {
                    ty: expected.clone(),
                    moved: false,
                    moved_projections: HashSet::new(),
                    borrow_kind: borrow_kind_for_type(&expected, ctx.structs, ctx.enums),
                    borrow_origin: binding_borrow_origin_from_expr(
                        &expected,
                        &lowered_expr,
                        env,
                        ctx,
                    ),
                    borrowed_owners,
                    active_borrow_count: 0,
                    active_mut_borrow_count: 0,
                },
            );
            Ok(Stmt::Let {
                name: name.clone(),
                ty: expected,
                expr: lowered_expr,
                span: SourceSpan {
                    line: *line,
                    column: *column,
                },
            })
        }
        syntax::Stmt::Print { expr, line, column } => {
            let lowered = lower_expr(expr, env, ctx)?;
            if !matches!(
                lowered.ty(),
                Type::Error | Type::Int | Type::Bool | Type::String
            ) {
                return Err(Diagnostic::new(
                    "type",
                    format!("print expects int, bool, or string, got {}", lowered.ty()),
                )
                .with_span(*line, *column));
            }
            Ok(Stmt::Print {
                expr: lowered,
                span: SourceSpan {
                    line: *line,
                    column: *column,
                },
            })
        }
        syntax::Stmt::Panic { expr, line, column } => {
            let syntax::Expr::Call {
                name,
                type_args,
                args,
                ..
            } = expr
            else {
                return Err(Diagnostic::new(
                    "type",
                    "panic statement expects `panic(\"message\")`",
                )
                .with_span(*line, *column));
            };
            if name != "panic" {
                return Err(Diagnostic::new(
                    "type",
                    "panic statement expects `panic(\"message\")`",
                )
                .with_span(*line, *column));
            }
            if !type_args.is_empty() {
                return Err(
                    Diagnostic::new("type", "panic does not accept type arguments")
                        .with_span(*line, *column),
                );
            }
            if args.len() != 1 {
                return Err(Diagnostic::new(
                    "type",
                    format!("panic expects 1 argument, got {}", args.len()),
                )
                .with_span(*line, *column));
            }
            let message = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
            if message.ty() != &Type::String {
                return Err(Diagnostic::new(
                    "type",
                    format!("panic expects a string argument, got {}", message.ty()),
                )
                .with_span(args[0].line(), args[0].column()));
            }
            move_lowered_value(&message, env)?;
            Ok(Stmt::Panic {
                message,
                span: SourceSpan {
                    line: *line,
                    column: *column,
                },
            })
        }
        syntax::Stmt::If {
            cond,
            then_block,
            else_block,
            line,
            column,
        } => {
            let lowered_cond = lower_expr(cond, env, ctx)?;
            if lowered_cond.ty() != &Type::Bool {
                return Err(Diagnostic::new(
                    "type",
                    format!("if condition expects bool, got {}", lowered_cond.ty()),
                )
                .with_span(*line, *column));
            }
            if let Some(known_cond) = static_bool_value(&lowered_cond) {
                if known_cond {
                    let mut then_env = env.clone();
                    let (then_block, then_after, _) = lower_block(then_block, &mut then_env, ctx)?;
                    *env = then_after;
                    return Ok(Stmt::If {
                        cond: lowered_cond,
                        then_block,
                        else_block: else_block.as_ref().map(|_| Vec::new()),
                        span: SourceSpan {
                            line: *line,
                            column: *column,
                        },
                    });
                }
                if let Some(else_block) = else_block {
                    let mut else_env = env.clone();
                    let (block, after, _) = lower_block(else_block, &mut else_env, ctx)?;
                    *env = after;
                    return Ok(Stmt::If {
                        cond: lowered_cond,
                        then_block: Vec::new(),
                        else_block: Some(block),
                        span: SourceSpan {
                            line: *line,
                            column: *column,
                        },
                    });
                }
                return Ok(Stmt::If {
                    cond: lowered_cond,
                    then_block: Vec::new(),
                    else_block: None,
                    span: SourceSpan {
                        line: *line,
                        column: *column,
                    },
                });
            }
            let before = env.clone();
            let mut then_env = before.clone();
            let (then_block, then_after, then_returns) =
                lower_block(then_block, &mut then_env, ctx)?;
            let (else_block, else_after, else_returns) = if let Some(else_block) = else_block {
                let mut else_env = before.clone();
                let (block, after, returns) = lower_block(else_block, &mut else_env, ctx)?;
                (Some(block), Some(after), returns)
            } else {
                (None, None, false)
            };
            merge_branch_state(
                env,
                &before,
                &then_after,
                then_returns,
                else_after.as_ref(),
                else_returns,
            );
            Ok(Stmt::If {
                cond: lowered_cond,
                then_block,
                else_block,
                span: SourceSpan {
                    line: *line,
                    column: *column,
                },
            })
        }
        syntax::Stmt::While {
            cond,
            body,
            line,
            column,
        } => {
            let lowered_cond = lower_expr(cond, env, ctx)?;
            if lowered_cond.ty() != &Type::Bool {
                return Err(Diagnostic::new(
                    "type",
                    format!("while condition expects bool, got {}", lowered_cond.ty()),
                )
                .with_span(*line, *column));
            }
            if static_bool_value(&lowered_cond) == Some(false) {
                return Ok(Stmt::While {
                    cond: lowered_cond,
                    body: Vec::new(),
                    span: SourceSpan {
                        line: *line,
                        column: *column,
                    },
                });
            }
            let before = env.clone();
            let mut body_env = before.clone();
            let (body, body_after, body_returns) = lower_block(body, &mut body_env, ctx)?;
            // AG1.1: reject moves of outer non-Copy variables inside the loop
            // body — on subsequent iterations the value would not be available.
            if !body_returns {
                for (name, pre_binding) in &before {
                    if pre_binding.moved || pre_binding.ty.is_copy() {
                        continue;
                    }
                    if let Some(post_binding) = body_after.get(name) {
                        let moved_projection_in_body = post_binding
                            .moved_projections
                            .iter()
                            .any(|projection| !pre_binding.moved_projections.contains(projection));
                        if post_binding.moved || moved_projection_in_body {
                            return Err(ownership_error(
                                OWNERSHIP_LOOP_MOVE_OUTER_NON_COPY,
                                format!(
                                    "cannot move non-copy value `{}` inside loop body — \
                                     value would not be available on subsequent iterations",
                                    name
                                ),
                            )
                            .with_span(*line, *column));
                        }
                    }
                }
            }
            merge_loop_state(env, &before, &body_after, body_returns);
            Ok(Stmt::While {
                cond: lowered_cond,
                body,
                span: SourceSpan {
                    line: *line,
                    column: *column,
                },
            })
        }
        syntax::Stmt::Match {
            expr,
            arms,
            line,
            column,
        } => {
            let lowered_expr = lower_expr(expr, env, ctx)?;
            let match_borrowed_owners = expr_borrowed_owners(&lowered_expr, env, ctx);
            let match_borrow_kind = borrow_kind_for_type(lowered_expr.ty(), ctx.structs, ctx.enums);
            let reuse_existing_match_binding =
                matches!(lowered_expr, Expr::VarRef { .. }) && !match_borrowed_owners.is_empty();
            if let Some(borrow_kind) = match_borrow_kind
                && !reuse_existing_match_binding
            {
                increment_active_borrows(&match_borrowed_owners, env, borrow_kind, *line, *column)?;
            }
            if matches!(lowered_expr, Expr::VarRef { .. }) && !lowered_expr.ty().is_copy() {
                move_lowered_owner_value(&lowered_expr, env)?;
            }
            let (enum_name, variant_defs) =
                match_variants(lowered_expr.ty(), ctx).ok_or_else(|| {
                    Diagnostic::new(
                        "type",
                        format!(
                            "match expects an enum-like value, got {}",
                            lowered_expr.ty()
                        ),
                    )
                    .with_span(*line, *column)
                })?;
            let before = env.clone();
            let mut seen = HashMap::new();
            let mut lowered_arms = Vec::new();
            let mut arm_states = Vec::new();
            for arm in arms {
                let variant_def = variant_defs
                    .iter()
                    .find(|variant| variant.name == arm.variant)
                    .ok_or_else(|| {
                        Diagnostic::new(
                            "type",
                            message_with_suggestion(
                                format!("enum {enum_name:?} has no variant {:?}", arm.variant),
                                &arm.variant,
                                variant_defs.iter().map(|variant| variant.name.as_str()),
                            ),
                        )
                        .with_span(arm.line, arm.column)
                    })?;
                if seen.insert(arm.variant.clone(), ()).is_some() {
                    return Err(Diagnostic::new(
                        "type",
                        format!("duplicate match arm {:?}", arm.variant),
                    )
                    .with_span(arm.line, arm.column));
                }
                let mut arm_env = before.clone();
                let binding_tys = if arm.is_named {
                    if variant_def.payload_names.is_empty() {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "match arm {:?} uses named bindings, but variant {:?} is positional",
                                arm.variant, arm.variant
                            ),
                        )
                        .with_span(arm.line, arm.column));
                    }
                    if arm.bindings.len() != variant_def.payload_names.len() {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "match arm {:?} expects {} named bindings, got {}",
                                arm.variant,
                                variant_def.payload_names.len(),
                                arm.bindings.len()
                            ),
                        )
                        .with_span(arm.line, arm.column));
                    }
                    let mut seen_named = HashMap::new();
                    let mut payload_tys = Vec::new();
                    for binding in &arm.bindings {
                        let Some(position) = variant_def
                            .payload_names
                            .iter()
                            .position(|name| name == binding)
                        else {
                            return Err(Diagnostic::new(
                                "type",
                                format!(
                                    "match arm {:?} has no named payload {:?}",
                                    arm.variant, binding
                                ),
                            )
                            .with_span(arm.line, arm.column));
                        };
                        if seen_named.insert(binding.clone(), ()).is_some() {
                            return Err(Diagnostic::new(
                                "type",
                                format!(
                                    "match arm {:?} repeats named payload {:?}",
                                    arm.variant, binding
                                ),
                            )
                            .with_span(arm.line, arm.column));
                        }
                        payload_tys.push(variant_def.payload_tys[position].clone());
                    }
                    payload_tys
                } else {
                    if !variant_def.payload_names.is_empty() {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "match arm {:?} must use named bindings for variant {:?}",
                                arm.variant, arm.variant
                            ),
                        )
                        .with_span(arm.line, arm.column));
                    }
                    if arm.bindings.len() != variant_def.payload_tys.len() {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "match arm {:?} expects {} bindings, got {}",
                                arm.variant,
                                variant_def.payload_tys.len(),
                                arm.bindings.len()
                            ),
                        )
                        .with_span(arm.line, arm.column));
                    }
                    variant_def.payload_tys.clone()
                };
                for (binding_index, (binding, payload_ty)) in
                    arm.bindings.iter().zip(binding_tys.iter()).enumerate()
                {
                    if ctx.functions.contains_key(binding) {
                        return Err(Diagnostic::new(
                            "type",
                            format!("match binding {binding:?} conflicts with a function name"),
                        )
                        .with_span(arm.line, arm.column));
                    }
                    if arm_env.contains_key(binding) {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "match binding {binding:?} reuses an existing name in the current scope"
                            ),
                        )
                        .with_span(arm.line, arm.column));
                    }
                    arm_env.insert(
                        binding.clone(),
                        Binding {
                            ty: payload_ty.clone(),
                            moved: false,
                            moved_projections: HashSet::new(),
                            borrow_kind: borrow_kind_for_type(payload_ty, ctx.structs, ctx.enums),
                            borrow_origin: match_binding_borrow_origin(
                                &lowered_expr,
                                &arm.variant,
                                binding,
                                binding_index,
                                payload_ty,
                                &before,
                                ctx,
                            ),
                            borrowed_owners: match_binding_borrowed_owners(
                                &lowered_expr,
                                &arm.variant,
                                binding,
                                binding_index,
                                payload_ty,
                                &before,
                                ctx,
                            ),
                            active_borrow_count: 0,
                            active_mut_borrow_count: 0,
                        },
                    );
                }
                let (body, after, returns) = lower_block(&arm.body, &mut arm_env, ctx)?;
                lowered_arms.push(MatchArm {
                    enum_name: enum_name.clone(),
                    variant: arm.variant.clone(),
                    bindings: arm.bindings.clone(),
                    is_named: arm.is_named,
                    body,
                });
                arm_states.push((after, returns));
            }
            let missing = variant_defs
                .iter()
                .filter(|variant| !seen.contains_key(&variant.name))
                .map(|variant| variant.name.clone())
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "match on {:?} is not exhaustive; missing {}",
                        enum_name,
                        missing.join(", ")
                    ),
                )
                .with_span(*line, *column));
            }
            merge_match_state(env, &before, &arm_states);
            if let Some(borrow_kind) = match_borrow_kind
                && !reuse_existing_match_binding
            {
                release_active_borrow_owners(&match_borrowed_owners, env, borrow_kind);
            }
            Ok(Stmt::Match {
                expr: lowered_expr,
                arms: lowered_arms,
                span: SourceSpan {
                    line: *line,
                    column: *column,
                },
            })
        }
        syntax::Stmt::Defer { expr, line, column } => {
            let lowered_expr = lower_expr(expr, env, ctx)?;
            if !lowered_expr.ty().is_copy() {
                move_lowered_value(&lowered_expr, env)?;
            }
            Ok(Stmt::Defer {
                expr: lowered_expr,
                span: SourceSpan {
                    line: *line,
                    column: *column,
                },
            })
        }
        syntax::Stmt::Return { expr, line, column } => {
            let Some(expected) = ctx.current_return.as_ref() else {
                return Err(
                    Diagnostic::new("control", "return is only valid inside a function")
                        .with_span(*line, *column),
                );
            };
            let lowered_expr = lower_expr_with_expected(expr, Some(expected), env, ctx)?;
            if lowered_expr.ty() != expected {
                return Err(Diagnostic::new(
                    "type",
                    format!("return expects {expected}, got {}", lowered_expr.ty()),
                )
                .with_span(*line, *column));
            }
            if contains_borrowed_slice_type(expected, ctx.structs, ctx.enums)
                && !ctx.current_borrow_return_params.is_empty()
            {
                match expr_borrow_origin(&lowered_expr, env, ctx) {
                    None => {}
                    Some(BorrowOrigin::Param(origin))
                        if ctx.current_borrow_return_params.contains(&origin) => {}
                    _ => {
                        return Err(ownership_error(
                            OWNERSHIP_BORROW_RETURN_REQUIRES_PARAM_ORIGIN,
                            format!(
                                "returning borrowed values requires data derived from one of the borrowed parameters in stage1"
                            ),
                        )
                        .with_span(*line, *column));
                    }
                }
            }
            Ok(Stmt::Return {
                expr: lowered_expr,
                span: SourceSpan {
                    line: *line,
                    column: *column,
                },
            })
        }
    }
}

fn merge_branch_state(
    env: &mut HashMap<String, Binding>,
    before: &HashMap<String, Binding>,
    then_after: &HashMap<String, Binding>,
    then_returns: bool,
    else_after: Option<&HashMap<String, Binding>>,
    else_returns: bool,
) {
    env.clear();
    for (name, binding) in before {
        let then_moved = if then_returns {
            binding.moved
        } else {
            then_after
                .get(name)
                .map(|entry| entry.moved)
                .unwrap_or(binding.moved)
        };
        let else_moved = if else_returns {
            binding.moved
        } else {
            else_after
                .and_then(|branch| branch.get(name).map(|entry| entry.moved))
                .unwrap_or(binding.moved)
        };
        env.insert(
            name.clone(),
            Binding {
                ty: binding.ty.clone(),
                moved: then_moved || else_moved,
                moved_projections: merge_projection_sets(
                    binding,
                    then_after.get(name),
                    then_returns,
                    else_after.and_then(|branch| branch.get(name)),
                    else_returns,
                ),
                borrow_kind: binding.borrow_kind,
                borrow_origin: binding.borrow_origin.clone(),
                borrowed_owners: binding.borrowed_owners.clone(),
                active_borrow_count: merge_borrow_count(
                    binding.active_borrow_count,
                    then_returns,
                    then_after.get(name).map(|entry| entry.active_borrow_count),
                    else_returns,
                    else_after
                        .and_then(|branch| branch.get(name).map(|entry| entry.active_borrow_count)),
                ),
                active_mut_borrow_count: merge_borrow_count(
                    binding.active_mut_borrow_count,
                    then_returns,
                    then_after
                        .get(name)
                        .map(|entry| entry.active_mut_borrow_count),
                    else_returns,
                    else_after.and_then(|branch| {
                        branch.get(name).map(|entry| entry.active_mut_borrow_count)
                    }),
                ),
            },
        );
    }
}

fn merge_projection_sets(
    before: &Binding,
    then_after: Option<&Binding>,
    then_returns: bool,
    else_after: Option<&Binding>,
    else_returns: bool,
) -> HashSet<ProjectionPath> {
    let mut moved = before.moved_projections.clone();
    if !then_returns {
        if let Some(binding) = then_after {
            moved.extend(binding.moved_projections.iter().cloned());
        }
    }
    if !else_returns {
        if let Some(binding) = else_after {
            moved.extend(binding.moved_projections.iter().cloned());
        }
    }
    moved
}

fn merge_loop_state(
    env: &mut HashMap<String, Binding>,
    before: &HashMap<String, Binding>,
    body_after: &HashMap<String, Binding>,
    body_returns: bool,
) {
    // AG1.1: the loop body may execute zero times, so post-loop ownership
    // state preserves the pre-loop moved flags.  Moves of outer non-Copy
    // values inside the body are rejected earlier (before this function is
    // called), so the only moved-state change that can reach here is for
    // values that were already moved before the loop.  Borrow counts still
    // take the max of pre-loop and body-after to stay conservative.
    env.clear();
    for (name, binding) in before {
        env.insert(
            name.clone(),
            Binding {
                ty: binding.ty.clone(),
                moved: binding.moved,
                moved_projections: binding.moved_projections.clone(),
                borrow_kind: binding.borrow_kind,
                borrow_origin: binding.borrow_origin.clone(),
                borrowed_owners: binding.borrowed_owners.clone(),
                active_borrow_count: if body_returns {
                    binding.active_borrow_count
                } else {
                    let body_count = body_after
                        .get(name)
                        .map(|entry| entry.active_borrow_count)
                        .unwrap_or(binding.active_borrow_count);
                    binding.active_borrow_count.max(body_count)
                },
                active_mut_borrow_count: if body_returns {
                    binding.active_mut_borrow_count
                } else {
                    let body_count = body_after
                        .get(name)
                        .map(|entry| entry.active_mut_borrow_count)
                        .unwrap_or(binding.active_mut_borrow_count);
                    binding.active_mut_borrow_count.max(body_count)
                },
            },
        );
    }
}

fn merge_match_state(
    env: &mut HashMap<String, Binding>,
    before: &HashMap<String, Binding>,
    arm_states: &[(HashMap<String, Binding>, bool)],
) {
    env.clear();
    for (name, binding) in before {
        let moved = arm_states.iter().any(|(after, returns)| {
            if *returns {
                binding.moved
            } else {
                after
                    .get(name)
                    .map(|entry| entry.moved)
                    .unwrap_or(binding.moved)
            }
        });
        env.insert(
            name.clone(),
            Binding {
                ty: binding.ty.clone(),
                moved,
                moved_projections: merge_match_projection_sets(binding, name, arm_states),
                borrow_kind: binding.borrow_kind,
                borrow_origin: binding.borrow_origin.clone(),
                borrowed_owners: binding.borrowed_owners.clone(),
                active_borrow_count: arm_states
                    .iter()
                    .filter_map(|(after, returns)| {
                        if *returns {
                            Some(binding.active_borrow_count)
                        } else {
                            after.get(name).map(|entry| entry.active_borrow_count)
                        }
                    })
                    .max()
                    .unwrap_or(binding.active_borrow_count),
                active_mut_borrow_count: arm_states
                    .iter()
                    .filter_map(|(after, returns)| {
                        if *returns {
                            Some(binding.active_mut_borrow_count)
                        } else {
                            after.get(name).map(|entry| entry.active_mut_borrow_count)
                        }
                    })
                    .max()
                    .unwrap_or(binding.active_mut_borrow_count),
            },
        );
    }
}

fn merge_match_projection_sets(
    before: &Binding,
    name: &str,
    arm_states: &[(HashMap<String, Binding>, bool)],
) -> HashSet<ProjectionPath> {
    let mut moved = before.moved_projections.clone();
    for (after, returns) in arm_states {
        if *returns {
            continue;
        }
        if let Some(binding) = after.get(name) {
            moved.extend(binding.moved_projections.iter().cloned());
        }
    }
    moved
}

fn match_variants(ty: &Type, ctx: &LowerContext<'_>) -> Option<(String, Vec<EnumVariantDef>)> {
    match ty {
        Type::Enum(enum_name) => ctx
            .enums
            .get(enum_name)
            .map(|enum_def| (enum_name.clone(), enum_def.variants.clone())),
        Type::Option(inner) => Some((
            String::from("Option"),
            vec![
                EnumVariantDef {
                    name: String::from("Some"),
                    payload_tys: vec![(*inner.clone()).clone()],
                    payload_names: Vec::new(),
                },
                EnumVariantDef {
                    name: String::from("None"),
                    payload_tys: Vec::new(),
                    payload_names: Vec::new(),
                },
            ],
        )),
        Type::Result(ok, err) => Some((
            String::from("Result"),
            vec![
                EnumVariantDef {
                    name: String::from("Ok"),
                    payload_tys: vec![(*ok.clone()).clone()],
                    payload_names: Vec::new(),
                },
                EnumVariantDef {
                    name: String::from("Err"),
                    payload_tys: vec![(*err.clone()).clone()],
                    payload_names: Vec::new(),
                },
            ],
        )),
        _ => None,
    }
}

fn lower_expr(
    expr: &syntax::Expr,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Expr, Diagnostic> {
    lower_expr_with_expected(expr, None, env, ctx)
}

fn lower_expr_with_expected(
    expr: &syntax::Expr,
    expected: Option<&Type>,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Expr, Diagnostic> {
    match expr {
        syntax::Expr::Literal(literal) => Ok(lower_literal(literal)),
        syntax::Expr::VarRef { name, line, column } => {
            if let Some(binding) = env.get(name) {
                if binding.moved {
                    return Err(ownership_error(
                        OWNERSHIP_USE_AFTER_MOVE,
                        format!("use of moved value {name:?}"),
                    )
                    .with_span(*line, *column));
                }
                if !binding.moved_projections.is_empty() {
                    return Err(ownership_error(
                        OWNERSHIP_USE_AFTER_MOVE,
                        format!("use of partially moved value {name:?}"),
                    )
                    .with_span(*line, *column));
                }
                return Ok(Expr::VarRef {
                    name: name.clone(),
                    ty: binding.ty.clone(),
                });
            }
            if name == "None" {
                if let Some(Type::Option(inner)) = expected {
                    return Ok(Expr::EnumVariant {
                        enum_name: String::from("Option"),
                        variant: String::from("None"),
                        field_names: Vec::new(),
                        payloads: Vec::new(),
                        ty: Type::Option(inner.clone()),
                    });
                }
                return Err(
                    Diagnostic::new("type", "None requires an expected Option<T> context")
                        .with_span(*line, *column),
                );
            }
            if let Some(variant) = resolve_variant(name, expected, ctx, *line, *column)? {
                if !variant.payload_tys.is_empty() {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "enum variant {name:?} requires {} arguments",
                            variant.payload_tys.len()
                        ),
                    )
                    .with_span(*line, *column));
                }
                return Ok(Expr::EnumVariant {
                    enum_name: variant.enum_name.clone(),
                    variant: name.clone(),
                    field_names: Vec::new(),
                    payloads: Vec::new(),
                    ty: Type::Enum(variant.enum_name.clone()),
                });
            }
            Err(Diagnostic::new(
                "type",
                message_with_suggestion(format!("undefined variable {name:?}"), name, env.keys()),
            )
            .with_span(*line, *column))
        }
        syntax::Expr::Call {
            name,
            type_args,
            args,
            line,
            column,
        } => {
            if is_async_runtime_intrinsic(name) {
                return lower_async_runtime_intrinsic(
                    name, type_args, args, *line, *column, env, ctx,
                );
            }
            if !type_args.is_empty() {
                return Err(
                    Diagnostic::new("type", format!("function {name:?} is not generic"))
                        .with_span(*line, *column),
                );
            }
            if name == "assert_true" {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("assert_true expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::Bool), env, ctx)?;
                if lowered.ty() != &Type::Bool {
                    return Err(Diagnostic::new(
                        "type",
                        format!("assert_true expects a bool argument, got {}", lowered.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: with_assert_location(vec![lowered], *line, *column),
                    ty: Type::Int,
                });
            }
            if name == "assert_property" {
                if args.len() != 2 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("assert_property expects 2 arguments, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let label = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if label.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!("assert_property expects a string name, got {}", label.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let holds = lower_expr_with_expected(&args[1], Some(&Type::Bool), env, ctx)?;
                if holds.ty() != &Type::Bool {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "assert_property expects a bool condition, got {}",
                            holds.ty()
                        ),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                move_lowered_value(&label, env)?;
                move_lowered_value(&holds, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: with_assert_location(vec![label, holds], *line, *column),
                    ty: Type::Int,
                });
            }
            if name == "assert_snapshot" {
                if args.len() != 3 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("assert_snapshot expects 3 arguments, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let label = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                let actual = lower_expr_with_expected(&args[1], Some(&Type::String), env, ctx)?;
                let expected = lower_expr_with_expected(&args[2], Some(&Type::String), env, ctx)?;
                for (expr, role) in [
                    (&label, "name"),
                    (&actual, "actual"),
                    (&expected, "expected"),
                ] {
                    if expr.ty() != &Type::String {
                        return Err(Diagnostic::new(
                            "type",
                            format!("assert_snapshot expects string {role}, got {}", expr.ty()),
                        )
                        .with_span(*line, *column));
                    }
                }
                move_lowered_value(&label, env)?;
                move_lowered_value(&actual, env)?;
                move_lowered_value(&expected, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: with_assert_location(vec![label, actual, expected], *line, *column),
                    ty: Type::Int,
                });
            }
            if name == "assert_contains" {
                if args.len() != 2 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("assert_contains expects 2 arguments, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let haystack = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if haystack.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "assert_contains expects a string haystack, got {}",
                            haystack.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let needle = lower_expr_with_expected(&args[1], Some(&Type::String), env, ctx)?;
                if needle.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "assert_contains expects a string needle, got {}",
                            needle.ty()
                        ),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                move_lowered_value(&haystack, env)?;
                move_lowered_value(&needle, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: with_assert_location(vec![haystack, needle], *line, *column),
                    ty: Type::Int,
                });
            }
            if name == "assert_eq" || name == "assert_ne" || name == "assert_case_eq" {
                let has_label = name == "assert_case_eq";
                let expected_args = if has_label { 3 } else { 2 };
                if args.len() != expected_args {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "{name} expects {expected_args} arguments, got {}",
                            args.len()
                        ),
                    )
                    .with_span(*line, *column));
                }
                let mut lowered_args = Vec::new();
                let value_start = if has_label {
                    let label = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                    if label.ty() != &Type::String {
                        return Err(Diagnostic::new(
                            "type",
                            format!("{name} expects a string case name, got {}", label.ty()),
                        )
                        .with_span(args[0].line(), args[0].column()));
                    }
                    move_lowered_value(&label, env)?;
                    lowered_args.push(label);
                    1
                } else {
                    0
                };
                let lhs = lower_expr(&args[value_start], env, ctx)?;
                if !matches!(lhs.ty(), Type::Int | Type::Bool | Type::String) {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "{name} expects int, bool, or string arguments, got {}",
                            lhs.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let rhs =
                    lower_expr_with_expected(&args[value_start + 1], Some(lhs.ty()), env, ctx)?;
                if rhs.ty() != lhs.ty() {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} requires both arguments to share one type"),
                    )
                    .with_span(args[value_start + 1].line(), args[value_start + 1].column()));
                }
                move_lowered_value(&lhs, env)?;
                move_lowered_value(&rhs, env)?;
                lowered_args.push(lhs);
                lowered_args.push(rhs);
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: with_assert_location(lowered_args, *line, *column),
                    ty: Type::Int,
                });
            }
            if name == "len" {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("len expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr(&args[0], env, ctx)?;
                if !matches!(
                    lowered.ty(),
                    Type::Array(_) | Type::Slice(_) | Type::MutSlice(_)
                ) {
                    return Err(Diagnostic::new(
                        "type",
                        format!("len expects an array or slice value, got {}", lowered.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::Int,
                });
            }
            if name == "io_eprintln" {
                // Ungated: stderr output is ambient, matching `print`'s
                // ungated statement form. No capability check.
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("io_eprintln expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if lowered.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "io_eprintln expects a string argument, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::Int,
                });
            }
            if name == "json_parse_int" {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("json_parse_int expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if lowered.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "json_parse_int expects a string argument, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::Option(Box::new(Type::Int)),
                });
            }
            if name == "json_parse_bool" {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("json_parse_bool expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if lowered.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "json_parse_bool expects a string argument, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::Option(Box::new(Type::Bool)),
                });
            }
            if name == "json_parse_string" {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("json_parse_string expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if lowered.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "json_parse_string expects a string argument, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::Option(Box::new(Type::String)),
                });
            }
            if matches!(
                name.as_str(),
                "json_parse_field_int" | "json_parse_field_bool" | "json_parse_field_string"
            ) {
                if args.len() != 2 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects 2 arguments, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let text = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if text.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects a string JSON argument, got {}", text.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let key = lower_expr_with_expected(&args[1], Some(&Type::String), env, ctx)?;
                if key.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects a string key argument, got {}", key.ty()),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                move_lowered_value(&text, env)?;
                move_lowered_value(&key, env)?;
                let ty = match name.as_str() {
                    "json_parse_field_int" => Type::Option(Box::new(Type::Int)),
                    "json_parse_field_bool" => Type::Option(Box::new(Type::Bool)),
                    "json_parse_field_string" => Type::Option(Box::new(Type::String)),
                    _ => unreachable!(),
                };
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![text, key],
                    ty,
                });
            }
            if name == "json_stringify_int" {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("json_stringify_int expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::Int), env, ctx)?;
                if lowered.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "json_stringify_int expects an int argument, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::String,
                });
            }
            if name == "json_stringify_bool" {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("json_stringify_bool expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::Bool), env, ctx)?;
                if lowered.ty() != &Type::Bool {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "json_stringify_bool expects a bool argument, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::String,
                });
            }
            if name == "json_stringify_string" {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "json_stringify_string expects 1 argument, got {}",
                            args.len()
                        ),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if lowered.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "json_stringify_string expects a string argument, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::String,
                });
            }
            if name == "regex_is_match" || name == "regex_find" || name == "regex_replace_all" {
                let expected_len = if name == "regex_replace_all" { 3 } else { 2 };
                if args.len() != expected_len {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "{name} expects {expected_len} arguments, got {}",
                            args.len()
                        ),
                    )
                    .with_span(*line, *column));
                }
                let mut lowered_args = Vec::new();
                for (idx, arg) in args.iter().enumerate() {
                    let lowered = lower_expr_with_expected(arg, Some(&Type::String), env, ctx)?;
                    if lowered.ty() != &Type::String {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "{name} expects argument {} type string, got {}",
                                idx + 1,
                                lowered.ty()
                            ),
                        )
                        .with_span(arg.line(), arg.column()));
                    }
                    move_lowered_value(&lowered, env)?;
                    lowered_args.push(lowered);
                }
                let ty = if name == "regex_is_match" {
                    Type::Bool
                } else if name == "regex_find" {
                    Type::Option(Box::new(Type::String))
                } else {
                    Type::String
                };
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: lowered_args,
                    ty,
                });
            }
            if name == "fs_read" {
                require_capability(ctx.capabilities, CapabilityKind::Fs, name, *line, *column)?;
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("fs_read expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if lowered.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!("fs_read expects a string argument, got {}", lowered.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::Option(Box::new(Type::String)),
                });
            }
            if matches!(name.as_str(), "fs_write" | "fs_append" | "fs_replace") {
                require_capability(
                    ctx.capabilities,
                    CapabilityKind::FsWrite,
                    name,
                    *line,
                    *column,
                )?;
                if args.len() != 2 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects 2 arguments, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let path = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if path.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects argument 1 type string, got {}", path.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let content = lower_expr_with_expected(&args[1], Some(&Type::String), env, ctx)?;
                if content.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "{name} expects argument 2 type string, got {}",
                            content.ty()
                        ),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                move_lowered_value(&path, env)?;
                move_lowered_value(&content, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![path, content],
                    ty: Type::Int,
                });
            }
            if matches!(
                name.as_str(),
                "fs_create" | "fs_mkdir" | "fs_mkdir_all" | "fs_remove_file" | "fs_remove_dir"
            ) {
                require_capability(
                    ctx.capabilities,
                    CapabilityKind::FsWrite,
                    name,
                    *line,
                    *column,
                )?;
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let path = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if path.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects argument 1 type string, got {}", path.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&path, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![path],
                    ty: Type::Int,
                });
            }
            if name == "net_resolve" {
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("net_resolve expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if lowered.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "net_resolve expects a string argument, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::Option(Box::new(Type::String)),
                });
            }
            if name == "net_tcp_listen_loopback_once" {
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                if args.len() != 2 {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "net_tcp_listen_loopback_once expects 2 arguments, got {}",
                            args.len()
                        ),
                    )
                    .with_span(*line, *column));
                }
                let response = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if response.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "net_tcp_listen_loopback_once expects argument 1 type string, got {}",
                            response.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let timeout = lower_expr_with_expected(&args[1], Some(&Type::Int), env, ctx)?;
                if timeout.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "net_tcp_listen_loopback_once expects argument 2 type int, got {}",
                            timeout.ty()
                        ),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                move_lowered_value(&response, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![response, timeout],
                    ty: Type::Option(Box::new(Type::Int)),
                });
            }
            if name == "net_tcp_dial" {
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                if args.len() != 4 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("net_tcp_dial expects 4 arguments, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let host = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if host.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "net_tcp_dial expects argument 1 type string, got {}",
                            host.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let port = lower_expr_with_expected(&args[1], Some(&Type::Int), env, ctx)?;
                if port.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "net_tcp_dial expects argument 2 type int, got {}",
                            port.ty()
                        ),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                let message = lower_expr_with_expected(&args[2], Some(&Type::String), env, ctx)?;
                if message.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "net_tcp_dial expects argument 3 type string, got {}",
                            message.ty()
                        ),
                    )
                    .with_span(args[2].line(), args[2].column()));
                }
                let timeout = lower_expr_with_expected(&args[3], Some(&Type::Int), env, ctx)?;
                if timeout.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "net_tcp_dial expects argument 4 type int, got {}",
                            timeout.ty()
                        ),
                    )
                    .with_span(args[3].line(), args[3].column()));
                }
                move_lowered_value(&host, env)?;
                move_lowered_value(&message, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![host, port, message, timeout],
                    ty: Type::Option(Box::new(Type::String)),
                });
            }
            if name == "net_udp_bind_loopback_once" {
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                if args.len() != 2 {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "net_udp_bind_loopback_once expects 2 arguments, got {}",
                            args.len()
                        ),
                    )
                    .with_span(*line, *column));
                }
                let response = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if response.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "net_udp_bind_loopback_once expects argument 1 type string, got {}",
                            response.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let timeout = lower_expr_with_expected(&args[1], Some(&Type::Int), env, ctx)?;
                if timeout.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "net_udp_bind_loopback_once expects argument 2 type int, got {}",
                            timeout.ty()
                        ),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                move_lowered_value(&response, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![response, timeout],
                    ty: Type::Option(Box::new(Type::Int)),
                });
            }
            if name == "net_udp_send_recv" {
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                if args.len() != 4 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("net_udp_send_recv expects 4 arguments, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let host = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if host.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "net_udp_send_recv expects argument 1 type string, got {}",
                            host.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let port = lower_expr_with_expected(&args[1], Some(&Type::Int), env, ctx)?;
                if port.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "net_udp_send_recv expects argument 2 type int, got {}",
                            port.ty()
                        ),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                let message = lower_expr_with_expected(&args[2], Some(&Type::String), env, ctx)?;
                if message.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "net_udp_send_recv expects argument 3 type string, got {}",
                            message.ty()
                        ),
                    )
                    .with_span(args[2].line(), args[2].column()));
                }
                let timeout = lower_expr_with_expected(&args[3], Some(&Type::Int), env, ctx)?;
                if timeout.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "net_udp_send_recv expects argument 4 type int, got {}",
                            timeout.ty()
                        ),
                    )
                    .with_span(args[3].line(), args[3].column()));
                }
                move_lowered_value(&host, env)?;
                move_lowered_value(&message, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![host, port, message, timeout],
                    ty: Type::Option(Box::new(Type::String)),
                });
            }
            if name == "http_get" {
                // HTTP GET shares the `net` capability surface: any code that
                // can open a raw TCP socket could implement HTTP itself, so a
                // separate `http` manifest flag would not add meaningful
                // isolation in stage1.
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("http_get expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if lowered.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!("http_get expects a string argument, got {}", lowered.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::Option(Box::new(Type::String)),
                });
            }
            if name == "http_serve_once" {
                // HTTP server support shares the existing `net` capability: the
                // same manifest approval that allows socket clients also gates
                // loop-local service binds in stage1.
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                if args.len() != 2 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("http_serve_once expects 2 arguments, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let bind = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if bind.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "http_serve_once expects a string bind argument, got {}",
                            bind.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let body = lower_expr_with_expected(&args[1], Some(&Type::String), env, ctx)?;
                if body.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "http_serve_once expects a string body argument, got {}",
                            body.ty()
                        ),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                move_lowered_value(&bind, env)?;
                move_lowered_value(&body, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![bind, body],
                    ty: Type::Bool,
                });
            }
            if name == "http_serve_route" {
                // Route-based HTTP service support shares the existing `net`
                // capability with the lower-level socket and HTTP helpers.
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                if args.len() != 4 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("http_serve_route expects 4 arguments, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let bind = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if bind.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "http_serve_route expects a string bind argument, got {}",
                            bind.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let route_path = lower_expr_with_expected(&args[1], Some(&Type::String), env, ctx)?;
                if route_path.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "http_serve_route expects a string route path argument, got {}",
                            route_path.ty()
                        ),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                let body = lower_expr_with_expected(&args[2], Some(&Type::String), env, ctx)?;
                if body.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "http_serve_route expects a string body argument, got {}",
                            body.ty()
                        ),
                    )
                    .with_span(args[2].line(), args[2].column()));
                }
                let max_requests = lower_expr_with_expected(&args[3], Some(&Type::Int), env, ctx)?;
                if max_requests.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "http_serve_route expects an int max_requests argument, got {}",
                            max_requests.ty()
                        ),
                    )
                    .with_span(args[3].line(), args[3].column()));
                }
                move_lowered_value(&bind, env)?;
                move_lowered_value(&route_path, env)?;
                move_lowered_value(&body, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![bind, route_path, body, max_requests],
                    ty: Type::Bool,
                });
            }
            if name == "process_status" {
                require_capability(
                    ctx.capabilities,
                    CapabilityKind::Process,
                    name,
                    *line,
                    *column,
                )?;
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("process_status expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if lowered.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "process_status expects a string argument, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::Int,
                });
            }
            if name == "clock_now_ms" {
                require_capability(
                    ctx.capabilities,
                    CapabilityKind::Clock,
                    name,
                    *line,
                    *column,
                )?;
                if !args.is_empty() {
                    return Err(Diagnostic::new(
                        "type",
                        format!("clock_now_ms expects 0 arguments, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: Vec::new(),
                    ty: Type::Int,
                });
            }
            if name == "clock_sleep_ms" {
                require_capability(
                    ctx.capabilities,
                    CapabilityKind::Clock,
                    name,
                    *line,
                    *column,
                )?;
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("clock_sleep_ms expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::Int), env, ctx)?;
                if lowered.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "clock_sleep_ms expects an int argument, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::Int,
                });
            }
            if name == "clock_elapsed_ms" {
                require_capability(
                    ctx.capabilities,
                    CapabilityKind::Clock,
                    name,
                    *line,
                    *column,
                )?;
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("clock_elapsed_ms expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::Int), env, ctx)?;
                if lowered.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "clock_elapsed_ms expects an int argument, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::Int,
                });
            }
            if name == "env_get" {
                require_capability(ctx.capabilities, CapabilityKind::Env, name, *line, *column)?;
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("env_get expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if lowered.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!("env_get expects a string argument, got {}", lowered.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::Option(Box::new(Type::String)),
                });
            }
            if name == "crypto_sha256" {
                require_capability(
                    ctx.capabilities,
                    CapabilityKind::Crypto,
                    name,
                    *line,
                    *column,
                )?;
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("crypto_sha256 expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if lowered.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "crypto_sha256 expects a string argument, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::String,
                });
            }
            if name == "crypto_hmac_sha256" {
                require_capability(
                    ctx.capabilities,
                    CapabilityKind::Crypto,
                    name,
                    *line,
                    *column,
                )?;
                if args.len() != 2 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("crypto_hmac_sha256 expects 2 arguments, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let key = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if key.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!("crypto_hmac_sha256 expects a string key, got {}", key.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&key, env)?;
                let message = lower_expr_with_expected(&args[1], Some(&Type::String), env, ctx)?;
                if message.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "crypto_hmac_sha256 expects a string message, got {}",
                            message.ty()
                        ),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                move_lowered_value(&message, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![key, message],
                    ty: Type::String,
                });
            }
            if name == "crypto_constant_time_eq" {
                require_capability(
                    ctx.capabilities,
                    CapabilityKind::Crypto,
                    name,
                    *line,
                    *column,
                )?;
                if args.len() != 2 {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "crypto_constant_time_eq expects 2 arguments, got {}",
                            args.len()
                        ),
                    )
                    .with_span(*line, *column));
                }
                let left = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if left.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "crypto_constant_time_eq expects a string left argument, got {}",
                            left.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&left, env)?;
                let right = lower_expr_with_expected(&args[1], Some(&Type::String), env, ctx)?;
                if right.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "crypto_constant_time_eq expects a string right argument, got {}",
                            right.ty()
                        ),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                move_lowered_value(&right, env)?;
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![left, right],
                    ty: Type::Bool,
                });
            }
            if name == "first" || name == "last" {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr(&args[0], env, ctx)?;
                let element_ty = match lowered.ty() {
                    Type::Array(element_ty)
                    | Type::Slice(element_ty)
                    | Type::MutSlice(element_ty) => (*element_ty.clone()).clone(),
                    _ => {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "{name} expects an array or slice value, got {}",
                                lowered.ty()
                            ),
                        )
                        .with_span(args[0].line(), args[0].column()));
                    }
                };
                if matches!(lowered.ty(), Type::Slice(_) | Type::MutSlice(_))
                    && !element_ty.is_copy()
                {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "{name} requires a Copy element type when called on a borrowed slice, got {element_ty}"
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                if matches!(lowered.ty(), Type::Array(_)) && !element_ty.is_copy() {
                    move_lowered_owner_value(&lowered, env)?;
                }
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: vec![lowered],
                    ty: element_ty,
                });
            }
            if let Some(signature) = ctx.functions.get(name) {
                if signature.is_extern {
                    require_capability(
                        ctx.capabilities,
                        CapabilityKind::Ffi,
                        name,
                        *line,
                        *column,
                    )?;
                }
                if args.len() != signature.params.len() {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "function {name:?} expects {} arguments, got {}",
                            signature.params.len(),
                            args.len()
                        ),
                    )
                    .with_span(*line, *column));
                }
                let mut lowered_args = Vec::new();
                let mut temporary_borrows = Vec::new();
                for (arg, expected) in args.iter().zip(signature.params.iter()) {
                    let lowered = lower_expr_with_expected(arg, Some(expected), env, ctx)?;
                    if lowered.ty() != expected {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "function {name:?} expects argument type {expected}, got {}",
                                lowered.ty()
                            ),
                        )
                        .with_span(arg.line(), arg.column()));
                    }
                    record_temporary_borrows(&lowered, env, ctx, &mut temporary_borrows)?;
                    if !expected.is_copy() {
                        move_lowered_value(&lowered, env)?;
                    }
                    lowered_args.push(lowered);
                }
                release_temporary_borrows(&temporary_borrows, env);
                return Ok(Expr::Call {
                    name: name.clone(),
                    args: lowered_args,
                    ty: signature.return_ty.clone(),
                });
            }
            if name == "Some" {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("Option::Some expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let inner_expected = match expected {
                    Some(Type::Option(inner)) => Some(inner.as_ref()),
                    _ => None,
                };
                let lowered = lower_expr_with_expected(&args[0], inner_expected, env, ctx)?;
                let inner_ty = lowered.ty().clone();
                if let Some(expected_inner) = inner_expected
                    && &inner_ty != expected_inner
                {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "Option::Some expects payload type {expected_inner}, got {inner_ty}"
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                if !inner_ty.is_copy() {
                    move_lowered_value(&lowered, env)?;
                }
                return Ok(Expr::EnumVariant {
                    enum_name: String::from("Option"),
                    variant: String::from("Some"),
                    field_names: Vec::new(),
                    payloads: vec![lowered],
                    ty: Type::Option(Box::new(inner_ty)),
                });
            }
            if name == "Ok" {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("Result::Ok expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let Some(Type::Result(ok_ty, err_ty)) = expected else {
                    return Err(Diagnostic::new(
                        "type",
                        "Ok requires an expected Result<T, E> context",
                    )
                    .with_span(*line, *column));
                };
                let lowered = lower_expr_with_expected(&args[0], Some(ok_ty.as_ref()), env, ctx)?;
                if lowered.ty() != ok_ty.as_ref() {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "Result::Ok expects payload type {ok_ty}, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                if !ok_ty.is_copy() {
                    move_lowered_value(&lowered, env)?;
                }
                return Ok(Expr::EnumVariant {
                    enum_name: String::from("Result"),
                    variant: String::from("Ok"),
                    field_names: Vec::new(),
                    payloads: vec![lowered],
                    ty: Type::Result(ok_ty.clone(), err_ty.clone()),
                });
            }
            if name == "Err" {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("Result::Err expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let Some(Type::Result(ok_ty, err_ty)) = expected else {
                    return Err(Diagnostic::new(
                        "type",
                        "Err requires an expected Result<T, E> context",
                    )
                    .with_span(*line, *column));
                };
                let lowered = lower_expr_with_expected(&args[0], Some(err_ty.as_ref()), env, ctx)?;
                if lowered.ty() != err_ty.as_ref() {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "Result::Err expects payload type {err_ty}, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                if !err_ty.is_copy() {
                    move_lowered_value(&lowered, env)?;
                }
                return Ok(Expr::EnumVariant {
                    enum_name: String::from("Result"),
                    variant: String::from("Err"),
                    field_names: Vec::new(),
                    payloads: vec![lowered],
                    ty: Type::Result(ok_ty.clone(), err_ty.clone()),
                });
            }
            if let Some(variant) = resolve_variant(name, expected, ctx, *line, *column)? {
                return lower_variant_constructor(name, args, *line, *column, variant, env, ctx);
            }
            Err(Diagnostic::new(
                "type",
                message_with_suggestion(
                    format!("undefined function {name:?}"),
                    name,
                    ctx.functions
                        .keys()
                        .map(|candidate| candidate.rsplit("__").next().unwrap_or(candidate)),
                ),
            )
            .with_span(*line, *column))
        }
        syntax::Expr::MethodCall {
            base,
            method,
            type_args,
            args,
            line,
            column,
        } => {
            if !type_args.is_empty() {
                return Err(
                    Diagnostic::new("type", format!("method {:?} is not generic", method))
                        .with_span(*line, *column),
                );
            }
            if let syntax::Expr::VarRef {
                name: type_name, ..
            } = base.as_ref()
                && !env.contains_key(type_name)
                && let Some(methods) = ctx.methods.get(type_name)
                && let Some(signature) = methods.get(method)
            {
                if signature.has_self {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "method {:?} on {:?} requires a value receiver",
                            method, type_name
                        ),
                    )
                    .with_span(*line, *column));
                }
                let mut lowered_args = Vec::new();
                let mut temporary_borrows = Vec::new();
                if args.len() != signature.params.len() {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "associated function {:?} expects {} arguments, got {}",
                            method,
                            signature.params.len(),
                            args.len()
                        ),
                    )
                    .with_span(*line, *column));
                }
                for (arg, expected) in args.iter().zip(signature.params.iter()) {
                    let lowered = lower_expr_with_expected(arg, Some(expected), env, ctx)?;
                    if lowered.ty() != expected {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "associated function {:?} expects argument type {expected}, got {}",
                                method,
                                lowered.ty()
                            ),
                        )
                        .with_span(arg.line(), arg.column()));
                    }
                    record_temporary_borrows(&lowered, env, ctx, &mut temporary_borrows)?;
                    if !expected.is_copy() {
                        move_lowered_value(&lowered, env)?;
                    }
                    lowered_args.push(lowered);
                }
                release_temporary_borrows(&temporary_borrows, env);
                return Ok(Expr::Call {
                    name: signature.function_name.clone(),
                    args: lowered_args,
                    ty: signature.return_ty.clone(),
                });
            }
            let lowered_base = lower_expr(base, env, ctx)?;
            let Some(owner_name) = method_owner_name(lowered_base.ty()) else {
                return Err(Diagnostic::new(
                    "type",
                    format!("type {} does not support method calls", lowered_base.ty()),
                )
                .with_span(*line, *column));
            };
            let Some(methods) = ctx.methods.get(owner_name) else {
                return Err(Diagnostic::new(
                    "type",
                    format!("type {} has no impl methods", lowered_base.ty()),
                )
                .with_span(*line, *column));
            };
            let Some(signature) = methods.get(method) else {
                return Err(Diagnostic::new(
                    "type",
                    format!("type {} has no method {:?}", lowered_base.ty(), method),
                )
                .with_span(*line, *column));
            };
            if !signature.has_self {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "associated function {:?} must be called as {}.{}()",
                        method, owner_name, method
                    ),
                )
                .with_span(*line, *column));
            }
            if args.len() + 1 != signature.params.len() {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "method {:?} expects {} arguments, got {}",
                        method,
                        signature.params.len() - 1,
                        args.len()
                    ),
                )
                .with_span(*line, *column));
            }
            let mut lowered_args = Vec::new();
            let mut temporary_borrows = Vec::new();
            let self_expected = &signature.params[0];
            if lowered_base.ty() != self_expected {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "method {:?} expects receiver type {self_expected}, got {}",
                        method,
                        lowered_base.ty()
                    ),
                )
                .with_span(base.line(), base.column()));
            }
            record_temporary_borrows(&lowered_base, env, ctx, &mut temporary_borrows)?;
            if !self_expected.is_copy() {
                move_lowered_value(&lowered_base, env)?;
            }
            lowered_args.push(lowered_base);
            for (arg, expected) in args.iter().zip(signature.params.iter().skip(1)) {
                let lowered = lower_expr_with_expected(arg, Some(expected), env, ctx)?;
                if lowered.ty() != expected {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "method {:?} expects argument type {expected}, got {}",
                            method,
                            lowered.ty()
                        ),
                    )
                    .with_span(arg.line(), arg.column()));
                }
                record_temporary_borrows(&lowered, env, ctx, &mut temporary_borrows)?;
                if !expected.is_copy() {
                    move_lowered_value(&lowered, env)?;
                }
                lowered_args.push(lowered);
            }
            release_temporary_borrows(&temporary_borrows, env);
            Ok(Expr::Call {
                name: signature.function_name.clone(),
                args: lowered_args,
                ty: signature.return_ty.clone(),
            })
        }
        syntax::Expr::BinaryAdd {
            lhs,
            rhs,
            line,
            column,
        } => {
            let lhs = lower_expr(lhs, env, ctx)?;
            let rhs = lower_expr(rhs, env, ctx)?;
            let lhs_ty = lhs.ty().clone();
            let rhs_ty = rhs.ty().clone();
            if lhs_ty != rhs_ty || !matches!(lhs_ty, Type::Int | Type::String) {
                return Err(
                    Diagnostic::new(
                        "type",
                        format!(
                            "operator '+' expects matching int or string operands, got {lhs_ty} and {rhs_ty}"
                        ),
                    )
                    .with_span(*line, *column),
                );
            }
            Ok(Expr::BinaryAdd {
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                ty: lhs_ty,
            })
        }
        syntax::Expr::BinaryCompare {
            op,
            lhs,
            rhs,
            line,
            column,
        } => {
            let lhs = lower_expr(lhs, env, ctx)?;
            let rhs = lower_expr(rhs, env, ctx)?;
            let lhs_ty = lhs.ty().clone();
            let rhs_ty = rhs.ty().clone();
            match op {
                syntax::CompareOp::Eq | syntax::CompareOp::Ne => {
                    if lhs_ty != rhs_ty {
                        return Err(
                            Diagnostic::new(
                                "type",
                                format!(
                                    "operator '{}' expects matching operand types, got {lhs_ty} and {rhs_ty}",
                                    op.lexeme()
                                ),
                            )
                            .with_span(*line, *column),
                        );
                    }
                }
                syntax::CompareOp::Lt
                | syntax::CompareOp::Le
                | syntax::CompareOp::Gt
                | syntax::CompareOp::Ge => {
                    if lhs_ty != Type::Int || rhs_ty != Type::Int {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "operator '{}' expects int operands, got {lhs_ty} and {rhs_ty}",
                                op.lexeme()
                            ),
                        )
                        .with_span(*line, *column));
                    }
                }
            }
            Ok(Expr::BinaryCompare {
                op: lower_compare_op(*op),
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                ty: Type::Bool,
            })
        }
        syntax::Expr::Try { expr, line, column } => {
            let Some(current_return) = ctx.current_return.as_ref() else {
                return Err(
                    Diagnostic::new("control", "`?` is only valid inside a function")
                        .with_span(*line, *column),
                );
            };
            let wrapped_expected = expected.and_then(|inner| match current_return {
                Type::Option(_) => Some(Type::Option(Box::new(inner.clone()))),
                Type::Result(_, err) => Some(Type::Result(
                    Box::new(inner.clone()),
                    Box::new((**err).clone()),
                )),
                _ => None,
            });
            let lowered = if let Some(wrapped_expected) = wrapped_expected.as_ref() {
                lower_expr_with_expected(expr, Some(wrapped_expected), env, ctx)?
            } else {
                lower_expr(expr, env, ctx)?
            };
            let result_ty = match (lowered.ty(), current_return) {
                (Type::Option(inner), Type::Option(_)) => (**inner).clone(),
                (Type::Result(ok, err), Type::Result(_, return_err)) => {
                    if err.as_ref() != return_err.as_ref() {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "`?` cannot propagate Result error type {err} from a function returning Result<_, {return_err}>"
                            ),
                        )
                        .with_span(*line, *column));
                    }
                    (**ok).clone()
                }
                (Type::Option(_), other) => {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "`?` on Option<T> requires the enclosing function to return Option<_>, got {other}"
                        ),
                    )
                    .with_span(*line, *column));
                }
                (Type::Result(_, _), other) => {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "`?` on Result<T, E> requires the enclosing function to return Result<_, E>, got {other}"
                        ),
                    )
                    .with_span(*line, *column));
                }
                (other, _) => {
                    return Err(Diagnostic::new(
                        "type",
                        format!("`?` expects an Option<T> or Result<T, E> expression, got {other}"),
                    )
                    .with_span(*line, *column));
                }
            };
            if !lowered.ty().is_copy() {
                move_lowered_owner_value(&lowered, env)?;
            }
            Ok(Expr::Try {
                expr: Box::new(lowered),
                ty: result_ty,
            })
        }
        syntax::Expr::Await { expr, line, column } => {
            let lowered = lower_expr(expr, env, ctx)?;
            let inner_ty = match lowered.ty() {
                Type::Task(inner) => (**inner).clone(),
                other => {
                    return Err(Diagnostic::new(
                        "type",
                        format!("await expects a Task<T>, got {other}"),
                    )
                    .with_span(*line, *column));
                }
            };
            move_lowered_value(&lowered, env)?;
            Ok(Expr::Await {
                expr: Box::new(lowered),
                ty: inner_ty,
            })
        }
        syntax::Expr::StructLiteral {
            name,
            fields,
            line,
            column,
        } => {
            if let Some(variant) = resolve_variant(name, expected, ctx, *line, *column)?
                && !variant.payload_names.is_empty()
            {
                return lower_named_variant_constructor(
                    name, fields, *line, *column, variant, env, ctx,
                );
            }
            let concrete_name = if ctx.structs.contains_key(name) {
                name.clone()
            } else if let Some(Type::Struct(expected_name)) = expected {
                let prefix = format!("{name}__");
                if expected_name.starts_with(&prefix) && ctx.structs.contains_key(expected_name) {
                    expected_name.clone()
                } else {
                    return Err(
                        Diagnostic::new("type", format!("undefined struct {name:?}"))
                            .with_span(*line, *column),
                    );
                }
            } else {
                return Err(
                    Diagnostic::new("type", format!("undefined struct {name:?}"))
                        .with_span(*line, *column),
                );
            };
            let struct_def = ctx
                .structs
                .get(&concrete_name)
                .expect("concrete struct name checked above");
            let mut field_defs = HashMap::new();
            for field in &struct_def.fields {
                field_defs.insert(field.name.clone(), field.ty.clone());
            }
            let mut lowered_fields = HashMap::new();
            for field in fields {
                let expected = field_defs.get(&field.name).ok_or_else(|| {
                    Diagnostic::new(
                        "type",
                        message_with_suggestion(
                            format!("struct {concrete_name:?} has no field {:?}", field.name),
                            &field.name,
                            field_defs.keys(),
                        ),
                    )
                    .with_span(field.line, field.column)
                })?;
                if lowered_fields.contains_key(&field.name) {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "duplicate field {:?} in struct literal {concrete_name:?}",
                            field.name
                        ),
                    )
                    .with_span(field.line, field.column));
                }
                let lowered = lower_expr_with_expected(&field.expr, Some(expected), env, ctx)?;
                if lowered.ty() != expected {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "struct {concrete_name:?} field {:?} expects {expected}, got {}",
                            field.name,
                            lowered.ty()
                        ),
                    )
                    .with_span(field.line, field.column));
                }
                if !expected.is_copy() {
                    move_lowered_owner_value(&lowered, env)?;
                }
                lowered_fields.insert(
                    field.name.clone(),
                    StructFieldValue {
                        name: field.name.clone(),
                        expr: lowered,
                    },
                );
            }
            let mut ordered_fields = Vec::new();
            for field in &struct_def.fields {
                let lowered = lowered_fields.remove(&field.name).ok_or_else(|| {
                    Diagnostic::new(
                        "type",
                        format!(
                            "struct literal {concrete_name:?} is missing field {:?}",
                            field.name
                        ),
                    )
                    .with_span(*line, *column)
                })?;
                ordered_fields.push(lowered);
            }
            Ok(Expr::StructLiteral {
                name: concrete_name.clone(),
                fields: ordered_fields,
                ty: Type::Struct(concrete_name),
            })
        }
        syntax::Expr::TupleLiteral {
            elements,
            line,
            column,
        } => {
            let expected_elements = match expected {
                Some(Type::Tuple(expected_elements))
                    if expected_elements.len() == elements.len() =>
                {
                    Some(expected_elements)
                }
                _ => None,
            };
            let mut lowered_elements = Vec::new();
            let mut element_tys = Vec::new();
            let mut temporary_borrows = Vec::new();
            for (index, element) in elements.iter().enumerate() {
                let lowered = lower_expr_with_expected(
                    element,
                    expected_elements.and_then(|expected| expected.get(index)),
                    env,
                    ctx,
                )?;
                record_temporary_borrows(&lowered, env, ctx, &mut temporary_borrows)?;
                if !lowered.ty().is_copy() {
                    move_lowered_owner_value(&lowered, env)?;
                }
                element_tys.push(lowered.ty().clone());
                lowered_elements.push(lowered);
            }
            release_temporary_borrows(&temporary_borrows, env);
            if lowered_elements.len() < 2 {
                return Err(Diagnostic::new(
                    "type",
                    "tuple literals require at least two elements",
                )
                .with_span(*line, *column));
            }
            Ok(Expr::TupleLiteral {
                elements: lowered_elements,
                ty: Type::Tuple(element_tys),
            })
        }
        syntax::Expr::FieldAccess {
            base,
            field,
            line,
            column,
        } => {
            let lowered_base = lower_projection_base_expr(base, env, ctx)?;
            let Type::Struct(struct_name) = lowered_base.ty() else {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "field access expects a struct value, got {}",
                        lowered_base.ty()
                    ),
                )
                .with_span(*line, *column));
            };
            let struct_def = ctx.structs.get(struct_name).ok_or_else(|| {
                Diagnostic::new(
                    "type",
                    format!("internal error: missing struct definition {struct_name:?}"),
                )
                .with_span(*line, *column)
            })?;
            let field_ty = struct_def
                .fields
                .iter()
                .find(|entry| entry.name == *field)
                .map(|entry| entry.ty.clone())
                .ok_or_else(|| {
                    Diagnostic::new(
                        "type",
                        message_with_suggestion(
                            format!("struct {struct_name:?} has no field {field:?}"),
                            field,
                            struct_def.fields.iter().map(|entry| entry.name.as_str()),
                        ),
                    )
                    .with_span(*line, *column)
                })?;
            if !field_ty.is_copy() {
                let projected = Expr::FieldAccess {
                    base: Box::new(lowered_base),
                    field: field.clone(),
                    ty: field_ty.clone(),
                };
                move_lowered_owner_value(&projected, env)?;
                return Ok(projected);
            }
            Ok(Expr::FieldAccess {
                base: Box::new(lowered_base),
                field: field.clone(),
                ty: field_ty,
            })
        }
        syntax::Expr::TupleIndex {
            base,
            index,
            line,
            column,
        } => {
            let lowered_base = lower_projection_base_expr(base, env, ctx)?;
            let Type::Tuple(element_tys) = lowered_base.ty() else {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "tuple index expects a tuple value, got {}",
                        lowered_base.ty()
                    ),
                )
                .with_span(*line, *column));
            };
            let element_ty = element_tys.get(*index).cloned().ok_or_else(|| {
                Diagnostic::new(
                    "type",
                    format!(
                        "tuple index {} is out of bounds for {}",
                        index,
                        lowered_base.ty()
                    ),
                )
                .with_span(*line, *column)
            })?;
            if !element_ty.is_copy() {
                let projected = Expr::TupleIndex {
                    base: Box::new(lowered_base),
                    index: *index,
                    ty: element_ty.clone(),
                };
                move_lowered_owner_value(&projected, env)?;
                return Ok(projected);
            }
            Ok(Expr::TupleIndex {
                base: Box::new(lowered_base),
                index: *index,
                ty: element_ty,
            })
        }
        syntax::Expr::MapLiteral {
            entries,
            line,
            column,
        } => {
            if entries.is_empty() {
                return Err(Diagnostic::new(
                    "type",
                    "empty map literals are not yet supported in stage1",
                )
                .with_span(*line, *column));
            }
            let mut lowered_entries = Vec::new();
            let mut key_ty = None;
            let mut value_ty = None;
            let mut temporary_borrows = Vec::new();
            for entry in entries {
                let lowered_key = lower_expr(&entry.key, env, ctx)?;
                let lowered_value = lower_expr(&entry.value, env, ctx)?;
                if let Some(expected) = key_ty.as_ref() {
                    if lowered_key.ty() != expected {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "map literal expects matching key types, got {expected} and {}",
                                lowered_key.ty()
                            ),
                        )
                        .with_span(entry.line, entry.column));
                    }
                } else {
                    key_ty = Some(lowered_key.ty().clone());
                }
                if let Some(expected) = value_ty.as_ref() {
                    if lowered_value.ty() != expected {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "map literal expects matching value types, got {expected} and {}",
                                lowered_value.ty()
                            ),
                        )
                        .with_span(entry.line, entry.column));
                    }
                } else {
                    value_ty = Some(lowered_value.ty().clone());
                }
                if !lowered_key.ty().supports_map_key() {
                    return Err(Diagnostic::new(
                        "type",
                        format!("map literal key type {} is not supported", lowered_key.ty()),
                    )
                    .with_span(entry.line, entry.column));
                }
                record_temporary_borrows(&lowered_key, env, ctx, &mut temporary_borrows)?;
                record_temporary_borrows(&lowered_value, env, ctx, &mut temporary_borrows)?;
                if !lowered_key.ty().is_copy() {
                    move_lowered_owner_value(&lowered_key, env)?;
                }
                if !lowered_value.ty().is_copy() {
                    move_lowered_owner_value(&lowered_value, env)?;
                }
                lowered_entries.push(MapEntry {
                    key: lowered_key,
                    value: lowered_value,
                });
            }
            release_temporary_borrows(&temporary_borrows, env);
            let key_ty = key_ty.expect("non-empty map literal must have a key type");
            let value_ty = value_ty.expect("non-empty map literal must have a value type");
            Ok(Expr::MapLiteral {
                entries: lowered_entries,
                ty: Type::Map(Box::new(key_ty), Box::new(value_ty)),
            })
        }
        syntax::Expr::ArrayLiteral {
            elements,
            line,
            column,
        } => {
            if elements.is_empty() {
                return Err(Diagnostic::new(
                    "type",
                    "empty array literals are not yet supported in stage1",
                )
                .with_span(*line, *column));
            }
            let mut lowered_elements = Vec::new();
            let mut element_ty = None;
            let mut temporary_borrows = Vec::new();
            for element in elements {
                let lowered = lower_expr(element, env, ctx)?;
                if let Some(expected) = element_ty.as_ref() {
                    if lowered.ty() != expected {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "array literal expects matching element types, got {expected} and {}",
                                lowered.ty()
                            ),
                        )
                        .with_span(element.line(), element.column()));
                    }
                } else {
                    element_ty = Some(lowered.ty().clone());
                }
                record_temporary_borrows(&lowered, env, ctx, &mut temporary_borrows)?;
                if !lowered.ty().is_copy() {
                    move_lowered_owner_value(&lowered, env)?;
                }
                lowered_elements.push(lowered);
            }
            release_temporary_borrows(&temporary_borrows, env);
            let element_ty = element_ty.expect("non-empty array literal must have an element type");
            Ok(Expr::ArrayLiteral {
                elements: lowered_elements,
                ty: Type::Array(Box::new(element_ty)),
            })
        }
        syntax::Expr::Slice {
            base,
            start,
            end,
            line,
            column,
        } => {
            let lowered_base = lower_projection_base_expr(base, env, ctx)?;
            let element_ty = match lowered_base.ty() {
                Type::Array(element_ty) | Type::Slice(element_ty) | Type::MutSlice(element_ty) => {
                    (*element_ty.clone()).clone()
                }
                _ => {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "slice expects an array or slice value, got {}",
                            lowered_base.ty()
                        ),
                    )
                    .with_span(*line, *column));
                }
            };
            if !is_borrowable_slice_base(&lowered_base) {
                return Err(Diagnostic::new(
                    "type",
                    "borrowed slices currently require a named array, field, tuple field, or slice value",
                )
                .with_span(*line, *column));
            }
            let lowered_start = if let Some(start) = start {
                let lowered = lower_expr(start, env, ctx)?;
                if lowered.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!("array slice start expects int, got {}", lowered.ty()),
                    )
                    .with_span(start.line(), start.column()));
                }
                Some(Box::new(lowered))
            } else {
                None
            };
            let lowered_end = if let Some(end) = end {
                let lowered = lower_expr(end, env, ctx)?;
                if lowered.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!("array slice end expects int, got {}", lowered.ty()),
                    )
                    .with_span(end.line(), end.column()));
                }
                Some(Box::new(lowered))
            } else {
                None
            };
            let ty = match (expected, lowered_base.ty()) {
                (Some(Type::MutSlice(_)), Type::Array(_) | Type::MutSlice(_)) => {
                    Type::MutSlice(Box::new(element_ty))
                }
                (_, Type::MutSlice(_)) => Type::MutSlice(Box::new(element_ty)),
                _ => Type::Slice(Box::new(element_ty)),
            };
            Ok(Expr::Slice {
                base: Box::new(lowered_base),
                start: lowered_start,
                end: lowered_end,
                ty,
            })
        }
        syntax::Expr::Index {
            base,
            index,
            line,
            column,
        } => {
            let lowered_base = lower_projection_base_expr(base, env, ctx)?;
            let lowered_index = lower_expr(index, env, ctx)?;
            let result_ty = match lowered_base.ty() {
                Type::Array(element_ty) => {
                    if lowered_index.ty() != &Type::Int {
                        return Err(Diagnostic::new(
                            "type",
                            format!("array index expects int, got {}", lowered_index.ty()),
                        )
                        .with_span(*line, *column));
                    }
                    let element_ty = (*element_ty.clone()).clone();
                    if !element_ty.is_copy() {
                        move_lowered_owner_value(&lowered_base, env)?;
                    }
                    element_ty
                }
                Type::Slice(element_ty) | Type::MutSlice(element_ty) => {
                    if lowered_index.ty() != &Type::Int {
                        return Err(Diagnostic::new(
                            "type",
                            format!("slice index expects int, got {}", lowered_index.ty()),
                        )
                        .with_span(*line, *column));
                    }
                    let element_ty = (*element_ty.clone()).clone();
                    if !element_ty.is_copy() {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "borrowed slice indexing requires a Copy element type, got {element_ty}"
                            ),
                        )
                        .with_span(*line, *column));
                    }
                    element_ty
                }
                Type::Map(key_ty, value_ty) => {
                    if lowered_index.ty() != key_ty.as_ref() {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "map index expects key type {}, got {}",
                                key_ty,
                                lowered_index.ty()
                            ),
                        )
                        .with_span(*line, *column));
                    }
                    let value_ty = (*value_ty.clone()).clone();
                    if !value_ty.is_copy() {
                        move_lowered_owner_value(&lowered_base, env)?;
                    }
                    value_ty
                }
                _ => {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "index expects an array or map value, got {}",
                            lowered_base.ty()
                        ),
                    )
                    .with_span(*line, *column));
                }
            };
            Ok(Expr::Index {
                base: Box::new(lowered_base),
                index: Box::new(lowered_index),
                ty: result_ty,
            })
        }
    }
}

fn lower_async_runtime_intrinsic(
    name: &str,
    type_args: &[syntax::TypeName],
    args: &[syntax::Expr],
    line: usize,
    column: usize,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Expr, Diagnostic> {
    if type_args.len() != 1 {
        return Err(Diagnostic::new(
            "type",
            format!(
                "{name} expects 1 explicit type argument, got {}",
                type_args.len()
            ),
        )
        .with_span(line, column));
    }
    let value_ty = lower_type(
        &type_args[0],
        ctx.structs,
        ctx.enums,
        ctx.aliases,
        line,
        column,
    )?;
    let expected_len = match name {
        "async_channel" => 0,
        "async_ready"
        | "async_is_canceled"
        | "async_spawn"
        | "async_join"
        | "async_cancel"
        | "async_recv"
        | "async_selected"
        | "async_selected_value" => 1,
        "async_timeout" | "async_send" | "async_select" => 2,
        _ => unreachable!("async runtime intrinsic checked before lowering"),
    };
    if args.len() != expected_len {
        return Err(Diagnostic::new(
            "type",
            format!(
                "{name} expects {expected_len} arguments, got {}",
                args.len()
            ),
        )
        .with_span(line, column));
    }

    let lowered_args = match name {
        "async_ready" => {
            let value = lower_expr_with_expected(&args[0], Some(&value_ty), env, ctx)?;
            if value.ty() != &value_ty {
                return Err(Diagnostic::new(
                    "type",
                    format!("{name} expects value type {value_ty}, got {}", value.ty()),
                )
                .with_span(args[0].line(), args[0].column()));
            }
            move_lowered_value(&value, env)?;
            vec![value]
        }
        "async_spawn" | "async_cancel" | "async_is_canceled" => {
            let expected = Type::Task(Box::new(value_ty.clone()));
            let task = lower_expr_with_expected(&args[0], Some(&expected), env, ctx)?;
            if task.ty() != &expected {
                return Err(Diagnostic::new(
                    "type",
                    format!("{name} expects {expected}, got {}", task.ty()),
                )
                .with_span(args[0].line(), args[0].column()));
            }
            move_lowered_value(&task, env)?;
            vec![task]
        }
        "async_join" => {
            let expected = Type::JoinHandle(Box::new(value_ty.clone()));
            let handle = lower_expr_with_expected(&args[0], Some(&expected), env, ctx)?;
            if handle.ty() != &expected {
                return Err(Diagnostic::new(
                    "type",
                    format!("{name} expects {expected}, got {}", handle.ty()),
                )
                .with_span(args[0].line(), args[0].column()));
            }
            move_lowered_value(&handle, env)?;
            vec![handle]
        }
        "async_timeout" => {
            let expected = Type::Task(Box::new(value_ty.clone()));
            let task = lower_expr_with_expected(&args[0], Some(&expected), env, ctx)?;
            let ms = lower_expr_with_expected(&args[1], Some(&Type::Int), env, ctx)?;
            if task.ty() != &expected || ms.ty() != &Type::Int {
                return Err(Diagnostic::new(
                    "type",
                    format!("{name} expects Task<{value_ty}> and int arguments"),
                )
                .with_span(line, column));
            }
            move_lowered_value(&task, env)?;
            vec![task, ms]
        }
        "async_channel" => Vec::new(),
        "async_send" => {
            let channel_ty = Type::AsyncChannel(Box::new(value_ty.clone()));
            let channel = lower_expr_with_expected(&args[0], Some(&channel_ty), env, ctx)?;
            let value = lower_expr_with_expected(&args[1], Some(&value_ty), env, ctx)?;
            if channel.ty() != &channel_ty || value.ty() != &value_ty {
                return Err(Diagnostic::new(
                    "type",
                    format!("{name} expects AsyncChannel<{value_ty}> and {value_ty} arguments"),
                )
                .with_span(line, column));
            }
            move_lowered_value(&channel, env)?;
            move_lowered_value(&value, env)?;
            vec![channel, value]
        }
        "async_recv" => {
            let channel_ty = Type::AsyncChannel(Box::new(value_ty.clone()));
            let channel = lower_expr_with_expected(&args[0], Some(&channel_ty), env, ctx)?;
            if channel.ty() != &channel_ty {
                return Err(Diagnostic::new(
                    "type",
                    format!("{name} expects {channel_ty}, got {}", channel.ty()),
                )
                .with_span(args[0].line(), args[0].column()));
            }
            move_lowered_value(&channel, env)?;
            vec![channel]
        }
        "async_select" => {
            let task_ty = Type::Task(Box::new(Type::Option(Box::new(value_ty.clone()))));
            let left = lower_expr_with_expected(&args[0], Some(&task_ty), env, ctx)?;
            let right = lower_expr_with_expected(&args[1], Some(&task_ty), env, ctx)?;
            if left.ty() != &task_ty || right.ty() != &task_ty {
                return Err(Diagnostic::new(
                    "type",
                    format!("{name} expects two {task_ty} arguments"),
                )
                .with_span(line, column));
            }
            move_lowered_value(&left, env)?;
            move_lowered_value(&right, env)?;
            vec![left, right]
        }
        "async_selected" | "async_selected_value" => {
            let result_ty = Type::SelectResult(Box::new(value_ty.clone()));
            let result = lower_expr_with_expected(&args[0], Some(&result_ty), env, ctx)?;
            if result.ty() != &result_ty {
                return Err(Diagnostic::new(
                    "type",
                    format!("{name} expects {result_ty}, got {}", result.ty()),
                )
                .with_span(args[0].line(), args[0].column()));
            }
            move_lowered_value(&result, env)?;
            vec![result]
        }
        _ => unreachable!("async runtime intrinsic checked before lowering"),
    };

    let ty = match name {
        "async_ready" | "async_cancel" | "async_join" => Type::Task(Box::new(value_ty)),
        "async_spawn" => Type::JoinHandle(Box::new(value_ty)),
        "async_is_canceled" => Type::Bool,
        "async_timeout" => Type::Task(Box::new(Type::Option(Box::new(value_ty)))),
        "async_channel" => Type::AsyncChannel(Box::new(value_ty)),
        "async_send" => Type::Task(Box::new(Type::AsyncChannel(Box::new(value_ty)))),
        "async_recv" => Type::Task(Box::new(Type::Option(Box::new(value_ty)))),
        "async_select" => Type::Task(Box::new(Type::SelectResult(Box::new(value_ty)))),
        "async_selected" => Type::Int,
        "async_selected_value" => Type::Option(Box::new(value_ty)),
        _ => unreachable!("async runtime intrinsic checked before lowering"),
    };
    Ok(Expr::Call {
        name: name.to_string(),
        args: lowered_args,
        ty,
    })
}

fn lower_projection_base_expr(
    expr: &syntax::Expr,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Expr, Diagnostic> {
    match expr {
        syntax::Expr::VarRef { name, line, column } => {
            let Some(binding) = env.get(name) else {
                return lower_expr(expr, env, ctx);
            };
            if binding.moved {
                return Err(ownership_error(
                    OWNERSHIP_USE_AFTER_MOVE,
                    format!("use of moved value {name:?}"),
                )
                .with_span(*line, *column));
            }
            Ok(Expr::VarRef {
                name: name.clone(),
                ty: binding.ty.clone(),
            })
        }
        syntax::Expr::FieldAccess {
            base,
            field,
            line,
            column,
        } => {
            let lowered_base = lower_projection_base_expr(base, env, ctx)?;
            let Type::Struct(struct_name) = lowered_base.ty() else {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "field access expects a struct value, got {}",
                        lowered_base.ty()
                    ),
                )
                .with_span(*line, *column));
            };
            let struct_def = ctx.structs.get(struct_name).ok_or_else(|| {
                Diagnostic::new(
                    "type",
                    format!("internal error: missing struct definition {struct_name:?}"),
                )
                .with_span(*line, *column)
            })?;
            let field_ty = struct_def
                .fields
                .iter()
                .find(|entry| entry.name == *field)
                .map(|entry| entry.ty.clone())
                .ok_or_else(|| {
                    Diagnostic::new(
                        "type",
                        message_with_suggestion(
                            format!("struct {struct_name:?} has no field {field:?}"),
                            field,
                            struct_def.fields.iter().map(|entry| entry.name.as_str()),
                        ),
                    )
                    .with_span(*line, *column)
                })?;
            let projected = Expr::FieldAccess {
                base: Box::new(lowered_base),
                field: field.clone(),
                ty: field_ty,
            };
            ensure_lowered_projection_traversable(&projected, env)?;
            Ok(projected)
        }
        syntax::Expr::TupleIndex {
            base,
            index,
            line,
            column,
        } => {
            let lowered_base = lower_projection_base_expr(base, env, ctx)?;
            let Type::Tuple(element_tys) = lowered_base.ty() else {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "tuple index expects a tuple value, got {}",
                        lowered_base.ty()
                    ),
                )
                .with_span(*line, *column));
            };
            let element_ty = element_tys.get(*index).cloned().ok_or_else(|| {
                Diagnostic::new(
                    "type",
                    format!(
                        "tuple index {} is out of bounds for {}",
                        index,
                        lowered_base.ty()
                    ),
                )
                .with_span(*line, *column)
            })?;
            let projected = Expr::TupleIndex {
                base: Box::new(lowered_base),
                index: *index,
                ty: element_ty,
            };
            ensure_lowered_projection_traversable(&projected, env)?;
            Ok(projected)
        }
        _ => lower_expr(expr, env, ctx),
    }
}

fn validate_ffi_signature(function: &syntax::Function, return_ty: &Type) -> Result<(), Diagnostic> {
    validate_ffi_type(return_ty, function.line, function.column)?;
    for param in &function.params {
        validate_ffi_type_name(&param.ty, param.line, param.column)?;
    }
    Ok(())
}

fn validate_ffi_type_name(
    ty: &syntax::TypeName,
    line: usize,
    column: usize,
) -> Result<(), Diagnostic> {
    match ty {
        syntax::TypeName::Int | syntax::TypeName::Bool | syntax::TypeName::String => Ok(()),
        syntax::TypeName::Ptr(inner) | syntax::TypeName::MutPtr(inner) => {
            validate_ffi_type_name(inner, line, column)
        }
        _ => Err(Diagnostic::new(
            "type",
            "FFI signatures only support int, bool, string, ptr<T>, and mutptr<T> in stage1",
        )
        .with_span(line, column)),
    }
}

fn validate_ffi_type(ty: &Type, line: usize, column: usize) -> Result<(), Diagnostic> {
    match ty {
        Type::Int | Type::Bool | Type::String => Ok(()),
        Type::Ptr(inner) | Type::MutPtr(inner) => validate_ffi_type(inner, line, column),
        _ => Err(Diagnostic::new(
            "type",
            "FFI signatures only support int, bool, string, ptr<T>, and mutptr<T> in stage1",
        )
        .with_span(line, column)),
    }
}

fn require_capability(
    capabilities: &CapabilityConfig,
    kind: CapabilityKind,
    intrinsic_name: &str,
    line: usize,
    column: usize,
) -> Result<(), Diagnostic> {
    if capabilities.enabled(kind) {
        return Ok(());
    }
    let requirement = if kind == CapabilityKind::Env {
        String::from("[capabilities].env = [\"NAME\"] or env_unrestricted = true")
    } else {
        format!("[capabilities].{} = true", kind.name())
    };
    Err(Diagnostic::new(
        "capability",
        format!("call to {intrinsic_name:?} requires {requirement}"),
    )
    .with_span(line, column))
}

fn resolve_variant<'a>(
    name: &str,
    expected: Option<&Type>,
    ctx: &'a LowerContext<'_>,
    line: usize,
    column: usize,
) -> Result<Option<&'a VariantInfo>, Diagnostic> {
    let Some(candidates) = ctx.variants.get(name) else {
        return Ok(None);
    };
    if let Some(Type::Enum(expected_enum)) = expected {
        return Ok(candidates
            .iter()
            .find(|variant| &variant.enum_name == expected_enum));
    }
    if candidates.len() == 1 {
        return Ok(candidates.first());
    }
    Err(Diagnostic::new(
        "type",
        format!("enum variant {name:?} is ambiguous without an expected enum type"),
    )
    .with_span(line, column))
}

fn lower_variant_constructor(
    name: &str,
    args: &[syntax::Expr],
    line: usize,
    column: usize,
    variant: &VariantInfo,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Expr, Diagnostic> {
    if !variant.payload_names.is_empty() {
        return Err(Diagnostic::new(
            "type",
            format!("enum variant {name:?} requires named payload fields"),
        )
        .with_span(line, column));
    }
    if variant.payload_tys.is_empty() {
        return Err(Diagnostic::new(
            "type",
            format!("enum variant {name:?} does not take arguments"),
        )
        .with_span(line, column));
    }
    if args.len() != variant.payload_tys.len() {
        return Err(Diagnostic::new(
            "type",
            format!(
                "enum variant {name:?} expects {} arguments, got {}",
                variant.payload_tys.len(),
                args.len()
            ),
        )
        .with_span(line, column));
    }
    let mut lowered_payloads = Vec::new();
    for (arg, expected) in args.iter().zip(variant.payload_tys.iter()) {
        let lowered = lower_expr_with_expected(arg, Some(expected), env, ctx)?;
        if lowered.ty() != expected {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "enum variant {name:?} expects payload type {expected}, got {}",
                    lowered.ty()
                ),
            )
            .with_span(arg.line(), arg.column()));
        }
        if !expected.is_copy() {
            move_lowered_value(&lowered, env)?;
        }
        lowered_payloads.push(lowered);
    }
    Ok(Expr::EnumVariant {
        enum_name: variant.enum_name.clone(),
        variant: name.to_string(),
        field_names: Vec::new(),
        payloads: lowered_payloads,
        ty: Type::Enum(variant.enum_name.clone()),
    })
}

fn lower_named_variant_constructor(
    name: &str,
    fields: &[syntax::StructFieldValue],
    line: usize,
    column: usize,
    variant: &VariantInfo,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Expr, Diagnostic> {
    let mut lowered_fields = HashMap::new();
    for field in fields {
        let Some(position) = variant
            .payload_names
            .iter()
            .position(|payload_name| payload_name == &field.name)
        else {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "enum variant {name:?} has no named payload {:?}",
                    field.name
                ),
            )
            .with_span(field.line, field.column));
        };
        if lowered_fields.contains_key(&field.name) {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "duplicate named payload {:?} in enum variant literal {name:?}",
                    field.name
                ),
            )
            .with_span(field.line, field.column));
        }
        let expected = &variant.payload_tys[position];
        let lowered = lower_expr_with_expected(&field.expr, Some(expected), env, ctx)?;
        if lowered.ty() != expected {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "enum variant {name:?} payload {:?} expects {expected}, got {}",
                    field.name,
                    lowered.ty()
                ),
            )
            .with_span(field.line, field.column));
        }
        if !expected.is_copy() {
            move_lowered_owner_value(&lowered, env)?;
        }
        lowered_fields.insert(field.name.clone(), lowered);
    }
    let mut ordered_payloads = Vec::new();
    for payload_name in &variant.payload_names {
        let lowered = lowered_fields.remove(payload_name).ok_or_else(|| {
            Diagnostic::new(
                "type",
                format!(
                    "enum variant literal {name:?} is missing named payload {:?}",
                    payload_name
                ),
            )
            .with_span(line, column)
        })?;
        ordered_payloads.push(lowered);
    }
    Ok(Expr::EnumVariant {
        enum_name: variant.enum_name.clone(),
        variant: name.to_string(),
        field_names: variant.payload_names.clone(),
        payloads: ordered_payloads,
        ty: Type::Enum(variant.enum_name.clone()),
    })
}

fn move_lowered_value(expr: &Expr, env: &mut HashMap<String, Binding>) -> Result<(), Diagnostic> {
    let Expr::VarRef { name, .. } = expr else {
        return Ok(());
    };
    mark_projection_moved(name, Vec::new(), env)
}

fn move_lowered_owner_value(
    expr: &Expr,
    env: &mut HashMap<String, Binding>,
) -> Result<(), Diagnostic> {
    let Some((name, projection)) = ownership_projection(expr) else {
        return Ok(());
    };
    mark_projection_moved(name, projection, env)
}

fn mark_projection_moved(
    name: &str,
    projection: ProjectionPath,
    env: &mut HashMap<String, Binding>,
) -> Result<(), Diagnostic> {
    let binding = env.get_mut(name).ok_or_else(|| {
        Diagnostic::new(
            "type",
            format!("internal error: missing binding for moved value {name:?}"),
        )
    })?;
    if binding.active_borrow_count > 0 {
        return Err(ownership_error(
            OWNERSHIP_MOVE_WHILE_BORROWED,
            format!("cannot move value {name:?} while borrowed slices are still live"),
        ));
    }
    if projection_is_unavailable(binding, &projection) {
        return Err(ownership_error(
            OWNERSHIP_USE_AFTER_MOVE,
            format!(
                "use of moved value {:?}",
                format_projected_name(name, &projection)
            ),
        ));
    }
    if projection.is_empty() {
        binding.moved = true;
    } else {
        binding.moved_projections.insert(projection);
    }
    Ok(())
}

fn ensure_lowered_projection_traversable(
    expr: &Expr,
    env: &HashMap<String, Binding>,
) -> Result<(), Diagnostic> {
    let Some((name, projection)) = ownership_projection(expr) else {
        return Ok(());
    };
    let Some(binding) = env.get(name) else {
        return Ok(());
    };
    if projection_has_moved_ancestor(binding, &projection) {
        return Err(ownership_error(
            OWNERSHIP_USE_AFTER_MOVE,
            format!(
                "use of moved value {:?}",
                format_projected_name(name, &projection)
            ),
        ));
    }
    Ok(())
}

fn ownership_projection(expr: &Expr) -> Option<(&str, ProjectionPath)> {
    match expr {
        Expr::VarRef { name, .. } => Some((name, Vec::new())),
        Expr::Try { expr, .. } | Expr::Await { expr, .. } => ownership_projection(expr),
        Expr::FieldAccess { base, field, .. } => {
            let (name, mut path) = ownership_projection(base)?;
            path.push(ProjectionSegment::Field(field.clone()));
            Some((name, path))
        }
        Expr::TupleIndex { base, index, .. } => {
            let (name, mut path) = ownership_projection(base)?;
            path.push(ProjectionSegment::TupleIndex(*index));
            Some((name, path))
        }
        Expr::Index { base, .. } => ownership_projection(base),
        _ => None,
    }
}

fn projection_is_unavailable(binding: &Binding, projection: &[ProjectionSegment]) -> bool {
    binding.moved
        || binding
            .moved_projections
            .iter()
            .any(|moved| projection_conflicts(moved, projection))
}

fn projection_has_moved_ancestor(binding: &Binding, projection: &[ProjectionSegment]) -> bool {
    binding.moved
        || binding
            .moved_projections
            .iter()
            .any(|moved| is_projection_prefix(moved, projection))
}

fn projection_conflicts(left: &[ProjectionSegment], right: &[ProjectionSegment]) -> bool {
    is_projection_prefix(left, right) || is_projection_prefix(right, left)
}

fn is_projection_prefix(prefix: &[ProjectionSegment], projection: &[ProjectionSegment]) -> bool {
    prefix.len() <= projection.len()
        && prefix
            .iter()
            .zip(projection.iter())
            .all(|(left, right)| left == right)
}

fn format_projected_name(name: &str, projection: &[ProjectionSegment]) -> String {
    let mut out = name.to_string();
    for segment in projection {
        match segment {
            ProjectionSegment::Field(field) => {
                out.push('.');
                out.push_str(field);
            }
            ProjectionSegment::TupleIndex(index) => {
                out.push('.');
                out.push_str(&index.to_string());
            }
        }
    }
    out
}

fn is_borrowable_slice_base(expr: &Expr) -> bool {
    match expr {
        Expr::VarRef { .. } => true,
        Expr::FieldAccess { base, .. } => is_borrowable_slice_base(base),
        Expr::TupleIndex { base, .. } => is_borrowable_slice_base(base),
        Expr::Slice { .. } => true,
        _ => false,
    }
}

fn binding_borrow_origin(
    ty: &Type,
    param_name: Option<&str>,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> Option<BorrowOrigin> {
    if !contains_borrowed_slice_type(ty, structs, enums) {
        return None;
    }
    Some(match param_name {
        Some(name) => BorrowOrigin::Param(name.to_string()),
        None => BorrowOrigin::Local,
    })
}

fn binding_borrow_origin_from_expr(
    ty: &Type,
    expr: &Expr,
    env: &HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Option<BorrowOrigin> {
    if !contains_borrowed_slice_type(ty, ctx.structs, ctx.enums) {
        return None;
    }
    expr_borrow_origin(expr, env, ctx)
}

fn binding_borrowed_owners_from_expr(
    ty: &Type,
    expr: &Expr,
    env: &HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> HashSet<String> {
    if !contains_borrowed_slice_type(ty, ctx.structs, ctx.enums) {
        return HashSet::new();
    }
    expr_borrowed_owners(expr, env, ctx)
}

fn expr_borrow_origin(
    expr: &Expr,
    env: &HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Option<BorrowOrigin> {
    if !contains_borrowed_slice_type(expr.ty(), ctx.structs, ctx.enums) {
        return None;
    }
    match expr {
        Expr::VarRef { name, .. } => env
            .get(name)
            .and_then(|binding| binding.borrow_origin.clone()),
        Expr::Slice { base, .. } => match base.ty() {
            Type::Slice(_) | Type::MutSlice(_) => expr_borrow_origin(base, env, ctx),
            Type::Array(_) => Some(BorrowOrigin::Local),
            _ => Some(BorrowOrigin::Local),
        },
        Expr::Call { name, args, .. } => ctx
            .functions
            .get(name)
            .map(|signature| {
                merge_borrow_origins(
                    signature
                        .borrow_return_params
                        .iter()
                        .map(|index| expr_borrow_origin(&args[*index], env, ctx)),
                )
            })
            .flatten(),
        Expr::TupleLiteral { elements, .. } => merge_borrow_origins(
            elements
                .iter()
                .map(|element| expr_borrow_origin(element, env, ctx)),
        ),
        Expr::Try { expr, .. } | Expr::Await { expr, .. } => expr_borrow_origin(expr, env, ctx),
        Expr::TupleIndex { base, .. } => expr_borrow_origin(base, env, ctx),
        Expr::MapLiteral { entries, .. } => {
            merge_borrow_origins(entries.iter().flat_map(|entry| {
                [
                    expr_borrow_origin(&entry.key, env, ctx),
                    expr_borrow_origin(&entry.value, env, ctx),
                ]
            }))
        }
        Expr::EnumVariant { payloads, .. } => merge_borrow_origins(
            payloads
                .iter()
                .map(|payload| expr_borrow_origin(payload, env, ctx)),
        ),
        Expr::FieldAccess { base, .. } => expr_borrow_origin(base, env, ctx),
        Expr::ArrayLiteral { elements, .. } => merge_borrow_origins(
            elements
                .iter()
                .map(|element| expr_borrow_origin(element, env, ctx)),
        ),
        Expr::StructLiteral { fields, .. } => merge_borrow_origins(
            fields
                .iter()
                .map(|field| expr_borrow_origin(&field.expr, env, ctx)),
        ),
        Expr::Index { base, .. } => expr_borrow_origin(base, env, ctx),
        Expr::Literal { .. } | Expr::BinaryAdd { .. } | Expr::BinaryCompare { .. } => None,
    }
}

fn merge_borrow_origins<I>(origins: I) -> Option<BorrowOrigin>
where
    I: IntoIterator<Item = Option<BorrowOrigin>>,
{
    let mut merged = None;
    for origin in origins.into_iter().flatten() {
        match &merged {
            None => merged = Some(origin),
            Some(existing) if existing == &origin => {}
            Some(_) => return Some(BorrowOrigin::Local),
        }
    }
    merged
}

fn match_binding_payload_expr<'a>(
    matched_expr: &'a Expr,
    variant_name: &str,
    binding_name: &str,
    binding_index: usize,
) -> Option<&'a Expr> {
    let Expr::EnumVariant {
        variant,
        field_names,
        payloads,
        ..
    } = matched_expr
    else {
        return None;
    };
    if variant != variant_name {
        return None;
    }
    if field_names.is_empty() {
        return payloads.get(binding_index);
    }
    field_names
        .iter()
        .position(|field_name| field_name == binding_name)
        .and_then(|index| payloads.get(index))
}

fn match_binding_borrow_origin(
    matched_expr: &Expr,
    variant_name: &str,
    binding_name: &str,
    binding_index: usize,
    payload_ty: &Type,
    env: &HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Option<BorrowOrigin> {
    if !contains_borrowed_slice_type(payload_ty, ctx.structs, ctx.enums) {
        return None;
    }
    if let Some(payload_expr) =
        match_binding_payload_expr(matched_expr, variant_name, binding_name, binding_index)
    {
        return expr_borrow_origin(payload_expr, env, ctx);
    }
    expr_borrow_origin(matched_expr, env, ctx)
}

fn match_binding_borrowed_owners(
    matched_expr: &Expr,
    variant_name: &str,
    binding_name: &str,
    binding_index: usize,
    payload_ty: &Type,
    env: &HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> HashSet<String> {
    if !contains_borrowed_slice_type(payload_ty, ctx.structs, ctx.enums) {
        return HashSet::new();
    }
    if let Some(payload_expr) =
        match_binding_payload_expr(matched_expr, variant_name, binding_name, binding_index)
    {
        return expr_borrowed_owners(payload_expr, env, ctx);
    }
    expr_borrowed_owners(matched_expr, env, ctx)
}

fn expr_borrowed_owners(
    expr: &Expr,
    env: &HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> HashSet<String> {
    if !contains_borrowed_slice_type(expr.ty(), ctx.structs, ctx.enums) {
        return HashSet::new();
    }
    match expr {
        Expr::VarRef { name, .. } => env
            .get(name)
            .map(|binding| binding.borrowed_owners.clone())
            .unwrap_or_default(),
        Expr::Slice { base, .. } => match base.ty() {
            Type::Slice(_) | Type::MutSlice(_) => expr_borrowed_owners(base, env, ctx),
            Type::Array(_) => owned_borrow_root(base).into_iter().collect(),
            _ => HashSet::new(),
        },
        Expr::Call { name, args, .. } => ctx
            .functions
            .get(name)
            .map(|signature| {
                let mut owners = HashSet::new();
                for index in &signature.borrow_return_params {
                    owners.extend(expr_borrowed_owners(&args[*index], env, ctx));
                }
                owners
            })
            .unwrap_or_default(),
        Expr::TupleLiteral { elements, .. } => collect_expr_borrowed_owners(elements, env, ctx),
        Expr::Try { expr, .. } | Expr::Await { expr, .. } => expr_borrowed_owners(expr, env, ctx),
        Expr::TupleIndex { base, .. } => expr_borrowed_owners(base, env, ctx),
        Expr::MapLiteral { entries, .. } => {
            let mut owners = HashSet::new();
            for entry in entries {
                owners.extend(expr_borrowed_owners(&entry.key, env, ctx));
                owners.extend(expr_borrowed_owners(&entry.value, env, ctx));
            }
            owners
        }
        Expr::ArrayLiteral { elements, .. } => collect_expr_borrowed_owners(elements, env, ctx),
        Expr::EnumVariant { payloads, .. } => collect_expr_borrowed_owners(payloads, env, ctx),
        Expr::FieldAccess { base, .. } => expr_borrowed_owners(base, env, ctx),
        Expr::Index { base, .. } => expr_borrowed_owners(base, env, ctx),
        Expr::Literal { .. } | Expr::BinaryAdd { .. } | Expr::BinaryCompare { .. } => {
            HashSet::new()
        }
        Expr::StructLiteral { fields, .. } => {
            let mut owners = HashSet::new();
            for field in fields {
                owners.extend(expr_borrowed_owners(&field.expr, env, ctx));
            }
            owners
        }
    }
}

fn collect_expr_borrowed_owners(
    exprs: &[Expr],
    env: &HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> HashSet<String> {
    let mut owners = HashSet::new();
    for expr in exprs {
        owners.extend(expr_borrowed_owners(expr, env, ctx));
    }
    owners
}

fn owned_borrow_root(expr: &Expr) -> Option<String> {
    match expr {
        Expr::VarRef { name, ty } if !matches!(ty, Type::Slice(_) | Type::MutSlice(_)) => {
            Some(name.clone())
        }
        Expr::FieldAccess { base, .. } => owned_borrow_root(base),
        Expr::TupleIndex { base, .. } => owned_borrow_root(base),
        _ => None,
    }
}

fn contains_borrowed_slice_type(
    ty: &Type,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> bool {
    contains_borrowed_slice_type_inner(ty, structs, enums, &mut HashSet::new(), &mut HashSet::new())
}

fn contains_mut_borrowed_slice_type(
    ty: &Type,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> bool {
    contains_mut_borrowed_slice_type_inner(
        ty,
        structs,
        enums,
        &mut HashSet::new(),
        &mut HashSet::new(),
    )
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
        Type::Error
        | Type::Slice(_)
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

fn classify_borrow_return(
    params: &[Type],
    return_ty: &Type,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    line: usize,
    column: usize,
) -> Result<Vec<usize>, Diagnostic> {
    if !contains_borrowed_slice_type(return_ty, structs, enums) {
        return Ok(Vec::new());
    }
    let matches = params
        .iter()
        .enumerate()
        .filter_map(|(index, ty)| contains_borrowed_slice_type(ty, structs, enums).then_some(index))
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

fn borrow_kind_for_type(
    ty: &Type,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> Option<BorrowKind> {
    if contains_mut_borrowed_slice_type(ty, structs, enums) {
        Some(BorrowKind::Mutable)
    } else if contains_borrowed_slice_type(ty, structs, enums) {
        Some(BorrowKind::Shared)
    } else {
        None
    }
}

fn increment_active_borrows(
    owner_names: &HashSet<String>,
    env: &mut HashMap<String, Binding>,
    borrow_kind: BorrowKind,
    line: usize,
    column: usize,
) -> Result<(), Diagnostic> {
    for owner_name in owner_names {
        let binding = env.get_mut(owner_name).ok_or_else(|| {
            Diagnostic::new(
                "type",
                format!("internal error: missing borrow owner {owner_name:?}"),
            )
        })?;
        match borrow_kind {
            BorrowKind::Shared if binding.active_mut_borrow_count > 0 => {
                return Err(ownership_error(
                    OWNERSHIP_SHARED_BORROW_WHILE_MUTABLE_LIVE,
                    format!(
                        "cannot create shared borrow of value {owner_name:?} while a mutable borrow is still live"
                    ),
                )
                .with_span(line, column));
            }
            BorrowKind::Mutable if binding.active_mut_borrow_count > 0 => {
                return Err(ownership_error(
                    OWNERSHIP_MUTABLE_BORROW_WHILE_MUTABLE_LIVE,
                    format!(
                        "cannot create mutable borrow of value {owner_name:?} while another mutable borrow is still live"
                    ),
                )
                .with_span(line, column));
            }
            BorrowKind::Mutable if binding.active_borrow_count > 0 => {
                return Err(ownership_error(
                    OWNERSHIP_MUTABLE_BORROW_WHILE_SHARED_LIVE,
                    format!(
                        "cannot create mutable borrow of value {owner_name:?} while a shared borrow is still live"
                    ),
                )
                .with_span(line, column));
            }
            _ => {}
        }
        binding.active_borrow_count += 1;
        if matches!(borrow_kind, BorrowKind::Mutable) {
            binding.active_mut_borrow_count += 1;
        }
    }
    Ok(())
}

fn record_temporary_borrows(
    expr: &Expr,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
    temporary_borrows: &mut Vec<(HashSet<String>, BorrowKind)>,
) -> Result<(), Diagnostic> {
    let owners = expr_borrowed_owners(expr, env, ctx);
    let Some(borrow_kind) = borrow_kind_for_type(expr.ty(), ctx.structs, ctx.enums) else {
        return Ok(());
    };
    increment_active_borrows(&owners, env, borrow_kind, 0, 0)?;
    temporary_borrows.push((owners, borrow_kind));
    Ok(())
}

fn release_temporary_borrows(
    temporary_borrows: &[(HashSet<String>, BorrowKind)],
    env: &mut HashMap<String, Binding>,
) {
    for (owner_names, borrow_kind) in temporary_borrows.iter().rev() {
        release_active_borrow_owners(owner_names, env, *borrow_kind);
    }
}

fn release_active_borrow_owners(
    owner_names: &HashSet<String>,
    env: &mut HashMap<String, Binding>,
    borrow_kind: BorrowKind,
) {
    for owner_name in owner_names {
        decrement_active_borrow(owner_name, env, borrow_kind);
    }
}

fn decrement_active_borrow(
    owner_name: &str,
    env: &mut HashMap<String, Binding>,
    borrow_kind: BorrowKind,
) {
    let Some(binding) = env.get_mut(owner_name) else {
        return;
    };
    binding.active_borrow_count = binding.active_borrow_count.saturating_sub(1);
    if matches!(borrow_kind, BorrowKind::Mutable) {
        binding.active_mut_borrow_count = binding.active_mut_borrow_count.saturating_sub(1);
    }
}

fn release_scope_borrows(env: &mut HashMap<String, Binding>, scope_names: &HashSet<String>) {
    let released = env
        .keys()
        .filter(|name| !scope_names.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    for name in &released {
        let (owner_names, borrow_kind) = env
            .get(name)
            .map(|binding| {
                (
                    binding.borrowed_owners.iter().cloned().collect::<Vec<_>>(),
                    binding.borrow_kind,
                )
            })
            .unwrap_or_default();
        let Some(borrow_kind) = borrow_kind else {
            continue;
        };
        for owner_name in owner_names {
            decrement_active_borrow(&owner_name, env, borrow_kind);
        }
    }
    for name in released {
        env.remove(&name);
    }
}

fn merge_borrow_count(
    before: usize,
    then_returns: bool,
    then_after: Option<usize>,
    else_returns: bool,
    else_after: Option<usize>,
) -> usize {
    let then_count = if then_returns {
        before
    } else {
        then_after.unwrap_or(before)
    };
    let else_count = if else_returns {
        before
    } else {
        else_after.unwrap_or(before)
    };
    then_count.max(else_count)
}

fn lower_literal(literal: &syntax::Literal) -> Expr {
    match literal {
        syntax::Literal::Int(value) => Expr::Literal {
            ty: Type::Int,
            value: LiteralValue::Int(*value),
        },
        syntax::Literal::Bool(value) => Expr::Literal {
            ty: Type::Bool,
            value: LiteralValue::Bool(*value),
        },
        syntax::Literal::String(value) => Expr::Literal {
            ty: Type::String,
            value: LiteralValue::String(value.clone()),
        },
    }
}

fn lower_type<T, U>(
    ty: &syntax::TypeName,
    structs: &HashMap<String, T>,
    enums: &HashMap<String, U>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
    line: usize,
    column: usize,
) -> Result<Type, Diagnostic> {
    let mut resolving = HashSet::new();
    lower_type_inner(ty, structs, enums, aliases, &mut resolving, line, column)
}

fn lower_type_inner<T, U>(
    ty: &syntax::TypeName,
    structs: &HashMap<String, T>,
    enums: &HashMap<String, U>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
    resolving: &mut HashSet<String>,
    line: usize,
    column: usize,
) -> Result<Type, Diagnostic> {
    match ty {
        syntax::TypeName::Int => Ok(Type::Int),
        syntax::TypeName::Bool => Ok(Type::Bool),
        syntax::TypeName::String => Ok(Type::String),
        syntax::TypeName::Named(name, args) => {
            if is_async_runtime_type(name) {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("async runtime type {name:?} expects 1 type argument"),
                    )
                    .with_span(line, column));
                }
                let inner = Box::new(lower_type_inner(
                    &args[0], structs, enums, aliases, resolving, line, column,
                )?);
                return Ok(match name.as_str() {
                    "Task" => Type::Task(inner),
                    "JoinHandle" => Type::JoinHandle(inner),
                    "AsyncChannel" => Type::AsyncChannel(inner),
                    "SelectResult" => Type::SelectResult(inner),
                    _ => unreachable!("async runtime type checked above"),
                });
            }
            if !args.is_empty() {
                return Err(Diagnostic::new(
                    "type",
                    format!("generic type {name:?} was not monomorphized"),
                )
                .with_span(line, column));
            }
            if structs.contains_key(name) {
                return Ok(Type::Struct(name.clone()));
            }
            if enums.contains_key(name) {
                return Ok(Type::Enum(name.clone()));
            }
            if let Some(type_alias) = aliases.get(name) {
                if !resolving.insert(name.clone()) {
                    return Err(Diagnostic::new(
                        "type",
                        format!("type alias {name:?} is recursive"),
                    )
                    .with_span(type_alias.line, type_alias.column));
                }
                let lowered = lower_type_inner(
                    &type_alias.ty,
                    structs,
                    enums,
                    aliases,
                    resolving,
                    type_alias.line,
                    type_alias.column,
                );
                resolving.remove(name);
                return lowered;
            }
            Err(Diagnostic::new("type", format!("unknown type {name:?}")).with_span(line, column))
        }
        syntax::TypeName::Ptr(inner) => Ok(Type::Ptr(Box::new(lower_type_inner(
            inner, structs, enums, aliases, resolving, line, column,
        )?))),
        syntax::TypeName::MutPtr(inner) => Ok(Type::MutPtr(Box::new(lower_type_inner(
            inner, structs, enums, aliases, resolving, line, column,
        )?))),
        syntax::TypeName::Slice(inner) => Ok(Type::Slice(Box::new(lower_type_inner(
            inner, structs, enums, aliases, resolving, line, column,
        )?))),
        syntax::TypeName::MutSlice(inner) => Ok(Type::MutSlice(Box::new(lower_type_inner(
            inner, structs, enums, aliases, resolving, line, column,
        )?))),
        syntax::TypeName::Option(inner) => Ok(Type::Option(Box::new(lower_type_inner(
            inner, structs, enums, aliases, resolving, line, column,
        )?))),
        syntax::TypeName::Result(ok, err) => Ok(Type::Result(
            Box::new(lower_type_inner(
                ok, structs, enums, aliases, resolving, line, column,
            )?),
            Box::new(lower_type_inner(
                err, structs, enums, aliases, resolving, line, column,
            )?),
        )),
        syntax::TypeName::Tuple(elements) => Ok(Type::Tuple(
            elements
                .iter()
                .map(|element| {
                    lower_type_inner(element, structs, enums, aliases, resolving, line, column)
                })
                .collect::<Result<Vec<_>, _>>()?,
        )),
        syntax::TypeName::Map(key, value) => {
            let key = lower_type_inner(key, structs, enums, aliases, resolving, line, column)?;
            if !key.supports_map_key() {
                return Err(Diagnostic::new(
                    "type",
                    format!("map key type {key} is not supported"),
                )
                .with_span(line, column));
            }
            Ok(Type::Map(
                Box::new(key),
                Box::new(lower_type_inner(
                    value, structs, enums, aliases, resolving, line, column,
                )?),
            ))
        }
        syntax::TypeName::Array(inner) => Ok(Type::Array(Box::new(lower_type_inner(
            inner, structs, enums, aliases, resolving, line, column,
        )?))),
    }
}

fn lower_compare_op(op: syntax::CompareOp) -> CompareOp {
    match op {
        syntax::CompareOp::Eq => CompareOp::Eq,
        syntax::CompareOp::Ne => CompareOp::Ne,
        syntax::CompareOp::Lt => CompareOp::Lt,
        syntax::CompareOp::Le => CompareOp::Le,
        syntax::CompareOp::Gt => CompareOp::Gt,
        syntax::CompareOp::Ge => CompareOp::Ge,
    }
}

fn with_assert_location(args: Vec<Expr>, line: usize, column: usize) -> Vec<Expr> {
    let mut args = args;
    args.push(Expr::Literal {
        ty: Type::Int,
        value: LiteralValue::Int(line as i64),
    });
    args.push(Expr::Literal {
        ty: Type::Int,
        value: LiteralValue::Int(column as i64),
    });
    args
}

fn static_bool_value(expr: &Expr) -> Option<bool> {
    match expr {
        Expr::Literal {
            value: LiteralValue::Bool(value),
            ..
        } => Some(*value),
        Expr::BinaryCompare { op, lhs, rhs, .. } => {
            let lhs = literal_value(lhs)?;
            let rhs = literal_value(rhs)?;
            Some(match (lhs, rhs) {
                (LiteralValue::Int(lhs), LiteralValue::Int(rhs)) => match op {
                    CompareOp::Eq => lhs == rhs,
                    CompareOp::Ne => lhs != rhs,
                    CompareOp::Lt => lhs < rhs,
                    CompareOp::Le => lhs <= rhs,
                    CompareOp::Gt => lhs > rhs,
                    CompareOp::Ge => lhs >= rhs,
                },
                (LiteralValue::Bool(lhs), LiteralValue::Bool(rhs)) => match op {
                    CompareOp::Eq => lhs == rhs,
                    CompareOp::Ne => lhs != rhs,
                    _ => return None,
                },
                (LiteralValue::String(lhs), LiteralValue::String(rhs)) => match op {
                    CompareOp::Eq => lhs == rhs,
                    CompareOp::Ne => lhs != rhs,
                    _ => return None,
                },
                _ => return None,
            })
        }
        _ => None,
    }
}

fn literal_value(expr: &Expr) -> Option<&LiteralValue> {
    match expr {
        Expr::Literal { value, .. } => Some(value),
        _ => None,
    }
}

impl Expr {
    pub fn ty(&self) -> &Type {
        match self {
            Expr::Literal { ty, .. } => ty,
            Expr::VarRef { ty, .. } => ty,
            Expr::Call { ty, .. } => ty,
            Expr::BinaryAdd { ty, .. } => ty,
            Expr::BinaryCompare { ty, .. } => ty,
            Expr::Try { ty, .. } => ty,
            Expr::Await { ty, .. } => ty,
            Expr::StructLiteral { ty, .. } => ty,
            Expr::FieldAccess { ty, .. } => ty,
            Expr::TupleLiteral { ty, .. } => ty,
            Expr::TupleIndex { ty, .. } => ty,
            Expr::MapLiteral { ty, .. } => ty,
            Expr::EnumVariant { ty, .. } => ty,
            Expr::ArrayLiteral { ty, .. } => ty,
            Expr::Slice { ty, .. } => ty,
            Expr::Index { ty, .. } => ty,
        }
    }
}

impl Stmt {
    fn always_returns(&self) -> bool {
        match self {
            Stmt::Return { .. } | Stmt::Panic { .. } => true,
            Stmt::Defer { .. } => false,
            Stmt::If {
                cond,
                then_block,
                else_block: Some(else_block),
                ..
            } => match static_bool_value(cond) {
                Some(true) => block_always_returns(then_block),
                Some(false) => block_always_returns(else_block),
                None => block_always_returns(then_block) && block_always_returns(else_block),
            },
            Stmt::If {
                cond,
                then_block,
                else_block: None,
                ..
            } => {
                static_bool_value(cond).is_some_and(|value| value)
                    && block_always_returns(then_block)
            }
            Stmt::Match { arms, .. } => arms.iter().all(|arm| block_always_returns(&arm.body)),
            _ => false,
        }
    }
}

fn block_always_returns(block: &[Stmt]) -> bool {
    block.last().is_some_and(Stmt::always_returns)
}

impl syntax::Stmt {
    fn line(&self) -> usize {
        match self {
            syntax::Stmt::Let { line, .. }
            | syntax::Stmt::Print { line, .. }
            | syntax::Stmt::Panic { line, .. }
            | syntax::Stmt::Defer { line, .. }
            | syntax::Stmt::If { line, .. }
            | syntax::Stmt::While { line, .. }
            | syntax::Stmt::Match { line, .. }
            | syntax::Stmt::Return { line, .. } => *line,
        }
    }

    fn column(&self) -> usize {
        match self {
            syntax::Stmt::Let { column, .. }
            | syntax::Stmt::Print { column, .. }
            | syntax::Stmt::Panic { column, .. }
            | syntax::Stmt::Defer { column, .. }
            | syntax::Stmt::If { column, .. }
            | syntax::Stmt::While { column, .. }
            | syntax::Stmt::Match { column, .. }
            | syntax::Stmt::Return { column, .. } => *column,
        }
    }
}

impl syntax::Expr {
    fn line(&self) -> usize {
        match self {
            syntax::Expr::Literal(_) => 1,
            syntax::Expr::VarRef { line, .. }
            | syntax::Expr::Call { line, .. }
            | syntax::Expr::MethodCall { line, .. }
            | syntax::Expr::BinaryAdd { line, .. }
            | syntax::Expr::BinaryCompare { line, .. }
            | syntax::Expr::Try { line, .. }
            | syntax::Expr::Await { line, .. }
            | syntax::Expr::StructLiteral { line, .. }
            | syntax::Expr::FieldAccess { line, .. }
            | syntax::Expr::TupleLiteral { line, .. }
            | syntax::Expr::TupleIndex { line, .. }
            | syntax::Expr::MapLiteral { line, .. }
            | syntax::Expr::ArrayLiteral { line, .. }
            | syntax::Expr::Slice { line, .. }
            | syntax::Expr::Index { line, .. } => *line,
        }
    }

    fn column(&self) -> usize {
        match self {
            syntax::Expr::Literal(_) => 1,
            syntax::Expr::VarRef { column, .. }
            | syntax::Expr::Call { column, .. }
            | syntax::Expr::MethodCall { column, .. }
            | syntax::Expr::BinaryAdd { column, .. }
            | syntax::Expr::BinaryCompare { column, .. }
            | syntax::Expr::Try { column, .. }
            | syntax::Expr::Await { column, .. }
            | syntax::Expr::StructLiteral { column, .. }
            | syntax::Expr::FieldAccess { column, .. }
            | syntax::Expr::TupleLiteral { column, .. }
            | syntax::Expr::TupleIndex { column, .. }
            | syntax::Expr::MapLiteral { column, .. }
            | syntax::Expr::ArrayLiteral { column, .. }
            | syntax::Expr::Slice { column, .. }
            | syntax::Expr::Index { column, .. } => *column,
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

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Error => write!(f, "<type-error>"),
            Type::Int => write!(f, "int"),
            Type::Bool => write!(f, "bool"),
            Type::String => write!(f, "string"),
            Type::Struct(name) => write!(f, "{name}"),
            Type::Enum(name) => write!(f, "{name}"),
            Type::Ptr(inner) => write!(f, "ptr<{inner}>"),
            Type::MutPtr(inner) => write!(f, "mutptr<{inner}>"),
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
            Type::Array(inner) => write!(f, "[{inner}]"),
            Type::Task(inner) => write!(f, "Task<{inner}>"),
            Type::JoinHandle(inner) => write!(f, "JoinHandle<{inner}>"),
            Type::AsyncChannel(inner) => write!(f, "AsyncChannel<{inner}>"),
            Type::SelectResult(inner) => write!(f, "SelectResult<{inner}>"),
        }
    }
}
