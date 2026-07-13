use axiomc::agent_task::{AgentTaskContract, compile_task_contract};
use axiomc::bounded_executor::{
    BoundedExecutor, BudgetUsage, DeliveryEvidenceKey, ExecutionMode, ExecutionState,
    ExecutorRequest, ExecutorResumeRequest, ExecutorSealKey, FailureCause, RetryPolicy,
    SignedDeliveryEvidence, TrustedDeliveryEvidenceProvider,
};
use axiomc::transactional_workspace::{TransactionalWorkspace, WorkspacePolicy};
use axiomc::verification_planner::{VerdictStatus, VerificationVerdict};
use jsonschema::Validator;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const KEY_BYTES: &[u8; 32] = b"0123456789abcdef0123456789abcdef";

fn seal_key() -> ExecutorSealKey {
    ExecutorSealKey::from_secret("executor-test", KEY_BYTES).unwrap()
}

fn delivery_key() -> DeliveryEvidenceKey {
    DeliveryEvidenceKey::from_secret("github-test", KEY_BYTES).unwrap()
}

fn now_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
}

fn fixture(name: &str) -> Value {
    serde_json::from_str(
        &fs::read_to_string(
            repo_root()
                .join("stage1/json-fixtures/bounded-executor")
                .join(name),
        )
        .expect("read bounded-executor fixture"),
    )
    .expect("bounded-executor fixture is JSON")
}

fn validator(name: &str) -> Validator {
    let schema: Value = serde_json::from_str(
        &fs::read_to_string(repo_root().join("stage1/schemas").join(name))
            .expect("read bounded-executor schema"),
    )
    .expect("bounded-executor schema is JSON");
    jsonschema::validator_for(&schema).expect("compile bounded-executor schema")
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
        .output()
        .expect("run git fixture command");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("git output is UTF-8")
}

struct Harness {
    _root: TempDir,
    source: PathBuf,
    transaction: PathBuf,
    base_sha: String,
    contract: AgentTaskContract,
}

fn harness() -> Harness {
    let root = tempfile::tempdir().expect("create executor fixture root");
    let source = root.path().join("source");
    fs::create_dir_all(source.join("src")).expect("create fixture project");
    fs::copy(
        repo_root().join("stage1/examples/agent_native_authorize/axiom.toml"),
        source.join("axiom.toml"),
    )
    .expect("copy project manifest");
    fs::copy(
        repo_root().join("stage1/examples/agent_native_authorize/src/main.ax"),
        source.join("src/main.ax"),
    )
    .expect("copy project source");
    git(&source, &["init", "-q"]);
    git(&source, &["config", "user.email", "test@example.invalid"]);
    git(&source, &["config", "user.name", "Executor Contract Test"]);
    git(&source, &["add", "axiom.toml", "src/main.ax"]);
    git(
        &source,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "base"],
    );
    let base_sha = git(&source, &["rev-parse", "HEAD"]).trim().to_owned();

    let mut spec: Value = serde_json::from_str(
        &fs::read_to_string(
            repo_root().join("stage1/json-fixtures/task-contract/repair-approved.spec.json"),
        )
        .expect("read repair task fixture"),
    )
    .expect("repair task fixture is JSON");
    spec["task"]["authority"]["source_revision"] = base_sha.clone().into();
    let spec_path = root.path().join("task.json");
    fs::write(&spec_path, serde_json::to_vec_pretty(&spec).unwrap()).expect("write task fixture");
    let contract = compile_task_contract(&source, &spec_path)
        .expect("compile approved task")
        .contract;
    let transaction = root.path().join("transaction");
    Harness {
        _root: root,
        source,
        transaction,
        base_sha,
        contract,
    }
}

fn workspace(harness: &Harness) -> TransactionalWorkspace {
    let policy = WorkspacePolicy {
        allowed_read_paths: BTreeSet::from(["src/main.ax".into()]),
        allowed_write_paths: BTreeSet::from(["src/main.ax".into()]),
        allowed_commands: BTreeSet::new(),
        allow_network: false,
        verified_sandbox: true,
    };
    TransactionalWorkspace::create_for_task(
        &harness.source,
        &harness.transaction,
        &harness.base_sha,
        policy,
        &harness.contract.contract_digest,
        DIGEST,
        "codex/executor-fixture",
    )
    .expect("create task-bound transaction")
}

