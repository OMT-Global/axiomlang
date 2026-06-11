use crate::diagnostics::Diagnostic;
use crate::mir::{
    ArithmeticOp, CompareOp, Expr, Function, LiteralValue, MatchArm, MatchExprArm, Program, Stmt,
    Type,
};
use crate::syntax::NumericType;
use axiomc_backend_cranelift::OutputLine;
use std::collections::HashMap;
use std::env;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

const SPIKE_FS_ROOT_BINDING: &str = "$axiom_fs_root";
const SPIKE_MAX_FS_READ_BYTES: u64 = 64 * 1024 * 1024;
const SPIKE_MAX_FS_WRITE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
enum SpikeValue {
    Int(i64),
    UInt(u64),
    Float(f64),
    Bool(bool),
    Text(String),
    Struct {
        name: String,
        fields: Vec<(String, SpikeValue)>,
    },
    Enum {
        enum_name: String,
        variant: String,
        field_names: Vec<String>,
        payloads: Vec<SpikeValue>,
    },
    Tuple(Vec<SpikeValue>),
    Map(Vec<(SpikeValue, SpikeValue)>),
    Array(Vec<SpikeValue>),
}

type SpikeEnv = HashMap<String, SpikeValue>;

#[derive(Clone, Debug, PartialEq, Eq)]
enum RegexAtom {
    Literal(char),
    Any,
    Class {
        ranges: Vec<(char, char)>,
        negated: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RegexQuantifier {
    One,
    ZeroOrOne,
    ZeroOrMore,
    OneOrMore,
}

#[derive(Clone, Debug)]
struct RegexToken {
    atom: RegexAtom,
    quantifier: RegexQuantifier,
}

#[derive(Clone, Debug)]
struct RegexProgram {
    tokens: Vec<RegexToken>,
    start_anchor: bool,
    end_anchor: bool,
}

pub fn compile_cranelift_hello_spike(
    program: &Program,
    package_root: &Path,
    fs_root: &Path,
    object_path: &Path,
    binary_path: &Path,
    target: Option<&str>,
    _debug: bool,
) -> Result<(), Diagnostic> {
    if target.is_some() {
        return Err(unsupported(
            "the cranelift backend spike currently supports only the host target",
        ));
    }
    let lines = collect_output_lines(program, package_root, fs_root)?;
    axiomc_backend_cranelift::compile_output_lines(&lines, object_path, binary_path).map_err(
        |err| {
            Diagnostic::new("build", err.to_string()).with_path(object_path.display().to_string())
        },
    )
}

fn collect_output_lines(
    program: &Program,
    _package_root: &Path,
    fs_root: &Path,
) -> Result<Vec<OutputLine>, Diagnostic> {
    let functions = program
        .functions
        .iter()
        .map(|function| (function.name.as_str(), function))
        .collect::<HashMap<_, _>>();
    let mut env = SpikeEnv::new();
    env.insert(
        SPIKE_FS_ROOT_BINDING.to_string(),
        SpikeValue::Text(fs_root.display().to_string()),
    );
    let mut lines = Vec::new();
    for static_def in &program.statics {
        let value = eval_expr(&static_def.expr, &functions, &env, &mut lines)?;
        env.insert(static_def.name.clone(), value);
    }
    eval_block(&program.stmts, &functions, &mut env, &mut lines)?;
    Ok(lines)
}

fn eval_block(
    stmts: &[Stmt],
    functions: &HashMap<&str, &Function>,
    env: &mut SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<Option<SpikeValue>, Diagnostic> {
    for stmt in stmts {
        if let Some(value) = eval_stmt(stmt, functions, env, lines)? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

fn eval_stmt(
    stmt: &Stmt,
    functions: &HashMap<&str, &Function>,
    env: &mut SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<Option<SpikeValue>, Diagnostic> {
    match stmt {
        Stmt::Let { name, expr, .. } => {
            let value = eval_expr(expr, functions, env, lines)?;
            env.insert(name.clone(), value);
            Ok(None)
        }
        Stmt::Print { expr, .. } => {
            let value = eval_expr(expr, functions, env, lines)?;
            lines.push(OutputLine::stdout(render_value(&value)));
            Ok(None)
        }
        Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            let branch = match eval_expr(cond, functions, env, lines)? {
                SpikeValue::Bool(true) => Some(then_block.as_slice()),
                SpikeValue::Bool(false) => else_block.as_deref(),
                _ => return Err(unsupported("if conditions must be boolean")),
            };
            if let Some(branch) = branch {
                eval_block(branch, functions, env, lines)
            } else {
                Ok(None)
            }
        }
        Stmt::While { cond, .. } => match eval_expr(cond, functions, env, lines)? {
            SpikeValue::Bool(false) => Ok(None),
            SpikeValue::Bool(true) => Err(unsupported(
                "runtime loops are not part of the cranelift hello spike",
            )),
            _ => Err(unsupported("while conditions must be boolean")),
        },
        Stmt::Match { expr, arms, .. } => eval_match_stmt(expr, arms, functions, env, lines),
        Stmt::Return { expr, .. } => Ok(Some(eval_expr(expr, functions, env, lines)?)),
        Stmt::Assign { .. } | Stmt::Panic { .. } | Stmt::Defer { .. } => Err(unsupported(
            "only let, print, if, while false, match, and return statements are supported by the cranelift hello spike",
        )),
    }
}

fn eval_expr(
    expr: &Expr,
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match expr {
        Expr::Literal(LiteralValue::Int(value)) => Ok(SpikeValue::Int(*value)),
        Expr::Literal(LiteralValue::Numeric { raw, ty }) => eval_numeric_literal(raw, *ty),
        Expr::Literal(LiteralValue::Bool(value)) => Ok(SpikeValue::Bool(*value)),
        Expr::Literal(LiteralValue::String(value)) | Expr::Literal(LiteralValue::Str(value)) => {
            Ok(SpikeValue::Text(value.clone()))
        }
        Expr::VarRef { name, .. } => env
            .get(name)
            .cloned()
            .ok_or_else(|| unsupported(&format!("unknown cranelift spike variable {name:?}"))),
        Expr::Call { name, args, .. } => eval_call(name, args, functions, env, lines),
        Expr::BinaryAdd { op, lhs, rhs, ty } => {
            eval_arithmetic(*op, lhs, rhs, ty, functions, env, lines)
        }
        Expr::BinaryCompare {
            op,
            lhs,
            rhs,
            ty: _,
        } => eval_compare(*op, lhs, rhs, functions, env, lines),
        Expr::BinaryLogic { op, lhs, rhs, .. } => {
            let left = expect_bool(eval_expr(lhs, functions, env, lines)?)?;
            match op {
                crate::mir::LogicOp::And if !left => Ok(SpikeValue::Bool(false)),
                crate::mir::LogicOp::Or if left => Ok(SpikeValue::Bool(true)),
                crate::mir::LogicOp::And | crate::mir::LogicOp::Or => Ok(SpikeValue::Bool(
                    expect_bool(eval_expr(rhs, functions, env, lines)?)?,
                )),
            }
        }
        Expr::Cast { expr, ty } => cast_spike_value(eval_expr(expr, functions, env, lines)?, ty),
        Expr::StructLiteral { name, fields, .. } => fields
            .iter()
            .map(|field| {
                Ok((
                    field.name.clone(),
                    eval_expr(&field.expr, functions, env, lines)?,
                ))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|fields| SpikeValue::Struct {
                name: name.clone(),
                fields,
            }),
        Expr::FieldAccess { base, field, .. } => match eval_expr(base, functions, env, lines)? {
            SpikeValue::Struct { name, fields } => fields
                .into_iter()
                .find_map(|(candidate, value)| (candidate == *field).then_some(value))
                .ok_or_else(|| {
                    unsupported(&format!(
                        "struct {name:?} has no field {field:?} in the cranelift spike"
                    ))
                }),
            _ => Err(unsupported("field access requires a struct value")),
        },
        Expr::EnumVariant {
            enum_name,
            variant,
            field_names,
            payloads,
            ..
        } => payloads
            .iter()
            .map(|payload| eval_expr(payload, functions, env, lines))
            .collect::<Result<Vec<_>, _>>()
            .map(|payloads| SpikeValue::Enum {
                enum_name: enum_name.clone(),
                variant: variant.clone(),
                field_names: field_names.clone(),
                payloads,
            }),
        Expr::Match { expr, arms, .. } => eval_match_expr(expr, arms, functions, env, lines),
        Expr::TupleLiteral { elements, .. } => elements
            .iter()
            .map(|element| eval_expr(element, functions, env, lines))
            .collect::<Result<Vec<_>, _>>()
            .map(SpikeValue::Tuple),
        Expr::TupleIndex { base, index, .. } => match eval_expr(base, functions, env, lines)? {
            SpikeValue::Tuple(elements) => elements
                .get(*index)
                .cloned()
                .ok_or_else(|| unsupported("tuple index is outside the tuple width")),
            _ => Err(unsupported("tuple indexing requires a tuple value")),
        },
        Expr::MapLiteral { entries, .. } => {
            let mut values = Vec::new();
            for entry in entries {
                let key = eval_expr(&entry.key, functions, env, lines)?;
                validate_map_key(&key)?;
                let value = eval_expr(&entry.value, functions, env, lines)?;
                insert_map_entry(&mut values, key, value)?;
            }
            Ok(SpikeValue::Map(values))
        }
        Expr::ArrayLiteral { elements, .. } => elements
            .iter()
            .map(|element| eval_expr(element, functions, env, lines))
            .collect::<Result<Vec<_>, _>>()
            .map(SpikeValue::Array),
        Expr::Slice {
            base, start, end, ..
        } => {
            let elements = match eval_expr(base, functions, env, lines)? {
                SpikeValue::Array(elements) => elements,
                _ => {
                    return Err(unsupported(
                        "slicing supports arrays in the cranelift spike",
                    ));
                }
            };
            let start = match start {
                Some(expr) => expect_non_negative_index(eval_expr(expr, functions, env, lines)?)?,
                None => 0,
            };
            let end = match end {
                Some(expr) => expect_non_negative_index(eval_expr(expr, functions, env, lines)?)?,
                None => elements.len(),
            };
            if start > end || end > elements.len() {
                return Err(unsupported("slice range is outside the array length"));
            }
            Ok(SpikeValue::Array(elements[start..end].to_vec()))
        }
        Expr::Index { base, index, .. } => match eval_expr(base, functions, env, lines)? {
            SpikeValue::Array(elements) => {
                let index = expect_non_negative_index(eval_expr(index, functions, env, lines)?)?;
                elements
                    .get(index)
                    .cloned()
                    .ok_or_else(|| unsupported("array index is outside the array length"))
            }
            SpikeValue::Map(entries) => {
                let key = eval_expr(index, functions, env, lines)?;
                validate_map_key(&key)?;
                for (candidate, value) in entries {
                    if map_keys_equal(&candidate, &key)? {
                        return Ok(value);
                    }
                }
                Err(unsupported("map key not found"))
            }
            _ => Err(unsupported("indexing requires an array or map value")),
        },
        _ => Err(unsupported(
            "this expression is outside the cranelift hello spike subset",
        )),
    }
}

fn eval_match_stmt(
    expr: &Expr,
    arms: &[MatchArm],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<Option<SpikeValue>, Diagnostic> {
    let matched = expect_enum_value(eval_expr(expr, functions, env, lines)?)?;
    let arm = arms
        .iter()
        .find(|arm| arm.enum_name == matched.enum_name && arm.variant == matched.variant)
        .ok_or_else(|| unsupported("match statement has no matching enum arm"))?;
    let mut arm_env = env.clone();
    if !arm.ignore_payloads {
        bind_match_payloads(
            &mut arm_env,
            &arm.bindings,
            arm.is_named,
            &matched.field_names,
            &matched.payloads,
        )?;
    }
    eval_block(&arm.body, functions, &mut arm_env, lines)
}

fn eval_match_expr(
    expr: &Expr,
    arms: &[MatchExprArm],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let matched = expect_enum_value(eval_expr(expr, functions, env, lines)?)?;
    let arm = arms
        .iter()
        .find(|arm| arm.enum_name == matched.enum_name && arm.variant == matched.variant)
        .ok_or_else(|| unsupported("match expression has no matching enum arm"))?;
    let mut arm_env = env.clone();
    bind_match_payloads(
        &mut arm_env,
        &arm.bindings,
        arm.is_named,
        &matched.field_names,
        &matched.payloads,
    )?;
    eval_expr(&arm.expr, functions, &arm_env, lines)
}

struct MatchedEnum {
    enum_name: String,
    variant: String,
    field_names: Vec<String>,
    payloads: Vec<SpikeValue>,
}

fn expect_enum_value(value: SpikeValue) -> Result<MatchedEnum, Diagnostic> {
    match value {
        SpikeValue::Enum {
            enum_name,
            variant,
            field_names,
            payloads,
        } => Ok(MatchedEnum {
            enum_name,
            variant,
            field_names,
            payloads,
        }),
        _ => Err(unsupported("match requires an enum value")),
    }
}

fn bind_match_payloads(
    env: &mut SpikeEnv,
    bindings: &[String],
    is_named: bool,
    field_names: &[String],
    payloads: &[SpikeValue],
) -> Result<(), Diagnostic> {
    if bindings.len() != payloads.len() {
        return Err(unsupported("match payload binding count mismatch"));
    }
    for (index, binding) in bindings.iter().enumerate() {
        if binding == "_" {
            continue;
        }
        let payload_index = if is_named {
            field_names
                .iter()
                .position(|field_name| field_name == binding)
                .ok_or_else(|| unsupported("named enum match binding has no payload field"))?
        } else {
            index
        };
        let value = payloads
            .get(payload_index)
            .ok_or_else(|| unsupported("match payload binding index mismatch"))?;
        env.insert(binding.clone(), value.clone());
    }
    Ok(())
}

fn eval_numeric_literal(raw: &str, ty: NumericType) -> Result<SpikeValue, Diagnostic> {
    match ty {
        NumericType::F32 => raw
            .parse::<f32>()
            .map(|value| SpikeValue::Float(value as f64))
            .map_err(|_| unsupported("invalid f32 numeric literal")),
        NumericType::F64 => raw
            .parse::<f64>()
            .map(SpikeValue::Float)
            .map_err(|_| unsupported("invalid f64 numeric literal")),
        NumericType::I8
        | NumericType::I16
        | NumericType::I32
        | NumericType::I64
        | NumericType::Isize => raw
            .parse::<i64>()
            .map(|value| cast_signed_integer(value, ty))
            .map_err(|_| unsupported("invalid signed integer numeric literal")),
        NumericType::U8
        | NumericType::U16
        | NumericType::U32
        | NumericType::U64
        | NumericType::Usize => raw
            .parse::<u128>()
            .map_err(|_| unsupported("invalid unsigned integer numeric literal"))
            .and_then(|value| {
                u64::try_from(value)
                    .map(|value| cast_unsigned_integer(value, ty))
                    .map_err(|_| unsupported("invalid unsigned integer numeric literal"))
            }),
    }
}

fn cast_spike_value(value: SpikeValue, ty: &Type) -> Result<SpikeValue, Diagnostic> {
    match ty {
        Type::Int => match value {
            SpikeValue::Int(value) => Ok(SpikeValue::Int(value)),
            SpikeValue::UInt(value) => Ok(SpikeValue::UInt(value)),
            SpikeValue::Float(value) => Ok(SpikeValue::Int(value as i64)),
            _ => Err(unsupported("only numeric values can be cast to int")),
        },
        Type::Numeric(numeric_ty) => cast_to_numeric(value, *numeric_ty),
        _ => Ok(value),
    }
}

fn cast_to_integer_like(value: SpikeValue, ty: NumericType) -> Result<SpikeValue, Diagnostic> {
    match value {
        SpikeValue::Int(value) => Ok(cast_signed_integer(value, ty)),
        SpikeValue::UInt(value) => Ok(cast_unsigned_integer(value, ty)),
        SpikeValue::Float(value) => Ok(cast_float(value, ty)),
        _ => Err(unsupported("only numeric values can be cast to int")),
    }
}

fn cast_to_numeric(value: SpikeValue, ty: NumericType) -> Result<SpikeValue, Diagnostic> {
    match value {
        SpikeValue::Int(value) => Ok(cast_signed_integer(value, ty)),
        SpikeValue::UInt(value) => Ok(cast_unsigned_integer(value, ty)),
        SpikeValue::Float(value) => Ok(cast_float(value, ty)),
        _ => Err(unsupported(
            "only numeric values can be cast to numeric types",
        )),
    }
}

fn cast_signed_integer(value: i64, ty: NumericType) -> SpikeValue {
    match ty {
        NumericType::I8 => SpikeValue::Int(value as i8 as i64),
        NumericType::I16 => SpikeValue::Int(value as i16 as i64),
        NumericType::I32 => SpikeValue::Int(value as i32 as i64),
        NumericType::I64 => SpikeValue::Int(value),
        NumericType::Isize => SpikeValue::Int(value as isize as i64),
        NumericType::U8 => SpikeValue::UInt(value as u8 as u64),
        NumericType::U16 => SpikeValue::UInt(value as u16 as u64),
        NumericType::U32 => SpikeValue::UInt(value as u32 as u64),
        NumericType::U64 => SpikeValue::UInt(value as u64),
        NumericType::Usize => SpikeValue::UInt(value as usize as u64),
        NumericType::F32 => SpikeValue::Float((value as f32) as f64),
        NumericType::F64 => SpikeValue::Float(value as f64),
    }
}

fn cast_unsigned_integer(value: u64, ty: NumericType) -> SpikeValue {
    match ty {
        NumericType::I8 => SpikeValue::Int(value as i8 as i64),
        NumericType::I16 => SpikeValue::Int(value as i16 as i64),
        NumericType::I32 => SpikeValue::Int(value as i32 as i64),
        NumericType::I64 => SpikeValue::Int(value as i64),
        NumericType::Isize => SpikeValue::Int(value as isize as i64),
        NumericType::U8 => SpikeValue::UInt(value as u8 as u64),
        NumericType::U16 => SpikeValue::UInt(value as u16 as u64),
        NumericType::U32 => SpikeValue::UInt(value as u32 as u64),
        NumericType::U64 => SpikeValue::UInt(value),
        NumericType::Usize => SpikeValue::UInt(value as usize as u64),
        NumericType::F32 => SpikeValue::Float((value as f32) as f64),
        NumericType::F64 => SpikeValue::Float(value as f64),
    }
}

fn cast_float(value: f64, ty: NumericType) -> SpikeValue {
    match ty {
        NumericType::I8 => SpikeValue::Int(value as i8 as i64),
        NumericType::I16 => SpikeValue::Int(value as i16 as i64),
        NumericType::I32 => SpikeValue::Int(value as i32 as i64),
        NumericType::I64 => SpikeValue::Int(value as i64),
        NumericType::Isize => SpikeValue::Int(value as isize as i64),
        NumericType::U8 => SpikeValue::UInt(value as u8 as u64),
        NumericType::U16 => SpikeValue::UInt(value as u16 as u64),
        NumericType::U32 => SpikeValue::UInt(value as u32 as u64),
        NumericType::U64 => SpikeValue::UInt(value as u64),
        NumericType::Usize => SpikeValue::UInt(value as usize as u64),
        NumericType::F32 => SpikeValue::Float((value as f32) as f64),
        NumericType::F64 => SpikeValue::Float(value),
    }
}

fn eval_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    if name == "len" {
        return eval_len_call(args, functions, env, lines);
    }
    if name == "first" || name == "last" {
        return eval_first_last_call(name, args, functions, env, lines);
    }
    if name == "contains" || name == "map_contains_key" {
        return eval_map_contains_call(args, functions, env, lines);
    }
    if name == "io_eprintln" {
        return eval_io_eprintln_call(args, functions, env, lines);
    }
    if is_json_call(name) {
        return eval_json_call(name, args, functions, env, lines);
    }
    if is_crypto_call(name) {
        return eval_crypto_call(name, args, functions, env, lines);
    }
    if name == "env_get" {
        return eval_env_get_call(args, functions, env, lines);
    }
    if name == "fs_read" {
        return eval_fs_read_call(args, functions, env, lines);
    }
    if is_fs_write_call(name) {
        return eval_fs_write_call(name, args, functions, env, lines);
    }
    if name == "process_status" {
        return eval_process_status_call(args, functions, env, lines);
    }
    if name == "clock_now_ms" {
        return eval_clock_now_ms_call(args);
    }
    if name == "clock_elapsed_ms" {
        return eval_clock_elapsed_ms_call(args, functions, env, lines);
    }
    if name == "clock_sleep_ms" {
        return eval_clock_sleep_ms_call(args, functions, env, lines);
    }
    if is_regex_call(name) {
        return eval_regex_call(name, args, functions, env, lines);
    }
    let function = functions
        .get(name)
        .ok_or_else(|| unsupported(&format!("unsupported cranelift spike call {name:?}")))?;
    if function.params.len() != args.len() {
        return Err(unsupported("function argument count mismatch"));
    }
    let mut local_env = env.clone();
    for (param, arg) in function.params.iter().zip(args) {
        local_env.insert(param.name.clone(), eval_expr(arg, functions, env, lines)?);
    }
    let returned = eval_block(&function.body, functions, &mut local_env, lines)?;
    returned.ok_or_else(|| unsupported("cranelift spike functions must return a value"))
}

fn eval_len_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [arg] = args else {
        return Err(unsupported("len expects exactly one argument"));
    };
    let value = eval_expr(arg, functions, env, lines)?;
    let len = match value {
        // Match the generated-Rust backend, which lowers `len(...)` to Rust
        // `.len()` (encoded byte length). Using char count here would diverge
        // for non-ASCII strings (e.g. `len("é")` is 2, not 1).
        SpikeValue::Text(value) => value.len(),
        SpikeValue::Tuple(values) | SpikeValue::Array(values) => values.len(),
        _ => return Err(unsupported("len supports strings, tuples, and arrays")),
    };
    Ok(SpikeValue::Int(len as i64))
}

fn eval_first_last_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [arg] = args else {
        return Err(unsupported(&format!("{name} expects exactly one argument")));
    };
    // HIR restricts `first`/`last` to arrays and slices and returns the element
    // directly (it panics at runtime on an empty collection). The spike models
    // owned arrays and evaluated array slices with the same value shape.
    let elements = match eval_expr(arg, functions, env, lines)? {
        SpikeValue::Array(elements) => elements,
        _ => {
            return Err(unsupported(&format!(
                "{name} supports arrays in the cranelift spike"
            )));
        }
    };
    let selected = if name == "first" {
        elements.first()
    } else {
        elements.last()
    };
    selected
        .cloned()
        .ok_or_else(|| unsupported(&format!("{name} on an empty array")))
}

fn eval_map_contains_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [map, key] = args else {
        return Err(unsupported("map contains expects exactly two arguments"));
    };
    let entries = match eval_expr(map, functions, env, lines)? {
        SpikeValue::Map(entries) => entries,
        _ => return Err(unsupported("map contains expects a map value")),
    };
    let key = eval_expr(key, functions, env, lines)?;
    validate_map_key(&key)?;
    let contains = entries.iter().try_fold(false, |found, (candidate, _)| {
        Ok::<_, Diagnostic>(found || map_keys_equal(candidate, &key)?)
    })?;
    Ok(SpikeValue::Bool(contains))
}

fn is_json_call(name: &str) -> bool {
    matches!(
        name,
        "json_parse_int"
            | "json_parse_bool"
            | "json_parse_string"
            | "json_parse_value"
            | "json_parse_field_int"
            | "json_parse_field_bool"
            | "json_parse_field_string"
            | "json_parse_field_value"
            | "json_stringify_int"
            | "json_stringify_bool"
            | "json_stringify_string"
            | "json_stringify_value"
    )
}

fn eval_json_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "json_parse_int" => {
            let text = eval_json_unary_text(name, args, functions, env, lines)?;
            Ok(spike_option(json_parse_int(&text).map(SpikeValue::Int)))
        }
        "json_parse_bool" => {
            let text = eval_json_unary_text(name, args, functions, env, lines)?;
            Ok(spike_option(json_parse_bool(&text).map(SpikeValue::Bool)))
        }
        "json_parse_string" => {
            let text = eval_json_unary_text(name, args, functions, env, lines)?;
            Ok(spike_option(json_parse_string(&text).map(SpikeValue::Text)))
        }
        "json_parse_value" => {
            let text = eval_json_unary_text(name, args, functions, env, lines)?;
            Ok(spike_option(json_parse_value(&text).map(SpikeValue::Text)))
        }
        "json_parse_field_int" => {
            let (text, key) = eval_json_binary_text(name, args, functions, env, lines)?;
            Ok(spike_option(
                json_object_field(&text, &key)
                    .and_then(|value| json_parse_int(&value))
                    .map(SpikeValue::Int),
            ))
        }
        "json_parse_field_bool" => {
            let (text, key) = eval_json_binary_text(name, args, functions, env, lines)?;
            Ok(spike_option(
                json_object_field(&text, &key)
                    .and_then(|value| json_parse_bool(&value))
                    .map(SpikeValue::Bool),
            ))
        }
        "json_parse_field_string" => {
            let (text, key) = eval_json_binary_text(name, args, functions, env, lines)?;
            Ok(spike_option(
                json_object_field(&text, &key)
                    .and_then(|value| json_parse_string(&value))
                    .map(SpikeValue::Text),
            ))
        }
        "json_parse_field_value" => {
            let (text, key) = eval_json_binary_text(name, args, functions, env, lines)?;
            Ok(spike_option(
                json_object_field(&text, &key)
                    .and_then(|value| json_parse_value(&value))
                    .map(SpikeValue::Text),
            ))
        }
        "json_stringify_int" => {
            let value = eval_json_unary(name, args, functions, env, lines)?;
            let value = expect_signed_integer(value)?;
            Ok(SpikeValue::Text(value.to_string()))
        }
        "json_stringify_bool" => {
            let value = eval_json_unary(name, args, functions, env, lines)?;
            Ok(SpikeValue::Text(expect_bool(value)?.to_string()))
        }
        "json_stringify_string" => {
            let text = eval_json_unary_text(name, args, functions, env, lines)?;
            Ok(SpikeValue::Text(json_escape_string(&text)))
        }
        "json_stringify_value" => {
            let text = eval_json_unary_text(name, args, functions, env, lines)?;
            Ok(SpikeValue::Text(json_parse_value(&text).unwrap_or(text)))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike JSON call {name:?}"
        ))),
    }
}

