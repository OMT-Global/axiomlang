//! Canonical, Axiom-neutral Intent IR emission.
//!
//! IDs describe Axiom packages and declarations. They deliberately do not
//! expose implementation backend, build-tool, or host-language terminology.

use crate::diagnostics::Diagnostic;
use crate::manifest::{CapabilityKind, Manifest, load_manifest};
use crate::syntax::{Program, parse_program};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const SCHEMA_VERSION: &str = "axiom.intent_ir.v0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IntentIrDocument {
    pub schema_version: String,
    #[serde(default)]
    pub graph_id: String,
    #[serde(default)]
    pub package: String,
    #[serde(default)]
    pub provenance: IntentIrProvenance,
    #[serde(default)]
    pub diagnostics: Vec<IntentIrDiagnostic>,
    pub nodes: Vec<IntentIrNode>,
    pub edges: Vec<IntentIrEdge>,
}

impl IntentIrDocument {
    pub fn contains_node(&self, id: &str) -> bool {
        self.nodes.binary_search_by(|node| node.id.as_str().cmp(id)).is_ok()
    }

    pub fn node_id(&self, kind: &str, name: &str) -> Option<&str> {
        self.nodes
            .iter()
            .find(|node| node.kind == kind && node.name == name)
            .map(|node| node.id.as_str())
    }

    pub fn node_id_for_source(&self, kind: &str, path: &str) -> Option<&str> {
        let path = normalize_text_path(path);
        self.nodes
            .iter()
            .find(|node| {
                node.kind == kind
                    && node.source_span.as_ref().is_some_and(|span| {
                        span.path == path || path.ends_with(&format!("/{}", span.path))
                    })
            })
            .map(|node| node.id.as_str())
    }

