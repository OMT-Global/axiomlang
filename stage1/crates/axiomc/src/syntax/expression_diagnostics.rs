//! Targeted diagnostics for expression text the term parser cannot accept.
//!
//! Without these, an oversized number, an unsuffixed float, or an unsupported
//! operator falls through to "invalid identifier" or a tuple-index type error.

use super::{NUMERIC_LITERAL_SUFFIXES, find_compare_operator, find_top_level_char};
use crate::diagnostics::Diagnostic;
use std::path::Path;

fn parse_error(message: String, path: &Path, line_no: usize, column: usize) -> Diagnostic {
    Diagnostic::new("parse", message)
        .with_path(path.display().to_string())
        .with_span(line_no, column)
}

/// `1.5` reaches the tuple-index branch as base `1`, index `5`. Float literals
/// need a type suffix, so say that instead of reporting a tuple-index error.
pub(super) fn unsuffixed_float_diagnostic(
    base_raw: &str,
    fraction: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Option<Diagnostic> {
    let whole = base_raw.trim();
    if whole.is_empty() || !whole.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some(parse_error(
        format!(
            "float literal {whole}.{fraction} needs a type suffix, for example {whole}.{fraction}f64"
        ),
        path,
        line_no,
        column,
    ))
}

/// Names the mistake when text that reached the identifier fallthrough is a
/// recognisable unsupported expression.
pub(super) fn unsupported_expression_diagnostic(
    raw: &str,
    path: &Path,
    line_no: usize,
    column: usize,
) -> Option<Diagnostic> {
    unsupported_expression_message(raw).map(|message| parse_error(message, path, line_no, column))
}

fn unsupported_expression_message(raw: &str) -> Option<String> {
    let digits = raw.strip_prefix('-').unwrap_or(raw);
    if !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit()) {
        return Some(format!(
            "integer literal {raw} is out of range for `int` ({} to {})",
            i64::MIN,
            i64::MAX
        ));
    }
    if raw.starts_with('!') && !raw.starts_with("!=") {
        return Some(
            "unary `!` is not supported in stage1; compare with `== false` instead".to_string(),
        );
    }
    if find_top_level_char(raw, '%').is_some() {
        return Some("operator `%` is not supported in stage1".to_string());
    }
    if find_compare_operator(raw).is_some() {
        return Some(format!(
            "comparison operators cannot be chained in {raw:?}; parenthesize one comparison"
        ));
    }
    None
}

pub(super) fn looks_like_invalid_numeric_literal(raw: &str) -> bool {
    NUMERIC_LITERAL_SUFFIXES.iter().any(|suffix| {
        let Some(number) = raw.strip_suffix(suffix) else {
            return false;
        };
        !number.is_empty()
            && (number.parse::<f64>().is_ok()
                || number
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_digit() || ch == '-' || ch == '.'))
    })
}