fn eval_json_unary(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [arg] = args else {
        return Err(unsupported(&format!("{name} expects exactly one argument")));
    };
    eval_expr(arg, functions, env, lines)
}

fn eval_json_unary_text(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<String, Diagnostic> {
    match eval_json_unary(name, args, functions, env, lines)? {
        SpikeValue::Text(value) => Ok(value),
        _ => Err(unsupported(&format!("{name} expects a string argument"))),
    }
}

fn eval_json_binary_text(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<(String, String), Diagnostic> {
    let [left, right] = args else {
        return Err(unsupported(&format!(
            "{name} expects exactly two arguments"
        )));
    };
    let left = match eval_expr(left, functions, env, lines)? {
        SpikeValue::Text(value) => value,
        _ => {
            return Err(unsupported(&format!(
                "{name} expects a string JSON argument"
            )));
        }
    };
    let right = match eval_expr(right, functions, env, lines)? {
        SpikeValue::Text(value) => value,
        _ => {
            return Err(unsupported(&format!(
                "{name} expects a string key argument"
            )));
        }
    };
    Ok((left, right))
}

fn spike_option(value: Option<SpikeValue>) -> SpikeValue {
    match value {
        Some(value) => SpikeValue::Enum {
            enum_name: String::from("Option"),
            variant: String::from("Some"),
            field_names: Vec::new(),
            payloads: vec![value],
        },
        None => SpikeValue::Enum {
            enum_name: String::from("Option"),
            variant: String::from("None"),
            field_names: Vec::new(),
            payloads: Vec::new(),
        },
    }
}

fn json_parse_int(text: &str) -> Option<i64> {
    text.trim().parse::<i64>().ok()
}

fn json_parse_bool(text: &str) -> Option<bool> {
    match text.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn json_parse_string(text: &str) -> Option<String> {
    let text = text.trim();
    if text.len() < 2 || !text.starts_with('"') || !text.ends_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut chars = text[1..text.len() - 1].chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next()? {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            '/' => out.push('/'),
            'b' => out.push('\u{0008}'),
            'f' => out.push('\u{000C}'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'u' => {
                let mut value = 0u32;
                for _ in 0..4 {
                    value = (value << 4) + chars.next()?.to_digit(16)?;
                }
                out.push(char::from_u32(value)?);
            }
            _ => return None,
        }
    }
    Some(out)
}

fn json_skip_ws(text: &str, mut index: usize) -> usize {
    let bytes = text.as_bytes();
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    index
}

fn json_scan_string_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.get(start).copied()? != b'"' {
        return None;
    }
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return Some(index + 1),
            _ => index += 1,
        }
    }
    None
}

