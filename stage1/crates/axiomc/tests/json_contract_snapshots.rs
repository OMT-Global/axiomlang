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

    let mutation_input = temp.path().join("mutation-survivors.json");
    fs::write(
        &mutation_input,
        r#"{"survivors":[{"id":"m1","file":"src/main.ax","function":"main","line":1,"mutator":"replace_literal","description":"changed greeting","status":"survived"}]}"#,
    )
    .expect("write mutation input");
    let project_str = project.to_str().expect("project path");
    let mutation_input_str = mutation_input.to_str().expect("mutation input path");
    let invocations: [(&str, Vec<&str>); 6] = [
        ("check", vec!["check", project_str, "--json"]),
        ("build", vec!["build", project_str, "--json"]),
        ("test", vec!["test", project_str, "--json"]),
        ("caps", vec!["caps", project_str, "--json"]),
        ("run", vec!["run", project_str, "--json"]),
        (
            "mutation-report",
            vec!["mutation-report", mutation_input_str, "--json"],
        ),
    ];

    for (command, args) in invocations {
        let output = run_axiomc_json(&args);
        assert_payload_matches_schema(&validator, command, &output);

        let normalized = normalize_payload(output, &project);
        let snapshot = read_json(&contracts.join(format!("snapshots/{command}.json")));
        assert_eq!(normalized, snapshot, "{command} JSON contract drifted");
    }
}

#[test]
fn debug_map_sidecar_matches_checked_in_contract_snapshot() {
    let contracts = contract_root();
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("debug-map-contract");

    run_axiomc(&[
        "new",
        project.to_str().expect("project path"),
        "--name",
        "debug-map-contract",
    ]);
    fs::write(
        project.join("src/helper.ax"),
        "pub fn helper(): int {\nreturn 7\n}\n",
    )
    .expect("write helper source");
    fs::write(
        project.join("src/main.ax"),
        "import \"helper.ax\"\nlet answer: int = helper()\nprint answer\n",
    )
    .expect("write main source");

    let build = run_axiomc_json(&[
        "build",
        project.to_str().expect("project path"),
        "--debug",
        "--json",
    ]);
    let debug_map_path = build["debug_map"]
        .as_str()
        .expect("build payload debug_map path");
    let debug_map = read_json(Path::new(debug_map_path));
    let normalized = normalize_payload(debug_map, &project);
    let snapshot = read_json(&contracts.join("snapshots/debug-map.json"));

    assert_eq!(normalized, snapshot, "debug map sidecar drifted");
}

#[test]
fn cli_json_outputs_validate_against_public_v1_schema() {
    let schema = read_json(&public_v1_schema_path());
    let validator = jsonschema::validator_for(&schema).expect("compile public v1 schema");
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("contract-app");

    run_axiomc(&[
        "new",
        project.to_str().expect("project path"),
        "--name",
        "contract-app",
    ]);

    let project_str = project.to_str().expect("project path");
    let mutation_input = temp.path().join("mutation-survivors.json");
    fs::write(
        &mutation_input,
        r#"{"survivors":[{"id":"m1","file":"src/main.ax","function":"main","line":1,"mutator":"replace_literal","description":"changed greeting","status":"survived"}]}"#,
    )
    .expect("write mutation input");
    let mutation_input_str = mutation_input.to_str().expect("mutation input path");
    let invocations: [(&str, Vec<&str>); 8] = [
        ("check", vec!["check", project_str, "--json"]),
        ("build", vec!["build", project_str, "--json"]),
        ("test", vec!["test", project_str, "--json"]),
        ("caps", vec!["caps", project_str, "--json"]),
        ("parse", vec!["parse", project_str, "--json"]),
        ("fmt", vec!["fmt", project_str, "--check", "--json"]),
        ("run", vec!["run", project_str, "--json"]),
        (
            "mutation-report",
            vec!["mutation-report", mutation_input_str, "--json"],
        ),
    ];

    for (label, args) in invocations {
        let output = run_axiomc_json(&args);
        assert_payload_matches_schema(&validator, label, &output);
        assert_eq!(
            output["schema_version"], "axiom.stage1.v1",
            "{label} did not declare axiom.stage1.v1"
        );
        assert!(
            output.get("ok").is_some(),
            "{label} payload missing required `ok` field"
        );
        assert_eq!(
            output["command"]
                .as_str()
                .map(|s| s.split(' ').next().unwrap_or(s)),
            Some(label),
            "{label} payload command field drifted"
        );
    }
}

#[test]
fn inspect_graph_json_validates_against_semantic_graph_schema() {
    let schema = read_json(&semantic_graph_schema_path());
    let validator = jsonschema::validator_for(&schema).expect("compile semantic graph schema");
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("semantic-graph-app");

    run_axiomc(&[
        "new",
        project.to_str().expect("project path"),
        "--name",
        "semantic-graph-app",
    ]);

    let output = run_axiomc_json(&[
        "inspect",
        "graph",
        project.to_str().expect("project path"),
        "--json",
    ]);
    assert_payload_matches_schema(&validator, "inspect graph", &output);
}

#[test]
fn inspect_graph_json_schema_accepts_full_report_failures() {
    let schema = read_json(&semantic_graph_schema_path());
    let validator = jsonschema::validator_for(&schema).expect("compile semantic graph schema");
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("semantic-graph-invalid-lockfile-app");

    run_axiomc(&[
        "new",
        project.to_str().expect("project path"),
        "--name",
        "semantic-graph-invalid-lockfile-app",
    ]);
    fs::write(project.join("axiom.lock"), "invalid lockfile\n").expect("write invalid lockfile");

    let output = run_axiomc_json(&[
        "inspect",
        "graph",
        project.to_str().expect("project path"),
        "--json",
    ]);
    assert_eq!(output["ok"], false);
    assert_eq!(output["lockfile_status"], "invalid");
    assert_payload_matches_schema(&validator, "inspect graph", &output);
}

fn public_v1_schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/axiom.stage1.v1.schema.json")
        .canonicalize()
        .expect("public v1 schema path")
}

fn semantic_graph_schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/axiom-semantic-graph-v0.schema.json")
        .canonicalize()
        .expect("semantic graph schema path")
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
