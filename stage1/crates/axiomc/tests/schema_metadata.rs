use axiomc::{
    json_contract,
    manifest::{
        DEPENDENCY_VERSION_PATTERN, KNOWN_CAPABILITIES, PER_TEST_CAPABILITIES_SUPPORTED,
        TEST_KIND_NAMES,
    },
};
use jsonschema::Validator;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;

fn schema_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("schemas")
}

fn compile_validator(schema: &Value) -> Validator {
    jsonschema::validator_for(schema).expect("compile JSON schema")
}

#[test]
fn filesystem_v1_schema_enforces_promotion_boundaries() {
    let stage1 = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let schema: Value = serde_json::from_str(
        &fs::read_to_string(
            stage1.join("compiler-contracts/schemas/axiom.filesystem.v1.schema.json"),
        )
        .expect("read Filesystem v1 schema"),
    )
    .expect("Filesystem v1 schema is valid JSON");
    let snapshot: Value = serde_json::from_str(
        &fs::read_to_string(stage1.join("compiler-contracts/snapshots/filesystem-v1.json"))
            .expect("read Filesystem v1 snapshot"),
    )
    .expect("Filesystem v1 snapshot is valid JSON");
    let validator = compile_validator(&schema);

    validator
        .validate(&snapshot)
        .expect("checked Filesystem v1 snapshot matches its schema");

    let mut incomplete_promotion = snapshot.clone();
    incomplete_promotion["implementation"]["tier"] = serde_json::json!("runtime_complete");
    assert!(
        !validator.is_valid(&incomplete_promotion),
        "runtime_complete requires complete executable evidence"
    );

    let mut complete_promotion = snapshot;
    complete_promotion["implementation"]["tier"] = serde_json::json!("runtime_complete");
    complete_promotion["implementation"]["status"] = serde_json::json!("qualified");
    complete_promotion["implementation"]["blockers"] = serde_json::json!([]);
    for field in [
        "scoped_text_io",
        "root_scoped_metadata",
        "root_scoped_write",
        "typed_paths",
        "binary_handles",
        "deterministic_traversal",
        "atomic_replace",
        "secure_temporary_resources",
        "runtime_effects_only",
        "descriptor_anchored_replace",
        "pathname_operations_toctou_safe",
    ] {
        complete_promotion["implementation"][field] = serde_json::json!(true);
    }
    for fixture in complete_promotion["fixtures"]
        .as_array_mut()
        .expect("Filesystem v1 fixtures are an array")
    {
        fixture["evidence"] = serde_json::json!("runtime");
    }
    validator
        .validate(&complete_promotion)
        .expect("fully evidenced runtime_complete contract is promotion-capable");
}

#[test]
fn quality_v1_schemas_reject_contradictory_reports() {
    let policy_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-quality-policy-v1.schema.json"))
            .expect("read quality policy schema"),
    )
    .expect("quality policy schema is valid JSON");
    let report_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-quality-report-v1.schema.json"))
            .expect("read quality report schema"),
    )
    .expect("quality report schema is valid JSON");
    let policy_validator = compile_validator(&policy_schema);
    let report_validator = compile_validator(&report_schema);

    let policy_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("quality")
        .join("quality-policy-v1.json");
    let policy: Value = serde_json::from_str(
        &fs::read_to_string(policy_path).expect("read checked-in quality policy"),
    )
    .expect("quality policy is valid JSON");
    policy_validator
        .validate(&policy)
        .expect("checked-in quality policy matches its schema");

    let report = serde_json::json!({
        "schemaVersion": "axiom.quality_report.v1",
        "headSha": "1111111111111111111111111111111111111111",
        "baseSha": null,
        "target": "aarch64-apple-darwin",
        "tool": {
            "name": "cargo-llvm-cov",
            "requiredVersion": "0.8.5",
            "observedVersion": "0.8.5"
        },
        "profile": {
            "manifest": "stage1/Cargo.toml",
            "package": "axiomc",
            "targets": ["lib", "bin:axiomc"],
            "locked": true,
            "testThreads": 1,
            "skippedTests": ["tests::check_properties_runs_property_only_tests"],
            "budgetSeconds": 600
        },
        "status": "passed",
        "failureClass": null,
        "coverage": {
            "global": {
                "status": "passed",
                "coveredLines": 3,
                "totalLines": 5,
                "floor": { "numerator": 3, "denominator": 5 }
            },
            "changed": {
                "status": "not_applicable",
                "coveredLines": 0,
                "totalLines": 0,
                "floor": { "numerator": 3, "denominator": 5 }
            }
        },
        "findings": [],
        "artifacts": {
            "lcov": ".axiom-build/reports/stage1-coverage.lcov",
            "report": ".axiom-build/reports/stage1-quality-report.json"
        },
        "reproducer": "make stage1-quality-gate",
        "governingIssue": {
            "number": 1463,
            "url": "https://github.com/OMT-Global/axiomlang/issues/1463"
        }
    });
    report_validator
        .validate(&report)
        .expect("minimal passing quality report matches its schema");

    let finding = serde_json::json!({
        "code": "global_coverage_regression",
        "message": "global executable lines are below the configured floor",
        "semanticArea": "compiler.stage1",
        "path": "stage1/crates/axiomc/src/lib.rs",
        "startLine": 10,
        "endLine": 12,
        "reproducer": "make stage1-quality-gate",
        "governingIssue": 1463
    });
    let mut valid_failure = report.clone();
    valid_failure["status"] = serde_json::json!("failed");
    valid_failure["failureClass"] = serde_json::json!("quality");
    valid_failure["coverage"]["global"]["status"] = serde_json::json!("failed");
    valid_failure["coverage"]["global"]["coveredLines"] = serde_json::json!(2);
    valid_failure["findings"] = serde_json::json!([finding]);
    report_validator
        .validate(&valid_failure)
        .expect("quality report accepts a fully evidenced coverage failure");

    let mut unknown_policy_field = policy.clone();
    unknown_policy_field["unexpected"] = serde_json::json!(true);
    assert!(
        !policy_validator.is_valid(&unknown_policy_field),
        "quality policies reject unknown fields"
    );

    let mut unknown_field = report.clone();
    unknown_field["unexpected"] = serde_json::json!(true);
    assert!(
        !report_validator.is_valid(&unknown_field),
        "quality reports reject unknown root fields"
    );

    let mut invalid_sha = report.clone();
    invalid_sha["headSha"] = serde_json::json!("not-a-git-sha");
    assert!(
        !report_validator.is_valid(&invalid_sha),
        "quality reports reject malformed head SHAs"
    );

    let mut pass_with_failure_class = report.clone();
    pass_with_failure_class["failureClass"] = serde_json::json!("quality");
    assert!(
        !report_validator.is_valid(&pass_with_failure_class),
        "passing reports reject non-null failure classes"
    );

    let mut pass_with_finding = report.clone();
    pass_with_finding["findings"] = valid_failure["findings"].clone();
    assert!(
        !report_validator.is_valid(&pass_with_finding),
        "passing reports reject findings"
    );

    let mut pass_with_failed_coverage = report.clone();
    pass_with_failed_coverage["coverage"]["global"]["status"] = serde_json::json!("failed");
    assert!(
        !report_validator.is_valid(&pass_with_failed_coverage),
        "passing reports reject failed coverage results"
    );

    let mut pass_without_lcov = report.clone();
    pass_without_lcov["artifacts"]["lcov"] = Value::Null;
    assert!(
        !report_validator.is_valid(&pass_without_lcov),
        "passing reports require their LCOV artifact"
    );

    let mut failure_without_finding = valid_failure.clone();
    failure_without_finding["findings"] = serde_json::json!([]);
    assert!(
        !report_validator.is_valid(&failure_without_finding),
        "failed reports require at least one finding"
    );

    let mut failure_without_class = valid_failure.clone();
    failure_without_class["failureClass"] = Value::Null;
    assert!(
        !report_validator.is_valid(&failure_without_class),
        "failed reports require a non-null failure class"
    );

    let mut comparison_without_base = report.clone();
    comparison_without_base["coverage"]["changed"] = serde_json::json!({
        "status": "passed",
        "coveredLines": 3,
        "totalLines": 5,
        "floor": { "numerator": 3, "denominator": 5 }
    });
    assert!(
        !report_validator.is_valid(&comparison_without_base),
        "changed coverage cannot be evaluated without a comparison SHA"
    );

    for missing in ["semanticArea", "path", "startLine", "endLine", "reproducer"] {
        let mut malformed = valid_failure.clone();
        malformed["findings"][0]
            .as_object_mut()
            .expect("finding is an object")
            .remove(missing);
        assert!(
            !report_validator.is_valid(&malformed),
            "quality findings reject a missing {missing}"
        );
    }
}

