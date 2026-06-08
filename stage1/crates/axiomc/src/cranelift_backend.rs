use crate::diagnostics::Diagnostic;
use crate::mir::{
    ArithmeticOp, CompareOp, Expr, Function, LiteralValue, MatchArm, MatchExprArm, Program, Stmt,
    Type,
};
use crate::syntax::NumericType;
use axiomc_backend_cranelift::OutputLine;
use std::collections::HashMap;
use std::path::Path;

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

pub fn compile_cranelift_hello_spike(
    program: &Program,
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
    let lines = collect_output_lines(program)?;
    axiomc_backend_cranelift::compile_output_lines(&lines, object_path, binary_path).map_err(
        |err| {
            Diagnostic::new("build", err.to_string()).with_path(object_path.display().to_string())
        },
    )
}

fn collect_output_lines(program: &Program) -> Result<Vec<OutputLine>, Diagnostic> {
    let functions = program
        .functions
        .iter()
        .map(|function| (function.name.as_str(), function))
        .collect::<HashMap<_, _>>();
    let mut env = SpikeEnv::new();
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
    if name == "crypto_sha256" {
        return eval_crypto_sha256_call(args, functions, env, lines);
    }
    if name == "env_get" {
        return eval_env_get_call(args, functions, env);
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

fn eval_crypto_sha256_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [arg] = args else {
        return Err(unsupported("crypto_sha256 expects exactly one argument"));
    };
    let input = match eval_expr(arg, functions, env, lines)? {
        SpikeValue::Text(value) => value,
        _ => return Err(unsupported("crypto_sha256 expects a string argument")),
    };
    Ok(SpikeValue::Text(sha256_hex(&input)))
}

fn sha256_hex(input: &str) -> String {
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
    let mut data = input.as_bytes().to_vec();
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
    let mut output = String::with_capacity(64);
    for value in state {
        output.push_str(&format!("{value:08x}"));
    }
    output
}

    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
) -> Result<SpikeValue, Diagnostic> {
    let [name] = args else {
        return Err(unsupported("env_get expects exactly one argument"));
    };
    let name = match eval_expr(name, functions, env)? {
        SpikeValue::Text(value) => value,
        _ => return Err(unsupported("env_get expects a string argument")),
    };
    let value = std::env::var(name).ok();
    Ok(option_text(value))
}

fn option_text(value: Option<String>) -> SpikeValue {
    match value {
        Some(value) => SpikeValue::Enum {
            enum_name: String::from("Option"),
            variant: String::from("Some"),
            field_names: Vec::new(),
            payloads: vec![SpikeValue::Text(value)],
        },
        None => SpikeValue::Enum {
            enum_name: String::from("Option"),
            variant: String::from("None"),
            field_names: Vec::new(),
            payloads: Vec::new(),
        },
    }
}

fn eval_io_eprintln_call(
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
            collect_output_lines(&hello_program()).expect("fold hello"),
            vec![
                OutputLine::stdout("hello from stage1"),
                OutputLine::stdout("42")
            ]
        );
    }
}