fn verdict(
    base_sha: &str,
    delivered_head_sha: &str,
    status: VerdictStatus,
    failed: Vec<String>,
) -> VerificationVerdict {
    VerificationVerdict {
        schema_version: "axiom.verification_verdict.v0".into(),
        plan_digest: DIGEST.into(),
        status,
        source_head_sha: base_sha.into(),
        delivered_head_sha: delivered_head_sha.into(),
        missing: Vec::new(),
        duplicate: Vec::new(),
        invalid: Vec::new(),
        failed,
    }
}

fn apply_fixture(harness: &Harness) -> (BoundedExecutor, TransactionalWorkspace, Vec<u8>) {
    let mut workspace = workspace(harness);
    let original = workspace.read("src/main.ax").expect("read candidate base");
    let mut executor =
        BoundedExecutor::new(&harness.contract, &harness.base_sha, false, &seal_key()).unwrap();
    executor
        .propose_replace("src/main.ax", &original, "print authorized", "print true")
        .unwrap();
    executor.apply_proposal(&mut workspace).unwrap();
    (executor, workspace, original)
}

fn assert_report_valid(executor: &BoundedExecutor) {
    let value = serde_json::to_value(executor).expect("serialize executor report");
    let encoded = serde_json::to_string(&value).unwrap();
    assert!(!encoded.contains("0123456789abcdef0123456789abcdef"));
    assert!(!encoded.contains("\"seal_key\":"));
    let errors: Vec<_> = validator("axiom-executor-report-v0.schema.json")
        .iter_errors(&value)
        .map(|error| error.to_string())
        .collect();
    assert!(
        errors.is_empty(),
        "executor report schema errors: {errors:#?}\n{value:#}"
    );
}

fn commit_candidate(harness: &Harness) -> String {
    git(&harness.transaction, &["add", "src/main.ax"]);
    git(
        &harness.transaction,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "candidate"],
    );
    git(&harness.transaction, &["rev-parse", "HEAD"])
        .trim()
        .to_owned()
}

fn verified_harness() -> (Harness, BoundedExecutor, TransactionalWorkspace, String) {
    let harness = harness();
    let (mut executor, mut workspace, _) = apply_fixture(&harness);
    let delivered_head = commit_candidate(&harness);
    executor
        .submit_verification(
            &mut workspace,
            verdict(
                &harness.base_sha,
                &delivered_head,
                VerdictStatus::Passed,
                vec![],
            ),
        )
        .unwrap();
    (harness, executor, workspace, delivered_head)
}

fn signed_delivery(
    executor: &BoundedExecutor,
    head: &str,
    key: &DeliveryEvidenceKey,
    fetched_at: u64,
    components: (bool, bool, bool),
) -> SignedDeliveryEvidence {
    let binding = executor.candidate_binding().unwrap();
    key.sign(SignedDeliveryEvidence {
        provider_id: "github-test".into(),
        candidate_digest: executor.candidate_digest().unwrap().into(),
        plan_digest: DIGEST.into(),
        delivered_head_sha: head.into(),
        transaction_id: binding.transaction_id.clone(),
        policy_digest: binding.policy_digest.clone(),
        fetched_at_epoch_seconds: fetched_at,
        required_checks_passed: components.0,
        review_satisfied: components.1,
        conversations_resolved: components.2,
        evidence_mac: String::new(),
    })
    .unwrap()
}

#[test]
fn dry_run_and_scope_escape_fixtures_are_deterministic_and_fail_closed() {
    let success = fixture("deterministic-success.json");
    let harness = harness();
    let current = success["initial"].as_str().unwrap().as_bytes();
    let mut first =
        BoundedExecutor::new(&harness.contract, &harness.base_sha, true, &seal_key()).unwrap();
    first
        .propose_replace(
            success["path"].as_str().unwrap(),
            current,
            success["find"].as_str().unwrap(),
            success["replacement"].as_str().unwrap(),
        )
        .unwrap();
    let mut second =
        BoundedExecutor::new(&harness.contract, &harness.base_sha, true, &seal_key()).unwrap();
    second
        .propose_replace(
            success["path"].as_str().unwrap(),
            current,
            success["find"].as_str().unwrap(),
            success["replacement"].as_str().unwrap(),
        )
        .unwrap();
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    assert_eq!(first.state(), ExecutionState::DryRun);
    assert_report_valid(&first);

    let escape = fixture("scope-escape.json");
    let mut executor =
        BoundedExecutor::new(&harness.contract, &harness.base_sha, false, &seal_key()).unwrap();
    assert!(
        executor
            .propose_replace(
                escape["path"].as_str().unwrap(),
                escape["initial"].as_str().unwrap().as_bytes(),
                escape["find"].as_str().unwrap(),
                escape["replacement"].as_str().unwrap(),
            )
            .is_err()
    );
    assert_eq!(executor.state(), ExecutionState::Rejected);
    assert_report_valid(&executor);
}

