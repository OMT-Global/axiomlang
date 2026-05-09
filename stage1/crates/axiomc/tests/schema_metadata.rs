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
    let commands = compiler_schema["properties"]["command"]["enum"]
        .as_array()
        .expect("compiler schema command enum");
    for command in ["check", "build", "test", "caps", "bench", "repl"] {
        assert!(
            commands.iter().any(|value| value == command),
            "compiler schema includes {command} command envelopes"
        );
    }
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
            manifest_unsafe_opt_ins.iter().any(|value| value == capability),
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
