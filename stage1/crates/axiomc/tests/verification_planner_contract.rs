use axiomc::intent_ir::IntentIrDocument;
use axiomc::verification_planner::{
    CoverageConfidence, EvidenceKind, EvidenceResult, EvidenceStatus, RESULTS_SCHEMA_VERSION,
    VerificationResults, VerdictStatus, evaluate_verification, plan_verification,
};
use jsonschema::Validator;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SOURCE_SHA: &str = "1111111111111111111111111111111111111111";
const DELIVERED_SHA: &str = "2222222222222222222222222222222222222222";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCase {
    scenario: String,
    before: IntentIrDocument,
    after: IntentIrDocument,
    expected_evidence: Vec<EvidenceKind>,
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
}

fn fixture_root() -> PathBuf {
    repository_root()
        .join("stage1")
        .join("json-fixtures")
        .join("verification-planner")
}

fn schema(name: &str) -> (Value, Validator) {
    let value: Value = serde_json::from_str(
        &fs::read_to_string(
            repository_root()
                .join("stage1")
                .join("schemas")
                .join(name),
        )
        .expect("read verification schema"),
    )
    .expect("verification schema is JSON");
    let validator = jsonschema::validator_for(&value).expect("compile verification schema");
    (value, validator)
}

fn read_case(name: &str) -> FixtureCase {
    serde_json::from_str(
        &fs::read_to_string(fixture_root().join(name)).expect("read planner fixture"),
    )
    .expect("planner fixture follows the typed corpus contract")
}

fn passing_results(plan: &axiomc::verification_planner::VerificationPlan) -> VerificationResults {
    VerificationResults {
        schema_version: RESULTS_SCHEMA_VERSION.into(),
        plan_digest: plan.plan_digest.clone(),
        source_head_sha: plan.bindings.source_head_sha.clone(),
        delivered_head_sha: plan.bindings.delivered_head_sha.clone(),
        results: plan
            .requirements
            .iter()
            .map(|requirement| EvidenceResult {
                id: requirement.id.clone(),
                plan_digest: plan.plan_digest.clone(),
                source_head_sha: plan.bindings.source_head_sha.clone(),
                delivered_head_sha: plan.bindings.delivered_head_sha.clone(),
                status: EvidenceStatus::Passed,
                evidence_digest: format!("sha256:{:064x}", requirement.id.len()),
            })
            .collect(),
    }
}

#[test]
fn semantic_fixture_corpus_maps_every_required_impact_deterministically() {
    let (_, plan_validator) = schema("axiom-verification-plan-v0.schema.json");
    for name in [
        "localized.json",
        "public-api.json",
        "capability-escalation.json",
        "schema-change.json",
        "artifact-drift.json",
        "performance-sensitive.json",
        "unknown-impact.json",
    ] {
        let case = read_case(name);
        let first = plan_verification(&case.before, &case.after, SOURCE_SHA, DELIVERED_SHA)
            .unwrap_or_else(|error| panic!("{} plans: {error}", case.scenario));
        let second = plan_verification(&case.before, &case.after, SOURCE_SHA, DELIVERED_SHA)
            .expect("same fixture plans twice");
        assert_eq!(first, second, "{} plan is deterministic", case.scenario);
        assert_eq!(
            serde_json::to_vec(&first).expect("serialize first plan"),
            serde_json::to_vec(&second).expect("serialize second plan"),
            "{} plan is byte stable",
            case.scenario
        );
        plan_validator
            .validate(&serde_json::to_value(&first).expect("plan value"))
            .unwrap_or_else(|error| panic!("{} plan matches schema: {error}", case.scenario));
        let actual: BTreeSet<_> = first.requirements.iter().map(|item| item.kind).collect();
        let expected: BTreeSet<_> = case.expected_evidence.into_iter().collect();
        assert_eq!(actual, expected, "{} evidence mapping", case.scenario);
        assert!(
            !first.requirements.is_empty(),
            "{} semantic change cannot yield an empty plan",
            case.scenario
        );
    }
}

