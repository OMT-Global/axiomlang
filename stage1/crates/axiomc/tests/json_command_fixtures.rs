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
fn build_fixtures_cover_direct_native_target_and_no_fallback_failure() {
    let validator = schema_validator();
    let success = fixture("build", "success.json");
    assert_matches_stage1_schema(&validator, &success);
    assert_envelope(&success, "build", true);
    assert_eq!(success["backend"], "cranelift");
    assert!(success["generated_rust"].is_null());
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
        "axiomc-stage1-0.1.0-cranelift"
    );
    assert_eq!(success["cache_key"]["debug"], false);
    assert!(success["cache_key"]["backend_input_hash"].is_string());
    assert!(success["cache_key"]["lockfile_hash"].is_string());
    assert!(success["cache_key"]["manifest_hash"].is_string());
    assert_eq!(success["cache_key"]["sources"][0]["path"], success["entry"]);
    assert!(success["cache_key"]["sources"][0]["source_hash"].is_string());
    assert_eq!(success["cache_key"]["target"], "aarch64-apple-darwin");
    assert_eq!(success["cache_key"]["version"], 1);
    assert!(success["duration_ms"].is_u64());
    assert!(success["cache_hits"].is_u64());
    assert!(success["cache_misses"].is_u64());
    assert_eq!(success["packages"][0]["backend"], "cranelift");
    assert!(success["packages"][0]["generated_rust"].is_null());
    assert!(success["packages"][0]["target"].is_string());
    assert_eq!(success["packages"][0]["metadata"], success["metadata"]);
    assert_eq!(success["packages"][0]["cache_key"], success["cache_key"]);

    let unsupported_target = fixture("build", "unsupported-target.json");
    assert_matches_stage1_schema(&validator, &unsupported_target);
    assert_envelope(&unsupported_target, "build", false);
    assert_eq!(unsupported_target["error"]["kind"], "build");
    assert_eq!(unsupported_target["error"]["code"], "build.failed");
    assert!(
        unsupported_target["error"]["message"]
            .as_str()
            .expect("error message")
            .contains("--backend cranelift")
    );
    assert!(
        unsupported_target["error"]["message"]
            .as_str()
            .expect("error message")
            .contains("only the host target")
    );

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

#[test]
fn run_fixtures_cover_direct_native_success_and_runtime_failure() {
    let validator = schema_validator();
    let success = fixture("run", "success.json");
    assert_matches_stage1_schema(&validator, &success);
    assert_envelope(&success, "run", true);
    assert_eq!(success["backend"], "cranelift");
    assert!(success["generated_rust"].is_null());
    assert_eq!(success["result"], "success");
    assert_eq!(success["exit_code"], 0);
    assert!(success["binary"].is_string());
    assert!(success["duration_ms"].is_u64());
    assert_eq!(success["stdout"], "hello from run\n");
    assert_eq!(success["stderr"], "");

    let failure = fixture("run", "failure.json");
    assert_matches_stage1_schema(&validator, &failure);
    assert_envelope(&failure, "run", false);
    assert_eq!(failure["backend"], "cranelift");
    assert!(failure["generated_rust"].is_null());
    assert_eq!(failure["result"], "failure");
    assert_eq!(failure["exit_code"], 1);
    assert!(
        failure["stderr"]
            .as_str()
            .expect("runtime stderr")
            .contains("\"kind\":\"panic\"")
    );
}

#[test]
fn doc_fixtures_cover_public_api_extraction_and_missing_sources() {
    let validator = schema_validator();
    let success = fixture("doc", "success.json");
    assert_matches_stage1_schema(&validator, &success);
    assert_envelope(&success, "doc", true);
    assert!(success["markdown"].is_string());
    assert!(success["html"].is_string());
    let item = &success["items"][0];
    assert_eq!(item["kind"], "function");
    assert_eq!(item["public"], true);
    assert_eq!(item["signature"], "pub fn add(left: int, right: int): int {");

    let failure = fixture("doc", "failure.json");
    assert_matches_stage1_schema(&validator, &failure);
    assert_envelope(&failure, "doc", false);
    assert_eq!(failure["error"]["kind"], "doc");
    assert!(
        failure["error"]["message"]
            .as_str()
            .expect("doc error message")
            .contains("no .ax files found")
    );
}
