//! Strict parser and filesystem materializer for `AXIOM_PACKAGE_ARCHIVE_V1`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

pub const ARCHIVE_MAGIC: &[u8] = b"AXIOM_PACKAGE_ARCHIVE_V1\n";
pub const EXTRACTOR_VERSION: &str = "axiom-package-extractor-v1";
pub const TREE_INTEGRITY_SCHEMA: &str = "axiom.package_tree_integrity.v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveLimits {
    pub max_archive_bytes: usize,
    pub max_file_bytes: usize,
    pub max_files: usize,
    pub max_directories: usize,
    pub max_tree_entries: usize,
    pub max_path_bytes: usize,
    pub max_path_components: usize,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_archive_bytes: 64 * 1024 * 1024,
            max_file_bytes: 16 * 1024 * 1024,
            max_files: 4_096,
            max_directories: 4_096,
            max_tree_entries: 8_192,
            max_path_bytes: 1_024,
            max_path_components: 64,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveError {
    pub code: &'static str,
    pub message: String,
}

impl ArchiveError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ArchiveError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub path: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedArchive {
    pub archive_sha256: String,
    pub entries: Vec<ArchiveEntry>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TreeFileRecord {
    pub path: String,
    pub length: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TreeIntegrityManifest {
    pub schema_version: String,
    pub extractor_version: String,
    pub archive_sha256: String,
    pub files: Vec<TreeFileRecord>,
}

pub fn parse_archive(
    bytes: &[u8],
    expected_sha256: &str,
    limits: ArchiveLimits,
) -> Result<ParsedArchive, ArchiveError> {
    validate_sha256(expected_sha256, "expected archive digest")?;
    if bytes.len() > limits.max_archive_bytes {
        return Err(ArchiveError::new(
            "archive_too_large",
            format!(
                "archive is {} bytes; limit is {}",
                bytes.len(),
                limits.max_archive_bytes
            ),
        ));
    }
    let archive_sha256 = sha256_hex(bytes);
    if archive_sha256 != expected_sha256 {
        return Err(ArchiveError::new(
            "archive_digest_mismatch",
            format!("expected {expected_sha256}, computed {archive_sha256}"),
        ));
    }
    if !bytes.starts_with(ARCHIVE_MAGIC) {
        return Err(ArchiveError::new(
            "archive_magic_invalid",
            "archive does not begin with AXIOM_PACKAGE_ARCHIVE_V1",
        ));
    }

    let mut cursor = ARCHIVE_MAGIC.len();
    let mut extracted_bytes = 0usize;
    let mut entries = Vec::new();
    let mut paths = BTreeSet::new();
    let mut directories = BTreeSet::new();
    let mut previous_path: Option<String> = None;
    while cursor < bytes.len() {
        if entries.len() == limits.max_files {
            return Err(ArchiveError::new(
                "archive_file_count_exceeded",
                format!("archive contains more than {} files", limits.max_files),
            ));
        }
        let remaining = &bytes[cursor..];
        let newline = remaining
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or_else(|| {
                ArchiveError::new("archive_header_truncated", "file header has no newline")
            })?;
        if newline > limits.max_path_bytes.saturating_add(64) {
            return Err(ArchiveError::new(
                "archive_header_too_large",
                "file header exceeds its deterministic bound",
            ));
        }
        let header = std::str::from_utf8(&remaining[..newline]).map_err(|_| {
            ArchiveError::new("archive_header_invalid", "file header is not valid UTF-8")
        })?;
        let body = header
            .strip_prefix("--- file ")
            .and_then(|header| header.strip_suffix(" ---"))
            .ok_or_else(|| {
                ArchiveError::new(
                    "archive_header_invalid",
                    format!("malformed file header at byte {cursor}"),
                )
            })?;
        let (path, length_text) = body.rsplit_once(' ').ok_or_else(|| {
            ArchiveError::new(
                "archive_header_invalid",
                "file header must contain a path and byte length",
            )
        })?;
        validate_archive_path(path, limits)?;
        if length_text.is_empty()
            || (length_text.len() > 1 && length_text.starts_with('0'))
            || !length_text.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(ArchiveError::new(
                "archive_length_invalid",
                format!("non-canonical byte length for {path:?}"),
            ));
        }
        let length = length_text.parse::<usize>().map_err(|_| {
            ArchiveError::new(
                "archive_length_invalid",
                format!("byte length for {path:?} does not fit usize"),
            )
        })?;
        if length > limits.max_file_bytes {
            return Err(ArchiveError::new(
                "archive_file_too_large",
                format!(
                    "{path:?} is {length} bytes; per-file limit is {}",
                    limits.max_file_bytes
                ),
            ));
        }
        extracted_bytes = extracted_bytes.checked_add(length).ok_or_else(|| {
            ArchiveError::new("archive_size_overflow", "extracted byte count overflowed")
        })?;
        if extracted_bytes > limits.max_archive_bytes {
            return Err(ArchiveError::new(
                "archive_total_bytes_exceeded",
                "sum of extracted file bytes exceeds the archive limit",
            ));
        }
        if !paths.insert(path.to_owned()) {
            return Err(ArchiveError::new(
                "archive_duplicate_path",
                format!("duplicate archive path {path:?}"),
            ));
        }
        if previous_path
            .as_deref()
            .is_some_and(|previous| previous >= path)
        {
            return Err(ArchiveError::new(
                "archive_path_order_invalid",
                "archive paths must be strictly increasing",
            ));
        }
        reject_file_directory_collision(path, &paths)?;
        let mut prefix = String::new();
        for component in path
            .split('/')
            .take(path.split('/').count().saturating_sub(1))
        {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            directories.insert(prefix.clone());
        }
        if directories.len() > limits.max_directories
            || paths.len().saturating_add(directories.len()) > limits.max_tree_entries
        {
            return Err(ArchiveError::new(
                "archive_tree_entries_exceeded",
                "archive implies more filesystem entries than the extraction budget",
            ));
        }

        cursor = cursor
            .checked_add(newline + 1)
            .ok_or_else(|| ArchiveError::new("archive_size_overflow", "cursor overflowed"))?;
        let end = cursor
            .checked_add(length)
            .ok_or_else(|| ArchiveError::new("archive_size_overflow", "file extent overflowed"))?;
        if end > bytes.len() {
            return Err(ArchiveError::new(
                "archive_content_truncated",
                format!("{path:?} declares {length} bytes beyond end of archive"),
            ));
        }
        let content = bytes[cursor..end].to_vec();
        cursor = end;
        if !content.ends_with(b"\n") {
            if bytes.get(cursor) != Some(&b'\n') {
                return Err(ArchiveError::new(
                    "archive_separator_missing",
                    format!("{path:?} is not followed by the canonical newline separator"),
                ));
            }
            cursor += 1;
        }
        previous_path = Some(path.to_owned());
        entries.push(ArchiveEntry {
            path: path.to_owned(),
            bytes: content,
        });
    }

    Ok(ParsedArchive {
        archive_sha256,
        entries,
    })
}

pub fn extract_archive(
    archive: &ParsedArchive,
    destination: &Path,
) -> Result<TreeIntegrityManifest, ArchiveError> {
    let manifest = expected_tree_integrity(archive)?;
    fs::create_dir(destination).map_err(|error| {
        ArchiveError::new(
            "extraction_root_create_failed",
            format!("failed to create {}: {error}", destination.display()),
        )
    })?;
    let result = (|| {
        sync_directory(destination)?;
        sync_parent_directory(destination)?;
        let mut directories_to_sync = BTreeSet::from([destination.to_path_buf()]);
        let mut previous: Option<&str> = None;
        for entry in &archive.entries {
            validate_archive_path(&entry.path, ArchiveLimits::default())?;
            if previous.is_some_and(|path| path >= entry.path.as_str()) {
                return Err(ArchiveError::new(
                    "archive_path_order_invalid",
                    "parsed archive entries must remain strictly ordered",
                ));
            }
            let target = safe_join(destination, &entry.path)?;
            let parent = target.parent().ok_or_else(|| {
                ArchiveError::new("archive_path_invalid", "archive file has no parent")
            })?;
            ensure_directories(destination, parent, &mut directories_to_sync)?;
            let mut file = open_new_nofollow(&target)?;
            file.write_all(&entry.bytes).map_err(|error| {
                ArchiveError::new(
                    "extraction_write_failed",
                    format!("failed to write {}: {error}", target.display()),
                )
            })?;
            file.sync_all().map_err(|error| {
                ArchiveError::new(
                    "extraction_sync_failed",
                    format!("failed to sync {}: {error}", target.display()),
                )
            })?;
            sync_directory(parent)?;
            previous = Some(&entry.path);
        }
        let mut directories = directories_to_sync.into_iter().collect::<Vec<_>>();
        directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
        for directory in directories {
            sync_directory(&directory)?;
        }
        sync_parent_directory(destination)?;
        verify_tree(destination, &manifest)?;
        Ok(manifest)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result
}

pub fn expected_tree_integrity(
    archive: &ParsedArchive,
) -> Result<TreeIntegrityManifest, ArchiveError> {
    validate_sha256(&archive.archive_sha256, "archive digest")?;
    let limits = ArchiveLimits::default();
    if archive.entries.len() > limits.max_files {
        return Err(ArchiveError::new(
            "archive_file_count_exceeded",
            "parsed archive exceeds the extraction file-count limit",
        ));
    }
    let mut files = Vec::with_capacity(archive.entries.len());
    let mut previous: Option<&str> = None;
    for entry in &archive.entries {
        validate_archive_path(&entry.path, ArchiveLimits::default())?;
        if previous.is_some_and(|path| path >= entry.path.as_str()) {
            return Err(ArchiveError::new(
                "archive_path_order_invalid",
                "parsed archive entries must remain strictly ordered",
            ));
        }
        files.push(TreeFileRecord {
            path: entry.path.clone(),
            length: entry.bytes.len() as u64,
            sha256: sha256_hex(&entry.bytes),
        });
        previous = Some(&entry.path);
    }
    let manifest = TreeIntegrityManifest {
        schema_version: TREE_INTEGRITY_SCHEMA.to_owned(),
        extractor_version: EXTRACTOR_VERSION.to_owned(),
        archive_sha256: archive.archive_sha256.clone(),
        files,
    };
    validate_integrity_manifest(&manifest)?;
    Ok(manifest)
}

pub fn verify_tree(root: &Path, manifest: &TreeIntegrityManifest) -> Result<(), ArchiveError> {
    verify_tree_with_limits(root, manifest, ArchiveLimits::default())
}

pub fn verify_tree_with_limits(
    root: &Path,
    manifest: &TreeIntegrityManifest,
    limits: ArchiveLimits,
) -> Result<(), ArchiveError> {
    validate_integrity_manifest(manifest)?;
    let metadata = fs::symlink_metadata(root).map_err(|error| {
        ArchiveError::new(
            "tree_unavailable",
            format!("failed to stat {}: {error}", root.display()),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ArchiveError::new(
            "tree_root_invalid",
            "tree root must be a non-symlink directory",
        ));
    }
    let mut scan = TreeScan {
        files: Vec::with_capacity(manifest.files.len()),
        total_bytes: 0,
        entries: 0,
        directories: 0,
    };
    collect_tree_files(root, root, 0, limits, &mut scan)?;
    let mut actual = scan.files;
    actual.sort_by(|left, right| left.path.cmp(&right.path));
    if actual != manifest.files {
        return Err(ArchiveError::new(
            "tree_integrity_mismatch",
            "tree paths, lengths, or file digests differ from integrity manifest",
        ));
    }
    Ok(())
}

pub fn integrity_manifest_bytes(manifest: &TreeIntegrityManifest) -> Result<Vec<u8>, ArchiveError> {
    validate_integrity_manifest(manifest)?;
    let mut bytes = serde_json::to_vec(manifest).map_err(|error| {
        ArchiveError::new(
            "integrity_manifest_invalid",
            format!("failed to render integrity manifest: {error}"),
        )
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn integrity_manifest_sha256(manifest: &TreeIntegrityManifest) -> Result<String, ArchiveError> {
    Ok(sha256_hex(&integrity_manifest_bytes(manifest)?))
}

pub fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, ArchiveError> {
    validate_archive_path(relative, ArchiveLimits::default())?;
    let mut joined = root.to_path_buf();
    for component in relative.split('/') {
        joined.push(component);
    }
    Ok(joined)
}

fn validate_integrity_manifest(manifest: &TreeIntegrityManifest) -> Result<(), ArchiveError> {
    if manifest.schema_version != TREE_INTEGRITY_SCHEMA
        || manifest.extractor_version != EXTRACTOR_VERSION
    {
        return Err(ArchiveError::new(
            "integrity_manifest_version_invalid",
            "tree integrity schema or extractor version is unsupported",
        ));
    }
    validate_sha256(&manifest.archive_sha256, "integrity archive digest")?;
    let limits = ArchiveLimits::default();
    if manifest.files.len() > limits.max_files {
        return Err(ArchiveError::new(
            "integrity_file_count_exceeded",
            "integrity manifest exceeds file-count limit",
        ));
    }
    let mut previous: Option<&str> = None;
    let mut total_bytes = 0u64;
    let mut directories = BTreeSet::new();
    for file in &manifest.files {
        validate_archive_path(&file.path, limits)?;
        validate_sha256(&file.sha256, "tree file digest")?;
        if file.length > limits.max_file_bytes as u64 {
            return Err(ArchiveError::new(
                "integrity_file_too_large",
                format!("integrity entry {:?} exceeds file limit", file.path),
            ));
        }
        total_bytes = total_bytes.checked_add(file.length).ok_or_else(|| {
            ArchiveError::new(
                "integrity_total_bytes_exceeded",
                "integrity file lengths overflowed",
            )
        })?;
        if total_bytes > limits.max_archive_bytes as u64 {
            return Err(ArchiveError::new(
                "integrity_total_bytes_exceeded",
                "integrity file lengths exceed the total tree limit",
            ));
        }
        if previous.is_some_and(|path| path >= file.path.as_str()) {
            return Err(ArchiveError::new(
                "integrity_path_order_invalid",
                "integrity file paths must be strictly increasing",
            ));
        }
        let components = file.path.split('/').collect::<Vec<_>>();
        let mut prefix = String::new();
        for component in components.iter().take(components.len().saturating_sub(1)) {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            directories.insert(prefix.clone());
            if directories.len() > limits.max_directories
                || manifest.files.len().saturating_add(directories.len()) > limits.max_tree_entries
            {
                return Err(ArchiveError::new(
                    "integrity_tree_entries_exceeded",
                    "integrity manifest implies too many filesystem entries",
                ));
            }
        }
        previous = Some(&file.path);
    }
    Ok(())
}

fn validate_archive_path(path: &str, limits: ArchiveLimits) -> Result<(), ArchiveError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains('\\')
        || path.contains('\0')
    {
        return Err(ArchiveError::new(
            "archive_path_invalid",
            format!("unsafe archive path {path:?}"),
        ));
    }
    if path.as_bytes().len() > limits.max_path_bytes {
        return Err(ArchiveError::new(
            "archive_path_too_long",
            format!("archive path exceeds {} bytes", limits.max_path_bytes),
        ));
    }
    if path.nfc().collect::<String>() != path {
        return Err(ArchiveError::new(
            "archive_path_noncanonical",
            format!("archive path is not NFC-normalized: {path:?}"),
        ));
    }
    let components = path.split('/').collect::<Vec<_>>();
    if components.len() > limits.max_path_components {
        return Err(ArchiveError::new(
            "archive_path_too_deep",
            format!(
                "archive path exceeds {} components",
                limits.max_path_components
            ),
        ));
    }
    if components.iter().any(|component| {
        component.is_empty()
            || *component == "."
            || *component == ".."
            || component.contains(':')
            || component.chars().any(char::is_control)
    }) {
        return Err(ArchiveError::new(
            "archive_path_noncanonical",
            format!("archive path has an unsafe component: {path:?}"),
        ));
    }
    Ok(())
}

fn reject_file_directory_collision(
    path: &str,
    paths: &BTreeSet<String>,
) -> Result<(), ArchiveError> {
    let mut prefix = String::new();
    for component in path
        .split('/')
        .take(path.split('/').count().saturating_sub(1))
    {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(component);
        if paths.contains(&prefix) {
            return Err(ArchiveError::new(
                "archive_path_collision",
                format!("{prefix:?} is both a file and a directory"),
            ));
        }
    }
    if paths
        .range(format!("{path}/")..)
        .next()
        .is_some_and(|candidate| candidate.starts_with(&format!("{path}/")))
    {
        return Err(ArchiveError::new(
            "archive_path_collision",
            format!("{path:?} is both a file and a directory"),
        ));
    }
    Ok(())
}

fn ensure_directories(
    root: &Path,
    parent: &Path,
    directories_to_sync: &mut BTreeSet<PathBuf>,
) -> Result<(), ArchiveError> {
    let relative = parent.strip_prefix(root).map_err(|_| {
        ArchiveError::new(
            "extraction_escape",
            "archive parent escaped extraction root",
        )
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        directories_to_sync.insert(current.clone());
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(ArchiveError::new(
                        "extraction_parent_invalid",
                        format!("{} is not a safe directory", current.display()),
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|error| {
                    ArchiveError::new(
                        "extraction_directory_create_failed",
                        format!("failed to create {}: {error}", current.display()),
                    )
                })?;
                sync_directory(&current)?;
                sync_parent_directory(&current)?;
            }
            Err(error) => {
                return Err(ArchiveError::new(
                    "extraction_parent_unavailable",
                    format!("failed to stat {}: {error}", current.display()),
                ));
            }
        }
    }
    Ok(())
}

fn open_new_nofollow(path: &Path) -> Result<File, ArchiveError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        options.mode(0o600);
    }
    options.open(path).map_err(|error| {
        ArchiveError::new(
            "extraction_file_create_failed",
            format!("failed to create {}: {error}", path.display()),
        )
    })
}

fn open_read_nofollow(path: &Path) -> Result<File, ArchiveError> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            ArchiveError::new(
                "tree_file_unavailable",
                format!("failed to inspect {}: {error}", path.display()),
            )
        })?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(ArchiveError::new(
                "tree_symlink_rejected",
                format!("tree file is a reparse point: {}", path.display()),
            ));
        }
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|error| {
        ArchiveError::new(
            "tree_file_unavailable",
            format!("failed to open {}: {error}", path.display()),
        )
    })?;
    let metadata = file.metadata().map_err(|error| {
        ArchiveError::new(
            "tree_file_unavailable",
            format!("failed to inspect open file {}: {error}", path.display()),
        )
    })?;
    if !metadata.is_file() {
        return Err(ArchiveError::new(
            "tree_entry_type_rejected",
            format!("tree entry is not a regular file: {}", path.display()),
        ));
    }
    Ok(file)
}