#[test]
fn toolchain_qualification_v0_schema_accepts_strict_passing_evidence() {
    let schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-toolchain-qualification-v0.schema.json"))
            .expect("read toolchain qualification schema"),
    )
    .expect("toolchain qualification schema is valid JSON");
    let validator = compile_validator(&schema);
    let fixture = serde_json::json!({
        "schema": "axiom.toolchain_qualification.v0",
        "trigger": "workflow_dispatch",
        "headSha": "1111111111111111111111111111111111111111",
        "target": "aarch64-apple-darwin",
        "status": "passed",
        "durationMs": 1,
        "failureClass": "none",
        "artifactPaths": [".axiom-build/reports/stage1-quality-report.json"],
        "checks": [{
            "id": "stage1_quality_gate",
            "command": "make stage1-quality-gate",
            "target": "aarch64-apple-darwin",
            "required": true,
            "status": "passed",
            "durationMs": 1,
            "failureClass": "none",
            "exitCode": 0,
            "artifacts": [".axiom-build/reports/stage1-quality-report.json"]
        }]
    });
    validator
        .validate(&fixture)
        .expect("minimal passing qualification evidence matches its schema");

    let mut unknown_field = fixture;
    unknown_field["unexpected"] = serde_json::json!(true);
    assert!(
        !validator.is_valid(&unknown_field),
        "qualification evidence rejects unknown fields"
    );
}

