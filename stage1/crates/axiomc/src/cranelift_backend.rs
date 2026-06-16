use crate::diagnostics::Diagnostic;
use crate::mir::{
    ArithmeticOp, CompareOp, EnumDef, EnumVariantDef, Expr, Function, LiteralValue, LogicOp,
    MapEntry, MatchArm, MatchExprArm, Program, StaticDef, Stmt, StructDef, Type,
};
use crate::syntax::NumericType;
use axiomc_backend_cranelift::{
    I64BinaryOp as CraneliftI64BinaryOp, I64Cast as CraneliftI64Cast,
    I64Compare as CraneliftI64Compare, I64CompareOp as CraneliftI64CompareOp,
    I64Condition as CraneliftI64Condition, I64ExitBody, I64ExitProgram,
    I64Expr as CraneliftI64Expr, I64Function as CraneliftI64Function,
    I64ReturnBlock as CraneliftI64ReturnBlock, I64Stmt as CraneliftI64Stmt,
    I64ValueBody as CraneliftI64ValueBody, I64ValueReturnBlock as CraneliftI64ValueReturnBlock,
    OutputLine, OutputStream,
};
use std::collections::{HashMap, HashSet};
use std::env;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};

const SPIKE_FS_ROOT_BINDING: &str = "$axiom_fs_root";
const SPIKE_MAX_FS_READ_BYTES: u64 = 64 * 1024 * 1024;
const SPIKE_MAX_FS_WRITE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Default)]
struct I64StaticBindings {
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
    time_wrappers: HashSet<String>,
    time_duration_ms_wrappers: HashSet<String>,
    time_sleep_wrappers: HashSet<String>,
    fs_read_wrappers: HashSet<String>,
    fs_write_wrappers: HashMap<String, String>,
    fs_shim_wrappers: HashSet<String>,
    net_shim_wrappers: HashSet<String>,
    http_shim_wrappers: HashSet<String>,
    http_get_wrappers: HashSet<String>,
    http_serve_once_wrappers: HashSet<String>,
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
    log_wrappers: HashSet<String>,
    log_field_string_wrappers: HashSet<String>,
    log_field_int_wrappers: HashSet<String>,
    log_field_bool_wrappers: HashSet<String>,
    log_fields2_wrappers: HashSet<String>,
    log_fields3_wrappers: HashSet<String>,
    log_event_wrappers: HashSet<String>,
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
    fs_root: Option<PathBuf>,
    structs: HashMap<String, StructDef>,
    enums: HashMap<String, EnumDef>,
    functions: HashMap<String, Function>,
}

struct I64HelperSignature {
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
    Task {
        value: Option<Box<SpikeValue>>,
        canceled: bool,
    },
    JoinHandle(Box<SpikeValue>),
    AsyncChannel {
        slot: Option<Box<SpikeValue>>,
    },
    SelectResult {
        selected: i64,
        value: Option<Box<SpikeValue>>,
    },
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

struct SpikeHttpServer {
    listener: TcpListener,
}

struct SpikeHttpRequest {
    stream: TcpStream,
    method: String,
    path: String,
    body: String,
}

struct SpikeTcpListener {
    port: i64,
}

struct SpikeTcpStream {
    received: String,
    written: String,
}

static SPIKE_HTTP_NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);
static SPIKE_HTTP_SERVERS: OnceLock<Mutex<HashMap<i64, SpikeHttpServer>>> = OnceLock::new();
static SPIKE_HTTP_REQUESTS: OnceLock<Mutex<HashMap<i64, SpikeHttpRequest>>> = OnceLock::new();
static SPIKE_TCP_NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);
static SPIKE_TCP_LISTENERS: OnceLock<Mutex<HashMap<i64, SpikeTcpListener>>> = OnceLock::new();
static SPIKE_TCP_STREAMS: OnceLock<Mutex<HashMap<i64, SpikeTcpStream>>> = OnceLock::new();

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
    if let Some(program) = lower_i64_exit_program(program, fs_root) {
        return axiomc_backend_cranelift::compile_i64_exit_program(
            program,
            object_path,
            binary_path,
        )
        .map_err(|err| {
            Diagnostic::new("build", err.to_string()).with_path(object_path.display().to_string())
        });
    }
    if program.stmts.is_empty()
        && program
            .functions
            .iter()
            .any(|function| function.source_name == "main" && function.params.is_empty())
    {
        return Err(unsupported(
            "main function is outside the direct-native i64 ABI subset",
        ));
    }
    let lines = collect_output_lines(program, package_root, fs_root)?;
    axiomc_backend_cranelift::compile_output_lines(&lines, object_path, binary_path).map_err(
        |err| {
            Diagnostic::new("build", err.to_string()).with_path(object_path.display().to_string())
        },
    )
}

fn lower_i64_exit_program(program: &Program, fs_root: &Path) -> Option<I64ExitProgram> {
    if !program.stmts.is_empty() {
        return None;
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
    static_bindings.fs_root = Some(fs_root.to_path_buf());
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
    static_bindings.net_shim_wrappers = program
        .functions
        .iter()
        .filter(|function| is_i64_std_net_shim_wrapper(function))
        .map(|function| function.name.clone())
        .collect();
    static_bindings.http_shim_wrappers = program
        .functions
        .iter()
        .filter(|function| function.path == "<stdlib>/http.ax")
        .map(|function| function.name.clone())
        .collect();
    static_bindings.http_get_wrappers = program
        .functions
        .iter()
        .filter(|function| function.path == "<stdlib>/http.ax" && function.source_name == "get")
        .map(|function| function.name.clone())
        .collect();
    static_bindings.http_serve_once_wrappers = program
        .functions
        .iter()
        .filter(|function| {
            function.path == "<stdlib>/http.ax" && function.source_name == "serve_once"
        })
        .map(|function| function.name.clone())
        .collect();
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
    let process_status_wrappers = static_bindings.process_status_wrappers.clone();
    let env_get_wrappers = static_bindings.env_get_wrappers.clone();
    let time_wrappers = static_bindings.time_wrappers.clone();
    let fs_shim_wrappers = static_bindings.fs_shim_wrappers.clone();
    let net_shim_wrappers = static_bindings.net_shim_wrappers.clone();
    let http_shim_wrappers = static_bindings.http_shim_wrappers.clone();
    let collection_wrappers = static_bindings.collection_wrappers.clone();
    let regex_wrappers = static_bindings.regex_wrappers.clone();
    let encoding_wrappers = static_bindings.encoding_wrappers.clone();
    let json_wrappers = static_bindings.json_wrappers.clone();
    let log_wrappers = static_bindings.log_wrappers.clone();
    let string_builder_wrappers = static_bindings.string_builder_wrappers.clone();
    let crypto_wrappers = static_bindings.crypto_wrappers.clone();
    let ffi_strlen_symbols = static_bindings.ffi_strlen_symbols.clone();
    let sync_once_wrappers = static_bindings.sync_once_wrappers.clone();
    let sync_once_with_wrappers = static_bindings.sync_once_with_wrappers.clone();
    let sync_once_is_set_wrappers = static_bindings.sync_once_is_set_wrappers.clone();
    let sync_once_take_wrappers = static_bindings.sync_once_take_wrappers.clone();
    let sync_channel_wrappers = static_bindings.sync_channel_wrappers.clone();
    let sync_send_wrappers = static_bindings.sync_send_wrappers.clone();
    let sync_try_recv_wrappers = static_bindings.sync_try_recv_wrappers.clone();
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
    )?;
    Some(I64ExitProgram {
        functions,
        locals,
        stmts,
        body,
    })
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
            Stmt::Let { name, ty, expr, .. }
                if is_i64_compatible_type(ty) && !seen_runtime_stmt =>
            {
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
                    &local_conditions,
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
                static_bindings
                    .map_literals
                    .insert(name.clone(), entries.clone());
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
                ty: Type::Slice(_) | Type::MutSlice(_),
                expr,
                ..
            } if !seen_runtime_stmt => {
                lower_i64_slice_projection_aliases(
                    name,
                    expr,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    static_bindings,
                    false,
                )?;
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
                expr:
                    expr @ Expr::Call {
                        name: call_name,
                        args,
                        ..
                    },
                ..
            } if (is_i64_option_local_payload_type_static(inner, static_bindings)
                || is_i64_known_string_option_call_let_type(inner.as_ref()))
                && !seen_runtime_stmt =>
            {
                if let Some(assigns) = lower_i64_known_string_option_call_let_stmts(
                    name,
                    inner.as_ref(),
                    expr,
                    &mut *static_bindings,
                ) {
                    let has_runtime_stmts = !assigns.is_empty();
                    lowered_stmts.extend(assigns);
                    seen_runtime_stmt = has_runtime_stmts;
                } else if let Some(assigns) = lower_i64_known_scalar_option_call_let_stmts(
                    name,
                    inner.as_ref(),
                    expr,
                    &mut locals,
                    &mut local_indexes,
                    static_bindings,
                ) {
                    lowered_stmts.extend(assigns);
                    seen_runtime_stmt = true;
                } else {
                    lowered_stmts.extend(lower_i64_option_call_let_stmts(
                        name,
                        inner.as_ref(),
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
                )?);
            }
            Stmt::Assign { .. } | Stmt::If { .. } | Stmt::While { .. } | Stmt::Match { .. } => {
                seen_runtime_stmt = true;
                lowered_stmts.extend(lower_i64_runtime_stmt_stmts(
                    stmt,
                    &mut locals,
                    local_indexes.clone(),
                    local_conditions.clone(),
                    helper_signatures,
                    static_bindings,
                )?);
            }
            _ => return None,
        }
    }
    let body = match return_stmt {
        Stmt::Return { expr, .. } => CraneliftI64ValueBody::Return(
            lower_i64_aggregate_return_values(
                expr,
                shape,
                &local_indexes,
                &local_conditions,
                helper_signatures,
                static_bindings,
            )
            .filter(|results| results.len() == shape.slot_count())?,
        ),
        Stmt::If {
            cond,
            then_block,
            else_block: Some(else_block),
            ..
        } => CraneliftI64ValueBody::IfBlockReturn {
            cond: lower_i64_condition(
                cond,
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
            )?,
            else_block: lower_i64_aggregate_return_block(
                else_block,
                shape,
                &mut locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?,
        },
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
            )?);
        } else {
            stmts.extend(lower_i64_runtime_stmt_stmts(
                stmt,
                locals,
                local_indexes.clone(),
                local_conditions.clone(),
                helper_signatures,
                static_bindings,
            )?);
        }
    }
    let results = lower_i64_aggregate_return_values(
        expr,
        shape,
        &local_indexes,
        &local_conditions,
        helper_signatures,
        static_bindings,
    )?;
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
            Stmt::Let { name, ty, expr, .. }
                if is_i64_compatible_type(ty) && !seen_runtime_stmt =>
            {
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
                    &local_conditions,
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
                static_bindings
                    .map_literals
                    .insert(name.clone(), entries.clone());
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
                ty: Type::Slice(_) | Type::MutSlice(_),
                expr,
                ..
            } if !seen_runtime_stmt => {
                lower_i64_slice_projection_aliases(
                    name,
                    expr,
                    &mut locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    static_bindings,
                    false,
                )?;
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
                expr:
                    expr @ Expr::Call {
                        name: call_name,
                        args,
                        ..
                    },
                ..
            } if (is_i64_option_local_payload_type_static(inner, static_bindings)
                || is_i64_known_string_option_call_let_type(inner.as_ref()))
                && !seen_runtime_stmt =>
            {
                if let Some(assigns) = lower_i64_known_string_option_call_let_stmts(
                    name,
                    inner.as_ref(),
                    expr,
                    &mut *static_bindings,
                ) {
                    let has_runtime_stmts = !assigns.is_empty();
                    lowered_stmts.extend(assigns);
                    seen_runtime_stmt = has_runtime_stmts;
                } else if let Some(assigns) = lower_i64_known_scalar_option_call_let_stmts(
                    name,
                    inner.as_ref(),
                    expr,
                    &mut locals,
                    &mut local_indexes,
                    static_bindings,
                ) {
                    lowered_stmts.extend(assigns);
                    seen_runtime_stmt = true;
                } else {
                    lowered_stmts.extend(lower_i64_option_call_let_stmts(
                        name,
                        inner.as_ref(),
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
                )?);
            }
            Stmt::Assign { .. } | Stmt::If { .. } | Stmt::While { .. } | Stmt::Match { .. } => {
                seen_runtime_stmt = true;
                lowered_stmts.extend(lower_i64_runtime_stmt_stmts(
                    stmt,
                    &mut locals,
                    local_indexes.clone(),
                    local_conditions.clone(),
                    helper_signatures,
                    static_bindings,
                )?);
            }
            _ => return None,
        }
    }
    let body = match return_stmt {
        Stmt::Return { expr, .. } => lower_i64_exit_return(
            expr,
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
            let cond = lower_i64_condition(
                cond,
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
            )?;
            let else_block = lower_i64_return_block(
                else_block,
                &mut locals,
                local_indexes.clone(),
                local_conditions.clone(),
                helper_signatures,
                static_bindings,
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
) -> Option<Vec<CraneliftI64Stmt>> {
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
    Some(vec![lower_i64_runtime_stmt(
        stmt,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?])
}

fn lower_i64_runtime_stmt(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: HashMap<String, usize>,
    local_conditions: HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Stmt> {
    match stmt {
        Stmt::Match { .. } => {
            if let Some(stmt) = lower_i64_option_match_stmt(
                stmt,
                locals,
                local_indexes.clone(),
                local_conditions.clone(),
                helper_signatures,
                static_bindings,
            ) {
                Some(stmt)
            } else if let Some(stmt) = lower_i64_result_match_stmt(
                stmt,
                locals,
                local_indexes.clone(),
                local_conditions.clone(),
                helper_signatures,
                static_bindings,
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
                )
            }
        }
        Stmt::Assign { .. } => Some(CraneliftI64Stmt::Assign(lower_i64_assign(
            stmt,
            &local_indexes,
            &local_conditions,
            helper_signatures,
            static_bindings,
        )?)),
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
            )?,
            else_body: lower_i64_runtime_stmts(
                else_block.as_deref().unwrap_or(&[]),
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
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
            )?,
        }),
        _ => None,
    }
}

