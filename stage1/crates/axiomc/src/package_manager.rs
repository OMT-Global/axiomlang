//! High-level package resolution, trusted transport, and materialization bridge.
//!
//! `PackageManager` owns the manifest/lock pairing for one project. Transport
//! is supplied only to operations that are allowed to fetch bytes; locked
//! materialization and vendoring deliberately have no transport handle.

use crate::diagnostics::Diagnostic;
use crate::lockfile::{
    LOCKFILE_V2_VERSION, LockedCompatibilityEvidence, LockedDependencyEdgeV2,
    LockedDependencyReason, LockedDependencySourceKind, LockedPackage, LockedPackageV2,
    LockedRegistryV2, Lockfile, LockfileV2, ParsedLockfile, canonical_path_package_id,
    canonical_registry_package_id, load_lockfile_with_sha256,
    validate_lockfile_version_for_manifest, write_lockfile_v2_atomic_cas,
};
#[cfg(test)]
use crate::lockfile::{load_lockfile, write_lockfile_v2_atomic};
use crate::manifest::{
    DEFAULT_PACKAGE_CACHE_DIR, DEFAULT_PACKAGE_VENDOR_DIR, DependencySpec, Manifest,
    RegistryConfig, load_manifest, lockfile_path, manifest_path,
    normalize_project_relative_materialization_root, parse_manifest_exact,
};
use crate::package_archive::{ArchiveLimits, parse_archive};
use crate::package_resolver::{
    AuthenticatedCatalog as ResolverCatalog, CandidateRejection, CatalogCandidate, Dependency,
    LockedSelection, PackageKey, PathDependency, RegistryDependency, Resolution, ResolutionMode,
    ResolveError, ResolveRequest, ResolverLimits, ResolverSource, SourceFailure, TraceEvent,
    VerifiedCandidate, resolve_packages,
};
use crate::package_store::{
    CachedPackage, PackageStore, StoreError, VendorLifecycleEvidence, VendorPackage,
    VendorSnapshot, VerifiedArtifacts, read_bounded_regular_file,
};
use crate::package_trust::{
    AuthenticatedRegistryCatalog, AuthenticatedRegistryRelease, PackageArtifacts,
    PackageTrustInput, PackageVerification, TrustRootsEnvelope, VerificationExpectation,
    authenticate_registry_catalog, parse_package_signature_json, parse_registry_index_json,
    parse_trust_roots_json, parse_verification_expectation_json,
    verification_expectation_for_authenticated_release, verify_package_with_artifacts,
};
use crate::package_version::{ReleaseVersion, VersionRequirement};
use crate::registry_client::{
    MAX_PACKAGE_ARCHIVE_BODY_BYTES, RegistryClient, RegistryClientError,
    resolve_registry_artifact_url,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

const MAX_TRUST_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;
const MAX_CANDIDATE_DOWNLOAD_BYTES: usize = 256 * 1024 * 1024;

pub const PACKAGE_OPERATION_REPORT_SCHEMA: &str = "axiom.package_operation_report.v1";
pub const MATERIALIZED_PACKAGE_GRAPH_SCHEMA: &str = "axiom.materialized_package_graph.v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageOperation {
    Fetch,
    Update,
    Vendor,
}

#[derive(Clone, Copy, Debug)]
pub struct FetchOptions<'a> {
    pub transport: &'a RegistryClient,
    pub resolver_limits: ResolverLimits,
}

