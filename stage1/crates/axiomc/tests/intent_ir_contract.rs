use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[test]
fn agent_native_authorize_emits_schema_valid_byte_stable_intent_ir() {
    let project = example("agent_native_authorize");
    let first = inspect_intent(&project);
    let second = inspect_intent(&project);
    let relative = inspect_intent_from(
        project.parent().expect("examples directory"),
        Path::new("agent_native_authorize"),
    );

    assert_eq!(
        first.stdout, second.stdout,
        "unchanged Intent IR must be byte-stable"
    );
    assert_eq!(
        first.stdout, relative.stdout,
        "absolute and relative invocation paths must emit identical Intent IR"
    );
    let document = parse_and_validate(&first);
    assert_eq!(document["schema_version"], "axiom.intent_ir.v0");

    let kinds = document["nodes"]
        .as_array()
        .expect("Intent IR nodes")
        .iter()
        .filter_map(|node| node["kind"].as_str())
        .collect::<BTreeSet<_>>();
    for required in [
        "Package",
        "Module",
        "Function",
        "Capability",
        "Effect",
        "Axiom",
        "Evidence",
        "Artifact",
        "RuntimeSurface",
    ] {
        assert!(
            kinds.contains(required),
            "agent-native fixture must emit {required}"
        );
    }
    assert!(
        document["nodes"]
            .as_array()
            .expect("Intent IR nodes")
            .iter()
            .filter(|node| matches!(node["kind"].as_str(), Some("Function" | "Type")))
            .all(|node| node["metadata"]["visibility"].is_string()),
        "function and type nodes must expose visibility for public-contract impact mapping"
    );

    assert_contract_traceability(&document);
    assert_axiom_neutral(&first.stdout);
}

#[test]
fn workspace_emits_multiple_packages_and_dependency_nodes() {
    let project = example("workspace");
    let first = inspect_intent(&project);
    let second = inspect_intent(&project);

    assert_eq!(
        first.stdout, second.stdout,
        "workspace Intent IR must be byte-stable"
    );
    let document = parse_and_validate(&first);
    let nodes = document["nodes"].as_array().expect("Intent IR nodes");
    assert!(
        nodes
            .iter()
            .filter(|node| node["kind"] == "Package")
            .count()
            >= 3,
        "workspace root and both member packages must be represented"
    );
    assert!(
        nodes.iter().any(|node| node["kind"] == "Dependency"),
        "workspace dependency must be represented"
    );
    assert_contract_traceability(&document);
    assert_axiom_neutral(&first.stdout);
}

#[test]
fn incomplete_module_is_an_explicit_traceable_diagnostic() {
    let temp = tempfile::tempdir().expect("temporary package");
    fs::create_dir(temp.path().join("src")).expect("create source directory");
    fs::write(
        temp.path().join("axiom.toml"),
        r#"[package]
name = "incomplete-intent"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"
"#,
    )
    .expect("write manifest");
    fs::write(temp.path().join("src/main.ax"), "fn incomplete( {\n")
        .expect("write malformed source");

    let output = inspect_intent(temp.path());
    let document = parse_and_validate(&output);
    let diagnostics = document["diagnostics"]
        .as_array()
        .expect("Intent IR diagnostics");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["code"] == "intent_ir_incomplete_module"),
        "malformed input must be reported instead of silently omitted"
    );
    assert_contract_traceability(&document);
}

#[test]
fn semantic_consumers_reuse_canonical_node_ids() {
    let project = example("agent_native_authorize");
    let intent = parse_and_validate(&inspect_intent(&project));
    let node_ids = intent["nodes"]
        .as_array()
        .expect("Intent IR nodes")
        .iter()
        .map(|node| node["id"].as_str().expect("node id"))
        .collect::<BTreeSet<_>>();

    let repair = run_json(&["repair-plan", project.to_str().unwrap(), "--json"]);
    for task in repair["tasks"].as_array().expect("repair tasks") {
        assert!(node_ids.contains(task["target_node"].as_str().expect("repair target")));
    }
    let evidence = run_json(&["evidence", project.to_str().unwrap(), "--json"]);
    for item in evidence["evidence"].as_array().expect("evidence items") {
        assert!(node_ids.contains(item["target"].as_str().expect("evidence target")));
    }
    let artifacts = run_json(&["inspect", "artifacts", project.to_str().unwrap(), "--json"]);
    for artifact in artifacts["artifacts"].as_array().expect("artifacts") {
        for source in artifact["generated_from"].as_array().expect("artifact sources") {
            assert!(node_ids.contains(source.as_str().expect("artifact source")));
        }
    }
    let verification = run_json(&["verify", project.to_str().unwrap(), "--json"]);
    for axiom in verification["axioms"].as_array().expect("verified axioms") {
        assert!(node_ids.contains(axiom["id"].as_str().expect("axiom id")));
        for backing in axiom["backing_evidence"].as_array().expect("backing evidence") {
            assert!(node_ids.contains(backing["semantic_node"].as_str().expect("evidence node")));
        }
    }
}

