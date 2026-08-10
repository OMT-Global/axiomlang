use axiomc::{diagnostics::Diagnostic, json_contract};
use jsonschema::Validator;
use serde_json::{Map, Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn cli_json_outputs_match_checked_in_contract_snapshots() {
    let contracts = contract_root();
    let schema = read_json(&contracts.join("schemas/axiom.stage1.command.schema.json"));
    let validator = jsonschema::validator_for(&schema).expect("compile JSON contract schema");
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("contract-app");

    run_axiomc(&[
        "new",
        project.to_str().expect("project path"),
        "--name",
        "contract-app",
    ]);

    let mutation_input = temp.path().join("mutation-survivors.json");
    fs::write(
        &mutation_input,
        r#"{"survivors":[{"id":"m1","file":"src/main.ax","function":"main","line":1,"mutator":"replace_literal","description":"changed greeting","status":"survived"}]}"#,
    )
    .expect("write mutation input");
    let project_str = project.to_str().expect("project path");
    let mutation_input_str = mutation_input.to_str().expect("mutation input path");
    let invocations: [(&str, Vec<&str>); 6] = [
        ("check", vec!["check", project_str, "--json"]),
        ("build", vec!["build", project_str, "--json"]),
        ("test", vec!["test", project_str, "--json"]),
        ("caps", vec!["caps", project_str, "--json"]),
        ("run", vec!["run", project_str, "--json"]),
        (
            "mutation-report",
            vec!["mutation-report", mutation_input_str, "--json"],
        ),
    ];

    for (command, args) in invocations {
        let output = run_axiomc_json(&args);
        assert_payload_matches_schema(&validator, command, &output);

        let normalized = normalize_payload(output, &project);
        let snapshot = read_json(&contracts.join(format!("snapshots/{command}.json")));
        assert_eq!(normalized, snapshot, "{command} JSON contract drifted");
    }
}

#[test]
fn command_failure_envelopes_validate_against_the_published_schema() {
    let schema = read_json(&contract_root().join("schemas/axiom.stage1.command.schema.json"));
    let validator = jsonschema::validator_for(&schema).expect("compile JSON contract schema");
    let diagnostic = Diagnostic::new("manifest", "fixture failure");

    for command in [
        "check",
        "build",
        "run",
        "test",
        "caps",
        "mutation-report",
        "parse",
        "doc",
        "lsp",
    ] {
        let payload = json_contract::error(command, &diagnostic);
        validator
            .validate(&payload)
            .unwrap_or_else(|error| panic!("{command} failure envelope must validate: {error}"));
    }

    for (group, name) in [
        ("check", "parse-error.json"),
        ("build", "runtime-lowering-required.json"),
        ("doc", "failure.json"),
    ] {
        let payload = read_json(
            &contract_root()
                .parent()
                .expect("stage1 root")
                .join("json-fixtures")
                .join(group)
                .join(name),
        );
        validator
            .validate(&payload)
            .unwrap_or_else(|error| panic!("{group}/{name} must validate: {error}"));
    }
}

#[test]
fn command_failure_envelopes_reject_unrelated_commands_and_fields() {
    let schema = read_json(&contract_root().join("schemas/axiom.stage1.command.schema.json"));
    let validator = jsonschema::validator_for(&schema).expect("compile JSON contract schema");
    let diagnostic = Diagnostic::new("manifest", "fixture failure");

    let mut unrelated = json_contract::error("doctor", &diagnostic);
    assert!(
        !validator.is_valid(&unrelated),
        "failure envelope must reject commands outside the published command branches"
    );

    unrelated["command"] = serde_json::json!("check");
    unrelated["unexpected"] = serde_json::json!(true);
    assert!(
        !validator.is_valid(&unrelated),
        "failure envelope must reject unrelated fields"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_build_json_validates_against_command_schema() {
    if which::which("cc").is_err() {
        eprintln!("skipping Cranelift build JSON schema test because cc is unavailable");
        return;
    }

    let contracts = contract_root();
    let schema = read_json(&contracts.join("schemas/axiom.stage1.command.schema.json"));
    let validator = jsonschema::validator_for(&schema).expect("compile JSON contract schema");
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("cranelift-contract-app");

    run_axiomc(&[
        "new",
        project.to_str().expect("project path"),
        "--name",
        "cranelift-contract-app",
    ]);

    let output = run_axiomc_json(&[
        "build",
        project.to_str().expect("project path"),
        "--backend",
        "cranelift",
        "--json",
    ]);
    assert!(output["generated_rust"].is_null());
    assert_payload_matches_schema(&validator, "cranelift build", &output);
}

#[cfg(not(windows))]
#[test]
fn real_test_case_results_validate_against_both_stage1_schemas() {
    if which::which("cc").is_err() {
        eprintln!("skipping test-case JSON schema test because cc is unavailable");
        return;
    }

    let command_schema = read_json(
        &contract_root().join("schemas/axiom.stage1.command.schema.json"),
    );
    let command_validator =
        jsonschema::validator_for(&command_schema).expect("compile command schema");
    let public_schema = read_json(&public_v1_schema_path());
    let public_validator =
        jsonschema::validator_for(&public_schema).expect("compile public stage1 schema");

    let temp = tempfile::tempdir().expect("tempdir");
    let success_project = temp.path().join("test-case-success");
    run_axiomc(&[
        "new",
        success_project.to_str().expect("success project path"),
        "--name",
        "test-case-success",
    ]);
    let success = run_axiomc_json(&[
        "test",
        success_project.to_str().expect("success project path"),
        "--backend",
        "cranelift",
        "--json",
    ]);

    let (blocked_status, blocked) = run_axiomc_json_with_status(&[
        "test",
        repository_root()
            .join("stage1/examples/proof_worker")
            .to_str()
            .expect("proof worker path"),
        "--backend",
        "cranelift",
        "--json",
    ]);
    assert!(!blocked_status, "proof worker must fail closed");
    assert!(blocked["cases"][0]["binary"].is_null());
    assert_eq!(
        blocked["cases"][0]["lowering"]["execution_mode"],
        "not_produced"
    );

    let (expected_status, expected_failure) = run_axiomc_json_with_status(&[
        "test",
        repository_root()
            .join("stage1/conformance/pass/net_tcp_echo")
            .to_str()
            .expect("expected build-failure path"),
        "--backend",
        "cranelift",
        "--json",
    ]);
    assert!(expected_status, "matching expected build failure must pass");
    assert!(expected_failure["cases"][0]["expected_error"].is_object());
    assert!(expected_failure["cases"][0]["binary"].is_null());
    assert!(expected_failure["cases"][0]["exit_code"].is_null());

    for (label, payload) in [
        ("successful test case", success),
        ("blocked test case", blocked),
        ("expected build-failure case", expected_failure),
    ] {
        assert_payload_matches_schema(&command_validator, label, &payload);
        assert_payload_matches_schema(&public_validator, label, &payload);
    }
}

#[cfg(not(windows))]
#[test]
fn cranelift_debug_build_emits_direct_native_debug_sidecars() {
    if which::which("cc").is_err() {
        eprintln!("skipping Cranelift debug build test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("cranelift-debug-contract-app");

    run_axiomc(&[
        "new",
        project.to_str().expect("project path"),
        "--name",
        "cranelift-debug-contract-app",
    ]);

    let output = run_axiomc_json(&[
        "build",
        project.to_str().expect("project path"),
        "--backend",
        "cranelift",
        "--debug",
        "--json",
    ]);

    assert!(output["generated_rust"].is_null());
    let debug_map_path = output["debug_map"]
        .as_str()
        .expect("cranelift debug build should emit debug_map");
    let debug_manifest_path = output["debug_manifest"]
        .as_str()
        .expect("cranelift debug build should emit debug_manifest");

    let debug_map = read_json(Path::new(debug_map_path));
    assert_eq!(
        debug_map["schema_version"],
        "axiom.stage1.direct_native.debug_map.v1"
    );
    assert!(
        debug_map["binary"]
            .as_str()
            .is_some_and(|path| path.contains("cranelift-debug-contract-app"))
    );

    let debug_manifest = read_json(Path::new(debug_manifest_path));
    assert_eq!(
        debug_manifest["schema_version"],
        "axiom.stage1.direct_native.debug_manifest.v1"
    );
    assert_eq!(debug_manifest["artifact_class"], "native_binary");
    assert!(debug_manifest.get("generated_rust").is_none());
    assert!(debug_manifest.get("generated_rust_hash").is_none());
    assert!(debug_manifest.get("rustc").is_none());
}

#[test]
fn debug_map_sidecar_matches_checked_in_contract_snapshot() {
    let contracts = contract_root();
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("debug-map-contract");

    run_axiomc(&[
        "new",
        project.to_str().expect("project path"),
        "--name",
        "debug-map-contract",
    ]);
    fs::write(
        project.join("src/helper.ax"),
        "pub fn helper(): int {\nreturn 7\n}\n",
    )
    .expect("write helper source");
    fs::write(
        project.join("src/main.ax"),
        "import \"helper.ax\"\nlet answer: int = helper()\nprint answer\n",
    )
    .expect("write main source");

    let build = run_axiomc_json(&[
        "build",
        project.to_str().expect("project path"),
        "--debug",
        "--json",
    ]);
    let debug_map_path = build["debug_map"]
        .as_str()
        .expect("build payload debug_map path");
    let debug_map = read_json(Path::new(debug_map_path));
    let normalized = normalize_payload(debug_map, &project);
    let snapshot = read_json(&contracts.join("snapshots/debug-map.json"));

    assert_eq!(normalized, snapshot, "debug map sidecar drifted");
}

#[test]
fn command_schema_validates_all_build_lowering_evidence_tuples() {
    let contracts = contract_root();
    let schema = read_json(&contracts.join("schemas/axiom.stage1.command.schema.json"));
    let validator = jsonschema::validator_for(&schema).expect("compile JSON command schema");

    let tuples: [(&str, Value, Value); 5] = [
        (
            "direct_native_runtime",
            json!({
                "schema_version": "axiom.build-lowering-evidence.v1",
                "execution_mode": "direct_native_runtime",
                "lowering_mode": "direct_native_runtime",
                "direct_native_runtime": true,
                "known_value_static_folds": false,
                "legacy_fallback_attempted": false,
            }),
            json!({
                "schema_version": "axiom.build-lowering-evidence.v1",
                "execution_mode": "direct_native_runtime",
                "lowering_mode": "direct_native_runtime",
                "direct_native_runtime": false,
                "known_value_static_folds": false,
                "legacy_fallback_attempted": false,
            }),
        ),
        (
            "direct_native_runtime_with_static_folds",
            json!({
                "schema_version": "axiom.build-lowering-evidence.v1",
                "execution_mode": "direct_native_runtime",
                "lowering_mode": "direct_native_runtime_with_static_folds",
                "direct_native_runtime": true,
                "known_value_static_folds": true,
                "legacy_fallback_attempted": false,
            }),
            json!({
                "schema_version": "axiom.build-lowering-evidence.v1",
                "execution_mode": "direct_native_runtime",
                "lowering_mode": "direct_native_runtime_with_static_folds",
                "direct_native_runtime": false,
                "known_value_static_folds": true,
                "legacy_fallback_attempted": false,
            }),
        ),
        (
            "bounded_static_output",
            json!({
                "schema_version": "axiom.build-lowering-evidence.v1",
                "execution_mode": "bounded_static_output",
                "lowering_mode": "bounded_static_output",
                "direct_native_runtime": false,
                "known_value_static_folds": true,
                "legacy_fallback_attempted": false,
            }),
            json!({
                "schema_version": "axiom.build-lowering-evidence.v1",
                "execution_mode": "bounded_static_output",
                "lowering_mode": "bounded_static_output",
                "direct_native_runtime": true,
                "known_value_static_folds": true,
                "legacy_fallback_attempted": false,
            }),
        ),
        (
            "generated_rust_runtime",
            json!({
                "schema_version": "axiom.build-lowering-evidence.v1",
                "execution_mode": "generated_rust_runtime",
                "lowering_mode": "generated_rust_compatibility",
                "direct_native_runtime": false,
                "known_value_static_folds": false,
                "legacy_fallback_attempted": false,
            }),
            json!({
                "schema_version": "axiom.build-lowering-evidence.v1",
                "execution_mode": "generated_rust_runtime",
                "lowering_mode": "generated_rust_compatibility",
                "direct_native_runtime": false,
                "known_value_static_folds": true,
                "legacy_fallback_attempted": false,
            }),
        ),
        (
            "runtime_lowering_required",
            json!({
                "schema_version": "axiom.build-lowering-evidence.v1",
                "execution_mode": "not_produced",
                "lowering_mode": "runtime_lowering_required",
                "direct_native_runtime": false,
                "known_value_static_folds": false,
                "legacy_fallback_attempted": true,
            }),
            json!({
                "schema_version": "axiom.build-lowering-evidence.v1",
                "execution_mode": "not_produced",
                "lowering_mode": "runtime_lowering_required",
                "direct_native_runtime": false,
                "known_value_static_folds": true,
                "legacy_fallback_attempted": true,
            }),
        ),
    ];

    for (mode, valid, invalid) in tuples {
        let valid_payload = test_command_payload_with_lowering(valid);
        assert_payload_matches_schema(
            &validator,
            &format!("build lowering tuple {mode}"),
            &valid_payload,
        );

        let invalid_payload = test_command_payload_with_lowering(invalid);
        assert!(
            validator.validate(&invalid_payload).is_err(),
            "invalid build lowering tuple {mode} should be rejected"
        );
    }
}

#[test]
fn command_schema_accepts_real_fail_closed_test_case_shapes() {
    let contracts = contract_root();
    let schema = read_json(&contracts.join("schemas/axiom.stage1.command.schema.json"));
    let validator = jsonschema::validator_for(&schema).expect("compile JSON command schema");
    let compile_fail_fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/fail/comparison_predictable_diagnostic")
        .canonicalize()
        .expect("compile-fail fixture");
    let compile_fail = run_axiomc_json(&[
        "test",
        compile_fail_fixture
            .to_str()
            .expect("compile-fail fixture path"),
        "--json",
    ]);
    let compile_fail_case = &compile_fail["cases"][0];
    assert!(compile_fail_case["binary"].is_null());
    assert!(compile_fail_case["exit_code"].is_null());
    assert!(compile_fail_case["expected_stdout"].is_null());
    assert!(compile_fail_case["expected_stderr"].is_null());
    assert!(compile_fail_case["expected_error"].is_object());
    let focused_test_schema = json!({
        "$schema": schema["$schema"],
        "$defs": schema["$defs"],
        "$ref": "#/$defs/test",
    });
    let focused_test_validator =
        jsonschema::validator_for(&focused_test_schema).expect("compile focused test schema");
    assert_payload_matches_schema(
        &focused_test_validator,
        "compile-fail test branch",
        &compile_fail,
    );
    assert_payload_matches_schema(&validator, "compile-fail test", &compile_fail);

    let mut missing_nullable_field = compile_fail.clone();
    missing_nullable_field["cases"][0]
        .as_object_mut()
        .expect("compile-fail case object")
        .remove("binary");
    assert!(
        validator.validate(&missing_nullable_field).is_err(),
        "required nullable test-case fields must not become optional"
    );

    let mut unexpected_expected_error_field = compile_fail.clone();
    unexpected_expected_error_field["cases"][0]["expected_error"]["unexpected"] = json!(true);
    assert!(
        validator
            .validate(&unexpected_expected_error_field)
            .is_err(),
        "expected diagnostic objects must remain closed"
    );

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("failing-test-contract");
    run_axiomc(&[
        "new",
        project.to_str().expect("project path"),
        "--name",
        "failing-test-contract",
    ]);
    fs::write(
        project.join("src/main_test.ax"),
        "let value: int = \"not an int\"\n",
    )
    .expect("write invalid test source");
    let failed = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(["test", project.to_str().expect("project path"), "--json"])
        .output()
        .expect("run failing test command");
    assert!(
        !failed.status.success(),
        "invalid test source must fail closed"
    );
    assert!(
        failed.stderr.is_empty(),
        "JSON test failure should not use stderr: {}",
        String::from_utf8_lossy(&failed.stderr)
    );
    let failed: Value = serde_json::from_slice(&failed.stdout).expect("parse failing test JSON");
    let failed_case = &failed["cases"][0];
    assert!(failed_case["binary"].is_null());
    assert!(failed_case["exit_code"].is_null());
    assert!(failed_case["error"].is_object());
    assert_payload_matches_schema(&validator, "failed test", &failed);

    let mut legacy_string_error = failed;
    legacy_string_error["cases"][0]["error"] = json!("unstructured compiler failure");
    assert!(
        validator.validate(&legacy_string_error).is_err(),
        "test-case errors must remain structured diagnostics"
    );
}

#[test]
fn public_schema_rejects_nested_only_case_lowering_contradictions() {
    let schema = read_json(&public_v1_schema_path());
    let validator = jsonschema::validator_for(&schema).expect("compile public v1 schema");
    let valid_lowering = json!({
        "schema_version": "axiom.build-lowering-evidence.v1",
        "execution_mode": "direct_native_runtime",
        "lowering_mode": "direct_native_runtime",
        "direct_native_runtime": true,
        "known_value_static_folds": false,
        "legacy_fallback_attempted": false,
    });
    let valid = test_command_payload_with_lowering(valid_lowering);
    assert_payload_matches_schema(&validator, "valid nested test lowering", &valid);

    let mut contradiction = valid;
    contradiction["cases"][0]["lowering"]["legacy_fallback_attempted"] = json!(true);
    assert!(
        validator.validate(&contradiction).is_err(),
        "public stage1 schema must reject nested-only contradictory test-case evidence"
    );
}

#[test]
fn cli_json_outputs_validate_against_public_v1_schema() {
    let schema = read_json(&public_v1_schema_path());
    let validator = jsonschema::validator_for(&schema).expect("compile public v1 schema");
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("contract-app");

    run_axiomc(&[
        "new",
        project.to_str().expect("project path"),
        "--name",
        "contract-app",
    ]);

    let project_str = project.to_str().expect("project path");
    let mutation_input = temp.path().join("mutation-survivors.json");
    fs::write(
        &mutation_input,
        r#"{"survivors":[{"id":"m1","file":"src/main.ax","function":"main","line":1,"mutator":"replace_literal","description":"changed greeting","status":"survived"}]}"#,
    )
    .expect("write mutation input");
    let mutation_input_str = mutation_input.to_str().expect("mutation input path");
    let doc_out = project.join("docs/api");
    let doc_out_str = doc_out.to_str().expect("doc output path");
    let invocations: [(&str, Vec<&str>); 9] = [
        ("check", vec!["check", project_str, "--json"]),
        ("build", vec!["build", project_str, "--json"]),
        ("test", vec!["test", project_str, "--json"]),
        ("caps", vec!["caps", project_str, "--json"]),
        ("parse", vec!["parse", project_str, "--json"]),
        ("fmt", vec!["fmt", project_str, "--check", "--json"]),
        (
            "doc",
            vec!["doc", project_str, "--out-dir", doc_out_str, "--json"],
        ),
        ("run", vec!["run", project_str, "--json"]),
        (
            "mutation-report",
            vec!["mutation-report", mutation_input_str, "--json"],
        ),
    ];

    for (label, args) in invocations {
        let output = run_axiomc_json(&args);
        assert_payload_matches_schema(&validator, label, &output);
        assert_eq!(
            output["schema_version"], "axiom.stage1.v1",
            "{label} did not declare axiom.stage1.v1"
        );
        assert!(
            output.get("ok").is_some(),
            "{label} payload missing required `ok` field"
        );
        assert_eq!(
            output["command"]
                .as_str()
                .map(|s| s.split(' ').next().unwrap_or(s)),
            Some(label),
            "{label} payload command field drifted"
        );
    }
}

#[test]
fn doc_json_output_validates_against_doc_schema() {
    let public_schema = read_json(&public_v1_schema_path());
    let public_validator =
        jsonschema::validator_for(&public_schema).expect("compile public v1 schema");
    let doc_schema = read_json(&doc_schema_path());
    let doc_validator = jsonschema::validator_for(&doc_schema).expect("compile doc schema");
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("doc-json");
    fs::create_dir_all(project.join("src")).expect("mkdir");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"doc-json\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nenv = [\"AXIOM_ENV\"]\n",
    )
    .expect("write manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"doc-json\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write lock");
    fs::write(
        project.join("src/main.ax"),
        "/// Handles a request.\n/// Example: route(\"/health\")\npub fn route(path: string): string {\nreturn \"ok\"\n}\n\n/// Response envelope.\npub struct Response {\nstatus: int\n}\n",
    )
    .expect("write source");

    let out_dir = project.join("docs/api");
    let output = run_axiomc_json(&[
        "doc",
        project.to_str().expect("project path"),
        "--out-dir",
        out_dir.to_str().expect("out dir"),
        "--json",
    ]);

    assert_payload_matches_schema(&public_validator, "doc", &output);
    assert_payload_matches_schema(&doc_validator, "doc", &output);
    assert_eq!(output["command"], "doc");
    assert_eq!(output["functions"].as_array().expect("functions").len(), 1);
    assert_eq!(output["types"].as_array().expect("types").len(), 1);
    assert_eq!(output["functions"][0]["kind"], "function");
    assert_eq!(output["types"][0]["kind"], "struct");
    assert_eq!(output["items"][0]["kind"], "function");
    assert_eq!(output["items"][0]["examples"][0], "route(\"/health\")");
    assert!(
        output["capabilities"]
            .as_array()
            .expect("capabilities array")
            .iter()
            .any(|capability| capability["name"] == "env")
    );
}

#[test]
fn doc_md_output_matches_checked_in_golden() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("doc-md");
    fs::create_dir_all(project.join("src")).expect("mkdir");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"doc-md\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n",
    )
    .expect("write manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"doc-md\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write lock");
    fs::write(
        project.join("src/main.ax"),
        "/// Handles a request.\n/// Example: route(\"/health\")\npub fn route(path: string): string {\nreturn \"ok\"\n}\n\n/// Response envelope.\npub struct Response {\nstatus: int\n}\n",
    )
    .expect("write source");

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(["doc", "--md", project.to_str().expect("project path")])
        .output()
        .expect("run axiomc doc --md");

    assert!(
        output.status.success(),
        "doc --md failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "doc --md should not emit stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("dist/docs/index.md"),
        "doc --md should report the markdown path"
    );
    let markdown =
        fs::read_to_string(project.join("dist/docs/index.md")).expect("read markdown output");
    assert!(
        !project.join("dist/docs/index.html").exists(),
        "doc --md should not write HTML output"
    );
    let normalized = markdown.replace(&project.display().to_string(), "<project>");
    let expected =
        fs::read_to_string(contract_root().join("snapshots/doc-md.md")).expect("read golden");
    assert_eq!(normalized, expected);
}

#[test]
fn inspect_graph_json_validates_against_semantic_graph_schema() {
    let schema = read_json(&semantic_graph_schema_path());
    let validator = jsonschema::validator_for(&schema).expect("compile semantic graph schema");
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("semantic-graph-app");

    run_axiomc(&[
        "new",
        project.to_str().expect("project path"),
        "--name",
        "semantic-graph-app",
    ]);

    let output = run_axiomc_json(&[
        "inspect",
        "graph",
        project.to_str().expect("project path"),
        "--json",
    ]);
    assert_payload_matches_schema(&validator, "inspect graph", &output);
}

#[test]
fn inspect_graph_json_schema_accepts_full_report_failures() {
    let schema = read_json(&semantic_graph_schema_path());
    let validator = jsonschema::validator_for(&schema).expect("compile semantic graph schema");
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("semantic-graph-invalid-lockfile-app");

    run_axiomc(&[
        "new",
        project.to_str().expect("project path"),
        "--name",
        "semantic-graph-invalid-lockfile-app",
    ]);
    fs::write(project.join("axiom.lock"), "invalid lockfile\n").expect("write invalid lockfile");

    let output = run_axiomc_json(&[
        "inspect",
        "graph",
        project.to_str().expect("project path"),
        "--json",
    ]);
    assert_eq!(output["ok"], false);
    assert_eq!(output["lockfile_status"], "invalid");
    assert_payload_matches_schema(&validator, "inspect graph", &output);
}

fn public_v1_schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/axiom.stage1.v1.schema.json")
        .canonicalize()
        .expect("public v1 schema path")
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repository root")
}

fn doc_schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/axiom.docs.v1.schema.json")
        .canonicalize()
        .expect("doc schema path")
}

fn semantic_graph_schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/axiom-semantic-graph-v0.schema.json")
        .canonicalize()
        .expect("semantic graph schema path")
}

#[test]
fn doc_json_failure_uses_error_contract() {
    let temp = tempfile::tempdir().expect("tempdir");
    let missing = temp.path().join("missing-doc-project");
    let out_dir = temp.path().join("docs");
    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "doc",
            missing.to_str().expect("missing path"),
            "--out-dir",
            out_dir.to_str().expect("out dir"),
            "--json",
        ])
        .output()
        .expect("run failing axiomc doc --json");

    assert!(
        !output.status.success(),
        "doc --json should fail for missing input"
    );
    assert!(
        output.stderr.is_empty(),
        "JSON failures should not use stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON error payload");
    assert_eq!(payload["ok"], false);
    assert_eq!(payload["command"], "doc");
    assert!(payload.get("error").is_some(), "missing JSON error object");
}

