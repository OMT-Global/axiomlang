//! Issue #1786: common expression mistakes get a targeted parse diagnostic
//! with a span and a stable code instead of "invalid identifier" or a
//! tuple-index type error.
use axiomc::syntax;
use std::path::Path;

fn parse_error(source: &str) -> axiomc::diagnostics::Diagnostic {
    syntax::parse_program(source, Path::new("main.ax")).expect_err("source must be rejected")
}

fn assert_parse_error(source: &str, expected: &str, line: usize, column: usize) {
    let error = parse_error(source);
    assert_eq!(error.kind, "parse", "{error:?}");
    assert!(error.message.contains(expected), "{}", error.message);
    assert!(!error.message.contains("invalid identifier"), "{}", error.message);
    assert_eq!(error.path.as_deref(), Some("main.ax"), "{error:?}");
    assert_eq!((error.line, error.column), (Some(line), Some(column)), "{error:?}");
}

#[test]
fn unsuffixed_float_literal_asks_for_a_type_suffix() {
    assert_parse_error("print 1.5 + 2.25\n", "float literal 1.5 needs a type suffix", 1, 7);
    // A `let` initializer reports the parser's existing initializer column.
    let error = parse_error("let x: f64 = 0.25\n");
    assert!(error.message.contains("for example 0.25f64"), "{}", error.message);
    assert_eq!(error.line, Some(1), "{error:?}");
}

#[test]
fn out_of_range_integer_literal_is_named_as_such() {
    assert_parse_error(
        "print 9223372036854775808\n",
        "integer literal 9223372036854775808 is out of range for `int`",
        1,
        7,
    );
    assert_parse_error("print -9223372036854775809\n", "is out of range for `int`", 1, 7);
}

#[test]
fn unsupported_operators_are_named() {
    assert_parse_error("let a: bool = true\nprint !a\n", "unary `!` is not supported", 2, 7);
    assert_parse_error("let x: int = 7\nprint x % 2\n", "operator `%` is not supported", 2, 7);
}

#[test]
fn chained_comparison_suggests_parentheses() {
    assert_parse_error("print 1 < 2 == true\n", "comparison operators cannot be chained", 1, 7);
}

#[test]
fn valid_neighbouring_forms_still_parse() {
    for source in [
        "print 1.5f64 + 2.25f64\n",
        "let t: (int, int) = (1, 2)\nprint t.0\n",
        "let p: ((int, int), int) = ((1, 2), 3)\nprint p.0.1\n",
        "print \"50% off\" + \"!\"\n",
        "let a: bool = true\nprint a != false\n",
        "print -9223372036854775808\n",
        "print (1 < 2) == true\n",
    ] {
        syntax::parse_program(source, Path::new("main.ax"))
            .unwrap_or_else(|error| panic!("{source:?} must parse: {error:?}"));
    }
}

#[test]
fn each_mistake_has_a_stable_catalogued_code() {
    for (source, code) in [
        ("print 1.5 + 2.25\n", "parse.float_literal_requires_suffix"),
        ("print 9223372036854775808\n", "parse.integer_literal_out_of_range"),
        ("let a: bool = true\nprint !a\n", "parse.unsupported_operator"),
        ("let x: int = 7\nprint x % 2\n", "parse.unsupported_operator"),
        ("print 1 < 2 == true\n", "parse.chained_comparison"),
    ] {
        let error = parse_error(source);
        assert_eq!(error.code.as_deref(), Some(code), "{source:?}: {error:?}");
        let info = axiomc::diagnostic_catalog::diagnostic_code_info(code)
            .unwrap_or_else(|| panic!("{code} must be in the diagnostic catalog"));
        assert_eq!(info.kind, "parse");
    }
}