#[test]
fn verification_planner_v0_schemas_are_strict_and_exact_head_bound() {
    let plan: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-verification-plan-v0.schema.json"))
            .expect("read verification plan schema"),
    )
    .expect("verification plan schema is valid JSON");
    let results: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-verification-results-v0.schema.json"))
            .expect("read verification results schema"),
    )
    .expect("verification results schema is valid JSON");
    let verdict: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-verification-verdict-v0.schema.json"))
            .expect("read verification verdict schema"),
    )
    .expect("verification verdict schema is valid JSON");

    for (schema, id, version) in [
        (
            &plan,
            "https://axiom.omt.global/schemas/axiom-verification-plan-v0.schema.json",
            "axiom.verification_plan.v0",
        ),
        (
            &results,
            "https://axiom.omt.global/schemas/axiom-verification-results-v0.schema.json",
            "axiom.verification_results.v0",
        ),
        (
            &verdict,
            "https://axiom.omt.global/schemas/axiom-verification-verdict-v0.schema.json",
            "axiom.verification_verdict.v0",
        ),
    ] {
        assert_eq!(schema["$id"], id);
        assert_eq!(schema["properties"]["schema_version"]["const"], version);
        assert_eq!(schema["additionalProperties"], false);
        compile_validator(schema);
    }
    let digest = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let source_sha = "1111111111111111111111111111111111111111";
    let delivered_sha = "2222222222222222222222222222222222222222";
    let requirement_id = "evidence-positive";
    let mut result = serde_json::json!({
        "schema_version": "axiom.verification_results.v0",
        "plan_digest": digest,
        "source_head_sha": source_sha,
        "delivered_head_sha": delivered_sha,
        "results": [{
            "id": requirement_id,
            "plan_digest": digest,
            "source_head_sha": source_sha,
            "delivered_head_sha": delivered_sha,
            "status": "passed",
            "evidence_digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        }]
    });
    let result_validator = compile_validator(&results);
    result_validator
        .validate(&result)
        .expect("exact-head result validates");
    result["results"][0]["delivered_head_sha"] = serde_json::json!("not-a-commit");
    assert!(
        !result_validator.is_valid(&result),
        "result evidence cannot omit a valid delivered head binding"
    );

    assert_eq!(
        plan["allOf"][0]["then"]["properties"]["requirements"]["minItems"],
        1
    );
    assert_eq!(plan["$defs"]["requirement"]["additionalProperties"], false);
    assert_eq!(results["$defs"]["result"]["additionalProperties"], false);
    assert_eq!(verdict["$defs"]["ids"]["uniqueItems"], true);
    let impossible_success = serde_json::json!({
        "schema_version": "axiom.verification_verdict.v0",
        "plan_digest": digest,
        "status": "passed",
        "source_head_sha": source_sha,
        "delivered_head_sha": delivered_sha,
        "missing": [requirement_id],
        "duplicate": [],
        "invalid": [],
        "failed": []
    });
    assert!(
        !compile_validator(&verdict).is_valid(&impossible_success),
        "a passing verdict cannot retain evidence blockers"
    );
}

#[test]
fn bounded_executor_v0_schemas_are_strict_and_fail_closed() {
    let schemas: Vec<Value> = [
        "axiom-executor-request-v0.schema.json",
        "axiom-executor-report-v0.schema.json",
        "axiom-executor-resume-v0.schema.json",
    ]
    .into_iter()
    .map(|name| {
        serde_json::from_str(
            &fs::read_to_string(schema_dir().join(name)).expect("read executor schema"),
        )
        .expect("executor schema is valid JSON")
    })
    .collect();
    for (schema, id, version) in [
        (
            &schemas[0],
            "https://axiom.omt.global/schemas/axiom-executor-request-v0.schema.json",
            "axiom.executor_request.v0",
        ),
        (
            &schemas[1],
            "https://axiom.omt.global/schemas/axiom-executor-report-v0.schema.json",
            "axiom.bounded_executor.v0",
        ),
        (
            &schemas[2],
            "https://axiom.omt.global/schemas/axiom-executor-resume-v0.schema.json",
            "axiom.executor_resume.v0",
        ),
    ] {
        assert_eq!(schema["$id"], id);
        assert_eq!(schema["properties"]["schema_version"]["const"], version);
        assert_eq!(schema["additionalProperties"], false);
        compile_validator(schema);
    }

    assert_eq!(schemas[1]["properties"]["seal_mac"]["$ref"], "#/$defs/mac");
    assert!(schemas[1]["properties"].get("seal_key").is_none());
    assert!(schemas[2]["properties"].get("seal_key").is_none());
    assert!(
        schemas[1]["$defs"]["signedDeliveryEvidence"]["properties"]
            .get("fresh")
            .is_none()
    );
    assert_eq!(
        schemas[1]["$defs"]["signedDeliveryEvidence"]["properties"]["evidence_mac"]["$ref"],
        "#/$defs/mac"
    );

    let digest = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let sha = "1111111111111111111111111111111111111111";
    let resolved = serde_json::json!({
        "schema_version": "axiom.bounded_executor.v0",
        "task_contract_digest": digest,
        "base_sha": sha,
        "dry_run": false,
        "state": "resolved",
        "budgets": {
            "limits": { "time_seconds": 1, "tokens": 1, "retries": 0, "cost_usd_micros": 0 },
            "consumed": { "time_seconds": 0, "tokens": 0, "retries": 0, "cost_usd_micros": 0 }
        },
        "failures": [],
        "events": [{
            "sequence": 0,
            "kind": "resolved",
            "detail": "candidate",
            "state": "resolved",
            "budget_usage": { "time_seconds": 0, "tokens": 0, "retries": 0, "cost_usd_micros": 0 },
            "previous_digest": digest,
            "event_digest": digest
        }],
        "state_digest": digest
    });
    assert!(
        !compile_validator(&schemas[1]).is_valid(&resolved),
        "resolved cannot omit proposal, candidate, fresh verification, or delivery evidence"
    );

    let mut resume = serde_json::json!({
        "schema_version": "axiom.executor_resume.v0",
        "executor_state_mac": "hmac-sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "seal_key_id": "executor-test",
        "task_contract_digest": digest,
        "transaction_id": "txn-0123456789abcdef",
        "transaction_digest": digest,
        "candidate_digest": null,
        "remaining_budgets": { "time_seconds": 0, "tokens": 0, "retries": 0, "cost_usd_micros": 0 },
        "next_event_sequence": 1
    });
    let validator = compile_validator(&schemas[2]);
    assert!(validator.is_valid(&resume));
    resume["widened_files"] = serde_json::json!(["outside.ax"]);
    assert!(
        !validator.is_valid(&resume),
        "resume cannot carry widened authority"
    );
}

#[test]
fn execution_transaction_v0_schemas_are_strict_and_secret_safe() {
    let policy: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-execution-policy-v0.schema.json"))
            .expect("read execution policy schema"),
    )
    .expect("execution policy schema is valid JSON");
    let audit: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-execution-transaction-v0.schema.json"))
            .expect("read execution transaction schema"),
    )
    .expect("execution transaction schema is valid JSON");

    assert_eq!(
        policy["$id"],
        "https://axiom.omt.global/schemas/axiom-execution-policy-v0.schema.json"
    );
    assert_eq!(
        audit["$id"],
        "https://axiom.omt.global/schemas/axiom-execution-transaction-v0.schema.json"
    );
    assert_eq!(policy["additionalProperties"], false);
    assert_eq!(audit["additionalProperties"], false);
    assert_eq!(
        policy["properties"]["paths"]["properties"]["follow_symlinks"]["const"],
        false
    );
    assert_eq!(
        policy["properties"]["commands"]["properties"]["allow_shell"]["const"],
        false
    );
    assert_eq!(
        policy["properties"]["delivery"]["properties"]["allow_force_push"]["const"],
        false
    );
    assert_eq!(
        policy["properties"]["delivery"]["properties"]["allow_self_approval"]["const"],
        false
    );
    assert_eq!(
        policy["properties"]["delivery"]["properties"]["allow_policy_edits"]["const"],
        false
    );

    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("json-fixtures")
        .join("execution-transaction");
    let mut policy_fixture: Value = serde_json::from_str(
        &fs::read_to_string(fixture_root.join("strict-local.policy.json"))
            .expect("read execution policy fixture"),
    )
    .expect("execution policy fixture is valid JSON");
    let audit_fixture: Value = serde_json::from_str(
        &fs::read_to_string(fixture_root.join("interrupted.audit.json"))
            .expect("read execution audit fixture"),
    )
    .expect("execution audit fixture is valid JSON");
    compile_validator(&policy)
        .validate(&policy_fixture)
        .expect("execution policy fixture matches schema");
    compile_validator(&audit)
        .validate(&audit_fixture)
        .expect("execution audit fixture matches schema");

    let serialized = serde_json::to_string(&audit_fixture).expect("serialize audit fixture");
    assert!(!serialized.contains("secret_value"));
    assert!(!serialized.contains(&["github_", "pat_"].concat()));

    policy_fixture["network"]["allowed_hosts"] = serde_json::json!(["example.com"]);
    assert!(
        !compile_validator(&policy).is_valid(&policy_fixture),
        "deny-mode network policy cannot retain an allowlist"
    );
}

#[test]
fn agent_task_v0_schemas_are_strict_and_current() {
    let input: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-agent-task-spec-v0.schema.json"))
            .expect("read agent task specification schema"),
    )
    .expect("agent task specification schema is valid JSON");
    let output: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-agent-task-v0.schema.json"))
            .expect("read agent task contract schema"),
    )
    .expect("agent task contract schema is valid JSON");

    assert_eq!(
        input["$id"],
        "https://axiom.omt.global/schemas/axiom-agent-task-spec-v0.schema.json"
    );
    assert_eq!(
        output["$id"],
        "https://axiom.omt.global/schemas/axiom-agent-task-v0.schema.json"
    );
    assert_eq!(
        input["properties"]["schema_version"]["const"],
        "axiom.agent_task.spec.v0"
    );
    assert_eq!(
        output["properties"]["schema_version"]["const"],
        "axiom.agent_task.v0"
    );
    assert_eq!(output["properties"]["command"]["const"], "task-contract");
    assert_eq!(input["additionalProperties"], false);
    assert_eq!(input["properties"]["task"]["unevaluatedProperties"], false);
    assert_eq!(input["$defs"]["authority"]["additionalProperties"], false);
    assert_eq!(
        input["$defs"]["terminalConditions"]["additionalProperties"],
        false
    );
    assert_eq!(
        input["$defs"]["deliveryPermissions"]["properties"]["approve_own_pull_request"]["const"],
        false
    );
    assert_eq!(
        input["$defs"]["deliveryPermissions"]["properties"]["force_push"]["const"],
        false
    );

    // Compile the self-contained input schema here. The output contract reuses
    // the exact task definition by URI so consumers cannot validate against a
    // looser parallel definition.
    let validator = compile_validator(&input);
    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("json-fixtures")
        .join("task-contract")
        .join("feature-approved.spec.json");
    let mut fixture: Value = serde_json::from_str(
        &fs::read_to_string(fixture_path).expect("read approved task fixture"),
    )
    .expect("approved task fixture is valid JSON");
    assert!(validator.is_valid(&fixture));
    fixture["task"]["terminal_conditions"]["unexpected"] = serde_json::json!(true);
    assert!(
        !validator.is_valid(&fixture),
        "terminal conditions must reject undeclared fields"
    );
}