fn json_scan_value_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if start >= bytes.len() {
        return None;
    }
    if bytes[start] == b'"' {
        return json_scan_string_end(text, start);
    }
    let mut index = start;
    let mut depth = 0i64;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => index = json_scan_string_end(text, index)?,
            b'{' | b'[' => {
                depth += 1;
                index += 1;
            }
            b'}' | b']' if depth > 0 => {
                depth -= 1;
                index += 1;
            }
            b',' | b'}' if depth == 0 => return Some(index),
            _ => index += 1,
        }
    }
    Some(index)
}

fn json_object_field(text: &str, key: &str) -> Option<String> {
    let text = text.trim();
    let bytes = text.as_bytes();
    if bytes.first().copied()? != b'{' || bytes.last().copied()? != b'}' {
        return None;
    }
    let mut index = 1usize;
    loop {
        index = json_skip_ws(text, index);
        if index >= bytes.len() || bytes[index] == b'}' {
            return None;
        }
        let key_end = json_scan_string_end(text, index)?;
        let found_key = json_parse_string(&text[index..key_end])?;
        index = json_skip_ws(text, key_end);
        if bytes.get(index).copied()? != b':' {
            return None;
        }
        let value_start = json_skip_ws(text, index + 1);
        let value_end = json_scan_value_end(text, value_start)?;
        if found_key == key {
            return Some(text[value_start..value_end].trim().to_string());
        }
        index = json_skip_ws(text, value_end);
        match bytes.get(index).copied()? {
            b',' => index += 1,
            b'}' => return None,
            _ => return None,
        }
    }
}

