//! Corruption controls for persisted tree-integrity manifests (#1660).
use axiomc::package_archive::{
    ArchiveLimits, TreeIntegrityManifest, extract_archive, integrity_manifest_bytes, parse_archive,
    verify_tree,
};
use sha2::{Digest, Sha256};

type Mutation = fn(&mut TreeIntegrityManifest);

#[test]
fn corrupted_integrity_manifests_fail_closed_without_changing_a_valid_tree() {
    let bytes = b"AXIOM_PACKAGE_ARCHIVE_V1\n--- file a 1 ---\nx\n--- file b 1 ---\ny\n";
    let archive = parse_archive(
        bytes,
        &format!("{:x}", Sha256::digest(bytes)),
        ArchiveLimits::default(),
    )
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let tree = temp.path().join("tree");
    let valid = extract_archive(&archive, &tree).unwrap();
    verify_tree(&tree, &valid).unwrap();
    let cases: [(&str, &str, Mutation); 10] = [
        ("schema", "integrity_manifest_version_invalid", |m| {
            m.schema_version.push_str("-unknown");
        }),
        ("extractor", "integrity_manifest_version_invalid", |m| {
            m.extractor_version.push_str("-unknown");
        }),
        ("archive digest", "sha256_invalid", |m| {
            m.archive_sha256 = "0".repeat(63);
        }),
        ("file digest", "sha256_invalid", |m| {
            m.files[0].sha256 = "G".repeat(64);
        }),
        ("traversal", "archive_path_noncanonical", |m| {
            m.files[0].path = "../outside".into();
        }),
        ("duplicate", "integrity_path_order_invalid", |m| {
            m.files[1] = m.files[0].clone();
        }),
        ("order", "integrity_path_order_invalid", |m| {
            m.files.swap(0, 1);
        }),
        ("file length", "integrity_file_too_large", |m| {
            m.files[0].length = ArchiveLimits::default().max_file_bytes as u64 + 1;
        }),
        ("aggregate length", "integrity_total_bytes_exceeded", |m| {
            let file = m.files[0].clone();
            m.files = (0..5)
                .map(|i| {
                    let mut entry = file.clone();
                    entry.path = format!("{i:04}");
                    entry.length = ArchiveLimits::default().max_file_bytes as u64;
                    entry
                })
                .collect();
        }),
        ("file count", "integrity_file_count_exceeded", |m| {
            let file = m.files[0].clone();
            m.files = (0..=ArchiveLimits::default().max_files)
                .map(|i| {
                    let mut entry = file.clone();
                    entry.path = format!("{i:04}");
                    entry
                })
                .collect();
        }),
    ];
    for (label, expected_code, mutate) in cases {
        let mut corrupt = valid.clone();
        mutate(&mut corrupt);
        let serialized = integrity_manifest_bytes(&corrupt).unwrap_err();
        assert_eq!(serialized.code, expected_code, "serialize: {label}");
        let verified = verify_tree(&tree, &corrupt).unwrap_err();
        assert_eq!(verified.code, expected_code, "verify: {label}");
        // A malformed manifest must be diagnosed before filesystem availability,
        // rather than changing its meaning depending on the cached tree's state.
        let missing = verify_tree(&temp.path().join("absent"), &corrupt).unwrap_err();
        assert_eq!(missing.code, expected_code, "missing tree: {label}");
        assert_eq!(std::fs::read(tree.join("a")).unwrap(), b"x");
        assert_eq!(std::fs::read(tree.join("b")).unwrap(), b"y");
        assert!(!temp.path().join("outside").exists());
        verify_tree(&tree, &valid).unwrap();
    }
}
