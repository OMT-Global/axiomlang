use crate::diagnostics::Diagnostic;
use crate::manifest::{
    DependencySpec, Manifest, is_supported_registry_index_url, load_manifest, lockfile_path,
    manifest_path,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const LOCKFILE_V1_VERSION: u32 = 1;
pub const LOCKFILE_V2_VERSION: u32 = 2;
pub const REGISTRY_LOCKFILE_V2_REQUIRED: &str = "registry dependency graphs require axiom.lock version 2 under --locked; regenerate it with `axiomc pkg update`";
const MAX_LOCKFILE_BYTES: usize = 4 * 1024 * 1024;
static LOCKFILE_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    pub version: u32,
    pub package: Vec<LockedPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LockedPackage {
    pub name: String,
    pub version: String,
    pub source: String,
}

/// A strictly parsed lockfile without erasing its version-specific contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedLockfile {
    V1(Lockfile),
    V2(LockfileV2),
}

impl ParsedLockfile {
    pub fn version(&self) -> u32 {
        match self {
            Self::V1(lockfile) => lockfile.version,
            Self::V2(lockfile) => lockfile.version,
        }
    }

    pub fn as_v1(&self) -> Option<&Lockfile> {
        match self {
            Self::V1(lockfile) => Some(lockfile),
            Self::V2(_) => None,
        }
    }

