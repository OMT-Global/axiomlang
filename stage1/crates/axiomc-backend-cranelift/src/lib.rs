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
use std::path::Path;
use std::process::Command;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I64BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    UDiv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I64CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Ult,
    Ule,
    Ugt,
    Uge,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum I64Expr {
    Literal(i64),
    Local(usize),
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
    WriteIntLine {
        stream: OutputStream,
        value: I64Expr,
    },
    WriteUIntLine {
        stream: OutputStream,
        value: I64Expr,
    },
    WriteJsonStringifiedIntLine {
        stream: OutputStream,
        value: I64Expr,
    },
    WriteJsonFieldIntLine {
        stream: OutputStream,
        prefix: String,
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
    fs::write(object_path, bytes).map_err(|err| {
        CraneliftBackendError::new(format!("failed to write {}: {err}", object_path.display()))
    })
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
    let output_data_ids = declare_i64_output_data(&mut module, &program)?;
    let function_ids = declare_i64_functions(&mut module, &program.functions)?;

    for (index, function) in program.functions.iter().enumerate() {
        define_i64_function(
            &mut module,
            &function_ids,
            write_id,
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
        let mut locals = Vec::new();
        for local_expr in &program.locals {
            let local = builder.declare_var(types::I64);
            let value = emit_i64_expr(&mut builder, &locals, &function_refs, local_expr)?;
            builder.def_var(local, value);
            locals.push(local);
        }
        emit_i64_stmts(
            &mut module,
            &mut builder,
            &locals,
            &function_refs,
            write_ref,
            &output_data_ids,
            &program.stmts,
        )?;
        emit_i64_exit_body(
            &mut module,
            &mut builder,
            &locals,
            &function_refs,
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
    fs::write(object_path, bytes).map_err(|err| {
        CraneliftBackendError::new(format!("failed to write {}: {err}", object_path.display()))
    })
}

fn declare_i64_output_data(
    module: &mut ObjectModule,
    program: &I64ExitProgram,
) -> Result<Vec<(OutputStream, String, bool, cranelift_module::DataId, usize)>, CraneliftBackendError>
{
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
            Ok((stream, text, append_newline, data_id, byte_len))
        })
        .collect()
}

fn collect_i64_output_lines(stmts: &[I64Stmt], lines: &mut Vec<(OutputStream, String, bool)>) {
    for stmt in stmts {
        match stmt {
            I64Stmt::WriteText { stream, text } => lines.push((*stream, text.clone(), false)),
            I64Stmt::WriteLine { stream, text } => lines.push((*stream, text.clone(), true)),
            I64Stmt::WriteJsonFieldIntLine { stream, prefix, .. } => {
                lines.push((*stream, prefix.clone(), false))
            }
            I64Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                collect_i64_output_lines(then_body, lines);
                collect_i64_output_lines(else_body, lines);
            }
            I64Stmt::While { body, .. } => collect_i64_output_lines(body, lines),
            I64Stmt::Assign(_) | I64Stmt::WriteIntLine { .. } | I64Stmt::CallAssign { .. } => {}
            I64Stmt::WriteUIntLine { .. } | I64Stmt::WriteJsonStringifiedIntLine { .. } => {}
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
    output_data_ids: &[(OutputStream, String, bool, cranelift_module::DataId, usize)],
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
        let mut locals = Vec::new();
        for param in builder.block_params(block).to_vec() {
            let local = builder.declare_var(types::I64);
            builder.def_var(local, param);
            locals.push(local);
        }
        for local_expr in &function.locals {
            let local = builder.declare_var(types::I64);
            let value = emit_i64_expr(&mut builder, &locals, &function_refs, local_expr)?;
            builder.def_var(local, value);
            locals.push(local);
        }
        emit_i64_stmts(
            module,
            &mut builder,
            &locals,
            &function_refs,
            write_ref,
            output_data_ids,
            &function.stmts,
        )?;
        emit_i64_value_body(
            module,
            &mut builder,
            &locals,
            &function_refs,
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
    write_ref: FuncRef,
    output_data_ids: &[(OutputStream, String, bool, cranelift_module::DataId, usize)],
    stmts: &[I64Stmt],
) -> Result<(), CraneliftBackendError> {
    for stmt in stmts {
        emit_i64_stmt(
            module,
            builder,
            locals,
            function_refs,
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
    write_ref: FuncRef,
    output_data_ids: &[(OutputStream, String, bool, cranelift_module::DataId, usize)],
    stmt: &I64Stmt,
) -> Result<(), CraneliftBackendError> {
    match stmt {
        I64Stmt::Assign(assign) => emit_i64_assign(builder, locals, function_refs, assign),
        I64Stmt::WriteText { stream, text } => emit_i64_write_static(
            module,
            builder,
            write_ref,
            output_data_ids,
            *stream,
            text,
            false,
        ),
        I64Stmt::WriteLine { stream, text } => emit_i64_write_static(
            module,
            builder,
            write_ref,
            output_data_ids,
            *stream,
            text,
            true,
        ),
        I64Stmt::WriteIntLine { stream, value } => emit_i64_write_int_line(
            module,
            builder,
            locals,
            function_refs,
            write_ref,
            *stream,
            value,
        ),
        I64Stmt::WriteUIntLine { stream, value } => emit_i64_write_uint_line(
            module,
            builder,
            locals,
            function_refs,
            write_ref,
            *stream,
            value,
        ),
        I64Stmt::WriteJsonStringifiedIntLine { stream, value } => {
            emit_i64_write_json_stringified_int_line(
                module,
                builder,
                locals,
                function_refs,
                write_ref,
                *stream,
                value,
            )
        }
        I64Stmt::WriteJsonFieldIntLine {
            stream,
            prefix,
            value,
        } => {
            emit_i64_write_static(
                module,
                builder,
                write_ref,
                output_data_ids,
                *stream,
                prefix,
                false,
            )?;
            emit_i64_write_int_line(
                module,
                builder,
                locals,
                function_refs,
                write_ref,
                *stream,
                value,
            )
        }
        I64Stmt::CallAssign {
            locals: assign_locals,
            function,
            args,
        } => emit_i64_call_assign(
            builder,
            locals,
            function_refs,
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
            let condition = emit_i64_condition(builder, locals, function_refs, cond)?;
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
            let condition = emit_i64_condition(builder, locals, function_refs, cond)?;
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

fn emit_i64_write_static(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    write_ref: FuncRef,
    output_data_ids: &[(OutputStream, String, bool, cranelift_module::DataId, usize)],
    stream: OutputStream,
    text: &str,
    append_newline: bool,
) -> Result<(), CraneliftBackendError> {
    let (data_id, byte_len) = output_data_ids
        .iter()
        .find_map(
            |(candidate_stream, candidate_text, candidate_append_newline, data_id, byte_len)| {
                (*candidate_stream == stream
                    && candidate_text == text
                    && *candidate_append_newline == append_newline)
                    .then_some((data_id, byte_len))
            },
        )
        .ok_or_else(|| CraneliftBackendError::new("missing i64 output static data"))?;
    let data_ref = module.declare_data_in_func(*data_id, builder.func);
    let pointer_type = module.target_config().pointer_type();
    let pointer = builder.ins().global_value(pointer_type, data_ref);
    let fd = builder.ins().iconst(
        types::I32,
        match stream {
            OutputStream::Stdout => 1,
            OutputStream::Stderr => 2,
        },
    );
    let len = builder.ins().iconst(pointer_type, *byte_len as i64);
    builder.ins().call(write_ref, &[fd, pointer, len]);
    Ok(())
}

fn emit_i64_write_int_line(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    write_ref: FuncRef,
    stream: OutputStream,
    value: &I64Expr,
) -> Result<(), CraneliftBackendError> {
    emit_i64_write_decimal_line(
        module,
        builder,
        locals,
        function_refs,
        write_ref,
        stream,
        value,
        true,
        false,
    )
}

fn emit_i64_write_uint_line(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    write_ref: FuncRef,
    stream: OutputStream,
    value: &I64Expr,
) -> Result<(), CraneliftBackendError> {
    emit_i64_write_decimal_line(
        module,
        builder,
        locals,
        function_refs,
        write_ref,
        stream,
        value,
        false,
        false,
    )
}

fn emit_i64_write_json_stringified_int_line(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    write_ref: FuncRef,
    stream: OutputStream,
    value: &I64Expr,
) -> Result<(), CraneliftBackendError> {
    emit_i64_write_decimal_line(
        module,
        builder,
        locals,
        function_refs,
        write_ref,
        stream,
        value,
        true,
        true,
    )
}

fn emit_i64_write_decimal_line(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    write_ref: FuncRef,
    stream: OutputStream,
    value: &I64Expr,
    signed: bool,
    json_stringified: bool,
) -> Result<(), CraneliftBackendError> {
    let pointer_type = module.target_config().pointer_type();
    let value = emit_i64_expr(builder, locals, function_refs, value)?;
    let buffer_len = if json_stringified { 34 } else { 32 };
    let buffer = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        buffer_len,
        0,
    ));
    let newline = builder.ins().iconst(types::I8, i64::from(b'\n'));
    builder
        .ins()
        .stack_store(newline, buffer, (buffer_len - 1) as i32);
    if json_stringified {
        let quote = builder.ins().iconst(types::I8, i64::from(b'"'));
        builder
            .ins()
            .stack_store(quote, buffer, (buffer_len - 2) as i32);
    }

    let zero = builder.ins().iconst(types::I64, 0);
    let ten = builder.ins().iconst(types::I64, 10);
    let (magnitude, is_negative) = if signed {
        let is_negative = builder.ins().icmp(IntCC::SignedLessThan, value, zero);
        let negated = builder.ins().isub(zero, value);
        (
            builder.ins().select(is_negative, negated, value),
            Some(is_negative),
        )
    } else {
        (value, None)
    };

    let loop_block = builder.create_block();
    let after_digits = builder.create_block();
    let write_block = builder.create_block();
    builder.append_block_param(loop_block, types::I64);
    builder.append_block_param(loop_block, pointer_type);
    builder.append_block_param(after_digits, pointer_type);
    builder.append_block_param(write_block, pointer_type);

    let initial_pos = builder.ins().iconst(pointer_type, (buffer_len - 1) as i64);
    let initial_pos = if json_stringified {
        builder.ins().iadd_imm(initial_pos, -1)
    } else {
        initial_pos
    };
    builder.ins().jump(
        loop_block,
        &[BlockArg::Value(magnitude), BlockArg::Value(initial_pos)],
    );

    builder.switch_to_block(loop_block);
    let current = builder.block_params(loop_block)[0];
    let pos = builder.block_params(loop_block)[1];
    let remainder = builder.ins().urem(current, ten);
    let quotient = builder.ins().udiv(current, ten);
    let digit = builder.ins().iadd_imm(remainder, i64::from(b'0'));
    let digit = builder.ins().ireduce(types::I8, digit);
    let next_pos = builder.ins().iadd_imm(pos, -1);
    let base = builder.ins().stack_addr(pointer_type, buffer, 0);
    let digit_addr = builder.ins().iadd(base, next_pos);
    builder.ins().store(MemFlags::new(), digit, digit_addr, 0);
    let has_more_digits = builder.ins().icmp_imm(IntCC::NotEqual, quotient, 0);
    builder.ins().brif(
        has_more_digits,
        loop_block,
        &[BlockArg::Value(quotient), BlockArg::Value(next_pos)],
        after_digits,
        &[BlockArg::Value(next_pos)],
    );
    builder.seal_block(loop_block);

    builder.switch_to_block(after_digits);
    let pos = builder.block_params(after_digits)[0];
    if let Some(is_negative) = is_negative {
        let negative_block = builder.create_block();
        let positive_block = builder.create_block();
        builder
            .ins()
            .brif(is_negative, negative_block, &[], positive_block, &[]);
        builder.seal_block(after_digits);

        builder.switch_to_block(negative_block);
        let signed_pos = builder.ins().iadd_imm(pos, -1);
        let base = builder.ins().stack_addr(pointer_type, buffer, 0);
        let sign_addr = builder.ins().iadd(base, signed_pos);
        let sign = builder.ins().iconst(types::I8, i64::from(b'-'));
        builder.ins().store(MemFlags::new(), sign, sign_addr, 0);
        builder
            .ins()
            .jump(write_block, &[BlockArg::Value(signed_pos)]);
        builder.seal_block(negative_block);

        builder.switch_to_block(positive_block);
        builder.ins().jump(write_block, &[BlockArg::Value(pos)]);
        builder.seal_block(positive_block);
    } else {
        builder.ins().jump(write_block, &[BlockArg::Value(pos)]);
        builder.seal_block(after_digits);
    }

    builder.switch_to_block(write_block);
    let start = builder.block_params(write_block)[0];
    let base = builder.ins().stack_addr(pointer_type, buffer, 0);
    let start = if json_stringified {
        let quoted_start = builder.ins().iadd_imm(start, -1);
        let quote_addr = builder.ins().iadd(base, quoted_start);
        let quote = builder.ins().iconst(types::I8, i64::from(b'"'));
        builder.ins().store(MemFlags::new(), quote, quote_addr, 0);
        quoted_start
    } else {
        start
    };
    let pointer = builder.ins().iadd(base, start);
    let end = builder.ins().iconst(pointer_type, buffer_len as i64);
    let len = builder.ins().isub(end, start);
    let fd = builder.ins().iconst(
        types::I32,
        match stream {
            OutputStream::Stdout => 1,
            OutputStream::Stderr => 2,
        },
    );
    builder.ins().call(write_ref, &[fd, pointer, len]);
    builder.seal_block(write_block);
    Ok(())
}

fn emit_i64_assign(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    assign: &I64Assign,
) -> Result<(), CraneliftBackendError> {
    let local = locals.get(assign.local).copied().ok_or_else(|| {
        CraneliftBackendError::new(format!(
            "i64 assignment local {} is out of range",
            assign.local
        ))
    })?;
    let value = emit_i64_expr(builder, locals, function_refs, &assign.value)?;
    builder.def_var(local, value);
    Ok(())
}

fn emit_i64_call_assign(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    assign_locals: &[usize],
    function: usize,
    args: &[I64Expr],
) -> Result<(), CraneliftBackendError> {
    let function_ref = function_refs.get(function).copied().ok_or_else(|| {
        CraneliftBackendError::new(format!("i64 function index {function} is out of range"))
    })?;
    let args = args
        .iter()
        .map(|arg| emit_i64_expr(builder, locals, function_refs, arg))
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
    write_ref: FuncRef,
    output_data_ids: &[(OutputStream, String, bool, cranelift_module::DataId, usize)],
    body: &I64ExitBody,
) -> Result<(), CraneliftBackendError> {
    match body {
        I64ExitBody::Return(result) => emit_i64_return(builder, locals, function_refs, result),
        I64ExitBody::BlockReturn(block) => {
            emit_i64_stmts(
                module,
                builder,
                locals,
                function_refs,
                write_ref,
                output_data_ids,
                &block.stmts,
            )?;
            emit_i64_return(builder, locals, function_refs, &block.result)
        }
        I64ExitBody::IfReturn {
            cond,
            then_result,
            else_result,
        } => {
            let then_block = builder.create_block();
            let else_block = builder.create_block();
            let condition = emit_i64_condition(builder, locals, function_refs, cond)?;
            builder
                .ins()
                .brif(condition, then_block, &[], else_block, &[]);

            builder.switch_to_block(then_block);
            builder.seal_block(then_block);
            emit_i64_return(builder, locals, function_refs, then_result)?;

            builder.switch_to_block(else_block);
            builder.seal_block(else_block);
            emit_i64_return(builder, locals, function_refs, else_result)
        }
        I64ExitBody::IfBlockReturn {
            cond,
            then_block,
            else_block,
        } => {
            let then_cranelift_block = builder.create_block();
            let else_cranelift_block = builder.create_block();
            let condition = emit_i64_condition(builder, locals, function_refs, cond)?;
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
                write_ref,
                output_data_ids,
                &then_block.stmts,
            )?;
            emit_i64_return(builder, locals, function_refs, &then_block.result)?;

            builder.switch_to_block(else_cranelift_block);
            builder.seal_block(else_cranelift_block);
            emit_i64_stmts(
                module,
                builder,
                locals,
                function_refs,
                write_ref,
                output_data_ids,
                &else_block.stmts,
            )?;
            emit_i64_return(builder, locals, function_refs, &else_block.result)
        }
    }
}

fn emit_i64_value_body(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    write_ref: FuncRef,
    output_data_ids: &[(OutputStream, String, bool, cranelift_module::DataId, usize)],
    returns: usize,
    body: &I64ValueBody,
) -> Result<(), CraneliftBackendError> {
    match body {
        I64ValueBody::Return(results) => {
            emit_i64_value_return(builder, locals, function_refs, returns, results)
        }
        I64ValueBody::BlockReturn(block) => {
            emit_i64_stmts(
                module,
                builder,
                locals,
                function_refs,
                write_ref,
                output_data_ids,
                &block.stmts,
            )?;
            emit_i64_value_return(builder, locals, function_refs, returns, &block.results)
        }
        I64ValueBody::IfReturn {
            cond,
            then_results,
            else_results,
        } => {
            let then_block = builder.create_block();
            let else_block = builder.create_block();
            let condition = emit_i64_condition(builder, locals, function_refs, cond)?;
            builder
                .ins()
                .brif(condition, then_block, &[], else_block, &[]);

            builder.switch_to_block(then_block);
            builder.seal_block(then_block);
            emit_i64_value_return(builder, locals, function_refs, returns, then_results)?;

            builder.switch_to_block(else_block);
            builder.seal_block(else_block);
            emit_i64_value_return(builder, locals, function_refs, returns, else_results)
        }
        I64ValueBody::IfBlockReturn {
            cond,
            then_block,
            else_block,
        } => {
            let then_cranelift_block = builder.create_block();
            let else_cranelift_block = builder.create_block();
            let condition = emit_i64_condition(builder, locals, function_refs, cond)?;
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
                write_ref,
                output_data_ids,
                &then_block.stmts,
            )?;
            emit_i64_value_return(builder, locals, function_refs, returns, &then_block.results)?;

            builder.switch_to_block(else_cranelift_block);
            builder.seal_block(else_cranelift_block);
            emit_i64_stmts(
                module,
                builder,
                locals,
                function_refs,
                write_ref,
                output_data_ids,
                &else_block.stmts,
            )?;
            emit_i64_value_return(builder, locals, function_refs, returns, &else_block.results)
        }
    }
}

fn emit_i64_return(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    result: &I64Expr,
) -> Result<(), CraneliftBackendError> {
    let result = emit_i64_expr(builder, locals, function_refs, result)?;
    let exit_code = builder.ins().ireduce(types::I32, result);
    builder.ins().return_(&[exit_code]);
    Ok(())
}

fn emit_i64_value_return(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
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
        .map(|result| emit_i64_expr(builder, locals, function_refs, result))
        .collect::<Result<Vec<_>, _>>()?;
    builder.ins().return_(&results);
    Ok(())
}

fn emit_i64_compare(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    cond: &I64Compare,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let lhs = emit_i64_expr(builder, locals, function_refs, &cond.lhs)?;
    let rhs = emit_i64_expr(builder, locals, function_refs, &cond.rhs)?;
    Ok(builder.ins().icmp(i64_compare_op(cond.op), lhs, rhs))
}

fn emit_i64_condition(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    cond: &I64Condition,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    match cond {
        I64Condition::Literal(value) => {
            let value = builder.ins().iconst(types::I8, i64::from(*value));
            Ok(builder.ins().icmp_imm(IntCC::NotEqual, value, 0))
        }
        I64Condition::Compare(compare) => emit_i64_compare(builder, locals, function_refs, compare),
        I64Condition::And { lhs, rhs } => {
            emit_i64_short_circuit_condition(builder, locals, function_refs, lhs, rhs, false)
        }
        I64Condition::Or { lhs, rhs } => {
            emit_i64_short_circuit_condition(builder, locals, function_refs, lhs, rhs, true)
        }
    }
}

fn emit_i64_short_circuit_condition(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
    lhs: &I64Condition,
    rhs: &I64Condition,
    short_circuit_value: bool,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let lhs = emit_i64_condition(builder, locals, function_refs, lhs)?;
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
    let rhs = emit_i64_condition(builder, locals, function_refs, rhs)?;
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
        I64CompareOp::Ult => IntCC::UnsignedLessThan,
        I64CompareOp::Ule => IntCC::UnsignedLessThanOrEqual,
        I64CompareOp::Ugt => IntCC::UnsignedGreaterThan,
        I64CompareOp::Uge => IntCC::UnsignedGreaterThanOrEqual,
    }
}

fn emit_i64_expr(
    builder: &mut FunctionBuilder<'_>,
    locals: &[Variable],
    function_refs: &[FuncRef],
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
        I64Expr::ConditionValue(cond) => {
            let cond = emit_i64_condition(builder, locals, function_refs, cond)?;
            Ok(emit_i64_bool_value(builder, cond))
        }
        I64Expr::Cast { cast, expr } => {
            let value = emit_i64_expr(builder, locals, function_refs, expr)?;
            Ok(emit_i64_cast(builder, value, *cast))
        }
        I64Expr::Call { function, args } => {
            let function_ref = function_refs.get(*function).copied().ok_or_else(|| {
                CraneliftBackendError::new(format!("i64 function index {function} is out of range"))
            })?;
            let args = args
                .iter()
                .map(|arg| emit_i64_expr(builder, locals, function_refs, arg))
                .collect::<Result<Vec<_>, _>>()?;
            let call = builder.ins().call(function_ref, &args);
            let results = builder.inst_results(call);
            Ok(results[0])
        }
        I64Expr::Binary { op, lhs, rhs } => {
            let lhs = emit_i64_expr(builder, locals, function_refs, lhs)?;
            let rhs = emit_i64_expr(builder, locals, function_refs, rhs)?;
            Ok(match op {
                I64BinaryOp::Add => builder.ins().iadd(lhs, rhs),
                I64BinaryOp::Sub => builder.ins().isub(lhs, rhs),
                I64BinaryOp::Mul => builder.ins().imul(lhs, rhs),
                I64BinaryOp::Div => builder.ins().sdiv(lhs, rhs),
                I64BinaryOp::UDiv => builder.ins().udiv(lhs, rhs),
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
    cond: &I64Condition,
    then_result: &I64Expr,
    else_result: &I64Expr,
) -> Result<cranelift_codegen::ir::Value, CraneliftBackendError> {
    let cond = emit_i64_condition(builder, locals, function_refs, cond)?;
    let then_block = builder.create_block();
    let else_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);
    builder.ins().brif(cond, then_block, &[], else_block, &[]);

    builder.switch_to_block(then_block);
    builder.seal_block(then_block);
    let then_result = emit_i64_expr(builder, locals, function_refs, then_result)?;
    let then_args = [BlockArg::Value(then_result)];
    builder.ins().jump(merge_block, &then_args);

    builder.switch_to_block(else_block);
    builder.seal_block(else_block);
    let else_result = emit_i64_expr(builder, locals, function_refs, else_result)?;
    let else_args = [BlockArg::Value(else_result)];
    builder.ins().jump(merge_block, &else_args);

    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    Ok(builder.block_params(merge_block)[0])
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
    let mut command = Command::new("cc");
    let output = command
        .arg(object_path)
        .arg("-o")
        .arg(binary_path)
        .output()
        .map_err(|err| {
            CraneliftBackendError::new(format!("failed to invoke system linker `cc`: {err}"))
        })?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(CraneliftBackendError::new(format!(
        "system linker `cc` failed for cranelift object: {}",
        stderr.trim()
    )))
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
    fn links_i64_dynamic_int_lines() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let object = temp.path().join("i64-int-lines.o");
        let binary = temp.path().join("i64-int-lines");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: vec![
                    I64Stmt::WriteIntLine {
                        stream: OutputStream::Stdout,
                        value: I64Expr::Literal(0),
                    },
                    I64Stmt::WriteIntLine {
                        stream: OutputStream::Stdout,
                        value: I64Expr::Literal(42),
                    },
                    I64Stmt::WriteIntLine {
                        stream: OutputStream::Stdout,
                        value: I64Expr::Literal(-42),
                    },
                    I64Stmt::WriteUIntLine {
                        stream: OutputStream::Stdout,
                        value: I64Expr::Literal(-1),
                    },
                    I64Stmt::WriteUIntLine {
                        stream: OutputStream::Stdout,
                        value: I64Expr::Literal(i64::MIN),
                    },
                    I64Stmt::WriteIntLine {
                        stream: OutputStream::Stderr,
                        value: I64Expr::Literal(i64::MIN),
                    },
                ],
                body: I64ExitBody::Return(I64Expr::Literal(7)),
            },
            &object,
            &binary,
        )
        .expect("compile i64 dynamic int lines");
        let output = Command::new(&binary).output().expect("run binary");
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "0\n42\n-42\n18446744073709551615\n9223372036854775808\n"
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "-9223372036854775808\n"
        );
    }

    #[test]
    fn links_i64_json_stringified_int_lines() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let object = temp.path().join("i64-json-stringified-int-lines.o");
        let binary = temp.path().join("i64-json-stringified-int-lines");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: vec![
                    I64Stmt::WriteJsonStringifiedIntLine {
                        stream: OutputStream::Stdout,
                        value: I64Expr::Literal(42),
                    },
                    I64Stmt::WriteJsonStringifiedIntLine {
                        stream: OutputStream::Stdout,
                        value: I64Expr::Literal(-42),
                    },
                    I64Stmt::WriteJsonStringifiedIntLine {
                        stream: OutputStream::Stderr,
                        value: I64Expr::Literal(i64::MIN),
                    },
                ],
                body: I64ExitBody::Return(I64Expr::Literal(7)),
            },
            &object,
            &binary,
        )
        .expect("compile i64 JSON-stringified int lines");
        let output = Command::new(&binary).output().expect("run binary");
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "\"42\"\n\"-42\"\n");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "\"-9223372036854775808\"\n"
        );
    }

    #[test]
    fn links_i64_json_field_int_lines() {
        if std::env::var_os("AXIOM_SKIP_CRANELIFT_LINK_TEST").is_some() {
            return;
        }
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping cranelift link test because cc is unavailable");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let object = temp.path().join("i64-json-field-int-lines.o");
        let binary = temp.path().join("i64-json-field-int-lines");
        compile_i64_exit_program(
            I64ExitProgram {
                functions: Vec::new(),
                locals: Vec::new(),
                stmts: vec![
                    I64Stmt::WriteJsonFieldIntLine {
                        stream: OutputStream::Stdout,
                        prefix: "\"count\":".to_string(),
                        value: I64Expr::Literal(42),
                    },
                    I64Stmt::WriteJsonFieldIntLine {
                        stream: OutputStream::Stderr,
                        prefix: "\"delta\":".to_string(),
                        value: I64Expr::Literal(-42),
                    },
                ],
                body: I64ExitBody::Return(I64Expr::Literal(7)),
            },
            &object,
            &binary,
        )
        .expect("compile i64 JSON field int lines");
        let output = Command::new(&binary).output().expect("run binary");
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "\"count\":42\n");
        assert_eq!(String::from_utf8_lossy(&output.stderr), "\"delta\":-42\n");
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
