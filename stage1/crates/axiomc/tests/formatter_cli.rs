use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

fn run_fmt(args: &[&str], input: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn axiomc fmt");
    child
        .stdin
        .take()
        .expect("formatter stdin")
        .write_all(input.as_bytes())
        .expect("write formatter stdin");
    child.wait_with_output().expect("wait for axiomc fmt")
}

#[test]
fn fmt_stdin_writes_formatted_source_to_stdout() {
    let output = run_fmt(&["fmt", "--stdin"], "fn main() {}   \n");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        "fn main() {}\n"
    );
}

#[test]
fn fmt_stdin_check_returns_one_and_precise_json() {
    let output = run_fmt(
        &["fmt", "--stdin", "--check", "--json"],
        "fn main() {}   \n",
    );
    assert_eq!(output.status.code(), Some(1));
    let payload: Value = serde_json::from_slice(&output.stdout).expect("formatter JSON");
    assert_eq!(payload["command"], "fmt");
    assert_eq!(payload["input"], "stdin");
    assert_eq!(payload["ok"], false);
    assert_eq!(payload["files"][0]["path"], "<stdin>");
    assert_eq!(payload["files"][0]["edits"][0]["replacement"], "");
}

#[test]
fn fmt_stdin_range_without_line_ending_preserves_suffix_boundary() {
    let output = run_fmt(
        &["fmt", "--stdin", "--range", "6:15"],
        "first\nsecond   \nthird\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        "first\nsecond\nthird\n"
    );
}

#[test]
fn fmt_stdin_range_leaves_bytes_outside_range_unchanged() {
    let output = run_fmt(
        &["fmt", "--stdin", "--range", "9:19"],
        "first   \nsecond   \nthird   \n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        "first   \nsecond\nthird   \n"
    );
}
