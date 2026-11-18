//! Line-comment helpers: per-line `//` stripping and the `#` comment diagnostic.

use crate::diagnostics::Diagnostic;
use std::path::Path;

/// Byte offset of the first `#` on `line` that is outside a string literal
/// and before any `//` comment, or `None` when the line has no such `#`.
fn hash_outside_strings(line: &str) -> Option<usize> {
    let (mut in_string, mut escaped) = (false, false);
    for (index, byte) in line.bytes().enumerate() {
        match (in_string, escaped, byte) {
            (true, true, _) => escaped = false,
            (true, false, b'\\') => escaped = true,
            (true, false, b'"') => in_string = false,
            (false, _, b'"') => in_string = true,
            (false, _, b'/') if line.as_bytes().get(index + 1) == Some(&b'/') => return None,
            (false, _, b'#') => return Some(index),
            _ => {}
        }
    }
    None
}

/// Diagnoses every `#` that is outside a string literal and before any `//`
/// comment. `#` is not part of Axiom syntax, and other languages use it for
/// comments, so this points the author at `//` instead of whatever generic
/// error the rest of the line would produce. Runs on the original source, so
/// line numbers are the author's even when macros expand.
pub fn hash_comment_diagnostics(source: &str, path: &Path) -> Vec<Diagnostic> {
    source
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let byte = hash_outside_strings(line)?;
            let column = line[..byte].chars().count() + 1;
            Some(
                Diagnostic::new(
                    "parse",
                    "`#` does not start a comment; use `//` for comments and `///` for doc comments",
                )
                .with_path(path.display().to_string())
                .with_span(index + 1, column),
            )
        })
        .collect()
}

/// The first `#` diagnostic, with any others as related diagnostics. `fmt`
/// uses this to refuse a file the parser would reject (#1783).
pub fn reject_hash_comments(source: &str, path: &Path) -> Result<(), Diagnostic> {
    let mut diagnostics = hash_comment_diagnostics(source, path);
    if diagnostics.is_empty() {
        return Ok(());
    }
    let mut first = diagnostics.remove(0);
    first.related = diagnostics;
    Err(first)
}

/// Returns `line` up to its `//` comment, ignoring `//` inside string literals.
pub(super) fn strip_line_comment(line: &str) -> &str {
    let (mut in_string, mut escaped) = (false, false);
    for (index, byte) in line.bytes().enumerate() { match (in_string, escaped, byte) { (true, true, _) => escaped = false, (true, false, b'\\') => escaped = true, (true, false, b'"') => in_string = false, (false, _, b'"') => in_string = true, (false, _, b'/') if line.as_bytes().get(index + 1) == Some(&b'/') => return &line[..index], _ => {} } }
    line
}

#[cfg(test)]
mod tests {
    use super::super::parse_program;
    use std::path::Path;

    #[test]
    fn hash_outside_strings_reports_the_supported_comment_syntax_everywhere() {
        for (source, line, column) in [
            ("# leading\nprint 1\n", 1, 1),
            ("fn f(x: int): int {\n    # inner\n    return x\n}\nprint f(1)\n", 2, 5),
            // #1783 follow-ups: trailing, after `{`, and inside declaration bodies.
            ("print \"ok\" # trailing\n", 1, 12),
            ("let x: int = 1 # t\nprint x\n", 1, 16),
            ("import \"std/io.ax\" # c\nprint 1\n", 1, 20),
            ("fn f(): int { # c\n    return 1\n}\nprint f()\n", 1, 15),
            ("if true { # c\n    print 1\n}\n", 1, 11),
            ("struct P {\n    # c\n    x: int\n}\nprint 1\n", 2, 5),
            ("enum E {\n    # c\n    A\n}\nprint 1\n", 2, 5),
            ("match 1 {\n    # c\n    _ => print 1\n}\n", 2, 5),
            // Columns count characters, not bytes; an escaped quote stays in the string.
            ("print \"é\" # x\n", 1, 11),
            ("print \"a\\\"#\" # x\n", 1, 14),
        ] {
            let error = parse_program(source, Path::new("main.ax")).expect_err("hash comment");
            assert!(
                error.message.contains("use `//` for comments"),
                "{source:?}: {}",
                error.message
            );
            assert_eq!((error.line, error.column), (Some(line), Some(column)), "{source:?}");
        }
        // `#` inside a string literal and after `//` is ordinary text.
        parse_program(
            "print \"# not a comment\" // # still fine\nprint \"a\\\"#\"\n",
            Path::new("main.ax"),
        )
        .expect("hash inside string and comment");
    }

    #[test]
    fn every_hash_comment_is_reported_with_the_first_one_leading() {
        let error = parse_program("print 1 # a\nprint 2\n# b\n", Path::new("main.ax"))
            .expect_err("hash comments");
        assert_eq!((error.line, error.column), (Some(1), Some(9)));
        let related = error
            .related
            .iter()
            .map(|related| (related.line, related.column))
            .collect::<Vec<_>>();
        assert_eq!(related, vec![(Some(3), Some(1))]);
    }
}
