//! Scoped parity between editor metadata and the authoritative TOML parser.
//! This matrix does not claim full manifest-semantic or filesystem validation.
use axiomc::manifest::parse_manifest_exact;
use jsonschema::Validator;
use serde_json::Value;
use std::{fs, path::Path};

const PACKAGE: &str = "[package]\nname = \"sample\"\nversion = \"0.1.0\"\n";

fn validator() -> Validator {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/axiom.toml.schema.json");
    let schema: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    jsonschema::validator_for(&schema).expect("compile manifest schema")
}

fn assert_parity(validator: &Validator, source: &str, expected: bool) {
    let decoded: toml::Value = toml::from_str(source).expect("test input must be valid TOML");
    let instance = serde_json::to_value(decoded).unwrap();
    let parser = parse_manifest_exact(source.as_bytes(), Path::new("axiom.toml")).is_ok();
    assert_eq!(parser, expected, "parser changed for {source:?}");
    assert_eq!(
        validator.is_valid(&instance),
        expected,
        "schema disagrees for {source:?}"
    );
}

#[test]
fn manifest_defaults_apply_only_when_build_table_is_absent() {
    let v = validator();
    for source in [PACKAGE, "[workspace]\n", "[workspace]\nmembers = []\n"] {
        assert_parity(&v, source, true);
    }
    assert_parity(&v, "", false);
    for fields in ["", "entry = \"src/main.ax\"\n", "out_dir = \"dist\"\n"] {
        assert_parity(&v, &format!("{PACKAGE}\n[build]\n{fields}"), false);
    }
    assert_parity(
        &v,
        &format!("{PACKAGE}\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n"),
        true,
    );
}

#[test]
fn manifest_build_requires_package_even_in_workspace() {
    let v = validator();
    let build = "[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n";
    for workspace in ["", "[workspace]\n", "[workspace]\nmembers = []\n"] {
        assert_parity(&v, &format!("{workspace}{build}"), false);
        assert_parity(&v, &format!("{PACKAGE}{workspace}{build}"), true);
    }
}

#[test]
fn manifest_build_paths_reject_blank_but_allow_embedded_spaces() {
    let v = validator();
    // TOML string literals below include whitespace escapes, decoded before validation.
    for value in ["\"\"", "\" \"", "\"\\t\"", "\"\\r\\n\"", "\"\\u2003\""] {
        for field in ["entry", "out_dir"] {
            let entry = if field == "entry" {
                value
            } else {
                "\"src/main.ax\""
            };
            let out = if field == "out_dir" {
                value
            } else {
                "\"dist\""
            };
            assert_parity(
                &v,
                &format!("{PACKAGE}[build]\nentry = {entry}\nout_dir = {out}\n"),
                false,
            );
        }
    }
    assert_parity(
        &v,
        &format!("{PACKAGE}[build]\nentry = \"src/my main.ax\"\nout_dir = \"build output\"\n"),
        true,
    );
}

#[test]
fn manifest_dependency_paths_reject_blank_in_both_forms() {
    let v = validator();
    for (value, valid) in [
        ("\"\"", false),
        ("\" \"", false),
        ("\"\\t\"", false),
        ("\"\\u2003\"", false),
        ("\"../some lib\"", true),
        ("\"../lib\"", true),
    ] {
        assert_parity(
            &v,
            &format!("{PACKAGE}[dependencies]\nlib = {value}\n"),
            valid,
        );
        assert_parity(
            &v,
            &format!("{PACKAGE}[dependencies.lib]\npath = {value}\n"),
            valid,
        );
    }
}
