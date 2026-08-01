use serde_json::{Value, json};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

#[test]
fn lsp_stdio_publishes_diagnostics_and_exits_cleanly() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn axiomc lsp");

    let mut stdin = child.stdin.take().expect("lsp stdin");
    let stdout = child.stdout.take().expect("lsp stdout");
    let messages = spawn_lsp_reader(stdout);

    write_lsp_message(
        &mut stdin,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
    )
    .expect("write initialize");
    let initialize = recv_lsp_message(&messages, "initialize response");
    assert_eq!(initialize["id"], json!(1));
    assert_eq!(
        initialize["result"]["capabilities"]["textDocumentSync"]["change"],
        json!(2)
    );

    write_lsp_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": "file:///tmp/axiom-lsp-bad.ax",
                    "languageId": "axiom",
                    "version": 1,
                    "text": "}\n"
                }
            }
        }),
    )
    .expect("write didOpen");
    let diagnostics = recv_lsp_message(&messages, "publishDiagnostics");
    assert_eq!(
        diagnostics["method"],
        json!("textDocument/publishDiagnostics")
    );
    assert_eq!(
        diagnostics["params"]["uri"],
        json!("file:///tmp/axiom-lsp-bad.ax")
    );
    let diagnostic_items = diagnostics["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert_eq!(diagnostic_items.len(), 1);
    assert_eq!(diagnostic_items[0]["code"], json!("parse"));
    assert!(
        diagnostic_items[0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unexpected closing brace")
    );

    write_lsp_message(
        &mut stdin,
        json!({"jsonrpc": "2.0", "id": 2, "method": "shutdown", "params": null}),
    )
    .expect("write shutdown");
    let shutdown = recv_lsp_message(&messages, "shutdown response");
    assert_eq!(shutdown["id"], json!(2));
    assert!(shutdown["result"].is_null());

    write_lsp_message(
        &mut stdin,
        json!({"jsonrpc": "2.0", "method": "exit", "params": null}),
    )
    .expect("write exit");
    drop(stdin);

    let status = wait_for_child(&mut child, Duration::from_secs(2)).expect("wait for lsp exit");
    assert!(status.success(), "lsp exited with status {status}");
}

fn spawn_lsp_reader(stdout: ChildStdout) -> Receiver<Result<Value, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            match read_lsp_message(&mut reader) {
                Ok(Some(message)) => {
                    if tx.send(Ok(message)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = tx.send(Err(error.to_string()));
                    break;
                }
            }
        }
    });
    rx
}

fn recv_lsp_message(rx: &Receiver<Result<Value, String>>, label: &str) -> Value {
    rx.recv_timeout(Duration::from_secs(2))
        .unwrap_or_else(|_| panic!("timed out waiting for {label}"))
        .unwrap_or_else(|error| panic!("failed to read {label}: {error}"))
}

fn write_lsp_message(stdin: &mut ChildStdin, payload: Value) -> io::Result<()> {
    let body = serde_json::to_string(&payload)?;
    write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    stdin.flush()
}

fn read_lsp_message<R>(reader: &mut R) -> io::Result<Option<Value>>
where
    R: BufRead + Read,
{
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.trim().eq_ignore_ascii_case("Content-Length") {
                content_length = Some(value.trim().parse::<usize>().map_err(|error| {
                    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
                })?);
            }
        }
    }

    let length = content_length
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).map(Some).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid JSON-RPC body: {error}"),
        )
    })
}

fn wait_for_child(child: &mut Child, timeout: Duration) -> io::Result<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            child.kill()?;
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("child did not exit within {timeout:?}"),
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
