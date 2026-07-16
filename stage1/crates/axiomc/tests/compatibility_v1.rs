use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn stage1_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn repo_path(relative: &str) -> PathBuf {
    stage1_path("").join("..").join(relative)
}

fn read_json(path: &Path) -> Value {
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&source).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

#[test]
fn compatibility_contract_fixtures_and_report_conform_to_published_schemas() {
    let contract_schema = read_json(&stage1_path("schemas/axiom-public-contract-v1.schema.json"));
    let contract_validator = jsonschema::validator_for(&contract_schema)
        .expect("compile public contract schema");
    let old = stage1_path("examples/compatibility_v1/old.json");
    let current = stage1_path("examples/compatibility_v1/current.json");
    for fixture in [&old, &current] {
        let payload = read_json(fixture);
        if let Err(error) = contract_validator.validate(&payload) {
            panic!("{} must satisfy public contract schema: {error}", fixture.display());
        }
    }

    let checker = repo_path("scripts/ci/check-compatibility-v1.py");
    let output = Command::new("python3")
        .arg(checker)
        .args(["--old", old.to_str().unwrap(), "--new", current.to_str().unwrap(), "--json"])
        .current_dir(repo_path(""))
        .output()
        .expect("run compatibility checker");
    assert!(
        output.status.success(),
        "compatibility checker failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("parse compatibility report");
    let report_schema = read_json(&stage1_path("schemas/axiom-compatibility-report-v1.schema.json"));
    let report_validator = jsonschema::validator_for(&report_schema)
        .expect("compile compatibility report schema");
    if let Err(error) = report_validator.validate(&report) {
        panic!("compatibility report must satisfy its published schema: {error}");
    }
}

#[test]
fn public_contract_schema_rejects_unknown_surface_kind() {
    let schema = read_json(&stage1_path("schemas/axiom-public-contract-v1.schema.json"));
    let validator = jsonschema::validator_for(&schema).expect("compile public contract schema");
    let mut contract = read_json(&stage1_path("examples/compatibility_v1/old.json"));
    contract["surfaces"][0]["kind"] = serde_json::json!("rust_enum");
    assert!(validator.validate(&contract).is_err());
}