fn lower_i64_option_match_stmt(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: HashMap<String, usize>,
    local_conditions: HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
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
    Some(CraneliftI64Stmt::If {
        cond,
        then_body: lower_i64_runtime_stmts(
            &some_arm.body,
            locals,
            some_indexes,
            some_conditions,
            helper_signatures,
            static_bindings,
        )?,
        else_body: lower_i64_runtime_stmts(
            &none_arm.body,
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
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
        )?,
        else_body: lower_i64_runtime_stmts(
            &err_arm.body,
            locals,
            err_indexes,
            err_conditions,
            helper_signatures,
            static_bindings,
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
) -> Option<Vec<CraneliftI64Stmt>> {
    let mut lowered = Vec::new();
    for stmt in stmts {
        if matches!(stmt, Stmt::Let { .. }) {
            lowered.extend(lower_i64_runtime_let_stmts(
                stmt,
                locals,
                &mut local_indexes,
                &mut local_conditions,
                helper_signatures,
                static_bindings,
            )?);
        } else {
            lowered.extend(lower_i64_runtime_stmt_stmts(
                stmt,
                locals,
                local_indexes.clone(),
                local_conditions.clone(),
                helper_signatures,
                static_bindings,
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
) -> Option<Vec<CraneliftI64Stmt>> {
    if let Stmt::Let {
        name,
        ty: Type::Slice(_) | Type::MutSlice(_),
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
    Some(vec![lower_i64_runtime_let(
        stmt,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?])
}

fn lower_i64_runtime_string_len_let_stmts(
    stmt: &Stmt,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
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
    Some(assigns)
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
    let mut assign_locals = Vec::with_capacity(1 + payload_slots);
    let tag_local = local_indexes.len();
    local_indexes.insert(i64_option_tag_key(name), tag_local);
    locals.push(CraneliftI64Expr::Literal(0));
    assign_locals.push(tag_local);
    for index in 0..payload_slots {
        let payload_local = local_indexes.len();
        local_indexes.insert(i64_option_payload_slot_key(name, index), payload_local);
        if index == 0 {
            local_indexes.insert(i64_option_payload_key(name), payload_local);
        }
        locals.push(CraneliftI64Expr::Literal(0));
        assign_locals.push(payload_local);
    }
    Some(vec![CraneliftI64Stmt::CallAssign {
        locals: assign_locals,
        function: signature.function,
        args: lowered_args,
    }])
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
    let mut assign_locals = Vec::with_capacity(1 + payload_slots);
    let tag_local = local_indexes.len();
    local_indexes.insert(i64_result_tag_key(name), tag_local);
    locals.push(CraneliftI64Expr::Literal(0));
    assign_locals.push(tag_local);
    for index in 0..payload_slots {
        let payload_local = local_indexes.len();
        local_indexes.insert(i64_result_payload_slot_key(name, index), payload_local);
        if index == 0 {
            local_indexes.insert(i64_result_payload_key(name), payload_local);
        }
        locals.push(CraneliftI64Expr::Literal(0));
        assign_locals.push(payload_local);
    }
    Some(vec![CraneliftI64Stmt::CallAssign {
        locals: assign_locals,
        function: signature.function,
        args: lowered_args,
    }])
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
    let mut assign_locals = Vec::with_capacity(1 + payload_slots);
    let tag_local = local_indexes.len();
    local_indexes.insert(i64_enum_tag_key(name), tag_local);
    locals.push(CraneliftI64Expr::Literal(0));
    assign_locals.push(tag_local);
    for index in 0..payload_slots {
        let payload_local = local_indexes.len();
        local_indexes.insert(i64_enum_payload_slot_key(name, index), payload_local);
        if index == 0 {
            local_indexes.insert(i64_enum_payload_key(name), payload_local);
        }
        locals.push(CraneliftI64Expr::Literal(0));
        assign_locals.push(payload_local);
    }
    Some(vec![CraneliftI64Stmt::CallAssign {
        locals: assign_locals,
        function: signature.function,
        args: lowered_args,
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
    Some(vec![CraneliftI64Stmt::CallAssign {
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
    }])
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
) -> Option<CraneliftI64ReturnBlock> {
    let (terminal_stmt, body_stmts) = stmts.split_last()?;
    let mut stmts = Vec::new();
    for stmt in body_stmts {
        match stmt {
            Stmt::Let { .. } => {
                stmts.extend(lower_i64_runtime_let_stmts(
                    stmt,
                    locals,
                    &mut local_indexes,
                    &mut local_conditions,
                    helper_signatures,
                    static_bindings,
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
                )?);
            }
        }
    }
    let result = match terminal_stmt {
        Stmt::Return { expr, .. } => lower_i64_return_value_expr(
            expr,
            &local_indexes,
            &local_conditions,
            helper_signatures,
            static_bindings,
        )?,
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

fn lower_i64_exit_return(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<I64ExitBody> {
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
    _local_indexes: &HashMap<String, usize>,
    _local_conditions: &HashMap<String, CraneliftI64Condition>,
    _helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let message = i64_string_text(message, static_bindings)?;
    Some(vec![CraneliftI64Stmt::WriteLine {
        stream: OutputStream::Stderr,
        text: format!(
            "{{\"kind\":\"panic\",\"message\":{}}}",
            json_escape_string(&message)
        ),
    }])
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
    if !is_i64_option_local_payload_type_static(inner, static_bindings) {
        return None;
    }
    let tag = *local_indexes.get(i64_option_tag_key(name).as_str())?;
    let payloads = i64_option_payload_locals(name, inner.as_ref(), local_indexes, static_bindings)?;
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
    if !is_i64_option_local_payload_type_static(inner, static_bindings) {
        return None;
    }
    let tag = *local_indexes.get(i64_option_tag_key(name).as_str())?;
    let payloads = i64_option_payload_locals(name, inner.as_ref(), local_indexes, static_bindings)?;
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
            if let Some(condition) =
                lower_i64_map_contains_key_condition(name, args, static_bindings)
            {
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
        "string_starts_with" => {
            let [text, prefix] = args else {
                return None;
            };
            if let Some(condition) = lower_i64_map_key_array_string_index_starts_with_condition(
                text,
                prefix,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            ) {
                return Some(condition);
            }
            Some(CraneliftI64Condition::Literal(
                i64_string_text(text, static_bindings)?
                    .starts_with(i64_string_text(prefix, static_bindings)?.as_str()),
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
        name if is_i64_http_serve_once_name(name, static_bindings) => {
            let [bind, body] = args else {
                return None;
            };
            Some(CraneliftI64Condition::Literal(http_serve_once(
                &i64_string_text(bind, static_bindings)?,
                &i64_string_text(body, static_bindings)?,
            )))
        }
        name if is_i64_http_serve_route_name(name) => {
            let [bind, route_path, body, max_requests] = args else {
                return None;
            };
            Some(CraneliftI64Condition::Literal(http_serve_route(
                &i64_string_text(bind, static_bindings)?,
                &i64_string_text(route_path, static_bindings)?,
                &i64_string_text(body, static_bindings)?,
                i64_static_scalar_value(max_requests, static_bindings)?,
            )))
        }
        name if is_i64_crypto_constant_time_eq_name(name, static_bindings) => {
            let [left, right] = args else {
                return None;
            };
            Some(CraneliftI64Condition::Literal(constant_time_eq_bytes(
                i64_string_text(left, static_bindings)?.as_bytes(),
                i64_string_text(right, static_bindings)?.as_bytes(),
            )))
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
    let value = eval_block(&function.body, &functions, &mut env, &mut lines)
        .ok()
        .flatten()?;
    lines.is_empty().then_some(value)
}

fn i64_known_static_env(static_bindings: &I64StaticBindings) -> Option<SpikeEnv> {
    let mut env = SpikeEnv::new();
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
            | "string_clone"
            | "string_starts_with"
            | "string_strip_prefix"
            | "string_strip_suffix"
            | "string_trim"
            | "string_trim_start"
            | "string_line_at"
            | "encoding_url_component_encode"
            | "encoding_url_component_decode"
            | "encoding_path_segment_encode"
            | "encoding_url_query_pair_encode"
            | "encoding_path_join_segment"
            | "json_parse_int"
            | "json_parse_bool"
            | "json_parse_string"
            | "json_stringify_int"
            | "json_stringify_bool"
            | "json_stringify_string"
            | "json_serdes_parse"
            | "json_serdes_parse_str"
            | "json_serdes_value_to_json"
            | "json_serdes_to_json"
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
        "env_get" => {
            let [key] = args.as_slice() else {
                return None;
            };
            Some(std::env::var(i64_string_text(key, static_bindings)?).ok())
        }
        name if static_bindings.env_get_wrappers.contains(name) => {
            let [key] = args.as_slice() else {
                return None;
            };
            Some(std::env::var(i64_string_text(key, static_bindings)?).ok())
        }
        "fs_read" | "read_file" | "std_fs_read_file" => {
            let [path] = args.as_slice() else {
                return None;
            };
            let fs_root = static_bindings.fs_root.as_deref()?;
            Some(spike_fs_read_text_for_root(
                fs_root,
                &i64_string_text(path, static_bindings)?,
            ))
        }
        "net_resolve" | "resolve" | "std_net_resolve" => {
            let [host] = args.as_slice() else {
                return None;
            };
            Some(i64_net_resolve_text(&i64_string_text(
                host,
                static_bindings,
            )?))
        }
        name if is_i64_http_get_name(name, static_bindings) => {
            let [url] = args.as_slice() else {
                return None;
            };
            Some(http_get(&i64_string_text(url, static_bindings)?))
        }
        name if static_bindings.fs_read_wrappers.contains(name) => {
            let [path] = args.as_slice() else {
                return None;
            };
            let fs_root = static_bindings.fs_root.as_deref()?;
            Some(spike_fs_read_text_for_root(
                fs_root,
                &i64_string_text(path, static_bindings)?,
            ))
        }
        name if is_i64_json_parse_string_name(name, static_bindings) => {
            let [text] = args.as_slice() else {
                return None;
            };
            Some(json_parse_string(&i64_string_text(text, static_bindings)?))
        }
        "json_parse_value" => {
            let [text] = args.as_slice() else {
                return None;
            };
            Some(json_parse_value(&i64_string_text(text, static_bindings)?))
        }
        name if is_i64_json_parse_field_string_name(name, static_bindings) => {
            let [text, key] = args.as_slice() else {
                return None;
            };
            let text = i64_string_text(text, static_bindings)?;
            let key = i64_string_text(key, static_bindings)?;
            Some(json_object_field(&text, &key).and_then(|value| json_parse_string(&value)))
        }
        "json_parse_field_value" => {
            let [text, key] = args.as_slice() else {
                return None;
            };
            let text = i64_string_text(text, static_bindings)?;
            let key = i64_string_text(key, static_bindings)?;
            Some(json_object_field(&text, &key).and_then(|value| json_parse_value(&value)))
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
        name if is_i64_net_tcp_loopback_once_name(name) => {
            let [response, timeout_ms] = args.as_slice() else {
                return None;
            };
            let response = i64_string_text(response, static_bindings)?;
            let timeout = net_timeout(i64_static_scalar_value(timeout_ms, static_bindings)?);
            Some(net_tcp_listen_loopback_once(response, timeout))
        }
        name if is_i64_net_udp_loopback_once_name(name) => {
            let [response, timeout_ms] = args.as_slice() else {
                return None;
            };
            let response = i64_string_text(response, static_bindings)?;
            let timeout = net_timeout(i64_static_scalar_value(timeout_ms, static_bindings)?);
            Some(net_udp_bind_loopback_once(response, timeout))
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
        CraneliftI64Condition::And { .. } | CraneliftI64Condition::Or { .. } => None,
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
            lower_i64_clock_intrinsic_expr(name, args, static_bindings)
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

fn lower_i64_slice_projection_aliases(
    name: &str,
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    static_bindings: &I64StaticBindings,
    runtime: bool,
) -> Option<Vec<CraneliftI64Stmt>> {
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
    let mut assigns = Vec::new();
    for (slice_index, base_index) in (start..end).enumerate() {
        let base_key = i64_array_projection_key(base_name, base_index);
        let base_local = *local_indexes.get(base_key.as_str())?;
        let local = locals.len();
        if runtime {
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

fn i64_json_safe_string_len_key(name: &str) -> String {
    format!("{name}$json_safe_len")
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
        let elements = lower_i64_slice_local_call_arg_exprs(name, local_indexes)?;
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

fn lower_i64_clock_intrinsic_expr(
    name: &str,
    args: &[Expr],
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let milliseconds = match name {
        "clock_sleep_ms" => {
            let [milliseconds] = args else {
                return None;
            };
            i64_static_scalar_value(milliseconds, static_bindings)?
        }
        name if is_i64_time_sleep_name(name, static_bindings) => {
            let [duration] = args else {
                return None;
            };
            lower_i64_duration_ms_value(duration, static_bindings)?
        }
        _ => return None,
    };
    match milliseconds {
        value if value < 0 => Some(CraneliftI64Expr::Literal(-1)),
        0 => Some(CraneliftI64Expr::Literal(0)),
        _ => None,
    }
}

fn lower_i64_duration_ms_value(expr: &Expr, static_bindings: &I64StaticBindings) -> Option<i64> {
    match expr {
        Expr::Call { name, args, .. } if is_i64_time_duration_ms_name(name, static_bindings) => {
            let [milliseconds] = args.as_slice() else {
                return None;
            };
            i64_static_scalar_value(milliseconds, static_bindings)
        }
        Expr::StructLiteral { fields, .. } => fields
            .iter()
            .find(|field| field.name == "ms")
            .and_then(|field| i64_static_scalar_value(&field.expr, static_bindings)),
        _ => None,
    }
}

fn lower_i64_process_intrinsic_expr(
    name: &str,
    args: &[Expr],
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if name != "process_status" && !static_bindings.process_status_wrappers.contains(name) {
        return None;
    }
    let [command] = args else {
        return None;
    };
    match i64_string_text(command, static_bindings)?.as_str() {
        "/usr/bin/true" => Some(CraneliftI64Expr::Literal(0)),
        "/usr/bin/false" => Some(CraneliftI64Expr::Literal(1)),
        "__axiom_stage1_missing_binary__" => Some(CraneliftI64Expr::Literal(-1)),
        _ => None,
    }
}

fn lower_i64_fs_write_intrinsic_expr(
    name: &str,
    args: &[Expr],
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let name = i64_fs_write_intrinsic_name(name, static_bindings)?;
    Some(CraneliftI64Expr::Literal(i64_fs_write_result(
        name,
        args,
        static_bindings,
    )?))
}

fn lower_i64_crypto_random_intrinsic_expr(
    name: &str,
    args: &[Expr],
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if !is_i64_crypto_random_u64_name(name, static_bindings) || !args.is_empty() {
        return None;
    }
    let bytes: [u8; 8] = crypto_random_bytes(8).ok()?.try_into().ok()?;
    Some(CraneliftI64Expr::Literal(i64::from_ne_bytes(bytes)))
}

fn lower_i64_crypto_random_bytes_len_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if !is_i64_crypto_random_bytes_name(name, static_bindings) {
        return None;
    }
    let [length] = args.as_slice() else {
        return None;
    };
    let arg_value = i64_static_scalar_value(length, static_bindings)
        .map(|length| format!("int:{length}"))
        .unwrap_or_else(|| "int".to_string());
    let length = lower_i64_expr(
        length,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    i64_audited_crypto_expr(
        "crypto_rand_bytes",
        "length",
        arg_value,
        CraneliftI64Expr::RandomBytesLen {
            length: Box::new(length),
        },
        static_bindings,
        CraneliftI64AuditSuccess::NonNegative,
    )
    let length = i64_static_scalar_value(length, static_bindings)?;
    if !(0..=65_536).contains(&length) {
        return None;
    }
    Some(CraneliftI64Expr::Literal(length))
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
        let len = text
            .as_bytes()
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(text.len());
        return Some(CraneliftI64Expr::Literal(len as i64));
    }
    lower_i64_string_len_expr(
        value,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
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
    let key = lower_i64_map_key_expr(key, static_bindings)?;
    let mut selected = None;
    for entry in entries.iter().rev() {
        if lower_i64_map_key_expr(&entry.key, static_bindings)? == key {
            selected = Some(&entry.value);
            break;
        }
    }
    lower_i64_expr(
        selected.unwrap_or(default),
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )
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
    let key = lower_i64_map_key_expr(key, static_bindings)?;
    for entry in entries.iter().rev() {
        if lower_i64_map_key_expr(&entry.key, static_bindings)? == key {
            return Some(CraneliftI64Condition::Literal(true));
        }
    }
    Some(CraneliftI64Condition::Literal(false))
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

fn is_i64_std_fs_read_wrapper(function: &Function) -> bool {
    function.path == "<stdlib>/fs.ax" && function.source_name == "read_file"
}

fn i64_std_fs_write_intrinsic(function: &Function) -> Option<&'static str> {
    if function.path != "<stdlib>/fs.ax" {
        return None;
    }
    match function.source_name.as_str() {
        "write_file" => Some("fs_write"),
        "create_file" => Some("fs_create"),
        "append_file" => Some("fs_append"),
        "mkdir" => Some("fs_mkdir"),
        "mkdir_all" => Some("fs_mkdir_all"),
        "remove_file" => Some("fs_remove_file"),
        "remove_dir" => Some("fs_remove_dir"),
        "replace_file" => Some("fs_replace"),
        _ => None,
    }
}

fn i64_fs_write_intrinsic_name<'a>(
    name: &'a str,
    static_bindings: &'a I64StaticBindings,
) -> Option<&'a str> {
    match name {
        "fs_write" | "fs_create" | "fs_append" | "fs_mkdir" | "fs_mkdir_all" | "fs_remove_file"
        | "fs_remove_dir" | "fs_replace" => Some(name),
        "write_file" | "std_fs_write_file" => Some("fs_write"),
        "create_file" | "std_fs_create_file" => Some("fs_create"),
        "append_file" | "std_fs_append_file" => Some("fs_append"),
        "mkdir" | "std_fs_mkdir" => Some("fs_mkdir"),
        "mkdir_all" | "std_fs_mkdir_all" => Some("fs_mkdir_all"),
        "remove_file" | "std_fs_remove_file" => Some("fs_remove_file"),
        "remove_dir" | "std_fs_remove_dir" => Some("fs_remove_dir"),
        "replace_file" | "std_fs_replace_file" => Some("fs_replace"),
        _ => static_bindings
            .fs_write_wrappers
            .get(name)
            .map(String::as_str),
    }
}

fn is_i64_std_time_wrapper(function: &Function, source_name: &str) -> bool {
    function.path == "<stdlib>/time.ax" && function.source_name == source_name
}

fn is_i64_time_duration_ms_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.time_duration_ms_wrappers.contains(name)
}

fn is_i64_time_sleep_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.time_sleep_wrappers.contains(name)
}

fn is_i64_std_fs_shim_wrapper(function: &Function) -> bool {
    matches!(
        (function.path.as_str(), function.source_name.as_str()),
        (
            "<stdlib>/fs.ax",
            "read_file"
                | "write_file"
                | "create_file"
                | "append_file"
                | "mkdir"
                | "mkdir_all"
                | "remove_file"
                | "remove_dir"
                | "replace_file"
        )
    )
}

fn is_i64_std_net_shim_wrapper(function: &Function) -> bool {
    matches!(
        (function.path.as_str(), function.source_name.as_str()),
        (
            "<stdlib>/net.ax",
            "resolve"
                | "tcp_listen_loopback_once"
                | "tcp_dial"
                | "udp_bind_loopback_once"
                | "udp_send_recv"
        )
    )
}

fn is_i64_net_tcp_loopback_once_name(name: &str) -> bool {
    matches!(
        name,
        "net_tcp_listen_loopback_once"
            | "tcp_listen_loopback_once"
            | "std_net_tcp_listen_loopback_once"
    )
}

fn is_i64_net_udp_loopback_once_name(name: &str) -> bool {
    matches!(
        name,
        "net_udp_bind_loopback_once" | "udp_bind_loopback_once" | "std_net_udp_bind_loopback_once"
    )
}

fn is_i64_http_get_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    matches!(name, "http_get" | "get" | "std_http_get")
        || static_bindings.http_get_wrappers.contains(name)
}

fn is_i64_http_serve_once_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    matches!(
        name,
        "http_serve_once" | "serve_once" | "std_http_serve_once"
    ) || static_bindings.http_serve_once_wrappers.contains(name)
}

fn is_i64_http_serve_route_name(name: &str) -> bool {
    name == "http_serve_route"
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

fn is_i64_std_json_wrapper(function: &Function, source_name: &str) -> bool {
    function.path == "<stdlib>/json.ax" && function.source_name == source_name
}

fn is_i64_json_parse_int_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "json_parse_int" || static_bindings.json_parse_int_wrappers.contains(name)
}

fn is_i64_json_parse_bool_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "json_parse_bool" || static_bindings.json_parse_bool_wrappers.contains(name)
}

fn is_i64_json_parse_string_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "json_parse_string" || static_bindings.json_parse_string_wrappers.contains(name)
}

fn is_i64_json_parse_field_int_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "json_parse_field_int" || static_bindings.json_parse_field_int_wrappers.contains(name)
}

fn is_i64_json_parse_field_bool_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "json_parse_field_bool"
        || static_bindings
            .json_parse_field_bool_wrappers
            .contains(name)
}

fn is_i64_json_parse_field_string_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "json_parse_field_string"
        || static_bindings
            .json_parse_field_string_wrappers
            .contains(name)
}

fn is_i64_json_stringify_int_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "json_stringify_int" || static_bindings.json_stringify_int_wrappers.contains(name)
}

fn is_i64_json_stringify_bool_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "json_stringify_bool" || static_bindings.json_stringify_bool_wrappers.contains(name)
}

fn is_i64_json_stringify_string_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "json_stringify_string"
        || static_bindings
            .json_stringify_string_wrappers
            .contains(name)
}

fn is_i64_std_log_wrapper(function: &Function, source_name: &str) -> bool {
    function.path == "<stdlib>/log.ax" && function.source_name == source_name
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

fn is_i64_std_crypto_wrapper(function: &Function, source_name: &str) -> bool {
    matches!(
        function.path.as_str(),
        "<stdlib>/crypto_hash.ax"
            | "<stdlib>/crypto_mac.ax"
            | "<stdlib>/crypto_rand.ax"
            | "<stdlib>/crypto.ax"
    ) && function.source_name == source_name
}

fn is_i64_crypto_sha256_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "crypto_sha256" || static_bindings.crypto_sha256_wrappers.contains(name)
}

fn is_i64_crypto_hmac_sha256_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "crypto_hmac_sha256" || static_bindings.crypto_hmac_sha256_wrappers.contains(name)
}

fn is_i64_crypto_hmac_sha512_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "crypto_hmac_sha512" || static_bindings.crypto_hmac_sha512_wrappers.contains(name)
}

fn is_i64_crypto_constant_time_eq_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "crypto_constant_time_eq"
        || static_bindings
            .crypto_constant_time_eq_wrappers
            .contains(name)
}

fn is_i64_crypto_constant_time_eq_u8_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "crypto_constant_time_eq_u8"
        || static_bindings
            .crypto_constant_time_eq_u8_wrappers
            .contains(name)
}

fn is_i64_crypto_verify_sha256_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.crypto_verify_sha256_wrappers.contains(name)
}

fn is_i64_crypto_verify_sha512_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.crypto_verify_sha512_wrappers.contains(name)
}

fn is_i64_crypto_random_bytes_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "crypto_rand_bytes" || static_bindings.crypto_random_bytes_wrappers.contains(name)
}

fn is_i64_crypto_random_u64_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "crypto_rand_u64" || static_bindings.crypto_random_u64_wrappers.contains(name)
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
    local_conditions: &HashMap<String, CraneliftI64Condition>,
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

fn lower_i64_string_len_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if let Some(value) = i64_string_text(expr, static_bindings) {
        return Some(CraneliftI64Expr::Literal(value.len() as i64));
    }
    match expr {
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
            Some(CraneliftI64Expr::Literal(64))
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
            Some(CraneliftI64Expr::Literal(64))
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
            Some(CraneliftI64Expr::Literal(128))
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
        _ => None,
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

fn lower_i64_json_safe_string_len_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    match expr {
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
        Expr::VarRef {
            name,
            ty: Type::String | Type::Str,
        } => local_indexes
            .get(i64_json_safe_string_len_key(name).as_str())
            .copied()
            .map(CraneliftI64Expr::Local),
        Expr::BinaryAdd {
            op: ArithmeticOp::Add,
            lhs,
            rhs,
            ty: Type::String | Type::Str,
        } => Some(CraneliftI64Expr::Binary {
            op: CraneliftI64BinaryOp::Add,
            lhs: Box::new(lower_i64_json_safe_string_len_expr(
                lhs,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?),
            rhs: Box::new(lower_i64_json_safe_string_len_expr(
                rhs,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?),
        }),
        _ => None,
    }
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
        Expr::Await { expr, .. } => await_spike_task(eval_expr(expr, functions, env, lines)?),
        Expr::StringBorrow { expr, .. } => eval_expr(expr, functions, env, lines),
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
    if is_assert_call(name) {
        return eval_assert_call(name, args, functions, env, lines);
    }
    if name == "len" {
        return eval_len_call(args, functions, env, lines);
    }
    if name == "first" || name == "last" {
        return eval_first_last_call(name, args, functions, env, lines);
    }
    if name == "contains" || name == "map_contains_key" {
        return eval_map_contains_call(args, functions, env, lines);
    }
    if name == "map_get" || name == "get" {
        return eval_map_get_call(args, functions, env, lines);
    }
    if name == "get_or_default" {
        return eval_map_get_or_default_call(args, functions, env, lines);
    }
    if name == "keys" || name == "map_keys" {
        return eval_map_keys_call(args, functions, env, lines);
    }
    if name == "io_eprintln" {
        return eval_io_eprintln_call(args, functions, env, lines);
    }
    if is_json_call(name) {
        return eval_json_call(name, args, functions, env, lines);
    }
    if is_json_serdes_call(name) {
        return eval_json_serdes_call(name, args, functions, env, lines);
    }
    if is_crypto_call(name) {
        return eval_crypto_call(name, args, functions, env, lines);
    }
    if is_encoding_call(name) {
        return eval_encoding_call(name, args, functions, env, lines);
    }
    if is_string_call(name) {
        return eval_string_call(name, args, functions, env, lines);
    }
    if is_async_call(name) {
        return eval_async_call(name, args, functions, env, lines);
    }
    if is_cli_call(name) {
        return eval_cli_call(name, args, functions, env, lines);
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
    if is_net_call(name) {
        return eval_net_call(name, args, functions, env, lines);
    }
    if is_http_call(name) {
        return eval_http_call(name, args, functions, env, lines);
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
    if function.is_extern {
        return eval_extern_call(function, args, functions, env, lines);
    }
    let mut local_env = env.clone();
    for (param, arg) in function.params.iter().zip(args) {
        local_env.insert(param.name.clone(), eval_expr(arg, functions, env, lines)?);
    }
    let returned = eval_block(&function.body, functions, &mut local_env, lines)?
        .ok_or_else(|| unsupported("cranelift spike functions must return a value"))?;
    if function.is_async {
        Ok(spike_task(returned))
    } else {
        Ok(returned)
    }
}

fn is_assert_call(name: &str) -> bool {
    matches!(
        name,
        "assert_true"
            | "assert_property"
            | "assert_snapshot"
            | "assert_contains"
            | "assert_eq"
            | "assert_case_eq"
            | "assert_ne"
    )
}

fn eval_assert_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "assert_true" => {
            let [condition, _line, _column] = args else {
                return Err(unsupported(
                    "assert_true expects condition, line, and column",
                ));
            };
            let condition = expect_bool(eval_expr(condition, functions, env, lines)?)?;
            assert_result(condition, "assert_true failed")
        }
        "assert_property" => {
            let [_label, condition, _line, _column] = args else {
                return Err(unsupported(
                    "assert_property expects label, condition, line, and column",
                ));
            };
            let condition = expect_bool(eval_expr(condition, functions, env, lines)?)?;
            assert_result(condition, "assert_property failed")
        }
        "assert_snapshot" => {
            let [_label, actual, expected, _line, _column] = args else {
                return Err(unsupported(
                    "assert_snapshot expects label, actual, expected, line, and column",
                ));
            };
            let actual = expect_text(eval_expr(actual, functions, env, lines)?, name)?;
            let expected = expect_text(eval_expr(expected, functions, env, lines)?, name)?;
            assert_result(actual == expected, "assert_snapshot failed")
        }
        "assert_contains" => {
            let [haystack, needle, _line, _column] = args else {
                return Err(unsupported(
                    "assert_contains expects haystack, needle, line, and column",
                ));
            };
            let haystack = expect_text(eval_expr(haystack, functions, env, lines)?, name)?;
            let needle = expect_text(eval_expr(needle, functions, env, lines)?, name)?;
            assert_result(haystack.contains(&needle), "assert_contains failed")
        }
        "assert_eq" | "assert_ne" => {
            let [left, right, _line, _column] = args else {
                return Err(unsupported(
                    "assert_eq/assert_ne expects left, right, line, and column",
                ));
            };
            let left = eval_expr(left, functions, env, lines)?;
            let right = eval_expr(right, functions, env, lines)?;
            let equal = spike_values_equal(&left, &right)?;
            assert_result(equal == (name == "assert_eq"), &format!("{name} failed"))
        }
        "assert_case_eq" => {
            let [_label, left, right, _line, _column] = args else {
                return Err(unsupported(
                    "assert_case_eq expects label, left, right, line, and column",
                ));
            };
            let left = eval_expr(left, functions, env, lines)?;
            let right = eval_expr(right, functions, env, lines)?;
            assert_result(spike_values_equal(&left, &right)?, "assert_case_eq failed")
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike assertion call {name:?}"
        ))),
    }
}

fn assert_result(condition: bool, message: &str) -> Result<SpikeValue, Diagnostic> {
    if condition {
        Ok(SpikeValue::Int(0))
    } else {
        Err(unsupported(message))
    }
}

fn eval_extern_call(
    function: &Function,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match (
        function.source_name.as_str(),
        function.extern_abi.as_deref(),
        function.extern_library.as_deref(),
    ) {
        ("strlen", Some("C"), Some("c")) => {
            let [value] = args else {
                return Err(unsupported("strlen expects exactly one argument"));
            };
            let value = expect_text(eval_expr(value, functions, env, lines)?, "strlen")?;
            let len = value
                .as_bytes()
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(value.len());
            Ok(SpikeValue::Int(len as i64))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike extern call {:?} from {:?}",
            function.name, function.extern_library
        ))),
    }
}

fn is_cli_call(name: &str) -> bool {
    matches!(name, "cli_args" | "cli_arg_count" | "cli_arg")
}

fn eval_cli_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "cli_args" => {
            let [] = args else {
                return Err(unsupported("cli_args expects no arguments"));
            };
            Ok(SpikeValue::Array(Vec::new()))
        }
        "cli_arg_count" => {
            let [] = args else {
                return Err(unsupported("cli_arg_count expects no arguments"));
            };
            Ok(SpikeValue::Int(0))
        }
        "cli_arg" => {
            let [index] = args else {
                return Err(unsupported("cli_arg expects exactly one argument"));
            };
            let _index = expect_signed_integer(eval_expr(index, functions, env, lines)?)?;
            Ok(spike_option(None))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike CLI call {name:?}"
        ))),
    }
}

