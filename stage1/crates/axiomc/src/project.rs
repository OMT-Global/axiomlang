use crate::codegen::{
    NativeBackendKind, compile_native, render_rust_for_package_with_capabilities,
};
use crate::diagnostics::Diagnostic;
use crate::hir;
use crate::lockfile::validate_lockfile;
use crate::manifest::{
    BuildSection, CapabilityConfig, CapabilityDescriptor, CapabilityKind, Manifest, PackageSection,
    TestKind, binary_path_for_target, capability_descriptors, entry_path, generated_rust_path,
    load_manifest, manifest_path, out_dir_path,
};
use crate::mir;
use crate::stdlib;
use crate::syntax;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::env;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const BUILD_CACHE_VERSION: u32 = 1;
const BUILD_CACHE_COMPILER: &str = concat!("axiomc-stage1-", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Serialize)]
pub struct CheckedPackage {
    pub package_root: String,
    pub manifest: String,
    pub entry: String,
    pub statement_count: usize,
    pub capabilities: Vec<CapabilityDescriptor>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckOutput {
    pub manifest: String,
    pub entry: String,
    pub statement_count: usize,
    pub capabilities: Vec<CapabilityDescriptor>,
    pub warnings: Vec<String>,
    pub packages: Vec<CheckedPackage>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuiltPackage {
    pub backend: NativeBackendKind,
    pub package_root: String,
    pub manifest: String,
    pub entry: String,
    pub binary: String,
    pub generated_rust: String,
    pub debug_map: Option<String>,
    pub statement_count: usize,
    pub target: Option<String>,
    pub debug: bool,
    pub cache_status: BuildCacheStatus,
    pub compile_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildOutput {
    pub backend: NativeBackendKind,
    pub locked: bool,
    pub offline: bool,
    pub manifest: String,
    pub entry: String,
    pub binary: String,
    pub generated_rust: String,
    pub debug_map: Option<String>,
    pub statement_count: usize,
    pub target: Option<String>,
    pub debug: bool,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub duration_ms: u64,
    pub packages: Vec<BuiltPackage>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuildCacheStatus {
    Hit,
    Miss,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestCaseResult {
    pub package_root: String,
    pub name: String,
    pub kind: TestKind,
    pub entry: String,
    pub ok: bool,
    pub binary: Option<String>,
    pub generated_rust: Option<String>,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub expected_stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_error: Option<ExpectedDiagnostic>,
    pub duration_ms: u64,
    pub error: Option<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExpectedDiagnostic {
    pub kind: String,
    pub code: Option<String>,
    pub message: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestOutput {
    pub manifest: String,
    pub packages: Vec<String>,
    pub cases: Vec<TestCaseResult>,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub kinds: BTreeMap<TestKind, usize>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct CheckOptions {
    pub package: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct BuildOptions {
    /// Preparatory backend plumbing; `generated-rust` remains the only implemented option today.
    pub backend: NativeBackendKind,
    pub target: Option<String>,
    pub package: Option<String>,
    pub debug: bool,
    /// Require the checked-in axiom.lock graph to match the local manifest graph.
    pub locked: bool,
    /// Resolve the build graph without network access. Stage1 currently supports local path graphs only.
    pub offline: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub package: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TestOptions {
    pub filter: Option<String>,
    pub package: Option<String>,
    pub include_benchmarks: bool,
}

pub fn check_project(project_root: &Path) -> Result<CheckOutput, Diagnostic> {
    check_project_with_options(project_root, &CheckOptions::default())
}

pub fn check_project_with_options(
    project_root: &Path,
    options: &CheckOptions,
) -> Result<CheckOutput, Diagnostic> {
    let project_root = canonicalize_existing_path(&normalize_path(project_root), "project root")?;
    let graph = load_package_graph(&project_root)?;
    validate_workspace_root_lockfile(&graph, &project_root)?;
    let mut packages = Vec::new();
    for package_root in workspace_package_roots(&graph, &project_root, options.package.as_deref())?
    {
        let analyzed = analyze_package(&graph, &package_root)?;
        packages.push(CheckedPackage {
            package_root: package_root.display().to_string(),
            manifest: manifest_path(&package_root).display().to_string(),
            entry: analyzed.entry_path.display().to_string(),
            statement_count: analyzed.mir.statement_count(),
            capabilities: capability_descriptors(&analyzed.manifest.capabilities),
            warnings: analyzed.manifest.capabilities.warnings(),
        });
    }
    let root = packages.first().cloned().ok_or_else(|| {
        Diagnostic::new(
            "manifest",
            format!(
                "internal error: no packages discovered for {}",
                project_root.display()
            ),
        )
    })?;
    Ok(CheckOutput {
        manifest: root.manifest,
        entry: root.entry,
        statement_count: root.statement_count,
        capabilities: root.capabilities,
        warnings: root.warnings,
        packages,
    })
}

pub fn build_project(project_root: &Path) -> Result<BuildOutput, Diagnostic> {
    build_project_with_options(project_root, &BuildOptions::default())
}

fn resolved_build_target(target: Option<&str>) -> Option<String> {
    match target {
        Some("wasm32") | Some("wasm32-wasi") => Some(String::from("wasm32-wasip1")),
        Some(target) => Some(target.to_string()),
        None => None,
    }
}

pub fn build_project_with_options(
    project_root: &Path,
    options: &BuildOptions,
) -> Result<BuildOutput, Diagnostic> {
    let project_root = canonicalize_existing_path(&normalize_path(project_root), "project root")?;
    let graph = load_package_graph(&project_root)?;
    validate_workspace_root_lockfile(&graph, &project_root)?;
    let started = Instant::now();
    let mut packages = Vec::new();
    for package_root in workspace_package_roots(&graph, &project_root, options.package.as_deref())?
    {
        let analyzed = analyze_package(&graph, &package_root)?;
        let generated_rust = generated_rust_path(&package_root, &analyzed.manifest);
        let resolved_target = resolved_build_target(options.target.as_deref());
        let binary = binary_path_for_target(
            &package_root,
            &analyzed.manifest,
            resolved_target.as_deref(),
        );
        let report = build_artifacts(
            &graph,
            &package_root,
            &analyzed,
            &generated_rust,
            &binary,
            resolved_target.as_deref(),
            options,
        )?;
        packages.push(BuiltPackage {
            backend: options.backend,
            package_root: package_root.display().to_string(),
            manifest: manifest_path(&package_root).display().to_string(),
            entry: analyzed.entry_path.display().to_string(),
            binary: binary.display().to_string(),
            generated_rust: generated_rust.display().to_string(),
            debug_map: options
                .debug
                .then(|| debug_source_map_path(&generated_rust).display().to_string()),
            statement_count: analyzed.mir.statement_count(),
            target: resolved_target.clone(),
            debug: options.debug,
            cache_status: report.cache_status,
            compile_ms: report.compile_ms,
        });
    }
    let root = packages.first().cloned().ok_or_else(|| {
        Diagnostic::new(
            "manifest",
            format!(
                "internal error: no packages discovered for {}",
                project_root.display()
            ),
        )
    })?;
    let cache_hits = packages
        .iter()
        .filter(|package| package.cache_status == BuildCacheStatus::Hit)
        .count();
    let cache_misses = packages.len().saturating_sub(cache_hits);
    Ok(BuildOutput {
        backend: options.backend,
        locked: options.locked,
        offline: options.offline,
        manifest: root.manifest,
        entry: root.entry,
        binary: root.binary,
        generated_rust: root.generated_rust,
        debug_map: root.debug_map,
        statement_count: root.statement_count,
        target: root.target,
        debug: root.debug,
        cache_hits,
        cache_misses,
        duration_ms: started.elapsed().as_millis() as u64,
        packages,
    })
}

pub fn run_project(project_root: &Path) -> Result<i32, Diagnostic> {
    run_project_with_options(project_root, &RunOptions::default())
}

pub fn run_project_with_options(
    project_root: &Path,
    options: &RunOptions,
) -> Result<i32, Diagnostic> {
    let project_root = canonicalize_existing_path(&normalize_path(project_root), "project root")?;
    let graph = load_package_graph(&project_root)?;
    if options.package.is_none() && graph.context(&project_root)?.manifest.is_workspace_only() {
        return Err(Diagnostic::new(
            "run",
            "workspace-only manifests require -p/--package for `axiomc run`",
        )
        .with_path(manifest_path(&project_root).display().to_string()));
    }
    let built = build_project_with_options(
        &project_root,
        &BuildOptions {
            backend: NativeBackendKind::GeneratedRust,
            target: None,
            package: options.package.clone(),
            debug: false,
            locked: true,
            offline: true,
        },
    )?;
    let build_output_dir = Path::new(&built.generated_rust).parent().ok_or_else(|| {
        Diagnostic::new(
            "run",
            format!(
                "failed to determine build output directory for {}",
                built.binary
            ),
        )
    })?;
    let status = command_for_build_output(&built.binary, build_output_dir)
        .and_then(|mut command| command.status())
        .map_err(|err| {
            Diagnostic::new("run", format!("failed to execute {}: {err}", built.binary))
        })?;
    Ok(status.code().unwrap_or(1))
}

pub fn run_project_tests(project_root: &Path) -> Result<TestOutput, Diagnostic> {
    run_project_tests_with_options(project_root, &TestOptions::default())
}

pub fn run_project_tests_with_options(
    project_root: &Path,
    options: &TestOptions,
) -> Result<TestOutput, Diagnostic> {
    let project_root = canonicalize_existing_path(&normalize_path(project_root), "project root")?;
    let graph = load_package_graph(&project_root)?;
    validate_workspace_root_lockfile(&graph, &project_root)?;
    let manifest_path_text = manifest_path(&project_root).display().to_string();
    let mut packages = Vec::new();
    let mut cases = Vec::new();
    let started = Instant::now();
    for package_root in workspace_package_roots(&graph, &project_root, options.package.as_deref())?
    {
        let manifest = graph.context(&package_root)?.manifest.clone();
        validate_lockfile(&package_root, &manifest)?;
        if expected_error_path(&package_root).exists() {
            let case_name = manifest
                .package
                .as_ref()
                .map(|package| package.name.clone())
                .unwrap_or_else(|| package_root.display().to_string());
            let entry = manifest.build.entry.clone();
            if options
                .filter
                .as_deref()
                .map(|filter| case_name.contains(filter) || entry.contains(filter))
                .unwrap_or(true)
            {
                packages.push(package_root.display().to_string());
                cases.push(run_compile_fail_case(
                    &package_root,
                    &graph,
                    &manifest,
                    &case_name,
                ));
            }
            continue;
        }
        let tests = collect_test_targets(
            &package_root,
            &manifest,
            options.filter.as_deref(),
            options.include_benchmarks,
        )?;
        if tests.is_empty() {
            continue;
        }
        packages.push(package_root.display().to_string());
        for test in &tests {
            cases.push(run_test_case(&package_root, &graph, &manifest, test));
        }
    }
    if cases.is_empty() {
        return Err(Diagnostic::new(
            "test",
            "no tests discovered under src/**/*_test.ax across the workspace and no [[tests]] configured in axiom.toml",
        )
        .with_path(manifest_path_text));
    }
    let passed = cases.iter().filter(|case| case.ok).count();
    let failed = cases.len().saturating_sub(passed);
    let mut kinds = BTreeMap::new();
    for case in &cases {
        *kinds.entry(case.kind).or_insert(0) += 1;
    }
    Ok(TestOutput {
        manifest: manifest_path(&project_root).display().to_string(),
        packages,
        cases,
        passed,
        failed,
        skipped: 0,
        kinds,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

fn collect_test_targets(
    project_root: &Path,
    manifest: &Manifest,
    filter: Option<&str>,
    include_benchmarks: bool,
) -> Result<Vec<crate::manifest::TestTarget>, Diagnostic> {
    let mut tests = manifest.tests.clone();
    if !include_benchmarks {
        tests.retain(|test| test.kind != TestKind::Benchmark);
    }
    if let Some(expected_stdout) = load_package_expected_output(project_root)? {
        for test in &mut tests {
            if test.kind != TestKind::Benchmark && test.stdout.is_none() {
                test.stdout = Some(expected_stdout.clone());
            }
        }
    }
    let mut seen_entries = tests
        .iter()
        .map(|test| test.entry.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for discovered in discover_test_targets(project_root, include_benchmarks)? {
        if seen_entries.insert(discovered.entry.clone()) {
            tests.push(discovered);
        }
    }
    if let Some(filter) = filter {
        tests.retain(|test| test_matches_filter(test, filter));
    }
    Ok(tests)
}

fn discover_test_targets(
    project_root: &Path,
    include_benchmarks: bool,
) -> Result<Vec<crate::manifest::TestTarget>, Diagnostic> {
    let src_root = project_root.join("src");
    if !src_root.exists() {
        return Ok(Vec::new());
    }
    let package_expected_output = load_package_expected_output(project_root)?;
    let mut tests = Vec::new();
    collect_discovered_tests(
        project_root,
        &src_root,
        package_expected_output.as_deref(),
        include_benchmarks,
        &mut tests,
    )?;
    tests.sort_by(|left, right| left.entry.cmp(&right.entry));
    Ok(tests)
}

fn collect_discovered_tests(
    project_root: &Path,
    dir: &Path,
    package_expected_output: Option<&str>,
    include_benchmarks: bool,
    tests: &mut Vec<crate::manifest::TestTarget>,
) -> Result<(), Diagnostic> {
    let entries = fs::read_dir(dir).map_err(|err| {
        Diagnostic::new("test", format!("failed to read {}: {err}", dir.display()))
            .with_path(dir.display().to_string())
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| {
            Diagnostic::new("test", format!("failed to read {}: {err}", dir.display()))
                .with_path(dir.display().to_string())
        })?;
        let path = entry.path();
        if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            collect_discovered_tests(
                project_root,
                &path,
                package_expected_output,
                include_benchmarks,
                tests,
            )?;
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("ax") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        let kind = discovered_test_kind(stem, include_benchmarks);
        if kind.is_none() {
            continue;
        }
        let kind = kind.expect("test kind checked");
        let relative = normalize_path(path.strip_prefix(project_root).unwrap_or(&path));
        let stdout_path = path.with_extension("stdout");
        let stdout = if stdout_path.exists() {
            Some(fs::read_to_string(&stdout_path).map_err(|err| {
                Diagnostic::new(
                    "test",
                    format!("failed to read {}: {err}", stdout_path.display()),
                )
                .with_path(stdout_path.display().to_string())
            })?)
        } else if kind == TestKind::Benchmark {
            None
        } else {
            package_expected_output.map(str::to_string)
        };
        tests.push(crate::manifest::TestTarget {
            name: relative.with_extension("").display().to_string(),
            entry: relative.display().to_string(),
            stdout,
            kind,
        });
    }
    Ok(())
}

fn discovered_test_kind(stem: &str, include_benchmarks: bool) -> Option<TestKind> {
    if include_benchmarks && stem.ends_with("_bench") {
        return Some(TestKind::Benchmark);
    }
    if stem.ends_with("_property") || stem.ends_with("_property_test") {
        return Some(TestKind::Property);
    }
    if stem.ends_with("_table_test") {
        return Some(TestKind::Table);
    }
    if stem.ends_with("_snapshot_test") || stem.ends_with("_golden_test") {
        return Some(TestKind::Snapshot);
    }
    if stem.ends_with("_test") {
        return Some(TestKind::Unit);
    }
    None
}

fn load_package_expected_output(project_root: &Path) -> Result<Option<String>, Diagnostic> {
    let path = project_root.join("expected-output.txt");
    if !path.exists() {
        return Ok(None);
    }
    fs::read_to_string(&path).map(Some).map_err(|err| {
        Diagnostic::new("test", format!("failed to read {}: {err}", path.display()))
            .with_path(path.display().to_string())
    })
}

pub fn project_capabilities(project_root: &Path) -> Result<Vec<CapabilityDescriptor>, Diagnostic> {
    let manifest = load_manifest(project_root)?;
    Ok(capability_descriptors(&manifest.capabilities))
}

#[derive(Debug, Clone)]
struct PackageContext {
    root: PathBuf,
    manifest: Manifest,
    source_root: PathBuf,
    dependencies: BTreeMap<String, PathBuf>,
    workspace_members: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default)]
struct PackageGraph {
    packages: HashMap<PathBuf, PackageContext>,
}

impl PackageGraph {
    fn context(&self, package_root: &Path) -> Result<&PackageContext, Diagnostic> {
        self.packages.get(package_root).ok_or_else(|| {
            Diagnostic::new(
                "manifest",
                format!(
                    "internal error: unknown package root {}",
                    package_root.display()
                ),
            )
        })
    }
}

struct AnalyzedProject {
    manifest: Manifest,
    entry_path: PathBuf,
    mir: mir::Program,
    modules: Vec<LoadedModule>,
}

#[derive(Debug, Clone)]
struct LoadedModule {
    path: PathBuf,
    program: syntax::Program,
    is_entry: bool,
    package_root: PathBuf,
    source_root: PathBuf,
    package_name: String,
}

#[derive(Debug, Clone)]
struct ModuleSymbols {
    module_id: String,
    functions: HashMap<String, String>,
    public_functions: HashMap<String, String>,
    package_functions: HashMap<String, String>,
    private_functions: HashSet<String>,
    consts: HashMap<String, syntax::ConstDecl>,
    public_consts: HashMap<String, syntax::ConstDecl>,
    package_consts: HashMap<String, syntax::ConstDecl>,
    private_consts: HashSet<String>,
    aliases: HashMap<String, String>,
    public_aliases: HashMap<String, String>,
    package_aliases: HashMap<String, String>,
    private_aliases: HashSet<String>,
    structs: HashMap<String, String>,
    public_structs: HashMap<String, String>,
    package_structs: HashMap<String, String>,
    private_structs: HashSet<String>,
    enums: HashMap<String, String>,
    public_enums: HashMap<String, String>,
    package_enums: HashMap<String, String>,
    private_enums: HashSet<String>,
}

fn analyze_package(
    graph: &PackageGraph,
    package_root: &Path,
) -> Result<AnalyzedProject, Diagnostic> {
    let package_root = normalize_path(package_root);
    let package_root = canonicalize_existing_path(&package_root, "package root")?;
    let manifest = graph.context(&package_root)?.manifest.clone();
    if manifest.is_workspace_only() {
        return Err(Diagnostic::new(
            "manifest",
            format!(
                "workspace-only manifest at {} is not directly buildable",
                manifest_path(&package_root).display()
            ),
        )
        .with_path(manifest_path(&package_root).display().to_string()));
    }
    validate_lockfile(&package_root, &manifest)?;
    let entry = entry_path(&package_root, &manifest);
    let entry = canonicalize_package_path(
        &entry,
        &package_root,
        "manifest",
        "build.entry resolves outside the package",
    )?;
    analyze_entry(graph, &package_root, manifest, entry)
}

fn analyze_entry(
    graph: &PackageGraph,
    package_root: &Path,
    manifest: Manifest,
    entry: PathBuf,
) -> Result<AnalyzedProject, Diagnostic> {
    let modules = load_modules(graph, package_root, &entry)?;
    validate_module_capabilities(graph, &modules)?;
    let flattened = flatten_modules(graph, &modules)?;
    let hir = hir::lower_with_capabilities(&flattened, &manifest.capabilities)
        .map_err(|error| diagnostic_with_default_path(error, &entry))?;
    let mir = mir::lower(&hir);
    Ok(AnalyzedProject {
        manifest,
        entry_path: entry,
        mir,
        modules,
    })
}

fn validate_workspace_root_lockfile(
    graph: &PackageGraph,
    project_root: &Path,
) -> Result<(), Diagnostic> {
    let manifest = graph.context(project_root)?.manifest.clone();
    if manifest.is_workspace_only() {
        validate_lockfile(project_root, &manifest)?;
    }
    Ok(())
}

fn load_package_graph(project_root: &Path) -> Result<PackageGraph, Diagnostic> {
    let mut graph = PackageGraph::default();
    let mut visiting = Vec::new();
    load_package_graph_recursive(project_root, &mut graph, &mut visiting)?;
    register_stdlib_package(&mut graph);
    Ok(graph)
}

/// Registers the synthetic `<stdlib>` package in the graph. The synthetic
/// manifest enables every capability so `validate_module_capabilities` does not
/// reject stdlib wrappers against their own package config; actual capability
/// enforcement still runs on the flattened program via
/// `hir::lower_with_capabilities`, which uses the **entry** package's
/// capabilities. That keeps stdlib wrappers transparent for capability rules:
/// an import of `std/time.ax` does not grant clock access unless the importing
/// package's manifest also declares `[capabilities] clock = true`.
fn register_stdlib_package(graph: &mut PackageGraph) {
    let root = stdlib::stdlib_root();
    if graph.packages.contains_key(&root) {
        return;
    }
    let manifest = Manifest {
        package: Some(PackageSection {
            name: stdlib::STDLIB_PACKAGE_NAME.to_string(),
            version: stdlib::STDLIB_PACKAGE_VERSION.to_string(),
        }),
        dependencies: BTreeMap::new(),
        workspace: None,
        build: BuildSection {
            entry: String::from("lib.ax"),
            out_dir: String::from("dist"),
        },
        tests: Vec::new(),
        capabilities: CapabilityConfig {
            fs: true,
            fs_write: true,
            fs_root: None,
            net: true,
            process: true,
            env: true,
            env_vars: Vec::new(),
            env_unrestricted: true,
            env_legacy_unrestricted: false,
            clock: true,
            crypto: true,
            ffi: false,
        },
    };
    graph.packages.insert(
        root.clone(),
        PackageContext {
            root: root.clone(),
            manifest,
            source_root: root,
            dependencies: BTreeMap::new(),
            workspace_members: Vec::new(),
        },
    );
}

fn load_package_graph_recursive(
    project_root: &Path,
    graph: &mut PackageGraph,
    visiting: &mut Vec<PathBuf>,
) -> Result<(), Diagnostic> {
    let project_root = normalize_path(project_root);
    let project_root = canonicalize_existing_path(&project_root, "project root")?;
    if graph.packages.contains_key(&project_root) {
        return Ok(());
    }
    if visiting.contains(&project_root) {
        return Err(Diagnostic::new(
            "manifest",
            format!("dependency cycle detected at {}", project_root.display()),
        )
        .with_path(manifest_path(&project_root).display().to_string()));
    }
    let manifest = load_manifest(&project_root)?;
    let workspace_members = resolve_workspace_members(&project_root, &manifest)?;
    let source_root = if manifest.package.is_some() {
        entry_path(&project_root, &manifest)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| project_root.clone())
    } else {
        let src_root = project_root.join("src");
        if src_root.exists() {
            src_root
        } else {
            project_root.clone()
        }
    };
    let source_root = if source_root.exists() {
        canonicalize_package_path(
            &source_root,
            &project_root,
            "manifest",
            "build.entry source root resolves outside the package",
        )?
    } else {
        source_root
    };
    visiting.push(project_root.clone());
    let mut dependencies = BTreeMap::new();
    for (name, spec) in &manifest.dependencies {
        let dependency_root = normalize_path(project_root.join(&spec.path));
        if !dependency_root.exists() {
            return Err(Diagnostic::new(
                "manifest",
                format!(
                    "dependency {name:?} is missing at {}",
                    dependency_root.display()
                ),
            )
            .with_path(manifest_path(&project_root).display().to_string()));
        }
        let dependency_root = canonicalize_existing_path(&dependency_root, "dependency path")?;
        load_package_graph_recursive(&dependency_root, graph, visiting)?;
        dependencies.insert(name.clone(), dependency_root);
    }
    for member_root in &workspace_members {
        load_package_graph_recursive(member_root, graph, visiting)?;
    }
    visiting.pop();
    graph.packages.insert(
        project_root.clone(),
        PackageContext {
            root: project_root,
            manifest,
            source_root,
            dependencies,
            workspace_members,
        },
    );
    Ok(())
}

fn resolve_workspace_members(
    project_root: &Path,
    manifest: &Manifest,
) -> Result<Vec<PathBuf>, Diagnostic> {
    let mut members = Vec::new();
    let mut seen = HashSet::new();
    for (index, member) in manifest
        .workspace
        .as_ref()
        .into_iter()
        .flat_map(|workspace| workspace.members.iter())
        .enumerate()
    {
        if member.trim().is_empty() {
            return Err(
                Diagnostic::new("manifest", "workspace member paths must not be empty")
                    .with_path(manifest_path(project_root).display().to_string()),
            );
        }
        let candidate = Path::new(member);
        if candidate.is_absolute() {
            return Err(Diagnostic::new(
                "manifest",
                format!("workspace.members[{index}] must be relative"),
            )
            .with_path(manifest_path(project_root).display().to_string()));
        }
        if candidate
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(Diagnostic::new(
                "manifest",
                format!("workspace.members[{index}] must not use parent traversal"),
            )
            .with_path(manifest_path(project_root).display().to_string()));
        }
        let member_root = normalize_path(project_root.join(member));
        if member_root == project_root {
            return Err(Diagnostic::new(
                "manifest",
                "workspace members must not include the root package",
            )
            .with_path(manifest_path(project_root).display().to_string()));
        }
        if !member_root.exists() {
            return Err(Diagnostic::new(
                "manifest",
                format!(
                    "workspace member {member:?} is missing at {}",
                    member_root.display()
                ),
            )
            .with_path(manifest_path(project_root).display().to_string()));
        }
        let member_root = canonicalize_existing_path(&member_root, "workspace member path")?;
        if !member_root.starts_with(project_root) {
            return Err(Diagnostic::new(
                "manifest",
                format!("workspace member {member:?} resolves outside the workspace root"),
            )
            .with_path(manifest_path(project_root).display().to_string()));
        }
        let member_manifest = manifest_path(&member_root);
        if !member_manifest.exists() {
            return Err(Diagnostic::new(
                "manifest",
                format!("workspace member {member:?} is missing axiom.toml"),
            )
            .with_path(manifest_path(project_root).display().to_string()));
        }
        if seen.insert(member_root.clone()) {
            members.push(member_root);
        }
    }
    Ok(members)
}

#[derive(Debug, Clone)]
struct BuildArtifactReport {
    cache_status: BuildCacheStatus,
    compile_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct BuildCacheFile {
    version: u32,
    backend: NativeBackendKind,
    compiler: String,
    target: Option<String>,
    debug: bool,
    manifest_hash: String,
    lockfile_hash: String,
    rust_hash: String,
    binary_hash: Option<String>,
    modules: Vec<CachedModule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CachedModule {
    path: String,
    source_hash: String,
    imports: Vec<String>,
}

fn build_artifacts(
    graph: &PackageGraph,
    package_root: &Path,
    analyzed: &AnalyzedProject,
    generated_rust: &Path,
    binary: &Path,
    resolved_target: Option<&str>,
    options: &BuildOptions,
) -> Result<BuildArtifactReport, Diagnostic> {
    ensure_output_path_stays_inside_package(package_root, generated_rust, "generated Rust output")?;
    ensure_output_path_stays_inside_package(package_root, binary, "binary output")?;
    if let Some(parent) = generated_rust.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            Diagnostic::new(
                "build",
                format!("failed to create {}: {err}", parent.display()),
            )
        })?;
    }
    if let Some(parent) = binary.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            Diagnostic::new(
                "build",
                format!("failed to create {}: {err}", parent.display()),
            )
        })?;
    }
    let fs_root = fs_root_path_for_package(package_root, &analyzed.manifest)?;
    let rust_source = render_rust_for_package_with_capabilities(
        &analyzed.mir,
        options.debug,
        package_root,
        &fs_root,
        &analyzed.manifest.capabilities,
    );
    let cache = build_cache_file(
        graph,
        package_root,
        analyzed,
        &rust_source,
        options.backend,
        resolved_target.map(str::to_string),
        options.debug,
    )?;
    let cache_path = build_cache_path(generated_rust);
    if read_build_cache(&cache_path)
        .as_ref()
        .is_some_and(|stored| cache_matches(stored, &cache, generated_rust, binary))
    {
        if options.debug {
            write_debug_source_map(generated_rust, &debug_source_map_path(generated_rust))?;
        }
        return Ok(BuildArtifactReport {
            cache_status: BuildCacheStatus::Hit,
            compile_ms: 0,
        });
    }
    fs::write(generated_rust, rust_source).map_err(|err| {
        Diagnostic::new(
            "build",
            format!("failed to write {}: {err}", generated_rust.display()),
        )
    })?;
    if options.debug {
        write_debug_source_map(generated_rust, &debug_source_map_path(generated_rust))?;
    }
    let started = Instant::now();
    compile_native(
        options.backend,
        generated_rust,
        binary,
        resolved_target,
        options.debug,
    )?;
    let compile_ms = started.elapsed().as_millis() as u64;
    let mut cache = cache;
    cache.binary_hash = Some(hash_file_bytes(binary)?);
    write_build_cache(&cache_path, &cache)?;
    Ok(BuildArtifactReport {
        cache_status: BuildCacheStatus::Miss,
        compile_ms,
    })
}

fn build_cache_path(generated_rust: &Path) -> PathBuf {
    generated_rust.with_extension("build-cache.toml")
}

fn debug_source_map_path(generated_rust: &Path) -> PathBuf {
    generated_rust.with_extension("debug-map.json")
}

#[derive(Debug, Serialize)]
struct DebugSourceMap<'a> {
    schema_version: &'static str,
    generated_rust: &'a str,
    mappings: Vec<DebugSourceMapping>,
}

#[derive(Debug, Serialize)]
struct DebugSourceMapping {
    generated_line: usize,
    source: String,
    line: usize,
    column: usize,
}

fn write_debug_source_map(generated_rust: &Path, debug_map: &Path) -> Result<(), Diagnostic> {
    let generated_source = fs::read_to_string(generated_rust).map_err(|err| {
        Diagnostic::new(
            "build",
            format!("failed to read {}: {err}", generated_rust.display()),
        )
    })?;
    let generated_rust_path = generated_rust.display().to_string();
    let map = DebugSourceMap {
        schema_version: "axiom.stage1.debug_map.v1",
        generated_rust: &generated_rust_path,
        mappings: debug_source_mappings(&generated_source),
    };
    let content = serde_json::to_string_pretty(&map).map_err(|err| {
        Diagnostic::new("build", format!("failed to render debug source map: {err}"))
    })?;
    fs::write(debug_map, format!("{content}\n")).map_err(|err| {
        Diagnostic::new(
            "build",
            format!("failed to write {}: {err}", debug_map.display()),
        )
    })
}

fn debug_source_mappings(generated_source: &str) -> Vec<DebugSourceMapping> {
    generated_source
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let marker = line.trim_start().strip_prefix("// axiom-source: ")?;
            let (source, source_line, source_column) = parse_debug_source_marker(marker)?;
            Some(DebugSourceMapping {
                generated_line: index + 2,
                source,
                line: source_line,
                column: source_column,
            })
        })
        .collect()
}

fn parse_debug_source_marker(marker: &str) -> Option<(String, usize, usize)> {
    let mut parts = marker.rsplitn(3, ':');
    let column = parts.next()?.parse().ok()?;
    let line = parts.next()?.parse().ok()?;
    let source = parts.next()?.to_string();
    Some((source, line, column))
}

fn read_build_cache(path: &Path) -> Option<BuildCacheFile> {
    let content = fs::read_to_string(path).ok()?;
    toml::from_str(&content).ok()
}

fn cache_matches(
    stored: &BuildCacheFile,
    expected: &BuildCacheFile,
    generated_rust: &Path,
    binary: &Path,
) -> bool {
    let Some(binary_hash) = stored.binary_hash.as_ref() else {
        return false;
    };
    let Ok(generated_rust_source) = fs::read_to_string(generated_rust) else {
        return false;
    };
    let Ok(actual_binary_hash) = hash_file_bytes(binary) else {
        return false;
    };
    let mut stored_key = stored.clone();
    let mut expected_key = expected.clone();
    stored_key.binary_hash = None;
    expected_key.binary_hash = None;
    stored_key == expected_key
        && hash_text(&generated_rust_source) == expected.rust_hash
        && actual_binary_hash == *binary_hash
}

fn write_build_cache(path: &Path, cache: &BuildCacheFile) -> Result<(), Diagnostic> {
    let content = toml::to_string_pretty(cache).map_err(|err| {
        Diagnostic::new(
            "build",
            format!("failed to render build cache metadata: {err}"),
        )
    })?;
    fs::write(path, content).map_err(|err| {
        Diagnostic::new(
            "build",
            format!("failed to write {}: {err}", path.display()),
        )
    })
}

fn build_cache_file(
    graph: &PackageGraph,
    package_root: &Path,
    analyzed: &AnalyzedProject,
    rust_source: &str,
    backend: NativeBackendKind,
    target: Option<String>,
    debug: bool,
) -> Result<BuildCacheFile, Diagnostic> {
    Ok(BuildCacheFile {
        version: BUILD_CACHE_VERSION,
        backend,
        compiler: format!("{}-{}", BUILD_CACHE_COMPILER, backend),
        target,
        debug,
        manifest_hash: hash_file(&manifest_path(package_root))?,
        lockfile_hash: hash_file(&crate::manifest::lockfile_path(package_root))?,
        rust_hash: hash_text(rust_source),
        binary_hash: None,
        modules: cached_modules(graph, &analyzed.modules)?,
    })
}

fn cached_modules(
    graph: &PackageGraph,
    modules: &[LoadedModule],
) -> Result<Vec<CachedModule>, Diagnostic> {
    modules
        .iter()
        .map(|module| {
            let source = module_source(&module.path)?;
            let mut imports = module
                .program
                .imports
                .iter()
                .map(|import| {
                    resolve_import_path(graph, &module.package_root, &module.path, import)
                        .map(|(_, path)| path.display().to_string())
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            imports.sort();
            Ok(CachedModule {
                path: module.path.display().to_string(),
                source_hash: hash_text(&source),
                imports,
            })
        })
        .collect()
}

fn module_source(module_path: &Path) -> Result<String, Diagnostic> {
    if stdlib::is_stdlib_path(module_path) {
        return stdlib::stdlib_source_for(module_path)
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
            });
    }
    fs::read_to_string(module_path).map_err(|err| {
        Diagnostic::new(
            "source",
            format!("failed to read {}: {err}", module_path.display()),
        )
        .with_path(module_path.display().to_string())
    })
}

fn hash_file(path: &Path) -> Result<String, Diagnostic> {
    let content = fs::read_to_string(path).map_err(|err| {
        Diagnostic::new("build", format!("failed to read {}: {err}", path.display()))
            .with_path(path.display().to_string())
    })?;
    Ok(hash_text(&content))
}

fn hash_file_bytes(path: &Path) -> Result<String, Diagnostic> {
    let content = fs::read(path).map_err(|err| {
        Diagnostic::new("build", format!("failed to read {}: {err}", path.display()))
            .with_path(path.display().to_string())
    })?;
    Ok(hash_bytes(&content))
}

fn hash_text(value: &str) -> String {
    hash_bytes(value.as_bytes())
}

fn hash_bytes(value: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn run_test_case(
    project_root: &Path,
    graph: &PackageGraph,
    manifest: &Manifest,
    test: &crate::manifest::TestTarget,
) -> TestCaseResult {
    let started = Instant::now();
    let entry_path = project_root.join(&test.entry);
    let generated_rust = match test_generated_rust_path(project_root, manifest, &test.name) {
        Ok(path) => path,
        Err(error) => {
            return failed_test_case_result(project_root, test, &started, error, None, None);
        }
    };
    let binary = match test_binary_path(project_root, manifest, &test.name) {
        Ok(path) => path,
        Err(error) => {
            return failed_test_case_result(
                project_root,
                test,
                &started,
                error,
                Some(generated_rust.display().to_string()),
                None,
            );
        }
    };
    let analyzed = match analyze_entry(graph, project_root, manifest.clone(), entry_path.clone()) {
        Ok(analyzed) => analyzed,
        Err(error) => {
            return failed_test_case_result(project_root, test, &started, error, None, None);
        }
    };
    if let Err(error) = build_artifacts(
        graph,
        project_root,
        &analyzed,
        &generated_rust,
        &binary,
        None,
        &BuildOptions::default(),
    ) {
        return TestCaseResult {
            package_root: project_root.display().to_string(),
            name: test.name.clone(),
            kind: test.kind,
            entry: test.entry.clone(),
            ok: false,
            binary: Some(binary.display().to_string()),
            generated_rust: Some(generated_rust.display().to_string()),
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            expected_stdout: test.stdout.clone(),
            expected_error: None,
            duration_ms: started.elapsed().as_millis() as u64,
            error: Some(error),
        };
    }

    let build_output_dir = out_dir_path(project_root, manifest);
    match command_for_build_output(&binary, &build_output_dir)
        .and_then(|mut command| command.output())
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code();
            let error = if !output.status.success() {
                let detail = stderr.trim();
                Some(
                    Diagnostic::new(
                        "test",
                        if detail.is_empty() {
                            format!(
                                "test {:?} exited with status {}",
                                test.name,
                                exit_code.unwrap_or(1)
                            )
                        } else {
                            format!(
                                "test {:?} exited with status {}: {}",
                                test.name,
                                exit_code.unwrap_or(1),
                                detail
                            )
                        },
                    )
                    .with_path(entry_path.display().to_string()),
                )
            } else if let Some(expected_stdout) = &test.stdout {
                if &stdout != expected_stdout {
                    Some(
                        Diagnostic::new(
                            "test",
                            format!(
                                "test {:?} stdout did not match expected output: expected {:?}, got {:?}",
                                test.name, expected_stdout, stdout
                            ),
                        )
                        .with_path(entry_path.display().to_string()),
                    )
                } else {
                    None
                }
            } else {
                None
            };
            TestCaseResult {
                package_root: project_root.display().to_string(),
                name: test.name.clone(),
                kind: test.kind,
                entry: test.entry.clone(),
                ok: error.is_none(),
                binary: Some(binary.display().to_string()),
                generated_rust: Some(generated_rust.display().to_string()),
                exit_code,
                stdout,
                stderr,
                expected_stdout: test.stdout.clone(),
                expected_error: None,
                duration_ms: started.elapsed().as_millis() as u64,
                error,
            }
        }
        Err(err) => TestCaseResult {
            package_root: project_root.display().to_string(),
            name: test.name.clone(),
            kind: test.kind,
            entry: test.entry.clone(),
            ok: false,
            binary: Some(binary.display().to_string()),
            generated_rust: Some(generated_rust.display().to_string()),
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            expected_stdout: test.stdout.clone(),
            expected_error: None,
            duration_ms: started.elapsed().as_millis() as u64,
            error: Some(
                Diagnostic::new(
                    "test",
                    format!("failed to execute {}: {err}", binary.display()),
                )
                .with_path(entry_path.display().to_string()),
            ),
        },
    }
}

fn run_compile_fail_case(
    project_root: &Path,
    graph: &PackageGraph,
    manifest: &Manifest,
    case_name: &str,
) -> TestCaseResult {
    let started = Instant::now();
    let expected = match load_expected_error(project_root) {
        Ok(expected) => expected,
        Err(error) => {
            return TestCaseResult {
                package_root: project_root.display().to_string(),
                name: case_name.to_string(),
                kind: TestKind::Unit,
                entry: manifest.build.entry.clone(),
                ok: false,
                binary: None,
                generated_rust: None,
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                expected_stdout: None,
                expected_error: None,
                duration_ms: started.elapsed().as_millis() as u64,
                error: Some(error),
            };
        }
    };
    let entry_path = project_root.join(&manifest.build.entry);
    let actual = match analyze_entry(graph, project_root, manifest.clone(), entry_path.clone()) {
        Ok(_) => {
            return TestCaseResult {
                package_root: project_root.display().to_string(),
                name: case_name.to_string(),
                kind: TestKind::Unit,
                entry: manifest.build.entry.clone(),
                ok: false,
                binary: None,
                generated_rust: None,
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                expected_stdout: None,
                expected_error: Some(expected),
                duration_ms: started.elapsed().as_millis() as u64,
                error: Some(
                    Diagnostic::new(
                        "test",
                        format!("compile-fail fixture {case_name:?} checked successfully"),
                    )
                    .with_path(entry_path.display().to_string()),
                ),
            };
        }
        Err(error) => diagnostic_with_default_path(error, &entry_path),
    };
    let mismatch = expected_error_mismatch(project_root, &expected, &actual);
    TestCaseResult {
        package_root: project_root.display().to_string(),
        name: case_name.to_string(),
        kind: TestKind::Unit,
        entry: manifest.build.entry.clone(),
        ok: mismatch.is_none(),
        binary: None,
        generated_rust: None,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        expected_stdout: None,
        expected_error: Some(expected),
        duration_ms: started.elapsed().as_millis() as u64,
        error: mismatch.map(|message| {
            Diagnostic::new("test", message)
                .with_path(expected_error_path(project_root).display().to_string())
        }),
    }
}

fn expected_error_path(project_root: &Path) -> PathBuf {
    project_root.join("expected-error.json")
}

fn load_expected_error(project_root: &Path) -> Result<ExpectedDiagnostic, Diagnostic> {
    let path = expected_error_path(project_root);
    let content = fs::read_to_string(&path).map_err(|err| {
        Diagnostic::new("test", format!("failed to read {}: {err}", path.display()))
            .with_path(path.display().to_string())
    })?;
    serde_json::from_str(&content).map_err(|err| {
        Diagnostic::new(
            "test",
            format!("invalid expected-error.json at {}: {err}", path.display()),
        )
        .with_path(path.display().to_string())
    })
}

fn expected_error_mismatch(
    project_root: &Path,
    expected: &ExpectedDiagnostic,
    actual: &Diagnostic,
) -> Option<String> {
    let actual_path = actual
        .path
        .as_deref()
        .map(|path| relative_diagnostic_path(project_root, path))
        .unwrap_or_default();
    let actual_line = actual.line.unwrap_or_default();
    let actual_column = actual.column.unwrap_or_default();
    if actual.kind == expected.kind
        && actual.code == expected.code
        && actual.message == expected.message
        && actual_path == expected.path
        && actual_line == expected.line
        && actual_column == expected.column
    {
        return None;
    }
    Some(format!(
        "compile-fail diagnostic mismatch: expected kind={:?} code={:?} message={:?} path={:?} line={} column={}, got kind={:?} code={:?} message={:?} path={:?} line={} column={}",
        expected.kind,
        expected.code,
        expected.message,
        expected.path,
        expected.line,
        expected.column,
        actual.kind,
        actual.code,
        actual.message,
        actual_path,
        actual_line,
        actual_column,
    ))
}

fn relative_diagnostic_path(project_root: &Path, path: &str) -> String {
    let path = normalize_path(path);
    path.strip_prefix(project_root)
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

fn diagnostic_with_default_path(mut diagnostic: Diagnostic, path: &Path) -> Diagnostic {
    if diagnostic.path.is_none() {
        diagnostic.path = Some(path.display().to_string());
    }
    diagnostic
}

#[cfg(test)]
pub(crate) fn command_for_executable(path: impl AsRef<Path>) -> io::Result<Command> {
    // Relative executable names are expanded to absolute paths before Command creation, avoiding
    // ambient PATH lookup for generated binaries.
    Ok(Command::new(executable_path(path)?))
}

pub(crate) fn command_for_build_output(
    path: impl AsRef<Path>,
    build_output_dir: impl AsRef<Path>,
) -> io::Result<Command> {
    let executable = executable_path(path)?;
    let build_output_dir = executable_path(build_output_dir)?;
    if !executable.starts_with(&build_output_dir) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "refusing to execute binary outside build output directory: {}",
                executable.display()
            ),
        ));
    }
    // The binary path is compiler-generated and constrained to the package build output directory
    // before it reaches Command, so execution does not depend on ambient PATH lookup.
    Ok(Command::new(executable))
}

