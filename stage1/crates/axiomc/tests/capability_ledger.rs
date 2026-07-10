use serde_json::Value;
use std::path::{Path, PathBuf};

fn stage1_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn read_json(relative: &str) -> Value {
    let path = stage1_path(relative);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&source)
        .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

#[test]
fn checked_capability_ledger_conforms_to_published_schema() {
    let schema = read_json("schemas/axiom-capability-ledger-v1.schema.json");
    let ledger = read_json("compiler-contracts/snapshots/capability-ledger.json");
    let validator = jsonschema::validator_for(&schema).expect("compile capability ledger schema");

    if let Err(error) = validator.validate(&ledger) {
        panic!("checked capability ledger must satisfy its published schema: {error}");
    }
}

#[test]
fn capability_ledger_schema_rejects_rows_without_evidence_tiers() {
    let schema = read_json("schemas/axiom-capability-ledger-v1.schema.json");
    let mut ledger = read_json("compiler-contracts/snapshots/capability-ledger.json");
    ledger["commands"][0]
        .as_object_mut()
        .expect("command row")
        .remove("evidenceTier");
    let validator = jsonschema::validator_for(&schema).expect("compile capability ledger schema");

    assert!(validator.validate(&ledger).is_err());
}

#[test]
fn capability_ledger_validation_detects_schema_instance_drift() {
    let mut schema = read_json("schemas/axiom-capability-ledger-v1.schema.json");
    let ledger = read_json("compiler-contracts/snapshots/capability-ledger.json");
    schema["required"]
        .as_array_mut()
        .expect("schema required fields")
        .push(serde_json::json!("impossible"));
    schema["properties"]["impossible"] = serde_json::json!({"const": true});
    let validator = jsonschema::validator_for(&schema).expect("compile modified ledger schema");

    assert!(validator.validate(&ledger).is_err());
}