    pub fn as_v2(&self) -> Option<&LockfileV2> {
        match self {
            Self::V1(_) => None,
            Self::V2(lockfile) => Some(lockfile),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LockfileV2 {
    pub version: u32,
    pub compatibility: LockedCompatibilityEvidence,
    /// Entry packages whose dependency closures form the complete graph.
    pub roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub registry: Vec<LockedRegistryV2>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub package: Vec<LockedPackageV2>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edge: Vec<LockedDependencyEdgeV2>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LockedCompatibilityEvidence {
    pub contract: String,
    pub compiler: String,
    pub edition_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LockedRegistryV2 {
    pub name: String,
    pub source: String,
    pub registry_identity: String,
    pub source_identity: String,
    /// SHA-256 of the exact trust-roots envelope bytes.
    pub trust_roots_sha256: String,
    /// SHA-256 of the exact verification-expectation bytes.
    pub expectation_sha256: String,
    /// Pins for the current candidate root authenticated from that envelope.
    pub current_root_version: u64,
    pub current_root_sequence: u64,
    pub current_root_transcript_sha256: String,
    pub index_sha256: String,
    pub index_transcript_sha256: String,
    pub index_generation: u64,
    pub index_sequence: u64,
    pub index_snapshot_id: String,
    pub index_signer_key_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LockedPackageV2 {
    pub id: String,
    pub name: String,
    pub version: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive_length: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_signature_sha256: Option<String>,
    /// SHA-256 of the exact Package Trust verification-result bytes.
    ///
    /// Together with the registry index digest this selects one immutable
    /// evidence version for the content-addressed archive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signer_key_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yanked_at_resolution: Option<bool>,
    pub compatibility: LockedCompatibilityEvidence,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum LockedDependencySourceKind {
    Path,
    Registry,
}

impl LockedDependencySourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Registry => "registry",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum LockedDependencyReason {
    RootPathConstraint,
    TransitivePathConstraint,
    HighestCompatible,
    ExactLockedReplay,
    TrustedYankedLockedReplay,
}

impl LockedDependencyReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RootPathConstraint => "root_path_constraint",
            Self::TransitivePathConstraint => "transitive_path_constraint",
            Self::HighestCompatible => "highest_compatible",
            Self::ExactLockedReplay => "exact_locked_replay",
            Self::TrustedYankedLockedReplay => "trusted_yanked_locked_replay",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LockedDependencyEdgeV2 {
    pub from: String,
    pub to: String,
    pub alias: String,
    pub requested: String,
    pub source_kind: LockedDependencySourceKind,
    pub reason: LockedDependencyReason,
}

pub fn parse_lockfile_exact(
    content: &[u8],
    source_path: &Path,
) -> Result<ParsedLockfile, Diagnostic> {
    if content.len() > MAX_LOCKFILE_BYTES {
        return Err(lockfile_error(
            source_path,
            format!(
                "axiom.lock exceeds the {} byte parsing limit",
                MAX_LOCKFILE_BYTES
            ),
        ));
    }
    let content = std::str::from_utf8(content)
        .map_err(|err| lockfile_error(source_path, format!("axiom.lock is not UTF-8: {err}")))?;
    let value: toml::Value = toml::from_str(content)
        .map_err(|err| lockfile_error(source_path, format!("invalid axiom.lock: {err}")))?;
    let version = value
        .as_table()
        .and_then(|table| table.get("version"))
        .and_then(toml::Value::as_integer)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| {
            lockfile_error(
                source_path,
                "invalid axiom.lock: version must be an unsigned integer",
            )
        })?;
    match version {
        LOCKFILE_V1_VERSION => {
            let lockfile: Lockfile = toml::from_str(content).map_err(|err| {
                lockfile_error(source_path, format!("invalid axiom.lock v1: {err}"))
            })?;
            validate_lockfile_v1_model(&lockfile, source_path)?;
            Ok(ParsedLockfile::V1(lockfile))
        }
        LOCKFILE_V2_VERSION => {
            let lockfile: LockfileV2 = toml::from_str(content).map_err(|err| {
                lockfile_error(source_path, format!("invalid axiom.lock v2: {err}"))
            })?;
            validate_lockfile_v2_at(&lockfile, source_path)?;
            Ok(ParsedLockfile::V2(lockfile))
        }
        other => Err(lockfile_error(
            source_path,
            format!(
                "unsupported axiom.lock version {other}; supported versions are {LOCKFILE_V1_VERSION} and {LOCKFILE_V2_VERSION}"
            ),
        )),
    }
}

pub fn load_lockfile(project_root: &Path) -> Result<ParsedLockfile, Diagnostic> {
    load_lockfile_with_sha256(project_root).map(|(lockfile, _sha256)| lockfile)
}

/// Securely load, parse, and hash the exact bytes of `axiom.lock`.
///
/// The digest and parsed model are derived from the same bounded descriptor
/// read so callers can carry an exact validated lock identity without racing a
/// second filesystem read.
pub fn load_lockfile_with_sha256(
    project_root: &Path,
) -> Result<(ParsedLockfile, String), Diagnostic> {
    let path = lockfile_path(project_root);
    let content = read_lockfile_bounded(&path)?;
    let sha256 = format!("{:x}", Sha256::digest(&content));
    let lockfile = parse_lockfile_exact(&content, &path)?;
    Ok((lockfile, sha256))
}

#[cfg(unix)]
fn open_lockfile_no_follow(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new()
        .read(true)
        // O_NOFOLLOW closes the final-component symlink race, O_NONBLOCK
        // prevents a swapped FIFO from blocking before descriptor validation,
        // and O_CLOEXEC avoids inheritance into compiler subprocesses.
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
}

#[cfg(windows)]
fn open_lockfile_no_follow(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(not(any(unix, windows)))]
fn open_lockfile_no_follow(_path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "secure axiom.lock loading requires descriptor-level no-follow support",
    ))
}

fn read_lockfile_bounded(path: &Path) -> Result<Vec<u8>, Diagnostic> {
    try_read_lockfile_bounded(path)?.ok_or_else(|| {
        lockfile_error(
            path,
            "failed to securely open axiom.lock: file does not exist",
        )
    })
}

fn try_read_lockfile_bounded(path: &Path) -> Result<Option<Vec<u8>>, Diagnostic> {
    let mut file = match open_lockfile_no_follow(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(lockfile_error(
                path,
                format!("failed to securely open axiom.lock: {err}"),
            ));
        }
    };
    let metadata = file.metadata().map_err(|err| {
        lockfile_error(path, format!("failed to inspect opened axiom.lock: {err}"))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(lockfile_error(
            path,
            "axiom.lock must be a regular non-symlink file",
        ));
    }
    if metadata.len() > MAX_LOCKFILE_BYTES as u64 {
        return Err(lockfile_error(
            path,
            format!(
                "axiom.lock exceeds the {} byte parsing limit",
                MAX_LOCKFILE_BYTES
            ),
        ));
    }

    let mut content = Vec::with_capacity(metadata.len() as usize);
    (&mut file)
        .take(MAX_LOCKFILE_BYTES as u64 + 1)
        .read_to_end(&mut content)
        .map_err(|err| lockfile_error(path, format!("failed to read opened axiom.lock: {err}")))?;
    if content.len() > MAX_LOCKFILE_BYTES {
        return Err(lockfile_error(
            path,
            format!(
                "axiom.lock exceeds the {} byte parsing limit",
                MAX_LOCKFILE_BYTES
            ),
        ));
    }
    Ok(Some(content))
}

fn exact_lockfile_sha256(path: &Path) -> Result<Option<String>, Diagnostic> {
    try_read_lockfile_bounded(path)
        .map(|content| content.map(|content| format!("{:x}", Sha256::digest(content))))
}

pub fn render_lockfile_v2(lockfile: &LockfileV2) -> Result<String, Diagnostic> {
    validate_lockfile_v2(lockfile)?;
    let mut rendered = toml::to_string_pretty(lockfile).map_err(|err| {
        Diagnostic::new("lockfile", format!("failed to render axiom.lock v2: {err}"))
    })?;
    if !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    Ok(rendered)
}

pub fn validate_lockfile_v2(lockfile: &LockfileV2) -> Result<(), Diagnostic> {
    validate_lockfile_v2_at(lockfile, Path::new("axiom.lock"))
}

pub fn write_lockfile_v2_atomic(
    project_root: &Path,
    lockfile: &LockfileV2,
) -> Result<(), Diagnostic> {
    let target = lockfile_path(project_root);
    let expected_sha256 = exact_lockfile_sha256(&target)?;
    write_lockfile_v2_atomic_cas(project_root, lockfile, expected_sha256.as_deref())
}

/// Atomically replace `axiom.lock` only if its exact bytes still match the
/// caller's previously observed state.
///
/// `Some(digest)` requires an existing lockfile with that lowercase SHA-256;
/// `None` requires the lockfile to remain absent.
pub fn write_lockfile_v2_atomic_cas(
    project_root: &Path,
    lockfile: &LockfileV2,
    expected_sha256: Option<&str>,
) -> Result<(), Diagnostic> {
    write_lockfile_v2_atomic_cas_impl(project_root, lockfile, expected_sha256, || Ok(()))
}

fn write_lockfile_v2_atomic_cas_impl(
    project_root: &Path,
    lockfile: &LockfileV2,
    expected_sha256: Option<&str>,
    before_compare: impl FnOnce() -> Result<(), Diagnostic>,
) -> Result<(), Diagnostic> {
    let target = lockfile_path(project_root);
    if let Some(expected_sha256) = expected_sha256 {
        validate_sha256(&target, "expected axiom.lock SHA-256", expected_sha256)?;
    }
    let rendered = render_lockfile_v2(lockfile)?;
    let parent = target.parent().unwrap_or(project_root);
    let mut temporary = None;
    for _ in 0..64 {
        let sequence = LOCKFILE_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".axiom.lock.tmp.{}.{}",
            std::process::id(),
            sequence
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(lockfile_error(
                    &target,
                    format!("failed to create temporary axiom.lock: {err}"),
                ));
            }
        }
    }
    let Some((temporary_path, mut temporary_file)) = temporary else {
        return Err(lockfile_error(
            &target,
            "failed to allocate a unique temporary axiom.lock",
        ));
    };
    let result = (|| {
        temporary_file
            .write_all(rendered.as_bytes())
            .map_err(|err| {
                lockfile_error(
                    &target,
                    format!("failed to write temporary axiom.lock: {err}"),
                )
            })?;
        temporary_file.sync_all().map_err(|err| {
            lockfile_error(
                &target,
                format!("failed to sync temporary axiom.lock: {err}"),
            )
        })?;
        drop(temporary_file);
        before_compare()?;
        let observed_sha256 = exact_lockfile_sha256(&target)?;
        let unchanged = match (expected_sha256, observed_sha256.as_deref()) {
            (None, None) => true,
            (Some(expected), Some(observed)) => expected == observed,
            _ => false,
        };
        if !unchanged {
            return Err(lockfile_error(
                &target,
                "axiom.lock changed while package resolution was in progress; refusing to overwrite it",
            ));
        }
        fs::rename(&temporary_path, &target).map_err(|err| {
            lockfile_error(&target, format!("failed to replace axiom.lock: {err}"))
        })?;
        if let Ok(directory) = fs::File::open(parent) {
            directory.sync_all().map_err(|err| {
                lockfile_error(
                    &target,
                    format!("failed to sync axiom.lock directory: {err}"),
                )
            })?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

pub fn validate_lockfile_version_for_manifest(
    manifest: &Manifest,
    lockfile: &ParsedLockfile,
) -> Result<(), Diagnostic> {
    let has_registry_dependency = manifest
        .dependencies
        .values()
        .any(DependencySpec::is_registry);
    if has_registry_dependency && !matches!(lockfile, ParsedLockfile::V2(_)) {
        return Err(Diagnostic::new("lockfile", REGISTRY_LOCKFILE_V2_REQUIRED));
    }
    if !has_registry_dependency && matches!(lockfile, ParsedLockfile::V2(_)) {
        // A v2 lock remains valid for path-only graphs; this is required for a
        // stable migration and for workspaces that temporarily remove their
        // last registry dependency.
        return Ok(());
    }
    Ok(())
}

pub fn canonical_registry_package_id(
    registry: &str,
    namespace: &str,
    package: &str,
    version: &str,
) -> String {
    format!("registry:{registry}/{namespace}/{package}@{version}")
}

pub fn canonical_path_package_id(source: &str, package: &str, version: &str) -> String {
    let source = source.strip_prefix("path:").unwrap_or(source);
    let source = if source == "path" || source.is_empty() {
        "."
    } else {
        source
    };
    format!("path:{source}#{package}@{version}")
}

fn validate_lockfile_v1_model(lockfile: &Lockfile, path: &Path) -> Result<(), Diagnostic> {
    if lockfile.version != LOCKFILE_V1_VERSION {
        return Err(lockfile_error(
            path,
            format!("axiom.lock v1 parser received version {}", lockfile.version),
        ));
    }
    let mut names = BTreeSet::new();
    for package in &lockfile.package {
        require_nonempty(path, "package.name", &package.name)?;
        require_nonempty(path, "package.version", &package.version)?;
        validate_path_source(path, &package.source)?;
        if !names.insert(package.name.as_str()) {
            return Err(lockfile_error(
                path,
                format!("duplicate axiom.lock v1 package name {:?}", package.name),
            ));
        }
    }
    Ok(())
}

fn validate_lockfile_v2_at(lockfile: &LockfileV2, path: &Path) -> Result<(), Diagnostic> {
    if lockfile.version != LOCKFILE_V2_VERSION {
        return Err(lockfile_error(
            path,
            format!(
                "axiom.lock v2 requires version {LOCKFILE_V2_VERSION}, got {}",
                lockfile.version
            ),
        ));
    }
    validate_compatibility(path, "compatibility", &lockfile.compatibility)?;

    let registry_names = lockfile
        .registry
        .iter()
        .map(|registry| registry.name.as_str())
        .collect::<Vec<_>>();
    require_strictly_sorted(path, "registry records", &registry_names)?;
    let mut registry_identities = BTreeSet::new();
    for registry in &lockfile.registry {
        validate_coordinate(path, "registry.name", &registry.name)?;
        if !is_supported_registry_index_url(&registry.source) {
            return Err(lockfile_error(
                path,
                format!(
                    "registry {:?} has unsupported or non-canonical source {:?}",
                    registry.name, registry.source
                ),
            ));
        }
        require_nonempty(
            path,
            "registry.registry_identity",
            &registry.registry_identity,
        )?;
        require_nonempty(path, "registry.source_identity", &registry.source_identity)?;
        validate_sha256(
            path,
            "registry.trust_roots_sha256",
            &registry.trust_roots_sha256,
        )?;
        validate_sha256(
            path,
            "registry.expectation_sha256",
            &registry.expectation_sha256,
        )?;
        validate_sha256(
            path,
            "registry.current_root_transcript_sha256",
            &registry.current_root_transcript_sha256,
        )?;
        validate_sha256(path, "registry.index_sha256", &registry.index_sha256)?;
        validate_sha256(
            path,
            "registry.index_transcript_sha256",
            &registry.index_transcript_sha256,
        )?;
        if registry.current_root_version == 0
            || registry.current_root_sequence == 0
            || registry.index_generation == 0
            || registry.index_sequence == 0
        {
            return Err(lockfile_error(
                path,
                format!(
                    "registry {:?} root/index version and sequence pins must be greater than zero",
                    registry.name
                ),
            ));
        }
        require_nonempty(
            path,
            "registry.index_snapshot_id",
            &registry.index_snapshot_id,
        )?;
        if registry.index_signer_key_ids.is_empty() {
            return Err(lockfile_error(
                path,
                format!(
                    "registry {:?} index_signer_key_ids must not be empty",
                    registry.name
                ),
            ));
        }
        require_strictly_sorted(
            path,
            "registry index_signer_key_ids",
            &registry
                .index_signer_key_ids
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        )?;
        for signer in &registry.index_signer_key_ids {
            validate_key_id(path, "registry.index_signer_key_ids", signer)?;
        }
        if !registry_identities.insert((
            registry.registry_identity.as_str(),
            registry.source_identity.as_str(),
        )) {
            return Err(lockfile_error(
                path,
                "duplicate registry/source identity pair",
            ));
        }
    }
    let configured_registries = lockfile
        .registry
        .iter()
        .map(|registry| registry.name.as_str())
        .collect::<BTreeSet<_>>();

    let package_ids = lockfile
        .package
        .iter()
        .map(|package| package.id.as_str())
        .collect::<Vec<_>>();
    require_strictly_sorted(path, "package records", &package_ids)?;
    let known_packages = package_ids.iter().copied().collect::<BTreeSet<_>>();
    let mut registry_coordinates = BTreeSet::new();
    for package in &lockfile.package {
        validate_locked_package_v2(path, package, &configured_registries)?;
        if let (Some(registry), Some(namespace)) =
            (package.registry.as_deref(), package.namespace.as_deref())
            && !registry_coordinates.insert((registry, namespace, package.name.as_str()))
        {
            return Err(lockfile_error(
                path,
                format!(
                    "axiom.lock v2 selects multiple versions for registry coordinate {registry}/{namespace}/{}",
                    package.name
                ),
            ));
        }
    }
    if lockfile.roots.is_empty() {
        return Err(lockfile_error(
            path,
            "axiom.lock v2 roots must contain at least one entry package id",
        ));
    }
    require_strictly_sorted(
        path,
        "root package ids",
        &lockfile
            .roots
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    )?;
    for root in &lockfile.roots {
        if !known_packages.contains(root.as_str()) {
            return Err(lockfile_error(
                path,
                format!("axiom.lock v2 root {root:?} is absent from package records"),
            ));
        }
        let root_package = lockfile
            .package
            .binary_search_by(|package| package.id.as_str().cmp(root.as_str()))
            .ok()
            .map(|index| &lockfile.package[index])
            .expect("known root package id was checked above");
        if !root_package.source.starts_with("path") {
            return Err(lockfile_error(
                path,
                format!("axiom.lock v2 root {root:?} must identify a path package"),
            ));
        }
    }

    let edge_keys = lockfile.edge.iter().map(edge_order_key).collect::<Vec<_>>();
    require_strictly_sorted(path, "dependency edge records", &edge_keys)?;
    let mut aliases = BTreeSet::new();
    for edge in &lockfile.edge {
        if !known_packages.contains(edge.from.as_str())
            || !known_packages.contains(edge.to.as_str())
        {
            return Err(lockfile_error(
                path,
                format!(
                    "dependency edge {:?} -> {:?} references a package id absent from axiom.lock",
                    edge.from, edge.to
                ),
            ));
        }
        validate_portable_name(path, "edge.alias", &edge.alias)?;
        validate_constraint(path, "edge.requested", &edge.requested, true)?;
        if !aliases.insert((edge.from.as_str(), edge.alias.as_str())) {
            return Err(lockfile_error(
                path,
                format!(
                    "duplicate dependency alias {:?} from package {:?}",
                    edge.alias, edge.from
                ),
            ));
        }
        let target = lockfile
            .package
            .binary_search_by(|package| package.id.as_str().cmp(edge.to.as_str()))
            .ok()
            .map(|index| &lockfile.package[index])
            .expect("known package id was checked above");
        match edge.source_kind {
            LockedDependencySourceKind::Path if !target.source.starts_with("path") => {
                return Err(lockfile_error(
                    path,
                    format!(
                        "path dependency edge {:?} targets non-path source {:?}",
                        edge.alias, target.source
                    ),
                ));
            }
            LockedDependencySourceKind::Registry if !target.source.starts_with("registry:") => {
                return Err(lockfile_error(
                    path,
                    format!(
                        "registry dependency edge {:?} targets non-registry source {:?}",
                        edge.alias, target.source
                    ),
                ));
            }
            _ => {}
        }
        if !locked_version_matches(&edge.requested, &target.version) {
            return Err(lockfile_error(
                path,
                format!(
                    "dependency edge {:?} requests {:?}, which does not select locked version {:?}",
                    edge.alias, edge.requested, target.version
                ),
            ));
        }
        match (edge.source_kind, edge.reason) {
            (LockedDependencySourceKind::Path, LockedDependencyReason::RootPathConstraint)
                if lockfile
                    .roots
                    .binary_search_by(|root| root.as_str().cmp(edge.from.as_str()))
                    .is_ok() => {}
            (
                LockedDependencySourceKind::Path,
                LockedDependencyReason::TransitivePathConstraint,
            ) => {}
            (
                LockedDependencySourceKind::Registry,
                LockedDependencyReason::HighestCompatible
                | LockedDependencyReason::ExactLockedReplay,
            ) if target.yanked_at_resolution == Some(false) => {}
            (
                LockedDependencySourceKind::Registry,
                LockedDependencyReason::TrustedYankedLockedReplay,
            ) if target.yanked_at_resolution == Some(true) => {}
            _ => {
                return Err(lockfile_error(
                    path,
                    format!(
                        "dependency edge {:?} has reason {:?} inconsistent with its source and yank evidence",
                        edge.alias, edge.reason
                    ),
                ));
            }
        }
    }
    validate_acyclic_v2_graph(path, lockfile)?;
    validate_connected_v2_graph(path, lockfile)?;
    Ok(())
}

fn validate_acyclic_v2_graph(path: &Path, lockfile: &LockfileV2) -> Result<(), Diagnostic> {
    let mut incoming = lockfile
        .package
        .iter()
        .map(|package| (package.id.as_str(), 0usize))
        .collect::<BTreeMap<_, _>>();
    let mut outgoing = BTreeMap::<&str, Vec<&str>>::new();
    for edge in &lockfile.edge {
        *incoming
            .get_mut(edge.to.as_str())
            .expect("edge package ids were validated before cycle detection") += 1;
        outgoing
            .entry(edge.from.as_str())
            .or_default()
            .push(edge.to.as_str());
    }

    let mut ready = incoming
        .iter()
        .filter_map(|(package, degree)| (*degree == 0).then_some(*package))
        .collect::<BTreeSet<_>>();
    let mut visited = 0usize;
    while let Some(package) = ready.iter().next().copied() {
        ready.remove(package);
        visited += 1;
        for target in outgoing.get(package).into_iter().flatten() {
            let degree = incoming
                .get_mut(target)
                .expect("edge package ids were validated before cycle detection");
            *degree -= 1;
            if *degree == 0 {
                ready.insert(target);
            }
        }
    }

    if visited == incoming.len() {
        return Ok(());
    }
    let cycle_members = incoming
        .into_iter()
        .filter_map(|(package, degree)| (degree > 0).then_some(package))
        .collect::<Vec<_>>();
    Err(lockfile_error(
        path,
        format!(
            "axiom.lock v2 dependency graph contains a cycle involving {}",
            cycle_members.join(", ")
        ),
    ))
}

fn validate_connected_v2_graph(path: &Path, lockfile: &LockfileV2) -> Result<(), Diagnostic> {
    let mut reachable = lockfile
        .roots
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    loop {
        let before = reachable.len();
        for edge in &lockfile.edge {
            if reachable.contains(edge.from.as_str()) {
                reachable.insert(edge.to.as_str());
            }
        }
        if reachable.len() == before {
            break;
        }
    }
    if reachable.len() != lockfile.package.len() {
        let orphan = lockfile
            .package
            .iter()
            .find(|package| !reachable.contains(package.id.as_str()))
            .map(|package| package.id.as_str())
            .unwrap_or("<unknown>");
        return Err(lockfile_error(
            path,
            format!("axiom.lock v2 contains orphan package record {orphan:?}"),
        ));
    }
    Ok(())
}

fn validate_locked_package_v2(
    path: &Path,
    package: &LockedPackageV2,
    configured_registries: &BTreeSet<&str>,
) -> Result<(), Diagnostic> {
    validate_portable_name(path, "package.name", &package.name)?;
    validate_compatibility(path, "package.compatibility", &package.compatibility)?;
    if package.source == "path" || package.source.starts_with("path:") {
        validate_exact_version(path, "package.version", &package.version)?;
        validate_path_source(path, &package.source)?;
        let expected = canonical_path_package_id(&package.source, &package.name, &package.version);
        if package.id != expected {
            return Err(lockfile_error(
                path,
                format!(
                    "path package id {:?} is not canonical; expected {expected:?}",
                    package.id
                ),
            ));
        }
        if package.registry.is_some()
            || package.namespace.is_some()
            || package.archive_sha256.is_some()
            || package.archive_length.is_some()
            || package.manifest_sha256.is_some()
            || package.provenance_sha256.is_some()
            || package.package_signature_sha256.is_some()
            || package.verification_sha256.is_some()
            || package.publisher_identity.is_some()
            || !package.signer_key_ids.is_empty()
            || package.cache_key.is_some()
            || package.yanked_at_resolution.is_some()
        {
            return Err(lockfile_error(
                path,
                format!(
                    "path package {:?} must not contain registry trust evidence",
                    package.id
                ),
            ));
        }
        return Ok(());
    }

    let Some(source_coordinates) = package.source.strip_prefix("registry:") else {
        return Err(lockfile_error(
            path,
            format!(
                "package {:?} has invalid source {:?}",
                package.id, package.source
            ),
        ));
    };
    validate_coordinate(path, "package.name", &package.name)?;
    validate_exact_version(path, "package.version", &package.version)?;
    let registry = required_option(path, "package.registry", package.registry.as_deref())?;
    let namespace = required_option(path, "package.namespace", package.namespace.as_deref())?;
    validate_coordinate(path, "package.registry", registry)?;
    validate_coordinate(path, "package.namespace", namespace)?;
    if !configured_registries.contains(registry) {
        return Err(lockfile_error(
            path,
            format!(
                "package {:?} references missing registry record {registry:?}",
                package.id
            ),
        ));
    }
    let expected_source = format!("registry:{registry}/{namespace}/{}", package.name);
    if source_coordinates != expected_source.trim_start_matches("registry:") {
        return Err(lockfile_error(
            path,
            format!(
                "registry package source {:?} is not canonical; expected {expected_source:?}",
                package.source
            ),
        ));
    }
    let expected_id =
        canonical_registry_package_id(registry, namespace, &package.name, &package.version);
    if package.id != expected_id {
        return Err(lockfile_error(
            path,
            format!(
                "registry package id {:?} is not canonical; expected {expected_id:?}",
                package.id
            ),
        ));
    }
    let archive_sha256 = required_option(
        path,
        "package.archive_sha256",
        package.archive_sha256.as_deref(),
    )?;
    validate_sha256(path, "package.archive_sha256", archive_sha256)?;
    if package.archive_length == Some(0) || package.archive_length.is_none() {
        return Err(lockfile_error(
            path,
            "registry package archive_length must be greater than zero",
        ));
    }
    if package
        .archive_length
        .is_some_and(|length| length > 64 * 1024 * 1024)
    {
        return Err(lockfile_error(
            path,
            "registry package archive_length exceeds the 64 MiB package archive limit",
        ));
    }
    for (field, digest) in [
        (
            "package.manifest_sha256",
            package.manifest_sha256.as_deref(),
        ),
        (
            "package.provenance_sha256",
            package.provenance_sha256.as_deref(),
        ),
        (
            "package.package_signature_sha256",
            package.package_signature_sha256.as_deref(),
        ),
        (
            "package.verification_sha256",
            package.verification_sha256.as_deref(),
        ),
    ] {
        validate_sha256(path, field, required_option(path, field, digest)?)?;
    }
    require_nonempty(
        path,
        "package.publisher_identity",
        required_option(
            path,
            "package.publisher_identity",
            package.publisher_identity.as_deref(),
        )?,
    )?;
    if package.signer_key_ids.is_empty() {
        return Err(lockfile_error(
            path,
            "registry package signer_key_ids must not be empty",
        ));
    }
    require_strictly_sorted(
        path,
        "package signer_key_ids",
        &package
            .signer_key_ids
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    )?;
    for signer in &package.signer_key_ids {
        validate_key_id(path, "package.signer_key_ids", signer)?;
    }
    let cache_key = required_option(path, "package.cache_key", package.cache_key.as_deref())?;
    let expected_cache_key = format!("sha256:{archive_sha256}");
    if cache_key != expected_cache_key {
        return Err(lockfile_error(
            path,
            format!("package.cache_key must equal the content address {expected_cache_key:?}"),
        ));
    }
    if package.yanked_at_resolution.is_none() {
        return Err(lockfile_error(
            path,
            "registry package yanked_at_resolution must be recorded",
        ));
    }
    Ok(())
}

fn validate_compatibility(
    path: &Path,
    field: &str,
    evidence: &LockedCompatibilityEvidence,
) -> Result<(), Diagnostic> {
    require_nonempty(path, &format!("{field}.contract"), &evidence.contract)?;
    require_nonempty(path, &format!("{field}.compiler"), &evidence.compiler)?;
    require_nonempty(
        path,
        &format!("{field}.edition_policy"),
        &evidence.edition_policy,
    )
}

fn edge_order_key(
    edge: &LockedDependencyEdgeV2,
) -> (
    &str,
    &str,
    &str,
    &str,
    LockedDependencySourceKind,
    LockedDependencyReason,
) {
    (
        edge.from.as_str(),
        edge.alias.as_str(),
        edge.to.as_str(),
        edge.requested.as_str(),
        edge.source_kind,
        edge.reason,
    )
}

fn require_strictly_sorted<T: Ord + std::fmt::Debug>(
    path: &Path,
    label: &str,
    values: &[T],
) -> Result<(), Diagnostic> {
    if values.windows(2).all(|pair| pair[0] < pair[1]) {
        Ok(())
    } else {
        Err(lockfile_error(
            path,
            format!("{label} must be strictly sorted and duplicate-free"),
        ))
    }
}

fn validate_path_source(path: &Path, source: &str) -> Result<(), Diagnostic> {
    if source == "path" {
        return Ok(());
    }
    let Some(relative) = source.strip_prefix("path:") else {
        return Err(lockfile_error(
            path,
            format!("path package has invalid source {source:?}"),
        ));
    };
    if relative.is_empty()
        || relative == "."
        || relative.contains('\\')
        || Path::new(relative).is_absolute()
        || Path::new(relative)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || normalize_dependency_source(relative) != relative
    {
        return Err(lockfile_error(
            path,
            format!("path source {source:?} is not portable and canonical"),
        ));
    }
    Ok(())
}

fn validate_sha256(path: &Path, field: &str, digest: &str) -> Result<(), Diagnostic> {
    if digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(lockfile_error(
            path,
            format!("{field} must contain exactly 64 lowercase hexadecimal characters"),
        ))
    }
}

fn validate_key_id(path: &Path, field: &str, key_id: &str) -> Result<(), Diagnostic> {
    let Some(digest) = key_id.strip_prefix("sha256:") else {
        return Err(lockfile_error(
            path,
            format!("{field} entries must use sha256:<64 lowercase hex> key ids"),
        ));
    };
    validate_sha256(path, field, digest)
}

fn validate_coordinate(path: &Path, field: &str, value: &str) -> Result<(), Diagnostic> {
    let mut chars = value.chars();
    let valid = value.len() <= 256
        && chars
            .next()
            .is_some_and(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
        && chars.all(|ch| {
            ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_' | '.')
        });
    if valid {
        Ok(())
    } else {
        Err(lockfile_error(
            path,
            format!("{field} must be a portable lowercase coordinate"),
        ))
    }
}

fn validate_portable_name(path: &Path, field: &str, value: &str) -> Result<(), Diagnostic> {
    let valid = !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(lockfile_error(
            path,
            format!("{field} must be a portable ASCII name"),
        ))
    }
}

fn validate_constraint(
    path: &Path,
    field: &str,
    value: &str,
    allow_wildcard: bool,
) -> Result<(), Diagnostic> {
    if allow_wildcard && value == "*" {
        return Ok(());
    }
    validate_exact_version(path, field, value.strip_prefix('^').unwrap_or(value))
}

fn locked_version_matches(constraint: &str, version: &str) -> bool {
    if constraint == "*" || constraint == version {
        return true;
    }
    let Some(base) = constraint.strip_prefix('^').and_then(parse_version_triplet) else {
        return false;
    };
    let Some(selected) = parse_version_triplet(version) else {
        return false;
    };
    if base.0 == 0 && base.1 == 0 {
        selected.0 == 0 && selected.1 == 0 && selected.2 == base.2
    } else if base.0 == 0 {
        selected.0 == 0 && selected.1 == base.1 && selected >= base
    } else {
        selected.0 == base.0 && selected >= base
    }
}

fn parse_version_triplet(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn validate_exact_version(path: &Path, field: &str, value: &str) -> Result<(), Diagnostic> {
    let parts = value.split('.').collect::<Vec<_>>();
    let numeric = parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (*part == "0" || !part.starts_with('0'))
        });
    if numeric {
        Ok(())
    } else {
        Err(lockfile_error(
            path,
            format!("{field} must be a canonical MAJOR.MINOR.PATCH version"),
        ))
    }
}

fn required_option<'a>(
    path: &Path,
    field: &str,
    value: Option<&'a str>,
) -> Result<&'a str, Diagnostic> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| lockfile_error(path, format!("missing or empty {field}")))
}

fn require_nonempty(path: &Path, field: &str, value: &str) -> Result<(), Diagnostic> {
    if value.is_empty() || value.trim() != value {
        Err(lockfile_error(
            path,
            format!("{field} must be non-empty without surrounding whitespace"),
        ))
    } else {
        Ok(())
    }
}

fn lockfile_error(path: &Path, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("lockfile", message).with_path(path.display().to_string())
}

pub fn expected_lockfile(manifest: &Manifest) -> Lockfile {
    Lockfile {
        version: LOCKFILE_V1_VERSION,
        package: manifest
            .package
            .as_ref()
            .map(|package| {
                vec![LockedPackage {
                    name: package.name.clone(),
                    version: package.version.clone(),
                    source: String::from("path"),
                }]
            })
            .unwrap_or_default(),
    }
}

pub fn expected_lockfile_for_project(
    project_root: &Path,
    manifest: &Manifest,
) -> Result<Lockfile, Diagnostic> {
    if manifest
        .dependencies
        .values()
        .any(DependencySpec::is_registry)
    {
        return Err(Diagnostic::new("lockfile", REGISTRY_LOCKFILE_V2_REQUIRED)
            .with_path(lockfile_path(project_root).display().to_string()));
    }
    let project_root = normalize_path(project_root);
    let mut package = expected_lockfile(manifest).package;
    let mut visited = BTreeSet::from([project_root.clone()]);
    collect_workspace_packages(
        &project_root,
        &project_root,
        manifest,
        &mut visited,
        &mut package,
    )?;
    collect_dependency_packages(
        &project_root,
        &project_root,
        manifest,
        &mut visited,
        &mut package,
    )?;
    if let Some((_, dependencies)) = package.split_first_mut() {
        dependencies.sort_by(|left, right| {
            left.source
                .cmp(&right.source)
                .then(left.name.cmp(&right.name))
        });
    }
    Ok(Lockfile {
        version: LOCKFILE_V1_VERSION,
        package,
    })
}

pub fn render_lockfile(manifest: &Manifest) -> Result<String, Diagnostic> {
    toml::to_string_pretty(&expected_lockfile(manifest))
        .map_err(|err| Diagnostic::new("lockfile", format!("failed to render axiom.lock: {err}")))
}

pub fn render_lockfile_for_project(
    project_root: &Path,
    manifest: &Manifest,
) -> Result<String, Diagnostic> {
    toml::to_string_pretty(&expected_lockfile_for_project(project_root, manifest)?)
        .map_err(|err| Diagnostic::new("lockfile", format!("failed to render axiom.lock: {err}")))
}

pub fn validate_lockfile(project_root: &Path, manifest: &Manifest) -> Result<(), Diagnostic> {
    let lockfile = load_lockfile(project_root)?;
    validate_lockfile_version_for_manifest(manifest, &lockfile)?;
    match lockfile {
        ParsedLockfile::V1(lockfile) => {
            let expected = expected_lockfile_for_project(project_root, manifest)?;
            compare_v1_lockfile(project_root, &lockfile, &expected)
        }
        ParsedLockfile::V2(lockfile) => {
            validate_v2_against_manifest(project_root, manifest, &lockfile)
        }
    }
}

pub fn validate_lockfile_packages(
    project_root: &Path,
    packages: &[LockedPackage],
) -> Result<(), Diagnostic> {
    let path = lockfile_path(project_root);
    let content = std::fs::read(&path).map_err(|err| {
        Diagnostic::new("lockfile", format!("failed to read axiom.lock: {err}"))
            .with_path(path.display().to_string())
    })?;
    let ParsedLockfile::V1(lockfile) = parse_lockfile_exact(&content, &path)? else {
        return Err(lockfile_error(
            &path,
            "validate_lockfile_packages requires axiom.lock version 1",
        ));
    };
    let expected = Lockfile {
        version: LOCKFILE_V1_VERSION,
        package: packages.to_vec(),
    };
    compare_v1_lockfile(project_root, &lockfile, &expected)
}

fn compare_v1_lockfile(
    project_root: &Path,
    lockfile: &Lockfile,
    expected: &Lockfile,
) -> Result<(), Diagnostic> {
    let path = lockfile_path(project_root);
    if lockfile != expected {
        let detail = lockfile_mismatch_detail(&lockfile, &expected);
        return Err(
            Diagnostic::new(
                "lockfile",
                format!(
                    "axiom.lock does not match axiom.toml; regenerate it with `axiomc new` or update it manually; {detail}"
                ),
            )
            .with_path(path.display().to_string()),
        );
    }
    Ok(())
}

fn validate_v2_against_manifest(
    project_root: &Path,
    manifest: &Manifest,
    lockfile: &LockfileV2,
) -> Result<(), Diagnostic> {
    let path = lockfile_path(project_root);
    if let Some(registry) = &manifest.registry {
        let Some(locked_registry) = lockfile
            .registry
            .iter()
            .find(|entry| entry.name == registry.name)
        else {
            return Err(lockfile_error(
                &path,
                format!(
                    "axiom.lock v2 is missing configured registry {:?}",
                    registry.name
                ),
            ));
        };
        if locked_registry.source != registry.index {
            return Err(lockfile_error(
                &path,
                format!(
                    "configured registry source changed (axiom.lock has {:?}; axiom.toml expects {:?})",
                    locked_registry.source, registry.index
                ),
            ));
        }
    }
    let root_id = manifest
        .package
        .as_ref()
        .map(|package| canonical_path_package_id("path", &package.name, &package.version));
    for (alias, dependency) in &manifest.dependencies {
        let matching_edges = lockfile
            .edge
            .iter()
            .filter(|edge| {
                edge.alias == *alias
                    && root_id
                        .as_deref()
                        .is_none_or(|root_id| edge.from == root_id)
            })
            .collect::<Vec<_>>();
        if matching_edges.is_empty() {
            return Err(lockfile_error(
                &path,
                format!("axiom.lock v2 is missing dependency edge for alias {alias:?}"),
            ));
        }
        if matching_edges.len() > 1 {
            return Err(lockfile_error(
                &path,
                format!("axiom.lock v2 has ambiguous dependency edges for alias {alias:?}"),
            ));
        }
        let edge = matching_edges[0];
        let expected_kind = if dependency.is_registry() {
            LockedDependencySourceKind::Registry
        } else {
            LockedDependencySourceKind::Path
        };
        if edge.source_kind != expected_kind
            || dependency
                .version
                .as_deref()
                .is_some_and(|version| edge.requested != version)
        {
            return Err(lockfile_error(
                &path,
                format!("axiom.lock v2 dependency decision for alias {alias:?} is stale"),
            ));
        }
    }
    Ok(())
}

fn lockfile_mismatch_detail(lockfile: &Lockfile, expected: &Lockfile) -> String {
    if lockfile.version != expected.version {
        return format!(
            "lockfile version is {}, expected {}",
            lockfile.version, expected.version
        );
    }

    let locked_by_name = lockfile
        .package
        .iter()
        .map(|package| (package.name.as_str(), package))
        .collect::<BTreeMap<_, _>>();
    let expected_by_name = expected
        .package
        .iter()
        .map(|package| (package.name.as_str(), package))
        .collect::<BTreeMap<_, _>>();

    for expected_package in &expected.package {
        let Some(locked_package) = locked_by_name.get(expected_package.name.as_str()) else {
            return format!(
                "package {:?} is missing from axiom.lock (expected version {:?} from source {:?})",
                expected_package.name, expected_package.version, expected_package.source
            );
        };
        if locked_package.version != expected_package.version
            || locked_package.source != expected_package.source
        {
            return format!(
                "package {:?} changed (axiom.lock has version {:?} from source {:?}; axiom.toml expects version {:?} from source {:?})",
                expected_package.name,
                locked_package.version,
                locked_package.source,
                expected_package.version,
                expected_package.source
            );
        }
    }

    for locked_package in &lockfile.package {
        if !expected_by_name.contains_key(locked_package.name.as_str()) {
            return format!(
                "package {:?} is extra in axiom.lock (locked version {:?} from source {:?})",
                locked_package.name, locked_package.version, locked_package.source
            );
        }
    }

    "package entries differ in order or duplicate package names".to_string()
}

fn dependency_root(
    root_project_root: &Path,
    project_root: &Path,
    spec: &DependencySpec,
) -> Result<PathBuf, Diagnostic> {
    let path_source = spec.path_source().ok_or_else(|| {
        Diagnostic::new("lockfile", REGISTRY_LOCKFILE_V2_REQUIRED)
            .with_path(lockfile_path(root_project_root).display().to_string())
    })?;
    let dependency_root = normalize_path(project_root.join(path_source));
    let canonical_project_root = canonicalize_path(project_root, "dependency source package")?;
    let canonical_dependency_root = canonicalize_path(&dependency_root, "dependency path")?;
    let canonical_root_project_root = canonicalize_path(root_project_root, "package root")?;
    if canonical_dependency_root.starts_with(&canonical_root_project_root)
        || workspace_declares_dependency_member(&canonical_project_root, &canonical_dependency_root)
    {
        return Ok(dependency_root);
    }
    Err(
        Diagnostic::new(
            "manifest",
            "dependency path must stay inside the workspace or package root; declare sibling packages as workspace members before depending on them",
        )
        .with_path(manifest_path(project_root).display().to_string()),
    )
}

fn collect_dependency_packages(
    root_project_root: &Path,
    project_root: &Path,
    manifest: &Manifest,
    visited: &mut BTreeSet<PathBuf>,
    packages: &mut Vec<LockedPackage>,
) -> Result<(), Diagnostic> {
    for spec in manifest.dependencies.values() {
        let dependency_root = dependency_root(root_project_root, project_root, spec)?;
        if !visited.insert(dependency_root.clone()) {
            continue;
        }
        let dependency_manifest = load_manifest(&dependency_root)?;
        let dependency_package = dependency_manifest.package.as_ref().ok_or_else(|| {
            Diagnostic::new(
                "manifest",
                format!(
                    "dependency at {} must define a [package] section",
                    dependency_root.display()
                ),
            )
            .with_path(dependency_root.join("axiom.toml").display().to_string())
        })?;
        packages.push(LockedPackage {
            name: dependency_package.name.clone(),
            version: dependency_package.version.clone(),
            source: format!(
                "path:{}",
                normalize_dependency_source(
                    &relative_path(root_project_root, &dependency_root)
                        .display()
                        .to_string(),
                )
            ),
        });
        collect_dependency_packages(
            root_project_root,
            &dependency_root,
            &dependency_manifest,
            visited,
            packages,
        )?;
    }
    Ok(())
}

fn canonicalize_path(path: &Path, label: &str) -> Result<PathBuf, Diagnostic> {
    fs::canonicalize(path).map_err(|err| {
        Diagnostic::new(
            "manifest",
            format!("{label} {} is not accessible: {err}", path.display()),
        )
        .with_path(path.display().to_string())
    })
}

fn workspace_declares_dependency_member(project_root: &Path, dependency_root: &Path) -> bool {
    for ancestor in project_root.ancestors().skip(1) {
        let manifest_file = manifest_path(ancestor);
        if !manifest_file.exists() {
            continue;
        }
        let Ok(manifest) = load_manifest(ancestor) else {
            continue;
        };
        let Some(workspace) = manifest.workspace.as_ref() else {
            continue;
        };
        let mut members = BTreeSet::new();
        for member in &workspace.members {
            let member_root = ancestor.join(member);
            if let Ok(member_root) = fs::canonicalize(&member_root) {
                members.insert(member_root);
            }
        }
        if members.contains(project_root) && members.contains(dependency_root) {
            return true;
        }
    }
    false
}

fn collect_workspace_packages(
    root_project_root: &Path,
    project_root: &Path,
    manifest: &Manifest,
    visited: &mut BTreeSet<PathBuf>,
    packages: &mut Vec<LockedPackage>,
) -> Result<(), Diagnostic> {
    for member in manifest
        .workspace
        .as_ref()
        .into_iter()
        .flat_map(|workspace| workspace.members.iter())
    {
        let member_root = normalize_path(project_root.join(member));
        if !visited.insert(member_root.clone()) {
            continue;
        }
        let member_manifest = load_manifest(&member_root)?;
        if let Some(member_package) = member_manifest.package.as_ref() {
            packages.push(LockedPackage {
                name: member_package.name.clone(),
                version: member_package.version.clone(),
                source: format!(
                    "path:{}",
                    normalize_dependency_source(
                        &relative_path(root_project_root, &member_root)
                            .display()
                            .to_string(),
                    )
                ),
            });
        }
        collect_workspace_packages(
            root_project_root,
            &member_root,
            &member_manifest,
            visited,
            packages,
        )?;
        collect_dependency_packages(
            root_project_root,
            &member_root,
            &member_manifest,
            visited,
            packages,
        )?;
    }
    Ok(())
}

fn relative_path(from: &Path, to: &Path) -> PathBuf {
    let from_components = from.components().collect::<Vec<_>>();
    let to_components = to.components().collect::<Vec<_>>();
    let mut shared = 0usize;
    while shared < from_components.len()
        && shared < to_components.len()
        && from_components[shared] == to_components[shared]
    {
        shared += 1;
    }

    let mut relative = PathBuf::new();
    for _ in shared..from_components.len() {
        relative.push("..");
    }
    for component in &to_components[shared..] {
        relative.push(component.as_os_str());
    }
    relative
}

fn normalize_dependency_source(path: &str) -> String {
    let mut normalized = PathBuf::new();
    let mut saw_component = false;
    for component in Path::new(path).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push("..");
                }
                saw_component = true;
            }
            Component::Normal(value) => {
                normalized.push(value);
                saw_component = true;
            }
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    if !saw_component {
        return String::from(".");
    }
    normalized.to_string_lossy().replace('\\', "/")
}

