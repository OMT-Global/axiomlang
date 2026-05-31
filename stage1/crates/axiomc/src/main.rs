use axiomc::codegen::NativeBackendKind;
use axiomc::dap;
use axiomc::diagnostic_catalog::{DiagnosticCodeInfo, diagnostic_code_info};
use axiomc::diagnostics::Diagnostic;
use axiomc::json_contract;
use axiomc::lockfile::{expected_lockfile_for_project, validate_lockfile};
use axiomc::lsp;
use axiomc::manifest::{
    CapabilityDescriptor, TestKind, binary_path, entry_path, generated_rust_path, load_manifest,
    lockfile_path, manifest_path, out_dir_path,
};
#[cfg(test)]
use axiomc::new_project::create_project;
use axiomc::new_project::{WorkloadTemplate, create_project_with_template};
use axiomc::project::{
    BuildOptions, BuildOutput, CheckOptions, RunOptions, TestOptions, build_project_with_options,
    capability_sbom, check_project_with_options, list_project_tests_with_options,
    package_graph_metadata, project_capabilities, run_project_tests_with_options,
    run_project_with_options, trace_provenance,
};
use axiomc::registry::{
    PublishOptions, load_registry_index, publish_package, render_registry_index,
};
use axiomc::syntax::parse_program;
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::Instant;

