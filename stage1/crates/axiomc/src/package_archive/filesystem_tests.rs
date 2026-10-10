use super::*;

fn fixture() -> ParsedArchive {
    let bytes = b"AXIOM_PACKAGE_ARCHIVE_V1\n--- file dir/a 1 ---\nx\n";
    parse_archive(bytes, &sha256_hex(bytes), ArchiveLimits::default()).unwrap()
}

#[test]
fn extraction_preserves_existing_file_and_directory_destinations() {
    let archive = fixture();
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("file");
    fs::write(&file, b"owner file").unwrap();
    let directory = temp.path().join("directory");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("sentinel"), b"owner directory").unwrap();

    for destination in [&file, &directory] {
        assert_eq!(
            extract_archive(&archive, destination).unwrap_err().code,
            "extraction_root_create_failed"
        );
    }
    assert_eq!(fs::read(&file).unwrap(), b"owner file");
    assert_eq!(
        fs::read(directory.join("sentinel")).unwrap(),
        b"owner directory"
    );
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);

    let fresh = temp.path().join("fresh");
    let manifest = extract_archive(&archive, &fresh).unwrap();
    verify_tree(&fresh, &manifest).unwrap();
    assert_eq!(fs::read(fresh.join("dir/a")).unwrap(), b"x");
}

#[cfg(unix)]
#[test]
fn extraction_preserves_symlink_destination_and_external_tree() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let outside = temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"external owner data").unwrap();
    let destination = temp.path().join("destination");
    symlink(&outside, &destination).unwrap();
    assert_eq!(
        extract_archive(&fixture(), &destination).unwrap_err().code,
        "extraction_root_create_failed"
    );
    assert_eq!(fs::read_link(&destination).unwrap(), outside);
    assert_eq!(
        fs::read(outside.join("sentinel")).unwrap(),
        b"external owner data"
    );
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn verification_rejects_root_and_intermediate_directory_symlinks() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let tree = temp.path().join("tree");
    let manifest = extract_archive(&fixture(), &tree).unwrap();
    let alias = temp.path().join("alias");
    symlink(&tree, &alias).unwrap();
    assert_eq!(
        verify_tree(&alias, &manifest).unwrap_err().code,
        "tree_root_invalid"
    );
    verify_tree(&tree, &manifest).unwrap();

    let outside = temp.path().join("outside");
    fs::rename(tree.join("dir"), &outside).unwrap();
    symlink(&outside, tree.join("dir")).unwrap();
    // The external bytes exactly match the manifest: rejection must come from
    // the symlink boundary, not from a digest mismatch.
    assert_eq!(
        verify_tree(&tree, &manifest).unwrap_err().code,
        "tree_symlink_rejected"
    );
    assert_eq!(fs::read(outside.join("a")).unwrap(), b"x");
    assert_eq!(fs::read_link(tree.join("dir")).unwrap(), outside);
    fs::remove_file(tree.join("dir")).unwrap();
    fs::rename(&outside, tree.join("dir")).unwrap();
    verify_tree(&tree, &manifest).unwrap();
}

// APFS rejects non-UTF-8 names at creation; exercise this fixture on the
// Linux CI filesystem instead of silently skipping a failed setup on macOS.
#[cfg(target_os = "linux")]
#[test]
fn verification_rejects_invalid_utf8_filesystem_names() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let temp = tempfile::tempdir().unwrap();
    let tree = temp.path().join("tree");
    let manifest = extract_archive(&fixture(), &tree).unwrap();
    let invalid = tree.join(OsString::from_vec(vec![b'b', 0xff]));
    fs::write(&invalid, b"untrusted").unwrap();
    assert_eq!(
        verify_tree(&tree, &manifest).unwrap_err().code,
        "tree_path_invalid"
    );
    assert_eq!(fs::read(&invalid).unwrap(), b"untrusted");
    fs::remove_file(&invalid).unwrap();
    verify_tree(&tree, &manifest).unwrap();
}

#[cfg(unix)]
#[test]
fn verification_rejects_unix_socket_entries() {
    use std::os::unix::net::UnixListener;

    let temp = tempfile::tempdir().unwrap();
    let tree = temp.path().join("tree");
    let manifest = extract_archive(&fixture(), &tree).unwrap();
    let socket = tree.join("socket");
    let listener = UnixListener::bind(&socket).unwrap();
    assert_eq!(
        verify_tree(&tree, &manifest).unwrap_err().code,
        "tree_entry_type_rejected"
    );
    assert!(fs::symlink_metadata(&socket).is_ok());
    drop(listener);
    fs::remove_file(&socket).unwrap();
    verify_tree(&tree, &manifest).unwrap();
}
