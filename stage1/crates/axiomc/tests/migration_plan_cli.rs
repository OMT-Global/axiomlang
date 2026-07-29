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

fn run_report_value(report: &Value) -> Output {
    let directory = tempfile::tempdir().expect("create report tempdir");
    let path = directory.path().join("report.json");
    fs::write(
        &path,
        serde_json::to_vec_pretty(report).expect("encode report"),
    )
    .expect("write report");
    Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(["migrate", "--report"])
        .arg(&path)
        .arg("--json")
        .current_dir(repo_root())
        .output()
        .expect("run axiomc migrate for generated report")
}

fn read_json(relative: &str) -> Value {
    let path = repo_path(relative);
    serde_json::from_str(
        &fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display())),
    )
    .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

fn neutral_report() -> Value {
    let mut report = read_json(SUCCESS_REPORT);
    report["policies"] = serde_json::json!({
        "old": "1.0.0",
        "new": "1.0.0",
        "severity": "compatible",
        "migration": null
    });
    report["contracts"] = serde_json::json!({"old": "1.0.0", "new": "1.0.0"});
    report["compiler"] = serde_json::json!({
        "old": {"current": "0.2.0", "minimum": "0.1.0", "maximum": "0.3.0"},
        "new": {"current": "0.2.0", "minimum": "0.1.0", "maximum": "0.3.0"},
        "severity": "compatible",
        "migration": null
    });
    report["edition"] = serde_json::json!({
        "old": "2026",
        "new": "2026",
        "severity": "compatible",
        "migration": null
    });
    report["summary"] = serde_json::json!({
        "breaking": 0,
        "additive": 0,
        "deprecated": 0,
        "compatible": 0
    });
    report["changes"] = serde_json::json!([]);
    report
}

fn output_message(output: &Output) -> String {
    let payload: Value =
        serde_json::from_slice(&output.stdout).expect("failure must be structured JSON");
    payload["error"]["message"]
        .as_str()
        .expect("failure must include an error message")
        .to_owned()
}

