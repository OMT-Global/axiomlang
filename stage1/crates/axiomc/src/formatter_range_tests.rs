use super::*;

#[test]
fn stdin_range_without_line_ending_does_not_extend_replacement() {
    let source = "first\nsecond   \nthird\n";
    let start = "first\n".len();
    let end = start + "second   ".len();
    let (formatted, report) = format_axiom_stdin(
        source,
        false,
        Some(FormatRange {
            start_byte: start,
            end_byte: end,
        }),
    )
    .expect("stdin format report");

    assert_eq!(formatted, "first\nsecond\nthird\n");
    assert_eq!(report.changed, 1);
}

#[test]
fn stdin_range_rejects_split_crlf_boundary() {
    let source = "first\r\nsecond   \r\nthird\r\n";
    let start = "first\r\n".len();
    let end = start + "second   \r".len();
    let error = format_axiom_stdin(
        source,
        false,
        Some(FormatRange {
            start_byte: start,
            end_byte: end,
        }),
    )
    .expect_err("split CRLF range must fail");

    assert_eq!(error.code.as_deref(), Some("fmt.range.invalid"));
    assert!(
        error.message.contains("must not split a CRLF"),
        "{}",
        error.message
    );
}
