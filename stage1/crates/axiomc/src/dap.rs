use crate::diagnostics::Diagnostic;
use crate::framed_protocol;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, Write};
use std::path::PathBuf;

const LOCALS_VARIABLES_REFERENCE: i64 = 1;
const NATIVE_DEBUG_STATUS_SCHEMA_VERSION: &str = "axiom.native_debug_status.v1";
const SOURCE_SIMULATOR_MODE: &str = "source-simulator";


#[derive(Debug, Clone, PartialEq)]
pub struct DapResponse {
    pub messages: Vec<Value>,
    pub exit: bool,
}

#[derive(Debug, Clone)]
struct Breakpoint {
    id: i64,
    line: i64,
    source_resolved: bool,
}

#[derive(Debug, Clone)]
struct Variable {
    name: String,
    value: String,
    type_name: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum RuntimeState {
    #[default]
    NotStarted,
    SourceSimulationStopped,
    SourceSimulationTerminated,
}

impl RuntimeState {
    fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::SourceSimulationStopped => "source_simulation_stopped",
            Self::SourceSimulationTerminated => "source_simulation_terminated",
        }
    }
}

#[derive(Debug, Default)]
pub struct DapSession {
    next_seq: i64,
    next_breakpoint_id: i64,
    program: Option<PathBuf>,
    source_lines: Vec<String>,
    breakpoints: BTreeMap<String, Vec<Breakpoint>>,
    locals: Vec<Variable>,
    current_line: i64,
    source_generation: Option<String>,
    runtime_state: RuntimeState,
}