impl<'a> FetchOptions<'a> {
    pub fn new(transport: &'a RegistryClient) -> Self {
        Self {
            transport,
            resolver_limits: ResolverLimits::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct UpdateOptions<'a> {
    pub transport: &'a RegistryClient,
    pub package: Option<String>,
    pub resolver_limits: ResolverLimits,
}

impl<'a> UpdateOptions<'a> {
    pub fn new(transport: &'a RegistryClient) -> Self {
        Self {
            transport,
            package: None,
            resolver_limits: ResolverLimits::default(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct VendorOptions {
    pub out: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MaterializeOptions {
    /// Prefer an intact vendor snapshot over the content-addressed cache.
    pub prefer_vendor: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PackageOperationReport {
    pub schema_version: String,
    pub operation: PackageOperation,
    pub project: String,
    pub lockfile: String,
    pub packages: Vec<MaterializedPackage>,
    pub graph: MaterializedPackageGraph,
    pub trace: Vec<TraceEvent>,
    pub transport_used: bool,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor_lifecycle: Option<VendorLifecycleEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MaterializedPackageGraph {
    pub schema_version: String,
    pub lockfile_sha256: String,
    pub roots: Vec<String>,
    pub packages: Vec<MaterializedPackage>,
    pub edges: Vec<MaterializedDependencyEdge>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MaterializedPackage {
    pub id: String,
    pub name: String,
    pub version: String,
    pub source: String,
    pub root: String,
    #[serde(skip)]
    pub verified_archive: Option<Arc<[u8]>>,
    #[serde(skip)]
    pub verified_manifest: Option<Arc<[u8]>>,
    pub trust: Option<PackageTrustEvidence>,
    pub materialization: MaterializationEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MaterializedDependencyEdge {
    pub from: String,
    pub to: String,
    pub alias: String,
    pub requested: String,
    pub source_kind: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PackageTrustEvidence {
    pub registry: String,
    pub registry_identity: String,
    pub source_identity: String,
    pub publisher_identity: String,
    pub archive_sha256: String,
    pub manifest_sha256: String,
    pub provenance_sha256: String,
    pub package_signature_sha256: String,
    pub signer_key_ids: Vec<String>,
    pub index_sha256: String,
    pub index_generation: u64,
    pub index_sequence: u64,
    pub index_transcript_sha256: String,
    pub verification_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MaterializationEvidence {
    pub source: String,
    pub content_key: Option<String>,
    pub package_trust_verified: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PackageManagerError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trace: Vec<TraceEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolver: Option<serde_json::Value>,
}

impl PackageManagerError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            trace: Vec::new(),
            resolver: None,
        }
    }

    fn from_source(failure: SourceFailure) -> Self {
        Self::new(failure.code, failure.message)
    }
}

impl fmt::Display for PackageManagerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for PackageManagerError {}

#[derive(Clone, Debug)]
pub struct PackageManager {
    project_root: PathBuf,
    manifest: Manifest,
}

#[derive(Clone, Debug)]
struct VerifiedDownload {
    release: AuthenticatedRegistryRelease,
    verification_bytes: Vec<u8>,
    archive: Arc<[u8]>,
    manifest: Arc<[u8]>,
    provenance: Vec<u8>,
    package_signature_bytes: Vec<u8>,
    registry_index_bytes: Arc<[u8]>,
}

struct OnlineResolverSource<'a> {
    transport: &'a RegistryClient,
    operation_deadline: Instant,
    registry: RegistryConfig,
    trust_roots_bytes: Vec<u8>,
    expectation_bytes: Vec<u8>,
    trust_roots: TrustRootsEnvelope,
    expectation: VerificationExpectation,
    catalog: AuthenticatedRegistryCatalog,
    index_bytes: Arc<[u8]>,
    candidate_download_bytes: usize,
    downloads: BTreeMap<(PackageKey, ReleaseVersion, String), VerifiedDownload>,
}

#[derive(Clone, Debug)]
struct LocalPackage {
    id: String,
    source: String,
    root: PathBuf,
    manifest: Manifest,
}

#[derive(Clone, Debug)]
struct LocalGraph {
    roots: Vec<String>,
    packages: Vec<LocalPackage>,
    path_edges: Vec<LockedDependencyEdgeV2>,
    registry_edges: Vec<(String, RegistryDependency)>,
    resolver_dependencies: Vec<Dependency>,
}

#[derive(Clone, Debug)]
struct MaterializedRoot {
    path: PathBuf,
    source: String,
    content_key: Option<String>,
    verified_archive: Option<Arc<[u8]>>,
    verified_manifest: Option<Arc<[u8]>>,
}

#[derive(Clone, Debug)]
struct OfflineTrustContext {
    registry: RegistryConfig,
    trust_roots: TrustRootsEnvelope,
    expectation: VerificationExpectation,
}

struct OnlineResolution<'a> {
    source: OnlineResolverSource<'a>,
    local: LocalGraph,
    resolution: Resolution,
    previous: Option<LockfileV2>,
    frozen: BTreeMap<PackageKey, LockedSelection>,
    mode: ResolutionMode,
    expected_lock_sha256: Option<String>,
}

#[derive(Debug)]
struct PreviousLockState {
    lockfile: Option<LockfileV2>,
    expected_sha256: Option<String>,
}

struct PackageOperationLock {
    path: PathBuf,
    file: std::fs::File,
}

impl PackageOperationLock {
    fn acquire(project_root: &Path) -> Result<Self, PackageManagerError> {
        let path = project_root.join(".axiom-package-operation.lock");
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let mut file = options.open(&path).map_err(|error| {
            PackageManagerError::new(
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    "package_operation_in_progress"
                } else {
                    "package_operation_lock_failed"
                },
                format!(
                    "failed to acquire package operation lock {}: {error}",
                    path.display()
                ),
            )
        })?;
        file.write_all(format!("pid={}\n", std::process::id()).as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|error| {
                let _ = std::fs::remove_file(&path);
                PackageManagerError::new(
                    "package_operation_lock_failed",
                    format!(
                        "failed to persist package operation lock {}: {error}",
                        path.display()
                    ),
                )
            })?;
        Ok(Self { path, file })
    }
}

impl Drop for PackageOperationLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let owned = self.file.metadata();
            let current = std::fs::symlink_metadata(&self.path);
            if owned
                .ok()
                .zip(current.ok())
                .is_some_and(|(owned, current)| {
                    owned.dev() == current.dev() && owned.ino() == current.ino()
                })
            {
                let _ = std::fs::remove_file(&self.path);
            }
        }
        #[cfg(not(unix))]
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

impl PackageManager {
    pub fn open(project_root: &Path) -> Result<Self, PackageManagerError> {
        let project_root = std::fs::canonicalize(project_root).map_err(|error| {
            PackageManagerError::new(
                "project_unavailable",
                format!(
                    "failed to open package project {}: {error}",
                    project_root.display()
                ),
            )
        })?;
        let manifest = load_manifest(&project_root)
            .map_err(|error| PackageManagerError::new("manifest_invalid", error.to_string()))?;
        Ok(Self {
            project_root,
            manifest,
        })
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn fetch(
        &self,
        options: FetchOptions<'_>,
    ) -> Result<PackageOperationReport, PackageManagerError> {
        let _operation_lock = PackageOperationLock::acquire(&self.project_root)?;
        let previous = self.optional_v2_lockfile()?;
        if let Some(report) = self.finish_path_only_operation(
            PackageOperation::Fetch,
            None,
            previous.expected_sha256.as_deref(),
        )? {
            return Ok(report);
        }
        let mode = if previous.lockfile.is_some() {
            ResolutionMode::Locked
        } else {
            ResolutionMode::Fresh
        };
        let resolved = self.resolve_online(
            options.transport,
            options.resolver_limits,
            mode,
            previous.lockfile,
            previous.expected_sha256,
            None,
        )?;
        self.finish_online(PackageOperation::Fetch, resolved, true)
    }

    pub fn update(
        &self,
        options: UpdateOptions<'_>,
    ) -> Result<PackageOperationReport, PackageManagerError> {
        let _operation_lock = PackageOperationLock::acquire(&self.project_root)?;
        let previous = self.optional_v2_lockfile()?;
        if let Some(report) = self.finish_path_only_operation(
            PackageOperation::Update,
            options.package.as_deref(),
            previous.expected_sha256.as_deref(),
        )? {
            return Ok(report);
        }
        let resolved = self.resolve_online(
            options.transport,
            options.resolver_limits,
            ResolutionMode::Update,
            previous.lockfile,
            previous.expected_sha256,
            options.package.as_deref(),
        )?;
        self.finish_online(PackageOperation::Update, resolved, true)
    }

    fn finish_path_only_operation(
        &self,
        operation: PackageOperation,
        update_target: Option<&str>,
        expected_lock_sha256: Option<&str>,
    ) -> Result<Option<PackageOperationReport>, PackageManagerError> {
        let fallback = RegistryConfig {
            name: "path-only".to_owned(),
            index: "file:///path-only/index.json".to_owned(),
            trust_roots: "unused".to_owned(),
            expectation: "unused".to_owned(),
            cache: None,
            vendor: None,
        };
        let registry = self.manifest.registry.as_ref().unwrap_or(&fallback);
        let local = collect_local_graph(&self.project_root, &self.manifest, registry, "path-only")?;
        if !local.registry_edges.is_empty() {
            return Ok(None);
        }
        if let Some(target) = update_target {
            return Err(PackageManagerError::new(
                "update_target_invalid",
                format!("targeted update {target:?} requires a direct registry dependency"),
            ));
        }
        let lockfile = build_path_only_lockfile(&local)?;
        let roots = local_materialized_roots(&local);
        write_lockfile_v2_atomic_cas(&self.project_root, &lockfile, expected_lock_sha256)
            .map_err(lockfile_write_error)?;
        let (written, lockfile_sha256) = self.required_v2_lockfile()?;
        if written != lockfile {
            return Err(PackageManagerError::new(
                "lockfile_write_mismatch",
                "reloaded path-only lock differs from the exact lock model written",
            ));
        }
        let graph = materialized_graph(&lockfile, &lockfile_sha256, &roots, &lockfile.roots)?;
        Ok(Some(operation_report(
            operation,
            &self.project_root,
            &lockfile_path(&self.project_root),
            graph,
            Vec::new(),
            false,
            None,
        )))
    }

    pub fn vendor(
        &self,
        options: VendorOptions,
    ) -> Result<PackageOperationReport, PackageManagerError> {
        let _operation_lock = PackageOperationLock::acquire(&self.project_root)?;
        let (lockfile, lockfile_sha256) = self.required_v2_lockfile()?;
        let local = verify_locked_local_graph(&self.project_root, &self.manifest, &lockfile)?;
        if !lockfile_has_registry_packages(&lockfile) {
            let roots = local_materialized_roots(&local);
            let graph = materialized_graph(&lockfile, &lockfile_sha256, &roots, &lockfile.roots)?;
            return Ok(operation_report(
                PackageOperation::Vendor,
                &self.project_root,
                &lockfile_path(&self.project_root),
                graph,
                Vec::new(),
                false,
                None,
            ));
        }
        let trust = offline_trust_context(&self.project_root, &self.manifest, &lockfile)?;
        let store = self.package_store()?;
        let mut roots = local_materialized_roots(&local);
        let mut retained_verified_bytes = 0usize;
        for package in lockfile
            .package
            .iter()
            .filter(|package| package.registry.is_some())
        {
            let registry = locked_registry_for_package(&lockfile, package)?;
            let archive_sha256 = locked_archive_sha256(package)?;
            let materialized = store
                .load_verified_exact(
                    archive_sha256,
                    &registry.index_sha256,
                    locked_verification_sha256(package)?,
                )
                .map_err(store_error)?;
            verify_cached_against_lock(&materialized, package, registry)?;
            verify_offline_package_trust(&materialized, package, registry, &lockfile, &trust)?;
        }
        let vendor_packages = locked_vendor_packages(&lockfile)?;
        let vendor_root = self.vendor_root(options.out.as_deref())?;
        let snapshot = store
            .vendor_snapshot(&vendor_root, &vendor_packages)
            .map_err(store_error)?;
        let _snapshot_lease = PackageStore::lease_vendor_snapshot(&vendor_root, &snapshot)
            .map_err(store_error)?;
        let vendor_lifecycle = snapshot.lifecycle.clone();
        verify_vendor_against_lock(&snapshot, &lockfile)?;
        for package in lockfile
            .package
            .iter()
            .filter(|package| package.registry.is_some())
        {
            let registry = locked_registry_for_package(&lockfile, package)?;
            let materialized = snapshot
                .package(&package.id)
                .map_err(store_error)?
                .ok_or_else(|| {
                    PackageManagerError::new(
                        "vendor_lock_mismatch",
                        format!("verified vendor snapshot lacks {}", package.id),
                    )
                })?;
            verify_cached_against_lock(&materialized, package, registry)?;
            verify_offline_package_trust(&materialized, package, registry, &lockfile, &trust)?;
            let tree = snapshot.package_tree(&package.id).ok_or_else(|| {
                PackageManagerError::new(
                    "vendor_lock_mismatch",
                    format!("verified vendor snapshot lacks a tree for {}", package.id),
                )
            })?;
            let (verified_archive, verified_manifest) = retain_verified_materialization_bytes(
                &mut retained_verified_bytes,
                materialized.artifacts.archive,
                materialized.artifacts.manifest,
            )?;
            roots.insert(
                package.id.clone(),
                MaterializedRoot {
                    path: tree,
                    source: "vendor".to_owned(),
                    content_key: package.cache_key.clone(),
                    verified_archive: Some(verified_archive),
                    verified_manifest: Some(verified_manifest),
                },
            );
        }
        let graph = materialized_graph(&lockfile, &lockfile_sha256, &roots, &lockfile.roots)?;
        Ok(operation_report(
            PackageOperation::Vendor,
            &self.project_root,
            &lockfile_path(&self.project_root),
            graph,
            Vec::new(),
            false,
            Some(vendor_lifecycle),
        ))
    }

    pub fn materialize_locked(
        &self,
        options: MaterializeOptions,
    ) -> Result<MaterializedPackageGraph, PackageManagerError> {
        let (lockfile, lockfile_sha256) = self.required_v2_lockfile()?;
        let local = verify_locked_local_graph(&self.project_root, &self.manifest, &lockfile)?;
        if !lockfile_has_registry_packages(&lockfile) {
            let roots = local_materialized_roots(&local);
            return materialized_graph(&lockfile, &lockfile_sha256, &roots, &lockfile.roots);
        }
        let trust = offline_trust_context(&self.project_root, &self.manifest, &lockfile)?;
        let store = self.package_store()?;
        let mut roots = local_materialized_roots(&local);
        let mut retained_verified_bytes = 0usize;
        let vendor_packages = locked_vendor_packages(&lockfile)?;
        let vendor_root = self.vendor_root(None)?;
        let use_vendor = options.prefer_vendor || vendor_snapshot_is_present(&vendor_root)?;
        if use_vendor {
            let snapshot =
                PackageStore::verify_vendor_snapshot_exact(&vendor_root, &vendor_packages)
                    .map_err(store_error)?;
            let _snapshot_lease = PackageStore::lease_vendor_snapshot(&vendor_root, &snapshot)
                .map_err(store_error)?;
            verify_vendor_against_lock(&snapshot, &lockfile)?;
            for package in lockfile
                .package
                .iter()
                .filter(|package| package.registry.is_some())
            {
                let registry = locked_registry_for_package(&lockfile, package)?;
                let materialized = snapshot
                    .package(&package.id)
                    .map_err(store_error)?
                    .ok_or_else(|| {
                        PackageManagerError::new(
                            "vendor_lock_mismatch",
                            format!("verified vendor snapshot lacks {}", package.id),
                        )
                    })?;
                verify_cached_against_lock(&materialized, package, registry)?;
                verify_offline_package_trust(&materialized, package, registry, &lockfile, &trust)?;
                let tree = snapshot.package_tree(&package.id).ok_or_else(|| {
                    PackageManagerError::new(
                        "vendor_lock_mismatch",
                        format!("verified vendor snapshot lacks a tree for {}", package.id),
                    )
                })?;
                let (verified_archive, verified_manifest) = retain_verified_materialization_bytes(
                    &mut retained_verified_bytes,
                    materialized.artifacts.archive,
                    materialized.artifacts.manifest,
                )?;
                roots.insert(
                    package.id.clone(),
                    MaterializedRoot {
                        path: tree,
                        source: "vendor".to_owned(),
                        content_key: package.cache_key.clone(),
                        verified_archive: Some(verified_archive),
                        verified_manifest: Some(verified_manifest),
                    },
                );
            }
        } else {
            for package in lockfile
                .package
                .iter()
                .filter(|package| package.registry.is_some())
            {
                let registry = locked_registry_for_package(&lockfile, package)?;
                let materialized = store
                    .load_verified_exact(
                        locked_archive_sha256(package)?,
                        &registry.index_sha256,
                        locked_verification_sha256(package)?,
                    )
                    .map_err(store_error)?;
                verify_cached_against_lock(&materialized, package, registry)?;
                verify_offline_package_trust(&materialized, package, registry, &lockfile, &trust)?;
                let (verified_archive, verified_manifest) = retain_verified_materialization_bytes(
                    &mut retained_verified_bytes,
                    materialized.artifacts.archive,
                    materialized.artifacts.manifest,
                )?;
                roots.insert(
                    package.id.clone(),
                    MaterializedRoot {
                        path: materialized.tree,
                        source: "cache".to_owned(),
                        content_key: package.cache_key.clone(),
                        verified_archive: Some(verified_archive),
                        verified_manifest: Some(verified_manifest),
                    },
                );
            }
        }
        materialized_graph(&lockfile, &lockfile_sha256, &roots, &lockfile.roots)
    }

    fn optional_v2_lockfile(&self) -> Result<PreviousLockState, PackageManagerError> {
        let path = lockfile_path(&self.project_root);
        match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(PreviousLockState {
                    lockfile: None,
                    expected_sha256: None,
                });
            }
            Err(error) => {
                return Err(PackageManagerError::new(
                    "lockfile_invalid",
                    format!("failed to inspect {}: {error}", path.display()),
                ));
            }
            Ok(_) => {}
        }
        let (parsed, expected_sha256) = load_lockfile_with_sha256(&self.project_root)
            .map_err(|error| PackageManagerError::new("lockfile_invalid", error.to_string()))?;
        match parsed {
            // Fetch/update are the explicit migration boundary from a valid
            // path-only v1 lock to a registry-capable v2 lock. The v2 file is
            // written only after resolution, trust verification, and cache
            // admission all succeed.
            ParsedLockfile::V1(lockfile) => {
                self.validate_v1_migration_lockfile(&lockfile)?;
                Ok(PreviousLockState {
                    lockfile: None,
                    expected_sha256: Some(expected_sha256),
                })
            }
            ParsedLockfile::V2(lockfile) => {
                validate_lockfile_version_for_manifest(
                    &self.manifest,
                    &ParsedLockfile::V2(lockfile.clone()),
                )
                .map_err(|error| PackageManagerError::new("lockfile_invalid", error.to_string()))?;
                Ok(PreviousLockState {
                    lockfile: Some(lockfile),
                    expected_sha256: Some(expected_sha256),
                })
            }
        }
    }

    fn required_v2_lockfile(&self) -> Result<(LockfileV2, String), PackageManagerError> {
        let (parsed, sha256) = load_lockfile_with_sha256(&self.project_root)
            .map_err(|error| PackageManagerError::new("lockfile_unavailable", error.to_string()))?;
        validate_lockfile_version_for_manifest(&self.manifest, &parsed)
            .map_err(|error| PackageManagerError::new("lockfile_invalid", error.to_string()))?;
        match parsed {
            ParsedLockfile::V2(lockfile) => Ok((lockfile, sha256)),
            ParsedLockfile::V1(_) => Err(PackageManagerError::new(
                "lockfile_v2_required",
                "locked package materialization requires axiom.lock version 2",
            )),
        }
    }

    fn validate_v1_migration_lockfile(
        &self,
        lockfile: &Lockfile,
    ) -> Result<(), PackageManagerError> {
        let fallback = RegistryConfig {
            name: "path-only".to_owned(),
            index: "file:///path-only/index.json".to_owned(),
            trust_roots: "unused".to_owned(),
            expectation: "unused".to_owned(),
            cache: None,
            vendor: None,
        };
        let registry = self.manifest.registry.as_ref().unwrap_or(&fallback);
        let local =
            collect_local_graph(&self.project_root, &self.manifest, registry, "v1-migration")?;
        let expected = local
            .packages
            .iter()
            .map(|package| {
                let section = package.manifest.package.as_ref().ok_or_else(|| {
                    PackageManagerError::new(
                        "path_package_missing",
                        format!(
                            "path package {} has no [package] section",
                            package.root.display()
                        ),
                    )
                })?;
                Ok(LockedPackage {
                    name: section.name.clone(),
                    version: section.version.clone(),
                    source: package.source.clone(),
                })
            })
            .collect::<Result<Vec<_>, PackageManagerError>>()?;
        if lockfile.package != expected {
            return Err(PackageManagerError::new(
                "stale_v1_lockfile",
                "axiom.lock v1 does not match the current path package graph; repair it before registry migration",
            ));
        }
        Ok(())
    }

    fn resolve_online<'a>(
        &self,
        transport: &'a RegistryClient,
        limits: ResolverLimits,
        mode: ResolutionMode,
        previous: Option<LockfileV2>,
        expected_lock_sha256: Option<String>,
        target: Option<&str>,
    ) -> Result<OnlineResolution<'a>, PackageManagerError> {
        let operation_deadline = transport.operation_deadline().map_err(transport_error)?;
        let mut source = self.online_source(
            transport,
            operation_deadline,
            (mode != ResolutionMode::Fresh)
                .then_some(previous.as_ref())
                .flatten(),
        )?;
        if mode == ResolutionMode::Update
            && let Some(lockfile) = previous.as_ref()
        {
            validate_update_registry_continuity(lockfile, &source)?;
        }
        if mode == ResolutionMode::Locked {
            let lockfile = previous.as_ref().ok_or_else(|| {
                PackageManagerError::new(
                    "lockfile_v2_required",
                    "locked fetch requires axiom.lock version 2",
                )
            })?;
            validate_locked_registry(lockfile, &source)?;
        }
        let local = collect_local_graph(
            &self.project_root,
            &self.manifest,
            &source.registry,
            source.catalog.source_identity(),
        )?;
        let locked = previous
            .as_ref()
            .map(|lockfile| locked_selections(lockfile, source.catalog.source_identity()))
            .transpose()?
            .unwrap_or_default();
        let mut frozen = BTreeMap::new();
        let target_key = if let Some(target) = target {
            let matches = local
                .registry_edges
                .iter()
                .filter(|(_, dependency)| {
                    dependency.alias == target || dependency.package.name == target
                })
                .map(|(_, dependency)| dependency.package.clone())
                .collect::<BTreeSet<_>>();
            if matches.len() != 1 {
                return Err(PackageManagerError::new(
                    "update_target_invalid",
                    format!(
                        "targeted update {target:?} must identify exactly one direct registry dependency"
                    ),
                ));
            }
            matches.into_iter().next()
        } else {
            None
        };
        if mode == ResolutionMode::Locked {
            frozen = locked.clone();
        } else if let Some(target_key) = &target_key {
            frozen.extend(
                locked
                    .iter()
                    .filter(|(key, _)| *key != target_key)
                    .map(|(key, selection)| (key.clone(), selection.clone())),
            );
        }
        let request = ResolveRequest {
            dependencies: local.resolver_dependencies.clone(),
            locked: locked.clone(),
            frozen: frozen.clone(),
            mode,
            limits,
        };
        let resolution = resolve_packages(&mut source, request)
            .map_err(|error| map_resolve_error(error, !frozen.is_empty()))?;
        validate_frozen_downloads(previous.as_ref(), &frozen, &resolution, &source)?;
        retain_selected_downloads(&mut source, &resolution);
        if let Some(target_key) = target_key
            && let Some(previous) = locked.get(&target_key)
            && resolution
                .packages
                .iter()
                .any(|package| package.package == target_key && package.version == previous.version)
            && resolution.trace.iter().any(trace_is_frozen_mismatch)
        {
            return Err(PackageManagerError::new(
                "broader_update_required",
                format!(
                    "updating {} requires changing another locked package; run an untargeted update",
                    target_key
                ),
            ));
        }
        let resolved = OnlineResolution {
            source,
            local,
            resolution,
            previous,
            frozen,
            mode,
            expected_lock_sha256,
        };
        if resolved.mode == ResolutionMode::Locked {
            let previous = resolved.previous.as_ref().ok_or_else(|| {
                PackageManagerError::new(
                    "lockfile_v2_required",
                    "locked resolution completed without an axiom.lock v2 input",
                )
            })?;
            if !same_dependency_edge_set(&resolved_dependency_edges(&resolved)?, &previous.edge) {
                return Err(PackageManagerError::new(
                    "locked_dependency_mismatch",
                    "resolved dependency edge set differs from exact axiom.lock pins",
                ));
            }
        }
        Ok(resolved)
    }

    fn finish_online(
        &self,
        operation: PackageOperation,
        resolved: OnlineResolution<'_>,
        transport_used: bool,
    ) -> Result<PackageOperationReport, PackageManagerError> {
        let lockfile = build_resolved_lockfile(&resolved)?;
        let store = self.package_store()?;
        let mut roots = local_materialized_roots(&resolved.local);
        let mut retained_verified_bytes = 0usize;
        for selected in &resolved.resolution.packages {
            let download = selected_download(&resolved.source, selected)?;
            let package_id = canonical_registry_package_id(
                &selected.package.registry,
                &selected.package.namespace,
                &selected.package.name,
                &selected.version.to_string(),
            );
            let locked = lockfile
                .package
                .iter()
                .find(|package| package.id == package_id)
                .ok_or_else(|| {
                    PackageManagerError::new(
                        "resolution_incomplete",
                        format!("selected package {package_id} is absent from the new lockfile"),
                    )
                })?;
            let registry = locked_registry_for_package(&lockfile, locked)?;
            if sha256_hex(&download.verification_bytes) != locked_verification_sha256(locked)? {
                return Err(PackageManagerError::new(
                    "locked_package_mismatch",
                    format!(
                        "Package Trust verification evidence differs from exact lock pins for {}",
                        locked.id
                    ),
                ));
            }
            let cached = store
                .admit(VerifiedArtifacts {
                    archive_sha256: download.release.archive_sha256(),
                    archive: &download.archive,
                    manifest: &download.manifest,
                    provenance: &download.provenance,
                    signature: &download.package_signature_bytes,
                    registry_index: &download.registry_index_bytes,
                    verification: &download.verification_bytes,
                })
                .map_err(store_error)?;
            verify_cached_against_lock(&cached, locked, registry)?;
            let (verified_archive, verified_manifest) = retain_verified_materialization_bytes(
                &mut retained_verified_bytes,
                cached.artifacts.archive,
                cached.artifacts.manifest,
            )?;
            roots.insert(
                locked.id.clone(),
                MaterializedRoot {
                    path: cached.tree,
                    source: "cache".to_owned(),
                    content_key: locked.cache_key.clone(),
                    verified_archive: Some(verified_archive),
                    verified_manifest: Some(verified_manifest),
                },
            );
        }
        write_lockfile_v2_atomic_cas(
            &self.project_root,
            &lockfile,
            resolved.expected_lock_sha256.as_deref(),
        )
        .map_err(lockfile_write_error)?;
        let (written, lockfile_sha256) = self.required_v2_lockfile()?;
        if written != lockfile {
            return Err(PackageManagerError::new(
                "lockfile_write_mismatch",
                "reloaded resolved lock differs from the exact lock model written",
            ));
        }
        let graph = materialized_graph(&lockfile, &lockfile_sha256, &roots, &lockfile.roots)?;
        Ok(operation_report(
            operation,
            &self.project_root,
            &lockfile_path(&self.project_root),
            graph,
            resolved.resolution.trace.clone(),
            transport_used,
            None,
        ))
    }

    fn package_store(&self) -> Result<PackageStore, PackageManagerError> {
        let configured = self
            .manifest
            .registry
            .as_ref()
            .and_then(|registry| registry.cache.as_deref())
            .unwrap_or(DEFAULT_PACKAGE_CACHE_DIR);
        self.validate_storage_path_collision(Path::new(configured), None)?;
        PackageStore::open_anchored(&self.project_root, Path::new(configured)).map_err(store_error)
    }

    fn vendor_root(&self, out: Option<&Path>) -> Result<PathBuf, PackageManagerError> {
        let configured = out
            .map(Path::to_path_buf)
            .or_else(|| {
                self.manifest
                    .registry
                    .as_ref()
                    .and_then(|registry| registry.vendor.as_deref())
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| PathBuf::from(DEFAULT_PACKAGE_VENDOR_DIR));
        let configured = if configured.is_absolute() {
            configured
        } else {
            let value = configured.to_str().ok_or_else(|| {
                PackageManagerError::new("vendor_root_invalid", "vendor root must be valid UTF-8")
            })?;
            PathBuf::from(
                normalize_project_relative_materialization_root(
                    &manifest_path(&self.project_root),
                    "pkg vendor --out",
                    value,
                )
                .map_err(|error| {
                    PackageManagerError::new("vendor_root_invalid", error.to_string())
                })?,
            )
        };
        let cache = Path::new(
            self.manifest
                .registry
                .as_ref()
                .and_then(|registry| registry.cache.as_deref())
                .unwrap_or(DEFAULT_PACKAGE_CACHE_DIR),
        );
        self.validate_storage_path_collision(cache, Some(&configured))?;
        if configured.is_absolute() {
            let parent = configured.parent().ok_or_else(|| {
                PackageManagerError::new(
                    "vendor_root_invalid",
                    "absolute vendor override has no parent directory",
                )
            })?;
            let parent = std::fs::canonicalize(parent).map_err(|error| {
                PackageManagerError::new(
                    "vendor_root_invalid",
                    format!(
                        "failed to canonicalize vendor override parent {}: {error}",
                        parent.display()
                    ),
                )
            })?;
            let name = configured.file_name().ok_or_else(|| {
                PackageManagerError::new(
                    "vendor_root_invalid",
                    "absolute vendor override must name a directory below its parent",
                )
            })?;
            PackageStore::prepare_anchored_root(&parent, Path::new(name)).map_err(store_error)
        } else {
            PackageStore::prepare_anchored_root(&self.project_root, &configured)
                .map_err(store_error)
        }
    }

    fn validate_storage_path_collision(
        &self,
        cache: &Path,
        vendor_override: Option<&Path>,
    ) -> Result<(), PackageManagerError> {
        let vendor = vendor_override
            .map(Path::to_path_buf)
            .or_else(|| {
                self.manifest
                    .registry
                    .as_ref()
                    .and_then(|registry| registry.vendor.as_deref())
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| PathBuf::from(DEFAULT_PACKAGE_VENDOR_DIR));
        let effective = |configured: &Path| {
            if configured.is_absolute() {
                normalize_lexical_path(configured)
            } else {
                normalize_lexical_path(&self.project_root.join(configured))
            }
        };
        if effective(cache) == effective(&vendor) {
            return Err(PackageManagerError::new(
                "package_storage_path_collision",
                format!(
                    "effective package cache and vendor roots both resolve to {}",
                    effective(cache).display()
                ),
            ));
        }
        Ok(())
    }

    fn online_source<'a>(
        &self,
        transport: &'a RegistryClient,
        operation_deadline: Instant,
        previous: Option<&LockfileV2>,
    ) -> Result<OnlineResolverSource<'a>, PackageManagerError> {
        let registry = self.manifest.registry.clone().ok_or_else(|| {
            PackageManagerError::new(
                "registry_missing",
                "registry dependencies require a root [registry] configuration",
            )
        })?;
        let trust_roots_path = self.project_root.join(&registry.trust_roots);
        let expectation_path = self.project_root.join(&registry.expectation);
        let trust_roots_bytes = read_bounded_file(&trust_roots_path, MAX_TRUST_DOCUMENT_BYTES)
            .map_err(|message| PackageManagerError::new("trust_roots_unavailable", message))?;
        let expectation_bytes = read_bounded_file(&expectation_path, MAX_TRUST_DOCUMENT_BYTES)
            .map_err(|message| PackageManagerError::new("expectation_unavailable", message))?;
        if let Some(previous) = previous {
            validate_previous_trust_documents(
                previous,
                &registry,
                &trust_roots_bytes,
                &expectation_bytes,
            )?;
        }
        let trust_roots = parse_trust_roots_json(&trust_roots_bytes)
            .map_err(|error| PackageManagerError::new("trust_roots_invalid", error.to_string()))?;
        let mut expectation = parse_verification_expectation_json(&expectation_bytes)
            .map_err(|error| PackageManagerError::new("expectation_invalid", error.to_string()))?;
        let index_bytes = transport
            .fetch_until(&registry.index, operation_deadline)
            .map_err(transport_error)?;
        if let Some(previous) = previous {
            advance_expectation_from_lock(&mut expectation, previous, &registry)?;
        }
        let catalog = authenticate_registry_catalog(&index_bytes, &trust_roots, &expectation)
            .map_err(|error| {
                PackageManagerError::new("registry_catalog_rejected", error.reason_codes.join(", "))
            })?;
        let index_bytes: Arc<[u8]> = Arc::from(index_bytes);
        Ok(OnlineResolverSource {
            transport,
            operation_deadline,
            registry,
            trust_roots_bytes,
            expectation_bytes,
            trust_roots,
            expectation,
            catalog,
            index_bytes,
            candidate_download_bytes: 0,
            downloads: BTreeMap::new(),
        })
    }
}

impl ResolverSource for OnlineResolverSource<'_> {
    fn authenticate_catalog(
        &mut self,
        package: &PackageKey,
    ) -> Result<ResolverCatalog, SourceFailure> {
        self.require_catalog_identity(package)?;
        let mut candidates = Vec::new();
        for release in self.catalog.releases().iter().filter(|release| {
            release.namespace() == package.namespace && release.name() == package.name
        }) {
            let version = ReleaseVersion::parse(release.version()).map_err(|error| {
                SourceFailure::new(
                    "catalog_version_invalid",
                    format!(
                        "authenticated release {}/{} has unsupported version: {error}",
                        release.namespace(),
                        release.name()
                    ),
                )
            })?;
            candidates.push(CatalogCandidate {
                version,
                yanked: release.yanked(),
                release_id: release.target_path().to_owned(),
            });
        }
        Ok(ResolverCatalog::new(
            package.clone(),
            candidates,
            sha256_hex(self.catalog.exact_index_bytes()),
        ))
    }

    fn verify_candidate(
        &mut self,
        catalog: &ResolverCatalog,
        candidate: &CatalogCandidate,
    ) -> Result<VerifiedCandidate, SourceFailure> {
        self.require_catalog_identity(catalog.package())?;
        let version = candidate.version.to_string();
        let release = self
            .catalog
            .release(
                &catalog.package().namespace,
                &catalog.package().name,
                &version,
            )
            .filter(|release| release.target_path() == candidate.release_id)
            .cloned()
            .ok_or_else(|| {
                SourceFailure::new(
                    "release_identity_mismatch",
                    format!(
                        "authenticated catalog does not contain {} at release identity {:?}",
                        catalog.package(),
                        candidate.release_id
                    ),
                )
            })?;
        let paths = release.artifact_paths();
        let archive = self.fetch_package_archive(&paths.archive, release.archive_length())?;
        let manifest_bytes = self.fetch_artifact(&paths.manifest)?;
        let provenance = self.fetch_artifact(&paths.provenance)?;
        let package_signature_bytes = self.fetch_artifact(&paths.package_signature)?;
        let package_signature = parse_package_signature_json(&package_signature_bytes)
            .map_err(|error| SourceFailure::new("package_signature_invalid", error.to_string()))?;
        let release_expectation = verification_expectation_for_authenticated_release(
            &self.expectation,
            &self.catalog,
            &release,
        )
        .map_err(|error| {
            SourceFailure::new("release_expectation_invalid", error.reason_codes.join(", "))
        })?;
        let registry_index = parse_registry_index_json(self.catalog.exact_index_bytes())
            .map_err(|error| SourceFailure::new("registry_index_invalid", error.to_string()))?;
        let verification = verify_package_with_artifacts(
            &PackageTrustInput {
                package_signature: package_signature.clone(),
                trust_roots: self.trust_roots.clone(),
                registry_index,
                verification_expectation: release_expectation,
            },
            PackageArtifacts {
                archive: Some(&archive),
                manifest: Some(&manifest_bytes),
                provenance: Some(&provenance),
            },
        );
        if verification.decision != "trusted" {
            return Err(SourceFailure::new(
                "package_trust_rejected",
                format!(
                    "{} rejected: {}",
                    catalog.package(),
                    verification.reason_codes.join(", ")
                ),
            ));
        }
        let manifest = parse_manifest_exact(&manifest_bytes, Path::new("authenticated/axiom.toml"))
            .map_err(|error| SourceFailure::new("package_manifest_invalid", error.to_string()))?;
        validate_authenticated_archive_contract(
            &archive,
            release.archive_sha256(),
            &manifest_bytes,
            &manifest,
        )?;
        let package_section = manifest.package.as_ref().ok_or_else(|| {
            SourceFailure::new(
                "package_identity_mismatch",
                "authenticated registry manifest has no [package] section",
            )
        })?;
        if package_section.name != catalog.package().name || package_section.version != version {
            return Err(SourceFailure::new(
                "package_identity_mismatch",
                format!(
                    "authenticated manifest identifies {}@{}, expected {}@{}",
                    package_section.name,
                    package_section.version,
                    catalog.package().name,
                    version
                ),
            ));
        }
        let dependencies =
            manifest_dependencies(&manifest, &self.registry, self.catalog.source_identity())?;
        let mut signer_key_ids = verification
            .signers
            .iter()
            .map(|signer| signer.key_id.clone())
            .collect::<Vec<_>>();
        signer_key_ids.sort();
        signer_key_ids.dedup();
        let compatibility = current_compatibility()
            .map_err(|error| SourceFailure::new(error.code, error.message))?;
        let verified = VerifiedCandidate {
            package: catalog.package().clone(),
            version: candidate.version,
            release_id: candidate.release_id.clone(),
            dependencies,
            manifest_digest: release.manifest_sha256().to_owned(),
            signer_key_ids,
            edition: compatibility.edition_policy.clone(),
            compatibility: compatibility.contract.clone(),
        };
        let verification_bytes = serde_json::to_vec(&verification).map_err(|error| {
            SourceFailure::new(
                "package_verification_invalid",
                format!("failed to serialize Package Trust evidence: {error}"),
            )
        })?;
        self.downloads.insert(
            (
                verified.package.clone(),
                verified.version,
                verified.release_id.clone(),
            ),
            VerifiedDownload {
                release,
                verification_bytes,
                archive: Arc::from(archive),
                manifest: Arc::from(manifest_bytes),
                provenance,
                package_signature_bytes,
                registry_index_bytes: self.index_bytes.clone(),
            },
        );
        Ok(verified)
    }

    fn discard_candidate(&mut self, candidate: &CatalogCandidate) {
        self.downloads.retain(|(_, version, release_id), _| {
            *version != candidate.version || release_id != &candidate.release_id
        });
    }
}

fn validate_authenticated_archive_contract(
    archive_bytes: &[u8],
    expected_sha256: &str,
    authenticated_manifest_bytes: &[u8],
    manifest: &Manifest,
) -> Result<(), SourceFailure> {
    let archive = parse_archive(archive_bytes, expected_sha256, ArchiveLimits::default())
        .map_err(|error| SourceFailure::new(error.code, error.message))?;
    let embedded_manifest = archive
        .entries
        .iter()
        .find(|entry| entry.path == "axiom.toml")
        .ok_or_else(|| {
            SourceFailure::new(
                "archive_manifest_missing",
                "authenticated package archive does not contain axiom.toml",
            )
        })?;
    if embedded_manifest.bytes != authenticated_manifest_bytes {
        return Err(SourceFailure::new(
            "archive_manifest_mismatch",
            "archive axiom.toml differs from the separately authenticated manifest bytes",
        ));
    }
    if manifest.workspace.is_some() {
        return Err(SourceFailure::new(
            "registry_workspace_unsupported",
            "registry packages may not contain a [workspace] manifest",
        ));
    }
    let entry = manifest.build.entry.as_str();
    if !entry.ends_with(".ax") {
        return Err(SourceFailure::new(
            "registry_build_entry_invalid",
            format!("registry package build.entry {entry:?} must name an .ax file"),
        ));
    }
    if !archive
        .entries
        .iter()
        .any(|archive_entry| archive_entry.path == entry)
    {
        return Err(SourceFailure::new(
            "registry_build_entry_missing",
            format!(
                "registry package build.entry {entry:?} is absent from the authenticated archive"
            ),
        ));
    }
    Ok(())
}

impl OnlineResolverSource<'_> {
    fn require_catalog_identity(&self, package: &PackageKey) -> Result<(), SourceFailure> {
        if package.registry != self.registry.name
            || package.source != self.catalog.source_identity()
        {
            return Err(SourceFailure::new(
                "registry_identity_mismatch",
                format!(
                    "package source {} does not match authenticated registry {}@{}",
                    package,
                    self.registry.name,
                    self.catalog.source_identity()
                ),
            ));
        }
        Ok(())
    }

    fn fetch_artifact(&mut self, relative_path: &str) -> Result<Vec<u8>, SourceFailure> {
        let url = resolve_registry_artifact_url(&self.registry.index, relative_path)
            .map_err(source_transport_error)?;
        let bytes = self
            .transport
            .fetch_until(&url, self.operation_deadline)
            .map_err(source_transport_error)?;
        self.charge_candidate_bytes(bytes.len())?;
        Ok(bytes)
    }

    fn fetch_package_archive(
        &mut self,
        relative_path: &str,
        authenticated_length: u64,
    ) -> Result<Vec<u8>, SourceFailure> {
        let authenticated_length = validate_authenticated_archive_length(authenticated_length)?;
        let url = resolve_registry_artifact_url(&self.registry.index, relative_path)
            .map_err(source_transport_error)?;
        let bytes = self
            .transport
            .fetch_package_archive_until(&url, self.operation_deadline)
            .map_err(source_transport_error)?;
        if bytes.len() != authenticated_length {
            return Err(SourceFailure::new(
                "archive_length_mismatch",
                format!(
                    "downloaded archive has {} bytes, authenticated catalog requires {authenticated_length}",
                    bytes.len()
                ),
            ));
        }
        self.charge_candidate_bytes(bytes.len())?;
        Ok(bytes)
    }

    fn charge_candidate_bytes(&mut self, bytes: usize) -> Result<(), SourceFailure> {
        self.candidate_download_bytes =
            charged_candidate_bytes(self.candidate_download_bytes, bytes)?;
        Ok(())
    }
}

fn validate_authenticated_archive_length(length: u64) -> Result<usize, SourceFailure> {
    if length == 0 || length > MAX_PACKAGE_ARCHIVE_BODY_BYTES as u64 {
        return Err(SourceFailure::new(
            "archive_length_out_of_bounds",
            format!(
                "authenticated archive length {length} exceeds the {} byte package limit",
                MAX_PACKAGE_ARCHIVE_BODY_BYTES
            ),
        ));
    }
    usize::try_from(length).map_err(|_| {
        SourceFailure::new(
            "archive_length_out_of_bounds",
            "authenticated archive length does not fit this platform",
        )
    })
}

fn charged_candidate_bytes(current: usize, bytes: usize) -> Result<usize, SourceFailure> {
    let total = current.checked_add(bytes).ok_or_else(|| {
        SourceFailure::new(
            "candidate_download_budget_exceeded",
            "candidate download byte counter overflowed",
        )
    })?;
    if total > MAX_CANDIDATE_DOWNLOAD_BYTES {
        return Err(SourceFailure::new(
            "candidate_download_budget_exceeded",
            format!(
                "candidate downloads exceeded the {} byte operation budget",
                MAX_CANDIDATE_DOWNLOAD_BYTES
            ),
        ));
    }
    Ok(total)
}

fn retain_verified_materialization_bytes(
    retained: &mut usize,
    archive: Vec<u8>,
    manifest: Vec<u8>,
) -> Result<(Arc<[u8]>, Arc<[u8]>), PackageManagerError> {
    let total = retained
        .checked_add(archive.len())
        .and_then(|total| total.checked_add(manifest.len()))
        .filter(|total| *total <= MAX_CANDIDATE_DOWNLOAD_BYTES)
        .ok_or_else(|| {
            PackageManagerError::new(
                "materialized_verified_bytes_budget_exceeded",
                format!(
                    "verified registry archive and manifest bytes exceed the {} byte materialization budget",
                    MAX_CANDIDATE_DOWNLOAD_BYTES
                ),
            )
        })?;
    *retained = total;
    Ok((Arc::from(archive), Arc::from(manifest)))
}

fn locked_selections(
    lockfile: &LockfileV2,
    source_identity: &str,
) -> Result<BTreeMap<PackageKey, LockedSelection>, PackageManagerError> {
    let mut selections = BTreeMap::new();
    for package in &lockfile.package {
        let (Some(registry), Some(namespace)) =
            (package.registry.as_deref(), package.namespace.as_deref())
        else {
            continue;
        };
        let version = ReleaseVersion::parse(&package.version).map_err(|error| {
            PackageManagerError::new(
                "lockfile_invalid",
                format!("locked package {} has invalid version: {error}", package.id),
            )
        })?;
        let key = PackageKey::new(
            registry.to_owned(),
            source_identity.to_owned(),
            namespace.to_owned(),
            package.name.clone(),
        );
        if selections
            .insert(key, LockedSelection { version })
            .is_some()
        {
            return Err(PackageManagerError::new(
                "lockfile_invalid",
                "lockfile contains duplicate registry package selections",
            ));
        }
    }
    Ok(selections)
}

fn validate_locked_registry(
    lockfile: &LockfileV2,
    source: &OnlineResolverSource<'_>,
) -> Result<(), PackageManagerError> {
    let record = lockfile
        .registry
        .iter()
        .find(|record| record.name == source.registry.name)
        .ok_or_else(|| {
            PackageManagerError::new(
                "locked_registry_missing",
                format!(
                    "axiom.lock has no registry record for {:?}",
                    source.registry.name
                ),
            )
        })?;
    let mut current_signers = source.catalog.index_signer_key_ids().to_vec();
    current_signers.sort();
    let exact = record.source == source.registry.index
        && record.registry_identity == source.catalog.registry_identity()
        && record.source_identity == source.catalog.source_identity()
        && record.trust_roots_sha256 == sha256_hex(&source.trust_roots_bytes)
        && record.expectation_sha256 == sha256_hex(&source.expectation_bytes)
        && record.current_root_version == source.catalog.root_version()
        && record.current_root_sequence == source.catalog.root_sequence()
        && record.current_root_transcript_sha256 == source.catalog.root_transcript_sha256()
        && record.index_sha256 == sha256_hex(source.catalog.exact_index_bytes())
        && record.index_transcript_sha256 == source.catalog.index_transcript_sha256()
        && record.index_generation == source.catalog.generation()
        && record.index_sequence == source.catalog.sequence()
        && record.index_snapshot_id == source.catalog.snapshot_id()
        && record.index_signer_key_ids == current_signers;
    if !exact {
        return Err(PackageManagerError::new(
            "locked_registry_mismatch",
            "authenticated registry metadata does not match the exact axiom.lock v2 pins",
        ));
    }
    Ok(())
}

fn validate_previous_trust_documents(
    lockfile: &LockfileV2,
    configured: &RegistryConfig,
    trust_roots_bytes: &[u8],
    expectation_bytes: &[u8],
) -> Result<(), PackageManagerError> {
    let locked = lockfile
        .registry
        .iter()
        .find(|registry| registry.name == configured.name)
        .ok_or_else(|| {
            PackageManagerError::new(
                "update_registry_missing",
                format!(
                    "previous axiom.lock has no registry state for {:?}",
                    configured.name
                ),
            )
        })?;
    if locked.source != configured.index {
        return Err(PackageManagerError::new(
            "update_registry_source_changed",
            "configured registry source differs from the previously accepted lock state",
        ));
    }
    if sha256_hex(trust_roots_bytes) != locked.trust_roots_sha256 {
        return Err(PackageManagerError::new(
            "trusted_roots_reset_rejected",
            "current trust-roots bytes differ from the exact previously accepted lock pin",
        ));
    }
    if sha256_hex(expectation_bytes) != locked.expectation_sha256 {
        return Err(PackageManagerError::new(
            "verification_policy_reset_rejected",
            "current verification-expectation bytes differ from the exact previously accepted lock pin",
        ));
    }
    Ok(())
}

fn advance_expectation_from_lock(
    expectation: &mut VerificationExpectation,
    lockfile: &LockfileV2,
    configured: &RegistryConfig,
) -> Result<(), PackageManagerError> {
    let locked = lockfile
        .registry
        .iter()
        .find(|registry| registry.name == configured.name)
        .ok_or_else(|| {
            PackageManagerError::new(
                "update_registry_missing",
                format!(
                    "previous axiom.lock has no registry state for {:?}",
                    configured.name
                ),
            )
        })?;
    if locked.source != configured.index {
        return Err(PackageManagerError::new(
            "update_registry_source_changed",
            "configured registry source differs from the previously accepted lock state",
        ));
    }
    let trusted_state = expectation
        .0
        .get_mut("trusted_state")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            PackageManagerError::new(
                "expectation_invalid",
                "verification expectation lacks a trusted_state object",
            )
        })?;
    let advance_number =
        |state: &mut serde_json::Map<String, serde_json::Value>, field: &str, locked_value: u64| {
            let current = state.get(field).and_then(serde_json::Value::as_u64);
            if current.is_none_or(|value| value < locked_value) {
                state.insert(field.to_owned(), serde_json::Value::from(locked_value));
            }
        };
    advance_number(
        trusted_state,
        "highest_root_version",
        locked.current_root_version,
    );
    advance_number(
        trusted_state,
        "highest_root_sequence",
        locked.current_root_sequence,
    );
    advance_number(
        trusted_state,
        "highest_index_generation",
        locked.index_generation,
    );
    advance_number(
        trusted_state,
        "highest_index_sequence",
        locked.index_sequence,
    );

    // `trusted_root_anchor` pins the bootstrap root in the trust-roots
    // transition envelope, not the newest candidate root. Replacing it with
    // the lock's current candidate makes the same authenticated root chain
    // fail ROOT_BOOTSTRAP_MISMATCH on every subsequent online or offline use.
    // The lock advances only the monotonic candidate-root high-water marks.

    let snapshot = serde_json::json!({
        "generation": locked.index_generation,
        "sequence": locked.index_sequence,
        "snapshot_id": locked.index_snapshot_id,
        "index_transcript_sha256": locked.index_transcript_sha256,
    });
    let seen = trusted_state
        .get_mut("seen_snapshots")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| {
            PackageManagerError::new(
                "expectation_invalid",
                "verification expectation trusted_state lacks seen_snapshots",
            )
        })?;
    if let Some(rebound) = seen.iter().find(|candidate| {
        candidate
            .get("generation")
            .and_then(serde_json::Value::as_u64)
            == Some(locked.index_generation)
            && candidate
                .get("sequence")
                .and_then(serde_json::Value::as_u64)
                == Some(locked.index_sequence)
            && *candidate != &snapshot
    }) {
        return Err(PackageManagerError::new(
            "update_snapshot_rebound",
            format!("previous lock conflicts with expectation snapshot {rebound}"),
        ));
    }
    if !seen.contains(&snapshot) {
        seen.push(snapshot);
    }
    Ok(())
}

