//! `axiomc run` must not report a child killed by a signal as a plain exit 1
//! (issue #1784).
#![cfg(unix)]

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SIGSEGV: i32 = 11;

fn axiomc(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(args)
        .output()
        .expect("run axiomc")
}

fn package(dir: &Path, name: &str, source: &str) -> PathBuf {
    let root = dir.join(name);
    let created = axiomc(&["new", root.to_str().expect("UTF-8 path")]);
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    fs::write(root.join("src/main.ax"), source).expect("write source");
    root
}

// Unbounded recursion exhausts the native stack; the kernel delivers SIGSEGV.
const UNBOUNDED_RECURSION: &str =
    "fn descend(n: int): int {\n    return descend(n + 1)\n}\nprint descend(0)\n";

#[test]
fn run_reports_signal_termination_on_stderr_and_in_the_exit_code() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = package(dir.path(), "crash", UNBOUNDED_RECURSION);

    let output = axiomc(&["run", root.to_str().expect("UTF-8 path")]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(128 + SIGSEGV), "{stderr}");
    assert!(
        stderr.contains("was terminated by SIGSEGV (11)"),
        "{stderr}"
    );
}

#[test]
fn run_json_keeps_the_envelope_shape_and_reports_128_plus_signal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = package(dir.path(), "crash", UNBOUNDED_RECURSION);

    let output = axiomc(&["run", root.to_str().expect("UTF-8 path"), "--json"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("run JSON envelope");

    assert_eq!(output.status.code(), Some(128 + SIGSEGV), "{stderr}");
    assert_eq!(payload["command"], "run");
    assert_eq!(payload["ok"], false);
    assert_eq!(payload["result"], "failure");
    assert_eq!(payload["exit_code"], 128 + SIGSEGV);
    assert!(
        stderr.contains("was terminated by SIGSEGV (11)"),
        "{stderr}"
    );
}

#[test]
fn run_passes_an_ordinary_exit_code_through_without_a_signal_message() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = package(dir.path(), "panics", "panic \"ordinary failure\"\n");

    let output = axiomc(&["run", root.to_str().expect("UTF-8 path")]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(!stderr.contains("terminated by"), "{stderr}");
}