fn executable_path(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    let path = path.as_ref();
    if path.is_absolute() {
        return Ok(normalize_path(path));
    }
    Ok(normalize_path(env::current_dir()?.join(path)))
}

fn failed_test_case_result(
    project_root: &Path,
    test: &crate::manifest::TestTarget,
    started: &Instant,
    error: Diagnostic,
    generated_rust: Option<String>,
    binary: Option<String>,
) -> TestCaseResult {
    TestCaseResult {
        package_root: project_root.display().to_string(),
        name: test.name.clone(),
        kind: test.kind,
        entry: test.entry.clone(),
        ok: false,
        binary,
        generated_rust,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        expected_stdout: test.stdout.clone(),
        expected_error: None,
        duration_ms: started.elapsed().as_millis() as u64,
        error: Some(error),
    }
}

fn workspace_package_roots(
    graph: &PackageGraph,
    project_root: &Path,
    selected_package: Option<&str>,
) -> Result<Vec<PathBuf>, Diagnostic> {
    let mut roots = Vec::new();
    let mut seen = BTreeSet::new();
    collect_workspace_package_roots(graph, project_root, &mut seen, &mut roots)?;
    if let Some(selected_package) = selected_package {
        let matched = roots
            .into_iter()
            .filter(|root| {
                graph
                    .context(root)
                    .ok()
                    .and_then(|package| package.manifest.package.as_ref())
                    .map(|package| package.name == selected_package)
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        if matched.is_empty() {
            return Err(Diagnostic::new(
                "manifest",
                format!("workspace package {selected_package:?} was not found"),
            )
            .with_path(manifest_path(project_root).display().to_string()));
        }
        if matched.len() > 1 {
            return Err(Diagnostic::new(
                "manifest",
                format!("workspace package name {selected_package:?} is ambiguous"),
            )
            .with_path(manifest_path(project_root).display().to_string()));
        }
        return Ok(matched);
    }
    Ok(roots)
}

fn collect_workspace_package_roots(
    graph: &PackageGraph,
    package_root: &Path,
    seen: &mut BTreeSet<PathBuf>,
    roots: &mut Vec<PathBuf>,
) -> Result<(), Diagnostic> {
    let package_root = normalize_path(package_root);
    if !seen.insert(package_root.clone()) {
        return Ok(());
    }
    let package = graph.context(&package_root)?;
    if package.manifest.package.is_some() {
        roots.push(package_root.clone());
    }
    for member in &package.workspace_members {
        collect_workspace_package_roots(graph, member, seen, roots)?;
    }
    Ok(())
}

fn test_generated_rust_path(
    project_root: &Path,
    manifest: &Manifest,
    test_name: &str,
) -> Result<PathBuf, Diagnostic> {
    Ok(out_dir_path(project_root, manifest)
        .join("tests")
        .join(format!(
            "{}.generated.rs",
            test_artifact_name(project_root, manifest, test_name)?
        )))
}

fn test_binary_path(
    project_root: &Path,
    manifest: &Manifest,
    test_name: &str,
) -> Result<PathBuf, Diagnostic> {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    Ok(out_dir_path(project_root, manifest)
        .join("tests")
        .join(format!(
            "{}{}",
            test_artifact_name(project_root, manifest, test_name)?,
            suffix
        )))
}

fn test_artifact_name(
    project_root: &Path,
    manifest: &Manifest,
    test_name: &str,
) -> Result<String, Diagnostic> {
    let package = package_section(
        manifest,
        "test artifacts require a package manifest",
        &manifest_path(project_root),
    )?;
    Ok(format!("{}-{}", package.name, slugify_test_name(test_name)))
}

fn slugify_test_name(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        String::from("test")
    } else {
        trimmed.to_string()
    }
}

fn test_matches_filter(test: &crate::manifest::TestTarget, filter: &str) -> bool {
    test.name.contains(filter) || test.entry.contains(filter)
}

fn load_modules(
    graph: &PackageGraph,
    package_root: &Path,
    entry_path: &Path,
) -> Result<Vec<LoadedModule>, Diagnostic> {
    let mut ordered = Vec::new();
    let mut loaded = HashMap::new();
    let mut visiting = Vec::new();
    load_module_recursive(
        graph,
        package_root,
        entry_path,
        true,
        &mut ordered,
        &mut loaded,
        &mut visiting,
    )?;
    Ok(ordered)
}

fn load_module_recursive(
    graph: &PackageGraph,
    package_root: &Path,
    module_path: &Path,
    is_entry: bool,
    ordered: &mut Vec<LoadedModule>,
    loaded: &mut HashMap<PathBuf, ()>,
    visiting: &mut Vec<PathBuf>,
) -> Result<(), Diagnostic> {
    let module_path = normalize_path(module_path);
    if visiting.contains(&module_path) {
        return Err(Diagnostic::new(
            "import",
            format!("circular import detected at {}", module_path.display()),
        )
        .with_path(module_path.display().to_string()));
    }
    if loaded.contains_key(&module_path) {
        return Ok(());
    }
    let package = graph.context(package_root)?;

    let source = if stdlib::is_stdlib_path(&module_path) {
        stdlib::stdlib_source_for(&module_path)
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
        fs::read_to_string(&module_path).map_err(|err| {
            Diagnostic::new(
                "source",
                format!("failed to read {}: {err}", module_path.display()),
            )
            .with_path(module_path.display().to_string())
        })?
    };
    let program = syntax::parse_program(&source, &module_path)?;
    if !is_entry && !program.stmts.is_empty() {
        let stmt = &program.stmts[0];
        return Err(Diagnostic::new(
            "import",
            "imported stage1 modules may only contain imports, const declarations, type alias declarations, struct declarations, enum declarations, and function declarations",
        )
        .with_path(module_path.display().to_string())
        .with_span(stmt_line(stmt), stmt_column(stmt)));
    }

    visiting.push(module_path.clone());
    for import in &program.imports {
        let (import_package_root, import_path) =
            resolve_import_path(graph, package_root, &module_path, import)?;
        load_module_recursive(
            graph,
            &import_package_root,
            &import_path,
            false,
            ordered,
            loaded,
            visiting,
        )?;
    }
    visiting.pop();

    loaded.insert(module_path.clone(), ());
    let package_name = package_section(
        &package.manifest,
        "loaded modules require a package manifest",
        &manifest_path(&package.root),
    )?
    .name
    .clone();
    ordered.push(LoadedModule {
        path: module_path,
        program,
        is_entry,
        package_root: package.root.clone(),
        source_root: package.source_root.clone(),
        package_name,
    });
    Ok(())
}

fn package_section<'a>(
    manifest: &'a Manifest,
    message: &'static str,
    manifest_file: &Path,
) -> Result<&'a PackageSection, Diagnostic> {
    manifest.package.as_ref().ok_or_else(|| {
        Diagnostic::new("manifest", message).with_path(manifest_file.display().to_string())
    })
}

