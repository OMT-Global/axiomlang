//! Bounded, reproducible byte-mutation campaign for untrusted package archives.
use axiomc::package_archive::{ARCHIVE_MAGIC, ArchiveLimits, parse_archive};
use sha2::{Digest, Sha256};

const SEEDS: &[(&str, &[u8])] = &[
    ("empty", ARCHIVE_MAGIC),
    (
        "package",
        include_bytes!("package_archive_fuzz/package.archive"),
    ),
    (
        "binary",
        b"AXIOM_PACKAGE_ARCHIVE_V1\n--- file a.bin 5 ---\n\0\xff\n\x80\0\n--- file b 0 ---\n\n",
    ),
    (
        "duplicate",
        b"AXIOM_PACKAGE_ARCHIVE_V1\n--- file a 1 ---\nx\n--- file a 1 ---\ny\n",
    ),
    (
        "traversal",
        b"AXIOM_PACKAGE_ARCHIVE_V1\n--- file ../escape 1 ---\nx\n",
    ),
    (
        "huge-length",
        b"AXIOM_PACKAGE_ARCHIVE_V1\n--- file a 184467440737095516160 ---\nx\n",
    ),
    (
        "invalid-utf8",
        b"AXIOM_PACKAGE_ARCHIVE_V1\n--- file \xff 1 ---\nx\n",
    ),
    (
        "truncated",
        b"AXIOM_PACKAGE_ARCHIVE_V1\n--- file src/main.ax 100 ---\nx",
    ),
];

fn exercise(bytes: &[u8], label: &str, limits: ArchiveLimits) -> bool {
    // Rehash every mutation so fuzzing reaches structural parsing rather than
    // stopping at the digest-mismatch guard for every changed input.
    let digest = format!("{:x}", Sha256::digest(bytes));
    let result = std::panic::catch_unwind(|| parse_archive(bytes, &digest, limits))
        .unwrap_or_else(|_| panic!("archive parser panicked: {label}; bytes={bytes:?}"));
    match result {
        Err(error) => {
            assert!(!error.code.is_empty(), "{label}: error code missing");
            assert!(!error.message.is_empty(), "{label}: error detail missing");
            false
        }
        Ok(parsed) => {
            // A successful parse must round-trip to the exact canonical wire
            // representation, including arbitrary binary payloads and separators.
            let mut encoded = ARCHIVE_MAGIC.to_vec();
            for entry in &parsed.entries {
                encoded.extend_from_slice(
                    format!("--- file {} {} ---\n", entry.path, entry.bytes.len()).as_bytes(),
                );
                encoded.extend_from_slice(&entry.bytes);
                if !entry.bytes.ends_with(b"\n") {
                    encoded.push(b'\n');
                }
            }
            assert_eq!(encoded, bytes, "{label}: accepted noncanonical bytes");
            assert_eq!(parsed.archive_sha256, digest, "{label}: digest drift");
            true
        }
    }
}

#[test]
fn bounded_archive_mutations_do_not_panic_and_preserve_canonical_roundtrips() {
    let limits = ArchiveLimits {
        max_archive_bytes: 4096,
        max_file_bytes: 1024,
        max_files: 16,
        max_directories: 16,
        max_tree_entries: 32,
        max_path_bytes: 128,
        max_path_components: 8,
    };
    let mut accepted = 0;
    let mut rejected = 0;
    for &(name, seed) in SEEDS {
        let mut check = |bytes: &[u8], mutation: String| {
            if exercise(bytes, &format!("{name}/{mutation}"), limits) {
                accepted += 1;
            } else {
                rejected += 1;
            }
        };
        check(seed, "seed".into());
        // Every truncation and single-bit change, including inside binary data.
        for end in 0..seed.len() {
            check(&seed[..end], format!("truncate-{end}"));
        }
        for offset in 0..seed.len() {
            for bit in 0..8 {
                let mut bytes = seed.to_vec();
                bytes[offset] ^= 1 << bit;
                check(&bytes, format!("flip-{offset}-{bit}"));
            }
        }
        // Insert framing/path delimiters and invalid encoding at every boundary.
        for offset in 0..=seed.len() {
            for byte in [0, b'\n', b'/', b' ', 0xff] {
                let mut bytes = seed.to_vec();
                bytes.insert(offset, byte);
                check(&bytes, format!("insert-{offset}-{byte}"));
            }
        }
        // Exercise resource-limit rejection on the original corpus as well.
        assert!(
            !exercise(
                seed,
                &format!("{name}/zero-budget"),
                ArchiveLimits {
                    max_archive_bytes: 0,
                    ..limits
                },
            ),
            "{name}: a zero-byte archive budget must reject every nonempty seed"
        );
    }
    assert!(
        accepted > 0,
        "campaign must reach successful structural parsing"
    );
    assert!(rejected > 0, "campaign must reach structured rejection");
    eprintln!("archive mutation campaign: {accepted} accepted, {rejected} rejected");
}

#[test]
fn archive_seed_dispositions_are_explicit() {
    for (index, &(name, seed)) in SEEDS.iter().enumerate() {
        assert_eq!(
            exercise(seed, name, ArchiveLimits::default()),
            index < 3,
            "{name}"
        );
    }
}
