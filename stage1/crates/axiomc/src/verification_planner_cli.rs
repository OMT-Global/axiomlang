use axiomc::diagnostics::Diagnostic;
use axiomc::intent_ir::IntentIrDocument;
use axiomc::verification_planner::{SemanticDiffContract, VerificationResults, VerdictStatus, evaluate_verification, plan_verification_with_diff};
use serde::de::DeserializeOwned;
use std::fs;
use std::path::Path;
use std::process::Command;

pub(super) fn run(
    before: &Path,
    after: &Path,
    diff: &Path,
    project: &Path,
    source_head: &str,
    delivered_head: &str,
    results: Option<&Path>,
    json: bool,
) -> i32 {
    match execute(before, after, diff, project, source_head, delivered_head, results) {
        Ok(Output::Plan(plan)) => {
            if json {
                super::print_json("verification-plan", &plan)
            } else {
                println!(
                    "requirements={} changes={} coverage={:?}",
                    plan.requirements.len(), plan.changes.len(), plan.coverage.confidence
                );
                0
            }
        }
        Ok(Output::Verdict(verdict)) => {
            let status = if verdict.status == VerdictStatus::Passed { 0 } else { 1 };
            if json {
                super::print_json_with_status("verification-plan", &verdict, status)
            } else {
                println!("status={:?} missing={} invalid={} failed={}", verdict.status, verdict.missing.len(), verdict.invalid.len(), verdict.failed.len());
                status
            }
        }
        Err(error) => super::print_error("verification-plan", error, json),
    }
}

enum Output {
    Plan(axiomc::verification_planner::VerificationPlan),
    Verdict(axiomc::verification_planner::VerificationVerdict),
}

fn execute(
    before: &Path,
    after: &Path,
    diff: &Path,
    project: &Path,
    source_head: &str,
    delivered_head: &str,
    results: Option<&Path>,
) -> Result<Output, Diagnostic> {
    let before_binding = before.display().to_string();
    let after_binding = after.display().to_string();
    let before = load::<IntentIrDocument>(before, "before Intent IR")?;
    let after = load::<IntentIrDocument>(after, "after Intent IR")?;
    let diff = load::<SemanticDiffContract>(diff, "semantic diff")?;
    if diff.old != before_binding || diff.new != after_binding {
        return Err(Diagnostic::new(
            "verification_plan",
            "semantic diff input paths do not match the supplied Intent IR snapshots",
        ));
    }
    let observed_head = current_head(project)?;
    if delivered_head != observed_head {
        return Err(Diagnostic::new(
            "verification_plan",
            format!("delivered head {delivered_head} is stale; current project HEAD is {observed_head}"),
        ));
    }
    let plan = plan_verification_with_diff(&before, &after, &diff, source_head, &observed_head)
        .map_err(|error| Diagnostic::new("verification_plan", error))?;
    let Some(results_path) = results else { return Ok(Output::Plan(plan)); };
    let results = load::<VerificationResults>(results_path, "verification results")?;
    Ok(Output::Verdict(evaluate_verification(&plan, &results, &observed_head)))
}

fn current_head(project: &Path) -> Result<String, Diagnostic> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(project)
        .output()
        .map_err(|error| Diagnostic::new("verification_plan", format!("failed to inspect project HEAD: {error}")))?;
    if !output.status.success() {
        return Err(Diagnostic::new("verification_plan", "project HEAD is unavailable"));
    }
    String::from_utf8(output.stdout)
        .map(|head| head.trim().to_owned())
        .map_err(|error| Diagnostic::new("verification_plan", format!("project HEAD is not UTF-8: {error}")))
}

fn load<T: DeserializeOwned>(path: &Path, label: &str) -> Result<T, Diagnostic> {
    let bytes = fs::read(path).map_err(|error| {
        Diagnostic::new("verification_plan", format!("failed to read {label} {}: {error}", path.display()))
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        Diagnostic::new("verification_plan", format!("invalid {label} {}: {error}", path.display()))
    })
}
