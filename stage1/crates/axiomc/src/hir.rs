use crate::borrowck;
use crate::diagnostics::{Diagnostic, message_with_suggestion};
use crate::manifest::{CapabilityConfig, CapabilityKind};
use crate::syntax;
use std::collections::{HashMap, HashSet};

mod capabilities;
mod control_flow;
mod definitions;
mod diagnostics;
mod expressions;
mod generics;
mod model;
mod ownership;
mod properties;
mod reachability;
mod signatures;
mod source_locations;
mod symbols;
mod types;

use self::capabilities::{
    is_stdlib_http_get_wrapper, is_stdlib_process_wrapper, net_binding_origin_from_expr,
    require_capability, stdlib_dynamic_http_socket_allowed, stdlib_dynamic_net_host_allowed,
    stdlib_dynamic_net_peer_host_allowed, stdlib_dynamic_net_peer_port_allowed,
    stdlib_dynamic_net_socket_allowed, validate_ffi_signature, validate_http_get_net_allowlist_hir,
    validate_net_host_allowlist_hir, validate_net_port_allowlist_hir,
    validate_net_socket_allowlist_hir, validate_process_command_allowlist_hir,
    validate_stdlib_network_wrapper_call_hir,
};
use self::definitions::{
    VariantInfo, collect_enum_definitions, collect_struct_definitions, collect_trait_definitions,
    collect_type_names, validate_recursive_type_cycles, validate_trait_bounds_in_program,
    validate_trait_type_uses_in_program,
};
use self::diagnostics::{
    append_diagnostic, primary_diagnostic, single_diagnostic, sort_diagnostics,
};
use self::expressions::{
    coerce_lowered_expr_to_expected, is_castable_numeric, is_ordered_numeric, is_string_like_type,
    lower_binary_add_chain, lower_call_arg_with_expected, method_owner_name,
    numeric_method_return_ty, static_bool_value,
};
use self::generics::monomorphize_program;
use self::model::type_assignable_to;
pub use self::model::*;
use self::ownership::{
    BorrowKind, BorrowOrigin, BorrowedOwner, ProjectionPath, binding_borrow_origin,
    binding_borrow_origin_from_expr, binding_borrowed_owners_from_expr, borrow_kind_for_type,
    borrow_region_facts_for_binding, borrow_region_facts_for_enum_payload_binding,
    borrow_region_facts_for_return_expr, contains_borrowed_slice_type,
    ensure_lowered_projection_traversable, expr_borrow_origin, expr_borrowed_owners,
    increment_active_borrows, is_borrowable_slice_base, is_supported_helper_call_array_index_base,
    match_binding_borrow_origin, match_binding_borrowed_owners, merge_borrow_count,
    move_lowered_owner_value, move_lowered_value, ownership_projection, record_temporary_borrows,
    release_active_borrow_owners, release_scope_borrows, release_temporary_borrows,
};
use self::properties::{validate_property_signature, validate_property_verdict};
use self::reachability::reachable_function_names;
use self::signatures::{
    FunctionSig, MethodSig, collect_function_signatures, collect_method_signatures,
    function_symbol_name, validate_trait_impls,
};
use self::symbols::{
    is_async_runtime_intrinsic, is_async_runtime_type, monomorphized_function_name,
    monomorphized_type_name, preserves_intrinsic_type_args,
};
use self::types::{lower_compare_op, lower_literal, lower_logic_op, lower_type};

#[derive(Debug, Clone)]
struct MatchArmInput {
    variant: String,
    bindings: Vec<String>,
    is_named: bool,
    ignore_payloads: bool,
    body: Vec<syntax::Stmt>,
    line: usize,
    column: usize,
}

#[derive(Debug, Clone)]
struct MatchExprArmInput {
    variant: String,
    bindings: Vec<String>,
    is_named: bool,
    expr: syntax::Expr,
    line: usize,
    column: usize,
}

impl From<&syntax::MatchArm> for MatchArmInput {
    fn from(arm: &syntax::MatchArm) -> Self {
        Self {
            variant: arm.variant.clone(),
            bindings: arm.bindings.clone(),
            is_named: arm.is_named,
            ignore_payloads: false,
            body: arm.body.clone(),
            line: arm.line,
            column: arm.column,
        }
    }
}

impl From<&syntax::MatchExprArm> for MatchExprArmInput {
    fn from(arm: &syntax::MatchExprArm) -> Self {
        Self {
            variant: arm.variant.clone(),
            bindings: arm.bindings.clone(),
            is_named: arm.is_named,
            expr: arm.expr.clone(),
            line: arm.line,
            column: arm.column,
        }
    }
}

#[derive(Debug, Clone)]
struct Binding {
    ty: Type,
    moved: bool,
    moved_projections: HashSet<ProjectionPath>,
    borrow_kind: Option<BorrowKind>,
    borrow_origin: Option<BorrowOrigin>,
    net_origin: Option<NetBindingOrigin>,
    borrowed_owners: HashSet<BorrowedOwner>,
    active_borrow_count: usize,
    active_mut_borrow_count: usize,
    active_borrows: HashMap<ProjectionPath, borrowck::BorrowState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NetBindingOrigin {
    LoopbackTcpListener,
    LoopbackTcpListenerPort,
}

struct LowerContext<'a> {
    current_path: &'a str,
    consts: &'a HashMap<String, syntax::ConstDecl>,
    structs: &'a HashMap<String, StructDef>,
    enums: &'a HashMap<String, EnumDef>,
    aliases: &'a HashMap<String, syntax::TypeAliasDecl>,
    variants: &'a HashMap<String, Vec<VariantInfo>>,
    functions: &'a HashMap<String, FunctionSig>,
    methods: &'a HashMap<String, HashMap<String, MethodSig>>,
    capabilities: &'a CapabilityConfig,
    current_return: Option<Type>,
    current_function: Option<String>,
    current_property: bool,
    current_borrow_return_params: HashSet<String>,
}

const OWNERSHIP_CLOSURE_MOVE_CAPTURED_NON_COPY: &str = "closure_move_captured_non_copy";
const OWNERSHIP_CLOSURE_BORROWED_SLICE_RETURN: &str = "closure_borrowed_slice_return";
const OWNERSHIP_LOOP_MOVE_OUTER_NON_COPY: &str = borrowck::LOOP_MOVE_OUTER_NON_COPY;
const OWNERSHIP_BORROW_RETURN_REQUIRES_PARAM_ORIGIN: &str =
    borrowck::BORROW_RETURN_REQUIRES_PARAM_ORIGIN;
const OWNERSHIP_MOVE_WHILE_BORROWED: &str = borrowck::MOVE_WHILE_BORROWED;
const OWNERSHIP_USE_AFTER_MOVE: &str = borrowck::USE_AFTER_MOVE;

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
    let consts = program
        .consts
        .iter()
        .map(|constant| (constant.name.clone(), constant.clone()))
        .collect::<HashMap<_, _>>();
    let syntax_functions = program
        .functions
        .iter()
        .map(|function| (function.name.clone(), function.clone()))
        .collect::<HashMap<_, _>>();
    let consts = resolve_const_int_decls(&consts, &syntax_functions).map_err(single_diagnostic)?;
    validate_const_array_lengths_in_program(&program, &consts).map_err(single_diagnostic)?;
    let (struct_names, enum_names, aliases) =
        collect_type_names(&program.structs, &program.enums, &program.type_aliases)
            .map_err(single_diagnostic)?;
    let traits = collect_trait_definitions(&program.traits, &struct_names, &enum_names, &aliases)
        .map_err(single_diagnostic)?;
    validate_trait_bounds_in_program(&program, &traits).map_err(single_diagnostic)?;
    validate_trait_type_uses_in_program(&program, &traits).map_err(single_diagnostic)?;
    let (enums, variants) = collect_enum_definitions(
        &program.enums,
        &struct_names,
        &enum_names,
        &aliases,
        &consts,
    )
    .map_err(single_diagnostic)?;
    let structs = collect_struct_definitions(&program.structs, &enum_names, &aliases, &consts)
        .map_err(single_diagnostic)?;
    validate_recursive_type_cycles(
        &program,
        &structs,
        &enums,
        &struct_names,
        &enum_names,
        &aliases,
        &consts,
    )
    .map_err(single_diagnostic)?;
    let functions =
        collect_function_signatures(&program.functions, &structs, &enums, &aliases, &consts)
            .map_err(single_diagnostic)?;
    let methods =
        collect_method_signatures(&program.functions, &structs, &enums, &aliases, &consts)
            .map_err(single_diagnostic)?;
    validate_trait_impls(
        &program.functions,
        &traits,
        &structs,
        &enums,
        &aliases,
        &consts,
        &methods,
    )
    .map_err(single_diagnostic)?;
    let mut diagnostics = Vec::new();
    let mut lowered_structs = structs.values().cloned().collect::<Vec<_>>();
    lowered_structs.sort_by(|lhs, rhs| lhs.name.cmp(&rhs.name));
    let mut lowered_enums = enums.values().cloned().collect::<Vec<_>>();
    lowered_enums.sort_by(|lhs, rhs| lhs.name.cmp(&rhs.name));
    let mut lowered_traits = traits.values().cloned().collect::<Vec<_>>();
    lowered_traits.sort_by(|lhs, rhs| lhs.name.cmp(&rhs.name));
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
            &consts,
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
        current_path: &program.path,
        consts: &consts,
        structs: &structs,
        enums: &enums,
        aliases: &aliases,
        variants: &variants,
        functions: &functions,
        methods: &methods,
        capabilities,
        current_return: None,
        current_function: None,
        current_property: false,
        current_borrow_return_params: HashSet::new(),
    };
    let statics = match lower_static_decls(&program.consts, &structs, &enums, &aliases, &ctx) {
        Ok(statics) => statics,
        Err(error) if recover => {
            append_diagnostic(&mut diagnostics, error);
            Vec::new()
        }
        Err(error) => return Err(single_diagnostic(error)),
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
        traits: lowered_traits,
        statics,
        functions: lowered_functions,
        stmts,
    })
}

fn ownership_error(code: &'static str, message: impl Into<String>) -> Diagnostic {
    borrowck::ownership_error(code, message)
}

impl Type {
    fn is_error(&self) -> bool {
        matches!(self, Type::Error)
    }

