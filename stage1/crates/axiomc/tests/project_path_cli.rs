//! Relative project paths must select the directory the operating system
//! would select (issue #1779).

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn axiomc(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run axiomc")
}

fn new_package(parent: &Path, name: &str, source: &str) {
    let output = axiomc(parent, &["new", name]);
    assert!(
        output.status.success(),
        "axiomc new {name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::write(parent.join(name).join("src/main.ax"), source).expect("write package source");
}

fn assert_checks(cwd: &Path, project: &str) {
    let output = axiomc(cwd, &["check", project]);
    assert!(
        output.status.success(),
        "axiomc check {project} from {}: {}{}",
        cwd.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

const VALID: &str = "print \"selected\"\n";
// A decoy that fails `check`, so selecting it is observable without a native build.
const DECOY: &str = "let broken: int = \"not an int\"\n";

#[test]
fn current_directory_spellings_select_the_enclosing_package() {
    let dir = tempfile::tempdir().expect("tempdir");
    new_package(dir.path(), "app", VALID);
    let app = dir.path().join("app");

    assert_checks(&app, ".");
    assert_checks(&app, "./");
    assert_checks(&app, "../app");
    assert_checks(&app.join("src"), "..");
}

#[test]
fn leading_parent_component_is_not_dropped_when_a_same_named_decoy_exists() {
    let dir = tempfile::tempdir().expect("tempdir");
    new_package(dir.path(), "app", VALID);
    fs::create_dir(dir.path().join("sub")).expect("create sub");
    new_package(&dir.path().join("sub"), "app", DECOY);
    let sub = dir.path().join("sub");

    // The decoy really does fail, so a pass below proves `../app` was honoured.
    let decoy = axiomc(&sub, &["check", "app"]);
    assert!(!decoy.status.success(), "decoy package must fail check");

    assert_checks(&sub, "../app");
    assert_checks(&sub, "./../app");
    assert_checks(&sub.join("app"), "../../app");
}

#[cfg(unix)]
#[test]
fn parent_component_after_a_symlink_follows_the_link_target() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::create_dir_all(dir.path().join("real")).expect("create real");
    new_package(&dir.path().join("real"), "linked", VALID);
    new_package(&dir.path().join("real"), "sibling", VALID);
    // Lexical resolution of `alias/../sibling` would select this decoy.
    new_package(dir.path(), "sibling", DECOY);
    std::os::unix::fs::symlink(dir.path().join("real/linked"), dir.path().join("alias"))
        .expect("create symlink");

    assert_checks(dir.path(), "alias/../sibling");
}

#[test]
fn missing_relative_project_reports_the_path_the_caller_wrote() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = axiomc(dir.path(), &["check", "../definitely-missing-project"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("../definitely-missing-project"),
        "diagnostic should name the requested path: {stderr}"
    );
}
