//! Content-addressed package cache and deterministic vendor snapshots.
//!
//! This module proves byte and tree integrity. Callers must still rerun Package
//! Trust over [`CachedPackage::verified_artifacts`] before treating the package
//! as authenticated; a rehashed cache record is deliberately not a trust verdict.

use crate::package_archive::{
    ArchiveError, ArchiveLimits, EXTRACTOR_VERSION, TreeIntegrityManifest, expected_tree_integrity,
    extract_archive, integrity_manifest_bytes, integrity_manifest_sha256, parse_archive,
    sha256_hex, verify_tree, verify_tree_with_limits,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const CACHE_COMMIT_SCHEMA: &str = "axiom.package_cache_commit.v1";
const VENDOR_MANIFEST_SCHEMA: &str = "axiom.vendor_manifest.v1";
const TRANSACTION_MARKER_SCHEMA: &str = "axiom.store_transaction.v1";
const TRANSACTION_MARKER_NAME: &str = ".axiom-transaction";
const MAX_EVIDENCE_BYTES: usize = 16 * 1024 * 1024;
const MAX_VENDOR_PACKAGES: usize = 4_096;
const MAX_VENDOR_BYTES: u64 = 512 * 1024 * 1024;
const STALE_TRANSACTION_AGE_NANOS: u128 = 24 * 60 * 60 * 1_000_000_000;
const EVIDENCE_NAMES: [&str; 6] = [
    "integrity.json",
    "manifest",
    "provenance",
    "registry-index",
    "signature",
    "verification",
];
static TRANSACTION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreError {
    pub code: &'static str,
    pub message: String,
}

impl StoreError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for StoreError {}

impl From<ArchiveError> for StoreError {
    fn from(error: ArchiveError) -> Self {
        Self {
            code: error.code,
            message: error.message,
        }
    }
}

/// Secure bounded reader shared by package-store and manager trust inputs.
///
/// On Unix, nonblocking no-follow open happens before descriptor metadata
/// validation so a FIFO substitution cannot stall the caller.
pub fn read_bounded_regular_file(path: &Path, limit: usize) -> Result<Vec<u8>, StoreError> {
    read_regular_file(path, limit)
}

#[derive(Clone, Copy, Debug)]
pub struct VerifiedArtifacts<'a> {
    pub archive_sha256: &'a str,
    pub archive: &'a [u8],
    pub manifest: &'a [u8],
    pub provenance: &'a [u8],
    pub signature: &'a [u8],
    pub registry_index: &'a [u8],
    pub verification: &'a [u8],
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CacheCommit {
    pub schema_version: String,
    pub extractor_version: String,
    pub archive_sha256: String,
    pub archive_length: u64,
    pub tree_manifest_sha256: String,
    pub manifest_sha256: String,
    pub provenance_sha256: String,
    pub signature_sha256: String,
    pub registry_index_sha256: String,
    pub verification_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RehashedArtifacts {
    pub archive: Vec<u8>,
    pub manifest: Vec<u8>,
    pub provenance: Vec<u8>,
    pub signature: Vec<u8>,
    pub registry_index: Vec<u8>,
    pub verification: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedPackage {
    pub archive_sha256: String,
    pub blob: PathBuf,
    pub tree: PathBuf,
    pub evidence: PathBuf,
    pub integrity: TreeIntegrityManifest,
    pub commit: CacheCommit,
    pub artifacts: RehashedArtifacts,
}

impl CachedPackage {
    /// Exact bytes rehashed by offline cache verification. These are inputs to,
    /// not a replacement for, a fresh Package Trust verification.
    pub fn verified_artifacts(&self) -> Result<VerifiedArtifacts<'_>, StoreError> {
        verify_tree(&self.tree, &self.integrity)?;
        verify_evidence_directory(&self.evidence)?;
        let current = read_artifacts(&self.blob, &self.evidence, ArchiveLimits::default())?;
        if current != self.artifacts {
            return Err(StoreError::new(
                "cache_artifact_mismatch",
                "cached artifacts changed after package verification",
            ));
        }
        Ok(VerifiedArtifacts {
            archive_sha256: &self.archive_sha256,
            archive: &self.artifacts.archive,
            manifest: &self.artifacts.manifest,
            provenance: &self.artifacts.provenance,
            signature: &self.artifacts.signature,
            registry_index: &self.artifacts.registry_index,
            verification: &self.artifacts.verification,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VendorPackage<'a> {
    pub package_id: &'a str,
    pub archive_sha256: &'a str,
    pub registry_index_sha256: &'a str,
    pub verification_sha256: &'a str,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VendorManifestPackage {
    pub package_id: String,
    pub content_key: String,
    pub archive_sha256: String,
    pub registry_index_sha256: String,
    pub verification_sha256: String,
    pub evidence_identity: String,
    pub tree_manifest_sha256: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VendorManifest {
    pub schema_version: String,
    pub packages: Vec<VendorManifestPackage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VendorSnapshot {
    pub digest: String,
    pub root: PathBuf,
    pub manifest: VendorManifest,
    pub packages: BTreeMap<String, VendorPackagePaths>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VendorPackagePaths {
    pub archive_sha256: String,
    pub registry_index_sha256: String,
    pub verification_sha256: String,
    pub evidence_identity: String,
    pub blob: PathBuf,
    pub tree: PathBuf,
    pub evidence: PathBuf,
    pub commit: PathBuf,
}

impl VendorSnapshot {
    /// Return an exact, reverified tree root for one expected locked package.
    pub fn package_tree(&self, package_id: &str) -> Option<PathBuf> {
        self.packages
            .get(package_id)
            .map(|package| package.tree.clone())
    }

    /// Load and rehash one exact package so callers retain at most one package's
    /// archive and evidence buffers while traversing a vendor snapshot.
    pub fn package(&self, package_id: &str) -> Result<Option<CachedPackage>, StoreError> {
        let Some(package) = self.packages.get(package_id) else {
            return Ok(None);
        };
        let cached = load_package_from_paths(
            &package.archive_sha256,
            &package.blob,
            &package.tree,
            &package.evidence,
            &package.commit,
            &package.registry_index_sha256,
            &package.evidence_identity,
            ArchiveLimits::default(),
        )?;
        if cached.commit.verification_sha256 != package.verification_sha256 {
            return Err(StoreError::new(
                "vendor_evidence_selector_mismatch",
                "vendor package path does not match its exact verification digest",
            ));
        }
        Ok(Some(cached))
    }
}

#[derive(Clone, Copy, Debug)]
struct VendorLimits {
    max_packages: usize,
    max_bytes: u64,
}

impl Default for VendorLimits {
    fn default() -> Self {
        Self {
            max_packages: MAX_VENDOR_PACKAGES,
            max_bytes: MAX_VENDOR_BYTES,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PackageStore {
    root: PathBuf,
    limits: ArchiveLimits,
    vendor_limits: VendorLimits,
}

impl PackageStore {
    /// Open a project-relative cache root without following configured path
    /// components beneath the trusted anchor.
    pub fn open_anchored(anchor: &Path, configured: &Path) -> Result<Self, StoreError> {
        let root = secure_anchored_root(anchor, configured, true)?;
        Self::with_limits(&root, ArchiveLimits::default())
    }

    /// Create and validate a configured root beneath a trusted anchor.
    pub fn prepare_anchored_root(anchor: &Path, configured: &Path) -> Result<PathBuf, StoreError> {
        secure_anchored_root(anchor, configured, true)
    }

    /// Validate an existing configured root beneath a trusted anchor.
    pub fn require_anchored_root(anchor: &Path, configured: &Path) -> Result<PathBuf, StoreError> {
        secure_anchored_root(anchor, configured, false)
    }

    pub fn open(root: &Path) -> Result<Self, StoreError> {
        Self::with_limits(root, ArchiveLimits::default())
    }

    pub fn with_limits(root: &Path, limits: ArchiveLimits) -> Result<Self, StoreError> {
        ensure_directory_tree(root)?;
        for relative in [
            "blobs/sha256",
            "trees/axiom-package-extractor-v1/sha256",
            "evidence/sha256",
            "commits/sha256",
            ".transactions",
        ] {
            ensure_directory_tree(&root.join(relative))?;
        }
        Ok(Self {
            root: root.to_path_buf(),
            limits,
            vendor_limits: VendorLimits::default(),
        })
    }

    pub fn admit(&self, artifacts: VerifiedArtifacts<'_>) -> Result<CachedPackage, StoreError> {
        let parsed = parse_archive(artifacts.archive, artifacts.archive_sha256, self.limits)?;
        let registry_index_sha256 = sha256_hex(artifacts.registry_index);
        let verification_sha256 = sha256_hex(artifacts.verification);
        let evidence_identity = evidence_identity(&registry_index_sha256, &verification_sha256);
        let transaction = unique_transaction(&self.root.join(".transactions"), "admit")?;
        let result = (|| {
            let blob = transaction.join("blob");
            write_new_file(&blob, artifacts.archive)?;
            let tree = transaction.join("tree");
            let integrity = extract_archive(&parsed, &tree)?;
            let evidence = transaction.join("evidence");
            create_synced_directory(&evidence)?;
            write_new_file(
                &evidence.join("integrity.json"),
                &integrity_manifest_bytes(&integrity)?,
            )?;
            write_new_file(&evidence.join("manifest"), artifacts.manifest)?;
            write_new_file(&evidence.join("provenance"), artifacts.provenance)?;
            write_new_file(&evidence.join("signature"), artifacts.signature)?;
            write_new_file(&evidence.join("registry-index"), artifacts.registry_index)?;
            write_new_file(&evidence.join("verification"), artifacts.verification)?;
            sync_directory(&evidence)?;

            let commit = build_commit(&integrity, artifacts);
            let commit_bytes = canonical_json_bytes(&commit)?;
            write_new_file(&transaction.join("commit.json"), &commit_bytes)?;
            let digest = artifacts.archive_sha256;
            let final_blob = self.blob_path(digest);
            let final_tree = self.tree_path(digest);
            ensure_directory_tree(&self.evidence_archive_path(digest))?;
            ensure_directory_tree(&self.commit_archive_path(digest))?;
            ensure_directory_tree(&self.commit_index_path(digest, &registry_index_sha256))?;
            let final_evidence = self.evidence_path(digest, &evidence_identity);
            let final_commit = self.commit_path(digest, &registry_index_sha256, &evidence_identity);
            publish_file(&blob, &final_blob, artifacts.archive)?;
            publish_directory(&tree, &final_tree)?;
            verify_tree_with_limits(&final_tree, &integrity, self.limits)?;
            publish_directory(&evidence, &final_evidence)?;
            verify_evidence_directory(&final_evidence)?;
            // The commit is the admission marker and is deliberately published last.
            publish_file(
                &transaction.join("commit.json"),
                &final_commit,
                &commit_bytes,
            )?;
            self.load_verified_identity(digest, &registry_index_sha256, &evidence_identity)
        })();
        finish_transaction(result, &transaction)
    }

    pub fn load_verified(&self, archive_sha256: &str) -> Result<CachedPackage, StoreError> {
        validate_digest(archive_sha256)?;
        let versions = self.evidence_versions(archive_sha256)?;
        if versions.len() != 1 {
            return Err(StoreError::new(
                if versions.is_empty() {
                    "cache_evidence_unavailable"
                } else {
                    "cache_evidence_ambiguous"
                },
                format!(
                    "archive {archive_sha256} has {} committed evidence versions; select an exact registry index",
                    versions.len()
                ),
            ));
        }
        self.load_verified_identity(archive_sha256, &versions[0].0, &versions[0].1)
    }

    pub fn load_verified_for_index(
        &self,
        archive_sha256: &str,
        registry_index_sha256: &str,
    ) -> Result<CachedPackage, StoreError> {
        validate_digest(archive_sha256)?;
        validate_digest(registry_index_sha256)?;
        let identities = self.evidence_versions_for_index(archive_sha256, registry_index_sha256)?;
        if identities.len() != 1 {
            return Err(StoreError::new(
                if identities.is_empty() {
                    "cache_evidence_unavailable"
                } else {
                    "cache_evidence_ambiguous"
                },
                format!(
                    "archive {archive_sha256} has {} evidence versions for registry index {registry_index_sha256}",
                    identities.len()
                ),
            ));
        }
        self.load_verified_identity(archive_sha256, registry_index_sha256, &identities[0])
    }

    pub fn load_verified_exact(
        &self,
        archive_sha256: &str,
        registry_index_sha256: &str,
        verification_sha256: &str,
    ) -> Result<CachedPackage, StoreError> {
        validate_digest(archive_sha256)?;
        validate_digest(registry_index_sha256)?;
        validate_digest(verification_sha256)?;
        self.load_verified_identity(
            archive_sha256,
            registry_index_sha256,
            &evidence_identity(registry_index_sha256, verification_sha256),
        )
    }

    fn load_verified_identity(
        &self,
        archive_sha256: &str,
        registry_index_sha256: &str,
        identity: &str,
    ) -> Result<CachedPackage, StoreError> {
        validate_digest(identity)?;
        load_package_from_paths(
            archive_sha256,
            &self.blob_path(archive_sha256),
            &self.tree_path(archive_sha256),
            &self.evidence_path(archive_sha256, identity),
            &self.commit_path(archive_sha256, registry_index_sha256, identity),
            registry_index_sha256,
            identity,
            self.limits,
        )
    }

    pub fn vendor_snapshot(
        &self,
        vendor_root: &Path,
        packages: &[VendorPackage<'_>],
    ) -> Result<VendorSnapshot, StoreError> {
        let expected = canonical_expected_packages(packages, self.vendor_limits)?;
        ensure_directory_tree(vendor_root)?;
        ensure_directory_tree(&vendor_root.join("snapshots/sha256"))?;
        ensure_directory_tree(&vendor_root.join(".transactions"))?;
        let (manifest_bytes, digest) = self.preflight_vendor_snapshot(&expected)?;
        let final_snapshot = vendor_root.join("snapshots/sha256").join(&digest);
        if let Ok(verified) = verify_vendor_snapshot_at(
            &final_snapshot,
            &digest,
            &expected,
            self.vendor_limits,
        ) {
            atomic_replace_file(
                &vendor_root.join("CURRENT"),
                format!("{digest}\n").as_bytes(),
            )?;
            return Ok(verified);
        }
        let transaction = unique_transaction(&vendor_root.join(".transactions"), "vendor")?;
        let result = (|| {
            let snapshot = transaction.join("snapshot");
            create_synced_directory(&snapshot)?;
            let package_root = snapshot.join("packages/sha256");
            ensure_directory_tree(&package_root)?;

            let mut budget = VendorBudget::new(self.vendor_limits, expected.len())?;
            let mut copied_digests = BTreeSet::new();
            let mut copied_evidence = BTreeSet::new();
            for (_package_id, archive_sha256, registry_index_sha256, verification_sha256) in
                &expected
            {
                let package = self.load_verified_exact(
                    archive_sha256,
                    registry_index_sha256,
                    verification_sha256,
                )?;
                let identity = evidence_identity(registry_index_sha256, verification_sha256);
                let new_archive = copied_digests.insert(archive_sha256.clone());
                let new_evidence =
                    copied_evidence.insert((archive_sha256.clone(), identity.clone()));
                budget.account_package(&package, new_archive, new_evidence)?;
                let artifacts = package.verified_artifacts()?;
                let destination = package_root.join(&package.archive_sha256);
                if new_archive {
                    create_synced_directory(&destination)?;
                    write_new_file(&destination.join("archive"), artifacts.archive)?;
                    let parsed =
                        parse_archive(artifacts.archive, &package.archive_sha256, self.limits)?;
                    extract_archive(&parsed, &destination.join("tree"))?;
                    create_synced_directory(&destination.join("evidence"))?;
                    create_synced_directory(&destination.join("commits"))?;
                }
                if new_evidence {
                    let evidence = destination.join("evidence").join(&identity);
                    create_synced_directory(&evidence)?;
                    write_new_file(
                        &evidence.join("integrity.json"),
                        &integrity_manifest_bytes(&package.integrity)?,
                    )?;
                    write_new_file(&evidence.join("manifest"), artifacts.manifest)?;
                    write_new_file(&evidence.join("provenance"), artifacts.provenance)?;
                    write_new_file(&evidence.join("signature"), artifacts.signature)?;
                    write_new_file(&evidence.join("registry-index"), artifacts.registry_index)?;
                    write_new_file(&evidence.join("verification"), artifacts.verification)?;
                    write_new_file(
                        &destination.join("commits").join(format!("{identity}.json")),
                        &canonical_json_bytes(&package.commit)?,
                    )?;
                    sync_directory(&evidence)?;
                    sync_directory(&destination.join("evidence"))?;
                    sync_directory(&destination.join("commits"))?;
                }
                sync_directory(&destination)?;
            }
            sync_directory(&package_root)?;
            sync_directory(&snapshot.join("packages"))?;

            budget.account_bytes(manifest_bytes.len() as u64)?;
            write_new_file(&snapshot.join("vendor-manifest.json"), &manifest_bytes)?;
            sync_directory(&snapshot)?;
            publish_directory(&snapshot, &final_snapshot)?;
            let verified =
                verify_vendor_snapshot_at(&final_snapshot, &digest, &expected, self.vendor_limits)?;
            atomic_replace_file(
                &vendor_root.join("CURRENT"),
                format!("{digest}\n").as_bytes(),
            )?;
            Ok(verified)
        })();
        finish_transaction(result, &transaction)
    }

    fn preflight_vendor_snapshot(
        &self,
        expected: &[ExpectedVendorPackage],
    ) -> Result<(Vec<u8>, String), StoreError> {
        let mut budget = VendorBudget::new(self.vendor_limits, expected.len())?;
        let mut copied_digests = BTreeSet::new();
        let mut copied_evidence = BTreeSet::new();
        let mut records = Vec::with_capacity(expected.len());
        for (package_id, archive_sha256, registry_index_sha256, verification_sha256) in expected {
            let package = self.load_verified_exact(
                archive_sha256,
                registry_index_sha256,
                verification_sha256,
            )?;
            let identity = evidence_identity(registry_index_sha256, verification_sha256);
            let new_archive = copied_digests.insert(archive_sha256.clone());
            let new_evidence =
                copied_evidence.insert((archive_sha256.clone(), identity.clone()));
            budget.account_package(&package, new_archive, new_evidence)?;
            records.push(VendorManifestPackage {
                package_id: package_id.clone(),
                content_key: format!("sha256:{archive_sha256}"),
                archive_sha256: archive_sha256.clone(),
                registry_index_sha256: registry_index_sha256.clone(),
                verification_sha256: verification_sha256.clone(),
                evidence_identity: identity,
                tree_manifest_sha256: package.commit.tree_manifest_sha256.clone(),
            });
        }
        let manifest = VendorManifest {
            schema_version: VENDOR_MANIFEST_SCHEMA.to_owned(),
            packages: records,
        };
        let manifest_bytes = canonical_json_bytes(&manifest)?;
        budget.account_bytes(manifest_bytes.len() as u64)?;
        let digest = sha256_hex(&manifest_bytes);
        Ok((manifest_bytes, digest))
    }

    /// Rehash the current snapshot and require the caller's exact locked
    /// registry package IDs and archive digests.
    pub fn verify_vendor_snapshot(
        vendor_root: &Path,
        expected_packages: &[VendorPackage<'_>],
    ) -> Result<VendorSnapshot, StoreError> {
        let limits = VendorLimits::default();
        let expected = canonical_expected_packages(expected_packages, limits)?;
        let current = read_regular_file(&vendor_root.join("CURRENT"), 65)?;
        let current = std::str::from_utf8(&current)
            .map_err(|_| StoreError::new("vendor_current_invalid", "CURRENT is not UTF-8"))?;
        let digest = current.strip_suffix('\n').ok_or_else(|| {
            StoreError::new(
                "vendor_current_invalid",
                "CURRENT must end in one canonical newline",
            )
        })?;
        validate_digest(digest)?;
        let root = vendor_root.join("snapshots/sha256").join(digest);
        verify_vendor_snapshot_at(&root, digest, &expected, limits)
    }

    pub fn verify_vendor_snapshot_exact(
        vendor_root: &Path,
        expected_packages: &[VendorPackage<'_>],
    ) -> Result<VendorSnapshot, StoreError> {
        Self::verify_vendor_snapshot(vendor_root, expected_packages)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn blob_path(&self, digest: &str) -> PathBuf {
        self.root.join("blobs/sha256").join(digest)
    }

    fn tree_path(&self, digest: &str) -> PathBuf {
        self.root
            .join("trees")
            .join(EXTRACTOR_VERSION)
            .join("sha256")
            .join(digest)
    }

    fn evidence_archive_path(&self, digest: &str) -> PathBuf {
        self.root.join("evidence/sha256").join(digest)
    }

    fn evidence_path(&self, digest: &str, identity: &str) -> PathBuf {
        self.evidence_archive_path(digest).join(identity)
    }

    fn commit_archive_path(&self, digest: &str) -> PathBuf {
        self.root.join("commits/sha256").join(digest)
    }

    fn commit_index_path(&self, digest: &str, registry_index_sha256: &str) -> PathBuf {
        self.commit_archive_path(digest).join(registry_index_sha256)
    }

    fn commit_path(&self, digest: &str, registry_index_sha256: &str, identity: &str) -> PathBuf {
        self.commit_index_path(digest, registry_index_sha256)
            .join(format!("{identity}.json"))
    }

    fn evidence_versions(&self, digest: &str) -> Result<Vec<(String, String)>, StoreError> {
        let directory = self.commit_archive_path(digest);
        match fs::symlink_metadata(&directory) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(StoreError::new(
                    "cache_evidence_unavailable",
                    format!("failed to stat {}: {error}", directory.display()),
                ));
            }
            Ok(_) => require_safe_directory(&directory)?,
        }
        let mut versions = Vec::new();
        for entry in fs::read_dir(&directory).map_err(|error| {
            StoreError::new(
                "cache_evidence_unavailable",
                format!("failed to read {}: {error}", directory.display()),
            )
        })? {
            let entry = entry.map_err(|error| {
                StoreError::new("cache_evidence_unavailable", error.to_string())
            })?;
            let index = entry.file_name().into_string().map_err(|_| {
                StoreError::new(
                    "cache_evidence_invalid",
                    "commit index directory is not UTF-8",
                )
            })?;
            validate_digest(&index)?;
            for identity in self.evidence_versions_for_index(digest, &index)? {
                versions.push((index.clone(), identity));
                if versions.len() > 1 {
                    return Ok(versions);
                }
            }
        }
        versions.sort();
        versions.dedup();
        Ok(versions)
    }

    fn evidence_versions_for_index(
        &self,
        digest: &str,
        registry_index_sha256: &str,
    ) -> Result<Vec<String>, StoreError> {
        let directory = self.commit_index_path(digest, registry_index_sha256);
        match fs::symlink_metadata(&directory) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(StoreError::new(
                    "cache_evidence_unavailable",
                    format!("failed to stat {}: {error}", directory.display()),
                ));
            }
            Ok(_) => require_safe_directory(&directory)?,
        }
        let mut identities = Vec::new();
        for entry in fs::read_dir(&directory).map_err(|error| {
            StoreError::new(
                "cache_evidence_unavailable",
                format!("failed to read {}: {error}", directory.display()),
            )
        })? {
            let entry = entry.map_err(|error| {
                StoreError::new("cache_evidence_unavailable", error.to_string())
            })?;
            let name = entry.file_name().into_string().map_err(|_| {
                StoreError::new(
                    "cache_evidence_invalid",
                    "commit version filename is not UTF-8",
                )
            })?;
            let identity = name.strip_suffix(".json").ok_or_else(|| {
                StoreError::new(
                    "cache_evidence_invalid",
                    format!("unexpected commit entry {name:?}"),
                )
            })?;
            validate_digest(identity)?;
            let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
                StoreError::new("cache_evidence_unavailable", error.to_string())
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(StoreError::new(
                    "cache_evidence_invalid",
                    format!("commit version {name:?} is not a regular file"),
                ));
            }
            identities.push(identity.to_owned());
            if identities.len() > 1 {
                return Ok(identities);
            }
        }
        identities.sort();
        identities.dedup();
        Ok(identities)
    }
}

type ExpectedVendorPackage = (String, String, String, String);

struct VendorBudget {
    limits: VendorLimits,
    bytes: u64,
}

impl VendorBudget {
    fn new(limits: VendorLimits, packages: usize) -> Result<Self, StoreError> {
        if packages > limits.max_packages {
            return Err(StoreError::new(
                "vendor_package_count_exceeded",
                format!(
                    "vendor snapshot requests {packages} packages; limit is {}",
                    limits.max_packages
                ),
            ));
        }
        Ok(Self { limits, bytes: 0 })
    }

    fn account_bytes(&mut self, bytes: u64) -> Result<(), StoreError> {
        self.bytes = self.bytes.checked_add(bytes).ok_or_else(|| {
            StoreError::new(
                "vendor_total_bytes_exceeded",
                "vendor snapshot byte count overflowed",
            )
        })?;
        if self.bytes > self.limits.max_bytes {
            return Err(StoreError::new(
                "vendor_total_bytes_exceeded",
                format!(
                    "vendor snapshot requires {} bytes; limit is {}",
                    self.bytes, self.limits.max_bytes
                ),
            ));
        }
        Ok(())
    }

    fn account_package(
        &mut self,
        package: &CachedPackage,
        new_archive: bool,
        new_evidence: bool,
    ) -> Result<(), StoreError> {
        if new_archive {
            let tree_bytes = package
                .integrity
                .files
                .iter()
                .try_fold(0u64, |total, file| {
                    total.checked_add(file.length).ok_or_else(|| {
                        StoreError::new(
                            "vendor_total_bytes_exceeded",
                            "vendor tree byte count overflowed",
                        )
                    })
                })?;
            self.account_bytes(package.commit.archive_length)?;
            self.account_bytes(tree_bytes)?;
        }
        if new_evidence {
            let integrity_bytes = integrity_manifest_bytes(&package.integrity)?.len() as u64;
            let commit_bytes = canonical_json_bytes(&package.commit)?.len() as u64;
            let evidence_bytes = [
                package.artifacts.manifest.len(),
                package.artifacts.provenance.len(),
                package.artifacts.signature.len(),
                package.artifacts.registry_index.len(),
                package.artifacts.verification.len(),
            ]
            .into_iter()
            .try_fold(0u64, |total, length| {
                total.checked_add(length as u64).ok_or_else(|| {
                    StoreError::new(
                        "vendor_total_bytes_exceeded",
                        "vendor evidence byte count overflowed",
                    )
                })
            })?;
            self.account_bytes(integrity_bytes)?;
            self.account_bytes(commit_bytes)?;
            self.account_bytes(evidence_bytes)?;
        }
        Ok(())
    }
}

fn verify_vendor_snapshot_at(
    root: &Path,
    digest: &str,
    expected: &[ExpectedVendorPackage],
    limits: VendorLimits,
) -> Result<VendorSnapshot, StoreError> {
    validate_digest(digest)?;
    require_safe_directory(root)?;
    let manifest_bytes = read_regular_file(&root.join("vendor-manifest.json"), MAX_EVIDENCE_BYTES)?;
    if sha256_hex(&manifest_bytes) != digest {
        return Err(StoreError::new(
            "vendor_manifest_digest_mismatch",
            "snapshot path does not name the exact vendor manifest bytes",
        ));
    }
    let manifest: VendorManifest = parse_json_exact(&manifest_bytes, "vendor_manifest_invalid")?;
    if canonical_json_bytes(&manifest)? != manifest_bytes {
        return Err(StoreError::new(
            "vendor_manifest_noncanonical",
            "vendor manifest is not the canonical byte representation",
        ));
    }
    let mut budget = VendorBudget::new(limits, manifest.packages.len())?;
    budget.account_bytes(manifest_bytes.len() as u64)?;
    validate_vendor_manifest(&manifest, expected)?;
    verify_vendor_layout(root, &manifest)?;

    let mut seen_archives = BTreeSet::new();
    let mut seen_evidence = BTreeSet::new();
    let mut packages = BTreeMap::new();
    for record in &manifest.packages {
        let paths = vendor_package_paths(root, record);
        let package = load_package_from_paths(
            &record.archive_sha256,
            &paths.blob,
            &paths.tree,
            &paths.evidence,
            &paths.commit,
            &record.registry_index_sha256,
            &record.evidence_identity,
            ArchiveLimits::default(),
        )?;
        if package.commit.tree_manifest_sha256 != record.tree_manifest_sha256 {
            return Err(StoreError::new(
                "vendor_tree_manifest_mismatch",
                format!(
                    "vendor record for {:?} does not bind its exact tree",
                    record.package_id
                ),
            ));
        }
        if package.commit.registry_index_sha256 != record.registry_index_sha256
            || package.commit.verification_sha256 != record.verification_sha256
        {
            return Err(StoreError::new(
                "vendor_evidence_selector_mismatch",
                format!(
                    "vendor record for {:?} does not bind its exact evidence selector",
                    record.package_id
                ),
            ));
        }
        let new_archive = seen_archives.insert(record.archive_sha256.clone());
        let new_evidence = seen_evidence.insert((
            record.archive_sha256.clone(),
            record.evidence_identity.clone(),
        ));
        budget.account_package(&package, new_archive, new_evidence)?;
        packages.insert(record.package_id.clone(), paths);
    }
    Ok(VendorSnapshot {
        digest: digest.to_owned(),
        root: root.to_path_buf(),
        manifest,
        packages,
    })
}

fn vendor_package_paths(root: &Path, record: &VendorManifestPackage) -> VendorPackagePaths {
    let package_root = root.join("packages/sha256").join(&record.archive_sha256);
    VendorPackagePaths {
        archive_sha256: record.archive_sha256.clone(),
        registry_index_sha256: record.registry_index_sha256.clone(),
        verification_sha256: record.verification_sha256.clone(),
        evidence_identity: record.evidence_identity.clone(),
        blob: package_root.join("archive"),
        tree: package_root.join("tree"),
        evidence: package_root
            .join("evidence")
            .join(&record.evidence_identity),
        commit: package_root
            .join("commits")
            .join(format!("{}.json", record.evidence_identity)),
    }
}

fn build_commit(
    integrity: &TreeIntegrityManifest,
    artifacts: VerifiedArtifacts<'_>,
) -> CacheCommit {
    CacheCommit {
        schema_version: CACHE_COMMIT_SCHEMA.to_owned(),
        extractor_version: EXTRACTOR_VERSION.to_owned(),
        archive_sha256: artifacts.archive_sha256.to_owned(),
        archive_length: artifacts.archive.len() as u64,
        tree_manifest_sha256: integrity_manifest_sha256(integrity)
            .expect("validated integrity manifest must serialize"),
        manifest_sha256: sha256_hex(artifacts.manifest),
        provenance_sha256: sha256_hex(artifacts.provenance),
        signature_sha256: sha256_hex(artifacts.signature),
        registry_index_sha256: sha256_hex(artifacts.registry_index),
        verification_sha256: sha256_hex(artifacts.verification),
    }
}

fn load_package_from_paths(
    archive_sha256: &str,
    blob_path: &Path,
    tree_path: &Path,
    evidence_path: &Path,
    commit_path: &Path,
    expected_registry_index_sha256: &str,
    expected_evidence_identity: &str,
    limits: ArchiveLimits,
) -> Result<CachedPackage, StoreError> {
    validate_digest(archive_sha256)?;
    verify_evidence_directory(evidence_path)?;
    let artifacts = read_artifacts(blob_path, evidence_path, limits)?;
    let archive = &artifacts.archive;
    let parsed_archive = parse_archive(archive, archive_sha256, limits)?;
    let signed_integrity = expected_tree_integrity(&parsed_archive)?;
    let integrity_bytes =
        read_regular_file(&evidence_path.join("integrity.json"), MAX_EVIDENCE_BYTES)?;
    let integrity: TreeIntegrityManifest =
        parse_json_exact(&integrity_bytes, "integrity_manifest_invalid")?;
    if integrity_manifest_bytes(&integrity)? != integrity_bytes {
        return Err(StoreError::new(
            "integrity_manifest_noncanonical",
            "integrity.json is not the canonical byte representation",
        ));
    }
    if integrity != signed_integrity {
        return Err(StoreError::new(
            "archive_tree_binding_mismatch",
            "tree integrity manifest is not derived from the exact signed archive bytes",
        ));
    }
    verify_tree(tree_path, &signed_integrity)?;

    let commit_bytes = read_regular_file(commit_path, MAX_EVIDENCE_BYTES)?;
    let commit: CacheCommit = parse_json_exact(&commit_bytes, "cache_commit_invalid")?;
    if canonical_json_bytes(&commit)? != commit_bytes {
        return Err(StoreError::new(
            "cache_commit_noncanonical",
            "cache commit is not the canonical byte representation",
        ));
    }
    let expected = CacheCommit {
        schema_version: CACHE_COMMIT_SCHEMA.to_owned(),
        extractor_version: EXTRACTOR_VERSION.to_owned(),
        archive_sha256: archive_sha256.to_owned(),
        archive_length: artifacts.archive.len() as u64,
        tree_manifest_sha256: integrity_manifest_sha256(&signed_integrity)?,
        manifest_sha256: sha256_hex(&artifacts.manifest),
        provenance_sha256: sha256_hex(&artifacts.provenance),
        signature_sha256: sha256_hex(&artifacts.signature),
        registry_index_sha256: sha256_hex(&artifacts.registry_index),
        verification_sha256: sha256_hex(&artifacts.verification),
    };
    if commit != expected {
        return Err(StoreError::new(
            "cache_commit_mismatch",
            "cache commit does not bind the exact blob, tree, and evidence bytes",
        ));
    }
    if commit.registry_index_sha256 != expected_registry_index_sha256 {
        return Err(StoreError::new(
            "cache_registry_index_mismatch",
            "cache commit is stored under a different registry index",
        ));
    }
    if evidence_identity(&commit.registry_index_sha256, &commit.verification_sha256)
        != expected_evidence_identity
    {
        return Err(StoreError::new(
            "cache_evidence_identity_mismatch",
            "cache evidence path does not bind its exact index and verification bytes",
        ));
    }
    Ok(CachedPackage {
        archive_sha256: archive_sha256.to_owned(),
        blob: blob_path.to_path_buf(),
        tree: tree_path.to_path_buf(),
        evidence: evidence_path.to_path_buf(),
        integrity: signed_integrity,
        commit,
        artifacts,
    })
}

fn read_artifacts(
    blob_path: &Path,
    evidence_path: &Path,
    limits: ArchiveLimits,
) -> Result<RehashedArtifacts, StoreError> {
    Ok(RehashedArtifacts {
        archive: read_regular_file(blob_path, limits.max_archive_bytes)?,
        manifest: read_regular_file(&evidence_path.join("manifest"), MAX_EVIDENCE_BYTES)?,
        provenance: read_regular_file(&evidence_path.join("provenance"), MAX_EVIDENCE_BYTES)?,
        signature: read_regular_file(&evidence_path.join("signature"), MAX_EVIDENCE_BYTES)?,
        registry_index: read_regular_file(
            &evidence_path.join("registry-index"),
            MAX_EVIDENCE_BYTES,
        )?,
        verification: read_regular_file(&evidence_path.join("verification"), MAX_EVIDENCE_BYTES)?,
    })
}

fn validate_vendor_manifest(
    manifest: &VendorManifest,
    expected: &[ExpectedVendorPackage],
) -> Result<(), StoreError> {
    if manifest.schema_version != VENDOR_MANIFEST_SCHEMA {
        return Err(StoreError::new(
            "vendor_manifest_version_invalid",
            "unsupported vendor manifest schema",
        ));
    }
    let mut actual = Vec::with_capacity(manifest.packages.len());
    let mut previous: Option<&str> = None;
    for package in &manifest.packages {
        if package.package_id.is_empty()
            || previous.is_some_and(|value| value >= package.package_id.as_str())
        {
            return Err(StoreError::new(
                "vendor_manifest_order_invalid",
                "vendor package IDs must be nonempty and strictly sorted",
            ));
        }
        validate_digest(&package.archive_sha256)?;
        validate_digest(&package.registry_index_sha256)?;
        validate_digest(&package.verification_sha256)?;
        validate_digest(&package.evidence_identity)?;
        validate_digest(&package.tree_manifest_sha256)?;
        if package.content_key != format!("sha256:{}", package.archive_sha256) {
            return Err(StoreError::new(
                "vendor_content_key_invalid",
                "vendor content key does not match archive digest",
            ));
        }
        if package.evidence_identity
            != evidence_identity(&package.registry_index_sha256, &package.verification_sha256)
        {
            return Err(StoreError::new(
                "vendor_evidence_identity_invalid",
                "vendor evidence identity does not match its exact index and verification digests",
            ));
        }
        actual.push((
            package.package_id.clone(),
            package.archive_sha256.clone(),
            package.registry_index_sha256.clone(),
            package.verification_sha256.clone(),
        ));
        previous = Some(&package.package_id);
    }
    if actual != expected {
        return Err(StoreError::new(
            "vendor_lock_mismatch",
            "vendor snapshot does not exactly match expected locked registry packages",
        ));
    }
    Ok(())
}

fn canonical_expected_packages(
    packages: &[VendorPackage<'_>],
    limits: VendorLimits,
) -> Result<Vec<ExpectedVendorPackage>, StoreError> {
    if packages.len() > limits.max_packages {
        return Err(StoreError::new(
            "vendor_package_count_exceeded",
            format!(
                "vendor snapshot requests {} packages; limit is {}",
                packages.len(),
                limits.max_packages
            ),
        ));
    }
    let mut expected = Vec::with_capacity(packages.len());
    let mut ids = BTreeSet::new();
    for package in packages {
        if package.package_id.is_empty() || !ids.insert(package.package_id) {
            return Err(StoreError::new(
                "vendor_package_invalid",
                "vendor package IDs must be nonempty and unique",
            ));
        }
        validate_digest(package.archive_sha256)?;
        validate_digest(package.registry_index_sha256)?;
        validate_digest(package.verification_sha256)?;
        expected.push((
            package.package_id.to_owned(),
            package.archive_sha256.to_owned(),
            package.registry_index_sha256.to_owned(),
            package.verification_sha256.to_owned(),
        ));
    }
    expected.sort();
    Ok(expected)
}

fn verify_vendor_layout(root: &Path, manifest: &VendorManifest) -> Result<(), StoreError> {
    verify_directory_names(root, &["packages", "vendor-manifest.json"])?;
    let packages = root.join("packages");
    verify_directory_names(&packages, &["sha256"])?;
    let mut evidence_by_digest = BTreeMap::<&str, BTreeSet<&str>>::new();
    for package in &manifest.packages {
        evidence_by_digest
            .entry(&package.archive_sha256)
            .or_default()
            .insert(&package.evidence_identity);
    }
    let digest_names = evidence_by_digest.keys().copied().collect::<Vec<_>>();
    verify_directory_names(&packages.join("sha256"), &digest_names)?;
    for (digest, identities) in evidence_by_digest {
        let package_root = packages.join("sha256").join(digest);
        verify_directory_names(&package_root, &["archive", "commits", "evidence", "tree"])?;
        let identity_names = identities.iter().copied().collect::<Vec<_>>();
        verify_directory_names(&package_root.join("evidence"), &identity_names)?;
        let commit_names = identities
            .iter()
            .map(|identity| format!("{identity}.json"))
            .collect::<Vec<_>>();
        let commit_name_refs = commit_names.iter().map(String::as_str).collect::<Vec<_>>();
        verify_directory_names(&package_root.join("commits"), &commit_name_refs)?;
    }
    Ok(())
}

fn verify_directory_names(path: &Path, expected: &[&str]) -> Result<(), StoreError> {
    require_safe_directory(path)?;
    let mut remaining = expected.iter().copied().collect::<BTreeSet<_>>();
    if remaining.len() != expected.len() {
        return Err(StoreError::new(
            "store_expected_entry_set_invalid",
            format!("expected entry names for {} are not unique", path.display()),
        ));
    }
    let entries = fs::read_dir(path).map_err(|error| {
        StoreError::new(
            "store_directory_unavailable",
            format!("failed to read {}: {error}", path.display()),
        )
    })?;
    for entry in entries {
        let name = entry
            .map_err(|error| StoreError::new("store_directory_unavailable", error.to_string()))?
            .file_name()
            .into_string()
            .map_err(|_| {
                StoreError::new(
                    "store_entry_invalid",
                    format!("{} contains a non-UTF-8 entry", path.display()),
                )
            })?;
        if !remaining.remove(name.as_str()) {
            return Err(StoreError::new(
                "store_entry_set_mismatch",
                format!("{} has missing or unexpected entries", path.display()),
            ));
        }
    }
    if !remaining.is_empty() {
        return Err(StoreError::new(
            "store_entry_set_mismatch",
            format!("{} has missing or unexpected entries", path.display()),
        ));
    }
    Ok(())
}

fn canonical_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, StoreError> {
    let mut bytes = serde_json::to_vec(value).map_err(|error| {
        StoreError::new(
            "canonical_json_failed",
            format!("failed to serialize store record: {error}"),
        )
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn parse_json_exact<T: DeserializeOwned>(
    bytes: &[u8],
    code: &'static str,
) -> Result<T, StoreError> {
    serde_json::from_slice(bytes)
        .map_err(|error| StoreError::new(code, format!("invalid JSON record: {error}")))
}

fn validate_digest(digest: &str) -> Result<(), StoreError> {
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(StoreError::new(
            "content_digest_invalid",
            "content digest must be 64 lowercase hexadecimal characters",
        ));
    }
    Ok(())
}

fn evidence_identity(registry_index_sha256: &str, verification_sha256: &str) -> String {
    sha256_hex(
        format!(
            "axiom-package-evidence-v1\nregistry-index-sha256={registry_index_sha256}\nverification-sha256={verification_sha256}\n"
        )
        .as_bytes(),
    )
}

fn verify_evidence_directory(path: &Path) -> Result<(), StoreError> {
    require_safe_directory(path)?;
    let mut remaining = EVIDENCE_NAMES.into_iter().collect::<BTreeSet<_>>();
    let entries = fs::read_dir(path).map_err(|error| {
        StoreError::new(
            "evidence_unavailable",
            format!("failed to read {}: {error}", path.display()),
        )
    })?;
    for entry in entries {
        let name = entry
            .map_err(|error| StoreError::new("evidence_unavailable", error.to_string()))?
            .file_name()
            .into_string()
            .map_err(|_| {
                StoreError::new("evidence_entry_invalid", "evidence filename is not UTF-8")
            })?;
        if !remaining.remove(name.as_str()) {
            return Err(StoreError::new(
                "evidence_set_mismatch",
                "evidence directory has missing or unexpected entries",
            ));
        }
    }
    if !remaining.is_empty() {
        return Err(StoreError::new(
            "evidence_set_mismatch",
            "evidence directory has missing or unexpected entries",
        ));
    }
    for name in EVIDENCE_NAMES {
        let metadata = fs::symlink_metadata(path.join(name)).map_err(|error| {
            StoreError::new(
                "evidence_unavailable",
                format!("failed to stat evidence {name}: {error}"),
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(StoreError::new(
                "evidence_entry_invalid",
                format!("evidence {name} must be a regular non-symlink file"),
            ));
        }
    }
    Ok(())
}

fn secure_anchored_root(
    anchor: &Path,
    configured: &Path,
    create: bool,
) -> Result<PathBuf, StoreError> {
    let relative = if configured.is_absolute() {
        configured.strip_prefix(anchor).map_err(|_| {
            StoreError::new(
                "store_root_escape",
                format!(
                    "{} is not beneath trusted anchor {}",
                    configured.display(),
                    anchor.display()
                ),
            )
        })?
    } else {
        configured
    };
    let mut names = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(name) => names.push(name.to_owned()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(StoreError::new(
                    "store_root_escape",
                    format!(
                        "configured root {:?} escapes trusted anchor {}",
                        configured,
                        anchor.display()
                    ),
                ));
            }
        }
    }
    if names.is_empty() {
        return Err(StoreError::new(
            "store_root_invalid",
            "configured root must name a directory beneath the trusted anchor",
        ));
    }

    #[cfg(unix)]
    {
        secure_anchored_root_unix(anchor, &names, create)
    }
    #[cfg(not(unix))]
    {
        secure_anchored_root_portable(anchor, &names, create)
    }
}

#[cfg(unix)]
fn secure_anchored_root_unix(
    anchor: &Path,
    names: &[std::ffi::OsString],
    create: bool,
) -> Result<PathBuf, StoreError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    let mut directory = options.open(anchor).map_err(|error| {
        StoreError::new(
            "store_root_invalid",
            format!(
                "failed to securely open anchor {}: {error}",
                anchor.display()
            ),
        )
    })?;
    if !directory
        .metadata()
        .map_err(|error| StoreError::new("store_root_invalid", error.to_string()))?
        .is_dir()
    {
        return Err(StoreError::new(
            "store_root_invalid",
            format!("trusted anchor {} is not a directory", anchor.display()),
        ));
    }

    let mut path = anchor.to_path_buf();
    for component in names {
        let name = CString::new(component.as_os_str().as_bytes()).map_err(|_| {
            StoreError::new(
                "store_root_invalid",
                "configured root component contains a NUL byte",
            )
        })?;
        let open_component = |parent: &File| -> std::io::Result<File> {
            let descriptor = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY
                        | libc::O_DIRECTORY
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC
                        | libc::O_NONBLOCK,
                    0,
                )
            };
            if descriptor < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(unsafe { File::from_raw_fd(descriptor) })
            }
        };
        let child = match open_component(&directory) {
            Ok(child) => child,
            Err(error) if create && error.kind() == std::io::ErrorKind::NotFound => {
                let created = unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) };
                if created != 0 {
                    let create_error = std::io::Error::last_os_error();
                    if create_error.kind() != std::io::ErrorKind::AlreadyExists {
                        return Err(StoreError::new(
                            "store_directory_create_failed",
                            format!(
                                "failed to securely create {:?} beneath {}: {create_error}",
                                name,
                                path.display()
                            ),
                        ));
                    }
                }
                let child = open_component(&directory).map_err(|error| {
                    StoreError::new(
                        "store_root_invalid",
                        format!(
                            "failed to securely open {:?} beneath {}: {error}",
                            name,
                            path.display()
                        ),
                    )
                })?;
                child.sync_all().map_err(|error| {
                    StoreError::new(
                        "store_sync_failed",
                        format!(
                            "failed to sync new directory below {}: {error}",
                            path.display()
                        ),
                    )
                })?;
                directory.sync_all().map_err(|error| {
                    StoreError::new(
                        "store_sync_failed",
                        format!("failed to sync {}: {error}", path.display()),
                    )
                })?;
                child
            }
            Err(error) => {
                return Err(StoreError::new(
                    if error.kind() == std::io::ErrorKind::NotFound {
                        "store_directory_unavailable"
                    } else {
                        "store_root_invalid"
                    },
                    format!(
                        "failed to securely open {:?} beneath {}: {error}",
                        name,
                        path.display()
                    ),
                ));
            }
        };
        path.push(component);
        directory = child;
    }
    Ok(path)
}

#[cfg(not(unix))]
fn secure_anchored_root_portable(
    anchor: &Path,
    names: &[std::ffi::OsString],
    create: bool,
) -> Result<PathBuf, StoreError> {
    let mut current = anchor.to_path_buf();
    require_safe_directory(&current)?;
    for name in names {
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if unsafe_directory_metadata(&metadata) {
                    return Err(StoreError::new(
                        "store_root_invalid",
                        format!(
                            "{} is a symlink, reparse point, or non-directory",
                            current.display()
                        ),
                    ));
                }
            }
            Err(error) if create && error.kind() == std::io::ErrorKind::NotFound => {
                create_synced_directory(&current)?;
            }
            Err(error) => {
                return Err(StoreError::new(
                    "store_directory_unavailable",
                    format!("failed to inspect {}: {error}", current.display()),
                ));
            }
        }
    }
    Ok(current)
}

fn unsafe_directory_metadata(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !metadata.is_dir()
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink() || !metadata.is_dir()
    }
}

fn create_synced_directory(path: &Path) -> Result<(), StoreError> {
    fs::create_dir(path).map_err(|error| {
        StoreError::new(
            "store_directory_create_failed",
            format!("failed to create {}: {error}", path.display()),
        )
    })?;
    sync_directory(path)?;
    sync_parent_directory(path)
}

fn ensure_directory_tree(path: &Path) -> Result<(), StoreError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if unsafe_directory_metadata(&metadata) {
            return Err(StoreError::new(
                "store_directory_invalid",
                format!("{} is not a safe directory", path.display()),
            ));
        }
        return Ok(());
    }
    let parent = path.parent().ok_or_else(|| {
        StoreError::new(
            "store_directory_invalid",
            format!("{} has no parent", path.display()),
        )
    })?;
    if parent != path {
        ensure_directory_tree(parent)?;
    }
    match fs::create_dir(path) {
        Ok(()) => {
            sync_directory(path)?;
            sync_parent_directory(path)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            require_safe_directory(path)
        }
        Err(error) => Err(StoreError::new(
            "store_directory_create_failed",
            format!("failed to create {}: {error}", path.display()),
        )),
    }
}

fn require_safe_directory(path: &Path) -> Result<(), StoreError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        StoreError::new(
            "store_directory_unavailable",
            format!("failed to stat {}: {error}", path.display()),
        )
    })?;
    if unsafe_directory_metadata(&metadata) {
        return Err(StoreError::new(
            "store_directory_invalid",
            format!("{} must be a non-symlink directory", path.display()),
        ));
    }
    Ok(())
}