    pub fn is_copy(&self) -> bool {
        match self {
            Type::Error
            | Type::Never
            | Type::Int
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

    fn supports_map_key(&self) -> bool {
        match self {
            Type::Int | Type::Numeric(_) | Type::Bool | Type::String | Type::Str => true,
            Type::Tuple(elements) => elements.iter().all(Type::supports_map_key),
            Type::Error
            | Type::Never
            | Type::Struct(_)
            | Type::Enum(_)
            | Type::Ptr(_)
            | Type::MutPtr(_)
            | Type::MutRef(_)
            | Type::Slice(_)
            | Type::MutSlice(_)
            | Type::Option(_)
            | Type::Result(_, _)
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

fn lower_static_decls(
    consts: &[syntax::ConstDecl],
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
    ctx: &LowerContext<'_>,
) -> Result<Vec<StaticDef>, Diagnostic> {
    let mut lowered = Vec::new();
    for decl in consts.iter().filter(|decl| decl.is_static) {
        let ty = lower_type(
            &decl.ty,
            structs,
            enums,
            aliases,
            ctx.consts,
            decl.line,
            decl.column,
        )?;
        let mut env = HashMap::new();
        let mut expr = lower_expr_with_expected(&decl.expr, Some(&ty), &mut env, ctx)?;
        if expr.ty() != &ty {
            return Err(Diagnostic::new(
                "type",
                format!("static {:?} expects {}, got {}", decl.name, ty, expr.ty()),
            )
            .with_span(decl.line, decl.column));
        }
        if matches!(ty, Type::Bool) {
            if let Some(value) = static_bool_value(&expr) {
                expr = Expr::Literal {
                    ty: Type::Bool,
                    value: LiteralValue::Bool(value),
                };
            }
        }
        if matches!(ty, Type::String)
            && !matches!(
                expr,
                Expr::Literal {
                    value: LiteralValue::String(_),
                    ..
                }
            )
        {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "static {:?} string initializers must be string literals in stage1",
                    decl.name
                ),
            )
            .with_span(decl.line, decl.column));
        }
        lowered.push(StaticDef {
            name: decl.name.clone(),
            ty,
            expr,
        });
    }
    Ok(lowered)
}

fn validate_const_function_body(
    function: &syntax::Function,
    functions: &HashMap<String, FunctionSig>,
) -> Result<(), Diagnostic> {
    if !function.is_const {
        return Ok(());
    }
    if function.is_async {
        return Err(Diagnostic::new(
            "type",
            format!("const fn {:?} cannot be async", function.name),
        )
        .with_span(function.line, function.column));
    }
    if function.is_extern {
        return Err(Diagnostic::new(
            "type",
            format!("const fn {:?} cannot be extern", function.name),
        )
        .with_span(function.line, function.column));
    }
    for stmt in &function.body {
        validate_const_function_stmt(function, stmt, functions)?;
    }
    Ok(())
}

fn validate_const_function_stmt(
    function: &syntax::Function,
    stmt: &syntax::Stmt,
    functions: &HashMap<String, FunctionSig>,
) -> Result<(), Diagnostic> {
    match stmt {
        syntax::Stmt::Let { expr, .. } | syntax::Stmt::Return { expr, .. } => {
            validate_const_function_expr(function, expr, functions)
        }
        syntax::Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            validate_const_function_expr(function, cond, functions)?;
            for stmt in then_block {
                validate_const_function_stmt(function, stmt, functions)?;
            }
            if let Some(else_block) = else_block {
                for stmt in else_block {
                    validate_const_function_stmt(function, stmt, functions)?;
                }
            }
            Ok(())
        }
        _ => Err(Diagnostic::new(
            "type",
            format!(
                "const fn {:?} only supports let, if/else, and return statements in stage1",
                function.name
            ),
        )
        .with_span(stmt.line(), stmt.column())),
    }
}

fn validate_const_function_expr(
    function: &syntax::Function,
    expr: &syntax::Expr,
    functions: &HashMap<String, FunctionSig>,
) -> Result<(), Diagnostic> {
    match expr {
        syntax::Expr::Literal(_) | syntax::Expr::VarRef { .. } => Ok(()),
        syntax::Expr::BinaryAdd { lhs, rhs, .. } | syntax::Expr::BinaryCompare { lhs, rhs, .. } => {
            validate_const_function_expr(function, lhs, functions)?;
            validate_const_function_expr(function, rhs, functions)
        }
        syntax::Expr::Call { name, args, .. } => {
            let Some(signature) = functions.get(name) else {
                return Err(const_function_call_error(function, name, expr));
            };
            if !signature.is_const || signature.is_extern {
                return Err(const_function_call_error(function, name, expr));
            }
            for arg in args {
                validate_const_function_expr(function, arg, functions)?;
            }
            Ok(())
        }
        _ => Err(Diagnostic::new(
            "type",
            format!(
                "const fn {:?} only supports literals, variables, arithmetic/comparison expressions, and calls to other const fn in stage1",
                function.name
            ),
        )
        .with_span(expr.line(), expr.column())),
    }
}

fn const_function_call_error(
    function: &syntax::Function,
    callee: &str,
    expr: &syntax::Expr,
) -> Diagnostic {
    Diagnostic::new(
        "type",
        format!(
            "const fn {:?} can only call other const fn; {callee:?} is a host runtime or non-const call",
            function.name
        ),
    )
    .with_span(expr.line(), expr.column())
}

fn lower_function(
    function: &syntax::Function,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    aliases: &HashMap<String, syntax::TypeAliasDecl>,
    consts: &HashMap<String, syntax::ConstDecl>,
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
        consts,
        function.line,
        function.column,
    )?;
    if function.is_property {
        validate_property_signature(function, &return_ty)?;
    }
    if function.is_async {
        require_capability(
            capabilities,
            CapabilityKind::Async,
            "async fn",
            function.line,
            function.column,
        )?;
    }
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
    validate_const_function_body(function, functions)?;
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
            consts,
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
                net_origin: None,
                borrowed_owners: HashSet::new(),
                active_borrow_count: 0,
                active_mut_borrow_count: 0,
                active_borrows: HashMap::new(),
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
        let ty = lower_type(
            &param.ty,
            structs,
            enums,
            aliases,
            consts,
            param.line,
            param.column,
        )?;
        env.insert(
            param.name.clone(),
            Binding {
                ty: ty.clone(),
                moved: false,
                moved_projections: HashSet::new(),
                borrow_kind: borrow_kind_for_type(&ty, structs, enums),
                borrow_origin: binding_borrow_origin(&ty, Some(&param.name), structs, enums),
                net_origin: None,
                borrowed_owners: HashSet::new(),
                active_borrow_count: 0,
                active_mut_borrow_count: 0,
                active_borrows: HashMap::new(),
            },
        );
        params.push(Param {
            name: param.name.clone(),
            ty,
        });
    }
    let ctx = LowerContext {
        current_path: &function.path,
        consts,
        structs,
        enums,
        aliases,
        variants,
        functions,
        methods,
        capabilities,
        current_return: Some(return_ty.clone()),
        current_function: Some(function.source_name.clone()),
        current_property: function.is_property,
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
    if function.is_property {
        validate_property_verdict(function, &params, &body)?;
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
        is_property: function.is_property,
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
            net_origin: None,
            borrowed_owners: HashSet::new(),
            active_borrow_count: 0,
            active_mut_borrow_count: 0,
            active_borrows: HashMap::new(),
        });
    }
}

fn declared_array_len(
    ty: &syntax::TypeName,
    ctx: &LowerContext<'_>,
    line: usize,
    column: usize,
) -> Result<Option<usize>, Diagnostic> {
    match ty {
        syntax::TypeName::Array(_, Some(raw)) => {
            let value = resolve_const_array_len(raw.trim(), ctx.consts, line, column)?;
            if value < 0 {
                return Err(Diagnostic::new("type", "array length must be non-negative")
                    .with_span(line, column));
            }
            Ok(Some(value as usize))
        }
        _ => Ok(None),
    }
}

fn resolve_const_array_len(
    raw: &str,
    consts: &HashMap<String, syntax::ConstDecl>,
    line: usize,
    column: usize,
) -> Result<i64, Diagnostic> {
    if let Ok(value) = raw.parse::<i64>() {
        return Ok(value);
    }
    let Some(const_decl) = consts.get(raw) else {
        return Err(Diagnostic::new(
            "type",
            format!("array length {raw:?} must be a known int const/static expression"),
        )
        .with_span(line, column));
    };
    eval_const_int_expr(&const_decl.expr).ok_or_else(|| {
        Diagnostic::new(
            "type",
            format!(
                "array length const {:?} must evaluate to int",
                const_decl.name
            ),
        )
        .with_span(const_decl.line, const_decl.column)
    })
}

fn eval_const_int_expr(expr: &syntax::Expr) -> Option<i64> {
    match expr {
        syntax::Expr::Literal(syntax::Literal::Int(value)) => Some(*value),
        syntax::Expr::BinaryAdd { op, lhs, rhs, .. } => match op {
            syntax::ArithmeticOp::Add => {
                Some(eval_const_int_expr(lhs)? + eval_const_int_expr(rhs)?)
            }
            syntax::ArithmeticOp::Sub => {
                Some(eval_const_int_expr(lhs)? - eval_const_int_expr(rhs)?)
            }
            syntax::ArithmeticOp::Mul => {
                Some(eval_const_int_expr(lhs)? * eval_const_int_expr(rhs)?)
            }
            syntax::ArithmeticOp::Div => {
                Some(eval_const_int_expr(lhs)? / eval_const_int_expr(rhs)?)
            }
        },
        _ => None,
    }
}

fn resolve_const_int_decls(
    consts: &HashMap<String, syntax::ConstDecl>,
    functions: &HashMap<String, syntax::Function>,
) -> Result<HashMap<String, syntax::ConstDecl>, Diagnostic> {
    let mut resolved = consts.clone();
    let mut values = HashMap::new();
    for name in consts.keys() {
        let mut resolving = HashSet::new();
        match eval_const_int_decl(name, consts, functions, &mut values, &mut resolving)? {
            Some(value) => {
                if let Some(decl) = resolved.get_mut(name) {
                    decl.expr = syntax::Expr::Literal(syntax::Literal::Int(value));
                }
            }
            None => {
                let decl = &consts[name];
                if matches!(decl.ty, syntax::TypeName::Int)
                    && const_int_expr_contains_call(&decl.expr)
                {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "const {:?} requires a pure const fn integer expression",
                            decl.name
                        ),
                    )
                    .with_span(decl.line, decl.column));
                }
            }
        }
    }
    Ok(resolved)
}

fn eval_const_int_decl(
    name: &str,
    consts: &HashMap<String, syntax::ConstDecl>,
    functions: &HashMap<String, syntax::Function>,
    values: &mut HashMap<String, i64>,
    resolving: &mut HashSet<String>,
) -> Result<Option<i64>, Diagnostic> {
    if let Some(value) = values.get(name) {
        return Ok(Some(*value));
    }
    let Some(decl) = consts.get(name) else {
        return Ok(None);
    };
    if !resolving.insert(name.to_string()) {
        return Err(Diagnostic::new(
            "type",
            format!("const {name:?} has a recursive initializer"),
        )
        .with_span(decl.line, decl.column));
    }
    let mut locals = HashMap::new();
    let value = eval_const_int_expr_resolved(
        &decl.expr,
        consts,
        functions,
        values,
        resolving,
        &mut locals,
    )?;
    resolving.remove(name);
    if let Some(value) = value {
        values.insert(name.to_string(), value);
    }
    Ok(value)
}

fn eval_const_int_expr_resolved(
    expr: &syntax::Expr,
    consts: &HashMap<String, syntax::ConstDecl>,
    functions: &HashMap<String, syntax::Function>,
    values: &mut HashMap<String, i64>,
    resolving: &mut HashSet<String>,
    locals: &mut HashMap<String, i64>,
) -> Result<Option<i64>, Diagnostic> {
    match expr {
        syntax::Expr::Literal(syntax::Literal::Int(value)) => Ok(Some(*value)),
        syntax::Expr::VarRef { name, .. } => {
            if let Some(value) = locals.get(name) {
                return Ok(Some(*value));
            }
            eval_const_int_decl(name, consts, functions, values, resolving)
        }
        syntax::Expr::BinaryAdd { op, lhs, rhs, .. } => {
            let lhs =
                eval_const_int_expr_resolved(lhs, consts, functions, values, resolving, locals)?;
            let rhs =
                eval_const_int_expr_resolved(rhs, consts, functions, values, resolving, locals)?;
            Ok(match (lhs, rhs) {
                (Some(lhs), Some(rhs)) => Some(match op {
                    syntax::ArithmeticOp::Add => lhs + rhs,
                    syntax::ArithmeticOp::Sub => lhs - rhs,
                    syntax::ArithmeticOp::Mul => lhs * rhs,
                    syntax::ArithmeticOp::Div => lhs / rhs,
                }),
                _ => None,
            })
        }
        syntax::Expr::Call { name, args, .. } => {
            let Some(function) = functions.get(name) else {
                return Ok(None);
            };
            if !function.is_const || function.is_extern || function.params.len() != args.len() {
                return Ok(None);
            }
            let mut function_locals = HashMap::new();
            for (param, arg) in function.params.iter().zip(args.iter()) {
                let Some(value) = eval_const_int_expr_resolved(
                    arg, consts, functions, values, resolving, locals,
                )?
                else {
                    return Ok(None);
                };
                function_locals.insert(param.name.clone(), value);
            }
            eval_const_int_block(
                &function.body,
                consts,
                functions,
                values,
                resolving,
                &mut function_locals,
            )
        }
        _ => Ok(None),
    }
}

fn eval_const_int_block(
    body: &[syntax::Stmt],
    consts: &HashMap<String, syntax::ConstDecl>,
    functions: &HashMap<String, syntax::Function>,
    values: &mut HashMap<String, i64>,
    resolving: &mut HashSet<String>,
    locals: &mut HashMap<String, i64>,
) -> Result<Option<i64>, Diagnostic> {
    for stmt in body {
        match stmt {
            syntax::Stmt::Let { name, expr, .. } => {
                let Some(value) = eval_const_int_expr_resolved(
                    expr, consts, functions, values, resolving, locals,
                )?
                else {
                    return Ok(None);
                };
                locals.insert(name.clone(), value);
            }
            syntax::Stmt::Return { expr, .. } => {
                return eval_const_int_expr_resolved(
                    expr, consts, functions, values, resolving, locals,
                );
            }
            _ => return Ok(None),
        }
    }
    Ok(None)
}

fn const_int_expr_contains_call(expr: &syntax::Expr) -> bool {
    match expr {
        syntax::Expr::Call { .. } => true,
        syntax::Expr::BinaryAdd { lhs, rhs, .. } | syntax::Expr::BinaryCompare { lhs, rhs, .. } => {
            const_int_expr_contains_call(lhs) || const_int_expr_contains_call(rhs)
        }
        _ => false,
    }
}

