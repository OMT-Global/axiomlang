//! Hostile vendor reader markers must retain snapshots until safe reclamation.
use super::*;

const DEAD_PID: u32 = 2_000_000_000;

struct Fixture {
    _temp: tempfile::TempDir,
    store: PackageStore,
    vendor: PathBuf,
    old: VendorSnapshot,
    current: VendorSnapshot,
    digest: String,
    index: String,
    verification: String,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let store = PackageStore::open(&temp.path().join("cache")).unwrap();
        let bytes = b"AXIOM_PACKAGE_ARCHIVE_V1\n--- file a 1 ---\nx\n";
        let digest = sha256_hex(bytes);
        store
            .admit(VerifiedArtifacts {
                archive_sha256: &digest,
                archive: bytes,
                manifest: b"manifest",
                provenance: b"provenance",
                signature: b"signature",
                registry_index: b"registry-index",
                verification: b"verification",
            })
            .unwrap();
        let index = sha256_hex(b"registry-index");
        let verification = sha256_hex(b"verification");
        let vendor = temp.path().join("vendor");
        let package = |package_id| VendorPackage {
            package_id,
            archive_sha256: &digest,
            registry_index_sha256: &index,
            verification_sha256: &verification,
        };
        let old = store
            .vendor_snapshot(&vendor, &[package("registry:demo/old@1.0.0")])
            .unwrap();
        let lease = PackageStore::lease_vendor_snapshot(&vendor, &old).unwrap();
        let current = store
            .vendor_snapshot(&vendor, &[package("registry:demo/current@1.0.0")])
            .unwrap();
        assert_eq!(current.lifecycle.deferred, 1);
        drop(lease);
        Self {
            _temp: temp,
            store,
            vendor,
            old,
            current,
            digest,
            index,
            verification,
        }
    }

    fn markers(&self) -> PathBuf {
        self.vendor.join(VENDOR_LEASES_DIR).join(&self.old.digest)
    }

    fn refresh(&self) -> VendorSnapshot {
        let expected = [VendorPackage {
            package_id: "registry:demo/current@1.0.0",
            archive_sha256: &self.digest,
            registry_index_sha256: &self.index,
            verification_sha256: &self.verification,
        }];
        let snapshot = self.store.vendor_snapshot(&self.vendor, &expected).unwrap();
        assert_eq!(snapshot.digest, self.current.digest);
        PackageStore::verify_vendor_snapshot(&self.vendor, &expected).unwrap();
        snapshot
    }

    fn retained(&self, reason: &str) {
        let snapshot = self.refresh();
        assert_eq!(snapshot.lifecycle.reclaimed, 0);
        assert_eq!(snapshot.lifecycle.deferred, 1);
        assert_eq!(snapshot.lifecycle.deferred_reasons.get(reason), Some(&1));
        let tree = self.old.package_tree("registry:demo/old@1.0.0").unwrap();
        assert_eq!(fs::read(tree.join("a")).unwrap(), b"x");
    }

    fn reclaimed(&self) {
        assert_eq!(self.refresh().lifecycle.reclaimed, 1);
        assert!(!self.old.root.exists());
    }
}

#[test]
fn corrupt_marker_bytes_retain_snapshot_until_removed() {
    let valid = vendor_lease_marker_bytes(DEAD_PID, 0, 0);
    for bytes in [
        vec![],
        valid[..valid.len() - 1].to_vec(),
        vec![0xff],
        vendor_lease_marker_bytes(DEAD_PID, 0, 1),
        vec![b'x'; 257],
    ] {
        let fixture = Fixture::new();
        let marker = fixture.markers().join(format!("{DEAD_PID}-0-0"));
        fs::write(&marker, &bytes).unwrap();
        fixture.retained("unreadable_reader_lease");
        assert_eq!(fs::read(&marker).unwrap(), bytes);
        fs::remove_file(marker).unwrap();
        fixture.reclaimed();
    }
}

#[test]
fn malformed_marker_names_retain_snapshot_until_removed() {
    for name in [
        "unknown",
        "4294967296-0-0",
        "1-340282366920938463463374607431768211456-0",
        "1-0-18446744073709551616",
    ] {
        let fixture = Fixture::new();
        let marker = fixture.markers().join(name);
        let bytes = vendor_lease_marker_bytes(DEAD_PID, 0, 0);
        fs::write(&marker, &bytes).unwrap();
        fixture.retained("unreadable_reader_lease");
        assert_eq!(fs::read(&marker).unwrap(), bytes);
        fs::remove_file(marker).unwrap();
        fixture.reclaimed();
    }
}

#[test]
fn old_live_and_future_markers_retain_snapshot_until_removed() {
    for (pid, created) in [(std::process::id(), 0), (DEAD_PID, u128::MAX)] {
        let fixture = Fixture::new();
        let marker = fixture.markers().join(format!("{pid}-{created}-0"));
        let bytes = vendor_lease_marker_bytes(pid, created, 0);
        fs::write(&marker, &bytes).unwrap();
        fixture.retained("active_reader");
        assert_eq!(fs::read(&marker).unwrap(), bytes);
        fs::remove_file(marker).unwrap();
        fixture.reclaimed();
    }
}

#[cfg(unix)]
#[test]
fn symlink_marker_preserves_external_target_and_snapshot() {
    use std::os::unix::fs::symlink;
    assert!(!process_is_alive(DEAD_PID));
    let fixture = Fixture::new();
    let external = fixture._temp.path().join("external-marker");
    let bytes = vendor_lease_marker_bytes(DEAD_PID, 0, 0);
    fs::write(&external, &bytes).unwrap();
    let marker = fixture.markers().join(format!("{DEAD_PID}-0-0"));
    symlink(&external, &marker).unwrap();
    fixture.retained("unreadable_reader_lease");
    assert_eq!(fs::read_link(&marker).unwrap(), external);
    assert_eq!(fs::read(&external).unwrap(), bytes);
    fs::remove_file(marker).unwrap();
    fixture.reclaimed();
    assert_eq!(fs::read(external).unwrap(), bytes);
}

#[cfg(unix)]
#[test]
fn valid_stale_dead_marker_is_removed_and_snapshot_reclaimed() {
    assert!(!process_is_alive(DEAD_PID));
    let fixture = Fixture::new();
    let marker = fixture.markers().join(format!("{DEAD_PID}-0-0"));
    fs::write(&marker, vendor_lease_marker_bytes(DEAD_PID, 0, 0)).unwrap();
    fixture.reclaimed();
    assert!(!marker.exists());
}
