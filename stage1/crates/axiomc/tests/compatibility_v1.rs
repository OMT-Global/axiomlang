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
    serde_json::from_str(&source)
        .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

#[test]
fn compatibility_contract_fixtures_and_report_conform_to_published_schemas() {
    let policy_schema = read_json(&stage1_path(
        "schemas/axiom-compatibility-policy-v1.schema.json",
    ));
    let policy_validator =
        jsonschema::validator_for(&policy_schema).expect("compile compatibility policy schema");
    let policy = read_json(&stage1_path("compatibility/policy-v1.json"));
    policy_validator
        .validate(&policy)
        .expect("checked compatibility policy must satisfy its published schema");
    for invalid in [
        {
            let mut value = policy.clone();
            value["unexpected"] = serde_json::json!(true);
            value
        },
        {
            let mut value = policy.clone();
            value["compiler_support"]["unexpected"] = serde_json::json!(true);
            value
        },
        {
            let mut value = policy.clone();
            value["evolution"]["language"]["identity"] = serde_json::json!("");
            value
        },
        {
            let mut value = policy.clone();
            value["policy_version"] = serde_json::json!("01.0.0");
            value
        },
        {
            let mut value = policy.clone();
            value["editions"]["lifecycle"] =
                serde_json::json!(["experimental", "supported", "deprecated"]);
            value
        },
        {
            let mut value = policy.clone();
            value["editions"]["lifecycle"] =
                serde_json::json!(["supported", "experimental", "deprecated", "removed"]);
            value
        },
        {
            let mut value = policy.clone();
            let duplicate = value["support_matrix"][0].clone();
            value["support_matrix"]
                .as_array_mut()
                .unwrap()
                .push(duplicate);
            value
        },
        {
            let mut value = policy.clone();
            value["support_matrix"][0]["compiler"] = serde_json::json!("01.0.0");
            value
        },
    ] {
        assert!(
            !policy_validator.is_valid(&invalid),
            "policy schema must reject schema/runtime parity mutation: {invalid}"
        );
    }

    let contract_schema = read_json(&stage1_path("schemas/axiom-public-contract-v1.schema.json"));
    let contract_validator =
        jsonschema::validator_for(&contract_schema).expect("compile public contract schema");
    let old = stage1_path("compatibility/fixtures/migration-plan-scenario/old.json");
    let current = stage1_path("compatibility/fixtures/migration-plan-scenario/new.json");
    for fixture in [&old, &current] {
        let payload = read_json(fixture);
        if let Err(error) = contract_validator.validate(&payload) {
            panic!(
                "{} must satisfy public contract schema: {error}",
                fixture.display()
            );
        }
    }

    let checker = repo_path("scripts/ci/check-compatibility-v1.py");
    let output = Command::new("python3")
        .arg(&checker)
        .args([
            "--old",
            old.to_str().unwrap(),
            "--new",
            current.to_str().unwrap(),
            "--policy",
            stage1_path("compatibility/fixtures/migration-plan-scenario/policy.json")
                .to_str()
                .unwrap(),
            "--json",
        ])
        .current_dir(repo_path(""))
        .output()
        .expect("run compatibility checker");
    assert!(
        output.status.success(),
        "compatibility checker failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("parse compatibility report");
    assert_eq!(
        report["policies"],
        serde_json::json!({
            "old": "1.0.0",
            "new": "1.0.0",
            "severity": "compatible",
            "migration": null
        })
    );
    let report_schema = read_json(&stage1_path(
        "schemas/axiom-compatibility-report-v1.schema.json",
    ));
    let report_validator =
        jsonschema::validator_for(&report_schema).expect("compile compatibility report schema");
    if let Err(error) = report_validator.validate(&report) {
        panic!("compatibility report must satisfy its published schema: {error}");
    }
    for invalid_semver in ["01.0.0", "1.01.0", "1.0.01"] {
        let mut invalid_report = report.clone();
        invalid_report["contracts"]["new"] = serde_json::json!(invalid_semver);
        assert!(
            !report_validator.is_valid(&invalid_report),
            "report schema must reject noncanonical SemVer {invalid_semver}"
        );
    }
    let temp = tempfile::tempdir().expect("create invalid-contract tempdir");
    let mut invalid_contract = read_json(&current);
    invalid_contract["surfaces"]
        .as_array_mut()
        .expect("contract surfaces")
        .retain(|surface| surface["kind"] != "artifact");
    let invalid_path = temp.path().join("missing-artifact.json");
    std::fs::write(
        &invalid_path,
        serde_json::to_vec_pretty(&invalid_contract).expect("encode invalid contract"),
    )
    .expect("write invalid contract");
    let failure = Command::new("python3")
        .arg(&checker)
        .args([
            "--old",
            old.to_str().unwrap(),
            "--new",
            invalid_path.to_str().unwrap(),
            "--policy",
            stage1_path("compatibility/fixtures/migration-plan-scenario/policy.json")
                .to_str()
                .unwrap(),
            "--json",
        ])
        .current_dir(repo_path(""))
        .output()
        .expect("run failing compatibility checker");
    assert!(!failure.status.success(), "invalid contract must fail");
    let failure: Value =
        serde_json::from_slice(&failure.stdout).expect("parse compatibility failure");
    report_validator
        .validate(&failure)
        .expect("compatibility failure must satisfy the published report schema");
    let deprecated = report["changes"]
        .as_array()
        .expect("compatibility changes")
        .iter()
        .find(|change| change["severity"] == "deprecated")
        .expect("deprecated compatibility change");
    assert_eq!(
        deprecated["replacement"], "axiom://stdlib/text/split-lines",
        "compatibility reports must preserve structured replacement IDs for migration consumers"
    );

    let mut missing_replacement = report.clone();
    let deprecated = missing_replacement["changes"]
        .as_array_mut()
        .expect("compatibility changes")
        .iter_mut()
        .find(|change| change["severity"] == "deprecated")
        .expect("deprecated compatibility change");
    deprecated
        .as_object_mut()
        .expect("compatibility change object")
        .remove("replacement");
    assert!(
        !report_validator.is_valid(&missing_replacement),
        "deprecated report changes cannot omit their replacement"
    );

    let mut unexpected_edition_replacement = report.clone();
    unexpected_edition_replacement["edition"]["replacement"] = serde_json::json!("2028");
    assert!(
        !report_validator.is_valid(&unexpected_edition_replacement),
        "non-deprecated edition changes cannot declare a replacement"
    );

    let mut unexpected_additive_replacement = report.clone();
    let additive = unexpected_additive_replacement["changes"]
        .as_array_mut()
        .expect("compatibility changes")
        .iter_mut()
        .find(|change| change["severity"] == "additive")
        .expect("additive compatibility change");
    additive["replacement"] = serde_json::json!("axiom://language/while");
    assert!(
        !report_validator.is_valid(&unexpected_additive_replacement),
        "non-actionable report changes cannot declare a replacement"
    );

    let mut breaking_without_migration = report.clone();
    let breaking = breaking_without_migration["changes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|change| change["severity"] == "breaking")
        .unwrap();
    breaking["migration"] = Value::Null;
    assert!(
        !report_validator.is_valid(&breaking_without_migration),
        "breaking report changes require migration"
    );

    let mut deprecated_without_migration = report.clone();
    let deprecated = deprecated_without_migration["changes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|change| change["severity"] == "deprecated")
        .unwrap();
    deprecated["migration"] = Value::Null;
    assert!(
        !report_validator.is_valid(&deprecated_without_migration),
        "deprecated report changes require migration"
    );

    let mut added_as_breaking = report.clone();
    let added = added_as_breaking["changes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|change| change["change"] == "added")
        .unwrap();
    added["severity"] = serde_json::json!("breaking");
    added["migration"] = serde_json::json!("Incorrect.");
    assert!(
        !report_validator.is_valid(&added_as_breaking),
        "added report changes must be additive"
    );

    let mut removed_without_replacement = report.clone();
    let removed = removed_without_replacement["changes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|change| change["severity"] == "breaking")
        .unwrap();
    removed["change"] = serde_json::json!("removed");
    removed.as_object_mut().unwrap().remove("new_version");
    removed.as_object_mut().unwrap().remove("replacement");
    assert!(
        !report_validator.is_valid(&removed_without_replacement),
        "removed report changes require a replacement"
    );

    let mut compatible_edition_migration = report.clone();
    compatible_edition_migration["edition"]["migration"] = serde_json::json!("Unnecessary.");
    assert!(
        !report_validator.is_valid(&compatible_edition_migration),
        "compatible edition state cannot declare migration"
    );

    let mut duplicate_compiler = report;
    duplicate_compiler["changes"][0]["surface_kind"] = serde_json::json!("compiler");
    assert!(
        !report_validator.is_valid(&duplicate_compiler),
        "compiler drift has one authoritative top-level representation"
    );
}

#[test]
fn public_contract_schema_rejects_unknown_surface_kind() {
    let schema = read_json(&stage1_path("schemas/axiom-public-contract-v1.schema.json"));
    let validator = jsonschema::validator_for(&schema).expect("compile public contract schema");
    let contract = read_json(&stage1_path(
        "compatibility/fixtures/migration-plan-scenario/old.json",
    ));
    let mut unknown_kind = contract.clone();
    unknown_kind["surfaces"][0]["kind"] = serde_json::json!("rust_enum");
    assert!(validator.validate(&unknown_kind).is_err());
    for invalid_semver in ["01.0.0", "1.01.0", "1.0.01"] {
        let mut invalid_contract = contract.clone();
        invalid_contract["contract_version"] = serde_json::json!(invalid_semver);
        assert!(
            !validator.is_valid(&invalid_contract),
            "contract schema must reject noncanonical SemVer {invalid_semver}"
        );
    }

    let mut deprecated_edition = read_json(&stage1_path(
        "compatibility/fixtures/migration-plan-scenario/new.json",
    ));
    deprecated_edition["edition"]["status"] = serde_json::json!("deprecated");
    assert!(
        !validator.is_valid(&deprecated_edition),
        "deprecated editions require a structured replacement edition"
    );

    let mut unexpected_edition_replacement = read_json(&stage1_path(
        "compatibility/fixtures/migration-plan-scenario/new.json",
    ));
    unexpected_edition_replacement["edition"]["replacement"] = serde_json::json!("2028");
    assert!(
        !validator.is_valid(&unexpected_edition_replacement),
        "non-deprecated editions cannot declare a replacement"
    );

    let mut unexpected_surface_replacement = read_json(&stage1_path(
        "compatibility/fixtures/migration-plan-scenario/new.json",
    ));
    unexpected_surface_replacement["surfaces"][0]["replacement"] =
        serde_json::json!("axiom://language/while");
    assert!(
        !validator.is_valid(&unexpected_surface_replacement),
        "non-deprecated surfaces cannot declare a replacement"
    );
}
