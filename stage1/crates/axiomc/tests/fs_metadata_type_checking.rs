//! Issue #1582: `fs_file_exists` / `fs_file_size`, the intrinsics behind
//! `std/fs.ax`'s `file_exists` and `file_size`, must type-check wherever they
//! are reached, not only from code under `fn main` that never lowers the
//! wrapper bodies.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const FS_CAPABILITIES: &str = "fs = true\n\"fs:write\" = true\n";

fn axiomc(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(args)
        .output()
        .expect("run axiomc")
}

fn package(dir: &Path, capabilities: &str, source: &str) -> PathBuf {
    let root = dir.join("fs-metadata");
    let created = axiomc(&["new", root.to_str().expect("UTF-8 path")]);
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let manifest_path = root.join("axiom.toml");
    let manifest = fs::read_to_string(&manifest_path).expect("read manifest");
    let start = manifest.find("[capabilities]").expect("capabilities table");
    let manifest = format!("{}[capabilities]\n{capabilities}", &manifest[..start]);
    fs::write(&manifest_path, manifest).expect("write manifest");
    fs::write(root.join("src/main.ax"), source).expect("write source");
    root
}

fn command(subcommand: &str, capabilities: &str, source: &str) -> Output {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = package(dir.path(), capabilities, source);
    axiomc(&[subcommand, root.to_str().expect("UTF-8 path")])
}

#[test]
fn metadata_helpers_type_check_from_top_level_statements() {
    let output = command(
        "check",
        FS_CAPABILITIES,
        "import \"std/fs.ax\"\nprint file_exists(\"axiom.toml\")\nprint file_size(\"axiom.toml\")\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn metadata_helpers_run_when_called_through_a_function_from_top_level() {
    let output = command(
        "run",
        FS_CAPABILITIES,
        concat!(
            "import \"std/fs.ax\"\n",
            "fn manifest_found(): bool {\n",
            "    return file_exists(\"axiom.toml\")\n",
            "}\n",
            "fn missing_size(): int {\n",
            "    return file_size(\"missing.txt\")\n",
            "}\n",
            "print manifest_found()\n",
            "print missing_size()\n",
        ),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "true\n-1\n");
}

#[test]
fn metadata_intrinsics_type_check_and_run_when_called_directly() {
    let output = command(
        "run",
        FS_CAPABILITIES,
        concat!(
            "fn main(): int {\n",
            "    print fs_file_exists(\"axiom.toml\")\n",
            "    print fs_file_exists(\"src\")\n",
            "    print fs_file_size(\"missing.txt\")\n",
            "    return 0\n",
            "}\n",
        ),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "true\nfalse\n-1\n");
}

#[test]
fn metadata_intrinsics_require_the_fs_capability_and_one_string_path() {
    let denied = command(
        "check",
        "fs = false\n",
        "fn main(): int {\n    print fs_file_exists(\"axiom.toml\")\n    return 0\n}\n",
    );
    let stderr = String::from_utf8_lossy(&denied.stderr);
    assert!(!denied.status.success(), "{stderr}");
    assert!(
        stderr.contains("requires [capabilities].fs = true"),
        "{stderr}"
    );

    for (source, expected) in [
        (
            "print fs_file_size(3)\n",
            "fs_file_size expects a string argument, got int",
        ),
        (
            "print fs_file_exists(\"a\", \"b\")\n",
            "fs_file_exists expects 1 argument, got 2",
        ),
    ] {
        let output = command("check", FS_CAPABILITIES, source);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{source:?}: {stderr}");
        assert!(stderr.contains(expected), "{source:?}: {stderr}");
    }
}