impl DapSession {
    pub fn handle_message(&mut self, payload: &str) -> Result<DapResponse, Diagnostic> {
        let request: Value = serde_json::from_str(payload)
            .map_err(|err| Diagnostic::new("dap", format!("invalid DAP payload: {err}")))?;
        let command = request.get("command").and_then(Value::as_str).unwrap_or("");
        let request_seq = request.get("seq").and_then(Value::as_i64).unwrap_or(0);
        let arguments = request
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let mut messages = Vec::new();
        let mut exit = false;
        match command {
            "initialize" => {
                let body = self.initialize_body();
                messages.push(self.success_response(request_seq, command, body));
                messages.push(self.event("initialized", json!({})));
            }
            "launch" => match self.launch(&arguments) {
                Ok(()) => {
                    let status = self.debug_status();
                    messages.push(self.success_response(
                        request_seq,
                        command,
                        json!({ "axiomDebugging": status }),
                    ));
                    let status = self.debug_status();
                    messages.push(self.event(
                        "stopped",
                        json!({
                            "reason": "entry",
                            "description": "AxiOM source simulation only; no process was launched or stopped",
                            "threadId": 1,
                            "allThreadsStopped": true,
                            "axiomDebugging": status
                        }),
                    ));
                }
                Err(error) => {
                    messages.push(self.error_response(request_seq, command, error.message))
                }
            },
            "setBreakpoints" => {
                let breakpoints = self.set_breakpoints(&arguments);
                messages.push(self.success_response(
                    request_seq,
                    command,
                    json!({ "breakpoints": breakpoints }),
                ));
            }
            "configurationDone" => {
                messages.push(self.success_response(request_seq, command, json!({})))
            }
            "threads" => {
                if !self.source_simulation_active() {
                    messages.push(self.error_response(
                        request_seq,
                        command,
                        observation_unavailable_message(command),
                    ));
                } else {
                    let status = self.debug_status();
                    messages.push(self.success_response(
                        request_seq,
                        command,
                        json!({
                            "threads": [{
                                "id": 1,
                                "name": "axiom source simulator",
                                "axiomRuntimeVerified": false
                            }],
                            "axiomDebugging": status
                        }),
                    ));
                }
            }
            "stackTrace" => {
                if !self.source_simulation_active() {
                    messages.push(self.error_response(
                        request_seq,
                        command,
                        observation_unavailable_message(command),
                    ));
                } else {
                    let status = self.debug_status();
                    messages.push(self.success_response(
                        request_seq,
                        command,
                        json!({
                            "stackFrames": [self.stack_frame()],
                            "totalFrames": 1,
                            "axiomDebugging": status
                        }),
                    ));
                }
            }
            "scopes" => {
                if !self.source_simulation_active() {
                    messages.push(self.error_response(
                        request_seq,
                        command,
                        observation_unavailable_message(command),
                    ));
                } else {
                    let status = self.debug_status();
                    messages.push(self.success_response(
                        request_seq,
                        command,
                        json!({
                            "scopes": [{
                                "name": "Source-simulated locals",
                                "variablesReference": LOCALS_VARIABLES_REFERENCE,
                                "expensive": false,
                                "axiomRuntimeVerified": false
                            }],
                            "axiomDebugging": status
                        }),
                    ));
                }
            }
            "variables" => {
                if !self.source_simulation_active() {
                    messages.push(self.error_response(
                        request_seq,
                        command,
                        observation_unavailable_message(command),
                    ));
                } else {
                    let variables = self.variables(&arguments);
                    let status = self.debug_status();
                    messages.push(self.success_response(
                        request_seq,
                        command,
                        json!({ "variables": variables, "axiomDebugging": status }),
                    ));
                }
            }
            "continue" => {
                if !self.source_simulation_active() {
                    messages.push(self.error_response(
                        request_seq,
                        command,
                        "continue requires an active source-simulator session; no process-backed runtime is available"
                            .to_string(),
                    ));
                    return Ok(DapResponse { messages, exit });
                }
                messages.push(self.success_response(
                    request_seq,
                    command,
                    json!({ "allThreadsContinued": true }),
                ));
                if self.advance_to_next_source_location() {
                    let status = self.debug_status();
                    messages.push(self.event(
                        "stopped",
                        json!({
                            "reason": "step",
                            "description": "Source simulation reached a requested line; no native breakpoint was installed",
                            "threadId": 1,
                            "allThreadsStopped": true,
                            "axiomDebugging": status
                        }),
                    ));
                } else {
                    self.runtime_state = RuntimeState::SourceSimulationTerminated;
                    let status = self.debug_status();
                    messages.push(self.event(
                        "terminated",
                        json!({ "axiomDebugging": status }),
                    ));
                }
            }
            "next" | "stepIn" | "stepOut" => {
                if !self.source_simulation_active() {
                    messages.push(self.error_response(
                        request_seq,
                        command,
                        format!(
                            "DAP command {command:?} requires an active source-simulator session; no process-backed runtime is available"
                        ),
                    ));
                    return Ok(DapResponse { messages, exit });
                }
                messages.push(self.success_response(request_seq, command, json!({})));
                if self.step_one_line() {
                    let status = self.debug_status();
                    messages.push(self.event(
                        "stopped",
                        json!({
                            "reason": "step",
                            "description": "AxiOM source simulation only; no process instruction was stepped",
                            "threadId": 1,
                            "allThreadsStopped": true,
                            "axiomDebugging": status
                        }),
                    ));
                } else {
                    self.runtime_state = RuntimeState::SourceSimulationTerminated;
                    let status = self.debug_status();
                    messages.push(self.event(
                        "terminated",
                        json!({ "axiomDebugging": status }),
                    ));
                }
            }
            "axiom/debugStatus" => {
                let status = self.debug_status();
                messages.push(self.success_response(request_seq, command, status));
            }
            "attach" | "pause" | "terminate" => messages.push(self.error_response(
                request_seq,
                command,
                format!(
                    "DAP command {command:?} requires process-backed native debugging, which is not implemented"
                ),
            )),
            "disconnect" => {
                self.runtime_state = RuntimeState::SourceSimulationTerminated;
                messages.push(self.success_response(request_seq, command, json!({})));
                exit = true;
            }
            other => messages.push(self.error_response(
                request_seq,
                command,
                format!("unsupported DAP command {other:?}"),
            )),
        }

        Ok(DapResponse { messages, exit })
    }