fn json_parse_value(text: &str) -> Option<String> {
    let text = text.trim();
    let end = json_scan_value_end(text, 0)?;
    if json_skip_ws(text, end) == text.len() {
        Some(text.to_string())
    } else {
        None
    }
}

fn json_escape_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn is_crypto_call(name: &str) -> bool {
    matches!(
        name,
        "crypto_sha256"
            | "crypto_hmac_sha256"
            | "crypto_hmac_sha512"
            | "crypto_constant_time_eq"
            | "crypto_constant_time_eq_u8"
    )
}

fn eval_crypto_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "crypto_sha256" => {
            let [arg] = args else {
                return Err(unsupported("crypto_sha256 expects exactly one argument"));
            };
            let input = expect_text(eval_expr(arg, functions, env, lines)?, name)?;
            Ok(SpikeValue::Text(sha256_hex(input.as_bytes())))
        }
        "crypto_hmac_sha256" | "crypto_hmac_sha512" => {
            let [key, message] = args else {
                return Err(unsupported(&format!(
                    "{name} expects exactly two arguments"
                )));
            };
            let key = expect_text(eval_expr(key, functions, env, lines)?, name)?;
            let message = expect_text(eval_expr(message, functions, env, lines)?, name)?;
            let tag = if name == "crypto_hmac_sha256" {
                hmac_hex(key.as_bytes(), message.as_bytes(), 64, sha256_bytes)
            } else {
                hmac_hex(key.as_bytes(), message.as_bytes(), 128, sha512_bytes)
            };
            Ok(SpikeValue::Text(tag))
        }
        "crypto_constant_time_eq" => {
            let [left, right] = args else {
                return Err(unsupported(
                    "crypto_constant_time_eq expects exactly two arguments",
                ));
            };
            let left = expect_text(eval_expr(left, functions, env, lines)?, name)?;
            let right = expect_text(eval_expr(right, functions, env, lines)?, name)?;
            Ok(SpikeValue::Bool(constant_time_eq_bytes(
                left.as_bytes(),
                right.as_bytes(),
            )))
        }
        "crypto_constant_time_eq_u8" => {
            let [left, right] = args else {
                return Err(unsupported(
                    "crypto_constant_time_eq_u8 expects exactly two arguments",
                ));
            };
            let left = expect_u8_array(eval_expr(left, functions, env, lines)?, name)?;
            let right = expect_u8_array(eval_expr(right, functions, env, lines)?, name)?;
            Ok(SpikeValue::Bool(constant_time_eq_bytes(&left, &right)))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike crypto call {name:?}"
        ))),
    }
}

fn eval_env_get_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [name] = args else {
        return Err(unsupported("env_get expects exactly one argument"));
    };
    let name = expect_text(eval_expr(name, functions, env, lines)?, "env_get")?;
    Ok(spike_option(env::var(name).ok().map(SpikeValue::Text)))
}

fn is_fs_write_call(name: &str) -> bool {
    matches!(
        name,
        "fs_write"
            | "fs_create"
            | "fs_append"
            | "fs_mkdir"
            | "fs_mkdir_all"
            | "fs_remove_file"
            | "fs_remove_dir"
            | "fs_replace"
    )
}

fn eval_fs_read_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [path] = args else {
        return Err(unsupported("fs_read expects exactly one argument"));
    };
    let path = expect_text(eval_expr(path, functions, env, lines)?, "fs_read")?;
    let Some(candidate) = spike_fs_existing_candidate(env, &path)? else {
        return Ok(spike_option(None));
    };
    let Some(metadata) = std::fs::metadata(&candidate).ok() else {
        return Ok(spike_option(None));
    };
    if !metadata.is_file() || metadata.len() > SPIKE_MAX_FS_READ_BYTES {
        return Ok(spike_option(None));
    }
    let Some(file) = std::fs::File::open(&candidate).ok() else {
        return Ok(spike_option(None));
    };
    let mut reader = file.take(SPIKE_MAX_FS_READ_BYTES + 1);
    let mut content = String::new();
    if reader.read_to_string(&mut content).is_err()
        || content.len() as u64 > SPIKE_MAX_FS_READ_BYTES
    {
        return Ok(spike_option(None));
    }
    Ok(spike_option(Some(SpikeValue::Text(content))))
}

fn eval_fs_write_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let result = match name {
        "fs_write" => {
            let (path, content) = eval_fs_path_content(name, args, functions, env, lines)?;
            if content.len() > SPIKE_MAX_FS_WRITE_BYTES {
                -1
            } else {
                spike_fs_write_candidate(env, &path, false)?
                    .and_then(|candidate| std::fs::write(candidate, content).ok())
                    .map(|()| 0)
                    .unwrap_or(-1)
            }
        }
        "fs_create" => {
            let path = eval_fs_path(name, args, functions, env, lines)?;
            spike_fs_write_candidate(env, &path, false)?
                .and_then(|candidate| {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(candidate)
                        .ok()
                })
                .map(|_| 0)
                .unwrap_or(-1)
        }
        "fs_append" => {
            let (path, content) = eval_fs_path_content(name, args, functions, env, lines)?;
            if content.len() > SPIKE_MAX_FS_WRITE_BYTES {
                -1
            } else {
                spike_fs_write_candidate(env, &path, false)?
                    .and_then(|candidate| {
                        let mut file = std::fs::OpenOptions::new()
                            .append(true)
                            .create(true)
                            .open(candidate)
                            .ok()?;
                        std::io::Write::write_all(&mut file, content.as_bytes()).ok()
                    })
                    .map(|()| 0)
                    .unwrap_or(-1)
            }
        }
        "fs_mkdir" => {
            let path = eval_fs_path(name, args, functions, env, lines)?;
            spike_fs_write_candidate(env, &path, false)?
                .and_then(|candidate| std::fs::create_dir(candidate).ok())
                .map(|()| 0)
                .unwrap_or(-1)
        }
        "fs_mkdir_all" => {
            let path = eval_fs_path(name, args, functions, env, lines)?;
            spike_fs_write_candidate(env, &path, true)?
                .and_then(|candidate| std::fs::create_dir_all(candidate).ok())
                .map(|()| 0)
                .unwrap_or(-1)
        }
        "fs_remove_file" => {
            let path = eval_fs_path(name, args, functions, env, lines)?;
            spike_fs_existing_candidate(env, &path)?
                .and_then(|candidate| {
                    std::fs::metadata(&candidate)
                        .ok()
                        .filter(|metadata| metadata.is_file())?;
                    std::fs::remove_file(candidate).ok()
                })
                .map(|()| 0)
                .unwrap_or(-1)
        }
        "fs_remove_dir" => {
            let path = eval_fs_path(name, args, functions, env, lines)?;
            spike_fs_existing_candidate(env, &path)?
                .and_then(|candidate| {
                    std::fs::metadata(&candidate)
                        .ok()
                        .filter(|metadata| metadata.is_dir())?;
                    std::fs::remove_dir(candidate).ok()
                })
                .map(|()| 0)
                .unwrap_or(-1)
        }
        "fs_replace" => {
            let (path, content) = eval_fs_path_content(name, args, functions, env, lines)?;
            if content.len() > SPIKE_MAX_FS_WRITE_BYTES {
                -1
            } else {
                spike_fs_write_candidate(env, &path, false)?
                    .and_then(|candidate| std::fs::write(candidate, content).ok())
                    .map(|()| 0)
                    .unwrap_or(-1)
            }
        }
        _ => {
            return Err(unsupported(&format!(
                "unsupported cranelift spike filesystem call {name:?}"
            )));
        }
    };
    Ok(SpikeValue::Int(result))
}

