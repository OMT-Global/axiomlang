use jsonschema::Validator;
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn cli_json_outputs_match_checked_in_contract_snapshots() {
    let contracts = contract_root();
    let schema = read_json(&contracts.join("schemas/axiom.stage1.command.schema.json"));
    let validator = jsonschema::validator_for(&schema).expect("compile JSON contract schema");
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("contract-app");

    run_axiomc(&[
        "new",
        project.to_str().expect("project path"),
        "--name",
        "contract-app",
    ]);

    for command in ["check", "build", "test", "caps"] {
        let mut args = vec![command, project.to_str().expect("project path"), "--json"];
        if command == "caps" {
            args = vec![command, project.to_str().expect("project path"), "--json"];
        }
        let output = run_axiomc_json(&args);
        assert_payload_matches_schema(&validator, command, &output);

        let normalized = normalize_payload(output, &project);
        let snapshot = read_json(&contracts.join(format!("snapshots/{command}.json")));
        assert_eq!(normalized, snapshot, "{command} JSON contract drifted");
    }
}

#[test]
fn doc_json_failure_uses_error_contract() {
    let temp = tempfile::tempdir().expect("tempdir");
    let missing = temp.path().join("missing-doc-project");
    let out_dir = temp.path().join("docs");
    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "doc",
            missing.to_str().expect("missing path"),
            "--out-dir",
            out_dir.to_str().expect("out dir"),
            "--json",
        ])
        .output()
        .expect("run failing axiomc doc --json");

    assert!(
        !output.status.success(),
        "doc --json should fail for missing input"
    );
    assert!(
        output.stderr.is_empty(),
        "JSON failures should not use stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON error payload");
    assert_eq!(payload["ok"], false);
    assert_eq!(payload["command"], "doc");
    assert!(payload.get("error").is_some(), "missing JSON error object");
}

fn contract_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../compiler-contracts")
        .canonicalize()
        .expect("contract root")
}

fn run_axiomc(args: &[&str]) {
    let status = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(args)
        .status()
        .expect("run axiomc");
    assert!(status.success(), "axiomc {args:?} failed with {status}");
}

fn run_axiomc_json(args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(args)
        .output()
        .expect("run axiomc json command");
    assert!(
        output.status.success(),
        "axiomc {args:?} failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse axiomc json")
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).expect("read json")).expect("parse json")
}

fn assert_payload_matches_schema(validator: &Validator, command: &str, payload: &Value) {
    if let Err(error) = validator.validate(payload) {
        panic!("{command} JSON payload failed schema validation: {error}");
    }
}

fn normalize_payload(mut payload: Value, project: &Path) -> Value {
    let aliases = vec![
        project.display().to_string(),
        project
            .canonicalize()
            .expect("canonical project path")
            .display()
            .to_string(),
    ];
    normalize_value(&mut payload, &aliases, None);
    payload
}

fn normalize_value(value: &mut Value, project_aliases: &[String], key: Option<&str>) {
    match value {
        Value::String(text) if key.is_some_and(|key| key.ends_with("_hash")) => {
            *text = "<hash>".to_string();
        }
        Value::String(text) => {
            if let Some(project) = project_aliases
                .iter()
                .find(|project| text.starts_with(*project))
            {
                *text = text.replacen(project, "<project>", 1);
            }
        }
        Value::Number(_) if matches!(key, Some("duration_ms" | "compile_ms")) => {
            *value = Value::from(0);
        }
        Value::Array(items) => {
            for item in items {
                normalize_value(item, project_aliases, None);
            }
        }
        Value::Object(map) => normalize_object(map, project_aliases),
        _ => {}
    }
}

fn normalize_object(map: &mut Map<String, Value>, project_aliases: &[String]) {
    for (key, value) in map {
        normalize_value(value, project_aliases, Some(key));
    }
}