fn is_async_call(name: &str) -> bool {
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

fn eval_async_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "async_ready" => {
            let [value] = args else {
                return Err(unsupported("async_ready expects exactly one argument"));
            };
            eval_expr(value, functions, env, lines).map(spike_task)
        }
        "async_spawn" => {
            let [task] = args else {
                return Err(unsupported("async_spawn expects exactly one argument"));
            };
            let task = eval_expr(task, functions, env, lines)?;
            expect_task_value(&task, "async_spawn")?;
            Ok(SpikeValue::JoinHandle(Box::new(task)))
        }
        "async_join" => {
            let [handle] = args else {
                return Err(unsupported("async_join expects exactly one argument"));
            };
            match eval_expr(handle, functions, env, lines)? {
                SpikeValue::JoinHandle(task) => await_spike_task(*task).map(spike_task),
                _ => Err(unsupported("async_join expects a join handle")),
            }
        }
        "async_cancel" => {
            let [task] = args else {
                return Err(unsupported("async_cancel expects exactly one argument"));
            };
            match eval_expr(task, functions, env, lines)? {
                SpikeValue::Task { value, .. } => Ok(SpikeValue::Task {
                    value,
                    canceled: true,
                }),
                _ => Err(unsupported("async_cancel expects a task")),
            }
        }
        "async_is_canceled" => {
            let [task] = args else {
                return Err(unsupported(
                    "async_is_canceled expects exactly one argument",
                ));
            };
            match eval_expr(task, functions, env, lines)? {
                SpikeValue::Task { canceled, .. } => Ok(SpikeValue::Bool(canceled)),
                _ => Err(unsupported("async_is_canceled expects a task")),
            }
        }
        "async_timeout" => {
            let [task, _milliseconds] = args else {
                return Err(unsupported("async_timeout expects exactly two arguments"));
            };
            match eval_expr(task, functions, env, lines)? {
                SpikeValue::Task { canceled: true, .. } => Ok(spike_task(spike_option(None))),
                task @ SpikeValue::Task { .. } => {
                    await_spike_task(task).map(|value| spike_task(spike_option(Some(value))))
                }
                _ => Err(unsupported("async_timeout expects a task")),
            }
        }
        "async_channel" => {
            let [] = args else {
                return Err(unsupported("async_channel expects no arguments"));
            };
            Ok(SpikeValue::AsyncChannel { slot: None })
        }
        "async_send" => {
            let [channel, value] = args else {
                return Err(unsupported("async_send expects exactly two arguments"));
            };
            match eval_expr(channel, functions, env, lines)? {
                SpikeValue::AsyncChannel { slot: None } => {
                    let value = eval_expr(value, functions, env, lines)?;
                    Ok(spike_task(SpikeValue::AsyncChannel {
                        slot: Some(Box::new(value)),
                    }))
                }
                SpikeValue::AsyncChannel { slot: Some(_) } => {
                    Err(unsupported("async_send on a full channel"))
                }
                _ => Err(unsupported("async_send expects an async channel")),
            }
        }
        "async_recv" => {
            let [channel] = args else {
                return Err(unsupported("async_recv expects exactly one argument"));
            };
            match eval_expr(channel, functions, env, lines)? {
                SpikeValue::AsyncChannel { slot } => {
                    Ok(spike_task(spike_option(slot.map(|value| *value))))
                }
                _ => Err(unsupported("async_recv expects an async channel")),
            }
        }
        "async_select" => {
            let [left, right] = args else {
                return Err(unsupported("async_select expects exactly two arguments"));
            };
            let left = await_spike_task(eval_expr(left, functions, env, lines)?)?;
            if option_payload(&left)?.is_some() {
                return Ok(spike_task(SpikeValue::SelectResult {
                    selected: 0,
                    value: option_payload(&left)?.map(Box::new),
                }));
            }
            let right = await_spike_task(eval_expr(right, functions, env, lines)?)?;
            Ok(spike_task(SpikeValue::SelectResult {
                selected: 1,
                value: option_payload(&right)?.map(Box::new),
            }))
        }
        "async_selected" => {
            let [result] = args else {
                return Err(unsupported("async_selected expects exactly one argument"));
            };
            match eval_expr(result, functions, env, lines)? {
                SpikeValue::SelectResult { selected, .. } => Ok(SpikeValue::Int(selected)),
                _ => Err(unsupported("async_selected expects a select result")),
            }
        }
        "async_selected_value" => {
            let [result] = args else {
                return Err(unsupported(
                    "async_selected_value expects exactly one argument",
                ));
            };
            match eval_expr(result, functions, env, lines)? {
                SpikeValue::SelectResult { value, .. } => {
                    Ok(spike_option(value.map(|value| *value)))
                }
                _ => Err(unsupported("async_selected_value expects a select result")),
            }
        }
        _ => Err(unsupported("unknown async runtime intrinsic")),
    }
}

fn spike_task(value: SpikeValue) -> SpikeValue {
    SpikeValue::Task {
        value: Some(Box::new(value)),
        canceled: false,
    }
}

fn expect_task_value(value: &SpikeValue, name: &str) -> Result<(), Diagnostic> {
    match value {
        SpikeValue::Task { .. } => Ok(()),
        _ => Err(unsupported(&format!("{name} expects a task"))),
    }
}

