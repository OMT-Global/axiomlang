use crate::codegen::SUPPORTED_NATIVE_BACKENDS;
use crate::diagnostics::Diagnostic;
use crate::json_contract;
use crate::manifest::{CapabilityDescriptor, KNOWN_CAPABILITIES};
use crate::project::{CheckOptions, check_project_with_options};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    schema_version: &'static str,
    pub ok: bool,
    command: &'static str,
    project: String,
    rustc: ToolProbe,
    cargo: ToolProbe,
    target_triple: Option<String>,
    lockfile_status: &'static str,
    capabilities: Vec<CapabilityDescriptor>,
    workspace_graph: Vec<DoctorPackage>,
    capability_ledger: CapabilityLedgerFacts,
    known_unsupported_features: Vec<&'static str>,
    error: Option<Diagnostic>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CapabilityLedgerFacts {
    schema_version: &'static str,
    snapshot: &'static str,
    commands: usize,
    stdlib_modules: usize,
    stdlib_functions: usize,
    capabilities: usize,
    runtime_abi_rows: usize,
    schemas: usize,
    supported_native_backend: &'static str,
    production_qualified_rows: usize,
}

#[derive(Debug, Deserialize)]
struct CheckedCapabilityLedger {
    summary: CheckedCapabilityLedgerSummary,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheckedCapabilityLedgerSummary {
    commands: usize,
    stdlib_modules: usize,
    stdlib_functions: usize,
    capabilities: usize,
    runtime_abi_rows: usize,
    schemas: usize,
    supported_native_backend: String,
    evidence_tiers: std::collections::BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize)]
struct ToolProbe {
    available: bool,
    version: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct DoctorPackage {
    package_root: String,
    manifest: String,
    entry: String,
    statement_count: usize,
}

pub fn doctor_report(project: &Path, command_count: usize) -> DoctorReport {
    let rustc = probe_tool("rustc", &["-vV"]);
    let cargo = probe_tool("cargo", &["--version"]);
    let target_triple = rustc.version.as_deref().and_then(parse_rustc_host_target);
    let check = check_project_with_options(project, &CheckOptions::default());
    let (ok, lockfile_status, capabilities, workspace_graph, error) = match check {
        Ok(output) => {
            let packages = output
                .packages
                .iter()
                .map(|package| DoctorPackage {
                    package_root: package.package_root.clone(),
                    manifest: package.manifest.clone(),
                    entry: package.entry.clone(),
                    statement_count: package.statement_count,
                })
                .collect();
            (
                rustc.available && cargo.available,
                "valid",
                output.capabilities,
                packages,
                None,
            )
        }
        Err(error) => {
            let lockfile_status =
                if error.message.contains("axiom.lock") || error.message.contains("lockfile") {
                    "invalid"
                } else {
                    "unknown"
                };
            (false, lockfile_status, Vec::new(), Vec::new(), Some(error))
        }
    };
    let checked_ledger: CheckedCapabilityLedger = serde_json::from_str(include_str!(
        "../../../compiler-contracts/snapshots/capability-ledger.json"
    ))
    .expect("checked capability ledger must be valid JSON");
    debug_assert_eq!(checked_ledger.summary.commands, command_count);
    debug_assert_eq!(
        checked_ledger.summary.stdlib_modules,
        crate::stdlib::stdlib_module_names().count()
    );
    debug_assert_eq!(
        checked_ledger.summary.capabilities,
        KNOWN_CAPABILITIES.len()
    );
    DoctorReport {
        schema_version: json_contract::JSON_SCHEMA_VERSION,
        ok,
        command: "doctor",
        project: project.display().to_string(),
        rustc,
        cargo,
        target_triple,
        lockfile_status,
        capabilities,
        workspace_graph,
        capability_ledger: CapabilityLedgerFacts {
            schema_version: "axiom.capability_ledger.v1",
            snapshot: "stage1/compiler-contracts/snapshots/capability-ledger.json",
            commands: command_count,
            stdlib_modules: checked_ledger.summary.stdlib_modules,
            stdlib_functions: checked_ledger.summary.stdlib_functions,
            capabilities: checked_ledger.summary.capabilities,
            runtime_abi_rows: checked_ledger.summary.runtime_abi_rows,
            schemas: checked_ledger.summary.schemas,
            supported_native_backend: if checked_ledger.summary.supported_native_backend
                == SUPPORTED_NATIVE_BACKENDS
            {
                SUPPORTED_NATIVE_BACKENDS
            } else {
                "ledger-backend-drift"
            },
            production_qualified_rows: checked_ledger
                .summary
                .evidence_tiers
                .get("production_qualified")
                .copied()
                .unwrap_or_default(),
        },
        known_unsupported_features: vec![
            "remote package registry resolution",
            "native Axiom DWARF line tables",
            "general borrow checker",
            "dynamic trait dispatch",
        ],
        error,
    }
}

fn probe_tool(program: &str, args: &[&str]) -> ToolProbe {
    match Command::new(program).args(args).output() {
        Ok(output) if output.status.success() => ToolProbe {
            available: true,
            version: Some(String::from_utf8_lossy(&output.stdout).trim().to_string()),
            error: None,
        },
        Ok(output) => ToolProbe {
            available: false,
            version: None,
            error: Some(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        },
        Err(error) => ToolProbe {
            available: false,
            version: None,
            error: Some(error.to_string()),
        },
    }
}

fn parse_rustc_host_target(version: &str) -> Option<String> {
    version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_string)
}

pub fn doctor_text(report: &DoctorReport) -> String {
    let mut lines = vec![
        format!("project: {}", report.project),
        format!("ok: {}", report.ok),
        format!("rustc: {}", tool_text(&report.rustc)),
        format!("cargo: {}", tool_text(&report.cargo)),
        format!(
            "target_triple: {}",
            report.target_triple.as_deref().unwrap_or("unknown")
        ),
        format!("lockfile_status: {}", report.lockfile_status),
        format!("packages: {}", report.workspace_graph.len()),
    ];
    if let Some(error) = &report.error {
        lines.push(format!("error: {error}"));
    }
    lines.push(format!(
        "capability_ledger: {} (commands={}, stdlib_modules={}, stdlib_functions={}, capabilities={}, production_qualified={})",
        report.capability_ledger.snapshot,
        report.capability_ledger.commands,
        report.capability_ledger.stdlib_modules,
        report.capability_ledger.stdlib_functions,
        report.capability_ledger.capabilities,
        report.capability_ledger.production_qualified_rows,
    ));
    lines.push(format!(
        "known_unsupported_features: {}",
        report.known_unsupported_features.join(", ")
    ));
    lines.join("\n")
}

fn tool_text(tool: &ToolProbe) -> String {
    if tool.available {
        tool.version.as_deref().unwrap_or("available").to_string()
    } else {
        format!("missing ({})", tool.error.as_deref().unwrap_or("unknown"))
    }
}

#[cfg(test)]
mod tests {
    use super::{CheckedCapabilityLedger, doctor_report, parse_rustc_host_target};
    use crate::json_contract;
    use crate::new_project::{WorkloadTemplate, create_project_with_template};

