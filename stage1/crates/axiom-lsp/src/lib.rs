use serde_json::{Value, json};
use std::io::{BufRead, Read, Write};

pub const TEXT_DOCUMENT_SYNC_KIND_FULL: u8 = 1;

#[derive(Debug, Clone, PartialEq)]
pub struct LspResponse {
    pub messages: Vec<Value>,
    pub exit: bool,
}

pub trait DocumentAnalyzer {
    fn publish_diagnostics(&self, uri: &str, source: &str) -> Value;
}

pub fn run_stdio<R, W, A, E>(mut input: R, mut output: W, analyzer: &A) -> Result<(), E>
where
    R: BufRead,
    W: Write,
    A: DocumentAnalyzer,
    E: From<String>,
{
    while let Some(message) = read_message(&mut input)? {
        let response = handle_message(&message, analyzer)?;
        for payload in response.messages {
            write_message(&mut output, &payload)?;
        }
        output
            .flush()
            .map_err(|err| E::from(format!("failed to flush LSP output: {err}")))?;
        if response.exit {
            break;
        }
    }
    Ok(())
}

pub fn handle_message<A, E>(payload: &str, analyzer: &A) -> Result<LspResponse, E>
where
    A: DocumentAnalyzer,
    E: From<String>,
{
    let value: Value = serde_json::from_str(payload)
        .map_err(|err| E::from(format!("invalid JSON-RPC payload: {err}")))?;
    let method = value.get("method").and_then(Value::as_str);
    let id = value.get("id").cloned();
    let messages = match method {
        Some("initialize") => id.map(initialize_response).into_iter().collect(),
        Some("shutdown") => id.map(empty_response).into_iter().collect(),
        Some("textDocument/didOpen") => {
            did_open_diagnostics(&value, analyzer).into_iter().collect()
        }
        Some("textDocument/didChange") => did_change_diagnostics(&value, analyzer)
            .into_iter()
            .collect(),
        Some("initialized") | Some("exit") => Vec::new(),
        Some(other) => id
            .map(|request_id| unsupported_method_response(request_id, other))
            .into_iter()
            .collect(),
        None => Vec::new(),
    };
    Ok(LspResponse {
        messages,
        exit: matches!(method, Some("exit")),
    })
}

pub fn initialize_response(id: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "serverInfo": { "name": "axiom-analyzer", "version": env!("CARGO_PKG_VERSION") },
            "capabilities": { "textDocumentSync": { "openClose": true, "change": TEXT_DOCUMENT_SYNC_KIND_FULL } }
        }
    })
}

fn empty_response(id: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": null })
}

fn unsupported_method_response(id: Value, method: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("unsupported method {method:?}") } })
}

fn did_open_diagnostics<A: DocumentAnalyzer>(message: &Value, analyzer: &A) -> Option<Value> {
    let document = message.get("params")?.get("textDocument")?;
    let uri = document.get("uri")?.as_str()?;
    let text = document.get("text")?.as_str()?;
    Some(analyzer.publish_diagnostics(uri, text))
}

fn did_change_diagnostics<A: DocumentAnalyzer>(message: &Value, analyzer: &A) -> Option<Value> {
    let params = message.get("params")?;
    let uri = params.get("textDocument")?.get("uri")?.as_str()?;
    let change = params.get("contentChanges")?.as_array()?.first()?;
    if change.get("range").is_some() {
        return None;
    }
    let text = change.get("text")?.as_str()?;
    Some(analyzer.publish_diagnostics(uri, text))
}

fn read_message<R, E>(input: &mut R) -> Result<Option<String>, E>
where
    R: BufRead,
    E: From<String>,
{
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let bytes = input
            .read_line(&mut line)
            .map_err(|err| E::from(format!("failed to read LSP header: {err}")))?;
        if bytes == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if !name.trim().eq_ignore_ascii_case("Content-Length") {
                continue;
            }
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|err| E::from(format!("invalid Content-Length header: {err}")))?,
            );
        }
    }
    let length = content_length
        .ok_or_else(|| E::from("missing Content-Length header in LSP message".to_string()))?;
    let mut body = vec![0; length];
    input
        .read_exact(&mut body)
        .map_err(|err| E::from(format!("failed to read LSP body: {err}")))?;
    String::from_utf8(body)
        .map(Some)
        .map_err(|err| E::from(format!("LSP body is not UTF-8: {err}")))
}

fn write_message<W, E>(output: &mut W, payload: &Value) -> Result<(), E>
where
    W: Write,
    E: From<String>,
{
    let body = serde_json::to_string(payload)
        .map_err(|err| E::from(format!("failed to serialize LSP message: {err}")))?;
    write!(output, "Content-Length: {}\r\n\r\n{}", body.len(), body)
        .map_err(|err| E::from(format!("failed to write LSP message: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StaticAnalyzer;
    impl DocumentAnalyzer for StaticAnalyzer {
        fn publish_diagnostics(&self, uri: &str, _source: &str) -> Value {
            json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": { "uri": uri, "diagnostics": [] } })
        }
    }

    fn notification(method: &str, params: Value) -> String {
        json!({ "jsonrpc": "2.0", "method": method, "params": params }).to_string()
    }

    #[test]
    fn initialize_advertises_full_document_diagnostics_sync() {
        let response: LspResponse = handle_message::<_, String>(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            &StaticAnalyzer,
        )
        .expect("initialize");
        assert!(!response.exit);
        assert_eq!(
            response.messages[0]["result"]["capabilities"]["textDocumentSync"]["change"],
            json!(TEXT_DOCUMENT_SYNC_KIND_FULL)
        );
    }

    #[test]
    fn did_open_streams_diagnostics_from_analyzer() {
        let payload = notification(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": "file:///tmp/good.ax", "languageId": "axiom", "version": 1, "text": "print 1\n" } }),
        );
        let response: LspResponse =
            handle_message::<_, String>(&payload, &StaticAnalyzer).expect("didOpen");
        assert_eq!(response.messages.len(), 1);
        assert_eq!(
            response.messages[0]["params"]["uri"],
            json!("file:///tmp/good.ax")
        );
        assert_eq!(response.messages[0]["params"]["diagnostics"], json!([]));
    }

    #[test]
    fn did_change_ignores_incremental_range_changes() {
        let payload = notification(
            "textDocument/didChange",
            json!({ "textDocument": { "uri": "file:///tmp/good.ax", "version": 3 }, "contentChanges": [{ "range": { "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 0 } }, "text": "}" }] }),
        );
        let response: LspResponse =
            handle_message::<_, String>(&payload, &StaticAnalyzer).expect("didChange");
        assert!(response.messages.is_empty());
    }

    #[test]
    fn stdio_loop_reads_and_writes_framed_messages() {
        let body = r#"{"jsonrpc":"2.0","id":7,"method":"initialize","params":{}}"#;
        let input = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let mut output = Vec::new();
        run_stdio::<_, _, _, String>(
            std::io::Cursor::new(input.into_bytes()),
            &mut output,
            &StaticAnalyzer,
        )
        .expect("run stdio");
        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.starts_with("Content-Length: "));
        assert!(output.contains(r#""id":7"#));
        assert!(output.contains(r#""axiom-analyzer""#));
    }
}
