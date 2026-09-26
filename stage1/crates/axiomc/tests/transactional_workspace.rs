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

/// Publish a race-result file atomically. The parent polls for existence, so
/// a plain create-then-write can be observed empty mid-publication.
fn publish_result(root: &Path, id: &str, bytes: &[u8]) {
    let tmp = root.join(format!("result-{id}.tmp"));
    fs::write(&tmp, bytes).unwrap();
    fs::rename(&tmp, root.join(format!("result-{id}"))).unwrap();
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

#[cfg(unix)]
#[test]
fn symlink_aliases_are_denied_for_every_filesystem_operation() {
    use std::os::unix::fs::symlink;

    let (root, source, sha) = fixture();
    let worktree = root.path().join("transaction");
    let mut scoped = policy();
    for path in [
        "owned.txt",
        "escape/owned.txt",
        "escape/renamed.txt",
        "destination/owned.txt",
        "leaf.txt",
        "protected_alias/secret.txt",
        "nested/ordinary.txt",
    ] {
        scoped.allowed_read_paths.insert(path.to_owned());
        scoped.allowed_write_paths.insert(path.to_owned());
    }
    let mut transaction =
        TransactionalWorkspace::create(&source, &worktree, &sha, scoped).expect("create");

    transaction
        .write("nested/ordinary.txt", b"ordinary")
        .expect("ordinary nested write remains supported");
    assert_eq!(
        transaction.read("nested/ordinary.txt").unwrap(),
        b"ordinary"
    );

    symlink(&source, worktree.join("escape")).expect("create external alias");
    symlink(worktree.join("owned.txt"), worktree.join("leaf.txt")).expect("create leaf alias");
    fs::create_dir_all(worktree.join(".codex/policies")).expect("create protected fixture");
    fs::write(worktree.join(".codex/policies/secret.txt"), b"protected")
        .expect("write protected fixture");
    symlink(
        worktree.join(".codex/policies"),
        worktree.join("protected_alias"),
    )
    .expect("create protected alias");
    symlink(&source, worktree.join("destination")).expect("create destination alias");

    let expected = "path contains a symlink or reparse component";
    assert_eq!(transaction.read("escape/owned.txt").unwrap_err(), expected);
    assert_eq!(
        transaction
            .write("escape/owned.txt", b"escape")
            .unwrap_err(),
        expected
    );
    assert_eq!(
        transaction.delete("escape/owned.txt").unwrap_err(),
        expected
    );
    assert_eq!(
        transaction
            .rename("escape/owned.txt", "escape/renamed.txt")
            .unwrap_err(),
        expected
    );
    assert_eq!(
        transaction.chmod("escape/owned.txt", 0o600).unwrap_err(),
        expected
    );
    assert_eq!(
        transaction.record_artifact("escape/owned.txt").unwrap_err(),
        expected
    );

    assert_eq!(transaction.read("leaf.txt").unwrap_err(), expected);
    assert_eq!(
        transaction.write("leaf.txt", b"leaf").unwrap_err(),
        expected
    );
    assert_eq!(transaction.delete("leaf.txt").unwrap_err(), expected);
    assert_eq!(transaction.chmod("leaf.txt", 0o600).unwrap_err(), expected);
    assert_eq!(
        transaction.record_artifact("leaf.txt").unwrap_err(),
        expected
    );
    assert_eq!(
        transaction
            .rename("owned.txt", "destination/owned.txt")
            .unwrap_err(),
        expected
    );
    assert_eq!(
        transaction.read("protected_alias/secret.txt").unwrap_err(),
        expected
    );
    assert_eq!(
        transaction
            .write("protected_alias/secret.txt", b"bypass")
            .unwrap_err(),
        expected
    );
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

#[cfg(unix)]
#[test]
fn abort_uses_retained_root_descriptor_after_worktree_path_is_replaced() {
    use std::os::unix::fs::symlink;

    let (root, source, sha) = fixture();
    let worktree = root.path().join("transaction");
    let moved_worktree = root.path().join("retained-transaction");
    let mut transaction =
        TransactionalWorkspace::create(&source, &worktree, &sha, policy()).expect("create");
    transaction.write("allowed.txt", b"changed").expect("write");
    transaction
        .write("created.txt", b"created")
        .expect("create untracked file");

    fs::rename(&worktree, &moved_worktree).expect("move checked-out worktree");
    symlink(&source, &worktree).expect("replace worktree pathname with source symlink");

    transaction
        .abort()
        .expect("rollback through retained descriptor");

    assert_eq!(fs::read(source.join("allowed.txt")).unwrap(), b"original");
    assert_eq!(
        fs::read(moved_worktree.join("allowed.txt")).unwrap(),
        b"original"
    );
    assert!(!moved_worktree.join("created.txt").exists());
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

#[test]
fn foreign_owner_same_generation_is_rejected_before_any_file_effect() {
    use sha2::{Digest, Sha256};
    let (root, source, sha) = fixture();
    let worktree = root.path().join("transaction");
    let mut transaction =
        TransactionalWorkspace::create(&source, &worktree, &sha, policy()).unwrap();
    let mut foreign = transaction.state().clone();
    foreign.owner_epoch = format!("sha256:{}", "f".repeat(64));
    assert_ne!(foreign.owner_epoch, transaction.state().owner_epoch);
    foreign.checksum.clear();
    foreign.checksum = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&foreign).unwrap())
    );
    let bytes = serde_json::to_vec_pretty(&foreign).unwrap();
    let state_path = worktree.join(".axiom-transaction.json");
    fs::write(&state_path, &bytes).unwrap();
    let error = transaction
        .write("allowed.txt", b"must not happen")
        .unwrap_err();
    assert!(error.contains("owner/generation conflict"), "{error}");
    assert_eq!(fs::read(worktree.join("allowed.txt")).unwrap(), b"original");
    assert_eq!(fs::read(state_path).unwrap(), bytes);
}

