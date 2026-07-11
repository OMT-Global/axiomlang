use super::axiom_files;
use axiomc::{diagnostics::Diagnostic, json_contract};
use serde::Serialize;
use std::fs;
use std::path::Path;

const FORMAT_SCHEMA_PATH: &str = "stage1/schemas/axiom-format-edit-v1.schema.json";

#[derive(Debug, Clone, Serialize)]
struct FormatEdit {
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
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct FormatReport {
    schema_version: &'static str,
    schema: &'static str,
    ok: bool,
    command: &'static str,
    check: bool,
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
        });
    }
    Ok(FormatReport {
        schema_version: json_contract::JSON_SCHEMA_VERSION,
        schema: FORMAT_SCHEMA_PATH,
        ok: !check || changed == 0,
        command: "fmt",
        check,
        files: reports,
        changed,
    })
}

fn format_axiom_source(source: &str) -> String {
    let mut lines = Vec::new();
    let mut previous_blank = false;
    for line in source.replace("\r\n", "\n").replace('\t', "    ").lines() {
        let trimmed_end = line.trim_end();
        let blank = trimmed_end.is_empty();
        if blank && previous_blank {
            continue;
        }
        lines.push(trimmed_end.to_string());
        previous_blank = blank;
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    format!("{}\n", lines.join("\n"))
}

fn format_edits(original: &str, formatted: &str) -> Vec<FormatEdit> {
    if original == formatted {
        return Vec::new();
    }
    let lines = source_lines(original);
    let last_content = lines
        .iter()
        .rposition(|line| !normalized_line(line.text).is_empty());
    let mut expected = Vec::new();
    let mut previous_blank = false;
    for (index, line) in lines.iter().enumerate() {
        let normalized = normalized_line(line.text);
        let blank = normalized.is_empty();
        let emit = index <= last_content.unwrap_or(0) && !(blank && previous_blank);
        expected.push(emit.then(|| format!("{normalized}\n")));
        if emit {
            previous_blank = blank;
        }
    }
    let rebuilt = expected.iter().flatten().cloned().collect::<String>();
    if rebuilt != formatted && !(lines.is_empty() && formatted == "\n") {
        return vec![precise_edit("replace_line", 1, 0, original, formatted)];
    }

    let mut edits = Vec::new();
    for (index, (line, after)) in lines.iter().zip(&expected).enumerate() {
        match after {
            Some(after) if line.text != after => edits.push(precise_edit(
                "replace_line",
                index + 1,
                line.start_byte,
                line.text,
                after,
            )),
            None => edits.push(precise_edit(
                "delete_line",
                index + 1,
                line.start_byte,
                line.text,
                "",
            )),
            _ => {}
        }
    }
    if lines.is_empty() {
        edits.push(precise_edit("insert_line", 1, 0, "", formatted));
    }
    edits
}

struct SourceLine<'a> {
    start_byte: usize,
    text: &'a str,
}

fn source_lines(source: &str) -> Vec<SourceLine<'_>> {
    let mut start_byte = 0;
    source
        .split_inclusive('\n')
        .map(|text| {
            let line = SourceLine { start_byte, text };
            start_byte += text.len();
            line
        })
        .collect()
}

fn normalized_line(line: &str) -> String {
    trim_line_ending(line)
        .replace('\t', "    ")
        .trim_end()
        .to_string()
}

fn precise_edit(
    action: &'static str,
    line: usize,
    base: usize,
    before: &str,
    after: &str,
) -> FormatEdit {
    let prefix = common_prefix_bytes(before, after);
    let suffix = common_suffix_bytes(&before[prefix..], &after[prefix..]);
    FormatEdit {
        action,
        line,
        before: (action != "insert_line").then(|| trim_line_ending(before).to_string()),
        after: (action != "delete_line").then(|| trim_line_ending(after).to_string()),
        start_byte: base + prefix,
        end_byte: base + before.len() - suffix,
        replacement: after[prefix..after.len() - suffix].to_string(),
    }
}

fn common_prefix_bytes(left: &str, right: &str) -> usize {
    left.chars()
        .zip(right.chars())
        .take_while(|(left, right)| left == right)
        .map(|(character, _)| character.len_utf8())
        .sum()
}

fn common_suffix_bytes(left: &str, right: &str) -> usize {
    left.chars()
        .rev()
        .zip(right.chars().rev())
        .take_while(|(left, right)| left == right)
        .map(|(character, _)| character.len_utf8())
        .sum()
}

fn trim_line_ending(line: &str) -> &str {
    line.strip_suffix('\n')
        .and_then(|line| line.strip_suffix('\r').or(Some(line)))
        .unwrap_or(line)
}

#[cfg(test)]
#[path = "formatter_tests.rs"]
mod tests;
