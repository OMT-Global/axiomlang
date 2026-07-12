use super::axiom_files;
use axiomc::{diagnostics::Diagnostic, json_contract};
use serde::Serialize;
use std::fs;
use std::path::Path;

#[path = "formatter_report.rs"]
mod formatter_report;
use formatter_report::format_edits;

const FORMAT_SCHEMA_PATH: &str = "stage1/schemas/axiom-format-edit-v1.schema.json";

#[derive(Debug, Clone, Serialize)]
pub(super) struct FormatEdit {
    action: &'static str,
    line: usize,
    before: Option<String>,
    after: Option<String>,
    start_byte: usize,
    end_byte: usize,
    replacement: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct FormatFileReport {
    pub(super) path: String,
    pub(super) changed: bool,
    edits: Vec<FormatEdit>,
    pub(super) range: Option<FormatRange>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub(super) struct FormatRange {
    pub(super) start_byte: usize,
    pub(super) end_byte: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct FormatReport {
    schema_version: &'static str,
    schema: &'static str,
    ok: bool,
    command: &'static str,
    check: bool,
    input: &'static str,
    pub(super) files: Vec<FormatFileReport>,
    pub(super) changed: usize,
}

pub(super) fn format_axiom_sources(path: &Path, check: bool) -> Result<FormatReport, Diagnostic> {
    let files = axiom_files(path)?;
    if files.is_empty() {
        return Err(Diagnostic::new(
            "fmt",
            format!("no .ax files found under {}", path.display()),
        ));
    }
    let mut reports = Vec::new();
    let mut changed = 0;
    for file in files {
        let original = fs::read_to_string(&file).map_err(|err| {
            Diagnostic::new("fmt", format!("failed to read {}: {err}", file.display()))
                .with_path(file.display().to_string())
        })?;
        let formatted = format_axiom_source(&original);
        let is_changed = formatted != original;
        let edits = format_edits(&original, &formatted);
        if is_changed {
            changed += 1;
            if !check {
                fs::write(&file, formatted).map_err(|err| {
                    Diagnostic::new("fmt", format!("failed to write {}: {err}", file.display()))
                        .with_path(file.display().to_string())
                })?;
            }
        }
        reports.push(FormatFileReport {
            path: file.display().to_string(),
            changed: is_changed,
            edits,
            range: None,
        });
    }
    Ok(FormatReport {
        schema_version: json_contract::JSON_SCHEMA_VERSION,
        schema: FORMAT_SCHEMA_PATH,
        ok: !check || changed == 0,
        command: "fmt",
        check,
        input: "files",
        files: reports,
        changed,
    })
}

pub(super) fn format_axiom_stdin(
    source: &str,
    check: bool,
    range: Option<FormatRange>,
) -> Result<(String, FormatReport), Diagnostic> {
    let formatted = match range {
        Some(range) => format_axiom_range(source, range)?,
        None => format_axiom_source(source),
    };
    let changed = formatted != source;
    let report = FormatReport {
        schema_version: json_contract::JSON_SCHEMA_VERSION,
        schema: FORMAT_SCHEMA_PATH,
        ok: !check || !changed,
        command: "fmt",
        check,
        input: "stdin",
        files: vec![FormatFileReport {
            path: "<stdin>".to_string(),
            changed,
            edits: format_edits(source, &formatted),
            range,
        }],
        changed: usize::from(changed),
    };
    Ok((formatted, report))
}

fn format_axiom_range(source: &str, range: FormatRange) -> Result<String, Diagnostic> {
    if range.start_byte > range.end_byte || range.end_byte > source.len() {
        return Err(Diagnostic::new(
            "fmt.range.invalid",
            format!(
                "format range {}:{} is outside the {}-byte input",
                range.start_byte,
                range.end_byte,
                source.len()
            ),
        )
        .with_code("fmt.range.invalid"));
    }
    if !source.is_char_boundary(range.start_byte) || !source.is_char_boundary(range.end_byte) {
        return Err(Diagnostic::new(
            "fmt.range.invalid",
            "format range boundaries must be UTF-8 byte boundaries",
        )
        .with_code("fmt.range.invalid"));
    }
    let slice = &source[range.start_byte..range.end_byte];
    let mut replacement = format_axiom_source(slice);
    if range.end_byte < source.len() && !ends_with_line_ending(slice) {
        replacement.pop();
    }

    let mut formatted = String::with_capacity(source.len() + replacement.len());
    formatted.push_str(&source[..range.start_byte]);
    formatted.push_str(&replacement);
    formatted.push_str(&source[range.end_byte..]);
    Ok(formatted)
}

fn ends_with_line_ending(source: &str) -> bool {
    source.ends_with('\n') || source.ends_with('\r')
}
pub(super) fn format_axiom_source(source: &str) -> String {
    formatter_core::format(source)
}

#[path = "formatter_core.rs"]
mod formatter_core;

#[cfg(test)]
#[path = "formatter_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "formatter_syntax_tests.rs"]
mod syntax_tests;
