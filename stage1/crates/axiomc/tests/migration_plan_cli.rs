use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SUCCESS_REPORT: &str = "stage1/json-fixtures/migration-plan/success.report.json";
const SUCCESS_PLAN: &str = "stage1/json-fixtures/migration-plan/success.plan.json";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn repo_path(relative: &str) -> PathBuf {
    repo_root().join(relative)
}

fn run_fixture(relative: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(["migrate", "--report", relative, "--json"])
        .current_dir(repo_root())
        .output()
        .unwrap_or_else(|error| panic!("run axiomc migrate for {relative}: {error}"))
}

fn run_text_fixture(relative: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(["migrate", "--report", relative])
        .current_dir(repo_root())
        .output()
        .unwrap_or_else(|error| panic!("run text axiomc migrate for {relative}: {error}"))
}

fn read_json(relative: &str) -> Value {
    let path = repo_path(relative);
    serde_json::from_str(
        &fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display())),
    )
    .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

#[test]
fn success_report_fixture_tracks_the_compatibility_checker() {
    let output = Command::new("python3")
        .arg("scripts/ci/check-compatibility-v1.py")
        .args([
            "--old",
            "stage1/examples/compatibility_v1/old.json",
            "--new",
            "stage1/examples/compatibility_v1/current.json",
            "--json",
        ])
        .current_dir(repo_root())
        .output()
        .expect("run Compatibility v1 checker");
    assert!(
        output.status.success(),
        "compatibility checker failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let generated: Value =
        serde_json::from_slice(&output.stdout).expect("parse generated compatibility report");
    assert_eq!(
        generated,
        read_json(SUCCESS_REPORT),
        "positive migration input must track the canonical Compatibility v1 checker"
    );
}

#[test]
fn migrate_emits_byte_stable_schema_valid_plan_without_side_effects() {
    let first = run_fixture(SUCCESS_REPORT);
    let second = run_fixture(SUCCESS_REPORT);
    assert!(
        first.status.success(),
        "migrate failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(
        first.stdout, second.stdout,
        "migration plans must be byte-stable"
    );

    let expected = fs::read(repo_path(SUCCESS_PLAN)).expect("read expected migration plan");
    assert_eq!(
        first.stdout, expected,
        "CLI output drifted from its positive fixture"
    );
    let plan: Value = serde_json::from_slice(&first.stdout).expect("parse migration plan");
    let schema = read_json("stage1/schemas/axiom-migration-plan-v1.schema.json");
    jsonschema::validator_for(&schema)
        .expect("compile migration plan schema")
        .validate(&plan)
        .expect("migration plan must satisfy its published schema");

    assert_eq!(
        plan["actions"]
            .as_array()
            .expect("migration actions")
            .iter()
            .map(|action| action["kind"].as_str().expect("action kind"))
            .collect::<Vec<_>>(),
        [
            "edition",
            "breaking",
            "breaking",
            "deprecated",
            "replacement"
        ]
    );
    assert_eq!(
        plan["effects"],
        serde_json::json!({
            "source_rewriting": false,
            "package_resolution": false,
            "release_publication": false,
            "policy_changes": false
        })
    );

    let help = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(["check", "--help"])
        .output()
        .expect("run axiomc check --help");
    assert!(help.status.success(), "check help must be available");
    let help = String::from_utf8(help.stdout).expect("UTF-8 check help");
    for flag in ["--json", "--properties", "--backend"] {
        assert!(
            help.contains(flag),
            "canonical migration action references unsupported flag {flag}"
        );
    }
    assert!(
        !plan["actions"][1]["instruction"]
            .as_str()
            .expect("CLI migration instruction")
            .contains("--format"),
        "canonical migration action cannot recommend the unsupported --format flag"
    );
}

#[test]
fn migrate_text_output_states_that_no_changes_were_applied() {
    let output = run_text_fixture(SUCCESS_REPORT);
    assert!(
        output.status.success(),
        "text migrate failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 text migration plan");
    assert!(stdout.contains("migration plan 2026 -> 2027 (5 actions; no changes applied)"));
    assert!(stdout.contains("1. edition:2026->2027:"));
    assert!(stdout.contains("5. replacement:axiom://stdlib/text/lines:"));
}

#[test]
fn migrate_carries_deprecated_edition_replacement() {
    let fixture = "stage1/json-fixtures/migration-plan/deprecated-edition.report.json";
    let report = read_json(fixture);
    let report_schema = read_json("stage1/schemas/axiom-compatibility-report-v1.schema.json");
    assert!(
        jsonschema::validator_for(&report_schema)
            .expect("compile compatibility report schema")
            .is_valid(&report),
        "deprecated edition fixture must satisfy the input schema"
    );
    let output = run_fixture(fixture);
    assert!(
        output.status.success(),
        "deprecated edition plan failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let plan: Value = serde_json::from_slice(&output.stdout).expect("parse migration plan");
    assert_eq!(
        plan["editions"],
        serde_json::json!({"from": "2026", "to": "2026"})
    );
    assert_eq!(
        plan["actions"],
        serde_json::json!([{
            "sequence": 1,
            "id": "edition:2026->2026",
            "kind": "edition",
            "severity": "deprecated",
            "surface_kind": null,
            "surface_id": null,
            "instruction": "Adopt edition 2027 before support for edition 2026 ends.",
            "replacement": "2027"
        }])
    );
    let schema = read_json("stage1/schemas/axiom-migration-plan-v1.schema.json");
    assert!(
        jsonschema::validator_for(&schema)
            .expect("compile migration plan schema")
            .is_valid(&plan),
        "deprecated edition plan must satisfy its schema"
    );
}

#[test]
fn migrate_rejects_malformed_failed_and_incomplete_reports() {
    for (fixture, expected) in [
        (
            "stage1/json-fixtures/migration-plan/malformed.report.json",
            "unknown field `unexpected`",
        ),
        (
            "stage1/json-fixtures/migration-plan/failed.report.json",
            "must be successful (ok=true)",
        ),
        (
            "stage1/json-fixtures/migration-plan/missing-action.report.json",
            "edition migration action is required",
        ),
        (
            "stage1/json-fixtures/migration-plan/missing-replacement.report.json",
            "replacement is required",
        ),
        (
            "stage1/json-fixtures/migration-plan/no-actions.report.json",
            "contains no migration actions",
        ),
        (
            "stage1/json-fixtures/migration-plan/null-version.report.json",
            "invalid compatibility report",
        ),
        (
            "stage1/json-fixtures/migration-plan/unexpected-edition-replacement.report.json",
            "edition replacement is only valid for deprecated editions",
        ),
    ] {
        let output = run_fixture(fixture);
        assert!(
            !output.status.success(),
            "{fixture} must fail closed, stdout={}",
            String::from_utf8_lossy(&output.stdout)
        );
        let payload: Value =
            serde_json::from_slice(&output.stdout).expect("failure must be structured JSON");
        assert_eq!(payload["ok"], false);
        assert_eq!(payload["command"], "migrate");
        assert!(
            payload["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains(expected)),
            "{fixture} did not report {expected:?}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[test]
fn migration_plan_schema_enforces_plan_only_boundaries() {
    let schema = read_json("stage1/schemas/axiom-migration-plan-v1.schema.json");
    assert_eq!(
        schema["$id"],
        "https://axiom.omt.global/schemas/axiom-migration-plan-v1.schema.json"
    );
    assert_eq!(
        schema["properties"]["schema_version"]["const"],
        "axiom.migration_plan.v1"
    );
    assert_eq!(schema["additionalProperties"], false);
    let validator = jsonschema::validator_for(&schema).expect("compile migration plan schema");
    let mut plan = read_json(SUCCESS_PLAN);
    assert!(validator.is_valid(&plan));

    plan["effects"]["source_rewriting"] = serde_json::json!(true);
    assert!(
        !validator.is_valid(&plan),
        "plan-only output cannot claim source rewriting"
    );

    let mut plan = read_json(SUCCESS_PLAN);
    plan["actions"] = serde_json::json!([]);
    assert!(
        !validator.is_valid(&plan),
        "a successful migration plan cannot omit actions"
    );

    let mut plan = read_json(SUCCESS_PLAN);
    plan["actions"][0]["surface_id"] = serde_json::json!("axiom://language/loop");
    assert!(
        !validator.is_valid(&plan),
        "edition actions cannot impersonate surface actions"
    );

    let mut plan = read_json(SUCCESS_PLAN);
    plan["actions"][0]["replacement"] = serde_json::json!("2028");
    assert!(
        !validator.is_valid(&plan),
        "breaking edition actions cannot contradict their target with a replacement"
    );
}