#[derive(Debug, Parser)]
#[command(name = "axiomc", about = "Axiom stage1 bootstrap compiler")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a new stage1 package with axiom.toml, axiom.lock, and starter source.
    New {
        path: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value = "cli")]
        template: String,
    },
    /// Parse the primary stage1 package entrypoint without typechecking.
    Parse {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Check a stage1 package or workspace member without building an artifact.
    Check {
        path: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        exports: bool,
        #[arg(long = "debug-symbols")]
        debug_symbols: bool,
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
    },
    /// Build a stage1 package through the current generated-Rust backend path into a native or WASM artifact.
    Build {
        path: PathBuf,
        #[arg(long)]
        json: bool,
        /// Select the preparatory native-backend plumbing seam. Today only `generated-rust` is implemented; additional native backends remain future work.
        #[arg(long, default_value_t = NativeBackendKind::GeneratedRust)]
        backend: NativeBackendKind,
        #[arg(long)]
        debug: bool,
        #[arg(long)]
        timings: bool,
        #[arg(long)]
        target: Option<String>,
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
        /// Require axiom.lock to exactly match the local manifest/workspace/dependency graph.
        #[arg(long)]
        locked: bool,
        /// Resolve the build using only local path graph data and no network access.
        #[arg(long)]
        offline: bool,
    },
    /// Build and run a stage1 package through the current generated-Rust backend path.
    Run {
        path: PathBuf,
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Trace package intent-to-artifact provenance emitted by `axiomc build`.
    Trace {
        /// Project path to trace, or an axiom:// node or artifact id to trace in the current project.
        query: String,
        #[arg(long)]
        json: bool,
    },
    /// Discover, build, and run package test entrypoints.
    Test {
        path: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        filter: Option<String>,
        #[arg(long)]
        include_benchmarks: bool,
        #[arg(long)]
        list: bool,
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
    },
    /// Inspect manifest capability requirements.
    Caps {
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
        #[arg(long, value_enum)]
        format: Option<CapsFormat>,
        #[command(subcommand)]
        command: Option<CapsCommand>,
    },
    /// Emit a structured repair plan for diagnostics and missing evidence.
    RepairPlan {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Emit semantic evidence requirements and observed test evidence.
    Evidence {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Inspect project metadata for agent tooling.
    Inspect {
        #[command(subcommand)]
        command: InspectCommand,
    },
    /// Generate target artifacts from stage1 semantic intent.
    Generate {
        #[command(subcommand)]
        command: GenerateCommand,
    },
    /// Inspect local package graph metadata.
    Pkg {
        #[command(subcommand)]
        command: PkgCommand,
    },
    /// Explain a stable diagnostic code.
    Explain {
        code: String,
        #[arg(long)]
        json: bool,
    },
    /// Report local stage1 project and toolchain health.
    Doctor {
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Format .ax source files with the canonical stage1 style.
    Fmt {
        path: PathBuf,
        #[arg(long)]
        check: bool,
        #[arg(long)]
        json: bool,
    },
    /// Generate Markdown and HTML API docs from source doc comments.
    Doc {
        path: PathBuf,
        #[arg(long, default_value = "docs/axiom")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Run discovered *_bench.ax entrypoints with warmup and iterations.
    Bench {
        path: PathBuf,
        #[arg(long, default_value_t = 1)]
        warmup: usize,
        #[arg(long, default_value_t = 5)]
        iterations: usize,
        #[arg(long)]
        json: bool,
    },
    /// Convert mutation-test survivors into a stable issue-comment report.
    MutationReport {
        /// JSON mutation output from tools such as cargo-mutants.
        input: PathBuf,
        /// Emit the normalized machine-readable report instead of Markdown.
        #[arg(long)]
        json: bool,
    },
    /// Start a small stage1 scratch REPL backed by axiomc check/run.
    Repl {
        #[arg(long)]
        json: bool,
    },
    /// Pack and publish a stage1 package into a local registry tree.
    ///
    /// Note: --signing-key is required and the emitted `.sig` payload is a
    /// tamper-detection integrity tag bound to that key, not a cryptographic
    /// signature. The stage1 registry does not yet provide authenticity proof.
    Publish {
        path: PathBuf,
        #[arg(long = "registry-dir")]
        registry_dir: PathBuf,
        #[arg(
            long = "signing-key",
            help = "Required integrity key bound into the emitted .sig payload. Not authenticity proof."
        )]
        signing_key: Option<String>,
        #[arg(long)]
        allow_overwrite: bool,
    },
    /// Build a static package-registry index from package release folders.
    RegistryIndex {
        packages_dir: PathBuf,
        #[arg(long)]
        base_url: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Validate a static package-registry index JSON file.
    RegistryValidate { index: PathBuf },
    /// Start the bounded axiom-analyzer Language Server Protocol endpoint.
    Lsp,
    /// Start the bounded axiom-debug Debug Adapter Protocol endpoint.
    Dap,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CapsFormat {
    SbomJson,
}

#[derive(Debug, Subcommand)]
enum CapsCommand {
    /// Diff two caps JSON payloads and fail on capability escalation.
    Diff { old: PathBuf, new: PathBuf },
}

#[derive(Debug, Subcommand)]
enum InspectCommand {
    /// Emit exported functions, types, consts, imports, and capability use.
    Symbols {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Emit package, module, and semantic declaration graph details.
    Graph {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Emit semantic effect nodes for known runtime and stdlib surfaces.
    Effects {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Emit planned and generated artifact records for a package.
    Artifacts {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum GenerateCommand {
    /// Generate an OpenAPI 3.1 document from HTTP-serving routes.
    Openapi {
        path: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Generate a deterministic policy allowlist bundle from manifest capabilities and effects.
    Policy {
        path: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Generate a deterministic operator runbook from semantic intent and evidence.
    Runbook {
        path: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum PkgCommand {
    /// Print resolved packages, members, dependencies, entrypoints, capabilities, and lockfile status.
    Graph {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Command::New {
            path,
            name,
            template,
        } => match WorkloadTemplate::parse(&template)
            .and_then(|template| create_project_with_template(&path, name.as_deref(), template))
        {
            Ok(()) => {
                println!(
                    "initialized stage1 {template} project in {}",
                    path.display()
                );
                0
            }
            Err(error) => print_error("new", error, false),
        },
        Command::Parse { path, json } => match parse_project_entry(&path) {
            Ok(output) => {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "schema_version": json_contract::JSON_SCHEMA_VERSION,
                            "ok": true,
                            "command": "parse",
                            "project": path.display().to_string(),
                            "manifest": output.manifest,
                            "entry": output.entry,
                            "statement_count": output.statement_count,
                        })
                    );
                } else {
                    eprintln!("OK statements={}", output.statement_count);
                }
                0
            }
            Err(error) => print_error("parse", error, json),
        },
        Command::Check {
            path,
            json,
            exports,
            debug_symbols,
            package,
        } => match check_project_with_options(
            &path,
            &CheckOptions {
                package: package.clone(),
                include_exports: exports,
                include_debug_symbols: debug_symbols,
            },
        ) {
            Ok(output) => {
                if json {
                    println!("{}", json_contract::check_success(&path, &output));
                } else {
                    for warning in &output.warnings {
                        eprintln!("{warning}");
                    }
                    eprintln!("OK");
                }
                0
            }
            Err(error) => print_error("check", error, json),
        },
        Command::Build {
            path,
            json,
            backend,
            debug,
            timings,
            target,
            locked,
            offline,
            package,
        } => {
            match build_project_with_options(
                &path,
                &BuildOptions {
                    backend,
                    target,
                    package: package.clone(),
                    debug,
                    locked,
                    offline,
                },
            ) {
                Ok(output) => {
                    if json {
                        println!("{}", json_contract::build_success(&path, &output));
                    } else {
                        for line in build_summary_lines(&output, timings) {
                            eprintln!("{line}");
                        }
                    }
                    0
                }
                Err(error) => print_error("build", error, json),
            }
        }
        Command::Run {
            path,
            package,
            args,
        } => match run_project_with_options(
            &path,
            &RunOptions {
                package: package.clone(),
                args: args.clone(),
            },
        ) {
            Ok(code) => code,
            Err(error) => print_error("run", error, false),
        },
        Command::Trace { query, json } => {
            let (project, node_query) = trace_project_and_query(&query);
            match trace_provenance(&project, node_query.as_deref()) {
                Ok(report) => {
                    if json {
                        println!(
                            "{}",
                            json_contract::to_pretty_string(&report)
                                .unwrap_or_else(|_| String::from("{}"))
                        );
                    } else {
                        println!(
                            "nodes={} artifacts={} relationships={}",
                            report.nodes.len(),
                            report.artifacts.len(),
                            report.relationships.len()
                        );
                    }
                    0
                }
                Err(error) => print_error("trace", error, json),
            }
        }
        Command::Test {
            path,
            json,
            filter,
            include_benchmarks,
            list,
            package,
        } => {
            let options = TestOptions {
                filter: filter.clone(),
                package: package.clone(),
                include_benchmarks,
            };
            if list {
                match list_project_tests_with_options(&path, &options) {
                    Ok(output) => {
                        if json {
                            println!(
                                "{}",
                                json_contract::test_list_success(&path, filter.as_deref(), &output)
                            );
                        } else {
                            for test in &output.tests {
                                let package = test.package.as_deref().unwrap_or("<unnamed>");
                                eprintln!(
                                    "{:?} {} {} ({})",
                                    test.kind, package, test.name, test.entry
                                );
                            }
                            eprintln!("discovered: {}", output.tests.len());
                        }
                        0
                    }
                    Err(error) => print_error("test", error, json),
                }
            } else {
                match run_project_tests_with_options(&path, &options) {
                    Ok(output) => {
                        let ok = output.failed == 0;
                        if json {
                            println!(
                                "{}",
                                json_contract::test_success(&path, filter.as_deref(), &output)
                            );
                        } else {
                            for case in &output.cases {
                                let status = if case.ok { "PASS" } else { "FAIL" };
                                eprintln!(
                                    "{status} {:?} {} ({})",
                                    case.kind, case.name, case.entry
                                );
                                if let Some(error) = &case.error {
                                    eprintln!("  {}", error);
                                }
                                eprintln!("  duration: {} ms", case.duration_ms);
                            }
                            eprintln!(
                                "passed: {} failed: {} skipped: {} duration: {} ms",
                                output.passed, output.failed, output.skipped, output.duration_ms
                            );
                        }
                        if ok { 0 } else { 1 }
                    }
                    Err(error) => print_error("test", error, json),
                }
            }
        }
        Command::Caps {
            path,
            json,
            format,
            command,
        } => match command {
            Some(CapsCommand::Diff { old, new }) => {
                if path.is_some() {
                    print_error(
                        "caps",
                        Diagnostic::new("caps", "`caps diff` does not accept PATH"),
                        json,
                    )
                } else {
                    match diff_caps_files(&old, &new) {
                        Ok(report) => match json_contract::to_pretty_string(&report) {
                            Ok(output) => {
                                println!("{output}");
                                if report.escalated { 1 } else { 0 }
                            }
                            Err(error) => print_error("caps", error, false),
                        },
                        Err(error) => print_error("caps", error, json),
                    }
                }
            }
            None => {
                let project = path.unwrap_or_else(|| PathBuf::from("."));
                if matches!(format, Some(CapsFormat::SbomJson)) {
                    match capability_sbom(&project) {
                        Ok(sbom) => match json_contract::to_pretty_string(&sbom) {
                            Ok(output) => {
                                println!("{output}");
                                0
                            }
                            Err(error) => print_error("caps", error, false),
                        },
                        Err(error) => print_error("caps", error, json),
                    }
                } else if json {
                    match (project_capabilities(&project), capability_sbom(&project)) {
                        (Ok(capabilities), Ok(sbom)) => {
                            let payload = json_contract::caps_manifest_success(
                                &project,
                                &capabilities,
                                &sbom,
                            );
                            match json_contract::to_pretty_string(&payload) {
                                Ok(output) => {
                                    println!("{output}");
                                    0
                                }
                                Err(error) => print_error("caps", error, false),
                            }
                        }
                        (Err(error), _) | (_, Err(error)) => print_error("caps", error, json),
                    }
                } else {
                    match project_capabilities(&project) {
                        Ok(capabilities) => {
                            let payload = json_contract::caps_success(&project, &capabilities);
                            match json_contract::to_pretty_string(&payload) {
                                Ok(output) => {
                                    println!("{output}");
                                    0
                                }
                                Err(error) => print_error("caps", error, false),
                            }
                        }
                        Err(error) => print_error("caps", error, json),
                    }
                }
            }
        },
        Command::Explain { code, json } => match diagnostic_code_info(&code) {
            Some(info) => {
                if json {
                    println!(
                        "{}",
                        json_contract::to_pretty_string(&explain_payload(info))
                            .unwrap_or_else(|_| String::from("{}"))
                    );
                } else {
                    println!("{}", explain_text(info));
                }
                0
            }
            None => print_error(
                "explain",
                Diagnostic::new("diagnostic", format!("unknown diagnostic code {code:?}")),
                json,
            ),
        },
        Command::RepairPlan { path, json } => match repair_plan(&path) {
            Ok(report) => {
                if json {
                    println!(
                        "{}",
                        json_contract::to_pretty_string(&report)
                            .unwrap_or_else(|_| String::from("{}"))
                    );
                } else {
                    println!("tasks={}", report.tasks.len());
                    for task in &report.tasks {
                        println!("{} {} {}", task.id, task.reason, task.target_node);
                    }
                }
                0
            }
            Err(error) => print_error("repair-plan", error, json),
        },
        Command::Evidence { path, json } => match evidence_report(&path) {
            Ok(report) => {
                if json {
                    println!(
                        "{}",
                        json_contract::to_pretty_string(&report)
                            .unwrap_or_else(|_| String::from("{}"))
                    );
                } else {
                    println!(
                        "package={} evidence={} passing={} failing={} missing={}",
                        report.package,
                        report.evidence.len(),
                        report.summary.passing,
                        report.summary.failing,
                        report.summary.missing
                    );
                }
                0
            }
            Err(error) => print_error("evidence", error, json),
        },
        Command::Inspect { command } => match command {
            InspectCommand::Symbols { path, json } => match inspect_symbols(&path) {
                Ok(report) => {
                    if json {
                        println!(
                            "{}",
                            json_contract::to_pretty_string(&report)
                                .unwrap_or_else(|_| String::from("{}"))
                        );
                    } else {
                        for symbol in &report.symbols {
                            println!(
                                "{} {} {}:{}",
                                symbol.kind, symbol.name, symbol.span.path, symbol.span.line
                            );
                        }
                    }
                    0
                }
                Err(error) => print_error("inspect symbols", error, json),
            },
            InspectCommand::Graph { path, json } => match inspect_graph(&path) {
                Ok(report) => {
                    if json {
                        println!(
                            "{}",
                            json_contract::to_pretty_string(&report)
                                .unwrap_or_else(|_| String::from("{}"))
                        );
                    } else {
                        println!(
                            "packages={} modules={} semantic_nodes={} semantic_edges={} import_errors={}",
                            report.packages.len(),
                            report.modules.len(),
                            report.nodes.len(),
                            report.edges.len(),
                            report.import_errors.len()
                        );
                    }
                    0
                }
                Err(error) => print_error("inspect graph", error, json),
            },
            InspectCommand::Effects { path, json } => match inspect_effects(&path) {
                Ok(report) => {
                    if json {
                        println!(
                            "{}",
                            json_contract::to_pretty_string(&report)
                                .unwrap_or_else(|_| String::from("{}"))
                        );
                    } else {
                        for effect in &report.effects {
                            println!(
                                "{} {} {}:{}",
                                effect.kind,
                                effect.resource,
                                effect.source_span.path,
                                effect.source_span.line
                            );
                        }
                    }
                    0
                }
                Err(error) => print_error("inspect effects", error, json),
            },
            InspectCommand::Artifacts { path, json } => match inspect_artifacts(&path) {
                Ok(report) => {
                    if json {
                        println!(
                            "{}",
                            json_contract::to_pretty_string(&report)
                                .unwrap_or_else(|_| String::from("{}"))
                        );
                    } else {
                        for artifact in &report.artifacts {
                            println!("{} {} {}", artifact.kind, artifact.status, artifact.path);
                        }
                    }
                    0
                }
                Err(error) => print_error("inspect artifacts", error, json),
            },
        },
        Command::Generate { command } => match command {
            GenerateCommand::Openapi { path, out, json } => match generate_openapi(&path, &out) {
                Ok(report) => {
                    if json {
                        println!(
                            "{}",
                            json_contract::to_pretty_string(&report)
                                .unwrap_or_else(|_| String::from("{}"))
                        );
                    } else {
                        eprintln!("wrote {}", report.artifact.path);
                        if !report.diagnostics.is_empty() {
                            for diagnostic in &report.diagnostics {
                                eprintln!("{}", diagnostic.message);
                            }
                        }
                    }
                    0
                }
                Err(error) => print_error("generate openapi", error, json),
            },
            GenerateCommand::Policy { path, out, json } => match generate_policy(&path, &out) {
                Ok(report) => {
                    if json {
                        println!(
                            "{}",
                            json_contract::to_pretty_string(&report)
                                .unwrap_or_else(|_| String::from("{}"))
                        );
                    } else {
                        eprintln!("wrote {}", report.artifact.path);
                    }
                    0
                }
                Err(error) => print_error("generate policy", error, json),
            },
            GenerateCommand::Runbook { path, out, json } => match generate_runbook(&path, &out) {
                Ok(report) => {
                    if json {
                        println!(
                            "{}",
                            json_contract::to_pretty_string(&report)
                                .unwrap_or_else(|_| String::from("{}"))
                        );
                    } else {
                        eprintln!("wrote {}", report.artifact.path);
                        if !report.diagnostics.is_empty() {
                            for diagnostic in &report.diagnostics {
                                eprintln!("{}", diagnostic.message);
                            }
                        }
                    }
                    0
                }
                Err(error) => print_error("generate runbook", error, json),
            },
        },
        Command::Pkg { command } => match command {
            PkgCommand::Graph { path, json } => match package_graph_metadata(&path) {
                Ok(output) => {
                    if json {
                        println!(
                            "{}",
                            json_contract::to_pretty_string(&output)
                                .unwrap_or_else(|_| String::from("{}"))
                        );
                    } else {
                        for package in &output.packages {
                            let name = package.name.as_deref().unwrap_or("<workspace>");
                            println!("{} {}", name, package.root);
                        }
                    }
                    0
                }
                Err(error) => print_error("pkg graph", error, json),
            },
        },
        Command::Doctor { path, json } => {
            let project = path.unwrap_or_else(|| PathBuf::from("."));
            let report = doctor_report(&project);
            if json {
                match json_contract::to_pretty_string(&report) {
                    Ok(output) => {
                        println!("{output}");
                        if report.ok { 0 } else { 1 }
                    }
                    Err(error) => print_error("doctor", error, false),
                }
            } else {
                println!("{}", doctor_text(&report));
                if report.ok { 0 } else { 1 }
            }
        }
        Command::Fmt { path, check, json } => match format_axiom_sources(&path, check) {
            Ok(report) => {
                let serialization_error = if json {
                    match json_contract::to_pretty_string(&report) {
                        Ok(output) => {
                            println!("{output}");
                            None
                        }
                        Err(error) => Some(error),
                    }
                } else {
                    None
                };
                if let Some(error) = serialization_error {
                    print_error("fmt", error, true)
                } else {
                    if !json {
                        for file in &report.files {
                            if file.changed {
                                eprintln!("formatted {}", file.path);
                            }
                        }
                        if check && report.changed > 0 {
                            eprintln!("{} file(s) need formatting", report.changed);
                        } else {
                            eprintln!("checked {} file(s)", report.files.len());
                        }
                    }
                    if check && report.changed > 0 { 1 } else { 0 }
                }
            }
            Err(error) => print_error("fmt", error, json),
        },
        Command::Doc {
            path,
            out_dir,
            json,
        } => match generate_docs(&path, &out_dir) {
            Ok(output) => {
                if json {
                    match json_contract::to_pretty_string(&output) {
                        Ok(payload) => {
                            println!("{payload}");
                            0
                        }
                        Err(error) => print_error("doc", error, json),
                    }
                } else {
                    eprintln!("wrote {}", output.markdown.display());
                    eprintln!("wrote {}", output.html.display());
                    0
                }
            }
            Err(error) => print_error("doc", error, json),
        },
        Command::Bench {
            path,
            warmup,
            iterations,
            json,
        } => match run_benchmarks(&path, warmup, iterations) {
            Ok(report) => {
                if json {
                    println!(
                        "{}",
                        json_contract::to_pretty_string(&report)
                            .unwrap_or_else(|_| String::from("{}"))
                    );
                } else {
                    for bench in &report.benches {
                        println!(
                            "{} median={}ms p95={}ms iterations={}",
                            bench.name, bench.median_ms, bench.p95_ms, bench.iterations
                        );
                    }
                }
                if report.failed == 0 { 0 } else { 1 }
            }
            Err(error) => print_error("bench", error, json),
        },
        Command::MutationReport { input, json } => match mutation_report_from_path(&input) {
            Ok(report) => {
                if json {
                    println!(
                        "{}",
                        json_contract::to_pretty_string(&report)
                            .unwrap_or_else(|_| String::from("{}"))
                    );
                } else {
                    println!("{}", render_mutation_issue_report(&report));
                }
                0
            }
            Err(error) => print_error("mutation-report", error, json),
        },
        Command::Repl { json } => match run_repl(io::stdin().lock(), io::stdout(), json) {
            Ok(()) => 0,
            Err(error) => print_error("repl", error, json),
        },
        Command::Publish {
            path,
            registry_dir,
            signing_key,
            allow_overwrite,
        } => match publish_package(
            &path,
            &registry_dir,
            &PublishOptions {
                signing_key,
                allow_overwrite,
            },
        ) {
            Ok(output) => {
                eprintln!(
                    "published {}@{} to {}",
                    output.package, output.version, output.release_dir
                );
                eprintln!("wrote {}", output.archive);
                eprintln!("wrote {}", output.signature);
                0
            }
            Err(error) => print_error("publish", error, false),
        },
        Command::RegistryIndex {
            packages_dir,
            base_url,
            out,
        } => match render_registry_index(&packages_dir, &base_url) {
            Ok(index) => {
                if let Some(path) = out {
                    match fs::write(&path, index) {
                        Ok(()) => {
                            eprintln!("wrote {}", path.display());
                            0
                        }
                        Err(err) => print_error(
                            "registry-index",
                            Diagnostic::new(
                                "registry",
                                format!("failed to write {}: {err}", path.display()),
                            )
                            .with_path(path.display().to_string()),
                            false,
                        ),
                    }
                } else {
                    println!("{index}");
                    0
                }
            }
            Err(error) => print_error("registry-index", error, false),
        },
        Command::RegistryValidate { index } => match load_registry_index(&index) {
            Ok(_) => {
                eprintln!("OK");
                0
            }
            Err(error) => print_error("registry-validate", error, false),
        },
        Command::Lsp => match lsp::run_stdio(io::stdin().lock(), io::stdout()) {
            Ok(()) => 0,
            Err(error) => print_error("lsp", error, false),
        },
        Command::Dap => match dap::run_stdio(io::stdin().lock(), io::stdout()) {
            Ok(()) => 0,
            Err(error) => print_error("dap", error, false),
        },
    };
    std::process::exit(code);
}

#[derive(Debug)]
struct ParseOutput {
    manifest: String,
    entry: String,
    statement_count: usize,
}

fn parse_project_entry(path: &Path) -> Result<ParseOutput, Diagnostic> {
    let manifest = load_manifest(path)?;
    let entry = entry_path(path, &manifest);
    let source = fs::read_to_string(&entry).map_err(|err| {
        Diagnostic::new(
            "parse",
            format!("failed to read {}: {err}", entry.display()),
        )
    })?;
    let program = parse_program(&source, &entry)?;
    Ok(ParseOutput {
        manifest: path.join("axiom.toml").display().to_string(),
        entry: entry.display().to_string(),
        statement_count: program.stmts.len(),
    })
}

fn build_summary_lines(output: &BuildOutput, timings: bool) -> Vec<String> {
    let mut lines = vec![format!(
        "wrote {} (backend={})",
        output.binary, output.backend
    )];
    if let Some(debug_map) = &output.debug_map {
        lines.push(format!("wrote debug map {debug_map}"));
    }
    if let Some(debug_manifest) = &output.debug_manifest {
        lines.push(format!("wrote debug manifest {debug_manifest}"));
    }
    if timings {
        lines.push(
            format!(
                "timings total={}ms cache_hits={} cache_misses={}",
                output.duration_ms, output.cache_hits, output.cache_misses
            )
            .trim_end()
            .to_string(),
        );
        for package in &output.packages {
            lines.push(format!(
                "timings package={} cache_status={:?} compile={}ms",
                package.package_root, package.cache_status, package.compile_ms
            ));
        }
    }
    lines
}

#[derive(Debug, Deserialize)]
struct CapsPayload {
    capabilities: Vec<CapsDescriptor>,
}

#[derive(Debug, Clone, Deserialize)]
struct CapsDescriptor {
    name: String,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    allowed: Vec<String>,
    #[serde(default)]
    unsafe_unrestricted: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct CapsDiffReport {
    schema_version: &'static str,
    ok: bool,
    command: &'static str,
    old: String,
    new: String,
    added_capabilities: Vec<String>,
    removed_capabilities: Vec<String>,
    escalated_capabilities: Vec<String>,
    added_scopes: Vec<CapsScopeDiff>,
    removed_scopes: Vec<CapsScopeDiff>,
    unsafe_escalations: Vec<String>,
    unsafe_reductions: Vec<String>,
    escalated: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct CapsScopeDiff {
    capability: String,
    scopes: Vec<String>,
}

fn diff_caps_files(old: &Path, new: &Path) -> Result<CapsDiffReport, Diagnostic> {
    let old_payload = read_caps_payload(old)?;
    let new_payload = read_caps_payload(new)?;
    Ok(diff_caps_payloads(
        &old_payload,
        &new_payload,
        old.display().to_string(),
        new.display().to_string(),
    ))
}

fn read_caps_payload(path: &Path) -> Result<CapsPayload, Diagnostic> {
    let content = fs::read_to_string(path).map_err(|err| {
        Diagnostic::new("caps", format!("failed to read {}: {err}", path.display()))
            .with_path(path.display().to_string())
    })?;
    serde_json::from_str(&content).map_err(|err| {
        Diagnostic::new(
            "caps",
            format!("failed to parse caps JSON {}: {err}", path.display()),
        )
        .with_path(path.display().to_string())
    })
}

fn diff_caps_payloads(
    old: &CapsPayload,
    new: &CapsPayload,
    old_path: String,
    new_path: String,
) -> CapsDiffReport {
    let old_caps = caps_by_name(old);
    let new_caps = caps_by_name(new);
    let names: BTreeSet<String> = old_caps.keys().chain(new_caps.keys()).cloned().collect();

    let mut added_capabilities = Vec::new();
    let mut removed_capabilities = Vec::new();
    let mut escalated_capabilities = Vec::new();
    let mut added_scopes = Vec::new();
    let mut removed_scopes = Vec::new();
    let mut unsafe_escalations = Vec::new();
    let mut unsafe_reductions = Vec::new();

    for name in names {
        let old_cap = old_caps.get(&name);
        let new_cap = new_caps.get(&name);
        let old_enabled = old_cap.is_some_and(|cap| cap.enabled);
        let new_enabled = new_cap.is_some_and(|cap| cap.enabled);

        match (old_enabled, new_enabled) {
            (false, true) => {
                added_capabilities.push(name.clone());
                escalated_capabilities.push(name.clone());
            }
            (true, false) => removed_capabilities.push(name.clone()),
            _ => {}
        }

        if old_enabled && new_enabled {
            if let Some(diff) = scope_diff(&name, old_cap, new_cap, true) {
                added_scopes.push(diff);
            }
            if let Some(diff) = scope_diff(&name, old_cap, new_cap, false) {
                removed_scopes.push(diff);
            }
        }

        let old_unsafe = old_cap.is_some_and(|cap| cap.unsafe_unrestricted);
        let new_unsafe = new_cap.is_some_and(|cap| cap.unsafe_unrestricted);
        match (old_unsafe, new_unsafe) {
            (false, true) => unsafe_escalations.push(name.clone()),
            (true, false) => unsafe_reductions.push(name.clone()),
            _ => {}
        }
    }

    let escalated = !escalated_capabilities.is_empty()
        || !added_scopes.is_empty()
        || !unsafe_escalations.is_empty();

    CapsDiffReport {
        schema_version: json_contract::JSON_SCHEMA_VERSION,
        ok: !escalated,
        command: "caps diff",
        old: old_path,
        new: new_path,
        added_capabilities,
        removed_capabilities,
        escalated_capabilities,
        added_scopes,
        removed_scopes,
        unsafe_escalations,
        unsafe_reductions,
        escalated,
    }
}

fn caps_by_name(payload: &CapsPayload) -> BTreeMap<String, CapsDescriptor> {
    payload
        .capabilities
        .iter()
        .map(|capability| (capability.name.clone(), capability.clone()))
        .collect()
}

fn scope_diff(
    name: &str,
    old: Option<&CapsDescriptor>,
    new: Option<&CapsDescriptor>,
    added: bool,
) -> Option<CapsScopeDiff> {
    let old_scopes: BTreeSet<String> = old
        .into_iter()
        .flat_map(|capability| capability.allowed.iter().cloned())
        .collect();
    let new_scopes: BTreeSet<String> = new
        .into_iter()
        .flat_map(|capability| capability.allowed.iter().cloned())
        .collect();
    let scopes: Vec<String> = if added {
        new_scopes.difference(&old_scopes).cloned().collect()
    } else {
        old_scopes.difference(&new_scopes).cloned().collect()
    };
    (!scopes.is_empty()).then(|| CapsScopeDiff {
        capability: name.to_string(),
        scopes,
    })
}

fn explain_payload(info: &DiagnosticCodeInfo) -> serde_json::Value {
    serde_json::json!({
        "schema_version": json_contract::JSON_SCHEMA_VERSION,
        "ok": true,
        "command": "explain",
        "diagnostic": info,
    })
}

fn explain_text(info: &DiagnosticCodeInfo) -> String {
    format!(
        "{code} ({kind})
{title}

{explanation}

Example:
{example}

Suggested fix:
{suggested_fix}",
        code = info.code,
        kind = info.kind,
        title = info.title,
        explanation = info.explanation,
        example = info.example,
        suggested_fix = info.suggested_fix,
    )
}

fn print_error(command: &str, error: Diagnostic, json: bool) -> i32 {
    if json {
        println!("{}", json_contract::error(command, &error));
    } else {
        eprintln!("{error}");
        for related in &error.related {
            eprintln!("{related}");
        }
    }
    1
}

#[derive(Debug, Clone, Serialize)]
struct FormatEdit {
    action: String,
    line: usize,
    before: Option<String>,
    after: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RepairPlanReport {
    schema_version: &'static str,
    ok: bool,
    command: &'static str,
    project: String,
    tasks: Vec<RepairTask>,
}

#[derive(Debug, Clone, Serialize)]
struct RepairTask {
    id: String,
    reason: String,
    target_node: String,
    allowed_files: Vec<String>,
    required_evidence: Vec<&'static str>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize)]
struct InspectSymbolsReport {
    schema_version: &'static str,
    schema: &'static str,
    ok: bool,
    command: &'static str,
    project: String,
    symbols: Vec<InspectedSymbol>,
}

#[derive(Debug, Clone, Serialize)]
struct InspectedSymbol {
    name: String,
    kind: &'static str,
    signature: String,
    span: SymbolSpan,
    imports: Vec<String>,
    capabilities: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
struct SymbolSpan {
    path: String,
    line: usize,
    column: usize,
}

fn repair_plan(path: &Path) -> Result<RepairPlanReport, Diagnostic> {
    let mut tasks = Vec::new();
    match check_project_with_options(path, &CheckOptions::default()) {
        Ok(output) => {
            if !repair_tests_discoverable(path)? {
                tasks.push(missing_evidence_task(
                    "repair-001",
                    package_node_for_path(path),
                    repair_allowed_files(path)?,
                ));
            }
            for warning in output.warnings {
                tasks.push(diagnostic_repair_task(
                    tasks.len() + 1,
                    package_node_for_path(path),
                    Diagnostic::new("warning", warning).normalized_for_json(),
                ));
            }
        }
        Err(error) => {
            tasks.push(diagnostic_repair_task(
                tasks.len() + 1,
                package_node_for_path(path),
                error.normalized_for_json(),
            ));
        }
    }
    Ok(RepairPlanReport {
        schema_version: "axiom.repair.v0",
        ok: true,
        command: "repair-plan",
        project: path.display().to_string(),
        tasks,
    })
}

fn diagnostic_repair_task(
    index: usize,
    fallback_target: String,
    diagnostic: Diagnostic,
) -> RepairTask {
    let allowed_files = diagnostic
        .path
        .as_ref()
        .map(|path| vec![path.clone()])
        .unwrap_or_default();
    let target_node = diagnostic
        .path
        .as_ref()
        .map(|path| {
            format!(
                "{}/diagnostic/{}",
                package_node_component(path),
                repair_component(diagnostic.code.as_deref().unwrap_or(&diagnostic.kind))
            )
        })
        .unwrap_or(fallback_target);
    RepairTask {
        id: format!("repair-{index:03}"),
        reason: diagnostic
            .code
            .clone()
            .unwrap_or_else(|| repair_component(&diagnostic.kind)),
        target_node,
        allowed_files,
        required_evidence: vec!["unit_test"],
        diagnostics: vec![diagnostic],
    }
}

fn missing_evidence_task(id: &str, target_node: String, allowed_files: Vec<String>) -> RepairTask {
    RepairTask {
        id: id.to_string(),
        reason: String::from("missing_evidence"),
        target_node,
        allowed_files,
        required_evidence: vec!["unit_test"],
        diagnostics: Vec::new(),
    }
}

fn repair_tests_discoverable(path: &Path) -> Result<bool, Diagnostic> {
    if load_manifest(path)
        .map(|manifest| !manifest.tests.is_empty())
        .unwrap_or(false)
    {
        return Ok(true);
    }
    Ok(axiom_files(path)?.iter().any(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("_test.ax") || name.ends_with("_bench.ax"))
    }))
}

#[derive(Debug, Clone, Serialize)]
struct EvidenceReport {
    schema_version: &'static str,
    ok: bool,
    command: &'static str,
    project: String,
    package: String,
    validation_status: &'static str,
    summary: EvidenceSummary,
    evidence: Vec<EvidenceItem>,
}

#[derive(Debug, Clone, Serialize)]
struct EvidenceSummary {
    passing: usize,
    failing: usize,
    missing: usize,
    provided: usize,
}

#[derive(Debug, Clone, Serialize)]
struct EvidenceItem {
    id: String,
    evidence_type: &'static str,
    status: &'static str,
    target: String,
    path: Option<String>,
    diagnostics: Vec<String>,
}

fn evidence_report(project: &Path) -> Result<EvidenceReport, Diagnostic> {
    let manifest = load_manifest(project)?;
    let package_name = manifest
        .package
        .as_ref()
        .map(|package| package.name.clone())
        .unwrap_or_else(|| String::from("workspace"));
    let package_component = evidence_id_component(&package_name);
    let package_target = format!("axiom://package/{package_component}");
    let mut evidence = Vec::new();
    if evidence_tests_discoverable(project, &manifest)? {
        let test_output = run_project_tests_with_options(
            project,
            &TestOptions {
                filter: None,
                package: None,
                include_benchmarks: true,
            },
        )?;
        for case in &test_output.cases {
            let evidence_type = evidence_type_for_test_kind(case.kind);
            let status = if case.ok { "passing" } else { "failing" };
            let diagnostics = case
                .error
                .as_ref()
                .map(|error| vec![error.to_string()])
                .unwrap_or_default();
            evidence.push(EvidenceItem {
                id: format!(
                    "axiom://package/{package_component}/evidence/{}",
                    evidence_id_component(&case.name)
                ),
                evidence_type,
                status,
                target: package_target.clone(),
                path: Some(case.entry.clone()),
                diagnostics,
            });
        }
    }
    if evidence.is_empty() {
        evidence.push(EvidenceItem {
            id: format!("axiom://package/{package_component}/evidence/unit-test-required"),
            evidence_type: "unit_test",
            status: "missing",
            target: package_target,
            path: None,
            diagnostics: vec![String::from("no manifest test targets were discovered")],
        });
    }
    let summary = EvidenceSummary {
        passing: evidence
            .iter()
            .filter(|item| item.status == "passing")
            .count(),
        failing: evidence
            .iter()
            .filter(|item| item.status == "failing")
            .count(),
        missing: evidence
            .iter()
            .filter(|item| item.status == "missing")
            .count(),
        provided: evidence
            .iter()
            .filter(|item| matches!(item.status, "passing" | "failing" | "provided"))
            .count(),
    };
    let validation_status = if summary.failing > 0 {
        "failing"
    } else if summary.missing > 0 {
        "missing"
    } else {
        "passing"
    };

    Ok(EvidenceReport {
        schema_version: "axiom.evidence.v0",
        ok: summary.failing == 0,
        command: "evidence",
        project: project.display().to_string(),
        package: package_name,
        validation_status,
        summary,
        evidence,
    })
}

fn evidence_tests_discoverable(
    project: &Path,
    manifest: &axiomc::manifest::Manifest,
) -> Result<bool, Diagnostic> {
    if !manifest.tests.is_empty() {
        return Ok(true);
    }
    Ok(axiom_files(project)?.iter().any(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("_test.ax") || name.ends_with("_bench.ax"))
    }))
}

fn repair_allowed_files(path: &Path) -> Result<Vec<String>, Diagnostic> {
    Ok(axiom_files(path)?
        .into_iter()
        .map(|path| path.display().to_string())
        .collect())
}

fn package_node_for_path(path: &Path) -> String {
    let name = load_manifest(path)
        .ok()
        .and_then(|manifest| manifest.package.map(|package| package.name))
        .unwrap_or_else(|| {
            path.file_stem()
                .or_else(|| path.file_name())
                .and_then(|name| name.to_str())
                .unwrap_or("package")
                .to_string()
        });
    format!("axiom://package/{}", repair_component(&name))
}

fn package_node_component(path: &str) -> String {
    format!("axiom://package/{}", repair_component(path))
}

fn repair_component(value: &str) -> String {
    normalized_id_component(value, "node")
}

fn evidence_type_for_test_kind(kind: TestKind) -> &'static str {
    match kind {
        TestKind::Unit | TestKind::Table => "unit_test",
        TestKind::Property => "property_test",
        TestKind::Snapshot => "golden_output",
        TestKind::Benchmark => "benchmark_baseline",
    }
}

fn evidence_id_component(value: &str) -> String {
    normalized_id_component(value, "unnamed")
}

fn normalized_id_component(value: &str, fallback: &str) -> String {
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
        String::from(fallback)
    } else {
        trimmed.to_string()
    }
}

#[derive(Debug, Clone, Serialize)]
struct SemanticGraphNode {
    id: String,
    kind: &'static str,
    name: String,
    span: SymbolSpan,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    severity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    assertion: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    inputs: Vec<SemanticGraphInput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    effects: Vec<SemanticGraphEffect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SemanticGraphInput {
    name: String,
    ty: String,
}

#[derive(Debug, Clone, Serialize)]
struct SemanticGraphEffect {
    kind: String,
    target: String,
}

#[derive(Debug, Clone, Serialize)]
struct SemanticGraphEdge {
    from: String,
    kind: &'static str,
    to: String,
}

#[derive(Debug, Clone, Serialize)]
struct InspectEffectsReport {
    schema_version: &'static str,
    ok: bool,
    command: &'static str,
    project: String,
    effects: Vec<EffectNode>,
}

#[derive(Debug, Clone, Serialize)]
struct EffectNode {
    id: String,
    kind: &'static str,
    resource: String,
    operation: &'static str,
    capability_gate: &'static str,
    source_span: SymbolSpan,
    policy: EffectPolicy,
}

#[derive(Debug, Clone, Serialize)]
struct EffectPolicy {
    host_allowed: bool,
    port_allowed: bool,
}

const INSPECT_SCHEMA_PATH: &str = "stage1/schemas/axiom-inspect-v0.schema.json";

fn inspect_semantic_graph(
    path: &Path,
) -> Result<(Vec<SemanticGraphNode>, Vec<SemanticGraphEdge>), Diagnostic> {
    let files = axiom_files(path)?;
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for file in files {
        let source = fs::read_to_string(&file).map_err(|err| {
            Diagnostic::new(
                "inspect",
                format!("failed to read {}: {err}", file.display()),
            )
            .with_path(file.display().to_string())
        })?;
        let program = parse_program(&source, &file)?;
        for axiom in &program.axioms {
            nodes.push(SemanticGraphNode {
                id: semantic_node_id("axiom", &axiom.name),
                kind: "axiom",
                name: axiom.name.clone(),
                span: symbol_span(&file, axiom.line, axiom.column),
                scope: axiom.scope.clone(),
                severity: axiom.severity.clone(),
                description: axiom.description.clone(),
                assertion: axiom.assertion.clone(),
                inputs: Vec::new(),
                effects: Vec::new(),
                evidence: Vec::new(),
            });
        }
        for evidence in &program.evidence {
            nodes.push(SemanticGraphNode {
                id: semantic_node_id("evidence", &evidence.name),
                kind: "evidence",
                name: evidence.name.clone(),
                span: symbol_span(&file, evidence.line, evidence.column),
                scope: None,
                severity: None,
                description: evidence.description.clone(),
                assertion: None,
                inputs: Vec::new(),
                effects: Vec::new(),
                evidence: Vec::new(),
            });
        }
        for capability in &program.semantic_capabilities {
            let capability_id = semantic_node_id("capability", &capability.name);
            nodes.push(SemanticGraphNode {
                id: capability_id.clone(),
                kind: "capability",
                name: capability.name.clone(),
                span: symbol_span(&file, capability.line, capability.column),
                scope: None,
                severity: None,
                description: None,
                assertion: None,
                inputs: capability
                    .inputs
                    .iter()
                    .map(|input| SemanticGraphInput {
                        name: input.name.clone(),
                        ty: render_type(&input.ty),
                    })
                    .collect(),
                effects: capability
                    .effects
                    .iter()
                    .map(|effect| SemanticGraphEffect {
                        kind: effect.kind.clone(),
                        target: effect.target.clone(),
                    })
                    .collect(),
                evidence: capability
                    .evidence
                    .iter()
                    .map(|reference| reference.name.clone())
                    .collect(),
            });
            for reference in &capability.preserves {
                edges.push(SemanticGraphEdge {
                    from: capability_id.clone(),
                    kind: "preserves",
                    to: semantic_node_id("axiom", &reference.name),
                });
            }
            for reference in &capability.evidence {
                edges.push(SemanticGraphEdge {
                    from: capability_id.clone(),
                    kind: "requires_evidence",
                    to: semantic_node_id("evidence", &reference.name),
                });
            }
        }
    }
    nodes.sort_by(|left, right| left.id.cmp(&right.id));
    edges.sort_by(|left, right| {
        left.from
            .cmp(&right.from)
            .then_with(|| left.kind.cmp(right.kind))
            .then_with(|| left.to.cmp(&right.to))
    });
    Ok((nodes, edges))
}

fn semantic_node_id(kind: &str, name: &str) -> String {
    format!("axiom://semantic/{kind}/{}", semantic_id_segment(name))
}

fn semantic_id_segment(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '~') {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        String::from("node")
    } else {
        trimmed.to_string()
    }
}

fn inspect_symbols(path: &Path) -> Result<InspectSymbolsReport, Diagnostic> {
    let files = axiom_files(path)?;
    let mut symbols = Vec::new();
    for file in files {
        let source = fs::read_to_string(&file).map_err(|err| {
            Diagnostic::new(
                "inspect",
                format!("failed to read {}: {err}", file.display()),
            )
            .with_path(file.display().to_string())
        })?;
        let program = parse_program(&source, &file)?;
        let imports = program
            .imports
            .iter()
            .map(|import| import.path.clone())
            .collect::<Vec<_>>();
        for decl in &program.consts {
            if decl.visibility.is_public() {
                symbols.push(InspectedSymbol {
                    name: decl.name.clone(),
                    kind: "const",
                    signature: format!("pub const {}: {}", decl.name, render_type(&decl.ty)),
                    span: symbol_span(&file, decl.line, decl.column),
                    imports: imports.clone(),
                    capabilities: capabilities_in_expr(&decl.expr),
                });
            }
        }
        for decl in &program.type_aliases {
            if decl.visibility.is_public() {
                symbols.push(InspectedSymbol {
                    name: decl.name.clone(),
                    kind: "type",
                    signature: format!("pub type {} = {}", decl.name, render_type(&decl.ty)),
                    span: symbol_span(&file, decl.line, decl.column),
                    imports: imports.clone(),
                    capabilities: Vec::new(),
                });
            }
        }
        for decl in &program.structs {
            if decl.visibility.is_public() {
                let fields = decl
                    .fields
                    .iter()
                    .map(|field| format!("{}: {}", field.name, render_type(&field.ty)))
                    .collect::<Vec<_>>()
                    .join(", ");
                symbols.push(InspectedSymbol {
                    name: decl.name.clone(),
                    kind: "struct",
                    signature: format!("pub struct {} {{ {} }}", decl.name, fields),
                    span: symbol_span(&file, decl.line, decl.column),
                    imports: imports.clone(),
                    capabilities: Vec::new(),
                });
            }
        }
        for decl in &program.enums {
            if decl.visibility.is_public() {
                symbols.push(InspectedSymbol {
                    name: decl.name.clone(),
                    kind: "enum",
                    signature: format!("pub enum {}", decl.name),
                    span: symbol_span(&file, decl.line, decl.column),
                    imports: imports.clone(),
                    capabilities: Vec::new(),
                });
            }
        }
        for function in &program.functions {
            if function.visibility.is_public() {
                symbols.push(InspectedSymbol {
                    name: function.source_name.clone(),
                    kind: "function",
                    signature: function_signature(function),
                    span: symbol_span(&file, function.line, function.column),
                    imports: imports.clone(),
                    capabilities: capabilities_in_stmts(&function.body),
                });
            }
        }
    }
    symbols.sort_by(|left, right| {
        left.span
            .path
            .cmp(&right.span.path)
            .then_with(|| left.span.line.cmp(&right.span.line))
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(InspectSymbolsReport {
        schema_version: json_contract::JSON_SCHEMA_VERSION,
        schema: INSPECT_SCHEMA_PATH,
        ok: true,
        command: "inspect symbols",
        project: path.display().to_string(),
        symbols,
    })
}

fn inspect_effects(path: &Path) -> Result<InspectEffectsReport, Diagnostic> {
    let files = axiom_files(path)?;
    let mut effects = Vec::new();
    for file in files {
        let source = fs::read_to_string(&file).map_err(|err| {
            Diagnostic::new(
                "inspect",
                format!("failed to read {}: {err}", file.display()),
            )
            .with_path(file.display().to_string())
        })?;
        let program = parse_program(&source, &file)?;
        for decl in &program.consts {
            collect_effects_in_expr(&decl.expr, &file, &mut effects);
        }
        collect_effects_in_stmts(&program.stmts, &file, &mut effects);
        for function in &program.functions {
            collect_effects_in_stmts(&function.body, &file, &mut effects);
        }
    }
    for (index, effect) in effects.iter_mut().enumerate() {
        effect.id = format!(
            "axiom://package/{}/effect/{}-{}",
            effect_id_component(&effect.source_span.path),
            effect.kind.replace('.', "-"),
            index + 1
        );
    }
    effects.sort_by(|left, right| {
        left.source_span
            .path
            .cmp(&right.source_span.path)
            .then_with(|| left.source_span.line.cmp(&right.source_span.line))
            .then_with(|| left.kind.cmp(right.kind))
    });
    Ok(InspectEffectsReport {
        schema_version: "axiom.effects.v0",
        ok: true,
        command: "inspect effects",
        project: path.display().to_string(),
        effects,
    })
}

fn collect_effects_in_stmts(
    stmts: &[axiomc::syntax::Stmt],
    file: &Path,
    effects: &mut Vec<EffectNode>,
) {
    use axiomc::syntax::Stmt;
    for stmt in stmts {
        match stmt {
            Stmt::Let { expr, .. }
            | Stmt::Print { expr, .. }
            | Stmt::Panic { expr, .. }
            | Stmt::Defer { expr, .. }
            | Stmt::Return { expr, .. } => collect_effects_in_expr(expr, file, effects),
            Stmt::Assign { target, expr, .. } => {
                collect_effects_in_expr(target, file, effects);
                collect_effects_in_expr(expr, file, effects);
            }
            Stmt::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                collect_effects_in_expr(cond, file, effects);
                collect_effects_in_stmts(then_block, file, effects);
                for block in else_block.iter().flatten() {
                    collect_effects_in_stmts(std::slice::from_ref(block), file, effects);
                }
            }
            Stmt::IfLet {
                expr,
                then_block,
                else_block,
                ..
            } => {
                collect_effects_in_expr(expr, file, effects);
                collect_effects_in_stmts(then_block, file, effects);
                for block in else_block.iter().flatten() {
                    collect_effects_in_stmts(std::slice::from_ref(block), file, effects);
                }
            }
            Stmt::While { cond, body, .. } => {
                collect_effects_in_expr(cond, file, effects);
                collect_effects_in_stmts(body, file, effects);
            }
            Stmt::Match { expr, arms, .. } => {
                collect_effects_in_expr(expr, file, effects);
                for arm in arms {
                    collect_effects_in_stmts(&arm.body, file, effects);
                }
            }
        }
    }
}

fn collect_effects_in_expr(
    expr: &axiomc::syntax::Expr,
    file: &Path,
    effects: &mut Vec<EffectNode>,
) {
    use axiomc::syntax::Expr;
    match expr {
        Expr::Call {
            name,
            args,
            line,
            column,
            ..
        } => {
            if let Some((kind, operation, gate)) = effect_for_call(name) {
                effects.push(EffectNode {
                    id: String::new(),
                    kind,
                    resource: effect_resource(name, args),
                    operation,
                    capability_gate: gate,
                    source_span: symbol_span(file, *line, *column),
                    policy: EffectPolicy {
                        host_allowed: true,
                        port_allowed: true,
                    },
                });
            }
            for arg in args {
                collect_effects_in_expr(arg, file, effects);
            }
        }
        Expr::MethodCall { base, args, .. } => {
            collect_effects_in_expr(base, file, effects);
            for arg in args {
                collect_effects_in_expr(arg, file, effects);
            }
        }
        Expr::BinaryAdd { lhs, rhs, .. } | Expr::BinaryCompare { lhs, rhs, .. } => {
            collect_effects_in_expr(lhs, file, effects);
            collect_effects_in_expr(rhs, file, effects);
        }
        Expr::Cast { expr, .. }
        | Expr::Try { expr, .. }
        | Expr::Await { expr, .. }
        | Expr::MutBorrow { expr, .. }
        | Expr::Deref { expr, .. } => collect_effects_in_expr(expr, file, effects),
        Expr::StructLiteral { fields, .. } => {
            for field in fields {
                collect_effects_in_expr(&field.expr, file, effects);
            }
        }
        Expr::FieldAccess { base, .. } | Expr::TupleIndex { base, .. } => {
            collect_effects_in_expr(base, file, effects);
        }
        Expr::Slice {
            base, start, end, ..
        } => {
            collect_effects_in_expr(base, file, effects);
            if let Some(start) = start {
                collect_effects_in_expr(start, file, effects);
            }
            if let Some(end) = end {
                collect_effects_in_expr(end, file, effects);
            }
        }
        Expr::TupleLiteral { elements, .. } | Expr::ArrayLiteral { elements, .. } => {
            for element in elements {
                collect_effects_in_expr(element, file, effects);
            }
        }
        Expr::MapLiteral { entries, .. } => {
            for entry in entries {
                collect_effects_in_expr(&entry.key, file, effects);
                collect_effects_in_expr(&entry.value, file, effects);
            }
        }
        Expr::Index { base, index, .. } => {
            collect_effects_in_expr(base, file, effects);
            collect_effects_in_expr(index, file, effects);
        }
        Expr::Closure { body, .. } => collect_effects_in_expr(body, file, effects),
        Expr::Match { expr, arms, .. } => {
            collect_effects_in_expr(expr, file, effects);
            for arm in arms {
                collect_effects_in_expr(&arm.expr, file, effects);
            }
        }
        Expr::Literal(_) | Expr::VarRef { .. } => {}
    }
}

fn effect_for_call(name: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match name {
        "clock_now_ms" | "clock_elapsed_ms" | "now_ms" | "now" | "elapsed_ms" => {
            Some(("clock.now", "read", "clock"))
        }
        "clock_sleep_ms" | "sleep" => Some(("clock.sleep", "sleep", "clock")),
        "env_get" | "get_env" => Some(("env.read", "read", "env")),
        "fs_read" | "read_file" => Some(("fs.read", "read", "fs")),
        "fs_write" | "fs_create" | "fs_append" | "fs_mkdir" | "fs_mkdir_all" | "fs_remove_file"
        | "fs_remove_dir" | "fs_replace" | "write_file" | "create_file" | "append_file"
        | "mkdir" | "mkdir_all" | "remove_file" | "remove_dir" | "replace_file" => {
            Some(("fs.write", "write", "fs:write"))
        }
        "net_resolve" => Some(("network.dns.resolve", "resolve", "net")),
        "http_get" => Some(("network.http.get", "get", "net")),
        "http_serve_once"
        | "http_serve_route"
        | "net_tcp_listen_loopback_once"
        | "tcp_listen_loopback_once" => Some(("network.tcp.bind", "bind", "net")),
        "net_tcp_dial" | "tcp_dial" => Some(("network.tcp.connect", "connect", "net")),
        "net_udp_bind_loopback_once"
        | "udp_bind_loopback_once"
        | "net_udp_send_recv"
        | "udp_send_recv" => Some(("network.udp.send", "send", "net")),
        "process_status" => Some(("process.status", "read", "process")),
        "crypto_sha256" | "verify_sha256" | "verify_sha512" => {
            Some(("crypto.hash", "hash", "crypto"))
        }
        "crypto_hmac_sha256"
        | "crypto_hmac_sha512"
        | "hmac_sha256"
        | "hmac_sha512"
        | "crypto_constant_time_eq"
        | "crypto_constant_time_eq_u8"
        | "constant_time_eq"
        | "constant_time_eq_u8" => Some(("crypto.mac", "authenticate", "crypto")),
        _ => None,
    }
}

fn effect_resource(name: &str, args: &[axiomc::syntax::Expr]) -> String {
    match name {
        "clock_now_ms" | "clock_elapsed_ms" | "clock_sleep_ms" => String::from("system_clock"),
        "process_status" => String::from("process"),
        "crypto_sha256"
        | "crypto_hmac_sha256"
        | "crypto_hmac_sha512"
        | "crypto_constant_time_eq"
        | "crypto_constant_time_eq_u8"
        | "hmac_sha256"
        | "hmac_sha512"
        | "verify_sha256"
        | "verify_sha512"
        | "constant_time_eq"
        | "constant_time_eq_u8" => String::from("runtime_crypto"),
        _ => args
            .first()
            .map(effect_literal_resource)
            .unwrap_or_else(|| String::from("*")),
    }
}

fn effect_literal_resource(expr: &axiomc::syntax::Expr) -> String {
    match expr {
        axiomc::syntax::Expr::Literal(axiomc::syntax::Literal::String(value)) => value.clone(),
        axiomc::syntax::Expr::Literal(axiomc::syntax::Literal::Int(value)) => value.to_string(),
        axiomc::syntax::Expr::Literal(axiomc::syntax::Literal::Bool(value)) => value.to_string(),
        axiomc::syntax::Expr::Literal(axiomc::syntax::Literal::Numeric { raw, .. }) => raw.clone(),
        axiomc::syntax::Expr::VarRef { name, .. } => format!("${name}"),
        _ => String::from("<dynamic>"),
    }
}

fn effect_id_component(value: &str) -> String {
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
        String::from("source")
    } else {
        trimmed.to_string()
    }
}

const OPENAPI_TARGET_ID: &str = "axiom://target/stage1-openapi-v0";
const OPENAPI_ARTIFACT_KIND: &str = "openapi_spec";
const OPENAPI_SCHEMA_VERSION: &str = "axiom.generate.openapi.v0";
const OPENAPI_SPEC_VERSION: &str = "3.1.0";
const OPENAPI_RUNTIME_RESPONSE_STATUS: u16 = 200;
const OPENAPI_RUNTIME_CONTENT_TYPE: &str = "text/plain; charset=utf-8";

#[derive(Debug, Clone, Serialize)]
struct GenerateOpenApiReport {
    schema_version: &'static str,
    ok: bool,
    command: &'static str,
    project: String,
    target_contract: TargetContract,
    artifact: GeneratedArtifact,
    routes: Vec<OpenApiRouteReport>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize)]
struct TargetContract {
    id: &'static str,
    #[serde(rename = "class")]
    target_class: &'static str,
    description: &'static str,
    status: &'static str,
    input_node_kinds: Vec<&'static str>,
    supported_effect_kinds: Vec<&'static str>,
    supported_type_features: Vec<&'static str>,
    artifact_outputs: Vec<GeneratedArtifact>,
    evidence_requirements: Vec<&'static str>,
    unsupported_feature_diagnostics: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
struct GeneratedArtifact {
    id: String,
    kind: &'static str,
    path: String,
    generated_from: Vec<String>,
    status: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct OpenApiRouteReport {
    path: String,
    method: &'static str,
    operation_id: String,
    response_status: u16,
    content_type: String,
    effect_kind: &'static str,
    capability_gate: &'static str,
    source_span: SymbolSpan,
}

#[derive(Debug, Clone)]
struct OpenApiRoute {
    path: String,
    status: u16,
    content_type: String,
    source_span: SymbolSpan,
}

#[derive(Debug, Default)]
struct OpenApiRouteContext {
    route_var_scopes: Vec<BTreeMap<String, OpenApiRoute>>,
    functions: BTreeMap<String, Vec<axiomc::syntax::Stmt>>,
    active_functions: BTreeSet<String>,
}

impl OpenApiRouteContext {
    fn new(functions: &[axiomc::syntax::Function]) -> Self {
        Self {
            route_var_scopes: vec![BTreeMap::new()],
            functions: functions
                .iter()
                .map(|function| (function.name.clone(), function.body.clone()))
                .collect(),
            active_functions: BTreeSet::new(),
        }
    }

    fn push_scope(&mut self) {
        self.route_var_scopes.push(BTreeMap::new());
    }

    fn pop_scope(&mut self) {
        if self.route_var_scopes.len() > 1 {
            self.route_var_scopes.pop();
        }
    }

    fn insert_route_var(&mut self, name: String, route: OpenApiRoute) {
        if let Some(scope) = self.route_var_scopes.last_mut() {
            scope.insert(name, route);
        }
    }

    fn route_var(&self, name: &str) -> Option<&OpenApiRoute> {
        self.route_var_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
    }

    fn function_body(&self, name: &str) -> Option<Vec<axiomc::syntax::Stmt>> {
        self.functions.get(name).cloned()
    }

    fn enter_function(&mut self, name: &str) -> bool {
        self.active_functions.insert(name.to_string())
    }

    fn exit_function(&mut self, name: &str) {
        self.active_functions.remove(name);
    }
}

fn generate_openapi(project: &Path, out: &Path) -> Result<GenerateOpenApiReport, Diagnostic> {
    let manifest = load_manifest(project)?;
    let package = manifest.package.as_ref().ok_or_else(|| {
        Diagnostic::new(
            "openapi",
            "OpenAPI generation requires a package manifest with [package].",
        )
    })?;
    let output_path = if out.is_absolute() {
        out.to_path_buf()
    } else {
        project.join(out)
    };
    let mut routes = collect_openapi_routes(project)?;
    sort_dedup_openapi_routes(&mut routes);
    let document = render_openapi_document(&package.name, &package.version, &routes);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            Diagnostic::new(
                "openapi",
                format!("failed to create {}: {err}", parent.display()),
            )
        })?;
    }
    let body = serde_json::to_string_pretty(&document)
        .map_err(|err| Diagnostic::new("json", format!("failed to serialize OpenAPI: {err}")))?;
    fs::write(&output_path, format!("{body}\n")).map_err(|err| {
        Diagnostic::new(
            "openapi",
            format!("failed to write {}: {err}", output_path.display()),
        )
    })?;
    let package_id = package_node_for_path(project);
    let artifact = openapi_artifact(project, &output_path, &package_id, "generated");
    let diagnostics =
        if routes.is_empty() {
            vec![Diagnostic::new(
            "openapi",
            "no HTTP-serving routes discovered for OpenAPI generation",
        )
        .with_help(
            "The generated document is valid and intentionally contains an empty paths object.",
        )
        .normalized_for_json()]
        } else {
            Vec::new()
        };
    Ok(GenerateOpenApiReport {
        schema_version: OPENAPI_SCHEMA_VERSION,
        ok: true,
        command: "generate openapi",
        project: project.display().to_string(),
        target_contract: openapi_target_contract(artifact.clone()),
        artifact,
        routes: routes.iter().map(openapi_route_report).collect(),
        diagnostics,
    })
}

fn openapi_target_contract(artifact: GeneratedArtifact) -> TargetContract {
    TargetContract {
        id: OPENAPI_TARGET_ID,
        target_class: OPENAPI_ARTIFACT_KIND,
        description: "Stage 1 OpenAPI generator for HTTP-serving semantic routes.",
        status: "experimental",
        input_node_kinds: vec![
            "Package",
            "Module",
            "Function",
            "Capability",
            "Effect",
            "Type",
        ],
        supported_effect_kinds: vec!["network.http.get", "network.tcp.bind"],
        supported_type_features: vec!["aggregate.struct", "aggregate.enum"],
        artifact_outputs: vec![artifact],
        evidence_requirements: vec!["unit_test", "fixture"],
        unsupported_feature_diagnostics: Vec::new(),
    }
}

fn openapi_artifact(
    project: &Path,
    output_path: &Path,
    package_id: &str,
    status: &'static str,
) -> GeneratedArtifact {
    GeneratedArtifact {
        id: format!("{package_id}/artifact/openapi-spec"),
        kind: OPENAPI_ARTIFACT_KIND,
        path: project_relative_path(project, output_path),
        generated_from: vec![package_id.to_string()],
        status,
    }
}

fn project_relative_path(project: &Path, path: &Path) -> String {
    if let Ok(stripped) = path.strip_prefix(project) {
        return stripped.display().to_string();
    }
    if !project.is_absolute() {
        if let Ok(cwd) = std::env::current_dir() {
            let absolute_project = cwd.join(project);
            if let Ok(stripped) = path.strip_prefix(&absolute_project) {
                return stripped.display().to_string();
            }
        }
    }
    path.display().to_string()
}

fn collect_openapi_routes(project: &Path) -> Result<Vec<OpenApiRoute>, Diagnostic> {
    let files = axiom_files(project)?;
    let mut routes = Vec::new();
    for file in files {
        let source = fs::read_to_string(&file).map_err(|err| {
            Diagnostic::new(
                "openapi",
                format!("failed to read {}: {err}", file.display()),
            )
            .with_path(file.display().to_string())
        })?;
        let program = parse_program(&source, &file)?;
        let http_imported = program
            .imports
            .iter()
            .any(|import| import.path == "std/http.ax");
        let mut top_level = OpenApiRouteContext::new(&program.functions);
        for decl in &program.consts {
            collect_openapi_served_routes_in_expr(
                &decl.expr,
                &file,
                http_imported,
                &mut top_level,
                &mut routes,
            );
        }
        collect_openapi_routes_in_stmts(
            &program.stmts,
            &file,
            http_imported,
            &mut top_level,
            &mut routes,
        );
    }
    Ok(routes)
}

fn collect_openapi_routes_in_stmts(
    stmts: &[axiomc::syntax::Stmt],
    file: &Path,
    http_imported: bool,
    context: &mut OpenApiRouteContext,
    routes: &mut Vec<OpenApiRoute>,
) {
    use axiomc::syntax::Stmt;
    for stmt in stmts {
        match stmt {
            Stmt::Let { name, expr, .. } => {
                collect_openapi_served_routes_in_expr(expr, file, http_imported, context, routes);
                if let Some(route) = openapi_route_from_expr(expr, file, http_imported, context) {
                    context.insert_route_var(name.clone(), route);
                }
            }
            Stmt::Print { expr, .. }
            | Stmt::Panic { expr, .. }
            | Stmt::Defer { expr, .. }
            | Stmt::Return { expr, .. } => {
                collect_openapi_served_routes_in_expr(expr, file, http_imported, context, routes)
            }
            Stmt::Assign { target, expr, .. } => {
                collect_openapi_served_routes_in_expr(target, file, http_imported, context, routes);
                collect_openapi_served_routes_in_expr(expr, file, http_imported, context, routes);
            }
            Stmt::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                collect_openapi_served_routes_in_expr(cond, file, http_imported, context, routes);
                collect_openapi_routes_in_block(then_block, file, http_imported, context, routes);
                if let Some(else_block) = else_block {
                    collect_openapi_routes_in_block(
                        else_block,
                        file,
                        http_imported,
                        context,
                        routes,
                    );
                }
            }
            Stmt::IfLet {
                expr,
                then_block,
                else_block,
                ..
            } => {
                collect_openapi_served_routes_in_expr(expr, file, http_imported, context, routes);
                collect_openapi_routes_in_block(then_block, file, http_imported, context, routes);
                if let Some(else_block) = else_block {
                    collect_openapi_routes_in_block(
                        else_block,
                        file,
                        http_imported,
                        context,
                        routes,
                    );
                }
            }
            Stmt::While { cond, body, .. } => {
                collect_openapi_served_routes_in_expr(cond, file, http_imported, context, routes);
                collect_openapi_routes_in_block(body, file, http_imported, context, routes);
            }
            Stmt::Match { expr, arms, .. } => {
                collect_openapi_served_routes_in_expr(expr, file, http_imported, context, routes);
                for arm in arms {
                    collect_openapi_routes_in_block(
                        &arm.body,
                        file,
                        http_imported,
                        context,
                        routes,
                    );
                }
            }
        }
    }
}

fn collect_openapi_routes_in_block(
    stmts: &[axiomc::syntax::Stmt],
    file: &Path,
    http_imported: bool,
    context: &mut OpenApiRouteContext,
    routes: &mut Vec<OpenApiRoute>,
) {
    context.push_scope();
    collect_openapi_routes_in_stmts(stmts, file, http_imported, context, routes);
    context.pop_scope();
}

fn collect_openapi_routes_in_called_function(
    name: &str,
    file: &Path,
    http_imported: bool,
    context: &mut OpenApiRouteContext,
    routes: &mut Vec<OpenApiRoute>,
) {
    let Some(body) = context.function_body(name) else {
        return;
    };
    if !context.enter_function(name) {
        return;
    }
    collect_openapi_routes_in_block(&body, file, http_imported, context, routes);
    context.exit_function(name);
}

fn collect_openapi_served_routes_in_expr(
    expr: &axiomc::syntax::Expr,
    file: &Path,
    http_imported: bool,
    context: &mut OpenApiRouteContext,
    routes: &mut Vec<OpenApiRoute>,
) {
    use axiomc::syntax::Expr;
    if let Some(route) = openapi_served_route_from_expr(expr, file, http_imported, context) {
        routes.push(route);
    }
    match expr {
        Expr::Call { args, .. } => {
            for arg in args {
                collect_openapi_served_routes_in_expr(arg, file, http_imported, context, routes);
            }
            if let Expr::Call { name, .. } = expr {
                collect_openapi_routes_in_called_function(
                    name,
                    file,
                    http_imported,
                    context,
                    routes,
                );
            }
        }
        Expr::MethodCall { base, args, .. } => {
            collect_openapi_served_routes_in_expr(base, file, http_imported, context, routes);
            for arg in args {
                collect_openapi_served_routes_in_expr(arg, file, http_imported, context, routes);
            }
        }
        Expr::BinaryAdd { lhs, rhs, .. } | Expr::BinaryCompare { lhs, rhs, .. } => {
            collect_openapi_served_routes_in_expr(lhs, file, http_imported, context, routes);
            collect_openapi_served_routes_in_expr(rhs, file, http_imported, context, routes);
        }
        Expr::Cast { expr, .. }
        | Expr::Try { expr, .. }
        | Expr::Await { expr, .. }
        | Expr::MutBorrow { expr, .. }
        | Expr::Deref { expr, .. } => {
            collect_openapi_served_routes_in_expr(expr, file, http_imported, context, routes)
        }
        Expr::StructLiteral { fields, .. } => {
            for field in fields {
                collect_openapi_served_routes_in_expr(
                    &field.expr,
                    file,
                    http_imported,
                    context,
                    routes,
                );
            }
        }
        Expr::FieldAccess { base, .. } | Expr::TupleIndex { base, .. } => {
            collect_openapi_served_routes_in_expr(base, file, http_imported, context, routes);
        }
        Expr::Slice {
            base, start, end, ..
        } => {
            collect_openapi_served_routes_in_expr(base, file, http_imported, context, routes);
            if let Some(start) = start {
                collect_openapi_served_routes_in_expr(start, file, http_imported, context, routes);
            }
            if let Some(end) = end {
                collect_openapi_served_routes_in_expr(end, file, http_imported, context, routes);
            }
        }
        Expr::TupleLiteral { elements, .. } | Expr::ArrayLiteral { elements, .. } => {
            for element in elements {
                collect_openapi_served_routes_in_expr(
                    element,
                    file,
                    http_imported,
                    context,
                    routes,
                );
            }
        }
        Expr::MapLiteral { entries, .. } => {
            for entry in entries {
                collect_openapi_served_routes_in_expr(
                    &entry.key,
                    file,
                    http_imported,
                    context,
                    routes,
                );
                collect_openapi_served_routes_in_expr(
                    &entry.value,
                    file,
                    http_imported,
                    context,
                    routes,
                );
            }
        }
        Expr::Index { base, index, .. } => {
            collect_openapi_served_routes_in_expr(base, file, http_imported, context, routes);
            collect_openapi_served_routes_in_expr(index, file, http_imported, context, routes);
        }
        Expr::Closure { body, .. } => {
            collect_openapi_served_routes_in_expr(body, file, http_imported, context, routes)
        }
        Expr::Match { expr, arms, .. } => {
            collect_openapi_served_routes_in_expr(expr, file, http_imported, context, routes);
            for arm in arms {
                collect_openapi_served_routes_in_expr(
                    &arm.expr,
                    file,
                    http_imported,
                    context,
                    routes,
                );
            }
        }
        Expr::Literal(_) | Expr::VarRef { .. } => {}
    }
}

fn openapi_served_route_from_expr(
    expr: &axiomc::syntax::Expr,
    file: &Path,
    http_imported: bool,
    context: &OpenApiRouteContext,
) -> Option<OpenApiRoute> {
    use axiomc::syntax::Expr;
    match expr {
        Expr::Call {
            name,
            args,
            line,
            column,
            ..
        } if name == "http_serve_route" => {
            let path = args.get(1).and_then(literal_string)?;
            Some(openapi_route(file, *line, *column, path))
        }
        Expr::Call {
            name,
            args,
            line,
            column,
            ..
        } if http_imported && name == "serve" => {
            let mut route = args
                .get(1)
                .and_then(|expr| openapi_route_from_expr(expr, file, http_imported, context))?;
            route.source_span = symbol_span(file, *line, *column);
            Some(route)
        }
        _ => None,
    }
}

fn openapi_route_from_expr(
    expr: &axiomc::syntax::Expr,
    file: &Path,
    http_imported: bool,
    context: &OpenApiRouteContext,
) -> Option<OpenApiRoute> {
    use axiomc::syntax::Expr;
    match expr {
        Expr::Call {
            name,
            args,
            line,
            column,
            ..
        } if http_imported && name == "route" => {
            let path = args.first().and_then(literal_string)?;
            Some(openapi_route(file, *line, *column, path))
        }
        Expr::Call {
            name,
            args,
            line,
            column,
            ..
        } if http_imported && name == "route_response" => {
            let path = args.first().and_then(literal_string)?;
            Some(openapi_route(file, *line, *column, path))
        }
        Expr::StructLiteral {
            name,
            fields,
            line,
            column,
            ..
        } if http_imported && name == "HttpRoute" => {
            let path = struct_field_expr(fields, "path").and_then(literal_string)?;
            Some(openapi_route(file, *line, *column, path))
        }
        Expr::VarRef { name, .. } => context.route_var(name).cloned(),
        _ => None,
    }
}

fn openapi_route(file: &Path, line: usize, column: usize, path: &str) -> OpenApiRoute {
    OpenApiRoute {
        path: normalize_openapi_path(path),
        status: OPENAPI_RUNTIME_RESPONSE_STATUS,
        content_type: String::from(OPENAPI_RUNTIME_CONTENT_TYPE),
        source_span: symbol_span(file, line, column),
    }
}

fn struct_field_expr<'a>(
    fields: &'a [axiomc::syntax::StructFieldValue],
    name: &str,
) -> Option<&'a axiomc::syntax::Expr> {
    fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| &field.expr)
}

fn literal_string(expr: &axiomc::syntax::Expr) -> Option<&str> {
    match expr {
        axiomc::syntax::Expr::Literal(axiomc::syntax::Literal::String(value)) => Some(value),
        _ => None,
    }
}

fn normalize_openapi_path(path: &str) -> String {
    if path.is_empty() {
        String::from("/")
    } else if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

fn sort_dedup_openapi_routes(routes: &mut Vec<OpenApiRoute>) {
    routes.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.status.cmp(&right.status))
            .then_with(|| left.content_type.cmp(&right.content_type))
            .then_with(|| left.source_span.path.cmp(&right.source_span.path))
            .then_with(|| left.source_span.line.cmp(&right.source_span.line))
            .then_with(|| left.source_span.column.cmp(&right.source_span.column))
    });
    routes.dedup_by(|left, right| {
        left.path == right.path
            && left.status == right.status
            && left.content_type == right.content_type
    });
}

fn render_openapi_document(
    package_name: &str,
    package_version: &str,
    routes: &[OpenApiRoute],
) -> serde_json::Value {
    let mut grouped: BTreeMap<String, Vec<&OpenApiRoute>> = BTreeMap::new();
    for route in routes {
        grouped.entry(route.path.clone()).or_default().push(route);
    }
    let mut paths = serde_json::Map::new();
    for (path, path_routes) in grouped {
        let mut responses = serde_json::Map::new();
        for route in path_routes {
            let mut content = serde_json::Map::new();
            content.insert(
                route.content_type.clone(),
                serde_json::json!({
                    "schema": {
                        "type": "string"
                    }
                }),
            );
            responses.insert(
                route.status.to_string(),
                serde_json::json!({
                    "description": "Generated from an Axiom HTTP route.",
                    "content": content
                }),
            );
        }
        let operation_id = openapi_operation_id(&path);
        paths.insert(
            path.clone(),
            serde_json::json!({
                "get": {
                    "operationId": operation_id,
                    "summary": format!("GET {path}"),
                    "description": "Generated from Axiom HTTP-serving semantic routes.",
                    "responses": responses,
                    "x-axiom": {
                        "target_id": OPENAPI_TARGET_ID,
                        "effect_kind": "network.tcp.bind",
                        "capability_gate": "net"
                    }
                }
            }),
        );
    }
    serde_json::json!({
        "openapi": OPENAPI_SPEC_VERSION,
        "info": {
            "title": package_name,
            "version": package_version
        },
        "paths": paths,
        "components": {
            "schemas": {}
        }
    })
}

fn openapi_route_report(route: &OpenApiRoute) -> OpenApiRouteReport {
    OpenApiRouteReport {
        path: route.path.clone(),
        method: "get",
        operation_id: openapi_operation_id(&route.path),
        response_status: route.status,
        content_type: route.content_type.clone(),
        effect_kind: "network.tcp.bind",
        capability_gate: "net",
        source_span: route.source_span.clone(),
    }
}

fn openapi_operation_id(path: &str) -> String {
    let mut out = String::from("get");
    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        out.push('_');
        out.push_str(&normalized_id_component(segment, "path"));
    }
    out
}