    pub fn semantic_target_for_source(&self, path: &str) -> &str {
        self.node_id_for_source("Module", path)
            .unwrap_or(&self.package)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntentIrProvenance {
    pub producer: String,
    pub source_digest: String,
    pub path_policy: String,
    pub inputs: Vec<IntentIrInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct IntentIrInput {
    pub package: String,
    pub path: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct IntentIrDiagnostic {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub node_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IntentIrNode {
    pub id: String,
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_span: Option<SourceSpan>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub metadata: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceSpan {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IntentIrEdge {
    pub from: String,
    pub kind: String,
    pub to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub metadata: Map<String, Value>,
}

#[derive(Default)]
struct Graph {
    nodes: BTreeMap<String, IntentIrNode>,
    edges: BTreeMap<(String, String, String), IntentIrEdge>,
    diagnostics: Vec<IntentIrDiagnostic>,
    inputs: Vec<IntentIrInput>,
}

impl Graph {
    fn node(&mut self, node: IntentIrNode) {
        self.nodes.insert(node.id.clone(), node);
    }

    fn edge(&mut self, from: &str, kind: &str, to: &str) {
        let key = (from.to_owned(), kind.to_owned(), to.to_owned());
        self.edges.entry(key).or_insert_with(|| IntentIrEdge {
            from: from.to_owned(),
            kind: kind.to_owned(),
            to: to.to_owned(),
            description: None,
            metadata: Map::new(),
        });
    }

    fn diagnostic(&mut self, code: &str, message: impl Into<String>, semantic_node: &str) {
        self.diagnostics.push(IntentIrDiagnostic {
            code: code.to_owned(),
            severity: "warning".to_owned(),
            message: message.into(),
            node_ids: vec![semantic_node.to_owned()],
        });
    }

    fn input(&mut self, package: &str, path: String, bytes: &[u8]) {
        self.inputs.push(IntentIrInput {
            package: package.to_owned(),
            path,
            digest: stable_digest(bytes),
        });
    }
}

/// Emits one deterministic graph for the package or workspace at `root`.
pub fn emit_intent_ir(root: &Path) -> Result<IntentIrDocument, Diagnostic> {
    let root = root
        .canonicalize()
        .map_err(|error| io_error("intent_ir_root", root, error))?;
    let root_manifest = load_manifest(&root)?;
    let root_name = package_name(&root_manifest, &root);
    let root_id = package_id(&root_name);
    let mut graph = Graph::default();
    let mut packages = vec![root.clone()];
    if let Some(workspace) = &root_manifest.workspace {
        for member in &workspace.members {
            packages.push(root.join(member));
        }
    }
    packages.sort_by_key(|path| normalized_relative(&root, path));
    packages.dedup();
    for package_root in packages {
        match load_manifest(&package_root) {
            Ok(manifest) => emit_package(&root, &package_root, &manifest, &mut graph)?,
            Err(error) => graph.diagnostic(
                "intent_ir_incomplete_package",
                format!("workspace package could not be loaded: {error}"),
                &root_id,
            ),
        }
    }
    graph.diagnostics.sort();
    graph.diagnostics.dedup();
    graph.inputs.sort();
    graph.inputs.dedup();
    let source_digest = stable_digest(
        graph
            .inputs
            .iter()
            .flat_map(|input| input.digest.bytes())
            .collect::<Vec<_>>()
            .as_slice(),
    );
    Ok(IntentIrDocument {
        schema_version: SCHEMA_VERSION.to_owned(),
        graph_id: format!("axiom://graph/{}", segment(&root_name)),
        package: root_id,
        provenance: IntentIrProvenance {
            producer: "axiomc".to_owned(),
            source_digest,
            path_policy: "package_relative".to_owned(),
            inputs: graph.inputs,
        },
        diagnostics: graph.diagnostics,
        nodes: graph.nodes.into_values().collect(),
        edges: graph.edges.into_values().collect(),
    })
}

fn emit_package(
    workspace_root: &Path,
    package_root: &Path,
    manifest: &Manifest,
    graph: &mut Graph,
) -> Result<(), Diagnostic> {
    let name = package_name(manifest, package_root);
    let package = package_id(&name);
    let mut metadata = Map::new();
    metadata.insert(
        "path".into(),
        json!(normalized_relative(workspace_root, package_root)),
    );
    if let Some(section) = &manifest.package {
        metadata.insert("version".into(), json!(section.version));
    }
    graph.node(node(&package, "Package", &name, None, metadata));
    let manifest_path = package_root.join(crate::manifest::MANIFEST_FILENAME);
    if let Ok(bytes) = fs::read(&manifest_path) {
        graph.input(
            &package,
            normalized_relative(workspace_root, &manifest_path),
            &bytes,
        );
    }

    for (alias, dependency) in &manifest.dependencies {
        let id = child_id(&package, "dependency", alias);
        graph.node(node(
            &id,
            "Dependency",
            alias,
            None,
            object([
                ("path", json!(normalize_text_path(&dependency.path))),
                ("version", json!(dependency.version)),
            ]),
        ));
        graph.edge(&package, "depends_on", &id);
    }

    for kind in crate::manifest::KNOWN_CAPABILITIES {
        if manifest.capabilities.enabled(kind) {
            emit_manifest_capability(manifest, kind, &package, graph);
        }
    }

    let files = axiom_files(package_root)?;
    if files.is_empty() {
        graph.diagnostic(
            "intent_ir_no_sources",
            "package contains no Axiom source files",
            &package,
        );
    }
    for file in files {
        let relative = normalized_relative(workspace_root, &file);
        let module_name = module_name(package_root, &file);
        let module = child_id(&package, "module", &module_name);
        graph.node(node(
            &module,
            "Module",
            &module_name,
            Some(SourceSpan {
                path: relative.clone(),
                line: None,
                column: None,
            }),
            Map::new(),
        ));
        graph.edge(&package, "declares", &module);
        let source =
            fs::read_to_string(&file).map_err(|error| io_error("intent_ir_read", &file, error))?;
        graph.input(&package, relative.clone(), source.as_bytes());
        match parse_program(&source, &file) {
            Ok(program) => emit_program(&package, &module, &relative, &program, graph),
            Err(error) => graph.diagnostic(
                "intent_ir_incomplete_module",
                format!("module could not be parsed: {error}"),
                &module,
            ),
        }
    }

    emit_artifacts(package_root, workspace_root, manifest, &package, graph)?;
    emit_decisions(package_root, workspace_root, &package, graph)?;
    Ok(())
}

fn emit_program(package: &str, module: &str, path: &str, program: &Program, graph: &mut Graph) {
    for import in &program.imports {
        let target = child_id(package, "module", &import.path.replace("::", "/"));
        if !graph.nodes.contains_key(&target) {
            graph.node(node(
                &target,
                "Module",
                &import.path,
                None,
                object([("resolution", json!("referenced"))]),
            ));
            graph.edge(package, "declares", &target);
        }
        graph.edge(module, "uses", &target);
    }
    for declaration in &program.type_aliases {
        emit_decl(
            graph,
            package,
            module,
            path,
            "Type",
            "type",
            &declaration.name,
            declaration.line,
            declaration.column,
            object([("visibility", json!(declaration.visibility))]),
        );
    }
    for declaration in &program.structs {
        emit_decl(
            graph,
            package,
            module,
            path,
            "Type",
            "type",
            &declaration.name,
            declaration.line,
            declaration.column,
            object([
                ("shape", json!("struct")),
                ("fields", json!(declaration.fields)),
                ("visibility", json!(declaration.visibility)),
            ]),
        );
    }
    for declaration in &program.enums {
        emit_decl(
            graph,
            package,
            module,
            path,
            "Type",
            "type",
            &declaration.name,
            declaration.line,
            declaration.column,
            object([
                ("shape", json!("enum")),
                ("variants", json!(declaration.variants)),
                ("visibility", json!(declaration.visibility)),
            ]),
        );
    }
    for declaration in &program.traits {
        emit_decl(
            graph,
            package,
            module,
            path,
            "Type",
            "type",
            &declaration.name,
            declaration.line,
            declaration.column,
            object([
                ("shape", json!("trait")),
                ("methods", json!(declaration.methods)),
                ("visibility", json!(declaration.visibility)),
            ]),
        );
    }
    for function in &program.functions {
        emit_decl(
            graph,
            package,
            module,
            path,
            "Function",
            "function",
            &function.name,
            function.line,
            function.column,
            object([
                ("parameters", json!(function.params)),
                ("return_type", json!(function.return_ty)),
                ("async", json!(function.is_async)),
                ("property", json!(function.is_property)),
                ("visibility", json!(function.visibility)),
            ]),
        );
    }
    for axiom in &program.axioms {
        let mut metadata = object([
            ("scope", json!(axiom.scope)),
            ("severity", json!(axiom.severity)),
            ("assertion", json!(axiom.assertion)),
        ]);
        if let Some(description) = &axiom.description {
            metadata.insert("description".into(), json!(description));
        }
        emit_decl(
            graph,
            package,
            module,
            path,
            "Axiom",
            "axiom",
            &axiom.name,
            axiom.line,
            axiom.column,
            metadata,
        );
    }
    for evidence in &program.evidence {
        emit_decl(
            graph,
            package,
            module,
            path,
            "Evidence",
            "evidence",
            &evidence.name,
            evidence.line,
            evidence.column,
            object([("description", json!(evidence.description))]),
        );
    }
    for capability in &program.semantic_capabilities {
        let id = emit_decl(
            graph,
            package,
            module,
            path,
            "Capability",
            "capability",
            &capability.name,
            capability.line,
            capability.column,
            object([("inputs", json!(capability.inputs))]),
        );
        for effect in &capability.effects {
            let effect_name = format!("{}:{}", effect.kind, effect.target);
            let effect_id = child_id(&id, "effect", &effect_name);
            graph.node(node(
                &effect_id,
                "Effect",
                &effect_name,
                Some(span(path, effect.line, effect.column)),
                object([
                    ("effect_kind", json!(effect.kind)),
                    ("target", json!(effect.target)),
                ]),
            ));
            graph.edge(&id, "allows_effect", &effect_id);
        }
        for reference in &capability.preserves {
            let referenced = ensure_reference(graph, module, "Axiom", "axiom", &reference.name);
            graph.edge(&id, "preserves", &referenced);
        }
        for reference in &capability.evidence {
            let referenced =
                ensure_reference(graph, module, "Evidence", "evidence", &reference.name);
            graph.edge(&id, "verified_by", &referenced);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_decl(
    graph: &mut Graph,
    _package: &str,
    module: &str,
    path: &str,
    kind: &str,
    family: &str,
    name: &str,
    line: usize,
    column: usize,
    metadata: Map<String, Value>,
) -> String {
    let id = child_id(module, family, name);
    graph.node(node(
        &id,
        kind,
        name,
        Some(span(path, line, column)),
        metadata,
    ));
    graph.edge(module, "declares", &id);
    id
}

fn ensure_reference(
    graph: &mut Graph,
    module: &str,
    kind: &str,
    family: &str,
    name: &str,
) -> String {
    let id = child_id(module, family, name);
    if !graph.nodes.contains_key(&id) {
        graph.node(node(
            &id,
            kind,
            name,
            None,
            object([("resolution", json!("referenced"))]),
        ));
        graph.edge(module, "declares", &id);
    }
    id
}

fn emit_manifest_capability(
    manifest: &Manifest,
    kind: CapabilityKind,
    package: &str,
    graph: &mut Graph,
) {
    let name = kind.name();
    let capability = child_id(package, "capability", name);
    graph.node(node(&capability, "Capability", name, None, Map::new()));
    graph.edge(package, "requires", &capability);
    let targets: Vec<String> = match kind {
        CapabilityKind::Fs | CapabilityKind::FsWrite => {
            manifest.capabilities.fs_root.iter().cloned().collect()
        }
        CapabilityKind::Net => manifest.capabilities.net_hosts.clone(),
        CapabilityKind::Process => Vec::new(),
        CapabilityKind::Env => manifest.capabilities.env_vars.clone(),
        _ => Vec::new(),
    };
    let targets = if targets.is_empty() {
        vec!["unrestricted".to_owned()]
    } else {
        targets
    };
    for target in targets {
        let surface_name = format!("{name}:{target}");
        let surface = child_id(package, "runtime-surface", &surface_name);
        graph.node(node(
            &surface,
            "RuntimeSurface",
            &surface_name,
            None,
            object([("capability", json!(name)), ("target", json!(target))]),
        ));
        graph.edge(&capability, "uses", &surface);
    }
}

fn emit_artifacts(
    package_root: &Path,
    workspace_root: &Path,
    manifest: &Manifest,
    package: &str,
    graph: &mut Graph,
) -> Result<(), Diagnostic> {
    let out = package_root.join(&manifest.build.out_dir);
    let planned_path = normalize_text_path(&format!(
        "{}/{}",
        manifest.build.out_dir,
        package.rsplit('/').next().unwrap_or("package")
    ));
    let planned = child_id(package, "artifact", &planned_path);
    graph.node(node(
        &planned,
        "Artifact",
        &planned_path,
        None,
        object([
            ("state", json!("planned")),
            ("provenance_node", json!(package)),
        ]),
    ));
    graph.edge(&planned, "generated_from", package);
    if out.is_dir() {
        for path in regular_files(&out)? {
            let relative = normalized_relative(workspace_root, &path);
            let bytes =
                fs::read(&path).map_err(|error| io_error("intent_ir_read", &path, error))?;
            graph.input(package, relative.clone(), &bytes);
            let artifact = child_id(package, "artifact", &relative);
            graph.node(node(
                &artifact,
                "Artifact",
                &relative,
                Some(SourceSpan {
                    path: relative.clone(),
                    line: None,
                    column: None,
                }),
                object([
                    ("state", json!("materialized")),
                    ("provenance_node", json!(package)),
                ]),
            ));
            graph.edge(&artifact, "generated_from", package);
        }
    }
    Ok(())
}

fn emit_decisions(
    package_root: &Path,
    workspace_root: &Path,
    package: &str,
    graph: &mut Graph,
) -> Result<(), Diagnostic> {
    let mut paths = Vec::new();
    for directory in [package_root.join("decisions"), package_root.join(".axiom/decisions")] {
        if directory.is_dir() {
            paths.extend(regular_files(&directory)?);
        }
    }
    paths.sort();
    paths.dedup();
    for path in paths {
        let relative = normalized_relative(workspace_root, &path);
        let bytes = fs::read(&path).map_err(|error| io_error("intent_ir_read", &path, error))?;
        graph.input(package, relative.clone(), &bytes);
        let value: Value = match serde_json::from_slice(&bytes).ok() {
            Some(value) => value,
            None => {
                graph.diagnostic(
                    "intent_ir_incomplete_decision",
                    format!("decision record is not valid JSON: {relative}"),
                    package,
                );
                continue;
            }
        };
        let name = value
            .get("id")
            .or_else(|| value.get("name"))
            .and_then(Value::as_str)
            .unwrap_or(&relative)
            .to_owned();
        let id = child_id(package, "decision", &name);
        graph.node(node(
            &id,
            "Decision",
            &name,
            Some(SourceSpan {
                path: relative,
                line: None,
                column: None,
            }),
            object([("record", value)]),
        ));
        graph.edge(package, "declares", &id);
    }
    Ok(())
}

fn node(
    id: &str,
    kind: &str,
    name: &str,
    source_span: Option<SourceSpan>,
    metadata: Map<String, Value>,
) -> IntentIrNode {
    IntentIrNode {
        id: id.to_owned(),
        kind: kind.to_owned(),
        name: name.to_owned(),
        description: None,
        source_span,
        metadata,
    }
}

fn span(path: &str, line: usize, column: usize) -> SourceSpan {
    SourceSpan {
        path: path.to_owned(),
        line: Some(line.max(1)),
        column: Some(column.max(1)),
    }
}
fn package_id(name: &str) -> String {
    format!("axiom://package/{}", segment(name))
}
fn child_id(package: &str, family: &str, name: &str) -> String {
    format!("{package}/{family}/{}", segment(name))
}

fn segment(value: &str) -> String {
    let mut result = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"._~-".contains(&byte) {
            result.push(byte as char);
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    if result.is_empty() {
        "unnamed".to_owned()
    } else {
        result
    }
}

fn package_name(manifest: &Manifest, root: &Path) -> String {
    manifest
        .package
        .as_ref()
        .map(|package| package.name.clone())
        .or_else(|| {
            root.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "package".into())
}

fn module_name(root: &Path, file: &Path) -> String {
    let relative = file.strip_prefix(root).unwrap_or(file);
    let without_extension = relative.with_extension("");
    normalize_path(&without_extension)
}

fn normalize_text_path(value: &str) -> String {
    value
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect::<Vec<_>>()
        .join("/")
}
fn normalized_relative(root: &Path, path: &Path) -> String {
    normalize_path(path.strip_prefix(root).unwrap_or(path))
}
fn normalize_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            Component::ParentDir => Some("..".into()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn axiom_files(root: &Path) -> Result<Vec<PathBuf>, Diagnostic> {
    let mut pending = vec![root.to_owned()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory)
            .map_err(|error| io_error("intent_ir_read_dir", &directory, error))?;
        for entry in entries {
            let entry = entry
                .map_err(|error| io_error("intent_ir_read_dir", &directory, error))?;
            let path = entry.path();
            if path.is_dir() {
                let skipped = matches!(
                    path.file_name().and_then(|value| value.to_str()),
                    Some(".git" | "target")
                );
                let nested_package = path != root
                    && path.join(crate::manifest::MANIFEST_FILENAME).is_file();
                if !skipped && !nested_package {
                    pending.push(path);
                }
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("ax") {
                files.push(path);
            }
        }
    }
    files.sort_by_key(|path| normalize_path(path));
    Ok(files)
}

fn regular_files(root: &Path) -> Result<Vec<PathBuf>, Diagnostic> {
    let mut pending = vec![root.to_owned()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory)
            .map_err(|error| io_error("intent_ir_read_dir", &directory, error))?;
        for entry in entries {
            let entry = entry.map_err(|error| io_error("intent_ir_read_dir", &directory, error))?;
            let path = entry.path();
            if path.is_dir() {
                if !matches!(
                    path.file_name().and_then(|value| value.to_str()),
                    Some(".git" | "target")
                ) {
                    pending.push(path);
                }
            } else if path.is_file() {
                files.push(path);
            }
        }
    }
    files.sort_by_key(|path| normalize_path(path));
    Ok(files)
}

fn object<const N: usize>(entries: [(&str, Value); N]) -> Map<String, Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

// A small deterministic content digest for graph identity. This is provenance,
// not an authenticity primitive; release attestations provide cryptographic
// integrity. Four independently seeded FNV-1a lanes avoid platform hash state.
fn stable_digest(bytes: &[u8]) -> String {
    const SEEDS: [u64; 4] = [
        0xcbf29ce484222325,
        0x84222325cbf29ce4,
        0x9e3779b185ebca87,
        0x517cc1b727220a95,
    ];
    SEEDS
        .iter()
        .map(|seed| {
            let mut hash = *seed;
            for byte in bytes {
                hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
            }
            format!("{hash:016x}")
        })
        .collect()
}

fn io_error(code: &'static str, path: &Path, error: std::io::Error) -> Diagnostic {
    Diagnostic::new("intent_ir", format!("{}: {error}", path.display()))
        .with_code(code)
        .with_path(path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_are_stable_and_schema_safe() {
        assert_eq!(segment("api/main value"), "api%2Fmain%20value");
        assert_eq!(
            child_id("axiom://package/demo", "function", "main"),
            "axiom://package/demo/function/main"
        );
    }

    #[test]
    fn path_normalization_is_posix() {
        assert_eq!(
            normalize_text_path(r"src\\nested\\main.ax"),
            "src/nested/main.ax"
        );
    }
}
