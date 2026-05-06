use axiomc::codegen::NativeBackendKind;
use axiomc::dap;
use axiomc::diagnostic_catalog::{DiagnosticCodeInfo, diagnostic_code_info};
use axiomc::diagnostics::Diagnostic;
use axiomc::json_contract;
use axiomc::lsp;
use axiomc::manifest::CapabilityDescriptor;
>>>>>>> origin/codex/issue-376-doctor-json
>>>>>>> origin/codex/issue-377-inspect-symbols
use axiomc::lockfile::{expected_lockfile_for_project, validate_lockfile};
use axiomc::manifest::{load_manifest, manifest_path};
>>>>>>> origin/codex/issue-406-collection-lookup
>>>>>>> origin/codex/agent-g-regex
>>>>>>> origin/codex/agent-f-fs
>>>>>>> origin/codex/agent-i-language-slice
>>>>>>> origin/codex/issue-387-capability-validation
>>>>>>> origin/codex/issue-395-effective-fs-roots
>>>>>>> origin/codex/worker-h-issue-413
>>>>>>> origin/codex/worker-j-issue-362
>>>>>>> origin/codex/worker-j-issue-363
>>>>>>> origin/codex/issue-369-check-fixtures
>>>>>>> origin/codex/issue-370-command-fixtures
>>>>>>> origin/codex/issue-418-schema-metadata
>>>>>>> origin/codex/issue-422-comparison-gate
>>>>>>> origin/codex/issue-425-crap-thresholds
>>>>>>> origin/codex/issue-423-mutation-smoke
>>>>>>> origin/codex/issue-424-survivor-report
>>>>>>> origin/codex/issue-409-proof-cli
>>>>>>> origin/codex/issue-410-proof-worker
>>>>>>> origin/codex/worker-f-issue-341
>>>>>>> origin/codex/worker-f-issue-343
>>>>>>> origin/codex/worker-c-issue-361
>>>>>>> origin/codex/agent-o-debug-info
use axiomc::new_project::create_project;
use axiomc::diagnostics::Diagnostic;
use axiomc::json_contract;
use axiomc::lsp;
use axiomc::new_project::{WorkloadTemplate, create_project_with_template};
use axiomc::project::{
    BuildOptions, BuildOutput, CheckOptions, RunOptions, TestOptions, build_project_with_options,
    check_project_with_options, project_capabilities, run_project_tests_with_options,
    run_project_with_options,
};
use axiomc::registry::{
    PublishOptions, load_registry_index, publish_package, render_registry_index,
};
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
use axiomc::diagnostics::Diagnostic;
use axiomc::json_contract;
use axiomc::lsp;
use axiomc::new_project::create_project;
use axiomc::project::{
    build_project_with_options, check_project_with_options, list_project_tests_with_options,
    build_project_with_options, check_project_with_options, package_graph_metadata,
    project_capabilities, run_project_tests_with_options, run_project_with_options, BuildOptions,
    BuildOutput, CheckOptions, RunOptions, TestOptions,
};
use axiomc::registry::{load_registry_index, render_registry_index};
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
use axiomc::syntax::parse_program;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
>>>>>>> origin/codex/issue-424-survivor-report
>>>>>>> origin/codex/worker-f-issue-341
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
    /// Check a stage1 package or workspace member without building an artifact.
    Check {
        path: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        exports: bool,
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
        #[arg(long)]
        locked: bool,
        #[arg(long)]
        offline: bool,
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
    /// Discover, build, and run package test entrypoints.
    Test {
        path: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        list: bool,
        #[arg(long)]
        filter: Option<String>,
        #[arg(long)]
        include_benchmarks: bool,
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
    },
    /// Inspect manifest capability requirements.
    Caps {
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
        #[command(subcommand)]
        command: Option<CapsCommand>,
    },
    /// Report local stage1 project and toolchain health.
    Doctor {
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Inspect project metadata for agent tooling.
    Inspect {
        #[command(subcommand)]
        command: InspectCommand,
    },
    /// Inspect project metadata for agent tooling.
    Inspect {
        #[command(subcommand)]
        command: InspectCommand,
    },
    /// Explain a stable diagnostic code.
    Explain {
        code: String,
        #[arg(long)]
        json: bool,
    },
    /// Inspect package metadata and resolved local package graph.
    Pkg {
        #[command(subcommand)]
        command: PkgCommand,
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
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
>>>>>>> origin/codex/agent-i-language-slice
>>>>>>> origin/codex/issue-387-capability-validation
>>>>>>> origin/codex/issue-395-effective-fs-roots
>>>>>>> origin/codex/worker-h-issue-413
>>>>>>> origin/codex/worker-j-issue-362
>>>>>>> origin/codex/worker-j-issue-363
>>>>>>> origin/codex/issue-369-check-fixtures
>>>>>>> origin/codex/issue-370-command-fixtures
>>>>>>> origin/codex/issue-418-schema-metadata
>>>>>>> origin/codex/issue-422-comparison-gate
>>>>>>> origin/codex/issue-425-crap-thresholds
=======
=======
>>>>>>> origin/codex/worker-c-issue-361
=======
>>>>>>> origin/codex/agent-o-debug-info
    /// Pack, sign, and publish a stage1 package into a local registry tree.
    Publish {
        path: PathBuf,
        #[arg(long = "registry-dir")]
        registry_dir: PathBuf,
        #[arg(long = "signing-key")]
        signing_key: Option<String>,
        #[arg(long)]
        allow_overwrite: bool,
    },
=======
=======
>>>>>>> origin/codex/worker-h-issue-414
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
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
}

<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
#[derive(Debug, Subcommand)]
enum CapsCommand {
    /// Diff two caps JSON payloads and fail on capability escalation.
    Diff { old: PathBuf, new: PathBuf },
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
}

#[derive(Debug, Subcommand)]
enum InspectCommand {
    /// Emit exported functions, types, consts, imports, and capability use.
    Symbols {
    /// Emit package and module dependency graph details.
}

#[derive(Debug, Subcommand)]
enum PkgCommand {
    /// Print resolved packages, members, dependencies, entrypoints, capabilities, and lockfile status.
    Graph {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
>>>>>>> origin/codex/worker-h-issue-414
=======
>>>>>>> origin/codex/agent-o-debug-info
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
        Command::Check {
            path,
            json,
            exports,
            package,
        } => match check_project_with_options(
            &path,
            &CheckOptions {
                package: package.clone(),
                include_exports: exports,
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
            locked,
            offline,
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
        Command::Test {
            path,
            json,
            list,
            filter,
            include_benchmarks,
            package,
        } => {
            let options = TestOptions {
                filter: filter.clone(),
                package: package.clone(),
                include_benchmarks,
            },
        ) {
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
                        eprintln!("{status} {:?} {} ({})", case.kind, case.name, case.entry);
                        if let Some(error) = &case.error {
                            eprintln!("  {}", error);
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
                            for case in &output.cases {
                                let package =
                                    case.package.as_deref().unwrap_or(&case.package_root);
                                println!("{package}\t{}\t{}", case.name, case.entry);
                            }
                            eprintln!("listed: {} test(s)", output.total);
                        }
                        0
                    }
                    Err(error) => print_error("test", error, json),
                }
                if ok { 0 } else { 1 }
>>>>>>> origin/codex/worker-h-issue-414
                if ok {
                    0
                } else {
                    1
                }
            }
            Err(error) => print_error("test", error, json),
        },
        Command::Caps {
            path,
            json,
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
                                eprintln!("{status} {} ({})", case.name, case.entry);
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
        Command::Caps { path, json } => {
            let project = path.unwrap_or_else(|| PathBuf::from("."));
            match project_capabilities(&project) {
                Ok(capabilities) => {
                    if json {
                        println!("{}", json_contract::caps_success(&project, &capabilities));
                        0
                    } else {
                        let payload = json_contract::caps_success(&project, &capabilities);
                        match json_contract::to_pretty_string(&payload) {
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
                match project_capabilities(&project) {
                    Ok(capabilities) => {
                        if json {
                            println!("{}", json_contract::caps_success(&project, &capabilities));
                            0
                        } else {
                            let payload = json_contract::caps_success(&project, &capabilities);
                            match json_contract::to_pretty_string(&payload) {
                                Ok(output) => {
                                    println!("{output}");
                                    0
                                }
                                Err(error) => print_error("caps", error, false),
                            }
                        }
                    }
                    Err(error) => print_error("caps", error, json),
                }
            }
        },
        }
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
        Command::Inspect { command } => match command {
            InspectCommand::Symbols { path, json } => match inspect_symbols(&path) {
            InspectCommand::Graph { path, json } => match inspect_graph(&path) {
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
                        println!(
                            "packages={} modules={} import_errors={}",
                            report.packages.len(),
                            report.modules.len(),
                            report.import_errors.len()
                        );
                    }
                    0
                }
                Err(error) => print_error("inspect graph", error, json),
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
>>>>>>> origin/codex/issue-423-mutation-smoke
>>>>>>> origin/codex/issue-424-survivor-report
>>>>>>> origin/codex/worker-f-issue-341
        Command::Pkg { command } => match command {
            PkgCommand::Graph { path, json } => match package_graph_metadata(&path) {
                Ok(output) => {
                    if json {
                        match serde_json::to_string(&output) {
                            Ok(output) => {
                                println!("{output}");
                                0
                            }
                            Err(error) => print_error(
                                "pkg graph",
                                Diagnostic::new(
                                    "json",
                                    format!("failed to serialize package graph JSON: {error}"),
                                ),
                                false,
                            ),
                        }
                    } else {
                        match json_contract::to_pretty_string(&output) {
                            Ok(output) => {
                                println!("{output}");
                                0
                            }
                            Err(error) => print_error("pkg graph", error, false),
                        }
                    }
                }
                Err(error) => print_error("pkg graph", error, json),
            },
        },
        Command::Fmt { path, check } => match format_axiom_sources(&path, check) {
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
                    if check && report.changed > 0 {
                        1
                    } else {
                        0
                    }
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
>>>>>>> origin/codex/worker-h-issue-414
                if report.failed == 0 {
                    0
                } else {
                    1
                }
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
<<<<<<< HEAD
>>>>>>> origin/codex/agent-o-debug-info
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
=======
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
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
=======
=======
=======
=======
=======
=======
    };
    std::process::exit(code);
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
struct DoctorReport {
struct InspectSymbolsReport {
struct InspectGraphReport {
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

fn inspect_symbols(path: &Path) -> Result<InspectSymbolsReport, Diagnostic> {
    let files = axiom_files(path)?;
    let mut symbols = Vec::new();
    lockfile_status: &'static str,
    lockfile_packages: Vec<LockfilePackageReport>,
    packages: Vec<PackageNode>,
    modules: Vec<ModuleNode>,
    stdlib_modules: Vec<&'static str>,
    cycles: Vec<Vec<String>>,
    import_errors: Vec<ImportErrorReport>,
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
}

#[derive(Debug, Clone, Serialize)]
struct ModuleImport {
    path: String,
    resolved: Option<String>,
    is_stdlib: bool,
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
    let packages = package_nodes(project, &manifest);
    let (modules, import_errors) = module_nodes(project, &manifest)?;
    let cycles = module_cycles(&modules);

    Ok(InspectGraphReport {
        schema_version: json_contract::JSON_SCHEMA_VERSION,
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
    })
}

fn package_nodes(project: &Path, manifest: &axiomc::manifest::Manifest) -> Vec<PackageNode> {
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

fn module_nodes(
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
    let stdlib = stdlib_module_set();
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
        ok: true,
        command: "inspect symbols",
        project: path.display().to_string(),
        symbols,
    })
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
        TypeName::Bool => "bool".to_string(),
        TypeName::String => "string".to_string(),
        TypeName::Named(name, args) if args.is_empty() => name.clone(),
        TypeName::Named(name, args) => format!(
            "{}<{}>",
            name,
            args.iter().map(render_type).collect::<Vec<_>>().join(", ")
        ),
        TypeName::Ptr(inner) => format!("ptr<{}>", render_type(inner)),
        TypeName::MutPtr(inner) => format!("mut ptr<{}>", render_type(inner)),
        TypeName::Slice(inner) => format!("&[{}]", render_type(inner)),
        TypeName::MutSlice(inner) => format!("&mut [{}]", render_type(inner)),
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
        TypeName::Array(inner) => format!("[{}]", render_type(inner)),
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
        Expr::Try { expr, .. } | Expr::Await { expr, .. } => {
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
        Expr::Literal(_) | Expr::VarRef { .. } => {}
    }
}

fn capability_for_call(name: &str) -> Option<&'static str> {
    match name {
        "clock_now_ms" | "clock_elapsed_ms" | "clock_sleep_ms" => Some("clock"),
        "env_get" => Some("env"),
        "fs_read" => Some("fs"),
        "net_resolve"
        | "http_get"
        | "net_tcp_listen_loopback_once"
        | "tcp_listen_loopback_once"
        | "net_tcp_dial"
        | "tcp_dial"
        | "net_udp_bind_loopback_once"
        | "udp_bind_loopback_once"
        | "net_udp_send_recv"
        | "udp_send_recv" => Some("net"),
        "process_status" => Some("process"),
        "crypto_sha256" => Some("crypto"),
        _ => None,
    }
        let mut imports = Vec::new();
        for import in program.imports {
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
                    path: import.path,
                    resolved: None,
                    is_stdlib: true,
                });
                continue;
            }
            let candidate = dependency_import_candidate(&dependencies, &import.path).unwrap_or_else(
                || {
                    file.parent()
                        .map(|parent| parent.join(&import.path))
                        .unwrap_or_else(|| PathBuf::from(&import.path))
                },
            );
            let resolved = normalize_for_graph(&candidate);
            if !known.contains(&resolved) {
                errors.push(ImportErrorReport {
                    module: file.display().to_string(),
                    import: import.path.clone(),
                    message: format!("missing import {}", candidate.display()),
                });
            }
            imports.push(ModuleImport {
                path: import.path,
                resolved: Some(resolved),
                is_stdlib: false,
            });
        }
        modules.push(ModuleNode {
            path: normalize_for_graph(&file),
            imports,
        });
    }
    modules.sort_by(|left, right| left.path.cmp(&right.path));
    Ok((modules, errors))
}

fn dependency_import_candidate(
    dependencies: &HashMap<&str, PathBuf>,
    import: &str,
) -> Option<PathBuf> {
    let (dependency, rest) = import.split_once('/')?;
    dependencies.get(dependency).map(|source_root| source_root.join(rest))
}

fn stdlib_module_set() -> BTreeSet<&'static str> {
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

fn module_cycles(modules: &[ModuleNode]) -> Vec<Vec<String>> {
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
        find_cycles(node, node, &graph, &mut stack, &mut cycles);
    }
    cycles.sort();
    cycles.dedup();
    cycles
}

fn find_cycles(
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
            cycles.push(canonical_cycle(cycle));
        } else if graph.contains_key(next) {
            find_cycles(start, next, graph, stack, cycles);
        }
    }
    stack.pop();
}

fn canonical_cycle(mut cycle: Vec<String>) -> Vec<String> {
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
        "{code} ({kind})\n{title}\n\n{explanation}\n\nExample:\n{example}\n\nSuggested fix:\n{suggested_fix}",
        code = info.code,
        kind = info.kind,
        title = info.title,
        explanation = info.explanation,
        example = info.example,
        suggested_fix = info.suggested_fix,
    )
}
#[derive(Debug, Clone)]
struct FormatFileReport {
    path: String,
    changed: bool,
    edits: Vec<FormatEdit>,
}

#[derive(Debug, Clone, Serialize)]
struct FormatReport {
    schema_version: &'static str,
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

#[derive(Debug, Clone)]
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
        assert!(help.contains("Report local stage1 project and toolchain health"));
        assert!(help.contains("Inspect project metadata for agent tooling"));
        assert!(help.contains("Explain a stable diagnostic code"));
        assert!(help.contains("Format .ax source files"));
        assert!(help.contains("Generate Markdown and HTML API docs"));
        assert!(help.contains("Run discovered *_bench.ax entrypoints"));
        assert!(help.contains("Start a small stage1 scratch REPL"));
        assert!(help.contains("Pack, sign, and publish a stage1 package"));
>>>>>>> origin/codex/issue-381-test-list
>>>>>>> origin/codex/issue-408-cli-args
>>>>>>> origin/codex/worker-h-issue-414
        assert!(help.contains("Build a static package-registry index"));
        assert!(help.contains("Validate a static package-registry index JSON file"));
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
<<<<<<< HEAD
<<<<<<< HEAD
        assert!(rendered.contains("only generated-rust is implemented in this preparatory backend plumbing"));
<<<<<<< HEAD
<<<<<<< HEAD
=======
=======
=======
=======
>>>>>>> origin/codex/agent-o-debug-info
    }

    fn build_output(debug_map: Option<String>, debug_manifest: Option<String>) -> BuildOutput {
        BuildOutput {
            backend: NativeBackendKind::GeneratedRust,
            locked: false,
            offline: false,
>>>>>>> origin/codex/issue-381-test-list
>>>>>>> origin/codex/issue-408-cli-args
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
>>>>>>> origin/codex/issue-369-check-fixtures
>>>>>>> origin/codex/issue-370-command-fixtures
>>>>>>> origin/codex/issue-418-schema-metadata
>>>>>>> origin/codex/issue-422-comparison-gate
>>>>>>> origin/codex/issue-425-crap-thresholds
>>>>>>> origin/codex/issue-423-mutation-smoke
>>>>>>> origin/codex/issue-424-survivor-report
>>>>>>> origin/codex/issue-409-proof-cli
>>>>>>> origin/codex/issue-410-proof-worker
>>>>>>> origin/codex/worker-f-issue-341
>>>>>>> origin/codex/worker-f-issue-343
>>>>>>> origin/codex/worker-c-issue-361
>>>>>>> origin/codex/worker-h-issue-414
>>>>>>> origin/codex/agent-o-debug-info
            manifest: String::from("axiom.toml"),
            entry: String::from("src/main.ax"),
            binary: String::from("dist/app"),
            generated_rust: String::from("target/main.rs"),
            debug_map,
            debug_manifest,
            statement_count: 1,
            target: None,
            debug: true,
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
=======
=======
=======
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
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
=======
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
<<<<<<< HEAD
    fn build_json_includes_target_debug_and_cache_key_metadata() {
        let payload = json_contract::build_success(
            Path::new("stage1/examples/hello"),
            &build_output(Some(String::from("target/main.debug-map.json"))),
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
    fn build_summary_mentions_debug_map_when_available() {
=======
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
    fn build_summary_omits_debug_map_for_release_builds() {
        assert_eq!(
            build_summary_lines(&build_output(None), false),
            vec![String::from("wrote dist/app (backend=generated-rust)")]
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
=======
=======
=======
=======
=======
=======
=======
            build_summary_lines(&build_output(None, None), false),
            vec![String::from("wrote dist/app (backend=generated-rust)")]
        );
    }

    #[test]
    fn inspect_symbols_reports_public_symbols_and_capabilities() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source_dir = dir.path().join("src");
        fs::create_dir_all(&source_dir).expect("create source dir");
        fs::write(
            source_dir.join("main.ax"),
            "import \"time.ax\"\n\npub const LIMIT: int = 3\n\npub struct Job {\nname: string\n}\n\npub fn now(): int {\nreturn clock_now_ms()\n}\n\npub fn dial(): int {\nreturn net_tcp_dial(\"127.0.0.1\", 80)\n}\n\npub fn slice_time(values: [int]): [int] {\nreturn values[0:clock_now_ms()]\n}\n\nfn private_helper(): int {\nreturn 1\n}\n",
        )
        .expect("write main source");
        fs::write(
            source_dir.join("time.ax"),
            "pub fn exported(): int {\nreturn 7\n}\n",
        )
        .expect("write imported source");

        let report = inspect_symbols(dir.path()).expect("inspect symbols");

        assert_eq!(report.command, "inspect symbols");
        assert_eq!(report.symbols.len(), 6);
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
    fn doctor_reports_project_health_json_fields() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("doctor");
        create_project(&project, Some("doctor-app")).expect("create project");

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
    }

    #[test]
    fn rustc_host_target_parser_reads_verbose_version_output() {
        let version = "rustc 1.90.0\nhost: aarch64-apple-darwin\nrelease: 1.90.0\n";

        assert_eq!(
            parse_rustc_host_target(version).as_deref(),
            Some("aarch64-apple-darwin")
=======
=======
=======
=======
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
    fn inspect_graph_detects_local_module_cycles() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("cycle");
        create_project(&project, Some("cycle-app")).expect("create project");
        fs::write(project.join("src/main.ax"), "import \"a.ax\"\n").expect("write main source");
        fs::write(project.join("src/a.ax"), "import \"b.ax\"\n").expect("write a source");
        fs::write(project.join("src/b.ax"), "import \"a.ax\"\n").expect("write b source");

        let report = inspect_graph(&project).expect("inspect graph");

        assert!(!report.cycles.is_empty());
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
        assert!(report.files[0]
            .edits
            .iter()
            .any(|edit| edit.action == "replace_line" && edit.line == 1));
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
        assert_eq!(output.items[0].examples, vec![String::from("route(\"/health\")")]);
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