fn unique_transaction(parent: &Path, prefix: &str) -> Result<PathBuf, StoreError> {
    require_safe_directory(parent)?;
    let now = unix_epoch_nanos()?;
    reap_stale_transactions(parent, now)?;
    for _ in 0..128 {
        let sequence = TRANSACTION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let epoch_nanos = unix_epoch_nanos()?;
        let pid = std::process::id();
        let path = parent.join(format!(".{prefix}-{pid}-{epoch_nanos}-{sequence}",));
        match fs::create_dir(&path) {
            Ok(()) => {
                let marker = transaction_marker_bytes(pid, epoch_nanos, sequence);
                if let Err(error) = write_new_file(&path.join(TRANSACTION_MARKER_NAME), &marker) {
                    return Err(cleanup_created_transaction(error, &path));
                }
                if let Err(error) = sync_directory(&path) {
                    return Err(cleanup_created_transaction(error, &path));
                }
                if let Err(error) = sync_parent_directory(&path) {
                    return Err(cleanup_created_transaction(error, &path));
                }
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(StoreError::new(
                    "store_transaction_create_failed",
                    format!("failed to create {}: {error}", path.display()),
                ));
            }
        }
    }
    Err(StoreError::new(
        "store_transaction_exhausted",
        "could not allocate a unique store transaction",
    ))
}

