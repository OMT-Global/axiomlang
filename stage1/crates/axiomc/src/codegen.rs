use crate::diagnostics::Diagnostic;
use crate::manifest::CapabilityConfig;
use crate::mir::{
    EnumDef, Expr, Function, LiteralValue, MatchArm, Param, Program, SourceSpan, StaticDef, Stmt,
    StructDef, StructField, Type,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::path::Path;
use std::process::Command;
use std::str::FromStr;

/// Preparatory selector for native-build backend plumbing.
///
/// Stage1 currently implements only the generated-Rust path; additional
/// native backends remain follow-on work under #105.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum NativeBackendKind {
    #[default]
    GeneratedRust,
}

impl NativeBackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GeneratedRust => "generated-rust",
        }
    }
}

impl fmt::Display for NativeBackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for NativeBackendKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "generated-rust" => Ok(Self::GeneratedRust),
            other => Err(format!(
                "unsupported backend {other:?}; only generated-rust is implemented in this preparatory backend plumbing"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GeneratedRustBackendInput, NativeBackendKind, deterministic_numbers, deterministic_strings,
        render_generated_rust,
    };
    use crate::mir::{Function, Program, Type};
    use std::str::FromStr;

    #[test]
    fn parses_generated_rust_backend() {
        assert_eq!(
            NativeBackendKind::from_str("generated-rust").expect("parse generated-rust"),
            NativeBackendKind::GeneratedRust
        );
    }

    #[test]
    fn rejects_unsupported_backend_value() {
        let error = NativeBackendKind::from_str("direct-native")
            .expect_err("unsupported backend values should be rejected");
        assert!(
            error.contains(
                "only generated-rust is implemented in this preparatory backend plumbing"
            )
        );
    }

    #[test]
    fn generated_rust_backend_accepts_mir_input_contract() {
        let program = Program {
            path: String::from("contract"),
            functions: vec![Function {
                name: String::from("main"),
                source_name: String::from("main"),
                path: String::from("contract"),
                params: vec![],
                return_ty: Type::Int,
                body: vec![],
                is_async: false,
                is_extern: false,
                extern_abi: None,
                extern_library: None,
                line: 1,
                column: 1,
            }],
            structs: vec![],
            enums: vec![],
            statics: vec![],
            stmts: vec![],
        };

        let rendered = render_generated_rust(&GeneratedRustBackendInput::from_mir(program));

        assert!(rendered.contains("fn main()"));
    }

    #[test]
    fn deterministic_capability_allowlists_are_sorted_and_deduplicated() {
        let env_vars = vec![
            String::from("ZED"),
            String::from("ALPHA"),
            String::from("ZED"),
        ];
        let net_hosts = vec![
            String::from("z.example"),
            String::from("a.example"),
            String::from("z.example"),
        ];
        let net_ports = vec![443, 80, 443];

        assert_eq!(deterministic_strings(&env_vars), vec!["ALPHA", "ZED"]);
        assert_eq!(
            deterministic_strings(&net_hosts),
            vec!["a.example", "z.example"]
        );
        assert_eq!(deterministic_numbers(&net_ports), vec![80, 443]);
    }
}

/// Typed input contract for the generated-Rust backend.
///
/// Codegen intentionally receives a lowered MIR program plus backend context
/// only. Keeping this as the public seam prevents the generated-Rust backend
/// from depending on parser, syntax, or HIR internals.
#[derive(Debug, Clone)]
pub struct GeneratedRustBackendInput {
    pub program: Program,
    pub debug: bool,
    pub package_root: std::path::PathBuf,
    pub fs_root: std::path::PathBuf,
    pub capabilities: CapabilityConfig,
}

impl GeneratedRustBackendInput {
    pub fn from_mir(program: Program) -> Self {
        Self {
            program,
            debug: false,
            package_root: std::path::PathBuf::from("."),
            fs_root: std::path::PathBuf::from("."),
            capabilities: CapabilityConfig::default(),
        }
    }

    pub fn with_debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    pub fn with_paths(
        mut self,
        package_root: impl Into<std::path::PathBuf>,
        fs_root: impl Into<std::path::PathBuf>,
    ) -> Self {
        self.package_root = package_root.into();
        self.fs_root = fs_root.into();
        self
    }

    pub fn with_capabilities(mut self, capabilities: CapabilityConfig) -> Self {
        self.capabilities = capabilities;
        self
    }
}

pub fn render_generated_rust(input: &GeneratedRustBackendInput) -> String {
    render_rust_for_package_with_capabilities(
        &input.program,
        input.debug,
        &input.package_root,
        &input.fs_root,
        &input.capabilities,
    )
}

pub fn render_rust(program: &Program) -> String {
    render_generated_rust(&GeneratedRustBackendInput::from_mir(program.clone()))
}

pub fn render_rust_with_debug(program: &Program, debug: bool) -> String {
    render_generated_rust(&GeneratedRustBackendInput::from_mir(program.clone()).with_debug(debug))
}

pub fn render_rust_for_package(
    program: &Program,
    debug: bool,
    package_root: &Path,
    fs_root: &Path,
) -> String {
    render_rust_for_package_with_capabilities(
        program,
        debug,
        package_root,
        fs_root,
        &CapabilityConfig::default(),
    )
}