#[test]
fn typed_docs_multi_package_surface_matches_snapshot() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("docs-workspace");
    for member in ["members/alpha", "members/beta"] {
        fs::create_dir_all(project.join(member).join("src")).expect("mkdir member source");
    }
    fs::create_dir_all(project.join("src")).expect("mkdir root source");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"docs-workspace\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[workspace]\nmembers = [\"members/alpha\", \"members/beta\"]\n",
    )
    .expect("write root manifest");
    for (member, name) in [("alpha", "docs-alpha"), ("beta", "docs-beta")] {
        fs::write(
            project.join("members").join(member).join("axiom.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n"),
        )
        .expect("write member manifest");
        fs::write(
            project.join("members").join(member).join("axiom.lock"),
            format!("version = 1\n\n[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\nsource = \"path\"\n"),
        )
        .expect("write member lock");
    }
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"docs-workspace\"\nversion = \"0.1.0\"\nsource = \"path\"\n\n[[package]]\nname = \"docs-alpha\"\nversion = \"0.1.0\"\nsource = \"path:members/alpha\"\n\n[[package]]\nname = \"docs-beta\"\nversion = \"0.1.0\"\nsource = \"path:members/beta\"\n",
    )
    .expect("write lock");
    for (relative, source) in [
        ("src/main.ax", "pub fn root_api(): int {\nreturn 1\n}\n"),
        ("members/alpha/src/main.ax", "pub fn alpha_api(): int {\nreturn 2\n}\n"),
        ("members/beta/src/main.ax", "pub fn beta_api(): int {\nreturn 3\n}\n"),
    ] {
        fs::write(project.join(relative), source).expect("write source");
    }

    let output = run_axiomc_json(&[
        "doc",
        project.to_str().expect("project path"),
        "--out-dir",
        "dist/docs",
        "--json",
    ]);
    let snapshot_surface = serde_json::json!({
        "schema": output["schema"],
        "packages": output["packages"],
        "items": output["items"],
        "search": output["search"],
    });
    let snapshot = read_json(&contract_root().join("snapshots/docs-v1-multi-package.json"));
    assert_eq!(snapshot_surface, snapshot, "typed documentation surface drifted");
}