#[test]
fn intent_ir_v0_requires_deterministic_provenance_and_traceable_diagnostics() {
    let schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-intent-ir-v0.schema.json"))
            .expect("read Intent IR schema"),
    )
    .expect("Intent IR schema is valid JSON");
    let validator = compile_validator(&schema);
    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("intent_ir_smoke")
        .join("intent-ir.json");
    let fixture: Value = serde_json::from_str(
        &fs::read_to_string(fixture_path).expect("read Intent IR smoke fixture"),
    )
    .expect("Intent IR smoke fixture is valid JSON");

    assert!(validator.is_valid(&fixture));
    assert_eq!(fixture["provenance"]["path_policy"], "package_relative");
    assert_eq!(fixture["diagnostics"], serde_json::json!([]));

    let mut absolute_input = fixture.clone();
    absolute_input["provenance"]["inputs"][0]["path"] = serde_json::json!("/checkout/src/main.ax");
    assert!(!validator.is_valid(&absolute_input));

    let mut untraceable_diagnostic = fixture;
    untraceable_diagnostic["diagnostics"] = serde_json::json!([{
        "code": "intent_ir_incomplete_module",
        "severity": "warning",
        "message": "module could not be represented",
        "node_ids": []
    }]);
    assert!(!validator.is_valid(&untraceable_diagnostic));
}

#[test]
fn formatter_edit_v1_schema_metadata_is_current() {
    let schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-format-edit-v1.schema.json"))
            .expect("read formatter edit schema"),
    )
    .expect("formatter edit schema is valid JSON");

    assert_eq!(
        schema["$id"],
        "https://axiom.omt.global/schemas/axiom-format-edit-v1.schema.json"
    );
    assert_eq!(schema["title"], "Axiom formatter edit report v1");
    assert_eq!(
        schema["properties"]["schema_version"]["const"],
        json_contract::JSON_SCHEMA_VERSION
    );
    assert_eq!(schema["properties"]["command"]["const"], "fmt");
    let edit = &schema["$defs"]["edit"];
    for field in [
        "action",
        "line",
        "before",
        "after",
        "start_byte",
        "end_byte",
        "replacement",
    ] {
        assert!(
            edit["required"]
                .as_array()
                .expect("formatter edit required fields")
                .iter()
                .any(|required| required == field),
            "formatter edit schema requires {field}"
        );
    }
    let validator = compile_validator(&schema);
    let valid_edit = serde_json::json!({
        "schema_version": json_contract::JSON_SCHEMA_VERSION,
        "schema": "stage1/schemas/axiom-format-edit-v1.schema.json",
        "ok": false,
        "command": "fmt",
        "check": true,
        "input": "files",
        "files": [{
            "path": "src/main.ax",
            "changed": true,
            "range": null,
            "edits": [{
                "action": "replace_line",
                "line": 1,
                "before": "print 1",
                "after": "print 1",
                "start_byte": 7,
                "end_byte": 7,
                "replacement": "\n"
            }]
        }],
        "changed": 1
    });
    assert!(validator.is_valid(&valid_edit));

    let mut missing_replacement = valid_edit.clone();
    missing_replacement["files"][0]["edits"][0]
        .as_object_mut()
        .expect("formatter edit object")
        .remove("replacement");
    assert!(!validator.is_valid(&missing_replacement));

    let mut negative_offset = valid_edit;
    negative_offset["files"][0]["edits"][0]["start_byte"] = serde_json::json!(-1);
    assert!(!validator.is_valid(&negative_offset));
}

