use axiomc::json_contract;
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
    let success = fixture("build", "success.json");
    assert_envelope(&success, "build", true);
    assert_eq!(success["target"], "aarch64-apple-darwin");
    assert!(success["duration_ms"].is_u64());
    assert!(success["cache_hits"].is_u64());
    assert!(success["cache_misses"].is_u64());
    assert!(success["packages"][0]["target"].is_string());

    let failure = fixture("build", "failure.json");
    assert_envelope(&failure, "build", false);
    assert_eq!(failure["error"]["kind"], "build");
    assert!(failure["error"]["message"].is_string());
}

#[test]
fn test_fixtures_cover_filter_durations_and_failed_cases() {
    let filtered = fixture("test", "filter-success.json");
    assert_envelope(&filtered, "test", true);
    assert_eq!(filtered["filter"], "math");
    assert_eq!(filtered["passed"], 1);
    assert_eq!(filtered["failed"], 0);
    assert!(filtered["duration_ms"].is_u64());
    assert!(filtered["cases"][0]["duration_ms"].is_u64());

    let failure = fixture("test", "failure.json");
    assert_envelope(&failure, "test", false);
    assert_eq!(failure["passed"], 0);
    assert_eq!(failure["failed"], 1);
    assert_eq!(failure["cases"][0]["ok"], false);
    assert_eq!(failure["cases"][0]["error"]["kind"], "test");
}

#[test]
fn caps_fixture_covers_unsafe_capability_state() {
    let payload = fixture("caps", "unsafe-env.json");
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
