use axiomc::{json_contract, manifest::KNOWN_CAPABILITIES};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn schema_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("schemas")
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
        "fixture maps the current generated-Rust backend"
    );
    assert!(
        ids.contains(&"axiom://target/stage1-direct-native"),
        "fixture maps the direct-native backend roadmap"
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
