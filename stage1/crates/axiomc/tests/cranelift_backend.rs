use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_hello_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("hello");
    fs::create_dir_all(project.join("src")).expect("create project src");
    copy_fixture("axiom.toml", &project.join("axiom.toml"));
    copy_fixture("axiom.lock", &project.join("axiom.lock"));
    copy_fixture("src/main.ax", &project.join("src/main.ax"));

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["packages"][0]["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    assert_eq!(payload["generated_rust"], Value::Null);
    assert!(Path::new(binary).exists(), "cranelift binary exists");
    assert!(
        Path::new(binary).with_extension("cranelift.o").exists(),
        "cranelift object exists"
    );

    let run = Command::new(binary).output().expect("run cranelift binary");
    assert!(
        run.status.success(),
        "cranelift binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "hello from stage1\n42\ntrue\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_i64_main_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("i64-main-exit");
    write_i64_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift i64 main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift i64 main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_terminal_panic_to_native_report() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let known_project = temp.path().join("terminal-panic-known");
    write_terminal_panic_project(
        &known_project,
        r#"fn main(): int {
panic("boom")
}
"#,
    );
    assert_terminal_panic_report(
        &known_project,
        "{\"kind\":\"panic\",\"message\":\"boom\"}\n",
    );

    let known_facts_project = temp.path().join("terminal-panic-known-facts");
    write_terminal_panic_project(
        &known_facts_project,
        r#"static STATIC_MESSAGE: string = "static boom"

fn helper_message(): string {
return "helper boom"
}

fn main(): int {
let local_message: string = "local boom"
let clone_message: string = "clone boom"
panic(local_message + " " + string_clone(clone_message) + " " + STATIC_MESSAGE + " " + helper_message())
}
"#,
    );
    assert_terminal_panic_report(
        &known_facts_project,
        "{\"kind\":\"panic\",\"message\":\"local boom clone boom static boom helper boom\"}\n",
    );

    let then_branch_project = temp.path().join("terminal-panic-then-branch");
    write_terminal_panic_project(
        &then_branch_project,
        r#"fn should_panic(): bool {
return true
}

fn main(): int {
if should_panic() {
panic("then boom")
} else {
return 0
}
}
"#,
    );
    assert_terminal_panic_report(
        &then_branch_project,
        "{\"kind\":\"panic\",\"message\":\"then boom\"}\n",
    );

    let branch_known_project = temp.path().join("terminal-panic-branch-known-facts");
    write_terminal_panic_project(
        &branch_known_project,
        r#"static STATIC_MESSAGE: string = "static branch"

fn should_panic(): bool {
return true
}

fn main(): int {
if should_panic() {
let branch_message: string = "branch local"
panic(branch_message + " " + STATIC_MESSAGE)
} else {
return 0
}
}
"#,
    );
    assert_terminal_panic_report(
        &branch_known_project,
        "{\"kind\":\"panic\",\"message\":\"branch local static branch\"}\n",
    );

    let bool_stringify_project = temp.path().join("terminal-panic-bool-stringify");
    write_terminal_panic_project(
        &bool_stringify_project,
        r#"import "std/json.ax"

fn main(): int {
let status: int = 1
panic(stringify_bool(status == 1))
}
"#,
    );
    assert_terminal_panic_report(
        &bool_stringify_project,
        "{\"kind\":\"panic\",\"message\":\"true\"}\n",
    );

    let branch_bool_alias_project = temp.path().join("terminal-panic-branch-bool-alias");
    write_terminal_panic_project(
        &branch_bool_alias_project,
        r#"import "std/json.ax"

fn should_panic(): bool {
return true
}

fn main(): int {
let status: int = 0
if should_panic() {
let message: string = stringify_bool(status == 1)
panic(message)
} else {
return 0
}
}
"#,
    );
    assert_terminal_panic_report(
        &branch_bool_alias_project,
        "{\"kind\":\"panic\",\"message\":\"false\"}\n",
    );

    let int_stringify_project = temp.path().join("terminal-panic-int-stringify");
    write_terminal_panic_project(
        &int_stringify_project,
        r#"import "std/json.ax"

fn main(): int {
let status: int = 40
panic(stringify_int(status + 2))
}
"#,
    );
    assert_terminal_panic_report(
        &int_stringify_project,
        "{\"kind\":\"panic\",\"message\":42}\n",
    );

    let branch_int_alias_project = temp.path().join("terminal-panic-branch-int-alias");
    write_terminal_panic_project(
        &branch_int_alias_project,
        r#"import "std/json.ax"

fn should_panic(): bool {
return true
}

fn main(): int {
let status: int = 7
if should_panic() {
let message: string = stringify_int(status - 10)
panic(message)
} else {
return 0
}
}
"#,
    );
    assert_terminal_panic_report(
        &branch_int_alias_project,
        "{\"kind\":\"panic\",\"message\":-3}\n",
    );

    let quoted_stringify_project = temp.path().join("terminal-panic-quoted-stringify");
    write_terminal_panic_project(
        &quoted_stringify_project,
        r#"import "std/json.ax"

fn main(): int {
let status: int = match parse_int("12345") { Some(value) => value, None => 1 }
let message: string = stringify_int(status)
panic(stringify_string(message))
}
"#,
    );
    assert_terminal_panic_report(
        &quoted_stringify_project,
        "{\"kind\":\"panic\",\"message\":\"12345\"}\n",
    );

    let key_projection_project = temp.path().join("terminal-panic-key-projection");
    write_terminal_panic_project(
        &key_projection_project,
        r#"fn main(): int {
let scores: {string: int} = {"build": 7, "deploy": 9}
let names: [string] = keys<string, int>(scores)
let selected_index: int = 1
let selected_line: string = names[selected_index]
panic(selected_line)
}
"#,
    );
    assert_terminal_panic_report(
        &key_projection_project,
        "{\"kind\":\"panic\",\"message\":\"deploy\"}\n",
    );

    let log_event_project = temp.path().join("terminal-panic-log-event");
    write_terminal_panic_project(
        &log_event_project,
        r#"import "std/log.ax"

fn main(): int {
let count: int = match json_parse_int("12345") { Some(value) => value, None => 1 }
let ready: bool = match json_parse_bool("false") { Some(value) => value, None => true }
let message_count: int = match json_parse_int("12345") { Some(value) => value, None => 1 }
let message: string = json_stringify_int(message_count)
panic(event("warn", message, fields2(field_int("count", count), field_bool("ready", ready))))
}
"#,
    );
    assert_terminal_panic_report(
        &log_event_project,
        "{\"kind\":\"panic\",\"message\":\"{\\\"level\\\":\\\"warn\\\",\\\"message\\\":\\\"12345\\\",\\\"attributes\\\":{\\\"count\\\":12345,\\\"ready\\\":false}}\"}\n",
    );

    let else_branch_project = temp.path().join("terminal-panic-else-branch");
    write_terminal_panic_project(
        &else_branch_project,
        r#"fn should_panic(): bool {
return false
}