    #[test]
    fn reports_project_health_and_capability_ledger_json_fields() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("doctor");
        create_project_with_template(&project, Some("doctor-app"), WorkloadTemplate::Cli)
            .expect("create project");

        let checked_ledger: CheckedCapabilityLedger = serde_json::from_str(include_str!(
            "../../../compiler-contracts/snapshots/capability-ledger.json"
        ))
        .expect("checked capability ledger must be valid JSON");
        let summary = checked_ledger.summary;
        let production_qualified_rows = summary
            .evidence_tiers
            .get("production_qualified")
            .copied()
            .unwrap_or_default();

        let report = doctor_report(&project, summary.commands);
        let payload = serde_json::to_value(&report).expect("serialize doctor report");

        assert_eq!(
            payload["schema_version"],
            json_contract::JSON_SCHEMA_VERSION
        );
        assert_eq!(payload["command"], "doctor");
        assert_eq!(payload["lockfile_status"], "valid");
        assert_eq!(payload["workspace_graph"].as_array().map(Vec::len), Some(1));
        assert!(payload["target_triple"].is_string());
        assert_eq!(
            payload["capability_ledger"]["schemaVersion"],
            "axiom.capability_ledger.v1"
        );
        assert_eq!(payload["capability_ledger"]["commands"], summary.commands);
        assert_eq!(
            payload["capability_ledger"]["stdlibModules"],
            summary.stdlib_modules
        );
        assert_eq!(
            payload["capability_ledger"]["stdlibFunctions"],
            summary.stdlib_functions
        );
        assert_eq!(
            payload["capability_ledger"]["capabilities"],
            summary.capabilities
        );
        assert_eq!(
            payload["capability_ledger"]["runtimeAbiRows"],
            summary.runtime_abi_rows
        );
        assert_eq!(payload["capability_ledger"]["schemas"], summary.schemas);
        assert_eq!(
            payload["capability_ledger"]["supportedNativeBackend"],
            "cranelift"
        );
        assert_eq!(
            payload["capability_ledger"]["productionQualifiedRows"],
            production_qualified_rows
        );
        assert!(
            !payload["known_unsupported_features"]
                .as_array()
                .expect("unsupported features")
                .contains(&serde_json::json!("closures"))
        );
    }

    #[test]
    fn rustc_host_target_parser_reads_verbose_version_output() {
        let version = "rustc 1.90.0\nhost: aarch64-apple-darwin\nrelease: 1.90.0\n";
        assert_eq!(
            parse_rustc_host_target(version).as_deref(),
            Some("aarch64-apple-darwin")
        );
    }
}
