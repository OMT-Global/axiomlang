use crate::diagnostics::Diagnostic;
use crate::manifest::CapabilityDescriptor;
use crate::project::{
    BuildOutput, CapabilitySbomOutput, CheckOutput, RunOutput, TestListOutput, TestOutput,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const JSON_SCHEMA_VERSION: &str = "axiom.stage1.v1";

pub fn check_success(project: &Path, output: &CheckOutput) -> Value {
    let mut payload = json!({
        "schema_version": JSON_SCHEMA_VERSION,
        "ok": true,
        "command": "check",
        "project": project.display().to_string(),
        "manifest": output.manifest,
        "entry": output.entry,
        "statement_count": output.statement_count,
        "capabilities": output.capabilities,
        "exports": output.exports,
        "warnings": output.warnings,
        "packages": output.packages,
    });
    if let Some(debug_symbols) = &output.debug_symbols {
        payload["debug_symbols"] = json!(debug_symbols);
    }
    payload
}

pub fn build_success(project: &Path, output: &BuildOutput) -> Value {
    let payload = json!({
        "schema_version": JSON_SCHEMA_VERSION,
        "ok": true,
        "command": "build",
        "project": project.display().to_string(),
        "backend": output.backend,
        "locked": output.locked,
        "offline": output.offline,
        "manifest": output.manifest,
        "entry": output.entry,
        "binary": output.binary,
        "generated_rust": output.generated_rust,
        "debug_map": output.debug_map,
        "debug_manifest": output.debug_manifest,
        "statement_count": output.statement_count,
        "target": output.target,
        "debug": output.debug,
        "cache_key": output.cache_key,
        "metadata": output.metadata,
        "cache_hits": output.cache_hits,
        "cache_misses": output.cache_misses,
        "duration_ms": output.duration_ms,
        "packages": output.packages,
    });
    payload
}

pub fn run_success(project: &Path, output: &RunOutput) -> Value {
    json!({
        "schema_version": JSON_SCHEMA_VERSION,
        "ok": output.exit_code == 0,
        "command": "run",
        "project": project.display().to_string(),
        "manifest": output.manifest,
        "entry": output.entry,
        "binary": output.binary,
        "generated_rust": output.generated_rust,
        "package": output.package,
        "args": output.args,
        "exit_code": output.exit_code,
        "result": output.result,
        "stdout": output.stdout,
        "stderr": output.stderr,
        "duration_ms": output.duration_ms,
    })
}

pub fn test_list_success(project: &Path, filter: Option<&str>, output: &TestListOutput) -> Value {
    json!({
        "schema_version": JSON_SCHEMA_VERSION,
        "ok": true,
        "command": "test",
        "mode": "list",
        "project": project.display().to_string(),
        "manifest": output.manifest,
        "packages": output.packages,
        "filter": filter,
        "tests": output.tests,
    })
}

pub fn test_success(project: &Path, filter: Option<&str>, output: &TestOutput) -> Value {
    json!({
        "schema_version": JSON_SCHEMA_VERSION,
        "ok": output.failed == 0,
        "command": "test",
        "project": project.display().to_string(),
        "manifest": output.manifest,
        "packages": output.packages,
        "filter": filter,
        "passed": output.passed,
        "failed": output.failed,
        "skipped": output.skipped,
        "kinds": output.kinds,
        "duration_ms": output.duration_ms,
        "cases": output.cases,
    })
}

pub fn caps_success(project: &Path, capabilities: &[CapabilityDescriptor]) -> Value {
    json!({
        "schema_version": JSON_SCHEMA_VERSION,
        "ok": true,
        "command": "caps",
        "project": project.display().to_string(),
        "capabilities": capabilities,
    })
}

pub fn caps_manifest_success(
    project: &Path,
    capabilities: &[CapabilityDescriptor],
    sbom: &CapabilitySbomOutput,
) -> Value {
    let by_name: BTreeMap<&str, &CapabilityDescriptor> = capabilities
        .iter()
        .map(|capability| (capability.name.as_str(), capability))
        .collect();
    let mut requested_by_capability: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for package in &sbom.packages {
        for use_site in &package.intrinsic_use {
            requested_by_capability
                .entry(use_site.capability.clone())
                .or_default()
                .insert(format!(
                    "{}:{}:{}:{}",
                    use_site.module, use_site.line, use_site.column, use_site.intrinsic
                ));
        }
    }

    let requested: Vec<Value> = requested_by_capability
        .iter()
        .map(|(name, triggers)| {
            json!({
                "name": name,
                "granted": by_name.get(name.as_str()).is_some_and(|capability| capability.enabled),
                "triggers": triggers.iter().cloned().collect::<Vec<_>>(),
            })
        })
        .collect();
    let granted: Vec<&CapabilityDescriptor> = capabilities
        .iter()
        .filter(|capability| capability.enabled)
        .collect();
    let denied: Vec<Value> = requested_by_capability
        .keys()
        .filter(|name| {
            !by_name
                .get(name.as_str())
                .is_some_and(|capability| capability.enabled)
        })
        .map(|name| json!({ "name": name }))
        .collect();
    let unsafe_entries: Vec<Value> = sbom
        .packages
        .iter()
        .flat_map(|package| {
            package.unsafe_grants.iter().map(move |grant| {
                json!({
                    "package": package.root,
                    "capability": grant.capability,
                    "kind": grant.kind,
                    "rationale": grant.rationale,
                })
            })
        })
        .collect();
    let transitive_package_usage: Vec<Value> = sbom
        .packages
        .iter()
        .map(|package| {
            json!({
                "root": package.root,
                "manifest": package.manifest,
                "name": package.name,
                "version": package.version,
                "workspace_only": package.workspace_only,
                "entrypoint": package.entrypoint,
                "dependencies": package.package_graph.dependencies,
                "members": package.package_graph.members,
                "capability_scopes": package.capability_scopes,
            })
        })
        .collect();
    let mut stdlib_modules_by_capability: BTreeMap<String, BTreeMap<String, BTreeSet<String>>> =
        BTreeMap::new();
    for package in &sbom.packages {
        for use_site in &package.intrinsic_use {
            stdlib_modules_by_capability
                .entry(use_site.capability.clone())
                .or_default()
                .entry(use_site.module.clone())
                .or_default()
                .insert(format!(
                    "{}:{}:{}",
                    use_site.line, use_site.column, use_site.intrinsic
                ));
        }
    }
    let stdlib_modules: Vec<Value> = stdlib_modules_by_capability
        .into_iter()
        .map(|(capability, modules)| {
            let modules = modules
                .into_iter()
                .map(|(module, triggers)| {
                    json!({
                        "module": module,
                        "triggers": triggers.into_iter().collect::<Vec<_>>(),
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "capability": capability,
                "modules": modules,
            })
        })
        .collect();

    json!({
        "schema_version": JSON_SCHEMA_VERSION,
        "ok": true,
        "command": "caps",
        "project": project.display().to_string(),
        "manifest": sbom.manifest,
        "capabilities": capabilities,
        "requested": requested,
        "granted": granted,
        "denied": denied,
        "unsafe": unsafe_entries,
        "transitive_package_usage": transitive_package_usage,
        "stdlib_modules": stdlib_modules,
    })
}

pub fn error(command: &str, error: &Diagnostic) -> Value {
    let error = error.normalized_for_json();
    json!({
        "schema_version": JSON_SCHEMA_VERSION,
        "ok": false,
        "command": command,
        "error": error,
    })
}

pub fn to_pretty_string<T: Serialize>(payload: &T) -> Result<String, Diagnostic> {
    serde_json::to_string_pretty(payload)
        .map_err(|err| Diagnostic::new("json", format!("failed to serialize JSON output: {err}")))
}