fn validate_module_capabilities(
    graph: &PackageGraph,
    modules: &[LoadedModule],
) -> Result<(), Diagnostic> {
    for module in modules {
        let package = graph.context(&module.package_root)?;
        validate_program_capabilities(
            &module.path,
            &module.program,
            &package.manifest.capabilities,
        )?;
    }
    Ok(())
}

fn validate_program_capabilities(
    module_path: &Path,
    program: &syntax::Program,
    capabilities: &CapabilityConfig,
) -> Result<(), Diagnostic> {
    for function in &program.functions {
        for stmt in &function.body {
            validate_stmt_capabilities(module_path, stmt, capabilities)?;
        }
    }
    for stmt in &program.stmts {
        validate_stmt_capabilities(module_path, stmt, capabilities)?;
    }
    Ok(())
}

fn validate_stmt_capabilities(
    module_path: &Path,
    stmt: &syntax::Stmt,
    capabilities: &CapabilityConfig,
) -> Result<(), Diagnostic> {
    match stmt {
        syntax::Stmt::Let { expr, .. }
        | syntax::Stmt::Print { expr, .. }
        | syntax::Stmt::Panic { expr, .. }
        | syntax::Stmt::Defer { expr, .. }
        | syntax::Stmt::Return { expr, .. } => {
            validate_expr_capabilities(module_path, expr, capabilities)?;
        }
        syntax::Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            validate_expr_capabilities(module_path, cond, capabilities)?;
            for stmt in then_block {
                validate_stmt_capabilities(module_path, stmt, capabilities)?;
            }
            if let Some(else_block) = else_block {
                for stmt in else_block {
                    validate_stmt_capabilities(module_path, stmt, capabilities)?;
                }
            }
        }
        syntax::Stmt::While { cond, body, .. } => {
            validate_expr_capabilities(module_path, cond, capabilities)?;
            for stmt in body {
                validate_stmt_capabilities(module_path, stmt, capabilities)?;
            }
        }
        syntax::Stmt::Match { expr, arms, .. } => {
            validate_expr_capabilities(module_path, expr, capabilities)?;
            for arm in arms {
                for stmt in &arm.body {
                    validate_stmt_capabilities(module_path, stmt, capabilities)?;
                }
            }
        }
    }
    Ok(())
}

