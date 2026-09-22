//! Compile and execute the actual emitted JSON helper, not a reimplementation.
use super::json_serdes::render_json_serdes_support;
use crate::cranelift_backend::json_serdes_parse_document;
use std::process::Command;

#[test]
fn structured_json_errors_execute_in_both_backends() {
    let cases = [
        ("", 0, "$"),
        ("x", 0, "$"),
        ("{\"outer\":[true,}", 15, "$.outer[1]"),
        ("{\"é\":[true,}", 12, "$[\"\\u00e9\"][1]"),
        ("{\"a.b\":?}", 7, "$[\"a.b\"]"),
        ("[1] false", 4, "$"),
        ("[1", 2, "$"),
        ("{\"a\":}", 5, "$.a"),
        ("{\"a\" ?}", 5, "$.a"),
        ("[1,2,]", 5, "$[2]"),
    ];
    let mut source = String::from(r#"
#![allow(non_camel_case_types, dead_code)]
use std::collections::HashMap;
#[derive(Debug)]
enum std_serdes_Value { Null, Bool(bool), Int(i64), Float(f64), Text(String), Array(Vec<Self>), Object(HashMap<String, Self>) }
#[derive(Debug)]
struct std_serdes_ParseError { message: String, offset: i64, path: String }
fn axiom_json_stringify_bool(value: bool) -> String { value.to_string() }
fn axiom_json_stringify_int(value: i64) -> String { value.to_string() }
"#);
    render_json_serdes_support(&mut source);
    source.push_str("\nfn main() {\n");
    for (input, offset, path) in cases {
        let error = json_serdes_parse_document(input).expect_err("native must reject malformed JSON");
        assert_eq!(error.offset, offset, "native byte offset for {input:?}");
        assert_eq!(error.path, path, "native path for {input:?}");
        assert!(!error.message.is_empty());
        source.push_str(&format!(
            "let e = axiom_json_serdes_parse_str({input:?}).expect_err(\"generated must reject malformed JSON\"); assert_eq!(e.offset, {offset}); assert_eq!(e.path, {path:?}); assert!(!e.message.is_empty());\n"
        ));
    }
    for input in ["null", "{\"é\":[true, null, 1.25]}", "\"\\uD834\\uDD1E\""] {
        assert!(json_serdes_parse_document(input).is_ok(), "native valid JSON {input:?}");
        source.push_str(&format!("assert!(axiom_json_serdes_parse_str({input:?}).is_ok());\n"));
    }
    for input in ["\"line\nbreak\"", "{\"a\":1,}", "\"\\uD800\""] {
        assert!(json_serdes_parse_document(input).is_err(), "native rejects invalid JSON {input:?}");
        source.push_str(&format!("assert!(axiom_json_serdes_parse_str({input:?}).is_err());\n"));
    }
    source.push_str(r#"
let oversized = "x".repeat(AXIOM_JSON_MAX_DOCUMENT_BYTES + 1);
let e = axiom_json_serdes_parse_str(&oversized).expect_err("document bound");
assert_eq!(e.offset, AXIOM_JSON_MAX_DOCUMENT_BYTES as i64);
assert_eq!(e.path, "$");
let nested = "[".repeat(AXIOM_JSON_MAX_DEPTH + 1) + &"]".repeat(AXIOM_JSON_MAX_DEPTH + 1);
assert!(axiom_json_serdes_parse_str(&nested).unwrap_err().message.contains("level limit"));
let collection = format!("[{}]", vec!["0"; AXIOM_JSON_MAX_COLLECTION_ITEMS + 1].join(","));
assert!(axiom_json_serdes_parse_str(&collection).unwrap_err().message.contains("item limit"));
let number = "1".repeat(AXIOM_JSON_MAX_NUMBER_DIGITS + 1);
assert!(axiom_json_serdes_parse_str(&number).unwrap_err().message.contains("digit limit"));
"#);
    source.push_str("println!(\"STRUCTURED_JSON_RUNTIME_COMPLETE\");\n}\n");
    let temp = tempfile::tempdir().expect("runtime scratch");
    let src = temp.path().join("json_runtime.rs");
    let bin = temp.path().join("json_runtime");
    std::fs::write(&src, source).expect("write emitted runtime");
    let compilation = Command::new("rustc").arg("--edition=2021").arg(&src).arg("-o").arg(&bin).output().expect("run rustc");
    assert!(compilation.status.success(), "generated runtime must compile: {}", String::from_utf8_lossy(&compilation.stderr));
    let execution = Command::new(bin).output().expect("execute emitted parser");
    assert!(execution.status.success(), "generated runtime assertions: {}", String::from_utf8_lossy(&execution.stderr));
    assert!(String::from_utf8_lossy(&execution.stdout).contains("STRUCTURED_JSON_RUNTIME_COMPLETE"));
}
