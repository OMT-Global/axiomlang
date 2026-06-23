use cranelift_codegen::ir::{
    AbiParam, BlockArg, FuncRef, InstBuilder, MemFlags, StackSlotData, StackSlotKind,
    condcodes::IntCC, types,
};
use cranelift_codegen::isa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataDescription, FuncId, Linkage, Module, default_libcall_names};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::error::Error;
use std::fmt;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const I64_REALPATH_BUFFER_BYTES: u32 = 4096;
const I64_TIMESPEC_BYTES: u32 = 16;
const I64_TIMESPEC_SECONDS_OFFSET: i32 = 0;
const I64_TIMESPEC_NANOS_OFFSET: i32 = 8;
const I64_TIME_UTC_BASE: i64 = 1;

#[derive(Debug)]
pub struct CraneliftBackendError {
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLine {
    pub stream: OutputStream,
    pub text: String,
}

struct I64OutputData {
    stream: OutputStream,
    text: String,
    append_newline: bool,
    data_id: cranelift_module::DataId,
    byte_len: usize,
}

#[derive(Clone, Copy)]
struct I64RuntimeRefs {
    write: FuncRef,
    read: FuncRef,
    sleep: FuncRef,
    timespec_get: FuncRef,
    getenv: FuncRef,
    strlen: FuncRef,
    atoll: FuncRef,
    open: FuncRef,
    creat: FuncRef,
    lseek: FuncRef,
    close: FuncRef,
    access: FuncRef,
    system: FuncRef,
    fopen: FuncRef,
    fwrite: FuncRef,
    fclose: FuncRef,
    unlink: FuncRef,
    rename: FuncRef,
    mkdir: FuncRef,
    rmdir: FuncRef,
    opendir: FuncRef,
    closedir: FuncRef,
    realpath: FuncRef,
    strncmp: FuncRef,
    getaddrinfo: Option<FuncRef>,
    freeaddrinfo: Option<FuncRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I64BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I64CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I64Cast {
    Signed8,
    Signed16,
    Signed32,
    Signed64,
    Unsigned8,
    Unsigned16,
    Unsigned32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I64AuditSuccess {
    ExitZero,
    NonNegative,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum I64Expr {
    Literal(i64),
    Local(usize),
    ClockNowMs,
    ClockElapsedMs {
        start: Box<I64Expr>,
    },
    SleepMs {
        milliseconds: Box<I64Expr>,
    },
    EnvLen {
        key: String,
    },
    AuditEnv {
        intrinsic: String,
        package: String,
        key_len: usize,
        success: I64AuditSuccess,
        result: Box<I64Expr>,
    },
    AuditProcess {
        intrinsic: String,
        package: String,
        command_len: usize,
        success: I64AuditSuccess,
        result: Box<I64Expr>,
    },
    AuditClock {
        intrinsic: String,
        package: String,
        arg_name: String,
        success: I64AuditSuccess,
        result: Box<I64Expr>,
    },
    CStringLen {
        value: String,
    },
    AuditFfi {
        intrinsic: String,
        package: String,
        library: String,
        symbol: String,
        arg_type: String,
        success: I64AuditSuccess,
        result: Box<I64Expr>,
    },
    RandomU64 {
        intrinsic: String,
        package: String,
    },
    RandomBytesLen {
        length: Box<I64Expr>,
    },
    AuditCrypto {
        intrinsic: String,
        package: String,
        arg_name: String,
        arg_value: String,
        success: I64AuditSuccess,
        result: Box<I64Expr>,
    },
    FileLen {
        path: String,
        max_bytes: u64,
    },
    AuditFs {
        intrinsic: String,
        package: String,
        path_len: usize,
        content_len: Option<usize>,
        success: I64AuditSuccess,
        result: Box<I64Expr>,
    },
    RuntimeFsGuard {
        root: String,
        path: String,
        fallback_path: String,
        result: Box<I64Expr>,
    },
    WriteFile {
        path: String,
        content: String,
    },
    AppendFile {
        path: String,
        content: String,
    },
    ReplaceFile {
        path: String,
        temp_path: String,
        content: String,
    },
    CreateFile {
        path: String,
    },
    RemoveFile {
        path: String,
    },
    MakeDir {
        path: String,
    },
    MakeDirAll {
        path: String,
    },
    RemoveDir {
        path: String,
    },
    ProcessStatus {
        command: String,
    },
    NetResolveLen {
        host: String,
        resolved_len: i64,
    },
    AuditNet {
        intrinsic: String,
        package: String,
        host_len: usize,
        success: I64AuditSuccess,
        result: Box<I64Expr>,
    },
    ConditionValue(Box<I64Condition>),
    Cast {
        cast: I64Cast,
        expr: Box<I64Expr>,
    },
    Call {
        function: usize,
        args: Vec<I64Expr>,
    },
    Binary {
        op: I64BinaryOp,
        lhs: Box<I64Expr>,
        rhs: Box<I64Expr>,
    },
    Select {
        cond: Box<I64Condition>,
        then_result: Box<I64Expr>,
        else_result: Box<I64Expr>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I64Compare {
    pub op: I64CompareOp,
    pub lhs: I64Expr,
    pub rhs: I64Expr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum I64Condition {
    Literal(bool),
    Compare(I64Compare),
    And {
        lhs: Box<I64Condition>,
        rhs: Box<I64Condition>,
    },
    Or {
        lhs: Box<I64Condition>,
        rhs: Box<I64Condition>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum I64ExitBody {
    Return(I64Expr),
    BlockReturn(I64ReturnBlock),
    IfReturn {
        cond: I64Condition,
        then_result: I64Expr,
        else_result: I64Expr,
    },
    IfBlockReturn {
        cond: I64Condition,
        then_block: I64ReturnBlock,
        else_block: I64ReturnBlock,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I64ReturnBlock {
    pub stmts: Vec<I64Stmt>,
    pub result: I64Expr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum I64ValueBody {
    Return(Vec<I64Expr>),
    BlockReturn(I64ValueReturnBlock),
    IfReturn {
        cond: I64Condition,
        then_results: Vec<I64Expr>,
        else_results: Vec<I64Expr>,
    },
    IfBlockReturn {
        cond: I64Condition,
        then_block: I64ValueReturnBlock,
        else_block: I64ValueReturnBlock,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I64ValueReturnBlock {
    pub stmts: Vec<I64Stmt>,
    pub results: Vec<I64Expr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I64Assign {
    pub local: usize,
    pub value: I64Expr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum I64Stmt {
    Assign(I64Assign),
    WriteText {
        stream: OutputStream,
        text: String,
    },
    WriteLine {
        stream: OutputStream,
        text: String,
    },
    WriteI64Text {
        stream: OutputStream,
        value: I64Expr,
    },
    WriteI64Line {
        stream: OutputStream,
        value: I64Expr,
    },
    CallAssign {
        locals: Vec<usize>,
        function: usize,
        args: Vec<I64Expr>,
    },
    If {
        cond: I64Condition,
        then_body: Vec<I64Stmt>,
        else_body: Vec<I64Stmt>,
    },
    While {
        cond: I64Condition,
        body: Vec<I64Stmt>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I64Function {
    pub params: usize,
    pub returns: usize,
    pub locals: Vec<I64Expr>,
    pub stmts: Vec<I64Stmt>,
    pub body: I64ValueBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I64ExitProgram {
    pub functions: Vec<I64Function>,
    pub locals: Vec<I64Expr>,
    pub stmts: Vec<I64Stmt>,
    pub body: I64ExitBody,
}

impl OutputLine {
    pub fn stdout(text: impl Into<String>) -> Self {
        Self {
            stream: OutputStream::Stdout,
            text: text.into(),
        }
    }

    pub fn stderr(text: impl Into<String>) -> Self {
        Self {
            stream: OutputStream::Stderr,
            text: text.into(),
        }
    }
}

impl CraneliftBackendError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CraneliftBackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for CraneliftBackendError {}

pub fn compile_print_lines(
    lines: &[String],
    object_path: &Path,
    binary_path: &Path,
) -> Result<(), CraneliftBackendError> {
    let lines = lines
        .iter()
        .cloned()
        .map(OutputLine::stdout)
        .collect::<Vec<_>>();
    compile_output_lines(&lines, object_path, binary_path)
}

pub fn compile_output_lines(
    lines: &[OutputLine],
    object_path: &Path,
    binary_path: &Path,
) -> Result<(), CraneliftBackendError> {
    emit_cranelift_object(lines, object_path)?;
    link_object(object_path, binary_path)
}

pub fn compile_i64_exit_program(
    program: I64ExitProgram,
    object_path: &Path,
    binary_path: &Path,
) -> Result<(), CraneliftBackendError> {
    emit_i64_exit_object(program, object_path)?;
    link_object(object_path, binary_path)
}

fn emit_cranelift_object(
    lines: &[OutputLine],
    object_path: &Path,
) -> Result<(), CraneliftBackendError> {
    let isa_builder = host_isa_builder()?;
    let mut flag_builder = settings::builder();
    flag_builder.set("is_pic", "true").map_err(|message| {
        CraneliftBackendError::new(format!("cranelift flag setup: {message}"))
    })?;
    let flags = settings::Flags::new(flag_builder);
    let isa = isa_builder
        .finish(flags)
        .map_err(|message| CraneliftBackendError::new(format!("cranelift ISA setup: {message}")))?;
    let builder = ObjectBuilder::new(isa, "axiom_cranelift_hello", default_libcall_names())
        .map_err(|message| {
            CraneliftBackendError::new(format!("cranelift object setup: {message}"))
        })?;
    let mut module = ObjectModule::new(builder);
    let pointer_type = module.target_config().pointer_type();

    let mut write_sig = module.make_signature();
    write_sig.params.push(AbiParam::new(types::I32));
    write_sig.params.push(AbiParam::new(pointer_type));
    write_sig.params.push(AbiParam::new(pointer_type));
    write_sig.returns.push(AbiParam::new(pointer_type));
    let write_id = module
        .declare_function("write", Linkage::Import, &write_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare write import: {message}"))
        })?;
    let mut data_ids = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let data_id = module
            .declare_data(
                &format!("__axiom_line_{index}"),
                Linkage::Local,
                false,
                false,
            )
            .map_err(|message| CraneliftBackendError::new(format!("declare data: {message}")))?;
        let mut description = DataDescription::new();
        let mut bytes = line.text.as_bytes().to_vec();
        bytes.push(b'\n');
        let byte_len = bytes.len();
        description.define(bytes.into_boxed_slice());
        module
            .define_data(data_id, &description)
            .map_err(|message| CraneliftBackendError::new(format!("define data: {message}")))?;
        data_ids.push((line.stream, data_id, byte_len));
    }

    let mut context = module.make_context();
    context
        .func
        .signature
        .returns
        .push(AbiParam::new(types::I32));
    let main_id = module
        .declare_function("main", Linkage::Export, &context.func.signature)
        .map_err(|message| CraneliftBackendError::new(format!("declare main: {message}")))?;
    let mut builder_context = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);
        let write_ref = module.declare_func_in_func(write_id, builder.func);
        for (stream, data_id, byte_len) in data_ids {
            let data_ref = module.declare_data_in_func(data_id, builder.func);
            let pointer = builder.ins().global_value(pointer_type, data_ref);
            let fd = builder.ins().iconst(
                types::I32,
                match stream {
                    OutputStream::Stdout => 1,
                    OutputStream::Stderr => 2,
                },
            );
            let len = builder.ins().iconst(pointer_type, byte_len as i64);
            builder.ins().call(write_ref, &[fd, pointer, len]);
        }
        let ok = builder.ins().iconst(types::I32, 0);
        builder.ins().return_(&[ok]);
        builder.finalize();
    }
    module
        .define_function(main_id, &mut context)
        .map_err(|message| CraneliftBackendError::new(format!("define main: {message}")))?;
    module.clear_context(&mut context);
    let product = module.finish();
    let bytes = product.emit().map_err(|message| {
        CraneliftBackendError::new(format!("emit cranelift object: {message}"))
    })?;
    write_output_file(object_path, bytes)
}

fn emit_i64_exit_object(
    program: I64ExitProgram,
    object_path: &Path,
) -> Result<(), CraneliftBackendError> {
    let isa_builder = host_isa_builder()?;
    let mut flag_builder = settings::builder();
    flag_builder.set("is_pic", "true").map_err(|message| {
        CraneliftBackendError::new(format!("cranelift flag setup: {message}"))
    })?;
    let flags = settings::Flags::new(flag_builder);
    let isa = isa_builder
        .finish(flags)
        .map_err(|message| CraneliftBackendError::new(format!("cranelift ISA setup: {message}")))?;
    let builder = ObjectBuilder::new(isa, "axiom_cranelift_i64_exit", default_libcall_names())
        .map_err(|message| {
            CraneliftBackendError::new(format!("cranelift object setup: {message}"))
        })?;
    let mut module = ObjectModule::new(builder);
    let mut write_sig = module.make_signature();
    write_sig.params.push(AbiParam::new(types::I32));
    let pointer_type = module.target_config().pointer_type();
    write_sig.params.push(AbiParam::new(pointer_type));
    write_sig.params.push(AbiParam::new(pointer_type));
    write_sig.returns.push(AbiParam::new(pointer_type));
    let write_id = module
        .declare_function("write", Linkage::Import, &write_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare write import: {message}"))
        })?;
    let mut read_sig = module.make_signature();
    read_sig.params.push(AbiParam::new(types::I32));
    read_sig.params.push(AbiParam::new(pointer_type));
    read_sig.params.push(AbiParam::new(pointer_type));
    read_sig.returns.push(AbiParam::new(pointer_type));
    let read_id = module
        .declare_function("read", Linkage::Import, &read_sig)
        .map_err(|message| CraneliftBackendError::new(format!("declare read import: {message}")))?;
    let mut sleep_sig = module.make_signature();
    sleep_sig.params.push(AbiParam::new(types::I32));
    sleep_sig.returns.push(AbiParam::new(types::I32));
    let sleep_id = module
        .declare_function("usleep", Linkage::Import, &sleep_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare usleep import: {message}"))
        })?;
    let mut timespec_get_sig = module.make_signature();
    timespec_get_sig.params.push(AbiParam::new(pointer_type));
    timespec_get_sig.params.push(AbiParam::new(types::I32));
    timespec_get_sig.returns.push(AbiParam::new(types::I32));
    let timespec_get_id = module
        .declare_function("timespec_get", Linkage::Import, &timespec_get_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare timespec_get import: {message}"))
        })?;
    let mut getenv_sig = module.make_signature();
    getenv_sig.params.push(AbiParam::new(pointer_type));
    getenv_sig.returns.push(AbiParam::new(pointer_type));
    let getenv_id = module
        .declare_function("getenv", Linkage::Import, &getenv_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare getenv import: {message}"))
        })?;
    let mut strlen_sig = module.make_signature();
    strlen_sig.params.push(AbiParam::new(pointer_type));
    strlen_sig.returns.push(AbiParam::new(pointer_type));
    let strlen_id = module
        .declare_function("strlen", Linkage::Import, &strlen_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare strlen import: {message}"))
        })?;
    let mut atoll_sig = module.make_signature();
    atoll_sig.params.push(AbiParam::new(pointer_type));
    atoll_sig.returns.push(AbiParam::new(types::I64));
    let atoll_id = module
        .declare_function("atoll", Linkage::Import, &atoll_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare atoll import: {message}"))
        })?;
    let mut open_sig = module.make_signature();
    open_sig.params.push(AbiParam::new(pointer_type));
    open_sig.params.push(AbiParam::new(types::I32));
    open_sig.returns.push(AbiParam::new(types::I32));
    let open_id = module
        .declare_function("open", Linkage::Import, &open_sig)
        .map_err(|message| CraneliftBackendError::new(format!("declare open import: {message}")))?;
    let mut creat_sig = module.make_signature();
    creat_sig.params.push(AbiParam::new(pointer_type));
    creat_sig.params.push(AbiParam::new(types::I32));
    creat_sig.returns.push(AbiParam::new(types::I32));
    let creat_id = module
        .declare_function("creat", Linkage::Import, &creat_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare creat import: {message}"))
        })?;
    let mut lseek_sig = module.make_signature();
    lseek_sig.params.push(AbiParam::new(types::I32));
    lseek_sig.params.push(AbiParam::new(types::I64));
    lseek_sig.params.push(AbiParam::new(types::I32));
    lseek_sig.returns.push(AbiParam::new(types::I64));
    let lseek_id = module
        .declare_function("lseek", Linkage::Import, &lseek_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare lseek import: {message}"))
        })?;
    let mut close_sig = module.make_signature();
    close_sig.params.push(AbiParam::new(types::I32));
    close_sig.returns.push(AbiParam::new(types::I32));
    let close_id = module
        .declare_function("close", Linkage::Import, &close_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare close import: {message}"))
        })?;
    let mut access_sig = module.make_signature();
    access_sig.params.push(AbiParam::new(pointer_type));
    access_sig.params.push(AbiParam::new(types::I32));
    access_sig.returns.push(AbiParam::new(types::I32));
    let access_id = module
        .declare_function("access", Linkage::Import, &access_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare access import: {message}"))
        })?;
    let mut system_sig = module.make_signature();
    system_sig.params.push(AbiParam::new(pointer_type));
    system_sig.returns.push(AbiParam::new(types::I32));
    let system_id = module
        .declare_function("system", Linkage::Import, &system_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare system import: {message}"))
        })?;
    let mut fopen_sig = module.make_signature();
    fopen_sig.params.push(AbiParam::new(pointer_type));
    fopen_sig.params.push(AbiParam::new(pointer_type));
    fopen_sig.returns.push(AbiParam::new(pointer_type));
    let fopen_id = module
        .declare_function("fopen", Linkage::Import, &fopen_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare fopen import: {message}"))
        })?;
    let mut fwrite_sig = module.make_signature();
    fwrite_sig.params.push(AbiParam::new(pointer_type));
    fwrite_sig.params.push(AbiParam::new(pointer_type));
    fwrite_sig.params.push(AbiParam::new(pointer_type));
    fwrite_sig.params.push(AbiParam::new(pointer_type));
    fwrite_sig.returns.push(AbiParam::new(pointer_type));
    let fwrite_id = module
        .declare_function("fwrite", Linkage::Import, &fwrite_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare fwrite import: {message}"))
        })?;
    let mut fclose_sig = module.make_signature();
    fclose_sig.params.push(AbiParam::new(pointer_type));
    fclose_sig.returns.push(AbiParam::new(types::I32));
    let fclose_id = module
        .declare_function("fclose", Linkage::Import, &fclose_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare fclose import: {message}"))
        })?;
    let mut unlink_sig = module.make_signature();
    unlink_sig.params.push(AbiParam::new(pointer_type));
    unlink_sig.returns.push(AbiParam::new(types::I32));
    let unlink_id = module
        .declare_function("unlink", Linkage::Import, &unlink_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare unlink import: {message}"))
        })?;
    let mut rename_sig = module.make_signature();
    rename_sig.params.push(AbiParam::new(pointer_type));
    rename_sig.params.push(AbiParam::new(pointer_type));
    rename_sig.returns.push(AbiParam::new(types::I32));
    let rename_id = module
        .declare_function("rename", Linkage::Import, &rename_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare rename import: {message}"))
        })?;
    let mut mkdir_sig = module.make_signature();
    mkdir_sig.params.push(AbiParam::new(pointer_type));
    mkdir_sig.params.push(AbiParam::new(types::I32));
    mkdir_sig.returns.push(AbiParam::new(types::I32));
    let mkdir_id = module
        .declare_function("mkdir", Linkage::Import, &mkdir_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare mkdir import: {message}"))
        })?;
    let mut rmdir_sig = module.make_signature();
    rmdir_sig.params.push(AbiParam::new(pointer_type));
    rmdir_sig.returns.push(AbiParam::new(types::I32));
    let rmdir_id = module
        .declare_function("rmdir", Linkage::Import, &rmdir_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare rmdir import: {message}"))
        })?;
    let mut opendir_sig = module.make_signature();
    opendir_sig.params.push(AbiParam::new(pointer_type));
    opendir_sig.returns.push(AbiParam::new(pointer_type));
    let opendir_id = module
        .declare_function("opendir", Linkage::Import, &opendir_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare opendir import: {message}"))
        })?;
    let mut closedir_sig = module.make_signature();
    closedir_sig.params.push(AbiParam::new(pointer_type));
    closedir_sig.returns.push(AbiParam::new(types::I32));
    let closedir_id = module
        .declare_function("closedir", Linkage::Import, &closedir_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare closedir import: {message}"))
        })?;
    let mut realpath_sig = module.make_signature();
    realpath_sig.params.push(AbiParam::new(pointer_type));
    realpath_sig.params.push(AbiParam::new(pointer_type));
    realpath_sig.returns.push(AbiParam::new(pointer_type));
    let realpath_id = module
        .declare_function("realpath", Linkage::Import, &realpath_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare realpath import: {message}"))
        })?;
    let mut strncmp_sig = module.make_signature();
    strncmp_sig.params.push(AbiParam::new(pointer_type));
    strncmp_sig.params.push(AbiParam::new(pointer_type));
    strncmp_sig.params.push(AbiParam::new(pointer_type));
    strncmp_sig.returns.push(AbiParam::new(types::I32));
    let strncmp_id = module
        .declare_function("strncmp", Linkage::Import, &strncmp_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare strncmp import: {message}"))
        })?;
    #[cfg(not(windows))]
    let mut getaddrinfo_sig = module.make_signature();
    #[cfg(not(windows))]
    getaddrinfo_sig.params.push(AbiParam::new(pointer_type));
    #[cfg(not(windows))]
    getaddrinfo_sig.params.push(AbiParam::new(pointer_type));
    #[cfg(not(windows))]
    getaddrinfo_sig.params.push(AbiParam::new(pointer_type));
    #[cfg(not(windows))]
    getaddrinfo_sig.params.push(AbiParam::new(pointer_type));
    #[cfg(not(windows))]
    getaddrinfo_sig.returns.push(AbiParam::new(types::I32));
    #[cfg(not(windows))]
    let getaddrinfo_id = module
        .declare_function("getaddrinfo", Linkage::Import, &getaddrinfo_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare getaddrinfo import: {message}"))
        })?;
    #[cfg(not(windows))]
    let mut freeaddrinfo_sig = module.make_signature();
    #[cfg(not(windows))]
    freeaddrinfo_sig.params.push(AbiParam::new(pointer_type));
    #[cfg(not(windows))]
    let freeaddrinfo_id = module
        .declare_function("freeaddrinfo", Linkage::Import, &freeaddrinfo_sig)
        .map_err(|message| {
            CraneliftBackendError::new(format!("declare freeaddrinfo import: {message}"))
        })?;
    let output_data_ids = declare_i64_output_data(&mut module, &program)?;
    let function_ids = declare_i64_functions(&mut module, &program.functions)?;

    for (index, function) in program.functions.iter().enumerate() {
        #[cfg(not(windows))]
        {
            define_i64_function(
                &mut module,
                &function_ids,
                write_id,
                read_id,
                sleep_id,
                timespec_get_id,
                getenv_id,
                strlen_id,
                atoll_id,
                open_id,
                creat_id,
                lseek_id,
                close_id,
                access_id,
                system_id,
                fopen_id,
                fwrite_id,
                fclose_id,
                unlink_id,
                rename_id,
                mkdir_id,
                rmdir_id,
                opendir_id,
                closedir_id,
                realpath_id,
                strncmp_id,
                getaddrinfo_id,
                freeaddrinfo_id,
                &output_data_ids,
                index,
                function,
            )?;
            continue;
        }
        #[cfg(windows)]
        define_i64_function(
            &mut module,
            &function_ids,
            write_id,
            read_id,
            sleep_id,
            timespec_get_id,
            getenv_id,
            strlen_id,
            atoll_id,
            open_id,
            creat_id,
            lseek_id,
            close_id,
            access_id,
            system_id,
            fopen_id,
            fwrite_id,
            fclose_id,
            unlink_id,
            rename_id,
            mkdir_id,
            rmdir_id,
            opendir_id,
            closedir_id,
            realpath_id,
            strncmp_id,
            &output_data_ids,
            index,
            function,
        )?;
    }

    let mut context = module.make_context();
    context
        .func
        .signature
        .returns
        .push(AbiParam::new(types::I32));
    let main_id = module
        .declare_function("main", Linkage::Export, &context.func.signature)
        .map_err(|message| CraneliftBackendError::new(format!("declare main: {message}")))?;
    let mut builder_context = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);
        let function_refs = i64_function_refs(&mut module, &mut builder, &function_ids);
        let write_ref = module.declare_func_in_func(write_id, builder.func);
        let read_ref = module.declare_func_in_func(read_id, builder.func);
        let sleep_ref = module.declare_func_in_func(sleep_id, builder.func);
        let timespec_get_ref = module.declare_func_in_func(timespec_get_id, builder.func);
        let getenv_ref = module.declare_func_in_func(getenv_id, builder.func);
        let strlen_ref = module.declare_func_in_func(strlen_id, builder.func);
        let atoll_ref = module.declare_func_in_func(atoll_id, builder.func);
        let open_ref = module.declare_func_in_func(open_id, builder.func);
        let creat_ref = module.declare_func_in_func(creat_id, builder.func);
        let lseek_ref = module.declare_func_in_func(lseek_id, builder.func);
        let close_ref = module.declare_func_in_func(close_id, builder.func);
        let access_ref = module.declare_func_in_func(access_id, builder.func);
        let system_ref = module.declare_func_in_func(system_id, builder.func);
        let fopen_ref = module.declare_func_in_func(fopen_id, builder.func);
        let fwrite_ref = module.declare_func_in_func(fwrite_id, builder.func);
        let fclose_ref = module.declare_func_in_func(fclose_id, builder.func);
        let unlink_ref = module.declare_func_in_func(unlink_id, builder.func);
        let rename_ref = module.declare_func_in_func(rename_id, builder.func);
        let mkdir_ref = module.declare_func_in_func(mkdir_id, builder.func);
        let rmdir_ref = module.declare_func_in_func(rmdir_id, builder.func);
        let opendir_ref = module.declare_func_in_func(opendir_id, builder.func);
        let closedir_ref = module.declare_func_in_func(closedir_id, builder.func);
        let realpath_ref = module.declare_func_in_func(realpath_id, builder.func);
        let strncmp_ref = module.declare_func_in_func(strncmp_id, builder.func);
        #[cfg(not(windows))]
        let getaddrinfo_ref = module.declare_func_in_func(getaddrinfo_id, builder.func);
        #[cfg(not(windows))]
        let freeaddrinfo_ref = module.declare_func_in_func(freeaddrinfo_id, builder.func);
        let runtime_refs = I64RuntimeRefs {
            write: write_ref,
            read: read_ref,
            sleep: sleep_ref,
            timespec_get: timespec_get_ref,
            getenv: getenv_ref,
            strlen: strlen_ref,
            atoll: atoll_ref,
            open: open_ref,
            creat: creat_ref,
            lseek: lseek_ref,
            close: close_ref,
            access: access_ref,
            system: system_ref,
            fopen: fopen_ref,
            fwrite: fwrite_ref,
            fclose: fclose_ref,
            unlink: unlink_ref,
            rename: rename_ref,
            mkdir: mkdir_ref,
            rmdir: rmdir_ref,
            opendir: opendir_ref,
            closedir: closedir_ref,
            realpath: realpath_ref,
            strncmp: strncmp_ref,
            getaddrinfo: {
                #[cfg(not(windows))]
                {
                    Some(getaddrinfo_ref)
                }
                #[cfg(windows)]
                {
                    None
                }
            },
            freeaddrinfo: {
                #[cfg(not(windows))]
                {
                    Some(freeaddrinfo_ref)
                }
                #[cfg(windows)]
                {
                    None
                }
            },
        };
        let mut locals = Vec::new();
        for local_expr in &program.locals {
            let local = builder.declare_var(types::I64);
            let value = emit_i64_expr(
                &mut builder,
                &locals,
                &function_refs,
                runtime_refs,
                local_expr,
            )?;
            builder.def_var(local, value);
            locals.push(local);
        }
        emit_i64_stmts(
            &mut module,
            &mut builder,
            &locals,
            &function_refs,
            runtime_refs,
            write_ref,
            &output_data_ids,
            &program.stmts,
        )?;
        emit_i64_exit_body(
            &mut module,
            &mut builder,
            &locals,
            &function_refs,
            runtime_refs,
            write_ref,
            &output_data_ids,
            &program.body,
        )?;
        builder.finalize();
    }
    module
        .define_function(main_id, &mut context)
        .map_err(|message| CraneliftBackendError::new(format!("define main: {message}")))?;
    module.clear_context(&mut context);
    let product = module.finish();
    let bytes = product.emit().map_err(|message| {
        CraneliftBackendError::new(format!("emit cranelift object: {message}"))
    })?;
    write_output_file(object_path, bytes)
}

