use super::{canonicalize_existing_path, normalize_path};
use crate::diagnostics::Diagnostic;
use crate::{stdlib, syntax};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ModuleFingerprint {
    pub(super) generation: u64,
    pub(super) metadata: ModuleMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ModuleMetadata {
    pub(super) length: u64,
    pub(super) modified: Option<SystemTime>,
    pub(super) readonly: bool,
    pub(super) is_file: bool,
}

#[derive(Debug, Default)]
pub(super) struct ModuleParseCache {
    pub(super) programs: HashMap<(PathBuf, usize), syntax::Program>,
    overlays: BTreeMap<PathBuf, String>,
    fingerprints: HashMap<(PathBuf, usize), ModuleFingerprint>,
}

impl ModuleParseCache {
    pub(super) fn with_overlays(overlays: &BTreeMap<PathBuf, String>) -> Self {
        let mut cache = Self::default();
        cache.set_overlays(overlays);
        cache
    }

    pub(super) fn set_overlays(&mut self, overlays: &BTreeMap<PathBuf, String>) {
        let mut normalized = BTreeMap::new();
        for (path, source) in overlays {
            let path = normalize_path(path);
            normalized.insert(path.clone(), source.clone());
            if let Ok(canonical) = fs::canonicalize(&path) {
                normalized.insert(canonical, source.clone());
            }
        }
        self.overlays = normalized;
    }

    pub(super) fn overlay_source(&self, module_path: &Path) -> Option<&str> {
        self.overlays
            .get(&normalize_path(module_path))
            .map(String::as_str)
    }

    pub(super) fn invalidate_changed_programs(&mut self) -> Vec<PathBuf> {
        let keys = self.programs.keys().cloned().collect::<Vec<_>>();
        let mut changed = Vec::new();
        for (path, macro_recursion_limit) in keys {
            let current = self.fingerprint_for_path(&path);
            let cached = self
                .fingerprints
                .get(&(path.clone(), macro_recursion_limit));
            if cached != current.as_ref().ok() {
                self.programs.remove(&(path.clone(), macro_recursion_limit));
                self.fingerprints
                    .remove(&(path.clone(), macro_recursion_limit));
                changed.push(path);
            }
        }
        changed
    }

    pub(super) fn fingerprint_for_path(
        &self,
        module_path: &Path,
    ) -> Result<ModuleFingerprint, Diagnostic> {
        let normalized = normalize_path(module_path);
        if let Some(source) = self.overlays.get(&normalized) {
            return Ok(ModuleFingerprint {
                generation: content_generation(source),
                metadata: ModuleMetadata {
                    length: source.len() as u64,
                    modified: None,
                    readonly: false,
                    is_file: true,
                },
            });
        }
        let is_stdlib = stdlib::is_stdlib_path(module_path);
        let source = if is_stdlib {
            stdlib::stdlib_source_for(module_path)
                .map(str::to_string)
                .ok_or_else(|| {
                    Diagnostic::new(
                        "source",
                        format!("missing stdlib source for {}", module_path.display()),
                    )
                    .with_path(module_path.display().to_string())
                })?
        } else {
            fs::read_to_string(module_path).map_err(|err| {
                Diagnostic::new(
                    "source",
                    format!("failed to read {}: {err}", module_path.display()),
                )
                .with_path(module_path.display().to_string())
            })?
        };
        if is_stdlib {
            return Ok(ModuleFingerprint {
                generation: content_generation(&source),
                metadata: ModuleMetadata {
                    length: source.len() as u64,
                    modified: None,
                    readonly: true,
                    is_file: true,
                },
            });
        }
        let metadata = fs::metadata(module_path).map_err(|err| {
            Diagnostic::new(
                "source",
                format!("failed to stat {}: {err}", module_path.display()),
            )
            .with_path(module_path.display().to_string())
        })?;
        Ok(ModuleFingerprint {
            generation: content_generation(&source),
            metadata: ModuleMetadata {
                length: metadata.len(),
                modified: metadata.modified().ok(),
                readonly: metadata.permissions().readonly(),
                is_file: metadata.is_file(),
            },
        })
    }

    pub(super) fn remember_fingerprints(&mut self) {
        let keys = self.programs.keys().cloned().collect::<Vec<_>>();
        for (path, macro_recursion_limit) in keys {
            if let Ok(fingerprint) = self.fingerprint_for_path(&path) {
                self.fingerprints
                    .insert((path, macro_recursion_limit), fingerprint);
            }
        }
    }
}

fn content_generation(source: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn parse_module_with_cache(
    module_path: &Path,
    macro_recursion_limit: usize,
    parse_cache: &mut ModuleParseCache,
) -> Result<syntax::Program, Diagnostic> {
    let normalized = normalize_path(module_path);
    if let Some(source) = parse_cache.overlays.get(&normalized) {
        return syntax::parse_program_with_options(
            source,
            module_path,
            &syntax::ParseOptions {
                macro_recursion_limit,
                ..syntax::ParseOptions::default()
            },
        );
    }
    let cache_key = module_parse_cache_key(module_path, macro_recursion_limit)?;
    if let Some(program) = parse_cache.programs.get(&cache_key) {
        return Ok(program.clone());
    }
    let source = if stdlib::is_stdlib_path(module_path) {
        stdlib::stdlib_source_for(module_path)
            .map(str::to_string)
            .ok_or_else(|| {
                Diagnostic::new(
                    "source",
                    format!(
                        "internal error: missing stdlib source for {}",
                        module_path.display()
                    ),
                )
                .with_path(module_path.display().to_string())
            })?
    } else {
        fs::read_to_string(module_path).map_err(|err| {
            Diagnostic::new(
                "source",
                format!("failed to read {}: {err}", module_path.display()),
            )
            .with_path(module_path.display().to_string())
        })?
    };
    let program = syntax::parse_program_with_options(
        &source,
        module_path,
        &syntax::ParseOptions {
            macro_recursion_limit,
            ..syntax::ParseOptions::default()
        },
    )?;
    parse_cache.programs.insert(cache_key, program.clone());
    Ok(program)
}

// The parse result depends on the macro recursion limit, so the limit is part
// of the key: a cache hit must never return a program parsed under a
// different limit than the caller requested.
pub(super) fn module_parse_cache_key(
    module_path: &Path,
    macro_recursion_limit: usize,
) -> Result<(PathBuf, usize), Diagnostic> {
    let path = if stdlib::is_stdlib_path(module_path) {
        normalize_path(module_path)
    } else {
        canonicalize_existing_path(module_path, "module path")?
    };
    Ok((path, macro_recursion_limit))
}
