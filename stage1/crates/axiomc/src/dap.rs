use crate::diagnostics::Diagnostic;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, Write};
use std::path::PathBuf;

const LOCALS_VARIABLES_REFERENCE: i64 = 1;

pub fn run_stdio<R, W>(mut input: R, mut output: W) -> Result<(), Diagnostic>
where
    R: BufRead,
    W: Write,
{
    let mut session = DapSession::default();
    while let Some(message) = read_message(&mut input)? {
        let response = session.handle_message(&message)?;
        for payload in response.messages {
            write_message(&mut output, &payload)?;
        }
        output
            .flush()
            .map_err(|err| Diagnostic::new("dap", format!("failed to flush DAP output: {err}")))?;
        if response.exit {
            break;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct DapResponse {
    pub messages: Vec<Value>,
    pub exit: bool,
}

#[derive(Debug, Clone)]
struct Breakpoint {
    id: i64,
    line: i64,
    verified: bool,
}

#[derive(Debug, Clone)]
struct Variable {
    name: String,
    value: String,
    type_name: String,
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
                messages.push(self.success_response(request_seq, command, initialize_body()));
                messages.push(self.event("initialized", json!({})));
            }
            "launch" => match self.launch(&arguments) {
                Ok(()) => {
                    messages.push(self.success_response(request_seq, command, json!({})));
                    messages.push(self.event(
                        "stopped",
                        json!({
                            "reason": "entry",
                            "threadId": 1,
                            "allThreadsStopped": true
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
            "threads" => messages.push(self.success_response(
                request_seq,
                command,
                json!({ "threads": [{ "id": 1, "name": "axiom main" }] }),
            )),
            "stackTrace" => messages.push(self.success_response(
                request_seq,
                command,
                json!({
                    "stackFrames": [self.stack_frame()],
                    "totalFrames": 1
                }),
            )),
            "scopes" => messages.push(self.success_response(
                request_seq,
                command,
                json!({
                    "scopes": [{
                        "name": "Locals",
                        "variablesReference": LOCALS_VARIABLES_REFERENCE,
                        "expensive": false
                    }]
                }),
            )),
            "variables" => {
                let variables = self.variables(&arguments);
                messages.push(self.success_response(
                    request_seq,
                    command,
                    json!({ "variables": variables }),
                ));
            }
            "continue" => {
                messages.push(self.success_response(
                    request_seq,
                    command,
                    json!({ "allThreadsContinued": true }),
                ));
                if self.advance_to_next_breakpoint() {
                    messages.push(self.event(
                        "stopped",
                        json!({
                            "reason": "breakpoint",
                            "threadId": 1,
                            "allThreadsStopped": true
                        }),
                    ));
                } else {
                    messages.push(self.event("terminated", json!({})));
                }
            }
            "next" | "stepIn" | "stepOut" => {
                self.step_one_line();
                messages.push(self.success_response(request_seq, command, json!({})));
                messages.push(self.event(
                    "stopped",
                    json!({
                        "reason": "step",
                        "threadId": 1,
                        "allThreadsStopped": true
                    }),
                ));
            }
            "disconnect" => {
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
        let program = arguments
            .get("program")
            .and_then(Value::as_str)
            .ok_or_else(|| Diagnostic::new("dap", "launch requires a string `program` argument"))?;
        let path = PathBuf::from(program);
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
                    verified: line >= 1 && line <= line_count,
                }
            })
            .collect::<Vec<_>>();
        let body = breakpoints
            .iter()
            .map(|breakpoint| {
                json!({
                    "id": breakpoint.id,
                    "verified": breakpoint.verified,
                    "line": breakpoint.line
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
            }
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
                    "variablesReference": 0
                }))
                .collect::<Vec<_>>()
        )
    }

    fn advance_to_next_breakpoint(&mut self) -> bool {
        let Some(program) = &self.program else {
            return false;
        };
        let path = program.display().to_string();
        let Some(breakpoints) = self.breakpoints.get(&path) else {
            return false;
        };
        if let Some(next) = breakpoints
            .iter()
            .filter(|breakpoint| breakpoint.verified && breakpoint.line > self.current_line)
            .map(|breakpoint| breakpoint.line)
            .min()
        {
            self.current_line = next;
            true
        } else {
            false
        }
    }

    fn step_one_line(&mut self) {
        let max_line = self.source_lines.len().max(1) as i64;
        self.current_line = (self.current_line + 1).min(max_line);
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

fn initialize_body() -> Value {
    json!({
        "adapterID": "axiom",
        "supportsConfigurationDoneRequest": true,
        "supportsStepInTargetsRequest": false,
        "supportsSetVariable": false,
        "supportsEvaluateForHovers": false,
        "supportsExceptionInfoRequest": false
    })
}

fn first_executable_line(lines: &[String]) -> Option<i64> {
    lines
        .iter()
        .position(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with("//")
        })
        .map(|index| index as i64 + 1)
}

fn collect_static_locals(source: &str) -> Vec<Variable> {
    source
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let rest = trimmed.strip_prefix("let ")?;
            let (name_and_type, value) = rest.split_once('=')?;
            let (name, type_name) = name_and_type
                .split_once(':')
                .map(|(name, type_name)| (name.trim(), type_name.trim()))
                .unwrap_or((name_and_type.trim(), "unknown"));
            if name.is_empty() {
                return None;
            }
            Some(Variable {
                name: name.to_string(),
                value: value.trim().trim_end_matches(';').to_string(),
                type_name: type_name.to_string(),
            })
        })
        .collect()
}

fn read_message<R>(input: &mut R) -> Result<Option<String>, Diagnostic>
where
    R: BufRead,
{
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let bytes = input
            .read_line(&mut line)
            .map_err(|err| Diagnostic::new("dap", format!("failed to read DAP header: {err}")))?;
        if bytes == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.trim().eq_ignore_ascii_case("Content-Length") {
                content_length = Some(value.trim().parse::<usize>().map_err(|err| {
                    Diagnostic::new("dap", format!("invalid Content-Length header: {err}"))
                })?);
            }
        }
    }

    let length = content_length
        .ok_or_else(|| Diagnostic::new("dap", "missing Content-Length header in DAP message"))?;
    let mut body = vec![0; length];
    input
        .read_exact(&mut body)
        .map_err(|err| Diagnostic::new("dap", format!("failed to read DAP body: {err}")))?;
    String::from_utf8(body)
        .map(Some)
        .map_err(|err| Diagnostic::new("dap", format!("DAP body is not UTF-8: {err}")))
}

fn write_message<W>(output: &mut W, payload: &Value) -> Result<(), Diagnostic>
where
    W: Write,
{
    let body = serde_json::to_string(payload)
        .map_err(|err| Diagnostic::new("dap", format!("failed to serialize DAP message: {err}")))?;
    write!(output, "Content-Length: {}\r\n\r\n{}", body.len(), body)
        .map_err(|err| Diagnostic::new("dap", format!("failed to write DAP message: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn request(seq: i64, command: &str, arguments: Value) -> String {
        json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": arguments
        })
        .to_string()
    }

    #[test]
    fn initialize_advertises_axiom_debug_capabilities() {
        let mut session = DapSession::default();
        let response = session
            .handle_message(&request(1, "initialize", json!({})))
            .expect("initialize");

        assert_eq!(response.messages.len(), 2);
        assert_eq!(response.messages[0]["type"], json!("response"));
        assert_eq!(response.messages[0]["body"]["adapterID"], json!("axiom"));
        assert_eq!(response.messages[1]["event"], json!("initialized"));
    }

    #[test]
    fn launch_breakpoints_stack_and_variables_round_trip() {
        let dir = tempdir().expect("tempdir");
        let program = dir.path().join("main.ax");
        fs::write(&program, "let answer: int = 42\nprint answer\n").expect("write program");
        let program_path = program.display().to_string();
        let mut session = DapSession::default();

        let launch = session
            .handle_message(&request(1, "launch", json!({ "program": program_path })))
            .expect("launch");
        assert_eq!(launch.messages[0]["success"], json!(true));
        assert_eq!(launch.messages[1]["event"], json!("stopped"));

        let breakpoints = session
            .handle_message(&request(
                2,
                "setBreakpoints",
                json!({
                    "source": { "path": program.display().to_string() },
                    "breakpoints": [{ "line": 2 }]
                }),
            ))
            .expect("set breakpoints");
        assert_eq!(
            breakpoints.messages[0]["body"]["breakpoints"][0]["verified"],
            json!(true)
        );

        let continued = session
            .handle_message(&request(3, "continue", json!({ "threadId": 1 })))
            .expect("continue");
        assert_eq!(continued.messages[1]["event"], json!("stopped"));

        let stack = session
            .handle_message(&request(4, "stackTrace", json!({ "threadId": 1 })))
            .expect("stack");
        assert_eq!(
            stack.messages[0]["body"]["stackFrames"][0]["line"],
            json!(2)
        );

        let scopes = session
            .handle_message(&request(5, "scopes", json!({ "frameId": 1 })))
            .expect("scopes");
        assert_eq!(
            scopes.messages[0]["body"]["scopes"][0]["variablesReference"],
            json!(LOCALS_VARIABLES_REFERENCE)
        );

        let variables = session
            .handle_message(&request(
                6,
                "variables",
                json!({ "variablesReference": LOCALS_VARIABLES_REFERENCE }),
            ))
            .expect("variables");
        assert_eq!(
            variables.messages[0]["body"]["variables"][0]["name"],
            json!("answer")
        );
        assert_eq!(
            variables.messages[0]["body"]["variables"][0]["value"],
            json!("42")
        );
        assert_eq!(
            variables.messages[0]["body"]["variables"][0]["type"],
            json!("int")
        );
    }

    #[test]
    fn set_breakpoints_can_verify_source_before_launch() {
        let dir = tempdir().expect("tempdir");
        let program = dir.path().join("main.ax");
        fs::write(&program, "let answer: int = 42\nprint answer\n").expect("write program");
        let mut session = DapSession::default();

        let breakpoints = session
            .handle_message(&request(
                1,
                "setBreakpoints",
                json!({
                    "source": { "path": program.display().to_string() },
                    "breakpoints": [{ "line": 2 }]
                }),
            ))
            .expect("set breakpoints");

        assert_eq!(
            breakpoints.messages[0]["body"]["breakpoints"][0]["verified"],
            json!(true)
        );
    }

    #[test]
    fn stdio_loop_reads_and_writes_framed_messages() {
        let body = request(7, "initialize", json!({}));
        let input = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let mut output = Vec::new();

        run_stdio(std::io::Cursor::new(input.into_bytes()), &mut output).expect("run stdio");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.starts_with("Content-Length: "));
        assert!(output.contains(r#""adapterID":"axiom""#));
    }

    #[test]
    fn disconnect_stops_stdio_loop() {
        let mut session = DapSession::default();
        let response = session
            .handle_message(&request(9, "disconnect", json!({})))
            .expect("disconnect");

        assert!(response.exit);
        assert_eq!(response.messages[0]["success"], json!(true));
    }
}
