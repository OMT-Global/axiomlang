use cranelift_codegen::ir::{AbiParam, InstBuilder, types};
use cranelift_codegen::isa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataDescription, Linkage, Module, default_libcall_names};
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
    emit_cranelift_object(lines, object_path)?;
    link_object(object_path, binary_path)
}

fn emit_cranelift_object(
    lines: &[String],
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

    let mut puts_sig = module.make_signature();
    puts_sig.params.push(AbiParam::new(pointer_type));
    puts_sig.returns.push(AbiParam::new(types::I32));
    let puts_id = module
        .declare_function("puts", Linkage::Import, &puts_sig)
        .map_err(|message| CraneliftBackendError::new(format!("declare puts import: {message}")))?;

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
        let mut bytes = line.as_bytes().to_vec();
        bytes.push(0);
        description.define(bytes.into_boxed_slice());
        module
            .define_data(data_id, &description)
            .map_err(|message| CraneliftBackendError::new(format!("define data: {message}")))?;
        data_ids.push(data_id);
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
        let puts_ref = module.declare_func_in_func(puts_id, builder.func);
        for data_id in data_ids {
            let data_ref = module.declare_data_in_func(data_id, builder.func);
            let pointer = builder.ins().global_value(pointer_type, data_ref);
            builder.ins().call(puts_ref, &[pointer]);
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
}