fn validate_const_array_lengths_in_program(
    program: &syntax::Program,
    consts: &HashMap<String, syntax::ConstDecl>,
) -> Result<(), Diagnostic> {
    for struct_decl in &program.structs {
        for field in &struct_decl.fields {
            validate_const_array_lengths_in_type(&field.ty, consts, field.line, field.column)?;
        }
    }
    for enum_decl in &program.enums {
        for variant in &enum_decl.variants {
            for payload_ty in &variant.payload_tys {
                validate_const_array_lengths_in_type(
                    payload_ty,
                    consts,
                    variant.line,
                    variant.column,
                )?;
            }
        }
    }
    for alias in &program.type_aliases {
        validate_const_array_lengths_in_type(&alias.ty, consts, alias.line, alias.column)?;
    }
    for function in &program.functions {
        validate_const_array_lengths_in_type(
            &function.return_ty,
            consts,
            function.line,
            function.column,
        )?;
        for param in &function.params {
            validate_const_array_lengths_in_type(&param.ty, consts, param.line, param.column)?;
        }
    }
    for stmt in &program.stmts {
        if let syntax::Stmt::Let {
            ty, line, column, ..
        } = stmt
        {
            validate_const_array_lengths_in_type(ty, consts, *line, *column)?;
        }
    }
    Ok(())
}

fn validate_const_array_lengths_in_type(
    ty: &syntax::TypeName,
    consts: &HashMap<String, syntax::ConstDecl>,
    line: usize,
    column: usize,
) -> Result<(), Diagnostic> {
    match ty {
        syntax::TypeName::Array(inner, len) => {
            if let Some(raw) = len {
                let value = resolve_const_array_len(raw.trim(), consts, line, column)?;
                if value < 0 {
                    return Err(Diagnostic::new("type", "array length must be non-negative")
                        .with_span(line, column));
                }
            }
            validate_const_array_lengths_in_type(inner, consts, line, column)
        }
        syntax::TypeName::Slice(inner)
        | syntax::TypeName::MutSlice(inner)
        | syntax::TypeName::Option(inner) => {
            validate_const_array_lengths_in_type(inner, consts, line, column)
        }
        syntax::TypeName::Result(ok, err) | syntax::TypeName::Map(ok, err) => {
            validate_const_array_lengths_in_type(ok, consts, line, column)?;
            validate_const_array_lengths_in_type(err, consts, line, column)
        }
        syntax::TypeName::Tuple(elements) => {
            for element in elements {
                validate_const_array_lengths_in_type(element, consts, line, column)?;
            }
            Ok(())
        }
        syntax::TypeName::Named(_, args) => {
            for arg in args {
                validate_const_array_lengths_in_type(arg, consts, line, column)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn lower_match_stmt(
    expr: &syntax::Expr,
    arms: Vec<MatchArmInput>,
    line: usize,
    column: usize,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Stmt, Diagnostic> {
    let lowered_expr = lower_expr(expr, env, ctx)?;
    let match_borrowed_owners = expr_borrowed_owners(&lowered_expr, env, ctx);
    let match_borrow_kind = borrow_kind_for_type(lowered_expr.ty(), ctx.structs, ctx.enums);
    let reuse_existing_match_binding =
        matches!(lowered_expr, Expr::VarRef { .. }) && !match_borrowed_owners.is_empty();
    if let Some(borrow_kind) = match_borrow_kind
        && !reuse_existing_match_binding
    {
        increment_active_borrows(&match_borrowed_owners, env, borrow_kind, line, column)?;
    }
    if matches!(lowered_expr, Expr::VarRef { .. }) && !lowered_expr.ty().is_copy() {
        move_lowered_owner_value(&lowered_expr, env)?;
    }
    let Some((enum_name, variant_defs)) = match_variants(lowered_expr.ty(), ctx) else {
        return lower_const_match_stmt(
            lowered_expr,
            arms,
            line,
            column,
            env,
            ctx,
            match_borrow_kind,
            match_borrowed_owners,
            reuse_existing_match_binding,
        );
    };
    let before = env.clone();
    let mut seen = HashMap::new();
    let mut lowered_arms = Vec::new();
    let mut arm_states = Vec::new();
    let mut ignored_body_cache: HashMap<String, (Vec<Stmt>, HashMap<String, Binding>, bool)> =
        HashMap::new();
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
            return Err(
                Diagnostic::new("type", format!("duplicate match arm {:?}", arm.variant))
                    .with_span(arm.line, arm.column),
            );
        }
        let mut arm_env = before.clone();
        let binding_tys = if arm.ignore_payloads {
            if !arm.bindings.is_empty() {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "match arm {:?} cannot both ignore payloads and bind names",
                        arm.variant
                    ),
                )
                .with_span(arm.line, arm.column));
            }
            Vec::new()
        } else if arm.is_named {
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
            } else {
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
            }
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
            } else {
                variant_def.payload_tys.clone()
            }
        };
        let mut arm_borrow_region_facts = Vec::new();
        for (binding_index, (binding, payload_ty)) in
            arm.bindings.iter().zip(binding_tys.iter()).enumerate()
        {
            let payload_index = if arm.is_named {
                variant_def
                    .payload_names
                    .iter()
                    .position(|name| name == binding)
                    .expect("named match binding already validated")
            } else {
                binding_index
            };
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
            let borrowed_owners = match_binding_borrowed_owners(
                &lowered_expr,
                &arm.variant,
                binding,
                binding_index,
                payload_ty,
                &before,
                ctx,
            );
            arm_borrow_region_facts.extend(borrow_region_facts_for_enum_payload_binding(
                binding,
                payload_ty,
                &borrowed_owners,
                &lowered_expr,
                &arm.variant,
                payload_index,
            ));
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
                    net_origin: None,
                    borrowed_owners,
                    active_borrow_count: 0,
                    active_mut_borrow_count: 0,
                    active_borrows: HashMap::new(),
                },
            );
        }
        let (body, after, returns) = if arm.ignore_payloads && arm.bindings.is_empty() {
            let cache_key = format!("{:?}", arm.body);
            if let Some((body, after, returns)) = ignored_body_cache.get(&cache_key) {
                (body.clone(), after.clone(), *returns)
            } else {
                let lowered = lower_block(&arm.body, &mut arm_env, ctx)?;
                ignored_body_cache.insert(cache_key, lowered.clone());
                lowered
            }
        } else {
            lower_block(&arm.body, &mut arm_env, ctx)?
        };
        lowered_arms.push(MatchArm {
            enum_name: enum_name.clone(),
            variant: arm.variant.clone(),
            bindings: arm.bindings.clone(),
            is_named: arm.is_named,
            ignore_payloads: arm.ignore_payloads,
            borrow_region_facts: arm_borrow_region_facts,
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
        .with_span(line, column));
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
        span: SourceSpan::point(line, column),
    })
}

fn lower_const_match_stmt(
    lowered_expr: Expr,
    arms: Vec<MatchArmInput>,
    line: usize,
    column: usize,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
    match_borrow_kind: Option<BorrowKind>,
    match_borrowed_owners: HashSet<BorrowedOwner>,
    reuse_existing_match_binding: bool,
) -> Result<Stmt, Diagnostic> {
    if lowered_expr.ty() != &Type::Int {
        return Err(Diagnostic::new(
            "type",
            format!(
                "match expects an enum-like value or int const patterns, got {}",
                lowered_expr.ty()
            ),
        )
        .with_span(line, column));
    }
    let before = env.clone();
    let mut seen = HashMap::new();
    let mut lowered_arms = Vec::new();
    let mut arm_states = Vec::new();
    for arm in arms {
        if arm.is_named || !arm.bindings.is_empty() || arm.ignore_payloads {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "const match arm {:?} cannot bind payload values",
                    arm.variant
                ),
            )
            .with_span(arm.line, arm.column));
        }
        let value = resolve_const_match_int_pattern(&arm, ctx)?;
        if seen.insert(value, ()).is_some() {
            return Err(
                Diagnostic::new("type", format!("duplicate match arm {:?}", arm.variant))
                    .with_span(arm.line, arm.column),
            );
        }
        let mut arm_env = before.clone();
        let (body, after, returns) = lower_block(&arm.body, &mut arm_env, ctx)?;
        lowered_arms.push(MatchArm {
            enum_name: String::new(),
            variant: value.to_string(),
            bindings: Vec::new(),
            is_named: false,
            ignore_payloads: false,
            borrow_region_facts: Vec::new(),
            body,
        });
        arm_states.push((after, returns));
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
        span: SourceSpan::point(line, column),
    })
}

fn lower_match_expr(
    expr: &syntax::Expr,
    arms: Vec<MatchExprArmInput>,
    expected: Option<&Type>,
    line: usize,
    column: usize,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Expr, Diagnostic> {
    let lowered_expr = lower_expr(expr, env, ctx)?;
    let Some((enum_name, variant_defs)) = match_variants(lowered_expr.ty(), ctx) else {
        return Err(Diagnostic::new(
            "type",
            format!(
                "match expression expects an enum-like value, got {}",
                lowered_expr.ty()
            ),
        )
        .with_span(line, column));
    };
    let match_borrowed_owners = expr_borrowed_owners(&lowered_expr, env, ctx);
    let match_borrow_kind = borrow_kind_for_type(lowered_expr.ty(), ctx.structs, ctx.enums);
    let reuse_existing_match_binding =
        matches!(lowered_expr, Expr::VarRef { .. }) && !match_borrowed_owners.is_empty();
    if let Some(borrow_kind) = match_borrow_kind
        && !reuse_existing_match_binding
    {
        increment_active_borrows(&match_borrowed_owners, env, borrow_kind, line, column)?;
    }
    if matches!(lowered_expr, Expr::VarRef { .. }) && !lowered_expr.ty().is_copy() {
        move_lowered_owner_value(&lowered_expr, env)?;
    }

    let before = env.clone();
    let mut seen = HashMap::new();
    let mut lowered_arms = Vec::new();
    let mut arm_states = Vec::new();
    let mut result_ty = expected.cloned();
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
            return Err(
                Diagnostic::new("type", format!("duplicate match arm {:?}", arm.variant))
                    .with_span(arm.line, arm.column),
            );
        }
        let mut arm_env = before.clone();
        let binding_tys = match_match_arm_binding_types(
            &arm.variant,
            &arm.bindings,
            arm.is_named,
            variant_def,
            arm.line,
            arm.column,
        )?;
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
                    net_origin: None,
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
                    active_borrows: HashMap::new(),
                },
            );
        }
        let lowered_arm_expr =
            lower_expr_with_expected(&arm.expr, result_ty.as_ref(), &mut arm_env, ctx)?;
        if let Some(expected_ty) = result_ty.as_ref() {
            if !type_assignable_to(lowered_arm_expr.ty(), expected_ty) {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "match expression arm {:?} expects {expected_ty}, got {}",
                        arm.variant,
                        lowered_arm_expr.ty()
                    ),
                )
                .with_span(arm.line, arm.column));
            }
        } else {
            result_ty = Some(lowered_arm_expr.ty().clone());
        }
        if !lowered_arm_expr.ty().is_copy() {
            move_lowered_owner_value(&lowered_arm_expr, &mut arm_env)?;
        }
        lowered_arms.push(MatchExprArm {
            enum_name: enum_name.clone(),
            variant: arm.variant,
            bindings: arm.bindings,
            is_named: arm.is_named,
            expr: lowered_arm_expr,
        });
        arm_states.push((arm_env, false));
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
        .with_span(line, column));
    }
    merge_match_state(env, &before, &arm_states);
    if let Some(borrow_kind) = match_borrow_kind
        && !reuse_existing_match_binding
    {
        release_active_borrow_owners(&match_borrowed_owners, env, borrow_kind);
    }
    Ok(Expr::Match {
        expr: Box::new(lowered_expr),
        arms: lowered_arms,
        ty: result_ty.unwrap_or(Type::Error),
    })
}

fn match_match_arm_binding_types(
    variant: &str,
    bindings: &[String],
    is_named: bool,
    variant_def: &EnumVariantDef,
    line: usize,
    column: usize,
) -> Result<Vec<Type>, Diagnostic> {
    if is_named {
        if variant_def.payload_names.is_empty() {
            return Err(Diagnostic::new(
                "type",
                format!("match arm {variant:?} uses named bindings, but variant {variant:?} is positional"),
            )
            .with_span(line, column));
        }
        if bindings.len() != variant_def.payload_names.len() {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "match arm {variant:?} expects {} named bindings, got {}",
                    variant_def.payload_names.len(),
                    bindings.len()
                ),
            )
            .with_span(line, column));
        }
        let mut seen_named = HashMap::new();
        let mut payload_tys = Vec::new();
        for binding in bindings {
            let Some(position) = variant_def
                .payload_names
                .iter()
                .position(|name| name == binding)
            else {
                return Err(Diagnostic::new(
                    "type",
                    format!("match arm {variant:?} has no named payload {binding:?}"),
                )
                .with_span(line, column));
            };
            if seen_named.insert(binding.clone(), ()).is_some() {
                return Err(Diagnostic::new(
                    "type",
                    format!("match arm {variant:?} repeats named payload {binding:?}"),
                )
                .with_span(line, column));
            }
            payload_tys.push(variant_def.payload_tys[position].clone());
        }
        Ok(payload_tys)
    } else {
        if !variant_def.payload_names.is_empty() {
            return Err(Diagnostic::new(
                "type",
                format!("match arm {variant:?} must use named bindings for variant {variant:?}"),
            )
            .with_span(line, column));
        }
        if bindings.len() != variant_def.payload_tys.len() {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "match arm {variant:?} expects {} bindings, got {}",
                    variant_def.payload_tys.len(),
                    bindings.len()
                ),
            )
            .with_span(line, column));
        }
        Ok(variant_def.payload_tys.clone())
    }
}