fn await_spike_task(value: SpikeValue) -> Result<SpikeValue, Diagnostic> {
    match value {
        SpikeValue::Task {
            value: Some(value),
            canceled: false,
        } => Ok(*value),
        SpikeValue::Task { canceled: true, .. } => Err(unsupported("awaited task was canceled")),
        SpikeValue::Task { value: None, .. } => {
            Err(unsupported("task had no value or scheduled body"))
        }
        _ => Err(unsupported("await expects a task")),
    }
}

fn option_payload(value: &SpikeValue) -> Result<Option<SpikeValue>, Diagnostic> {
    match value {
        SpikeValue::Enum {
            enum_name,
            variant,
            payloads,
            ..
        } if enum_name == "Option" && variant == "Some" && payloads.len() == 1 => {
            Ok(Some(payloads[0].clone()))
        }
        SpikeValue::Enum {
            enum_name,
            variant,
            payloads,
            ..
        } if enum_name == "Option" && variant == "None" && payloads.is_empty() => Ok(None),
        _ => Err(unsupported("expected an Option value")),
    }
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

fn eval_map_get_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [map, key] = args else {
        return Err(unsupported("map_get/get expects exactly two arguments"));
    };
    let entries = match eval_expr(map, functions, env, lines)? {
        SpikeValue::Map(entries) => entries,
        _ => return Err(unsupported("map_get/get expects a map value")),
    };
    let key = eval_expr(key, functions, env, lines)?;
    validate_map_key(&key)?;
    for (candidate, value) in entries {
        if map_keys_equal(&candidate, &key)? {
            return Ok(spike_option(Some(value)));
        }
    }
    Ok(spike_option(None))
}

fn eval_map_get_or_default_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [map, key, default] = args else {
        return Err(unsupported(
            "get_or_default expects exactly three arguments",
        ));
    };
    let entries = match eval_expr(map, functions, env, lines)? {
        SpikeValue::Map(entries) => entries,
        _ => return Err(unsupported("get_or_default expects a map value")),
    };
    let key = eval_expr(key, functions, env, lines)?;
    validate_map_key(&key)?;
    for (candidate, value) in entries {
        if map_keys_equal(&candidate, &key)? {
            return Ok(value);
        }
    }
    eval_expr(default, functions, env, lines)
}

fn eval_map_keys_call(
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    let [map] = args else {
        return Err(unsupported("map_keys expects exactly one argument"));
    };
    let entries = match eval_expr(map, functions, env, lines)? {
        SpikeValue::Map(entries) => entries,
        _ => return Err(unsupported("map_keys expects a map value")),
    };
    entries
        .into_iter()
        .map(|(key, _)| {
            validate_map_key(&key)?;
            Ok(key)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(SpikeValue::Array)
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

fn is_json_serdes_call(name: &str) -> bool {
    matches!(
        name,
        "json_serdes_parse"
            | "json_serdes_parse_str"
            | "json_serdes_value_to_json"
            | "json_serdes_to_json"
    )
}

fn eval_json_serdes_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "json_serdes_parse" | "json_serdes_parse_str" => {
            let text = eval_json_unary_text(name, args, functions, env, lines)?;
            Ok(json_serdes_result(json_serdes_parse_document(&text)))
        }
        "json_serdes_value_to_json" => {
            let value = eval_json_unary(name, args, functions, env, lines)?;
            Ok(SpikeValue::Text(json_serdes_value_to_json(&value)?))
        }
        "json_serdes_to_json" => {
            let value = eval_json_unary(name, args, functions, env, lines)?;
            let SpikeValue::Map(entries) = value else {
                return Err(unsupported("json_serdes_to_json expects an object map"));
            };
            Ok(SpikeValue::Text(json_serdes_object_to_json(&entries)?))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike JSON serdes call {name:?}"
        ))),
    }
}

fn json_serdes_result(value: Result<SpikeValue, String>) -> SpikeValue {
    match value {
        Ok(value) => SpikeValue::Enum {
            enum_name: String::from("Result"),
            variant: String::from("Ok"),
            field_names: Vec::new(),
            payloads: vec![value],
        },
        Err(message) => SpikeValue::Enum {
            enum_name: String::from("Result"),
            variant: String::from("Err"),
            field_names: Vec::new(),
            payloads: vec![SpikeValue::Struct {
                name: String::from("std_serdes_ParseError"),
                fields: vec![(String::from("message"), SpikeValue::Text(message))],
            }],
        },
    }
}

fn json_serdes_parse_document(text: &str) -> Result<SpikeValue, String> {
    let (value, index) = json_serdes_parse_value(text, json_skip_ws(text, 0))?;
    if json_skip_ws(text, index) == text.len() {
        Ok(value)
    } else {
        Err(String::from("trailing characters after JSON value"))
    }
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
        ("Text", [SpikeValue::Text(value)]) => Ok(json_escape_string(value)),
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
                    json_escape_string(key),
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

fn is_crypto_call(name: &str) -> bool {
    matches!(
        name,
        "crypto_sha256"
            | "crypto_hmac_sha256"
            | "crypto_hmac_sha512"
            | "crypto_constant_time_eq"
            | "crypto_constant_time_eq_u8"
            | "crypto_rand_bytes"
            | "crypto_rand_u64"
            | "crypto_ed25519_keygen"
            | "crypto_ed25519_sign"
            | "crypto_ed25519_verify"
            | "crypto_aead_seal"
            | "crypto_aead_open"
    )
}

fn is_encoding_call(name: &str) -> bool {
    matches!(
        name,
        "encoding_url_component_encode"
            | "encoding_url_component_decode"
            | "encoding_path_segment_encode"
            | "encoding_url_query_pair_encode"
            | "encoding_path_join_segment"
    )
}

fn is_string_call(name: &str) -> bool {
    matches!(
        name,
        "string_line_at"
            | "string_clone"
            | "string_starts_with"
            | "string_strip_prefix"
            | "string_strip_suffix"
            | "string_trim"
            | "string_trim_start"
    )
}

fn eval_string_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "string_line_at" => {
            let [text, index] = args else {
                return Err(unsupported("string_line_at expects exactly two arguments"));
            };
            let text = expect_text(eval_expr(text, functions, env, lines)?, name)?;
            let index = expect_int(eval_expr(index, functions, env, lines)?)?;
            let line = if index < 0 {
                None
            } else {
                text.lines()
                    .nth(index as usize)
                    .map(|line| SpikeValue::Text(line.to_string()))
            };
            Ok(spike_option(line))
        }
        "string_clone" => {
            let [text] = args else {
                return Err(unsupported("string_clone expects exactly one argument"));
            };
            let text = expect_text(eval_expr(text, functions, env, lines)?, name)?;
            Ok(SpikeValue::Text(text))
        }
        "string_starts_with" => {
            let [text, prefix] = args else {
                return Err(unsupported(
                    "string_starts_with expects exactly two arguments",
                ));
            };
            let text = expect_text(eval_expr(text, functions, env, lines)?, name)?;
            let prefix = expect_text(eval_expr(prefix, functions, env, lines)?, name)?;
            Ok(SpikeValue::Bool(text.starts_with(&prefix)))
        }
        "string_strip_prefix" => {
            let [text, prefix] = args else {
                return Err(unsupported(
                    "string_strip_prefix expects exactly two arguments",
                ));
            };
            let text = expect_text(eval_expr(text, functions, env, lines)?, name)?;
            let prefix = expect_text(eval_expr(prefix, functions, env, lines)?, name)?;
            Ok(spike_option(
                text.strip_prefix(&prefix)
                    .map(|rest| SpikeValue::Text(rest.to_string())),
            ))
        }
        "string_strip_suffix" => {
            let [text, suffix] = args else {
                return Err(unsupported(
                    "string_strip_suffix expects exactly two arguments",
                ));
            };
            let text = expect_text(eval_expr(text, functions, env, lines)?, name)?;
            let suffix = expect_text(eval_expr(suffix, functions, env, lines)?, name)?;
            Ok(spike_option(
                text.strip_suffix(&suffix)
                    .map(|rest| SpikeValue::Text(rest.to_string())),
            ))
        }
        "string_trim" | "string_trim_start" => {
            let [text] = args else {
                return Err(unsupported(&format!("{name} expects exactly one argument")));
            };
            let text = expect_text(eval_expr(text, functions, env, lines)?, name)?;
            let trimmed = if name == "string_trim" {
                text.trim()
            } else {
                text.trim_start()
            };
            Ok(SpikeValue::Text(trimmed.to_string()))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike string call {name:?}"
        ))),
    }
}

fn eval_encoding_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "encoding_url_component_encode" | "encoding_path_segment_encode" => {
            let [value] = args else {
                return Err(unsupported(&format!("{name} expects exactly one argument")));
            };
            let value = expect_text(eval_expr(value, functions, env, lines)?, name)?;
            Ok(SpikeValue::Text(percent_encode(&value)))
        }
        "encoding_url_component_decode" => {
            let [value] = args else {
                return Err(unsupported(
                    "encoding_url_component_decode expects exactly one argument",
                ));
            };
            let value = expect_text(eval_expr(value, functions, env, lines)?, name)?;
            Ok(spike_option(percent_decode(&value).map(SpikeValue::Text)))
        }
        "encoding_url_query_pair_encode" => {
            let [key, value] = args else {
                return Err(unsupported(
                    "encoding_url_query_pair_encode expects exactly two arguments",
                ));
            };
            let key = expect_text(eval_expr(key, functions, env, lines)?, name)?;
            let value = expect_text(eval_expr(value, functions, env, lines)?, name)?;
            Ok(SpikeValue::Text(format!(
                "{}={}",
                percent_encode(&key),
                percent_encode(&value)
            )))
        }
        "encoding_path_join_segment" => {
            let [base, segment] = args else {
                return Err(unsupported(
                    "encoding_path_join_segment expects exactly two arguments",
                ));
            };
            let base = expect_text(eval_expr(base, functions, env, lines)?, name)?;
            let segment = expect_text(eval_expr(segment, functions, env, lines)?, name)?;
            let encoded = percent_encode(&segment);
            let joined = if base.is_empty() {
                encoded
            } else if base.ends_with('/') {
                format!("{base}{encoded}")
            } else {
                format!("{base}/{encoded}")
            };
            Ok(SpikeValue::Text(joined))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike encoding call {name:?}"
        ))),
    }
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
        "crypto_rand_bytes" => {
            let [length] = args else {
                return Err(unsupported(
                    "crypto_rand_bytes expects exactly one argument",
                ));
            };
            let length = expect_int(eval_expr(length, functions, env, lines)?)?;
            let bytes = crypto_random_bytes(length)?;
            Ok(SpikeValue::Array(
                bytes
                    .into_iter()
                    .map(|value| SpikeValue::UInt(value as u64))
                    .collect(),
            ))
        }
        "crypto_rand_u64" => {
            let [] = args else {
                return Err(unsupported("crypto_rand_u64 expects no arguments"));
            };
            let bytes = crypto_random_bytes(8)?;
            let value = u64::from_ne_bytes(
                bytes
                    .try_into()
                    .map_err(|_| unsupported("crypto_rand_u64 expected 8 random bytes"))?,
            );
            Ok(SpikeValue::UInt(value))
        }
        "crypto_ed25519_keygen" => {
            let [] = args else {
                return Err(unsupported("crypto_ed25519_keygen expects no arguments"));
            };
            let (public_key, secret_key) = crypto_ed25519_keygen()?;
            Ok(SpikeValue::Tuple(vec![
                spike_u8_array(public_key),
                spike_u8_array(secret_key),
            ]))
        }
        "crypto_ed25519_sign" => {
            let [secret_key, message] = args else {
                return Err(unsupported(
                    "crypto_ed25519_sign expects exactly two arguments",
                ));
            };
            let secret_key = expect_u8_array(eval_expr(secret_key, functions, env, lines)?, name)?;
            let message = expect_u8_array(eval_expr(message, functions, env, lines)?, name)?;
            Ok(spike_u8_array(crypto_ed25519_sign(&secret_key, &message)?))
        }
        "crypto_ed25519_verify" => {
            let [public_key, message, signature] = args else {
                return Err(unsupported(
                    "crypto_ed25519_verify expects exactly three arguments",
                ));
            };
            let public_key = expect_u8_array(eval_expr(public_key, functions, env, lines)?, name)?;
            let message = expect_u8_array(eval_expr(message, functions, env, lines)?, name)?;
            let signature = expect_u8_array(eval_expr(signature, functions, env, lines)?, name)?;
            Ok(SpikeValue::Bool(crypto_ed25519_verify(
                &public_key,
                &message,
                &signature,
            )?))
        }
        "crypto_aead_seal" => {
            let [alg, key, nonce, aad, plaintext] = args else {
                return Err(unsupported(
                    "crypto_aead_seal expects exactly five arguments",
                ));
            };
            let alg = expect_text(eval_expr(alg, functions, env, lines)?, name)?;
            let key = expect_u8_array(eval_expr(key, functions, env, lines)?, name)?;
            let nonce = expect_u8_array(eval_expr(nonce, functions, env, lines)?, name)?;
            let aad = expect_u8_array(eval_expr(aad, functions, env, lines)?, name)?;
            let plaintext = expect_u8_array(eval_expr(plaintext, functions, env, lines)?, name)?;
            Ok(spike_u8_array(crypto_aead_seal(
                &alg, &key, &nonce, &aad, &plaintext,
            )?))
        }
        "crypto_aead_open" => {
            let [alg, key, nonce, aad, ciphertext] = args else {
                return Err(unsupported(
                    "crypto_aead_open expects exactly five arguments",
                ));
            };
            let alg = expect_text(eval_expr(alg, functions, env, lines)?, name)?;
            let key = expect_u8_array(eval_expr(key, functions, env, lines)?, name)?;
            let nonce = expect_u8_array(eval_expr(nonce, functions, env, lines)?, name)?;
            let aad = expect_u8_array(eval_expr(aad, functions, env, lines)?, name)?;
            let ciphertext = expect_u8_array(eval_expr(ciphertext, functions, env, lines)?, name)?;
            Ok(spike_option(
                crypto_aead_open(&alg, &key, &nonce, &aad, &ciphertext)?.map(spike_u8_array),
            ))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike crypto call {name:?}"
        ))),
    }
}

fn spike_u8_array(bytes: Vec<u8>) -> SpikeValue {
    SpikeValue::Array(
        bytes
            .into_iter()
            .map(|value| SpikeValue::UInt(value as u64))
            .collect(),
    )
}

fn crypto_random_bytes(length: i64) -> Result<Vec<u8>, Diagnostic> {
    if !(0..=65536).contains(&length) {
        return Err(unsupported(
            "crypto_rand_bytes length must be between 0 and 65536",
        ));
    }
    let mut bytes = vec![0; length as usize];
    if bytes.is_empty() {
        return Ok(bytes);
    }
    fill_crypto_random_bytes(&mut bytes)?;
    Ok(bytes)
}

#[cfg(not(windows))]
fn fill_crypto_random_bytes(bytes: &mut [u8]) -> Result<(), Diagnostic> {
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(bytes))
        .map_err(|err| {
            unsupported(&format!(
                "failed to read random bytes from /dev/urandom: {err}"
            ))
        })
}

#[cfg(windows)]
fn fill_crypto_random_bytes(_bytes: &mut [u8]) -> Result<(), Diagnostic> {
    Err(unsupported(
        "crypto random bytes are not supported by the cranelift spike on Windows",
    ))
}

#[cfg(unix)]
fn crypto_ed25519_keygen() -> Result<(Vec<u8>, Vec<u8>), Diagnostic> {
    spike_crypto_ed25519_keygen_inner()
        .ok_or_else(|| unsupported("crypto_ed25519_keygen failed; check OpenSSL Ed25519 support"))
}

#[cfg(not(unix))]
fn crypto_ed25519_keygen() -> Result<(Vec<u8>, Vec<u8>), Diagnostic> {
    Err(unsupported(
        "crypto Ed25519 is not supported by the cranelift spike on Windows",
    ))
}

#[cfg(unix)]
fn crypto_ed25519_sign(secret_key: &[u8], message: &[u8]) -> Result<Vec<u8>, Diagnostic> {
    spike_crypto_ed25519_sign_inner(secret_key, message).ok_or_else(|| {
        unsupported("crypto_ed25519_sign failed; check key length and OpenSSL support")
    })
}

#[cfg(not(unix))]
fn crypto_ed25519_sign(_secret_key: &[u8], _message: &[u8]) -> Result<Vec<u8>, Diagnostic> {
    Err(unsupported(
        "crypto Ed25519 is not supported by the cranelift spike on Windows",
    ))
}

#[cfg(unix)]
fn crypto_ed25519_verify(
    public_key: &[u8],
    message: &[u8],
    signature: &[u8],
) -> Result<bool, Diagnostic> {
    spike_crypto_ed25519_verify_inner(public_key, message, signature)
        .ok_or_else(|| unsupported("crypto_ed25519_verify failed; check OpenSSL support"))
}

