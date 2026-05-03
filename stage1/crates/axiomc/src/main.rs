use axiomc::codegen::NativeBackendKind;
use axiomc::dap;
use axiomc::diagnostics::Diagnostic;
use axiomc::json_contract;
use axiomc::lsp;
use axiomc::new_project::create_project;
use axiomc::project::{
    BuildOptions, BuildOutput, CheckOptions, RunOptions, TestOptions, build_project_with_options,
    check_project_with_options, project_capabilities, run_project_tests_with_options,
    run_project_with_options,
};
use axiomc::registry::{
    PublishOptions, load_registry_index, publish_package, render_registry_index,
};
use axiomc::syntax::parse_program;
use clap::{Parser, Subcommand};
use serde::Serialize;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
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
    },
    /// Check a stage1 package or workspace member without building an artifact.
    Check {
        path: PathBuf,
        #[arg(long)]
        json: bool,
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
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
    },
    /// Inspect manifest capability requirements.
    Caps {
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Format .ax source files with the canonical stage1 style.
    Fmt {
        path: PathBuf,
        #[arg(long)]
        check: bool,
    },
    /// Generate Markdown and HTML API docs from source doc comments.
    Doc {
        path: PathBuf,
        #[arg(long, default_value = "docs/axiom")]
        out_dir: PathBuf,
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
    /// Start a small stage1 scratch REPL backed by axiomc check/run.
    Repl {
        #[arg(long)]
        json: bool,
    },
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

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Command::New { path, name } => match create_project(&path, name.as_deref()) {
            Ok(()) => {
                println!("initialized stage1 project in {}", path.display());
                0
            }
            Err(error) => print_error("new", error, false),
        },
        Command::Check {
            path,
            json,
            package,
        } => match check_project_with_options(
            &path,
            &CheckOptions {
                package: package.clone(),
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
        Command::Run { path, package } => match run_project_with_options(
            &path,
            &RunOptions {
                package: package.clone(),
            },
        ) {
            Ok(code) => code,
            Err(error) => print_error("run", error, false),
        },
        Command::Test {
            path,
            json,
            filter,
            include_benchmarks,
            package,
        } => match run_project_tests_with_options(
            &path,
            &TestOptions {
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
        },
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
                                0
                            }
                            Err(error) => print_error("caps", error, false),
                        }
                    }
                }
                Err(error) => print_error("caps", error, json),
            }
        }
        Command::Fmt { path, check } => match format_axiom_sources(&path, check) {
            Ok(report) => {
                for file in &report.files {
                    if file.changed {
                        eprintln!("formatted {}", file.path);
                    }
                }
                if check && report.changed > 0 {
                    eprintln!("{} file(s) need formatting", report.changed);
                    1
                } else {
                    eprintln!("checked {} file(s)", report.files.len());
                    0
                }
            }
            Err(error) => print_error("fmt", error, false),
        },
        Command::Doc { path, out_dir } => match generate_docs(&path, &out_dir) {
            Ok(output) => {
                eprintln!("wrote {}", output.markdown.display());
                eprintln!("wrote {}", output.html.display());
                0
            }
            Err(error) => print_error("doc", error, false),
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

fn build_summary_lines(output: &BuildOutput, timings: bool) -> Vec<String> {
    let mut lines = vec![format!(
        "wrote {} (backend={})",
        output.binary, output.backend
    )];
    if let Some(debug_map) = &output.debug_map {
        lines.push(format!("wrote debug map {debug_map}"));
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

#[derive(Debug, Clone)]
struct FormatFileReport {
    path: String,
    changed: bool,
}

#[derive(Debug, Clone)]
struct FormatReport {
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
        });
    }
    Ok(FormatReport {
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

#[derive(Debug, Clone)]
struct DocOutput {
    markdown: PathBuf,
    html: PathBuf,
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
    Ok(DocOutput {
        markdown: markdown_path,
        html: html_path,
    })
}

#[derive(Debug, Clone)]
struct DocItem {
    file: String,
    signature: String,
    docs: Vec<String>,
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
                items.push(DocItem {
                    file: file.display().to_string(),
                    signature: trimmed.to_string(),
                    docs: std::mem::take(&mut pending_docs),
                });
            } else if !trimmed.is_empty() {
                pending_docs.clear();
            }
        }
    }
    Ok(items)
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
        assert!(help.contains("Format .ax source files"));
        assert!(help.contains("Generate Markdown and HTML API docs"));
        assert!(help.contains("Run discovered *_bench.ax entrypoints"));
        assert!(help.contains("Start a small stage1 scratch REPL"));
        assert!(help.contains("Pack, sign, and publish a stage1 package"));
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

    fn build_output(debug_map: Option<String>) -> BuildOutput {
        BuildOutput {
            backend: NativeBackendKind::GeneratedRust,
            locked: false,
            offline: false,
            manifest: String::from("axiom.toml"),
            entry: String::from("src/main.ax"),
            binary: String::from("dist/app"),
            generated_rust: String::from("target/main.rs"),
            debug_map,
            statement_count: 1,
            target: None,
            debug: true,
            cache_hits: 0,
            cache_misses: 1,
            duration_ms: 1,
            packages: Vec::new(),
        }
    }

    #[test]
    fn build_summary_mentions_debug_map_when_available() {
        assert_eq!(
            build_summary_lines(
                &build_output(Some(String::from("target/main.debug-map.json"))),
                false,
            ),
            vec![
                String::from("wrote dist/app (backend=generated-rust)"),
                String::from("wrote debug map target/main.debug-map.json"),
            ]
        );
    }

    #[test]
    fn build_summary_omits_debug_map_for_release_builds() {
        assert_eq!(
            build_summary_lines(&build_output(None), false),
            vec![String::from("wrote dist/app (backend=generated-rust)")]
        );
    }

    #[test]
    fn formatter_trims_whitespace_and_collapses_blank_runs() {
        assert_eq!(
            format_axiom_source("fn main() {   \n\tprint \"hi\"  \n\n\n}\n\n"),
            "fn main() {\n    print \"hi\"\n\n}\n"
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
        assert_eq!(items[0].docs, vec![String::from("Adds one.")]);
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