fn main(): int {
if should_panic() {
return 0
} else {
panic("else boom")
}
}
"#,
    );
    assert_terminal_panic_report(
        &else_branch_project,
        "{\"kind\":\"panic\",\"message\":\"else boom\"}\n",
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_rejects_helper_terminal_panic_as_native_value_return() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("terminal-panic-helper");
    write_terminal_panic_project(
        &project,
        r#"fn fail(): int {
panic("helper boom")
}

fn main(): int {
let ignored: int = fail()
return 0
}
"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        !output.status.success(),
        "cranelift helper panic build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["ok"], Value::Bool(false));
    assert!(
        payload["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("main function is outside the direct-native i64 ABI subset"),
        "unexpected helper panic rejection: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_i64_returning_main_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("i64-returning-main-exit");
    write_i64_returning_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift i64 returning main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift i64 returning main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_i64_typed_numeric_returning_main_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    for (name, return_ty, literal) in [
        ("i32-returning-main-exit", "i32", "48i32"),
        ("u32-returning-main-exit", "u32", "49u32"),
    ] {
        let project = temp.path().join(name);
        write_typed_numeric_returning_main_exit_project(&project, name, return_ty, literal);

        let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
            .args([
                "build",
                project.to_str().expect("project path"),
                "--backend",
                "cranelift",
                "--json",
            ])
            .output()
            .expect("run axiomc build --backend cranelift");
        assert!(
            output.status.success(),
            "cranelift {return_ty} returning main build failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
        assert_eq!(payload["backend"], "cranelift");
        assert_eq!(payload["generated_rust"], Value::Null);
        let binary = payload["binary"].as_str().expect("binary path");
        let run = Command::new(binary)
            .output()
            .expect("run cranelift typed numeric main binary");
        let expected = if return_ty == "i32" { 48 } else { 49 };
        assert_eq!(run.status.code(), Some(expected));
        assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    }
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_option_int_match_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("option-int-match-main-exit");
    write_option_int_match_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift option int match main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift option int match main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_option_bool_match_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("option-bool-match-main-exit");
    write_option_bool_match_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift option bool match main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift option bool match main binary");
    assert_eq!(run.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_option_tuple_payload_match_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    for (variant, expected) in [("Some", 48), ("None", 49)] {
        let temp = tempfile::tempdir().expect("tempdir");
        let project = temp
            .path()
            .join(format!("option-tuple-payload-match-main-exit-{variant}"));
        write_option_tuple_payload_match_main_exit_project(&project, variant);

        let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
            .args([
                "build",
                project.to_str().expect("project path"),
                "--backend",
                "cranelift",
                "--json",
            ])
            .output()
            .expect("run axiomc build --backend cranelift");
        assert!(
            output.status.success(),
            "cranelift option tuple payload {variant} match main build failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
        assert_eq!(payload["backend"], "cranelift");
        assert_eq!(payload["generated_rust"], Value::Null);
        let binary = payload["binary"].as_str().expect("binary path");
        let run = Command::new(binary)
            .output()
            .expect("run cranelift option tuple payload match main binary");
        assert_eq!(run.status.code(), Some(expected));
        assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    }
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_option_array_payload_match_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    for (variant, expected) in [("Some", 48), ("None", 49)] {
        let temp = tempfile::tempdir().expect("tempdir");
        let project = temp
            .path()
            .join(format!("option-array-payload-match-main-exit-{variant}"));
        write_option_array_payload_match_main_exit_project(&project, variant);

        let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
            .args([
                "build",
                project.to_str().expect("project path"),
                "--backend",
                "cranelift",
                "--json",
            ])
            .output()
            .expect("run axiomc build --backend cranelift");
        assert!(
            output.status.success(),
            "cranelift option array payload {variant} match main build failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
        assert_eq!(payload["backend"], "cranelift");
        assert_eq!(payload["generated_rust"], Value::Null);
        let binary = payload["binary"].as_str().expect("binary path");
        let run = Command::new(binary)
            .output()
            .expect("run cranelift option array payload match main binary");
        assert_eq!(run.status.code(), Some(expected));
        assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    }
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_option_struct_payload_match_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    for (variant, expected) in [("Some", 48), ("None", 49)] {
        let temp = tempfile::tempdir().expect("tempdir");
        let project = temp
            .path()
            .join(format!("option-struct-payload-match-main-exit-{variant}"));
        write_option_struct_payload_match_main_exit_project(&project, variant);

        let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
            .args([
                "build",
                project.to_str().expect("project path"),
                "--backend",
                "cranelift",
                "--json",
            ])
            .output()
            .expect("run axiomc build --backend cranelift");
        assert!(
            output.status.success(),
            "cranelift option struct payload {variant} match main build failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
        assert_eq!(payload["backend"], "cranelift");
        assert_eq!(payload["generated_rust"], Value::Null);
        let binary = payload["binary"].as_str().expect("binary path");
        let run = Command::new(binary)
            .output()
            .expect("run cranelift option struct payload match main binary");
        assert_eq!(run.status.code(), Some(expected));
        assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    }
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_nested_option_match_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("nested-option-match-main-exit");
    write_nested_option_match_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift nested option match main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift nested option match main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_nested_result_match_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("nested-result-match-main-exit");
    write_nested_result_match_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift nested result match main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift nested result match main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_enum_nested_payload_match_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("enum-nested-payload-match-main-exit");
    write_enum_nested_payload_match_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift enum nested payload match main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift enum nested payload match main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_result_match_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    for (name, variant, expected) in [
        ("result-ok-match-main-exit", "Ok", 48),
        ("result-err-match-main-exit", "Err", 49),
    ] {
        let project = temp.path().join(name);
        write_result_match_main_exit_project(&project, name, variant, expected);

        let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
            .args([
                "build",
                project.to_str().expect("project path"),
                "--backend",
                "cranelift",
                "--json",
            ])
            .output()
            .expect("run axiomc build --backend cranelift");
        assert!(
            output.status.success(),
            "cranelift {variant} result match main build failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
        assert_eq!(payload["backend"], "cranelift");
        assert_eq!(payload["generated_rust"], Value::Null);
        let binary = payload["binary"].as_str().expect("binary path");
        let run = Command::new(binary)
            .output()
            .expect("run cranelift result match main binary");
        assert_eq!(run.status.code(), Some(expected));
        assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    }
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_result_bool_match_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    for (name, variant) in [
        ("result-ok-bool-match-main-exit", "Ok"),
        ("result-err-bool-match-main-exit", "Err"),
    ] {
        let project = temp.path().join(name);
        write_result_bool_match_main_exit_project(&project, name, variant);

        let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
            .args([
                "build",
                project.to_str().expect("project path"),
                "--backend",
                "cranelift",
                "--json",
            ])
            .output()
            .expect("run axiomc build --backend cranelift");
        assert!(
            output.status.success(),
            "cranelift {variant} result bool match main build failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
        assert_eq!(payload["backend"], "cranelift");
        assert_eq!(payload["generated_rust"], Value::Null);
        let binary = payload["binary"].as_str().expect("binary path");
        let run = Command::new(binary)
            .output()
            .expect("run cranelift result bool match main binary");
        assert_eq!(run.status.code(), Some(1));
        assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    }
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_result_mixed_match_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    for (name, variant, expected) in [
        ("result-ok-mixed-match-main-exit", "Ok", 48),
        ("result-err-mixed-match-main-exit", "Err", 49),
    ] {
        let project = temp.path().join(name);
        write_result_mixed_match_main_exit_project(&project, name, variant);

        let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
            .args([
                "build",
                project.to_str().expect("project path"),
                "--backend",
                "cranelift",
                "--json",
            ])
            .output()
            .expect("run axiomc build --backend cranelift");
        assert!(
            output.status.success(),
            "cranelift {variant} result mixed match main build failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
        assert_eq!(payload["backend"], "cranelift");
        assert_eq!(payload["generated_rust"], Value::Null);
        let binary = payload["binary"].as_str().expect("binary path");
        let run = Command::new(binary)
            .output()
            .expect("run cranelift result mixed match main binary");
        assert_eq!(run.status.code(), Some(expected));
        assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    }
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_result_mixed_reverse_match_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    for (name, variant) in [
        ("result-ok-mixed-reverse-match-main-exit", "Ok"),
        ("result-err-mixed-reverse-match-main-exit", "Err"),
    ] {
        let project = temp.path().join(name);
        write_result_mixed_reverse_match_main_exit_project(&project, name, variant);

        let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
            .args([
                "build",
                project.to_str().expect("project path"),
                "--backend",
                "cranelift",
                "--json",
            ])
            .output()
            .expect("run axiomc build --backend cranelift");
        assert!(
            output.status.success(),
            "cranelift {variant} result mixed reverse match main build failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
        assert_eq!(payload["backend"], "cranelift");
        assert_eq!(payload["generated_rust"], Value::Null);
        let binary = payload["binary"].as_str().expect("binary path");
        let run = Command::new(binary)
            .output()
            .expect("run cranelift result mixed reverse match main binary");
        assert_eq!(run.status.code(), Some(1));
        assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    }
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_result_typed_numeric_match_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    for (name, variant, expected) in [
        ("result-ok-typed-numeric-match-main-exit", "Ok", 48),
        ("result-err-typed-numeric-match-main-exit", "Err", 49),
    ] {
        let project = temp.path().join(name);
        write_result_typed_numeric_match_main_exit_project(&project, name, variant);

        let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
            .args([
                "build",
                project.to_str().expect("project path"),
                "--backend",
                "cranelift",
                "--json",
            ])
            .output()
            .expect("run axiomc build --backend cranelift");
        assert!(
            output.status.success(),
            "cranelift {variant} result typed numeric match main build failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
        assert_eq!(payload["backend"], "cranelift");
        assert_eq!(payload["generated_rust"], Value::Null);
        let binary = payload["binary"].as_str().expect("binary path");
        let run = Command::new(binary)
            .output()
            .expect("run cranelift result typed numeric match main binary");
        assert_eq!(run.status.code(), Some(expected));
        assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    }
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_result_numeric_width_match_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    for (name, ok_ty, err_ty, ok_literal, err_literal, variant, expected) in [
        (
            "result-ok-i64-u16-match-main-exit",
            "i64",
            "u16",
            "48i64",
            "49u16",
            "Ok",
            48,
        ),
        (
            "result-err-i64-u16-match-main-exit",
            "i64",
            "u16",
            "48i64",
            "49u16",
            "Err",
            49,
        ),
        (
            "result-ok-u8-i8-match-main-exit",
            "u8",
            "i8",
            "48u8",
            "49i8",
            "Ok",
            48,
        ),
        (
            "result-err-u8-i8-match-main-exit",
            "u8",
            "i8",
            "48u8",
            "49i8",
            "Err",
            49,
        ),
    ] {
        let project = temp.path().join(name);
        write_result_numeric_width_match_main_exit_project(
            &project,
            name,
            ok_ty,
            err_ty,
            ok_literal,
            err_literal,
            variant,
        );

        let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
            .args([
                "build",
                project.to_str().expect("project path"),
                "--backend",
                "cranelift",
                "--json",
            ])
            .output()
            .expect("run axiomc build --backend cranelift");
        assert!(
            output.status.success(),
            "cranelift {variant} result numeric width match main build failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
        assert_eq!(payload["backend"], "cranelift");
        assert_eq!(payload["generated_rust"], Value::Null);
        let binary = payload["binary"].as_str().expect("binary path");
        let run = Command::new(binary)
            .output()
            .expect("run cranelift result numeric width match main binary");
        assert_eq!(run.status.code(), Some(expected));
        assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    }
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_result_tuple_payload_match_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    for (name, variant, expected) in [
        ("result-ok-tuple-payload-match-main-exit", "Ok", 48),
        ("result-err-tuple-payload-match-main-exit", "Err", 49),
    ] {
        let project = temp.path().join(name);
        write_result_tuple_payload_match_main_exit_project(&project, name, variant);

        let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
            .args([
                "build",
                project.to_str().expect("project path"),
                "--backend",
                "cranelift",
                "--json",
            ])
            .output()
            .expect("run axiomc build --backend cranelift");
        assert!(
            output.status.success(),
            "cranelift {variant} result tuple payload match main build failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
        assert_eq!(payload["backend"], "cranelift");
        assert_eq!(payload["generated_rust"], Value::Null);
        let binary = payload["binary"].as_str().expect("binary path");
        let run = Command::new(binary)
            .output()
            .expect("run cranelift result tuple payload match main binary");
        assert_eq!(run.status.code(), Some(expected));
        assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    }
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_result_dual_tuple_payload_match_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    for (name, variant, expected) in [
        ("result-ok-dual-tuple-payload-match-main-exit", "Ok", 48),
        ("result-err-dual-tuple-payload-match-main-exit", "Err", 49),
    ] {
        let project = temp.path().join(name);
        write_result_dual_tuple_payload_match_main_exit_project(&project, name, variant);

        let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
            .args([
                "build",
                project.to_str().expect("project path"),
                "--backend",
                "cranelift",
                "--json",
            ])
            .output()
            .expect("run axiomc build --backend cranelift");
        assert!(
            output.status.success(),
            "cranelift {variant} result dual tuple payload match main build failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
        assert_eq!(payload["backend"], "cranelift");
        assert_eq!(payload["generated_rust"], Value::Null);
        let binary = payload["binary"].as_str().expect("binary path");
        let run = Command::new(binary)
            .output()
            .expect("run cranelift result dual tuple payload match main binary");
        assert_eq!(run.status.code(), Some(expected));
        assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    }
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_result_array_payload_match_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    for (name, variant, expected) in [
        ("result-ok-array-payload-match-main-exit", "Ok", 48),
        ("result-err-array-payload-match-main-exit", "Err", 49),
    ] {
        let project = temp.path().join(name);
        write_result_array_payload_match_main_exit_project(&project, name, variant);

        let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
            .args([
                "build",
                project.to_str().expect("project path"),
                "--backend",
                "cranelift",
                "--json",
            ])
            .output()
            .expect("run axiomc build --backend cranelift");
        assert!(
            output.status.success(),
            "cranelift {variant} result array payload match main build failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
        assert_eq!(payload["backend"], "cranelift");
        assert_eq!(payload["generated_rust"], Value::Null);
        let binary = payload["binary"].as_str().expect("binary path");
        let run = Command::new(binary)
            .output()
            .expect("run cranelift result array payload match main binary");
        assert_eq!(run.status.code(), Some(expected));
        assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    }
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_result_struct_payload_match_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    for (name, variant, expected) in [
        ("result-ok-struct-payload-match-main-exit", "Ok", 48),
        ("result-err-struct-payload-match-main-exit", "Err", 49),
    ] {
        let project = temp.path().join(name);
        write_result_struct_payload_match_main_exit_project(&project, name, variant);

        let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
            .args([
                "build",
                project.to_str().expect("project path"),
                "--backend",
                "cranelift",
                "--json",
            ])
            .output()
            .expect("run axiomc build --backend cranelift");
        assert!(
            output.status.success(),
            "cranelift {variant} result struct payload match main build failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
        assert_eq!(payload["backend"], "cranelift");
        assert_eq!(payload["generated_rust"], Value::Null);
        let binary = payload["binary"].as_str().expect("binary path");
        let run = Command::new(binary)
            .output()
            .expect("run cranelift result struct payload match main binary");
        assert_eq!(run.status.code(), Some(expected));
        assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    }
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_aggregate_helper_reassignment_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("aggregate-helper-reassignment-main-exit");
    write_aggregate_helper_reassignment_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift aggregate helper reassignment build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift aggregate helper reassignment binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_i64_bool_returning_main_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("bool-returning-main-exit");
    write_bool_returning_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift bool main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift bool main binary");
    assert_eq!(run.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_bool_tuple_index_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("bool-tuple-index-main-exit");
    write_bool_tuple_index_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift bool tuple index main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift bool tuple index main binary");
    assert_eq!(run.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_tuple_returning_helper_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("tuple-returning-helper-main-exit");
    write_tuple_returning_helper_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift tuple-returning helper main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift tuple-returning helper main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_array_literal_index_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("array-literal-index-main-exit");
    write_array_literal_index_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift array literal index main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift array literal index main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_fixed_array_intrinsics_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("fixed-array-intrinsics-main-exit");
    write_fixed_array_intrinsics_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift fixed array intrinsics main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift fixed array intrinsics main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_static_slice_bounds_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("static-slice-bounds-main-exit");
    write_static_slice_bounds_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift static slice bounds main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift static slice bounds main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_string_literal_len_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("string-literal-len-main-exit");
    write_string_literal_len_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift string literal len main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift string literal len main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[test]
fn cranelift_backend_rejects_unsupported_string_helper_main() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("unsupported-string-helper-main");
    write_unsupported_string_helper_main_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        !output.status.success(),
        "cranelift unsupported string helper main unexpectedly built: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.stdout.is_empty() {
        assert!(
            stderr.contains("main function is outside the direct-native i64 ABI subset"),
            "unexpected cranelift unsupported string helper error: stdout={stdout} stderr={stderr}"
        );
        return;
    }
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["ok"], Value::Bool(false));
    let message = payload["error"]["message"].as_str().expect("error message");
    assert!(
        message.contains("main function is outside the direct-native i64 ABI subset"),
        "unexpected cranelift unsupported string helper error: {message}"
    );
}

#[test]
fn cranelift_backend_lowers_known_string_helpers_to_runtime_exit_code() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("known-string-helper-main-exit");
    write_known_string_helper_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift known string helper main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift known string helper main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_std_encoding_wrappers_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("std-encoding-wrapper-main-exit");
    write_std_encoding_wrapper_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift std encoding wrapper main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift std encoding wrapper main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_known_crypto_text_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("known-crypto-text-main-exit");
    write_known_crypto_text_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift known crypto text main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift known crypto text main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_std_crypto_wrappers_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("std-crypto-wrapper-main-exit");
    write_std_crypto_wrapper_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift std crypto wrapper main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let audit_log = temp.path().join("std-crypto-audit.jsonl");
    let run = Command::new(binary)
        .env("AXIOM_HOST_AUDIT_LOG", &audit_log)
        .output()
        .expect("run cranelift std crypto wrapper main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    let audit = fs::read_to_string(&audit_log).expect("read std crypto audit log");
    assert!(audit.contains("\"intrinsic\":\"crypto_sha256\""));
    assert!(audit.contains("\"intrinsic\":\"crypto_hmac_sha256\""));
    assert!(audit.contains("\"intrinsic\":\"crypto_hmac_sha512\""));
    assert!(audit.contains("\"inputs\":\"strings:1\""));
    assert!(audit.contains("\"inputs\":\"strings:2\""));
    assert_eq!(audit.matches("\"outcome\":\"ok\"").count(), 3, "{audit}");
    assert!(!audit.contains("ba7816bf"));
    assert!(!audit.contains("f7bc83f4"));
    assert!(!audit.contains("164b7a7b"));
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_known_regex_text_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("known-regex-text-main-exit");
    write_known_regex_text_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift known regex text main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift known regex text main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_std_regex_wrappers_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("std-regex-wrapper-main-exit");
    write_std_regex_wrapper_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift std regex wrapper main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift std regex wrapper main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_known_json_text_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("known-json-text-main-exit");
    write_known_json_text_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift known json text main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift known json text main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_std_json_wrappers_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("std-json-wrapper-main-exit");
    write_std_json_wrapper_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift std json wrapper main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift std json wrapper main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_std_log_format_wrappers_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("std-log-format-wrapper-main-exit");
    write_std_log_format_wrapper_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift std log format wrapper main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift std log format wrapper main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_std_log_selected_projection_lengths_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("std-log-selected-projection-main-exit");
    write_std_log_selected_projection_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift std log selected projection main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift std log selected projection main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_std_log_dynamic_scalar_lengths_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("std-log-dynamic-scalar-main-exit");
    write_std_log_dynamic_scalar_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift std log dynamic scalar main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift std log dynamic scalar main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_std_log_dynamic_scalar_info_attrs_to_runtime_stderr() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("std-log-dynamic-scalar-info-attrs");
    write_std_log_dynamic_scalar_info_attrs_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift std log dynamic scalar info attrs build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let audit_log = temp.path().join("std-log-stderr-audit.jsonl");
    let run = Command::new(binary)
        .env("AXIOM_HOST_AUDIT_LOG", &audit_log)
        .output()
        .expect("run cranelift std log dynamic scalar info attrs binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        "{\"level\":\"info\",\"message\":\"12345\",\"attributes\":{\"count\":12345,\"ready\":false}}\n"
    );
    let audit = fs::read_to_string(&audit_log).expect("read std log stderr audit log");
    assert!(
        audit.contains("\"intrinsic\":\"io_stderr_write\""),
        "{audit}"
    );
    assert!(audit.contains("\"stream\":\"stderr\""), "{audit}");
    assert!(audit.contains("\"bytes\":\"int:"), "{audit}");
    assert_eq!(audit.matches("\"outcome\":\"ok\"").count(), 15, "{audit}");
    assert!(!audit.contains("12345"));
    assert!(!audit.contains("ready"));
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_std_log_level_wrapper_to_runtime_stderr() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("std-log-level-wrapper");
    write_std_log_level_wrapper_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift std log level wrapper build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift std log level wrapper binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        "{\"level\":\"info\",\"message\":\"12345\",\"attributes\":{}}\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_std_log_dynamic_event_print_to_runtime_stdout() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("std-log-dynamic-event-print");
    write_std_log_dynamic_event_print_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift std log dynamic event print build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let audit_log = temp.path().join("std-log-stdout-audit.jsonl");
    let run = Command::new(binary)
        .env("AXIOM_HOST_AUDIT_LOG", &audit_log)
        .output()
        .expect("run cranelift std log dynamic event print binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "{\"level\":\"warn\",\"message\":\"\\\"12345\\\"\",\"attributes\":{\"count\":12345,\"ready\":false}}\n{\"level\":\"info\",\"message\":\"12345\",\"attributes\":{\"count\":12345,\"ready\":false,\"quoted\":\"\\\"12345\\\"\"}}\n"
    );
    assert_eq!(String::from_utf8_lossy(&run.stderr), "");
    let audit = fs::read_to_string(&audit_log).expect("read std log stdout audit log");
    assert!(
        audit.contains("\"intrinsic\":\"io_stdout_write\""),
        "{audit}"
    );
    assert!(audit.contains("\"stream\":\"stdout\""), "{audit}");
    assert!(audit.contains("\"bytes\":\"int:"), "{audit}");
    assert_eq!(audit.matches("\"outcome\":\"ok\"").count(), 40, "{audit}");
    assert!(!audit.contains("12345"));
    assert!(!audit.contains("quoted"));
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_struct_literal_field_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("struct-literal-field-main-exit");
    write_struct_literal_field_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift struct literal field main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift struct literal field main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_i64_while_loop_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("i64-while-loop-exit");
    write_i64_while_loop_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift i64 while loop build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift i64 while loop binary");
    assert_eq!(run.status.code(), Some(42));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_regex_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("regex-surface");
    write_regex_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift regex build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift regex binary");
    assert!(
        run.status.success(),
        "cranelift regex binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "true
238
issue-#-ready
xa
xaa
ba
"
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout.lines().nth(3),
        Some("xa"),
        "anchored replace_all must only rewrite the original leading match"
    );
    assert_eq!(stdout.lines().nth(4), Some("xaa"));
    assert_eq!(stdout.lines().nth(5), Some("ba"));
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_scalar_aggregate_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("scalar-aggregate");
    write_scalar_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift scalar build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift scalar binary");
    assert!(
        run.status.success(),
        "cranelift scalar binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "native\n7\n12\n10\n");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_std_string_builder_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("string-builder");
    write_std_string_builder_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift string builder build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift string builder binary");
    assert!(
        run.status.success(),
        "cranelift string builder binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "hello stdlib
first line
second line
"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_std_string_builder_wrappers_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("string-builder-main-exit");
    write_std_string_builder_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift string builder main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift string builder main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_string_intrinsics_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("string-intrinsics");
    write_string_intrinsics_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift string intrinsics build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift string intrinsics binary");
    assert!(
        run.status.success(),
        "cranelift string intrinsics binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "native
true
1
stage
[padded]
[left  ]
second
none
"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_numeric_cross_width_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("numeric-cross-width");
    write_numeric_cross_width_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift numeric build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift numeric binary");
    assert!(
        run.status.success(),
        "cranelift numeric binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "7\n255\n3\n-4\n42\n255\n44\n-126\n18446744073709551615\n18446744073709551615\n255\n0\n16777216\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_const_sized_array_conformance_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("const-sized-array");
    copy_conformance_fixture("const_sized_arrays", &project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift const-sized-array build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift const-sized-array binary");
    assert!(
        run.status.success(),
        "cranelift const-sized-array binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n6\n");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_static_scalar_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("static-scalar-globals");
    write_static_scalar_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift static scalar build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift static scalar binary");
    assert!(
        run.status.success(),
        "cranelift static scalar binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "hello static\n42\n43\ntrue\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_struct_field_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("struct-field");
    write_struct_field_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift struct build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(
        payload.get("generated_rust"),
        Some(&Value::Null),
        "build JSON must explicitly report generated_rust: null"
    );
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift struct binary");
    assert!(
        run.status.success(),
        "cranelift struct binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "3\ntrue\nstage1 structs\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_enum_match_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("enum-match");
    write_enum_match_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift enum match build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift enum match binary");
    assert!(
        run.status.success(),
        "cranelift enum match binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "multi\nnamed\npayload\n2\n8\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_enum_payload_match_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    for (variant, expected) in [("Ready", 48), ("Fallback", 49), ("Off", 1)] {
        let temp = tempfile::tempdir().expect("tempdir");
        let project = temp
            .path()
            .join(format!("enum-payload-match-main-exit-{variant}"));
        write_enum_payload_match_main_exit_project(&project, variant);

        let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
            .args([
                "build",
                project.to_str().expect("project path"),
                "--backend",
                "cranelift",
                "--json",
            ])
            .output()
            .expect("run axiomc build --backend cranelift");
        assert!(
            output.status.success(),
            "cranelift enum payload {variant} match main build failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
        assert_eq!(payload["backend"], "cranelift");
        assert_eq!(payload["generated_rust"], Value::Null);
        let binary = payload["binary"].as_str().expect("binary path");
        let run = Command::new(binary)
            .output()
            .expect("run cranelift enum payload match main binary");
        assert_eq!(run.status.code(), Some(expected));
        assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    }
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_array_helpers_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("array-helpers");
    write_array_helpers_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift array-helpers build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift array-helpers binary");
    assert!(
        run.status.success(),
        "cranelift array-helpers binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n10\n30\n40\n");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_process_status_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }
    if !Path::new("/usr/bin/true").exists() || !Path::new("/usr/bin/false").exists() {
        eprintln!(
            "skipping cranelift process-status test because /usr/bin/true or /usr/bin/false is unavailable"
        );
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("process-status");
    write_process_status_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        output.status.success(),
        "cranelift process-status build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift process-status binary");
    assert!(
        run.status.success(),
        "cranelift process-status binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "0\n1\n-1\n");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_process_status_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }
    if !Path::new("/usr/bin/true").exists() || !Path::new("/usr/bin/false").exists() {
        eprintln!(
            "skipping cranelift process-status test because /usr/bin/true or /usr/bin/false is unavailable"
        );
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("process-status-main-exit");
    write_process_status_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift process-status main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let audit_log = temp.path().join("process-audit.jsonl");
    let run = Command::new(binary)
        .env("AXIOM_HOST_AUDIT_LOG", &audit_log)
        .output()
        .expect("run cranelift process-status main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    let audit = fs::read_to_string(&audit_log).expect("read process audit log");
    assert!(
        audit.contains("\"intrinsic\":\"process_status\""),
        "{audit}"
    );
    assert_eq!(audit.matches("\"outcome\":\"ok\"").count(), 2, "{audit}");
    assert_eq!(
        audit.matches("\"outcome\":\"denied\"").count(),
        1,
        "{audit}"
    );
    assert!(
        audit.contains("\"args\":{\"command\":\"string:13\"}"),
        "{audit}"
    );
    assert!(
        audit.contains("\"args\":{\"command\":\"string:14\"}"),
        "{audit}"
    );
    assert!(
        audit.contains("\"args\":{\"command\":\"string:31\"}"),
        "{audit}"
    );
    assert!(
        !audit.contains("/usr/bin") && !audit.contains("__axiom_stage1_missing_binary__"),
        "audit log should not contain process commands: {audit}"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_rejects_unapproved_process_status_command() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("process-status-unapproved");
    write_process_status_unapproved_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift process-status build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostic.contains("allowlisted deterministic commands"),
        "unexpected diagnostic: {diagnostic}"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_fs_read_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("fs-read-main-exit");
    write_fs_read_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift fs-read main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let audit_log = project.join("native-fs-read-audit.jsonl");
    assert!(
        !audit_log.exists(),
        "build should not create the native fs read audit log"
    );
    fs::write(project.join("src/fixture.txt"), "runtime-file\n")
        .expect("rewrite fs-read fixture for runtime");
    let run = Command::new(binary)
        .env("AXIOM_HOST_AUDIT_LOG", &audit_log)
        .output()
        .expect("run cranelift fs-read main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    let audit = fs::read_to_string(&audit_log).expect("read native fs read audit log");
    assert!(
        audit.contains("\"intrinsic\":\"fs_read\""),
        "audit log: {audit}"
    );
    assert!(audit.contains("\"outcome\":\"ok\""), "audit log: {audit}");
    assert!(
        audit.contains("\"outcome\":\"denied\""),
        "audit log: {audit}"
    );
    assert!(
        audit.contains("\"args\":{\"path\":\"string:15\"}"),
        "audit log: {audit}"
    );
    assert!(!audit.contains("native-fs"));
    assert!(!audit.contains("runtime-file"));
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_denies_fs_read_symlink_escape_at_runtime() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("fs-read-symlink-escape");
    write_fs_read_symlink_escape_project(&project);
    let outside = temp.path().join("escape.txt");
    let link = project.join("src/fixture.txt");

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift fs-read symlink build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    fs::write(&outside, "outside-readable").expect("write outside symlink target");
    fs::remove_file(&link).expect("remove build-time read fixture");
    std::os::unix::fs::symlink(&outside, &link).expect("create runtime read symlink escape");

    let run = Command::new(binary)
        .output()
        .expect("run cranelift fs-read symlink binary");
    assert_eq!(run.status.code(), Some(37));
    assert!(
        fs::symlink_metadata(&link)
            .expect("stat runtime read symlink")
            .file_type()
            .is_symlink(),
        "runtime denial should leave the read symlink fixture in place"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_fs_write_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("fs-write-main-exit");
    write_fs_write_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift fs-write main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let runtime_file = project.join("scratch/data.txt");
    let replace_temp_file = project.join("scratch/.data.txt.axiom-replace.tmp");
    let created_file = project.join("scratch/created.txt");
    let runtime_dir = project.join("scratch/native-dir");
    let runtime_dir_all = project.join("scratch/native-all");
    let runtime_nested_dir = project.join("scratch/native-all/deep");
    let audit_log = project.join("native-fs-audit.jsonl");
    assert!(
        !runtime_file.exists(),
        "build should not create the fs_write runtime fixture"
    );
    assert!(
        !replace_temp_file.exists(),
        "build should not create the fs_replace temp fixture"
    );
    assert!(
        !created_file.exists(),
        "build should not create the create_file runtime fixture"
    );
    assert!(
        !runtime_dir.exists(),
        "build should not create the mkdir runtime fixture"
    );
    assert!(
        !runtime_dir_all.exists(),
        "build should not create the mkdir_all runtime fixture"
    );
    assert!(
        !audit_log.exists(),
        "build should not create the native fs audit log"
    );
    let run = Command::new(binary)
        .env("AXIOM_HOST_AUDIT_LOG", &audit_log)
        .output()
        .expect("run cranelift fs-write main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    assert!(
        !runtime_file.exists(),
        "runtime remove_file should remove the fs_write fixture"
    );
    assert!(
        !replace_temp_file.exists(),
        "runtime replace_file should not leave the temp fixture"
    );
    assert_eq!(
        fs::read_to_string(&created_file).expect("read create_file runtime fixture"),
        ""
    );
    assert!(
        !runtime_dir.exists(),
        "runtime remove_dir should remove the mkdir fixture"
    );
    assert!(
        runtime_nested_dir.is_dir(),
        "runtime mkdir_all should create the nested directory fixture"
    );
    assert!(
        runtime_dir_all.is_dir(),
        "runtime mkdir_all should create the parent directory fixture"
    );
    let audit = fs::read_to_string(&audit_log).expect("read native fs audit log");
    for intrinsic in [
        "fs_write",
        "fs_append",
        "fs_replace",
        "fs_remove_file",
        "fs_create",
        "fs_mkdir",
        "fs_remove_dir",
        "fs_mkdir_all",
    ] {
        assert!(
            audit.contains(&format!("\"intrinsic\":\"{intrinsic}\"")),
            "missing {intrinsic} in audit log: {audit}"
        );
    }
    assert!(audit.contains("\"outcome\":\"ok\""), "audit log: {audit}");
    assert!(
        audit.contains("\"outcome\":\"denied\""),
        "audit log: {audit}"
    );
    assert!(
        audit.contains("\"args\":{\"path\":\"string:16\",\"content\":\"string:13\"}"),
        "audit log: {audit}"
    );
    assert!(!audit.contains("runtime-write"));
    assert!(!audit.contains("runtime-append"));
    assert!(!audit.contains("runtime-replace"));
    assert!(!audit.contains("blocked"));
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_denies_fs_write_symlink_escape_at_runtime() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("fs-write-symlink-escape");
    write_fs_write_symlink_escape_project(&project);
    let outside = temp.path().join("escape.txt");
    let link = project.join("scratch/link.txt");

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift fs-write symlink build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    assert!(
        !link.exists(),
        "build should not create the fs_write symlink target"
    );
    fs::write(&outside, "outside-safe").expect("write outside symlink target");
    std::os::unix::fs::symlink(&outside, &link).expect("create runtime symlink escape");

    let run = Command::new(binary)
        .output()
        .expect("run cranelift fs-write symlink binary");
    assert_eq!(run.status.code(), Some(37));
    assert_eq!(
        fs::read_to_string(&outside).expect("read outside symlink target"),
        "outside-safe"
    );
    assert!(
        fs::symlink_metadata(&link)
            .expect("stat runtime symlink")
            .file_type()
            .is_symlink(),
        "runtime denial should leave the symlink fixture in place"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_fs_write_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("fs-write");
    write_fs_write_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        output.status.success(),
        "cranelift fs-write build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift fs-write binary");
    assert!(
        run.status.success(),
        "cranelift fs-write binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "0\n0\n0\none\ntwo\n0\nfinal\n0\n0\n0\n0\n0\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_honors_fs_root_for_fs_write_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("fs-root");
    write_fs_root_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        output.status.success(),
        "cranelift fs-root build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift fs-root binary");
    assert!(
        run.status.success(),
        "cranelift fs-root binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "0\n0\n-1\nmissing\nok\n"
    );
    assert_eq!(
        fs::read_to_string(project.join("src/main.ax")).expect("read source"),
        fs_root_source(&project)
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_borrowed_slice_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("borrowed-slice");
    write_borrowed_slice_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift borrowed-slice build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift borrowed-slice binary");
    assert!(
        run.status.success(),
        "cranelift borrowed-slice binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n4\n8\n6\n3\n");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_owned_move_state_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("owned-move-state");
    write_owned_move_state_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift owned move-state build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift owned move-state binary");
    assert!(
        run.status.success(),
        "cranelift owned move-state binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "3\nleft\n");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_debug_build_emits_sidecars_without_axiom_dwarf() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("hello-debug");
    fs::create_dir_all(project.join("src")).expect("create project src");
    copy_fixture("axiom.toml", &project.join("axiom.toml"));
    copy_fixture("axiom.lock", &project.join("axiom.lock"));
    copy_fixture("src/main.ax", &project.join("src/main.ax"));

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--debug",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift --debug");
    assert!(
        output.status.success(),
        "cranelift debug build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["debug"], true);
    let binary = payload["binary"].as_str().expect("binary path");
    assert_eq!(payload["generated_rust"], Value::Null);
    let debug_map = payload["debug_map"].as_str().expect("debug map path");
    let debug_manifest = payload["debug_manifest"]
        .as_str()
        .expect("debug manifest path");
    assert!(Path::new(binary).exists(), "cranelift binary exists");
    assert!(
        Path::new(binary).with_extension("cranelift.o").exists(),
        "cranelift object exists"
    );
    assert!(Path::new(debug_map).exists(), "debug map exists");
    assert!(Path::new(debug_manifest).exists(), "debug manifest exists");

    let source = project
        .join("src/main.ax")
        .canonicalize()
        .expect("canonical source path")
        .display()
        .to_string();
    let map: Value =
        serde_json::from_str(&fs::read_to_string(debug_map).expect("read cranelift debug map"))
            .expect("parse cranelift debug map");
    assert_eq!(
        map["schema_version"],
        "axiom.stage1.direct_native.debug_map.v1"
    );
    assert_eq!(map["backend"], "cranelift");
    assert_eq!(map["binary"], binary);
    assert!(
        map["source_spans"]
            .as_array()
            .expect("debug source spans")
            .iter()
            .any(|span| span["source"] == source),
        "debug map should retain Axiom source spans for cranelift builds"
    );

    let manifest: Value = serde_json::from_str(
        &fs::read_to_string(debug_manifest).expect("read cranelift debug manifest"),
    )
    .expect("parse cranelift debug manifest");
    assert_eq!(
        manifest["schema_version"],
        "axiom.stage1.direct_native.debug_manifest.v1"
    );
    assert_eq!(manifest["backend"], "cranelift");
    assert_eq!(manifest["artifact_class"], "native_binary");
    assert_eq!(manifest["binary"], binary);
    assert!(
        manifest["binary_hash"]
            .as_str()
            .is_some_and(|hash| !hash.is_empty()),
        "direct-native debug manifest should keep the binary hash in the integrity envelope"
    );
    assert_eq!(manifest["debug_map"], debug_map);
    assert_eq!(manifest["native_debug"]["producer"], "cranelift");
    assert_eq!(manifest["native_debug"]["debuginfo"], 0);
    assert_eq!(manifest["native_debug"]["opt_level"], 0);
    assert_eq!(manifest["native_debug"]["axiom_dwarf"], false);
    assert!(manifest["native_debug"]["native_debug_info"]
        .as_str()
        .expect("native debug info")
        .contains("does not emit native Axiom DWARF yet"));
    assert!(
        manifest.get("rustc").is_none(),
        "cranelift debug manifests should not claim rustc debug settings"
    );
    assert!(
        manifest["source_files"]
            .as_array()
            .expect("source files")
            .iter()
            .any(|source_file| source_file["path"] == source
                && source_file["mapping_count"].as_u64().unwrap_or(0) > 0),
        "debug manifest should count Axiom source mappings for cranelift builds"
    );

    let run = Command::new(binary)
        .output()
        .expect("run cranelift debug binary");
    assert!(
        run.status.success(),
        "cranelift debug binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "hello from stage1
42
true
"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_map_index_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("map-index");
    write_map_index_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift map build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift map binary");
    assert!(
        run.status.success(),
        "cranelift map binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "11
true
false
high
9
9
13
"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_map_get_or_default_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("map-get-or-default-main-exit");
    write_map_get_or_default_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift map get_or_default main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift map get_or_default main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_static_bool_map_keys_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("static-bool-map-keys-main-exit");
    write_static_bool_map_keys_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift static bool map keys main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift static bool map keys main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_std_collection_wrappers_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("std-collection-wrapper-main-exit");
    write_std_collection_wrapper_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift std collection wrapper main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .env("AXIOM_CRANELIFT_DYNAMIC_KEY_INDEX", "deploy")
        .output()
        .expect("run cranelift std collection wrapper main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_std_collection_lookup_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("collection-lookup");
    write_std_collection_lookup_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift collection lookup build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift collection lookup binary");
    assert!(
        run.status.success(),
        "cranelift collection lookup binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "true
9
0
13
2
build
deploy
"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_net_resolve_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("net-resolve");
    write_net_resolve_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift net resolve build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift net resolve binary");
    assert!(
        run.status.success(),
        "cranelift net resolve binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "true\n");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_net_resolve_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("net-resolve-main-exit");
    write_net_resolve_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift net resolve main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift net resolve main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_net_loopback_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("net-loopback");
    write_net_loopback_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift net loopback build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift net loopback binary");
    assert!(
        run.status.success(),
        "cranelift net loopback binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "true
true
"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_net_loopback_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("net-loopback-main-exit");
    write_net_loopback_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift net loopback main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift net loopback main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_http_client_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("http-client");
    let (port, server) = start_http_fixture_server("axiom-http-ok");
    write_http_client_project(&project, port);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift http client build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    server.join().expect("join http fixture server");

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift http client binary");
    assert!(
        run.status.success(),
        "cranelift http client binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "axiom-http-ok\n");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_http_client_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("http-client-main-exit");
    let (port, server) = start_http_fixture_server_requests("axiom-http-ok", 2);
    write_http_client_main_exit_project(&project, port);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift http client main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    server.join().expect("join http fixture server");

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift http client main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_http_server_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("http-server");
    let Some(port) = reserve_loopback_port() else {
        return;
    };
    write_http_server_project(&project, port);
    let client = start_http_server_probe_client(port);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift http server build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let response = client.join().expect("join http server probe client");
    assert!(
        response.starts_with("HTTP/1.0 201 Created\r\n"),
        "unexpected http server response: {response:?}"
    );
    assert!(
        response.ends_with("axiom-response"),
        "unexpected http server response body: {response:?}"
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift http server binary");
    assert!(
        run.status.success(),
        "cranelift http server binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "true\nPOST\n/server\naxiom-request\ntrue\ntrue\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_http_server_once_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("http-server-once-main-exit");
    let Some(port) = reserve_loopback_port() else {
        return;
    };
    write_http_server_once_main_exit_project(&project, port);
    let client = start_http_route_probe_client(port, "/once");

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift http server once main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let response = client.join().expect("join http server once probe client");
    assert!(
        response.starts_with("HTTP/1.0 200 OK\r\n"),
        "unexpected http server once response: {response:?}"
    );
    assert!(
        response.ends_with("server-once-ok"),
        "unexpected http server once response body: {response:?}"
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift http server once main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_http_server_route_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("http-server-route-main-exit");
    let Some(port) = reserve_loopback_port() else {
        return;
    };
    write_http_server_route_main_exit_project(&project, port);
    let first_client = start_http_route_probe_client(port, "/route");
    let second_client = start_http_route_probe_client(port, "/route");

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift http server route main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for response in [
        first_client.join().expect("join first http route client"),
        second_client.join().expect("join second http route client"),
    ] {
        assert!(
            response.starts_with("HTTP/1.0 200 OK\r\n"),
            "unexpected http server route response: {response:?}"
        );
        assert!(
            response.ends_with("route-ok"),
            "unexpected http server route response body: {response:?}"
        );
    }

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift http server route main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_http_async_server_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("http-async-server");
    let Some(port) = reserve_loopback_port() else {
        return;
    };
    write_http_async_server_project(&project, port);
    let client = start_http_route_probe_client(port, "/ready");

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift http async server build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let response = client.join().expect("join http async server probe client");
    assert!(
        response.starts_with("HTTP/1.0 200 OK\r\n"),
        "unexpected http async server response: {response:?}"
    );
    assert!(
        response.ends_with("async-response"),
        "unexpected http async server response body: {response:?}"
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift http async server binary");
    assert!(
        run.status.success(),
        "cranelift http async server binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "true\ntrue\n");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_crypto_hash_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("crypto-hash");
    write_crypto_hash_project(&project, true);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift crypto hash build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift crypto hash binary");
    assert!(
        run.status.success(),
        "cranelift crypto hash binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_crypto_mac_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("crypto-mac");
    write_crypto_mac_project(&project, true);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift crypto mac build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift crypto mac binary");
    assert!(
        run.status.success(),
        "cranelift crypto mac binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8
164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea2505549758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737
true
true
false
false
true
false
"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_crypto_random_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("crypto-random");
    write_crypto_random_project(&project, true);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift crypto random build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift crypto random binary");
    assert!(
        run.status.success(),
        "cranelift crypto random binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "16
0
true
"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_crypto_random_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("crypto-random-main-exit");
    write_crypto_random_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift crypto random main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let audit_log = temp.path().join("crypto-random-audit.jsonl");
    let run = Command::new(binary)
        .env("AXIOM_HOST_AUDIT_LOG", &audit_log)
        .env("AXIOM_TEST_RANDOM_BYTES", "0123456789abcdef")
        .env("AXIOM_TEST_RANDOM_U64", "48")
        .output()
        .expect("run cranelift crypto random main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    let audit = fs::read_to_string(&audit_log).expect("read crypto random audit log");
    assert!(audit.contains("\"intrinsic\":\"crypto_rand_bytes\""));
    assert!(audit.contains("\"intrinsic\":\"crypto_rand_u64\""));
    assert!(audit.contains("\"length\":\"int:16\""));
    assert!(audit.contains("\"length\":\"int:0\""));
    assert!(audit.contains("\"args\":{}"));
    assert_eq!(audit.matches("\"outcome\":\"ok\"").count(), 3);
    assert!(!audit.contains("\"bytes\""));
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_sync_primitives_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("sync-primitives");
    write_sync_primitives_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift sync primitives build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift sync primitives binary");
    assert!(
        run.status.success(),
        "cranelift sync primitives binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "2
true
empty
message
"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_sync_mutex_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("sync-mutex-main-exit");
    write_sync_mutex_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift sync mutex main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift sync mutex main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_sync_once_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("sync-once-main-exit");
    write_sync_once_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift sync once main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift sync once main binary");
    assert_eq!(run.status.code(), Some(40));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_sync_channel_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("sync-channel-main-exit");
    write_sync_channel_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift sync channel main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift sync channel main binary");
    assert_eq!(run.status.code(), Some(50));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_std_async_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("std-async");
    write_std_async_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift std async build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift std async binary");
    assert!(
        run.status.success(),
        "cranelift std async binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "41
7
true
6
message
1
right
"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_std_async_net_tcp_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("std-async-net-tcp");
    write_std_async_net_tcp_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift std async net TCP build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift std async net TCP binary");
    assert!(
        run.status.success(),
        "cranelift std async net TCP binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "alpha\nbeta\n");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_logging_stdio_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("logging-stdio");
    write_logging_stdio_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift logging stdio build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift logging stdio binary");
    assert!(
        run.status.success(),
        "cranelift logging stdio binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "true\n");
    assert_eq!(String::from_utf8_lossy(&run.stderr), "hello stderr\n");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_known_eprintln_runtime_stderr_in_direct_native_main() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("logging-stdio-main-exit");
    write_logging_stdio_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift logging stdio main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift logging stdio main binary");
    assert_eq!(
        run.status.code(),
        Some(72),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        "after assign\nbranch stderr local\ntail stderr\ntrue\n25\n\"true\"\n\"25\"\ndeploy\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_known_print_runtime_stdout_in_direct_native_main() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("print-stdio-main-exit");
    write_print_stdio_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift print stdio main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift print stdio main binary");
    assert_eq!(run.status.code(), Some(7));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "main stdout\nmain stdout\nstatic stdout\nmain stdout suffix\nbranch stdout local\nhelper stdout\ndeploy\n"
    );
    assert_eq!(String::from_utf8_lossy(&run.stderr), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_bool_print_runtime_stdout_in_direct_native_main() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("bool-print-stdio-main-exit");
    write_bool_print_stdio_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift bool print stdio main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift bool print stdio main binary");
    assert_eq!(run.status.code(), Some(13));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "true\ntrue\n");
    assert_eq!(String::from_utf8_lossy(&run.stderr), "hello stderr\n");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_integer_print_runtime_stdout_in_direct_native_main() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("integer-print-stdio-main-exit");
    write_integer_print_stdio_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift integer print stdio main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift integer print stdio main binary");
    assert_eq!(run.status.code(), Some(42));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n-3\n0\n");
    assert_eq!(String::from_utf8_lossy(&run.stderr), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_json_scalar_stringify_print_to_native_stdout() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("json-stringify-print-main-exit");
    write_json_stringify_print_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift json stringify print build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift json stringify print binary");
    assert_eq!(run.status.code(), Some(42));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "42\n\"42\"\ntrue\nfalse\n\"false\"\n"
    );
    assert_eq!(String::from_utf8_lossy(&run.stderr), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_helper_eprintln_to_native_stderr() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("helper-eprintln-main-exit");
    write_helper_eprintln_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift helper eprintln build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift helper eprintln binary");
    assert_eq!(run.status.code(), Some(90));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        "helper stderr\nhelper stderr\nhelper static\nhelper stderr suffix\nhelper text\n42\ntrue\ndeploy\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_aggregate_helper_eprintln_to_native_stderr() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("aggregate-helper-eprintln-main-exit");
    write_aggregate_helper_eprintln_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift aggregate helper eprintln build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift aggregate helper eprintln binary");
    assert_eq!(run.status.code(), Some(144));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        "aggregate helper\naggregate helper\naggregate static\naggregate helper suffix\naggregate helper text\n31\nfalse\ndeploy\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_helper_print_to_native_stdout() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("helper-print-main-exit");
    write_helper_print_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift helper print build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift helper print binary");
    assert_eq!(run.status.code(), Some(21));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "helper stdout\nhelper stdout\nhelper static\nhelper stdout suffix\nhelper text\n21\ntrue\n22\ndeploy\n"
    );
    assert_eq!(String::from_utf8_lossy(&run.stderr), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_aggregate_helper_print_to_native_stdout() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("aggregate-helper-print-main-exit");
    write_aggregate_helper_print_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift aggregate helper print build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift aggregate helper print binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "aggregate stdout\naggregate stdout\naggregate static\naggregate stdout suffix\naggregate helper text\n17\ndeploy\n"
    );
    assert_eq!(String::from_utf8_lossy(&run.stderr), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_std_log_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("std-log");
    write_std_log_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift std log build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift std log binary");
    assert!(
        run.status.success(),
        "cranelift std log binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "{\"level\":\"info\",\"message\":\"started\",\"attributes\":{\"component\":\"worker\",\"attempt\":2,\"ready\":true}}\ntrue\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        "{\"level\":\"info\",\"message\":\"started\",\"attributes\":{\"component\":\"worker\",\"attempt\":2,\"ready\":true}}\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_std_encoding_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("std-encoding");
    write_std_encoding_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift std encoding build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift std encoding binary");
    assert!(
        run.status.success(),
        "cranelift std encoding binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "hello%20world%2Fone
hello world/one
bad percent
reports%2FApril%202026
q=agent%20path%2Fone
/docs/stage%201%2Fencoding
"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_clock_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("clock");
    write_clock_project(&project, true, false);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift clock build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift clock binary");
    assert!(
        run.status.success(),
        "cranelift clock binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "true\ntrue\ntrue\ntrue\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_zero_clock_sleep_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("clock-sleep-zero-main-exit");
    write_clock_sleep_zero_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift clock sleep zero main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift clock sleep zero main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_std_time_sleep_wrappers_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("std-time-sleep-main-exit");
    write_std_time_sleep_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift std time sleep wrapper main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let audit_log = temp.path().join("clock-audit.jsonl");
    let run = Command::new(binary)
        .env("AXIOM_HOST_AUDIT_LOG", &audit_log)
        .output()
        .expect("run cranelift std time sleep wrapper main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    let audit = fs::read_to_string(&audit_log).expect("read clock audit log");
    assert!(
        audit.contains("\"intrinsic\":\"clock_sleep_ms\""),
        "{audit}"
    );
    assert!(
        audit.contains("\"args\":{\"milliseconds\":\"int\"}"),
        "{audit}"
    );
    assert_eq!(audit.matches("\"outcome\":\"ok\"").count(), 4, "{audit}");
    assert_eq!(
        audit.matches("\"outcome\":\"denied\"").count(),
        2,
        "{audit}"
    );
    assert!(
        !audit.contains("1001") && !audit.contains("-1"),
        "audit log should not contain clock duration values: {audit}"
    );
}

#[test]
fn cranelift_backend_rejects_clock_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("clock-denied");
    write_clock_project(&project, false, false);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift clock denied build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].clock = true"),
        "expected clock capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "clock capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_nonzero_clock_sleep_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("clock-nonzero-sleep");
    write_clock_project(&project, true, true);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        output.status.success(),
        "cranelift nonzero clock sleep build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift nonzero clock sleep binary");
    assert!(
        run.status.success(),
        "cranelift nonzero clock sleep binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "true\ntrue\ntrue\ntrue\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_json_serdes_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("json-serdes");
    write_json_serdes_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift json build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift json binary");
    assert!(
        run.status.success(),
        "cranelift json binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        r#"42
false
"hello"
42
true
{"name":"axiom","count":3,"ready":true}
axiom
3
true
7
false
"7"
{"score":7,"ready":false}
{"score":7,"ready":false}
[7,false,"7"]
{"type":"object","properties":{"name":{"type":"string"},"score":{"type":"integer"},"ready":{"type":"boolean"}}}
7
{"name":"axiom","count":3,"ready":true}
"axiom"
no int
"#,
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_std_serdes_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("std-serdes");
    write_std_serdes_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift std/serdes build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift std/serdes binary");
    assert!(
        run.status.success(),
        "cranelift std/serdes binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        r#"{"count":3,"items":["one",2],"name":"axiom","ready":true}
0
{"count":3,"items":["one",2],"name":"axiom","nested":{"ok":false},"ready":true}
axiom
3
false
one
2
true
false
2
{"count":3,"name":"axiom"}
parse error
"#,
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_std_serdes_known_json_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("std-serdes-known-json-main-exit");
    write_std_serdes_known_json_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift std/serdes known JSON main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift std/serdes known JSON main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_std_cli_no_args_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("std-cli");
    write_std_cli_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift std/cli build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift std/cli binary");
    assert!(
        run.status.success(),
        "cranelift std/cli binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "0
0
missing
"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_rejects_float_map_keys() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("float-map-key");
    write_float_map_key_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift float-keyed map build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("map float keys are not supported by the cranelift spike"),
        "expected float map key diagnostic, got: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_crypto_hash_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("crypto-hash-denied");
    write_crypto_hash_project(&project, false);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift crypto hash denial build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].crypto = true"),
        "expected crypto capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_crypto_mac_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("crypto-mac-denied");
    write_crypto_mac_project(&project, false);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift crypto mac denial build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].crypto = true"),
        "expected crypto capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_crypto_random_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("crypto-random-denied");
    write_crypto_random_project(&project, false);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift crypto random denial build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].crypto = true"),
        "expected crypto capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_crypto_signature_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("crypto-signature-denied");
    write_crypto_signature_project(&project, false);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift crypto signature denial build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].crypto = true"),
        "expected crypto capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_crypto_signature_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("crypto-signature");
    write_crypto_signature_project(&project, true);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift crypto signature build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift crypto signature binary");
    assert!(
        run.status.success(),
        "cranelift crypto signature binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "true\n");
}

#[test]
fn cranelift_backend_rejects_crypto_aead_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("crypto-aead-denied");
    write_crypto_aead_project(&project, false);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift crypto AEAD denial build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].crypto = true"),
        "expected crypto capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[cfg(unix)]
#[test]
fn cranelift_backend_builds_crypto_aead_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("crypto-aead");
    write_crypto_aead_project(&project, true);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift crypto AEAD build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift crypto AEAD binary");
    assert!(
        run.status.success(),
        "cranelift crypto AEAD binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "5\n");
}

#[test]
fn cranelift_backend_rejects_process_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("process-denied");
    write_process_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift process-denied build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].process = true"),
        "expected process capability diagnostic, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial must happen before backend lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_http_client_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("http-client-denied");
    write_http_client_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift http client denial build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].net = true"),
        "expected net capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_ffi_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("ffi-denied");
    write_ffi_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift ffi denial build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].ffi = true"),
        "expected ffi capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_ffi_strlen_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("ffi-strlen");
    write_ffi_strlen_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        output.status.success(),
        "cranelift ffi strlen build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift ffi strlen binary");
    assert!(
        run.status.success(),
        "cranelift ffi strlen binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "5\n0\n");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_ffi_strlen_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("ffi-strlen-main-exit");
    write_ffi_strlen_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift ffi strlen main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let audit_log = temp.path().join("ffi-audit.jsonl");
    let run = Command::new(binary)
        .env("AXIOM_HOST_AUDIT_LOG", &audit_log)
        .output()
        .expect("run cranelift ffi strlen main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    let audit = fs::read_to_string(&audit_log).expect("read ffi audit log");
    assert!(audit.contains("\"intrinsic\":\"ffi_call\""), "{audit}");
    assert!(audit.contains("\"library\":\"c\""), "{audit}");
    assert!(audit.contains("\"symbol\":\"strlen\""), "{audit}");
    assert!(audit.contains("\"value\":\"string\""), "{audit}");
    assert_eq!(audit.matches("\"outcome\":\"ok\"").count(), 7, "{audit}");
    assert!(
        !audit.contains("hello")
            && !audit.contains("direct-native")
            && !audit.contains("helper")
            && !audit.contains("helper-local")
            && !audit.contains("build")
            && !audit.contains("deploy"),
        "audit log should not contain FFI string argument values: {audit}"
    );
}

#[test]
fn cranelift_backend_rejects_http_server_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("http-server-denied");
    write_http_server_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift http server denial build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].net = true"),
        "expected net capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_async_runtime_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("async-runtime-denied");
    write_async_runtime_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift async runtime denial build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].async = true"),
        "expected async capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_capability_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("fs-denied");
    write_fs_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift fs-denied build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].fs = true"),
        "expected capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_tcp_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("tcp-denied");
    write_tcp_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift tcp denied build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].net = true"),
        "expected net capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_dynamic_net_targets_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("dynamic-net-targets");
    write_dynamic_net_targets_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift dynamic-net-targets build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires a string literal listed in [capabilities].net.hosts"),
        "expected dynamic host rejection before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_udp_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("udp-denied");
    write_udp_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift udp denied build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].net = true"),
        "expected net capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_fs_write_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("fs-write-denied");
    write_fs_write_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift fs-write denied build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].fs:write = true"),
        "expected fs:write capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_env_read_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("env-read");
    write_env_read_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .env("AXIOM_CRANELIFT_ENV_READ", "native-env")
        .env_remove("__AXIOM_CRANELIFT_ENV_MISSING__")
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift env build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift env binary");
    assert!(
        run.status.success(),
        "cranelift env binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "native-env\nmissing\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_lowers_env_read_to_runtime_exit_code() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("env-read-main-exit");
    write_env_read_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .env_remove("AXIOM_CRANELIFT_ENV_READ")
        .env_remove("__AXIOM_CRANELIFT_ENV_MISSING__")
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift env main build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let audit_log = temp.path().join("env-audit.jsonl");
    let run = Command::new(binary)
        .env("AXIOM_CRANELIFT_ENV_READ", "runtime-env")
        .env("AXIOM_HOST_AUDIT_LOG", &audit_log)
        .env_remove("__AXIOM_CRANELIFT_ENV_MISSING__")
        .output()
        .expect("run cranelift env main binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    let audit = fs::read_to_string(&audit_log).expect("read env audit log");
    assert!(audit.contains("\"intrinsic\":\"env_get\""), "{audit}");
    assert!(audit.contains("\"outcome\":\"ok\""), "{audit}");
    assert!(audit.contains("\"outcome\":\"denied\""), "{audit}");
    assert!(
        audit.contains("\"args\":{\"key\":\"string:24\"}"),
        "{audit}"
    );
    assert!(
        audit.contains("\"args\":{\"key\":\"string:31\"}"),
        "{audit}"
    );
    assert!(
        !audit.contains("runtime-env"),
        "audit log should not contain environment values: {audit}"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_honors_env_allowlist_at_runtime() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("env-allowlist-main-exit");
    write_env_allowlist_main_exit_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift env allowlist build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let audit_log = temp.path().join("env-allowlist-audit.jsonl");
    let run = Command::new(binary)
        .env("AXIOM_CRANELIFT_ENV_READ", "runtime-env")
        .env("AXIOM_CRANELIFT_ENV_BLOCKED", "blocked-env")
        .env("AXIOM_HOST_AUDIT_LOG", &audit_log)
        .output()
        .expect("run cranelift env allowlist binary");
    assert_eq!(run.status.code(), Some(48));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    let audit = fs::read_to_string(&audit_log).expect("read env allowlist audit log");
    assert!(audit.contains("\"intrinsic\":\"env_get\""), "{audit}");
    assert!(audit.contains("\"outcome\":\"ok\""), "{audit}");
    assert!(audit.contains("\"outcome\":\"denied\""), "{audit}");
    assert!(
        audit.contains("\"args\":{\"key\":\"string:24\"}"),
        "{audit}"
    );
    assert!(
        audit.contains("\"args\":{\"key\":\"string:27\"}"),
        "{audit}"
    );
    for secret in [
        "AXIOM_CRANELIFT_ENV_READ",
        "AXIOM_CRANELIFT_ENV_BLOCKED",
        "runtime-env",
        "blocked-env",
    ] {
        assert!(
            !audit.contains(secret),
            "audit log should not contain environment names or values: {audit}"
        );
    }
}

#[test]
fn cranelift_backend_rejects_env_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("env-denied");
    write_env_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift env-denied build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].env"),
        "expected env capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "env capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_http_async_server_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("http-async-server-denied");
    write_http_async_server_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift http async server denial build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].async = true"),
        "expected async capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

fn copy_fixture(relative: &str, destination: &Path) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/hello")
        .join(relative);
    fs::copy(&fixture, destination).unwrap_or_else(|err| {
        panic!(
            "copy fixture {} to {}: {err}",
            fixture.display(),
            destination.display()
        )
    });
}

fn copy_conformance_fixture(fixture_name: &str, destination: &Path) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/pass")
        .join(fixture_name);
    fs::create_dir_all(destination.join("src")).expect("create conformance project src");
    for relative in ["axiom.toml", "axiom.lock", "src/main_test.ax"] {
        let source = fixture.join(relative);
        let target = destination.join(relative);
        fs::copy(&source, &target).unwrap_or_else(|err| {
            panic!(
                "copy fixture {} to {}: {err}",
                source.display(),
                target.display()
            )
        });
    }
}

fn write_regex_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create regex project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-regex-surface"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = true
clock = false
crypto = false

[unsafe_rationale]
env = "Cranelift ABI regression needs a runtime-only projected key index source."
"#,
    )
    .expect("write regex manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-regex-surface"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write regex lockfile");
    fs::write(
        project.join("src/main.ax"),
        r##"import "std/regex.ax"

print is_match("^h.llo$", "hello")
match find("[0-9]+", "issue-238-ready") {
Some(value) {
print value
}
None {
print "none"
}
}
print replace_all("[0-9]+", "issue-238-ready", "#")
print replace_all("^a", "aa", "x")
print replace_all("^a", "aaa", "x")
print replace_all("^a", "ba", "x")
"##,
    )
    .expect("write regex source");
}

fn write_i64_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create i64 main exit project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-i64-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = true\nclock = false\ncrypto = false\n\n[unsafe_rationale]\nenv = \"Cranelift ABI regression needs a runtime-only projected key index source.\"\n",
    )
    .expect("write i64 main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-i64-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write i64 main exit lockfile");
    fs::write(
        project.join("src/main.ax"),
        "static EXIT_BIAS: int = -26\nstatic STATIC_WIDE: i64 = 7i64\nstatic STATIC_BYTE: u8 = 5u8\nstatic STATIC_DELTA: i8 = 4i8\nstatic STATIC_HALF: u16 = 4u16\nstatic FORCE_EXIT: bool = false\n\nfn sum_to(limit: int): int {\nif limit > 0 {\nreturn limit + sum_to(limit - 1)\n} else {\nreturn 0\n}\n}\n\nfn offset(value: int, bump: int): int {\nreturn value + bump\n}\n\nfn widen(value: int): i64 {\nreturn value as i64\n}\n\nfn normalize(value: i64): int {\nreturn value as int\n}\n\nfn signed_step(value: i32, small: i8, delta: i16, platform: isize): i32 {\nlet combined: i32 = value + (small as i32) + (delta as i32) + (platform as i32)\nreturn combined\n}\n\nfn unsigned_step(value: u32, byte: u8, half: u16): u32 {\nlet combined: u32 = value + (byte as u32) + (half as u32)\nreturn combined\n}\n\nfn wrap_byte(value: u8): u8 {\nreturn value + 1u8\n}\n\nfn wrap_small(value: i8): i8 {\nreturn value + 1i8\n}\n\nfn scale(value: int): int {\nlet adjusted: int = offset(value, 3)\nlet index: int = 0\nlet scaled: int = 0\nwhile index < 2 {\nlet part: int = adjusted\nscaled = scaled + part\nindex = index + 1\n}\nif scaled > 0 {\nlet result: int = scaled\nreturn result\n} else {\nlet fallback: int = 0\nreturn fallback\n}\n}\n\nfn main(): int {\nlet base: int = sum_to(6)\nlet widened: i64 = widen(base)\nlet roundtrip: int = normalize(widened)\nlet signed: int = normalize(STATIC_WIDE)\nlet unsigned: int = STATIC_BYTE as int\nlet signed_seed: i32 = 6i32\nlet signed_delta: i16 = 5i16\nlet signed_platform: isize = 0isize\nlet signed_step_value: int = signed_step(signed_seed, STATIC_DELTA, signed_delta, signed_platform) as int\nlet unsigned_seed: u32 = 8u32\nlet unsigned_byte: u8 = 6u8\nlet unsigned_step_value: int = unsigned_step(unsigned_seed, unsigned_byte, STATIC_HALF) as int\nlet narrowed_unsigned: u8 = 300i64 as u8\nlet narrowed_signed: i8 = 255i64 as i8\nlet wrapped_unsigned_sum: u8 = wrap_byte(255u8)\nlet wrapped_signed_sum: i8 = wrap_small(127i8)\nlet tuple_int: int = (12, 99).0\nlet tuple_typed: int = (1u8, 2i8).1 as int\nlet tuple_adjustment: int = tuple_int + tuple_typed\nlet cast_adjustment: int = (narrowed_unsigned as int) + (narrowed_signed as int)\nlet arithmetic_adjustment: int = (wrapped_unsigned_sum as int) + (wrapped_signed_sum as int)\nlet signed_adjusted: int = roundtrip + signed + unsigned + signed_step_value\nlet scalar_adjusted: int = signed_adjusted + unsigned_step_value + cast_adjustment + arithmetic_adjustment + tuple_adjustment\nlet normalized: int = scalar_adjusted - EXIT_BIAS\nlet scaled: int = scale(normalized)\nlet enough: bool = scaled > 40\nlet exact: bool = normalized == 21\nif (enough && exact) || FORCE_EXIT {\nreturn scaled\n} else {\nreturn 1\n}\n}\n",
    )
    .expect("write i64 main exit source");
}

fn write_i64_returning_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create i64 returning main exit project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-i64-returning-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = true\nclock = false\ncrypto = false\n\n[unsafe_rationale]\nenv = \"Cranelift ABI regression needs a runtime-only projected key index source.\"\n",
    )
    .expect("write i64 returning main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-i64-returning-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write i64 returning main exit lockfile");
    fs::write(
        project.join("src/main.ax"),
        "fn main(): i64 {\nreturn 48i64\n}\n",
    )
    .expect("write i64 returning main exit source");
}

fn write_terminal_panic_project(project: &Path, source: &str) {
    fs::create_dir_all(project.join("src")).expect("create terminal panic project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-terminal-panic\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write terminal panic manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-terminal-panic\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write terminal panic lockfile");
    fs::write(project.join("src/main.ax"), source).expect("write terminal panic source");
}

fn assert_terminal_panic_report(project: &Path, expected_stderr: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift terminal panic build failed for {}: stdout={} stderr={}",
        project.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["generated_rust"], Value::Null);
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift terminal panic binary");
    assert_eq!(run.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "");
    assert_eq!(String::from_utf8_lossy(&run.stderr), expected_stderr);
}

fn write_typed_numeric_returning_main_exit_project(
    project: &Path,
    package_name: &str,
    return_ty: &str,
    literal: &str,
) {
    fs::create_dir_all(project.join("src")).expect("create typed numeric main exit project src");
    fs::write(
        project.join("axiom.toml"),
        format!(
            "[package]\nname = \"{package_name}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
        ),
    )
    .expect("write typed numeric main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        format!(
            "version = 1\n\n[[package]]\nname = \"{package_name}\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
        ),
    )
    .expect("write typed numeric main exit lockfile");
    fs::write(
        project.join("src/main.ax"),
        format!("fn main(): {return_ty} {{\nreturn {literal}\n}}\n"),
    )
    .expect("write typed numeric main exit source");
}

fn write_option_int_match_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create option int match main exit project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-option-int-match-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write option int match main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-option-int-match-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write option int match main exit lockfile");
    fs::write(
        project.join("src/main.ax"),
        "fn score(value: Option<int>): int {\nreturn match value { Some(payload) => payload, None => 1 }\n}\n\nfn main(): int {\nlet ready: Option<int> = None\nlet index: int = 0\nwhile index < 1 {\nready = Some(48)\nindex = index + 1\n}\nlet exit_code: int = match ready { Some(value) => value, None => 1 }\nlet statement_code: int = 0\nmatch ready {\nSome(value) {\nstatement_code = value\n}\nNone {\nstatement_code = 1\n}\n}\nlet helper_code: int = score(ready)\nlet literal_code: int = score(Some(48))\nlet none_code: int = score(None)\nif exit_code == statement_code && helper_code == literal_code && none_code == 1 {\nreturn statement_code\n} else {\nreturn 1\n}\n}\n",
    )
    .expect("write option int match main exit source");
}

fn write_option_bool_match_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src"))
        .expect("create option bool match main exit project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-option-bool-match-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write option bool match main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-option-bool-match-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write option bool match main exit lockfile");
    fs::write(
        project.join("src/main.ax"),
        "fn enabled(value: Option<bool>): bool {\nreturn match value { Some(payload) => payload, None => false }\n}\n\nfn main(): bool {\nlet ready: Option<bool> = None\nlet index: int = 0\nwhile index < 1 {\nready = Some(true)\nindex = index + 1\n}\nlet exit_ok: bool = match ready { Some(value) => value, None => false }\nlet statement_ok: bool = false\nmatch ready {\nSome(value) {\nstatement_ok = value\n}\nNone {\nstatement_ok = false\n}\n}\nlet helper_ok: bool = enabled(ready)\nlet literal_ok: bool = enabled(Some(true))\nlet none_ok: bool = enabled(None) == false\nreturn exit_ok && statement_ok && helper_ok && literal_ok && none_ok\n}\n",
    )
    .expect("write option bool match main exit source");
}

fn write_option_tuple_payload_match_main_exit_project(project: &Path, variant: &str) {
    fs::create_dir_all(project.join("src"))
        .expect("create option tuple payload match main exit project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-option-tuple-payload-match-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write option tuple payload match main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-option-tuple-payload-match-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write option tuple payload match main exit lockfile");
    let value = if variant == "Some" {
        "Some((48, true))"
    } else {
        "None"
    };
    fs::write(
        project.join("src/main.ax"),
        format!("fn choose_option(flag: bool): Option<(int, bool)> {{\nif flag {{\nlet value: int = 48\nreturn Some((value, true))\n}} else {{\nreturn None\n}}\n}}\n\nfn forward_option(value: Option<(int, bool)>): Option<(int, bool)> {{\nreturn value\n}}\n\nfn score(value: Option<(int, bool)>): int {{\nreturn match value {{ Some(pair) => pair.0, None => 49 }}\n}}\n\nfn main(): int {{\nlet ready: Option<(int, bool)> = None\nready = {value}\nlet returned_some: Option<(int, bool)> = choose_option(true)\nlet returned_none: Option<(int, bool)> = choose_option(false)\nlet forwarded_some: Option<(int, bool)> = forward_option(returned_some)\nlet forwarded_none: Option<(int, bool)> = forward_option(returned_none)\nlet match_code: int = match ready {{ Some(pair) => pair.0, None => 49 }}\nlet statement_code: int = 0\nmatch ready {{\nSome(pair) {{\nif pair.1 {{\nstatement_code = pair.0\n}} else {{\nstatement_code = 1\n}}\n}}\nNone {{\nstatement_code = 49\n}}\n}}\nlet helper_code: int = score(ready)\nlet returned_some_code: int = score(returned_some)\nlet returned_none_code: int = score(returned_none)\nlet forwarded_some_code: int = score(forwarded_some)\nlet forwarded_none_code: int = score(forwarded_none)\nlet literal_some_code: int = score(Some((48, true)))\nlet literal_none_code: int = score(None)\nif match_code == statement_code && statement_code == helper_code && returned_some_code == 48 && returned_none_code == 49 && forwarded_some_code == 48 && forwarded_none_code == 49 && literal_some_code == 48 && literal_none_code == 49 {{\nreturn match_code\n}} else {{\nreturn 1\n}}\n}}\n"),
    )
    .expect("write option tuple payload match main exit source");
}

fn write_option_array_payload_match_main_exit_project(project: &Path, variant: &str) {
    fs::create_dir_all(project.join("src"))
        .expect("create option array payload match main exit project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-option-array-payload-match-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write option array payload match main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-option-array-payload-match-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write option array payload match main exit lockfile");
    let value = if variant == "Some" {
        "Some([20, 28])"
    } else {
        "None"
    };
    fs::write(
        project.join("src/main.ax"),
        format!("fn choose_option(flag: bool): Option<[int; 2]> {{\nif flag {{\nreturn Some([20, 28])\n}} else {{\nreturn None\n}}\n}}\n\nfn forward_option(value: Option<[int; 2]>): Option<[int; 2]> {{\nreturn value\n}}\n\nfn score(value: Option<[int; 2]>): int {{\nreturn match value {{ Some(values) => values[0] + values[1], None => 49 }}\n}}\n\nfn main(): int {{\nlet ready_for_match: Option<[int; 2]> = {value}\nlet ready_for_statement: Option<[int; 2]> = {value}\nlet ready_for_helper: Option<[int; 2]> = {value}\nlet returned_some_for_score: Option<[int; 2]> = choose_option(true)\nlet returned_none_for_score: Option<[int; 2]> = choose_option(false)\nlet returned_some_for_forward: Option<[int; 2]> = choose_option(true)\nlet returned_none_for_forward: Option<[int; 2]> = choose_option(false)\nlet forwarded_some: Option<[int; 2]> = forward_option(returned_some_for_forward)\nlet forwarded_none: Option<[int; 2]> = forward_option(returned_none_for_forward)\nlet match_code: int = match ready_for_match {{ Some(values) => values[0] + values[1], None => 49 }}\nlet statement_code: int = 0\nmatch ready_for_statement {{\nSome(values) {{\nstatement_code = values[0] + values[1]\n}}\nNone {{\nstatement_code = 49\n}}\n}}\nlet helper_code: int = score(ready_for_helper)\nlet returned_some_code: int = score(returned_some_for_score)\nlet returned_none_code: int = score(returned_none_for_score)\nlet forwarded_some_code: int = score(forwarded_some)\nlet forwarded_none_code: int = score(forwarded_none)\nlet literal_some_code: int = score(Some([20, 28]))\nlet literal_none_code: int = score(None)\nif match_code == statement_code && statement_code == helper_code && returned_some_code == 48 && returned_none_code == 49 && forwarded_some_code == 48 && forwarded_none_code == 49 && literal_some_code == 48 && literal_none_code == 49 {{\nreturn match_code\n}} else {{\nreturn 1\n}}\n}}\n"),
    )
    .expect("write option array payload match main exit source");
}

fn write_option_struct_payload_match_main_exit_project(project: &Path, variant: &str) {
    fs::create_dir_all(project.join("src"))
        .expect("create option struct payload match main exit project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-option-struct-payload-match-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write option struct payload match main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-option-struct-payload-match-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write option struct payload match main exit lockfile");
    let value = if variant == "Some" {
        "Some(Step { value: 48, enabled: true, small: 2u8 })"
    } else {
        "None"
    };
    fs::write(
        project.join("src/main.ax"),
        format!("struct Step {{\nvalue: int\nenabled: bool\nsmall: u8\n}}\n\nfn choose_option(flag: bool): Option<Step> {{\nif flag {{\nreturn Some(Step {{ value: 48, enabled: true, small: 2u8 }})\n}} else {{\nreturn None\n}}\n}}\n\nfn forward_option(value: Option<Step>): Option<Step> {{\nreturn value\n}}\n\nfn score(value: Option<Step>): int {{\nreturn match value {{ Some(step) => step.value, None => 49 }}\n}}\n\nfn main(): int {{\nlet ready_for_match: Option<Step> = {value}\nlet ready_for_statement: Option<Step> = {value}\nlet ready_for_helper: Option<Step> = {value}\nlet returned_some_for_score: Option<Step> = choose_option(true)\nlet returned_none_for_score: Option<Step> = choose_option(false)\nlet returned_some_for_forward: Option<Step> = choose_option(true)\nlet returned_none_for_forward: Option<Step> = choose_option(false)\nlet forwarded_some: Option<Step> = forward_option(returned_some_for_forward)\nlet forwarded_none: Option<Step> = forward_option(returned_none_for_forward)\nlet match_code: int = match ready_for_match {{ Some(step) => step.value, None => 49 }}\nlet statement_code: int = 0\nmatch ready_for_statement {{\nSome(step) {{\nif step.enabled {{\nstatement_code = step.value\n}} else {{\nstatement_code = 1\n}}\n}}\nNone {{\nstatement_code = 49\n}}\n}}\nlet helper_code: int = score(ready_for_helper)\nlet returned_some_code: int = score(returned_some_for_score)\nlet returned_none_code: int = score(returned_none_for_score)\nlet forwarded_some_code: int = score(forwarded_some)\nlet forwarded_none_code: int = score(forwarded_none)\nlet literal_some_code: int = score(Some(Step {{ small: 2u8, enabled: true, value: 48 }}))\nlet literal_none_code: int = score(None)\nif match_code == statement_code && statement_code == helper_code && returned_some_code == 48 && returned_none_code == 49 && forwarded_some_code == 48 && forwarded_none_code == 49 && literal_some_code == 48 && literal_none_code == 49 {{\nreturn match_code\n}} else {{\nreturn 1\n}}\n}}\n"),
    )
    .expect("write option struct payload match main exit source");
}

fn write_nested_option_match_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create nested option match main exit src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-nested-option-match-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write nested option match main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-nested-option-match-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write nested option match main exit lockfile");
    fs::write(
        project.join("src/main.ax"),
        "fn choose_nested(flag: bool): Option<Option<int>> {\nif flag {\nreturn Some(Some(48))\n} else {\nreturn Some(None)\n}\n}\n\nfn forward_nested(value: Option<Option<int>>): Option<Option<int>> {\nreturn value\n}\n\nfn score(value: Option<Option<int>>): int {\nreturn match value { Some(inner) => match inner { Some(payload) => payload, None => 49 }, None => 1 }\n}\n\nfn choose_result(flag: bool): Result<Option<int>, int> {\nif flag {\nreturn Ok(Some(48))\n} else {\nreturn Ok(None)\n}\n}\n\nfn forward_result(value: Result<Option<int>, int>): Result<Option<int>, int> {\nreturn value\n}\n\nfn score_result(value: Result<Option<int>, int>): int {\nreturn match value { Ok(inner) => match inner { Some(payload) => payload, None => 49 }, Err(error) => error }\n}\n\nfn main(): int {\nlet ready: Option<Option<int>> = None\nready = Some(Some(48))\nlet returned_some: Option<Option<int>> = choose_nested(true)\nlet returned_none: Option<Option<int>> = choose_nested(false)\nlet forwarded_some: Option<Option<int>> = forward_nested(returned_some)\nlet forwarded_none: Option<Option<int>> = forward_nested(returned_none)\nlet result_ready: Result<Option<int>, int> = Err(1)\nresult_ready = Ok(Some(48))\nlet result_returned_some: Result<Option<int>, int> = choose_result(true)\nlet result_returned_none: Result<Option<int>, int> = choose_result(false)\nlet result_forwarded_some: Result<Option<int>, int> = forward_result(result_returned_some)\nlet result_forwarded_none: Result<Option<int>, int> = forward_result(result_returned_none)\nlet match_code: int = match ready { Some(inner) => match inner { Some(payload) => payload, None => 49 }, None => 1 }\nlet helper_code: int = score(ready)\nlet returned_some_code: int = score(returned_some)\nlet returned_none_code: int = score(returned_none)\nlet forwarded_some_code: int = score(forwarded_some)\nlet forwarded_none_code: int = score(forwarded_none)\nlet inline_some_code: int = score(Some(Some(48)))\nlet inline_inner_none_code: int = score(Some(None))\nlet inline_outer_none_code: int = score(None)\nlet result_match_code: int = match result_ready { Ok(inner) => match inner { Some(payload) => payload, None => 49 }, Err(error) => error }\nlet result_helper_code: int = score_result(result_ready)\nlet result_returned_some_code: int = score_result(result_returned_some)\nlet result_returned_none_code: int = score_result(result_returned_none)\nlet result_forwarded_some_code: int = score_result(result_forwarded_some)\nlet result_forwarded_none_code: int = score_result(result_forwarded_none)\nlet result_inline_some_code: int = score_result(Ok(Some(48)))\nlet result_inline_inner_none_code: int = score_result(Ok(None))\nlet result_inline_err_code: int = score_result(Err(1))\nif match_code == 48 && helper_code == 48 && returned_some_code == 48 && returned_none_code == 49 && forwarded_some_code == 48 && forwarded_none_code == 49 && inline_some_code == 48 && inline_inner_none_code == 49 && inline_outer_none_code == 1 && result_match_code == 48 && result_helper_code == 48 && result_returned_some_code == 48 && result_returned_none_code == 49 && result_forwarded_some_code == 48 && result_forwarded_none_code == 49 && result_inline_some_code == 48 && result_inline_inner_none_code == 49 && result_inline_err_code == 1 {\nreturn match_code\n} else {\nreturn 2\n}\n}\n",
    )
    .expect("write nested option match main exit source");
}

fn write_nested_result_match_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create nested result match main exit src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-nested-result-match-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write nested result match main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-nested-result-match-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write nested result match main exit lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"fn choose_option_result(flag: bool): Option<Result<int, int>> {
if flag {
return Some(Ok(48))
} else {
return Some(Err(49))
}
}

fn forward_option_result(value: Option<Result<int, int>>): Option<Result<int, int>> {
return value
}

fn score_option_result(value: Option<Result<int, int>>): int {
return match value { Some(inner) => match inner { Ok(payload) => payload, Err(error) => error }, None => 1 }
}

fn choose_result_result(flag: bool): Result<Result<int, int>, int> {
if flag {
return Ok(Ok(48))
} else {
return Ok(Err(49))
}
}

fn forward_result_result(value: Result<Result<int, int>, int>): Result<Result<int, int>, int> {
return value
}

fn score_result_result(value: Result<Result<int, int>, int>): int {
return match value { Ok(inner) => match inner { Ok(payload) => payload, Err(error) => error }, Err(error) => error }
}

fn main(): int {
let option_ready: Option<Result<int, int>> = None
option_ready = Some(Ok(48))
let option_returned_ok: Option<Result<int, int>> = choose_option_result(true)
let option_returned_err: Option<Result<int, int>> = choose_option_result(false)
let option_forwarded_ok: Option<Result<int, int>> = forward_option_result(option_returned_ok)
let option_forwarded_err: Option<Result<int, int>> = forward_option_result(option_returned_err)
let result_ready: Result<Result<int, int>, int> = Err(1)
result_ready = Ok(Ok(48))
let result_returned_ok: Result<Result<int, int>, int> = choose_result_result(true)
let result_returned_err: Result<Result<int, int>, int> = choose_result_result(false)
let result_forwarded_ok: Result<Result<int, int>, int> = forward_result_result(result_returned_ok)
let result_forwarded_err: Result<Result<int, int>, int> = forward_result_result(result_returned_err)
let option_match_code: int = match option_ready { Some(inner) => match inner { Ok(payload) => payload, Err(error) => error }, None => 1 }
let option_helper_code: int = score_option_result(option_ready)
let option_returned_ok_code: int = score_option_result(option_returned_ok)
let option_returned_err_code: int = score_option_result(option_returned_err)
let option_forwarded_ok_code: int = score_option_result(option_forwarded_ok)
let option_forwarded_err_code: int = score_option_result(option_forwarded_err)
let option_inline_ok_code: int = score_option_result(Some(Ok(48)))
let option_inline_err_code: int = score_option_result(Some(Err(49)))
let option_inline_none_code: int = score_option_result(None)
let result_match_code: int = match result_ready { Ok(inner) => match inner { Ok(payload) => payload, Err(error) => error }, Err(error) => error }
let result_helper_code: int = score_result_result(result_ready)
let result_returned_ok_code: int = score_result_result(result_returned_ok)
let result_returned_err_code: int = score_result_result(result_returned_err)
let result_forwarded_ok_code: int = score_result_result(result_forwarded_ok)
let result_forwarded_err_code: int = score_result_result(result_forwarded_err)
let result_inline_ok_code: int = score_result_result(Ok(Ok(48)))
let result_inline_err_code: int = score_result_result(Ok(Err(49)))
let result_inline_outer_err_code: int = score_result_result(Err(1))
if option_match_code == 48 && option_helper_code == 48 && option_returned_ok_code == 48 && option_returned_err_code == 49 && option_forwarded_ok_code == 48 && option_forwarded_err_code == 49 && option_inline_ok_code == 48 && option_inline_err_code == 49 && option_inline_none_code == 1 && result_match_code == 48 && result_helper_code == 48 && result_returned_ok_code == 48 && result_returned_err_code == 49 && result_forwarded_ok_code == 48 && result_forwarded_err_code == 49 && result_inline_ok_code == 48 && result_inline_err_code == 49 && result_inline_outer_err_code == 1 {
return option_match_code
} else {
return 2
}
}
"#,
    )
    .expect("write nested result match main exit source");
}

fn write_enum_nested_payload_match_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src"))
        .expect("create enum nested payload match main exit src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-enum-nested-payload-match-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write enum nested payload match main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-enum-nested-payload-match-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write enum nested payload match main exit lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"enum Choice {
Wrapped(Option<Result<int, int>>)
Done(Result<Option<int>, int>)
Off
}

fn choose_wrapped(flag: bool): Choice {
if flag {
return Wrapped(Some(Ok(48)))
} else {
return Wrapped(Some(Err(49)))
}
}

fn choose_done(flag: bool): Choice {
if flag {
return Done(Ok(Some(48)))
} else {
return Done(Ok(None))
}
}

fn forward_choice(value: Choice): Choice {
return value
}

fn score(choice: Choice): int {
return match choice { Wrapped(nested) => match nested { Some(inner) => match inner { Ok(payload) => payload, Err(error) => error }, None => 1 }, Done(nested) => match nested { Ok(inner) => match inner { Some(payload) => payload, None => 49 }, Err(error) => error }, Off => 1 }
}

fn main(): int {
let wrapped: Choice = Off
wrapped = Wrapped(Some(Ok(48)))
let done: Choice = Off
done = Done(Ok(Some(48)))
let returned_wrapped_ok_for_score: Choice = choose_wrapped(true)
let returned_wrapped_err_for_score: Choice = choose_wrapped(false)
let returned_done_some_for_score: Choice = choose_done(true)
let returned_done_none_for_score: Choice = choose_done(false)
let returned_wrapped_ok_for_forward: Choice = choose_wrapped(true)
let returned_done_some_for_forward: Choice = choose_done(true)
let forwarded_wrapped_ok: Choice = forward_choice(returned_wrapped_ok_for_forward)
let forwarded_done_some: Choice = forward_choice(returned_done_some_for_forward)
let wrapped_code: int = score(wrapped)
let done_code: int = score(done)
let returned_wrapped_ok_code: int = score(returned_wrapped_ok_for_score)
let returned_wrapped_err_code: int = score(returned_wrapped_err_for_score)
let returned_done_some_code: int = score(returned_done_some_for_score)
let returned_done_none_code: int = score(returned_done_none_for_score)
let forwarded_wrapped_ok_code: int = score(forwarded_wrapped_ok)
let forwarded_done_some_code: int = score(forwarded_done_some)
let inline_wrapped_ok_code: int = score(Wrapped(Some(Ok(48))))
let inline_wrapped_err_code: int = score(Wrapped(Some(Err(49))))
let inline_wrapped_none_code: int = score(Wrapped(None))
let inline_done_some_code: int = score(Done(Ok(Some(48))))
let inline_done_none_code: int = score(Done(Ok(None)))
let inline_done_err_code: int = score(Done(Err(1)))
if wrapped_code == 48 && done_code == 48 && returned_wrapped_ok_code == 48 && returned_wrapped_err_code == 49 && returned_done_some_code == 48 && returned_done_none_code == 49 && forwarded_wrapped_ok_code == 48 && forwarded_done_some_code == 48 && inline_wrapped_ok_code == 48 && inline_wrapped_err_code == 49 && inline_wrapped_none_code == 1 && inline_done_some_code == 48 && inline_done_none_code == 49 && inline_done_err_code == 1 {
return wrapped_code
} else {
return 2
}
}
"#,
    )
    .expect("write enum nested payload match main exit source");
}

fn write_result_match_main_exit_project(
    project: &Path,
    package_name: &str,
    variant: &str,
    payload: i32,
) {
    fs::create_dir_all(project.join("src")).expect("create result match main exit project src");
    fs::write(
        project.join("axiom.toml"),
        format!("[package]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n"),
    )
    .expect("write result match main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        format!("version = 1\n\n[[package]]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\nsource = \"path\"\n"),
    )
    .expect("write result match main exit lockfile");
    fs::write(
        project.join("src/main.ax"),
        format!("fn score(value: Result<int, int>): int {{\nreturn match value {{ Ok(payload) => payload, Err(error) => error }}\n}}\n\nfn main(): int {{\nlet ready: Result<int, int> = Ok(1)\nready = {variant}({payload})\nlet match_code: int = match ready {{ Ok(value) => value, Err(error) => error }}\nlet statement_code: int = 0\nmatch ready {{\nOk(value) {{\nstatement_code = value\n}}\nErr(error) {{\nstatement_code = error\n}}\n}}\nlet helper_code: int = score(ready)\nlet literal_code: int = score({variant}({payload}))\nif match_code == statement_code && statement_code == helper_code && helper_code == literal_code {{\nreturn match_code\n}} else {{\nreturn 1\n}}\n}}\n"),
    )
    .expect("write result match main exit source");
}

fn write_result_bool_match_main_exit_project(project: &Path, package_name: &str, variant: &str) {
    fs::create_dir_all(project.join("src"))
        .expect("create result bool match main exit project src");
    fs::write(
        project.join("axiom.toml"),
        format!("[package]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n"),
    )
    .expect("write result bool match main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        format!("version = 1\n\n[[package]]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\nsource = \"path\"\n"),
    )
    .expect("write result bool match main exit lockfile");
    fs::write(
        project.join("src/main.ax"),
        format!("fn enabled(value: Result<bool, bool>): bool {{\nreturn match value {{ Ok(payload) => payload, Err(error) => error }}\n}}\n\nfn main(): bool {{\nlet ready: Result<bool, bool> = Ok(false)\nready = {variant}(true)\nlet match_ok: bool = match ready {{ Ok(value) => value, Err(error) => error }}\nlet statement_ok: bool = false\nmatch ready {{\nOk(value) {{\nstatement_ok = value\n}}\nErr(error) {{\nstatement_ok = error\n}}\n}}\nlet helper_ok: bool = enabled(ready)\nlet literal_ok: bool = enabled({variant}(true))\nlet false_ok: bool = enabled(Ok(false)) == false\nlet false_err: bool = enabled(Err(false)) == false\nreturn match_ok && statement_ok && helper_ok && literal_ok && false_ok && false_err\n}}\n"),
    )
    .expect("write result bool match main exit source");
}

fn write_result_mixed_match_main_exit_project(project: &Path, package_name: &str, variant: &str) {
    fs::create_dir_all(project.join("src"))
        .expect("create result mixed match main exit project src");
    fs::write(
        project.join("axiom.toml"),
        format!("[package]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n"),
    )
    .expect("write result mixed match main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        format!("version = 1\n\n[[package]]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\nsource = \"path\"\n"),
    )
    .expect("write result mixed match main exit lockfile");
    let value = if variant == "Ok" {
        "Ok(48)"
    } else {
        "Err(true)"
    };
    fs::write(
        project.join("src/main.ax"),
        format!("fn score(value: Result<int, bool>): int {{\nreturn match value {{ Ok(payload) => payload, Err(error) => 49 }}\n}}\n\nfn main(): int {{\nlet ready: Result<int, bool> = Ok(1)\nready = {value}\nlet match_code: int = match ready {{ Ok(value) => value, Err(error) => 49 }}\nlet statement_ok: bool = false\nmatch ready {{\nOk(value) {{\nstatement_ok = value == 48\n}}\nErr(error) {{\nstatement_ok = error\n}}\n}}\nlet helper_code: int = score(ready)\nlet literal_ok_code: int = score(Ok(48))\nlet literal_err_code: int = score(Err(false))\nif statement_ok && match_code == helper_code && literal_ok_code == 48 && literal_err_code == 49 {{\nreturn match_code\n}} else {{\nreturn 1\n}}\n}}\n"),
    )
    .expect("write result mixed match main exit source");
}

fn write_result_mixed_reverse_match_main_exit_project(
    project: &Path,
    package_name: &str,
    variant: &str,
) {
    fs::create_dir_all(project.join("src"))
        .expect("create result mixed reverse match main exit project src");
    fs::write(
        project.join("axiom.toml"),
        format!("[package]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n"),
    )
    .expect("write result mixed reverse match main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        format!("version = 1\n\n[[package]]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\nsource = \"path\"\n"),
    )
    .expect("write result mixed reverse match main exit lockfile");
    let value = if variant == "Ok" {
        "Ok(true)"
    } else {
        "Err(49)"
    };
    fs::write(
        project.join("src/main.ax"),
        format!("fn enabled(value: Result<bool, int>): bool {{\nreturn match value {{ Ok(payload) => payload, Err(error) => error == 49 }}\n}}\n\nfn main(): bool {{\nlet ready: Result<bool, int> = Ok(false)\nready = {value}\nlet match_ok: bool = match ready {{ Ok(value) => value, Err(error) => error == 49 }}\nlet statement_ok: bool = false\nmatch ready {{\nOk(value) {{\nstatement_ok = value\n}}\nErr(error) {{\nstatement_ok = error == 49\n}}\n}}\nlet helper_ok: bool = enabled(ready)\nlet literal_ok: bool = enabled(Ok(true))\nlet literal_err: bool = enabled(Err(49))\nlet false_ok: bool = enabled(Ok(false)) == false\nlet false_err: bool = enabled(Err(1)) == false\nreturn match_ok && statement_ok && helper_ok && literal_ok && literal_err && false_ok && false_err\n}}\n"),
    )
    .expect("write result mixed reverse match main exit source");
}

fn write_result_typed_numeric_match_main_exit_project(
    project: &Path,
    package_name: &str,
    variant: &str,
) {
    fs::create_dir_all(project.join("src"))
        .expect("create result typed numeric match main exit project src");
    fs::write(
        project.join("axiom.toml"),
        format!("[package]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n"),
    )
    .expect("write result typed numeric match main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        format!("version = 1\n\n[[package]]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\nsource = \"path\"\n"),
    )
    .expect("write result typed numeric match main exit lockfile");
    let value = if variant == "Ok" {
        "Ok(48i32)"
    } else {
        "Err(49u32)"
    };
    fs::write(
        project.join("src/main.ax"),
        format!("fn score(value: Result<i32, u32>): int {{\nreturn match value {{ Ok(payload) => payload as int, Err(error) => error as int }}\n}}\n\nfn main(): int {{\nlet ready: Result<i32, u32> = Ok(1i32)\nready = {value}\nlet match_code: int = match ready {{ Ok(value) => value as int, Err(error) => error as int }}\nlet statement_code: int = 0\nmatch ready {{\nOk(value) {{\nstatement_code = value as int\n}}\nErr(error) {{\nstatement_code = error as int\n}}\n}}\nlet helper_code: int = score(ready)\nlet literal_ok_code: int = score(Ok(48i32))\nlet literal_err_code: int = score(Err(49u32))\nif match_code == statement_code && statement_code == helper_code && literal_ok_code == 48 && literal_err_code == 49 {{\nreturn match_code\n}} else {{\nreturn 1\n}}\n}}\n"),
    )
    .expect("write result typed numeric match main exit source");
}

fn write_result_numeric_width_match_main_exit_project(
    project: &Path,
    package_name: &str,
    ok_ty: &str,
    err_ty: &str,
    ok_literal: &str,
    err_literal: &str,
    variant: &str,
) {
    fs::create_dir_all(project.join("src"))
        .expect("create result numeric width match main exit project src");
    fs::write(
        project.join("axiom.toml"),
        format!("[package]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n"),
    )
    .expect("write result numeric width match main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        format!("version = 1\n\n[[package]]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\nsource = \"path\"\n"),
    )
    .expect("write result numeric width match main exit lockfile");
    let value = if variant == "Ok" {
        format!("Ok({ok_literal})")
    } else {
        format!("Err({err_literal})")
    };
    fs::write(
        project.join("src/main.ax"),
        format!("fn score(value: Result<{ok_ty}, {err_ty}>): int {{\nreturn match value {{ Ok(payload) => payload as int, Err(error) => error as int }}\n}}\n\nfn main(): int {{\nlet ready: Result<{ok_ty}, {err_ty}> = Ok({ok_literal})\nready = {value}\nlet match_code: int = match ready {{ Ok(value) => value as int, Err(error) => error as int }}\nlet statement_code: int = 0\nmatch ready {{\nOk(value) {{\nstatement_code = value as int\n}}\nErr(error) {{\nstatement_code = error as int\n}}\n}}\nlet helper_code: int = score(ready)\nlet literal_ok_code: int = score(Ok({ok_literal}))\nlet literal_err_code: int = score(Err({err_literal}))\nif match_code == statement_code && statement_code == helper_code && literal_ok_code == 48 && literal_err_code == 49 {{\nreturn match_code\n}} else {{\nreturn 1\n}}\n}}\n"),
    )
    .expect("write result numeric width match main exit source");
}

fn write_result_tuple_payload_match_main_exit_project(
    project: &Path,
    package_name: &str,
    variant: &str,
) {
    fs::create_dir_all(project.join("src"))
        .expect("create result tuple payload match main exit project src");
    fs::write(
        project.join("axiom.toml"),
        format!("[package]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n"),
    )
    .expect("write result tuple payload match main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        format!("version = 1\n\n[[package]]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\nsource = \"path\"\n"),
    )
    .expect("write result tuple payload match main exit lockfile");
    let value = if variant == "Ok" {
        "Ok((48, true))"
    } else {
        "Err(49)"
    };
    fs::write(
        project.join("src/main.ax"),
        format!("fn choose_result(flag: bool): Result<(int, bool), int> {{\nif flag {{\nlet value: int = 48\nreturn Ok((value, true))\n}} else {{\nreturn Err(49)\n}}\n}}\n\nfn forward_result(value: Result<(int, bool), int>): Result<(int, bool), int> {{\nreturn value\n}}\n\nfn score(value: Result<(int, bool), int>): int {{\nreturn match value {{ Ok(pair) => pair.0, Err(error) => error }}\n}}\n\nfn main(): int {{\nlet ready: Result<(int, bool), int> = Ok((1, false))\nready = {value}\nlet returned_ok: Result<(int, bool), int> = choose_result(true)\nlet returned_err: Result<(int, bool), int> = choose_result(false)\nlet forwarded_ok: Result<(int, bool), int> = forward_result(returned_ok)\nlet forwarded_err: Result<(int, bool), int> = forward_result(returned_err)\nlet match_code: int = match ready {{ Ok(pair) => pair.0, Err(error) => error }}\nlet statement_code: int = 0\nmatch ready {{\nOk(pair) {{\nif pair.1 {{\nstatement_code = pair.0\n}} else {{\nstatement_code = 1\n}}\n}}\nErr(error) {{\nstatement_code = error\n}}\n}}\nlet helper_code: int = score(ready)\nlet returned_ok_code: int = score(returned_ok)\nlet returned_err_code: int = score(returned_err)\nlet forwarded_ok_code: int = score(forwarded_ok)\nlet forwarded_err_code: int = score(forwarded_err)\nlet literal_ok_code: int = score(Ok((48, true)))\nlet literal_err_code: int = score(Err(49))\nif match_code == statement_code && statement_code == helper_code && returned_ok_code == 48 && returned_err_code == 49 && forwarded_ok_code == 48 && forwarded_err_code == 49 && literal_ok_code == 48 && literal_err_code == 49 {{\nreturn match_code\n}} else {{\nreturn 1\n}}\n}}\n"),
    )
    .expect("write result tuple payload match main exit source");
}

fn write_result_dual_tuple_payload_match_main_exit_project(
    project: &Path,
    package_name: &str,
    variant: &str,
) {
    fs::create_dir_all(project.join("src"))
        .expect("create result dual tuple payload match main exit project src");
    fs::write(
        project.join("axiom.toml"),
        format!("[package]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n"),
    )
    .expect("write result dual tuple payload match main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        format!("version = 1\n\n[[package]]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\nsource = \"path\"\n"),
    )
    .expect("write result dual tuple payload match main exit lockfile");
    let value = if variant == "Ok" {
        "Ok((48, true))"
    } else {
        "Err((49, true))"
    };
    fs::write(
        project.join("src/main.ax"),
        format!("fn score(value: Result<(int, bool), (int, bool)>): int {{\nreturn match value {{ Ok(pair) => pair.0, Err(error) => error.0 }}\n}}\n\nfn main(): int {{\nlet ready: Result<(int, bool), (int, bool)> = Ok((1, false))\nready = {value}\nlet match_code: int = match ready {{ Ok(pair) => pair.0, Err(error) => error.0 }}\nlet statement_code: int = 0\nmatch ready {{\nOk(pair) {{\nif pair.1 {{\nstatement_code = pair.0\n}} else {{\nstatement_code = 1\n}}\n}}\nErr(error) {{\nif error.1 {{\nstatement_code = error.0\n}} else {{\nstatement_code = 1\n}}\n}}\n}}\nlet helper_code: int = score(ready)\nlet literal_ok_code: int = score(Ok((48, true)))\nlet literal_err_code: int = score(Err((49, true)))\nif match_code == statement_code && statement_code == helper_code && literal_ok_code == 48 && literal_err_code == 49 {{\nreturn match_code\n}} else {{\nreturn 1\n}}\n}}\n"),
    )
    .expect("write result dual tuple payload match main exit source");
}

fn write_result_array_payload_match_main_exit_project(
    project: &Path,
    package_name: &str,
    variant: &str,
) {
    fs::create_dir_all(project.join("src"))
        .expect("create result array payload match main exit project src");
    fs::write(
        project.join("axiom.toml"),
        format!("[package]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n"),
    )
    .expect("write result array payload match main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        format!("version = 1\n\n[[package]]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\nsource = \"path\"\n"),
    )
    .expect("write result array payload match main exit lockfile");
    let value = if variant == "Ok" {
        "Ok([20, 28])"
    } else {
        "Err([21, 28])"
    };
    fs::write(
        project.join("src/main.ax"),
        format!("fn choose_result(flag: bool): Result<[int; 2], [int; 2]> {{\nif flag {{\nreturn Ok([20, 28])\n}} else {{\nreturn Err([21, 28])\n}}\n}}\n\nfn forward_result(value: Result<[int; 2], [int; 2]>): Result<[int; 2], [int; 2]> {{\nreturn value\n}}\n\nfn score(value: Result<[int; 2], [int; 2]>): int {{\nreturn match value {{ Ok(values) => values[0] + values[1], Err(error) => error[0] + error[1] }}\n}}\n\nfn main(): int {{\nlet ready_for_match: Result<[int; 2], [int; 2]> = {value}\nlet ready_for_statement: Result<[int; 2], [int; 2]> = {value}\nlet ready_for_helper: Result<[int; 2], [int; 2]> = {value}\nlet returned_ok_for_score: Result<[int; 2], [int; 2]> = choose_result(true)\nlet returned_err_for_score: Result<[int; 2], [int; 2]> = choose_result(false)\nlet returned_ok_for_forward: Result<[int; 2], [int; 2]> = choose_result(true)\nlet returned_err_for_forward: Result<[int; 2], [int; 2]> = choose_result(false)\nlet forwarded_ok: Result<[int; 2], [int; 2]> = forward_result(returned_ok_for_forward)\nlet forwarded_err: Result<[int; 2], [int; 2]> = forward_result(returned_err_for_forward)\nlet match_code: int = match ready_for_match {{ Ok(values) => values[0] + values[1], Err(error) => error[0] + error[1] }}\nlet statement_code: int = 0\nmatch ready_for_statement {{\nOk(values) {{\nstatement_code = values[0] + values[1]\n}}\nErr(error) {{\nstatement_code = error[0] + error[1]\n}}\n}}\nlet helper_code: int = score(ready_for_helper)\nlet returned_ok_code: int = score(returned_ok_for_score)\nlet returned_err_code: int = score(returned_err_for_score)\nlet forwarded_ok_code: int = score(forwarded_ok)\nlet forwarded_err_code: int = score(forwarded_err)\nlet literal_ok_code: int = score(Ok([20, 28]))\nlet literal_err_code: int = score(Err([21, 28]))\nif match_code == statement_code && statement_code == helper_code && returned_ok_code == 48 && returned_err_code == 49 && forwarded_ok_code == 48 && forwarded_err_code == 49 && literal_ok_code == 48 && literal_err_code == 49 {{\nreturn match_code\n}} else {{\nreturn 1\n}}\n}}\n"),
    )
    .expect("write result array payload match main exit source");
}

fn write_result_struct_payload_match_main_exit_project(
    project: &Path,
    package_name: &str,
    variant: &str,
) {
    fs::create_dir_all(project.join("src"))
        .expect("create result struct payload match main exit project src");
    fs::write(
        project.join("axiom.toml"),
        format!("[package]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n"),
    )
    .expect("write result struct payload match main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        format!("version = 1\n\n[[package]]\nname = \"cranelift-{package_name}\"\nversion = \"0.1.0\"\nsource = \"path\"\n"),
    )
    .expect("write result struct payload match main exit lockfile");
    let value = if variant == "Ok" {
        "Ok(Step { value: 48, enabled: true, small: 2u8 })"
    } else {
        "Err(Step { value: 49, enabled: false, small: 3u8 })"
    };
    fs::write(
        project.join("src/main.ax"),
        format!("struct Step {{\nvalue: int\nenabled: bool\nsmall: u8\n}}\n\nfn choose_result(flag: bool): Result<Step, Step> {{\nif flag {{\nreturn Ok(Step {{ value: 48, enabled: true, small: 2u8 }})\n}} else {{\nreturn Err(Step {{ value: 49, enabled: false, small: 3u8 }})\n}}\n}}\n\nfn forward_result(value: Result<Step, Step>): Result<Step, Step> {{\nreturn value\n}}\n\nfn score(value: Result<Step, Step>): int {{\nreturn match value {{ Ok(step) => step.value, Err(error) => error.value }}\n}}\n\nfn main(): int {{\nlet ready_for_match: Result<Step, Step> = {value}\nlet ready_for_statement: Result<Step, Step> = {value}\nlet ready_for_helper: Result<Step, Step> = {value}\nlet returned_ok_for_score: Result<Step, Step> = choose_result(true)\nlet returned_err_for_score: Result<Step, Step> = choose_result(false)\nlet returned_ok_for_forward: Result<Step, Step> = choose_result(true)\nlet returned_err_for_forward: Result<Step, Step> = choose_result(false)\nlet forwarded_ok: Result<Step, Step> = forward_result(returned_ok_for_forward)\nlet forwarded_err: Result<Step, Step> = forward_result(returned_err_for_forward)\nlet match_code: int = match ready_for_match {{ Ok(step) => step.value, Err(error) => error.value }}\nlet statement_code: int = 0\nmatch ready_for_statement {{\nOk(step) {{\nif step.enabled {{\nstatement_code = step.value\n}} else {{\nstatement_code = 1\n}}\n}}\nErr(error) {{\nif error.enabled {{\nstatement_code = 1\n}} else {{\nstatement_code = error.value\n}}\n}}\n}}\nlet helper_code: int = score(ready_for_helper)\nlet returned_ok_code: int = score(returned_ok_for_score)\nlet returned_err_code: int = score(returned_err_for_score)\nlet forwarded_ok_code: int = score(forwarded_ok)\nlet forwarded_err_code: int = score(forwarded_err)\nlet literal_ok_code: int = score(Ok(Step {{ small: 2u8, enabled: true, value: 48 }}))\nlet literal_err_code: int = score(Err(Step {{ small: 3u8, enabled: false, value: 49 }}))\nif match_code == statement_code && statement_code == helper_code && returned_ok_code == 48 && returned_err_code == 49 && forwarded_ok_code == 48 && forwarded_err_code == 49 && literal_ok_code == 48 && literal_err_code == 49 {{\nreturn match_code\n}} else {{\nreturn 1\n}}\n}}\n"),
    )
    .expect("write result struct payload match main exit source");
}

fn write_aggregate_helper_reassignment_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src"))
        .expect("create aggregate helper reassignment main exit project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-aggregate-helper-reassignment-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write aggregate helper reassignment main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-aggregate-helper-reassignment-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write aggregate helper reassignment main exit lockfile");
    fs::write(
        project.join("src/main.ax"),
        "struct Step {\nvalue: int\nenabled: bool\nsmall: u8\n}\n\nenum Choice {\nReady { step: Step }\nOff\n}\n\nfn make_pair(): (int, bool) {\nreturn (48, true)\n}\n\nfn make_values(): [int; 2] {\nreturn [20, 28]\n}\n\nfn make_step(): Step {\nreturn Step { value: 48, enabled: true, small: 2u8 }\n}\n\nfn make_option(): Option<Step> {\nreturn Some(Step { value: 48, enabled: true, small: 2u8 })\n}\n\nfn make_result(): Result<Step, Step> {\nreturn Ok(Step { value: 48, enabled: true, small: 2u8 })\n}\n\nfn make_choice(): Choice {\nreturn Ready { step: Step { value: 48, enabled: true, small: 2u8 } }\n}\n\nfn score_option(value: Option<Step>): int {\nreturn match value { Some(step) => step.value, None => 1 }\n}\n\nfn score_result(value: Result<Step, Step>): int {\nreturn match value { Ok(step) => step.value, Err(error) => error.value }\n}\n\nfn score_choice(value: Choice): int {\nreturn match value { Ready { step } => step.value, Off => 1 }\n}\n\nfn main(): int {\nlet pair: (int, bool) = (0, false)\nlet values: [int; 2] = [0, 0]\nlet step: Step = Step { value: 0, enabled: false, small: 0u8 }\nlet maybe: Option<Step> = None\nlet outcome: Result<Step, Step> = Err(Step { value: 1, enabled: false, small: 0u8 })\nlet choice: Choice = Off\nlet index: int = 0\nwhile index < 1 {\npair = make_pair()\nvalues = make_values()\nindex = index + 1\n}\nif pair.1 {\nstep = make_step()\nmaybe = make_option()\noutcome = make_result()\nchoice = make_choice()\n} else {\nstep = Step { value: 1, enabled: false, small: 0u8 }\nmaybe = None\noutcome = Err(Step { value: 1, enabled: false, small: 0u8 })\nchoice = Off\n}\nlet pair_code: int = pair.0\nlet pair_enabled: bool = pair.1\nlet array_code: int = values[0] + values[1]\nlet step_code: int = step.value\nlet step_enabled: bool = step.enabled\nlet option_code: int = score_option(maybe)\nlet result_code: int = score_result(outcome)\nlet choice_code: int = score_choice(choice)\nif pair_enabled && step_enabled && pair_code == 48 && array_code == 48 && step_code == 48 && option_code == 48 && result_code == 48 && choice_code == 48 {\nreturn pair_code\n} else {\nreturn 1\n}\n}\n",
    )
    .expect("write aggregate helper reassignment main exit source");
}

fn write_bool_returning_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create bool returning main exit project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-bool-returning-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write bool returning main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-bool-returning-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write bool returning main exit lockfile");
    fs::write(
        project.join("src/main.ax"),
        "static ENABLED: bool = true\n\nfn is_answer(enabled: bool, value: int): bool {\nreturn enabled == true && value == 42\n}\n\nfn choose(flag: bool, lhs: bool, rhs: bool): bool {\nif flag {\nreturn lhs\n} else {\nreturn rhs\n}\n}\n\nfn both(lhs: bool, rhs: bool): bool {\nreturn lhs && rhs\n}\n\nfn main(): bool {\nlet lhs: int = 41\nlet rhs: int = 1\nlet matches: bool = is_answer(true, lhs + rhs)\nlet blocked: bool = is_answer(false, 42)\nlet exact: bool = lhs + rhs == 42\nlet forwarded: bool = both(matches, exact)\nlet chosen: bool = choose(matches, forwarded, blocked)\nlet same: bool = matches == exact\nlet differs: bool = chosen != blocked\nreturn same && differs && chosen == ENABLED && blocked == false\n}\n",
    )
    .expect("write bool returning main exit source");
}

fn write_bool_tuple_index_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create bool tuple index main exit project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-bool-tuple-index-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write bool tuple index main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-bool-tuple-index-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write bool tuple index main exit lockfile");
    fs::write(
        project.join("src/main.ax"),
        "fn tuple_gate(pair: (bool, bool)): bool {\nlet first: bool = pair.0\nlet second: bool = pair.1\nreturn first && second == false\n}\n\nfn tuple_score(pair: (int, u8, bool)): int {\nlet base: int = pair.0\nlet bump: int = pair.1 as int\nlet enabled: bool = pair.2\nif enabled {\nreturn base + bump\n} else {\nreturn 1\n}\n}\n\nfn main(): bool {\nlet dynamic: bool = 40 + 2 == 42\nlet pair: (bool, bool) = (dynamic, false)\nlet gate: bool = pair.0\nlet blocked: bool = pair.1\nlet score: int = tuple_score((40, 2u8, true))\nlet local_score: int = tuple_score((39, 3u8, dynamic))\nreturn gate && blocked == false && tuple_gate(pair) && tuple_gate((true, false)) && score == 42 && local_score == 42\n}\n",
    )
    .expect("write bool tuple index main exit source");
}

fn write_tuple_returning_helper_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src"))
        .expect("create tuple returning helper main exit project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-tuple-returning-helper-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write tuple returning helper main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-tuple-returning-helper-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write tuple returning helper main exit lockfile");
    fs::write(
        project.join("src/main.ax"),
        "fn make_pair(base: int, enabled: bool): (int, bool) {\nreturn (base + 6, enabled)\n}\n\nfn make_local_pair(base: int): (int, bool) {\nlet pair: (int, bool) = (base + 6, true)\nreturn pair\n}\n\nfn forward_pair(pair: (int, bool)): (int, bool) {\nreturn pair\n}\n\nfn make_typed_pair(seed: u8): (u8, bool) {\nreturn (seed + 1u8, seed == 41u8)\n}\n\nfn forward_typed_pair(pair: (u8, bool)): (u8, bool) {\nreturn pair\n}\n\nfn choose_pair(flag: bool, base: int): (int, bool) {\nlet offset: int = 6\nlet ready: bool = base == 42\nif flag {\nlet value: int = base + offset\nreturn (value, ready)\n} else {\nlet fallback: int = 1\nreturn (fallback, false)\n}\n}\n\nfn main(): int {\nlet pair: (int, bool) = make_pair(42, true)\nlet local_pair: (int, bool) = make_local_pair(42)\nlet pair_to_forward: (int, bool) = make_pair(42, true)\nlet forwarded_pair: (int, bool) = forward_pair(pair_to_forward)\nlet typed: (u8, bool) = make_typed_pair(41u8)\nlet typed_to_forward: (u8, bool) = make_typed_pair(41u8)\nlet forwarded_typed: (u8, bool) = forward_typed_pair(typed_to_forward)\nlet branch_pair: (int, bool) = choose_pair(true, 42)\nlet blocked_pair: (int, bool) = choose_pair(false, 42)\nlet value: int = pair.0\nlet enabled: bool = pair.1\nlet local_value: int = local_pair.0\nlet local_enabled: bool = local_pair.1\nlet forwarded_value: int = forwarded_pair.0\nlet forwarded_enabled: bool = forwarded_pair.1\nlet typed_value: int = typed.0 as int\nlet typed_enabled: bool = typed.1\nlet forwarded_typed_value: int = forwarded_typed.0 as int\nlet forwarded_typed_enabled: bool = forwarded_typed.1\nlet branch_value: int = branch_pair.0\nlet branch_enabled: bool = branch_pair.1\nlet blocked_value: int = blocked_pair.0\nlet blocked_enabled: bool = blocked_pair.1\nif enabled && local_enabled && forwarded_enabled && typed_enabled && forwarded_typed_enabled && branch_enabled && blocked_enabled == false && value == 48 && local_value == 48 && forwarded_value == 48 && typed_value == 42 && forwarded_typed_value == 42 && branch_value == 48 && blocked_value == 1 {\nreturn value\n} else {\nreturn 1\n}\n}\n",
    )
    .expect("write tuple returning helper main exit source");
}

fn write_array_literal_index_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src"))
        .expect("create array literal index main exit project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-array-literal-index-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write array literal index main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-array-literal-index-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write array literal index main exit lockfile");
    fs::write(
        project.join("src/main.ax"),
        "fn sum_pair(values: [int; 2]): int {\nreturn values[0] + values[1]\n}\n\nfn pick_pair(values: [int; 2], index: int): int {\nreturn values[index]\n}\n\nfn make_pair(base: int): [int; 2] {\nreturn [base, 99]\n}\n\nfn make_local_pair(base: int): [int; 2] {\nlet values: [int; 2] = [base, 99]\nreturn values\n}\n\nfn forward_pair(values: [int; 2]): [int; 2] {\nreturn values\n}\n\nfn choose_pair(flag: bool): [int; 2] {\nif flag {\nlet value: int = 12\nreturn [value, 99]\n} else {\nreturn [1, 0]\n}\n}\n\nfn second_byte(values: [u8; 2]): int {\nreturn values[1] as int\n}\n\nfn pick_byte(values: [u8; 2], index: int): int {\nreturn values[index] as int\n}\n\nfn make_bytes(): [u8; 2] {\nreturn [1u8, 2u8]\n}\n\nfn forward_bytes(values: [u8; 2]): [u8; 2] {\nreturn values\n}\n\nfn array_gate(flags: [bool; 2]): bool {\nlet first: bool = flags[0]\nlet second: bool = flags[1]\nreturn first && second == false\n}\n\nfn pick_flag(flags: [bool; 2], index: int): bool {\nreturn flags[index]\n}\n\nfn make_flags(flag: bool): [bool; 2] {\nreturn [flag, false]\n}\n\nfn forward_flags(flags: [bool; 2]): [bool; 2] {\nreturn flags\n}\n\nfn main(): int {\nlet values: [int; 2] = [12, 99]\nlet helper_values: [int; 2] = [12, 99]\nlet returned_values: [int; 2] = make_pair(12)\nlet local_values: [int; 2] = make_local_pair(12)\nlet values_to_forward: [int; 2] = make_pair(12)\nlet forwarded_values: [int; 2] = forward_pair(values_to_forward)\nlet branch_values: [int; 2] = choose_pair(true)\nlet fallback_values: [int; 2] = choose_pair(false)\nlet bytes: [u8; 2] = [1u8, 2u8]\nlet helper_bytes: [u8; 2] = [1u8, 2u8]\nlet returned_bytes: [u8; 2] = make_bytes()\nlet bytes_to_forward: [u8; 2] = make_bytes()\nlet forwarded_bytes: [u8; 2] = forward_bytes(bytes_to_forward)\nlet first_index: int = 0\nlet second_index: int = 1\nlet first: int = values[first_index]\nlet typed: int = bytes[second_index] as int\nlet dynamic: bool = first + typed == 14\nlet flags: [bool; 2] = [dynamic, false]\nlet helper_flags: [bool; 2] = [dynamic, false]\nlet returned_flags: [bool; 2] = make_flags(dynamic)\nlet flags_to_forward: [bool; 2] = make_flags(dynamic)\nlet forwarded_flags: [bool; 2] = forward_flags(flags_to_forward)\nlet gate: bool = flags[first_index]\nlet blocked: bool = flags[second_index]\nlet local_sum: int = sum_pair(values)\nlet literal_sum: int = sum_pair([20, 28])\nlet helper_pick: int = pick_pair(helper_values, first_index)\nlet literal_pick: int = pick_pair([20, 28], second_index)\nlet returned_sum: int = sum_pair(returned_values)\nlet local_returned_sum: int = sum_pair(local_values)\nlet forwarded_sum: int = sum_pair(forwarded_values)\nlet branch_sum: int = sum_pair(branch_values)\nlet fallback_sum: int = sum_pair(fallback_values)\nlet typed_arg: int = second_byte(bytes)\nlet literal_typed_arg: int = second_byte([3u8, 4u8])\nlet dynamic_byte: int = pick_byte(helper_bytes, second_index)\nlet returned_byte: int = second_byte(returned_bytes)\nlet forwarded_byte: int = second_byte(forwarded_bytes)\nlet helper_flag: bool = pick_flag(helper_flags, first_index)\nlet literal_flag_blocked: bool = pick_flag([true, false], second_index)\nlet returned_flag: bool = pick_flag(returned_flags, first_index)\nlet forwarded_flag: bool = pick_flag(forwarded_flags, first_index)\nlet helper_numbers_ok: bool = local_sum == 111 && literal_sum == 48 && helper_pick == 12 && literal_pick == 28 && returned_sum == 111 && local_returned_sum == 111 && forwarded_sum == 111 && branch_sum == 111 && fallback_sum == 1\nlet helper_bytes_ok: bool = typed_arg == 2 && literal_typed_arg == 4 && dynamic_byte == 2 && returned_byte == 2 && forwarded_byte == 2\nlet helper_flags_ok: bool = array_gate([dynamic, false]) && array_gate([true, false]) && helper_flag && literal_flag_blocked == false && returned_flag && forwarded_flag\nif gate && blocked == false && helper_flags_ok && helper_numbers_ok && helper_bytes_ok {\nreturn first + typed + 34\n} else {\nreturn 1\n}\n}\n",
    )
    .expect("write array literal index main exit source");
}

fn write_fixed_array_intrinsics_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src"))
        .expect("create fixed array intrinsics main exit project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-fixed-array-intrinsics-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write fixed array intrinsics main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-fixed-array-intrinsics-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write fixed array intrinsics main exit lockfile");
    fs::write(
        project.join("src/main.ax"),
        "fn score(values: [int; 3]): int {\nreturn len(values) + first(values) + last(values)\n}\n\nfn gate(flags: [bool; 2]): bool {\nreturn first(flags) && last(flags) == false\n}\n\nfn slice_score(len_values: [int; 3], first_values: [int; 3], last_values: [int; 3]): int {\nreturn len(len_values[1:]) + first(first_values[1:]) + last(last_values[1:])\n}\n\nfn prefix_score(len_values: [int; 3], first_values: [int; 3], last_values: [int; 3]): int {\nreturn len(len_values[:2]) + first(first_values[:2]) + last(last_values[:2])\n}\n\nfn slice_pick(values: [int; 3], index: int): int {\nreturn values[1:][index]\n}\n\nfn slice_gate(first_flags: [bool; 3], last_flags: [bool; 3]): bool {\nreturn first(first_flags[1:]) == false && last(last_flags[1:])\n}\n\nfn slice_index_gate(flags: [bool; 3], index: int): bool {\nreturn flags[1:][index]\n}\n\nfn make_values(): [int; 3] {\nreturn [20, 3, 25]\n}\n\nfn make_flags(): [bool; 2] {\nreturn [true, false]\n}\n\nfn main(): int {\nlet values: [int; 3] = [20, 3, 25]\nlet returned: [int; 3] = make_values()\nlet slice_len_values: [int; 3] = [1, 20, 26]\nlet slice_first_values: [int; 3] = [1, 20, 26]\nlet slice_last_values: [int; 3] = [1, 20, 26]\nlet prefix_len_values: [int; 3] = [20, 26, 1]\nlet prefix_first_values: [int; 3] = [20, 26, 1]\nlet prefix_last_values: [int; 3] = [20, 26, 1]\nlet slice_index_literal_values: [int; 3] = [1, 20, 26]\nlet slice_index_dynamic_values: [int; 3] = [1, 20, 26]\nlet slice_local_values: [int; 3] = [1, 20, 26]\nlet slice_local_index_values: [int; 3] = [1, 20, 26]\nlet helper_slice_index_values: [int; 3] = [1, 20, 26]\nlet helper_slice_len_values: [int; 3] = [1, 20, 26]\nlet helper_slice_first_values: [int; 3] = [1, 20, 26]\nlet helper_slice_last_values: [int; 3] = [1, 20, 26]\nlet helper_prefix_len_values: [int; 3] = [20, 26, 1]\nlet helper_prefix_first_values: [int; 3] = [20, 26, 1]\nlet helper_prefix_last_values: [int; 3] = [20, 26, 1]\nlet flags: [bool; 2] = [true, false]\nlet returned_flags: [bool; 2] = make_flags()\nlet slice_gate_first_flags: [bool; 3] = [false, false, true]\nlet slice_gate_last_flags: [bool; 3] = [false, false, true]\nlet slice_index_flags: [bool; 3] = [false, false, true]\nlet slice_local_flags: [bool; 3] = [false, false, true]\nlet helper_slice_gate_first_flags: [bool; 3] = [false, false, true]\nlet helper_slice_gate_last_flags: [bool; 3] = [false, false, true]\nlet helper_slice_index_flags: [bool; 3] = [false, false, true]\nlet dynamic_slice_index: int = 1\nlet local_code: int = len(values) + first(values) + last(values)\nlet literal_code: int = len([20, 3, 25]) + first([20, 3, 25]) + last([20, 3, 25])\nlet helper_code: int = score(values)\nlet returned_code: int = len(returned) + first(returned) + last(returned)\nlet slice_code: int = len(slice_len_values[1:]) + first(slice_first_values[1:]) + last(slice_last_values[1:])\nlet prefix_code: int = len(prefix_len_values[:2]) + first(prefix_first_values[:2]) + last(prefix_last_values[:2])\nlet slice_index_code: int = slice_index_literal_values[1:][0] + slice_index_dynamic_values[1:][dynamic_slice_index] + 2\nlet literal_dynamic_code: int = [20, 26][dynamic_slice_index] + 22\nlet slice_window: &[int] = slice_local_values[1:]\nlet slice_index_window: &[int] = slice_local_index_values[1:]\nlet slice_local_code: int = len(slice_window) + first(slice_window) + last(slice_window)\nlet slice_local_index_code: int = slice_index_window[0] + slice_index_window[dynamic_slice_index] + 2\nlet helper_slice_index_code: int = slice_pick(helper_slice_index_values, dynamic_slice_index) + 22\nlet helper_slice_code: int = slice_score(helper_slice_len_values, helper_slice_first_values, helper_slice_last_values)\nlet helper_prefix_code: int = prefix_score(helper_prefix_len_values, helper_prefix_first_values, helper_prefix_last_values)\nlet bool_len: int = len(flags)\nlet local_gate: bool = first(flags) && last(flags) == false\nlet literal_gate: bool = first([true, false]) && last([true, false]) == false\nlet helper_gate: bool = gate(flags)\nlet returned_gate: bool = first(returned_flags) && last(returned_flags) == false\nlet slice_gate_local: bool = first(slice_gate_first_flags[1:]) == false && last(slice_gate_last_flags[1:])\nlet slice_index_gate_local: bool = slice_index_flags[1:][dynamic_slice_index]\nlet flag_window: &[bool] = slice_local_flags[1:]\nlet slice_local_gate: bool = first(flag_window) == false && last(flag_window) && flag_window[dynamic_slice_index]\nlet literal_dynamic_gate: bool = [false, true][dynamic_slice_index]\nlet helper_slice_gate: bool = slice_gate(helper_slice_gate_first_flags, helper_slice_gate_last_flags)\nlet helper_slice_index_gate: bool = slice_index_gate(helper_slice_index_flags, dynamic_slice_index)\nif local_gate && literal_gate && helper_gate && returned_gate && slice_gate_local && slice_index_gate_local && slice_local_gate && literal_dynamic_gate && helper_slice_gate && helper_slice_index_gate && bool_len == 2 && local_code == 48 && literal_code == 48 && helper_code == 48 && returned_code == 48 && slice_code == 48 && prefix_code == 48 && slice_index_code == 48 && literal_dynamic_code == 48 && slice_local_code == 48 && slice_local_index_code == 48 && helper_slice_index_code == 48 && helper_slice_code == 48 && helper_prefix_code == 48 {\nreturn local_code\n} else {\nreturn 1\n}\n}\n",
    )
    .expect("write fixed array intrinsics main exit source");
}

fn write_static_slice_bounds_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src"))
        .expect("create static slice bounds main exit project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-static-slice-bounds-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write static slice bounds main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-static-slice-bounds-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write static slice bounds main exit lockfile");
    fs::write(
        project.join("src/main.ax"),
        "static TAIL_START: int = 1\nstatic PREFIX_END: int = 2\n\nfn tail_score(values: [int; 3]): int {\nreturn len(values[TAIL_START:]) + first(values[TAIL_START:]) + last(values[TAIL_START:])\n}\n\nfn prefix_score(values: [int; 3]): int {\nreturn len(values[:PREFIX_END]) + first(values[:PREFIX_END]) + last(values[:PREFIX_END])\n}\n\nfn main(): int {\nlet tail_values: [int; 3] = [1, 20, 26]\nlet prefix_values: [int; 3] = [20, 26, 1]\nlet helper_tail_values: [int; 3] = [1, 20, 26]\nlet helper_prefix_values: [int; 3] = [20, 26, 1]\nlet tail_window: &[int] = tail_values[TAIL_START:]\nlet prefix_window: &[int] = prefix_values[:PREFIX_END]\nlet tail_code: int = len(tail_window) + first(tail_window) + last(tail_window)\nlet prefix_code: int = len(prefix_window) + first(prefix_window) + last(prefix_window)\nlet helper_tail: int = tail_score(helper_tail_values)\nlet helper_prefix: int = prefix_score(helper_prefix_values)\nif tail_code == 48 && prefix_code == 48 && helper_tail == 48 && helper_prefix == 48 {\nreturn 48\n} else {\nreturn 1\n}\n}\n",
    )
    .expect("write static slice bounds main exit source");
}

fn write_string_literal_len_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src"))
        .expect("create string literal len main exit project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-string-literal-len-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write string literal len main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-string-literal-len-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write string literal len main exit lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"static BANNER: string = "direct-native"
static PADDED: string = "  direct-native  "
static SECOND_LINE_INDEX: int = 1
static MISSING_LINE_INDEX: int = -1

fn local_len(): int {
let text: string = "native"
return len(text)
}

fn add_base(value: int): int {
return value + 19
}

fn native_prefix(): bool {
let value: string = string_clone(BANNER)
return string_starts_with(value, "direct")
}

fn static_prefix(): bool {
let trimmed: string = string_trim(PADDED)
return string_starts_with(trimmed, "direct")
}

fn main(): int {
let owned: string = "direct-native"
let short: string = "abi"
let prefix_text: string = string_clone(BANNER)
let miss_text: string = string_trim(PADDED)
let compare_text: string = string_trim_start("  direct-native")
let literal_len: int = len("runtime")
let local_len_value: int = len(owned)
let static_len_value: int = len(BANNER)
let clone_len_value: int = len(string_clone(BANNER))
let trim_len_value: int = len(string_trim(PADDED))
let trim_start_len_value: int = len(string_trim_start("  abi"))
let concat_len_value: int = len(string_clone(BANNER) + string_trim_start("  abi"))
let encoded_len: int = len(encoding_url_component_encode("hello world/one"))
let segment_len: int = len(encoding_path_segment_encode("a/b c"))
let pair_len: int = len(encoding_url_query_pair_encode("q", "agent path/one"))
let joined_len: int = len(encoding_path_join_segment("/docs", "stage 1/encoding"))
let strip_prefix_len: int = match string_strip_prefix(BANNER, "direct-") { Some(rest) => len(rest), None => 1 }
let strip_suffix_gate: bool = match string_strip_suffix(BANNER, "-native") { Some(rest) => string_starts_with(rest, "direct"), None => false }
let line_len: int = match string_line_at("first\nsecond\nthird", SECOND_LINE_INDEX) { Some(line) => len(line), None => 1 }
let line_missing: int = match string_line_at("first\nsecond", MISSING_LINE_INDEX) { Some(line) => len(line), None => 4 }
let decoded_len: int = match encoding_url_component_decode("hello%20axiom") { Some(value) => len(value), None => 1 }
let decode_missing: int = match encoding_url_component_decode("bad%2") { Some(value) => len(value), None => 4 }
let short_len: int = len(short)
let helper_len: int = local_len()
let helper_arg_len: int = add_base(len(short))
let prefix_gate: bool = string_starts_with(prefix_text, "direct")
let miss_gate: bool = string_starts_with(miss_text, "rust") == false
let helper_gate: bool = native_prefix()
let static_gate: bool = static_prefix()
let compare_gate: bool = compare_text == BANNER
if prefix_gate && miss_gate && helper_gate && static_gate && compare_gate && strip_suffix_gate && literal_len == 7 && local_len_value == 13 && static_len_value == 13 && clone_len_value == 13 && trim_len_value == 13 && trim_start_len_value == 3 && concat_len_value == 16 && encoded_len == 19 && segment_len == 9 && pair_len == 20 && joined_len == 26 && strip_prefix_len == 6 && line_len == 6 && line_missing == 4 && decoded_len == 11 && decode_missing == 4 && short_len == 3 && helper_len == 6 && helper_arg_len == 22 {
return literal_len + local_len_value + helper_len + helper_arg_len
} else {
return 1
}
}
"#,
    )
    .expect("write string literal len main exit source");
}

fn write_unsupported_string_helper_main_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create unsupported string helper main src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-unsupported-string-helper-main"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write unsupported string helper main manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-unsupported-string-helper-main"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write unsupported string helper main lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"fn make_banner(): string {
print "side-effect"
return "direct-native"
}

fn main(): int {
return len(make_banner())
}
"#,
    )
    .expect("write unsupported string helper main source");
}

fn write_known_string_helper_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create known string helper main project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-known-string-helper-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write known string helper main manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-known-string-helper-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write known string helper main lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"struct BannerBox {
text: string
bonus: int
}

static BANNER: string = "direct-native"

fn score(text: string): int {
return len(text)
}

fn make_banner(): string {
return "direct-native"
}

fn forward_text(text: string): string {
return text
}

fn local_banner(): string {
let text: string = "direct-native"
let copy: string = forward_text(text)
return copy
}

fn branch_banner(flag: bool): string {
if flag {
return "direct-native"
} else {
return "fallback"
}
}

fn local_score(text: string): int {
let copy: string = forward_text(text)
return len(copy)
}

fn branch_score(flag: bool): int {
if flag {
return len("direct-native")
} else {
return 1
}
}

fn has_native_prefix(text: string): bool {
return string_starts_with(text, "direct")
}

fn has_local_native_prefix(text: string): bool {
let copy: string = forward_text(text)
return string_starts_with(copy, "direct")
}

fn has_branch_native_prefix(flag: bool): bool {
if flag {
return string_starts_with("direct-native", "direct")
} else {
return false
}
}

fn match_banner(value: Option<string>): string {
return match value { Some(text) => text, None => "fallback" }
}

fn match_score(value: Option<string>): int {
return match value { Some(text) => len(text), None => 1 }
}

fn has_match_native_prefix(value: Option<string>): bool {
return match value { Some(text) => string_starts_with(text, "direct"), None => false }
}

fn match_stmt_banner(value: Option<string>): string {
match value {
Some(text) {
return text
}
None {
return "fallback"
}
}
}

fn match_stmt_score(value: Option<string>): int {
match value {
Some(text) {
return len(text)
}
None {
return 1
}
}
}

fn has_match_stmt_native_prefix(value: Option<string>): bool {
match value {
Some(text) {
return string_starts_with(text, "direct")
}
None {
return false
}
}
}

fn tuple_banner(value: (string, int)): string {
return value.0
}

fn tuple_score(value: (string, int)): int {
return len(value.0) + value.1
}

fn has_tuple_native_prefix(value: (string, int)): bool {
return string_starts_with(value.0, "direct")
}

fn struct_banner(value: BannerBox): string {
return value.text
}

fn struct_score(value: BannerBox): int {
return len(value.text) + value.bonus
}

fn has_struct_native_prefix(value: BannerBox): bool {
return string_starts_with(value.text, "direct")
}

fn map_index_banner(key: string): string {
return {"build": "forge", "deploy": "direct-native"}[key]
}

fn map_index_score(key: string): int {
return len({"build": "forge", "deploy": "direct-native"}[key])
}

fn has_map_index_native_prefix(key: string): bool {
let text: string = {"build": "forge", "deploy": "direct-native"}[key]
return string_starts_with(text, "direct")
}

fn main(): int {
let direct: int = score("direct-native")
let static_score: int = score(BANNER)
let forwarded_score: int = score(forward_text(BANNER))
let returned_text: string = make_banner()
let local_text: string = local_banner()
let branch_text: string = branch_banner(true)
let match_text: string = match_banner(Some(BANNER))
let match_stmt_text: string = match_stmt_banner(Some(BANNER))
let tuple_text: string = tuple_banner((BANNER, 5))
let struct_text: string = struct_banner(BannerBox { text: BANNER, bonus: 7 })
let map_index_text: string = map_index_banner("deploy")
let forwarded_len_text: string = forward_text(BANNER)
let forwarded_compare_text: string = forward_text(BANNER)
let returned_len: int = len(returned_text)
let local_len: int = len(local_text)
let branch_len: int = len(branch_text)
let match_len: int = len(match_text)
let match_stmt_len: int = len(match_stmt_text)
let tuple_len: int = len(tuple_text)
let struct_len: int = len(struct_text)
let map_index_len: int = len(map_index_text)
let forwarded_len: int = len(forwarded_len_text)
let local_score_value: int = local_score(BANNER)
let branch_score_value: int = branch_score(true)
let match_score_value: int = match_score(Some(BANNER))
let match_none_score_value: int = match_score(None)
let match_stmt_score_value: int = match_stmt_score(Some(BANNER))
let match_stmt_none_score_value: int = match_stmt_score(None)
let tuple_score_value: int = tuple_score((BANNER, 5))
let struct_score_value: int = struct_score(BannerBox { text: BANNER, bonus: 7 })
let map_index_score_value: int = map_index_score("deploy")
let prefix_gate: bool = has_native_prefix("direct-native")
let local_prefix_gate: bool = has_local_native_prefix(BANNER)
let branch_prefix_gate: bool = has_branch_native_prefix(true)
let match_prefix_gate: bool = has_match_native_prefix(Some(BANNER))
let match_none_prefix_gate: bool = has_match_native_prefix(None) == false
let match_stmt_prefix_gate: bool = has_match_stmt_native_prefix(Some(BANNER))
let match_stmt_none_prefix_gate: bool = has_match_stmt_native_prefix(None) == false
let tuple_prefix_gate: bool = has_tuple_native_prefix((BANNER, 5))
let struct_prefix_gate: bool = has_struct_native_prefix(BannerBox { text: BANNER, bonus: 7 })
let map_index_prefix_gate: bool = has_map_index_native_prefix("deploy")
let forwarded_gate: bool = forwarded_compare_text == "direct-native"
if direct == 13 && static_score == 13 && forwarded_score == 13 && returned_len == 13 && local_len == 13 && branch_len == 13 && match_len == 13 && match_stmt_len == 13 && tuple_len == 13 && struct_len == 13 && map_index_len == 13 && forwarded_len == 13 && local_score_value == 13 && branch_score_value == 13 && match_score_value == 13 && match_none_score_value == 1 && match_stmt_score_value == 13 && match_stmt_none_score_value == 1 && tuple_score_value == 18 && struct_score_value == 20 && map_index_score_value == 13 && prefix_gate && local_prefix_gate && branch_prefix_gate && match_prefix_gate && match_none_prefix_gate && match_stmt_prefix_gate && match_stmt_none_prefix_gate && tuple_prefix_gate && struct_prefix_gate && map_index_prefix_gate && forwarded_gate {
return 48
} else {
return 1
}
}
"#,
    )
    .expect("write known string helper main source");
}

fn write_std_encoding_wrapper_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create std encoding wrapper project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-std-encoding-wrapper-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write std encoding wrapper manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-std-encoding-wrapper-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write std encoding wrapper lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/encoding.ax\"\n\nfn main(): int {\nlet component: string = url_component_encode(\"hello world/one\")\nlet segment: string = path_segment_encode(\"a/b c\")\nlet pair: string = query_pair_encode(\"q\", \"agent path/one\")\nlet joined: string = path_join_segment(\"/docs\", \"stage 1/encoding\")\nlet decoded_len: int = match url_component_decode(\"hello%20axiom\") { Some(value) => len(value), None => 1 }\nlet decode_missing: int = match url_component_decode(\"bad%2\") { Some(value) => len(value), None => 4 }\nlet component_gate: bool = component == \"hello%20world%2Fone\"\nlet segment_gate: bool = segment == \"a%2Fb%20c\"\nlet pair_gate: bool = pair == \"q=agent%20path%2Fone\"\nlet joined_gate: bool = joined == \"/docs/stage%201%2Fencoding\"\nif component_gate && segment_gate && pair_gate && joined_gate && len(component) == 19 && len(segment) == 9 && len(pair) == 20 && len(joined) == 26 && decoded_len == 11 && decode_missing == 4 {\nreturn 48\n} else {\nreturn 1\n}\n}\n",
    )
    .expect("write std encoding wrapper source");
}

fn write_known_crypto_text_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create known crypto text project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-known-crypto-text-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = true\n",
    )
    .expect("write known crypto text manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-known-crypto-text-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write known crypto text lockfile");
    fs::write(
        project.join("src/main.ax"),
        "static KEY: string = \"key\"\n\nfn main(): int {\nlet message_for_len: string = \"The quick brown fox jumps over the lazy dog\"\nlet message_for_gate: string = \"The quick brown fox jumps over the lazy dog\"\nlet sha_for_gate: string = crypto_sha256(\"abc\")\nlet hmac256_for_gate: string = crypto_hmac_sha256(KEY, message_for_gate)\nlet hmac512_for_gate: string = crypto_hmac_sha512(\"Jefe\", \"what do ya want for nothing?\")\nlet sha_len: int = len(crypto_sha256(\"abc\"))\nlet hmac256_len: int = len(crypto_hmac_sha256(KEY, message_for_len))\nlet hmac512_len: int = len(crypto_hmac_sha512(\"Jefe\", \"what do ya want for nothing?\"))\nlet dynamic_sha_input: int = match json_parse_int(\"12345\") { Some(value) => value, None => 1 }\nlet dynamic_clone_sha_input: int = match json_parse_int(\"12345\") { Some(value) => value, None => 1 }\nlet dynamic_hmac_key: int = match json_parse_int(\"321\") { Some(value) => value, None => 1 }\nlet dynamic_hmac256_message: bool = match json_parse_bool(\"true\") { Some(value) => value, None => false }\nlet dynamic_hmac512_message: bool = match json_parse_bool(\"false\") { Some(value) => value, None => true }\nlet dynamic_sha_len: int = len(crypto_sha256(json_stringify_int(dynamic_sha_input)))\nlet dynamic_clone_sha_text: string = json_stringify_int(dynamic_clone_sha_input)\nlet dynamic_clone_sha_len: int = len(crypto_sha256(string_clone(dynamic_clone_sha_text)))\nlet dynamic_hmac256_len: int = len(crypto_hmac_sha256(json_stringify_int(dynamic_hmac_key), json_stringify_bool(dynamic_hmac256_message)))\nlet dynamic_hmac512_len: int = len(crypto_hmac_sha512(KEY, json_stringify_bool(dynamic_hmac512_message)))\nlet sha_gate: bool = string_starts_with(sha_for_gate, \"ba7816bf\")\nlet hmac256_gate: bool = string_starts_with(hmac256_for_gate, \"f7bc83f4\")\nlet hmac512_gate: bool = string_starts_with(hmac512_for_gate, \"164b7a7b\")\nlet constant_gate: bool = crypto_constant_time_eq(crypto_sha256(\"abc\"), \"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\")\nlet mismatch_gate: bool = crypto_constant_time_eq(\"short\", \"shorter\") == false\nlet byte_left: [u8; 3] = [1u8, 2u8, 3u8]\nlet byte_same: [u8; 3] = [1u8, 2u8, 3u8]\nlet byte_different: [u8; 3] = [1u8, 2u8, 4u8]\nlet byte_literal_left: [u8; 2] = [4u8, 5u8]\nlet byte_literal_same: [u8; 2] = [4u8, 5u8]\nlet byte_short: [u8; 1] = [4u8]\nlet byte_gate: bool = crypto_constant_time_eq_u8(byte_left[:], byte_same[:])\nlet byte_mismatch_gate: bool = crypto_constant_time_eq_u8(byte_left[:], byte_different[:]) == false\nlet byte_literal_gate: bool = crypto_constant_time_eq_u8(byte_literal_left[:], byte_literal_same[:])\nlet byte_len_gate: bool = crypto_constant_time_eq_u8(byte_literal_left[:], byte_short[:]) == false\nif sha_gate && hmac256_gate && hmac512_gate && constant_gate && mismatch_gate && byte_gate && byte_mismatch_gate && byte_literal_gate && byte_len_gate && sha_len == 64 && hmac256_len == 64 && hmac512_len == 128 && dynamic_sha_len == 64 && dynamic_clone_sha_len == 64 && dynamic_hmac256_len == 64 && dynamic_hmac512_len == 128 {\nreturn 48\n} else {\nreturn 1\n}\n}\n",
    )
    .expect("write known crypto text source");
}

fn write_std_crypto_wrapper_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create std crypto wrapper project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-std-crypto-wrapper-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = true\n",
    )
    .expect("write std crypto wrapper manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-std-crypto-wrapper-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write std crypto wrapper lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/crypto_hash.ax\"\nimport \"std/crypto_mac.ax\"\n\nstatic KEY: string = \"key\"\n\nfn main(): int {\nlet message_for_len: string = \"The quick brown fox jumps over the lazy dog\"\nlet message_for_gate: string = \"The quick brown fox jumps over the lazy dog\"\nlet message_for_verify: string = \"The quick brown fox jumps over the lazy dog\"\nlet sha_for_gate: string = sha256(\"abc\")\nlet hmac256_for_gate: string = hmac_sha256(KEY, message_for_gate)\nlet hmac512_for_gate: string = hmac_sha512(\"Jefe\", \"what do ya want for nothing?\")\nlet sha_len: int = len(sha256(\"abc\"))\nlet hmac256_len: int = len(hmac_sha256(KEY, message_for_len))\nlet hmac512_len: int = len(hmac_sha512(\"Jefe\", \"what do ya want for nothing?\"))\nlet sha_gate: bool = string_starts_with(sha_for_gate, \"ba7816bf\")\nlet hmac256_gate: bool = string_starts_with(hmac256_for_gate, \"f7bc83f4\")\nlet hmac512_gate: bool = string_starts_with(hmac512_for_gate, \"164b7a7b\")\nlet constant_gate: bool = constant_time_eq(sha256(\"abc\"), \"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\")\nlet verify256_gate: bool = verify_sha256(\"f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8\", KEY, message_for_verify)\nlet verify512_gate: bool = verify_sha512(\"164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea2505549758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737\", \"Jefe\", \"what do ya want for nothing?\")\nlet mismatch_gate: bool = constant_time_eq(\"short\", \"shorter\") == false\nlet byte_left: [u8; 3] = [1u8, 2u8, 3u8]\nlet byte_same: [u8; 3] = [1u8, 2u8, 3u8]\nlet byte_different: [u8; 3] = [1u8, 2u8, 4u8]\nlet byte_gate: bool = constant_time_eq_u8(byte_left[:], byte_same[:])\nlet byte_mismatch_gate: bool = constant_time_eq_u8(byte_left[:], byte_different[:]) == false\nif sha_gate && hmac256_gate && hmac512_gate && constant_gate && verify256_gate && verify512_gate && mismatch_gate && byte_gate && byte_mismatch_gate && sha_len == 64 && hmac256_len == 64 && hmac512_len == 128 {\nreturn 48\n} else {\nreturn 1\n}\n}\n",
    )
    .expect("write std crypto wrapper source");
}

fn write_known_regex_text_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create known regex text project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-known-regex-text-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write known regex text manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-known-regex-text-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write known regex text lockfile");
    fs::write(
        project.join("src/main.ax"),
        "static ISSUE_TEXT: string = \"issue-238-ready\"\n\nfn main(): int {\nlet replaced: string = regex_replace_all(\"[0-9]+\", ISSUE_TEXT, \"#\")\nlet anchored: string = regex_replace_all(\"^a\", \"aaa\", \"x\")\nlet match_gate: bool = regex_is_match(\"^issue-[0-9]+-ready$\", ISSUE_TEXT)\nlet replaced_gate: bool = replaced == \"issue-#-ready\"\nlet anchored_gate: bool = anchored == \"xaa\"\nlet found_len: int = match regex_find(\"[0-9]+\", ISSUE_TEXT) { Some(value) => len(value), None => 1 }\nlet missing_len: int = match regex_find(\"z+\", ISSUE_TEXT) { Some(value) => len(value), None => 4 }\nlet replaced_len: int = len(regex_replace_all(\"[a-z]+\", \"abc-123\", \"x\"))\nif match_gate && replaced_gate && anchored_gate && found_len == 3 && missing_len == 4 && replaced_len == 5 {\nreturn 48\n} else {\nreturn 1\n}\n}\n",
    )
    .expect("write known regex text source");
}

fn write_std_regex_wrapper_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create std regex wrapper project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-std-regex-wrapper-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write std regex wrapper manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-std-regex-wrapper-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write std regex wrapper lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/regex.ax\"\n\nstatic ISSUE_TEXT: string = \"issue-238-ready\"\n\nfn main(): int {\nlet replaced: string = replace_all(\"[0-9]+\", ISSUE_TEXT, \"#\")\nlet anchored: string = replace_all(\"^a\", \"aaa\", \"x\")\nlet match_gate: bool = is_match(\"^issue-[0-9]+-ready$\", ISSUE_TEXT)\nlet replaced_gate: bool = replaced == \"issue-#-ready\"\nlet anchored_gate: bool = anchored == \"xaa\"\nlet found_len: int = match find(\"[0-9]+\", ISSUE_TEXT) { Some(value) => len(value), None => 1 }\nlet missing_len: int = match find(\"z+\", ISSUE_TEXT) { Some(value) => len(value), None => 4 }\nlet replaced_len: int = len(replace_all(\"[a-z]+\", \"abc-123\", \"x\"))\nif match_gate && replaced_gate && anchored_gate && found_len == 3 && missing_len == 4 && replaced_len == 5 {\nreturn 48\n} else {\nreturn 1\n}\n}\n",
    )
    .expect("write std regex wrapper source");
}

fn write_known_json_text_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create known json text project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-known-json-text-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write known json text manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-known-json-text-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write known json text lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"static DOC: string = "{\"name\":\"axiom\",\"count\":3,\"ready\":true,\"nested\":{\"ok\":true}}"
static STATIC_COUNT: int = 321
static STATIC_READY: bool = true

fn json_int_len(value: int): int {
let rendered: string = json_stringify_int(value)
return len(rendered)
}

fn json_name_len(): int {
return match json_parse_field_string(DOC, "name") { Some(value) => len(value), None => 1 }
}

fn json_value_len(): int {
return match json_parse_value("[1,true]") { Some(value) => len(value), None => 1 }
}

fn quoted_static_bool_len(): int {
let rendered: string = json_stringify_bool(STATIC_READY)
return len(json_stringify_string(rendered))
}

fn main(): int {
let quoted_len: int = len(json_stringify_string("axiom"))
let int_len: int = len(json_stringify_int(42))
let static_int_len: int = len(json_stringify_int(STATIC_COUNT))
let bool_len: int = len(json_stringify_bool(false))
let static_bool_len: int = len(json_stringify_bool(STATIC_READY))
let value_len: int = len(json_stringify_value(DOC))
let parsed_int: int = match json_parse_int(" 42 ") { Some(value) => value, None => 1 }
let parsed_int_for_len: int = match json_parse_int(" 42 ") { Some(value) => value, None => 1 }
let parsed_int_for_helper_len: int = match json_parse_int(" 42 ") { Some(value) => value, None => 1 }
let parsed_int_for_clone_len: int = match json_parse_int(" 42 ") { Some(value) => value, None => 1 }
let parsed_int_for_negative_len: int = match json_parse_int(" 42 ") { Some(value) => value, None => 1 }
let parsed_int_for_quoted_len: int = match json_parse_int(" 42 ") { Some(value) => value, None => 1 }
let parsed_int_for_negative_quoted_len: int = match json_parse_int(" 42 ") { Some(value) => value, None => 1 }
let parsed_bool: bool = match json_parse_bool("true") { Some(value) => value, None => false }
let parsed_bool_for_len: bool = match json_parse_bool("true") { Some(value) => value, None => false }
let parsed_bool_for_clone_len: bool = match json_parse_bool("true") { Some(value) => value, None => false }
let parsed_bool_for_quoted_len: bool = match json_parse_bool("true") { Some(value) => value, None => false }
let parsed_int_for_branch_len: int = match json_parse_int(" 42 ") { Some(value) => value, None => 1 }
let parsed_bool_for_branch_len: bool = match json_parse_bool("true") { Some(value) => value, None => false }
let dynamic_int_len: int = len(json_stringify_int(parsed_int_for_len))
let helper_int_len: int = json_int_len(parsed_int_for_helper_len)
let helper_name_len: int = json_name_len()
let helper_value_len: int = json_value_len()
let helper_quoted_static_bool_len: int = quoted_static_bool_len()
let negative_int_len: int = len(json_stringify_int(0 - parsed_int_for_negative_len))
let dynamic_bool_len: int = len(json_stringify_bool(parsed_bool_for_len))
let dynamic_quoted_int_text: string = json_stringify_int(parsed_int_for_quoted_len)
let dynamic_quoted_negative_int_text: string = json_stringify_int(0 - parsed_int_for_negative_quoted_len)
let dynamic_quoted_bool_text: string = json_stringify_bool(parsed_bool_for_quoted_len)
let dynamic_quoted_int_len: int = len(json_stringify_string(dynamic_quoted_int_text))
let dynamic_quoted_negative_int_len: int = len(json_stringify_string(dynamic_quoted_negative_int_text))
let dynamic_quoted_bool_len: int = len(json_stringify_string(dynamic_quoted_bool_text))
let dynamic_clone_int_text: string = json_stringify_int(parsed_int_for_clone_len)
let dynamic_clone_int_len: int = len(string_clone(dynamic_clone_int_text))
let dynamic_clone_bool_text: string = json_stringify_bool(parsed_bool_for_clone_len)
let dynamic_clone_bool_len: int = len(string_clone(dynamic_clone_bool_text))
let dynamic_concat_len: int = len(dynamic_clone_int_text + dynamic_clone_bool_text)
let branch_len: int = 0
let branch_quoted_len: int = 0
if parsed_bool_for_branch_len {
let branch_text: string = json_stringify_int(parsed_int_for_branch_len)
let branch_clone_len: int = len(string_clone(branch_text))
let branch_quoted_text: string = json_stringify_bool(parsed_bool_for_branch_len)
let branch_quoted_len_value: int = len(json_stringify_string(branch_quoted_text))
branch_len = branch_clone_len
branch_quoted_len = branch_quoted_len_value
} else {
branch_len = 1
branch_quoted_len = 1
}
let count_value: int = match json_parse_field_int(DOC, "count") { Some(value) => value, None => 1 }
let ready_value: bool = match json_parse_field_bool(DOC, "ready") { Some(value) => value, None => false }
let name_len: int = match json_parse_field_string(DOC, "name") { Some(value) => len(value), None => 1 }
let field_value_len: int = match json_parse_field_value(DOC, "nested") { Some(value) => len(value), None => 1 }
let parsed_string_len: int = match json_parse_string("\"hello\"") { Some(value) => len(value), None => 1 }
let parsed_value_len: int = match json_parse_value("[1,true]") { Some(value) => len(value), None => 1 }
let missing_len: int = match json_parse_field_string(DOC, "missing") { Some(value) => len(value), None => 4 }
let missing_int: int = match json_parse_field_int(DOC, "missing") { Some(value) => value, None => 4 }
if parsed_bool && ready_value && quoted_len == 7 && int_len == 2 && static_int_len == 3 && bool_len == 5 && static_bool_len == 4 && dynamic_int_len == 2 && helper_int_len == 2 && helper_name_len == 5 && helper_value_len == 8 && helper_quoted_static_bool_len == 6 && negative_int_len == 3 && dynamic_bool_len == 4 && dynamic_quoted_int_len == 4 && dynamic_quoted_negative_int_len == 5 && dynamic_quoted_bool_len == 6 && dynamic_clone_int_len == 2 && dynamic_clone_bool_len == 4 && dynamic_concat_len == 6 && branch_len == 2 && branch_quoted_len == 6 && value_len == 60 && parsed_int == 42 && count_value == 3 && name_len == 5 && field_value_len == 11 && parsed_string_len == 5 && parsed_value_len == 8 && missing_len == 4 && missing_int == 4 {
return 48
} else {
return 1
}
}
"#,
    )
    .expect("write known json text source");
}