fn eval_fs_path(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<String, Diagnostic> {
    let [path] = args else {
        return Err(unsupported(&format!("{name} expects exactly one argument")));
    };
    expect_text(eval_expr(path, functions, env, lines)?, name)
}

fn eval_fs_path_content(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<(String, String), Diagnostic> {
    let [path, content] = args else {
        return Err(unsupported(&format!(
            "{name} expects exactly two arguments"
        )));
    };
    let path = expect_text(eval_expr(path, functions, env, lines)?, name)?;
    let content = expect_text(eval_expr(content, functions, env, lines)?, name)?;
    Ok((path, content))
}

fn spike_fs_root(env: &SpikeEnv) -> Result<PathBuf, Diagnostic> {
    match env.get(SPIKE_FS_ROOT_BINDING) {
        Some(SpikeValue::Text(root)) => Ok(PathBuf::from(root)),
        _ => Err(unsupported(
            "cranelift spike filesystem root is unavailable",
        )),
    }
}

fn spike_fs_existing_candidate(env: &SpikeEnv, path: &str) -> Result<Option<PathBuf>, Diagnostic> {
    let fs_root = spike_fs_root(env)?;
    let Some(candidate) = spike_fs_join_candidate(&fs_root, path) else {
        return Ok(None);
    };
    let Ok(canonical_root) = std::fs::canonicalize(fs_root) else {
        return Ok(None);
    };
    let Ok(canonical_candidate) = std::fs::canonicalize(candidate) else {
        return Ok(None);
    };
    Ok(canonical_candidate
        .starts_with(canonical_root)
        .then_some(canonical_candidate))
}

fn spike_fs_write_candidate(
    env: &SpikeEnv,
    path: &str,
    allow_missing_ancestors: bool,
) -> Result<Option<PathBuf>, Diagnostic> {
    let fs_root = spike_fs_root(env)?;
    let Some(candidate) = spike_fs_join_candidate(&fs_root, path) else {
        return Ok(None);
    };
    let Ok(canonical_root) = std::fs::canonicalize(&fs_root) else {
        return Ok(None);
    };
    if let Ok(canonical_candidate) = std::fs::canonicalize(&candidate) {
        return Ok(canonical_candidate
            .starts_with(canonical_root)
            .then_some(canonical_candidate));
    }
    let Some(parent) = candidate.parent() else {
        return Ok(None);
    };
    if !allow_missing_ancestors {
        let Ok(canonical_parent) = std::fs::canonicalize(parent) else {
            return Ok(None);
        };
        if !canonical_parent.starts_with(&canonical_root) {
            return Ok(None);
        }
        let Some(file_name) = candidate.file_name() else {
            return Ok(None);
        };
        return Ok(Some(canonical_parent.join(file_name)));
    }
    let mut ancestor = parent;
    while !ancestor.exists() {
        let Some(parent) = ancestor.parent() else {
            return Ok(None);
        };
        ancestor = parent;
    }
    let Ok(canonical_ancestor) = std::fs::canonicalize(ancestor) else {
        return Ok(None);
    };
    Ok(canonical_ancestor
        .starts_with(canonical_root)
        .then_some(candidate))
}

fn spike_fs_join_candidate(package_root: &Path, path: &str) -> Option<PathBuf> {
    let requested = Path::new(path);
    if requested.as_os_str().is_empty()
        || requested
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    Some(if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        package_root.join(requested)
    })
}

fn eval_process_status_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [command] = args else {
        return Err(unsupported("process_status expects exactly one argument"));
    };
    let command = expect_text(eval_expr(command, functions, env, lines)?, "process_status")?;
    let status = Command::new(&command)
        .status()
        .ok()
        .and_then(|status| status.code())
        .map(i64::from)
        .unwrap_or(-1);
    Ok(SpikeValue::Int(status))
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn hmac_hex(key: &[u8], message: &[u8], block_len: usize, digest: fn(&[u8]) -> Vec<u8>) -> String {
    let mut key_bytes = key.to_vec();
    if key_bytes.len() > block_len {
        key_bytes = digest(&key_bytes);
    }
    key_bytes.resize(block_len, 0);

    let mut inner = Vec::with_capacity(block_len + message.len());
    let mut outer = Vec::with_capacity(block_len + block_len);
    for byte in key_bytes {
        inner.push(byte ^ 0x36);
        outer.push(byte ^ 0x5c);
    }
    inner.extend_from_slice(message);
    let inner_digest = digest(&inner);
    outer.extend_from_slice(&inner_digest);
    hex_lower(&digest(&outer))
}

fn constant_time_eq_bytes(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&left_byte, &right_byte) in left.iter().zip(right.iter()) {
        diff |= left_byte ^ right_byte;
    }
    diff == 0
}

fn sha256_hex(input: &[u8]) -> String {
    hex_lower(&sha256_bytes(input))
}

fn sha256_bytes(input: &[u8]) -> Vec<u8> {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut data = input.to_vec();
    let bit_len = (data.len() as u64) * 8;
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in data.chunks(64) {
        let mut schedule = [0u32; 64];
        for (index, word) in schedule.iter_mut().take(16).enumerate() {
            let start = index * 4;
            *word = u32::from_be_bytes([
                chunk[start],
                chunk[start + 1],
                chunk[start + 2],
                chunk[start + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = schedule[index - 15].rotate_right(7)
                ^ schedule[index - 15].rotate_right(18)
                ^ (schedule[index - 15] >> 3);
            let s1 = schedule[index - 2].rotate_right(17)
                ^ schedule[index - 2].rotate_right(19)
                ^ (schedule[index - 2] >> 10);
            schedule[index] = schedule[index - 16]
                .wrapping_add(s0)
                .wrapping_add(schedule[index - 7])
                .wrapping_add(s1);
        }
        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];
        for index in 0..64 {
            let sigma1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(sigma1)
                .wrapping_add(choice)
                .wrapping_add(K[index])
                .wrapping_add(schedule[index]);
            let sigma0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = sigma0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }
    let mut output = Vec::with_capacity(32);
    for value in state {
        output.extend_from_slice(&value.to_be_bytes());
    }
    output
}

fn sha512_bytes(input: &[u8]) -> Vec<u8> {
    const K: [u64; 80] = [
        0x428a2f98d728ae22,
        0x7137449123ef65cd,
        0xb5c0fbcfec4d3b2f,
        0xe9b5dba58189dbbc,
        0x3956c25bf348b538,
        0x59f111f1b605d019,
        0x923f82a4af194f9b,
        0xab1c5ed5da6d8118,
        0xd807aa98a3030242,
        0x12835b0145706fbe,
        0x243185be4ee4b28c,
        0x550c7dc3d5ffb4e2,
        0x72be5d74f27b896f,
        0x80deb1fe3b1696b1,
        0x9bdc06a725c71235,
        0xc19bf174cf692694,
        0xe49b69c19ef14ad2,
        0xefbe4786384f25e3,
        0x0fc19dc68b8cd5b5,
        0x240ca1cc77ac9c65,
        0x2de92c6f592b0275,
        0x4a7484aa6ea6e483,
        0x5cb0a9dcbd41fbd4,
        0x76f988da831153b5,
        0x983e5152ee66dfab,
        0xa831c66d2db43210,
        0xb00327c898fb213f,
        0xbf597fc7beef0ee4,
        0xc6e00bf33da88fc2,
        0xd5a79147930aa725,
        0x06ca6351e003826f,
        0x142929670a0e6e70,
        0x27b70a8546d22ffc,
        0x2e1b21385c26c926,
        0x4d2c6dfc5ac42aed,
        0x53380d139d95b3df,
        0x650a73548baf63de,
        0x766a0abb3c77b2a8,
        0x81c2c92e47edaee6,
        0x92722c851482353b,
        0xa2bfe8a14cf10364,
        0xa81a664bbc423001,
        0xc24b8b70d0f89791,
        0xc76c51a30654be30,
        0xd192e819d6ef5218,
        0xd69906245565a910,
        0xf40e35855771202a,
        0x106aa07032bbd1b8,
        0x19a4c116b8d2d0c8,
        0x1e376c085141ab53,
        0x2748774cdf8eeb99,
        0x34b0bcb5e19b48a8,
        0x391c0cb3c5c95a63,
        0x4ed8aa4ae3418acb,
        0x5b9cca4f7763e373,
        0x682e6ff3d6b2b8a3,
        0x748f82ee5defb2fc,
        0x78a5636f43172f60,
        0x84c87814a1f0ab72,
        0x8cc702081a6439ec,
        0x90befffa23631e28,
        0xa4506cebde82bde9,
        0xbef9a3f7b2c67915,
        0xc67178f2e372532b,
        0xca273eceea26619c,
        0xd186b8c721c0c207,
        0xeada7dd6cde0eb1e,
        0xf57d4f7fee6ed178,
        0x06f067aa72176fba,
        0x0a637dc5a2c898a6,
        0x113f9804bef90dae,
        0x1b710b35131c471b,
        0x28db77f523047d84,
        0x32caab7b40c72493,
        0x3c9ebe0a15c9bebc,
        0x431d67c49c100d4c,
        0x4cc5d4becb3e42b6,
        0x597f299cfc657e2a,
        0x5fcb6fab3ad6faec,
        0x6c44198c4a475817,
    ];
    let mut state: [u64; 8] = [
        0x6a09e667f3bcc908,
        0xbb67ae8584caa73b,
        0x3c6ef372fe94f82b,
        0xa54ff53a5f1d36f1,
        0x510e527fade682d1,
        0x9b05688c2b3e6c1f,
        0x1f83d9abfb41bd6b,
        0x5be0cd19137e2179,
    ];
    let mut data = input.to_vec();
    let bit_len = (data.len() as u128) * 8;
    data.push(0x80);
    while data.len() % 128 != 112 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in data.chunks(128) {
        let mut schedule = [0u64; 80];
        for (index, word) in schedule.iter_mut().take(16).enumerate() {
            let start = index * 8;
            *word = u64::from_be_bytes([
                chunk[start],
                chunk[start + 1],
                chunk[start + 2],
                chunk[start + 3],
                chunk[start + 4],
                chunk[start + 5],
                chunk[start + 6],
                chunk[start + 7],
            ]);
        }
        for index in 16..80 {
            let s0 = schedule[index - 15].rotate_right(1)
                ^ schedule[index - 15].rotate_right(8)
                ^ (schedule[index - 15] >> 7);
            let s1 = schedule[index - 2].rotate_right(19)
                ^ schedule[index - 2].rotate_right(61)
                ^ (schedule[index - 2] >> 6);
            schedule[index] = schedule[index - 16]
                .wrapping_add(s0)
                .wrapping_add(schedule[index - 7])
                .wrapping_add(s1);
        }
        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];
        for index in 0..80 {
            let sigma1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
            let choice = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(sigma1)
                .wrapping_add(choice)
                .wrapping_add(K[index])
                .wrapping_add(schedule[index]);
            let sigma0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = sigma0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }
    let mut output = Vec::with_capacity(64);
    for value in state {
        output.extend_from_slice(&value.to_be_bytes());
    }
    output
}

fn is_regex_call(name: &str) -> bool {
    matches!(name, "regex_is_match" | "regex_find" | "regex_replace_all")
}

fn eval_regex_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "env_get" => {
            let [name] = args else {
                return Err(unsupported("env_get expects exactly one argument"));
            };
            let name = match eval_expr(name, functions, env, lines)? {
                SpikeValue::Text(value) => value,
                _ => return Err(unsupported("env_get expects a string argument")),
            };
            let value = std::env::var(name).ok();
            Ok(spike_option(value.map(SpikeValue::Text)))
        }
        "regex_is_match" => {
            let (pattern, text) = eval_regex_binary_text(name, args, functions, env, lines)?;
            Ok(SpikeValue::Bool(regex_find_span(&pattern, &text).is_some()))
        }
        "regex_find" => {
            let (pattern, text) = eval_regex_binary_text(name, args, functions, env, lines)?;
            let found = regex_find_span(&pattern, &text)
                .map(|(start, end)| SpikeValue::Text(text[start..end].to_string()));
            Ok(spike_option(found))
        }
        "regex_replace_all" => {
            let [pattern, text, replacement] = args else {
                return Err(unsupported(
                    "regex_replace_all expects exactly three arguments",
                ));
            };
            let pattern = expect_text(
                eval_expr(pattern, functions, env, lines)?,
                "regex_replace_all",
            )?;
            let text = expect_text(eval_expr(text, functions, env, lines)?, "regex_replace_all")?;
            let replacement = expect_text(
                eval_expr(replacement, functions, env, lines)?,
                "regex_replace_all",
            )?;
            Ok(SpikeValue::Text(regex_replace_all(
                &pattern,
                &text,
                &replacement,
            )))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike regex call {name:?}"
        ))),
    }
}