#[test]
fn editor_metadata_schemas_are_parseable_and_current() {
    let compiler_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom.stage1.v1.schema.json"))
            .expect("read compiler JSON schema"),
    )
    .expect("compiler JSON schema is valid JSON");
    let manifest_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom.toml.schema.json"))
            .expect("read manifest JSON schema"),
    )
    .expect("manifest schema is valid JSON");
    let inspect_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-inspect-v0.schema.json"))
            .expect("read inspect JSON schema"),
    )
    .expect("inspect schema is valid JSON");
    let doc_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom.docs.v1.schema.json"))
            .expect("read doc JSON schema"),
    )
    .expect("doc schema is valid JSON");

    assert_eq!(
        compiler_schema["properties"]["schema_version"]["const"],
        json_contract::JSON_SCHEMA_VERSION
    );
    assert_eq!(
        compiler_schema["$id"],
        "https://axiom.omt.global/schemas/axiom.stage1.v1.schema.json"
    );
    assert_eq!(
        compiler_schema["properties"]["command"]["type"], "string",
        "compiler schema accepts all command names used by shared JSON error envelopes"
    );
    assert_eq!(
        compiler_schema["properties"]["command"]["minLength"], 1,
        "compiler schema rejects empty command names without pinning the CLI command set"
    );
    assert_eq!(
        manifest_schema["$id"],
        "https://axiom.omt.global/schemas/axiom.toml.schema.json"
    );
    assert_eq!(
        inspect_schema["$id"],
        "https://axiom.omt.global/schemas/axiom-inspect-v0.schema.json"
    );
    assert_eq!(
        doc_schema["$id"],
        "https://axiom.omt.global/schemas/axiom.docs.v1.schema.json"
    );
    assert_eq!(doc_schema["properties"]["command"]["const"], "doc");
    assert_eq!(doc_schema["properties"]["schema"]["const"], "axiom.docs.v1");
    assert_eq!(
        doc_schema["properties"]["schema_version"]["const"],
        json_contract::JSON_SCHEMA_VERSION
    );
    assert_eq!(
        inspect_schema["properties"]["schema_version"]["const"],
        json_contract::JSON_SCHEMA_VERSION
    );
    let inspect_commands = inspect_schema["properties"]["command"]["enum"]
        .as_array()
        .expect("inspect command enum");
    for command in [
        "inspect graph",
        "inspect symbols",
        "inspect effects",
        "inspect evidence",
        "inspect artifacts",
    ] {
        assert!(
            inspect_commands.iter().any(|value| value == command),
            "inspect schema includes {command}"
        );
    }
    let inspect_validator = compile_validator(&inspect_schema);
    for sample in [
        serde_json::json!({
            "schema_version": json_contract::JSON_SCHEMA_VERSION,
            "schema": "stage1/schemas/axiom-inspect-v0.schema.json",
            "ok": true,
            "command": "inspect graph",
            "project": "example",
            "packages": [],
            "modules": []
        }),
        serde_json::json!({
            "schema_version": json_contract::JSON_SCHEMA_VERSION,
            "schema": "stage1/schemas/axiom-inspect-v0.schema.json",
            "ok": true,
            "command": "inspect symbols",
            "project": "example",
            "symbols": []
        }),
        serde_json::json!({
            "schema_version": json_contract::JSON_SCHEMA_VERSION,
            "schema": "stage1/schemas/axiom-inspect-v0.schema.json",
            "ok": true,
            "command": "inspect effects",
            "project": "example",
            "effects": []
        }),
        serde_json::json!({
            "schema_version": json_contract::JSON_SCHEMA_VERSION,
            "schema": "stage1/schemas/axiom-inspect-v0.schema.json",
            "ok": true,
            "command": "inspect evidence",
            "project": "example",
            "evidence": []
        }),
        serde_json::json!({
            "schema_version": json_contract::JSON_SCHEMA_VERSION,
            "schema": "stage1/schemas/axiom-inspect-v0.schema.json",
            "ok": true,
            "command": "inspect artifacts",
            "project": "example",
            "artifacts": []
        }),
    ] {
        inspect_validator
            .validate(&sample)
            .expect("inspect sample validates against inspect schema");
    }
    assert!(manifest_schema["properties"]["capabilities"]["properties"]["env"]["oneOf"].is_array());

    let test_target = &manifest_schema["properties"]["tests"]["items"]["properties"];
    let parser_contract: Value = serde_json::from_str(
        &fs::read_to_string(
            schema_dir()
                .parent()
                .expect("stage1 root")
                .join("compatibility/manifest-parser-contract-v1.json"),
        )
        .expect("read manifest parser contract"),
    )
    .expect("manifest parser contract is valid JSON");
    for field in [
        "kind",
        "stderr",
        "expected_error",
        "http",
        "capabilities",
        "package",
    ] {
        assert!(
            test_target[field].is_object(),
            "manifest schema includes tests[].{field}"
        );
    }
    assert_eq!(
        test_target["kind"]["enum"],
        serde_json::json!(TEST_KIND_NAMES),
        "manifest schema test kinds must exactly match the parser contract"
    );
    assert_eq!(
        parser_contract["test_kinds"],
        serde_json::json!(TEST_KIND_NAMES),
        "governed parser contract must match parser test kinds"
    );
    assert!(
        !PER_TEST_CAPABILITIES_SUPPORTED,
        "update schema parity when per-test capabilities become enforceable"
    );
    assert_eq!(
        test_target["capabilities"]["maxItems"], 0,
        "schema must reject non-empty per-test capabilities while the parser does"
    );
    assert_eq!(
        parser_contract["test_capabilities"]["names"],
        serde_json::json!(
            KNOWN_CAPABILITIES
                .iter()
                .map(|capability| capability.name())
                .collect::<Vec<_>>()
        ),
        "governed parser contract must match parser capability names"
    );
    assert_eq!(
        test_target["capabilities"]["items"]["enum"], parser_contract["test_capabilities"]["names"],
        "manifest schema capability names must match the governed parser contract"
    );
    assert_eq!(
        parser_contract["dependency_version_pattern"], DEPENDENCY_VERSION_PATTERN,
        "governed parser contract must match canonical dependency version syntax"
    );
    assert_eq!(
        manifest_schema["properties"]["dependencies"]["additionalProperties"]["oneOf"][1]["properties"]
            ["version"]["pattern"],
        parser_contract["dependency_version_pattern"],
        "manifest schema dependency versions must match the governed parser contract"
    );

    let manifest_capabilities = &manifest_schema["properties"]["capabilities"]["properties"];
    for field in [
        "deny_by_default",
        "unsafe_opt_ins",
        "unsafe_rationale",
        "owners",
        "rationale",
    ] {
        assert!(
            manifest_capabilities[field].is_object(),
            "manifest schema includes capabilities.{field}"
        );
    }

    let manifest_validator = compile_validator(&manifest_schema);
    let parser_parity_manifest = serde_json::json!({
        "package": {"name": "parity", "version": "0.1.0"},
        "dependencies": {
            "dep": {"path": "../dep", "version": "^1.2.3"}
        },
        "tests": [{
            "name": "http",
            "entry": "src/http_test.ax",
            "http": {
                "bind": "127.0.0.1:0",
                "path": "/health",
                "expected_body": "ok"
            }
        }],
        "capabilities": {
            "env": true,
            "env_unrestricted": true,
            "unsafe_rationale": "This test intentionally reads the inherited environment."
        }
    });
    manifest_validator
        .validate(&parser_parity_manifest)
        .expect("manifest schema accepts fields supported by the parser");
    for registry in [
        "https://registry.example.test/index",
        "file:///tmp/axiom-registry",
    ] {
        let mut with_registry = parser_parity_manifest.clone();
        with_registry["publish"] = serde_json::json!({"registry": registry});
        manifest_validator
            .validate(&with_registry)
            .unwrap_or_else(|error| {
                panic!("schema must accept parser registry {registry}: {error}")
            });
    }
    for registry in [
        "https://registry.example.test/index?mirror=1",
        "https://registry.example.test/index#fragment",
        "file:///tmp/registry?mirror=1",
    ] {
        let mut with_registry = parser_parity_manifest.clone();
        with_registry["publish"] = serde_json::json!({"registry": registry});
        assert!(
            !manifest_validator.is_valid(&with_registry),
            "schema must reject parser-invalid registry {registry}"
        );
    }
    for kind in TEST_KIND_NAMES {
        let mut with_kind = parser_parity_manifest.clone();
        with_kind["tests"][0]["kind"] = serde_json::json!(kind);
        manifest_validator
            .validate(&with_kind)
            .unwrap_or_else(|error| panic!("schema must accept parser test kind {kind}: {error}"));
    }
    let mut unknown_kind = parser_parity_manifest.clone();
    unknown_kind["tests"][0]["kind"] = serde_json::json!("integration");
    assert!(
        !manifest_validator.is_valid(&unknown_kind),
        "schema must reject a test kind rejected by the parser"
    );
    let mut unsupported_capability = parser_parity_manifest.clone();
    unsupported_capability["tests"][0]["capabilities"] = serde_json::json!(["net"]);
    assert!(
        !manifest_validator.is_valid(&unsupported_capability),
        "schema must reject non-empty per-test capabilities while the parser does"
    );
    let mut invalid_dependency_version = parser_parity_manifest;
    invalid_dependency_version["dependencies"]["dep"]["version"] = serde_json::json!("^01.2.3");
    assert!(
        !manifest_validator.is_valid(&invalid_dependency_version),
        "manifest schema and parser both reject noncanonical dependency versions"
    );

    let known_capability_names: Vec<&str> = KNOWN_CAPABILITIES
        .iter()
        .map(|capability| capability.name())
        .collect();
    for capability in &known_capability_names {
        assert!(
            manifest_capabilities[*capability].is_object(),
            "manifest schema includes capabilities.{capability}"
        );
    }
    let manifest_unsafe_opt_ins = manifest_capabilities["unsafe_opt_ins"]["items"]["enum"]
        .as_array()
        .expect("manifest unsafe opt-in capability enum");
    for capability in &known_capability_names {
        assert!(
            manifest_unsafe_opt_ins
                .iter()
                .any(|value| value == capability),
            "manifest schema unsafe_opt_ins includes {capability}"
        );
    }

    let descriptor = &compiler_schema["$defs"]["capability"]["properties"];
    for field in ["deny_by_default", "unsafe_opt_in", "owner", "rationale"] {
        assert!(
            descriptor[field].is_object(),
            "compiler schema includes capability descriptor {field}"
        );
    }
    let descriptor_names = descriptor["name"]["enum"]
        .as_array()
        .expect("compiler capability descriptor name enum");
    for capability in &known_capability_names {
        assert!(
            descriptor_names.iter().any(|value| value == capability),
            "compiler schema capability descriptors include {capability}"
        );
    }
}