const POLICY_TARGET_ID: &str = "axiom://target/stage1-policy-bundle-v0";
const POLICY_ARTIFACT_KIND: &str = "policy_bundle";
const POLICY_SCHEMA_VERSION: &str = "axiom.policy_bundle.v0";
const GENERATE_POLICY_SCHEMA_VERSION: &str = "axiom.generate.policy.v0";
const RUNBOOK_TARGET_ID: &str = "axiom://target/stage1-runbook-v0";
const RUNBOOK_ARTIFACT_KIND: &str = "runbook";
const GENERATE_RUNBOOK_SCHEMA_VERSION: &str = "axiom.generate.runbook.v0";

#[derive(Debug, Clone, Serialize)]
struct GeneratePolicyReport {
    schema_version: &'static str,
    ok: bool,
    command: &'static str,
    project: String,
    target_contract: TargetContract,
    artifact: GeneratedArtifact,
    allowed_effect_kinds: Vec<String>,
    observed_effects: Vec<PolicyObservedEffect>,
}

#[derive(Debug, Clone, Serialize)]
struct PolicyBundle {
    schema_version: &'static str,
    package: String,
    target_id: &'static str,
    generated_from: Vec<String>,
    capabilities: Vec<PolicyCapability>,
    allowed_effect_kinds: Vec<String>,
    allowed_effects: Vec<PolicyEffectAllowance>,
    observed_effects: Vec<PolicyObservedEffect>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct PolicyCapability {
    name: String,
    enabled: bool,
    description: &'static str,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    allowed: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    configured_root: Option<String>,
    #[serde(skip_serializing_if = "is_false_bool")]
    deny_by_default: bool,
    #[serde(skip_serializing_if = "is_false_bool")]
    unsafe_unrestricted: bool,
    #[serde(skip_serializing_if = "is_false_bool")]
    unsafe_opt_in: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    unsafe_rationale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rationale: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct PolicyEffectAllowance {
    kind: String,
    capability_gate: String,
}

#[derive(Debug, Clone, Serialize)]
struct PolicyObservedEffect {
    kind: String,
    resource: String,
    operation: &'static str,
    capability_gate: &'static str,
    source_span: SymbolSpan,
}

fn generate_policy(project: &Path, out: &Path) -> Result<GeneratePolicyReport, Diagnostic> {
    let manifest = load_manifest(project)?;
    let _package = manifest.package.as_ref().ok_or_else(|| {
        Diagnostic::new(
            "policy",
            "policy bundle generation requires a package manifest with [package].",
        )
    })?;
    let output_path = if out.is_absolute() {
        out.to_path_buf()
    } else {
        project.join(out)
    };
    let capabilities = project_capabilities(project)?;
    let allowed_effects = policy_allowed_effects(&capabilities);
    let allowed_effect_kinds = allowed_effects
        .iter()
        .map(|effect| effect.kind.clone())
        .collect::<Vec<_>>();
    let observed_effects = inspect_effects(project)?
        .effects
        .iter()
        .map(policy_observed_effect)
        .collect::<Vec<_>>();
    let package_id = package_node_for_path(project);
    let bundle = PolicyBundle {
        schema_version: POLICY_SCHEMA_VERSION,
        package: package_id.clone(),
        target_id: POLICY_TARGET_ID,
        generated_from: vec![package_id.clone()],
        capabilities: policy_capabilities(&capabilities),
        allowed_effect_kinds: allowed_effect_kinds.clone(),
        allowed_effects: allowed_effects.clone(),
        observed_effects: observed_effects.clone(),
    };
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            Diagnostic::new(
                "policy",
                format!("failed to create {}: {err}", parent.display()),
            )
        })?;
    }
    let body = serde_json::to_string_pretty(&bundle)
        .map_err(|err| Diagnostic::new("json", format!("failed to serialize policy: {err}")))?;
    fs::write(&output_path, format!("{body}\n")).map_err(|err| {
        Diagnostic::new(
            "policy",
            format!("failed to write {}: {err}", output_path.display()),
        )
    })?;
    let artifact = policy_artifact(project, &output_path, &package_id, "generated");
    Ok(GeneratePolicyReport {
        schema_version: GENERATE_POLICY_SCHEMA_VERSION,
        ok: true,
        command: "generate policy",
        project: project.display().to_string(),
        target_contract: policy_target_contract(artifact.clone()),
        artifact,
        allowed_effect_kinds,
        observed_effects,
    })
}

