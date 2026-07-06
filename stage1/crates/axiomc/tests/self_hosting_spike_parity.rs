//! Parity evidence for the self-hosting feasibility spike (#1253).
//!
//! The AxiOM package at `stage1/selfhost/compiler-diagnostics-spike` mirrors a
//! slice of `axiomc::diagnostics`. Its driver prints one labeled result per
//! line; this test computes the same labeled lines from the Rust
//! implementation and compares both against the checked-in
//! `expected-output.txt`. `scripts/ci/run-self-hosting-spike-parity.sh` runs
//! the AxiOM side through `axiomc run` on the direct-native backend and diffs
//! its stdout against the same file, so the two implementations are proven
//! equivalent over the corpus.
//!
//! The case list below intentionally duplicates the driver corpus in
//! `stage1/selfhost/compiler-diagnostics-spike/src/main.ax`; if either side
//! changes without the other, the shared expected file diff fails.
//!
//! Regenerate with:
//! `AXIOM_UPDATE_SPIKE_EXPECTED=1 cargo test -p axiomc --test self_hosting_spike_parity`

use axiomc::diagnostics::{
    edit_distance, is_plausible_suggestion, repair_hint, stable_diagnostic_code, Diagnostic,
};
use std::fs;
use std::path::Path;

const STABLE_CODE_CASES: &[(&str, &str, &str)] = &[
    ("parse missing-closing", "parse", "missing closing brace after block"),
    ("parse unexpected-closing", "parse", "unexpected closing brace at top level"),
    ("parse unsupported", "parse", "match guards are not supported in stage1"),
    ("parse unexpected-token", "parse", "unexpected token near let"),
    ("parse missing-token", "parse", "missing argument after comma"),
    ("parse fallback", "parse", "invalid identifier"),
    ("manifest dependency-path", "manifest", "dependency path escapes the package root"),
    ("manifest resolves-outside", "manifest", "entry resolves outside the workspace"),
    ("manifest capability", "manifest", "unknown capability flag net2"),
    ("manifest fallback", "manifest", "malformed manifest table"),
    ("import circular", "import", "circular import chain detected"),
    ("import cycle", "import", "module cycle between a and b"),
    ("import not-found", "import", "module not found in package"),
    ("import failed-read", "import", "failed to read imported source"),
    ("import missing-import", "import", "missing import target file"),
    ("import fallback", "import", "import alias forms are rejected"),
    ("capability requires", "capability", "call to fs_read requires capabilities fs"),
    ("capability not-enabled", "capability", "clock capability not enabled for package"),
    ("capability fallback", "capability", "capability record was malformed"),
    ("type undefined", "type", "undefined function frobnicate"),
    ("type unknown", "type", "unknown type name Widget"),
    ("type expected", "type", "expected int, got bool"),
    ("type mismatch", "type", "arm type mismatch in match expression"),
    ("type fallback", "type", "trait bound failure"),
    ("ownership", "ownership", "use of moved value greeting"),
    ("codegen", "codegen", "internal lowering failure"),
    ("build", "build", "native backend failed to produce artifact"),
    ("runtime", "runtime", "index out of bounds"),
    ("control missing-return", "control", "function does not return along all paths"),
    ("control unreachable", "control", "unreachable statement after panic"),
    ("control fallback", "control", "loop guard shape rejected"),
    ("fmt", "fmt", "formatting failed for source"),
    ("source", "source", "path is not an .ax source file"),
    ("json", "json", "serialization failed for envelope"),
    ("unknown-kind", "lint", "unused variable total"),
];

const REPAIR_KINDS: &[&str] = &[
    "parse",
    "type",
    "ownership",
    "manifest",
    "capability",
    "import",
    "fmt",
    "source",
    "build",
    "codegen",
    "runtime",
];

const RENDER_MESSAGE: &str = "use of moved value greeting";

const EDIT_DISTANCE_CASES: &[(&str, &str, &str)] = &[
    ("kitten-sitting", "kitten", "sitting"),
    ("flaw-lawn", "flaw", "lawn"),
    ("agent-agent", "agent", "agent"),
    ("empty-flaw", "", "flaw"),
];