#[cfg(unix)]
#[test]
fn state_symlink_substitution_fails_before_effect_and_preserves_target() {
    use std::os::unix::fs::symlink;
    let (root, source, sha) = fixture();
    let worktree = root.path().join("transaction");
    let mut transaction =
        TransactionalWorkspace::create(&source, &worktree, &sha, policy()).unwrap();
    let state_path = worktree.join(".axiom-transaction.json");
    let outside = root.path().join("outside-state");
    let bytes = fs::read(&state_path).unwrap();
    fs::rename(&state_path, &outside).unwrap();
    symlink(&outside, &state_path).unwrap();
    assert!(transaction
        .write("allowed.txt", b"must not happen")
        .is_err());
    assert_eq!(fs::read(worktree.join("allowed.txt")).unwrap(), b"original");
    assert_eq!(fs::read(&outside).unwrap(), bytes);
    assert!(fs::symlink_metadata(&state_path)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[cfg(unix)]
#[test]
fn recovery_rejects_symlink_or_hardlinked_lease_without_touching_state() {
    use std::os::unix::fs::symlink;
    let (root, source, sha) = fixture();
    let worktree = root.path().join("transaction");
    let transaction = TransactionalWorkspace::create(&source, &worktree, &sha, policy()).unwrap();
    drop(transaction);
    let lease_path = worktree.join(".axiom-transaction.lock");
    let outside = root.path().join("lease-outside");
    let state_path = worktree.join(".axiom-transaction.json");
    let bytes = fs::read(&state_path).unwrap();
    fs::rename(&lease_path, &outside).unwrap();
    symlink(&outside, &lease_path).unwrap();
    assert!(TransactionalWorkspace::recover(&worktree).is_err());
    fs::remove_file(&lease_path).unwrap();
    fs::hard_link(&outside, &lease_path).unwrap();
    assert!(TransactionalWorkspace::recover(&worktree).is_err());
    assert_eq!(fs::read(state_path).unwrap(), bytes);
}

#[test]
fn simultaneous_recovery_has_one_owner_and_crash_preserves_acknowledged_events() {
    let (root, source, sha) = fixture();
    let worktree = root.path().join("transaction");
    let mut transaction =
        TransactionalWorkspace::create(&source, &worktree, &sha, policy()).unwrap();
    transaction.mark_interrupted().unwrap();
    let previous_epoch = transaction.state().owner_epoch.clone();
    let previous_events = transaction.state().events.len();
    drop(transaction);
    let mut children = Vec::new();
    for id in 0..2 {
        children.push(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "simultaneous_recovery_child", "--nocapture"])
                .env("AXIOM_RECOVERY_RACE_ROOT", root.path())
                .env("AXIOM_RECOVERY_RACE_ID", id.to_string())
                .spawn()
                .unwrap(),
        );
    }
    let wait_for = |paths: &[PathBuf]| {
        let start = Instant::now();
        while !paths.iter().all(|path| path.exists()) {
            assert!(
                start.elapsed() < Duration::from_secs(15),
                "race fixture timed out"
            );
            thread::sleep(Duration::from_millis(10));
        }
    };
    wait_for(&[root.path().join("ready-0"), root.path().join("ready-1")]);
    fs::write(root.path().join("start"), b"start").unwrap();
    wait_for(&[root.path().join("result-0"), root.path().join("result-1")]);
    let outcomes: Vec<_> = (0..2)
        .map(|id| fs::read_to_string(root.path().join(format!("result-{id}"))).unwrap())
        .collect();
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| *outcome == "owned")
            .count(),
        1,
        "{outcomes:?}"
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| outcome.contains("lease unavailable"))
            .count(),
        1,
        "{outcomes:?}"
    );
    for (child, outcome) in children.iter_mut().zip(outcomes) {
        if outcome == "owned" {
            child.kill().unwrap();
        }
        let status = child.wait().unwrap();
        if outcome != "owned" {
            assert!(status.success());
        }
    }
    let recovered = TransactionalWorkspace::recover(&worktree).unwrap();
    assert_ne!(recovered.state().owner_epoch, previous_epoch);
    assert_eq!(recovered.state().events.len(), previous_events + 2);
    for (sequence, event) in recovered.state().events.iter().enumerate() {
        assert_eq!(event.sequence, sequence as u64);
    }
    assert_eq!(recovered.state().events.last().unwrap().operation, "read");
}