fn validate_update_registry_continuity(
    lockfile: &LockfileV2,
    source: &OnlineResolverSource<'_>,
) -> Result<(), PackageManagerError> {
    let locked = lockfile
        .registry
        .iter()
        .find(|registry| registry.name == source.registry.name)
        .ok_or_else(|| {
            PackageManagerError::new(
                "update_registry_missing",
                "previous lock has no state for the configured registry",
            )
        })?;
    if locked.registry_identity != source.catalog.registry_identity()
        || locked.source_identity != source.catalog.source_identity()
    {
        return Err(PackageManagerError::new(
            "update_registry_identity_changed",
            "authenticated registry identity differs from the previously accepted lock state",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FrozenPackageIdentity {
    version: String,
    archive_sha256: String,
    archive_length: u64,
    manifest_sha256: String,
    provenance_sha256: String,
    package_signature_sha256: String,
    publisher_identity: String,
    signer_key_ids: Vec<String>,
    yanked: bool,
}

fn locked_frozen_identity(
    package: &LockedPackageV2,
) -> Result<FrozenPackageIdentity, PackageManagerError> {
    Ok(FrozenPackageIdentity {
        version: package.version.clone(),
        archive_sha256: required_locked_field(package, "archive_sha256", &package.archive_sha256)?
            .to_owned(),
        archive_length: package.archive_length.ok_or_else(|| {
            PackageManagerError::new(
                "lockfile_invalid",
                format!("locked package {} lacks archive_length", package.id),
            )
        })?,
        manifest_sha256: required_locked_field(
            package,
            "manifest_sha256",
            &package.manifest_sha256,
        )?
        .to_owned(),
        provenance_sha256: required_locked_field(
            package,
            "provenance_sha256",
            &package.provenance_sha256,
        )?
        .to_owned(),
        package_signature_sha256: required_locked_field(
            package,
            "package_signature_sha256",
            &package.package_signature_sha256,
        )?
        .to_owned(),
        publisher_identity: required_locked_field(
            package,
            "publisher_identity",
            &package.publisher_identity,
        )?
        .to_owned(),
        signer_key_ids: package.signer_key_ids.clone(),
        yanked: package.yanked_at_resolution.ok_or_else(|| {
            PackageManagerError::new(
                "lockfile_invalid",
                format!("locked package {} lacks yank evidence", package.id),
            )
        })?,
    })
}

fn downloaded_frozen_identity(
    selected: &crate::package_resolver::ResolvedPackage,
    download: &VerifiedDownload,
) -> FrozenPackageIdentity {
    FrozenPackageIdentity {
        version: selected.version.to_string(),
        archive_sha256: download.release.archive_sha256().to_owned(),
        archive_length: download.release.archive_length(),
        manifest_sha256: download.release.manifest_sha256().to_owned(),
        provenance_sha256: download.release.provenance_statement_sha256().to_owned(),
        package_signature_sha256: download.release.package_signature_sha256().to_owned(),
        publisher_identity: download.release.publisher_identity().to_owned(),
        signer_key_ids: selected.signer_key_ids.clone(),
        yanked: selected.yanked,
    }
}

fn ensure_frozen_identity(
    package: &PackageKey,
    locked: FrozenPackageIdentity,
    observed: FrozenPackageIdentity,
) -> Result<(), PackageManagerError> {
    if locked != observed {
        return Err(PackageManagerError::new(
            "frozen_package_identity_changed",
            format!(
                "targeted update encountered changed artifact or security identity for frozen package {package}"
            ),
        ));
    }
    Ok(())
}

fn validate_frozen_downloads(
    previous: Option<&LockfileV2>,
    frozen: &BTreeMap<PackageKey, LockedSelection>,
    resolution: &Resolution,
    source: &OnlineResolverSource<'_>,
) -> Result<(), PackageManagerError> {
    if frozen.is_empty() {
        return Ok(());
    }
    let previous = previous.ok_or_else(|| {
        PackageManagerError::new(
            "lockfile_v2_required",
            "frozen package verification requires a previous lockfile",
        )
    })?;
    for key in frozen.keys() {
        let selected = resolution
            .packages
            .iter()
            .find(|selected| &selected.package == key)
            .ok_or_else(|| {
                PackageManagerError::new(
                    "frozen_package_missing",
                    format!("resolver omitted frozen package {key}"),
                )
            })?;
        let locked = previous
            .package
            .iter()
            .find(|package| {
                package.registry.as_deref() == Some(&key.registry)
                    && package.namespace.as_deref() == Some(&key.namespace)
                    && package.name == key.name
            })
            .ok_or_else(|| {
                PackageManagerError::new(
                    "frozen_package_missing",
                    format!("previous lock omitted frozen package {key}"),
                )
            })?;
        let download = selected_download(source, selected)?;
        ensure_frozen_identity(
            key,
            locked_frozen_identity(locked)?,
            downloaded_frozen_identity(selected, download),
        )?;
    }
    Ok(())
}

fn retain_selected_downloads(source: &mut OnlineResolverSource<'_>, resolution: &Resolution) {
    let selected = resolution
        .packages
        .iter()
        .map(|package| {
            (
                package.package.clone(),
                package.version,
                package.release_id.clone(),
            )
        })
        .collect::<BTreeSet<_>>();
    source.downloads.retain(|key, _| selected.contains(key));
}

fn trace_is_frozen_mismatch(event: &TraceEvent) -> bool {
    matches!(
        event,
        TraceEvent::CandidateRejected {
            reason: CandidateRejection::FrozenMismatch { .. },
            ..
        }
    )
}

fn build_path_only_lockfile(local: &LocalGraph) -> Result<LockfileV2, PackageManagerError> {
    if !local.registry_edges.is_empty() {
        return Err(PackageManagerError::new(
            "registry_dependency_unresolved",
            "path-only lock construction received registry dependencies",
        ));
    }
    let compatibility = current_compatibility()?;
    let mut package = local
        .packages
        .iter()
        .map(|local| {
            let section = local.manifest.package.as_ref().ok_or_else(|| {
                PackageManagerError::new(
                    "path_package_missing",
                    format!(
                        "path package {} has no [package] section",
                        local.root.display()
                    ),
                )
            })?;
            Ok(LockedPackageV2 {
                id: local.id.clone(),
                name: section.name.clone(),
                version: section.version.clone(),
                source: local.source.clone(),
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
                compatibility: compatibility.clone(),
            })
        })
        .collect::<Result<Vec<_>, PackageManagerError>>()?;
    package.sort_by(|left, right| left.id.cmp(&right.id));
    let mut roots = local.roots.clone();
    roots.sort();
    roots.dedup();
    let mut edge = local.path_edges.clone();
    edge.sort_by(lock_edge_order);
    let lockfile = LockfileV2 {
        version: LOCKFILE_V2_VERSION,
        compatibility,
        roots,
        registry: Vec::new(),
        package,
        edge,
    };
    crate::lockfile::validate_lockfile_v2(&lockfile)
        .map_err(|error| PackageManagerError::new("lockfile_invalid", error.to_string()))?;
    Ok(lockfile)
}

fn build_resolved_lockfile(
    resolved: &OnlineResolution<'_>,
) -> Result<LockfileV2, PackageManagerError> {
    if resolved
        .resolution
        .path_dependencies
        .iter()
        .any(|dependency| dependency.from.is_some())
    {
        return Err(PackageManagerError::new(
            "registry_path_dependency_unsupported",
            "authenticated registry packages may not depend on mutable path sources",
        ));
    }
    if resolved.mode == ResolutionMode::Locked {
        return resolved.previous.clone().ok_or_else(|| {
            PackageManagerError::new(
                "lockfile_v2_required",
                "locked resolution completed without an axiom.lock v2 input",
            )
        });
    }

    let compatibility = current_compatibility()?;
    let mut index_signer_key_ids = resolved.source.catalog.index_signer_key_ids().to_vec();
    index_signer_key_ids.sort();
    index_signer_key_ids.dedup();
    let registry = vec![LockedRegistryV2 {
        name: resolved.source.registry.name.clone(),
        source: resolved.source.registry.index.clone(),
        registry_identity: resolved.source.catalog.registry_identity().to_owned(),
        source_identity: resolved.source.catalog.source_identity().to_owned(),
        trust_roots_sha256: sha256_hex(&resolved.source.trust_roots_bytes),
        expectation_sha256: sha256_hex(&resolved.source.expectation_bytes),
        current_root_version: resolved.source.catalog.root_version(),
        current_root_sequence: resolved.source.catalog.root_sequence(),
        current_root_transcript_sha256: resolved.source.catalog.root_transcript_sha256().to_owned(),
        index_sha256: sha256_hex(resolved.source.catalog.exact_index_bytes()),
        index_transcript_sha256: resolved.source.catalog.index_transcript_sha256().to_owned(),
        index_generation: resolved.source.catalog.generation(),
        index_sequence: resolved.source.catalog.sequence(),
        index_snapshot_id: resolved.source.catalog.snapshot_id().to_owned(),
        index_signer_key_ids,
    }];

    let mut package =
        Vec::with_capacity(resolved.local.packages.len() + resolved.resolution.packages.len());
    for local in &resolved.local.packages {
        let section = local.manifest.package.as_ref().ok_or_else(|| {
            PackageManagerError::new(
                "path_package_missing",
                format!(
                    "path package {} has no [package] section",
                    local.root.display()
                ),
            )
        })?;
        package.push(LockedPackageV2 {
            id: local.id.clone(),
            name: section.name.clone(),
            version: section.version.clone(),
            source: local.source.clone(),
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
            compatibility: compatibility.clone(),
        });
    }
    for selected in &resolved.resolution.packages {
        let download = selected_download(&resolved.source, selected)?;
        let mut signer_key_ids = selected.signer_key_ids.clone();
        signer_key_ids.sort();
        signer_key_ids.dedup();
        let version = selected.version.to_string();
        let archive_sha256 = download.release.archive_sha256().to_owned();
        package.push(LockedPackageV2 {
            id: canonical_registry_package_id(
                &selected.package.registry,
                &selected.package.namespace,
                &selected.package.name,
                &version,
            ),
            name: selected.package.name.clone(),
            version,
            source: format!(
                "registry:{}/{}/{}",
                selected.package.registry, selected.package.namespace, selected.package.name
            ),
            registry: Some(selected.package.registry.clone()),
            namespace: Some(selected.package.namespace.clone()),
            archive_sha256: Some(archive_sha256.clone()),
            archive_length: Some(download.release.archive_length()),
            manifest_sha256: Some(download.release.manifest_sha256().to_owned()),
            provenance_sha256: Some(download.release.provenance_statement_sha256().to_owned()),
            package_signature_sha256: Some(download.release.package_signature_sha256().to_owned()),
            verification_sha256: Some(sha256_hex(&download.verification_bytes)),
            publisher_identity: Some(download.release.publisher_identity().to_owned()),
            signer_key_ids,
            cache_key: Some(format!("sha256:{archive_sha256}")),
            yanked_at_resolution: Some(selected.yanked),
            compatibility: LockedCompatibilityEvidence {
                contract: selected.compatibility.clone(),
                compiler: compatibility.compiler.clone(),
                edition_policy: selected.edition.clone(),
            },
        });
    }
    package.sort_by(|left, right| left.id.cmp(&right.id));

    let edge = resolved_dependency_edges(resolved)?;
    let mut roots = resolved.local.roots.clone();
    roots.sort();
    roots.dedup();
    let lockfile = LockfileV2 {
        version: LOCKFILE_V2_VERSION,
        compatibility,
        roots,
        registry,
        package,
        edge,
    };
    crate::lockfile::validate_lockfile_v2(&lockfile)
        .map_err(|error| PackageManagerError::new("lockfile_invalid", error.to_string()))?;
    Ok(lockfile)
}

fn same_dependency_edge_set(
    resolved: &[LockedDependencyEdgeV2],
    locked: &[LockedDependencyEdgeV2],
) -> bool {
    let identity = |edge: &LockedDependencyEdgeV2| {
        (
            edge.from.clone(),
            edge.to.clone(),
            edge.alias.clone(),
            edge.requested.clone(),
            edge.source_kind,
        )
    };
    resolved.len() == locked.len()
        && resolved.iter().map(identity).collect::<BTreeSet<_>>()
            == locked.iter().map(identity).collect::<BTreeSet<_>>()
}

fn resolved_dependency_edges(
    resolved: &OnlineResolution<'_>,
) -> Result<Vec<LockedDependencyEdgeV2>, PackageManagerError> {
    let selected_by_key = resolved
        .resolution
        .packages
        .iter()
        .map(|selected| (selected.package.clone(), selected))
        .collect::<BTreeMap<_, _>>();
    let mut edge = resolved.local.path_edges.clone();
    for (from, dependency) in &resolved.local.registry_edges {
        let selected = selected_by_key.get(&dependency.package).ok_or_else(|| {
            PackageManagerError::new(
                "resolution_incomplete",
                format!("resolver omitted direct dependency {}", dependency.package),
            )
        })?;
        edge.push(registry_lock_edge(
            from.clone(),
            dependency,
            selected,
            resolved,
        ));
    }
    for dependency in &resolved.resolution.edges {
        let Some(from) = &dependency.from else {
            continue;
        };
        let selected = selected_by_key.get(&dependency.to).ok_or_else(|| {
            PackageManagerError::new(
                "resolution_incomplete",
                format!("resolver omitted transitive dependency {}", dependency.to),
            )
        })?;
        edge.push(registry_lock_edge(
            registry_package_id(from, &resolved.resolution)?,
            &RegistryDependency {
                alias: dependency.alias.clone(),
                package: dependency.to.clone(),
                requirement: dependency.requirement.clone(),
            },
            selected,
            resolved,
        ));
    }
    edge.sort_by(lock_edge_order);
    edge.dedup();
    Ok(edge)
}

fn selected_download<'a>(
    source: &'a OnlineResolverSource<'_>,
    selected: &crate::package_resolver::ResolvedPackage,
) -> Result<&'a VerifiedDownload, PackageManagerError> {
    source
        .downloads
        .get(&(
            selected.package.clone(),
            selected.version,
            selected.release_id.clone(),
        ))
        .ok_or_else(|| {
            PackageManagerError::new(
                "verified_artifact_missing",
                format!(
                    "selected package {}@{} has no fully verified artifact set",
                    selected.package, selected.version
                ),
            )
        })
}

fn registry_package_id(
    package: &PackageKey,
    resolution: &Resolution,
) -> Result<String, PackageManagerError> {
    let selected = resolution
        .packages
        .iter()
        .find(|selected| &selected.package == package)
        .ok_or_else(|| {
            PackageManagerError::new(
                "resolution_incomplete",
                format!("resolver edge source {package} has no selected package"),
            )
        })?;
    Ok(canonical_registry_package_id(
        &package.registry,
        &package.namespace,
        &package.name,
        &selected.version.to_string(),
    ))
}

fn registry_lock_edge(
    from: String,
    dependency: &RegistryDependency,
    selected: &crate::package_resolver::ResolvedPackage,
    resolved: &OnlineResolution<'_>,
) -> LockedDependencyEdgeV2 {
    LockedDependencyEdgeV2 {
        from,
        to: canonical_registry_package_id(
            &selected.package.registry,
            &selected.package.namespace,
            &selected.package.name,
            &selected.version.to_string(),
        ),
        alias: dependency.alias.clone(),
        requested: dependency.requirement.to_string(),
        source_kind: LockedDependencySourceKind::Registry,
        reason: selection_reason(selected, resolved),
    }
}

fn selection_reason(
    selected: &crate::package_resolver::ResolvedPackage,
    resolved: &OnlineResolution<'_>,
) -> LockedDependencyReason {
    let retained = resolved
        .previous
        .as_ref()
        .and_then(|lockfile| {
            lockfile.package.iter().find(|package| {
                package.registry.as_deref() == Some(&selected.package.registry)
                    && package.namespace.as_deref() == Some(&selected.package.namespace)
                    && package.name == selected.package.name
                    && package.version == selected.version.to_string()
            })
        })
        .is_some();
    if retained && selected.yanked {
        LockedDependencyReason::TrustedYankedLockedReplay
    } else if retained && resolved.frozen.contains_key(&selected.package) {
        LockedDependencyReason::ExactLockedReplay
    } else {
        LockedDependencyReason::HighestCompatible
    }
}

fn materialized_graph(
    lockfile: &LockfileV2,
    lockfile_sha256: &str,
    roots: &BTreeMap<String, MaterializedRoot>,
    root_ids: &[String],
) -> Result<MaterializedPackageGraph, PackageManagerError> {
    let registry_by_name = lockfile
        .registry
        .iter()
        .map(|registry| (registry.name.as_str(), registry))
        .collect::<BTreeMap<_, _>>();
    let mut packages = Vec::with_capacity(lockfile.package.len());
    for package in &lockfile.package {
        let root = roots.get(&package.id).ok_or_else(|| {
            PackageManagerError::new(
                "materialization_incomplete",
                format!(
                    "locked package {} has no verified materialized root",
                    package.id
                ),
            )
        })?;
        let trust = match package.registry.as_deref() {
            Some(registry_name) => {
                root.verified_archive.as_ref().ok_or_else(|| {
                    PackageManagerError::new(
                        "materialization_incomplete",
                        format!(
                            "registry package {} lacks exact reverified archive bytes",
                            package.id
                        ),
                    )
                })?;
                root.verified_manifest.as_ref().ok_or_else(|| {
                    PackageManagerError::new(
                        "materialization_incomplete",
                        format!(
                            "registry package {} lacks exact authenticated manifest bytes",
                            package.id
                        ),
                    )
                })?;
                let registry = registry_by_name.get(registry_name).ok_or_else(|| {
                    PackageManagerError::new(
                        "lockfile_invalid",
                        format!(
                            "package {} references missing registry {registry_name:?}",
                            package.id
                        ),
                    )
                })?;
                Some(PackageTrustEvidence {
                    registry: registry_name.to_owned(),
                    registry_identity: registry.registry_identity.clone(),
                    source_identity: registry.source_identity.clone(),
                    publisher_identity: package.publisher_identity.clone().ok_or_else(|| {
                        PackageManagerError::new(
                            "lockfile_invalid",
                            format!("package {} lacks publisher identity", package.id),
                        )
                    })?,
                    archive_sha256: package.archive_sha256.clone().ok_or_else(|| {
                        PackageManagerError::new(
                            "lockfile_invalid",
                            format!("package {} lacks archive digest", package.id),
                        )
                    })?,
                    manifest_sha256: package.manifest_sha256.clone().ok_or_else(|| {
                        PackageManagerError::new(
                            "lockfile_invalid",
                            format!("package {} lacks manifest digest", package.id),
                        )
                    })?,
                    provenance_sha256: package.provenance_sha256.clone().ok_or_else(|| {
                        PackageManagerError::new(
                            "lockfile_invalid",
                            format!("package {} lacks provenance digest", package.id),
                        )
                    })?,
                    package_signature_sha256: package.package_signature_sha256.clone().ok_or_else(
                        || {
                            PackageManagerError::new(
                                "lockfile_invalid",
                                format!("package {} lacks package-signature digest", package.id),
                            )
                        },
                    )?,
                    signer_key_ids: package.signer_key_ids.clone(),
                    index_sha256: registry.index_sha256.clone(),
                    index_generation: registry.index_generation,
                    index_sequence: registry.index_sequence,
                    index_transcript_sha256: registry.index_transcript_sha256.clone(),
                    verification_sha256: required_locked_field(
                        package,
                        "verification_sha256",
                        &package.verification_sha256,
                    )?
                    .to_owned(),
                })
            }
            None => None,
        };
        packages.push(MaterializedPackage {
            id: package.id.clone(),
            name: package.name.clone(),
            version: package.version.clone(),
            source: package.source.clone(),
            root: root.path.display().to_string(),
            verified_archive: root.verified_archive.clone(),
            verified_manifest: root.verified_manifest.clone(),
            trust,
            materialization: MaterializationEvidence {
                source: root.source.clone(),
                content_key: root.content_key.clone(),
                package_trust_verified: package.registry.is_none()
                    || root.content_key.as_deref() == package.cache_key.as_deref(),
            },
        });
    }
    packages.sort_by(|left, right| left.id.cmp(&right.id));
    let mut edges = lockfile
        .edge
        .iter()
        .map(|edge| MaterializedDependencyEdge {
            from: edge.from.clone(),
            to: edge.to.clone(),
            alias: edge.alias.clone(),
            requested: edge.requested.clone(),
            source_kind: edge.source_kind.as_str().to_owned(),
            reason: edge.reason.as_str().to_owned(),
        })
        .collect::<Vec<_>>();
    edges.sort_by(|left, right| {
        left.from
            .cmp(&right.from)
            .then(left.alias.cmp(&right.alias))
            .then(left.to.cmp(&right.to))
            .then(left.requested.cmp(&right.requested))
    });
    let package_ids = lockfile
        .package
        .iter()
        .map(|package| package.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut graph_roots = root_ids.to_vec();
    graph_roots.sort();
    graph_roots.dedup();
    if let Some(missing) = graph_roots
        .iter()
        .find(|root| !package_ids.contains(root.as_str()))
    {
        return Err(PackageManagerError::new(
            "materialization_incomplete",
            format!("graph root {missing:?} is absent from axiom.lock"),
        ));
    }
    Ok(MaterializedPackageGraph {
        schema_version: MATERIALIZED_PACKAGE_GRAPH_SCHEMA.to_owned(),
        lockfile_sha256: lockfile_sha256.to_owned(),
        roots: graph_roots,
        packages,
        edges,
    })
}

fn operation_report(
    operation: PackageOperation,
    project_root: &Path,
    lockfile_path: &Path,
    graph: MaterializedPackageGraph,
    trace: Vec<TraceEvent>,
    transport_used: bool,
    vendor_lifecycle: Option<VendorLifecycleEvidence>,
) -> PackageOperationReport {
    let registry_count = graph
        .packages
        .iter()
        .filter(|package| package.trust.is_some())
        .count();
    let summary = format!(
        "{} materialized {} packages ({} registry, {} path)",
        match operation {
            PackageOperation::Fetch => "fetch",
            PackageOperation::Update => "update",
            PackageOperation::Vendor => "vendor",
        },
        graph.packages.len(),
        registry_count,
        graph.packages.len().saturating_sub(registry_count),
    );
    PackageOperationReport {
        schema_version: PACKAGE_OPERATION_REPORT_SCHEMA.to_owned(),
        operation,
        project: project_root.display().to_string(),
        lockfile: lockfile_path.display().to_string(),
        packages: graph.packages.clone(),
        graph,
        trace,
        transport_used,
        summary,
        vendor_lifecycle,
    }
}

fn local_materialized_roots(local: &LocalGraph) -> BTreeMap<String, MaterializedRoot> {
    local
        .packages
        .iter()
        .map(|package| {
            (
                package.id.clone(),
                MaterializedRoot {
                    path: package.root.clone(),
                    source: "path".to_owned(),
                    content_key: None,
                    verified_archive: None,
                    verified_manifest: None,
                },
            )
        })
        .collect()
}

fn locked_registry_for_package<'a>(
    lockfile: &'a LockfileV2,
    package: &LockedPackageV2,
) -> Result<&'a LockedRegistryV2, PackageManagerError> {
    let name = package.registry.as_deref().ok_or_else(|| {
        PackageManagerError::new(
            "lockfile_invalid",
            format!("package {} has no registry identity", package.id),
        )
    })?;
    lockfile
        .registry
        .iter()
        .find(|registry| registry.name == name)
        .ok_or_else(|| {
            PackageManagerError::new(
                "lockfile_invalid",
                format!(
                    "package {} references missing registry {name:?}",
                    package.id
                ),
            )
        })
}

fn verify_cached_against_lock(
    cached: &CachedPackage,
    package: &LockedPackageV2,
    registry: &LockedRegistryV2,
) -> Result<(), PackageManagerError> {
    let archive_sha256 = required_locked_field(package, "archive_sha256", &package.archive_sha256)?;
    let archive_length = package.archive_length.ok_or_else(|| {
        PackageManagerError::new(
            "lockfile_invalid",
            format!("locked package {} lacks archive_length", package.id),
        )
    })?;
    let manifest_sha256 =
        required_locked_field(package, "manifest_sha256", &package.manifest_sha256)?;
    let provenance_sha256 =
        required_locked_field(package, "provenance_sha256", &package.provenance_sha256)?;
    let package_signature_sha256 = required_locked_field(
        package,
        "package_signature_sha256",
        &package.package_signature_sha256,
    )?;
    let verification_sha256 =
        required_locked_field(package, "verification_sha256", &package.verification_sha256)?;
    let expected_cache_key = format!("sha256:{archive_sha256}");
    let exact = cached.archive_sha256 == archive_sha256
        && cached.integrity.archive_sha256 == archive_sha256
        && cached.commit.archive_sha256 == archive_sha256
        && cached.commit.archive_length == archive_length
        && cached.commit.manifest_sha256 == manifest_sha256
        && cached.commit.provenance_sha256 == provenance_sha256
        && cached.commit.signature_sha256 == package_signature_sha256
        && cached.commit.registry_index_sha256 == registry.index_sha256
        && cached.commit.verification_sha256 == verification_sha256
        && package.cache_key.as_deref() == Some(expected_cache_key.as_str());
    if !exact {
        return Err(PackageManagerError::new(
            "locked_package_mismatch",
            format!(
                "verified cache material does not match exact axiom.lock pins for {}",
                package.id
            ),
        ));
    }
    Ok(())
}

fn required_locked_field<'a>(
    package: &LockedPackageV2,
    field: &str,
    value: &'a Option<String>,
) -> Result<&'a str, PackageManagerError> {
    value.as_deref().ok_or_else(|| {
        PackageManagerError::new(
            "lockfile_invalid",
            format!("locked package {} lacks {field}", package.id),
        )
    })
}

