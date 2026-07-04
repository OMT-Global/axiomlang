use crate::diagnostics::Diagnostic;
use crate::manifest::{DependencySpec, Manifest, load_manifest, lockfile_path, manifest_path};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    pub version: u32,
    pub package: Vec<LockedPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LockedPackage {
    pub name: String,
    pub version: String,
    pub source: String,
}

pub fn expected_lockfile(manifest: &Manifest) -> Lockfile {
    Lockfile {
        version: 1,
        package: manifest
            .package
            .as_ref()
            .map(|package| {
                vec![LockedPackage {
                    name: package.name.clone(),
                    version: package.version.clone(),
                    source: String::from("path"),
                }]
            })
            .unwrap_or_default(),
    }
}

pub fn expected_lockfile_for_project(
    project_root: &Path,
    manifest: &Manifest,
) -> Result<Lockfile, Diagnostic> {
    let project_root = normalize_path(project_root);
    let mut package = expected_lockfile(manifest).package;
    let mut visited = BTreeSet::from([project_root.clone()]);
    collect_workspace_packages(
        &project_root,
        &project_root,
        manifest,
        &mut visited,
        &mut package,
    )?;
    collect_dependency_packages(
        &project_root,
        &project_root,
        manifest,
        &mut visited,
        &mut package,
    )?;
    if package.is_empty() {
        package.sort_by(|left, right| {
            left.source
                .cmp(&right.source)
                .then(left.name.cmp(&right.name))
        });
    } else if let Some((root, dependencies)) = package.split_first_mut() {
        dependencies.sort_by(|left, right| {
            left.source
                .cmp(&right.source)
                .then(left.name.cmp(&right.name))
        });
        package = std::iter::once(root.clone())
            .chain(dependencies.iter().cloned())
            .collect();
    }
    Ok(Lockfile {
        version: 1,
        package,
    })
}

pub fn render_lockfile(manifest: &Manifest) -> Result<String, Diagnostic> {
    toml::to_string_pretty(&expected_lockfile(manifest))
        .map_err(|err| Diagnostic::new("lockfile", format!("failed to render axiom.lock: {err}")))
}

pub fn render_lockfile_for_project(
    project_root: &Path,
    manifest: &Manifest,
) -> Result<String, Diagnostic> {
    toml::to_string_pretty(&expected_lockfile_for_project(project_root, manifest)?)
        .map_err(|err| Diagnostic::new("lockfile", format!("failed to render axiom.lock: {err}")))
}

pub fn validate_lockfile(project_root: &Path, manifest: &Manifest) -> Result<(), Diagnostic> {
    let expected = expected_lockfile_for_project(project_root, manifest)?;
    validate_lockfile_packages(project_root, &expected.package)
}

pub fn validate_lockfile_packages(
    project_root: &Path,
    packages: &[LockedPackage],
) -> Result<(), Diagnostic> {
    let path = lockfile_path(project_root);
    let content = std::fs::read_to_string(&path).map_err(|err| {
        Diagnostic::new("lockfile", format!("failed to read axiom.lock: {err}"))
            .with_path(path.display().to_string())
    })?;
    let lockfile: Lockfile = toml::from_str(&content).map_err(|err| {
        Diagnostic::new("lockfile", format!("invalid axiom.lock: {err}"))
            .with_path(path.display().to_string())
    })?;
    let expected = Lockfile {
        version: 1,
        package: packages.to_vec(),
    };
    if lockfile != expected {
        let detail = lockfile_mismatch_detail(&lockfile, &expected);
        return Err(
            Diagnostic::new(
                "lockfile",
                format!(
                    "axiom.lock does not match axiom.toml; regenerate it with `axiomc new` or update it manually; {detail}"
                ),
            )
            .with_path(path.display().to_string()),
        );
    }
    Ok(())
}

fn lockfile_mismatch_detail(lockfile: &Lockfile, expected: &Lockfile) -> String {
    if lockfile.version != expected.version {
        return format!(
            "lockfile version is {}, expected {}",
            lockfile.version, expected.version
        );
    }

    let locked_by_name = lockfile
        .package
        .iter()
        .map(|package| (package.name.as_str(), package))
        .collect::<BTreeMap<_, _>>();
    let expected_by_name = expected
        .package
        .iter()
        .map(|package| (package.name.as_str(), package))
        .collect::<BTreeMap<_, _>>();

    for expected_package in &expected.package {
        let Some(locked_package) = locked_by_name.get(expected_package.name.as_str()) else {
            return format!(
                "package {:?} is missing from axiom.lock (expected version {:?} from source {:?})",
                expected_package.name, expected_package.version, expected_package.source
            );
        };
        if locked_package.version != expected_package.version
            || locked_package.source != expected_package.source
        {
            return format!(
                "package {:?} changed (axiom.lock has version {:?} from source {:?}; axiom.toml expects version {:?} from source {:?})",
                expected_package.name,
                locked_package.version,
                locked_package.source,
                expected_package.version,
                expected_package.source
            );
        }
    }

    for locked_package in &lockfile.package {
        if !expected_by_name.contains_key(locked_package.name.as_str()) {
            return format!(
                "package {:?} is extra in axiom.lock (locked version {:?} from source {:?})",
                locked_package.name, locked_package.version, locked_package.source
            );
        }
    }

    "package entries differ in order or duplicate package names".to_string()
}