fn eval_regex_binary_text(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<(String, String), Diagnostic> {
    let [pattern, text] = args else {
        return Err(unsupported(&format!(
            "{name} expects exactly two arguments"
        )));
    };
    let pattern = expect_text(eval_expr(pattern, functions, env, lines)?, name)?;
    let text = expect_text(eval_expr(text, functions, env, lines)?, name)?;
    Ok((pattern, text))
}

fn eval_io_eprintln_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [arg] = args else {
        return Err(unsupported("io_eprintln expects exactly one argument"));
    };
    let text = match eval_expr(arg, functions, env, lines)? {
        SpikeValue::Text(value) => value,
        _ => return Err(unsupported("io_eprintln expects a string")),
    };
    let written = text.len() as i64 + 1;
    lines.push(OutputLine::stderr(text));
    Ok(SpikeValue::Int(written))
}

fn eval_clock_now_ms_call(args: &[Expr]) -> Result<SpikeValue, Diagnostic> {
    let [] = args else {
        return Err(unsupported("clock_now_ms expects no arguments"));
    };
    current_time_ms().map(SpikeValue::Int)
}

fn eval_clock_elapsed_ms_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [start] = args else {
        return Err(unsupported("clock_elapsed_ms expects exactly one argument"));
    };
    let start = expect_signed_integer(eval_expr(start, functions, env, lines)?)?;
    let now = current_time_ms()?;
    Ok(SpikeValue::Int(if now < start { -1 } else { now - start }))
}

fn eval_clock_sleep_ms_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [milliseconds] = args else {
        return Err(unsupported("clock_sleep_ms expects exactly one argument"));
    };
    let milliseconds = expect_signed_integer(eval_expr(milliseconds, functions, env, lines)?)?;
    if milliseconds < 0 {
        return Ok(SpikeValue::Int(-1));
    }
    if milliseconds == 0 {
        return Ok(SpikeValue::Int(0));
    }
    Err(unsupported(
        "nonzero clock_sleep_ms is not supported by the cranelift spike",
    ))
}

fn current_time_ms() -> Result<i64, Diagnostic> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| unsupported("system clock must be after unix epoch"))?;
    Ok(now.as_millis() as i64)
}

fn expect_text(value: SpikeValue, name: &str) -> Result<String, Diagnostic> {
    match value {
        SpikeValue::Text(value) => Ok(value),
        _ => Err(unsupported(&format!("{name} expects string arguments"))),
    }
}

fn expect_u8_array(value: SpikeValue, name: &str) -> Result<Vec<u8>, Diagnostic> {
    let SpikeValue::Array(values) = value else {
        return Err(unsupported(&format!("{name} expects byte-slice arguments")));
    };
    values
        .into_iter()
        .map(|value| match value {
            SpikeValue::UInt(value) if value <= u8::MAX as u64 => Ok(value as u8),
            SpikeValue::Int(value) if (0..=u8::MAX as i64).contains(&value) => Ok(value as u8),
            _ => Err(unsupported(&format!("{name} expects byte-slice arguments"))),
        })
        .collect()
}

fn regex_escape_char(ch: char) -> char {
    match ch {
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        other => other,
    }
}

fn regex_parse_atom(chars: &[char], pos: &mut usize) -> Option<RegexAtom> {
    if *pos >= chars.len() {
        return None;
    }
    let ch = chars[*pos];
    *pos += 1;
    match ch {
        '.' => Some(RegexAtom::Any),
        '\\' => {
            if *pos >= chars.len() {
                Some(RegexAtom::Literal('\\'))
            } else {
                let escaped = regex_escape_char(chars[*pos]);
                *pos += 1;
                Some(RegexAtom::Literal(escaped))
            }
        }
        '[' => {
            let mut negated = false;
            if *pos < chars.len() && chars[*pos] == '^' {
                negated = true;
                *pos += 1;
            }
            let mut ranges = Vec::new();
            let mut first = true;
            while *pos < chars.len() {
                if chars[*pos] == ']' && !first {
                    *pos += 1;
                    return Some(RegexAtom::Class { ranges, negated });
                }
                first = false;
                let start = if chars[*pos] == '\\' {
                    *pos += 1;
                    if *pos >= chars.len() {
                        return None;
                    }
                    let escaped = regex_escape_char(chars[*pos]);
                    *pos += 1;
                    escaped
                } else {
                    let value = chars[*pos];
                    *pos += 1;
                    value
                };
                if *pos + 1 < chars.len() && chars[*pos] == '-' && chars[*pos + 1] != ']' {
                    *pos += 1;
                    let end = if chars[*pos] == '\\' {
                        *pos += 1;
                        if *pos >= chars.len() {
                            return None;
                        }
                        let escaped = regex_escape_char(chars[*pos]);
                        *pos += 1;
                        escaped
                    } else {
                        let value = chars[*pos];
                        *pos += 1;
                        value
                    };
                    if start <= end {
                        ranges.push((start, end));
                    } else {
                        ranges.push((end, start));
                    }
                } else {
                    ranges.push((start, start));
                }
            }
            None
        }
        '(' | ')' | '|' => None,
        other => Some(RegexAtom::Literal(other)),
    }
}

fn regex_parse(pattern: &str) -> Option<RegexProgram> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut pos = 0usize;
    let mut start_anchor = false;
    let mut end_anchor = false;
    if pos < chars.len() && chars[pos] == '^' {
        start_anchor = true;
        pos += 1;
    }
    let mut parse_end = chars.len();
    if parse_end > pos && chars[parse_end - 1] == '$' {
        let escaped = parse_end >= 2 && chars[parse_end - 2] == '\\';
        if !escaped {
            end_anchor = true;
            parse_end -= 1;
        }
    }
    let mut tokens = Vec::new();
    while pos < parse_end {
        let mut atom_pos = pos;
        let atom = regex_parse_atom(&chars[..parse_end], &mut atom_pos)?;
        pos = atom_pos;
        let quantifier = if pos < parse_end {
            match chars[pos] {
                '?' => {
                    pos += 1;
                    RegexQuantifier::ZeroOrOne
                }
                '*' => {
                    pos += 1;
                    RegexQuantifier::ZeroOrMore
                }
                '+' => {
                    pos += 1;
                    RegexQuantifier::OneOrMore
                }
                _ => RegexQuantifier::One,
            }
        } else {
            RegexQuantifier::One
        };
        tokens.push(RegexToken { atom, quantifier });
    }
    Some(RegexProgram {
        tokens,
        start_anchor,
        end_anchor,
    })
}

fn regex_atom_matches(atom: &RegexAtom, ch: char) -> bool {
    match atom {
        RegexAtom::Literal(expected) => *expected == ch,
        RegexAtom::Any => true,
        RegexAtom::Class { ranges, negated } => {
            let found = ranges.iter().any(|(start, end)| *start <= ch && ch <= *end);
            if *negated { !found } else { found }
        }
    }
}