fn resolve_const_match_int_pattern(
    arm: &MatchArmInput,
    ctx: &LowerContext<'_>,
) -> Result<i64, Diagnostic> {
    if let Ok(value) = arm.variant.parse::<i64>() {
        return Ok(value);
    }
    if !starts_with_ascii_uppercase(&arm.variant) {
        return Err(Diagnostic::new(
            "type",
            format!(
                "match pattern {:?} must name an uppercase int const",
                arm.variant
            ),
        )
        .with_span(arm.line, arm.column));
    }
    let Some(const_decl) = ctx.consts.get(&arm.variant) else {
        return Err(Diagnostic::new(
            "type",
            format!(
                "match pattern {:?} must name a known int const",
                arm.variant
            ),
        )
        .with_span(arm.line, arm.column));
    };
    eval_const_int_expr(&const_decl.expr).ok_or_else(|| {
        Diagnostic::new(
            "type",
            format!(
                "match pattern const {:?} must evaluate to int",
                const_decl.name
            ),
        )
        .with_span(const_decl.line, const_decl.column)
    })
}

fn starts_with_ascii_uppercase(value: &str) -> bool {
    value
        .chars()
        .next()
        .map(|ch| ch.is_ascii_uppercase())
        .unwrap_or(false)
}

fn is_assignment_target(expr: &Expr, ctx: &LowerContext<'_>) -> bool {
    match expr {
        Expr::VarRef { ty, .. } => is_local_assignment_type(ty, ctx),
        Expr::Deref { .. } => true,
        Expr::Index { base, .. } => matches!(base.ty(), Type::MutSlice(_)),
        _ => false,
    }
}

fn is_local_assignment_type(ty: &Type, ctx: &LowerContext<'_>) -> bool {
    is_supported_local_assignment_type(ty, ctx)
}

fn is_scalar_local_assignment_type(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Numeric(_) | Type::Bool)
}

fn is_supported_local_assignment_type(ty: &Type, ctx: &LowerContext<'_>) -> bool {
    is_supported_local_assignment_type_inner(ty, ctx, 0)
}

