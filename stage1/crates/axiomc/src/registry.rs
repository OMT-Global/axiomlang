use crate::diagnostics::Diagnostic;
use crate::lockfile::validate_lockfile;
use crate::manifest::{
    LOCK_FILENAME, MANIFEST_FILENAME, capability_descriptors, load_manifest, manifest_path,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

const REGISTRY_METADATA_FILENAME: &str = "axiom-registry.toml";
const DEFAULT_ARCHIVE_FILENAME: &str = "package.axp";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryIndex {
    pub version: u32,
    pub packages: BTreeMap<String, Vec<RegistryRelease>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryCapability {
    pub name: String,
    pub enabled: bool,
    pub description: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub allowed: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub unsafe_unrestricted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryRelease {
    pub version: String,
    pub source: String,
    pub manifest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    pub yanked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yank_reason: Option<String>,
    pub capabilities: Vec<RegistryCapability>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PublishOutput {
    pub package: String,
    pub version: String,
    pub release_dir: String,
    pub manifest: String,
    pub archive: String,
    pub signature: String,
    pub archive_hash: String,
}

#[derive(Debug, Clone, Default)]
pub struct PublishOptions {
    pub signing_key: Option<String>,
    pub allow_overwrite: bool,
}

#[derive(Debug, Default, Deserialize)]
struct RawRegistryMetadata {
    archive: Option<String>,
    signature: Option<String>,
    yanked: Option<bool>,
    yank_reason: Option<String>,
}

pub fn publish_package(
    project_root: &Path,
    registry_root: &Path,
    options: &PublishOptions,
) -> Result<PublishOutput, Diagnostic> {
    let project_root = fs::canonicalize(project_root).map_err(|err| {
        Diagnostic::new(
            "publish",
            format!(
                "failed to resolve project root {}: {err}",
                project_root.display()
            ),
        )
        .with_path(project_root.display().to_string())
    })?;
    let manifest = load_manifest(&project_root)?;
    validate_lockfile(&project_root, &manifest)?;
    let package = manifest.package.as_ref().ok_or_else(|| {
        Diagnostic::new("publish", "published packages require a [package] section")
            .with_path(manifest_path(&project_root).display().to_string())
    })?;
    let package_segment = safe_registry_path_segment("package name", &package.name)?;
    let version_segment = safe_registry_path_segment("package version", &package.version)?;
    let release_dir = registry_root.join(package_segment).join(version_segment);
    if release_dir.exists() && !options.allow_overwrite {
        return Err(Diagnostic::new(
            "publish",
            format!(
                "registry release {}@{} already exists; pass --allow-overwrite to replace it",
                package.name, package.version
            ),
        )
        .with_path(release_dir.display().to_string()));
    }
    fs::create_dir_all(&release_dir).map_err(|err| {
        Diagnostic::new(
            "publish",
            format!(
                "failed to create release directory {}: {err}",
                release_dir.display()
            ),
        )
        .with_path(release_dir.display().to_string())
    })?;

    let manifest_out = release_dir.join(MANIFEST_FILENAME);
    fs::copy(project_root.join(MANIFEST_FILENAME), &manifest_out).map_err(|err| {
        Diagnostic::new(
            "publish",
            format!("failed to copy {MANIFEST_FILENAME}: {err}"),
        )
        .with_path(manifest_out.display().to_string())
    })?;
    let lock_out = release_dir.join(LOCK_FILENAME);
    fs::copy(project_root.join(LOCK_FILENAME), &lock_out).map_err(|err| {
        Diagnostic::new("publish", format!("failed to copy {LOCK_FILENAME}: {err}"))
            .with_path(lock_out.display().to_string())
    })?;

    let archive_bytes = render_package_archive(&project_root)?;
    let archive_hash = hash_bytes(&archive_bytes);
    let archive_out = release_dir.join(DEFAULT_ARCHIVE_FILENAME);
    fs::write(&archive_out, &archive_bytes).map_err(|err| {
        Diagnostic::new("publish", format!("failed to write package archive: {err}"))
            .with_path(archive_out.display().to_string())
    })?;
    let signature = render_archive_signature(
        &package.name,
        &package.version,
        &archive_hash,
        options
            .signing_key
            .as_deref()
            .unwrap_or("axiom-stage1-dev-key"),
    );
    let signature_out = release_dir.join(format!("{DEFAULT_ARCHIVE_FILENAME}.sig"));
    fs::write(&signature_out, signature).map_err(|err| {
        Diagnostic::new(
            "publish",
            format!("failed to write package signature: {err}"),
        )
        .with_path(signature_out.display().to_string())
    })?;

    Ok(PublishOutput {
        package: package.name.clone(),
        version: package.version.clone(),
        release_dir: release_dir.display().to_string(),
        manifest: manifest_out.display().to_string(),
        archive: archive_out.display().to_string(),
        signature: signature_out.display().to_string(),
        archive_hash,
    })
}

fn render_package_archive(project_root: &Path) -> Result<Vec<u8>, Diagnostic> {
    let mut files = publishable_files(project_root)?;
    files.sort();
    let mut archive = Vec::new();
    archive.extend_from_slice(b"AXIOM_PACKAGE_ARCHIVE_V1\n");
    for path in files {
        let relative = path.strip_prefix(project_root).unwrap_or(&path);
        let relative = normalize_archive_path(relative)?;
        let content = fs::read(&path).map_err(|err| {
            Diagnostic::new(
                "publish",
                format!("failed to read {}: {err}", path.display()),
            )
            .with_path(path.display().to_string())
        })?;
        archive
            .extend_from_slice(format!("--- file {relative} {} ---\n", content.len()).as_bytes());
        archive.extend_from_slice(&content);
        if !content.ends_with(b"\n") {
            archive.push(b'\n');
        }
    }
    Ok(archive)
}

fn publishable_files(project_root: &Path) -> Result<Vec<PathBuf>, Diagnostic> {
    let mut files = Vec::new();
    collect_publishable_files(project_root, &mut files)?;
    Ok(files)
}

fn collect_publishable_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), Diagnostic> {
    for entry in fs::read_dir(dir).map_err(|err| {
        Diagnostic::new(
            "publish",
            format!("failed to read {}: {err}", dir.display()),
        )
        .with_path(dir.display().to_string())
    })? {
        let entry = entry.map_err(|err| {
            Diagnostic::new("publish", format!("failed to read directory entry: {err}"))
        })?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if matches!(name.as_ref(), ".git" | "target" | "dist") {
                continue;
            }
            collect_publishable_files(&path, files)?;
        } else if should_publish_file(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn should_publish_file(path: &Path) -> bool {
    if path
        .file_name()
        .is_some_and(|name| name == MANIFEST_FILENAME || name == LOCK_FILENAME)
    {
        return true;
    }
    path.extension().is_some_and(|extension| extension == "ax")
}

fn normalize_archive_path(path: &Path) -> Result<String, Diagnostic> {
    let mut out = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => out.push(value.to_string_lossy().to_string()),
            Component::CurDir => {}
            _ => {
                return Err(Diagnostic::new(
                    "publish",
                    format!("unsupported archive path component in {}", path.display()),
                ));
            }
        }
    }
    Ok(out.join("/"))
}

fn render_archive_signature(
    package: &str,
    version: &str,
    archive_hash: &str,
    signing_key: &str,
) -> String {
    let signature =
        hash_bytes(format!("{signing_key}\0{package}\0{version}\0{archive_hash}").as_bytes());
    format!(
        "axiom-signature-v1\npackage={package}\nversion={version}\narchive_hash={archive_hash}\nsignature={signature}\n"
    )
}

fn hash_bytes(value: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub fn build_registry_index(
    packages_root: &Path,
    base_url: &str,
) -> Result<RegistryIndex, Diagnostic> {
    let base_url = normalize_base_url(base_url, packages_root)?;
    let mut packages = BTreeMap::new();
    for package_dir in read_sorted_dirs(packages_root)? {
        let package_name = file_name(&package_dir)?;
        let mut releases = Vec::new();
        for version_dir in read_sorted_dirs(&package_dir)? {
            let release = load_release(&package_name, &version_dir, &base_url)?;
            releases.push(release);
        }
        if !releases.is_empty() {
            packages.insert(package_name, releases);
        }
    }
    Ok(RegistryIndex {
        version: 1,
        packages,
    })
}

pub fn render_registry_index(packages_root: &Path, base_url: &str) -> Result<String, Diagnostic> {
    let index = build_registry_index(packages_root, base_url)?;
    serde_json::to_string_pretty(&index).map_err(|err| {
        Diagnostic::new(
            "registry",
            format!("failed to render registry index: {err}"),
        )
    })
}

pub fn load_registry_index(path: &Path) -> Result<RegistryIndex, Diagnostic> {
    let content = fs::read_to_string(path).map_err(|err| {
        Diagnostic::new("registry", format!("failed to read registry index: {err}"))
            .with_path(path.display().to_string())
    })?;
    let index: RegistryIndex = serde_json::from_str(&content).map_err(|err| {
        Diagnostic::new("registry", format!("invalid registry index: {err}"))
            .with_path(path.display().to_string())
    })?;
    validate_registry_index(&index, Some(path))?;
    Ok(index)
}

pub fn validate_registry_index(
    index: &RegistryIndex,
    path: Option<&Path>,
) -> Result<(), Diagnostic> {
    if index.version != 1 {
        return Err(registry_error(
            path,
            format!(
                "unsupported registry index version {}; expected 1",
                index.version
            ),
        ));
    }
    for (package, releases) in &index.packages {
        if package.trim().is_empty() {
            return Err(registry_error(path, "package names must not be empty"));
        }
        let mut seen_versions = std::collections::BTreeSet::new();
        for release in releases {
            if release.version.trim().is_empty() {
                return Err(registry_error(
                    path,
                    format!("package {package:?} contains an empty version string"),
                ));
            }
            if !seen_versions.insert(release.version.clone()) {
                return Err(registry_error(
                    path,
                    format!(
                        "package {package:?} contains duplicate version {:?}",
                        release.version
                    ),
                ));
            }
            if release.archive.is_some() && release.signature.is_none() {
                return Err(registry_error(
                    path,
                    format!(
                        "package {package:?} version {:?} declares an archive without a signature",
                        release.version
                    ),
                ));
            }
            if release.yank_reason.is_some() && !release.yanked {
                return Err(registry_error(
                    path,
                    format!(
                        "package {package:?} version {:?} has yank_reason but is not yanked",
                        release.version
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn load_release(
    package_name: &str,
    version_dir: &Path,
    base_url: &str,
) -> Result<RegistryRelease, Diagnostic> {
    let version = file_name(version_dir)?;
    let manifest = load_manifest(version_dir)?;
    let manifest_path = manifest_path(version_dir);
    let package = manifest.package.as_ref().ok_or_else(|| {
        Diagnostic::new(
            "registry",
            "registry release manifest requires a [package] section",
        )
        .with_path(manifest_path.display().to_string())
    })?;
    if package.name != package_name {
        return Err(Diagnostic::new(
            "registry",
            format!(
                "package directory {:?} does not match manifest package name {:?}",
                package_name, package.name
            ),
        )
        .with_path(manifest_path.display().to_string()));
    }
    if package.version != version {
        return Err(Diagnostic::new(
            "registry",
            format!(
                "version directory {:?} does not match manifest package version {:?}",
                version, package.version
            ),
        )
        .with_path(manifest_path.display().to_string()));
    }
    let metadata = load_registry_metadata(version_dir)?;
    let yanked = metadata.yanked.unwrap_or(false);
    if metadata.yank_reason.is_some() && !yanked {
        return Err(Diagnostic::new(
            "registry",
            format!(
                "registry release {package_name}@{version} declares yank_reason but is not yanked"
            ),
        )
        .with_path(
            version_dir
                .join(REGISTRY_METADATA_FILENAME)
                .display()
                .to_string(),
        ));
    }
    let archive_file = match metadata.archive {
        Some(value) => Some(value),
        None => version_dir
            .join(DEFAULT_ARCHIVE_FILENAME)
            .exists()
            .then(|| String::from(DEFAULT_ARCHIVE_FILENAME)),
    };
    let signature_file = match metadata.signature {
        Some(value) => Some(value),
        None => archive_file.as_ref().and_then(|archive| {
            version_dir
                .join(format!("{archive}.sig"))
                .exists()
                .then(|| format!("{archive}.sig"))
        }),
    };
    if archive_file.is_some() && signature_file.is_none() {
        return Err(Diagnostic::new(
            "registry",
            format!(
                "registry release {package_name}@{version} includes an archive but no signature"
            ),
        )
        .with_path(version_dir.display().to_string()));
    }
    Ok(RegistryRelease {
        version: package.version.clone(),
        source: format!("registry+{}/{}/{}", base_url, package_name, version),
        manifest: format!("{}/{}/{}/axiom.toml", base_url, package_name, version),
        archive: archive_file
            .map(|file| format!("{}/{}/{}/{}", base_url, package_name, version, file)),
        signature: signature_file
            .map(|file| format!("{}/{}/{}/{}", base_url, package_name, version, file)),
        yanked,
        yank_reason: metadata.yank_reason,
        capabilities: capability_descriptors(&manifest.capabilities)
            .into_iter()
            .map(|capability| RegistryCapability {
                name: capability.name,
                enabled: capability.enabled,
                description: capability.description.to_string(),
                allowed: capability.allowed,
                unsafe_unrestricted: capability.unsafe_unrestricted,
            })
            .collect(),
    })
}

fn load_registry_metadata(version_dir: &Path) -> Result<RawRegistryMetadata, Diagnostic> {
    let path = version_dir.join(REGISTRY_METADATA_FILENAME);
    if !path.exists() {
        return Ok(RawRegistryMetadata::default());
    }
    let content = fs::read_to_string(&path).map_err(|err| {
        Diagnostic::new(
            "registry",
            format!("failed to read {REGISTRY_METADATA_FILENAME}: {err}"),
        )
        .with_path(path.display().to_string())
    })?;
    toml::from_str(&content).map_err(|err| {
        Diagnostic::new(
            "registry",
            format!("invalid {REGISTRY_METADATA_FILENAME}: {err}"),
        )
        .with_path(path.display().to_string())
    })
}

fn read_sorted_dirs(path: &Path) -> Result<Vec<PathBuf>, Diagnostic> {
    let mut dirs = fs::read_dir(path)
        .map_err(|err| {
            Diagnostic::new(
                "registry",
                format!("failed to read {}: {err}", path.display()),
            )
            .with_path(path.display().to_string())
        })?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|entry| entry.is_dir())
        .collect::<Vec<_>>();
    dirs.sort();
    Ok(dirs)
}

fn file_name(path: &Path) -> Result<String, Diagnostic> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .ok_or_else(|| Diagnostic::new("registry", format!("invalid path {}", path.display())))
}

fn normalize_base_url(base_url: &str, packages_root: &Path) -> Result<String, Diagnostic> {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(Diagnostic::new("registry", "--base-url must not be empty")
            .with_path(packages_root.display().to_string()));
    }
    Ok(trimmed.to_string())
}

fn safe_registry_path_segment(kind: &str, value: &str) -> Result<String, Diagnostic> {
    let trimmed = value.trim();
    let invalid = trimmed.is_empty()
        || trimmed != value
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || Path::new(trimmed)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)));
    if invalid {
        return Err(Diagnostic::new(
            "publish",
            format!("registry {kind} must be a safe path segment: {value:?}"),
        ));
    }
    Ok(trimmed.to_string())
}

fn registry_error(path: Option<&Path>, message: impl Into<String>) -> Diagnostic {
    let diagnostic = Diagnostic::new("registry", message.into());
    if let Some(path) = path {
        diagnostic.with_path(path.display().to_string())
    } else {
        diagnostic
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_release(root: &Path, package: &str, version: &str, manifest: &str) -> PathBuf {
        let dir = root.join(package).join(version);
        fs::create_dir_all(&dir).expect("create release dir");
        fs::write(dir.join("axiom.toml"), manifest).expect("write manifest");
        dir
    }

    fn write_publishable_project(root: &Path, package: &str, version: &str) -> PathBuf {
        let project = root.join(package);
        fs::create_dir_all(project.join("src")).expect("create project src");
        fs::write(
            project.join("axiom.toml"),
            format!(
                "[package]\nname = {package:?}\nversion = {version:?}\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n"
            ),
        )
        .expect("write manifest");
        fs::write(
            project.join("axiom.lock"),
            format!("version = 1\n\n[[package]]\nname = {package:?}\nversion = {version:?}\nsource = \"path\"\n"),
        )
        .expect("write lockfile");
        fs::write(project.join("src/main.ax"), "print \"hello\"\n").expect("write source");
        project
    }

    #[test]
    fn publishes_package_archive_signature_and_registry_index_release() {
        let dir = tempdir().expect("tempdir");
        let project = write_publishable_project(dir.path(), "core", "1.0.0");
        let registry = dir.path().join("registry");

        let output = publish_package(
            &project,
            &registry,
            &PublishOptions {
                signing_key: Some(String::from("test-key")),
                allow_overwrite: false,
            },
        )
        .expect("publish package");

        assert_eq!(output.package, "core");
        assert_eq!(output.version, "1.0.0");
        let release = registry.join("core").join("1.0.0");
        assert!(release.join("axiom.toml").exists());
        assert!(release.join("axiom.lock").exists());
        let archive = fs::read_to_string(release.join("package.axp")).expect("read archive");
        assert!(archive.contains("AXIOM_PACKAGE_ARCHIVE_V1"));
        assert!(archive.contains("--- file src/main.ax"));
        let signature =
            fs::read_to_string(release.join("package.axp.sig")).expect("read signature");
        assert!(signature.contains("axiom-signature-v1"));
        assert!(signature.contains(&format!("archive_hash={}", output.archive_hash)));

        let index = build_registry_index(&registry, "https://packages.example.test")
            .expect("build registry index");
        let release = &index.packages["core"][0];
        assert_eq!(
            release.archive.as_deref(),
            Some("https://packages.example.test/core/1.0.0/package.axp")
        );
        assert_eq!(
            release.signature.as_deref(),
            Some("https://packages.example.test/core/1.0.0/package.axp.sig")
        );
    }

    #[test]
    fn publish_rejects_existing_release_without_overwrite() {
        let dir = tempdir().expect("tempdir");
        let project = write_publishable_project(dir.path(), "core", "1.0.0");
        let registry = dir.path().join("registry");
        publish_package(&project, &registry, &PublishOptions::default()).expect("initial publish");

        let error = publish_package(&project, &registry, &PublishOptions::default())
            .expect_err("duplicate publish should fail");

        assert_eq!(error.kind, "publish");
        assert!(error.message.contains("already exists"));
    }

    #[test]
    fn publish_rejects_traversal_package_name() {
        let dir = tempdir().expect("tempdir");
        let project = write_publishable_project(dir.path(), "../escaped-publish", "1.0.0");
        let registry = dir.path().join("registry");

        let error = publish_package(&project, &registry, &PublishOptions::default())
            .expect_err("traversal package name should fail");

        assert_eq!(error.kind, "publish");
        assert!(error.message.contains("package name"));
        assert!(!dir.path().join("escaped-publish").exists());
    }

    #[test]
    fn publish_rejects_traversal_package_version() {
        let dir = tempdir().expect("tempdir");
        let project = write_publishable_project(dir.path(), "core", "../escaped-version");
        let registry = dir.path().join("registry");

        let error = publish_package(&project, &registry, &PublishOptions::default())
            .expect_err("traversal package version should fail");

        assert_eq!(error.kind, "publish");
        assert!(error.message.contains("package version"));
        assert!(!dir.path().join("registry").join("escaped-version").exists());
        assert!(!dir.path().join("escaped-version").exists());
    }

    #[test]
    fn builds_static_registry_index_with_capabilities_and_yanks() {
        let dir = tempdir().expect("tempdir");
        let release = write_release(
            dir.path(),
            "core",
            "1.2.3",
            "[package]\nname = \"core\"\nversion = \"1.2.3\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nnet = true\nenv = [\"API_TOKEN\"]\n",
        );
        fs::write(release.join("package.axp"), "archive").expect("write archive");
        fs::write(release.join("package.axp.sig"), "signature").expect("write signature");
        fs::write(
            release.join("axiom-registry.toml"),
            "yanked = true\nyank_reason = \"security fix required\"\n",
        )
        .expect("write metadata");

        let index = build_registry_index(dir.path(), "https://packages.example.test/registry/")
            .expect("build index");
        let release = &index.packages["core"][0];
        assert_eq!(
            release.source,
            "registry+https://packages.example.test/registry/core/1.2.3"
        );
        assert_eq!(
            release.archive.as_deref(),
            Some("https://packages.example.test/registry/core/1.2.3/package.axp")
        );
        assert_eq!(
            release.signature.as_deref(),
            Some("https://packages.example.test/registry/core/1.2.3/package.axp.sig")
        );
        assert!(release.yanked);
        assert_eq!(
            release.yank_reason.as_deref(),
            Some("security fix required")
        );
        assert!(
            release
                .capabilities
                .iter()
                .any(|cap| cap.name == "net" && cap.enabled)
        );
        let env = release
            .capabilities
            .iter()
            .find(|cap| cap.name == "env")
            .expect("env cap");
        assert_eq!(env.allowed, vec![String::from("API_TOKEN")]);
    }

    #[test]
    fn rejects_unsigned_archives() {
        let dir = tempdir().expect("tempdir");
        let release = write_release(
            dir.path(),
            "core",
            "1.0.0",
            "[package]\nname = \"core\"\nversion = \"1.0.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n",
        );
        fs::write(release.join("package.axp"), "archive").expect("write archive");
        let error = build_registry_index(dir.path(), "https://packages.example.test")
            .expect_err("unsigned archive should fail");
        assert_eq!(error.kind, "registry");
        assert!(error.message.contains("archive but no signature"));
    }

    #[test]
    fn rejects_yank_reason_without_yanked_metadata() {
        let dir = tempdir().expect("tempdir");
        let release = write_release(
            dir.path(),
            "core",
            "1.0.0",
            "[package]\nname = \"core\"\nversion = \"1.0.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n",
        );
        fs::write(
            release.join("axiom-registry.toml"),
            "yank_reason = \"metadata drift\"\n",
        )
        .expect("write metadata");

        let error = build_registry_index(dir.path(), "https://packages.example.test")
            .expect_err("yank_reason without yanked should fail");
        assert_eq!(error.kind, "registry");
        assert!(error.message.contains("yank_reason but is not yanked"));
        assert!(
            error
                .path
                .as_deref()
                .is_some_and(|path| path.ends_with("axiom-registry.toml"))
        );
    }

    #[test]
    fn validates_index_file_contract() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("index.json");
        fs::write(
            &path,
            r#"{"version":1,"packages":{"core":[{"version":"1.0.0","source":"registry+https://packages.example.test/core/1.0.0","manifest":"https://packages.example.test/core/1.0.0/axiom.toml","archive":"https://packages.example.test/core/1.0.0/package.axp","signature":"https://packages.example.test/core/1.0.0/package.axp.sig","yanked":false,"capabilities":[]}]}}"#,
        )
        .expect("write index");
        load_registry_index(&path).expect("valid index");
    }
}