fn contract_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../compiler-contracts")
        .canonicalize()
        .expect("contract root")
}

fn run_axiomc(args: &[&str]) {
    let status = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(args)
        .status()
        .expect("run axiomc");
    assert!(status.success(), "axiomc {args:?} failed with {status}");
}

fn run_axiomc_json(args: &[&str]) -> Value {
    let (success, payload) = run_axiomc_json_with_status(args);
    assert!(success, "axiomc {args:?} failed with JSON payload {payload}");
    payload
}

fn run_axiomc_json_with_status(args: &[&str]) -> (bool, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(args)
        .output()
        .expect("run axiomc json command");
    assert!(
        output.stderr.is_empty(),
        "axiomc {args:?} wrote stderr for JSON command: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        output.status.success(),
        serde_json::from_slice(&output.stdout).expect("parse axiomc json"),
    )
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).expect("read json")).expect("parse json")
}

fn assert_payload_matches_schema(validator: &Validator, command: &str, payload: &Value) {
    let errors: Vec<_> = validator
        .iter_errors(payload)
        .map(|error| format!("{}: {error}", error.instance_path))
        .collect();
    if !errors.is_empty() {
        panic!(
            "{command} JSON payload failed schema validation:\n{}\n{payload:#}",
            errors.join("\n")
        );
    }
}

