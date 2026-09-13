use super::normalize_path;
use crate::syntax;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub(super) struct ModuleParseCache {
    pub(super) programs: HashMap<(PathBuf, usize), syntax::Program>,
    overlays: BTreeMap<PathBuf, String>,
}

impl ModuleParseCache {
    pub(super) fn with_overlays(overlays: &BTreeMap<PathBuf, String>) -> Self {
        let mut normalized = BTreeMap::new();
        for (path, source) in overlays {
            let path = normalize_path(path);
            normalized.insert(path.clone(), source.clone());
            if let Ok(canonical) = fs::canonicalize(&path) {
                normalized.insert(canonical, source.clone());
            }
        }
        Self {
            programs: HashMap::new(),
            overlays: normalized,
        }
    }

    pub(super) fn overlay_source(&self, module_path: &Path) -> Option<&str> {
        self.overlays
            .get(&normalize_path(module_path))
            .map(String::as_str)
    }
}
