use jsonschema::Validator;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
}

fn fixture(name: &str) -> PathBuf {
    repo_root()
        .join("stage1/json-fixtures/task-contract")
        .join(name)
}

fn project() -> PathBuf {
    repo_root().join("stage1/examples/agent_native_authorize")
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).expect("read JSON fixture"))
        .expect("fixture is valid JSON")
}

fn run(spec: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .current_dir(project())
        .args([
            "task-contract",
            spec.to_str().expect("UTF-8 spec path"),
            "--project",
            ".",
            "--json",
        ])
        .output()
        .expect("run task-contract")
}

fn successful_json(spec: &Path) -> (Output, Value) {
    let output = run(spec);
    assert!(
        output.status.success(),
        "task-contract failed; stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let payload = serde_json::from_slice(&output.stdout).expect("task-contract emits JSON");
    (output, payload)
}

fn schemas() -> (Value, Value) {
    let root = repo_root().join("stage1/schemas");
    (
        read_json(&root.join("axiom-agent-task-spec-v0.schema.json")),
        read_json(&root.join("axiom-agent-task-v0.schema.json")),
    )
}

fn output_validator(input_schema: &Value, output_schema: &Value) -> Validator {
    // Resolve the public output schema's local task-definition reference in
    // memory. The checked-in schema keeps the URI reference so schema-aware
    // tools can share one task definition instead of drifting copies.
    let mut resolved = output_schema.clone();
    resolved["$defs"] = input_schema["$defs"].clone();
    resolved["properties"]["contract"]["allOf"][0] = input_schema["$defs"]["taskBase"].clone();
    jsonschema::validator_for(&resolved).expect("compile resolved output schema")
}

fn assert_output_valid(validator: &Validator, payload: &Value) {
    let errors: Vec<_> = validator
        .iter_errors(payload)
        .map(|error| error.to_string())
        .collect();
    assert!(errors.is_empty(), "output schema errors: {errors:#?}");
}

#[test]
fn approved_feature_is_schema_valid_byte_deterministic_and_project_relative() {
    let spec_path = fixture("feature-approved.spec.json");
    let spec = read_json(&spec_path);
    let (input_schema, output_schema) = schemas();
    assert!(
        jsonschema::validator_for(&input_schema)
            .expect("compile input schema")
            .is_valid(&spec)
    );

    let (first, payload) = successful_json(&spec_path);
    let (second, _) = successful_json(&spec_path);
    assert_eq!(
        first.stdout, second.stdout,
        "contract JSON must be byte stable"
    );
    assert_output_valid(&output_validator(&input_schema, &output_schema), &payload);
    assert_eq!(payload["project"], ".");
    assert_eq!(
        payload["contract"]["scope"]["allowed_files"],
        spec["task"]["scope"]["allowed_files"]
    );

    let rendered = String::from_utf8(first.stdout).expect("UTF-8 JSON");
    assert!(!rendered.contains(repo_root().to_str().expect("UTF-8 repository path")));
    for path in payload["contract"]["scope"]["allowed_files"]
        .as_array()
        .expect("allowed files")
    {
        let path = path.as_str().expect("path string");
        assert!(!Path::new(path).is_absolute());
        assert!(!path.split('/').any(|part| part == ".."));
    }
}

#[test]
fn approved_repair_preserves_repair_plan_boundaries_losslessly() {
    let spec_path = fixture("repair-approved.spec.json");
    let spec = read_json(&spec_path);
    let (input_schema, output_schema) = schemas();
    let (_, payload) = successful_json(&spec_path);

    assert_output_valid(&output_validator(&input_schema, &output_schema), &payload);
    assert_eq!(payload["contract"]["repair"], spec["task"]["repair"]);
    assert_eq!(
        payload["contract"]["repair"]["allowed_files"],
        payload["contract"]["scope"]["allowed_files"]
    );
    let repair_evidence = payload["contract"]["repair"]["required_evidence"]
        .as_array()
        .expect("repair evidence");
    let contract_evidence = payload["contract"]["required_evidence"]
        .as_array()
        .expect("contract evidence");
    for kind in repair_evidence {
        assert!(contract_evidence.iter().any(|entry| entry["kind"] == *kind));
    }
}

#[test]
fn schema_matches_runtime_optional_defaults_and_typed_repair_diagnostics() {
    let mut spec = read_json(&fixture("repair-approved.spec.json"));
    spec["task"]["scope"]["denied_files"] = serde_json::json!([]);
    spec["task"]["repair"]["diagnostics"] = serde_json::json!([{
        "kind": "capability",
        "message": "capability is denied",
        "repair": {"action": "remove_capability_use"}
    }]);

    let (input_schema, output_schema) = schemas();
    let validator = jsonschema::validator_for(&input_schema).expect("compile input schema");
    assert!(validator.is_valid(&spec));

    let temp = tempfile::tempdir().expect("temp directory");
    let path = temp.path().join("defaults-and-typed-diagnostic.json");
    fs::write(&path, serde_json::to_vec_pretty(&spec).unwrap()).expect("write task spec");
    let (_, payload) = successful_json(&path);
    assert_output_valid(&output_validator(&input_schema, &output_schema), &payload);
    assert!(payload["contract"]["scope"].get("denied_files").is_none());
    assert_eq!(
        payload["contract"]["repair"]["diagnostics"],
        serde_json::json!([{
            "kind": "capability",
            "message": "capability is denied",
            "repair": {"action": "remove_capability_use"}
        }])
    );
}

#[test]
fn invalid_authority_scope_conflicts_actions_budgets_dependencies_and_delivery_fail_closed() {
    let baseline = read_json(&fixture("feature-approved.spec.json"));
    let cases: Vec<(&str, Box<dyn Fn(&mut Value)>)> = vec![
        (
            "ambiguous authority",
            Box::new(|v| v["task"]["authority"]["issue"] = 1420.into()),
        ),
        (
            "unapproved authority",
            Box::new(|v| v["task"]["authority"]["approval"]["state"] = "pending".into()),
        ),
        (
            "missing scope",
            Box::new(|v| {
                v["task"]
                    .as_object_mut()
                    .unwrap()
                    .remove("scope")
                    .map(drop)
                    .unwrap()
            }),
        ),
        (
            "conflicting commands",
            Box::new(|v| {
                let required = v["task"]["commands"]["required"].clone();
                v["task"]["commands"]["forbidden"] = required;
            }),
        ),
        (
            "conflicting capabilities",
            Box::new(|v| {
                let required = v["task"]["capabilities"]["required"].clone();
                v["task"]["capabilities"]["forbidden"] = required;
            }),
        ),
        (
            "conflicting acceptance",
            Box::new(|v| {
                let mut opposite = v["task"]["acceptance_criteria"][0].clone();
                opposite["expected"] = "fail".into();
                v["task"]["acceptance_criteria"]
                    .as_array_mut()
                    .unwrap()
                    .push(opposite);
            }),
        ),
        (
            "irreversible action",
            Box::new(|v| v["task"]["delivery_permissions"]["irreversible_actions"] = true.into()),
        ),
        (
            "unknown terminal condition field",
            Box::new(|v| v["task"]["terminal_conditions"]["unexpected"] = true.into()),
        ),
        (
            "scope widening",
            Box::new(|v| {
                v["task"]["scope"]["allowed_files"] = serde_json::json!(["../outside.ax"])
            }),
        ),
        (
            "invalid budget",
            Box::new(|v| v["task"]["budgets"]["time_seconds"] = 0.into()),
        ),
        (
            "dangling dependency",
            Box::new(
                |v| v["task"]["dependencies"] = serde_json::json!([{"id":"a","status":"satisfied","depends_on":["missing"],"precondition":"ready"}]),
            ),
        ),
        (
            "dependency cycle",
            Box::new(|v| {
                v["task"]["dependencies"] = serde_json::json!([
                    {"id":"a","status":"satisfied","depends_on":["b"],"precondition":"b ready"},
                    {"id":"b","status":"satisfied","depends_on":["a"],"precondition":"a ready"}
                ])
            }),
        ),
        (
            "delivery above class",
            Box::new(|v| {
                v["task"]["autonomy"]["class"] = 1.into();
                v["task"]["delivery_permissions"]["push"] = true.into();
            }),
        ),
    ];

    let temp = tempfile::tempdir().expect("temp directory");
    for (name, mutate) in cases {
        let mut invalid = baseline.clone();
        mutate(&mut invalid);
        let path = temp.path().join(format!("{}.json", name.replace(' ', "-")));
        fs::write(&path, serde_json::to_vec_pretty(&invalid).unwrap()).expect("write invalid spec");
        let output = run(&path);
        assert!(!output.status.success(), "{name} must fail closed");
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("\"contract\""),
            "{name} emitted an executable contract"
        );
    }
}
