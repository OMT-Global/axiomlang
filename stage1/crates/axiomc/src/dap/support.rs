use super::*;

pub fn run_stdio<R, W>(mut input: R, mut output: W) -> Result<(), Diagnostic>
where
    R: BufRead,
    W: Write,
{
    let mut session = DapSession::default();
    while let Some(message) = framed_protocol::read_message(&mut input, "dap")? {
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
pub(super) fn first_executable_line(lines: &[String]) -> Option<i64> {
    lines
        .iter()
        .position(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with("//")
        })
        .map(|index| index as i64 + 1)
}

pub(super) fn collect_static_locals(source: &str) -> Vec<Variable> {
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

fn write_message<W>(output: &mut W, payload: &Value) -> Result<(), Diagnostic>
where
    W: Write,
{
    let body = serde_json::to_string(payload)
        .map_err(|err| Diagnostic::new("dap", format!("failed to serialize DAP message: {err}")))?;
    write!(output, "Content-Length: {}\r\n\r\n{}", body.len(), body)
        .map_err(|err| Diagnostic::new("dap", format!("failed to write DAP message: {err}")))
}
