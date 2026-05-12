use axiomc::json_contract;
use jsonschema::Validator;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str) -> Value {
    let path = fixture_dir().join(name);
    serde_json::from_str(&fs::read_to_string(path).expect("read check fixture"))
        .expect("check fixture is valid JSON")
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("json-fixtures")
        .join("check")
}

fn schema_validator() -> Validator {
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("schemas")
        .join("axiom.stage1.v1.schema.json");
    let schema: Value =
        serde_json::from_str(&fs::read_to_string(schema_path).expect("read schema"))
            .expect("schema is valid JSON");
    jsonschema::validator_for(&schema).expect("compile stage1 JSON schema")
}

fn assert_check_envelope(payload: &Value, ok: bool) {
    assert_eq!(
        payload["schema_version"],
        json_contract::JSON_SCHEMA_VERSION
    );
    assert_eq!(payload["command"], "check");
    assert_eq!(payload["ok"], ok);
}

#[test]
fn check_fixtures_validate_against_stage1_v1_schema() {
    let validator = schema_validator();

    for entry in fs::read_dir(fixture_dir()).expect("read check fixtures") {
        let entry = entry.expect("read fixture entry");
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let payload: Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read fixture"))
                .expect("fixture is valid JSON");
        if let Err(error) = validator.validate(&payload) {
            panic!(
                "{} failed axiom.stage1.v1 schema validation: {error}",
                path.display()
            );
        }
    }
}

#[test]
fn check_success_fixture_matches_stage1_v1_envelope() {
    let payload = fixture("success.json");
    let generated = normalized_check_success_for_hello();

    assert_eq!(
        payload, generated,
        "success fixture drifted from axiomc check --json stage1/examples/hello"
    );
    assert_check_envelope(&payload, true);
    assert!(payload["project"].is_string());
    assert!(payload["manifest"].is_string());
    assert!(payload["entry"].is_string());
    assert!(payload["statement_count"].is_u64());
    assert!(payload["capabilities"].is_array());
    assert_eq!(
        capability_contract(&payload["capabilities"]),
        expected_capability_contract()
    );
    assert!(payload["warnings"].is_array());
    assert!(payload["packages"].is_array());
    let package = &payload["packages"][0];
    assert_eq!(
        capability_contract(&package["capabilities"]),
        expected_capability_contract()
    );
}

fn normalized_check_success_for_hello() -> Value {
    let stage1_root = stage1_root();
    let project = Path::new("stage1").join("examples").join("hello");
    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "check",
            project.to_str().expect("hello project path"),
            "--json",
        ])
        .current_dir(stage1_root.parent().expect("repo root"))
        .output()
        .expect("run axiomc check --json");
    assert!(
        output.status.success(),
        "axiomc check --json failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut payload: Value = serde_json::from_slice(&output.stdout).expect("parse check JSON");
    normalize_repo_paths(&mut payload, stage1_root.parent().expect("repo root"));
    payload
}

fn stage1_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("stage1 root")
}

fn normalize_repo_paths(value: &mut Value, repo_root: &Path) {
    match value {
        Value::String(text) => {
            let repo_root = repo_root.display().to_string();
            if text.starts_with(&repo_root) {
                *text = text.replacen(&repo_root, "/workspace/axiom", 1);
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_repo_paths(item, repo_root);
            }
        }
        Value::Object(map) => {
            for value in map.values_mut() {
                normalize_repo_paths(value, repo_root);
            }
        }
        _ => {}
    }
}

fn capability_contract(capabilities: &Value) -> Vec<(String, bool, String)> {
    capabilities
        .as_array()
        .expect("capabilities array")
        .iter()
        .map(|capability| {
            (
                capability["name"]
                    .as_str()
                    .expect("capability name")
                    .to_owned(),
                capability["enabled"].as_bool().expect("capability enabled"),
                capability["description"]
                    .as_str()
                    .expect("capability description")
                    .to_owned(),
            )
        })
        .collect()
}

fn expected_capability_contract() -> Vec<(String, bool, String)> {
    [
        ("fs", false, "filesystem read access"),
        ("fs:write", false, "filesystem write access"),
        ("net", false, "network access"),
        ("process", false, "child process execution"),
        ("env", false, "environment variable access"),
        ("clock", false, "wall-clock time access"),
        ("crypto", false, "hashing and cryptography primitives"),
        ("ffi", false, "foreign function interface access"),
        ("async", false, "host async runtime access"),
    ]
    .into_iter()
    .map(|(name, enabled, description)| (name.to_owned(), enabled, description.to_owned()))
    .collect()
}

#[test]
fn check_error_fixtures_cover_required_diagnostic_classes() {
    let cases = [
        ("parse-error.json", "parse", None),
        ("type-error.json", "type", None),
        ("borrow-error.json", "ownership", Some("use_after_move")),
        ("capability-denial.json", "capability", None),
    ];

    for (name, kind, code) in cases {
        let payload = fixture(name);
        assert_check_envelope(&payload, false);
        assert_eq!(payload["error"]["kind"], kind);
        assert!(payload["error"]["message"].is_string());
        assert!(payload["error"]["path"].is_string());
        assert!(payload["error"]["line"].is_u64());
        assert!(payload["error"]["column"].is_u64());
        if let Some(code) = code {
            assert_eq!(payload["error"]["code"], code);
        }
    }
}