#[test]
fn planner_quality_baseline_is_complete_for_the_1417_dimensions() {
    let scenarios = [
        "localized.json", "public-api.json", "capability-escalation.json", "schema-change.json",
        "artifact-drift.json", "performance-sensitive.json", "unknown-impact.json",
    ];
    let measured = scenarios
        .iter()
        .filter(|name| {
            let case = read_case(name);
            plan_verification(&case.before, &case.after, SOURCE_SHA, DELIVERED_SHA)
                .is_ok_and(|plan| !plan.requirements.is_empty())
        })
        .count();
    let baseline: Value = serde_json::from_str(
        &fs::read_to_string(fixture_root().join("quality-baseline.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(baseline["schema_version"], "axiom.verification_planner_quality.v0");
    assert_eq!(baseline["governing_suite"], 1417);
    assert_eq!(baseline["scenario_count"], scenarios.len());
    assert_eq!(baseline["mapped_scenarios"], measured);
    assert_eq!(baseline["empty_changed_plans"], 0);
    assert_eq!(baseline["unknown_scenarios_broadened"], 1);
}

#[test]
fn unknown_impact_broadens_to_every_suite_and_is_explained() {
    let case = read_case("unknown-impact.json");
    let plan = plan_verification(&case.before, &case.after, SOURCE_SHA, DELIVERED_SHA)
        .expect("unknown impact plans conservatively");
    assert_eq!(plan.requirements.len(), 7);
    assert_eq!(plan.coverage.confidence, CoverageConfidence::Conservative);
    assert!(!plan.coverage.complete);
    assert!(!plan.coverage.unknown_impacts.is_empty());
    assert!(plan.coverage.explanation.contains("broadened"));
}

#[test]
fn terminal_success_requires_schema_valid_fresh_exactly_once_evidence() {
    let case = read_case("capability-escalation.json");
    let plan = plan_verification(&case.before, &case.after, SOURCE_SHA, DELIVERED_SHA)
        .expect("capability plan");
    let (_, results_validator) = schema("axiom-verification-results-v0.schema.json");
    let (_, verdict_validator) = schema("axiom-verification-verdict-v0.schema.json");
    let results = passing_results(&plan);
    results_validator
        .validate(&serde_json::to_value(&results).expect("results value"))
        .expect("fresh result envelope matches schema");
    let verdict = evaluate_verification(&plan, &results, DELIVERED_SHA);
    assert_eq!(verdict.status, VerdictStatus::Passed);
    verdict_validator
        .validate(&serde_json::to_value(&verdict).expect("verdict value"))
        .expect("passing verdict matches schema");

    let mut missing = results.clone();
    let missing_id = missing.results.pop().expect("planned result").id;
    let verdict = evaluate_verification(&plan, &missing, DELIVERED_SHA);
    assert_eq!(verdict.status, VerdictStatus::Failed);
    assert_eq!(verdict.missing, vec![missing_id]);
    verdict_validator
        .validate(&serde_json::to_value(&verdict).expect("failed verdict value"))
        .expect("failed verdict identifies at least one blocker");

    let mut duplicate = results.clone();
    duplicate.results.push(duplicate.results[0].clone());
    assert!(
        !results_validator.is_valid(&serde_json::to_value(&duplicate).expect("duplicate value")),
        "duplicate result envelope is not schema-valid"
    );
    let verdict = evaluate_verification(&plan, &duplicate, DELIVERED_SHA);
    assert_eq!(verdict.status, VerdictStatus::Failed);
    assert!(!verdict.duplicate.is_empty());

    let mut stale = results.clone();
    stale.results[0].delivered_head_sha = "3333333333333333333333333333333333333333".into();
    let verdict = evaluate_verification(&plan, &stale, DELIVERED_SHA);
    assert_eq!(verdict.status, VerdictStatus::Failed);
    assert!(verdict.invalid.iter().any(|item| item.contains("delivered_head_sha")));

    let mut failed = results;
    failed.results[0].status = EvidenceStatus::Failed;
    let verdict = evaluate_verification(&plan, &failed, DELIVERED_SHA);
    assert_eq!(verdict.status, VerdictStatus::Failed);
    assert_eq!(verdict.failed, vec![failed.results[0].id.clone()]);
}

#[test]
fn malformed_or_stale_inputs_fail_closed() {
    let case = read_case("localized.json");
    assert!(
        plan_verification(&case.before, &case.after, "short", DELIVERED_SHA).is_err(),
        "abbreviated source commits are not exact-head bindings"
    );
    let mut unsupported = case.after;
    unsupported.schema_version = "axiom.intent_ir.v999".into();
    assert!(
        plan_verification(&case.before, &unsupported, SOURCE_SHA, DELIVERED_SHA).is_err(),
        "unsupported semantic input does not produce a plan"
    );
}

#[test]
fn cli_emits_the_versioned_plan_and_evaluates_exact_head_results() {
    let case = read_case("capability-escalation.json");
    let temp = tempfile::tempdir().expect("temporary CLI fixture");
    let before = temp.path().join("before.json");
    let after = temp.path().join("after.json");
    fs::write(&before, serde_json::to_vec(&case.before).unwrap()).unwrap();
    fs::write(&after, serde_json::to_vec(&case.after).unwrap()).unwrap();
    let diff = temp.path().join("diff.json");
    let diff_output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(["semantic-diff", before.to_str().unwrap(), after.to_str().unwrap(), "--json"])
        .output()
        .expect("run semantic diff CLI");
    assert!(diff_output.status.success());
    fs::write(&diff, diff_output.stdout).unwrap();
    let delivered_head = git_head(&repository_root());
    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "verification-plan", before.to_str().unwrap(), after.to_str().unwrap(),
            "--diff", diff.to_str().unwrap(), "--project", repository_root().to_str().unwrap(),
            "--source-head", SOURCE_SHA, "--delivered-head", &delivered_head, "--json",
        ])
        .output()
        .expect("run verification-plan CLI");
    assert!(output.status.success(), "stdout={} stderr={}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let plan: axiomc::verification_planner::VerificationPlan =
        serde_json::from_slice(&output.stdout).expect("CLI plan JSON");
    assert_eq!(plan.schema_version, "axiom.verification_plan.v0");

    let results_path = temp.path().join("results.json");
    fs::write(&results_path, serde_json::to_vec(&passing_results(&plan)).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "verification-plan", before.to_str().unwrap(), after.to_str().unwrap(),
            "--diff", diff.to_str().unwrap(), "--project", repository_root().to_str().unwrap(),
            "--source-head", SOURCE_SHA, "--delivered-head", &delivered_head,
            "--results", results_path.to_str().unwrap(), "--json",
        ])
        .output()
        .expect("run verification evaluator CLI");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let verdict: Value = serde_json::from_slice(&output.stdout).expect("CLI verdict JSON");
    assert_eq!(verdict["schema_version"], "axiom.verification_verdict.v0");
    assert_eq!(verdict["status"], "passed");
}

fn git_head(project: &Path) -> String {
    let output = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(project).output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().into()
}
