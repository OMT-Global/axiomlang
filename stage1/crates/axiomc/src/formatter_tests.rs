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
fn utf8_offsets_are_bytes_and_preserve_character_boundaries() {
    let original = "print \"café\"   \n";
    let formatted = format_axiom_source(original);
    let edits = format_edits(original, &formatted);

    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].start_byte, "print \"café\"".len());
    assert_eq!(edits[0].end_byte, "print \"café\"   ".len());
    assert_eq!(edits[0].replacement, "");
    assert_eq!(replay(original, &edits), formatted);
}

#[test]
fn crlf_edits_delete_only_carriage_returns() {
    let original = "fn main() {\r\n}\r\n";
    let formatted = format_axiom_source(original);
    let edits = format_edits(original, &formatted);

    assert_eq!(edits.len(), 2);
    assert!(edits.iter().all(|edit| edit.replacement.is_empty()));
    assert!(
        edits
            .iter()
            .all(|edit| edit.end_byte - edit.start_byte == 1)
    );
    assert_eq!(replay(original, &edits), formatted);
}

#[test]
fn empty_source_uses_an_insert_line_edit() {
    let formatted = format_axiom_source("");
    let edits = format_edits("", &formatted);

    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].action, "insert_line");
    assert_eq!(edits[0].start_byte, 0);
    assert_eq!(edits[0].end_byte, 0);
    assert_eq!(edits[0].replacement, "\n");
    assert_eq!(replay("", &edits), formatted);
}

#[test]
fn trailing_blank_lines_use_delete_line_edits() {
    let original = "fn main() {}\n\n\n";
    let formatted = format_axiom_source(original);
    let edits = format_edits(original, &formatted);

    assert_eq!(edits.len(), 2);
    assert!(edits.iter().all(|edit| edit.action == "delete_line"));
    assert_eq!(replay(original, &edits), formatted);
}

#[test]
fn reverse_replay_handles_multiple_non_ascii_edits() {
    let original = "print \"olá\"  \r\n\n\nprint \"fim\"   ";
    let formatted = format_axiom_source(original);
    let edits = format_edits(original, &formatted);

    assert!(edits.len() >= 3);
    assert_eq!(replay(original, &edits), formatted);
}