fn write_std_json_wrapper_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create std json wrapper project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-std-json-wrapper-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write std json wrapper manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-std-json-wrapper-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write std json wrapper lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/json.ax\"\n\nstatic DOC: string = \"{\\\"name\\\":\\\"axiom\\\",\\\"count\\\":3,\\\"ready\\\":true}\"\nstatic STATIC_COUNT: int = 321\nstatic STATIC_READY: bool = true\n\nfn main(): int {\nlet int_len: int = len(stringify_int(42))\nlet static_int_len: int = len(stringify_int(STATIC_COUNT))\nlet bool_len: int = len(stringify_bool(false))\nlet static_bool_len: int = len(stringify_bool(STATIC_READY))\nlet string_len: int = len(stringify_string(\"axiom\"))\nlet parsed_int: int = match parse_int(\" 42 \") { Some(value) => value, None => 1 }\nlet parsed_bool: bool = match parse_bool(\"true\") { Some(value) => value, None => false }\nlet parsed_string_len: int = match parse_string(\"\\\"hello\\\"\") { Some(value) => len(value), None => 1 }\nlet missing_string_len: int = match parse_string(\"42\") { Some(value) => len(value), None => 4 }\nlet count_value: int = match parse_field_int(DOC, \"count\") { Some(value) => value, None => 1 }\nlet ready_value: bool = match parse_field_bool(DOC, \"ready\") { Some(value) => value, None => false }\nlet name_len: int = match parse_field_string(DOC, \"name\") { Some(value) => len(value), None => 1 }\nlet missing_int: int = match parse_field_int(DOC, \"missing\") { Some(value) => value, None => 4 }\nlet dynamic_int: int = match parse_int(\"12345\") { Some(value) => value, None => 1 }\nlet dynamic_bool: bool = match parse_bool(\"false\") { Some(value) => value, None => true }\nlet dynamic_int_len: int = len(stringify_int(dynamic_int))\nlet dynamic_bool_len: int = len(stringify_bool(dynamic_bool))\nlet dynamic_string_text: string = stringify_int(dynamic_int)\nlet dynamic_string_len: int = len(stringify_string(dynamic_string_text))\nif parsed_bool && ready_value && int_len == 2 && static_int_len == 3 && bool_len == 5 && static_bool_len == 4 && string_len == 7 && parsed_int == 42 && parsed_string_len == 5 && missing_string_len == 4 && count_value == 3 && name_len == 5 && missing_int == 4 && dynamic_int_len == 5 && dynamic_bool_len == 5 && dynamic_string_len == 7 {\nreturn 48\n} else {\nreturn 1\n}\n}\n",
    )
    .expect("write std json wrapper source");
}

