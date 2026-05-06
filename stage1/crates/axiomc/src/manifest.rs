use crate::diagnostics::Diagnostic;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const MANIFEST_FILENAME: &str = "axiom.toml";
pub const LOCK_FILENAME: &str = "axiom.lock";
pub const KNOWN_CAPABILITIES: [CapabilityKind; 9] = [
>>>>>>> origin/codex/issue-380-doc-json
>>>>>>> origin/codex/issue-376-doctor-json
>>>>>>> origin/codex/issue-377-inspect-symbols
>>>>>>> origin/codex/issue-378-inspect-graph
>>>>>>> origin/codex/issue-406-collection-lookup
>>>>>>> origin/codex/issue-383-new-templates
>>>>>>> origin/codex/agent-g-regex
>>>>>>> origin/codex/agent-f-fs
>>>>>>> origin/codex/agent-i-language-slice
>>>>>>> origin/codex/issue-387-capability-validation
>>>>>>> origin/codex/issue-395-effective-fs-roots
>>>>>>> origin/codex/worker-h-issue-413
>>>>>>> origin/codex/worker-j-issue-362
>>>>>>> origin/codex/worker-j-issue-363
pub const KNOWN_CAPABILITIES: [CapabilityKind; 8] = [
    CapabilityKind::Fs,
    CapabilityKind::FsWrite,
    CapabilityKind::Net,
    CapabilityKind::Process,
    CapabilityKind::Env,
    CapabilityKind::Clock,
    CapabilityKind::Crypto,
    CapabilityKind::Ffi,
    CapabilityKind::Async,
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Manifest {
    pub package: Option<PackageSection>,
    pub dependencies: BTreeMap<String, DependencySpec>,
    pub workspace: Option<WorkspaceSection>,
    pub build: BuildSection,
    pub tests: Vec<TestTarget>,
    pub capabilities: CapabilityConfig,
    pub publish: PublishSection,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PackageSection {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkspaceSection {
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BuildSection {
    pub entry: String,
    pub out_dir: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DependencySpec {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TestTarget {
    pub name: String,
    pub entry: String,
    pub stdout: Option<String>,
    pub kind: TestKind,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord, Default)]
#[serde(rename_all = "snake_case")]
pub enum TestKind {
    #[default]
    Unit,
    Table,
    Property,
    Snapshot,
    Benchmark,
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
    pub stderr: Option<String>,
=======
=======
=======
=======
=======
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct PublishSection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
=======
=======
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct CapabilityConfig {
    pub fs: bool,
    pub fs_write: bool,
    pub fs_root: Option<String>,
    pub net: bool,
    pub process: bool,
    pub env: bool,
    pub env_vars: Vec<String>,
    pub env_unrestricted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsafe_rationale: Option<String>,
    #[serde(skip_serializing)]
    pub env_legacy_unrestricted: bool,
    pub clock: bool,
    pub crypto: bool,
    pub ffi: bool,
    pub async_runtime: bool,
    pub deny_by_default: bool,
    pub unsafe_opt_ins: Vec<String>,
    pub owners: BTreeMap<String, String>,
    pub rationale: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    Fs,
    FsWrite,
    Net,
    Process,
    Env,
    Clock,
    Crypto,
    Ffi,
    Async,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CapabilityDescriptor {
    pub name: String,
    pub enabled: bool,
    pub description: &'static str,
    #[serde(skip_serializing_if = "is_false")]
    pub deny_by_default: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configured_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_root: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub unsafe_unrestricted: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub unsafe_opt_in: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsafe_rationale: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    package: Option<RawPackageSection>,
    dependencies: Option<BTreeMap<String, RawDependencySpec>>,
    workspace: Option<RawWorkspaceSection>,
    build: Option<RawBuildSection>,
    tests: Option<Vec<RawTestTarget>>,
    capabilities: Option<RawCapabilityConfig>,
    publish: Option<RawPublishSection>,
    registry: Option<toml::Value>,
    publish: Option<toml::Value>,
}

#[derive(Debug, Deserialize)]
struct RawPackageSection {
    name: Option<String>,
    version: Option<String>,
    checksum: Option<toml::Value>,
    registry: Option<toml::Value>,
    source: Option<toml::Value>,
}

#[derive(Debug, Deserialize)]
struct RawWorkspaceSection {
    members: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct RawBuildSection {
    entry: Option<String>,
    out_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawDependencySpec {
    Path(String),
    Detailed(RawDependencyDetail),
}

#[derive(Debug, Deserialize)]
struct RawDependencyDetail {
    path: Option<String>,
    version: Option<String>,
    version: Option<toml::Value>,
    checksum: Option<toml::Value>,
    registry: Option<toml::Value>,
    source: Option<toml::Value>,
}

#[derive(Debug, Deserialize)]
struct RawTestTarget {
    name: Option<String>,
    entry: Option<String>,
    stdout: Option<String>,
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
    kind: Option<String>,
    stderr: Option<String>,
=======
    kind: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawPublishSection {
    registry: Option<String>,
    checksum: Option<String>,
    include: Option<Vec<String>>,
=======
=======
    kind: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawCapabilityConfig {
    fs: Option<bool>,
    #[serde(rename = "fs:write")]
    fs_write: Option<bool>,
    fs_root: Option<String>,
    net: Option<bool>,
    process: Option<bool>,
    env: Option<RawEnvCapability>,
    env_unrestricted: Option<bool>,
    unsafe_rationale: Option<String>,
    clock: Option<bool>,
    crypto: Option<bool>,
    ffi: Option<bool>,
    #[serde(rename = "async")]
    async_runtime: Option<bool>,
>>>>>>> origin/codex/issue-406-collection-lookup
>>>>>>> origin/codex/agent-f-fs
>>>>>>> origin/codex/issue-387-capability-validation
>>>>>>> origin/codex/worker-h-issue-413
    deny_by_default: Option<bool>,
    unsafe_opt_ins: Option<Vec<String>>,
    owners: Option<BTreeMap<String, String>>,
    rationale: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawEnvCapability {
    LegacyBool(bool),
    AllowList(Vec<String>),
}

fn is_false(value: &bool) -> bool {
    !*value
}

pub fn load_manifest(project_root: &Path) -> Result<Manifest, Diagnostic> {
    let path = manifest_path(project_root);
    let content = std::fs::read_to_string(&path).map_err(|err| {
        Diagnostic::new(
            "manifest",
            format!("failed to read {}: {err}", MANIFEST_FILENAME),
        )
        .with_path(path.display().to_string())
    })?;
    let raw: RawManifest = toml::from_str(&content).map_err(|err| {
        Diagnostic::new("manifest", format!("invalid {MANIFEST_FILENAME}: {err}"))
            .with_path(path.display().to_string())
    })?;
    normalize_manifest(raw, &path)
}

pub fn manifest_path(project_root: &Path) -> PathBuf {
    project_root.join(MANIFEST_FILENAME)
}

pub fn lockfile_path(project_root: &Path) -> PathBuf {
    project_root.join(LOCK_FILENAME)
}

pub fn entry_path(project_root: &Path, manifest: &Manifest) -> PathBuf {
    project_root.join(&manifest.build.entry)
}

pub fn out_dir_path(project_root: &Path, manifest: &Manifest) -> PathBuf {
    project_root.join(&manifest.build.out_dir)
}

pub fn binary_path(project_root: &Path, manifest: &Manifest) -> PathBuf {
    binary_path_for_target(project_root, manifest, None)
}

pub fn binary_path_for_target(
    project_root: &Path,
    manifest: &Manifest,
    target: Option<&str>,
) -> PathBuf {
    let suffix = match target {
        Some(target) if target.starts_with("wasm32") => ".wasm",
        _ if cfg!(windows) => ".exe",
        _ => "",
    };
    let package = manifest
        .package
        .as_ref()
        .expect("binary path requires a package manifest");
    out_dir_path(project_root, manifest).join(format!("{}{}", package.name, suffix))
}

pub fn generated_rust_path(project_root: &Path, manifest: &Manifest) -> PathBuf {
    let package = manifest
        .package
        .as_ref()
        .expect("generated rust path requires a package manifest");
    out_dir_path(project_root, manifest).join(format!("{}.generated.rs", package.name))
}

pub fn capability_descriptors(config: &CapabilityConfig) -> Vec<CapabilityDescriptor> {
    KNOWN_CAPABILITIES
        .iter()
        .map(|kind| CapabilityDescriptor {
            name: kind.name().to_string(),
            enabled: config.enabled(*kind),
            description: kind.description(),
            deny_by_default: config.deny_by_default,
            allowed: if *kind == CapabilityKind::Env {
                config.env_vars.clone()
            } else {
                Vec::new()
            },
            configured_root: None,
            effective_root: None,
            package_root: None,
            unsafe_unrestricted: *kind == CapabilityKind::Env && config.env_unrestricted,
            unsafe_opt_in: config.unsafe_opt_ins.iter().any(|name| name == kind.name()),
            owner: config.owners.get(kind.name()).cloned(),
            rationale: config.rationale.get(kind.name()).cloned(),
            unsafe_rationale: (*kind == CapabilityKind::Env && config.env_unrestricted)
                .then(|| config.unsafe_rationale.clone())
                .flatten(),
        })
        .collect()
}

pub fn render_manifest(name: &str) -> String {
    format!(
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
        "[package]\nname = {name:?}\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\n\"fs:write\" = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\nffi = false\nasync = false\n"
=======
=======
=======
=======
        "[package]\nname = {name:?}\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\n\"fs:write\" = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\nffi = false\n"
    )
}

impl CapabilityConfig {
    pub fn enabled(&self, kind: CapabilityKind) -> bool {
        match kind {
            CapabilityKind::Fs => self.fs,
            CapabilityKind::FsWrite => self.fs_write,
            CapabilityKind::Net => self.net,
            CapabilityKind::Process => self.process,
            CapabilityKind::Env => self.env,
            CapabilityKind::Clock => self.clock,
            CapabilityKind::Crypto => self.crypto,
            CapabilityKind::Ffi => self.ffi,
            CapabilityKind::Async => self.async_runtime,
        }
    }

    pub fn warnings(&self) -> Vec<String> {
        if self.env_legacy_unrestricted {
            vec![String::from(
                "warning: [capabilities].env = true is deprecated and grants unrestricted environment access; prefer env = [\"NAME\"] or use env_unrestricted = true only during migration",
            )]
        } else if self.env_unrestricted {
            vec![String::from(
                "warning: [capabilities].env_unrestricted = true grants unrestricted environment access and bypasses the env allowlist",
            )]
        } else {
            Vec::new()
        }
    }
}

impl Manifest {
    pub fn is_workspace_only(&self) -> bool {
        self.package.is_none()
    }
}

impl CapabilityKind {
    pub fn from_name(name: &str) -> Option<Self> {
        KNOWN_CAPABILITIES
            .iter()
            .copied()
            .find(|kind| kind.name() == name)
    }

    pub fn name(self) -> &'static str {
        match self {
            CapabilityKind::Fs => "fs",
            CapabilityKind::FsWrite => "fs:write",
            CapabilityKind::Net => "net",
            CapabilityKind::Process => "process",
            CapabilityKind::Env => "env",
            CapabilityKind::Clock => "clock",
            CapabilityKind::Crypto => "crypto",
            CapabilityKind::Ffi => "ffi",
            CapabilityKind::Async => "async",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            CapabilityKind::Fs => "filesystem read access",
            CapabilityKind::FsWrite => "filesystem write access",
            CapabilityKind::Net => "network access",
            CapabilityKind::Process => "child process execution",
            CapabilityKind::Env => "environment variable access",
            CapabilityKind::Clock => "wall-clock time access",
            CapabilityKind::Crypto => "hashing and cryptography primitives",
            CapabilityKind::Ffi => "foreign function interface access",
            CapabilityKind::Async => "host async runtime access",
        }
    }
}

fn normalize_manifest(raw: RawManifest, path: &Path) -> Result<Manifest, Diagnostic> {
    validate_reserved_root_publish_fields(&raw, path)?;
    let workspace = normalize_workspace(raw.workspace, path)?;
    let package = normalize_package(raw.package, workspace.is_some(), path)?;
    let raw_build = raw.build;
    if package.is_none() && raw_build.is_some() {
        return Err(
            Diagnostic::new("manifest", "[build] requires a [package] section")
                .with_path(path.display().to_string()),
        );
    }
    let build = raw_build.unwrap_or(RawBuildSection {
        entry: Some(String::from("src/main.ax")),
        out_dir: Some(String::from("dist")),
    });
    let entry = required_field(build.entry, path, "build.entry")?;
    let out_dir = required_field(build.out_dir, path, "build.out_dir")?;
    validate_relative_path(path, "build.entry", &entry)?;
    validate_relative_path(path, "build.out_dir", &out_dir)?;
    let dependencies = normalize_dependencies(raw.dependencies.unwrap_or_default(), path)?;
    let tests = normalize_tests(raw.tests.unwrap_or_default(), path)?;
    let publish = normalize_publish(raw.publish.unwrap_or_default(), path)?;
    let capabilities = raw.capabilities.unwrap_or_default();
    let fs_root =
        normalize_optional_relative_path(path, "capabilities.fs_root", capabilities.fs_root)?;
    let explicit_env_unrestricted = capabilities.env_unrestricted.unwrap_or(false);
    let (env, env_vars, env_unrestricted, env_legacy_unrestricted) = normalize_env_capability(
        path,
        capabilities.env,
        explicit_env_unrestricted,
    )?;
    let unsafe_rationale = normalize_unsafe_rationale(
        path,
        capabilities.unsafe_rationale,
        explicit_env_unrestricted,
    )?;
    let unsafe_opt_ins = normalize_capability_name_list(
        path,
        "capabilities.unsafe_opt_ins",
        capabilities.unsafe_opt_ins.unwrap_or_default(),
    )?;
    let owners = normalize_capability_string_map(
        path,
        "capabilities.owners",
        capabilities.owners.unwrap_or_default(),
    )?;
    let rationale = normalize_capability_string_map(
        path,
        "capabilities.rationale",
        capabilities.rationale.unwrap_or_default(),
    )?;
    Ok(Manifest {
        package,
        dependencies,
        workspace,
        build: BuildSection { entry, out_dir },
        tests,
        capabilities: CapabilityConfig {
            fs: capabilities.fs.unwrap_or(false),
            fs_write: capabilities.fs_write.unwrap_or(false),
            fs_root,
            net: capabilities.net.unwrap_or(false),
            process: capabilities.process.unwrap_or(false),
            env,
            env_vars,
            env_unrestricted,
            unsafe_rationale,
            env_legacy_unrestricted,
            clock: capabilities.clock.unwrap_or(false),
            crypto: capabilities.crypto.unwrap_or(false),
            ffi: capabilities.ffi.unwrap_or(false),
            async_runtime: capabilities.async_runtime.unwrap_or(false),
            deny_by_default: capabilities.deny_by_default.unwrap_or(false),
            unsafe_opt_ins,
            owners,
            rationale,
        },
        publish,
    })
}

fn normalize_capability_name_list(
    path: &Path,
    field_name: &str,
    values: Vec<String>,
) -> Result<Vec<String>, Diagnostic> {
    let mut names = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (index, value) in values.into_iter().enumerate() {
        let item_field = format!("{field_name}[{index}]");
        let name = normalize_capability_name(path, &item_field, value)?;
        if !seen.insert(name.clone()) {
            return Err(Diagnostic::new(
                "manifest",
                format!("duplicate capability metadata entry {name:?}"),
            )
            .with_path(path.display().to_string()));
        }
        names.push(name);
    }
    Ok(names)
}

fn normalize_capability_string_map(
    path: &Path,
    field_name: &str,
    values: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, Diagnostic> {
    let mut normalized = BTreeMap::new();
    for (name, value) in values {
        let field = format!("{field_name}.{name}");
        let name = normalize_capability_name(path, &field, name)?;
        if value.trim().is_empty() {
            return Err(
                Diagnostic::new("manifest", format!("{field} must not be empty"))
                    .with_path(path.display().to_string()),
            );
        }
        normalized.insert(name, value);
    }
    Ok(normalized)
}

fn normalize_capability_name(
    path: &Path,
    field_name: &str,
    value: String,
) -> Result<String, Diagnostic> {
    let name = required_field(Some(value), path, field_name)?;
    if CapabilityKind::from_name(&name).is_none() {
        return Err(Diagnostic::new(
            "manifest",
            format!("{field_name} references unknown capability {name:?}"),
        )
        .with_path(path.display().to_string()));
    }
    Ok(name)
fn validate_reserved_root_publish_fields(raw: &RawManifest, path: &Path) -> Result<(), Diagnostic> {
    if raw.registry.is_some() {
        return Err(reserved_manifest_field(path, "[registry]"));
    }
    if raw.publish.is_some() {
        return Err(reserved_manifest_field(path, "[publish]"));
    }
    Ok(())
}

fn reserved_manifest_field(path: &Path, field_name: &str) -> Diagnostic {
    Diagnostic::new(
        "manifest",
        format!("{field_name} is reserved for future registry publishing"),
    )
    .with_path(path.display().to_string())
}

fn normalize_env_capability(
    path: &Path,
    raw_env: Option<RawEnvCapability>,
    env_unrestricted: bool,
) -> Result<(bool, Vec<String>, bool, bool), Diagnostic> {
    match raw_env {
        Some(RawEnvCapability::LegacyBool(enabled)) => Ok((
            enabled || env_unrestricted,
            Vec::new(),
            enabled || env_unrestricted,
            enabled,
        )),
        Some(RawEnvCapability::AllowList(values)) => {
            let vars = normalize_env_allowlist(path, values)?;
            Ok((true, vars, env_unrestricted, false))
        }
        None => Ok((env_unrestricted, Vec::new(), env_unrestricted, false)),
    }
}

fn normalize_unsafe_rationale(
    path: &Path,
    rationale: Option<String>,
    env_unrestricted: bool,
) -> Result<Option<String>, Diagnostic> {
    let rationale = rationale.map(|value| value.trim().to_string());
    if env_unrestricted {
        match rationale {
            Some(value) if !value.is_empty() => Ok(Some(value)),
            _ => Err(Diagnostic::new(
                "manifest",
                "capabilities.unsafe_rationale is required when unrestricted environment access is enabled",
            )
            .with_path(path.display().to_string())),
        }
    } else {
        Ok(rationale.filter(|value| !value.is_empty()))
    }
}

fn normalize_env_allowlist(path: &Path, values: Vec<String>) -> Result<Vec<String>, Diagnostic> {
    let mut vars = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (index, value) in values.into_iter().enumerate() {
        let field_name = format!("capabilities.env[{index}]");
        if value.trim().is_empty() {
            return Err(
                Diagnostic::new("manifest", format!("{field_name} must not be empty"))
                    .with_path(path.display().to_string()),
            );
        }
        if value.contains('=') {
            return Err(
                Diagnostic::new("manifest", format!("{field_name} must not contain '='"))
                    .with_path(path.display().to_string()),
            );
        }
        if !seen.insert(value.clone()) {
            return Err(Diagnostic::new(
                "manifest",
                format!("duplicate environment variable {value:?}"),
            )
            .with_path(path.display().to_string()));
        }
        vars.push(value);
    }
    Ok(vars)
}

fn normalize_package(
    raw_package: Option<RawPackageSection>,
    has_workspace: bool,
    path: &Path,
) -> Result<Option<PackageSection>, Diagnostic> {
    let Some(package) = raw_package else {
        if has_workspace {
            return Ok(None);
        }
        return Err(Diagnostic::new("manifest", "missing [package] section")
            .with_path(path.display().to_string()));
    };
    if package.checksum.is_some() {
        return Err(reserved_manifest_field(path, "package.checksum"));
    }
    if package.registry.is_some() {
        return Err(reserved_manifest_field(path, "package.registry"));
    }
    if package.source.is_some() {
        return Err(reserved_manifest_field(path, "package.source"));
    }
    let package_name = required_field(package.name, path, "package.name")?;
    let package_version = required_field(package.version, path, "package.version")?;
    Ok(Some(PackageSection {
        name: package_name,
        version: package_version,
    }))
}

fn normalize_workspace(
    raw_workspace: Option<RawWorkspaceSection>,
    path: &Path,
) -> Result<Option<WorkspaceSection>, Diagnostic> {
    let Some(raw_workspace) = raw_workspace else {
        return Ok(None);
    };
    let mut members = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (index, member) in raw_workspace
        .members
        .unwrap_or_default()
        .into_iter()
        .enumerate()
    {
        let field_name = format!("workspace.members[{index}]");
        let member = required_field(Some(member), path, &field_name)?;
        validate_relative_path(path, &field_name, &member)?;
        if !seen.insert(member.clone()) {
            return Err(Diagnostic::new(
                "manifest",
                format!("duplicate workspace member {member:?}"),
            )
            .with_path(path.display().to_string()));
        }
        members.push(member);
    }
    Ok(Some(WorkspaceSection { members }))
}

fn required_field(
    value: Option<String>,
    path: &Path,
    field_name: &str,
) -> Result<String, Diagnostic> {
    match value {
        Some(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(
            Diagnostic::new("manifest", format!("missing or empty {field_name}"))
                .with_path(path.display().to_string()),
        ),
    }
}

fn normalize_dependencies(
    raw_dependencies: BTreeMap<String, RawDependencySpec>,
    path: &Path,
) -> Result<BTreeMap<String, DependencySpec>, Diagnostic> {
    let mut dependencies = BTreeMap::new();
    for (name, raw_spec) in raw_dependencies {
        if name.trim().is_empty() {
            return Err(
                Diagnostic::new("manifest", "dependency names must not be empty")
                    .with_path(path.display().to_string()),
            );
        }
        match raw_spec {
            RawDependencySpec::Path(value) => {
                validate_dependency_path(path, &format!("dependencies.{name}.path"), &value)?;
                dependencies.insert(
                    name,
                    DependencySpec {
                        path: value,
                        version: None,
                    },
                );
                continue;
            }
            RawDependencySpec::Detailed(detail) => {
                let raw_path =
                    required_field(detail.path, path, &format!("dependencies.{name}.path"))?;
                validate_dependency_path(path, &format!("dependencies.{name}.path"), &raw_path)?;
                let version = normalize_dependency_version(
                    path,
                    &format!("dependencies.{name}.version"),
                    detail.version,
                )?;
                dependencies.insert(
                    name,
                    DependencySpec {
                        path: raw_path,
                        version,
                    },
                );
                if detail.version.is_some() {
                    return Err(reserved_manifest_field(
                        path,
                        &format!("dependencies.{name}.version"),
                    ));
                }
                if detail.checksum.is_some() {
                    return Err(reserved_manifest_field(
                        path,
                        &format!("dependencies.{name}.checksum"),
                    ));
                }
                if detail.registry.is_some() {
                    return Err(reserved_manifest_field(
                        path,
                        &format!("dependencies.{name}.registry"),
                    ));
                }
                if detail.source.is_some() {
                    return Err(reserved_manifest_field(
                        path,
                        &format!("dependencies.{name}.source"),
                    ));
                }
                required_field(detail.path, path, &format!("dependencies.{name}.path"))?
            }
        };
    }
    Ok(dependencies)
}

fn normalize_dependency_version(
    path: &Path,
    field_name: &str,
    version: Option<String>,
) -> Result<Option<String>, Diagnostic> {
    let Some(version) = version else {
        return Ok(None);
    };
    let version = required_field(Some(version), path, field_name)?;
    validate_version_constraint(path, field_name, &version)?;
    Ok(Some(version))
}

fn validate_version_constraint(
    path: &Path,
    field_name: &str,
    version: &str,
) -> Result<(), Diagnostic> {
    if version == "*" {
        return Ok(());
    }
    let candidate = version.strip_prefix('^').unwrap_or(version);
    let parts = candidate.split('.').collect::<Vec<_>>();
    if parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
    {
        return Ok(());
    }
    Err(
        Diagnostic::new(
            "manifest",
            format!("{field_name} must be '*', an exact MAJOR.MINOR.PATCH version, or a caret constraint like ^1.2.3"),
        )
        .with_path(path.display().to_string()),
    )
}

fn normalize_tests(
    raw_tests: Vec<RawTestTarget>,
    path: &Path,
) -> Result<Vec<TestTarget>, Diagnostic> {
    let mut tests = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for (index, raw_test) in raw_tests.into_iter().enumerate() {
        let field_prefix = format!("tests[{index}]");
        let name = required_field(raw_test.name, path, &format!("{field_prefix}.name"))?;
        if !names.insert(name.clone()) {
            return Err(
                Diagnostic::new("manifest", format!("duplicate test target {name:?}"))
                    .with_path(path.display().to_string()),
            );
        }
        let entry = required_field(raw_test.entry, path, &format!("{field_prefix}.entry"))?;
        validate_relative_path(path, &format!("{field_prefix}.entry"), &entry)?;
        tests.push(TestTarget {
            name,
            entry,
            stdout: raw_test.stdout,
            kind: normalize_test_kind(raw_test.kind, path, &format!("{field_prefix}.kind"))?,
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
            stderr: raw_test.stderr,
=======
=======
=======
        });
    }
    Ok(tests)
}

<<<<<<< HEAD
<<<<<<< HEAD
>>>>>>> origin/codex/worker-j-issue-362
=======
fn normalize_test_kind(
    value: Option<String>,
    path: &Path,
    field_name: &str,
) -> Result<TestKind, Diagnostic> {
    match value.as_deref().unwrap_or("unit") {
        "unit" => Ok(TestKind::Unit),
        "table" => Ok(TestKind::Table),
        "property" => Ok(TestKind::Property),
        "snapshot" => Ok(TestKind::Snapshot),
        "benchmark" => Ok(TestKind::Benchmark),
        other => Err(Diagnostic::new(
            "manifest",
            format!(
                "{field_name} must be one of unit, table, property, snapshot, or benchmark; got {other:?}"
            ),
        )
        .with_path(path.display().to_string())),
    }
=======
fn normalize_publish(raw: RawPublishSection, path: &Path) -> Result<PublishSection, Diagnostic> {
    let registry = match raw.registry {
        Some(registry) => {
            let registry = required_field(Some(registry), path, "publish.registry")?;
            validate_registry_source(path, &registry)?;
            Some(registry)
        }
        None => None,
    };
    let checksum = match raw.checksum {
        Some(checksum) => {
            let checksum = required_field(Some(checksum), path, "publish.checksum")?;
            validate_sha256_checksum(path, &checksum)?;
            Some(checksum)
        }
        None => None,
    };
    let mut include = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (index, value) in raw.include.unwrap_or_default().into_iter().enumerate() {
        let field_name = format!("publish.include[{index}]");
        let value = required_field(Some(value), path, &field_name)?;
        validate_relative_path(path, &field_name, &value)?;
        if !seen.insert(value.clone()) {
            return Err(Diagnostic::new(
                "manifest",
                format!("duplicate publish include path {value:?}"),
            )
            .with_path(path.display().to_string()));
        }
        include.push(value);
    }
    Ok(PublishSection {
        registry,
        checksum,
        include,
    })
}

fn validate_registry_source(path: &Path, registry: &str) -> Result<(), Diagnostic> {
    if let Some(rest) = registry.strip_prefix("https://") {
        let host = rest.split('/').next().unwrap_or_default();
        if !host.is_empty() && !host.chars().any(char::is_whitespace) {
            return Ok(());
        }
    } else if let Some(rest) = registry.strip_prefix("file:") {
        if !rest.is_empty() && !rest.chars().any(char::is_whitespace) {
            return Ok(());
        }
    }
    Err(Diagnostic::new(
        "manifest",
        "publish.registry must be a valid https:// or file: registry source",
    )
    .with_path(path.display().to_string()))
}

fn validate_sha256_checksum(path: &Path, checksum: &str) -> Result<(), Diagnostic> {
    let Some(hex) = checksum.strip_prefix("sha256:") else {
        return Err(Diagnostic::new(
            "manifest",
            "publish.checksum must use sha256:<64 lowercase hex characters>",
        )
        .with_path(path.display().to_string()));
    };
    if hex.len() == 64
        && hex
            .chars()
            .all(|ch| ch.is_ascii_digit() || ('a'..='f').contains(&ch))
    {
        return Ok(());
    }
    Err(Diagnostic::new(
        "manifest",
        "publish.checksum must use sha256:<64 lowercase hex characters>",
    )
    .with_path(path.display().to_string()))
}

fn normalize_optional_relative_path(
    path: &Path,
    field_name: &str,
    value: Option<String>,
) -> Result<Option<String>, Diagnostic> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = required_field(Some(value), path, field_name)?;
    validate_relative_path(path, field_name, &value)?;
    Ok(Some(value))
}

fn validate_relative_path(path: &Path, field_name: &str, value: &str) -> Result<(), Diagnostic> {
    let candidate = Path::new(value);
    if candidate.is_absolute() {
        return Err(
            Diagnostic::new("manifest", format!("{field_name} must be relative"))
                .with_path(path.display().to_string()),
        );
    }
    if candidate
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(Diagnostic::new(
            "manifest",
            format!("{field_name} must not use parent traversal"),
        )
        .with_path(path.display().to_string()));
    }
    Ok(())
}

fn validate_dependency_path(path: &Path, field_name: &str, value: &str) -> Result<(), Diagnostic> {
    if Path::new(value).is_absolute() {
        return Err(
            Diagnostic::new("manifest", format!("{field_name} must be relative"))
                .with_path(path.display().to_string()),
        );
    }
    Ok(())
}
