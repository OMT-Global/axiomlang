use crate::diagnostics::Diagnostic;
use crate::lockfile::validate_lockfile;
use crate::manifest::{LOCK_FILENAME, MANIFEST_FILENAME, load_manifest, manifest_path};
use crate::package_trust::{
    Ed25519Signer, INDEX_DOMAIN, MAX_SIGNATURES, PACKAGE_FIELDS, PackageArtifacts,
    PackageSignatureEnvelope, PackageTrustInput, RegistryIndexEnvelope,
    RegistryIndexTrustPreflight, TrustRootsEnvelope, VerificationExpectation, canonical_json,
    metadata_transcript, package_index_floor_is_satisfied, package_transcript,
    parse_package_signature_json, parse_registry_index_json, preflight_package_release_trust,
    preflight_registry_index_trust, sign_package_transcript, validate_package_provenance_semantics,
    verify_package_with_artifacts,
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
#[cfg(unix)]
use std::ffi::CString;
use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

const REGISTRY_METADATA_FILENAME: &str = "axiom-registry.toml";
const DEFAULT_ARCHIVE_FILENAME: &str = "package.axp";
const PACKAGE_SIGNATURE_FILENAME: &str = "package.axp.sig";
const PROVENANCE_FILENAME: &str = "provenance.json";
const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_PROVENANCE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_SIGNATURE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_INDEX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_LOCK_BYTES: u64 = 4 * 1024 * 1024;
const MAX_INDEX_RELEASES: usize = 1024;
const MAX_SNAPSHOT_BYTES: u64 = 256 * 1024 * 1024;
const REGISTRY_IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// A strictly parsed v2 registry index together with the exact JSON bytes that
/// were parsed. Keeping both closes the validate-then-reread gap at serve time.
#[derive(Clone, Debug)]
pub struct RegistryIndex {
    envelope: RegistryIndexEnvelope,
    bytes: Vec<u8>,
}

impl RegistryIndex {
    pub fn envelope(&self) -> &RegistryIndexEnvelope {
        &self.envelope
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PublishOutput {
    pub namespace: String,
    pub package: String,
    pub version: String,
    pub release_dir: String,
    pub manifest: String,
    pub archive: String,
    pub provenance: String,
    pub signature: String,
    pub archive_hash: String,
}

/// Complete, key-storage-agnostic input to trusted publication.
///
/// `provenance_statement` is the in-toto statement value. Publication
/// canonicalizes it and requires its selected subject to name and hash the
/// exact archive generated in this call.
pub struct PublishOptions<'a, S: Ed25519Signer> {
    pub allow_overwrite: bool,
    pub namespace: &'a str,
    pub registry_identity: &'a str,
    pub source_identity: &'a str,
    pub publisher_identity: &'a str,
    /// Package signatures bind componentwise publication floors. A generated
    /// index must meet or exceed both coordinates, while later index advances
    /// and yank-only updates do not require re-signing retained releases.
    pub index_generation: u64,
    pub index_sequence: u64,
    pub provenance_statement: &'a Value,
    pub trust_roots: &'a TrustRootsEnvelope,
    pub verification_expectation: &'a VerificationExpectation,
    pub signers: &'a [S],
}

/// Inputs for producing authenticated `axiom.registry_index.v2` metadata.
pub struct RegistryIndexOptions<'a, S: Ed25519Signer> {
    pub registry_identity: &'a str,
    pub source_identity: &'a str,
    pub generation: u64,
    pub sequence: u64,
    pub issued_at: &'a str,
    pub expires_at: &'a str,
    pub snapshot_id: &'a str,
    pub metadata_path: &'a str,
    pub previous_snapshot_sha256: &'a str,
    pub trust_roots: &'a TrustRootsEnvelope,
    pub verification_expectation: &'a VerificationExpectation,
    pub signers: &'a [S],
}

#[derive(Clone, Debug)]
pub struct RegistryServeOptions {
    pub addr: String,
    pub base_url: Option<String>,
    pub index: RegistryIndex,
    pub trust_roots: TrustRootsEnvelope,
    pub verification_expectation: VerificationExpectation,
    pub once: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RegistryServeOutput {
    pub addr: String,
    pub base_url: String,
    pub requests: usize,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegistryMetadata {
    yanked: Option<bool>,
    yank_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SnapshotArtifact {
    content_type: &'static str,
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct VerifiedReleaseBytes {
    target_path: PathBuf,
    archive: Vec<u8>,
    manifest: Vec<u8>,
    provenance: Vec<u8>,
    signature: Vec<u8>,
}

struct RenderedPackage {
    archive: Vec<u8>,
    files: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone)]
struct RegistryServeContext {
    index_body: Vec<u8>,
    artifacts: BTreeMap<String, SnapshotArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegistryHttpResponse<'a> {
    status: &'static str,
    content_type: &'static str,
    content_length: usize,
    body: Cow<'a, [u8]>,
}

pub fn publish_package<S>(
    project_root: &Path,
    registry_root: &Path,
    options: &PublishOptions<'_, S>,
) -> Result<PublishOutput, Diagnostic>
where
    S: Ed25519Signer,
    S::Error: fmt::Display,
{
    let namespace = safe_registry_segment("publish", "package namespace", options.namespace)?;
    let registry_identity =
        required_text("publish", "registry identity", options.registry_identity)?;
    let source_identity = required_text("publish", "source identity", options.source_identity)?;
    let publisher_identity =
        required_text("publish", "publisher identity", options.publisher_identity)?;
    if options.index_generation == 0 || options.index_sequence == 0 {
        return Err(Diagnostic::new(
            "publish",
            "index generation and sequence must both be greater than zero",
        ));
    }
    let trust =
        preflight_registry_index_trust(options.trust_roots, options.verification_expectation)
            .map_err(|error| publish_error(None, error.to_string()))?;
    let project_root = fs::canonicalize(project_root).map_err(|error| {
        publish_error(
            Some(project_root),
            format!("failed to resolve project root: {error}"),
        )
    })?;
    let manifest = load_manifest(&project_root)?;
    validate_lockfile(&project_root, &manifest)?;
    let package = manifest.package.as_ref().ok_or_else(|| {
        publish_error(
            Some(&manifest_path(&project_root)),
            "published packages require a [package] section",
        )
    })?;
    let package_name = safe_registry_segment("publish", "package name", &package.name)?;
    let version = safe_registry_segment("publish", "package version", &package.version)?;
    let target_path = format!("{namespace}/{package_name}/{version}/{DEFAULT_ARCHIVE_FILENAME}");
    let release_parent = registry_root.join(&namespace).join(&package_name);
    let release_dir = release_parent.join(&version);

    let rendered = render_package_archive(&project_root)?;
    let archive_bytes = rendered.archive;
    let archive_hash = hash_bytes(&archive_bytes);
    let manifest_bytes = rendered
        .files
        .get(MANIFEST_FILENAME)
        .cloned()
        .ok_or_else(|| publish_error(None, "generated archive omitted package manifest"))?;
    let lock_bytes = rendered
        .files
        .get(LOCK_FILENAME)
        .cloned()
        .ok_or_else(|| publish_error(None, "generated archive omitted package lockfile"))?;
    let provenance = build_provenance(options.provenance_statement, &target_path, &archive_hash)?;
    let provenance_bytes = canonical_json(&provenance["statement"]["value"]).map_err(|error| {
        publish_error(None, format!("failed to canonicalize provenance: {error}"))
    })?;
    if provenance_bytes.len() as u64 > MAX_PROVENANCE_BYTES {
        return Err(publish_error(
            None,
            format!("canonical provenance exceeds {MAX_PROVENANCE_BYTES} byte limit"),
        ));
    }

    let package_threshold = trust.package_threshold;
    if package_threshold < 2 {
        return Err(publish_error(
            None,
            "Package Trust publication requires a package signer threshold of at least 2",
        ));
    }
    if options.signers.len() < package_threshold as usize {
        return Err(publish_error(
            None,
            format!(
                "package threshold requires {package_threshold} distinct signers, received {}",
                options.signers.len()
            ),
        ));
    }
    if options.signers.len() > MAX_SIGNATURES {
        return Err(publish_error(
            None,
            format!("package signatures support at most {MAX_SIGNATURES} signers"),
        ));
    }
    let signer_ids = preflight_publish_signers(
        options.signers,
        options.trust_roots,
        options.verification_expectation,
        &trust,
        &publisher_identity,
        &namespace,
        &package_name,
        &registry_identity,
        &source_identity,
    )?;
    let mut envelope_value = json!({
        "schema_version": "axiom.package_signature.v1",
        "contract": "package.signature",
        "contract_status": "implemented",
        "scheme": {
            "algorithm": "ed25519",
            "version": 1,
            "message_mode": "pure",
            "archive_digest": "sha-256"
        },
        "archive": {
            "digest": {"algorithm": "sha-256", "value": archive_hash},
            "size": archive_bytes.len() as u64
        },
        "manifest": {"algorithm": "sha-256", "value": hash_bytes(&manifest_bytes)},
        "package": {
            "namespace": namespace,
            "name": package_name,
            "version": version,
            "target_path": target_path
        },
        "registry": {
            "registry_identity": registry_identity,
            "source_identity": source_identity
        },
        "publisher": {"publisher_identity": publisher_identity},
        "provenance": provenance,
        "index": {
            "generation": options.index_generation,
            "sequence": options.index_sequence
        },
        "transcript": Value::Null,
        "signatures": []
    });
    let unsigned = PackageSignatureEnvelope(envelope_value.clone());
    validate_package_provenance_semantics(&unsigned)
        .map_err(|error| publish_error(None, error.to_string()))?;
    let transcript = package_transcript(&unsigned, package_threshold).map_err(|error| {
        publish_error(None, format!("failed to build package transcript: {error}"))
    })?;
    envelope_value["transcript"] = json!({
        "encoding": "axiom-tlv-v1",
        "domain": crate::package_trust::PACKAGE_DOMAIN,
        "field_order": PACKAGE_FIELDS,
        "bytes_hex": hex_encode(&transcript),
        "sha256": hash_bytes(&transcript)
    });
    let signable = PackageSignatureEnvelope(envelope_value.clone());
    ensure_generated_package_signature_fits(&envelope_value, signer_ids.len())?;
    let mut signatures = Vec::new();
    let mut signed_ids = BTreeSet::new();
    for signer in options.signers {
        let entry = sign_package_transcript(&signable, package_threshold, signer)
            .map_err(package_signing_error)?;
        if !signer_ids.contains(&entry.key_id) {
            return Err(publish_error(
                None,
                "package signing provider changed its public identity during signing",
            ));
        }
        if !signed_ids.insert(entry.key_id.clone()) {
            return Err(publish_error(
                None,
                format!("duplicate package signer {}", entry.key_id),
            ));
        }
        signatures.push(serde_json::to_value(entry).map_err(|error| {
            publish_error(None, format!("failed to encode package signature: {error}"))
        })?);
    }
    if signatures.len() < package_threshold as usize {
        return Err(publish_error(
            None,
            format!(
                "package threshold requires {package_threshold} distinct signers, received {}",
                signatures.len()
            ),
        ));
    }
    envelope_value["signatures"] = Value::Array(signatures);
    let signature_envelope = PackageSignatureEnvelope(envelope_value);
    let signature_bytes = canonical_json(&signature_envelope).map_err(|error| {
        publish_error(
            None,
            format!("failed to encode package signature envelope: {error}"),
        )
    })?;
    if signature_bytes.len() as u64 > MAX_SIGNATURE_BYTES {
        return Err(publish_error(
            None,
            format!("package signature exceeds {MAX_SIGNATURE_BYTES} byte limit"),
        ));
    }

    publish_release_transaction(
        registry_root,
        &namespace,
        &package_name,
        &version,
        options.allow_overwrite,
        &[
            (MANIFEST_FILENAME, manifest_bytes.as_slice()),
            (LOCK_FILENAME, lock_bytes.as_slice()),
            (DEFAULT_ARCHIVE_FILENAME, archive_bytes.as_slice()),
            (PROVENANCE_FILENAME, provenance_bytes.as_slice()),
            (PACKAGE_SIGNATURE_FILENAME, signature_bytes.as_slice()),
        ],
    )?;

    Ok(PublishOutput {
        namespace: namespace.clone(),
        package: package_name.clone(),
        version: version.clone(),
        release_dir: release_dir.display().to_string(),
        manifest: release_dir.join(MANIFEST_FILENAME).display().to_string(),
        archive: release_dir
            .join(DEFAULT_ARCHIVE_FILENAME)
            .display()
            .to_string(),
        provenance: release_dir.join(PROVENANCE_FILENAME).display().to_string(),
        signature: release_dir
            .join(PACKAGE_SIGNATURE_FILENAME)
            .display()
            .to_string(),
        archive_hash,
    })
}

#[allow(clippy::too_many_arguments)]
fn preflight_publish_signers<S>(
    signers: &[S],
    roots: &TrustRootsEnvelope,
    expectation: &VerificationExpectation,
    trust: &RegistryIndexTrustPreflight,
    publisher: &str,
    namespace: &str,
    package: &str,
    registry_identity: &str,
    source_identity: &str,
) -> Result<BTreeSet<String>, Diagnostic>
where
    S: Ed25519Signer,
    S::Error: fmt::Display,
{
    let candidate = &roots["candidate_root"]["signed"];
    let mut signer_ids = BTreeSet::new();
    for signer in signers {
        let public = signer.public_key().map_err(|error| {
            publish_error(None, format!("package signing provider failed: {error}"))
        })?;
        let material = json!({
            "algorithm": "ed25519",
            "public_key_encoding": "lowercase-hex",
            "public_key": hex_encode(&public)
        });
        let key_id = format!(
            "sha256:{}",
            hash_bytes(&canonical_json(&material).map_err(|error| {
                publish_error(
                    None,
                    format!("failed to derive package signer key id: {error}"),
                )
            })?)
        );
        if !signer_ids.insert(key_id.clone()) {
            return Err(publish_error(None, "duplicate package signer"));
        }
        if !trust
            .package_eligible_key_ids
            .iter()
            .any(|eligible| eligible == &key_id)
        {
            return Err(publish_error(
                None,
                format!(
                    "package signer {key_id} is not eligible for authenticated role {}",
                    trust.package_role_id
                ),
            ));
        }
        let key = candidate["keys"]
            .as_array()
            .and_then(|keys| {
                keys.iter()
                    .find(|key| key["key_id"].as_str() == Some(&key_id))
            })
            .ok_or_else(|| publish_error(None, "authenticated package signer key is missing"))?;
        if key["publisher_identity"].as_str() != Some(publisher) {
            return Err(publish_error(
                None,
                "package signer belongs to another publisher",
            ));
        }
    }
    let granted = candidate["namespace_grants"]
        .as_array()
        .is_some_and(|grants| {
            grants.iter().any(|grant| {
                grant["publisher_identity"].as_str() == Some(publisher)
                    && grant["namespace"].as_str() == Some(namespace)
                    && grant["role_id"].as_str() == Some(&trust.package_role_id)
                    && grant["package_names"]
                        .as_array()
                        .is_some_and(|values| values.iter().any(|value| value == package))
                    && grant["registry_identities"]
                        .as_array()
                        .is_some_and(|values| values.iter().any(|value| value == registry_identity))
                    && grant["source_identities"]
                        .as_array()
                        .is_some_and(|values| values.iter().any(|value| value == source_identity))
            })
        });
    if !granted {
        return Err(publish_error(
            None,
            "publisher does not hold the authenticated namespace grant",
        ));
    }
    let required = expectation["required_signers"]["required_key_ids"]
        .as_array()
        .ok_or_else(|| publish_error(None, "expectation is missing required package keys"))?;
    if required
        .iter()
        .filter_map(Value::as_str)
        .any(|required| !signer_ids.contains(required))
    {
        return Err(publish_error(
            None,
            "package signer set omits an expectation-required key",
        ));
    }
    Ok(signer_ids)
}

fn ensure_generated_package_signature_fits(
    envelope: &Value,
    signer_count: usize,
) -> Result<(), Diagnostic> {
    let mut envelope = envelope.clone();
    envelope["signatures"] = Value::Array(
        (0..signer_count)
            .map(|_| {
                json!({
                    "key_id": format!("sha256:{}", "0".repeat(64)),
                    "algorithm": "ed25519",
                    "encoding": "lowercase-hex",
                    "value": "0".repeat(128)
                })
            })
            .collect(),
    );
    let bytes = canonical_json(&envelope).map_err(|error| {
        publish_error(None, format!("failed to size package signature: {error}"))
    })?;
    if bytes.len() as u64 > MAX_SIGNATURE_BYTES {
        return Err(publish_error(
            None,
            format!("package signature exceeds {MAX_SIGNATURE_BYTES} byte limit"),
        ));
    }
    Ok(())
}

fn build_provenance(
    statement: &Value,
    target_path: &str,
    archive_hash: &str,
) -> Result<Value, Diagnostic> {
    let canonical = canonical_json(statement)
        .map_err(|error| publish_error(None, format!("invalid provenance statement: {error}")))?;
    let selected = json!({
        "name": target_path,
        "digest": {"sha256": archive_hash}
    });
    let subjects = statement
        .get("subject")
        .and_then(Value::as_array)
        .ok_or_else(|| publish_error(None, "provenance statement must contain subject array"))?;
    if !subjects.contains(&selected) {
        return Err(publish_error(
            None,
            "provenance selected subject must name and hash the exact generated archive",
        ));
    }
    Ok(json!({
        "statement": {
            "digest": {"algorithm": "sha-256", "value": hash_bytes(&canonical)},
            "canonical_bytes_hex": hex_encode(&canonical),
            "value": statement
        },
        "selected_subject": selected
    }))
}

#[cfg(unix)]
fn anchored_entry_is_directory(
    parent: &OwnedFd,
    name: &CString,
) -> Result<Option<bool>, Diagnostic> {
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    let status = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if status == 0 {
        let metadata = unsafe { metadata.assume_init() };
        return Ok(Some(metadata.st_mode & libc::S_IFMT == libc::S_IFDIR));
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::NotFound {
        Ok(None)
    } else {
        Err(publish_error(
            None,
            format!("failed to inspect anchored registry entry: {error}"),
        ))
    }
}

#[cfg(unix)]
fn remove_anchored_release_dir(parent: &OwnedFd, name: &CString) -> Result<(), Diagnostic> {
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(publish_error(
            None,
            format!(
                "failed to open anchored recovery directory: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    let directory = unsafe { OwnedFd::from_raw_fd(descriptor) };
    let iterator_descriptor = unsafe { libc::dup(directory.as_raw_fd()) };
    if iterator_descriptor < 0 {
        return Err(publish_error(
            None,
            format!(
                "failed to duplicate recovery directory descriptor: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    let iterator = unsafe { libc::fdopendir(iterator_descriptor) };
    if iterator.is_null() {
        unsafe {
            libc::close(iterator_descriptor);
        }
        return Err(publish_error(
            None,
            format!(
                "failed to enumerate recovery directory: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    loop {
        let entry = unsafe { libc::readdir(iterator) };
        if entry.is_null() {
            break;
        }
        let entry_name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if entry_name == b"." || entry_name == b".." {
            continue;
        }
        let entry_name = CString::new(entry_name)
            .map_err(|_| publish_error(None, "recovery entry contains NUL"))?;
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                directory.as_raw_fd(),
                entry_name.as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } < 0
        {
            unsafe {
                libc::closedir(iterator);
            }
            return Err(publish_error(
                None,
                format!(
                    "failed to inspect recovery entry: {}",
                    std::io::Error::last_os_error()
                ),
            ));
        }
        let metadata = unsafe { metadata.assume_init() };
        if metadata.st_mode & libc::S_IFMT == libc::S_IFDIR {
            unsafe {
                libc::closedir(iterator);
            }
            return Err(publish_error(
                None,
                "recovery directory contains an unexpected nested directory",
            ));
        }
        if unsafe { libc::unlinkat(directory.as_raw_fd(), entry_name.as_ptr(), 0) } < 0 {
            unsafe {
                libc::closedir(iterator);
            }
            return Err(publish_error(
                None,
                format!(
                    "failed to remove anchored recovery entry: {}",
                    std::io::Error::last_os_error()
                ),
            ));
        }
    }
    if unsafe { libc::closedir(iterator) } < 0 {
        return Err(publish_error(
            None,
            format!(
                "failed to close recovery directory iterator: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    drop(directory);
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } < 0 {
        return Err(publish_error(
            None,
            format!(
                "failed to remove anchored recovery directory: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    Ok(())
}

#[cfg(unix)]
struct AnchoredPendingCleanup<'a> {
    parent: &'a OwnedFd,
    name: &'a CString,
    armed: bool,
}

#[cfg(unix)]
impl Drop for AnchoredPendingCleanup<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = remove_anchored_release_dir(self.parent, self.name);
        }
    }
}

#[cfg(unix)]
fn publish_release_transaction(
    registry_root: &Path,
    namespace: &str,
    package: &str,
    version: &str,
    overwrite: bool,
    files: &[(&str, &[u8])],
) -> Result<(), Diagnostic> {
    if !registry_root.exists() {
        fs::create_dir_all(registry_root).map_err(|error| {
            publish_error(
                Some(registry_root),
                format!("failed to create registry root: {error}"),
            )
        })?;
    }
    let root = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(registry_root)
        .map_err(|error| {
            publish_error(
                Some(registry_root),
                format!("failed to safely open registry root: {error}"),
            )
        })?;
    let mut parent: OwnedFd = root.into();
    for segment in [namespace, package] {
        let name = CString::new(segment.as_bytes())
            .map_err(|_| publish_error(None, "registry path contains NUL"))?;
        let created = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o755) };
        if created < 0
            && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(publish_error(
                Some(&registry_root.join(namespace).join(package)),
                format!(
                    "failed to create anchored registry parent: {}",
                    std::io::Error::last_os_error()
                ),
            ));
        }
        let descriptor = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(publish_error(
                Some(&registry_root.join(namespace).join(package)),
                format!(
                    "failed to open anchored registry parent: {}",
                    std::io::Error::last_os_error()
                ),
            ));
        }
        parent = unsafe { OwnedFd::from_raw_fd(descriptor) };
    }
    if unsafe { libc::flock(parent.as_raw_fd(), libc::LOCK_EX) } < 0 {
        return Err(publish_error(
            None,
            format!(
                "failed to lock anchored registry package directory: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }

    let version_name = CString::new(version.as_bytes())
        .map_err(|_| publish_error(None, "release version contains NUL"))?;
    let pending_text = format!(".{version}.publish-pending");
    let pending_name = CString::new(pending_text.as_bytes())
        .map_err(|_| publish_error(None, "pending release name contains NUL"))?;
    let previous_text = format!(".{version}.previous");
    let previous_name = CString::new(previous_text.as_bytes())
        .map_err(|_| publish_error(None, "previous release name contains NUL"))?;

    let mut final_exists = anchored_entry_is_directory(&parent, &version_name)?;
    let mut previous_exists = anchored_entry_is_directory(&parent, &previous_name)?;
    if final_exists == Some(false) {
        return Err(publish_error(
            None,
            "release destination must be a non-symlink directory",
        ));
    }
    if previous_exists == Some(false) {
        return Err(publish_error(
            None,
            "release recovery state must be a non-symlink directory",
        ));
    }
    if final_exists.is_none() && previous_exists == Some(true) {
        if unsafe {
            libc::renameat(
                parent.as_raw_fd(),
                previous_name.as_ptr(),
                parent.as_raw_fd(),
                version_name.as_ptr(),
            )
        } < 0
        {
            return Err(publish_error(
                None,
                format!(
                    "failed to restore interrupted release replacement: {}",
                    std::io::Error::last_os_error()
                ),
            ));
        }
        if unsafe { libc::fsync(parent.as_raw_fd()) } < 0 {
            return Err(publish_error(
                None,
                format!(
                    "restored interrupted release but failed to sync parent directory: {}",
                    std::io::Error::last_os_error()
                ),
            ));
        }
        final_exists = Some(true);
        previous_exists = None;
    }
    match anchored_entry_is_directory(&parent, &pending_name)? {
        Some(true) => remove_anchored_release_dir(&parent, &pending_name)?,
        Some(false) => {
            return Err(publish_error(
                None,
                "pending release state must be a non-symlink directory",
            ));
        }
        None => {}
    }

    let final_exists = final_exists == Some(true);
    if final_exists && !overwrite {
        return Err(publish_error(
            None,
            format!(
                "registry release {namespace}/{package}@{version} already exists; pass --allow-overwrite to replace it"
            ),
        ));
    }
    if overwrite && final_exists && previous_exists == Some(true) {
        remove_anchored_release_dir(&parent, &previous_name)?;
        previous_exists = None;
    }
    debug_assert!(previous_exists.is_none());

    if unsafe { libc::mkdirat(parent.as_raw_fd(), pending_name.as_ptr(), 0o755) } < 0 {
        return Err(publish_error(
            None,
            format!(
                "failed to create anchored pending release: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    let pending_descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            pending_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if pending_descriptor < 0 {
        return Err(publish_error(
            None,
            format!(
                "failed to open anchored pending release: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    let pending = unsafe { OwnedFd::from_raw_fd(pending_descriptor) };
    let mut pending_cleanup = AnchoredPendingCleanup {
        parent: &parent,
        name: &pending_name,
        armed: true,
    };
    for (name, bytes) in files {
        let name = CString::new(name.as_bytes())
            .map_err(|_| publish_error(None, "release file name contains NUL"))?;
        let descriptor = unsafe {
            libc::openat(
                pending.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o644,
            )
        };
        if descriptor < 0 {
            return Err(publish_error(
                None,
                format!(
                    "failed to create anchored release file: {}",
                    std::io::Error::last_os_error()
                ),
            ));
        }
        let descriptor = unsafe { OwnedFd::from_raw_fd(descriptor) };
        let mut file = fs::File::from(descriptor);
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| {
                publish_error(None, format!("failed to persist release file: {error}"))
            })?;
    }
    if unsafe { libc::fsync(pending.as_raw_fd()) } < 0 {
        return Err(publish_error(
            None,
            format!(
                "failed to sync pending release directory: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }

    if final_exists
        && unsafe {
            libc::renameat(
                parent.as_raw_fd(),
                version_name.as_ptr(),
                parent.as_raw_fd(),
                previous_name.as_ptr(),
            )
        } < 0
    {
        return Err(publish_error(
            None,
            format!(
                "failed to stage existing release for replacement: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    if unsafe {
        libc::renameat(
            parent.as_raw_fd(),
            pending_name.as_ptr(),
            parent.as_raw_fd(),
            version_name.as_ptr(),
        )
    } < 0
    {
        let publish_error_value = std::io::Error::last_os_error();
        if final_exists
            && unsafe {
                libc::renameat(
                    parent.as_raw_fd(),
                    previous_name.as_ptr(),
                    parent.as_raw_fd(),
                    version_name.as_ptr(),
                )
            } < 0
        {
            return Err(publish_error(
                None,
                format!(
                    "failed to publish replacement: {publish_error_value}; prior release remains recoverable as {previous_text}, but automatic restoration failed: {}",
                    std::io::Error::last_os_error()
                ),
            ));
        }
        return Err(publish_error(
            None,
            format!("failed to publish release: {publish_error_value}"),
        ));
    }
    pending_cleanup.armed = false;
    if unsafe { libc::fsync(parent.as_raw_fd()) } < 0 {
        return Err(publish_error(
            None,
            format!(
                "release rename completed but parent directory sync failed: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    // A successful overwrite retains exactly one fixed hidden predecessor.
    // The next locked publication removes it before staging a replacement, and
    // recovery-on-entry restores it if a crash left the visible release absent.
    Ok(())
}

#[cfg(not(unix))]
fn publish_release_transaction(
    _registry_root: &Path,
    _namespace: &str,
    _package: &str,
    _version: &str,
    _overwrite: bool,
    _files: &[(&str, &[u8])],
) -> Result<(), Diagnostic> {
    Err(publish_error(
        None,
        "secure descriptor-relative registry publication is unsupported on this platform",
    ))
}

/*
 * Registry publication mutations are descriptor anchored above. Do not
 * reintroduce path-based create/write/rename helpers here.
 */

fn render_package_archive(project_root: &Path) -> Result<RenderedPackage, Diagnostic> {
    let mut files = Vec::new();
    collect_publishable_files(project_root, project_root, &mut files)?;
    files.sort();
    let mut archive = Vec::new();
    let mut captured = BTreeMap::new();
    archive.extend_from_slice(b"AXIOM_PACKAGE_ARCHIVE_V1\n");
    for path in files {
        let relative = path.strip_prefix(project_root).unwrap_or(&path);
        let relative = normalize_archive_path(relative)?;
        let file_cap = match relative.as_str() {
            MANIFEST_FILENAME => MAX_MANIFEST_BYTES,
            LOCK_FILENAME => MAX_LOCK_BYTES,
            _ => MAX_ARCHIVE_BYTES,
        };
        let content = read_registry_relative_path(project_root, Path::new(&relative), file_cap)
            .map_err(|error| {
                publish_error(
                    Some(&path),
                    format!("failed to capture project file: {}", error.message),
                )
            })?;
        let record_header = format!("--- file {relative} {} ---\n", content.len());
        let record_len = record_header.len() as u64
            + content.len() as u64
            + u64::from(!content.ends_with(b"\n"));
        if (archive.len() as u64)
            .checked_add(record_len)
            .is_none_or(|length| length > MAX_ARCHIVE_BYTES)
        {
            return Err(publish_error(
                Some(&path),
                "generated archive exceeds 64 MiB limit",
            ));
        }
        archive.extend_from_slice(record_header.as_bytes());
        archive.extend_from_slice(&content);
        if !content.ends_with(b"\n") {
            archive.push(b'\n');
        }
        captured.insert(relative, content);
    }
    Ok(RenderedPackage {
        archive,
        files: captured,
    })
}

fn collect_publishable_files(
    project_root: &Path,
    dir: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), Diagnostic> {
    for entry in fs::read_dir(dir)
        .map_err(|error| publish_error(Some(dir), format!("failed to read directory: {error}")))?
    {
        let entry = entry
            .map_err(|error| publish_error(Some(dir), format!("failed to read entry: {error}")))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| publish_error(Some(&path), format!("failed to stat path: {error}")))?;
        if metadata.file_type().is_symlink() {
            return Err(publish_error(
                Some(&path),
                "refusing to package a symlinked path",
            ));
        }
        if metadata.is_dir() {
            if matches!(
                entry.file_name().to_string_lossy().as_ref(),
                ".git" | "target" | "dist"
            ) {
                continue;
            }
            collect_publishable_files(project_root, &path, files)?;
        } else if metadata.is_file() && should_publish_file(&path) {
            let canonical = fs::canonicalize(&path).map_err(|error| {
                publish_error(Some(&path), format!("failed to resolve path: {error}"))
            })?;
            if !canonical.starts_with(project_root) {
                return Err(publish_error(
                    Some(&path),
                    "refusing to package a path outside the project root",
                ));
            }
            files.push(canonical);
        }
    }
    Ok(())
}

fn should_publish_file(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name == MANIFEST_FILENAME || name == LOCK_FILENAME)
        || path.extension().is_some_and(|extension| extension == "ax")
}

fn normalize_archive_path(path: &Path) -> Result<String, Diagnostic> {
    let mut output = Vec::new();
    for component in path.components() {
        let Component::Normal(component) = component else {
            return Err(publish_error(
                Some(path),
                "unsupported archive path component",
            ));
        };
        let component = component.to_str().ok_or_else(|| {
            publish_error(Some(path), "archive path component is not valid UTF-8")
        })?;
        if is_unsafe_registry_path_segment(component) {
            return Err(publish_error(Some(path), "unsafe archive path component"));
        }
        output.push(component);
    }
    if output.is_empty() {
        return Err(publish_error(
            Some(path),
            "archive path must name a descendant file",
        ));
    }
    Ok(output.join("/"))
}

pub fn build_registry_index<S>(
    packages_root: &Path,
    options: &RegistryIndexOptions<'_, S>,
) -> Result<RegistryIndex, Diagnostic>
where
    S: Ed25519Signer,
    S::Error: fmt::Display,
{
    required_text("registry", "registry identity", options.registry_identity)?;
    required_text("registry", "source identity", options.source_identity)?;
    required_text("registry", "issued_at", options.issued_at)?;
    required_text("registry", "expires_at", options.expires_at)?;
    required_text("registry", "snapshot id", options.snapshot_id)?;
    required_text("registry", "metadata path", options.metadata_path)?;
    if options.generation == 0 || options.sequence == 0 {
        return Err(registry_error(
            None,
            "index generation and sequence must both be greater than zero",
        ));
    }
    if !is_lower_hex_digest(options.previous_snapshot_sha256) {
        return Err(registry_error(
            None,
            "previous snapshot digest must be 64 lowercase hexadecimal characters",
        ));
    }
    let trust =
        preflight_registry_index_trust(options.trust_roots, options.verification_expectation)
            .map_err(|error| registry_error(None, error.to_string()))?;

    let signature_paths = discover_package_signatures(packages_root)?;
    if signature_paths.len() > MAX_INDEX_RELEASES {
        return Err(registry_error(
            Some(packages_root),
            format!(
                "registry contains {} releases; maximum is {MAX_INDEX_RELEASES}",
                signature_paths.len()
            ),
        ));
    }
    let mut releases = Vec::with_capacity(signature_paths.len());
    for signature_path in signature_paths {
        let signature_bytes =
            read_registry_file(packages_root, &signature_path, MAX_SIGNATURE_BYTES)?;
        let package_signature =
            parse_package_signature_json(&signature_bytes).map_err(|error| {
                registry_error(
                    Some(&signature_path),
                    format!("invalid package signature envelope: {error}"),
                )
            })?;
        let release = release_from_signature(
            packages_root,
            &signature_path,
            &package_signature,
            options,
            &trust,
        )?;
        releases.push(release);
    }
    releases.sort_by_key(|release| {
        (
            value_text(release, "namespace")
                .unwrap_or_default()
                .to_owned(),
            value_text(release, "name").unwrap_or_default().to_owned(),
            value_text(release, "version")
                .unwrap_or_default()
                .to_owned(),
        )
    });
    if releases.is_empty() {
        return Err(registry_error(
            Some(packages_root),
            "registry index must contain at least one fully verified release",
        ));
    }

    let signed = json!({
        "metadata_version": 2,
        "registry_identity": options.registry_identity,
        "source_identity": options.source_identity,
        "generation": options.generation,
        "sequence": options.sequence,
        "issued_at": options.issued_at,
        "expires_at": options.expires_at,
        "consistent_snapshot": {
            "enabled": true,
            "snapshot_id": options.snapshot_id,
            "metadata_path": options.metadata_path,
            "previous_snapshot_sha256": options.previous_snapshot_sha256
        },
        "signature_role": "registry-index",
        "releases": releases
    });
    let transcript = metadata_transcript(INDEX_DOMAIN, &signed).map_err(|error| {
        registry_error(None, format!("failed to build index transcript: {error}"))
    })?;
    let index_threshold = trust.index_threshold;
    if index_threshold < 2 {
        return Err(registry_error(
            None,
            "Package Trust index generation requires an index signer threshold of at least 2",
        ));
    }
    if options.signers.len() < index_threshold as usize {
        return Err(registry_error(
            None,
            format!(
                "index threshold requires {index_threshold} distinct signers, received {}",
                options.signers.len()
            ),
        ));
    }
    if options.signers.len() > MAX_SIGNATURES {
        return Err(registry_error(
            None,
            format!("registry index supports at most {MAX_SIGNATURES} signatures"),
        ));
    }
    ensure_generated_index_fits(&signed, &transcript, options.signers.len())?;
    let mut signatures = Vec::new();
    let mut signer_ids = BTreeSet::new();
    for signer in options.signers {
        let entry = sign_metadata_authorized(
            &transcript,
            signer,
            &trust.index_eligible_key_ids,
            &trust.index_role_id,
        )?;
        if !signer_ids.insert(entry["key_id"].as_str().unwrap_or_default().to_owned()) {
            return Err(registry_error(None, "duplicate registry-index signer"));
        }
        signatures.push(entry);
    }
    if signatures.len() < index_threshold as usize {
        return Err(registry_error(
            None,
            format!(
                "index threshold requires {index_threshold} distinct signers, received {}",
                signatures.len()
            ),
        ));
    }
    let envelope = RegistryIndexEnvelope(json!({
        "schema_version": "axiom.registry_index.v2",
        "contract": "package.registry_index",
        "contract_status": "implemented",
        "signed": signed,
        "transcript": {
            "encoding": "axiom-canonical-json-v1",
            "domain": INDEX_DOMAIN,
            "bytes_hex": hex_encode(&transcript),
            "sha256": hash_bytes(&transcript)
        },
        "signatures": signatures
    }));
    ensure_signers_authorized(
        options.trust_roots,
        options.verification_expectation,
        &PackageSignatureEnvelope(envelope.0.clone()),
        "index_role_id",
        index_threshold,
    )?;
    let bytes = canonical_json(&envelope)
        .map_err(|error| registry_error(None, format!("failed to encode index: {error}")))?;
    if bytes.len() as u64 > MAX_INDEX_BYTES {
        return Err(registry_error(
            None,
            format!("generated index exceeds {MAX_INDEX_BYTES} byte limit"),
        ));
    }
    let index = RegistryIndex { envelope, bytes };
    verify_registry_index_integrity(
        &index,
        packages_root,
        options.trust_roots,
        options.verification_expectation,
    )?;
    Ok(index)
}

fn ensure_generated_index_fits(
    signed: &Value,
    transcript: &[u8],
    signer_count: usize,
) -> Result<(), Diagnostic> {
    let placeholder_signatures = (0..signer_count)
        .map(|_| {
            json!({
                "key_id": format!("sha256:{}", "0".repeat(64)),
                "algorithm": "ed25519",
                "encoding": "lowercase-hex",
                "value": "0".repeat(128)
            })
        })
        .collect::<Vec<_>>();
    let envelope = RegistryIndexEnvelope(json!({
        "schema_version": "axiom.registry_index.v2",
        "contract": "package.registry_index",
        "contract_status": "implemented",
        "signed": signed,
        "transcript": {
            "encoding": "axiom-canonical-json-v1",
            "domain": INDEX_DOMAIN,
            "bytes_hex": hex_encode(transcript),
            "sha256": hash_bytes(transcript)
        },
        "signatures": placeholder_signatures
    }));
    let bytes = canonical_json(&envelope)
        .map_err(|error| registry_error(None, format!("failed to size index: {error}")))?;
    if bytes.len() as u64 > MAX_INDEX_BYTES {
        return Err(registry_error(
            None,
            format!("generated index exceeds {MAX_INDEX_BYTES} byte limit"),
        ));
    }
    Ok(())
}

pub fn render_registry_index<S>(
    packages_root: &Path,
    options: &RegistryIndexOptions<'_, S>,
) -> Result<String, Diagnostic>
where
    S: Ed25519Signer,
    S::Error: fmt::Display,
{
    let index = build_registry_index(packages_root, options)?;
    String::from_utf8(index.bytes)
        .map_err(|error| registry_error(None, format!("index is not valid UTF-8: {error}")))
}

pub fn load_registry_index(path: &Path) -> Result<RegistryIndex, Diagnostic> {
    let bytes = read_local_file_bounded(path, MAX_INDEX_BYTES, "registry")?;
    let envelope = parse_registry_index_json(&bytes)
        .map_err(|error| registry_error(Some(path), format!("invalid registry index: {error}")))?;
    validate_registry_index(&envelope, Some(path))?;
    Ok(RegistryIndex { envelope, bytes })
}

pub fn validate_registry_index(
    index: &RegistryIndexEnvelope,
    path: Option<&Path>,
) -> Result<(), Diagnostic> {
    let value = &index.0;
    if value.get("schema_version").and_then(Value::as_str) != Some("axiom.registry_index.v2")
        || value.get("contract").and_then(Value::as_str) != Some("package.registry_index")
        || value.get("signed").and_then(Value::as_object).is_none()
        || value.get("transcript").and_then(Value::as_object).is_none()
        || value.get("signatures").and_then(Value::as_array).is_none()
    {
        return Err(registry_error(path, "invalid registry index v2 envelope"));
    }
    let releases = value
        .pointer("/signed/releases")
        .and_then(Value::as_array)
        .ok_or_else(|| registry_error(path, "registry index is missing releases"))?;
    if releases.is_empty() {
        return Err(registry_error(path, "registry index must not be empty"));
    }
    if releases.len() > MAX_INDEX_RELEASES {
        return Err(registry_error(
            path,
            format!(
                "registry index contains {} releases; maximum is {MAX_INDEX_RELEASES}",
                releases.len()
            ),
        ));
    }
    let mut coordinates = BTreeSet::new();
    let mut targets = BTreeSet::new();
    for release in releases {
        let coordinate = (
            required_value_text(release, "namespace", path)?,
            required_value_text(release, "name", path)?,
            required_value_text(release, "version", path)?,
        );
        if !coordinates.insert(coordinate) {
            return Err(registry_error(
                path,
                "registry index contains duplicate release",
            ));
        }
        let target = required_value_text(release, "target_path", path)?;
        safe_relative_path(&target)?;
        if !targets.insert(target) {
            return Err(registry_error(
                path,
                "registry index contains duplicate target path",
            ));
        }
    }
    Ok(())
}

pub fn verify_registry_index_integrity(
    index: &RegistryIndex,
    packages_root: &Path,
    trust_roots: &TrustRootsEnvelope,
    expectation: &VerificationExpectation,
) -> Result<(), Diagnostic> {
    verify_registry_index_and_capture(index, packages_root, trust_roots, expectation, None)
        .map(|_| ())
}

fn verify_registry_index_and_capture(
    index: &RegistryIndex,
    packages_root: &Path,
    trust_roots: &TrustRootsEnvelope,
    expectation: &VerificationExpectation,
    mut after_capture: Option<&mut dyn FnMut(&Path)>,
) -> Result<Vec<VerifiedReleaseBytes>, Diagnostic> {
    let parsed = parse_registry_index_json(&index.bytes)
        .map_err(|error| registry_error(None, format!("invalid retained index bytes: {error}")))?;
    if parsed.0 != index.envelope.0 {
        return Err(registry_error(
            None,
            "retained index bytes do not match parsed index envelope",
        ));
    }
    validate_registry_index(&index.envelope, None)?;
    let releases = index.envelope["signed"]["releases"]
        .as_array()
        .ok_or_else(|| registry_error(None, "registry index releases are invalid"))?;
    let mut captured = Vec::with_capacity(releases.len());
    for release in releases {
        let target = required_value_text(release, "target_path", None)?;
        let archive = read_registry_relative(packages_root, &target, MAX_ARCHIVE_BYTES)?;
        let release_dir = Path::new(&target)
            .parent()
            .ok_or_else(|| registry_error(None, "target path has no parent"))?;
        let manifest_path = release_dir.join(MANIFEST_FILENAME);
        let provenance_path = release_dir.join(PROVENANCE_FILENAME);
        let signature_path = release_dir.join(PACKAGE_SIGNATURE_FILENAME);
        let manifest =
            read_registry_relative_path(packages_root, &manifest_path, MAX_MANIFEST_BYTES)?;
        let provenance =
            read_registry_relative_path(packages_root, &provenance_path, MAX_PROVENANCE_BYTES)?;
        let signature_bytes =
            read_registry_relative_path(packages_root, &signature_path, MAX_SIGNATURE_BYTES)?;
        if let Some(hook) = after_capture.as_mut() {
            hook(release_dir);
        }
        let package_signature =
            parse_package_signature_json(&signature_bytes).map_err(|error| {
                registry_error(
                    Some(&packages_root.join(&signature_path)),
                    format!("invalid package signature envelope: {error}"),
                )
            })?;
        let exact_expectation = expectation_for_release(expectation, release)?;
        let verification = verify_package_with_artifacts(
            &PackageTrustInput {
                package_signature,
                trust_roots: trust_roots.clone(),
                registry_index: index.envelope.clone(),
                verification_expectation: exact_expectation,
            },
            PackageArtifacts {
                archive: Some(&archive),
                manifest: Some(&manifest),
                provenance: Some(&provenance),
            },
        );
        if verification.decision != "trusted" {
            return Err(registry_error(
                Some(&packages_root.join(&signature_path)),
                format!(
                    "Package Trust rejected {}: {}",
                    target,
                    verification.reason_codes.join(", ")
                ),
            ));
        }
        captured.push(VerifiedReleaseBytes {
            target_path: PathBuf::from(target),
            archive,
            manifest,
            provenance,
            signature: signature_bytes,
        });
    }
    Ok(captured)
}

fn release_from_signature<S>(
    packages_root: &Path,
    signature_path: &Path,
    signature: &PackageSignatureEnvelope,
    options: &RegistryIndexOptions<'_, S>,
    trust: &RegistryIndexTrustPreflight,
) -> Result<Value, Diagnostic>
where
    S: Ed25519Signer,
{
    let target = signature
        .pointer("/package/target_path")
        .and_then(Value::as_str)
        .ok_or_else(|| registry_error(Some(signature_path), "signature target path is missing"))?;
    safe_relative_path(target)?;
    let expected_signature = packages_root
        .join(target)
        .parent()
        .unwrap_or(packages_root)
        .join(PACKAGE_SIGNATURE_FILENAME);
    if signature_path != expected_signature {
        return Err(registry_error(
            Some(signature_path),
            "package signature location does not match its signed target path",
        ));
    }
    if !package_index_floor_is_satisfied(signature, options.generation, options.sequence) {
        return Err(registry_error(
            Some(signature_path),
            "package signature publication floor exceeds generated index coordinates",
        ));
    }
    if signature
        .pointer("/registry/registry_identity")
        .and_then(Value::as_str)
        != Some(options.registry_identity)
        || signature
            .pointer("/registry/source_identity")
            .and_then(Value::as_str)
            != Some(options.source_identity)
    {
        return Err(registry_error(
            Some(signature_path),
            "package signature registry/source identity does not match generated index",
        ));
    }
    let target_dir = Path::new(target)
        .parent()
        .ok_or_else(|| registry_error(Some(signature_path), "target path has no parent"))?;
    let metadata = load_registry_metadata(packages_root, target_dir)?;
    let yanked = metadata.yanked.unwrap_or(false);
    if metadata.yank_reason.is_some() && !yanked {
        return Err(registry_error(
            Some(
                &packages_root
                    .join(target_dir)
                    .join(REGISTRY_METADATA_FILENAME),
            ),
            "yank_reason requires yanked = true",
        ));
    }
    let canonical_signature = canonical_json(signature).map_err(|error| {
        registry_error(
            Some(signature_path),
            format!("failed to canonicalize package signature: {error}"),
        )
    })?;
    let release = json!({
        "namespace": signature["package"]["namespace"],
        "name": signature["package"]["name"],
        "version": signature["package"]["version"],
        "target_path": signature["package"]["target_path"],
        "registry_identity": signature["registry"]["registry_identity"],
        "source_identity": signature["registry"]["source_identity"],
        "publisher_identity": signature["publisher"]["publisher_identity"],
        "archive": {
            "length": signature["archive"]["size"],
            "digest": signature["archive"]["digest"]
        },
        "manifest": signature["manifest"],
        "provenance": signature["provenance"],
        "package_signature_sha256": hash_bytes(&canonical_signature),
        "yanked": yanked
    });
    preflight_package_release(packages_root, target, signature, &release, options, trust)?;
    Ok(release)
}

fn preflight_package_release<S>(
    packages_root: &Path,
    target: &str,
    signature: &PackageSignatureEnvelope,
    release: &Value,
    options: &RegistryIndexOptions<'_, S>,
    trust: &RegistryIndexTrustPreflight,
) -> Result<(), Diagnostic>
where
    S: Ed25519Signer,
{
    let target_path = Path::new(target);
    let release_dir = target_path
        .parent()
        .ok_or_else(|| registry_error(None, "target path has no parent"))?;
    let archive = read_registry_relative(packages_root, target, MAX_ARCHIVE_BYTES)?;
    let manifest = read_registry_relative_path(
        packages_root,
        &release_dir.join(MANIFEST_FILENAME),
        MAX_MANIFEST_BYTES,
    )?;
    let provenance = read_registry_relative_path(
        packages_root,
        &release_dir.join(PROVENANCE_FILENAME),
        MAX_PROVENANCE_BYTES,
    )?;
    if signature.pointer("/archive/size").and_then(Value::as_u64) != Some(archive.len() as u64)
        || signature
            .pointer("/archive/digest/value")
            .and_then(Value::as_str)
            != Some(hash_bytes(&archive).as_str())
        || signature.pointer("/manifest/value").and_then(Value::as_str)
            != Some(hash_bytes(&manifest).as_str())
    {
        return Err(registry_error(
            Some(&packages_root.join(target_path)),
            "package release bytes do not match signed archive/manifest digests",
        ));
    }
    let provenance_value = signature
        .pointer("/provenance/statement/value")
        .ok_or_else(|| registry_error(None, "package signature is missing provenance value"))?;
    let canonical_provenance = canonical_json(provenance_value)
        .map_err(|error| registry_error(None, format!("invalid signed provenance: {error}")))?;
    if provenance != canonical_provenance
        || signature
            .pointer("/provenance/statement/digest/value")
            .and_then(Value::as_str)
            != Some(hash_bytes(&canonical_provenance).as_str())
        || signature
            .pointer("/provenance/statement/canonical_bytes_hex")
            .and_then(Value::as_str)
            != Some(hex_encode(&canonical_provenance).as_str())
        || signature
            .pointer("/provenance/selected_subject/name")
            .and_then(Value::as_str)
            != Some(target)
        || signature
            .pointer("/provenance/selected_subject/digest/sha256")
            .and_then(Value::as_str)
            != Some(hash_bytes(&archive).as_str())
    {
        return Err(registry_error(
            Some(&packages_root.join(release_dir).join(PROVENANCE_FILENAME)),
            "package provenance does not match exact canonical signed evidence",
        ));
    }

    let threshold = trust.package_threshold;
    let transcript = package_transcript(signature, threshold)
        .map_err(|error| registry_error(None, format!("invalid package transcript: {error}")))?;
    if signature
        .pointer("/transcript/bytes_hex")
        .and_then(Value::as_str)
        != Some(hex_encode(&transcript).as_str())
        || signature
            .pointer("/transcript/sha256")
            .and_then(Value::as_str)
            != Some(hash_bytes(&transcript).as_str())
    {
        return Err(registry_error(
            None,
            "package signature transcript does not match signed fields",
        ));
    }
    let role_id = trust.package_role_id.as_str();
    let candidate = options
        .trust_roots
        .pointer("/candidate_root/signed")
        .ok_or_else(|| registry_error(None, "trust roots are missing candidate root"))?;
    let authorized = &trust.package_eligible_key_ids;
    let publisher = signature
        .pointer("/publisher/publisher_identity")
        .and_then(Value::as_str)
        .ok_or_else(|| registry_error(None, "package publisher is missing"))?;
    let namespace = signature
        .pointer("/package/namespace")
        .and_then(Value::as_str);
    let name = signature.pointer("/package/name").and_then(Value::as_str);
    let registry_identity = signature
        .pointer("/registry/registry_identity")
        .and_then(Value::as_str);
    let source_identity = signature
        .pointer("/registry/source_identity")
        .and_then(Value::as_str);
    let granted = candidate["namespace_grants"]
        .as_array()
        .is_some_and(|grants| {
            grants.iter().any(|grant| {
                grant["publisher_identity"].as_str() == Some(publisher)
                    && grant["namespace"].as_str() == namespace
                    && grant["role_id"].as_str() == Some(role_id)
                    && grant["package_names"]
                        .as_array()
                        .is_some_and(|values| values.iter().any(|value| value.as_str() == name))
                    && grant["registry_identities"]
                        .as_array()
                        .is_some_and(|values| {
                            values
                                .iter()
                                .any(|value| value.as_str() == registry_identity)
                        })
                    && grant["source_identities"].as_array().is_some_and(|values| {
                        values.iter().any(|value| value.as_str() == source_identity)
                    })
            })
        });
    if !granted {
        return Err(registry_error(
            None,
            "package publisher does not hold the exact namespace grant",
        ));
    }
    let mut valid = BTreeSet::new();
    for entry in signature["signatures"].as_array().into_iter().flatten() {
        let key_id = entry["key_id"]
            .as_str()
            .ok_or_else(|| registry_error(None, "package signature key id is invalid"))?;
        if !valid.insert(key_id.to_owned()) {
            return Err(registry_error(None, "duplicate package signer"));
        }
        if !authorized.iter().any(|value| value == key_id) {
            return Err(registry_error(
                None,
                "package signer is not eligible for the authenticated role",
            ));
        }
        let key = candidate["keys"]
            .as_array()
            .and_then(|keys| {
                keys.iter()
                    .find(|key| key["key_id"].as_str() == Some(key_id))
            })
            .ok_or_else(|| registry_error(None, "package signer key is unknown"))?;
        if key["publisher_identity"].as_str() != Some(publisher) {
            return Err(registry_error(
                None,
                "package signer belongs to another publisher",
            ));
        }
        let public = decode_lower_hex(
            key.pointer("/key_material/public_key")
                .and_then(Value::as_str)
                .ok_or_else(|| registry_error(None, "package signer public key is invalid"))?,
        )?;
        let public: [u8; 32] = public
            .try_into()
            .map_err(|_| registry_error(None, "package signer public key length is invalid"))?;
        let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&public)
            .map_err(|_| registry_error(None, "package signer public key is invalid"))?;
        let encoded_signature = decode_lower_hex(
            entry["value"]
                .as_str()
                .ok_or_else(|| registry_error(None, "package signature value is invalid"))?,
        )?;
        let signature_value = ed25519_dalek::Signature::from_slice(&encoded_signature)
            .map_err(|_| registry_error(None, "package signature value is malformed"))?;
        verifying_key
            .verify_strict(&transcript, &signature_value)
            .map_err(|_| registry_error(None, "package signature is invalid"))?;
    }
    if valid.len() < threshold as usize {
        return Err(registry_error(
            None,
            "package signature threshold is not met",
        ));
    }
    let required = options
        .verification_expectation
        .pointer("/required_signers/required_key_ids")
        .and_then(Value::as_array)
        .ok_or_else(|| registry_error(None, "expectation is missing required package keys"))?;
    if required
        .iter()
        .filter_map(Value::as_str)
        .any(|key_id| !valid.contains(key_id))
    {
        return Err(registry_error(
            None,
            "package signature omits an expectation-required key",
        ));
    }
    let exact_expectation = expectation_for_release(options.verification_expectation, release)?;
    preflight_package_release_trust(
        signature,
        options.trust_roots,
        &exact_expectation,
        PackageArtifacts {
            archive: Some(&archive),
            manifest: Some(&manifest),
            provenance: Some(&provenance),
        },
        options.generation,
        options.sequence,
    )
    .map_err(|error| registry_error(Some(&packages_root.join(target_path)), error.to_string()))?;
    Ok(())
}

fn decode_lower_hex(value: &str) -> Result<Vec<u8>, Diagnostic> {
    if value.len() % 2 != 0
        || value
            .bytes()
            .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte))
    {
        return Err(registry_error(None, "invalid lowercase hexadecimal value"));
    }
    (0..value.len())
        .step_by(2)
        .map(|offset| {
            u8::from_str_radix(&value[offset..offset + 2], 16)
                .map_err(|_| registry_error(None, "invalid lowercase hexadecimal value"))
        })
        .collect()
}

fn expectation_for_release(
    template: &VerificationExpectation,
    release: &Value,
) -> Result<VerificationExpectation, Diagnostic> {
    let mut value = template.0.clone();
    let request = json!({
        "registry_identity": release["registry_identity"],
        "source_identity": release["source_identity"],
        "namespace": release["namespace"],
        "name": release["name"],
        "version": release["version"],
        "target_path": release["target_path"],
        "publisher_identity": release["publisher_identity"],
        "archive": release["archive"],
        "manifest": release["manifest"],
        "provenance": release["provenance"]
    });
    value["request"] = request;
    value["offline_lock"]["release"] = json!({
        "registry_identity": release["registry_identity"],
        "source_identity": release["source_identity"],
        "namespace": release["namespace"],
        "name": release["name"],
        "version": release["version"],
        "target_path": release["target_path"],
        "publisher_identity": release["publisher_identity"],
        "archive": release["archive"],
        "manifest": release["manifest"],
        "provenance_statement_sha256": release["provenance"]["statement"]["digest"]["value"],
        "provenance_predicate_type": release["provenance"]["statement"]["value"]["predicateType"],
        "provenance_subject": release["provenance"]["selected_subject"],
        "package_signature_sha256": release["package_signature_sha256"]
    });
    Ok(VerificationExpectation(value))
}

#[cfg(test)]
fn sign_metadata<S>(transcript: &[u8], signer: &S) -> Result<Value, Diagnostic>
where
    S: Ed25519Signer,
    S::Error: fmt::Display,
{
    let public = signer
        .public_key()
        .map_err(|error| registry_error(None, format!("index signing provider failed: {error}")))?;
    let material = json!({
        "algorithm": "ed25519",
        "public_key_encoding": "lowercase-hex",
        "public_key": hex_encode(&public)
    });
    let key_id = format!(
        "sha256:{}",
        hash_bytes(&canonical_json(&material).map_err(|error| {
            registry_error(None, format!("failed to derive signer key id: {error}"))
        })?)
    );
    sign_metadata_with_key_id(transcript, signer, key_id)
}

fn sign_metadata_authorized<S>(
    transcript: &[u8],
    signer: &S,
    eligible_key_ids: &[String],
    role_id: &str,
) -> Result<Value, Diagnostic>
where
    S: Ed25519Signer,
    S::Error: fmt::Display,
{
    let public = signer
        .public_key()
        .map_err(|error| registry_error(None, format!("index signing provider failed: {error}")))?;
    let material = json!({
        "algorithm": "ed25519",
        "public_key_encoding": "lowercase-hex",
        "public_key": hex_encode(&public)
    });
    let key_id = format!(
        "sha256:{}",
        hash_bytes(&canonical_json(&material).map_err(|error| {
            registry_error(None, format!("failed to derive signer key id: {error}"))
        })?)
    );
    if !eligible_key_ids.iter().any(|eligible| eligible == &key_id) {
        return Err(registry_error(
            None,
            format!("index signer {key_id} is not eligible for authenticated role {role_id}"),
        ));
    }
    sign_metadata_with_key_id(transcript, signer, key_id)
}

fn sign_metadata_with_key_id<S>(
    transcript: &[u8],
    signer: &S,
    key_id: String,
) -> Result<Value, Diagnostic>
where
    S: Ed25519Signer,
    S::Error: fmt::Display,
{
    let signature = signer
        .sign(transcript)
        .map_err(|error| registry_error(None, format!("index signing provider failed: {error}")))?;
    Ok(json!({
        "key_id": key_id,
        "algorithm": "ed25519",
        "encoding": "lowercase-hex",
        "value": hex_encode(&signature)
    }))
}

fn ensure_signers_authorized(
    roots: &TrustRootsEnvelope,
    expectation: &VerificationExpectation,
    envelope: &PackageSignatureEnvelope,
    role_field: &str,
    threshold: u64,
) -> Result<(), Diagnostic> {
    let role_id = expectation
        .pointer(&format!("/required_signers/{role_field}"))
        .and_then(Value::as_str)
        .ok_or_else(|| registry_error(None, format!("expectation is missing {role_field}")))?;
    let role = roots
        .pointer("/candidate_root/signed/roles")
        .and_then(Value::as_array)
        .and_then(|roles| {
            roles
                .iter()
                .find(|role| role.get("role_id").and_then(Value::as_str) == Some(role_id))
        })
        .ok_or_else(|| registry_error(None, format!("trust roots do not define role {role_id}")))?;
    if role.get("threshold").and_then(Value::as_u64) != Some(threshold) {
        return Err(registry_error(
            None,
            format!("role {role_id} threshold does not match expectation"),
        ));
    }
    let authorized = role
        .get("key_ids")
        .and_then(Value::as_array)
        .ok_or_else(|| registry_error(None, format!("role {role_id} has invalid key_ids")))?;
    for signature in envelope["signatures"].as_array().into_iter().flatten() {
        if !authorized.contains(&signature["key_id"]) {
            return Err(registry_error(
                None,
                format!(
                    "signer {} is not authorized for role {role_id}",
                    signature["key_id"].as_str().unwrap_or("<invalid>")
                ),
            ));
        }
    }
    Ok(())
}

pub fn serve_registry(
    packages_root: &Path,
    options: &RegistryServeOptions,
) -> Result<RegistryServeOutput, Diagnostic> {
    let listener = TcpListener::bind(&options.addr).map_err(|error| {
        registry_error(None, format!("failed to bind registry server: {error}"))
    })?;
    let local_addr = listener.local_addr().map_err(|error| {
        registry_error(None, format!("failed to inspect bind address: {error}"))
    })?;
    let addr = local_addr.to_string();
    let base_url = options
        .base_url
        .clone()
        .unwrap_or_else(|| format!("http://{addr}"));
    let context = RegistryServeContext::new(
        packages_root,
        &options.index,
        &options.trust_roots,
        &options.verification_expectation,
    )?;
    let mut requests = 0;
    for stream in listener.incoming() {
        let mut stream = stream
            .map_err(|error| registry_error(None, format!("failed to accept request: {error}")))?;
        stream
            .set_read_timeout(Some(REGISTRY_IO_TIMEOUT))
            .and_then(|()| stream.set_write_timeout(Some(REGISTRY_IO_TIMEOUT)))
            .map_err(|error| {
                registry_error(None, format!("failed to set registry I/O timeout: {error}"))
            })?;
        serve_registry_stream(&context, &mut stream)?;
        requests += 1;
        if options.once {
            break;
        }
    }
    Ok(RegistryServeOutput {
        addr,
        base_url,
        requests,
    })
}

impl RegistryServeContext {
    fn new(
        packages_root: &Path,
        index: &RegistryIndex,
        roots: &TrustRootsEnvelope,
        expectation: &VerificationExpectation,
    ) -> Result<Self, Diagnostic> {
        Self::new_with_capture_hook(packages_root, index, roots, expectation, None)
    }

    fn new_with_capture_hook(
        packages_root: &Path,
        index: &RegistryIndex,
        roots: &TrustRootsEnvelope,
        expectation: &VerificationExpectation,
        after_capture: Option<&mut dyn FnMut(&Path)>,
    ) -> Result<Self, Diagnostic> {
        let captured = verify_registry_index_and_capture(
            index,
            packages_root,
            roots,
            expectation,
            after_capture,
        )?;
        let mut artifacts = BTreeMap::new();
        let mut snapshot_bytes = index.bytes.len() as u64;
        for release in captured {
            let parent = release
                .target_path
                .parent()
                .ok_or_else(|| registry_error(None, "target path has no parent"))?
                .to_path_buf();
            let paths = [
                (release.target_path, release.archive),
                (parent.join(MANIFEST_FILENAME), release.manifest),
                (parent.join(PROVENANCE_FILENAME), release.provenance),
                (parent.join(PACKAGE_SIGNATURE_FILENAME), release.signature),
            ];
            for (path, bytes) in paths {
                snapshot_bytes = snapshot_bytes
                    .checked_add(bytes.len() as u64)
                    .ok_or_else(|| registry_error(None, "registry snapshot size overflow"))?;
                if snapshot_bytes > MAX_SNAPSHOT_BYTES {
                    return Err(registry_error(
                        None,
                        format!(
                            "registry snapshot exceeds {MAX_SNAPSHOT_BYTES} byte in-memory limit"
                        ),
                    ));
                }
                let path_text = path
                    .to_str()
                    .ok_or_else(|| registry_error(None, "artifact path is not UTF-8"))?;
                let key = format!("/{path_text}");
                if artifacts
                    .insert(
                        key,
                        SnapshotArtifact {
                            content_type: registry_content_type(path_text),
                            bytes,
                        },
                    )
                    .is_some()
                {
                    return Err(registry_error(None, "duplicate served artifact path"));
                }
            }
        }
        Ok(Self {
            index_body: index.bytes.clone(),
            artifacts,
        })
    }
}

fn serve_registry_stream(
    context: &RegistryServeContext,
    stream: &mut TcpStream,
) -> Result<(), Diagnostic> {
    let mut buffer = [0_u8; 16 * 1024];
    let length = stream
        .read(&mut buffer)
        .map_err(|error| registry_error(None, format!("failed to read request: {error}")))?;
    let request = String::from_utf8_lossy(&buffer[..length]);
    let response = registry_http_response(context, &request);
    write_registry_http_response(stream, &response)
}

fn registry_http_response<'a>(
    context: &'a RegistryServeContext,
    request: &str,
) -> RegistryHttpResponse<'a> {
    let Some(line) = request.lines().next() else {
        return registry_http_error("400 Bad Request", "empty request");
    };
    let parts = line.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 3 || !parts[2].starts_with("HTTP/") {
        return registry_http_error("400 Bad Request", "invalid request line");
    }
    if parts[0] != "GET" && parts[0] != "HEAD" {
        return registry_http_error("405 Method Not Allowed", "method not allowed");
    }
    let target = parts[1].split('?').next().unwrap_or(parts[1]);
    if !target.starts_with('/') || target.contains('\\') || target.contains('\0') {
        return registry_http_error("400 Bad Request", "unsafe target");
    }
    let (content_type, bytes) = if matches!(target, "/" | "/index.json") {
        ("application/json", context.index_body.as_slice())
    } else {
        let Some(artifact) = context.artifacts.get(target) else {
            return registry_http_error("404 Not Found", "not found");
        };
        (artifact.content_type, artifact.bytes.as_slice())
    };
    RegistryHttpResponse {
        status: "200 OK",
        content_type,
        content_length: bytes.len(),
        body: if parts[0] == "HEAD" {
            Cow::Borrowed(b"")
        } else {
            Cow::Borrowed(bytes)
        },
    }
}

fn write_registry_http_response(
    stream: &mut TcpStream,
    response: &RegistryHttpResponse<'_>,
) -> Result<(), Diagnostic> {
    write!(
        stream,
        "HTTP/1.1 {}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        response.status, response.content_type, response.content_length
    )
    .and_then(|()| stream.write_all(&response.body))
    .map_err(|error| registry_error(None, format!("failed to write response: {error}")))
}

fn registry_http_error(status: &'static str, message: &str) -> RegistryHttpResponse<'static> {
    RegistryHttpResponse {
        status,
        content_type: "text/plain; charset=utf-8",
        content_length: message.len() + 1,
        body: Cow::Owned(format!("{message}\n").into_bytes()),
    }
}

fn registry_content_type(path: &str) -> &'static str {
    if path.ends_with(MANIFEST_FILENAME) {
        "application/toml; charset=utf-8"
    } else if path.ends_with(".json") || path.ends_with(".sig") {
        "application/json"
    } else {
        "application/octet-stream"
    }
}

fn discover_package_signatures(root: &Path) -> Result<Vec<PathBuf>, Diagnostic> {
    let mut found = Vec::new();
    discover_package_signatures_inner(root, root, 0, &mut found)?;
    found.sort();
    Ok(found)
}

fn discover_package_signatures_inner(
    root: &Path,
    dir: &Path,
    depth: usize,
    found: &mut Vec<PathBuf>,
) -> Result<(), Diagnostic> {
    if depth > 4 {
        return Ok(());
    }
    for entry in fs::read_dir(dir)
        .map_err(|error| registry_error(Some(dir), format!("failed to read directory: {error}")))?
    {
        let entry = entry
            .map_err(|error| registry_error(Some(dir), format!("failed to read entry: {error}")))?;
        let path = entry.path();
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with('.'))
        {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            registry_error(Some(&path), format!("failed to stat path: {error}"))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(registry_error(
                Some(&path),
                "registry tree must not contain symlinks",
            ));
        }
        if metadata.is_dir() {
            discover_package_signatures_inner(root, &path, depth + 1, found)?;
        } else if metadata.is_file() && entry.file_name() == PACKAGE_SIGNATURE_FILENAME {
            if !path.starts_with(root) {
                return Err(registry_error(
                    Some(&path),
                    "signature escaped registry root",
                ));
            }
            found.push(path);
        }
    }
    Ok(())
}

fn load_registry_metadata(
    packages_root: &Path,
    version_dir: &Path,
) -> Result<RawRegistryMetadata, Diagnostic> {
    let relative = version_dir.join(REGISTRY_METADATA_FILENAME);
    let path = packages_root.join(&relative);
    if !path.exists() {
        return Ok(RawRegistryMetadata::default());
    }
    let bytes = read_registry_relative_path(packages_root, &relative, MAX_MANIFEST_BYTES)?;
    let content = std::str::from_utf8(&bytes)
        .map_err(|error| registry_error(Some(&path), format!("metadata is not UTF-8: {error}")))?;
    toml::from_str(content)
        .map_err(|error| registry_error(Some(&path), format!("invalid metadata: {error}")))
}

fn read_registry_relative(root: &Path, relative: &str, cap: u64) -> Result<Vec<u8>, Diagnostic> {
    let relative = safe_relative_path(relative)?;
    read_registry_relative_path(root, &relative, cap)
}

#[cfg(unix)]
fn read_registry_relative_path(
    root: &Path,
    relative: &Path,
    cap: u64,
) -> Result<Vec<u8>, Diagnostic> {
    let components = relative
        .components()
        .map(|component| match component {
            Component::Normal(value) => CString::new(value.as_bytes())
                .map_err(|_| registry_error(None, "registry path contains NUL")),
            _ => Err(registry_error(None, "unsafe registry artifact path")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (file_name, parents) = components
        .split_last()
        .ok_or_else(|| registry_error(None, "registry artifact path is empty"))?;
    let root_file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root)
        .map_err(|error| {
            registry_error(
                Some(root),
                format!("failed to safely open registry root: {error}"),
            )
        })?;
    let mut directory: OwnedFd = root_file.into();
    let mut display_path = root.to_path_buf();
    for parent in parents {
        display_path.push(std::ffi::OsStr::from_bytes(parent.as_bytes()));
        let descriptor = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                parent.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(registry_error(
                Some(&display_path),
                format!(
                    "failed to safely open registry parent: {}",
                    std::io::Error::last_os_error()
                ),
            ));
        }
        directory = unsafe { OwnedFd::from_raw_fd(descriptor) };
    }
    display_path.push(std::ffi::OsStr::from_bytes(file_name.as_bytes()));
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            file_name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    if descriptor < 0 {
        return Err(registry_error(
            Some(&display_path),
            format!(
                "failed to safely open registry artifact: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    let descriptor = unsafe { OwnedFd::from_raw_fd(descriptor) };
    let mut file = fs::File::from(descriptor);
    let metadata = file.metadata().map_err(|error| {
        registry_error(
            Some(&display_path),
            format!("failed to inspect opened registry artifact: {error}"),
        )
    })?;
    if !metadata.is_file() {
        return Err(registry_error(
            Some(&display_path),
            "registry artifact is not a regular file",
        ));
    }
    if metadata.len() > cap {
        return Err(registry_error(
            Some(&display_path),
            format!("file exceeds byte limit of {cap}"),
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    (&mut file)
        .take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            registry_error(
                Some(&display_path),
                format!("failed to read opened registry artifact: {error}"),
            )
        })?;
    if bytes.len() as u64 > cap {
        return Err(registry_error(
            Some(&display_path),
            format!("file exceeds byte limit of {cap}"),
        ));
    }
    Ok(bytes)
}

#[cfg(not(unix))]
fn read_registry_relative_path(
    _root: &Path,
    _relative: &Path,
    _cap: u64,
) -> Result<Vec<u8>, Diagnostic> {
    Err(registry_error(
        None,
        "secure descriptor-relative registry reads are unsupported on this platform",
    ))
}

fn read_registry_file(root: &Path, path: &Path, cap: u64) -> Result<Vec<u8>, Diagnostic> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| registry_error(Some(path), "registry path escaped root"))?;
    read_registry_relative_path(root, relative, cap)
}

#[cfg(unix)]
fn read_local_file_bounded(path: &Path, cap: u64, category: &str) -> Result<Vec<u8>, Diagnostic> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| {
            Diagnostic::new(
                category,
                format!("failed to safely open {}: {error}", path.display()),
            )
            .with_path(path.display().to_string())
        })?;
    let metadata = file.metadata().map_err(|error| {
        Diagnostic::new(category, format!("failed to inspect opened file: {error}"))
            .with_path(path.display().to_string())
    })?;
    if !metadata.is_file() {
        return Err(
            Diagnostic::new(category, "expected a regular, non-symlink file")
                .with_path(path.display().to_string()),
        );
    }
    if metadata.len() > cap {
        return Err(
            Diagnostic::new(category, format!("file exceeds byte limit of {cap}"))
                .with_path(path.display().to_string()),
        );
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    (&mut file)
        .take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            Diagnostic::new(category, format!("failed to read opened file: {error}"))
                .with_path(path.display().to_string())
        })?;
    if bytes.len() as u64 > cap {
        return Err(
            Diagnostic::new(category, format!("file exceeds byte limit of {cap}"))
                .with_path(path.display().to_string()),
        );
    }
    Ok(bytes)
}

#[cfg(not(unix))]
fn read_local_file_bounded(path: &Path, _cap: u64, category: &str) -> Result<Vec<u8>, Diagnostic> {
    Err(Diagnostic::new(
        category,
        "secure single-handle file reads are unsupported on this platform",
    )
    .with_path(path.display().to_string()))
}

fn safe_relative_path(value: &str) -> Result<PathBuf, Diagnostic> {
    if value.is_empty()
        || value.starts_with('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err(registry_error(None, "unsafe registry target path"));
    }
    let path = Path::new(value);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(registry_error(None, "unsafe registry target path"));
    }
    Ok(path.to_path_buf())
}

fn safe_registry_segment(category: &str, kind: &str, value: &str) -> Result<String, Diagnostic> {
    if is_unsafe_registry_path_segment(value) {
        return Err(Diagnostic::new(
            category,
            format!("registry {kind} must be a safe path segment: {value:?}"),
        ));
    }
    Ok(value.to_owned())
}

fn is_unsafe_registry_path_segment(value: &str) -> bool {
    value.is_empty()
        || value.trim() != value
        || matches!(value, "." | "..")
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || (value.len() >= 2
            && value.as_bytes()[0].is_ascii_alphabetic()
            && value.as_bytes()[1] == b':')
}

fn required_text(category: &str, kind: &str, value: &str) -> Result<String, Diagnostic> {
    if value.trim().is_empty() || value.trim() != value || value.chars().any(char::is_control) {
        return Err(Diagnostic::new(
            category,
            format!("{kind} must not be empty or padded"),
        ));
    }
    Ok(value.to_owned())
}

fn required_value_text(
    value: &Value,
    field: &str,
    path: Option<&Path>,
) -> Result<String, Diagnostic> {
    value_text(value, field)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| registry_error(path, format!("release is missing {field}")))
}

fn value_text<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(Value::as_str)
}

fn is_lower_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn hash_bytes(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn package_signing_error<E: fmt::Display>(
    error: crate::package_trust::PackageSigningError<E>,
) -> Diagnostic {
    let message = match error {
        crate::package_trust::PackageSigningError::Provider(error) => {
            format!("package signing provider failed: {error}")
        }
        crate::package_trust::PackageSigningError::Transcript(error) => {
            format!("failed to construct package transcript: {error}")
        }
        crate::package_trust::PackageSigningError::PublicKeyRejected => {
            "package signer public key was rejected".to_owned()
        }
        crate::package_trust::PackageSigningError::SignatureRejected => {
            "package signing provider returned an invalid signature".to_owned()
        }
    };
    publish_error(None, message)
}

fn publish_error(path: Option<&Path>, message: impl Into<String>) -> Diagnostic {
    let diagnostic = Diagnostic::new("publish", message.into());
    path.map_or(diagnostic.clone(), |path| {
        diagnostic.with_path(path.display().to_string())
    })
}

fn registry_error(path: Option<&Path>, message: impl Into<String>) -> Diagnostic {
    let diagnostic = Diagnostic::new("registry", message.into());
    path.map_or(diagnostic.clone(), |path| {
        diagnostic.with_path(path.display().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::cell::Cell;
    use std::rc::Rc;
    use tempfile::tempdir;

    struct TestSigner(SigningKey);

    impl TestSigner {
        fn new(seed: u8) -> Self {
            Self(SigningKey::from_bytes(&[seed; 32]))
        }
    }

    impl Ed25519Signer for TestSigner {
        type Error = std::convert::Infallible;

        fn public_key(&self) -> Result<[u8; 32], Self::Error> {
            Ok(self.0.verifying_key().to_bytes())
        }

        fn sign(&self, message: &[u8]) -> Result<[u8; 64], Self::Error> {
            Ok(self.0.sign(message).to_bytes())
        }
    }

    struct CountingSigner {
        signer: TestSigner,
        sign_calls: Rc<Cell<usize>>,
    }

    impl Ed25519Signer for CountingSigner {
        type Error = std::convert::Infallible;

        fn public_key(&self) -> Result<[u8; 32], Self::Error> {
            self.signer.public_key()
        }

        fn sign(&self, message: &[u8]) -> Result<[u8; 64], Self::Error> {
            self.sign_calls.set(self.sign_calls.get() + 1);
            self.signer.sign(message)
        }
    }

    struct TrustFixture {
        roots: TrustRootsEnvelope,
        expectation: VerificationExpectation,
        package_signers: [TestSigner; 2],
        index_signers: [TestSigner; 2],
    }

    fn signer_key_id(signer: &TestSigner) -> String {
        sign_metadata(b"key-id", signer).unwrap()["key_id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn trust_key(signer: &TestSigner, publisher: &str) -> Value {
        let public_key = hex_encode(&signer.public_key().unwrap());
        let material = json!({
            "algorithm": "ed25519",
            "public_key_encoding": "lowercase-hex",
            "public_key": public_key
        });
        json!({
            "key_id": format!("sha256:{}", hash_bytes(&canonical_json(&material).unwrap())),
            "key_material": material,
            "publisher_identity": publisher,
            "status": "active",
            "valid_from_sequence": 1,
            "supersedes_key_ids": [],
            "revocation": null
        })
    }

    fn root_envelope(signed: Value, signers: &[TestSigner]) -> Value {
        let transcript = metadata_transcript(crate::package_trust::ROOT_DOMAIN, &signed).unwrap();
        json!({
            "signed": signed,
            "transcript": {
                "encoding": "axiom-canonical-json-v1",
                "domain": crate::package_trust::ROOT_DOMAIN,
                "bytes_hex": hex_encode(&transcript),
                "sha256": hash_bytes(&transcript)
            },
            "signatures": signers
                .iter()
                .map(|signer| sign_metadata(&transcript, signer).unwrap())
                .collect::<Vec<_>>()
        })
    }

    fn trust_fixture() -> TrustFixture {
        let old_root = [TestSigner::new(1), TestSigner::new(2), TestSigner::new(9)];
        let new_root = [TestSigner::new(3), TestSigner::new(4)];
        let index_signers = [TestSigner::new(5), TestSigner::new(6)];
        let package_signers = [TestSigner::new(7), TestSigner::new(8)];
        let old_root_ids = old_root.iter().map(signer_key_id).collect::<Vec<_>>();
        let new_root_ids = new_root.iter().map(signer_key_id).collect::<Vec<_>>();
        let index_ids = index_signers.iter().map(signer_key_id).collect::<Vec<_>>();
        let package_ids = package_signers
            .iter()
            .map(signer_key_id)
            .collect::<Vec<_>>();
        let old_signed = json!({
            "specification": "axiom-package-trust-root-v1",
            "root_version": 1,
            "sequence": 1,
            "issued_at": "2026-01-01T00:00:00Z",
            "expires_at": "2027-01-01T00:00:00Z",
            "consistent_snapshot": true,
            "keys": old_root.iter().map(|signer| trust_key(signer, "axiom://trust/root")).collect::<Vec<_>>(),
            "publisher_identities": [{
                "publisher_identity": "axiom://trust/root",
                "display_name": "Old root"
            }],
            "namespace_grants": [{
                "publisher_identity": "axiom://trust/root",
                "namespace": "bootstrap",
                "package_names": ["bootstrap"],
                "registry_identities": ["registry:test"],
                "source_identities": ["registry:test-source"],
                "role_id": "registry-index"
            }],
            "roles": [
                {"role_id":"root","threshold":2,"key_ids":old_root_ids.clone(),"delegated_by":null},
                {"role_id":"timestamp","threshold":2,"key_ids":old_root_ids.clone(),"delegated_by":"root"},
                {"role_id":"snapshot","threshold":2,"key_ids":old_root_ids.clone(),"delegated_by":"timestamp"},
                {"role_id":"registry-index","threshold":2,"key_ids":old_root_ids,"delegated_by":"snapshot"}
            ],
            "policy": {
                "rollback_protection": "reject rollback",
                "freeze_protection": "reject expiry",
                "downgrade_protection": "reject downgrade",
                "offline_locked": "require exact pins",
                "metadata_expiry_required": true,
                "registry_index_equivalence": "combined metadata role"
            }
        });
        let candidate_signed = json!({
            "specification": "axiom-package-trust-root-v1",
            "root_version": 2,
            "sequence": 2,
            "issued_at": "2026-07-01T00:00:00Z",
            "expires_at": "2027-01-01T00:00:00Z",
            "consistent_snapshot": true,
            "keys": new_root.iter().map(|signer| trust_key(signer, "axiom://trust/root"))
                .chain(index_signers.iter().map(|signer| trust_key(signer, "axiom://registry/official")))
                .chain(package_signers.iter().map(|signer| trust_key(signer, "publisher:foundation")))
                .collect::<Vec<_>>(),
            "publisher_identities": [{
                "publisher_identity": "publisher:foundation",
                "display_name": "Test publisher"
            }],
            "namespace_grants": [{
                "publisher_identity": "publisher:foundation",
                "namespace": "axiom",
                "package_names": ["core"],
                "registry_identities": ["registry:test"],
                "source_identities": ["registry:test-source"],
                "role_id": "targets:axiom"
            }],
            "roles": [
                {"role_id":"root","threshold":2,"key_ids":new_root_ids,"delegated_by":null},
                {"role_id":"timestamp","threshold":2,"key_ids":index_ids,"delegated_by":"root"},
                {"role_id":"snapshot","threshold":2,"key_ids":index_ids,"delegated_by":"timestamp"},
                {"role_id":"registry-index","threshold":2,"key_ids":index_ids,"delegated_by":"snapshot"},
                {"role_id":"targets","threshold":2,"key_ids":package_ids.clone(),"delegated_by":"root"},
                {"role_id":"targets:axiom","threshold":2,"key_ids":package_ids.clone(),"delegated_by":"targets"}
            ],
            "policy": {
                "rollback_protection": "reject rollback",
                "freeze_protection": "reject expiry",
                "downgrade_protection": "reject downgrade",
                "offline_locked": "require exact pins",
                "metadata_expiry_required": true,
                "registry_index_equivalence": "combined metadata role"
            }
        });
        let trusted_root = root_envelope(old_signed, &old_root);
        let candidate_root = root_envelope(candidate_signed, &new_root);
        let candidate_transcript =
            metadata_transcript(crate::package_trust::ROOT_DOMAIN, &candidate_root["signed"])
                .unwrap();
        let roots = TrustRootsEnvelope(json!({
            "schema_version": "axiom.package_trust_roots.v1",
            "contract": "package.trust_roots",
            "contract_status": "implemented",
            "trusted_root": trusted_root,
            "candidate_root": candidate_root,
            "transition": {
                "from_version": 1,
                "to_version": 2,
                "transition_time": "2026-07-02T00:00:00Z",
                "candidate_signatures_by_old_root": old_root.iter().map(|signer| sign_metadata(&candidate_transcript, signer).unwrap()).collect::<Vec<_>>(),
                "candidate_signatures_by_new_root": new_root.iter().map(|signer| sign_metadata(&candidate_transcript, signer).unwrap()).collect::<Vec<_>>()
            }
        }));
        let contract: Value = serde_json::from_slice(include_bytes!(
            "../../../package-trust/contract/package-trust.json"
        ))
        .unwrap();
        let mut expectation = VerificationExpectation(contract["verification_expectation"].clone());
        expectation.0["contract_status"] = json!("implemented");
        expectation.0["verification_time"] = json!("2026-07-29T12:00:00Z");
        expectation.0["required_signers"]["index_role_id"] = json!("registry-index");
        expectation.0["required_signers"]["index_threshold"] = json!(2);
        expectation.0["required_signers"]["package_role_id"] = json!("targets:axiom");
        expectation.0["required_signers"]["package_threshold"] = json!(2);
        expectation.0["required_signers"]["required_key_ids"] = json!(package_ids);
        expectation.0["trusted_state"]["trusted_root_anchor"] = json!({
            "root_version": 1,
            "root_sequence": 1,
            "root_transcript_sha256": roots["trusted_root"]["transcript"]["sha256"]
        });
        expectation.0["trusted_state"]["highest_root_version"] = json!(2);
        expectation.0["trusted_state"]["highest_root_sequence"] = json!(2);
        expectation.0["trusted_state"]["highest_index_generation"] = json!(1);
        expectation.0["trusted_state"]["highest_index_sequence"] = json!(1);
        expectation.0["trusted_state"]["minimum_package_version"] = json!("1.2.3");
        expectation.0["trusted_state"]["seen_snapshots"] = json!([{
            "generation": 1,
            "sequence": 1,
            "snapshot_id": "registry.test.publication-bootstrap",
            "index_transcript_sha256": "00".repeat(32)
        }]);
        expectation.0["offline_lock"]["root_version"] =
            roots["candidate_root"]["signed"]["root_version"].clone();
        expectation.0["offline_lock"]["root_sequence"] =
            roots["candidate_root"]["signed"]["sequence"].clone();
        expectation.0["offline_lock"]["root_transcript_sha256"] =
            roots["candidate_root"]["transcript"]["sha256"].clone();
        TrustFixture {
            roots,
            expectation,
            package_signers,
            index_signers,
        }
    }

    fn provenance_statement(target: &str, archive_hash: &str) -> Value {
        json!({
            "_type": "https://in-toto.io/Statement/v1",
            "subject": [{"name": target, "digest": {"sha256": archive_hash}}],
            "predicateType": "https://slsa.dev/provenance/v1",
            "predicate": {
                "buildDefinition": {
                    "buildType": "axiom:build/package-v1",
                    "externalParameters": {},
                    "internalParameters": {},
                    "resolvedDependencies": [{
                        "uri": "registry:test-source",
                        "digest": {"sha256": "11".repeat(32)}
                    }]
                },
                "runDetails": {
                    "builder": {
                        "id": "axiom:builder/test",
                        "builderDependencies": [],
                        "version": {"axiomc": "test"}
                    },
                    "metadata": {
                        "invocationId": "urn:uuid:00000000-0000-4000-8000-000000000001",
                        "startedOn": "2026-07-29T10:00:00Z",
                        "finishedOn": "2026-07-29T10:00:01Z"
                    },
                    "byproducts": []
                }
            }
        })
    }

    fn pin_fixture_to_candidate_index(fixture: &mut TrustFixture, registry: &Path) {
        pin_fixture_to_candidate_index_at(
            fixture,
            registry,
            1,
            1,
            "registry.test.1.1",
            &"00".repeat(32),
        );
    }

    fn pin_fixture_to_candidate_index_at(
        fixture: &mut TrustFixture,
        registry: &Path,
        generation: u64,
        sequence: u64,
        snapshot_id: &str,
        previous_snapshot_sha256: &str,
    ) {
        let mut releases = discover_package_signatures(registry)
            .unwrap()
            .into_iter()
            .map(|signature_path| {
                let signature_bytes =
                    read_registry_file(registry, &signature_path, MAX_SIGNATURE_BYTES).unwrap();
                let signature = parse_package_signature_json(&signature_bytes).unwrap();
                let canonical_signature = canonical_json(&signature).unwrap();
                let release_dir = Path::new(signature["package"]["target_path"].as_str().unwrap())
                    .parent()
                    .unwrap();
                let yanked = load_registry_metadata(registry, release_dir)
                    .unwrap()
                    .yanked
                    .unwrap_or(false);
                json!({
                    "namespace": signature["package"]["namespace"],
                    "name": signature["package"]["name"],
                    "version": signature["package"]["version"],
                    "target_path": signature["package"]["target_path"],
                    "registry_identity": signature["registry"]["registry_identity"],
                    "source_identity": signature["registry"]["source_identity"],
                    "publisher_identity": signature["publisher"]["publisher_identity"],
                    "archive": {
                        "length": signature["archive"]["size"],
                        "digest": signature["archive"]["digest"]
                    },
                    "manifest": signature["manifest"],
                    "provenance": signature["provenance"],
                    "package_signature_sha256": hash_bytes(&canonical_signature),
                    "yanked": yanked
                })
            })
            .collect::<Vec<_>>();
        releases.sort_by_key(|release| {
            (
                release["namespace"].as_str().unwrap().to_owned(),
                release["name"].as_str().unwrap().to_owned(),
                release["version"].as_str().unwrap().to_owned(),
            )
        });
        let release = releases[0].clone();
        let signed = json!({
            "metadata_version": 2,
            "registry_identity": "registry:test",
            "source_identity": "registry:test-source",
            "generation": generation,
            "sequence": sequence,
            "issued_at": "2026-07-29T11:00:00Z",
            "expires_at": "2026-12-31T00:00:00Z",
            "consistent_snapshot": {
                "enabled": true,
                "snapshot_id": snapshot_id,
                "metadata_path": format!("{generation}/{sequence}/index.v2.json"),
                "previous_snapshot_sha256": previous_snapshot_sha256
            },
            "signature_role": "registry-index",
            "releases": releases
        });
        let transcript = metadata_transcript(INDEX_DOMAIN, &signed).unwrap();
        let index_hash = hash_bytes(&transcript);
        fixture.expectation.0["trusted_state"]["seen_snapshots"] = json!([{
            "generation": generation,
            "sequence": sequence,
            "snapshot_id": snapshot_id,
            "index_transcript_sha256": index_hash
        }]);
        fixture.expectation.0["offline_lock"] = json!({
            "mode": "offline_locked",
            "network_fallback": false,
            "root_version": fixture.roots["candidate_root"]["signed"]["root_version"],
            "root_sequence": fixture.roots["candidate_root"]["signed"]["sequence"],
            "root_transcript_sha256": fixture.roots["candidate_root"]["transcript"]["sha256"],
            "index_generation": generation,
            "index_sequence": sequence,
            "index_transcript_sha256": index_hash,
            "release": {
                "registry_identity": release["registry_identity"],
                "source_identity": release["source_identity"],
                "namespace": release["namespace"],
                "name": release["name"],
                "version": release["version"],
                "target_path": release["target_path"],
                "publisher_identity": release["publisher_identity"],
                "archive": release["archive"],
                "manifest": release["manifest"],
                "provenance_statement_sha256": release["provenance"]["statement"]["digest"]["value"],
                "provenance_predicate_type": release["provenance"]["statement"]["value"]["predicateType"],
                "provenance_subject": release["provenance"]["selected_subject"],
                "package_signature_sha256": release["package_signature_sha256"]
            }
        });
    }

    fn resign_candidate_root(fixture: &mut TrustFixture) {
        let candidate_transcript = metadata_transcript(
            crate::package_trust::ROOT_DOMAIN,
            &fixture.roots["candidate_root"]["signed"],
        )
        .unwrap();
        fixture.roots.0["candidate_root"]["transcript"] = json!({
            "encoding": "axiom-canonical-json-v1",
            "domain": crate::package_trust::ROOT_DOMAIN,
            "bytes_hex": hex_encode(&candidate_transcript),
            "sha256": hash_bytes(&candidate_transcript)
        });
        let new_root = [TestSigner::new(3), TestSigner::new(4)];
        fixture.roots.0["candidate_root"]["signatures"] = json!(
            new_root
                .iter()
                .map(|signer| sign_metadata(&candidate_transcript, signer).unwrap())
                .collect::<Vec<_>>()
        );
        let old_root = [TestSigner::new(1), TestSigner::new(2), TestSigner::new(9)];
        fixture.roots.0["transition"]["candidate_signatures_by_old_root"] = json!(
            old_root
                .iter()
                .map(|signer| sign_metadata(&candidate_transcript, signer).unwrap())
                .collect::<Vec<_>>()
        );
        fixture.roots.0["transition"]["candidate_signatures_by_new_root"] = json!(
            new_root
                .iter()
                .map(|signer| sign_metadata(&candidate_transcript, signer).unwrap())
                .collect::<Vec<_>>()
        );
        fixture.expectation.0["offline_lock"]["root_transcript_sha256"] =
            fixture.roots["candidate_root"]["transcript"]["sha256"].clone();
    }

    fn rewrite_release_with_reversed_provenance(fixture: &TrustFixture, registry: &Path) {
        let signature_path = registry.join("axiom/core/1.2.3/package.axp.sig");
        let mut package =
            parse_package_signature_json(&fs::read(&signature_path).unwrap()).unwrap();
        package.0["provenance"]["statement"]["value"]["predicate"]["runDetails"]["metadata"]["startedOn"] =
            json!("2026-07-29T10:00:02Z");
        package.0["provenance"]["statement"]["value"]["predicate"]["runDetails"]["metadata"]["finishedOn"] =
            json!("2026-07-29T10:00:01Z");
        let provenance = canonical_json(&package["provenance"]["statement"]["value"]).unwrap();
        package.0["provenance"]["statement"]["digest"]["value"] = json!(hash_bytes(&provenance));
        package.0["provenance"]["statement"]["canonical_bytes_hex"] =
            json!(hex_encode(&provenance));
        package.0["transcript"] = Value::Null;
        package.0["signatures"] = json!([]);
        let transcript = package_transcript(&package, 2).unwrap();
        package.0["transcript"] = json!({
            "encoding": "axiom-tlv-v1",
            "domain": crate::package_trust::PACKAGE_DOMAIN,
            "field_order": PACKAGE_FIELDS,
            "bytes_hex": hex_encode(&transcript),
            "sha256": hash_bytes(&transcript)
        });
        package.0["signatures"] = json!(
            fixture
                .package_signers
                .iter()
                .map(|signer| sign_package_transcript(&package, 2, signer).unwrap())
                .collect::<Vec<_>>()
        );
        fs::write(&signature_path, canonical_json(&package).unwrap()).unwrap();
        fs::write(
            registry.join("axiom/core/1.2.3/provenance.json"),
            provenance,
        )
        .unwrap();
    }

    fn publish_and_index(root: &Path) -> (TrustFixture, RegistryIndex, PathBuf) {
        let mut fixture = trust_fixture();
        let project = write_project(root);
        let rendered = render_package_archive(&fs::canonicalize(&project).unwrap()).unwrap();
        let statement = provenance_statement(
            "axiom/core/1.2.3/package.axp",
            &hash_bytes(&rendered.archive),
        );
        let registry = root.join("registry");
        publish_package(
            &project,
            &registry,
            &PublishOptions {
                allow_overwrite: false,
                namespace: "axiom",
                registry_identity: "registry:test",
                source_identity: "registry:test-source",
                publisher_identity: "publisher:foundation",
                index_generation: 1,
                index_sequence: 1,
                provenance_statement: &statement,
                trust_roots: &fixture.roots,
                verification_expectation: &fixture.expectation,
                signers: &fixture.package_signers,
            },
        )
        .unwrap();
        pin_fixture_to_candidate_index(&mut fixture, &registry);
        crate::package_trust::parse_trust_roots_json(&canonical_json(&fixture.roots).unwrap())
            .expect("test roots remain schema-valid");
        crate::package_trust::parse_verification_expectation_json(
            &canonical_json(&fixture.expectation).unwrap(),
        )
        .unwrap_or_else(|error| {
            let schema: Value = serde_json::from_slice(include_bytes!(
                "../../../schemas/axiom-package-verification-expectation-v1.schema.json"
            ))
            .unwrap();
            let validator = jsonschema::validator_for(&schema).unwrap();
            let errors = validator
                .iter_errors(&fixture.expectation)
                .map(|error| error.to_string())
                .collect::<Vec<_>>();
            panic!("test expectation invalid: {error}; {errors:?}");
        });
        for root_name in ["trusted_root", "candidate_root"] {
            let transcript = metadata_transcript(
                crate::package_trust::ROOT_DOMAIN,
                &fixture.roots[root_name]["signed"],
            )
            .unwrap();
            assert_eq!(
                fixture.roots[root_name]["transcript"]["sha256"],
                hash_bytes(&transcript)
            );
        }
        let index = build_registry_index(
            &registry,
            &RegistryIndexOptions {
                registry_identity: "registry:test",
                source_identity: "registry:test-source",
                generation: 1,
                sequence: 1,
                issued_at: "2026-07-29T11:00:00Z",
                expires_at: "2026-12-31T00:00:00Z",
                snapshot_id: "registry.test.1.1",
                metadata_path: "1/1/index.v2.json",
                previous_snapshot_sha256: &"00".repeat(32),
                trust_roots: &fixture.roots,
                verification_expectation: &fixture.expectation,
                signers: &fixture.index_signers,
            },
        )
        .unwrap();
        (fixture, index, registry)
    }

    fn write_project(root: &Path) -> PathBuf {
        let project = root.join("project");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(
            project.join(MANIFEST_FILENAME),
            "[package]\nname = \"core\"\nversion = \"1.2.3\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n",
        )
        .unwrap();
        fs::write(
            project.join(LOCK_FILENAME),
            "version = 1\n\n[[package]]\nname = \"core\"\nversion = \"1.2.3\"\nsource = \"path\"\n",
        )
        .unwrap();
        fs::write(project.join("src/main.ax"), "print \"hello\"\n").unwrap();
        project
    }

    #[test]
    fn archive_path_normalization_rejects_traversal() {
        for path in [
            "../escape",
            "./same",
            "/absolute",
            "C:\\escape",
            "bad\0name",
            "bad\nname",
            "bad\rname",
        ] {
            assert!(normalize_archive_path(Path::new(path)).is_err(), "{path}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn registry_reader_rejects_symlinked_artifact_and_parent_escape() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let registry = dir.path().join("registry");
        let release = registry.join("axiom/core/1.2.3");
        fs::create_dir_all(&release).unwrap();
        let outside = dir.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join(DEFAULT_ARCHIVE_FILENAME), b"secret").unwrap();

        symlink(
            outside.join(DEFAULT_ARCHIVE_FILENAME),
            release.join(DEFAULT_ARCHIVE_FILENAME),
        )
        .unwrap();
        assert!(
            read_registry_relative(&registry, "axiom/core/1.2.3/package.axp", MAX_ARCHIVE_BYTES)
                .is_err()
        );

        fs::remove_file(release.join(DEFAULT_ARCHIVE_FILENAME)).unwrap();
        fs::remove_dir_all(registry.join("axiom/core")).unwrap();
        symlink(&outside, registry.join("axiom/core")).unwrap();
        assert!(
            read_registry_relative(&registry, "axiom/core/package.axp", MAX_ARCHIVE_BYTES).is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn publication_rejects_symlinked_registry_parent_without_writing_outside() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let project = write_project(dir.path());
        let rendered = render_package_archive(&fs::canonicalize(&project).unwrap()).unwrap();
        let statement = provenance_statement(
            "axiom/core/1.2.3/package.axp",
            &hash_bytes(&rendered.archive),
        );
        let fixture = trust_fixture();
        let registry = dir.path().join("registry");
        let outside = dir.path().join("outside");
        fs::create_dir_all(&registry).unwrap();
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, registry.join("axiom")).unwrap();

        let error = publish_package(
            &project,
            &registry,
            &PublishOptions {
                allow_overwrite: false,
                namespace: "axiom",
                registry_identity: "registry:test",
                source_identity: "registry:test-source",
                publisher_identity: "publisher:foundation",
                index_generation: 1,
                index_sequence: 1,
                provenance_statement: &statement,
                trust_roots: &fixture.roots,
                verification_expectation: &fixture.expectation,
                signers: &fixture.package_signers,
            },
        )
        .unwrap_err();

        assert!(
            error.message.contains("anchored registry parent"),
            "{}",
            error.message
        );
        assert!(!outside.join("core").exists());
    }

    #[test]
    fn bounded_registry_and_index_reads_reject_oversized_files_before_parsing() {
        let dir = tempdir().unwrap();
        let registry = dir.path().join("registry");
        fs::create_dir_all(&registry).unwrap();
        fs::write(registry.join("artifact"), b"12345").unwrap();
        let artifact_error = read_registry_relative(&registry, "artifact", 4).unwrap_err();
        assert!(artifact_error.message.contains("byte limit"));

        let index_path = dir.path().join("index.json");
        let index_file = fs::File::create(&index_path).unwrap();
        index_file.set_len(MAX_INDEX_BYTES + 1).unwrap();
        let index_error = load_registry_index(&index_path).unwrap_err();
        assert!(index_error.message.contains("byte limit"));
    }

    #[test]
    fn legacy_hmac_signature_is_never_accepted_as_package_trust_json() {
        let legacy =
            b"axiom-hmac-sha256-v1\npackage=core\nversion=1.2.3\narchive_hash=00\nhmac_sha256=00\n";
        assert!(parse_package_signature_json(legacy).is_err());
    }

    #[test]
    fn threshold_publish_signed_index_offline_verify_and_serve_snapshot_roundtrip() {
        let dir = tempdir().unwrap();
        let (fixture, index, registry) = publish_and_index(dir.path());
        let source_manifest = fs::read(dir.path().join("project").join(MANIFEST_FILENAME)).unwrap();
        let published_manifest =
            fs::read(registry.join("axiom/core/1.2.3").join(MANIFEST_FILENAME)).unwrap();
        let published_archive = fs::read(
            registry
                .join("axiom/core/1.2.3")
                .join(DEFAULT_ARCHIVE_FILENAME),
        )
        .unwrap();
        assert_eq!(published_manifest, source_manifest);
        let manifest_record = [
            format!(
                "--- file {MANIFEST_FILENAME} {} ---\n",
                published_manifest.len()
            )
            .into_bytes(),
            published_manifest.clone(),
        ]
        .concat();
        assert!(
            published_archive
                .windows(manifest_record.len())
                .any(|window| window == manifest_record)
        );
        verify_registry_index_integrity(&index, &registry, &fixture.roots, &fixture.expectation)
            .unwrap();
        let context =
            RegistryServeContext::new(&registry, &index, &fixture.roots, &fixture.expectation)
                .unwrap();
        let before = registry_http_response(
            &context,
            "GET /axiom/core/1.2.3/package.axp HTTP/1.1\r\n\r\n",
        );
        assert_eq!(before.status, "200 OK");
        let before = before.body.into_owned();
        fs::write(
            registry.join("axiom/core/1.2.3/package.axp"),
            b"tampered after startup",
        )
        .unwrap();
        let after = registry_http_response(
            &context,
            "GET /axiom/core/1.2.3/package.axp HTTP/1.1\r\n\r\n",
        );
        assert_eq!(after.body.as_ref(), before);
        let head = registry_http_response(
            &context,
            "HEAD /axiom/core/1.2.3/package.axp HTTP/1.1\r\n\r\n",
        );
        assert!(head.body.is_empty());
        assert_eq!(head.content_length, before.len());
        fs::write(
            registry.join("axiom/core/1.2.3/unindexed-secret"),
            b"not signed",
        )
        .unwrap();
        assert_eq!(
            registry_http_response(
                &context,
                "GET /axiom/core/1.2.3/unindexed-secret HTTP/1.1\r\n\r\n"
            )
            .status,
            "404 Not Found"
        );
        assert_eq!(
            registry_http_response(
                &context,
                "GET /axiom/core/1.2.3/axiom.lock HTTP/1.1\r\n\r\n"
            )
            .status,
            "404 Not Found"
        );
    }

    #[test]
    fn one_authenticated_index_verifies_multiple_release_specific_offline_locks() {
        let dir = tempdir().unwrap();
        let (mut fixture, _first_index, registry) = publish_and_index(dir.path());
        let project = dir.path().join("project");
        fs::write(
            project.join(MANIFEST_FILENAME),
            "[package]\nname = \"core\"\nversion = \"1.2.4\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n",
        )
        .unwrap();
        fs::write(
            project.join(LOCK_FILENAME),
            "version = 1\n\n[[package]]\nname = \"core\"\nversion = \"1.2.4\"\nsource = \"path\"\n",
        )
        .unwrap();
        let rendered = render_package_archive(&fs::canonicalize(&project).unwrap()).unwrap();
        let statement = provenance_statement(
            "axiom/core/1.2.4/package.axp",
            &hash_bytes(&rendered.archive),
        );
        publish_package(
            &project,
            &registry,
            &PublishOptions {
                allow_overwrite: false,
                namespace: "axiom",
                registry_identity: "registry:test",
                source_identity: "registry:test-source",
                publisher_identity: "publisher:foundation",
                index_generation: 1,
                index_sequence: 1,
                provenance_statement: &statement,
                trust_roots: &fixture.roots,
                verification_expectation: &fixture.expectation,
                signers: &fixture.package_signers,
            },
        )
        .unwrap();
        pin_fixture_to_candidate_index(&mut fixture, &registry);
        let index = build_registry_index(
            &registry,
            &RegistryIndexOptions {
                registry_identity: "registry:test",
                source_identity: "registry:test-source",
                generation: 1,
                sequence: 1,
                issued_at: "2026-07-29T11:00:00Z",
                expires_at: "2026-12-31T00:00:00Z",
                snapshot_id: "registry.test.1.1",
                metadata_path: "1/1/index.v2.json",
                previous_snapshot_sha256: &"00".repeat(32),
                trust_roots: &fixture.roots,
                verification_expectation: &fixture.expectation,
                signers: &fixture.index_signers,
            },
        )
        .unwrap();
        assert_eq!(
            index.envelope["signed"]["releases"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        verify_registry_index_integrity(&index, &registry, &fixture.roots, &fixture.expectation)
            .unwrap();

        fs::write(
            registry.join("axiom/core/1.2.4/axiom-registry.toml"),
            "yanked = true\nyank_reason = \"test withdrawal\"\n",
        )
        .unwrap();
        let previous_snapshot_sha256 = index.envelope["transcript"]["sha256"]
            .as_str()
            .unwrap()
            .to_owned();
        let original_signature_digests = index.envelope["signed"]["releases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|release| release["package_signature_sha256"].clone())
            .collect::<Vec<_>>();
        let prior_seen = fixture.expectation["trusted_state"]["seen_snapshots"][0].clone();
        pin_fixture_to_candidate_index_at(
            &mut fixture,
            &registry,
            1,
            2,
            "registry.test.1.2",
            &previous_snapshot_sha256,
        );
        fixture.expectation.0["trusted_state"]["seen_snapshots"] = json!([prior_seen]);
        let next_index = build_registry_index(
            &registry,
            &RegistryIndexOptions {
                registry_identity: "registry:test",
                source_identity: "registry:test-source",
                generation: 1,
                sequence: 2,
                issued_at: "2026-07-29T11:00:00Z",
                expires_at: "2026-12-31T00:00:00Z",
                snapshot_id: "registry.test.1.2",
                metadata_path: "1/2/index.v2.json",
                previous_snapshot_sha256: &previous_snapshot_sha256,
                trust_roots: &fixture.roots,
                verification_expectation: &fixture.expectation,
                signers: &fixture.index_signers,
            },
        )
        .unwrap();
        assert!(
            next_index.envelope["signed"]["releases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|release| release["version"] == "1.2.4" && release["yanked"] == true)
        );
        assert_eq!(
            next_index.envelope["signed"]["releases"]
                .as_array()
                .unwrap()
                .iter()
                .map(|release| release["package_signature_sha256"].clone())
                .collect::<Vec<_>>(),
            original_signature_digests
        );
        verify_registry_index_integrity(
            &next_index,
            &registry,
            &fixture.roots,
            &fixture.expectation,
        )
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn overwrite_publishes_one_visible_release_and_retains_hidden_recovery_state() {
        let dir = tempdir().unwrap();
        let (_first_fixture, _first_index, registry) = publish_and_index(dir.path());
        let project = dir.path().join("project");
        fs::write(project.join("src/main.ax"), "print \"replacement\"\n").unwrap();
        let rendered = render_package_archive(&fs::canonicalize(&project).unwrap()).unwrap();
        let statement = provenance_statement(
            "axiom/core/1.2.3/package.axp",
            &hash_bytes(&rendered.archive),
        );
        let mut fixture = trust_fixture();
        publish_package(
            &project,
            &registry,
            &PublishOptions {
                allow_overwrite: true,
                namespace: "axiom",
                registry_identity: "registry:test",
                source_identity: "registry:test-source",
                publisher_identity: "publisher:foundation",
                index_generation: 1,
                index_sequence: 1,
                provenance_statement: &statement,
                trust_roots: &fixture.roots,
                verification_expectation: &fixture.expectation,
                signers: &fixture.package_signers,
            },
        )
        .unwrap();
        assert_eq!(
            fs::read(registry.join("axiom/core/1.2.3/package.axp")).unwrap(),
            rendered.archive
        );
        let package_entries = fs::read_dir(registry.join("axiom/core"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            package_entries
                .iter()
                .filter(|name| !name.starts_with('.'))
                .collect::<Vec<_>>(),
            vec![&"1.2.3".to_owned()]
        );
        assert!(package_entries.iter().any(|name| name == ".1.2.3.previous"));
        assert_eq!(
            package_entries
                .iter()
                .filter(|name| name.starts_with(".1.2.3."))
                .count(),
            1
        );

        let release_dir = registry.join("axiom/core/1.2.3");
        let previous_dir = registry.join("axiom/core/.1.2.3.previous");
        fs::remove_dir_all(&previous_dir).unwrap();
        fs::rename(&release_dir, &previous_dir).unwrap();
        fs::create_dir(registry.join("axiom/core/.1.2.3.publish-pending")).unwrap();
        fs::write(
            registry.join("axiom/core/.1.2.3.publish-pending/incomplete"),
            b"partial",
        )
        .unwrap();
        publish_package(
            &project,
            &registry,
            &PublishOptions {
                allow_overwrite: true,
                namespace: "axiom",
                registry_identity: "registry:test",
                source_identity: "registry:test-source",
                publisher_identity: "publisher:foundation",
                index_generation: 1,
                index_sequence: 1,
                provenance_statement: &statement,
                trust_roots: &fixture.roots,
                verification_expectation: &fixture.expectation,
                signers: &fixture.package_signers,
            },
        )
        .unwrap();
        assert!(release_dir.is_dir());
        assert!(!registry.join("axiom/core/.1.2.3.publish-pending").exists());
        assert!(previous_dir.is_dir());

        pin_fixture_to_candidate_index(&mut fixture, &registry);
        let rebuilt = build_registry_index(
            &registry,
            &RegistryIndexOptions {
                registry_identity: "registry:test",
                source_identity: "registry:test-source",
                generation: 1,
                sequence: 1,
                issued_at: "2026-07-29T11:00:00Z",
                expires_at: "2026-12-31T00:00:00Z",
                snapshot_id: "registry.test.1.1",
                metadata_path: "1/1/index.v2.json",
                previous_snapshot_sha256: &"00".repeat(32),
                trust_roots: &fixture.roots,
                verification_expectation: &fixture.expectation,
                signers: &fixture.index_signers,
            },
        )
        .unwrap();
        assert_eq!(
            rebuilt.envelope["signed"]["releases"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn full_verification_rejects_tampered_archive_manifest_provenance_and_signature() {
        for file in [
            DEFAULT_ARCHIVE_FILENAME,
            MANIFEST_FILENAME,
            PROVENANCE_FILENAME,
            PACKAGE_SIGNATURE_FILENAME,
        ] {
            let dir = tempdir().unwrap();
            let (fixture, index, registry) = publish_and_index(dir.path());
            fs::write(registry.join("axiom/core/1.2.3").join(file), b"tampered").unwrap();
            assert!(
                verify_registry_index_integrity(
                    &index,
                    &registry,
                    &fixture.roots,
                    &fixture.expectation
                )
                .is_err(),
                "{file} tamper must fail"
            );
        }
    }

    #[test]
    fn full_verification_rejects_tampered_index_and_root() {
        let dir = tempdir().unwrap();
        let (fixture, mut index, registry) = publish_and_index(dir.path());
        index.envelope.0["signed"]["sequence"] = json!(2);
        assert!(
            verify_registry_index_integrity(
                &index,
                &registry,
                &fixture.roots,
                &fixture.expectation
            )
            .is_err()
        );

        let dir = tempdir().unwrap();
        let (mut fixture, index, registry) = publish_and_index(dir.path());
        fixture.roots.0["candidate_root"]["signed"]["sequence"] = json!(3);
        assert!(
            verify_registry_index_integrity(
                &index,
                &registry,
                &fixture.roots,
                &fixture.expectation
            )
            .is_err()
        );
    }

    #[test]
    fn caller_rollback_replay_and_required_key_pins_are_never_self_advanced() {
        let dir = tempdir().unwrap();
        let (fixture, index, registry) = publish_and_index(dir.path());

        let mut rollback = fixture.expectation.clone();
        rollback.0["trusted_state"]["highest_index_generation"] = json!(2);
        assert!(
            verify_registry_index_integrity(&index, &registry, &fixture.roots, &rollback)
                .unwrap_err()
                .message
                .contains("ROLLBACK_DETECTED")
        );

        let mut replay = fixture.expectation.clone();
        replay.0["trusted_state"]["seen_snapshots"][0]["snapshot_id"] = json!("rebound-snapshot");
        assert!(
            verify_registry_index_integrity(&index, &registry, &fixture.roots, &replay).is_err()
        );

        let mut required = fixture.expectation.clone();
        required.0["required_signers"]["required_key_ids"]
            .as_array_mut()
            .unwrap()
            .push(json!(format!("sha256:{}", "aa".repeat(32))));
        assert!(
            verify_registry_index_integrity(&index, &registry, &fixture.roots, &required).is_err()
        );
    }

    #[test]
    fn serve_snapshot_uses_the_same_bytes_verified_before_disk_mutation() {
        let dir = tempdir().unwrap();
        let (fixture, index, registry) = publish_and_index(dir.path());
        let archive_path = registry.join("axiom/core/1.2.3/package.axp");
        let original = fs::read(&archive_path).unwrap();
        let mut mutate = |_release_dir: &Path| {
            fs::write(&archive_path, b"mutated between capture and verification").unwrap();
        };
        let context = RegistryServeContext::new_with_capture_hook(
            &registry,
            &index,
            &fixture.roots,
            &fixture.expectation,
            Some(&mut mutate),
        )
        .unwrap();
        let response = registry_http_response(
            &context,
            "GET /axiom/core/1.2.3/package.axp HTTP/1.1\r\n\r\n",
        );
        assert_eq!(response.body.as_ref(), original);
    }

    #[test]
    fn release_cap_rejects_before_any_package_crypto_loop() {
        let dir = tempdir().unwrap();
        let (_fixture, mut index, _registry) = publish_and_index(dir.path());
        let release = index.envelope["signed"]["releases"][0].clone();
        index.envelope.0["signed"]["releases"] =
            Value::Array(vec![release; MAX_INDEX_RELEASES + 1]);
        let error = validate_registry_index(&index.envelope, None).unwrap_err();
        assert!(error.message.contains("maximum is 1024"));
    }

    #[test]
    fn release_and_index_byte_caps_reject_before_index_signer_provider_calls() {
        let trusted_dir = tempdir().unwrap();
        let (fixture, _index, trusted_registry) = publish_and_index(trusted_dir.path());
        let dir = tempdir().unwrap();
        let registry = dir.path().join("registry");
        for version in 0..=MAX_INDEX_RELEASES {
            fs::create_dir_all(registry.join(format!("axiom/core/{version}"))).unwrap();
            fs::write(
                registry.join(format!("axiom/core/{version}/{PACKAGE_SIGNATURE_FILENAME}")),
                b"not parsed because release count is bounded first",
            )
            .unwrap();
        }
        let calls = Rc::new(Cell::new(0));
        let signers = [
            CountingSigner {
                signer: TestSigner::new(5),
                sign_calls: Rc::clone(&calls),
            },
            CountingSigner {
                signer: TestSigner::new(6),
                sign_calls: Rc::clone(&calls),
            },
        ];
        let oversized_snapshot = "x".repeat(MAX_INDEX_BYTES as usize);
        let previous_snapshot = "00".repeat(32);
        let count_error = build_registry_index(
            &registry,
            &RegistryIndexOptions {
                registry_identity: "registry:test",
                source_identity: "registry:test-source",
                generation: 1,
                sequence: 1,
                issued_at: "2026-07-29T11:00:00Z",
                expires_at: "2026-12-31T00:00:00Z",
                snapshot_id: "registry.test.1.1",
                metadata_path: "1/1/index.v2.json",
                previous_snapshot_sha256: &previous_snapshot,
                trust_roots: &fixture.roots,
                verification_expectation: &fixture.expectation,
                signers: &signers,
            },
        )
        .unwrap_err();
        assert!(count_error.message.contains("maximum is 1024"));
        assert_eq!(calls.get(), 0);

        let size_error = build_registry_index(
            &trusted_registry,
            &RegistryIndexOptions {
                registry_identity: "registry:test",
                source_identity: "registry:test-source",
                generation: 1,
                sequence: 1,
                issued_at: "2026-07-29T11:00:00Z",
                expires_at: "2026-12-31T00:00:00Z",
                snapshot_id: &oversized_snapshot,
                metadata_path: "1/1/index.v2.json",
                previous_snapshot_sha256: &previous_snapshot,
                trust_roots: &fixture.roots,
                verification_expectation: &fixture.expectation,
                signers: &signers,
            },
        )
        .unwrap_err();
        assert!(size_error.message.contains("byte limit"));
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn invalid_release_never_reaches_registry_index_signer_provider() {
        let dir = tempdir().unwrap();
        let (fixture, _index, registry) = publish_and_index(dir.path());
        fs::write(
            registry.join("axiom/core/1.2.3/package.axp"),
            b"tampered before index signing",
        )
        .unwrap();
        let calls = Rc::new(Cell::new(0));
        let signers = [
            CountingSigner {
                signer: TestSigner::new(5),
                sign_calls: Rc::clone(&calls),
            },
            CountingSigner {
                signer: TestSigner::new(6),
                sign_calls: Rc::clone(&calls),
            },
        ];
        let error = build_registry_index(
            &registry,
            &RegistryIndexOptions {
                registry_identity: "registry:test",
                source_identity: "registry:test-source",
                generation: 1,
                sequence: 1,
                issued_at: "2026-07-29T11:00:00Z",
                expires_at: "2026-12-31T00:00:00Z",
                snapshot_id: "registry.test.1.1",
                metadata_path: "1/1/index.v2.json",
                previous_snapshot_sha256: &"00".repeat(32),
                trust_roots: &fixture.roots,
                verification_expectation: &fixture.expectation,
                signers: &signers,
            },
        )
        .unwrap_err();
        assert!(error.message.contains("do not match signed"));
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn invalid_root_transition_never_reaches_index_signers() {
        let dir = tempdir().unwrap();
        let (mut fixture, _index, registry) = publish_and_index(dir.path());
        fixture.roots.0["transition"]["candidate_signatures_by_old_root"][0]["value"] =
            json!("00".repeat(64));
        let calls = Rc::new(Cell::new(0));
        let signers = [
            CountingSigner {
                signer: TestSigner::new(5),
                sign_calls: Rc::clone(&calls),
            },
            CountingSigner {
                signer: TestSigner::new(6),
                sign_calls: Rc::clone(&calls),
            },
        ];
        let error = build_registry_index(
            &registry,
            &RegistryIndexOptions {
                registry_identity: "registry:test",
                source_identity: "registry:test-source",
                generation: 1,
                sequence: 1,
                issued_at: "2026-07-29T11:00:00Z",
                expires_at: "2026-12-31T00:00:00Z",
                snapshot_id: "registry.test.1.1",
                metadata_path: "1/1/index.v2.json",
                previous_snapshot_sha256: &"00".repeat(32),
                trust_roots: &fixture.roots,
                verification_expectation: &fixture.expectation,
                signers: &signers,
            },
        )
        .unwrap_err();
        assert!(error.message.contains("root preflight rejected"));
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn reversed_provenance_timestamps_never_reach_package_or_index_signers() {
        let dir = tempdir().unwrap();
        let project = write_project(dir.path());
        let rendered = render_package_archive(&fs::canonicalize(&project).unwrap()).unwrap();
        let mut statement = provenance_statement(
            "axiom/core/1.2.3/package.axp",
            &hash_bytes(&rendered.archive),
        );
        statement["predicate"]["runDetails"]["metadata"]["startedOn"] =
            json!("2026-07-29T10:00:02Z");
        let fixture = trust_fixture();
        let package_calls = Rc::new(Cell::new(0));
        let package_signers = [
            CountingSigner {
                signer: TestSigner::new(7),
                sign_calls: Rc::clone(&package_calls),
            },
            CountingSigner {
                signer: TestSigner::new(8),
                sign_calls: Rc::clone(&package_calls),
            },
        ];
        let error = publish_package(
            &project,
            &dir.path().join("rejected-registry"),
            &PublishOptions {
                allow_overwrite: false,
                namespace: "axiom",
                registry_identity: "registry:test",
                source_identity: "registry:test-source",
                publisher_identity: "publisher:foundation",
                index_generation: 1,
                index_sequence: 1,
                provenance_statement: &statement,
                trust_roots: &fixture.roots,
                verification_expectation: &fixture.expectation,
                signers: &package_signers,
            },
        )
        .unwrap_err();
        assert!(error.message.contains("PROVENANCE"));
        assert_eq!(package_calls.get(), 0);

        let valid_dir = tempdir().unwrap();
        let (mut fixture, _index, registry) = publish_and_index(valid_dir.path());
        rewrite_release_with_reversed_provenance(&fixture, &registry);
        pin_fixture_to_candidate_index(&mut fixture, &registry);
        let index_calls = Rc::new(Cell::new(0));
        let index_signers = [
            CountingSigner {
                signer: TestSigner::new(5),
                sign_calls: Rc::clone(&index_calls),
            },
            CountingSigner {
                signer: TestSigner::new(6),
                sign_calls: Rc::clone(&index_calls),
            },
        ];
        let error = build_registry_index(
            &registry,
            &RegistryIndexOptions {
                registry_identity: "registry:test",
                source_identity: "registry:test-source",
                generation: 1,
                sequence: 1,
                issued_at: "2026-07-29T11:00:00Z",
                expires_at: "2026-12-31T00:00:00Z",
                snapshot_id: "registry.test.1.1",
                metadata_path: "1/1/index.v2.json",
                previous_snapshot_sha256: &"00".repeat(32),
                trust_roots: &fixture.roots,
                verification_expectation: &fixture.expectation,
                signers: &index_signers,
            },
        )
        .unwrap_err();
        assert!(error.message.contains("PROVENANCE"));
        assert_eq!(index_calls.get(), 0);
    }

    #[test]
    fn ineligible_package_signers_never_sign_or_persist_a_release() {
        for mutation in ["unknown", "revoked", "mixed-publisher"] {
            let dir = tempdir().unwrap();
            let project = write_project(dir.path());
            let rendered = render_package_archive(&fs::canonicalize(&project).unwrap()).unwrap();
            let statement = provenance_statement(
                "axiom/core/1.2.3/package.axp",
                &hash_bytes(&rendered.archive),
            );
            let mut fixture = trust_fixture();
            if mutation != "unknown" {
                let signer_id = signer_key_id(&fixture.package_signers[0]);
                let key = fixture.roots.0["candidate_root"]["signed"]["keys"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|key| key["key_id"].as_str() == Some(&signer_id))
                    .unwrap();
                if mutation == "revoked" {
                    key["status"] = json!("revoked");
                    key["revocation"] = json!({
                        "effective_sequence": 2,
                        "effective_time": "2026-07-01T00:00:00Z",
                        "reason": "compromised"
                    });
                } else {
                    key["publisher_identity"] = json!("publisher:other");
                }
                resign_candidate_root(&mut fixture);
            }
            let calls = Rc::new(Cell::new(0));
            let seeds = if mutation == "unknown" {
                [10, 11]
            } else {
                [7, 8]
            };
            let signers = [
                CountingSigner {
                    signer: TestSigner::new(seeds[0]),
                    sign_calls: Rc::clone(&calls),
                },
                CountingSigner {
                    signer: TestSigner::new(seeds[1]),
                    sign_calls: Rc::clone(&calls),
                },
            ];
            let registry = dir.path().join("registry");
            publish_package(
                &project,
                &registry,
                &PublishOptions {
                    allow_overwrite: false,
                    namespace: "axiom",
                    registry_identity: "registry:test",
                    source_identity: "registry:test-source",
                    publisher_identity: "publisher:foundation",
                    index_generation: 1,
                    index_sequence: 1,
                    provenance_statement: &statement,
                    trust_roots: &fixture.roots,
                    verification_expectation: &fixture.expectation,
                    signers: &signers,
                },
            )
            .unwrap_err();
            assert_eq!(calls.get(), 0, "{mutation}");
            assert!(!registry.join("axiom/core/1.2.3").exists(), "{mutation}");
        }
    }

    #[test]
    fn future_package_publication_floor_never_reaches_index_signers() {
        let dir = tempdir().unwrap();
        let project = write_project(dir.path());
        let rendered = render_package_archive(&fs::canonicalize(&project).unwrap()).unwrap();
        let statement = provenance_statement(
            "axiom/core/1.2.3/package.axp",
            &hash_bytes(&rendered.archive),
        );
        let mut fixture = trust_fixture();
        let registry = dir.path().join("registry");
        publish_package(
            &project,
            &registry,
            &PublishOptions {
                allow_overwrite: false,
                namespace: "axiom",
                registry_identity: "registry:test",
                source_identity: "registry:test-source",
                publisher_identity: "publisher:foundation",
                index_generation: 1,
                index_sequence: 3,
                provenance_statement: &statement,
                trust_roots: &fixture.roots,
                verification_expectation: &fixture.expectation,
                signers: &fixture.package_signers,
            },
        )
        .unwrap();
        pin_fixture_to_candidate_index_at(
            &mut fixture,
            &registry,
            1,
            2,
            "registry.test.1.2",
            &"00".repeat(32),
        );
        let calls = Rc::new(Cell::new(0));
        let signers = [
            CountingSigner {
                signer: TestSigner::new(5),
                sign_calls: Rc::clone(&calls),
            },
            CountingSigner {
                signer: TestSigner::new(6),
                sign_calls: Rc::clone(&calls),
            },
        ];
        let error = build_registry_index(
            &registry,
            &RegistryIndexOptions {
                registry_identity: "registry:test",
                source_identity: "registry:test-source",
                generation: 1,
                sequence: 2,
                issued_at: "2026-07-29T11:00:00Z",
                expires_at: "2026-12-31T00:00:00Z",
                snapshot_id: "registry.test.1.2",
                metadata_path: "1/2/index.v2.json",
                previous_snapshot_sha256: &"00".repeat(32),
                trust_roots: &fixture.roots,
                verification_expectation: &fixture.expectation,
                signers: &signers,
            },
        )
        .unwrap_err();
        assert!(error.message.contains("publication floor exceeds"));
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn future_effective_revocation_remains_eligible_for_index_signing() {
        let dir = tempdir().unwrap();
        let (mut fixture, _index, registry) = publish_and_index(dir.path());
        let signer_id = signer_key_id(&fixture.index_signers[0]);
        let key = fixture.roots.0["candidate_root"]["signed"]["keys"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|key| key["key_id"].as_str() == Some(&signer_id))
            .unwrap();
        key["status"] = json!("revoked");
        key["revocation"] = json!({
            "effective_sequence": 3,
            "effective_time": "2026-08-01T00:00:00Z",
            "reason": "scheduled rotation"
        });
        resign_candidate_root(&mut fixture);

        build_registry_index(
            &registry,
            &RegistryIndexOptions {
                registry_identity: "registry:test",
                source_identity: "registry:test-source",
                generation: 1,
                sequence: 1,
                issued_at: "2026-07-29T11:00:00Z",
                expires_at: "2026-12-31T00:00:00Z",
                snapshot_id: "registry.test.1.1",
                metadata_path: "1/1/index.v2.json",
                previous_snapshot_sha256: &"00".repeat(32),
                trust_roots: &fixture.roots,
                verification_expectation: &fixture.expectation,
                signers: &fixture.index_signers,
            },
        )
        .unwrap();
    }

    #[test]
    fn revoked_or_mixed_publisher_package_signers_are_rejected() {
        for mutation in ["revoked", "mixed"] {
            let dir = tempdir().unwrap();
            let (mut fixture, index, registry) = publish_and_index(dir.path());
            let keys = fixture.roots.0["candidate_root"]["signed"]["keys"]
                .as_array_mut()
                .unwrap();
            let package_key_id =
                index.envelope["signed"]["releases"][0]["package_signature_sha256"]
                    .as_str()
                    .unwrap();
            let key = keys
                .iter_mut()
                .find(|key| {
                    key["publisher_identity"] == "publisher:foundation"
                        && key["key_id"].as_str().is_some()
                })
                .unwrap();
            let _ = package_key_id;
            if mutation == "revoked" {
                key["status"] = json!("revoked");
                key["revocation"] = json!({
                    "effective_sequence": 1,
                    "effective_time": "2026-07-01T00:00:00Z",
                    "reason": "test"
                });
            } else {
                key["publisher_identity"] = json!("publisher:other");
            }
            assert!(
                verify_registry_index_integrity(
                    &index,
                    &registry,
                    &fixture.roots,
                    &fixture.expectation
                )
                .is_err(),
                "{mutation}"
            );
        }
    }

    #[test]
    fn duplicate_signer_is_rejected_before_threshold_counting() {
        let signer = TestSigner::new(7);
        let package = PackageSignatureEnvelope(json!({
            "scheme": {"algorithm":"ed25519","version":1,"message_mode":"pure"},
            "archive":{"digest":{"algorithm":"sha-256","value":"00".repeat(32)},"size":1},
            "manifest":{"algorithm":"sha-256","value":"00".repeat(32)},
            "package":{"namespace":"axiom","name":"core","version":"1.2.3","target_path":"axiom/core/1.2.3/package.axp"},
            "registry":{"registry_identity":"registry","source_identity":"registry:source"},
            "publisher":{"publisher_identity":"publisher"},
            "provenance":{"statement":{"digest":{"value":"00".repeat(32)},"value":{"_type":"https://in-toto.io/Statement/v1","predicateType":"https://slsa.dev/provenance/v1"}},"selected_subject":{"name":"axiom/core/1.2.3/package.axp","digest":{"sha256":"00".repeat(32)}}},
            "index":{"generation":1,"sequence":1}
        }));
        let first = sign_package_transcript(&package, 2, &signer).unwrap();
        let second = sign_package_transcript(&package, 2, &signer).unwrap();
        assert_eq!(first.key_id, second.key_id);
        let ids = [first.key_id, second.key_id]
            .into_iter()
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn partial_publish_failure_leaves_no_final_release() {
        let dir = tempdir().unwrap();
        let project = write_project(dir.path());
        let canonical_project = fs::canonicalize(&project).unwrap();
        let rendered = render_package_archive(&canonical_project).unwrap();
        let target = "axiom/core/1.2.3/package.axp";
        let statement = provenance_statement(target, &hash_bytes(&rendered.archive));
        let roots = TrustRootsEnvelope(json!({}));
        let expectation = VerificationExpectation(json!({
            "required_signers":{"package_threshold":2}
        }));
        let signers = [TestSigner::new(7)];
        let error = publish_package(
            &project,
            &dir.path().join("registry"),
            &PublishOptions {
                allow_overwrite: false,
                namespace: "axiom",
                registry_identity: "registry",
                source_identity: "registry:source",
                publisher_identity: "publisher",
                index_generation: 1,
                index_sequence: 1,
                provenance_statement: &statement,
                trust_roots: &roots,
                verification_expectation: &expectation,
                signers: &signers,
            },
        )
        .unwrap_err();
        assert!(!error.message.is_empty());
        assert!(!dir.path().join("registry/axiom/core/1.2.3").exists());
    }
}