fn normalize_payload(mut payload: Value, project: &Path) -> Value {
    let aliases = vec![
        project.display().to_string(),
        project
            .canonicalize()
            .expect("canonical project path")
            .display()
            .to_string(),
    ];
    normalize_value(&mut payload, &aliases, None);
    payload
}

fn normalize_value(value: &mut Value, project_aliases: &[String], key: Option<&str>) {
    match value {
        Value::String(text) if key.is_some_and(|key| key.ends_with("_hash")) => {
            *text = "<hash>".to_string();
        }
        Value::String(text) => {
            if let Some(project) = project_aliases
                .iter()
                .find(|project| text.starts_with(*project))
            {
                *text = text.replacen(project, "<project>", 1);
            }
        }
        Value::Number(_) if matches!(key, Some("duration_ms" | "compile_ms")) => {
            *value = Value::from(0);
        }
        Value::Array(items) => {
            for item in items {
                normalize_value(item, project_aliases, None);
            }
        }
        Value::Object(map) => normalize_object(map, project_aliases),
        _ => {}
    }
}

fn normalize_object(map: &mut Map<String, Value>, project_aliases: &[String]) {
    for (key, value) in map {
        normalize_value(value, project_aliases, Some(key));
    }
}

fn test_command_payload_with_lowering(lowering: Value) -> Value {
    let case = json!({
        "name": "src/main_test",
        "kind": "unit",
        "entry": "src/main_test.ax",
        "package_root": "<project>",
        "binary": "<project>/dist/tests/contract-app-src-main-test",
        "generated_rust": null,
        "ok": true,
        "exit_code": 0,
        "stdout": "hello from stage1\n",
        "stderr": "",
        "expected_stdout": "hello from stage1\n",
        "duration_ms": 0,
        "error": null,
        "expected_stderr": null,
        "lowering": lowering
    });

    json!({
        "schema_version": "axiom.stage1.v1",
        "ok": true,
        "command": "test",
        "project": "<project>",
        "backend": "cranelift",
        "manifest": "<project>/axiom.toml",
        "packages": ["<project>"],
        "filter": null,
        "properties_only": false,
        "passed": 1,
        "failed": 0,
        "skipped": 0,
        "kinds": {"unit": 1},
        "duration_ms": 0,
        "properties": {"passed": 0, "failed": 0, "total": 1},
        "cases": [case]
    })
}
