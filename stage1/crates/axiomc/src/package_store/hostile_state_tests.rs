//! Reconstructed partial publications and hostile transaction cleanup state.
use super::*;

const DEAD_PID: u32 = 2_000_000_000;

fn artifacts<'a>(archive: &'a [u8], digest: &'a str) -> VerifiedArtifacts<'a> {
    VerifiedArtifacts {
        archive_sha256: digest,
        archive,
        manifest: b"manifest",
        provenance: b"provenance",
        signature: b"signature",
        registry_index: b"registry-index",
        verification: b"verification",
    }
}

fn transaction(parent: &Path, name: &str, marker: Option<&[u8]>) -> PathBuf {
    let path = parent.join(name);
    fs::create_dir(&path).unwrap();
    fs::write(path.join("partial"), b"partial").unwrap();
    if let Some(marker) = marker {
        fs::write(path.join(TRANSACTION_MARKER_NAME), marker).unwrap();
    }
    path
}

#[test]
fn incomplete_admission_prefixes_reject_offline_load_and_recover_on_retry() {
    let bytes = b"AXIOM_PACKAGE_ARCHIVE_V1\n--- file a 1 ---\nx\n";
    let other_bytes = b"AXIOM_PACKAGE_ARCHIVE_V1\n--- file b 1 ---\ny\n";
    let digest = sha256_hex(bytes);
    let other_digest = sha256_hex(other_bytes);
    let index = sha256_hex(b"registry-index");
    let verification = sha256_hex(b"verification");
    for (phase, label) in ["blob", "tree", "evidence"].into_iter().enumerate() {
        let temp = tempfile::tempdir().unwrap();
        let store = PackageStore::open(&temp.path().join("cache")).unwrap();
        let cached = store.admit(artifacts(bytes, &digest)).unwrap();
        store.admit(artifacts(other_bytes, &other_digest)).unwrap();
        let commit = store.commit_path(&digest, &index, &evidence_identity(&index, &verification));

        // Reconstruct each intact publication prefix before the final commit.
        // This models on-disk state, not a process crash or power-loss ordering.
        fs::remove_file(&commit).unwrap();
        if phase < 2 {
            fs::remove_dir_all(&cached.evidence).unwrap();
        }
        if phase < 1 {
            fs::remove_dir_all(&cached.tree).unwrap();
        }
        let error = store.load_verified(&digest).unwrap_err();
        assert_eq!(error.code, "cache_evidence_unavailable", "{label}");
        let error = store
            .load_verified_exact(&digest, &index, &verification)
            .unwrap_err();
        assert!(
            !error.code.is_empty() && !error.message.is_empty(),
            "{label}"
        );
        assert!(!commit.exists(), "{label}: reads must not publish a commit");
        assert_eq!(
            store
                .load_verified(&other_digest)
                .unwrap()
                .verified_artifacts()
                .unwrap()
                .archive,
            other_bytes,
            "{label}: unrelated committed package changed"
        );

        let recovered = store.admit(artifacts(bytes, &digest)).unwrap();
        assert_eq!(
            recovered.verified_artifacts().unwrap().archive,
            bytes,
            "{label}"
        );
        assert_eq!(fs::read(recovered.tree.join("a")).unwrap(), b"x", "{label}");
        assert_eq!(
            store
                .load_verified_exact(&digest, &index, &verification)
                .unwrap()
                .commit,
            recovered.commit,
            "{label}"
        );
        assert_eq!(
            fs::read_dir(store.root().join(".transactions"))
                .unwrap()
                .count(),
            0
        );
    }
}