fn declare_i64_output_data(
    module: &mut ObjectModule,
    program: &I64ExitProgram,
) -> Result<Vec<I64OutputData>, CraneliftBackendError> {
    let mut lines = Vec::new();
    collect_i64_output_lines(&program.stmts, &mut lines);
    collect_i64_exit_body_output_lines(&program.body, &mut lines);
    for function in &program.functions {
        collect_i64_output_lines(&function.stmts, &mut lines);
        collect_i64_value_body_output_lines(&function.body, &mut lines);
    }
    lines
        .into_iter()
        .enumerate()
        .map(|(index, (stream, text, append_newline))| {
            let data_id = module
                .declare_data(
                    &format!("__axiom_i64_line_{index}"),
                    Linkage::Local,
                    false,
                    false,
                )
                .map_err(|message| {
                    CraneliftBackendError::new(format!("declare i64 output data: {message}"))
                })?;
            let mut description = DataDescription::new();
            let mut bytes = text.as_bytes().to_vec();
            if append_newline {
                bytes.push(b'\n');
            }
            let byte_len = bytes.len();
            description.define(bytes.into_boxed_slice());
            module
                .define_data(data_id, &description)
                .map_err(|message| {
                    CraneliftBackendError::new(format!("define i64 output data: {message}"))
                })?;
            Ok(I64OutputData {
                stream,
                text,
                append_newline,
                data_id,
                byte_len,
            })
        })
        .collect()
}

fn collect_i64_output_lines(stmts: &[I64Stmt], lines: &mut Vec<(OutputStream, String, bool)>) {
    for stmt in stmts {
        match stmt {
            I64Stmt::WriteText { stream, text } => lines.push((*stream, text.clone(), false)),
            I64Stmt::WriteLine { stream, text } => lines.push((*stream, text.clone(), true)),
            I64Stmt::WriteI64Text { .. } | I64Stmt::WriteI64Line { .. } => {}
            I64Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                collect_i64_output_lines(then_body, lines);
                collect_i64_output_lines(else_body, lines);
            }
            I64Stmt::While { body, .. } => collect_i64_output_lines(body, lines),
            I64Stmt::Assign(_) | I64Stmt::CallAssign { .. } => {}
        }
    }
}

fn collect_i64_exit_body_output_lines(
    body: &I64ExitBody,
    lines: &mut Vec<(OutputStream, String, bool)>,
) {
    match body {
        I64ExitBody::BlockReturn(block) => collect_i64_output_lines(&block.stmts, lines),
        I64ExitBody::IfBlockReturn {
            then_block,
            else_block,
            ..
        } => {
            collect_i64_output_lines(&then_block.stmts, lines);
            collect_i64_output_lines(&else_block.stmts, lines);
        }
        I64ExitBody::Return(_) | I64ExitBody::IfReturn { .. } => {}
    }
}

fn collect_i64_value_body_output_lines(
    body: &I64ValueBody,
    lines: &mut Vec<(OutputStream, String, bool)>,
) {
    match body {
        I64ValueBody::BlockReturn(block) => collect_i64_output_lines(&block.stmts, lines),
        I64ValueBody::IfBlockReturn {
            then_block,
            else_block,
            ..
        } => {
            collect_i64_output_lines(&then_block.stmts, lines);
            collect_i64_output_lines(&else_block.stmts, lines);
        }
        I64ValueBody::Return(_) | I64ValueBody::IfReturn { .. } => {}
    }
}

fn declare_i64_functions(
    module: &mut ObjectModule,
    functions: &[I64Function],
) -> Result<Vec<FuncId>, CraneliftBackendError> {
    functions
        .iter()
        .enumerate()
        .map(|(index, function)| {
            let mut signature = module.make_signature();
            for _ in 0..function.params {
                signature.params.push(AbiParam::new(types::I64));
            }
            for _ in 0..function.returns {
                signature.returns.push(AbiParam::new(types::I64));
            }
            module
                .declare_function(
                    &format!("__axiom_i64_fn_{index}"),
                    Linkage::Local,
                    &signature,
                )
                .map_err(|message| {
                    CraneliftBackendError::new(format!("declare i64 helper function: {message}"))
                })
        })
        .collect()
}

fn define_i64_function(
    module: &mut ObjectModule,
    function_ids: &[FuncId],
    write_id: FuncId,
    read_id: FuncId,
    sleep_id: FuncId,
    timespec_get_id: FuncId,
    getenv_id: FuncId,
    strlen_id: FuncId,
    atoll_id: FuncId,
    open_id: FuncId,
    creat_id: FuncId,
    lseek_id: FuncId,
    close_id: FuncId,
    access_id: FuncId,
    system_id: FuncId,
    fopen_id: FuncId,
    fwrite_id: FuncId,
    fclose_id: FuncId,
    unlink_id: FuncId,
    rename_id: FuncId,
    mkdir_id: FuncId,
    rmdir_id: FuncId,
    opendir_id: FuncId,
    closedir_id: FuncId,
    realpath_id: FuncId,
    strncmp_id: FuncId,
    #[cfg(not(windows))] getaddrinfo_id: FuncId,
    #[cfg(not(windows))] freeaddrinfo_id: FuncId,
    output_data_ids: &[I64OutputData],
    index: usize,
    function: &I64Function,
) -> Result<(), CraneliftBackendError> {
    let mut context = module.make_context();
    for _ in 0..function.params {
        context
            .func
            .signature
            .params
            .push(AbiParam::new(types::I64));
    }
    for _ in 0..function.returns {
        context
            .func
            .signature
            .returns
            .push(AbiParam::new(types::I64));
    }
    let function_id = *function_ids.get(index).ok_or_else(|| {
        CraneliftBackendError::new(format!("i64 helper function index {index} is out of range"))
    })?;
    let mut builder_context = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
        let block = builder.create_block();
        builder.append_block_params_for_function_params(block);
        builder.switch_to_block(block);
        builder.seal_block(block);
        let function_refs = i64_function_refs(module, &mut builder, function_ids);
        let write_ref = module.declare_func_in_func(write_id, builder.func);
        let read_ref = module.declare_func_in_func(read_id, builder.func);
        let sleep_ref = module.declare_func_in_func(sleep_id, builder.func);
        let timespec_get_ref = module.declare_func_in_func(timespec_get_id, builder.func);
        let getenv_ref = module.declare_func_in_func(getenv_id, builder.func);
        let strlen_ref = module.declare_func_in_func(strlen_id, builder.func);
        let atoll_ref = module.declare_func_in_func(atoll_id, builder.func);
        let open_ref = module.declare_func_in_func(open_id, builder.func);
        let creat_ref = module.declare_func_in_func(creat_id, builder.func);
        let lseek_ref = module.declare_func_in_func(lseek_id, builder.func);
        let close_ref = module.declare_func_in_func(close_id, builder.func);
        let access_ref = module.declare_func_in_func(access_id, builder.func);
        let system_ref = module.declare_func_in_func(system_id, builder.func);
        let fopen_ref = module.declare_func_in_func(fopen_id, builder.func);
        let fwrite_ref = module.declare_func_in_func(fwrite_id, builder.func);
        let fclose_ref = module.declare_func_in_func(fclose_id, builder.func);
        let unlink_ref = module.declare_func_in_func(unlink_id, builder.func);
        let rename_ref = module.declare_func_in_func(rename_id, builder.func);
        let mkdir_ref = module.declare_func_in_func(mkdir_id, builder.func);
        let rmdir_ref = module.declare_func_in_func(rmdir_id, builder.func);
        let opendir_ref = module.declare_func_in_func(opendir_id, builder.func);
        let closedir_ref = module.declare_func_in_func(closedir_id, builder.func);
        let realpath_ref = module.declare_func_in_func(realpath_id, builder.func);
        let strncmp_ref = module.declare_func_in_func(strncmp_id, builder.func);
        #[cfg(not(windows))]
        let getaddrinfo_ref = module.declare_func_in_func(getaddrinfo_id, builder.func);
        #[cfg(not(windows))]
        let freeaddrinfo_ref = module.declare_func_in_func(freeaddrinfo_id, builder.func);
        let runtime_refs = I64RuntimeRefs {
            write: write_ref,
            read: read_ref,
            sleep: sleep_ref,
            timespec_get: timespec_get_ref,
            getenv: getenv_ref,
            strlen: strlen_ref,
            atoll: atoll_ref,
            open: open_ref,
            creat: creat_ref,
            lseek: lseek_ref,
            close: close_ref,
            access: access_ref,
            system: system_ref,
            fopen: fopen_ref,
            fwrite: fwrite_ref,
            fclose: fclose_ref,
            unlink: unlink_ref,
            rename: rename_ref,
            mkdir: mkdir_ref,
            rmdir: rmdir_ref,
            opendir: opendir_ref,
            closedir: closedir_ref,
            realpath: realpath_ref,
            strncmp: strncmp_ref,
            getaddrinfo: {
                #[cfg(not(windows))]
                {
                    Some(getaddrinfo_ref)
                }
                #[cfg(windows)]
                {
                    None
                }
            },
            freeaddrinfo: {
                #[cfg(not(windows))]
                {
                    Some(freeaddrinfo_ref)
                }
                #[cfg(windows)]
                {
                    None
                }
            },
        };
        let mut locals = Vec::new();
        for param in builder.block_params(block).to_vec() {
            let local = builder.declare_var(types::I64);
            builder.def_var(local, param);
            locals.push(local);
        }
        for local_expr in &function.locals {
            let local = builder.declare_var(types::I64);
            let value = emit_i64_expr(
                &mut builder,
                &locals,
                &function_refs,
                runtime_refs,
                local_expr,
            )?;
            builder.def_var(local, value);
            locals.push(local);
        }
        emit_i64_stmts(
            module,
            &mut builder,
            &locals,
            &function_refs,
            runtime_refs,
            write_ref,
            output_data_ids,
            &function.stmts,
        )?;
        emit_i64_value_body(
            module,
            &mut builder,
            &locals,
            &function_refs,
            runtime_refs,
            write_ref,
            output_data_ids,
            function.returns,
            &function.body,
        )?;
        builder.finalize();
    }
    module
        .define_function(function_id, &mut context)
        .map_err(|message| CraneliftBackendError::new(format!("define i64 helper: {message}")))?;
    module.clear_context(&mut context);
    Ok(())
}

fn i64_function_refs(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    function_ids: &[FuncId],
) -> Vec<FuncRef> {
    function_ids
        .iter()
        .map(|id| module.declare_func_in_func(*id, builder.func))
        .collect()
}

fn emit_i64_stmts(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    write_ref: FuncRef,
    output_data_ids: &[I64OutputData],
    stmts: &[I64Stmt],
) -> Result<(), CraneliftBackendError> {
    for stmt in stmts {
        emit_i64_stmt(
            module,
            builder,
            locals,
            function_refs,
            runtime_refs,
            write_ref,
            output_data_ids,
            stmt,
        )?;
    }
    Ok(())
}

