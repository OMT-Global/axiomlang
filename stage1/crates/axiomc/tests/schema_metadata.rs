use axiomc::{json_contract, manifest::KNOWN_CAPABILITIES};
use jsonschema::Validator;
use serde_json::Value;
use std::fs;
use std::path::Path;

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
    result["results"][0]["delivered_head_sha"] =
        serde_json::json!("not-a-commit");
    assert!(
        !result_validator.is_valid(&result),
        "result evidence cannot omit a valid delivered head binding"
    );

    assert_eq!(plan["allOf"][0]["then"]["properties"]["requirements"]["minItems"], 1);
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
    assert_eq!(policy["properties"]["paths"]["properties"]["follow_symlinks"]["const"], false);
    assert_eq!(policy["properties"]["commands"]["properties"]["allow_shell"]["const"], false);
    assert_eq!(policy["properties"]["delivery"]["properties"]["allow_force_push"]["const"], false);
    assert_eq!(policy["properties"]["delivery"]["properties"]["allow_self_approval"]["const"], false);
    assert_eq!(policy["properties"]["delivery"]["properties"]["allow_policy_edits"]["const"], false);

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
        "files": [{
            "path": "src/main.ax",
            "changed": true,
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
        &fs::read_to_string(schema_dir().join("axiom-doc-v0.schema.json"))
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
        "https://axiom.omt.global/schemas/axiom-doc-v0.schema.json"
    );
    assert_eq!(doc_schema["properties"]["command"]["const"], "doc");
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
    for field in [
        "kind",
        "stderr",
        "expected_error",
        "capabilities",
        "package",
    ] {
        assert!(
            test_target[field].is_object(),
            "manifest schema includes tests[].{field}"
        );
    }

    let manifest_capabilities = &manifest_schema["properties"]["capabilities"]["properties"];
    for field in ["deny_by_default", "unsafe_opt_ins", "owners", "rationale"] {
        assert!(
            manifest_capabilities[field].is_object(),
            "manifest schema includes capabilities.{field}"
        );
    }

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