fn cleanup_created_transaction(error: StoreError, path: &Path) -> StoreError {
    match fs::remove_dir_all(path) {
        Ok(()) => error,
        Err(cleanup) => StoreError::new(
            "store_transaction_cleanup_failed",
            format!(
                "{error}; additionally failed to clean {}: {cleanup}",
                path.display()
            ),
        ),
    }
}

fn unix_epoch_nanos() -> Result<u128, StoreError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|_| {
            StoreError::new(
                "store_clock_invalid",
                "system clock precedes the Unix epoch",
            )
        })
}

fn transaction_marker_bytes(pid: u32, epoch_nanos: u128, sequence: u64) -> Vec<u8> {
    format!(
        "{TRANSACTION_MARKER_SCHEMA}\npid={pid}\ncreated_unix_nanos={epoch_nanos}\nsequence={sequence}\n"
    )
    .into_bytes()
}

fn parse_transaction_name(name: &str) -> Option<(u32, u128, u64)> {
    let name = name.strip_prefix('.')?;
    let (prefix_and_pid, sequence) = name.rsplit_once('-')?;
    let (prefix_and_pid, epoch_nanos) = prefix_and_pid.rsplit_once('-')?;
    let (prefix, pid) = prefix_and_pid.rsplit_once('-')?;
    if !matches!(prefix, "admit" | "vendor" | "replace") {
        return None;
    }
    Some((
        pid.parse().ok()?,
        epoch_nanos.parse().ok()?,
        sequence.parse().ok()?,
    ))
}