fn write_std_log_format_wrapper_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create std log format wrapper project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-std-log-format-wrapper-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write std log format wrapper manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-std-log-format-wrapper-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write std log format wrapper lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/log.ax\"\n\nfn main(): int {\nlet component_gate: bool = field_string(\"component\", \"worker\") == \"\\\"component\\\":\\\"worker\\\"\"\nlet attempt_gate: bool = field_int(\"attempt\", 2) == \"\\\"attempt\\\":2\"\nlet ready_gate: bool = field_bool(\"ready\", true) == \"\\\"ready\\\":true\"\nlet attrs: string = fields3(field_string(\"component\", \"worker\"), field_int(\"attempt\", 2), field_bool(\"ready\", true))\nlet subset: string = fields2(field_string(\"component\", \"worker\"), field_bool(\"ready\", true))\nlet record: string = event(\"info\", \"started\", fields3(field_string(\"component\", \"worker\"), field_int(\"attempt\", 2), field_bool(\"ready\", true)))\nlet escaped: string = event(\"warn\", \"quote \\\"ok\\\"\", fields2(field_string(\"path\", \"a/b\"), field_bool(\"ready\", false)))\nlet expected: string = \"{\\\"level\\\":\\\"info\\\",\\\"message\\\":\\\"started\\\",\\\"attributes\\\":{\\\"component\\\":\\\"worker\\\",\\\"attempt\\\":2,\\\"ready\\\":true}}\"\nlet expected_escaped: string = \"{\\\"level\\\":\\\"warn\\\",\\\"message\\\":\\\"quote \\\\\\\"ok\\\\\\\"\\\",\\\"attributes\\\":{\\\"path\\\":\\\"a/b\\\",\\\"ready\\\":false}}\"\nif component_gate && attempt_gate && ready_gate && attrs == \"\\\"component\\\":\\\"worker\\\",\\\"attempt\\\":2,\\\"ready\\\":true\" && subset == \"\\\"component\\\":\\\"worker\\\",\\\"ready\\\":true\" && record == expected && escaped == expected_escaped && len(record) == 97 && len(escaped) == 83 {\nreturn 48\n} else {\nreturn 1\n}\n}\n",
    )
    .expect("write std log format wrapper source");
}

