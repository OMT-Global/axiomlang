use crate::diagnostics::Diagnostic;
use crate::manifest::CapabilityConfig;
use crate::mir::{
    ArithmeticOp, CompareOp, EnumDef, EnumVariantDef, Expr, Function, LiteralValue, LogicOp,
    MapEntry, MatchArm, MatchExprArm, Program, StaticDef, Stmt, StructDef, Type,
};
use crate::syntax::NumericType;
use axiomc_backend_cranelift::{
    I64_STDIN_BUFFER_BYTES, I64AuditSuccess as CraneliftI64AuditSuccess,
    I64BinaryOp as CraneliftI64BinaryOp, I64Cast as CraneliftI64Cast,
    I64Compare as CraneliftI64Compare, I64CompareOp as CraneliftI64CompareOp,
    I64Condition as CraneliftI64Condition, I64ExitBody, I64ExitProgram,
    I64Expr as CraneliftI64Expr, I64Function as CraneliftI64Function,
    I64ReturnBlock as CraneliftI64ReturnBlock, I64Stmt as CraneliftI64Stmt,
    I64ValueBody as CraneliftI64ValueBody, I64ValueReturnBlock as CraneliftI64ValueReturnBlock,
    OutputLine, OutputStream,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::env;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};

mod intrinsics;
pub(crate) use intrinsics::*;
mod evaluator;
pub(crate) use evaluator::*;
mod host_fs;
pub(crate) use host_fs::*;
mod host_crypto;
pub(crate) use host_crypto::*;
mod host_net_http;
pub(crate) use host_net_http::*;
mod host_env_proc_clock;
pub(crate) use host_env_proc_clock::*;
mod host_json_serdes;
pub(crate) use host_json_serdes::*;
mod static_output_purity;
mod compilation_mode;
pub use compilation_mode::CraneliftCompilationMode;
use compilation_mode::{direct_native_mode, known_value_fold_call, runtime_lowering_required};
use static_output_purity::allows_static_output_evaluation;

const SPIKE_PACKAGE_ROOT_BINDING: &str = "$axiom_package_root";
const SPIKE_FS_ROOT_BINDING: &str = "$axiom_fs_root";
const SPIKE_ENV_ALLOWLIST_BINDING: &str = "$axiom_env_allowlist";
const SPIKE_ENV_UNRESTRICTED_BINDING: &str = "$axiom_env_unrestricted";
const SPIKE_MAX_FS_READ_BYTES: u64 = 64 * 1024 * 1024;
const SPIKE_MAX_FS_WRITE_BYTES: usize = 64 * 1024 * 1024;
const SPIKE_MAX_CLOCK_SLEEP_MS: i64 = 1_000;
const CRANELIFT_RUNTIME_TRAP_KIND: &str = "cranelift-runtime-trap";

/// A mutable-slice binding that aliases a local fixed array's element slots.
/// Writes and reads through the binding resolve to the base projection locals
/// so mutation is visible through the base array.
#[derive(Clone)]
struct I64MutSliceAlias {
    base: String,
    start: usize,
    len: usize,
}

#[derive(Clone, Default)]
pub(crate) struct I64StaticBindings {
    mut_slice_aliases: HashMap<String, I64MutSliceAlias>,
    values: HashMap<String, CraneliftI64Expr>,
    conditions: HashMap<String, CraneliftI64Condition>,
    strings: HashMap<String, String>,
    string_options: HashMap<String, Option<String>>,
    string_builders: HashMap<String, String>,
    i64_once_cells: HashMap<String, Option<i64>>,
    bool_once_cells: HashMap<String, Option<bool>>,
    i64_channels: HashMap<String, Option<i64>>,
    bool_channels: HashMap<String, Option<bool>>,
    map_literals: HashMap<String, Vec<MapEntry>>,
    map_key_arrays: HashMap<String, Vec<I64MapKey>>,
    map_key_array_string_indexes: HashMap<String, I64MapKeyArrayStringIndex>,
    process_status_wrappers: HashSet<String>,
    env_get_wrappers: HashSet<String>,
    env_allowed_names: HashSet<String>,
    env_unrestricted: bool,
    time_wrappers: HashSet<String>,
    time_now_wrappers: HashSet<String>,
    time_now_ms_wrappers: HashSet<String>,
    time_elapsed_ms_wrappers: HashSet<String>,
    time_duration_ms_wrappers: HashSet<String>,
    time_sleep_wrappers: HashSet<String>,
    fs_read_wrappers: HashSet<String>,
    fs_write_wrappers: HashMap<String, String>,
    has_fs_write_calls: bool,
    fs_shim_wrappers: HashSet<String>,
    net_shim_wrappers: HashSet<String>,
    net_resolve_wrappers: HashSet<String>,
    net_unrestricted: bool,
    net_allowed_hosts: HashSet<String>,
    http_shim_wrappers: HashSet<String>,
    http_get_wrappers: HashSet<String>,
    http_serve_once_wrappers: HashSet<String>,
    http_listen_wrappers: HashSet<String>,
    http_local_port_wrappers: HashSet<String>,
    http_accept_wrappers: HashSet<String>,
    http_respond_wrappers: HashSet<String>,
    http_close_wrappers: HashSet<String>,
    http_server_ports: HashMap<String, u16>,
    collection_wrappers: HashSet<String>,
    collection_contains_wrappers: HashSet<String>,
    collection_get_wrappers: HashSet<String>,
    collection_get_or_default_wrappers: HashSet<String>,
    collection_keys_wrappers: HashSet<String>,
    regex_wrappers: HashSet<String>,
    regex_is_match_wrappers: HashSet<String>,
    regex_find_wrappers: HashSet<String>,
    regex_replace_all_wrappers: HashSet<String>,
    encoding_wrappers: HashSet<String>,
    encoding_url_component_encode_wrappers: HashSet<String>,
    encoding_url_component_decode_wrappers: HashSet<String>,
    encoding_path_segment_encode_wrappers: HashSet<String>,
    encoding_url_query_pair_encode_wrappers: HashSet<String>,
    encoding_path_join_segment_wrappers: HashSet<String>,
    json_wrappers: HashSet<String>,
    json_parse_int_wrappers: HashSet<String>,
    json_parse_bool_wrappers: HashSet<String>,
    json_parse_string_wrappers: HashSet<String>,
    json_parse_field_int_wrappers: HashSet<String>,
    json_parse_field_bool_wrappers: HashSet<String>,
    json_parse_field_string_wrappers: HashSet<String>,
    json_stringify_int_wrappers: HashSet<String>,
    json_stringify_bool_wrappers: HashSet<String>,
    json_stringify_string_wrappers: HashSet<String>,
    io_eprintln_wrappers: HashSet<String>,
    io_readline_wrappers: HashSet<String>,
    io_read_to_string_wrappers: HashSet<String>,
    log_wrappers: HashSet<String>,
    log_field_string_wrappers: HashSet<String>,
    log_field_int_wrappers: HashSet<String>,
    log_field_bool_wrappers: HashSet<String>,
    log_fields2_wrappers: HashSet<String>,
    log_fields3_wrappers: HashSet<String>,
    log_event_wrappers: HashSet<String>,
    log_info_attrs_wrappers: HashSet<String>,
    log_level_wrappers: HashMap<String, String>,
    string_builder_wrappers: HashSet<String>,
    string_builder_new_wrappers: HashSet<String>,
    string_builder_from_string_wrappers: HashSet<String>,
    string_builder_push_str_wrappers: HashSet<String>,
    string_builder_push_line_wrappers: HashSet<String>,
    string_builder_finish_wrappers: HashSet<String>,
    crypto_wrappers: HashSet<String>,
    crypto_sha256_wrappers: HashSet<String>,
    crypto_hmac_sha256_wrappers: HashSet<String>,
    crypto_hmac_sha512_wrappers: HashSet<String>,
    crypto_constant_time_eq_wrappers: HashSet<String>,
    crypto_constant_time_eq_u8_wrappers: HashSet<String>,
    crypto_verify_sha256_wrappers: HashSet<String>,
    crypto_verify_sha512_wrappers: HashSet<String>,
    crypto_random_bytes_wrappers: HashSet<String>,
    crypto_random_u64_wrappers: HashSet<String>,
    ffi_strlen_symbols: HashSet<String>,
    sync_once_wrappers: HashSet<String>,
    sync_once_with_wrappers: HashSet<String>,
    sync_once_is_set_wrappers: HashSet<String>,
    sync_once_take_wrappers: HashSet<String>,
    sync_channel_wrappers: HashSet<String>,
    sync_send_wrappers: HashSet<String>,
    sync_try_recv_wrappers: HashSet<String>,
    package_root: Option<PathBuf>,
    fs_root: Option<PathBuf>,
    structs: HashMap<String, StructDef>,
    enums: HashMap<String, EnumDef>,
    functions: HashMap<String, Function>,
}

pub(crate) struct I64HelperSignature {
    function: usize,
    params: usize,
    returns_bool: bool,
    return_ty: Type,
    returns: usize,
    struct_fields: Vec<Option<Vec<String>>>,
}

type I64StructDefs<'a> = HashMap<&'a str, &'a StructDef>;

#[derive(Clone)]
enum I64AggregateReturnShape {
    Array {
        element: Type,
        size: usize,
    },
    Tuple(Vec<Type>),
    Struct {
        name: String,
        fields: Vec<(String, Type)>,
    },
    Option {
        inner: Type,
        payload_slots: usize,
    },
    Result {
        ok: Type,
        err: Type,
        payload_slots: usize,
    },
    Enum {
        name: String,
        payload_slots: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum I64MapKey {
    Int(i64),
    Bool(bool),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct I64MapKeyArrayStringIndex {
    array_name: String,
    index: Expr,
    transform: I64MapKeyArrayStringTransform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum I64MapKeyArrayStringTransform {
    Identity,
    Trim,
    TrimStart,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SpikeValue {
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
    Closure {
        params: Vec<crate::mir::Param>,
        body: Box<Expr>,
        env: SpikeEnv,
    },
    MutRef(String),
    MutSlice {
        target: String,
        start: usize,
        end: usize,
    },
    Task {
        value: Option<Box<SpikeValue>>,
        canceled: bool,
        /// Deferred time the task's body took to evaluate. Used by `await` and
        /// `async_timeout` to decide how long consumption should wait.
        duration_ms: i64,
    },
    JoinHandle {
        task: Box<SpikeValue>,
        ready_at_ms: i64,
    },
    AsyncChannel {
        slot: Option<Box<SpikeValue>>,
    },
    SelectResult {
        selected: i64,
        value: Option<Box<SpikeValue>>,
    },
    ControlReturn(Box<SpikeValue>),
}

pub(crate) type SpikeEnv = HashMap<String, SpikeValue>;

pub(crate) struct SpikeHttpServer {
    listener: TcpListener,
}

pub(crate) struct SpikeHttpRequest {
    stream: TcpStream,
    method: String,
    path: String,
    body: String,
}

pub(crate) struct SpikeTcpListener {
    port: i64,
}

pub(crate) struct SpikeTcpStream {
    listener_port: i64,
    received: String,
    written: String,
}

pub(crate) struct SpikeUdpSocket {
    addr: SocketAddr,
    datagrams: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SpikeStdin {
    content: String,
    offset: usize,
}

impl SpikeStdin {
    fn new(content: Option<&str>) -> Self {
        Self {
            content: content.unwrap_or_default().to_string(),
            offset: 0,
        }
    }

    fn readline(&mut self) -> Option<String> {
        if self.offset >= self.content.len() {
            return None;
        }
        let remaining = &self.content[self.offset..];
        let (line_end, next_offset) = match remaining.find('\n') {
            Some(newline) => (self.offset + newline, self.offset + newline + 1),
            None => (self.content.len(), self.content.len()),
        };
        let mut line = self.content[self.offset..line_end].to_string();
        if line.ends_with('\r') {
            line.pop();
        }
        self.offset = next_offset;
        Some(line)
    }

    fn read_to_string(&mut self) -> String {
        if self.offset >= self.content.len() {
            return String::new();
        }
        let remaining = self.content[self.offset..].to_string();
        self.offset = self.content.len();
        remaining
    }
}

static SPIKE_HTTP_NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);
static SPIKE_HTTP_SERVERS: OnceLock<Mutex<HashMap<i64, SpikeHttpServer>>> = OnceLock::new();
static SPIKE_HTTP_REQUESTS: OnceLock<Mutex<HashMap<i64, SpikeHttpRequest>>> = OnceLock::new();
static SPIKE_TCP_NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);
static SPIKE_TCP_LISTENERS: OnceLock<Mutex<HashMap<i64, SpikeTcpListener>>> = OnceLock::new();
static SPIKE_TCP_STREAMS: OnceLock<Mutex<HashMap<i64, SpikeTcpStream>>> = OnceLock::new();
static SPIKE_TCP_RESPONSES: OnceLock<Mutex<HashMap<i64, String>>> = OnceLock::new();
static SPIKE_UDP_NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);
static SPIKE_UDP_SOCKETS: OnceLock<Mutex<HashMap<i64, SpikeUdpSocket>>> = OnceLock::new();

thread_local! {
    static SPIKE_STDIN: RefCell<SpikeStdin> = RefCell::new(SpikeStdin::default());
    // While evaluating a spawned task's expression, sleeps accumulate here
    // instead of blocking, so sibling tasks model concurrent execution: the
    // spawn records the task's virtual duration and `async_join` waits out the
    // handle's ready-at timestamp rather than re-running the task body.
    static SPIKE_VIRTUAL_SLEEP: std::cell::Cell<Option<i64>> = const { std::cell::Cell::new(None) };
    // Whether the current compilation is a debug build. Read while lowering
    // sized-integer arithmetic to decide between overflow-trapping (debug) and
    // wrapping (release) semantics.
    static I64_DEBUG_BUILD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn i64_debug_build() -> bool {
    I64_DEBUG_BUILD.with(std::cell::Cell::get)
}

/// Signed integer bounds and display name for integer types narrower than the
/// native i64 lowering width. `None` for i64/isize (no narrowing overflow) and
/// non-integer types.
fn i64_sized_signed_overflow_bounds(ty: &Type) -> Option<(i64, i64, &'static str)> {
    match ty {
        Type::Numeric(NumericType::I8) => Some((i64::from(i8::MIN), i64::from(i8::MAX), "i8")),
        Type::Numeric(NumericType::I16) => Some((i64::from(i16::MIN), i64::from(i16::MAX), "i16")),
        Type::Numeric(NumericType::I32) => Some((i64::from(i32::MIN), i64::from(i32::MAX), "i32")),
        _ => None,
    }
}

fn i64_arithmetic_word(op: ArithmeticOp) -> &'static str {
    match op {
        ArithmeticOp::Add => "addition",
        ArithmeticOp::Sub => "subtraction",
        ArithmeticOp::Mul => "multiplication",
        ArithmeticOp::Div => "division",
    }
}

pub fn compile_cranelift_hello_spike(
    program: &Program,
    capabilities: &CapabilityConfig,
    package_root: &Path,
    fs_root: &Path,
    object_path: &Path,
    binary_path: &Path,
    target: Option<&str>,
    debug: bool,
) -> Result<CraneliftCompilationMode, Diagnostic> {
    if target.is_some() {
        return Err(unsupported(
            "the cranelift backend spike currently supports only the host target",
        ));
    }
    I64_DEBUG_BUILD.with(|flag| flag.set(debug));
    if let Some(lowered) = lower_i64_exit_program(program, capabilities, package_root, fs_root) {
        axiomc_backend_cranelift::compile_i64_exit_program(lowered, object_path, binary_path)
            .map_err(|err| {
                Diagnostic::new("build", err.to_string())
                    .with_path(object_path.display().to_string())
            })?;
        return Ok(direct_native_mode(program));
    }
    if let Some(lowered) =
        lower_i64_top_level_output_program(program, capabilities, package_root, fs_root)
    {
        axiomc_backend_cranelift::compile_i64_exit_program(lowered, object_path, binary_path)
            .map_err(|err| {
                Diagnostic::new("build", err.to_string())
                    .with_path(object_path.display().to_string())
            })?;
        return Ok(direct_native_mode(program));
    }
    if program.stmts.is_empty()
        && program
            .functions
            .iter()
            .any(|function| function.source_name == "main" && function.params.is_empty())
    {
        return Err(runtime_lowering_required());
    }
    if !allows_static_output_evaluation(program, capabilities) {
        return Err(runtime_lowering_required());
    }
    let output = collect_output_program(program, capabilities, package_root, fs_root, None)?;
    axiomc_backend_cranelift::compile_output_lines_with_exit_code(
        &output.lines,
        output.exit_code,
        object_path,
        binary_path,
    )
    .map_err(|err| {
        Diagnostic::new("build", err.to_string()).with_path(object_path.display().to_string())
    })?;
    Ok(CraneliftCompilationMode::BoundedStaticOutput)
}

fn program_uses_known_value_folds(program: &Program) -> bool {
    if program
        .statics
        .iter()
        .any(|static_def| expr_uses_known_value_fold(&static_def.expr))
        || stmts_use_known_value_folds(&program.stmts)
    {
        return true;
    }
    let functions = program
        .functions
        .iter()
        .map(|function| (function.name.as_str(), function))
        .collect::<HashMap<_, _>>();
    let mut pending = Vec::new();
    collect_stmt_calls(&program.stmts, &mut pending);
    pending.extend(
        program
            .functions
            .iter()
            .filter(|function| function.source_name == "main" && function.params.is_empty())
            .map(|function| function.name.clone()),
    );
    let mut visited = HashSet::new();
    while let Some(name) = pending.pop() {
        if !visited.insert(name.clone()) {
            continue;
        }
        let Some(function) = functions.get(name.as_str()) else {
            continue;
        };
        if stmts_use_known_value_folds(&function.body) {
            return true;
        }
        collect_stmt_calls(&function.body, &mut pending);
    }
    false
}

fn collect_stmt_calls(stmts: &[Stmt], calls: &mut Vec<String>) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { expr, .. }
            | Stmt::Assign { expr, .. }
            | Stmt::Print { expr, .. }
            | Stmt::Panic { message: expr, .. }
            | Stmt::Defer { expr, .. }
            | Stmt::Return { expr, .. } => collect_expr_calls(expr, calls),
            Stmt::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                collect_expr_calls(cond, calls);
                collect_stmt_calls(then_block, calls);
                if let Some(block) = else_block {
                    collect_stmt_calls(block, calls);
                }
            }
            Stmt::While { cond, body, .. } => {
                collect_expr_calls(cond, calls);
                collect_stmt_calls(body, calls);
            }
            Stmt::Match { expr, arms, .. } => {
                collect_expr_calls(expr, calls);
                for arm in arms {
                    collect_stmt_calls(&arm.body, calls);
                }
            }
        }
    }
}

fn collect_expr_calls(expr: &Expr, calls: &mut Vec<String>) {
    match expr {
        Expr::Call { name, args, .. } => {
            calls.push(name.clone());
            for arg in args {
                collect_expr_calls(arg, calls);
            }
        }
        Expr::BinaryAdd { lhs, rhs, .. }
        | Expr::BinaryCompare { lhs, rhs, .. }
        | Expr::BinaryLogic { lhs, rhs, .. } => {
            collect_expr_calls(lhs, calls);
            collect_expr_calls(rhs, calls);
        }
        Expr::Cast { expr, .. }
        | Expr::MutBorrow { expr, .. }
        | Expr::Deref { expr, .. }
        | Expr::Try { expr, .. }
        | Expr::Await { expr, .. }
        | Expr::Closure { body: expr, .. }
        | Expr::FieldAccess { base: expr, .. }
        | Expr::TupleIndex { base: expr, .. }
        | Expr::StringBorrow { expr, .. } => collect_expr_calls(expr, calls),
        Expr::StructLiteral { fields, .. } => {
            for field in fields {
                collect_expr_calls(&field.expr, calls);
            }
        }
        Expr::TupleLiteral { elements, .. } | Expr::ArrayLiteral { elements, .. } => {
            for element in elements {
                collect_expr_calls(element, calls);
            }
        }
        Expr::MapLiteral { entries, .. } => {
            for entry in entries {
                collect_expr_calls(&entry.key, calls);
                collect_expr_calls(&entry.value, calls);
            }
        }
        Expr::EnumVariant { payloads, .. } => {
            for payload in payloads {
                collect_expr_calls(payload, calls);
            }
        }
        Expr::Slice {
            base, start, end, ..
        } => {
            collect_expr_calls(base, calls);
            if let Some(expr) = start {
                collect_expr_calls(expr, calls);
            }
            if let Some(expr) = end {
                collect_expr_calls(expr, calls);
            }
        }
        Expr::Index { base, index, .. } => {
            collect_expr_calls(base, calls);
            collect_expr_calls(index, calls);
        }
        Expr::Match { expr, arms, .. } => {
            collect_expr_calls(expr, calls);
            for arm in arms {
                collect_expr_calls(&arm.expr, calls);
            }
        }
        Expr::Literal(_) | Expr::VarRef { .. } => {}
    }
}

fn stmts_use_known_value_folds(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|stmt| match stmt {
        Stmt::Let { expr, .. }
        | Stmt::Assign { expr, .. }
        | Stmt::Print { expr, .. }
        | Stmt::Panic { message: expr, .. }
        | Stmt::Defer { expr, .. }
        | Stmt::Return { expr, .. } => expr_uses_known_value_fold(expr),
        Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_known_value_fold(cond)
                || stmts_use_known_value_folds(then_block)
                || else_block
                    .as_deref()
                    .is_some_and(stmts_use_known_value_folds)
        }
        Stmt::While { cond, body, .. } => {
            expr_uses_known_value_fold(cond) || stmts_use_known_value_folds(body)
        }
        Stmt::Match { expr, arms, .. } => {
            expr_uses_known_value_fold(expr)
                || arms
                    .iter()
                    .any(|arm| stmts_use_known_value_folds(&arm.body))
        }
    })
}

fn expr_uses_known_value_fold(expr: &Expr) -> bool {
    match expr {
        Expr::Call { name, args, .. } => {
            known_value_fold_call(name) || args.iter().any(expr_uses_known_value_fold)
        }
        Expr::BinaryAdd { lhs, rhs, .. }
        | Expr::BinaryCompare { lhs, rhs, .. }
        | Expr::BinaryLogic { lhs, rhs, .. } => {
            expr_uses_known_value_fold(lhs) || expr_uses_known_value_fold(rhs)
        }
        Expr::Cast { expr, .. }
        | Expr::MutBorrow { expr, .. }
        | Expr::Deref { expr, .. }
        | Expr::Try { expr, .. }
        | Expr::Await { expr, .. }
        | Expr::Closure { body: expr, .. }
        | Expr::FieldAccess { base: expr, .. }
        | Expr::TupleIndex { base: expr, .. }
        | Expr::StringBorrow { expr, .. } => expr_uses_known_value_fold(expr),
        Expr::StructLiteral { fields, .. } => fields
            .iter()
            .any(|field| expr_uses_known_value_fold(&field.expr)),
        Expr::TupleLiteral { elements, .. } | Expr::ArrayLiteral { elements, .. } => {
            elements.iter().any(expr_uses_known_value_fold)
        }
        Expr::MapLiteral { entries, .. } => entries.iter().any(|entry| {
            expr_uses_known_value_fold(&entry.key) || expr_uses_known_value_fold(&entry.value)
        }),
        Expr::EnumVariant { payloads, .. } => payloads.iter().any(expr_uses_known_value_fold),
        Expr::Slice {
            base, start, end, ..
        } => {
            expr_uses_known_value_fold(base)
                || start.as_deref().is_some_and(expr_uses_known_value_fold)
                || end.as_deref().is_some_and(expr_uses_known_value_fold)
        }
        Expr::Index { base, index, .. } => {
            expr_uses_known_value_fold(base) || expr_uses_known_value_fold(index)
        }
        Expr::Match { expr, arms, .. } => {
            expr_uses_known_value_fold(expr)
                || arms.iter().any(|arm| expr_uses_known_value_fold(&arm.expr))
        }
        Expr::Literal(_) | Expr::VarRef { .. } => false,
    }
}

fn lower_i64_top_level_output_program(
    program: &Program,
    capabilities: &CapabilityConfig,
    package_root: &Path,
    fs_root: &Path,
) -> Option<I64ExitProgram> {
    if program.stmts.is_empty() {
        return None;
    }
    let struct_defs = program
        .structs
        .iter()
        .map(|struct_def| (struct_def.name.as_str(), struct_def))
        .collect::<HashMap<_, _>>();
    let mut static_bindings = lower_i64_static_bindings(&program.statics)?;
    static_bindings.package_root = Some(package_root.to_path_buf());
    static_bindings.fs_root = Some(fs_root.to_path_buf());
    static_bindings.env_allowed_names = capabilities.env_vars.iter().cloned().collect();
    static_bindings.env_unrestricted = capabilities.env_unrestricted;
    static_bindings.net_unrestricted = capabilities.net && capabilities.net_hosts.is_empty();
    static_bindings.net_allowed_hosts = capabilities.net_hosts.iter().cloned().collect();
    static_bindings.fs_read_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_fs_read_wrapper(function))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.fs_write_wrappers = program
        .functions
        .iter()
        .filter_map(|function| {
            i64_std_fs_write_intrinsic(function).map(|intrinsic| {
                [
                    (function.name.clone(), intrinsic.to_string()),
                    (function.source_name.clone(), intrinsic.to_string()),
                ]
            })
        })
        .flatten()
        .collect();
    static_bindings.fs_shim_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_fs_shim_wrapper(function))
        .map(|function| function.name.clone())
        .collect();
    populate_i64_http_static_bindings(program, &mut static_bindings);
    static_bindings.has_fs_write_calls = program
        .stmts
        .iter()
        .any(|stmt| i64_stmt_has_fs_write_call(stmt, &static_bindings))
        || program
            .functions
            .iter()
            .any(|function| i64_stmts_have_fs_write_call(&function.body, &static_bindings));
    if program
        .stmts
        .iter()
        .any(|stmt| i64_stmt_has_fs_read_call(stmt, &static_bindings))
    {
        return None;
    }
    static_bindings.structs = program
        .structs
        .iter()
        .map(|struct_def| (struct_def.name.clone(), struct_def.clone()))
        .collect();
    static_bindings.enums = program
        .enums
        .iter()
        .map(|enum_def| (enum_def.name.clone(), enum_def.clone()))
        .collect();
    static_bindings.functions = program
        .functions
        .iter()
        .map(|function| (function.name.clone(), function.clone()))
        .collect();
    let fs_shim_wrappers = &static_bindings.fs_shim_wrappers;
    let helper_functions = program
        .functions
        .iter()
        .filter(|function| {
            !fs_shim_wrappers.contains(&function.name)
                && !is_i64_std_http_shim_wrapper(function)
                && is_i64_function_return_type(&function.return_ty, &struct_defs, &static_bindings)
                && function
                    .params
                    .iter()
                    .all(|param| is_i64_param_type(&param.ty, &struct_defs, &static_bindings))
        })
        .collect::<Vec<_>>();
    let helper_signatures = helper_functions
        .iter()
        .enumerate()
        .map(|(index, function)| {
            Some((
                function.name.as_str(),
                I64HelperSignature {
                    function: index,
                    params: function.params.len(),
                    returns_bool: matches!(function.return_ty, Type::Bool),
                    return_ty: function.return_ty.clone(),
                    returns: i64_return_slot_count_for_type(
                        &function.return_ty,
                        &struct_defs,
                        &static_bindings,
                    )?,
                    struct_fields: function
                        .params
                        .iter()
                        .map(|param| match &param.ty {
                            Type::Struct(name) => Some(
                                i64_scalar_struct_def(name, &struct_defs)?
                                    .fields
                                    .iter()
                                    .map(|field| field.name.clone())
                                    .collect(),
                            ),
                            _ => None,
                        })
                        .collect(),
                },
            ))
        })
        .collect::<Option<HashMap<_, _>>>()?;
    let functions = helper_functions
        .iter()
        .map(|function| {
            lower_i64_function(function, &helper_signatures, &static_bindings, &struct_defs)
        })
        .collect::<Option<Vec<_>>>()?;
    let mut locals = Vec::new();
    let stmts = lower_i64_runtime_stmts(
        &program.stmts,
        &mut locals,
        HashMap::new(),
        HashMap::new(),
        &helper_signatures,
        &static_bindings,
        true,
    )?;
    Some(I64ExitProgram {
        functions,
        locals,
        stmts,
        body: I64ExitBody::Return(CraneliftI64Expr::Literal(0)),
    })
}

fn lower_i64_exit_program(
    program: &Program,
    capabilities: &CapabilityConfig,
    package_root: &Path,
    fs_root: &Path,
) -> Option<I64ExitProgram> {
    if !program.stmts.is_empty() {
        return lower_i64_top_level_runtime_stmts(&program.stmts);
    }
    if let Some(main) = program.functions.iter().find(|function| {
        function.source_name == "main"
            && function.params.is_empty()
            && !function.is_property
            && !function.is_async
            && !function.is_extern
    }) && let Some(program) = lower_i64_top_level_runtime_stmts(&main.body)
    {
        return Some(program);
    }
    let main = program.functions.iter().find(|function| {
        function.source_name == "main"
            && function.params.is_empty()
            && is_i64_exit_type(&function.return_ty)
            && !function.is_property
            && !function.is_async
            && !function.is_extern
    })?;
    let struct_defs = program
        .structs
        .iter()
        .map(|struct_def| (struct_def.name.as_str(), struct_def))
        .collect::<HashMap<_, _>>();
    let mut static_bindings = lower_i64_static_bindings(&program.statics)?;
    static_bindings.package_root = Some(package_root.to_path_buf());
    static_bindings.fs_root = Some(fs_root.to_path_buf());
    static_bindings.env_allowed_names = capabilities.env_vars.iter().cloned().collect();
    static_bindings.env_unrestricted = capabilities.env_unrestricted;
    static_bindings.net_unrestricted = capabilities.net && capabilities.net_hosts.is_empty();
    static_bindings.net_allowed_hosts = capabilities.net_hosts.iter().cloned().collect();
    static_bindings.process_status_wrappers = program
        .functions
        .iter()
        .filter(|function| {
            function.path == "<stdlib>/process.ax" && function.source_name == "run_status"
        })
        .map(|function| function.name.clone())
        .collect();
    static_bindings.env_get_wrappers = program
        .functions
        .iter()
        .filter(|function| function.path == "<stdlib>/env.ax" && function.source_name == "get_env")
        .map(|function| function.name.clone())
        .collect();
    static_bindings.time_wrappers = program
        .functions
        .iter()
        .filter(|function| function.path == "<stdlib>/time.ax")
        .map(|function| function.name.clone())
        .collect();
    static_bindings.time_now_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_time_wrapper(function, "now"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.time_now_ms_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_time_wrapper(function, "now_ms"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.time_elapsed_ms_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_time_wrapper(function, "elapsed_ms"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.time_duration_ms_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_time_wrapper(function, "duration_ms"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.time_sleep_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_time_wrapper(function, "sleep"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.fs_read_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_fs_read_wrapper(function))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.fs_write_wrappers = program
        .functions
        .iter()
        .filter_map(|function| {
            i64_std_fs_write_intrinsic(function).map(|intrinsic| {
                [
                    (function.name.clone(), intrinsic.to_string()),
                    (function.source_name.clone(), intrinsic.to_string()),
                ]
            })
        })
        .flatten()
        .collect();
    static_bindings.fs_shim_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_fs_shim_wrapper(function))
        .map(|function| function.name.clone())
        .collect();
    static_bindings.has_fs_write_calls = program
        .functions
        .iter()
        .any(|function| i64_stmts_have_fs_write_call(&function.body, &static_bindings));
    static_bindings.net_shim_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_net_shim_wrapper(function))
        .map(|function| function.name.clone())
        .collect();
    static_bindings.net_resolve_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_net_wrapper(function, "resolve"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    populate_i64_http_static_bindings(program, &mut static_bindings);
    static_bindings.collection_wrappers = program
        .functions
        .iter()
        .filter(|function| function.path == "<stdlib>/collections.ax")
        .map(|function| function.name.clone())
        .collect();
    static_bindings.collection_contains_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_collection_wrapper(function, "contains"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.collection_get_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_collection_wrapper(function, "get"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.collection_get_or_default_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_collection_wrapper(function, "get_or_default"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.collection_keys_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_collection_wrapper(function, "keys"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.regex_wrappers = program
        .functions
        .iter()
        .filter(|function| function.path == "<stdlib>/regex.ax")
        .map(|function| function.name.clone())
        .collect();
    static_bindings.regex_is_match_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_regex_wrapper(function, "is_match"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.regex_find_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_regex_wrapper(function, "find"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.regex_replace_all_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_regex_wrapper(function, "replace_all"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.encoding_wrappers = program
        .functions
        .iter()
        .filter(|function| function.path == "<stdlib>/encoding.ax")
        .map(|function| function.name.clone())
        .collect();
    static_bindings.encoding_url_component_encode_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_encoding_wrapper(function, "url_component_encode"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.encoding_url_component_decode_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_encoding_wrapper(function, "url_component_decode"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.encoding_path_segment_encode_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_encoding_wrapper(function, "path_segment_encode"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.encoding_url_query_pair_encode_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_encoding_wrapper(function, "query_pair_encode"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.encoding_path_join_segment_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_encoding_wrapper(function, "path_join_segment"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.json_wrappers = program
        .functions
        .iter()
        .filter(|function| function.path == "<stdlib>/json.ax")
        .map(|function| function.name.clone())
        .collect();
    static_bindings.json_parse_int_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_json_wrapper(function, "parse_int"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.json_parse_bool_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_json_wrapper(function, "parse_bool"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.json_parse_string_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_json_wrapper(function, "parse_string"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.json_parse_field_int_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_json_wrapper(function, "parse_field_int"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.json_parse_field_bool_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_json_wrapper(function, "parse_field_bool"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.json_parse_field_string_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_json_wrapper(function, "parse_field_string"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.json_stringify_int_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_json_wrapper(function, "stringify_int"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.json_stringify_bool_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_json_wrapper(function, "stringify_bool"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.json_stringify_string_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_json_wrapper(function, "stringify_string"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.io_eprintln_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_io_wrapper(function, "eprintln"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.io_readline_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_io_wrapper(function, "readline"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.io_read_to_string_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_io_wrapper(function, "read_to_string"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.log_wrappers = program
        .functions
        .iter()
        .filter(|function| function.path == "<stdlib>/log.ax")
        .map(|function| function.name.clone())
        .collect();
    static_bindings.log_field_string_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_log_wrapper(function, "field_string"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.log_field_int_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_log_wrapper(function, "field_int"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.log_field_bool_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_log_wrapper(function, "field_bool"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.log_fields2_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_log_wrapper(function, "fields2"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.log_fields3_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_log_wrapper(function, "fields3"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.log_event_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_log_wrapper(function, "event"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.log_info_attrs_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_log_wrapper(function, "info_attrs"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    for (source_name, level) in [
        ("debug", "debug"),
        ("info", "info"),
        ("warn", "warn"),
        ("error", "error"),
    ] {
        for function in program
            .functions
            .iter()
            .filter(|function| is_i64_std_log_wrapper(function, source_name))
        {
            static_bindings
                .log_level_wrappers
                .insert(function.name.clone(), level.to_string());
            static_bindings
                .log_level_wrappers
                .insert(function.source_name.clone(), level.to_string());
        }
    }
    static_bindings.string_builder_wrappers = program
        .functions
        .iter()
        .filter(|function| function.path == "<stdlib>/string_builder.ax")
        .map(|function| function.name.clone())
        .collect();
    static_bindings.string_builder_new_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_string_builder_wrapper(function, "builder"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.string_builder_from_string_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_string_builder_wrapper(function, "from_string"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.string_builder_push_str_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_string_builder_wrapper(function, "push_str"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.string_builder_push_line_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_string_builder_wrapper(function, "push_line"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.string_builder_finish_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_string_builder_wrapper(function, "finish"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.crypto_wrappers = program
        .functions
        .iter()
        .filter(|function| {
            is_i64_std_crypto_wrapper(function, "sha256")
                || is_i64_std_crypto_wrapper(function, "hmac_sha256")
                || is_i64_std_crypto_wrapper(function, "hmac_sha512")
                || is_i64_std_crypto_wrapper(function, "constant_time_eq")
                || is_i64_std_crypto_wrapper(function, "constant_time_eq_u8")
                || is_i64_std_crypto_wrapper(function, "verify_sha256")
                || is_i64_std_crypto_wrapper(function, "verify_sha512")
                || is_i64_std_crypto_wrapper(function, "random_bytes")
                || is_i64_std_crypto_wrapper(function, "random_u64")
                || function.path == "<stdlib>/crypto_rand.ax"
        })
        .map(|function| function.name.clone())
        .collect();
    static_bindings.crypto_sha256_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_crypto_wrapper(function, "sha256"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.crypto_hmac_sha256_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_crypto_wrapper(function, "hmac_sha256"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.crypto_hmac_sha512_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_crypto_wrapper(function, "hmac_sha512"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.crypto_constant_time_eq_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_crypto_wrapper(function, "constant_time_eq"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.crypto_constant_time_eq_u8_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_crypto_wrapper(function, "constant_time_eq_u8"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.crypto_verify_sha256_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_crypto_wrapper(function, "verify_sha256"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.crypto_verify_sha512_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_crypto_wrapper(function, "verify_sha512"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.crypto_random_bytes_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_crypto_wrapper(function, "random_bytes"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.crypto_random_u64_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_crypto_wrapper(function, "random_u64"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.ffi_strlen_symbols = program
        .functions
        .iter()
        .filter(|function| is_i64_supported_strlen_extern(function))
        .map(|function| function.name.clone())
        .collect();
    static_bindings.sync_once_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_sync_wrapper(function, "once"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.sync_once_with_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_sync_wrapper(function, "once_with"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.sync_once_is_set_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_sync_wrapper(function, "once_is_set"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.sync_once_take_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_sync_wrapper(function, "once_take"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.sync_channel_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_sync_wrapper(function, "channel"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.sync_send_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_sync_wrapper(function, "send"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.sync_try_recv_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_sync_wrapper(function, "try_recv"))
        .flat_map(|function| [function.name.clone(), function.source_name.clone()])
        .collect();
    static_bindings.structs = program
        .structs
        .iter()
        .map(|struct_def| (struct_def.name.clone(), struct_def.clone()))
        .collect();
    static_bindings.enums = program
        .enums
        .iter()
        .map(|enum_def| (enum_def.name.clone(), enum_def.clone()))
        .collect();
    static_bindings.functions = program
        .functions
        .iter()
        .filter(|function| function.name != main.name)
        .map(|function| (function.name.clone(), function.clone()))
        .collect();
    let process_status_wrappers = &static_bindings.process_status_wrappers;
    let env_get_wrappers = &static_bindings.env_get_wrappers;
    let time_wrappers = &static_bindings.time_wrappers;
    let fs_shim_wrappers = &static_bindings.fs_shim_wrappers;
    let net_shim_wrappers = &static_bindings.net_shim_wrappers;
    let http_shim_wrappers = &static_bindings.http_shim_wrappers;
    let collection_wrappers = &static_bindings.collection_wrappers;
    let regex_wrappers = &static_bindings.regex_wrappers;
    let encoding_wrappers = &static_bindings.encoding_wrappers;
    let json_wrappers = &static_bindings.json_wrappers;
    let log_wrappers = &static_bindings.log_wrappers;
    let string_builder_wrappers = &static_bindings.string_builder_wrappers;
    let crypto_wrappers = &static_bindings.crypto_wrappers;
    let ffi_strlen_symbols = &static_bindings.ffi_strlen_symbols;
    let sync_once_wrappers = &static_bindings.sync_once_wrappers;
    let sync_once_with_wrappers = &static_bindings.sync_once_with_wrappers;
    let sync_once_is_set_wrappers = &static_bindings.sync_once_is_set_wrappers;
    let sync_once_take_wrappers = &static_bindings.sync_once_take_wrappers;
    let sync_channel_wrappers = &static_bindings.sync_channel_wrappers;
    let sync_send_wrappers = &static_bindings.sync_send_wrappers;
    let sync_try_recv_wrappers = &static_bindings.sync_try_recv_wrappers;
    let helper_functions = program
        .functions
        .iter()
        .filter(|function| {
            function.name != main.name
                && !process_status_wrappers.contains(&function.name)
                && !env_get_wrappers.contains(&function.name)
                && !time_wrappers.contains(&function.name)
                && !fs_shim_wrappers.contains(&function.name)
                && !net_shim_wrappers.contains(&function.name)
                && !http_shim_wrappers.contains(&function.name)
                && !collection_wrappers.contains(&function.name)
                && !regex_wrappers.contains(&function.name)
                && !encoding_wrappers.contains(&function.name)
                && !json_wrappers.contains(&function.name)
                && !log_wrappers.contains(&function.name)
                && !string_builder_wrappers.contains(&function.name)
                && !crypto_wrappers.contains(&function.name)
                && !ffi_strlen_symbols.contains(&function.name)
                && !sync_once_wrappers.contains(&function.name)
                && !sync_once_with_wrappers.contains(&function.name)
                && !sync_once_is_set_wrappers.contains(&function.name)
                && !sync_once_take_wrappers.contains(&function.name)
                && !sync_channel_wrappers.contains(&function.name)
                && !sync_send_wrappers.contains(&function.name)
                && !sync_try_recv_wrappers.contains(&function.name)
                && is_i64_function_return_type(&function.return_ty, &struct_defs, &static_bindings)
                && function
                    .params
                    .iter()
                    .all(|param| is_i64_param_type(&param.ty, &struct_defs, &static_bindings))
        })
        .collect::<Vec<_>>();
    let helper_signatures = helper_functions
        .iter()
        .enumerate()
        .map(|(index, function)| {
            Some((
                function.name.as_str(),
                I64HelperSignature {
                    function: index,
                    params: function.params.len(),
                    returns_bool: matches!(function.return_ty, Type::Bool),
                    return_ty: function.return_ty.clone(),
                    returns: i64_return_slot_count_for_type(
                        &function.return_ty,
                        &struct_defs,
                        &static_bindings,
                    )?,
                    struct_fields: function
                        .params
                        .iter()
                        .map(|param| match &param.ty {
                            Type::Struct(name) => Some(
                                i64_scalar_struct_def(name, &struct_defs)?
                                    .fields
                                    .iter()
                                    .map(|field| field.name.clone())
                                    .collect(),
                            ),
                            _ => None,
                        })
                        .collect(),
                },
            ))
        })
        .collect::<Option<HashMap<_, _>>>()?;
    let functions = helper_functions
        .iter()
        .map(|function| {
            lower_i64_function(function, &helper_signatures, &static_bindings, &struct_defs)
        })
        .collect::<Option<Vec<_>>>()?;
    let (locals, stmts, body) = lower_i64_body(
        &main.params,
        &main.body,
        &helper_signatures,
        &static_bindings,
        &struct_defs,
        true,
        true,
    )?;
    Some(I64ExitProgram {
        functions,
        locals,
        stmts,
        body,
    })
}

fn lower_i64_top_level_runtime_stmts(source_stmts: &[Stmt]) -> Option<I64ExitProgram> {
    let mut stmts = Vec::new();
    let mut cli_arrays = HashSet::new();
    let mut stdin_remaining = HashSet::new();
    for stmt in source_stmts {
        match stmt {
            Stmt::Print {
                expr: Expr::Call { name, args, .. },
                ..
            } if is_i64_top_level_cli_arg_count_name(name) && args.is_empty() => {
                stmts.push(CraneliftI64Stmt::WriteArgCountLine {
                    stream: OutputStream::Stdout,
                });
            }
            Stmt::Match { expr, arms, .. } => {
                if let Some(stmt) = lower_i64_top_level_cli_arg_match(expr, arms) {
                    stmts.push(stmt);
                } else if let Some(stmt) = lower_i64_top_level_readline_match(expr, arms) {
                    stmts.push(stmt);
                } else {
                    return None;
                }
            }
            Stmt::Let {
                name,
                expr:
                    Expr::Call {
                        name: call_name,
                        args,
                        ..
                    },
                ..
            } if is_i64_top_level_cli_args_name(call_name) && args.is_empty() => {
                cli_arrays.insert(name.clone());
            }
            Stmt::Let {
                name,
                expr:
                    Expr::Call {
                        name: call_name,
                        args,
                        ..
                    },
                ..
            } if is_i64_top_level_io_read_to_string_name(call_name) && args.is_empty() => {
                stdin_remaining.insert(name.clone());
            }
            Stmt::Print {
                expr: Expr::Call { name, args, .. },
                ..
            } if name == "first"
                && matches!(args.as_slice(), [Expr::VarRef { name, .. }] if cli_arrays.contains(name)) =>
            {
                stmts.push(CraneliftI64Stmt::WriteArgLine {
                    stream: OutputStream::Stdout,
                    index: 0,
                    fallback: String::new(),
                });
            }
            Stmt::Print {
                expr: Expr::VarRef { name, .. },
                ..
            } if stdin_remaining.contains(name) => {
                stmts.push(CraneliftI64Stmt::WriteStdinRemaining {
                    stream: OutputStream::Stdout,
                    max_bytes: I64_STDIN_BUFFER_BYTES,
                    append_newline: true,
                });
            }
            Stmt::Print {
                expr: Expr::Literal(LiteralValue::String(text) | LiteralValue::Str(text)),
                ..
            } => stmts.push(CraneliftI64Stmt::WriteLine {
                stream: OutputStream::Stdout,
                text: text.clone(),
            }),
            Stmt::Print {
                expr: Expr::Literal(LiteralValue::Int(value)),
                ..
            } => stmts.push(CraneliftI64Stmt::WriteI64Line {
                stream: OutputStream::Stdout,
                value: CraneliftI64Expr::Literal(*value),
            }),
            Stmt::Return {
                expr: Expr::Literal(LiteralValue::Int(0)),
                ..
            } => {}
            _ => return None,
        }
    }
    Some(I64ExitProgram {
        functions: Vec::new(),
        locals: Vec::new(),
        stmts,
        body: I64ExitBody::Return(CraneliftI64Expr::Literal(0)),
    })
}

fn lower_i64_top_level_cli_arg_match(expr: &Expr, arms: &[MatchArm]) -> Option<CraneliftI64Stmt> {
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if !is_i64_top_level_cli_arg_name(name) {
        return None;
    }
    let [Expr::Literal(LiteralValue::Int(index))] = args.as_slice() else {
        return None;
    };
    let index = usize::try_from(*index).ok()?;
    let (_some_arm, none_arm) = i64_option_stmt_match_arms(arms)?;
    Some(CraneliftI64Stmt::WriteArgLine {
        stream: OutputStream::Stdout,
        index,
        fallback: i64_single_printed_static_text(&none_arm.body)?,
    })
}

fn lower_i64_top_level_readline_match(expr: &Expr, arms: &[MatchArm]) -> Option<CraneliftI64Stmt> {
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if !is_i64_top_level_io_readline_name(name) || !args.is_empty() {
        return None;
    }
    let (_some_arm, none_arm) = i64_option_stmt_match_arms(arms)?;
    Some(CraneliftI64Stmt::WriteStdinLine {
        stream: OutputStream::Stdout,
        fallback: i64_single_printed_static_text(&none_arm.body)?,
        max_bytes: I64_STDIN_BUFFER_BYTES,
    })
}

fn i64_single_printed_static_text(stmts: &[Stmt]) -> Option<String> {
    let [Stmt::Print { expr, .. }] = stmts else {
        return None;
    };
    match expr {
        Expr::Literal(LiteralValue::String(text) | LiteralValue::Str(text)) => Some(text.clone()),
        _ => None,
    }
}

fn is_i64_top_level_cli_args_name(name: &str) -> bool {
    matches!(name, "cli_args" | "std_cli_args")
}

fn is_i64_top_level_cli_arg_count_name(name: &str) -> bool {
    matches!(name, "cli_arg_count" | "std_cli_arg_count")
}

fn is_i64_top_level_cli_arg_name(name: &str) -> bool {
    matches!(name, "cli_arg" | "std_cli_arg")
}

fn is_i64_top_level_io_readline_name(name: &str) -> bool {
    matches!(name, "io_readline" | "std_io_readline")
}

fn is_i64_top_level_io_read_to_string_name(name: &str) -> bool {
    matches!(name, "io_read_to_string" | "std_io_read_to_string")
}

fn lower_i64_function(
    function: &Function,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    struct_defs: &I64StructDefs<'_>,
) -> Option<CraneliftI64Function> {
    if !is_i64_function_return_type(&function.return_ty, struct_defs, static_bindings)
        || function.is_property
        || function.is_async
        || function.is_extern
        || function
            .params
            .iter()
            .any(|param| !is_i64_param_type(&param.ty, struct_defs, static_bindings))
    {
        return None;
    }
    if let Type::Option(inner) = &function.return_ty {
        if is_i64_option_local_payload_type_static(inner, static_bindings) {
            return lower_i64_option_return_function(
                function,
                helper_signatures,
                static_bindings,
                struct_defs,
            );
        }
    }
    if let Type::Result(ok, err) = &function.return_ty {
        if is_i64_result_local_payload_type_static(ok, err, static_bindings) {
            return lower_i64_result_return_function(
                function,
                helper_signatures,
                static_bindings,
                struct_defs,
            );
        }
    }
    if let Type::Enum(enum_name) = &function.return_ty {
        if is_i64_enum_payload_type(enum_name, static_bindings) {
            return lower_i64_enum_return_function(
                function,
                helper_signatures,
                static_bindings,
                struct_defs,
            );
        }
    }
    if let Type::Tuple(elements) = &function.return_ty {
        if is_i64_tuple_param_type(elements) {
            return lower_i64_tuple_return_function(
                function,
                helper_signatures,
                static_bindings,
                struct_defs,
            );
        }
    }
    if let Type::Array(element, Some(size)) = &function.return_ty {
        if is_i64_array_param_element_type(element) {
            return lower_i64_array_return_function(
                function,
                helper_signatures,
                static_bindings,
                struct_defs,
                element.as_ref().clone(),
                *size,
            );
        }
    }
    if let Type::Struct(name) = &function.return_ty {
        if i64_scalar_struct_def(name, struct_defs).is_some() {
            return lower_i64_struct_return_function(
                function,
                helper_signatures,
                static_bindings,
                struct_defs,
            );
        }
    }
    let (locals, stmts, body) = lower_i64_body(
        &function.params,
        &function.body,
        helper_signatures,
        static_bindings,
        struct_defs,
        true,
        false,
    )?;
    Some(CraneliftI64Function {
        params: i64_abi_param_count(&function.params, struct_defs, static_bindings)?,
        returns: 1,
        locals,
        stmts,
        body: i64_scalar_value_body(body),
    })
}

fn lower_i64_array_return_function(
    function: &Function,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    struct_defs: &I64StructDefs<'_>,
    element: Type,
    size: usize,
) -> Option<CraneliftI64Function> {
    if !is_i64_array_param_element_type(&element) {
        return None;
    }
    let shape = I64AggregateReturnShape::Array { element, size };
    let (locals, stmts, body) = lower_i64_aggregate_return_body(
        &function.params,
        &function.body,
        &shape,
        helper_signatures,
        static_bindings,
        struct_defs,
    )?;
    Some(CraneliftI64Function {
        params: i64_abi_param_count(&function.params, struct_defs, static_bindings)?,
        returns: shape.slot_count(),
        locals,
        stmts,
        body,
    })
}

fn lower_i64_tuple_return_function(
    function: &Function,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    struct_defs: &I64StructDefs<'_>,
) -> Option<CraneliftI64Function> {
    let Type::Tuple(elements) = &function.return_ty else {
        return None;
    };
    if !is_i64_tuple_param_type(elements) {
        return None;
    }
    let (locals, stmts, body) = lower_i64_aggregate_return_body(
        &function.params,
        &function.body,
        &I64AggregateReturnShape::Tuple(elements.clone()),
        helper_signatures,
        static_bindings,
        struct_defs,
    )?;
    Some(CraneliftI64Function {
        params: i64_abi_param_count(&function.params, struct_defs, static_bindings)?,
        returns: elements.len(),
        locals,
        stmts,
        body,
    })
}

fn lower_i64_struct_return_function(
    function: &Function,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    struct_defs: &I64StructDefs<'_>,
) -> Option<CraneliftI64Function> {
    let Type::Struct(name) = &function.return_ty else {
        return None;
    };
    let struct_def = i64_scalar_struct_def(name, struct_defs)?;
    let shape = I64AggregateReturnShape::Struct {
        name: name.clone(),
        fields: struct_def
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.ty.clone()))
            .collect(),
    };
    let (locals, stmts, body) = lower_i64_aggregate_return_body(
        &function.params,
        &function.body,
        &shape,
        helper_signatures,
        static_bindings,
        struct_defs,
    )?;
    Some(CraneliftI64Function {
        params: i64_abi_param_count(&function.params, struct_defs, static_bindings)?,
        returns: shape.slot_count(),
        locals,
        stmts,
        body,
    })
}

fn lower_i64_option_return_function(
    function: &Function,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    struct_defs: &I64StructDefs<'_>,
) -> Option<CraneliftI64Function> {
    let Type::Option(inner) = &function.return_ty else {
        return None;
    };
    if !is_i64_option_local_payload_type_static(inner, static_bindings) {
        return None;
    }
    let shape = I64AggregateReturnShape::Option {
        inner: inner.as_ref().clone(),
        payload_slots: i64_option_payload_slot_count_static(inner.as_ref(), static_bindings)?,
    };
    let (locals, stmts, body) = lower_i64_aggregate_return_body(
        &function.params,
        &function.body,
        &shape,
        helper_signatures,
        static_bindings,
        struct_defs,
    )?;
    Some(CraneliftI64Function {
        params: i64_abi_param_count(&function.params, struct_defs, static_bindings)?,
        returns: shape.slot_count(),
        locals,
        stmts,
        body,
    })
}

fn lower_i64_result_return_function(
    function: &Function,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    struct_defs: &I64StructDefs<'_>,
) -> Option<CraneliftI64Function> {
    let Type::Result(ok, err) = &function.return_ty else {
        return None;
    };
    if !is_i64_result_local_payload_type_static(ok, err, static_bindings) {
        return None;
    }
    let shape = I64AggregateReturnShape::Result {
        ok: ok.as_ref().clone(),
        err: err.as_ref().clone(),
        payload_slots: i64_result_payload_slot_count_static(ok.as_ref(), static_bindings)?.max(
            i64_result_payload_slot_count_static(err.as_ref(), static_bindings)?,
        ),
    };
    let (locals, stmts, body) = lower_i64_aggregate_return_body(
        &function.params,
        &function.body,
        &shape,
        helper_signatures,
        static_bindings,
        struct_defs,
    )?;
    Some(CraneliftI64Function {
        params: i64_abi_param_count(&function.params, struct_defs, static_bindings)?,
        returns: shape.slot_count(),
        locals,
        stmts,
        body,
    })
}

fn lower_i64_enum_return_function(
    function: &Function,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    struct_defs: &I64StructDefs<'_>,
) -> Option<CraneliftI64Function> {
    let Type::Enum(name) = &function.return_ty else {
        return None;
    };
    if !is_i64_enum_payload_type(name, static_bindings) {
        return None;
    }
    let shape = I64AggregateReturnShape::Enum {
        name: name.clone(),
        payload_slots: i64_enum_payload_slot_count(name, static_bindings)?,
    };
    let (locals, stmts, body) = lower_i64_aggregate_return_body(
        &function.params,
        &function.body,
        &shape,
        helper_signatures,
        static_bindings,
        struct_defs,
    )?;
    Some(CraneliftI64Function {
        params: i64_abi_param_count(&function.params, struct_defs, static_bindings)?,
        returns: shape.slot_count(),
        locals,
        stmts,
        body,
    })
}

fn lower_i64_aggregate_return_body(
    params: &[crate::mir::Param],
    body: &[Stmt],
    shape: &I64AggregateReturnShape,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    struct_defs: &I64StructDefs<'_>,
) -> Option<(
    Vec<CraneliftI64Expr>,
    Vec<CraneliftI64Stmt>,
    CraneliftI64ValueBody,
)> {
    let (return_stmt, body_stmts) = body.split_last()?;
    let mut locals = Vec::new();
    let mut lowered_stmts = Vec::new();
    let mut seen_runtime_stmt = false;
    let mut static_bindings = static_bindings.clone();
    let static_bindings = &mut static_bindings;
    let (mut local_indexes, mut local_conditions) =
        i64_param_local_bindings(params, struct_defs, static_bindings)?;
    for stmt in body_stmts {
        match stmt {
            Stmt::Let {
                ty,
                expr: Expr::Index { base, .. },
                ..
            } if !seen_runtime_stmt
                && (matches!(ty, Type::Bool) || is_i64_compatible_type(ty))
                && i64_is_helper_call_slice_expr(base.as_ref()) =>
            {
                lowered_stmts.extend(lower_i64_helper_call_slice_index_let_stmts(
                    stmt,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?);
                seen_runtime_stmt = true;
            }
            Stmt::Let { ty, expr, .. }
                if !seen_runtime_stmt
                    && (matches!(ty, Type::Bool) || is_i64_compatible_type(ty))
                    && i64_is_helper_call_array_index_expr(expr) =>
            {
                lowered_stmts.extend(lower_i64_helper_call_array_index_let_stmts(
                    stmt,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?);
                seen_runtime_stmt = true;
            }
            Stmt::Let { ty, expr, .. }
                if !seen_runtime_stmt
                    && (matches!(ty, Type::Bool) || is_i64_compatible_type(ty))
                    && i64_is_helper_call_projection_expr(expr) =>
            {
                lowered_stmts.extend(lower_i64_helper_call_projection_let_stmts(
                    stmt,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?);
                seen_runtime_stmt = true;
            }
            Stmt::Let {
                ty,
                expr: Expr::Call { args, .. },
                ..
            } if !seen_runtime_stmt
                && (matches!(ty, Type::Bool) || is_i64_compatible_type(ty))
                && (i64_has_helper_call_slice_index_arg(args)
                    || i64_has_helper_call_array_index_arg(args)
                    || i64_has_helper_call_projection_arg(args)) =>
            {
                lowered_stmts.extend(lower_i64_nested_aggregate_call_let_stmts(
                    stmt,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?);
                seen_runtime_stmt = true;
            }
            Stmt::Let { name, ty, expr, .. }
                if is_i64_compatible_type(ty) && !seen_runtime_stmt =>
            {
                if lower_i64_http_server_listen_local(
                    name,
                    expr,
                    &mut locals,
                    &mut local_indexes,
                    &mut *static_bindings,
                )
                .is_some()
                {
                    seen_runtime_stmt = true;
                    continue;
                }
                if let Some(stmts) = lower_i64_eprintln_let_stmts(
                    stmt,
                    &mut locals,
                    &mut local_indexes,
                    &local_conditions,
                    helper_signatures,
                    static_bindings,
                ) {
                    lowered_stmts.extend(stmts);
                    seen_runtime_stmt = true;
                    continue;
                }
                let local_expr = lower_i64_expr(
                    expr,
                    &local_indexes,
                    &local_conditions,
                    helper_signatures,
                    static_bindings,
                )?;
                local_indexes.insert(name.clone(), local_indexes.len());
                locals.push(local_expr);
            }
            Stmt::Let {
                name,
                ty: Type::String | Type::Str,
                expr,
                ..
            } if !seen_runtime_stmt => {
                lower_i64_string_len_projection_local(
                    name,
                    expr,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    &mut *static_bindings,
                )?;
            }
            Stmt::Let {
                name,
                ty: Type::Map(_, _),
                expr: Expr::MapLiteral { entries, .. },
                ..
            } if !seen_runtime_stmt => {
                let entries = i64_static_map_literal_entries(entries, static_bindings)?;
                static_bindings.map_literals.insert(name.clone(), entries);
            }
            Stmt::Let {
                name,
                ty: Type::Array(_, None),
                expr,
                ..
            } if !seen_runtime_stmt => {
                if let Some(keys) = i64_map_keys_expr(expr, static_bindings) {
                    static_bindings.map_key_arrays.insert(name.clone(), keys);
                } else {
                    return None;
                }
            }
            Stmt::Let { name, expr, .. }
                if is_i64_known_once_call_let(expr, static_bindings) && !seen_runtime_stmt =>
            {
                lower_i64_known_once_call_let(name, expr, &mut *static_bindings)?;
            }
            Stmt::Let { name, expr, .. }
                if is_i64_known_channel_call_let(expr, static_bindings) && !seen_runtime_stmt =>
            {
                lower_i64_known_channel_call_let(name, expr, &mut *static_bindings)?;
            }
            Stmt::Let {
                name,
                ty: Type::Struct(_),
                expr: Expr::StructLiteral { fields, .. },
                ..
            } if !seen_runtime_stmt => {
                lower_i64_struct_projection_locals(
                    name,
                    fields,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?;
            }
            Stmt::Let {
                name,
                ty: Type::Struct(_),
                expr: expr @ Expr::Call {
                    name: call_name, ..
                },
                ..
            } if is_i64_string_builder_constructor_name(call_name, static_bindings)
                && !seen_runtime_stmt =>
            {
                let text = i64_string_builder_text(expr, static_bindings)?;
                static_bindings.string_builders.insert(name.clone(), text);
            }
            Stmt::Let {
                name,
                ty: Type::Struct(struct_name),
                expr:
                    Expr::Call {
                        name: call_name,
                        args,
                        ..
                    },
                ..
            } if !seen_runtime_stmt => {
                lowered_stmts.extend(lower_i64_struct_call_let_stmts(
                    name,
                    struct_name,
                    call_name,
                    args,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?);
                seen_runtime_stmt = true;
            }
            Stmt::Let {
                name,
                ty: Type::Tuple(_),
                expr: Expr::TupleLiteral { elements, .. },
                ..
            } if !seen_runtime_stmt => {
                lower_i64_indexed_projection_locals(
                    name,
                    elements,
                    i64_tuple_projection_key,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?;
            }
            Stmt::Let {
                name,
                ty: Type::Tuple(return_elements),
                expr:
                    Expr::Call {
                        name: call_name,
                        args,
                        ..
                    },
                ..
            } if is_i64_tuple_param_type(return_elements) && !seen_runtime_stmt => {
                lowered_stmts.extend(lower_i64_tuple_call_let_stmts(
                    name,
                    return_elements,
                    call_name,
                    args,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?);
                seen_runtime_stmt = true;
            }
            Stmt::Let {
                name,
                ty: Type::Array(_, _),
                expr: Expr::ArrayLiteral { elements, .. },
                ..
            } if !seen_runtime_stmt => {
                lower_i64_indexed_projection_locals(
                    name,
                    elements,
                    i64_array_projection_key,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?;
            }
            Stmt::Let {
                name,
                ty: Type::MutSlice(_),
                expr,
                ..
            } if !seen_runtime_stmt
                && record_i64_mut_slice_alias(name, expr, &local_indexes, static_bindings)
                    .is_some() => {}
            Stmt::Let {
                name,
                ty: Type::Slice(_) | Type::MutSlice(_),
                expr,
                ..
            } if !seen_runtime_stmt => {
                let assigns = lower_i64_slice_projection_aliases(
                    name,
                    expr,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                    false,
                )?;
                seen_runtime_stmt = !assigns.is_empty();
                lowered_stmts.extend(assigns);
            }
            Stmt::Let {
                name,
                ty: Type::Array(element, Some(size)),
                expr:
                    Expr::Call {
                        name: call_name,
                        args,
                        ..
                    },
                ..
            } if is_i64_array_param_element_type(element) && !seen_runtime_stmt => {
                lowered_stmts.extend(lower_i64_array_call_let_stmts(
                    name,
                    element.as_ref(),
                    *size,
                    call_name,
                    args,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?);
                seen_runtime_stmt = true;
            }
            Stmt::Let {
                name,
                ty: Type::Option(inner),
                expr: expr @ Expr::Call { .. },
                ..
            } if (is_i64_option_local_payload_type_static(inner, static_bindings)
                || is_i64_known_string_option_call_let_type(inner.as_ref()))
                && !seen_runtime_stmt =>
            {
                if let Some(assigns) = lower_i64_runtime_string_option_call_let_stmts(
                    name,
                    inner.as_ref(),
                    expr,
                    &mut locals,
                    &mut local_indexes,
                    static_bindings,
                ) {
                    lowered_stmts.extend(assigns);
                    seen_runtime_stmt = true;
                } else if let Some(assigns) = lower_i64_known_string_option_call_let_stmts(
                    name,
                    inner.as_ref(),
                    expr,
                    &mut *static_bindings,
                ) {
                    let has_runtime_stmts = !assigns.is_empty();
                    lowered_stmts.extend(assigns);
                    seen_runtime_stmt = has_runtime_stmts;
                } else {
                    lowered_stmts.extend(lower_i64_scalar_option_call_let_stmts(
                        name,
                        inner.as_ref(),
                        expr,
                        &mut locals,
                        &mut local_indexes,
                        &mut local_conditions,
                        helper_signatures,
                        static_bindings,
                    )?);
                    seen_runtime_stmt = true;
                }
            }
            Stmt::Let {
                name,
                ty: Type::Option(inner),
                expr: Expr::EnumVariant { .. },
                ..
            } if is_i64_option_local_payload_type_static(inner, static_bindings)
                && !seen_runtime_stmt =>
            {
                lower_i64_option_locals(
                    name,
                    stmt,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?;
            }
            Stmt::Let {
                name,
                ty: Type::Enum(enum_name),
                expr:
                    Expr::Call {
                        name: call_name,
                        args,
                        ..
                    },
                ..
            } if is_i64_enum_payload_type(enum_name, static_bindings) && !seen_runtime_stmt => {
                lowered_stmts.extend(lower_i64_enum_call_let_stmts(
                    name,
                    enum_name,
                    call_name,
                    args,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?);
                seen_runtime_stmt = true;
            }
            Stmt::Let {
                name,
                ty: Type::Enum(enum_name),
                expr: Expr::EnumVariant { .. },
                ..
            } if is_i64_enum_payload_type(enum_name, static_bindings) && !seen_runtime_stmt => {
                lower_i64_enum_locals(
                    name,
                    stmt,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?;
            }
            Stmt::Let {
                name,
                ty: Type::Result(ok, err),
                expr:
                    Expr::Call {
                        name: call_name,
                        args,
                        ..
                    },
                ..
            } if is_i64_result_local_payload_type_static(ok, err, static_bindings)
                && !seen_runtime_stmt =>
            {
                lowered_stmts.extend(lower_i64_result_call_let_stmts(
                    name,
                    ok.as_ref(),
                    err.as_ref(),
                    call_name,
                    args,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?);
                seen_runtime_stmt = true;
            }
            Stmt::Let {
                name,
                ty: Type::Result(ok, err),
                expr: Expr::EnumVariant { .. },
                ..
            } if is_i64_result_local_payload_type_static(ok, err, static_bindings)
                && !seen_runtime_stmt =>
            {
                lower_i64_result_locals(
                    name,
                    stmt,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?;
            }
            Stmt::Let {
                name,
                ty: Type::Bool,
                expr,
                ..
            } if !seen_runtime_stmt => {
                if let Some(diag) = i64_http_non_loopback_bind_diag(expr, static_bindings) {
                    lowered_stmts.push(CraneliftI64Stmt::WriteLine {
                        stream: OutputStream::Stderr,
                        text: diag,
                    });
                    seen_runtime_stmt = true;
                }
                let local_expr = lower_i64_bool_value_expr(
                    expr,
                    &local_indexes,
                    &local_conditions,
                    helper_signatures,
                    static_bindings,
                )?;
                let local = local_indexes.len();
                local_indexes.insert(name.clone(), local);
                locals.push(local_expr);
                local_conditions.insert(name.clone(), i64_local_truthy_condition(local));
            }
            Stmt::Let { .. } if seen_runtime_stmt => {
                lowered_stmts.extend(lower_i64_runtime_let_stmts(
                    stmt,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                    true,
                )?);
            }
            Stmt::Assign { .. }
            | Stmt::Print { .. }
            | Stmt::If { .. }
            | Stmt::While { .. }
            | Stmt::Match { .. } => {
                seen_runtime_stmt = true;
                lowered_stmts.extend(lower_i64_runtime_stmt_stmts(
                    stmt,
                    &mut locals,
                    local_indexes.clone(),
                    local_conditions.clone(),
                    helper_signatures,
                    static_bindings,
                    true,
                )?);
            }
            _ => return None,
        }
    }
    let body = match return_stmt {
        Stmt::Return { expr, .. } => {
            if let Some((mut call_stmts, results)) = lower_i64_aggregate_call_return_stmts(
                expr,
                shape,
                &mut locals,
                &mut local_indexes,
                &mut local_conditions,
                helper_signatures,
                static_bindings,
            ) {
                lowered_stmts.append(&mut call_stmts);
                CraneliftI64ValueBody::BlockReturn(CraneliftI64ValueReturnBlock {
                    stmts: std::mem::take(&mut lowered_stmts),
                    results,
                })
            } else {
                CraneliftI64ValueBody::Return(
                    lower_i64_aggregate_return_values(
                        expr,
                        shape,
                        &local_indexes,
                        &local_conditions,
                        helper_signatures,
                        static_bindings,
                    )
                    .filter(|results| results.len() == shape.slot_count())?,
                )
            }
        }
        Stmt::If {
            cond,
            then_block,
            else_block: Some(else_block),
            ..
        } => {
            let cond = if let Some((rewritten_cond, mut setup)) =
                rewrite_i64_helper_call_aggregate_condition_expr(
                    cond,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                ) {
                lowered_stmts.append(&mut setup);
                rewritten_cond
            } else {
                cond.clone()
            };
            CraneliftI64ValueBody::IfBlockReturn {
                cond: lower_i64_condition(
                    &cond,
                    &local_indexes,
                    &local_conditions,
                    helper_signatures,
                    static_bindings,
                )?,
                then_block: lower_i64_aggregate_return_block(
                    then_block,
                    shape,
                    &mut locals,
                    local_indexes.clone(),
                    local_conditions.clone(),
                    helper_signatures,
                    static_bindings,
                    true,
                )?,
                else_block: lower_i64_aggregate_return_block(
                    else_block,
                    shape,
                    &mut locals,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                    true,
                )?,
            }
        }
        _ => return None,
    };
    Some((locals, lowered_stmts, body))
}

fn lower_i64_aggregate_return_block(
    stmts: &[Stmt],
    shape: &I64AggregateReturnShape,
    locals: &mut Vec<CraneliftI64Expr>,
    mut local_indexes: HashMap<String, usize>,
    mut local_conditions: HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    allow_stdio_effects: bool,
) -> Option<CraneliftI64ValueReturnBlock> {
    let (return_stmt, body_stmts) = stmts.split_last()?;
    let Stmt::Return { expr, .. } = return_stmt else {
        return None;
    };
    let mut stmts = Vec::new();
    for stmt in body_stmts {
        if matches!(stmt, Stmt::Let { .. }) {
            stmts.extend(lower_i64_runtime_let_stmts(
                stmt,
                locals,
                &mut local_indexes,
                &mut local_conditions,
                helper_signatures,
                static_bindings,
                allow_stdio_effects,
            )?);
        } else {
            stmts.extend(lower_i64_runtime_stmt_stmts(
                stmt,
                locals,
                local_indexes.clone(),
                local_conditions.clone(),
                helper_signatures,
                static_bindings,
                allow_stdio_effects,
            )?);
        }
    }
    let results = if let Some((mut call_stmts, results)) = lower_i64_aggregate_call_return_stmts(
        expr,
        shape,
        locals,
        &mut local_indexes,
        &mut local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        stmts.append(&mut call_stmts);
        results
    } else {
        lower_i64_aggregate_return_values(
            expr,
            shape,
            &local_indexes,
            &local_conditions,
            helper_signatures,
            static_bindings,
        )?
    };
    if results.len() != shape.slot_count() {
        return None;
    }
    Some(CraneliftI64ValueReturnBlock { stmts, results })
}

impl I64AggregateReturnShape {
    fn slot_count(&self) -> usize {
        match self {
            I64AggregateReturnShape::Array { size, .. } => *size,
            I64AggregateReturnShape::Tuple(elements) => elements.len(),
            I64AggregateReturnShape::Struct { fields, .. } => fields.len(),
            I64AggregateReturnShape::Option { payload_slots, .. }
            | I64AggregateReturnShape::Result { payload_slots, .. }
            | I64AggregateReturnShape::Enum { payload_slots, .. } => 1 + payload_slots,
        }
    }

    fn ty(&self) -> Type {
        match self {
            I64AggregateReturnShape::Array { element, size } => {
                Type::Array(Box::new(element.clone()), Some(*size))
            }
            I64AggregateReturnShape::Tuple(elements) => Type::Tuple(elements.clone()),
            I64AggregateReturnShape::Struct { name, .. } => Type::Struct(name.clone()),
            I64AggregateReturnShape::Option { inner, .. } => Type::Option(Box::new(inner.clone())),
            I64AggregateReturnShape::Result { ok, err, .. } => {
                Type::Result(Box::new(ok.clone()), Box::new(err.clone()))
            }
            I64AggregateReturnShape::Enum { name, .. } => Type::Enum(name.clone()),
        }
    }
}

fn lower_i64_aggregate_call_return_stmts(
    expr: &Expr,
    shape: &I64AggregateReturnShape,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<(Vec<CraneliftI64Stmt>, Vec<CraneliftI64Expr>)> {
    if !matches!(expr, Expr::Call { .. }) {
        return None;
    }
    let ty = shape.ty();
    if expr.ty() != ty {
        return None;
    }
    let temp_name = format!("__axiom_i64_return_{}", local_indexes.len());
    let stmts = lower_i64_aggregate_call_to_local_stmts(
        &temp_name,
        &ty,
        expr,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let temp = Expr::VarRef {
        name: temp_name,
        ty,
    };
    let results = lower_i64_aggregate_return_values(
        &temp,
        shape,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    (results.len() == shape.slot_count()).then_some((stmts, results))
}

fn i64_scalar_value_body(body: I64ExitBody) -> CraneliftI64ValueBody {
    match body {
        I64ExitBody::Return(result) => CraneliftI64ValueBody::Return(vec![result]),
        I64ExitBody::BlockReturn(block) => {
            CraneliftI64ValueBody::BlockReturn(CraneliftI64ValueReturnBlock {
                stmts: block.stmts,
                results: vec![block.result],
            })
        }
        I64ExitBody::IfReturn {
            cond,
            then_result,
            else_result,
        } => CraneliftI64ValueBody::IfReturn {
            cond,
            then_results: vec![then_result],
            else_results: vec![else_result],
        },
        I64ExitBody::IfBlockReturn {
            cond,
            then_block,
            else_block,
        } => CraneliftI64ValueBody::IfBlockReturn {
            cond,
            then_block: CraneliftI64ValueReturnBlock {
                stmts: then_block.stmts,
                results: vec![then_block.result],
            },
            else_block: CraneliftI64ValueReturnBlock {
                stmts: else_block.stmts,
                results: vec![else_block.result],
            },
        },
    }
}

fn i64_param_local_bindings(
    params: &[crate::mir::Param],
    struct_defs: &I64StructDefs<'_>,
    static_bindings: &I64StaticBindings,
) -> Option<(
    HashMap<String, usize>,
    HashMap<String, CraneliftI64Condition>,
)> {
    let mut local_indexes = HashMap::new();
    let mut local_conditions = HashMap::new();
    for param in params {
        if !is_i64_param_type(&param.ty, struct_defs, static_bindings) {
            return None;
        }
        insert_i64_param_local_bindings(
            &param.name,
            &param.ty,
            &mut local_indexes,
            &mut local_conditions,
            struct_defs,
            static_bindings,
        )?;
    }
    Some((local_indexes, local_conditions))
}

fn insert_i64_param_local_bindings(
    name: &str,
    ty: &Type,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    struct_defs: &I64StructDefs<'_>,
    static_bindings: &I64StaticBindings,
) -> Option<()> {
    match ty {
        Type::Struct(name_ty) => {
            let struct_def = i64_scalar_struct_def(name_ty, struct_defs)?;
            for field in &struct_def.fields {
                let local = local_indexes.len();
                let key = i64_struct_projection_key(name, &field.name);
                local_indexes.insert(key.clone(), local);
                if matches!(field.ty, Type::Bool) {
                    local_conditions.insert(key, i64_local_truthy_condition(local));
                }
            }
        }
        Type::Tuple(elements) if is_i64_tuple_param_type(elements) => {
            for (index, element) in elements.iter().enumerate() {
                let local = local_indexes.len();
                let key = i64_tuple_projection_key(name, index);
                local_indexes.insert(key.clone(), local);
                if matches!(element, Type::Bool) {
                    local_conditions.insert(key, i64_local_truthy_condition(local));
                }
            }
        }
        Type::Array(element, Some(size)) if is_i64_array_param_element_type(element) => {
            for index in 0..*size {
                let local = local_indexes.len();
                let key = i64_array_projection_key(name, index);
                local_indexes.insert(key.clone(), local);
                if matches!(element.as_ref(), Type::Bool) {
                    local_conditions.insert(key, i64_local_truthy_condition(local));
                }
            }
        }
        Type::Option(inner) if is_i64_option_local_payload_type_static(inner, static_bindings) => {
            let tag_local = local_indexes.len();
            local_indexes.insert(i64_option_tag_key(name), tag_local);
            for index in 0..i64_option_payload_slot_count_static(inner.as_ref(), static_bindings)? {
                let payload_local = local_indexes.len();
                local_indexes.insert(i64_option_payload_slot_key(name, index), payload_local);
                if index == 0 {
                    local_indexes.insert(i64_option_payload_key(name), payload_local);
                }
            }
        }
        Type::Result(ok, err)
            if is_i64_result_local_payload_type_static(ok, err, static_bindings) =>
        {
            let tag_local = local_indexes.len();
            local_indexes.insert(i64_result_tag_key(name), tag_local);
            let slot_count =
                i64_result_payload_slot_count_static(ok.as_ref(), static_bindings)?.max(
                    i64_result_payload_slot_count_static(err.as_ref(), static_bindings)?,
                );
            for index in 0..slot_count {
                let payload_local = local_indexes.len();
                local_indexes.insert(i64_result_payload_slot_key(name, index), payload_local);
                if index == 0 {
                    local_indexes.insert(i64_result_payload_key(name), payload_local);
                }
            }
        }
        Type::Enum(enum_name) if is_i64_enum_payload_type(enum_name, static_bindings) => {
            let tag_local = local_indexes.len();
            local_indexes.insert(i64_enum_tag_key(name), tag_local);
            for index in 0..i64_enum_payload_slot_count(enum_name, static_bindings)? {
                let payload_local = local_indexes.len();
                local_indexes.insert(i64_enum_payload_slot_key(name, index), payload_local);
                if index == 0 {
                    local_indexes.insert(i64_enum_payload_key(name), payload_local);
                }
            }
        }
        _ => {
            let local = local_indexes.len();
            local_indexes.insert(name.to_string(), local);
            if matches!(ty, Type::Bool) {
                local_conditions.insert(name.to_string(), i64_local_truthy_condition(local));
            }
        }
    }
    Some(())
}

fn i64_local_truthy_condition(local: usize) -> CraneliftI64Condition {
    CraneliftI64Condition::Compare(CraneliftI64Compare {
        op: CraneliftI64CompareOp::Ne,
        lhs: CraneliftI64Expr::Local(local),
        rhs: CraneliftI64Expr::Literal(0),
    })
}

fn lower_i64_aggregate_return_values(
    expr: &Expr,
    shape: &I64AggregateReturnShape,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Expr>> {
    match (shape, expr) {
        (
            I64AggregateReturnShape::Array { element, size },
            Expr::VarRef {
                name,
                ty: Type::Array(expr_element, Some(expr_size)),
            },
        ) if expr_element.as_ref() == element
            && expr_size == size
            && is_i64_array_param_element_type(element) =>
        {
            (0..*size)
                .map(|index| {
                    local_indexes
                        .get(i64_array_projection_key(name, index).as_str())
                        .copied()
                        .map(CraneliftI64Expr::Local)
                })
                .collect()
        }
        (
            I64AggregateReturnShape::Array { element, size },
            Expr::ArrayLiteral {
                elements,
                ty: Type::Array(expr_element, Some(expr_size)),
            },
        ) => {
            if expr_element.as_ref() != element
                || expr_size != size
                || elements.len() != *size
                || !is_i64_array_param_element_type(element)
            {
                return None;
            }
            elements
                .iter()
                .map(|value| {
                    lower_i64_aggregate_return_value(
                        value,
                        element,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )
                })
                .collect()
        }
        (
            I64AggregateReturnShape::Tuple(element_tys),
            Expr::VarRef {
                name,
                ty: Type::Tuple(expr_element_tys),
            },
        ) if expr_element_tys == element_tys && is_i64_tuple_param_type(element_tys) => (0
            ..element_tys.len())
            .map(|index| {
                local_indexes
                    .get(i64_tuple_projection_key(name, index).as_str())
                    .copied()
                    .map(CraneliftI64Expr::Local)
            })
            .collect(),
        (
            I64AggregateReturnShape::Tuple(element_tys),
            Expr::TupleLiteral {
                elements,
                ty: Type::Tuple(expr_element_tys),
            },
        ) => {
            if elements.len() != element_tys.len()
                || expr_element_tys != element_tys
                || !is_i64_tuple_param_type(element_tys)
            {
                return None;
            }
            elements
                .iter()
                .zip(element_tys)
                .map(|(element, ty)| {
                    lower_i64_aggregate_return_value(
                        element,
                        ty,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )
                })
                .collect()
        }
        (
            I64AggregateReturnShape::Struct { name, fields },
            Expr::VarRef {
                name: binding,
                ty: Type::Struct(expr_name),
            },
        ) if expr_name == name => fields
            .iter()
            .map(|(field_name, _)| {
                local_indexes
                    .get(i64_struct_projection_key(binding, field_name).as_str())
                    .copied()
                    .map(CraneliftI64Expr::Local)
            })
            .collect(),
        (
            I64AggregateReturnShape::Struct { name, fields },
            Expr::StructLiteral {
                fields: literal_fields,
                ty: Type::Struct(expr_name),
                ..
            },
        ) if expr_name == name => fields
            .iter()
            .map(|(field_name, ty)| {
                let field = literal_fields
                    .iter()
                    .find(|field| field.name == *field_name)?;
                lower_i64_aggregate_return_value(
                    &field.expr,
                    ty,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )
            })
            .collect(),
        (
            I64AggregateReturnShape::Struct { name, .. },
            Expr::Call {
                name: call_name,
                args,
                ty: Type::Struct(expr_name),
            },
        ) if expr_name == name => lower_i64_time_struct_call_ms_expr(
            name,
            call_name,
            args,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )
        .map(|value| vec![value]),
        (
            I64AggregateReturnShape::Option {
                inner,
                payload_slots,
            },
            Expr::VarRef {
                name,
                ty: Type::Option(expr_inner),
            },
        ) if expr_inner.as_ref() == inner => {
            let mut results = vec![CraneliftI64Expr::Local(
                *local_indexes.get(i64_option_tag_key(name).as_str())?,
            )];
            let payloads = i64_option_payload_locals(name, inner, local_indexes, static_bindings)?;
            if payloads.len() != *payload_slots {
                return None;
            }
            results.extend(payloads.into_iter().map(CraneliftI64Expr::Local));
            Some(results)
        }
        (
            I64AggregateReturnShape::Option {
                inner,
                payload_slots,
            },
            Expr::EnumVariant {
                enum_name,
                variant,
                payloads,
                ty: Type::Option(expr_inner),
                ..
            },
        ) if enum_name == "Option" && expr_inner.as_ref() == inner => {
            let (tag, payloads) = match (variant.as_str(), payloads.as_slice()) {
                ("Some", [payload]) => (
                    CraneliftI64Expr::Literal(1),
                    lower_i64_option_payload_exprs(
                        payload,
                        *payload_slots,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )?,
                ),
                ("None", []) => (
                    CraneliftI64Expr::Literal(0),
                    vec![CraneliftI64Expr::Literal(0); *payload_slots],
                ),
                _ => return None,
            };
            let mut results = vec![tag];
            results.extend(payloads);
            Some(results)
        }
        (
            I64AggregateReturnShape::Result {
                ok,
                err,
                payload_slots,
            },
            Expr::VarRef {
                name,
                ty: Type::Result(expr_ok, expr_err),
            },
        ) if expr_ok.as_ref() == ok && expr_err.as_ref() == err => {
            let mut results = vec![CraneliftI64Expr::Local(
                *local_indexes.get(i64_result_tag_key(name).as_str())?,
            )];
            let payloads =
                i64_result_payload_locals(name, ok, err, local_indexes, static_bindings)?;
            if payloads.len() != *payload_slots {
                return None;
            }
            results.extend(payloads.into_iter().map(CraneliftI64Expr::Local));
            Some(results)
        }
        (
            I64AggregateReturnShape::Result {
                ok,
                err,
                payload_slots,
            },
            Expr::EnumVariant {
                enum_name,
                variant,
                payloads,
                ty: Type::Result(expr_ok, expr_err),
                ..
            },
        ) if enum_name == "Result" && expr_ok.as_ref() == ok && expr_err.as_ref() == err => {
            let (tag, payloads) = lower_i64_result_variant_parts(
                variant,
                payloads,
                *payload_slots,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            let mut results = vec![tag];
            results.extend(payloads);
            Some(results)
        }
        (
            I64AggregateReturnShape::Enum {
                name,
                payload_slots,
            },
            Expr::VarRef {
                name: binding,
                ty: Type::Enum(expr_name),
            },
        ) if expr_name == name => {
            let mut results = vec![CraneliftI64Expr::Local(
                *local_indexes.get(i64_enum_tag_key(binding).as_str())?,
            )];
            let payloads = i64_enum_payload_locals(binding, name, static_bindings, local_indexes)?;
            if payloads.len() != *payload_slots {
                return None;
            }
            results.extend(payloads.into_iter().map(CraneliftI64Expr::Local));
            Some(results)
        }
        (
            I64AggregateReturnShape::Enum {
                name,
                payload_slots,
            },
            Expr::EnumVariant {
                enum_name,
                variant,
                payloads,
                ty: Type::Enum(expr_name),
                ..
            },
        ) if enum_name == name && expr_name == name => {
            let (tag, payloads) = lower_i64_enum_variant_parts(
                enum_name,
                variant,
                payloads,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            if payloads.len() != *payload_slots {
                return None;
            }
            let mut results = vec![tag];
            results.extend(payloads);
            Some(results)
        }
        _ => None,
    }
}

fn lower_i64_aggregate_return_value(
    expr: &Expr,
    ty: &Type,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    match ty {
        Type::Bool => lower_i64_bool_value_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        ),
        ty if is_i64_compatible_type(ty) => {
            let expr = lower_i64_expr(
                expr,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            lower_i64_cast_expr(expr, ty)
        }
        _ => None,
    }
}

fn lower_i64_body(
    params: &[crate::mir::Param],
    stmts: &[Stmt],
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    struct_defs: &I64StructDefs<'_>,
    allow_stdio_effects: bool,
    allow_terminal_panic: bool,
) -> Option<(Vec<CraneliftI64Expr>, Vec<CraneliftI64Stmt>, I64ExitBody)> {
    let (return_stmt, body_stmts) = stmts.split_last()?;
    let mut locals = Vec::new();
    let mut local_indexes = HashMap::new();
    let mut local_conditions = HashMap::new();
    let mut lowered_stmts = Vec::new();
    let mut seen_runtime_stmt = false;
    let mut static_bindings = static_bindings.clone();
    let static_bindings = &mut static_bindings;
    for param in params {
        if !is_i64_param_type(&param.ty, struct_defs, static_bindings) {
            return None;
        }
        match &param.ty {
            Type::Struct(name) => {
                let struct_def = i64_scalar_struct_def(name, struct_defs)?;
                for field in &struct_def.fields {
                    let local = local_indexes.len();
                    let key = i64_struct_projection_key(&param.name, &field.name);
                    local_indexes.insert(key.clone(), local);
                    if matches!(field.ty, Type::Bool) {
                        local_conditions.insert(
                            key,
                            CraneliftI64Condition::Compare(CraneliftI64Compare {
                                op: CraneliftI64CompareOp::Ne,
                                lhs: CraneliftI64Expr::Local(local),
                                rhs: CraneliftI64Expr::Literal(0),
                            }),
                        );
                    }
                }
            }
            Type::Tuple(elements) if is_i64_tuple_param_type(elements) => {
                for (index, element) in elements.iter().enumerate() {
                    let local = local_indexes.len();
                    let key = i64_tuple_projection_key(&param.name, index);
                    local_indexes.insert(key.clone(), local);
                    if matches!(element, Type::Bool) {
                        local_conditions.insert(
                            key,
                            CraneliftI64Condition::Compare(CraneliftI64Compare {
                                op: CraneliftI64CompareOp::Ne,
                                lhs: CraneliftI64Expr::Local(local),
                                rhs: CraneliftI64Expr::Literal(0),
                            }),
                        );
                    }
                }
            }
            Type::Array(element, Some(size)) if is_i64_array_param_element_type(element) => {
                for index in 0..*size {
                    let local = local_indexes.len();
                    let key = i64_array_projection_key(&param.name, index);
                    local_indexes.insert(key.clone(), local);
                    if matches!(element.as_ref(), Type::Bool) {
                        local_conditions.insert(
                            key,
                            CraneliftI64Condition::Compare(CraneliftI64Compare {
                                op: CraneliftI64CompareOp::Ne,
                                lhs: CraneliftI64Expr::Local(local),
                                rhs: CraneliftI64Expr::Literal(0),
                            }),
                        );
                    }
                }
            }
            Type::Option(inner)
                if is_i64_option_local_payload_type_static(inner, static_bindings) =>
            {
                let tag_local = local_indexes.len();
                local_indexes.insert(i64_option_tag_key(&param.name), tag_local);
                for index in
                    0..i64_option_payload_slot_count_static(inner.as_ref(), static_bindings)?
                {
                    let payload_local = local_indexes.len();
                    local_indexes.insert(
                        i64_option_payload_slot_key(&param.name, index),
                        payload_local,
                    );
                    if index == 0 {
                        local_indexes.insert(i64_option_payload_key(&param.name), payload_local);
                    }
                }
            }
            Type::Result(ok, err)
                if is_i64_result_local_payload_type_static(ok, err, static_bindings) =>
            {
                let tag_local = local_indexes.len();
                local_indexes.insert(i64_result_tag_key(&param.name), tag_local);
                let slot_count =
                    i64_result_payload_slot_count_static(ok.as_ref(), static_bindings)?.max(
                        i64_result_payload_slot_count_static(err.as_ref(), static_bindings)?,
                    );
                for index in 0..slot_count {
                    let payload_local = local_indexes.len();
                    local_indexes.insert(
                        i64_result_payload_slot_key(&param.name, index),
                        payload_local,
                    );
                    if index == 0 {
                        local_indexes.insert(i64_result_payload_key(&param.name), payload_local);
                    }
                }
            }
            Type::Enum(enum_name) if is_i64_enum_payload_type(enum_name, static_bindings) => {
                let tag_local = local_indexes.len();
                local_indexes.insert(i64_enum_tag_key(&param.name), tag_local);
                for index in 0..i64_enum_payload_slot_count(enum_name, static_bindings)? {
                    let payload_local = local_indexes.len();
                    local_indexes
                        .insert(i64_enum_payload_slot_key(&param.name, index), payload_local);
                    if index == 0 {
                        local_indexes.insert(i64_enum_payload_key(&param.name), payload_local);
                    }
                }
            }
            _ => {
                let local = local_indexes.len();
                local_indexes.insert(param.name.clone(), local);
                if matches!(param.ty, Type::Bool) {
                    local_conditions.insert(
                        param.name.clone(),
                        CraneliftI64Condition::Compare(CraneliftI64Compare {
                            op: CraneliftI64CompareOp::Ne,
                            lhs: CraneliftI64Expr::Local(local),
                            rhs: CraneliftI64Expr::Literal(0),
                        }),
                    );
                }
            }
        }
    }
    for stmt in body_stmts {
        match stmt {
            Stmt::Let {
                ty,
                expr: Expr::Index { base, .. },
                ..
            } if !seen_runtime_stmt
                && (matches!(ty, Type::Bool) || is_i64_compatible_type(ty))
                && i64_is_helper_call_slice_expr(base.as_ref()) =>
            {
                lowered_stmts.extend(lower_i64_helper_call_slice_index_let_stmts(
                    stmt,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?);
                seen_runtime_stmt = true;
            }
            Stmt::Let { ty, expr, .. }
                if !seen_runtime_stmt
                    && (matches!(ty, Type::Bool) || is_i64_compatible_type(ty))
                    && i64_is_helper_call_array_index_expr(expr) =>
            {
                lowered_stmts.extend(lower_i64_helper_call_array_index_let_stmts(
                    stmt,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?);
                seen_runtime_stmt = true;
            }
            Stmt::Let { ty, expr, .. }
                if !seen_runtime_stmt
                    && (matches!(ty, Type::Bool) || is_i64_compatible_type(ty))
                    && i64_is_helper_call_projection_expr(expr) =>
            {
                lowered_stmts.extend(lower_i64_helper_call_projection_let_stmts(
                    stmt,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?);
                seen_runtime_stmt = true;
            }
            Stmt::Let {
                ty,
                expr: Expr::Call { args, .. },
                ..
            } if !seen_runtime_stmt
                && (matches!(ty, Type::Bool) || is_i64_compatible_type(ty))
                && (i64_has_helper_call_slice_index_arg(args)
                    || i64_has_helper_call_array_index_arg(args)
                    || i64_has_helper_call_projection_arg(args)) =>
            {
                lowered_stmts.extend(lower_i64_nested_aggregate_call_let_stmts(
                    stmt,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?);
                seen_runtime_stmt = true;
            }
            Stmt::Let { name, ty, expr, .. }
                if is_i64_compatible_type(ty) && !seen_runtime_stmt =>
            {
                if lower_i64_http_server_listen_local(
                    name,
                    expr,
                    &mut locals,
                    &mut local_indexes,
                    &mut *static_bindings,
                )
                .is_some()
                {
                    seen_runtime_stmt = true;
                    continue;
                }
                if allow_stdio_effects {
                    if let Some(stmts) = lower_i64_eprintln_let_stmts(
                        stmt,
                        &mut locals,
                        &mut local_indexes,
                        &local_conditions,
                        helper_signatures,
                        static_bindings,
                    ) {
                        lowered_stmts.extend(stmts);
                        seen_runtime_stmt = true;
                        continue;
                    }
                }
                let local_expr = lower_i64_expr(
                    expr,
                    &local_indexes,
                    &local_conditions,
                    helper_signatures,
                    static_bindings,
                )?;
                local_indexes.insert(name.clone(), local_indexes.len());
                locals.push(local_expr);
            }
            Stmt::Let {
                name,
                ty: Type::String | Type::Str,
                expr,
                ..
            } if !seen_runtime_stmt => {
                lower_i64_string_len_projection_local(
                    name,
                    expr,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    &mut *static_bindings,
                )?;
            }
            Stmt::Let {
                name,
                ty: Type::Map(_, _),
                expr: Expr::MapLiteral { entries, .. },
                ..
            } if !seen_runtime_stmt => {
                let entries = i64_static_map_literal_entries(entries, static_bindings)?;
                static_bindings.map_literals.insert(name.clone(), entries);
            }
            Stmt::Let {
                name,
                ty: Type::Array(_, None),
                expr,
                ..
            } if !seen_runtime_stmt => {
                if let Some(keys) = i64_map_keys_expr(expr, static_bindings) {
                    static_bindings.map_key_arrays.insert(name.clone(), keys);
                } else {
                    return None;
                }
            }
            Stmt::Let { name, expr, .. }
                if is_i64_known_once_call_let(expr, static_bindings) && !seen_runtime_stmt =>
            {
                lower_i64_known_once_call_let(name, expr, &mut *static_bindings)?;
            }
            Stmt::Let { name, expr, .. }
                if is_i64_known_channel_call_let(expr, static_bindings) && !seen_runtime_stmt =>
            {
                lower_i64_known_channel_call_let(name, expr, &mut *static_bindings)?;
            }
            Stmt::Let {
                name,
                ty: Type::Struct(_),
                expr: Expr::StructLiteral { fields, .. },
                ..
            } if !seen_runtime_stmt => {
                lower_i64_struct_projection_locals(
                    name,
                    fields,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?;
            }
            Stmt::Let {
                name,
                ty: Type::Struct(_),
                expr: expr @ Expr::Call {
                    name: call_name, ..
                },
                ..
            } if is_i64_string_builder_constructor_name(call_name, static_bindings)
                && !seen_runtime_stmt =>
            {
                let text = i64_string_builder_text(expr, static_bindings)?;
                static_bindings.string_builders.insert(name.clone(), text);
            }
            Stmt::Let {
                name,
                ty: Type::Struct(struct_name),
                expr:
                    Expr::Call {
                        name: call_name,
                        args,
                        ..
                    },
                ..
            } if !seen_runtime_stmt => {
                lowered_stmts.extend(lower_i64_struct_call_let_stmts(
                    name,
                    struct_name,
                    call_name,
                    args,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?);
                seen_runtime_stmt = true;
            }
            Stmt::Let {
                name,
                ty: Type::Tuple(_),
                expr: Expr::TupleLiteral { elements, .. },
                ..
            } if !seen_runtime_stmt => {
                lower_i64_indexed_projection_locals(
                    name,
                    elements,
                    i64_tuple_projection_key,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?;
            }
            Stmt::Let {
                name,
                ty: Type::Tuple(elements),
                expr:
                    Expr::Call {
                        name: call_name,
                        args,
                        ..
                    },
                ..
            } if is_i64_tuple_param_type(elements) && !seen_runtime_stmt => {
                lowered_stmts.extend(lower_i64_tuple_call_let_stmts(
                    name,
                    elements,
                    call_name,
                    args,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?);
                seen_runtime_stmt = true;
            }
            Stmt::Let {
                name,
                ty: Type::Array(_, _),
                expr: Expr::ArrayLiteral { elements, .. },
                ..
            } if !seen_runtime_stmt => {
                lower_i64_indexed_projection_locals(
                    name,
                    elements,
                    i64_array_projection_key,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?;
            }
            Stmt::Let {
                name,
                ty: Type::MutSlice(_),
                expr,
                ..
            } if !seen_runtime_stmt
                && record_i64_mut_slice_alias(name, expr, &local_indexes, static_bindings)
                    .is_some() => {}
            Stmt::Let {
                name,
                ty: Type::Slice(_) | Type::MutSlice(_),
                expr,
                ..
            } if !seen_runtime_stmt => {
                let assigns = lower_i64_slice_projection_aliases(
                    name,
                    expr,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                    false,
                )?;
                seen_runtime_stmt = !assigns.is_empty();
                lowered_stmts.extend(assigns);
            }
            Stmt::Let {
                name,
                ty: Type::Array(element, Some(size)),
                expr:
                    Expr::Call {
                        name: call_name,
                        args,
                        ..
                    },
                ..
            } if is_i64_array_param_element_type(element) && !seen_runtime_stmt => {
                lowered_stmts.extend(lower_i64_array_call_let_stmts(
                    name,
                    element.as_ref(),
                    *size,
                    call_name,
                    args,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?);
                seen_runtime_stmt = true;
            }
            Stmt::Let {
                name,
                ty: Type::Option(inner),
                expr: expr @ Expr::Call { .. },
                ..
            } if (is_i64_option_local_payload_type_static(inner, static_bindings)
                || is_i64_known_string_option_call_let_type(inner.as_ref()))
                && !seen_runtime_stmt =>
            {
                if let Some(assigns) = lower_i64_runtime_string_option_call_let_stmts(
                    name,
                    inner.as_ref(),
                    expr,
                    &mut locals,
                    &mut local_indexes,
                    static_bindings,
                ) {
                    lowered_stmts.extend(assigns);
                    seen_runtime_stmt = true;
                } else if let Some(assigns) = lower_i64_known_string_option_call_let_stmts(
                    name,
                    inner.as_ref(),
                    expr,
                    &mut *static_bindings,
                ) {
                    let has_runtime_stmts = !assigns.is_empty();
                    lowered_stmts.extend(assigns);
                    seen_runtime_stmt = has_runtime_stmts;
                } else {
                    lowered_stmts.extend(lower_i64_scalar_option_call_let_stmts(
                        name,
                        inner.as_ref(),
                        expr,
                        &mut locals,
                        &mut local_indexes,
                        &mut local_conditions,
                        helper_signatures,
                        static_bindings,
                    )?);
                    seen_runtime_stmt = true;
                }
            }
            Stmt::Let {
                name,
                ty: Type::Result(ok, err),
                expr:
                    Expr::Call {
                        name: call_name,
                        args,
                        ..
                    },
                ..
            } if is_i64_result_local_payload_type_static(ok, err, static_bindings)
                && !seen_runtime_stmt =>
            {
                lowered_stmts.extend(lower_i64_result_call_let_stmts(
                    name,
                    ok.as_ref(),
                    err.as_ref(),
                    call_name,
                    args,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?);
                seen_runtime_stmt = true;
            }
            Stmt::Let {
                name,
                ty: Type::Enum(enum_name),
                expr:
                    Expr::Call {
                        name: call_name,
                        args,
                        ..
                    },
                ..
            } if is_i64_enum_payload_type(enum_name, static_bindings) && !seen_runtime_stmt => {
                lowered_stmts.extend(lower_i64_enum_call_let_stmts(
                    name,
                    enum_name,
                    call_name,
                    args,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?);
                seen_runtime_stmt = true;
            }
            Stmt::Let {
                name,
                ty: Type::Option(inner),
                expr: Expr::EnumVariant { .. },
                ..
            } if is_i64_option_local_payload_type_static(inner, static_bindings)
                && !seen_runtime_stmt =>
            {
                lower_i64_option_locals(
                    name,
                    stmt,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?;
            }
            Stmt::Let {
                name,
                ty: Type::Enum(enum_name),
                expr: Expr::EnumVariant { .. },
                ..
            } if is_i64_enum_payload_type(enum_name, static_bindings) && !seen_runtime_stmt => {
                lower_i64_enum_locals(
                    name,
                    stmt,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?;
            }
            Stmt::Let {
                name,
                ty: Type::Result(ok, err),
                expr: Expr::EnumVariant { .. },
                ..
            } if is_i64_result_local_payload_type_static(ok, err, static_bindings)
                && !seen_runtime_stmt =>
            {
                lower_i64_result_locals(
                    name,
                    stmt,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                )?;
            }
            Stmt::Let {
                name,
                ty: Type::Bool,
                expr,
                ..
            } if !seen_runtime_stmt => {
                let local_expr = lower_i64_bool_value_expr(
                    expr,
                    &local_indexes,
                    &local_conditions,
                    helper_signatures,
                    static_bindings,
                )?;
                let local = local_indexes.len();
                local_indexes.insert(name.clone(), local);
                locals.push(local_expr);
                local_conditions.insert(
                    name.clone(),
                    CraneliftI64Condition::Compare(CraneliftI64Compare {
                        op: CraneliftI64CompareOp::Ne,
                        lhs: CraneliftI64Expr::Local(local),
                        rhs: CraneliftI64Expr::Literal(0),
                    }),
                );
            }
            Stmt::Let { .. } if seen_runtime_stmt => {
                lowered_stmts.extend(lower_i64_runtime_let_stmts(
                    stmt,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                    allow_stdio_effects,
                )?);
            }
            Stmt::Assign { .. }
            | Stmt::Print { .. }
            | Stmt::If { .. }
            | Stmt::While { .. }
            | Stmt::Match { .. } => {
                seen_runtime_stmt = true;
                lowered_stmts.extend(lower_i64_runtime_stmt_stmts(
                    stmt,
                    &mut locals,
                    local_indexes.clone(),
                    local_conditions.clone(),
                    helper_signatures,
                    static_bindings,
                    allow_stdio_effects,
                )?);
            }
            _ => return None,
        }
    }
    let body = match return_stmt {
        Stmt::Return { expr, .. } => lower_i64_exit_return(
            expr,
            &mut locals,
            &local_indexes,
            &local_conditions,
            helper_signatures,
            static_bindings,
        )?,
        Stmt::If {
            cond,
            then_block,
            else_block: Some(else_block),
            ..
        } => {
            let cond = if let Some((rewritten_cond, mut setup)) =
                rewrite_i64_helper_call_aggregate_condition_expr(
                    cond,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                ) {
                lowered_stmts.append(&mut setup);
                rewritten_cond
            } else {
                cond.clone()
            };
            let cond = lower_i64_condition(
                &cond,
                &local_indexes,
                &local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            let then_block = lower_i64_return_block(
                then_block,
                &mut locals,
                local_indexes.clone(),
                local_conditions.clone(),
                helper_signatures,
                static_bindings,
                allow_stdio_effects,
            )?;
            let else_block = lower_i64_return_block(
                else_block,
                &mut locals,
                local_indexes.clone(),
                local_conditions.clone(),
                helper_signatures,
                static_bindings,
                allow_stdio_effects,
            )?;
            I64ExitBody::IfBlockReturn {
                cond,
                then_block,
                else_block,
            }
        }
        Stmt::Panic { message, .. } if allow_terminal_panic => lower_i64_panic_exit_body(
            message,
            &local_indexes,
            &local_conditions,
            helper_signatures,
            static_bindings,
        )?,
        _ => return None,
    };
    Some((locals, lowered_stmts, body))
}

fn lower_i64_runtime_stmt_stmts(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: HashMap<String, usize>,
    local_conditions: HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    allow_stdio_effects: bool,
) -> Option<Vec<CraneliftI64Stmt>> {
    if let Some(assigns) =
        lower_i64_aggregate_local_assign_stmts(stmt, &local_indexes, static_bindings)
    {
        return Some(assigns);
    }
    if let Some(assigns) = lower_i64_option_assign_stmts(
        stmt,
        &local_indexes,
        &local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(assigns);
    }
    if let Some(assigns) = lower_i64_result_assign_stmts(
        stmt,
        &local_indexes,
        &local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(assigns);
    }
    if let Some(assigns) = lower_i64_enum_assign_stmts(
        stmt,
        &local_indexes,
        &local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(assigns);
    }
    if let Some(assigns) = lower_i64_aggregate_call_assign_stmts(
        stmt,
        locals,
        &local_indexes,
        &local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(assigns);
    }
    if let Some(assigns) = lower_i64_projection_assign_stmts(
        stmt,
        &local_indexes,
        &local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(assigns);
    }
    if allow_stdio_effects {
        if let Some(stmts) = lower_i64_print_stmt_stmts(
            stmt,
            &local_indexes,
            &local_conditions,
            helper_signatures,
            static_bindings,
        ) {
            return Some(stmts);
        }
    }
    if let Some(stmts) = lower_i64_helper_call_aggregate_condition_stmt_stmts(
        stmt,
        locals,
        local_indexes.clone(),
        local_conditions.clone(),
        helper_signatures,
        static_bindings,
        allow_stdio_effects,
    ) {
        return Some(stmts);
    }
    Some(vec![lower_i64_runtime_stmt(
        stmt,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
        allow_stdio_effects,
    )?])
}

fn lower_i64_helper_call_aggregate_condition_stmt_stmts(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: HashMap<String, usize>,
    local_conditions: HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    allow_stdio_effects: bool,
) -> Option<Vec<CraneliftI64Stmt>> {
    match stmt {
        Stmt::If {
            cond,
            then_block,
            else_block,
            span,
        } => {
            let mut trial_locals = locals.clone();
            let mut trial_indexes = local_indexes;
            let mut trial_conditions = local_conditions;
            let (rewritten_cond, mut setup) = rewrite_i64_helper_call_aggregate_condition_expr(
                cond,
                &mut trial_locals,
                &mut trial_indexes,
                &mut trial_conditions,
                helper_signatures,
                static_bindings,
            )?;
            let rewritten = Stmt::If {
                cond: rewritten_cond,
                then_block: then_block.clone(),
                else_block: else_block.clone(),
                span: *span,
            };
            setup.push(lower_i64_runtime_stmt(
                &rewritten,
                &mut trial_locals,
                trial_indexes,
                trial_conditions,
                helper_signatures,
                static_bindings,
                allow_stdio_effects,
            )?);
            *locals = trial_locals;
            Some(setup)
        }
        Stmt::While { cond, body, span } => {
            let mut trial_locals = locals.clone();
            let mut trial_indexes = local_indexes;
            let mut trial_conditions = local_conditions;
            let (rewritten_cond, mut setup) = rewrite_i64_helper_call_aggregate_condition_expr(
                cond,
                &mut trial_locals,
                &mut trial_indexes,
                &mut trial_conditions,
                helper_signatures,
                static_bindings,
            )?;
            let rewritten = Stmt::While {
                cond: rewritten_cond,
                body: body.clone(),
                span: *span,
            };
            setup.push(lower_i64_runtime_stmt(
                &rewritten,
                &mut trial_locals,
                trial_indexes,
                trial_conditions,
                helper_signatures,
                static_bindings,
                allow_stdio_effects,
            )?);
            *locals = trial_locals;
            Some(setup)
        }
        _ => None,
    }
}

fn rewrite_i64_helper_call_aggregate_condition_expr(
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<(Expr, Vec<CraneliftI64Stmt>)> {
    let mut setup = Vec::new();
    let mut rewrote = false;
    let rewritten = rewrite_i64_helper_call_aggregate_condition_expr_inner(
        expr,
        0,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
        &mut setup,
        &mut rewrote,
    )?;
    rewrote.then_some((rewritten, setup))
}

#[allow(clippy::too_many_arguments)]
fn rewrite_i64_helper_call_aggregate_condition_expr_inner(
    expr: &Expr,
    arg_index: usize,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    setup: &mut Vec<CraneliftI64Stmt>,
    rewrote: &mut bool,
) -> Option<Expr> {
    if let Some((rewritten, mut assigns)) = rewrite_i64_nested_aggregate_call_arg(
        expr,
        arg_index,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        setup.append(&mut assigns);
        *rewrote = true;
        return Some(rewritten);
    }

    match expr {
        Expr::Call { name, args, ty } => {
            let mut rewritten_args = Vec::with_capacity(args.len());
            for (index, arg) in args.iter().enumerate() {
                rewritten_args.push(rewrite_i64_helper_call_aggregate_condition_expr_inner(
                    arg,
                    index,
                    locals,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                    setup,
                    rewrote,
                )?);
            }
            Some(Expr::Call {
                name: name.clone(),
                args: rewritten_args,
                ty: ty.clone(),
            })
        }
        Expr::BinaryCompare { op, lhs, rhs, ty } => Some(Expr::BinaryCompare {
            op: *op,
            lhs: Box::new(rewrite_i64_helper_call_aggregate_condition_expr_inner(
                lhs,
                arg_index,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
                setup,
                rewrote,
            )?),
            rhs: Box::new(rewrite_i64_helper_call_aggregate_condition_expr_inner(
                rhs,
                arg_index + 1,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
                setup,
                rewrote,
            )?),
            ty: ty.clone(),
        }),
        Expr::BinaryLogic { op, lhs, rhs, ty } => Some(Expr::BinaryLogic {
            op: *op,
            lhs: Box::new(rewrite_i64_helper_call_aggregate_condition_expr_inner(
                lhs,
                arg_index,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
                setup,
                rewrote,
            )?),
            rhs: Box::new(rewrite_i64_helper_call_aggregate_condition_expr_inner(
                rhs,
                arg_index + 1,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
                setup,
                rewrote,
            )?),
            ty: ty.clone(),
        }),
        Expr::BinaryAdd { op, lhs, rhs, ty } => Some(Expr::BinaryAdd {
            op: *op,
            lhs: Box::new(rewrite_i64_helper_call_aggregate_condition_expr_inner(
                lhs,
                arg_index,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
                setup,
                rewrote,
            )?),
            rhs: Box::new(rewrite_i64_helper_call_aggregate_condition_expr_inner(
                rhs,
                arg_index + 1,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
                setup,
                rewrote,
            )?),
            ty: ty.clone(),
        }),
        Expr::Cast { expr, ty } => Some(Expr::Cast {
            expr: Box::new(rewrite_i64_helper_call_aggregate_condition_expr_inner(
                expr,
                arg_index,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
                setup,
                rewrote,
            )?),
            ty: ty.clone(),
        }),
        _ => Some(expr.clone()),
    }
}

fn lower_i64_aggregate_local_assign_stmts(
    stmt: &Stmt,
    local_indexes: &HashMap<String, usize>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Stmt::Assign {
        target: Expr::VarRef {
            name: target,
            ty: target_ty,
        },
        expr: Expr::VarRef {
            name: source,
            ty: source_ty,
        },
        ..
    } = stmt
    else {
        return None;
    };
    if target_ty != source_ty {
        return None;
    }
    let slot_pairs = match target_ty {
        Type::Struct(struct_name) => {
            let struct_def = i64_scalar_static_struct_def(struct_name, static_bindings)?;
            struct_def
                .fields
                .iter()
                .map(|field| {
                    Some((
                        *local_indexes
                            .get(i64_struct_projection_key(target, &field.name).as_str())?,
                        *local_indexes
                            .get(i64_struct_projection_key(source, &field.name).as_str())?,
                    ))
                })
                .collect::<Option<Vec<_>>>()?
        }
        Type::Tuple(elements) if is_i64_tuple_param_type(elements) => (0..elements.len())
            .map(|index| {
                Some((
                    *local_indexes.get(i64_tuple_projection_key(target, index).as_str())?,
                    *local_indexes.get(i64_tuple_projection_key(source, index).as_str())?,
                ))
            })
            .collect::<Option<Vec<_>>>()?,
        Type::Array(element, Some(size)) if is_i64_array_param_element_type(element) => (0..*size)
            .map(|index| {
                Some((
                    *local_indexes.get(i64_array_projection_key(target, index).as_str())?,
                    *local_indexes.get(i64_array_projection_key(source, index).as_str())?,
                ))
            })
            .collect::<Option<Vec<_>>>()?,
        Type::Option(inner) if is_i64_option_local_payload_type_static(inner, static_bindings) => {
            let mut slots = vec![(
                *local_indexes.get(i64_option_tag_key(target).as_str())?,
                *local_indexes.get(i64_option_tag_key(source).as_str())?,
            )];
            slots.extend(
                i64_option_payload_locals(target, inner.as_ref(), local_indexes, static_bindings)?
                    .into_iter()
                    .zip(i64_option_payload_locals(
                        source,
                        inner.as_ref(),
                        local_indexes,
                        static_bindings,
                    )?),
            );
            slots
        }
        Type::Result(ok, err)
            if is_i64_result_local_payload_type_static(ok, err, static_bindings) =>
        {
            let mut slots = vec![(
                *local_indexes.get(i64_result_tag_key(target).as_str())?,
                *local_indexes.get(i64_result_tag_key(source).as_str())?,
            )];
            slots.extend(
                i64_result_payload_locals(
                    target,
                    ok.as_ref(),
                    err.as_ref(),
                    local_indexes,
                    static_bindings,
                )?
                .into_iter()
                .zip(i64_result_payload_locals(
                    source,
                    ok.as_ref(),
                    err.as_ref(),
                    local_indexes,
                    static_bindings,
                )?),
            );
            slots
        }
        Type::Enum(enum_name) if is_i64_enum_payload_type(enum_name, static_bindings) => {
            let mut slots = vec![(
                *local_indexes.get(i64_enum_tag_key(target).as_str())?,
                *local_indexes.get(i64_enum_tag_key(source).as_str())?,
            )];
            slots.extend(
                i64_enum_payload_locals(target, enum_name, static_bindings, local_indexes)?
                    .into_iter()
                    .zip(i64_enum_payload_locals(
                        source,
                        enum_name,
                        static_bindings,
                        local_indexes,
                    )?),
            );
            slots
        }
        _ => return None,
    };
    Some(
        slot_pairs
            .into_iter()
            .map(|(target, source)| {
                CraneliftI64Stmt::Assign(axiomc_backend_cranelift::I64Assign {
                    local: target,
                    value: CraneliftI64Expr::Local(source),
                })
            })
            .collect(),
    )
}

fn lower_i64_runtime_stmt(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: HashMap<String, usize>,
    local_conditions: HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    allow_stdio_effects: bool,
) -> Option<CraneliftI64Stmt> {
    match stmt {
        Stmt::Match { .. } => {
            if let Some(stmt) = lower_i64_known_string_option_match_stmt(
                stmt,
                locals,
                local_indexes.clone(),
                local_conditions.clone(),
                helper_signatures,
                static_bindings,
                allow_stdio_effects,
            ) {
                Some(stmt)
            } else if let Some(stmt) = lower_i64_env_option_match_stmt(
                stmt,
                locals,
                local_indexes.clone(),
                local_conditions.clone(),
                helper_signatures,
                static_bindings,
                allow_stdio_effects,
            ) {
                Some(stmt)
            } else if let Some(stmt) = lower_i64_option_match_stmt(
                stmt,
                locals,
                local_indexes.clone(),
                local_conditions.clone(),
                helper_signatures,
                static_bindings,
                allow_stdio_effects,
            ) {
                Some(stmt)
            } else if let Some(stmt) = lower_i64_option_int_value_match_stmt(
                stmt,
                locals,
                local_indexes.clone(),
                local_conditions.clone(),
                helper_signatures,
                static_bindings,
                allow_stdio_effects,
            ) {
                Some(stmt)
            } else if let Some(stmt) = lower_i64_result_match_stmt(
                stmt,
                locals,
                local_indexes.clone(),
                local_conditions.clone(),
                helper_signatures,
                static_bindings,
                allow_stdio_effects,
            ) {
                Some(stmt)
            } else {
                lower_i64_enum_match_stmt(
                    stmt,
                    locals,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                    allow_stdio_effects,
                )
            }
        }
        Stmt::Assign { .. } => lower_i64_assign(
            stmt,
            &local_indexes,
            &local_conditions,
            helper_signatures,
            static_bindings,
        )
        .map(CraneliftI64Stmt::Assign)
        .or_else(|| {
            lower_i64_index_assign_stmt(
                stmt,
                &local_indexes,
                &local_conditions,
                helper_signatures,
                static_bindings,
            )
        }),
        Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => Some(CraneliftI64Stmt::If {
            cond: lower_i64_condition(
                cond,
                &local_indexes,
                &local_conditions,
                helper_signatures,
                static_bindings,
            )?,
            then_body: lower_i64_runtime_stmts(
                then_block,
                locals,
                local_indexes.clone(),
                local_conditions.clone(),
                helper_signatures,
                static_bindings,
                allow_stdio_effects,
            )?,
            else_body: lower_i64_runtime_stmts(
                else_block.as_deref().unwrap_or(&[]),
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
                allow_stdio_effects,
            )?,
        }),
        Stmt::While { cond, body, .. } => Some(CraneliftI64Stmt::While {
            cond: lower_i64_condition(
                cond,
                &local_indexes,
                &local_conditions,
                helper_signatures,
                static_bindings,
            )?,
            body: lower_i64_runtime_stmts(
                body,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
                allow_stdio_effects,
            )?,
        }),
        _ => None,
    }
}

fn lower_i64_known_string_option_match_stmt(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: HashMap<String, usize>,
    local_conditions: HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    allow_stdio_effects: bool,
) -> Option<CraneliftI64Stmt> {
    let Stmt::Match { expr, arms, .. } = stmt else {
        return None;
    };
    let value = i64_string_option_text(expr, static_bindings)?;
    let (some_arm, none_arm) = i64_option_stmt_match_arms(arms)?;
    match value {
        Some(value) => {
            let mut arm_static_bindings = static_bindings.clone();
            if !some_arm.ignore_payloads
                && let Some(binding) = some_arm.bindings.first()
                && binding != "_"
            {
                arm_static_bindings.strings.insert(binding.clone(), value);
            }
            Some(CraneliftI64Stmt::If {
                cond: CraneliftI64Condition::Literal(true),
                then_body: lower_i64_runtime_stmts(
                    &some_arm.body,
                    locals,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    &arm_static_bindings,
                    allow_stdio_effects,
                )?,
                else_body: Vec::new(),
            })
        }
        None => Some(CraneliftI64Stmt::If {
            cond: CraneliftI64Condition::Literal(false),
            then_body: Vec::new(),
            else_body: lower_i64_runtime_stmts(
                &none_arm.body,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
                allow_stdio_effects,
            )?,
        }),
    }
}

fn i64_element_slot_locals(
    base: &Expr,
    local_indexes: &HashMap<String, usize>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<usize>> {
    match base {
        // Writes through a mutable slice are only sound when the binding is a
        // recorded alias of its base array; targeting the copied projection
        // locals of a non-aliased slice would silently drop the write.
        Expr::VarRef {
            name,
            ty: Type::MutSlice(_),
        } => i64_mut_slice_alias_slots(name, local_indexes, static_bindings),
        Expr::VarRef {
            name,
            ty: Type::Array(_, Some(size)),
        } => {
            let mut slots = Vec::new();
            for index in 0..*size {
                slots.push(
                    local_indexes
                        .get(i64_array_projection_key(name, index).as_str())
                        .copied()?,
                );
            }
            (!slots.is_empty()).then_some(slots)
        }
        Expr::Slice { .. } => {
            let (name, start, size) = i64_static_slice_base_range(base, static_bindings)?;
            let mut slots = Vec::new();
            for index in 0..size {
                slots.push(
                    local_indexes
                        .get(i64_array_projection_key(name, start + index).as_str())
                        .copied()?,
                );
            }
            (!slots.is_empty()).then_some(slots)
        }
        _ => None,
    }
}

/// Lower `values[index] = expr` for element-slot-backed arrays and slices.
/// Literal indexes assign the slot directly; runtime indexes lower to a
/// compare chain over the slots, matching the silent out-of-range behavior of
/// the dynamic-index read selects.
fn lower_i64_index_assign_stmt(
    stmt: &Stmt,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Stmt> {
    let Stmt::Assign {
        target: Expr::Index { base, index, .. },
        expr,
        ..
    } = stmt
    else {
        return None;
    };
    let slots = i64_element_slot_locals(base, local_indexes, static_bindings)?;
    let element_ty = match base.ty() {
        Type::Slice(element) | Type::MutSlice(element) | Type::Array(element, _) => {
            element.as_ref().clone()
        }
        _ => return None,
    };
    let value = match element_ty {
        Type::Bool => lower_i64_bool_value_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
        ty if is_i64_compatible_type(&ty) => lower_i64_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
        _ => return None,
    };
    if let Some(index) = lower_i64_literal_index(index) {
        let slot = slots.get(index).copied()?;
        return Some(CraneliftI64Stmt::Assign(
            axiomc_backend_cranelift::I64Assign { local: slot, value },
        ));
    }
    let index = lower_i64_expr(
        index,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let mut lowered: Vec<CraneliftI64Stmt> = Vec::new();
    for (candidate, slot) in slots.iter().enumerate().rev() {
        lowered = vec![CraneliftI64Stmt::If {
            cond: CraneliftI64Condition::Compare(CraneliftI64Compare {
                op: CraneliftI64CompareOp::Eq,
                lhs: index.clone(),
                rhs: CraneliftI64Expr::Literal(candidate as i64),
            }),
            then_body: vec![CraneliftI64Stmt::Assign(
                axiomc_backend_cranelift::I64Assign {
                    local: *slot,
                    value: value.clone(),
                },
            )],
            else_body: lowered,
        }];
    }
    lowered.into_iter().next()
}

/// Lower option-int match statements whose scrutinee is a value expression
/// rather than an already-projected option local: `Some(<i64 expr>)`, `None`,
/// or `string_byte_at(<static string>, <runtime index>)`. The tag lowers to a
/// runtime condition and the payload is materialized into a fresh local that
/// is assigned at the head of the Some arm, so the shape stays valid inside
/// runtime loop bodies where locals are re-evaluated per iteration.
fn lower_i64_option_int_value_match_stmt(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: HashMap<String, usize>,
    local_conditions: HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    allow_stdio_effects: bool,
) -> Option<CraneliftI64Stmt> {
    let Stmt::Match { expr, arms, .. } = stmt else {
        return None;
    };
    let (cond, payload) = match expr {
        Expr::EnumVariant {
            enum_name,
            variant,
            payloads,
            ty: Type::Option(inner),
            ..
        } if enum_name == "Option" && matches!(inner.as_ref(), Type::Int) => match variant.as_str()
        {
            "Some" => {
                let [payload] = payloads.as_slice() else {
                    return None;
                };
                let payload = lower_i64_expr(
                    payload,
                    &local_indexes,
                    &local_conditions,
                    helper_signatures,
                    static_bindings,
                )?;
                (CraneliftI64Condition::Literal(true), Some(payload))
            }
            "None" => (CraneliftI64Condition::Literal(false), None),
            _ => return None,
        },
        Expr::Call { name, args, .. } if name == "string_byte_at" => {
            let [text, index] = args.as_slice() else {
                return None;
            };
            let text = i64_string_text(text, static_bindings)?;
            let index = lower_i64_expr(
                index,
                &local_indexes,
                &local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            let bytes = text.as_bytes();
            if bytes.is_empty() {
                (CraneliftI64Condition::Literal(false), None)
            } else {
                let cond = CraneliftI64Condition::And {
                    lhs: Box::new(CraneliftI64Condition::Compare(CraneliftI64Compare {
                        op: CraneliftI64CompareOp::Ge,
                        lhs: index.clone(),
                        rhs: CraneliftI64Expr::Literal(0),
                    })),
                    rhs: Box::new(CraneliftI64Condition::Compare(CraneliftI64Compare {
                        op: CraneliftI64CompareOp::Lt,
                        lhs: index.clone(),
                        rhs: CraneliftI64Expr::Literal(bytes.len() as i64),
                    })),
                };
                let last = bytes.len() - 1;
                let mut payload = CraneliftI64Expr::Literal(i64::from(bytes[last]));
                for candidate in (0..last).rev() {
                    payload = CraneliftI64Expr::Select {
                        cond: Box::new(CraneliftI64Condition::Compare(CraneliftI64Compare {
                            op: CraneliftI64CompareOp::Eq,
                            lhs: index.clone(),
                            rhs: CraneliftI64Expr::Literal(candidate as i64),
                        })),
                        then_result: Box::new(CraneliftI64Expr::Literal(i64::from(
                            bytes[candidate],
                        ))),
                        else_result: Box::new(payload),
                    };
                }
                (cond, Some(payload))
            }
        }
        _ => return None,
    };
    let (some_arm, none_arm) = i64_option_stmt_match_arms(arms)?;
    let mut some_indexes = local_indexes.clone();
    let some_conditions = local_conditions.clone();
    let mut then_head = Vec::new();
    if !some_arm.ignore_payloads
        && let Some(binding) = some_arm.bindings.first()
        && binding != "_"
    {
        let payload = payload.unwrap_or(CraneliftI64Expr::Literal(0));
        let payload_local = locals.len();
        locals.push(CraneliftI64Expr::Literal(0));
        then_head.push(CraneliftI64Stmt::Assign(
            axiomc_backend_cranelift::I64Assign {
                local: payload_local,
                value: payload,
            },
        ));
        some_indexes.insert(binding.clone(), payload_local);
    }
    let mut then_body = then_head;
    then_body.extend(lower_i64_runtime_stmts(
        &some_arm.body,
        locals,
        some_indexes,
        some_conditions,
        helper_signatures,
        static_bindings,
        allow_stdio_effects,
    )?);
    Some(CraneliftI64Stmt::If {
        cond,
        then_body,
        else_body: lower_i64_runtime_stmts(
            &none_arm.body,
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
            allow_stdio_effects,
        )?,
    })
}

fn lower_i64_option_match_stmt(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: HashMap<String, usize>,
    local_conditions: HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    allow_stdio_effects: bool,
) -> Option<CraneliftI64Stmt> {
    let Stmt::Match { expr, arms, .. } = stmt else {
        return None;
    };
    let (cond, some_indexes, some_conditions, some_arm, none_arm) =
        lower_i64_option_stmt_match_parts(
            expr,
            arms,
            &local_indexes,
            &local_conditions,
            static_bindings,
        )?;
    if matches!(expr.ty(), Type::Option(inner) if matches!(inner.as_ref(), Type::String | Type::Str))
        && !some_arm.ignore_payloads
        && let Some(binding) = some_arm.bindings.first()
        && binding != "_"
        && !i64_env_option_payload_uses_len_only(&some_arm.body, binding)
    {
        return None;
    }
    Some(CraneliftI64Stmt::If {
        cond,
        then_body: lower_i64_runtime_stmts(
            &some_arm.body,
            locals,
            some_indexes,
            some_conditions,
            helper_signatures,
            static_bindings,
            allow_stdio_effects,
        )?,
        else_body: lower_i64_runtime_stmts(
            &none_arm.body,
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
            allow_stdio_effects,
        )?,
    })
}

fn lower_i64_result_match_stmt(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: HashMap<String, usize>,
    local_conditions: HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    allow_stdio_effects: bool,
) -> Option<CraneliftI64Stmt> {
    let Stmt::Match { expr, arms, .. } = stmt else {
        return None;
    };
    let (cond, ok_indexes, ok_conditions, err_indexes, err_conditions, ok_arm, err_arm) =
        lower_i64_result_stmt_match_parts(
            expr,
            arms,
            &local_indexes,
            &local_conditions,
            static_bindings,
        )?;
    Some(CraneliftI64Stmt::If {
        cond,
        then_body: lower_i64_runtime_stmts(
            &ok_arm.body,
            locals,
            ok_indexes,
            ok_conditions,
            helper_signatures,
            static_bindings,
            allow_stdio_effects,
        )?,
        else_body: lower_i64_runtime_stmts(
            &err_arm.body,
            locals,
            err_indexes,
            err_conditions,
            helper_signatures,
            static_bindings,
            allow_stdio_effects,
        )?,
    })
}

fn lower_i64_enum_match_stmt(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: HashMap<String, usize>,
    local_conditions: HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    allow_stdio_effects: bool,
) -> Option<CraneliftI64Stmt> {
    let Stmt::Match { expr, arms, .. } = stmt else {
        return None;
    };
    if arms.len() < 2 {
        return None;
    }
    let Expr::VarRef {
        name,
        ty: Type::Enum(enum_name),
    } = expr
    else {
        return None;
    };
    let enum_def = i64_scalar_enum_def(enum_name, static_bindings)?;
    let tag = *local_indexes.get(i64_enum_tag_key(name).as_str())?;
    let payloads = i64_enum_payload_locals(name, enum_name, static_bindings, &local_indexes)?;
    let mut lowered: Option<Vec<CraneliftI64Stmt>> = None;
    for arm in arms.iter().rev() {
        if arm.enum_name != enum_def.name {
            return None;
        }
        let variant = enum_def
            .variants
            .iter()
            .position(|variant| variant.name == arm.variant)?;
        let variant_def = enum_def.variants.get(variant)?;
        if !arm.ignore_payloads && arm.bindings.len() != variant_def.payload_tys.len() {
            return None;
        }
        let mut arm_indexes = local_indexes.clone();
        let mut arm_conditions = local_conditions.clone();
        let binding_names = i64_enum_match_binding_names(
            arm.is_named,
            arm.ignore_payloads,
            arm.bindings.as_slice(),
            variant_def,
        )?;
        insert_i64_enum_payload_bindings(
            binding_names.as_deref(),
            arm.is_named,
            &variant_def.payload_tys,
            &variant_def.payload_names,
            &payloads,
            static_bindings,
            &mut arm_indexes,
            &mut arm_conditions,
        );
        let body = lower_i64_runtime_stmts(
            &arm.body,
            locals,
            arm_indexes,
            arm_conditions,
            helper_signatures,
            static_bindings,
            allow_stdio_effects,
        )?;
        lowered = Some(match lowered {
            None => body,
            Some(else_body) => vec![CraneliftI64Stmt::If {
                cond: CraneliftI64Condition::Compare(CraneliftI64Compare {
                    op: CraneliftI64CompareOp::Eq,
                    lhs: CraneliftI64Expr::Local(tag),
                    rhs: CraneliftI64Expr::Literal(variant as i64),
                }),
                then_body: body,
                else_body,
            }],
        });
    }
    let mut stmts = lowered?;
    if stmts.len() == 1 { stmts.pop() } else { None }
}

fn lower_i64_runtime_stmts(
    stmts: &[Stmt],
    locals: &mut Vec<CraneliftI64Expr>,
    mut local_indexes: HashMap<String, usize>,
    mut local_conditions: HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    allow_stdio_effects: bool,
) -> Option<Vec<CraneliftI64Stmt>> {
    let mut lowered = Vec::new();
    let mut scoped_static_bindings = static_bindings.clone();
    let static_bindings = &mut scoped_static_bindings;
    for stmt in stmts {
        if matches!(stmt, Stmt::Let { .. }) {
            if record_i64_known_string_let(stmt, static_bindings).unwrap_or(false) {
                continue;
            }
            if record_i64_known_map_let(stmt, static_bindings).unwrap_or(false) {
                continue;
            }
            if record_i64_known_map_key_array_let(stmt, static_bindings).unwrap_or(false) {
                continue;
            }
            lowered.extend(lower_i64_runtime_let_stmts(
                stmt,
                locals,
                &mut local_indexes,
                &mut local_conditions,
                helper_signatures,
                static_bindings,
                allow_stdio_effects,
            )?);
        } else {
            lowered.extend(lower_i64_runtime_stmt_stmts(
                stmt,
                locals,
                local_indexes.clone(),
                local_conditions.clone(),
                helper_signatures,
                static_bindings,
                allow_stdio_effects,
            )?);
        }
    }
    Some(lowered)
}

fn lower_i64_runtime_let(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Stmt> {
    let Stmt::Let { name, ty, expr, .. } = stmt else {
        return None;
    };
    let value = match ty {
        Type::Bool => lower_i64_bool_value_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
        ty if is_i64_compatible_type(ty) => lower_i64_return_value_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
        _ => return None,
    };
    let local = local_indexes.len();
    local_indexes.insert(name.clone(), local);
    locals.push(CraneliftI64Expr::Literal(0));
    if matches!(ty, Type::Bool) {
        local_conditions.insert(
            name.clone(),
            CraneliftI64Condition::Compare(CraneliftI64Compare {
                op: CraneliftI64CompareOp::Ne,
                lhs: CraneliftI64Expr::Local(local),
                rhs: CraneliftI64Expr::Literal(0),
            }),
        );
    }
    Some(CraneliftI64Stmt::Assign(
        axiomc_backend_cranelift::I64Assign { local, value },
    ))
}

fn lower_i64_runtime_let_stmts(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    allow_stdio_effects: bool,
) -> Option<Vec<CraneliftI64Stmt>> {
    if let Stmt::Let {
        ty: Type::MutSlice(_),
        ..
    } = stmt
    {
        // Mutable-slice bindings alias their base at function scope via
        // record_i64_mut_slice_alias; inside runtime blocks the bindings are
        // immutable here, and lowering them as element copies would silently
        // drop writes, so reject and let the caller fall back.
        return None;
    }
    if let Stmt::Let {
        name,
        ty: Type::Slice(_),
        expr,
        ..
    } = stmt
    {
        return lower_i64_slice_projection_aliases(
            name,
            expr,
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
            true,
        );
    }
    if let Some(assigns) = lower_i64_runtime_string_len_let_stmts(
        stmt,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(assigns);
    }
    if allow_stdio_effects {
        if let Some(assigns) = lower_i64_eprintln_let_stmts(
            stmt,
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        ) {
            return Some(assigns);
        }
    }
    if let Some(assigns) = lower_i64_nested_aggregate_call_let_stmts(
        stmt,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(assigns);
    }
    if let Some(assigns) = lower_i64_helper_call_slice_index_let_stmts(
        stmt,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(assigns);
    }
    if let Some(assigns) = lower_i64_helper_call_array_index_let_stmts(
        stmt,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(assigns);
    }
    if let Some(assigns) = lower_i64_helper_call_projection_let_stmts(
        stmt,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(assigns);
    }
    if let Some(assigns) = lower_i64_runtime_projection_let_stmts(
        stmt,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(assigns);
    }
    if let Stmt::Let {
        name,
        ty: Type::Bool,
        expr,
        ..
    } = stmt
        && let Some(diag) = i64_http_non_loopback_bind_diag(expr, static_bindings)
    {
        let value = lower_i64_bool_value_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?;
        let local = local_indexes.len();
        local_indexes.insert(name.clone(), local);
        locals.push(CraneliftI64Expr::Literal(0));
        local_conditions.insert(name.clone(), i64_local_truthy_condition(local));
        return Some(vec![
            CraneliftI64Stmt::WriteLine {
                stream: OutputStream::Stderr,
                text: diag,
            },
            CraneliftI64Stmt::Assign(axiomc_backend_cranelift::I64Assign { local, value }),
        ]);
    }
    if let Some(assigns) = lower_i64_aggregate_local_let_stmts(
        stmt,
        locals,
        local_indexes,
        local_conditions,
        static_bindings,
    ) {
        return Some(assigns);
    }
    if let Stmt::Let {
        name,
        ty: Type::Option(inner),
        expr,
        ..
    } = stmt
        && let Some(assigns) = lower_i64_runtime_string_option_call_let_stmts(
            name,
            inner.as_ref(),
            expr,
            locals,
            local_indexes,
            static_bindings,
        )
    {
        return Some(assigns);
    }
    if let Stmt::Let {
        name,
        ty: Type::Option(inner),
        expr: expr @ Expr::Call { .. },
        ..
    } = stmt
        && is_i64_option_local_payload_type_static(inner, static_bindings)
    {
        return lower_i64_scalar_option_call_let_stmts(
            name,
            inner.as_ref(),
            expr,
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        );
    }
    Some(vec![lower_i64_runtime_let(
        stmt,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?])
}

fn lower_i64_helper_call_slice_index_let_stmts(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Stmt::Let {
        name,
        ty,
        expr,
        span,
    } = stmt
    else {
        return None;
    };
    if !matches!(ty, Type::Bool) && !is_i64_compatible_type(ty) {
        return None;
    }
    let (base, index, index_ty, cast_ty) = match expr {
        Expr::Index {
            base,
            index,
            ty: index_ty,
        } => (base, index, index_ty, None),
        Expr::Cast {
            expr: cast_expr,
            ty: cast_ty,
        } if is_i64_compatible_type(cast_ty) => match cast_expr.as_ref() {
            Expr::Index {
                base,
                index,
                ty: index_ty,
            } => (base, index, index_ty, Some(cast_ty.clone())),
            _ => return None,
        },
        _ => return None,
    };
    let Expr::Slice {
        base: slice_base,
        start,
        end,
        ty: slice_ty,
    } = base.as_ref()
    else {
        return None;
    };
    if !matches!(slice_base.as_ref(), Expr::Call { .. }) {
        return None;
    }

    let mut trial_locals = locals.clone();
    let mut trial_indexes = local_indexes.clone();
    let mut trial_conditions = local_conditions.clone();
    let temp_name = format!("__axiom_i64_slice_index_{}", trial_indexes.len());
    let base_ty = slice_base.ty();
    let mut lowered = lower_i64_aggregate_call_to_local_stmts(
        &temp_name,
        &base_ty,
        slice_base.as_ref(),
        &mut trial_locals,
        &mut trial_indexes,
        &mut trial_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let rewritten_expr = Expr::Index {
        base: Box::new(Expr::Slice {
            base: Box::new(Expr::VarRef {
                name: temp_name,
                ty: base_ty,
            }),
            start: start.clone(),
            end: end.clone(),
            ty: slice_ty.clone(),
        }),
        index: index.clone(),
        ty: index_ty.clone(),
    };
    let rewritten_expr = if let Some(cast_ty) = cast_ty {
        Expr::Cast {
            expr: Box::new(rewritten_expr),
            ty: cast_ty,
        }
    } else {
        rewritten_expr
    };
    let rewritten = Stmt::Let {
        name: name.clone(),
        ty: ty.clone(),
        expr: rewritten_expr,
        span: *span,
    };
    lowered.push(lower_i64_runtime_let(
        &rewritten,
        &mut trial_locals,
        &mut trial_indexes,
        &mut trial_conditions,
        helper_signatures,
        static_bindings,
    )?);
    *locals = trial_locals;
    *local_indexes = trial_indexes;
    *local_conditions = trial_conditions;
    Some(lowered)
}

fn lower_i64_helper_call_array_index_let_stmts(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Stmt::Let {
        name,
        ty,
        expr,
        span,
    } = stmt
    else {
        return None;
    };
    if !matches!(ty, Type::Bool) && !is_i64_compatible_type(ty) {
        return None;
    }
    let (base, index, index_ty, cast_ty) = match expr {
        Expr::Index {
            base,
            index,
            ty: index_ty,
        } if matches!(base.as_ref(), Expr::Call { .. }) => (base, index, index_ty, None),
        Expr::Cast {
            expr: cast_expr,
            ty: cast_ty,
        } if is_i64_compatible_type(cast_ty) => match cast_expr.as_ref() {
            Expr::Index {
                base,
                index,
                ty: index_ty,
            } if matches!(base.as_ref(), Expr::Call { .. }) => {
                (base, index, index_ty, Some(cast_ty.clone()))
            }
            _ => return None,
        },
        _ => return None,
    };

    let mut trial_locals = locals.clone();
    let mut trial_indexes = local_indexes.clone();
    let mut trial_conditions = local_conditions.clone();
    let temp_name = format!("__axiom_i64_array_index_{}", trial_indexes.len());
    let base_ty = base.ty();
    let mut lowered = lower_i64_aggregate_call_to_local_stmts(
        &temp_name,
        &base_ty,
        base.as_ref(),
        &mut trial_locals,
        &mut trial_indexes,
        &mut trial_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let rewritten_expr = Expr::Index {
        base: Box::new(Expr::VarRef {
            name: temp_name,
            ty: base_ty,
        }),
        index: index.clone(),
        ty: index_ty.clone(),
    };
    let rewritten_expr = if let Some(cast_ty) = cast_ty {
        Expr::Cast {
            expr: Box::new(rewritten_expr),
            ty: cast_ty,
        }
    } else {
        rewritten_expr
    };
    let rewritten = Stmt::Let {
        name: name.clone(),
        ty: ty.clone(),
        expr: rewritten_expr,
        span: *span,
    };
    lowered.push(lower_i64_runtime_let(
        &rewritten,
        &mut trial_locals,
        &mut trial_indexes,
        &mut trial_conditions,
        helper_signatures,
        static_bindings,
    )?);
    *locals = trial_locals;
    *local_indexes = trial_indexes;
    *local_conditions = trial_conditions;
    Some(lowered)
}

fn i64_is_helper_call_slice_expr(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Slice { base, .. } if matches!(base.as_ref(), Expr::Call { .. })
    )
}

fn i64_is_helper_call_slice_index_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Index { base, .. } => i64_is_helper_call_slice_expr(base.as_ref()),
        Expr::Cast { expr, .. } => i64_is_helper_call_slice_index_expr(expr),
        _ => false,
    }
}

fn i64_has_helper_call_slice_index_arg(args: &[Expr]) -> bool {
    args.iter().any(i64_is_helper_call_slice_index_expr)
}

fn i64_is_helper_call_array_index_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Index { base, .. } => matches!(base.as_ref(), Expr::Call { .. }),
        Expr::Cast { expr, .. } => i64_is_helper_call_array_index_expr(expr),
        _ => false,
    }
}

fn i64_has_helper_call_array_index_arg(args: &[Expr]) -> bool {
    args.iter().any(i64_is_helper_call_array_index_expr)
}

fn lower_i64_helper_call_projection_let_stmts(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Stmt::Let {
        name,
        ty,
        expr,
        span,
        ..
    } = stmt
    else {
        return None;
    };
    if !matches!(ty, Type::Bool) && !is_i64_compatible_type(ty) {
        return None;
    }

    enum HelperCallProjection {
        Tuple { index: usize, ty: Type },
        Field { field: String, ty: Type },
    }

    let mut trial_locals = locals.clone();
    let mut trial_indexes = local_indexes.clone();
    let mut trial_conditions = local_conditions.clone();
    let (temp_prefix, base, projection, cast_ty) = match expr {
        Expr::TupleIndex {
            base,
            index,
            ty: index_ty,
        } if matches!(base.as_ref(), Expr::Call { .. }) => (
            "__axiom_i64_tuple_projection",
            base,
            HelperCallProjection::Tuple {
                index: *index,
                ty: index_ty.clone(),
            },
            None,
        ),
        Expr::FieldAccess {
            base,
            field,
            ty: field_ty,
        } if matches!(base.as_ref(), Expr::Call { .. }) => (
            "__axiom_i64_struct_projection",
            base,
            HelperCallProjection::Field {
                field: field.clone(),
                ty: field_ty.clone(),
            },
            None,
        ),
        Expr::Cast {
            expr: cast_expr,
            ty: cast_ty,
        } if is_i64_compatible_type(cast_ty) => match cast_expr.as_ref() {
            Expr::TupleIndex {
                base,
                index,
                ty: index_ty,
            } if matches!(base.as_ref(), Expr::Call { .. }) => (
                "__axiom_i64_tuple_projection",
                base,
                HelperCallProjection::Tuple {
                    index: *index,
                    ty: index_ty.clone(),
                },
                Some(cast_ty.clone()),
            ),
            Expr::FieldAccess {
                base,
                field,
                ty: field_ty,
            } if matches!(base.as_ref(), Expr::Call { .. }) => (
                "__axiom_i64_struct_projection",
                base,
                HelperCallProjection::Field {
                    field: field.clone(),
                    ty: field_ty.clone(),
                },
                Some(cast_ty.clone()),
            ),
            _ => return None,
        },
        _ => return None,
    };
    let temp_name = format!("{}_{}", temp_prefix, trial_indexes.len());
    let base_ty = base.ty();
    let mut lowered = lower_i64_aggregate_call_to_local_stmts(
        &temp_name,
        &base_ty,
        base.as_ref(),
        &mut trial_locals,
        &mut trial_indexes,
        &mut trial_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let rewritten_base = Box::new(Expr::VarRef {
        name: temp_name,
        ty: base_ty,
    });
    let rewritten_expr = match projection {
        HelperCallProjection::Tuple { index, ty } => Expr::TupleIndex {
            base: rewritten_base,
            index,
            ty,
        },
        HelperCallProjection::Field { field, ty } => Expr::FieldAccess {
            base: rewritten_base,
            field,
            ty,
        },
    };
    let rewritten_expr = if let Some(cast_ty) = cast_ty {
        Expr::Cast {
            expr: Box::new(rewritten_expr),
            ty: cast_ty,
        }
    } else {
        rewritten_expr
    };
    let rewritten = Stmt::Let {
        name: name.clone(),
        ty: ty.clone(),
        expr: rewritten_expr,
        span: *span,
    };
    lowered.push(lower_i64_runtime_let(
        &rewritten,
        &mut trial_locals,
        &mut trial_indexes,
        &mut trial_conditions,
        helper_signatures,
        static_bindings,
    )?);
    *locals = trial_locals;
    *local_indexes = trial_indexes;
    *local_conditions = trial_conditions;
    Some(lowered)
}

fn i64_is_helper_call_projection_expr(expr: &Expr) -> bool {
    match expr {
        Expr::TupleIndex { base, .. } | Expr::FieldAccess { base, .. } => {
            matches!(base.as_ref(), Expr::Call { .. })
        }
        Expr::Cast { expr, .. } => i64_is_helper_call_projection_expr(expr),
        _ => false,
    }
}

fn i64_has_helper_call_projection_arg(args: &[Expr]) -> bool {
    args.iter().any(i64_is_helper_call_projection_expr)
}

fn lower_i64_aggregate_local_let_stmts(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Stmt::Let {
        name,
        ty: target_ty,
        expr: Expr::VarRef {
            name: source,
            ty: source_ty,
        },
        ..
    } = stmt
    else {
        return None;
    };
    if target_ty != source_ty {
        return None;
    }
    let slot_pairs = match target_ty {
        Type::Struct(struct_name) => {
            let struct_def = i64_scalar_static_struct_def(struct_name, static_bindings)?;
            let mut slots = Vec::new();
            for field in &struct_def.fields {
                let local = local_indexes.len();
                let key = i64_struct_projection_key(name, &field.name);
                local_indexes.insert(key.clone(), local);
                locals.push(CraneliftI64Expr::Literal(0));
                if matches!(field.ty, Type::Bool) {
                    local_conditions.insert(key, i64_local_truthy_condition(local));
                }
                slots.push((
                    local,
                    *local_indexes.get(i64_struct_projection_key(source, &field.name).as_str())?,
                ));
            }
            slots
        }
        Type::Tuple(elements) if is_i64_tuple_param_type(elements) => {
            let mut slots = Vec::new();
            for (index, element) in elements.iter().enumerate() {
                let local = local_indexes.len();
                let key = i64_tuple_projection_key(name, index);
                local_indexes.insert(key.clone(), local);
                locals.push(CraneliftI64Expr::Literal(0));
                if matches!(element, Type::Bool) {
                    local_conditions.insert(key, i64_local_truthy_condition(local));
                }
                slots.push((
                    local,
                    *local_indexes.get(i64_tuple_projection_key(source, index).as_str())?,
                ));
            }
            slots
        }
        Type::Array(element, Some(size)) if is_i64_array_param_element_type(element) => {
            let mut slots = Vec::new();
            for index in 0..*size {
                let local = local_indexes.len();
                let key = i64_array_projection_key(name, index);
                local_indexes.insert(key.clone(), local);
                locals.push(CraneliftI64Expr::Literal(0));
                if matches!(element.as_ref(), Type::Bool) {
                    local_conditions.insert(key, i64_local_truthy_condition(local));
                }
                slots.push((
                    local,
                    *local_indexes.get(i64_array_projection_key(source, index).as_str())?,
                ));
            }
            slots
        }
        Type::Option(inner) if is_i64_option_local_payload_type_static(inner, static_bindings) => {
            let mut slots = Vec::new();
            let tag_local = local_indexes.len();
            local_indexes.insert(i64_option_tag_key(name), tag_local);
            locals.push(CraneliftI64Expr::Literal(0));
            slots.push((
                tag_local,
                *local_indexes.get(i64_option_tag_key(source).as_str())?,
            ));
            for (index, source_local) in
                i64_option_payload_locals(source, inner.as_ref(), local_indexes, static_bindings)?
                    .into_iter()
                    .enumerate()
            {
                let payload_local = local_indexes.len();
                local_indexes.insert(i64_option_payload_slot_key(name, index), payload_local);
                if index == 0 {
                    local_indexes.insert(i64_option_payload_key(name), payload_local);
                }
                locals.push(CraneliftI64Expr::Literal(0));
                slots.push((payload_local, source_local));
            }
            slots
        }
        Type::Result(ok, err)
            if is_i64_result_local_payload_type_static(ok, err, static_bindings) =>
        {
            let mut slots = Vec::new();
            let tag_local = local_indexes.len();
            local_indexes.insert(i64_result_tag_key(name), tag_local);
            locals.push(CraneliftI64Expr::Literal(0));
            slots.push((
                tag_local,
                *local_indexes.get(i64_result_tag_key(source).as_str())?,
            ));
            for (index, source_local) in i64_result_payload_locals(
                source,
                ok.as_ref(),
                err.as_ref(),
                local_indexes,
                static_bindings,
            )?
            .into_iter()
            .enumerate()
            {
                let payload_local = local_indexes.len();
                local_indexes.insert(i64_result_payload_slot_key(name, index), payload_local);
                if index == 0 {
                    local_indexes.insert(i64_result_payload_key(name), payload_local);
                }
                locals.push(CraneliftI64Expr::Literal(0));
                slots.push((payload_local, source_local));
            }
            slots
        }
        Type::Enum(enum_name) if is_i64_enum_payload_type(enum_name, static_bindings) => {
            let mut slots = Vec::new();
            let tag_local = local_indexes.len();
            local_indexes.insert(i64_enum_tag_key(name), tag_local);
            locals.push(CraneliftI64Expr::Literal(0));
            slots.push((
                tag_local,
                *local_indexes.get(i64_enum_tag_key(source).as_str())?,
            ));
            for (index, source_local) in
                i64_enum_payload_locals(source, enum_name, static_bindings, local_indexes)?
                    .into_iter()
                    .enumerate()
            {
                let payload_local = local_indexes.len();
                local_indexes.insert(i64_enum_payload_slot_key(name, index), payload_local);
                if index == 0 {
                    local_indexes.insert(i64_enum_payload_key(name), payload_local);
                }
                locals.push(CraneliftI64Expr::Literal(0));
                slots.push((payload_local, source_local));
            }
            slots
        }
        _ => return None,
    };
    Some(
        slot_pairs
            .into_iter()
            .map(|(target, source)| {
                CraneliftI64Stmt::Assign(axiomc_backend_cranelift::I64Assign {
                    local: target,
                    value: CraneliftI64Expr::Local(source),
                })
            })
            .collect(),
    )
}

fn lower_i64_nested_aggregate_call_let_stmts(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Stmt::Let {
        name,
        ty,
        expr:
            Expr::Call {
                name: call_name,
                args,
                ty: call_ty,
            },
        span,
    } = stmt
    else {
        return None;
    };
    if !matches!(ty, Type::Bool) && !is_i64_compatible_type(ty) {
        return None;
    }

    let mut trial_locals = locals.clone();
    let mut trial_indexes = local_indexes.clone();
    let mut trial_conditions = local_conditions.clone();
    let mut lowered = Vec::new();
    let mut rewritten_args = Vec::with_capacity(args.len());
    let mut rewrote_any_arg = false;
    for (index, arg) in args.iter().enumerate() {
        if let Some((rewritten, mut assigns)) = rewrite_i64_nested_aggregate_call_arg(
            arg,
            index,
            &mut trial_locals,
            &mut trial_indexes,
            &mut trial_conditions,
            helper_signatures,
            static_bindings,
        ) {
            lowered.append(&mut assigns);
            rewritten_args.push(rewritten);
            rewrote_any_arg = true;
            continue;
        }
        rewritten_args.push(arg.clone());
    }
    if !rewrote_any_arg {
        return None;
    }

    let rewritten = Stmt::Let {
        name: name.clone(),
        ty: ty.clone(),
        expr: Expr::Call {
            name: call_name.clone(),
            args: rewritten_args,
            ty: call_ty.clone(),
        },
        span: *span,
    };
    lowered.push(lower_i64_runtime_let(
        &rewritten,
        &mut trial_locals,
        &mut trial_indexes,
        &mut trial_conditions,
        helper_signatures,
        static_bindings,
    )?);
    *locals = trial_locals;
    *local_indexes = trial_indexes;
    *local_conditions = trial_conditions;
    Some(lowered)
}

fn rewrite_i64_nested_aggregate_call_arg(
    arg: &Expr,
    arg_index: usize,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<(Expr, Vec<CraneliftI64Stmt>)> {
    if matches!(arg, Expr::Call { .. }) {
        let temp_name = format!("__axiom_i64_arg_{}_{}", local_indexes.len(), arg_index);
        let assigns = lower_i64_aggregate_call_to_local_stmts(
            &temp_name,
            &arg.ty(),
            arg,
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?;
        return Some((
            Expr::VarRef {
                name: temp_name,
                ty: arg.ty(),
            },
            assigns,
        ));
    }

    if let Expr::Index {
        base,
        index,
        ty: index_ty,
    } = arg
        && matches!(base.as_ref(), Expr::Call { .. })
    {
        let temp_name = format!(
            "__axiom_i64_array_index_arg_{}_{}",
            local_indexes.len(),
            arg_index
        );
        let base_ty = base.ty();
        let assigns = lower_i64_aggregate_call_to_local_stmts(
            &temp_name,
            &base_ty,
            base.as_ref(),
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?;
        return Some((
            Expr::Index {
                base: Box::new(Expr::VarRef {
                    name: temp_name,
                    ty: base_ty,
                }),
                index: index.clone(),
                ty: index_ty.clone(),
            },
            assigns,
        ));
    }

    if let Expr::Index {
        base,
        index,
        ty: index_ty,
    } = arg
        && let Expr::Slice {
            base: slice_base,
            start,
            end,
            ty: slice_ty,
        } = base.as_ref()
        && matches!(slice_base.as_ref(), Expr::Call { .. })
    {
        let temp_name = format!(
            "__axiom_i64_slice_index_arg_{}_{}",
            local_indexes.len(),
            arg_index
        );
        let base_ty = slice_base.ty();
        let assigns = lower_i64_aggregate_call_to_local_stmts(
            &temp_name,
            &base_ty,
            slice_base.as_ref(),
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?;
        return Some((
            Expr::Index {
                base: Box::new(Expr::Slice {
                    base: Box::new(Expr::VarRef {
                        name: temp_name,
                        ty: base_ty,
                    }),
                    start: start.clone(),
                    end: end.clone(),
                    ty: slice_ty.clone(),
                }),
                index: index.clone(),
                ty: index_ty.clone(),
            },
            assigns,
        ));
    }

    if let Expr::Cast { expr, ty: cast_ty } = arg
        && is_i64_compatible_type(cast_ty)
        && let Some((rewritten, assigns)) = rewrite_i64_nested_aggregate_call_arg(
            expr,
            arg_index,
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )
    {
        return Some((
            Expr::Cast {
                expr: Box::new(rewritten),
                ty: cast_ty.clone(),
            },
            assigns,
        ));
    }

    if let Expr::TupleIndex {
        base,
        index,
        ty: index_ty,
    } = arg
        && matches!(base.as_ref(), Expr::Call { .. })
    {
        let temp_name = format!(
            "__axiom_i64_tuple_projection_arg_{}_{}",
            local_indexes.len(),
            arg_index
        );
        let base_ty = base.ty();
        let assigns = lower_i64_aggregate_call_to_local_stmts(
            &temp_name,
            &base_ty,
            base.as_ref(),
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?;
        return Some((
            Expr::TupleIndex {
                base: Box::new(Expr::VarRef {
                    name: temp_name,
                    ty: base_ty,
                }),
                index: *index,
                ty: index_ty.clone(),
            },
            assigns,
        ));
    }

    if let Expr::FieldAccess {
        base,
        field,
        ty: field_ty,
    } = arg
        && matches!(base.as_ref(), Expr::Call { .. })
    {
        let temp_name = format!(
            "__axiom_i64_struct_projection_arg_{}_{}",
            local_indexes.len(),
            arg_index
        );
        let base_ty = base.ty();
        let assigns = lower_i64_aggregate_call_to_local_stmts(
            &temp_name,
            &base_ty,
            base.as_ref(),
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?;
        return Some((
            Expr::FieldAccess {
                base: Box::new(Expr::VarRef {
                    name: temp_name,
                    ty: base_ty,
                }),
                field: field.clone(),
                ty: field_ty.clone(),
            },
            assigns,
        ));
    }

    let Expr::Slice {
        base,
        start,
        end,
        ty,
    } = arg
    else {
        return None;
    };
    if !matches!(base.as_ref(), Expr::Call { .. }) {
        return None;
    }
    let temp_name = format!(
        "__axiom_i64_slice_arg_{}_{}",
        local_indexes.len(),
        arg_index
    );
    let base_ty = base.ty();
    let assigns = lower_i64_aggregate_call_to_local_stmts(
        &temp_name,
        &base_ty,
        base.as_ref(),
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    Some((
        Expr::Slice {
            base: Box::new(Expr::VarRef {
                name: temp_name,
                ty: base_ty,
            }),
            start: start.clone(),
            end: end.clone(),
            ty: ty.clone(),
        },
        assigns,
    ))
}

fn lower_i64_aggregate_call_to_local_stmts(
    name: &str,
    ty: &Type,
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Expr::Call {
        name: call_name,
        args,
        ..
    } = expr
    else {
        return None;
    };
    match ty {
        Type::Struct(struct_name) => lower_i64_struct_call_let_stmts(
            name,
            struct_name,
            call_name,
            args,
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        ),
        Type::Tuple(elements) if is_i64_tuple_param_type(elements) => {
            lower_i64_tuple_call_let_stmts(
                name,
                elements,
                call_name,
                args,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        Type::Array(element, Some(size)) if is_i64_array_param_element_type(element) => {
            lower_i64_array_call_let_stmts(
                name,
                element.as_ref(),
                *size,
                call_name,
                args,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        Type::Option(inner) if is_i64_option_local_payload_type_static(inner, static_bindings) => {
            lower_i64_option_call_let_stmts(
                name,
                inner.as_ref(),
                call_name,
                args,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        Type::Result(ok, err)
            if is_i64_result_local_payload_type_static(ok, err, static_bindings) =>
        {
            lower_i64_result_call_let_stmts(
                name,
                ok.as_ref(),
                err.as_ref(),
                call_name,
                args,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        Type::Enum(enum_name) if is_i64_enum_payload_type(enum_name, static_bindings) => {
            lower_i64_enum_call_let_stmts(
                name,
                enum_name,
                call_name,
                args,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        _ => None,
    }
}

fn lower_i64_runtime_string_len_let_stmts(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Stmt::Let {
        name,
        ty: Type::String | Type::Str,
        expr,
        ..
    } = stmt
    else {
        return None;
    };
    let value = lower_i64_string_len_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let json_safe = lower_i64_json_safe_string_len_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )
    .is_some();
    let local = local_indexes.len();
    local_indexes.insert(i64_string_len_key(name), local);
    locals.push(CraneliftI64Expr::Literal(0));
    let mut assigns = vec![CraneliftI64Stmt::Assign(
        axiomc_backend_cranelift::I64Assign { local, value },
    )];
    if json_safe {
        let json_safe_local = local_indexes.len();
        local_indexes.insert(i64_json_safe_string_len_key(name), json_safe_local);
        locals.push(CraneliftI64Expr::Literal(0));
        assigns.push(CraneliftI64Stmt::Assign(
            axiomc_backend_cranelift::I64Assign {
                local: json_safe_local,
                value: CraneliftI64Expr::Local(local),
            },
        ));
    }
    assigns.extend(lower_i64_runtime_printable_string_alias_stmts(
        name,
        expr,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?);
    Some(assigns)
}

fn lower_i64_runtime_printable_string_alias_stmts(
    name: &str,
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Some((key, value, is_bool)) = lower_i64_printable_string_alias_parts(
        name,
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) else {
        return Some(Vec::new());
    };
    let local = local_indexes.len();
    local_indexes.insert(key.clone(), local);
    locals.push(CraneliftI64Expr::Literal(0));
    if is_bool {
        local_conditions.insert(key, i64_local_truthy_condition(local));
    }
    Some(vec![CraneliftI64Stmt::Assign(
        axiomc_backend_cranelift::I64Assign { local, value },
    )])
}

fn lower_i64_eprintln_let_stmts(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Stmt::Let {
        name,
        ty,
        expr: Expr::Call {
            name: call_name,
            args,
            ..
        },
        ..
    } = stmt
    else {
        return None;
    };
    if !is_i64_compatible_type(ty) || !is_i64_io_eprintln_name(call_name, static_bindings) {
        if is_i64_log_info_attrs_name(call_name, static_bindings) {
            let [message, attributes] = args.as_slice() else {
                return None;
            };
            let (mut stmts, written) = lower_i64_log_event_output_stmts(
                "info",
                message,
                Some(attributes),
                OutputStream::Stderr,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            let local = local_indexes.len();
            local_indexes.insert(name.clone(), local);
            locals.push(CraneliftI64Expr::Literal(0));
            stmts.push(CraneliftI64Stmt::Assign(
                axiomc_backend_cranelift::I64Assign {
                    local,
                    value: written,
                },
            ));
            return Some(stmts);
        }
        if let Some(level) = i64_log_level_wrapper(call_name, static_bindings) {
            let [message] = args.as_slice() else {
                return None;
            };
            let (mut stmts, written) = lower_i64_log_event_output_stmts(
                level,
                message,
                None,
                OutputStream::Stderr,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            let local = local_indexes.len();
            local_indexes.insert(name.clone(), local);
            locals.push(CraneliftI64Expr::Literal(0));
            stmts.push(CraneliftI64Stmt::Assign(
                axiomc_backend_cranelift::I64Assign {
                    local,
                    value: written,
                },
            ));
            return Some(stmts);
        }
        return None;
    }
    let [message] = args.as_slice() else {
        return None;
    };
    let (mut stmts, written) = lower_i64_eprintln_message_stmts(
        message,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let local = local_indexes.len();
    local_indexes.insert(name.clone(), local);
    locals.push(CraneliftI64Expr::Literal(0));
    stmts.push(CraneliftI64Stmt::Assign(
        axiomc_backend_cranelift::I64Assign {
            local,
            value: written,
        },
    ));
    Some(stmts)
}

fn lower_i64_eprintln_message_stmts(
    message: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<(Vec<CraneliftI64Stmt>, CraneliftI64Expr)> {
    if let Some(text) = i64_string_text(message, static_bindings) {
        let written = i64::try_from(text.len()).ok()?.checked_add(1)?;
        return Some((
            vec![CraneliftI64Stmt::WriteLine {
                stream: OutputStream::Stderr,
                text,
            }],
            CraneliftI64Expr::Literal(written),
        ));
    }
    if let Some((stmts, written)) = lower_i64_dynamic_known_string_line_stmts_with_written(
        message,
        OutputStream::Stderr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some((stmts, written));
    }
    if let Expr::VarRef {
        name,
        ty: Type::String | Type::Str,
    } = message
    {
        if let Some(local) = local_indexes.get(i64_printable_i64_string_key(name).as_str()) {
            let value = CraneliftI64Expr::Local(*local);
            return Some((
                vec![CraneliftI64Stmt::WriteI64Line {
                    stream: OutputStream::Stderr,
                    value: value.clone(),
                }],
                CraneliftI64Expr::Binary {
                    op: CraneliftI64BinaryOp::Add,
                    lhs: Box::new(i64_decimal_string_len_expr(value)),
                    rhs: Box::new(CraneliftI64Expr::Literal(1)),
                },
            ));
        }
        if let Some(cond) = local_conditions
            .get(i64_printable_bool_string_key(name).as_str())
            .cloned()
        {
            return Some(lower_i64_eprintln_bool_message_stmts(cond));
        }
    }
    if let Expr::Call { name, args, .. } = message {
        if is_i64_log_event_name(name, static_bindings) {
            let [level, message, attributes] = args.as_slice() else {
                return None;
            };
            let level = i64_string_text(level, static_bindings)?;
            return lower_i64_log_event_output_stmts(
                &level,
                message,
                Some(attributes),
                OutputStream::Stderr,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            );
        }
        if is_i64_json_stringify_int_name(name, static_bindings) {
            let [value] = args.as_slice() else {
                return None;
            };
            let value = lower_i64_expr(
                value,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            return Some((
                vec![CraneliftI64Stmt::WriteI64Line {
                    stream: OutputStream::Stderr,
                    value: value.clone(),
                }],
                CraneliftI64Expr::Binary {
                    op: CraneliftI64BinaryOp::Add,
                    lhs: Box::new(i64_decimal_string_len_expr(value)),
                    rhs: Box::new(CraneliftI64Expr::Literal(1)),
                },
            ));
        }
        if is_i64_json_stringify_bool_name(name, static_bindings) {
            let [value] = args.as_slice() else {
                return None;
            };
            let cond = lower_i64_condition(
                value,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            return Some(lower_i64_eprintln_bool_message_stmts(cond));
        }
        if is_i64_json_stringify_string_name(name, static_bindings) {
            return lower_i64_json_stringify_string_line_stmts_with_written(
                message,
                OutputStream::Stderr,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            );
        }
    }
    None
}

fn lower_i64_eprintln_bool_message_stmts(
    cond: CraneliftI64Condition,
) -> (Vec<CraneliftI64Stmt>, CraneliftI64Expr) {
    (
        vec![CraneliftI64Stmt::If {
            cond: cond.clone(),
            then_body: vec![CraneliftI64Stmt::WriteLine {
                stream: OutputStream::Stderr,
                text: String::from("true"),
            }],
            else_body: vec![CraneliftI64Stmt::WriteLine {
                stream: OutputStream::Stderr,
                text: String::from("false"),
            }],
        }],
        CraneliftI64Expr::Select {
            cond: Box::new(cond),
            then_result: Box::new(CraneliftI64Expr::Literal(5)),
            else_result: Box::new(CraneliftI64Expr::Literal(6)),
        },
    )
}

fn lower_i64_log_event_output_stmts(
    level: &str,
    message: &Expr,
    attributes: Option<&Expr>,
    stream: OutputStream,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<(Vec<CraneliftI64Stmt>, CraneliftI64Expr)> {
    let level_text = json_escape_string(level);
    let mut stmts = vec![
        CraneliftI64Stmt::WriteText {
            stream,
            text: String::from("{\"level\":"),
        },
        CraneliftI64Stmt::WriteText {
            stream,
            text: level_text.clone(),
        },
        CraneliftI64Stmt::WriteText {
            stream,
            text: String::from(",\"message\":"),
        },
    ];
    stmts.extend(lower_i64_log_json_string_value_stmts(
        message,
        stream,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?);
    stmts.push(CraneliftI64Stmt::WriteText {
        stream,
        text: String::from(",\"attributes\":{"),
    });
    if let Some(attributes) = attributes {
        stmts.extend(lower_i64_log_fields_output_stmts(
            attributes,
            stream,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?);
    }
    stmts.push(CraneliftI64Stmt::WriteLine {
        stream,
        text: String::from("}}"),
    });

    let prefix_len = i64::try_from("{\"level\":".len()).ok()?
        + i64::try_from(level_text.len()).ok()?
        + i64::try_from(",\"message\":".len()).ok()?;
    let suffix_len =
        i64::try_from(",\"attributes\":{".len()).ok()? + i64::try_from("}}".len()).ok()? + 1;
    let message_len = lower_i64_json_escaped_string_len_expr(
        message,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let attributes_len = if let Some(attributes) = attributes {
        lower_i64_string_len_expr(
            attributes,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?
    } else {
        CraneliftI64Expr::Literal(0)
    };
    Some((
        stmts,
        CraneliftI64Expr::Binary {
            op: CraneliftI64BinaryOp::Add,
            lhs: Box::new(CraneliftI64Expr::Literal(prefix_len + suffix_len)),
            rhs: Box::new(CraneliftI64Expr::Binary {
                op: CraneliftI64BinaryOp::Add,
                lhs: Box::new(message_len),
                rhs: Box::new(attributes_len),
            }),
        },
    ))
}

fn lower_i64_log_json_string_value_stmts(
    expr: &Expr,
    stream: OutputStream,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    if let Some(text) = i64_string_text(expr, static_bindings) {
        return Some(vec![CraneliftI64Stmt::WriteText {
            stream,
            text: json_escape_string(&text),
        }]);
    }
    if let Some(stmts) = lower_i64_dynamic_known_string_text_stmts(
        expr,
        stream,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
        |value| json_escape_string(value),
    ) {
        return Some(stmts);
    }
    if let Expr::VarRef {
        name,
        ty: Type::String | Type::Str,
    } = expr
    {
        if let Some(local) = local_indexes.get(i64_printable_i64_string_key(name).as_str()) {
            return Some(vec![
                CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("\""),
                },
                CraneliftI64Stmt::WriteI64Text {
                    stream,
                    value: CraneliftI64Expr::Local(*local),
                },
                CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("\""),
                },
            ]);
        }
        if let Some(cond) = local_conditions
            .get(i64_printable_bool_string_key(name).as_str())
            .cloned()
        {
            return Some(vec![CraneliftI64Stmt::If {
                cond,
                then_body: vec![CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("\"true\""),
                }],
                else_body: vec![CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("\"false\""),
                }],
            }]);
        }
    }
    if let Expr::Call { name, args, .. } = expr {
        if is_i64_json_stringify_int_name(name, static_bindings) {
            let [value] = args.as_slice() else {
                return None;
            };
            return Some(vec![
                CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("\""),
                },
                CraneliftI64Stmt::WriteI64Text {
                    stream,
                    value: lower_i64_expr(
                        value,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )?,
                },
                CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("\""),
                },
            ]);
        }
        if is_i64_json_stringify_bool_name(name, static_bindings) {
            let [value] = args.as_slice() else {
                return None;
            };
            return Some(vec![CraneliftI64Stmt::If {
                cond: lower_i64_condition(
                    value,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )?,
                then_body: vec![CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("\"true\""),
                }],
                else_body: vec![CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("\"false\""),
                }],
            }]);
        }
        if is_i64_json_stringify_string_name(name, static_bindings) {
            let [value] = args.as_slice() else {
                return None;
            };
            let mut stmts = vec![CraneliftI64Stmt::WriteText {
                stream,
                text: String::from("\""),
            }];
            stmts.extend(lower_i64_log_json_string_content_stmts(
                value,
                stream,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?);
            stmts.push(CraneliftI64Stmt::WriteText {
                stream,
                text: String::from("\""),
            });
            return Some(stmts);
        }
    }
    None
}

fn lower_i64_log_fields_output_stmts(
    expr: &Expr,
    stream: OutputStream,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    if let Some(text) = i64_string_text(expr, static_bindings) {
        return Some(vec![CraneliftI64Stmt::WriteText { stream, text }]);
    }
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if is_i64_log_field_string_name(name, static_bindings) {
        let [key, value] = args.as_slice() else {
            return None;
        };
        let mut stmts = vec![
            CraneliftI64Stmt::WriteText {
                stream,
                text: json_escape_string(&i64_string_text(key, static_bindings)?),
            },
            CraneliftI64Stmt::WriteText {
                stream,
                text: String::from(":"),
            },
        ];
        stmts.extend(lower_i64_log_json_string_value_stmts(
            value,
            stream,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?);
        return Some(stmts);
    }
    if is_i64_log_field_int_name(name, static_bindings) {
        let [key, value] = args.as_slice() else {
            return None;
        };
        return Some(vec![
            CraneliftI64Stmt::WriteText {
                stream,
                text: json_escape_string(&i64_string_text(key, static_bindings)?),
            },
            CraneliftI64Stmt::WriteText {
                stream,
                text: String::from(":"),
            },
            CraneliftI64Stmt::WriteI64Text {
                stream,
                value: lower_i64_expr(
                    value,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )?,
            },
        ]);
    }
    if is_i64_log_field_bool_name(name, static_bindings) {
        let [key, value] = args.as_slice() else {
            return None;
        };
        return Some(vec![
            CraneliftI64Stmt::WriteText {
                stream,
                text: json_escape_string(&i64_string_text(key, static_bindings)?),
            },
            CraneliftI64Stmt::WriteText {
                stream,
                text: String::from(":"),
            },
            CraneliftI64Stmt::If {
                cond: lower_i64_condition(
                    value,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )?,
                then_body: vec![CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("true"),
                }],
                else_body: vec![CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("false"),
                }],
            },
        ]);
    }
    if is_i64_log_fields2_name(name, static_bindings)
        || is_i64_log_fields3_name(name, static_bindings)
    {
        let mut iter = args.iter();
        let first = iter.next()?;
        let mut stmts = lower_i64_log_fields_output_stmts(
            first,
            stream,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?;
        for field in iter {
            stmts.push(CraneliftI64Stmt::WriteText {
                stream,
                text: String::from(","),
            });
            stmts.extend(lower_i64_log_fields_output_stmts(
                field,
                stream,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?);
        }
        return Some(stmts);
    }
    None
}

fn lower_i64_print_stmt_stmts(
    stmt: &Stmt,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Stmt::Print { expr, .. } = stmt else {
        return None;
    };
    if let Some(text) = i64_string_text(expr, static_bindings) {
        return Some(vec![CraneliftI64Stmt::WriteLine {
            stream: OutputStream::Stdout,
            text,
        }]);
    }
    if let Some(stmts) = lower_i64_dynamic_known_string_line_stmts(
        expr,
        OutputStream::Stdout,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(stmts);
    }
    if let Expr::VarRef {
        name,
        ty: Type::String | Type::Str,
    } = expr
    {
        if local_indexes.contains_key(i64_printable_i64_string_key(name).as_str()) {
            return lower_i64_print_i64_line_stmts(
                &Expr::VarRef {
                    name: i64_printable_i64_string_key(name),
                    ty: Type::Int,
                },
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            );
        }
        if local_conditions.contains_key(i64_printable_bool_string_key(name).as_str()) {
            return lower_i64_print_bool_line_stmts(
                &Expr::VarRef {
                    name: i64_printable_bool_string_key(name),
                    ty: Type::Bool,
                },
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            );
        }
    }
    if let Expr::Call { name, args, .. } = expr {
        if is_i64_log_event_name(name, static_bindings) {
            let [level, message, attributes] = args.as_slice() else {
                return None;
            };
            let level = i64_string_text(level, static_bindings)?;
            let (stmts, _) = lower_i64_log_event_output_stmts(
                &level,
                message,
                Some(attributes),
                OutputStream::Stdout,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            return Some(stmts);
        }
        if is_i64_json_stringify_int_name(name, static_bindings) {
            let [value] = args.as_slice() else {
                return None;
            };
            return lower_i64_print_i64_line_stmts(
                value,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            );
        }
        if is_i64_json_stringify_bool_name(name, static_bindings) {
            let [value] = args.as_slice() else {
                return None;
            };
            return lower_i64_print_bool_line_stmts(
                value,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            );
        }
        if is_i64_json_stringify_string_name(name, static_bindings) {
            return lower_i64_json_stringify_string_line_stmts(
                expr,
                OutputStream::Stdout,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            );
        }
    }
    if matches!(expr.ty(), Type::Bool) {
        return lower_i64_print_bool_line_stmts(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        );
    }
    let ty = expr.ty();
    if !is_i64_compatible_type(&ty) {
        return None;
    }
    lower_i64_print_i64_line_stmts(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )
}

fn lower_i64_print_i64_line_stmts(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    Some(vec![CraneliftI64Stmt::WriteI64Line {
        stream: OutputStream::Stdout,
        value: lower_i64_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
    }])
}

fn lower_i64_print_bool_line_stmts(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    // An http server given a non-loopback bind refuses to serve and reports
    // the structured runtime error on stderr, matching the generated-runtime
    // contract, before printing the false result.
    if let Some(diag) = i64_http_non_loopback_bind_diag(expr, static_bindings) {
        return Some(vec![
            CraneliftI64Stmt::WriteLine {
                stream: OutputStream::Stderr,
                text: diag,
            },
            CraneliftI64Stmt::WriteLine {
                stream: OutputStream::Stdout,
                text: String::from("false"),
            },
        ]);
    }
    Some(vec![CraneliftI64Stmt::If {
        cond: lower_i64_condition(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
        then_body: vec![CraneliftI64Stmt::WriteLine {
            stream: OutputStream::Stdout,
            text: String::from("true"),
        }],
        else_body: vec![CraneliftI64Stmt::WriteLine {
            stream: OutputStream::Stdout,
            text: String::from("false"),
        }],
    }])
}

/// Structured runtime error an http server reports on stderr when its bind
/// address is not loopback-only, matching the generated-runtime contract.
const HTTP_NON_LOOPBACK_BIND_DIAG: &str =
    "{\"kind\":\"net\",\"message\":\"http server bind address must resolve only to loopback\"}";

fn lower_i64_dynamic_known_string_line_stmts(
    expr: &Expr,
    stream: OutputStream,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    lower_i64_dynamic_known_string_line_stmts_with_written(
        expr,
        stream,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )
    .map(|(stmts, _)| stmts)
}

fn lower_i64_dynamic_known_string_line_stmts_with_written(
    expr: &Expr,
    stream: OutputStream,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<(Vec<CraneliftI64Stmt>, CraneliftI64Expr)> {
    let (keys, index, transform) = i64_map_key_array_string_index_source(expr, static_bindings)?;
    if keys.is_empty() {
        return None;
    }
    if let Some(index) = lower_i64_literal_index(&index) {
        let text = i64_map_key_text(keys.get(index)?, transform)?;
        let written = i64_newline_byte_count(&text)?;
        return Some((
            vec![CraneliftI64Stmt::WriteLine { stream, text }],
            CraneliftI64Expr::Literal(written),
        ));
    }
    let index = lower_i64_expr(
        &index,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let last = keys.len() - 1;
    let fallback_text = i64_map_key_text(keys.get(last)?, transform)?;
    let fallback_written = i64_newline_byte_count(&fallback_text)?;
    let mut stmts = vec![CraneliftI64Stmt::WriteLine {
        stream,
        text: fallback_text,
    }];
    let mut written = CraneliftI64Expr::Literal(fallback_written);
    for candidate in (0..last).rev() {
        let text = i64_map_key_text(keys.get(candidate)?, transform)?;
        let candidate_written = i64_newline_byte_count(&text)?;
        let cond = CraneliftI64Condition::Compare(CraneliftI64Compare {
            op: CraneliftI64CompareOp::Eq,
            lhs: index.clone(),
            rhs: CraneliftI64Expr::Literal(candidate as i64),
        });
        stmts = vec![CraneliftI64Stmt::If {
            cond: cond.clone(),
            then_body: vec![CraneliftI64Stmt::WriteLine { stream, text }],
            else_body: stmts,
        }];
        written = CraneliftI64Expr::Select {
            cond: Box::new(cond),
            then_result: Box::new(CraneliftI64Expr::Literal(candidate_written)),
            else_result: Box::new(written),
        };
    }
    Some((stmts, written))
}

fn lower_i64_dynamic_known_string_text_stmts(
    expr: &Expr,
    stream: OutputStream,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    map_text: impl Fn(&str) -> String,
) -> Option<Vec<CraneliftI64Stmt>> {
    let (keys, index, transform) = i64_map_key_array_string_index_source(expr, static_bindings)?;
    if keys.is_empty() {
        return None;
    }
    if let Some(index) = lower_i64_literal_index(&index) {
        let text = map_text(&i64_map_key_text(keys.get(index)?, transform)?);
        return Some(vec![CraneliftI64Stmt::WriteText { stream, text }]);
    }
    let index = lower_i64_expr(
        &index,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let last = keys.len() - 1;
    let fallback_text = map_text(&i64_map_key_text(keys.get(last)?, transform)?);
    let mut stmts = vec![CraneliftI64Stmt::WriteText {
        stream,
        text: fallback_text,
    }];
    for candidate in (0..last).rev() {
        let text = map_text(&i64_map_key_text(keys.get(candidate)?, transform)?);
        stmts = vec![CraneliftI64Stmt::If {
            cond: CraneliftI64Condition::Compare(CraneliftI64Compare {
                op: CraneliftI64CompareOp::Eq,
                lhs: index.clone(),
                rhs: CraneliftI64Expr::Literal(candidate as i64),
            }),
            then_body: vec![CraneliftI64Stmt::WriteText { stream, text }],
            else_body: stmts,
        }];
    }
    Some(stmts)
}

fn i64_map_key_text(key: &I64MapKey, transform: I64MapKeyArrayStringTransform) -> Option<String> {
    match key {
        I64MapKey::Text(value) => {
            Some(i64_apply_map_key_array_string_transform(value, transform).to_string())
        }
        _ => None,
    }
}

fn i64_newline_byte_count(text: &str) -> Option<i64> {
    i64::try_from(text.len()).ok()?.checked_add(1)
}

fn i64_printable_i64_string_key(name: &str) -> String {
    format!("{name}#print_i64_string")
}

fn i64_printable_bool_string_key(name: &str) -> String {
    format!("{name}#print_bool_string")
}

fn lower_i64_runtime_projection_let_stmts(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Stmt::Let { name, ty, expr, .. } = stmt else {
        return None;
    };
    match (ty, expr) {
        (Type::Struct(_), Expr::StructLiteral { fields, .. }) => {
            lower_i64_runtime_struct_projection_let_stmts(
                name,
                fields,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        (
            Type::Struct(struct_name),
            Expr::Call {
                name: call_name,
                args,
                ..
            },
        ) => lower_i64_struct_call_let_stmts(
            name,
            struct_name,
            call_name,
            args,
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        ),
        (Type::Tuple(_), Expr::TupleLiteral { elements, .. }) => {
            lower_i64_runtime_indexed_projection_let_stmts(
                name,
                elements,
                i64_tuple_projection_key,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        (
            Type::Tuple(elements),
            Expr::Call {
                name: call_name,
                args,
                ..
            },
        ) if is_i64_tuple_param_type(elements) => lower_i64_tuple_call_let_stmts(
            name,
            elements,
            call_name,
            args,
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        ),
        (
            Type::Option(inner),
            expr @ Expr::Call {
                name: call_name,
                args,
                ..
            },
        ) if is_i64_option_local_payload_type_static(inner, static_bindings) => {
            lower_i64_known_scalar_option_call_let_stmts(
                name,
                inner.as_ref(),
                expr,
                locals,
                local_indexes,
                static_bindings,
            )
            .or_else(|| {
                lower_i64_option_call_let_stmts(
                    name,
                    inner.as_ref(),
                    call_name,
                    args,
                    locals,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )
            })
        }
        (
            Type::Option(inner),
            Expr::EnumVariant {
                enum_name,
                variant,
                payloads,
                ..
            },
        ) if enum_name == "Option"
            && is_i64_option_local_payload_type_static(inner, static_bindings) =>
        {
            lower_i64_option_literal_let_stmts(
                name,
                inner.as_ref(),
                variant,
                payloads,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        (
            Type::Result(ok, err),
            Expr::Call {
                name: call_name,
                args,
                ..
            },
        ) if is_i64_result_local_payload_type_static(ok, err, static_bindings) => {
            lower_i64_result_call_let_stmts(
                name,
                ok.as_ref(),
                err.as_ref(),
                call_name,
                args,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        (
            Type::Result(ok, err),
            Expr::EnumVariant {
                enum_name,
                variant,
                payloads,
                ..
            },
        ) if enum_name == "Result"
            && is_i64_result_local_payload_type_static(ok, err, static_bindings) =>
        {
            lower_i64_result_literal_let_stmts(
                name,
                ok.as_ref(),
                err.as_ref(),
                variant,
                payloads,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        (
            Type::Enum(enum_name),
            Expr::EnumVariant {
                enum_name: expr_enum,
                variant,
                payloads,
                ..
            },
        ) if enum_name == expr_enum && is_i64_enum_payload_type(enum_name, static_bindings) => {
            lower_i64_enum_literal_let_stmts(
                name,
                enum_name,
                variant,
                payloads,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        (
            Type::Enum(enum_name),
            Expr::Call {
                name: call_name,
                args,
                ..
            },
        ) if is_i64_enum_payload_type(enum_name, static_bindings) => lower_i64_enum_call_let_stmts(
            name,
            enum_name,
            call_name,
            args,
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        ),
        (Type::Array(_, _), Expr::ArrayLiteral { elements, .. }) => {
            lower_i64_runtime_indexed_projection_let_stmts(
                name,
                elements,
                i64_array_projection_key,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        (
            Type::Array(element, Some(size)),
            Expr::Call {
                name: call_name,
                args,
                ..
            },
        ) if is_i64_array_param_element_type(element) => lower_i64_array_call_let_stmts(
            name,
            element.as_ref(),
            *size,
            call_name,
            args,
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        ),
        _ => None,
    }
}

fn lower_i64_option_literal_let_stmts(
    name: &str,
    inner: &Type,
    variant: &str,
    payloads: &[Expr],
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let payload_slots = i64_option_payload_slot_count_static(inner, static_bindings)?;
    let (tag, payloads) = match (variant, payloads) {
        ("Some", [payload]) => (
            CraneliftI64Expr::Literal(1),
            lower_i64_option_payload_exprs(
                payload,
                payload_slots,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?,
        ),
        ("None", []) => (
            CraneliftI64Expr::Literal(0),
            vec![CraneliftI64Expr::Literal(0); payload_slots],
        ),
        _ => return None,
    };
    let mut assigns = Vec::with_capacity(1 + payload_slots);
    let tag_local = local_indexes.len();
    local_indexes.insert(i64_option_tag_key(name), tag_local);
    locals.push(CraneliftI64Expr::Literal(0));
    assigns.push(CraneliftI64Stmt::Assign(
        axiomc_backend_cranelift::I64Assign {
            local: tag_local,
            value: tag,
        },
    ));
    for (index, payload) in payloads.into_iter().enumerate() {
        let payload_local = local_indexes.len();
        local_indexes.insert(i64_option_payload_slot_key(name, index), payload_local);
        if index == 0 {
            local_indexes.insert(i64_option_payload_key(name), payload_local);
        }
        locals.push(CraneliftI64Expr::Literal(0));
        assigns.push(CraneliftI64Stmt::Assign(
            axiomc_backend_cranelift::I64Assign {
                local: payload_local,
                value: payload,
            },
        ));
    }
    Some(assigns)
}

fn lower_i64_known_scalar_option_call_let_stmts(
    name: &str,
    inner: &Type,
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let value = match inner {
        Type::Bool => {
            i64_bool_option_value(expr, static_bindings)?.map(|value| if value { 1 } else { 0 })
        }
        ty if is_i64_compatible_type(ty) => i64_i64_option_value(expr, static_bindings)?,
        _ => return None,
    };
    let payload_slots = i64_option_payload_slot_count_static(inner, static_bindings)?;
    if payload_slots != 1 {
        return None;
    }
    let mut assigns = Vec::with_capacity(2);
    let tag_local = local_indexes.len();
    local_indexes.insert(i64_option_tag_key(name), tag_local);
    locals.push(CraneliftI64Expr::Literal(0));
    let payload_local = local_indexes.len();
    local_indexes.insert(i64_option_payload_slot_key(name, 0), payload_local);
    local_indexes.insert(i64_option_payload_key(name), payload_local);
    locals.push(CraneliftI64Expr::Literal(0));
    let (tag, payload) = match value {
        Some(value) => (
            CraneliftI64Expr::Literal(1),
            CraneliftI64Expr::Literal(value),
        ),
        None => (CraneliftI64Expr::Literal(0), CraneliftI64Expr::Literal(0)),
    };
    assigns.push(CraneliftI64Stmt::Assign(
        axiomc_backend_cranelift::I64Assign {
            local: tag_local,
            value: tag,
        },
    ));
    assigns.push(CraneliftI64Stmt::Assign(
        axiomc_backend_cranelift::I64Assign {
            local: payload_local,
            value: payload,
        },
    ));
    Some(assigns)
}

fn is_i64_known_once_call_let(expr: &Expr, static_bindings: &I64StaticBindings) -> bool {
    matches!(expr, Expr::Call { name, .. } if is_i64_sync_once_name(name, static_bindings) || is_i64_sync_once_with_name(name, static_bindings))
}

fn lower_i64_known_once_call_let(
    name: &str,
    expr: &Expr,
    static_bindings: &mut I64StaticBindings,
) -> Option<()> {
    if let Some(value) = i64_i64_once_cell_value(expr, static_bindings) {
        static_bindings
            .i64_once_cells
            .insert(name.to_string(), value);
        return Some(());
    }
    if let Some(value) = i64_bool_once_cell_value(expr, static_bindings) {
        static_bindings
            .bool_once_cells
            .insert(name.to_string(), value);
        return Some(());
    }
    None
}

fn is_i64_known_channel_call_let(expr: &Expr, static_bindings: &I64StaticBindings) -> bool {
    matches!(expr, Expr::Call { name, .. } if is_i64_sync_channel_name(name, static_bindings) || is_i64_sync_send_name(name, static_bindings))
}

fn lower_i64_known_channel_call_let(
    name: &str,
    expr: &Expr,
    static_bindings: &mut I64StaticBindings,
) -> Option<()> {
    if let Some(value) = i64_i64_channel_cell_value(expr, static_bindings) {
        static_bindings.i64_channels.insert(name.to_string(), value);
        return Some(());
    }
    if let Some(value) = i64_bool_channel_cell_value(expr, static_bindings) {
        static_bindings
            .bool_channels
            .insert(name.to_string(), value);
        return Some(());
    }
    None
}

fn is_i64_known_string_option_call_let_type(inner: &Type) -> bool {
    matches!(inner, Type::String | Type::Str)
}

fn lower_i64_known_string_option_call_let_stmts(
    name: &str,
    inner: &Type,
    expr: &Expr,
    static_bindings: &mut I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    if !is_i64_known_string_option_call_let_type(inner) {
        return None;
    }
    let value = i64_string_option_text(expr, static_bindings)?;
    static_bindings
        .string_options
        .insert(name.to_string(), value);
    Some(Vec::new())
}

fn lower_i64_string_option_len_call_let_stmts(
    name: &str,
    payload_len: CraneliftI64Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
) -> Option<Vec<CraneliftI64Stmt>> {
    let payload_local = local_indexes.len();
    local_indexes.insert(i64_option_payload_slot_key(name, 0), payload_local);
    local_indexes.insert(i64_option_payload_key(name), payload_local);
    locals.push(CraneliftI64Expr::Literal(0));

    let tag_local = local_indexes.len();
    local_indexes.insert(i64_option_tag_key(name), tag_local);
    locals.push(CraneliftI64Expr::Literal(0));

    let tag = CraneliftI64Expr::Select {
        cond: Box::new(CraneliftI64Condition::Compare(CraneliftI64Compare {
            op: CraneliftI64CompareOp::Ge,
            lhs: CraneliftI64Expr::Local(payload_local),
            rhs: CraneliftI64Expr::Literal(0),
        })),
        then_result: Box::new(CraneliftI64Expr::Literal(1)),
        else_result: Box::new(CraneliftI64Expr::Literal(0)),
    };
    Some(vec![
        CraneliftI64Stmt::Assign(axiomc_backend_cranelift::I64Assign {
            local: payload_local,
            value: payload_len,
        }),
        CraneliftI64Stmt::Assign(axiomc_backend_cranelift::I64Assign {
            local: tag_local,
            value: tag,
        }),
    ])
}

fn lower_i64_runtime_string_option_call_let_stmts(
    name: &str,
    inner: &Type,
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    if !matches!(inner, Type::String | Type::Str) {
        return None;
    }
    let payload_len = lower_i64_runtime_string_option_len_expr(expr, static_bindings)?;
    lower_i64_string_option_len_call_let_stmts(name, payload_len, locals, local_indexes)
}

fn lower_i64_runtime_string_option_len_expr(
    expr: &Expr,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if let Some(key) = i64_env_get_key(expr, static_bindings) {
        return i64_env_len_expr(&key, static_bindings);
    }
    let Expr::Call {
        name: call_name,
        args,
        ..
    } = expr
    else {
        return None;
    };
    if is_i64_io_readline_name(call_name, static_bindings) && args.is_empty() {
        return Some(CraneliftI64Expr::StdinLineLen {
            max_bytes: I64_STDIN_BUFFER_BYTES,
        });
    }
    if let Some(path) = i64_fs_read_path(expr, static_bindings) {
        return i64_fs_read_file_len_expr(&path.candidate, path.requested_len, static_bindings);
    }
    let host = i64_net_resolve_host(expr, static_bindings)?;
    i64_net_resolve_len_expr(&host, static_bindings)
}

fn lower_i64_result_literal_let_stmts(
    name: &str,
    ok: &Type,
    err: &Type,
    variant: &str,
    payloads: &[Expr],
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let payload_slots = i64_result_payload_slot_count_static(ok, static_bindings)?
        .max(i64_result_payload_slot_count_static(err, static_bindings)?);
    let (tag, payloads) = lower_i64_result_variant_parts(
        variant,
        payloads,
        payload_slots,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let mut assigns = Vec::with_capacity(1 + payload_slots);
    let tag_local = local_indexes.len();
    local_indexes.insert(i64_result_tag_key(name), tag_local);
    locals.push(CraneliftI64Expr::Literal(0));
    assigns.push(CraneliftI64Stmt::Assign(
        axiomc_backend_cranelift::I64Assign {
            local: tag_local,
            value: tag,
        },
    ));
    for (index, payload) in payloads.into_iter().enumerate() {
        let payload_local = local_indexes.len();
        local_indexes.insert(i64_result_payload_slot_key(name, index), payload_local);
        if index == 0 {
            local_indexes.insert(i64_result_payload_key(name), payload_local);
        }
        locals.push(CraneliftI64Expr::Literal(0));
        assigns.push(CraneliftI64Stmt::Assign(
            axiomc_backend_cranelift::I64Assign {
                local: payload_local,
                value: payload,
            },
        ));
    }
    Some(assigns)
}

fn lower_i64_enum_literal_let_stmts(
    name: &str,
    enum_name: &str,
    variant: &str,
    payloads: &[Expr],
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let payload_slots = i64_enum_payload_slot_count(enum_name, static_bindings)?;
    let (tag, payloads) = lower_i64_enum_variant_parts(
        enum_name,
        variant,
        payloads,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let mut assigns = Vec::with_capacity(1 + payload_slots);
    let tag_local = local_indexes.len();
    local_indexes.insert(i64_enum_tag_key(name), tag_local);
    locals.push(CraneliftI64Expr::Literal(0));
    assigns.push(CraneliftI64Stmt::Assign(
        axiomc_backend_cranelift::I64Assign {
            local: tag_local,
            value: tag,
        },
    ));
    for (index, payload) in payloads.into_iter().enumerate() {
        let payload_local = local_indexes.len();
        local_indexes.insert(i64_enum_payload_slot_key(name, index), payload_local);
        if index == 0 {
            local_indexes.insert(i64_enum_payload_key(name), payload_local);
        }
        locals.push(CraneliftI64Expr::Literal(0));
        assigns.push(CraneliftI64Stmt::Assign(
            axiomc_backend_cranelift::I64Assign {
                local: payload_local,
                value: payload,
            },
        ));
    }
    Some(assigns)
}

fn lower_i64_tuple_call_let_stmts(
    name: &str,
    elements: &[Type],
    call_name: &str,
    args: &[Expr],
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let signature = helper_signatures.get(call_name)?;
    if args.len() != signature.params
        || signature.returns != elements.len()
        || !matches!(&signature.return_ty, Type::Tuple(return_elements) if return_elements == elements)
    {
        return None;
    }
    let mut lowered_args = Vec::new();
    for (index, arg) in args.iter().enumerate() {
        let struct_fields = signature
            .struct_fields
            .get(index)
            .and_then(|fields| fields.as_deref());
        lowered_args.extend(lower_i64_call_arg_exprs(
            arg,
            struct_fields,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?);
    }
    let mut assign_locals = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        let local = local_indexes.len();
        let key = i64_tuple_projection_key(name, index);
        local_indexes.insert(key.clone(), local);
        locals.push(CraneliftI64Expr::Literal(0));
        assign_locals.push(local);
        if matches!(element, Type::Bool) {
            local_conditions.insert(key, i64_local_truthy_condition(local));
        }
    }
    Some(vec![CraneliftI64Stmt::CallAssign {
        locals: assign_locals,
        function: signature.function,
        args: lowered_args,
    }])
}

fn lower_i64_array_call_let_stmts(
    name: &str,
    element: &Type,
    size: usize,
    call_name: &str,
    args: &[Expr],
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let signature = helper_signatures.get(call_name)?;
    if args.len() != signature.params
        || signature.returns != size
        || !matches!(&signature.return_ty, Type::Array(return_element, Some(return_size)) if return_element.as_ref() == element && *return_size == size)
        || !is_i64_array_param_element_type(element)
    {
        return None;
    }
    let mut lowered_args = Vec::new();
    for (index, arg) in args.iter().enumerate() {
        let struct_fields = signature
            .struct_fields
            .get(index)
            .and_then(|fields| fields.as_deref());
        lowered_args.extend(lower_i64_call_arg_exprs(
            arg,
            struct_fields,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?);
    }
    let mut assign_locals = Vec::new();
    for index in 0..size {
        let local = local_indexes.len();
        let key = i64_array_projection_key(name, index);
        local_indexes.insert(key.clone(), local);
        locals.push(CraneliftI64Expr::Literal(0));
        assign_locals.push(local);
        if matches!(element, Type::Bool) {
            local_conditions.insert(key, i64_local_truthy_condition(local));
        }
    }
    Some(vec![CraneliftI64Stmt::CallAssign {
        locals: assign_locals,
        function: signature.function,
        args: lowered_args,
    }])
}

fn lower_i64_struct_call_let_stmts(
    name: &str,
    struct_name: &str,
    call_name: &str,
    args: &[Expr],
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    if let Some(stmts) = lower_i64_time_struct_call_let_stmts(
        name,
        struct_name,
        call_name,
        args,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(stmts);
    }
    if is_i64_http_accept_name(call_name, static_bindings) {
        let [server] = args else {
            return None;
        };
        let stream_local = local_indexes.len();
        local_indexes.insert(i64_struct_projection_key(name, "stream"), stream_local);
        locals.push(CraneliftI64Expr::Literal(0));
        return Some(vec![CraneliftI64Stmt::Assign(
            axiomc_backend_cranelift::I64Assign {
                local: stream_local,
                value: CraneliftI64Expr::HttpServerAccept {
                    server: Box::new(lower_i64_expr(
                        server,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )?),
                },
            },
        )]);
    }
    let signature = helper_signatures.get(call_name)?;
    let struct_def = i64_scalar_static_struct_def(struct_name, static_bindings)?;
    if args.len() != signature.params
        || signature.returns != struct_def.fields.len()
        || !matches!(&signature.return_ty, Type::Struct(return_name) if return_name == struct_name)
    {
        return None;
    }
    let mut lowered_args = Vec::new();
    for (index, arg) in args.iter().enumerate() {
        let struct_fields = signature
            .struct_fields
            .get(index)
            .and_then(|fields| fields.as_deref());
        lowered_args.extend(lower_i64_call_arg_exprs(
            arg,
            struct_fields,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?);
    }
    let mut assign_locals = Vec::new();
    for field in &struct_def.fields {
        let local = local_indexes.len();
        let key = i64_struct_projection_key(name, &field.name);
        local_indexes.insert(key.clone(), local);
        locals.push(CraneliftI64Expr::Literal(0));
        assign_locals.push(local);
        if matches!(field.ty, Type::Bool) {
            local_conditions.insert(key, i64_local_truthy_condition(local));
        }
    }
    Some(vec![CraneliftI64Stmt::CallAssign {
        locals: assign_locals,
        function: signature.function,
        args: lowered_args,
    }])
}

fn lower_i64_time_struct_call_let_stmts(
    name: &str,
    struct_name: &str,
    call_name: &str,
    args: &[Expr],
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let value = lower_i64_time_struct_call_ms_expr(
        struct_name,
        call_name,
        args,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let local = local_indexes.len();
    local_indexes.insert(i64_struct_projection_key(name, "ms"), local);
    locals.push(CraneliftI64Expr::Literal(0));
    Some(vec![CraneliftI64Stmt::Assign(
        axiomc_backend_cranelift::I64Assign { local, value },
    )])
}

fn lower_i64_time_struct_call_ms_expr(
    struct_name: &str,
    call_name: &str,
    args: &[Expr],
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let struct_def = i64_scalar_static_struct_def(struct_name, static_bindings)?;
    let [field] = struct_def.fields.as_slice() else {
        return None;
    };
    if field.name != "ms" || !is_i64_compatible_type(&field.ty) {
        return None;
    }
    let value = if is_i64_time_now_name(call_name, static_bindings) {
        let [] = args else {
            return None;
        };
        CraneliftI64Expr::ClockNowMs
    } else if is_i64_time_duration_ms_name(call_name, static_bindings) {
        let [milliseconds] = args else {
            return None;
        };
        lower_i64_expr(
            milliseconds,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?
    } else {
        return None;
    };
    Some(value)
}

fn lower_i64_scalar_option_call_let_stmts(
    name: &str,
    inner: &Type,
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Expr::Call {
        name: call_name,
        args,
        ..
    } = expr
    else {
        return None;
    };
    if let Some(assigns) = lower_i64_known_scalar_option_call_let_stmts(
        name,
        inner,
        expr,
        locals,
        local_indexes,
        static_bindings,
    ) {
        return Some(assigns);
    }
    if let Some(assigns) = lower_i64_dynamic_map_get_option_call_let_stmts(
        name,
        inner,
        expr,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(assigns);
    }
    lower_i64_option_call_let_stmts(
        name,
        inner,
        call_name,
        args,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )
}

fn lower_i64_option_call_let_stmts(
    name: &str,
    inner: &Type,
    call_name: &str,
    args: &[Expr],
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let signature = helper_signatures.get(call_name)?;
    let payload_slots = i64_option_payload_slot_count_static(inner, static_bindings)?;
    if args.len() != signature.params
        || signature.returns != 1 + payload_slots
        || !matches!(&signature.return_ty, Type::Option(return_inner) if return_inner.as_ref() == inner)
    {
        return None;
    }
    let lowered_args = lower_i64_flat_call_args(
        args,
        signature,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    lower_i64_tagged_payload_call_assign_stmts(
        name,
        payload_slots,
        signature.function,
        lowered_args,
        locals,
        local_indexes,
        i64_option_tag_key,
        i64_option_payload_slot_key,
        i64_option_payload_key,
    )
}

fn lower_i64_result_call_let_stmts(
    name: &str,
    ok: &Type,
    err: &Type,
    call_name: &str,
    args: &[Expr],
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let signature = helper_signatures.get(call_name)?;
    let payload_slots = i64_result_payload_slot_count_static(ok, static_bindings)?
        .max(i64_result_payload_slot_count_static(err, static_bindings)?);
    if args.len() != signature.params
        || signature.returns != 1 + payload_slots
        || !matches!(&signature.return_ty, Type::Result(return_ok, return_err) if return_ok.as_ref() == ok && return_err.as_ref() == err)
    {
        return None;
    }
    let lowered_args = lower_i64_flat_call_args(
        args,
        signature,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    lower_i64_tagged_payload_call_assign_stmts(
        name,
        payload_slots,
        signature.function,
        lowered_args,
        locals,
        local_indexes,
        i64_result_tag_key,
        i64_result_payload_slot_key,
        i64_result_payload_key,
    )
}

fn lower_i64_enum_call_let_stmts(
    name: &str,
    enum_name: &str,
    call_name: &str,
    args: &[Expr],
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let signature = helper_signatures.get(call_name)?;
    let payload_slots = i64_enum_payload_slot_count(enum_name, static_bindings)?;
    if args.len() != signature.params
        || signature.returns != 1 + payload_slots
        || !matches!(&signature.return_ty, Type::Enum(return_name) if return_name == enum_name)
    {
        return None;
    }
    let lowered_args = lower_i64_flat_call_args(
        args,
        signature,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    lower_i64_tagged_payload_call_assign_stmts(
        name,
        payload_slots,
        signature.function,
        lowered_args,
        locals,
        local_indexes,
        i64_enum_tag_key,
        i64_enum_payload_slot_key,
        i64_enum_payload_key,
    )
}

fn lower_i64_tagged_payload_call_assign_stmts(
    name: &str,
    payload_slots: usize,
    function: usize,
    args: Vec<CraneliftI64Expr>,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    tag_key: fn(&str) -> String,
    payload_slot_key: fn(&str, usize) -> String,
    payload_key: fn(&str) -> String,
) -> Option<Vec<CraneliftI64Stmt>> {
    let mut assign_locals = Vec::with_capacity(1 + payload_slots);
    let tag_local = local_indexes.len();
    local_indexes.insert(tag_key(name), tag_local);
    locals.push(CraneliftI64Expr::Literal(0));
    assign_locals.push(tag_local);
    for index in 0..payload_slots {
        let payload_local = local_indexes.len();
        local_indexes.insert(payload_slot_key(name, index), payload_local);
        if index == 0 {
            local_indexes.insert(payload_key(name), payload_local);
        }
        locals.push(CraneliftI64Expr::Literal(0));
        assign_locals.push(payload_local);
    }
    Some(vec![CraneliftI64Stmt::CallAssign {
        locals: assign_locals,
        function,
        args,
    }])
}

fn lower_i64_flat_call_args(
    args: &[Expr],
    signature: &I64HelperSignature,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Expr>> {
    let mut lowered_args = Vec::new();
    for (index, arg) in args.iter().enumerate() {
        let struct_fields = signature
            .struct_fields
            .get(index)
            .and_then(|fields| fields.as_deref());
        lowered_args.extend(lower_i64_call_arg_exprs(
            arg,
            struct_fields,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?);
    }
    Some(lowered_args)
}

fn lower_i64_projection_assign_stmts(
    stmt: &Stmt,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Stmt::Assign {
        target: Expr::VarRef { name, ty },
        expr,
        ..
    } = stmt
    else {
        return None;
    };
    match (ty, expr) {
        (Type::Struct(_), Expr::StructLiteral { fields, .. }) => fields
            .iter()
            .map(|field| {
                lower_i64_projection_reassign(
                    i64_struct_projection_key(name, &field.name),
                    &field.expr,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )
            })
            .collect(),
        (Type::Tuple(_), Expr::TupleLiteral { elements, .. }) => elements
            .iter()
            .enumerate()
            .map(|(index, element)| {
                lower_i64_projection_reassign(
                    i64_tuple_projection_key(name, index),
                    element,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )
            })
            .collect(),
        (Type::Array(_, _), Expr::ArrayLiteral { elements, .. }) => elements
            .iter()
            .enumerate()
            .map(|(index, element)| {
                lower_i64_projection_reassign(
                    i64_array_projection_key(name, index),
                    element,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )
            })
            .collect(),
        _ => None,
    }
}

fn lower_i64_option_assign_stmts(
    stmt: &Stmt,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Stmt::Assign {
        target: Expr::VarRef {
            name,
            ty: Type::Option(inner),
        },
        expr:
            Expr::EnumVariant {
                enum_name,
                variant,
                payloads,
                ..
            },
        ..
    } = stmt
    else {
        return None;
    };
    if enum_name != "Option" || !is_i64_option_local_payload_type_static(inner, static_bindings) {
        return None;
    }
    let tag_local = *local_indexes.get(i64_option_tag_key(name).as_str())?;
    let payload_slot_count = i64_option_payload_slot_count_static(inner.as_ref(), static_bindings)?;
    let (tag, payloads) = match (variant.as_str(), payloads.as_slice()) {
        ("Some", [payload]) => (
            CraneliftI64Expr::Literal(1),
            lower_i64_option_payload_exprs(
                payload,
                payload_slot_count,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?,
        ),
        ("None", []) => (
            CraneliftI64Expr::Literal(0),
            vec![CraneliftI64Expr::Literal(0); payload_slot_count],
        ),
        _ => return None,
    };
    let mut assigns = vec![CraneliftI64Stmt::Assign(
        axiomc_backend_cranelift::I64Assign {
            local: tag_local,
            value: tag,
        },
    )];
    for (index, value) in payloads.into_iter().enumerate() {
        let local = *local_indexes.get(i64_option_payload_slot_key(name, index).as_str())?;
        assigns.push(CraneliftI64Stmt::Assign(
            axiomc_backend_cranelift::I64Assign { local, value },
        ));
    }
    Some(assigns)
}

fn lower_i64_result_assign_stmts(
    stmt: &Stmt,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Stmt::Assign {
        target: Expr::VarRef {
            name,
            ty: Type::Result(ok, err),
        },
        expr:
            Expr::EnumVariant {
                enum_name,
                variant,
                payloads,
                ..
            },
        ..
    } = stmt
    else {
        return None;
    };
    if enum_name != "Result" || !is_i64_result_local_payload_type_static(ok, err, static_bindings) {
        return None;
    }
    let tag_local = *local_indexes.get(i64_result_tag_key(name).as_str())?;
    let payload_slot_count =
        i64_result_payload_slot_count_static(ok.as_ref(), static_bindings)?.max(
            i64_result_payload_slot_count_static(err.as_ref(), static_bindings)?,
        );
    let (tag, payload) = lower_i64_result_variant_parts(
        variant,
        payloads,
        payload_slot_count,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let mut assigns = vec![CraneliftI64Stmt::Assign(
        axiomc_backend_cranelift::I64Assign {
            local: tag_local,
            value: tag,
        },
    )];
    for (index, value) in payload.into_iter().enumerate() {
        let local = *local_indexes.get(i64_result_payload_slot_key(name, index).as_str())?;
        assigns.push(CraneliftI64Stmt::Assign(
            axiomc_backend_cranelift::I64Assign { local, value },
        ));
    }
    Some(assigns)
}

fn lower_i64_aggregate_call_assign_stmts(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Stmt::Assign {
        target: Expr::VarRef { name, ty },
        expr: Expr::Call {
            name: call_name,
            args,
            ..
        },
        ..
    } = stmt
    else {
        return None;
    };
    let signature = helper_signatures.get(call_name.as_str())?;
    let assign_locals = match ty {
        Type::Struct(struct_name) => {
            let struct_def = i64_scalar_static_struct_def(struct_name, static_bindings)?;
            if signature.returns != struct_def.fields.len()
                || !matches!(&signature.return_ty, Type::Struct(return_name) if return_name == struct_name)
            {
                return None;
            }
            struct_def
                .fields
                .iter()
                .map(|field| {
                    local_indexes
                        .get(i64_struct_projection_key(name, &field.name).as_str())
                        .copied()
                })
                .collect::<Option<Vec<_>>>()?
        }
        Type::Tuple(elements) if is_i64_tuple_param_type(elements) => {
            if signature.returns != elements.len()
                || !matches!(&signature.return_ty, Type::Tuple(return_elements) if return_elements == elements)
            {
                return None;
            }
            (0..elements.len())
                .map(|index| {
                    local_indexes
                        .get(i64_tuple_projection_key(name, index).as_str())
                        .copied()
                })
                .collect::<Option<Vec<_>>>()?
        }
        Type::Array(element, Some(size)) if is_i64_array_param_element_type(element) => {
            if signature.returns != *size
                || !matches!(&signature.return_ty, Type::Array(return_element, Some(return_size)) if return_element.as_ref() == element.as_ref() && *return_size == *size)
            {
                return None;
            }
            (0..*size)
                .map(|index| {
                    local_indexes
                        .get(i64_array_projection_key(name, index).as_str())
                        .copied()
                })
                .collect::<Option<Vec<_>>>()?
        }
        Type::Option(inner) if is_i64_option_local_payload_type_static(inner, static_bindings) => {
            let payload_slots =
                i64_option_payload_slot_count_static(inner.as_ref(), static_bindings)?;
            if signature.returns != 1 + payload_slots
                || !matches!(&signature.return_ty, Type::Option(return_inner) if return_inner.as_ref() == inner.as_ref())
            {
                return None;
            }
            let mut locals = vec![*local_indexes.get(i64_option_tag_key(name).as_str())?];
            locals.extend(i64_option_payload_locals(
                name,
                inner.as_ref(),
                local_indexes,
                static_bindings,
            )?);
            locals
        }
        Type::Result(ok, err)
            if is_i64_result_local_payload_type_static(ok, err, static_bindings) =>
        {
            let payload_slots =
                i64_result_payload_slot_count_static(ok.as_ref(), static_bindings)?.max(
                    i64_result_payload_slot_count_static(err.as_ref(), static_bindings)?,
                );
            if signature.returns != 1 + payload_slots
                || !matches!(&signature.return_ty, Type::Result(return_ok, return_err) if return_ok.as_ref() == ok.as_ref() && return_err.as_ref() == err.as_ref())
            {
                return None;
            }
            let mut locals = vec![*local_indexes.get(i64_result_tag_key(name).as_str())?];
            locals.extend(i64_result_payload_locals(
                name,
                ok.as_ref(),
                err.as_ref(),
                local_indexes,
                static_bindings,
            )?);
            locals
        }
        Type::Enum(enum_name) if is_i64_enum_payload_type(enum_name, static_bindings) => {
            let payload_slots = i64_enum_payload_slot_count(enum_name, static_bindings)?;
            if signature.returns != 1 + payload_slots
                || !matches!(&signature.return_ty, Type::Enum(return_name) if return_name == enum_name)
            {
                return None;
            }
            let mut locals = vec![*local_indexes.get(i64_enum_tag_key(name).as_str())?];
            locals.extend(i64_enum_payload_locals(
                name,
                enum_name,
                static_bindings,
                local_indexes,
            )?);
            locals
        }
        _ => return None,
    };
    if args.len() != signature.params {
        return None;
    }

    let mut rewritten_args = Vec::with_capacity(args.len());
    let mut setup = Vec::new();
    let mut trial_locals = locals.clone();
    let mut trial_indexes = local_indexes.clone();
    let mut trial_conditions = local_conditions.clone();
    let mut rewrote_any_arg = false;
    for (index, arg) in args.iter().enumerate() {
        if let Some((rewritten, mut assigns)) = rewrite_i64_nested_aggregate_call_arg(
            arg,
            index,
            &mut trial_locals,
            &mut trial_indexes,
            &mut trial_conditions,
            helper_signatures,
            static_bindings,
        ) {
            setup.append(&mut assigns);
            rewritten_args.push(rewritten);
            rewrote_any_arg = true;
            continue;
        }
        rewritten_args.push(arg.clone());
    }
    let (args, local_indexes, local_conditions) = if rewrote_any_arg {
        *locals = trial_locals;
        (rewritten_args.as_slice(), &trial_indexes, &trial_conditions)
    } else {
        (args.as_slice(), local_indexes, local_conditions)
    };
    setup.push(CraneliftI64Stmt::CallAssign {
        locals: assign_locals,
        function: signature.function,
        args: lower_i64_flat_call_args(
            args,
            signature,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
    });
    Some(setup)
}

fn lower_i64_enum_assign_stmts(
    stmt: &Stmt,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Stmt::Assign {
        target: Expr::VarRef {
            name,
            ty: Type::Enum(enum_name),
        },
        expr:
            Expr::EnumVariant {
                enum_name: expr_enum,
                variant,
                payloads,
                ..
            },
        ..
    } = stmt
    else {
        return None;
    };
    if enum_name != expr_enum {
        return None;
    }
    let tag_local = *local_indexes.get(i64_enum_tag_key(name).as_str())?;
    let (tag, payloads) = lower_i64_enum_variant_parts(
        enum_name,
        variant,
        payloads,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let mut assigns = vec![CraneliftI64Stmt::Assign(
        axiomc_backend_cranelift::I64Assign {
            local: tag_local,
            value: tag,
        },
    )];
    for (index, value) in payloads.into_iter().enumerate() {
        let local = *local_indexes.get(i64_enum_payload_slot_key(name, index).as_str())?;
        assigns.push(CraneliftI64Stmt::Assign(
            axiomc_backend_cranelift::I64Assign { local, value },
        ));
    }
    Some(assigns)
}

fn lower_i64_projection_reassign(
    key: String,
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Stmt> {
    let value = match expr.ty() {
        Type::Bool => lower_i64_bool_value_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
        ty if is_i64_compatible_type(&ty) => lower_i64_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
        _ => return None,
    };
    Some(CraneliftI64Stmt::Assign(
        axiomc_backend_cranelift::I64Assign {
            local: *local_indexes.get(key.as_str())?,
            value,
        },
    ))
}

fn lower_i64_assign(
    stmt: &Stmt,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<axiomc_backend_cranelift::I64Assign> {
    let Stmt::Assign {
        target: Expr::VarRef { name, ty },
        expr,
        ..
    } = stmt
    else {
        return None;
    };
    let value = match ty {
        Type::Bool => lower_i64_bool_value_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
        ty if is_i64_compatible_type(ty) => lower_i64_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
        _ => return None,
    };
    Some(axiomc_backend_cranelift::I64Assign {
        local: *local_indexes.get(name.as_str())?,
        value,
    })
}

fn lower_i64_return_block(
    stmts: &[Stmt],
    locals: &mut Vec<CraneliftI64Expr>,
    mut local_indexes: HashMap<String, usize>,
    mut local_conditions: HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    allow_stdio_effects: bool,
) -> Option<CraneliftI64ReturnBlock> {
    let (terminal_stmt, body_stmts) = stmts.split_last()?;
    let mut stmts = Vec::new();
    let mut scoped_static_bindings = static_bindings.clone();
    let static_bindings = &mut scoped_static_bindings;
    for stmt in body_stmts {
        match stmt {
            Stmt::Let { .. } => {
                if record_i64_known_string_let(stmt, static_bindings).unwrap_or(false) {
                    continue;
                }
                if record_i64_known_map_let(stmt, static_bindings).unwrap_or(false) {
                    continue;
                }
                if record_i64_known_map_key_array_let(stmt, static_bindings).unwrap_or(false) {
                    continue;
                }
                stmts.extend(lower_i64_runtime_let_stmts(
                    stmt,
                    locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
                    allow_stdio_effects,
                )?);
            }
            _ => {
                stmts.extend(lower_i64_runtime_stmt_stmts(
                    stmt,
                    locals,
                    local_indexes.clone(),
                    local_conditions.clone(),
                    helper_signatures,
                    static_bindings,
                    allow_stdio_effects,
                )?);
            }
        }
    }
    let result = match terminal_stmt {
        Stmt::Return { expr, .. } => {
            if let Some(block) = lower_i64_helper_call_slice_index_return_block(
                expr,
                locals,
                &local_indexes,
                &local_conditions,
                helper_signatures,
                static_bindings,
            ) {
                stmts.extend(block.stmts);
                block.result
            } else if let Some(block) = lower_i64_helper_call_array_index_return_block(
                expr,
                locals,
                &local_indexes,
                &local_conditions,
                helper_signatures,
                static_bindings,
            ) {
                stmts.extend(block.stmts);
                block.result
            } else if let Some(block) = lower_i64_helper_call_projection_return_block(
                expr,
                locals,
                &local_indexes,
                &local_conditions,
                helper_signatures,
                static_bindings,
            ) {
                stmts.extend(block.stmts);
                block.result
            } else {
                lower_i64_return_value_expr(
                    expr,
                    &local_indexes,
                    &local_conditions,
                    helper_signatures,
                    static_bindings,
                )?
            }
        }
        Stmt::Panic { message, .. } => {
            stmts.extend(lower_i64_panic_report_stmts(
                message,
                &local_indexes,
                &local_conditions,
                helper_signatures,
                static_bindings,
            )?);
            CraneliftI64Expr::Literal(1)
        }
        _ => return None,
    };
    Some(CraneliftI64ReturnBlock { stmts, result })
}

fn record_i64_known_string_let(
    stmt: &Stmt,
    static_bindings: &mut I64StaticBindings,
) -> Option<bool> {
    let (name, expr) = match stmt {
        Stmt::Let {
            name,
            ty: Type::String | Type::Str,
            expr,
            ..
        } => (name, expr),
        Stmt::Assign {
            target:
                Expr::VarRef {
                    name,
                    ty: Type::String | Type::Str,
                },
            expr,
            ..
        } => (name, expr),
        _ => return None,
    };
    let Some(text) = i64_string_text(expr, static_bindings) else {
        return Some(false);
    };
    static_bindings.strings.insert(name.clone(), text);
    Some(true)
}

fn lower_i64_helper_call_array_index_return_block(
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64ReturnBlock> {
    if !i64_is_helper_call_array_index_expr(expr) {
        return None;
    }
    let (base, index, index_ty, cast_ty) = match expr {
        Expr::Index {
            base,
            index,
            ty: index_ty,
        } if matches!(base.as_ref(), Expr::Call { .. }) => (base, index, index_ty, None),
        Expr::Cast {
            expr: cast_expr,
            ty: cast_ty,
        } if is_i64_compatible_type(cast_ty) => match cast_expr.as_ref() {
            Expr::Index {
                base,
                index,
                ty: index_ty,
            } if matches!(base.as_ref(), Expr::Call { .. }) => {
                (base, index, index_ty, Some(cast_ty.clone()))
            }
            _ => return None,
        },
        _ => return None,
    };

    let mut trial_locals = locals.clone();
    let mut trial_indexes = local_indexes.clone();
    let mut trial_conditions = local_conditions.clone();
    let temp_name = format!("__axiom_i64_array_index_return_{}", trial_indexes.len());
    let base_ty = base.ty();
    let stmts = lower_i64_aggregate_call_to_local_stmts(
        &temp_name,
        &base_ty,
        base.as_ref(),
        &mut trial_locals,
        &mut trial_indexes,
        &mut trial_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let rewritten_expr = Expr::Index {
        base: Box::new(Expr::VarRef {
            name: temp_name,
            ty: base_ty,
        }),
        index: index.clone(),
        ty: index_ty.clone(),
    };
    let rewritten_expr = if let Some(cast_ty) = cast_ty {
        Expr::Cast {
            expr: Box::new(rewritten_expr),
            ty: cast_ty,
        }
    } else {
        rewritten_expr
    };
    let result = lower_i64_return_value_expr(
        &rewritten_expr,
        &trial_indexes,
        &trial_conditions,
        helper_signatures,
        static_bindings,
    )?;
    *locals = trial_locals;
    Some(CraneliftI64ReturnBlock { stmts, result })
}

fn lower_i64_helper_call_slice_index_return_block(
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64ReturnBlock> {
    if !i64_is_helper_call_slice_index_expr(expr) {
        return None;
    }
    let (base, index, index_ty, cast_ty) = match expr {
        Expr::Index {
            base,
            index,
            ty: index_ty,
        } => (base, index, index_ty, None),
        Expr::Cast {
            expr: cast_expr,
            ty: cast_ty,
        } if is_i64_compatible_type(cast_ty) => match cast_expr.as_ref() {
            Expr::Index {
                base,
                index,
                ty: index_ty,
            } => (base, index, index_ty, Some(cast_ty.clone())),
            _ => return None,
        },
        _ => return None,
    };
    let Expr::Slice {
        base: slice_base,
        start,
        end,
        ty: slice_ty,
    } = base.as_ref()
    else {
        return None;
    };
    if !matches!(slice_base.as_ref(), Expr::Call { .. }) {
        return None;
    }

    let mut trial_locals = locals.clone();
    let mut trial_indexes = local_indexes.clone();
    let mut trial_conditions = local_conditions.clone();
    let temp_name = format!("__axiom_i64_slice_index_return_{}", trial_indexes.len());
    let base_ty = slice_base.ty();
    let stmts = lower_i64_aggregate_call_to_local_stmts(
        &temp_name,
        &base_ty,
        slice_base.as_ref(),
        &mut trial_locals,
        &mut trial_indexes,
        &mut trial_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let rewritten_expr = Expr::Index {
        base: Box::new(Expr::Slice {
            base: Box::new(Expr::VarRef {
                name: temp_name,
                ty: base_ty,
            }),
            start: start.clone(),
            end: end.clone(),
            ty: slice_ty.clone(),
        }),
        index: index.clone(),
        ty: index_ty.clone(),
    };
    let rewritten_expr = if let Some(cast_ty) = cast_ty {
        Expr::Cast {
            expr: Box::new(rewritten_expr),
            ty: cast_ty,
        }
    } else {
        rewritten_expr
    };
    let result = lower_i64_return_value_expr(
        &rewritten_expr,
        &trial_indexes,
        &trial_conditions,
        helper_signatures,
        static_bindings,
    )?;
    *locals = trial_locals;
    Some(CraneliftI64ReturnBlock { stmts, result })
}

fn lower_i64_helper_call_projection_return_block(
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64ReturnBlock> {
    if !i64_is_helper_call_projection_expr(expr) {
        return None;
    }

    enum HelperCallProjection {
        Tuple { index: usize, ty: Type },
        Field { field: String, ty: Type },
    }

    let (temp_prefix, base, projection, cast_ty) = match expr {
        Expr::TupleIndex {
            base,
            index,
            ty: index_ty,
        } if matches!(base.as_ref(), Expr::Call { .. }) => (
            "__axiom_i64_tuple_projection_return",
            base,
            HelperCallProjection::Tuple {
                index: *index,
                ty: index_ty.clone(),
            },
            None,
        ),
        Expr::FieldAccess {
            base,
            field,
            ty: field_ty,
        } if matches!(base.as_ref(), Expr::Call { .. }) => (
            "__axiom_i64_struct_projection_return",
            base,
            HelperCallProjection::Field {
                field: field.clone(),
                ty: field_ty.clone(),
            },
            None,
        ),
        Expr::Cast {
            expr: cast_expr,
            ty: cast_ty,
        } if is_i64_compatible_type(cast_ty) => match cast_expr.as_ref() {
            Expr::TupleIndex {
                base,
                index,
                ty: index_ty,
            } if matches!(base.as_ref(), Expr::Call { .. }) => (
                "__axiom_i64_tuple_projection_return",
                base,
                HelperCallProjection::Tuple {
                    index: *index,
                    ty: index_ty.clone(),
                },
                Some(cast_ty.clone()),
            ),
            Expr::FieldAccess {
                base,
                field,
                ty: field_ty,
            } if matches!(base.as_ref(), Expr::Call { .. }) => (
                "__axiom_i64_struct_projection_return",
                base,
                HelperCallProjection::Field {
                    field: field.clone(),
                    ty: field_ty.clone(),
                },
                Some(cast_ty.clone()),
            ),
            _ => return None,
        },
        _ => return None,
    };

    let mut trial_locals = locals.clone();
    let mut trial_indexes = local_indexes.clone();
    let mut trial_conditions = local_conditions.clone();
    let temp_name = format!("{}_{}", temp_prefix, trial_indexes.len());
    let base_ty = base.ty();
    let stmts = lower_i64_aggregate_call_to_local_stmts(
        &temp_name,
        &base_ty,
        base.as_ref(),
        &mut trial_locals,
        &mut trial_indexes,
        &mut trial_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let rewritten_base = Box::new(Expr::VarRef {
        name: temp_name,
        ty: base_ty,
    });
    let rewritten_expr = match projection {
        HelperCallProjection::Tuple { index, ty } => Expr::TupleIndex {
            base: rewritten_base,
            index,
            ty,
        },
        HelperCallProjection::Field { field, ty } => Expr::FieldAccess {
            base: rewritten_base,
            field,
            ty,
        },
    };
    let rewritten_expr = if let Some(cast_ty) = cast_ty {
        Expr::Cast {
            expr: Box::new(rewritten_expr),
            ty: cast_ty,
        }
    } else {
        rewritten_expr
    };
    let result = lower_i64_return_value_expr(
        &rewritten_expr,
        &trial_indexes,
        &trial_conditions,
        helper_signatures,
        static_bindings,
    )?;
    *locals = trial_locals;
    Some(CraneliftI64ReturnBlock { stmts, result })
}

fn record_i64_known_map_let(stmt: &Stmt, static_bindings: &mut I64StaticBindings) -> Option<bool> {
    let Stmt::Let {
        name,
        ty: Type::Map(_, _),
        expr: Expr::MapLiteral { entries, .. },
        ..
    } = stmt
    else {
        return None;
    };
    let Some(entries) = i64_static_map_literal_entries(entries, static_bindings) else {
        return Some(false);
    };
    static_bindings.map_literals.insert(name.clone(), entries);
    Some(true)
}

fn i64_static_map_literal_entries(
    entries: &[MapEntry],
    static_bindings: &I64StaticBindings,
) -> Option<Vec<MapEntry>> {
    entries
        .iter()
        .map(|entry| {
            Some(MapEntry {
                key: i64_static_map_key_literal_expr(&entry.key, static_bindings)?,
                value: i64_static_map_value_literal_expr(&entry.value, static_bindings)?,
            })
        })
        .collect()
}

fn i64_static_map_key_literal_expr(
    expr: &Expr,
    static_bindings: &I64StaticBindings,
) -> Option<Expr> {
    Some(match lower_i64_map_key_expr(expr, static_bindings)? {
        I64MapKey::Int(value) => Expr::Literal(LiteralValue::Int(value)),
        I64MapKey::Bool(value) => Expr::Literal(LiteralValue::Bool(value)),
        I64MapKey::Text(value) => Expr::Literal(LiteralValue::String(value)),
    })
}

fn i64_static_map_value_literal_expr(
    expr: &Expr,
    static_bindings: &I64StaticBindings,
) -> Option<Expr> {
    if let Some(value) = i64_static_scalar_value(expr, static_bindings) {
        return Some(Expr::Literal(LiteralValue::Int(value)));
    }
    if let Some(value) = i64_static_bool_value(expr, static_bindings) {
        return Some(Expr::Literal(LiteralValue::Bool(value)));
    }
    i64_string_text(expr, static_bindings).map(|value| Expr::Literal(LiteralValue::String(value)))
}

fn record_i64_known_map_key_array_let(
    stmt: &Stmt,
    static_bindings: &mut I64StaticBindings,
) -> Option<bool> {
    let Stmt::Let {
        name,
        ty: Type::Array(_, None),
        expr,
        ..
    } = stmt
    else {
        return None;
    };
    let Some(keys) = i64_map_keys_expr(expr, static_bindings) else {
        return Some(false);
    };
    static_bindings.map_key_arrays.insert(name.clone(), keys);
    Some(true)
}

fn lower_i64_exit_return(
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<I64ExitBody> {
    if let Some(block) = lower_i64_helper_call_slice_index_return_block(
        expr,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(I64ExitBody::BlockReturn(block));
    }
    if let Some(block) = lower_i64_helper_call_array_index_return_block(
        expr,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(I64ExitBody::BlockReturn(block));
    }
    if let Some(block) = lower_i64_helper_call_projection_return_block(
        expr,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(I64ExitBody::BlockReturn(block));
    }
    if let Some(body) = lower_i64_option_match_exit_return(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(body);
    }
    if let Some(body) = lower_i64_result_match_exit_return(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(body);
    }
    match expr.ty() {
        Type::Bool => Some(I64ExitBody::IfReturn {
            cond: lower_i64_condition(
                expr,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?,
            then_result: CraneliftI64Expr::Literal(1),
            else_result: CraneliftI64Expr::Literal(0),
        }),
        ty if is_i64_compatible_type(&ty) => Some(I64ExitBody::Return(lower_i64_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?)),
        _ => None,
    }
}

fn lower_i64_panic_exit_body(
    message: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<I64ExitBody> {
    let stmts = lower_i64_panic_report_stmts(
        message,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    Some(I64ExitBody::BlockReturn(CraneliftI64ReturnBlock {
        stmts,
        result: CraneliftI64Expr::Literal(1),
    }))
}

fn lower_i64_panic_report_stmts(
    message: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    if let Some(value) = lower_i64_panic_int_stringify_value(
        message,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(vec![
            CraneliftI64Stmt::WriteText {
                stream: OutputStream::Stderr,
                text: String::from("{\"kind\":\"panic\",\"message\":"),
            },
            CraneliftI64Stmt::WriteI64Text {
                stream: OutputStream::Stderr,
                value,
            },
            CraneliftI64Stmt::WriteLine {
                stream: OutputStream::Stderr,
                text: String::from("}"),
            },
        ]);
    }
    if let Some(cond) = lower_i64_panic_bool_stringify_condition(
        message,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(vec![CraneliftI64Stmt::If {
            cond,
            then_body: vec![CraneliftI64Stmt::WriteLine {
                stream: OutputStream::Stderr,
                text: String::from("{\"kind\":\"panic\",\"message\":\"true\"}"),
            }],
            else_body: vec![CraneliftI64Stmt::WriteLine {
                stream: OutputStream::Stderr,
                text: String::from("{\"kind\":\"panic\",\"message\":\"false\"}"),
            }],
        }]);
    }
    if let Some(stmts) = lower_i64_panic_json_stringify_string_report_stmts(
        message,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(stmts);
    }
    if let Some(stmts) = lower_i64_log_event_panic_report_stmts(
        message,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(stmts);
    }
    if let Some(stmts) = lower_i64_dynamic_known_string_panic_report_stmts(
        message,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(stmts);
    }
    let message = i64_string_text(message, static_bindings)?;
    Some(vec![CraneliftI64Stmt::WriteLine {
        stream: OutputStream::Stderr,
        text: format!(
            "{{\"kind\":\"panic\",\"message\":{}}}",
            json_escape_string(&message)
        ),
    }])
}

fn lower_i64_panic_json_stringify_string_report_stmts(
    message: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Expr::Call { name, args, .. } = message else {
        return None;
    };
    if !is_i64_json_stringify_string_name(name, static_bindings) {
        return None;
    }
    let [value] = args.as_slice() else {
        return None;
    };
    let mut stmts = vec![CraneliftI64Stmt::WriteText {
        stream: OutputStream::Stderr,
        text: String::from("{\"kind\":\"panic\",\"message\":"),
    }];
    stmts.extend(lower_i64_log_json_string_value_stmts(
        value,
        OutputStream::Stderr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?);
    stmts.push(CraneliftI64Stmt::WriteLine {
        stream: OutputStream::Stderr,
        text: String::from("}"),
    });
    Some(stmts)
}

fn lower_i64_log_event_panic_report_stmts(
    message: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Expr::Call { name, args, .. } = message else {
        return None;
    };
    if !is_i64_log_event_name(name, static_bindings) {
        return None;
    }
    let [level, message, attributes] = args.as_slice() else {
        return None;
    };
    let mut stmts = vec![
        CraneliftI64Stmt::WriteText {
            stream: OutputStream::Stderr,
            text: String::from("{\"kind\":\"panic\",\"message\":\""),
        },
        CraneliftI64Stmt::WriteText {
            stream: OutputStream::Stderr,
            text: json_escape_string_content("{\"level\":"),
        },
        CraneliftI64Stmt::WriteText {
            stream: OutputStream::Stderr,
            text: json_escape_string_content(&json_escape_string(&i64_string_text(
                level,
                static_bindings,
            )?)),
        },
        CraneliftI64Stmt::WriteText {
            stream: OutputStream::Stderr,
            text: json_escape_string_content(",\"message\":"),
        },
    ];
    stmts.extend(lower_i64_log_json_string_content_stmts(
        message,
        OutputStream::Stderr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?);
    stmts.push(CraneliftI64Stmt::WriteText {
        stream: OutputStream::Stderr,
        text: json_escape_string_content(",\"attributes\":{"),
    });
    stmts.extend(lower_i64_log_fields_content_stmts(
        attributes,
        OutputStream::Stderr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?);
    stmts.push(CraneliftI64Stmt::WriteLine {
        stream: OutputStream::Stderr,
        text: json_escape_string_content("}}") + "\"}",
    });
    Some(stmts)
}

fn lower_i64_log_json_string_content_stmts(
    expr: &Expr,
    stream: OutputStream,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    if let Some(text) = i64_string_text(expr, static_bindings) {
        return Some(vec![CraneliftI64Stmt::WriteText {
            stream,
            text: json_escape_string_content(&json_escape_string(&text)),
        }]);
    }
    if let Some(stmts) = lower_i64_dynamic_known_string_text_stmts(
        expr,
        stream,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
        |value| json_escape_string_content(&json_escape_string(value)),
    ) {
        return Some(stmts);
    }
    if let Expr::VarRef {
        name,
        ty: Type::String | Type::Str,
    } = expr
    {
        if let Some(local) = local_indexes.get(i64_printable_i64_string_key(name).as_str()) {
            return Some(vec![
                CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("\\\""),
                },
                CraneliftI64Stmt::WriteI64Text {
                    stream,
                    value: CraneliftI64Expr::Local(*local),
                },
                CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("\\\""),
                },
            ]);
        }
        if let Some(cond) = local_conditions
            .get(i64_printable_bool_string_key(name).as_str())
            .cloned()
        {
            return Some(vec![CraneliftI64Stmt::If {
                cond,
                then_body: vec![CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("\\\"true\\\""),
                }],
                else_body: vec![CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("\\\"false\\\""),
                }],
            }]);
        }
    }
    if let Expr::Call { name, args, .. } = expr {
        if is_i64_json_stringify_int_name(name, static_bindings) {
            let [value] = args.as_slice() else {
                return None;
            };
            return Some(vec![
                CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("\\\""),
                },
                CraneliftI64Stmt::WriteI64Text {
                    stream,
                    value: lower_i64_expr(
                        value,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )?,
                },
                CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("\\\""),
                },
            ]);
        }
        if is_i64_json_stringify_bool_name(name, static_bindings) {
            let [value] = args.as_slice() else {
                return None;
            };
            return Some(vec![CraneliftI64Stmt::If {
                cond: lower_i64_condition(
                    value,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )?,
                then_body: vec![CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("\\\"true\\\""),
                }],
                else_body: vec![CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("\\\"false\\\""),
                }],
            }]);
        }
    }
    None
}

fn lower_i64_log_fields_content_stmts(
    expr: &Expr,
    stream: OutputStream,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    if let Some(text) = i64_string_text(expr, static_bindings) {
        return Some(vec![CraneliftI64Stmt::WriteText {
            stream,
            text: json_escape_string_content(&text),
        }]);
    }
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if is_i64_log_field_string_name(name, static_bindings) {
        let [key, value] = args.as_slice() else {
            return None;
        };
        let mut stmts = vec![
            CraneliftI64Stmt::WriteText {
                stream,
                text: json_escape_string_content(&json_escape_string(&i64_string_text(
                    key,
                    static_bindings,
                )?)),
            },
            CraneliftI64Stmt::WriteText {
                stream,
                text: String::from(":"),
            },
        ];
        stmts.extend(lower_i64_log_json_string_content_stmts(
            value,
            stream,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?);
        return Some(stmts);
    }
    if is_i64_log_field_int_name(name, static_bindings) {
        let [key, value] = args.as_slice() else {
            return None;
        };
        return Some(vec![
            CraneliftI64Stmt::WriteText {
                stream,
                text: json_escape_string_content(&json_escape_string(&i64_string_text(
                    key,
                    static_bindings,
                )?)),
            },
            CraneliftI64Stmt::WriteText {
                stream,
                text: String::from(":"),
            },
            CraneliftI64Stmt::WriteI64Text {
                stream,
                value: lower_i64_expr(
                    value,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )?,
            },
        ]);
    }
    if is_i64_log_field_bool_name(name, static_bindings) {
        let [key, value] = args.as_slice() else {
            return None;
        };
        return Some(vec![
            CraneliftI64Stmt::WriteText {
                stream,
                text: json_escape_string_content(&json_escape_string(&i64_string_text(
                    key,
                    static_bindings,
                )?)),
            },
            CraneliftI64Stmt::WriteText {
                stream,
                text: String::from(":"),
            },
            CraneliftI64Stmt::If {
                cond: lower_i64_condition(
                    value,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )?,
                then_body: vec![CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("true"),
                }],
                else_body: vec![CraneliftI64Stmt::WriteText {
                    stream,
                    text: String::from("false"),
                }],
            },
        ]);
    }
    if is_i64_log_fields2_name(name, static_bindings)
        || is_i64_log_fields3_name(name, static_bindings)
    {
        let mut iter = args.iter();
        let first = iter.next()?;
        let mut stmts = lower_i64_log_fields_content_stmts(
            first,
            stream,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?;
        for field in iter {
            stmts.push(CraneliftI64Stmt::WriteText {
                stream,
                text: String::from(","),
            });
            stmts.extend(lower_i64_log_fields_content_stmts(
                field,
                stream,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?);
        }
        return Some(stmts);
    }
    None
}

fn lower_i64_dynamic_known_string_panic_report_stmts(
    message: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let (keys, index, transform) = i64_map_key_array_string_index_source(message, static_bindings)?;
    if keys.is_empty() {
        return None;
    }
    if let Some(index) = lower_i64_literal_index(&index) {
        let text = i64_panic_report_text(&i64_map_key_text(keys.get(index)?, transform)?);
        return Some(vec![CraneliftI64Stmt::WriteLine {
            stream: OutputStream::Stderr,
            text,
        }]);
    }
    let index = lower_i64_expr(
        &index,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let last = keys.len() - 1;
    let fallback_text = i64_panic_report_text(&i64_map_key_text(keys.get(last)?, transform)?);
    let mut stmts = vec![CraneliftI64Stmt::WriteLine {
        stream: OutputStream::Stderr,
        text: fallback_text,
    }];
    for candidate in (0..last).rev() {
        let text = i64_panic_report_text(&i64_map_key_text(keys.get(candidate)?, transform)?);
        stmts = vec![CraneliftI64Stmt::If {
            cond: CraneliftI64Condition::Compare(CraneliftI64Compare {
                op: CraneliftI64CompareOp::Eq,
                lhs: index.clone(),
                rhs: CraneliftI64Expr::Literal(candidate as i64),
            }),
            then_body: vec![CraneliftI64Stmt::WriteLine {
                stream: OutputStream::Stderr,
                text,
            }],
            else_body: stmts,
        }];
    }
    Some(stmts)
}

fn i64_panic_report_text(message: &str) -> String {
    format!(
        "{{\"kind\":\"panic\",\"message\":{}}}",
        json_escape_string(message)
    )
}

fn lower_i64_panic_int_stringify_value(
    message: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    match message {
        Expr::VarRef {
            name,
            ty: Type::String | Type::Str,
        } => local_indexes
            .get(i64_printable_i64_string_key(name).as_str())
            .map(|local| CraneliftI64Expr::Local(*local)),
        Expr::Call { name, args, .. } if is_i64_json_stringify_int_name(name, static_bindings) => {
            let [value] = args.as_slice() else {
                return None;
            };
            lower_i64_expr(
                value,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        _ => None,
    }
}

fn lower_i64_panic_bool_stringify_condition(
    message: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Condition> {
    match message {
        Expr::VarRef {
            name,
            ty: Type::String | Type::Str,
        } => local_conditions
            .get(i64_printable_bool_string_key(name).as_str())
            .cloned(),
        Expr::Call { name, args, .. } if is_i64_json_stringify_bool_name(name, static_bindings) => {
            let [value] = args.as_slice() else {
                return None;
            };
            lower_i64_condition(
                value,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        _ => None,
    }
}

fn lower_i64_option_match_exit_return(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<I64ExitBody> {
    let Expr::Match {
        expr: matched,
        arms,
        ty,
    } = expr
    else {
        return None;
    };
    if !is_i64_exit_type(ty) {
        return None;
    }
    if let Some(value) = lower_i64_known_scalar_option_match_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(I64ExitBody::Return(value));
    }
    if let Some(value) = lower_i64_env_option_match_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(I64ExitBody::Return(value));
    }
    if let Some(value) = lower_i64_fs_read_option_match_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(I64ExitBody::Return(value));
    }
    if let Some(value) = lower_i64_net_option_match_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(I64ExitBody::Return(value));
    }
    if let Some(value) = lower_i64_known_string_option_match_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(I64ExitBody::Return(value));
    }
    let (cond, some_indexes, some_conditions, some_arm, none_arm) = lower_i64_option_match_parts(
        matched,
        arms,
        local_indexes,
        local_conditions,
        static_bindings,
    )?;
    Some(I64ExitBody::IfReturn {
        cond,
        then_result: lower_i64_return_value_expr(
            &some_arm.expr,
            &some_indexes,
            &some_conditions,
            helper_signatures,
            static_bindings,
        )?,
        else_result: lower_i64_return_value_expr(
            &none_arm.expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
    })
}

fn lower_i64_option_match_value_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let Expr::Match {
        expr: matched,
        arms,
        ty,
    } = expr
    else {
        return None;
    };
    if !is_i64_exit_type(ty) {
        return None;
    }
    if let Some(value) = lower_i64_known_scalar_option_match_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(value);
    }
    if let Some(value) = lower_i64_env_option_match_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(value);
    }
    if let Some(value) = lower_i64_fs_read_option_match_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(value);
    }
    if let Some(value) = lower_i64_net_option_match_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(value);
    }
    if let Some(value) = lower_i64_known_string_option_match_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(value);
    }
    let (cond, some_indexes, some_conditions, some_arm, none_arm) = lower_i64_option_match_parts(
        matched,
        arms,
        local_indexes,
        local_conditions,
        static_bindings,
    )?;
    Some(CraneliftI64Expr::Select {
        cond: Box::new(cond),
        then_result: Box::new(lower_i64_return_value_expr(
            &some_arm.expr,
            &some_indexes,
            &some_conditions,
            helper_signatures,
            static_bindings,
        )?),
        else_result: Box::new(lower_i64_return_value_expr(
            &none_arm.expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?),
    })
}

fn lower_i64_known_scalar_option_match_value_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let Expr::Match {
        expr: matched,
        arms,
        ty,
    } = expr
    else {
        return None;
    };
    if !is_i64_exit_type(ty) {
        return None;
    }
    let Type::Option(inner) = matched.ty() else {
        return None;
    };
    let (some_arm, none_arm) = i64_option_match_arms(arms)?;
    match inner.as_ref() {
        Type::Bool => match i64_bool_option_value(matched, static_bindings)? {
            Some(value) => {
                let mut arm_static_bindings = static_bindings.clone();
                if let Some(binding) = some_arm.bindings.first()
                    && binding != "_"
                {
                    arm_static_bindings
                        .conditions
                        .insert(binding.clone(), CraneliftI64Condition::Literal(value));
                }
                lower_i64_return_value_expr(
                    &some_arm.expr,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    &arm_static_bindings,
                )
            }
            None => lower_i64_return_value_expr(
                &none_arm.expr,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            ),
        },
        ty if is_i64_compatible_type(ty) => match i64_i64_option_value(matched, static_bindings)? {
            Some(value) => {
                let mut arm_static_bindings = static_bindings.clone();
                if let Some(binding) = some_arm.bindings.first()
                    && binding != "_"
                {
                    arm_static_bindings
                        .values
                        .insert(binding.clone(), CraneliftI64Expr::Literal(value));
                }
                lower_i64_return_value_expr(
                    &some_arm.expr,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    &arm_static_bindings,
                )
            }
            None => lower_i64_return_value_expr(
                &none_arm.expr,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            ),
        },
        _ => None,
    }
}

fn lower_i64_fs_read_option_match_value_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let Expr::Match {
        expr: matched,
        arms,
        ty,
    } = expr
    else {
        return None;
    };
    if !is_i64_exit_type(ty) {
        return None;
    }
    let Type::Option(inner) = matched.ty() else {
        return None;
    };
    if !matches!(inner.as_ref(), Type::String | Type::Str) {
        return None;
    }
    let path = i64_fs_read_path(matched, static_bindings)?;
    let (some_arm, none_arm) = i64_option_match_arms(arms)?;
    let binding = some_arm
        .bindings
        .first()
        .filter(|binding| binding.as_str() != "_");
    let file_len = i64_fs_read_file_len_expr(&path.candidate, path.requested_len, static_bindings)?;
    let then_result = lower_i64_fs_read_some_arm_expr(
        &some_arm.expr,
        binding.map(String::as_str),
        &path.candidate,
        path.requested_len,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let else_result = lower_i64_return_value_expr(
        &none_arm.expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    Some(CraneliftI64Expr::Select {
        cond: Box::new(CraneliftI64Condition::Compare(CraneliftI64Compare {
            op: CraneliftI64CompareOp::Ge,
            lhs: file_len,
            rhs: CraneliftI64Expr::Literal(0),
        })),
        then_result: Box::new(then_result),
        else_result: Box::new(else_result),
    })
}

fn lower_i64_fs_read_some_arm_expr(
    expr: &Expr,
    binding: Option<&str>,
    path: &str,
    path_len: usize,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if let Some(binding) = binding
        && let Expr::Call { name, args, .. } = expr
        && name == "len"
        && let [Expr::VarRef { name, .. }] = args.as_slice()
        && name == binding
    {
        return i64_fs_read_file_len_expr(path, path_len, static_bindings);
    }
    lower_i64_return_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )
}

pub(crate) struct I64NetResolveHost {
    host: String,
    resolved_len: i64,
}

fn lower_i64_known_string_option_match_value_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let Expr::Match { expr, arms, ty } = expr else {
        return None;
    };
    if !is_i64_exit_type(ty) {
        return None;
    }
    let value = i64_string_option_text(expr, static_bindings)?;
    let (some_arm, none_arm) = i64_option_match_arms(arms)?;
    match value {
        Some(value) => {
            let mut arm_static_bindings = static_bindings.clone();
            if let Some(binding) = some_arm.bindings.first()
                && binding != "_"
            {
                arm_static_bindings.strings.insert(binding.clone(), value);
            }
            lower_i64_return_value_expr(
                &some_arm.expr,
                local_indexes,
                local_conditions,
                helper_signatures,
                &arm_static_bindings,
            )
        }
        None => lower_i64_return_value_expr(
            &none_arm.expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        ),
    }
}

fn lower_i64_option_match_parts<'a>(
    matched: &Expr,
    arms: &'a [MatchExprArm],
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    static_bindings: &I64StaticBindings,
) -> Option<(
    CraneliftI64Condition,
    HashMap<String, usize>,
    HashMap<String, CraneliftI64Condition>,
    &'a MatchExprArm,
    &'a MatchExprArm,
)> {
    let Expr::VarRef {
        name,
        ty: Type::Option(inner),
    } = matched
    else {
        return None;
    };
    let string_len_option = i64_string_len_option_local(name, inner.as_ref(), local_indexes);
    if !is_i64_option_local_payload_type_static(inner, static_bindings)
        && string_len_option.is_none()
    {
        return None;
    }
    let tag = *local_indexes.get(i64_option_tag_key(name).as_str())?;
    let payloads = if let Some(payloads) = string_len_option {
        payloads
    } else {
        i64_option_payload_locals(name, inner.as_ref(), local_indexes, static_bindings)?
    };
    let (some_arm, none_arm) = i64_option_match_arms(arms)?;
    let mut some_indexes = local_indexes.clone();
    let mut some_conditions = local_conditions.clone();
    insert_i64_option_payload_binding(
        some_arm.bindings.first(),
        inner.as_ref(),
        &payloads,
        static_bindings,
        &mut some_indexes,
        &mut some_conditions,
    );
    Some((
        CraneliftI64Condition::Compare(CraneliftI64Compare {
            op: CraneliftI64CompareOp::Ne,
            lhs: CraneliftI64Expr::Local(tag),
            rhs: CraneliftI64Expr::Literal(0),
        }),
        some_indexes,
        some_conditions,
        some_arm,
        none_arm,
    ))
}

fn lower_i64_option_stmt_match_parts<'a>(
    matched: &Expr,
    arms: &'a [MatchArm],
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    static_bindings: &I64StaticBindings,
) -> Option<(
    CraneliftI64Condition,
    HashMap<String, usize>,
    HashMap<String, CraneliftI64Condition>,
    &'a MatchArm,
    &'a MatchArm,
)> {
    let Expr::VarRef {
        name,
        ty: Type::Option(inner),
    } = matched
    else {
        return None;
    };
    let string_len_option = i64_string_len_option_local(name, inner.as_ref(), local_indexes);
    if !is_i64_option_local_payload_type_static(inner, static_bindings)
        && string_len_option.is_none()
    {
        return None;
    }
    let tag = *local_indexes.get(i64_option_tag_key(name).as_str())?;
    let payloads = if let Some(payloads) = string_len_option {
        payloads
    } else {
        i64_option_payload_locals(name, inner.as_ref(), local_indexes, static_bindings)?
    };
    let (some_arm, none_arm) = i64_option_stmt_match_arms(arms)?;
    let mut some_indexes = local_indexes.clone();
    let mut some_conditions = local_conditions.clone();
    insert_i64_option_payload_binding(
        (!some_arm.ignore_payloads)
            .then(|| some_arm.bindings.first())
            .flatten(),
        inner.as_ref(),
        &payloads,
        static_bindings,
        &mut some_indexes,
        &mut some_conditions,
    );
    Some((
        CraneliftI64Condition::Compare(CraneliftI64Compare {
            op: CraneliftI64CompareOp::Ne,
            lhs: CraneliftI64Expr::Local(tag),
            rhs: CraneliftI64Expr::Literal(0),
        }),
        some_indexes,
        some_conditions,
        some_arm,
        none_arm,
    ))
}

fn insert_i64_option_payload_binding(
    binding: Option<&String>,
    payload_ty: &Type,
    payloads: &[usize],
    static_bindings: &I64StaticBindings,
    indexes: &mut HashMap<String, usize>,
    conditions: &mut HashMap<String, CraneliftI64Condition>,
) {
    let Some(binding) = binding else {
        return;
    };
    if binding == "_" {
        return;
    }
    match payload_ty {
        Type::Array(element, Some(size)) if is_i64_array_param_element_type(element) => {
            for index in 0..*size {
                let Some(payload) = payloads.get(index).copied() else {
                    return;
                };
                let key = i64_array_projection_key(binding, index);
                indexes.insert(key.clone(), payload);
                if matches!(element.as_ref(), Type::Bool) {
                    conditions.insert(
                        key,
                        CraneliftI64Condition::Compare(CraneliftI64Compare {
                            op: CraneliftI64CompareOp::Ne,
                            lhs: CraneliftI64Expr::Local(payload),
                            rhs: CraneliftI64Expr::Literal(0),
                        }),
                    );
                }
            }
        }
        Type::Tuple(elements) if is_i64_tuple_param_type(elements) => {
            for (index, element) in elements.iter().enumerate() {
                let Some(payload) = payloads.get(index).copied() else {
                    return;
                };
                let key = i64_tuple_projection_key(binding, index);
                indexes.insert(key.clone(), payload);
                if matches!(element, Type::Bool) {
                    conditions.insert(
                        key,
                        CraneliftI64Condition::Compare(CraneliftI64Compare {
                            op: CraneliftI64CompareOp::Ne,
                            lhs: CraneliftI64Expr::Local(payload),
                            rhs: CraneliftI64Expr::Literal(0),
                        }),
                    );
                }
            }
        }
        Type::Struct(name) => {
            let Some(struct_def) = i64_scalar_static_struct_def(name, static_bindings) else {
                return;
            };
            for (index, field) in struct_def.fields.iter().enumerate() {
                let Some(payload) = payloads.get(index).copied() else {
                    return;
                };
                let key = i64_struct_projection_key(binding, &field.name);
                indexes.insert(key.clone(), payload);
                if matches!(field.ty, Type::Bool) {
                    conditions.insert(
                        key,
                        CraneliftI64Condition::Compare(CraneliftI64Compare {
                            op: CraneliftI64CompareOp::Ne,
                            lhs: CraneliftI64Expr::Local(payload),
                            rhs: CraneliftI64Expr::Literal(0),
                        }),
                    );
                }
            }
        }
        Type::Option(inner) if is_i64_option_local_payload_type_static(inner, static_bindings) => {
            let Some(tag) = payloads.first().copied() else {
                return;
            };
            indexes.insert(i64_option_tag_key(binding), tag);
            for index in 0..i64_option_payload_slot_count_static(inner.as_ref(), static_bindings)
                .unwrap_or(0)
            {
                let Some(payload) = payloads.get(1 + index).copied() else {
                    return;
                };
                let key = i64_option_payload_slot_key(binding, index);
                indexes.insert(key, payload);
            }
        }
        Type::Result(ok, err)
            if is_i64_result_local_payload_type_static(ok, err, static_bindings) =>
        {
            let Some(tag) = payloads.first().copied() else {
                return;
            };
            indexes.insert(i64_result_tag_key(binding), tag);
            let slot_count = i64_result_payload_slot_count_static(ok.as_ref(), static_bindings)
                .and_then(|ok_slots| {
                    i64_result_payload_slot_count_static(err.as_ref(), static_bindings)
                        .map(|err_slots| ok_slots.max(err_slots))
                })
                .unwrap_or(0);
            for index in 0..slot_count {
                let Some(payload) = payloads.get(1 + index).copied() else {
                    return;
                };
                indexes.insert(i64_result_payload_slot_key(binding, index), payload);
            }
        }
        Type::Bool => {
            let Some(payload) = payloads.first().copied() else {
                return;
            };
            indexes.insert(binding.clone(), payload);
            conditions.insert(
                binding.clone(),
                CraneliftI64Condition::Compare(CraneliftI64Compare {
                    op: CraneliftI64CompareOp::Ne,
                    lhs: CraneliftI64Expr::Local(payload),
                    rhs: CraneliftI64Expr::Literal(0),
                }),
            );
        }
        ty if is_i64_compatible_type(ty) => {
            let Some(payload) = payloads.first().copied() else {
                return;
            };
            indexes.insert(binding.clone(), payload);
        }
        Type::String | Type::Str => {
            let Some(payload) = payloads.first().copied() else {
                return;
            };
            indexes.insert(i64_string_len_key(binding), payload);
        }
        _ => {}
    }
}

fn i64_option_match_arms(arms: &[MatchExprArm]) -> Option<(&MatchExprArm, &MatchExprArm)> {
    let some_arm = arms.iter().find(|arm| {
        arm.enum_name == "Option"
            && arm.variant == "Some"
            && !arm.is_named
            && arm.bindings.len() == 1
    })?;
    let none_arm = arms.iter().find(|arm| {
        arm.enum_name == "Option"
            && arm.variant == "None"
            && !arm.is_named
            && arm.bindings.is_empty()
    })?;
    Some((some_arm, none_arm))
}

fn i64_option_stmt_match_arms(arms: &[MatchArm]) -> Option<(&MatchArm, &MatchArm)> {
    let some_arm = arms.iter().find(|arm| {
        arm.enum_name == "Option"
            && arm.variant == "Some"
            && !arm.is_named
            && (arm.ignore_payloads || arm.bindings.len() == 1)
    })?;
    let none_arm = arms.iter().find(|arm| {
        arm.enum_name == "Option"
            && arm.variant == "None"
            && !arm.is_named
            && arm.bindings.is_empty()
    })?;
    Some((some_arm, none_arm))
}

fn lower_i64_result_match_exit_return(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<I64ExitBody> {
    let Expr::Match {
        expr: matched,
        arms,
        ty,
    } = expr
    else {
        return None;
    };
    if !is_i64_exit_type(ty) {
        return None;
    }
    let (cond, ok_indexes, ok_conditions, err_indexes, err_conditions, ok_arm, err_arm) =
        lower_i64_result_match_parts(
            matched,
            arms,
            local_indexes,
            local_conditions,
            static_bindings,
        )?;
    Some(I64ExitBody::IfReturn {
        cond,
        then_result: lower_i64_return_value_expr(
            &ok_arm.expr,
            &ok_indexes,
            &ok_conditions,
            helper_signatures,
            static_bindings,
        )?,
        else_result: lower_i64_return_value_expr(
            &err_arm.expr,
            &err_indexes,
            &err_conditions,
            helper_signatures,
            static_bindings,
        )?,
    })
}

fn lower_i64_result_match_value_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let Expr::Match {
        expr: matched,
        arms,
        ty,
    } = expr
    else {
        return None;
    };
    if !is_i64_exit_type(ty) {
        return None;
    }
    let (cond, ok_indexes, ok_conditions, err_indexes, err_conditions, ok_arm, err_arm) =
        lower_i64_result_match_parts(
            matched,
            arms,
            local_indexes,
            local_conditions,
            static_bindings,
        )?;
    Some(CraneliftI64Expr::Select {
        cond: Box::new(cond),
        then_result: Box::new(lower_i64_return_value_expr(
            &ok_arm.expr,
            &ok_indexes,
            &ok_conditions,
            helper_signatures,
            static_bindings,
        )?),
        else_result: Box::new(lower_i64_return_value_expr(
            &err_arm.expr,
            &err_indexes,
            &err_conditions,
            helper_signatures,
            static_bindings,
        )?),
    })
}

fn lower_i64_enum_match_value_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let Expr::Match {
        expr: matched,
        arms,
        ty,
    } = expr
    else {
        return None;
    };
    if !is_i64_exit_type(ty) {
        return None;
    }
    let Expr::VarRef {
        name,
        ty: Type::Enum(enum_name),
    } = matched.as_ref()
    else {
        return None;
    };
    let enum_def = i64_scalar_enum_def(enum_name, static_bindings)?;
    let tag = *local_indexes.get(i64_enum_tag_key(name).as_str())?;
    let payloads = i64_enum_payload_locals(name, enum_name, static_bindings, local_indexes)?;
    let mut lowered = None;
    for arm in arms.iter().rev() {
        if arm.enum_name != enum_def.name {
            return None;
        }
        let variant = enum_def
            .variants
            .iter()
            .position(|variant| variant.name == arm.variant)?;
        let variant_def = enum_def.variants.get(variant)?;
        if arm.bindings.len() != variant_def.payload_tys.len() {
            return None;
        }
        let mut arm_indexes = local_indexes.clone();
        let mut arm_conditions = local_conditions.clone();
        let binding_names =
            i64_enum_match_expr_binding_names(arm.is_named, arm.bindings.as_slice(), variant_def)?;
        insert_i64_enum_payload_bindings(
            Some(binding_names.as_slice()),
            arm.is_named,
            &variant_def.payload_tys,
            &variant_def.payload_names,
            &payloads,
            static_bindings,
            &mut arm_indexes,
            &mut arm_conditions,
        );
        let result = lower_i64_return_value_expr(
            &arm.expr,
            &arm_indexes,
            &arm_conditions,
            helper_signatures,
            static_bindings,
        )?;
        lowered = Some(match lowered {
            None => result,
            Some(else_result) => CraneliftI64Expr::Select {
                cond: Box::new(CraneliftI64Condition::Compare(CraneliftI64Compare {
                    op: CraneliftI64CompareOp::Eq,
                    lhs: CraneliftI64Expr::Local(tag),
                    rhs: CraneliftI64Expr::Literal(variant as i64),
                })),
                then_result: Box::new(result),
                else_result: Box::new(else_result),
            },
        });
    }
    lowered
}

fn insert_i64_enum_payload_binding(
    binding: Option<&String>,
    payload_ty: &Type,
    payloads: &[usize],
    static_bindings: &I64StaticBindings,
    indexes: &mut HashMap<String, usize>,
    conditions: &mut HashMap<String, CraneliftI64Condition>,
) {
    let Some(binding) = binding else {
        return;
    };
    if binding == "_" {
        return;
    }
    match payload_ty {
        Type::Array(element, Some(size)) if is_i64_array_param_element_type(element) => {
            for index in 0..*size {
                let Some(payload) = payloads.get(index).copied() else {
                    return;
                };
                let key = i64_array_projection_key(binding, index);
                indexes.insert(key.clone(), payload);
                if matches!(element.as_ref(), Type::Bool) {
                    conditions.insert(
                        key,
                        CraneliftI64Condition::Compare(CraneliftI64Compare {
                            op: CraneliftI64CompareOp::Ne,
                            lhs: CraneliftI64Expr::Local(payload),
                            rhs: CraneliftI64Expr::Literal(0),
                        }),
                    );
                }
            }
        }
        Type::Tuple(elements) if is_i64_tuple_param_type(elements) => {
            for (index, element) in elements.iter().enumerate() {
                let Some(payload) = payloads.get(index).copied() else {
                    return;
                };
                let key = i64_tuple_projection_key(binding, index);
                indexes.insert(key.clone(), payload);
                if matches!(element, Type::Bool) {
                    conditions.insert(
                        key,
                        CraneliftI64Condition::Compare(CraneliftI64Compare {
                            op: CraneliftI64CompareOp::Ne,
                            lhs: CraneliftI64Expr::Local(payload),
                            rhs: CraneliftI64Expr::Literal(0),
                        }),
                    );
                }
            }
        }
        Type::Struct(name) => {
            let Some(struct_def) = i64_scalar_static_struct_def(name, static_bindings) else {
                return;
            };
            for (index, field) in struct_def.fields.iter().enumerate() {
                let Some(payload) = payloads.get(index).copied() else {
                    return;
                };
                let key = i64_struct_projection_key(binding, &field.name);
                indexes.insert(key.clone(), payload);
                if matches!(field.ty, Type::Bool) {
                    conditions.insert(
                        key,
                        CraneliftI64Condition::Compare(CraneliftI64Compare {
                            op: CraneliftI64CompareOp::Ne,
                            lhs: CraneliftI64Expr::Local(payload),
                            rhs: CraneliftI64Expr::Literal(0),
                        }),
                    );
                }
            }
        }
        Type::Option(inner) if is_i64_option_local_payload_type_static(inner, static_bindings) => {
            let Some(tag) = payloads.first().copied() else {
                return;
            };
            indexes.insert(i64_option_tag_key(binding), tag);
            for index in 0..i64_option_payload_slot_count_static(inner.as_ref(), static_bindings)
                .unwrap_or(0)
            {
                let Some(payload) = payloads.get(1 + index).copied() else {
                    return;
                };
                indexes.insert(i64_option_payload_slot_key(binding, index), payload);
            }
        }
        Type::Result(ok, err)
            if is_i64_result_local_payload_type_static(ok, err, static_bindings) =>
        {
            let Some(tag) = payloads.first().copied() else {
                return;
            };
            indexes.insert(i64_result_tag_key(binding), tag);
            let slot_count = i64_result_payload_slot_count_static(ok.as_ref(), static_bindings)
                .and_then(|ok_slots| {
                    i64_result_payload_slot_count_static(err.as_ref(), static_bindings)
                        .map(|err_slots| ok_slots.max(err_slots))
                })
                .unwrap_or(0);
            for index in 0..slot_count {
                let Some(payload) = payloads.get(1 + index).copied() else {
                    return;
                };
                indexes.insert(i64_result_payload_slot_key(binding, index), payload);
            }
        }
        Type::Bool => {
            let Some(payload) = payloads.first().copied() else {
                return;
            };
            indexes.insert(binding.clone(), payload);
            conditions.insert(
                binding.clone(),
                CraneliftI64Condition::Compare(CraneliftI64Compare {
                    op: CraneliftI64CompareOp::Ne,
                    lhs: CraneliftI64Expr::Local(payload),
                    rhs: CraneliftI64Expr::Literal(0),
                }),
            );
        }
        ty if is_i64_compatible_type(ty) => {
            let Some(payload) = payloads.first().copied() else {
                return;
            };
            indexes.insert(binding.clone(), payload);
        }
        _ => {}
    }
}

fn insert_i64_enum_payload_bindings(
    bindings: Option<&[String]>,
    is_named: bool,
    payload_tys: &[Type],
    payload_names: &[String],
    payloads: &[usize],
    static_bindings: &I64StaticBindings,
    indexes: &mut HashMap<String, usize>,
    conditions: &mut HashMap<String, CraneliftI64Condition>,
) {
    let Some(bindings) = bindings else {
        return;
    };
    for (index, binding) in bindings.iter().enumerate() {
        let payload_index = if is_named {
            let Some(payload_index) = payload_names.iter().position(|name| name == binding) else {
                return;
            };
            payload_index
        } else {
            index
        };
        let Some(payload_ty) = payload_tys.get(payload_index) else {
            return;
        };
        let slot_offset: usize = payload_tys
            .iter()
            .take(payload_index)
            .map(|ty| i64_enum_payload_variant_slot_count(ty, static_bindings))
            .try_fold(0usize, |total, count| Some(total + count?))
            .unwrap_or(0);
        let Some(slot_count) = i64_enum_payload_variant_slot_count(payload_ty, static_bindings)
        else {
            return;
        };
        let Some(payload_slots) = payloads.get(slot_offset..slot_offset + slot_count) else {
            return;
        };
        insert_i64_enum_payload_binding(
            Some(binding),
            payload_ty,
            payload_slots,
            static_bindings,
            indexes,
            conditions,
        );
    }
}

fn i64_enum_match_binding_names(
    is_named: bool,
    ignore_payloads: bool,
    bindings: &[String],
    variant_def: &EnumVariantDef,
) -> Option<Option<Vec<String>>> {
    if ignore_payloads {
        return Some(None);
    }
    i64_enum_match_expr_binding_names(is_named, bindings, variant_def).map(Some)
}

fn i64_enum_match_expr_binding_names(
    is_named: bool,
    bindings: &[String],
    variant_def: &EnumVariantDef,
) -> Option<Vec<String>> {
    if is_named {
        if variant_def.payload_names.is_empty() || bindings.len() != variant_def.payload_names.len()
        {
            return None;
        }
        for binding in bindings {
            if !variant_def.payload_names.iter().any(|name| name == binding) {
                return None;
            }
        }
    } else {
        if !variant_def.payload_names.is_empty() || bindings.len() != variant_def.payload_tys.len()
        {
            return None;
        }
    }
    Some(bindings.to_vec())
}

fn lower_i64_result_match_parts<'a>(
    matched: &Expr,
    arms: &'a [MatchExprArm],
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    static_bindings: &I64StaticBindings,
) -> Option<(
    CraneliftI64Condition,
    HashMap<String, usize>,
    HashMap<String, CraneliftI64Condition>,
    HashMap<String, usize>,
    HashMap<String, CraneliftI64Condition>,
    &'a MatchExprArm,
    &'a MatchExprArm,
)> {
    let Expr::VarRef {
        name,
        ty: Type::Result(ok, err),
    } = matched
    else {
        return None;
    };
    if !is_i64_result_local_payload_type_static(ok, err, static_bindings) {
        return None;
    }
    let tag = *local_indexes.get(i64_result_tag_key(name).as_str())?;
    let payloads = i64_result_payload_locals(
        name,
        ok.as_ref(),
        err.as_ref(),
        local_indexes,
        static_bindings,
    )?;
    let (ok_arm, err_arm) = i64_result_match_arms(arms)?;
    let mut ok_indexes = local_indexes.clone();
    let mut ok_conditions = local_conditions.clone();
    insert_i64_result_payload_binding(
        ok_arm.bindings.first(),
        ok.as_ref(),
        &payloads,
        static_bindings,
        &mut ok_indexes,
        &mut ok_conditions,
    );
    let mut err_indexes = local_indexes.clone();
    let mut err_conditions = local_conditions.clone();
    insert_i64_result_payload_binding(
        err_arm.bindings.first(),
        err.as_ref(),
        &payloads,
        static_bindings,
        &mut err_indexes,
        &mut err_conditions,
    );
    Some((
        CraneliftI64Condition::Compare(CraneliftI64Compare {
            op: CraneliftI64CompareOp::Ne,
            lhs: CraneliftI64Expr::Local(tag),
            rhs: CraneliftI64Expr::Literal(0),
        }),
        ok_indexes,
        ok_conditions,
        err_indexes,
        err_conditions,
        ok_arm,
        err_arm,
    ))
}

fn lower_i64_result_stmt_match_parts<'a>(
    matched: &Expr,
    arms: &'a [MatchArm],
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    static_bindings: &I64StaticBindings,
) -> Option<(
    CraneliftI64Condition,
    HashMap<String, usize>,
    HashMap<String, CraneliftI64Condition>,
    HashMap<String, usize>,
    HashMap<String, CraneliftI64Condition>,
    &'a MatchArm,
    &'a MatchArm,
)> {
    let Expr::VarRef {
        name,
        ty: Type::Result(ok, err),
    } = matched
    else {
        return None;
    };
    if !is_i64_result_local_payload_type_static(ok, err, static_bindings) {
        return None;
    }
    let tag = *local_indexes.get(i64_result_tag_key(name).as_str())?;
    let payloads = i64_result_payload_locals(
        name,
        ok.as_ref(),
        err.as_ref(),
        local_indexes,
        static_bindings,
    )?;
    let (ok_arm, err_arm) = i64_result_stmt_match_arms(arms)?;
    let mut ok_indexes = local_indexes.clone();
    let mut ok_conditions = local_conditions.clone();
    insert_i64_result_payload_binding(
        (!ok_arm.ignore_payloads)
            .then(|| ok_arm.bindings.first())
            .flatten(),
        ok.as_ref(),
        &payloads,
        static_bindings,
        &mut ok_indexes,
        &mut ok_conditions,
    );
    let mut err_indexes = local_indexes.clone();
    let mut err_conditions = local_conditions.clone();
    insert_i64_result_payload_binding(
        (!err_arm.ignore_payloads)
            .then(|| err_arm.bindings.first())
            .flatten(),
        err.as_ref(),
        &payloads,
        static_bindings,
        &mut err_indexes,
        &mut err_conditions,
    );
    Some((
        CraneliftI64Condition::Compare(CraneliftI64Compare {
            op: CraneliftI64CompareOp::Ne,
            lhs: CraneliftI64Expr::Local(tag),
            rhs: CraneliftI64Expr::Literal(0),
        }),
        ok_indexes,
        ok_conditions,
        err_indexes,
        err_conditions,
        ok_arm,
        err_arm,
    ))
}

fn insert_i64_result_payload_binding(
    binding: Option<&String>,
    payload_ty: &Type,
    payloads: &[usize],
    static_bindings: &I64StaticBindings,
    indexes: &mut HashMap<String, usize>,
    conditions: &mut HashMap<String, CraneliftI64Condition>,
) {
    let Some(binding) = binding else {
        return;
    };
    if binding == "_" {
        return;
    }
    match payload_ty {
        Type::Array(element, Some(size)) if is_i64_array_param_element_type(element) => {
            for index in 0..*size {
                let Some(payload) = payloads.get(index).copied() else {
                    return;
                };
                let key = i64_array_projection_key(binding, index);
                indexes.insert(key.clone(), payload);
                if matches!(element.as_ref(), Type::Bool) {
                    conditions.insert(
                        key,
                        CraneliftI64Condition::Compare(CraneliftI64Compare {
                            op: CraneliftI64CompareOp::Ne,
                            lhs: CraneliftI64Expr::Local(payload),
                            rhs: CraneliftI64Expr::Literal(0),
                        }),
                    );
                }
            }
        }
        Type::Tuple(elements) if is_i64_tuple_param_type(elements) => {
            for (index, element) in elements.iter().enumerate() {
                let Some(payload) = payloads.get(index).copied() else {
                    return;
                };
                let key = i64_tuple_projection_key(binding, index);
                indexes.insert(key.clone(), payload);
                if matches!(element, Type::Bool) {
                    conditions.insert(
                        key,
                        CraneliftI64Condition::Compare(CraneliftI64Compare {
                            op: CraneliftI64CompareOp::Ne,
                            lhs: CraneliftI64Expr::Local(payload),
                            rhs: CraneliftI64Expr::Literal(0),
                        }),
                    );
                }
            }
        }
        Type::Struct(name) => {
            let Some(struct_def) = i64_scalar_static_struct_def(name, static_bindings) else {
                return;
            };
            for (index, field) in struct_def.fields.iter().enumerate() {
                let Some(payload) = payloads.get(index).copied() else {
                    return;
                };
                let key = i64_struct_projection_key(binding, &field.name);
                indexes.insert(key.clone(), payload);
                if matches!(field.ty, Type::Bool) {
                    conditions.insert(
                        key,
                        CraneliftI64Condition::Compare(CraneliftI64Compare {
                            op: CraneliftI64CompareOp::Ne,
                            lhs: CraneliftI64Expr::Local(payload),
                            rhs: CraneliftI64Expr::Literal(0),
                        }),
                    );
                }
            }
        }
        Type::Option(inner) if is_i64_option_local_payload_type_static(inner, static_bindings) => {
            let Some(tag) = payloads.first().copied() else {
                return;
            };
            indexes.insert(i64_option_tag_key(binding), tag);
            for index in 0..i64_option_payload_slot_count_static(inner.as_ref(), static_bindings)
                .unwrap_or(0)
            {
                let Some(payload) = payloads.get(1 + index).copied() else {
                    return;
                };
                indexes.insert(i64_option_payload_slot_key(binding, index), payload);
            }
        }
        Type::Result(ok, err)
            if is_i64_result_local_payload_type_static(ok, err, static_bindings) =>
        {
            let Some(tag) = payloads.first().copied() else {
                return;
            };
            indexes.insert(i64_result_tag_key(binding), tag);
            let slot_count = i64_result_payload_slot_count_static(ok.as_ref(), static_bindings)
                .and_then(|ok_slots| {
                    i64_result_payload_slot_count_static(err.as_ref(), static_bindings)
                        .map(|err_slots| ok_slots.max(err_slots))
                })
                .unwrap_or(0);
            for index in 0..slot_count {
                let Some(payload) = payloads.get(1 + index).copied() else {
                    return;
                };
                indexes.insert(i64_result_payload_slot_key(binding, index), payload);
            }
        }
        Type::Bool => {
            let Some(payload) = payloads.first().copied() else {
                return;
            };
            indexes.insert(binding.clone(), payload);
            conditions.insert(
                binding.clone(),
                CraneliftI64Condition::Compare(CraneliftI64Compare {
                    op: CraneliftI64CompareOp::Ne,
                    lhs: CraneliftI64Expr::Local(payload),
                    rhs: CraneliftI64Expr::Literal(0),
                }),
            );
        }
        ty if is_i64_compatible_type(ty) => {
            let Some(payload) = payloads.first().copied() else {
                return;
            };
            indexes.insert(binding.clone(), payload);
        }
        _ => {}
    }
}

fn i64_result_match_arms(arms: &[MatchExprArm]) -> Option<(&MatchExprArm, &MatchExprArm)> {
    let ok_arm = arms.iter().find(|arm| {
        arm.enum_name == "Result" && arm.variant == "Ok" && !arm.is_named && arm.bindings.len() == 1
    })?;
    let err_arm = arms.iter().find(|arm| {
        arm.enum_name == "Result"
            && arm.variant == "Err"
            && !arm.is_named
            && arm.bindings.len() == 1
    })?;
    Some((ok_arm, err_arm))
}

fn i64_result_stmt_match_arms(arms: &[MatchArm]) -> Option<(&MatchArm, &MatchArm)> {
    let ok_arm = arms.iter().find(|arm| {
        arm.enum_name == "Result"
            && arm.variant == "Ok"
            && !arm.is_named
            && (arm.ignore_payloads || arm.bindings.len() == 1)
    })?;
    let err_arm = arms.iter().find(|arm| {
        arm.enum_name == "Result"
            && arm.variant == "Err"
            && !arm.is_named
            && (arm.ignore_payloads || arm.bindings.len() == 1)
    })?;
    Some((ok_arm, err_arm))
}

fn lower_i64_return_value_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if let Some(expr) = lower_i64_option_match_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(expr);
    }
    if let Some(expr) = lower_i64_result_match_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(expr);
    }
    if let Some(expr) = lower_i64_enum_match_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(expr);
    }
    match expr.ty() {
        Type::Bool => lower_i64_bool_value_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        ),
        ty if is_i64_compatible_type(&ty) => lower_i64_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        ),
        _ => None,
    }
}

fn lower_i64_bool_value_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if let Some(expr) = lower_i64_option_match_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(expr);
    }
    if let Some(expr) = lower_i64_result_match_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(expr);
    }
    Some(CraneliftI64Expr::ConditionValue(Box::new(
        lower_i64_condition(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
    )))
}

fn lower_i64_condition(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Condition> {
    match expr {
        Expr::Literal(LiteralValue::Bool(value)) => Some(CraneliftI64Condition::Literal(*value)),
        Expr::VarRef {
            name,
            ty: Type::Bool,
        } => local_conditions
            .get(name.as_str())
            .cloned()
            .or_else(|| static_bindings.conditions.get(name).cloned()),
        Expr::BinaryCompare { ty: Type::Bool, .. } => {
            if let Some(condition) = lower_i64_bool_literal_compare(
                expr,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            ) {
                Some(condition)
            } else if let Some(condition) = lower_i64_bool_value_compare(
                expr,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            ) {
                Some(condition)
            } else if let Some(condition) = lower_i64_map_key_array_string_index_compare(
                expr,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            ) {
                Some(condition)
            } else if let Some(condition) = lower_i64_string_literal_compare(expr, static_bindings)
            {
                Some(condition)
            } else {
                Some(CraneliftI64Condition::Compare(lower_i64_compare(
                    expr,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )?))
            }
        }
        Expr::Call {
            name,
            args,
            ty: Type::Bool,
        } => {
            if let Some(condition) = lower_i64_map_contains_key_condition(
                name,
                args,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            ) {
                return Some(condition);
            }
            if let Some(condition) = lower_i64_known_bool_intrinsic_condition(
                name,
                args,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            ) {
                return Some(condition);
            }
            if let Some(condition) = i64_known_helper_call_condition(name, args, static_bindings) {
                return Some(condition);
            }
            let call = lower_i64_fixed_array_bool_intrinsic_expr(
                name,
                args,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
            .or_else(|| {
                lower_i64_call_expr(
                    name,
                    args,
                    true,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )
            })?;
            Some(CraneliftI64Condition::Compare(CraneliftI64Compare {
                op: CraneliftI64CompareOp::Ne,
                lhs: call,
                rhs: CraneliftI64Expr::Literal(0),
            }))
        }
        Expr::TupleIndex {
            base,
            index,
            ty: Type::Bool,
        } => {
            if let Expr::VarRef { name, .. } = base.as_ref() {
                return local_conditions
                    .get(i64_tuple_projection_key(name, *index).as_str())
                    .cloned();
            }
            let Expr::TupleLiteral { elements, .. } = base.as_ref() else {
                return None;
            };
            lower_i64_condition(
                elements.get(*index)?,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        Expr::Index {
            base,
            index,
            ty: Type::Bool,
        } => {
            if let Some(value) = lower_i64_slice_projection_index_expr(
                base,
                index,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            ) {
                return Some(CraneliftI64Condition::Compare(CraneliftI64Compare {
                    op: CraneliftI64CompareOp::Ne,
                    lhs: value,
                    rhs: CraneliftI64Expr::Literal(0),
                }));
            }
            if let Expr::VarRef {
                name,
                ty: Type::Array(_, Some(size)),
            } = base.as_ref()
            {
                if let Some(index) = lower_i64_literal_index(index) {
                    return local_conditions
                        .get(i64_array_projection_key(name, index).as_str())
                        .cloned();
                }
                let value = lower_i64_array_projection_index_expr(
                    name,
                    *size,
                    index,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )?;
                return Some(CraneliftI64Condition::Compare(CraneliftI64Compare {
                    op: CraneliftI64CompareOp::Ne,
                    lhs: value,
                    rhs: CraneliftI64Expr::Literal(0),
                }));
            }
            if let Some(value) = lower_i64_array_literal_projection_index_expr(
                base,
                index,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            ) {
                return Some(CraneliftI64Condition::Compare(CraneliftI64Compare {
                    op: CraneliftI64CompareOp::Ne,
                    lhs: value,
                    rhs: CraneliftI64Expr::Literal(0),
                }));
            }
            let element = lower_i64_array_literal_element(base, index)?;
            lower_i64_condition(
                element,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        Expr::FieldAccess {
            base,
            field,
            ty: Type::Bool,
        } => {
            if let Expr::VarRef { name, .. } = base.as_ref() {
                return local_conditions
                    .get(i64_struct_projection_key(name, field).as_str())
                    .cloned();
            }
            let element = lower_i64_struct_literal_field(base, field)?;
            lower_i64_condition(
                element,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        Expr::BinaryLogic {
            op,
            lhs,
            rhs,
            ty: Type::Bool,
        } => {
            let lhs = Box::new(lower_i64_condition(
                lhs,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?);
            let rhs = Box::new(lower_i64_condition(
                rhs,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?);
            match op {
                LogicOp::And => Some(CraneliftI64Condition::And { lhs, rhs }),
                LogicOp::Or => Some(CraneliftI64Condition::Or { lhs, rhs }),
            }
        }
        _ => None,
    }
}

fn lower_i64_bool_value_compare(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Condition> {
    let Expr::BinaryCompare {
        op,
        lhs,
        rhs,
        ty: Type::Bool,
    } = expr
    else {
        return None;
    };
    let op = match op {
        CompareOp::Eq => CraneliftI64CompareOp::Eq,
        CompareOp::Ne => CraneliftI64CompareOp::Ne,
        CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge => return None,
    };
    if !matches!(lhs.ty(), Type::Bool) || !matches!(rhs.ty(), Type::Bool) {
        return None;
    }
    Some(CraneliftI64Condition::Compare(CraneliftI64Compare {
        op,
        lhs: lower_i64_bool_value_expr(
            lhs,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
        rhs: lower_i64_bool_value_expr(
            rhs,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
    }))
}

fn lower_i64_map_key_array_string_index_compare(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Condition> {
    let Expr::BinaryCompare {
        op,
        lhs,
        rhs,
        ty: Type::Bool,
    } = expr
    else {
        return None;
    };
    let selected = if let Some(known) = i64_string_text(rhs, static_bindings) {
        lower_i64_map_key_array_string_index_match_expr(
            lhs,
            &known,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )
    } else if let Some(known) = i64_string_text(lhs, static_bindings) {
        lower_i64_map_key_array_string_index_match_expr(
            rhs,
            &known,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )
    } else {
        None
    }?;
    let expected = match op {
        CompareOp::Eq => 1,
        CompareOp::Ne => 0,
        CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge => return None,
    };
    Some(CraneliftI64Condition::Compare(CraneliftI64Compare {
        op: CraneliftI64CompareOp::Eq,
        lhs: selected,
        rhs: CraneliftI64Expr::Literal(expected),
    }))
}

fn lower_i64_map_key_array_string_index_match_expr(
    expr: &Expr,
    expected: &str,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    lower_i64_map_key_array_string_index_predicate_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
        |value| value == expected,
    )
}

fn lower_i64_map_key_array_string_index_predicate_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    predicate: impl Fn(&str) -> bool,
) -> Option<CraneliftI64Expr> {
    lower_i64_map_key_array_string_index_mapped_i64_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
        |value| predicate(value) as i64,
    )
}

fn lower_i64_map_key_array_string_index_mapped_i64_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    mapper: impl Fn(&str) -> i64,
) -> Option<CraneliftI64Expr> {
    let (keys, index, transform) = i64_map_key_array_string_index_source(expr, static_bindings)?;
    if keys.is_empty() {
        return None;
    }
    if let Some(index) = lower_i64_literal_index(&index) {
        return match keys.get(index)? {
            I64MapKey::Text(value) => Some(CraneliftI64Expr::Literal(mapper(
                i64_apply_map_key_array_string_transform(value, transform),
            ))),
            _ => None,
        };
    }
    let index = lower_i64_expr(
        &index,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let last = keys.len() - 1;
    let mut result = match keys.get(last)? {
        I64MapKey::Text(value) => CraneliftI64Expr::Literal(mapper(
            i64_apply_map_key_array_string_transform(value, transform),
        )),
        _ => return None,
    };
    for candidate in (0..last).rev() {
        let mapped = match keys.get(candidate)? {
            I64MapKey::Text(value) => {
                mapper(i64_apply_map_key_array_string_transform(value, transform))
            }
            _ => return None,
        };
        result = CraneliftI64Expr::Select {
            cond: Box::new(CraneliftI64Condition::Compare(CraneliftI64Compare {
                op: CraneliftI64CompareOp::Eq,
                lhs: index.clone(),
                rhs: CraneliftI64Expr::Literal(candidate as i64),
            })),
            then_result: Box::new(CraneliftI64Expr::Literal(mapped)),
            else_result: Box::new(result),
        };
    }
    Some(result)
}

fn i64_map_key_array_string_index_source(
    expr: &Expr,
    static_bindings: &I64StaticBindings,
) -> Option<(Vec<I64MapKey>, Expr, I64MapKeyArrayStringTransform)> {
    let binding = i64_map_key_array_string_index_binding(expr, static_bindings)?;
    let keys = static_bindings
        .map_key_arrays
        .get(binding.array_name.as_str())?
        .clone();
    Some((keys, binding.index, binding.transform))
}

fn i64_map_key_array_string_index_binding(
    expr: &Expr,
    static_bindings: &I64StaticBindings,
) -> Option<I64MapKeyArrayStringIndex> {
    if let Expr::StringBorrow { expr, .. } = expr {
        return i64_map_key_array_string_index_binding(expr, static_bindings);
    }
    if let Some((array_name, index)) = i64_map_key_array_string_index_parts(expr) {
        return Some(I64MapKeyArrayStringIndex {
            array_name: array_name.to_string(),
            index: index.clone(),
            transform: I64MapKeyArrayStringTransform::Identity,
        });
    }
    if let Expr::Call {
        name,
        args,
        ty: Type::String | Type::Str,
    } = expr
        && (name == "string_trim" || name == "string_trim_start")
    {
        let [text] = args.as_slice() else {
            return None;
        };
        let mut binding = i64_map_key_array_string_index_binding(text, static_bindings)?;
        binding.transform = i64_map_key_array_string_transform_compose(
            binding.transform,
            if name == "string_trim" {
                I64MapKeyArrayStringTransform::Trim
            } else {
                I64MapKeyArrayStringTransform::TrimStart
            },
        );
        return Some(binding);
    }
    let Expr::VarRef {
        name,
        ty: Type::String | Type::Str,
    } = expr
    else {
        return None;
    };
    static_bindings
        .map_key_array_string_indexes
        .get(name)
        .cloned()
}

fn i64_map_key_array_string_transform_compose(
    existing: I64MapKeyArrayStringTransform,
    next: I64MapKeyArrayStringTransform,
) -> I64MapKeyArrayStringTransform {
    match (existing, next) {
        (_, I64MapKeyArrayStringTransform::Identity) => existing,
        (I64MapKeyArrayStringTransform::Identity, transform) => transform,
        (I64MapKeyArrayStringTransform::Trim, _) => I64MapKeyArrayStringTransform::Trim,
        (I64MapKeyArrayStringTransform::TrimStart, I64MapKeyArrayStringTransform::Trim) => {
            I64MapKeyArrayStringTransform::Trim
        }
        (I64MapKeyArrayStringTransform::TrimStart, I64MapKeyArrayStringTransform::TrimStart) => {
            I64MapKeyArrayStringTransform::TrimStart
        }
    }
}

fn i64_apply_map_key_array_string_transform<'a>(
    value: &'a str,
    transform: I64MapKeyArrayStringTransform,
) -> &'a str {
    match transform {
        I64MapKeyArrayStringTransform::Identity => value,
        I64MapKeyArrayStringTransform::Trim => value.trim(),
        I64MapKeyArrayStringTransform::TrimStart => value.trim_start(),
    }
}

fn i64_map_key_array_string_index_parts(expr: &Expr) -> Option<(&str, &Expr)> {
    let Expr::Index {
        base,
        index,
        ty: Type::String | Type::Str,
    } = expr
    else {
        return None;
    };
    let Expr::VarRef {
        name,
        ty: Type::Array(element, None),
    } = base.as_ref()
    else {
        return None;
    };
    if !matches!(element.as_ref(), Type::String | Type::Str) {
        return None;
    }
    Some((name, index))
}

fn lower_i64_string_literal_compare(
    expr: &Expr,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Condition> {
    let Expr::BinaryCompare {
        op,
        lhs,
        rhs,
        ty: Type::Bool,
    } = expr
    else {
        return None;
    };
    let lhs = i64_string_text(lhs, static_bindings)?;
    let rhs = i64_string_text(rhs, static_bindings)?;
    let value = match op {
        CompareOp::Eq => lhs == rhs,
        CompareOp::Ne => lhs != rhs,
        CompareOp::Lt => lhs < rhs,
        CompareOp::Le => lhs <= rhs,
        CompareOp::Gt => lhs > rhs,
        CompareOp::Ge => lhs >= rhs,
    };
    Some(CraneliftI64Condition::Literal(value))
}

fn lower_i64_map_key_array_string_index_starts_with_condition(
    text: &Expr,
    prefix: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Condition> {
    let prefix = i64_string_text(prefix, static_bindings)?;
    let selected = lower_i64_map_key_array_string_index_predicate_expr(
        text,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
        |value| value.starts_with(prefix.as_str()),
    )?;
    Some(CraneliftI64Condition::Compare(CraneliftI64Compare {
        op: CraneliftI64CompareOp::Eq,
        lhs: selected,
        rhs: CraneliftI64Expr::Literal(1),
    }))
}

fn lower_i64_known_bool_intrinsic_condition(
    name: &str,
    args: &[Expr],
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Condition> {
    match name {
        "string_contains" | "string_starts_with" => {
            let [text, needle] = args else {
                return None;
            };
            if name == "string_starts_with" {
                if let Some(condition) = lower_i64_map_key_array_string_index_starts_with_condition(
                    text,
                    needle,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                ) {
                    return Some(condition);
                }
            }
            let text = i64_string_text(text, static_bindings)?;
            let needle = i64_string_text(needle, static_bindings)?;
            Some(CraneliftI64Condition::Literal(
                if name == "string_contains" {
                    text.contains(needle.as_str())
                } else {
                    text.starts_with(needle.as_str())
                },
            ))
        }
        name if is_i64_regex_is_match_name(name, static_bindings) => {
            let [pattern, text] = args else {
                return None;
            };
            Some(CraneliftI64Condition::Literal(
                regex_find_span(
                    &i64_string_text(pattern, static_bindings)?,
                    &i64_string_text(text, static_bindings)?,
                )
                .is_some(),
            ))
        }
        name if is_i64_http_respond_name(name, static_bindings) => {
            let [request, status, body] = args else {
                return None;
            };
            let request = lower_i64_http_request_stream_expr(
                request,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            let status = i64_static_scalar_value(status, static_bindings)?;
            let body = i64_string_text(body, static_bindings)?;
            Some(CraneliftI64Condition::Compare(CraneliftI64Compare {
                op: CraneliftI64CompareOp::Ne,
                lhs: CraneliftI64Expr::HttpResponseWrite {
                    request: Box::new(request),
                    response: i64_http_response(status, &body),
                },
                rhs: CraneliftI64Expr::Literal(0),
            }))
        }
        name if is_i64_http_close_name(name, static_bindings) => {
            let [server] = args else {
                return None;
            };
            Some(CraneliftI64Condition::Compare(CraneliftI64Compare {
                op: CraneliftI64CompareOp::Ne,
                lhs: CraneliftI64Expr::HttpServerClose {
                    server: Box::new(lower_i64_expr(
                        server,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )?),
                },
                rhs: CraneliftI64Expr::Literal(0),
            }))
        }
        name if is_i64_http_serve_once_name(name, static_bindings) => {
            let [bind, body] = args else {
                return None;
            };
            let bind = i64_string_text(bind, static_bindings)?;
            let body = i64_string_text(body, static_bindings)?;
            // Loopback serving lowers to a runtime one-shot server so the built
            // binary actually serves instead of the spike serving at compile
            // time. Non-loopback binds keep the fast compile-time fold (which
            // returns false without entering an accept loop).
            if let Some(addr) = http_parse_loopback_bind(&bind) {
                Some(CraneliftI64Condition::Compare(CraneliftI64Compare {
                    op: CraneliftI64CompareOp::Ne,
                    lhs: CraneliftI64Expr::HttpServeOnce {
                        port: addr.port(),
                        response: i64_http_ok_response(&body),
                    },
                    rhs: CraneliftI64Expr::Literal(0),
                }))
            } else {
                Some(CraneliftI64Condition::Literal(http_serve_once(
                    &bind, &body,
                )))
            }
        }
        name if is_i64_http_serve_route_name(name) => {
            let [bind, route_path, body, max_requests] = args else {
                return None;
            };
            let bind = i64_string_text(bind, static_bindings)?;
            let route_path = http_strip_crlf(&i64_string_text(route_path, static_bindings)?);
            let body = i64_string_text(body, static_bindings)?;
            let max_requests = i64_static_scalar_value(max_requests, static_bindings)?;
            if let Some(addr) = http_parse_loopback_bind(&bind) {
                if route_path.is_empty() || max_requests <= 0 {
                    return Some(CraneliftI64Condition::Literal(false));
                }
                Some(CraneliftI64Condition::Compare(CraneliftI64Compare {
                    op: CraneliftI64CompareOp::Ne,
                    lhs: CraneliftI64Expr::HttpServeRoute {
                        port: addr.port(),
                        route_path,
                        matched_response: i64_http_ok_response(&body),
                        unmatched_response: i64_http_not_found_response(),
                        max_requests,
                    },
                    rhs: CraneliftI64Expr::Literal(0),
                }))
            } else {
                Some(CraneliftI64Condition::Literal(http_serve_route(
                    &bind,
                    &route_path,
                    &body,
                    max_requests,
                )))
            }
        }
        name if is_i64_crypto_constant_time_eq_name(name, static_bindings) => {
            let [left, right] = args else {
                return None;
            };
            let left_text = i64_string_text(left, static_bindings)?;
            let right_text = i64_string_text(right, static_bindings)?;
            let result = constant_time_eq_bytes(left_text.as_bytes(), right_text.as_bytes());
            i64_audited_known_crypto_condition(left, result, static_bindings)
                .or_else(|| i64_audited_known_crypto_condition(right, result, static_bindings))
                .or(Some(CraneliftI64Condition::Literal(result)))
        }
        name if is_i64_crypto_constant_time_eq_u8_name(name, static_bindings) => {
            lower_i64_byte_slice_eq_condition(
                args,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        name if is_i64_crypto_verify_sha256_name(name, static_bindings)
            || is_i64_crypto_verify_sha512_name(name, static_bindings) =>
        {
            let [tag, key, message] = args else {
                return None;
            };
            let expected = if is_i64_crypto_verify_sha256_name(name, static_bindings) {
                hmac_hex(
                    i64_string_text(key, static_bindings)?.as_bytes(),
                    i64_string_text(message, static_bindings)?.as_bytes(),
                    64,
                    sha256_bytes,
                )
            } else {
                hmac_hex(
                    i64_string_text(key, static_bindings)?.as_bytes(),
                    i64_string_text(message, static_bindings)?.as_bytes(),
                    128,
                    sha512_bytes,
                )
            };
            Some(CraneliftI64Condition::Literal(constant_time_eq_bytes(
                i64_string_text(tag, static_bindings)?.as_bytes(),
                expected.as_bytes(),
            )))
        }
        name if is_i64_sync_once_is_set_name(name, static_bindings) => {
            let [cell] = args else {
                return None;
            };
            if let Some(value) = i64_i64_once_cell_value(cell, static_bindings) {
                return Some(CraneliftI64Condition::Literal(value.is_some()));
            }
            Some(CraneliftI64Condition::Literal(
                i64_bool_once_cell_value(cell, static_bindings)?.is_some(),
            ))
        }
        _ => None,
    }
}

fn lower_i64_byte_slice_eq_condition(
    args: &[Expr],
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Condition> {
    let [left, right] = args else {
        return None;
    };
    if !matches!(
        i64_fixed_array_or_slice_element(left, static_bindings)?,
        Type::Numeric(NumericType::U8)
    ) || !matches!(
        i64_fixed_array_or_slice_element(right, static_bindings)?,
        Type::Numeric(NumericType::U8)
    ) {
        return None;
    }
    let left = lower_i64_array_or_slice_call_arg_exprs(
        left,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let right = lower_i64_array_or_slice_call_arg_exprs(
        right,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    if left.len() != right.len() {
        return Some(CraneliftI64Condition::Literal(false));
    }
    left.into_iter()
        .zip(right)
        .map(|(lhs, rhs)| {
            CraneliftI64Condition::Compare(CraneliftI64Compare {
                op: CraneliftI64CompareOp::Eq,
                lhs,
                rhs,
            })
        })
        .reduce(|lhs, rhs| CraneliftI64Condition::And {
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        })
        .or(Some(CraneliftI64Condition::Literal(true)))
}

fn i64_string_literal_text(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Literal(LiteralValue::String(value)) | Expr::Literal(LiteralValue::Str(value)) => {
            Some(value.as_str())
        }
        Expr::StringBorrow { expr, .. } => i64_string_literal_text(expr),
        _ => None,
    }
}

fn i64_string_text(expr: &Expr, static_bindings: &I64StaticBindings) -> Option<String> {
    if let Some(value) = i64_string_literal_text(expr) {
        return Some(value.to_string());
    }
    match expr {
        Expr::VarRef {
            name,
            ty: Type::String | Type::Str,
        } => static_bindings.strings.get(name).cloned(),
        Expr::BinaryAdd {
            op: ArithmeticOp::Add,
            lhs,
            rhs,
            ty: Type::String | Type::Str,
        } => Some(format!(
            "{}{}",
            i64_string_text(lhs, static_bindings)?,
            i64_string_text(rhs, static_bindings)?
        )),
        Expr::Call {
            name,
            args,
            ty: Type::String | Type::Str,
        } => i64_string_call_text(name, args, static_bindings),
        Expr::Index {
            base,
            index,
            ty: Type::String | Type::Str,
        } => i64_map_key_array_string_index_text(base, index, static_bindings),
        Expr::StringBorrow { expr, .. } => i64_string_text(expr, static_bindings),
        _ => None,
    }
}

fn i64_string_builder_text(expr: &Expr, static_bindings: &I64StaticBindings) -> Option<String> {
    match expr {
        Expr::VarRef {
            name,
            ty: Type::Struct(_),
        } => static_bindings.string_builders.get(name).cloned(),
        Expr::Call { name, args, .. } if is_i64_string_builder_new_name(name, static_bindings) => {
            let [] = args.as_slice() else {
                return None;
            };
            Some(String::new())
        }
        Expr::Call { name, args, .. }
            if is_i64_string_builder_from_string_name(name, static_bindings) =>
        {
            let [value] = args.as_slice() else {
                return None;
            };
            i64_string_text(value, static_bindings)
        }
        Expr::Call { name, args, .. }
            if is_i64_string_builder_push_str_name(name, static_bindings)
                || is_i64_string_builder_push_line_name(name, static_bindings) =>
        {
            let [builder, text] = args.as_slice() else {
                return None;
            };
            let mut value = i64_string_builder_text(builder, static_bindings)?;
            value.push_str(&i64_string_text(text, static_bindings)?);
            if is_i64_string_builder_push_line_name(name, static_bindings) {
                value.push('\n');
            }
            Some(value)
        }
        Expr::StructLiteral { fields, .. } => fields
            .iter()
            .find(|field| field.name == "value")
            .and_then(|field| i64_string_text(&field.expr, static_bindings)),
        _ => None,
    }
}

fn i64_map_key_array_string_index_text(
    base: &Expr,
    index: &Expr,
    static_bindings: &I64StaticBindings,
) -> Option<String> {
    let Expr::VarRef {
        name,
        ty: Type::Array(element, None),
    } = base
    else {
        return None;
    };
    if !matches!(element.as_ref(), Type::String | Type::Str) {
        return None;
    }
    let index = lower_i64_literal_index(index)?;
    match static_bindings.map_key_arrays.get(name)?.get(index)? {
        I64MapKey::Text(value) => Some(value.clone()),
        _ => None,
    }
}

fn i64_string_call_text(
    name: &str,
    args: &[Expr],
    static_bindings: &I64StaticBindings,
) -> Option<String> {
    match name {
        "string_clone" => {
            let [text] = args else {
                return None;
            };
            i64_string_text(text, static_bindings)
        }
        "string_trim" | "string_trim_start" => {
            let [text] = args else {
                return None;
            };
            let text = i64_string_text(text, static_bindings)?;
            let trimmed = if name == "string_trim" {
                text.trim()
            } else {
                text.trim_start()
            };
            Some(trimmed.to_string())
        }
        name if is_i64_encoding_percent_encode_name(name, static_bindings) => {
            let [text] = args else {
                return None;
            };
            Some(percent_encode(&i64_string_text(text, static_bindings)?))
        }
        name if is_i64_encoding_url_query_pair_encode_name(name, static_bindings) => {
            let [key, value] = args else {
                return None;
            };
            Some(format!(
                "{}={}",
                percent_encode(&i64_string_text(key, static_bindings)?),
                percent_encode(&i64_string_text(value, static_bindings)?)
            ))
        }
        name if is_i64_encoding_path_join_segment_name(name, static_bindings) => {
            let [base, segment] = args else {
                return None;
            };
            let base = i64_string_text(base, static_bindings)?;
            let encoded = percent_encode(&i64_string_text(segment, static_bindings)?);
            Some(if base.is_empty() {
                encoded
            } else if base.ends_with('/') {
                format!("{base}{encoded}")
            } else {
                format!("{base}/{encoded}")
            })
        }
        name if is_i64_crypto_sha256_name(name, static_bindings) => {
            let [input] = args else {
                return None;
            };
            Some(sha256_hex(
                i64_string_text(input, static_bindings)?.as_bytes(),
            ))
        }
        name if is_i64_crypto_hmac_sha256_name(name, static_bindings)
            || is_i64_crypto_hmac_sha512_name(name, static_bindings) =>
        {
            let [key, message] = args else {
                return None;
            };
            let key = i64_string_text(key, static_bindings)?;
            let message = i64_string_text(message, static_bindings)?;
            Some(if is_i64_crypto_hmac_sha256_name(name, static_bindings) {
                hmac_hex(key.as_bytes(), message.as_bytes(), 64, sha256_bytes)
            } else {
                hmac_hex(key.as_bytes(), message.as_bytes(), 128, sha512_bytes)
            })
        }
        name if is_i64_regex_replace_all_name(name, static_bindings) => {
            let [pattern, text, replacement] = args else {
                return None;
            };
            Some(regex_replace_all(
                &i64_string_text(pattern, static_bindings)?,
                &i64_string_text(text, static_bindings)?,
                &i64_string_text(replacement, static_bindings)?,
            ))
        }
        name if is_i64_json_stringify_string_name(name, static_bindings) => {
            let [text] = args else {
                return None;
            };
            Some(json_escape_string(&i64_string_text(text, static_bindings)?))
        }
        "json_stringify_value" => {
            let [text] = args else {
                return None;
            };
            let text = i64_string_text(text, static_bindings)?;
            Some(json_parse_value(&text).unwrap_or(text))
        }
        name if is_i64_json_stringify_int_name(name, static_bindings) => {
            let [value] = args else {
                return None;
            };
            Some(i64_static_scalar_value(value, static_bindings)?.to_string())
        }
        name if is_i64_json_stringify_bool_name(name, static_bindings) => {
            let [value] = args else {
                return None;
            };
            Some(i64_static_bool_value(value, static_bindings)?.to_string())
        }
        name if is_i64_log_field_string_name(name, static_bindings) => {
            let [key, value] = args else {
                return None;
            };
            Some(format!(
                "{}:{}",
                json_escape_string(&i64_string_text(key, static_bindings)?),
                json_escape_string(&i64_string_text(value, static_bindings)?)
            ))
        }
        name if is_i64_log_field_int_name(name, static_bindings) => {
            let [key, value] = args else {
                return None;
            };
            Some(format!(
                "{}:{}",
                json_escape_string(&i64_string_text(key, static_bindings)?),
                i64_static_scalar_value(value, static_bindings)?
            ))
        }
        name if is_i64_log_field_bool_name(name, static_bindings) => {
            let [key, value] = args else {
                return None;
            };
            Some(format!(
                "{}:{}",
                json_escape_string(&i64_string_text(key, static_bindings)?),
                i64_static_bool_value(value, static_bindings)?
            ))
        }
        name if is_i64_log_fields2_name(name, static_bindings) => {
            let [first, second] = args else {
                return None;
            };
            Some(format!(
                "{},{}",
                i64_string_text(first, static_bindings)?,
                i64_string_text(second, static_bindings)?
            ))
        }
        name if is_i64_log_fields3_name(name, static_bindings) => {
            let [first, second, third] = args else {
                return None;
            };
            Some(format!(
                "{},{},{}",
                i64_string_text(first, static_bindings)?,
                i64_string_text(second, static_bindings)?,
                i64_string_text(third, static_bindings)?
            ))
        }
        name if is_i64_log_event_name(name, static_bindings) => {
            let [level, message, attributes] = args else {
                return None;
            };
            Some(format!(
                "{{\"level\":{},\"message\":{},\"attributes\":{{{}}}}}",
                json_escape_string(&i64_string_text(level, static_bindings)?),
                json_escape_string(&i64_string_text(message, static_bindings)?),
                i64_string_text(attributes, static_bindings)?
            ))
        }
        name if is_i64_string_builder_finish_name(name, static_bindings) => {
            let [builder] = args else {
                return None;
            };
            i64_string_builder_text(builder, static_bindings)
        }
        _ => match i64_known_helper_call_value(name, args, static_bindings)? {
            SpikeValue::Text(value) => Some(value),
            _ => None,
        },
    }
}

fn i64_known_helper_call_i64_expr(
    name: &str,
    args: &[Expr],
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    match i64_known_helper_call_value(name, args, static_bindings)? {
        SpikeValue::Int(value) => Some(CraneliftI64Expr::Literal(value)),
        SpikeValue::UInt(value) => i64::try_from(value).ok().map(CraneliftI64Expr::Literal),
        SpikeValue::Bool(value) => Some(CraneliftI64Expr::Literal(i64::from(value))),
        _ => None,
    }
}

fn i64_known_helper_call_condition(
    name: &str,
    args: &[Expr],
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Condition> {
    match i64_known_helper_call_value(name, args, static_bindings)? {
        SpikeValue::Bool(value) => Some(CraneliftI64Condition::Literal(value)),
        _ => None,
    }
}

fn i64_known_helper_call_value(
    name: &str,
    args: &[Expr],
    static_bindings: &I64StaticBindings,
) -> Option<SpikeValue> {
    let function = static_bindings.functions.get(name)?;
    if function.params.len() != args.len()
        || !i64_known_helper_function_is_pure(function, static_bindings, 0)
    {
        return None;
    }
    let functions = static_bindings
        .functions
        .iter()
        .map(|(name, function)| (name.as_str(), function))
        .collect::<HashMap<_, _>>();
    let mut lines = Vec::new();
    let base_env = i64_known_static_env(static_bindings)?;
    let mut env = base_env.clone();
    for (param, arg) in function.params.iter().zip(args) {
        if !i64_known_expr_is_pure(arg, static_bindings, 0) {
            return None;
        }
        let value = eval_expr(arg, &functions, &base_env, &mut lines).ok()?;
        env.insert(param.name.clone(), value);
    }
    let value = run_function_body(&function.body, &functions, &mut env, &mut lines)
        .ok()
        .flatten()?;
    lines.is_empty().then_some(value)
}

fn i64_known_static_env(static_bindings: &I64StaticBindings) -> Option<SpikeEnv> {
    let mut env = SpikeEnv::new();
    if let Some(root) = &static_bindings.package_root {
        env.insert(
            SPIKE_PACKAGE_ROOT_BINDING.to_string(),
            SpikeValue::Text(root.display().to_string()),
        );
    }
    if let Some(root) = &static_bindings.fs_root {
        env.insert(
            SPIKE_FS_ROOT_BINDING.to_string(),
            SpikeValue::Text(root.display().to_string()),
        );
    }
    for (name, value) in &static_bindings.values {
        let value = match value {
            CraneliftI64Expr::Literal(value) => SpikeValue::Int(*value),
            _ => return None,
        };
        env.insert(name.clone(), value);
    }
    for (name, condition) in &static_bindings.conditions {
        let value = match condition {
            CraneliftI64Condition::Literal(value) => SpikeValue::Bool(*value),
            _ => return None,
        };
        env.insert(name.clone(), value);
    }
    for (name, value) in &static_bindings.strings {
        env.insert(name.clone(), SpikeValue::Text(value.clone()));
    }
    Some(env)
}

fn i64_known_helper_function_is_pure(
    function: &Function,
    static_bindings: &I64StaticBindings,
    depth: usize,
) -> bool {
    if depth > 8 || function.is_property || function.is_async || function.is_extern {
        return false;
    }
    i64_known_helper_body_is_pure(&function.body, static_bindings, depth + 1)
}

fn i64_known_helper_body_is_pure(
    body: &[Stmt],
    static_bindings: &I64StaticBindings,
    depth: usize,
) -> bool {
    if depth > 8 {
        return false;
    }
    if body.is_empty() {
        return false;
    }
    body.iter().enumerate().all(|(index, stmt)| match stmt {
        Stmt::Let { expr, .. } => {
            index + 1 < body.len() && i64_known_expr_is_pure(expr, static_bindings, depth + 1)
        }
        Stmt::Return { expr, .. } => {
            index + 1 == body.len() && i64_known_expr_is_pure(expr, static_bindings, depth + 1)
        }
        Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            index + 1 == body.len()
                && i64_known_expr_is_pure(cond, static_bindings, depth + 1)
                && i64_known_helper_body_is_pure(then_block, static_bindings, depth + 1)
                && else_block.as_ref().is_some_and(|else_block| {
                    i64_known_helper_body_is_pure(else_block, static_bindings, depth + 1)
                })
        }
        Stmt::Match { expr, arms, .. } => {
            index + 1 == body.len()
                && i64_known_expr_is_pure(expr, static_bindings, depth + 1)
                && arms
                    .iter()
                    .all(|arm| i64_known_helper_body_is_pure(&arm.body, static_bindings, depth + 1))
        }
        _ => false,
    })
}

fn i64_known_expr_is_pure(expr: &Expr, static_bindings: &I64StaticBindings, depth: usize) -> bool {
    if depth > 8 {
        return false;
    }
    match expr {
        Expr::Literal(_) | Expr::VarRef { .. } => true,
        Expr::StringBorrow { expr, .. } | Expr::Cast { expr, .. } => {
            i64_known_expr_is_pure(expr, static_bindings, depth + 1)
        }
        Expr::BinaryAdd { lhs, rhs, .. }
        | Expr::BinaryCompare { lhs, rhs, .. }
        | Expr::BinaryLogic { lhs, rhs, .. } => {
            i64_known_expr_is_pure(lhs, static_bindings, depth + 1)
                && i64_known_expr_is_pure(rhs, static_bindings, depth + 1)
        }
        Expr::Call { name, args, .. } => {
            args.iter()
                .all(|arg| i64_known_expr_is_pure(arg, static_bindings, depth + 1))
                && (i64_known_pure_intrinsic_call(name, static_bindings)
                    || static_bindings.functions.get(name).is_some_and(|function| {
                        i64_known_helper_function_is_pure(function, static_bindings, depth + 1)
                    }))
        }
        Expr::Index { base, index, .. } => {
            i64_known_expr_is_pure(base, static_bindings, depth + 1)
                && i64_known_expr_is_pure(index, static_bindings, depth + 1)
        }
        Expr::FieldAccess { base, .. } | Expr::TupleIndex { base, .. } => {
            i64_known_expr_is_pure(base, static_bindings, depth + 1)
        }
        Expr::ArrayLiteral { elements, .. } | Expr::TupleLiteral { elements, .. } => elements
            .iter()
            .all(|element| i64_known_expr_is_pure(element, static_bindings, depth + 1)),
        Expr::MapLiteral { entries, .. } => entries.iter().all(|entry| {
            i64_known_expr_is_pure(&entry.key, static_bindings, depth + 1)
                && i64_known_expr_is_pure(&entry.value, static_bindings, depth + 1)
        }),
        Expr::EnumVariant { payloads, .. } => payloads
            .iter()
            .all(|payload| i64_known_expr_is_pure(payload, static_bindings, depth + 1)),
        Expr::StructLiteral { fields, .. } => fields
            .iter()
            .all(|field| i64_known_expr_is_pure(&field.expr, static_bindings, depth + 1)),
        Expr::Match { expr, arms, .. } => {
            i64_known_expr_is_pure(expr, static_bindings, depth + 1)
                && arms
                    .iter()
                    .all(|arm| i64_known_expr_is_pure(&arm.expr, static_bindings, depth + 1))
        }
        _ => false,
    }
}

fn i64_known_pure_intrinsic_call(name: &str, static_bindings: &I64StaticBindings) -> bool {
    matches!(
        name,
        "len"
            | "first"
            | "last"
            | "get"
            | "map_get"
            | "string_clone"
            | "string_contains"
            | "string_starts_with"
            | "string_strip_prefix"
            | "string_strip_suffix"
            | "string_trim"
            | "string_trim_start"
            | "string_line_at"
            | "string_byte_at"
            | "encoding_url_component_encode"
            | "encoding_url_component_decode"
            | "encoding_path_segment_encode"
            | "encoding_url_query_pair_encode"
            | "encoding_path_join_segment"
            | "json_parse_int"
            | "json_parse_bool"
            | "json_parse_string"
            | "json_stringify_int"
            | "int_to_string"
            | "json_stringify_bool"
            | "json_stringify_string"
            | "json_serdes_parse"
            | "json_serdes_parse_str"
            | "json_serdes_value_to_json"
            | "json_serdes_to_json"
            | "std_serdes_is_null"
            | "std_serdes_as_bool"
            | "std_serdes_as_int"
            | "std_serdes_as_text"
            | "std_serdes_as_array"
            | "std_serdes_as_object"
            | "std_serdes_field"
            | "std_serdes_bool_field"
            | "std_serdes_text_field"
            | "std_serdes_int_field"
            | "std_serdes_array_field"
            | "std_serdes_object_field"
            | "std_serdes_value_item"
    ) || is_i64_encoding_percent_encode_name(name, static_bindings)
        || is_i64_encoding_url_query_pair_encode_name(name, static_bindings)
        || is_i64_encoding_path_join_segment_name(name, static_bindings)
        || is_i64_json_stringify_int_name(name, static_bindings)
        || is_i64_json_stringify_bool_name(name, static_bindings)
        || is_i64_json_stringify_string_name(name, static_bindings)
}

fn i64_static_scalar_value(expr: &Expr, static_bindings: &I64StaticBindings) -> Option<i64> {
    if let Some(value) = lower_i64_literal_value(expr) {
        return Some(value);
    }
    let Expr::VarRef { name, .. } = expr else {
        return None;
    };
    match static_bindings.values.get(name)? {
        CraneliftI64Expr::Literal(value) => Some(*value),
        _ => None,
    }
}

fn i64_static_bool_value(expr: &Expr, static_bindings: &I64StaticBindings) -> Option<bool> {
    if let Expr::Literal(LiteralValue::Bool(value)) = expr {
        return Some(*value);
    }
    let Expr::VarRef {
        name,
        ty: Type::Bool,
    } = expr
    else {
        return None;
    };
    match static_bindings.conditions.get(name)? {
        CraneliftI64Condition::Literal(value) => Some(*value),
        _ => None,
    }
}

fn i64_string_option_text(
    expr: &Expr,
    static_bindings: &I64StaticBindings,
) -> Option<Option<String>> {
    if let Expr::VarRef {
        name,
        ty: Type::Option(inner),
    } = expr
        && is_i64_known_string_option_call_let_type(inner)
    {
        return static_bindings.string_options.get(name).cloned();
    }
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if let Some(value) = i64_map_get_value_expr(name, args, static_bindings) {
        return match value {
            Some(value) => Some(Some(i64_string_text(value, static_bindings)?)),
            None => Some(None),
        };
    }
    match name.as_str() {
        "string_strip_prefix" => {
            let [text, prefix] = args.as_slice() else {
                return None;
            };
            let text = i64_string_text(text, static_bindings)?;
            let prefix = i64_string_text(prefix, static_bindings)?;
            Some(
                text.strip_prefix(&prefix)
                    .map(std::string::ToString::to_string),
            )
        }
        "string_strip_suffix" => {
            let [text, suffix] = args.as_slice() else {
                return None;
            };
            let text = i64_string_text(text, static_bindings)?;
            let suffix = i64_string_text(suffix, static_bindings)?;
            Some(
                text.strip_suffix(&suffix)
                    .map(std::string::ToString::to_string),
            )
        }
        "string_line_at" => {
            let [text, index] = args.as_slice() else {
                return None;
            };
            let text = i64_string_text(text, static_bindings)?;
            let index = i64_static_scalar_value(index, static_bindings)?;
            if index < 0 {
                return Some(None);
            }
            Some(
                text.lines()
                    .nth(index as usize)
                    .map(std::string::ToString::to_string),
            )
        }
        name if is_i64_encoding_url_component_decode_name(name, static_bindings) => {
            let [text] = args.as_slice() else {
                return None;
            };
            Some(percent_decode(&i64_string_text(text, static_bindings)?))
        }
        name if is_i64_regex_find_name(name, static_bindings) => {
            let [pattern, text] = args.as_slice() else {
                return None;
            };
            let pattern = i64_string_text(pattern, static_bindings)?;
            let text = i64_string_text(text, static_bindings)?;
            Some(regex_find_span(&pattern, &text).map(|(start, end)| text[start..end].to_string()))
        }
        name if is_i64_json_parse_string_name(name, static_bindings) => {
            let [text] = args.as_slice() else {
                return None;
            };
            Some(json_parse_string(&i64_string_text(text, static_bindings)?))
        }
        name if is_i64_json_parse_field_string_name(name, static_bindings) => {
            let [text, key] = args.as_slice() else {
                return None;
            };
            let text = i64_string_text(text, static_bindings)?;
            let key = i64_string_text(key, static_bindings)?;
            Some(json_object_field(&text, &key).and_then(|value| json_parse_string(&value)))
        }
        _ => None,
    }
}

fn i64_i64_option_value(expr: &Expr, static_bindings: &I64StaticBindings) -> Option<Option<i64>> {
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if is_i64_sync_once_take_name(name, static_bindings) {
        let [cell] = args.as_slice() else {
            return None;
        };
        return i64_i64_once_cell_value(cell, static_bindings);
    }
    if is_i64_sync_try_recv_name(name, static_bindings) {
        let [channel] = args.as_slice() else {
            return None;
        };
        return i64_i64_channel_cell_value(channel, static_bindings);
    }
    if let Some(value) = i64_map_get_value_expr(name, args, static_bindings) {
        return match value {
            Some(value) => Some(Some(i64_static_scalar_value(value, static_bindings)?)),
            None => Some(None),
        };
    }
    match name.as_str() {
        "string_byte_at" => {
            let [text, index] = args.as_slice() else {
                return None;
            };
            let text = i64_string_text(text, static_bindings)?;
            let index = i64_static_scalar_value(index, static_bindings)?;
            if index < 0 {
                return Some(None);
            }
            Some(
                text.as_bytes()
                    .get(index as usize)
                    .map(|byte| i64::from(*byte)),
            )
        }
        name if is_i64_json_parse_int_name(name, static_bindings) => {
            let [text] = args.as_slice() else {
                return None;
            };
            Some(json_parse_int(&i64_string_text(text, static_bindings)?))
        }
        name if is_i64_json_parse_field_int_name(name, static_bindings) => {
            let [text, key] = args.as_slice() else {
                return None;
            };
            let text = i64_string_text(text, static_bindings)?;
            let key = i64_string_text(key, static_bindings)?;
            Some(json_object_field(&text, &key).and_then(|value| json_parse_int(&value)))
        }
        _ => None,
    }
}

fn i64_bool_option_value(expr: &Expr, static_bindings: &I64StaticBindings) -> Option<Option<bool>> {
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if is_i64_sync_once_take_name(name, static_bindings) {
        let [cell] = args.as_slice() else {
            return None;
        };
        return i64_bool_once_cell_value(cell, static_bindings);
    }
    if is_i64_sync_try_recv_name(name, static_bindings) {
        let [channel] = args.as_slice() else {
            return None;
        };
        return i64_bool_channel_cell_value(channel, static_bindings);
    }
    if let Some(value) = i64_map_get_value_expr(name, args, static_bindings) {
        return match value {
            Some(value) => Some(Some(i64_static_bool_value(value, static_bindings)?)),
            None => Some(None),
        };
    }
    match name.as_str() {
        name if is_i64_json_parse_bool_name(name, static_bindings) => {
            let [text] = args.as_slice() else {
                return None;
            };
            Some(json_parse_bool(&i64_string_text(text, static_bindings)?))
        }
        name if is_i64_json_parse_field_bool_name(name, static_bindings) => {
            let [text, key] = args.as_slice() else {
                return None;
            };
            let text = i64_string_text(text, static_bindings)?;
            let key = i64_string_text(key, static_bindings)?;
            Some(json_object_field(&text, &key).and_then(|value| json_parse_bool(&value)))
        }
        _ => None,
    }
}

fn i64_i64_once_cell_value(
    expr: &Expr,
    static_bindings: &I64StaticBindings,
) -> Option<Option<i64>> {
    if let Expr::VarRef { name, .. } = expr {
        return static_bindings.i64_once_cells.get(name).copied();
    }
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if is_i64_sync_once_with_name(name, static_bindings) {
        let [value] = args.as_slice() else {
            return None;
        };
        return Some(Some(i64_static_scalar_value(value, static_bindings)?));
    }
    if is_i64_sync_once_name(name, static_bindings) {
        let [value] = args.as_slice() else {
            return None;
        };
        return i64_i64_option_expr_value(value, static_bindings);
    }
    None
}

fn i64_i64_option_expr_value(
    expr: &Expr,
    static_bindings: &I64StaticBindings,
) -> Option<Option<i64>> {
    match expr {
        Expr::EnumVariant {
            enum_name,
            variant,
            payloads,
            ty: Type::Option(inner),
            ..
        } if enum_name == "Option" && is_i64_compatible_type(inner.as_ref()) => {
            match (variant.as_str(), payloads.as_slice()) {
                ("Some", [payload]) => {
                    Some(Some(i64_static_scalar_value(payload, static_bindings)?))
                }
                ("None", []) => Some(None),
                _ => None,
            }
        }
        Expr::Call { .. } => i64_i64_option_value(expr, static_bindings),
        _ => None,
    }
}

fn i64_bool_once_cell_value(
    expr: &Expr,
    static_bindings: &I64StaticBindings,
) -> Option<Option<bool>> {
    if let Expr::VarRef { name, .. } = expr {
        return static_bindings.bool_once_cells.get(name).copied();
    }
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if is_i64_sync_once_with_name(name, static_bindings) {
        let [value] = args.as_slice() else {
            return None;
        };
        return Some(Some(i64_static_bool_value(value, static_bindings)?));
    }
    if is_i64_sync_once_name(name, static_bindings) {
        let [value] = args.as_slice() else {
            return None;
        };
        return i64_bool_option_expr_value(value, static_bindings);
    }
    None
}

fn i64_bool_option_expr_value(
    expr: &Expr,
    static_bindings: &I64StaticBindings,
) -> Option<Option<bool>> {
    match expr {
        Expr::EnumVariant {
            enum_name,
            variant,
            payloads,
            ty: Type::Option(inner),
            ..
        } if enum_name == "Option" && matches!(inner.as_ref(), Type::Bool) => {
            match (variant.as_str(), payloads.as_slice()) {
                ("Some", [payload]) => Some(Some(i64_static_bool_value(payload, static_bindings)?)),
                ("None", []) => Some(None),
                _ => None,
            }
        }
        Expr::Call { .. } => i64_bool_option_value(expr, static_bindings),
        _ => None,
    }
}

fn i64_i64_channel_cell_value(
    expr: &Expr,
    static_bindings: &I64StaticBindings,
) -> Option<Option<i64>> {
    if let Expr::VarRef { name, .. } = expr {
        return static_bindings.i64_channels.get(name).copied();
    }
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if is_i64_sync_channel_name(name, static_bindings) {
        let [slot] = args.as_slice() else {
            return None;
        };
        return i64_i64_option_expr_value(slot, static_bindings);
    }
    if is_i64_sync_send_name(name, static_bindings) {
        let [_channel, value] = args.as_slice() else {
            return None;
        };
        return Some(Some(i64_static_scalar_value(value, static_bindings)?));
    }
    None
}

fn i64_bool_channel_cell_value(
    expr: &Expr,
    static_bindings: &I64StaticBindings,
) -> Option<Option<bool>> {
    if let Expr::VarRef { name, .. } = expr {
        return static_bindings.bool_channels.get(name).copied();
    }
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if is_i64_sync_channel_name(name, static_bindings) {
        let [slot] = args.as_slice() else {
            return None;
        };
        return i64_bool_option_expr_value(slot, static_bindings);
    }
    if is_i64_sync_send_name(name, static_bindings) {
        let [_channel, value] = args.as_slice() else {
            return None;
        };
        return Some(Some(i64_static_bool_value(value, static_bindings)?));
    }
    None
}

fn lower_i64_bool_literal_compare(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Condition> {
    let Expr::BinaryCompare {
        op,
        lhs,
        rhs,
        ty: Type::Bool,
    } = expr
    else {
        return None;
    };
    let (condition, value) = match (lhs.as_ref(), rhs.as_ref()) {
        (expr, Expr::Literal(LiteralValue::Bool(value))) => {
            let condition = lower_i64_condition(
                expr,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            (condition, *value)
        }
        (Expr::Literal(LiteralValue::Bool(value)), expr) => {
            let condition = lower_i64_condition(
                expr,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            (condition, *value)
        }
        _ => return None,
    };
    match (op, value) {
        (CompareOp::Eq, true) | (CompareOp::Ne, false) => Some(condition),
        (CompareOp::Eq, false) | (CompareOp::Ne, true) => invert_i64_simple_condition(condition),
        _ => None,
    }
}

fn invert_i64_simple_condition(condition: CraneliftI64Condition) -> Option<CraneliftI64Condition> {
    match condition {
        CraneliftI64Condition::Literal(value) => Some(CraneliftI64Condition::Literal(!value)),
        CraneliftI64Condition::Compare(compare) => {
            Some(CraneliftI64Condition::Compare(CraneliftI64Compare {
                op: invert_i64_compare_op(compare.op)?,
                lhs: compare.lhs,
                rhs: compare.rhs,
            }))
        }
        CraneliftI64Condition::And { lhs, rhs } => Some(CraneliftI64Condition::Or {
            lhs: Box::new(invert_i64_simple_condition(*lhs)?),
            rhs: Box::new(invert_i64_simple_condition(*rhs)?),
        }),
        CraneliftI64Condition::Or { lhs, rhs } => Some(CraneliftI64Condition::And {
            lhs: Box::new(invert_i64_simple_condition(*lhs)?),
            rhs: Box::new(invert_i64_simple_condition(*rhs)?),
        }),
    }
}

fn invert_i64_compare_op(op: CraneliftI64CompareOp) -> Option<CraneliftI64CompareOp> {
    match op {
        CraneliftI64CompareOp::Eq => Some(CraneliftI64CompareOp::Ne),
        CraneliftI64CompareOp::Ne => Some(CraneliftI64CompareOp::Eq),
        _ => None,
    }
}

fn lower_i64_compare(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Compare> {
    let Expr::BinaryCompare {
        op,
        lhs,
        rhs,
        ty: Type::Bool,
    } = expr
    else {
        return None;
    };
    Some(CraneliftI64Compare {
        op: lower_i64_compare_op(*op),
        lhs: lower_i64_expr(
            lhs,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
        rhs: lower_i64_expr(
            rhs,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
    })
}

fn lower_i64_compare_op(op: CompareOp) -> CraneliftI64CompareOp {
    match op {
        CompareOp::Eq => CraneliftI64CompareOp::Eq,
        CompareOp::Ne => CraneliftI64CompareOp::Ne,
        CompareOp::Lt => CraneliftI64CompareOp::Lt,
        CompareOp::Le => CraneliftI64CompareOp::Le,
        CompareOp::Gt => CraneliftI64CompareOp::Gt,
        CompareOp::Ge => CraneliftI64CompareOp::Ge,
    }
}

fn lower_i64_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if let Some(expr) = lower_i64_option_match_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(expr);
    }
    if let Some(expr) = lower_i64_result_match_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(expr);
    }
    if let Some(expr) = lower_i64_enum_match_value_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(expr);
    }
    match expr {
        Expr::Literal(LiteralValue::Int(value)) => Some(CraneliftI64Expr::Literal(*value)),
        Expr::Literal(LiteralValue::Numeric { raw, ty }) => {
            lower_i64_numeric_literal(raw, *ty).map(CraneliftI64Expr::Literal)
        }
        Expr::Call { name, args, .. } if is_i64_crypto_random_u64_name(name, static_bindings) => {
            lower_i64_crypto_random_intrinsic_expr(name, args, static_bindings)
        }
        Expr::VarRef { name, ty } if is_i64_compatible_type(ty) => local_indexes
            .get(name.as_str())
            .copied()
            .map(CraneliftI64Expr::Local)
            .or_else(|| static_bindings.values.get(name).cloned()),
        Expr::VarRef { name, .. } => static_bindings.values.get(name).cloned(),
        Expr::Call { name, args, ty } if is_i64_compatible_type(ty) => {
            lower_i64_clock_intrinsic_expr(
                name,
                args,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
            .or_else(|| lower_i64_process_intrinsic_expr(name, args, static_bindings))
            .or_else(|| lower_i64_fs_write_intrinsic_expr(name, args, static_bindings))
            .or_else(|| lower_i64_crypto_random_intrinsic_expr(name, args, static_bindings))
            .or_else(|| {
                lower_i64_ffi_intrinsic_expr(
                    name,
                    args,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )
            })
            .or_else(|| {
                lower_i64_map_get_or_default_expr(
                    name,
                    args,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )
            })
            .or_else(|| {
                lower_i64_fixed_array_intrinsic_expr(
                    name,
                    args,
                    ty,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )
            })
            .or_else(|| {
                lower_i64_http_server_intrinsic_expr(
                    name,
                    args,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )
            })
            .or_else(|| i64_known_helper_call_i64_expr(name, args, static_bindings))
            .or_else(|| {
                lower_i64_call_expr(
                    name,
                    args,
                    false,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )
            })
        }
        Expr::BinaryAdd { op, lhs, rhs, ty } if is_i64_compatible_type(ty) => {
            let arith_op = *op;
            let op = match op {
                ArithmeticOp::Add => CraneliftI64BinaryOp::Add,
                ArithmeticOp::Sub => CraneliftI64BinaryOp::Sub,
                ArithmeticOp::Mul => CraneliftI64BinaryOp::Mul,
                ArithmeticOp::Div => CraneliftI64BinaryOp::Div,
            };
            let expr = CraneliftI64Expr::Binary {
                op,
                lhs: Box::new(lower_i64_expr(
                    lhs,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )?),
                rhs: Box::new(lower_i64_expr(
                    rhs,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )?),
            };
            // Debug builds trap on sized-integer overflow before the wrapping
            // cast; release builds keep the wrapping behavior.
            let expr = match i64_sized_signed_overflow_bounds(ty) {
                Some((min, max, ty_name)) if i64_debug_build() => {
                    CraneliftI64Expr::CheckedSignedRange {
                        value: Box::new(expr),
                        min,
                        max,
                        message: format!(
                            "{{\"kind\":\"runtime\",\"message\":\"numeric overflow: {ty_name} {}\"}}",
                            i64_arithmetic_word(arith_op)
                        ),
                    }
                }
                _ => expr,
            };
            lower_i64_cast_expr(expr, ty)
        }
        Expr::Cast { expr, ty } if is_i64_compatible_type(ty) => {
            let expr = lower_i64_expr(
                expr,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            lower_i64_cast_expr(expr, ty)
        }
        Expr::TupleIndex { base, index, ty } if is_i64_compatible_type(ty) => {
            if let Expr::VarRef { name, .. } = base.as_ref() {
                return local_indexes
                    .get(i64_tuple_projection_key(name, *index).as_str())
                    .copied()
                    .map(CraneliftI64Expr::Local);
            }
            let Expr::TupleLiteral { elements, .. } = base.as_ref() else {
                return None;
            };
            let element = elements.get(*index)?;
            let expr = lower_i64_expr(
                element,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            lower_i64_cast_expr(expr, ty)
        }
        Expr::Index { base, index, ty } if is_i64_compatible_type(ty) => {
            if let Some(expr) = lower_i64_slice_projection_index_expr(
                base,
                index,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            ) {
                return lower_i64_cast_expr(expr, ty);
            }
            if let Expr::VarRef {
                name,
                ty: Type::Array(_, Some(size)),
            } = base.as_ref()
            {
                let expr = if let Some(index) = lower_i64_literal_index(index) {
                    local_indexes
                        .get(i64_array_projection_key(name, index).as_str())
                        .copied()
                        .map(CraneliftI64Expr::Local)?
                } else {
                    lower_i64_array_projection_index_expr(
                        name,
                        *size,
                        index,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )?
                };
                return lower_i64_cast_expr(expr, ty);
            }
            if let Some(expr) = lower_i64_array_literal_projection_index_expr(
                base,
                index,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            ) {
                return lower_i64_cast_expr(expr, ty);
            }
            let element = lower_i64_array_literal_element(base, index)?;
            let expr = lower_i64_expr(
                element,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            lower_i64_cast_expr(expr, ty)
        }
        Expr::FieldAccess { base, field, ty } if is_i64_compatible_type(ty) => {
            if let Expr::VarRef { name, .. } = base.as_ref() {
                return local_indexes
                    .get(i64_struct_projection_key(name, field).as_str())
                    .copied()
                    .map(CraneliftI64Expr::Local);
            }
            let element = lower_i64_struct_literal_field(base, field)?;
            let expr = lower_i64_expr(
                element,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            lower_i64_cast_expr(expr, ty)
        }
        Expr::Try { expr, .. } => lower_i64_try_value_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        ),
        _ => None,
    }
}

fn lower_i64_try_value_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    match expr {
        Expr::EnumVariant {
            enum_name,
            variant,
            payloads,
            ..
        } if (enum_name == "Option" && variant == "Some")
            || (enum_name == "Result" && variant == "Ok") =>
        {
            let [payload] = payloads.as_slice() else {
                return None;
            };
            lower_i64_expr(
                payload,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        _ => None,
    }
}

fn lower_i64_indexed_projection_locals(
    name: &str,
    elements: &[Expr],
    key_for_index: fn(&str, usize) -> String,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<()> {
    for (index, element) in elements.iter().enumerate() {
        let key = key_for_index(name, index);
        lower_i64_projection_local(
            key,
            element,
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?;
    }
    Some(())
}

fn lower_i64_struct_projection_locals(
    name: &str,
    fields: &[crate::mir::StructFieldValue],
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<()> {
    for field in fields {
        let key = i64_struct_projection_key(name, &field.name);
        lower_i64_projection_local(
            key,
            &field.expr,
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?;
    }
    Some(())
}

/// Record a mutable-slice binding as a true alias of its base array's
/// element slots. No locals are created and the local-index allocator is left
/// untouched; reads and writes through the binding resolve the base slots via
/// `static_bindings.mut_slice_aliases`. Only local variable bases with static
/// ranges are supported; call-produced temporaries stay on the copying path.
fn record_i64_mut_slice_alias(
    name: &str,
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    static_bindings: &mut I64StaticBindings,
) -> Option<()> {
    let Expr::Slice {
        base, start, end, ..
    } = expr
    else {
        return None;
    };
    let Expr::VarRef {
        name: base_name,
        ty: Type::Array(_, Some(base_size)),
    } = base.as_ref()
    else {
        return None;
    };
    let (start, end) = i64_static_slice_range(
        *base_size,
        start.as_deref(),
        end.as_deref(),
        static_bindings,
    )?;
    // The base may itself be a recorded alias; resolve one level so chained
    // views still target real projection locals.
    let (base_name, start) = match static_bindings.mut_slice_aliases.get(base_name.as_str()) {
        Some(alias) if end <= alias.len => (alias.base.clone(), alias.start + start),
        Some(_) => return None,
        None => (base_name.clone(), start),
    };
    local_indexes.get(i64_array_projection_key(&base_name, start).as_str())?;
    static_bindings.mut_slice_aliases.insert(
        name.to_string(),
        I64MutSliceAlias {
            base: base_name,
            start,
            len: end - start,
        },
    );
    Some(())
}

/// Resolve the projection locals a recorded mutable-slice alias points at.
fn i64_mut_slice_alias_slots(
    name: &str,
    local_indexes: &HashMap<String, usize>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<usize>> {
    let alias = static_bindings.mut_slice_aliases.get(name)?;
    let mut slots = Vec::new();
    for index in 0..alias.len {
        slots.push(
            local_indexes
                .get(i64_array_projection_key(&alias.base, alias.start + index).as_str())
                .copied()?,
        );
    }
    (!slots.is_empty()).then_some(slots)
}

fn lower_i64_slice_projection_aliases(
    name: &str,
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
    runtime: bool,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Expr::Slice {
        base, start, end, ..
    } = expr
    else {
        return None;
    };
    let mut assigns = Vec::new();
    let (base_name, base_size, copy_aliases) = match base.as_ref() {
        Expr::VarRef {
            name: base_name,
            ty: Type::Array(_, Some(base_size)),
        } => (base_name.clone(), *base_size, runtime),
        Expr::Call {
            name: call_name,
            args,
            ty: Type::Array(element, Some(base_size)),
        } if is_i64_array_param_element_type(element) => {
            let temp_name = format!("__axiom_i64_slice_base_{}", local_indexes.len());
            assigns.extend(lower_i64_array_call_let_stmts(
                &temp_name,
                element.as_ref(),
                *base_size,
                call_name,
                args,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?);
            (temp_name, *base_size, true)
        }
        _ => return None,
    };
    let (start, end) =
        i64_static_slice_range(base_size, start.as_deref(), end.as_deref(), static_bindings)?;
    for (slice_index, base_index) in (start..end).enumerate() {
        let base_key = i64_array_projection_key(&base_name, base_index);
        let base_local = *local_indexes.get(base_key.as_str())?;
        let local = locals.len();
        if copy_aliases {
            locals.push(CraneliftI64Expr::Literal(0));
            assigns.push(CraneliftI64Stmt::Assign(
                axiomc_backend_cranelift::I64Assign {
                    local,
                    value: CraneliftI64Expr::Local(base_local),
                },
            ));
        } else {
            locals.push(CraneliftI64Expr::Local(base_local));
        }
        let slice_key = i64_array_projection_key(name, slice_index);
        local_indexes.insert(slice_key.clone(), local);
        local_conditions.insert(slice_key, i64_local_truthy_condition(local));
    }
    Some(assigns)
}

fn lower_i64_projection_local(
    key: String,
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<()> {
    let value = match expr.ty() {
        Type::Bool => lower_i64_bool_value_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
        ty if is_i64_compatible_type(&ty) => lower_i64_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
        _ => return None,
    };
    let local = local_indexes.len();
    local_indexes.insert(key.clone(), local);
    locals.push(value);
    if matches!(expr.ty(), Type::Bool) {
        local_conditions.insert(
            key,
            CraneliftI64Condition::Compare(CraneliftI64Compare {
                op: CraneliftI64CompareOp::Ne,
                lhs: CraneliftI64Expr::Local(local),
                rhs: CraneliftI64Expr::Literal(0),
            }),
        );
    }
    Some(())
}

fn lower_i64_option_locals(
    name: &str,
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<()> {
    let Stmt::Let {
        ty: Type::Option(inner),
        expr:
            Expr::EnumVariant {
                enum_name,
                variant,
                payloads,
                ..
            },
        ..
    } = stmt
    else {
        return None;
    };
    if enum_name != "Option" {
        return None;
    }
    let payload_slot_count = i64_option_payload_slot_count_static(inner.as_ref(), static_bindings)?;
    let (tag, payloads) = match (variant.as_str(), payloads.as_slice()) {
        ("Some", [payload]) => (
            CraneliftI64Expr::Literal(1),
            lower_i64_option_payload_exprs(
                payload,
                payload_slot_count,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?,
        ),
        ("None", []) => (
            CraneliftI64Expr::Literal(0),
            vec![CraneliftI64Expr::Literal(0); payload_slot_count],
        ),
        _ => return None,
    };
    let tag_local = local_indexes.len();
    local_indexes.insert(i64_option_tag_key(name), tag_local);
    locals.push(tag);
    for (index, payload) in payloads.into_iter().enumerate() {
        let payload_local = local_indexes.len();
        local_indexes.insert(i64_option_payload_slot_key(name, index), payload_local);
        if index == 0 {
            local_indexes.insert(i64_option_payload_key(name), payload_local);
        }
        locals.push(payload);
    }
    Some(())
}

fn lower_i64_enum_locals(
    name: &str,
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<()> {
    let Stmt::Let {
        ty: Type::Enum(enum_name),
        expr:
            Expr::EnumVariant {
                enum_name: expr_enum,
                variant,
                payloads,
                ..
            },
        ..
    } = stmt
    else {
        return None;
    };
    if enum_name != expr_enum {
        return None;
    }
    let (tag, payloads) = lower_i64_enum_variant_parts(
        enum_name,
        variant,
        payloads,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let tag_local = local_indexes.len();
    local_indexes.insert(i64_enum_tag_key(name), tag_local);
    locals.push(tag);
    for (index, payload) in payloads.into_iter().enumerate() {
        let payload_local = local_indexes.len();
        local_indexes.insert(i64_enum_payload_slot_key(name, index), payload_local);
        if index == 0 {
            local_indexes.insert(i64_enum_payload_key(name), payload_local);
        }
        locals.push(payload);
    }
    Some(())
}

fn lower_i64_enum_variant_parts(
    enum_name: &str,
    variant: &str,
    payloads: &[Expr],
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<(CraneliftI64Expr, Vec<CraneliftI64Expr>)> {
    let enum_def = i64_scalar_enum_def(enum_name, static_bindings)?;
    let tag = enum_def
        .variants
        .iter()
        .position(|candidate| candidate.name == *variant)? as i64;
    let variant_def = enum_def
        .variants
        .iter()
        .find(|candidate| candidate.name == *variant)?;
    if variant_def.payload_tys.len() != payloads.len() {
        return None;
    }
    let mut lowered_payloads = payloads
        .iter()
        .map(|payload| {
            lower_i64_enum_payload_exprs(
                payload,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        })
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    lowered_payloads.resize(
        i64_enum_payload_slot_count(enum_name, static_bindings)?,
        CraneliftI64Expr::Literal(0),
    );
    Some((CraneliftI64Expr::Literal(tag), lowered_payloads))
}

fn lower_i64_enum_payload_exprs(
    payload: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Expr>> {
    match payload {
        Expr::TupleLiteral { elements, ty } => {
            let Type::Tuple(element_tys) = ty else {
                return None;
            };
            if !is_i64_tuple_param_type(element_tys) || elements.len() != element_tys.len() {
                return None;
            }
            elements
                .iter()
                .map(|element| {
                    lower_i64_option_payload_expr(
                        element,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )
                })
                .collect()
        }
        Expr::ArrayLiteral { elements, ty } => {
            let Type::Array(element_ty, Some(size)) = ty else {
                return None;
            };
            if elements.len() != *size || !is_i64_array_param_element_type(element_ty) {
                return None;
            }
            elements
                .iter()
                .map(|element| {
                    lower_i64_option_payload_expr(
                        element,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )
                })
                .collect()
        }
        Expr::StructLiteral {
            fields,
            ty: Type::Struct(name),
            ..
        } => {
            let struct_def = i64_scalar_static_struct_def(name, static_bindings)?;
            struct_def
                .fields
                .iter()
                .map(|field| {
                    let value = fields
                        .iter()
                        .find(|value| value.name == field.name)
                        .map(|value| &value.expr)?;
                    lower_i64_option_payload_expr(
                        value,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )
                })
                .collect()
        }
        Expr::VarRef {
            ty: Type::Option(inner),
            ..
        } if is_i64_option_local_payload_type_static(inner, static_bindings) => {
            lower_i64_option_call_arg_exprs(
                payload,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        Expr::EnumVariant {
            enum_name,
            ty: Type::Option(inner),
            ..
        } if enum_name == "Option"
            && is_i64_option_local_payload_type_static(inner, static_bindings) =>
        {
            lower_i64_option_call_arg_exprs(
                payload,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        Expr::VarRef {
            ty: Type::Result(ok, err),
            ..
        } if is_i64_result_local_payload_type_static(ok, err, static_bindings) => {
            lower_i64_result_call_arg_exprs(
                payload,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        Expr::EnumVariant {
            enum_name,
            ty: Type::Result(ok, err),
            ..
        } if enum_name == "Result"
            && is_i64_result_local_payload_type_static(ok, err, static_bindings) =>
        {
            lower_i64_result_call_arg_exprs(
                payload,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        _ => Some(vec![lower_i64_option_payload_expr(
            payload,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?]),
    }
}

fn lower_i64_result_locals(
    name: &str,
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<()> {
    let Stmt::Let {
        ty: Type::Result(ok, err),
        expr:
            Expr::EnumVariant {
                enum_name,
                variant,
                payloads,
                ..
            },
        ..
    } = stmt
    else {
        return None;
    };
    if enum_name != "Result" || !is_i64_result_local_payload_type_static(ok, err, static_bindings) {
        return None;
    }
    let payload_slot_count =
        i64_result_payload_slot_count_static(ok.as_ref(), static_bindings)?.max(
            i64_result_payload_slot_count_static(err.as_ref(), static_bindings)?,
        );
    let (tag, payloads) = lower_i64_result_variant_parts(
        variant,
        payloads,
        payload_slot_count,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let tag_local = local_indexes.len();
    local_indexes.insert(i64_result_tag_key(name), tag_local);
    locals.push(tag);
    for (index, payload) in payloads.into_iter().enumerate() {
        let payload_local = local_indexes.len();
        local_indexes.insert(i64_result_payload_slot_key(name, index), payload_local);
        if index == 0 {
            local_indexes.insert(i64_result_payload_key(name), payload_local);
        }
        locals.push(payload);
    }
    Some(())
}

fn lower_i64_result_variant_parts(
    variant: &str,
    payloads: &[Expr],
    payload_slot_count: usize,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<(CraneliftI64Expr, Vec<CraneliftI64Expr>)> {
    let (tag, payload) = match (variant, payloads) {
        ("Ok", [payload]) => (
            CraneliftI64Expr::Literal(1),
            lower_i64_result_payload_exprs(
                payload,
                payload_slot_count,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?,
        ),
        ("Err", [payload]) => (
            CraneliftI64Expr::Literal(0),
            lower_i64_result_payload_exprs(
                payload,
                payload_slot_count,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?,
        ),
        _ => return None,
    };
    Some((tag, payload))
}

fn lower_i64_result_payload_exprs(
    payload: &Expr,
    payload_slot_count: usize,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Expr>> {
    let mut payloads = match payload {
        Expr::TupleLiteral { elements, ty } => {
            let Type::Tuple(element_tys) = ty else {
                return None;
            };
            if !is_i64_tuple_param_type(element_tys) || elements.len() != element_tys.len() {
                return None;
            }
            elements
                .iter()
                .map(|element| {
                    lower_i64_option_payload_expr(
                        element,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )
                })
                .collect::<Option<Vec<_>>>()?
        }
        Expr::ArrayLiteral { elements, ty } => {
            let Type::Array(element_ty, Some(size)) = ty else {
                return None;
            };
            if elements.len() != *size || !is_i64_array_param_element_type(element_ty) {
                return None;
            }
            elements
                .iter()
                .map(|element| {
                    lower_i64_option_payload_expr(
                        element,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )
                })
                .collect::<Option<Vec<_>>>()?
        }
        Expr::VarRef {
            name,
            ty: Type::Array(element, Some(size)),
        } if is_i64_array_param_element_type(element) => (0..*size)
            .map(|index| {
                local_indexes
                    .get(i64_array_projection_key(name, index).as_str())
                    .copied()
                    .map(CraneliftI64Expr::Local)
            })
            .collect::<Option<Vec<_>>>()?,
        Expr::StructLiteral {
            fields,
            ty: Type::Struct(name),
            ..
        } => {
            let struct_def = i64_scalar_static_struct_def(name, static_bindings)?;
            struct_def
                .fields
                .iter()
                .map(|field| {
                    let value = fields
                        .iter()
                        .find(|value| value.name == field.name)
                        .map(|value| &value.expr)?;
                    lower_i64_option_payload_expr(
                        value,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )
                })
                .collect::<Option<Vec<_>>>()?
        }
        Expr::VarRef {
            name,
            ty: Type::Struct(struct_name),
        } => {
            let struct_def = i64_scalar_static_struct_def(struct_name, static_bindings)?;
            struct_def
                .fields
                .iter()
                .map(|field| {
                    local_indexes
                        .get(i64_struct_projection_key(name, &field.name).as_str())
                        .copied()
                        .map(CraneliftI64Expr::Local)
                })
                .collect::<Option<Vec<_>>>()?
        }
        Expr::VarRef {
            ty: Type::Option(inner),
            ..
        } if is_i64_option_local_payload_type_static(inner, static_bindings) => {
            lower_i64_option_call_arg_exprs(
                payload,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?
        }
        Expr::EnumVariant {
            enum_name,
            ty: Type::Option(inner),
            ..
        } if enum_name == "Option"
            && is_i64_option_local_payload_type_static(inner, static_bindings) =>
        {
            lower_i64_option_call_arg_exprs(
                payload,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?
        }
        Expr::VarRef {
            ty: Type::Result(ok, err),
            ..
        } if is_i64_result_local_payload_type_static(ok, err, static_bindings) => {
            lower_i64_result_call_arg_exprs(
                payload,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?
        }
        Expr::EnumVariant {
            enum_name,
            ty: Type::Result(ok, err),
            ..
        } if enum_name == "Result"
            && is_i64_result_local_payload_type_static(ok, err, static_bindings) =>
        {
            lower_i64_result_call_arg_exprs(
                payload,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?
        }
        _ => vec![lower_i64_option_payload_expr(
            payload,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?],
    };
    if payloads.len() > payload_slot_count {
        return None;
    }
    payloads.resize(payload_slot_count, CraneliftI64Expr::Literal(0));
    Some(payloads)
}

fn lower_i64_option_payload_exprs(
    payload: &Expr,
    payload_slot_count: usize,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Expr>> {
    let mut payloads = match payload {
        Expr::TupleLiteral { elements, ty } => {
            let Type::Tuple(element_tys) = ty else {
                return None;
            };
            if !is_i64_tuple_param_type(element_tys) || elements.len() != element_tys.len() {
                return None;
            }
            elements
                .iter()
                .map(|element| {
                    lower_i64_option_payload_expr(
                        element,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )
                })
                .collect::<Option<Vec<_>>>()?
        }
        Expr::ArrayLiteral { elements, ty } => {
            let Type::Array(element_ty, Some(size)) = ty else {
                return None;
            };
            if elements.len() != *size || !is_i64_array_param_element_type(element_ty) {
                return None;
            }
            elements
                .iter()
                .map(|element| {
                    lower_i64_option_payload_expr(
                        element,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )
                })
                .collect::<Option<Vec<_>>>()?
        }
        Expr::VarRef {
            name,
            ty: Type::Array(element, Some(size)),
        } if is_i64_array_param_element_type(element) => (0..*size)
            .map(|index| {
                local_indexes
                    .get(i64_array_projection_key(name, index).as_str())
                    .copied()
                    .map(CraneliftI64Expr::Local)
            })
            .collect::<Option<Vec<_>>>()?,
        Expr::StructLiteral {
            fields,
            ty: Type::Struct(name),
            ..
        } => {
            let struct_def = i64_scalar_static_struct_def(name, static_bindings)?;
            struct_def
                .fields
                .iter()
                .map(|field| {
                    let value = fields
                        .iter()
                        .find(|value| value.name == field.name)
                        .map(|value| &value.expr)?;
                    lower_i64_option_payload_expr(
                        value,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )
                })
                .collect::<Option<Vec<_>>>()?
        }
        Expr::VarRef {
            name,
            ty: Type::Struct(struct_name),
        } => {
            let struct_def = i64_scalar_static_struct_def(struct_name, static_bindings)?;
            struct_def
                .fields
                .iter()
                .map(|field| {
                    local_indexes
                        .get(i64_struct_projection_key(name, &field.name).as_str())
                        .copied()
                        .map(CraneliftI64Expr::Local)
                })
                .collect::<Option<Vec<_>>>()?
        }
        Expr::VarRef {
            ty: Type::Option(inner),
            ..
        } if is_i64_option_local_payload_type_static(inner, static_bindings) => {
            lower_i64_option_call_arg_exprs(
                payload,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?
        }
        Expr::EnumVariant {
            enum_name,
            ty: Type::Option(inner),
            ..
        } if enum_name == "Option"
            && is_i64_option_local_payload_type_static(inner, static_bindings) =>
        {
            lower_i64_option_call_arg_exprs(
                payload,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?
        }
        Expr::VarRef {
            ty: Type::Result(ok, err),
            ..
        } if is_i64_result_local_payload_type_static(ok, err, static_bindings) => {
            lower_i64_result_call_arg_exprs(
                payload,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?
        }
        Expr::EnumVariant {
            enum_name,
            ty: Type::Result(ok, err),
            ..
        } if enum_name == "Result"
            && is_i64_result_local_payload_type_static(ok, err, static_bindings) =>
        {
            lower_i64_result_call_arg_exprs(
                payload,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?
        }
        _ => vec![lower_i64_option_payload_expr(
            payload,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?],
    };
    if payloads.len() > payload_slot_count {
        return None;
    }
    payloads.resize(payload_slot_count, CraneliftI64Expr::Literal(0));
    Some(payloads)
}

fn lower_i64_option_payload_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    match expr.ty() {
        Type::Bool => lower_i64_bool_value_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        ),
        ty if is_i64_compatible_type(&ty) => lower_i64_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        ),
        _ => None,
    }
}

fn lower_i64_runtime_indexed_projection_let_stmts(
    name: &str,
    elements: &[Expr],
    key_for_index: fn(&str, usize) -> String,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let mut stmts = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        stmts.push(lower_i64_runtime_projection_assign(
            key_for_index(name, index),
            element,
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?);
    }
    Some(stmts)
}

fn lower_i64_runtime_struct_projection_let_stmts(
    name: &str,
    fields: &[crate::mir::StructFieldValue],
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let mut stmts = Vec::new();
    for field in fields {
        stmts.push(lower_i64_runtime_projection_assign(
            i64_struct_projection_key(name, &field.name),
            &field.expr,
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?);
    }
    Some(stmts)
}

fn lower_i64_runtime_projection_assign(
    key: String,
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Stmt> {
    let value = match expr.ty() {
        Type::Bool => lower_i64_bool_value_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
        ty if is_i64_compatible_type(&ty) => lower_i64_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?,
        _ => return None,
    };
    let local = local_indexes.len();
    local_indexes.insert(key.clone(), local);
    locals.push(CraneliftI64Expr::Literal(0));
    if matches!(expr.ty(), Type::Bool) {
        local_conditions.insert(
            key,
            CraneliftI64Condition::Compare(CraneliftI64Compare {
                op: CraneliftI64CompareOp::Ne,
                lhs: CraneliftI64Expr::Local(local),
                rhs: CraneliftI64Expr::Literal(0),
            }),
        );
    }
    Some(CraneliftI64Stmt::Assign(
        axiomc_backend_cranelift::I64Assign { local, value },
    ))
}

fn i64_struct_projection_key(name: &str, field: &str) -> String {
    format!("{name}.{field}")
}

fn i64_tuple_projection_key(name: &str, index: usize) -> String {
    format!("{name}.{index}")
}

fn i64_array_projection_key(name: &str, index: usize) -> String {
    format!("{name}[{index}]")
}

fn i64_string_len_key(name: &str) -> String {
    format!("{name}$len")
}

fn i64_option_tag_key(name: &str) -> String {
    format!("{name}?tag")
}

fn i64_option_payload_key(name: &str) -> String {
    format!("{name}?payload")
}

fn i64_option_payload_slot_key(name: &str, index: usize) -> String {
    if index == 0 {
        i64_option_payload_key(name)
    } else {
        format!("{name}?payload{index}")
    }
}

fn i64_option_payload_locals(
    name: &str,
    payload_ty: &Type,
    local_indexes: &HashMap<String, usize>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<usize>> {
    (0..i64_option_payload_slot_count_static(payload_ty, static_bindings)?)
        .map(|index| {
            local_indexes
                .get(i64_option_payload_slot_key(name, index).as_str())
                .copied()
        })
        .collect()
}

fn i64_string_len_option_local(
    name: &str,
    payload_ty: &Type,
    local_indexes: &HashMap<String, usize>,
) -> Option<Vec<usize>> {
    if !matches!(payload_ty, Type::String | Type::Str) {
        return None;
    }
    Some(vec![
        *local_indexes.get(i64_option_payload_slot_key(name, 0).as_str())?,
    ])
}

fn i64_enum_tag_key(name: &str) -> String {
    format!("{name}#tag")
}

fn i64_enum_payload_key(name: &str) -> String {
    i64_enum_payload_slot_key(name, 0)
}

fn i64_enum_payload_slot_key(name: &str, index: usize) -> String {
    format!("{name}#payload{index}")
}

fn i64_result_tag_key(name: &str) -> String {
    format!("{name}!tag")
}

fn i64_result_payload_key(name: &str) -> String {
    format!("{name}!payload")
}

fn i64_result_payload_slot_key(name: &str, index: usize) -> String {
    if index == 0 {
        i64_result_payload_key(name)
    } else {
        format!("{name}!payload{index}")
    }
}

fn i64_result_payload_locals(
    name: &str,
    ok: &Type,
    err: &Type,
    local_indexes: &HashMap<String, usize>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<usize>> {
    let slot_count = i64_result_payload_slot_count_static(ok, static_bindings)?
        .max(i64_result_payload_slot_count_static(err, static_bindings)?);
    (0..slot_count)
        .map(|index| {
            local_indexes
                .get(i64_result_payload_slot_key(name, index).as_str())
                .copied()
        })
        .collect()
}

fn lower_i64_struct_literal_field<'a>(base: &'a Expr, field: &str) -> Option<&'a Expr> {
    let Expr::StructLiteral { fields, .. } = base else {
        return None;
    };
    fields
        .iter()
        .find(|candidate| candidate.name == field)
        .map(|candidate| &candidate.expr)
}

fn lower_i64_array_literal_element<'a>(base: &'a Expr, index: &Expr) -> Option<&'a Expr> {
    let Expr::ArrayLiteral { elements, .. } = base else {
        return None;
    };
    let index = lower_i64_literal_index(index)?;
    elements.get(index)
}

fn lower_i64_array_projection_index_expr(
    name: &str,
    size: usize,
    index: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if size == 0 {
        return None;
    }
    let index = lower_i64_expr(
        index,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let last = size - 1;
    let mut result =
        CraneliftI64Expr::Local(*local_indexes.get(i64_array_projection_key(name, last).as_str())?);
    for candidate in (0..last).rev() {
        result = CraneliftI64Expr::Select {
            cond: Box::new(CraneliftI64Condition::Compare(CraneliftI64Compare {
                op: CraneliftI64CompareOp::Eq,
                lhs: index.clone(),
                rhs: CraneliftI64Expr::Literal(candidate as i64),
            })),
            then_result: Box::new(CraneliftI64Expr::Local(
                *local_indexes.get(i64_array_projection_key(name, candidate).as_str())?,
            )),
            else_result: Box::new(result),
        };
    }
    Some(result)
}

fn lower_i64_slice_projection_index_expr(
    base: &Expr,
    index: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if let Expr::VarRef {
        name,
        ty: Type::Slice(_) | Type::MutSlice(_),
    } = base
    {
        let elements =
            if let Some(slots) = i64_mut_slice_alias_slots(name, local_indexes, static_bindings) {
                slots.into_iter().map(CraneliftI64Expr::Local).collect()
            } else {
                lower_i64_slice_local_call_arg_exprs(name, local_indexes)?
            };
        if elements.is_empty() {
            return None;
        }
        if let Some(index) = lower_i64_literal_index(index) {
            return elements.get(index).cloned();
        }
        let index = lower_i64_expr(
            index,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?;
        let last = elements.len() - 1;
        let mut result = elements.get(last)?.clone();
        for candidate in (0..last).rev() {
            result = CraneliftI64Expr::Select {
                cond: Box::new(CraneliftI64Condition::Compare(CraneliftI64Compare {
                    op: CraneliftI64CompareOp::Eq,
                    lhs: index.clone(),
                    rhs: CraneliftI64Expr::Literal(candidate as i64),
                })),
                then_result: Box::new(elements.get(candidate)?.clone()),
                else_result: Box::new(result),
            };
        }
        return Some(result);
    }
    let (name, start, size) = i64_static_slice_base_range(base, static_bindings)?;
    if size == 0 {
        return None;
    }
    if let Some(index) = lower_i64_literal_index(index) {
        if index >= size {
            return None;
        }
        return local_indexes
            .get(i64_array_projection_key(name, start + index).as_str())
            .copied()
            .map(CraneliftI64Expr::Local);
    }
    let index = lower_i64_expr(
        index,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let last = size - 1;
    let mut result = CraneliftI64Expr::Local(
        *local_indexes.get(i64_array_projection_key(name, start + last).as_str())?,
    );
    for candidate in (0..last).rev() {
        result = CraneliftI64Expr::Select {
            cond: Box::new(CraneliftI64Condition::Compare(CraneliftI64Compare {
                op: CraneliftI64CompareOp::Eq,
                lhs: index.clone(),
                rhs: CraneliftI64Expr::Literal(candidate as i64),
            })),
            then_result: Box::new(CraneliftI64Expr::Local(
                *local_indexes.get(i64_array_projection_key(name, start + candidate).as_str())?,
            )),
            else_result: Box::new(result),
        };
    }
    Some(result)
}

fn lower_i64_array_literal_projection_index_expr(
    base: &Expr,
    index: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let Expr::ArrayLiteral { elements, ty } = base else {
        return None;
    };
    let Type::Array(element, Some(size)) = ty else {
        return None;
    };
    if elements.len() != *size || !is_i64_array_param_element_type(element) || *size == 0 {
        return None;
    }
    if let Some(index) = lower_i64_literal_index(index) {
        let element = elements.get(index)?;
        return lower_i64_expr(
            element,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )
        .or_else(|| {
            lower_i64_bool_argument_expr(
                element,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        });
    }
    let index = lower_i64_expr(
        index,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let last = elements.len() - 1;
    let mut result = lower_i64_expr(
        elements.get(last)?,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )
    .or_else(|| {
        lower_i64_bool_argument_expr(
            elements.get(last)?,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )
    })?;
    for candidate in (0..last).rev() {
        let element = lower_i64_expr(
            elements.get(candidate)?,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )
        .or_else(|| {
            lower_i64_bool_argument_expr(
                elements.get(candidate)?,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        })?;
        result = CraneliftI64Expr::Select {
            cond: Box::new(CraneliftI64Condition::Compare(CraneliftI64Compare {
                op: CraneliftI64CompareOp::Eq,
                lhs: index.clone(),
                rhs: CraneliftI64Expr::Literal(candidate as i64),
            })),
            then_result: Box::new(element),
            else_result: Box::new(result),
        };
    }
    Some(result)
}

fn i64_audited_ffi_expr(
    intrinsic: &str,
    library: &str,
    symbol: &str,
    arg_type: &str,
    result: CraneliftI64Expr,
    static_bindings: &I64StaticBindings,
    success: CraneliftI64AuditSuccess,
) -> Option<CraneliftI64Expr> {
    let package = static_bindings.package_root.as_deref()?;
    Some(CraneliftI64Expr::AuditFfi {
        intrinsic: intrinsic.to_string(),
        package: package.display().to_string(),
        library: library.to_string(),
        symbol: symbol.to_string(),
        arg_type: arg_type.to_string(),
        success,
        result: Box::new(result),
    })
}

fn lower_i64_ffi_intrinsic_expr(
    name: &str,
    args: &[Expr],
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if !static_bindings.ffi_strlen_symbols.contains(name) {
        return None;
    }
    let [value] = args else {
        return None;
    };
    if let Some(text) = i64_string_text(value, static_bindings) {
        return i64_audited_ffi_expr(
            "ffi_call",
            "c",
            "strlen",
            "string",
            CraneliftI64Expr::CStringLen { value: text },
            static_bindings,
            CraneliftI64AuditSuccess::NonNegative,
        );
    }
    let result = lower_i64_string_len_expr(
        value,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    i64_audited_ffi_expr(
        "ffi_call",
        "c",
        "strlen",
        "string",
        result,
        static_bindings,
        CraneliftI64AuditSuccess::NonNegative,
    )
}

fn lower_i64_map_get_or_default_expr(
    name: &str,
    args: &[Expr],
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if name != "get_or_default"
        && !static_bindings
            .collection_get_or_default_wrappers
            .contains(name)
    {
        return None;
    }
    let [map, key, default] = args else {
        return None;
    };
    let entries = i64_map_literal_entries(map, static_bindings)?;
    if let Some(key) = lower_i64_map_key_expr(key, static_bindings) {
        let mut selected = None;
        for entry in entries.iter().rev() {
            if lower_i64_map_key_expr(&entry.key, static_bindings)? == key {
                selected = Some(&entry.value);
                break;
            }
        }
        return lower_i64_expr(
            selected.unwrap_or(default),
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        );
    }
    let mut result = lower_i64_expr(
        default,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    for entry in entries {
        let candidate = lower_i64_map_key_expr(&entry.key, static_bindings)?;
        let cond = lower_i64_map_key_match_condition(
            key,
            &candidate,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?;
        let value = lower_i64_expr(
            &entry.value,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?;
        result = CraneliftI64Expr::Select {
            cond: Box::new(cond),
            then_result: Box::new(value),
            else_result: Box::new(result),
        };
    }
    Some(result)
}

fn lower_i64_dynamic_map_get_option_call_let_stmts(
    name: &str,
    inner: &Type,
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    if !is_i64_compatible_type(inner) && !matches!(inner, Type::Bool) {
        return None;
    }
    let Expr::Call {
        name: call_name,
        args,
        ..
    } = expr
    else {
        return None;
    };
    if call_name != "map_get"
        && call_name != "get"
        && !static_bindings.collection_get_wrappers.contains(call_name)
    {
        return None;
    }
    let [map, key] = args.as_slice() else {
        return None;
    };
    if lower_i64_map_key_expr(key, static_bindings).is_some() {
        return None;
    }
    if i64_option_payload_slot_count_static(inner, static_bindings)? != 1 {
        return None;
    }
    let entries = i64_map_literal_entries(map, static_bindings)?;
    let mut tag = CraneliftI64Condition::Literal(false);
    let mut payload = CraneliftI64Expr::Literal(0);
    for entry in entries {
        let candidate = lower_i64_map_key_expr(&entry.key, static_bindings)?;
        let cond = lower_i64_map_key_match_condition(
            key,
            &candidate,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?;
        let value = if matches!(inner, Type::Bool) {
            lower_i64_bool_value_expr(
                &entry.value,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?
        } else {
            lower_i64_expr(
                &entry.value,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?
        };
        tag = CraneliftI64Condition::Or {
            lhs: Box::new(cond.clone()),
            rhs: Box::new(tag),
        };
        payload = CraneliftI64Expr::Select {
            cond: Box::new(cond),
            then_result: Box::new(value),
            else_result: Box::new(payload),
        };
    }

    let tag_local = local_indexes.len();
    local_indexes.insert(i64_option_tag_key(name), tag_local);
    locals.push(CraneliftI64Expr::Literal(0));
    let payload_local = local_indexes.len();
    local_indexes.insert(i64_option_payload_slot_key(name, 0), payload_local);
    local_indexes.insert(i64_option_payload_key(name), payload_local);
    locals.push(CraneliftI64Expr::Literal(0));

    Some(vec![
        CraneliftI64Stmt::Assign(axiomc_backend_cranelift::I64Assign {
            local: tag_local,
            value: CraneliftI64Expr::ConditionValue(Box::new(tag)),
        }),
        CraneliftI64Stmt::Assign(axiomc_backend_cranelift::I64Assign {
            local: payload_local,
            value: payload,
        }),
    ])
}

fn i64_map_get_value_expr<'a>(
    name: &str,
    args: &'a [Expr],
    static_bindings: &'a I64StaticBindings,
) -> Option<Option<&'a Expr>> {
    if name != "map_get" && name != "get" && !static_bindings.collection_get_wrappers.contains(name)
    {
        return None;
    }
    let [map, key] = args else {
        return None;
    };
    let entries = i64_map_literal_entries(map, static_bindings)?;
    let key = lower_i64_map_key_expr(key, static_bindings)?;
    for entry in entries.iter().rev() {
        if lower_i64_map_key_expr(&entry.key, static_bindings)? == key {
            return Some(Some(&entry.value));
        }
    }
    Some(None)
}

fn lower_i64_map_contains_key_condition(
    name: &str,
    args: &[Expr],
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Condition> {
    if name != "map_contains_key"
        && name != "contains"
        && !static_bindings.collection_contains_wrappers.contains(name)
    {
        return None;
    }
    let [map, key] = args else {
        return None;
    };
    let entries = i64_map_literal_entries(map, static_bindings)?;
    if let Some(key) = lower_i64_map_key_expr(key, static_bindings) {
        for entry in entries.iter().rev() {
            if lower_i64_map_key_expr(&entry.key, static_bindings)? == key {
                return Some(CraneliftI64Condition::Literal(true));
            }
        }
        return Some(CraneliftI64Condition::Literal(false));
    }
    let mut result = CraneliftI64Condition::Literal(false);
    for entry in entries {
        let candidate = lower_i64_map_key_expr(&entry.key, static_bindings)?;
        let cond = lower_i64_map_key_match_condition(
            key,
            &candidate,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?;
        result = CraneliftI64Condition::Or {
            lhs: Box::new(cond),
            rhs: Box::new(result),
        };
    }
    Some(result)
}

fn lower_i64_map_key_match_condition(
    key: &Expr,
    candidate: &I64MapKey,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Condition> {
    if let Some(key) = lower_i64_map_key_expr(key, static_bindings) {
        return Some(CraneliftI64Condition::Literal(key == *candidate));
    }
    let I64MapKey::Text(candidate) = candidate else {
        return None;
    };
    let selected = lower_i64_map_key_array_string_index_match_expr(
        key,
        candidate,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    Some(CraneliftI64Condition::Compare(CraneliftI64Compare {
        op: CraneliftI64CompareOp::Eq,
        lhs: selected,
        rhs: CraneliftI64Expr::Literal(1),
    }))
}

fn lower_i64_map_keys_len_expr(
    expr: &Expr,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if let Expr::VarRef {
        name,
        ty: Type::Array(_, None),
    } = expr
    {
        return static_bindings
            .map_key_arrays
            .get(name)
            .map(|keys| CraneliftI64Expr::Literal(keys.len() as i64));
    }
    i64_map_keys_expr(expr, static_bindings)
        .map(|keys| CraneliftI64Expr::Literal(keys.len() as i64))
}

fn i64_map_keys_expr(expr: &Expr, static_bindings: &I64StaticBindings) -> Option<Vec<I64MapKey>> {
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if name != "map_keys"
        && name != "keys"
        && !static_bindings.collection_keys_wrappers.contains(name)
    {
        return None;
    }
    let [map] = args.as_slice() else {
        return None;
    };
    i64_map_unique_keys(map, static_bindings)
}

fn i64_map_unique_keys(map: &Expr, static_bindings: &I64StaticBindings) -> Option<Vec<I64MapKey>> {
    let entries = i64_map_literal_entries(map, static_bindings)?;
    let mut keys = Vec::new();
    for entry in entries.iter().rev() {
        let key = lower_i64_map_key_expr(&entry.key, static_bindings)?;
        if !keys.iter().any(|candidate| candidate == &key) {
            keys.push(key);
        }
    }
    keys.reverse();
    Some(keys)
}

fn lower_i64_map_key_expr(expr: &Expr, static_bindings: &I64StaticBindings) -> Option<I64MapKey> {
    if let Some(text) = i64_string_text(expr, static_bindings) {
        return Some(I64MapKey::Text(text));
    }
    if let Some(value) = i64_static_bool_value(expr, static_bindings) {
        return Some(I64MapKey::Bool(value));
    }
    i64_static_scalar_value(expr, static_bindings).map(I64MapKey::Int)
}

fn i64_map_literal_entries<'a>(
    map: &'a Expr,
    static_bindings: &'a I64StaticBindings,
) -> Option<&'a [MapEntry]> {
    match map {
        Expr::MapLiteral { entries, .. } => Some(entries.as_slice()),
        Expr::VarRef {
            name,
            ty: Type::Map(_, _),
        } => static_bindings
            .map_literals
            .get(name)
            .map(std::vec::Vec::as_slice),
        _ => None,
    }
}

fn is_i64_std_time_wrapper(function: &Function, source_name: &str) -> bool {
    function.path == "<stdlib>/time.ax" && function.source_name == source_name
}

fn is_i64_time_duration_ms_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.time_duration_ms_wrappers.contains(name)
}

fn is_i64_time_now_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.time_now_wrappers.contains(name)
}

fn is_i64_time_now_ms_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.time_now_ms_wrappers.contains(name)
}

fn is_i64_time_elapsed_ms_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.time_elapsed_ms_wrappers.contains(name)
}

fn is_i64_time_sleep_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.time_sleep_wrappers.contains(name)
}

fn is_i64_std_collection_wrapper(function: &Function, source_name: &str) -> bool {
    function.path == "<stdlib>/collections.ax" && function.source_name == source_name
}

fn is_i64_std_regex_wrapper(function: &Function, source_name: &str) -> bool {
    function.path == "<stdlib>/regex.ax" && function.source_name == source_name
}

fn is_i64_regex_is_match_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "regex_is_match" || static_bindings.regex_is_match_wrappers.contains(name)
}

fn is_i64_regex_find_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "regex_find" || static_bindings.regex_find_wrappers.contains(name)
}

fn is_i64_regex_replace_all_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "regex_replace_all" || static_bindings.regex_replace_all_wrappers.contains(name)
}

fn is_i64_std_encoding_wrapper(function: &Function, source_name: &str) -> bool {
    function.path == "<stdlib>/encoding.ax" && function.source_name == source_name
}

fn is_i64_encoding_percent_encode_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    matches!(
        name,
        "encoding_url_component_encode" | "encoding_path_segment_encode"
    ) || static_bindings
        .encoding_url_component_encode_wrappers
        .contains(name)
        || static_bindings
            .encoding_path_segment_encode_wrappers
            .contains(name)
}

fn is_i64_encoding_url_component_decode_name(
    name: &str,
    static_bindings: &I64StaticBindings,
) -> bool {
    name == "encoding_url_component_decode"
        || static_bindings
            .encoding_url_component_decode_wrappers
            .contains(name)
}

fn is_i64_encoding_url_query_pair_encode_name(
    name: &str,
    static_bindings: &I64StaticBindings,
) -> bool {
    name == "encoding_url_query_pair_encode"
        || static_bindings
            .encoding_url_query_pair_encode_wrappers
            .contains(name)
}

fn is_i64_encoding_path_join_segment_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "encoding_path_join_segment"
        || static_bindings
            .encoding_path_join_segment_wrappers
            .contains(name)
}

fn is_i64_std_log_wrapper(function: &Function, source_name: &str) -> bool {
    function.path == "<stdlib>/log.ax" && function.source_name == source_name
}

fn is_i64_std_io_wrapper(function: &Function, source_name: &str) -> bool {
    function.path == "<stdlib>/io.ax" && function.source_name == source_name
}

fn is_i64_io_eprintln_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "io_eprintln" || static_bindings.io_eprintln_wrappers.contains(name)
}

fn is_i64_io_readline_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "io_readline" || static_bindings.io_readline_wrappers.contains(name)
}

fn is_i64_io_read_to_string_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "io_read_to_string" || static_bindings.io_read_to_string_wrappers.contains(name)
}

fn is_i64_log_field_string_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.log_field_string_wrappers.contains(name)
}

fn is_i64_log_field_int_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.log_field_int_wrappers.contains(name)
}

fn is_i64_log_field_bool_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.log_field_bool_wrappers.contains(name)
}

fn is_i64_log_fields2_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.log_fields2_wrappers.contains(name)
}

fn is_i64_log_fields3_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.log_fields3_wrappers.contains(name)
}

fn is_i64_log_event_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.log_event_wrappers.contains(name)
}

fn is_i64_log_info_attrs_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.log_info_attrs_wrappers.contains(name)
}

fn i64_log_level_wrapper<'a>(
    name: &str,
    static_bindings: &'a I64StaticBindings,
) -> Option<&'a str> {
    static_bindings
        .log_level_wrappers
        .get(name)
        .map(String::as_str)
}

fn is_i64_std_string_builder_wrapper(function: &Function, source_name: &str) -> bool {
    function.path == "<stdlib>/string_builder.ax" && function.source_name == source_name
}

fn is_i64_string_builder_new_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.string_builder_new_wrappers.contains(name)
}

fn is_i64_string_builder_from_string_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings
        .string_builder_from_string_wrappers
        .contains(name)
}

fn is_i64_string_builder_push_str_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings
        .string_builder_push_str_wrappers
        .contains(name)
}

fn is_i64_string_builder_push_line_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings
        .string_builder_push_line_wrappers
        .contains(name)
}

fn is_i64_string_builder_finish_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings
        .string_builder_finish_wrappers
        .contains(name)
}

fn is_i64_string_builder_constructor_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    is_i64_string_builder_new_name(name, static_bindings)
        || is_i64_string_builder_from_string_name(name, static_bindings)
        || is_i64_string_builder_push_str_name(name, static_bindings)
        || is_i64_string_builder_push_line_name(name, static_bindings)
}

fn is_i64_supported_strlen_extern(function: &Function) -> bool {
    function.is_extern
        && function.source_name == "strlen"
        && function.extern_abi.as_deref() == Some("C")
        && function.extern_library.as_deref() == Some("c")
}

fn is_i64_std_sync_wrapper(function: &Function, source_name: &str) -> bool {
    function.path == "<stdlib>/sync.ax" && function.source_name == source_name
}

fn is_i64_sync_once_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.sync_once_wrappers.contains(name)
}

fn is_i64_sync_once_with_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.sync_once_with_wrappers.contains(name)
}

fn is_i64_sync_once_is_set_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.sync_once_is_set_wrappers.contains(name)
}

fn is_i64_sync_once_take_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.sync_once_take_wrappers.contains(name)
}

fn is_i64_sync_channel_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.sync_channel_wrappers.contains(name)
}

fn is_i64_sync_send_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.sync_send_wrappers.contains(name)
}

fn is_i64_sync_try_recv_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.sync_try_recv_wrappers.contains(name)
}

fn lower_i64_literal_value(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Literal(LiteralValue::Int(value)) => Some(*value),
        Expr::Literal(LiteralValue::Numeric { raw, ty }) => lower_i64_numeric_literal(raw, *ty),
        _ => None,
    }
}

fn lower_i64_fixed_array_intrinsic_expr(
    name: &str,
    args: &[Expr],
    ty: &Type,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let [arg] = args else {
        return None;
    };
    if name == "len" {
        if let Some(length) = lower_i64_string_len_expr(
            arg,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        ) {
            return Some(length);
        }
        if let Some(length) = lower_i64_crypto_random_bytes_len_expr(
            arg,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        ) {
            return Some(length);
        }
        if let Some(length) = lower_i64_map_keys_len_expr(arg, static_bindings) {
            return Some(length);
        }
    }
    let element = i64_fixed_array_or_slice_element(arg, static_bindings)?;
    if !is_i64_array_param_element_type(&element) {
        return None;
    }
    let elements = lower_i64_array_or_slice_call_arg_exprs(
        arg,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    match name {
        "len" => Some(CraneliftI64Expr::Literal(elements.len() as i64)),
        "first" | "last" => {
            if elements.is_empty() {
                return None;
            }
            let index = if name == "first" {
                0
            } else {
                elements.len() - 1
            };
            lower_i64_cast_expr(elements.get(index)?.clone(), ty)
        }
        _ => None,
    }
}

fn lower_i64_string_len_projection_local(
    name: &str,
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &mut I64StaticBindings,
) -> Option<()> {
    let text = i64_string_text(expr, static_bindings);
    let value = text
        .as_ref()
        .map(|text| CraneliftI64Expr::Literal(text.len() as i64))
        .or_else(|| {
            lower_i64_string_len_expr(
                expr,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        })?;
    let local = local_indexes.len();
    local_indexes.insert(i64_string_len_key(name), local);
    let json_safe = lower_i64_json_safe_string_len_expr(
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )
    .is_some();
    locals.push(value);
    if json_safe {
        let json_safe_local = local_indexes.len();
        local_indexes.insert(i64_json_safe_string_len_key(name), json_safe_local);
        locals.push(CraneliftI64Expr::Local(local));
    }
    lower_i64_printable_string_alias_local(
        name,
        expr,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    if let Some(text) = text {
        static_bindings.strings.insert(name.to_string(), text);
    }
    if let Some(binding) = i64_map_key_array_string_index_binding(expr, static_bindings) {
        static_bindings
            .map_key_array_string_indexes
            .insert(name.to_string(), binding);
    }
    Some(())
}

fn lower_i64_printable_string_alias_local(
    name: &str,
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<()> {
    let Some((key, value, is_bool)) = lower_i64_printable_string_alias_parts(
        name,
        expr,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) else {
        return Some(());
    };
    let local = local_indexes.len();
    local_indexes.insert(key.clone(), local);
    locals.push(value);
    if is_bool {
        local_conditions.insert(key, i64_local_truthy_condition(local));
    }
    Some(())
}

fn lower_i64_printable_string_alias_parts(
    name: &str,
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<(String, CraneliftI64Expr, bool)> {
    let Expr::Call {
        name: call_name,
        args,
        ..
    } = expr
    else {
        return None;
    };
    if is_i64_json_stringify_int_name(call_name, static_bindings) {
        let [value] = args.as_slice() else {
            return None;
        };
        return Some((
            i64_printable_i64_string_key(name),
            lower_i64_expr(
                value,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?,
            false,
        ));
    }
    if is_i64_json_stringify_bool_name(call_name, static_bindings) {
        let [value] = args.as_slice() else {
            return None;
        };
        return Some((
            i64_printable_bool_string_key(name),
            lower_i64_bool_value_expr(
                value,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?,
            true,
        ));
    }
    None
}

fn lower_i64_string_len_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let crypto_len_call = matches!(
        expr,
        Expr::Call { name, .. }
            if is_i64_crypto_sha256_name(name, static_bindings)
                || is_i64_crypto_hmac_sha256_name(name, static_bindings)
                || is_i64_crypto_hmac_sha512_name(name, static_bindings)
    );
    if !crypto_len_call {
        if let Some(value) = i64_string_text(expr, static_bindings) {
            return Some(CraneliftI64Expr::Literal(value.len() as i64));
        }
    }
    if let Expr::VarRef {
        name,
        ty: Type::String | Type::Str,
    } = expr
        && let Some(value) = static_bindings.values.get(name)
    {
        return Some(value.clone());
    }
    match expr {
        Expr::Call { name, args, .. } if is_i64_crypto_sha256_name(name, static_bindings) => {
            let [input] = args.as_slice() else {
                return None;
            };
            let _ = lower_i64_string_len_expr(
                input,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            i64_audited_crypto_expr(
                "crypto_sha256",
                "inputs",
                "strings:1".to_string(),
                CraneliftI64Expr::Literal(64),
                static_bindings,
                CraneliftI64AuditSuccess::NonNegative,
            )
        }
        Expr::Call { name, args, .. } if is_i64_crypto_hmac_sha256_name(name, static_bindings) => {
            let [key, message] = args.as_slice() else {
                return None;
            };
            let _ = lower_i64_string_len_expr(
                key,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            let _ = lower_i64_string_len_expr(
                message,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            i64_audited_crypto_expr(
                "crypto_hmac_sha256",
                "inputs",
                "strings:2".to_string(),
                CraneliftI64Expr::Literal(64),
                static_bindings,
                CraneliftI64AuditSuccess::NonNegative,
            )
        }
        Expr::Call { name, args, .. } if is_i64_crypto_hmac_sha512_name(name, static_bindings) => {
            let [key, message] = args.as_slice() else {
                return None;
            };
            let _ = lower_i64_string_len_expr(
                key,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            let _ = lower_i64_string_len_expr(
                message,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            i64_audited_crypto_expr(
                "crypto_hmac_sha512",
                "inputs",
                "strings:2".to_string(),
                CraneliftI64Expr::Literal(128),
                static_bindings,
                CraneliftI64AuditSuccess::NonNegative,
            )
        }
        Expr::Call { name, args, .. } if is_i64_io_read_to_string_name(name, static_bindings) => {
            if !args.is_empty() {
                return None;
            }
            Some(CraneliftI64Expr::StdinLen {
                max_bytes: I64_STDIN_BUFFER_BYTES,
            })
        }
        Expr::BinaryAdd {
            op: ArithmeticOp::Add,
            lhs,
            rhs,
            ty: Type::String | Type::Str,
        } => Some(CraneliftI64Expr::Binary {
            op: CraneliftI64BinaryOp::Add,
            lhs: Box::new(lower_i64_string_len_expr(
                lhs,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?),
            rhs: Box::new(lower_i64_string_len_expr(
                rhs,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?),
        }),
        Expr::Call { name, args, .. } if name == "string_clone" => {
            let [text] = args.as_slice() else {
                return None;
            };
            lower_i64_string_len_expr(
                text,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        }
        Expr::Call {
            name,
            args,
            ty: Type::String | Type::Str,
        } if name == "string_trim" || name == "string_trim_start" => {
            let [_text] = args.as_slice() else {
                return None;
            };
            lower_i64_map_key_array_string_index_mapped_i64_expr(
                expr,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
                |value| value.len() as i64,
            )
        }
        Expr::Call { name, args, .. } if is_i64_json_stringify_int_name(name, static_bindings) => {
            let [value] = args.as_slice() else {
                return None;
            };
            let value = lower_i64_expr(
                value,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            Some(i64_decimal_string_len_expr(value))
        }
        Expr::Call { name, args, .. } if is_i64_json_stringify_bool_name(name, static_bindings) => {
            let [value] = args.as_slice() else {
                return None;
            };
            Some(CraneliftI64Expr::Select {
                cond: Box::new(lower_i64_condition(
                    value,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )?),
                then_result: Box::new(CraneliftI64Expr::Literal(4)),
                else_result: Box::new(CraneliftI64Expr::Literal(5)),
            })
        }
        Expr::Call { name, args, .. }
            if is_i64_json_stringify_string_name(name, static_bindings) =>
        {
            let [text] = args.as_slice() else {
                return None;
            };
            Some(CraneliftI64Expr::Binary {
                op: CraneliftI64BinaryOp::Add,
                lhs: Box::new(lower_i64_json_safe_string_len_expr(
                    text,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )?),
                rhs: Box::new(CraneliftI64Expr::Literal(2)),
            })
        }
        Expr::Call { name, args, .. } if is_i64_log_field_string_name(name, static_bindings) => {
            let [key, value] = args.as_slice() else {
                return None;
            };
            let key_len =
                i64::try_from(json_escape_string(&i64_string_text(key, static_bindings)?).len())
                    .ok()?;
            Some(CraneliftI64Expr::Binary {
                op: CraneliftI64BinaryOp::Add,
                lhs: Box::new(CraneliftI64Expr::Literal(key_len + 1)),
                rhs: Box::new(lower_i64_json_escaped_string_len_expr(
                    value,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )?),
            })
        }
        Expr::Call { name, args, .. } if is_i64_log_field_int_name(name, static_bindings) => {
            let [key, value] = args.as_slice() else {
                return None;
            };
            let key_len =
                i64::try_from(json_escape_string(&i64_string_text(key, static_bindings)?).len())
                    .ok()?;
            Some(CraneliftI64Expr::Binary {
                op: CraneliftI64BinaryOp::Add,
                lhs: Box::new(CraneliftI64Expr::Literal(key_len + 1)),
                rhs: Box::new(i64_decimal_string_len_expr(lower_i64_expr(
                    value,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )?)),
            })
        }
        Expr::Call { name, args, .. } if is_i64_log_field_bool_name(name, static_bindings) => {
            let [key, value] = args.as_slice() else {
                return None;
            };
            let key_len =
                i64::try_from(json_escape_string(&i64_string_text(key, static_bindings)?).len())
                    .ok()?;
            Some(CraneliftI64Expr::Binary {
                op: CraneliftI64BinaryOp::Add,
                lhs: Box::new(CraneliftI64Expr::Literal(key_len + 1)),
                rhs: Box::new(CraneliftI64Expr::Select {
                    cond: Box::new(lower_i64_condition(
                        value,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )?),
                    then_result: Box::new(CraneliftI64Expr::Literal(4)),
                    else_result: Box::new(CraneliftI64Expr::Literal(5)),
                }),
            })
        }
        Expr::Call { name, args, .. } if is_i64_log_fields2_name(name, static_bindings) => {
            let [first, second] = args.as_slice() else {
                return None;
            };
            Some(CraneliftI64Expr::Binary {
                op: CraneliftI64BinaryOp::Add,
                lhs: Box::new(CraneliftI64Expr::Binary {
                    op: CraneliftI64BinaryOp::Add,
                    lhs: Box::new(lower_i64_string_len_expr(
                        first,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )?),
                    rhs: Box::new(CraneliftI64Expr::Literal(1)),
                }),
                rhs: Box::new(lower_i64_string_len_expr(
                    second,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )?),
            })
        }
        Expr::Call { name, args, .. } if is_i64_log_fields3_name(name, static_bindings) => {
            let [first, second, third] = args.as_slice() else {
                return None;
            };
            Some(CraneliftI64Expr::Binary {
                op: CraneliftI64BinaryOp::Add,
                lhs: Box::new(CraneliftI64Expr::Binary {
                    op: CraneliftI64BinaryOp::Add,
                    lhs: Box::new(lower_i64_string_len_expr(
                        first,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )?),
                    rhs: Box::new(CraneliftI64Expr::Literal(1)),
                }),
                rhs: Box::new(CraneliftI64Expr::Binary {
                    op: CraneliftI64BinaryOp::Add,
                    lhs: Box::new(lower_i64_string_len_expr(
                        second,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )?),
                    rhs: Box::new(CraneliftI64Expr::Binary {
                        op: CraneliftI64BinaryOp::Add,
                        lhs: Box::new(CraneliftI64Expr::Literal(1)),
                        rhs: Box::new(lower_i64_string_len_expr(
                            third,
                            local_indexes,
                            local_conditions,
                            helper_signatures,
                            static_bindings,
                        )?),
                    }),
                }),
            })
        }
        Expr::Call { name, args, .. } if is_i64_log_event_name(name, static_bindings) => {
            let [level, message, attributes] = args.as_slice() else {
                return None;
            };
            let prefix_len = i64::try_from("{\"level\":".len()).ok()?
                + i64::try_from(
                    json_escape_string(&i64_string_text(level, static_bindings)?).len(),
                )
                .ok()?
                + i64::try_from(",\"message\":".len()).ok()?;
            let suffix_len =
                i64::try_from(",\"attributes\":{".len()).ok()? + i64::try_from("}}".len()).ok()?;
            let attributes_len = lower_i64_string_len_expr(
                attributes,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            let message_len = lower_i64_json_escaped_string_len_expr(
                message,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            Some(CraneliftI64Expr::Binary {
                op: CraneliftI64BinaryOp::Add,
                lhs: Box::new(CraneliftI64Expr::Binary {
                    op: CraneliftI64BinaryOp::Add,
                    lhs: Box::new(CraneliftI64Expr::Literal(prefix_len + suffix_len)),
                    rhs: Box::new(attributes_len),
                }),
                rhs: Box::new(message_len),
            })
        }
        Expr::Literal(LiteralValue::String(_)) | Expr::Literal(LiteralValue::Str(_)) => {
            lower_i64_string_literal_len_expr(expr)
        }
        Expr::VarRef {
            name,
            ty: Type::String | Type::Str,
        } => local_indexes
            .get(i64_string_len_key(name).as_str())
            .copied()
            .map(CraneliftI64Expr::Local)
            .or_else(|| {
                static_bindings
                    .strings
                    .get(name)
                    .map(|value| CraneliftI64Expr::Literal(value.len() as i64))
            }),
        Expr::Index {
            base,
            index,
            ty: Type::String | Type::Str,
        } => lower_i64_map_key_array_string_index_len_expr(
            base,
            index,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        ),
        Expr::StringBorrow { expr, .. } => lower_i64_string_len_expr(
            expr,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        ),
        _ => i64_string_text(expr, static_bindings)
            .map(|value| CraneliftI64Expr::Literal(value.len() as i64)),
    }
}

fn lower_i64_map_key_array_string_index_len_expr(
    base: &Expr,
    index: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let Expr::VarRef {
        name,
        ty: Type::Array(element, None),
    } = base
    else {
        return None;
    };
    if !matches!(element.as_ref(), Type::String | Type::Str) {
        return None;
    }
    let keys = static_bindings.map_key_arrays.get(name)?;
    if keys.is_empty() {
        return None;
    }
    if let Some(index) = lower_i64_literal_index(index) {
        return match keys.get(index)? {
            I64MapKey::Text(value) => Some(CraneliftI64Expr::Literal(value.len() as i64)),
            _ => None,
        };
    }
    let index = lower_i64_expr(
        index,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    let last = keys.len() - 1;
    let mut result = match keys.get(last)? {
        I64MapKey::Text(value) => CraneliftI64Expr::Literal(value.len() as i64),
        _ => return None,
    };
    for candidate in (0..last).rev() {
        let length = match keys.get(candidate)? {
            I64MapKey::Text(value) => value.len() as i64,
            _ => return None,
        };
        result = CraneliftI64Expr::Select {
            cond: Box::new(CraneliftI64Condition::Compare(CraneliftI64Compare {
                op: CraneliftI64CompareOp::Eq,
                lhs: index.clone(),
                rhs: CraneliftI64Expr::Literal(candidate as i64),
            })),
            then_result: Box::new(CraneliftI64Expr::Literal(length)),
            else_result: Box::new(result),
        };
    }
    Some(result)
}

fn i64_decimal_string_len_expr(value: CraneliftI64Expr) -> CraneliftI64Expr {
    let positive = i64_positive_decimal_string_len_expr(value.clone());
    let negative = i64_negative_decimal_string_len_expr(value.clone());
    CraneliftI64Expr::Select {
        cond: Box::new(CraneliftI64Condition::Compare(CraneliftI64Compare {
            op: CraneliftI64CompareOp::Ge,
            lhs: value,
            rhs: CraneliftI64Expr::Literal(0),
        })),
        then_result: Box::new(positive),
        else_result: Box::new(negative),
    }
}

fn i64_positive_decimal_string_len_expr(value: CraneliftI64Expr) -> CraneliftI64Expr {
    let mut result = CraneliftI64Expr::Literal(19);
    let thresholds = [
        9_i64,
        99,
        999,
        9_999,
        99_999,
        999_999,
        9_999_999,
        99_999_999,
        999_999_999,
        9_999_999_999,
        99_999_999_999,
        999_999_999_999,
        9_999_999_999_999,
        99_999_999_999_999,
        999_999_999_999_999,
        9_999_999_999_999_999,
        99_999_999_999_999_999,
        999_999_999_999_999_999,
    ];
    for (index, threshold) in thresholds.into_iter().enumerate().rev() {
        result = CraneliftI64Expr::Select {
            cond: Box::new(CraneliftI64Condition::Compare(CraneliftI64Compare {
                op: CraneliftI64CompareOp::Le,
                lhs: value.clone(),
                rhs: CraneliftI64Expr::Literal(threshold),
            })),
            then_result: Box::new(CraneliftI64Expr::Literal(index as i64 + 1)),
            else_result: Box::new(result),
        };
    }
    result
}

fn i64_negative_decimal_string_len_expr(value: CraneliftI64Expr) -> CraneliftI64Expr {
    let mut result = CraneliftI64Expr::Literal(20);
    let thresholds = [
        -9_i64,
        -99,
        -999,
        -9_999,
        -99_999,
        -999_999,
        -9_999_999,
        -99_999_999,
        -999_999_999,
        -9_999_999_999,
        -99_999_999_999,
        -999_999_999_999,
        -9_999_999_999_999,
        -99_999_999_999_999,
        -999_999_999_999_999,
        -9_999_999_999_999_999,
        -99_999_999_999_999_999,
        -999_999_999_999_999_999,
    ];
    for (index, threshold) in thresholds.into_iter().enumerate().rev() {
        result = CraneliftI64Expr::Select {
            cond: Box::new(CraneliftI64Condition::Compare(CraneliftI64Compare {
                op: CraneliftI64CompareOp::Ge,
                lhs: value.clone(),
                rhs: CraneliftI64Expr::Literal(threshold),
            })),
            then_result: Box::new(CraneliftI64Expr::Literal(index as i64 + 2)),
            else_result: Box::new(result),
        };
    }
    result
}

fn lower_i64_string_literal_len_expr(expr: &Expr) -> Option<CraneliftI64Expr> {
    match expr {
        Expr::Literal(LiteralValue::String(value)) | Expr::Literal(LiteralValue::Str(value)) => {
            Some(CraneliftI64Expr::Literal(value.len() as i64))
        }
        _ => None,
    }
}

fn lower_i64_fixed_array_bool_intrinsic_expr(
    name: &str,
    args: &[Expr],
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let [arg] = args else {
        return None;
    };
    let element = i64_fixed_array_or_slice_element(arg, static_bindings)?;
    if !matches!(element, Type::Bool) {
        return None;
    }
    let elements = lower_i64_array_or_slice_call_arg_exprs(
        arg,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    if elements.is_empty() {
        return None;
    }
    let index = match name {
        "first" => 0,
        "last" => elements.len() - 1,
        _ => return None,
    };
    elements.get(index).cloned()
}

fn i64_fixed_array_or_slice_element(
    arg: &Expr,
    static_bindings: &I64StaticBindings,
) -> Option<Type> {
    match arg {
        Expr::Slice { base, .. } => {
            let (_, _, _) = i64_static_slice_base_range(arg, static_bindings)?;
            let Expr::VarRef {
                ty: Type::Array(element, Some(_)),
                ..
            } = base.as_ref()
            else {
                return None;
            };
            Some(element.as_ref().clone())
        }
        Expr::VarRef {
            ty: Type::Slice(element) | Type::MutSlice(element),
            ..
        } => Some(element.as_ref().clone()),
        Expr::ArrayLiteral { ty, .. } | Expr::VarRef { ty, .. } => {
            let Type::Array(element, Some(_)) = ty else {
                return None;
            };
            Some(element.as_ref().clone())
        }
        _ => {
            let Type::Array(element, Some(size)) = arg.ty() else {
                return None;
            };
            let _ = size;
            Some(element.as_ref().clone())
        }
    }
}

fn i64_static_slice_base_range<'a>(
    arg: &'a Expr,
    static_bindings: &I64StaticBindings,
) -> Option<(&'a str, usize, usize)> {
    let Expr::Slice {
        base, start, end, ..
    } = arg
    else {
        return None;
    };
    let Expr::VarRef {
        name,
        ty: Type::Array(_, Some(base_size)),
    } = base.as_ref()
    else {
        return None;
    };
    let (start, end) = i64_static_slice_range(
        *base_size,
        start.as_deref(),
        end.as_deref(),
        static_bindings,
    )?;
    Some((name.as_str(), start, end - start))
}

fn i64_static_slice_range(
    base_size: usize,
    start: Option<&Expr>,
    end: Option<&Expr>,
    static_bindings: &I64StaticBindings,
) -> Option<(usize, usize)> {
    let start = match start {
        Some(expr) => lower_i64_static_index(expr, static_bindings)?,
        None => 0,
    };
    let end = match end {
        Some(expr) => lower_i64_static_index(expr, static_bindings)?,
        None => base_size,
    };
    (start <= end && end <= base_size).then_some((start, end))
}

fn lower_i64_static_index(expr: &Expr, static_bindings: &I64StaticBindings) -> Option<usize> {
    let value = i64_static_scalar_value(expr, static_bindings)?;
    usize::try_from(value).ok()
}

fn lower_i64_literal_index(expr: &Expr) -> Option<usize> {
    match expr {
        Expr::Literal(LiteralValue::Int(value)) => usize::try_from(*value).ok(),
        Expr::Literal(LiteralValue::Numeric { raw, ty }) => {
            let value = lower_i64_numeric_literal(raw, *ty)?;
            usize::try_from(value).ok()
        }
        _ => None,
    }
}

fn lower_i64_call_expr(
    name: &str,
    args: &[Expr],
    expect_bool_return: bool,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let signature = helper_signatures.get(name)?;
    if signature.returns_bool != expect_bool_return {
        return None;
    }
    if args.len() != signature.params {
        return None;
    }
    let mut lowered_args = Vec::new();
    for (index, arg) in args.iter().enumerate() {
        let struct_fields = signature
            .struct_fields
            .get(index)
            .and_then(|fields| fields.as_deref());
        lowered_args.extend(lower_i64_call_arg_exprs(
            arg,
            struct_fields,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?);
    }
    Some(CraneliftI64Expr::Call {
        function: signature.function,
        args: lowered_args,
    })
}

fn lower_i64_call_arg_exprs(
    arg: &Expr,
    struct_fields: Option<&[String]>,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Expr>> {
    if let Some(option_args) = lower_i64_option_call_arg_exprs(
        arg,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(option_args);
    }
    if let Some(result_args) = lower_i64_result_call_arg_exprs(
        arg,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(result_args);
    }
    if let Some(enum_args) = lower_i64_enum_call_arg_exprs(
        arg,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(enum_args);
    }
    if let Some(struct_args) = lower_i64_struct_call_arg_exprs(
        arg,
        struct_fields,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(struct_args);
    }
    if let Some(tuple_args) = lower_i64_tuple_call_arg_exprs(
        arg,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(tuple_args);
    }
    if let Some(array_args) = lower_i64_array_call_arg_exprs(
        arg,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(array_args);
    }
    Some(vec![
        lower_i64_expr(
            arg,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )
        .or_else(|| {
            lower_i64_bool_argument_expr(
                arg,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )
        })?,
    ])
}

fn lower_i64_option_call_arg_exprs(
    arg: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Expr>> {
    match arg {
        Expr::VarRef {
            name,
            ty: Type::Option(inner),
        } if is_i64_option_local_payload_type_static(inner, static_bindings) => {
            let mut args = vec![CraneliftI64Expr::Local(
                *local_indexes.get(i64_option_tag_key(name).as_str())?,
            )];
            args.extend(
                i64_option_payload_locals(name, inner.as_ref(), local_indexes, static_bindings)?
                    .into_iter()
                    .map(CraneliftI64Expr::Local),
            );
            Some(args)
        }
        Expr::EnumVariant {
            enum_name,
            variant,
            payloads,
            ty: Type::Option(inner),
            ..
        } if enum_name == "Option"
            && is_i64_option_local_payload_type_static(inner, static_bindings) =>
        {
            let payload_slot_count =
                i64_option_payload_slot_count_static(inner.as_ref(), static_bindings)?;
            let (tag, payloads) = match (variant.as_str(), payloads.as_slice()) {
                ("Some", [payload]) => (
                    CraneliftI64Expr::Literal(1),
                    lower_i64_option_payload_exprs(
                        payload,
                        payload_slot_count,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )?,
                ),
                ("None", []) => (
                    CraneliftI64Expr::Literal(0),
                    vec![CraneliftI64Expr::Literal(0); payload_slot_count],
                ),
                _ => return None,
            };
            let mut args = vec![tag];
            args.extend(payloads);
            Some(args)
        }
        _ => None,
    }
}

fn lower_i64_enum_call_arg_exprs(
    arg: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Expr>> {
    match arg {
        Expr::VarRef {
            name,
            ty: Type::Enum(enum_name),
        } if is_i64_enum_payload_type(enum_name, static_bindings) => {
            let mut args = vec![CraneliftI64Expr::Local(
                *local_indexes.get(i64_enum_tag_key(name).as_str())?,
            )];
            args.extend(
                i64_enum_payload_locals(name, enum_name, static_bindings, local_indexes)?
                    .into_iter()
                    .map(CraneliftI64Expr::Local),
            );
            Some(args)
        }
        Expr::EnumVariant {
            enum_name,
            variant,
            payloads,
            ty: Type::Enum(ty_enum),
            ..
        } if enum_name == ty_enum && is_i64_enum_payload_type(enum_name, static_bindings) => {
            let (tag, payloads) = lower_i64_enum_variant_parts(
                enum_name,
                variant,
                payloads,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            let mut args = vec![tag];
            args.extend(payloads);
            Some(args)
        }
        _ => None,
    }
}

fn lower_i64_result_call_arg_exprs(
    arg: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Expr>> {
    match arg {
        Expr::VarRef {
            name,
            ty: Type::Result(ok, err),
        } if is_i64_result_local_payload_type_static(ok, err, static_bindings) => {
            let mut args = vec![CraneliftI64Expr::Local(
                *local_indexes.get(i64_result_tag_key(name).as_str())?,
            )];
            args.extend(
                i64_result_payload_locals(
                    name,
                    ok.as_ref(),
                    err.as_ref(),
                    local_indexes,
                    static_bindings,
                )?
                .into_iter()
                .map(CraneliftI64Expr::Local),
            );
            Some(args)
        }
        Expr::EnumVariant {
            enum_name,
            variant,
            payloads,
            ty: Type::Result(ok, err),
            ..
        } if enum_name == "Result"
            && is_i64_result_local_payload_type_static(ok, err, static_bindings) =>
        {
            let payload_slot_count =
                i64_result_payload_slot_count_static(ok.as_ref(), static_bindings)?.max(
                    i64_result_payload_slot_count_static(err.as_ref(), static_bindings)?,
                );
            let (tag, payload) = lower_i64_result_variant_parts(
                variant,
                payloads,
                payload_slot_count,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?;
            let mut args = vec![tag];
            args.extend(payload);
            Some(args)
        }
        _ => None,
    }
}

fn lower_i64_struct_call_arg_exprs(
    arg: &Expr,
    struct_fields: Option<&[String]>,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Expr>> {
    let struct_fields = struct_fields?;
    match arg {
        Expr::VarRef {
            name,
            ty: Type::Struct(_),
        } => lower_i64_local_struct_call_arg_exprs(name, struct_fields, local_indexes),
        Expr::StructLiteral {
            fields,
            ty: Type::Struct(_),
            ..
        } => struct_fields
            .iter()
            .map(|field_name| {
                let field = fields.iter().find(|field| field.name == *field_name)?;
                if !is_i64_struct_field_type(&field.expr.ty()) {
                    return None;
                }
                lower_i64_expr(
                    &field.expr,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )
                .or_else(|| {
                    lower_i64_bool_argument_expr(
                        &field.expr,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )
                })
            })
            .collect(),
        _ => None,
    }
}

fn lower_i64_local_struct_call_arg_exprs(
    name: &str,
    struct_fields: &[String],
    local_indexes: &HashMap<String, usize>,
) -> Option<Vec<CraneliftI64Expr>> {
    if struct_fields.is_empty() {
        return None;
    }
    struct_fields
        .iter()
        .map(|field| {
            local_indexes
                .get(i64_struct_projection_key(name, field).as_str())
                .copied()
                .map(CraneliftI64Expr::Local)
        })
        .collect()
}

fn lower_i64_tuple_call_arg_exprs(
    arg: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Expr>> {
    match arg {
        Expr::VarRef {
            name,
            ty: Type::Tuple(elements),
        } if is_i64_tuple_param_type(elements) => (0..elements.len())
            .map(|index| {
                local_indexes
                    .get(i64_tuple_projection_key(name, index).as_str())
                    .copied()
                    .map(CraneliftI64Expr::Local)
            })
            .collect(),
        Expr::TupleLiteral {
            elements,
            ty: Type::Tuple(element_tys),
        } if elements.len() == element_tys.len() && is_i64_tuple_param_type(element_tys) => {
            elements
                .iter()
                .map(|element| {
                    lower_i64_expr(
                        element,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )
                    .or_else(|| {
                        lower_i64_bool_argument_expr(
                            element,
                            local_indexes,
                            local_conditions,
                            helper_signatures,
                            static_bindings,
                        )
                    })
                })
                .collect()
        }
        _ => None,
    }
}

fn lower_i64_array_call_arg_exprs(
    arg: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Expr>> {
    match arg {
        Expr::VarRef {
            name,
            ty: Type::Array(element, Some(size)),
        } if is_i64_array_param_element_type(element) => (0..*size)
            .map(|index| {
                local_indexes
                    .get(i64_array_projection_key(name, index).as_str())
                    .copied()
                    .map(CraneliftI64Expr::Local)
            })
            .collect(),
        Expr::ArrayLiteral {
            elements,
            ty: Type::Array(element, Some(size)),
        } if elements.len() == *size && is_i64_array_param_element_type(element) => elements
            .iter()
            .map(|element| {
                lower_i64_expr(
                    element,
                    local_indexes,
                    local_conditions,
                    helper_signatures,
                    static_bindings,
                )
                .or_else(|| {
                    lower_i64_bool_argument_expr(
                        element,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )
                })
            })
            .collect(),
        _ => None,
    }
}

fn lower_i64_array_or_slice_call_arg_exprs(
    arg: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Expr>> {
    if let Some(values) = lower_i64_array_call_arg_exprs(
        arg,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    ) {
        return Some(values);
    }
    if let Expr::VarRef {
        name,
        ty: Type::Slice(_) | Type::MutSlice(_),
    } = arg
    {
        return lower_i64_slice_local_call_arg_exprs(name, local_indexes);
    }
    let Expr::Slice {
        base, start, end, ..
    } = arg
    else {
        return None;
    };
    let Expr::VarRef {
        name,
        ty: Type::Array(element, Some(base_size)),
    } = base.as_ref()
    else {
        return None;
    };
    if !is_i64_array_param_element_type(element) {
        return None;
    }
    let (start, end) = i64_static_slice_range(
        *base_size,
        start.as_deref(),
        end.as_deref(),
        static_bindings,
    )?;
    (start..end)
        .map(|index| {
            local_indexes
                .get(i64_array_projection_key(name, index).as_str())
                .copied()
                .map(CraneliftI64Expr::Local)
        })
        .collect()
}

fn lower_i64_slice_local_call_arg_exprs(
    name: &str,
    local_indexes: &HashMap<String, usize>,
) -> Option<Vec<CraneliftI64Expr>> {
    let mut values = Vec::new();
    for index in 0.. {
        let Some(local) = local_indexes
            .get(i64_array_projection_key(name, index).as_str())
            .copied()
        else {
            break;
        };
        values.push(CraneliftI64Expr::Local(local));
    }
    (!values.is_empty()).then_some(values)
}

fn lower_i64_bool_argument_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    match expr {
        Expr::Literal(LiteralValue::Bool(value)) => {
            Some(CraneliftI64Expr::Literal(i64::from(*value)))
        }
        Expr::VarRef {
            name,
            ty: Type::Bool,
        } => {
            if let Some(CraneliftI64Condition::Literal(value)) =
                static_bindings.conditions.get(name)
            {
                Some(CraneliftI64Expr::Literal(i64::from(*value)))
            } else {
                Some(CraneliftI64Expr::ConditionValue(Box::new(
                    lower_i64_condition(
                        expr,
                        local_indexes,
                        local_conditions,
                        helper_signatures,
                        static_bindings,
                    )?,
                )))
            }
        }
        _ => Some(CraneliftI64Expr::ConditionValue(Box::new(
            lower_i64_condition(
                expr,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?,
        ))),
    }
}

fn lower_i64_static_bindings(statics: &[StaticDef]) -> Option<I64StaticBindings> {
    let local_indexes = HashMap::new();
    let local_conditions = HashMap::new();
    let helper_signatures = HashMap::<&str, I64HelperSignature>::new();
    let mut static_bindings = I64StaticBindings::default();
    for static_def in statics {
        match static_def.ty {
            Type::Int | Type::Numeric(NumericType::I64) => {
                let value = lower_i64_expr(
                    &static_def.expr,
                    &local_indexes,
                    &local_conditions,
                    &helper_signatures,
                    &static_bindings,
                )?;
                static_bindings
                    .values
                    .insert(static_def.name.clone(), value);
            }
            Type::Numeric(numeric_ty) if !numeric_ty.is_float() => {
                let value = lower_i64_expr(
                    &static_def.expr,
                    &local_indexes,
                    &local_conditions,
                    &helper_signatures,
                    &static_bindings,
                )?;
                static_bindings
                    .values
                    .insert(static_def.name.clone(), value);
            }
            Type::Bool => {
                let condition = lower_i64_condition(
                    &static_def.expr,
                    &local_indexes,
                    &local_conditions,
                    &helper_signatures,
                    &static_bindings,
                )?;
                static_bindings
                    .conditions
                    .insert(static_def.name.clone(), condition);
            }
            Type::String | Type::Str => {
                let value = i64_string_text(&static_def.expr, &static_bindings)?;
                static_bindings
                    .strings
                    .insert(static_def.name.clone(), value);
            }
            _ => return None,
        }
    }
    Some(static_bindings)
}

fn is_i64_compatible_type(ty: &Type) -> bool {
    matches!(ty, Type::Int)
        || matches!(
            ty,
            Type::Numeric(
                NumericType::I8
                    | NumericType::I16
                    | NumericType::I32
                    | NumericType::I64
                    | NumericType::Isize
                    | NumericType::U8
                    | NumericType::U16
                    | NumericType::U32
            )
        )
}

fn is_i64_option_payload_type(ty: &Type) -> bool {
    is_i64_compatible_type(ty) || matches!(ty, Type::Bool)
}

fn is_i64_option_local_payload_type(ty: &Type) -> bool {
    is_i64_option_payload_type(ty)
        || matches!(ty, Type::Tuple(elements) if is_i64_tuple_param_type(elements))
        || matches!(ty, Type::Array(element, Some(_)) if is_i64_array_param_element_type(element))
}

fn i64_option_payload_slot_count(ty: &Type) -> usize {
    match ty {
        Type::Tuple(elements) if is_i64_tuple_param_type(elements) => elements.len(),
        Type::Array(element, Some(size)) if is_i64_array_param_element_type(element) => *size,
        _ => 1,
    }
}

fn is_i64_option_local_payload_type_static(ty: &Type, static_bindings: &I64StaticBindings) -> bool {
    is_i64_option_local_payload_type(ty)
        || matches!(ty, Type::Option(inner) if is_i64_option_local_payload_type_static(inner, static_bindings))
        || matches!(ty, Type::Result(ok, err) if is_i64_result_local_payload_type_static(ok, err, static_bindings))
        || matches!(ty, Type::Struct(name) if i64_scalar_static_struct_def(name, static_bindings).is_some())
}

fn i64_option_payload_slot_count_static(
    ty: &Type,
    static_bindings: &I64StaticBindings,
) -> Option<usize> {
    match ty {
        Type::Struct(name) => Some(
            i64_scalar_static_struct_def(name, static_bindings)?
                .fields
                .len(),
        ),
        Type::Option(inner) if is_i64_option_local_payload_type_static(inner, static_bindings) => {
            Some(1 + i64_option_payload_slot_count_static(inner.as_ref(), static_bindings)?)
        }
        Type::Result(ok, err)
            if is_i64_result_local_payload_type_static(ok, err, static_bindings) =>
        {
            Some(
                1 + i64_result_payload_slot_count_static(ok.as_ref(), static_bindings)?.max(
                    i64_result_payload_slot_count_static(err.as_ref(), static_bindings)?,
                ),
            )
        }
        ty if is_i64_option_local_payload_type(ty) => Some(i64_option_payload_slot_count(ty)),
        _ => None,
    }
}

fn is_i64_result_local_payload_variant_type(ty: &Type) -> bool {
    is_i64_option_payload_type(ty)
        || matches!(ty, Type::Tuple(elements) if is_i64_tuple_param_type(elements))
        || matches!(ty, Type::Array(element, Some(_)) if is_i64_array_param_element_type(element))
}

fn i64_result_payload_slot_count(ty: &Type) -> usize {
    match ty {
        Type::Tuple(elements) if is_i64_tuple_param_type(elements) => elements.len(),
        Type::Array(element, Some(size)) if is_i64_array_param_element_type(element) => *size,
        _ => 1,
    }
}

fn is_i64_result_local_payload_type_static(
    ok: &Type,
    err: &Type,
    static_bindings: &I64StaticBindings,
) -> bool {
    is_i64_result_local_payload_variant_type_static(ok, static_bindings)
        && is_i64_result_local_payload_variant_type_static(err, static_bindings)
}

fn is_i64_result_local_payload_variant_type_static(
    ty: &Type,
    static_bindings: &I64StaticBindings,
) -> bool {
    is_i64_result_local_payload_variant_type(ty)
        || matches!(ty, Type::Option(inner) if is_i64_option_local_payload_type_static(inner, static_bindings))
        || matches!(ty, Type::Result(ok, err) if is_i64_result_local_payload_type_static(ok, err, static_bindings))
        || matches!(ty, Type::Struct(name) if i64_scalar_static_struct_def(name, static_bindings).is_some())
}

fn i64_result_payload_slot_count_static(
    ty: &Type,
    static_bindings: &I64StaticBindings,
) -> Option<usize> {
    match ty {
        Type::Struct(name) => Some(
            i64_scalar_static_struct_def(name, static_bindings)?
                .fields
                .len(),
        ),
        Type::Option(inner) if is_i64_option_local_payload_type_static(inner, static_bindings) => {
            Some(1 + i64_option_payload_slot_count_static(inner.as_ref(), static_bindings)?)
        }
        Type::Result(ok, err)
            if is_i64_result_local_payload_type_static(ok, err, static_bindings) =>
        {
            Some(
                1 + i64_result_payload_slot_count_static(ok.as_ref(), static_bindings)?.max(
                    i64_result_payload_slot_count_static(err.as_ref(), static_bindings)?,
                ),
            )
        }
        ty if is_i64_result_local_payload_variant_type(ty) => {
            Some(i64_result_payload_slot_count(ty))
        }
        _ => None,
    }
}

fn lower_i64_cast(ty: &Type) -> Option<CraneliftI64Cast> {
    match ty {
        Type::Int => Some(CraneliftI64Cast::Signed64),
        Type::Numeric(NumericType::I8) => Some(CraneliftI64Cast::Signed8),
        Type::Numeric(NumericType::I16) => Some(CraneliftI64Cast::Signed16),
        Type::Numeric(NumericType::I32) => Some(CraneliftI64Cast::Signed32),
        Type::Numeric(NumericType::I64 | NumericType::Isize) => Some(CraneliftI64Cast::Signed64),
        Type::Numeric(NumericType::U8) => Some(CraneliftI64Cast::Unsigned8),
        Type::Numeric(NumericType::U16) => Some(CraneliftI64Cast::Unsigned16),
        Type::Numeric(NumericType::U32) => Some(CraneliftI64Cast::Unsigned32),
        _ => None,
    }
}

fn lower_i64_cast_expr(expr: CraneliftI64Expr, ty: &Type) -> Option<CraneliftI64Expr> {
    let cast = lower_i64_cast(ty)?;
    match cast {
        CraneliftI64Cast::Signed64 => Some(expr),
        _ => Some(CraneliftI64Expr::Cast {
            cast,
            expr: Box::new(expr),
        }),
    }
}

fn is_i64_exit_type(ty: &Type) -> bool {
    is_i64_compatible_type(ty) || matches!(ty, Type::Bool)
}

fn is_i64_function_return_type(
    ty: &Type,
    struct_defs: &I64StructDefs<'_>,
    static_bindings: &I64StaticBindings,
) -> bool {
    is_i64_exit_type(ty)
        || matches!(ty, Type::Tuple(elements) if is_i64_tuple_param_type(elements))
        || matches!(ty, Type::Array(element, Some(_)) if is_i64_array_param_element_type(element))
        || matches!(ty, Type::Struct(name) if i64_scalar_struct_def(name, struct_defs).is_some())
        || matches!(ty, Type::Option(inner) if is_i64_option_local_payload_type_static(inner, static_bindings))
        || matches!(ty, Type::Result(ok, err) if is_i64_result_local_payload_type_static(ok, err, static_bindings))
        || matches!(ty, Type::Enum(enum_name) if is_i64_enum_payload_type(enum_name, static_bindings))
}

fn i64_return_slot_count_for_type(
    ty: &Type,
    struct_defs: &I64StructDefs<'_>,
    static_bindings: &I64StaticBindings,
) -> Option<usize> {
    match ty {
        ty if is_i64_exit_type(ty) => Some(1),
        Type::Array(element, Some(size)) if is_i64_array_param_element_type(element) => Some(*size),
        Type::Tuple(elements) if is_i64_tuple_param_type(elements) => Some(elements.len()),
        Type::Struct(name) => Some(i64_scalar_struct_def(name, struct_defs)?.fields.len()),
        Type::Option(inner) if is_i64_option_local_payload_type_static(inner, static_bindings) => {
            Some(1 + i64_option_payload_slot_count_static(inner.as_ref(), static_bindings)?)
        }
        Type::Result(ok, err)
            if is_i64_result_local_payload_type_static(ok, err, static_bindings) =>
        {
            Some(
                1 + i64_result_payload_slot_count_static(ok.as_ref(), static_bindings)?.max(
                    i64_result_payload_slot_count_static(err.as_ref(), static_bindings)?,
                ),
            )
        }
        Type::Enum(enum_name) if is_i64_enum_payload_type(enum_name, static_bindings) => {
            Some(1 + i64_enum_payload_slot_count(enum_name, static_bindings)?)
        }
        _ => None,
    }
}

fn is_i64_param_type(
    ty: &Type,
    struct_defs: &I64StructDefs<'_>,
    static_bindings: &I64StaticBindings,
) -> bool {
    is_i64_compatible_type(ty)
        || matches!(ty, Type::Bool)
        || matches!(ty, Type::Struct(name) if i64_scalar_struct_def(name, struct_defs).is_some())
        || matches!(ty, Type::Tuple(elements) if is_i64_tuple_param_type(elements))
        || matches!(ty, Type::Array(element, Some(_)) if is_i64_array_param_element_type(element))
        || matches!(ty, Type::Option(inner) if is_i64_option_local_payload_type_static(inner, static_bindings))
        || matches!(ty, Type::Result(ok, err) if is_i64_result_local_payload_type_static(ok, err, static_bindings))
        || matches!(ty, Type::Enum(enum_name) if is_i64_enum_payload_type(enum_name, static_bindings))
}

fn i64_scalar_struct_def<'a>(
    name: &str,
    struct_defs: &'a I64StructDefs<'a>,
) -> Option<&'a StructDef> {
    let struct_def = *struct_defs.get(name)?;
    if struct_def
        .fields
        .iter()
        .all(|field| is_i64_struct_field_type(&field.ty))
    {
        Some(struct_def)
    } else {
        None
    }
}

fn i64_scalar_static_struct_def<'a>(
    name: &str,
    static_bindings: &'a I64StaticBindings,
) -> Option<&'a StructDef> {
    let struct_def = static_bindings.structs.get(name)?;
    if struct_def
        .fields
        .iter()
        .all(|field| is_i64_struct_field_type(&field.ty))
    {
        Some(struct_def)
    } else {
        None
    }
}

fn is_i64_struct_field_type(ty: &Type) -> bool {
    is_i64_compatible_type(ty) || matches!(ty, Type::Bool)
}

fn is_i64_tuple_param_type(elements: &[Type]) -> bool {
    elements.iter().all(is_i64_tuple_param_element_type)
}

fn is_i64_tuple_param_element_type(ty: &Type) -> bool {
    is_i64_compatible_type(ty) || matches!(ty, Type::Bool)
}

fn is_i64_array_param_element_type(ty: &Type) -> bool {
    is_i64_compatible_type(ty) || matches!(ty, Type::Bool)
}

fn is_i64_enum_payload_type(enum_name: &str, static_bindings: &I64StaticBindings) -> bool {
    i64_scalar_enum_def(enum_name, static_bindings).is_some()
}

fn i64_enum_payload_slot_count(
    enum_name: &str,
    static_bindings: &I64StaticBindings,
) -> Option<usize> {
    i64_scalar_enum_def(enum_name, static_bindings).map(|enum_def| {
        enum_def
            .variants
            .iter()
            .map(|variant| {
                variant
                    .payload_tys
                    .iter()
                    .map(|ty| i64_enum_payload_variant_slot_count(ty, static_bindings))
                    .try_fold(0usize, |total, count| Some(total + count?))
                    .unwrap_or(0)
            })
            .max()
            .unwrap_or(0)
    })
}

fn i64_enum_payload_variant_slot_count(
    ty: &Type,
    static_bindings: &I64StaticBindings,
) -> Option<usize> {
    match ty {
        Type::Tuple(elements) if is_i64_tuple_param_type(elements) => Some(elements.len()),
        Type::Struct(name) => Some(
            i64_scalar_static_struct_def(name, static_bindings)?
                .fields
                .len(),
        ),
        Type::Option(inner) if is_i64_option_local_payload_type_static(inner, static_bindings) => {
            Some(1 + i64_option_payload_slot_count_static(inner.as_ref(), static_bindings)?)
        }
        Type::Result(ok, err)
            if is_i64_result_local_payload_type_static(ok, err, static_bindings) =>
        {
            Some(
                1 + i64_result_payload_slot_count_static(ok.as_ref(), static_bindings)?.max(
                    i64_result_payload_slot_count_static(err.as_ref(), static_bindings)?,
                ),
            )
        }
        _ => Some(1),
    }
}

fn i64_enum_payload_locals(
    name: &str,
    enum_name: &str,
    static_bindings: &I64StaticBindings,
    local_indexes: &HashMap<String, usize>,
) -> Option<Vec<usize>> {
    (0..i64_enum_payload_slot_count(enum_name, static_bindings)?)
        .map(|index| {
            local_indexes
                .get(i64_enum_payload_slot_key(name, index).as_str())
                .copied()
        })
        .collect()
}

fn i64_scalar_enum_def<'a>(
    enum_name: &str,
    static_bindings: &'a I64StaticBindings,
) -> Option<&'a EnumDef> {
    let enum_def = static_bindings.enums.get(enum_name)?;
    if enum_def.variants.iter().all(|variant| {
        variant
            .payload_tys
            .iter()
            .all(|ty| is_i64_enum_payload_variant_type(ty, static_bindings))
    }) {
        Some(enum_def)
    } else {
        None
    }
}

fn is_i64_enum_payload_variant_type(ty: &Type, static_bindings: &I64StaticBindings) -> bool {
    is_i64_compatible_type(ty)
        || matches!(ty, Type::Bool)
        || matches!(ty, Type::Tuple(elements) if is_i64_tuple_param_type(elements))
        || matches!(ty, Type::Struct(name) if i64_scalar_static_struct_def(name, static_bindings).is_some())
        || matches!(ty, Type::Option(inner) if is_i64_option_local_payload_type_static(inner, static_bindings))
        || matches!(ty, Type::Result(ok, err) if is_i64_result_local_payload_type_static(ok, err, static_bindings))
}

fn i64_abi_param_count(
    params: &[crate::mir::Param],
    struct_defs: &I64StructDefs<'_>,
    static_bindings: &I64StaticBindings,
) -> Option<usize> {
    params
        .iter()
        .map(|param| i64_abi_param_count_for_type(&param.ty, struct_defs, static_bindings))
        .try_fold(0usize, |total, count| Some(total + count?))
}

fn i64_abi_param_count_for_type(
    ty: &Type,
    struct_defs: &I64StructDefs<'_>,
    static_bindings: &I64StaticBindings,
) -> Option<usize> {
    match ty {
        ty if is_i64_compatible_type(ty) || matches!(ty, Type::Bool) => Some(1),
        Type::Struct(name) => Some(i64_scalar_struct_def(name, struct_defs)?.fields.len()),
        Type::Tuple(elements) if is_i64_tuple_param_type(elements) => Some(elements.len()),
        Type::Array(element, Some(size)) if is_i64_array_param_element_type(element) => Some(*size),
        Type::Option(inner) if is_i64_option_local_payload_type_static(inner, static_bindings) => {
            Some(1 + i64_option_payload_slot_count_static(inner.as_ref(), static_bindings)?)
        }
        Type::Result(ok, err)
            if is_i64_result_local_payload_type_static(ok, err, static_bindings) =>
        {
            Some(
                1 + i64_result_payload_slot_count_static(ok.as_ref(), static_bindings)?.max(
                    i64_result_payload_slot_count_static(err.as_ref(), static_bindings)?,
                ),
            )
        }
        Type::Enum(enum_name) if is_i64_enum_payload_type(enum_name, static_bindings) => {
            Some(1 + i64_enum_payload_slot_count(enum_name, static_bindings)?)
        }
        _ => None,
    }
}

fn lower_i64_numeric_literal(raw: &str, ty: NumericType) -> Option<i64> {
    let value = match ty {
        NumericType::F32 | NumericType::F64 => return None,
        NumericType::I8
        | NumericType::I16
        | NumericType::I32
        | NumericType::I64
        | NumericType::Isize => {
            let value = raw.parse::<i64>().ok()?;
            cast_signed_integer(value, ty)
        }
        NumericType::U8
        | NumericType::U16
        | NumericType::U32
        | NumericType::U64
        | NumericType::Usize => {
            let value = raw.parse::<u128>().ok()?;
            let value = u64::try_from(value).ok()?;
            cast_unsigned_integer(value, ty)
        }
    };
    match value {
        SpikeValue::Int(value) => Some(value),
        SpikeValue::UInt(value) => Some(value as i64),
        SpikeValue::Float(_) => None,
        _ => None,
    }
}

#[cfg(test)]
fn collect_output_lines(
    program: &Program,
    capabilities: &CapabilityConfig,
    _package_root: &Path,
    fs_root: &Path,
    stdin: Option<&str>,
) -> Result<Vec<OutputLine>, Diagnostic> {
    collect_output_program(program, capabilities, _package_root, fs_root, stdin)
        .map(|output| output.lines)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StaticOutputProgram {
    lines: Vec<OutputLine>,
    exit_code: i32,
}

fn collect_output_program(
    program: &Program,
    capabilities: &CapabilityConfig,
    package_root: &Path,
    fs_root: &Path,
    stdin: Option<&str>,
) -> Result<StaticOutputProgram, Diagnostic> {
    with_spike_stdin(stdin, || {
        let functions = program
            .functions
            .iter()
            .map(|function| (function.name.as_str(), function))
            .collect::<HashMap<_, _>>();
        let mut env = SpikeEnv::new();
        env.insert(
            SPIKE_PACKAGE_ROOT_BINDING.to_string(),
            SpikeValue::Text(package_root.display().to_string()),
        );
        env.insert(
            SPIKE_FS_ROOT_BINDING.to_string(),
            SpikeValue::Text(fs_root.display().to_string()),
        );
        env.insert(
            SPIKE_ENV_ALLOWLIST_BINDING.to_string(),
            SpikeValue::Array(
                capabilities
                    .env_vars
                    .iter()
                    .cloned()
                    .map(SpikeValue::Text)
                    .collect(),
            ),
        );
        env.insert(
            SPIKE_ENV_UNRESTRICTED_BINDING.to_string(),
            SpikeValue::Bool(capabilities.env_unrestricted),
        );
        let mut lines = Vec::new();
        let result = (|| {
            for static_def in &program.statics {
                let value = eval_expr(&static_def.expr, &functions, &env, &mut lines)?;
                env.insert(static_def.name.clone(), value);
            }
            run_function_body(&program.stmts, &functions, &mut env, &mut lines)
        })();
        match result {
            Ok(_) => Ok(StaticOutputProgram {
                lines,
                exit_code: 0,
            }),
            Err(diagnostic) if is_cranelift_runtime_trap(&diagnostic) => {
                lines.push(OutputLine::stderr(runtime_trap_text(&diagnostic)));
                Ok(StaticOutputProgram {
                    lines,
                    exit_code: 1,
                })
            }
            Err(diagnostic) => Err(diagnostic),
        }
    })
}

pub(crate) struct MatchedEnum {
    enum_name: String,
    variant: String,
    field_names: Vec<String>,
    payloads: Vec<SpikeValue>,
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

/// Wait out a task's deferred (virtual) duration. Async bodies evaluate with
/// virtual sleeps, so the time is spent when the task is consumed: for real
/// when awaited at top level, or added to the enclosing task's accumulator
/// when awaited inside another async body.
fn spike_wait_out_duration(duration_ms: i64) {
    if duration_ms <= 0 {
        return;
    }
    let deferred = SPIKE_VIRTUAL_SLEEP.with(|slot| match slot.get() {
        Some(accumulated) => {
            slot.set(Some(accumulated.saturating_add(duration_ms)));
            true
        }
        None => false,
    });
    if !deferred {
        std::thread::sleep(std::time::Duration::from_millis(duration_ms as u64));
    }
}

/// Extract a task's value without spending its deferred duration. Callers that
/// model the wait separately (join's group deadline, timeout's bounded wait)
/// use this instead of `await_spike_task`.
fn spike_task_value(value: SpikeValue) -> Result<SpikeValue, Diagnostic> {
    match value {
        SpikeValue::Task {
            value: Some(value),
            canceled: false,
            ..
        } => Ok(*value),
        SpikeValue::Task { canceled: true, .. } => Err(unsupported("awaited task was canceled")),
        SpikeValue::Task { value: None, .. } => {
            Err(unsupported("task had no value or scheduled body"))
        }
        _ => Err(unsupported("await expects a task")),
    }
}

fn await_spike_task(value: SpikeValue) -> Result<SpikeValue, Diagnostic> {
    if let SpikeValue::Task {
        canceled: false,
        duration_ms,
        ..
    } = &value
    {
        spike_wait_out_duration(*duration_ms);
    }
    spike_task_value(value)
}

fn json_serdes_parse_value(text: &str, index: usize) -> Result<(SpikeValue, usize), String> {
    let index = json_skip_ws(text, index);
    match text.as_bytes().get(index).copied() {
        Some(b'n') if text[index..].starts_with("null") => {
            Ok((json_serdes_value_variant("Null", Vec::new()), index + 4))
        }
        Some(b't') if text[index..].starts_with("true") => Ok((
            json_serdes_value_variant("Bool", vec![SpikeValue::Bool(true)]),
            index + 4,
        )),
        Some(b'f') if text[index..].starts_with("false") => Ok((
            json_serdes_value_variant("Bool", vec![SpikeValue::Bool(false)]),
            index + 5,
        )),
        Some(b'"') => {
            let end = json_scan_string_end(text, index)
                .ok_or_else(|| String::from("unterminated JSON string"))?;
            let value = json_parse_string(&text[index..end])
                .ok_or_else(|| String::from("invalid JSON string"))?;
            Ok((
                json_serdes_value_variant("Text", vec![SpikeValue::Text(value)]),
                end,
            ))
        }
        Some(b'[') => json_serdes_parse_array(text, index),
        Some(b'{') => json_serdes_parse_object(text, index),
        Some(b'-' | b'0'..=b'9') => json_serdes_parse_number(text, index),
        Some(_) => Err(String::from("unexpected JSON token")),
        None => Err(String::from("empty JSON input")),
    }
}

fn json_serdes_parse_array(text: &str, index: usize) -> Result<(SpikeValue, usize), String> {
    let mut index = index + 1;
    let mut values = Vec::new();
    loop {
        index = json_skip_ws(text, index);
        match text.as_bytes().get(index).copied() {
            Some(b']') => {
                return Ok((
                    json_serdes_value_variant("Array", vec![SpikeValue::Array(values)]),
                    index + 1,
                ));
            }
            Some(_) => {
                let (value, next) = json_serdes_parse_value(text, index)?;
                values.push(value);
                index = json_skip_ws(text, next);
                match text.as_bytes().get(index).copied() {
                    Some(b',') => index += 1,
                    Some(b']') => {
                        return Ok((
                            json_serdes_value_variant("Array", vec![SpikeValue::Array(values)]),
                            index + 1,
                        ));
                    }
                    _ => return Err(String::from("array expects ',' or ']'")),
                }
            }
            None => return Err(String::from("unterminated JSON array")),
        }
    }
}

fn json_serdes_parse_object(text: &str, index: usize) -> Result<(SpikeValue, usize), String> {
    let mut index = index + 1;
    let mut entries = Vec::new();
    loop {
        index = json_skip_ws(text, index);
        match text.as_bytes().get(index).copied() {
            Some(b'}') => {
                return Ok((
                    json_serdes_value_variant("Object", vec![SpikeValue::Map(entries)]),
                    index + 1,
                ));
            }
            Some(b'"') => {
                let key_end = json_scan_string_end(text, index)
                    .ok_or_else(|| String::from("unterminated JSON object key"))?;
                let key = json_parse_string(&text[index..key_end])
                    .ok_or_else(|| String::from("invalid JSON object key"))?;
                index = json_skip_ws(text, key_end);
                if text.as_bytes().get(index).copied() != Some(b':') {
                    return Err(String::from("object field expects ':'"));
                }
                let (value, next) = json_serdes_parse_value(text, index + 1)?;
                insert_map_entry(&mut entries, SpikeValue::Text(key), value)
                    .map_err(|err| err.message)?;
                index = json_skip_ws(text, next);
                match text.as_bytes().get(index).copied() {
                    Some(b',') => index += 1,
                    Some(b'}') => {
                        return Ok((
                            json_serdes_value_variant("Object", vec![SpikeValue::Map(entries)]),
                            index + 1,
                        ));
                    }
                    _ => return Err(String::from("object expects ',' or '}'")),
                }
            }
            Some(_) => return Err(String::from("object expects string keys")),
            None => return Err(String::from("unterminated JSON object")),
        }
    }
}

fn json_serdes_parse_number(text: &str, index: usize) -> Result<(SpikeValue, usize), String> {
    let start = index;
    let bytes = text.as_bytes();
    let mut index = index;
    if bytes.get(index).copied() == Some(b'-') {
        index += 1;
    }
    match bytes.get(index).copied() {
        Some(b'0') => index += 1,
        Some(b'1'..=b'9') => {
            index += 1;
            while matches!(bytes.get(index).copied(), Some(b'0'..=b'9')) {
                index += 1;
            }
        }
        _ => return Err(String::from("invalid JSON number")),
    }
    let mut is_float = false;
    if bytes.get(index).copied() == Some(b'.') {
        is_float = true;
        index += 1;
        let fraction_start = index;
        while matches!(bytes.get(index).copied(), Some(b'0'..=b'9')) {
            index += 1;
        }
        if index == fraction_start {
            return Err(String::from("invalid JSON fraction"));
        }
    }
    if matches!(bytes.get(index).copied(), Some(b'e' | b'E')) {
        is_float = true;
        index += 1;
        if matches!(bytes.get(index).copied(), Some(b'+' | b'-')) {
            index += 1;
        }
        let exponent_start = index;
        while matches!(bytes.get(index).copied(), Some(b'0'..=b'9')) {
            index += 1;
        }
        if index == exponent_start {
            return Err(String::from("invalid JSON exponent"));
        }
    }
    let raw = &text[start..index];
    if is_float {
        let value = raw
            .parse::<f64>()
            .map_err(|_| String::from("invalid JSON float"))?;
        if !value.is_finite() {
            return Err(String::from("non-finite JSON float"));
        }
        Ok((
            json_serdes_value_variant("Float", vec![SpikeValue::Float(value)]),
            index,
        ))
    } else {
        raw.parse::<i64>()
            .map(|value| {
                (
                    json_serdes_value_variant("Int", vec![SpikeValue::Int(value)]),
                    index,
                )
            })
            .map_err(|_| String::from("invalid JSON int"))
    }
}

fn json_serdes_value_variant(variant: &str, payloads: Vec<SpikeValue>) -> SpikeValue {
    SpikeValue::Enum {
        enum_name: String::from("std_serdes_Value"),
        variant: variant.to_string(),
        field_names: Vec::new(),
        payloads,
    }
}

/// Escape a std/serdes string for JSON output. Unlike `json_escape_string`,
/// this emits ASCII-safe output: BMP characters above 0x7f become `\uxxxx`
/// escapes and astral characters become lowercase surrogate pairs, matching
/// the generated-runtime serdes contract.
fn json_serdes_escape_string(value: &str) -> String {
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
            ch if (ch as u32) <= 0x7f => out.push(ch),
            ch if (ch as u32) <= 0xffff => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => {
                let scalar = (ch as u32) - 0x10000;
                let high = 0xd800 + (scalar >> 10);
                let low = 0xdc00 + (scalar & 0x3ff);
                out.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
            }
        }
    }
    out.push('"');
    out
}

fn json_serdes_value_to_json(value: &SpikeValue) -> Result<String, Diagnostic> {
    let SpikeValue::Enum {
        enum_name,
        variant,
        payloads,
        ..
    } = value
    else {
        return Err(unsupported(
            "json_serdes_value_to_json expects std/serdes Value",
        ));
    };
    if enum_name != "std_serdes_Value" {
        return Err(unsupported(
            "json_serdes_value_to_json expects std/serdes Value",
        ));
    }
    match (variant.as_str(), payloads.as_slice()) {
        ("Null", []) => Ok(String::from("null")),
        ("Bool", [SpikeValue::Bool(value)]) => Ok(value.to_string()),
        ("Int", [SpikeValue::Int(value)]) => Ok(value.to_string()),
        ("Float", [SpikeValue::Float(value)]) => Ok(json_serdes_float_to_json(*value)),
        ("Text", [SpikeValue::Text(value)]) => Ok(json_serdes_escape_string(value)),
        ("Array", [SpikeValue::Array(values)]) => {
            let rendered = values
                .iter()
                .map(json_serdes_value_to_json)
                .collect::<Result<Vec<_>, _>>()?
                .join(",");
            Ok(format!("[{rendered}]"))
        }
        ("Object", [SpikeValue::Map(entries)]) => json_serdes_object_to_json(entries),
        _ => Err(unsupported("unsupported std/serdes Value shape")),
    }
}

fn json_serdes_object_to_json(entries: &[(SpikeValue, SpikeValue)]) -> Result<String, Diagnostic> {
    let mut rendered = entries
        .iter()
        .map(|(key, value)| {
            let SpikeValue::Text(key) = key else {
                return Err(unsupported("json_serdes_to_json expects string keys"));
            };
            Ok((
                key.clone(),
                format!(
                    "{}:{}",
                    json_serdes_escape_string(key),
                    json_serdes_value_to_json(value)?
                ),
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    rendered.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(format!(
        "{{{}}}",
        rendered
            .into_iter()
            .map(|(_, entry)| entry)
            .collect::<Vec<_>>()
            .join(",")
    ))
}

fn json_serdes_float_to_json(value: f64) -> String {
    if !value.is_finite() {
        return String::from("null");
    }
    let mut rendered = value.to_string();
    if !rendered.contains('.') && !rendered.contains('e') && !rendered.contains('E') {
        rendered.push_str(".0");
    }
    rendered
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

fn spike_values_equal(left: &SpikeValue, right: &SpikeValue) -> Result<bool, Diagnostic> {
    match (left, right) {
        (SpikeValue::Int(left), SpikeValue::Int(right)) => Ok(left == right),
        (SpikeValue::UInt(left), SpikeValue::UInt(right)) => Ok(left == right),
        (SpikeValue::Float(left), SpikeValue::Float(right)) => {
            if !left.is_finite() || !right.is_finite() {
                return Err(unsupported("non-finite float comparison"));
            }
            Ok(left == right)
        }
        (SpikeValue::Bool(left), SpikeValue::Bool(right)) => Ok(left == right),
        (SpikeValue::Text(left), SpikeValue::Text(right)) => Ok(left == right),
        (
            SpikeValue::Struct {
                name: left_name,
                fields: left_fields,
            },
            SpikeValue::Struct {
                name: right_name,
                fields: right_fields,
            },
        ) => Ok(left_name == right_name && named_spike_values_equal(left_fields, right_fields)?),
        (
            SpikeValue::Enum {
                enum_name: left_enum,
                variant: left_variant,
                field_names: left_fields,
                payloads: left_payloads,
            },
            SpikeValue::Enum {
                enum_name: right_enum,
                variant: right_variant,
                field_names: right_fields,
                payloads: right_payloads,
            },
        ) => Ok(left_enum == right_enum
            && left_variant == right_variant
            && left_fields == right_fields
            && spike_value_slices_equal(left_payloads, right_payloads)?),
        (SpikeValue::Tuple(left), SpikeValue::Tuple(right))
        | (SpikeValue::Array(left), SpikeValue::Array(right)) => {
            spike_value_slices_equal(left, right)
        }
        (SpikeValue::Map(left), SpikeValue::Map(right)) => spike_maps_equal(left, right),
        (
            SpikeValue::Task { .. }
            | SpikeValue::JoinHandle { .. }
            | SpikeValue::AsyncChannel { .. }
            | SpikeValue::SelectResult { .. }
            | SpikeValue::Closure { .. }
            | SpikeValue::MutRef(_)
            | SpikeValue::MutSlice { .. },
            _,
        )
        | (
            _,
            SpikeValue::Task { .. }
            | SpikeValue::JoinHandle { .. }
            | SpikeValue::AsyncChannel { .. }
            | SpikeValue::SelectResult { .. }
            | SpikeValue::Closure { .. }
            | SpikeValue::MutRef(_)
            | SpikeValue::MutSlice { .. },
        ) => Err(unsupported(
            "runtime handle equality is not supported by the cranelift spike",
        )),
        _ => Ok(false),
    }
}

fn named_spike_values_equal(
    left: &[(String, SpikeValue)],
    right: &[(String, SpikeValue)],
) -> Result<bool, Diagnostic> {
    if left.len() != right.len() {
        return Ok(false);
    }
    left.iter().zip(right.iter()).try_fold(
        true,
        |equal, ((left_name, left), (right_name, right))| {
            Ok::<_, Diagnostic>(
                equal && left_name == right_name && spike_values_equal(left, right)?,
            )
        },
    )
}

fn spike_value_slices_equal(left: &[SpikeValue], right: &[SpikeValue]) -> Result<bool, Diagnostic> {
    if left.len() != right.len() {
        return Ok(false);
    }
    left.iter()
        .zip(right.iter())
        .try_fold(true, |equal, (left, right)| {
            Ok::<_, Diagnostic>(equal && spike_values_equal(left, right)?)
        })
}

fn spike_maps_equal(
    left: &[(SpikeValue, SpikeValue)],
    right: &[(SpikeValue, SpikeValue)],
) -> Result<bool, Diagnostic> {
    if left.len() != right.len() {
        return Ok(false);
    }
    let mut matched = vec![false; right.len()];
    for (left_key, left_value) in left {
        let mut found = false;
        for (index, (right_key, right_value)) in right.iter().enumerate() {
            if matched[index] || !map_keys_equal(left_key, right_key)? {
                continue;
            }
            if !spike_values_equal(left_value, right_value)? {
                return Ok(false);
            }
            matched[index] = true;
            found = true;
            break;
        }
        if !found {
            return Ok(false);
        }
    }
    Ok(true)
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

fn is_cranelift_runtime_trap(diagnostic: &Diagnostic) -> bool {
    diagnostic.kind == CRANELIFT_RUNTIME_TRAP_KIND
}

fn runtime_trap_text(diagnostic: &Diagnostic) -> String {
    let kind = diagnostic.code.as_deref().unwrap_or("runtime");
    let kind = serde_json::to_string(kind).unwrap_or_else(|_| String::from("\"runtime\""));
    let message =
        serde_json::to_string(&diagnostic.message).unwrap_or_else(|_| String::from("\"\""));
    format!("{{\"kind\":{kind},\"message\":{message}}}")
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
            collect_output_lines(
                &hello_program(),
                &CapabilityConfig::default(),
                Path::new("."),
                Path::new("."),
                None,
            )
            .expect("fold hello"),
            vec![
                OutputLine::stdout("hello from stage1"),
                OutputLine::stdout("42")
            ]
        );
    }

    #[test]
    fn folds_panic_into_stderr_exit_program() {
        let program = Program {
            stmts: vec![Stmt::Panic {
                message: Expr::Literal(LiteralValue::String(String::from("conformance panic"))),
                span: crate::mir::SourceSpan { line: 1, column: 1 },
            }],
            ..hello_program()
        };

        assert_eq!(
            collect_output_program(
                &program,
                &CapabilityConfig::default(),
                Path::new("."),
                Path::new("."),
                None,
            )
            .expect("fold panic"),
            StaticOutputProgram {
                lines: vec![OutputLine::stderr(
                    "{\"kind\":\"panic\",\"message\":\"conformance panic\"}"
                )],
                exit_code: 1,
            }
        );
    }

    #[test]
    fn folds_array_bounds_trap_into_stderr_exit_program() {
        let program = Program {
            stmts: vec![Stmt::Print {
                expr: Expr::Index {
                    base: Box::new(Expr::ArrayLiteral {
                        elements: vec![Expr::Literal(LiteralValue::Int(1))],
                        ty: Type::Array(Box::new(Type::Int), None),
                    }),
                    index: Box::new(Expr::Literal(LiteralValue::Int(2))),
                    ty: Type::Int,
                },
                span: crate::mir::SourceSpan { line: 1, column: 1 },
            }],
            ..hello_program()
        };

        assert_eq!(
            collect_output_program(
                &program,
                &CapabilityConfig::default(),
                Path::new("."),
                Path::new("."),
                None,
            )
            .expect("fold bounds trap"),
            StaticOutputProgram {
                lines: vec![OutputLine::stderr(
                    "{\"kind\":\"runtime\",\"message\":\"array index out of bounds\"}"
                )],
                exit_code: 1,
            }
        );
    }

    #[test]
    fn folds_slice_bounds_trap_into_stderr_exit_program() {
        let program = Program {
            stmts: vec![Stmt::Print {
                expr: Expr::Call {
                    name: String::from("len"),
                    args: vec![Expr::Slice {
                        base: Box::new(Expr::ArrayLiteral {
                            elements: vec![Expr::Literal(LiteralValue::Int(1))],
                            ty: Type::Array(Box::new(Type::Int), None),
                        }),
                        start: Some(Box::new(Expr::Literal(LiteralValue::Int(0)))),
                        end: Some(Box::new(Expr::Literal(LiteralValue::Int(2)))),
                        ty: Type::Slice(Box::new(Type::Int)),
                    }],
                    ty: Type::Int,
                },
                span: crate::mir::SourceSpan { line: 1, column: 1 },
            }],
            ..hello_program()
        };

        assert_eq!(
            collect_output_program(
                &program,
                &CapabilityConfig::default(),
                Path::new("."),
                Path::new("."),
                None,
            )
            .expect("fold slice bounds trap"),
            StaticOutputProgram {
                lines: vec![OutputLine::stderr(
                    "{\"kind\":\"runtime\",\"message\":\"array slice end out of bounds\"}"
                )],
                exit_code: 1,
            }
        );
    }

    #[test]
    fn folds_static_array_bounds_trap_into_stderr_exit_program() {
        let program = Program {
            statics: vec![StaticDef {
                name: String::from("answer"),
                ty: Type::Int,
                expr: Expr::Index {
                    base: Box::new(Expr::ArrayLiteral {
                        elements: vec![Expr::Literal(LiteralValue::Int(1))],
                        ty: Type::Array(Box::new(Type::Int), None),
                    }),
                    index: Box::new(Expr::Literal(LiteralValue::Int(2))),
                    ty: Type::Int,
                },
            }],
            stmts: vec![Stmt::Print {
                expr: Expr::VarRef {
                    name: String::from("answer"),
                    ty: Type::Int,
                },
                span: crate::mir::SourceSpan { line: 1, column: 1 },
            }],
            ..hello_program()
        };

        assert_eq!(
            collect_output_program(
                &program,
                &CapabilityConfig::default(),
                Path::new("."),
                Path::new("."),
                None,
            )
            .expect("fold static bounds trap"),
            StaticOutputProgram {
                lines: vec![OutputLine::stderr(
                    "{\"kind\":\"runtime\",\"message\":\"array index out of bounds\"}"
                )],
                exit_code: 1,
            }
        );
    }

    #[test]
    fn folds_closure_calls_into_print_lines() {
        let int_to_int = Type::Fn(vec![Type::Int], Box::new(Type::Int));
        let int_param = || crate::mir::Param {
            name: String::from("x"),
            ty: Type::Int,
        };
        let program = Program {
            path: String::from("closures"),
            structs: vec![],
            enums: vec![],
            statics: vec![],
            functions: vec![Function {
                name: String::from("apply"),
                source_name: String::from("apply"),
                path: String::from("closures"),
                params: vec![
                    crate::mir::Param {
                        name: String::from("f"),
                        ty: int_to_int.clone(),
                    },
                    crate::mir::Param {
                        name: String::from("value"),
                        ty: Type::Int,
                    },
                ],
                return_ty: Type::Int,
                body: vec![Stmt::Return {
                    expr: Expr::Call {
                        name: String::from("f"),
                        args: vec![Expr::VarRef {
                            name: String::from("value"),
                            ty: Type::Int,
                        }],
                        ty: Type::Int,
                    },
                    span: crate::mir::SourceSpan { line: 2, column: 1 },
                }],
                is_property: false,
                is_async: false,
                is_extern: false,
                extern_abi: None,
                extern_library: None,
                line: 1,
                column: 1,
            }],
            stmts: vec![
                Stmt::Let {
                    name: String::from("inc"),
                    ty: int_to_int.clone(),
                    expr: Expr::Closure {
                        params: vec![int_param()],
                        body: Box::new(Expr::BinaryAdd {
                            op: ArithmeticOp::Add,
                            lhs: Box::new(Expr::VarRef {
                                name: String::from("x"),
                                ty: Type::Int,
                            }),
                            rhs: Box::new(Expr::Literal(LiteralValue::Int(1))),
                            ty: Type::Int,
                        }),
                        ty: int_to_int.clone(),
                    },
                    span: crate::mir::SourceSpan { line: 5, column: 1 },
                },
                Stmt::Print {
                    expr: Expr::Call {
                        name: String::from("inc"),
                        args: vec![Expr::Literal(LiteralValue::Int(41))],
                        ty: Type::Int,
                    },
                    span: crate::mir::SourceSpan { line: 6, column: 1 },
                },
                Stmt::Print {
                    expr: Expr::Call {
                        name: String::from("apply"),
                        args: vec![
                            Expr::Closure {
                                params: vec![crate::mir::Param {
                                    name: String::from("n"),
                                    ty: Type::Int,
                                }],
                                body: Box::new(Expr::BinaryAdd {
                                    op: ArithmeticOp::Add,
                                    lhs: Box::new(Expr::VarRef {
                                        name: String::from("n"),
                                        ty: Type::Int,
                                    }),
                                    rhs: Box::new(Expr::Literal(LiteralValue::Int(2))),
                                    ty: Type::Int,
                                }),
                                ty: int_to_int.clone(),
                            },
                            Expr::Literal(LiteralValue::Int(40)),
                        ],
                        ty: Type::Int,
                    },
                    span: crate::mir::SourceSpan { line: 7, column: 1 },
                },
                Stmt::Let {
                    name: String::from("len"),
                    ty: int_to_int.clone(),
                    expr: Expr::Closure {
                        params: vec![int_param()],
                        body: Box::new(Expr::BinaryAdd {
                            op: ArithmeticOp::Add,
                            lhs: Box::new(Expr::VarRef {
                                name: String::from("x"),
                                ty: Type::Int,
                            }),
                            rhs: Box::new(Expr::Literal(LiteralValue::Int(3))),
                            ty: Type::Int,
                        }),
                        ty: int_to_int.clone(),
                    },
                    span: crate::mir::SourceSpan { line: 8, column: 1 },
                },
                Stmt::Print {
                    expr: Expr::Call {
                        name: String::from("len"),
                        args: vec![Expr::Literal(LiteralValue::Int(39))],
                        ty: Type::Int,
                    },
                    span: crate::mir::SourceSpan { line: 8, column: 8 },
                },
                Stmt::Let {
                    name: String::from("base"),
                    ty: Type::Int,
                    expr: Expr::Literal(LiteralValue::Int(10)),
                    span: crate::mir::SourceSpan { line: 9, column: 1 },
                },
                Stmt::Let {
                    name: String::from("add_base"),
                    ty: int_to_int.clone(),
                    expr: Expr::Closure {
                        params: vec![int_param()],
                        body: Box::new(Expr::BinaryAdd {
                            op: ArithmeticOp::Add,
                            lhs: Box::new(Expr::VarRef {
                                name: String::from("x"),
                                ty: Type::Int,
                            }),
                            rhs: Box::new(Expr::VarRef {
                                name: String::from("base"),
                                ty: Type::Int,
                            }),
                            ty: Type::Int,
                        }),
                        ty: int_to_int,
                    },
                    span: crate::mir::SourceSpan {
                        line: 10,
                        column: 1,
                    },
                },
                Stmt::Print {
                    expr: Expr::Call {
                        name: String::from("add_base"),
                        args: vec![Expr::Literal(LiteralValue::Int(5))],
                        ty: Type::Int,
                    },
                    span: crate::mir::SourceSpan {
                        line: 11,
                        column: 1,
                    },
                },
            ],
        };

        assert_eq!(
            collect_output_lines(
                &program,
                &CapabilityConfig::default(),
                Path::new("."),
                Path::new("."),
                None,
            )
            .expect("fold closures"),
            vec![
                OutputLine::stdout("42"),
                OutputLine::stdout("42"),
                OutputLine::stdout("42"),
                OutputLine::stdout("15")
            ]
        );
    }

    #[test]
    fn folds_match_arm_assignment_into_print_lines() {
        let span = crate::mir::SourceSpan { line: 1, column: 1 };
        let option_int = Type::Option(Box::new(Type::Int));
        let program = Program {
            path: String::from("match-assign"),
            structs: vec![],
            enums: vec![EnumDef {
                name: String::from("Option"),
                variants: vec![
                    EnumVariantDef {
                        name: String::from("Some"),
                        payload_tys: vec![Type::Int],
                        payload_names: vec![],
                    },
                    EnumVariantDef {
                        name: String::from("None"),
                        payload_tys: vec![],
                        payload_names: vec![],
                    },
                ],
            }],
            statics: vec![],
            functions: vec![],
            stmts: vec![
                Stmt::Let {
                    name: String::from("value"),
                    ty: Type::Int,
                    expr: Expr::Literal(LiteralValue::Int(0)),
                    span,
                },
                Stmt::Match {
                    expr: Expr::EnumVariant {
                        enum_name: String::from("Option"),
                        variant: String::from("Some"),
                        field_names: vec![],
                        payloads: vec![Expr::Literal(LiteralValue::Int(1))],
                        ty: option_int,
                    },
                    arms: vec![
                        MatchArm {
                            enum_name: String::from("Option"),
                            variant: String::from("Some"),
                            bindings: vec![],
                            is_named: false,
                            ignore_payloads: true,
                            body: vec![Stmt::Assign {
                                target: Expr::VarRef {
                                    name: String::from("value"),
                                    ty: Type::Int,
                                },
                                expr: Expr::Literal(LiteralValue::Int(1)),
                                span,
                            }],
                        },
                        MatchArm {
                            enum_name: String::from("Option"),
                            variant: String::from("None"),
                            bindings: vec![],
                            is_named: false,
                            ignore_payloads: true,
                            body: vec![],
                        },
                    ],
                    span,
                },
                Stmt::Print {
                    expr: Expr::VarRef {
                        name: String::from("value"),
                        ty: Type::Int,
                    },
                    span,
                },
            ],
        };

        assert_eq!(
            collect_output_lines(
                &program,
                &CapabilityConfig::default(),
                Path::new("."),
                Path::new("."),
                None,
            )
            .expect("fold match arm assignment"),
            vec![OutputLine::stdout("1")]
        );
    }

    #[test]
    fn folds_mutable_local_borrow_write_through_into_print_lines() {
        let program = Program {
            path: String::from("mut-ref"),
            structs: vec![],
            enums: vec![],
            statics: vec![],
            functions: vec![],
            stmts: vec![
                Stmt::Let {
                    name: String::from("value"),
                    ty: Type::String,
                    expr: Expr::Literal(LiteralValue::String(String::from("alpha"))),
                    span: crate::mir::SourceSpan { line: 1, column: 1 },
                },
                Stmt::Let {
                    name: String::from("local"),
                    ty: Type::MutRef(Box::new(Type::String)),
                    expr: Expr::MutBorrow {
                        expr: Box::new(Expr::VarRef {
                            name: String::from("value"),
                            ty: Type::String,
                        }),
                        ty: Type::MutRef(Box::new(Type::String)),
                    },
                    span: crate::mir::SourceSpan { line: 2, column: 1 },
                },
                Stmt::Assign {
                    target: Expr::Deref {
                        expr: Box::new(Expr::VarRef {
                            name: String::from("local"),
                            ty: Type::MutRef(Box::new(Type::String)),
                        }),
                        ty: Type::String,
                    },
                    expr: Expr::Literal(LiteralValue::String(String::from("beta"))),
                    span: crate::mir::SourceSpan { line: 3, column: 1 },
                },
                Stmt::Print {
                    expr: Expr::Deref {
                        expr: Box::new(Expr::VarRef {
                            name: String::from("local"),
                            ty: Type::MutRef(Box::new(Type::String)),
                        }),
                        ty: Type::String,
                    },
                    span: crate::mir::SourceSpan { line: 4, column: 1 },
                },
            ],
        };

        assert_eq!(
            collect_output_lines(
                &program,
                &CapabilityConfig::default(),
                Path::new("."),
                Path::new("."),
                None,
            )
            .expect("fold mutable local write-through"),
            vec![OutputLine::stdout("beta")]
        );
    }

    #[test]
    fn folds_mutable_slice_write_through_into_print_lines() {
        let int_array = Type::Array(Box::new(Type::Int), None);
        let mut_int_slice = Type::MutSlice(Box::new(Type::Int));
        let program = Program {
            path: String::from("mut-slice"),
            structs: vec![],
            enums: vec![],
            statics: vec![],
            functions: vec![],
            stmts: vec![
                Stmt::Let {
                    name: String::from("values"),
                    ty: int_array.clone(),
                    expr: Expr::ArrayLiteral {
                        elements: vec![
                            Expr::Literal(LiteralValue::Int(5)),
                            Expr::Literal(LiteralValue::Int(8)),
                            Expr::Literal(LiteralValue::Int(13)),
                        ],
                        ty: int_array.clone(),
                    },
                    span: crate::mir::SourceSpan { line: 1, column: 1 },
                },
                Stmt::Let {
                    name: String::from("view"),
                    ty: mut_int_slice.clone(),
                    expr: Expr::Slice {
                        base: Box::new(Expr::VarRef {
                            name: String::from("values"),
                            ty: int_array.clone(),
                        }),
                        start: None,
                        end: None,
                        ty: mut_int_slice.clone(),
                    },
                    span: crate::mir::SourceSpan { line: 2, column: 1 },
                },
                Stmt::Assign {
                    target: Expr::Index {
                        base: Box::new(Expr::VarRef {
                            name: String::from("view"),
                            ty: mut_int_slice,
                        }),
                        index: Box::new(Expr::Literal(LiteralValue::Int(0))),
                        ty: Type::Int,
                    },
                    expr: Expr::Literal(LiteralValue::Int(6)),
                    span: crate::mir::SourceSpan { line: 3, column: 1 },
                },
                Stmt::Print {
                    expr: Expr::Index {
                        base: Box::new(Expr::VarRef {
                            name: String::from("values"),
                            ty: int_array,
                        }),
                        index: Box::new(Expr::Literal(LiteralValue::Int(0))),
                        ty: Type::Int,
                    },
                    span: crate::mir::SourceSpan { line: 4, column: 1 },
                },
            ],
        };

        assert_eq!(
            collect_output_lines(
                &program,
                &CapabilityConfig::default(),
                Path::new("."),
                Path::new("."),
                None,
            )
            .expect("fold mutable slice write-through"),
            vec![OutputLine::stdout("6")]
        );
    }

    #[test]
    fn folds_mutable_slice_call_writeback_into_print_lines() {
        let int_array = Type::Array(Box::new(Type::Int), None);
        let mut_int_slice = Type::MutSlice(Box::new(Type::Int));
        let program = Program {
            path: String::from("mut-slice-call"),
            structs: vec![],
            enums: vec![],
            statics: vec![],
            functions: vec![Function {
                name: String::from("bump_first"),
                source_name: String::from("bump_first"),
                path: String::from("mut-slice-call"),
                params: vec![crate::mir::Param {
                    name: String::from("values"),
                    ty: mut_int_slice.clone(),
                }],
                return_ty: Type::Int,
                body: vec![
                    Stmt::Assign {
                        target: Expr::Index {
                            base: Box::new(Expr::VarRef {
                                name: String::from("values"),
                                ty: mut_int_slice.clone(),
                            }),
                            index: Box::new(Expr::Literal(LiteralValue::Int(0))),
                            ty: Type::Int,
                        },
                        expr: Expr::BinaryAdd {
                            op: ArithmeticOp::Add,
                            lhs: Box::new(Expr::Call {
                                name: String::from("first"),
                                args: vec![Expr::VarRef {
                                    name: String::from("values"),
                                    ty: mut_int_slice.clone(),
                                }],
                                ty: Type::Int,
                            }),
                            rhs: Box::new(Expr::Literal(LiteralValue::Int(1))),
                            ty: Type::Int,
                        },
                        span: crate::mir::SourceSpan { line: 2, column: 1 },
                    },
                    Stmt::Return {
                        expr: Expr::Call {
                            name: String::from("first"),
                            args: vec![Expr::VarRef {
                                name: String::from("values"),
                                ty: mut_int_slice.clone(),
                            }],
                            ty: Type::Int,
                        },
                        span: crate::mir::SourceSpan { line: 3, column: 1 },
                    },
                ],
                is_property: false,
                is_async: false,
                is_extern: false,
                extern_abi: None,
                extern_library: None,
                line: 1,
                column: 1,
            }],
            stmts: vec![
                Stmt::Let {
                    name: String::from("values"),
                    ty: int_array.clone(),
                    expr: Expr::ArrayLiteral {
                        elements: vec![
                            Expr::Literal(LiteralValue::Int(5)),
                            Expr::Literal(LiteralValue::Int(8)),
                            Expr::Literal(LiteralValue::Int(13)),
                        ],
                        ty: int_array.clone(),
                    },
                    span: crate::mir::SourceSpan { line: 5, column: 1 },
                },
                Stmt::Print {
                    expr: Expr::Call {
                        name: String::from("bump_first"),
                        args: vec![Expr::Slice {
                            base: Box::new(Expr::VarRef {
                                name: String::from("values"),
                                ty: int_array.clone(),
                            }),
                            start: None,
                            end: None,
                            ty: mut_int_slice,
                        }],
                        ty: Type::Int,
                    },
                    span: crate::mir::SourceSpan { line: 6, column: 1 },
                },
                Stmt::Print {
                    expr: Expr::Call {
                        name: String::from("first"),
                        args: vec![Expr::VarRef {
                            name: String::from("values"),
                            ty: int_array,
                        }],
                        ty: Type::Int,
                    },
                    span: crate::mir::SourceSpan { line: 7, column: 1 },
                },
            ],
        };

        assert_eq!(
            collect_output_lines(
                &program,
                &CapabilityConfig::default(),
                Path::new("."),
                Path::new("."),
                None,
            )
            .expect("fold mutable slice call writeback"),
            vec![OutputLine::stdout("6"), OutputLine::stdout("6")]
        );
    }

    #[test]
    fn function_receiver_alias_is_not_overwritten_by_later_self_param() {
        let point_ty = Type::Struct(String::from("Point"));
        let function = Function {
            name: String::from("Point__same_x"),
            source_name: String::from("same_x"),
            path: String::from("test"),
            params: vec![
                crate::mir::Param {
                    name: String::from("self_"),
                    ty: point_ty.clone(),
                },
                crate::mir::Param {
                    name: String::from("self_"),
                    ty: point_ty.clone(),
                },
            ],
            return_ty: Type::Bool,
            body: vec![Stmt::Return {
                expr: Expr::BinaryCompare {
                    op: CompareOp::Eq,
                    lhs: Box::new(Expr::FieldAccess {
                        base: Box::new(Expr::VarRef {
                            name: String::from("self"),
                            ty: point_ty.clone(),
                        }),
                        field: String::from("x"),
                        ty: Type::Int,
                    }),
                    rhs: Box::new(Expr::FieldAccess {
                        base: Box::new(Expr::VarRef {
                            name: String::from("self_"),
                            ty: point_ty.clone(),
                        }),
                        field: String::from("x"),
                        ty: Type::Int,
                    }),
                    ty: Type::Bool,
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
        };
        let mut lines = Vec::new();
        let functions = HashMap::from([(function.name.as_str(), &function)]);
        let args = vec![
            Expr::StructLiteral {
                name: String::from("Point"),
                fields: vec![crate::mir::StructFieldValue {
                    name: String::from("x"),
                    expr: Expr::Literal(LiteralValue::Int(7)),
                }],
                ty: point_ty.clone(),
            },
            Expr::StructLiteral {
                name: String::from("Point"),
                fields: vec![crate::mir::StructFieldValue {
                    name: String::from("x"),
                    expr: Expr::Literal(LiteralValue::Int(9)),
                }],
                ty: point_ty,
            },
        ];

        assert_eq!(
            eval_call(
                "Point__same_x",
                &args,
                &functions,
                &HashMap::new(),
                &mut lines
            )
            .expect("receiver alias should evaluate"),
            SpikeValue::Bool(false)
        );
    }

    #[test]
    fn folds_nested_mutable_slice_call_writeback_into_print_lines() {
        let int_array = Type::Array(Box::new(Type::Int), None);
        let mut_int_slice = Type::MutSlice(Box::new(Type::Int));
        let program = Program {
            path: String::from("nested-mut-slice-call"),
            structs: vec![],
            enums: vec![],
            statics: vec![],
            functions: vec![Function {
                name: String::from("bump_first"),
                source_name: String::from("bump_first"),
                path: String::from("nested-mut-slice-call"),
                params: vec![crate::mir::Param {
                    name: String::from("values"),
                    ty: mut_int_slice.clone(),
                }],
                return_ty: Type::Int,
                body: vec![
                    Stmt::Assign {
                        target: Expr::Index {
                            base: Box::new(Expr::VarRef {
                                name: String::from("values"),
                                ty: mut_int_slice.clone(),
                            }),
                            index: Box::new(Expr::Literal(LiteralValue::Int(0))),
                            ty: Type::Int,
                        },
                        expr: Expr::BinaryAdd {
                            op: ArithmeticOp::Add,
                            lhs: Box::new(Expr::Call {
                                name: String::from("first"),
                                args: vec![Expr::VarRef {
                                    name: String::from("values"),
                                    ty: mut_int_slice.clone(),
                                }],
                                ty: Type::Int,
                            }),
                            rhs: Box::new(Expr::Literal(LiteralValue::Int(1))),
                            ty: Type::Int,
                        },
                        span: crate::mir::SourceSpan { line: 2, column: 1 },
                    },
                    Stmt::Return {
                        expr: Expr::Call {
                            name: String::from("first"),
                            args: vec![Expr::VarRef {
                                name: String::from("values"),
                                ty: mut_int_slice.clone(),
                            }],
                            ty: Type::Int,
                        },
                        span: crate::mir::SourceSpan { line: 3, column: 1 },
                    },
                ],
                is_property: false,
                is_async: false,
                is_extern: false,
                extern_abi: None,
                extern_library: None,
                line: 1,
                column: 1,
            }],
            stmts: vec![
                Stmt::Let {
                    name: String::from("values"),
                    ty: int_array.clone(),
                    expr: Expr::ArrayLiteral {
                        elements: vec![
                            Expr::Literal(LiteralValue::Int(5)),
                            Expr::Literal(LiteralValue::Int(8)),
                        ],
                        ty: int_array.clone(),
                    },
                    span: crate::mir::SourceSpan { line: 5, column: 1 },
                },
                Stmt::Let {
                    name: String::from("result"),
                    ty: Type::Int,
                    expr: Expr::BinaryAdd {
                        op: ArithmeticOp::Add,
                        lhs: Box::new(Expr::Call {
                            name: String::from("bump_first"),
                            args: vec![Expr::Slice {
                                base: Box::new(Expr::VarRef {
                                    name: String::from("values"),
                                    ty: int_array.clone(),
                                }),
                                start: None,
                                end: None,
                                ty: mut_int_slice,
                            }],
                            ty: Type::Int,
                        }),
                        rhs: Box::new(Expr::Literal(LiteralValue::Int(0))),
                        ty: Type::Int,
                    },
                    span: crate::mir::SourceSpan { line: 6, column: 1 },
                },
                Stmt::Print {
                    expr: Expr::VarRef {
                        name: String::from("result"),
                        ty: Type::Int,
                    },
                    span: crate::mir::SourceSpan { line: 7, column: 1 },
                },
                Stmt::Print {
                    expr: Expr::Call {
                        name: String::from("first"),
                        args: vec![Expr::VarRef {
                            name: String::from("values"),
                            ty: int_array,
                        }],
                        ty: Type::Int,
                    },
                    span: crate::mir::SourceSpan { line: 8, column: 1 },
                },
            ],
        };

        assert_eq!(
            collect_output_lines(
                &program,
                &CapabilityConfig::default(),
                Path::new("."),
                Path::new("."),
                None,
            )
            .expect("fold nested mutable slice call writeback"),
            vec![OutputLine::stdout("6"), OutputLine::stdout("6")]
        );
    }

    #[test]
    fn stdio_stdin_calls_consume_manifest_input() {
        with_spike_stdin(Some("first\r\nsecond\nremaining"), || {
            assert_eq!(
                eval_io_readline_call(&[]).expect("first line"),
                spike_option(Some(SpikeValue::Text(String::from("first"))))
            );
            assert_eq!(
                eval_io_readline_call(&[]).expect("second line"),
                spike_option(Some(SpikeValue::Text(String::from("second"))))
            );
            assert_eq!(
                eval_io_read_to_string_call(&[]).expect("remaining input"),
                SpikeValue::Text(String::from("remaining"))
            );
            assert_eq!(eval_io_readline_call(&[]).expect("eof"), spike_option(None));
            Ok(())
        })
        .expect("stdin evaluation");
    }

    #[test]
    fn folds_const_match_statement_into_print_lines() {
        let mut program = hello_program();
        program.functions.clear();
        program.stmts = vec![Stmt::Match {
            expr: Expr::Literal(LiteralValue::Int(7)),
            arms: vec![
                MatchArm {
                    enum_name: String::new(),
                    variant: String::from("3"),
                    bindings: Vec::new(),
                    is_named: false,
                    ignore_payloads: false,
                    body: vec![Stmt::Print {
                        expr: Expr::Literal(LiteralValue::String(String::from("wrong"))),
                        span: crate::mir::SourceSpan { line: 1, column: 1 },
                    }],
                },
                MatchArm {
                    enum_name: String::new(),
                    variant: String::from("7"),
                    bindings: Vec::new(),
                    is_named: false,
                    ignore_payloads: false,
                    body: vec![Stmt::Print {
                        expr: Expr::Literal(LiteralValue::String(String::from("ready"))),
                        span: crate::mir::SourceSpan { line: 1, column: 1 },
                    }],
                },
            ],
            span: crate::mir::SourceSpan { line: 1, column: 1 },
        }];

        assert_eq!(
            collect_output_lines(
                &program,
                &CapabilityConfig::default(),
                Path::new("."),
                Path::new("."),
                None,
            )
            .expect("fold match"),
            vec![OutputLine::stdout("ready")]
        );
    }

    #[test]
    fn loopback_bind_parser_accepts_localhost() {
        assert_eq!(
            http_parse_loopback_bind("localhost:0"),
            Some(SocketAddr::from(([127, 0, 0, 1], 0)))
        );
        assert_eq!(
            http_parse_loopback_bind("127.0.0.1:8080"),
            Some(SocketAddr::from(([127, 0, 0, 1], 8080)))
        );
        assert_eq!(http_parse_loopback_bind("example.com:80"), None);
        assert_eq!(http_parse_loopback_bind("192.0.2.1:80"), None);
    }

    #[test]
    fn static_map_lookups_respect_last_duplicate_key() {
        let map = Expr::MapLiteral {
            entries: vec![
                MapEntry {
                    key: Expr::Literal(LiteralValue::Int(1)),
                    value: Expr::Literal(LiteralValue::Int(10)),
                },
                MapEntry {
                    key: Expr::Literal(LiteralValue::Int(1)),
                    value: Expr::Literal(LiteralValue::Int(20)),
                },
                MapEntry {
                    key: Expr::Literal(LiteralValue::Int(2)),
                    value: Expr::Literal(LiteralValue::Int(30)),
                },
            ],
            ty: Type::Map(Box::new(Type::Int), Box::new(Type::Int)),
        };
        let key = Expr::Literal(LiteralValue::Int(1));
        let missing_key = Expr::Literal(LiteralValue::Int(3));
        let expected_value = Expr::Literal(LiteralValue::Int(20));
        let expected_default = Expr::Literal(LiteralValue::Int(99));
        let args = vec![map.clone(), key.clone()];
        let static_bindings = I64StaticBindings::default();

        assert_eq!(
            i64_map_get_value_expr("get", &args, &static_bindings),
            Some(Some(&expected_value))
        );
        assert_eq!(
            lower_i64_map_contains_key_condition(
                "contains",
                &args,
                &HashMap::new(),
                &HashMap::new(),
                &HashMap::new(),
                &static_bindings
            ),
            Some(CraneliftI64Condition::Literal(true))
        );
        assert_eq!(
            lower_i64_map_contains_key_condition(
                "contains",
                &[map.clone(), missing_key],
                &HashMap::new(),
                &HashMap::new(),
                &HashMap::new(),
                &static_bindings
            ),
            Some(CraneliftI64Condition::Literal(false))
        );
        assert_eq!(
            lower_i64_map_get_or_default_expr(
                "get_or_default",
                &[map, key, expected_default],
                &HashMap::new(),
                &HashMap::new(),
                &HashMap::new(),
                &static_bindings
            ),
            Some(CraneliftI64Expr::Literal(20))
        );
    }

    #[test]
    fn static_map_keys_respect_last_duplicate_key_position() {
        let map = Expr::MapLiteral {
            entries: vec![
                MapEntry {
                    key: Expr::Literal(LiteralValue::Int(1)),
                    value: Expr::Literal(LiteralValue::Int(10)),
                },
                MapEntry {
                    key: Expr::Literal(LiteralValue::Int(2)),
                    value: Expr::Literal(LiteralValue::Int(20)),
                },
                MapEntry {
                    key: Expr::Literal(LiteralValue::Int(1)),
                    value: Expr::Literal(LiteralValue::Int(30)),
                },
            ],
            ty: Type::Map(Box::new(Type::Int), Box::new(Type::Int)),
        };

        assert_eq!(
            i64_map_keys_expr(
                &Expr::Call {
                    name: String::from("keys"),
                    args: vec![map],
                    ty: Type::Array(Box::new(Type::Int), None),
                },
                &I64StaticBindings::default()
            ),
            Some(vec![I64MapKey::Int(2), I64MapKey::Int(1)])
        );
    }

    #[test]
    fn net_tcp_close_removes_stream_state_before_future_loopback_echoes() {
        let listener_port = 4242;
        let stream_handle = 7;
        let streams = spike_tcp_streams();
        let listeners = spike_tcp_listeners();
        {
            let mut streams = streams.lock().expect("lock tcp streams");
            streams.insert(
                stream_handle,
                SpikeTcpStream {
                    listener_port,
                    received: String::from("old"),
                    written: String::from("stale"),
                },
            );
        }
        {
            let mut listeners = listeners.lock().expect("lock tcp listeners");
            listeners.insert(
                11,
                SpikeTcpListener {
                    port: listener_port,
                },
            );
        }

        assert_eq!(net_tcp_close(stream_handle), 0);
        assert_eq!(
            net_tcp_registered_loopback_echo("127.0.0.1", listener_port, "fresh"),
            Some(String::from("fresh"))
        );
        assert!(
            streams
                .lock()
                .expect("lock tcp streams after close")
                .get(&stream_handle)
                .is_none()
        );
    }

    #[test]
    fn static_map_literal_entries_snapshot_static_var_values() {
        let mut static_bindings = I64StaticBindings::default();
        static_bindings
            .values
            .insert(String::from("code"), CraneliftI64Expr::Literal(7));
        let entries = vec![MapEntry {
            key: Expr::Literal(LiteralValue::String(String::from("deploy"))),
            value: Expr::VarRef {
                name: String::from("code"),
                ty: Type::Int,
            },
        }];

        let recorded =
            i64_static_map_literal_entries(&entries, &static_bindings).expect("static map entries");

        static_bindings
            .values
            .insert(String::from("code"), CraneliftI64Expr::Literal(48));
        static_bindings
            .map_literals
            .insert(String::from("codes"), recorded);

        assert_eq!(
            lower_i64_map_get_or_default_expr(
                "get_or_default",
                &[
                    Expr::VarRef {
                        name: String::from("codes"),
                        ty: Type::Map(Box::new(Type::String), Box::new(Type::Int)),
                    },
                    Expr::Literal(LiteralValue::String(String::from("deploy"))),
                    Expr::Literal(LiteralValue::Int(0)),
                ],
                &HashMap::new(),
                &HashMap::new(),
                &HashMap::new(),
                &static_bindings,
            ),
            Some(CraneliftI64Expr::Literal(7))
        );

        assert_eq!(
            i64_static_map_literal_entries(
                &[MapEntry {
                    key: Expr::Literal(LiteralValue::String(String::from("deploy"))),
                    value: Expr::VarRef {
                        name: String::from("runtime_code"),
                        ty: Type::Int,
                    },
                }],
                &I64StaticBindings::default(),
            ),
            None
        );
    }

    #[test]
    fn fs_read_folding_is_disabled_when_program_writes() {
        let mut static_bindings = I64StaticBindings::default();
        static_bindings.fs_root = Some(PathBuf::from("."));
        static_bindings.has_fs_write_calls = true;

        assert_eq!(
            i64_fs_read_file_len_expr("fixture.txt", "fixture.txt".len(), &static_bindings),
            None
        );
    }

    #[test]
    fn tagged_payload_call_assign_helper_preserves_result_key_layout() {
        let mut locals = Vec::new();
        let mut local_indexes = HashMap::new();

        let lowered = lower_i64_tagged_payload_call_assign_stmts(
            "outcome",
            2,
            7,
            vec![CraneliftI64Expr::Literal(11)],
            &mut locals,
            &mut local_indexes,
            i64_result_tag_key,
            i64_result_payload_slot_key,
            i64_result_payload_key,
        )
        .expect("tagged payload call assign lowering");

        assert_eq!(
            local_indexes,
            HashMap::from([
                (String::from("outcome!tag"), 0),
                (String::from("outcome!payload"), 1),
                (String::from("outcome!payload1"), 2),
            ])
        );
        assert_eq!(
            locals,
            vec![
                CraneliftI64Expr::Literal(0),
                CraneliftI64Expr::Literal(0),
                CraneliftI64Expr::Literal(0),
            ]
        );
        assert_eq!(
            lowered,
            vec![CraneliftI64Stmt::CallAssign {
                locals: vec![0, 1, 2],
                function: 7,
                args: vec![CraneliftI64Expr::Literal(11)],
            }]
        );
    }

    #[test]
    fn string_option_len_call_lowering_emits_shared_payload_and_tag_shape() {
        let mut locals = Vec::new();
        let mut local_indexes = HashMap::new();

        let lowered = lower_i64_string_option_len_call_let_stmts(
            "maybe_line",
            CraneliftI64Expr::Literal(42),
            &mut locals,
            &mut local_indexes,
        )
        .expect("string option len lowering");

        assert_eq!(
            local_indexes,
            HashMap::from([
                (String::from("maybe_line?payload"), 0),
                (String::from("maybe_line?tag"), 1),
            ])
        );
        assert_eq!(
            locals,
            vec![CraneliftI64Expr::Literal(0), CraneliftI64Expr::Literal(0)]
        );
        assert_eq!(
            lowered,
            vec![
                CraneliftI64Stmt::Assign(axiomc_backend_cranelift::I64Assign {
                    local: 0,
                    value: CraneliftI64Expr::Literal(42),
                }),
                CraneliftI64Stmt::Assign(axiomc_backend_cranelift::I64Assign {
                    local: 1,
                    value: CraneliftI64Expr::Select {
                        cond: Box::new(CraneliftI64Condition::Compare(CraneliftI64Compare {
                            op: CraneliftI64CompareOp::Ge,
                            lhs: CraneliftI64Expr::Local(0),
                            rhs: CraneliftI64Expr::Literal(0),
                        })),
                        then_result: Box::new(CraneliftI64Expr::Literal(1)),
                        else_result: Box::new(CraneliftI64Expr::Literal(0)),
                    },
                }),
            ]
        );
    }

    #[test]
    fn write_candidate_rejects_dangling_symlink_leaf() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        let target = root.join("target.txt");
        let link = root.join("dangling.txt");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).expect("create dangling symlink");
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&target, &link).expect("create dangling symlink");

        assert_eq!(
            spike_fs_write_candidate_for_scope(root, root, "dangling.txt", false),
            None
        );
    }

    #[test]
    fn cranelift_http_resolver_rejects_blocked_network_addresses() {
        for ip in [
            "0.0.0.0",
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "192.0.2.1",
            "192.168.0.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "::",
            "::1",
            "::ffff:127.0.0.1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
        ] {
            assert!(
                is_blocked_network_ip(ip.parse().expect("valid IP literal")),
                "{ip} should be blocked"
            );
        }

        assert!(
            resolve_public_socket_addrs("127.0.0.1", 80).is_none(),
            "loopback HTTP targets must not be reachable during Cranelift folding"
        );
    }

    #[test]
    fn cranelift_http_resolver_allows_public_addresses() {
        for ip in ["1.1.1.1", "8.8.8.8", "2001:4860:4860::8888"] {
            assert!(
                !is_blocked_network_ip(ip.parse().expect("valid IP literal")),
                "{ip} should be allowed"
            );
        }
    }

    #[test]
    fn i64_net_resolve_text_normalizes_ipv6_literals() {
        assert_eq!(
            super::i64_net_resolve_text("0:0:0:0:0:0:0:1").as_deref(),
            None
        );
        assert_eq!(super::i64_net_resolve_text("127.0.0.1").as_deref(), None);
    }

    #[test]
    fn i64_net_resolve_text_allows_public_numeric_literals() {
        assert_eq!(
            super::i64_net_resolve_text("8.8.8.8").as_deref(),
            Some("8.8.8.8")
        );
    }

    #[test]
    fn i64_net_resolve_host_uses_canonical_ipv6_length() {
        let expr = Expr::Call {
            name: String::from("net_resolve"),
            args: vec![Expr::Literal(LiteralValue::String(String::from(
                "2001:4860:4860:0:0:0:0:8888",
            )))],
            ty: Type::Option(Box::new(Type::String)),
        };

        let host = super::i64_net_resolve_host(&expr, &I64StaticBindings::default())
            .expect("numeric IPv6 host should lower");

        assert_eq!(host.host, "2001:4860:4860:0:0:0:0:8888");
        assert_eq!(host.resolved_len, 20);
        assert_ne!(host.resolved_len, host.host.len() as i64);
    }
}