#[test]
fn simultaneous_recovery_child() {
    let Ok(root) = std::env::var("AXIOM_RECOVERY_RACE_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let id = std::env::var("AXIOM_RECOVERY_RACE_ID").unwrap();
    fs::write(root.join(format!("ready-{id}")), b"ready").unwrap();
    let start = Instant::now();
    while !root.join("start").exists() {
        assert!(start.elapsed() < Duration::from_secs(15));
        thread::sleep(Duration::from_millis(10));
    }
    match TransactionalWorkspace::recover(&root.join("transaction")) {
        Ok(mut transaction) => {
            transaction.resume().unwrap();
            transaction.read("allowed.txt").unwrap();
            publish_result(&root, &id, b"owned");
            // Parent kills this owner after its acknowledged event is durable.
            thread::sleep(Duration::from_secs(20));
            drop(transaction);
            panic!("parent failed to terminate crash fixture");
        }
        Err(error) => publish_result(&root, &id, error.as_bytes()),
    }
}

#[test]
fn residual_control_live_owner_excludes_recovery() {
    let (root, source, sha) = fixture();
    let worktree = root.path().join("transaction");
    let transaction = TransactionalWorkspace::create(&source, &worktree, &sha, policy()).unwrap();
    assert!(
        TransactionalWorkspace::recover(&worktree).is_err(),
        "live owner must exclude recovery"
    );
    drop(transaction);
}
#[test]
fn residual_control_stale_generation_rejected() {
    let (root, source, sha) = fixture();
    let worktree = root.path().join("transaction");
    let mut transaction =
        TransactionalWorkspace::create(&source, &worktree, &sha, policy()).unwrap();
    let path = worktree.join(".axiom-transaction.json");
    let stale = fs::read(&path).unwrap();
    transaction.read("allowed.txt").unwrap();
    fs::write(&path, stale).unwrap();
    assert!(
        transaction
            .write("allowed.txt", b"must not happen")
            .is_err(),
        "stale state must reject effect"
    );
    assert_eq!(fs::read(worktree.join("allowed.txt")).unwrap(), b"original");
}