#[cfg(not(unix))]
fn crypto_ed25519_verify(
    _public_key: &[u8],
    _message: &[u8],
    _signature: &[u8],
) -> Result<bool, Diagnostic> {
    Err(unsupported(
        "crypto Ed25519 is not supported by the cranelift spike on Windows",
    ))
}

#[cfg(unix)]
fn crypto_aead_seal(
    alg: &str,
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, Diagnostic> {
    spike_crypto_aead_seal_inner(alg, key, nonce, aad, plaintext).ok_or_else(|| {
        unsupported(
            "crypto_aead_seal failed; check algorithm, key length, nonce length, and OpenSSL support",
        )
    })
}

#[cfg(not(unix))]
fn crypto_aead_seal(
    _alg: &str,
    _key: &[u8],
    _nonce: &[u8],
    _aad: &[u8],
    _plaintext: &[u8],
) -> Result<Vec<u8>, Diagnostic> {
    Err(unsupported(
        "crypto AEAD is not supported by the cranelift spike on this platform",
    ))
}

#[cfg(unix)]
fn crypto_aead_open(
    alg: &str,
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Option<Vec<u8>>, Diagnostic> {
    Ok(spike_crypto_aead_open_inner(
        alg, key, nonce, aad, ciphertext,
    ))
}

#[cfg(not(unix))]
fn crypto_aead_open(
    _alg: &str,
    _key: &[u8],
    _nonce: &[u8],
    _aad: &[u8],
    _ciphertext: &[u8],
) -> Result<Option<Vec<u8>>, Diagnostic> {
    Err(unsupported(
        "crypto AEAD is not supported by the cranelift spike on this platform",
    ))
}

#[cfg(unix)]
fn spike_crypto_aead_seal_inner(
    alg: &str,
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    plaintext: &[u8],
) -> Option<Vec<u8>> {
    let crypto = SpikeAeadCrypto::load().ok()?;
    let cipher = spike_crypto_aead_cipher(&crypto, alg)?;
    if key.len() != cipher.key_len || nonce.len() != cipher.nonce_len {
        return None;
    }
    if plaintext.len() > std::os::raw::c_int::MAX as usize
        || aad.len() > std::os::raw::c_int::MAX as usize
    {
        return None;
    }
    let ctx = SpikeAeadCtxGuard::new(unsafe { (crypto.evp_cipher_ctx_new)() }, &crypto)?;
    if unsafe {
        (crypto.evp_encrypt_init_ex)(
            ctx.ctx,
            cipher.cipher,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    } <= 0
    {
        return None;
    }
    if unsafe {
        (crypto.evp_cipher_ctx_ctrl)(
            ctx.ctx,
            SPIKE_EVP_CTRL_AEAD_SET_IVLEN,
            cipher.nonce_len as std::os::raw::c_int,
            std::ptr::null_mut(),
        )
    } <= 0
    {
        return None;
    }
    if unsafe {
        (crypto.evp_encrypt_init_ex)(
            ctx.ctx,
            std::ptr::null(),
            std::ptr::null_mut(),
            key.as_ptr(),
            nonce.as_ptr(),
        )
    } <= 0
    {
        return None;
    }
    let mut chunk_len = 0 as std::os::raw::c_int;
    if !aad.is_empty()
        && unsafe {
            (crypto.evp_encrypt_update)(
                ctx.ctx,
                std::ptr::null_mut(),
                &mut chunk_len,
                aad.as_ptr(),
                aad.len() as std::os::raw::c_int,
            )
        } <= 0
    {
        return None;
    }
    let mut output = vec![0u8; plaintext.len() + cipher.tag_len];
    let mut written = 0usize;
    if !plaintext.is_empty() {
        if unsafe {
            (crypto.evp_encrypt_update)(
                ctx.ctx,
                output.as_mut_ptr(),
                &mut chunk_len,
                plaintext.as_ptr(),
                plaintext.len() as std::os::raw::c_int,
            )
        } <= 0
        {
            return None;
        }
        written += chunk_len as usize;
    }
    if unsafe {
        (crypto.evp_encrypt_final_ex)(ctx.ctx, output[written..].as_mut_ptr(), &mut chunk_len)
    } <= 0
    {
        return None;
    }
    written += chunk_len as usize;
    output.truncate(written);
    let mut tag = vec![0u8; cipher.tag_len];
    if unsafe {
        (crypto.evp_cipher_ctx_ctrl)(
            ctx.ctx,
            SPIKE_EVP_CTRL_AEAD_GET_TAG,
            cipher.tag_len as std::os::raw::c_int,
            tag.as_mut_ptr().cast::<std::os::raw::c_void>(),
        )
    } <= 0
    {
        return None;
    }
    output.extend_from_slice(&tag);
    Some(output)
}

#[cfg(unix)]
fn spike_crypto_aead_open_inner(
    alg: &str,
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
) -> Option<Vec<u8>> {
    let crypto = SpikeAeadCrypto::load().ok()?;
    let cipher = spike_crypto_aead_cipher(&crypto, alg)?;
    if key.len() != cipher.key_len
        || nonce.len() != cipher.nonce_len
        || ciphertext.len() < cipher.tag_len
    {
        return None;
    }
    let encrypted_len = ciphertext.len() - cipher.tag_len;
    if encrypted_len > std::os::raw::c_int::MAX as usize
        || aad.len() > std::os::raw::c_int::MAX as usize
    {
        return None;
    }
    let (encrypted, tag) = ciphertext.split_at(encrypted_len);
    let ctx = SpikeAeadCtxGuard::new(unsafe { (crypto.evp_cipher_ctx_new)() }, &crypto)?;
    if unsafe {
        (crypto.evp_decrypt_init_ex)(
            ctx.ctx,
            cipher.cipher,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    } <= 0
    {
        return None;
    }
    if unsafe {
        (crypto.evp_cipher_ctx_ctrl)(
            ctx.ctx,
            SPIKE_EVP_CTRL_AEAD_SET_IVLEN,
            cipher.nonce_len as std::os::raw::c_int,
            std::ptr::null_mut(),
        )
    } <= 0
    {
        return None;
    }
    if unsafe {
        (crypto.evp_decrypt_init_ex)(
            ctx.ctx,
            std::ptr::null(),
            std::ptr::null_mut(),
            key.as_ptr(),
            nonce.as_ptr(),
        )
    } <= 0
    {
        return None;
    }
    let mut chunk_len = 0 as std::os::raw::c_int;
    if !aad.is_empty()
        && unsafe {
            (crypto.evp_decrypt_update)(
                ctx.ctx,
                std::ptr::null_mut(),
                &mut chunk_len,
                aad.as_ptr(),
                aad.len() as std::os::raw::c_int,
            )
        } <= 0
    {
        return None;
    }
    let mut output = vec![0u8; encrypted_len + cipher.tag_len];
    let mut written = 0usize;
    if !encrypted.is_empty() {
        if unsafe {
            (crypto.evp_decrypt_update)(
                ctx.ctx,
                output.as_mut_ptr(),
                &mut chunk_len,
                encrypted.as_ptr(),
                encrypted.len() as std::os::raw::c_int,
            )
        } <= 0
        {
            return None;
        }
        written += chunk_len as usize;
    }
    if unsafe {
        (crypto.evp_cipher_ctx_ctrl)(
            ctx.ctx,
            SPIKE_EVP_CTRL_AEAD_SET_TAG,
            cipher.tag_len as std::os::raw::c_int,
            tag.as_ptr() as *mut std::os::raw::c_void,
        )
    } <= 0
    {
        return None;
    }
    if unsafe {
        (crypto.evp_decrypt_final_ex)(ctx.ctx, output[written..].as_mut_ptr(), &mut chunk_len)
    } <= 0
    {
        return None;
    }
    written += chunk_len as usize;
    output.truncate(written);
    Some(output)
}

#[cfg(unix)]
struct SpikeAeadCipher {
    cipher: *const SpikeEvpCipher,
    key_len: usize,
    nonce_len: usize,
    tag_len: usize,
}

#[cfg(unix)]
fn spike_crypto_aead_cipher(crypto: &SpikeAeadCrypto, alg: &str) -> Option<SpikeAeadCipher> {
    let (cipher, key_len) = match alg {
        "AES-128-GCM" => (unsafe { (crypto.evp_aes_128_gcm)() }, 16),
        "AES-256-GCM" => (unsafe { (crypto.evp_aes_256_gcm)() }, 32),
        "CHACHA20-POLY1305" => (unsafe { (crypto.evp_chacha20_poly1305)() }, 32),
        _ => return None,
    };
    if cipher.is_null() {
        return None;
    }
    Some(SpikeAeadCipher {
        cipher,
        key_len,
        nonce_len: 12,
        tag_len: 16,
    })
}

#[cfg(unix)]
const SPIKE_EVP_CTRL_AEAD_SET_IVLEN: std::os::raw::c_int = 0x9;
#[cfg(unix)]
const SPIKE_EVP_CTRL_AEAD_GET_TAG: std::os::raw::c_int = 0x10;
#[cfg(unix)]
const SPIKE_EVP_CTRL_AEAD_SET_TAG: std::os::raw::c_int = 0x11;

#[cfg(unix)]
#[repr(C)]
struct SpikeEvpCipher {
    _private: [u8; 0],
}

#[cfg(unix)]
#[repr(C)]
struct SpikeEvpCipherCtx {
    _private: [u8; 0],
}

#[cfg(unix)]
#[repr(C)]
struct SpikeEvpPkeyCtx {
    _private: [u8; 0],
}

#[cfg(unix)]
#[repr(C)]
struct SpikeEvpPkey {
    _private: [u8; 0],
}

#[cfg(unix)]
#[repr(C)]
struct SpikeEvpMdCtx {
    _private: [u8; 0],
}

#[cfg(unix)]
const SPIKE_EVP_PKEY_ED25519: std::os::raw::c_int = 1087;

#[cfg(unix)]
struct SpikeEd25519Crypto {
    handle: *mut std::os::raw::c_void,
    evp_pkey_ctx_new_id: unsafe extern "C" fn(
        std::os::raw::c_int,
        *mut std::os::raw::c_void,
    ) -> *mut SpikeEvpPkeyCtx,
    evp_pkey_ctx_free: unsafe extern "C" fn(*mut SpikeEvpPkeyCtx),
    evp_pkey_keygen_init: unsafe extern "C" fn(*mut SpikeEvpPkeyCtx) -> std::os::raw::c_int,
    evp_pkey_keygen:
        unsafe extern "C" fn(*mut SpikeEvpPkeyCtx, *mut *mut SpikeEvpPkey) -> std::os::raw::c_int,
    evp_pkey_free: unsafe extern "C" fn(*mut SpikeEvpPkey),
    evp_pkey_get_raw_public_key:
        unsafe extern "C" fn(*const SpikeEvpPkey, *mut u8, *mut usize) -> std::os::raw::c_int,
    evp_pkey_get_raw_private_key:
        unsafe extern "C" fn(*const SpikeEvpPkey, *mut u8, *mut usize) -> std::os::raw::c_int,
    evp_pkey_new_raw_private_key: unsafe extern "C" fn(
        std::os::raw::c_int,
        *mut std::os::raw::c_void,
        *const u8,
        usize,
    ) -> *mut SpikeEvpPkey,
    evp_pkey_new_raw_public_key: unsafe extern "C" fn(
        std::os::raw::c_int,
        *mut std::os::raw::c_void,
        *const u8,
        usize,
    ) -> *mut SpikeEvpPkey,
    evp_md_ctx_new: unsafe extern "C" fn() -> *mut SpikeEvpMdCtx,
    evp_md_ctx_free: unsafe extern "C" fn(*mut SpikeEvpMdCtx),
    evp_digest_sign_init: unsafe extern "C" fn(
        *mut SpikeEvpMdCtx,
        *mut *mut std::os::raw::c_void,
        *const std::os::raw::c_void,
        *mut std::os::raw::c_void,
        *mut SpikeEvpPkey,
    ) -> std::os::raw::c_int,
    evp_digest_sign: unsafe extern "C" fn(
        *mut SpikeEvpMdCtx,
        *mut u8,
        *mut usize,
        *const u8,
        usize,
    ) -> std::os::raw::c_int,
    evp_digest_verify_init: unsafe extern "C" fn(
        *mut SpikeEvpMdCtx,
        *mut *mut std::os::raw::c_void,
        *const std::os::raw::c_void,
        *mut std::os::raw::c_void,
        *mut SpikeEvpPkey,
    ) -> std::os::raw::c_int,
    evp_digest_verify: unsafe extern "C" fn(
        *mut SpikeEvpMdCtx,
        *const u8,
        usize,
        *const u8,
        usize,
    ) -> std::os::raw::c_int,
}

#[cfg(unix)]
struct SpikeAeadCrypto {
    handle: *mut std::os::raw::c_void,
    evp_aes_128_gcm: unsafe extern "C" fn() -> *const SpikeEvpCipher,
    evp_aes_256_gcm: unsafe extern "C" fn() -> *const SpikeEvpCipher,
    evp_chacha20_poly1305: unsafe extern "C" fn() -> *const SpikeEvpCipher,
    evp_cipher_ctx_new: unsafe extern "C" fn() -> *mut SpikeEvpCipherCtx,
    evp_cipher_ctx_free: unsafe extern "C" fn(*mut SpikeEvpCipherCtx),
    evp_cipher_ctx_ctrl: unsafe extern "C" fn(
        *mut SpikeEvpCipherCtx,
        std::os::raw::c_int,
        std::os::raw::c_int,
        *mut std::os::raw::c_void,
    ) -> std::os::raw::c_int,
    evp_encrypt_init_ex: unsafe extern "C" fn(
        *mut SpikeEvpCipherCtx,
        *const SpikeEvpCipher,
        *mut std::os::raw::c_void,
        *const u8,
        *const u8,
    ) -> std::os::raw::c_int,
    evp_encrypt_update: unsafe extern "C" fn(
        *mut SpikeEvpCipherCtx,
        *mut u8,
        *mut std::os::raw::c_int,
        *const u8,
        std::os::raw::c_int,
    ) -> std::os::raw::c_int,
    evp_encrypt_final_ex: unsafe extern "C" fn(
        *mut SpikeEvpCipherCtx,
        *mut u8,
        *mut std::os::raw::c_int,
    ) -> std::os::raw::c_int,
    evp_decrypt_init_ex: unsafe extern "C" fn(
        *mut SpikeEvpCipherCtx,
        *const SpikeEvpCipher,
        *mut std::os::raw::c_void,
        *const u8,
        *const u8,
    ) -> std::os::raw::c_int,
    evp_decrypt_update: unsafe extern "C" fn(
        *mut SpikeEvpCipherCtx,
        *mut u8,
        *mut std::os::raw::c_int,
        *const u8,
        std::os::raw::c_int,
    ) -> std::os::raw::c_int,
    evp_decrypt_final_ex: unsafe extern "C" fn(
        *mut SpikeEvpCipherCtx,
        *mut u8,
        *mut std::os::raw::c_int,
    ) -> std::os::raw::c_int,
}

#[cfg(unix)]
macro_rules! spike_crypto_aead_load_typed_symbol {
    ($handle:expr, $symbol:literal) => {{
        let value = spike_crypto_aead_load_symbol($handle, $symbol)?;
        unsafe { spike_crypto_aead_cast_typed_symbol(value) }
    }};
}

#[cfg(unix)]
impl SpikeEd25519Crypto {
    fn load() -> Result<Self, String> {
        let handle = spike_crypto_aead_open_library(SPIKE_OPENSSL_CRYPTO_CANDIDATES)?;
        Ok(Self {
            handle,
            evp_pkey_ctx_new_id: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_PKEY_CTX_new_id"
            ),
            evp_pkey_ctx_free: spike_crypto_aead_load_typed_symbol!(handle, "EVP_PKEY_CTX_free"),
            evp_pkey_keygen_init: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_PKEY_keygen_init"
            ),
            evp_pkey_keygen: spike_crypto_aead_load_typed_symbol!(handle, "EVP_PKEY_keygen"),
            evp_pkey_free: spike_crypto_aead_load_typed_symbol!(handle, "EVP_PKEY_free"),
            evp_pkey_get_raw_public_key: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_PKEY_get_raw_public_key"
            ),
            evp_pkey_get_raw_private_key: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_PKEY_get_raw_private_key"
            ),
            evp_pkey_new_raw_private_key: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_PKEY_new_raw_private_key"
            ),
            evp_pkey_new_raw_public_key: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_PKEY_new_raw_public_key"
            ),
            evp_md_ctx_new: spike_crypto_aead_load_typed_symbol!(handle, "EVP_MD_CTX_new"),
            evp_md_ctx_free: spike_crypto_aead_load_typed_symbol!(handle, "EVP_MD_CTX_free"),
            evp_digest_sign_init: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_DigestSignInit"
            ),
            evp_digest_sign: spike_crypto_aead_load_typed_symbol!(handle, "EVP_DigestSign"),
            evp_digest_verify_init: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_DigestVerifyInit"
            ),
            evp_digest_verify: spike_crypto_aead_load_typed_symbol!(handle, "EVP_DigestVerify"),
        })
    }
}

