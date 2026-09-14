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

fn command_schema_validator() -> Validator {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("compiler-contracts")
        .join("schemas")
        .join("axiom.stage1.command.schema.json");
    let schema: Value =
        serde_json::from_str(&fs::read_to_string(path).expect("read command schema"))
            .expect("command schema is valid JSON");
    jsonschema::validator_for(&schema).expect("compile command JSON schema")
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
fn fmt_fixture_covers_replayable_byte_edits() {
    let stage1_validator = schema_validator();
    let payload = fixture("fmt", "changes.json");
    assert_matches_stage1_schema(&stage1_validator, &payload);
    assert_envelope(&payload, "fmt", false);

    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("schemas")
        .join("axiom-format-edit-v1.schema.json");
    let schema: Value =
        serde_json::from_str(&fs::read_to_string(schema_path).expect("read formatter edit schema"))
            .expect("formatter edit schema is valid JSON");
    jsonschema::validator_for(&schema)
        .expect("compile formatter edit schema")
        .validate(&payload)
        .expect("fmt fixture matches formatter edit schema");

    let edits = payload["files"][0]["edits"]
        .as_array()
        .expect("formatter fixture edits");
    assert_eq!(edits[0]["start_byte"], 11);
    assert_eq!(edits[0]["end_byte"], 14);
    assert_eq!(edits[1]["start_byte"], edits[1]["end_byte"]);
    assert_eq!(edits[1]["replacement"], "\n");
}

#[test]
fn build_fixtures_cover_direct_native_target_and_no_fallback_failure() {
    let validator = schema_validator();
    let command_validator = command_schema_validator();
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
    assert_eq!(success["cache_key"]["version"], 2);
    assert_eq!(
        success["lowering"]["execution_mode"],
        "direct_native_runtime"
    );
    assert_eq!(
        success["lowering"]["lowering_mode"],
        "direct_native_runtime_with_static_folds"
    );
    assert_eq!(success["lowering"]["known_value_static_folds"], true);
    assert_eq!(success["lowering"]["legacy_fallback_attempted"], false);
    assert!(success["duration_ms"].is_u64());
    assert!(success["cache_hits"].is_u64());
    assert!(success["cache_misses"].is_u64());
    assert_eq!(success["packages"][0]["backend"], "cranelift");
    assert!(success["packages"][0]["generated_rust"].is_null());
    assert!(success["packages"][0]["target"].is_string());
    assert_eq!(success["packages"][0]["metadata"], success["metadata"]);
    assert_eq!(success["packages"][0]["cache_key"], success["cache_key"]);
    assert_eq!(success["packages"][0]["lowering"], success["lowering"]);

    let blocked = fixture("build", "runtime-lowering-required.json");
    assert_matches_stage1_schema(&validator, &blocked);
    assert_matches_stage1_schema(&command_validator, &blocked);
    assert_envelope(&blocked, "build", false);
    assert_eq!(
        blocked["error"]["code"],
        "backend.runtime_lowering_required"
    );
    assert_eq!(
        blocked["lowering"]["lowering_mode"],
        "runtime_lowering_required"
    );
    assert_eq!(blocked["lowering"]["direct_native_runtime"], false);
    assert_eq!(blocked["lowering"]["legacy_fallback_attempted"], true);

    let lowering_schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("schemas")
        .join("axiom-build-lowering-evidence-v1.schema.json");
    let lowering_schema: Value = serde_json::from_str(
        &fs::read_to_string(lowering_schema_path).expect("read lowering evidence schema"),
    )
    .expect("lowering evidence schema is valid JSON");
    let lowering_validator =
        jsonschema::validator_for(&lowering_schema).expect("compile lowering evidence schema");
    lowering_validator
        .validate(&success["lowering"])
        .expect("success lowering evidence matches schema");
    lowering_validator
        .validate(&blocked["lowering"])
        .expect("blocked lowering evidence matches schema");
    let exact_lowering_tuples = [
        serde_json::json!({
            "schema_version": "axiom.build-lowering-evidence.v1",
            "execution_mode": "direct_native_runtime",
            "lowering_mode": "direct_native_runtime",
            "direct_native_runtime": true,
            "known_value_static_folds": false,
            "legacy_fallback_attempted": false
        }),
        serde_json::json!({
            "schema_version": "axiom.build-lowering-evidence.v1",
            "execution_mode": "direct_native_runtime",
            "lowering_mode": "direct_native_runtime_with_static_folds",
            "direct_native_runtime": true,
            "known_value_static_folds": true,
            "legacy_fallback_attempted": false
        }),
        serde_json::json!({
            "schema_version": "axiom.build-lowering-evidence.v1",
            "execution_mode": "generated_rust_runtime",
            "lowering_mode": "generated_rust_compatibility",
            "direct_native_runtime": false,
            "known_value_static_folds": false,
            "legacy_fallback_attempted": false
        }),
        serde_json::json!({
            "schema_version": "axiom.build-lowering-evidence.v1",
            "execution_mode": "not_produced",
            "lowering_mode": "runtime_lowering_required",
            "direct_native_runtime": false,
            "known_value_static_folds": false,
            "legacy_fallback_attempted": true
        }),
        serde_json::json!({
            "schema_version": "axiom.build-lowering-evidence.v1",
            "execution_mode": "bounded_static_output",
            "lowering_mode": "bounded_static_output",
            "direct_native_runtime": false,
            "known_value_static_folds": true,
            "legacy_fallback_attempted": false
        }),
    ];
    for evidence in exact_lowering_tuples {
        let mode = evidence["lowering_mode"]
            .as_str()
            .expect("lowering mode")
            .to_string();
        lowering_validator
            .validate(&evidence)
            .unwrap_or_else(|error| panic!("{mode} exact tuple failed schema validation: {error}"));

        let mut build_payload = success.clone();
        build_payload["lowering"] = evidence.clone();
        build_payload["packages"][0]["lowering"] = evidence.clone();
        assert_matches_stage1_schema(&validator, &build_payload);

        let mut contradiction = evidence.clone();
        let legacy_fallback = contradiction["legacy_fallback_attempted"]
            .as_bool()
            .expect("legacy fallback boolean");
        contradiction["legacy_fallback_attempted"] = serde_json::json!(!legacy_fallback);
        assert!(
            lowering_validator.validate(&contradiction).is_err(),
            "{mode} must reject a contradictory lowering tuple"
        );

        build_payload["packages"][0]["lowering"] = contradiction.clone();
        assert!(
            validator.validate(&build_payload).is_err(),
            "public stage1 schema must reject nested-only contradictory {mode} evidence"
        );

        build_payload["packages"][0]["lowering"] = evidence.clone();
        build_payload["lowering"] = contradiction;
        assert!(
            validator.validate(&build_payload).is_err(),
            "public stage1 schema must reject top-level contradictory {mode} evidence"
        );
    }

    let unsupported_target = fixture("build", "unsupported-target.json");
    assert_matches_stage1_schema(&validator, &unsupported_target);
    assert_matches_stage1_schema(&command_validator, &unsupported_target);
    assert_envelope(&unsupported_target, "build", false);
    assert_eq!(unsupported_target["error"]["kind"], "target");
    assert_eq!(unsupported_target["error"]["code"], "target.unsupported");
    assert!(
        unsupported_target["error"]["message"]
            .as_str()
            .expect("error message")
            .contains("direct-native backend")
    );
    assert!(
        unsupported_target["error"]["message"]
            .as_str()
            .expect("error message")
            .contains("host target")
    );

    let failure = fixture("build", "failure.json");
    assert_matches_stage1_schema(&validator, &failure);
    assert_matches_stage1_schema(&command_validator, &failure);
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
    assert_eq!(
        item["signature"],
        "pub fn add(left: int, right: int): int {"
    );

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
