//! Manual, reproducible scaling evidence. Timings are not correctness assertions.
use axiomc::transactional_workspace::{TransactionalWorkspace, WorkspacePolicy};
use std::{collections::BTreeSet, fs, path::Path, process::Command, time::Instant};

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

#[test]
#[ignore = "manual large-tree timing comparison; no wall-clock correctness threshold"]
fn bounded_writes_large_tree_benchmark() {
    for unrelated_files in [0, 2000] {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("source");
        fs::create_dir(&source).unwrap();
        git(&source, &["init", "-q"]);
        git(&source, &["config", "user.name", "fixture"]);
        git(
            &source,
            &["config", "user.email", "fixture@example.invalid"],
        );
        fs::write(source.join("allowed.txt"), b"original").unwrap();
        let unrelated = source.join("unrelated");
        fs::create_dir(&unrelated).unwrap();
        for index in 0..unrelated_files {
            fs::write(unrelated.join(format!("{index:05}.txt")), vec![b'x'; 4096]).unwrap();
        }
        git(&source, &["add", "."]);
        git(
            &source,
            &["-c", "commit.gpgsign=false", "commit", "-qm", "fixture"],
        );
        let sha = git(&source, &["rev-parse", "HEAD"]);
        let policy = WorkspacePolicy {
            allowed_read_paths: BTreeSet::from(["allowed.txt".into()]),
            allowed_write_paths: BTreeSet::from(["allowed.txt".into()]),
            ..WorkspacePolicy::default()
        };
        let start = Instant::now();
        let mut transaction = TransactionalWorkspace::create(
            &source,
            &fixture.path().join("transaction"),
            &sha,
            policy,
        )
        .unwrap();
        let create_ms = start.elapsed().as_millis();
        let start = Instant::now();
        for index in 0..32 {
            transaction
                .write("allowed.txt", format!("write-{index}").as_bytes())
                .unwrap();
        }
        let writes_ms = start.elapsed().as_millis();
        assert_eq!(transaction.state().events.len(), 65);
        drop(transaction);
        let start = Instant::now();
        let recovered =
            TransactionalWorkspace::recover(&fixture.path().join("transaction")).unwrap();
        assert_eq!(recovered.state().events.len(), 65);
        println!("BENCH unrelated_files={unrelated_files} writes=32 create_ms={create_ms} writes_ms={writes_ms} recovery_ms={}", start.elapsed().as_millis());
    }
}