#[cfg(unix)]
impl Drop for SpikeEd25519Crypto {
    fn drop(&mut self) {
        unsafe {
            let _ = spike_crypto_aead_dlclose(self.handle);
        }
    }
}

#[cfg(unix)]
impl SpikeAeadCrypto {
    fn load() -> Result<Self, String> {
        let handle = spike_crypto_aead_open_library(SPIKE_OPENSSL_CRYPTO_CANDIDATES)?;
        Ok(Self {
            handle,
            evp_aes_128_gcm: spike_crypto_aead_load_typed_symbol!(handle, "EVP_aes_128_gcm"),
            evp_aes_256_gcm: spike_crypto_aead_load_typed_symbol!(handle, "EVP_aes_256_gcm"),
            evp_chacha20_poly1305: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_chacha20_poly1305"
            ),
            evp_cipher_ctx_new: spike_crypto_aead_load_typed_symbol!(handle, "EVP_CIPHER_CTX_new"),
            evp_cipher_ctx_free: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_CIPHER_CTX_free"
            ),
            evp_cipher_ctx_ctrl: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_CIPHER_CTX_ctrl"
            ),
            evp_encrypt_init_ex: spike_crypto_aead_load_typed_symbol!(handle, "EVP_EncryptInit_ex"),
            evp_encrypt_update: spike_crypto_aead_load_typed_symbol!(handle, "EVP_EncryptUpdate"),
            evp_encrypt_final_ex: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_EncryptFinal_ex"
            ),
            evp_decrypt_init_ex: spike_crypto_aead_load_typed_symbol!(handle, "EVP_DecryptInit_ex"),
            evp_decrypt_update: spike_crypto_aead_load_typed_symbol!(handle, "EVP_DecryptUpdate"),
            evp_decrypt_final_ex: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_DecryptFinal_ex"
            ),
        })
    }
}

#[cfg(unix)]
impl Drop for SpikeAeadCrypto {
    fn drop(&mut self) {
        unsafe {
            let _ = spike_crypto_aead_dlclose(self.handle);
        }
    }
}

#[cfg(unix)]
struct SpikeEd25519PkeyCtxGuard<'a> {
    ctx: *mut SpikeEvpPkeyCtx,
    crypto: &'a SpikeEd25519Crypto,
}

#[cfg(unix)]
impl<'a> SpikeEd25519PkeyCtxGuard<'a> {
    fn new(ctx: *mut SpikeEvpPkeyCtx, crypto: &'a SpikeEd25519Crypto) -> Option<Self> {
        (!ctx.is_null()).then_some(Self { ctx, crypto })
    }
}

#[cfg(unix)]
impl Drop for SpikeEd25519PkeyCtxGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.crypto.evp_pkey_ctx_free)(self.ctx);
        }
    }
}

#[cfg(unix)]
struct SpikeEd25519PkeyGuard<'a> {
    pkey: *mut SpikeEvpPkey,
    crypto: &'a SpikeEd25519Crypto,
}

#[cfg(unix)]
impl<'a> SpikeEd25519PkeyGuard<'a> {
    fn new(pkey: *mut SpikeEvpPkey, crypto: &'a SpikeEd25519Crypto) -> Option<Self> {
        (!pkey.is_null()).then_some(Self { pkey, crypto })
    }
}

#[cfg(unix)]
impl Drop for SpikeEd25519PkeyGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.crypto.evp_pkey_free)(self.pkey);
        }
    }
}

#[cfg(unix)]
struct SpikeEd25519MdCtxGuard<'a> {
    ctx: *mut SpikeEvpMdCtx,
    crypto: &'a SpikeEd25519Crypto,
}

#[cfg(unix)]
impl<'a> SpikeEd25519MdCtxGuard<'a> {
    fn new(ctx: *mut SpikeEvpMdCtx, crypto: &'a SpikeEd25519Crypto) -> Option<Self> {
        (!ctx.is_null()).then_some(Self { ctx, crypto })
    }
}

#[cfg(unix)]
impl Drop for SpikeEd25519MdCtxGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.crypto.evp_md_ctx_free)(self.ctx);
        }
    }
}

#[cfg(unix)]
struct SpikeAeadCtxGuard<'a> {
    ctx: *mut SpikeEvpCipherCtx,
    crypto: &'a SpikeAeadCrypto,
}

#[cfg(unix)]
impl<'a> SpikeAeadCtxGuard<'a> {
    fn new(ctx: *mut SpikeEvpCipherCtx, crypto: &'a SpikeAeadCrypto) -> Option<Self> {
        (!ctx.is_null()).then_some(Self { ctx, crypto })
    }
}

#[cfg(unix)]
impl Drop for SpikeAeadCtxGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.crypto.evp_cipher_ctx_free)(self.ctx);
        }
    }
}

#[cfg(unix)]
const SPIKE_OPENSSL_CRYPTO_CANDIDATES: &[&str] = &[
    "/usr/lib/x86_64-linux-gnu/libcrypto.so.3",
    "/lib/x86_64-linux-gnu/libcrypto.so.3",
    "/usr/lib/aarch64-linux-gnu/libcrypto.so.3",
    "/lib/aarch64-linux-gnu/libcrypto.so.3",
    "/usr/lib64/libcrypto.so.3",
    "/lib64/libcrypto.so.3",
    "/usr/lib/libcrypto.so.3",
    "/lib/libcrypto.so.3",
    "/opt/homebrew/opt/openssl@3/lib/libcrypto.3.dylib",
    "/usr/local/opt/openssl@3/lib/libcrypto.3.dylib",
    "/usr/lib/x86_64-linux-gnu/libcrypto.so.1.1",
    "/lib/x86_64-linux-gnu/libcrypto.so.1.1",
    "/usr/lib/aarch64-linux-gnu/libcrypto.so.1.1",
    "/lib/aarch64-linux-gnu/libcrypto.so.1.1",
    "/usr/lib64/libcrypto.so.1.1",
    "/lib64/libcrypto.so.1.1",
    "/usr/lib/libcrypto.so.1.1",
    "/lib/libcrypto.so.1.1",
];

#[cfg(unix)]
unsafe fn spike_crypto_aead_cast_typed_symbol<T: Copy>(value: *mut std::os::raw::c_void) -> T {
    debug_assert_eq!(
        std::mem::size_of::<T>(),
        std::mem::size_of::<*mut std::os::raw::c_void>()
    );
    let mut output = std::mem::MaybeUninit::<T>::uninit();
    unsafe {
        std::ptr::copy_nonoverlapping(
            (&value as *const *mut std::os::raw::c_void).cast::<u8>(),
            output.as_mut_ptr().cast::<u8>(),
            std::mem::size_of::<T>(),
        );
        output.assume_init()
    }
}

#[cfg(unix)]
fn spike_crypto_aead_open_library(
    candidates: &[&str],
) -> Result<*mut std::os::raw::c_void, String> {
    for candidate in candidates {
        let name = match std::ffi::CString::new(*candidate) {
            Ok(name) => name,
            Err(_) => continue,
        };
        let handle = unsafe { spike_crypto_aead_dlopen(name.as_ptr(), 2) };
        if !handle.is_null() {
            return Ok(handle);
        }
    }
    Err(format!(
        "AEAD support requires one of {}",
        candidates.join(", ")
    ))
}

#[cfg(unix)]
fn spike_crypto_aead_load_symbol(
    handle: *mut std::os::raw::c_void,
    symbol: &str,
) -> Result<*mut std::os::raw::c_void, String> {
    let name = std::ffi::CString::new(symbol).map_err(|_| String::from("invalid symbol name"))?;
    let value = unsafe { spike_crypto_aead_dlsym(handle, name.as_ptr()) };
    if value.is_null() {
        return Err(format!("AEAD support missing OpenSSL symbol {symbol}"));
    }
    Ok(value)
}

#[cfg(unix)]
#[cfg_attr(not(target_os = "macos"), link(name = "dl"))]
unsafe extern "C" {
    #[link_name = "dlopen"]
    fn spike_crypto_aead_dlopen(
        filename: *const std::os::raw::c_char,
        flags: std::os::raw::c_int,
    ) -> *mut std::os::raw::c_void;
    #[link_name = "dlsym"]
    fn spike_crypto_aead_dlsym(
        handle: *mut std::os::raw::c_void,
        symbol: *const std::os::raw::c_char,
    ) -> *mut std::os::raw::c_void;
    #[link_name = "dlclose"]
    fn spike_crypto_aead_dlclose(handle: *mut std::os::raw::c_void) -> std::os::raw::c_int;
}

#[cfg(unix)]
fn spike_crypto_ed25519_keygen_inner() -> Option<(Vec<u8>, Vec<u8>)> {
    let crypto = SpikeEd25519Crypto::load().ok()?;
    let ctx = SpikeEd25519PkeyCtxGuard::new(
        unsafe { (crypto.evp_pkey_ctx_new_id)(SPIKE_EVP_PKEY_ED25519, std::ptr::null_mut()) },
        &crypto,
    )?;
    if unsafe { (crypto.evp_pkey_keygen_init)(ctx.ctx) } <= 0 {
        return None;
    }
    let mut pkey = std::ptr::null_mut();
    if unsafe { (crypto.evp_pkey_keygen)(ctx.ctx, &mut pkey) } <= 0 || pkey.is_null() {
        return None;
    }
    let pkey = SpikeEd25519PkeyGuard::new(pkey, &crypto)?;
    let mut public_key = vec![0u8; 32];
    let mut public_len = public_key.len();
    if unsafe {
        (crypto.evp_pkey_get_raw_public_key)(pkey.pkey, public_key.as_mut_ptr(), &mut public_len)
    } <= 0
    {
        return None;
    }
    let mut private_key = vec![0u8; 32];
    let mut private_len = private_key.len();
    if unsafe {
        (crypto.evp_pkey_get_raw_private_key)(pkey.pkey, private_key.as_mut_ptr(), &mut private_len)
    } <= 0
    {
        return None;
    }
    if public_len != 32 || private_len != 32 {
        return None;
    }
    public_key.truncate(public_len);
    private_key.truncate(private_len);
    private_key.extend_from_slice(&public_key);
    Some((public_key, private_key))
}

#[cfg(unix)]
fn spike_crypto_ed25519_sign_inner(secret_key: &[u8], message: &[u8]) -> Option<Vec<u8>> {
    let crypto = SpikeEd25519Crypto::load().ok()?;
    let signing_key = spike_crypto_ed25519_signing_key(secret_key)?;
    let pkey = unsafe {
        (crypto.evp_pkey_new_raw_private_key)(
            SPIKE_EVP_PKEY_ED25519,
            std::ptr::null_mut(),
            signing_key.as_ptr(),
            signing_key.len(),
        )
    };
    let pkey = SpikeEd25519PkeyGuard::new(pkey, &crypto)?;
    let ctx = SpikeEd25519MdCtxGuard::new(unsafe { (crypto.evp_md_ctx_new)() }, &crypto)?;
    if unsafe {
        (crypto.evp_digest_sign_init)(
            ctx.ctx,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null_mut(),
            pkey.pkey,
        )
    } <= 0
    {
        return None;
    }
    let mut signature_len = 0usize;
    if unsafe {
        (crypto.evp_digest_sign)(
            ctx.ctx,
            std::ptr::null_mut(),
            &mut signature_len,
            message.as_ptr(),
            message.len(),
        )
    } <= 0
        || signature_len == 0
        || signature_len > 1024
    {
        return None;
    }
    let mut signature = vec![0u8; signature_len];
    if unsafe {
        (crypto.evp_digest_sign)(
            ctx.ctx,
            signature.as_mut_ptr(),
            &mut signature_len,
            message.as_ptr(),
            message.len(),
        )
    } <= 0
    {
        return None;
    }
    signature.truncate(signature_len);
    Some(signature)
}

#[cfg(unix)]
fn spike_crypto_ed25519_verify_inner(
    public_key: &[u8],
    message: &[u8],
    signature: &[u8],
) -> Option<bool> {
    if public_key.len() != 32 || signature.len() != 64 {
        return Some(false);
    }
    let crypto = SpikeEd25519Crypto::load().ok()?;
    let pkey = unsafe {
        (crypto.evp_pkey_new_raw_public_key)(
            SPIKE_EVP_PKEY_ED25519,
            std::ptr::null_mut(),
            public_key.as_ptr(),
            public_key.len(),
        )
    };
    let pkey = SpikeEd25519PkeyGuard::new(pkey, &crypto)?;
    let ctx = SpikeEd25519MdCtxGuard::new(unsafe { (crypto.evp_md_ctx_new)() }, &crypto)?;
    if unsafe {
        (crypto.evp_digest_verify_init)(
            ctx.ctx,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null_mut(),
            pkey.pkey,
        )
    } <= 0
    {
        return None;
    }
    let result = unsafe {
        (crypto.evp_digest_verify)(
            ctx.ctx,
            signature.as_ptr(),
            signature.len(),
            message.as_ptr(),
            message.len(),
        )
    };
    if result == 1 {
        Some(true)
    } else if result == 0 {
        Some(false)
    } else {
        None
    }
}

#[cfg(unix)]
fn spike_crypto_ed25519_signing_key(secret_key: &[u8]) -> Option<&[u8]> {
    match secret_key.len() {
        32 => Some(secret_key),
        64 => Some(&secret_key[..32]),
        _ => None,
    }
}

fn is_net_call(name: &str) -> bool {
    matches!(
        name,
        "net_resolve"
            | "net_tcp_listen"
            | "net_tcp_listener_port"
            | "net_tcp_accept"
            | "net_tcp_read_string"
            | "net_tcp_write_string"
            | "net_tcp_close"
            | "net_tcp_close_listener"
            | "net_tcp_listen_loopback_once"
            | "net_tcp_dial"
            | "net_udp_bind_loopback_once"
            | "net_udp_send_recv"
    )
}

fn eval_net_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "net_resolve" => {
            let [host] = args else {
                return Err(unsupported("net_resolve expects exactly one argument"));
            };
            let host = expect_text(eval_expr(host, functions, env, lines)?, name)?;
            let resolved = (host.as_str(), 0)
                .to_socket_addrs()
                .ok()
                .and_then(|mut addrs| addrs.next())
                .map(|addr| SpikeValue::Text(addr.ip().to_string()));
            Ok(spike_option(resolved))
        }
        "net_tcp_listen_loopback_once" => {
            let [response, timeout_ms] = args else {
                return Err(unsupported(
                    "net_tcp_listen_loopback_once expects exactly two arguments",
                ));
            };
            let response = expect_text(eval_expr(response, functions, env, lines)?, name)?;
            let timeout = net_timeout(expect_int(eval_expr(timeout_ms, functions, env, lines)?)?);
            Ok(spike_option(
                net_tcp_listen_loopback_once(response, timeout).map(SpikeValue::Int),
            ))
        }
        "net_tcp_listen" => {
            let [bind] = args else {
                return Err(unsupported("net_tcp_listen expects exactly one argument"));
            };
            let bind = expect_text(eval_expr(bind, functions, env, lines)?, name)?;
            Ok(SpikeValue::Int(net_tcp_listen(&bind).ok_or_else(|| {
                unsupported("net_tcp_listen failed in cranelift spike")
            })?))
        }
        "net_tcp_listener_port" => {
            let [listener] = args else {
                return Err(unsupported(
                    "net_tcp_listener_port expects exactly one argument",
                ));
            };
            let listener = expect_int(eval_expr(listener, functions, env, lines)?)?;
            Ok(SpikeValue::Int(
                net_tcp_listener_port(listener).ok_or_else(|| {
                    unsupported("net_tcp_listener_port failed in cranelift spike")
                })?,
            ))
        }
        "net_tcp_accept" => {
            let [listener] = args else {
                return Err(unsupported("net_tcp_accept expects exactly one argument"));
            };
            let listener = expect_int(eval_expr(listener, functions, env, lines)?)?;
            Ok(SpikeValue::Int(net_tcp_accept(listener).ok_or_else(
                || unsupported("net_tcp_accept failed in cranelift spike"),
            )?))
        }
        "net_tcp_read_string" => {
            let [stream, max_bytes] = args else {
                return Err(unsupported(
                    "net_tcp_read_string expects exactly two arguments",
                ));
            };
            let stream = expect_int(eval_expr(stream, functions, env, lines)?)?;
            let max_bytes = expect_int(eval_expr(max_bytes, functions, env, lines)?)?;
            Ok(SpikeValue::Text(
                net_tcp_read_string(stream, max_bytes)
                    .ok_or_else(|| unsupported("net_tcp_read_string failed in cranelift spike"))?,
            ))
        }
        "net_tcp_write_string" => {
            let [stream, message] = args else {
                return Err(unsupported(
                    "net_tcp_write_string expects exactly two arguments",
                ));
            };
            let stream = expect_int(eval_expr(stream, functions, env, lines)?)?;
            let message = expect_text(eval_expr(message, functions, env, lines)?, name)?;
            Ok(SpikeValue::Int(net_tcp_write_string(stream, &message)))
        }
        "net_tcp_close" => {
            let [stream] = args else {
                return Err(unsupported("net_tcp_close expects exactly one argument"));
            };
            let stream = expect_int(eval_expr(stream, functions, env, lines)?)?;
            Ok(SpikeValue::Int(net_tcp_close(stream)))
        }
        "net_tcp_close_listener" => {
            let [listener] = args else {
                return Err(unsupported(
                    "net_tcp_close_listener expects exactly one argument",
                ));
            };
            let listener = expect_int(eval_expr(listener, functions, env, lines)?)?;
            Ok(SpikeValue::Int(net_tcp_close_listener(listener)))
        }
        "net_tcp_dial" => {
            let [host, port, message, timeout_ms] = args else {
                return Err(unsupported("net_tcp_dial expects exactly four arguments"));
            };
            let host = expect_text(eval_expr(host, functions, env, lines)?, name)?;
            let port = expect_int(eval_expr(port, functions, env, lines)?)?;
            let message = expect_text(eval_expr(message, functions, env, lines)?, name)?;
            let timeout = net_timeout(expect_int(eval_expr(timeout_ms, functions, env, lines)?)?);
            Ok(spike_option(
                net_tcp_dial(host, port, message, timeout).map(SpikeValue::Text),
            ))
        }
        "net_udp_bind_loopback_once" => {
            let [response, timeout_ms] = args else {
                return Err(unsupported(
                    "net_udp_bind_loopback_once expects exactly two arguments",
                ));
            };
            let response = expect_text(eval_expr(response, functions, env, lines)?, name)?;
            let timeout = net_timeout(expect_int(eval_expr(timeout_ms, functions, env, lines)?)?);
            Ok(spike_option(
                net_udp_bind_loopback_once(response, timeout).map(SpikeValue::Int),
            ))
        }
        "net_udp_send_recv" => {
            let [host, port, message, timeout_ms] = args else {
                return Err(unsupported(
                    "net_udp_send_recv expects exactly four arguments",
                ));
            };
            let host = expect_text(eval_expr(host, functions, env, lines)?, name)?;
            let port = expect_int(eval_expr(port, functions, env, lines)?)?;
            let message = expect_text(eval_expr(message, functions, env, lines)?, name)?;
            let timeout = net_timeout(expect_int(eval_expr(timeout_ms, functions, env, lines)?)?);
            Ok(spike_option(
                net_udp_send_recv(host, port, message, timeout).map(SpikeValue::Text),
            ))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike net call {name:?}"
        ))),
    }
}

