use axiomc::json_contract;
use jsonschema::Validator;
use serde_json::Value;
use std::fs;
use std::path::Path;

fn fixture(group: &str, name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("json-fixtures")
        .join(group)
        .join(name);
    serde_json::from_str(&fs::read_to_string(path).expect("read JSON fixture"))
        .expect("fixture is valid JSON")
}

fn schema_validator() -> Validator {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("schemas")
        .join("axiom.stage1.v1.schema.json");
    let schema: Value =
        serde_json::from_str(&fs::read_to_string(path).expect("read stage1 schema"))
            .expect("stage1 schema is valid JSON");
    jsonschema::validator_for(&schema).expect("compile stage1 JSON schema")
}

fn assert_matches_stage1_schema(validator: &Validator, payload: &Value) {
    if let Err(error) = validator.validate(payload) {
        panic!("fixture failed stage1 schema validation: {error}");
    }
}

fn assert_envelope(payload: &Value, command: &str, ok: bool) {
    assert_eq!(
        payload["schema_version"],
        json_contract::JSON_SCHEMA_VERSION
    );
    assert_eq!(payload["command"], command);
    assert_eq!(payload["ok"], ok);
}

#[test]
fn build_fixtures_cover_target_triple_and_failure_diagnostic() {
    let validator = schema_validator();
    let success = fixture("build", "success.json");
    assert_matches_stage1_schema(&validator, &success);
    assert_envelope(&success, "build", true);
    assert_eq!(success["backend"], "generated-rust");
    assert_eq!(success["locked"], false);
    assert_eq!(success["offline"], false);
    assert_eq!(success["target"], "aarch64-apple-darwin");
    assert_eq!(success["metadata"]["target"], "aarch64-apple-darwin");
    assert_eq!(success["metadata"]["debug"], false);
    assert!(success["metadata"]["lockfile"].is_string());
    assert!(success["metadata"]["lockfile_hash"].is_string());
    assert!(success["metadata"]["source_hash"].is_string());
    assert_eq!(
        success["cache_key"]["compiler"],
        "axiomc-stage1-0.1.0-generated-rust"
    );
    assert_eq!(success["cache_key"]["debug"], false);
    assert!(success["cache_key"]["generated_rust_hash"].is_string());
    assert!(success["cache_key"]["lockfile_hash"].is_string());
    assert!(success["cache_key"]["manifest_hash"].is_string());
    assert_eq!(success["cache_key"]["sources"][0]["path"], success["entry"]);
    assert!(success["cache_key"]["sources"][0]["source_hash"].is_string());
    assert_eq!(success["cache_key"]["target"], "aarch64-apple-darwin");
    assert_eq!(success["cache_key"]["version"], 1);
    assert!(success["duration_ms"].is_u64());
    assert!(success["cache_hits"].is_u64());
    assert!(success["cache_misses"].is_u64());
    assert_eq!(success["packages"][0]["backend"], "generated-rust");
    assert!(success["packages"][0]["target"].is_string());
    assert_eq!(success["packages"][0]["metadata"], success["metadata"]);
    assert_eq!(success["packages"][0]["cache_key"], success["cache_key"]);

    let failure = fixture("build", "failure.json");
    assert_matches_stage1_schema(&validator, &failure);
    assert_envelope(&failure, "build", false);
    assert_eq!(failure["error"]["kind"], "build");
    assert!(failure["error"]["message"].is_string());
}

#[test]
fn test_fixtures_cover_filter_durations_and_failed_cases() {
    let validator = schema_validator();
    let filtered = fixture("test", "filter-success.json");
    assert_matches_stage1_schema(&validator, &filtered);
    assert_envelope(&filtered, "test", true);
    assert_eq!(filtered["filter"], "math");
    assert_eq!(filtered["passed"], 1);
    assert_eq!(filtered["failed"], 0);
    assert_eq!(filtered["kinds"]["unit"], 1);
    assert!(filtered["duration_ms"].is_u64());
    assert!(filtered["cases"][0]["duration_ms"].is_u64());

    let failure = fixture("test", "failure.json");
    assert_matches_stage1_schema(&validator, &failure);
    assert_envelope(&failure, "test", false);
    assert_eq!(failure["passed"], 0);
    assert_eq!(failure["failed"], 1);
    assert_eq!(failure["kinds"]["unit"], 1);
    assert_eq!(failure["cases"][0]["ok"], false);
    assert_eq!(failure["cases"][0]["error"]["kind"], "test");
}

#[test]
fn caps_fixture_covers_unsafe_capability_state() {
    let validator = schema_validator();
    let payload = fixture("caps", "unsafe-env.json");
    assert_matches_stage1_schema(&validator, &payload);
    assert_envelope(&payload, "caps", true);

    let env = payload["capabilities"]
        .as_array()
        .expect("capabilities array")
        .iter()
        .find(|capability| capability["name"] == "env")
        .expect("env capability fixture");
    assert_eq!(env["enabled"], true);
    assert_eq!(env["unsafe_unrestricted"], true);
}
