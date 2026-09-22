use super::module_parse_cache::ModuleFingerprint;
use super::{
    ModuleParseCache, analyze_entry_with_parse_cache, buildable_package_manifest,
    canonicalize_existing_path, canonicalize_package_path, load_package_graph, normalize_path,
};
use crate::diagnostics::Diagnostic;
use crate::lsp::LspAnalysisCache;
use crate::manifest::{entry_path, manifest_path};
use crate::syntax;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LspResolvedModule {
    pub path: PathBuf,
    pub program: syntax::Program,
    pub imports: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
struct CachedModule {
    module: LspResolvedModule,
    fingerprint: ModuleFingerprint,
}

#[derive(Debug, Clone)]
struct CachedPackage {
    modules: BTreeMap<PathBuf, CachedModule>,
    reverse_dependencies: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
    observed_paths: BTreeSet<PathBuf>,
    observed_fingerprints: BTreeMap<PathBuf, ModuleFingerprint>,
    manifest_fingerprint: Option<ModuleFingerprint>,
    result: Result<Vec<LspResolvedModule>, Diagnostic>,
}

/// Persistent analysis state for one LSP server/workspace.
///
/// The compiler's project analyzer remains the authority for semantic
/// analysis. This cache keeps its parse cache alive across requests and keeps
/// the last resolved module result so an unchanged request can publish cached
/// diagnostics without entering the compiler analysis path again.
#[derive(Debug, Default)]
struct PersistentAnalysisState {
    parse_cache: ModuleParseCache,
    packages: BTreeMap<PathBuf, CachedPackage>,
    pub(super) analysis_runs: usize,
    pub(super) cache_hits: usize,
    pub(super) last_invalidated: BTreeSet<PathBuf>,
}

impl PersistentAnalysisState {
    fn analyze_package(
        &mut self,
        package_root: &Path,
        overlays: &BTreeMap<PathBuf, String>,
    ) -> Result<Vec<LspResolvedModule>, Diagnostic> {
        let package_root =
            canonicalize_existing_path(&normalize_path(package_root), "package root")?;
        self.parse_cache.set_overlays(overlays);
        let changed_programs = self
            .parse_cache
            .invalidate_changed_programs()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let manifest_path = manifest_path(&package_root);
        let manifest_fingerprint = self.parse_cache.fingerprint_for_path(&manifest_path).ok();
        let previous = self.packages.get(&package_root).cloned();

        let Some(previous_package) = previous.as_ref() else {
            return self.run_analysis(package_root, BTreeSet::new(), manifest_fingerprint, None);
        };

        let mut changed = changed_programs
            .into_iter()
            .filter(|path| {
                previous_package.observed_paths.contains(path) || path.starts_with(&package_root)
            })
            .collect::<BTreeSet<_>>();
        for path in &previous_package.observed_paths {
            let current = self.parse_cache.fingerprint_for_path(path).ok();
            let cached = previous_package.observed_fingerprints.get(path);
            if cached.is_some() && current.as_ref() != cached {
                changed.insert(path.clone());
            }
        }
        let manifest_changed = manifest_fingerprint != previous_package.manifest_fingerprint;
        if manifest_changed {
            changed.insert(manifest_path);
        }

        let invalidated = if manifest_changed {
            previous_package.modules.keys().cloned().collect()
        } else {
            reverse_dependency_closure(&changed, &previous_package.reverse_dependencies)
        };
        if changed.is_empty() {
            self.cache_hits = self.cache_hits.saturating_add(1);
            return previous_package.result.clone();
        }

        self.run_analysis(
            package_root,
            invalidated,
            manifest_fingerprint,
            Some(previous_package.clone()),
        )
    }

    fn run_analysis(
        &mut self,
        package_root: PathBuf,
        invalidated: BTreeSet<PathBuf>,
        manifest_fingerprint: Option<ModuleFingerprint>,
        previous: Option<CachedPackage>,
    ) -> Result<Vec<LspResolvedModule>, Diagnostic> {
        let result = (|| {
            let graph = load_package_graph(&package_root)?;
            let manifest = buildable_package_manifest(&graph, &package_root)?;
            let entry = canonicalize_package_path(
                &entry_path(&package_root, &manifest),
                &package_root,
                "manifest",
                "build.entry resolves outside the package",
            )?;
            let analyzed = analyze_entry_with_parse_cache(
                &graph,
                &package_root,
                manifest,
                entry,
                syntax::DEFAULT_MACRO_RECURSION_LIMIT,
                &mut self.parse_cache,
            )?;
            Ok(analyzed
                .modules
                .into_iter()
                .map(|module| LspResolvedModule {
                    path: module.path,
                    program: module.program,
                    imports: module
                        .resolved_imports
                        .into_iter()
                        .map(|import| import.path)
                        .collect(),
                })
                .collect::<Vec<_>>())
        })();

        self.parse_cache.remember_fingerprints();
        self.analysis_runs = self.analysis_runs.saturating_add(1);
        self.last_invalidated = invalidated.clone();

        let Ok(modules) = &result else {
            // A first analysis can fail before the parse cache has a program
            // to invalidate (for example, when an opened overlay is
            // malformed). Keep the diagnostic tied to its source path when
            // possible; otherwise do not retain an error that has no
            // observable invalidation source.
            let mut observed_paths = invalidated;
            if let Some(path) = result
                .as_ref()
                .err()
                .and_then(|diagnostic: &Diagnostic| diagnostic.path.as_deref())
            {
                observed_paths.insert(normalize_path(Path::new(path)));
            }
            let observed_fingerprints: BTreeMap<PathBuf, ModuleFingerprint> = observed_paths
                .iter()
                .filter_map(|path| {
                    self.parse_cache
                        .fingerprint_for_path(path)
                        .ok()
                        .map(|fingerprint| (path.clone(), fingerprint))
                })
                .collect();
            if observed_fingerprints.is_empty() {
                return result;
            }
            self.packages.insert(
                package_root,
                CachedPackage {
                    modules: BTreeMap::new(),
                    reverse_dependencies: BTreeMap::new(),
                    observed_paths,
                    observed_fingerprints,
                    manifest_fingerprint,
                    result: result.clone(),
                },
            );
            return result;
        };

        let previous_modules = previous.as_ref().map(|package| &package.modules);
        let mut cached_modules = BTreeMap::new();
        let mut resolved = Vec::with_capacity(modules.len());
        for module in modules {
            let fingerprint = self
                .parse_cache
                .fingerprint_for_path(&module.path)
                .unwrap_or_else(|_| synthetic_fingerprint(&module));
            let cached = previous_modules
                .and_then(|modules| modules.get(&module.path))
                .filter(|cached| {
                    !invalidated.contains(&module.path) && cached.fingerprint == fingerprint
                })
                .map(|cached| cached.module.clone())
                .unwrap_or_else(|| module.clone());
            resolved.push(cached.clone());
            cached_modules.insert(
                module.path.clone(),
                CachedModule {
                    module: cached,
                    fingerprint,
                },
            );
        }

        let mut reverse_dependencies = BTreeMap::<PathBuf, BTreeSet<PathBuf>>::new();
        let mut observed_paths = BTreeSet::new();
        for module in &resolved {
            observed_paths.insert(module.path.clone());
            for import in &module.imports {
                observed_paths.insert(import.clone());
                reverse_dependencies
                    .entry(import.clone())
                    .or_default()
                    .insert(module.path.clone());
            }
        }
        let observed_fingerprints = observed_paths
            .iter()
            .filter_map(|path| {
                self.parse_cache
                    .fingerprint_for_path(path)
                    .ok()
                    .map(|fingerprint| (path.clone(), fingerprint))
            })
            .collect();
        self.packages.insert(
            package_root,
            CachedPackage {
                modules: cached_modules,
                reverse_dependencies,
                observed_paths,
                observed_fingerprints,
                manifest_fingerprint,
                result: Ok(resolved.clone()),
            },
        );
        Ok(resolved)
    }
}

impl LspAnalysisCache {
    fn state_mut(&mut self) -> &mut PersistentAnalysisState {
        if self.state.is_none() {
            self.state = Some(Box::new(PersistentAnalysisState::default()));
        }
        self.state
            .as_mut()
            .and_then(|state| state.downcast_mut::<PersistentAnalysisState>())
            .expect("LSP analysis cache state has the expected type")
    }

    pub(crate) fn analysis_runs(&self) -> usize {
        self.state
            .as_ref()
            .and_then(|state| state.downcast_ref::<PersistentAnalysisState>())
            .map_or(0, |state| state.analysis_runs)
    }

    pub(crate) fn cache_hits(&self) -> usize {
        self.state
            .as_ref()
            .and_then(|state| state.downcast_ref::<PersistentAnalysisState>())
            .map_or(0, |state| state.cache_hits)
    }

    pub(crate) fn last_invalidated(&self) -> BTreeSet<PathBuf> {
        self.state
            .as_ref()
            .and_then(|state| state.downcast_ref::<PersistentAnalysisState>())
            .map_or_else(BTreeSet::new, |state| state.last_invalidated.clone())
    }

    fn analyze_package(
        &mut self,
        package_root: &Path,
        overlays: &BTreeMap<PathBuf, String>,
    ) -> Result<Vec<LspResolvedModule>, Diagnostic> {
        self.state_mut().analyze_package(package_root, overlays)
    }
}

fn reverse_dependency_closure(
    changed: &BTreeSet<PathBuf>,
    reverse_dependencies: &BTreeMap<PathBuf, BTreeSet<PathBuf>>,
) -> BTreeSet<PathBuf> {
    let mut invalidated = changed.clone();
    let mut pending = changed.iter().cloned().collect::<Vec<_>>();
    while let Some(path) = pending.pop() {
        if let Some(dependents) = reverse_dependencies.get(&path) {
            for dependent in dependents {
                if invalidated.insert(dependent.clone()) {
                    pending.push(dependent.clone());
                }
            }
        }
    }
    invalidated
}

fn synthetic_fingerprint(module: &LspResolvedModule) -> ModuleFingerprint {
    ModuleFingerprint {
        generation: module.path.to_string_lossy().len() as u64,
        metadata: super::module_parse_cache::ModuleMetadata {
            length: 0,
            modified: None,
            readonly: false,
            is_file: true,
        },
    }
}

/// Analyze a package through normal import resolution and HIR while using
/// editor overlays and persistent per-workspace state.
pub fn analyze_package_for_lsp(
    package_root: &Path,
    overlays: &BTreeMap<PathBuf, String>,
    cache: &mut LspAnalysisCache,
) -> Result<Vec<LspResolvedModule>, Diagnostic> {
    cache.analyze_package(package_root, overlays)
}