#[test]
fn cleanup_preserves_missing_malformed_and_mismatched_owner_markers() {
    assert!(!process_is_alive(DEAD_PID), "fixture PID must be absent");
    let valid = transaction_marker_bytes(DEAD_PID, 0, 7);
    let cases = [
        ("missing", None),
        ("directory", None),
        ("empty", Some(Vec::new())),
        ("truncated", Some(valid[..valid.len() - 1].to_vec())),
        ("invalid-utf8", Some(vec![0xff])),
        ("oversized", Some(vec![b'x'; 257])),
        (
            "wrong-pid",
            Some(transaction_marker_bytes(DEAD_PID - 1, 0, 7)),
        ),
        ("wrong-time", Some(transaction_marker_bytes(DEAD_PID, 1, 7))),
        (
            "wrong-sequence",
            Some(transaction_marker_bytes(DEAD_PID, 0, 8)),
        ),
    ];
    for (label, marker) in cases {
        let temp = tempfile::tempdir().unwrap();
        let path = transaction(
            temp.path(),
            &format!(".admit-{DEAD_PID}-0-7"),
            marker.as_deref(),
        );
        if label == "directory" {
            fs::create_dir(path.join(TRANSACTION_MARKER_NAME)).unwrap();
        }
        let owned = transaction(
            temp.path(),
            &format!(".admit-{DEAD_PID}-0-8"),
            Some(&transaction_marker_bytes(DEAD_PID, 0, 8)),
        );
        reap_stale_transactions(temp.path(), STALE_TRANSACTION_AGE_NANOS).unwrap();
        assert!(
            !owned.exists(),
            "{label}: valid stale control was not reclaimed"
        );
        assert_eq!(
            fs::read(path.join("partial")).unwrap(),
            b"partial",
            "{label}"
        );
        if let Some(marker) = marker {
            assert_eq!(
                fs::read(path.join(TRANSACTION_MARKER_NAME)).unwrap(),
                marker,
                "{label}"
            );
        }
    }
}

#[test]
fn cleanup_respects_age_boundary_and_future_timestamps() {
    assert!(!process_is_alive(DEAD_PID), "fixture PID must be absent");
    let temp = tempfile::tempdir().unwrap();
    let now = 2 * STALE_TRANSACTION_AGE_NANOS;
    for (sequence, created, reclaimed) in [
        (1, now - STALE_TRANSACTION_AGE_NANOS, true),
        (2, now - STALE_TRANSACTION_AGE_NANOS + 1, false),
        (3, now + 1, false),
    ] {
        let path = transaction(
            temp.path(),
            &format!(".admit-{DEAD_PID}-{created}-{sequence}"),
            Some(&transaction_marker_bytes(DEAD_PID, created, sequence)),
        );
        reap_stale_transactions(temp.path(), now).unwrap();
        assert_eq!(!path.exists(), reclaimed, "sequence {sequence}");
        if !reclaimed {
            assert_eq!(fs::read(path.join("partial")).unwrap(), b"partial");
        }
    }
}

#[cfg(unix)]
#[test]
fn cleanup_preserves_a_live_owner_even_with_an_old_timestamp() {
    let temp = tempfile::tempdir().unwrap();
    let pid = std::process::id();
    let path = transaction(
        temp.path(),
        &format!(".admit-{pid}-0-7"),
        Some(&transaction_marker_bytes(pid, 0, 7)),
    );
    reap_stale_transactions(temp.path(), STALE_TRANSACTION_AGE_NANOS).unwrap();
    assert_eq!(fs::read(path.join("partial")).unwrap(), b"partial");
}

#[cfg(unix)]
#[test]
fn cleanup_does_not_follow_transaction_or_marker_symlinks() {
    use std::os::unix::fs::symlink;
    assert!(!process_is_alive(DEAD_PID), "fixture PID must be absent");
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("transactions");
    fs::create_dir(&root).unwrap();
    let outside = transaction(
        temp.path(),
        "outside",
        Some(&transaction_marker_bytes(DEAD_PID, 0, 7)),
    );
    let linked_transaction = root.join(format!(".admit-{DEAD_PID}-0-7"));
    symlink(&outside, &linked_transaction).unwrap();
    let linked_marker = transaction(&root, &format!(".admit-{DEAD_PID}-0-8"), None);
    let outside_marker = temp.path().join("outside-marker");
    let marker = transaction_marker_bytes(DEAD_PID, 0, 8);
    fs::write(&outside_marker, &marker).unwrap();
    symlink(&outside_marker, linked_marker.join(TRANSACTION_MARKER_NAME)).unwrap();
    let owned = transaction(
        &root,
        &format!(".admit-{DEAD_PID}-0-9"),
        Some(&transaction_marker_bytes(DEAD_PID, 0, 9)),
    );
    symlink(&outside, owned.join("external-tree")).unwrap();

    reap_stale_transactions(&root, STALE_TRANSACTION_AGE_NANOS).unwrap();
    assert!(!owned.exists(), "valid stale directory should be reclaimed");
    assert!(
        fs::symlink_metadata(linked_transaction)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::symlink_metadata(linked_marker.join(TRANSACTION_MARKER_NAME))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(linked_marker.join("partial")).unwrap(), b"partial");
    assert_eq!(fs::read(outside.join("partial")).unwrap(), b"partial");
    assert_eq!(fs::read(outside_marker).unwrap(), marker);
}