fn deterministic_strings(values: &[String]) -> Vec<&str> {
    values
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn deterministic_numbers(values: &[u16]) -> Vec<u16> {
    values
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn deterministic_named_refs<T, F>(values: &[T], name: F) -> Vec<&T>
where
    F: Fn(&T) -> &str,
{
    let mut values = values.iter().collect::<Vec<_>>();
    values.sort_by(|left, right| name(left).cmp(name(right)));
    values
}

pub fn render_rust_for_package_with_capabilities(
    program: &Program,
    debug: bool,
    package_root: &Path,
    fs_root: &Path,
    capabilities: &CapabilityConfig,
) -> String {
    let type_context = TypeContext::new(program);
    let uses_http_get = program_uses_call(program, "http_get");
    let uses_http_serve_once = program_uses_call(program, "http_serve_once");
    let uses_http_serve_route = program_uses_call(program, "http_serve_route");
    let uses_ffi = program.functions.iter().any(|function| function.is_extern);
    let uses_ffi_cstring = program
        .functions
        .iter()
        .filter(|function| function.is_extern)
        .any(|function| {
            function
                .params
                .iter()
                .any(|param| matches!(param.ty, Type::String))
        });
    let uses_ffi_cstr = program
        .functions
        .iter()
        .filter(|function| function.is_extern)
        .any(|function| matches!(function.return_ty, Type::String));
    let mut out = String::new();
    out.push_str(
        "#[allow(unused_imports)]
",
    );
    out.push_str(
        "use std::collections::HashMap;
",
    );
    if uses_ffi_cstr && uses_ffi_cstring {
        out.push_str(
            "use std::ffi::{CStr, CString};
",
        );
    } else if uses_ffi_cstr {
        out.push_str(
            "use std::ffi::CStr;
",
        );
    } else if uses_ffi_cstring {
        out.push_str(
            "use std::ffi::CString;
",
        );
    }
    if uses_ffi {
        out.push_str(
            "use std::os::raw::c_char;
",
        );
    }
    out.push_str("use std::panic;\n");
    out.push_str("use std::thread;\n");
    out.push_str("use std::sync::{Arc, Condvar, Mutex, Once};\n\n");
    let package_root = rust_path_literal(package_root);
    let fs_root = rust_path_literal(fs_root);
    out.push_str(&format!(
        "const AXIOM_PACKAGE_ROOT: &str = {package_root:?};\n"
    ));
    out.push_str(&format!("const AXIOM_FS_ROOT: &str = {fs_root:?};\n"));
    out.push_str(&format!(
        "const AXIOM_ENV_UNRESTRICTED: bool = {};\n",
        capabilities.env_unrestricted
    ));
    out.push_str(&format!(
        "const AXIOM_ASYNC_CAPABILITY: bool = {};\n",
        capabilities.async_runtime
    ));
    out.push_str(&format!("const AXIOM_DEBUG_BUILD: bool = {debug};\n"));
    out.push_str("const AXIOM_ENV_ALLOWLIST: &[&str] = &[\n");
    for name in deterministic_strings(&capabilities.env_vars) {
        out.push_str(&format!("    {name:?},\n"));
    }
    out.push_str("];\n");
    out.push_str("const AXIOM_NET_HOST_ALLOWLIST: &[&str] = &[\n");
    for host in deterministic_strings(&capabilities.net_hosts) {
        out.push_str(&format!("    {host:?},\n"));
    }
    out.push_str("];\n");
    out.push_str("const AXIOM_NET_PORT_ALLOWLIST: &[u16] = &[\n");
    for port in deterministic_numbers(&capabilities.net_ports) {
        out.push_str(&format!("    {port},\n"));
    }
    out.push_str("];\n");
    out.push_str("const AXIOM_MAX_FS_READ_BYTES: u64 = 64 * 1024 * 1024;\n");
    out.push_str("const AXIOM_MAX_FS_WRITE_BYTES: usize = 64 * 1024 * 1024;\n\n");
    out.push_str("const AXIOM_HOST_AUDIT_LOG_ENV: &str = \"AXIOM_HOST_AUDIT_LOG\";\n\n");
    out.push_str("struct AxiomRuntimeAbort;\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("struct AxiomTask<T> {\n");
    out.push_str("    value: Option<T>,\n");
    out.push_str("    thunk: Option<Box<dyn FnOnce() -> T + Send>>,\n");
    out.push_str("    canceled: bool,\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("struct AxiomJoinHandle<T> {\n");
    out.push_str("    handle: Option<thread::JoinHandle<T>>,\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("#[derive(Clone)]\n");
    out.push_str("struct AxiomChannel<T> {\n");
    out.push_str("    state: Arc<(Mutex<AxiomChannelState<T>>, Condvar)>,\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("#[derive(Debug, PartialEq)]\n");
    out.push_str("struct AxiomChannelState<T> {\n");
    out.push_str("    slot: Option<T>,\n");
    out.push_str("    closed: bool,\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("#[derive(Debug, PartialEq)]\n");
    out.push_str("struct AxiomSelectResult<T> {\n");
    out.push_str("    selected: i64,\n");
    out.push_str("    value: Option<T>,\n");
    out.push_str("}\n\n");
    out.push_str("fn axiom_install_panic_hook() {\n");
    out.push_str("    static AXIOM_PANIC_HOOK: Once = Once::new();\n");
    out.push_str("    AXIOM_PANIC_HOOK.call_once(|| {\n");
    out.push_str("        panic::set_hook(Box::new(|_| {}));\n");
    out.push_str("    });\n");
    out.push_str("}\n\n");
    out.push_str("fn axiom_runtime_report(kind: &str, message: &str) {\n");
    out.push_str("    eprintln!(\n");
    out.push_str("        \"{{\\\"kind\\\":\\\"{}\\\",\\\"message\\\":{}}}\",\n");
    out.push_str("        kind,\n");
    out.push_str("        axiom_json_escape_string(message)\n");
    out.push_str("    );\n");
    out.push_str("}\n\n");
    out.push_str("fn axiom_runtime_error(kind: &str, message: &str) -> ! {\n");
    out.push_str("    axiom_runtime_report(kind, message);\n");
    out.push_str("    panic::panic_any(AxiomRuntimeAbort)\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_panic(message: String) -> ! {\n");
    out.push_str("    axiom_runtime_error(\"panic\", &message)\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_task_ready<T>(value: T) -> AxiomTask<T> {\n");
    out.push_str("    AxiomTask { value: Some(value), thunk: None, canceled: false }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_task_deferred<T: Send + 'static>(thunk: impl FnOnce() -> T + Send + 'static) -> AxiomTask<T> {\n");
    out.push_str("    AxiomTask { value: None, thunk: Some(Box::new(thunk)), canceled: false }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_await<T>(mut task: AxiomTask<T>) -> T {\n");
    out.push_str("    if task.canceled {\n");
    out.push_str("        axiom_runtime_error(\"async\", \"awaited task was canceled\");\n");
    out.push_str("    }\n");
    out.push_str("    if let Some(value) = task.value.take() { return value; }\n");
    out.push_str("    match task.thunk.take() {\n");
    out.push_str("        Some(thunk) => thunk(),\n");
    out.push_str("        None => axiom_runtime_error(\"async\", \"task had no value or scheduled body\"),\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str(r#"#[allow(dead_code)]
struct AxiomRuntimeScheduler {
    scheduled: usize,
    completed: usize,
}

#[allow(dead_code)]
impl AxiomRuntimeScheduler {
    fn new() -> Self {
        Self { scheduled: 0, completed: 0 }
    }

    fn schedule<T: Send + 'static>(&mut self, task: AxiomTask<T>) -> AxiomJoinHandle<T> {
        self.scheduled += 1;
        AxiomJoinHandle {
            handle: Some(thread::spawn(move || axiom_await(task))),
        }
    }

    fn join<T: Send + 'static>(&mut self, mut handle: AxiomJoinHandle<T>) -> AxiomTask<T> {
        match handle.handle.take() {
            Some(join_handle) => {
                self.completed += 1;
                axiom_task_deferred(move || {
                    join_handle
                        .join()
                        .unwrap_or_else(|_| axiom_runtime_error("async", "joined task panicked"))
                })
            }
            None => axiom_runtime_error("async", "invalid join handle state"),
        }
    }
}

#[allow(dead_code)]
fn axiom_async_spawn<T: Send + 'static>(task: AxiomTask<T>) -> AxiomJoinHandle<T> {
    let mut scheduler = AxiomRuntimeScheduler::new();
    scheduler.schedule(task)
}

#[allow(dead_code)]
fn axiom_async_join<T: Send + 'static>(handle: AxiomJoinHandle<T>) -> AxiomTask<T> {
    let mut scheduler = AxiomRuntimeScheduler::new();
    scheduler.join(handle)
}

#[allow(dead_code)]
fn axiom_async_cancel<T>(mut task: AxiomTask<T>) -> AxiomTask<T> {
    task.canceled = true;
    task
}

#[allow(dead_code)]
fn axiom_async_timeout<T: Send + 'static>(task: AxiomTask<T>, timeout_ms: i64) -> AxiomTask<Option<T>> {
    axiom_task_deferred(move || {
        if task.canceled {
            return None;
        }
        let timeout = std::time::Duration::from_millis(timeout_ms.clamp(0, 30_000) as u64);
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let value = axiom_await(task);
            let _ = sender.send(value);
        });
        match receiver.recv_timeout(timeout) {
            Ok(value) => {
                let _ = worker.join();
                Some(value)
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                let _ = worker.join();
                axiom_runtime_error("async", "timed task panicked")
            }
        }
    })
}

#[allow(dead_code)]
fn axiom_async_channel<T>() -> AxiomChannel<T> {
    AxiomChannel {
        state: Arc::new((
            Mutex::new(AxiomChannelState { slot: None, closed: false }),
            Condvar::new(),
        )),
    }
}

#[allow(dead_code)]
fn axiom_async_send<T: Send + 'static>(channel: AxiomChannel<T>, value: T) -> AxiomTask<AxiomChannel<T>> {
    axiom_task_deferred(move || {
        let (lock, wakeup) = &*channel.state;
        let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        while state.slot.is_some() && !state.closed {
            state = wakeup.wait(state).unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        if state.closed {
            axiom_runtime_error("async", "send on closed channel");
        }
        state.slot = Some(value);
        wakeup.notify_one();
        drop(state);
        channel
    })
}

#[allow(dead_code)]
fn axiom_async_recv<T: Send + 'static>(channel: AxiomChannel<T>) -> AxiomTask<Option<T>> {
    axiom_task_deferred(move || {
        let (lock, wakeup) = &*channel.state;
        let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            if state.slot.is_some() {
                let value = state.slot.take();
                wakeup.notify_one();
                return value;
            }
            if state.closed {
                return None;
            }
            state = wakeup.wait(state).unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    })
}

"#);
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_numeric_checked_add_i8(left: i8, right: i8) -> i8 {\n");
    out.push_str("    if AXIOM_DEBUG_BUILD {\n");
    out.push_str("        match left.checked_add(right) {\n");
    out.push_str("            Some(value) => value,\n");
    out.push_str("            None => axiom_runtime_error(\"runtime\", \"numeric overflow: i8 addition\"),\n");
    out.push_str("        }\n");
    out.push_str("    } else {\n");
    out.push_str("        left.wrapping_add(right)\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_numeric_checked_add_i16(left: i16, right: i16) -> i16 {\n");
    out.push_str("    if AXIOM_DEBUG_BUILD {\n");
    out.push_str("        match left.checked_add(right) {\n");
    out.push_str("            Some(value) => value,\n");
    out.push_str("            None => axiom_runtime_error(\"runtime\", \"numeric overflow: i16 addition\"),\n");
    out.push_str("        }\n");
    out.push_str("    } else {\n");
    out.push_str("        left.wrapping_add(right)\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_numeric_checked_add_i32(left: i32, right: i32) -> i32 {\n");
    out.push_str("    if AXIOM_DEBUG_BUILD {\n");
    out.push_str("        match left.checked_add(right) {\n");
    out.push_str("            Some(value) => value,\n");
    out.push_str("            None => axiom_runtime_error(\"runtime\", \"numeric overflow: i32 addition\"),\n");
    out.push_str("        }\n");
    out.push_str("    } else {\n");
    out.push_str("        left.wrapping_add(right)\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_numeric_checked_add_i64(left: i64, right: i64) -> i64 {\n");
    out.push_str("    if AXIOM_DEBUG_BUILD {\n");
    out.push_str("        match left.checked_add(right) {\n");
    out.push_str("            Some(value) => value,\n");
    out.push_str("            None => axiom_runtime_error(\"runtime\", \"numeric overflow: i64 addition\"),\n");
    out.push_str("        }\n");
    out.push_str("    } else {\n");
    out.push_str("        left.wrapping_add(right)\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_numeric_checked_add_isize(left: isize, right: isize) -> isize {\n");
    out.push_str("    if AXIOM_DEBUG_BUILD {\n");
    out.push_str("        match left.checked_add(right) {\n");
    out.push_str("            Some(value) => value,\n");
    out.push_str("            None => axiom_runtime_error(\"runtime\", \"numeric overflow: isize addition\"),\n");
    out.push_str("        }\n");
    out.push_str("    } else {\n");
    out.push_str("        left.wrapping_add(right)\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_array_get<T: Copy>(values: &[T], index: i64) -> T {\n");
    out.push_str("    if index < 0 {\n");
    out.push_str(
        "        axiom_runtime_error(\"runtime\", \"array index must be non-negative\");\n",
    );
    out.push_str("    }\n");
    out.push_str("    match values.get(index as usize) {\n");
    out.push_str("        Some(value) => *value,\n");
    out.push_str(
        "        None => axiom_runtime_error(\"runtime\", \"array index out of bounds\"),\n",
    );
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_array_get_mut<T>(values: &mut [T], index: i64) -> &mut T {\n");
    out.push_str("    if index < 0 {\n");
    out.push_str(
        "        axiom_runtime_error(\"runtime\", \"array index must be non-negative\");\n",
    );
    out.push_str("    }\n");
    out.push_str("    match values.get_mut(index as usize) {\n");
    out.push_str("        Some(value) => value,\n");
    out.push_str(
        "        None => axiom_runtime_error(\"runtime\", \"array index out of bounds\"),\n",
    );
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_array_take<T>(values: Vec<T>, index: i64) -> T {\n");
    out.push_str("    if index < 0 {\n");
    out.push_str(
        "        axiom_runtime_error(\"runtime\", \"array index must be non-negative\");\n",
    );
    out.push_str("    }\n");
    out.push_str("    match values.into_iter().nth(index as usize) {\n");
    out.push_str("        Some(value) => value,\n");
    out.push_str(
        "        None => axiom_runtime_error(\"runtime\", \"array index out of bounds\"),\n",
    );
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str(
        "fn axiom_array_slice_bounds(len: usize, start: Option<i64>, end: Option<i64>) -> (usize, usize) {\n",
    );
    out.push_str("    let start = start.unwrap_or(0);\n");
    out.push_str("    let end = end.unwrap_or(len as i64);\n");
    out.push_str("    if start < 0 {\n");
    out.push_str(
        "        axiom_runtime_error(\"runtime\", \"array slice start must be non-negative\");\n",
    );
    out.push_str("    }\n");
    out.push_str("    if end < 0 {\n");
    out.push_str(
        "        axiom_runtime_error(\"runtime\", \"array slice end must be non-negative\");\n",
    );
    out.push_str("    }\n");
    out.push_str("    let start = start as usize;\n");
    out.push_str("    let end = end as usize;\n");
    out.push_str("    if start > end {\n");
    out.push_str(
        "        axiom_runtime_error(\"runtime\", \"array slice start must be <= end\");\n",
    );
    out.push_str("    }\n");
    out.push_str("    if end > len {\n");
    out.push_str("        axiom_runtime_error(\"runtime\", \"array slice end out of bounds\");\n");
    out.push_str("    }\n");
    out.push_str("    (start, end)\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_slice_view<'a, T>(values: &'a [T], start: Option<i64>, end: Option<i64>) -> &'a [T] {\n");
    out.push_str("    let (start, end) = axiom_array_slice_bounds(values.len(), start, end);\n");
    out.push_str("    match values.get(start..end) {\n");
    out.push_str("        Some(slice) => slice,\n");
    out.push_str(
        "        None => axiom_runtime_error(\"runtime\", \"array slice out of bounds\"),\n",
    );
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_slice_view_mut<'a, T>(values: &'a mut [T], start: Option<i64>, end: Option<i64>) -> &'a mut [T] {\n");
    out.push_str("    let (start, end) = axiom_array_slice_bounds(values.len(), start, end);\n");
    out.push_str("    match values.get_mut(start..end) {\n");
    out.push_str("        Some(slice) => slice,\n");
    out.push_str(
        "        None => axiom_runtime_error(\"runtime\", \"array slice out of bounds\"),\n",
    );
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_last_index(len: usize) -> i64 {\n");
    out.push_str("    if len == 0 {\n");
    out.push_str("        axiom_runtime_error(\"runtime\", \"collection must not be empty\");\n");
    out.push_str("    }\n");
    out.push_str("    (len - 1) as i64\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_io_eprintln(text: String) -> i64 {\n");
    out.push_str("    use std::io::Write;\n");
    out.push_str("    let stderr = std::io::stderr();\n");
    out.push_str("    let mut handle = stderr.lock();\n");
    out.push_str(
        "    match handle.write_all(text.as_bytes()).and_then(|_| handle.write_all(b\"\\n\")) {\n",
    );
    out.push_str("        Ok(()) => (text.len() as i64) + 1,\n");
    out.push_str("        Err(_) => -1,\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_io_readline() -> Option<String> {\n");
    out.push_str("    let stdin = std::io::stdin();\n");
    out.push_str("    let mut handle = stdin.lock();\n");
    out.push_str("    let mut line = String::new();\n");
    out.push_str("    match std::io::BufRead::read_line(&mut handle, &mut line) {\n");
    out.push_str("        Ok(0) => None,\n");
    out.push_str("        Ok(_) => {\n");
    out.push_str("            if line.ends_with('\\n') { line.pop(); }\n");
    out.push_str("            if line.ends_with('\\r') { line.pop(); }\n");
    out.push_str("            Some(line)\n");
    out.push_str("        }\n");
    out.push_str("        Err(_) => None,\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_io_read_to_string() -> String {\n");
    out.push_str("    let stdin = std::io::stdin();\n");
    out.push_str("    let mut handle = stdin.lock();\n");
    out.push_str("    let mut content = String::new();\n");
    out.push_str("    match std::io::Read::read_to_string(&mut handle, &mut content) {\n");
    out.push_str("        Ok(_) => content,\n");
    out.push_str("        Err(_) => String::new(),\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str(
        r##"#[allow(dead_code)]
fn axiom_json_escape(value: &str) -> String {
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

#[allow(dead_code)]
fn axiom_capability_audit(intrinsic: &str, capability: &str, arg_summary: &str, outcome: &str) {
    let Ok(path) = std::env::var("AXIOM_CAPABILITY_AUDIT_JSONL") else {
        return;
    };
    if path.trim().is_empty() {
        return;
    }
    let event = format!(
        "{{\"package\":\"{}\",\"intrinsic\":\"{}\",\"capability\":\"{}\",\"args\":\"{}\",\"outcome\":\"{}\"}}\n",
        axiom_json_escape(AXIOM_PACKAGE_ROOT),
        axiom_json_escape(intrinsic),
        axiom_json_escape(capability),
        axiom_json_escape(arg_summary),
        axiom_json_escape(outcome),
    );
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        use std::io::Write;
        let _ = file.write_all(event.as_bytes());
    }
}

"##,
    );
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_assert_fail(message: String, _line: i64, _column: i64) -> i64 {\n");
    out.push_str("    axiom_runtime_error(\"assertion\", &message)\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_json_parse_int(text: String) -> Option<i64> {\n");
    out.push_str("    text.trim().parse::<i64>().ok()\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_json_parse_bool(text: String) -> Option<bool> {\n");
    out.push_str("    match text.trim() {\n");
    out.push_str("        \"true\" => Some(true),\n");
    out.push_str("        \"false\" => Some(false),\n");
    out.push_str("        _ => None,\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_json_parse_string(text: String) -> Option<String> {\n");
    out.push_str("    let text = text.trim();\n");
    out.push_str("    if text.len() < 2 || !text.starts_with('\"') || !text.ends_with('\"') {\n");
    out.push_str("        return None;\n");
    out.push_str("    }\n");
    out.push_str("    let mut out = String::new();\n");
    out.push_str("    let mut chars = text[1..text.len() - 1].chars();\n");
    out.push_str("    while let Some(ch) = chars.next() {\n");
    out.push_str("        if ch != '\\\\' {\n");
    out.push_str("            out.push(ch);\n");
    out.push_str("            continue;\n");
    out.push_str("        }\n");
    out.push_str("        match chars.next()? {\n");
    out.push_str("            '\"' => out.push('\"'),\n");
    out.push_str("            '\\\\' => out.push('\\\\'),\n");
    out.push_str("            '/' => out.push('/'),\n");
    out.push_str("            'b' => out.push('\\u{0008}'),\n");
    out.push_str("            'f' => out.push('\\u{000C}'),\n");
    out.push_str("            'n' => out.push('\\n'),\n");
    out.push_str("            'r' => out.push('\\r'),\n");
    out.push_str("            't' => out.push('\\t'),\n");
    out.push_str("            'u' => {\n");
    out.push_str("                let mut value = 0u32;\n");
    out.push_str("                for _ in 0..4 {\n");
    out.push_str("                    value = (value << 4) + chars.next()?.to_digit(16)?;\n");
    out.push_str("                }\n");
    out.push_str("                out.push(char::from_u32(value)?);\n");
    out.push_str("            }\n");
    out.push_str("            _ => return None,\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("    Some(out)\n");
    out.push_str("}\n\n");

    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_json_skip_ws(text: &str, mut index: usize) -> usize {\n");
    out.push_str("    let bytes = text.as_bytes();\n");
    out.push_str("    while index < bytes.len() && bytes[index].is_ascii_whitespace() {\n");
    out.push_str("        index += 1;\n");
    out.push_str("    }\n");
    out.push_str("    index\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_json_scan_string_end(text: &str, start: usize) -> Option<usize> {\n");
    out.push_str("    let bytes = text.as_bytes();\n");
    out.push_str("    if bytes.get(start).copied()? != b'\\\"' {\n");
    out.push_str("        return None;\n");
    out.push_str("    }\n");
    out.push_str("    let mut index = start + 1;\n");
    out.push_str("    while index < bytes.len() {\n");
    out.push_str("        match bytes[index] {\n");
    out.push_str("            b'\\\\' => index += 2,\n");
    out.push_str("            b'\\\"' => return Some(index + 1),\n");
    out.push_str("            _ => index += 1,\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("    None\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_json_scan_value_end(text: &str, start: usize) -> Option<usize> {\n");
    out.push_str("    let bytes = text.as_bytes();\n");
    out.push_str("    if start >= bytes.len() {\n");
    out.push_str("        return None;\n");
    out.push_str("    }\n");
    out.push_str("    if bytes[start] == b'\\\"' {\n");
    out.push_str("        return axiom_json_scan_string_end(text, start);\n");
    out.push_str("    }\n");
    out.push_str("    let mut index = start;\n");
    out.push_str("    let mut depth = 0i64;\n");
    out.push_str("    while index < bytes.len() {\n");
    out.push_str("        match bytes[index] {\n");
    out.push_str("            b'\\\"' => index = axiom_json_scan_string_end(text, index)?,\n");
    out.push_str("            b'{' | b'[' => { depth += 1; index += 1; }\n");
    out.push_str("            b'}' | b']' if depth > 0 => { depth -= 1; index += 1; }\n");
    out.push_str("            b',' | b'}' if depth == 0 => return Some(index),\n");
    out.push_str("            _ => index += 1,\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("    Some(index)\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_json_object_field(text: String, key: String) -> Option<String> {\n");
    out.push_str("    let text = text.trim();\n");
    out.push_str("    let bytes = text.as_bytes();\n");
    out.push_str("    if bytes.first().copied()? != b'{' || bytes.last().copied()? != b'}' {\n");
    out.push_str("        return None;\n");
    out.push_str("    }\n");
    out.push_str("    let mut index = 1usize;\n");
    out.push_str("    loop {\n");
    out.push_str("        index = axiom_json_skip_ws(text, index);\n");
    out.push_str("        if index >= bytes.len() || bytes[index] == b'}' {\n");
    out.push_str("            return None;\n");
    out.push_str("        }\n");
    out.push_str("        let key_end = axiom_json_scan_string_end(text, index)?;\n");
    out.push_str(
        "        let found_key = axiom_json_parse_string(text[index..key_end].to_string())?;\n",
    );
    out.push_str("        index = axiom_json_skip_ws(text, key_end);\n");
    out.push_str("        if bytes.get(index).copied()? != b':' {\n");
    out.push_str("            return None;\n");
    out.push_str("        }\n");
    out.push_str("        let value_start = axiom_json_skip_ws(text, index + 1);\n");
    out.push_str("        let value_end = axiom_json_scan_value_end(text, value_start)?;\n");
    out.push_str("        if found_key == key {\n");
    out.push_str("            return Some(text[value_start..value_end].trim().to_string());\n");
    out.push_str("        }\n");
    out.push_str("        index = axiom_json_skip_ws(text, value_end);\n");
    out.push_str("        match bytes.get(index).copied()? {\n");
    out.push_str("            b',' => index += 1,\n");
    out.push_str("            b'}' => return None,\n");
    out.push_str("            _ => return None,\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_json_parse_field_int(text: String, key: String) -> Option<i64> {\n");
    out.push_str("    axiom_json_parse_int(axiom_json_object_field(text, key)?)\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_json_parse_value(text: String) -> Option<String> {\n");
    out.push_str("    let text = text.trim();\n");
    out.push_str("    let end = axiom_json_scan_value_end(text, 0)?;\n");
    out.push_str("    if axiom_json_skip_ws(text, end) == text.len() { Some(text.to_string()) } else { None }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_json_parse_field_bool(text: String, key: String) -> Option<bool> {\n");
    out.push_str("    axiom_json_parse_bool(axiom_json_object_field(text, key)?)\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str(
        "fn axiom_json_parse_field_string(text: String, key: String) -> Option<String> {\n",
    );
    out.push_str("    axiom_json_parse_string(axiom_json_object_field(text, key)?)\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str(
        "fn axiom_json_parse_field_value(text: String, key: String) -> Option<String> {\n",
    );
    out.push_str("    axiom_json_parse_value(axiom_json_object_field(text, key)?)\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_json_escape_string(value: &str) -> String {\n");
    out.push_str("    let mut out = String::from(\"\\\"\");\n");
    out.push_str("    for ch in value.chars() {\n");
    out.push_str("        match ch {\n");
    out.push_str("            '\"' => out.push_str(\"\\\\\\\"\"),\n");
    out.push_str("            '\\\\' => out.push_str(\"\\\\\\\\\"),\n");
    out.push_str("            '\\n' => out.push_str(\"\\\\n\"),\n");
    out.push_str("            '\\r' => out.push_str(\"\\\\r\"),\n");
    out.push_str("            '\\t' => out.push_str(\"\\\\t\"),\n");
    out.push_str("            '\\u{0008}' => out.push_str(\"\\\\b\"),\n");
    out.push_str("            '\\u{000C}' => out.push_str(\"\\\\f\"),\n");
    out.push_str("            ch if ch.is_control() => out.push_str(&format!(\"\\\\u{:04x}\", ch as u32)),\n");
    out.push_str("            _ => out.push(ch),\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("    out.push('\"');\n");
    out.push_str("    out\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_json_stringify_int(value: i64) -> String {\n");
    out.push_str("    value.to_string()\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_json_stringify_bool(value: bool) -> String {\n");
    out.push_str("    value.to_string()\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_json_stringify_string(value: String) -> String {\n");
    out.push_str("    axiom_json_escape_string(&value)\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_json_stringify_value(value: String) -> String {\n");
    out.push_str("    axiom_json_parse_value(value.clone()).unwrap_or(value)\n");
    out.push_str("}\n\n");
    out.push_str(r#"#[derive(Clone, Debug, PartialEq, Eq)]
enum AxiomRegexAtom {
    Literal(char),
    Any,
    Class { ranges: Vec<(char, char)>, negated: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AxiomRegexQuantifier {
    One,
    ZeroOrOne,
    ZeroOrMore,
    OneOrMore,
}

#[derive(Clone, Debug)]
struct AxiomRegexToken {
    atom: AxiomRegexAtom,
    quantifier: AxiomRegexQuantifier,
}

#[derive(Clone, Debug)]
struct AxiomRegexProgram {
    tokens: Vec<AxiomRegexToken>,
    start_anchor: bool,
    end_anchor: bool,
}

fn axiom_regex_escape_char(ch: char) -> char {
    match ch {
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        other => other,
    }
}

fn axiom_regex_parse_atom(chars: &[char], pos: &mut usize) -> Option<AxiomRegexAtom> {
    if *pos >= chars.len() {
        return None;
    }
    let ch = chars[*pos];
    *pos += 1;
    match ch {
        '.' => Some(AxiomRegexAtom::Any),
        '\\' => {
            if *pos >= chars.len() {
                Some(AxiomRegexAtom::Literal('\\'))
            } else {
                let escaped = axiom_regex_escape_char(chars[*pos]);
                *pos += 1;
                Some(AxiomRegexAtom::Literal(escaped))
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
                    return Some(AxiomRegexAtom::Class { ranges, negated });
                }
                first = false;
                let start = if chars[*pos] == '\\' {
                    *pos += 1;
                    if *pos >= chars.len() { return None; }
                    let escaped = axiom_regex_escape_char(chars[*pos]);
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
                        if *pos >= chars.len() { return None; }
                        let escaped = axiom_regex_escape_char(chars[*pos]);
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
        other => Some(AxiomRegexAtom::Literal(other)),
    }
}

fn axiom_regex_parse(pattern: &str) -> Option<AxiomRegexProgram> {
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
        let atom = axiom_regex_parse_atom(&chars[..parse_end], &mut atom_pos)?;
        pos = atom_pos;
        let quantifier = if pos < parse_end {
            match chars[pos] {
                '?' => { pos += 1; AxiomRegexQuantifier::ZeroOrOne }
                '*' => { pos += 1; AxiomRegexQuantifier::ZeroOrMore }
                '+' => { pos += 1; AxiomRegexQuantifier::OneOrMore }
                _ => AxiomRegexQuantifier::One,
            }
        } else {
            AxiomRegexQuantifier::One
        };
        tokens.push(AxiomRegexToken { atom, quantifier });
    }
    Some(AxiomRegexProgram { tokens, start_anchor, end_anchor })
}

fn axiom_regex_atom_matches(atom: &AxiomRegexAtom, ch: char) -> bool {
    match atom {
        AxiomRegexAtom::Literal(expected) => *expected == ch,
        AxiomRegexAtom::Any => true,
        AxiomRegexAtom::Class { ranges, negated } => {
            let found = ranges.iter().any(|(start, end)| *start <= ch && ch <= *end);
            if *negated { !found } else { found }
        }
    }
}

fn axiom_regex_add_state(program: &AxiomRegexProgram, states: &mut Vec<usize>, state: usize) {
    if states.contains(&state) {
        return;
    }
    states.push(state);
    if state >= program.tokens.len() {
        return;
    }
    match program.tokens[state].quantifier {
        AxiomRegexQuantifier::ZeroOrOne | AxiomRegexQuantifier::ZeroOrMore => {
            axiom_regex_add_state(program, states, state + 1);
        }
        AxiomRegexQuantifier::One | AxiomRegexQuantifier::OneOrMore => {}
    }
}

fn axiom_regex_accepts(program: &AxiomRegexProgram, states: &[usize], at_text_end: bool) -> bool {
    states.iter().any(|state| {
        *state == program.tokens.len() && (!program.end_anchor || at_text_end)
    })
}

fn axiom_regex_match_from(program: &AxiomRegexProgram, text: &[char], start: usize) -> Option<usize> {
    let mut states = Vec::new();
    axiom_regex_add_state(program, &mut states, 0);
    let mut last_accept = if axiom_regex_accepts(program, &states, start == text.len()) {
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
            if !axiom_regex_atom_matches(&token.atom, ch) {
                continue;
            }
            match token.quantifier {
                AxiomRegexQuantifier::One | AxiomRegexQuantifier::ZeroOrOne => {
                    axiom_regex_add_state(program, &mut next, state + 1);
                }
                AxiomRegexQuantifier::ZeroOrMore => {
                    axiom_regex_add_state(program, &mut next, state);
                    axiom_regex_add_state(program, &mut next, state + 1);
                }
                AxiomRegexQuantifier::OneOrMore => {
                    axiom_regex_add_state(program, &mut next, state);
                    axiom_regex_add_state(program, &mut next, state + 1);
                }
            }
        }
        pos += 1;
        if axiom_regex_accepts(program, &next, pos == text.len()) {
            last_accept = Some(pos);
        }
        states = next;
        if states.is_empty() {
            return last_accept;
        }
    }
    last_accept
}

fn axiom_regex_find_span(pattern: &str, text: &str) -> Option<(usize, usize)> {
    let program = axiom_regex_parse(pattern)?;
    let chars: Vec<char> = text.chars().collect();
    let byte_offsets: Vec<usize> = text.char_indices().map(|(idx, _)| idx).chain(std::iter::once(text.len())).collect();
    let starts: Box<dyn Iterator<Item = usize>> = if program.start_anchor {
        Box::new(std::iter::once(0))
    } else {
        Box::new(0..=chars.len())
    };
    for start in starts {
        if let Some(end) = axiom_regex_match_from(&program, &chars, start) {
            return Some((byte_offsets[start], byte_offsets[end]));
        }
    }
    None
}

#[allow(dead_code)]
fn axiom_regex_is_match(pattern: String, text: String) -> bool {
    axiom_regex_find_span(&pattern, &text).is_some()
}

#[allow(dead_code)]
fn axiom_regex_find(pattern: String, text: String) -> Option<String> {
    let (start, end) = axiom_regex_find_span(&pattern, &text)?;
    Some(text[start..end].to_string())
}

#[allow(dead_code)]
fn axiom_regex_replace_all(pattern: String, text: String, replacement: String) -> String {
    if axiom_regex_parse(&pattern).is_none() {
        return text;
    }
    let mut remaining = text.as_str();
    let mut out = String::new();
    loop {
        let Some((start, end)) = axiom_regex_find_span(&pattern, remaining) else {
            out.push_str(remaining);
            break;
        };
        out.push_str(&remaining[..start]);
        out.push_str(&replacement);
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

"#);
    out.push_str(
        r#"#[allow(dead_code)]
fn axiom_encoding_is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
}

#[allow(dead_code)]
fn axiom_percent_encode(value: String) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::new();
    for byte in value.bytes() {
        if axiom_encoding_is_unreserved(byte) {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    out
}

#[allow(dead_code)]
fn axiom_query_pair_encode(name: String, value: String) -> String {
    format!("{}={}", axiom_percent_encode(name), axiom_percent_encode(value))
}

#[allow(dead_code)]
fn axiom_path_join_segment(base: String, segment: String) -> String {
    let encoded = axiom_percent_encode(segment);
    if base.is_empty() {
        encoded
    } else if base.ends_with('/') {
        format!("{base}{encoded}")
    } else {
        format!("{base}/{encoded}")
    }
}

#[allow(dead_code)]
fn axiom_percent_decode(value: String) -> Option<String> {
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

"#,
    );
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_cli_args() -> Vec<String> {\n");
    out.push_str("    std::env::args().skip(1).collect()\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_cli_arg_count() -> i64 {\n");
    out.push_str("    std::env::args().skip(1).count() as i64\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_cli_arg(index: i64) -> Option<String> {\n");
    out.push_str("    if index < 0 {\n");
    out.push_str("        return None;\n");
    out.push_str("    }\n");
    out.push_str("    std::env::args().skip(1).nth(index as usize)\n");
    out.push_str("}\n\n");
    out.push_str(
        r#"#[allow(dead_code)]
fn axiom_host_arg_summary(parts: &[(&str, String)]) -> String {
    let mut items = Vec::new();
    for (name, summary) in parts {
        items.push(format!(
            "\"{}\":\"{}\"",
            axiom_json_escape(name),
            axiom_json_escape(summary)
        ));
    }
    format!("{{{}}}", items.join(","))
}

#[allow(dead_code)]
fn axiom_host_audit(intrinsic: &str, args: String, outcome: &str) {
    let Ok(path) = std::env::var(AXIOM_HOST_AUDIT_LOG_ENV) else {
        return;
    };
    if path.trim().is_empty() {
        return;
    }
    let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    use std::io::Write;
    let _ = writeln!(
        file,
        "{{\"package\":\"{}\",\"intrinsic\":\"{}\",\"args\":{},\"outcome\":\"{}\"}}",
        axiom_json_escape(AXIOM_PACKAGE_ROOT),
        axiom_json_escape(intrinsic),
        args,
        axiom_json_escape(outcome)
    );
}

"#,
    );
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_fs_read(path: String) -> Option<String> {\n");
    out.push_str("    use std::io::Read;\n");
    out.push_str("    let args = axiom_host_arg_summary(&[(\"path\", format!(\"string:{}\", path.len()))]);\n");
    out.push_str(
        "    let canonical_package_root = std::fs::canonicalize(AXIOM_PACKAGE_ROOT).ok()?;\n",
    );
    out.push_str("    let canonical_fs_root = std::fs::canonicalize(AXIOM_FS_ROOT).ok()?;\n");
    out.push_str("    if !canonical_fs_root.starts_with(&canonical_package_root) {\n");
    out.push_str("        axiom_host_audit(\"fs_read\", args, \"denied\");\n");
    out.push_str("        return None;\n");
    out.push_str("    }\n");
    out.push_str("    let requested = std::path::Path::new(&path);\n");
    out.push_str("    let candidate = if requested.is_absolute() {\n");
    out.push_str("        requested.to_path_buf()\n");
    out.push_str("    } else {\n");
    out.push_str("        canonical_package_root.join(requested)\n");
    out.push_str("    };\n");
    out.push_str(
        "    let Some(canonical_candidate) = std::fs::canonicalize(candidate).ok() else {\n",
    );
    out.push_str("        axiom_host_audit(\"fs_read\", args, \"missing\");\n");
    out.push_str("        return None;\n");
    out.push_str("    };\n");
    out.push_str("    if !canonical_candidate.starts_with(&canonical_fs_root) {\n");
    out.push_str("        axiom_host_audit(\"fs_read\", args, \"denied\");\n");
    out.push_str("        return None;\n");
    out.push_str("    }\n");
    out.push_str("    let Some(metadata) = std::fs::metadata(&canonical_candidate).ok() else {\n");
    out.push_str("        axiom_host_audit(\"fs_read\", args, \"missing\");\n");
    out.push_str("        return None;\n");
    out.push_str("    };\n");
    out.push_str("    if !metadata.is_file() || metadata.len() > AXIOM_MAX_FS_READ_BYTES {\n");
    out.push_str("        axiom_host_audit(\"fs_read\", args, \"denied\");\n");
    out.push_str("        return None;\n");
    out.push_str("    }\n");
    out.push_str("    let Some(file) = std::fs::File::open(&canonical_candidate).ok() else {\n");
    out.push_str("        axiom_host_audit(\"fs_read\", args, \"denied\");\n");
    out.push_str("        return None;\n");
    out.push_str("    };\n");
    out.push_str("    let mut reader = file.take(AXIOM_MAX_FS_READ_BYTES + 1);\n");
    out.push_str("    let mut content = String::new();\n");
    out.push_str("    if reader.read_to_string(&mut content).is_err() {\n");
    out.push_str("        axiom_host_audit(\"fs_read\", args, \"denied\");\n");
    out.push_str("        return None;\n");
    out.push_str("    }\n");
    out.push_str("    if content.len() as u64 > AXIOM_MAX_FS_READ_BYTES {\n");
    out.push_str("        axiom_host_audit(\"fs_read\", args, \"denied\");\n");
    out.push_str("        return None;\n");
    out.push_str("    }\n");
    out.push_str("    axiom_host_audit(\"fs_read\", args, \"ok\");\n");
    out.push_str("    Some(content)\n");
    out.push_str("}\n\n");
    out.push_str(
        r#"#[allow(dead_code)]
fn axiom_fs_candidate(path: &str, allow_missing_ancestors: bool) -> Option<std::path::PathBuf> {
    let canonical_package_root = std::fs::canonicalize(AXIOM_PACKAGE_ROOT).ok()?;
    let canonical_fs_root = std::fs::canonicalize(AXIOM_FS_ROOT).ok()?;
    if !canonical_fs_root.starts_with(&canonical_package_root) {
        return None;
    }
    let requested = std::path::Path::new(path);
    if requested.as_os_str().is_empty() {
        return None;
    }
    if requested
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return None;
    }
    let candidate = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        canonical_package_root.join(requested)
    };
    if let Ok(canonical_candidate) = std::fs::canonicalize(&candidate) {
        if canonical_candidate.starts_with(&canonical_fs_root) {
            return Some(canonical_candidate);
        }
        return None;
    }
    let parent = candidate.parent()?;
    if !allow_missing_ancestors {
        let canonical_parent = std::fs::canonicalize(parent).ok()?;
        if !canonical_parent.starts_with(&canonical_fs_root) {
            return None;
        }
        let file_name = candidate.file_name()?;
        return Some(canonical_parent.join(file_name));
    }
    let mut ancestor = parent;
    while !ancestor.exists() {
        ancestor = ancestor.parent()?;
    }
    let canonical_ancestor = std::fs::canonicalize(ancestor).ok()?;
    if !canonical_ancestor.starts_with(&canonical_fs_root) {
        return None;
    }
    Some(candidate)
}

#[allow(dead_code)]
fn axiom_fs_audit_args(path_len: usize, content_len: Option<usize>) -> String {
    match content_len {
        Some(content_len) => axiom_host_arg_summary(&[("path", format!("string:{path_len}")), ("content", format!("string:{content_len}"))]),
        None => axiom_host_arg_summary(&[("path", format!("string:{path_len}"))]),
    }
}

#[allow(dead_code)]
fn axiom_fs_write(path: String, content: String) -> i64 {
    let path_len = path.len();
    let content_len = content.len();
    let result = if content.len() > AXIOM_MAX_FS_WRITE_BYTES {
        -1
    } else {
        match axiom_fs_candidate(&path, false) {
            Some(candidate) => match std::fs::write(candidate, content) {
                Ok(()) => 0,
                Err(_) => -1,
            },
            None => -1,
        }
    };
    { let args = axiom_fs_audit_args(path_len, Some(content_len)); axiom_host_audit("fs_write", args, if result == 0 { "ok" } else { "denied" }); result }
}

#[allow(dead_code)]
fn axiom_fs_create(path: String) -> i64 {
    let path_len = path.len();
    let result = match axiom_fs_candidate(&path, false) {
        Some(candidate) => match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(candidate)
        {
            Ok(_) => 0,
            Err(_) => -1,
        },
        None => -1,
    };
    { let args = axiom_fs_audit_args(path_len, None); axiom_host_audit("fs_create", args, if result == 0 { "ok" } else { "denied" }); result }
}

#[allow(dead_code)]
fn axiom_fs_append(path: String, content: String) -> i64 {
    let path_len = path.len();
    let content_len = content.len();
    let result = if content.len() > AXIOM_MAX_FS_WRITE_BYTES {
        -1
    } else {
        match axiom_fs_candidate(&path, false) {
            Some(candidate) => match std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(candidate)
            {
                Ok(mut file) => {
                    use std::io::Write;
                    match file.write_all(content.as_bytes()) {
                        Ok(()) => 0,
                        Err(_) => -1,
                    }
                }
                Err(_) => -1,
            },
            None => -1,
        }
    };
    { let args = axiom_fs_audit_args(path_len, Some(content_len)); axiom_host_audit("fs_append", args, if result == 0 { "ok" } else { "denied" }); result }
}

#[allow(dead_code)]
fn axiom_fs_mkdir(path: String) -> i64 {
    let path_len = path.len();
    let result = match axiom_fs_candidate(&path, false) {
        Some(candidate) => match std::fs::create_dir(candidate) {
            Ok(()) => 0,
            Err(_) => -1,
        },
        None => -1,
    };
    { let args = axiom_fs_audit_args(path_len, None); axiom_host_audit("fs_mkdir", args, if result == 0 { "ok" } else { "denied" }); result }
}

#[allow(dead_code)]
fn axiom_fs_mkdir_all(path: String) -> i64 {
    let path_len = path.len();
    let result = match axiom_fs_candidate(&path, true) {
        Some(candidate) => match std::fs::create_dir_all(candidate) {
            Ok(()) => 0,
            Err(_) => -1,
        },
        None => -1,
    };
    { let args = axiom_fs_audit_args(path_len, None); axiom_host_audit("fs_mkdir_all", args, if result == 0 { "ok" } else { "denied" }); result }
}

#[allow(dead_code)]
fn axiom_fs_remove_file(path: String) -> i64 {
    let path_len = path.len();
    let result = match axiom_fs_candidate(&path, false) {
        Some(candidate) => match std::fs::metadata(&candidate) {
            Ok(metadata) if metadata.is_file() => match std::fs::remove_file(candidate) {
                Ok(()) => 0,
                Err(_) => -1,
            },
            _ => -1,
        },
        None => -1,
    };
    { let args = axiom_fs_audit_args(path_len, None); axiom_host_audit("fs_remove_file", args, if result == 0 { "ok" } else { "denied" }); result }
}

#[allow(dead_code)]
fn axiom_fs_remove_dir(path: String) -> i64 {
    let path_len = path.len();
    let result = match axiom_fs_candidate(&path, false) {
        Some(candidate) => match std::fs::metadata(&candidate) {
            Ok(metadata) if metadata.is_dir() => match std::fs::remove_dir(candidate) {
                Ok(()) => 0,
                Err(_) => -1,
            },
            _ => -1,
        },
        None => -1,
    };
    { let args = axiom_fs_audit_args(path_len, None); axiom_host_audit("fs_remove_dir", args, if result == 0 { "ok" } else { "denied" }); result }
}

#[allow(dead_code)]
fn axiom_fs_replace(path: String, content: String) -> i64 {
    let args = axiom_host_arg_summary(&[("path", format!("string:{}", path.len())), ("content", format!("string:{}", content.len()))]);
    let result = (|| {
        if content.len() > AXIOM_MAX_FS_WRITE_BYTES {
            return -1;
        }
        match axiom_fs_candidate(&path, false) {
            Some(candidate) => {
                let Some(parent) = candidate.parent() else {
                    return -1;
                };
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_nanos())
                    .unwrap_or(0);
                let temp = parent.join(format!(".axiom-replace-{}-{stamp}.tmp", std::process::id()));
                match std::fs::write(&temp, content) {
                    Ok(()) => match std::fs::rename(&temp, &candidate) {
                        Ok(()) => 0,
                        Err(_) => {
                            let _ = std::fs::remove_file(&temp);
                            -1
                        }
                    },
                    Err(_) => -1,
                }
            }
            None => -1,
        }
    })();
    axiom_host_audit("fs_replace", args, if result == 0 { "ok" } else { "denied" });
    result
}


"#,
    );
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_is_blocked_network_ip(ip: std::net::IpAddr) -> bool {\n");
    out.push_str("    match ip {\n");
    out.push_str("        std::net::IpAddr::V4(addr) => {\n");
    out.push_str("            let octets = addr.octets();\n");
    out.push_str("            addr.is_private()\n");
    out.push_str("                || addr.is_loopback()\n");
    out.push_str("                || addr.is_link_local()\n");
    out.push_str("                || addr.is_unspecified()\n");
    out.push_str("                || addr.is_broadcast()\n");
    out.push_str("                || addr.is_multicast()\n");
    out.push_str("                || octets[0] == 0\n");
    out.push_str("                || (octets[0] == 100 && (64..=127).contains(&octets[1]))\n");
    out.push_str("                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)\n");
    out.push_str("                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)\n");
    out.push_str("                || (octets[0] == 198 && (18..=19).contains(&octets[1]))\n");
    out.push_str("                || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)\n");
    out.push_str("                || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113)\n");
    out.push_str("        }\n");
    out.push_str("        std::net::IpAddr::V6(addr) => {\n");
    out.push_str("            if let Some(mapped) = addr.to_ipv4_mapped() {\n");
    out.push_str(
        "                return axiom_is_blocked_network_ip(std::net::IpAddr::V4(mapped));\n",
    );
    out.push_str("            }\n");
    out.push_str("            let segments = addr.segments();\n");
    out.push_str("            addr.is_loopback()\n");
    out.push_str("                || addr.is_unspecified()\n");
    out.push_str("                || addr.is_multicast()\n");
    out.push_str("                || (segments[0] & 0xfe00) == 0xfc00\n");
    out.push_str("                || (segments[0] & 0xffc0) == 0xfe80\n");
    out.push_str("                || (segments[0] == 0x2001 && segments[1] == 0x0db8)\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str(
        "fn axiom_resolve_public_socket_addrs(host: &str, port: u16) -> Option<Vec<std::net::SocketAddr>> {\n",
    );
    out.push_str("    use std::net::ToSocketAddrs;\n");
    out.push_str("    let addrs: Vec<std::net::SocketAddr> = (host, port).to_socket_addrs().ok()?.collect();\n");
    out.push_str("    if addrs.is_empty() {\n");
    out.push_str("        return None;\n");
    out.push_str("    }\n");
    out.push_str("    // Network intrinsics reject private, loopback, link-local,\n");
    out.push_str("    // multicast, documentation, and metadata-style addresses.\n");
    out.push_str("    if addrs.iter().any(|addr| axiom_is_blocked_network_ip(addr.ip())) {\n");
    out.push_str("        return None;\n");
    out.push_str("    }\n");
    out.push_str("    Some(addrs)\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_net_host_allowed(host: &str) -> bool {\n");
    out.push_str("    AXIOM_NET_HOST_ALLOWLIST.is_empty() || AXIOM_NET_HOST_ALLOWLIST.iter().any(|allowed| allowed.eq_ignore_ascii_case(host))\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_net_port_allowed(port: u16) -> bool {\n");
    out.push_str(
        "    AXIOM_NET_PORT_ALLOWLIST.is_empty() || AXIOM_NET_PORT_ALLOWLIST.contains(&port)\n",
    );
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_net_resolve(host: String) -> Option<String> {\n");
    out.push_str("    let args = axiom_host_arg_summary(&[(\"host\", format!(\"string:{}\", host.len()))]);\n");
    out.push_str("    let resolved = if axiom_net_host_allowed(host.as_str()) { axiom_resolve_public_socket_addrs(host.as_str(), 0) } else { None }\n");
    out.push_str("        .and_then(|addrs| addrs.into_iter().next())\n");
    out.push_str("        .map(|addr| addr.ip().to_string());\n");
    out.push_str("    axiom_host_audit(\"net_resolve\", args, if resolved.is_some() { \"ok\" } else { \"denied\" });\n");
    out.push_str("    resolved\n");
    out.push_str("}\n\n");
    out.push_str(
        r#"#[allow(dead_code)]
fn axiom_net_timeout(timeout_ms: i64) -> Option<std::time::Duration> {
    Some(std::time::Duration::from_millis(timeout_ms.clamp(1, 30_000) as u64))
}

#[allow(dead_code)]
fn axiom_loopback_socket_addr(host: String, port: i64) -> Option<std::net::SocketAddr> {
    let port = u16::try_from(port).ok()?;
    if !axiom_net_port_allowed(port) || !axiom_net_host_allowed(host.as_str()) {
        return None;
    }
    let ip = match host.as_str() {
        "localhost" => std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        _ => host.parse::<std::net::IpAddr>().ok()?,
    };
    if !ip.is_loopback() {
        return None;
    }
    Some(std::net::SocketAddr::new(ip, port))
}

#[allow(dead_code)]
struct AxiomTcpRegistry {
    next_handle: i64,
    listeners: HashMap<i64, std::net::TcpListener>,
    streams: HashMap<i64, std::net::TcpStream>,
}

#[allow(dead_code)]
impl AxiomTcpRegistry {
    fn new() -> Self {
        Self {
            next_handle: 1,
            listeners: HashMap::new(),
            streams: HashMap::new(),
        }
    }

    fn allocate(&mut self) -> i64 {
        let handle = self.next_handle;
        self.next_handle = self.next_handle.saturating_add(1).max(1);
        handle
    }
}

#[allow(dead_code)]
fn axiom_tcp_registry() -> &'static Mutex<AxiomTcpRegistry> {
    static REGISTRY: std::sync::OnceLock<Mutex<AxiomTcpRegistry>> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(AxiomTcpRegistry::new()))
}

#[allow(dead_code)]
fn axiom_parse_tcp_bind(bind: &str) -> Option<std::net::SocketAddr> {
    if let Ok(addr) = bind.parse::<std::net::SocketAddr>() {
        return addr.ip().is_loopback().then_some(addr);
    }
    let port = bind.strip_prefix("localhost:")?.parse::<u16>().ok()?;
    Some(std::net::SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), port))
}

#[allow(dead_code)]
fn axiom_net_tcp_listen(bind: String) -> i64 {
    let args = axiom_host_arg_summary(&[("bind", format!("string:{}", bind.len()))]);
    let result = (|| {
        let addr = axiom_parse_tcp_bind(bind.as_str())?;
        let listener = std::net::TcpListener::bind(addr).ok()?;
        listener.set_nonblocking(false).ok()?;
        let mut registry = axiom_tcp_registry().lock().ok()?;
        let handle = registry.allocate();
        registry.listeners.insert(handle, listener);
        Some(handle)
    })();
    axiom_host_audit("net_tcp_listen", args, if result.is_some() { "ok" } else { "denied" });
    result.unwrap_or_else(|| axiom_runtime_error("runtime", "net_tcp_listen failed"))
}

#[allow(dead_code)]
fn axiom_net_tcp_listener_port(listener: i64) -> i64 {
    let args = axiom_host_arg_summary(&[("listener", format!("handle:{}", listener))]);
    let result = axiom_tcp_registry()
        .lock()
        .ok()
        .and_then(|registry| registry.listeners.get(&listener).and_then(|listener| listener.local_addr().ok()))
        .map(|addr| i64::from(addr.port()));
    axiom_host_audit("net_tcp_listener_port", args, if result.is_some() { "ok" } else { "denied" });
    result.unwrap_or(-1)
}

#[allow(dead_code)]
fn axiom_net_tcp_accept(listener: i64) -> i64 {
    let args = axiom_host_arg_summary(&[("listener", format!("handle:{}", listener))]);
    let result = (|| {
        let mut registry = axiom_tcp_registry().lock().ok()?;
        let (stream, _peer) = registry.listeners.get(&listener)?.accept().ok()?;
        let handle = registry.allocate();
        registry.streams.insert(handle, stream);
        Some(handle)
    })();
    axiom_host_audit("net_tcp_accept", args, if result.is_some() { "ok" } else { "denied" });
    result.unwrap_or_else(|| axiom_runtime_error("runtime", "net_tcp_accept failed"))
}

#[allow(dead_code)]
fn axiom_net_tcp_read(stream: i64, buf: &mut [u8]) -> i64 {
    use std::io::Read;
    let args = axiom_host_arg_summary(&[("stream", format!("handle:{}", stream)), ("buf", format!("bytes:{}", buf.len()))]);
    let result = axiom_tcp_registry()
        .lock()
        .ok()
        .and_then(|mut registry| registry.streams.get_mut(&stream).and_then(|stream| stream.read(buf).ok()))
        .map(|read| read as i64);
    axiom_host_audit("net_tcp_read", args, if result.is_some() { "ok" } else { "denied" });
    result.unwrap_or(-1)
}

#[allow(dead_code)]
fn axiom_net_tcp_write(stream: i64, buf: &[u8]) -> i64 {
    use std::io::Write;
    let args = axiom_host_arg_summary(&[("stream", format!("handle:{}", stream)), ("buf", format!("bytes:{}", buf.len()))]);
    let result = axiom_tcp_registry()
        .lock()
        .ok()
        .and_then(|mut registry| registry.streams.get_mut(&stream).and_then(|stream| stream.write(buf).ok()))
        .map(|written| written as i64);
    axiom_host_audit("net_tcp_write", args, if result.is_some() { "ok" } else { "denied" });
    result.unwrap_or(-1)
}

#[allow(dead_code)]
fn axiom_net_tcp_close(stream: i64) -> i64 {
    let args = axiom_host_arg_summary(&[("stream", format!("handle:{}", stream))]);
    let closed = axiom_tcp_registry()
        .lock()
        .ok()
        .and_then(|mut registry| registry.streams.remove(&stream))
        .is_some();
    axiom_host_audit("net_tcp_close", args, if closed { "ok" } else { "denied" });
    if closed { 0 } else { -1 }
}

#[allow(dead_code)]
fn axiom_net_tcp_close_listener(listener: i64) -> i64 {
    let args = axiom_host_arg_summary(&[("listener", format!("handle:{}", listener))]);
    let closed = axiom_tcp_registry()
        .lock()
        .ok()
        .and_then(|mut registry| registry.listeners.remove(&listener))
        .is_some();
    axiom_host_audit("net_tcp_close_listener", args, if closed { "ok" } else { "denied" });
    if closed { 0 } else { -1 }
}

#[allow(dead_code)]
fn axiom_net_tcp_listen_loopback_once(response: String, timeout_ms: i64) -> Option<i64> {
    use std::io::{Read, Write};
    let args = axiom_host_arg_summary(&[("response", format!("string:{}", response.len())), ("timeout_ms", format!("int:{}", timeout_ms))]);
    let result = (|| {
        let timeout = axiom_net_timeout(timeout_ms)?;
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).ok()?;
        listener.set_nonblocking(false).ok()?;
        let port = listener.local_addr().ok()?.port();
        std::thread::spawn(move || {
            let _ = listener.set_nonblocking(true);
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
                                    if total_read >= 65_536 { break; }
                                }
                                Err(err) if matches!(err.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => break,
                                Err(_) => break,
                            }
                        }
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.flush();
                        break;
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        if std::time::Instant::now() >= deadline { break; }
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Some(i64::from(port))
    })();
    axiom_host_audit("net_tcp_listen_loopback_once", args, if result.is_some() { "ok" } else { "denied" });
    result
}

#[allow(dead_code)]
fn axiom_net_tcp_dial(host: String, port: i64, message: String, timeout_ms: i64) -> Option<String> {
    use std::io::{Read, Write};
    let args = axiom_host_arg_summary(&[("host", format!("string:{}", host.len())), ("port", format!("int:{}", port)), ("message", format!("string:{}", message.len())), ("timeout_ms", format!("int:{}", timeout_ms))]);
    let result = (|| {
        let timeout = axiom_net_timeout(timeout_ms)?;
        let addr = axiom_loopback_socket_addr(host, port)?;
        let mut stream = std::net::TcpStream::connect_timeout(&addr, timeout).ok()?;
        stream.set_read_timeout(Some(timeout)).ok()?;
        stream.set_write_timeout(Some(timeout)).ok()?;
        stream.write_all(message.as_bytes()).ok()?;
        stream.shutdown(std::net::Shutdown::Write).ok()?;
        let mut response = Vec::new();
        stream.take(64 * 1024).read_to_end(&mut response).ok()?;
        String::from_utf8(response).ok()
    })();
    axiom_host_audit("net_tcp_dial", args, if result.is_some() { "ok" } else { "denied" });
    result
}

#[allow(dead_code)]
fn axiom_net_udp_bind_loopback_once(response: String, timeout_ms: i64) -> Option<i64> {
    let args = axiom_host_arg_summary(&[("response", format!("string:{}", response.len())), ("timeout_ms", format!("int:{}", timeout_ms))]);
    let result = (|| {
        let timeout = axiom_net_timeout(timeout_ms)?;
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
    })();
    axiom_host_audit("net_udp_bind_loopback_once", args, if result.is_some() { "ok" } else { "denied" });
    result
}

#[allow(dead_code)]
fn axiom_net_udp_send_recv(host: String, port: i64, message: String, timeout_ms: i64) -> Option<String> {
    let args = axiom_host_arg_summary(&[("host", format!("string:{}", host.len())), ("port", format!("int:{}", port)), ("message", format!("string:{}", message.len())), ("timeout_ms", format!("int:{}", timeout_ms))]);
    let result = (|| {
        let timeout = axiom_net_timeout(timeout_ms)?;
        let addr = axiom_loopback_socket_addr(host, port)?;
        let socket = std::net::UdpSocket::bind(("127.0.0.1", 0)).ok()?;
        socket.set_read_timeout(Some(timeout)).ok()?;
        socket.set_write_timeout(Some(timeout)).ok()?;
        socket.send_to(message.as_bytes(), addr).ok()?;
        let mut response = vec![0u8; 64 * 1024];
        let (n, _peer) = socket.recv_from(&mut response).ok()?;
        response.truncate(n);
        String::from_utf8(response).ok()
    })();
    axiom_host_audit("net_udp_send_recv", args, if result.is_some() { "ok" } else { "denied" });
    result
}


"#,
    );
    if uses_http_get || uses_http_serve_once || uses_http_serve_route {
        out.push_str(
            r#"#[allow(dead_code)]
fn axiom_http_strip_crlf(value: &str) -> String {
    value.chars().filter(|ch| *ch != '\r' && *ch != '\n').collect()
}

#[allow(dead_code)]
fn axiom_http_split_url(url: &str) -> Option<(&str, &str, u16, &str)> {
    let (scheme, rest, default_port) = if let Some(rest) = url.strip_prefix("http://") {
        ("http", rest, 80u16)
    } else if let Some(rest) = url.strip_prefix("https://") {
        ("https", rest, 443u16)
    } else {
        return None;
    };
    let (host_port, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };
    if host_port.is_empty() {
        return None;
    }
    let (host, port) = match host_port.rfind(':') {
        Some(idx) => {
            let parsed: u16 = host_port[idx + 1..].parse().ok()?;
            (&host_port[..idx], parsed)
        }
        None => (host_port, default_port),
    };
    if host.is_empty() {
        return None;
    }
    Some((scheme, host, port, path))
}

#[allow(dead_code)]
fn axiom_http_request(host: &str, path: &str) -> String {
    format!(
        "GET {} HTTP/1.0\r\nHost: {}\r\nUser-Agent: axiom-stage1/0.1\r\nConnection: close\r\n\r\n",
        path, host
    )
}

#[allow(dead_code)]
fn axiom_http_read_response<R: std::io::Read>(reader: &mut R) -> Option<String> {
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
            if let Some(sep) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                if sep > MAX_HEADER_BYTES {
                    return None;
                }
                body_start = Some(sep + 4);
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
    let sep = body_start - 4;
    let head = &raw[..sep];
    let body = &raw[body_start..];
    let status_line_end = head.iter().position(|b| *b == b'\r').unwrap_or(head.len());
    let status_line = std::str::from_utf8(&head[..status_line_end]).ok()?;
    let mut parts = status_line.splitn(3, ' ');
    let _version = parts.next()?;
    let status_code: u16 = parts.next()?.parse().ok()?;
    if !(200..300).contains(&status_code) {
        return None;
    }
    String::from_utf8(body.to_vec()).ok()
}

#[allow(dead_code)]
fn axiom_https_get_native_tls(host: &str, port: u16, request: &str) -> Result<Vec<u8>, String> {
    #[cfg(target_os = "linux")]
    {
        axiom_openssl_tls_get(host, port, request)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (host, port, request);
        Err(String::from("https TLS is not supported on this platform in stage1"))
    }
}

#[cfg(target_os = "linux")]
#[allow(dead_code)]
fn axiom_openssl_tls_get(host: &str, port: u16, request: &str) -> Result<Vec<u8>, String> {
    use std::ffi::{CStr, CString};
    use std::net::TcpStream;
    use std::os::raw::{c_char, c_int, c_long, c_ulong, c_void};
    use std::os::unix::io::AsRawFd;
    use std::time::Duration;

    #[repr(C)]
    struct SslCtx {
        _private: [u8; 0],
    }
    #[repr(C)]
    struct SslMethod {
        _private: [u8; 0],
    }
    #[repr(C)]
    struct Ssl {
        _private: [u8; 0],
    }

    type TlsClientMethod = unsafe extern "C" fn() -> *const SslMethod;
    type SslCtxNew = unsafe extern "C" fn(*const SslMethod) -> *mut SslCtx;
    type SslCtxFree = unsafe extern "C" fn(*mut SslCtx);
    type SslCtxSetVerify = unsafe extern "C" fn(
        *mut SslCtx,
        c_int,
        Option<unsafe extern "C" fn(c_int, *mut c_void) -> c_int>,
    );
    type SslNew = unsafe extern "C" fn(*mut SslCtx) -> *mut Ssl;
    type SslFree = unsafe extern "C" fn(*mut Ssl);
    type SslSetFd = unsafe extern "C" fn(*mut Ssl, c_int) -> c_int;
    type SslCtrl = unsafe extern "C" fn(*mut Ssl, c_int, c_long, *mut c_void) -> c_long;
    type SslConnect = unsafe extern "C" fn(*mut Ssl) -> c_int;
    type SslWrite = unsafe extern "C" fn(*mut Ssl, *const c_void, c_int) -> c_int;
    type SslRead = unsafe extern "C" fn(*mut Ssl, *mut c_void, c_int) -> c_int;
    type SslShutdown = unsafe extern "C" fn(*mut Ssl) -> c_int;
    type ErrGetError = unsafe extern "C" fn() -> c_ulong;
    type ErrErrorStringN = unsafe extern "C" fn(c_ulong, *mut c_char, usize);

    #[link(name = "dl")]
    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> c_int;
    }

    const RTLD_NOW: c_int = 2;

    struct OpenSsl {
        ssl_handle: *mut c_void,
        crypto_handle: *mut c_void,
        tls_client_method: TlsClientMethod,
        ssl_ctx_new: SslCtxNew,
        ssl_ctx_free: SslCtxFree,
        ssl_ctx_set_verify: SslCtxSetVerify,
        ssl_new: SslNew,
        ssl_free: SslFree,
        ssl_set_fd: SslSetFd,
        ssl_ctrl: SslCtrl,
        ssl_connect: SslConnect,
        ssl_write: SslWrite,
        ssl_read: SslRead,
        ssl_shutdown: SslShutdown,
        err_get_error: ErrGetError,
        err_error_string_n: ErrErrorStringN,
    }

    impl OpenSsl {
        fn load() -> Result<Self, String> {
            let ssl_handle = open_library(&["libssl.so.3", "libssl.so.1.1", "libssl.so"])?;
            let crypto_handle =
                match open_library(&["libcrypto.so.3", "libcrypto.so.1.1", "libcrypto.so"]) {
                    Ok(handle) => handle,
                    Err(err) => {
                        unsafe {
                            let _ = dlclose(ssl_handle);
                        }
                        return Err(err);
                    }
                };
            Ok(Self {
                ssl_handle,
                crypto_handle,
                tls_client_method: load_symbol(ssl_handle, "TLS_client_method")?,
                ssl_ctx_new: load_symbol(ssl_handle, "SSL_CTX_new")?,
                ssl_ctx_free: load_symbol(ssl_handle, "SSL_CTX_free")?,
                ssl_ctx_set_verify: load_symbol(ssl_handle, "SSL_CTX_set_verify")?,
                ssl_new: load_symbol(ssl_handle, "SSL_new")?,
                ssl_free: load_symbol(ssl_handle, "SSL_free")?,
                ssl_set_fd: load_symbol(ssl_handle, "SSL_set_fd")?,
                ssl_ctrl: load_symbol(ssl_handle, "SSL_ctrl")?,
                ssl_connect: load_symbol(ssl_handle, "SSL_connect")?,
                ssl_write: load_symbol(ssl_handle, "SSL_write")?,
                ssl_read: load_symbol(ssl_handle, "SSL_read")?,
                ssl_shutdown: load_symbol(ssl_handle, "SSL_shutdown")?,
                err_get_error: load_symbol(crypto_handle, "ERR_get_error")?,
                err_error_string_n: load_symbol(crypto_handle, "ERR_error_string_n")?,
            })
        }
    }

    impl Drop for OpenSsl {
        fn drop(&mut self) {
            unsafe {
                let _ = dlclose(self.ssl_handle);
                let _ = dlclose(self.crypto_handle);
            }
        }
    }

    fn open_library(candidates: &[&str]) -> Result<*mut c_void, String> {
        for candidate in candidates {
            let name = CString::new(*candidate).map_err(|_| String::from("invalid library name"))?;
            let handle = unsafe { dlopen(name.as_ptr(), RTLD_NOW) };
            if !handle.is_null() {
                return Ok(handle);
            }
        }
        Err(format!(
            "https TLS support requires one of {}",
            candidates.join(", ")
        ))
    }

    fn load_symbol<T: Copy>(handle: *mut c_void, symbol: &str) -> Result<T, String> {
        let name = CString::new(symbol).map_err(|_| String::from("invalid symbol name"))?;
        let value = unsafe { dlsym(handle, name.as_ptr()) };
        if value.is_null() {
            return Err(format!("https TLS support missing OpenSSL symbol {symbol}"));
        }
        Ok(unsafe { std::mem::transmute_copy(&value) })
    }

    struct SslCtxGuard<'a> {
        ctx: *mut SslCtx,
        openssl: &'a OpenSsl,
    }
    impl Drop for SslCtxGuard<'_> {
        fn drop(&mut self) {
            unsafe { (self.openssl.ssl_ctx_free)(self.ctx) };
        }
    }

    struct SslGuard<'a> {
        ssl: *mut Ssl,
        openssl: &'a OpenSsl,
    }
    impl Drop for SslGuard<'_> {
        fn drop(&mut self) {
            unsafe {
                let _ = (self.openssl.ssl_shutdown)(self.ssl);
                (self.openssl.ssl_free)(self.ssl);
            }
        }
    }

    fn openssl_error(openssl: &OpenSsl) -> String {
        let error = unsafe { (openssl.err_get_error)() };
        if error == 0 {
            return String::from("unknown OpenSSL error");
        }
        let mut buf = [0 as c_char; 256];
        unsafe {
            (openssl.err_error_string_n)(error, buf.as_mut_ptr(), buf.len());
            CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
        }
    }

    let addrs = axiom_resolve_public_socket_addrs(host, port)
        .ok_or_else(|| String::from("https target address is not public"))?;
    let mut stream = None;
    for addr in addrs {
        if let Ok(candidate) = TcpStream::connect_timeout(&addr, Duration::from_secs(5)) {
            stream = Some(candidate);
            break;
        }
    }
    let stream = stream.ok_or_else(|| String::from("https TCP connect failed"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|err| format!("https TCP read timeout setup failed: {err}"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|err| format!("https TCP write timeout setup failed: {err}"))?;
    let server_name = CString::new(host).map_err(|_| String::from("https host contains NUL"))?;
    unsafe {
        let openssl = OpenSsl::load()?;
        let method = (openssl.tls_client_method)();
        if method.is_null() {
            return Err(format!(
                "https TLS method unavailable: {}",
                openssl_error(&openssl)
            ));
        }
        let ctx = (openssl.ssl_ctx_new)(method);
        if ctx.is_null() {
            return Err(format!(
                "https TLS context setup failed: {}",
                openssl_error(&openssl)
            ));
        }
        let ctx = SslCtxGuard {
            ctx,
            openssl: &openssl,
        };
        (openssl.ssl_ctx_set_verify)(ctx.ctx, 0, None);

        let ssl = (openssl.ssl_new)(ctx.ctx);
        if ssl.is_null() {
            return Err(format!(
                "https TLS session setup failed: {}",
                openssl_error(&openssl)
            ));
        }
        let ssl = SslGuard {
            ssl,
            openssl: &openssl,
        };
        if (openssl.ssl_set_fd)(ssl.ssl, stream.as_raw_fd()) != 1 {
            return Err(format!(
                "https TLS socket setup failed: {}",
                openssl_error(&openssl)
            ));
        }
        let _ = (openssl.ssl_ctrl)(ssl.ssl, 55, 0, server_name.as_ptr() as *mut c_void);
        if (openssl.ssl_connect)(ssl.ssl) != 1 {
            return Err(format!(
                "https TLS handshake failed: {}",
                openssl_error(&openssl)
            ));
        }

        let request_bytes = request.as_bytes();
        let mut written = 0usize;
        while written < request_bytes.len() {
            let remaining = &request_bytes[written..];
            let chunk_len = remaining.len().min(c_int::MAX as usize) as c_int;
            let n = (openssl.ssl_write)(ssl.ssl, remaining.as_ptr().cast(), chunk_len);
            if n <= 0 {
                return Err(format!(
                    "https TLS request write failed: {}",
                    openssl_error(&openssl)
                ));
            }
            written += n as usize;
        }

        let mut response = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = (openssl.ssl_read)(ssl.ssl, buf.as_mut_ptr().cast(), buf.len() as c_int);
            if n <= 0 {
                break;
            }
            response.extend_from_slice(&buf[..n as usize]);
            if response.len() > 64 * 1024 + 1024 * 1024 + 4 {
                return Err(String::from("https TLS response exceeded size limit"));
            }
        }
        Ok(response)
    }
}

#[allow(dead_code)]
fn axiom_http_get(url: String) -> Option<String> {
    let args = axiom_host_arg_summary(&[("url", format!("string:{}", url.len()))]);
    let result = (|| -> Option<String> {
        use std::io::Write;
        use std::net::TcpStream;
        use std::time::Duration;

        let (scheme, host, port, path) = axiom_http_split_url(&url)?;
        let clean_host = axiom_http_strip_crlf(host);
        let clean_path = axiom_http_strip_crlf(path);
        if clean_host.is_empty() || clean_path.is_empty() {
            return None;
        }
        if !axiom_net_host_allowed(clean_host.as_str()) || !axiom_net_port_allowed(port) {
            return None;
        }
        let request = axiom_http_request(clean_host.as_str(), clean_path.as_str());
        if scheme == "https" {
            let response = match axiom_https_get_native_tls(clean_host.as_str(), port, &request) {
                Ok(response) => response,
                Err(err) => {
                    axiom_runtime_report("net", &err);
                    return None;
                }
            };
            return axiom_http_read_response(&mut response.as_slice());
        }

        let addrs = axiom_resolve_public_socket_addrs(clean_host.as_str(), port)?;
        let mut stream = None;
        for addr in addrs {
            if let Ok(candidate) = TcpStream::connect_timeout(&addr, Duration::from_secs(5)) {
                stream = Some(candidate);
                break;
            }
        }
        let mut stream = stream?;
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
        stream.set_write_timeout(Some(Duration::from_secs(5))).ok()?;
        stream.write_all(request.as_bytes()).ok()?;
        axiom_http_read_response(&mut stream)
    })();
    axiom_host_audit("http_get", args, if result.is_some() { "ok" } else { "denied" });
    result
}

#[allow(dead_code)]
fn axiom_http_read_request<R: std::io::Read>(reader: &mut R) -> Option<String> {
    const MAX_HEADER_BYTES: usize = 64 * 1024;
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = reader.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        raw.extend_from_slice(&buf[..n]);
        if raw.windows(4).any(|w| w == b"\r\n\r\n") {
            let request = String::from_utf8_lossy(&raw);
            let request_line = request.lines().next()?;
            let mut parts = request_line.split_whitespace();
            let method = parts.next()?;
            let path = parts.next()?;
            if method != "GET" && method != "HEAD" {
                return Some(String::from(""));
            }
            return Some(axiom_http_strip_crlf(path));
        }
        if raw.len() > MAX_HEADER_BYTES {
            return None;
        }
    }
}

#[allow(dead_code)]
fn axiom_http_response_with_status(status: &str, body: &str) -> Vec<u8> {
    let body_bytes = body.as_bytes();
    let headers = format!(
        "HTTP/1.0 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body_bytes.len()
    );
    let mut response = headers.into_bytes();
    response.extend_from_slice(body_bytes);
    response
}

#[allow(dead_code)]
fn axiom_http_response(body: &str) -> Vec<u8> {
    axiom_http_response_with_status("200 OK", body)
}

#[allow(dead_code)]
fn axiom_http_loopback_bind_addr(bind: &str) -> Option<std::net::SocketAddr> {
    use std::net::ToSocketAddrs;
    let addrs: Vec<std::net::SocketAddr> = bind.to_socket_addrs().ok()?.collect();
    if addrs.is_empty() || addrs.iter().any(|addr| !addr.ip().is_loopback()) {
        return None;
    }
    addrs.into_iter().next()
}

#[allow(dead_code)]
fn axiom_http_serve_route(bind: String, route_path: String, body: String, max_requests: i64) -> bool {
    let args = axiom_host_arg_summary(&[("bind", format!("string:{}", bind.len())), ("route_path", format!("string:{}", route_path.len())), ("body", format!("string:{}", body.len())), ("max_requests", format!("int:{max_requests}"))]);
    let result = (|| -> bool {
    use std::io::Write;
    use std::net::TcpListener;
    use std::time::Duration;

    if max_requests <= 0 || max_requests > 1024 {
        axiom_runtime_report("net", "http server max_requests must be between 1 and 1024");
        return false;
    }
    let Some(addr) = axiom_http_loopback_bind_addr(bind.as_str()) else {
        axiom_runtime_report("net", "http server bind address must resolve only to loopback");
        return false;
    };
    let listener = match TcpListener::bind(addr) {
        Ok(listener) => listener,
        Err(err) => {
            axiom_runtime_report("net", &format!("http server bind failed: {err}"));
            return false;
        }
    };
    let mut handles = Vec::new();
    for _ in 0..max_requests {
        let (mut stream, _) = match listener.accept() {
            Ok(pair) => pair,
            Err(err) => {
                axiom_runtime_report("net", &format!("http server accept failed: {err}"));
                return false;
            }
        };
        let route_path = route_path.clone();
        let body = body.clone();
        handles.push(std::thread::spawn(move || -> bool {
            if stream.set_read_timeout(Some(Duration::from_secs(5))).is_err() {
                return false;
            }
            if stream.set_write_timeout(Some(Duration::from_secs(5))).is_err() {
                return false;
            }
            let request_path = match axiom_http_read_request(&mut stream) {
                Some(path) => path,
                None => return false,
            };
            let response = if request_path == route_path {
                axiom_http_response(body.as_str())
            } else {
                axiom_http_response_with_status("404 Not Found", "not found")
            };
            stream.write_all(&response).is_ok()
        }));
    }
    let mut ok = true;
    for handle in handles {
        ok = handle.join().unwrap_or(false) && ok;
    }
    ok
    })();
    axiom_host_audit("http_serve_route", args, if result { "ok" } else { "denied" });
    result
}

#[allow(dead_code)]
fn axiom_http_serve_once(bind: String, body: String) -> bool {
    let args = axiom_host_arg_summary(&[("bind", format!("string:{}", bind.len())), ("body", format!("string:{}", body.len()))]);
    let result = (|| -> bool {
    use std::io::Write;
    use std::net::TcpListener;
    use std::time::Duration;

    let Some(addr) = axiom_http_loopback_bind_addr(bind.as_str()) else {
        axiom_runtime_report("net", "http server bind address must resolve only to loopback");
        return false;
    };
    let listener = match TcpListener::bind(addr) {
        Ok(listener) => listener,
        Err(err) => {
            axiom_runtime_report("net", &format!("http server bind failed: {err}"));
            return false;
        }
    };
    let (mut stream, _) = match listener.accept() {
        Ok(pair) => pair,
        Err(err) => {
            axiom_runtime_report("net", &format!("http server accept failed: {err}"));
            return false;
        }
    };
    if stream.set_read_timeout(Some(Duration::from_secs(5))).is_err() {
        axiom_runtime_report("net", "http server read timeout setup failed");
        return false;
    }
    if stream.set_write_timeout(Some(Duration::from_secs(5))).is_err() {
        axiom_runtime_report("net", "http server write timeout setup failed");
        return false;
    }
    if axiom_http_read_request(&mut stream).is_none() {
        axiom_runtime_report("net", "http server request read failed");
        return false;
    }
    let response = axiom_http_response(body.as_str());
    if stream.write_all(&response).is_err() {
        axiom_runtime_report("net", "http server response write failed");
        return false;
    }
    true
    })();
    axiom_host_audit("http_serve_once", args, if result { "ok" } else { "denied" });
    result
}

"#,
        );
    }
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_process_status(program: String) -> i64 {\n");
    out.push_str("    let arg_summary = format!(\"program_len={}\", program.len());\n");
    out.push_str("    let args = axiom_host_arg_summary(&[(\"program\", format!(\"string:{}\", program.len()))]);\n");
    out.push_str("    let status = std::process::Command::new(program)\n");
    out.push_str("        .status()\n");
    out.push_str("        .ok()\n");
    out.push_str("        .and_then(|status| status.code())\n");
    out.push_str("        .map(i64::from)\n");
    out.push_str("        .unwrap_or(-1);\n");
    out.push_str("    axiom_host_audit(\"process_status\", args, if status >= 0 { \"ok\" } else { \"denied\" });\n");
    out.push_str("    axiom_capability_audit(\"process_status\", \"process\", &arg_summary, if status == -1 { \"error\" } else { \"status\" });\n");
    out.push_str("    status\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_clock_now_ms() -> i64 {\n");
    out.push_str("    use std::time::{SystemTime, UNIX_EPOCH};\n");
    out.push_str("    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {\n");
    out.push_str("        Ok(now) => now,\n");
    out.push_str("        Err(_) => axiom_runtime_error(\"runtime\", \"system clock must be after unix epoch\"),\n");
    out.push_str("    };\n");
    out.push_str("    let result = now.as_millis() as i64;\n");
    out.push_str("    axiom_capability_audit(\"clock_now_ms\", \"clock\", \"argc=0\", \"ok\");\n");
    out.push_str("    result\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_clock_elapsed_ms(start_ms: i64) -> i64 {\n");
    out.push_str("    let now = axiom_clock_now_ms();\n");
    out.push_str("    if now < start_ms {\n");
    out.push_str("        return -1;\n");
    out.push_str("    }\n");
    out.push_str("    now - start_ms\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_clock_sleep_ms(milliseconds: i64) -> i64 {\n");
    out.push_str("    if milliseconds < 0 {\n");
    out.push_str("        return -1;\n");
    out.push_str("    }\n");
    out.push_str(
        "    std::thread::sleep(std::time::Duration::from_millis(milliseconds as u64));\n",
    );
    out.push_str("    0\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_env_get(name: String) -> Option<String> {\n");
    out.push_str("    let arg_summary = format!(\"name_len={}\", name.len());\n");
    out.push_str("    let args = axiom_host_arg_summary(&[(\"name\", format!(\"string:{}\", name.len()))]);\n");
    out.push_str(
        "    if !AXIOM_ENV_UNRESTRICTED && !AXIOM_ENV_ALLOWLIST.contains(&name.as_str()) {\n",
    );
    out.push_str("        axiom_host_audit(\"env_get\", args, \"denied\");\n");
    out.push_str(
        "        axiom_capability_audit(\"env_get\", \"env\", &arg_summary, \"denied\");\n",
    );
    out.push_str("        return None;\n");
    out.push_str("    }\n");
    out.push_str("    let value = std::env::var(name).ok();\n");
    out.push_str("    axiom_host_audit(\"env_get\", args, if value.is_some() { \"ok\" } else { \"missing\" });\n");
    out.push_str("    axiom_capability_audit(\"env_get\", \"env\", &arg_summary, if value.is_some() { \"some\" } else { \"none\" });\n");
    out.push_str("    value\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_crypto_sha256(input: String) -> String {\n");
    out.push_str("    let arg_summary = format!(\"input_len={}\", input.len());\n");
    out.push_str(
        "    axiom_capability_audit(\"crypto_sha256\", \"crypto\", &arg_summary, \"ok\");\n",
    );
    out.push_str("    const K: [u32; 64] = [\n");
    out.push_str("        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,\n");
    out.push_str("        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,\n");
    out.push_str("        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,\n");
    out.push_str("        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,\n");
    out.push_str("        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,\n");
    out.push_str("        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,\n");
    out.push_str("        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,\n");
    out.push_str("        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,\n");
    out.push_str("        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,\n");
    out.push_str("        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,\n");
    out.push_str("        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,\n");
    out.push_str("        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,\n");
    out.push_str("        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,\n");
    out.push_str("        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,\n");
    out.push_str("        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,\n");
    out.push_str("        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,\n");
    out.push_str("    ];\n");
    out.push_str("    let mut state: [u32; 8] = [\n");
    out.push_str("        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,\n");
    out.push_str("        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,\n");
    out.push_str("    ];\n");
    out.push_str("    let mut data = input.into_bytes();\n");
    out.push_str("    let bit_len = (data.len() as u64) * 8;\n");
    out.push_str("    data.push(0x80);\n");
    out.push_str("    while data.len() % 64 != 56 {\n");
    out.push_str("        data.push(0);\n");
    out.push_str("    }\n");
    out.push_str("    data.extend_from_slice(&bit_len.to_be_bytes());\n");
    out.push_str("    for chunk in data.chunks(64) {\n");
    out.push_str("        let mut schedule = [0u32; 64];\n");
    out.push_str("        for (index, word) in schedule.iter_mut().take(16).enumerate() {\n");
    out.push_str("            let start = index * 4;\n");
    out.push_str("            *word = u32::from_be_bytes([\n");
    out.push_str("                chunk[start],\n");
    out.push_str("                chunk[start + 1],\n");
    out.push_str("                chunk[start + 2],\n");
    out.push_str("                chunk[start + 3],\n");
    out.push_str("            ]);\n");
    out.push_str("        }\n");
    out.push_str("        for index in 16..64 {\n");
    out.push_str("            let s0 = schedule[index - 15].rotate_right(7)\n");
    out.push_str("                ^ schedule[index - 15].rotate_right(18)\n");
    out.push_str("                ^ (schedule[index - 15] >> 3);\n");
    out.push_str("            let s1 = schedule[index - 2].rotate_right(17)\n");
    out.push_str("                ^ schedule[index - 2].rotate_right(19)\n");
    out.push_str("                ^ (schedule[index - 2] >> 10);\n");
    out.push_str("            schedule[index] = schedule[index - 16]\n");
    out.push_str("                .wrapping_add(s0)\n");
    out.push_str("                .wrapping_add(schedule[index - 7])\n");
    out.push_str("                .wrapping_add(s1);\n");
    out.push_str("        }\n");
    out.push_str("        let mut a = state[0];\n");
    out.push_str("        let mut b = state[1];\n");
    out.push_str("        let mut c = state[2];\n");
    out.push_str("        let mut d = state[3];\n");
    out.push_str("        let mut e = state[4];\n");
    out.push_str("        let mut f = state[5];\n");
    out.push_str("        let mut g = state[6];\n");
    out.push_str("        let mut h = state[7];\n");
    out.push_str("        for index in 0..64 {\n");
    out.push_str(
        "            let sigma1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);\n",
    );
    out.push_str("            let choice = (e & f) ^ ((!e) & g);\n");
    out.push_str("            let temp1 = h\n");
    out.push_str("                .wrapping_add(sigma1)\n");
    out.push_str("                .wrapping_add(choice)\n");
    out.push_str("                .wrapping_add(K[index])\n");
    out.push_str("                .wrapping_add(schedule[index]);\n");
    out.push_str(
        "            let sigma0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);\n",
    );
    out.push_str("            let majority = (a & b) ^ (a & c) ^ (b & c);\n");
    out.push_str("            let temp2 = sigma0.wrapping_add(majority);\n");
    out.push_str("            h = g;\n");
    out.push_str("            g = f;\n");
    out.push_str("            f = e;\n");
    out.push_str("            e = d.wrapping_add(temp1);\n");
    out.push_str("            d = c;\n");
    out.push_str("            c = b;\n");
    out.push_str("            b = a;\n");
    out.push_str("            a = temp1.wrapping_add(temp2);\n");
    out.push_str("        }\n");
    out.push_str("        state[0] = state[0].wrapping_add(a);\n");
    out.push_str("        state[1] = state[1].wrapping_add(b);\n");
    out.push_str("        state[2] = state[2].wrapping_add(c);\n");
    out.push_str("        state[3] = state[3].wrapping_add(d);\n");
    out.push_str("        state[4] = state[4].wrapping_add(e);\n");
    out.push_str("        state[5] = state[5].wrapping_add(f);\n");
    out.push_str("        state[6] = state[6].wrapping_add(g);\n");
    out.push_str("        state[7] = state[7].wrapping_add(h);\n");
    out.push_str("    }\n");
    out.push_str("    let mut output = String::new();\n");
    out.push_str("    for value in state {\n");
    out.push_str("        output.push_str(&format!(\"{value:08x}\"));\n");
    out.push_str("    }\n");
    out.push_str("    output\n");
    out.push_str("}\n\n");
    out.push_str(
        r#"#[allow(dead_code)]
fn axiom_crypto_sha256_bytes(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
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
    let mut output = [0u8; 32];
    for (index, value) in state.iter().enumerate() {
        output[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    output
}

#[allow(dead_code)]
fn axiom_crypto_hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[allow(dead_code)]
fn axiom_crypto_hmac(
    key: String,
    message: String,
    block_len: usize,
    digest_len: usize,
    digest: fn(&[u8]) -> Vec<u8>,
) -> String {
    let mut key_bytes = key.into_bytes();
    if key_bytes.len() > block_len {
        key_bytes = digest(&key_bytes);
    }
    key_bytes.resize(block_len, 0);
    let mut inner = Vec::with_capacity(block_len + message.len());
    let mut outer = Vec::with_capacity(block_len + digest_len);
    for byte in key_bytes {
        inner.push(byte ^ 0x36);
        outer.push(byte ^ 0x5c);
    }
    inner.extend_from_slice(message.as_bytes());
    let inner_digest = digest(&inner);
    outer.extend_from_slice(&inner_digest);
    axiom_crypto_hex_lower(&digest(&outer))
}

#[allow(dead_code)]
fn axiom_crypto_hmac_sha256(key: String, message: String) -> String {
    axiom_crypto_hmac(key, message, 64, 32, |input| {
        axiom_crypto_sha256_bytes(input).to_vec()
    })
}

#[allow(dead_code)]
fn axiom_crypto_hmac_sha512(key: String, message: String) -> String {
    axiom_crypto_hmac(key, message, 128, 64, axiom_crypto_sha512_bytes)
}

#[allow(dead_code)]
fn axiom_crypto_sha512_bytes(input: &[u8]) -> Vec<u8> {
    const K: [u64; 80] = [
        0x428a2f98d728ae22, 0x7137449123ef65cd, 0xb5c0fbcfec4d3b2f, 0xe9b5dba58189dbbc,
        0x3956c25bf348b538, 0x59f111f1b605d019, 0x923f82a4af194f9b, 0xab1c5ed5da6d8118,
        0xd807aa98a3030242, 0x12835b0145706fbe, 0x243185be4ee4b28c, 0x550c7dc3d5ffb4e2,
        0x72be5d74f27b896f, 0x80deb1fe3b1696b1, 0x9bdc06a725c71235, 0xc19bf174cf692694,
        0xe49b69c19ef14ad2, 0xefbe4786384f25e3, 0x0fc19dc68b8cd5b5, 0x240ca1cc77ac9c65,
        0x2de92c6f592b0275, 0x4a7484aa6ea6e483, 0x5cb0a9dcbd41fbd4, 0x76f988da831153b5,
        0x983e5152ee66dfab, 0xa831c66d2db43210, 0xb00327c898fb213f, 0xbf597fc7beef0ee4,
        0xc6e00bf33da88fc2, 0xd5a79147930aa725, 0x06ca6351e003826f, 0x142929670a0e6e70,
        0x27b70a8546d22ffc, 0x2e1b21385c26c926, 0x4d2c6dfc5ac42aed, 0x53380d139d95b3df,
        0x650a73548baf63de, 0x766a0abb3c77b2a8, 0x81c2c92e47edaee6, 0x92722c851482353b,
        0xa2bfe8a14cf10364, 0xa81a664bbc423001, 0xc24b8b70d0f89791, 0xc76c51a30654be30,
        0xd192e819d6ef5218, 0xd69906245565a910, 0xf40e35855771202a, 0x106aa07032bbd1b8,
        0x19a4c116b8d2d0c8, 0x1e376c085141ab53, 0x2748774cdf8eeb99, 0x34b0bcb5e19b48a8,
        0x391c0cb3c5c95a63, 0x4ed8aa4ae3418acb, 0x5b9cca4f7763e373, 0x682e6ff3d6b2b8a3,
        0x748f82ee5defb2fc, 0x78a5636f43172f60, 0x84c87814a1f0ab72, 0x8cc702081a6439ec,
        0x90befffa23631e28, 0xa4506cebde82bde9, 0xbef9a3f7b2c67915, 0xc67178f2e372532b,
        0xca273eceea26619c, 0xd186b8c721c0c207, 0xeada7dd6cde0eb1e, 0xf57d4f7fee6ed178,
        0x06f067aa72176fba, 0x0a637dc5a2c898a6, 0x113f9804bef90dae, 0x1b710b35131c471b,
        0x28db77f523047d84, 0x32caab7b40c72493, 0x3c9ebe0a15c9bebc, 0x431d67c49c100d4c,
        0x4cc5d4becb3e42b6, 0x597f299cfc657e2a, 0x5fcb6fab3ad6faec, 0x6c44198c4a475817,
    ];
    let mut state: [u64; 8] = [
        0x6a09e667f3bcc908, 0xbb67ae8584caa73b, 0x3c6ef372fe94f82b, 0xa54ff53a5f1d36f1,
        0x510e527fade682d1, 0x9b05688c2b3e6c1f, 0x1f83d9abfb41bd6b, 0x5be0cd19137e2179,
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
                chunk[start], chunk[start + 1], chunk[start + 2], chunk[start + 3],
                chunk[start + 4], chunk[start + 5], chunk[start + 6], chunk[start + 7],
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

#[allow(dead_code)]
fn axiom_crypto_constant_time_eq(left: String, right: String) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    axiom_crypto_constant_time_eq_u8(left, right)
}

#[allow(dead_code)]
fn axiom_crypto_constant_time_eq_u8(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&left_byte, &right_byte) in left.iter().zip(right.iter()) {
        diff |= left_byte ^ right_byte;
    }
    diff == 0
}

#[allow(dead_code)]
fn axiom_crypto_fill_random_bytes(buffer: &mut [u8]) -> bool {
    if buffer.is_empty() {
        return true;
    }

    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"))]
    {
        unsafe extern "C" {
            fn arc4random_buf(buf: *mut std::ffi::c_void, nbytes: usize);
        }
        unsafe {
            arc4random_buf(buffer.as_mut_ptr().cast::<std::ffi::c_void>(), buffer.len());
        }
        true
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        unsafe extern "C" {
            fn getrandom(buf: *mut std::ffi::c_void, buflen: usize, flags: u32) -> isize;
        }
        let mut filled = 0usize;
        while filled < buffer.len() {
            let result = unsafe {
                getrandom(
                    buffer[filled..].as_mut_ptr().cast::<std::ffi::c_void>(),
                    buffer.len() - filled,
                    0,
                )
            };
            if result < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return false;
            }
            if result == 0 {
                return false;
            }
            filled += result as usize;
        }
        true
    }

    #[cfg(target_os = "windows")]
    {
        const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x00000002;
        #[link(name = "bcrypt")]
        unsafe extern "system" {
            fn BCryptGenRandom(
                h_algorithm: *mut std::ffi::c_void,
                pb_buffer: *mut u8,
                cb_buffer: u32,
                dw_flags: u32,
            ) -> i32;
        }
        if buffer.len() > u32::MAX as usize {
            return false;
        }
        let status = unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        status >= 0
    }

    #[cfg(not(any(
        target_os = "android",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "ios",
        target_os = "linux",
        target_os = "macos",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "windows"
    )))]
    {
        false
    }
}

#[allow(dead_code)]
fn axiom_crypto_rand_bytes(n: i64) -> Vec<u8> {
    let arg_summary = format!("n={}", n);
    if !(0..=65536).contains(&n) {
        axiom_capability_audit("crypto_rand_bytes", "crypto", &arg_summary, "denied");
        return Vec::new();
    }
    let mut output = vec![0u8; n as usize];
    let status = if axiom_crypto_fill_random_bytes(&mut output) {
        "ok"
    } else {
        output.clear();
        "error"
    };
    axiom_capability_audit("crypto_rand_bytes", "crypto", &arg_summary, status);
    output
}

#[allow(dead_code)]
fn axiom_crypto_rand_u64() -> u64 {
    let mut output = [0u8; 8];
    if axiom_crypto_fill_random_bytes(&mut output) {
        axiom_capability_audit("crypto_rand_u64", "crypto", "n=8", "ok");
        u64::from_ne_bytes(output)
    } else {
        axiom_capability_audit("crypto_rand_u64", "crypto", "n=8", "error");
        0
    }
}

#[allow(dead_code)]
fn axiom_crypto_ed25519_keygen() -> (Vec<u8>, Vec<u8>) {
    match axiom_crypto_ed25519_keygen_inner() {
        Some(keys) => {
            axiom_capability_audit("crypto_ed25519_keygen", "crypto", "key=ed25519", "ok");
            keys
        }
        None => {
            axiom_capability_audit("crypto_ed25519_keygen", "crypto", "key=ed25519", "error");
            (Vec::new(), Vec::new())
        }
    }
}

#[allow(dead_code)]
fn axiom_crypto_ed25519_sign(secret_key: &[u8], message: &[u8]) -> Vec<u8> {
    let arg_summary = format!("secret_len={},message_len={}", secret_key.len(), message.len());
    match axiom_crypto_ed25519_sign_inner(secret_key, message) {
        Some(signature) => {
            axiom_capability_audit("crypto_ed25519_sign", "crypto", &arg_summary, "ok");
            signature
        }
        None => {
            axiom_capability_audit("crypto_ed25519_sign", "crypto", &arg_summary, "error");
            Vec::new()
        }
    }
}

#[allow(dead_code)]
fn axiom_crypto_ed25519_verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    let arg_summary = format!(
        "public_len={},message_len={},signature_len={}",
        public_key.len(),
        message.len(),
        signature.len()
    );
    let ok = axiom_crypto_ed25519_verify_inner(public_key, message, signature);
    axiom_capability_audit(
        "crypto_ed25519_verify",
        "crypto",
        &arg_summary,
        if ok { "ok" } else { "denied" },
    );
    ok
}

#[allow(dead_code)]
fn axiom_crypto_ed25519_keygen_inner() -> Option<(Vec<u8>, Vec<u8>)> {
    let crypto = AxiomEd25519Crypto::load().ok()?;
    let ctx = AxiomPkeyCtxGuard::new(
        unsafe {
            (crypto.evp_pkey_ctx_new_id)(AXIOM_EVP_PKEY_ED25519, std::ptr::null_mut())
        },
        &crypto,
    )?;
    if unsafe { (crypto.evp_pkey_keygen_init)(ctx.ctx) } <= 0 {
        return None;
    }
    let mut pkey: *mut AxiomEvpPkey = std::ptr::null_mut();
    if unsafe { (crypto.evp_pkey_keygen)(ctx.ctx, &mut pkey) } <= 0 || pkey.is_null() {
        return None;
    }
    let pkey = AxiomPkeyGuard { pkey, crypto: &crypto };
    let public = axiom_crypto_ed25519_raw_public_key(&crypto, pkey.pkey)?;
    let private = axiom_crypto_ed25519_raw_private_key(&crypto, pkey.pkey)?;
    let mut secret = private;
    secret.extend_from_slice(&public);
    Some((public, secret))
}

#[allow(dead_code)]
fn axiom_crypto_ed25519_sign_inner(secret_key: &[u8], message: &[u8]) -> Option<Vec<u8>> {
    let private = axiom_crypto_ed25519_private_seed(secret_key)?;
    let crypto = AxiomEd25519Crypto::load().ok()?;
    let pkey = unsafe {
        (crypto.evp_pkey_new_raw_private_key)(
            AXIOM_EVP_PKEY_ED25519,
            std::ptr::null_mut(),
            private.as_ptr(),
            private.len(),
        )
    };
    if pkey.is_null() {
        return None;
    }
    let pkey = AxiomPkeyGuard { pkey, crypto: &crypto };
    let md_ctx = AxiomMdCtxGuard::new(unsafe { (crypto.evp_md_ctx_new)() }, &crypto)?;
    let mut pkey_ctx: *mut AxiomEvpPkeyCtx = std::ptr::null_mut();
    if unsafe {
        (crypto.evp_digest_sign_init)(
            md_ctx.ctx,
            &mut pkey_ctx,
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
            md_ctx.ctx,
            std::ptr::null_mut(),
            &mut signature_len,
            message.as_ptr(),
            message.len(),
        )
    } <= 0
    {
        return None;
    }
    let mut signature = vec![0u8; signature_len];
    if unsafe {
        (crypto.evp_digest_sign)(
            md_ctx.ctx,
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

#[allow(dead_code)]
fn axiom_crypto_ed25519_verify_inner(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    if public_key.len() != 32 || signature.len() != 64 {
        return false;
    }
    let Ok(crypto) = AxiomEd25519Crypto::load() else {
        return false;
    };
    let pkey = unsafe {
        (crypto.evp_pkey_new_raw_public_key)(
            AXIOM_EVP_PKEY_ED25519,
            std::ptr::null_mut(),
            public_key.as_ptr(),
            public_key.len(),
        )
    };
    if pkey.is_null() {
        return false;
    }
    let pkey = AxiomPkeyGuard { pkey, crypto: &crypto };
    let Some(md_ctx) = AxiomMdCtxGuard::new(unsafe { (crypto.evp_md_ctx_new)() }, &crypto) else {
        return false;
    };
    let mut pkey_ctx: *mut AxiomEvpPkeyCtx = std::ptr::null_mut();
    if unsafe {
        (crypto.evp_digest_verify_init)(
            md_ctx.ctx,
            &mut pkey_ctx,
            std::ptr::null(),
            std::ptr::null_mut(),
            pkey.pkey,
        )
    } <= 0
    {
        return false;
    }
    unsafe {
        (crypto.evp_digest_verify)(
            md_ctx.ctx,
            signature.as_ptr(),
            signature.len(),
            message.as_ptr(),
            message.len(),
        ) == 1
    }
}

#[allow(dead_code)]
fn axiom_crypto_ed25519_private_seed(secret_key: &[u8]) -> Option<&[u8]> {
    if secret_key.len() == 32 {
        Some(secret_key)
    } else if secret_key.len() == 64 {
        Some(&secret_key[..32])
    } else {
        None
    }
}

#[allow(dead_code)]
fn axiom_crypto_ed25519_raw_public_key(
    crypto: &AxiomEd25519Crypto,
    pkey: *mut AxiomEvpPkey,
) -> Option<Vec<u8>> {
    let mut len = 0usize;
    if unsafe { (crypto.evp_pkey_get_raw_public_key)(pkey, std::ptr::null_mut(), &mut len) } <= 0 {
        return None;
    }
    let mut bytes = vec![0u8; len];
    if unsafe { (crypto.evp_pkey_get_raw_public_key)(pkey, bytes.as_mut_ptr(), &mut len) } <= 0 {
        return None;
    }
    bytes.truncate(len);
    Some(bytes)
}

#[allow(dead_code)]
fn axiom_crypto_ed25519_raw_private_key(
    crypto: &AxiomEd25519Crypto,
    pkey: *mut AxiomEvpPkey,
) -> Option<Vec<u8>> {
    let mut len = 0usize;
    if unsafe { (crypto.evp_pkey_get_raw_private_key)(pkey, std::ptr::null_mut(), &mut len) } <= 0
    {
        return None;
    }
    let mut bytes = vec![0u8; len];
    if unsafe { (crypto.evp_pkey_get_raw_private_key)(pkey, bytes.as_mut_ptr(), &mut len) } <= 0 {
        return None;
    }
    bytes.truncate(len);
    Some(bytes)
}

const AXIOM_EVP_PKEY_ED25519: std::os::raw::c_int = 1087;

#[repr(C)]
struct AxiomEvpPkey {
    _private: [u8; 0],
}

#[repr(C)]
struct AxiomEvpPkeyCtx {
    _private: [u8; 0],
}

#[repr(C)]
struct AxiomEvpMdCtx {
    _private: [u8; 0],
}

#[allow(dead_code)]
struct AxiomEd25519Crypto {
    handle: *mut std::os::raw::c_void,
    evp_pkey_ctx_new_id: unsafe extern "C" fn(
        std::os::raw::c_int,
        *mut std::os::raw::c_void,
    ) -> *mut AxiomEvpPkeyCtx,
    evp_pkey_ctx_free: unsafe extern "C" fn(*mut AxiomEvpPkeyCtx),
    evp_pkey_keygen_init: unsafe extern "C" fn(*mut AxiomEvpPkeyCtx) -> std::os::raw::c_int,
    evp_pkey_keygen:
        unsafe extern "C" fn(*mut AxiomEvpPkeyCtx, *mut *mut AxiomEvpPkey) -> std::os::raw::c_int,
    evp_pkey_free: unsafe extern "C" fn(*mut AxiomEvpPkey),
    evp_pkey_get_raw_public_key:
        unsafe extern "C" fn(*mut AxiomEvpPkey, *mut u8, *mut usize) -> std::os::raw::c_int,
    evp_pkey_get_raw_private_key:
        unsafe extern "C" fn(*mut AxiomEvpPkey, *mut u8, *mut usize) -> std::os::raw::c_int,
    evp_pkey_new_raw_private_key: unsafe extern "C" fn(
        std::os::raw::c_int,
        *mut std::os::raw::c_void,
        *const u8,
        usize,
    ) -> *mut AxiomEvpPkey,
    evp_pkey_new_raw_public_key: unsafe extern "C" fn(
        std::os::raw::c_int,
        *mut std::os::raw::c_void,
        *const u8,
        usize,
    ) -> *mut AxiomEvpPkey,
    evp_md_ctx_new: unsafe extern "C" fn() -> *mut AxiomEvpMdCtx,
    evp_md_ctx_free: unsafe extern "C" fn(*mut AxiomEvpMdCtx),
    evp_digest_sign_init: unsafe extern "C" fn(
        *mut AxiomEvpMdCtx,
        *mut *mut AxiomEvpPkeyCtx,
        *const std::os::raw::c_void,
        *mut std::os::raw::c_void,
        *mut AxiomEvpPkey,
    ) -> std::os::raw::c_int,
    evp_digest_sign: unsafe extern "C" fn(
        *mut AxiomEvpMdCtx,
        *mut u8,
        *mut usize,
        *const u8,
        usize,
    ) -> std::os::raw::c_int,
    evp_digest_verify_init: unsafe extern "C" fn(
        *mut AxiomEvpMdCtx,
        *mut *mut AxiomEvpPkeyCtx,
        *const std::os::raw::c_void,
        *mut std::os::raw::c_void,
        *mut AxiomEvpPkey,
    ) -> std::os::raw::c_int,
    evp_digest_verify: unsafe extern "C" fn(
        *mut AxiomEvpMdCtx,
        *const u8,
        usize,
        *const u8,
        usize,
    ) -> std::os::raw::c_int,
}

impl AxiomEd25519Crypto {
    fn load() -> Result<Self, String> {
        let handle = axiom_crypto_open_library(&[
            "libcrypto.so.3",
            "libcrypto.so.1.1",
            "libcrypto.so",
            "/opt/homebrew/opt/openssl@3/lib/libcrypto.3.dylib",
            "/usr/local/opt/openssl@3/lib/libcrypto.3.dylib",
            "libcrypto.3.dylib",
        ])?;
        Ok(Self {
            handle,
            evp_pkey_ctx_new_id: axiom_crypto_load_symbol(handle, "EVP_PKEY_CTX_new_id")?,
            evp_pkey_ctx_free: axiom_crypto_load_symbol(handle, "EVP_PKEY_CTX_free")?,
            evp_pkey_keygen_init: axiom_crypto_load_symbol(handle, "EVP_PKEY_keygen_init")?,
            evp_pkey_keygen: axiom_crypto_load_symbol(handle, "EVP_PKEY_keygen")?,
            evp_pkey_free: axiom_crypto_load_symbol(handle, "EVP_PKEY_free")?,
            evp_pkey_get_raw_public_key: axiom_crypto_load_symbol(
                handle,
                "EVP_PKEY_get_raw_public_key",
            )?,
            evp_pkey_get_raw_private_key: axiom_crypto_load_symbol(
                handle,
                "EVP_PKEY_get_raw_private_key",
            )?,
            evp_pkey_new_raw_private_key: axiom_crypto_load_symbol(
                handle,
                "EVP_PKEY_new_raw_private_key",
            )?,
            evp_pkey_new_raw_public_key: axiom_crypto_load_symbol(
                handle,
                "EVP_PKEY_new_raw_public_key",
            )?,
            evp_md_ctx_new: axiom_crypto_load_symbol(handle, "EVP_MD_CTX_new")?,
            evp_md_ctx_free: axiom_crypto_load_symbol(handle, "EVP_MD_CTX_free")?,
            evp_digest_sign_init: axiom_crypto_load_symbol(handle, "EVP_DigestSignInit")?,
            evp_digest_sign: axiom_crypto_load_symbol(handle, "EVP_DigestSign")?,
            evp_digest_verify_init: axiom_crypto_load_symbol(handle, "EVP_DigestVerifyInit")?,
            evp_digest_verify: axiom_crypto_load_symbol(handle, "EVP_DigestVerify")?,
        })
    }
}

impl Drop for AxiomEd25519Crypto {
    fn drop(&mut self) {
        unsafe {
            let _ = axiom_crypto_dlclose(self.handle);
        }
    }
}

struct AxiomPkeyCtxGuard<'a> {
    ctx: *mut AxiomEvpPkeyCtx,
    crypto: &'a AxiomEd25519Crypto,
}

impl<'a> AxiomPkeyCtxGuard<'a> {
    fn new(ctx: *mut AxiomEvpPkeyCtx, crypto: &'a AxiomEd25519Crypto) -> Option<Self> {
        (!ctx.is_null()).then_some(Self { ctx, crypto })
    }
}

impl Drop for AxiomPkeyCtxGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.crypto.evp_pkey_ctx_free)(self.ctx);
        }
    }
}

struct AxiomPkeyGuard<'a> {
    pkey: *mut AxiomEvpPkey,
    crypto: &'a AxiomEd25519Crypto,
}

impl Drop for AxiomPkeyGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.crypto.evp_pkey_free)(self.pkey);
        }
    }
}

struct AxiomMdCtxGuard<'a> {
    ctx: *mut AxiomEvpMdCtx,
    crypto: &'a AxiomEd25519Crypto,
}

impl<'a> AxiomMdCtxGuard<'a> {
    fn new(ctx: *mut AxiomEvpMdCtx, crypto: &'a AxiomEd25519Crypto) -> Option<Self> {
        (!ctx.is_null()).then_some(Self { ctx, crypto })
    }
}

impl Drop for AxiomMdCtxGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.crypto.evp_md_ctx_free)(self.ctx);
        }
    }
}