fn net_timeout(timeout_ms: i64) -> std::time::Duration {
    std::time::Duration::from_millis(timeout_ms.clamp(1, 30_000) as u64)
}

fn net_loopback_socket_addr(host: &str, port: i64) -> Option<SocketAddr> {
    let port = u16::try_from(port).ok()?;
    match host {
        "localhost" | "127.0.0.1" => Some(SocketAddr::from(([127, 0, 0, 1], port))),
        "::1" => Some(SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port))),
        _ => None,
    }
}

fn spike_tcp_listeners() -> &'static Mutex<HashMap<i64, SpikeTcpListener>> {
    SPIKE_TCP_LISTENERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn spike_tcp_streams() -> &'static Mutex<HashMap<i64, SpikeTcpStream>> {
    SPIKE_TCP_STREAMS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn spike_tcp_next_handle() -> i64 {
    SPIKE_TCP_NEXT_HANDLE.fetch_add(1, Ordering::Relaxed)
}

fn net_tcp_listen(bind: &str) -> Option<i64> {
    let addr = http_parse_loopback_bind(bind)?;
    let handle = spike_tcp_next_handle();
    let port = if addr.port() == 0 {
        20_000 + handle.rem_euclid(30_000)
    } else {
        i64::from(addr.port())
    };
    spike_tcp_listeners()
        .lock()
        .ok()?
        .insert(handle, SpikeTcpListener { port });
    Some(handle)
}

fn net_tcp_listener_port(listener: i64) -> Option<i64> {
    let listeners = spike_tcp_listeners().lock().ok()?;
    Some(listeners.get(&listener)?.port)
}

fn net_tcp_accept(listener: i64) -> Option<i64> {
    let listeners = spike_tcp_listeners().lock().ok()?;
    listeners.get(&listener)?;
    drop(listeners);
    let handle = spike_tcp_next_handle();
    spike_tcp_streams().lock().ok()?.insert(
        handle,
        SpikeTcpStream {
            received: String::new(),
            written: String::new(),
        },
    );
    Some(handle)
}

fn net_tcp_read_string(stream: i64, max_bytes: i64) -> Option<String> {
    let streams = spike_tcp_streams().lock().ok()?;
    let stream = streams.get(&stream)?;
    let max_bytes = usize::try_from(max_bytes.max(0)).ok()?;
    Some(stream.received.chars().take(max_bytes).collect())
}

fn net_tcp_write_string(stream: i64, message: &str) -> i64 {
    let Ok(mut streams) = spike_tcp_streams().lock() else {
        return -1;
    };
    let Some(stream) = streams.get_mut(&stream) else {
        return -1;
    };
    stream.written.push_str(message);
    i64::try_from(message.len()).unwrap_or(-1)
}

fn net_tcp_close(stream: i64) -> i64 {
    if spike_tcp_streams()
        .lock()
        .ok()
        .and_then(|mut streams| streams.remove(&stream))
        .is_some()
    {
        0
    } else {
        -1
    }
}

fn net_tcp_close_listener(listener: i64) -> i64 {
    if spike_tcp_listeners()
        .lock()
        .ok()
        .and_then(|mut listeners| listeners.remove(&listener))
        .is_some()
    {
        0
    } else {
        -1
    }
}

fn net_tcp_registered_loopback_echo(host: &str, port: i64, message: &str) -> Option<String> {
    net_loopback_socket_addr(host, port)?;
    let listeners = spike_tcp_listeners().lock().ok()?;
    listeners
        .values()
        .any(|listener| listener.port == port)
        .then(|| message.to_string())
}

fn net_tcp_listen_loopback_once(response: String, timeout: std::time::Duration) -> Option<i64> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).ok()?;
    listener.set_nonblocking(true).ok()?;
    let port = listener.local_addr().ok()?.port();
    std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match listener.accept() {
                Ok((mut stream, _peer)) => {
                    let _ = stream.set_read_timeout(Some(timeout));
                    let _ = stream.set_write_timeout(Some(timeout));
                    let mut total_read = 0usize;
                    let mut buf = [0u8; 4096];
                    loop {
                        match stream.read(&mut buf) {
                            Ok(0) => break,
                            Ok(read) => {
                                total_read = total_read.saturating_add(read);
                                if total_read >= 65_536 {
                                    break;
                                }
                            }
                            Err(err)
                                if matches!(
                                    err.kind(),
                                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                                ) =>
                            {
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                    break;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    });
    Some(i64::from(port))
}

fn net_tcp_dial(
    host: String,
    port: i64,
    message: String,
    timeout: std::time::Duration,
) -> Option<String> {
    if let Some(response) = net_tcp_registered_loopback_echo(&host, port, &message) {
        return Some(response);
    }
    let addr = net_loopback_socket_addr(&host, port)?;
    let mut stream = std::net::TcpStream::connect_timeout(&addr, timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    stream.write_all(message.as_bytes()).ok()?;
    stream.shutdown(std::net::Shutdown::Write).ok()?;
    let mut response = Vec::new();
    stream.take(64 * 1024).read_to_end(&mut response).ok()?;
    String::from_utf8(response).ok()
}

fn net_udp_bind_loopback_once(response: String, timeout: std::time::Duration) -> Option<i64> {
    let socket = std::net::UdpSocket::bind(("127.0.0.1", 0)).ok()?;
    socket.set_read_timeout(Some(timeout)).ok()?;
    socket.set_write_timeout(Some(timeout)).ok()?;
    let port = socket.local_addr().ok()?.port();
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        if let Ok((_n, peer)) = socket.recv_from(&mut buf) {
            let _ = socket.send_to(response.as_bytes(), peer);
        }
    });
    Some(i64::from(port))
}

fn net_udp_send_recv(
    host: String,
    port: i64,
    message: String,
    timeout: std::time::Duration,
) -> Option<String> {
    let addr = net_loopback_socket_addr(&host, port)?;
    let socket = std::net::UdpSocket::bind(("127.0.0.1", 0)).ok()?;
    socket.set_read_timeout(Some(timeout)).ok()?;
    socket.set_write_timeout(Some(timeout)).ok()?;
    socket.send_to(message.as_bytes(), addr).ok()?;
    let mut response = vec![0u8; 64 * 1024];
    let (n, _peer) = socket.recv_from(&mut response).ok()?;
    response.truncate(n);
    String::from_utf8(response).ok()
}

fn is_http_call(name: &str) -> bool {
    matches!(
        name,
        "http_get"
            | "http_server_listen"
            | "http_server_local_port"
            | "http_server_accept"
            | "http_request_method"
            | "http_request_path"
            | "http_request_body"
            | "http_response_write"
            | "http_server_close"
            | "http_serve_once"
            | "http_serve_route"
            | "http_async_serve_route"
    )
}

fn eval_http_call(
    name: &str,
    args: &[Expr],
    functions: &HashMap<&str, &Function>,
    env: &SpikeEnv,
    lines: &mut Vec<OutputLine>,
) -> Result<SpikeValue, Diagnostic> {
    match name {
        "http_get" => {
            let [url] = args else {
                return Err(unsupported("http_get expects exactly one argument"));
            };
            let url = expect_text(eval_expr(url, functions, env, lines)?, name)?;
            Ok(spike_option(http_get(&url).map(SpikeValue::Text)))
        }
        "http_server_listen" => {
            let [bind] = args else {
                return Err(unsupported(
                    "http_server_listen expects exactly one argument",
                ));
            };
            let bind = expect_text(eval_expr(bind, functions, env, lines)?, name)?;
            Ok(SpikeValue::Int(http_server_listen(&bind).ok_or_else(
                || unsupported("http_server_listen failed in cranelift spike"),
            )?))
        }
        "http_server_local_port" => {
            let [server] = args else {
                return Err(unsupported(
                    "http_server_local_port expects exactly one argument",
                ));
            };
            let server = expect_int(eval_expr(server, functions, env, lines)?)?;
            Ok(SpikeValue::Int(http_server_local_port(server).ok_or_else(
                || unsupported("http_server_local_port failed in cranelift spike"),
            )?))
        }
        "http_server_accept" => {
            let [server] = args else {
                return Err(unsupported(
                    "http_server_accept expects exactly one argument",
                ));
            };
            let server = expect_int(eval_expr(server, functions, env, lines)?)?;
            Ok(SpikeValue::Int(http_server_accept(server).ok_or_else(
                || unsupported("http_server_accept failed in cranelift spike"),
            )?))
        }
        "http_request_method" | "http_request_path" | "http_request_body" => {
            let [request] = args else {
                return Err(unsupported(&format!("{name} expects exactly one argument")));
            };
            let request = expect_int(eval_expr(request, functions, env, lines)?)?;
            let value = http_request_part(request, name)
                .ok_or_else(|| unsupported("http request handle missing in cranelift spike"))?;
            Ok(SpikeValue::Text(value))
        }
        "http_response_write" => {
            let [request, status, body] = args else {
                return Err(unsupported(
                    "http_response_write expects exactly three arguments",
                ));
            };
            let request = expect_int(eval_expr(request, functions, env, lines)?)?;
            let status = expect_int(eval_expr(status, functions, env, lines)?)?;
            let body = expect_text(eval_expr(body, functions, env, lines)?, name)?;
            Ok(SpikeValue::Bool(http_response_write(
                request, status, &body,
            )))
        }
        "http_server_close" => {
            let [server] = args else {
                return Err(unsupported(
                    "http_server_close expects exactly one argument",
                ));
            };
            let server = expect_int(eval_expr(server, functions, env, lines)?)?;
            Ok(SpikeValue::Bool(http_server_close(server)))
        }
        "http_serve_once" => {
            let [bind, body] = args else {
                return Err(unsupported("http_serve_once expects exactly two arguments"));
            };
            let bind = expect_text(eval_expr(bind, functions, env, lines)?, name)?;
            let body = expect_text(eval_expr(body, functions, env, lines)?, name)?;
            Ok(SpikeValue::Bool(http_serve_once(&bind, &body)))
        }
        "http_serve_route" => {
            let [bind, route_path, body, max_requests] = args else {
                return Err(unsupported(
                    "http_serve_route expects exactly four arguments",
                ));
            };
            let bind = expect_text(eval_expr(bind, functions, env, lines)?, name)?;
            let route_path = expect_text(eval_expr(route_path, functions, env, lines)?, name)?;
            let body = expect_text(eval_expr(body, functions, env, lines)?, name)?;
            let max_requests = expect_int(eval_expr(max_requests, functions, env, lines)?)?;
            Ok(SpikeValue::Bool(http_serve_route(
                &bind,
                &route_path,
                &body,
                max_requests,
            )))
        }
        "http_async_serve_route" => {
            let [server, route_path, body, max_requests] = args else {
                return Err(unsupported(
                    "http_async_serve_route expects exactly four arguments",
                ));
            };
            let server = expect_int(eval_expr(server, functions, env, lines)?)?;
            let route_path = expect_text(eval_expr(route_path, functions, env, lines)?, name)?;
            let body = expect_text(eval_expr(body, functions, env, lines)?, name)?;
            let max_requests = expect_int(eval_expr(max_requests, functions, env, lines)?)?;
            Ok(spike_task(SpikeValue::Bool(http_serve_route_on_server(
                server,
                &route_path,
                &body,
                max_requests,
            ))))
        }
        _ => Err(unsupported(&format!(
            "unsupported cranelift spike http call {name:?}"
        ))),
    }
}

fn spike_http_servers() -> &'static Mutex<HashMap<i64, SpikeHttpServer>> {
    SPIKE_HTTP_SERVERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn spike_http_requests() -> &'static Mutex<HashMap<i64, SpikeHttpRequest>> {
    SPIKE_HTTP_REQUESTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn spike_http_next_handle() -> i64 {
    SPIKE_HTTP_NEXT_HANDLE.fetch_add(1, Ordering::Relaxed)
}

fn http_get(url: &str) -> Option<String> {
    let (scheme, host, port, path) = http_split_url(url)?;
    if scheme != "http" {
        return None;
    }
    let host = http_strip_crlf(host);
    let path = http_strip_crlf(path);
    if host.is_empty() || path.is_empty() {
        return None;
    }
    let request = http_request(&host, &path);
    let mut stream = None;
    for addr in (host.as_str(), port).to_socket_addrs().ok()? {
        if let Ok(candidate) =
            std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(5))
        {
            stream = Some(candidate);
            break;
        }
    }
    let mut stream = stream?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok()?;
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(5)))
        .ok()?;
    stream.write_all(request.as_bytes()).ok()?;
    http_read_response(&mut stream)
}

fn http_strip_crlf(value: &str) -> String {
    value
        .chars()
        .filter(|ch| *ch != '\r' && *ch != '\n')
        .collect()
}

fn http_split_url(url: &str) -> Option<(&str, &str, u16, &str)> {
    let (scheme, rest, default_port) = if let Some(rest) = url.strip_prefix("http://") {
        ("http", rest, 80u16)
    } else if let Some(rest) = url.strip_prefix("https://") {
        ("https", rest, 443u16)
    } else {
        return None;
    };
    let (host_port, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    if host_port.is_empty() {
        return None;
    }
    let (host, port) = match host_port.rfind(':') {
        Some(index) => {
            let parsed = host_port[index + 1..].parse().ok()?;
            (&host_port[..index], parsed)
        }
        None => (host_port, default_port),
    };
    if host.is_empty() {
        return None;
    }
    Some((scheme, host, port, path))
}

fn http_request(host: &str, path: &str) -> String {
    format!(
        "GET {} HTTP/1.0\r\nHost: {}\r\nUser-Agent: axiom-stage1/0.1\r\nConnection: close\r\n\r\n",
        path, host
    )
}

fn http_read_response<R: Read>(reader: &mut R) -> Option<String> {
    const MAX_HEADER_BYTES: usize = 64 * 1024;
    const MAX_BODY_BYTES: usize = 1024 * 1024;
    let mut raw = Vec::new();
    let mut body_start = None;
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
        if body_start.is_none() {
            if let Some(separator) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                if separator > MAX_HEADER_BYTES {
                    return None;
                }
                body_start = Some(separator + 4);
            } else if raw.len() > MAX_HEADER_BYTES {
                return None;
            }
        }
        if let Some(start) = body_start {
            if raw.len().saturating_sub(start) > MAX_BODY_BYTES {
                return None;
            }
        }
    }
    let body_start = body_start?;
    let header_end = body_start - 4;
    let head = &raw[..header_end];
    let body = &raw[body_start..];
    let status_line_end = head
        .iter()
        .position(|byte| *byte == b'\r')
        .unwrap_or(head.len());
    let status_line = std::str::from_utf8(&head[..status_line_end]).ok()?;
    let mut parts = status_line.splitn(3, ' ');
    let _version = parts.next()?;
    let status_code: u16 = parts.next()?.parse().ok()?;
    if !(200..300).contains(&status_code) {
        return None;
    }
    String::from_utf8(body.to_vec()).ok()
}

