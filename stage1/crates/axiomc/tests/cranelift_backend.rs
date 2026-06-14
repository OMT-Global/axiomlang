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
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift process-status binary");
    assert!(
        run.status.success(),
        "cranelift process-status binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "0\n1\n");
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
    assert!(
        manifest["native_debug"]["native_debug_info"]
            .as_str()
            .expect("native debug info")
            .contains("does not emit native Axiom DWARF yet")
    );
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

#[test]
fn cranelift_backend_rejects_nonzero_clock_sleep() {
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
        !output.status.success(),
        "cranelift nonzero clock sleep build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("nonzero clock_sleep_ms is not supported by the cranelift spike"),
        "expected nonzero sleep guard, got: {combined}"
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
{"count":3,"items":["one",2],"name":"axiom","nested":{"ok":false},"ready":true}
axiom
3
false
one
parse error
"#,
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
env = false
clock = false
crypto = false
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
        "[package]\nname = \"cranelift-i64-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
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
        "[package]\nname = \"cranelift-i64-returning-main-exit\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
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
        format!("fn main(): int {{\nlet ready_for_match: Option<[int; 2]> = {value}\nlet ready_for_statement: Option<[int; 2]> = {value}\nlet match_code: int = match ready_for_match {{ Some(values) => values[0] + values[1], None => 49 }}\nlet statement_code: int = 0\nmatch ready_for_statement {{\nSome(values) {{\nstatement_code = values[0] + values[1]\n}}\nNone {{\nstatement_code = 49\n}}\n}}\nif match_code == statement_code {{\nreturn match_code\n}} else {{\nreturn 1\n}}\n}}\n"),
    )
    .expect("write option array payload match main exit source");
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
"#,
    )
    .expect("write process-status source");
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
env = false
clock = false
crypto = false
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

fn start_http_fixture_server(body: &'static str) -> (u16, std::thread::JoinHandle<()>) {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind http fixture");
    listener
        .set_nonblocking(true)
        .expect("set http fixture nonblocking");
    let port = listener.local_addr().expect("http fixture addr").port();
    let handle = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
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
                    break;
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
print await served
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

let manual: {string: Value} = {"name": Text("axiom"), "count": Int(3), "ready": Bool(true), "items": Array([Text("one"), Int(2)])}
print to_json(manual)

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