    fn launch(&mut self, arguments: &Value) -> Result<(), Diagnostic> {
        self.breakpoints.clear();
        if self.program.is_some() {
            self.program = None;
            self.source_lines.clear();
            self.locals.clear();
            self.current_line = 0;
            self.source_generation = None;
            self.runtime_state = RuntimeState::SourceSimulationTerminated;
        }
        let mode = arguments.get("mode").and_then(Value::as_str);
        if mode != Some(SOURCE_SIMULATOR_MODE) {
            return Err(Diagnostic::new(
                "dap",
                "process-backed launch is not implemented; pass `mode: \"source-simulator\"` to opt into the non-runtime source simulator",
            ));
        }
        let program = arguments
            .get("program")
            .and_then(Value::as_str)
            .ok_or_else(|| Diagnostic::new("dap", "launch requires a string `program` argument"))?;
        let path = PathBuf::from(program);
        if path.extension().and_then(|extension| extension.to_str()) != Some("ax") {
            return Err(Diagnostic::new(
                "dap",
                "source-simulator mode accepts only an `.ax` source file; it does not launch native binaries",
            ));
        }
        let source = fs::read_to_string(&path).map_err(|err| {
            Diagnostic::new(
                "dap",
                format!("failed to read program {}: {err}", path.display()),
            )
            .with_path(path.display().to_string())
        })?;
        self.source_lines = source.lines().map(str::to_string).collect();
        self.locals = collect_static_locals(&source);
        self.current_line = first_executable_line(&self.source_lines).unwrap_or(1);
        self.source_generation = Some(format!("sha256:{:x}", Sha256::digest(source.as_bytes())));
        self.runtime_state = RuntimeState::SourceSimulationStopped;
        self.program = Some(path);
        Ok(())
    }

