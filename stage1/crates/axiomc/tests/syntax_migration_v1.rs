use axiomc::diagnostics::Diagnostic;
use axiomc::syntax::{
    parse_program, parse_program_with_options, parse_program_with_recovery, ParseOptions, Stmt,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../compiler-contracts/fixtures/syntax-migration-v1")
        .join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(path).expect("read syntax fixture"))
        .expect("parse syntax fixture")
}

fn input(document: &Value) -> (&str, &Path) {
    let input = document.get("input").expect("fixture input");
    let source = input
        .get("source")
        .and_then(Value::as_str)
        .expect("fixture source");
    let path = input
        .get("path")
        .and_then(Value::as_str)
        .expect("fixture path");
    (source, Path::new(path))
}

fn diagnostic_record(diagnostic: &Diagnostic) -> Value {
    let normalized = diagnostic.normalized_for_json();
    json!({
        "kind": normalized.kind,
        "code": normalized.code,
        "message": normalized.message,
        "path": normalized.path,
        "line": normalized.line,
        "column": normalized.column,
    })
}

#[test]
fn bootstrap_recovery_diagnostics_match_fixture() {
    let document = fixture("bootstrap-recovery-diagnostics");
    let (source, path) = input(&document);
    let diagnostics = parse_program_with_recovery(source, path)
        .expect_err("fixture must produce ordered diagnostics");
    let actual = diagnostics
        .iter()
        .map(diagnostic_record)
        .collect::<Vec<_>>();
    assert_eq!(
        Value::Array(actual),
        document["expected"]["compiler_diagnostics"]
    );
}

#[test]
fn bootstrap_macro_provenance_matches_fixture() {
    let document = fixture("bootstrap-macro-provenance");
    let (source, path) = input(&document);
    let program = parse_program(source, path).expect("fixture must parse");
    let actual = program
        .macro_expansions
        .iter()
        .map(|expansion| {
            json!({
                "macro_name": expansion.macro_name,
                "depth": expansion.depth,
                "definition_span": {
                    "path": expansion.definition_site.path,
                    "line": expansion.definition_site.line,
                    "column": expansion.definition_site.column,
                },
                "call_span": {
                    "path": expansion.call_site.path,
                    "line": expansion.call_site.line,
                    "column": expansion.call_site.column,
                },
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        Value::Array(actual),
        document["expected"]["macro_provenance"]
    );
}

#[test]
fn bootstrap_macro_limits_match_fixtures() {
    for name in [
        "bootstrap-macro-byte-limit",
        "bootstrap-macro-invocation-limit",
        "bootstrap-macro-recursion-limit",
    ] {
        let document = fixture(name);
        let (source, path) = input(&document);
        let qualification = &document["qualification"];
        let option = qualification["option"].as_str().expect("option");
        let value = qualification["value"].as_u64().expect("value") as usize;
        let mut options = ParseOptions::default();
        match option {
            "macro_expansion_byte_limit" => options.macro_expansion_byte_limit = value,
            "macro_expansion_invocation_limit" => options.macro_expansion_invocation_limit = value,
            "macro_recursion_limit" => options.macro_recursion_limit = value,
            other => panic!("unknown macro option {other}"),
        }
        let diagnostic = parse_program_with_options(source, path, &options)
            .expect_err("bounded macro fixture must fail");
        assert_eq!(
            Value::Array(vec![diagnostic_record(&diagnostic)]),
            document["expected"]["compiler_diagnostics"],
            "fixture {name}",
        );
    }
}

#[test]
fn bootstrap_node_identity_matches_fixture() {
    let document = fixture("bootstrap-node-identity");
    let (source, path) = input(&document);
    let first = parse_program(source, path).expect("first parse");
    let second = parse_program(source, path).expect("second parse");
    let first_enum = &first.enums[0];
    let second_enum = &second.enums[0];
    assert_eq!(
        first_enum.stable_id_in(&first.path),
        second_enum.stable_id_in(&second.path),
    );
    let expected = &document["expected"]["node_identities"][0];
    assert_eq!(first_enum.name, expected["name"].as_str().unwrap());
    assert_eq!(first_enum.stable_id_in(&first.path).0, expected["id"]);
    assert_eq!(expected["canonical_axiom_id"], false);
    let span = first_enum.span_in(&first.path);
    assert_eq!(
        json!({"kind": "enum", "line": span.line, "column": span.column}),
        document["expected"]["spans"][0],
    );
}

#[test]
fn bootstrap_line_comment_coordinates_match_fixture() {
    let document = fixture("bootstrap-line-comment-coordinates");
    let (source, path) = input(&document);
    let program = parse_program(source, path).expect("comment fixture must parse");
    let actual = program
        .stmts
        .iter()
        .map(|statement| {
            let kind = match statement {
                Stmt::Let { .. } => "let",
                Stmt::Print { .. } => "print",
                other => panic!("unexpected statement {other:?}"),
            };
            let span = statement.span_in(&program.path);
            json!({"kind": kind, "line": span.line, "column": span.column})
        })
        .collect::<Vec<_>>();
    assert_eq!(Value::Array(actual), document["expected"]["spans"]);
}
