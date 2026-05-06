use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn cli_json_outputs_match_checked_in_contract_snapshots() {
    let contracts = contract_root();
    let schema = read_json(&contracts.join("schemas/axiom.stage1.command.schema.json"));
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("contract-app");

    run_axiomc(&["new", project.to_str().expect("project path"), "--name", "contract-app"]);

    for command in ["check", "build", "test", "caps"] {
        let mut args = vec![command, project.to_str().expect("project path"), "--json"];
        if command == "caps" {
            args = vec![command, project.to_str().expect("project path"), "--json"];
        }
        let output = run_axiomc_json(&args);
        assert_schema_required_fields(&schema, &output);

        let normalized = normalize_payload(output, &project);
        let snapshot = read_json(&contracts.join(format!("snapshots/{command}.json")));
        assert_eq!(normalized, snapshot, "{command} JSON contract drifted");
    }
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

fn assert_schema_required_fields(schema: &Value, payload: &Value) {
    let command = payload["command"].as_str().expect("payload command");
    let variants = schema["oneOf"].as_array().expect("schema oneOf");
    let variant = variants
        .iter()
        .find(|variant| variant["properties"]["command"]["const"] == command)
        .unwrap_or_else(|| panic!("schema variant for command {command}"));
    for field in variant["required"].as_array().expect("required fields") {
        let field = field.as_str().expect("required field name");
        assert!(
            payload.get(field).is_some(),
            "{command} payload is missing required field {field}"
        );
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
        Value::String(text) => {
            if let Some(project) = project_aliases.iter().find(|project| text.starts_with(*project))
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
