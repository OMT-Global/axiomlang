use super::*;

fn replay(original: &str, edits: &[FormatEdit]) -> String {
    let mut replayed = original.to_string();
    for edit in edits.iter().rev() {
        assert!(edit.start_byte <= edit.end_byte);
        assert!(original.is_char_boundary(edit.start_byte));
        assert!(original.is_char_boundary(edit.end_byte));
        replayed.replace_range(edit.start_byte..edit.end_byte, &edit.replacement);
    }
    replayed
}

#[test]
fn formatter_trims_whitespace_and_collapses_blank_runs() {
    assert_eq!(
        format_axiom_source("fn main() {   \n\tprint \"hi\"  \n\n\n}\n\n"),
        "fn main() {\n    print \"hi\"\n\n}\n"
    );
}

#[test]
fn formatter_check_reports_schema_and_precise_edits_without_writing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("src/main.ax");
    fs::create_dir_all(source.parent().expect("source parent")).expect("mkdir");
    let original = "fn main() {   \n\tprint \"hi\"  \n\n\n}\n\n";
    fs::write(&source, original).expect("write source");

    let report = format_axiom_sources(dir.path(), true).expect("format report");

    assert_eq!(report.schema_version, json_contract::JSON_SCHEMA_VERSION);
    assert_eq!(report.schema, FORMAT_SCHEMA_PATH);
    assert_eq!(report.command, "fmt");
    assert!(!report.ok);
    assert!(report.check);
    assert_eq!(report.input, "files");
    assert_eq!(report.changed, 1);
    assert_eq!(report.files.len(), 1);
    assert!(report.files[0].changed);
    assert_eq!(
        replay(original, &report.files[0].edits),
        format_axiom_source(original)
    );
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(
        FORMAT_SCHEMA_PATH
            .strip_prefix("stage1/")
            .expect("stage1 schema path"),
    );
    let schema: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(schema_path).expect("read formatter schema"))
            .expect("formatter schema JSON");
    jsonschema::validator_for(&schema)
        .expect("compile formatter schema")
        .validate(&serde_json::to_value(&report).expect("serialize formatter report"))
        .expect("formatter report matches schema");
    assert_eq!(fs::read_to_string(&source).expect("read source"), original);
}

#[test]
fn stdin_range_formats_only_requested_utf8_byte_slice() {
    let source = "first   \nsecond   \nthird   \n";
    let start = "first   \n".len();
    let end = start + "second   \n".len();
    let (formatted, report) = format_axiom_stdin(
        source,
        false,
        Some(FormatRange {
            start_byte: start,
            end_byte: end,
        }),
    )
    .expect("stdin format report");

    assert_eq!(formatted, "first   \nsecond\nthird   \n");
    assert_eq!(report.input, "stdin");
    assert_eq!(report.changed, 1);
    assert_eq!(report.files[0].path, "<stdin>");
    assert_eq!(report.files[0].range.unwrap().start_byte, start);
    assert_eq!(replay(source, &report.files[0].edits), formatted);
}

#[test]
fn stdin_check_is_non_mutating_and_reports_work() {
    let source = "fn main() {}   \n";
    let (formatted, report) = format_axiom_stdin(source, true, None).expect("stdin check");
    assert_eq!(formatted, "fn main() {}\n");
    assert!(!report.ok);
    assert!(report.check);
    assert_eq!(report.changed, 1);
}

#[test]
fn stdin_range_rejects_non_utf8_boundaries() {
    let error = format_axiom_stdin(
        "café\n",
        false,
        Some(FormatRange {
            start_byte: 4,
            end_byte: 5,
        }),
    )
    .expect_err("split code point must fail");
    assert_eq!(error.code.as_deref(), Some("fmt.range.invalid"));
}

#[test]
fn missing_final_newline_is_an_exact_insertion() {
    let original = "fn main() {}";
    let formatted = format_axiom_source(original);
    let edits = format_edits(original, &formatted);

    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].action, "replace_line");
    assert_eq!(edits[0].line, 1);
    assert_eq!(edits[0].before.as_deref(), Some("fn main() {}"));
    assert_eq!(edits[0].after.as_deref(), Some("fn main() {}"));
    assert_eq!(edits[0].start_byte, original.len());
    assert_eq!(edits[0].end_byte, original.len());
    assert_eq!(edits[0].replacement, "\n");
    assert_eq!(replay(original, &edits), formatted);
}

#[test]
fn formatter_keeps_generic_arguments_tight() {
    assert_eq!(
        format_axiom_source("pub fn worker_name(raw: Option<string>): string {\nreturn raw\n}\n"),
        "pub fn worker_name(raw: Option<string>): string {\n    return raw\n}\n"
    );
    assert_eq!(
        format_axiom_source("fn main() {\nlet configured: Once < string > = once_with < string > (\"default\")\n}\n"),
        "fn main() {\n    let configured: Once<string> = once_with<string>(\"default\")\n}\n"
    );
    assert_eq!(
        format_axiom_source("fn main() {\nlet m: Map<string, Option<int>> = defaults()\n}\n"),
        "fn main() {\n    let m: Map<string, Option<int>> = defaults()\n}\n"
    );
}

#[test]
fn formatter_preserves_spacing_of_comparison_operators() {
    assert_eq!(
        format_axiom_source("fn main() {\nlet ok: bool = count < limit && count >= 0\n}\n"),
        "fn main() {\n    let ok: bool = count < limit && count >= 0\n}\n"
    );
}

#[test]
fn formatter_keeps_borrow_operator_tight() {
    assert_eq!(
        format_axiom_source("fn tail(values: & [int]): &[int] {\nreturn values[1:]\n}\n"),
        "fn tail(values: &[int]): &[int] {\n    return values[1:]\n}\n"
    );
}