fn locked_archive_sha256(package: &LockedPackageV2) -> Result<&str, PackageManagerError> {
    package.archive_sha256.as_deref().ok_or_else(|| {
        PackageManagerError::new(
            "lockfile_invalid",
            format!("locked package {} lacks archive_sha256", package.id),
        )
    })
}

fn locked_verification_sha256(package: &LockedPackageV2) -> Result<&str, PackageManagerError> {
    required_locked_field(package, "verification_sha256", &package.verification_sha256)
}

fn locked_vendor_packages(
    lockfile: &LockfileV2,
) -> Result<Vec<VendorPackage<'_>>, PackageManagerError> {
    lockfile
        .package
        .iter()
        .filter(|package| package.registry.is_some())
        .map(|package| {
            let registry = locked_registry_for_package(lockfile, package)?;
            Ok(VendorPackage {
                package_id: &package.id,
                archive_sha256: locked_archive_sha256(package)?,
                registry_index_sha256: &registry.index_sha256,
                verification_sha256: locked_verification_sha256(package)?,
            })
        })
        .collect()
}

fn verify_vendor_against_lock(
    snapshot: &VendorSnapshot,
    lockfile: &LockfileV2,
) -> Result<(), PackageManagerError> {
    let expected = lockfile
        .package
        .iter()
        .filter(|package| package.registry.is_some())
        .map(|package| {
            Ok((
                package.id.clone(),
                (
                    package.cache_key.clone().ok_or_else(|| {
                        PackageManagerError::new(
                            "lockfile_invalid",
                            format!("locked package {} lacks cache_key", package.id),
                        )
                    })?,
                    locked_archive_sha256(package)?.to_owned(),
                    locked_registry_for_package(lockfile, package)?
                        .index_sha256
                        .clone(),
                    locked_verification_sha256(package)?.to_owned(),
                ),
            ))
        })
        .collect::<Result<BTreeMap<_, _>, PackageManagerError>>()?;
    let observed = snapshot
        .manifest
        .packages
        .iter()
        .map(|package| {
            (
                package.package_id.clone(),
                (
                    package.content_key.clone(),
                    package.archive_sha256.clone(),
                    package.registry_index_sha256.clone(),
                    package.verification_sha256.clone(),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    if observed != expected {
        return Err(PackageManagerError::new(
            "vendor_lock_mismatch",
            "verified vendor snapshot does not exactly match registry packages in axiom.lock",
        ));
    }
    Ok(())
}

fn offline_trust_context(
    project_root: &Path,
    manifest: &Manifest,
    lockfile: &LockfileV2,
) -> Result<OfflineTrustContext, PackageManagerError> {
    let configured = manifest.registry.as_ref().ok_or_else(|| {
        PackageManagerError::new(
            "registry_missing",
            "locked registry graph requires a root [registry] configuration",
        )
    })?;
    if lockfile.registry.len() != 1
        || lockfile.registry[0].name != configured.name
        || lockfile.registry[0].source != configured.index
    {
        return Err(PackageManagerError::new(
            "locked_registry_mismatch",
            "configured registry does not exactly match axiom.lock",
        ));
    }
    let trust_roots = read_bounded_file(
        &project_root.join(&configured.trust_roots),
        MAX_TRUST_DOCUMENT_BYTES,
    )
    .map_err(|message| PackageManagerError::new("trust_roots_unavailable", message))?;
    if sha256_hex(&trust_roots) != lockfile.registry[0].trust_roots_sha256 {
        return Err(PackageManagerError::new(
            "locked_trust_roots_mismatch",
            "current trust-roots bytes do not match the exact axiom.lock pin",
        ));
    }
    let trust_roots = parse_trust_roots_json(&trust_roots)
        .map_err(|error| PackageManagerError::new("trust_roots_invalid", error.to_string()))?;
    let expectation = read_bounded_file(
        &project_root.join(&configured.expectation),
        MAX_TRUST_DOCUMENT_BYTES,
    )
    .map_err(|message| PackageManagerError::new("expectation_unavailable", message))?;
    if sha256_hex(&expectation) != lockfile.registry[0].expectation_sha256 {
        return Err(PackageManagerError::new(
            "locked_expectation_mismatch",
            "current verification-expectation bytes do not match the exact axiom.lock pin",
        ));
    }
    let mut expectation = parse_verification_expectation_json(&expectation)
        .map_err(|error| PackageManagerError::new("expectation_invalid", error.to_string()))?;
    advance_expectation_from_lock(&mut expectation, lockfile, configured)?;
    Ok(OfflineTrustContext {
        registry: configured.clone(),
        trust_roots,
        expectation,
    })
}

fn verify_offline_package_trust(
    cached: &CachedPackage,
    package: &LockedPackageV2,
    registry: &LockedRegistryV2,
    lockfile: &LockfileV2,
    trust: &OfflineTrustContext,
) -> Result<(), PackageManagerError> {
    let artifacts = cached.verified_artifacts().map_err(store_error)?;
    let catalog = authenticate_registry_catalog(
        &artifacts.registry_index,
        &trust.trust_roots,
        &trust.expectation,
    )
    .map_err(|error| {
        PackageManagerError::new(
            "offline_registry_catalog_rejected",
            error.reason_codes.join(", "),
        )
    })?;
    validate_offline_catalog_pins(&catalog, registry)?;
    let namespace = package.namespace.as_deref().ok_or_else(|| {
        PackageManagerError::new(
            "lockfile_invalid",
            format!("locked package {} lacks namespace", package.id),
        )
    })?;
    let release = catalog
        .release(namespace, &package.name, &package.version)
        .ok_or_else(|| {
            PackageManagerError::new(
                "offline_release_missing",
                format!(
                    "locked package {} is absent from its exact authenticated registry index",
                    package.id
                ),
            )
        })?;
    validate_offline_release_pins(release, package, registry, artifacts.archive.len())?;
    let package_signature =
        parse_package_signature_json(&artifacts.signature).map_err(|error| {
            PackageManagerError::new("package_signature_invalid", error.to_string())
        })?;
    let registry_index = parse_registry_index_json(&artifacts.registry_index)
        .map_err(|error| PackageManagerError::new("registry_index_invalid", error.to_string()))?;
    let expectation =
        verification_expectation_for_authenticated_release(&trust.expectation, &catalog, release)
            .map_err(|error| {
            PackageManagerError::new("release_expectation_invalid", error.reason_codes.join(", "))
        })?;
    let verification = verify_package_with_artifacts(
        &PackageTrustInput {
            package_signature,
            trust_roots: trust.trust_roots.clone(),
            registry_index,
            verification_expectation: expectation,
        },
        PackageArtifacts {
            archive: Some(&artifacts.archive),
            manifest: Some(&artifacts.manifest),
            provenance: Some(&artifacts.provenance),
        },
    );
    if verification.decision != "trusted" {
        return Err(PackageManagerError::new(
            "offline_package_trust_rejected",
            format!(
                "{} rejected: {}",
                package.id,
                verification.reason_codes.join(", ")
            ),
        ));
    }
    let mut signers = verification
        .signers
        .iter()
        .map(|signer| signer.key_id.clone())
        .collect::<Vec<_>>();
    signers.sort();
    signers.dedup();
    if signers != package.signer_key_ids {
        return Err(PackageManagerError::new(
            "locked_package_mismatch",
            format!(
                "fresh Package Trust signer evidence differs from axiom.lock for {}",
                package.id
            ),
        ));
    }
    let stored_verification: PackageVerification = serde_json::from_slice(&artifacts.verification)
        .map_err(|error| {
            PackageManagerError::new(
                "stored_verification_invalid",
                format!("stored Package Trust evidence is invalid: {error}"),
            )
        })?;
    let canonical = serde_json::to_vec(&stored_verification).map_err(|error| {
        PackageManagerError::new(
            "stored_verification_invalid",
            format!("stored Package Trust evidence cannot be rendered: {error}"),
        )
    })?;
    if stored_verification != verification || canonical != artifacts.verification {
        return Err(PackageManagerError::new(
            "stored_verification_mismatch",
            format!(
                "stored Package Trust verdict differs from a fresh verification for {}",
                package.id
            ),
        ));
    }
    verify_offline_manifest_edges(
        &artifacts.manifest,
        package,
        lockfile,
        &trust.registry,
        catalog.source_identity(),
    )
}

fn validate_offline_catalog_pins(
    catalog: &AuthenticatedRegistryCatalog,
    locked: &LockedRegistryV2,
) -> Result<(), PackageManagerError> {
    let mut signer_ids = catalog.index_signer_key_ids().to_vec();
    signer_ids.sort();
    signer_ids.dedup();
    let exact = catalog.registry_identity() == locked.registry_identity
        && catalog.source_identity() == locked.source_identity
        && catalog.root_version() == locked.current_root_version
        && catalog.root_sequence() == locked.current_root_sequence
        && catalog.root_transcript_sha256() == locked.current_root_transcript_sha256
        && sha256_hex(catalog.exact_index_bytes()) == locked.index_sha256
        && catalog.index_transcript_sha256() == locked.index_transcript_sha256
        && catalog.generation() == locked.index_generation
        && catalog.sequence() == locked.index_sequence
        && catalog.snapshot_id() == locked.index_snapshot_id
        && signer_ids == locked.index_signer_key_ids;
    if !exact {
        return Err(PackageManagerError::new(
            "locked_registry_mismatch",
            "fresh offline catalog authentication differs from exact axiom.lock pins",
        ));
    }
    Ok(())
}

fn validate_offline_release_pins(
    release: &AuthenticatedRegistryRelease,
    package: &LockedPackageV2,
    registry: &LockedRegistryV2,
    archive_length: usize,
) -> Result<(), PackageManagerError> {
    let exact = release.registry_identity() == registry.registry_identity
        && release.source_identity() == registry.source_identity
        && release.namespace() == package.namespace.as_deref().unwrap_or_default()
        && release.name() == package.name
        && release.version() == package.version
        && package.archive_sha256.as_deref() == Some(release.archive_sha256())
        && package.archive_length == Some(release.archive_length())
        && archive_length as u64 == release.archive_length()
        && package.manifest_sha256.as_deref() == Some(release.manifest_sha256())
        && package.provenance_sha256.as_deref() == Some(release.provenance_statement_sha256())
        && package.package_signature_sha256.as_deref() == Some(release.package_signature_sha256())
        && package.publisher_identity.as_deref() == Some(release.publisher_identity())
        && package.yanked_at_resolution == Some(release.yanked());
    if !exact {
        return Err(PackageManagerError::new(
            "locked_package_mismatch",
            format!(
                "authenticated registry release differs from exact axiom.lock pins for {}",
                package.id
            ),
        ));
    }
    Ok(())
}

fn verify_offline_manifest_edges(
    manifest_bytes: &[u8],
    package: &LockedPackageV2,
    lockfile: &LockfileV2,
    registry: &RegistryConfig,
    source_identity: &str,
) -> Result<(), PackageManagerError> {
    let manifest = parse_manifest_exact(manifest_bytes, Path::new("cached/axiom.toml"))
        .map_err(|error| PackageManagerError::new("package_manifest_invalid", error.to_string()))?;
    let section = manifest.package.as_ref().ok_or_else(|| {
        PackageManagerError::new(
            "package_identity_mismatch",
            format!(
                "cached manifest for {} has no [package] section",
                package.id
            ),
        )
    })?;
    if section.name != package.name || section.version != package.version {
        return Err(PackageManagerError::new(
            "package_identity_mismatch",
            format!("cached manifest identity differs from {}", package.id),
        ));
    }
    let dependencies = manifest_dependencies(&manifest, registry, source_identity)
        .map_err(PackageManagerError::from_source)?;
    let outgoing = lockfile
        .edge
        .iter()
        .filter(|edge| edge.from == package.id)
        .collect::<Vec<_>>();
    if outgoing.len() != dependencies.len() {
        return Err(PackageManagerError::new(
            "locked_dependency_mismatch",
            format!(
                "cached manifest dependency count differs from axiom.lock for {}",
                package.id
            ),
        ));
    }
    for dependency in dependencies {
        let Dependency::Registry(dependency) = dependency else {
            return Err(PackageManagerError::new(
                "registry_path_dependency_unsupported",
                "authenticated registry packages may not depend on mutable path sources",
            ));
        };
        let matched = outgoing.iter().any(|edge| {
            edge.alias == dependency.alias
                && edge.requested == dependency.requirement.to_string()
                && edge.source_kind == LockedDependencySourceKind::Registry
                && lockfile.package.iter().any(|target| {
                    target.id == edge.to
                        && target.registry.as_deref() == Some(&dependency.package.registry)
                        && target.namespace.as_deref() == Some(&dependency.package.namespace)
                        && target.name == dependency.package.name
                })
        });
        if !matched {
            return Err(PackageManagerError::new(
                "locked_dependency_mismatch",
                format!(
                    "cached dependency {:?} differs from axiom.lock for {}",
                    dependency.alias, package.id
                ),
            ));
        }
    }
    Ok(())
}

fn store_error(error: StoreError) -> PackageManagerError {
    PackageManagerError::new(error.code, error.message)
}

fn lockfile_write_error(error: Diagnostic) -> PackageManagerError {
    let message = error.to_string();
    PackageManagerError::new(
        if message.contains("changed while package resolution was in progress") {
            "lockfile_concurrent_change"
        } else {
            "lockfile_write_failed"
        },
        message,
    )
}

fn lockfile_has_registry_packages(lockfile: &LockfileV2) -> bool {
    lockfile
        .package
        .iter()
        .any(|package| package.registry.is_some())
}

fn verify_locked_local_graph(
    project_root: &Path,
    manifest: &Manifest,
    lockfile: &LockfileV2,
) -> Result<LocalGraph, PackageManagerError> {
    let current = current_compatibility()?;
    if lockfile.compatibility != current {
        return Err(PackageManagerError::new(
            "locked_compatibility_mismatch",
            "axiom.lock compatibility evidence does not match this compiler",
        ));
    }
    if lockfile
        .package
        .iter()
        .any(|package| package.compatibility != current)
    {
        return Err(PackageManagerError::new(
            "locked_compatibility_mismatch",
            "one or more locked package compatibility records do not match this compiler",
        ));
    }
    let path_only_registry = RegistryConfig {
        name: "path-only".to_owned(),
        index: "file:///path-only/index.json".to_owned(),
        trust_roots: "unused".to_owned(),
        expectation: "unused".to_owned(),
        cache: None,
        vendor: None,
    };
    let (registry_config, source_identity) = if lockfile_has_registry_packages(lockfile) {
        let registry_config = manifest.registry.as_ref().ok_or_else(|| {
            PackageManagerError::new(
                "registry_missing",
                "registry package materialization requires the configured root registry",
            )
        })?;
        let registry_record = lockfile
            .registry
            .iter()
            .find(|registry| registry.name == registry_config.name)
            .ok_or_else(|| {
                PackageManagerError::new(
                    "locked_registry_missing",
                    format!(
                        "axiom.lock has no registry record for {:?}",
                        registry_config.name
                    ),
                )
            })?;
        if registry_record.source != registry_config.index {
            return Err(PackageManagerError::new(
                "locked_registry_mismatch",
                "configured registry source differs from axiom.lock",
            ));
        }
        (registry_config, registry_record.source_identity.as_str())
    } else {
        if !lockfile.registry.is_empty() {
            return Err(PackageManagerError::new(
                "lockfile_invalid",
                "path-only axiom.lock must not retain registry trust records",
            ));
        }
        (&path_only_registry, "path-only")
    };
    let local = collect_local_graph(project_root, manifest, registry_config, source_identity)?;
    if lockfile.roots != local.roots {
        return Err(PackageManagerError::new(
            "locked_path_graph_mismatch",
            "local project/workspace roots differ from axiom.lock",
        ));
    }
    let locked_paths = lockfile
        .package
        .iter()
        .filter(|package| package.registry.is_none())
        .collect::<Vec<_>>();
    if locked_paths.len() != local.packages.len() {
        return Err(PackageManagerError::new(
            "locked_path_graph_mismatch",
            "local path package count differs from axiom.lock",
        ));
    }
    for package in &local.packages {
        let section = package.manifest.package.as_ref().ok_or_else(|| {
            PackageManagerError::new(
                "path_package_missing",
                format!(
                    "path package {} has no [package] section",
                    package.root.display()
                ),
            )
        })?;
        let locked = locked_paths
            .iter()
            .find(|locked| locked.id == package.id)
            .ok_or_else(|| {
                PackageManagerError::new(
                    "locked_path_graph_mismatch",
                    format!(
                        "local path package {} is absent from axiom.lock",
                        package.id
                    ),
                )
            })?;
        if locked.name != section.name
            || locked.version != section.version
            || locked.source != package.source
            || locked.compatibility != current
        {
            return Err(PackageManagerError::new(
                "locked_path_graph_mismatch",
                format!("local path package {} differs from axiom.lock", package.id),
            ));
        }
    }
    let locked_path_edges = lockfile
        .edge
        .iter()
        .filter(|edge| edge.source_kind == LockedDependencySourceKind::Path)
        .cloned()
        .collect::<Vec<_>>();
    if locked_path_edges != local.path_edges {
        return Err(PackageManagerError::new(
            "locked_path_graph_mismatch",
            "local path dependency edges differ from axiom.lock",
        ));
    }
    let local_ids = local
        .packages
        .iter()
        .map(|package| package.id.as_str())
        .collect::<BTreeSet<_>>();
    let locked_local_registry_edges = lockfile
        .edge
        .iter()
        .filter(|edge| {
            edge.source_kind == LockedDependencySourceKind::Registry
                && local_ids.contains(edge.from.as_str())
        })
        .collect::<Vec<_>>();
    if locked_local_registry_edges.len() != local.registry_edges.len() {
        return Err(PackageManagerError::new(
            "locked_path_graph_mismatch",
            "local registry dependency edge set differs from axiom.lock",
        ));
    }
    for (from, dependency) in &local.registry_edges {
        let matching = locked_local_registry_edges.iter().any(|edge| {
            edge.from == *from
                && edge.alias == dependency.alias
                && edge.requested == dependency.requirement.to_string()
                && edge.source_kind == LockedDependencySourceKind::Registry
                && lockfile.package.iter().any(|package| {
                    package.id == edge.to
                        && package.registry.as_deref() == Some(&dependency.package.registry)
                        && package.namespace.as_deref() == Some(&dependency.package.namespace)
                        && package.name == dependency.package.name
                })
        });
        if !matching {
            return Err(PackageManagerError::new(
                "locked_path_graph_mismatch",
                format!(
                    "registry dependency {:?} from {from} differs from axiom.lock",
                    dependency.alias
                ),
            ));
        }
    }
    Ok(local)
}

fn map_resolve_error(error: ResolveError, targeted: bool) -> PackageManagerError {
    let trace = match &error {
        ResolveError::Conflict { trace, .. }
        | ResolveError::BudgetExceeded { trace, .. }
        | ResolveError::Source { trace, .. } => trace.clone(),
        _ => Vec::new(),
    };
    let broader = match &error {
        ResolveError::Conflict { trace, .. } | ResolveError::BudgetExceeded { trace, .. } => {
            targeted && trace.iter().any(trace_is_frozen_mismatch)
        }
        _ => false,
    };
    let resolver = serde_json::to_value(&error).ok();
    let mut mapped = if broader {
        PackageManagerError::new(
            "broader_update_required",
            "targeted update conflicts with a frozen package; run an untargeted update",
        )
    } else {
        let code = match error {
            ResolveError::Conflict { .. } => "resolution_conflict",
            ResolveError::BudgetExceeded { .. } => "resolution_budget_exceeded",
            ResolveError::InvalidCatalog { .. } => "registry_catalog_invalid",
            ResolveError::InvalidRequest(_) => "resolution_request_invalid",
            ResolveError::InvalidResolution(_) => "resolution_output_invalid",
            ResolveError::Source { .. } => "package_source_failed",
        };
        PackageManagerError::new(code, error.to_string())
    };
    mapped.trace = trace;
    mapped.resolver = resolver;
    mapped
}

fn manifest_dependencies(
    manifest: &Manifest,
    registry: &RegistryConfig,
    source_identity: &str,
) -> Result<Vec<Dependency>, SourceFailure> {
    let mut dependencies = Vec::with_capacity(manifest.dependencies.len());
    for (alias, spec) in &manifest.dependencies {
        dependencies.push(manifest_dependency(alias, spec, registry, source_identity)?);
    }
    Ok(dependencies)
}

fn collect_local_graph(
    project_root: &Path,
    manifest: &Manifest,
    registry: &RegistryConfig,
    source_identity: &str,
) -> Result<LocalGraph, PackageManagerError> {
    let project_root = std::fs::canonicalize(project_root).map_err(|error| {
        PackageManagerError::new(
            "project_unavailable",
            format!("failed to canonicalize package root: {error}"),
        )
    })?;
    let mut queue = VecDeque::from([(project_root.clone(), true)]);
    let mut visited = BTreeSet::new();
    let mut packages = Vec::new();
    let mut roots = Vec::new();
    while let Some((root, is_graph_root)) = queue.pop_front() {
        if !visited.insert(root.clone()) {
            continue;
        }
        let package_manifest = if root == project_root {
            manifest.clone()
        } else {
            load_manifest(&root).map_err(|error| {
                PackageManagerError::new("path_manifest_invalid", error.to_string())
            })?
        };
        if let Some(workspace) = &package_manifest.workspace {
            for member in &workspace.members {
                let member_root =
                    canonical_local_root(&project_root, &root, member, "workspace member")?;
                queue.push_back((member_root, true));
            }
        }
        for (alias, dependency) in &package_manifest.dependencies {
            if let Some(path) = dependency.path_source() {
                let dependency_root =
                    canonical_local_root(&project_root, &root, path, "path dependency")?;
                queue.push_back((dependency_root, false));
            } else if package_manifest.package.is_none() {
                return Err(PackageManagerError::new(
                    "virtual_root_registry_dependency_unsupported",
                    format!(
                        "registry dependency {alias:?} must originate from a package with a stable lockfile identity"
                    ),
                ));
            }
        }
        let Some(section) = package_manifest.package.as_ref() else {
            continue;
        };
        let source = local_path_source(&project_root, &root)?;
        let id = canonical_path_package_id(&source, &section.name, &section.version);
        if is_graph_root {
            roots.push(id.clone());
        }
        packages.push(LocalPackage {
            id,
            source,
            root,
            manifest: package_manifest,
        });
    }
    if packages.is_empty() {
        return Err(PackageManagerError::new(
            "path_package_missing",
            "package graph contains no [package] section",
        ));
    }
    let distinct_ids = packages
        .iter()
        .map(|package| package.id.as_str())
        .collect::<BTreeSet<_>>();
    if distinct_ids.len() != packages.len() {
        return Err(PackageManagerError::new(
            "path_graph_ambiguous",
            "multiple local packages have the same canonical lockfile identity",
        ));
    }
    for package in &packages {
        if !package.root.starts_with(&project_root) {
            return Err(PackageManagerError::new(
                "path_dependency_outside_project",
                format!(
                    "local package {} resolves outside {}",
                    package.root.display(),
                    project_root.display()
                ),
            ));
        }
    }
    packages.sort_by(|left, right| left.id.cmp(&right.id));
    roots.sort();
    roots.dedup();

    let ids_by_root = packages
        .iter()
        .map(|package| (package.root.clone(), package.id.clone()))
        .collect::<BTreeMap<_, _>>();
    let root_ids = roots.iter().cloned().collect::<BTreeSet<_>>();
    let mut path_edges = Vec::new();
    let mut registry_edges = Vec::new();
    let mut resolver_dependencies = Vec::new();
    for package in &packages {
        for (alias, spec) in &package.manifest.dependencies {
            if let Some(path) = spec.path_source() {
                let target_root =
                    std::fs::canonicalize(package.root.join(path)).map_err(|error| {
                        PackageManagerError::new(
                            "path_dependency_unavailable",
                            format!(
                                "failed to canonicalize dependency {alias:?} from {}: {error}",
                                package.root.display()
                            ),
                        )
                    })?;
                let to = ids_by_root.get(&target_root).ok_or_else(|| {
                    PackageManagerError::new(
                        "path_dependency_unlocked",
                        format!(
                            "dependency {alias:?} from {} is absent from the validated path graph",
                            package.root.display()
                        ),
                    )
                })?;
                path_edges.push(LockedDependencyEdgeV2 {
                    from: package.id.clone(),
                    to: to.clone(),
                    alias: alias.clone(),
                    requested: spec.version.clone().unwrap_or_else(|| "*".to_owned()),
                    source_kind: LockedDependencySourceKind::Path,
                    reason: if root_ids.contains(&package.id) {
                        LockedDependencyReason::RootPathConstraint
                    } else {
                        LockedDependencyReason::TransitivePathConstraint
                    },
                });
                resolver_dependencies.push(Dependency::Path(PathDependency {
                    alias: alias.clone(),
                    path: path.to_owned(),
                    version: None,
                }));
            } else {
                let dependency = match manifest_dependency(alias, spec, registry, source_identity)
                    .map_err(PackageManagerError::from_source)?
                {
                    Dependency::Registry(dependency) => dependency,
                    Dependency::Path(_) => unreachable!("path dependency was handled above"),
                };
                registry_edges.push((package.id.clone(), dependency.clone()));
                resolver_dependencies.push(Dependency::Registry(dependency));
            }
        }
    }
    path_edges.sort_by(lock_edge_order);
    registry_edges.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then(left.1.alias.cmp(&right.1.alias))
            .then(left.1.package.cmp(&right.1.package))
    });
    resolver_dependencies.sort_by(|left, right| {
        serde_json::to_string(left)
            .unwrap_or_default()
            .cmp(&serde_json::to_string(right).unwrap_or_default())
    });
    Ok(LocalGraph {
        roots,
        packages,
        path_edges,
        registry_edges,
        resolver_dependencies,
    })
}

fn canonical_local_root(
    project_root: &Path,
    containing_root: &Path,
    relative: &str,
    kind: &str,
) -> Result<PathBuf, PackageManagerError> {
    let root = std::fs::canonicalize(containing_root.join(relative)).map_err(|error| {
        PackageManagerError::new(
            "path_package_unavailable",
            format!(
                "failed to canonicalize {kind} {relative:?} from {}: {error}",
                containing_root.display()
            ),
        )
    })?;
    if !root.starts_with(project_root) {
        return Err(PackageManagerError::new(
            "path_dependency_outside_project",
            format!(
                "{kind} {} resolves outside {}",
                root.display(),
                project_root.display()
            ),
        ));
    }
    Ok(root)
}

fn local_path_source(
    project_root: &Path,
    package_root: &Path,
) -> Result<String, PackageManagerError> {
    if package_root == project_root {
        return Ok("path".to_owned());
    }
    let relative = package_root.strip_prefix(project_root).map_err(|_| {
        PackageManagerError::new(
            "path_dependency_outside_project",
            format!(
                "local package {} resolves outside {}",
                package_root.display(),
                project_root.display()
            ),
        )
    })?;
    let portable = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    Ok(format!("path:{portable}"))
}

fn lock_edge_order(
    left: &LockedDependencyEdgeV2,
    right: &LockedDependencyEdgeV2,
) -> std::cmp::Ordering {
    left.from
        .cmp(&right.from)
        .then(left.alias.cmp(&right.alias))
        .then(left.to.cmp(&right.to))
        .then(left.requested.cmp(&right.requested))
        .then(left.source_kind.cmp(&right.source_kind))
        .then(left.reason.cmp(&right.reason))
}

fn manifest_dependency(
    alias: &str,
    spec: &DependencySpec,
    registry: &RegistryConfig,
    source_identity: &str,
) -> Result<Dependency, SourceFailure> {
    if let Some(path) = spec.path_source() {
        return Ok(Dependency::Path(PathDependency {
            alias: alias.to_owned(),
            path: path.to_owned(),
            version: None,
        }));
    }
    let dependency = spec.registry_source().ok_or_else(|| {
        SourceFailure::new(
            "dependency_source_invalid",
            format!("dependency {alias:?} has no path or registry source"),
        )
    })?;
    if dependency.registry != registry.name {
        return Err(SourceFailure::new(
            "registry_alias_mismatch",
            format!(
                "dependency {alias:?} selects registry {:?}, but only {:?} is authenticated",
                dependency.registry, registry.name
            ),
        ));
    }
    let requirement = spec.version.as_deref().ok_or_else(|| {
        SourceFailure::new(
            "dependency_requirement_missing",
            format!("registry dependency {alias:?} has no version requirement"),
        )
    })?;
    Ok(Dependency::Registry(RegistryDependency {
        alias: alias.to_owned(),
        package: PackageKey::new(
            registry.name.clone(),
            source_identity.to_owned(),
            dependency.namespace.clone(),
            dependency.package.clone(),
        ),
        requirement: VersionRequirement::parse(requirement).map_err(|error| {
            SourceFailure::new(
                "dependency_requirement_invalid",
                format!("dependency {alias:?}: {error}"),
            )
        })?,
    }))
}

fn read_bounded_file(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    read_bounded_regular_file(path, limit).map_err(|error| error.to_string())
}

fn normalize_lexical_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                let _ = normalized.pop();
            }
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(std::path::MAIN_SEPARATOR.to_string()),
            std::path::Component::Normal(component) => normalized.push(component),
        }
    }
    normalized
}

