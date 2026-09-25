use super::*;

fn replay(original: &str, edits: &[FormatEdit]) -> String {
    let mut replayed = original.to_string();
    for edit in edits.iter().rev() {
        replayed.replace_range(edit.start_byte..edit.end_byte, &edit.replacement);
    }
    replayed
}

#[test]
fn utf8_offsets_are_bytes_and_preserve_boundaries() {
    let original = "print \"café\"   \n";
    let formatted = format_axiom_source(original);
    let edits = format_edits(original, &formatted);
    assert_eq!(edits[0].start_byte, "print \"café\"".len());
    assert_eq!(replay(original, &edits), formatted);
}

#[test]
fn crlf_and_blank_line_edits_replay() {
    for original in ["fn main() {\r\n}\r\n", "fn main() {}\n\n\n", ""] {
        let formatted = format_axiom_source(original);
        assert_eq!(replay(original, &format_edits(original, &formatted)), formatted);
    }
}

#[test]
fn reverse_replay_handles_multiple_non_ascii_edits() {
    let original = "print \"olá\"  \r\n\n\nprint \"fim\"   ";
    let formatted = format_axiom_source(original);
    assert_eq!(replay(original, &format_edits(original, &formatted)), formatted);
}

#[test]
fn syntax_aware_layout_is_canonical_and_idempotent() {
    let source = "fn  main( value:int ){\nif(value>=2){\nprint  value+1\n}else{\nprint  0\n}\n}\n";
    let expected = "fn main(value: int) {\n    if (value >= 2) {\n        print value + 1\n    } else {\n        print 0\n    }\n}\n";
    let formatted = format_axiom_source(source);
    assert_eq!(formatted, expected);
    assert_eq!(format_axiom_source(&formatted), formatted);
}

#[test]
fn literals_comments_and_doc_text_are_lexically_opaque() {
    let source = "///  Keep {  spaces } and \\t docs\nfn main(){\nprint \"a  { # // }  b\"   #  keep   comment {\nprint '}'\n}\n";
    let formatted = format_axiom_source(source);
    assert!(formatted.contains("///  Keep {  spaces } and \\t docs"));
    assert!(formatted.contains("\"a  { # // }  b\" #  keep   comment {"));
    assert!(formatted.contains("print '}'"));
}

#[test]
fn macro_token_tree_content_is_preserved_verbatim() {
    let source = "macro build {\n  ($x:expr)=>{  emit(\"{  }\", $x)  }\n}\nfn main(){print 1}\n";
    let formatted = format_axiom_source(source);
    assert!(formatted.starts_with("macro build {\n  ($x:expr)=>{  emit(\"{  }\", $x)  }\n}\n"));
    assert_eq!(format_axiom_source(&formatted), formatted);
}

#[test]
fn contiguous_imports_sort_with_associated_comments() {
    let source = "# z docs\nimport \"z.ax\"\n# a docs\nimport \"a.ax\"\n\nimport \"d.ax\"\nimport \"c.ax\"\n\nprint 1\n";
    assert_eq!(format_axiom_source(source), "# a docs\nimport \"a.ax\"\n# z docs\nimport \"z.ax\"\n\nimport \"c.ax\"\nimport \"d.ax\"\n\nprint 1\n");
}

#[test]
fn malformed_input_does_not_panic() {
    let source = "fn broken( {\nprint \"unterminated { # text\n}}}\n";
    let once = format_axiom_source(source);
    assert_eq!(format_axiom_source(&once), once);
}

#[test]
fn mutable_borrow_formatting_round_trips_through_parser() {
    let source = "fn bump(values: &mut [int]): int {
return values[0]
}
";
    let formatted = format_axiom_source(source);
    assert!(formatted.contains("values: &mut [int]"), "{formatted}");
    axiomc::syntax::parse_program(&formatted, std::path::Path::new("mutable-borrow.ax"))
        .expect("formatted mutable-borrow type must parse");
    assert_eq!(format_axiom_source(&formatted), formatted);
}

#[test]
fn parenthesized_statement_expressions_round_trip_through_parser() {
    let source = "fn verdict(input: int): bool {
return (input == input) == true
}
print (1 + 2)
";
    let formatted = format_axiom_source(source);
    assert!(formatted.contains("return (input == input)"), "{formatted}");
    assert!(formatted.contains("print (1 + 2)"), "{formatted}");
    axiomc::syntax::parse_program(&formatted, std::path::Path::new("statement-expressions.ax"))
        .expect("formatted statement expressions must parse");
    assert_eq!(format_axiom_source(&formatted), formatted);
}
