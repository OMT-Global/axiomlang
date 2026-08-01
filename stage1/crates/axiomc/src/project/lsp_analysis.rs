use super::{
    ModuleParseCache, analyze_entry_with_parse_cache, buildable_package_manifest,
    canonicalize_existing_path, canonicalize_package_path, load_package_graph, normalize_path,
};
use crate::diagnostics::Diagnostic;
use crate::manifest::entry_path;
use crate::syntax;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LspResolvedModule {
    pub path: PathBuf,
    pub program: syntax::Program,
    pub imports: Vec<PathBuf>,
}

/// Analyze a package through normal import resolution and HIR while using editor overlays.
pub fn analyze_package_for_lsp(
    package_root: &Path,
    overlays: &BTreeMap<PathBuf, String>,
) -> Result<Vec<LspResolvedModule>, Diagnostic> {
    let package_root = canonicalize_existing_path(&normalize_path(package_root), "package root")?;
    let graph = load_package_graph(&package_root)?;
    let manifest = buildable_package_manifest(&graph, &package_root)?;
    let entry = canonicalize_package_path(
        &entry_path(&package_root, &manifest),
        &package_root,
        "manifest",
        "build.entry resolves outside the package",
    )?;
    let mut parse_cache = ModuleParseCache::with_overlays(overlays);
    let analyzed = analyze_entry_with_parse_cache(
        &graph,
        &package_root,
        manifest,
        entry,
        syntax::DEFAULT_MACRO_RECURSION_LIMIT,
        &mut parse_cache,
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
        .collect())
}
