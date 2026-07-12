use axiomc::transactional_workspace::{TransactionPhase, TransactionalWorkspace, WorkspacePolicy};
use jsonschema::Validator;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("run git fixture command");
    assert!(
        output.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("git output is UTF-8")
}

fn fixture() -> (TempDir, PathBuf, String) {
    let root = TempDir::new().expect("create fixture root");
    let source = root.path().join("source");
    fs::create_dir(&source).expect("create source repository");
    git(&source, &["init", "-q"]);
    git(&source, &["config", "user.email", "test@example.invalid"]);
    git(&source, &["config", "user.name", "Test"]);
    fs::write(source.join("allowed.txt"), b"original").expect("write allowed fixture");
    fs::write(source.join("owned.txt"), b"committed").expect("write owned fixture");
    git(&source, &["add", "allowed.txt", "owned.txt"]);
    git(
        &source,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "base"],
    );
    let sha = git(&source, &["rev-parse", "HEAD"]).trim().to_owned();
    (root, source, sha)
}

fn policy() -> WorkspacePolicy {
    WorkspacePolicy {
        allowed_read_paths: BTreeSet::from(["allowed.txt".to_owned()]),
        allowed_write_paths: BTreeSet::from(["allowed.txt".to_owned(), "created.txt".to_owned()]),
        allowed_commands: BTreeSet::from(["git".to_owned()]),
        allow_network: false,
        verified_sandbox: true,
    }
}

fn audit_validator() -> Validator {
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("schemas")
        .join("axiom-execution-transaction-v0.schema.json");
    let schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_path).expect("read execution transaction schema"),
    )
    .expect("parse execution transaction schema");
    jsonschema::validator_for(&schema).expect("compile execution transaction schema")
}

#[test]
fn denial_matrix_fails_closed_without_out_of_scope_mutation() {
    let (root, source, sha) = fixture();
    let worktree = root.path().join("transaction");
    let mut transaction =
        TransactionalWorkspace::create(&source, &worktree, &sha, policy()).expect("create");

    assert!(transaction.write("../owned.txt", b"traversal").is_err());
    assert!(transaction.write("owned.txt", b"scope escape").is_err());
    assert!(transaction.delete("owned.txt").is_err());
    assert!(transaction.rename("owned.txt", "created.txt").is_err());
    #[cfg(unix)]
    assert!(transaction.chmod("owned.txt", 0o777).is_err());
    assert!(transaction
        .write(".codex/policies/policy.json", b"bypass")
        .is_err());
    assert!(transaction.authorize_external("sh", false).is_err());
    assert!(transaction.authorize_external("git", true).is_err());
    for operation in [
        "push_protected_branch",
        "force_push",
        "self_approve",
        "edit_policy",
    ] {
        assert!(TransactionalWorkspace::reject_delivery_operation(operation).is_err());
    }
    assert_eq!(fs::read(source.join("owned.txt")).unwrap(), b"committed");
}

#[cfg(unix)]
#[test]
fn symlink_escape_is_denied_for_write_rename_delete_and_chmod() {
    use std::os::unix::fs::symlink;

    let (root, source, sha) = fixture();
    let worktree = root.path().join("transaction");
    let mut scoped = policy();
    for path in ["escape/owned.txt", "escape/renamed.txt"] {
        scoped.allowed_write_paths.insert(path.to_owned());
    }
    let mut transaction =
        TransactionalWorkspace::create(&source, &worktree, &sha, scoped).expect("create");
    symlink(&source, worktree.join("escape")).expect("create escape symlink");

    assert!(transaction.write("escape/owned.txt", b"escape").is_err());
    assert!(transaction.delete("escape/owned.txt").is_err());
    assert!(transaction
        .rename("escape/owned.txt", "escape/renamed.txt")
        .is_err());
    assert!(transaction.chmod("escape/owned.txt", 0o600).is_err());
    assert_eq!(fs::read(source.join("owned.txt")).unwrap(), b"committed");
}

