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

fn registry_config(name: &str) -> String {
    format!(
        "[registry]\nname = \"{name}\"\nindex = \"https://registry.example.test/index.json\"\n\
         trust_roots = \"trust/roots.json\"\nexpectation = \"trust/expectation.json\"\n"
    )
}

#[test]
fn manifest_registry_dependencies_require_root_registry() {
    let v = validator();
    for name in ["official", "default", "custom_42-registry"] {
        for root in [PACKAGE, "[workspace]\nmembers = []\n"] {
            for paths in ["", "local = \"../local\"\nother = { path = \"../other\" }\n"] {
                let source = format!(
                    "{root}[dependencies]\n{paths}dep = {{ registry = \"{name}\", namespace = \"team\", version = \"^1.2.3\" }}\n"
                );
                assert_parity(&v, &source, false);
                assert_parity(&v, &format!("{source}{}", registry_config(name)), true);
            }
        }
    }
}

#[test]
fn manifest_without_registry_dependencies_does_not_require_root_registry() {
    let v = validator();
    for dependencies in [
        "",
        "[dependencies]\n",
        "[dependencies]\nlocal = \"../local\"\n",
        "[dependencies.local]\npath = \"../local\"\n",
        // A dependency *named* registry is not a registry source.
        "[dependencies.registry]\npath = \"../local\"\n",
    ] {
        for root in [PACKAGE, "[workspace]\nmembers = []\n"] {
            let source = format!("{root}{dependencies}");
            assert_parity(&v, &source, true);
            // The converse does not hold: an unused registry is allowed.
            assert_parity(&v, &format!("{source}{}", registry_config("custom")), true);
        }
    }
}

#[test]
fn manifest_registry_name_equality_remains_parser_only() {
    let v = validator();
    let source = format!(
        "{PACKAGE}{}[dependencies]\ndep = {{ registry = \"other\", namespace = \"team\", version = \"^1.2.3\" }}\n",
        registry_config("official")
    );
    let decoded: toml::Value = toml::from_str(&source).unwrap();
    let instance = serde_json::to_value(decoded).unwrap();
    // Standard JSON Schema cannot compare arbitrary instance strings. The root
    // prerequisite is supported; matching the configured name remains parser-only.
    assert!(v.is_valid(&instance));
    let error = parse_manifest_exact(source.as_bytes(), Path::new("axiom.toml")).unwrap_err();
    assert!(error.message.contains("configured registry"), "{error:?}");
}
