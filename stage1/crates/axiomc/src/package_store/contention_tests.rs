//! Bounded contention over immutable cache publication and evidence identities.
use super::*;
use std::sync::Barrier;

const ARCHIVE: &[u8] = b"AXIOM_PACKAGE_ARCHIVE_V1\n--- file a 1 ---\nx\n--- file dir/b 1 ---\ny\n";

fn artifacts<'a>(
    digest: &'a str,
    index: &'a [u8],
    verification: &'a [u8],
) -> VerifiedArtifacts<'a> {
    VerifiedArtifacts {
        archive_sha256: digest,
        archive: ARCHIVE,
        manifest: b"manifest",
        provenance: b"provenance",
        signature: b"signature",
        registry_index: index,
        verification,
    }
}

fn assert_material(package: &CachedPackage, index: &[u8], verification: &[u8]) {
    let material = package.verified_artifacts().unwrap();
    assert_eq!(material.archive, ARCHIVE);
    assert_eq!(material.registry_index, index);
    assert_eq!(material.verification, verification);
    assert_eq!(package.commit.registry_index_sha256, sha256_hex(index));
    assert_eq!(package.commit.verification_sha256, sha256_hex(verification));
    assert_eq!(fs::read(package.tree.join("a")).unwrap(), b"x");
    assert_eq!(fs::read(package.tree.join("dir/b")).unwrap(), b"y");
}

fn contend(
    store: &PackageStore,
    digest: &str,
    identities: &[(&[u8], &[u8])],
) -> Vec<CachedPackage> {
    assert_eq!(
        store.load_verified(digest).unwrap_err().code,
        "cache_evidence_unavailable"
    );
    let start = Barrier::new(identities.len() + 1);
    let multiple_identities = identities.windows(2).any(|pair| pair[0] != pair[1]);
    let packages = std::thread::scope(|scope| {
        let workers: Vec<_> = identities
            .iter()
            .map(|&(index, verification)| {
                let start = &start;
                scope.spawn(move || {
                    start.wait();
                    store.admit(artifacts(digest, index, verification))
                })
            })
            .collect();
        start.wait();
        // Probe during the contention window. Scheduling may run these reads
        // before, between or after writers; no particular interleaving is claimed.
        for _ in 0..64 {
            match store.load_verified(digest) {
                Ok(package) => {
                    let material = package.verified_artifacts().unwrap();
                    assert!(identities.contains(&(material.registry_index, material.verification)));
                    assert_material(&package, material.registry_index, material.verification);
                }
                Err(error) => {
                    assert!(
                        error.code == "cache_evidence_unavailable"
                            || (multiple_identities && error.code == "cache_evidence_ambiguous"),
                        "unexpected read error during admission: {error}"
                    );
                    assert!(!error.message.is_empty());
                }
            }
            std::thread::yield_now();
        }
        workers
            .into_iter()
            .map(|worker| {
                worker
                    .join()
                    .expect("admission writer panicked")
                    .expect("admission failed")
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(
        fs::read_dir(store.root().join(".transactions"))
            .unwrap()
            .count(),
        0,
        "all writer transactions must be cleaned after publication"
    );
    packages
}

#[test]
fn identical_concurrent_admissions_converge_on_one_committed_package() {
    let digest = sha256_hex(ARCHIVE);
    let identities = [(b"index".as_slice(), b"verification".as_slice()); 4];
    for _ in 0..3 {
        let temp = tempfile::tempdir().unwrap();
        let store = PackageStore::open(&temp.path().join("cache")).unwrap();
        let packages = contend(&store, &digest, &identities);
        let committed = store.load_verified(&digest).unwrap();
        assert_material(&committed, identities[0].0, identities[0].1);
        for package in packages {
            assert_material(&package, identities[0].0, identities[0].1);
            assert_eq!(package.commit, committed.commit);
            assert_eq!(package.blob, committed.blob);
            assert_eq!(package.tree, committed.tree);
            assert_eq!(package.evidence, committed.evidence);
        }
        assert_eq!(store.evidence_versions(&digest).unwrap().len(), 1);
        assert_eq!(
            fs::read_dir(store.evidence_archive_path(&digest))
                .unwrap()
                .count(),
            1
        );
    }
}

#[test]
fn concurrent_evidence_versions_share_content_without_crossing_identities() {
    let digest = sha256_hex(ARCHIVE);
    let a = (b"index-a".as_slice(), b"verification-a".as_slice());
    let b = (b"index-b".as_slice(), b"verification-b".as_slice());
    let identities = [a, b, a, b];
    for _ in 0..3 {
        let temp = tempfile::tempdir().unwrap();
        let store = PackageStore::open(&temp.path().join("cache")).unwrap();
        let packages = contend(&store, &digest, &identities);
        for (package, &(index, verification)) in packages.iter().zip(&identities) {
            assert_material(package, index, verification);
            assert_eq!(package.blob, packages[0].blob);
            assert_eq!(package.tree, packages[0].tree);
            let exact = store
                .load_verified_exact(&digest, &sha256_hex(index), &sha256_hex(verification))
                .unwrap();
            assert_eq!(exact.commit, package.commit);
            assert_material(&exact, index, verification);
            assert_material(
                &store
                    .load_verified_for_index(&digest, &sha256_hex(index))
                    .unwrap(),
                index,
                verification,
            );
        }
        assert_ne!(packages[0].evidence, packages[1].evidence);
        assert_eq!(
            store.load_verified(&digest).unwrap_err().code,
            "cache_evidence_ambiguous"
        );
        for (index, wrong_verification) in [(a.0, b.1), (b.0, a.1)] {
            let error = store
                .load_verified_exact(&digest, &sha256_hex(index), &sha256_hex(wrong_verification))
                .unwrap_err();
            assert!(!error.code.is_empty() && !error.message.is_empty());
        }
        assert_eq!(store.evidence_versions(&digest).unwrap().len(), 2);
        assert_eq!(
            fs::read_dir(store.evidence_archive_path(&digest))
                .unwrap()
                .count(),
            2
        );
    }
}
