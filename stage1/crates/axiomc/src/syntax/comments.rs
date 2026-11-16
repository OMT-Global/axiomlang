//! Line-comment handling: `//` stripping and the `#` comment diagnostic.

use crate::diagnostics::Diagnostic;
use std::path::Path;

/// Strip `//` comments before macro expansion; `//` inside strings is untouched.
pub(super) fn strip_line_comments(source: &str) -> String {
    if !source.contains("//") { return source.to_string(); }
    let mut cleaned = String::with_capacity(source.len());
    let mut rest = source;
    loop {
        let (line, remainder) = match rest.split_once('\n') {
            Some((line, remainder)) => (line, Some(remainder)),
            None => (rest, None),
        };
        cleaned.push_str(strip_line_comment(line));
        match remainder {
            Some(remainder) => {
                cleaned.push('\n');
                rest = remainder;
            }
            None => break,
        }
    }
    cleaned
}

fn strip_line_comment(line: &str) -> &str {
    let (mut in_string, mut escaped) = (false, false);
    for (index, byte) in line.bytes().enumerate() { match (in_string, escaped, byte) { (true, true, _) => escaped = false, (true, false, b'\\') => escaped = true, (true, false, b'"') => in_string = false, (false, _, b'"') => in_string = true, (false, _, b'/') if line.as_bytes().get(index + 1) == Some(&b'/') => return &line[..index], _ => {} } }
    line
}

/// Diagnoses a statement line that starts with `#`, which other languages use
/// for comments, so the author is pointed at `//` instead of a generic error.
pub(super) fn hash_comment_diagnostic(line: &str, path: &Path, line_no: usize) -> Option<Diagnostic> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }
    let column = line.len() - trimmed.len() + 1;
    Some(
        Diagnostic::new(
            "parse",
            "`#` does not start a comment; use `//` for comments and `///` for doc comments",
        )
        .with_path(path.display().to_string())
        .with_span(line_no, column),
    )
}

#[cfg(test)]
mod tests {
    use super::super::parse_program;
    use std::path::Path;

    #[test]
    fn hash_prefixed_line_reports_the_supported_comment_syntax() {
        for (source, line, column) in [
            ("# leading\nprint 1\n", 1, 1),
            (
                "fn f(x: int): int {\n    # inner\n    return x\n}\nprint f(1)\n",
                2,
                5,
            ),
        ] {
            let error = parse_program(source, Path::new("main.ax")).expect_err("hash comment");
            assert!(
                error.message.contains("use `//` for comments"),
                "{}",
                error.message
            );
            assert_eq!((error.line, error.column), (Some(line), Some(column)));
        }
        // `#` inside a string literal and after `//` is ordinary text.
        parse_program(
            "print \"# not a comment\" // # still fine\n",
            Path::new("main.ax"),
        )
        .expect("hash inside string and comment");
    }
}