fn policy_target_contract(artifact: GeneratedArtifact) -> TargetContract {
    TargetContract {
        id: POLICY_TARGET_ID,
        target_class: POLICY_ARTIFACT_KIND,
        description: "Stage 1 policy bundle generator for manifest capabilities and effect allowlists.",
        status: "experimental",
        input_node_kinds: vec!["Package", "Capability", "Effect"],
        supported_effect_kinds: policy_known_effect_kinds(),
        supported_type_features: Vec::new(),
        artifact_outputs: vec![artifact],
        evidence_requirements: vec!["unit_test", "fixture"],
        unsupported_feature_diagnostics: Vec::new(),
    }
}

fn policy_artifact(
    project: &Path,
    output_path: &Path,
    package_id: &str,
    status: &'static str,
) -> GeneratedArtifact {
    GeneratedArtifact {
        id: format!("{package_id}/artifact/policy-bundle"),
        kind: POLICY_ARTIFACT_KIND,
        path: project_relative_path(project, output_path),
        generated_from: vec![package_id.to_string()],
        status,
    }
}

fn policy_allowed_effects(capabilities: &[CapabilityDescriptor]) -> Vec<PolicyEffectAllowance> {
    let enabled = capabilities
        .iter()
        .filter(|capability| capability.enabled)
        .map(|capability| capability.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut effects = Vec::new();
    for (capability, kinds) in policy_effects_by_capability() {
        if enabled.contains(capability) {
            for kind in kinds {
                effects.push(PolicyEffectAllowance {
                    kind: (*kind).to_string(),
                    capability_gate: capability.to_string(),
                });
            }
        }
    }
    effects.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.capability_gate.cmp(&right.capability_gate))
    });
    effects
}

fn policy_capabilities(capabilities: &[CapabilityDescriptor]) -> Vec<PolicyCapability> {
    capabilities
        .iter()
        .map(|capability| PolicyCapability {
            name: capability.name.clone(),
            enabled: capability.enabled,
            description: capability.description,
            allowed: capability.allowed.clone(),
            configured_root: capability.configured_root.clone(),
            deny_by_default: capability.deny_by_default,
            unsafe_unrestricted: capability.unsafe_unrestricted,
            unsafe_opt_in: capability.unsafe_opt_in,
            unsafe_rationale: capability.unsafe_rationale.clone(),
            owner: capability.owner.clone(),
            rationale: capability.rationale.clone(),
        })
        .collect()
}

fn is_false_bool(value: &bool) -> bool {
    !*value
}

fn policy_known_effect_kinds() -> Vec<&'static str> {
    let mut kinds = policy_effects_by_capability()
        .iter()
        .flat_map(|(_, kinds)| kinds.iter().copied())
        .collect::<Vec<_>>();
    kinds.sort_unstable();
    kinds.dedup();
    kinds
}

fn policy_effects_by_capability() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("clock", vec!["clock.now", "clock.sleep"]),
        ("env", vec!["env.read"]),
        ("fs", vec!["fs.read"]),
        ("fs:write", vec!["fs.write"]),
        (
            "net",
            vec![
                "network.dns.resolve",
                "network.http.get",
                "network.tcp.bind",
                "network.tcp.connect",
                "network.udp.send",
            ],
        ),
        ("process", vec!["process.status"]),
        ("crypto", vec!["crypto.hash", "crypto.mac"]),
    ]
}

fn policy_observed_effect(effect: &EffectNode) -> PolicyObservedEffect {
    PolicyObservedEffect {
        kind: effect.kind.to_string(),
        resource: effect.resource.clone(),
        operation: effect.operation,
        capability_gate: effect.capability_gate,
        source_span: effect.source_span.clone(),
    }
}

#[derive(Debug, Clone, Serialize)]
struct GenerateRunbookReport {
    schema_version: &'static str,
    ok: bool,
    command: &'static str,
    project: String,
    target_contract: TargetContract,
    artifact: GeneratedArtifact,
    semantic_capabilities: Vec<SemanticGraphNode>,
    observed_effects: Vec<EffectNode>,
    evidence_summary: EvidenceSummary,
    evidence: Vec<EvidenceItem>,
    artifacts: Vec<ArtifactNode>,
    diagnostics: Vec<Diagnostic>,
}

fn generate_runbook(project: &Path, out: &Path) -> Result<GenerateRunbookReport, Diagnostic> {
    let manifest = load_manifest(project)?;
    let _package = manifest.package.as_ref().ok_or_else(|| {
        Diagnostic::new(
            "runbook",
            "runbook generation requires a package manifest with [package].",
        )
    })?;
    let output_path = if out.is_absolute() {
        out.to_path_buf()
    } else {
        project.join(out)
    };
    let package_id = package_node_for_path(project);
    let capabilities = project_capabilities(project)?;
    let (semantic_nodes, _) = inspect_semantic_graph(project)?;
    let mut semantic_capabilities = semantic_nodes
        .iter()
        .filter(|node| node.kind == "capability")
        .cloned()
        .collect::<Vec<_>>();
    semantic_capabilities.sort_by(|left, right| left.name.cmp(&right.name));
    let mut semantic_evidence = semantic_nodes
        .iter()
        .filter(|node| node.kind == "evidence")
        .cloned()
        .collect::<Vec<_>>();
    semantic_evidence.sort_by(|left, right| left.name.cmp(&right.name));
    let effects = inspect_effects(project)?.effects;
    let evidence = evidence_report(project)?;
    let diagnostics = runbook_diagnostics(project)?;
    let artifact = runbook_artifact(project, &output_path, &package_id, "generated");
    let artifacts = runbook_artifacts_with_generated_output(
        project,
        inspect_artifacts(project)?.artifacts,
        &artifact,
    );
    let target_contract = runbook_target_contract(artifact.clone());
    let markdown = render_runbook_markdown(
        project,
        &manifest,
        &package_id,
        &artifact,
        &target_contract,
        &capabilities,
        &semantic_capabilities,
        &semantic_evidence,
        &effects,
        &evidence,
        &artifacts,
        &diagnostics,
    );
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            Diagnostic::new(
                "runbook",
                format!("failed to create {}: {err}", parent.display()),
            )
        })?;
    }
    fs::write(&output_path, markdown).map_err(|err| {
        Diagnostic::new(
            "runbook",
            format!("failed to write {}: {err}", output_path.display()),
        )
    })?;
    Ok(GenerateRunbookReport {
        schema_version: GENERATE_RUNBOOK_SCHEMA_VERSION,
        ok: true,
        command: "generate runbook",
        project: project.display().to_string(),
        target_contract,
        artifact,
        semantic_capabilities,
        observed_effects: effects,
        evidence_summary: evidence.summary.clone(),
        evidence: evidence.evidence,
        artifacts,
        diagnostics,
    })
}

fn runbook_target_contract(artifact: GeneratedArtifact) -> TargetContract {
    TargetContract {
        id: RUNBOOK_TARGET_ID,
        target_class: RUNBOOK_ARTIFACT_KIND,
        description: "Stage 1 operator runbook generator for capabilities, effects, evidence, and artifacts.",
        status: "experimental",
        input_node_kinds: vec![
            "Package",
            "Capability",
            "Effect",
            "RuntimeSurface",
            "Evidence",
            "Artifact",
        ],
        supported_effect_kinds: policy_known_effect_kinds(),
        supported_type_features: Vec::new(),
        artifact_outputs: vec![artifact],
        evidence_requirements: vec!["unit_test", "fixture"],
        unsupported_feature_diagnostics: Vec::new(),
    }
}

fn runbook_artifact(
    project: &Path,
    output_path: &Path,
    package_id: &str,
    status: &'static str,
) -> GeneratedArtifact {
    GeneratedArtifact {
        id: format!("{package_id}/artifact/operator-runbook"),
        kind: RUNBOOK_ARTIFACT_KIND,
        path: project_relative_path(project, output_path),
        generated_from: vec![package_id.to_string()],
        status,
    }
}

fn runbook_artifacts_with_generated_output(
    project: &Path,
    mut artifacts: Vec<ArtifactNode>,
    artifact: &GeneratedArtifact,
) -> Vec<ArtifactNode> {
    let mut found = false;
    for item in &mut artifacts {
        if item.kind == RUNBOOK_ARTIFACT_KIND
            && project_relative_path(project, Path::new(&item.path)) == artifact.path
        {
            item.exists = true;
            item.status = "generated";
            found = true;
        }
    }
    if !found {
        artifacts.push(ArtifactNode {
            id: artifact.id.clone(),
            kind: RUNBOOK_ARTIFACT_KIND,
            path: artifact.path.clone(),
            exists: true,
            source: "target_contract",
            generated_from: artifact.generated_from.clone(),
            status: "generated",
        });
    }
    artifacts.sort_by(|left, right| {
        left.kind
            .cmp(right.kind)
            .then_with(|| left.path.cmp(&right.path))
    });
    artifacts
}

fn runbook_diagnostics(project: &Path) -> Result<Vec<Diagnostic>, Diagnostic> {
    match check_project_with_options(project, &CheckOptions::default()) {
        Ok(output) => Ok(output
            .warnings
            .into_iter()
            .map(|warning| Diagnostic::new("warning", warning).normalized_for_json())
            .collect()),
        Err(error) => Ok(vec![error.normalized_for_json()]),
    }
}

#[allow(clippy::too_many_arguments)]
fn render_runbook_markdown(
    project: &Path,
    manifest: &axiomc::manifest::Manifest,
    package_id: &str,
    artifact: &GeneratedArtifact,
    target_contract: &TargetContract,
    capabilities: &[CapabilityDescriptor],
    semantic_capabilities: &[SemanticGraphNode],
    semantic_evidence: &[SemanticGraphNode],
    effects: &[EffectNode],
    evidence: &EvidenceReport,
    artifacts: &[ArtifactNode],
    diagnostics: &[Diagnostic],
) -> String {
    let package = manifest
        .package
        .as_ref()
        .expect("runbook generation already checked package manifest");
    let entry = project_relative_path(project, &entry_path(project, manifest));
    let evidence_refs = runbook_evidence_refs(&evidence.evidence);
    let evidence_by_id = semantic_evidence
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let mut out = String::new();
    out.push_str(&format!("# Operator Runbook: {}\n\n", package.name));
    out.push_str("## Package\n\n");
    out.push_str(&format!("- Package: `{}`\n", package.name));
    out.push_str(&format!("- Version: `{}`\n", package.version));
    out.push_str(&format!("- Package node: `{package_id}`\n"));
    out.push_str(&format!("- Build entry: `{entry}`\n"));
    out.push_str(&format!("- Target: `{}`\n", target_contract.id));
    out.push_str(&format!("- Artifact: `{}`\n\n", artifact.path));

    out.push_str("## Capability Gates\n\n");
    out.push_str("| Capability | Enabled | Allowed Values | Unsafe | Owner | Rationale |\n");
    out.push_str("|---|---|---|---|---|---|\n");
    for capability in capabilities {
        let allowed = if capability.allowed.is_empty() {
            String::from("-")
        } else {
            capability.allowed.join(", ")
        };
        let unsafe_state = if capability.unsafe_unrestricted {
            "unrestricted"
        } else if capability.unsafe_opt_in {
            "opt-in"
        } else {
            "-"
        };
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} |\n",
            markdown_cell(&capability.name),
            yes_no(capability.enabled),
            markdown_cell(&allowed),
            unsafe_state,
            markdown_cell(capability.owner.as_deref().unwrap_or("-")),
            markdown_cell(
                capability
                    .unsafe_rationale
                    .as_deref()
                    .or(capability.rationale.as_deref())
                    .unwrap_or("-")
            )
        ));
    }
    out.push('\n');

    out.push_str("## Semantic Capabilities\n\n");
    if semantic_capabilities.is_empty() {
        out.push_str("- No semantic capability declarations were found.\n\n");
    } else {
        for capability in semantic_capabilities {
            out.push_str(&format!("### {}\n\n", capability.name));
            out.push_str(&format!("- Node: `{}`\n", capability.id));
            out.push_str(&format!(
                "- Source: `{}`:{}:{}\n",
                capability.span.path, capability.span.line, capability.span.column
            ));
            out.push_str("- Inputs: ");
            if capability.inputs.is_empty() {
                out.push_str("none\n");
            } else {
                out.push_str(
                    &capability
                        .inputs
                        .iter()
                        .map(|input| format!("`{}: {}`", input.name, input.ty))
                        .collect::<Vec<_>>()
                        .join(", "),
                );
                out.push('\n');
            }
            out.push_str("- Declared effects: ");
            if capability.effects.is_empty() {
                out.push_str("none\n");
            } else {
                out.push_str(
                    &capability
                        .effects
                        .iter()
                        .map(|effect| format!("`{} {}`", effect.kind, effect.target))
                        .collect::<Vec<_>>()
                        .join(", "),
                );
                out.push('\n');
            }
            out.push_str("- Backing evidence: ");
            if capability.evidence.is_empty() {
                out.push_str("missing evidence\n\n");
            } else {
                let refs = capability
                    .evidence
                    .iter()
                    .map(|id| {
                        evidence_by_id
                            .get(id.as_str())
                            .map(|node| format!("`{}` (`{}`)", node.name, node.id))
                            .unwrap_or_else(|| format!("`{id}`"))
                    })
                    .collect::<Vec<_>>();
                out.push_str(&refs.join(", "));
                out.push_str("\n\n");
            }
        }
    }

    out.push_str("## Observed Runtime Effects\n\n");
    if effects.is_empty() {
        out.push_str("- No runtime effects were observed.\n\n");
    } else {
        out.push_str("| Effect | Operation | Resource | Capability Gate | Source | Evidence |\n");
        out.push_str("|---|---|---|---|---|---|\n");
        for effect in effects {
            out.push_str(&format!(
                "| `{}` | `{}` | {} | `{}` | `{}`:{}:{} | {} |\n",
                effect.kind,
                effect.operation,
                markdown_cell(&effect.resource),
                effect.capability_gate,
                markdown_cell(&effect.source_span.path),
                effect.source_span.line,
                effect.source_span.column,
                markdown_cell(&evidence_refs)
            ));
        }
        out.push('\n');
    }

    out.push_str("## Evidence\n\n");
    out.push_str(&format!(
        "- Validation status: `{}`\n",
        evidence.validation_status
    ));
    out.push_str(&format!(
        "- Summary: {} passing, {} failing, {} missing, {} provided\n\n",
        evidence.summary.passing,
        evidence.summary.failing,
        evidence.summary.missing,
        evidence.summary.provided
    ));
    out.push_str("| Evidence | Type | Status | Target | Path |\n");
    out.push_str("|---|---|---|---|---|\n");
    for item in &evidence.evidence {
        out.push_str(&format!(
            "| `{}` | `{}` | `{}` | `{}` | {} |\n",
            markdown_cell(&item.id),
            item.evidence_type,
            item.status,
            markdown_cell(&item.target),
            markdown_cell(item.path.as_deref().unwrap_or("-"))
        ));
    }
    out.push('\n');

    out.push_str("## Artifacts\n\n");
    out.push_str("| Kind | Status | Source | Path |\n");
    out.push_str("|---|---|---|---|\n");
    let mut rendered_artifacts = BTreeSet::new();
    for item in artifacts {
        let path = project_relative_path(project, Path::new(&item.path));
        if !rendered_artifacts.insert((item.kind, path.clone())) {
            continue;
        }
        out.push_str(&format!(
            "| `{}` | `{}` | `{}` | `{}` |\n",
            item.kind,
            item.status,
            item.source,
            markdown_cell(&path)
        ));
    }
    out.push('\n');

    out.push_str("## Unsupported Feature Diagnostics\n\n");
    if diagnostics.is_empty() {
        out.push_str("- None.\n");
    } else {
        for diagnostic in diagnostics {
            let code = diagnostic.code.as_deref().unwrap_or(&diagnostic.kind);
            out.push_str(&format!(
                "- `{}`: {}\n",
                markdown_cell(code),
                markdown_cell(&diagnostic.message)
            ));
        }
    }
    out
}

fn runbook_evidence_refs(evidence: &[EvidenceItem]) -> String {
    if evidence.is_empty() {
        return String::from("missing evidence");
    }
    evidence
        .iter()
        .map(|item| format!("{} ({})", item.id, item.status))
        .collect::<Vec<_>>()
        .join(", ")
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn markdown_cell(value: &str) -> String {
    value.replace('\n', " ").replace('|', "\\|")
}

fn symbol_span(path: &Path, line: usize, column: usize) -> SymbolSpan {
    SymbolSpan {
        path: path.display().to_string(),
        line,
        column,
    }
}

fn function_signature(function: &axiomc::syntax::Function) -> String {
    let async_prefix = if function.is_async { "async " } else { "" };
    let params = function
        .params
        .iter()
        .map(|param| format!("{}: {}", param.name, render_type(&param.ty)))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "pub {async_prefix}fn {}({params}): {}",
        function.source_name,
        render_type(&function.return_ty)
    )
}

fn render_type(ty: &axiomc::syntax::TypeName) -> String {
    use axiomc::syntax::TypeName;
    match ty {
        TypeName::Int => "int".to_string(),
        TypeName::Numeric(numeric) => numeric.as_str().to_string(),
        TypeName::Bool => "bool".to_string(),
        TypeName::String => "string".to_string(),
        TypeName::Str => "str".to_string(),
        TypeName::Named(name, args) if args.is_empty() => name.clone(),
        TypeName::Named(name, args) => format!(
            "{}<{}>",
            name,
            args.iter().map(render_type).collect::<Vec<_>>().join(", ")
        ),
        TypeName::Ptr(inner) => format!("ptr<{}>", render_type(inner)),
        TypeName::MutPtr(inner) => format!("mut ptr<{}>", render_type(inner)),
        TypeName::MutRef(inner) => format!("&mut {}", render_type(inner)),
        TypeName::Slice(inner) => format!("&[{}]", render_type(inner)),
        TypeName::MutSlice(inner) => format!("&mut [{}]", render_type(inner)),
        TypeName::LifetimeSlice(lifetime, inner) => {
            format!("&'{lifetime} [{}]", render_type(inner))
        }
        TypeName::LifetimeMutSlice(lifetime, inner) => {
            format!("&'{lifetime} mut [{}]", render_type(inner))
        }
        TypeName::Option(inner) => format!("Option<{}>", render_type(inner)),
        TypeName::Result(ok, err) => format!("Result<{}, {}>", render_type(ok), render_type(err)),
        TypeName::Tuple(elements) => format!(
            "({})",
            elements
                .iter()
                .map(render_type)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        TypeName::Map(key, value) => format!("{{{}: {}}}", render_type(key), render_type(value)),
        TypeName::Array(inner, Some(size)) => format!("[{}; {}]", render_type(inner), size),
        TypeName::Array(inner, None) => format!("[{}]", render_type(inner)),
        TypeName::Fn(params, ret) => format!(
            "fn({}) -> {}",
            params
                .iter()
                .map(render_type)
                .collect::<Vec<_>>()
                .join(", "),
            render_type(ret)
        ),
    }
}

fn capabilities_in_stmts(stmts: &[axiomc::syntax::Stmt]) -> Vec<&'static str> {
    let mut capabilities = Vec::new();
    for stmt in stmts {
        collect_stmt_capabilities(stmt, &mut capabilities);
    }
    capabilities.sort_unstable();
    capabilities.dedup();
    capabilities
}