#[allow(dead_code)]
fn axiom_crypto_open_library(candidates: &[&str]) -> Result<*mut std::os::raw::c_void, String> {
    for candidate in candidates {
        let name = match std::ffi::CString::new(*candidate) {
            Ok(name) => name,
            Err(_) => continue,
        };
        let handle = unsafe { axiom_crypto_dlopen(name.as_ptr(), 2) };
        if !handle.is_null() {
            return Ok(handle);
        }
    }
    Err(format!("Ed25519 support requires one of {}", candidates.join(", ")))
}

#[allow(dead_code)]
fn axiom_crypto_load_symbol<T: Copy>(
    handle: *mut std::os::raw::c_void,
    symbol: &str,
) -> Result<T, String> {
    let name = std::ffi::CString::new(symbol).map_err(|_| String::from("invalid symbol name"))?;
    let value = unsafe { axiom_crypto_dlsym(handle, name.as_ptr()) };
    if value.is_null() {
        return Err(format!("Ed25519 support missing OpenSSL symbol {symbol}"));
    }
    Ok(unsafe { std::mem::transmute_copy(&value) })
}

#[cfg(unix)]
#[link(name = "dl")]
unsafe extern "C" {
    #[link_name = "dlopen"]
    fn axiom_crypto_dlopen(
        filename: *const std::os::raw::c_char,
        flags: std::os::raw::c_int,
    ) -> *mut std::os::raw::c_void;
    #[link_name = "dlsym"]
    fn axiom_crypto_dlsym(
        handle: *mut std::os::raw::c_void,
        symbol: *const std::os::raw::c_char,
    ) -> *mut std::os::raw::c_void;
    #[link_name = "dlclose"]
    fn axiom_crypto_dlclose(handle: *mut std::os::raw::c_void) -> std::os::raw::c_int;
}