fn http_server_listen(bind: &str) -> Option<i64> {
    let addr = http_parse_loopback_bind(bind)?;
    let listener = TcpListener::bind(addr).ok()?;
    listener.set_nonblocking(true).ok()?;
    let handle = spike_http_next_handle();
    spike_http_servers()
        .lock()
        .ok()?
        .insert(handle, SpikeHttpServer { listener });
    Some(handle)
}

fn http_server_local_port(server: i64) -> Option<i64> {
    let servers = spike_http_servers().lock().ok()?;
    let server = servers.get(&server)?;
    Some(i64::from(server.listener.local_addr().ok()?.port()))
}

fn http_server_accept(server: i64) -> Option<i64> {
    let listener = {
        let servers = spike_http_servers().lock().ok()?;
        servers.get(&server)?.listener.try_clone().ok()?
    };
    let request = http_accept_request(&listener)?;
    let handle = spike_http_next_handle();
    spike_http_requests().lock().ok()?.insert(handle, request);
    Some(handle)
}

fn http_request_part(request: i64, name: &str) -> Option<String> {
    let requests = spike_http_requests().lock().ok()?;
    let request = requests.get(&request)?;
    match name {
        "http_request_method" => Some(request.method.clone()),
        "http_request_path" => Some(request.path.clone()),
        "http_request_body" => Some(request.body.clone()),
        _ => None,
    }
}

fn http_response_write(request: i64, status: i64, body: &str) -> bool {
    let Some(mut request) = spike_http_requests()
        .lock()
        .ok()
        .and_then(|mut requests| requests.remove(&request))
    else {
        return false;
    };
    let response = http_response(status, body);
    request.stream.write_all(response.as_bytes()).is_ok() && request.stream.flush().is_ok()
}

fn http_server_close(server: i64) -> bool {
    spike_http_servers()
        .lock()
        .ok()
        .and_then(|mut servers| servers.remove(&server))
        .is_some()
}

fn http_serve_once(bind: &str, body: &str) -> bool {
    let Some(server) = http_server_listen(bind) else {
        return false;
    };
    let result = http_server_accept(server)
        .map(|request| http_response_write(request, 200, body))
        .unwrap_or(false);
    let _ = http_server_close(server);
    result
}

fn http_serve_route(bind: &str, route_path: &str, body: &str, max_requests: i64) -> bool {
    let Some(server) = http_server_listen(bind) else {
        return false;
    };
    let result = http_serve_route_on_server(server, route_path, body, max_requests);
    let _ = http_server_close(server);
    result
}

fn http_serve_route_on_server(
    server: i64,
    route_path: &str,
    body: &str,
    max_requests: i64,
) -> bool {
    if max_requests <= 0 {
        return false;
    }
    let route_path = http_strip_crlf(route_path);
    if route_path.is_empty() {
        return false;
    }
    let mut served = 0i64;
    while served < max_requests {
        let Some(request) = http_server_accept(server) else {
            return false;
        };
        let Some(path) = http_request_part(request, "http_request_path") else {
            return false;
        };
        let matched = path == route_path;
        let status = if matched { 200 } else { 404 };
        let response_body = if matched { body } else { "not found" };
        if !http_response_write(request, status, response_body) {
            return false;
        }
        served += 1;
    }
    true
}

fn http_parse_loopback_bind(bind: &str) -> Option<SocketAddr> {
    let addr = bind.parse::<SocketAddr>().ok()?;
    if !addr.ip().is_loopback() {
        return None;
    }
    Some(addr)
}

fn http_accept_request(listener: &TcpListener) -> Option<SpikeHttpRequest> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match listener.accept() {
            Ok((mut stream, _peer)) => {
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .ok()?;
                stream
                    .set_write_timeout(Some(std::time::Duration::from_secs(2)))
                    .ok()?;
                let (method, path, body) = http_read_request(&mut stream)?;
                return Some(SpikeHttpRequest {
                    stream,
                    method,
                    path,
                    body,
                });
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(_) => return None,
        }
    }
}

fn http_read_request<R: Read>(reader: &mut R) -> Option<(String, String, String)> {
    const MAX_HEADER_BYTES: usize = 64 * 1024;
    const MAX_BODY_BYTES: usize = 1024 * 1024;
    let mut raw = Vec::new();
    let mut header_end = None;
    let mut content_length = 0usize;
    let mut buf = [0u8; 4096];
    loop {
        let n = reader.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
        if header_end.is_none() {
            if let Some(separator) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                if separator > MAX_HEADER_BYTES {
                    return None;
                }
                header_end = Some(separator + 4);
                let headers = std::str::from_utf8(&raw[..separator]).ok()?;
                content_length = http_content_length(headers)?;
                if content_length > MAX_BODY_BYTES {
                    return None;
                }
            } else if raw.len() > MAX_HEADER_BYTES {
                return None;
            }
        }
        if let Some(end) = header_end {
            if raw.len().saturating_sub(end) >= content_length {
                break;
            }
        }
    }
    let header_end = header_end?;
    let header = std::str::from_utf8(&raw[..header_end - 4]).ok()?;
    let (method, path) = http_request_line(header)?;
    let body_end = header_end.checked_add(content_length)?;
    let body = String::from_utf8(raw.get(header_end..body_end)?.to_vec()).ok()?;
    Some((method, path, body))
}

fn http_content_length(headers: &str) -> Option<usize> {
    for line in headers.lines().skip(1) {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            return value.trim().parse().ok();
        }
    }
    Some(0)
}

fn http_request_line(headers: &str) -> Option<(String, String)> {
    let line = headers.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = http_strip_crlf(parts.next()?);
    let path = http_strip_crlf(parts.next()?);
    if method.is_empty() || path.is_empty() {
        return None;
    }
    Some((method, path))
}

fn http_response(status: i64, body: &str) -> String {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    };
    format!(
        "HTTP/1.0 {} {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        reason,
        body.len(),
        body
    )
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
    Ok(spike_option(
        spike_fs_read_text(env, &path)?.map(SpikeValue::Text),
    ))
}

fn spike_fs_read_text(env: &SpikeEnv, path: &str) -> Result<Option<String>, Diagnostic> {
    let fs_root = spike_fs_root(env)?;
    Ok(spike_fs_read_text_for_root(&fs_root, path))
}

fn spike_fs_read_text_for_root(fs_root: &Path, path: &str) -> Option<String> {
    let candidate = spike_fs_existing_candidate_for_root(fs_root, path)?;
    let metadata = std::fs::metadata(&candidate).ok()?;
    if !metadata.is_file() || metadata.len() > SPIKE_MAX_FS_READ_BYTES {
        return None;
    }
    let file = std::fs::File::open(&candidate).ok()?;
    let mut reader = file.take(SPIKE_MAX_FS_READ_BYTES + 1);
    let mut content = String::new();
    if reader.read_to_string(&mut content).is_err()
        || content.len() as u64 > SPIKE_MAX_FS_READ_BYTES
    {
        return None;
    }
    Some(content)
}

fn i64_fs_write_result(
    name: &str,
    args: &[Expr],
    static_bindings: &I64StaticBindings,
) -> Option<i64> {
    let fs_root = static_bindings.fs_root.as_deref()?;
    match name {
        "fs_write" => {
            let (path, content) = i64_fs_path_content(args, static_bindings)?;
            if content.len() > SPIKE_MAX_FS_WRITE_BYTES {
                return Some(-1);
            }
            Some(
                spike_fs_write_candidate_for_root(fs_root, &path, false)
                    .and_then(|candidate| std::fs::write(candidate, content).ok())
                    .map(|()| 0)
                    .unwrap_or(-1),
            )
        }
        "fs_create" => {
            let path = i64_fs_path(args, static_bindings)?;
            Some(
                spike_fs_write_candidate_for_root(fs_root, &path, false)
                    .and_then(|candidate| {
                        std::fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(candidate)
                            .ok()
                    })
                    .map(|_| 0)
                    .unwrap_or(-1),
            )
        }
        "fs_append" => {
            let (path, content) = i64_fs_path_content(args, static_bindings)?;
            if content.len() > SPIKE_MAX_FS_WRITE_BYTES {
                return Some(-1);
            }
            Some(
                spike_fs_write_candidate_for_root(fs_root, &path, false)
                    .and_then(|candidate| {
                        let mut file = std::fs::OpenOptions::new()
                            .append(true)
                            .create(true)
                            .open(candidate)
                            .ok()?;
                        std::io::Write::write_all(&mut file, content.as_bytes()).ok()
                    })
                    .map(|()| 0)
                    .unwrap_or(-1),
            )
        }
        "fs_mkdir" => {
            let path = i64_fs_path(args, static_bindings)?;
            Some(
                spike_fs_write_candidate_for_root(fs_root, &path, false)
                    .and_then(|candidate| std::fs::create_dir(candidate).ok())
                    .map(|()| 0)
                    .unwrap_or(-1),
            )
        }
        "fs_mkdir_all" => {
            let path = i64_fs_path(args, static_bindings)?;
            Some(
                spike_fs_write_candidate_for_root(fs_root, &path, true)
                    .and_then(|candidate| std::fs::create_dir_all(candidate).ok())
                    .map(|()| 0)
                    .unwrap_or(-1),
            )
        }
        "fs_remove_file" => {
            let path = i64_fs_path(args, static_bindings)?;
            Some(
                spike_fs_existing_candidate_for_root(fs_root, &path)
                    .and_then(|candidate| {
                        std::fs::metadata(&candidate)
                            .ok()
                            .filter(|metadata| metadata.is_file())?;
                        std::fs::remove_file(candidate).ok()
                    })
                    .map(|()| 0)
                    .unwrap_or(-1),
            )
        }
        "fs_remove_dir" => {
            let path = i64_fs_path(args, static_bindings)?;
            Some(
                spike_fs_existing_candidate_for_root(fs_root, &path)
                    .and_then(|candidate| {
                        std::fs::metadata(&candidate)
                            .ok()
                            .filter(|metadata| metadata.is_dir())?;
                        std::fs::remove_dir(candidate).ok()
                    })
                    .map(|()| 0)
                    .unwrap_or(-1),
            )
        }
        "fs_replace" => {
            let (path, content) = i64_fs_path_content(args, static_bindings)?;
            if content.len() > SPIKE_MAX_FS_WRITE_BYTES {
                return Some(-1);
            }
            Some(
                spike_fs_write_candidate_for_root(fs_root, &path, false)
                    .and_then(|candidate| std::fs::write(candidate, content).ok())
                    .map(|()| 0)
                    .unwrap_or(-1),
            )
        }
        _ => None,
    }
}

fn i64_fs_path(args: &[Expr], static_bindings: &I64StaticBindings) -> Option<String> {
    let [path] = args else {
        return None;
    };
    i64_string_text(path, static_bindings)
}

fn i64_fs_path_content(
    args: &[Expr],
    static_bindings: &I64StaticBindings,
) -> Option<(String, String)> {
    let [path, content] = args else {
        return None;
    };
    Some((
        i64_string_text(path, static_bindings)?,
        i64_string_text(content, static_bindings)?,
    ))
}

fn i64_net_resolve_text(host: &str) -> Option<String> {
    (host, 0)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
        .map(|addr| addr.ip().to_string())
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
    Ok(spike_fs_existing_candidate_for_root(&fs_root, path))
}

fn spike_fs_existing_candidate_for_root(fs_root: &Path, path: &str) -> Option<PathBuf> {
    let candidate = spike_fs_join_candidate(fs_root, path)?;
    let canonical_root = std::fs::canonicalize(fs_root).ok()?;
    let canonical_candidate = std::fs::canonicalize(candidate).ok()?;
    canonical_candidate
        .starts_with(canonical_root)
        .then_some(canonical_candidate)
}

fn spike_fs_write_candidate(
    env: &SpikeEnv,
    path: &str,
    allow_missing_ancestors: bool,
) -> Result<Option<PathBuf>, Diagnostic> {
    let fs_root = spike_fs_root(env)?;
    Ok(spike_fs_write_candidate_for_root(
        &fs_root,
        path,
        allow_missing_ancestors,
    ))
}

fn spike_fs_write_candidate_for_root(
    fs_root: &Path,
    path: &str,
    allow_missing_ancestors: bool,
) -> Option<PathBuf> {
    let candidate = spike_fs_join_candidate(fs_root, path)?;
    let Ok(canonical_root) = std::fs::canonicalize(fs_root) else {
        return None;
    };
    if let Ok(canonical_candidate) = std::fs::canonicalize(&candidate) {
        return canonical_candidate
            .starts_with(canonical_root)
            .then_some(canonical_candidate);
    }
    let parent = candidate.parent()?;
    if !allow_missing_ancestors {
        let Ok(canonical_parent) = std::fs::canonicalize(parent) else {
            return None;
        };
        if !canonical_parent.starts_with(&canonical_root) {
            return None;
        }
        let file_name = candidate.file_name()?;
        return Some(canonical_parent.join(file_name));
    }
    let mut ancestor = parent;
    while !ancestor.exists() {
        let parent = ancestor.parent()?;
        ancestor = parent;
    }
    let Ok(canonical_ancestor) = std::fs::canonicalize(ancestor) else {
        return None;
    };
    canonical_ancestor
        .starts_with(canonical_root)
        .then_some(candidate)
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
    let status = match command.as_str() {
        "/usr/bin/true" => 0,
        "/usr/bin/false" => 1,
        "__axiom_stage1_missing_binary__" => -1,
        _ => {
            return Err(unsupported(
                "process_status spike only permits allowlisted deterministic commands",
            ));
        }
    };
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

fn expect_int(value: SpikeValue) -> Result<i64, Diagnostic> {
    match value {
        SpikeValue::Int(value) => Ok(value),
        SpikeValue::UInt(value) => {
            i64::try_from(value).map_err(|_| unsupported("integer value is outside the i64 range"))
        }
        _ => Err(unsupported("expected integer expression")),
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
    let result = match (&left, &right) {
        (SpikeValue::Int(left), SpikeValue::Int(right)) => compare_ord(op, *left, *right),
        (SpikeValue::UInt(left), SpikeValue::UInt(right)) => compare_ord(op, *left, *right),
        (SpikeValue::Float(left), SpikeValue::Float(right)) => compare_float(op, *left, *right)?,
        (SpikeValue::Bool(left), SpikeValue::Bool(right)) => compare_eq(op, *left, *right)?,
        (SpikeValue::Text(left), SpikeValue::Text(right)) => {
            compare_eq(op, left.as_str(), right.as_str())?
        }
        _ if matches!(op, CompareOp::Eq | CompareOp::Ne) => {
            let equal = spike_values_equal(&left, &right)?;
            matches!(op, CompareOp::Eq) == equal
        }
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
            | SpikeValue::JoinHandle(_)
            | SpikeValue::AsyncChannel { .. }
            | SpikeValue::SelectResult { .. },
            _,
        )
        | (
            _,
            SpikeValue::Task { .. }
            | SpikeValue::JoinHandle(_)
            | SpikeValue::AsyncChannel { .. }
            | SpikeValue::SelectResult { .. },
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

fn encoding_is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
}

fn percent_encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::new();
    for byte in value.bytes() {
        if encoding_is_unreserved(byte) {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    out
}

fn percent_decode(value: &str) -> Option<String> {
    fn hex(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    let bytes = value.as_bytes();
    let mut index = 0usize;
    let mut out = Vec::new();
    while index < bytes.len() {
        if bytes[index] != b'%' {
            out.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return None;
        }
        let high = hex(bytes[index + 1])?;
        let low = hex(bytes[index + 2])?;
        out.push((high << 4) | low);
        index += 3;
    }
    String::from_utf8(out).ok()
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
        | SpikeValue::Array(_)
        | SpikeValue::Task { .. }
        | SpikeValue::JoinHandle(_)
        | SpikeValue::AsyncChannel { .. }
        | SpikeValue::SelectResult { .. } => Err(unsupported(
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
        SpikeValue::Task { canceled, .. } => {
            format!("Task {{ canceled: {canceled} }}")
        }
        SpikeValue::JoinHandle(_) => String::from("JoinHandle"),
        SpikeValue::AsyncChannel { slot } => {
            format!("AsyncChannel {{ occupied: {} }}", slot.is_some())
        }
        SpikeValue::SelectResult { selected, value } => format!(
            "SelectResult {{ selected: {selected}, value: {} }}",
            render_value(&spike_option(value.as_ref().map(|value| (**value).clone())))
        ),
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
            lower_i64_map_contains_key_condition("contains", &args, &static_bindings),
            Some(CraneliftI64Condition::Literal(true))
        );
        assert_eq!(
            lower_i64_map_contains_key_condition(
                "contains",
                &[map.clone(), missing_key],
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
            spike_fs_write_candidate_for_root(root, "dangling.txt", false),
            None
        );
    }
}