fn emit_i64_stmt(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    write_ref: FuncRef,
    output_data_ids: &[I64OutputData],
    stmt: &I64Stmt,
) -> Result<(), CraneliftBackendError> {
    match stmt {
        I64Stmt::Assign(assign) => {
            emit_i64_assign(builder, locals, function_refs, runtime_refs, assign)
        }
        I64Stmt::WriteText { stream, text } => emit_i64_write_text(
            module,
            builder,
            runtime_refs,
            write_ref,
            output_data_ids,
            *stream,
            text,
        ),
        I64Stmt::WriteLine { stream, text } => emit_i64_write_line(
            module,
            builder,
            runtime_refs,
            write_ref,
            output_data_ids,
            *stream,
            text,
        ),
        I64Stmt::WriteI64Text { stream, value } => emit_i64_write_i64_text(
            builder,
            locals,
            function_refs,
            runtime_refs,
            write_ref,
            module.target_config().pointer_type(),
            *stream,
            value,
        ),
        I64Stmt::WriteI64Line { stream, value } => emit_i64_write_i64_line(
            builder,
            locals,
            function_refs,
            runtime_refs,
            write_ref,
            module.target_config().pointer_type(),
            *stream,
            value,
        ),
        I64Stmt::CallAssign {
            locals: assign_locals,
            function,
            args,
        } => emit_i64_call_assign(
            builder,
            locals,
            function_refs,
            runtime_refs,
            assign_locals,
            *function,
            args,
        ),
        I64Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            let then_block = builder.create_block();
            let else_block = builder.create_block();
            let after_if = builder.create_block();
            let condition = emit_i64_condition(builder, locals, function_refs, runtime_refs, cond)?;
            builder
                .ins()
                .brif(condition, then_block, &[], else_block, &[]);

            builder.switch_to_block(then_block);
            builder.seal_block(then_block);
            emit_i64_stmts(
                module,
                builder,
                locals,
                function_refs,
                runtime_refs,
                write_ref,
                output_data_ids,
                then_body,
            )?;
            builder.ins().jump(after_if, &[]);

            builder.switch_to_block(else_block);
            builder.seal_block(else_block);
            emit_i64_stmts(
                module,
                builder,
                locals,
                function_refs,
                runtime_refs,
                write_ref,
                output_data_ids,
                else_body,
            )?;
            builder.ins().jump(after_if, &[]);

            builder.switch_to_block(after_if);
            builder.seal_block(after_if);
            Ok(())
        }
        I64Stmt::While { cond, body } => {
            let loop_header = builder.create_block();
            let loop_body = builder.create_block();
            let after_loop = builder.create_block();

            builder.ins().jump(loop_header, &[]);
            builder.switch_to_block(loop_header);
            let condition = emit_i64_condition(builder, locals, function_refs, runtime_refs, cond)?;
            builder
                .ins()
                .brif(condition, loop_body, &[], after_loop, &[]);

            builder.switch_to_block(loop_body);
            builder.seal_block(loop_body);
            emit_i64_stmts(
                module,
                builder,
                locals,
                function_refs,
                runtime_refs,
                write_ref,
                output_data_ids,
                body,
            )?;
            builder.ins().jump(loop_header, &[]);
            builder.seal_block(loop_header);

            builder.switch_to_block(after_loop);
            builder.seal_block(after_loop);
            Ok(())
        }
    }
}

fn emit_i64_write_line(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    runtime_refs: I64RuntimeRefs,
    write_ref: FuncRef,
    output_data_ids: &[I64OutputData],
    stream: OutputStream,
    text: &str,
) -> Result<(), CraneliftBackendError> {
    emit_i64_write_static(
        module,
        builder,
        runtime_refs,
        write_ref,
        output_data_ids,
        stream,
        text,
        true,
    )
}

fn emit_i64_write_text(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    runtime_refs: I64RuntimeRefs,
    write_ref: FuncRef,
    output_data_ids: &[I64OutputData],
    stream: OutputStream,
    text: &str,
) -> Result<(), CraneliftBackendError> {
    emit_i64_write_static(
        module,
        builder,
        runtime_refs,
        write_ref,
        output_data_ids,
        stream,
        text,
        false,
    )
}

fn emit_i64_write_static(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    runtime_refs: I64RuntimeRefs,
    write_ref: FuncRef,
    output_data_ids: &[I64OutputData],
    stream: OutputStream,
    text: &str,
    append_newline: bool,
) -> Result<(), CraneliftBackendError> {
    let output_data = output_data_ids
        .iter()
        .find(|candidate| {
            candidate.stream == stream
                && candidate.text == text
                && candidate.append_newline == append_newline
        })
        .ok_or_else(|| CraneliftBackendError::new("missing i64 output data"))?;
    let data_ref = module.declare_data_in_func(output_data.data_id, builder.func);
    let pointer_type = module.target_config().pointer_type();
    let pointer = builder.ins().global_value(pointer_type, data_ref);
    let fd = builder.ins().iconst(types::I32, output_stream_fd(stream));
    let len = builder
        .ins()
        .iconst(pointer_type, output_data.byte_len as i64);
    builder.ins().call(write_ref, &[fd, pointer, len]);
    let line = i64_stdio_audit_line(stream, Some(output_data.byte_len), "ok");
    emit_i64_host_audit_line(builder, runtime_refs, &line)?;
    Ok(())
}

fn emit_i64_write_i64_line(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    write_ref: FuncRef,
    pointer_type: cranelift_codegen::ir::Type,
    stream: OutputStream,
    value: &I64Expr,
) -> Result<(), CraneliftBackendError> {
    emit_i64_write_i64(
        builder,
        locals,
        function_refs,
        runtime_refs,
        write_ref,
        pointer_type,
        stream,
        value,
        true,
    )
}

fn emit_i64_write_i64_text(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    write_ref: FuncRef,
    pointer_type: cranelift_codegen::ir::Type,
    stream: OutputStream,
    value: &I64Expr,
) -> Result<(), CraneliftBackendError> {
    emit_i64_write_i64(
        builder,
        locals,
        function_refs,
        runtime_refs,
        write_ref,
        pointer_type,
        stream,
        value,
        false,
    )
}

fn emit_i64_write_i64(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    write_ref: FuncRef,
    pointer_type: cranelift_codegen::ir::Type,
    stream: OutputStream,
    value: &I64Expr,
    append_newline: bool,
) -> Result<(), CraneliftBackendError> {
    let value = emit_i64_expr(builder, locals, function_refs, runtime_refs, value)?;
    let buffer =
        builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 32, 0));
    let base = builder.ins().stack_addr(pointer_type, buffer, 0);
    if append_newline {
        let newline = builder.ins().iconst(types::I8, i64::from(b'\n'));
        builder.ins().stack_store(newline, buffer, 31);
    }

    let zero_block = builder.create_block();
    let digits_block = builder.create_block();
    let digit_loop = builder.create_block();
    let digits_done = builder.create_block();
    let sign_block = builder.create_block();
    let no_sign_block = builder.create_block();
    let write_block = builder.create_block();
    let after_write = builder.create_block();

    builder.append_block_param(digit_loop, types::I64);
    builder.append_block_param(digit_loop, pointer_type);
    builder.append_block_param(digits_done, pointer_type);
    builder.append_block_param(write_block, pointer_type);

    let is_zero = builder.ins().icmp_imm(IntCC::Equal, value, 0);
    builder
        .ins()
        .brif(is_zero, zero_block, &[], digits_block, &[]);

    builder.switch_to_block(zero_block);
    builder.seal_block(zero_block);
    let zero_digit = builder.ins().iconst(types::I8, i64::from(b'0'));
    builder.ins().stack_store(zero_digit, buffer, 30);
    let zero_start = builder.ins().iconst(pointer_type, 30);
    let zero_args = [BlockArg::Value(zero_start)];
    builder.ins().jump(write_block, &zero_args);

    builder.switch_to_block(digits_block);
    builder.seal_block(digits_block);
    let is_negative = builder.ins().icmp_imm(IntCC::SignedLessThan, value, 0);
    let initial_pos = builder.ins().iconst(pointer_type, 31);
    let loop_args = [BlockArg::Value(value), BlockArg::Value(initial_pos)];
    builder.ins().jump(digit_loop, &loop_args);

    builder.switch_to_block(digit_loop);
    let current_value = builder.block_params(digit_loop)[0];
    let current_pos = builder.block_params(digit_loop)[1];
    let ten = builder.ins().iconst(types::I64, 10);
    let quotient = builder.ins().sdiv(current_value, ten);
    let remainder = builder.ins().srem(current_value, ten);
    let ascii_zero = builder.ins().iconst(types::I64, i64::from(b'0'));
    let positive_digit = builder.ins().iadd(ascii_zero, remainder);
    let negative_digit = builder.ins().isub(ascii_zero, remainder);
    let digit = builder
        .ins()
        .select(is_negative, negative_digit, positive_digit);
    let digit = builder.ins().ireduce(types::I8, digit);
    let one = builder.ins().iconst(pointer_type, 1);
    let next_pos = builder.ins().isub(current_pos, one);
    let digit_addr = builder.ins().iadd(base, next_pos);
    builder.ins().store(MemFlags::new(), digit, digit_addr, 0);
    let keep_going = builder.ins().icmp_imm(IntCC::NotEqual, quotient, 0);
    let continue_args = [BlockArg::Value(quotient), BlockArg::Value(next_pos)];
    let done_args = [BlockArg::Value(next_pos)];
    builder.ins().brif(
        keep_going,
        digit_loop,
        &continue_args,
        digits_done,
        &done_args,
    );
    builder.seal_block(digit_loop);

    builder.switch_to_block(digits_done);
    let start_pos = builder.block_params(digits_done)[0];
    builder
        .ins()
        .brif(is_negative, sign_block, &[], no_sign_block, &[]);
    builder.seal_block(digits_done);

    builder.switch_to_block(sign_block);
    builder.seal_block(sign_block);
    let sign_pos = builder.ins().isub(start_pos, one);
    let minus = builder.ins().iconst(types::I8, i64::from(b'-'));
    let sign_addr = builder.ins().iadd(base, sign_pos);
    builder.ins().store(MemFlags::new(), minus, sign_addr, 0);
    let sign_args = [BlockArg::Value(sign_pos)];
    builder.ins().jump(write_block, &sign_args);

    builder.switch_to_block(no_sign_block);
    builder.seal_block(no_sign_block);
    let no_sign_args = [BlockArg::Value(start_pos)];
    builder.ins().jump(write_block, &no_sign_args);

    builder.switch_to_block(write_block);
    let final_start = builder.block_params(write_block)[0];
    let start_ptr = builder.ins().iadd(base, final_start);
    let buffer_len = builder
        .ins()
        .iconst(pointer_type, if append_newline { 32 } else { 31 });
    let len = builder.ins().isub(buffer_len, final_start);
    let fd = builder.ins().iconst(types::I32, output_stream_fd(stream));
    builder.ins().call(write_ref, &[fd, start_ptr, len]);
    let line = i64_stdio_audit_line(stream, None, "ok");
    emit_i64_host_audit_line(builder, runtime_refs, &line)?;
    builder.ins().jump(after_write, &[]);
    builder.seal_block(write_block);

    builder.switch_to_block(after_write);
    builder.seal_block(after_write);
    Ok(())
}

fn output_stream_fd(stream: OutputStream) -> i64 {
    match stream {
        OutputStream::Stdout => 1,
        OutputStream::Stderr => 2,
    }
}

fn output_stream_name(stream: OutputStream) -> &'static str {
    match stream {
        OutputStream::Stdout => "stdout",
        OutputStream::Stderr => "stderr",
    }
}

fn emit_i64_assign(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    assign: &I64Assign,
) -> Result<(), CraneliftBackendError> {
    let local = locals.get(assign.local).copied().ok_or_else(|| {
        CraneliftBackendError::new(format!(
            "i64 assignment local {} is out of range",
            assign.local
        ))
    })?;
    let value = emit_i64_expr(builder, locals, function_refs, runtime_refs, &assign.value)?;
    builder.def_var(local, value);
    Ok(())
}

fn emit_i64_call_assign(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    assign_locals: &[usize],
    function: usize,
    args: &[I64Expr],
) -> Result<(), CraneliftBackendError> {
    let function_ref = function_refs.get(function).copied().ok_or_else(|| {
        CraneliftBackendError::new(format!("i64 function index {function} is out of range"))
    })?;
    let args = args
        .iter()
        .map(|arg| emit_i64_expr(builder, locals, function_refs, runtime_refs, arg))
        .collect::<Result<Vec<_>, _>>()?;
    let call = builder.ins().call(function_ref, &args);
    let results = builder.inst_results(call).to_vec();
    if results.len() != assign_locals.len() {
        return Err(CraneliftBackendError::new(format!(
            "i64 helper call returned {} values for {} assignment targets",
            results.len(),
            assign_locals.len()
        )));
    }
    for (local_index, result) in assign_locals.iter().zip(results) {
        let local = locals.get(*local_index).copied().ok_or_else(|| {
            CraneliftBackendError::new(format!(
                "i64 call assignment local {local_index} is out of range"
            ))
        })?;
        builder.def_var(local, result);
    }
    Ok(())
}

fn emit_i64_exit_body(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    write_ref: FuncRef,
    output_data_ids: &[I64OutputData],
    body: &I64ExitBody,
) -> Result<(), CraneliftBackendError> {
    match body {
        I64ExitBody::Return(result) => {
            emit_i64_return(builder, locals, function_refs, runtime_refs, result)
        }
        I64ExitBody::BlockReturn(block) => {
            emit_i64_stmts(
                module,
                builder,
                locals,
                function_refs,
                runtime_refs,
                write_ref,
                output_data_ids,
                &block.stmts,
            )?;
            emit_i64_return(builder, locals, function_refs, runtime_refs, &block.result)
        }
        I64ExitBody::IfReturn {
            cond,
            then_result,
            else_result,
        } => {
            let then_block = builder.create_block();
            let else_block = builder.create_block();
            let condition = emit_i64_condition(builder, locals, function_refs, runtime_refs, cond)?;
            builder
                .ins()
                .brif(condition, then_block, &[], else_block, &[]);

            builder.switch_to_block(then_block);
            builder.seal_block(then_block);
            emit_i64_return(builder, locals, function_refs, runtime_refs, then_result)?;

            builder.switch_to_block(else_block);
            builder.seal_block(else_block);
            emit_i64_return(builder, locals, function_refs, runtime_refs, else_result)
        }
        I64ExitBody::IfBlockReturn {
            cond,
            then_block,
            else_block,
        } => {
            let then_cranelift_block = builder.create_block();
            let else_cranelift_block = builder.create_block();
            let condition = emit_i64_condition(builder, locals, function_refs, runtime_refs, cond)?;
            builder.ins().brif(
                condition,
                then_cranelift_block,
                &[],
                else_cranelift_block,
                &[],
            );

            builder.switch_to_block(then_cranelift_block);
            builder.seal_block(then_cranelift_block);
            emit_i64_stmts(
                module,
                builder,
                locals,
                function_refs,
                runtime_refs,
                write_ref,
                output_data_ids,
                &then_block.stmts,
            )?;
            emit_i64_return(
                builder,
                locals,
                function_refs,
                runtime_refs,
                &then_block.result,
            )?;

            builder.switch_to_block(else_cranelift_block);
            builder.seal_block(else_cranelift_block);
            emit_i64_stmts(
                module,
                builder,
                locals,
                function_refs,
                runtime_refs,
                write_ref,
                output_data_ids,
                &else_block.stmts,
            )?;
            emit_i64_return(
                builder,
                locals,
                function_refs,
                runtime_refs,
                &else_block.result,
            )
        }
    }
}

fn emit_i64_value_body(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    write_ref: FuncRef,
    output_data_ids: &[I64OutputData],
    returns: usize,
    body: &I64ValueBody,
) -> Result<(), CraneliftBackendError> {
    match body {
        I64ValueBody::Return(results) => emit_i64_value_return(
            builder,
            locals,
            function_refs,
            runtime_refs,
            returns,
            results,
        ),
        I64ValueBody::BlockReturn(block) => {
            emit_i64_stmts(
                module,
                builder,
                locals,
                function_refs,
                runtime_refs,
                write_ref,
                output_data_ids,
                &block.stmts,
            )?;
            emit_i64_value_return(
                builder,
                locals,
                function_refs,
                runtime_refs,
                returns,
                &block.results,
            )
        }
        I64ValueBody::IfReturn {
            cond,
            then_results,
            else_results,
        } => {
            let then_block = builder.create_block();
            let else_block = builder.create_block();
            let condition = emit_i64_condition(builder, locals, function_refs, runtime_refs, cond)?;
            builder
                .ins()
                .brif(condition, then_block, &[], else_block, &[]);

            builder.switch_to_block(then_block);
            builder.seal_block(then_block);
            emit_i64_value_return(
                builder,
                locals,
                function_refs,
                runtime_refs,
                returns,
                then_results,
            )?;

            builder.switch_to_block(else_block);
            builder.seal_block(else_block);
            emit_i64_value_return(
                builder,
                locals,
                function_refs,
                runtime_refs,
                returns,
                else_results,
            )
        }
        I64ValueBody::IfBlockReturn {
            cond,
            then_block,
            else_block,
        } => {
            let then_cranelift_block = builder.create_block();
            let else_cranelift_block = builder.create_block();
            let condition = emit_i64_condition(builder, locals, function_refs, runtime_refs, cond)?;
            builder.ins().brif(
                condition,
                then_cranelift_block,
                &[],
                else_cranelift_block,
                &[],
            );

            builder.switch_to_block(then_cranelift_block);
            builder.seal_block(then_cranelift_block);
            emit_i64_stmts(
                module,
                builder,
                locals,
                function_refs,
                runtime_refs,
                write_ref,
                output_data_ids,
                &then_block.stmts,
            )?;
            emit_i64_value_return(
                builder,
                locals,
                function_refs,
                runtime_refs,
                returns,
                &then_block.results,
            )?;

            builder.switch_to_block(else_cranelift_block);
            builder.seal_block(else_cranelift_block);
            emit_i64_stmts(
                module,
                builder,
                locals,
                function_refs,
                runtime_refs,
                write_ref,
                output_data_ids,
                &else_block.stmts,
            )?;
            emit_i64_value_return(
                builder,
                locals,
                function_refs,
                runtime_refs,
                returns,
                &else_block.results,
            )
        }
    }
}

fn emit_i64_return(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    result: &I64Expr,
) -> Result<(), CraneliftBackendError> {
    let result = emit_i64_expr(builder, locals, function_refs, runtime_refs, result)?;
    let exit_code = builder.ins().ireduce(types::I32, result);
    builder.ins().return_(&[exit_code]);
    Ok(())
}

fn emit_i64_value_return(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    returns: usize,
    results: &[I64Expr],
) -> Result<(), CraneliftBackendError> {
    if results.len() != returns {
        return Err(CraneliftBackendError::new(format!(
            "i64 helper body returned {} values for {returns} declared returns",
            results.len()
        )));
    }
    let results = results
        .iter()
        .map(|result| emit_i64_expr(builder, locals, function_refs, runtime_refs, result))
        .collect::<Result<Vec<_>, _>>()?;
    builder.ins().return_(&results);
    Ok(())
}

fn emit_i64_compare(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    cond: &I64Compare,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let lhs = emit_i64_expr(builder, locals, function_refs, runtime_refs, &cond.lhs)?;
    let rhs = emit_i64_expr(builder, locals, function_refs, runtime_refs, &cond.rhs)?;
    Ok(builder.ins().icmp(i64_compare_op(cond.op), lhs, rhs))
}

fn emit_i64_condition(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    cond: &I64Condition,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    match cond {
        I64Condition::Literal(value) => {
            let value = builder.ins().iconst(types::I8, i64::from(*value));
            Ok(builder.ins().icmp_imm(IntCC::NotEqual, value, 0))
        }
        I64Condition::Compare(compare) => {
            emit_i64_compare(builder, locals, function_refs, runtime_refs, compare)
        }
        I64Condition::And { lhs, rhs } => emit_i64_short_circuit_condition(
            builder,
            locals,
            function_refs,
            runtime_refs,
            lhs,
            rhs,
            false,
        ),
        I64Condition::Or { lhs, rhs } => emit_i64_short_circuit_condition(
            builder,
            locals,
            function_refs,
            runtime_refs,
            lhs,
            rhs,
            true,
        ),
    }
}

fn emit_i64_short_circuit_condition(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    lhs: &I64Condition,
    rhs: &I64Condition,
    short_circuit_value: bool,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let lhs = emit_i64_condition(builder, locals, function_refs, runtime_refs, lhs)?;
    let rhs_block = builder.create_block();
    let short_circuit_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);

    let (true_block, false_block) = if short_circuit_value {
        (short_circuit_block, rhs_block)
    } else {
        (rhs_block, short_circuit_block)
    };
    builder.ins().brif(lhs, true_block, &[], false_block, &[]);

    builder.switch_to_block(short_circuit_block);
    builder.seal_block(short_circuit_block);
    let short_circuit_value = builder
        .ins()
        .iconst(types::I64, i64::from(short_circuit_value));
    let short_circuit_args = [BlockArg::Value(short_circuit_value)];
    builder.ins().jump(merge_block, &short_circuit_args);

    builder.switch_to_block(rhs_block);
    builder.seal_block(rhs_block);
    let rhs = emit_i64_condition(builder, locals, function_refs, runtime_refs, rhs)?;
    let rhs = emit_i64_bool_value(builder, rhs);
    let rhs_args = [BlockArg::Value(rhs)];
    builder.ins().jump(merge_block, &rhs_args);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    let merged = builder.block_params(merge_block)[0];
    Ok(builder.ins().icmp_imm(IntCC::NotEqual, merged, 0))
}

fn emit_i64_bool_value(
    builder: &mut FunctionBuilder<'_>,
    cond: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let true_value = builder.ins().iconst(types::I64, 1);
    let false_value = builder.ins().iconst(types::I64, 0);
    builder.ins().select(cond, true_value, false_value)
}