fn validate_expr_capabilities(
    module_path: &Path,
    expr: &syntax::Expr,
    capabilities: &CapabilityConfig,
) -> Result<(), Diagnostic> {
    match expr {
        syntax::Expr::Literal(_) | syntax::Expr::VarRef { .. } => Ok(()),
        syntax::Expr::Call {
            name,
            type_args: _,
            args,
            line,
            column,
        } => {
            if let Some(kind) = intrinsic_capability(name)
                && !capabilities.enabled(kind)
            {
                let requirement = if kind == CapabilityKind::Env {
                    String::from("[capabilities].env = [\"NAME\"] or env_unrestricted = true")
                } else {
                    format!("[capabilities].{} = true", kind.name())
                };
                return Err(Diagnostic::new(
                    "capability",
                    format!("call to {name:?} requires {requirement}"),
                )
                .with_path(module_path.display().to_string())
                .with_span(*line, *column));
            }
            for arg in args {
                validate_expr_capabilities(module_path, arg, capabilities)?;
            }
            Ok(())
        }
        syntax::Expr::MethodCall { base, args, .. } => {
            validate_expr_capabilities(module_path, base, capabilities)?;
            for arg in args {
                validate_expr_capabilities(module_path, arg, capabilities)?;
            }
            Ok(())
        }
        syntax::Expr::BinaryAdd { lhs, rhs, .. } | syntax::Expr::BinaryCompare { lhs, rhs, .. } => {
            validate_expr_capabilities(module_path, lhs, capabilities)?;
            validate_expr_capabilities(module_path, rhs, capabilities)
        }
        syntax::Expr::Try { expr, .. } | syntax::Expr::Await { expr, .. } => {
            validate_expr_capabilities(module_path, expr, capabilities)
        }
        syntax::Expr::StructLiteral { fields, .. } => {
            for field in fields {
                validate_expr_capabilities(module_path, &field.expr, capabilities)?;
            }
            Ok(())
        }
        syntax::Expr::FieldAccess { base, .. } | syntax::Expr::TupleIndex { base, .. } => {
            validate_expr_capabilities(module_path, base, capabilities)
        }
        syntax::Expr::TupleLiteral { elements, .. }
        | syntax::Expr::ArrayLiteral { elements, .. } => {
            for element in elements {
                validate_expr_capabilities(module_path, element, capabilities)?;
            }
            Ok(())
        }
        syntax::Expr::MapLiteral { entries, .. } => {
            for entry in entries {
                validate_expr_capabilities(module_path, &entry.key, capabilities)?;
                validate_expr_capabilities(module_path, &entry.value, capabilities)?;
            }
            Ok(())
        }
        syntax::Expr::Slice {
            base, start, end, ..
        } => {
            validate_expr_capabilities(module_path, base, capabilities)?;
            if let Some(start) = start {
                validate_expr_capabilities(module_path, start, capabilities)?;
            }
            if let Some(end) = end {
                validate_expr_capabilities(module_path, end, capabilities)?;
            }
            Ok(())
        }
        syntax::Expr::Index { base, index, .. } => {
            validate_expr_capabilities(module_path, base, capabilities)?;
            validate_expr_capabilities(module_path, index, capabilities)
        }
    }
}

fn intrinsic_capability(name: &str) -> Option<CapabilityKind> {
    match name {
        "fs_read" => Some(CapabilityKind::Fs),
        "fs_write" | "fs_create" | "fs_append" | "fs_mkdir" | "fs_mkdir_all" | "fs_remove_file"
        | "fs_remove_dir" | "fs_replace" => Some(CapabilityKind::FsWrite),
        "net_resolve" => Some(CapabilityKind::Net),
        "net_tcp_listen_loopback_once" => Some(CapabilityKind::Net),
        "net_tcp_dial" => Some(CapabilityKind::Net),
        "net_udp_bind_loopback_once" => Some(CapabilityKind::Net),
        "net_udp_send_recv" => Some(CapabilityKind::Net),
        "http_get" => Some(CapabilityKind::Net),
        "http_serve_once" => Some(CapabilityKind::Net),
        "http_serve_route" => Some(CapabilityKind::Net),
        "process_status" => Some(CapabilityKind::Process),
        "clock_now_ms" => Some(CapabilityKind::Clock),
        "clock_elapsed_ms" => Some(CapabilityKind::Clock),
        "clock_sleep_ms" => Some(CapabilityKind::Clock),
        "env_get" => Some(CapabilityKind::Env),
        "crypto_sha256" => Some(CapabilityKind::Crypto),
        "crypto_hmac_sha256" => Some(CapabilityKind::Crypto),
        "crypto_constant_time_eq" => Some(CapabilityKind::Crypto),
        _ => None,
    }
}

