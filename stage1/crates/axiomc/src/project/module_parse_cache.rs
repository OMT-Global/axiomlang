use super::{canonicalize_existing_path, normalize_path};
use crate::diagnostics::Diagnostic;
use crate::{stdlib, syntax};
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
        Self {
            programs: HashMap::new(),
            overlays: overlays
                .iter()
                .map(|(path, source)| (normalize_path(path), source.clone()))
                .collect(),
        }
    }
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
