use crate::diagnostics::Diagnostic;
use crate::lockfile::render_lockfile;
use crate::manifest::{
    BuildSection, CapabilityConfig, LOCK_FILENAME, MANIFEST_FILENAME, Manifest, PackageSection,
    render_manifest,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadTemplate {
    Cli,
    Worker,
    Service,
}

impl WorkloadTemplate {
    pub fn parse(value: &str) -> Result<Self, Diagnostic> {
        match value {
            "cli" => Ok(Self::Cli),
            "worker" => Ok(Self::Worker),
            "service" => Ok(Self::Service),
            _ => Err(Diagnostic::new(
                "new",
                format!("unknown project template {value:?}; expected cli, worker, or service"),
            )),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Worker => "worker",
            Self::Service => "service",
        }
    }
}

pub fn create_project(path: &Path, name: Option<&str>) -> Result<(), Diagnostic> {
    create_project_with_template(path, name, WorkloadTemplate::Cli)
}

pub fn create_project_with_template(
    path: &Path,
    name: Option<&str>,
    template: WorkloadTemplate,
) -> Result<(), Diagnostic> {
    if path.exists() {
        let mut entries = fs::read_dir(path).map_err(|err| {
            Diagnostic::new("new", format!("failed to read {}: {err}", path.display()))
        })?;
        if entries.next().is_some() {
            return Err(Diagnostic::new("new", "project directory must be empty")
                .with_path(path.display().to_string()));
        }
    } else {
        fs::create_dir_all(path).map_err(|err| {
            Diagnostic::new("new", format!("failed to create {}: {err}", path.display()))
        })?;
    }

    let project_name = sanitize_name(name.unwrap_or_else(|| {
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("axiom-app")
    }));
    let src_dir = path.join("src");
    fs::create_dir_all(&src_dir).map_err(|err| {
        Diagnostic::new(
            "new",
            format!("failed to create {}: {err}", src_dir.display()),
        )
    })?;
    let manifest_text = render_manifest(&project_name);
    fs::write(path.join(MANIFEST_FILENAME), manifest_text).map_err(|err| {
        Diagnostic::new("new", format!("failed to write {MANIFEST_FILENAME}: {err}"))
            .with_path(path.join(MANIFEST_FILENAME).display().to_string())
    })?;
    let manifest = Manifest {
        package: Some(PackageSection {
            name: project_name.clone(),
            version: String::from("0.1.0"),
        }),
        dependencies: BTreeMap::new(),
        workspace: None,
        build: BuildSection {
            entry: String::from("src/main.ax"),
            out_dir: String::from("dist"),
        },
        tests: Vec::new(),
        capabilities: CapabilityConfig::default(),
    };
    let lock_text = render_lockfile(&manifest)?;
    fs::write(path.join(LOCK_FILENAME), lock_text).map_err(|err| {
        Diagnostic::new("new", format!("failed to write {LOCK_FILENAME}: {err}"))
            .with_path(path.join(LOCK_FILENAME).display().to_string())
    })?;
    let starter = starter_source(template);
    fs::write(src_dir.join("main.ax"), starter.source).map_err(|err| {
        Diagnostic::new("new", format!("failed to write src/main.ax: {err}"))
            .with_path(src_dir.join("main.ax").display().to_string())
    })?;
    fs::write(src_dir.join("main_test.ax"), starter.test_source).map_err(|err| {
        Diagnostic::new("new", format!("failed to write src/main_test.ax: {err}"))
            .with_path(src_dir.join("main_test.ax").display().to_string())
    })?;
    fs::write(src_dir.join("main_test.stdout"), starter.stdout).map_err(|err| {
        Diagnostic::new(
            "new",
            format!("failed to write src/main_test.stdout: {err}"),
        )
        .with_path(src_dir.join("main_test.stdout").display().to_string())
    })?;
    Ok(())
}

struct StarterTemplate {
    source: &'static str,
    test_source: &'static str,
    stdout: &'static str,
}

fn starter_source(template: WorkloadTemplate) -> StarterTemplate {
    match template {
        WorkloadTemplate::Cli => StarterTemplate {
            source: "print \"hello from stage1\"\n",
            test_source: "print \"hello from stage1\"\n",
            stdout: "hello from stage1\n",
        },
        WorkloadTemplate::Worker => StarterTemplate {
            source: "fn handle(value: int): int {\nreturn value + 1\n}\n\nprint handle(41)\n",
            test_source: "fn handle(value: int): int {\nreturn value + 1\n}\n\nprint handle(41)\n",
            stdout: "42\n",
        },
        WorkloadTemplate::Service => StarterTemplate {
            source: "fn route(path: string): string {\nif path == \"/health\" {\nreturn \"ok\"\n}\nreturn \"not-found\"\n}\n\nprint route(\"/health\")\n",
            test_source: "fn route(path: string): string {\nif path == \"/health\" {\nreturn \"ok\"\n}\nreturn \"not-found\"\n}\n\nprint route(\"/health\")\n",
            stdout: "ok\n",
        },
    }
}

fn sanitize_name(input: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in input.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            last_dash = false;
            ch.to_ascii_lowercase()
        } else {
            if last_dash {
                continue;
            }
            last_dash = true;
            '-'
        };
        out.push(next);
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        String::from("axiom-app")
    } else {
        trimmed.to_string()
    }
}
