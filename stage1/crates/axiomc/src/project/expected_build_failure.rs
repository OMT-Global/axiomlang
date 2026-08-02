//! Expected build-failure fixture execution for fail-closed backend coverage.

use super::*;

pub(super) fn run_build_fail_case(
    project_root: &Path,
    graph: &PackageGraph,
    manifest: &Manifest,
    case_name: &str,
    kind: TestKind,
) -> TestCaseResult {
    let started = Instant::now();
    let expected = match load_expected_build_error(project_root) {
        Ok(expected) => expected,
        Err(error) => {
            return failed_result(
                project_root, manifest, case_name, kind, &started, None, error,
                None,
            );
        }
    };
    let entry_path = project_root.join(&manifest.build.entry);
    let (lowering, actual) = match analyze_package(graph, project_root) {
        Ok(analyzed) => {
            let generated_rust = generated_rust_path(project_root, manifest);
            let binary = binary_path_for_target(project_root, manifest, None);
            match build_artifacts(
                graph,
                project_root,
                &analyzed,
                &generated_rust,
                &binary,
                None,
                &BuildOptions::default(),
            ) {
                Ok(report) => (
                    Some(report.lowering),
                    Diagnostic::new(
                        "test",
                        "expected native build to fail closed, but it succeeded",
                    )
                    .with_path(entry_path.display().to_string()),
                ),
                Err(error) => (
                    runtime_lowering_blocked_evidence(&error),
                    diagnostic_with_default_path(error, &entry_path),
                ),
            }
        }
        Err(error) => (
            None,
            diagnostic_with_default_path(error, &entry_path),
        ),
    };
    let mismatch = expected_error_mismatch(project_root, &expected, &actual);
    TestCaseResult {
        package_root: project_root.display().to_string(),
        name: case_name.to_string(),
        kind,
        entry: manifest.build.entry.clone(),
        ok: mismatch.is_none(),
        binary: None,
        generated_rust: None,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        expected_stdout: None,
        expected_stderr: None,
        expected_error: Some(expected),
        lowering,
        duration_ms: started.elapsed().as_millis() as u64,
        error: mismatch.map(|message| Diagnostic::new("test", message)),
    }
}

fn failed_result(
    project_root: &Path,
    manifest: &Manifest,
    case_name: &str,
    kind: TestKind,
    started: &Instant,
    expected: Option<ExpectedDiagnostic>,
    error: Diagnostic,
    lowering: Option<BuildLoweringEvidence>,
) -> TestCaseResult {
    TestCaseResult {
        package_root: project_root.display().to_string(),
        name: case_name.to_string(),
        kind,
        entry: manifest.build.entry.clone(),
        ok: false,
        binary: None,
        generated_rust: None,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        expected_stdout: None,
        expected_stderr: None,
        expected_error: expected,
        lowering,
        duration_ms: started.elapsed().as_millis() as u64,
        error: Some(error),
    }
}

pub(super) fn expected_build_error_path(project_root: &Path) -> PathBuf {
    project_root.join("expected-build-error.json")
}

fn load_expected_build_error(project_root: &Path) -> Result<ExpectedDiagnostic, Diagnostic> {
    let path = expected_build_error_path(project_root);
    let content = fs::read_to_string(&path).map_err(|err| {
        Diagnostic::new("test", format!("failed to read {}: {err}", path.display()))
            .with_path(path.display().to_string())
    })?;
    serde_json::from_str(&content).map_err(|err| {
        Diagnostic::new(
            "test",
            format!(
                "invalid expected-build-error.json at {}: {err}",
                path.display()
            ),
        )
        .with_path(path.display().to_string())
    })
}