#[test]
fn declared_types_and_decisions_emit_their_node_families() {
    for (fixture, expected_kind) in [("structs", "Type"), ("decision_records", "Decision")] {
        let document = parse_and_validate(&inspect_intent(&example(fixture)));
        assert!(
            document["nodes"]
                .as_array()
                .expect("Intent IR nodes")
                .iter()
                .any(|node| node["kind"] == expected_kind),
            "{fixture} must emit {expected_kind}"
        );
        assert_contract_traceability(&document);
    }
}

fn assert_contract_traceability(document: &Value) {
    let nodes = document["nodes"].as_array().expect("Intent IR nodes");
    let node_ids = nodes
        .iter()
        .map(|node| node["id"].as_str().expect("node id"))
        .collect::<BTreeSet<_>>();
    let edges = document["edges"].as_array().expect("Intent IR edges");

    for edge in edges {
        let from = edge["from"].as_str().expect("edge from");
        let to = edge["to"].as_str().expect("edge to");
        assert!(node_ids.contains(from), "edge source {from} must exist");
        assert!(node_ids.contains(to), "edge target {to} must exist");
    }
    for artifact in nodes.iter().filter(|node| node["kind"] == "Artifact") {
        let id = artifact["id"].as_str().expect("artifact id");
        assert!(
            edges.iter().any(|edge| {
                edge["from"] == id
                    && matches!(edge["kind"].as_str(), Some("generated_from" | "implements"))
            }),
            "artifact {id} must trace to a semantic node"
        );
    }
    for diagnostic in document["diagnostics"]
        .as_array()
        .expect("Intent IR diagnostics")
    {
        for node_id in diagnostic["node_ids"]
            .as_array()
            .expect("diagnostic node ids")
        {
            let node_id = node_id.as_str().expect("diagnostic node id");
            assert!(
                node_ids.contains(node_id),
                "diagnostic target {node_id} must exist"
            );
        }
    }

    for input in document["provenance"]["inputs"]
        .as_array()
        .expect("provenance inputs")
    {
        let path = input["path"].as_str().expect("provenance path");
        assert!(
            !Path::new(path).is_absolute(),
            "provenance path must be relative: {path}"
        );
        assert!(!path.split('/').any(|segment| segment == ".."));
        let package = input["package"].as_str().expect("provenance package");
        assert!(
            node_ids.contains(package),
            "provenance package {package} must exist"
        );
    }
}

fn assert_axiom_neutral(bytes: &[u8]) {
    let serialized = String::from_utf8_lossy(bytes).to_ascii_lowercase();
    for forbidden in ["cargo", "cranelift", "rustc", "rust::", ".rs\""] {
        assert!(
            !serialized.contains(forbidden),
            "Intent IR captured host term {forbidden}"
        );
    }
}

fn parse_and_validate(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "inspect intent failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "JSON mode must not write stderr");
    let document: Value = serde_json::from_slice(&output.stdout).expect("parse Intent IR JSON");
    let schema: Value =
        serde_json::from_str(&fs::read_to_string(schema_path()).expect("read Intent IR schema"))
            .expect("parse Intent IR schema");
    let validator = jsonschema::validator_for(&schema).expect("compile Intent IR schema");
    if let Err(error) = validator.validate(&document) {
        panic!("Intent IR failed schema validation: {error}");
    }
    document
}

fn inspect_intent(project: &Path) -> Output {
    inspect_intent_from(Path::new(env!("CARGO_MANIFEST_DIR")), project)
}

fn inspect_intent_from(cwd: &Path, project: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .current_dir(cwd)
        .args([
            "inspect",
            "intent",
            project.to_str().expect("UTF-8 fixture path"),
            "--json",
        ])
        .output()
        .expect("run axiomc inspect intent")
}

fn run_json(args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(args)
        .output()
        .expect("run semantic consumer");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!("consumer JSON failed: {error}: {}", String::from_utf8_lossy(&output.stderr))
    })
}

fn example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name)
}

fn schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas/axiom-intent-ir-v0.schema.json")
}