fn vendor_snapshot_is_present(vendor_root: &Path) -> Result<bool, PackageManagerError> {
    match std::fs::symlink_metadata(vendor_root.join("CURRENT")) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(PackageManagerError::new(
            "vendor_snapshot_unavailable",
            format!(
                "failed to inspect vendor snapshot at {}: {error}",
                vendor_root.display()
            ),
        )),
    }
}

fn transport_error(error: RegistryClientError) -> PackageManagerError {
    PackageManagerError::new("registry_transport_failed", error.to_string())
}

fn source_transport_error(error: RegistryClientError) -> SourceFailure {
    SourceFailure::new("registry_transport_failed", error.to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn current_compatibility() -> Result<LockedCompatibilityEvidence, PackageManagerError> {
    let contract: serde_json::Value = serde_json::from_str(include_str!(
        "../../../compatibility/fixtures/current/contract.json"
    ))
    .map_err(|error| {
        PackageManagerError::new(
            "compatibility_contract_invalid",
            format!("embedded Compatibility v1 contract is invalid: {error}"),
        )
    })?;
    let required = |pointer: &str| {
        contract
            .pointer(pointer)
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                PackageManagerError::new(
                    "compatibility_contract_invalid",
                    format!("embedded Compatibility v1 contract lacks {pointer}"),
                )
            })
    };
    let edition = required("/edition/id")?;
    let policy = required("/policy_version")?;
    Ok(LockedCompatibilityEvidence {
        contract: required("/contract_version")?,
        compiler: required("/compiler/current")?,
        edition_policy: format!("{edition}@{policy}"),
    })
}