struct TreeScan {
    files: Vec<TreeFileRecord>,
    total_bytes: u64,
    entries: usize,
    directories: usize,
}

fn collect_tree_files(
    root: &Path,
    directory: &Path,
    depth: usize,
    limits: ArchiveLimits,
    scan: &mut TreeScan,
) -> Result<(), ArchiveError> {
    if depth > limits.max_path_components {
        return Err(ArchiveError::new(
            "tree_depth_exceeded",
            "tree exceeds maximum path depth",
        ));
    }
    let entries = fs::read_dir(directory).map_err(|error| {
        ArchiveError::new(
            "tree_read_failed",
            format!("failed to read {}: {error}", directory.display()),
        )
    })?;
    for entry in entries {
        let entry =
            entry.map_err(|error| ArchiveError::new("tree_read_failed", error.to_string()))?;
        scan.entries = scan.entries.checked_add(1).ok_or_else(|| {
            ArchiveError::new("tree_entry_count_exceeded", "tree entry count overflowed")
        })?;
        if scan.entries > limits.max_tree_entries {
            return Err(ArchiveError::new(
                "tree_entry_count_exceeded",
                "tree exceeds maximum filesystem entry count",
            ));
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            ArchiveError::new(
                "tree_stat_failed",
                format!("failed to stat {}: {error}", path.display()),
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(ArchiveError::new(
                "tree_symlink_rejected",
                format!("tree contains symlink {}", path.display()),
            ));
        }
        if metadata.is_dir() {
            scan.directories = scan.directories.checked_add(1).ok_or_else(|| {
                ArchiveError::new(
                    "tree_directory_count_exceeded",
                    "tree directory count overflowed",
                )
            })?;
            if scan.directories > limits.max_directories {
                return Err(ArchiveError::new(
                    "tree_directory_count_exceeded",
                    "tree exceeds maximum directory count",
                ));
            }
            collect_tree_files(root, &path, depth + 1, limits, scan)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(ArchiveError::new(
                "tree_entry_type_rejected",
                format!("tree contains non-regular entry {}", path.display()),
            ));
        }
        if metadata.len() > limits.max_file_bytes as u64 {
            return Err(ArchiveError::new(
                "tree_file_too_large",
                format!("tree file exceeds limit: {}", path.display()),
            ));
        }
        if scan.files.len() == limits.max_files {
            return Err(ArchiveError::new(
                "tree_file_count_exceeded",
                "tree exceeds maximum file count",
            ));
        }
        scan.total_bytes = scan
            .total_bytes
            .checked_add(metadata.len())
            .ok_or_else(|| {
                ArchiveError::new("tree_total_bytes_exceeded", "tree byte count overflowed")
            })?;
        if scan.total_bytes > limits.max_archive_bytes as u64 {
            return Err(ArchiveError::new(
                "tree_total_bytes_exceeded",
                "tree exceeds maximum cumulative file bytes",
            ));
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| ArchiveError::new("tree_path_invalid", "tree path escaped root"))?;
        let relative = relative
            .components()
            .map(|component| {
                component.as_os_str().to_str().ok_or_else(|| {
                    ArchiveError::new("tree_path_invalid", "tree path is not valid UTF-8")
                })
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("/");
        validate_archive_path(&relative, limits)?;
        scan.files.push(TreeFileRecord {
            path: relative,
            length: metadata.len(),
            sha256: hash_file(&path, metadata.len())?,
        });
    }
    Ok(())
}

fn hash_file(path: &Path, expected_length: u64) -> Result<String, ArchiveError> {
    let file = open_read_nofollow(path)?;
    let mut hasher = Sha256::new();
    let copied = std::io::copy(
        &mut file.take(expected_length.saturating_add(1)),
        &mut hasher,
    )
    .map_err(|error| {
        ArchiveError::new(
            "tree_file_read_failed",
            format!("failed to read {}: {error}", path.display()),
        )
    })?;
    if copied != expected_length {
        return Err(ArchiveError::new(
            "tree_file_raced",
            format!("{} changed while being verified", path.display()),
        ));
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn sync_directory(path: &Path) -> Result<(), ArchiveError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| {
            ArchiveError::new(
                "extraction_sync_failed",
                format!("failed to sync {}: {error}", path.display()),
            )
        })
}

fn sync_parent_directory(path: &Path) -> Result<(), ArchiveError> {
    let parent = path.parent().ok_or_else(|| {
        ArchiveError::new(
            "extraction_sync_failed",
            format!("{} has no parent directory", path.display()),
        )
    })?;
    sync_directory(parent)
}

fn validate_sha256(value: &str, label: &str) -> Result<(), ArchiveError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ArchiveError::new(
            "sha256_invalid",
            format!("{label} must be 64 lowercase hexadecimal characters"),
        ));
    }
    Ok(())
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut bytes = ARCHIVE_MAGIC.to_vec();
        for (path, content) in entries {
            bytes.extend_from_slice(format!("--- file {path} {} ---\n", content.len()).as_bytes());
            bytes.extend_from_slice(content);
            if !content.ends_with(b"\n") {
                bytes.push(b'\n');
            }
        }
        bytes
    }

    fn parse(bytes: &[u8]) -> Result<ParsedArchive, ArchiveError> {
        parse_archive(bytes, &sha256_hex(bytes), ArchiveLimits::default())
    }

    #[test]
    fn parses_extracts_and_reverifies_deterministic_archive() {
        let bytes = archive(&[
            ("axiom.toml", b"[package]\nname = \"demo\"\n"),
            ("src/main.ax", b"fn main() {}"),
        ]);
        let parsed = parse(&bytes).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let tree = temp.path().join("tree");
        let manifest = extract_archive(&parsed, &tree).unwrap();
        verify_tree(&tree, &manifest).unwrap();
        assert_eq!(fs::read(tree.join("src/main.ax")).unwrap(), b"fn main() {}");
        assert_eq!(manifest.files.len(), 2);
        assert_eq!(
            integrity_manifest_bytes(&manifest).unwrap(),
            integrity_manifest_bytes(&manifest).unwrap()
        );
    }

    #[test]
    fn rejects_unsafe_duplicate_and_noncanonical_paths() {
        for path in [
            "../escape",
            "/absolute",
            "a\\b",
            "a\0b",
            "a//b",
            "a/./b",
            "C:/absolute",
            "e\u{301}.txt",
        ] {
            let bytes = archive(&[(path, b"x")]);
            assert!(parse(&bytes).is_err(), "{path:?}");
        }
        let duplicate = archive(&[("a", b"x"), ("a", b"y")]);
        assert_eq!(
            parse(&duplicate).unwrap_err().code,
            "archive_duplicate_path"
        );
        let collision = archive(&[("a", b"x"), ("a/b", b"y")]);
        assert_eq!(
            parse(&collision).unwrap_err().code,
            "archive_path_collision"
        );
        let unsorted = archive(&[("b", b"x"), ("a", b"y")]);
        assert_eq!(
            parse(&unsorted).unwrap_err().code,
            "archive_path_order_invalid"
        );
    }

    #[test]
    fn rejects_malformed_lengths_truncation_trailing_garbage_and_bounds() {
        for bytes in [
            b"AXIOM_PACKAGE_ARCHIVE_V1\n--- file a 01 ---\nx\n".to_vec(),
            b"AXIOM_PACKAGE_ARCHIVE_V1\n--- file a 3 ---\nx\n".to_vec(),
            b"AXIOM_PACKAGE_ARCHIVE_V1\ntrailing".to_vec(),
        ] {
            assert!(parse(&bytes).is_err());
        }
        let bytes = archive(&[("a", b"xx")]);
        let limits = ArchiveLimits {
            max_file_bytes: 1,
            ..ArchiveLimits::default()
        };
        assert_eq!(
            parse_archive(&bytes, &sha256_hex(&bytes), limits)
                .unwrap_err()
                .code,
            "archive_file_too_large"
        );

        let nested = archive(&[("a/b", b"x")]);
        let limits = ArchiveLimits {
            max_directories: 0,
            ..ArchiveLimits::default()
        };
        assert_eq!(
            parse_archive(&nested, &sha256_hex(&nested), limits)
                .unwrap_err()
                .code,
            "archive_tree_entries_exceeded"
        );
    }

    #[test]
    fn digest_mismatch_is_rejected_before_extraction() {
        let bytes = archive(&[("a", b"x")]);
        assert_eq!(
            parse_archive(&bytes, &"0".repeat(64), ArchiveLimits::default())
                .unwrap_err()
                .code,
            "archive_digest_mismatch"
        );
    }

    #[test]
    fn offline_verification_rejects_tampering_and_extra_files() {
        let bytes = archive(&[("a", b"x")]);
        let parsed = parse(&bytes).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let tree = temp.path().join("tree");
        let manifest = extract_archive(&parsed, &tree).unwrap();
        fs::write(tree.join("a"), b"tampered").unwrap();
        assert_eq!(
            verify_tree(&tree, &manifest).unwrap_err().code,
            "tree_integrity_mismatch"
        );
        fs::write(tree.join("a"), b"x").unwrap();
        fs::write(tree.join("extra"), b"x").unwrap();
        assert_eq!(
            verify_tree(&tree, &manifest).unwrap_err().code,
            "tree_integrity_mismatch"
        );
    }

    #[test]
    fn offline_verification_enforces_aggregate_tree_resource_limits() {
        let bytes = archive(&[("a", b"x"), ("dir/b", b"y")]);
        let parsed = parse(&bytes).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let tree = temp.path().join("tree");
        let manifest = extract_archive(&parsed, &tree).unwrap();

        let byte_limits = ArchiveLimits {
            max_archive_bytes: 1,
            ..ArchiveLimits::default()
        };
        assert_eq!(
            verify_tree_with_limits(&tree, &manifest, byte_limits)
                .unwrap_err()
                .code,
            "tree_total_bytes_exceeded"
        );

        let entry_limits = ArchiveLimits {
            max_tree_entries: 2,
            ..ArchiveLimits::default()
        };
        assert_eq!(
            verify_tree_with_limits(&tree, &manifest, entry_limits)
                .unwrap_err()
                .code,
            "tree_entry_count_exceeded"
        );

        let directory_limits = ArchiveLimits {
            max_directories: 0,
            ..ArchiveLimits::default()
        };
        assert_eq!(
            verify_tree_with_limits(&tree, &manifest, directory_limits)
                .unwrap_err()
                .code,
            "tree_directory_count_exceeded"
        );
    }

    #[cfg(unix)]
    #[test]
    fn offline_verification_rejects_symlinks() {
        use std::os::unix::fs::symlink;
        let bytes = archive(&[("a", b"x")]);
        let parsed = parse(&bytes).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let tree = temp.path().join("tree");
        let manifest = extract_archive(&parsed, &tree).unwrap();
        fs::remove_file(tree.join("a")).unwrap();
        symlink("/dev/null", tree.join("a")).unwrap();
        assert_eq!(
            verify_tree(&tree, &manifest).unwrap_err().code,
            "tree_symlink_rejected"
        );
    }

    #[cfg(unix)]
    #[test]
    fn nofollow_reader_rejects_fifo_without_blocking() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let temp = tempfile::tempdir().unwrap();
        let fifo = temp.path().join("tree-fifo");
        let fifo_name = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo_name.as_ptr(), 0o600) }, 0);
        assert_eq!(
            open_read_nofollow(&fifo).unwrap_err().code,
            "tree_entry_type_rejected"
        );
    }
}