fn i64_compare_op(op: I64CompareOp) -> IntCC {
    match op {
        I64CompareOp::Eq => IntCC::Equal,
        I64CompareOp::Ne => IntCC::NotEqual,
        I64CompareOp::Lt => IntCC::SignedLessThan,
        I64CompareOp::Le => IntCC::SignedLessThanOrEqual,
        I64CompareOp::Gt => IntCC::SignedGreaterThan,
        I64CompareOp::Ge => IntCC::SignedGreaterThanOrEqual,
    }
}

fn emit_i64_expr(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    expr: &I64Expr,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    match expr {
        I64Expr::Literal(value) => Ok(builder.ins().iconst(types::I64, *value)),
        I64Expr::Local(index) => {
            let local = locals.get(*index).copied().ok_or_else(|| {
                CraneliftBackendError::new(format!("i64 local index {index} is out of range"))
            })?;
            Ok(builder.use_var(local))
        }
        I64Expr::ClockNowMs => Ok(emit_i64_clock_now_ms_expr(builder, runtime_refs)),
        I64Expr::ClockElapsedMs { start } => {
            emit_i64_clock_elapsed_ms_expr(builder, locals, function_refs, runtime_refs, start)
        }
        I64Expr::SleepMs { milliseconds } => {
            emit_i64_sleep_ms_expr(builder, locals, function_refs, runtime_refs, milliseconds)
        }
        I64Expr::EnvLen { key } => {
            emit_i64_env_len_expr(builder, runtime_refs.getenv, runtime_refs.strlen, key)
        }
        I64Expr::AuditEnv {
            intrinsic,
            package,
            key_len,
            success,
            result,
        } => emit_i64_audit_env_expr(
            builder,
            locals,
            function_refs,
            runtime_refs,
            intrinsic,
            package,
            *key_len,
            *success,
            result,
        ),
        I64Expr::AuditProcess {
            intrinsic,
            package,
            command_len,
            success,
            result,
        } => emit_i64_audit_process_expr(
            builder,
            locals,
            function_refs,
            runtime_refs,
            intrinsic,
            package,
            *command_len,
            *success,
            result,
        ),
        I64Expr::AuditClock {
            intrinsic,
            package,
            arg_name,
            success,
            result,
        } => emit_i64_audit_clock_expr(
            builder,
            locals,
            function_refs,
            runtime_refs,
            intrinsic,
            package,
            arg_name,
            *success,
            result,
        ),
        I64Expr::CStringLen { value } => emit_i64_c_string_len_expr(builder, runtime_refs, value),
        I64Expr::AuditFfi {
            intrinsic,
            package,
            library,
            symbol,
            arg_type,
            success,
            result,
        } => emit_i64_audit_ffi_expr(
            builder,
            locals,
            function_refs,
            runtime_refs,
            intrinsic,
            package,
            library,
            symbol,
            arg_type,
            *success,
            result,
        ),
        I64Expr::RandomU64 { intrinsic, package } => {
            emit_i64_random_u64_expr(builder, runtime_refs, intrinsic, package)
        }
        I64Expr::RandomBytesLen { length } => {
            emit_i64_random_bytes_len_expr(builder, locals, function_refs, runtime_refs, length)
        }
        I64Expr::AuditCrypto {
            intrinsic,
            package,
            arg_name,
            arg_value,
            success,
            result,
        } => emit_i64_audit_crypto_expr(
            builder,
            locals,
            function_refs,
            runtime_refs,
            intrinsic,
            package,
            arg_name,
            arg_value,
            *success,
            result,
        ),
        I64Expr::FileLen { path, max_bytes } => {
            emit_i64_file_len_expr(builder, runtime_refs, path, *max_bytes)
        }
        I64Expr::AuditFs {
            intrinsic,
            package,
            path_len,
            content_len,
            success,
            result,
        } => emit_i64_audit_fs_expr(
            builder,
            locals,
            function_refs,
            runtime_refs,
            intrinsic,
            package,
            *path_len,
            *content_len,
            *success,
            result,
        ),
        I64Expr::RuntimeFsGuard {
            root,
            path,
            fallback_path,
            result,
        } => emit_i64_runtime_fs_guard_expr(
            builder,
            locals,
            function_refs,
            runtime_refs,
            root,
            path,
            fallback_path,
            result,
        ),
        I64Expr::WriteFile { path, content } => {
            emit_i64_write_file_expr(builder, runtime_refs, path, content)
        }
        I64Expr::AppendFile { path, content } => {
            emit_i64_append_file_expr(builder, runtime_refs, path, content)
        }
        I64Expr::ReplaceFile {
            path,
            temp_path,
            content,
        } => emit_i64_replace_file_expr(builder, runtime_refs, path, temp_path, content),
        I64Expr::CreateFile { path } => emit_i64_create_file_expr(builder, runtime_refs, path),
        I64Expr::RemoveFile { path } => emit_i64_remove_file_expr(builder, runtime_refs, path),
        I64Expr::MakeDir { path } => emit_i64_make_dir_expr(builder, runtime_refs, path),
        I64Expr::MakeDirAll { path } => emit_i64_make_dir_all_expr(builder, runtime_refs, path),
        I64Expr::RemoveDir { path } => emit_i64_remove_dir_expr(builder, runtime_refs, path),
        I64Expr::ProcessStatus { command } => {
            emit_i64_process_status_expr(builder, runtime_refs, command)
        }
        I64Expr::NetResolveLen { host, resolved_len } => {
            emit_i64_net_resolve_len_expr(builder, runtime_refs, host, *resolved_len)
        }
        I64Expr::AuditNet {
            intrinsic,
            package,
            host_len,
            success,
            result,
        } => emit_i64_audit_net_expr(
            builder,
            locals,
            function_refs,
            runtime_refs,
            intrinsic,
            package,
            *host_len,
            *success,
            result,
        ),
        I64Expr::ConditionValue(cond) => {
            let cond = emit_i64_condition(builder, locals, function_refs, runtime_refs, cond)?;
            Ok(emit_i64_bool_value(builder, cond))
        }
        I64Expr::Cast { cast, expr } => {
            let value = emit_i64_expr(builder, locals, function_refs, runtime_refs, expr)?;
            Ok(emit_i64_cast(builder, value, *cast))
        }
        I64Expr::Call { function, args } => {
            let function_ref = function_refs.get(*function).copied().ok_or_else(|| {
                CraneliftBackendError::new(format!("i64 function index {function} is out of range"))
            })?;
            let args = args
                .iter()
                .map(|arg| emit_i64_expr(builder, locals, function_refs, runtime_refs, arg))
                .collect::<Result<Vec<_>, _>>()?;
            let call = builder.ins().call(function_ref, &args);
            let results = builder.inst_results(call);
            Ok(results[0])
        }
        I64Expr::Binary { op, lhs, rhs } => {
            let lhs = emit_i64_expr(builder, locals, function_refs, runtime_refs, lhs)?;
            let rhs = emit_i64_expr(builder, locals, function_refs, runtime_refs, rhs)?;
            Ok(match op {
                I64BinaryOp::Add => builder.ins().iadd(lhs, rhs),
                I64BinaryOp::Sub => builder.ins().isub(lhs, rhs),
                I64BinaryOp::Mul => builder.ins().imul(lhs, rhs),
                I64BinaryOp::Div => builder.ins().sdiv(lhs, rhs),
            })
        }
        I64Expr::Select {
            cond,
            then_result,
            else_result,
        } => emit_i64_select_expr(
            builder,
            locals,
            function_refs,
            runtime_refs,
            cond,
            then_result,
            else_result,
        ),
    }
}

fn emit_i64_select_expr(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    cond: &I64Condition,
    then_result: &I64Expr,
    else_result: &I64Expr,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let cond = emit_i64_condition(builder, locals, function_refs, runtime_refs, cond)?;
    let then_block = builder.create_block();
    let else_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);
    builder.ins().brif(cond, then_block, &[], else_block, &[]);

    builder.switch_to_block(then_block);
    builder.seal_block(then_block);
    let then_result = emit_i64_expr(builder, locals, function_refs, runtime_refs, then_result)?;
    let then_args = [BlockArg::Value(then_result)];
    builder.ins().jump(merge_block, &then_args);

    builder.switch_to_block(else_block);
    builder.seal_block(else_block);
    let else_result = emit_i64_expr(builder, locals, function_refs, runtime_refs, else_result)?;
    let else_args = [BlockArg::Value(else_result)];
    builder.ins().jump(merge_block, &else_args);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

fn emit_i64_env_len_expr(
    builder: &mut FunctionBuilder<'_>,
    getenv_ref: FuncRef,
    strlen_ref: FuncRef,
    key: &str,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let key_len = u32::try_from(key.len() + 1)
        .map_err(|_| CraneliftBackendError::new("environment key is too large"))?;
    let key_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        key_len,
        0,
    ));
    for (offset, byte) in key.bytes().chain(std::iter::once(0)).enumerate() {
        let byte_value = builder.ins().iconst(types::I8, i64::from(byte));
        builder
            .ins()
            .stack_store(byte_value, key_slot, offset as i32);
    }
    let key_ptr = builder.ins().stack_addr(types::I64, key_slot, 0);
    let call = builder.ins().call(getenv_ref, &[key_ptr]);
    let value_ptr = builder.inst_results(call)[0];

    let missing_block = builder.create_block();
    let present_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);

    let missing = builder.ins().icmp_imm(IntCC::Equal, value_ptr, 0);
    builder
        .ins()
        .brif(missing, missing_block, &[], present_block, &[]);

    builder.switch_to_block(missing_block);
    builder.seal_block(missing_block);
    let missing_result = builder.ins().iconst(types::I64, -1);
    builder
        .ins()
        .jump(merge_block, &[BlockArg::Value(missing_result)]);

    builder.switch_to_block(present_block);
    builder.seal_block(present_block);
    let call = builder.ins().call(strlen_ref, &[value_ptr]);
    let length = builder.inst_results(call)[0];
    builder.ins().jump(merge_block, &[BlockArg::Value(length)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

fn emit_i64_c_string_len_expr(
    builder: &mut FunctionBuilder<'_>,
    runtime_refs: I64RuntimeRefs,
    value: &str,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let value_len = u32::try_from(value.len() + 1)
        .map_err(|_| CraneliftBackendError::new("ffi string argument is too large"))?;
    let value_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        value_len,
        0,
    ));
    for (offset, byte) in value.bytes().chain(std::iter::once(0)).enumerate() {
        let byte_value = builder.ins().iconst(types::I8, i64::from(byte));
        builder
            .ins()
            .stack_store(byte_value, value_slot, offset as i32);
    }
    let value_ptr = builder.ins().stack_addr(types::I64, value_slot, 0);
    let call = builder.ins().call(runtime_refs.strlen, &[value_ptr]);
    Ok(builder.inst_results(call)[0])
}

fn emit_i64_random_u64_expr(
    builder: &mut FunctionBuilder<'_>,
    runtime_refs: I64RuntimeRefs,
    intrinsic: &str,
    package: &str,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let ok_line = i64_crypto_no_args_audit_line(intrinsic, package, "ok");
    let denied_line = i64_crypto_no_args_audit_line(intrinsic, package, "denied");
    let hook_key_ptr = emit_i64_path_ptr(builder, "AXIOM_TEST_RANDOM_U64")?;
    let hook_call = builder.ins().call(runtime_refs.getenv, &[hook_key_ptr]);
    let hook_ptr = builder.inst_results(hook_call)[0];

    let hook_len_block = builder.create_block();
    let hook_value_block = builder.create_block();
    let os_open_block = builder.create_block();
    let denied_block = builder.create_block();
    let read_block = builder.create_block();
    let success_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(read_block, types::I32);
    builder.append_block_param(success_block, types::I64);
    builder.append_block_param(merge_block, types::I64);

    let missing_hook = builder.ins().icmp_imm(IntCC::Equal, hook_ptr, 0);
    builder
        .ins()
        .brif(missing_hook, os_open_block, &[], hook_len_block, &[]);

    builder.switch_to_block(hook_len_block);
    builder.seal_block(hook_len_block);
    let hook_len_call = builder.ins().call(runtime_refs.strlen, &[hook_ptr]);
    let hook_len = builder.inst_results(hook_len_call)[0];
    let empty_hook = builder.ins().icmp_imm(IntCC::Equal, hook_len, 0);
    builder
        .ins()
        .brif(empty_hook, os_open_block, &[], hook_value_block, &[]);

    builder.switch_to_block(hook_value_block);
    builder.seal_block(hook_value_block);
    let value_call = builder.ins().call(runtime_refs.atoll, &[hook_ptr]);
    let value = builder.inst_results(value_call)[0];
    emit_i64_host_audit_line(builder, runtime_refs, &ok_line)?;
    builder.ins().jump(merge_block, &[BlockArg::Value(value)]);

    builder.switch_to_block(os_open_block);
    builder.seal_block(os_open_block);
    let bytes_slot =
        builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 0));
    let bytes_ptr = builder.ins().stack_addr(types::I64, bytes_slot, 0);
    let source_ptr = emit_i64_path_ptr(builder, "/dev/urandom")?;
    let open_flags = builder.ins().iconst(types::I32, 0);
    let open_call = builder
        .ins()
        .call(runtime_refs.open, &[source_ptr, open_flags]);
    let fd = builder.inst_results(open_call)[0];
    let open_failed = builder.ins().icmp_imm(IntCC::SignedLessThan, fd, 0);
    builder.ins().brif(
        open_failed,
        denied_block,
        &[],
        read_block,
        &[BlockArg::Value(fd)],
    );

    builder.switch_to_block(read_block);
    builder.seal_block(read_block);
    let fd = builder.block_params(read_block)[0];
    let requested = builder.ins().iconst(types::I64, 8);
    let read_call = builder
        .ins()
        .call(runtime_refs.read, &[fd, bytes_ptr, requested]);
    let bytes_read = builder.inst_results(read_call)[0];
    builder.ins().call(runtime_refs.close, &[fd]);
    let read_complete = builder.ins().icmp(IntCC::Equal, bytes_read, requested);
    let os_value = builder.ins().stack_load(types::I64, bytes_slot, 0);
    builder.ins().brif(
        read_complete,
        success_block,
        &[BlockArg::Value(os_value)],
        denied_block,
        &[],
    );

    builder.switch_to_block(success_block);
    builder.seal_block(success_block);
    let value = builder.block_params(success_block)[0];
    emit_i64_host_audit_line(builder, runtime_refs, &ok_line)?;
    builder.ins().jump(merge_block, &[BlockArg::Value(value)]);

    builder.switch_to_block(denied_block);
    builder.seal_block(denied_block);
    emit_i64_host_audit_line(builder, runtime_refs, &denied_line)?;
    let failed = builder.ins().iconst(types::I64, 0);
    builder.ins().jump(merge_block, &[BlockArg::Value(failed)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

fn emit_i64_random_bytes_len_expr(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    length: &I64Expr,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let length = emit_i64_expr(builder, locals, function_refs, runtime_refs, length)?;
    let failed_block = builder.create_block();
    let success_block = builder.create_block();
    let nonzero_block = builder.create_block();
    let hook_check_block = builder.create_block();
    let hook_len_block = builder.create_block();
    let os_open_block = builder.create_block();
    let read_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(read_block, types::I32);
    builder.append_block_param(merge_block, types::I64);

    let min_valid = builder
        .ins()
        .icmp_imm(IntCC::SignedGreaterThanOrEqual, length, 0);
    let max_valid = builder
        .ins()
        .icmp_imm(IntCC::SignedLessThanOrEqual, length, 65_536);
    let valid_length = builder.ins().band(min_valid, max_valid);
    builder
        .ins()
        .brif(valid_length, nonzero_block, &[], failed_block, &[]);

    builder.switch_to_block(nonzero_block);
    builder.seal_block(nonzero_block);
    let zero_length = builder.ins().icmp_imm(IntCC::Equal, length, 0);
    builder
        .ins()
        .brif(zero_length, success_block, &[], hook_check_block, &[]);

    builder.switch_to_block(hook_check_block);
    builder.seal_block(hook_check_block);
    let hook_key_ptr = emit_i64_path_ptr(builder, "AXIOM_TEST_RANDOM_BYTES")?;
    let hook_call = builder.ins().call(runtime_refs.getenv, &[hook_key_ptr]);
    let hook_ptr = builder.inst_results(hook_call)[0];
    let missing_hook = builder.ins().icmp_imm(IntCC::Equal, hook_ptr, 0);
    builder
        .ins()
        .brif(missing_hook, os_open_block, &[], hook_len_block, &[]);

    builder.switch_to_block(hook_len_block);
    builder.seal_block(hook_len_block);
    let hook_len_call = builder.ins().call(runtime_refs.strlen, &[hook_ptr]);
    let hook_len = builder.inst_results(hook_len_call)[0];
    let hook_has_bytes = builder
        .ins()
        .icmp(IntCC::SignedGreaterThanOrEqual, hook_len, length);
    builder
        .ins()
        .brif(hook_has_bytes, success_block, &[], failed_block, &[]);

    builder.switch_to_block(os_open_block);
    builder.seal_block(os_open_block);
    let bytes_slot =
        builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 65_536, 0));
    let bytes_ptr = builder.ins().stack_addr(types::I64, bytes_slot, 0);
    let source_ptr = emit_i64_path_ptr(builder, "/dev/urandom")?;
    let open_flags = builder.ins().iconst(types::I32, 0);
    let open_call = builder
        .ins()
        .call(runtime_refs.open, &[source_ptr, open_flags]);
    let fd = builder.inst_results(open_call)[0];
    let open_failed = builder.ins().icmp_imm(IntCC::SignedLessThan, fd, 0);
    builder.ins().brif(
        open_failed,
        failed_block,
        &[],
        read_block,
        &[BlockArg::Value(fd)],
    );

    builder.switch_to_block(read_block);
    builder.seal_block(read_block);
    let fd = builder.block_params(read_block)[0];
    let read_call = builder
        .ins()
        .call(runtime_refs.read, &[fd, bytes_ptr, length]);
    let bytes_read = builder.inst_results(read_call)[0];
    builder.ins().call(runtime_refs.close, &[fd]);
    let read_complete = builder.ins().icmp(IntCC::Equal, bytes_read, length);
    builder
        .ins()
        .brif(read_complete, success_block, &[], failed_block, &[]);

    builder.switch_to_block(success_block);
    builder.seal_block(success_block);
    builder.ins().jump(merge_block, &[BlockArg::Value(length)]);

    builder.switch_to_block(failed_block);
    builder.seal_block(failed_block);
    let failed = builder.ins().iconst(types::I64, -1);
    builder.ins().jump(merge_block, &[BlockArg::Value(failed)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

fn emit_i64_file_len_expr(
    builder: &mut FunctionBuilder<'_>,
    runtime_refs: I64RuntimeRefs,
    path: &str,
    max_bytes: u64,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    if path.as_bytes().contains(&0) {
        return Err(CraneliftBackendError::new(
            "filesystem path contains an interior null byte",
        ));
    }
    let max_bytes = i64::try_from(max_bytes)
        .map_err(|_| CraneliftBackendError::new("filesystem read cap is too large"))?;
    let path_len = u32::try_from(path.len() + 1)
        .map_err(|_| CraneliftBackendError::new("filesystem path is too large"))?;
    let path_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        path_len,
        0,
    ));
    for (offset, byte) in path.bytes().chain(std::iter::once(0)).enumerate() {
        let byte_value = builder.ins().iconst(types::I8, i64::from(byte));
        builder
            .ins()
            .stack_store(byte_value, path_slot, offset as i32);
    }

    let path_ptr = builder.ins().stack_addr(types::I64, path_slot, 0);
    let open_flags = builder.ins().iconst(types::I32, 0);
    let open_call = builder
        .ins()
        .call(runtime_refs.open, &[path_ptr, open_flags]);
    let fd = builder.inst_results(open_call)[0];

    let missing_block = builder.create_block();
    let seek_block = builder.create_block();
    let size_check_block = builder.create_block();
    let too_large_block = builder.create_block();
    let present_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);

    let open_failed = builder.ins().icmp_imm(IntCC::SignedLessThan, fd, 0);
    builder
        .ins()
        .brif(open_failed, missing_block, &[], seek_block, &[]);

    builder.switch_to_block(seek_block);
    builder.seal_block(seek_block);
    let zero = builder.ins().iconst(types::I64, 0);
    let seek_end = builder.ins().iconst(types::I32, 2);
    let seek_call = builder
        .ins()
        .call(runtime_refs.lseek, &[fd, zero, seek_end]);
    let length = builder.inst_results(seek_call)[0];
    builder.ins().call(runtime_refs.close, &[fd]);
    let seek_failed = builder.ins().icmp_imm(IntCC::SignedLessThan, length, 0);
    builder
        .ins()
        .brif(seek_failed, missing_block, &[], size_check_block, &[]);

    builder.switch_to_block(size_check_block);
    builder.seal_block(size_check_block);
    let too_large = builder
        .ins()
        .icmp_imm(IntCC::SignedGreaterThan, length, max_bytes);
    builder
        .ins()
        .brif(too_large, too_large_block, &[], present_block, &[]);

    builder.switch_to_block(too_large_block);
    builder.seal_block(too_large_block);
    let missing_result = builder.ins().iconst(types::I64, -1);
    builder
        .ins()
        .jump(merge_block, &[BlockArg::Value(missing_result)]);

    builder.switch_to_block(present_block);
    builder.seal_block(present_block);
    builder.ins().jump(merge_block, &[BlockArg::Value(length)]);

    builder.switch_to_block(missing_block);
    builder.seal_block(missing_block);
    let missing_result = builder.ins().iconst(types::I64, -1);
    builder
        .ins()
        .jump(merge_block, &[BlockArg::Value(missing_result)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

fn emit_i64_path_ptr(
    builder: &mut FunctionBuilder<'_>,
    path: &str,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    if path.as_bytes().contains(&0) {
        return Err(CraneliftBackendError::new(
            "filesystem path contains an interior null byte",
        ));
    }
    let path_len = u32::try_from(path.len() + 1)
        .map_err(|_| CraneliftBackendError::new("filesystem path is too large"))?;
    let path_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        path_len,
        0,
    ));
    for (offset, byte) in path.bytes().chain(std::iter::once(0)).enumerate() {
        let byte_value = builder.ins().iconst(types::I8, i64::from(byte));
        builder
            .ins()
            .stack_store(byte_value, path_slot, offset as i32);
    }
    Ok(builder.ins().stack_addr(types::I64, path_slot, 0))
}

fn i64_json_escape(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => escaped.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => escaped.push(ch),
        }
    }
    escaped
}