#[test]
fn identical_retry_stops_and_interruption_recovery_preserves_consumption() {
    let retry = fixture("bounded-retry.json");
    let harness = harness();
    let mut executor = BoundedExecutor::new_with_retry_policy(
        &harness.contract,
        &harness.base_sha,
        false,
        RetryPolicy {
            approved_causes: vec![FailureCause::Code],
        },
        &seal_key(),
    )
    .unwrap();
    executor
        .retry_failure(
            FailureCause::Code,
            retry["failure_code"].as_str().unwrap(),
            retry["failure_message"].as_str().unwrap(),
        )
        .unwrap();
    assert!(
        executor
            .retry_failure(
                FailureCause::Code,
                retry["failure_code"].as_str().unwrap(),
                retry["failure_message"].as_str().unwrap()
            )
            .is_err()
    );
    assert_eq!(executor.state(), ExecutionState::Escalated);
    assert_report_valid(&executor);

    let resume = fixture("interruption-resume.json");
    let mut workspace = workspace(&harness);
    let mut executor =
        BoundedExecutor::new(&harness.contract, &harness.base_sha, false, &seal_key()).unwrap();
    let current = workspace.read("src/main.ax").unwrap();
    executor
        .propose_replace("src/main.ax", &current, "print authorized", "print true")
        .unwrap();
    executor.apply_proposal(&mut workspace).unwrap();
    let usage: BudgetUsage =
        serde_json::from_value(resume["budget_charge_before_interrupt"].clone()).unwrap();
    executor.charge_budget(usage.clone()).unwrap();
    executor.interrupt().unwrap();
    let request = executor.resume_request().unwrap();
    let parsed = ExecutorResumeRequest::parse(&serde_json::to_vec(&request).unwrap()).unwrap();
    let encoded = serde_json::to_vec(&executor).unwrap();
    let mut recovered = BoundedExecutor::recover(
        &encoded,
        &parsed,
        &harness.contract,
        &workspace,
        &seal_key(),
    )
    .unwrap();
    recovered.resume().unwrap();
    assert_eq!(recovered.budgets().consumed, usage);
    assert_eq!(recovered.state(), ExecutionState::Edited);
    assert_report_valid(&recovered);
    workspace.abort().unwrap();
}

#[test]
fn success_requires_fresh_candidate_bound_evidence_and_is_schema_valid() {
    let _case = fixture("deterministic-success.json");
    let harness = harness();
    let (mut executor, mut workspace, _) = apply_fixture(&harness);
    let delivered_head = commit_candidate(&harness);
    executor
        .submit_verification(
            &mut workspace,
            verdict(
                &harness.base_sha,
                &delivered_head,
                VerdictStatus::Passed,
                vec![],
            ),
        )
        .unwrap();
    let candidate_binding = executor.candidate_binding().unwrap().clone();
    let candidate_digest = executor.candidate_digest().unwrap().to_owned();
    let evidence_key = delivery_key();
    let provider = TrustedDeliveryEvidenceProvider::new(evidence_key.clone(), 60).unwrap();
    let evidence = evidence_key
        .sign(SignedDeliveryEvidence {
            provider_id: "github-test".into(),
            candidate_digest,
            plan_digest: DIGEST.into(),
            delivered_head_sha: delivered_head,
            transaction_id: candidate_binding.transaction_id,
            policy_digest: candidate_binding.policy_digest,
            fetched_at_epoch_seconds: now_epoch_seconds(),
            required_checks_passed: true,
            review_satisfied: true,
            conversations_resolved: true,
            evidence_mac: String::new(),
        })
        .unwrap();
    executor
        .submit_delivery(&workspace, evidence, &provider)
        .unwrap();
    assert_eq!(executor.state(), ExecutionState::Resolved);
    assert_report_valid(&executor);
}