fn reap_stale_transactions(parent: &Path, now: u128) -> Result<(), StoreError> {
    let entries = fs::read_dir(parent).map_err(|error| {
        StoreError::new(
            "store_transaction_cleanup_failed",
            format!(
                "failed to inspect transactions in {}: {error}",
                parent.display()
            ),
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            StoreError::new("store_transaction_cleanup_failed", error.to_string())
        })?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Some((pid, epoch_nanos, sequence)) = parse_transaction_name(&name) else {
            continue;
        };
        if now.saturating_sub(epoch_nanos) < STALE_TRANSACTION_AGE_NANOS || process_is_alive(pid) {
            continue;
        }
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        let expected = transaction_marker_bytes(pid, epoch_nanos, sequence);
        let Ok(marker) = read_regular_file(&path.join(TRANSACTION_MARKER_NAME), 256) else {
            continue;
        };
        if marker != expected {
            continue;
        }
        match fs::remove_dir_all(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(StoreError::new(
                    "store_transaction_cleanup_failed",
                    format!("failed to remove stale {}: {error}", path.display()),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return true;
    };
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn process_is_alive(_pid: u32) -> bool {
    false
}

fn finish_transaction<T>(
    result: Result<T, StoreError>,
    transaction: &Path,
) -> Result<T, StoreError> {
    match fs::remove_dir_all(transaction) {
        Ok(()) => result,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => result,
        Err(cleanup) => match result {
            Ok(_) => Err(StoreError::new(
                "store_transaction_cleanup_failed",
                format!("failed to clean {}: {cleanup}", transaction.display()),
            )),
            Err(mut primary) => {
                primary.message.push_str(&format!(
                    "; additionally failed to clean {}: {cleanup}",
                    transaction.display()
                ));
                Err(primary)
            }
        },
    }
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| {
        StoreError::new(
            "store_write_failed",
            format!("failed to create {}: {error}", path.display()),
        )
    })?;
    file.write_all(bytes).map_err(|error| {
        StoreError::new(
            "store_write_failed",
            format!("failed to write {}: {error}", path.display()),
        )
    })?;
    file.sync_all().map_err(|error| {
        StoreError::new(
            "store_sync_failed",
            format!("failed to sync {}: {error}", path.display()),
        )
    })
}

fn read_regular_file(path: &Path, limit: usize) -> Result<Vec<u8>, StoreError> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            StoreError::new(
                "store_file_unavailable",
                format!("failed to inspect {}: {error}", path.display()),
            )
        })?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(StoreError::new(
                "store_file_invalid",
                format!("{} is a reparse point", path.display()),
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
    let mut file = options.open(path).map_err(|error| {
        StoreError::new(
            "store_file_unavailable",
            format!("failed to open {}: {error}", path.display()),
        )
    })?;
    let metadata = file.metadata().map_err(|error| {
        StoreError::new(
            "store_file_unavailable",
            format!("failed to stat {}: {error}", path.display()),
        )
    })?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err(StoreError::new(
            "store_file_invalid",
            format!("{} is not a bounded regular file", path.display()),
        ));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| {
            StoreError::new(
                "store_read_failed",
                format!("failed to read {}: {error}", path.display()),
            )
        })?;
    if bytes.len() > limit || bytes.len() as u64 != metadata.len() {
        return Err(StoreError::new(
            "store_file_raced",
            format!("{} changed while being read", path.display()),
        ));
    }
    Ok(bytes)
}

fn publish_file(source: &Path, destination: &Path, expected: &[u8]) -> Result<(), StoreError> {
    match fs::hard_link(source, destination) {
        Ok(()) => {
            sync_directory(destination.parent().unwrap_or(Path::new(".")))?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if read_regular_file(destination, expected.len())? == expected {
                Ok(())
            } else {
                Err(StoreError::new(
                    "content_address_collision",
                    format!("existing {} has different bytes", destination.display()),
                ))
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(StoreError::new(
            "publish_source_unavailable",
            format!("failed to publish {}: {error}", source.display()),
        )),
        Err(error) => Err(StoreError::new(
            "store_publish_failed",
            format!(
                "failed to publish {} to {}: {error}",
                source.display(),
                destination.display()
            ),
        )),
    }
}

fn publish_directory(source: &Path, destination: &Path) -> Result<(), StoreError> {
    match fs::rename(source, destination) {
        Ok(()) => {
            sync_directory(destination.parent().unwrap_or(Path::new(".")))?;
            Ok(())
        }
        Err(_error) if destination.exists() => {
            require_safe_directory(destination)?;
            Ok(())
        }
        Err(error) => Err(StoreError::new(
            "store_publish_failed",
            format!(
                "failed to publish {} to {}: {error}",
                source.display(),
                destination.display()
            ),
        )),
    }
}

fn atomic_replace_file(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let parent = path.parent().ok_or_else(|| {
        StoreError::new(
            "store_write_failed",
            format!("{} has no parent", path.display()),
        )
    })?;
    let transaction = unique_transaction(parent, "replace")?;
    let temporary = transaction.join("value");
    let result = (|| {
        write_new_file(&temporary, bytes)?;
        replace_file(&temporary, path).map_err(|error| {
            StoreError::new(
                "store_publish_failed",
                format!("failed to replace {}: {error}", path.display()),
            )
        })?;
        sync_directory(parent)
    })();
    finish_transaction(result, &transaction)
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    // `rename` cannot replace an existing file on Windows. MoveFileEx with
    // REPLACE_EXISTING preserves the same publication contract as rename on
    // Unix while WRITE_THROUGH makes the replacement durable before return.
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(
            existing_file_name: *const u16,
            new_file_name: *const u16,
            flags: u32,
        ) -> i32;
    }

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let replaced = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if replaced == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn sync_directory(path: &Path) -> Result<(), StoreError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| {
            StoreError::new(
                "store_sync_failed",
                format!("failed to sync {}: {error}", path.display()),
            )
        })
}

fn sync_parent_directory(path: &Path) -> Result<(), StoreError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    sync_directory(parent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package_archive::ARCHIVE_MAGIC;

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

    fn fixture<'a>(archive: &'a [u8], digest: &'a str) -> VerifiedArtifacts<'a> {
        fixture_version(archive, digest, b"registry-index", b"verification")
    }

    fn fixture_version<'a>(
        archive: &'a [u8],
        digest: &'a str,
        registry_index: &'a [u8],
        verification: &'a [u8],
    ) -> VerifiedArtifacts<'a> {
        VerifiedArtifacts {
            archive_sha256: digest,
            archive,
            manifest: b"manifest",
            provenance: b"provenance",
            signature: b"signature",
            registry_index,
            verification,
        }
    }

    #[test]
    fn admission_is_idempotent_and_offline_load_returns_exact_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let store = PackageStore::open(&temp.path().join("cache")).unwrap();
        let bytes = archive(&[("axiom.toml", b"[package]\n"), ("src/main.ax", b"main")]);
        let digest = sha256_hex(&bytes);
        let first = store.admit(fixture(&bytes, &digest)).unwrap();
        let second = store.admit(fixture(&bytes, &digest)).unwrap();
        assert_eq!(first.commit, second.commit);
        let artifacts = second.verified_artifacts().unwrap();
        assert_eq!(artifacts.archive, bytes);
        assert_eq!(artifacts.manifest, b"manifest");
        assert_eq!(fs::read(second.tree.join("src/main.ax")).unwrap(), b"main");
    }

    #[test]
    fn same_archive_preserves_two_exact_registry_index_evidence_versions() {
        let temp = tempfile::tempdir().unwrap();
        let store = PackageStore::open(&temp.path().join("cache")).unwrap();
        let bytes = archive(&[("a", b"x")]);
        let digest = sha256_hex(&bytes);
        let old_index = b"registry-index-generation-1";
        let new_index = b"registry-index-generation-2-yanked";
        store
            .admit(fixture_version(
                &bytes,
                &digest,
                old_index,
                b"verification-generation-1",
            ))
            .unwrap();
        store
            .admit(fixture_version(
                &bytes,
                &digest,
                new_index,
                b"verification-generation-2",
            ))
            .unwrap();

        assert_eq!(
            store.load_verified(&digest).unwrap_err().code,
            "cache_evidence_ambiguous"
        );
        let old = store
            .load_verified_for_index(&digest, &sha256_hex(old_index))
            .unwrap();
        let new = store
            .load_verified_for_index(&digest, &sha256_hex(new_index))
            .unwrap();
        assert_eq!(old.verified_artifacts().unwrap().registry_index, old_index);
        assert_eq!(new.verified_artifacts().unwrap().registry_index, new_index);
        assert_eq!(old.blob, new.blob);
        assert_eq!(old.tree, new.tree);
        assert_ne!(old.evidence, new.evidence);

        fs::write(
            old.evidence.join("verification"),
            b"corrupt unrelated evidence",
        )
        .unwrap();
        let exact_new = store
            .load_verified_for_index(&digest, &sha256_hex(new_index))
            .unwrap();
        assert_eq!(
            exact_new.verified_artifacts().unwrap().registry_index,
            new_index
        );
    }

    #[test]
    fn offline_load_rejects_blob_tree_evidence_and_commit_tampering() {
        for target in ["blob", "tree", "evidence", "commit"] {
            let temp = tempfile::tempdir().unwrap();
            let store = PackageStore::open(&temp.path().join("cache")).unwrap();
            let bytes = archive(&[("a", b"x")]);
            let digest = sha256_hex(&bytes);
            let cached = store.admit(fixture(&bytes, &digest)).unwrap();
            match target {
                "blob" => fs::write(&cached.blob, b"bad").unwrap(),
                "tree" => fs::write(cached.tree.join("a"), b"bad").unwrap(),
                "evidence" => fs::write(cached.evidence.join("manifest"), b"bad").unwrap(),
                "commit" => fs::write(
                    store.commit_path(
                        &digest,
                        &sha256_hex(b"registry-index"),
                        cached
                            .evidence
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap(),
                    ),
                    b"{\"schema_version\":\"forged\"}\n",
                )
                .unwrap(),
                _ => unreachable!(),
            }
            assert!(store.load_verified(&digest).is_err(), "{target}");
        }
    }

    #[test]
    fn offline_load_rejects_coherently_recomputed_tree_integrity_and_commit() {
        let temp = tempfile::tempdir().unwrap();
        let store = PackageStore::open(&temp.path().join("cache")).unwrap();
        let bytes = archive(&[("a", b"x")]);
        let digest = sha256_hex(&bytes);
        let cached = store.admit(fixture(&bytes, &digest)).unwrap();
        fs::write(cached.tree.join("a"), b"y").unwrap();

        let mut forged_integrity = cached.integrity.clone();
        forged_integrity.files[0].sha256 = sha256_hex(b"y");
        fs::write(
            cached.evidence.join("integrity.json"),
            integrity_manifest_bytes(&forged_integrity).unwrap(),
        )
        .unwrap();
        let mut forged_commit = cached.commit.clone();
        forged_commit.tree_manifest_sha256 = integrity_manifest_sha256(&forged_integrity).unwrap();
        let identity = cached
            .evidence
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap();
        fs::write(
            store.commit_path(&digest, &forged_commit.registry_index_sha256, identity),
            canonical_json_bytes(&forged_commit).unwrap(),
        )
        .unwrap();

        assert_eq!(
            store
                .load_verified_for_index(&digest, &forged_commit.registry_index_sha256,)
                .unwrap_err()
                .code,
            "archive_tree_binding_mismatch"
        );
    }

    #[cfg(unix)]
    #[test]
    fn offline_load_rejects_symlink_substitution() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let store = PackageStore::open(&temp.path().join("cache")).unwrap();
        let bytes = archive(&[("a", b"x")]);
        let digest = sha256_hex(&bytes);
        let cached = store.admit(fixture(&bytes, &digest)).unwrap();
        fs::remove_file(cached.tree.join("a")).unwrap();
        symlink("/dev/null", cached.tree.join("a")).unwrap();
        assert!(store.load_verified(&digest).is_err());
    }

    #[test]
    fn vendor_snapshot_binds_expected_lock_and_exposes_verified_tree() {
        let temp = tempfile::tempdir().unwrap();
        let store = PackageStore::open(&temp.path().join("cache")).unwrap();
        let bytes = archive(&[("a", b"x")]);
        let digest = sha256_hex(&bytes);
        store.admit(fixture(&bytes, &digest)).unwrap();
        let index_digest = sha256_hex(b"registry-index");
        let verification_digest = sha256_hex(b"verification");
        let expected = [VendorPackage {
            package_id: "registry:demo/core@1.0.0",
            archive_sha256: &digest,
            registry_index_sha256: &index_digest,
            verification_sha256: &verification_digest,
        }];
        let vendor = temp.path().join("vendor");
        let snapshot = store.vendor_snapshot(&vendor, &expected).unwrap();
        let reused = store.vendor_snapshot(&vendor, &expected).unwrap();
        assert_eq!(reused.digest, snapshot.digest);
        assert_eq!(vendor.join("snapshots/sha256").read_dir().unwrap().count(), 1);
        assert_eq!(
            fs::read(
                snapshot
                    .package_tree(expected[0].package_id)
                    .unwrap()
                    .join("a")
            )
            .unwrap(),
            b"x"
        );
        assert!(PackageStore::verify_vendor_snapshot(&vendor, &expected).is_ok());
        let wrong = [VendorPackage {
            package_id: "registry:demo/other@1.0.0",
            archive_sha256: &digest,
            registry_index_sha256: &index_digest,
            verification_sha256: &verification_digest,
        }];
        assert_eq!(
            PackageStore::verify_vendor_snapshot(&vendor, &wrong)
                .unwrap_err()
                .code,
            "vendor_lock_mismatch"
        );
    }

    #[test]
    fn current_pointer_replacement_overwrites_an_existing_file() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("replacement");
        let destination = temp.path().join("CURRENT");
        write_new_file(&source, b"new\n").unwrap();
        fs::write(&destination, b"old\n").unwrap();

        replace_file(&source, &destination).unwrap();

        assert_eq!(fs::read(&destination).unwrap(), b"new\n");
        assert!(!source.exists());
    }

    #[test]
    fn vendor_supports_shared_archive_with_exact_distinct_evidence_identities() {
        let temp = tempfile::tempdir().unwrap();
        let store = PackageStore::open(&temp.path().join("cache")).unwrap();
        let bytes = archive(&[("a", b"x")]);
        let digest = sha256_hex(&bytes);
        let index = b"registry-index";
        let verification_one = b"verification-1";
        let verification_two = b"verification-2";
        store
            .admit(fixture_version(&bytes, &digest, index, verification_one))
            .unwrap();
        store
            .admit(fixture_version(&bytes, &digest, index, verification_two))
            .unwrap();
        let index_digest = sha256_hex(index);
        let verification_one_digest = sha256_hex(verification_one);
        let verification_two_digest = sha256_hex(verification_two);
        let packages = [
            VendorPackage {
                package_id: "registry:demo/one@1.0.0",
                archive_sha256: &digest,
                registry_index_sha256: &index_digest,
                verification_sha256: &verification_one_digest,
            },
            VendorPackage {
                package_id: "registry:demo/two@1.0.0",
                archive_sha256: &digest,
                registry_index_sha256: &index_digest,
                verification_sha256: &verification_two_digest,
            },
        ];
        let vendor = temp.path().join("vendor");
        let snapshot = store.vendor_snapshot(&vendor, &packages).unwrap();
        let one = snapshot.package(packages[0].package_id).unwrap().unwrap();
        let two = snapshot.package(packages[1].package_id).unwrap().unwrap();
        assert_eq!(
            one.verified_artifacts().unwrap().verification,
            verification_one
        );
        assert_eq!(
            two.verified_artifacts().unwrap().verification,
            verification_two
        );
        assert_eq!(one.tree, two.tree);
        assert_ne!(one.evidence, two.evidence);
        assert_eq!(
            snapshot
                .root
                .join("packages/sha256")
                .read_dir()
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn vendor_enforces_aggregate_package_and_byte_budgets_before_publication() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = PackageStore::open(&temp.path().join("cache")).unwrap();
        let bytes = archive(&[("a", b"x")]);
        let digest = sha256_hex(&bytes);
        store.admit(fixture(&bytes, &digest)).unwrap();
        let index_digest = sha256_hex(b"registry-index");
        let verification_digest = sha256_hex(b"verification");
        let expected = [VendorPackage {
            package_id: "registry:demo/core@1.0.0",
            archive_sha256: &digest,
            registry_index_sha256: &index_digest,
            verification_sha256: &verification_digest,
        }];
        let vendor = temp.path().join("vendor");

        store.vendor_limits = VendorLimits {
            max_packages: 0,
            max_bytes: MAX_VENDOR_BYTES,
        };
        assert_eq!(
            store.vendor_snapshot(&vendor, &expected).unwrap_err().code,
            "vendor_package_count_exceeded"
        );

        store.vendor_limits = VendorLimits {
            max_packages: 1,
            max_bytes: 1,
        };
        assert_eq!(
            store.vendor_snapshot(&vendor, &expected).unwrap_err().code,
            "vendor_total_bytes_exceeded"
        );
        assert!(!vendor.join("CURRENT").exists());
    }

    #[test]
    fn corrupt_existing_same_digest_snapshot_never_replaces_current() {
        let temp = tempfile::tempdir().unwrap();
        let store = PackageStore::open(&temp.path().join("cache")).unwrap();
        let first_bytes = archive(&[("a", b"first")]);
        let second_bytes = archive(&[("a", b"second")]);
        let first_digest = sha256_hex(&first_bytes);
        let second_digest = sha256_hex(&second_bytes);
        store.admit(fixture(&first_bytes, &first_digest)).unwrap();
        store.admit(fixture(&second_bytes, &second_digest)).unwrap();
        let index_digest = sha256_hex(b"registry-index");
        let verification_digest = sha256_hex(b"verification");
        let first = [VendorPackage {
            package_id: "registry:demo/first@1.0.0",
            archive_sha256: &first_digest,
            registry_index_sha256: &index_digest,
            verification_sha256: &verification_digest,
        }];
        let second = [VendorPackage {
            package_id: "registry:demo/second@1.0.0",
            archive_sha256: &second_digest,
            registry_index_sha256: &index_digest,
            verification_sha256: &verification_digest,
        }];
        let vendor = temp.path().join("vendor");
        let first_snapshot = store.vendor_snapshot(&vendor, &first).unwrap();
        let second_snapshot = store.vendor_snapshot(&vendor, &second).unwrap();
        let current_before = fs::read(vendor.join("CURRENT")).unwrap();
        assert_eq!(
            current_before,
            format!("{}\n", second_snapshot.digest).as_bytes()
        );
        fs::write(
            first_snapshot
                .root
                .join("packages/sha256")
                .join(&first_digest)
                .join("tree/a"),
            b"corrupt",
        )
        .unwrap();

        assert!(store.vendor_snapshot(&vendor, &first).is_err());
        assert_eq!(fs::read(vendor.join("CURRENT")).unwrap(), current_before);
    }

    #[test]
    fn stale_owned_transactions_are_reaped_but_unowned_entries_are_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let store = PackageStore::open(&temp.path().join("cache")).unwrap();
        let transactions = store.root().join(".transactions");
        let stale_pid = 2_000_000_000u32;
        let stale = transactions.join(format!(".vendor-{stale_pid}-0-7"));
        fs::create_dir(&stale).unwrap();
        write_new_file(
            &stale.join(TRANSACTION_MARKER_NAME),
            &transaction_marker_bytes(stale_pid, 0, 7),
        )
        .unwrap();
        write_new_file(&stale.join("partial"), b"partial").unwrap();
        let unowned = transactions.join(".vendor-1999999999-0-8");
        fs::create_dir(&unowned).unwrap();

        let active = unique_transaction(&transactions, "admit").unwrap();
        assert!(!stale.exists());
        assert!(unowned.exists());
        finish_transaction(Ok(()), &active).unwrap();
    }

    #[test]
    fn stale_transaction_cleanup_streams_past_1024_entries() {
        let temp = tempfile::tempdir().unwrap();
        let store = PackageStore::open(&temp.path().join("cache")).unwrap();
        let transactions = store.root().join(".transactions");
        let stale_pid = 2_000_000_000u32;
        for index in 0..=1_024 {
            let path = transactions.join(format!(".vendor-{stale_pid}-0-{index}"));
            fs::create_dir(&path).unwrap();
            write_new_file(
                &path.join(TRANSACTION_MARKER_NAME),
                &transaction_marker_bytes(stale_pid, 0, index),
            )
            .unwrap();
        }
        reap_stale_transactions(&transactions, unix_epoch_nanos().unwrap()).unwrap();
        assert_eq!(transactions.read_dir().unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn anchored_root_rejects_symlink_ancestor_escape() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let anchor = temp.path().join("project");
        let outside = temp.path().join("outside");
        fs::create_dir(&anchor).unwrap();
        fs::create_dir(&outside).unwrap();
        symlink(&outside, anchor.join("cache-link")).unwrap();

        let error =
            PackageStore::open_anchored(&anchor, Path::new("cache-link/packages")).unwrap_err();
        assert_eq!(error.code, "store_root_invalid");
        assert!(!outside.join("packages").exists());
        assert_eq!(
            PackageStore::prepare_anchored_root(&anchor, Path::new("../escape"))
                .unwrap_err()
                .code,
            "store_root_escape"
        );
    }

    #[cfg(unix)]
    #[test]
    fn bounded_reader_rejects_fifo_without_blocking() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let temp = tempfile::tempdir().unwrap();
        let fifo = temp.path().join("evidence-fifo");
        let fifo_name = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo_name.as_ptr(), 0o600) }, 0);
        assert_eq!(
            read_bounded_regular_file(&fifo, 16).unwrap_err().code,
            "store_file_invalid"
        );
    }

    #[test]
    fn vendor_verification_rejects_tampered_tree_and_manifest_pointer() {
        let temp = tempfile::tempdir().unwrap();
        let store = PackageStore::open(&temp.path().join("cache")).unwrap();
        let bytes = archive(&[("a", b"x")]);
        let digest = sha256_hex(&bytes);
        store.admit(fixture(&bytes, &digest)).unwrap();
        let index_digest = sha256_hex(b"registry-index");
        let verification_digest = sha256_hex(b"verification");
        let expected = [VendorPackage {
            package_id: "registry:demo/core@1.0.0",
            archive_sha256: &digest,
            registry_index_sha256: &index_digest,
            verification_sha256: &verification_digest,
        }];
        let vendor = temp.path().join("vendor");
        let snapshot = store.vendor_snapshot(&vendor, &expected).unwrap();
        fs::write(
            snapshot
                .root
                .join("packages/sha256")
                .join(&digest)
                .join("tree/a"),
            b"bad",
        )
        .unwrap();
        assert!(PackageStore::verify_vendor_snapshot(&vendor, &expected).is_err());
        fs::write(vendor.join("CURRENT"), format!("{}\n", "0".repeat(64))).unwrap();
        assert!(PackageStore::verify_vendor_snapshot(&vendor, &expected).is_err());
    }
}
