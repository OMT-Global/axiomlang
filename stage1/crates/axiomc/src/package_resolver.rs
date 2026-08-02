//! Deterministic, bounded, single-version package resolution.
//!
//! The source boundary deliberately separates authenticated catalog metadata
//! from verified release manifests. Resolver graph expansion can only observe
//! dependencies returned by `verify_candidate`.

use crate::package_version::{ReleaseVersion, VersionRequirement};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PackageKey {
    pub registry: String,
    pub source: String,
    pub namespace: String,
    pub name: String,
}

impl PackageKey {
    pub fn new(
        registry: impl Into<String>,
        source: impl Into<String>,
        namespace: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self {
            registry: registry.into(),
            source: source.into(),
            namespace: namespace.into(),
            name: name.into(),
        }
    }
}

impl fmt::Display for PackageKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}@{}::{}/{}",
            self.registry, self.source, self.namespace, self.name
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Dependency {
    Registry(RegistryDependency),
    Path(PathDependency),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryDependency {
    pub alias: String,
    pub package: PackageKey,
    pub requirement: VersionRequirement,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PathDependency {
    pub alias: String,
    pub path: String,
    pub version: Option<ReleaseVersion>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogCandidate {
    pub version: ReleaseVersion,
    pub yanked: bool,
    /// Stable authenticated release identity, normally a target path or digest.
    pub release_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticatedCatalog {
    package: PackageKey,
    candidates: Vec<CatalogCandidate>,
    authentication_id: String,
}

impl AuthenticatedCatalog {
    pub fn new(
        package: PackageKey,
        candidates: Vec<CatalogCandidate>,
        authentication_id: impl Into<String>,
    ) -> Self {
        Self {
            package,
            candidates,
            authentication_id: authentication_id.into(),
        }
    }

    pub fn package(&self) -> &PackageKey {
        &self.package
    }

    pub fn candidates(&self) -> &[CatalogCandidate] {
        &self.candidates
    }

    pub fn authentication_id(&self) -> &str {
        &self.authentication_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedCandidate {
    pub package: PackageKey,
    pub version: ReleaseVersion,
    pub release_id: String,
    pub dependencies: Vec<Dependency>,
    pub manifest_digest: String,
    pub signer_key_ids: Vec<String>,
    pub edition: String,
    pub compatibility: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFailure {
    pub code: String,
    pub message: String,
}

impl SourceFailure {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for SourceFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SourceFailure {}

pub trait ResolverSource {
    /// Authenticate the registry index/catalog before exposing release rows.
    fn authenticate_catalog(
        &mut self,
        package: &PackageKey,
    ) -> Result<AuthenticatedCatalog, SourceFailure>;

    /// Fetch and fully Package-Trust-verify exact candidate artifacts, then
    /// return the dependency manifest obtained from those verified bytes.
    fn verify_candidate(
        &mut self,
        catalog: &AuthenticatedCatalog,
        candidate: &CatalogCandidate,
    ) -> Result<VerifiedCandidate, SourceFailure>;

    /// Release retained bytes for a candidate rejected after verification.
    /// Sources that retain verified artifacts can override this to keep
    /// backtracking within the live materialization budget.
    fn discard_candidate(&mut self, _candidate: &CatalogCandidate) {}
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockedSelection {
    pub version: ReleaseVersion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolverLimits {
    pub max_packages: usize,
    pub max_catalog_candidates: usize,
    pub max_candidate_attempts: usize,
    pub max_backtracks: usize,
    pub max_trace_events: usize,
}

impl Default for ResolverLimits {
    fn default() -> Self {
        Self {
            max_packages: 256,
            max_catalog_candidates: 16_384,
            max_candidate_attempts: 16_384,
            max_backtracks: 8_192,
            max_trace_events: 65_536,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionMode {
    Fresh,
    Locked,
    Update,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveRequest {
    pub dependencies: Vec<Dependency>,
    pub locked: BTreeMap<PackageKey, LockedSelection>,
    /// Exact selections that a targeted update must retain. These constrain
    /// selection without adding synthetic graph edges.
    pub frozen: BTreeMap<PackageKey, LockedSelection>,
    pub mode: ResolutionMode,
    pub limits: ResolverLimits,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    pub schema_version: String,
    pub packages: Vec<ResolvedPackage>,
    pub edges: Vec<ResolvedEdge>,
    pub path_dependencies: Vec<PreservedPathDependency>,
    pub trace: Vec<TraceEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedPackage {
    pub package: PackageKey,
    pub version: ReleaseVersion,
    pub release_id: String,
    pub manifest_digest: String,
    pub signer_key_ids: Vec<String>,
    pub edition: String,
    pub compatibility: String,
    pub yanked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResolvedEdge {
    pub from: Option<PackageKey>,
    pub alias: String,
    pub to: PackageKey,
    pub requirement: VersionRequirement,
    pub selected: ReleaseVersion,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PreservedPathDependency {
    pub from: Option<PackageKey>,
    pub alias: String,
    pub path: String,
    pub version: Option<ReleaseVersion>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TraceEvent {
    PathPreserved {
        from: Option<PackageKey>,
        alias: String,
        path: String,
    },
    ConstraintAdded {
        from: Option<PackageKey>,
        package: PackageKey,
        requirement: VersionRequirement,
    },
    CatalogAuthenticated {
        package: PackageKey,
        authentication_id: String,
        candidate_count: usize,
    },
    CandidateConsidered {
        package: PackageKey,
        version: ReleaseVersion,
        release_id: String,
    },
    CandidateRejected {
        package: PackageKey,
        version: ReleaseVersion,
        reason: CandidateRejection,
    },
    CandidateVerified {
        package: PackageKey,
        version: ReleaseVersion,
        manifest_digest: String,
    },
    Selected {
        package: PackageKey,
        version: ReleaseVersion,
    },
    Backtracked {
        package: PackageKey,
        version: ReleaseVersion,
    },
    Conflict {
        package: PackageKey,
        requirements: Vec<VersionRequirement>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum CandidateRejection {
    ConstraintMismatch,
    FrozenMismatch { required: ReleaseVersion },
    Yanked,
    VerificationFailed { code: String },
    IdentityMismatch,
    Cycle { through: PackageKey },
    DownstreamConflict,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResolveError {
    InvalidRequest(String),
    InvalidCatalog {
        package: Box<PackageKey>,
        message: String,
    },
    Source {
        package: Box<PackageKey>,
        failure: Box<SourceFailure>,
        trace: Vec<TraceEvent>,
    },
    Conflict {
        package: Box<PackageKey>,
        requirements: Vec<VersionRequirement>,
        trace: Vec<TraceEvent>,
    },
    BudgetExceeded {
        budget: &'static str,
        limit: usize,
        trace: Vec<TraceEvent>,
    },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(message) => formatter.write_str(message),
            Self::InvalidCatalog { package, message } => {
                write!(
                    formatter,
                    "invalid authenticated catalog for {package}: {message}"
                )
            }
            Self::Source {
                package, failure, ..
            } => {
                write!(formatter, "package source failed for {package}: {failure}")
            }
            Self::Conflict {
                package,
                requirements,
                ..
            } => write!(
                formatter,
                "no single version of {package} satisfies {}",
                requirements
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::BudgetExceeded { budget, limit, .. } => {
                write!(formatter, "resolver {budget} budget exceeded ({limit})")
            }
        }
    }
}

impl std::error::Error for ResolveError {}

#[derive(Clone, Debug)]
struct Constraint {
    from: Option<PackageKey>,
    alias: String,
    requirement: VersionRequirement,
}

#[derive(Clone, Debug)]
struct Selection {
    candidate: CatalogCandidate,
    verified: VerifiedCandidate,
}

#[derive(Clone, Debug, Default)]
struct SolverState {
    constraints: BTreeMap<PackageKey, Vec<Constraint>>,
    selected: BTreeMap<PackageKey, Selection>,
    paths: BTreeSet<PreservedPathDependency>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CandidateCacheKey {
    package: PackageKey,
    version: ReleaseVersion,
    release_id: String,
}

struct Solver<'a, S: ResolverSource> {
    source: &'a mut S,
    request: ResolveRequest,
    catalogs: BTreeMap<PackageKey, AuthenticatedCatalog>,
    verified: BTreeMap<CandidateCacheKey, Result<VerifiedCandidate, SourceFailure>>,
    trace: Vec<TraceEvent>,
    catalog_candidate_count: usize,
    candidate_attempts: usize,
    backtracks: usize,
    last_conflict: Option<(PackageKey, Vec<VersionRequirement>)>,
}

pub fn resolve_packages<S: ResolverSource>(
    source: &mut S,
    request: ResolveRequest,
) -> Result<Resolution, ResolveError> {
    validate_limits(request.limits)?;
    let mut solver = Solver {
        source,
        request,
        catalogs: BTreeMap::new(),
        verified: BTreeMap::new(),
        trace: Vec::new(),
        catalog_candidate_count: 0,
        candidate_attempts: 0,
        backtracks: 0,
        last_conflict: None,
    };
    let mut state = SolverState::default();
    let mut dependencies = solver.request.dependencies.clone();
    dependencies.sort_by(dependency_order);
    for dependency in dependencies {
        solver.add_dependency(&mut state, None, dependency)?;
    }
    let first = state.constraints.keys().next().cloned();
    match solver.solve(state)? {
        Some(state) => Ok(solver.finish(state)),
        None => {
            let (package, requirements) = solver.last_conflict.clone().unwrap_or_else(|| {
                (
                    first.unwrap_or_else(|| PackageKey::new("", "", "", "<root>")),
                    Vec::new(),
                )
            });
            solver.push_trace(TraceEvent::Conflict {
                package: package.clone(),
                requirements: requirements.clone(),
            })?;
            Err(ResolveError::Conflict {
                package: Box::new(package),
                requirements,
                trace: solver.trace,
            })
        }
    }
}

fn validate_limits(limits: ResolverLimits) -> Result<(), ResolveError> {
    if limits.max_packages == 0
        || limits.max_catalog_candidates == 0
        || limits.max_candidate_attempts == 0
        || limits.max_backtracks == 0
        || limits.max_trace_events == 0
    {
        return Err(ResolveError::InvalidRequest(
            "resolver budgets must all be positive".to_owned(),
        ));
    }
    Ok(())
}

fn dependency_order(left: &Dependency, right: &Dependency) -> std::cmp::Ordering {
    dependency_key(left).cmp(&dependency_key(right))
}

fn dependency_key(dependency: &Dependency) -> (u8, String, String) {
    match dependency {
        Dependency::Registry(dependency) => {
            (0, dependency.package.to_string(), dependency.alias.clone())
        }
        Dependency::Path(dependency) => (1, dependency.path.clone(), dependency.alias.clone()),
    }
}

impl<S: ResolverSource> Solver<'_, S> {
    fn push_trace(&mut self, event: TraceEvent) -> Result<(), ResolveError> {
        if self.trace.len() >= self.request.limits.max_trace_events {
            return Err(self.budget_error("trace events", self.request.limits.max_trace_events));
        }
        self.trace.push(event);
        Ok(())
    }

    fn budget_error(&self, budget: &'static str, limit: usize) -> ResolveError {
        ResolveError::BudgetExceeded {
            budget,
            limit,
            trace: self.trace.clone(),
        }
    }

    fn add_dependency(
        &mut self,
        state: &mut SolverState,
        from: Option<PackageKey>,
        dependency: Dependency,
    ) -> Result<(), ResolveError> {
        match dependency {
            Dependency::Path(dependency) => {
                if dependency.alias.is_empty()
                    || dependency.path.is_empty()
                    || dependency.path.contains('\0')
                {
                    return Err(ResolveError::InvalidRequest(
                        "path dependency alias and path must be non-empty and NUL-free".to_owned(),
                    ));
                }
                state.paths.insert(PreservedPathDependency {
                    from: from.clone(),
                    alias: dependency.alias.clone(),
                    path: dependency.path.clone(),
                    version: dependency.version,
                });
                self.push_trace(TraceEvent::PathPreserved {
                    from,
                    alias: dependency.alias,
                    path: dependency.path,
                })
            }
            Dependency::Registry(dependency) => {
                if dependency.alias.is_empty()
                    || dependency.package.registry.is_empty()
                    || dependency.package.source.is_empty()
                    || dependency.package.namespace.is_empty()
                    || dependency.package.name.is_empty()
                {
                    return Err(ResolveError::InvalidRequest(
                        "registry dependency coordinates and alias must be non-empty".to_owned(),
                    ));
                }
                if !state.constraints.contains_key(&dependency.package)
                    && state.constraints.len() >= self.request.limits.max_packages
                {
                    return Err(self.budget_error("packages", self.request.limits.max_packages));
                }
                state
                    .constraints
                    .entry(dependency.package.clone())
                    .or_default()
                    .push(Constraint {
                        from: from.clone(),
                        alias: dependency.alias,
                        requirement: dependency.requirement,
                    });
                self.push_trace(TraceEvent::ConstraintAdded {
                    from,
                    package: dependency.package,
                    requirement: dependency.requirement,
                })
            }
        }
    }

    fn solve(&mut self, state: SolverState) -> Result<Option<SolverState>, ResolveError> {
        let selected_conflict = state.selected.iter().find_map(|(package, selection)| {
            state.constraints.get(package).and_then(|constraints| {
                constraints
                    .iter()
                    .any(|constraint| !constraint.requirement.matches(selection.candidate.version))
                    .then(|| package.clone())
            })
        });
        if let Some(package) = selected_conflict {
            let requirements = self.requirements_for(&package, Some(&state));
            self.last_conflict = Some((package.clone(), requirements.clone()));
            self.push_trace(TraceEvent::Conflict {
                package,
                requirements,
            })?;
            return Ok(None);
        }
        let Some(package) = state
            .constraints
            .keys()
            .find(|package| !state.selected.contains_key(*package))
            .cloned()
        else {
            return Ok(Some(state));
        };
        let requirements = self.requirements_for(&package, Some(&state));
        let catalog = self.catalog(&package)?;
        let mut candidates = catalog.candidates.clone();
        candidates.sort_by(|left, right| {
            right
                .version
                .cmp(&left.version)
                .then(left.release_id.cmp(&right.release_id))
        });
        if self.request.mode == ResolutionMode::Locked
            && let Some(locked) = self.request.locked.get(&package)
        {
            candidates.sort_by_key(|candidate| candidate.version != locked.version);
        }
        let mut attempted = false;
        for candidate in candidates {
            self.bump_candidate_attempts()?;
            self.push_trace(TraceEvent::CandidateConsidered {
                package: package.clone(),
                version: candidate.version,
                release_id: candidate.release_id.clone(),
            })?;
            if let Some(frozen) = self.request.frozen.get(&package)
                && candidate.version != frozen.version
            {
                self.push_trace(TraceEvent::CandidateRejected {
                    package: package.clone(),
                    version: candidate.version,
                    reason: CandidateRejection::FrozenMismatch {
                        required: frozen.version,
                    },
                })?;
                continue;
            }
            if !requirements
                .iter()
                .all(|requirement| requirement.matches(candidate.version))
            {
                self.push_trace(TraceEvent::CandidateRejected {
                    package: package.clone(),
                    version: candidate.version,
                    reason: CandidateRejection::ConstraintMismatch,
                })?;
                continue;
            }
            let exact_locked_yank_replay = match self.request.mode {
                ResolutionMode::Fresh => false,
                ResolutionMode::Locked => {
                    self.request
                        .locked
                        .get(&package)
                        .is_some_and(|locked| locked.version == candidate.version)
                        || self
                            .request
                            .frozen
                            .get(&package)
                            .is_some_and(|locked| locked.version == candidate.version)
                }
                ResolutionMode::Update => self
                    .request
                    .frozen
                    .get(&package)
                    .is_some_and(|locked| locked.version == candidate.version),
            };
            if candidate.yanked && !exact_locked_yank_replay {
                self.push_trace(TraceEvent::CandidateRejected {
                    package: package.clone(),
                    version: candidate.version,
                    reason: CandidateRejection::Yanked,
                })?;
                continue;
            }
            attempted = true;
            let verified = match self.verified_candidate(&catalog, &candidate)? {
                Ok(verified) => verified,
                Err(failure) => {
                    self.push_trace(TraceEvent::CandidateRejected {
                        package: package.clone(),
                        version: candidate.version,
                        reason: CandidateRejection::VerificationFailed {
                            code: failure.code.clone(),
                        },
                    })?;
                    return Err(ResolveError::Source {
                        package: Box::new(package),
                        failure: Box::new(failure),
                        trace: self.trace.clone(),
                    });
                }
            };
            if verified.package != package
                || verified.version != candidate.version
                || verified.release_id != candidate.release_id
            {
                self.push_trace(TraceEvent::CandidateRejected {
                    package: package.clone(),
                    version: candidate.version,
                    reason: CandidateRejection::IdentityMismatch,
                })?;
                self.source.discard_candidate(&candidate);
                continue;
            }
            self.push_trace(TraceEvent::CandidateVerified {
                package: package.clone(),
                version: candidate.version,
                manifest_digest: verified.manifest_digest.clone(),
            })?;

            let mut next = state.clone();
            next.selected.insert(
                package.clone(),
                Selection {
                    candidate: candidate.clone(),
                    verified: verified.clone(),
                },
            );
            let mut dependencies = verified.dependencies.clone();
            dependencies.sort_by(dependency_order);
            let mut cycle = None;
            for dependency in dependencies {
                if let Dependency::Registry(registry) = &dependency
                    && dependency_path_exists(&next, &registry.package, &package)
                {
                    cycle = Some(registry.package.clone());
                    break;
                }
                self.add_dependency(&mut next, Some(package.clone()), dependency)?;
            }
            if let Some(through) = cycle {
                self.push_trace(TraceEvent::CandidateRejected {
                    package: package.clone(),
                    version: candidate.version,
                    reason: CandidateRejection::Cycle { through },
                })?;
                self.source.discard_candidate(&candidate);
                self.backtrack(&package, candidate.version)?;
                continue;
            }
            if let Some(solution) = self.solve(next)? {
                self.push_trace(TraceEvent::Selected {
                    package,
                    version: candidate.version,
                })?;
                return Ok(Some(solution));
            }
            self.push_trace(TraceEvent::CandidateRejected {
                package: package.clone(),
                version: candidate.version,
                reason: CandidateRejection::DownstreamConflict,
            })?;
            self.source.discard_candidate(&candidate);
            self.backtrack(&package, candidate.version)?;
        }
        if attempted || !catalog.candidates.is_empty() {
            self.last_conflict = Some((package.clone(), requirements.clone()));
            self.push_trace(TraceEvent::Conflict {
                package,
                requirements,
            })?;
        }
        Ok(None)
    }

    fn backtrack(
        &mut self,
        package: &PackageKey,
        version: ReleaseVersion,
    ) -> Result<(), ResolveError> {
        if self.backtracks >= self.request.limits.max_backtracks {
            return Err(self.budget_error("backtracks", self.request.limits.max_backtracks));
        }
        self.backtracks += 1;
        self.push_trace(TraceEvent::Backtracked {
            package: package.clone(),
            version,
        })
    }

    fn bump_candidate_attempts(&mut self) -> Result<(), ResolveError> {
        if self.candidate_attempts >= self.request.limits.max_candidate_attempts {
            return Err(self.budget_error(
                "candidate attempts",
                self.request.limits.max_candidate_attempts,
            ));
        }
        self.candidate_attempts += 1;
        Ok(())
    }

    fn catalog(&mut self, package: &PackageKey) -> Result<AuthenticatedCatalog, ResolveError> {
        if let Some(catalog) = self.catalogs.get(package) {
            return Ok(catalog.clone());
        }
        let catalog = self
            .source
            .authenticate_catalog(package)
            .map_err(|failure| ResolveError::Source {
                package: Box::new(package.clone()),
                failure: Box::new(failure),
                trace: self.trace.clone(),
            })?;
        if catalog.package != *package {
            return Err(ResolveError::InvalidCatalog {
                package: Box::new(package.clone()),
                message: format!("catalog is bound to {}, not {package}", catalog.package),
            });
        }
        if catalog.authentication_id.is_empty() {
            return Err(ResolveError::InvalidCatalog {
                package: Box::new(package.clone()),
                message: "catalog authentication identity must be non-empty".to_owned(),
            });
        }
        let mut versions = BTreeSet::new();
        for candidate in &catalog.candidates {
            if candidate.release_id.is_empty() || !versions.insert(candidate.version) {
                return Err(ResolveError::InvalidCatalog {
                    package: Box::new(package.clone()),
                    message: "release identities must be non-empty and versions unique".to_owned(),
                });
            }
        }
        self.catalog_candidate_count = self
            .catalog_candidate_count
            .checked_add(catalog.candidates.len())
            .ok_or_else(|| {
                self.budget_error(
                    "catalog candidates",
                    self.request.limits.max_catalog_candidates,
                )
            })?;
        if self.catalog_candidate_count > self.request.limits.max_catalog_candidates {
            return Err(self.budget_error(
                "catalog candidates",
                self.request.limits.max_catalog_candidates,
            ));
        }
        self.push_trace(TraceEvent::CatalogAuthenticated {
            package: package.clone(),
            authentication_id: catalog.authentication_id.clone(),
            candidate_count: catalog.candidates.len(),
        })?;
        self.catalogs.insert(package.clone(), catalog.clone());
        Ok(catalog)
    }

    fn verified_candidate(
        &mut self,
        catalog: &AuthenticatedCatalog,
        candidate: &CatalogCandidate,
    ) -> Result<Result<VerifiedCandidate, SourceFailure>, ResolveError> {
        let key = CandidateCacheKey {
            package: catalog.package.clone(),
            version: candidate.version,
            release_id: candidate.release_id.clone(),
        };
        if let Some(cached) = self.verified.get(&key) {
            return Ok(cached.clone());
        }
        let result = self.source.verify_candidate(catalog, candidate);
        self.verified.insert(key, result.clone());
        Ok(result)
    }

    fn requirements_for(
        &self,
        package: &PackageKey,
        state: Option<&SolverState>,
    ) -> Vec<VersionRequirement> {
        let constraints = state.and_then(|state| state.constraints.get(package));
        let mut requirements = constraints
            .into_iter()
            .flatten()
            .map(|constraint| constraint.requirement)
            .collect::<Vec<_>>();
        requirements.sort_by_key(ToString::to_string);
        requirements.dedup();
        requirements
    }

    fn finish(self, state: SolverState) -> Resolution {
        let packages = state
            .selected
            .iter()
            .map(|(package, selection)| {
                let mut signer_key_ids = selection.verified.signer_key_ids.clone();
                signer_key_ids.sort();
                signer_key_ids.dedup();
                ResolvedPackage {
                    package: package.clone(),
                    version: selection.candidate.version,
                    release_id: selection.candidate.release_id.clone(),
                    manifest_digest: selection.verified.manifest_digest.clone(),
                    signer_key_ids,
                    edition: selection.verified.edition.clone(),
                    compatibility: selection.verified.compatibility.clone(),
                    yanked: selection.candidate.yanked,
                }
            })
            .collect();
        let mut edges = state
            .constraints
            .iter()
            .flat_map(|(package, constraints)| {
                let selected = state.selected[package].candidate.version;
                constraints.iter().map(move |constraint| ResolvedEdge {
                    from: constraint.from.clone(),
                    alias: constraint.alias.clone(),
                    to: package.clone(),
                    requirement: constraint.requirement,
                    selected,
                })
            })
            .collect::<Vec<_>>();
        edges.sort();
        Resolution {
            schema_version: "axiom.package_resolution.v1".to_owned(),
            packages,
            edges,
            path_dependencies: state.paths.into_iter().collect(),
            trace: self.trace,
        }
    }
}

fn dependency_path_exists(state: &SolverState, start: &PackageKey, goal: &PackageKey) -> bool {
    if start == goal {
        return true;
    }
    let mut pending = vec![start.clone()];
    let mut visited = BTreeSet::new();
    while let Some(package) = pending.pop() {
        if !visited.insert(package.clone()) {
            continue;
        }
        for (target, constraints) in &state.constraints {
            if constraints
                .iter()
                .any(|constraint| constraint.from.as_ref() == Some(&package))
            {
                if target == goal {
                    return true;
                }
                pending.push(target.clone());
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MockSource {
        catalogs: BTreeMap<PackageKey, AuthenticatedCatalog>,
        verified: BTreeMap<(PackageKey, ReleaseVersion), Result<VerifiedCandidate, SourceFailure>>,
        calls: Vec<String>,
    }

    impl ResolverSource for MockSource {
        fn authenticate_catalog(
            &mut self,
            package: &PackageKey,
        ) -> Result<AuthenticatedCatalog, SourceFailure> {
            self.calls.push(format!("auth:{package}"));
            self.catalogs
                .get(package)
                .cloned()
                .ok_or_else(|| SourceFailure::new("missing_catalog", package.to_string()))
        }

        fn verify_candidate(
            &mut self,
            catalog: &AuthenticatedCatalog,
            candidate: &CatalogCandidate,
        ) -> Result<VerifiedCandidate, SourceFailure> {
            self.calls
                .push(format!("verify:{}@{}", catalog.package, candidate.version));
            self.verified
                .get(&(catalog.package.clone(), candidate.version))
                .cloned()
                .unwrap_or_else(|| {
                    Err(SourceFailure::new(
                        "missing_release",
                        candidate.release_id.clone(),
                    ))
                })
        }
    }

    fn key(name: &str) -> PackageKey {
        PackageKey::new("default", "file:///registry/index.json", "axiom", name)
    }

    fn requirement(value: &str) -> VersionRequirement {
        VersionRequirement::parse(value).expect("requirement fixture")
    }

    fn version(value: &str) -> ReleaseVersion {
        ReleaseVersion::parse(value).expect("version fixture")
    }

    fn registry(alias: &str, name: &str, required: &str) -> Dependency {
        Dependency::Registry(RegistryDependency {
            alias: alias.to_owned(),
            package: key(name),
            requirement: requirement(required),
        })
    }

    fn add_package(
        source: &mut MockSource,
        name: &str,
        releases: &[(&str, bool, Vec<Dependency>)],
    ) {
        let package = key(name);
        let candidates = releases
            .iter()
            .map(|(release, yanked, _)| CatalogCandidate {
                version: version(release),
                yanked: *yanked,
                release_id: format!("{name}-{release}"),
            })
            .collect::<Vec<_>>();
        source.catalogs.insert(
            package.clone(),
            AuthenticatedCatalog::new(package.clone(), candidates, format!("auth-{name}")),
        );
        for (release, _, dependencies) in releases {
            let release = version(release);
            source.verified.insert(
                (package.clone(), release),
                Ok(VerifiedCandidate {
                    package: package.clone(),
                    version: release,
                    release_id: format!("{name}-{release}"),
                    dependencies: dependencies.clone(),
                    manifest_digest: format!("digest-{name}-{release}"),
                    signer_key_ids: vec!["signer".to_owned()],
                    edition: "2026".to_owned(),
                    compatibility: "axiom-v1".to_owned(),
                }),
            );
        }
    }

    fn request(dependencies: Vec<Dependency>) -> ResolveRequest {
        ResolveRequest {
            dependencies,
            locked: BTreeMap::new(),
            frozen: BTreeMap::new(),
            mode: ResolutionMode::Fresh,
            limits: ResolverLimits::default(),
        }
    }

    #[test]
    fn exact_and_caret_choose_highest_compatible_release() {
        let mut source = MockSource::default();
        add_package(
            &mut source,
            "a",
            &[
                ("1.0.0", false, vec![]),
                ("1.5.0", false, vec![]),
                ("2.0.0", false, vec![]),
            ],
        );
        let exact = resolve_packages(&mut source, request(vec![registry("a", "a", "1.0.0")]))
            .expect("exact resolution");
        assert_eq!(exact.packages[0].version, version("1.0.0"));

        let caret = resolve_packages(&mut source, request(vec![registry("a", "a", "^1.0.0")]))
            .expect("caret resolution");
        assert_eq!(caret.packages[0].version, version("1.5.0"));
    }

    #[test]
    fn diamond_resolution_uses_one_version_and_backtracks_deterministically() {
        let mut source = MockSource::default();
        add_package(
            &mut source,
            "a",
            &[
                ("1.9.0", false, vec![registry("c", "c", "^1.0.0")]),
                ("1.0.0", false, vec![registry("c", "c", "^1.0.0")]),
            ],
        );
        add_package(
            &mut source,
            "b",
            &[("1.0.0", false, vec![registry("c", "c", "^1.0.0")])],
        );
        add_package(
            &mut source,
            "c",
            &[
                ("2.1.0", false, vec![]),
                ("1.9.0", false, vec![]),
                ("1.2.0", false, vec![]),
            ],
        );
        let resolution = resolve_packages(
            &mut source,
            request(vec![
                registry("a", "a", "^1.0.0"),
                registry("b", "b", "1.0.0"),
            ]),
        )
        .expect("diamond resolves");
        assert_eq!(
            resolution
                .packages
                .iter()
                .find(|package| package.package == key("c"))
                .expect("c selected")
                .version,
            version("1.9.0")
        );
        assert_eq!(
            resolution
                .packages
                .iter()
                .find(|package| package.package == key("a"))
                .expect("a selected")
                .version,
            version("1.9.0")
        );
    }

    #[test]
    fn eligible_newest_candidate_verification_failure_is_fatal() {
        let mut source = MockSource::default();
        add_package(
            &mut source,
            "a",
            &[("1.2.0", false, vec![]), ("1.1.0", false, vec![])],
        );
        source.verified.insert(
            (key("a"), version("1.2.0")),
            Err(SourceFailure::new(
                "archive_manifest_mismatch",
                "newest eligible release embeds a different manifest",
            )),
        );
        let error = resolve_packages(&mut source, request(vec![registry("a", "a", "^1.0.0")]))
            .expect_err("tampered newest candidate must fail closed");
        match error {
            ResolveError::Source {
                package,
                failure,
                trace,
            } => {
                assert_eq!(*package, key("a"));
                assert_eq!(failure.code, "archive_manifest_mismatch");
                assert!(trace.iter().any(|event| matches!(
                    event,
                    TraceEvent::CandidateRejected {
                        package,
                        version: rejected,
                        reason: CandidateRejection::VerificationFailed { code },
                    } if package == &key("a")
                        && *rejected == version("1.2.0")
                        && code == "archive_manifest_mismatch"
                )));
            }
            other => panic!("unexpected error {other:?}"),
        }
        assert!(
            !source
                .calls
                .iter()
                .any(|call| call.starts_with("verify:") && call.ends_with("@1.1.0")),
            "resolver must not downgrade after a trust failure"
        );
    }

    #[test]
    fn downstream_conflict_backtracks_from_highest_candidate() {
        let mut source = MockSource::default();
        add_package(
            &mut source,
            "a",
            &[
                ("1.9.0", false, vec![registry("c", "c", "^2.0.0")]),
                ("1.0.0", false, vec![registry("c", "c", "^1.0.0")]),
            ],
        );
        add_package(
            &mut source,
            "b",
            &[("1.0.0", false, vec![registry("c", "c", "^1.0.0")])],
        );
        add_package(
            &mut source,
            "c",
            &[("2.0.0", false, vec![]), ("1.0.0", false, vec![])],
        );
        let resolution = resolve_packages(
            &mut source,
            request(vec![
                registry("a", "a", "^1.0.0"),
                registry("b", "b", "1.0.0"),
            ]),
        )
        .expect("backtracking resolution");
        assert!(resolution.trace.iter().any(|event| matches!(
            event,
            TraceEvent::Backtracked { package, .. } if package == &key("a")
        )));
    }

    #[test]
    fn conflicts_and_cycles_fail_with_stable_trace() {
        let mut source = MockSource::default();
        add_package(
            &mut source,
            "a",
            &[("1.0.0", false, vec![registry("b", "b", "1.0.0")])],
        );
        add_package(
            &mut source,
            "b",
            &[("1.0.0", false, vec![registry("a", "a", "1.0.0")])],
        );
        let cycle = resolve_packages(&mut source, request(vec![registry("a", "a", "1.0.0")]))
            .expect_err("cycle fails");
        let trace = match cycle {
            ResolveError::Conflict { trace, .. } => trace,
            other => panic!("unexpected error {other:?}"),
        };
        assert!(trace.iter().any(|event| matches!(
            event,
            TraceEvent::CandidateRejected {
                reason: CandidateRejection::Cycle { .. },
                ..
            }
        )));

        let mut source = MockSource::default();
        add_package(&mut source, "a", &[("1.0.0", false, vec![])]);
        let conflict = resolve_packages(
            &mut source,
            request(vec![
                registry("a1", "a", "1.0.0"),
                registry("a2", "a", "2.0.0"),
            ]),
        )
        .expect_err("incompatible root constraints fail");
        match conflict {
            ResolveError::Conflict { requirements, .. } => {
                assert_eq!(requirements, [requirement("1.0.0"), requirement("2.0.0")]);
            }
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn independent_roots_do_not_form_a_cycle_from_selection_order() {
        let mut source = MockSource::default();
        add_package(&mut source, "a", &[("1.0.0", false, vec![])]);
        add_package(
            &mut source,
            "b",
            &[("1.0.0", false, vec![registry("a", "a", "1.0.0")])],
        );
        let resolution = resolve_packages(
            &mut source,
            request(vec![
                registry("a", "a", "1.0.0"),
                registry("b", "b", "1.0.0"),
            ]),
        )
        .expect("independent root plus inbound dependency resolves");
        assert_eq!(resolution.packages.len(), 2);
    }

    #[test]
    fn fresh_yanks_are_excluded_but_verified_locked_yanks_are_permitted() {
        let mut source = MockSource::default();
        add_package(
            &mut source,
            "a",
            &[("2.0.0", true, vec![]), ("1.0.0", false, vec![])],
        );
        let fresh = resolve_packages(&mut source, request(vec![registry("a", "a", "^1.0.0")]))
            .expect("fresh resolution");
        assert_eq!(fresh.packages[0].version, version("1.0.0"));

        let mut locked_request = request(vec![registry("a", "a", "^2.0.0")]);
        locked_request.locked.insert(
            key("a"),
            LockedSelection {
                version: version("2.0.0"),
            },
        );
        locked_request.mode = ResolutionMode::Locked;
        let locked = resolve_packages(&mut source, locked_request).expect("trusted locked yank");
        assert_eq!(locked.packages[0].version, version("2.0.0"));
        assert!(locked.packages[0].yanked);

        let mut source = MockSource::default();
        add_package(
            &mut source,
            "a",
            &[("1.9.0", false, vec![]), ("1.5.0", true, vec![])],
        );
        let mut update = request(vec![registry("a", "a", "^1.0.0")]);
        update.mode = ResolutionMode::Update;
        update.locked.insert(
            key("a"),
            LockedSelection {
                version: version("1.5.0"),
            },
        );
        let updated = resolve_packages(&mut source, update).expect("update moves off yank");
        assert_eq!(updated.packages[0].version, version("1.9.0"));
    }

    #[test]
    fn path_dependencies_are_preserved_without_source_access() {
        let mut source = MockSource::default();
        let resolution = resolve_packages(
            &mut source,
            request(vec![Dependency::Path(PathDependency {
                alias: "local".to_owned(),
                path: "../local".to_owned(),
                version: Some(version("1.0.0")),
            })]),
        )
        .expect("path-only resolution");
        assert!(resolution.packages.is_empty());
        assert_eq!(resolution.path_dependencies[0].path, "../local");
        assert!(source.calls.is_empty());
    }

    #[test]
    fn catalog_authentication_precedes_candidate_verification_and_dependency_expansion() {
        let mut source = MockSource::default();
        add_package(
            &mut source,
            "a",
            &[("1.0.0", false, vec![registry("b", "b", "1.0.0")])],
        );
        add_package(&mut source, "b", &[("1.0.0", false, vec![])]);
        resolve_packages(&mut source, request(vec![registry("a", "a", "1.0.0")]))
            .expect("trusted graph");
        assert_eq!(
            source.calls,
            [
                "auth:default@file:///registry/index.json::axiom/a",
                "verify:default@file:///registry/index.json::axiom/a@1.0.0",
                "auth:default@file:///registry/index.json::axiom/b",
                "verify:default@file:///registry/index.json::axiom/b@1.0.0",
            ]
        );
    }

    #[test]
    fn mismatched_exact_release_identity_cannot_expand_dependencies() {
        let mut source = MockSource::default();
        add_package(
            &mut source,
            "a",
            &[("1.0.0", false, vec![registry("b", "b", "1.0.0")])],
        );
        source
            .verified
            .get_mut(&(key("a"), version("1.0.0")))
            .expect("release")
            .as_mut()
            .expect("verified fixture")
            .release_id = "different-release".to_owned();
        let error = resolve_packages(&mut source, request(vec![registry("a", "a", "1.0.0")]))
            .expect_err("release identity mismatch fails");
        let trace = match error {
            ResolveError::Conflict { trace, .. } => trace,
            other => panic!("unexpected error {other:?}"),
        };
        assert!(trace.iter().any(|event| matches!(
            event,
            TraceEvent::CandidateRejected {
                reason: CandidateRejection::IdentityMismatch,
                ..
            }
        )));
        assert!(
            !source.calls.iter().any(|call| call.contains("axiom/b")),
            "dependencies from mismatched release identity must stay invisible"
        );
    }

    #[test]
    fn input_order_is_canonical_and_budgets_fail_closed() {
        let build = |dependencies: Vec<Dependency>| {
            let mut source = MockSource::default();
            add_package(&mut source, "a", &[("1.0.0", false, vec![])]);
            add_package(&mut source, "b", &[("1.0.0", false, vec![])]);
            resolve_packages(&mut source, request(dependencies)).expect("ordered resolution")
        };
        let left = build(vec![
            registry("b", "b", "1.0.0"),
            registry("a", "a", "1.0.0"),
        ]);
        let right = build(vec![
            registry("a", "a", "1.0.0"),
            registry("b", "b", "1.0.0"),
        ]);
        assert_eq!(left.packages, right.packages);
        assert_eq!(left.edges, right.edges);
        assert_eq!(left.trace, right.trace);

        let mut source = MockSource::default();
        add_package(
            &mut source,
            "a",
            &[("2.0.0", false, vec![]), ("1.0.0", false, vec![])],
        );
        let mut limited = request(vec![registry("a", "a", "^1.0.0")]);
        limited.limits.max_catalog_candidates = 1;
        assert!(matches!(
            resolve_packages(&mut source, limited),
            Err(ResolveError::BudgetExceeded {
                budget: "catalog candidates",
                ..
            })
        ));
    }

    #[test]
    fn duplicate_catalog_versions_are_rejected_even_with_distinct_release_ids() {
        let mut source = MockSource::default();
        let package = key("a");
        source.catalogs.insert(
            package.clone(),
            AuthenticatedCatalog::new(
                package,
                vec![
                    CatalogCandidate {
                        version: version("1.0.0"),
                        yanked: false,
                        release_id: "first-target".to_owned(),
                    },
                    CatalogCandidate {
                        version: version("1.0.0"),
                        yanked: false,
                        release_id: "second-target".to_owned(),
                    },
                ],
                "authenticated",
            ),
        );
        assert!(matches!(
            resolve_packages(&mut source, request(vec![registry("a", "a", "1.0.0")])),
            Err(ResolveError::InvalidCatalog { .. })
        ));
        assert!(
            !source.calls.iter().any(|call| call.starts_with("verify:")),
            "ambiguous catalog must fail before candidate verification"
        );
    }

    #[test]
    fn targeted_update_freezes_non_target_without_synthetic_edges() {
        let mut source = MockSource::default();
        add_package(
            &mut source,
            "a",
            &[("1.9.0", false, vec![]), ("1.0.0", false, vec![])],
        );
        add_package(
            &mut source,
            "b",
            &[("1.9.0", false, vec![]), ("1.0.0", false, vec![])],
        );
        let mut update = request(vec![
            registry("a", "a", "^1.0.0"),
            registry("b", "b", "^1.0.0"),
        ]);
        update.mode = ResolutionMode::Update;
        update.locked.insert(
            key("a"),
            LockedSelection {
                version: version("1.0.0"),
            },
        );
        let frozen = LockedSelection {
            version: version("1.0.0"),
        };
        update.locked.insert(key("b"), frozen.clone());
        update.frozen.insert(key("b"), frozen);

        let resolution = resolve_packages(&mut source, update).expect("targeted update");
        assert_eq!(
            resolution
                .packages
                .iter()
                .find(|package| package.package == key("a"))
                .expect("a")
                .version,
            version("1.9.0")
        );
        assert_eq!(
            resolution
                .packages
                .iter()
                .find(|package| package.package == key("b"))
                .expect("b")
                .version,
            version("1.0.0")
        );
        assert_eq!(resolution.edges.len(), 2);
    }

    #[test]
    fn targeted_update_preserves_broader_update_required_trace_evidence() {
        let mut source = MockSource::default();
        add_package(
            &mut source,
            "a",
            &[
                ("1.9.0", false, vec![registry("b", "b", "^1.5.0")]),
                ("1.0.0", false, vec![registry("b", "b", "^1.0.0")]),
            ],
        );
        add_package(
            &mut source,
            "b",
            &[("1.5.0", false, vec![]), ("1.0.0", false, vec![])],
        );
        let mut update = request(vec![
            registry("a", "a", "^1.0.0"),
            registry("b", "b", "^1.0.0"),
        ]);
        update.mode = ResolutionMode::Update;
        update.frozen.insert(
            key("b"),
            LockedSelection {
                version: version("1.0.0"),
            },
        );

        let resolution =
            resolve_packages(&mut source, update).expect("old target remains resolvable");
        assert_eq!(
            resolution
                .packages
                .iter()
                .find(|package| package.package == key("a"))
                .expect("target")
                .version,
            version("1.0.0")
        );
        assert!(resolution.trace.iter().any(|event| matches!(
            event,
            TraceEvent::CandidateRejected {
                package,
                reason: CandidateRejection::FrozenMismatch { required },
                ..
            } if package == &key("b") && *required == version("1.0.0")
        )));
    }

    #[test]
    fn resolution_and_trace_have_stable_tagged_serialization() {
        let package = key("a");
        let value = serde_json::to_value(Resolution {
            schema_version: "axiom.package_resolution.v1".to_owned(),
            packages: vec![],
            edges: vec![],
            path_dependencies: vec![],
            trace: vec![TraceEvent::CandidateRejected {
                package: package.clone(),
                version: version("1.2.3"),
                reason: CandidateRejection::FrozenMismatch {
                    required: version("1.0.0"),
                },
            }],
        })
        .expect("serialize resolution");
        assert_eq!(
            value,
            serde_json::json!({
                "schema_version": "axiom.package_resolution.v1",
                "packages": [],
                "edges": [],
                "path_dependencies": [],
                "trace": [{
                    "event": "candidate_rejected",
                    "package": {
                        "registry": "default",
                        "source": "file:///registry/index.json",
                        "namespace": "axiom",
                        "name": "a"
                    },
                    "version": {"major": 1, "minor": 2, "patch": 3},
                    "reason": {
                        "reason": "frozen_mismatch",
                        "required": {"major": 1, "minor": 0, "patch": 0}
                    }
                }]
            })
        );
    }
}