#[test]
fn evidence_regression_and_explicit_abort_roll_back_the_transaction() {
    let _regression = fixture("evidence-regression.json");
    let _rollback = fixture("rollback.json");
    let harness = harness();
    let (mut executor, mut workspace, original) = apply_fixture(&harness);
    let delivered_head = commit_candidate(&harness);
    executor
        .submit_verification(
            &mut workspace,
            verdict(
                &harness.base_sha,
                &delivered_head,
                VerdictStatus::Failed,
                vec!["evidence-regression".into()],
            ),
        )
        .unwrap();
    assert_eq!(executor.state(), ExecutionState::EvidenceFailed);
    assert_ne!(workspace.read("src/main.ax").unwrap(), original);
    executor.rollback(&mut workspace).unwrap();
    assert_eq!(executor.state(), ExecutionState::RolledBack);
    assert_eq!(
        fs::read(harness.transaction.join("src/main.ax")).unwrap(),
        original
    );
    assert_report_valid(&executor);
}

#[test]
fn verified_candidate_head_survives_restart_and_can_be_rolled_back() {
    let harness = harness();
    let (mut executor, mut workspace, original) = apply_fixture(&harness);
    let delivered_head = commit_candidate(&harness);
    executor
        .submit_verification(
            &mut workspace,
            verdict(
                &harness.base_sha,
                &delivered_head,
                VerdictStatus::Passed,
                vec![],
            ),
        )
        .unwrap();
    assert_eq!(
        workspace.state().authorized_candidate_head.as_deref(),
        Some(delivered_head.as_str())
    );

    executor.interrupt().unwrap();
    let resume = executor.resume_request().unwrap();
    let encoded = serde_json::to_vec(&executor).unwrap();
    drop(workspace);

    let mut recovered_workspace = TransactionalWorkspace::recover(&harness.transaction)
        .expect("recover the verified candidate transaction head");
    let mut recovered_executor = BoundedExecutor::recover(
        &encoded,
        &resume,
        &harness.contract,
        &recovered_workspace,
        &seal_key(),
    )
    .expect("recover the sealed executor after verification");
    recovered_executor.resume().unwrap();
    recovered_executor.rollback(&mut recovered_workspace).unwrap();
    assert_eq!(recovered_executor.state(), ExecutionState::RolledBack);
    assert_eq!(
        fs::read(harness.transaction.join("src/main.ax")).unwrap(),
        original
    );
}

#[test]
fn delivery_requires_authentic_fresh_satisfactory_provider_evidence() {
    let key = delivery_key();
    let provider = TrustedDeliveryEvidenceProvider::new(key.clone(), 60).unwrap();
    let now = now_epoch_seconds();

    let (_harness, mut executor, workspace, head) = verified_harness();
    let mut fabricated = signed_delivery(&executor, &head, &key, now, (true, true, true));
    fabricated.required_checks_passed = false;
    assert!(
        executor
            .submit_delivery(&workspace, fabricated, &provider)
            .is_err()
    );
    assert_ne!(executor.state(), ExecutionState::Resolved);

    let (_harness, mut executor, workspace, head) = verified_harness();
    let stale = signed_delivery(
        &executor,
        &head,
        &key,
        now.saturating_sub(61),
        (true, true, true),
    );
    assert!(
        executor
            .submit_delivery(&workspace, stale, &provider)
            .is_err()
    );
    assert_ne!(executor.state(), ExecutionState::Resolved);

    let (_harness, mut executor, workspace, head) = verified_harness();
    let future = signed_delivery(&executor, &head, &key, now + 60, (true, true, true));
    assert!(
        executor
            .submit_delivery(&workspace, future, &provider)
            .is_err()
    );
    assert_ne!(executor.state(), ExecutionState::Resolved);

    let (_harness, mut executor, workspace, head) = verified_harness();
    let unsatisfactory = signed_delivery(&executor, &head, &key, now, (true, false, true));
    assert!(
        executor
            .submit_delivery(&workspace, unsatisfactory, &provider)
            .is_err()
    );
    assert_ne!(executor.state(), ExecutionState::Resolved);
}

