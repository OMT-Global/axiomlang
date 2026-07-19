//! Execution-backed benchmark discovery, sampling, and baseline comparison.

use axiomc::diagnostics::Diagnostic;
use axiomc::manifest::TestKind;
use axiomc::project::{run_project_tests_with_options, TestOptions};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const BASELINE_SCHEMA_VERSION: &str = "axiom.stage1.bench.baseline.v1";

#[derive(Debug, Clone)]
pub struct BenchOptions {
    pub warmup: usize,
    pub iterations: usize,
    pub baseline: Option<PathBuf>,
    pub max_regression_percent: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchReport {
    pub schema_version: &'static str,
    pub command: &'static str,
    pub warmup: usize,
    pub iterations: usize,
    pub baseline: Option<String>,
    pub max_regression_percent: f64,
    pub benches: Vec<BenchResult>,
    pub passed: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchResult {
    pub id: String,
    pub path: String,
    pub warmup: usize,
    pub iterations: usize,
    pub samples_ms: Vec<u64>,
    pub median_ms: u64,
    pub p95_ms: u64,
    pub variance_ms2: f64,
    /// Stage1 does not currently expose a portable allocation counter.
    pub allocations: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_median_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regression_percent: Option<f64>,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchBaseline {
    schema_version: String,
    version: u64,
    benches: Vec<BenchBaselineEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchBaselineEntry {
    id: String,
    median_ms: u64,
}

pub fn run_benchmarks(
    project_root: &Path,
    options: &BenchOptions,
) -> Result<BenchReport, Diagnostic> {
    if options.iterations == 0 {
        return Err(Diagnostic::new(
            "bench",
            "iterations must be greater than zero",
        ));
    }
    if !options.max_regression_percent.is_finite() || options.max_regression_percent < 0.0 {
        return Err(Diagnostic::new(
            "bench",
            "max regression percent must be a finite non-negative number",
        ));
    }

    let baselines = match &options.baseline {
        Some(path) => load_baseline(path)?,
        None => Vec::new(),
    };
    let benchmarks = discover_benchmarks(project_root)?;
    if benchmarks.is_empty() {
        return Err(Diagnostic::new(
            "bench",
            format!("no *_bench.ax files found under {}", project_root.display()),
        ));
    }

    let mut benches = Vec::with_capacity(benchmarks.len());
    for benchmark in benchmarks {
        let baseline = baselines
            .iter()
            .find(|entry| entry.id == benchmark.id)
            .map(|entry| entry.median_ms);
        benches.push(run_one_benchmark(
            project_root,
            benchmark,
            options,
            baseline,
        ));
    }
    let passed = benches.iter().filter(|bench| bench.ok).count();
    Ok(BenchReport {
        schema_version: "axiom.stage1.bench.v1",
        command: "bench",
        warmup: options.warmup,
        iterations: options.iterations,
        baseline: options
            .baseline
            .as_ref()
            .map(|path| path.display().to_string()),
        max_regression_percent: options.max_regression_percent,
        failed: benches.len().saturating_sub(passed),
        passed,
        benches,
    })
}

#[derive(Debug, Clone)]
struct DiscoveredBenchmark {
    id: String,
    path: PathBuf,
}

fn run_one_benchmark(
    project_root: &Path,
    benchmark: DiscoveredBenchmark,
    options: &BenchOptions,
    baseline: Option<u64>,
) -> BenchResult {
    let mut failure = None;
    for _ in 0..options.warmup {
        if let Err(error) = execute_benchmark(project_root, &benchmark.id) {
            failure = Some(error.to_string());
            break;
        }
    }

    let mut samples_ms = Vec::with_capacity(options.iterations);
    if failure.is_none() {
        for _ in 0..options.iterations {
            match execute_benchmark(project_root, &benchmark.id) {
                Ok(duration_ms) => samples_ms.push(duration_ms),
                Err(error) => {
                    failure = Some(error.to_string());
                    break;
                }
            }
        }
    }
    let mut ordered_samples = samples_ms.clone();
    ordered_samples.sort_unstable();
    let median_ms = ordered_samples
        .get(ordered_samples.len() / 2)
        .copied()
        .unwrap_or_default();
    let p95_index = ((ordered_samples.len() * 95).div_ceil(100)).saturating_sub(1);
    let p95_ms = ordered_samples
        .get(p95_index.min(ordered_samples.len().saturating_sub(1)))
        .copied()
        .unwrap_or_default();
    let variance_ms2 = sample_variance(&samples_ms);
    let regression_percent = baseline.and_then(|baseline_ms| {
        (baseline_ms > 0).then(|| ((median_ms as f64 / baseline_ms as f64) - 1.0) * 100.0)
    });
    if failure.is_none()
        && exceeds_regression_limit(regression_percent, options.max_regression_percent)
    {
        failure = Some(format!(
            "benchmark regression {:.2}% exceeds the accepted {:.2}% threshold",
            regression_percent.unwrap_or_default(),
            options.max_regression_percent
        ));
    }
    BenchResult {
        id: benchmark.id,
        path: benchmark.path.display().to_string(),
        warmup: options.warmup,
        iterations: options.iterations,
        samples_ms,
        median_ms,
        p95_ms,
        variance_ms2,
        allocations: None,
        baseline_median_ms: baseline,
        regression_percent,
        ok: failure.is_none(),
        error: failure,
    }
}

fn execute_benchmark(project_root: &Path, benchmark_id: &str) -> Result<u64, Diagnostic> {
    let output = run_project_tests_with_options(
        project_root,
        &TestOptions {
            filter: Some(benchmark_id.to_string()),
            include_benchmarks: true,
            ..TestOptions::default()
        },
    )?;
    let cases = output
        .cases
        .iter()
        .filter(|case| case.kind == TestKind::Benchmark)
        .collect::<Vec<_>>();
    if cases.len() != 1 {
        return Err(Diagnostic::new(
            "bench",
            format!(
                "benchmark {benchmark_id:?} resolved to {} benchmark entrypoints, expected exactly one",
                cases.len()
            ),
        ));
    }
    let case = cases[0];
    if !case.ok {
        return Err(case.error.clone().unwrap_or_else(|| {
            Diagnostic::new("bench", format!("benchmark {benchmark_id:?} failed"))
        }));
    }
    Ok(case.duration_ms)
}

fn load_baseline(path: &Path) -> Result<Vec<BenchBaselineEntry>, Diagnostic> {
    let source = fs::read_to_string(path).map_err(|error| {
        Diagnostic::new(
            "bench",
            format!(
                "failed to read benchmark baseline {}: {error}",
                path.display()
            ),
        )
        .with_path(path.display().to_string())
    })?;
    let baseline = serde_json::from_str::<BenchBaseline>(&source).map_err(|error| {
        Diagnostic::new(
            "bench",
            format!("invalid benchmark baseline {}: {error}", path.display()),
        )
        .with_path(path.display().to_string())
    })?;
    if baseline.schema_version != BASELINE_SCHEMA_VERSION || baseline.version != 1 {
        return Err(Diagnostic::new(
            "bench",
            format!(
                "benchmark baseline {} has an unsupported schema version",
                path.display()
            ),
        )
        .with_path(path.display().to_string()));
    }
    let mut ids = baseline
        .benches
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    if ids.len() != baseline.benches.len()
        || baseline.benches.iter().any(|entry| entry.median_ms == 0)
    {
        return Err(Diagnostic::new(
            "bench",
            format!(
                "benchmark baseline {} has duplicate ids or zero medians",
                path.display()
            ),
        )
        .with_path(path.display().to_string()));
    }
    Ok(baseline.benches)
}

fn discover_benchmarks(project_root: &Path) -> Result<Vec<DiscoveredBenchmark>, Diagnostic> {
    let src = project_root.join("src");
    let mut paths = Vec::new();
    discover_benchmark_paths(&src, &mut paths)?;
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let relative = path.strip_prefix(project_root).map_err(|_| {
                Diagnostic::new(
                    "bench",
                    format!("benchmark {} escaped project root", path.display()),
                )
            })?;
            Ok(DiscoveredBenchmark {
                id: relative
                    .with_extension("")
                    .to_string_lossy()
                    .replace('\\', "/"),
                path,
            })
        })
        .collect()
}

fn discover_benchmark_paths(path: &Path, paths: &mut Vec<PathBuf>) -> Result<(), Diagnostic> {
    if !path.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(path).map_err(|error| {
        Diagnostic::new(
            "bench",
            format!("failed to read {}: {error}", path.display()),
        )
        .with_path(path.display().to_string())
    })? {
        let entry = entry.map_err(|error| {
            Diagnostic::new(
                "bench",
                format!("failed to inspect {}: {error}", path.display()),
            )
        })?;
        let entry_path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            Diagnostic::new(
                "bench",
                format!("failed to inspect {}: {error}", entry_path.display()),
            )
        })?;
        if file_type.is_dir() {
            discover_benchmark_paths(&entry_path, paths)?;
        } else if file_type.is_file()
            && entry_path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("_bench.ax"))
        {
            paths.push(entry_path);
        }
    }
    Ok(())
}