fn dependency_root(
    root_project_root: &Path,
    project_root: &Path,
    spec: &DependencySpec,
) -> Result<PathBuf, Diagnostic> {
    let dependency_root = normalize_path(project_root.join(&spec.path));
    let canonical_project_root = canonicalize_path(project_root, "dependency source package")?;
    let canonical_dependency_root = canonicalize_path(&dependency_root, "dependency path")?;
    let canonical_root_project_root = canonicalize_path(root_project_root, "package root")?;
    if canonical_dependency_root.starts_with(&canonical_root_project_root)
        || workspace_declares_dependency_member(&canonical_project_root, &canonical_dependency_root)
    {
        return Ok(dependency_root);
    }
    Err(
        Diagnostic::new(
            "manifest",
            "dependency path must stay inside the workspace or package root; declare sibling packages as workspace members before depending on them",
        )
        .with_path(manifest_path(project_root).display().to_string()),
    )
}

fn collect_dependency_packages(
    root_project_root: &Path,
    project_root: &Path,
    manifest: &Manifest,
    visited: &mut BTreeSet<PathBuf>,
    packages: &mut Vec<LockedPackage>,
) -> Result<(), Diagnostic> {
    for spec in manifest.dependencies.values() {
        let dependency_root = dependency_root(root_project_root, project_root, spec)?;
        if !visited.insert(dependency_root.clone()) {
            continue;
        }
        let dependency_manifest = load_manifest(&dependency_root)?;
        let dependency_package = dependency_manifest.package.as_ref().ok_or_else(|| {
            Diagnostic::new(
                "manifest",
                format!(
                    "dependency at {} must define a [package] section",
                    dependency_root.display()
                ),
            )
            .with_path(dependency_root.join("axiom.toml").display().to_string())
        })?;
        packages.push(LockedPackage {
            name: dependency_package.name.clone(),
            version: dependency_package.version.clone(),
            source: format!(
                "path:{}",
                normalize_dependency_source(
                    &relative_path(root_project_root, &dependency_root)
                        .display()
                        .to_string(),
                )
            ),
        });
        collect_dependency_packages(
            root_project_root,
            &dependency_root,
            &dependency_manifest,
            visited,
            packages,
        )?;
    }
    Ok(())
}

fn canonicalize_path(path: &Path, label: &str) -> Result<PathBuf, Diagnostic> {
    fs::canonicalize(path).map_err(|err| {
        Diagnostic::new(
            "manifest",
            format!("{label} {} is not accessible: {err}", path.display()),
        )
        .with_path(path.display().to_string())
    })
}

fn workspace_declares_dependency_member(project_root: &Path, dependency_root: &Path) -> bool {
    for ancestor in project_root.ancestors().skip(1) {
        let manifest_file = manifest_path(ancestor);
        if !manifest_file.exists() {
            continue;
        }
        let Ok(manifest) = load_manifest(ancestor) else {
            continue;
        };
        let Some(workspace) = manifest.workspace.as_ref() else {
            continue;
        };
        let mut members = BTreeSet::new();
        for member in &workspace.members {
            let member_root = ancestor.join(member);
            if let Ok(member_root) = fs::canonicalize(&member_root) {
                members.insert(member_root);
            }
        }
        if members.contains(project_root) && members.contains(dependency_root) {
            return true;
        }
    }
    false
}