fn capabilities_in_expr(expr: &axiomc::syntax::Expr) -> Vec<&'static str> {
    let mut capabilities = Vec::new();
    collect_expr_capabilities(expr, &mut capabilities);
    capabilities.sort_unstable();
    capabilities.dedup();
    capabilities
}

fn collect_stmt_capabilities(stmt: &axiomc::syntax::Stmt, capabilities: &mut Vec<&'static str>) {
    use axiomc::syntax::Stmt;
    match stmt {
        Stmt::Let { expr, .. }
        | Stmt::Print { expr, .. }
        | Stmt::Panic { expr, .. }
        | Stmt::Defer { expr, .. }
        | Stmt::Return { expr, .. } => collect_expr_capabilities(expr, capabilities),
        Stmt::Assign { target, expr, .. } => {
            collect_expr_capabilities(target, capabilities);
            collect_expr_capabilities(expr, capabilities);
        }
        Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            collect_expr_capabilities(cond, capabilities);
            for stmt in then_block {
                collect_stmt_capabilities(stmt, capabilities);
            }
            for stmt in else_block.iter().flatten() {
                collect_stmt_capabilities(stmt, capabilities);
            }
        }
        Stmt::IfLet {
            expr,
            then_block,
            else_block,
            ..
        } => {
            collect_expr_capabilities(expr, capabilities);
            for stmt in then_block {
                collect_stmt_capabilities(stmt, capabilities);
            }
            for stmt in else_block.iter().flatten() {
                collect_stmt_capabilities(stmt, capabilities);
            }
        }
        Stmt::While { cond, body, .. } => {
            collect_expr_capabilities(cond, capabilities);
            for stmt in body {
                collect_stmt_capabilities(stmt, capabilities);
            }
        }
        Stmt::Match { expr, arms, .. } => {
            collect_expr_capabilities(expr, capabilities);
            for arm in arms {
                for stmt in &arm.body {
                    collect_stmt_capabilities(stmt, capabilities);
                }
            }
        }
    }
}

fn collect_expr_capabilities(expr: &axiomc::syntax::Expr, capabilities: &mut Vec<&'static str>) {
    use axiomc::syntax::Expr;
    match expr {
        Expr::Call { name, args, .. } => {
            if let Some(capability) = capability_for_call(name) {
                capabilities.push(capability);
            }
            for arg in args {
                collect_expr_capabilities(arg, capabilities);
            }
        }
        Expr::MethodCall { base, args, .. } => {
            collect_expr_capabilities(base, capabilities);
            for arg in args {
                collect_expr_capabilities(arg, capabilities);
            }
        }
        Expr::BinaryAdd { lhs, rhs, .. } | Expr::BinaryCompare { lhs, rhs, .. } => {
            collect_expr_capabilities(lhs, capabilities);
            collect_expr_capabilities(rhs, capabilities);
        }
        Expr::Cast { expr, .. }
        | Expr::Try { expr, .. }
        | Expr::Await { expr, .. }
        | Expr::MutBorrow { expr, .. }
        | Expr::Deref { expr, .. } => {
            collect_expr_capabilities(expr, capabilities);
        }
        Expr::StructLiteral { fields, .. } => {
            for field in fields {
                collect_expr_capabilities(&field.expr, capabilities);
            }
        }
        Expr::FieldAccess { base, .. } | Expr::TupleIndex { base, .. } => {
            collect_expr_capabilities(base, capabilities);
        }
        Expr::Slice {
            base, start, end, ..
        } => {
            collect_expr_capabilities(base, capabilities);
            if let Some(start) = start {
                collect_expr_capabilities(start, capabilities);
            }
            if let Some(end) = end {
                collect_expr_capabilities(end, capabilities);
            }
        }
        Expr::TupleLiteral { elements, .. } | Expr::ArrayLiteral { elements, .. } => {
            for element in elements {
                collect_expr_capabilities(element, capabilities);
            }
        }
        Expr::MapLiteral { entries, .. } => {
            for entry in entries {
                collect_expr_capabilities(&entry.key, capabilities);
                collect_expr_capabilities(&entry.value, capabilities);
            }
        }
        Expr::Index { base, index, .. } => {
            collect_expr_capabilities(base, capabilities);
            collect_expr_capabilities(index, capabilities);
        }
        Expr::Closure { body, .. } => collect_expr_capabilities(body, capabilities),
        Expr::Match { expr, arms, .. } => {
            collect_expr_capabilities(expr, capabilities);
            for arm in arms {
                collect_expr_capabilities(&arm.expr, capabilities);
            }
        }
        Expr::Literal(_) | Expr::VarRef { .. } => {}
    }
}

fn capability_for_call(name: &str) -> Option<&'static str> {
    match name {
        "clock_now_ms" | "clock_elapsed_ms" | "clock_sleep_ms" | "now_ms" | "now"
        | "elapsed_ms" | "sleep" => Some("clock"),
        "env_get" | "get_env" => Some("env"),
        "fs_read" | "read_file" => Some("fs"),
        "fs_write" | "fs_create" | "fs_append" | "fs_mkdir" | "fs_mkdir_all" | "fs_remove_file"
        | "fs_remove_dir" | "fs_replace" | "write_file" | "create_file" | "append_file"
        | "mkdir" | "mkdir_all" | "remove_file" | "remove_dir" | "replace_file" => Some("fs:write"),
        "net_resolve"
        | "http_get"
        | "http_serve_once"
        | "http_serve_route"
        | "net_tcp_listen"
        | "net_tcp_listener_port"
        | "net_tcp_accept"
        | "net_tcp_read"
        | "net_tcp_write"
        | "net_tcp_close"
        | "net_tcp_close_listener"
        | "net_udp_bind"
        | "net_udp_local_addr"
        | "net_udp_local_port"
        | "net_udp_send_to"
        | "net_udp_recv_from"
        | "net_udp_close"
        | "net_tcp_listen_loopback_once"
        | "tcp_listen_loopback_once"
        | "net_tcp_dial"
        | "tcp_dial"
        | "net_udp_bind_loopback_once"
        | "udp_bind_loopback_once"
        | "net_udp_send_recv"
        | "udp_send_recv" => Some("net"),
        "process_status" => Some("process"),
        "crypto_sha256"
        | "crypto_hmac_sha256"
        | "crypto_hmac_sha512"
        | "crypto_constant_time_eq"
        | "crypto_constant_time_eq_u8"
        | "crypto_rand_bytes"
        | "crypto_rand_u64"
        | "crypto_aead_seal"
        | "crypto_aead_open"
        | "hmac_sha256"
        | "hmac_sha512"
        | "verify_sha256"
        | "verify_sha512"
        | "constant_time_eq"
        | "constant_time_eq_u8"
        | "random_bytes"
        | "random_u64"
        | "aead_seal"
        | "aead_open" => Some("crypto"),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize)]
struct DoctorReport {
    schema_version: &'static str,
    ok: bool,
    command: &'static str,
    project: String,
    rustc: ToolProbe,
    cargo: ToolProbe,
    target_triple: Option<String>,
    lockfile_status: &'static str,
    capabilities: Vec<axiomc::manifest::CapabilityDescriptor>,
    workspace_graph: Vec<DoctorPackage>,
    known_unsupported_features: Vec<&'static str>,
    error: Option<Diagnostic>,
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

fn doctor_report(project: &Path) -> DoctorReport {
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
        known_unsupported_features: vec![
            "package registry resolution",
            "native Axiom DWARF line tables",
            "general borrow checker",
            "trait-style interfaces",
            "closures",
        ],
        error,
    }
}