fn write_std_log_selected_projection_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src"))
        .expect("create std log selected projection project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-std-log-selected-projection-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write std log selected projection manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-std-log-selected-projection-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write std log selected projection lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/log.ax"

fn main(): int {
let scores: {string: int} = {"build": 7, "deploy": 9}
let event_scores: {string: int} = {"build": 7, "deploy": 9}
let names: [string] = keys<string, int>(scores)
let event_names: [string] = keys<string, int>(event_scores)
let selected_index: int = 1
let selected_line: string = names[selected_index]
let selected_event_line: string = event_names[selected_index]
let attrs: string = fields3(field_string("component", "worker"), field_int("attempt", 2), field_bool("ready", true))
let selected_field_len: int = len(field_string("selected", selected_line))
let selected_event_len: int = len(event("info", selected_event_line, attrs))
if selected_field_len == 19 && selected_event_len == 96 {
return 48
} else {
return 1
}
}
"#,
    )
    .expect("write std log selected projection source");
}

fn write_std_log_dynamic_scalar_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create std log dynamic scalar project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-std-log-dynamic-scalar-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write std log dynamic scalar manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-std-log-dynamic-scalar-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write std log dynamic scalar lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/log.ax"

fn main(): int {
let count: int = match json_parse_int("12345") { Some(value) => value, None => 1 }
let ready: bool = match json_parse_bool("false") { Some(value) => value, None => true }
let count_for_message: int = match json_parse_int("12345") { Some(value) => value, None => 1 }
let message: string = json_stringify_int(count_for_message)
let count_field_len: int = len(field_int("count", count))
let ready_field_len: int = len(field_bool("ready", ready))
let attrs_len: int = len(fields2(field_int("count", count), field_bool("ready", ready)))
let event_len: int = len(event("info", message, fields2(field_int("count", count), field_bool("ready", ready))))
if count_field_len == 13 && ready_field_len == 13 && attrs_len == 27 && event_len == 77 {
return 48
} else {
return 1
}
}
"#,
    )
    .expect("write std log dynamic scalar source");
}

