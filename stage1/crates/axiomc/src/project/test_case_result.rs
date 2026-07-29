use super::{BuildLoweringEvidence, TestKind};
use crate::diagnostics::Diagnostic;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct TestCaseResult {
    pub package_root: String,
    pub name: String,
    pub kind: TestKind,
    pub entry: String,
    pub ok: bool,
    pub binary: Option<String>,
    pub generated_rust: Option<String>,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub expected_stdout: Option<String>,
    pub expected_stderr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_error: Option<ExpectedDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lowering: Option<BuildLoweringEvidence>,
    pub duration_ms: u64,
    pub error: Option<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExpectedDiagnostic {
    pub kind: String,
    pub code: Option<String>,
    pub message: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
}