fn collect_workspace_packages(
    root_project_root: &Path,
    project_root: &Path,
    manifest: &Manifest,
    visited: &mut BTreeSet<PathBuf>,
    packages: &mut Vec<LockedPackage>,
) -> Result<(), Diagnostic> {
    for member in manifest
        .workspace
        .as_ref()
        .into_iter()
        .flat_map(|workspace| workspace.members.iter())
    {
        let member_root = normalize_path(project_root.join(member));
        if !visited.insert(member_root.clone()) {
            continue;
        }
        let member_manifest = load_manifest(&member_root)?;
        if let Some(member_package) = member_manifest.package.as_ref() {
            packages.push(LockedPackage {
                name: member_package.name.clone(),
                version: member_package.version.clone(),
                source: format!(
                    "path:{}",
                    normalize_dependency_source(
                        &relative_path(root_project_root, &member_root)
                            .display()
                            .to_string(),
                    )
                ),
            });
        }
        collect_workspace_packages(
            root_project_root,
            &member_root,
            &member_manifest,
            visited,
            packages,
        )?;
        collect_dependency_packages(
            root_project_root,
            &member_root,
            &member_manifest,
            visited,
            packages,
        )?;
    }
    Ok(())
}

fn relative_path(from: &Path, to: &Path) -> PathBuf {
    let from_components = from.components().collect::<Vec<_>>();
    let to_components = to.components().collect::<Vec<_>>();
    let mut shared = 0usize;
    while shared < from_components.len()
        && shared < to_components.len()
        && from_components[shared] == to_components[shared]
    {
        shared += 1;
    }

    let mut relative = PathBuf::new();
    for _ in shared..from_components.len() {
        relative.push("..");
    }
    for component in &to_components[shared..] {
        relative.push(component.as_os_str());
    }
    relative
}

fn normalize_dependency_source(path: &str) -> String {
    let mut normalized = PathBuf::new();
    let mut saw_component = false;
    for component in Path::new(path).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push("..");
                }
                saw_component = true;
            }
            Component::Normal(value) => {
                normalized.push(value);
                saw_component = true;
            }
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    if !saw_component {
        return String::from(".");
    }
    normalized.to_string_lossy().replace('\\', "/")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write_lockfile(project_root: &Path, package: Vec<LockedPackage>) {
        let lockfile = Lockfile {
            version: 1,
            package,
        };
        std::fs::write(
            lockfile_path(project_root),
            toml::to_string_pretty(&lockfile).expect("render lockfile fixture"),
        )
        .expect("write lockfile fixture");
    }

    #[test]
    fn lockfile_rejects_unknown_top_level_field() {
        let toml = "version = 1\nextra = \"tamper\"\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\nsource = \"path\"\n";
        let error = toml::from_str::<Lockfile>(toml)
            .expect_err("unknown lockfile field should be rejected");
        assert!(
            error.to_string().contains("unknown field"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn locked_package_rejects_unknown_field() {
        let toml = "version = 1\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\nsource = \"path\"\nchecksum = \"deadbeef\"\n";
        let error =
            toml::from_str::<Lockfile>(toml).expect_err("unknown package field should be rejected");
        assert!(
            error.to_string().contains("unknown field"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn validate_lockfile_reports_changed_package_detail() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_lockfile(
            dir.path(),
            vec![LockedPackage {
                name: "demo".to_string(),
                version: "0.1.0".to_string(),
                source: "path".to_string(),
            }],
        );

        let error = validate_lockfile_packages(
            dir.path(),
            &[LockedPackage {
                name: "demo".to_string(),
                version: "0.2.0".to_string(),
                source: "path:deps/demo".to_string(),
            }],
        )
        .expect_err("changed package should fail");

        assert_eq!(error.kind, "lockfile");
        assert!(error.message.contains("package \"demo\" changed"));
        assert!(
            error
                .message
                .contains("version \"0.1.0\" from source \"path\"")
        );
        assert!(
            error
                .message
                .contains("version \"0.2.0\" from source \"path:deps/demo\"")
        );
    }

    #[test]
    fn validate_lockfile_reports_missing_package_detail() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_lockfile(dir.path(), Vec::new());

        let error = validate_lockfile_packages(
            dir.path(),
            &[LockedPackage {
                name: "core".to_string(),
                version: "1.0.0".to_string(),
                source: "path:deps/core".to_string(),
            }],
        )
        .expect_err("missing package should fail");

        assert_eq!(error.kind, "lockfile");
        assert!(error.message.contains("package \"core\" is missing"));
        assert!(
            error
                .message
                .contains("expected version \"1.0.0\" from source \"path:deps/core\"")
        );
    }

    #[test]
    fn validate_lockfile_reports_extra_package_detail() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_lockfile(
            dir.path(),
            vec![LockedPackage {
                name: "old".to_string(),
                version: "0.9.0".to_string(),
                source: "path:deps/old".to_string(),
            }],
        );

        let error =
            validate_lockfile_packages(dir.path(), &[]).expect_err("extra package should fail");

        assert_eq!(error.kind, "lockfile");
        assert!(error.message.contains("package \"old\" is extra"));
        assert!(
            error
                .message
                .contains("locked version \"0.9.0\" from source \"path:deps/old\"")
        );
    }
}