fn sample_variance(samples: &[u64]) -> f64 {
    if samples.len() < 2 {
        return 0.0;
    }
    let mean = samples.iter().map(|sample| *sample as f64).sum::<f64>() / samples.len() as f64;
    samples
        .iter()
        .map(|sample| (*sample as f64 - mean).powi(2))
        .sum::<f64>()
        / (samples.len() - 1) as f64
}

fn exceeds_regression_limit(regression_percent: Option<f64>, max_regression_percent: f64) -> bool {
    regression_percent.is_some_and(|percent| percent > max_regression_percent)
}

#[cfg(test)]
mod tests {
    use super::{exceeds_regression_limit, sample_variance};
    use serde_json::Value;

    #[test]
    fn sample_variance_uses_the_unbiased_denominator() {
        assert_eq!(sample_variance(&[1, 3]), 2.0);
        assert_eq!(sample_variance(&[7]), 0.0);
    }

    #[test]
    fn regression_gate_rejects_only_values_over_the_accepted_limit() {
        assert!(!exceeds_regression_limit(None, 20.0));
        assert!(!exceeds_regression_limit(Some(20.0), 20.0));
        assert!(exceeds_regression_limit(Some(20.01), 20.0));
    }

    #[test]
    fn checked_in_baseline_matches_its_versioned_schema() {
        let schema: Value = serde_json::from_str(include_str!(
            "../../../schemas/axiom-benchmark-baseline-v1.schema.json"
        ))
        .expect("benchmark baseline schema is JSON");
        let baseline: Value = serde_json::from_str(include_str!(
            "../../../benchmarks/baselines/axiomc-bench-v1.json"
        ))
        .expect("benchmark baseline fixture is JSON");
        jsonschema::validator_for(&schema)
            .expect("compile benchmark baseline schema")
            .validate(&baseline)
            .expect("baseline fixture matches schema");
    }
}
