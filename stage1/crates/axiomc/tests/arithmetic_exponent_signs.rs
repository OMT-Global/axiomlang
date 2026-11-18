//! Issue #1760: `-` after an identifier ending in `e`/`E` is subtraction, and
//! both exponent signs of a float literal stay part of the literal.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

fn run(source: &str) -> Output {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = package(dir.path(), "exponent-signs", source);
    axiomc(&["run", root.to_str().expect("UTF-8 path")])
}

#[test]
fn subtraction_after_identifiers_ending_in_e_with_any_spacing() {
    let output = run(concat!(
        "let value: int = 3\n",
        "let line: int = 10\n",
        "let e: int = 4\n",
        "let E: int = 4\n",
        "let size: int = 9\n",
        "print value-1\n",
        "print value - 1\n",
        "print line-2\n",
        "print e-1\n",
        "print E - 1\n",
        "print (size-1) * 2\n",
        "print value-1-1\n",
    ));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "2\n2\n8\n3\n3\n16\n1\n"
    );
}

#[test]
fn both_exponent_signs_stay_in_float_literals_inside_nested_arithmetic() {
    let output = run(concat!(
        "let small: f32 = 1e+2f32\n",
        "let tiny: f64 = 1.5e-3f64\n",
        "let mixed: f64 = (1e+2f64 - 2.5e-1f64) * 2.0f64\n",
        "print small\n",
        "print tiny\n",
        "print mixed\n",
    ));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "100\n0.0015\n199.5\n"
    );
}

#[test]
fn malformed_exponents_are_still_rejected() {
    for source in [
        "let x: f64 = 1e+f64\nprint x\n",
        "let x: f64 = 1e - 2f64\nprint x\n",
        "let x: f64 = 1e-\nprint x\n",
    ] {
        let output = run(source);
        assert!(!output.status.success(), "{source:?} must be rejected");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("src/main.ax:1:"), "{source:?}: {stderr}");
    }
}