fn regex_add_state(program: &RegexProgram, states: &mut Vec<usize>, state: usize) {
    if states.contains(&state) {
        return;
    }
    states.push(state);
    if state >= program.tokens.len() {
        return;
    }
    match program.tokens[state].quantifier {
        RegexQuantifier::ZeroOrOne | RegexQuantifier::ZeroOrMore => {
            regex_add_state(program, states, state + 1);
        }
        RegexQuantifier::One | RegexQuantifier::OneOrMore => {}
    }
}

fn regex_accepts(program: &RegexProgram, states: &[usize], at_text_end: bool) -> bool {
    states
        .iter()
        .any(|state| *state == program.tokens.len() && (!program.end_anchor || at_text_end))
}

fn regex_match_from(program: &RegexProgram, text: &[char], start: usize) -> Option<usize> {
    let mut states = Vec::new();
    regex_add_state(program, &mut states, 0);
    let mut last_accept = if regex_accepts(program, &states, start == text.len()) {
        Some(start)
    } else {
        None
    };
    let mut pos = start;
    while pos < text.len() {
        let ch = text[pos];
        let mut next = Vec::new();
        for state in states.iter().copied() {
            if state >= program.tokens.len() {
                continue;
            }
            let token = &program.tokens[state];
            if !regex_atom_matches(&token.atom, ch) {
                continue;
            }
            match token.quantifier {
                RegexQuantifier::One | RegexQuantifier::ZeroOrOne => {
                    regex_add_state(program, &mut next, state + 1);
                }
                RegexQuantifier::ZeroOrMore => {
                    regex_add_state(program, &mut next, state);
                    regex_add_state(program, &mut next, state + 1);
                }
                RegexQuantifier::OneOrMore => {
                    regex_add_state(program, &mut next, state);
                    regex_add_state(program, &mut next, state + 1);
                }
            }
        }
        pos += 1;
        if regex_accepts(program, &next, pos == text.len()) {
            last_accept = Some(pos);
        }
        states = next;
        if states.is_empty() {
            return last_accept;
        }
    }
    last_accept
}

fn regex_find_span(pattern: &str, text: &str) -> Option<(usize, usize)> {
    let program = regex_parse(pattern)?;
    let chars: Vec<char> = text.chars().collect();
    let byte_offsets: Vec<usize> = text
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(text.len()))
        .collect();
    let starts: Box<dyn Iterator<Item = usize>> = if program.start_anchor {
        Box::new(std::iter::once(0))
    } else {
        Box::new(0..=chars.len())
    };
    for start in starts {
        if let Some(end) = regex_match_from(&program, &chars, start) {
            return Some((byte_offsets[start], byte_offsets[end]));
        }
    }
    None
}

fn regex_replace_all(pattern: &str, text: &str, replacement: &str) -> String {
    let Some(program) = regex_parse(pattern) else {
        return text.to_string();
    };
    if program.start_anchor {
        let Some((start, end)) = regex_find_span(pattern, text) else {
            return text.to_string();
        };
        let mut out = String::new();
        out.push_str(&text[..start]);
        out.push_str(replacement);
        out.push_str(&text[end..]);
        return out;
    }
    let mut remaining = text;
    let mut out = String::new();
    loop {
        let Some((start, end)) = regex_find_span(pattern, remaining) else {
            out.push_str(remaining);
            break;
        };
        out.push_str(&remaining[..start]);
        out.push_str(replacement);
        if end == 0 {
            if let Some(ch) = remaining.chars().next() {
                out.push(ch);
                remaining = &remaining[ch.len_utf8()..];
            } else {
                break;
            }
        } else {
            remaining = &remaining[end..];
        }
    }
    out
}

fn eval_arithmetic(
    op: ArithmeticOp,
    lhs: &Expr,
    rhs: &Expr,
    ty: &Type,
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let left = eval_expr(lhs, functions, env, lines)?;
    let right = eval_expr(rhs, functions, env, lines)?;
    match (ty, left, right) {
        (Type::Int, SpikeValue::Int(left), SpikeValue::Int(right)) => {
            let value = match op {
                ArithmeticOp::Add => left + right,
                ArithmeticOp::Sub => left - right,
                ArithmeticOp::Mul => left * right,
                ArithmeticOp::Div if right != 0 => left / right,
                ArithmeticOp::Div => return Err(unsupported("integer division by zero")),
            };
            Ok(SpikeValue::Int(value))
        }
        (Type::Numeric(numeric_ty), left, right) if is_signed_numeric(*numeric_ty) => {
            let left = expect_signed_integer(left)?;
            let right = expect_signed_integer(right)?;
            let value = match op {
                ArithmeticOp::Add => left + right,
                ArithmeticOp::Sub => left - right,
                ArithmeticOp::Mul => left * right,
                ArithmeticOp::Div if right != 0 => left / right,
                ArithmeticOp::Div => return Err(unsupported("integer division by zero")),
            };
            Ok(cast_signed_integer(value, *numeric_ty))
        }
        (Type::Numeric(numeric_ty), left, right) if is_unsigned_numeric(*numeric_ty) => {
            let left = expect_unsigned_integer(left)?;
            let right = expect_unsigned_integer(right)?;
            let value = match op {
                ArithmeticOp::Add => left + right,
                ArithmeticOp::Sub => left - right,
                ArithmeticOp::Mul => left * right,
                ArithmeticOp::Div if right != 0 => left / right,
                ArithmeticOp::Div => return Err(unsupported("integer division by zero")),
            };
            Ok(cast_unsigned_integer(value, *numeric_ty))
        }
        (
            Type::Numeric(numeric_ty @ (NumericType::F32 | NumericType::F64)),
            SpikeValue::Float(left),
            SpikeValue::Float(right),
        ) => eval_float_arithmetic(op, left, right, *numeric_ty),
        (Type::String | Type::Str, left, right) if op == ArithmeticOp::Add => Ok(SpikeValue::Text(
            format!("{}{}", render_value(&left), render_value(&right)),
        )),
        (Type::String | Type::Str, _, _) => Err(unsupported(
            "only string addition is supported by the cranelift spike",
        )),
        _ => Err(unsupported(
            "unsupported cranelift spike arithmetic operands",
        )),
    }
}

fn eval_float_arithmetic(
    op: ArithmeticOp,
    left: f64,
    right: f64,
    ty: NumericType,
) -> Result<SpikeValue, Diagnostic> {
    match ty {
        NumericType::F32 => {
            let left = left as f32;
            let right = right as f32;
            let value = match op {
                ArithmeticOp::Add => left + right,
                ArithmeticOp::Sub => left - right,
                ArithmeticOp::Mul => left * right,
                ArithmeticOp::Div if right != 0.0 => left / right,
                ArithmeticOp::Div => return Err(unsupported("floating-point division by zero")),
            };
            Ok(SpikeValue::Float(value as f64))
        }
        NumericType::F64 => {
            let value = match op {
                ArithmeticOp::Add => left + right,
                ArithmeticOp::Sub => left - right,
                ArithmeticOp::Mul => left * right,
                ArithmeticOp::Div if right != 0.0 => left / right,
                ArithmeticOp::Div => return Err(unsupported("floating-point division by zero")),
            };
            Ok(SpikeValue::Float(value))
        }
        _ => Err(unsupported(
            "floating-point arithmetic requires a float type",
        )),
    }
}

fn is_signed_numeric(ty: NumericType) -> bool {
    matches!(
        ty,
        NumericType::I8
            | NumericType::I16
            | NumericType::I32
            | NumericType::I64
            | NumericType::Isize
    )
}

fn is_unsigned_numeric(ty: NumericType) -> bool {
    matches!(
        ty,
        NumericType::U8
            | NumericType::U16
            | NumericType::U32
            | NumericType::U64
            | NumericType::Usize
    )
}

fn expect_signed_integer(value: SpikeValue) -> Result<i64, Diagnostic> {
    match value {
        SpikeValue::Int(value) => Ok(value),
        SpikeValue::UInt(value) => Ok(value as i64),
        _ => Err(unsupported("expected integer operands")),
    }
}

fn expect_unsigned_integer(value: SpikeValue) -> Result<u64, Diagnostic> {
    match value {
        SpikeValue::Int(value) => Ok(value as u64),
        SpikeValue::UInt(value) => Ok(value),
        _ => Err(unsupported("expected integer operands")),
    }
}

fn eval_compare(
    op: CompareOp,
    lhs: &Expr,
    rhs: &Expr,
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let left = eval_expr(lhs, functions, env, lines)?;
    let right = eval_expr(rhs, functions, env, lines)?;
    let result = match (left, right) {
        (SpikeValue::Int(left), SpikeValue::Int(right)) => compare_ord(op, left, right),
        (SpikeValue::UInt(left), SpikeValue::UInt(right)) => compare_ord(op, left, right),
        (SpikeValue::Float(left), SpikeValue::Float(right)) => compare_float(op, left, right)?,
        (SpikeValue::Bool(left), SpikeValue::Bool(right)) => compare_eq(op, left, right)?,
        (SpikeValue::Text(left), SpikeValue::Text(right)) => compare_eq(op, left, right)?,
        _ => return Err(unsupported("mismatched comparison operands")),
    };
    Ok(SpikeValue::Bool(result))
}

fn compare_ord<T: Ord>(op: CompareOp, left: T, right: T) -> bool {
    match op {
        CompareOp::Eq => left == right,
        CompareOp::Ne => left != right,
        CompareOp::Lt => left < right,
        CompareOp::Le => left <= right,
        CompareOp::Gt => left > right,
        CompareOp::Ge => left >= right,
    }
}