"#,
    );
    out.push_str("#[allow(dead_code)]\n");
    out.push_str(
        "fn axiom_map_get<K: Eq + std::hash::Hash, V: Copy>(values: &HashMap<K, V>, key: &K) -> V {\n",
    );
    out.push_str("    match values.get(key) {\n");
    out.push_str("        Some(value) => *value,\n");
    out.push_str("        None => axiom_runtime_error(\"runtime\", \"map key not found\"),\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str(
        "fn axiom_map_take<K: Eq + std::hash::Hash, V>(mut values: HashMap<K, V>, key: &K) -> V {\n",
    );
    out.push_str("    match values.remove(key) {\n");
    out.push_str("        Some(value) => value,\n");
    out.push_str("        None => axiom_runtime_error(\"runtime\", \"map key not found\"),\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_map_lookup<K: Eq + std::hash::Hash, V>(mut values: HashMap<K, V>, key: K) -> Option<V> {\n");
    out.push_str("    values.remove(&key)\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("fn axiom_map_contains_key<K: Eq + std::hash::Hash, V>(values: HashMap<K, V>, key: K) -> bool {\n");
    out.push_str("    values.contains_key(&key)\n");
    out.push_str("}\n\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str(
        "fn axiom_map_keys<K: Eq + std::hash::Hash, V>(values: HashMap<K, V>) -> Vec<K> {\n",
    );
    out.push_str("    values.into_keys().collect()\n");
    out.push_str("}\n\n");
    for enum_def in deterministic_named_refs(&program.enums, |enum_def| enum_def.name.as_str()) {
        render_enum(enum_def, &type_context, &mut out);
        out.push('\n');
    }
    for struct_def in
        deterministic_named_refs(&program.structs, |struct_def| struct_def.name.as_str())
    {
        render_struct(struct_def, &type_context, &mut out);
        out.push('\n');
    }
    for static_def in
        deterministic_named_refs(&program.statics, |static_def| static_def.name.as_str())
    {
        render_static(static_def, &type_context, &mut out);
        out.push('\n');
    }
    for function in deterministic_named_refs(&program.functions, |function| function.name.as_str())
    {
        render_function(function, &type_context, &mut out, debug);
        out.push('\n');
    }
    out.push_str("fn main() -> std::process::ExitCode {\n");
    out.push_str("    axiom_install_panic_hook();\n");
    out.push_str("    let result = panic::catch_unwind(|| {\n");
    let main_mutable_locals = collect_mutably_borrowed_locals(&program.stmts);
    render_stmt_block(
        &program.stmts,
        &type_context,
        &mut out,
        2,
        &program.path,
        false,
        debug,
        &[],
        &main_mutable_locals,
    );
    out.push_str("    });\n");
    out.push_str("    match result {\n");
    out.push_str("        Ok(()) => std::process::ExitCode::SUCCESS,\n");
    out.push_str("        Err(payload) if payload.is::<AxiomRuntimeAbort>() => std::process::ExitCode::from(1),\n");
    out.push_str("        Err(_) => {\n");
    out.push_str("            axiom_runtime_report(\"panic\", \"runtime panic\");\n");
    out.push_str("            std::process::ExitCode::from(1)\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("}\n");
    out
}

fn rust_path_literal(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn program_uses_call(program: &Program, name: &str) -> bool {
    program.stmts.iter().any(|stmt| stmt_uses_call(stmt, name))
        || program
            .functions
            .iter()
            .any(|function| function.body.iter().any(|stmt| stmt_uses_call(stmt, name)))
}

fn stmt_uses_call(stmt: &Stmt, name: &str) -> bool {
    match stmt {
        Stmt::Let { expr, .. }
        | Stmt::Print { expr, .. }
        | Stmt::Defer { expr, .. }
        | Stmt::Return { expr, .. } => expr_uses_call(expr, name),
        Stmt::Panic { message, .. } => expr_uses_call(message, name),
        Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_call(cond, name)
                || then_block.iter().any(|stmt| stmt_uses_call(stmt, name))
                || else_block
                    .as_ref()
                    .is_some_and(|block| block.iter().any(|stmt| stmt_uses_call(stmt, name)))
        }
        Stmt::While { cond, body, .. } => {
            expr_uses_call(cond, name) || body.iter().any(|stmt| stmt_uses_call(stmt, name))
        }
        Stmt::Match { expr, arms, .. } => {
            expr_uses_call(expr, name)
                || arms
                    .iter()
                    .any(|arm| arm.body.iter().any(|stmt| stmt_uses_call(stmt, name)))
        }
        Stmt::Assign { target, expr, .. } => {
            expr_uses_call(target, name) || expr_uses_call(expr, name)
        }
    }
}

fn expr_uses_call(expr: &Expr, name: &str) -> bool {
    match expr {
        Expr::Call {
            name: call_name,
            args,
            ..
        } => call_name == name || args.iter().any(|arg| expr_uses_call(arg, name)),
        Expr::BinaryAdd { lhs, rhs, .. } | Expr::BinaryCompare { lhs, rhs, .. } => {
            expr_uses_call(lhs, name) || expr_uses_call(rhs, name)
        }
        Expr::Cast { expr, .. } => expr_uses_call(expr, name),
        Expr::Try { expr, .. }
        | Expr::Await { expr, .. }
        | Expr::FieldAccess { base: expr, .. }
        | Expr::MutBorrow { expr, .. }
        | Expr::Deref { expr, .. } => expr_uses_call(expr, name),
        Expr::StructLiteral { fields, .. } => {
            fields.iter().any(|field| expr_uses_call(&field.expr, name))
        }
        Expr::TupleLiteral { elements, .. } | Expr::ArrayLiteral { elements, .. } => {
            elements.iter().any(|element| expr_uses_call(element, name))
        }
        Expr::TupleIndex { base, .. } => expr_uses_call(base, name),
        Expr::MapLiteral { entries, .. } => entries
            .iter()
            .any(|entry| expr_uses_call(&entry.key, name) || expr_uses_call(&entry.value, name)),
        Expr::EnumVariant { payloads, .. } => payloads.iter().any(|arg| expr_uses_call(arg, name)),
        Expr::Slice {
            base, start, end, ..
        } => {
            expr_uses_call(base, name)
                || start
                    .as_ref()
                    .is_some_and(|start| expr_uses_call(start, name))
                || end.as_ref().is_some_and(|end| expr_uses_call(end, name))
        }
        Expr::Index { base, index, .. } => {
            expr_uses_call(base, name) || expr_uses_call(index, name)
        }
        Expr::Closure { body, .. } => expr_uses_call(body, name),
        Expr::Match { expr, arms, .. } => {
            expr_uses_call(expr, name) || arms.iter().any(|arm| expr_uses_call(&arm.expr, name))
        }
        Expr::Literal(_) | Expr::VarRef { .. } => false,
        Expr::StringBorrow { expr, .. } => expr_uses_call(expr, name),
    }
}

struct TypeContext<'a> {
    structs: HashMap<&'a str, &'a StructDef>,
    enums: HashMap<&'a str, &'a EnumDef>,
}