fn probe_tool(program: &str, args: &[&str]) -> ToolProbe {
    match ProcessCommand::new(program).args(args).output() {
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

fn doctor_text(report: &DoctorReport) -> String {
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

#[derive(Debug, Clone, Serialize)]
struct FormatFileReport {
    path: String,
    changed: bool,
    edits: Vec<FormatEdit>,
}

#[derive(Debug, Clone, Serialize)]
struct FormatReport {
    schema_version: &'static str,
    ok: bool,
    command: &'static str,
    check: bool,
    files: Vec<FormatFileReport>,
    changed: usize,
}

fn format_axiom_sources(path: &Path, check: bool) -> Result<FormatReport, Diagnostic> {
    let files = axiom_files(path)?;
    if files.is_empty() {
        return Err(Diagnostic::new(
            "fmt",
            format!("no .ax files found under {}", path.display()),
        ));
    }
    let mut reports = Vec::new();
    let mut changed = 0;
    for file in files {
        let original = fs::read_to_string(&file).map_err(|err| {
            Diagnostic::new("fmt", format!("failed to read {}: {err}", file.display()))
                .with_path(file.display().to_string())
        })?;
        let formatted = format_axiom_source(&original);
        let is_changed = formatted != original;
        let edits = format_edits(&original, &formatted);
        if is_changed {
            changed += 1;
            if !check {
                fs::write(&file, formatted).map_err(|err| {
                    Diagnostic::new("fmt", format!("failed to write {}: {err}", file.display()))
                        .with_path(file.display().to_string())
                })?;
            }
        }
        reports.push(FormatFileReport {
            path: file.display().to_string(),
            changed: is_changed,
            edits,
        });
    }
    Ok(FormatReport {
        schema_version: json_contract::JSON_SCHEMA_VERSION,
        ok: !check || changed == 0,
        command: "fmt",
        check,
        files: reports,
        changed,
    })
}

fn format_axiom_source(source: &str) -> String {
    let mut lines = Vec::new();
    let mut previous_blank = false;
    for line in source.replace("\r\n", "\n").replace('\t', "    ").lines() {
        let trimmed_end = line.trim_end();
        let blank = trimmed_end.is_empty();
        if blank && previous_blank {
            continue;
        }
        lines.push(trimmed_end.to_string());
        previous_blank = blank;
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    format!("{}\n", lines.join("\n"))
}

fn format_edits(original: &str, formatted: &str) -> Vec<FormatEdit> {
    let original_lines: Vec<&str> = original.split_inclusive('\n').collect();
    let formatted_lines: Vec<&str> = formatted.split_inclusive('\n').collect();
    let max_len = original_lines.len().max(formatted_lines.len());
    let mut edits = Vec::new();
    for index in 0..max_len {
        match (original_lines.get(index), formatted_lines.get(index)) {
            (Some(before), Some(after)) if before != after => edits.push(FormatEdit {
                action: String::from("replace_line"),
                line: index + 1,
                before: Some(trim_line_ending(before).to_string()),
                after: Some(trim_line_ending(after).to_string()),
            }),
            (Some(before), None) => edits.push(FormatEdit {
                action: String::from("delete_line"),
                line: index + 1,
                before: Some(trim_line_ending(before).to_string()),
                after: None,
            }),
            (None, Some(after)) => edits.push(FormatEdit {
                action: String::from("insert_line"),
                line: index + 1,
                before: None,
                after: Some(trim_line_ending(after).to_string()),
            }),
            _ => {}
        }
    }
    edits
}

fn trim_line_ending(line: &str) -> &str {
    line.strip_suffix('\n')
        .and_then(|line| line.strip_suffix('\r').or(Some(line)))
        .unwrap_or(line)
}

#[derive(Debug, Clone, Serialize)]
struct DocOutput {
    schema_version: &'static str,
    command: &'static str,
    ok: bool,
    markdown: PathBuf,
    html: PathBuf,
    items: Vec<DocItem>,
    capabilities: Vec<CapabilityDescriptor>,
}

fn generate_docs(path: &Path, out_dir: &Path) -> Result<DocOutput, Diagnostic> {
    let files = axiom_files(path)?;
    if files.is_empty() {
        return Err(Diagnostic::new(
            "doc",
            format!("no .ax files found under {}", path.display()),
        ));
    }
    fs::create_dir_all(out_dir).map_err(|err| {
        Diagnostic::new(
            "doc",
            format!("failed to create {}: {err}", out_dir.display()),
        )
    })?;
    let items = extract_doc_items(&files)?;
    let markdown = render_markdown_docs(&items);
    let html = render_html_docs(&markdown);
    let markdown_path = out_dir.join("index.md");
    let html_path = out_dir.join("index.html");
    fs::write(&markdown_path, markdown).map_err(|err| {
        Diagnostic::new(
            "doc",
            format!("failed to write {}: {err}", markdown_path.display()),
        )
    })?;
    fs::write(&html_path, html).map_err(|err| {
        Diagnostic::new(
            "doc",
            format!("failed to write {}: {err}", html_path.display()),
        )
    })?;
    let capabilities = project_capabilities(path).unwrap_or_default();
    Ok(DocOutput {
        schema_version: json_contract::JSON_SCHEMA_VERSION,
        command: "doc",
        ok: true,
        markdown: markdown_path,
        html: html_path,
        items,
        capabilities,
    })
}

#[derive(Debug, Clone, Serialize)]
struct DocItem {
    file: String,
    kind: String,
    public: bool,
    signature: String,
    docs: Vec<String>,
    examples: Vec<String>,
}

fn extract_doc_items(files: &[PathBuf]) -> Result<Vec<DocItem>, Diagnostic> {
    let mut items = Vec::new();
    for file in files {
        let source = fs::read_to_string(file).map_err(|err| {
            Diagnostic::new("doc", format!("failed to read {}: {err}", file.display()))
                .with_path(file.display().to_string())
        })?;
        let mut pending_docs = Vec::new();
        for line in source.lines() {
            let trimmed = line.trim();
            if let Some(comment) = trimmed.strip_prefix("///") {
                pending_docs.push(comment.trim().to_string());
                continue;
            }
            if is_documented_signature(trimmed) {
                let examples = pending_docs
                    .iter()
                    .filter_map(|line| {
                        line.strip_prefix("Example:")
                            .or_else(|| line.strip_prefix("example:"))
                            .map(str::trim)
                            .map(str::to_string)
                    })
                    .collect();
                items.push(DocItem {
                    file: file.display().to_string(),
                    kind: doc_item_kind(trimmed).to_string(),
                    public: trimmed.starts_with("pub "),
                    signature: trimmed.to_string(),
                    docs: std::mem::take(&mut pending_docs),
                    examples,
                });
            } else if !trimmed.is_empty() {
                pending_docs.clear();
            }
        }
    }
    Ok(items)
}

fn doc_item_kind(line: &str) -> &'static str {
    let line = line.strip_prefix("pub ").unwrap_or(line);
    let line = line.strip_prefix("async ").unwrap_or(line);
    if line.starts_with("fn ") {
        "function"
    } else if line.starts_with("struct ") {
        "struct"
    } else if line.starts_with("enum ") {
        "enum"
    } else if line.starts_with("const ") {
        "const"
    } else {
        "declaration"
    }
}

fn is_documented_signature(line: &str) -> bool {
    line.starts_with("fn ")
        || line.starts_with("pub fn ")
        || line.starts_with("async fn ")
        || line.starts_with("pub async fn ")
        || line.starts_with("struct ")
        || line.starts_with("pub struct ")
        || line.starts_with("enum ")
        || line.starts_with("pub enum ")
        || line.starts_with("const ")
        || line.starts_with("pub const ")
}

fn render_markdown_docs(items: &[DocItem]) -> String {
    let mut output = String::from("# Axiom API\n\n");
    if items.is_empty() {
        output.push_str("No public or documented declarations found.\n");
        return output;
    }
    for item in items {
        output.push_str(&format!("## `{}`\n\n", item.signature));
        output.push_str(&format!("Source: `{}`\n\n", item.file));
        if item.docs.is_empty() {
            output.push_str("_No doc comment provided._\n\n");
        } else {
            output.push_str(&format!("{}\n\n", item.docs.join("\n")));
        }
    }
    output
}

fn render_html_docs(markdown: &str) -> String {
    let escaped = markdown
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>Axiom API</title></head><body><pre>{escaped}</pre></body></html>\n"
    )
}

#[derive(Debug, Clone, Serialize)]
struct BenchReport {
    schema_version: &'static str,
    benches: Vec<BenchResult>,
    passed: usize,
    failed: usize,
}

#[derive(Debug, Clone, Serialize)]
struct BenchResult {
    name: String,
    path: String,
    warmup: usize,
    iterations: usize,
    median_ms: u64,
    p95_ms: u64,
    allocations: Option<u64>,
    ok: bool,
}

fn run_benchmarks(
    project_root: &Path,
    warmup: usize,
    iterations: usize,
) -> Result<BenchReport, Diagnostic> {
    if iterations == 0 {
        return Err(Diagnostic::new(
            "bench",
            "iterations must be greater than zero",
        ));
    }
    let benches = discover_named_files(project_root, "_bench.ax")?;
    if benches.is_empty() {
        return Err(Diagnostic::new(
            "bench",
            format!("no *_bench.ax files found under {}", project_root.display()),
        ));
    }
    let mut results = Vec::new();
    for bench in benches {
        let name = bench
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("bench")
            .to_string();
        for _ in 0..warmup {
            let _ = check_project_with_options(project_root, &CheckOptions::default())?;
        }
        let mut times = Vec::new();
        for _ in 0..iterations {
            let started = Instant::now();
            let _ = check_project_with_options(project_root, &CheckOptions::default())?;
            times.push(started.elapsed().as_millis() as u64);
        }
        times.sort_unstable();
        let median = times[times.len() / 2];
        let p95_index = ((times.len() * 95).div_ceil(100)).saturating_sub(1);
        results.push(BenchResult {
            name,
            path: bench.display().to_string(),
            warmup,
            iterations,
            median_ms: median,
            p95_ms: times[p95_index.min(times.len() - 1)],
            allocations: None,
            ok: true,
        });
    }
    Ok(BenchReport {
        schema_version: "axiom.stage1.bench.v1",
        passed: results.len(),
        failed: 0,
        benches: results,
    })
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct MutationIssueReport {
    schema_version: &'static str,
    survivor_count: usize,
    groups: Vec<MutationSurvivorGroup>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct MutationSurvivorGroup {
    file: String,
    function: String,
    recommended_fixture: String,
    survivors: Vec<MutationSurvivor>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct MutationSurvivor {
    id: String,
    mutator: String,
    line: Option<u64>,
    description: String,
}

fn mutation_report_from_path(path: &Path) -> Result<MutationIssueReport, Diagnostic> {
    let source = fs::read_to_string(path).map_err(|err| {
        Diagnostic::new(
            "mutation-report",
            format!("failed to read {}: {err}", path.display()),
        )
        .with_path(path.display().to_string())
    })?;
    let base_dir = path.parent();
    mutation_report_from_json_str_with_base_dir(&source, base_dir)
}

fn mutation_report_from_json_str(source: &str) -> Result<MutationIssueReport, Diagnostic> {
    mutation_report_from_json_str_with_base_dir(source, None)
}

fn mutation_report_from_json_str_with_base_dir(
    source: &str,
    base_dir: Option<&Path>,
) -> Result<MutationIssueReport, Diagnostic> {
    let value: serde_json::Value = serde_json::from_str(source)
        .map_err(|err| Diagnostic::new("mutation-report", format!("invalid JSON: {err}")))?;
    let rows = mutation_rows(&value).ok_or_else(|| {
        Diagnostic::new(
            "mutation-report",
            "expected a JSON array or an object containing mutants/survivors/results",
        )
    })?;

    let mut survivors = Vec::new();
    for row in rows {
        if is_survivor(row) {
            let file = string_field(row, &["file", "source_file", "source", "path"])
                .unwrap_or_else(|| String::from("<unknown>"));
            let function = string_field(row, &["function", "function_name", "fn", "symbol"])
                .unwrap_or_else(|| {
                    infer_function_from_file_and_line(
                        &file,
                        number_field(row, &["line", "start_line"]),
                        base_dir,
                    )
                });
            survivors.push((
                file,
                function,
                MutationSurvivor {
                    id: string_field(row, &["id", "name", "mutant", "mutation_id"])
                        .unwrap_or_else(|| stable_survivor_id(row)),
                    mutator: string_field(row, &["mutator", "operator", "mutation", "kind"])
                        .unwrap_or_else(|| String::from("unknown")),
                    line: number_field(row, &["line", "start_line"]),
                    description: string_field(
                        row,
                        &["description", "replacement", "diff", "summary"],
                    )
                    .unwrap_or_else(|| String::from("surviving mutation")),
                },
            ));
        }
    }

    survivors.sort_by(|a, b| (&a.0, &a.1, a.2.line, &a.2.id).cmp(&(&b.0, &b.1, b.2.line, &b.2.id)));
    let mut groups: Vec<MutationSurvivorGroup> = Vec::new();
    for (file, function, survivor) in survivors {
        if let Some(group) = groups
            .last_mut()
            .filter(|g| g.file == file && g.function == function)
        {
            group.survivors.push(survivor);
        } else {
            groups.push(MutationSurvivorGroup {
                recommended_fixture: recommended_fixture_name(&file, &function),
                file,
                function,
                survivors: vec![survivor],
            });
        }
    }
    let survivor_count = groups.iter().map(|group| group.survivors.len()).sum();
    Ok(MutationIssueReport {
        schema_version: "axiom.stage1.mutation-issue-report.v1",
        survivor_count,
        groups,
    })
}

fn mutation_rows(value: &serde_json::Value) -> Option<Vec<&serde_json::Value>> {
    if let Some(rows) = value.as_array() {
        return Some(rows.iter().collect());
    }
    for key in ["survivors", "mutants", "results", "mutations"] {
        if let Some(rows) = value.get(key).and_then(|v| v.as_array()) {
            return Some(rows.iter().collect());
        }
    }
    None
}

fn is_survivor(value: &serde_json::Value) -> bool {
    for key in ["status", "outcome", "result"] {
        if let Some(status) = value.get(key).and_then(|v| v.as_str()) {
            let normalized = status.to_ascii_lowercase().replace(['_', '-'], " ");
            return normalized == "survived" || normalized == "survivor" || normalized == "live";
        }
    }
    value
        .get("survived")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn string_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key)?.as_str().map(ToString::to_string))
}

fn number_field(value: &serde_json::Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| value.get(*key)?.as_u64())
}

fn stable_survivor_id(value: &serde_json::Value) -> String {
    let encoded = serde_json::to_string(value).unwrap_or_default();
    format!("survivor-{:016x}", stable_hash(&encoded))
}

fn stable_hash(input: &str) -> u64 {
    input.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

fn infer_function_from_file_and_line(
    file: &str,
    line: Option<u64>,
    base_dir: Option<&Path>,
) -> String {
    if let Some(line) = line {
        let source_path = match (Path::new(file).is_absolute(), base_dir) {
            (true, _) | (_, None) => PathBuf::from(file),
            (false, Some(base_dir)) => base_dir.join(file),
        };
        if let Ok(source) = fs::read_to_string(source_path) {
            let mut current_function = None;
            for (index, source_line) in source.lines().enumerate() {
                let source_line_number = u64::try_from(index).unwrap_or(u64::MAX) + 1;
                if source_line_number > line {
                    break;
                }
                if let Some(function) = function_name_from_source_line(source_line) {
                    current_function = Some(function);
                }
            }
            if let Some(function) = current_function {
                return function;
            }
        }
    }

    Path::new(file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module")
        .to_string()
}

fn function_name_from_source_line(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let without_visibility = trimmed.strip_prefix("pub ").unwrap_or(trimmed);
    let without_async = without_visibility
        .strip_prefix("async ")
        .unwrap_or(without_visibility);
    let signature = without_async.strip_prefix("fn ")?;
    let name: String = signature
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() { None } else { Some(name) }
}

fn recommended_fixture_name(file: &str, function: &str) -> String {
    let stem = Path::new(file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("source");
    let raw = format!("mutation_{}_{}_survivors", stem, function);
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

fn render_mutation_issue_report(report: &MutationIssueReport) -> String {
    let mut out = String::new();
    out.push_str("## Mutation survivor report\n\n");
    out.push_str(&format!("Surviving mutants: {}\n\n", report.survivor_count));
    if report.groups.is_empty() {
        out.push_str("No surviving mutants found.\n");
        return out;
    }
    for group in &report.groups {
        out.push_str(&format!("### `{}` :: `{}`\n\n", group.file, group.function));
        out.push_str(&format!(
            "Recommended fixture: `{}`\n\n",
            group.recommended_fixture
        ));
        for survivor in &group.survivors {
            let line = survivor
                .line
                .map(|line| format!(":{line}"))
                .unwrap_or_default();
            out.push_str(&format!(
                "- `{}`{} `{}` — {}\n",
                survivor.id, line, survivor.mutator, survivor.description
            ));
        }
        out.push('\n');
    }
    out
}

fn run_repl<R: BufRead, W: Write>(
    mut input: R,
    mut output: W,
    json: bool,
) -> Result<(), Diagnostic> {
    if json {
        writeln!(
            output,
            "{{\"schema_version\":\"axiom.stage1.repl.v1\",\"status\":\"ready\"}}"
        )
        .map_err(|err| Diagnostic::new("repl", format!("failed to write prompt: {err}")))?;
    } else {
        writeln!(output, "axiomc repl (:quit to exit, :check to validate)")
            .map_err(|err| Diagnostic::new("repl", format!("failed to write prompt: {err}")))?;
    }
    let mut buffer = String::new();
    let mut program = String::new();
    loop {
        buffer.clear();
        let read = input
            .read_line(&mut buffer)
            .map_err(|err| Diagnostic::new("repl", format!("failed to read input: {err}")))?;
        if read == 0 {
            break;
        }
        let line = buffer.trim_end();
        if line == ":quit" || line == ":exit" {
            break;
        }
        if line == ":clear" {
            program.clear();
            writeln!(output, "cleared")
                .map_err(|err| Diagnostic::new("repl", format!("failed to write output: {err}")))?;
            continue;
        }
        if line == ":check" {
            match validate_repl_program(&program) {
                Ok(items) => writeln!(output, "ok: {items} item(s)").map_err(|err| {
                    Diagnostic::new("repl", format!("failed to write output: {err}"))
                })?,
                Err(error) => writeln!(output, "error: {error}").map_err(|err| {
                    Diagnostic::new("repl", format!("failed to write output: {err}"))
                })?,
            }
            continue;
        }
        program.push_str(line);
        program.push('\n');
        writeln!(output, "accepted: {}", line)
            .map_err(|err| Diagnostic::new("repl", format!("failed to write output: {err}")))?;
    }
    Ok(())
}

fn validate_repl_program(source: &str) -> Result<usize, Diagnostic> {
    let program = parse_program(source, Path::new("<repl>"))?;
    Ok(program.imports.len()
        + program.consts.len()
        + program.type_aliases.len()
        + program.structs.len()
        + program.enums.len()
        + program.functions.len()
        + program.stmts.len())
}

#[derive(Debug, Clone, Serialize)]
struct InspectGraphReport {
    schema_version: &'static str,
    schema: &'static str,
    ok: bool,
    command: &'static str,
    project: String,
    lockfile_status: &'static str,
    lockfile_packages: Vec<LockfilePackageReport>,
    packages: Vec<PackageNode>,
    modules: Vec<ModuleNode>,
    stdlib_modules: Vec<&'static str>,
    cycles: Vec<Vec<String>>,
    import_errors: Vec<ImportErrorReport>,
    nodes: Vec<SemanticGraphNode>,
    edges: Vec<SemanticGraphEdge>,
}

#[derive(Debug, Clone, Serialize)]
struct LockfilePackageReport {
    name: String,
    version: String,
    source: String,
}

#[derive(Debug, Clone, Serialize)]
struct PackageNode {
    name: Option<String>,
    root: String,
    manifest: String,
    dependencies: Vec<PackageEdge>,
    workspace_members: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct PackageEdge {
    name: String,
    path: String,
}

#[derive(Debug, Clone, Serialize)]
struct ModuleNode {
    path: String,
    imports: Vec<ModuleImport>,
    functions: Vec<GraphFunctionNode>,
    type_refs: Vec<String>,
    capabilities: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
struct ModuleImport {
    path: String,
    resolved: Option<String>,
    is_stdlib: bool,
}

#[derive(Debug, Clone, Serialize)]
struct GraphFunctionNode {
    name: String,
    signature: String,
    span: SymbolSpan,
    capabilities: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
struct ImportErrorReport {
    module: String,
    import: String,
    message: String,
}

fn inspect_graph(project: &Path) -> Result<InspectGraphReport, Diagnostic> {
    let manifest = load_manifest(project)?;
    let lockfile_status = match validate_lockfile(project, &manifest) {
        Ok(()) => "valid",
        Err(_) => "invalid",
    };
    let lockfile_packages = expected_lockfile_for_project(project, &manifest)?
        .package
        .into_iter()
        .map(|package| LockfilePackageReport {
            name: package.name,
            version: package.version,
            source: package.source,
        })
        .collect::<Vec<_>>();
    let packages = inspect_package_nodes(project, &manifest);
    let (modules, import_errors) = inspect_module_nodes(project, &manifest)?;
    let cycles = inspect_module_cycles(&modules);
    let (nodes, edges) = inspect_semantic_graph(project)?;

    Ok(InspectGraphReport {
        schema_version: json_contract::JSON_SCHEMA_VERSION,
        schema: INSPECT_SCHEMA_PATH,
        ok: import_errors.is_empty() && cycles.is_empty() && lockfile_status == "valid",
        command: "inspect graph",
        project: project.display().to_string(),
        lockfile_status,
        lockfile_packages,
        packages,
        modules,
        stdlib_modules: vec![
            "std/async.ax",
            "std/collections.ax",
            "std/crypto_hash.ax",
            "std/env.ax",
            "std/fs.ax",
            "std/http.ax",
            "std/io.ax",
            "std/json.ax",
            "std/log.ax",
            "std/net.ax",
            "std/process.ax",
            "std/string_builder.ax",
            "std/sync.ax",
            "std/time.ax",
        ],
        cycles,
        import_errors,
        nodes,
        edges,
    })
}

fn inspect_package_nodes(
    project: &Path,
    manifest: &axiomc::manifest::Manifest,
) -> Vec<PackageNode> {
    let dependencies = manifest
        .dependencies
        .iter()
        .map(|(name, spec)| PackageEdge {
            name: name.clone(),
            path: project.join(&spec.path).display().to_string(),
        })
        .collect::<Vec<_>>();
    let workspace_members = manifest
        .workspace
        .as_ref()
        .map(|workspace| {
            workspace
                .members
                .iter()
                .map(|member| project.join(member).display().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    vec![PackageNode {
        name: manifest
            .package
            .as_ref()
            .map(|package| package.name.clone()),
        root: project.display().to_string(),
        manifest: manifest_path(project).display().to_string(),
        dependencies,
        workspace_members,
    }]
}

fn inspect_module_nodes(
    project: &Path,
    manifest: &axiomc::manifest::Manifest,
) -> Result<(Vec<ModuleNode>, Vec<ImportErrorReport>), Diagnostic> {
    let files = axiom_files(project)?;
    let known = files
        .iter()
        .map(|path| normalize_for_graph(path))
        .collect::<BTreeSet<_>>();
    let dependencies = manifest
        .dependencies
        .iter()
        .map(|(name, spec)| (name.as_str(), project.join(&spec.path).join("src")))
        .collect::<HashMap<_, _>>();
    let stdlib = inspect_stdlib_module_set();
    let mut modules = Vec::new();
    let mut errors = Vec::new();
    for file in files {
        let source = fs::read_to_string(&file).map_err(|err| {
            Diagnostic::new(
                "inspect",
                format!("failed to read {}: {err}", file.display()),
            )
            .with_path(file.display().to_string())
        })?;
        let program = parse_program(&source, &file)?;
        let mut imports = Vec::new();
        for import in &program.imports {
            if import.path.starts_with("std/") {
                let exists = stdlib.contains(import.path.as_str());
                if !exists {
                    errors.push(ImportErrorReport {
                        module: file.display().to_string(),
                        import: import.path.clone(),
                        message: "unknown stdlib module".to_string(),
                    });
                }
                imports.push(ModuleImport {
                    path: import.path.clone(),
                    resolved: None,
                    is_stdlib: true,
                });
                continue;
            }
            let candidate = inspect_dependency_import_candidate(&dependencies, &import.path)
                .unwrap_or_else(|| {
                    file.parent()
                        .map(|parent| parent.join(&import.path))
                        .unwrap_or_else(|| PathBuf::from(&import.path))
                });
            let resolved = normalize_for_graph(&candidate);
            if !known.contains(&resolved) {
                errors.push(ImportErrorReport {
                    module: file.display().to_string(),
                    import: import.path.clone(),
                    message: format!("missing import {}", candidate.display()),
                });
            }
            imports.push(ModuleImport {
                path: import.path.clone(),
                resolved: Some(resolved),
                is_stdlib: false,
            });
        }
        let functions = program
            .functions
            .iter()
            .map(|function| GraphFunctionNode {
                name: function.source_name.clone(),
                signature: function_signature(function),
                span: symbol_span(&file, function.line, function.column),
                capabilities: capabilities_in_stmts(&function.body),
            })
            .collect::<Vec<_>>();
        let type_refs = collect_program_type_refs(&program);
        let mut capabilities = program
            .consts
            .iter()
            .flat_map(|decl| capabilities_in_expr(&decl.expr))
            .chain(
                program
                    .functions
                    .iter()
                    .flat_map(|function| capabilities_in_stmts(&function.body)),
            )
            .collect::<Vec<_>>();
        capabilities.sort_unstable();
        capabilities.dedup();
        modules.push(ModuleNode {
            path: normalize_for_graph(&file),
            imports,
            functions,
            type_refs,
            capabilities,
        });
    }
    modules.sort_by(|left, right| left.path.cmp(&right.path));
    Ok((modules, errors))
}

fn collect_program_type_refs(program: &axiomc::syntax::Program) -> Vec<String> {
    let mut refs = BTreeSet::new();
    for decl in &program.consts {
        refs.insert(render_type(&decl.ty));
    }
    for decl in &program.type_aliases {
        refs.insert(render_type(&decl.ty));
    }
    for decl in &program.structs {
        for field in &decl.fields {
            refs.insert(render_type(&field.ty));
        }
    }
    for decl in &program.enums {
        for variant in &decl.variants {
            for ty in &variant.payload_tys {
                refs.insert(render_type(ty));
            }
        }
    }
    for function in &program.functions {
        for param in &function.params {
            refs.insert(render_type(&param.ty));
        }
        refs.insert(render_type(&function.return_ty));
    }
    refs.into_iter().collect()
}

#[derive(Debug, Clone, Serialize)]
struct InspectEvidenceReport {
    schema_version: &'static str,
    schema: &'static str,
    ok: bool,
    command: &'static str,
    project: String,
    evidence: Vec<EvidenceNode>,
}

#[derive(Debug, Clone, Serialize)]
struct EvidenceNode {
    kind: &'static str,
    name: String,
    path: String,
    package: Option<String>,
    status: String,
}

fn inspect_evidence(project: &Path) -> Result<InspectEvidenceReport, Diagnostic> {
    let manifest = load_manifest(project)?;
    let lockfile_status = match validate_lockfile(project, &manifest) {
        Ok(()) => "valid".to_string(),
        Err(error) => format!("invalid: {error}"),
    };
    let mut evidence = vec![EvidenceNode {
        kind: "lockfile",
        name: "axiom.lock".to_string(),
        path: lockfile_path(project).display().to_string(),
        package: manifest
            .package
            .as_ref()
            .map(|package| package.name.clone()),
        status: lockfile_status,
    }];

    let test_options = TestOptions {
        filter: None,
        package: None,
        include_benchmarks: true,
    };
    match list_project_tests_with_options(project, &test_options) {
        Ok(list) => {
            for test in list.tests {
                evidence.push(EvidenceNode {
                    kind: "test",
                    name: test.name,
                    path: Path::new(&test.package_root)
                        .join(&test.entry)
                        .display()
                        .to_string(),
                    package: test.package,
                    status: format!("configured:{:?}", test.kind),
                });
            }
        }
        Err(error) if error.to_string().contains("no tests discovered") => {}
        Err(error) => evidence.push(EvidenceNode {
            kind: "test",
            name: "test-discovery".to_string(),
            path: manifest_path(project).display().to_string(),
            package: manifest
                .package
                .as_ref()
                .map(|package| package.name.clone()),
            status: format!("unavailable: {error}"),
        }),
    }

    Ok(InspectEvidenceReport {
        schema_version: json_contract::JSON_SCHEMA_VERSION,
        schema: INSPECT_SCHEMA_PATH,
        ok: evidence
            .iter()
            .all(|item| !item.status.starts_with("invalid")),
        command: "inspect evidence",
        project: project.display().to_string(),
        evidence,
    })
}

#[derive(Debug, Clone, Serialize)]
struct InspectArtifactsReport {
    schema_version: &'static str,
    schema: &'static str,
    ok: bool,
    command: &'static str,
    project: String,
    artifacts: Vec<ArtifactNode>,
}

#[derive(Debug, Clone, Serialize)]
struct ArtifactNode {
    id: String,
    kind: &'static str,
    path: String,
    exists: bool,
    source: &'static str,
    generated_from: Vec<String>,
    status: &'static str,
}

fn inspect_artifacts(project: &Path) -> Result<InspectArtifactsReport, Diagnostic> {
    let manifest = load_manifest(project)?;
    let package_id = package_node_for_path(project);
    let mut artifacts = Vec::new();
    push_artifact(
        &mut artifacts,
        &package_id,
        "manifest",
        manifest_path(project),
        "configured",
    );
    push_artifact(
        &mut artifacts,
        &package_id,
        "lockfile",
        lockfile_path(project),
        "configured",
    );
    push_artifact(
        &mut artifacts,
        &package_id,
        "build_entry",
        entry_path(project, &manifest),
        "configured",
    );
    push_artifact(
        &mut artifacts,
        &package_id,
        "build_output_dir",
        out_dir_path(project, &manifest),
        "configured",
    );
    push_artifact(
        &mut artifacts,
        &package_id,
        OPENAPI_ARTIFACT_KIND,
        out_dir_path(project, &manifest).join("openapi.json"),
        "target_contract",
    );
    push_artifact(
        &mut artifacts,
        &package_id,
        POLICY_ARTIFACT_KIND,
        out_dir_path(project, &manifest).join("policy-bundle.json"),
        "target_contract",
    );
    push_artifact(
        &mut artifacts,
        &package_id,
        RUNBOOK_ARTIFACT_KIND,
        out_dir_path(project, &manifest).join("runbook.md"),
        "target_contract",
    );
    if manifest.package.is_some() {
        push_artifact(
            &mut artifacts,
            &package_id,
            "generated_rust",
            generated_rust_path(project, &manifest),
            "configured",
        );
        push_artifact(
            &mut artifacts,
            &package_id,
            "native_binary",
            binary_path(project, &manifest),
            "configured",
        );
    }
    for test in &manifest.tests {
        push_artifact(
            &mut artifacts,
            &package_id,
            "test_entry",
            project.join(&test.entry),
            "configured",
        );
        if let Some(stdout) = &test.stdout {
            push_artifact(
                &mut artifacts,
                &package_id,
                "test_stdout_golden",
                project.join(stdout),
                "configured",
            );
        }
        if let Some(stderr) = &test.stderr {
            push_artifact(
                &mut artifacts,
                &package_id,
                "test_stderr_golden",
                project.join(stderr),
                "configured",
            );
        }
    }
    let test_options = TestOptions {
        filter: None,
        package: None,
        include_benchmarks: true,
    };
    if let Ok(list) = list_project_tests_with_options(project, &test_options) {
        for test in list.tests {
            push_artifact(
                &mut artifacts,
                &package_id,
                "test_entry",
                Path::new(&test.package_root).join(&test.entry),
                "configured",
            );
        }
    }
    let docs_dir = project.join("docs/axiom");
    if docs_dir.exists() {
        push_artifact(&mut artifacts, &package_id, "docs", docs_dir, "available");
    }
    inspect_existing_output_artifacts(
        &mut artifacts,
        &package_id,
        &out_dir_path(project, &manifest),
    )?;
    artifacts.sort_by(|left, right| {
        left.kind
            .cmp(right.kind)
            .then_with(|| left.path.cmp(&right.path))
    });
    artifacts.dedup_by(|left, right| left.kind == right.kind && left.path == right.path);

    Ok(InspectArtifactsReport {
        schema_version: json_contract::JSON_SCHEMA_VERSION,
        schema: INSPECT_SCHEMA_PATH,
        ok: true,
        command: "inspect artifacts",
        project: project.display().to_string(),
        artifacts,
    })
}

fn push_artifact(
    artifacts: &mut Vec<ArtifactNode>,
    package_id: &str,
    kind: &'static str,
    path: PathBuf,
    source: &'static str,
) {
    let status = if path.exists() {
        "generated"
    } else {
        "planned"
    };
    artifacts.push(ArtifactNode {
        id: artifact_node_id(package_id, kind, &path),
        kind,
        exists: path.exists(),
        path: path.display().to_string(),
        source,
        generated_from: vec![package_id.to_string()],
        status,
    });
}

fn artifact_node_id(package_id: &str, kind: &str, path: &Path) -> String {
    format!(
        "{package_id}/artifact/{}/{}",
        normalized_id_component(kind, "artifact"),
        normalized_id_component(&path.display().to_string(), "path")
    )
}

fn inspect_existing_output_artifacts(
    artifacts: &mut Vec<ArtifactNode>,
    package_id: &str,
    out_dir: &Path,
) -> Result<(), Diagnostic> {
    if !out_dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(out_dir).map_err(|err| {
        Diagnostic::new(
            "inspect",
            format!(
                "failed to read build output directory {}: {err}",
                out_dir.display()
            ),
        )
    })? {
        let entry = entry.map_err(|err| {
            Diagnostic::new(
                "inspect",
                format!(
                    "failed to inspect build output directory {}: {err}",
                    out_dir.display()
                ),
            )
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let kind = if name == "policy-bundle.json" || name.ends_with(".policy-bundle.json") {
            POLICY_ARTIFACT_KIND
        } else if name == "openapi.json" || name.ends_with(".openapi.json") {
            OPENAPI_ARTIFACT_KIND
        } else if name.ends_with(".generated.rs") {
            "generated_rust"
        } else if name.ends_with(".debug-map.json") {
            "debug_map"
        } else if name.ends_with(".debug-manifest.json") {
            "debug_manifest"
        } else if name.ends_with(".build-cache.toml") {
            "build_cache"
        } else {
            "build_output"
        };
        push_artifact(artifacts, package_id, kind, path, "available");
    }
    Ok(())
}

fn inspect_dependency_import_candidate(
    dependencies: &HashMap<&str, PathBuf>,
    import: &str,
) -> Option<PathBuf> {
    let (dependency, rest) = import.split_once('/')?;
    dependencies
        .get(dependency)
        .map(|source_root| source_root.join(rest))
}

fn inspect_stdlib_module_set() -> BTreeSet<&'static str> {
    [
        "std/async.ax",
        "std/collections.ax",
        "std/crypto_hash.ax",
        "std/env.ax",
        "std/fs.ax",
        "std/http.ax",
        "std/io.ax",
        "std/json.ax",
        "std/log.ax",
        "std/net.ax",
        "std/process.ax",
        "std/string_builder.ax",
        "std/sync.ax",
        "std/time.ax",
    ]
    .into_iter()
    .collect()
}

fn inspect_module_cycles(modules: &[ModuleNode]) -> Vec<Vec<String>> {
    let graph = modules
        .iter()
        .map(|module| {
            (
                module.path.clone(),
                module
                    .imports
                    .iter()
                    .filter_map(|import| import.resolved.clone())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut cycles = Vec::new();
    for node in graph.keys() {
        let mut stack = Vec::new();
        inspect_find_cycles(node, node, &graph, &mut stack, &mut cycles);
    }
    cycles.sort();
    cycles.dedup();
    cycles
}

fn inspect_find_cycles(
    start: &str,
    current: &str,
    graph: &HashMap<String, Vec<String>>,
    stack: &mut Vec<String>,
    cycles: &mut Vec<Vec<String>>,
) {
    if stack.iter().any(|node| node == current) {
        return;
    }
    stack.push(current.to_string());
    for next in graph.get(current).into_iter().flatten() {
        if next == start {
            let mut cycle = stack.clone();
            cycle.push(start.to_string());
            cycles.push(inspect_canonical_cycle(cycle));
        } else if graph.contains_key(next) {
            inspect_find_cycles(start, next, graph, stack, cycles);
        }
    }
    stack.pop();
}

fn inspect_canonical_cycle(mut cycle: Vec<String>) -> Vec<String> {
    if cycle.len() <= 2 {
        return cycle;
    }
    cycle.pop();
    if let Some((index, _)) = cycle.iter().enumerate().min_by_key(|(_, value)| *value) {
        cycle.rotate_left(index);
    }
    cycle.push(cycle[0].clone());
    cycle
}

fn normalize_for_graph(path: &Path) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

fn trace_project_and_query(query: &str) -> (PathBuf, Option<String>) {
    if query.starts_with("axiom://") {
        (PathBuf::from("."), Some(query.to_string()))
    } else {
        (PathBuf::from(query), None)
    }
}

fn axiom_files(path: &Path) -> Result<Vec<PathBuf>, Diagnostic> {
    if path.is_file() {
        return if path.extension().is_some_and(|ext| ext == "ax") {
            Ok(vec![path.to_path_buf()])
        } else {
            Err(Diagnostic::new(
                "source",
                format!("{} is not an .ax source file", path.display()),
            ))
        };
    }
    discover_named_files(path, ".ax")
}

fn discover_named_files(path: &Path, suffix: &str) -> Result<Vec<PathBuf>, Diagnostic> {
    let mut files = Vec::new();
    discover_named_files_into(path, suffix, &mut files)?;
    files.sort();
    Ok(files)
}

fn discover_named_files_into(
    path: &Path,
    suffix: &str,
    files: &mut Vec<PathBuf>,
) -> Result<(), Diagnostic> {
    for entry in fs::read_dir(path).map_err(|err| {
        Diagnostic::new(
            "source",
            format!("failed to read {}: {err}", path.display()),
        )
        .with_path(path.display().to_string())
    })? {
        let entry = entry.map_err(|err| {
            Diagnostic::new(
                "source",
                format!("failed to read {}: {err}", path.display()),
            )
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|err| {
            Diagnostic::new(
                "source",
                format!("failed to inspect {}: {err}", path.display()),
            )
        })?;
        if file_type.is_dir() {
            let name = entry.file_name();
            if name == "target" || name == "dist" || name == ".git" {
                continue;
            }
            discover_named_files_into(&path, suffix, files)?;
        } else if file_type.is_file()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(suffix))
        {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser};

    #[test]
    fn help_describes_supported_stage1_workflows() {
        let help = Cli::command().render_long_help().to_string();
        assert!(help.contains("Create a new stage1 package"));
        assert!(help.contains("Check a stage1 package or workspace member"));
        assert!(
            help.contains("Build a stage1 package through the current generated-Rust backend path")
        );
        assert!(help.contains(
            "Build and run a stage1 package through the current generated-Rust backend path"
        ));

        let mut command = Cli::command();
        let build_help = command
            .find_subcommand_mut("build")
            .expect("build subcommand")
            .render_long_help()
            .to_string();
        assert!(build_help.contains(
            "Today only `generated-rust` is implemented; additional native backends remain future work"
        ));
        assert!(build_help.contains(
            "Build a stage1 package through the current generated-Rust backend path into a native or WASM artifact"
        ));
        assert!(build_help.contains("--locked"));
        assert!(build_help.contains("--offline"));
        assert!(!build_help.contains("direct-native"));
        assert!(help.contains("Discover, build, and run package test entrypoints"));
        assert!(help.contains("Inspect manifest capability requirements"));
        assert!(help.contains("Inspect project metadata for agent tooling"));
        assert!(help.contains("Generate target artifacts from stage1 semantic intent"));
        assert!(help.contains("Inspect local package graph metadata"));
        assert!(help.contains("Explain a stable diagnostic code"));
        assert!(help.contains("Report local stage1 project and toolchain health"));
        assert!(help.contains("Format .ax source files"));
        assert!(help.contains("Generate Markdown and HTML API docs"));
        assert!(help.contains("Run discovered *_bench.ax entrypoints"));
        assert!(help.contains("Start a small stage1 scratch REPL"));
        assert!(help.contains("Pack and publish a stage1 package into a local registry tree"));
        assert!(help.contains("Build a static package-registry index"));
        assert!(help.contains("Validate a static package-registry index JSON file"));
    }

    #[test]
    fn pkg_graph_cli_parses_path_and_json_flag() {
        let cli = Cli::parse_from(["axiomc", "pkg", "graph", ".", "--json"]);
        match cli.command {
            Command::Pkg {
                command: PkgCommand::Graph { path, json },
            } => {
                assert_eq!(path, PathBuf::from("."));
                assert!(json);
            }
            other => panic!("expected pkg graph command, got {other:?}"),
        }
    }

    #[test]
    fn generate_openapi_cli_parses_output_path_and_json_flag() {
        let cli = Cli::parse_from([
            "axiomc",
            "generate",
            "openapi",
            ".",
            "--out",
            "dist/openapi.json",
            "--json",
        ]);
        match cli.command {
            Command::Generate {
                command: GenerateCommand::Openapi { path, out, json },
            } => {
                assert_eq!(path, PathBuf::from("."));
                assert_eq!(out, PathBuf::from("dist/openapi.json"));
                assert!(json);
            }
            other => panic!("expected generate openapi command, got {other:?}"),
        }
    }

    #[test]
    fn generate_policy_cli_parses_output_path_and_json_flag() {
        let cli = Cli::parse_from([
            "axiomc",
            "generate",
            "policy",
            ".",
            "--out",
            "dist/policy-bundle.json",
            "--json",
        ]);
        match cli.command {
            Command::Generate {
                command: GenerateCommand::Policy { path, out, json },
            } => {
                assert_eq!(path, PathBuf::from("."));
                assert_eq!(out, PathBuf::from("dist/policy-bundle.json"));
                assert!(json);
            }
            other => panic!("expected generate policy command, got {other:?}"),
        }
    }

    #[test]
    fn generate_runbook_cli_parses_output_path_and_json_flag() {
        let cli = Cli::parse_from([
            "axiomc",
            "generate",
            "runbook",
            ".",
            "--out",
            "dist/runbook.md",
            "--json",
        ]);
        match cli.command {
            Command::Generate {
                command: GenerateCommand::Runbook { path, out, json },
            } => {
                assert_eq!(path, PathBuf::from("."));
                assert_eq!(out, PathBuf::from("dist/runbook.md"));
                assert!(json);
            }
            other => panic!("expected generate runbook command, got {other:?}"),
        }
    }

    #[test]
    fn test_accepts_list_flag() {
        let cli = Cli::parse_from(["axiomc", "test", ".", "--list", "--json"]);
        match cli.command {
            Command::Test { list, json, .. } => {
                assert!(list);
                assert!(json);
            }
            other => panic!("expected test command, got {other:?}"),
        }
    }

    #[test]
    fn build_accepts_locked_offline_flags() {
        let cli = Cli::parse_from(["axiomc", "build", ".", "--locked", "--offline"]);
        match cli.command {
            Command::Build {
                locked, offline, ..
            } => {
                assert!(locked);
                assert!(offline);
            }
            other => panic!("expected build command, got {other:?}"),
        }
    }

    #[test]
    fn trace_cli_accepts_project_paths_and_node_ids() {
        let cli = Cli::parse_from(["axiomc", "trace", ".", "--json"]);
        match cli.command {
            Command::Trace { query, json } => {
                assert_eq!(query, ".");
                assert!(json);
            }
            other => panic!("expected trace command, got {other:?}"),
        }

        let (project, query) =
            trace_project_and_query("axiom://package/demo/function/src/main.ax/main");
        assert_eq!(project, PathBuf::from("."));
        assert_eq!(
            query.as_deref(),
            Some("axiom://package/demo/function/src/main.ax/main")
        );
    }

    #[test]
    fn build_rejects_unimplemented_native_backend_values() {
        let error = Cli::try_parse_from(["axiomc", "build", ".", "--backend", "direct-native"])
            .expect_err(
                "direct-native should remain unavailable in the preparatory backend plumbing",
            );
        let rendered = error.to_string();
        assert!(rendered.contains("unsupported backend \"direct-native\""));
        assert!(
            rendered.contains(
                "only generated-rust is implemented in this preparatory backend plumbing"
            )
        );
    }

    #[test]
    fn caps_diff_cli_parses_old_and_new_payload_paths() {
        let cli = Cli::try_parse_from(["axiomc", "caps", "diff", "old-caps.json", "new-caps.json"])
            .expect("parse caps diff command");

        match cli.command {
            Command::Caps {
                command: Some(CapsCommand::Diff { old, new }),
                ..
            } => {
                assert_eq!(old, PathBuf::from("old-caps.json"));
                assert_eq!(new, PathBuf::from("new-caps.json"));
            }
            other => panic!("expected caps diff command, got {other:?}"),
        }
    }

    #[test]
    fn caps_diff_cli_retains_path_for_runtime_rejection() {
        let cli = Cli::try_parse_from([
            "axiomc",
            "caps",
            "my-project",
            "diff",
            "old-caps.json",
            "new-caps.json",
        ])
        .expect("parse caps diff command with path");

        match cli.command {
            Command::Caps {
                path,
                command: Some(CapsCommand::Diff { old, new }),
                ..
            } => {
                assert_eq!(path, Some(PathBuf::from("my-project")));
                assert_eq!(old, PathBuf::from("old-caps.json"));
                assert_eq!(new, PathBuf::from("new-caps.json"));
            }
            other => panic!("expected caps diff command with path, got {other:?}"),
        }
    }

    fn build_output(debug_map: Option<String>, debug_manifest: Option<String>) -> BuildOutput {
        BuildOutput {
            backend: NativeBackendKind::GeneratedRust,
            locked: false,
            offline: false,
            manifest: String::from("axiom.toml"),
            entry: String::from("src/main.ax"),
            binary: String::from("dist/app"),
            generated_rust: String::from("target/main.rs"),
            debug_map,
            debug_manifest,
            statement_count: 1,
            target: None,
            debug: true,
            cache_key: axiomc::project::BuildCacheMetadata {
                version: 1,
                compiler: String::from("stage1"),
                target: None,
                debug: true,
                manifest_hash: String::from("manifest-hash"),
                lockfile_hash: String::from("lockfile-hash"),
                generated_rust_hash: String::from("rust-hash"),
                sources: Vec::new(),
            },
            metadata: axiomc::project::BuildMetadata {
                target: None,
                debug: true,
                lockfile: String::from("axiom.lock"),
                lockfile_hash: String::from("lock-hash"),
                source_hash: String::from("source-hash"),
            },
            cache_hits: 0,
            cache_misses: 1,
            duration_ms: 1,
            packages: Vec::new(),
        }
    }

    #[test]
    fn build_json_includes_target_debug_and_cache_key_metadata() {
        let payload = json_contract::build_success(
            Path::new("stage1/examples/hello"),
            &build_output(Some(String::from("target/main.debug-map.json")), None),
        );

        assert_eq!(payload["target"], serde_json::json!(null));
        assert_eq!(payload["debug"], serde_json::json!(true));
        assert_eq!(payload["metadata"]["target"], serde_json::json!(null));
        assert_eq!(payload["metadata"]["debug"], serde_json::json!(true));
        assert_eq!(
            payload["metadata"]["lockfile"],
            serde_json::json!("axiom.lock")
        );
        assert_eq!(
            payload["metadata"]["lockfile_hash"],
            serde_json::json!("lock-hash")
        );
        assert_eq!(
            payload["metadata"]["source_hash"],
            serde_json::json!("source-hash")
        );
    }

    #[test]
    fn build_summary_mentions_debug_artifacts_when_available() {
        assert_eq!(
            build_summary_lines(
                &build_output(
                    Some(String::from("target/main.debug-map.json")),
                    Some(String::from("target/main.debug-manifest.json")),
                ),
                false,
            ),
            vec![
                String::from("wrote dist/app (backend=generated-rust)"),
                String::from("wrote debug map target/main.debug-map.json"),
                String::from("wrote debug manifest target/main.debug-manifest.json"),
            ]
        );
    }

    #[test]
    fn build_summary_omits_debug_artifacts_for_release_builds() {
        assert_eq!(
            build_summary_lines(&build_output(None, None), false),
            vec![String::from("wrote dist/app (backend=generated-rust)")]
        );
    }

    #[test]
    fn caps_diff_reports_added_removed_and_escalated_capabilities() {
        let old = CapsPayload {
            capabilities: vec![
                CapsDescriptor {
                    name: String::from("fs"),
                    enabled: true,
                    allowed: Vec::new(),
                    unsafe_unrestricted: false,
                },
                CapsDescriptor {
                    name: String::from("env"),
                    enabled: true,
                    allowed: vec![String::from("AXIOM_SAFE")],
                    unsafe_unrestricted: false,
                },
                CapsDescriptor {
                    name: String::from("process"),
                    enabled: true,
                    allowed: Vec::new(),
                    unsafe_unrestricted: false,
                },
            ],
        };
        let new = CapsPayload {
            capabilities: vec![
                CapsDescriptor {
                    name: String::from("fs"),
                    enabled: false,
                    allowed: Vec::new(),
                    unsafe_unrestricted: false,
                },
                CapsDescriptor {
                    name: String::from("net"),
                    enabled: true,
                    allowed: Vec::new(),
                    unsafe_unrestricted: false,
                },
                CapsDescriptor {
                    name: String::from("env"),
                    enabled: true,
                    allowed: vec![String::from("AXIOM_SECRET"), String::from("AXIOM_SAFE")],
                    unsafe_unrestricted: true,
                },
                CapsDescriptor {
                    name: String::from("process"),
                    enabled: true,
                    allowed: Vec::new(),
                    unsafe_unrestricted: false,
                },
            ],
        };

        let report = diff_caps_payloads(
            &old,
            &new,
            String::from("old.json"),
            String::from("new.json"),
        );

        assert_eq!(report.added_capabilities, vec![String::from("net")]);
        assert_eq!(report.removed_capabilities, vec![String::from("fs")]);
        assert_eq!(report.escalated_capabilities, vec![String::from("net")]);
        assert_eq!(
            report.added_scopes,
            vec![CapsScopeDiff {
                capability: String::from("env"),
                scopes: vec![String::from("AXIOM_SECRET")],
            }]
        );
        assert_eq!(report.unsafe_escalations, vec![String::from("env")]);
        assert!(report.escalated);
        assert!(!report.ok);
    }

    #[test]
    fn caps_diff_allows_reductions_without_escalation() {
        let old = CapsPayload {
            capabilities: vec![CapsDescriptor {
                name: String::from("env"),
                enabled: true,
                allowed: vec![String::from("AXIOM_SECRET"), String::from("AXIOM_SAFE")],
                unsafe_unrestricted: true,
            }],
        };
        let new = CapsPayload {
            capabilities: vec![CapsDescriptor {
                name: String::from("env"),
                enabled: true,
                allowed: vec![String::from("AXIOM_SAFE")],
                unsafe_unrestricted: false,
            }],
        };

        let report = diff_caps_payloads(
            &old,
            &new,
            String::from("old.json"),
            String::from("new.json"),
        );

        assert_eq!(report.removed_scopes.len(), 1);
        assert_eq!(report.unsafe_reductions, vec![String::from("env")]);
        assert!(!report.escalated);
        assert!(report.ok);
    }

    #[test]
    fn inspect_symbols_reports_public_symbols_and_capabilities() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source_dir = dir.path().join("src");
        fs::create_dir_all(&source_dir).expect("create source dir");
        fs::write(
            source_dir.join("main.ax"),
            "import \"time.ax\"\n\npub const LIMIT: int = 3\n\npub struct Job {\nname: string\n}\n\npub fn now(): int {\nreturn clock_now_ms()\n}\n\npub fn dial(): int {\nreturn net_tcp_dial(\"127.0.0.1\", 80)\n}\n\npub fn write_file_cap(): int {\nreturn fs_write(\"tmp.txt\", \"ok\")\n}\n\npub fn create_file_cap(): int {\nreturn fs_create(\"tmp.txt\")\n}\n\npub fn serve_once_cap(): bool {\nreturn http_serve_once(\"127.0.0.1:0\", \"ok\")\n}\n\npub fn serve_route_cap(): bool {\nreturn http_serve_route(\"127.0.0.1:0\", \"/\", \"ok\", 1)\n}\n\npub fn mac(): string {\nreturn hmac_sha256(\"key\", \"message\")\n}\n\npub fn safe_eq(): bool {\nreturn constant_time_eq(\"left\", \"right\")\n}\n\npub fn slice_time(values: [int]): [int] {\nreturn values[0:clock_now_ms()]\n}\n\nfn private_helper(): int {\nreturn 1\n}\n",
        )
        .expect("write main source");
        fs::write(
            source_dir.join("time.ax"),
            "pub fn exported(): int {\nreturn 7\n}\n",
        )
        .expect("write imported source");

        let report = inspect_symbols(dir.path()).expect("inspect symbols");

        assert_eq!(report.command, "inspect symbols");
        assert_eq!(report.symbols.len(), 12);
        let now = report
            .symbols
            .iter()
            .find(|symbol| symbol.name == "now")
            .expect("now symbol");
        assert_eq!(now.kind, "function");
        assert!(now.signature.contains("pub fn now(): int"));
        assert_eq!(now.imports, vec![String::from("time.ax")]);
        assert_eq!(now.capabilities, vec!["clock"]);
        let dial = report
            .symbols
            .iter()
            .find(|symbol| symbol.name == "dial")
            .expect("dial symbol");
        assert_eq!(dial.capabilities, vec!["net"]);
        for symbol_name in ["write_file_cap", "create_file_cap"] {
            let symbol = report
                .symbols
                .iter()
                .find(|symbol| symbol.name == symbol_name)
                .expect("fs write symbol");
            assert_eq!(symbol.capabilities, vec!["fs:write"]);
        }
        for symbol_name in ["serve_once_cap", "serve_route_cap"] {
            let symbol = report
                .symbols
                .iter()
                .find(|symbol| symbol.name == symbol_name)
                .expect("net symbol");
            assert_eq!(symbol.capabilities, vec!["net"]);
        }
        for symbol_name in ["mac", "safe_eq"] {
            let symbol = report
                .symbols
                .iter()
                .find(|symbol| symbol.name == symbol_name)
                .expect("crypto symbol");
            assert_eq!(symbol.capabilities, vec!["crypto"]);
        }
        let slice_time = report
            .symbols
            .iter()
            .find(|symbol| symbol.name == "slice_time")
            .expect("slice_time symbol");
        assert_eq!(slice_time.capabilities, vec!["clock"]);
        assert!(
            report
                .symbols
                .iter()
                .any(|symbol| symbol.name == "exported")
        );
        assert!(
            !report
                .symbols
                .iter()
                .any(|symbol| symbol.name == "private_helper")
        );
    }

    #[test]
    fn repair_plan_reports_type_diagnostic_task() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("repair-type");
        fs::create_dir_all(project.join("src")).expect("create src");
        write_minimal_manifest(&project, "repair-type");
        fs::write(
            project.join("src").join("main.ax"),
            "pub fn main(): int {\nreturn \"wrong\"\n}\n",
        )
        .expect("write source");

        let report = repair_plan(&project).expect("repair plan");
        let payload = serde_json::to_value(&report).expect("serialize repair plan");
        validate_repair_plan_schema(&payload);

        assert_eq!(report.schema_version, "axiom.repair.v0");
        assert_eq!(report.command, "repair-plan");
        assert_eq!(report.tasks.len(), 1);
        assert!(report.tasks[0].reason.starts_with("type"));
        assert_eq!(report.tasks[0].required_evidence, vec!["unit_test"]);
        assert_eq!(report.tasks[0].diagnostics.len(), 1);
        assert_eq!(report.tasks[0].diagnostics[0].kind, "type");
        assert!(report.tasks[0].diagnostics[0].line.is_some());
    }

    #[test]
    fn repair_plan_reports_missing_evidence_task_for_testless_package() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("repair-evidence");
        fs::create_dir_all(project.join("src")).expect("create src");
        write_minimal_manifest(&project, "repair-evidence");
        fs::write(
            project.join("src").join("main.ax"),
            "pub fn main(): int {\nreturn 0\n}\n",
        )
        .expect("write source");

        let report = repair_plan(&project).expect("repair plan");
        let payload = serde_json::to_value(&report).expect("serialize repair plan");
        validate_repair_plan_schema(&payload);

        assert_eq!(report.tasks.len(), 1);
        assert_eq!(report.tasks[0].reason, "missing_evidence");
        assert_eq!(report.tasks[0].required_evidence, vec!["unit_test"]);
        assert!(report.tasks[0].diagnostics.is_empty());
        assert!(
            report.tasks[0]
                .allowed_files
                .iter()
                .any(|path| path.ends_with("src/main.ax"))
        );
    }

    fn write_minimal_manifest(project: &Path, name: &str) {
        fs::write(
            project.join("axiom.toml"),
            format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n"
            ),
        )
        .expect("write manifest");
        fs::write(
            project.join("axiom.lock"),
            format!("version = 1\n\n[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\nsource = \"path\"\n"),
        )
        .expect("write lockfile");
    }

    fn validate_repair_plan_schema(payload: &serde_json::Value) {
        let schema_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/axiom-repair-v0.schema.json");
        let schema: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(schema_path).expect("read schema"))
                .expect("schema json");
        let validator = jsonschema::validator_for(&schema).expect("compile schema");
        validator
            .validate(payload)
            .expect("repair plan matches schema");
    }

    #[test]
    fn evidence_report_emits_missing_placeholder_for_packages_without_tests() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("evidence-empty");
        fs::create_dir_all(project.join("src")).expect("create src");
        fs::write(
            project.join("axiom.toml"),
            "[package]\nname = \"evidence-empty\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
        )
        .expect("write manifest");
        fs::write(
            project.join("axiom.lock"),
            "version = 1\n\n[[package]]\nname = \"evidence-empty\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
        )
        .expect("write lockfile");
        fs::write(
            project.join("src").join("main.ax"),
            "pub fn main(): int {\nreturn 0\n}\n",
        )
        .expect("write main");

        let report = evidence_report(&project).expect("evidence report");
        let payload = serde_json::to_value(&report).expect("serialize evidence report");
        let schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../schemas/axiom-evidence-v0.schema.json");
        let schema: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(schema_path).expect("read schema"))
                .expect("schema json");
        let validator = jsonschema::validator_for(&schema).expect("compile schema");
        validator
            .validate(&payload)
            .expect("evidence report matches schema");

        assert_eq!(report.schema_version, "axiom.evidence.v0");
        assert_eq!(report.command, "evidence");
        assert_eq!(report.validation_status, "missing");
        assert_eq!(report.summary.missing, 1);
        assert_eq!(report.evidence[0].evidence_type, "unit_test");
        assert_eq!(report.evidence[0].status, "missing");
    }

    #[test]
    fn evidence_report_marks_failing_manifest_tests() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("evidence-failing");
        fs::create_dir_all(project.join("src")).expect("create src");
        fs::write(
            project.join("axiom.toml"),
            "[package]\nname = \"evidence-failing\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[[tests]]\nname = \"stdout mismatch\"\nentry = \"src/main_test.ax\"\nstdout = \"src/main_test.stdout\"\nkind = \"unit\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
        )
        .expect("write manifest");
        fs::write(
            project.join("axiom.lock"),
            "version = 1\n\n[[package]]\nname = \"evidence-failing\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
        )
        .expect("write lockfile");
        fs::write(
            project.join("src").join("main.ax"),
            "pub fn main(): int {\nreturn 0\n}\n",
        )
        .expect("write main");
        fs::write(
            project.join("src").join("main_test.ax"),
            "pub fn main(): int {\nprint \"actual\"\nreturn 0\n}\n",
        )
        .expect("write test");
        fs::write(project.join("src").join("main_test.stdout"), "expected\n")
            .expect("write stdout fixture");

        let report = evidence_report(&project).expect("evidence report");

        assert!(!report.ok);
        assert_eq!(report.validation_status, "failing");
        assert_eq!(report.summary.failing, 1);
        assert_eq!(report.summary.provided, 1);
        assert_eq!(report.evidence[0].evidence_type, "unit_test");
        assert_eq!(report.evidence[0].status, "failing");
        assert_eq!(report.evidence[0].path.as_deref(), Some("src/main_test.ax"));
    }

    #[test]
    fn inspect_effects_reports_known_runtime_surfaces() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source_dir = dir.path().join("src");
        fs::create_dir_all(&source_dir).expect("create source dir");
        fs::write(
            source_dir.join("main.ax"),
            "pub fn main(): int {\nlet now: int = clock_now_ms()\nlet home: string = env_get(\"HOME\")\nlet body: string = http_get(\"https://example.com\")\nlet ok: bool = fs_write(\"tmp.txt\", body)\nlet digest: string = crypto_sha256(home)\nreturn now\n}\n",
        )
        .expect("write source");

        let report = inspect_effects(dir.path()).expect("inspect effects");
        let payload = serde_json::to_value(&report).expect("serialize effects report");
        let schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../schemas/axiom-effects-v0.schema.json");
        let schema: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(schema_path).expect("read schema"))
                .expect("schema json");
        let validator = jsonschema::validator_for(&schema).expect("compile schema");
        validator
            .validate(&payload)
            .expect("effects report matches schema");

        assert_eq!(report.schema_version, "axiom.effects.v0");
        assert_eq!(report.command, "inspect effects");
        let kinds = report
            .effects
            .iter()
            .map(|effect| effect.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                "clock.now",
                "env.read",
                "network.http.get",
                "fs.write",
                "crypto.hash"
            ]
        );
        let http = report
            .effects
            .iter()
            .find(|effect| effect.kind == "network.http.get")
            .expect("http effect");
        assert_eq!(http.resource, "https://example.com");
        assert_eq!(http.capability_gate, "net");
        assert!(http.policy.host_allowed);
        assert!(http.policy.port_allowed);
    }

    #[test]
    fn explain_text_includes_example_and_fix() {
        let info = diagnostic_code_info("use_after_move").expect("diagnostic info");
        let text = explain_text(info);

        assert!(text.contains("use_after_move (ownership)"));
        assert!(text.contains("Example:"));
        assert!(text.contains("Suggested fix:"));
    }

    #[test]
    fn explain_json_payload_is_versioned() {
        let info = diagnostic_code_info("use_after_move").expect("diagnostic info");
        let payload = explain_payload(info);

        assert_eq!(
            payload["schema_version"],
            json_contract::JSON_SCHEMA_VERSION
        );
        assert_eq!(payload["command"], "explain");
        assert_eq!(payload["diagnostic"]["code"], "use_after_move");
    }

    #[test]
    fn doctor_reports_project_health_json_fields() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("doctor");
        create_project_with_template(&project, Some("doctor-app"), WorkloadTemplate::Cli)
            .expect("create project");

        let report = doctor_report(&project);

        assert_eq!(report.schema_version, json_contract::JSON_SCHEMA_VERSION);
        assert_eq!(report.command, "doctor");
        assert_eq!(report.lockfile_status, "valid");
        assert_eq!(report.workspace_graph.len(), 1);
        assert!(report.target_triple.is_some());
        assert!(
            report
                .known_unsupported_features
                .contains(&"package registry resolution")
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

    #[test]
    fn inspect_graph_reports_modules_lockfile_and_import_errors() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("graph");
        let dependency = project.join("deps/core");
        create_project(&project, Some("graph-app")).expect("create project");
        create_project(&dependency, Some("graph-core")).expect("create dependency");
        fs::write(
            project.join("axiom.toml"),
            "[package]\nname = \"graph-app\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[dependencies]\ncore = { path = \"deps/core\" }\n",
        )
        .expect("write manifest");
        fs::write(
            project.join("src/main.ax"),
            "import \"math.ax\"\nimport \"core/math.ax\"\nimport \"missing.ax\"\n\nprint value()\n",
        )
        .expect("write main source");
        fs::write(
            project.join("src/math.ax"),
            "import \"std/time.ax\"\n\npub fn value(): int {\nreturn 7\n}\n",
        )
        .expect("write math source");
        fs::write(
            dependency.join("src/math.ax"),
            "pub fn dep_value(): int {\nreturn 11\n}\n",
        )
        .expect("write dependency source");
        let dependency_manifest = load_manifest(&dependency).expect("load dependency manifest");
        fs::write(
            dependency.join("axiom.lock"),
            axiomc::lockfile::render_lockfile_for_project(&dependency, &dependency_manifest)
                .expect("dependency lockfile"),
        )
        .expect("write dependency lockfile");
        let manifest = load_manifest(&project).expect("load root manifest");
        fs::write(
            project.join("axiom.lock"),
            axiomc::lockfile::render_lockfile_for_project(&project, &manifest)
                .expect("root lockfile"),
        )
        .expect("write root lockfile");

        let report = inspect_graph(&project).expect("inspect graph");

        assert_eq!(report.command, "inspect graph");
        assert_eq!(report.lockfile_status, "valid");
        assert_eq!(report.lockfile_packages.len(), 2);
        assert_eq!(report.packages.len(), 1);
        assert!(report.modules.len() >= 4);
        assert!(report.stdlib_modules.contains(&"std/time.ax"));
        assert_eq!(report.import_errors.len(), 1);
        assert!(report.import_errors[0].message.contains("missing import"));
        let main = report
            .modules
            .iter()
            .find(|module| {
                module
                    .imports
                    .iter()
                    .any(|import| import.path == "core/math.ax")
            })
            .expect("main module");
        let dependency_import = main
            .imports
            .iter()
            .find(|import| import.path == "core/math.ax")
            .expect("dependency import");
        assert!(
            dependency_import
                .resolved
                .as_deref()
                .is_some_and(|path| path.ends_with("deps/core/src/math.ax"))
        );
        let math = report
            .modules
            .iter()
            .find(|module| {
                module
                    .functions
                    .iter()
                    .any(|function| function.name == "value")
            })
            .expect("math module");
        assert!(
            math.functions
                .iter()
                .any(|function| function.name == "value")
        );
        assert!(math.type_refs.contains(&String::from("int")));
    }

    #[test]
    fn inspect_graph_detects_local_module_cycles() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("cycle");
        create_project(&project, Some("cycle-app")).expect("create project");
        fs::write(project.join("src/main.ax"), "import \"a.ax\"\n").expect("write main source");
        fs::write(project.join("src/a.ax"), "import \"b.ax\"\n").expect("write a source");
        fs::write(project.join("src/b.ax"), "import \"a.ax\"\n").expect("write b source");

        let report = inspect_graph(&project).expect("inspect graph");

        assert!(!report.cycles.is_empty());
    }

    #[test]
    fn inspect_effects_evidence_and_artifacts_report_agent_shapes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("inspect");
        create_project(&project, Some("inspect-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            "[package]\nname = \"inspect-app\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = true\n\"fs:write\" = false\nnet = false\nprocess = false\nenv = false\nclock = true\ncrypto = false\nffi = false\nasync = false\n",
        )
        .expect("write manifest");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/time.ax\"\n\npub fn now(): int {\nreturn clock_now_ms()\n}\n\nprint now()\n",
        )
        .expect("write main source");
        fs::write(project.join("src/main_test.ax"), "print 1\n").expect("write test source");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            axiomc::lockfile::render_lockfile_for_project(&project, &manifest)
                .expect("render lockfile"),
        )
        .expect("write lockfile");

        let effects = inspect_effects(&project).expect("inspect effects");
        assert_eq!(effects.command, "inspect effects");
        assert_eq!(effects.schema_version, "axiom.effects.v0");
        assert!(effects.effects.iter().any(|effect| {
            effect.kind == "clock.now"
                && effect.resource == "system_clock"
                && effect.capability_gate == "clock"
        }));
        assert!(effects.effects.iter().any(|effect| {
            effect.kind == "clock.now" && effect.operation == "read" && effect.policy.host_allowed
        }));

        let evidence = inspect_evidence(&project).expect("inspect evidence");
        assert_eq!(evidence.command, "inspect evidence");
        assert!(
            evidence
                .evidence
                .iter()
                .any(|item| item.kind == "lockfile" && item.status == "valid")
        );
        assert!(
            evidence
                .evidence
                .iter()
                .any(|item| item.kind == "test" && item.name.ends_with("src/main_test"))
        );

        fs::create_dir_all(project.join("dist")).expect("create dist");
        fs::write(
            project.join("dist/inspect-app.generated.rs"),
            "// generated\n",
        )
        .expect("write generated artifact");
        let artifacts = inspect_artifacts(&project).expect("inspect artifacts");
        assert_eq!(artifacts.command, "inspect artifacts");
        assert!(
            artifacts
                .artifacts
                .iter()
                .any(|artifact| artifact.kind == "build_entry" && artifact.exists)
        );
        assert!(
            artifacts
                .artifacts
                .iter()
                .any(|artifact| artifact.kind == "generated_rust" && artifact.exists)
        );
        assert!(
            artifacts
                .artifacts
                .iter()
                .any(|artifact| artifact.kind == "test_entry" && artifact.exists)
        );
        assert!(artifacts.artifacts.iter().any(|artifact| {
            artifact.kind == OPENAPI_ARTIFACT_KIND
                && artifact.source == "target_contract"
                && artifact.status == "planned"
        }));
        assert!(artifacts.artifacts.iter().any(|artifact| {
            artifact.kind == POLICY_ARTIFACT_KIND
                && artifact.source == "target_contract"
                && artifact.status == "planned"
        }));
        assert!(artifacts.artifacts.iter().any(|artifact| {
            artifact.kind == RUNBOOK_ARTIFACT_KIND
                && artifact.source == "target_contract"
                && artifact.status == "planned"
        }));
    }

    #[test]
    fn generate_openapi_writes_deterministic_route_artifact() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("openapi-service");
        create_project(&project, Some("openapi-service")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            "[package]\nname = \"openapi-service\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = true\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
        )
        .expect("write manifest");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/http.ax\"\n\nlet selected_response: HttpResponse = response(202, \"ready\", [header(\"content-type\", \"application/json\")])\nlet selected_route: HttpRoute = route_response(\"/ready\", selected_response)\nprint serve(\"127.0.0.1:0\", selected_route, 1)\n",
        )
        .expect("write source");

        let report =
            generate_openapi(&project, Path::new("dist/openapi.json")).expect("generate openapi");

        assert_eq!(report.schema_version, OPENAPI_SCHEMA_VERSION);
        assert_eq!(report.target_contract.id, OPENAPI_TARGET_ID);
        assert_eq!(report.target_contract.target_class, OPENAPI_ARTIFACT_KIND);
        let target_payload =
            serde_json::to_value(&report.target_contract).expect("serialize target contract");
        let target_schema_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/axiom-target-v0.schema.json");
        let target_schema: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(target_schema_path).expect("read schema"))
                .expect("target schema json");
        jsonschema::validator_for(&target_schema)
            .expect("compile target schema")
            .validate(&target_payload)
            .expect("OpenAPI target contract matches target schema");
        assert_eq!(report.artifact.status, "generated");
        assert_eq!(report.routes.len(), 1);
        assert_eq!(report.routes[0].path, "/ready");
        assert_eq!(report.routes[0].response_status, 200);
        assert_eq!(report.routes[0].content_type, "text/plain; charset=utf-8");
        assert!(report.diagnostics.is_empty());

        let spec: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(project.join("dist/openapi.json")).expect("read spec"),
        )
        .expect("spec json");
        assert_eq!(spec["openapi"], OPENAPI_SPEC_VERSION);
        assert_eq!(spec["info"]["title"], "openapi-service");
        assert_eq!(spec["paths"]["/ready"]["get"]["operationId"], "get_ready");
        assert_eq!(
            spec["paths"]["/ready"]["get"]["responses"]["200"]["content"]["text/plain; charset=utf-8"]
                ["schema"]["type"],
            "string"
        );
        let responses = spec["paths"]["/ready"]["get"]["responses"]
            .as_object()
            .expect("responses object");
        assert!(!responses.contains_key("202"));
        let content = responses["200"]["content"]
            .as_object()
            .expect("response content object");
        assert!(!content.contains_key("application/json"));

        let artifacts = inspect_artifacts(&project).expect("inspect artifacts");
        assert!(artifacts.artifacts.iter().any(|artifact| {
            artifact.kind == OPENAPI_ARTIFACT_KIND
                && artifact.path.ends_with("dist/openapi.json")
                && artifact.status == "generated"
        }));
    }

    #[test]
    fn generate_openapi_only_projects_routes_passed_to_serve() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("openapi-served-routes");
        create_project(&project, Some("openapi-served-routes")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            "[package]\nname = \"openapi-served-routes\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = true\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
        )
        .expect("write manifest");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/http.ax\"\n\nfn helper_route(): HttpRoute {\nreturn route(\"/debug-function\", \"debug\")\n}\n\nlet unused_route: HttpRoute = route(\"/debug\", \"debug\")\nlet selected_route: HttpRoute = route(\"/ready\", \"ready\")\nprint serve(\"127.0.0.1:0\", selected_route, 1)\n",
        )
        .expect("write source");

        let report =
            generate_openapi(&project, Path::new("dist/openapi.json")).expect("generate openapi");

        assert_eq!(report.routes.len(), 1);
        assert_eq!(report.routes[0].path, "/ready");

        let spec: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(project.join("dist/openapi.json")).expect("read spec"),
        )
        .expect("spec json");
        let paths = spec["paths"].as_object().expect("paths object");
        assert!(paths.contains_key("/ready"));
        assert!(!paths.contains_key("/debug"));
        assert!(!paths.contains_key("/debug-function"));
    }

    #[test]
    fn generate_openapi_empty_service_emits_valid_empty_spec_with_diagnostic() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("no-http-service");
        create_project(&project, Some("no-http-service")).expect("create project");
        fs::write(project.join("src/main.ax"), "print \"hello\"\n").expect("write source");

        let report =
            generate_openapi(&project, Path::new("dist/openapi.json")).expect("generate openapi");

        assert_eq!(report.routes.len(), 0);
        assert_eq!(report.diagnostics.len(), 1);
        assert!(
            report.diagnostics[0]
                .message
                .contains("no HTTP-serving routes")
        );
        let spec: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(project.join("dist/openapi.json")).expect("read spec"),
        )
        .expect("spec json");
        assert_eq!(spec["openapi"], OPENAPI_SPEC_VERSION);
        assert!(spec["paths"].as_object().expect("paths object").is_empty());
    }

    #[test]
    fn generate_openapi_does_not_project_unused_helper_routes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("openapi-helper-routes");
        create_project(&project, Some("openapi-helper-routes")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            r#"[package]
name = "openapi-helper-routes"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = true
process = false
env = false
clock = false
crypto = false
"#,
        )
        .expect("write manifest");
        fs::write(
            project.join("src/main.ax"),
            r#"import "std/http.ax"

fn helper(): bool {
    return serve("127.0.0.1:0", route("/debug", "debug"), 1)
}

print "hello"
"#,
        )
        .expect("write source");

        let report =
            generate_openapi(&project, Path::new("dist/openapi.json")).expect("generate openapi");

        assert_eq!(report.routes.len(), 0);
        let spec: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(project.join("dist/openapi.json")).expect("read spec"),
        )
        .expect("spec json");
        assert!(spec["paths"].as_object().expect("paths object").is_empty());
    }

    #[test]
    fn generate_openapi_projects_routes_from_called_functions_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("openapi-called-functions");
        create_project(&project, Some("openapi-called-functions")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            r#"[package]
name = "openapi-called-functions"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = true
process = false
env = false
clock = false
crypto = false
"#,
        )
        .expect("write manifest");
        fs::write(
            project.join("src/main.ax"),
            r#"import "std/http.ax"

fn served_from_helper(): bool {
    let selected_route: HttpRoute = route("/from-helper", "ready")
    return serve("127.0.0.1:0", selected_route, 1)
}

fn unused_helper(): bool {
    return serve("127.0.0.1:0", route("/unused-helper", "debug"), 1)
}

print served_from_helper()
"#,
        )
        .expect("write source");

        let report =
            generate_openapi(&project, Path::new("dist/openapi.json")).expect("generate openapi");

        assert_eq!(report.routes.len(), 1);
        assert_eq!(report.routes[0].path, "/from-helper");
        let spec: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(project.join("dist/openapi.json")).expect("read spec"),
        )
        .expect("spec json");
        let paths = spec["paths"].as_object().expect("paths object");
        assert!(paths.contains_key("/from-helper"));
        assert!(!paths.contains_key("/unused-helper"));
    }

    #[test]
    fn generate_openapi_preserves_route_bindings_across_nested_scopes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("openapi-scoped-routes");
        create_project(&project, Some("openapi-scoped-routes")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            r#"[package]
name = "openapi-scoped-routes"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = true
process = false
env = false
clock = false
crypto = false
"#,
        )
        .expect("write manifest");
        fs::write(
            project.join("src/main.ax"),
            r#"import "std/http.ax"

let selected_route: HttpRoute = route("/outer", "ready")

if true {
    let selected_route: HttpRoute = route("/inner", "debug")
    print "shadowed"
}

print serve("127.0.0.1:0", selected_route, 1)
"#,
        )
        .expect("write source");

        let report =
            generate_openapi(&project, Path::new("dist/openapi.json")).expect("generate openapi");

        assert_eq!(report.routes.len(), 1);
        assert_eq!(report.routes[0].path, "/outer");
        let spec: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(project.join("dist/openapi.json")).expect("read spec"),
        )
        .expect("spec json");
        let paths = spec["paths"].as_object().expect("paths object");
        assert!(paths.contains_key("/outer"));
        assert!(!paths.contains_key("/inner"));
    }

    #[test]
    fn generate_policy_writes_manifest_effect_allowlist() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("policy-service");
        create_project(&project, Some("policy-service")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            "[package]\nname = \"policy-service\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = true\n\"fs:write\" = false\nnet = false\nprocess = false\nenv = [\"POLICY_MODE\"]\nclock = true\ncrypto = false\nffi = false\nasync = false\n",
        )
        .expect("write manifest");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/env.ax\"\nimport \"std/fs.ax\"\nimport \"std/time.ax\"\n\nlet now: int = now_ms()\nlet mode: Option<string> = get_env(\"POLICY_MODE\")\nlet contents: Option<string> = read_file(\"policy-input.txt\")\nprint now > 0\n",
        )
        .expect("write source");

        let report =
            generate_policy(&project, Path::new("dist/policy-bundle.json")).expect("policy");

        assert_eq!(report.schema_version, GENERATE_POLICY_SCHEMA_VERSION);
        assert_eq!(report.target_contract.id, POLICY_TARGET_ID);
        assert_eq!(report.target_contract.target_class, POLICY_ARTIFACT_KIND);
        let target_payload =
            serde_json::to_value(&report.target_contract).expect("serialize target contract");
        let target_schema_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/axiom-target-v0.schema.json");
        let target_schema: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(target_schema_path).expect("read schema"))
                .expect("target schema json");
        jsonschema::validator_for(&target_schema)
            .expect("compile target schema")
            .validate(&target_payload)
            .expect("policy target contract matches target schema");
        assert_eq!(report.artifact.status, "generated");
        assert_eq!(
            report.allowed_effect_kinds,
            vec!["clock.now", "clock.sleep", "env.read", "fs.read"]
        );
        let observed = report
            .observed_effects
            .iter()
            .map(|effect| effect.kind.as_str())
            .collect::<Vec<_>>();
        assert_eq!(observed, vec!["clock.now", "env.read", "fs.read"]);

        let bundle: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(project.join("dist/policy-bundle.json")).expect("read bundle"),
        )
        .expect("bundle json");
        assert_eq!(bundle["schema_version"], POLICY_SCHEMA_VERSION);
        assert_eq!(bundle["target_id"], POLICY_TARGET_ID);
        assert_eq!(
            bundle["allowed_effect_kinds"],
            serde_json::json!(["clock.now", "clock.sleep", "env.read", "fs.read"])
        );
    }

    #[test]
    fn generate_policy_reflects_removed_manifest_capability() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("policy-drift");
        create_project(&project, Some("policy-drift")).expect("create project");
        let manifest = |clock: bool| {
            format!(
                "[package]\nname = \"policy-drift\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = {clock}\ncrypto = false\n"
            )
        };
        fs::write(project.join("axiom.toml"), manifest(true)).expect("write manifest");
        fs::write(
            project.join("src/main.ax"),
            "let now: int = clock_now_ms()\nprint now > 0\n",
        )
        .expect("write source");

        let with_clock =
            generate_policy(&project, Path::new("dist/policy-bundle.json")).expect("policy");
        fs::write(project.join("axiom.toml"), manifest(false)).expect("rewrite manifest");
        let without_clock =
            generate_policy(&project, Path::new("dist/policy-bundle.json")).expect("policy");

        assert_eq!(
            with_clock.allowed_effect_kinds,
            vec!["clock.now", "clock.sleep"]
        );
        assert!(without_clock.allowed_effect_kinds.is_empty());
    }

    #[test]
    fn generate_runbook_writes_operator_markdown_from_semantic_evidence() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("runbook-service");
        create_project(&project, Some("runbook-service")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            "[package]\nname = \"runbook-service\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[[tests]]\nname = \"runbook-smoke\"\nentry = \"src/main_test.ax\"\nkind = \"unit\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = [\"RUNBOOK_MODE\"]\nclock = true\ncrypto = false\nffi = false\nasync = false\n",
        )
        .expect("write manifest");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/env.ax\"\nimport \"std/time.ax\"\n\naxiom OperatorModeVisible {\n    scope operations\n    severity info\n    description \"Operators can inspect the configured mode.\"\n    assert mode_is_documented\n}\n\nevidence RunbookSmokeTest {\n    description \"Manifest unit test exercises the runbook package.\"\n}\n\ncapability DescribeOperatorMode {\n    input mode: string\n    effects {\n        read OperatorEnv\n        read RuntimeClock\n    }\n    preserves OperatorModeVisible\n    requires evidence RunbookSmokeTest\n}\n\nlet _mode: Option<string> = get_env(\"RUNBOOK_MODE\")\nlet now: int = now_ms()\nprint now > 0\n",
        )
        .expect("write source");
        fs::write(project.join("src/main_test.ax"), "print \"runbook\"\n").expect("write test");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            axiomc::lockfile::render_lockfile_for_project(&project, &manifest)
                .expect("render lockfile"),
        )
        .expect("write lockfile");

        let report =
            generate_runbook(&project, Path::new("dist/runbook.md")).expect("generate runbook");

        assert_eq!(report.schema_version, GENERATE_RUNBOOK_SCHEMA_VERSION);
        assert_eq!(report.target_contract.id, RUNBOOK_TARGET_ID);
        assert_eq!(report.target_contract.target_class, RUNBOOK_ARTIFACT_KIND);
        let target_payload =
            serde_json::to_value(&report.target_contract).expect("serialize target contract");
        let target_schema_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/axiom-target-v0.schema.json");
        let target_schema: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(target_schema_path).expect("read schema"))
                .expect("target schema json");
        jsonschema::validator_for(&target_schema)
            .expect("compile target schema")
            .validate(&target_payload)
            .expect("runbook target contract matches target schema");
        assert_eq!(report.artifact.status, "generated");
        assert_eq!(report.semantic_capabilities.len(), 1);
        assert_eq!(report.semantic_capabilities[0].name, "DescribeOperatorMode");
        assert_eq!(report.evidence_summary.passing, 1);
        let observed = report
            .observed_effects
            .iter()
            .map(|effect| effect.kind)
            .collect::<Vec<_>>();
        assert_eq!(observed, vec!["env.read", "clock.now"]);

        let runbook =
            fs::read_to_string(project.join("dist/runbook.md")).expect("read generated runbook");
        assert!(runbook.contains("# Operator Runbook: runbook-service"));
        assert!(runbook.contains("DescribeOperatorMode"));
        assert!(runbook.contains("RunbookSmokeTest"));
        assert!(runbook.contains("env.read"));
        assert!(runbook.contains("dist/runbook.md"));

        let artifacts = inspect_artifacts(&project).expect("inspect artifacts");
        assert!(artifacts.artifacts.iter().any(|artifact| {
            artifact.kind == RUNBOOK_ARTIFACT_KIND
                && artifact.path.ends_with("dist/runbook.md")
                && artifact.status == "generated"
        }));
    }

    #[test]
    fn formatter_trims_whitespace_and_collapses_blank_runs() {
        assert_eq!(
            format_axiom_source("fn main() {   \n\tprint \"hi\"  \n\n\n}\n\n"),
            "fn main() {\n    print \"hi\"\n\n}\n"
        );
    }

    #[test]
    fn formatter_check_reports_json_planning_edits_without_writing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src/main.ax");
        fs::create_dir_all(source.parent().expect("source parent")).expect("mkdir");
        fs::write(&source, "fn main() {   \n\tprint \"hi\"  \n\n\n}\n\n").expect("write source");

        let report = format_axiom_sources(dir.path(), true).expect("format report");

        assert_eq!(report.schema_version, json_contract::JSON_SCHEMA_VERSION);
        assert_eq!(report.command, "fmt");
        assert!(report.check);
        assert_eq!(report.changed, 1);
        assert_eq!(report.files.len(), 1);
        assert!(report.files[0].changed);
        assert!(
            report.files[0]
                .edits
                .iter()
                .any(|edit| edit.action == "replace_line" && edit.line == 1)
        );
        assert_eq!(
            fs::read_to_string(&source).expect("read source"),
            "fn main() {   \n\tprint \"hi\"  \n\n\n}\n\n"
        );
    }

    #[test]
    fn formatter_check_reports_missing_final_newline_edit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src/main.ax");
        fs::create_dir_all(source.parent().expect("source parent")).expect("mkdir");
        fs::write(&source, "fn main() {}").expect("write source");

        let report = format_axiom_sources(dir.path(), true).expect("format report");

        assert_eq!(report.changed, 1);
        assert_eq!(report.files.len(), 1);
        assert_eq!(report.files[0].edits.len(), 1);
        assert_eq!(report.files[0].edits[0].action, "replace_line");
        assert_eq!(report.files[0].edits[0].line, 1);
        assert_eq!(
            report.files[0].edits[0].before.as_deref(),
            Some("fn main() {}")
        );
        assert_eq!(
            report.files[0].edits[0].after.as_deref(),
            Some("fn main() {}")
        );
    }

    #[test]
    fn doc_extractor_pairs_doc_comments_with_signatures() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src/main.ax");
        fs::create_dir_all(source.parent().expect("source parent")).expect("mkdir");
        fs::write(
            &source,
            "/// Adds one.\npub fn inc(value: int): int {\nreturn value + 1\n}\n",
        )
        .expect("write source");

        let items = extract_doc_items(&[source]).expect("extract docs");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].signature, "pub fn inc(value: int): int {");
        assert_eq!(items[0].kind, "function");
        assert!(items[0].public);
        assert_eq!(items[0].docs, vec![String::from("Adds one.")]);
    }

    #[test]
    fn mutation_report_groups_survivors_stably() {
        let source = r#"{
          "mutants": [
            {"id":"m2","status":"killed","file":"src/main.ax","function":"score","line":8,"mutator":"replace + with -"},
            {"id":"m1","status":"survived","file":"src/main.ax","function":"score","line":7,"mutator":"replace > with >=","description":"boundary branch survived"},
            {"id":"m3","survived":true,"file":"src/main.ax","function":"score","line":9,"operator":"remove call"},
            {"id":"m4","outcome":"survived","source_file":"src/lib.ax","fn":"parse","start_line":3,"kind":"literal replacement"}
          ]
        }"#;

        let report = mutation_report_from_json_str(source).expect("mutation report");

        assert_eq!(report.survivor_count, 3);
        assert_eq!(report.groups.len(), 2);
        assert_eq!(report.groups[0].file, "src/lib.ax");
        assert_eq!(
            report.groups[0].recommended_fixture,
            "mutation_lib_parse_survivors"
        );
        assert_eq!(report.groups[1].file, "src/main.ax");
        assert_eq!(report.groups[1].survivors.len(), 2);
        assert_eq!(report.groups[1].survivors[0].id, "m1");
    }

    #[test]
    fn mutation_report_infers_function_from_source_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source_path = dir.path().join("main.ax");
        fs::write(
            &source_path,
            "fn first(): int {\nreturn 1\n}\n\npub fn second(value: int): int {\nreturn value + 1\n}\n",
        )
        .expect("write source");
        let json = format!(
            r#"[
                {{"id":"m1","status":"survived","file":"{}","line":2}},
                {{"id":"m2","status":"survived","file":"{}","line":6}}
            ]"#,
            source_path.display(),
            source_path.display()
        );

        let report = mutation_report_from_json_str(&json).expect("mutation report");

        assert_eq!(report.groups.len(), 2);
        assert_eq!(report.groups[0].function, "first");
        assert_eq!(report.groups[1].function, "second");
    }

    #[test]
    fn mutation_report_infers_function_from_relative_source_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("src")).expect("create src dir");
        fs::write(
            dir.path().join("src/main.ax"),
            "fn first(): int {\nreturn 1\n}\n\nfn second(): int {\nreturn 2\n}\n",
        )
        .expect("write source");
        let report_path = dir.path().join("mutants.json");
        fs::write(
            &report_path,
            r#"[
                {"id":"m1","status":"survived","file":"src/main.ax","line":2},
                {"id":"m2","status":"survived","file":"src/main.ax","line":6}
            ]"#,
        )
        .expect("write report");

        let report = mutation_report_from_path(&report_path).expect("mutation report");

        assert_eq!(report.groups.len(), 2);
        assert_eq!(report.groups[0].function, "first");
        assert_eq!(report.groups[1].function, "second");
    }

    #[test]
    fn mutation_report_markdown_is_issue_comment_ready() {
        let report = mutation_report_from_json_str(
            r#"[{"id":"m1","status":"survived","file":"src/main.ax","function":"main","mutator":"negate condition","description":"condition still passes"}]"#,
        )
        .expect("mutation report");

        let markdown = render_mutation_issue_report(&report);

        assert!(markdown.contains("## Mutation survivor report"));
        assert!(markdown.contains("Recommended fixture: `mutation_main_main_survivors`"));
        assert!(markdown.contains("- `m1` `negate condition` — condition still passes"));
    }

    #[test]
    fn doc_json_surface_includes_items_and_capabilities() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("doc-json");
        fs::create_dir_all(project.join("src")).expect("mkdir");
        fs::write(
            project.join("axiom.toml"),
            "[package]\nname = \"doc-json\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nenv = true\nenv_vars = [\"AXIOM_ENV\"]\n",
        )
        .expect("write manifest");
        fs::write(project.join("axiom.lock"), "version = 1\n").expect("write lock");
        fs::write(
            project.join("src/main.ax"),
            "/// Handles a request.\n/// Example: route(\"/health\")\npub fn route(path: string): string {\nreturn \"ok\"\n}\n",
        )
        .expect("write source");

        let output = generate_docs(&project, &project.join("docs/api")).expect("generate docs");

        assert_eq!(output.command, "doc");
        assert!(output.ok);
        assert_eq!(output.items.len(), 1);
        assert_eq!(
            output.items[0].examples,
            vec![String::from("route(\"/health\")")]
        );
        assert!(
            output
                .capabilities
                .iter()
                .any(|capability| capability.name == "env")
        );
    }

    #[test]
    fn repl_accepts_lines_and_check_command() {
        let input = b"let answer: int = 42\n:check\n:quit\n";
        let mut output = Vec::new();

        run_repl(&input[..], &mut output, false).expect("run repl");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("accepted: let answer: int = 42"));
        assert!(output.contains("ok: 1 item(s)"));
    }

    #[test]
    fn repl_check_reports_parse_errors() {
        let input = b"let answer: = 42\n:check\n:quit\n";
        let mut output = Vec::new();

        run_repl(&input[..], &mut output, false).expect("run repl");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("accepted: let answer: = 42"));
        assert!(output.contains("error:"));
        assert!(!output.contains("ok:"));
    }
}