#[test]
fn inspect_evidence_cli_is_wired_for_text_and_json_output() {
    let temp = tempfile::tempdir().expect("create inspect evidence tempdir");
    let project = temp.path().join("inspect-evidence-app");
    let project_arg = project.to_str().expect("project path");
    let created = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(["new", project_arg, "--name", "inspect-evidence-app"])
        .output()
        .expect("run axiomc new");
    assert!(created.status.success(), "new failed: {:?}", created);

    let json_output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(["inspect", "evidence", project_arg, "--json"])
        .output()
        .expect("run inspect evidence json");
    assert!(
        json_output.status.success(),
        "inspect evidence json failed: {:?}",
        json_output
    );
    let payload: Value = serde_json::from_slice(&json_output.stdout).expect("evidence JSON");
    assert_eq!(payload["command"], "inspect evidence");
    assert!(payload["evidence"].is_array());
    let schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-inspect-v0.schema.json"))
            .expect("read inspect schema"),
    )
    .expect("inspect schema JSON");
    compile_validator(&schema)
        .validate(&payload)
        .expect("inspect evidence JSON validates against inspect schema");

    let text_output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(["inspect", "evidence", project_arg])
        .output()
        .expect("run inspect evidence text");
    assert!(
        text_output.status.success(),
        "inspect evidence text failed: {:?}",
        text_output
    );
    let text = String::from_utf8(text_output.stdout).expect("evidence text UTF-8");
    assert!(text.contains("lockfile axiom.lock"), "text output: {text}");
}

#[test]
fn backend_target_v0_schema_and_fixture_are_well_formed() {
    let schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-target-v0.schema.json"))
            .expect("read backend target schema"),
    )
    .expect("backend target schema is valid JSON");
    assert_eq!(
        schema["$id"],
        "https://axiom.omt.global/schemas/axiom-target-v0.schema.json"
    );
    assert_eq!(schema["title"], "Axiom Backend Target Interface v0");

    let contract = &schema["$defs"]["targetContract"];
    let required = contract["required"]
        .as_array()
        .expect("targetContract required list");
    for field in [
        "id",
        "class",
        "input_node_kinds",
        "supported_effect_kinds",
        "supported_type_features",
        "artifact_outputs",
        "evidence_requirements",
        "unsupported_feature_diagnostics",
    ] {
        assert!(
            required.iter().any(|value| value == field),
            "targetContract requires {field}"
        );
    }

    let classes = schema["$defs"]["targetClass"]["enum"]
        .as_array()
        .expect("target class enum");
    for class in [
        "native_binary",
        "rust_source",
        "zero_source",
        "go_source",
        "typescript_source",
        "python_source",
        "openapi_spec",
        "sql_migration",
        "terraform_module",
        "policy_bundle",
        "documentation",
        "runbook",
    ] {
        assert!(
            classes.iter().any(|value| value == class),
            "target class enum includes {class}"
        );
    }

    assert_eq!(
        schema["$defs"]["nodeId"]["pattern"], "^axiom://[A-Za-z0-9._~:/#@!$&'()*+,;=%-]+$",
        "target nodeId stays aligned with Intent IR nodeId characters"
    );

    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("target_smoke")
        .join("targets.json");
    let fixture: Value = serde_json::from_str(
        &fs::read_to_string(&fixture_path).expect("read backend target smoke fixture"),
    )
    .expect("backend target smoke fixture is valid JSON");
    assert_eq!(fixture["schema_version"], "axiom.target.v0");
    let targets = fixture["targets"]
        .as_array()
        .expect("smoke fixture targets array");
    let ids: Vec<&str> = targets
        .iter()
        .map(|t| t["id"].as_str().expect("target id"))
        .collect();
    assert!(
        ids.contains(&"axiom://target/stage1-generated-rust"),
        "fixture maps the generated-Rust compatibility backend"
    );
    assert!(
        ids.contains(&"axiom://target/stage1-direct-native"),
        "fixture maps the direct-native backend"
    );
    let generated_rust = targets
        .iter()
        .find(|target| target["id"] == "axiom://target/stage1-generated-rust")
        .expect("fixture includes generated-Rust target");
    let artifacts = generated_rust["artifact_outputs"]
        .as_array()
        .expect("generated-Rust target artifact outputs");
    assert!(
        artifacts.iter().any(|artifact| {
            artifact["id"] == "axiom://target/stage1-generated-rust/artifact/source"
                && artifact["kind"] == "rust_source"
        }),
        "generated-Rust target emits a Rust source artifact"
    );
}

#[test]
fn openapi_service_fixture_is_deterministic() {
    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("openapi_service")
        .join("dist")
        .join("openapi.json");
    let fixture: Value =
        serde_json::from_str(&fs::read_to_string(&fixture_path).expect("read OpenAPI fixture"))
            .expect("OpenAPI fixture is valid JSON");

    assert_eq!(fixture["openapi"], "3.1.0");
    assert_eq!(fixture["info"]["title"], "openapi-service");
    assert_eq!(
        fixture["paths"]["/ready"]["get"]["operationId"],
        "get_ready"
    );
    assert_eq!(
        fixture["paths"]["/ready"]["get"]["responses"]["200"]["content"]["text/plain; charset=utf-8"]
            ["schema"]["type"],
        "string"
    );
    assert_eq!(
        fixture["paths"]["/ready"]["get"]["x-axiom"]["target_id"],
        "axiom://target/stage1-openapi-v0"
    );
}

#[test]
fn policy_bundle_service_fixture_is_deterministic() {
    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("policy_bundle_service")
        .join("dist")
        .join("policy-bundle.json");
    let fixture: Value =
        serde_json::from_str(&fs::read_to_string(&fixture_path).expect("read policy fixture"))
            .expect("policy fixture is valid JSON");

    assert_eq!(fixture["schema_version"], "axiom.policy_bundle.v0");
    assert_eq!(
        fixture["target_id"],
        "axiom://target/stage1-policy-bundle-v0"
    );
    assert_eq!(
        fixture["allowed_effect_kinds"],
        serde_json::json!(["clock.now", "clock.sleep", "env.read", "fs.read"])
    );
    assert_eq!(
        fixture["observed_effects"]
            .as_array()
            .expect("effects")
            .len(),
        3
    );
}

#[test]
fn runbook_service_fixture_is_deterministic() {
    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("runbook_service")
        .join("dist")
        .join("runbook.md");
    let fixture = fs::read_to_string(&fixture_path).expect("read runbook fixture");

    assert!(fixture.contains("# Operator Runbook: runbook-service"));
    assert!(fixture.contains("axiom://target/stage1-runbook-v0"));
    assert!(fixture.contains("DescribeOperatorMode"));
    assert!(fixture.contains("RunbookSmokeTest"));
    assert!(fixture.contains("env.read"));
    assert!(fixture.contains("1 passing, 0 failing, 0 missing, 1 provided"));
    assert!(!fixture.contains(env!("CARGO_MANIFEST_DIR")));
    assert!(!fixture.contains("/Users/"));
    assert!(!fixture.contains("/home/"));
}

