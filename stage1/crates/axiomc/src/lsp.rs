use crate::diagnostics::Diagnostic;
use crate::hir;
use crate::manifest::CapabilityConfig;
use crate::mir;
use crate::syntax;
use axiom_lsp::{DocumentAnalyzer, LspResponse};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

struct CompilerAnalyzer;

impl DocumentAnalyzer for CompilerAnalyzer {
    fn publish_diagnostics(&self, uri: &str, source: &str) -> Value {
        publish_diagnostics(uri, source)
    }
}

pub fn run_stdio<R, W>(input: R, output: W) -> Result<(), Diagnostic>
where
    R: BufRead,
    W: Write,
{
    axiom_lsp::run_stdio::<_, _, _, String>(input, output, &CompilerAnalyzer)
        .map_err(|message| Diagnostic::new("lsp", message))
}

pub fn handle_message(payload: &str) -> Result<LspResponse, Diagnostic> {
    axiom_lsp::handle_message::<_, String>(payload, &CompilerAnalyzer)
        .map_err(|message| Diagnostic::new("lsp", message))
}

pub fn publish_diagnostics(uri: &str, source: &str) -> Value {
    let diagnostics = analyze_source(uri, source)
        .into_iter()
        .map(lsp_diagnostic)
        .collect::<Vec<Value>>();
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": uri,
            "diagnostics": diagnostics
        }
    })
}

pub fn analyze_source(uri: &str, source: &str) -> Vec<Diagnostic> {
    let path = path_for_uri(uri);
    match syntax::parse_program_with_recovery(source, &path) {
        Ok(program) => {
            let capabilities = CapabilityConfig::default();
            match hir::lower_with_capabilities_recovery(&program, &capabilities) {
                Ok(hir) => {
                    let _ = mir::lower(&hir);
                    Vec::new()
                }
                Err(diagnostics) => diagnostics_with_default_path(diagnostics, &path),
            }
        }
        Err(diagnostics) => diagnostics_with_default_path(diagnostics, &path),
    }
}

fn diagnostics_with_default_path(diagnostics: Vec<Diagnostic>, path: &Path) -> Vec<Diagnostic> {
    diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic_with_default_path(diagnostic, path))
        .collect()
}

fn diagnostic_with_default_path(mut diagnostic: Diagnostic, path: &Path) -> Diagnostic {
    if diagnostic.path.is_none() {
        diagnostic.path = Some(path.display().to_string());
    }
    diagnostic
}

fn lsp_diagnostic(diagnostic: Diagnostic) -> Value {
    let line = diagnostic.line.unwrap_or(1).saturating_sub(1);
    let column = diagnostic.column.unwrap_or(1).saturating_sub(1);
    json!({
        "range": {
            "start": { "line": line, "character": column },
            "end": { "line": line, "character": column.saturating_add(1) }
        },
        "severity": 1,
        "source": "axiomc",
        "code": diagnostic.code.unwrap_or(diagnostic.kind),
        "message": diagnostic.message
    })
}

fn path_for_uri(uri: &str) -> PathBuf {
    if let Some(path) = uri.strip_prefix("file://") {
        return PathBuf::from(percent_decode(path));
    }
    PathBuf::from(uri)
}