#[test]
fn typed_request_is_consumed_and_authority_widening_is_rejected() {
    let harness = harness();
    let mut workspace = workspace(&harness);
    let current = workspace.read("src/main.ax").unwrap();
    let mut planner =
        BoundedExecutor::new(&harness.contract, &harness.base_sha, true, &seal_key()).unwrap();
    let proposal = planner
        .propose_replace("src/main.ax", &current, "print authorized", "print true")
        .unwrap();
    let request = ExecutorRequest {
        schema_version: "axiom.executor_request.v0".into(),
        task_contract_digest: harness.contract.contract_digest.clone(),
        transaction_id: workspace.state().transaction_id.clone(),
        transaction_digest: workspace.state().policy_digest.clone(),
        base_sha: harness.base_sha.clone(),
        mode: ExecutionMode::Deterministic,
        dry_run: true,
        budgets: harness.contract.budgets.clone(),
        retry_policy: RetryPolicy {
            approved_causes: vec![FailureCause::Evidence],
        },
        proposal: Some(proposal),
    };
    let encoded = serde_json::to_vec(&request).unwrap();
    let parsed = ExecutorRequest::parse(&encoded).unwrap();
    validator("axiom-executor-request-v0.schema.json")
        .validate(&serde_json::from_slice::<Value>(&encoded).unwrap())
        .expect("runtime request matches schema");
    let executor = BoundedExecutor::from_request(
        parsed,
        &harness.contract,
        &workspace,
        Some(&current),
        &seal_key(),
    )
    .unwrap();
    assert_eq!(executor.state(), ExecutionState::DryRun);
    assert_report_valid(&executor);

    let mut widened = request.clone();
    widened.budgets.tokens += 1;
    assert!(
        BoundedExecutor::from_request(
            widened,
            &harness.contract,
            &workspace,
            Some(&current),
            &seal_key(),
        )
        .is_err()
    );
    let mut forbidden_retry = request;
    forbidden_retry.retry_policy.approved_causes = vec![FailureCause::Policy];
    assert!(
        BoundedExecutor::from_request(
            forbidden_retry,
            &harness.contract,
            &workspace,
            Some(&current),
            &seal_key(),
        )
        .is_err()
    );
}

#[test]
fn sealed_report_resume_and_candidate_binding_reject_tampering() {
    let harness = harness();
    let (mut executor, mut workspace, _) = apply_fixture(&harness);
    executor.interrupt().unwrap();
    let resume = executor.resume_request().unwrap();
    let encoded = serde_json::to_vec(&executor).unwrap();

    let mut tampered_report: Value = serde_json::from_slice(&encoded).unwrap();
    tampered_report["retry_policy"]["approved_causes"] = serde_json::json!(["code"]);
    assert!(
        BoundedExecutor::recover(
            &serde_json::to_vec(&tampered_report).unwrap(),
            &resume,
            &harness.contract,
            &workspace,
            &seal_key(),
        )
        .is_err()
    );

    let mut unkeyed_reseal: Value = serde_json::from_slice(&encoded).unwrap();
    let plain = unkeyed_reseal["state_digest"]
        .as_str()
        .unwrap()
        .strip_prefix("sha256:")
        .unwrap();
    unkeyed_reseal["seal_mac"] = format!("hmac-sha256:{plain}").into();
    assert!(
        BoundedExecutor::recover(
            &serde_json::to_vec(&unkeyed_reseal).unwrap(),
            &resume,
            &harness.contract,
            &workspace,
            &seal_key(),
        )
        .is_err()
    );

    let wrong_key =
        ExecutorSealKey::from_secret("executor-test", b"abcdef0123456789abcdef0123456789").unwrap();
    assert!(
        BoundedExecutor::recover(&encoded, &resume, &harness.contract, &workspace, &wrong_key,)
            .is_err()
    );

    let mut widened_resume = resume.clone();
    widened_resume.remaining_budgets.tokens += 1;
    assert!(
        BoundedExecutor::recover(
            &encoded,
            &widened_resume,
            &harness.contract,
            &workspace,
            &seal_key(),
        )
        .is_err()
    );

    let mut recovered = BoundedExecutor::recover(
        &encoded,
        &resume,
        &harness.contract,
        &workspace,
        &seal_key(),
    )
    .unwrap();
    recovered.resume().unwrap();
    fs::write(
        harness.transaction.join("axiom.toml"),
        "[package]\nname = \"agent-native-authorize\"\nversion = \"0.1.0\"\n# unrelated\n",
    )
    .unwrap();
    git(&harness.transaction, &["add", "axiom.toml"]);
    git(
        &harness.transaction,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "unrelated"],
    );
    let unrelated = git(&harness.transaction, &["rev-parse", "HEAD"])
        .trim()
        .to_owned();
    assert!(
        recovered
            .submit_verification(
                &mut workspace,
                verdict(&harness.base_sha, &unrelated, VerdictStatus::Passed, vec![]),
            )
            .is_err()
    );
}