fn write_std_log_dynamic_scalar_info_attrs_project(project: &Path) {
    fs::create_dir_all(project.join("src"))
        .expect("create std log dynamic scalar info attrs project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-std-log-dynamic-scalar-info-attrs"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write std log dynamic scalar info attrs manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-std-log-dynamic-scalar-info-attrs"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write std log dynamic scalar info attrs lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/log.ax"

fn main(): int {
let count: int = match json_parse_int("12345") { Some(value) => value, None => 1 }
let ready: bool = match json_parse_bool("false") { Some(value) => value, None => true }
let message_count: int = match json_parse_int("12345") { Some(value) => value, None => 1 }
let message: string = json_stringify_int(message_count)
let written: int = info_attrs(message, fields2(field_int("count", count), field_bool("ready", ready)))
if written == 78 {
return 48
} else {
return 1
}
}
"#,
    )
    .expect("write std log dynamic scalar info attrs source");
}

fn write_std_log_level_wrapper_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create std log level wrapper project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-std-log-level-wrapper"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write std log level wrapper manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-std-log-level-wrapper"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write std log level wrapper lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/log.ax"

fn main(): int {
let message_count: int = match json_parse_int("12345") { Some(value) => value, None => 1 }
let message: string = json_stringify_int(message_count)
let written: int = info(message)
if written == 51 {
return 48
} else {
return 1
}
}
"#,
    )
    .expect("write std log level wrapper source");
}

fn write_std_log_dynamic_event_print_project(project: &Path) {
    fs::create_dir_all(project.join("src"))
        .expect("create std log dynamic event print project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-std-log-dynamic-event-print"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write std log dynamic event print manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-std-log-dynamic-event-print"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write std log dynamic event print lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/log.ax"

fn main(): int {
let count: int = match json_parse_int("12345") { Some(value) => value, None => 1 }
let ready: bool = match json_parse_bool("false") { Some(value) => value, None => true }
let quoted_event_count: int = match json_parse_int("12345") { Some(value) => value, None => 1 }
let quoted_event_message: string = json_stringify_int(quoted_event_count)
let plain_count: int = match json_parse_int("12345") { Some(value) => value, None => 1 }
let plain_ready: bool = match json_parse_bool("false") { Some(value) => value, None => true }
let plain_event_count: int = match json_parse_int("12345") { Some(value) => value, None => 1 }
let plain_event_message: string = json_stringify_int(plain_event_count)
let quoted_field_count: int = match json_parse_int("12345") { Some(value) => value, None => 1 }
let quoted_field_message: string = json_stringify_int(quoted_field_count)
print event("warn", json_stringify_string(quoted_event_message), fields2(field_int("count", count), field_bool("ready", ready)))
print event("info", plain_event_message, fields3(field_int("count", plain_count), field_bool("ready", plain_ready), field_string("quoted", json_stringify_string(quoted_field_message))))
return 48
}
"#,
    )
    .expect("write std log dynamic event print source");
}

fn write_struct_literal_field_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src"))
        .expect("create struct literal field main exit project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-struct-literal-field-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write struct literal field main exit manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-struct-literal-field-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write struct literal field main exit lockfile");
    fs::write(
        project.join("src/main.ax"),
        "struct Step {\nvalue: int\nready: bool\nsmall: u8\n}\n\nfn make_step(value: int, ready: bool): Step {\nreturn Step { small: 2u8, ready: ready, value: value }\n}\n\nfn make_local_step(value: int): Step {\nlet step: Step = Step { small: 2u8, ready: true, value: value }\nreturn step\n}\n\nfn forward_step(step: Step): Step {\nreturn step\n}\n\nfn choose_step(flag: bool, value: int): Step {\nlet ready: bool = value == 12\nif flag {\nlet local_value: int = value\nreturn Step { ready: ready, small: 2u8, value: local_value }\n} else {\nlet fallback: int = 1\nreturn Step { small: 0u8, value: fallback, ready: false }\n}\n}\n\nfn step_score(step: Step): int {\nlet value: int = step.value\nlet small: int = step.small as int\nif step.ready {\nreturn value + small\n} else {\nreturn 1\n}\n}\n\nfn field_gate(step: Step): bool {\nlet ready: bool = step.ready\nreturn ready && step.value == 1 && step.small == 2u8\n}\n\nfn main(): int {\nlet step: Step = Step { ready: true, small: 2u8, value: 12 }\nlet returned_step: Step = make_step(12, true)\nlet local_step: Step = make_local_step(12)\nlet step_to_forward: Step = make_step(12, true)\nlet forwarded_step: Step = forward_step(step_to_forward)\nlet branch_step: Step = choose_step(true, 12)\nlet fallback_step: Step = choose_step(false, 12)\nlet blocked_step: Step = Step { small: 2u8, value: step.value, ready: false }\nlet gate_step: Step = Step { small: 2u8, ready: true, value: 1 }\nlet value: int = step.value\nlet small: int = step.small as int\nlet ready: bool = step.ready\nlet blocked: bool = blocked_step.ready\nlet returned_score: int = step_score(returned_step)\nlet local_score: int = step_score(local_step)\nlet forwarded_score: int = step_score(forwarded_step)\nlet branch_score: int = step_score(branch_step)\nlet fallback_score: int = step_score(fallback_step)\nlet helper_score: int = step_score(step)\nlet blocked_score: int = step_score(blocked_step)\nlet inline_score: int = step_score(Step { small: 2u8, ready: true, value: 12 })\nif ready && blocked == false && field_gate(gate_step) && helper_score == 14 && returned_score == 14 && local_score == 14 && forwarded_score == 14 && branch_score == 14 && fallback_score == 1 && blocked_score == 1 && inline_score == 14 {\nreturn value + small + 34\n} else {\nreturn 1\n}\n}\n",
    )
    .expect("write struct literal field main exit source");
}

fn write_i64_while_loop_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create i64 while loop exit project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-i64-while-loop-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write i64 while loop manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-i64-while-loop-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write i64 while loop lockfile");
    fs::write(
        project.join("src/main.ax"),
        "struct Step {\nvalue: int\nhit: bool\n}\n\nfn main(): int {\nlet index: int = 0\nlet total: int = 0\nlet reached_four: bool = false\nlet pair: (int, bool) = (0, false)\nlet step_values: [int; 2] = [0, 0]\nlet step_record: Step = Step { value: 0, hit: false }\nwhile index < 6 {\nindex = index + 1\npair = (index, index == 4)\nstep_values = [pair.0, 0]\nstep_record = Step { value: step_values[0], hit: pair.1 }\nlet step: int = step_record.value\nlet hit_now: bool = step_record.hit\nif hit_now {\nreached_four = true\n} else {\nreached_four = reached_four\n}\ntotal = total + step\n}\nlet doubled: int = total * 2\nif reached_four {\nlet exit_code: int = doubled\nreturn exit_code\n} else {\nlet fallback: int = 1\nreturn fallback\n}\n}\n",
    )
    .expect("write i64 while loop source");
}

fn write_scalar_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create scalar project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-scalar-aggregate\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write scalar manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-scalar-aggregate\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write scalar lockfile");
    fs::write(
        project.join("src/main.ax"),
        "fn sum_tail(values: [int; 3]): int {\nreturn values[1] * values[2]\n}\n\nfn adjust(value: int): int {\nreturn value / 2\n}\n\nlet label: (string, int) = (\"native\", 7)\nlet values: [int; 3] = [2, 3, 4]\nprint label.0\nprint label.1\nprint sum_tail(values)\nprint adjust(20)\n",
    )
    .expect("write scalar source");
}

fn write_std_string_builder_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create string builder project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-string-builder\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write string builder manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-string-builder\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write string builder lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/string_builder.ax"

let empty: StringBuilder = builder()
let greeting: StringBuilder = push_str(empty, "hello")
let spaced: StringBuilder = push_str(greeting, " ")
let finished: StringBuilder = push_str(spaced, "stdlib")
print finish(finished)

let seeded: StringBuilder = from_string("first")
let second: StringBuilder = push_line(seeded, " line")
let third: StringBuilder = push_str(second, "second line")
print finish(third)
"#,
    )
    .expect("write string builder source");
}

fn write_std_string_builder_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create string builder main project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-string-builder-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write string builder main manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-string-builder-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write string builder main lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/string_builder.ax"

fn main(): int {
let empty: StringBuilder = builder()
let greeting: StringBuilder = push_str(empty, "hello")
let spaced: StringBuilder = push_str(greeting, " ")
let finished: StringBuilder = push_str(spaced, "stdlib")
let message: string = finish(finished)
let seeded: StringBuilder = from_string("first")
let second: StringBuilder = push_line(seeded, " line")
let third: StringBuilder = push_str(second, "second line")
let report: string = finish(third)
let nested: string = finish(push_line(from_string("nested"), " ok"))
if message == "hello stdlib" && report == "first line\nsecond line" && nested == "nested ok\n" && len(message) == 12 && len(report) == 22 && len(nested) == 10 {
return 48
} else {
return 1
}
}
"#,
    )
    .expect("write string builder main source");
}

fn write_string_intrinsics_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create string intrinsics project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-string-intrinsics\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write string intrinsics manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-string-intrinsics\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write string intrinsics lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"print string_clone("native")
print string_starts_with("stage1", "stage")

match string_strip_prefix("stage1", "stage") {
Some(value) {
print value
}
None {
print "missing"
}
}

match string_strip_suffix("stage1", "1") {
Some(value) {
print value
}
None {
print "missing"
}
}

print "[" + string_trim("  padded  ") + "]"
print "[" + string_trim_start("  left  ") + "]"

match string_line_at("first\nsecond\nthird", 1) {
Some(value) {
print value
}
None {
print "missing"
}
}

match string_line_at("first\nsecond", -1) {
Some(value) {
print value
}
None {
print "none"
}
}
"#,
    )
    .expect("write string intrinsics source");
}

fn write_numeric_cross_width_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create numeric project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-numeric-cross-width\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write numeric manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-numeric-cross-width\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write numeric lockfile");
    fs::write(
        project.join("src/main.ax"),
        "let wide_signed: i64 = 7i64\nlet narrow_signed: i32 = wide_signed as i32\n\nlet byte: u8 = 255u8\nlet widened_unsigned: i32 = byte as i32\n\nlet signed32: i32 = 3i32\nlet float32: f32 = signed32 as f32\n\nlet float64: f64 = -4.75f64\nlet signed64: i64 = float64 as i64\n\nlet same: i32 = 42i32 as i32\n\nlet signed_to_unsigned: u8 = -1i64 as u8\nlet narrowed_unsigned: u8 = 300i64 as u8\nlet narrowed_signed: i8 = 130i64 as i8\nlet wrapped_int: int = 18446744073709551615u64 as int\nlet max_u64: u64 = 18446744073709551615u64\nlet saturated_float_unsigned: u8 = 300.0f64 as u8\nlet negative_float_unsigned: u8 = -1.0f64 as u8\nlet rounded_f32: f32 = 16777216f32 + 1f32\n\nprint narrow_signed\nprint widened_unsigned\nprint float32 as int\nprint signed64\nprint same\nprint signed_to_unsigned\nprint narrowed_unsigned\nprint narrowed_signed\nprint wrapped_int\nprint max_u64\nprint saturated_float_unsigned\nprint negative_float_unsigned\nprint rounded_f32 as int\n",
    )
    .expect("write numeric source");
}

fn write_static_scalar_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create static scalar project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-static-scalar\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write static scalar manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-static-scalar\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write static scalar lockfile");
    fs::write(
        project.join("src/main.ax"),
        "static GREETING: string = \"hello static\"\nstatic ANSWER: int = 42\nstatic READY: bool = true\n\nfn bump(value: int): int {\nreturn value + ANSWER\n}\n\nprint GREETING\nprint ANSWER\nprint bump(1)\nprint READY\n",
    )
    .expect("write static scalar source");
}

fn write_enum_match_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create enum match project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-enum-match\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write enum match manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-enum-match\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write enum match lockfile");
    fs::write(
        project.join("src/main.ax"),
        "enum Message {\nPair(int, string)\nJob { id: int, label: string }\nText(string)\n}\n\nenum Signal {\nRed\nYellow\nGreen\n}\n\nfn render(message: Message): string {\nmatch message {\nPair(count, label) {\nreturn label\n}\nJob { label, id } {\nreturn label\n}\nText(text) {\nreturn text\n}\n}\n}\n\nfn signal_priority(signal: Signal): int {\nreturn match signal { Red => 3, Yellow => 2, Green => 1 }\n}\n\nlet first: Message = Pair(7, \"multi\")\nlet second: Message = Job { id: 9, label: \"named\" }\nlet score: int = match Some(7) {\nSome(value) => value + 1\nNone => 0\n}\n\nprint render(first)\nprint render(second)\nprint render(Text(\"payload\"))\nprint signal_priority(Yellow)\nprint score\n",
    )
    .expect("write enum match source");
}

fn write_enum_payload_match_main_exit_project(project: &Path, variant: &str) {
    fs::create_dir_all(project.join("src")).expect("create enum payload match project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-enum-payload-match-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write enum payload match manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-enum-payload-match-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write enum payload match lockfile");
    let value = match variant {
        "Ready" => "Ready { step: Step { value: 48, enabled: true } }",
        "Fallback" => "Fallback { step: Step { enabled: true, value: 49 } }",
        "Off" => "Off",
        other => panic!("unexpected enum payload variant {other}"),
    };
    fs::write(
        project.join("src/main.ax"),
        format!("struct Step {{\nvalue: int\nenabled: bool\n}}\n\nenum Choice {{\nReady {{ step: Step }}\nFallback {{ step: Step }}\nOff\n}}\n\nfn choose_choice(mode: int): Choice {{\nif mode == 0 {{\nlet value: int = 48\nreturn Ready {{ step: Step {{ value: value, enabled: true }} }}\n}} else {{\nlet fallback: int = 49\nreturn Fallback {{ step: Step {{ enabled: true, value: fallback }} }}\n}}\n}}\n\nfn forward_choice(value: Choice): Choice {{\nreturn value\n}}\n\nfn score(choice: Choice): int {{\nlet code: int = 0\nmatch choice {{\nReady {{ step }} {{\nif step.enabled {{\ncode = step.value\n}} else {{\ncode = 2\n}}\n}}\nFallback {{ step }} {{\nif step.enabled {{\ncode = step.value\n}} else {{\ncode = 2\n}}\n}}\nOff {{\ncode = 1\n}}\n}}\nreturn code\n}}\n\nfn main(): int {{\nlet helper_choice: Choice = Off\nlet value_choice: Choice = Off\nlet stmt_choice: Choice = Off\nhelper_choice = {value}\nvalue_choice = {value}\nstmt_choice = {value}\nlet returned_ready: Choice = choose_choice(0)\nlet returned_fallback: Choice = choose_choice(1)\nlet ready_to_forward: Choice = choose_choice(0)\nlet fallback_to_forward: Choice = choose_choice(1)\nlet forwarded_ready: Choice = forward_choice(ready_to_forward)\nlet forwarded_fallback: Choice = forward_choice(fallback_to_forward)\nlet helper_code: int = score(helper_choice)\nlet inline_code: int = score({value})\nlet returned_ready_code: int = score(returned_ready)\nlet returned_fallback_code: int = score(returned_fallback)\nlet forwarded_ready_code: int = score(forwarded_ready)\nlet forwarded_fallback_code: int = score(forwarded_fallback)\nlet match_code: int = match value_choice {{ Ready {{ step }} => step.value, Fallback {{ step }} => step.value, Off => 1 }}\nlet statement_code: int = 0\nmatch stmt_choice {{\nReady {{ step }} {{\nif step.enabled {{\nstatement_code = step.value\n}} else {{\nstatement_code = 2\n}}\n}}\nFallback {{ step }} {{\nif step.enabled {{\nstatement_code = step.value\n}} else {{\nstatement_code = 2\n}}\n}}\nOff {{\nstatement_code = 1\n}}\n}}\nif match_code == statement_code && helper_code == match_code && inline_code == match_code && returned_ready_code == 48 && returned_fallback_code == 49 && forwarded_ready_code == 48 && forwarded_fallback_code == 49 {{\nreturn match_code\n}} else {{\nreturn 2\n}}\n}}\n"),
    )
    .expect("write enum payload match source");
}

fn write_struct_field_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create struct project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-struct-field\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write struct manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-struct-field\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write struct lockfile");
    fs::write(
        project.join("src/main.ax"),
        "struct Pipeline {\nname: string\nsteps: int\nready: bool\n}\n\nfn label(pipeline: Pipeline): string {\nreturn pipeline.name\n}\n\nlet pipeline: Pipeline = Pipeline { name: \"stage1 structs\", steps: 3, ready: true }\nprint pipeline.steps\nprint pipeline.ready\nprint label(pipeline)\n",
    )
    .expect("write struct source");
}

fn write_array_helpers_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create array-helpers project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-array-helpers\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write array-helpers manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-array-helpers\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write array-helpers lockfile");
    fs::write(
        project.join("src/main.ax"),
        "let values: [int; 3] = [10, 20, 30]\nprint len(values)\nprint first(values)\nprint last(values)\nprint first(values) + last(values)\n",
    )
    .expect("write array-helpers source");
}

fn write_borrowed_slice_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create borrowed-slice project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-borrowed-slice\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write borrowed-slice manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-borrowed-slice\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write borrowed-slice lockfile");
    fs::write(
        project.join("src/main.ax"),
        "fn tail(values: &[int]): &[int] {\nreturn values[1:]\n}\n\nlet values: [int] = [2, 4, 6, 8]\nlet window: &[int] = values[1:]\nprint len(window)\nprint first(window)\nprint last(window)\nprint window[1]\nlet nested: &[int] = tail(values[:])\nprint len(nested)\n",
    )
    .expect("write borrowed-slice source");
}

fn write_process_status_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create process-status project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-process-status"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = true
unsafe_rationale = "direct-native process-status regression executes deterministic system helpers"
env = false
clock = false
crypto = false
"#,
    )
    .expect("write process-status manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-process-status"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write process-status lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/process.ax"
print run_status("/usr/bin/true")
print run_status("/usr/bin/false")
print run_status("__axiom_stage1_missing_binary__")
"#,
    )
    .expect("write process-status source");
}

fn write_process_status_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create process-status main project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-process-status-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = true
unsafe_rationale = "direct-native process-status regression executes deterministic system helpers"
env = false
clock = false
crypto = false
"#,
    )
    .expect("write process-status main manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-process-status-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write process-status main lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/process.ax"

fn main(): int {
let ok: int = process_status("/usr/bin/true")
let fail: int = run_status("/usr/bin/false")
let missing: int = run_status("__axiom_stage1_missing_binary__")
if ok == 0 && fail == 1 && missing == -1 {
return 48
} else {
return 1
}
}
"#,
    )
    .expect("write process-status main source");
}

fn write_process_status_unapproved_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create process-status project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-process-status-unapproved"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = true
unsafe_rationale = "direct-native process-status regression rejects unapproved compiler-time commands"
env = false
clock = false
crypto = false
"#,
    )
    .expect("write process-status manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-process-status-unapproved"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write process-status lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/process.ax"
print run_status("/bin/sh")
"#,
    )
    .expect("write process-status source");
}

fn write_owned_move_state_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create owned move project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-owned-move-state"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = true
clock = false
crypto = false

[unsafe_rationale]
env = "Cranelift ABI regression needs a runtime-only projected key index source."
"#,
    )
    .expect("write owned move manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-owned-move-state"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write owned move lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"struct Pair {
name: string
values: [int]
}

let pair: Pair = Pair { name: "left", values: [1, 2, 3] }
let moved: [int] = pair.values
print len(moved)
print pair.name
"#,
    )
    .expect("write owned move source");
}

fn write_map_index_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create map project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-map-index\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write map manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-map-index\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write map lockfile");
    fs::write(
        project.join("src/main.ax"),
        "let scores: {string: int} = {\"build\": 7, \"deploy\": 9, \"deploy\": 11}\nprint scores[\"deploy\"]\n\nlet available: {string: int} = {\"build\": 7, \"deploy\": 9}\nprint map_contains_key<string, int>(available, \"build\")\n\nlet missing: {string: int} = {\"build\": 7, \"deploy\": 9}\nprint map_contains_key<string, int>(missing, \"test\")\n\nlet labels: {int: string} = {1: \"low\", 2: \"high\"}\nprint labels[2]\n\nlet direct_get_scores: {string: int} = {\"build\": 7, \"deploy\": 9}\nlet direct_found: Option<int> = get<string, int>(direct_get_scores, \"deploy\")\nmatch direct_found {\nSome(value) {\nprint value\n}\nNone {\nprint 0\n}\n}\n\nlet direct_hit_scores: {string: int} = {\"build\": 7, \"deploy\": 9}\nprint get_or_default<string, int>(direct_hit_scores, \"deploy\", 13)\n\nlet direct_missing_scores: {string: int} = {\"build\": 7, \"deploy\": 9}\nprint get_or_default<string, int>(direct_missing_scores, \"test\", 13)\n",
    )
    .expect("write map source");
}

fn write_map_get_or_default_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create map get_or_default project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-map-get-or-default-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write map get_or_default manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-map-get-or-default-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write map get_or_default lockfile");
    fs::write(
        project.join("src/main.ax"),
        "static STATIC_LOW_KEY: int = 1\nstatic STATIC_HIGH_KEY: int = 2\nstatic STATIC_MISSING_KEY: int = 3\n\nfn main(): int {\nlet string_hit: int = get_or_default<string, int>({\"build\": 7, \"deploy\": 9}, \"deploy\", 13)\nlet string_miss: int = get_or_default<string, int>({\"build\": 7, \"deploy\": 9}, \"test\", 13)\nlet int_hit: int = get_or_default<int, int>({1: 11, 2: 29}, 2, 13)\nlet static_int_hit: int = get_or_default<int, int>({STATIC_LOW_KEY: 11, STATIC_HIGH_KEY: 29}, STATIC_HIGH_KEY, 13)\nlet bool_hit: int = get_or_default<bool, int>({false: 3, true: 5}, true, 13)\nlet duplicate_hit: int = get_or_default<string, int>({\"deploy\": 9, \"deploy\": 11}, \"deploy\", 13)\nlet duplicate_contains: bool = map_contains_key<string, int>({\"deploy\": 9, \"deploy\": 11}, \"deploy\")\nlet duplicate_direct_get: int = match get<string, int>({\"deploy\": 9, \"deploy\": 11}, \"deploy\") { Some(value) => value, None => 1 }\nlet string_contains: bool = map_contains_key<string, int>({\"build\": 7, \"deploy\": 9}, \"deploy\")\nlet string_missing: bool = map_contains_key<string, int>({\"build\": 7, \"deploy\": 9}, \"test\") == false\nlet int_contains: bool = map_contains_key<int, int>({1: 11, 2: 29}, 1)\nlet static_int_contains: bool = map_contains_key<int, int>({STATIC_LOW_KEY: 11, STATIC_HIGH_KEY: 29}, STATIC_LOW_KEY)\nlet bool_contains: bool = map_contains_key<bool, int>({false: 3, true: 5}, false)\nlet direct_get_hit: int = match get<string, int>({\"build\": 7, \"deploy\": 9}, \"deploy\") { Some(value) => value, None => 1 }\nlet direct_get_miss: int = match get<string, int>({\"build\": 7, \"deploy\": 9}, \"test\") { Some(value) => value, None => 13 }\nlet direct_bool_hit: bool = match get<bool, bool>({false: true, true: false}, false) { Some(value) => value, None => false }\nlet direct_bool_miss: bool = match get<bool, bool>({false: true}, true) { Some(value) => value, None => true }\nlet direct_string_hit_len: int = match get<string, string>({\"build\": \"forge\", \"deploy\": \"ship\"}, \"deploy\") { Some(value) => len(value), None => 1 }\nlet direct_string_miss_len: int = match get<string, string>({\"build\": \"forge\", \"deploy\": \"ship\"}, \"test\") { Some(value) => len(value), None => 13 }\nlet stored_default_scores: {string: int} = {\"build\": 7, \"deploy\": 9}\nlet stored_default_hit: int = get_or_default<string, int>(stored_default_scores, \"deploy\", 13)\nlet stored_contains_scores: {string: int} = {\"build\": 7, \"deploy\": 9}\nlet stored_contains: bool = map_contains_key<string, int>(stored_contains_scores, \"build\")\nlet stored_direct_scores: {string: int} = {\"build\": 7, \"deploy\": 9}\nlet stored_direct_hit: int = match get<string, int>(stored_direct_scores, \"build\") { Some(value) => value, None => 1 }\nlet stored_string_values: {string: string} = {\"build\": \"forge\", \"deploy\": \"ship\"}\nlet stored_string_value_len: int = match get<string, string>(stored_string_values, \"deploy\") { Some(value) => len(value), None => 1 }\nlet stored_key_count_scores: {string: int} = {\"build\": 7, \"deploy\": 9, \"deploy\": 11}\nlet stored_key_count_names: [string] = keys<string, int>(stored_key_count_scores)\nlet stored_key_count: int = len(stored_key_count_names)\nlet first_key_scores: {string: int} = {\"build\": 7, \"deploy\": 9}\nlet first_key_names: [string] = keys<string, int>(first_key_scores)\nlet first_key_len: int = len(first_key_names[0])\nlet second_key_scores: {string: int} = {\"build\": 7, \"deploy\": 9}\nlet second_key_names: [string] = keys<string, int>(second_key_scores)\nlet second_key_len: int = len(second_key_names[1])\nlet local_string_value_hit: Option<string> = get<string, string>({\"build\": \"forge\", \"deploy\": \"ship\"}, \"deploy\")\nlet local_string_value_miss: Option<string> = get<string, string>({\"build\": \"forge\", \"deploy\": \"ship\"}, \"test\")\nlet local_get_hit: Option<int> = get<int, int>({1: 11, 2: 29}, 2)\nlet local_get_miss: Option<int> = get<int, int>({1: 11, 2: 29}, 3)\nlet static_local_get_hit: Option<int> = get<int, int>({STATIC_LOW_KEY: 11, STATIC_HIGH_KEY: 29}, STATIC_HIGH_KEY)\nlet static_local_get_miss: Option<int> = get<int, int>({STATIC_LOW_KEY: 11, STATIC_HIGH_KEY: 29}, STATIC_MISSING_KEY)\nlet local_string_get_hit: Option<int> = get<string, int>({\"build\": 7, \"deploy\": 9}, \"deploy\")\nlet local_bool_get_hit: Option<bool> = get<bool, bool>({false: true, true: false}, false)\nlet local_bool_get_miss: Option<bool> = get<bool, bool>({false: true}, true)\nlet local_get_hit_code: int = match local_get_hit { Some(value) => value, None => 1 }\nlet local_get_miss_code: int = match local_get_miss { Some(value) => value, None => 13 }\nlet static_local_get_hit_code: int = match static_local_get_hit { Some(value) => value, None => 1 }\nlet static_local_get_miss_code: int = match static_local_get_miss { Some(value) => value, None => 13 }\nlet local_string_get_hit_code: int = match local_string_get_hit { Some(value) => value, None => 1 }\nlet local_bool_get_hit_code: bool = match local_bool_get_hit { Some(value) => value, None => false }\nlet local_bool_get_miss_code: bool = match local_bool_get_miss { Some(value) => value, None => true }\nlet local_string_value_hit_len: int = match local_string_value_hit { Some(value) => len(value), None => 1 }\nlet local_string_value_miss_len: int = match local_string_value_miss { Some(value) => len(value), None => 13 }\nif string_hit == 9 && string_miss == 13 && int_hit == 29 && static_int_hit == 29 && bool_hit == 5 && duplicate_hit == 11 && duplicate_contains && duplicate_direct_get == 11 && string_contains && string_missing && int_contains && static_int_contains && bool_contains && direct_get_hit == 9 && direct_get_miss == 13 && direct_bool_hit && direct_bool_miss && direct_string_hit_len == 4 && direct_string_miss_len == 13 && stored_default_hit == 9 && stored_contains && stored_direct_hit == 7 && stored_string_value_len == 4 && stored_key_count == 2 && first_key_len == 5 && second_key_len == 6 && local_get_hit_code == 29 && local_get_miss_code == 13 && static_local_get_hit_code == 29 && static_local_get_miss_code == 13 && local_string_get_hit_code == 9 && local_bool_get_hit_code && local_bool_get_miss_code && local_string_value_hit_len == 4 && local_string_value_miss_len == 13 {\nreturn 48\n} else {\nreturn 1\n}\n}\n",
    )
    .expect("write map get_or_default source");
}

fn write_static_bool_map_keys_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create static bool map keys project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-static-bool-map-keys-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write static bool map keys manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-static-bool-map-keys-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write static bool map keys lockfile");
    fs::write(
        project.join("src/main.ax"),
        "static ENABLED: bool = true\nstatic DISABLED: bool = false\n\nfn main(): int {\nlet static_hit: int = get_or_default<bool, int>({DISABLED: 7, ENABLED: 29}, ENABLED, 13)\nlet static_contains: bool = map_contains_key<bool, int>({DISABLED: 7, ENABLED: 29}, DISABLED)\nlet static_missing: bool = map_contains_key<bool, int>({DISABLED: 7}, ENABLED) == false\nlet direct_bool_hit: bool = match get<bool, bool>({DISABLED: false, ENABLED: true}, ENABLED) { Some(value) => value, None => false }\nlet direct_bool_miss: bool = match get<bool, bool>({DISABLED: true}, ENABLED) { Some(value) => false, None => true }\nlet local_hit: Option<int> = get<bool, int>({DISABLED: 7, ENABLED: 29}, ENABLED)\nlet local_miss: Option<int> = get<bool, int>({DISABLED: 7}, ENABLED)\nlet local_hit_code: int = match local_hit { Some(value) => value, None => 1 }\nlet local_miss_code: int = match local_miss { Some(value) => value, None => 13 }\nif static_hit == 29 && static_contains && static_missing && direct_bool_hit && direct_bool_miss && local_hit_code == 29 && local_miss_code == 13 {\nreturn 48\n} else {\nreturn 1\n}\n}\n",
    )
    .expect("write static bool map keys source");
}

fn write_std_collection_lookup_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create collection lookup project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-collection-lookup\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write collection lookup manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-collection-lookup\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write collection lookup lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/collections.ax"