fn percent_decode(input: &str) -> String {
    let mut output = Vec::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            let mut encoded = [0; 4];
            output.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
            continue;
        }
        let hi = chars.next();
        let lo = chars.next();
        match (hi, lo) {
            (Some(hi), Some(lo)) => {
                let encoded = format!("{hi}{lo}");
                if let Ok(value) = u8::from_str_radix(&encoded, 16) {
                    output.push(value);
                } else {
                    output.push(b'%');
                    let mut hi_encoded = [0; 4];
                    output.extend_from_slice(hi.encode_utf8(&mut hi_encoded).as_bytes());
                    let mut lo_encoded = [0; 4];
                    output.extend_from_slice(lo.encode_utf8(&mut lo_encoded).as_bytes());
                }
            }
            (Some(hi), None) => {
                output.push(b'%');
                let mut encoded = [0; 4];
                output.extend_from_slice(hi.encode_utf8(&mut encoded).as_bytes());
            }
            _ => output.push(b'%'),
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(method: &str, params: Value) -> String {
        json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        })
        .to_string()
    }

    #[test]
    fn initialize_advertises_full_document_diagnostics_sync() {
        let response =
            handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
                .expect("handle initialize");

        assert!(!response.exit);
        assert_eq!(response.messages.len(), 1);
        assert_eq!(response.messages[0]["id"], json!(1));
        assert_eq!(
            response.messages[0]["result"]["serverInfo"]["name"],
            json!("axiom-analyzer")
        );
        assert_eq!(
            response.messages[0]["result"]["capabilities"]["textDocumentSync"]["change"],
            json!(axiom_lsp::TEXT_DOCUMENT_SYNC_KIND_FULL)
        );
    }

    #[test]
    fn did_open_publishes_compiler_diagnostic() {
        let payload = notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": "file:///tmp/bad.ax",
                    "languageId": "axiom",
                    "version": 1,
                    "text": "}\n"
                }
            }),
        );

        let response = handle_message(&payload).expect("handle didOpen");

        assert_eq!(response.messages.len(), 1);
        let diagnostics = response.messages[0]["params"]["diagnostics"]
            .as_array()
            .expect("diagnostics array");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0]["source"], json!("axiomc"));
        assert_eq!(diagnostics[0]["code"], json!("parse"));
        assert!(
            diagnostics[0]["message"]
                .as_str()
                .expect("message")
                .contains("unexpected closing brace")
        );
        assert_eq!(
            diagnostics[0]["range"]["start"],
            json!({ "line": 0, "character": 0 })
        );
    }

    #[test]
    fn did_open_publishes_multiple_parse_diagnostics() {
        let payload = notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": "file:///tmp/multi-parse.ax",
                    "languageId": "axiom",
                    "version": 1,
                    "text": "import math.ax\nlet answer int = 42\nelse {\n"
                }
            }),
        );

        let response = handle_message(&payload).expect("handle didOpen");

        let diagnostics = response.messages[0]["params"]["diagnostics"]
            .as_array()
            .expect("diagnostics array");
        assert_eq!(diagnostics.len(), 3);
        assert_eq!(
            diagnostics[0]["message"],
            json!("import must use a quoted relative path")
        );
        assert_eq!(
            diagnostics[1]["message"],
            json!("let binding is missing ':'")
        );
        assert_eq!(diagnostics[2]["message"], json!("unexpected else block"));
    }

    #[test]
    fn did_open_publishes_multiple_type_diagnostics() {
        let payload = notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": "file:///tmp/multi-type.ax",
                    "languageId": "axiom",
                    "version": 1,
                    "text": "print missing_name\nlet answer: int = \"nope\"\nprint answer\nprint also_missing\n"
                }
            }),
        );

        let response = handle_message(&payload).expect("handle didOpen");

        let diagnostics = response.messages[0]["params"]["diagnostics"]
            .as_array()
            .expect("diagnostics array");
        assert_eq!(diagnostics.len(), 3);
        assert_eq!(
            diagnostics[0]["range"]["start"],
            json!({ "line": 0, "character": 6 })
        );
        assert!(
            diagnostics[0]["message"]
                .as_str()
                .unwrap()
                .contains("undefined variable")
        );
        assert_eq!(
            diagnostics[1]["message"],
            json!("let binding \"answer\" expects int, got string")
        );
        assert_eq!(
            diagnostics[2]["range"]["start"],
            json!({ "line": 3, "character": 6 })
        );
        assert!(
            diagnostics[2]["message"]
                .as_str()
                .unwrap()
                .contains("undefined variable")
        );
    }

    #[test]
    fn did_change_recomputes_and_clears_diagnostics() {
        let payload = notification(
            "textDocument/didChange",
            json!({
                "textDocument": {
                    "uri": "file:///tmp/good.ax",
                    "version": 2
                },
                "contentChanges": [{
                    "text": "let answer: int = 42\nprint answer\n"
                }]
            }),
        );

        let response = handle_message(&payload).expect("handle didChange");

        assert_eq!(response.messages.len(), 1);
        assert_eq!(response.messages[0]["params"]["diagnostics"], json!([]));
    }

    #[test]
    fn did_change_ignores_incremental_range_changes() {
        let payload = notification(
            "textDocument/didChange",
            json!({
                "textDocument": {
                    "uri": "file:///tmp/good.ax",
                    "version": 3
                },
                "contentChanges": [{
                    "range": {
                        "start": { "line": 1, "character": 0 },
                        "end": { "line": 1, "character": 0 }
                    },
                    "text": "}"
                }]
            }),
        );

        let response = handle_message(&payload).expect("handle didChange");

        assert!(response.messages.is_empty());
    }

    #[test]
    fn did_open_exercises_hir_diagnostics_after_parse() {
        let payload = notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": "file:///tmp/type.ax",
                    "languageId": "axiom",
                    "version": 1,
                    "text": "print missing_name\n"
                }
            }),
        );

        let response = handle_message(&payload).expect("handle didOpen");

        let diagnostics = response.messages[0]["params"]["diagnostics"]
            .as_array()
            .expect("diagnostics array");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0]["code"], json!("type"));
        assert!(
            diagnostics[0]["message"]
                .as_str()
                .expect("message")
                .contains("undefined variable")
        );
    }

    #[test]
    fn stdio_loop_reads_and_writes_framed_messages() {
        let body = r#"{"jsonrpc":"2.0","id":7,"method":"initialize","params":{}}"#;
        let input = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let mut output = Vec::new();

        run_stdio(std::io::Cursor::new(input.into_bytes()), &mut output).expect("run stdio");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.starts_with("Content-Length: "));
        assert!(output.contains(r#""id":7"#));
        assert!(output.contains(r#""axiom-analyzer""#));
    }

    #[test]
    fn stdio_loop_accepts_case_insensitive_content_length_header() {
        let body = r#"{"jsonrpc":"2.0","id":8,"method":"initialize","params":{}}"#;
        let input = format!("content-length: {}\r\n\r\n{}", body.len(), body);
        let mut output = Vec::new();

        run_stdio(std::io::Cursor::new(input.into_bytes()), &mut output).expect("run stdio");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.starts_with("Content-Length: "));
        assert!(output.contains(r#""id":8"#));
    }

    #[test]
    fn percent_decode_decodes_utf8_file_uri_bytes_once() {
        let path = path_for_uri("file:///tmp/%E6%96%87%E4%BB%B6.ax");

        assert_eq!(path, PathBuf::from("/tmp/文件.ax"));
    }

    #[test]
    fn exit_notification_stops_stdio_loop() {
        let response =
            handle_message(r#"{"jsonrpc":"2.0","method":"exit","params":{}}"#).expect("exit");

        assert!(response.exit);
        assert!(response.messages.is_empty());
    }
}