fn compare_float(op: CompareOp, left: f64, right: f64) -> Result<bool, Diagnostic> {
    if !left.is_finite() || !right.is_finite() {
        return Err(unsupported("non-finite float comparison"));
    }
    Ok(match op {
        CompareOp::Eq => left == right,
        CompareOp::Ne => left != right,
        CompareOp::Lt => left < right,
        CompareOp::Le => left <= right,
        CompareOp::Gt => left > right,
        CompareOp::Ge => left >= right,
    })
}

fn compare_eq<T: Eq>(op: CompareOp, left: T, right: T) -> Result<bool, Diagnostic> {
    match op {
        CompareOp::Eq => Ok(left == right),
        CompareOp::Ne => Ok(left != right),
        _ => Err(unsupported("only equality comparisons are supported here")),
    }
}

fn expect_bool(value: SpikeValue) -> Result<bool, Diagnostic> {
    match value {
        SpikeValue::Bool(value) => Ok(value),
        _ => Err(unsupported("expected boolean expression")),
    }
}

fn expect_non_negative_index(value: SpikeValue) -> Result<usize, Diagnostic> {
    match value {
        SpikeValue::Int(value) if value >= 0 => Ok(value as usize),
        SpikeValue::Int(_) => Err(unsupported("array index cannot be negative")),
        SpikeValue::UInt(value) => usize::try_from(value)
            .map_err(|_| unsupported("array index is outside the host usize range")),
        _ => Err(unsupported("array index must be an integer")),
    }
}

fn insert_map_entry(
    entries: &mut Vec<(SpikeValue, SpikeValue)>,
    key: SpikeValue,
    value: SpikeValue,
) -> Result<(), Diagnostic> {
    for (candidate, existing) in entries.iter_mut() {
        if map_keys_equal(candidate, &key)? {
            *existing = value;
            return Ok(());
        }
    }
    entries.push((key, value));
    Ok(())
}

fn validate_map_key(value: &SpikeValue) -> Result<(), Diagnostic> {
    match value {
        SpikeValue::Int(_) | SpikeValue::UInt(_) | SpikeValue::Bool(_) | SpikeValue::Text(_) => {
            Ok(())
        }
        SpikeValue::Tuple(values) => values.iter().try_for_each(validate_map_key),
        SpikeValue::Float(_) => Err(unsupported(
            "map float keys are not supported by the cranelift spike",
        )),
        SpikeValue::Enum { .. }
        | SpikeValue::Struct { .. }
        | SpikeValue::Map(_)
        | SpikeValue::Array(_) => Err(unsupported(
            "map keys must be scalar values or scalar tuples in the cranelift spike",
        )),
    }
}

fn map_keys_equal(left: &SpikeValue, right: &SpikeValue) -> Result<bool, Diagnostic> {
    match (left, right) {
        (SpikeValue::Int(left), SpikeValue::Int(right)) => Ok(left == right),
        (SpikeValue::UInt(left), SpikeValue::UInt(right)) => Ok(left == right),
        (SpikeValue::Bool(left), SpikeValue::Bool(right)) => Ok(left == right),
        (SpikeValue::Text(left), SpikeValue::Text(right)) => Ok(left == right),
        (SpikeValue::Tuple(left), SpikeValue::Tuple(right)) if left.len() == right.len() => left
            .iter()
            .zip(right.iter())
            .try_fold(true, |matches, (left, right)| {
                Ok::<_, Diagnostic>(matches && map_keys_equal(left, right)?)
            }),
        (SpikeValue::Tuple(_), SpikeValue::Tuple(_)) => Ok(false),
        _ => Err(unsupported(
            "map key types must match in the cranelift spike",
        )),
    }
}

fn render_value(value: &SpikeValue) -> String {
    match value {
        SpikeValue::Int(value) => value.to_string(),
        SpikeValue::UInt(value) => value.to_string(),
        SpikeValue::Float(value) => value.to_string(),
        SpikeValue::Bool(true) => String::from("true"),
        SpikeValue::Bool(false) => String::from("false"),
        SpikeValue::Text(value) => value.clone(),
        SpikeValue::Struct { name, fields } => render_struct(name, fields),
        SpikeValue::Enum {
            variant, payloads, ..
        } => render_enum(variant, payloads),
        SpikeValue::Tuple(values) => render_sequence("(", ")", values),
        SpikeValue::Map(entries) => render_map(entries),
        SpikeValue::Array(values) => render_sequence("[", "]", values),
    }
}

fn render_enum(variant: &str, payloads: &[SpikeValue]) -> String {
    if payloads.is_empty() {
        return variant.to_string();
    }
    format!("{variant}{}", render_sequence("(", ")", payloads))
}

fn render_struct(name: &str, fields: &[(String, SpikeValue)]) -> String {
    let mut rendered = format!("{name} {{ ");
    for (index, (field, value)) in fields.iter().enumerate() {
        if index > 0 {
            rendered.push_str(", ");
        }
        rendered.push_str(field);
        rendered.push_str(": ");
        rendered.push_str(&render_value(value));
    }
    rendered.push_str(" }");
    rendered
}

fn render_sequence(open: &str, close: &str, values: &[SpikeValue]) -> String {
    let mut rendered = String::from(open);
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            rendered.push_str(", ");
        }
        rendered.push_str(&render_value(value));
    }
    rendered.push_str(close);
    rendered
}

fn render_map(entries: &[(SpikeValue, SpikeValue)]) -> String {
    let mut rendered = String::from("{");
    for (index, (key, value)) in entries.iter().enumerate() {
        if index > 0 {
            rendered.push_str(", ");
        }
        rendered.push_str(&render_value(key));
        rendered.push_str(": ");
        rendered.push_str(&render_value(value));
    }
    rendered.push('}');
    rendered
}

fn unsupported(message: &str) -> Diagnostic {
    Diagnostic::new(
        "build",
        format!("unsupported by --backend cranelift spike: {message}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_replace_all_start_anchor_only_replaces_original_match() {
        assert_eq!(regex_replace_all("^a", "aa", "x"), "xa");
        assert_eq!(regex_replace_all("^a", "aaa", "x"), "xaa");
        assert_eq!(regex_replace_all("^a", "ba", "x"), "ba");
        assert_eq!(regex_replace_all("a", "aaa", "x"), "xxx");
    }

    fn hello_program() -> Program {
        Program {
            path: String::from("hello"),
            structs: vec![],
            enums: vec![],
            statics: vec![],
            functions: vec![
                Function {
                    name: String::from("banner"),
                    source_name: String::from("banner"),
                    path: String::from("hello"),
                    params: vec![crate::mir::Param {
                        name: String::from("name"),
                        ty: Type::String,
                    }],
                    return_ty: Type::String,
                    body: vec![Stmt::Return {
                        expr: Expr::BinaryAdd {
                            op: ArithmeticOp::Add,
                            lhs: Box::new(Expr::Literal(LiteralValue::String(String::from(
                                "hello ",
                            )))),
                            rhs: Box::new(Expr::VarRef {
                                name: String::from("name"),
                                ty: Type::String,
                            }),
                            ty: Type::String,
                        },
                        span: crate::mir::SourceSpan { line: 1, column: 1 },
                    }],
                    is_property: false,
                    is_async: false,
                    is_extern: false,
                    extern_abi: None,
                    extern_library: None,
                    line: 1,
                    column: 1,
                },
                Function {
                    name: String::from("lucky"),
                    source_name: String::from("lucky"),
                    path: String::from("hello"),
                    params: vec![crate::mir::Param {
                        name: String::from("base"),
                        ty: Type::Int,
                    }],
                    return_ty: Type::Int,
                    body: vec![Stmt::Return {
                        expr: Expr::BinaryAdd {
                            op: ArithmeticOp::Add,
                            lhs: Box::new(Expr::VarRef {
                                name: String::from("base"),
                                ty: Type::Int,
                            }),
                            rhs: Box::new(Expr::Literal(LiteralValue::Int(2))),
                            ty: Type::Int,
                        },
                        span: crate::mir::SourceSpan { line: 1, column: 1 },
                    }],
                    is_property: false,
                    is_async: false,
                    is_extern: false,
                    extern_abi: None,
                    extern_library: None,
                    line: 1,
                    column: 1,
                },
            ],
            stmts: vec![
                Stmt::Let {
                    name: String::from("answer"),
                    ty: Type::Int,
                    expr: Expr::Call {
                        name: String::from("lucky"),
                        args: vec![Expr::Literal(LiteralValue::Int(40))],
                        ty: Type::Int,
                    },
                    span: crate::mir::SourceSpan { line: 1, column: 1 },
                },
                Stmt::If {
                    cond: Expr::BinaryCompare {
                        op: CompareOp::Eq,
                        lhs: Box::new(Expr::VarRef {
                            name: String::from("answer"),
                            ty: Type::Int,
                        }),
                        rhs: Box::new(Expr::Literal(LiteralValue::Int(42))),
                        ty: Type::Bool,
                    },
                    then_block: vec![Stmt::Print {
                        expr: Expr::Call {
                            name: String::from("banner"),
                            args: vec![Expr::Literal(LiteralValue::String(String::from(
                                "from stage1",
                            )))],
                            ty: Type::String,
                        },
                        span: crate::mir::SourceSpan { line: 1, column: 1 },
                    }],
                    else_block: None,
                    span: crate::mir::SourceSpan { line: 1, column: 1 },
                },
                Stmt::Print {
                    expr: Expr::VarRef {
                        name: String::from("answer"),
                        ty: Type::Int,
                    },
                    span: crate::mir::SourceSpan { line: 1, column: 1 },
                },
            ],
        }
    }

    #[test]
    fn folds_hello_subset_into_print_lines() {
        assert_eq!(
            collect_output_lines(&hello_program(), Path::new("."), Path::new("."))
                .expect("fold hello"),
            vec![
                OutputLine::stdout("hello from stage1"),
                OutputLine::stdout("42")
            ]
        );
    }
}