let scores: {string: int} = {"build": 7, "deploy": 9}
print contains<string, int>(scores, "deploy")

let deploy_scores: {string: int} = {"build": 7, "deploy": 9}
let found: Option<int> = get<string, int>(deploy_scores, "deploy")
match found {
Some(value) {
print value
}
None {
print 0
}
}

let missing_scores: {string: int} = {"build": 7, "deploy": 9}
let missing: Option<int> = get<string, int>(missing_scores, "test")
match missing {
Some(value) {
print value
}
None {
print 0
}
}

let fallback_scores: {string: int} = {"build": 7, "deploy": 9}
print get_or_default<string, int>(fallback_scores, "test", 13)

let key_count_scores: {string: int} = {"build": 7, "deploy": 9}
let key_count_names: [string] = keys<string, int>(key_count_scores)
print len(key_count_names)

let first_key_scores: {string: int} = {"build": 7, "deploy": 9}
let first_key_names: [string] = keys<string, int>(first_key_scores)
print first_key_names[0]

let second_key_scores: {string: int} = {"build": 7, "deploy": 9}
let second_key_names: [string] = keys<string, int>(second_key_scores)
print second_key_names[1]
"#,
    )
    .expect("write collection lookup source");
}

fn write_std_collection_wrapper_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src"))
        .expect("create std collection wrapper main project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-std-collection-wrapper-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write std collection wrapper main manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-std-collection-wrapper-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write std collection wrapper main lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/collections.ax"
fn choose_key_index(found: bool): int {
if found {
return 1
} else {
return 0
}
}

fn main(): int {
let contains_hit_scores: {string: int} = {"build": 7, "deploy": 9}
let contains_hit: bool = contains<string, int>(contains_hit_scores, "deploy")
let contains_miss_scores: {string: int} = {"build": 7, "deploy": 9}
let contains_miss: bool = contains<string, int>(contains_miss_scores, "test") == false
let get_hit_scores: {string: int} = {"build": 7, "deploy": 9}
let get_hit_code: int = match get<string, int>(get_hit_scores, "deploy") { Some(value) => value, None => 1 }
let get_miss_scores: {string: int} = {"build": 7, "deploy": 9}
let get_miss_code: int = match get<string, int>(get_miss_scores, "test") { Some(value) => value, None => 13 }
let fallback_scores: {string: int} = {"build": 7, "deploy": 9}
let fallback: int = get_or_default<string, int>(fallback_scores, "test", 13)
let key_count_scores: {string: int} = {"build": 7, "deploy": 9, "deploy": 11}
let key_count_names: [string] = keys<string, int>(key_count_scores)
let key_count: int = len(key_count_names)
let first_key_scores: {string: int} = {"build": 7, "deploy": 9}
let first_key_names: [string] = keys<string, int>(first_key_scores)
let first_key_len: int = len(first_key_names[0])
let second_key_scores: {string: int} = {"build": 7, "deploy": 9}
let second_key_names: [string] = keys<string, int>(second_key_scores)
let second_key_len: int = len(second_key_names[1])
let dynamic_key_scores: {string: int} = {"build": 7, "deploy": 9}
let dynamic_key_names: [string] = keys<string, int>(dynamic_key_scores)
let dynamic_key_index: int = choose_key_index(contains_hit)
let dynamic_key_len: int = len(dynamic_key_names[dynamic_key_index])
let dynamic_key_eq_scores: {string: int} = {"build": 7, "deploy": 9}
let dynamic_key_eq_names: [string] = keys<string, int>(dynamic_key_eq_scores)
let dynamic_key_eq_value: string = dynamic_key_eq_names[dynamic_key_index]
let dynamic_key_is_deploy: bool = dynamic_key_eq_value == "deploy"
let dynamic_key_ne_scores: {string: int} = {"build": 7, "deploy": 9}
let dynamic_key_ne_names: [string] = keys<string, int>(dynamic_key_ne_scores)
let dynamic_key_ne_value: string = dynamic_key_ne_names[dynamic_key_index]
let dynamic_key_not_build: bool = dynamic_key_ne_value != "build"
let dynamic_key_prefix_scores: {string: int} = {"build": 7, "deploy": 9}
let dynamic_key_prefix_names: [string] = keys<string, int>(dynamic_key_prefix_scores)
let dynamic_key_prefix_value: string = dynamic_key_prefix_names[dynamic_key_index]
let dynamic_key_has_prefix: bool = string_starts_with(dynamic_key_prefix_value, "dep")
let dynamic_key_trim_scores: {string: int} = {" build ": 7, " deploy ": 9}
let dynamic_key_trim_names: [string] = keys<string, int>(dynamic_key_trim_scores)
let dynamic_key_trim_value: string = dynamic_key_trim_names[dynamic_key_index]
let dynamic_key_trim_len: int = len(string_trim(dynamic_key_trim_value))
let dynamic_key_trim_start_len: int = len(string_trim_start(dynamic_key_trim_value))
let dynamic_key_trimmed_value: string = string_trim(dynamic_key_trim_value)
let dynamic_key_trim_start_value: string = string_trim_start(dynamic_key_trim_value)
let dynamic_key_trimmed_has_prefix: bool = string_starts_with(dynamic_key_trimmed_value, "dep")
let dynamic_key_trim_start_has_prefix: bool = string_starts_with(dynamic_key_trim_start_value, "dep")
if contains_hit && contains_miss && get_hit_code == 9 && get_miss_code == 13 && fallback == 13 && key_count == 2 && first_key_len == 5 && second_key_len == 6 && dynamic_key_len == 6 && dynamic_key_is_deploy && dynamic_key_not_build && dynamic_key_has_prefix && dynamic_key_trim_len == 6 && dynamic_key_trim_start_len == 7 && dynamic_key_trimmed_has_prefix && dynamic_key_trim_start_has_prefix {
return 48
} else {
return 1
}
}
"#,
    )
    .expect("write std collection wrapper main source");
}

fn write_net_resolve_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create net resolve project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-net-resolve"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = { hosts = ["localhost"], ports = [] }
process = false
env = false
clock = false
crypto = false

[unsafe_rationale]
net = "Cranelift ABI regression covers std/net.ax localhost DNS resolution for issue 928."
"#,
    )
    .expect("write net resolve manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-net-resolve"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write net resolve lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/net.ax"

match resolve("localhost") {
Some(address) {
print len(address) > 0
}
None {
print false
}
}
"#,
    )
    .expect("write net resolve source");
}

fn write_net_resolve_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create net resolve main project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-net-resolve-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = { hosts = ["localhost"], ports = [] }
process = false
env = false
clock = false
crypto = false

[unsafe_rationale]
net = "Direct-native DNS regression covers std/net.ax localhost resolution for issue 928."
"#,
    )
    .expect("write net resolve main manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-net-resolve-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write net resolve main lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/net.ax"

fn main(): int {
let resolved_len: int = match resolve("localhost") { Some(address) => len(address), None => 0 }
let stored_resolved: Option<string> = resolve("localhost")
let stored_direct: Option<string> = net_resolve("localhost")
let stored_statement: Option<string> = resolve("localhost")
let stored_resolved_len: int = match stored_resolved { Some(address) => len(address), None => 0 }
let stored_direct_len: int = match stored_direct { Some(address) => len(address), None => 0 }
let statement_len: int = 0
match stored_statement {
Some(address) {
statement_len = len(address)
}
None {
statement_len = 0
}
}
if resolved_len > 0 && stored_resolved_len == resolved_len && stored_direct_len == resolved_len && statement_len == resolved_len {
return 48
} else {
return 1
}
}
"#,
    )
    .expect("write net resolve main source");
}

fn write_net_loopback_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create net loopback project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-net-loopback"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = { hosts = ["127.0.0.1"], ports = [] }
process = false
env = false
clock = false
crypto = false

[unsafe_rationale]
net = "Cranelift ABI regression covers std/net.ax TCP and UDP loopback helpers for issue 928."
"#,
    )
    .expect("write net loopback manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-net-loopback"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write net loopback lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/net.ax"

match tcp_listen_loopback_once("tcp-pong", 1000) {
Some(port) {
print port > 0
}
None {
print false
}
}

match udp_bind_loopback_once("udp-pong", 1000) {
Some(port) {
print port > 0
}
None {
print false
}
}
"#,
    )
    .expect("write net loopback source");
}

fn write_net_loopback_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create net loopback main project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-net-loopback-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = { hosts = ["127.0.0.1"], ports = [] }
process = false
env = false
clock = false
crypto = false

[unsafe_rationale]
net = "Direct-native loopback regression covers std/net.ax TCP and UDP helpers for issue 928."
"#,
    )
    .expect("write net loopback main manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-net-loopback-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write net loopback main lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/net.ax"

fn main(): int {
let tcp_expr: Option<int> = tcp_listen_loopback_once("tcp-pong", 1000)
let tcp_statement: Option<int> = tcp_listen_loopback_once("tcp-pong", 1000)
let udp_expr: Option<int> = udp_bind_loopback_once("udp-pong", 1000)
let udp_statement: Option<int> = udp_bind_loopback_once("udp-pong", 1000)
let tcp_port: int = match tcp_expr { Some(port) => port, None => 0 }
let udp_port: int = match udp_expr { Some(port) => port, None => 0 }
let tcp_statement_port: int = 0
match tcp_statement {
Some(port) {
tcp_statement_port = port
}
None {
tcp_statement_port = 0
}
}
let udp_statement_port: int = 0
match udp_statement {
Some(port) {
udp_statement_port = port
}
None {
udp_statement_port = 0
}
}
if tcp_port > 0 && udp_port > 0 && tcp_statement_port > 0 && udp_statement_port > 0 {
return 48
} else {
return 1
}
}
"#,
    )
    .expect("write net loopback main source");
}

fn start_http_fixture_server(body: &'static str) -> (u16, std::thread::JoinHandle<()>) {
    start_http_fixture_server_requests(body, 1)
}

fn start_http_fixture_server_requests(
    body: &'static str,
    requests: usize,
) -> (u16, std::thread::JoinHandle<()>) {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind http fixture");
    listener
        .set_nonblocking(true)
        .expect("set http fixture nonblocking");
    let port = listener.local_addr().expect("http fixture addr").port();
    let handle = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut accepted = 0;
        while accepted < requests {
            match listener.accept() {
                Ok((mut stream, _peer)) => {
                    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                    let mut request = [0u8; 4096];
                    let _ = std::io::Read::read(&mut stream, &mut request);
                    let response = format!(
                        "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    std::io::Write::write_all(&mut stream, response.as_bytes())
                        .expect("write http fixture response");
                    let _ = std::io::Write::flush(&mut stream);
                    accepted += 1;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        panic!("timed out waiting for http fixture request");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(err) => panic!("accept http fixture request: {err}"),
            }
        }
    });
    (port, handle)
}

fn reserve_loopback_port() -> Option<u16> {
    let listener = match std::net::TcpListener::bind(("127.0.0.1", 0)) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("skipping cranelift http server test; cannot bind 127.0.0.1:0: {err}");
            return None;
        }
    };
    Some(listener.local_addr().expect("loopback addr").port())
}

fn start_http_server_probe_client(port: u16) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut stream = loop {
            match std::net::TcpStream::connect(("127.0.0.1", port)) {
                Ok(stream) => break stream,
                Err(err) if std::time::Instant::now() < deadline => {
                    let _ = err;
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(err) => panic!("http server probe never connected: {err}"),
            }
        };
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("set probe read timeout");
        let request =
            "POST /server HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Length: 13\r\n\r\naxiom-request";
        std::io::Write::write_all(&mut stream, request.as_bytes())
            .expect("write http server probe request");
        stream
            .shutdown(std::net::Shutdown::Write)
            .expect("shutdown http server probe request");
        let mut response = String::new();
        std::io::Read::read_to_string(&mut stream, &mut response)
            .expect("read http server probe response");
        response
    })
}

fn start_http_route_probe_client(port: u16, path: &'static str) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut stream = loop {
            match std::net::TcpStream::connect(("127.0.0.1", port)) {
                Ok(stream) => break stream,
                Err(err) if std::time::Instant::now() < deadline => {
                    let _ = err;
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(err) => panic!("http route probe never connected: {err}"),
            }
        };
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("set route probe read timeout");
        let request = format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n");
        std::io::Write::write_all(&mut stream, request.as_bytes())
            .expect("write http route probe request");
        stream
            .shutdown(std::net::Shutdown::Write)
            .expect("shutdown http route probe request");
        let mut response = String::new();
        std::io::Read::read_to_string(&mut stream, &mut response)
            .expect("read http route probe response");
        response
    })
}

fn write_http_client_project(project: &Path, port: u16) {
    fs::create_dir_all(project.join("src")).expect("create http client project src");
    fs::write(
        project.join("axiom.toml"),
        format!(
            r#"[package]
name = "cranelift-http-client"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = {{ hosts = ["127.0.0.1"], ports = [{port}] }}
process = false
env = false
clock = false
crypto = false

[unsafe_rationale]
net = "Cranelift ABI regression covers std/http.ax local HTTP GET for issue 928."
"#
        ),
    )
    .expect("write http client manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-http-client"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write http client lockfile");
    fs::write(
        project.join("src/main.ax"),
        format!(
            r#"import "std/http.ax"

match get("http://127.0.0.1:{port}/health") {{
Some(body) {{
print body
}}
None {{
print "missing"
}}
}}
"#
        ),
    )
    .expect("write http client source");
}

fn write_http_client_main_exit_project(project: &Path, port: u16) {
    fs::create_dir_all(project.join("src")).expect("create http client main project src");
    fs::write(
        project.join("axiom.toml"),
        format!(
            r#"[package]
name = "cranelift-http-client-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = {{ hosts = ["127.0.0.1"], ports = [{port}] }}
process = false
env = false
clock = false
crypto = false

[unsafe_rationale]
net = "Direct-native HTTP client regression covers std/http.ax local GET for issue 928."
"#
        ),
    )
    .expect("write http client main manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-http-client-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write http client main lockfile");
    fs::write(
        project.join("src/main.ax"),
        format!(
            r#"import "std/http.ax"

fn main(): int {{
let stored_expr: Option<string> = get("http://127.0.0.1:{port}/health")
let stored_statement: Option<string> = get("http://127.0.0.1:{port}/health")
let body_len: int = match stored_expr {{ Some(body) => len(body), None => 0 }}
let statement_len: int = 0
match stored_statement {{
Some(body) {{
statement_len = len(body)
}}
None {{
statement_len = 0
}}
}}
if body_len == 13 && statement_len == 13 {{
return 48
}} else {{
return 1
}}
}}
"#
        ),
    )
    .expect("write http client main source");
}

fn write_http_server_project(project: &Path, port: u16) {
    fs::create_dir_all(project.join("src")).expect("create http server project src");
    fs::write(
        project.join("axiom.toml"),
        format!(
            r#"[package]
name = "cranelift-http-server"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = {{ hosts = ["127.0.0.1"], ports = [{port}] }}
process = false
env = false
clock = false
crypto = false

[unsafe_rationale]
net = "Cranelift ABI regression covers std/http.ax local HTTP server primitives for issue 928."
"#
        ),
    )
    .expect("write http server manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-http-server"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write http server lockfile");
    fs::write(
        project.join("src/main.ax"),
        format!(
            r#"let server: int = http_server_listen("127.0.0.1:{port}")
print http_server_local_port(server) == {port}

let request: int = http_server_accept(server)
print http_request_method(request)
print http_request_path(request)
print http_request_body(request)
print http_response_write(request, 201, "axiom-response")
print http_server_close(server)
"#
        ),
    )
    .expect("write http server source");
}

fn write_http_server_once_main_exit_project(project: &Path, port: u16) {
    fs::create_dir_all(project.join("src")).expect("create http server once main project src");
    fs::write(
        project.join("axiom.toml"),
        format!(
            r#"[package]
name = "cranelift-http-server-once-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = {{ hosts = ["127.0.0.1"], ports = [{port}] }}
process = false
env = false
clock = false
crypto = false

[unsafe_rationale]
net = "Direct-native HTTP server regression covers std/http.ax serve_once for issue 928."
"#
        ),
    )
    .expect("write http server once main manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-http-server-once-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write http server once main lockfile");
    fs::write(
        project.join("src/main.ax"),
        format!(
            r#"import "std/http.ax"

fn main(): int {{
let served: bool = serve_once("127.0.0.1:{port}", "server-once-ok")
if served {{
return 48
}} else {{
return 1
}}
}}
"#
        ),
    )
    .expect("write http server once main source");
}

fn write_http_server_route_main_exit_project(project: &Path, port: u16) {
    fs::create_dir_all(project.join("src")).expect("create http server route main project src");
    fs::write(
        project.join("axiom.toml"),
        format!(
            r#"[package]
name = "cranelift-http-server-route-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = {{ hosts = ["127.0.0.1"], ports = [{port}] }}
process = false
env = false
clock = false
crypto = false

[unsafe_rationale]
net = "Direct-native HTTP server regression covers http_serve_route for issue 928."
"#
        ),
    )
    .expect("write http server route main manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-http-server-route-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write http server route main lockfile");
    fs::write(
        project.join("src/main.ax"),
        format!(
            r#"fn main(): int {{
let routed: bool = http_serve_route("127.0.0.1:{port}", "/route", "route-ok", 2)
if routed {{
return 48
}} else {{
return 1
}}
}}
"#
        ),
    )
    .expect("write http server route main source");
}

fn write_http_async_server_project(project: &Path, port: u16) {
    fs::create_dir_all(project.join("src")).expect("create http async server project src");
    fs::write(
        project.join("axiom.toml"),
        format!(
            r#"[package]
name = "cranelift-http-async-server"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = {{ hosts = ["127.0.0.1"], ports = [{port}] }}
process = false
env = false
clock = false
crypto = false
async = true

[unsafe_rationale]
net = "Cranelift ABI regression covers std/http_async.ax local async HTTP route serving for issue 928."
async = "Cranelift ABI regression covers std/http_async.ax local async HTTP route serving for issue 928."
"#
        ),
    )
    .expect("write http async server manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-http-async-server"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write http async server lockfile");
    fs::write(
        project.join("src/main.ax"),
        format!(
            r#"let server: int = http_server_listen("127.0.0.1:{port}")
let served: Task<bool> = http_async_serve_route(server, "/ready", "async-response", 1)
let ready: bool = await served
print ready
print http_server_close(server)
"#
        ),
    )
    .expect("write http async server source");
}

fn write_float_map_key_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create float map project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-float-map-key\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write float map manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-float-map-key\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write float map lockfile");
    fs::write(
        project.join("src/main.ax"),
        "let scores: {f64: int} = {1.5f64: 7}\nprint scores[1.5f64]\n",
    )
    .expect("write float map source");
}

fn write_crypto_hash_project(project: &Path, crypto: bool) {
    fs::create_dir_all(project.join("src")).expect("create crypto hash project src");
    fs::write(
        project.join("axiom.toml"),
        format!(
            "[package]\nname = \"cranelift-crypto-hash\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = {crypto}\n"
        ),
    )
    .expect("write crypto hash manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-crypto-hash\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write crypto hash lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/crypto_hash.ax\"\nprint sha256(\"abc\")\n",
    )
    .expect("write crypto hash source");
}

fn write_crypto_mac_project(project: &Path, crypto: bool) {
    fs::create_dir_all(project.join("src")).expect("create crypto mac project src");
    fs::write(
        project.join("axiom.toml"),
        format!(
            "[package]\nname = \"cranelift-crypto-mac\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = {crypto}\n"
        ),
    )
    .expect("write crypto mac manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-crypto-mac\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write crypto mac lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/crypto_mac.ax\"\nlet left: [u8] = [1u8, 2u8, 3u8]\nlet same: [u8] = [1u8, 2u8, 3u8]\nlet different: [u8] = [1u8, 2u8, 4u8]\nprint hmac_sha256(\"key\", \"The quick brown fox jumps over the lazy dog\")\nprint hmac_sha512(\"Jefe\", \"what do ya want for nothing?\")\nprint verify_sha256(\"f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8\", \"key\", \"The quick brown fox jumps over the lazy dog\")\nprint verify_sha512(\"164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea2505549758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737\", \"Jefe\", \"what do ya want for nothing?\")\nprint constant_time_eq(hmac_sha256(\"key\", \"The quick brown fox jumps over the lazy dog\"), \"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\")\nprint constant_time_eq(\"short\", \"shorter\")\nprint constant_time_eq_u8(left[:], same[:])\nprint constant_time_eq_u8(left[:], different[:])\n",
    )
    .expect("write crypto mac source");
}

fn write_crypto_random_project(project: &Path, crypto: bool) {
    fs::create_dir_all(project.join("src")).expect("create crypto random project src");
    fs::write(
        project.join("axiom.toml"),
        format!(
            "[package]\nname = \"cranelift-crypto-random\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = {crypto}\n"
        ),
    )
    .expect("write crypto random manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-crypto-random\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write crypto random lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/crypto_rand.ax\"\nlet sample: [u8] = random_bytes(16)\nprint len(sample)\nlet empty: [u8] = random_bytes(0)\nprint len(empty)\nlet value: u64 = random_u64()\nprint value == value\n",
    )
    .expect("write crypto random source");
}

fn write_crypto_random_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create crypto random project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-crypto-random-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = true\n\n[unsafe_rationale]\ncrypto = \"Direct-native random_bytes length and random_u64 regression covers std/crypto_rand.ax for issue 1001.\"\n",
    )
    .expect("write crypto random main manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-crypto-random-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write crypto random main lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/crypto_rand.ax\"\n\nstatic RANDOM_LEN: int = 16\n\nfn main(): int {\nlet sample_len: int = len(random_bytes(RANDOM_LEN))\nlet empty_len: int = len(random_bytes(0))\nlet value: int = random_u64() as int\nif sample_len == RANDOM_LEN && empty_len == 0 && value == 48 {\nreturn 48\n} else {\nreturn 1\n}\n}\n",
    )
    .expect("write crypto random main source");
}

fn write_crypto_signature_project(project: &Path, crypto: bool) {
    fs::create_dir_all(project.join("src")).expect("create crypto signature project src");
    fs::write(
        project.join("axiom.toml"),
        format!(
            "[package]\nname = \"cranelift-crypto-signature\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = {crypto}\n"
        ),
    )
    .expect("write crypto signature manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-crypto-signature\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write crypto signature lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/crypto_sign.ax\"\nlet message: [u8] = [104u8, 101u8, 108u8, 108u8, 111u8]\nlet keys: ([u8], [u8]) = ed25519_keygen()\nlet public_key: [u8] = keys.0\nlet secret_key: [u8] = keys.1\nlet signature: [u8] = ed25519_sign(secret_key[:], message[:])\nprint ed25519_verify(public_key[:], message[:], signature[:])\n",
    )
    .expect("write crypto signature source");
}

fn write_crypto_aead_project(project: &Path, crypto: bool) {
    fs::create_dir_all(project.join("src")).expect("create crypto AEAD project src");
    fs::write(
        project.join("axiom.toml"),
        format!(
            "[package]\nname = \"cranelift-crypto-aead\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = {crypto}\n"
        ),
    )
    .expect("write crypto AEAD manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-crypto-aead\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write crypto AEAD lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/crypto_aead.ax\"\nlet key: [u8] = [0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8, 8u8, 9u8, 10u8, 11u8, 12u8, 13u8, 14u8, 15u8, 16u8, 17u8, 18u8, 19u8, 20u8, 21u8, 22u8, 23u8, 24u8, 25u8, 26u8, 27u8, 28u8, 29u8, 30u8, 31u8]\nlet nonce: [u8] = [0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8, 8u8, 9u8, 10u8, 11u8]\nlet aad: [u8] = [97u8, 97u8, 100u8]\nlet plaintext: [u8] = [104u8, 101u8, 108u8, 108u8, 111u8]\nlet ciphertext: [u8] = aead_seal(Aes256Gcm, key[:], nonce[:], aad[:], plaintext[:])\nmatch aead_open(Aes256Gcm, key[:], nonce[:], aad[:], ciphertext[:]) {\nSome(opened) {\nprint len(opened)\n}\nNone {\nprint 0\n}\n}\n",
    )
    .expect("write crypto AEAD source");
}

fn write_sync_primitives_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create sync primitives project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-sync-primitives\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write sync primitives manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-sync-primitives\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write sync primitives lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/sync.ax\"\n\nlet counter: Mutex<int> = mutex<int>(1)\nlet guard: MutexGuard<int> = lock<int>(counter)\nlet updated: Mutex<int> = replace<int>(guard, 2)\nlet final_guard: MutexGuard<int> = lock<int>(updated)\nprint into_inner<int>(final_guard)\n\nlet ready: Once<string> = once_with<string>(\"configured\")\nprint once_is_set<string>(ready)\n\nlet empty: Once<int> = once<int>(None)\nmatch once_take<int>(empty) {\nSome(value) {\nprint value\n}\nNone {\nprint \"empty\"\n}\n}\n\nlet channel: Channel<string> = channel<string>(None)\nlet sent: Channel<string> = send<string>(channel, \"message\")\nmatch try_recv<string>(sent) {\nSome(message) {\nprint message\n}\nNone {\nprint \"missing\"\n}\n}\n",
    )
    .expect("write sync primitives source");
}

fn write_sync_mutex_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create sync mutex main project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-sync-mutex-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write sync mutex main manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-sync-mutex-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write sync mutex main lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/sync.ax\"\n\nfn main(): int {\nlet counter: Mutex<int> = mutex<int>(1)\nlet guard: MutexGuard<int> = lock<int>(counter)\nlet updated: Mutex<int> = replace<int>(guard, 48)\nlet final_guard: MutexGuard<int> = lock<int>(updated)\nreturn into_inner<int>(final_guard)\n}\n",
    )
    .expect("write sync mutex main source");
}

fn write_sync_once_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create sync once main project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-sync-once-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write sync once main manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-sync-once-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write sync once main lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/sync.ax\"\n\nfn main(): int {\nlet present_cell: Once<int> = once_with<int>(21)\nlet missing_cell: Once<int> = once<int>(None)\nlet ready_cell: Once<int> = once_with<int>(0)\nlet empty_cell: Once<int> = once<int>(None)\nlet bool_cell: Once<bool> = once_with<bool>(true)\nlet present: Option<int> = once_take<int>(present_cell)\nlet present_score: int = match present { Some(value) => value, None => 4 }\nlet missing: Option<int> = once_take<int>(missing_cell)\nlet missing_score: int = match missing { Some(value) => value, None => 19 }\nlet ready: bool = once_is_set<int>(ready_cell)\nlet empty: bool = once_is_set<int>(empty_cell)\nlet bool_ready: Option<bool> = once_take<bool>(bool_cell)\nlet bool_present: bool = match bool_ready { Some(value) => value, None => false }\nif ready && (empty == false) && bool_present {\nreturn present_score + missing_score\n} else {\nreturn 2\n}\n}\n",
    )
    .expect("write sync once main source");
}

fn write_sync_channel_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create sync channel main project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-sync-channel-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write sync channel main manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-sync-channel-main-exit\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write sync channel main lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/sync.ax\"\n\nfn main(): int {\nlet channel: Channel<int> = channel<int>(None)\nlet sent: Channel<int> = send<int>(channel, 31)\nlet empty: Channel<int> = channel<int>(None)\nlet bool_channel: Channel<bool> = channel<bool>(None)\nlet bool_sent: Channel<bool> = send<bool>(bool_channel, true)\nlet present: Option<int> = try_recv<int>(sent)\nlet present_score: int = match present { Some(value) => value, None => 4 }\nlet missing: Option<int> = try_recv<int>(empty)\nlet missing_score: int = match missing { Some(value) => value, None => 17 }\nlet ready: Option<bool> = try_recv<bool>(bool_sent)\nlet ready_present: bool = match ready { Some(value) => value, None => false }\nif ready_present {\nreturn present_score + missing_score + 2\n} else {\nreturn 5\n}\n}\n",
    )
    .expect("write sync channel main source");
}

fn write_std_async_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create std async project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-std-async"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
async = true

[unsafe_rationale]
async = "Cranelift ABI regression covers compiler-side std/async.ax evaluation for issue 928."
"#,
    )
    .expect("write std async manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-std-async"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write std async lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/async.ax"

async fn compute(value: int): int {
return value + 1
}

let direct: Task<int> = compute(40)
print await direct

let handle: JoinHandle<int> = spawn<int>(compute(6))
print await join<int>(handle)

let canceled: Task<int> = cancel<int>(compute(1))
print is_canceled<int>(canceled)

let maybe: Option<int> = await timeout<int>(compute(5), 100)
match maybe {
Some(value) {
print value
}
None {
print 0
}
}

let messages: AsyncChannel<string> = channel<string>()
let sent: AsyncChannel<string> = await send<string>(messages, "message")
let received: Option<string> = await recv<string>(sent)
match received {
Some(message) {
print message
}
None {
print "missing"
}
}

let left_index: Task<Option<string>> = ready<Option<string>>(None)
let right_index: Task<Option<string>> = ready<Option<string>>(Some("right"))
let picked_index: SelectResult<string> = await select<string>(left_index, right_index)
print selected<string>(picked_index)

let left_value: Task<Option<string>> = ready<Option<string>>(None)
let right_value: Task<Option<string>> = ready<Option<string>>(Some("right"))
let picked_value: SelectResult<string> = await select<string>(left_value, right_value)
match selected_value<string>(picked_value) {
Some(value) {
print value
}
None {
print "none"
}
}
"#,
    )
    .expect("write std async source");
}

fn write_std_async_net_tcp_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create std async net TCP project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-std-async-net-tcp"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
"fs:write" = false
net = true
process = false
env = false
clock = false
crypto = false
ffi = false
async = true

[unsafe_rationale]
net = "Cranelift ABI regression covers compiler-side std/async_net.ax loopback TCP evaluation for issue 928."
async = "Cranelift ABI regression covers compiler-side std/async_net.ax loopback TCP evaluation for issue 928."
"#,
    )
    .expect("write std async net TCP manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-std-async-net-tcp"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write std async net TCP lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/async.ax"
import "std/async_net.ax"

async fn echo_once(listener: TcpListener): int {
let stream: TcpStream = await accept(listener)
let received: string = await recv_text(stream, 64)
let _written: int = await send_text(stream, received)
return close(stream)
}

let listener: TcpListener = await listen("127.0.0.1:0")
let port: int = local_port(listener)
let first_handler: JoinHandle<int> = spawn<int>(echo_once(listener))
let second_handler: JoinHandle<int> = spawn<int>(echo_once(listener))
let first_client: JoinHandle<Option<string>> = spawn<Option<string>>(tcp_dial("127.0.0.1", port, "alpha", 1000))
let second_client: JoinHandle<Option<string>> = spawn<Option<string>>(tcp_dial("127.0.0.1", port, "beta", 1000))

match await join<Option<string>>(first_client) {
Some(reply) {
print reply
}
None {
print "first none"
}
}

match await join<Option<string>>(second_client) {
Some(reply) {
print reply
}
None {
print "second none"
}
}

let _first_done: int = await join<int>(first_handler)
let _second_done: int = await join<int>(second_handler)
let _listener_closed: int = close_listener(listener)
"#,
    )
    .expect("write std async net TCP source");
}

fn write_logging_stdio_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create logging stdio project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-logging-stdio"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write logging stdio manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-logging-stdio"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write logging stdio lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/io.ax"

let direct: int = eprintln("hello stderr")
print direct > 0
"#,
    )
    .expect("write logging stdio source");
}

fn write_logging_stdio_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create logging stdio main project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-logging-stdio-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false

[unsafe_rationale]
stdio = "Direct-native stderr regression covers std/io.ax eprintln for issue 1001."
"#,
    )
    .expect("write logging stdio main manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-logging-stdio-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write logging stdio main lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/io.ax"
import "std/json.ax"

fn main(): int {
let status: int = 0
let scores: {string: int} = {"build": 7, "deploy": 9}
let names: [string] = keys<string, int>(scores)
let selected_index: int = 1
let selected_line: string = names[selected_index]
status = status + 1
let first: int = eprintln("after assign")
if status == 1 {
let branch_line: string = "branch stderr"
let branch: int = eprintln(branch_line + " local")
status = branch
} else {
status = 1
}
let tail: int = eprintln("tail stderr")
let bool_line: string = stringify_bool(status == 20)
let quoted_bool_line: string = stringify_bool(status == 20)
let bool_written: int = eprintln(bool_line)
let number_written: int = eprintln(stringify_int(status + bool_written))
let quoted_bool_written: int = eprintln(stringify_string(quoted_bool_line))
let quoted_number_text: string = stringify_int(status + bool_written)
let quoted_number_written: int = eprintln(stringify_string(quoted_number_text))
let selected_written: int = eprintln(selected_line)
return first + status + tail + bool_written + number_written + quoted_bool_written + quoted_number_written + selected_written
}
"#,
    )
    .expect("write logging stdio main source");
}

fn write_print_stdio_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create print stdio main project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-print-stdio-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false

[unsafe_rationale]
stdio = "Direct-native stdout regression covers source print statements for issue 1001."
"#,
    )
    .expect("write print stdio main manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-print-stdio-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write print stdio main lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"static STATIC_LINE: string = "static stdout"

fn helper_line(): string {
return "helper stdout"
}

fn main(): int {
let line: string = "main stdout"
let scores: {string: int} = {"build": 7, "deploy": 9}
let names: [string] = keys<string, int>(scores)
let selected_index: int = 1
let selected_line: string = names[selected_index]
print line
print string_clone(line)
print STATIC_LINE
print line + " suffix"
let status: int = 0
status = status + 7
if status == 7 {
let branch_line: string = "branch stdout"
print branch_line + " local"
} else {
print "else stdout"
}
print helper_line()
print selected_line
return status
}
"#,
    )
    .expect("write print stdio main source");
}

fn write_bool_print_stdio_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create bool print stdio main project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-bool-print-stdio-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false