#[test]
fn failed_transaction_rolls_back_and_preserves_dirty_source_index() {
    let (root, source, sha) = fixture();
    fs::write(source.join("owned.txt"), b"user dirty").expect("make source dirty");
    fs::write(source.join("untracked.txt"), b"user untracked").expect("make untracked file");
    let before = git(&source, &["status", "--porcelain=v1"]);
    let worktree = root.path().join("transaction");
    let mut transaction =
        TransactionalWorkspace::create(&source, &worktree, &sha, policy()).expect("create");
    transaction.write("allowed.txt", b"changed").expect("write");
    transaction
        .write("created.txt", b"created")
        .expect("create");
    transaction.abort().expect("rollback");

    assert_eq!(transaction.state().phase, TransactionPhase::Aborted);
    assert_eq!(fs::read(worktree.join("allowed.txt")).unwrap(), b"original");
    assert!(!worktree.join("created.txt").exists());
    assert_eq!(fs::read(source.join("owned.txt")).unwrap(), b"user dirty");
    assert_eq!(
        fs::read(source.join("untracked.txt")).unwrap(),
        b"user untracked"
    );
    assert_eq!(git(&source, &["status", "--porcelain=v1"]), before);
    assert!(git(&source, &["diff", "--cached", "--name-only"]).is_empty());
}

#[test]
fn interrupted_transaction_is_inspectable_and_can_resume_or_roll_back() {
    let (root, source, sha) = fixture();
    let worktree = root.path().join("transaction");
    let mut transaction =
        TransactionalWorkspace::create(&source, &worktree, &sha, policy()).expect("create");
    transaction.read("allowed.txt").expect("record read");
    transaction.write("created.txt", b"partial").expect("write");
    assert!(transaction.authorize_external("git", false).is_err());
    transaction
        .record_artifact("allowed.txt")
        .expect("record artifact");
    transaction.mark_interrupted().expect("interrupt");
    drop(transaction);

    let mut recovered = TransactionalWorkspace::recover(&worktree).expect("inspect journal");
    assert_eq!(recovered.state().phase, TransactionPhase::Interrupted);
    let first = recovered.deterministic_audit_json().expect("first audit");
    assert_eq!(
        first,
        recovered.deterministic_audit_json().expect("second audit")
    );
    let audit: Value = serde_json::from_str(&first).expect("audit is JSON");
    audit_validator()
        .validate(&audit)
        .expect("runtime audit matches the execution transaction schema");
    assert_eq!(audit["base_sha"], sha);
    assert_eq!(audit["status"], "interrupted");
    assert!(audit["recovery"]["resumable"].as_bool().unwrap());
    for field in ["checkpoints", "reads", "writes", "commands", "artifacts"] {
        assert!(
            !audit[field]
                .as_array()
                .expect("audit collection")
                .is_empty(),
            "runtime audit records {field}"
        );
    }
    assert_eq!(
        audit["reads"][0]["digest"],
        "sha256:0682c5f2076f099c34cfdd15a9e063849ed437a49677e6fcc5b4198c76575be5"
    );
    assert_eq!(
        audit["writes"][0]["after_digest"],
        "sha256:9834a14ab9bcaa0f6a8da71073617eac8f004e596a3fa11d807b84631b825d9d"
    );
    assert_eq!(audit["commands"][0]["outcome"], "denied");
    assert_eq!(audit["commands"][0]["exit_code"], 126);
    assert_eq!(
        audit["artifacts"][0]["digest"],
        "sha256:0682c5f2076f099c34cfdd15a9e063849ed437a49677e6fcc5b4198c76575be5"
    );
    assert!(!first.contains("secret_value"));
    recovered.resume().expect("resume");
    recovered.mark_interrupted().expect("interrupt again");
    recovered.abort().expect("rollback recovered transaction");
    assert!(!worktree.join("created.txt").exists());
    assert_eq!(fs::read(source.join("owned.txt")).unwrap(), b"committed");
}