pub fn fetch_packages(project_root: &Path) -> Result<PackageOperationReport, PackageManagerError> {
    let manager = PackageManager::open(project_root)?;
    let transport = RegistryClient::default();
    manager.fetch(FetchOptions::new(&transport))
}

pub fn update_packages(
    project_root: &Path,
    package: Option<&str>,
) -> Result<PackageOperationReport, PackageManagerError> {
    let manager = PackageManager::open(project_root)?;
    let transport = RegistryClient::default();
    let mut options = UpdateOptions::new(&transport);
    options.package = package.map(str::to_owned);
    manager.update(options)
}

pub fn vendor_packages(
    project_root: &Path,
    out: Option<&Path>,
) -> Result<PackageOperationReport, PackageManagerError> {
    PackageManager::open(project_root)?.vendor(VendorOptions {
        out: out.map(Path::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_lockfile() -> LockfileV2 {
        let mut value: serde_json::Value = serde_json::from_str(include_str!(
            "../../../package-resolver/fixtures/lockfile-v2.json"
        ))
        .expect("parse lockfile fixture");
        value
            .as_object_mut()
            .expect("lockfile object")
            .entry("roots")
            .or_insert_with(|| serde_json::json!(["path:.#demo@0.1.0"]));
        serde_json::from_value(value).expect("deserialize lockfile fixture")
    }

    fn package_trust_expectation() -> VerificationExpectation {
        let contract: serde_json::Value = serde_json::from_str(include_str!(
            "../../../package-trust/contract/package-trust.json"
        ))
        .expect("parse Package Trust contract");
        VerificationExpectation(contract["verification_expectation"].clone())
    }

    fn write_path_only_project(root: &Path) {
        std::fs::create_dir_all(root).expect("create project");
        std::fs::write(
            root.join("axiom.toml"),
            "[package]\nname = \"app\"\nversion = \"1.0.0\"\n",
        )
        .expect("write manifest");
    }

    fn package_archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut archive = crate::package_archive::ARCHIVE_MAGIC.to_vec();
        for (path, bytes) in entries {
            archive.extend_from_slice(format!("--- file {path} {} ---\n", bytes.len()).as_bytes());
            archive.extend_from_slice(bytes);
            if !bytes.ends_with(b"\n") {
                archive.push(b'\n');
            }
        }
        archive
    }

    fn path_only_v2_lockfile() -> LockfileV2 {
        let compatibility = current_compatibility().expect("current compatibility");
        let root = canonical_path_package_id("path", "app", "1.0.0");
        LockfileV2 {
            version: LOCKFILE_V2_VERSION,
            compatibility: compatibility.clone(),
            roots: vec![root.clone()],
            registry: Vec::new(),
            package: vec![LockedPackageV2 {
                id: root,
                name: "app".to_owned(),
                version: "1.0.0".to_owned(),
                source: "path".to_owned(),
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
                compatibility,
            }],
            edge: Vec::new(),
        }
    }

    #[test]
    fn operation_and_graph_reports_are_deterministically_serializable() {
        let graph = MaterializedPackageGraph {
            schema_version: MATERIALIZED_PACKAGE_GRAPH_SCHEMA.to_owned(),
            lockfile_sha256: "0".repeat(64),
            roots: vec!["path:.#app@1.0.0".to_owned()],
            packages: Vec::new(),
            edges: Vec::new(),
        };
        let report = PackageOperationReport {
            schema_version: PACKAGE_OPERATION_REPORT_SCHEMA.to_owned(),
            operation: PackageOperation::Fetch,
            project: "/workspace/app".to_owned(),
            lockfile: "/workspace/app/axiom.lock".to_owned(),
            packages: Vec::new(),
            graph,
            trace: Vec::new(),
            transport_used: false,
            summary: "materialized 0 registry packages".to_owned(),
            vendor_lifecycle: None,
        };
        let first = serde_json::to_string(&report).expect("serialize report");
        let second = serde_json::to_string(&report).expect("serialize report again");
        assert_eq!(first, second);
        assert!(first.contains("\"operation\":\"fetch\""));
    }

    #[test]
    fn locked_and_vendor_options_expose_no_transport_handle() {
        let materialize = MaterializeOptions {
            prefer_vendor: true,
        };
        let vendor = VendorOptions {
            out: Some(PathBuf::from("vendor")),
        };
        assert!(materialize.prefer_vendor);
        assert_eq!(vendor.out.as_deref(), Some(Path::new("vendor")));
    }

    #[test]
    fn authenticated_archive_must_embed_exact_manifest_and_build_entry() {
        let manifest_bytes = br#"[package]
name = "demo"
version = "1.0.0"

[build]
entry = "src/main.ax"
out_dir = "dist"
"#;
        let manifest = parse_manifest_exact(manifest_bytes, Path::new("authenticated/axiom.toml"))
            .expect("parse manifest");
        let archive = package_archive(&[
            ("axiom.toml", manifest_bytes),
            ("src/main.ax", b"print \"trusted\"\n"),
        ]);
        let digest = sha256_hex(&archive);
        validate_authenticated_archive_contract(&archive, &digest, manifest_bytes, &manifest)
            .expect("exact archive contract");

        let mismatch = validate_authenticated_archive_contract(
            &archive,
            &digest,
            b"[package]\nname = \"other\"\nversion = \"1.0.0\"\n",
            &manifest,
        )
        .expect_err("embedded manifest mismatch");
        assert_eq!(mismatch.code, "archive_manifest_mismatch");

        let missing_entry_archive = package_archive(&[("axiom.toml", manifest_bytes)]);
        let missing = validate_authenticated_archive_contract(
            &missing_entry_archive,
            &sha256_hex(&missing_entry_archive),
            manifest_bytes,
            &manifest,
        )
        .expect_err("missing build entry");
        assert_eq!(missing.code, "registry_build_entry_missing");
    }

    #[test]
    fn authenticated_registry_archive_rejects_workspace_and_non_ax_entry() {
        let workspace_bytes = br#"[package]
name = "demo"
version = "1.0.0"

[workspace]
members = []

[build]
entry = "src/main.ax"
out_dir = "dist"
"#;
        let workspace =
            parse_manifest_exact(workspace_bytes, Path::new("authenticated/axiom.toml"))
                .expect("parse workspace manifest");
        let archive = package_archive(&[
            ("axiom.toml", workspace_bytes),
            ("src/main.ax", b"print \"trusted\"\n"),
        ]);
        let error = validate_authenticated_archive_contract(
            &archive,
            &sha256_hex(&archive),
            workspace_bytes,
            &workspace,
        )
        .expect_err("registry workspace");
        assert_eq!(error.code, "registry_workspace_unsupported");

        let non_ax_bytes = br#"[package]
name = "demo"
version = "1.0.0"

[build]
entry = "README.md"
out_dir = "dist"
"#;
        let non_ax = parse_manifest_exact(non_ax_bytes, Path::new("authenticated/axiom.toml"))
            .expect("parse non-ax entry manifest");
        let archive =
            package_archive(&[("README.md", b"not source\n"), ("axiom.toml", non_ax_bytes)]);
        let error = validate_authenticated_archive_contract(
            &archive,
            &sha256_hex(&archive),
            non_ax_bytes,
            &non_ax,
        )
        .expect_err("non-ax build entry");
        assert_eq!(error.code, "registry_build_entry_invalid");
    }

    #[test]
    fn cache_commit_must_match_every_lock_digest() {
        let lockfile = fixture_lockfile();
        let package = lockfile
            .package
            .iter()
            .find(|package| package.registry.is_some())
            .expect("registry package");
        let registry = &lockfile.registry[0];
        let digest = package.archive_sha256.clone().expect("archive digest");
        let mut cached = CachedPackage {
            archive_sha256: digest.clone(),
            blob: PathBuf::from("/cache/blob"),
            tree: PathBuf::from("/cache/tree"),
            evidence: PathBuf::from("/cache/evidence"),
            integrity: crate::package_archive::TreeIntegrityManifest {
                schema_version: "axiom.package_tree_integrity.v1".to_owned(),
                extractor_version: "axiom-package-extractor-v1".to_owned(),
                archive_sha256: digest.clone(),
                files: Vec::new(),
            },
            commit: crate::package_store::CacheCommit {
                schema_version: "axiom.package_cache_commit.v1".to_owned(),
                extractor_version: "axiom-package-extractor-v1".to_owned(),
                archive_sha256: digest,
                archive_length: package.archive_length.expect("archive length"),
                tree_manifest_sha256: "7".repeat(64),
                manifest_sha256: package.manifest_sha256.clone().expect("manifest digest"),
                provenance_sha256: package
                    .provenance_sha256
                    .clone()
                    .expect("provenance digest"),
                signature_sha256: package
                    .package_signature_sha256
                    .clone()
                    .expect("signature digest"),
                registry_index_sha256: registry.index_sha256.clone(),
                verification_sha256: "6".repeat(64),
            },
            artifacts: crate::package_store::RehashedArtifacts {
                archive: Vec::new(),
                manifest: Vec::new(),
                provenance: Vec::new(),
                signature: Vec::new(),
                registry_index: Vec::new(),
                verification: Vec::new(),
            },
        };
        verify_cached_against_lock(&cached, package, registry).expect("exact cache hit");
        cached.commit.registry_index_sha256 = "0".repeat(64);
        let error =
            verify_cached_against_lock(&cached, package, registry).expect_err("reject drift");
        assert_eq!(error.code, "locked_package_mismatch");
    }

    #[test]
    fn vendor_manifest_must_exactly_equal_locked_registry_set() {
        let lockfile = fixture_lockfile();
        let package = lockfile
            .package
            .iter()
            .find(|package| package.registry.is_some())
            .expect("registry package");
        let snapshot = VendorSnapshot {
            digest: "1".repeat(64),
            root: PathBuf::from("/vendor/snapshot"),
            manifest: crate::package_store::VendorManifest {
                schema_version: "axiom.vendor_manifest.v1".to_owned(),
                packages: vec![crate::package_store::VendorManifestPackage {
                    package_id: package.id.clone(),
                    content_key: package.cache_key.clone().expect("content key"),
                    archive_sha256: package.archive_sha256.clone().expect("archive digest"),
                    registry_index_sha256: lockfile.registry[0].index_sha256.clone(),
                    verification_sha256: package
                        .verification_sha256
                        .clone()
                        .expect("verification digest"),
                    evidence_identity: "3".repeat(64),
                    tree_manifest_sha256: "2".repeat(64),
                }],
            },
            packages: BTreeMap::new(),
            lifecycle: VendorLifecycleEvidence::default(),
        };
        verify_vendor_against_lock(&snapshot, &lockfile).expect("exact vendor snapshot");
        let mut extra = snapshot;
        extra
            .manifest
            .packages
            .push(extra.manifest.packages.first().expect("package").clone());
        extra.manifest.packages[1].package_id = "registry:default/axiom/extra@1.0.0".to_owned();
        let error = verify_vendor_against_lock(&extra, &lockfile).expect_err("reject extra");
        assert_eq!(error.code, "vendor_lock_mismatch");
    }

    #[test]
    fn materialized_graph_exposes_exact_roots_edges_and_trust() {
        let lockfile = fixture_lockfile();
        let root = lockfile
            .package
            .iter()
            .find(|package| lockfile.roots.contains(&package.id))
            .expect("root package");
        let registry_package = lockfile
            .package
            .iter()
            .find(|package| package.registry.is_some())
            .expect("registry package");
        let roots = lockfile
            .package
            .iter()
            .map(|package| {
                let registry = package.registry.is_some();
                (
                    package.id.clone(),
                    MaterializedRoot {
                        path: if registry {
                            PathBuf::from("/cache/tree")
                        } else if package.id == root.id {
                            PathBuf::from("/project")
                        } else {
                            PathBuf::from("/project/tools/local")
                        },
                        source: if registry { "cache" } else { "path" }.to_owned(),
                        content_key: package.cache_key.clone(),
                        verified_archive: registry.then(|| Arc::from(Vec::<u8>::new())),
                        verified_manifest: registry.then(|| Arc::from(Vec::<u8>::new())),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let graph = materialized_graph(&lockfile, &"0".repeat(64), &roots, &lockfile.roots)
            .expect("materialized graph");
        assert_eq!(graph.roots, lockfile.roots);
        assert_eq!(graph.packages.len(), lockfile.package.len());
        assert!(graph.packages[0].trust.is_none());
        let registry_materialized = graph
            .packages
            .iter()
            .find(|package| package.id == registry_package.id)
            .expect("materialized registry package");
        assert!(registry_materialized.trust.is_some());
        let registry_edge = graph
            .edges
            .iter()
            .find(|edge| edge.to == registry_package.id)
            .expect("registry dependency edge");
        assert_eq!(registry_edge.reason, "highest_compatible");
        assert_eq!(registry_edge.requested, "^1.2.0");
    }

    #[test]
    fn local_path_dependencies_preserve_one_project_root_and_exact_edge() {
        let directory = tempfile::tempdir().expect("temporary project");
        let dependency = directory.path().join("deps/util");
        std::fs::create_dir_all(&dependency).expect("create dependency");
        std::fs::write(
            directory.path().join("axiom.toml"),
            r#"
[package]
name = "app"
version = "1.0.0"

[dependencies.util]
path = "deps/util"
version = "^0.4.0"
"#,
        )
        .expect("write root manifest");
        std::fs::write(
            dependency.join("axiom.toml"),
            r#"
[package]
name = "util"
version = "0.4.2"
"#,
        )
        .expect("write dependency manifest");
        let manifest = load_manifest(directory.path()).expect("load manifest");
        let registry = RegistryConfig {
            name: "default".to_owned(),
            index: "file:///registry/index.json".to_owned(),
            trust_roots: "trust/roots.json".to_owned(),
            expectation: "trust/expectation.json".to_owned(),
            cache: None,
            vendor: None,
        };
        let graph = collect_local_graph(
            directory.path(),
            &manifest,
            &registry,
            "registry:test-source",
        )
        .expect("collect local graph");
        assert_eq!(graph.roots, vec!["path:.#app@1.0.0"]);
        assert_eq!(
            graph
                .packages
                .iter()
                .map(|package| package.source.as_str())
                .collect::<Vec<_>>(),
            vec!["path", "path:deps/util"]
        );
        assert_eq!(graph.path_edges.len(), 1);
        assert_eq!(graph.path_edges[0].from, "path:.#app@1.0.0");
        assert_eq!(graph.path_edges[0].to, "path:deps/util#util@0.4.2");
        assert_eq!(
            graph.path_edges[0].reason,
            LockedDependencyReason::RootPathConstraint
        );
    }

    #[test]
    fn update_expectation_advances_from_previous_lock_without_rollback() {
        let mut lockfile = fixture_lockfile();
        let registry = &mut lockfile.registry[0];
        registry.current_root_version = 8;
        registry.current_root_sequence = 2_000;
        registry.current_root_transcript_sha256 = "a".repeat(64);
        registry.index_generation = 43;
        registry.index_sequence = 2_001;
        registry.index_snapshot_id = "snapshot-43".to_owned();
        registry.index_transcript_sha256 = "b".repeat(64);
        let configured = RegistryConfig {
            name: registry.name.clone(),
            index: registry.source.clone(),
            trust_roots: "roots.json".to_owned(),
            expectation: "expectation.json".to_owned(),
            cache: None,
            vendor: None,
        };
        let mut expectation = package_trust_expectation();
        let bootstrap_anchor = expectation.0["trusted_state"]["trusted_root_anchor"].clone();
        advance_expectation_from_lock(&mut expectation, &lockfile, &configured)
            .expect("advance expectation");
        assert_eq!(
            expectation.0["trusted_state"]["trusted_root_anchor"], bootstrap_anchor,
            "lock continuity must not replace the bootstrap trust anchor"
        );
        assert_eq!(
            expectation["trusted_state"]["highest_root_version"],
            serde_json::json!(8)
        );
        assert_eq!(
            expectation["trusted_state"]["highest_root_sequence"],
            serde_json::json!(2_000)
        );
        assert_eq!(
            expectation["trusted_state"]["highest_index_generation"],
            serde_json::json!(43)
        );
        assert!(
            expectation["trusted_state"]["seen_snapshots"]
                .as_array()
                .expect("seen snapshots")
                .iter()
                .any(|snapshot| snapshot["snapshot_id"] == "snapshot-43")
        );
    }

    #[test]
    fn update_rejects_simultaneous_trust_roots_and_policy_replacement() {
        let mut lockfile = fixture_lockfile();
        let registry = &mut lockfile.registry[0];
        let accepted_roots = b"accepted trust roots";
        let accepted_policy = b"accepted verification policy";
        registry.trust_roots_sha256 = sha256_hex(accepted_roots);
        registry.expectation_sha256 = sha256_hex(accepted_policy);
        registry.current_root_version += 100;
        registry.current_root_sequence += 100;
        registry.index_generation += 100;
        registry.index_sequence += 100;
        let configured = RegistryConfig {
            name: registry.name.clone(),
            index: registry.source.clone(),
            trust_roots: "roots.json".to_owned(),
            expectation: "expectation.json".to_owned(),
            cache: None,
            vendor: None,
        };
        validate_previous_trust_documents(&lockfile, &configured, accepted_roots, accepted_policy)
            .expect("exact prior trust documents");

        let error = validate_previous_trust_documents(
            &lockfile,
            &configured,
            b"malicious same-identity higher-counter roots",
            b"malicious replacement verification policy",
        )
        .expect_err("simultaneous trust reset must fail before authentication");
        assert_eq!(error.code, "trusted_roots_reset_rejected");
    }

    #[test]
    fn update_expectation_rejects_same_position_snapshot_overwrite() {
        let mut lockfile = fixture_lockfile();
        let registry = &mut lockfile.registry[0];
        let mut expectation = package_trust_expectation();
        let seen = expectation.0["trusted_state"]["seen_snapshots"]
            .as_array_mut()
            .expect("seen snapshots");
        let existing = seen.last().expect("existing snapshot").clone();
        registry.index_generation = existing["generation"].as_u64().expect("generation");
        registry.index_sequence = existing["sequence"].as_u64().expect("sequence");
        registry.index_snapshot_id = "rebound-snapshot".to_owned();
        registry.index_transcript_sha256 = "0".repeat(64);
        let configured = RegistryConfig {
            name: registry.name.clone(),
            index: registry.source.clone(),
            trust_roots: "roots.json".to_owned(),
            expectation: "expectation.json".to_owned(),
            cache: None,
            vendor: None,
        };
        let error = advance_expectation_from_lock(&mut expectation, &lockfile, &configured)
            .expect_err("reject rebound");
        assert_eq!(error.code, "update_snapshot_rebound");
    }

    #[test]
    fn targeted_freeze_rejects_same_version_artifact_overwrite() {
        let lockfile = fixture_lockfile();
        let package = lockfile
            .package
            .iter()
            .find(|package| package.registry.is_some())
            .expect("registry package");
        let locked = locked_frozen_identity(package).expect("locked identity");
        let mut overwritten = locked.clone();
        overwritten.archive_sha256 = "0".repeat(64);
        let key = PackageKey::new(
            package.registry.clone().expect("registry"),
            lockfile.registry[0].source_identity.clone(),
            package.namespace.clone().expect("namespace"),
            package.name.clone(),
        );
        let error =
            ensure_frozen_identity(&key, locked, overwritten).expect_err("reject overwrite");
        assert_eq!(error.code, "frozen_package_identity_changed");
    }

    #[test]
    fn path_only_v2_materializes_without_registry_or_store() {
        let directory = tempfile::tempdir().expect("temporary project");
        write_path_only_project(directory.path());
        write_lockfile_v2_atomic(directory.path(), &path_only_v2_lockfile())
            .expect("write path-only v2 lock");
        let manager = PackageManager::open(directory.path()).expect("open manager");
        let graph = manager
            .materialize_locked(MaterializeOptions::default())
            .expect("materialize path-only graph");
        assert_eq!(graph.roots, vec!["path:.#app@1.0.0"]);
        assert_eq!(graph.packages.len(), 1);
        assert_eq!(graph.packages[0].materialization.source, "path");
        assert!(
            !directory.path().join(DEFAULT_PACKAGE_CACHE_DIR).exists(),
            "path-only materialization must not open the package store"
        );
    }

    #[test]
    fn update_rewrites_removed_last_registry_dependency_without_transport() {
        let directory = tempfile::tempdir().expect("temporary project");
        write_path_only_project(directory.path());
        let mut stale = path_only_v2_lockfile();
        let fixture = fixture_lockfile();
        stale.registry = fixture.registry;
        let mut registry_package = fixture
            .package
            .into_iter()
            .find(|package| package.registry.is_some())
            .expect("registry package fixture");
        registry_package.compatibility = stale.compatibility.clone();
        let registry_id = registry_package.id.clone();
        stale.package.push(registry_package);
        stale.package.sort_by(|left, right| left.id.cmp(&right.id));
        stale.edge.push(LockedDependencyEdgeV2 {
            from: stale.roots[0].clone(),
            to: registry_id,
            alias: "obsolete".to_owned(),
            requested: "^1.0.0".to_owned(),
            source_kind: LockedDependencySourceKind::Registry,
            reason: LockedDependencyReason::HighestCompatible,
        });
        stale.edge.sort_by(lock_edge_order);
        write_lockfile_v2_atomic(directory.path(), &stale).expect("write stale registry lock");

        let manager = PackageManager::open(directory.path()).expect("open path-only project");
        let transport = RegistryClient::default();
        let report = manager
            .update(UpdateOptions::new(&transport))
            .expect("transport-free path-only update");
        assert!(!report.transport_used);
        assert_eq!(report.graph.lockfile_sha256.len(), 64);
        let ParsedLockfile::V2(rewritten) =
            load_lockfile(directory.path()).expect("reload path-only v2")
        else {
            panic!("update must write v2");
        };
        assert!(rewritten.registry.is_empty());
        assert_eq!(rewritten.package.len(), 1);
        assert!(rewritten.package[0].registry.is_none());
        assert!(rewritten.edge.is_empty());
    }

    #[test]
    fn path_only_fetch_rejects_dependency_cycle() {
        let directory = tempfile::tempdir().expect("temporary project");
        let dependency = directory.path().join("deps/b");
        std::fs::create_dir_all(&dependency).expect("create dependency");
        std::fs::write(
            directory.path().join("axiom.toml"),
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n\n\
             [dependencies.b]\npath = \"deps/b\"\n",
        )
        .expect("write root manifest");
        std::fs::write(
            dependency.join("axiom.toml"),
            "[package]\nname = \"b\"\nversion = \"1.0.0\"\n\n\
             [dependencies.a]\npath = \"../..\"\n",
        )
        .expect("write dependency manifest");
        let manager = PackageManager::open(directory.path()).expect("open manager");
        let transport = RegistryClient::default();
        let error = manager
            .fetch(FetchOptions::new(&transport))
            .expect_err("path cycle must fail");
        assert_eq!(error.code, "lockfile_invalid");
    }

    #[test]
    fn path_only_cas_rejects_concurrent_lock_replacement() {
        let directory = tempfile::tempdir().expect("temporary project");
        write_path_only_project(directory.path());
        let original = path_only_v2_lockfile();
        write_lockfile_v2_atomic(directory.path(), &original).expect("write original lock");
        let manager = PackageManager::open(directory.path()).expect("open manager");
        let previous = manager
            .optional_v2_lockfile()
            .expect("capture exact previous lock");
        let lock_path = lockfile_path(directory.path());
        let mut replaced = std::fs::read(&lock_path).expect("read lock");
        replaced.push(b'\n');
        std::fs::write(&lock_path, &replaced).expect("replace lock bytes");

        let error = manager
            .finish_path_only_operation(
                PackageOperation::Update,
                None,
                previous.expected_sha256.as_deref(),
            )
            .expect_err("CAS must reject concurrent replacement");
        assert_eq!(error.code, "lockfile_concurrent_change");
        assert_eq!(
            std::fs::read(&lock_path).expect("read retained replacement"),
            replaced
        );
    }

    #[test]
    fn vendor_override_cannot_collide_with_effective_cache_root() {
        let directory = tempfile::tempdir().expect("temporary project");
        write_path_only_project(directory.path());
        let mut manager = PackageManager::open(directory.path()).expect("open manager");
        manager.manifest.registry = Some(RegistryConfig {
            name: "fixture".to_owned(),
            index: "file:///registry/index.json".to_owned(),
            trust_roots: "trust/roots.json".to_owned(),
            expectation: "trust/expectation.json".to_owned(),
            cache: None,
            vendor: None,
        });
        let error = manager
            .vendor_root(Some(Path::new(DEFAULT_PACKAGE_CACHE_DIR)))
            .expect_err("vendor override must not alias cache");
        assert_eq!(error.code, "package_storage_path_collision");
    }

    #[test]
    fn locked_local_graph_rejects_extra_stale_registry_edge() {
        let directory = tempfile::tempdir().expect("temporary project");
        let local_tools = directory.path().join("tools/local");
        std::fs::create_dir_all(&local_tools).expect("create local tools");
        std::fs::write(
            directory.path().join("axiom.toml"),
            r#"
[package]
name = "resolver-demo"
version = "0.1.0"

[registry]
name = "fixture"
index = "file:///registry/index.json"
trust_roots = "trust/roots.json"
expectation = "trust/expectation.json"

[dependencies.core]
registry = "fixture"
namespace = "axiom"
package = "core"
version = "^1.2.0"

[dependencies.local-tools]
path = "tools/local"
"#,
        )
        .expect("write root manifest");
        std::fs::write(
            local_tools.join("axiom.toml"),
            "[package]\nname = \"local-tools\"\nversion = \"0.4.0\"\n",
        )
        .expect("write local manifest");
        let manifest = load_manifest(directory.path()).expect("load manifest");
        let mut lockfile = fixture_lockfile();
        let compatibility = current_compatibility().expect("current compatibility");
        lockfile.compatibility = compatibility.clone();
        for package in &mut lockfile.package {
            package.compatibility = compatibility.clone();
        }
        verify_locked_local_graph(directory.path(), &manifest, &lockfile)
            .expect("baseline exact local graph");
        let original = lockfile
            .edge
            .iter()
            .find(|edge| edge.source_kind == LockedDependencySourceKind::Registry)
            .expect("registry edge")
            .clone();
        let mut stale = original;
        stale.alias = "obsolete-core".to_owned();
        lockfile.edge.push(stale);
        lockfile.edge.sort_by(lock_edge_order);
        let error = verify_locked_local_graph(directory.path(), &manifest, &lockfile)
            .expect_err("extra stale registry edge must fail before materialization");
        assert_eq!(error.code, "locked_path_graph_mismatch");
    }

    #[test]
    fn v1_migration_rejects_stale_path_graph_and_accepts_exact_graph() {
        let directory = tempfile::tempdir().expect("temporary project");
        write_path_only_project(directory.path());
        let lock_path = lockfile_path(directory.path());
        std::fs::write(
            &lock_path,
            "version = 1\n\n[[package]]\nname = \"app\"\nversion = \"0.9.0\"\nsource = \"path\"\n",
        )
        .expect("write stale v1 lock");
        let manager = PackageManager::open(directory.path()).expect("open manager");
        let error = manager
            .optional_v2_lockfile()
            .expect_err("stale v1 must fail closed");
        assert_eq!(error.code, "stale_v1_lockfile");
        std::fs::write(
            &lock_path,
            "version = 1\n\n[[package]]\nname = \"app\"\nversion = \"1.0.0\"\nsource = \"path\"\n",
        )
        .expect("write exact v1 lock");
        assert!(
            manager
                .optional_v2_lockfile()
                .expect("exact v1 migration boundary")
                .lockfile
                .is_none()
        );
    }

    #[test]
    fn resolver_failures_preserve_structured_trace_and_payload() {
        let trace = vec![TraceEvent::PathPreserved {
            from: None,
            alias: "local".to_owned(),
            path: "deps/local".to_owned(),
        }];
        let error = map_resolve_error(
            ResolveError::BudgetExceeded {
                budget: "candidate_attempts",
                limit: 1,
                trace: trace.clone(),
            },
            false,
        );
        assert_eq!(error.code, "resolution_budget_exceeded");
        assert_eq!(error.trace, trace);
        assert_eq!(
            error.resolver.as_ref().expect("resolver payload")["kind"],
            "budget_exceeded"
        );
    }

    #[test]
    fn authenticated_archive_and_cumulative_download_budgets_are_bounded() {
        assert_eq!(
            validate_authenticated_archive_length(MAX_PACKAGE_ARCHIVE_BODY_BYTES as u64)
                .expect("maximum archive"),
            MAX_PACKAGE_ARCHIVE_BODY_BYTES
        );
        assert_eq!(
            validate_authenticated_archive_length(MAX_PACKAGE_ARCHIVE_BODY_BYTES as u64 + 1)
                .expect_err("oversized archive")
                .code,
            "archive_length_out_of_bounds"
        );
        assert_eq!(
            charged_candidate_bytes(MAX_CANDIDATE_DOWNLOAD_BYTES - 1, 1)
                .expect("exact operation budget"),
            MAX_CANDIDATE_DOWNLOAD_BYTES
        );
        assert_eq!(
            charged_candidate_bytes(MAX_CANDIDATE_DOWNLOAD_BYTES, 1)
                .expect_err("operation budget exceeded")
                .code,
            "candidate_download_budget_exceeded"
        );
    }

    #[cfg(unix)]
    #[test]
    fn bounded_trust_file_reader_rejects_symlinks_at_open() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("temporary directory");
        let target = directory.path().join("expectation.json");
        let link = directory.path().join("linked-expectation.json");
        std::fs::write(&target, b"{}").expect("write target");
        symlink(&target, &link).expect("create symlink");

        let error = read_bounded_file(&link, 16).expect_err("symlink must be rejected");
        assert!(
            error.contains("store_file_unavailable"),
            "unexpected error: {error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn bounded_trust_file_reader_rejects_fifo_without_blocking() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::time::{Duration, Instant};

        let directory = tempfile::tempdir().expect("temporary directory");
        let fifo = directory.path().join("expectation.fifo");
        let fifo_c = CString::new(fifo.as_os_str().as_bytes()).expect("fifo path");
        let result = unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) };
        assert_eq!(
            result,
            0,
            "create FIFO: {}",
            std::io::Error::last_os_error()
        );

        let started = Instant::now();
        let error = read_bounded_file(&fifo, 16).expect_err("FIFO must be rejected");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "FIFO validation blocked instead of failing closed"
        );
        assert!(
            error.contains("store_file_invalid"),
            "unexpected error: {error}"
        );
    }
}
