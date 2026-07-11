use super::*;
use crate::manifest::KNOWN_CAPABILITIES;

const MAX_CALL_DEPTH: usize = 64;

/// Proves the narrow subset for which compile-time output materialization is
/// permitted. Unknown constructs fail closed; this function must run before
/// `collect_output_program` so rejected programs cannot execute in the build
/// process.
pub(super) fn allows_static_output_evaluation(
    program: &Program,
    capabilities: &CapabilityConfig,
) -> bool {
    if KNOWN_CAPABILITIES
        .iter()
        .copied()
        .any(|kind| capabilities.enabled(kind))
    {
        return false;
    }

    let functions = program
        .functions
        .iter()
        .map(|function| (function.name.as_str(), function))
        .collect::<HashMap<_, _>>();
    let mut stack = Vec::new();
    program
        .statics
        .iter()
        .all(|static_def| pure_expr(&static_def.expr, &functions, &mut stack))
        && pure_stmts(&program.stmts, &functions, &mut stack)
}

fn pure_stmts(
    stmts: &[Stmt],
    functions: &HashMap<&str, &Function>,
    stack: &mut Vec<String>,
) -> bool {
    stmts.iter().all(|stmt| match stmt {
        Stmt::Let { expr, .. }
        | Stmt::Assign { expr, .. }
        | Stmt::Print { expr, .. }
        | Stmt::Panic { message: expr, .. }
        | Stmt::Defer { expr, .. }
        | Stmt::Return { expr, .. } => pure_expr(expr, functions, stack),
        Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            pure_expr(cond, functions, stack)
                && pure_stmts(then_block, functions, stack)
                && else_block
                    .as_deref()
                    .is_none_or(|block| pure_stmts(block, functions, stack))
        }
        // Even a literal-false loop is rejected. The contract is structural,
        // not dependent on executing the evaluator to discover termination.
        Stmt::While { .. } => false,
        Stmt::Match { expr, arms, .. } => {
            pure_expr(expr, functions, stack)
                && arms
                    .iter()
                    .all(|arm| pure_stmts(&arm.body, functions, stack))
        }
    })
}

fn pure_expr(expr: &Expr, functions: &HashMap<&str, &Function>, stack: &mut Vec<String>) -> bool {
    match expr {
        Expr::Literal(_) | Expr::VarRef { .. } => true,
        Expr::Call { name, args, .. } => {
            args.iter().all(|arg| pure_expr(arg, functions, stack))
                && (pure_intrinsic(name) || pure_function(name, functions, stack))
        }
        Expr::BinaryAdd { lhs, rhs, .. }
        | Expr::BinaryCompare { lhs, rhs, .. }
        | Expr::BinaryLogic { lhs, rhs, .. } => {
            pure_expr(lhs, functions, stack) && pure_expr(rhs, functions, stack)
        }
        Expr::Cast { expr, .. }
        | Expr::MutBorrow { expr, .. }
        | Expr::Deref { expr, .. }
        | Expr::Try { expr, .. }
        | Expr::FieldAccess { base: expr, .. }
        | Expr::TupleIndex { base: expr, .. }
        | Expr::StringBorrow { expr, .. } => pure_expr(expr, functions, stack),
        Expr::Await { .. } | Expr::Closure { .. } => false,
        Expr::StructLiteral { fields, .. } => fields
            .iter()
            .all(|field| pure_expr(&field.expr, functions, stack)),
        Expr::TupleLiteral { elements, .. } | Expr::ArrayLiteral { elements, .. } => elements
            .iter()
            .all(|element| pure_expr(element, functions, stack)),
        Expr::MapLiteral { entries, .. } => entries.iter().all(|entry| {
            pure_expr(&entry.key, functions, stack) && pure_expr(&entry.value, functions, stack)
        }),
        Expr::EnumVariant { payloads, .. } => payloads
            .iter()
            .all(|payload| pure_expr(payload, functions, stack)),
        Expr::Slice {
            base, start, end, ..
        } => {
            pure_expr(base, functions, stack)
                && start
                    .as_deref()
                    .is_none_or(|expr| pure_expr(expr, functions, stack))
                && end
                    .as_deref()
                    .is_none_or(|expr| pure_expr(expr, functions, stack))
        }
        Expr::Index { base, index, .. } => {
            pure_expr(base, functions, stack) && pure_expr(index, functions, stack)
        }
        Expr::Match { expr, arms, .. } => {
            pure_expr(expr, functions, stack)
                && arms
                    .iter()
                    .all(|arm| pure_expr(&arm.expr, functions, stack))
        }
    }
}

