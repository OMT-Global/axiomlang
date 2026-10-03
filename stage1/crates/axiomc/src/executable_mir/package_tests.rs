use crate::project::{check_project, executable_mir_packages};
use std::fs;
use std::path::Path;

#[test]
fn executable_mir_inspection_rejects_unused_unsupported_body() {
    assert_unused_helper_rejected("fn unused(): int {\nprint 1\nreturn 2\n}\n");
}

#[test]
fn executable_mir_inspection_rejects_unused_unsupported_signature() {
    assert_unused_helper_rejected("fn unused(): string {\nreturn \"unused\"\n}\n");
}

#[test]
fn executable_mir_inspection_rejects_unused_async_helper() {
    assert_unused_helper_rejected("async fn unused(): int {\nreturn 2\n}\n");
}

#[test]
fn executable_mir_inspection_rejects_unused_property_helper() {
    assert_unused_helper_rejected(
        "property fn unused(value: int): bool {\nreturn value == value\n}\n",
    );
}

#[test]
fn executable_mir_inspection_rejects_unused_external_helper() {
    assert_unused_helper_rejected("extern fn unused(): int from \"c\"\n");
}

#[test]
fn executable_mir_inspection_retains_supported_call_edges() {
    let directory = tempfile::tempdir().expect("inspection project");
    write_project(
        directory.path(),
        "fn helper(value: int): int {\nreturn value + 1\n}\n",
    );
    fs::write(
        directory.path().join("src/main.ax"),
        "fn helper(value: int): int {\nreturn value + 1\n}\n\nfn main(): int {\nreturn helper(41)\n}\n",
    )
    .expect("source with scalar call");
    check_project(directory.path()).expect("valid scalar call source");
    let packages = executable_mir_packages(directory.path()).expect("supported call program");
    let program = &packages[0].program;
    assert_eq!(program.functions.len(), 2);
    let helper = &program.functions[1];
    assert!(program.functions[0].blocks.iter().any(|block| {
        block.instructions.iter().any(|instruction| {
            matches!(instruction, crate::executable_mir::Instruction::Call { function, .. } if function == &helper.name)
        })
    }));
}

#[test]
fn executable_mir_inspection_retains_unused_supported_helper() {
    let directory = tempfile::tempdir().expect("inspection project");
    write_project(directory.path(), "fn unused(): int {\nreturn 2\n}\n");
    check_project(directory.path()).expect("valid source");
    let packages = executable_mir_packages(directory.path()).expect("supported whole program");
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].program.functions.len(), 2);
}

fn assert_unused_helper_rejected(helper: &str) {
    let directory = tempfile::tempdir().expect("inspection project");
    write_project(directory.path(), helper);
    check_project(directory.path()).expect("valid source must reach the MIR boundary");
    let error = executable_mir_packages(directory.path())
        .expect_err("unsupported unused user helper must not yield a partial program");
    assert_eq!(error.code.as_deref(), Some("executable_mir.unsupported"));
}

fn write_project(root: &Path, helper: &str) {
    fs::create_dir_all(root.join("src")).expect("source directory");
    let mut manifest = "[package]\nname = \"whole-program-inspection\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n".to_string();
    if helper.starts_with("async fn") {
        manifest.push_str("async = true\n");
    }
    crate::manifest::parse_manifest_exact(manifest.as_bytes(), &root.join("axiom.toml"))
        .expect("valid fixture manifest");
    fs::write(root.join("axiom.toml"), manifest).expect("manifest");
    fs::write(
        root.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"whole-program-inspection\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("lockfile");
    fs::write(
        root.join("src/main.ax"),
        format!("{helper}\nfn main(): int {{\nreturn 0\n}}\n"),
    )
    .expect("source");
}