#[test]
fn success_report_fixture_tracks_the_compatibility_checker() {
    let output = Command::new("python3")
        .arg("scripts/ci/check-compatibility-v1.py")
        .args([
            "--old",
            "stage1/compatibility/fixtures/migration-plan-scenario/old.json",
            "--new",
            "stage1/compatibility/fixtures/migration-plan-scenario/new.json",
            "--policy",
            "stage1/compatibility/fixtures/migration-plan-scenario/policy.json",
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
        ["breaking", "deprecated", "replacement"]
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
    assert!(stdout.contains("migration plan 2026 -> 2026 (3 actions; no changes applied)"));
    assert!(stdout.contains("1. breaking:axiom://cli/check:"));
    assert!(stdout.contains("3. replacement:axiom://stdlib/text/lines:"));
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
fn migrate_requires_authoritative_report_metadata_and_compiler_action() {
    let valid = read_json(SUCCESS_REPORT);
    for field in ["policies", "contracts", "compiler"] {
        let mut missing = valid.clone();
        missing
            .as_object_mut()
            .expect("report object")
            .remove(field);
        let output = run_report_value(&missing);
        assert!(
            !output.status.success(),
            "missing schema-required {field} must fail"
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("missing field"),
            "missing {field} failure must come from strict report deserialization"
        );
    }

    let mut narrowing = valid.clone();
    narrowing["contracts"] = serde_json::json!({"old": "1.0.0", "new": "2.0.0"});
    narrowing["compiler"] = serde_json::json!({
        "old": {"current": "0.2.0", "minimum": "0.1.0", "maximum": "0.2.0"},
        "new": {"current": "0.2.0", "minimum": "0.2.0", "maximum": "0.2.0"},
        "severity": "breaking",
        "migration": "Install compiler 0.2.0."
    });
    narrowing["edition"] = serde_json::json!({
        "old": "2026",
        "new": "2026",
        "severity": "compatible",
        "migration": null
    });
    narrowing["summary"] = serde_json::json!({
        "breaking": 0,
        "additive": 0,
        "deprecated": 0,
        "compatible": 0
    });
    narrowing["changes"] = serde_json::json!([]);
    let output = run_report_value(&narrowing);
    assert!(
        output.status.success(),
        "top-level compiler narrowing must create a plan: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let plan: Value = serde_json::from_slice(&output.stdout).expect("parse compiler plan");
    assert_eq!(
        plan["actions"],
        serde_json::json!([{
            "sequence": 1,
            "id": "compiler:axiom://compiler/support-range",
            "kind": "breaking",
            "severity": "breaking",
            "surface_kind": "compiler",
            "surface_id": "axiom://compiler/support-range",
            "instruction": "Install compiler 0.2.0.",
            "replacement": null
        }])
    );

    let mut contradictory = narrowing.clone();
    contradictory["compiler"]["severity"] = serde_json::json!("compatible");
    assert!(
        !run_report_value(&contradictory).status.success(),
        "narrowed compiler range cannot contradict its authoritative severity"
    );

    for new_contract in ["1.0.0", "0.9.0"] {
        let mut unbumped = narrowing.clone();
        unbumped["contracts"] = serde_json::json!({"old": "1.0.0", "new": new_contract});
        let output = run_report_value(&unbumped);
        assert!(
            !output.status.success(),
            "compiler semantic drift requires an increased contract version"
        );
    }

    let mut suppressed = valid;
    suppressed["compiler"]["migration"] = serde_json::json!("Unnecessary action.");
    assert!(
        !run_report_value(&suppressed).status.success(),
        "compatible compiler state cannot smuggle a migration action"
    );

    let mut policy_only_drift = read_json(SUCCESS_REPORT);
    policy_only_drift["changes"] = serde_json::json!([]);
    policy_only_drift["summary"] = serde_json::json!({
        "breaking": 0,
        "additive": 0,
        "deprecated": 0,
        "compatible": 0
    });
    policy_only_drift["compiler"] = serde_json::json!({
        "old": {"current": "0.1.0", "minimum": "0.1.0", "maximum": "0.1.0"},
        "new": {"current": "0.1.0", "minimum": "0.1.0", "maximum": "0.1.0"},
        "severity": "compatible",
        "migration": null
    });
    policy_only_drift["edition"] = serde_json::json!({
        "old": "2026",
        "new": "2026",
        "severity": "compatible",
        "migration": null
    });
    policy_only_drift["policies"] = serde_json::json!({
        "old": "1.0.0",
        "new": "1.1.0",
        "severity": "additive",
        "migration": null
    });
    policy_only_drift["contracts"] = serde_json::json!({"old": "1.0.0", "new": "1.0.0"});
    assert!(
        !run_report_value(&policy_only_drift).status.success(),
        "policy semantic drift requires an increased contract version"
    );

    let mut additive_with_migration = read_json(SUCCESS_REPORT);
    let additive = additive_with_migration["changes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|change| change["severity"] == "additive")
        .unwrap();
    additive["migration"] = serde_json::json!("Ignored action.");
    assert!(
        !run_report_value(&additive_with_migration).status.success(),
        "additive changes cannot smuggle ignored migration actions"
    );

    let mut duplicated = read_json(SUCCESS_REPORT);
    duplicated["summary"]["compatible"] = serde_json::json!(1);
    duplicated["changes"] = serde_json::json!([{
        "change": "modified",
        "severity": "compatible",
        "surface_kind": "compiler",
        "surface_id": "axiom://compiler/support-range",
        "old_version": "0.1.0",
        "new_version": "0.1.0",
        "description": "duplicate compiler representation",
        "migration": null
    }]);
    let output = run_report_value(&duplicated);
    assert!(
        !output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .contains("compiler drift must use the top-level compiler report object"),
        "changes[] cannot contradict the authoritative compiler object"
    );
}

#[test]
fn migrate_enforces_policy_metadata_and_strongest_contract_bump() {
    let mut unchanged = neutral_report();
    unchanged["policies"]["severity"] = serde_json::json!("breaking");
    unchanged["policies"]["migration"] = serde_json::json!("Follow the new policy.");
    let output = run_report_value(&unchanged);
    assert!(
        !output.status.success()
            && output_message(&output).contains("unchanged policy versions must have compatible"),
        "unchanged policy versions cannot claim drift"
    );

    let mut blank = neutral_report();
    blank["policies"] = serde_json::json!({
        "old": "1.0.0",
        "new": "2.0.0",
        "severity": "breaking",
        "migration": "  "
    });
    blank["contracts"] = serde_json::json!({"old": "1.0.0", "new": "2.0.0"});
    let output = run_report_value(&blank);
    assert!(
        !output.status.success() && output_message(&output).contains("must be a non-empty string"),
        "policy migration actions must contain non-whitespace text"
    );

    let mut smuggled = neutral_report();
    smuggled["policies"] = serde_json::json!({
        "old": "1.0.0",
        "new": "1.1.0",
        "severity": "additive",
        "migration": "Ignored policy action."
    });
    smuggled["contracts"] = serde_json::json!({"old": "1.0.0", "new": "1.1.0"});
    let output = run_report_value(&smuggled);
    assert!(
        !output.status.success()
            && output_message(&output)
                .contains("compatible or additive policy drift cannot declare a migration"),
        "non-actionable policy drift cannot smuggle a migration"
    );

    let mut strongest_policy = neutral_report();
    strongest_policy["policies"] = serde_json::json!({
        "old": "1.0.0",
        "new": "2.0.0",
        "severity": "breaking",
        "migration": "Review the incompatible policy transition."
    });
    strongest_policy["compiler"] = serde_json::json!({
        "old": {"current": "0.2.0", "minimum": "0.2.0", "maximum": "0.2.0"},
        "new": {"current": "0.2.0", "minimum": "0.1.0", "maximum": "0.2.0"},
        "severity": "additive",
        "migration": null
    });
    strongest_policy["contracts"] = serde_json::json!({"old": "1.0.0", "new": "1.1.0"});
    let output = run_report_value(&strongest_policy);
    assert!(
        !output.status.success()
            && output_message(&output)
                .contains("breaking drift requires a major contract version bump"),
        "breaking policy drift must dominate additive compiler drift"
    );

    let mut strongest_surface = read_json(SUCCESS_REPORT);
    strongest_surface["contracts"] = serde_json::json!({"old": "1.0.0", "new": "1.1.0"});
    let output = run_report_value(&strongest_surface);
    assert!(
        !output.status.success()
            && output_message(&output)
                .contains("breaking drift requires a major contract version bump"),
        "breaking surface drift must dominate additive and deprecated surface drift"
    );
}

#[test]
fn migrate_mirrors_severity_specific_contract_version_rules() {
    let mut stable_breaking = neutral_report();
    stable_breaking["contracts"] = serde_json::json!({"old": "1.0.0", "new": "1.1.0"});
    stable_breaking["compiler"] = serde_json::json!({
        "old": {"current": "0.2.0", "minimum": "0.1.0", "maximum": "0.3.0"},
        "new": {"current": "0.2.0", "minimum": "0.2.0", "maximum": "0.3.0"},
        "severity": "breaking",
        "migration": "Install a compiler in the narrowed support range."
    });
    let output = run_report_value(&stable_breaking);
    assert!(
        !output.status.success()
            && output_message(&output)
                .contains("breaking drift requires a major contract version bump"),
        "post-1.0 breaking drift must require a major bump"
    );

    let mut stable_additive = neutral_report();
    stable_additive["contracts"] = serde_json::json!({"old": "1.0.0", "new": "1.0.1"});
    stable_additive["compiler"] = serde_json::json!({
        "old": {"current": "0.2.0", "minimum": "0.2.0", "maximum": "0.3.0"},
        "new": {"current": "0.2.0", "minimum": "0.1.0", "maximum": "0.3.0"},
        "severity": "additive",
        "migration": null
    });
    let output = run_report_value(&stable_additive);
    assert!(
        !output.status.success()
            && output_message(&output)
                .contains("additive or deprecated drift requires at least a minor"),
        "post-1.0 additive drift must require at least a minor bump"
    );

    let mut stable_deprecated = neutral_report();
    stable_deprecated["contracts"] = serde_json::json!({"old": "1.0.0", "new": "1.0.1"});
    stable_deprecated["edition"] = serde_json::json!({
        "old": "2026",
        "new": "2026",
        "severity": "deprecated",
        "migration": "Adopt edition 2027.",
        "replacement": "2027"
    });
    let output = run_report_value(&stable_deprecated);
    assert!(
        !output.status.success()
            && output_message(&output)
                .contains("additive or deprecated drift requires at least a minor"),
        "post-1.0 deprecated drift must require at least a minor bump"
    );

    let mut compatible = neutral_report();
    compatible["contracts"] = serde_json::json!({"old": "1.0.0", "new": "1.0.1"});
    compatible["compiler"]["new"]["current"] = serde_json::json!("0.3.0");
    let output = run_report_value(&compatible);
    assert!(
        !output.status.success()
            && output_message(&output).contains("contains no migration actions"),
        "a patch bump must satisfy compatible drift before plan generation"
    );

    let mut pre_one_breaking = stable_breaking.clone();
    pre_one_breaking["contracts"] = serde_json::json!({"old": "0.2.0", "new": "0.2.1"});
    let output = run_report_value(&pre_one_breaking);
    assert!(
        !output.status.success()
            && output_message(&output).contains("pre-1.0 breaking drift requires at least a minor"),
        "pre-1.0 breaking drift cannot use only a patch bump"
    );
    pre_one_breaking["contracts"] = serde_json::json!({"old": "0.2.0", "new": "0.3.0"});
    let output = run_report_value(&pre_one_breaking);
    assert!(
        output.status.success(),
        "pre-1.0 breaking drift must accept a minor bump: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let mut pre_one_additive = stable_additive;
    pre_one_additive["contracts"] = serde_json::json!({"old": "0.2.0", "new": "0.2.1"});
    let output = run_report_value(&pre_one_additive);
    assert!(
        !output.status.success()
            && output_message(&output).contains("contains no migration actions"),
        "experimental pre-1.0 additive drift may use any higher version"
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

    let mut edition_plan = read_json(SUCCESS_PLAN);
    edition_plan["editions"]["to"] = serde_json::json!("2027");
    edition_plan["actions"] = serde_json::json!([{
        "sequence": 1,
        "id": "edition:2026->2027",
        "kind": "edition",
        "severity": "breaking",
        "surface_kind": null,
        "surface_id": null,
        "instruction": "Adopt edition 2027.",
        "replacement": null
    }]);
    assert!(
        validator.is_valid(&edition_plan),
        "canonical breaking edition action must satisfy the schema"
    );

    let mut plan = edition_plan.clone();
    plan["actions"][0]["surface_id"] = serde_json::json!("axiom://language/loop");
    assert!(
        !validator.is_valid(&plan),
        "edition actions cannot impersonate surface actions"
    );

    let mut plan = edition_plan;
    plan["actions"][0]["replacement"] = serde_json::json!("2028");
    assert!(
        !validator.is_valid(&plan),
        "breaking edition actions cannot contradict their target with a replacement"
    );
}