fn flatten_modules(
    graph: &PackageGraph,
    modules: &[LoadedModule],
) -> Result<syntax::Program, Diagnostic> {
    let mut symbols = HashMap::new();
    for module in modules {
        symbols.insert(module.path.clone(), build_module_symbols(module)?);
    }

    let mut flattened_functions = Vec::new();
    let mut flattened_type_aliases = Vec::new();
    let mut flattened_structs = Vec::new();
    let mut flattened_enums = Vec::new();
    let mut flattened_stmts = Vec::new();
    for module in modules {
        let Some(module_symbols) = symbols.get(&module.path) else {
            continue;
        };
        let mut visible_functions = module_symbols.functions.clone();
        let mut visible_consts = module_symbols.consts.clone();
        let mut visible_aliases = module_symbols.aliases.clone();
        let mut visible_structs = module_symbols.structs.clone();
        let mut visible_enums = module_symbols.enums.clone();
        let mut private_imported = HashSet::new();
        let mut private_imported_consts = HashSet::new();
        let mut private_imported_types = HashSet::new();
        for import in &module.program.imports {
            let (import_package_root, import_path) =
                resolve_import_path(graph, &module.package_root, &module.path, import)?;
            let same_package = import_package_root == module.package_root;
            let imported_symbols = symbols.get(&import_path).ok_or_else(|| {
                Diagnostic::new(
                    "import",
                    format!("internal error: missing module {}", import_path.display()),
                )
            })?;
            for name in &imported_symbols.private_functions {
                private_imported.insert(name.clone());
            }
            for name in &imported_symbols.private_consts {
                private_imported_consts.insert(name.clone());
            }
            for (export_name, internal_name) in &imported_symbols.public_functions {
                if let Some(existing) = visible_functions.get(export_name)
                    && existing != internal_name
                {
                    return Err(Diagnostic::new(
                        "import",
                        format!("imported function {export_name:?} collides with an existing name"),
                    )
                    .with_path(module.path.display().to_string())
                    .with_span(import.line, import.column));
                }
                visible_functions.insert(export_name.clone(), internal_name.clone());
            }
            if same_package {
                for (export_name, internal_name) in &imported_symbols.package_functions {
                    if let Some(existing) = visible_functions.get(export_name)
                        && existing != internal_name
                    {
                        return Err(Diagnostic::new(
                            "import",
                            format!(
                                "imported function {export_name:?} collides with an existing name"
                            ),
                        )
                        .with_path(module.path.display().to_string())
                        .with_span(import.line, import.column));
                    }
                    visible_functions.insert(export_name.clone(), internal_name.clone());
                }
            } else {
                for name in imported_symbols.package_functions.keys() {
                    private_imported.insert(name.clone());
                }
            }
            for (export_name, const_decl) in &imported_symbols.public_consts {
                if visible_consts.contains_key(export_name) {
                    return Err(Diagnostic::new(
                        "import",
                        format!("imported const {export_name:?} collides with an existing name"),
                    )
                    .with_path(module.path.display().to_string())
                    .with_span(import.line, import.column));
                }
                visible_consts.insert(export_name.clone(), const_decl.clone());
            }
            if same_package {
                for (export_name, const_decl) in &imported_symbols.package_consts {
                    if visible_consts.contains_key(export_name) {
                        return Err(Diagnostic::new(
                            "import",
                            format!(
                                "imported const {export_name:?} collides with an existing name"
                            ),
                        )
                        .with_path(module.path.display().to_string())
                        .with_span(import.line, import.column));
                    }
                    visible_consts.insert(export_name.clone(), const_decl.clone());
                }
            } else {
                for name in imported_symbols.package_consts.keys() {
                    private_imported_consts.insert(name.clone());
                }
            }
            for name in &imported_symbols.private_structs {
                private_imported_types.insert(name.clone());
            }
            for name in &imported_symbols.private_enums {
                private_imported_types.insert(name.clone());
            }
            for name in &imported_symbols.private_aliases {
                private_imported_types.insert(name.clone());
            }
            for (export_name, internal_name) in &imported_symbols.public_aliases {
                if let Some(existing) = visible_aliases.get(export_name)
                    && existing != internal_name
                {
                    return Err(Diagnostic::new(
                        "import",
                        format!(
                            "imported type alias {export_name:?} collides with an existing name"
                        ),
                    )
                    .with_path(module.path.display().to_string())
                    .with_span(import.line, import.column));
                }
                visible_aliases.insert(export_name.clone(), internal_name.clone());
            }
            if same_package {
                for (export_name, internal_name) in &imported_symbols.package_aliases {
                    if let Some(existing) = visible_aliases.get(export_name)
                        && existing != internal_name
                    {
                        return Err(Diagnostic::new(
                            "import",
                            format!(
                                "imported type alias {export_name:?} collides with an existing name"
                            ),
                        )
                        .with_path(module.path.display().to_string())
                        .with_span(import.line, import.column));
                    }
                    visible_aliases.insert(export_name.clone(), internal_name.clone());
                }
            } else {
                for name in imported_symbols.package_aliases.keys() {
                    private_imported_types.insert(name.clone());
                }
            }
            for (export_name, internal_name) in &imported_symbols.public_structs {
                if let Some(existing) = visible_structs.get(export_name)
                    && existing != internal_name
                {
                    return Err(Diagnostic::new(
                        "import",
                        format!("imported struct {export_name:?} collides with an existing name"),
                    )
                    .with_path(module.path.display().to_string())
                    .with_span(import.line, import.column));
                }
                visible_structs.insert(export_name.clone(), internal_name.clone());
            }
            if same_package {
                for (export_name, internal_name) in &imported_symbols.package_structs {
                    if let Some(existing) = visible_structs.get(export_name)
                        && existing != internal_name
                    {
                        return Err(Diagnostic::new(
                            "import",
                            format!(
                                "imported struct {export_name:?} collides with an existing name"
                            ),
                        )
                        .with_path(module.path.display().to_string())
                        .with_span(import.line, import.column));
                    }
                    visible_structs.insert(export_name.clone(), internal_name.clone());
                }
            } else {
                for name in imported_symbols.package_structs.keys() {
                    private_imported_types.insert(name.clone());
                }
            }
            for (export_name, internal_name) in &imported_symbols.public_enums {
                if let Some(existing) = visible_enums.get(export_name)
                    && existing != internal_name
                {
                    return Err(Diagnostic::new(
                        "import",
                        format!("imported enum {export_name:?} collides with an existing name"),
                    )
                    .with_path(module.path.display().to_string())
                    .with_span(import.line, import.column));
                }
                visible_enums.insert(export_name.clone(), internal_name.clone());
            }
            if same_package {
                for (export_name, internal_name) in &imported_symbols.package_enums {
                    if let Some(existing) = visible_enums.get(export_name)
                        && existing != internal_name
                    {
                        return Err(Diagnostic::new(
                            "import",
                            format!("imported enum {export_name:?} collides with an existing name"),
                        )
                        .with_path(module.path.display().to_string())
                        .with_span(import.line, import.column));
                    }
                    visible_enums.insert(export_name.clone(), internal_name.clone());
                }
            } else {
                for name in imported_symbols.package_enums.keys() {
                    private_imported_types.insert(name.clone());
                }
            }
        }

        let visible_types = merge_visible_types(
            &visible_aliases,
            &visible_structs,
            &visible_enums,
            &module.path,
        )?;
        for const_decl in &module.program.consts {
            resolve_const_decl(
                const_decl,
                &visible_consts,
                &visible_functions,
                &visible_structs,
                &visible_types,
                &private_imported,
                &private_imported_consts,
                &private_imported_types,
                &module.path,
                &mut HashSet::new(),
            )?;
        }

        for type_alias in &module.program.type_aliases {
            flattened_type_aliases.push(rewrite_type_alias(
                type_alias,
                module_symbols,
                &visible_types,
                &private_imported_types,
                &module.path,
            )?);
        }

        for struct_decl in &module.program.structs {
            flattened_structs.push(rewrite_struct(
                struct_decl,
                module_symbols,
                &visible_types,
                &private_imported_types,
                &module.path,
            )?);
        }
        for enum_decl in &module.program.enums {
            flattened_enums.push(rewrite_enum(
                enum_decl,
                module_symbols,
                &visible_types,
                &private_imported_types,
                &module.path,
            )?);
        }
        for function in &module.program.functions {
            flattened_functions.push(rewrite_function(
                function,
                module_symbols,
                &visible_functions,
                &visible_consts,
                &visible_structs,
                &visible_types,
                &private_imported,
                &private_imported_consts,
                &private_imported_types,
                &module.path,
            )?);
        }
        if module.is_entry {
            for stmt in &module.program.stmts {
                flattened_stmts.push(rewrite_stmt(
                    stmt,
                    &visible_functions,
                    &visible_consts,
                    &visible_structs,
                    &visible_types,
                    &private_imported,
                    &private_imported_consts,
                    &private_imported_types,
                    &module.path,
                )?);
            }
        }
    }

    Ok(syntax::Program {
        path: modules
            .iter()
            .find(|module| module.is_entry)
            .map(|module| module.path.display().to_string())
            .unwrap_or_default(),
        imports: Vec::new(),
        consts: Vec::new(),
        type_aliases: flattened_type_aliases,
        structs: flattened_structs,
        enums: flattened_enums,
        functions: flattened_functions,
        stmts: flattened_stmts,
    })
}

fn build_module_symbols(module: &LoadedModule) -> Result<ModuleSymbols, Diagnostic> {
    let module_id = module_id_for_path(&module.path, &module.source_root, &module.package_name);
    let mut functions = HashMap::new();
    let mut public_functions = HashMap::new();
    let mut package_functions = HashMap::new();
    let mut private_functions = HashSet::new();
    let mut consts = HashMap::new();
    let mut public_consts = HashMap::new();
    let mut package_consts = HashMap::new();
    let mut private_consts = HashSet::new();
    let mut aliases = HashMap::new();
    let mut public_aliases = HashMap::new();
    let mut package_aliases = HashMap::new();
    let mut private_aliases = HashSet::new();
    let mut structs = HashMap::new();
    let mut public_structs = HashMap::new();
    let mut package_structs = HashMap::new();
    let mut private_structs = HashSet::new();
    let mut enums = HashMap::new();
    let mut public_enums = HashMap::new();
    let mut package_enums = HashMap::new();
    let mut private_enums = HashSet::new();
    for struct_decl in &module.program.structs {
        let internal_name = format!("{module_id}_{}", struct_decl.name);
        if structs
            .insert(struct_decl.name.clone(), internal_name.clone())
            .is_some()
        {
            return Err(Diagnostic::new(
                "type",
                format!("duplicate struct {:?}", struct_decl.name),
            )
            .with_path(module.path.display().to_string())
            .with_span(struct_decl.line, struct_decl.column));
        }
        match struct_decl.visibility {
            syntax::Visibility::Public => {
                public_structs.insert(struct_decl.name.clone(), internal_name);
            }
            syntax::Visibility::Package => {
                package_structs.insert(struct_decl.name.clone(), internal_name);
            }
            syntax::Visibility::Module => {
                private_structs.insert(struct_decl.name.clone());
            }
        }
    }
    for enum_decl in &module.program.enums {
        if structs.contains_key(&enum_decl.name) {
            return Err(Diagnostic::new(
                "type",
                format!("duplicate type name {:?}", enum_decl.name),
            )
            .with_path(module.path.display().to_string())
            .with_span(enum_decl.line, enum_decl.column));
        }
        let internal_name = format!("{module_id}_{}", enum_decl.name);
        if enums
            .insert(enum_decl.name.clone(), internal_name.clone())
            .is_some()
        {
            return Err(
                Diagnostic::new("type", format!("duplicate enum {:?}", enum_decl.name))
                    .with_path(module.path.display().to_string())
                    .with_span(enum_decl.line, enum_decl.column),
            );
        }
        match enum_decl.visibility {
            syntax::Visibility::Public => {
                public_enums.insert(enum_decl.name.clone(), internal_name);
            }
            syntax::Visibility::Package => {
                package_enums.insert(enum_decl.name.clone(), internal_name);
            }
            syntax::Visibility::Module => {
                private_enums.insert(enum_decl.name.clone());
            }
        }
    }
    for function in &module.program.functions {
        let internal_name = format!("{module_id}_{}", function.name);
        if functions
            .insert(function.name.clone(), internal_name.clone())
            .is_some()
        {
            return Err(
                Diagnostic::new("type", format!("duplicate function {:?}", function.name))
                    .with_path(module.path.display().to_string())
                    .with_span(function.line, function.column),
            );
        }
        match function.visibility {
            syntax::Visibility::Public => {
                public_functions.insert(function.name.clone(), internal_name);
            }
            syntax::Visibility::Package => {
                package_functions.insert(function.name.clone(), internal_name);
            }
            syntax::Visibility::Module => {
                private_functions.insert(function.name.clone());
            }
        }
    }
    for const_decl in &module.program.consts {
        if functions.contains_key(&const_decl.name)
            || structs.contains_key(&const_decl.name)
            || enums.contains_key(&const_decl.name)
        {
            return Err(
                Diagnostic::new("type", format!("duplicate const {:?}", const_decl.name))
                    .with_path(module.path.display().to_string())
                    .with_span(const_decl.line, const_decl.column),
            );
        }
        if consts
            .insert(const_decl.name.clone(), const_decl.clone())
            .is_some()
        {
            return Err(
                Diagnostic::new("type", format!("duplicate const {:?}", const_decl.name))
                    .with_path(module.path.display().to_string())
                    .with_span(const_decl.line, const_decl.column),
            );
        }
        match const_decl.visibility {
            syntax::Visibility::Public => {
                public_consts.insert(const_decl.name.clone(), const_decl.clone());
            }
            syntax::Visibility::Package => {
                package_consts.insert(const_decl.name.clone(), const_decl.clone());
            }
            syntax::Visibility::Module => {
                private_consts.insert(const_decl.name.clone());
            }
        }
    }
    for type_alias in &module.program.type_aliases {
        if structs.contains_key(&type_alias.name) || enums.contains_key(&type_alias.name) {
            return Err(Diagnostic::new(
                "type",
                format!("duplicate type name {:?}", type_alias.name),
            )
            .with_path(module.path.display().to_string())
            .with_span(type_alias.line, type_alias.column));
        }
        let internal_name = format!("{module_id}_{}", type_alias.name);
        if aliases
            .insert(type_alias.name.clone(), internal_name.clone())
            .is_some()
        {
            return Err(Diagnostic::new(
                "type",
                format!("duplicate type alias {:?}", type_alias.name),
            )
            .with_path(module.path.display().to_string())
            .with_span(type_alias.line, type_alias.column));
        }
        match type_alias.visibility {
            syntax::Visibility::Public => {
                public_aliases.insert(type_alias.name.clone(), internal_name);
            }
            syntax::Visibility::Package => {
                package_aliases.insert(type_alias.name.clone(), internal_name);
            }
            syntax::Visibility::Module => {
                private_aliases.insert(type_alias.name.clone());
            }
        }
    }
    Ok(ModuleSymbols {
        module_id,
        functions,
        public_functions,
        package_functions,
        private_functions,
        consts,
        public_consts,
        package_consts,
        private_consts,
        aliases,
        public_aliases,
        package_aliases,
        private_aliases,
        structs,
        public_structs,
        package_structs,
        private_structs,
        enums,
        public_enums,
        package_enums,
        private_enums,
    })
}

fn rewrite_type_alias(
    type_alias: &syntax::TypeAliasDecl,
    module_symbols: &ModuleSymbols,
    visible_types: &HashMap<String, String>,
    private_imported_types: &HashSet<String>,
    module_path: &Path,
) -> Result<syntax::TypeAliasDecl, Diagnostic> {
    Ok(syntax::TypeAliasDecl {
        name: module_symbols
            .aliases
            .get(&type_alias.name)
            .cloned()
            .unwrap_or_else(|| format!("{}_{}", module_symbols.module_id, type_alias.name)),
        ty: rewrite_type_name(
            &type_alias.ty,
            visible_types,
            private_imported_types,
            module_path,
            type_alias.line,
            type_alias.column,
        )?,
        visibility: type_alias.visibility,
        line: type_alias.line,
        column: type_alias.column,
    })
}