fn i64_fs_audit_line(
    intrinsic: &str,
    package: &str,
    path_len: usize,
    content_len: Option<usize>,
    outcome: &str,
) -> String {
    let args = match content_len {
        Some(content_len) => {
            format!("{{\"path\":\"string:{path_len}\",\"content\":\"string:{content_len}\"}}")
        }
        None => format!("{{\"path\":\"string:{path_len}\"}}"),
    };
    format!(
        "{{\"package\":\"{}\",\"intrinsic\":\"{}\",\"args\":{},\"outcome\":\"{}\"}}\n",
        i64_json_escape(package),
        i64_json_escape(intrinsic),
        args,
        i64_json_escape(outcome)
    )
}

fn i64_stdio_audit_line(stream: OutputStream, byte_len: Option<usize>, outcome: &str) -> String {
    let stream_name = output_stream_name(stream);
    let intrinsic = match stream {
        OutputStream::Stdout => "io_stdout_write",
        OutputStream::Stderr => "io_stderr_write",
    };
    let byte_shape = match byte_len {
        Some(byte_len) => format!("int:{byte_len}"),
        None => String::from("int"),
    };
    format!(
        "{{\"package\":\"direct-native-i64\",\"intrinsic\":\"{}\",\"args\":{{\"stream\":\"{}\",\"bytes\":\"{}\"}},\"outcome\":\"{}\"}}\n",
        i64_json_escape(intrinsic),
        i64_json_escape(stream_name),
        i64_json_escape(&byte_shape),
        i64_json_escape(outcome)
    )
}

fn i64_env_audit_line(intrinsic: &str, package: &str, key_len: usize, outcome: &str) -> String {
    format!(
        "{{\"package\":\"{}\",\"intrinsic\":\"{}\",\"args\":{{\"key\":\"string:{key_len}\"}},\"outcome\":\"{}\"}}\n",
        i64_json_escape(package),
        i64_json_escape(intrinsic),
        i64_json_escape(outcome)
    )
}

fn i64_process_audit_line(
    intrinsic: &str,
    package: &str,
    command_len: usize,
    outcome: &str,
) -> String {
    format!(
        "{{\"package\":\"{}\",\"intrinsic\":\"{}\",\"args\":{{\"command\":\"string:{command_len}\"}},\"outcome\":\"{}\"}}\n",
        i64_json_escape(package),
        i64_json_escape(intrinsic),
        i64_json_escape(outcome)
    )
}

fn i64_net_audit_line(intrinsic: &str, package: &str, host_len: usize, outcome: &str) -> String {
    format!(
        "{{\"package\":\"{}\",\"intrinsic\":\"{}\",\"args\":{{\"host\":\"string:{host_len}\"}},\"outcome\":\"{}\"}}\n",
        i64_json_escape(package),
        i64_json_escape(intrinsic),
        i64_json_escape(outcome)
    )
}

fn i64_clock_audit_line(intrinsic: &str, package: &str, arg_name: &str, outcome: &str) -> String {
    format!(
        "{{\"package\":\"{}\",\"intrinsic\":\"{}\",\"args\":{{\"{}\":\"int\"}},\"outcome\":\"{}\"}}\n",
        i64_json_escape(package),
        i64_json_escape(intrinsic),
        i64_json_escape(arg_name),
        i64_json_escape(outcome)
    )
}

fn i64_ffi_audit_line(
    intrinsic: &str,
    package: &str,
    library: &str,
    symbol: &str,
    arg_type: &str,
    outcome: &str,
) -> String {
    format!(
        "{{\"package\":\"{}\",\"intrinsic\":\"{}\",\"args\":{{\"library\":\"{}\",\"symbol\":\"{}\",\"value\":\"{}\"}},\"outcome\":\"{}\"}}\n",
        i64_json_escape(package),
        i64_json_escape(intrinsic),
        i64_json_escape(library),
        i64_json_escape(symbol),
        i64_json_escape(arg_type),
        i64_json_escape(outcome)
    )
}

fn i64_crypto_audit_line(
    intrinsic: &str,
    package: &str,
    arg_name: &str,
    arg_value: &str,
    outcome: &str,
) -> String {
    format!(
        "{{\"package\":\"{}\",\"intrinsic\":\"{}\",\"args\":{{\"{}\":\"{}\"}},\"outcome\":\"{}\"}}\n",
        i64_json_escape(package),
        i64_json_escape(intrinsic),
        i64_json_escape(arg_name),
        i64_json_escape(arg_value),
        i64_json_escape(outcome)
    )
}

fn i64_crypto_no_args_audit_line(intrinsic: &str, package: &str, outcome: &str) -> String {
    format!(
        "{{\"package\":\"{}\",\"intrinsic\":\"{}\",\"args\":{{}},\"outcome\":\"{}\"}}\n",
        i64_json_escape(package),
        i64_json_escape(intrinsic),
        i64_json_escape(outcome)
    )
}