    fn set_breakpoints(&mut self, arguments: &Value) -> Value {
        let path = arguments
            .get("source")
            .and_then(|source| source.get("path"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let line_count = self
            .line_count_for_source(&path)
            .unwrap_or_else(|| self.source_lines.len().max(1) as i64);
        let breakpoints = arguments
            .get("breakpoints")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|breakpoint| breakpoint.get("line").and_then(Value::as_i64))
            .map(|line| {
                let id = self.next_breakpoint_id();
                Breakpoint {
                    id,
                    line,
                    source_resolved: line >= 1 && line <= line_count,
                }
            })
            .collect::<Vec<_>>();
        let body = breakpoints
            .iter()
            .map(|breakpoint| {
                json!({
                    "id": breakpoint.id,
                    "verified": false,
                    "line": breakpoint.line,
                    "message": if breakpoint.source_resolved {
                        "Source line exists, but no process-backed native breakpoint was installed"
                    } else {
                        "Source line is outside the known source range"
                    },
                    "axiomSourceResolved": breakpoint.source_resolved
                })
            })
            .collect::<Vec<_>>();
        self.breakpoints.insert(path, breakpoints);
        json!(body)
    }

    fn line_count_for_source(&self, path: &str) -> Option<i64> {
        if path.is_empty() {
            return None;
        }
        if self
            .program
            .as_ref()
            .is_some_and(|program| program.display().to_string() == path)
            && !self.source_lines.is_empty()
        {
            return Some(self.source_lines.len() as i64);
        }
        fs::read_to_string(path)
            .ok()
            .map(|source| source.lines().count().max(1) as i64)
    }

    fn stack_frame(&self) -> Value {
        let source_path = self
            .program
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        json!({
            "id": 1,
            "name": "main",
            "line": self.current_line.max(1),
            "column": 1,
            "source": {
                "name": self.program.as_ref().and_then(|path| path.file_name()).and_then(|name| name.to_str()).unwrap_or("axiom program"),
                "path": source_path
            },
            "presentationHint": "subtle",
            "axiomRuntimeVerified": false
        })
    }

    fn variables(&self, arguments: &Value) -> Value {
        if arguments.get("variablesReference").and_then(Value::as_i64)
            != Some(LOCALS_VARIABLES_REFERENCE)
        {
            return json!([]);
        }
        json!(
            self.locals
                .iter()
                .map(|local| json!({
                    "name": local.name,
                    "value": local.value,
                    "type": local.type_name,
                    "variablesReference": 0,
                    "axiomRuntimeVerified": false
                }))
                .collect::<Vec<_>>()
        )
    }

    fn advance_to_next_source_location(&mut self) -> bool {
        let Some(program) = &self.program else {
            return false;
        };
        let path = program.display().to_string();
        let Some(breakpoints) = self.breakpoints.get(&path) else {
            return false;
        };
        if let Some(next) = breakpoints
            .iter()
            .filter(|breakpoint| breakpoint.source_resolved && breakpoint.line > self.current_line)
            .map(|breakpoint| breakpoint.line)
            .min()
        {
            self.current_line = next;
            true
        } else {
            false
        }
    }

    fn source_simulation_active(&self) -> bool {
        self.runtime_state == RuntimeState::SourceSimulationStopped && self.program.is_some()
    }

    fn step_one_line(&mut self) -> bool {
        let max_line = self.source_lines.len().max(1) as i64;
        if self.current_line >= max_line {
            return false;
        }
        self.current_line += 1;
        true
    }

    fn initialize_body(&self) -> Value {
        json!({
            "adapterID": "axiom",
            "supportsConfigurationDoneRequest": true,
            "supportsStepInTargetsRequest": false,
            "supportsSetVariable": false,
            "supportsEvaluateForHovers": false,
            "supportsExceptionInfoRequest": false,
            "supportsAxiomDebugStatusRequest": true,
            "axiomDebugging": self.debug_status()
        })
    }

    fn debug_status(&self) -> Value {
        json!({
            "schemaVersion": NATIVE_DEBUG_STATUS_SCHEMA_VERSION,
            "mode": "source_simulator",
            "processBacked": false,
            "nativeAxiomDwarf": false,
            "profileSymbolization": false,
            "runtimeState": self.runtime_state.as_str(),
            "identity": {
                "binaryDigest": Value::Null,
                "sourceGeneration": self.source_generation,
                "target": Value::Null
            },
            "unavailableReason": "native_debugging.dependencies_unmet",
            "blockerIssues": [1436, 1455]
        })
    }

    fn success_response(&mut self, request_seq: i64, command: &str, body: Value) -> Value {
        json!({
            "seq": self.next_seq(),
            "type": "response",
            "request_seq": request_seq,
            "success": true,
            "command": command,
            "body": body
        })
    }

    fn error_response(&mut self, request_seq: i64, command: &str, message: String) -> Value {
        json!({
            "seq": self.next_seq(),
            "type": "response",
            "request_seq": request_seq,
            "success": false,
            "command": command,
            "message": message
        })
    }

    fn event(&mut self, event: &str, body: Value) -> Value {
        json!({
            "seq": self.next_seq(),
            "type": "event",
            "event": event,
            "body": body
        })
    }

    fn next_seq(&mut self) -> i64 {
        self.next_seq += 1;
        self.next_seq
    }

    fn next_breakpoint_id(&mut self) -> i64 {
        self.next_breakpoint_id += 1;
        self.next_breakpoint_id
    }
}


fn observation_unavailable_message(command: &str) -> String {
    format!(
        "DAP command {command:?} requires an active stopped source-simulator session; synthetic observations are unavailable before launch or after termination"
    )
}

mod support;
use support::{collect_static_locals, first_executable_line};
pub use support::run_stdio;

#[cfg(test)]
mod tests;