const PLAUSIBLE_CASES: &[(&str, usize, usize, usize)] = &[
    ("empty-needle", 0, 3, 1),
    ("empty-candidate", 3, 0, 1),
    ("short-close", 5, 5, 2),
    ("short-far", 5, 5, 3),
    ("long-close", 6, 7, 3),
    ("long-far", 9, 8, 4),
    ("mixed-lengths", 4, 6, 3),
];

fn option_code_text(value: Option<String>) -> String {
    match value {
        Some(code) => format!("some:{code}"),
        None => "none".to_string(),
    }
}

fn render_case(path: Option<&str>, line: Option<usize>, column: Option<usize>) -> String {
    let mut diagnostic = Diagnostic::new("ownership", RENDER_MESSAGE);
    if let Some(path) = path {
        diagnostic = diagnostic.with_path(path);
    }
    diagnostic.line = line;
    diagnostic.column = column;
    diagnostic.to_string()
}

fn expected_lines_from_rust() -> Vec<String> {
    let mut lines = Vec::new();
    for (label, kind, message) in STABLE_CODE_CASES {
        lines.push(format!(
            "stable_code {label} = {}",
            option_code_text(stable_diagnostic_code(kind, message))
        ));
    }
    for kind in REPAIR_KINDS {
        let hint = repair_hint(kind, "").expect("repair hint for known kind");
        lines.push(format!(
            "repair {kind} = action={} edit={} command={}",
            hint.action,
            hint.edit.as_deref().unwrap_or("none"),
            hint.command.as_deref().unwrap_or("none"),
        ));
    }
    assert!(repair_hint("lint", "").is_none(), "unknown kind must have no hint");
    lines.push("repair unknown-kind = none".to_string());
    lines.push(format!(
        "render full = {}",
        render_case(Some("src/main.ax"), Some(3), Some(7))
    ));
    lines.push(format!(
        "render path-line = {}",
        render_case(Some("src/main.ax"), Some(3), None)
    ));
    lines.push(format!(
        "render path-only = {}",
        render_case(Some("src/main.ax"), None, None)
    ));
    lines.push(format!("render message-only = {}", render_case(None, None, None)));
    for (label, needle_len, candidate_len, distance) in PLAUSIBLE_CASES {
        let needle = "a".repeat(*needle_len);
        let candidate = "b".repeat(*candidate_len);
        lines.push(format!(
            "plausible {label} = {}",
            is_plausible_suggestion(&needle, &candidate, *distance)
        ));
    }
    lines
}

fn expected_output_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../selfhost/compiler-diagnostics-spike/expected-output.txt")
}

fn distance_expected_output_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../selfhost/compiler-diagnostics-distance-spike/expected-output.txt")
}

#[test]
fn rust_edit_distance_matches_distance_spike_expected_output() {
    let lines: Vec<String> = EDIT_DISTANCE_CASES
        .iter()
        .map(|(_, left, right)| edit_distance(left, right).to_string())
        .collect();
    let rendered = format!("{}\n", lines.join("\n"));
    let path = distance_expected_output_path();
    if std::env::var_os("AXIOM_UPDATE_SPIKE_EXPECTED").is_some() {
        fs::write(&path, &rendered).expect("write distance spike expected output");
        return;
    }
    let expected = fs::read_to_string(&path).expect("read distance spike expected output");
    assert_eq!(
        expected, rendered,
        "Rust edit_distance diverged from the checked-in distance spike \
         expected output; regenerate with AXIOM_UPDATE_SPIKE_EXPECTED=1 and \
         re-run scripts/ci/run-self-hosting-spike-parity.sh"
    );
}

#[test]
fn rust_diagnostics_match_spike_expected_output() {
    let lines = expected_lines_from_rust();
    let rendered = format!("{}\n", lines.join("\n"));
    let path = expected_output_path();
    if std::env::var_os("AXIOM_UPDATE_SPIKE_EXPECTED").is_some() {
        fs::write(&path, &rendered).expect("write spike expected output");
        return;
    }
    let expected = fs::read_to_string(&path).expect("read spike expected output");
    assert_eq!(
        expected, rendered,
        "Rust diagnostics implementation diverged from the checked-in spike \
         expected output; regenerate with AXIOM_UPDATE_SPIKE_EXPECTED=1 and \
         re-run scripts/ci/run-self-hosting-spike-parity.sh"
    );
}
