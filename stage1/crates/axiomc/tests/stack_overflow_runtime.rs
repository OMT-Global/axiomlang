//! Issue #1784: unbounded recursion in a direct-native binary reports a
//! structured runtime error instead of dying with a raw SIGSEGV, while any
//! other SIGSEGV still terminates the process by that signal.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use std::fs;
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

const STACK_OVERFLOW_LINE: &str = "{\"kind\":\"runtime\",\"message\":\"stack overflow\"}";

/// Native builds need a host linker. Without `cc` the tests skip, except under
/// `run-native-tests`, where PR CI requires them to execute.
fn native_toolchain_available() -> bool {
    if which::which("cc").is_ok() {
        return true;
    }
    assert!(
        !cfg!(feature = "run-native-tests"),
        "run-native-tests requires cc; the stack overflow tests must not pass without executing"
    );
    eprintln!("skipping direct-native stack overflow test because cc is unavailable");
    false
}

fn axiomc(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(args)
        .output()
        .expect("run axiomc")
}

/// Creates and builds a package named `overflow`, returning its binary.
fn build(dir: &Path, source: &str) -> PathBuf {
    let root = dir.join("overflow");
    let root_str = root.to_str().expect("UTF-8 path");
    let created = axiomc(&["new", root_str]);
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    fs::write(root.join("src/main.ax"), source).expect("write source");
    let built = axiomc(&["build", root_str]);
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    root.join("dist/overflow")
}

const UNBOUNDED_RECURSION: &str = "fn r(n: int): int {\n  return r(n + 1)\n}\nprint r(0)\n";

#[test]
fn unbounded_recursion_reports_a_structured_stack_overflow() {
    if !native_toolchain_available() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let binary = build(dir.path(), UNBOUNDED_RECURSION);
    let output = Command::new(&binary).output().expect("run binary");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "status {:?}: {stderr}",
        output.status
    );
    assert_eq!(stderr.trim_end(), STACK_OVERFLOW_LINE);
}

#[test]
fn axiomc_run_reports_stack_overflow_as_a_runtime_error_not_a_signal() {
    if !native_toolchain_available() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    build(dir.path(), UNBOUNDED_RECURSION);
    let root = dir.path().join("overflow");
    let output = axiomc(&["run", root.to_str().expect("UTF-8 path")]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains(STACK_OVERFLOW_LINE), "{stderr}");
    assert!(!stderr.contains("was terminated by"), "{stderr}");
}

#[test]
fn a_sigsegv_that_is_not_a_stack_fault_still_terminates_by_signal() {
    if !native_toolchain_available() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    // Blocks reading stdin, so the signal arrives while the program runs.
    let binary = build(
        dir.path(),
        "import \"std/io.ax\"\nfn main(): int {\nreturn len(read_to_string())\n}\n",
    );
    let mut child = Command::new(&binary)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn binary");
    thread::sleep(Duration::from_millis(300));
    let pid = i32::try_from(child.id()).expect("pid fits i32");
    assert_eq!(unsafe { libc::kill(pid, libc::SIGSEGV) }, 0);
    let output = child.wait_with_output().expect("wait binary");
    assert_eq!(
        output.status.signal(),
        Some(libc::SIGSEGV),
        "{:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("stack overflow"), "{stderr}");
}

#[test]
fn programs_that_do_not_overflow_are_unaffected() {
    if !native_toolchain_available() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let binary = build(
        dir.path(),
        "import \"std/io.ax\"\nfn main(): int {\nreturn len(read_to_string())\n}\n",
    );
    let mut child = Command::new(&binary)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn binary");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"hello\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait binary");
    assert_eq!(output.status.code(), Some(6), "{:?}", output.status);
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
