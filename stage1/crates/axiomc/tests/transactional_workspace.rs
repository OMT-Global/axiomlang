use axiomc::transactional_workspace::{TransactionPhase, TransactionalWorkspace, WorkspacePolicy};
use jsonschema::Validator;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};
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

#[test]
fn policy_scoped_fingerprint_updates_authorized_paths_but_ignores_unrelated_changes() {
    let (root, source, sha) = fixture();
    let worktree = root.path().join("transaction");
    let mut transaction =
        TransactionalWorkspace::create(&source, &worktree, &sha, policy()).expect("create");
    let initial_fingerprint = transaction.state().workspace_fingerprint.clone();
    let initial_cache = transaction.state().authorized_path_fingerprints.clone();

    transaction
        .write("allowed.txt", b"changed")
        .expect("update authorized path");
    let after_allowed_fingerprint = transaction.state().workspace_fingerprint.clone();
    assert_ne!(after_allowed_fingerprint, initial_fingerprint);
    assert_ne!(
        transaction.state().authorized_path_fingerprints["allowed.txt"],
        initial_cache["allowed.txt"]
    );
    assert_eq!(
        transaction.state().authorized_path_fingerprints["created.txt"],
        initial_cache["created.txt"]
    );

    transaction
        .write("created.txt", b"created")
        .expect("create authorized path");
    let after_created_fingerprint = transaction.state().workspace_fingerprint.clone();
    assert_ne!(after_created_fingerprint, after_allowed_fingerprint);
    assert_ne!(
        transaction.state().authorized_path_fingerprints["created.txt"],
        initial_cache["created.txt"]
    );

    fs::write(worktree.join("owned.txt"), b"unrelated change").expect("change unrelated path");
    assert_eq!(
        transaction.state().workspace_fingerprint,
        after_created_fingerprint,
        "unrelated worktree content is outside the policy-scoped fingerprint"
    );
    drop(transaction);
    assert!(TransactionalWorkspace::recover(&worktree).is_err());
}

#[test]
fn recovery_claims_a_new_owner_epoch_and_durable_generation() {
    let (root, source, sha) = fixture();
    let worktree = root.path().join("transaction");
    let mut transaction =
        TransactionalWorkspace::create(&source, &worktree, &sha, policy()).expect("create");
    let initial: Value =
        serde_json::from_slice(&fs::read(worktree.join(".axiom-transaction.json")).unwrap())
            .unwrap();
    let initial_epoch = initial["owner_epoch"].as_str().unwrap().to_owned();
    let initial_generation = initial["generation"].as_u64().unwrap();
    transaction.mark_interrupted().expect("interrupt");
    drop(transaction);

    let recovered = TransactionalWorkspace::recover(&worktree).expect("recover");
    assert_ne!(recovered.state().owner_epoch, initial_epoch);
    assert!(recovered.state().generation > initial_generation);
    let durable: Value =
        serde_json::from_slice(&fs::read(worktree.join(".axiom-transaction.json")).unwrap())
            .unwrap();
    assert_eq!(
        durable["owner_epoch"],
        Value::String(recovered.state().owner_epoch.clone())
    );
    assert_eq!(
        durable["generation"],
        Value::from(recovered.state().generation)
    );
}

#[test]
fn stale_generation_is_rejected_without_merging_audit_events() {
    let (root, source, sha) = fixture();
    let worktree = root.path().join("transaction");
    let mut transaction =
        TransactionalWorkspace::create(&source, &worktree, &sha, policy()).expect("create");
    let state_path = worktree.join(".axiom-transaction.json");
    let stale_state = fs::read(&state_path).expect("capture stale durable state");
    transaction.read("allowed.txt").expect("acknowledged read");

    // A stale writer can only be detected, not merged. Keep the legacy fixed
    // temp name occupied as well: persistence must allocate its own exclusive
    // temp file and leave an unrelated writer's temp untouched.
    let legacy_temp = worktree.join(".axiom-transaction.json.tmp");
    fs::write(&legacy_temp, b"unrelated writer temp").expect("occupy legacy temp name");
    fs::write(&state_path, stale_state).expect("install stale state");
    let error = transaction
        .read("allowed.txt")
        .expect_err("stale generation must fail closed");
    assert!(error.contains("owner/generation conflict"));
    assert_eq!(fs::read(&legacy_temp).unwrap(), b"unrelated writer temp");

    let durable: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    let events = durable["events"].as_array().unwrap();
    assert_eq!(events.len(), 1, "the rejected event was not durably merged");
    assert_eq!(events[0]["sequence"], 0);
}

#[test]
fn two_process_recovery_race_is_rejected_while_lease_is_held() {
    let (root, source, sha) = fixture();
    let worktree = root.path().join("transaction");
    let mut transaction =
        TransactionalWorkspace::create(&source, &worktree, &sha, policy()).expect("create");
    transaction.mark_interrupted().expect("interrupt");
    drop(transaction);

    let signal = root.path().join("lease-held");
    let child = std::env::current_exe().expect("test executable");
    let mut child = Command::new(child)
        .args(["--exact", "lease_holder_child", "--nocapture"])
        .env("AXIOM_LEASE_WORKTREE", worktree.to_str().unwrap())
        .env("AXIOM_LEASE_SIGNAL", signal.to_str().unwrap())
        .spawn()
        .expect("spawn lease holder");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !signal.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        signal.exists(),
        "child did not acquire the transaction lease"
    );

    let error = TransactionalWorkspace::recover(&worktree)
        .expect_err("second process must not recover the leased transaction");
    assert!(error.contains("transaction lease unavailable"));
    assert!(child.wait().expect("wait for lease holder").success());

    let recovered = TransactionalWorkspace::recover(&worktree).expect("recover after release");
    assert_eq!(recovered.state().phase, TransactionPhase::Interrupted);
}

#[test]
fn lease_holder_child() {
    let (Some(worktree), Some(signal)) = (
        std::env::var_os("AXIOM_LEASE_WORKTREE"),
        std::env::var_os("AXIOM_LEASE_SIGNAL"),
    ) else {
        return;
    };
    let worktree = PathBuf::from(worktree);
    let signal = PathBuf::from(signal);
    let transaction = TransactionalWorkspace::recover(&worktree).expect("acquire lease");
    fs::write(signal, b"held").expect("signal lease acquisition");
    thread::sleep(Duration::from_millis(750));
    drop(transaction);
}