[unsafe_rationale]
stdio = "Direct-native stdout regression covers boolean source print statements for issue 1001."
"#,
    )
    .expect("write bool print stdio main manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-bool-print-stdio-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write bool print stdio main lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/io.ax"

fn main(): int {
let direct: int = eprintln("hello stderr")
print direct > 0
print direct == 13
return direct
}
"#,
    )
    .expect("write bool print stdio main source");
}

fn write_integer_print_stdio_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create integer print stdio main project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-integer-print-stdio-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false

[unsafe_rationale]
stdio = "Direct-native stdout regression covers integer source print statements for issue 1001."
"#,
    )
    .expect("write integer print stdio main manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-integer-print-stdio-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write integer print stdio main lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"fn score(): int {
return 40 + 2
}

fn negative_score(): int {
return -3
}

fn zero_score(): int {
return 0
}

fn main(): int {
let value: int = score()
print value
let negative: int = negative_score()
print negative
let zero: int = zero_score()
print zero
return value
}
"#,
    )
    .expect("write integer print stdio main source");
}

fn write_json_stringify_print_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create json stringify print project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-json-stringify-print-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write json stringify print manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-json-stringify-print-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write json stringify print lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/json.ax"

fn score(): int {
return 40 + 2
}

fn main(): int {
let value: int = score()
let text: string = stringify_int(value)
print text
print stringify_string(text)
print stringify_bool(value == 42)
let disabled: string = stringify_bool(false)
print disabled
print stringify_string(disabled)
return value
}
"#,
    )
    .expect("write json stringify print source");
}

fn write_helper_eprintln_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create helper eprintln project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-helper-eprintln-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false

[unsafe_rationale]
stdio = "Direct-native stderr regression covers helper std/io.ax eprintln for issue 1001."
"#,
    )
    .expect("write helper eprintln manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-helper-eprintln-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write helper eprintln lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/io.ax"
import "std/json.ax"

static STATIC_LINE: string = "helper static"

fn helper_line(): string {
return "helper text"
}

fn emit(): int {
let first_line: string = "helper stderr"
let clone_line: string = "helper stderr"
let concat_line: string = "helper stderr"
let scores: {string: int} = {"build": 7, "deploy": 9}
let names: [string] = keys<string, int>(scores)
let selected_index: int = 1
let selected_line: string = names[selected_index]
let first: int = eprintln(first_line)
let cloned: int = eprintln(string_clone(clone_line))
let static_written: int = eprintln(STATIC_LINE)
let concat_written: int = eprintln(concat_line + " suffix")
let helper_written: int = eprintln(helper_line())
let value: int = 40 + 2
let text: string = stringify_int(value)
let int_written: int = eprintln(text)
let bool_written: int = eprintln(stringify_bool(value == 42))
let selected_written: int = eprintln(selected_line)
return first + cloned + static_written + concat_written + helper_written + int_written + bool_written + selected_written
}

fn main(): int {
return emit()
}
"#,
    )
    .expect("write helper eprintln source");
}

fn write_aggregate_helper_eprintln_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create aggregate helper eprintln project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-aggregate-helper-eprintln-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false

[unsafe_rationale]
stdio = "Direct-native stderr regression covers aggregate helper std/io.ax eprintln for issue 1001."
"#,
    )
    .expect("write aggregate helper eprintln manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-aggregate-helper-eprintln-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write aggregate helper eprintln lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/io.ax"
import "std/json.ax"

static STATIC_LINE: string = "aggregate static"

fn helper_line(): string {
return "aggregate helper text"
}

fn emit_pair(): (int, int) {
let first_line: string = "aggregate helper"
let clone_line: string = "aggregate helper"
let concat_line: string = "aggregate helper"
let scores: {string: int} = {"build": 7, "deploy": 9}
let names: [string] = keys<string, int>(scores)
let selected_index: int = 1
let selected_line: string = names[selected_index]
let first: int = eprintln(first_line)
let cloned: int = eprintln(string_clone(clone_line))
let static_written: int = eprintln(STATIC_LINE)
let concat_written: int = eprintln(concat_line + " suffix")
let helper_written: int = eprintln(helper_line())
let value: int = 31
let text: string = stringify_int(value)
let int_written: int = eprintln(text)
let bool_written: int = eprintln(stringify_bool(value == 32))
let selected_written: int = eprintln(selected_line)
let written: int = first + cloned + static_written + concat_written + helper_written + int_written + bool_written + selected_written
return (written, 31)
}

fn main(): int {
let result: (int, int) = emit_pair()
return result.0 + result.1
}
"#,
    )
    .expect("write aggregate helper eprintln source");
}

fn write_helper_print_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create helper print project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-helper-print-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false

[unsafe_rationale]
stdio = "Direct-native stdout regression covers scalar helper source print statements for issue 1001."
"#,
    )
    .expect("write helper print manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-helper-print-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write helper print lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/json.ax"

static STATIC_LINE: string = "helper static"

fn helper_line(): string {
return "helper text"
}

fn emit(): int {
let line: string = "helper stdout"
let scores: {string: int} = {"build": 7, "deploy": 9}
let names: [string] = keys<string, int>(scores)
let selected_index: int = 1
let selected_line: string = names[selected_index]
print line
print string_clone(line)
print STATIC_LINE
print line + " suffix"
print helper_line()
let value: int = 21
print value
print stringify_bool(value == 21)
let text: string = stringify_int(value + 1)
print text
print selected_line
return value
}

fn main(): int {
return emit()
}
"#,
    )
    .expect("write helper print source");
}

fn write_aggregate_helper_print_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create aggregate helper print project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-aggregate-helper-print-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false

[unsafe_rationale]
stdio = "Direct-native stdout regression covers aggregate helper source print statements for issue 1001."
"#,
    )
    .expect("write aggregate helper print manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-aggregate-helper-print-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write aggregate helper print lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"static STATIC_LINE: string = "aggregate static"

fn helper_line(): string {
return "aggregate helper text"
}

fn emit_pair(): (int, int) {
let line: string = "aggregate stdout"
let scores: {string: int} = {"build": 7, "deploy": 9}
let names: [string] = keys<string, int>(scores)
let selected_index: int = 1
let selected_line: string = names[selected_index]
print line
print string_clone(line)
print STATIC_LINE
print line + " suffix"
print helper_line()
let value: int = 17
print value
print selected_line
return (value, 31)
}

fn main(): int {
let result: (int, int) = emit_pair()
return result.0 + result.1
}
"#,
    )
    .expect("write aggregate helper print source");
}

fn write_std_log_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create std log project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-std-log"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write std log manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-std-log"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write std log lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/log.ax"

let attrs: string = fields3(field_string("component", "worker"), field_int("attempt", 2), field_bool("ready", true))
print event("info", "started", attrs)

let attrs_for_log: string = fields3(field_string("component", "worker"), field_int("attempt", 2), field_bool("ready", true))
let written: int = info_attrs("started", attrs_for_log)
print written > 0
"#,
    )
    .expect("write std log source");
}

fn write_std_encoding_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create std encoding project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-std-encoding"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write std encoding manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-std-encoding"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write std encoding lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/encoding.ax"

let encoded: string = url_component_encode("hello world/one")
print encoded

let decoded: Option<string> = url_component_decode(encoded)
match decoded {
Some(value) {
print value
}
None {
print "decode failed"
}
}

let bad: Option<string> = url_component_decode("bad%2")
match bad {
Some(value) {
print value
}
None {
print "bad percent"
}
}

print path_segment_encode("reports/April 2026")
print query_pair_encode("q", "agent path/one")
print path_join_segment("/docs", "stage 1/encoding")
"#,
    )
    .expect("write std encoding source");
}

fn write_clock_project(project: &Path, clock: bool, nonzero_sleep: bool) {
    fs::create_dir_all(project.join("src")).expect("create clock project src");
    fs::write(
        project.join("axiom.toml"),
        format!(
            r#"[package]
name = "cranelift-clock"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = {clock}
crypto = false
"#
        ),
    )
    .expect("write clock manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-clock"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write clock lockfile");
    let pause_ms = if nonzero_sleep { 1 } else { 0 };
    fs::write(
        project.join("src/main.ax"),
        format!(
            r#"import "std/time.ax"

let start: Instant = now()
let pause: Duration = duration_ms({pause_ms})
print start.ms > 0
print now_ms() > 0
print sleep(pause) == 0
let elapsed: int = elapsed_ms(start)
print elapsed == elapsed
"#
        ),
    )
    .expect("write clock source");
}

fn write_clock_sleep_zero_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create clock sleep zero project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-clock-sleep-zero-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = true
crypto = false
"#,
    )
    .expect("write clock sleep zero manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-clock-sleep-zero-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write clock sleep zero lockfile");
    fs::write(
        project.join("src/main.ax"),
        "static ZERO_MS: int = 0\n\nfn pause_zero(): int {\nreturn clock_sleep_ms(ZERO_MS)\n}\n\nfn main(): int {\nlet direct: int = clock_sleep_ms(ZERO_MS)\nlet helper: int = pause_zero()\nif direct == 0 && helper == 0 {\nreturn 48\n} else {\nreturn 1\n}\n}\n",
    )
    .expect("write clock sleep zero source");
}

fn write_std_time_sleep_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create std time sleep project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-std-time-sleep-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = true
crypto = false
"#,
    )
    .expect("write std time sleep manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-std-time-sleep-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write std time sleep lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/time.ax"

static ZERO_MS: int = 0
static NEGATIVE_MS: int = -1

fn pause_zero(): int {
return sleep(duration_ms(ZERO_MS))
}

fn pause_negative(): int {
return sleep(duration_ms(NEGATIVE_MS))
}

fn main(): int {
let direct: int = sleep(duration_ms(ZERO_MS))
let helper: int = pause_zero()
let negative: int = pause_negative()
let dynamic_ms: int = 1
let positive: int = sleep(duration_ms(dynamic_ms))
let direct_positive: int = clock_sleep_ms(dynamic_ms)
let capped: int = sleep(duration_ms(1001))
if direct == 0 && helper == 0 && negative == -1 && positive == 0 && direct_positive == 0 && capped == -1 {
return 48
} else {
return 1
}
}
"#,
    )
    .expect("write std time sleep source");
}

fn write_json_serdes_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create json project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-json-serdes"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write json manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-json-serdes"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write json lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/json.ax"

fn doc(): string {
return object3(field_string("name", "axiom"), field_int("count", 3), field_bool("ready", true))
}

print stringify_int(42)
print stringify_bool(false)
print stringify_string("hello")

match parse_int(" 42 ") {
Some(value) {
print value
}
None {
print -1
}
}

match parse_bool("true") {
Some(value) {
print value
}
None {
print false
}
}

print doc()

match parse_field_string(doc(), "name") {
Some(value) {
print value
}
None {
print "missing name"
}
}

match parse_field_int(doc(), "count") {
Some(value) {
print value
}
None {
print -1
}
}

match parse_field_bool(doc(), "ready") {
Some(value) {
print value
}
None {
print false
}
}

let dynamic_count: int = match parse_int("7") { Some(value) => value, None => 1 }
let dynamic_ready: bool = match parse_bool("false") { Some(value) => value, None => true }
let dynamic_count_text_for_value: string = stringify_int(dynamic_count)
print stringify_value(value_int(dynamic_count))
print stringify_value(value_bool(dynamic_ready))
print stringify_value(value_string(dynamic_count_text_for_value))
let dynamic_count_field_for_object: string = field_value("score", value_int(dynamic_count))
let dynamic_ready_field_for_object: string = field_value("ready", value_bool(dynamic_ready))
let dynamic_count_field_for_value: string = field_value("score", value_int(dynamic_count))
let dynamic_ready_field_for_value: string = field_value("ready", value_bool(dynamic_ready))
let dynamic_count_text_for_array: string = stringify_int(dynamic_count)
let dynamic_object_value: JsonValue = value_object2(dynamic_count_field_for_value, dynamic_ready_field_for_value)
let dynamic_array_value: JsonValue = array3(value_int(dynamic_count), value_bool(dynamic_ready), value_string(dynamic_count_text_for_array))
print object2(dynamic_count_field_for_object, dynamic_ready_field_for_object)
print stringify_value(dynamic_object_value)
print stringify_value(dynamic_array_value)
print schema_object3(schema_field_string("name"), schema_field_int("score"), schema_field_bool("ready"))
match parse_field_value(value_object2(field_value("score", value_int(dynamic_count)), field_value("ready", value_bool(dynamic_ready))), "score") {
Some(score_value) {
print stringify_value(score_value)
}
None {
print "missing score"
}
}

match parse_value(doc()) {
Some(value) {
print stringify_value(value)
}
None {
print "invalid"
}
}

match parse_value(doc()) {
Some(value) {
match parse_field_value(value, "name") {
Some(name_value) {
print stringify_value(name_value)
}
None {
print "missing value"
}
}
}
None {
print "invalid"
}
}

match parse_int("nope") {
Some(value) {
print value
}
None {
print "no int"
}
}
"#,
    )
    .expect("write json source");
}

fn write_std_serdes_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create std/serdes project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-std-serdes"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write std/serdes manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-std-serdes"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write std/serdes lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/serdes.ax"
import "std/testing.ax"

let manual: {string: Value} = {"name": Text("axiom"), "count": Int(3), "ready": Bool(true), "items": Array([Text("one"), Int(2)])}
let expected: {string: Value} = {"name": Text("axiom"), "count": Int(3), "ready": Bool(true), "items": Array([Text("one"), Int(2)])}
print to_json(manual)

match from_json_str("{\"count\":3,\"items\":[\"one\",2],\"name\":\"axiom\",\"ready\":true}") {
Ok(value) {
print assert_true(value == Object(expected))
}
Err(error) {
print parse_error_message(error)
}
}

match from_json_str("{\"name\":\"axiom\",\"count\":3,\"ready\":true,\"items\":[\"one\",2],\"nested\":{\"ok\":false}}") {
Ok(value) {
print stringify(value)
}
Err(error) {
print parse_error_message(error)
}
}

match from_json_str("{\"name\":\"axiom\",\"count\":3,\"ready\":true,\"items\":[\"one\",2],\"nested\":{\"ok\":false}}") {
Ok(value) {
match text_field(value, "name") {
Some(name) {
print name
}
None {
print "missing name"
}
}
}
Err(error) {
print parse_error_message(error)
}
}

match from_json_str("{\"name\":\"axiom\",\"count\":3,\"ready\":true,\"items\":[\"one\",2],\"nested\":{\"ok\":false}}") {
Ok(value) {
match int_field(value, "count") {
Some(count) {
print count
}
None {
print -1
}
}
}
Err(error) {
print parse_error_message(error)
}
}

match from_json_str("{\"name\":\"axiom\",\"count\":3,\"ready\":true,\"items\":[\"one\",2],\"nested\":{\"ok\":false}}") {
Ok(value) {
match object_field(value, "nested") {
Some(nested) {
match bool_field(Object(nested), "ok") {
Some(ok) {
print ok
}
None {
print true
}
}
}
None {
print true
}
}
}
Err(error) {
print parse_error_message(error)
}
}

match from_json_str("{\"name\":\"axiom\",\"count\":3,\"ready\":true,\"items\":[\"one\",2],\"nested\":{\"ok\":false}}") {
Ok(value) {
match array_field(value, "items") {
Some(items) {
match value_item(Array(items), 0) {
Some(item) {
match as_text(item) {
Some(text) {
print text
}
None {
print "missing text"
}
}
}
None {
print "missing item"
}
}
}
None {
print "missing array"
}
}
}
Err(error) {
print parse_error_message(error)
}
}

match from_json_str("{\"name\":\"axiom\",\"count\":3,\"ready\":true,\"items\":[\"one\",2],\"nested\":{\"ok\":false}}") {
Ok(value) {
match array_field(value, "items") {
Some(items) {
match value_item(Array(items), 1) {
Some(item) {
match as_int(item) {
Some(count) {
print count
}
None {
print -1
}
}
}
None {
print -1
}
}
}
None {
print -1
}
}
}
Err(error) {
print parse_error_message(error)
}
}

match from_json_str("null") {
Ok(value) {
print is_null(value)
}
Err(error) {
print parse_error_message(error)
}
}

match from_json_str("false") {
Ok(value) {
match as_bool(value) {
Some(flag) {
print flag
}
None {
print true
}
}
}
Err(error) {
print parse_error_message(error)
}
}

match from_json_str("[\"one\",2]") {
Ok(value) {
match as_array(value) {
Some(items) {
match value_item(Array(items), 1) {
Some(item) {
match as_int(item) {
Some(count) {
print count
}
None {
print -1
}
}
}
None {
print -1
}
}
}
None {
print -1
}
}
}
Err(error) {
print parse_error_message(error)
}
}

match from_json_str("{\"name\":\"axiom\",\"count\":3}") {
Ok(value) {
match as_object(value) {
Some(fields) {
print to_json(fields)
}
None {
print "missing object"
}
}
}
Err(error) {
print parse_error_message(error)
}
}

match from_json("{") {
Ok(value) {
print stringify(value)
}
Err(_error) {
print "parse error"
}
}
"#,
    )
    .expect("write std/serdes source");
}

fn write_std_serdes_known_json_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create std/serdes known JSON project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-std-serdes-known-json-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write std/serdes known JSON manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-std-serdes-known-json-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write std/serdes known JSON lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/serdes.ax"

fn object_json(): string {
return to_json({"name": Text("axiom"), "count": Int(3), "ready": Bool(true)})
}

fn stringified_text(): string {
return stringify(Text("direct-native"))
}

fn parsed_value_json(): string {
match from_json_str("{\"name\":\"axiom\",\"count\":3}") {
Ok(value) {
return stringify(value)
}
Err(error) {
return parse_error_message(error)
}
}
}

fn parsed_text(): string {
match from_json_str("\"direct-native\"") {
Ok(value) {
match as_text(value) {
Some(text) {
return text
}
None {
return "not text"
}
}
}
Err(error) {
return parse_error_message(error)
}
}
}

fn parse_error_text(): string {
match from_json_str("{") {
Ok(value) {
return stringify(value)
}
Err(error) {
return parse_error_message(error)
}
}
}

fn main(): int {
let object_text: string = object_json()
let text_json: string = stringified_text()
let parsed_json: string = parsed_value_json()
let text_value: string = parsed_text()
let error_text: string = parse_error_text()
let object_gate: bool = object_text == "{\"count\":3,\"name\":\"axiom\",\"ready\":true}"
let text_gate: bool = text_json == "\"direct-native\""
let parsed_gate: bool = parsed_json == "{\"count\":3,\"name\":\"axiom\"}"
let value_gate: bool = text_value == "direct-native"
let error_gate: bool = len(error_text) > 0
if object_gate && text_gate && parsed_gate && value_gate && error_gate {
return 48
} else {
return 1
}
}
"#,
    )
    .expect("write std/serdes known JSON source");
}

fn write_std_cli_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create std/cli project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-std-cli"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write std/cli manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-std-cli"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write std/cli lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/cli.ax"

print arg_count()
let values: [string] = args()
print len(values)

let first_arg: Option<string> = arg(0)
match first_arg {
Some(value) {
print value
}
None {
print "missing"
}
}
"#,
    )
    .expect("write std/cli source");
}

fn write_fs_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create fs denied project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-fs-denied\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write fs denied manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-fs-denied\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write fs denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/fs.ax\"\nmatch read_file(\"src/fixture.txt\") {\nSome(value) {\nprint value\n}\nNone {\nprint \"missing\"\n}\n}\n",
    )
    .expect("write fs denied source");
}

fn write_fs_read_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create fs-read main project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-fs-read-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = true
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write fs-read main manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-fs-read-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write fs-read main lockfile");
    fs::write(project.join("src/fixture.txt"), "native-fs\n").expect("write fs-read fixture");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/fs.ax"

fn main(): int {
let direct_len: int = match fs_read("src/fixture.txt") { Some(value) => len(value), None => 1 }
let wrapper_len: int = match read_file("src/fixture.txt") { Some(value) => len(value), None => 1 }
let missing_len: int = match read_file("src/missing.txt") { Some(value) => len(value), None => 28 }
let stored_direct: Option<string> = fs_read("src/fixture.txt")
let stored_wrapper: Option<string> = read_file("src/fixture.txt")
let stored_missing: Option<string> = read_file("src/missing.txt")
let stored_statement: Option<string> = read_file("src/fixture.txt")
let stored_direct_len: int = match stored_direct { Some(value) => len(value), None => 1 }
let stored_wrapper_len: int = match stored_wrapper { Some(value) => len(value), None => 1 }
let stored_missing_len: int = match stored_missing { Some(value) => len(value), None => 28 }
let statement_len: int = 0
match stored_statement {
Some(value) {
statement_len = len(value)
}
None {
statement_len = 1
}
}
if direct_len == 13 && wrapper_len == 13 && missing_len == 28 && stored_direct_len == 13 && stored_wrapper_len == 13 && stored_missing_len == 28 && statement_len == 13 {
return statement_len + 35
} else {
return 1
}
}
"#,
    )
    .expect("write fs-read main source");
}

fn write_fs_read_symlink_escape_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create fs-read symlink project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-fs-read-symlink-escape"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = true
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write fs-read symlink manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-fs-read-symlink-escape"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write fs-read symlink lockfile");
    fs::write(project.join("src/fixture.txt"), "native-fs\n")
        .expect("write fs-read symlink fixture");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/fs.ax"

fn main(): int {
let status: int = match read_file("src/fixture.txt") { Some(_value) => 1, None => 37 }
return status
}
"#,
    )
    .expect("write fs-read symlink source");
}

fn write_fs_write_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create fs-write main project src");
    fs::create_dir_all(project.join("scratch")).expect("create fs-write main scratch dir");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-fs-write-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = true
"fs:write" = true
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write fs-write main manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-fs-write-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write fs-write main lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/fs.ax"

fn main(): int {
let wrote: int = write_file("scratch/data.txt", "runtime-write")
let appended: int = append_file("scratch/data.txt", "+runtime-append")
let replaced: int = replace_file("scratch/data.txt", "runtime-replace")
let removed: int = remove_file("scratch/data.txt")
let created: int = create_file("scratch/created.txt")
let made_dir: int = mkdir("scratch/native-dir")
let removed_dir: int = remove_dir("scratch/native-dir")
let made_all: int = mkdir_all("scratch/native-all/deep")
let blocked: int = write_file("../escape.txt", "blocked")
if wrote == 0 && appended == 0 && replaced == 0 && removed == 0 && created == 0 && made_dir == 0 && removed_dir == 0 && made_all == 0 && blocked == -1 {
return 48
} else {
return 1
}
}
"#,
    )
    .expect("write fs-write main source");
}

fn write_fs_write_symlink_escape_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create fs-write symlink project src");
    fs::create_dir_all(project.join("scratch")).expect("create fs-write symlink scratch dir");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-fs-write-symlink-escape"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = true
"fs:write" = true
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write fs-write symlink manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-fs-write-symlink-escape"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write fs-write symlink lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/fs.ax"

fn main(): int {
let wrote: int = write_file("scratch/link.txt", "outside-overwrite")
if wrote == -1 {
return 37
} else {
return 1
}
}
"#,
    )
    .expect("write fs-write symlink source");
}

fn write_tcp_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create tcp denied project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-tcp-denied\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write tcp denied manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-tcp-denied\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write tcp denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/net.ax\"\nmatch tcp_listen_loopback_once(\"pong\", 1000) {\nSome(_port) {\nprint true\n}\nNone {\nprint false\n}\n}\n",
    )
    .expect("write tcp denied source");
}

fn write_dynamic_net_targets_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create dynamic net targets project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-dynamic-net-targets"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = { hosts = ["localhost"], ports = [8080] }
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write dynamic net targets manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-dynamic-net-targets"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write dynamic net targets lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/net.ax"

let host: string = "localhost"
let port: int = 8080

print net_resolve(host)
print tcp_dial(host, port, "ping", 1000)
"#,
    )
    .expect("write dynamic net targets source");
}

fn write_udp_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create udp denied project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-udp-denied\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write udp denied manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-udp-denied\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write udp denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/net.ax\"\nmatch udp_bind_loopback_once(\"pong\", 1000) {\nSome(_port) {\nprint true\n}\nNone {\nprint false\n}\n}\n",
    )
    .expect("write udp denied source");
}

fn write_process_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create process denied project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-process-denied\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write process denied manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-process-denied\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write process denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/process.ax\"\nprint run_status(\"/usr/bin/true\")\n",
    )
    .expect("write process denied source");
}

fn write_fs_write_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create fs write project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-fs-write\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = true\n\"fs:write\" = true\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write fs write manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-fs-write\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write fs write lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/fs.ax\"\nprint mkdir_all(\"scratch/nested\")\nprint write_file(\"scratch/nested/data.txt\", \"one\")\nprint append_file(\"scratch/nested/data.txt\", \"\\ntwo\")\nmatch read_file(\"scratch/nested/data.txt\") {\nSome(value) {\nprint value\n}\nNone {\nprint \"missing\"\n}\n}\nprint replace_file(\"scratch/nested/data.txt\", \"final\")\nmatch read_file(\"scratch/nested/data.txt\") {\nSome(value) {\nprint value\n}\nNone {\nprint \"missing\"\n}\n}\nprint create_file(\"scratch/empty.txt\")\nprint remove_file(\"scratch/empty.txt\")\nprint remove_file(\"scratch/nested/data.txt\")\nprint remove_dir(\"scratch/nested\")\nprint remove_dir(\"scratch\")\n",
    )
    .expect("write fs write source");
}

fn write_fs_root_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create fs-root project src");
    fs::create_dir_all(project.join("sandbox")).expect("create fs-root sandbox");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-fs-root\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = true\n\"fs:write\" = true\nfs_root = \"sandbox\"\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write fs-root manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-fs-root\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write fs-root lockfile");
    fs::write(project.join("src/main.ax"), fs_root_source(project)).expect("write fs-root source");
}

fn fs_root_source(project: &Path) -> String {
    let source = project.join("src/main.ax").display().to_string();
    let manifest = project.join("axiom.toml").display().to_string();
    format!(
        "import \"std/fs.ax\"\nprint mkdir_all(\"nested\")\nprint write_file(\"nested/data.txt\", \"ok\")\nprint write_file({source:?}, \"corrupt\")\nmatch read_file({manifest:?}) {{\nSome(_value) {{\nprint \"leak\"\n}}\nNone {{\nprint \"missing\"\n}}\n}}\nmatch read_file(\"nested/data.txt\") {{\nSome(value) {{\nprint value\n}}\nNone {{\nprint \"missing allowed\"\n}}\n}}\n"
    )
}

fn write_fs_write_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create fs write denied project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-fs-write-denied\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = true\n\"fs:write\" = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write fs write denied manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-fs-write-denied\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write fs write denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/fs.ax\"\nprint write_file(\"out.txt\", \"content\")\n",
    )
    .expect("write fs write denied source");
}

fn write_env_read_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create env project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-env-read\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = true\nclock = false\ncrypto = false\n\n[unsafe_rationale]\nenv = \"Cranelift ABI regression covers direct-native env.read behavior for issue 928.\"\n",
    )
    .expect("write env manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-env-read\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write env lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/env.ax\"\nmatch get_env(\"AXIOM_CRANELIFT_ENV_READ\") {\nSome(value) {\nprint value\n}\nNone {\nprint \"missing value\"\n}\n}\nmatch get_env(\"__AXIOM_CRANELIFT_ENV_MISSING__\") {\nSome(value) {\nprint value\n}\nNone {\nprint \"missing\"\n}\n}\n",
    )
    .expect("write env source");
}

fn write_env_read_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create env main project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-env-read-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = true
clock = false
crypto = false

[unsafe_rationale]
env = "direct-native env-read regression captures deterministic test environment values"
"#,
    )
    .expect("write env main manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-env-read-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write env main lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/env.ax"

static PRESENT_ENV: string = "AXIOM_CRANELIFT_ENV_READ"
static MISSING_ENV: string = "__AXIOM_CRANELIFT_ENV_MISSING__"

fn main(): int {
let present: int = match env_get(PRESENT_ENV) { Some(value) => len(value), None => 0 }
let missing: int = match get_env(MISSING_ENV) { Some(value) => len(value), None => 38 }
let stored_present: Option<string> = get_env(PRESENT_ENV)
let stored_missing: Option<string> = env_get(MISSING_ENV)
let stored_present_for_statement: Option<string> = get_env(PRESENT_ENV)
let stored_present_len: int = match stored_present { Some(value) => len(value), None => 0 }
let stored_missing_len: int = match stored_missing { Some(value) => len(value), None => 38 }
let statement_present_len: int = 0
match stored_present_for_statement {
Some(value) {
statement_present_len = len(value)
}
None {
statement_present_len = 1
}
}
if present == 11 && missing == 38 && stored_present_len == 11 && stored_missing_len == 38 && statement_present_len == 11 {
return statement_present_len + 37
} else {
return 1
}
}
"#,
    )
    .expect("write env main source");
}

fn write_env_allowlist_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create env allowlist project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-env-allowlist-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = ["AXIOM_CRANELIFT_ENV_READ"]
clock = false
crypto = false
"#,
    )
    .expect("write env allowlist manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-env-allowlist-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write env allowlist lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/env.ax"

static ALLOWED_ENV: string = "AXIOM_CRANELIFT_ENV_READ"
static BLOCKED_ENV: string = "AXIOM_CRANELIFT_ENV_BLOCKED"

fn main(): int {
let allowed: int = match env_get(ALLOWED_ENV) { Some(value) => len(value), None => 0 }
let blocked: int = match get_env(BLOCKED_ENV) { Some(value) => len(value), None => 0 }
let stored_allowed: Option<string> = get_env(ALLOWED_ENV)
let stored_blocked: Option<string> = env_get(BLOCKED_ENV)
let stored_allowed_len: int = match stored_allowed { Some(value) => len(value), None => 0 }
let stored_blocked_len: int = match stored_blocked { Some(value) => len(value), None => 0 }
if allowed == 11 && blocked == 0 && stored_allowed_len == 11 && stored_blocked_len == 0 {
return 48
} else {
return 1
}
}
"#,
    )
    .expect("write env allowlist source");
}

fn write_http_client_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create http client denied project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-http-client-denied"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write http client denied manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-http-client-denied"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write http client denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/http.ax"
print get("https://example.com")
"#,
    )
    .expect("write http client denied source");
}

fn write_ffi_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create ffi denied project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-ffi-denied"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
ffi = false
"#,
    )
    .expect("write ffi denied manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-ffi-denied"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write ffi denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"extern fn strlen(value: string): int from "c"
print strlen("hello")
"#,
    )
    .expect("write ffi denied source");
}

fn write_ffi_strlen_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create ffi strlen project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-ffi-strlen"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
ffi = true

[unsafe_rationale]
ffi = "Cranelift ABI regression covers the narrow C strlen extern call for issue 928."
"#,
    )
    .expect("write ffi strlen manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-ffi-strlen"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write ffi strlen lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"extern fn strlen(value: string): int from "c"
print strlen("hello")
print strlen("")
"#,
    )
    .expect("write ffi strlen source");
}

fn write_ffi_strlen_main_exit_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create ffi strlen main project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-ffi-strlen-main-exit"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
ffi = true

[unsafe_rationale]
ffi = "Cranelift ABI regression covers the narrow C strlen extern call for issue 928."
"#,
    )
    .expect("write ffi strlen main manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-ffi-strlen-main-exit"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write ffi strlen main lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"extern fn strlen(value: string): int from "c"

fn literal_probe(): int {
return strlen("helper")
}

fn local_probe(): int {
let text: string = "helper-local"
return strlen(text)
}

fn selected_probe(): int {
let scores: {string: int} = {"build": 7, "deploy": 9}
let names: [string] = keys<string, int>(scores)
let selected_index: int = 0
return strlen(names[selected_index])
}

fn main(): int {
let literal_len: int = strlen("hello")
let empty_len: int = strlen("")
let text: string = "direct-native"
let local_len: int = strlen(text)
let scores: {string: int} = {"build": 7, "deploy": 9}
let names: [string] = keys<string, int>(scores)
let selected_index: int = 1
let selected_len: int = strlen(names[selected_index])
let helper_literal_len: int = literal_probe()
let helper_local_len: int = local_probe()
let helper_selected_len: int = selected_probe()
if literal_len == 5 && empty_len == 0 && local_len == 13 && selected_len == 6 && helper_literal_len == 6 && helper_local_len == 12 && helper_selected_len == 5 {
return 48
} else {
return 1
}
}
"#,
    )
    .expect("write ffi strlen main source");
}

fn write_async_runtime_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create async runtime denied project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-async-runtime-denied"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
async = false
"#,
    )
    .expect("write async runtime denied manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-async-runtime-denied"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write async runtime denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/async.ax"
let task: Task<int> = ready<int>(1)
print await task
"#,
    )
    .expect("write async runtime denied source");
}

fn write_http_server_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create http server denied project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-http-server-denied"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write http server denied manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-http-server-denied"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write http server denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/http.ax"
print serve_once("127.0.0.1:0", "ok")
"#,
    )
    .expect("write http server denied source");
}

fn write_http_async_server_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create http async server denied project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-http-async-server-denied"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = true
process = false
env = false
clock = false
crypto = false
async = false

[unsafe_rationale]
net = "Cranelift ABI regression covers async server capability denial ordering for issue 928."
"#,
    )
    .expect("write http async server denied manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-http-async-server-denied"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write http async server denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/http_async.ax"
let task: Task<bool> = async_serve_route(1, "/", "ok", 1)
print true
"#,
    )
    .expect("write http async server denied source");
}

fn write_env_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create env denied project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-env-denied\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write env denied manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-env-denied\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write env denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/env.ax\"\nmatch get_env(\"AXIOM_CRANELIFT_ENV_READ\") {\nSome(value) {\nprint value\n}\nNone {\nprint \"missing\"\n}\n}\n",
    )
    .expect("write env denied source");
}
