use axiomc::json_contract;
use serde_json::Value;
use std::fs;
use std::path::Path;

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("json-fixtures")
        .join("check")
        .join(name);
    serde_json::from_str(&fs::read_to_string(path).expect("read check fixture"))
        .expect("check fixture is valid JSON")
}

fn assert_check_envelope(payload: &Value, ok: bool) {
    assert_eq!(
        payload["schema_version"],
        json_contract::JSON_SCHEMA_VERSION
    );
    assert_eq!(payload["command"], "check");
    assert_eq!(payload["ok"], ok);
}

#[test]
fn check_success_fixture_matches_stage1_v1_envelope() {
    let payload = fixture("success.json");

    assert_check_envelope(&payload, true);
    assert!(payload["project"].is_string());
    assert!(payload["manifest"].is_string());
    assert!(payload["entry"].is_string());
    assert!(payload["statement_count"].is_u64());
    assert!(payload["capabilities"].is_array());
    assert!(payload["warnings"].is_array());
    assert!(payload["packages"].is_array());
}

#[test]
fn check_error_fixtures_cover_required_diagnostic_classes() {
    let cases = [
        ("parse-error.json", "parse", None),
        ("type-error.json", "type", None),
        ("borrow-error.json", "ownership", Some("use_after_move")),
        ("capability-denial.json", "capability", None),
    ];

    for (name, kind, code) in cases {
        let payload = fixture(name);
        assert_check_envelope(&payload, false);
        assert_eq!(payload["error"]["kind"], kind);
        assert!(payload["error"]["message"].is_string());
        assert!(payload["error"]["path"].is_string());
        assert!(payload["error"]["line"].is_u64());
        assert!(payload["error"]["column"].is_u64());
        if let Some(code) = code {
            assert_eq!(payload["error"]["code"], code);
        }
    }
}
