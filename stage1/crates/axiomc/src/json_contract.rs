use crate::diagnostics::Diagnostic;
use crate::manifest::CapabilityDescriptor;
use crate::project::{BuildOutput, CheckOutput, TestOutput};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;

pub const JSON_SCHEMA_VERSION: &str = "axiom.stage1.v1";

pub fn check_success(project: &Path, output: &CheckOutput) -> Value {
    json!({
        "schema_version": JSON_SCHEMA_VERSION,
        "ok": true,
        "command": "check",
        "project": project.display().to_string(),
        "manifest": output.manifest,
        "entry": output.entry,
        "statement_count": output.statement_count,
        "capabilities": output.capabilities,
        "warnings": output.warnings,
        "packages": output.packages,
    })
}

pub fn build_success(project: &Path, output: &BuildOutput) -> Value {
    json!({
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
        "statement_count": output.statement_count,
        "target": output.target,
        "debug": output.debug,
        "cache_hits": output.cache_hits,
        "cache_misses": output.cache_misses,
        "duration_ms": output.duration_ms,
        "packages": output.packages,
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

pub fn error(command: &str, error: &Diagnostic) -> Value {
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