fn pure_function(
    name: &str,
    functions: &HashMap<&str, &Function>,
    stack: &mut Vec<String>,
) -> bool {
    let Some(function) = functions.get(name).copied() else {
        return false;
    };
    if function.is_async
        || function.is_extern
        || stack.len() >= MAX_CALL_DEPTH
        || stack.iter().any(|active| active == name)
    {
        return false;
    }
    stack.push(name.to_string());
    let pure = pure_stmts(&function.body, functions, stack);
    stack.pop();
    pure
}

fn pure_intrinsic(name: &str) -> bool {
    matches!(
        name,
        "assert"
            | "len"
            | "first"
            | "last"
            | "contains"
            | "get"
            | "get_or_default"
            | "keys"
            | "map_contains_key"
            | "map_get"
            | "map_keys"
            | "string_clone"
            | "string_trim"
            | "string_trim_start"
            | "string_strip_prefix"
            | "string_strip_suffix"
            | "string_line_at"
            | "string_byte_at"
            | "regex_is_match"
            | "regex_find"
            | "regex_replace_all"
            | "encoding_url_component_encode"
            | "encoding_url_component_decode"
            | "encoding_path_segment_encode"
            | "encoding_url_query_pair_encode"
            | "encoding_path_join_segment"
            | "json_parse_int"
            | "json_parse_bool"
            | "json_parse_string"
            | "json_parse_field_int"
            | "json_parse_field_bool"
            | "json_parse_field_string"
            | "json_stringify_int"
            | "json_stringify_bool"
            | "json_stringify_string"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_program(stmts: Vec<Stmt>) -> Program {
        Program {
            path: "test.ax".into(),
            structs: Vec::new(),
            enums: Vec::new(),
            statics: Vec::new(),
            functions: Vec::new(),
            stmts,
        }
    }

    #[test]
    fn permits_literal_output_without_capabilities() {
        let program = empty_program(vec![Stmt::Print {
            expr: Expr::Literal(LiteralValue::String("hello".into())),
            span: crate::mir::SourceSpan { line: 1, column: 1 },
        }]);
        assert!(allows_static_output_evaluation(
            &program,
            &CapabilityConfig::default()
        ));
    }

    #[test]
    fn rejects_enabled_host_capability_before_call_inspection() {
        let mut capabilities = CapabilityConfig::default();
        capabilities.net = true;
        assert!(!allows_static_output_evaluation(
            &empty_program(Vec::new()),
            &capabilities
        ));
    }

    #[test]
    fn rejects_unknown_calls_and_unbounded_loops() {
        let span = crate::mir::SourceSpan { line: 1, column: 1 };
        let unknown = empty_program(vec![Stmt::Print {
            expr: Expr::Call {
                name: "ambient_input".into(),
                args: Vec::new(),
                ty: Type::String,
            },
            span,
        }]);
        assert!(!allows_static_output_evaluation(
            &unknown,
            &CapabilityConfig::default()
        ));
        let looped = empty_program(vec![Stmt::While {
            cond: Expr::Literal(LiteralValue::Bool(false)),
            body: Vec::new(),
            span,
        }]);
        assert!(!allows_static_output_evaluation(
            &looped,
            &CapabilityConfig::default()
        ));
    }

    #[test]
    fn compilation_mode_reports_known_string_fold() {
        let span = crate::mir::SourceSpan { line: 1, column: 1 };
        let folded = empty_program(vec![Stmt::Print {
            expr: Expr::Call {
                name: "json_stringify_string".into(),
                args: vec![Expr::Literal(LiteralValue::String("known".into()))],
                ty: Type::String,
            },
            span,
        }]);
        assert_eq!(
            direct_native_mode(&folded),
            CraneliftCompilationMode::DirectNativeRuntimeWithStaticFolds
        );

        let runtime_only = empty_program(vec![Stmt::Return {
            expr: Expr::VarRef {
                name: "dynamic_cli_scalar".into(),
                ty: Type::Int,
            },
            span,
        }]);
        assert_eq!(
            direct_native_mode(&runtime_only),
            CraneliftCompilationMode::DirectNativeRuntime
        );
    }
}