impl<'a> TypeContext<'a> {
    fn empty() -> Self {
        Self {
            structs: HashMap::new(),
            enums: HashMap::new(),
        }
    }

    fn new(program: &'a Program) -> Self {
        Self {
            structs: program
                .structs
                .iter()
                .map(|struct_def| (struct_def.name.as_str(), struct_def))
                .collect(),
            enums: program
                .enums
                .iter()
                .map(|enum_def| (enum_def.name.as_str(), enum_def))
                .collect(),
        }
    }

    fn type_contains_borrowed_slice(&self, ty: &Type) -> bool {
        self.type_contains_borrowed_slice_inner(ty, &mut HashSet::new(), &mut HashSet::new())
    }

    fn struct_uses_borrowed_slice(&self, name: &str) -> bool {
        self.type_contains_borrowed_slice(&Type::Struct(name.to_string()))
    }

    fn enum_uses_borrowed_slice(&self, name: &str) -> bool {
        self.type_contains_borrowed_slice(&Type::Enum(name.to_string()))
    }

    fn type_contains_borrowed_slice_inner(
        &self,
        ty: &Type,
        visiting_structs: &mut HashSet<String>,
        visiting_enums: &mut HashSet<String>,
    ) -> bool {
        match ty {
            Type::Never
            | Type::Int
            | Type::Numeric(_)
            | Type::Bool
            | Type::String
            | Type::Ptr(_)
            | Type::MutPtr(_) => false,
            Type::Str => true,
            Type::Slice(_) | Type::MutSlice(_) | Type::MutRef(_) => true,
            Type::Struct(name) => {
                if !visiting_structs.insert(name.clone()) {
                    return false;
                }
                let contains = self.structs.get(name.as_str()).is_some_and(|struct_def| {
                    struct_def.fields.iter().any(|field| {
                        self.type_contains_borrowed_slice_inner(
                            &field.ty,
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
                let contains = self.enums.get(name.as_str()).is_some_and(|enum_def| {
                    enum_def.variants.iter().any(|variant| {
                        variant.payload_tys.iter().any(|payload_ty| {
                            self.type_contains_borrowed_slice_inner(
                                payload_ty,
                                visiting_structs,
                                visiting_enums,
                            )
                        })
                    })
                });
                visiting_enums.remove(name);
                contains
            }
            Type::Option(inner) => {
                self.type_contains_borrowed_slice_inner(inner, visiting_structs, visiting_enums)
            }
            Type::Result(ok, err) => {
                self.type_contains_borrowed_slice_inner(ok, visiting_structs, visiting_enums)
                    || self.type_contains_borrowed_slice_inner(
                        err,
                        visiting_structs,
                        visiting_enums,
                    )
            }
            Type::Tuple(elements) => elements.iter().any(|element| {
                self.type_contains_borrowed_slice_inner(element, visiting_structs, visiting_enums)
            }),
            Type::Map(key, value) => {
                self.type_contains_borrowed_slice_inner(key, visiting_structs, visiting_enums)
                    || self.type_contains_borrowed_slice_inner(
                        value,
                        visiting_structs,
                        visiting_enums,
                    )
            }
            Type::Array(inner, _)
            | Type::Task(inner)
            | Type::JoinHandle(inner)
            | Type::AsyncChannel(inner)
            | Type::SelectResult(inner) => {
                self.type_contains_borrowed_slice_inner(inner, visiting_structs, visiting_enums)
            }
            Type::Fn(params, return_ty) => {
                params.iter().any(|param| {
                    self.type_contains_borrowed_slice_inner(param, visiting_structs, visiting_enums)
                }) || self.type_contains_borrowed_slice_inner(
                    return_ty,
                    visiting_structs,
                    visiting_enums,
                )
            }
        }
    }
}

fn render_static(static_def: &StaticDef, type_context: &TypeContext<'_>, out: &mut String) {
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("#[allow(non_upper_case_globals)]\n");
    out.push_str(&format!(
        "static {}: {} = {};\n",
        static_def.name,
        rust_static_type(&static_def.ty, type_context),
        render_static_expr(&static_def.expr)
    ));
}

fn render_static_expr(expr: &Expr) -> String {
    match expr {
        Expr::Literal(LiteralValue::String(value)) => format!("{value:?}"),
        Expr::BinaryAdd { op, lhs, rhs, .. } => {
            format!(
                "{} {} {}",
                render_static_binary_operand(lhs),
                op.lexeme(),
                render_static_binary_operand(rhs)
            )
        }
        Expr::BinaryCompare { op, lhs, rhs, .. } => {
            format!(
                "{} {} {}",
                render_static_expr(lhs),
                op.lexeme(),
                render_static_expr(rhs)
            )
        }
        _ => render_expr(expr),
    }
}

fn render_static_binary_operand(expr: &Expr) -> String {
    match expr {
        Expr::BinaryAdd { .. } => format!("({})", render_static_expr(expr)),
        _ => render_static_expr(expr),
    }
}

fn rust_static_type(ty: &Type, type_context: &TypeContext<'_>) -> String {
    match ty {
        Type::String => String::from("&'static str"),
        _ => rust_type(ty, type_context),
    }
}

fn render_struct(struct_def: &StructDef, type_context: &TypeContext<'_>, out: &mut String) {
    let lifetime = if type_context.struct_uses_borrowed_slice(&struct_def.name) {
        "<'a>"
    } else {
        ""
    };
    out.push_str("#[allow(non_camel_case_types)]\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("#[derive(Debug, PartialEq)]\n");
    out.push_str(&format!("struct {}{} {{\n", struct_def.name, lifetime));
    for field in &struct_def.fields {
        render_struct_field(field, type_context, out, 1, !lifetime.is_empty());
    }
    out.push_str("}\n");
}

fn render_enum(enum_def: &EnumDef, type_context: &TypeContext<'_>, out: &mut String) {
    let lifetime = if type_context.enum_uses_borrowed_slice(&enum_def.name) {
        "<'a>"
    } else {
        ""
    };
    out.push_str("#[allow(non_camel_case_types)]\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("#[derive(Debug, PartialEq)]\n");
    out.push_str(&format!("enum {}{} {{\n", enum_def.name, lifetime));
    for variant in &enum_def.variants {
        if variant.payload_tys.is_empty() {
            out.push_str(&format!("    {},\n", variant.name));
        } else if !variant.payload_names.is_empty() {
            out.push_str(&format!("    {} {{\n", variant.name));
            for (payload_name, payload_ty) in
                variant.payload_names.iter().zip(variant.payload_tys.iter())
            {
                out.push_str(&format!(
                    "        {}: {},\n",
                    payload_name,
                    rust_type_inner(payload_ty, Some("'a"), type_context)
                ));
            }
            out.push_str("    },\n");
        } else {
            let payload_tys = variant
                .payload_tys
                .iter()
                .map(|payload_ty| rust_type_inner(payload_ty, Some("'a"), type_context))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("    {}({payload_tys}),\n", variant.name));
        }
    }
    out.push_str("}\n");
}

fn render_struct_field(
    field: &StructField,
    type_context: &TypeContext<'_>,
    out: &mut String,
    indent: usize,
    uses_slice_lifetime: bool,
) {
    let pad = "    ".repeat(indent);
    let lifetime = uses_slice_lifetime.then_some("'a");
    out.push_str(&format!(
        "{pad}{}: {},\n",
        field.name,
        rust_type_inner(&field.ty, lifetime, type_context)
    ));
}

fn render_function(
    function: &Function,
    type_context: &TypeContext<'_>,
    out: &mut String,
    debug: bool,
) {
    if function.is_extern {
        render_extern_function(function, type_context, out);
        return;
    }
    let uses_slice_lifetime = function_signature_uses_borrowed_slice(function, type_context);
    let mutable_locals = collect_mutably_borrowed_locals(&function.body);
    let params = function
        .params
        .iter()
        .map(|param| render_param(param, uses_slice_lifetime, type_context, &mutable_locals))
        .collect::<Vec<_>>()
        .join(", ");
    let lifetime = if uses_slice_lifetime { "<'a>" } else { "" };
    out.push_str("#[allow(non_snake_case)]\n");
    out.push_str(&format!(
        "fn {}{}({}) -> {} {{\n",
        function.name,
        lifetime,
        params,
        rust_type_in_signature(&function.return_ty, uses_slice_lifetime, type_context)
    ));
    if function.is_async {
        out.push_str("    axiom_task_deferred(move || {\n");
        render_stmt_block(
            &function.body,
            type_context,
            out,
            2,
            &function.path,
            false,
            debug,
            &[],
            &mutable_locals,
        );
        out.push_str("    })\n");
    } else {
        render_stmt_block(
            &function.body,
            type_context,
            out,
            1,
            &function.path,
            false,
            debug,
            &[],
            &mutable_locals,
        );
    }
    out.push_str("}\n");
}

fn render_extern_function(function: &Function, type_context: &TypeContext<'_>, out: &mut String) {
    let abi = function.extern_abi.as_deref().unwrap_or("C");
    let library = function
        .extern_library
        .as_deref()
        .expect("extern functions require a library");
    let extern_name = format!("{}_extern", function.name);
    out.push_str("#[link(name = ");
    out.push_str(&format!("{:?}", library));
    out.push_str(
        ")]
",
    );
    out.push_str("unsafe extern ");
    out.push_str(&format!("{:?}", abi));
    out.push_str(
        " {
",
    );
    out.push_str("    #[link_name = ");
    out.push_str(&format!("{:?}", function.source_name));
    out.push_str(
        "]
",
    );
    out.push_str("    fn ");
    out.push_str(&extern_name);
    out.push('(');
    out.push_str(
        &function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| format!("arg{index}: {}", rust_ffi_type(&param.ty, type_context)))
            .collect::<Vec<_>>()
            .join(", "),
    );
    out.push_str(") -> ");
    out.push_str(&rust_ffi_type(&function.return_ty, type_context));
    out.push_str(
        ";
}
",
    );
    out.push_str(
        "#[allow(non_snake_case)]
",
    );
    out.push_str(&format!(
        "fn {}({}) -> {} {{
",
        function.name,
        function
            .params
            .iter()
            .map(|param| format!("{}: {}", param.name, rust_type(&param.ty, type_context)))
            .collect::<Vec<_>>()
            .join(", "),
        rust_type(&function.return_ty, type_context)
    ));
    for param in &function.params {
        if matches!(param.ty, Type::String) {
            out.push_str(&format!(
                "    let {}__ffi = CString::new({}).unwrap_or_else(|_| axiom_runtime_error(\"ffi\", \"string argument contains interior NUL byte\"));\n",
                param.name, param.name
            ));
        }
    }
    out.push_str("    unsafe {\n");
    let call_args = function
        .params
        .iter()
        .map(|param| render_ffi_arg(&param.name, &param.ty))
        .collect::<Vec<_>>()
        .join(", ");
    if matches!(function.return_ty, Type::String) {
        out.push_str(&format!(
            "        let value = {extern_name}({call_args});\n"
        ));
        out.push_str("        if value.is_null() {\n");
        out.push_str("            axiom_runtime_error(\"ffi\", \"extern function returned a null string pointer\");\n");
        out.push_str("        }\n");
        out.push_str("        CStr::from_ptr(value).to_string_lossy().into_owned()\n");
    } else {
        out.push_str(&format!(
            "        {extern_name}({call_args})
"
        ));
    }
    out.push_str(
        "    }
",
    );
    out.push_str(
        "}
",
    );
}

fn render_ffi_arg(name: &str, ty: &Type) -> String {
    match ty {
        Type::String => format!("{}__ffi.as_ptr()", name),
        _ => name.to_string(),
    }
}

fn rust_ffi_type(ty: &Type, type_context: &TypeContext<'_>) -> String {
    match ty {
        Type::String => String::from("*const c_char"),
        Type::Ptr(inner) => format!("*const {}", rust_type(inner, type_context)),
        Type::MutPtr(inner) => format!("*mut {}", rust_type(inner, type_context)),
        Type::MutRef(inner) => format!("&mut {}", rust_type(inner, type_context)),
        _ => rust_type(ty, type_context),
    }
}

fn render_param(
    param: &Param,
    uses_slice_lifetime: bool,
    type_context: &TypeContext<'_>,
    mutable_locals: &HashSet<String>,
) -> String {
    let mutability = mutable_locals
        .contains(&param.name)
        .then_some("mut ")
        .unwrap_or("");
    format!(
        "{}{}: {}",
        mutability,
        param.name,
        rust_type_in_signature(&param.ty, uses_slice_lifetime, type_context)
    )
}

fn collect_mutably_borrowed_locals(stmts: &[Stmt]) -> HashSet<String> {
    let mut locals = HashSet::new();
    for stmt in stmts {
        collect_stmt_mutable_borrows(stmt, &mut locals);
    }
    locals
}

fn collect_stmt_mutable_borrows(stmt: &Stmt, locals: &mut HashSet<String>) {
    match stmt {
        Stmt::Let { expr, .. }
        | Stmt::Assign { expr, .. }
        | Stmt::Print { expr, .. }
        | Stmt::Defer { expr, .. }
        | Stmt::Return { expr, .. } => collect_expr_mutable_borrows(expr, locals),
        Stmt::Panic { message, .. } => collect_expr_mutable_borrows(message, locals),
        Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            collect_expr_mutable_borrows(cond, locals);
            for stmt in then_block {
                collect_stmt_mutable_borrows(stmt, locals);
            }
            if let Some(else_block) = else_block {
                for stmt in else_block {
                    collect_stmt_mutable_borrows(stmt, locals);
                }
            }
        }
        Stmt::While { cond, body, .. } => {
            collect_expr_mutable_borrows(cond, locals);
            for stmt in body {
                collect_stmt_mutable_borrows(stmt, locals);
            }
        }
        Stmt::Match { expr, arms, .. } => {
            collect_expr_mutable_borrows(expr, locals);
            for arm in arms {
                for stmt in &arm.body {
                    collect_stmt_mutable_borrows(stmt, locals);
                }
            }
        }
    }
}

fn mutable_borrow_root_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::VarRef { name, .. } => Some(name),
        Expr::FieldAccess { base, .. } | Expr::TupleIndex { base, .. } => {
            mutable_borrow_root_name(base)
        }
        Expr::Index { base, .. } => mutable_borrow_root_name(base),
        _ => None,
    }
}

fn collect_expr_mutable_borrows(expr: &Expr, locals: &mut HashSet<String>) {
    match expr {
        Expr::Slice {
            base,
            start,
            end,
            ty,
        } => {
            if matches!(ty, Type::MutSlice(_)) {
                if let Some(name) = mutable_borrow_root_name(base) {
                    locals.insert(name.to_string());
                }
            }
            collect_expr_mutable_borrows(base, locals);
            if let Some(start) = start {
                collect_expr_mutable_borrows(start, locals);
            }
            if let Some(end) = end {
                collect_expr_mutable_borrows(end, locals);
            }
        }
        Expr::MutBorrow { expr, .. } => {
            if let Some(name) = mutable_borrow_root_name(expr) {
                locals.insert(name.to_string());
            }
            collect_expr_mutable_borrows(expr, locals);
        }
        Expr::Call { args, .. }
        | Expr::TupleLiteral { elements: args, .. }
        | Expr::ArrayLiteral { elements: args, .. } => {
            for arg in args {
                collect_expr_mutable_borrows(arg, locals);
            }
        }
        Expr::BinaryAdd { lhs, rhs, .. } | Expr::BinaryCompare { lhs, rhs, .. } => {
            collect_expr_mutable_borrows(lhs, locals);
            collect_expr_mutable_borrows(rhs, locals);
        }
        Expr::Try { expr, .. }
        | Expr::Await { expr, .. }
        | Expr::Cast { expr, .. }
        | Expr::StringBorrow { expr, .. }
        | Expr::Deref { expr, .. }
        | Expr::FieldAccess { base: expr, .. }
        | Expr::TupleIndex { base: expr, .. } => collect_expr_mutable_borrows(expr, locals),
        Expr::StructLiteral { fields, .. } => {
            for field in fields {
                collect_expr_mutable_borrows(&field.expr, locals);
            }
        }
        Expr::MapLiteral { entries, .. } => {
            for entry in entries {
                collect_expr_mutable_borrows(&entry.key, locals);
                collect_expr_mutable_borrows(&entry.value, locals);
            }
        }
        Expr::EnumVariant { payloads, .. } => {
            for payload in payloads {
                collect_expr_mutable_borrows(payload, locals);
            }
        }
        Expr::Index { base, index, .. } => {
            collect_expr_mutable_borrows(base, locals);
            collect_expr_mutable_borrows(index, locals);
        }
        Expr::Closure { body, .. } => collect_expr_mutable_borrows(body, locals),
        Expr::Match { expr, arms, .. } => {
            collect_expr_mutable_borrows(expr, locals);
            for arm in arms {
                collect_expr_mutable_borrows(&arm.expr, locals);
            }
        }
        Expr::Literal(_) | Expr::VarRef { .. } => {}
    }
}

fn render_stmt_block(
    stmts: &[Stmt],
    type_context: &TypeContext<'_>,
    out: &mut String,
    indent: usize,
    source_path: &str,
    in_async_function: bool,
    debug: bool,
    active_defers: &[(String, SourceSpan)],
    mutable_locals: &HashSet<String>,
) {
    let mut local_defers: Vec<(String, SourceSpan)> = Vec::new();
    for stmt in stmts {
        render_stmt(
            stmt,
            type_context,
            out,
            indent,
            source_path,
            in_async_function,
            debug,
            active_defers,
            mutable_locals,
            &mut local_defers,
        );
    }
    render_deferred_exprs(out, indent, source_path, debug, &local_defers);
}

fn render_deferred_exprs(
    out: &mut String,
    indent: usize,
    source_path: &str,
    debug: bool,
    defers: &[(String, SourceSpan)],
) {
    let pad = "    ".repeat(indent);
    for (expr, span) in defers.iter().rev() {
        render_source_marker(source_path, *span, out, indent, debug);
        out.push_str(&format!(
            "{pad}let _ = {expr};
"
        ));
    }
}

fn render_stmt(
    stmt: &Stmt,
    type_context: &TypeContext<'_>,
    out: &mut String,
    indent: usize,
    source_path: &str,
    in_async_function: bool,
    debug: bool,
    active_defers: &[(String, SourceSpan)],
    mutable_locals: &HashSet<String>,
    local_defers: &mut Vec<(String, SourceSpan)>,
) {
    let pad = "    ".repeat(indent);
    match stmt {
        Stmt::Let {
            name,
            ty,
            expr,
            span,
            ..
        } => {
            render_source_marker(source_path, *span, out, indent, debug);
            let mutability = mutable_locals
                .contains(name)
                .then_some("mut ")
                .unwrap_or("");
            out.push_str(&format!(
                "{pad}let {mutability}{name}: {} = {};
",
                rust_type(ty, type_context),
                render_expr(expr)
            ));
        }
        Stmt::Assign { target, expr, span } => {
            render_source_marker(source_path, *span, out, indent, debug);
            out.push_str(&format!(
                "{pad}{} = {};\n",
                render_assignment_target(target),
                render_expr(expr)
            ));
        }
        Stmt::Print { expr, span } => {
            render_source_marker(source_path, *span, out, indent, debug);
            out.push_str(&format!(
                "{pad}println!(\"{{}}\", {});\n",
                render_expr(expr)
            ));
        }
        Stmt::Panic { message, span } => {
            render_source_marker(source_path, *span, out, indent, debug);
            render_deferred_exprs(out, indent, source_path, debug, local_defers);
            render_deferred_exprs(out, indent, source_path, debug, active_defers);
            out.push_str(&format!(
                "{pad}axiom_panic({});
",
                render_expr(message)
            ));
        }
        Stmt::Defer { expr, span } => {
            render_source_marker(source_path, *span, out, indent, debug);
            local_defers.push((render_expr(expr), *span));
        }
        Stmt::If {
            cond,
            then_block,
            else_block,
            span,
        } => {
            render_source_marker(source_path, *span, out, indent, debug);
            let mut scoped_defers = active_defers.to_vec();
            scoped_defers.extend(local_defers.iter().cloned());
            out.push_str(&format!(
                "{pad}if {} {{
",
                render_expr(cond)
            ));
            render_stmt_block(
                then_block,
                type_context,
                out,
                indent + 1,
                source_path,
                in_async_function,
                debug,
                &scoped_defers,
                mutable_locals,
            );
            if let Some(else_block) = else_block {
                out.push_str(&format!(
                    "{pad}}} else {{
"
                ));
                render_stmt_block(
                    else_block,
                    type_context,
                    out,
                    indent + 1,
                    source_path,
                    in_async_function,
                    debug,
                    &scoped_defers,
                    mutable_locals,
                );
                out.push_str(&format!(
                    "{pad}}}
"
                ));
            } else {
                out.push_str(&format!(
                    "{pad}}}
"
                ));
            }
        }
        Stmt::While { cond, body, span } => {
            render_source_marker(source_path, *span, out, indent, debug);
            let mut scoped_defers = active_defers.to_vec();
            scoped_defers.extend(local_defers.iter().cloned());
            out.push_str(&format!(
                "{pad}while {} {{
",
                render_expr(cond)
            ));
            render_stmt_block(
                body,
                type_context,
                out,
                indent + 1,
                source_path,
                in_async_function,
                debug,
                &scoped_defers,
                mutable_locals,
            );
            out.push_str(&format!(
                "{pad}}}
"
            ));
        }
        Stmt::Match { expr, arms, span } => {
            render_source_marker(source_path, *span, out, indent, debug);
            let mut scoped_defers = active_defers.to_vec();
            scoped_defers.extend(local_defers.iter().cloned());
            out.push_str(&format!(
                "{pad}match {} {{
",
                render_expr(expr)
            ));
            for arm in arms {
                render_match_arm(
                    arm,
                    type_context,
                    out,
                    indent + 1,
                    source_path,
                    in_async_function,
                    debug,
                    &scoped_defers,
                    mutable_locals,
                );
            }
            if arms.iter().all(|arm| arm.enum_name.is_empty()) {
                out.push_str(&format!("{pad}    _ => {{}}\n"));
            }
            out.push_str(&format!(
                "{pad}}}
"
            ));
        }
        Stmt::Return { expr, span } => {
            render_source_marker(source_path, *span, out, indent, debug);
            render_deferred_exprs(out, indent, source_path, debug, local_defers);
            render_deferred_exprs(out, indent, source_path, debug, active_defers);
            if in_async_function {
                out.push_str(&format!(
                    "{pad}return axiom_task_ready({});
",
                    render_expr(expr)
                ));
            } else {
                out.push_str(&format!(
                    "{pad}return {};
",
                    render_expr(expr)
                ));
            }
        }
    }
}

fn render_match_binding(binding: &str, mutable_locals: &HashSet<String>) -> String {
    mutable_locals
        .contains(binding)
        .then(|| format!("mut {binding}"))
        .unwrap_or_else(|| binding.to_string())
}

fn render_match_bindings(bindings: &[String], mutable_locals: &HashSet<String>) -> String {
    bindings
        .iter()
        .map(|binding| render_match_binding(binding, mutable_locals))
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_match_arm(
    arm: &MatchArm,
    type_context: &TypeContext<'_>,
    out: &mut String,
    indent: usize,
    source_path: &str,
    in_async_function: bool,
    debug: bool,
    active_defers: &[(String, SourceSpan)],
    mutable_locals: &HashSet<String>,
) {
    let pad = "    ".repeat(indent);
    if arm.enum_name.is_empty() {
        out.push_str(&format!("{pad}{} => {{\n", arm.variant));
        render_stmt_block(
            &arm.body,
            type_context,
            out,
            indent + 1,
            source_path,
            in_async_function,
            debug,
            active_defers,
            mutable_locals,
        );
        out.push_str(&format!("{pad}}},\n"));
        return;
    }
    if arm.bindings.is_empty() {
        if arm.ignore_payloads {
            out.push_str(&format!(
                "{pad}{}::{} {{ .. }} => {{\n",
                arm.enum_name, arm.variant
            ));
        } else {
            out.push_str(&format!("{pad}{}::{} => {{\n", arm.enum_name, arm.variant));
        }
    } else if arm.is_named {
        out.push_str(&format!(
            "{pad}{}::{} {{ {} }} => {{\n",
            arm.enum_name,
            arm.variant,
            render_match_bindings(&arm.bindings, mutable_locals)
        ));
    } else {
        out.push_str(&format!(
            "{pad}{}::{}({}) => {{\n",
            arm.enum_name,
            arm.variant,
            render_match_bindings(&arm.bindings, mutable_locals)
        ));
    }
    render_stmt_block(
        &arm.body,
        type_context,
        out,
        indent + 1,
        source_path,
        in_async_function,
        debug,
        active_defers,
        mutable_locals,
    );
    out.push_str(&format!("{pad}}},\n"));
}

fn render_source_marker(
    source_path: &str,
    span: SourceSpan,
    out: &mut String,
    indent: usize,
    debug: bool,
) {
    if !debug {
        return;
    }
    let pad = "    ".repeat(indent);
    out.push_str(&format!(
        "{pad}// axiom-source: {}:{}:{}\n",
        source_path, span.line, span.column
    ));
}

fn render_expr(expr: &Expr) -> String {
    match expr {
        Expr::Literal(LiteralValue::Int(value)) => value.to_string(),
        Expr::Literal(LiteralValue::Numeric { raw, ty }) => format!("{raw}{}", ty.as_str()),
        Expr::Literal(LiteralValue::Bool(value)) => value.to_string(),
        Expr::Literal(LiteralValue::String(value)) => format!("String::from({value:?})"),
        Expr::Literal(LiteralValue::Str(value)) => format!("{value:?}"),
        Expr::StringBorrow { expr, .. } => format!("{}.as_str()", render_expr(expr)),
        Expr::VarRef { name, .. } if name == "self" => String::from("self_"),
        Expr::VarRef { name, .. } => name.clone(),
        Expr::Call { name, args, .. } if name == "panic" => {
            format!("axiom_panic({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "assert_true" => {
            format!(
                "{{ let condition = {}; if condition {{ 0i64 }} else {{ axiom_assert_fail(String::from(\"expected condition to be true\"), {}, {}) }} }}",
                render_expr(&args[0]),
                render_expr(&args[1]),
                render_expr(&args[2])
            )
        }
        Expr::Call { name, args, .. } if name == "assert_property" => {
            format!(
                "{{ let name = {}; let holds = {}; if holds {{ 0i64 }} else {{ axiom_assert_fail(format!(\"property {{:?}} failed\", name), {}, {}) }} }}",
                render_expr(&args[0]),
                render_expr(&args[1]),
                render_expr(&args[2]),
                render_expr(&args[3])
            )
        }
        Expr::Call { name, args, .. } if name == "assert_snapshot" => {
            format!(
                "{{ let name = {}; let actual = {}; let expected = {}; if actual == expected {{ 0i64 }} else {{ axiom_assert_fail(format!(\"snapshot {{:?}} mismatch: expected {{:?}}, got {{:?}}\", name, expected, actual), {}, {}) }} }}",
                render_expr(&args[0]),
                render_expr(&args[1]),
                render_expr(&args[2]),
                render_expr(&args[3]),
                render_expr(&args[4])
            )
        }
        Expr::Call { name, args, .. } if name == "assert_contains" => {
            format!(
                "{{ let haystack = {}; let needle = {}; if haystack.contains(&needle) {{ 0i64 }} else {{ axiom_assert_fail(format!(\"expected {{:?}} to contain {{:?}}\", haystack, needle), {}, {}) }} }}",
                render_expr(&args[0]),
                render_expr(&args[1]),
                render_expr(&args[2]),
                render_expr(&args[3])
            )
        }
        Expr::Call { name, args, .. } if name == "assert_eq" => {
            format!(
                "{{ let left = {}; let right = {}; if left == right {{ 0i64 }} else {{ axiom_assert_fail(format!(\"expected left == right, left={{:?}}, right={{:?}}\", left, right), {}, {}) }} }}",
                render_expr(&args[0]),
                render_expr(&args[1]),
                render_expr(&args[2]),
                render_expr(&args[3])
            )
        }
        Expr::Call { name, args, .. } if name == "assert_case_eq" => {
            format!(
                "{{ let name = {}; let left = {}; let right = {}; if left == right {{ 0i64 }} else {{ axiom_assert_fail(format!(\"table case {{:?}} failed: expected {{:?}}, got {{:?}}\", name, right, left), {}, {}) }} }}",
                render_expr(&args[0]),
                render_expr(&args[1]),
                render_expr(&args[2]),
                render_expr(&args[3]),
                render_expr(&args[4])
            )
        }
        Expr::Call { name, args, .. } if name == "assert_ne" => {
            format!(
                "{{ let left = {}; let right = {}; if left != right {{ 0i64 }} else {{ axiom_assert_fail(format!(\"expected left != right, both were {{:?}}\", left), {}, {}) }} }}",
                render_expr(&args[0]),
                render_expr(&args[1]),
                render_expr(&args[2]),
                render_expr(&args[3])
            )
        }
        Expr::Call { name, args, .. } if name == "len" => {
            format!("({}).len() as i64", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "io_eprintln" => {
            format!("axiom_io_eprintln({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "io_readline" => {
            debug_assert!(args.is_empty());
            String::from("axiom_io_readline()")
        }
        Expr::Call { name, args, .. } if name == "io_read_to_string" => {
            debug_assert!(args.is_empty());
            String::from("axiom_io_read_to_string()")
        }
        Expr::Call { name, args, .. } if name == "json_parse_int" => {
            format!("axiom_json_parse_int({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "json_parse_bool" => {
            format!("axiom_json_parse_bool({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "json_parse_string" => {
            format!("axiom_json_parse_string({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "json_parse_field_int" => {
            format!(
                "axiom_json_parse_field_int({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "json_parse_field_bool" => {
            format!(
                "axiom_json_parse_field_bool({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "json_parse_field_string" => {
            format!(
                "axiom_json_parse_field_string({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "json_stringify_int" => {
            format!("axiom_json_stringify_int({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "json_stringify_bool" => {
            format!("axiom_json_stringify_bool({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "json_parse_value" => {
            format!("axiom_json_parse_value({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "json_parse_field_value" => {
            format!(
                "axiom_json_parse_field_value({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "json_stringify_string" => {
            format!("axiom_json_stringify_string({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "json_stringify_value" => {
            format!("axiom_json_stringify_value({})", render_expr(&args[0]))
        }
        Expr::Call { name, .. } if name == "cli_args" => String::from("axiom_cli_args()"),
        Expr::Call { name, .. } if name == "cli_arg_count" => String::from("axiom_cli_arg_count()"),
        Expr::Call { name, args, .. } if name == "cli_arg" => {
            format!("axiom_cli_arg({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if matches!(name.as_str(), "map_get" | "get") => {
            format!(
                "axiom_map_lookup({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }

        Expr::Call { name, args, .. } if name == "get_or_default" => {
            format!(
                "match axiom_map_lookup({}, {}) {{ Some(value) => value, None => {} }}",
                render_expr(&args[0]),
                render_expr(&args[1]),
                render_expr(&args[2])
            )
        }
        Expr::Call { name, args, .. }
            if matches!(name.as_str(), "map_contains_key" | "contains") =>
        {
            format!(
                "axiom_map_contains_key({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if matches!(name.as_str(), "map_keys" | "keys") => {
            format!("axiom_map_keys({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "regex_is_match" => {
            format!(
                "axiom_regex_is_match({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "regex_find" => {
            format!(
                "axiom_regex_find({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "regex_replace_all" => {
            format!(
                "axiom_regex_replace_all({}, {}, {})",
                render_expr(&args[0]),
                render_expr(&args[1]),
                render_expr(&args[2])
            )
        }
        Expr::Call { name, args, .. } if name == "encoding_url_component_encode" => {
            format!("axiom_percent_encode({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "encoding_url_component_decode" => {
            format!("axiom_percent_decode({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "encoding_path_segment_encode" => {
            format!("axiom_percent_encode({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "encoding_url_query_pair_encode" => {
            format!(
                "axiom_query_pair_encode({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "encoding_path_join_segment" => {
            format!(
                "axiom_path_join_segment({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "fs_read" => {
            format!("axiom_fs_read({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "fs_write" => {
            format!(
                "axiom_fs_write({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "fs_create" => {
            format!("axiom_fs_create({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "fs_append" => {
            format!(
                "axiom_fs_append({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "fs_mkdir" => {
            format!("axiom_fs_mkdir({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "fs_mkdir_all" => {
            format!("axiom_fs_mkdir_all({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "fs_remove_file" => {
            format!("axiom_fs_remove_file({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "fs_remove_dir" => {
            format!("axiom_fs_remove_dir({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "fs_replace" => {
            format!(
                "axiom_fs_replace({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "http_get" => {
            format!("axiom_http_get({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "http_serve_once" => {
            format!(
                "axiom_http_serve_once({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "http_serve_route" => {
            format!(
                "axiom_http_serve_route({}, {}, {}, {})",
                render_expr(&args[0]),
                render_expr(&args[1]),
                render_expr(&args[2]),
                render_expr(&args[3])
            )
        }
        Expr::Call { name, args, .. } if name == "net_resolve" => {
            format!("axiom_net_resolve({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "net_tcp_listen" => {
            format!("axiom_net_tcp_listen({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "net_tcp_listener_port" => {
            format!("axiom_net_tcp_listener_port({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "net_tcp_accept" => {
            format!("axiom_net_tcp_accept({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "net_tcp_read" => {
            format!(
                "axiom_net_tcp_read({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "net_tcp_write" => {
            format!(
                "axiom_net_tcp_write({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "net_tcp_close" => {
            format!("axiom_net_tcp_close({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "net_tcp_close_listener" => {
            format!("axiom_net_tcp_close_listener({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "net_tcp_listen_loopback_once" => {
            format!(
                "axiom_net_tcp_listen_loopback_once({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "net_tcp_dial" => {
            format!(
                "axiom_net_tcp_dial({}, {}, {}, {})",
                render_expr(&args[0]),
                render_expr(&args[1]),
                render_expr(&args[2]),
                render_expr(&args[3])
            )
        }
        Expr::Call { name, args, .. } if name == "net_udp_bind_loopback_once" => {
            format!(
                "axiom_net_udp_bind_loopback_once({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "net_udp_send_recv" => {
            format!(
                "axiom_net_udp_send_recv({}, {}, {}, {})",
                render_expr(&args[0]),
                render_expr(&args[1]),
                render_expr(&args[2]),
                render_expr(&args[3])
            )
        }
        Expr::Call { name, args, .. } if name == "process_status" => {
            format!("axiom_process_status({})", render_expr(&args[0]))
        }
        Expr::Call { name, .. } if name == "clock_now_ms" => String::from("axiom_clock_now_ms()"),
        Expr::Call { name, args, .. } if name == "clock_elapsed_ms" => {
            format!("axiom_clock_elapsed_ms({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "clock_sleep_ms" => {
            format!("axiom_clock_sleep_ms({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "env_get" => {
            format!("axiom_env_get({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "crypto_sha256" => {
            format!("axiom_crypto_sha256({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "crypto_hmac_sha256" => {
            format!(
                "axiom_crypto_hmac_sha256({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "crypto_hmac_sha512" => {
            format!(
                "axiom_crypto_hmac_sha512({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "crypto_constant_time_eq" => {
            format!(
                "axiom_crypto_constant_time_eq({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "crypto_constant_time_eq_u8" => {
            format!(
                "axiom_crypto_constant_time_eq_u8({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "crypto_rand_bytes" => {
            format!("axiom_crypto_rand_bytes({})", render_expr(&args[0]))
        }
        Expr::Call { name, .. } if name == "crypto_rand_u64" => {
            String::from("axiom_crypto_rand_u64()")
        }
        Expr::Call { name, .. } if name == "crypto_ed25519_keygen" => {
            String::from("axiom_crypto_ed25519_keygen()")
        }
        Expr::Call { name, args, .. } if name == "crypto_ed25519_sign" => {
            format!(
                "axiom_crypto_ed25519_sign({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "crypto_ed25519_verify" => {
            format!(
                "axiom_crypto_ed25519_verify({}, {}, {})",
                render_expr(&args[0]),
                render_expr(&args[1]),
                render_expr(&args[2])
            )
        }
        Expr::Call { name, args, .. } if name == "async_ready" => {
            format!("axiom_task_ready({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "async_spawn" => {
            format!("axiom_async_spawn({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "async_join" => {
            format!("axiom_async_join({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "async_cancel" => {
            format!("axiom_async_cancel({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "async_is_canceled" => {
            format!("({}).canceled", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "async_timeout" => {
            format!(
                "axiom_async_timeout({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, .. } if name == "async_channel" => String::from("axiom_async_channel()"),
        Expr::Call { name, args, .. } if name == "async_send" => {
            format!(
                "axiom_async_send({}, {})",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "async_recv" => {
            format!("axiom_async_recv({})", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "async_select" => {
            format!(
                "{{ let left = axiom_await({}); if left.is_some() {{ axiom_task_ready(AxiomSelectResult {{ selected: 0, value: left }}) }} else {{ let right = axiom_await({}); axiom_task_ready(AxiomSelectResult {{ selected: 1, value: right }}) }} }}",
                render_expr(&args[0]),
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } if name == "async_selected" => {
            format!("({}).selected", render_expr(&args[0]))
        }
        Expr::Call { name, args, .. } if name == "async_selected_value" => {
            format!("({}).value", render_expr(&args[0]))
        }
        Expr::Call { name, args, ty } if name == "first" => {
            render_collection_edge(&args[0], ty, false)
        }
        Expr::Call { name, args, ty } if name == "last" => {
            render_collection_edge(&args[0], ty, true)
        }
        Expr::Call { name, args, .. } if name.starts_with("__axiom_numeric_") => {
            let method = name.trim_start_matches("__axiom_numeric_");
            format!(
                "({}).{}({})",
                render_expr(&args[0]),
                method,
                render_expr(&args[1])
            )
        }
        Expr::Call { name, args, .. } => {
            let rendered_args = args.iter().map(render_expr).collect::<Vec<_>>().join(", ");
            format!("{name}({rendered_args})")
        }
        Expr::BinaryAdd { op, lhs, rhs, ty } => match ty {
            Type::Int | Type::Numeric(_) => render_numeric_binary(op, lhs, rhs, ty),
            Type::String | Type::Str => format!(
                "format!(\"{{}}{{}}\", {}, {})",
                render_expr(lhs),
                render_expr(rhs)
            ),
            Type::Never => unreachable!("type checker rejects never addition"),
            Type::Bool => unreachable!("type checker rejects bool addition"),
            Type::Struct(_) => unreachable!("type checker rejects struct addition"),
            Type::Enum(_) => unreachable!("type checker rejects enum addition"),
            Type::Ptr(_)
            | Type::MutPtr(_)
            | Type::MutRef(_)
            | Type::Slice(_)
            | Type::MutSlice(_) => {
                unreachable!("type checker rejects slice addition")
            }
            Type::Option(_) => unreachable!("type checker rejects option addition"),
            Type::Result(_, _) => unreachable!("type checker rejects result addition"),
            Type::Tuple(_) => unreachable!("type checker rejects tuple addition"),
            Type::Map(_, _) => unreachable!("type checker rejects map addition"),
            Type::Array(_, _) => unreachable!("type checker rejects array addition"),
            Type::Task(_) => unreachable!("type checker rejects task addition"),
            Type::JoinHandle(_) => unreachable!("type checker rejects join handle addition"),
            Type::AsyncChannel(_) => unreachable!("type checker rejects async channel addition"),
            Type::SelectResult(_) => unreachable!("type checker rejects select result addition"),
            Type::Fn(_, _) => unreachable!("type checker rejects function addition"),
        },
        Expr::BinaryCompare { op, lhs, rhs, .. } => {
            format!("{} {} {}", render_expr(lhs), op.lexeme(), render_expr(rhs))
        }
        Expr::Cast { expr, ty } => format!(
            "({}) as {}",
            render_expr(expr),
            rust_type(ty, &TypeContext::empty())
        ),
        Expr::MutBorrow { expr, .. } => format!("&mut {}", render_expr(expr)),
        Expr::Deref { expr, .. } => format!("*{}", render_expr(expr)),
        Expr::Try { expr, .. } => format!("({})?", render_expr(expr)),
        Expr::Await { expr, .. } => format!("axiom_await({})", render_expr(expr)),
        Expr::StructLiteral { name, fields, .. } => {
            let rendered_fields = fields
                .iter()
                .map(|field| format!("{}: {}", field.name, render_expr(&field.expr)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name} {{ {rendered_fields} }}")
        }
        Expr::FieldAccess { base, field, .. } => format!("({}).{}", render_expr(base), field),
        Expr::TupleLiteral { elements, .. } => {
            let rendered = elements
                .iter()
                .map(render_expr)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({rendered})")
        }
        Expr::TupleIndex { base, index, .. } => format!("({}).{}", render_expr(base), index),
        Expr::MapLiteral { entries, .. } => {
            let rendered = entries
                .iter()
                .map(|entry| {
                    format!(
                        "({}, {})",
                        render_expr(&entry.key),
                        render_expr(&entry.value)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("HashMap::from([{rendered}])")
        }
        Expr::EnumVariant {
            enum_name,
            variant,
            field_names,
            payloads,
            ..
        } => {
            if payloads.is_empty() {
                format!("{enum_name}::{variant}")
            } else if !field_names.is_empty() {
                let rendered_fields = field_names
                    .iter()
                    .zip(payloads.iter())
                    .map(|(field_name, payload)| format!("{field_name}: {}", render_expr(payload)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{enum_name}::{variant} {{ {rendered_fields} }}")
            } else {
                let rendered_payloads = payloads
                    .iter()
                    .map(render_expr)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{enum_name}::{variant}({rendered_payloads})")
            }
        }
        Expr::ArrayLiteral { elements, .. } => {
            let rendered = elements
                .iter()
                .map(render_expr)
                .collect::<Vec<_>>()
                .join(", ");
            format!("vec![{rendered}]")
        }
        Expr::Closure { params, body, .. } => {
            let rendered_params = params
                .iter()
                .map(|param| param.name.clone())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "Box::new(move |{rendered_params}| {{ {} }})",
                render_expr(body)
            )
        }
        Expr::Slice {
            base, start, end, ..
        } => {
            let start = start
                .as_ref()
                .map(|expr| format!("Some({})", render_expr(expr)))
                .unwrap_or_else(|| String::from("None"));
            let end = end
                .as_ref()
                .map(|expr| format!("Some({})", render_expr(expr)))
                .unwrap_or_else(|| String::from("None"));
            match base.ty() {
                Type::Array(_, _) => {
                    if matches!(expr.ty(), Type::MutSlice(_)) {
                        format!(
                            "axiom_slice_view_mut(&mut {}, {}, {})",
                            render_expr(base),
                            start,
                            end
                        )
                    } else {
                        format!(
                            "axiom_slice_view(&{}, {}, {})",
                            render_expr(base),
                            start,
                            end
                        )
                    }
                }
                Type::Slice(_) | Type::MutSlice(_) => {
                    if matches!(expr.ty(), Type::MutSlice(_)) {
                        format!(
                            "axiom_slice_view_mut({}, {}, {})",
                            render_expr(base),
                            start,
                            end
                        )
                    } else {
                        format!(
                            "axiom_slice_view({}, {}, {})",
                            render_expr(base),
                            start,
                            end
                        )
                    }
                }
                _ => unreachable!("type checker rejects slicing non-array values"),
            }
        }
        Expr::Index { base, index, ty } => match base.ty() {
            Type::Array(_, _) => {
                if ty.is_copy() {
                    format!(
                        "axiom_array_get(&{}, {})",
                        render_expr(base),
                        render_expr(index)
                    )
                } else {
                    format!(
                        "axiom_array_take({}, {})",
                        render_expr(base),
                        render_expr(index)
                    )
                }
            }
            Type::Slice(_) => {
                format!(
                    "axiom_array_get({}, {})",
                    render_expr(base),
                    render_expr(index)
                )
            }
            Type::MutSlice(_) => {
                format!(
                    "axiom_array_get(&*{}, {})",
                    render_expr(base),
                    render_expr(index)
                )
            }
            Type::Map(_, _) => {
                if ty.is_copy() {
                    format!(
                        "axiom_map_get(&{}, &{})",
                        render_expr(base),
                        render_expr(index)
                    )
                } else {
                    format!(
                        "axiom_map_take({}, &{})",
                        render_expr(base),
                        render_expr(index)
                    )
                }
            }
            _ => unreachable!("type checker rejects indexing non-collection values"),
        },
        Expr::Match { expr, arms, .. } => {
            let mut rendered = format!("match {} {{ ", render_expr(expr));
            for arm in arms {
                let mut arm_mutable_locals = HashSet::new();
                collect_expr_mutable_borrows(&arm.expr, &mut arm_mutable_locals);
                if arm.bindings.is_empty() {
                    rendered.push_str(&format!(
                        "{}::{} => {}, ",
                        arm.enum_name,
                        arm.variant,
                        render_expr(&arm.expr)
                    ));
                } else if arm.is_named {
                    rendered.push_str(&format!(
                        "{}::{} {{ {} }} => {}, ",
                        arm.enum_name,
                        arm.variant,
                        render_match_bindings(&arm.bindings, &arm_mutable_locals),
                        render_expr(&arm.expr)
                    ));
                } else {
                    rendered.push_str(&format!(
                        "{}::{}({}) => {}, ",
                        arm.enum_name,
                        arm.variant,
                        render_match_bindings(&arm.bindings, &arm_mutable_locals),
                        render_expr(&arm.expr)
                    ));
                }
            }
            rendered.push('}');
            rendered
        }
    }
}

fn render_assignment_target(expr: &Expr) -> String {
    match expr {
        Expr::Index { base, index, .. } if matches!(base.ty(), Type::MutSlice(_)) => {
            format!(
                "*axiom_array_get_mut({}, {})",
                render_expr(base),
                render_expr(index)
            )
        }
        _ => render_expr(expr),
    }
}

fn render_numeric_binary(
    op: &crate::mir::ArithmeticOp,
    lhs: &Expr,
    rhs: &Expr,
    ty: &Type,
) -> String {
    let left = render_binary_operand(lhs);
    let right = render_binary_operand(rhs);
    if !matches!(op, crate::mir::ArithmeticOp::Add) {
        return format!("{} {} {}", left, op.lexeme(), right);
    }
    match ty {
        Type::Int => format!("axiom_numeric_checked_add_i64({left}, {right})"),
        Type::Numeric(crate::syntax::NumericType::I8) => {
            format!("axiom_numeric_checked_add_i8({left}, {right})")
        }
        Type::Numeric(crate::syntax::NumericType::I16) => {
            format!("axiom_numeric_checked_add_i16({left}, {right})")
        }
        Type::Numeric(crate::syntax::NumericType::I32) => {
            format!("axiom_numeric_checked_add_i32({left}, {right})")
        }
        Type::Numeric(crate::syntax::NumericType::I64) => {
            format!("axiom_numeric_checked_add_i64({left}, {right})")
        }
        Type::Numeric(crate::syntax::NumericType::Isize) => {
            format!("axiom_numeric_checked_add_isize({left}, {right})")
        }
        Type::Numeric(
            crate::syntax::NumericType::U8
            | crate::syntax::NumericType::U16
            | crate::syntax::NumericType::U32
            | crate::syntax::NumericType::U64
            | crate::syntax::NumericType::Usize,
        ) => format!("({left}).wrapping_add({right})"),
        Type::Numeric(crate::syntax::NumericType::F32 | crate::syntax::NumericType::F64) => {
            format!("{left} + {right}")
        }
        _ => unreachable!("type checker rejects non-numeric binary arithmetic"),
    }
}

fn render_binary_operand(expr: &Expr) -> String {
    match expr {
        Expr::BinaryAdd { .. } => format!("({})", render_expr(expr)),
        _ => render_expr(expr),
    }
}

fn rust_type(ty: &Type, type_context: &TypeContext<'_>) -> String {
    rust_type_inner(ty, None, type_context)
}

fn rust_type_in_signature(
    ty: &Type,
    uses_slice_lifetime: bool,
    type_context: &TypeContext<'_>,
) -> String {
    if uses_slice_lifetime {
        rust_type_inner(ty, Some("'a"), type_context)
    } else {
        rust_type(ty, type_context)
    }
}

fn rust_type_inner(ty: &Type, lifetime: Option<&str>, type_context: &TypeContext<'_>) -> String {
    match ty {
        Type::Never => String::from("!"),
        Type::Int => String::from("i64"),
        Type::Numeric(numeric) => numeric.as_str().to_string(),
        Type::Bool => String::from("bool"),
        Type::String => String::from("String"),
        Type::Str => match lifetime {
            Some(lifetime) => format!("&{lifetime} str"),
            None => String::from("&str"),
        },
        Type::Struct(name) => {
            if type_context.struct_uses_borrowed_slice(name) {
                format!("{name}<{}>", lifetime.unwrap_or("'_"))
            } else {
                name.clone()
            }
        }
        Type::Enum(name) => {
            if type_context.enum_uses_borrowed_slice(name) {
                format!("{name}<{}>", lifetime.unwrap_or("'_"))
            } else {
                name.clone()
            }
        }
        Type::Ptr(inner) => {
            format!("*const {}", rust_type_inner(inner, lifetime, type_context))
        }
        Type::MutPtr(inner) => {
            format!("*mut {}", rust_type_inner(inner, lifetime, type_context))
        }
        Type::MutRef(inner) => {
            format!("&mut {}", rust_type_inner(inner, lifetime, type_context))
        }
        Type::Slice(inner) => {
            let inner = rust_type_inner(inner, lifetime, type_context);
            match lifetime {
                Some(lifetime) => format!("&{lifetime} [{inner}]"),
                None => format!("&[{inner}]"),
            }
        }
        Type::MutSlice(inner) => {
            let inner = rust_type_inner(inner, lifetime, type_context);
            match lifetime {
                Some(lifetime) => format!("&{lifetime} mut [{inner}]"),
                None => format!("&mut [{inner}]"),
            }
        }
        Type::Option(inner) => {
            format!("Option<{}>", rust_type_inner(inner, lifetime, type_context))
        }
        Type::Result(ok, err) => format!(
            "Result<{}, {}>",
            rust_type_inner(ok, lifetime, type_context),
            rust_type_inner(err, lifetime, type_context)
        ),
        Type::Tuple(elements) => format!(
            "({})",
            elements
                .iter()
                .map(|element| rust_type_inner(element, lifetime, type_context))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::Map(key, value) => format!(
            "HashMap<{}, {}>",
            rust_type_inner(key, lifetime, type_context),
            rust_type_inner(value, lifetime, type_context)
        ),
        Type::Array(inner, _) => format!("Vec<{}>", rust_type_inner(inner, lifetime, type_context)),
        Type::Task(inner) => format!(
            "AxiomTask<{}>",
            rust_type_inner(inner, lifetime, type_context)
        ),
        Type::JoinHandle(inner) => {
            format!(
                "AxiomJoinHandle<{}>",
                rust_type_inner(inner, lifetime, type_context)
            )
        }
        Type::AsyncChannel(inner) => {
            format!(
                "AxiomChannel<{}>",
                rust_type_inner(inner, lifetime, type_context)
            )
        }
        Type::SelectResult(inner) => {
            format!(
                "AxiomSelectResult<{}>",
                rust_type_inner(inner, lifetime, type_context)
            )
        }
        Type::Fn(params, return_ty) => format!(
            "Box<dyn Fn({}) -> {}>",
            params
                .iter()
                .map(|param| rust_type_inner(param, lifetime, type_context))
                .collect::<Vec<_>>()
                .join(", "),
            rust_type_inner(return_ty, lifetime, type_context)
        ),
    }
}

fn function_signature_uses_borrowed_slice(
    function: &Function,
    type_context: &TypeContext<'_>,
) -> bool {
    type_context.type_contains_borrowed_slice(&function.return_ty)
        || function
            .params
            .iter()
            .any(|param| type_context.type_contains_borrowed_slice(&param.ty))
}

fn render_collection_edge(collection: &Expr, result_ty: &Type, from_end: bool) -> String {
    let rendered = render_expr(collection);
    let index = if from_end {
        String::from("axiom_last_index(values.len())")
    } else {
        String::from("0")
    };
    match collection.ty() {
        Type::Array(_, _) => {
            if result_ty.is_copy() {
                format!("{{ let values = &{rendered}; axiom_array_get(values, {index}) }}")
            } else {
                format!(
                    "{{ let values = {rendered}; let index = {index}; axiom_array_take(values, index) }}"
                )
            }
        }
        Type::Slice(_) | Type::MutSlice(_) => format!(
            "{{ let values = &*{rendered}; let index = {index}; axiom_array_get(values, index) }}"
        ),
        _ => unreachable!("type checker rejects first/last on non-collection values"),
    }
}

impl crate::mir::CompareOp {
    fn lexeme(self) -> &'static str {
        match self {
            crate::mir::CompareOp::Eq => "==",
            crate::mir::CompareOp::Ne => "!=",
            crate::mir::CompareOp::Lt => "<",
            crate::mir::CompareOp::Le => "<=",
            crate::mir::CompareOp::Gt => ">",
            crate::mir::CompareOp::Ge => ">=",
        }
    }
}

impl crate::mir::ArithmeticOp {
    fn lexeme(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
        }
    }
}

pub fn compile_native(
    backend: NativeBackendKind,
    generated_rust: &Path,
    binary_path: &Path,
    target: Option<&str>,
    debug: bool,
) -> Result<(), Diagnostic> {
    match backend {
        NativeBackendKind::GeneratedRust => {
            compile_generated_rust(generated_rust, binary_path, target, debug)
        }
    }
}

const BUILD_GENERATED_RUST_COMPILATION_FAILED: &str = "generated_rust_compilation_failed";

fn compile_generated_rust(
    generated_rust: &Path,
    binary_path: &Path,
    target: Option<&str>,
    debug: bool,
) -> Result<(), Diagnostic> {
    let mut command = Command::new("rustc");
    command
        .arg("--crate-name")
        .arg("axiom_stage1_bootstrap")
        .arg("--edition=2024");
    if debug {
        // The generated-rust backend asks rustc for native debuginfo for the
        // Rust shim it compiles. Axiom source spans are emitted separately in
        // the sidecar debug map; rustc path remapping is intentionally not used
        // here because it cannot remap DWARF line-table rows to Axiom line
        // numbers or represent multiple Axiom source files correctly.
        command
            .arg("-C")
            .arg("debuginfo=2")
            .arg("-C")
            .arg("opt-level=0");
    } else {
        command.arg("-O");
    }
    if let Some(target) = target {
        command.arg("--target").arg(target);
    }
    let output = command
        .arg(generated_rust)
        .arg("-o")
        .arg(binary_path)
        .output()
        .map_err(|err| {
            Diagnostic::new("build", format!("failed to invoke rustc: {err}"))
                .with_path(generated_rust.display().to_string())
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = if stderr.trim().is_empty() {
            "rustc failed to produce the requested build artifact".to_string()
        } else {
            format!(
                "rustc failed to produce the requested build artifact: {}",
                stderr.trim()
            )
        };
        return Err(Diagnostic::new("build", message)
            .with_code(BUILD_GENERATED_RUST_COMPILATION_FAILED)
            .with_path(generated_rust.display().to_string()));
    }
    Ok(())
}