fn is_supported_local_assignment_type_inner(
    ty: &Type,
    ctx: &LowerContext<'_>,
    depth: usize,
) -> bool {
    if depth > 8 {
        return false;
    }
    match ty {
        Type::Int | Type::Numeric(_) | Type::Bool => true,
        Type::Option(inner) => is_supported_local_assignment_type_inner(inner, ctx, depth + 1),
        Type::Result(ok, err) => {
            is_supported_local_assignment_type_inner(ok, ctx, depth + 1)
                && is_supported_local_assignment_type_inner(err, ctx, depth + 1)
        }
        Type::Tuple(elements) => elements
            .iter()
            .all(|element| is_supported_local_assignment_type_inner(element, ctx, depth + 1)),
        Type::Array(element, Some(_)) => is_scalar_local_assignment_type(element),
        Type::Struct(name) => ctx
            .structs
            .get(name)
            .map(|struct_def| {
                struct_def.fields.iter().all(|field| {
                    is_supported_local_assignment_type_inner(&field.ty, ctx, depth + 1)
                })
            })
            .unwrap_or(false),
        Type::Enum(name) => ctx
            .enums
            .get(name)
            .map(|enum_def| {
                enum_def.variants.iter().all(|variant| {
                    variant.payload_tys.iter().all(|payload| {
                        is_supported_local_assignment_type_inner(payload, ctx, depth + 1)
                    })
                })
            })
            .unwrap_or(false),
        _ => false,
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
                let existing = &env[name];
                let message = if !existing.ty.is_copy() {
                    format!(
                        "rebinding name {name:?} is not supported; the existing binding holds an owned value"
                    )
                } else {
                    format!("rebinding name {name:?} is not supported in stage1")
                };
                return Err(Diagnostic::new("ownership", message)
                    .with_code("rebind_not_supported")
                    .with_span(*line, *column));
            }
            let expected = lower_type(
                ty,
                ctx.structs,
                ctx.enums,
                ctx.aliases,
                ctx.consts,
                *line,
                *column,
            )?;
            let expected_array_len = declared_array_len(ty, ctx, *line, *column)?;
            let lowered_expr = lower_expr_with_expected(expr, Some(&expected), env, ctx)?;
            let actual = lowered_expr.ty().clone();
            if let Some(expected_len) = expected_array_len {
                if let syntax::Expr::ArrayLiteral { elements, .. } = expr {
                    if elements.len() != expected_len {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "array literal length mismatch: declared {expected_len}, got {}",
                                elements.len()
                            ),
                        )
                        .with_span(*line, *column));
                    }
                }
            }
            if !type_assignable_to(&actual, &expected) && !actual.is_error() && !expected.is_error()
            {
                return Err(Diagnostic::new(
                    "type",
                    format!("let binding {name:?} expects {expected}, got {actual}"),
                )
                .with_span(*line, *column));
            }
            let borrowed_owners =
                binding_borrowed_owners_from_expr(&expected, &lowered_expr, env, ctx);
            let borrow_region_facts =
                borrow_region_facts_for_binding(name, &expected, &borrowed_owners);
            if let Some(borrow_kind) = borrow_kind_for_type(&expected, ctx.structs, ctx.enums) {
                increment_active_borrows(&borrowed_owners, env, borrow_kind, *line, *column)?;
            }
            if !actual.is_copy() {
                move_lowered_value(&lowered_expr, env)?;
            }
            let net_origin = net_binding_origin_from_expr(&lowered_expr, env, ctx);
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
                    net_origin,
                    borrowed_owners,
                    active_borrow_count: 0,
                    active_mut_borrow_count: 0,
                    active_borrows: HashMap::new(),
                },
            );
            Ok(Stmt::Let {
                name: name.clone(),
                ty: expected,
                expr: lowered_expr,
                borrow_region_facts,
                span: SourceSpan::point(*line, *column),
            })
        }
        syntax::Stmt::Assign {
            target,
            expr,
            line,
            column,
        } => {
            let lowered_target = lower_expr(target, env, ctx)?;
            if !is_assignment_target(&lowered_target, ctx) {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "assignment target must be a scalar local, dereference a mutable reference, or index a mutable slice, got {}",
                        lowered_target.ty()
                    ),
                )
                .with_span(*line, *column));
            }
            let target_ty = lowered_target.ty().clone();
            let lowered_expr = lower_expr_with_expected(expr, Some(&target_ty), env, ctx)?;
            if !type_assignable_to(lowered_expr.ty(), &target_ty) {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "assignment expects value type {target_ty}, got {}",
                        lowered_expr.ty()
                    ),
                )
                .with_span(expr.line(), expr.column()));
            }
            if !target_ty.is_copy() {
                move_lowered_value(&lowered_expr, env)?;
            }
            Ok(Stmt::Assign {
                target: lowered_target,
                expr: lowered_expr,
                span: SourceSpan::point(*line, *column),
            })
        }
        syntax::Stmt::Print { expr, line, column } => {
            let lowered = lower_expr(expr, env, ctx)?;
            if !matches!(
                lowered.ty(),
                Type::Error | Type::Int | Type::Numeric(_) | Type::Bool | Type::String | Type::Str
            ) {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "print expects int, bool, String, or &str, got {}",
                        lowered.ty()
                    ),
                )
                .with_span(*line, *column));
            }
            Ok(Stmt::Print {
                expr: lowered,
                span: SourceSpan::point(*line, *column),
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
                span: SourceSpan::point(*line, *column),
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
                        span: SourceSpan::point(*line, *column),
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
                        span: SourceSpan::point(*line, *column),
                    });
                }
                return Ok(Stmt::If {
                    cond: lowered_cond,
                    then_block: Vec::new(),
                    else_block: None,
                    span: SourceSpan::point(*line, *column),
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
                span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
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
                span: SourceSpan::point(*line, *column),
            })
        }
        syntax::Stmt::IfLet {
            variant,
            bindings,
            is_named,
            expr,
            then_block,
            else_block,
            line,
            column,
        } => {
            let mut probe_env = env.clone();
            let lowered_expr = lower_expr(expr, &mut probe_env, ctx)?;
            let (_, variant_defs) = match_variants(lowered_expr.ty(), ctx).ok_or_else(|| {
                Diagnostic::new(
                    "type",
                    format!(
                        "if let expects an enum-like value, got {}",
                        lowered_expr.ty()
                    ),
                )
                .with_span(*line, *column)
            })?;
            let variant_def = variant_defs
                .iter()
                .find(|candidate| candidate.name == *variant)
                .ok_or_else(|| {
                    Diagnostic::new(
                        "type",
                        message_with_suggestion(
                            format!("if let pattern has no variant {:?}", variant),
                            variant,
                            variant_defs.iter().map(|candidate| candidate.name.as_str()),
                        ),
                    )
                    .with_span(*line, *column)
                })?;
            if *is_named && variant_def.payload_names.is_empty() {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "if let pattern {:?} uses named bindings, but variant {:?} is positional",
                        variant, variant
                    ),
                )
                .with_span(*line, *column));
            }
            if !*is_named && !variant_def.payload_names.is_empty() {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "if let pattern {:?} must use named bindings for variant {:?}",
                        variant, variant
                    ),
                )
                .with_span(*line, *column));
            }
            let expected_bindings = if *is_named {
                variant_def.payload_names.len()
            } else {
                variant_def.payload_tys.len()
            };
            if bindings.len() != expected_bindings {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "if let pattern {:?} expects {} bindings, got {}",
                        variant,
                        expected_bindings,
                        bindings.len()
                    ),
                )
                .with_span(*line, *column));
            }
            let mut arms = Vec::new();
            arms.push(MatchArmInput {
                variant: variant.clone(),
                bindings: bindings.clone(),
                is_named: *is_named,
                ignore_payloads: false,
                body: then_block.clone(),
                line: *line,
                column: *column,
            });
            let fallback_body = else_block.clone().unwrap_or_default();
            for candidate in variant_defs {
                if candidate.name != *variant {
                    arms.push(MatchArmInput {
                        variant: candidate.name.clone(),
                        bindings: Vec::new(),
                        is_named: !candidate.payload_names.is_empty(),
                        ignore_payloads: true,
                        body: fallback_body.clone(),
                        line: *line,
                        column: *column,
                    });
                }
            }
            lower_match_stmt(expr, arms, *line, *column, env, ctx)
        }
        syntax::Stmt::Match {
            expr,
            arms,
            line,
            column,
        } => {
            let arms = arms.iter().map(MatchArmInput::from).collect();
            lower_match_stmt(expr, arms, *line, *column, env, ctx)
        }
        syntax::Stmt::Defer { expr, line, column } => {
            let lowered_expr = lower_expr(expr, env, ctx)?;
            if !lowered_expr.ty().is_copy() {
                move_lowered_value(&lowered_expr, env)?;
            }
            Ok(Stmt::Defer {
                expr: lowered_expr,
                span: SourceSpan::point(*line, *column),
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
            if !type_assignable_to(lowered_expr.ty(), expected) {
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
                        .with_help("the returned borrow must derive from a borrowed parameter, not from a locally-allocated value")
                        .with_span(*line, *column));
                    }
                }
            }
            let borrow_region_facts = ctx
                .current_function
                .as_ref()
                .map(|function| {
                    borrow_region_facts_for_return_expr(function, &lowered_expr, env, ctx)
                })
                .unwrap_or_default();
            Ok(Stmt::Return {
                expr: lowered_expr,
                borrow_region_facts,
                span: SourceSpan::point(*line, *column),
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
                net_origin: binding.net_origin.clone(),
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
                active_borrows: binding.active_borrows.clone(),
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
                net_origin: binding.net_origin.clone(),
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
                active_borrows: binding.active_borrows.clone(),
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
                net_origin: binding.net_origin.clone(),
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
                active_borrows: binding.active_borrows.clone(),
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
    lower_expr_with_expected_inner(expr, expected, env, ctx)
        .and_then(|lowered| coerce_lowered_expr_to_expected(lowered, expected))
}

fn lower_expr_with_expected_inner(
    expr: &syntax::Expr,
    expected: Option<&Type>,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Expr, Diagnostic> {
    match expr {
        syntax::Expr::Literal(literal) => Ok(lower_literal(literal, expected)),
        syntax::Expr::VarRef { name, line, column } => {
            if let Some(binding) = env.get(name) {
                if binding.moved {
                    return Err(ownership_error(
                        OWNERSHIP_USE_AFTER_MOVE,
                        format!("use of moved value {name:?}"),
                    )
                    .with_help("consider restructuring to avoid the move, or ensure the value is only used once")
                    .with_span_extent(*line, *column, name.chars().count()));
                }
                if !binding.moved_projections.is_empty() {
                    return Err(ownership_error(
                        OWNERSHIP_USE_AFTER_MOVE,
                        format!("use of partially moved value {name:?}"),
                    )
                    .with_help("consider restructuring to avoid the move, or ensure the value is only used once")
                    .with_span_extent(*line, *column, name.chars().count()));
                }
                if binding.active_mut_borrow_count > 0 {
                    return Err(ownership_error(
                        OWNERSHIP_MOVE_WHILE_BORROWED,
                        format!("cannot move value {name:?} while borrowed slices are still live"),
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
        syntax::Expr::Match {
            expr,
            arms,
            line,
            column,
        } => {
            let arms = arms.iter().map(MatchExprArmInput::from).collect();
            lower_match_expr(expr, arms, expected, *line, *column, env, ctx)
        }
        syntax::Expr::MutBorrow { expr, line, column } => {
            let syntax::Expr::VarRef { name, .. } = expr.as_ref() else {
                return Err(Diagnostic::new(
                    "type",
                    "mutable local borrows currently require a named local target",
                )
                .with_span(*line, *column));
            };
            let Some(binding) = env.get(name) else {
                return Err(Diagnostic::new(
                    "type",
                    message_with_suggestion(
                        format!("undefined variable {name:?}"),
                        name,
                        env.keys(),
                    ),
                )
                .with_span(*line, *column));
            };
            if binding.moved {
                return Err(ownership_error(
                    OWNERSHIP_USE_AFTER_MOVE,
                    format!("use of moved value {name:?}"),
                )
                .with_span_extent(*line, *column, name.chars().count()));
            }
            Ok(Expr::MutBorrow {
                expr: Box::new(Expr::VarRef {
                    name: name.clone(),
                    ty: binding.ty.clone(),
                }),
                ty: Type::MutRef(Box::new(binding.ty.clone())),
            })
        }
        syntax::Expr::Deref { expr, line, column } => {
            let lowered = lower_expr(expr, env, ctx)?;
            let inner_ty = match lowered.ty() {
                Type::MutRef(inner_ty) => (*inner_ty.clone()).clone(),
                ty => {
                    return Err(Diagnostic::new(
                        "type",
                        format!("dereference expects a mutable reference, got {ty}"),
                    )
                    .with_span(*line, *column));
                }
            };
            Ok(Expr::Deref {
                expr: Box::new(lowered),
                ty: inner_ty,
            })
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
            if matches!(
                name.as_str(),
                "map_get"
                    | "map_contains_key"
                    | "map_keys"
                    | "contains"
                    | "get"
                    | "get_or_default"
                    | "keys"
            ) {
                return lower_map_lookup_intrinsic(name, type_args, args, *line, *column, env, ctx);
            }
            if name == "panic" {
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
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![message],
                    ty: Type::Never,
                });
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
                if ctx.current_property && matches!(expected, Some(Type::Bool)) {
                    return Ok(lowered);
                }
                move_lowered_value(&lowered, env)?;
                let ty = if static_bool_value(&lowered) == Some(false) {
                    Type::Never
                } else {
                    Type::Int
                };
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: with_assert_location(vec![lowered], *line, *column),
                    ty,
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
                    span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
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
                if !matches!(lhs.ty(), Type::Int | Type::Bool | Type::String | Type::Str) {
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
                    span: SourceSpan::point(*line, *column),
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
                    Type::Array(_, _)
                        | Type::Slice(_)
                        | Type::MutSlice(_)
                        | Type::String
                        | Type::Str
                ) {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "len expects an array, slice, or string value, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::Int,
                });
            }
            if name == "io_readline" {
                // Ungated: stdin input is ambient, matching `print` and
                // `io_eprintln` for stdio access. No capability check.
                if !args.is_empty() {
                    return Err(Diagnostic::new(
                        "type",
                        format!("io_readline expects 0 arguments, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: Vec::new(),
                    ty: Type::Option(Box::new(Type::String)),
                });
            }
            if name == "io_read_to_string" {
                // Ungated: stdin input is ambient, matching `print` and
                // `io_eprintln` for stdio access. No capability check.
                if !args.is_empty() {
                    return Err(Diagnostic::new(
                        "type",
                        format!("io_read_to_string expects 0 arguments, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: Vec::new(),
                    ty: Type::String,
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
                    span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::String,
                });
            }
            if name == "json_parse_value" {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("json_parse_value expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if lowered.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "json_parse_value expects a string argument, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::Option(Box::new(Type::String)),
                });
            }
            if name == "json_stringify_value" {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "json_stringify_value expects 1 argument, got {}",
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
                            "json_stringify_value expects a string argument, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::String,
                });
            }
            if name == "json_parse_field_value" {
                if args.len() != 2 {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "json_parse_field_value expects 2 arguments, got {}",
                            args.len()
                        ),
                    )
                    .with_span(*line, *column));
                }
                let text = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if text.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "json_parse_field_value expects a string JSON argument, got {}",
                            text.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let key = lower_expr_with_expected(&args[1], Some(&Type::String), env, ctx)?;
                if key.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "json_parse_field_value expects a string key argument, got {}",
                            key.ty()
                        ),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                move_lowered_value(&text, env)?;
                move_lowered_value(&key, env)?;
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![text, key],
                    ty: Type::Option(Box::new(Type::String)),
                });
            }
            if name == "json_serdes_parse" {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("json_serdes_parse expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if lowered.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "json_serdes_parse expects a string argument, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let value_ty = Type::Enum(String::from("std_serdes_Value"));
                let error_ty = Type::Struct(String::from("std_serdes_ParseError"));
                if !ctx.enums.contains_key("std_serdes_Value")
                    || !ctx.structs.contains_key("std_serdes_ParseError")
                {
                    return Err(Diagnostic::new(
                        "type",
                        "json_serdes_parse requires std/serdes.ax Value and ParseError types",
                    )
                    .with_span(*line, *column));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::Result(Box::new(value_ty), Box::new(error_ty)),
                });
            }
            if name == "json_serdes_parse_str" {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "json_serdes_parse_str expects 1 argument, got {}",
                            args.len()
                        ),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::Str), env, ctx)?;
                if lowered.ty() != &Type::Str {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "json_serdes_parse_str expects an &str argument, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let value_ty = Type::Enum(String::from("std_serdes_Value"));
                let error_ty = Type::Struct(String::from("std_serdes_ParseError"));
                if !ctx.enums.contains_key("std_serdes_Value")
                    || !ctx.structs.contains_key("std_serdes_ParseError")
                {
                    return Err(Diagnostic::new(
                        "type",
                        "json_serdes_parse_str requires std/serdes.ax Value and ParseError types",
                    )
                    .with_span(*line, *column));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::Result(Box::new(value_ty), Box::new(error_ty)),
                });
            }
            if name == "json_serdes_value_to_json" {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "json_serdes_value_to_json expects 1 argument, got {}",
                            args.len()
                        ),
                    )
                    .with_span(*line, *column));
                }
                let value_ty = Type::Enum(String::from("std_serdes_Value"));
                let lowered = lower_expr_with_expected(&args[0], Some(&value_ty), env, ctx)?;
                if lowered.ty() != &value_ty {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "json_serdes_value_to_json expects std/serdes Value, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::String,
                });
            }
            if name == "json_serdes_to_json" {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("json_serdes_to_json expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let value_ty = Type::Enum(String::from("std_serdes_Value"));
                let expected_ty = Type::Map(Box::new(Type::String), Box::new(value_ty));
                let lowered = lower_expr_with_expected(&args[0], Some(&expected_ty), env, ctx)?;
                if lowered.ty() != &expected_ty {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "json_serdes_to_json expects {{string: Value}}, got {}",
                            lowered.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: lowered_args,
                    ty,
                });
            }
            if matches!(
                name.as_str(),
                "string_line_at"
                    | "string_clone"
                    | "string_starts_with"
                    | "string_strip_prefix"
                    | "string_strip_suffix"
                    | "string_trim"
                    | "string_trim_start"
            ) {
                let expected_len = if name == "string_clone"
                    || name == "string_trim"
                    || name == "string_trim_start"
                {
                    1
                } else {
                    2
                };
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
                let string_arg_count = if name == "string_line_at" {
                    1
                } else {
                    args.len()
                };
                for (idx, arg) in args.iter().take(string_arg_count).enumerate() {
                    let lowered = lower_expr_with_expected(arg, Some(&Type::Str), env, ctx)?;
                    if lowered.ty() != &Type::Str {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "{name} expects argument {} type &str, got {}",
                                idx + 1,
                                lowered.ty()
                            ),
                        )
                        .with_span(arg.line(), arg.column()));
                    }
                    lowered_args.push(lowered);
                }
                if name == "string_line_at" {
                    let lowered = lower_expr_with_expected(&args[1], Some(&Type::Int), env, ctx)?;
                    if lowered.ty() != &Type::Int {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "string_line_at expects argument 2 type int, got {}",
                                lowered.ty()
                            ),
                        )
                        .with_span(args[1].line(), args[1].column()));
                    }
                    lowered_args.push(lowered);
                }
                let ty = match name.as_str() {
                    "string_line_at" => Type::Option(Box::new(Type::String)),
                    "string_clone" => Type::String,
                    "string_starts_with" => Type::Bool,
                    "string_strip_prefix" => Type::Option(Box::new(Type::String)),
                    "string_strip_suffix" => Type::Option(Box::new(Type::String)),
                    "string_trim" | "string_trim_start" => Type::String,
                    _ => unreachable!("string intrinsic checked above"),
                };
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: lowered_args,
                    ty,
                });
            }
            if matches!(
                name.as_str(),
                "encoding_url_component_encode"
                    | "encoding_url_component_decode"
                    | "encoding_path_segment_encode"
                    | "encoding_url_query_pair_encode"
                    | "encoding_path_join_segment"
            ) {
                let expected_arity = if matches!(
                    name.as_str(),
                    "encoding_url_query_pair_encode" | "encoding_path_join_segment"
                ) {
                    2
                } else {
                    1
                };
                if args.len() != expected_arity {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "{name} expects {expected_arity} argument(s), got {}",
                            args.len()
                        ),
                    )
                    .with_span(*line, *column));
                }
                let mut lowered_args = Vec::new();
                for arg in args {
                    let lowered = lower_expr_with_expected(arg, Some(&Type::String), env, ctx)?;
                    if lowered.ty() != &Type::String {
                        return Err(Diagnostic::new(
                            "type",
                            format!("{name} expects string arguments, got {}", lowered.ty()),
                        )
                        .with_span(arg.line(), arg.column()));
                    }
                    move_lowered_value(&lowered, env)?;
                    lowered_args.push(lowered);
                }
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: lowered_args,
                    ty: if name == "encoding_url_component_decode" {
                        Type::Option(Box::new(Type::String))
                    } else {
                        Type::String
                    },
                });
            }
            if name == "cli_args" || name == "cli_arg_count" {
                if !args.is_empty() {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects 0 arguments, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: Vec::new(),
                    ty: if name == "cli_args" {
                        Type::Array(Box::new(Type::String), None)
                    } else {
                        Type::Int
                    },
                });
            }
            if name == "cli_arg" {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("cli_arg expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let lowered = lower_expr_with_expected(&args[0], Some(&Type::Int), env, ctx)?;
                if lowered.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!("cli_arg expects an int argument, got {}", lowered.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::Option(Box::new(Type::String)),
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
                    span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
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
                validate_net_host_allowlist_hir(
                    ctx.capabilities,
                    name,
                    &lowered,
                    *line,
                    *column,
                    stdlib_dynamic_net_host_allowed(ctx.capabilities, ctx),
                )?;
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::Option(Box::new(Type::String)),
                });
            }
            if name == "net_tcp_listen" {
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("net_tcp_listen expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let bind = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if bind.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "net_tcp_listen expects argument 1 type string, got {}",
                            bind.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                validate_net_socket_allowlist_hir(
                    ctx.capabilities,
                    name,
                    &bind,
                    *line,
                    *column,
                    stdlib_dynamic_net_socket_allowed(ctx.capabilities, ctx),
                )?;
                move_lowered_value(&bind, env)?;
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![bind],
                    ty: Type::Int,
                });
            }
            if matches!(
                name.as_str(),
                "net_tcp_listener_port"
                    | "net_tcp_accept"
                    | "net_tcp_close"
                    | "net_tcp_close_listener"
            ) {
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let handle = lower_expr_with_expected(&args[0], Some(&Type::Int), env, ctx)?;
                if handle.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects argument 1 type int, got {}", handle.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![handle],
                    ty: Type::Int,
                });
            }
            if name == "net_tcp_read" || name == "net_tcp_write" {
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                if args.len() != 2 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects 2 arguments, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let stream = lower_expr_with_expected(&args[0], Some(&Type::Int), env, ctx)?;
                if stream.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects argument 1 type int, got {}", stream.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let byte_ty = Type::Numeric(syntax::NumericType::U8);
                let buffer_ty = if name == "net_tcp_read" {
                    Type::MutSlice(Box::new(byte_ty))
                } else {
                    Type::Slice(Box::new(byte_ty))
                };
                let buffer = lower_expr_with_expected(&args[1], Some(&buffer_ty), env, ctx)?;
                if buffer.ty() != &buffer_ty {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "{name} expects argument 2 type {buffer_ty}, got {}",
                            buffer.ty()
                        ),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![stream, buffer],
                    ty: Type::Int,
                });
            }
            if name == "net_tcp_read_string" || name == "net_tcp_write_string" {
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                if args.len() != 2 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects 2 arguments, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let stream = lower_expr_with_expected(&args[0], Some(&Type::Int), env, ctx)?;
                if stream.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects argument 1 type int, got {}", stream.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let expected = if name == "net_tcp_read_string" {
                    Type::Int
                } else {
                    Type::String
                };
                let value = lower_expr_with_expected(&args[1], Some(&expected), env, ctx)?;
                if value.ty() != &expected {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "{name} expects argument 2 type {expected}, got {}",
                            value.ty()
                        ),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                if name == "net_tcp_write_string" {
                    move_lowered_value(&value, env)?;
                }
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![stream, value],
                    ty: if name == "net_tcp_read_string" {
                        Type::String
                    } else {
                        Type::Int
                    },
                });
            }
            if name == "net_udp_bind" {
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("net_udp_bind expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let bind = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if bind.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "net_udp_bind expects argument 1 type string, got {}",
                            bind.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                validate_net_socket_allowlist_hir(
                    ctx.capabilities,
                    name,
                    &bind,
                    *line,
                    *column,
                    stdlib_dynamic_net_socket_allowed(ctx.capabilities, ctx),
                )?;
                move_lowered_value(&bind, env)?;
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![bind],
                    ty: Type::Int,
                });
            }
            if matches!(
                name.as_str(),
                "net_udp_local_addr" | "net_udp_local_port" | "net_udp_close"
            ) {
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let handle = lower_expr_with_expected(&args[0], Some(&Type::Int), env, ctx)?;
                if handle.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects argument 1 type int, got {}", handle.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let ty = if name == "net_udp_local_addr" {
                    Type::String
                } else {
                    Type::Int
                };
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![handle],
                    ty,
                });
            }
            if name == "net_udp_send_to" || name == "net_udp_recv_from" {
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                let expected_len = if name == "net_udp_send_to" { 3 } else { 2 };
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
                let socket = lower_expr_with_expected(&args[0], Some(&Type::Int), env, ctx)?;
                if socket.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects argument 1 type int, got {}", socket.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let byte_ty = Type::Numeric(syntax::NumericType::U8);
                let buffer_ty = if name == "net_udp_recv_from" {
                    Type::MutSlice(Box::new(byte_ty))
                } else {
                    Type::Slice(Box::new(byte_ty))
                };
                let buffer = lower_expr_with_expected(&args[1], Some(&buffer_ty), env, ctx)?;
                if buffer.ty() != &buffer_ty {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "{name} expects argument 2 type {buffer_ty}, got {}",
                            buffer.ty()
                        ),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                if name == "net_udp_send_to" {
                    let peer = lower_expr_with_expected(&args[2], Some(&Type::String), env, ctx)?;
                    if peer.ty() != &Type::String {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "net_udp_send_to expects argument 3 type string, got {}",
                                peer.ty()
                            ),
                        )
                        .with_span(args[2].line(), args[2].column()));
                    }
                    validate_net_socket_allowlist_hir(
                        ctx.capabilities,
                        name,
                        &peer,
                        *line,
                        *column,
                        stdlib_dynamic_net_socket_allowed(ctx.capabilities, ctx),
                    )?;
                    move_lowered_value(&peer, env)?;
                    return Ok(Expr::Call {
                        span: SourceSpan::point(*line, *column),
                        name: name.clone(),
                        args: vec![socket, buffer, peer],
                        ty: Type::Int,
                    });
                }
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![socket, buffer],
                    ty: Type::Tuple(vec![Type::Int, Type::String]),
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
                    span: SourceSpan::point(*line, *column),
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
                validate_net_host_allowlist_hir(
                    ctx.capabilities,
                    name,
                    &host,
                    *line,
                    *column,
                    stdlib_dynamic_net_peer_host_allowed(ctx.capabilities, ctx),
                )?;
                validate_net_port_allowlist_hir(
                    ctx.capabilities,
                    name,
                    &port,
                    *line,
                    *column,
                    stdlib_dynamic_net_peer_port_allowed(ctx.capabilities, ctx),
                )?;
                move_lowered_value(&host, env)?;
                move_lowered_value(&message, env)?;
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
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
                validate_net_host_allowlist_hir(
                    ctx.capabilities,
                    name,
                    &host,
                    *line,
                    *column,
                    stdlib_dynamic_net_peer_host_allowed(ctx.capabilities, ctx),
                )?;
                validate_net_port_allowlist_hir(
                    ctx.capabilities,
                    name,
                    &port,
                    *line,
                    *column,
                    stdlib_dynamic_net_peer_port_allowed(ctx.capabilities, ctx),
                )?;
                move_lowered_value(&host, env)?;
                move_lowered_value(&message, env)?;
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
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
                validate_http_get_net_allowlist_hir(
                    ctx.capabilities,
                    name,
                    &lowered,
                    *line,
                    *column,
                    is_stdlib_http_get_wrapper(ctx),
                )?;
                move_lowered_value(&lowered, env)?;
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
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
                validate_net_socket_allowlist_hir(
                    ctx.capabilities,
                    name,
                    &bind,
                    *line,
                    *column,
                    stdlib_dynamic_http_socket_allowed(ctx.capabilities, ctx),
                )?;
                move_lowered_value(&bind, env)?;
                move_lowered_value(&body, env)?;
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
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
                validate_net_socket_allowlist_hir(
                    ctx.capabilities,
                    name,
                    &bind,
                    *line,
                    *column,
                    stdlib_dynamic_http_socket_allowed(ctx.capabilities, ctx),
                )?;
                move_lowered_value(&bind, env)?;
                move_lowered_value(&route_path, env)?;
                move_lowered_value(&body, env)?;
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![bind, route_path, body, max_requests],
                    ty: Type::Bool,
                });
            }
            if name == "http_server_listen" {
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("http_server_listen expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let bind = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if bind.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "http_server_listen expects a string bind argument, got {}",
                            bind.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                validate_net_socket_allowlist_hir(
                    ctx.capabilities,
                    name,
                    &bind,
                    *line,
                    *column,
                    stdlib_dynamic_http_socket_allowed(ctx.capabilities, ctx),
                )?;
                move_lowered_value(&bind, env)?;
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![bind],
                    ty: Type::Int,
                });
            }
            if matches!(
                name.as_str(),
                "http_server_local_port" | "http_server_accept" | "http_server_close"
            ) {
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let server = lower_expr_with_expected(&args[0], Some(&Type::Int), env, ctx)?;
                if server.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "{name} expects a Server handle argument, got {}",
                            server.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let ty = match name.as_str() {
                    "http_server_local_port" => Type::Int,
                    "http_server_accept" => Type::Int,
                    "http_server_close" => Type::Bool,
                    _ => unreachable!("HTTP server intrinsic checked above"),
                };
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![server],
                    ty,
                });
            }
            if matches!(
                name.as_str(),
                "http_request_method" | "http_request_path" | "http_request_body"
            ) {
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let stream = lower_expr_with_expected(&args[0], Some(&Type::Int), env, ctx)?;
                if stream.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "{name} expects a Request stream handle, got {}",
                            stream.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![stream],
                    ty: Type::String,
                });
            }
            if name == "http_response_write" {
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                if args.len() != 3 {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "http_response_write expects 3 arguments, got {}",
                            args.len()
                        ),
                    )
                    .with_span(*line, *column));
                }
                let stream = lower_expr_with_expected(&args[0], Some(&Type::Int), env, ctx)?;
                let status = lower_expr_with_expected(&args[1], Some(&Type::Int), env, ctx)?;
                let body = lower_expr_with_expected(&args[2], Some(&Type::String), env, ctx)?;
                if stream.ty() != &Type::Int
                    || status.ty() != &Type::Int
                    || body.ty() != &Type::String
                {
                    return Err(Diagnostic::new(
                        "type",
                        "http_response_write expects Request, int, and string arguments",
                    )
                    .with_span(*line, *column));
                }
                move_lowered_value(&body, env)?;
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![stream, status, body],
                    ty: Type::Bool,
                });
            }
            if name == "http_async_serve_route" {
                require_capability(ctx.capabilities, CapabilityKind::Net, name, *line, *column)?;
                require_capability(
                    ctx.capabilities,
                    CapabilityKind::Async,
                    name,
                    *line,
                    *column,
                )?;
                if args.len() != 4 {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "http_async_serve_route expects 4 arguments, got {}",
                            args.len()
                        ),
                    )
                    .with_span(*line, *column));
                }
                let server = lower_expr_with_expected(&args[0], Some(&Type::Int), env, ctx)?;
                let route_path = lower_expr_with_expected(&args[1], Some(&Type::String), env, ctx)?;
                let body = lower_expr_with_expected(&args[2], Some(&Type::String), env, ctx)?;
                let max_requests = lower_expr_with_expected(&args[3], Some(&Type::Int), env, ctx)?;
                if server.ty() != &Type::Int
                    || route_path.ty() != &Type::String
                    || body.ty() != &Type::String
                    || max_requests.ty() != &Type::Int
                {
                    return Err(Diagnostic::new(
                        "type",
                        "http_async_serve_route expects Server, string, string, and int arguments",
                    )
                    .with_span(*line, *column));
                }
                move_lowered_value(&route_path, env)?;
                move_lowered_value(&body, env)?;
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![server, route_path, body, max_requests],
                    ty: Type::Task(Box::new(Type::Bool)),
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
                validate_process_command_allowlist_hir(
                    ctx.capabilities,
                    ctx,
                    &lowered,
                    *line,
                    *column,
                    is_stdlib_process_wrapper(ctx),
                )?;
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
                    span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![lowered],
                    ty: Type::String,
                });
            }
            if name == "crypto_hmac_sha256" || name == "crypto_hmac_sha512" {
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
                        format!("{name} expects 2 arguments, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let key = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if key.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects a string key, got {}", key.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&key, env)?;
                let message = lower_expr_with_expected(&args[1], Some(&Type::String), env, ctx)?;
                if message.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects a string message, got {}", message.ty()),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                move_lowered_value(&message, env)?;
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
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
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![left, right],
                    ty: Type::Bool,
                });
            }
            if name == "crypto_constant_time_eq_u8" {
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
                            "crypto_constant_time_eq_u8 expects 2 arguments, got {}",
                            args.len()
                        ),
                    )
                    .with_span(*line, *column));
                }
                let byte_slice = Type::Slice(Box::new(Type::Numeric(syntax::NumericType::U8)));
                let left = lower_expr_with_expected(&args[0], Some(&byte_slice), env, ctx)?;
                if left.ty() != &byte_slice {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "crypto_constant_time_eq_u8 expects a &[u8] left argument, got {}",
                            left.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let right = lower_expr_with_expected(&args[1], Some(&byte_slice), env, ctx)?;
                if right.ty() != &byte_slice {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "crypto_constant_time_eq_u8 expects a &[u8] right argument, got {}",
                            right.ty()
                        ),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![left, right],
                    ty: Type::Bool,
                });
            }
            if name == "crypto_rand_bytes" {
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
                        format!("crypto_rand_bytes expects 1 argument, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let n = lower_expr(&args[0], env, ctx)?;
                if n.ty() != &Type::Int {
                    return Err(Diagnostic::new(
                        "type",
                        format!("crypto_rand_bytes expects an int length, got {}", n.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![n],
                    ty: Type::Array(Box::new(Type::Numeric(syntax::NumericType::U8)), None),
                });
            }
            if name == "crypto_rand_u64" {
                require_capability(
                    ctx.capabilities,
                    CapabilityKind::Crypto,
                    name,
                    *line,
                    *column,
                )?;
                if !args.is_empty() {
                    return Err(Diagnostic::new(
                        "type",
                        format!("crypto_rand_u64 expects 0 arguments, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: Vec::new(),
                    ty: Type::Numeric(syntax::NumericType::U64),
                });
            }
            if name == "crypto_ed25519_keygen" {
                require_capability(
                    ctx.capabilities,
                    CapabilityKind::Crypto,
                    name,
                    *line,
                    *column,
                )?;
                if !args.is_empty() {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "crypto_ed25519_keygen expects 0 arguments, got {}",
                            args.len()
                        ),
                    )
                    .with_span(*line, *column));
                }
                let bytes = Type::Array(Box::new(Type::Numeric(syntax::NumericType::U8)), None);
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: Vec::new(),
                    ty: Type::Tuple(vec![bytes.clone(), bytes]),
                });
            }
            if name == "crypto_ed25519_sign" || name == "crypto_ed25519_verify" {
                require_capability(
                    ctx.capabilities,
                    CapabilityKind::Crypto,
                    name,
                    *line,
                    *column,
                )?;
                let expected_len = if name == "crypto_ed25519_sign" { 2 } else { 3 };
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
                let byte_slice = Type::Slice(Box::new(Type::Numeric(syntax::NumericType::U8)));
                let first = lower_expr_with_expected(&args[0], Some(&byte_slice), env, ctx)?;
                if first.ty() != &byte_slice {
                    let label = if name == "crypto_ed25519_sign" {
                        "secret_key"
                    } else {
                        "public_key"
                    };
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects a &[u8] {label}, got {}", first.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                let message = lower_expr_with_expected(&args[1], Some(&byte_slice), env, ctx)?;
                if message.ty() != &byte_slice {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects a &[u8] message, got {}", message.ty()),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                if name == "crypto_ed25519_sign" {
                    return Ok(Expr::Call {
                        span: SourceSpan::point(*line, *column),
                        name: name.clone(),
                        args: vec![first, message],
                        ty: Type::Array(Box::new(Type::Numeric(syntax::NumericType::U8)), None),
                    });
                }
                let signature = lower_expr_with_expected(&args[2], Some(&byte_slice), env, ctx)?;
                if signature.ty() != &byte_slice {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects a &[u8] signature, got {}", signature.ty()),
                    )
                    .with_span(args[2].line(), args[2].column()));
                }
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![first, message, signature],
                    ty: Type::Bool,
                });
            }
            if name == "crypto_aead_seal" || name == "crypto_aead_open" {
                require_capability(
                    ctx.capabilities,
                    CapabilityKind::Crypto,
                    name,
                    *line,
                    *column,
                )?;
                if args.len() != 5 {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects 5 arguments, got {}", args.len()),
                    )
                    .with_span(*line, *column));
                }
                let alg = lower_expr_with_expected(&args[0], Some(&Type::String), env, ctx)?;
                if alg.ty() != &Type::String {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects a string algorithm, got {}", alg.ty()),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                move_lowered_value(&alg, env)?;
                let byte_slice = Type::Slice(Box::new(Type::Numeric(syntax::NumericType::U8)));
                let key = lower_expr_with_expected(&args[1], Some(&byte_slice), env, ctx)?;
                if key.ty() != &byte_slice {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects a &[u8] key, got {}", key.ty()),
                    )
                    .with_span(args[1].line(), args[1].column()));
                }
                let nonce = lower_expr_with_expected(&args[2], Some(&byte_slice), env, ctx)?;
                if nonce.ty() != &byte_slice {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects a &[u8] nonce, got {}", nonce.ty()),
                    )
                    .with_span(args[2].line(), args[2].column()));
                }
                let aad = lower_expr_with_expected(&args[3], Some(&byte_slice), env, ctx)?;
                if aad.ty() != &byte_slice {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects a &[u8] aad, got {}", aad.ty()),
                    )
                    .with_span(args[3].line(), args[3].column()));
                }
                let payload = lower_expr_with_expected(&args[4], Some(&byte_slice), env, ctx)?;
                if payload.ty() != &byte_slice {
                    return Err(Diagnostic::new(
                        "type",
                        format!("{name} expects a &[u8] payload, got {}", payload.ty()),
                    )
                    .with_span(args[4].line(), args[4].column()));
                }
                let bytes = Type::Array(Box::new(Type::Numeric(syntax::NumericType::U8)), None);
                let ty = if name == "crypto_aead_open" {
                    Type::Option(Box::new(bytes))
                } else {
                    bytes
                };
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![alg, key, nonce, aad, payload],
                    ty,
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
                    Type::Array(element_ty, _)
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
                if matches!(lowered.ty(), Type::Array(_, _)) && !element_ty.is_copy() {
                    move_lowered_owner_value(&lowered, env)?;
                }
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: name.clone(),
                    args: vec![lowered],
                    ty: element_ty,
                });
            }
            if let Some(binding) = env.get(name) {
                if let Type::Fn(param_tys, return_ty) = binding.ty.clone() {
                    if binding.moved {
                        return Err(ownership_error(
                            OWNERSHIP_USE_AFTER_MOVE,
                            format!("use of moved value {name:?}"),
                        )
                        .with_span_extent(
                            *line,
                            *column,
                            name.chars().count(),
                        ));
                    }
                    if !binding.moved_projections.is_empty() {
                        return Err(ownership_error(
                            OWNERSHIP_USE_AFTER_MOVE,
                            format!("use of partially moved value {name:?}"),
                        )
                        .with_span_extent(
                            *line,
                            *column,
                            name.chars().count(),
                        ));
                    }
                    if args.len() != param_tys.len() {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "function value {name:?} expects {} arguments, got {}",
                                param_tys.len(),
                                args.len()
                            ),
                        )
                        .with_span(*line, *column));
                    }
                    let mut lowered_args = Vec::new();
                    for (arg, expected) in args.iter().zip(param_tys.iter()) {
                        let lowered = lower_expr_with_expected(arg, Some(expected), env, ctx)?;
                        if !type_assignable_to(lowered.ty(), expected) {
                            return Err(Diagnostic::new(
                                "type",
                                format!(
                                    "function value {name:?} expects argument type {expected}, got {}",
                                    lowered.ty()
                                ),
                            )
                            .with_span(arg.line(), arg.column()));
                        }
                        if !expected.is_copy() {
                            move_lowered_value(&lowered, env)?;
                        }
                        lowered_args.push(lowered);
                    }
                    return Ok(Expr::Call {
                        span: SourceSpan::point(*line, *column),
                        name: name.clone(),
                        args: lowered_args,
                        ty: (*return_ty).clone(),
                    });
                }
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
                for (index, (arg, expected)) in args.iter().zip(signature.params.iter()).enumerate()
                {
                    let allow_temporary_string_borrow =
                        !signature.borrow_return_params.contains(&index);
                    let lowered = lower_call_arg_with_expected(
                        arg,
                        Some(expected),
                        env,
                        ctx,
                        allow_temporary_string_borrow,
                    )?;
                    if !type_assignable_to(lowered.ty(), expected) {
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
                validate_stdlib_network_wrapper_call_hir(
                    ctx,
                    ctx.capabilities,
                    env,
                    name,
                    signature,
                    &lowered_args,
                    *line,
                    *column,
                )?;
                release_temporary_borrows(&temporary_borrows, env);
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
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
                    && !type_assignable_to(&inner_ty, expected_inner)
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
                if !type_assignable_to(lowered.ty(), ok_ty.as_ref()) {
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
                if !type_assignable_to(lowered.ty(), err_ty.as_ref()) {
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
                for (index, (arg, expected)) in args.iter().zip(signature.params.iter()).enumerate()
                {
                    let allow_temporary_string_borrow =
                        !signature.borrow_return_params.contains(&index);
                    let lowered = lower_call_arg_with_expected(
                        arg,
                        Some(expected),
                        env,
                        ctx,
                        allow_temporary_string_borrow,
                    )?;
                    if !type_assignable_to(lowered.ty(), expected) {
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
                    span: SourceSpan::point(*line, *column),
                    name: signature.function_name.clone(),
                    args: lowered_args,
                    ty: signature.return_ty.clone(),
                });
            }
            let lowered_base = lower_expr(base, env, ctx)?;
            if let Some(return_ty) = numeric_method_return_ty(lowered_base.ty(), method) {
                if args.len() != 1 {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "numeric method {method:?} expects 1 argument, got {}",
                            args.len()
                        ),
                    )
                    .with_span(*line, *column));
                }
                let receiver_ty = lowered_base.ty().clone();
                let lowered_arg = lower_expr_with_expected(&args[0], Some(&receiver_ty), env, ctx)?;
                if lowered_arg.ty() != &receiver_ty {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "numeric method {method:?} expects argument type {receiver_ty}, got {}",
                            lowered_arg.ty()
                        ),
                    )
                    .with_span(args[0].line(), args[0].column()));
                }
                return Ok(Expr::Call {
                    span: SourceSpan::point(*line, *column),
                    name: format!("__axiom_numeric_{method}"),
                    args: vec![lowered_base, lowered_arg],
                    ty: return_ty,
                });
            }
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
            for (arg_index, (arg, expected)) in
                args.iter().zip(signature.params.iter().skip(1)).enumerate()
            {
                let param_index = arg_index + 1;
                let allow_temporary_string_borrow =
                    !signature.borrow_return_params.contains(&param_index);
                let lowered = lower_call_arg_with_expected(
                    arg,
                    Some(expected),
                    env,
                    ctx,
                    allow_temporary_string_borrow,
                )?;
                if !type_assignable_to(lowered.ty(), expected) {
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
                span: SourceSpan::point(*line, *column),
                name: signature.function_name.clone(),
                args: lowered_args,
                ty: signature.return_ty.clone(),
            })
        }
        syntax::Expr::BinaryAdd { .. } => lower_binary_add_chain(expr, env, ctx),
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
                    if lhs_ty != rhs_ty
                        && !(is_string_like_type(&lhs_ty) && is_string_like_type(&rhs_ty))
                    {
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
                    if lhs_ty != rhs_ty || !is_ordered_numeric(&lhs_ty) {
                        return Err(Diagnostic::new(
                            "type",
                            format!(
                                "operator '{}' expects matching numeric operands, got {lhs_ty} and {rhs_ty}",
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
        syntax::Expr::BinaryLogic {
            op,
            lhs,
            rhs,
            line,
            column,
        } => {
            let lhs = lower_expr_with_expected(lhs, Some(&Type::Bool), env, ctx)?;
            if lhs.ty() != &Type::Bool {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "operator '{}' expects bool operands, got {}",
                        op.lexeme(),
                        lhs.ty()
                    ),
                )
                .with_span(*line, *column));
            }
            let rhs = lower_expr_with_expected(rhs, Some(&Type::Bool), env, ctx)?;
            if rhs.ty() != &Type::Bool {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "operator '{}' expects bool operands, got {}",
                        op.lexeme(),
                        rhs.ty()
                    ),
                )
                .with_span(*line, *column));
            }
            Ok(Expr::BinaryLogic {
                op: lower_logic_op(*op),
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                ty: Type::Bool,
            })
        }
        syntax::Expr::Cast {
            expr,
            ty,
            line,
            column,
        } => {
            let expr = lower_expr(expr, env, ctx)?;
            let target = lower_type(
                ty,
                ctx.structs,
                ctx.enums,
                ctx.aliases,
                ctx.consts,
                *line,
                *column,
            )?;
            if !is_castable_numeric(expr.ty()) || !is_castable_numeric(&target) {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "cast expects numeric source and target types, got {} as {target}",
                        expr.ty()
                    ),
                )
                .with_span(*line, *column));
            }
            if expr.ty() == &target {
                return Ok(expr);
            }
            Ok(Expr::Cast {
                expr: Box::new(expr),
                ty: target,
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
            if !lowered.ty().is_copy()
                && !matches!(lowered, Expr::FieldAccess { .. } | Expr::TupleIndex { .. })
            {
                move_lowered_owner_value(&lowered, env)?;
            }
            Ok(Expr::Try {
                expr: Box::new(lowered),
                ty: result_ty,
            })
        }
        syntax::Expr::Await { expr, line, column } => {
            require_capability(
                ctx.capabilities,
                CapabilityKind::Async,
                "await",
                *line,
                *column,
            )?;
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
            type_args,
            fields,
            line,
            column,
        } => {
            if !type_args.is_empty() {
                return Err(Diagnostic::new(
                    "type",
                    format!("generic constructor {name:?} was not monomorphized"),
                )
                .with_span(*line, *column));
            }
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
                if !type_assignable_to(lowered.ty(), expected) {
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
            let expected_key_value = match expected {
                Some(Type::Map(key, value)) => Some((key.as_ref(), value.as_ref())),
                _ => None,
            };
            let mut lowered_entries = Vec::new();
            let mut key_ty = None;
            let mut value_ty = None;
            let mut temporary_borrows = Vec::new();
            for entry in entries {
                let lowered_key = lower_expr_with_expected(
                    &entry.key,
                    expected_key_value.map(|(key, _)| key),
                    env,
                    ctx,
                )?;
                let lowered_value = lower_expr_with_expected(
                    &entry.value,
                    expected_key_value.map(|(_, value)| value),
                    env,
                    ctx,
                )?;
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
            let expected_element = match expected {
                Some(Type::Array(element_ty, _)) => Some(element_ty.as_ref()),
                _ => None,
            };
            let mut lowered_elements = Vec::new();
            let mut element_ty = None;
            let mut temporary_borrows = Vec::new();
            for element in elements {
                let lowered = lower_expr_with_expected(element, expected_element, env, ctx)?;
                if let Some(expected) = element_ty.as_ref() {
                    if !type_assignable_to(lowered.ty(), expected) {
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
                ty: Type::Array(Box::new(element_ty), Some(elements.len())),
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
                Type::Array(element_ty, _)
                | Type::Slice(element_ty)
                | Type::MutSlice(element_ty) => (*element_ty.clone()).clone(),
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
                (Some(Type::MutSlice(_)), Type::Array(_, _) | Type::MutSlice(_)) => {
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
            let index_expected_ty = match lowered_base.ty() {
                Type::Map(key_ty, _) => Some(key_ty.as_ref()),
                _ => None,
            };
            let lowered_index = lower_expr_with_expected(index, index_expected_ty, env, ctx)?;
            let result_ty = match lowered_base.ty() {
                Type::Array(element_ty, _) => {
                    if lowered_index.ty() != &Type::Int {
                        return Err(Diagnostic::new(
                            "type",
                            format!("array index expects int, got {}", lowered_index.ty()),
                        )
                        .with_span(*line, *column));
                    }
                    if !is_supported_helper_call_array_index_base(&lowered_base) {
                        return Err(Diagnostic::new(
                            "type",
                            "helper-call array indexing currently requires a scalar or bool element type",
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
        syntax::Expr::Closure {
            params,
            body,
            line,
            column,
        } => {
            let Some(Type::Fn(expected_params, expected_return)) = expected else {
                return Err(Diagnostic::new(
                    "type",
                    "closure requires an expected fn type from an annotation or function parameter",
                )
                .with_span(*line, *column));
            };
            if params.len() != expected_params.len() {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "closure expects {} parameters from context, got {}",
                        expected_params.len(),
                        params.len()
                    ),
                )
                .with_span(*line, *column));
            }
            let mut closure_env = env.clone();
            let mut param_names = HashSet::new();
            for ((param, expected_ty), index) in params.iter().zip(expected_params.iter()).zip(0..)
            {
                let param_ty = lower_type(
                    &param.ty,
                    ctx.structs,
                    ctx.enums,
                    ctx.aliases,
                    ctx.consts,
                    param.line,
                    param.column,
                )?;
                if &param_ty != expected_ty {
                    return Err(Diagnostic::new(
                        "type",
                        format!(
                            "closure parameter {} expects type {expected_ty}, got {param_ty}",
                            index + 1
                        ),
                    )
                    .with_span(param.line, param.column));
                }
                if !param_names.insert(param.name.clone()) {
                    return Err(Diagnostic::new(
                        "type",
                        format!("duplicate closure parameter {:?}", param.name),
                    )
                    .with_span(param.line, param.column));
                }
                closure_env.insert(
                    param.name.clone(),
                    Binding {
                        ty: param_ty,
                        moved: false,
                        moved_projections: HashSet::new(),
                        borrow_kind: None,
                        borrow_origin: Some(BorrowOrigin::Local),
                        net_origin: None,
                        borrowed_owners: HashSet::new(),
                        active_borrow_count: 0,
                        active_mut_borrow_count: 0,
                        active_borrows: HashMap::new(),
                    },
                );
            }
            let mut referenced = HashSet::new();
            collect_var_refs(body, &mut referenced);
            for param in params {
                referenced.remove(&param.name);
            }
            let captured_names = referenced.clone();

            if contains_borrowed_slice_type(expected_return, ctx.structs, ctx.enums) {
                return Err(ownership_error(
                    OWNERSHIP_CLOSURE_BORROWED_SLICE_RETURN,
                    "closure fn values cannot return borrowed slice types in stage1 because codegen cannot express the returned reference lifetime",
                )
                .with_span(*line, *column));
            }

            let lowered_body =
                lower_expr_with_expected(body, Some(expected_return), &mut closure_env, ctx)?;
            if lowered_body.ty() != expected_return.as_ref() {
                return Err(Diagnostic::new(
                    "type",
                    format!(
                        "closure body expects type {expected_return}, got {}",
                        lowered_body.ty()
                    ),
                )
                .with_span(body.line(), body.column()));
            }

            if let Some((name, _)) = ownership_projection(&lowered_body)
                && captured_names.contains(name)
                && !lowered_body.ty().is_copy()
            {
                return Err(ownership_error(
                    OWNERSHIP_CLOSURE_MOVE_CAPTURED_NON_COPY,
                    format!(
                        "closure cannot move captured non-copy value `{}` because fn closures must be callable more than once",
                        name
                    ),
                )
                .with_span(*line, *column));
            }

            for name in &captured_names {
                let Some(pre_binding) = env.get(name) else {
                    continue;
                };
                if pre_binding.ty.is_copy() {
                    continue;
                }
                if let Some(post_binding) = closure_env.get(name) {
                    let moved_projection_in_body = post_binding
                        .moved_projections
                        .iter()
                        .any(|projection| !pre_binding.moved_projections.contains(projection));
                    if post_binding.moved || moved_projection_in_body {
                        return Err(ownership_error(
                            OWNERSHIP_CLOSURE_MOVE_CAPTURED_NON_COPY,
                            format!(
                                "closure cannot move captured non-copy value `{}` because fn closures must be callable more than once",
                                name
                            ),
                        )
                        .with_span(*line, *column));
                    }
                }
            }

            for name in referenced {
                if let Some(binding) = env.get_mut(&name)
                    && !binding.ty.is_copy()
                {
                    binding.moved = true;
                }
            }
            Ok(Expr::Closure {
                params: params
                    .iter()
                    .zip(expected_params.iter())
                    .map(|(param, ty)| Param {
                        name: param.name.clone(),
                        ty: ty.clone(),
                    })
                    .collect(),
                body: Box::new(lowered_body),
                ty: Type::Fn(expected_params.clone(), expected_return.clone()),
            })
        }
    }
}

fn collect_var_refs(expr: &syntax::Expr, refs: &mut HashSet<String>) {
    match expr {
        syntax::Expr::VarRef { name, .. } => {
            refs.insert(name.clone());
        }
        syntax::Expr::Call { name, args, .. } => {
            refs.insert(name.clone());
            for arg in args {
                collect_var_refs(arg, refs);
            }
        }
        syntax::Expr::TupleLiteral { elements: args, .. }
        | syntax::Expr::ArrayLiteral { elements: args, .. } => {
            for arg in args {
                collect_var_refs(arg, refs);
            }
        }
        syntax::Expr::MethodCall { base, args, .. } => {
            collect_var_refs(base, refs);
            for arg in args {
                collect_var_refs(arg, refs);
            }
        }
        syntax::Expr::BinaryAdd { lhs, rhs, .. }
        | syntax::Expr::BinaryCompare { lhs, rhs, .. }
        | syntax::Expr::BinaryLogic { lhs, rhs, .. }
        | syntax::Expr::Index {
            base: lhs,
            index: rhs,
            ..
        } => {
            collect_var_refs(lhs, refs);
            collect_var_refs(rhs, refs);
        }
        syntax::Expr::Try { expr, .. }
        | syntax::Expr::Await { expr, .. }
        | syntax::Expr::Cast { expr, .. }
        | syntax::Expr::MutBorrow { expr, .. }
        | syntax::Expr::Deref { expr, .. } => collect_var_refs(expr, refs),
        syntax::Expr::StructLiteral { fields, .. } => {
            for field in fields {
                collect_var_refs(&field.expr, refs);
            }
        }
        syntax::Expr::FieldAccess { base, .. } | syntax::Expr::TupleIndex { base, .. } => {
            collect_var_refs(base, refs)
        }
        syntax::Expr::MapLiteral { entries, .. } => {
            for entry in entries {
                collect_var_refs(&entry.key, refs);
                collect_var_refs(&entry.value, refs);
            }
        }
        syntax::Expr::Slice {
            base, start, end, ..
        } => {
            collect_var_refs(base, refs);
            if let Some(start) = start {
                collect_var_refs(start, refs);
            }
            if let Some(end) = end {
                collect_var_refs(end, refs);
            }
        }
        syntax::Expr::Closure { params, body, .. } => {
            collect_var_refs(body, refs);
            for param in params {
                refs.remove(&param.name);
            }
        }
        syntax::Expr::Match { expr, arms, .. } => {
            collect_var_refs(expr, refs);
            for arm in arms {
                collect_var_refs(&arm.expr, refs);
                for binding in &arm.bindings {
                    refs.remove(binding);
                }
            }
        }
        syntax::Expr::Literal(_) => {}
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
    require_capability(ctx.capabilities, CapabilityKind::Async, name, line, column)?;
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
        ctx.consts,
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
        span: SourceSpan::point(line, column),
        name: name.to_string(),
        args: lowered_args,
        ty,
    })
}

fn lower_map_lookup_intrinsic(
    name: &str,
    type_args: &[syntax::TypeName],
    args: &[syntax::Expr],
    line: usize,
    column: usize,
    env: &mut HashMap<String, Binding>,
    ctx: &LowerContext<'_>,
) -> Result<Expr, Diagnostic> {
    if !type_args.is_empty() && type_args.len() != 2 {
        return Err(Diagnostic::new(
            "type",
            format!(
                "{name} expects 0 or 2 type arguments, got {}",
                type_args.len()
            ),
        )
        .with_span(line, column));
    }
    let expected_args = if matches!(name, "map_keys" | "keys") {
        1
    } else if name == "get_or_default" {
        3
    } else {
        2
    };
    if args.len() != expected_args {
        return Err(Diagnostic::new(
            "type",
            format!(
                "{name} expects {expected_args} arguments, got {}",
                args.len()
            ),
        )
        .with_span(line, column));
    }
    let lowered_map = lower_expr(&args[0], env, ctx)?;
    let Type::Map(key_ty, value_ty) = lowered_map.ty() else {
        return Err(Diagnostic::new(
            "type",
            format!("{name} expects a map value, got {}", lowered_map.ty()),
        )
        .with_span(args[0].line(), args[0].column()));
    };
    let key_ty = (*key_ty.clone()).clone();
    let value_ty = (*value_ty.clone()).clone();
    if let [expected_key, expected_value] = type_args {
        let expected_key = lower_type(
            expected_key,
            ctx.structs,
            ctx.enums,
            ctx.aliases,
            ctx.consts,
            line,
            column,
        )?;
        let expected_value = lower_type(
            expected_value,
            ctx.structs,
            ctx.enums,
            ctx.aliases,
            ctx.consts,
            line,
            column,
        )?;
        if expected_key != key_ty || expected_value != value_ty {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "{name} type arguments expect {{{expected_key}: {expected_value}}}, got {}",
                    lowered_map.ty()
                ),
            )
            .with_span(line, column));
        }
    }
    if matches!(name, "map_keys" | "keys") {
        move_lowered_value(&lowered_map, env)?;
        return Ok(Expr::Call {
            span: SourceSpan::point(line, column),
            name: name.to_string(),
            args: vec![lowered_map],
            ty: Type::Array(Box::new(key_ty), None),
        });
    }
    let lowered_key = lower_expr_with_expected(&args[1], Some(&key_ty), env, ctx)?;
    if lowered_key.ty() != &key_ty {
        return Err(Diagnostic::new(
            "type",
            format!("{name} expects key type {key_ty}, got {}", lowered_key.ty()),
        )
        .with_span(args[1].line(), args[1].column()));
    }
    let mut lowered_args = vec![lowered_map, lowered_key];
    if name == "get_or_default" {
        let lowered_default = lower_expr_with_expected(&args[2], Some(&value_ty), env, ctx)?;
        if lowered_default.ty() != &value_ty {
            return Err(Diagnostic::new(
                "type",
                format!(
                    "{name} expects default type {value_ty}, got {}",
                    lowered_default.ty()
                ),
            )
            .with_span(args[2].line(), args[2].column()));
        }
        move_lowered_value(&lowered_default, env)?;
        lowered_args.push(lowered_default);
    }
    move_lowered_value(&lowered_args[0], env)?;
    move_lowered_value(&lowered_args[1], env)?;
    Ok(Expr::Call {
        span: SourceSpan::point(line, column),
        name: name.to_string(),
        args: lowered_args,
        ty: match name {
            "map_get" | "get" => Type::Option(Box::new(value_ty)),
            "get_or_default" => value_ty,
            _ => Type::Bool,
        },
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
                .with_span_extent(*line, *column, name.chars().count()));
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
        let lowered = lower_call_arg_with_expected(arg, Some(expected), env, ctx, false)?;
        if !type_assignable_to(lowered.ty(), expected) {
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
        if !type_assignable_to(lowered.ty(), expected) {
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

#[cfg(test)]
#[path = "../tests/hir_unit.rs"]
mod boundary_tests;