fn emit_i64_host_audit_line(
    builder: &mut FunctionBuilder<'_>,
    runtime_refs: I64RuntimeRefs,
    line: &str,
) -> Result<(), CraneliftBackendError> {
    let key_ptr = emit_i64_path_ptr(builder, "AXIOM_HOST_AUDIT_LOG")?;
    let getenv_call = builder.ins().call(runtime_refs.getenv, &[key_ptr]);
    let audit_path = builder.inst_results(getenv_call)[0];

    let skip_block = builder.create_block();
    let len_block = builder.create_block();
    let open_block = builder.create_block();
    let write_block = builder.create_block();
    let merge_block = builder.create_block();

    let missing_path = builder.ins().icmp_imm(IntCC::Equal, audit_path, 0);
    builder
        .ins()
        .brif(missing_path, skip_block, &[], len_block, &[]);

    builder.switch_to_block(len_block);
    builder.seal_block(len_block);
    let strlen_call = builder.ins().call(runtime_refs.strlen, &[audit_path]);
    let audit_path_len = builder.inst_results(strlen_call)[0];
    let empty_path = builder.ins().icmp_imm(IntCC::Equal, audit_path_len, 0);
    builder
        .ins()
        .brif(empty_path, skip_block, &[], open_block, &[]);

    builder.switch_to_block(open_block);
    builder.seal_block(open_block);
    let mode_ptr = emit_i64_path_ptr(builder, "ab")?;
    let fopen_call = builder
        .ins()
        .call(runtime_refs.fopen, &[audit_path, mode_ptr]);
    let file = builder.inst_results(fopen_call)[0];
    let open_failed = builder.ins().icmp_imm(IntCC::Equal, file, 0);
    builder
        .ins()
        .brif(open_failed, skip_block, &[], write_block, &[]);

    builder.switch_to_block(write_block);
    builder.seal_block(write_block);
    let line_len = u32::try_from(line.len())
        .map_err(|_| CraneliftBackendError::new("audit line is too large"))?;
    let line_ptr = emit_i64_path_ptr(builder, line)?;
    let element_size = builder.ins().iconst(types::I64, 1);
    let element_count = builder.ins().iconst(types::I64, i64::from(line_len));
    builder.ins().call(
        runtime_refs.fwrite,
        &[line_ptr, element_size, element_count, file],
    );
    builder.ins().call(runtime_refs.fclose, &[file]);
    builder.ins().jump(merge_block, &[]);

    builder.switch_to_block(skip_block);
    builder.seal_block(skip_block);
    builder.ins().jump(merge_block, &[]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(())
}

fn emit_i64_audit_fs_expr(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    intrinsic: &str,
    package: &str,
    path_len: usize,
    content_len: Option<usize>,
    success: I64AuditSuccess,
    result: &I64Expr,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let status = emit_i64_expr(builder, locals, function_refs, runtime_refs, result)?;
    let ok_line = i64_fs_audit_line(intrinsic, package, path_len, content_len, "ok");
    let denied_line = i64_fs_audit_line(intrinsic, package, path_len, content_len, "denied");

    let ok_block = builder.create_block();
    let denied_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);

    let ok = match success {
        I64AuditSuccess::ExitZero => builder.ins().icmp_imm(IntCC::Equal, status, 0),
        I64AuditSuccess::NonNegative => {
            builder
                .ins()
                .icmp_imm(IntCC::SignedGreaterThanOrEqual, status, 0)
        }
    };
    builder.ins().brif(ok, ok_block, &[], denied_block, &[]);

    builder.switch_to_block(ok_block);
    builder.seal_block(ok_block);
    emit_i64_host_audit_line(builder, runtime_refs, &ok_line)?;
    builder.ins().jump(merge_block, &[BlockArg::Value(status)]);

    builder.switch_to_block(denied_block);
    builder.seal_block(denied_block);
    emit_i64_host_audit_line(builder, runtime_refs, &denied_line)?;
    builder.ins().jump(merge_block, &[BlockArg::Value(status)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

fn emit_i64_audit_env_expr(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    intrinsic: &str,
    package: &str,
    key_len: usize,
    success: I64AuditSuccess,
    result: &I64Expr,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let status = emit_i64_expr(builder, locals, function_refs, runtime_refs, result)?;
    let ok_line = i64_env_audit_line(intrinsic, package, key_len, "ok");
    let denied_line = i64_env_audit_line(intrinsic, package, key_len, "denied");

    let ok_block = builder.create_block();
    let denied_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);

    let ok = match success {
        I64AuditSuccess::ExitZero => builder.ins().icmp_imm(IntCC::Equal, status, 0),
        I64AuditSuccess::NonNegative => {
            builder
                .ins()
                .icmp_imm(IntCC::SignedGreaterThanOrEqual, status, 0)
        }
    };
    builder.ins().brif(ok, ok_block, &[], denied_block, &[]);

    builder.switch_to_block(ok_block);
    builder.seal_block(ok_block);
    emit_i64_host_audit_line(builder, runtime_refs, &ok_line)?;
    builder.ins().jump(merge_block, &[BlockArg::Value(status)]);

    builder.switch_to_block(denied_block);
    builder.seal_block(denied_block);
    emit_i64_host_audit_line(builder, runtime_refs, &denied_line)?;
    builder.ins().jump(merge_block, &[BlockArg::Value(status)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

fn emit_i64_audit_process_expr(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    intrinsic: &str,
    package: &str,
    command_len: usize,
    success: I64AuditSuccess,
    result: &I64Expr,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let status = emit_i64_expr(builder, locals, function_refs, runtime_refs, result)?;
    let ok_line = i64_process_audit_line(intrinsic, package, command_len, "ok");
    let denied_line = i64_process_audit_line(intrinsic, package, command_len, "denied");

    let ok_block = builder.create_block();
    let denied_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);

    let ok = match success {
        I64AuditSuccess::ExitZero => builder.ins().icmp_imm(IntCC::Equal, status, 0),
        I64AuditSuccess::NonNegative => {
            builder
                .ins()
                .icmp_imm(IntCC::SignedGreaterThanOrEqual, status, 0)
        }
    };
    builder.ins().brif(ok, ok_block, &[], denied_block, &[]);

    builder.switch_to_block(ok_block);
    builder.seal_block(ok_block);
    emit_i64_host_audit_line(builder, runtime_refs, &ok_line)?;
    builder.ins().jump(merge_block, &[BlockArg::Value(status)]);

    builder.switch_to_block(denied_block);
    builder.seal_block(denied_block);
    emit_i64_host_audit_line(builder, runtime_refs, &denied_line)?;
    builder.ins().jump(merge_block, &[BlockArg::Value(status)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

fn emit_i64_audit_net_expr(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    intrinsic: &str,
    package: &str,
    host_len: usize,
    success: I64AuditSuccess,
    result: &I64Expr,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let status = emit_i64_expr(builder, locals, function_refs, runtime_refs, result)?;
    let ok_line = i64_net_audit_line(intrinsic, package, host_len, "ok");
    let denied_line = i64_net_audit_line(intrinsic, package, host_len, "denied");

    let ok_block = builder.create_block();
    let denied_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);

    let ok = match success {
        I64AuditSuccess::ExitZero => builder.ins().icmp_imm(IntCC::Equal, status, 0),
        I64AuditSuccess::NonNegative => {
            builder
                .ins()
                .icmp_imm(IntCC::SignedGreaterThanOrEqual, status, 0)
        }
    };
    builder.ins().brif(ok, ok_block, &[], denied_block, &[]);

    builder.switch_to_block(ok_block);
    builder.seal_block(ok_block);
    emit_i64_host_audit_line(builder, runtime_refs, &ok_line)?;
    builder.ins().jump(merge_block, &[BlockArg::Value(status)]);

    builder.switch_to_block(denied_block);
    builder.seal_block(denied_block);
    emit_i64_host_audit_line(builder, runtime_refs, &denied_line)?;
    builder.ins().jump(merge_block, &[BlockArg::Value(status)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

fn emit_i64_audit_clock_expr(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    intrinsic: &str,
    package: &str,
    arg_name: &str,
    success: I64AuditSuccess,
    result: &I64Expr,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let status = emit_i64_expr(builder, locals, function_refs, runtime_refs, result)?;
    let ok_line = i64_clock_audit_line(intrinsic, package, arg_name, "ok");
    let denied_line = i64_clock_audit_line(intrinsic, package, arg_name, "denied");

    let ok_block = builder.create_block();
    let denied_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);

    let ok = match success {
        I64AuditSuccess::ExitZero => builder.ins().icmp_imm(IntCC::Equal, status, 0),
        I64AuditSuccess::NonNegative => {
            builder
                .ins()
                .icmp_imm(IntCC::SignedGreaterThanOrEqual, status, 0)
        }
    };
    builder.ins().brif(ok, ok_block, &[], denied_block, &[]);

    builder.switch_to_block(ok_block);
    builder.seal_block(ok_block);
    emit_i64_host_audit_line(builder, runtime_refs, &ok_line)?;
    builder.ins().jump(merge_block, &[BlockArg::Value(status)]);

    builder.switch_to_block(denied_block);
    builder.seal_block(denied_block);
    emit_i64_host_audit_line(builder, runtime_refs, &denied_line)?;
    builder.ins().jump(merge_block, &[BlockArg::Value(status)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

fn emit_i64_audit_ffi_expr(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    intrinsic: &str,
    package: &str,
    library: &str,
    symbol: &str,
    arg_type: &str,
    success: I64AuditSuccess,
    result: &I64Expr,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let status = emit_i64_expr(builder, locals, function_refs, runtime_refs, result)?;
    let ok_line = i64_ffi_audit_line(intrinsic, package, library, symbol, arg_type, "ok");
    let denied_line = i64_ffi_audit_line(intrinsic, package, library, symbol, arg_type, "denied");

    let ok_block = builder.create_block();
    let denied_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);

    let ok = match success {
        I64AuditSuccess::ExitZero => builder.ins().icmp_imm(IntCC::Equal, status, 0),
        I64AuditSuccess::NonNegative => {
            builder
                .ins()
                .icmp_imm(IntCC::SignedGreaterThanOrEqual, status, 0)
        }
    };
    builder.ins().brif(ok, ok_block, &[], denied_block, &[]);

    builder.switch_to_block(ok_block);
    builder.seal_block(ok_block);
    emit_i64_host_audit_line(builder, runtime_refs, &ok_line)?;
    builder.ins().jump(merge_block, &[BlockArg::Value(status)]);

    builder.switch_to_block(denied_block);
    builder.seal_block(denied_block);
    emit_i64_host_audit_line(builder, runtime_refs, &denied_line)?;
    builder.ins().jump(merge_block, &[BlockArg::Value(status)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

fn emit_i64_audit_crypto_expr(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    intrinsic: &str,
    package: &str,
    arg_name: &str,
    arg_value: &str,
    success: I64AuditSuccess,
    result: &I64Expr,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let status = emit_i64_expr(builder, locals, function_refs, runtime_refs, result)?;
    let ok_line = i64_crypto_audit_line(intrinsic, package, arg_name, arg_value, "ok");
    let denied_line = i64_crypto_audit_line(intrinsic, package, arg_name, arg_value, "denied");

    let ok_block = builder.create_block();
    let denied_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);

    let ok = match success {
        I64AuditSuccess::ExitZero => builder.ins().icmp_imm(IntCC::Equal, status, 0),
        I64AuditSuccess::NonNegative => {
            builder
                .ins()
                .icmp_imm(IntCC::SignedGreaterThanOrEqual, status, 0)
        }
    };
    builder.ins().brif(ok, ok_block, &[], denied_block, &[]);

    builder.switch_to_block(ok_block);
    builder.seal_block(ok_block);
    emit_i64_host_audit_line(builder, runtime_refs, &ok_line)?;
    builder.ins().jump(merge_block, &[BlockArg::Value(status)]);

    builder.switch_to_block(denied_block);
    builder.seal_block(denied_block);
    emit_i64_host_audit_line(builder, runtime_refs, &denied_line)?;
    builder.ins().jump(merge_block, &[BlockArg::Value(status)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

fn emit_i64_runtime_fs_guard_expr(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    root: &str,
    path: &str,
    fallback_path: &str,
    result: &I64Expr,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    if root.as_bytes().contains(&0) {
        return Err(CraneliftBackendError::new(
            "filesystem root contains an interior null byte",
        ));
    }
    if matches!(
        std::fs::symlink_metadata(path),
        Ok(metadata) if metadata.file_type().is_symlink()
    ) {
        return Err(CraneliftBackendError::new(
            "filesystem path resolves through a dangling symlink",
        ));
    }
    let path_ptr = emit_i64_path_ptr(builder, path)?;
    let fallback_ptr = emit_i64_path_ptr(builder, fallback_path)?;
    let resolved_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        I64_REALPATH_BUFFER_BYTES,
        0,
    ));
    let resolved_ptr = builder.ins().stack_addr(types::I64, resolved_slot, 0);

    let realpath_call = builder
        .ins()
        .call(runtime_refs.realpath, &[path_ptr, resolved_ptr]);
    let resolved = builder.inst_results(realpath_call)[0];

    let fallback_block = builder.create_block();
    let check_block = builder.create_block();
    let allowed_block = builder.create_block();
    let denied_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(check_block, types::I64);
    builder.append_block_param(merge_block, types::I64);

    let missing = builder.ins().icmp_imm(IntCC::Equal, resolved, 0);
    builder.ins().brif(
        missing,
        fallback_block,
        &[],
        check_block,
        &[BlockArg::Value(resolved)],
    );

    builder.switch_to_block(fallback_block);
    builder.seal_block(fallback_block);
    let fallback_call = builder
        .ins()
        .call(runtime_refs.realpath, &[fallback_ptr, resolved_ptr]);
    let fallback_resolved = builder.inst_results(fallback_call)[0];
    let fallback_missing = builder.ins().icmp_imm(IntCC::Equal, fallback_resolved, 0);
    builder.ins().brif(
        fallback_missing,
        denied_block,
        &[],
        check_block,
        &[BlockArg::Value(fallback_resolved)],
    );

    builder.switch_to_block(check_block);
    builder.seal_block(check_block);
    let canonical_ptr = builder.block_params(check_block)[0];
    let in_root = emit_i64_canonical_root_check(builder, runtime_refs, canonical_ptr, root)?;
    builder
        .ins()
        .brif(in_root, allowed_block, &[], denied_block, &[]);

    builder.switch_to_block(allowed_block);
    builder.seal_block(allowed_block);
    let result = emit_i64_expr(builder, locals, function_refs, runtime_refs, result)?;
    builder.ins().jump(merge_block, &[BlockArg::Value(result)]);

    builder.switch_to_block(denied_block);
    builder.seal_block(denied_block);
    let denied = builder.ins().iconst(types::I64, -1);
    builder.ins().jump(merge_block, &[BlockArg::Value(denied)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

fn emit_i64_canonical_root_check(
    builder: &mut FunctionBuilder<'_>,
    runtime_refs: I64RuntimeRefs,
    canonical_ptr: cranelift_codegen::ir::Value,
    root: &str,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let root_len = i64::try_from(root.len())
        .map_err(|_| CraneliftBackendError::new("filesystem root is too large"))?;
    let root_ptr = emit_i64_path_ptr(builder, root)?;
    let root_len_value = builder.ins().iconst(types::I64, root_len);
    let strncmp_call = builder.ins().call(
        runtime_refs.strncmp,
        &[canonical_ptr, root_ptr, root_len_value],
    );
    let strncmp_result = builder.inst_results(strncmp_call)[0];
    let prefix_matches = builder.ins().icmp_imm(IntCC::Equal, strncmp_result, 0);
    if root == "/" {
        return Ok(prefix_matches);
    }
    let boundary_ptr = builder.ins().iadd(canonical_ptr, root_len_value);
    let boundary = builder
        .ins()
        .load(types::I8, MemFlags::new(), boundary_ptr, 0);
    let is_end = builder.ins().icmp_imm(IntCC::Equal, boundary, 0);
    let is_separator = builder
        .ins()
        .icmp_imm(IntCC::Equal, boundary, i64::from(b'/'));
    let boundary_ok = builder.ins().bor(is_end, is_separator);
    Ok(builder.ins().band(prefix_matches, boundary_ok))
}

fn emit_i64_write_file_expr(
    builder: &mut FunctionBuilder<'_>,
    runtime_refs: I64RuntimeRefs,
    path: &str,
    content: &str,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    if path.as_bytes().contains(&0) {
        return Err(CraneliftBackendError::new(
            "filesystem path contains an interior null byte",
        ));
    }
    let path_len = u32::try_from(path.len() + 1)
        .map_err(|_| CraneliftBackendError::new("filesystem path is too large"))?;
    let content_len = u32::try_from(content.len())
        .map_err(|_| CraneliftBackendError::new("filesystem write content is too large"))?;
    let path_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        path_len,
        0,
    ));
    for (offset, byte) in path.bytes().chain(std::iter::once(0)).enumerate() {
        let byte_value = builder.ins().iconst(types::I8, i64::from(byte));
        builder
            .ins()
            .stack_store(byte_value, path_slot, offset as i32);
    }
    let content_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        content_len.max(1),
        0,
    ));
    for (offset, byte) in content.bytes().enumerate() {
        let byte_value = builder.ins().iconst(types::I8, i64::from(byte));
        builder
            .ins()
            .stack_store(byte_value, content_slot, offset as i32);
    }

    let path_ptr = builder.ins().stack_addr(types::I64, path_slot, 0);
    let mode = builder.ins().iconst(types::I32, 0o666);
    let creat_call = builder.ins().call(runtime_refs.creat, &[path_ptr, mode]);
    let fd = builder.inst_results(creat_call)[0];

    let failed_block = builder.create_block();
    let write_block = builder.create_block();
    let close_block = builder.create_block();
    let success_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(close_block, types::I64);
    builder.append_block_param(merge_block, types::I64);

    let open_failed = builder.ins().icmp_imm(IntCC::SignedLessThan, fd, 0);
    builder
        .ins()
        .brif(open_failed, failed_block, &[], write_block, &[]);

    builder.switch_to_block(write_block);
    builder.seal_block(write_block);
    let content_ptr = builder.ins().stack_addr(types::I64, content_slot, 0);
    let expected_len = builder.ins().iconst(types::I64, i64::from(content_len));
    let write_call = builder
        .ins()
        .call(runtime_refs.write, &[fd, content_ptr, expected_len]);
    let written = builder.inst_results(write_call)[0];
    let full_write = builder.ins().icmp(IntCC::Equal, written, expected_len);
    let success_value = builder.ins().iconst(types::I64, 0);
    let failure_value = builder.ins().iconst(types::I64, -1);
    let write_result = builder
        .ins()
        .select(full_write, success_value, failure_value);
    builder
        .ins()
        .jump(close_block, &[BlockArg::Value(write_result)]);

    builder.switch_to_block(close_block);
    builder.seal_block(close_block);
    let write_result = builder.block_params(close_block)[0];
    let close_call = builder.ins().call(runtime_refs.close, &[fd]);
    let close_result = builder.inst_results(close_call)[0];
    let close_ok = builder.ins().icmp_imm(IntCC::Equal, close_result, 0);
    let write_ok = builder.ins().icmp_imm(IntCC::Equal, write_result, 0);
    let ok = builder.ins().band(close_ok, write_ok);
    builder
        .ins()
        .brif(ok, success_block, &[], failed_block, &[]);

    builder.switch_to_block(success_block);
    builder.seal_block(success_block);
    let success = builder.ins().iconst(types::I64, 0);
    builder.ins().jump(merge_block, &[BlockArg::Value(success)]);

    builder.switch_to_block(failed_block);
    builder.seal_block(failed_block);
    let failed = builder.ins().iconst(types::I64, -1);
    builder.ins().jump(merge_block, &[BlockArg::Value(failed)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

fn emit_i64_append_file_expr(
    builder: &mut FunctionBuilder<'_>,
    runtime_refs: I64RuntimeRefs,
    path: &str,
    content: &str,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    if path.as_bytes().contains(&0) {
        return Err(CraneliftBackendError::new(
            "filesystem path contains an interior null byte",
        ));
    }
    let path_len = u32::try_from(path.len() + 1)
        .map_err(|_| CraneliftBackendError::new("filesystem path is too large"))?;
    let content_len = u32::try_from(content.len())
        .map_err(|_| CraneliftBackendError::new("filesystem append content is too large"))?;
    let path_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        path_len,
        0,
    ));
    for (offset, byte) in path.bytes().chain(std::iter::once(0)).enumerate() {
        let byte_value = builder.ins().iconst(types::I8, i64::from(byte));
        builder
            .ins()
            .stack_store(byte_value, path_slot, offset as i32);
    }
    let content_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        content_len.max(1),
        0,
    ));
    for (offset, byte) in content.bytes().enumerate() {
        let byte_value = builder.ins().iconst(types::I8, i64::from(byte));
        builder
            .ins()
            .stack_store(byte_value, content_slot, offset as i32);
    }
    let mode_slot =
        builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 3, 0));
    for (offset, byte) in b"ab\0".iter().enumerate() {
        let byte_value = builder.ins().iconst(types::I8, i64::from(*byte));
        builder
            .ins()
            .stack_store(byte_value, mode_slot, offset as i32);
    }

    let path_ptr = builder.ins().stack_addr(types::I64, path_slot, 0);
    let mode_ptr = builder.ins().stack_addr(types::I64, mode_slot, 0);
    let fopen_call = builder
        .ins()
        .call(runtime_refs.fopen, &[path_ptr, mode_ptr]);
    let file = builder.inst_results(fopen_call)[0];

    let failed_block = builder.create_block();
    let write_block = builder.create_block();
    let close_block = builder.create_block();
    let success_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(close_block, types::I64);
    builder.append_block_param(merge_block, types::I64);

    let open_failed = builder.ins().icmp_imm(IntCC::Equal, file, 0);
    builder
        .ins()
        .brif(open_failed, failed_block, &[], write_block, &[]);

    builder.switch_to_block(write_block);
    builder.seal_block(write_block);
    let content_ptr = builder.ins().stack_addr(types::I64, content_slot, 0);
    let element_size = builder.ins().iconst(types::I64, 1);
    let element_count = builder.ins().iconst(types::I64, i64::from(content_len));
    let fwrite_call = builder.ins().call(
        runtime_refs.fwrite,
        &[content_ptr, element_size, element_count, file],
    );
    let written = builder.inst_results(fwrite_call)[0];
    let full_write = builder.ins().icmp(IntCC::Equal, written, element_count);
    let success_value = builder.ins().iconst(types::I64, 0);
    let failure_value = builder.ins().iconst(types::I64, -1);
    let write_result = builder
        .ins()
        .select(full_write, success_value, failure_value);
    builder
        .ins()
        .jump(close_block, &[BlockArg::Value(write_result)]);

    builder.switch_to_block(close_block);
    builder.seal_block(close_block);
    let write_result = builder.block_params(close_block)[0];
    let fclose_call = builder.ins().call(runtime_refs.fclose, &[file]);
    let close_result = builder.inst_results(fclose_call)[0];
    let close_ok = builder.ins().icmp_imm(IntCC::Equal, close_result, 0);
    let write_ok = builder.ins().icmp_imm(IntCC::Equal, write_result, 0);
    let ok = builder.ins().band(close_ok, write_ok);
    builder
        .ins()
        .brif(ok, success_block, &[], failed_block, &[]);

    builder.switch_to_block(success_block);
    builder.seal_block(success_block);
    let success = builder.ins().iconst(types::I64, 0);
    builder.ins().jump(merge_block, &[BlockArg::Value(success)]);

    builder.switch_to_block(failed_block);
    builder.seal_block(failed_block);
    let failed = builder.ins().iconst(types::I64, -1);
    builder.ins().jump(merge_block, &[BlockArg::Value(failed)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

fn emit_i64_create_file_expr(
    builder: &mut FunctionBuilder<'_>,
    runtime_refs: I64RuntimeRefs,
    path: &str,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    if path.as_bytes().contains(&0) {
        return Err(CraneliftBackendError::new(
            "filesystem path contains an interior null byte",
        ));
    }
    let path_len = u32::try_from(path.len() + 1)
        .map_err(|_| CraneliftBackendError::new("filesystem path is too large"))?;
    let path_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        path_len,
        0,
    ));
    for (offset, byte) in path.bytes().chain(std::iter::once(0)).enumerate() {
        let byte_value = builder.ins().iconst(types::I8, i64::from(byte));
        builder
            .ins()
            .stack_store(byte_value, path_slot, offset as i32);
    }
    let mode_slot =
        builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 3, 0));
    for (offset, byte) in b"wx\0".iter().enumerate() {
        let byte_value = builder.ins().iconst(types::I8, i64::from(*byte));
        builder
            .ins()
            .stack_store(byte_value, mode_slot, offset as i32);
    }

    let path_ptr = builder.ins().stack_addr(types::I64, path_slot, 0);
    let mode_ptr = builder.ins().stack_addr(types::I64, mode_slot, 0);
    let fopen_call = builder
        .ins()
        .call(runtime_refs.fopen, &[path_ptr, mode_ptr]);
    let file = builder.inst_results(fopen_call)[0];

    let failed_block = builder.create_block();
    let close_block = builder.create_block();
    let success_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);

    let open_failed = builder.ins().icmp_imm(IntCC::Equal, file, 0);
    builder
        .ins()
        .brif(open_failed, failed_block, &[], close_block, &[]);

    builder.switch_to_block(close_block);
    builder.seal_block(close_block);
    let fclose_call = builder.ins().call(runtime_refs.fclose, &[file]);
    let close_result = builder.inst_results(fclose_call)[0];
    let close_ok = builder.ins().icmp_imm(IntCC::Equal, close_result, 0);
    builder
        .ins()
        .brif(close_ok, success_block, &[], failed_block, &[]);

    builder.switch_to_block(success_block);
    builder.seal_block(success_block);
    let success = builder.ins().iconst(types::I64, 0);
    builder.ins().jump(merge_block, &[BlockArg::Value(success)]);

    builder.switch_to_block(failed_block);
    builder.seal_block(failed_block);
    let failed = builder.ins().iconst(types::I64, -1);
    builder.ins().jump(merge_block, &[BlockArg::Value(failed)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

fn emit_i64_replace_file_expr(
    builder: &mut FunctionBuilder<'_>,
    runtime_refs: I64RuntimeRefs,
    path: &str,
    temp_path: &str,
    content: &str,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let write_result = emit_i64_write_file_expr(builder, runtime_refs, temp_path, content)?;

    let rename_block = builder.create_block();
    let success_block = builder.create_block();
    let failed_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);

    let write_ok = builder.ins().icmp_imm(IntCC::Equal, write_result, 0);
    builder
        .ins()
        .brif(write_ok, rename_block, &[], failed_block, &[]);

    builder.switch_to_block(rename_block);
    builder.seal_block(rename_block);
    let temp_ptr = emit_i64_path_ptr(builder, temp_path)?;
    let path_ptr = emit_i64_path_ptr(builder, path)?;
    let rename_call = builder
        .ins()
        .call(runtime_refs.rename, &[temp_ptr, path_ptr]);
    let rename_result = builder.inst_results(rename_call)[0];
    let rename_ok = builder.ins().icmp_imm(IntCC::Equal, rename_result, 0);
    builder
        .ins()
        .brif(rename_ok, success_block, &[], failed_block, &[]);

    builder.switch_to_block(success_block);
    builder.seal_block(success_block);
    let success = builder.ins().iconst(types::I64, 0);
    builder.ins().jump(merge_block, &[BlockArg::Value(success)]);

    builder.switch_to_block(failed_block);
    builder.seal_block(failed_block);
    let temp_ptr = emit_i64_path_ptr(builder, temp_path)?;
    builder.ins().call(runtime_refs.unlink, &[temp_ptr]);
    let failed = builder.ins().iconst(types::I64, -1);
    builder.ins().jump(merge_block, &[BlockArg::Value(failed)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

fn emit_i64_remove_file_expr(
    builder: &mut FunctionBuilder<'_>,
    runtime_refs: I64RuntimeRefs,
    path: &str,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let path_ptr = emit_i64_path_ptr(builder, path)?;
    let unlink_call = builder.ins().call(runtime_refs.unlink, &[path_ptr]);
    let result = builder.inst_results(unlink_call)[0];
    let ok = builder.ins().icmp_imm(IntCC::Equal, result, 0);
    let success = builder.ins().iconst(types::I64, 0);
    let failed = builder.ins().iconst(types::I64, -1);
    Ok(builder.ins().select(ok, success, failed))
}

fn emit_i64_make_dir_expr(
    builder: &mut FunctionBuilder<'_>,
    runtime_refs: I64RuntimeRefs,
    path: &str,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let path_ptr = emit_i64_path_ptr(builder, path)?;
    let mode = builder.ins().iconst(types::I32, 0o777);
    let mkdir_call = builder.ins().call(runtime_refs.mkdir, &[path_ptr, mode]);
    let result = builder.inst_results(mkdir_call)[0];
    let ok = builder.ins().icmp_imm(IntCC::Equal, result, 0);
    let success = builder.ins().iconst(types::I64, 0);
    let failed = builder.ins().iconst(types::I64, -1);
    Ok(builder.ins().select(ok, success, failed))
}

fn i64_mkdir_all_prefixes(path: &str) -> Result<Vec<String>, CraneliftBackendError> {
    if path.as_bytes().contains(&0) {
        return Err(CraneliftBackendError::new(
            "filesystem path contains an interior null byte",
        ));
    }
    let mut prefixes = Vec::new();
    let mut current = PathBuf::new();
    for component in Path::new(path).components() {
        current.push(component.as_os_str());
        if current == Path::new("/") || current.as_os_str().is_empty() {
            continue;
        }
        prefixes.push(current.display().to_string());
    }
    if prefixes.is_empty() {
        return Err(CraneliftBackendError::new(
            "filesystem mkdir_all path has no directory components",
        ));
    }
    Ok(prefixes)
}

fn emit_i64_make_dir_all_expr(
    builder: &mut FunctionBuilder<'_>,
    runtime_refs: I64RuntimeRefs,
    path: &str,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let prefixes = i64_mkdir_all_prefixes(path)?;
    let mode = builder.ins().iconst(types::I32, 0o777);
    for prefix in prefixes {
        let prefix_ptr = emit_i64_path_ptr(builder, &prefix)?;
        builder.ins().call(runtime_refs.mkdir, &[prefix_ptr, mode]);
    }

    let path_ptr = emit_i64_path_ptr(builder, path)?;
    let opendir_call = builder.ins().call(runtime_refs.opendir, &[path_ptr]);
    let dir = builder.inst_results(opendir_call)[0];

    let failed_block = builder.create_block();
    let close_block = builder.create_block();
    let success_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);

    let open_failed = builder.ins().icmp_imm(IntCC::Equal, dir, 0);
    builder
        .ins()
        .brif(open_failed, failed_block, &[], close_block, &[]);

    builder.switch_to_block(close_block);
    builder.seal_block(close_block);
    let closedir_call = builder.ins().call(runtime_refs.closedir, &[dir]);
    let close_result = builder.inst_results(closedir_call)[0];
    let close_ok = builder.ins().icmp_imm(IntCC::Equal, close_result, 0);
    builder
        .ins()
        .brif(close_ok, success_block, &[], failed_block, &[]);

    builder.switch_to_block(success_block);
    builder.seal_block(success_block);
    let success = builder.ins().iconst(types::I64, 0);
    builder.ins().jump(merge_block, &[BlockArg::Value(success)]);

    builder.switch_to_block(failed_block);
    builder.seal_block(failed_block);
    let failed = builder.ins().iconst(types::I64, -1);
    builder.ins().jump(merge_block, &[BlockArg::Value(failed)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

fn emit_i64_remove_dir_expr(
    builder: &mut FunctionBuilder<'_>,
    runtime_refs: I64RuntimeRefs,
    path: &str,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let path_ptr = emit_i64_path_ptr(builder, path)?;
    let rmdir_call = builder.ins().call(runtime_refs.rmdir, &[path_ptr]);
    let result = builder.inst_results(rmdir_call)[0];
    let ok = builder.ins().icmp_imm(IntCC::Equal, result, 0);
    let success = builder.ins().iconst(types::I64, 0);
    let failed = builder.ins().iconst(types::I64, -1);
    Ok(builder.ins().select(ok, success, failed))
}

fn emit_i64_process_status_expr(
    builder: &mut FunctionBuilder<'_>,
    runtime_refs: I64RuntimeRefs,
    command: &str,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    if command.as_bytes().contains(&0) {
        return Err(CraneliftBackendError::new(
            "process command contains an interior null byte",
        ));
    }
    let command_len = u32::try_from(command.len() + 1)
        .map_err(|_| CraneliftBackendError::new("process command is too large"))?;
    let command_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        command_len,
        0,
    ));
    for (offset, byte) in command.bytes().chain(std::iter::once(0)).enumerate() {
        let byte_value = builder.ins().iconst(types::I8, i64::from(byte));
        builder
            .ins()
            .stack_store(byte_value, command_slot, offset as i32);
    }

    let command_ptr = builder.ins().stack_addr(types::I64, command_slot, 0);
    let executable_flag = builder.ins().iconst(types::I32, 1);
    let access_call = builder
        .ins()
        .call(runtime_refs.access, &[command_ptr, executable_flag]);
    let access_result = builder.inst_results(access_call)[0];

    let missing_block = builder.create_block();
    let system_block = builder.create_block();
    let normalize_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(normalize_block, types::I32);
    builder.append_block_param(merge_block, types::I64);

    let access_failed = builder.ins().icmp_imm(IntCC::NotEqual, access_result, 0);
    builder
        .ins()
        .brif(access_failed, missing_block, &[], system_block, &[]);

    builder.switch_to_block(system_block);
    builder.seal_block(system_block);
    let system_call = builder.ins().call(runtime_refs.system, &[command_ptr]);
    let status = builder.inst_results(system_call)[0];
    let system_failed = builder.ins().icmp_imm(IntCC::SignedLessThan, status, 0);
    builder.ins().brif(
        system_failed,
        missing_block,
        &[],
        normalize_block,
        &[BlockArg::Value(status)],
    );

    builder.switch_to_block(normalize_block);
    builder.seal_block(normalize_block);
    let status = builder.block_params(normalize_block)[0];
    let status = builder.ins().sextend(types::I64, status);
    let status_shift = builder.ins().iconst(types::I64, 256);
    let exit_code = builder.ins().sdiv(status, status_shift);
    builder
        .ins()
        .jump(merge_block, &[BlockArg::Value(exit_code)]);

    builder.switch_to_block(missing_block);
    builder.seal_block(missing_block);
    let missing_result = builder.ins().iconst(types::I64, -1);
    builder
        .ins()
        .jump(merge_block, &[BlockArg::Value(missing_result)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

#[cfg(not(windows))]
fn emit_i64_net_resolve_len_expr(
    builder: &mut FunctionBuilder<'_>,
    runtime_refs: I64RuntimeRefs,
    host: &str,
    resolved_len: i64,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    if host.as_bytes().contains(&0) {
        return Err(CraneliftBackendError::new(
            "network host contains an interior null byte",
        ));
    }
    let host_ptr = emit_i64_path_ptr(builder, host)?;
    let null_ptr = builder.ins().iconst(types::I64, 0);
    let result_slot =
        builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 0));
    builder.ins().stack_store(null_ptr, result_slot, 0);
    let result_ptr = builder.ins().stack_addr(types::I64, result_slot, 0);
    #[cfg(not(windows))]
    let call = builder.ins().call(
        runtime_refs
            .getaddrinfo
            .expect("getaddrinfo import missing"),
        &[host_ptr, null_ptr, null_ptr, result_ptr],
    );
    #[cfg(not(windows))]
    let status = builder.inst_results(call)[0];

    let success_block = builder.create_block();
    let free_block = builder.create_block();
    let failed_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);

    let status_ok = builder.ins().icmp_imm(IntCC::Equal, status, 0);
    builder
        .ins()
        .brif(status_ok, success_block, &[], failed_block, &[]);

    builder.switch_to_block(success_block);
    builder.seal_block(success_block);
    let result_head = builder.ins().stack_load(types::I64, result_slot, 0);
    let result_missing = builder.ins().icmp_imm(IntCC::Equal, result_head, 0);
    builder
        .ins()
        .brif(result_missing, failed_block, &[], free_block, &[]);

    builder.switch_to_block(free_block);
    builder.seal_block(free_block);
    #[cfg(not(windows))]
    builder.ins().call(
        runtime_refs
            .freeaddrinfo
            .expect("freeaddrinfo import missing"),
        &[result_head],
    );
    let len = builder.ins().iconst(types::I64, resolved_len);
    builder.ins().jump(merge_block, &[BlockArg::Value(len)]);

    builder.switch_to_block(failed_block);
    builder.seal_block(failed_block);
    let failed = builder.ins().iconst(types::I64, -1);
    builder.ins().jump(merge_block, &[BlockArg::Value(failed)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

#[cfg(windows)]
fn emit_i64_net_resolve_len_expr(
    _builder: &mut FunctionBuilder<'_>,
    _runtime_refs: I64RuntimeRefs,
    _host: &str,
    _resolved_len: i64,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    Err(CraneliftBackendError::new(
        "numeric native DNS lowering is unsupported on windows",
    ))
}

fn emit_i64_sleep_ms_expr(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    milliseconds: &I64Expr,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let milliseconds = emit_i64_expr(builder, locals, function_refs, runtime_refs, milliseconds)?;
    let negative_block = builder.create_block();
    let too_large_block = builder.create_block();
    let sleep_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);

    let is_negative = builder
        .ins()
        .icmp_imm(IntCC::SignedLessThan, milliseconds, 0);
    builder
        .ins()
        .brif(is_negative, negative_block, &[], too_large_block, &[]);

    builder.switch_to_block(negative_block);
    builder.seal_block(negative_block);
    let negative_result = builder.ins().iconst(types::I64, -1);
    builder
        .ins()
        .jump(merge_block, &[BlockArg::Value(negative_result)]);

    builder.switch_to_block(too_large_block);
    builder.seal_block(too_large_block);
    let is_too_large = builder
        .ins()
        .icmp_imm(IntCC::SignedGreaterThan, milliseconds, 1_000);
    let too_large_result = builder.ins().iconst(types::I64, -1);
    builder.ins().brif(
        is_too_large,
        merge_block,
        &[BlockArg::Value(too_large_result)],
        sleep_block,
        &[],
    );

    builder.switch_to_block(sleep_block);
    builder.seal_block(sleep_block);
    let micros_factor = builder.ins().iconst(types::I64, 1_000);
    let micros = builder.ins().imul(milliseconds, micros_factor);
    let micros = builder.ins().ireduce(types::I32, micros);
    let call = builder.ins().call(runtime_refs.sleep, &[micros]);
    let result = builder.inst_results(call)[0];
    let result = builder.ins().sextend(types::I64, result);
    builder.ins().jump(merge_block, &[BlockArg::Value(result)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
}

fn emit_i64_clock_now_ms_expr(
    builder: &mut FunctionBuilder<'_>,
    runtime_refs: I64RuntimeRefs,
) -> cranelift_codegen::ir::Value {
    let timespec_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        I64_TIMESPEC_BYTES,
        0,
    ));
    let timespec_ptr = builder.ins().stack_addr(types::I64, timespec_slot, 0);
    let time_utc = builder.ins().iconst(types::I32, I64_TIME_UTC_BASE);
    let call = builder
        .ins()
        .call(runtime_refs.timespec_get, &[timespec_ptr, time_utc]);
    let status = builder.inst_results(call)[0];
    let success_block = builder.create_block();
    let denied_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);

    let ok = builder.ins().icmp_imm(IntCC::Equal, status, 1);
    builder
        .ins()
        .brif(ok, success_block, &[], denied_block, &[]);

    builder.switch_to_block(success_block);
    builder.seal_block(success_block);
    // Preserve millisecond precision by lowering C11 timespec_get(TIME_UTC)
    // directly: tv_sec contributes epoch seconds, tv_nsec contributes the
    // subsecond millisecond portion. This path must not fall back to the
    // second-resolution host clock import.
    let timespec_seconds =
        builder
            .ins()
            .stack_load(types::I64, timespec_slot, I64_TIMESPEC_SECONDS_OFFSET);
    let timespec_nanos =
        builder
            .ins()
            .stack_load(types::I64, timespec_slot, I64_TIMESPEC_NANOS_OFFSET);
    let millis_factor = builder.ins().iconst(types::I64, 1_000);
    let epoch_millis_from_seconds = builder.ins().imul(timespec_seconds, millis_factor);
    let nanos_divisor = builder.ins().iconst(types::I64, 1_000_000);
    let subsecond_millis_from_nanos = builder.ins().sdiv(timespec_nanos, nanos_divisor);
    let epoch_millis = builder
        .ins()
        .iadd(epoch_millis_from_seconds, subsecond_millis_from_nanos);
    builder
        .ins()
        .jump(merge_block, &[BlockArg::Value(epoch_millis)]);

    builder.switch_to_block(denied_block);
    builder.seal_block(denied_block);
    let denied = builder.ins().iconst(types::I64, -1);
    builder.ins().jump(merge_block, &[BlockArg::Value(denied)]);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    builder.block_params(merge_block)[0]
}

fn emit_i64_clock_elapsed_ms_expr(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    runtime_refs: I64RuntimeRefs,
    start: &I64Expr,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let start = emit_i64_expr(builder, locals, function_refs, runtime_refs, start)?;
    let now = emit_i64_clock_now_ms_expr(builder, runtime_refs);
    let elapsed = builder.ins().isub(now, start);
    let moved_backwards = builder.ins().icmp(IntCC::SignedLessThan, now, start);
    let denied = builder.ins().iconst(types::I64, -1);
    Ok(builder.ins().select(moved_backwards, denied, elapsed))
}

fn emit_i64_cast(
    builder: &mut FunctionBuilder<'_>,
    value: cranelift_codegen::ir::Value,
    cast: I64Cast,
) -> cranelift_codegen::ir::Value {
    match cast {
        I64Cast::Signed8 => {
            let narrowed = builder.ins().ireduce(types::I8, value);
            builder.ins().sextend(types::I64, narrowed)
        }
        I64Cast::Signed16 => {
            let narrowed = builder.ins().ireduce(types::I16, value);
            builder.ins().sextend(types::I64, narrowed)
        }
        I64Cast::Signed32 => {
            let narrowed = builder.ins().ireduce(types::I32, value);
            builder.ins().sextend(types::I64, narrowed)
        }
        I64Cast::Signed64 => value,
        I64Cast::Unsigned8 => {
            let narrowed = builder.ins().ireduce(types::I8, value);
            builder.ins().uextend(types::I64, narrowed)
        }
        I64Cast::Unsigned16 => {
            let narrowed = builder.ins().ireduce(types::I16, value);
            builder.ins().uextend(types::I64, narrowed)
        }
        I64Cast::Unsigned32 => {
            let narrowed = builder.ins().ireduce(types::I32, value);
            builder.ins().uextend(types::I64, narrowed)
        }
    }
}

#[cfg(target_os = "macos")]
fn host_isa_builder() -> Result<isa::Builder, CraneliftBackendError> {
    let architecture = match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        other => {
            return Err(CraneliftBackendError::new(format!(
                "unsupported macOS architecture {other:?}"
            )));
        }
    };
    let triple = format!("{architecture}-apple-macosx")
        .parse()
        .map_err(|message| CraneliftBackendError::new(format!("macOS target triple: {message}")))?;
    isa::lookup(triple)
        .map_err(|message| CraneliftBackendError::new(format!("cranelift ISA: {message}")))
}

#[cfg(not(target_os = "macos"))]
fn host_isa_builder() -> Result<isa::Builder, CraneliftBackendError> {
    cranelift_native::builder()
        .map_err(|message| CraneliftBackendError::new(format!("cranelift host ISA: {message}")))
}

fn link_object(object_path: &Path, binary_path: &Path) -> Result<(), CraneliftBackendError> {
    let linked_binary_path = temporary_output_path(binary_path);
    let mut command = Command::new("cc");
    let output = command
        .arg(object_path)
        .arg("-o")
        .arg(&linked_binary_path)
        .output()
        .map_err(|err| {
            CraneliftBackendError::new(format!("failed to invoke system linker `cc`: {err}"))
        })?;
    if output.status.success() {
        fs::rename(&linked_binary_path, binary_path).map_err(|err| {
            let _ = fs::remove_file(&linked_binary_path);
            CraneliftBackendError::new(format!(
                "failed to move linked binary into {}: {err}",
                binary_path.display()
            ))
        })?;
        return Ok(());
    }
    let _ = fs::remove_file(&linked_binary_path);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(CraneliftBackendError::new(format!(
        "system linker `cc` failed for cranelift object: {}",
        stderr.trim()
    )))
}

fn temporary_output_path(path: &Path) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "axiom-cranelift-output".into());
    path.with_file_name(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        timestamp
    ))
}

fn write_output_file(path: &Path, content: impl AsRef<[u8]>) -> Result<(), CraneliftBackendError> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let mut file = options.open(path).map_err(|err| {
        CraneliftBackendError::new(format!("failed to write {}: {err}", path.display()))
    })?;
    file.write_all(content.as_ref()).map_err(|err| {
        CraneliftBackendError::new(format!("failed to write {}: {err}", path.display()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn i64_divide_by_zero_expr() -> I64Expr {
        I64Expr::Binary {
            op: I64BinaryOp::Div,
            lhs: Box::new(I64Expr::Literal(1)),
            rhs: Box::new(I64Expr::Literal(0)),
        }
    }

    fn i64_divide_by_zero_is_zero_condition() -> I64Condition {
        I64Condition::Compare(I64Compare {
            op: I64CompareOp::Eq,
            lhs: i64_divide_by_zero_expr(),
            rhs: I64Expr::Literal(0),
        })
    }

    #[test]
    fn links_hello_print_lines() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let object = temp.path().join("hello.o");
        let binary = temp.path().join("hello");
        compile_print_lines(
            &[
                String::from("hello from stage1"),
                String::from("42"),
                String::from("true"),
            ],
            &object,
            &binary,
        )
        .expect("compile print lines");
        let output = Command::new(&binary).output().expect("run binary");
        assert!(output.status.success(), "binary exits successfully");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "hello from stage1\n42\ntrue\n"
        );
    }

    #[test]
    fn links_stdout_and_stderr_lines() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let object = temp.path().join("stdio.o");
        let binary = temp.path().join("stdio");
        compile_output_lines(
            &[
                OutputLine::stdout("ready"),
                OutputLine::stderr("audit"),
                OutputLine::stdout("done"),
            ],
            &object,
            &binary,
        )
        .expect("compile output lines");
        let output = Command::new(&binary).output().expect("run binary");
        assert!(output.status.success(), "binary exits successfully");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "ready\ndone\n");
        assert_eq!(String::from_utf8_lossy(&output.stderr), "audit\n");
    }

    #[test]
    fn links_i64_exit_program() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let object = temp.path().join("i64-exit.o");
        let binary = temp.path().join("i64-exit");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::Binary {
                    op: I64BinaryOp::Add,
                    lhs: Box::new(I64Expr::Literal(7)),
                    rhs: Box::new(I64Expr::Literal(5)),
                }),
            },
            &object,
            &binary,
        )
        .expect("compile i64 exit program");
        let output = Command::new(&binary).output().expect("run binary");
        assert_eq!(output.status.code(), Some(12));
    }

    #[test]
    fn links_i64_exit_program_with_sleep_ms() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let zero_object = temp.path().join("i64-exit-sleep-zero.o");
        let zero_binary = temp.path().join("i64-exit-sleep-zero");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::SleepMs {
                    milliseconds: Box::new(I64Expr::Literal(0)),
                }),
            },
            &zero_object,
            &zero_binary,
        )
        .expect("compile i64 zero sleep exit program");
        let output = Command::new(&zero_binary)
            .output()
            .expect("run zero sleep binary");
        assert_eq!(output.status.code(), Some(0));

        let capped_object = temp.path().join("i64-exit-sleep-capped.o");
        let capped_binary = temp.path().join("i64-exit-sleep-capped");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::SleepMs {
                    milliseconds: Box::new(I64Expr::Literal(1001)),
                }),
            },
            &capped_object,
            &capped_binary,
        )
        .expect("compile i64 capped sleep exit program");
        let output = Command::new(&capped_binary)
            .output()
            .expect("run capped sleep binary");
        assert_eq!(output.status.code(), Some(255));
    }

    #[test]
    fn links_i64_exit_program_with_env_len() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let object = temp.path().join("i64-exit-env-len.o");
        let binary = temp.path().join("i64-exit-env-len");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::EnvLen {
                    key: String::from("AXIOM_CRANELIFT_BACKEND_ENV_LEN"),
                }),
            },
            &object,
            &binary,
        )
        .expect("compile i64 env len exit program");
        let output = Command::new(&binary)
            .env("AXIOM_CRANELIFT_BACKEND_ENV_LEN", "backend-env")
            .output()
            .expect("run env len binary");
        assert_eq!(output.status.code(), Some(11));
        let output = Command::new(&binary)
            .env_remove("AXIOM_CRANELIFT_BACKEND_ENV_LEN")
            .output()
            .expect("run missing env len binary");
        assert_eq!(output.status.code(), Some(255));
    }

    #[test]
    fn links_i64_exit_program_with_file_len() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = temp.path().join("fixture.txt");
        fs::write(&fixture, "compile-time").expect("write compile-time fixture");
        let object = temp.path().join("i64-exit-file-len.o");
        let binary = temp.path().join("i64-exit-file-len");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::FileLen {
                    path: fixture.display().to_string(),
                    max_bytes: 64 * 1024 * 1024,
                }),
            },
            &object,
            &binary,
        )
        .expect("compile i64 file len exit program");

        fs::write(&fixture, "runtime-file").expect("rewrite runtime fixture");
        let output = Command::new(&binary).output().expect("run file len binary");
        assert_eq!(output.status.code(), Some(12));

        fs::remove_file(&fixture).expect("remove runtime fixture");
        let output = Command::new(&binary)
            .output()
            .expect("run missing file len binary");
        assert_eq!(output.status.code(), Some(255));
    }

    #[test]
    fn links_i64_exit_program_with_write_file() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = temp.path().join("fixture.txt");
        let object = temp.path().join("i64-exit-write-file.o");
        let binary = temp.path().join("i64-exit-write-file");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::WriteFile {
                    path: fixture.display().to_string(),
                    content: String::from("runtime-write"),
                }),
            },
            &object,
            &binary,
        )
        .expect("compile i64 write file exit program");

        assert!(
            !fixture.exists(),
            "compile should not create the write_file fixture"
        );
        let output = Command::new(&binary)
            .output()
            .expect("run write file binary");
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(
            fs::read_to_string(&fixture).expect("read runtime write fixture"),
            "runtime-write"
        );
    }

    #[test]
    fn links_i64_exit_program_with_append_file() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = temp.path().join("fixture.txt");
        fs::write(&fixture, "base").expect("write append base fixture");
        let object = temp.path().join("i64-exit-append-file.o");
        let binary = temp.path().join("i64-exit-append-file");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::AppendFile {
                    path: fixture.display().to_string(),
                    content: String::from("+runtime-append"),
                }),
            },
            &object,
            &binary,
        )
        .expect("compile i64 append file exit program");

        assert_eq!(
            fs::read_to_string(&fixture).expect("read compile-time append fixture"),
            "base"
        );
        let output = Command::new(&binary)
            .output()
            .expect("run append file binary");
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(
            fs::read_to_string(&fixture).expect("read runtime append fixture"),
            "base+runtime-append"
        );
    }

    #[test]
    fn links_i64_exit_program_with_replace_file() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = temp.path().join("fixture.txt");
        let temp_fixture = temp.path().join(".fixture.txt.axiom-replace.tmp");
        fs::write(&fixture, "base").expect("write replace base fixture");
        let object = temp.path().join("i64-exit-replace-file.o");
        let binary = temp.path().join("i64-exit-replace-file");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::ReplaceFile {
                    path: fixture.display().to_string(),
                    temp_path: temp_fixture.display().to_string(),
                    content: String::from("runtime-replace"),
                }),
            },
            &object,
            &binary,
        )
        .expect("compile i64 replace file exit program");

        assert_eq!(
            fs::read_to_string(&fixture).expect("read compile-time replace fixture"),
            "base"
        );
        assert!(
            !temp_fixture.exists(),
            "compile should not create the replace temp fixture"
        );
        let output = Command::new(&binary)
            .output()
            .expect("run replace file binary");
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(
            fs::read_to_string(&fixture).expect("read runtime replace fixture"),
            "runtime-replace"
        );
        assert!(
            !temp_fixture.exists(),
            "runtime replace should not leave the temp fixture"
        );
    }

    #[test]
    fn links_i64_exit_program_with_remove_file() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = temp.path().join("fixture.txt");
        fs::write(&fixture, "remove-me").expect("write remove fixture");
        let object = temp.path().join("i64-exit-remove-file.o");
        let binary = temp.path().join("i64-exit-remove-file");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::RemoveFile {
                    path: fixture.display().to_string(),
                }),
            },
            &object,
            &binary,
        )
        .expect("compile i64 remove file exit program");

        assert!(fixture.exists(), "compile should not remove the fixture");
        let output = Command::new(&binary)
            .output()
            .expect("run remove file binary");
        assert_eq!(output.status.code(), Some(0));
        assert!(
            !fixture.exists(),
            "runtime remove_file should remove the fixture"
        );
    }

    #[test]
    fn links_i64_exit_program_with_create_file() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = temp.path().join("created.txt");
        let object = temp.path().join("i64-exit-create-file.o");
        let binary = temp.path().join("i64-exit-create-file");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::CreateFile {
                    path: fixture.display().to_string(),
                }),
            },
            &object,
            &binary,
        )
        .expect("compile i64 create file exit program");

        assert!(
            !fixture.exists(),
            "compile should not create the create_file fixture"
        );
        let output = Command::new(&binary)
            .output()
            .expect("run create file binary");
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(
            fs::read_to_string(&fixture).expect("read created fixture"),
            ""
        );

        let output = Command::new(&binary)
            .output()
            .expect("run create file binary again");
        assert_eq!(output.status.code(), Some(255));
    }

    #[test]
    fn links_i64_exit_program_with_directory_create_and_remove() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = temp.path().join("runtime-dir");
        let object = temp.path().join("i64-exit-dir.o");
        let binary = temp.path().join("i64-exit-dir");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: vec![I64Expr::MakeDir {
                    path: fixture.display().to_string(),
                }],
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::Binary {
                    op: I64BinaryOp::Add,
                    lhs: Box::new(I64Expr::Local(0)),
                    rhs: Box::new(I64Expr::RemoveDir {
                        path: fixture.display().to_string(),
                    }),
                }),
            },
            &object,
            &binary,
        )
        .expect("compile i64 directory exit program");

        assert!(
            !fixture.exists(),
            "compile should not create the directory fixture"
        );
        let output = Command::new(&binary)
            .output()
            .expect("run directory binary");
        assert_eq!(output.status.code(), Some(0));
        assert!(
            !fixture.exists(),
            "runtime remove_dir should remove the directory fixture"
        );
    }

    #[test]
    fn links_i64_exit_program_with_recursive_directory_create() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("runtime-dir-all");
        let nested = root.join("deep");
        let object = temp.path().join("i64-exit-dir-all.o");
        let binary = temp.path().join("i64-exit-dir-all");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: vec![I64Expr::MakeDirAll {
                    path: nested.display().to_string(),
                }],
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::Binary {
                    op: I64BinaryOp::Add,
                    lhs: Box::new(I64Expr::Binary {
                        op: I64BinaryOp::Add,
                        lhs: Box::new(I64Expr::Local(0)),
                        rhs: Box::new(I64Expr::RemoveDir {
                            path: nested.display().to_string(),
                        }),
                    }),
                    rhs: Box::new(I64Expr::RemoveDir {
                        path: root.display().to_string(),
                    }),
                }),
            },
            &object,
            &binary,
        )
        .expect("compile i64 recursive directory exit program");

        assert!(
            !nested.exists(),
            "compile should not create the nested directory fixture"
        );
        let output = Command::new(&binary)
            .output()
            .expect("run recursive directory binary");
        assert_eq!(output.status.code(), Some(0));
        assert!(
            !root.exists(),
            "runtime remove_dir should remove the recursive directory fixture"
        );
    }

    #[test]
    fn links_i64_exit_program_with_process_status() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        if !Path::new("/usr/bin/true").exists() || !Path::new("/usr/bin/false").exists() {
            eprintln!(
                "skipping cranelift process-status link test because /usr/bin/true or /usr/bin/false is unavailable"
            );
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let object = temp.path().join("i64-exit-process-status.o");
        let binary = temp.path().join("i64-exit-process-status");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::Binary {
                    op: I64BinaryOp::Add,
                    lhs: Box::new(I64Expr::ProcessStatus {
                        command: String::from("/usr/bin/false"),
                    }),
                    rhs: Box::new(I64Expr::ProcessStatus {
                        command: String::from("__axiom_stage1_missing_binary__"),
                    }),
                }),
            },
            &object,
            &binary,
        )
        .expect("compile i64 process status exit program");
        let output = Command::new(&binary)
            .output()
            .expect("run process status binary");
        assert_eq!(output.status.code(), Some(0));

        let object = temp.path().join("i64-exit-process-true.o");
        let binary = temp.path().join("i64-exit-process-true");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::ProcessStatus {
                    command: String::from("/usr/bin/true"),
                }),
            },
            &object,
            &binary,
        )
        .expect("compile i64 true process status exit program");
        let output = Command::new(&binary)
            .output()
            .expect("run true process status binary");
        assert_eq!(output.status.code(), Some(0));
    }

    #[test]
    fn links_i64_exit_program_with_local() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let object = temp.path().join("i64-exit-local.o");
        let binary = temp.path().join("i64-exit-local");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: vec![
                    I64Expr::Literal(9),
                    I64Expr::Binary {
                        op: I64BinaryOp::Add,
                        lhs: Box::new(I64Expr::Local(0)),
                        rhs: Box::new(I64Expr::Literal(3)),
                    },
                ],
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::Binary {
                    op: I64BinaryOp::Sub,
                    lhs: Box::new(I64Expr::Binary {
                        op: I64BinaryOp::Mul,
                        lhs: Box::new(I64Expr::Local(1)),
                        rhs: Box::new(I64Expr::Literal(4)),
                    }),
                    rhs: Box::new(I64Expr::Literal(6)),
                }),
            },
            &object,
            &binary,
        )
        .expect("compile i64 local exit program");
        let output = Command::new(&binary).output().expect("run binary");
        assert_eq!(output.status.code(), Some(42));
    }

    #[test]
    fn links_i64_exit_program_with_dynamic_integer_stdout() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let object = temp.path().join("i64-dynamic-stdout.o");
        let binary = temp.path().join("i64-dynamic-stdout");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: vec![
                    I64Expr::Literal(40),
                    I64Expr::Binary {
                        op: I64BinaryOp::Add,
                        lhs: Box::new(I64Expr::Local(0)),
                        rhs: Box::new(I64Expr::Literal(2)),
                    },
                ],
                stmts: vec![
                    I64Stmt::WriteI64Line {
                        stream: OutputStream::Stdout,
                        value: I64Expr::Local(1),
                    },
                    I64Stmt::WriteI64Line {
                        stream: OutputStream::Stdout,
                        value: I64Expr::Literal(-3),
                    },
                    I64Stmt::WriteI64Line {
                        stream: OutputStream::Stdout,
                        value: I64Expr::Literal(0),
                    },
                ],
                body: I64ExitBody::Return(I64Expr::Local(1)),
            },
            &object,
            &binary,
        )
        .expect("compile i64 dynamic stdout program");
        let output = Command::new(&binary)
            .output()
            .expect("run i64 dynamic stdout binary");
        assert_eq!(output.status.code(), Some(42));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n-3\n0\n");
    }

    #[test]
    fn links_i64_exit_program_with_branch() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let object = temp.path().join("i64-exit-branch.o");
        let binary = temp.path().join("i64-exit-branch");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: vec![
                    I64Expr::Literal(9),
                    I64Expr::Binary {
                        op: I64BinaryOp::Add,
                        lhs: Box::new(I64Expr::Local(0)),
                        rhs: Box::new(I64Expr::Literal(3)),
                    },
                ],
                stmts: Vec::new(),
                body: I64ExitBody::IfReturn {
                    cond: I64Condition::Compare(I64Compare {
                        op: I64CompareOp::Gt,
                        lhs: I64Expr::Local(1),
                        rhs: I64Expr::Literal(10),
                    }),
                    then_result: I64Expr::Binary {
                        op: I64BinaryOp::Mul,
                        lhs: Box::new(I64Expr::Local(1)),
                        rhs: Box::new(I64Expr::Literal(4)),
                    },
                    else_result: I64Expr::Literal(1),
                },
            },
            &object,
            &binary,
        )
        .expect("compile i64 branch exit program");
        let output = Command::new(&binary).output().expect("run binary");
        assert_eq!(output.status.code(), Some(48));
    }

    #[test]
    fn links_i64_exit_program_with_composed_branch_condition() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let object = temp.path().join("i64-exit-composed-branch.o");
        let binary = temp.path().join("i64-exit-composed-branch");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: vec![
                    I64Expr::Literal(12),
                    I64Expr::Binary {
                        op: I64BinaryOp::Mul,
                        lhs: Box::new(I64Expr::Local(0)),
                        rhs: Box::new(I64Expr::Literal(4)),
                    },
                ],
                stmts: Vec::new(),
                body: I64ExitBody::IfReturn {
                    cond: I64Condition::And {
                        lhs: Box::new(I64Condition::Compare(I64Compare {
                            op: I64CompareOp::Gt,
                            lhs: I64Expr::Local(1),
                            rhs: I64Expr::Literal(40),
                        })),
                        rhs: Box::new(I64Condition::Or {
                            lhs: Box::new(I64Condition::Literal(true)),
                            rhs: Box::new(I64Condition::Compare(I64Compare {
                                op: I64CompareOp::Lt,
                                lhs: I64Expr::Local(0),
                                rhs: I64Expr::Literal(0),
                            })),
                        }),
                    },
                    then_result: I64Expr::Local(1),
                    else_result: I64Expr::Literal(1),
                },
            },
            &object,
            &binary,
        )
        .expect("compile i64 composed branch exit program");
        let output = Command::new(&binary).output().expect("run binary");
        assert_eq!(output.status.code(), Some(48));
    }

    #[test]
    fn links_i64_exit_program_with_function_call() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let object = temp.path().join("i64-exit-call.o");
        let binary = temp.path().join("i64-exit-call");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: vec![I64Function {
                    params: 1,
                    returns: 1,
                    locals: vec![I64Expr::Binary {
                        op: I64BinaryOp::Add,
                        lhs: Box::new(I64Expr::Local(0)),
                        rhs: Box::new(I64Expr::Literal(3)),
                    }],
                    stmts: Vec::new(),
                    body: I64ValueBody::Return(vec![I64Expr::Binary {
                        op: I64BinaryOp::Mul,
                        lhs: Box::new(I64Expr::Local(1)),
                        rhs: Box::new(I64Expr::Literal(4)),
                    }]),
                }],
                locals: vec![I64Expr::Literal(9)],
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::Call {
                    function: 0,
                    args: vec![I64Expr::Local(0)],
                }),
            },
            &object,
            &binary,
        )
        .expect("compile i64 function call exit program");
        let output = Command::new(&binary).output().expect("run binary");
        assert_eq!(output.status.code(), Some(48));
    }

    #[test]
    fn links_i64_exit_program_with_condition_value_argument() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let object = temp.path().join("i64-exit-condition-value.o");
        let binary = temp.path().join("i64-exit-condition-value");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: vec![I64Function {
                    params: 1,
                    returns: 1,
                    locals: Vec::new(),
                    stmts: Vec::new(),
                    body: I64ValueBody::IfReturn {
                        cond: I64Condition::Compare(I64Compare {
                            op: I64CompareOp::Eq,
                            lhs: I64Expr::Local(0),
                            rhs: I64Expr::Literal(1),
                        }),
                        then_results: vec![I64Expr::Literal(48)],
                        else_results: vec![I64Expr::Literal(2)],
                    },
                }],
                locals: vec![I64Expr::Literal(42)],
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::Call {
                    function: 0,
                    args: vec![I64Expr::ConditionValue(Box::new(I64Condition::Compare(
                        I64Compare {
                            op: I64CompareOp::Eq,
                            lhs: I64Expr::Local(0),
                            rhs: I64Expr::Literal(42),
                        },
                    )))],
                }),
            },
            &object,
            &binary,
        )
        .expect("compile i64 condition-value argument program");
        let output = Command::new(&binary)
            .output()
            .expect("run condition-value argument binary");
        assert_eq!(output.status.code(), Some(48));
    }

    #[test]
    fn links_i64_exit_program_with_loop() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let object = temp.path().join("i64-exit-loop.o");
        let binary = temp.path().join("i64-exit-loop");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: vec![
                    I64Expr::Literal(0),
                    I64Expr::Literal(0),
                    I64Expr::Literal(7),
                    I64Expr::Literal(0),
                ],
                stmts: vec![I64Stmt::While {
                    cond: I64Condition::Compare(I64Compare {
                        op: I64CompareOp::Lt,
                        lhs: I64Expr::Local(1),
                        rhs: I64Expr::Local(2),
                    }),
                    body: vec![
                        I64Stmt::Assign(I64Assign {
                            local: 0,
                            value: I64Expr::Binary {
                                op: I64BinaryOp::Add,
                                lhs: Box::new(I64Expr::Local(0)),
                                rhs: Box::new(I64Expr::Local(1)),
                            },
                        }),
                        I64Stmt::Assign(I64Assign {
                            local: 1,
                            value: I64Expr::Binary {
                                op: I64BinaryOp::Add,
                                lhs: Box::new(I64Expr::Local(1)),
                                rhs: Box::new(I64Expr::Literal(1)),
                            },
                        }),
                        I64Stmt::If {
                            cond: I64Condition::Or {
                                lhs: Box::new(I64Condition::Compare(I64Compare {
                                    op: I64CompareOp::Ne,
                                    lhs: I64Expr::Local(3),
                                    rhs: I64Expr::Literal(0),
                                })),
                                rhs: Box::new(I64Condition::Compare(I64Compare {
                                    op: I64CompareOp::Eq,
                                    lhs: I64Expr::Local(1),
                                    rhs: I64Expr::Literal(4),
                                })),
                            },
                            then_body: vec![I64Stmt::Assign(I64Assign {
                                local: 3,
                                value: I64Expr::Literal(1),
                            })],
                            else_body: Vec::new(),
                        },
                    ],
                }],
                body: I64ExitBody::IfReturn {
                    cond: I64Condition::Compare(I64Compare {
                        op: I64CompareOp::Ne,
                        lhs: I64Expr::Local(3),
                        rhs: I64Expr::Literal(0),
                    }),
                    then_result: I64Expr::Local(0),
                    else_result: I64Expr::Literal(1),
                },
            },
            &object,
            &binary,
        )
        .expect("compile i64 loop exit program");
        let output = Command::new(&binary).output().expect("run binary");
        assert_eq!(output.status.code(), Some(21));
    }

    #[test]
    fn links_i64_exit_program_with_division() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let object = temp.path().join("i64-exit-div.o");
        let binary = temp.path().join("i64-exit-div");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::Binary {
                    op: I64BinaryOp::Div,
                    lhs: Box::new(I64Expr::Literal(84)),
                    rhs: Box::new(I64Expr::Literal(2)),
                }),
            },
            &object,
            &binary,
        )
        .expect("compile i64 division exit program");
        let output = Command::new(&binary).output().expect("run binary");
        assert_eq!(output.status.code(), Some(42));
    }

    #[test]
    fn clock_now_ms_imports_timespec_get_without_time_fallback() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        if Command::new("nm").arg("--version").output().is_err()
            && Command::new("nm").arg("-V").output().is_err()
        {
            eprintln!("skipping cranelift symbol test because nm is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let object = temp.path().join("i64-exit-clock-now.o");
        let binary = temp.path().join("i64-exit-clock-now");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::ClockNowMs),
            },
            &object,
            &binary,
        )
        .expect("compile i64 clock now exit program");

        let output = Command::new("nm")
            .arg("-u")
            .arg(&object)
            .output()
            .expect("inspect clock object symbols");
        assert!(
            output.status.success(),
            "nm failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let symbols = String::from_utf8_lossy(&output.stdout);
        let imported_symbols = symbols
            .lines()
            .filter_map(|line| line.split_whitespace().last())
            .collect::<Vec<_>>();
        assert!(
            imported_symbols
                .iter()
                .any(|symbol| *symbol == "timespec_get" || *symbol == "_timespec_get"),
            "clock object should import timespec_get, got:\n{symbols}"
        );
        assert!(
            !imported_symbols
                .iter()
                .any(|symbol| *symbol == "time" || *symbol == "_time"),
            "clock object should not import the second-resolution host clock symbol, got:\n{symbols}"
        );
    }

    #[test]
    fn clock_now_ms_reads_timespec_nanoseconds_at_lowering_boundary() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let object = temp.path().join("i64-exit-clock-now-shim.o");
        let binary = temp.path().join("i64-exit-clock-now-shim");
        let shim = temp.path().join("timespec_get_shim.c");
        fs::write(
            &shim,
            r#"#include <time.h>

#ifndef TIME_UTC
#define TIME_UTC 1
#endif

int timespec_get(struct timespec *ts, int base) {
    if (base != TIME_UTC) {
        return 0;
    }
    ts->tv_sec = 7;
    ts->tv_nsec = 456000000L;
    return TIME_UTC;
}
"#,
        )
        .expect("write deterministic timespec_get shim");
        emit_i64_exit_object(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: Vec::new(),
                body: I64ExitBody::IfReturn {
                    cond: I64Condition::Compare(I64Compare {
                        op: I64CompareOp::Eq,
                        lhs: I64Expr::ClockNowMs,
                        rhs: I64Expr::Literal(7_456),
                    }),
                    then_result: I64Expr::Literal(48),
                    else_result: I64Expr::Literal(1),
                },
            },
            &object,
        )
        .expect("emit i64 clock now exit object");
        let link = Command::new("cc")
            .arg(&object)
            .arg(&shim)
            .arg("-o")
            .arg(&binary)
            .output()
            .expect("link clock shim binary");
        assert!(
            link.status.success(),
            "cc failed: stdout={} stderr={}",
            String::from_utf8_lossy(&link.stdout),
            String::from_utf8_lossy(&link.stderr)
        );
        let output = Command::new(&binary)
            .output()
            .expect("run clock shim binary");
        assert_eq!(output.status.code(), Some(48));
    }

    #[test]
    fn clock_now_ms_tracks_subsecond_elapsed_after_sleep() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let object = temp.path().join("i64-exit-clock-precision.o");
        let binary = temp.path().join("i64-exit-clock-precision");
        let elapsed_ms = || I64Expr::ClockElapsedMs {
            start: Box::new(I64Expr::Local(0)),
        };
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: vec![I64Expr::ClockNowMs, I64Expr::Literal(-1)],
                stmts: vec![I64Stmt::Assign(I64Assign {
                    local: 1,
                    value: I64Expr::SleepMs {
                        milliseconds: Box::new(I64Expr::Literal(10)),
                    },
                })],
                body: I64ExitBody::IfReturn {
                    cond: I64Condition::And {
                        lhs: Box::new(I64Condition::Compare(I64Compare {
                            op: I64CompareOp::Eq,
                            lhs: I64Expr::Local(1),
                            rhs: I64Expr::Literal(0),
                        })),
                        rhs: Box::new(I64Condition::And {
                            lhs: Box::new(I64Condition::Compare(I64Compare {
                                op: I64CompareOp::Gt,
                                lhs: elapsed_ms(),
                                rhs: I64Expr::Literal(0),
                            })),
                            rhs: Box::new(I64Condition::Compare(I64Compare {
                                op: I64CompareOp::Lt,
                                lhs: elapsed_ms(),
                                rhs: I64Expr::Literal(1_000),
                            })),
                        }),
                    },
                    then_result: I64Expr::Literal(48),
                    else_result: I64Expr::Literal(1),
                },
            },
            &object,
            &binary,
        )
        .expect("compile i64 clock precision exit program");
        let output = Command::new(&binary)
            .output()
            .expect("run clock precision binary");
        assert_eq!(output.status.code(), Some(48));
    }

    #[test]
    fn links_i64_exit_program_short_circuits_boolean_conditions() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");

        let and_object = temp.path().join("i64-exit-short-circuit-and.o");
        let and_binary = temp.path().join("i64-exit-short-circuit-and");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: Vec::new(),
                body: I64ExitBody::IfReturn {
                    cond: I64Condition::And {
                        lhs: Box::new(I64Condition::Literal(false)),
                        rhs: Box::new(i64_divide_by_zero_is_zero_condition()),
                    },
                    then_result: I64Expr::Literal(1),
                    else_result: I64Expr::Literal(48),
                },
            },
            &and_object,
            &and_binary,
        )
        .expect("compile short-circuit and exit program");
        let output = Command::new(&and_binary)
            .output()
            .expect("run short-circuit and binary");
        assert_eq!(output.status.code(), Some(48));

        let or_object = temp.path().join("i64-exit-short-circuit-or.o");
        let or_binary = temp.path().join("i64-exit-short-circuit-or");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: Vec::new(),
                body: I64ExitBody::IfReturn {
                    cond: I64Condition::Or {
                        lhs: Box::new(I64Condition::Literal(true)),
                        rhs: Box::new(i64_divide_by_zero_is_zero_condition()),
                    },
                    then_result: I64Expr::Literal(48),
                    else_result: I64Expr::Literal(1),
                },
            },
            &or_object,
            &or_binary,
        )
        .expect("compile short-circuit or exit program");
        let output = Command::new(&or_binary)
            .output()
            .expect("run short-circuit or binary");
        assert_eq!(output.status.code(), Some(48));
    }

    #[test]
    fn links_i64_exit_program_selects_only_chosen_arm() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");

        let then_object = temp.path().join("i64-exit-select-then.o");
        let then_binary = temp.path().join("i64-exit-select-then");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::Select {
                    cond: Box::new(I64Condition::Literal(true)),
                    then_result: Box::new(I64Expr::Literal(48)),
                    else_result: Box::new(i64_divide_by_zero_expr()),
                }),
            },
            &then_object,
            &then_binary,
        )
        .expect("compile selected then-arm exit program");
        let output = Command::new(&then_binary)
            .output()
            .expect("run selected then-arm binary");
        assert_eq!(output.status.code(), Some(48));

        let else_object = temp.path().join("i64-exit-select-else.o");
        let else_binary = temp.path().join("i64-exit-select-else");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: Vec::new(),
                body: I64ExitBody::Return(I64Expr::Select {
                    cond: Box::new(I64Condition::Literal(false)),
                    then_result: Box::new(i64_divide_by_zero_expr()),
                    else_result: Box::new(I64Expr::Literal(48)),
                }),
            },
            &else_object,
            &else_binary,
        )
        .expect("compile selected else-arm exit program");
        let output = Command::new(&else_binary)
            .output()
            .expect("run selected else-arm binary");
        assert_eq!(output.status.code(), Some(48));
    }
}