#[test]
fn agent_native_authorize_fixtures_prove_semantic_evidence_artifact_flow() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("agent_native_authorize")
        .join("fixtures");
    let graph: Value =
        serde_json::from_str(&fs::read_to_string(fixture_dir.join("graph.json")).expect("graph"))
            .expect("graph fixture is valid JSON");
    let effects: Value = serde_json::from_str(
        &fs::read_to_string(fixture_dir.join("effects.json")).expect("effects"),
    )
    .expect("effects fixture is valid JSON");
    let evidence: Value = serde_json::from_str(
        &fs::read_to_string(fixture_dir.join("evidence.json")).expect("evidence"),
    )
    .expect("evidence fixture is valid JSON");
    let artifacts: Value = serde_json::from_str(
        &fs::read_to_string(fixture_dir.join("artifacts.json")).expect("artifacts"),
    )
    .expect("artifacts fixture is valid JSON");

    assert_eq!(graph["command"], "inspect graph");
    assert_eq!(effects["command"], "inspect effects");
    assert_eq!(evidence["command"], "evidence");
    assert_eq!(artifacts["command"], "inspect artifacts");

    let nodes = graph["nodes"].as_array().expect("graph nodes");
    assert!(
        nodes
            .iter()
            .any(|node| { node["kind"] == "capability" && node["name"] == "AuthorizeToken" })
    );
    assert!(nodes.iter().any(|node| {
        node["kind"] == "axiom" && node["name"] == "AuthorizationDecisionAuditable"
    }));
    assert!(
        nodes.iter().any(|node| {
            node["kind"] == "evidence" && node["name"] == "AuthorizeTokenSmokeTest"
        })
    );
    assert!(
        graph["edges"]
            .as_array()
            .expect("graph edges")
            .iter()
            .any(|edge| edge["kind"] == "requires_evidence"
                && edge["from"] == "axiom://semantic/capability/AuthorizeToken"
                && edge["to"] == "axiom://semantic/evidence/AuthorizeTokenSmokeTest")
    );

    assert_eq!(
        effects["effects"]
            .as_array()
            .expect("effects")
            .iter()
            .map(|effect| effect["kind"].as_str().expect("effect kind"))
            .collect::<Vec<_>>(),
        vec!["env.read", "clock.now"]
    );
    assert_eq!(evidence["summary"]["passing"], 1);
    assert_eq!(evidence["summary"]["missing"], 0);

    let artifact_kinds = artifacts["artifacts"]
        .as_array()
        .expect("artifacts")
        .iter()
        .map(|artifact| artifact["kind"].as_str().expect("artifact kind"))
        .collect::<std::collections::BTreeSet<_>>();
    for kind in [
        "manifest",
        "lockfile",
        "build_entry",
        "test_entry",
        "openapi_spec",
        "policy_bundle",
        "runbook",
    ] {
        assert!(
            artifact_kinds.contains(kind),
            "artifact fixture includes {kind}"
        );
    }

    for fixture_name in [
        "graph.json",
        "effects.json",
        "evidence.json",
        "artifacts.json",
    ] {
        let fixture = fs::read_to_string(fixture_dir.join(fixture_name)).expect("fixture text");
        assert!(!fixture.contains("/Users/"));
        assert!(!fixture.contains("/home/"));
        assert!(!fixture.contains("/private/"));
        assert!(!fixture.contains("codex/worktrees"));
    }
}

#[test]
fn semantic_verification_schemas_and_fixtures_are_well_formed() {
    let decision_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-decision-record-v0.schema.json"))
            .expect("read decision record schema"),
    )
    .expect("decision record schema is valid JSON");
    assert_eq!(
        decision_schema["$id"],
        "https://axiom.omt.global/schemas/axiom-decision-record-v0.schema.json"
    );
    let decision_validator = compile_validator(&decision_schema);
    let decision_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("decision_records")
        .join("decisions");
    for entry in fs::read_dir(&decision_dir).expect("read decision fixtures") {
        let path = entry.expect("decision fixture entry").path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let fixture: Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read decision record fixture"))
                .expect("decision record fixture is valid JSON");
        decision_validator
            .validate(&fixture)
            .expect("decision record fixture matches schema");
    }

    let verify_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-verify-v0.schema.json"))
            .expect("read verify schema"),
    )
    .expect("verify schema is valid JSON");
    assert_eq!(
        verify_schema["$id"],
        "https://axiom.omt.global/schemas/axiom-verify-v0.schema.json"
    );
    assert_eq!(verify_schema["properties"]["command"]["const"], "verify");

    let diff_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-semantic-diff-v0.schema.json"))
            .expect("read semantic diff schema"),
    )
    .expect("semantic diff schema is valid JSON");
    assert_eq!(
        diff_schema["$id"],
        "https://axiom.omt.global/schemas/axiom-semantic-diff-v0.schema.json"
    );
    let diff_validator = compile_validator(&diff_schema);
    diff_validator
        .validate(&serde_json::json!({
            "schema_version": "axiom.semantic_diff.v0",
            "ok": true,
            "command": "semantic-diff",
            "old": "base.json",
            "new": "breaking.json",
            "summary": {
                "breaking": 1,
                "additive": 0,
                "informational": 0
            },
            "changes": [
                {
                    "change": "added",
                    "severity": "breaking",
                    "node_kind": "Capability",
                    "node_id": "axiom://package/demo/capability/network",
                    "description": "added Capability network"
                }
            ]
        }))
        .expect("semantic diff sample validates");
}