fn rewrite_struct(
    struct_decl: &syntax::StructDecl,
    module_symbols: &ModuleSymbols,
    visible_types: &HashMap<String, String>,
    private_imported_types: &HashSet<String>,
    module_path: &Path,
) -> Result<syntax::StructDecl, Diagnostic> {
    let mut rewritten = struct_decl.clone();
    rewritten.name = module_symbols
        .structs
        .get(&struct_decl.name)
        .cloned()
        .unwrap_or_else(|| format!("{}_{}", module_symbols.module_id, struct_decl.name));
    rewritten.fields = struct_decl
        .fields
        .iter()
        .map(|field| {
            Ok(syntax::StructField {
                name: field.name.clone(),
                ty: rewrite_type_name(
                    &field.ty,
                    visible_types,
                    private_imported_types,
                    module_path,
                    field.line,
                    field.column,
                )?,
                line: field.line,
                column: field.column,
            })
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    Ok(rewritten)
}

fn rewrite_enum(
    enum_decl: &syntax::EnumDecl,
    module_symbols: &ModuleSymbols,
    visible_types: &HashMap<String, String>,
    private_imported_types: &HashSet<String>,
    module_path: &Path,
) -> Result<syntax::EnumDecl, Diagnostic> {
    let mut rewritten = enum_decl.clone();
    rewritten.name = module_symbols
        .enums
        .get(&enum_decl.name)
        .cloned()
        .unwrap_or_else(|| format!("{}_{}", module_symbols.module_id, enum_decl.name));
    rewritten.variants = enum_decl
        .variants
        .iter()
        .map(|variant| {
            Ok(syntax::EnumVariantDecl {
                name: variant.name.clone(),
                payload_tys: variant
                    .payload_tys
                    .iter()
                    .map(|ty| {
                        rewrite_type_name(
                            ty,
                            visible_types,
                            private_imported_types,
                            module_path,
                            variant.line,
                            variant.column,
                        )
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?,
                payload_names: variant.payload_names.clone(),
                line: variant.line,
                column: variant.column,
            })
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    Ok(rewritten)
}

fn rewrite_function(
    function: &syntax::Function,
    module_symbols: &ModuleSymbols,
    visible_functions: &HashMap<String, String>,
    visible_consts: &HashMap<String, syntax::ConstDecl>,
    visible_structs: &HashMap<String, String>,
    visible_types: &HashMap<String, String>,
    private_imported: &HashSet<String>,
    private_imported_consts: &HashSet<String>,
    private_imported_types: &HashSet<String>,
    module_path: &Path,
) -> Result<syntax::Function, Diagnostic> {
    let mut rewritten = function.clone();
    rewritten.name = module_symbols
        .functions
        .get(&function.name)
        .cloned()
        .unwrap_or_else(|| format!("{}_{}", module_symbols.module_id, function.name));
    rewritten.body = function
        .body
        .iter()
        .map(|stmt| {
            rewrite_stmt(
                stmt,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    rewritten.params = function
        .params
        .iter()
        .map(|param| {
            Ok(syntax::Param {
                name: param.name.clone(),
                ty: rewrite_type_name(
                    &param.ty,
                    visible_types,
                    private_imported_types,
                    module_path,
                    param.line,
                    param.column,
                )?,
                line: param.line,
                column: param.column,
            })
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    rewritten.return_ty = rewrite_type_name(
        &function.return_ty,
        visible_types,
        private_imported_types,
        module_path,
        function.line,
        function.column,
    )?;
    if let Some(target) = &function.impl_target {
        let rewritten_target = rewrite_type_name(
            &syntax::TypeName::Named(target.clone(), Vec::new()),
            visible_types,
            private_imported_types,
            module_path,
            function.line,
            function.column,
        )?;
        match rewritten_target {
            syntax::TypeName::Named(name, args) if args.is_empty() => {
                rewritten.impl_target = Some(name);
            }
            _ => {
                return Err(Diagnostic::new(
                    "type",
                    format!("impl target {:?} must resolve to a named type", target),
                )
                .with_path(module_path.display().to_string())
                .with_span(function.line, function.column));
            }
        }
    }
    Ok(rewritten)
}

fn rewrite_stmt(
    stmt: &syntax::Stmt,
    visible_functions: &HashMap<String, String>,
    visible_consts: &HashMap<String, syntax::ConstDecl>,
    visible_structs: &HashMap<String, String>,
    visible_types: &HashMap<String, String>,
    private_imported: &HashSet<String>,
    private_imported_consts: &HashSet<String>,
    private_imported_types: &HashSet<String>,
    module_path: &Path,
) -> Result<syntax::Stmt, Diagnostic> {
    Ok(match stmt {
        syntax::Stmt::Let {
            name,
            ty,
            expr,
            line,
            column,
        } => syntax::Stmt::Let {
            name: name.clone(),
            ty: rewrite_type_name(
                ty,
                visible_types,
                private_imported_types,
                module_path,
                *line,
                *column,
            )?,
            expr: rewrite_expr(
                expr,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Print { expr, line, column } => syntax::Stmt::Print {
            expr: rewrite_expr(
                expr,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Panic { expr, line, column } => syntax::Stmt::Panic {
            expr: rewrite_expr(
                expr,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Defer { expr, line, column } => syntax::Stmt::Defer {
            expr: rewrite_expr(
                expr,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::If {
            cond,
            then_block,
            else_block,
            line,
            column,
        } => syntax::Stmt::If {
            cond: rewrite_expr(
                cond,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?,
            then_block: then_block
                .iter()
                .map(|stmt| {
                    rewrite_stmt(
                        stmt,
                        visible_functions,
                        visible_consts,
                        visible_structs,
                        visible_types,
                        private_imported,
                        private_imported_consts,
                        private_imported_types,
                        module_path,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            else_block: else_block
                .as_ref()
                .map(|block| {
                    block
                        .iter()
                        .map(|stmt| {
                            rewrite_stmt(
                                stmt,
                                visible_functions,
                                visible_consts,
                                visible_structs,
                                visible_types,
                                private_imported,
                                private_imported_consts,
                                private_imported_types,
                                module_path,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::While {
            cond,
            body,
            line,
            column,
        } => syntax::Stmt::While {
            cond: rewrite_expr(
                cond,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?,
            body: body
                .iter()
                .map(|stmt| {
                    rewrite_stmt(
                        stmt,
                        visible_functions,
                        visible_consts,
                        visible_structs,
                        visible_types,
                        private_imported,
                        private_imported_consts,
                        private_imported_types,
                        module_path,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Match {
            expr,
            arms,
            line,
            column,
        } => syntax::Stmt::Match {
            expr: rewrite_expr(
                expr,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?,
            arms: arms
                .iter()
                .map(|arm| {
                    Ok(syntax::MatchArm {
                        variant: arm.variant.clone(),
                        bindings: arm.bindings.clone(),
                        is_named: arm.is_named,
                        body: arm
                            .body
                            .iter()
                            .map(|stmt| {
                                rewrite_stmt(
                                    stmt,
                                    visible_functions,
                                    visible_consts,
                                    visible_structs,
                                    visible_types,
                                    private_imported,
                                    private_imported_consts,
                                    private_imported_types,
                                    module_path,
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                        line: arm.line,
                        column: arm.column,
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?,
            line: *line,
            column: *column,
        },
        syntax::Stmt::Return { expr, line, column } => syntax::Stmt::Return {
            expr: rewrite_expr(
                expr,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?,
            line: *line,
            column: *column,
        },
    })
}

fn rewrite_expr(
    expr: &syntax::Expr,
    visible_functions: &HashMap<String, String>,
    visible_consts: &HashMap<String, syntax::ConstDecl>,
    visible_structs: &HashMap<String, String>,
    visible_types: &HashMap<String, String>,
    private_imported: &HashSet<String>,
    private_imported_consts: &HashSet<String>,
    private_imported_types: &HashSet<String>,
    module_path: &Path,
) -> Result<syntax::Expr, Diagnostic> {
    Ok(match expr {
        syntax::Expr::Literal(_) => expr.clone(),
        syntax::Expr::VarRef { name, line, column } => {
            if let Some(const_decl) = visible_consts.get(name) {
                resolve_const_decl(
                    const_decl,
                    visible_consts,
                    visible_functions,
                    visible_structs,
                    visible_types,
                    private_imported,
                    private_imported_consts,
                    private_imported_types,
                    module_path,
                    &mut HashSet::new(),
                )?
            } else if private_imported_consts.contains(name) {
                return Err(Diagnostic::new(
                    "import",
                    format!("const {name:?} is not visible from this module"),
                )
                .with_path(module_path.display().to_string())
                .with_span(*line, *column));
            } else {
                expr.clone()
            }
        }
        syntax::Expr::Call {
            name,
            type_args,
            args,
            line,
            column,
        } => {
            if !visible_functions.contains_key(name) && private_imported.contains(name) {
                return Err(Diagnostic::new(
                    "import",
                    format!("function {name:?} is not visible from this module"),
                )
                .with_path(module_path.display().to_string())
                .with_span(*line, *column));
            }
            syntax::Expr::Call {
                name: visible_functions
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| name.clone()),
                type_args: type_args
                    .iter()
                    .map(|type_arg| {
                        rewrite_type_name(
                            type_arg,
                            visible_types,
                            private_imported_types,
                            module_path,
                            *line,
                            *column,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                args: args
                    .iter()
                    .map(|arg| {
                        rewrite_expr(
                            arg,
                            visible_functions,
                            visible_consts,
                            visible_structs,
                            visible_types,
                            private_imported,
                            private_imported_consts,
                            private_imported_types,
                            module_path,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                line: *line,
                column: *column,
            }
        }
        syntax::Expr::MethodCall {
            base,
            method,
            type_args,
            args,
            line,
            column,
        } => {
            let rewritten_base = rewrite_expr(
                base,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?;
            let rewritten_base = match rewritten_base {
                syntax::Expr::VarRef { name, line, column } => {
                    if let Some(mapped) = visible_types.get(&name) {
                        syntax::Expr::VarRef {
                            name: mapped.clone(),
                            line,
                            column,
                        }
                    } else {
                        syntax::Expr::VarRef { name, line, column }
                    }
                }
                other => other,
            };
            syntax::Expr::MethodCall {
                base: Box::new(rewritten_base),
                method: method.clone(),
                type_args: type_args
                    .iter()
                    .map(|type_arg| {
                        rewrite_type_name(
                            type_arg,
                            visible_types,
                            private_imported_types,
                            module_path,
                            *line,
                            *column,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                args: args
                    .iter()
                    .map(|arg| {
                        rewrite_expr(
                            arg,
                            visible_functions,
                            visible_consts,
                            visible_structs,
                            visible_types,
                            private_imported,
                            private_imported_consts,
                            private_imported_types,
                            module_path,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                line: *line,
                column: *column,
            }
        }
        syntax::Expr::BinaryAdd {
            lhs,
            rhs,
            line,
            column,
        } => syntax::Expr::BinaryAdd {
            lhs: Box::new(rewrite_expr(
                lhs,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?),
            rhs: Box::new(rewrite_expr(
                rhs,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::BinaryCompare {
            op,
            lhs,
            rhs,
            line,
            column,
        } => syntax::Expr::BinaryCompare {
            op: *op,
            lhs: Box::new(rewrite_expr(
                lhs,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?),
            rhs: Box::new(rewrite_expr(
                rhs,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Try { expr, line, column } => syntax::Expr::Try {
            expr: Box::new(rewrite_expr(
                expr,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::Await { expr, line, column } => syntax::Expr::Await {
            expr: Box::new(rewrite_expr(
                expr,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?),
            line: *line,
            column: *column,
        },
        syntax::Expr::StructLiteral {
            name,
            fields,
            line,
            column,
        } => {
            if !visible_structs.contains_key(name) && private_imported_types.contains(name) {
                return Err(Diagnostic::new(
                    "import",
                    format!("struct {name:?} is not visible from this module"),
                )
                .with_path(module_path.display().to_string())
                .with_span(*line, *column));
            }
            syntax::Expr::StructLiteral {
                name: visible_structs
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| name.clone()),
                fields: fields
                    .iter()
                    .map(|field| {
                        Ok(syntax::StructFieldValue {
                            name: field.name.clone(),
                            expr: rewrite_expr(
                                &field.expr,
                                visible_functions,
                                visible_consts,
                                visible_structs,
                                visible_types,
                                private_imported,
                                private_imported_consts,
                                private_imported_types,
                                module_path,
                            )?,
                            line: field.line,
                            column: field.column,
                        })
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?,
                line: *line,
                column: *column,
            }
        }
        syntax::Expr::FieldAccess {
            base,
            field,
            line,
            column,
        } => syntax::Expr::FieldAccess {
            base: Box::new(rewrite_expr(
                base,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?),
            field: field.clone(),
            line: *line,
            column: *column,
        },
        syntax::Expr::TupleLiteral {
            elements,
            line,
            column,
        } => syntax::Expr::TupleLiteral {
            elements: elements
                .iter()
                .map(|element| {
                    rewrite_expr(
                        element,
                        visible_functions,
                        visible_consts,
                        visible_structs,
                        visible_types,
                        private_imported,
                        private_imported_consts,
                        private_imported_types,
                        module_path,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::TupleIndex {
            base,
            index,
            line,
            column,
        } => syntax::Expr::TupleIndex {
            base: Box::new(rewrite_expr(
                base,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?),
            index: *index,
            line: *line,
            column: *column,
        },
        syntax::Expr::MapLiteral {
            entries,
            line,
            column,
        } => syntax::Expr::MapLiteral {
            entries: entries
                .iter()
                .map(|entry| {
                    Ok(syntax::MapEntry {
                        key: rewrite_expr(
                            &entry.key,
                            visible_functions,
                            visible_consts,
                            visible_structs,
                            visible_types,
                            private_imported,
                            private_imported_consts,
                            private_imported_types,
                            module_path,
                        )?,
                        value: rewrite_expr(
                            &entry.value,
                            visible_functions,
                            visible_consts,
                            visible_structs,
                            visible_types,
                            private_imported,
                            private_imported_consts,
                            private_imported_types,
                            module_path,
                        )?,
                        line: entry.line,
                        column: entry.column,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::ArrayLiteral {
            elements,
            line,
            column,
        } => syntax::Expr::ArrayLiteral {
            elements: elements
                .iter()
                .map(|element| {
                    rewrite_expr(
                        element,
                        visible_functions,
                        visible_consts,
                        visible_structs,
                        visible_types,
                        private_imported,
                        private_imported_consts,
                        private_imported_types,
                        module_path,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::Slice {
            base,
            start,
            end,
            line,
            column,
        } => syntax::Expr::Slice {
            base: Box::new(rewrite_expr(
                base,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?),
            start: start
                .as_ref()
                .map(|expr| {
                    rewrite_expr(
                        expr,
                        visible_functions,
                        visible_consts,
                        visible_structs,
                        visible_types,
                        private_imported,
                        private_imported_consts,
                        private_imported_types,
                        module_path,
                    )
                    .map(Box::new)
                })
                .transpose()?,
            end: end
                .as_ref()
                .map(|expr| {
                    rewrite_expr(
                        expr,
                        visible_functions,
                        visible_consts,
                        visible_structs,
                        visible_types,
                        private_imported,
                        private_imported_consts,
                        private_imported_types,
                        module_path,
                    )
                    .map(Box::new)
                })
                .transpose()?,
            line: *line,
            column: *column,
        },
        syntax::Expr::Index {
            base,
            index,
            line,
            column,
        } => syntax::Expr::Index {
            base: Box::new(rewrite_expr(
                base,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?),
            index: Box::new(rewrite_expr(
                index,
                visible_functions,
                visible_consts,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
            )?),
            line: *line,
            column: *column,
        },
    })
}

fn rewrite_type_name(
    ty: &syntax::TypeName,
    visible_types: &HashMap<String, String>,
    private_imported_types: &HashSet<String>,
    module_path: &Path,
    line: usize,
    column: usize,
) -> Result<syntax::TypeName, Diagnostic> {
    match ty {
        syntax::TypeName::Int => Ok(syntax::TypeName::Int),
        syntax::TypeName::Bool => Ok(syntax::TypeName::Bool),
        syntax::TypeName::String => Ok(syntax::TypeName::String),
        syntax::TypeName::Named(name, args) => {
            if !visible_types.contains_key(name) && private_imported_types.contains(name) {
                return Err(Diagnostic::new(
                    "import",
                    format!("type {name:?} is not visible from this module"),
                )
                .with_path(module_path.display().to_string())
                .with_span(line, column));
            }
            Ok(syntax::TypeName::Named(
                visible_types
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| name.clone()),
                args.iter()
                    .map(|arg| {
                        rewrite_type_name(
                            arg,
                            visible_types,
                            private_imported_types,
                            module_path,
                            line,
                            column,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ))
        }
        syntax::TypeName::Ptr(inner) => Ok(syntax::TypeName::Ptr(Box::new(rewrite_type_name(
            inner,
            visible_types,
            private_imported_types,
            module_path,
            line,
            column,
        )?))),
        syntax::TypeName::MutPtr(inner) => {
            Ok(syntax::TypeName::MutPtr(Box::new(rewrite_type_name(
                inner,
                visible_types,
                private_imported_types,
                module_path,
                line,
                column,
            )?)))
        }
        syntax::TypeName::Option(inner) => {
            Ok(syntax::TypeName::Option(Box::new(rewrite_type_name(
                inner,
                visible_types,
                private_imported_types,
                module_path,
                line,
                column,
            )?)))
        }
        syntax::TypeName::Slice(inner) => Ok(syntax::TypeName::Slice(Box::new(rewrite_type_name(
            inner,
            visible_types,
            private_imported_types,
            module_path,
            line,
            column,
        )?))),
        syntax::TypeName::MutSlice(inner) => {
            Ok(syntax::TypeName::MutSlice(Box::new(rewrite_type_name(
                inner,
                visible_types,
                private_imported_types,
                module_path,
                line,
                column,
            )?)))
        }
        syntax::TypeName::Result(ok, err) => Ok(syntax::TypeName::Result(
            Box::new(rewrite_type_name(
                ok,
                visible_types,
                private_imported_types,
                module_path,
                line,
                column,
            )?),
            Box::new(rewrite_type_name(
                err,
                visible_types,
                private_imported_types,
                module_path,
                line,
                column,
            )?),
        )),
        syntax::TypeName::Tuple(elements) => Ok(syntax::TypeName::Tuple(
            elements
                .iter()
                .map(|element| {
                    rewrite_type_name(
                        element,
                        visible_types,
                        private_imported_types,
                        module_path,
                        line,
                        column,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
        )),
        syntax::TypeName::Map(key, value) => Ok(syntax::TypeName::Map(
            Box::new(rewrite_type_name(
                key,
                visible_types,
                private_imported_types,
                module_path,
                line,
                column,
            )?),
            Box::new(rewrite_type_name(
                value,
                visible_types,
                private_imported_types,
                module_path,
                line,
                column,
            )?),
        )),
        syntax::TypeName::Array(inner) => Ok(syntax::TypeName::Array(Box::new(rewrite_type_name(
            inner,
            visible_types,
            private_imported_types,
            module_path,
            line,
            column,
        )?))),
    }
}

fn resolve_const_decl(
    const_decl: &syntax::ConstDecl,
    visible_consts: &HashMap<String, syntax::ConstDecl>,
    visible_functions: &HashMap<String, String>,
    visible_structs: &HashMap<String, String>,
    visible_types: &HashMap<String, String>,
    private_imported: &HashSet<String>,
    private_imported_consts: &HashSet<String>,
    private_imported_types: &HashSet<String>,
    module_path: &Path,
    resolving: &mut HashSet<String>,
) -> Result<syntax::Expr, Diagnostic> {
    if !resolving.insert(const_decl.name.clone()) {
        return Err(
            Diagnostic::new("type", format!("recursive const {:?}", const_decl.name))
                .with_path(module_path.display().to_string())
                .with_span(const_decl.line, const_decl.column),
        );
    }
    let rewritten = resolve_const_expr(
        &const_decl.expr,
        visible_consts,
        visible_functions,
        visible_structs,
        visible_types,
        private_imported,
        private_imported_consts,
        private_imported_types,
        module_path,
        resolving,
    )?;
    resolving.remove(&const_decl.name);
    let actual = const_expr_type(&rewritten).ok_or_else(|| {
        Diagnostic::new(
            "type",
            format!(
                "const {:?} requires a compile-time scalar expression",
                const_decl.name
            ),
        )
        .with_path(module_path.display().to_string())
        .with_span(const_decl.line, const_decl.column)
    })?;
    let expected = const_type_name(&const_decl.ty).ok_or_else(|| {
        Diagnostic::new(
            "type",
            format!("const {:?} must use int, bool, or string", const_decl.name),
        )
        .with_path(module_path.display().to_string())
        .with_span(const_decl.line, const_decl.column)
    })?;
    if actual != expected {
        return Err(Diagnostic::new(
            "type",
            format!(
                "const {:?} expects {}, got {}",
                const_decl.name,
                const_type_label(&expected),
                const_type_label(&actual)
            ),
        )
        .with_path(module_path.display().to_string())
        .with_span(const_decl.line, const_decl.column));
    }
    Ok(rewritten)
}

fn resolve_const_expr(
    expr: &syntax::Expr,
    visible_consts: &HashMap<String, syntax::ConstDecl>,
    visible_functions: &HashMap<String, String>,
    visible_structs: &HashMap<String, String>,
    visible_types: &HashMap<String, String>,
    private_imported: &HashSet<String>,
    private_imported_consts: &HashSet<String>,
    private_imported_types: &HashSet<String>,
    module_path: &Path,
    resolving: &mut HashSet<String>,
) -> Result<syntax::Expr, Diagnostic> {
    match expr {
        syntax::Expr::VarRef { name, line, column } => {
            if let Some(const_decl) = visible_consts.get(name) {
                return resolve_const_decl(
                    const_decl,
                    visible_consts,
                    visible_functions,
                    visible_structs,
                    visible_types,
                    private_imported,
                    private_imported_consts,
                    private_imported_types,
                    module_path,
                    resolving,
                );
            }
            if private_imported_consts.contains(name) {
                return Err(Diagnostic::new(
                    "import",
                    format!("const {name:?} is not visible from this module"),
                )
                .with_path(module_path.display().to_string())
                .with_span(*line, *column));
            }
            Ok(expr.clone())
        }
        syntax::Expr::Literal(_) => Ok(expr.clone()),
        syntax::Expr::BinaryAdd {
            lhs,
            rhs,
            line,
            column,
        } => Ok(syntax::Expr::BinaryAdd {
            lhs: Box::new(resolve_const_expr(
                lhs,
                visible_consts,
                visible_functions,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
                resolving,
            )?),
            rhs: Box::new(resolve_const_expr(
                rhs,
                visible_consts,
                visible_functions,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
                resolving,
            )?),
            line: *line,
            column: *column,
        }),
        syntax::Expr::BinaryCompare {
            op,
            lhs,
            rhs,
            line,
            column,
        } => Ok(syntax::Expr::BinaryCompare {
            op: *op,
            lhs: Box::new(resolve_const_expr(
                lhs,
                visible_consts,
                visible_functions,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
                resolving,
            )?),
            rhs: Box::new(resolve_const_expr(
                rhs,
                visible_consts,
                visible_functions,
                visible_structs,
                visible_types,
                private_imported,
                private_imported_consts,
                private_imported_types,
                module_path,
                resolving,
            )?),
            line: *line,
            column: *column,
        }),
        _ => rewrite_expr(
            expr,
            visible_functions,
            visible_consts,
            visible_structs,
            visible_types,
            private_imported,
            private_imported_consts,
            private_imported_types,
            module_path,
        ),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConstValueType {
    Int,
    Bool,
    String,
}

fn const_expr_type(expr: &syntax::Expr) -> Option<ConstValueType> {
    match expr {
        syntax::Expr::Literal(syntax::Literal::Int(_)) => Some(ConstValueType::Int),
        syntax::Expr::Literal(syntax::Literal::Bool(_)) => Some(ConstValueType::Bool),
        syntax::Expr::Literal(syntax::Literal::String(_)) => Some(ConstValueType::String),
        syntax::Expr::BinaryAdd { lhs, rhs, .. } => {
            let lhs = const_expr_type(lhs)?;
            let rhs = const_expr_type(rhs)?;
            if lhs == rhs && matches!(lhs, ConstValueType::Int | ConstValueType::String) {
                Some(lhs)
            } else {
                None
            }
        }
        syntax::Expr::BinaryCompare { lhs, rhs, .. } => {
            if const_expr_type(lhs)? == const_expr_type(rhs)? {
                Some(ConstValueType::Bool)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn const_type_name(ty: &syntax::TypeName) -> Option<ConstValueType> {
    match ty {
        syntax::TypeName::Int => Some(ConstValueType::Int),
        syntax::TypeName::Bool => Some(ConstValueType::Bool),
        syntax::TypeName::String => Some(ConstValueType::String),
        _ => None,
    }
}

fn const_type_label(ty: &ConstValueType) -> &'static str {
    match ty {
        ConstValueType::Int => "int",
        ConstValueType::Bool => "bool",
        ConstValueType::String => "string",
    }
}

fn merge_visible_types(
    visible_aliases: &HashMap<String, String>,
    visible_structs: &HashMap<String, String>,
    visible_enums: &HashMap<String, String>,
    module_path: &Path,
) -> Result<HashMap<String, String>, Diagnostic> {
    let mut visible_types = visible_aliases.clone();
    for (name, internal_name) in visible_structs {
        if let Some(existing) = visible_types.get(name)
            && existing != internal_name
        {
            return Err(Diagnostic::new(
                "import",
                format!("imported type {name:?} collides with an existing name"),
            )
            .with_path(module_path.display().to_string()));
        }
        visible_types.insert(name.clone(), internal_name.clone());
    }
    for (name, internal_name) in visible_enums {
        if let Some(existing) = visible_types.get(name)
            && existing != internal_name
        {
            return Err(Diagnostic::new(
                "import",
                format!("imported type {name:?} collides with an existing name"),
            )
            .with_path(module_path.display().to_string()));
        }
        visible_types.insert(name.clone(), internal_name.clone());
    }
    Ok(visible_types)
}

fn resolve_import_path(
    graph: &PackageGraph,
    package_root: &Path,
    module_path: &Path,
    import: &syntax::Import,
) -> Result<(PathBuf, PathBuf), Diagnostic> {
    let package = graph.context(package_root)?;
    let relative = Path::new(&import.path);
    if relative.is_absolute() {
        return Err(Diagnostic::new("import", "stage1 imports must be relative")
            .with_path(module_path.display().to_string())
            .with_span(import.line, import.column));
    }
    if relative
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(Diagnostic::new(
            "import",
            "stage1 imports may not traverse parent directories",
        )
        .with_path(module_path.display().to_string())
        .with_span(import.line, import.column));
    }
    let mut components = relative.components();
    if let Some(Component::Normal(first)) = components.next() {
        let first_name = first.to_string_lossy().to_string();
        if first_name == stdlib::STDLIB_IMPORT_PREFIX {
            let mut remainder = PathBuf::new();
            for component in components {
                remainder.push(component.as_os_str());
            }
            if remainder.as_os_str().is_empty() {
                return Err(Diagnostic::new(
                    "import",
                    "stdlib import must include a module path (e.g. import \"std/time.ax\")",
                )
                .with_path(module_path.display().to_string())
                .with_span(import.line, import.column));
            }
            if !stdlib::stdlib_has_module(&remainder) {
                let module_name = remainder.to_string_lossy();
                return Err(Diagnostic::new(
                    "import",
                    crate::diagnostics::message_with_suggestion(
                        format!("unknown stdlib module {:?}", import.path),
                        &module_name,
                        stdlib::stdlib_module_names(),
                    ),
                )
                .with_path(module_path.display().to_string())
                .with_span(import.line, import.column));
            }
            let virtual_path = stdlib::stdlib_source_path(&remainder.to_string_lossy());
            return Ok((stdlib::stdlib_root(), virtual_path));
        }
        let dependency_name = first_name;
        if let Some(dependency_root) = package.dependencies.get(&dependency_name) {
            let dependency = graph.context(dependency_root)?;
            let mut remainder = PathBuf::new();
            for component in components {
                remainder.push(component.as_os_str());
            }
            if remainder.as_os_str().is_empty() {
                return Err(Diagnostic::new(
                    "import",
                    format!("dependency import {dependency_name:?} must include a module path"),
                )
                .with_path(module_path.display().to_string())
                .with_span(import.line, import.column));
            }
            let candidate = normalize_path(dependency.source_root.join(remainder));
            if !candidate.starts_with(&dependency.source_root) {
                return Err(Diagnostic::new(
                    "import",
                    "dependency imports must stay inside the package",
                )
                .with_path(module_path.display().to_string())
                .with_span(import.line, import.column));
            }
            if !candidate.exists() {
                return Err(Diagnostic::new(
                    "import",
                    format!("missing import {}", candidate.display()),
                )
                .with_path(module_path.display().to_string())
                .with_span(import.line, import.column));
            }
            let candidate = canonicalize_existing_path(&candidate, "import path")?;
            if !candidate.starts_with(&dependency.source_root) {
                return Err(Diagnostic::new(
                    "import",
                    "dependency imports must stay inside the package",
                )
                .with_path(module_path.display().to_string())
                .with_span(import.line, import.column));
            }
            return Ok((dependency.root.clone(), candidate));
        }
    }
    let base_dir = module_path.parent().unwrap_or(&package.source_root);
    let candidate = normalize_path(base_dir.join(relative));
    if !candidate.starts_with(&package.root) {
        return Err(
            Diagnostic::new("import", "stage1 imports must stay inside the package")
                .with_path(module_path.display().to_string())
                .with_span(import.line, import.column),
        );
    }
    if !candidate.exists() {
        return Err(
            Diagnostic::new("import", format!("missing import {}", candidate.display()))
                .with_path(module_path.display().to_string())
                .with_span(import.line, import.column),
        );
    }
    let candidate = canonicalize_existing_path(&candidate, "import path")?;
    if !candidate.starts_with(&package.root) {
        return Err(
            Diagnostic::new("import", "stage1 imports must stay inside the package")
                .with_path(module_path.display().to_string())
                .with_span(import.line, import.column),
        );
    }
    Ok((package.root.clone(), candidate))
}

fn canonicalize_existing_path(path: &Path, label: &str) -> Result<PathBuf, Diagnostic> {
    fs::canonicalize(path).map_err(|err| {
        Diagnostic::new(
            "path",
            format!("failed to resolve {label} {}: {err}", path.display()),
        )
        .with_path(path.display().to_string())
    })
}

fn canonicalize_package_path(
    path: &Path,
    package_root: &Path,
    kind: &'static str,
    outside_message: &'static str,
) -> Result<PathBuf, Diagnostic> {
    let package_root = canonicalize_existing_path(package_root, "package root")?;
    let canonical = canonicalize_existing_path(path, "package path")?;
    if !canonical.starts_with(&package_root) {
        return Err(Diagnostic::new(kind, outside_message).with_path(path.display().to_string()));
    }
    Ok(canonical)
}

fn fs_root_path_for_package(
    package_root: &Path,
    manifest: &Manifest,
) -> Result<PathBuf, Diagnostic> {
    let configured = if manifest.capabilities.fs || manifest.capabilities.fs_write {
        manifest.capabilities.fs_root.as_deref().unwrap_or(".")
    } else {
        "."
    };
    let root = normalize_path(package_root.join(configured));
    let canonical_package_root = canonicalize_existing_path(package_root, "package root")?;
    let canonical_root = canonicalize_existing_path(&root, "filesystem capability root")?;
    if !canonical_root.starts_with(&canonical_package_root) {
        return Err(Diagnostic::new(
            "capability",
            "capabilities.fs_root resolves outside the package",
        )
        .with_path(root.display().to_string()));
    }
    Ok(canonical_root)
}

fn ensure_output_path_stays_inside_package(
    package_root: &Path,
    path: &Path,
    label: &str,
) -> Result<(), Diagnostic> {
    let canonical_package_root = canonicalize_existing_path(package_root, "package root")?;
    let mut ancestor = path.parent().unwrap_or(package_root).to_path_buf();
    while !ancestor.exists() {
        if !ancestor.pop() {
            break;
        }
    }
    let canonical_ancestor = canonicalize_existing_path(&ancestor, label)?;
    if !canonical_ancestor.starts_with(&canonical_package_root) {
        return Err(
            Diagnostic::new("build", format!("{label} resolves outside the package"))
                .with_path(path.display().to_string()),
        );
    }
    Ok(())
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

fn module_id_for_path(path: &Path, source_root: &Path, package_name: &str) -> String {
    let relative = path.strip_prefix(source_root).unwrap_or(path);
    let stem = relative.with_extension("");
    let mut out = slug_identifier(package_name);
    for component in stem.components() {
        let component = component.as_os_str().to_string_lossy();
        if !out.is_empty() {
            out.push('_');
        }
        for ch in component.chars() {
            if ch.is_ascii_alphanumeric() {
                out.push(ch.to_ascii_lowercase());
            } else {
                out.push('_');
            }
        }
    }
    if out.is_empty() {
        String::from("module")
    } else {
        out
    }
}

fn slug_identifier(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        String::from("package")
    } else {
        out
    }
}

fn stmt_line(stmt: &syntax::Stmt) -> usize {
    match stmt {
        syntax::Stmt::Let { line, .. }
        | syntax::Stmt::Print { line, .. }
        | syntax::Stmt::Panic { line, .. }
        | syntax::Stmt::Defer { line, .. }
        | syntax::Stmt::If { line, .. }
        | syntax::Stmt::While { line, .. }
        | syntax::Stmt::Match { line, .. }
        | syntax::Stmt::Return { line, .. } => *line,
    }
}

fn stmt_column(stmt: &syntax::Stmt) -> usize {
    match stmt {
        syntax::Stmt::Let { column, .. }
        | syntax::Stmt::Print { column, .. }
        | syntax::Stmt::Panic { column, .. }
        | syntax::Stmt::Defer { column, .. }
        | syntax::Stmt::If { column, .. }
        | syntax::Stmt::While { column, .. }
        | syntax::Stmt::Match { column, .. }
        | syntax::Stmt::Return { column, .. } => *column,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashMap};
    use tempfile::tempdir;

    fn workspace_only_manifest() -> Manifest {
        Manifest {
            package: None,
            dependencies: BTreeMap::new(),
            workspace: None,
            build: BuildSection {
                entry: "src/main.ax".to_string(),
                out_dir: "dist".to_string(),
            },
            tests: Vec::new(),
            capabilities: CapabilityConfig::default(),
        }
    }

    fn package_manifest() -> Manifest {
        Manifest {
            package: Some(PackageSection {
                name: "demo".to_string(),
                version: "0.1.0".to_string(),
            }),
            dependencies: BTreeMap::new(),
            workspace: None,
            build: BuildSection {
                entry: "src/main.ax".to_string(),
                out_dir: "dist".to_string(),
            },
            tests: Vec::new(),
            capabilities: CapabilityConfig::default(),
        }
    }

    #[test]
    fn test_artifact_name_reports_missing_package_manifest() {
        let dir = tempdir().unwrap_or_else(|err| panic!("tempdir: {err}"));
        let error = match test_artifact_name(dir.path(), &workspace_only_manifest(), "main_test") {
            Ok(name) => panic!("workspace-only manifest returned test artifact {name}"),
            Err(error) => error,
        };

        assert_eq!(error.kind, "manifest");
        assert_eq!(error.message, "test artifacts require a package manifest");
        assert_eq!(
            error.path,
            Some(manifest_path(dir.path()).display().to_string())
        );
    }

    #[test]
    fn load_module_reports_missing_package_manifest() {
        let dir = tempdir().unwrap_or_else(|err| panic!("tempdir: {err}"));
        let root = normalize_path(dir.path());
        let source_root = root.join("src");
        fs::create_dir_all(&source_root).unwrap_or_else(|err| panic!("create src: {err}"));
        let entry = source_root.join("main.ax");
        fs::write(&entry, "").unwrap_or_else(|err| panic!("write entry: {err}"));

        let mut graph = PackageGraph::default();
        graph.packages.insert(
            root.clone(),
            PackageContext {
                root: root.clone(),
                manifest: workspace_only_manifest(),
                source_root,
                dependencies: BTreeMap::new(),
                workspace_members: Vec::new(),
            },
        );

        let error = match load_module_recursive(
            &graph,
            &root,
            &entry,
            true,
            &mut Vec::new(),
            &mut HashMap::new(),
            &mut Vec::new(),
        ) {
            Ok(()) => panic!("workspace-only manifest loaded a module"),
            Err(error) => error,
        };

        assert_eq!(error.kind, "manifest");
        assert_eq!(error.message, "loaded modules require a package manifest");
        assert_eq!(error.path, Some(manifest_path(&root).display().to_string()));
    }

    #[test]
    fn analyze_package_accepts_relative_root_with_canonical_graph_key() {
        let cwd = std::env::current_dir().unwrap_or_else(|err| panic!("current dir: {err}"));
        let dir = tempfile::Builder::new()
            .prefix("axiomc-relative-root-")
            .tempdir_in(&cwd)
            .unwrap_or_else(|err| panic!("tempdir in cwd: {err}"));
        let relative_root = dir
            .path()
            .strip_prefix(&cwd)
            .unwrap_or_else(|err| panic!("relative root: {err}"))
            .to_path_buf();
        let root =
            fs::canonicalize(dir.path()).unwrap_or_else(|err| panic!("canonical root: {err}"));
        let source_root = root.join("src");
        fs::create_dir_all(&source_root).unwrap_or_else(|err| panic!("create src: {err}"));
        fs::write(source_root.join("main.ax"), "")
            .unwrap_or_else(|err| panic!("write entry: {err}"));

        let manifest = package_manifest();
        fs::write(
            manifest_path(&root),
            crate::manifest::render_manifest("demo"),
        )
        .unwrap_or_else(|err| panic!("write manifest: {err}"));
        fs::write(
            crate::manifest::lockfile_path(&root),
            crate::lockfile::render_lockfile_for_project(&root, &manifest)
                .unwrap_or_else(|err| panic!("render lockfile: {err}")),
        )
        .unwrap_or_else(|err| panic!("write lockfile: {err}"));

        let mut graph = PackageGraph::default();
        graph.packages.insert(
            root.clone(),
            PackageContext {
                root,
                manifest,
                source_root,
                dependencies: BTreeMap::new(),
                workspace_members: Vec::new(),
            },
        );

        analyze_package(&graph, &relative_root)
            .unwrap_or_else(|err| panic!("relative package root should analyze: {err:?}"));
    }

    #[test]
    fn manifest_benchmark_tests_require_include_benchmarks() {
        let dir = tempdir().unwrap_or_else(|err| panic!("tempdir: {err}"));
        let root = dir.path();
        let mut manifest = package_manifest();
        manifest.tests = vec![
            crate::manifest::TestTarget {
                name: "unit".to_string(),
                entry: "src/unit_test.ax".to_string(),
                stdout: None,
                kind: TestKind::Unit,
            },
            crate::manifest::TestTarget {
                name: "bench".to_string(),
                entry: "src/demo_bench.ax".to_string(),
                stdout: None,
                kind: TestKind::Benchmark,
            },
        ];

        let default_tests = collect_test_targets(root, &manifest, None, false)
            .unwrap_or_else(|err| panic!("collect default tests: {err:?}"));
        assert_eq!(
            default_tests
                .iter()
                .map(|test| test.name.as_str())
                .collect::<Vec<_>>(),
            vec!["unit"]
        );

        let benchmark_tests = collect_test_targets(root, &manifest, None, true)
            .unwrap_or_else(|err| panic!("collect benchmark tests: {err:?}"));
        assert_eq!(
            benchmark_tests
                .iter()
                .map(|test| test.name.as_str())
                .collect::<Vec<_>>(),
            vec!["unit", "bench"]
        );
    }

    #[test]
    fn benchmark_tests_do_not_inherit_package_expected_output() {
        let dir = tempdir().unwrap_or_else(|err| panic!("tempdir: {err}"));
        let root = dir.path();
        let source_root = root.join("src");
        fs::create_dir_all(&source_root).unwrap_or_else(|err| panic!("create src: {err}"));
        fs::write(root.join("expected-output.txt"), "package\n")
            .unwrap_or_else(|err| panic!("write package expected output: {err}"));
        fs::write(source_root.join("unit_test.ax"), "")
            .unwrap_or_else(|err| panic!("write unit test: {err}"));
        fs::write(source_root.join("slow_bench.ax"), "")
            .unwrap_or_else(|err| panic!("write benchmark test: {err}"));
        fs::write(source_root.join("explicit_bench.ax"), "")
            .unwrap_or_else(|err| panic!("write explicit benchmark test: {err}"));
        fs::write(source_root.join("explicit_bench.stdout"), "explicit\n")
            .unwrap_or_else(|err| panic!("write explicit benchmark stdout: {err}"));

        let mut manifest = package_manifest();
        manifest.tests.push(crate::manifest::TestTarget {
            name: "manifest_bench".to_string(),
            entry: "src/manifest_bench.ax".to_string(),
            stdout: None,
            kind: TestKind::Benchmark,
        });

        let tests = collect_test_targets(root, &manifest, None, true)
            .unwrap_or_else(|err| panic!("collect benchmark tests: {err:?}"));
        let stdout_by_name = tests
            .iter()
            .map(|test| (test.name.as_str(), test.stdout.as_deref()))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(
            stdout_by_name.get("src/unit_test"),
            Some(&Some("package\n"))
        );
        assert_eq!(stdout_by_name.get("src/slow_bench"), Some(&None));
        assert_eq!(
            stdout_by_name.get("src/explicit_bench"),
            Some(&Some("explicit\n"))
        );
        assert_eq!(stdout_by_name.get("manifest_bench"), Some(&None));
    }

    #[cfg(unix)]
    #[test]
    fn load_module_rejects_symlinked_import_outside_package() {
        let dir = tempdir().unwrap_or_else(|err| panic!("tempdir: {err}"));
        let package_dir = dir.path().join("package");
        fs::create_dir_all(&package_dir).unwrap_or_else(|err| panic!("create package: {err}"));
        let root =
            fs::canonicalize(&package_dir).unwrap_or_else(|err| panic!("canonical root: {err}"));
        let source_root = root.join("src");
        fs::create_dir_all(&source_root).unwrap_or_else(|err| panic!("create src: {err}"));
        let entry = source_root.join("main.ax");
        let outside = dir.path().join("outside.ax");
        fs::write(&entry, "import \"escape.ax\"\n")
            .unwrap_or_else(|err| panic!("write entry: {err}"));
        fs::write(&outside, "fn leaked(): int {\nreturn 7\n}\n")
            .unwrap_or_else(|err| panic!("write outside module: {err}"));
        std::os::unix::fs::symlink(&outside, source_root.join("escape.ax"))
            .unwrap_or_else(|err| panic!("symlink import: {err}"));

        let mut graph = PackageGraph::default();
        graph.packages.insert(
            root.clone(),
            PackageContext {
                root: root.clone(),
                manifest: package_manifest(),
                source_root: fs::canonicalize(&source_root)
                    .unwrap_or_else(|err| panic!("canonical source root: {err}")),
                dependencies: BTreeMap::new(),
                workspace_members: Vec::new(),
            },
        );

        let error = match load_module_recursive(
            &graph,
            &root,
            &entry,
            true,
            &mut Vec::new(),
            &mut HashMap::new(),
            &mut Vec::new(),
        ) {
            Ok(()) => panic!("symlinked import outside package was loaded"),
            Err(error) => error,
        };

        assert_eq!(error.kind, "import");
        assert_eq!(error.message, "stage1 imports must stay inside the package");
    }

    #[cfg(unix)]
    #[test]
    fn output_path_rejects_symlinked_out_dir_outside_package() {
        let dir = tempdir().unwrap_or_else(|err| panic!("tempdir: {err}"));
        let package_dir = dir.path().join("package");
        fs::create_dir_all(&package_dir).unwrap_or_else(|err| panic!("create package: {err}"));
        let root =
            fs::canonicalize(&package_dir).unwrap_or_else(|err| panic!("canonical root: {err}"));
        let outside = dir.path().join("outside");
        fs::create_dir_all(&outside).unwrap_or_else(|err| panic!("create outside: {err}"));
        std::os::unix::fs::symlink(&outside, root.join("dist"))
            .unwrap_or_else(|err| panic!("symlink dist: {err}"));

        let output = root.join("dist").join("demo.generated.rs");
        let error = match ensure_output_path_stays_inside_package(
            &root,
            &output,
            "generated Rust output",
        ) {
            Ok(()) => panic!("symlinked output dir outside package was accepted"),
            Err(error) => error,
        };

        assert_eq!(error.kind, "build");
        assert_eq!(
            error.message,
            "generated Rust output resolves outside the package"
        );
    }
}
