#![cfg(not(windows))]

use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

#[test]
fn signed_addition_checks_runtime_overflow_in_debug_and_wraps_in_release() {
    if which::which("cc").is_err() {
        eprintln!("skipping direct-native numeric overflow test because cc is unavailable");
        return;
    }

    let directory = tempfile::tempdir().expect("temporary projects");
    for (ty, min, max, diagnostic_type) in [
        ("int", "-9223372036854775808", "9223372036854775807", "i64"),
        ("i64", "-9223372036854775808", "9223372036854775807", "i64"),
        (
            "isize",
            "-9223372036854775808",
            "9223372036854775807",
            "isize",
        ),
        ("i8", "-128", "127", "i8"),
        ("i32", "-2147483648", "2147483647", "i32"),
    ] {
        for lower_boundary in [false, true] {
            for debug in [false, true] {
                let sign = if lower_boundary { "lower" } else { "upper" };
                let mode = if debug { "debug" } else { "release" };
                let label = format!("signed-add-{ty}-{sign}-{mode}");
                let project = directory.path().join(&label);
                write_project(&project, &label, ty, min, max, lower_boundary);
                let mut command = Command::new(env!("CARGO_BIN_EXE_axiomc"));
                command
                    .arg("build")
                    .arg(&project)
                    .args(["--backend", "cranelift", "--json"]);
                if debug {
                    command.arg("--debug");
                }
                let build = command.output().expect("build numeric fixture");
                assert!(
                    build.status.success(),
                    "{label} build failed: stdout={} stderr={}",
                    String::from_utf8_lossy(&build.stdout),
                    String::from_utf8_lossy(&build.stderr)
                );
                let payload: Value = serde_json::from_slice(&build.stdout).expect("build JSON");
                assert_eq!(payload["backend"], "cranelift", "{label}");
                assert_eq!(payload["generated_rust"], Value::Null, "{label}");
                assert_eq!(
                    payload["lowering"]["execution_mode"], "direct_native_runtime",
                    "{label}"
                );
                assert_eq!(
                    payload["lowering"]["direct_native_runtime"], true,
                    "{label}"
                );
                assert_eq!(
                    payload["lowering"]["legacy_fallback_attempted"], false,
                    "{label}"
                );
                assert_eq!(
                    payload["lowering"]["known_value_static_folds"], false,
                    "{label}"
                );
                let binary = payload["binary"].as_str().expect("native binary path");

                // Run the same compiled artifact with two inputs. Empty stdin
                // adds zero; one byte crosses the selected signed boundary.
                let control = run_with_stdin(binary, b"");
                assert_eq!(
                    control.status.code(),
                    Some(0),
                    "{label} nonoverflow control failed: stderr={}",
                    String::from_utf8_lossy(&control.stderr)
                );
                assert!(control.stdout.is_empty(), "{label} control stdout");
                assert!(control.stderr.is_empty(), "{label} control stderr");

                let overflow = run_with_stdin(binary, b"x");
                assert!(overflow.stdout.is_empty(), "{label} overflow stdout");
                if debug {
                    assert!(!overflow.status.success(), "{label} accepted overflow");
                    assert_eq!(
                        String::from_utf8_lossy(&overflow.stderr),
                        format!(
                            "{{\"kind\":\"runtime\",\"message\":\"numeric overflow: {diagnostic_type} addition\"}}\n"
                        ),
                        "{label} must emit the structured overflow diagnostic"
                    );
                } else {
                    assert_eq!(
                        overflow.status.code(),
                        Some(42),
                        "{label} did not wrap to the opposite boundary: stderr={}",
                        String::from_utf8_lossy(&overflow.stderr)
                    );
                    assert!(overflow.stderr.is_empty(), "{label} release stderr");
                }
            }
        }
    }
}

fn run_with_stdin(binary: &str, input: &[u8]) -> Output {
    let mut child = Command::new(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start numeric fixture");
    child
        .stdin
        .take()
        .expect("fixture stdin")
        .write_all(input)
        .expect("write runtime numeric input");
    child.wait_with_output().expect("run numeric fixture")
}

fn write_project(project: &Path, name: &str, ty: &str, min: &str, max: &str, lower_boundary: bool) {
    fs::create_dir_all(project.join("src")).expect("create numeric source directory");
    fs::write(
        project.join("axiom.toml"),
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n"
        ),
    )
    .expect("write numeric manifest");
    fs::write(
        project.join("axiom.lock"),
        format!(
            "version = 1\n\n[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\nsource = \"path\"\n"
        ),
    )
    .expect("write numeric lockfile");
    let suffix = if ty == "int" { "" } else { ty };
    let runtime_offset = if ty == "int" {
        "len(content)".to_string()
    } else {
        format!("len(content) as {ty}")
    };
    let (boundary, wrapped) = if lower_boundary {
        (min, max)
    } else {
        (max, min)
    };
    let operand = if lower_boundary {
        "zero - offset".to_string()
    } else {
        "offset".to_string()
    };
    fs::write(
        project.join("src/main.ax"),
        format!(
            r#"import "std/io.ax"

fn main(): int {{
let content: string = read_to_string()
let offset: {ty} = {runtime_offset}
let zero: {ty} = 0{suffix}
let operand: {ty} = {operand}
let value: {ty} = {boundary}{suffix} + operand
if value == {wrapped}{suffix} {{
return 42
}} else {{
return 0
}}
}}
"#
        ),
    )
    .expect("write runtime numeric source");
}