#[test]
fn package_trust_v1_schemas_compile_and_validate_contract_sections() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("package-trust")
        .join("contract")
        .join("package-trust.json");
    let contract: Value = serde_json::from_str(
        &fs::read_to_string(fixtures).expect("read package trust contract fixture"),
    )
    .expect("package trust fixture is valid JSON");
    let schemas = [
        (
            "package_signature",
            "axiom-package-signature-v1.schema.json",
            "https://axiom.omt.global/schemas/axiom-package-signature-v1.schema.json",
            "axiom.package_signature.v1",
        ),
        (
            "trust_roots",
            "axiom-trust-roots-v1.schema.json",
            "https://axiom.omt.global/schemas/axiom-trust-roots-v1.schema.json",
            "axiom.package_trust_roots.v1",
        ),
        (
            "registry_index",
            "axiom-registry-index-v2.schema.json",
            "https://axiom.omt.global/schemas/axiom-registry-index-v2.schema.json",
            "axiom.registry_index.v2",
        ),
        (
            "verification_expectation",
            "axiom-package-verification-expectation-v1.schema.json",
            "https://axiom.omt.global/schemas/axiom-package-verification-expectation-v1.schema.json",
            "axiom.package_verification_expectation.v1",
        ),
        (
            "verification",
            "axiom-package-verification-v1.schema.json",
            "https://axiom.omt.global/schemas/axiom-package-verification-v1.schema.json",
            "axiom.package_verification.v1",
        ),
    ];
    for (section, file, id, version) in schemas {
        let schema: Value = serde_json::from_str(
            &fs::read_to_string(schema_dir().join(file)).expect("read package trust schema"),
        )
        .expect("package trust schema is valid JSON");
        assert_eq!(schema["$id"], id);
        assert_eq!(schema["properties"]["schema_version"]["const"], version);
        assert_eq!(
            schema["properties"]["contract_status"]["enum"],
            serde_json::json!(["contract_only", "implemented"])
        );
        assert_eq!(schema["additionalProperties"], false);
        let validator = compile_validator(&schema);
        assert_eq!(contract[section]["contract_status"], "contract_only");
        validator
            .validate(&contract[section])
            .unwrap_or_else(|error| panic!("{section} fixture must match {file}: {error}"));

        let mut implemented = contract[section].clone();
        implemented["contract_status"] = serde_json::json!("implemented");
        validator.validate(&implemented).unwrap_or_else(|error| {
            panic!("{section} implemented shape must match {file}: {error}")
        });

        let mut unknown = contract[section].clone();
        unknown["legacy_hmac"] = serde_json::json!("forbidden");
        assert!(
            !validator.is_valid(&unknown),
            "{section} root must reject unknown fields"
        );
    }

    let expectation_schema: Value = serde_json::from_str(
        &fs::read_to_string(
            schema_dir().join("axiom-package-verification-expectation-v1.schema.json"),
        )
        .expect("read package verification expectation schema"),
    )
    .expect("package verification expectation schema is valid JSON");
    let mut production_expectation = contract["verification_expectation"].clone();
    production_expectation["contract_status"] = serde_json::json!("implemented");
    production_expectation
        .as_object_mut()
        .expect("expectation is an object")
        .remove("expected");
    compile_validator(&expectation_schema)
        .validate(&production_expectation)
        .expect("production expectation does not require a fixture oracle");

    let package_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-package-signature-v1.schema.json"))
            .expect("read package signature schema"),
    )
    .expect("package signature schema is valid JSON");
    let package_validator = compile_validator(&package_schema);
    let mut observed_non_slsa = contract["package_signature"].clone();
    observed_non_slsa["provenance"]["statement"]["value"]["predicateType"] =
        serde_json::json!("https://example.test/other");
    package_validator
        .validate(&observed_non_slsa)
        .expect("package input accepts a bounded absolute non-SLSA predicate URI");
    observed_non_slsa["provenance"]["statement"]["value"]["predicateType"] =
        serde_json::json!("not-an-absolute-uri");
    assert!(
        !package_validator.is_valid(&observed_non_slsa),
        "package input rejects a relative predicate identifier"
    );

    let verification_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_dir().join("axiom-package-verification-v1.schema.json"))
            .expect("read package verification result schema"),
    )
    .expect("package verification result schema is valid JSON");
    let validator = compile_validator(&verification_schema);
    let mut implemented_trusted = contract["verification"].clone();
    implemented_trusted["contract_status"] = serde_json::json!("implemented");
    validator
        .validate(&implemented_trusted)
        .expect("implemented trusted result retains complete evidence");

    let rejected_partial = serde_json::json!({
        "schema_version": "axiom.package_verification.v1",
        "contract": "package.verification",
        "contract_status": "implemented",
        "decision": "rejected",
        "primary_reason_code": "OFFLINE_INPUT_MISSING",
        "reason_codes": ["OFFLINE_INPUT_MISSING"],
        "observed": {
            "registry_identity": "axiom-registry-production"
        },
        "signers": [],
        "archive": null,
        "manifest_digest": null,
        "provenance": null,
        "trust": {
            "package_threshold": 0,
            "package_valid_signers": 0,
            "index_threshold": 0,
            "index_valid_signers": 0
        }
    });
    validator
        .validate(&rejected_partial)
        .expect("rejected result may expose only the evidence available");

    let rejected_unavailable = serde_json::json!({
        "schema_version": "axiom.package_verification.v1",
        "contract": "package.verification",
        "contract_status": "implemented",
        "decision": "rejected",
        "primary_reason_code": "OFFLINE_INPUT_MISSING",
        "reason_codes": ["OFFLINE_INPUT_MISSING"],
        "observed": {
            "registry_identity": null,
            "source_identity": null,
            "namespace": null,
            "name": null,
            "version": null,
            "target_path": null,
            "publisher_identity": null
        },
        "signers": [],
        "archive": {},
        "manifest_digest": {},
        "provenance": {},
        "trust": {
            "root_version": null,
            "root_sequence": null,
            "root_transition_from": null,
            "index_generation": 0,
            "index_sequence": 0,
            "package_threshold": 0,
            "package_valid_signers": 0,
            "index_threshold": null,
            "index_valid_signers": 0,
            "offline_mode": null,
            "network_fallback": null,
            "consistent_snapshot": null
        }
    });
    validator
        .validate(&rejected_unavailable)
        .expect("rejected result may mark all unavailable evidence explicitly");

    let mut trusted_missing = implemented_trusted.clone();
    trusted_missing
        .as_object_mut()
        .expect("verification is an object")
        .remove("archive");
    assert!(
        !validator.is_valid(&trusted_missing),
        "trusted results cannot omit evidence"
    );

    let mut trusted_zero_signers = implemented_trusted.clone();
    trusted_zero_signers["signers"] = serde_json::json!([]);
    assert!(
        !validator.is_valid(&trusted_zero_signers),
        "trusted results require at least one signer"
    );

    let mut trusted_zero_counts = implemented_trusted.clone();
    trusted_zero_counts["trust"]["package_threshold"] = serde_json::json!(0);
    trusted_zero_counts["trust"]["package_valid_signers"] = serde_json::json!(0);
    assert!(
        !validator.is_valid(&trusted_zero_counts),
        "trusted results require nonzero thresholds and valid signer counts"
    );

    let mut rejected_non_slsa = implemented_trusted.clone();
    rejected_non_slsa["decision"] = serde_json::json!("rejected");
    rejected_non_slsa["primary_reason_code"] = serde_json::json!("PROVENANCE_PREDICATE_MISMATCH");
    rejected_non_slsa["reason_codes"] = serde_json::json!(["PROVENANCE_PREDICATE_MISMATCH"]);
    rejected_non_slsa["provenance"]["statement"]["value"]["predicateType"] =
        serde_json::json!("https://example.test/other");
    validator
        .validate(&rejected_non_slsa)
        .expect("rejected results preserve a bounded observed predicate URI");

    let mut trusted_non_slsa = implemented_trusted;
    trusted_non_slsa["provenance"]["statement"]["value"]["predicateType"] =
        serde_json::json!("https://example.test/other");
    assert!(
        !validator.is_valid(&trusted_non_slsa),
        "trusted results retain the expected SLSA v1 predicate"
    );

    let mut rejected_unknown = rejected_partial;
    rejected_unknown["observed"]["legacy_hmac"] = serde_json::json!("forbidden");
    assert!(
        !validator.is_valid(&rejected_unknown),
        "partial rejected evidence remains closed to unknown fields"
    );
}
