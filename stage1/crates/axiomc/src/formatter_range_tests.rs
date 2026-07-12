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