fn normalize_path(path: impl AsRef<Path>) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.as_ref().components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_manifest(root: &Path, body: &str) {
        fs::create_dir_all(root).expect("create package dir");
        fs::write(root.join("axiom.toml"), body).expect("write manifest");
    }

    fn write_lockfile(project_root: &Path, package: Vec<LockedPackage>) {
        let lockfile = Lockfile {
            version: 1,
            package,
        };
        std::fs::write(
            lockfile_path(project_root),
            toml::to_string_pretty(&lockfile).expect("render lockfile fixture"),
        )
        .expect("write lockfile fixture");
    }

    fn digest(byte: char) -> String {
        std::iter::repeat_n(byte, 64).collect()
    }

    fn key_id(byte: char) -> String {
        format!("sha256:{}", digest(byte))
    }

    fn compatibility() -> LockedCompatibilityEvidence {
        LockedCompatibilityEvidence {
            contract: "compatibility-v1".to_string(),
            compiler: "axiomc-0.3.0".to_string(),
            edition_policy: "2026-policy-only".to_string(),
        }
    }

    fn sample_lockfile_v2() -> LockfileV2 {
        let root_id = canonical_path_package_id("path", "app", "1.0.0");
        let dependency_id = canonical_registry_package_id("primary", "acme", "math", "1.2.3");
        LockfileV2 {
            version: LOCKFILE_V2_VERSION,
            compatibility: compatibility(),
            roots: vec![root_id.clone()],
            registry: vec![LockedRegistryV2 {
                name: "primary".to_string(),
                source: "https://registry.example.test/index.json".to_string(),
                registry_identity: "https://registry.example.test".to_string(),
                source_identity: "https://registry.example.test/index.json".to_string(),
                trust_roots_sha256: digest('1'),
                expectation_sha256: digest('9'),
                current_root_version: 4,
                current_root_sequence: 9,
                current_root_transcript_sha256: digest('2'),
                index_sha256: digest('3'),
                index_transcript_sha256: digest('4'),
                index_generation: 7,
                index_sequence: 11,
                index_snapshot_id: "snapshot-7-11".to_string(),
                index_signer_key_ids: vec![key_id('a'), key_id('b')],
            }],
            package: vec![
                LockedPackageV2 {
                    id: root_id.clone(),
                    name: "app".to_string(),
                    version: "1.0.0".to_string(),
                    source: "path".to_string(),
                    registry: None,
                    namespace: None,
                    archive_sha256: None,
                    archive_length: None,
                    manifest_sha256: None,
                    provenance_sha256: None,
                    package_signature_sha256: None,
                    verification_sha256: None,
                    publisher_identity: None,
                    signer_key_ids: Vec::new(),
                    cache_key: None,
                    yanked_at_resolution: None,
                    compatibility: compatibility(),
                },
                LockedPackageV2 {
                    id: dependency_id.clone(),
                    name: "math".to_string(),
                    version: "1.2.3".to_string(),
                    source: "registry:primary/acme/math".to_string(),
                    registry: Some("primary".to_string()),
                    namespace: Some("acme".to_string()),
                    archive_sha256: Some(digest('5')),
                    archive_length: Some(1234),
                    manifest_sha256: Some(digest('6')),
                    provenance_sha256: Some(digest('7')),
                    package_signature_sha256: Some(digest('8')),
                    verification_sha256: Some(digest('0')),
                    publisher_identity: Some("https://publisher.example.test/acme".to_string()),
                    signer_key_ids: vec![key_id('c'), key_id('d')],
                    cache_key: Some(format!("sha256:{}", digest('5'))),
                    yanked_at_resolution: Some(false),
                    compatibility: compatibility(),
                },
            ],
            edge: vec![LockedDependencyEdgeV2 {
                from: root_id,
                to: dependency_id,
                alias: "math".to_string(),
                requested: "^1.2.0".to_string(),
                source_kind: LockedDependencySourceKind::Registry,
                reason: LockedDependencyReason::HighestCompatible,
            }],
        }
    }

    #[test]
    fn lockfile_rejects_unknown_top_level_field() {
        let toml = "version = 1\nextra = \"tamper\"\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\nsource = \"path\"\n";
        let error = toml::from_str::<Lockfile>(toml)
            .expect_err("unknown lockfile field should be rejected");
        assert!(
            error.to_string().contains("unknown field"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn locked_package_rejects_unknown_field() {
        let toml = "version = 1\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\nsource = \"path\"\nchecksum = \"deadbeef\"\n";
        let error =
            toml::from_str::<Lockfile>(toml).expect_err("unknown package field should be rejected");
        assert!(
            error.to_string().contains("unknown field"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn expected_lockfile_keeps_root_first_and_sorts_dependencies_in_place() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("root");
        write_manifest(
            &root,
            "[package]\nname = \"root\"\nversion = \"1.0.0\"\n\n[workspace]\nmembers = [\"members/zeta\", \"members/alpha\"]\n",
        );
        write_manifest(
            &root.join("members/zeta"),
            "[package]\nname = \"zeta\"\nversion = \"1.0.0\"\n",
        );
        write_manifest(
            &root.join("members/alpha"),
            "[package]\nname = \"alpha\"\nversion = \"1.0.0\"\n",
        );
        let manifest = load_manifest(&root).expect("load root manifest");

        let lockfile =
            expected_lockfile_for_project(&root, &manifest).expect("render project lockfile");

        let packages = lockfile
            .package
            .iter()
            .map(|package| {
                (
                    package.name.as_str(),
                    package.version.as_str(),
                    package.source.as_str(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            packages,
            vec![
                ("root", "1.0.0", "path"),
                ("alpha", "1.0.0", "path:members/alpha"),
                ("zeta", "1.0.0", "path:members/zeta"),
            ]
        );
    }

    #[test]
    fn validate_lockfile_reports_changed_package_detail() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_lockfile(
            dir.path(),
            vec![LockedPackage {
                name: "demo".to_string(),
                version: "0.1.0".to_string(),
                source: "path".to_string(),
            }],
        );

        let error = validate_lockfile_packages(
            dir.path(),
            &[LockedPackage {
                name: "demo".to_string(),
                version: "0.2.0".to_string(),
                source: "path:deps/demo".to_string(),
            }],
        )
        .expect_err("changed package should fail");

        assert_eq!(error.kind, "lockfile");
        assert!(error.message.contains("package \"demo\" changed"));
        assert!(
            error
                .message
                .contains("version \"0.1.0\" from source \"path\"")
        );
        assert!(
            error
                .message
                .contains("version \"0.2.0\" from source \"path:deps/demo\"")
        );
    }

    #[test]
    fn validate_lockfile_reports_missing_package_detail() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_lockfile(dir.path(), Vec::new());

        let error = validate_lockfile_packages(
            dir.path(),
            &[LockedPackage {
                name: "core".to_string(),
                version: "1.0.0".to_string(),
                source: "path:deps/core".to_string(),
            }],
        )
        .expect_err("missing package should fail");

        assert_eq!(error.kind, "lockfile");
        assert!(error.message.contains("package \"core\" is missing"));
        assert!(
            error
                .message
                .contains("expected version \"1.0.0\" from source \"path:deps/core\"")
        );
    }

    #[test]
    fn validate_lockfile_reports_extra_package_detail() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_lockfile(
            dir.path(),
            vec![LockedPackage {
                name: "old".to_string(),
                version: "0.9.0".to_string(),
                source: "path:deps/old".to_string(),
            }],
        );

        let error =
            validate_lockfile_packages(dir.path(), &[]).expect_err("extra package should fail");

        assert_eq!(error.kind, "lockfile");
        assert!(error.message.contains("package \"old\" is extra"));
        assert!(
            error
                .message
                .contains("locked version \"0.9.0\" from source \"path:deps/old\"")
        );
    }

    #[test]
    fn lockfile_v2_roundtrips_with_exact_replay_pins() {
        let lockfile = sample_lockfile_v2();
        let rendered = render_lockfile_v2(&lockfile).expect("render v2");
        assert!(rendered.contains("roots = [\"path:.#app@1.0.0\"]"));
        assert!(rendered.contains("trust_roots_sha256"));
        assert!(rendered.contains("expectation_sha256"));
        assert!(rendered.contains("current_root_transcript_sha256"));
        assert!(rendered.contains("index_snapshot_id = \"snapshot-7-11\""));
        assert!(rendered.contains("index_signer_key_ids"));
        assert!(rendered.contains("verification_sha256"));
        let parsed = parse_lockfile_exact(rendered.as_bytes(), Path::new("fixture/axiom.lock"))
            .expect("parse rendered v2");
        assert_eq!(parsed, ParsedLockfile::V2(lockfile.clone()));
        let ParsedLockfile::V2(parsed) = parsed else {
            panic!("expected v2");
        };
        assert_eq!(
            render_lockfile_v2(&parsed).expect("rerender v2"),
            rendered,
            "v2 rendering must be deterministic"
        );
    }

    #[test]
    fn lockfile_v2_rejects_archive_above_runtime_limit() {
        let mut lockfile = sample_lockfile_v2();
        lockfile.package[1].archive_length = Some(64 * 1024 * 1024 + 1);
        let error = validate_lockfile_v2(&lockfile).expect_err("oversized archive must fail");
        assert!(
            error.message.contains("64 MiB package archive limit"),
            "unexpected diagnostic: {error:?}"
        );
    }

    #[test]
    fn lockfile_v2_accepts_multiple_explicit_workspace_roots() {
        let mut lockfile = sample_lockfile_v2();
        let workspace_root_id = canonical_path_package_id("path:members/tool", "tool", "1.0.0");
        let utility_id = canonical_path_package_id("path:deps/util", "util", "1.0.0");
        let path_package = |id: String, name: &str, source: &str| LockedPackageV2 {
            id,
            name: name.to_owned(),
            version: "1.0.0".to_owned(),
            source: source.to_owned(),
            registry: None,
            namespace: None,
            archive_sha256: None,
            archive_length: None,
            manifest_sha256: None,
            provenance_sha256: None,
            package_signature_sha256: None,
            verification_sha256: None,
            publisher_identity: None,
            signer_key_ids: Vec::new(),
            cache_key: None,
            yanked_at_resolution: None,
            compatibility: compatibility(),
        };
        lockfile.package.push(path_package(
            workspace_root_id.clone(),
            "tool",
            "path:members/tool",
        ));
        lockfile
            .package
            .push(path_package(utility_id.clone(), "util", "path:deps/util"));
        lockfile
            .package
            .sort_by(|left, right| left.id.cmp(&right.id));
        lockfile.roots.push(workspace_root_id.clone());
        lockfile.roots.sort();
        lockfile.edge.push(LockedDependencyEdgeV2 {
            from: workspace_root_id,
            to: utility_id,
            alias: "util".to_owned(),
            requested: "1.0.0".to_owned(),
            source_kind: LockedDependencySourceKind::Path,
            reason: LockedDependencyReason::RootPathConstraint,
        });
        lockfile
            .edge
            .sort_by(|left, right| edge_order_key(left).cmp(&edge_order_key(right)));

        validate_lockfile_v2(&lockfile).expect("multiple explicit roots should validate");

        let mut missing_root = lockfile.clone();
        missing_root
            .roots
            .push("path:missing#missing@1.0.0".to_owned());
        missing_root.roots.sort();
        assert!(
            validate_lockfile_v2(&missing_root)
                .expect_err("unknown root must fail")
                .message
                .contains("absent from package records")
        );
    }

    #[test]
    fn lockfile_v2_rejects_order_digest_and_dangling_edge_tamper() {
        let mut unsorted = sample_lockfile_v2();
        unsorted.package[1].signer_key_ids.reverse();
        assert!(
            validate_lockfile_v2(&unsorted)
                .expect_err("unsorted signer ids")
                .message
                .contains("strictly sorted")
        );

        let mut malformed_digest = sample_lockfile_v2();
        malformed_digest.registry[0].index_sha256 = "ABC".to_string();
        assert!(
            validate_lockfile_v2(&malformed_digest)
                .expect_err("malformed digest")
                .message
                .contains("64 lowercase hexadecimal")
        );

        let mut malformed_verification = sample_lockfile_v2();
        malformed_verification.package[1].verification_sha256 = Some("A".repeat(64));
        assert!(
            validate_lockfile_v2(&malformed_verification)
                .expect_err("uppercase verification digest")
                .message
                .contains("64 lowercase hexadecimal")
        );

        let mut missing_verification = sample_lockfile_v2();
        missing_verification.package[1].verification_sha256 = None;
        assert!(
            validate_lockfile_v2(&missing_verification)
                .expect_err("missing verification digest")
                .message
                .contains("missing or empty package.verification_sha256")
        );

        let mut dangling = sample_lockfile_v2();
        dangling.edge[0].to = "registry:primary/acme/missing@1.0.0".to_string();
        assert!(
            validate_lockfile_v2(&dangling)
                .expect_err("dangling edge")
                .message
                .contains("absent from axiom.lock")
        );
    }

    #[test]
    fn lockfile_v2_rejects_invalid_source_combinations_and_duplicate_aliases() {
        let mut path_with_registry_evidence = sample_lockfile_v2();
        path_with_registry_evidence.package[0].archive_sha256 = Some(digest('e'));
        assert!(
            validate_lockfile_v2(&path_with_registry_evidence)
                .expect_err("path trust evidence")
                .message
                .contains("must not contain registry trust evidence")
        );

        let mut path_with_verification_evidence = sample_lockfile_v2();
        path_with_verification_evidence.package[0].verification_sha256 = Some(digest('e'));
        assert!(
            validate_lockfile_v2(&path_with_verification_evidence)
                .expect_err("path verification evidence")
                .message
                .contains("must not contain registry trust evidence")
        );

        let mut duplicate_alias = sample_lockfile_v2();
        let mut second = duplicate_alias.edge[0].clone();
        second.to = duplicate_alias.package[0].id.clone();
        second.requested = "1.0.0".to_string();
        second.source_kind = LockedDependencySourceKind::Path;
        second.reason = LockedDependencyReason::RootPathConstraint;
        duplicate_alias.edge.push(second);
        duplicate_alias
            .edge
            .sort_by(|left, right| edge_order_key(left).cmp(&edge_order_key(right)));
        assert!(
            validate_lockfile_v2(&duplicate_alias)
                .expect_err("duplicate alias")
                .message
                .contains("duplicate dependency alias")
        );
    }

    #[test]
    fn lockfile_v2_rejects_noncanonical_path_sources_and_versions() {
        for source in [
            "path:",
            "path:.",
            "path:./child",
            "path:../outside",
            "path:child/../outside",
            "path:/absolute",
            "path:child\\nested",
            "path:child//nested",
            "path:child/",
        ] {
            let mut lockfile = sample_lockfile_v2();
            lockfile.package[0].source = source.to_string();
            lockfile.package[0].id = canonical_path_package_id(
                source,
                &lockfile.package[0].name,
                &lockfile.package[0].version,
            );
            lockfile.roots[0] = lockfile.package[0].id.clone();
            lockfile.edge[0].from = lockfile.package[0].id.clone();
            assert!(
                validate_lockfile_v2(&lockfile)
                    .expect_err("noncanonical path source must fail")
                    .message
                    .contains("not portable and canonical"),
                "runtime accepted or misdiagnosed {source:?}"
            );
        }

        let mut invalid_version = sample_lockfile_v2();
        invalid_version.package[0].version = "1.0".to_string();
        assert!(
            validate_lockfile_v2(&invalid_version)
                .expect_err("non-release path version must fail")
                .message
                .contains("canonical MAJOR.MINOR.PATCH version"),
        );
    }

    #[test]
    fn lockfile_v2_rejects_unknown_dependency_reason() {
        let rendered = render_lockfile_v2(&sample_lockfile_v2()).expect("render v2");
        let tampered = rendered.replace(
            "reason = \"highest_compatible\"",
            "reason = \"network_was_fast\"",
        );
        let error = parse_lockfile_exact(tampered.as_bytes(), Path::new("axiom.lock"))
            .expect_err("unknown reason must fail");
        assert!(
            error.message.contains("unknown variant") && error.message.contains("network_was_fast"),
            "unexpected error: {}",
            error.message
        );
    }

    #[test]
    fn lockfile_v2_rejects_constraint_yank_and_orphan_inconsistency() {
        let mut incompatible = sample_lockfile_v2();
        incompatible.edge[0].requested = "^2.0.0".to_string();
        assert!(
            validate_lockfile_v2(&incompatible)
                .expect_err("incompatible selected version")
                .message
                .contains("does not select locked version")
        );

        let mut yanked_without_replay = sample_lockfile_v2();
        yanked_without_replay.package[1].yanked_at_resolution = Some(true);
        assert!(
            validate_lockfile_v2(&yanked_without_replay)
                .expect_err("yanked selection reason mismatch")
                .message
                .contains("inconsistent with its source and yank evidence")
        );

        let mut orphaned = sample_lockfile_v2();
        let mut orphan = orphaned.package[0].clone();
        orphan.name = "orphan".to_string();
        orphan.source = "path:deps/orphan".to_string();
        orphan.id = canonical_path_package_id(&orphan.source, &orphan.name, &orphan.version);
        orphaned.package.push(orphan);
        orphaned
            .package
            .sort_by(|left, right| left.id.cmp(&right.id));
        assert!(
            validate_lockfile_v2(&orphaned)
                .expect_err("orphan package")
                .message
                .contains("orphan package record")
        );
    }

    #[test]
    fn lockfile_v2_rejects_multiple_versions_for_one_registry_coordinate() {
        let mut lockfile = sample_lockfile_v2();
        let mut second = lockfile.package[1].clone();
        second.version = "1.2.4".to_string();
        second.id = canonical_registry_package_id("primary", "acme", "math", "1.2.4");
        lockfile.package.push(second);
        lockfile
            .package
            .sort_by(|left, right| left.id.cmp(&right.id));

        let rendered = toml::to_string_pretty(&lockfile).expect("serialize tampered lockfile");
        let error = parse_lockfile_exact(rendered.as_bytes(), Path::new("axiom.lock"))
            .expect_err("duplicate registry coordinate must fail strict parsing");
        assert!(
            error
                .message
                .contains("selects multiple versions for registry coordinate primary/acme/math"),
            "unexpected diagnostic: {error:?}"
        );
    }

    #[test]
    fn lockfile_v2_rejects_path_and_registry_dependency_cycles() {
        let mut path_cycle = sample_lockfile_v2();
        let utility_id = canonical_path_package_id("path:deps/util", "util", "1.0.0");
        path_cycle.package.push(LockedPackageV2 {
            id: utility_id.clone(),
            name: "util".to_string(),
            version: "1.0.0".to_string(),
            source: "path:deps/util".to_string(),
            registry: None,
            namespace: None,
            archive_sha256: None,
            archive_length: None,
            manifest_sha256: None,
            provenance_sha256: None,
            package_signature_sha256: None,
            verification_sha256: None,
            publisher_identity: None,
            signer_key_ids: Vec::new(),
            cache_key: None,
            yanked_at_resolution: None,
            compatibility: compatibility(),
        });
        path_cycle
            .package
            .sort_by(|left, right| left.id.cmp(&right.id));
        path_cycle.edge.extend([
            LockedDependencyEdgeV2 {
                from: path_cycle.roots[0].clone(),
                to: utility_id.clone(),
                alias: "util".to_string(),
                requested: "1.0.0".to_string(),
                source_kind: LockedDependencySourceKind::Path,
                reason: LockedDependencyReason::RootPathConstraint,
            },
            LockedDependencyEdgeV2 {
                from: utility_id,
                to: path_cycle.roots[0].clone(),
                alias: "app".to_string(),
                requested: "1.0.0".to_string(),
                source_kind: LockedDependencySourceKind::Path,
                reason: LockedDependencyReason::TransitivePathConstraint,
            },
        ]);
        path_cycle
            .edge
            .sort_by(|left, right| edge_order_key(left).cmp(&edge_order_key(right)));
        let rendered = toml::to_string_pretty(&path_cycle).expect("serialize path cycle");
        assert!(
            parse_lockfile_exact(rendered.as_bytes(), Path::new("axiom.lock"))
                .expect_err("path cycle must fail strict parsing")
                .message
                .contains("dependency graph contains a cycle")
        );

        let mut registry_cycle = sample_lockfile_v2();
        let registry_id = registry_cycle.package[1].id.clone();
        registry_cycle.edge.push(LockedDependencyEdgeV2 {
            from: registry_id.clone(),
            to: registry_id,
            alias: "self".to_string(),
            requested: "1.2.3".to_string(),
            source_kind: LockedDependencySourceKind::Registry,
            reason: LockedDependencyReason::ExactLockedReplay,
        });
        registry_cycle
            .edge
            .sort_by(|left, right| edge_order_key(left).cmp(&edge_order_key(right)));
        assert!(
            validate_lockfile_v2(&registry_cycle)
                .expect_err("registry cycle must fail")
                .message
                .contains("dependency graph contains a cycle")
        );
    }

    #[test]
    fn lockfile_v2_accepts_dag_diamonds_across_multiple_roots() {
        let mut lockfile = sample_lockfile_v2();
        lockfile.registry.clear();
        lockfile.package.truncate(1);
        lockfile.edge.clear();
        let path_package = |name: &str, source: &str| LockedPackageV2 {
            id: canonical_path_package_id(source, name, "1.0.0"),
            name: name.to_string(),
            version: "1.0.0".to_string(),
            source: source.to_string(),
            registry: None,
            namespace: None,
            archive_sha256: None,
            archive_length: None,
            manifest_sha256: None,
            provenance_sha256: None,
            package_signature_sha256: None,
            verification_sha256: None,
            publisher_identity: None,
            signer_key_ids: Vec::new(),
            cache_key: None,
            yanked_at_resolution: None,
            compatibility: compatibility(),
        };
        let tool = path_package("tool", "path:tools/tool");
        let left = path_package("left", "path:deps/left");
        let right = path_package("right", "path:deps/right");
        let leaf = path_package("leaf", "path:deps/leaf");
        lockfile.roots.push(tool.id.clone());
        lockfile.roots.sort();
        lockfile
            .package
            .extend([tool.clone(), left.clone(), right.clone(), leaf.clone()]);
        lockfile
            .package
            .sort_by(|left, right| left.id.cmp(&right.id));
        lockfile.edge.extend([
            LockedDependencyEdgeV2 {
                from: lockfile.roots[0].clone(),
                to: left.id.clone(),
                alias: "left".to_string(),
                requested: "1.0.0".to_string(),
                source_kind: LockedDependencySourceKind::Path,
                reason: LockedDependencyReason::RootPathConstraint,
            },
            LockedDependencyEdgeV2 {
                from: lockfile.roots[0].clone(),
                to: right.id.clone(),
                alias: "right".to_string(),
                requested: "1.0.0".to_string(),
                source_kind: LockedDependencySourceKind::Path,
                reason: LockedDependencyReason::RootPathConstraint,
            },
            LockedDependencyEdgeV2 {
                from: tool.id,
                to: right.id.clone(),
                alias: "right".to_string(),
                requested: "1.0.0".to_string(),
                source_kind: LockedDependencySourceKind::Path,
                reason: LockedDependencyReason::RootPathConstraint,
            },
            LockedDependencyEdgeV2 {
                from: left.id,
                to: leaf.id.clone(),
                alias: "leaf".to_string(),
                requested: "1.0.0".to_string(),
                source_kind: LockedDependencySourceKind::Path,
                reason: LockedDependencyReason::TransitivePathConstraint,
            },
            LockedDependencyEdgeV2 {
                from: right.id,
                to: leaf.id,
                alias: "leaf".to_string(),
                requested: "1.0.0".to_string(),
                source_kind: LockedDependencySourceKind::Path,
                reason: LockedDependencyReason::TransitivePathConstraint,
            },
        ]);
        lockfile
            .edge
            .sort_by(|left, right| edge_order_key(left).cmp(&edge_order_key(right)));

        validate_lockfile_v2(&lockfile).expect("multiple-root DAG diamond should validate");
    }

    #[test]
    fn load_either_preserves_v1_and_requires_v2_for_registry_graphs() {
        let v1 =
            b"version = 1\n\n[[package]]\nname = \"app\"\nversion = \"1.0.0\"\nsource = \"path\"\n";
        let parsed =
            parse_lockfile_exact(v1, Path::new("axiom.lock")).expect("strict v1 should parse");
        assert!(matches!(parsed, ParsedLockfile::V1(_)));

        let manifest = crate::manifest::parse_manifest_exact(
            br#"
[package]
name = "app"
version = "1.0.0"

[registry]
name = "primary"
index = "https://registry.example.test/index.json"
trust_roots = "roots.json"
expectation = "expectation.json"

[dependencies.math]
registry = "primary"
namespace = "acme"
version = "^1.2.0"
"#,
            Path::new("axiom.toml"),
        )
        .expect("registry manifest");
        let error = validate_lockfile_version_for_manifest(&manifest, &parsed)
            .expect_err("registry graph with v1 must fail");
        assert_eq!(error.message, REGISTRY_LOCKFILE_V2_REQUIRED);
    }

    #[test]
    fn lockfile_v2_rejects_hostname_loopback_and_accepts_numeric_loopback() {
        let mut lockfile = sample_lockfile_v2();
        lockfile.registry[0].source = "http://localhost:8080/index.json".to_string();
        assert!(
            validate_lockfile_v2(&lockfile)
                .expect_err("hostname loopback must fail")
                .message
                .contains("unsupported or non-canonical source")
        );

        lockfile.registry[0].source = "http://127.0.0.1:8080/index.json".to_string();
        validate_lockfile_v2(&lockfile).expect("numeric IPv4 loopback should validate");

        lockfile.registry[0].source = "http://[::1]:8080/index.json".to_string();
        validate_lockfile_v2(&lockfile).expect("numeric IPv6 loopback should validate");
    }

    #[test]
    fn resolver_url_schemas_reject_hostname_and_malformed_numeric_loopback() {
        let manifest_schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../schemas/axiom.toml.schema.json"))
                .expect("parse manifest schema");
        let manifest_validator =
            jsonschema::validator_for(&manifest_schema).expect("compile manifest schema");
        let manifest_fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../package-resolver/fixtures/manifest-registry.json"
        ))
        .expect("parse manifest fixture");
        assert!(manifest_validator.is_valid(&manifest_fixture));

        for invalid in [
            "http://localhost:8080/index.json",
            "http://192.0.2.1:8080/index.json",
            "http://127.0.0.999:8080/index.json",
            "http://127.0.0.1:99999/index.json",
        ] {
            let mut value = manifest_fixture.clone();
            value["registry"]["index"] = serde_json::Value::String(invalid.to_string());
            assert!(
                !manifest_validator.is_valid(&value),
                "manifest schema accepted invalid registry URL {invalid:?}"
            );
        }
        for valid in [
            "http://127.0.0.1:8080/index.json",
            "http://127.255.255.255:65535/index.json",
            "http://[::1]:8080/index.json",
        ] {
            let mut value = manifest_fixture.clone();
            value["registry"]["index"] = serde_json::Value::String(valid.to_string());
            assert!(
                manifest_validator.is_valid(&value),
                "manifest schema rejected numeric loopback URL {valid:?}"
            );
        }

        let lock_schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../schemas/axiom-lockfile-v2.schema.json"
        ))
        .expect("parse lockfile schema");
        let lock_validator =
            jsonschema::validator_for(&lock_schema).expect("compile lockfile schema");
        let lock_fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../package-resolver/fixtures/lockfile-v2.json"
        ))
        .expect("parse lockfile fixture");
        assert!(lock_validator.is_valid(&lock_fixture));

        for invalid in [
            "http://localhost:8080/index.json",
            "http://192.0.2.1:8080/index.json",
            "http://127.0.0.999:8080/index.json",
            "http://127.0.0.1:99999/index.json",
        ] {
            let mut value = lock_fixture.clone();
            value["registry"][0]["source"] = serde_json::Value::String(invalid.to_string());
            assert!(
                !lock_validator.is_valid(&value),
                "lockfile schema accepted invalid registry URL {invalid:?}"
            );
        }
        for valid in [
            "http://127.0.0.1:8080/index.json",
            "http://127.255.255.255:65535/index.json",
            "http://[::1]:8080/index.json",
        ] {
            let mut value = lock_fixture.clone();
            value["registry"][0]["source"] = serde_json::Value::String(valid.to_string());
            assert!(
                lock_validator.is_valid(&value),
                "lockfile schema rejected numeric loopback URL {valid:?}"
            );
        }

        let mut invalid_version = lock_fixture.clone();
        invalid_version["package"][0]["version"] = serde_json::json!("1.0");
        assert!(
            !lock_validator.is_valid(&invalid_version),
            "lockfile schema accepted a non-release path-package version"
        );
        for invalid in [
            "path:",
            "path:.",
            "path:./child",
            "path:../outside",
            "path:child/../outside",
            "path:/absolute",
            "path:child\\nested",
            "path:child//nested",
            "path:child/",
        ] {
            let mut value = lock_fixture.clone();
            value["package"][0]["source"] = serde_json::Value::String(invalid.to_string());
            assert!(
                !lock_validator.is_valid(&value),
                "lockfile schema accepted non-canonical path source {invalid:?}"
            );
        }
    }

    #[test]
    fn bounded_lock_load_returns_digest_of_exact_v1_bytes() {
        let dir = tempdir().expect("tempdir");
        let content =
            b"version = 1\n\n[[package]]\nname = \"app\"\nversion = \"1.0.0\"\nsource = \"path\"\n";
        fs::write(lockfile_path(dir.path()), content).expect("write v1 lockfile");
        let (loaded, sha256) =
            load_lockfile_with_sha256(dir.path()).expect("load bounded v1 lockfile");
        assert!(matches!(loaded, ParsedLockfile::V1(_)));
        assert_eq!(
            sha256,
            "284c411ba5cdff2ad7703951b23acef592150ca832b61ff263c24b46f21084d4"
        );
        assert_eq!(
            load_lockfile(dir.path()).expect("compatibility loader"),
            loaded
        );
    }

    #[test]
    fn bounded_lock_load_rejects_oversized_files_before_parsing() {
        let dir = tempdir().expect("tempdir");
        fs::write(
            lockfile_path(dir.path()),
            vec![b' '; MAX_LOCKFILE_BYTES + 1],
        )
        .expect("write oversized lockfile");
        let error =
            load_lockfile_with_sha256(dir.path()).expect_err("oversized lockfile must fail");
        assert!(
            error
                .message
                .contains("exceeds the 4194304 byte parsing limit"),
            "unexpected diagnostic: {error:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn bounded_lock_load_rejects_final_component_symlinks() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().expect("tempdir");
        let real = dir.path().join("real.lock");
        fs::write(
            &real,
            b"version = 1\n\n[[package]]\nname = \"app\"\nversion = \"1.0.0\"\nsource = \"path\"\n",
        )
        .expect("write real lockfile");
        symlink(&real, lockfile_path(dir.path())).expect("create lockfile symlink");
        let error =
            load_lockfile_with_sha256(dir.path()).expect_err("lockfile symlink must fail closed");
        assert!(
            error.message.contains("failed to securely open axiom.lock"),
            "unexpected diagnostic: {error:?}"
        );
    }

    #[test]
    fn atomic_write_preserves_existing_lockfile_when_validation_fails() {
        let dir = tempdir().expect("tempdir");
        let path = lockfile_path(dir.path());
        fs::write(&path, "original lockfile\n").expect("write original");
        let mut invalid = sample_lockfile_v2();
        invalid.registry[0].index_generation = 0;
        let error =
            write_lockfile_v2_atomic(dir.path(), &invalid).expect_err("invalid v2 must not write");
        assert!(error.message.contains("greater than zero"));
        assert_eq!(
            fs::read_to_string(&path).expect("read preserved lock"),
            "original lockfile\n"
        );
        assert!(fs::read_dir(dir.path()).expect("read dir").all(|entry| {
            !entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".axiom.lock.tmp.")
        }));
    }

    #[test]
    fn conditional_atomic_write_requires_expected_presence_and_digest() {
        let dir = tempdir().expect("tempdir");
        let path = lockfile_path(dir.path());
        let first = sample_lockfile_v2();
        write_lockfile_v2_atomic_cas(dir.path(), &first, None)
            .expect("expected-absence write should create lockfile");
        let original = fs::read(&path).expect("read original lockfile");
        let (_loaded, original_sha256) =
            load_lockfile_with_sha256(dir.path()).expect("load original lock identity");

        let mut replacement = first.clone();
        replacement.compatibility.compiler = "axiomc-0.3.1".to_string();
        for package in &mut replacement.package {
            package.compatibility.compiler = "axiomc-0.3.1".to_string();
        }
        let error = write_lockfile_v2_atomic_cas(dir.path(), &replacement, None)
            .expect_err("expected absence must reject an existing lockfile");
        assert!(error.message.contains("changed while package resolution"));
        assert_eq!(fs::read(&path).expect("read preserved lockfile"), original);

        write_lockfile_v2_atomic_cas(dir.path(), &replacement, Some(&original_sha256))
            .expect("matching digest should replace lockfile");
        let replaced = fs::read(&path).expect("read replaced lockfile");
        assert_ne!(replaced, original);

        let error = write_lockfile_v2_atomic_cas(dir.path(), &first, Some(&original_sha256))
            .expect_err("stale digest must reject a changed lockfile");
        assert!(error.message.contains("changed while package resolution"));
        assert_eq!(
            fs::read(&path).expect("read preserved replacement"),
            replaced
        );
        assert!(fs::read_dir(dir.path()).expect("read dir").all(|entry| {
            !entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".axiom.lock.tmp.")
        }));
    }

    #[test]
    fn conditional_atomic_write_compares_immediately_before_replace() {
        let dir = tempdir().expect("tempdir");
        let path = lockfile_path(dir.path());
        let original = sample_lockfile_v2();
        write_lockfile_v2_atomic(dir.path(), &original).expect("write original lockfile");
        let (_loaded, original_sha256) =
            load_lockfile_with_sha256(dir.path()).expect("load original identity");
        let concurrent = b"concurrent operator edit\n";

        let error = write_lockfile_v2_atomic_cas_impl(
            dir.path(),
            &original,
            Some(&original_sha256),
            || {
                fs::write(&path, concurrent).map_err(|err| {
                    lockfile_error(&path, format!("failed to simulate concurrent edit: {err}"))
                })
            },
        )
        .expect_err("concurrent edit immediately before compare must fail closed");
        assert!(error.message.contains("changed while package resolution"));
        assert_eq!(
            fs::read(&path).expect("read concurrent edit"),
            concurrent,
            "CAS failure must preserve the concurrent writer's bytes"
        );
        assert!(fs::read_dir(dir.path()).expect("read dir").all(|entry| {
            !entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".axiom.lock.tmp.")
        }));
    }

    #[test]
    fn atomic_write_commits_a_strict_v2_lockfile() {
        let dir = tempdir().expect("tempdir");
        let expected = sample_lockfile_v2();
        write_lockfile_v2_atomic(dir.path(), &expected).expect("atomic write");
        assert_eq!(
            load_lockfile(dir.path()).expect("load written lock"),
            ParsedLockfile::V2(expected)
        );
    }
}
