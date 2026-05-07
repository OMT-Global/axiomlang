use axiomc::json_contract;
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
}
