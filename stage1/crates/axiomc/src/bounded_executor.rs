//! Deterministic, fail-closed execution authority for one approved task.
//!
//! This module deliberately has no command, network, model, or delivery
//! implementation. Proposals and verification results are untrusted data;
//! only a validated transaction may receive the single built-in file edit.

use crate::agent_task::{AgentTaskContract, Budgets, TASK_SCHEMA_VERSION};
use crate::transactional_workspace::{TransactionPhase, TransactionalWorkspace};
use crate::verification_planner::{VERDICT_SCHEMA_VERSION, VerdictStatus, VerificationVerdict};
use serde::{Deserialize, Serialize};

pub const EXECUTOR_SCHEMA_VERSION: &str = "axiom.bounded_executor.v0";
pub const REQUEST_SCHEMA_VERSION: &str = "axiom.executor_request.v0";
pub const RESUME_SCHEMA_VERSION: &str = "axiom.executor_resume.v0";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Deterministic,
    Assisted,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionState {
    Planned,
    DryRun,
    Edited,
    EvidenceFailed,
    VerificationPassed,
    Resolved,
    RolledBack,
    Rejected,
    Escalated,
    Interrupted,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureCause {
    Code,
    Evidence,
    Environment,
    Conflict,
    Policy,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct BudgetUsage {
    pub time_seconds: u64,
    pub tokens: u64,
    pub retries: u32,
    pub cost_usd_micros: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutorBudgets {
    pub limits: Budgets,
    pub consumed: BudgetUsage,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReplaceTextProposal {
    pub path: String,
    pub expected_before_digest: String,
    pub find: String,
    pub replacement: String,
    pub candidate_digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionFailure {
    pub cause: FailureCause,
    pub fingerprint: String,
    pub message: String,
    pub attempt: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionEvent {
    pub sequence: u64,
    pub kind: String,
    pub detail: String,
    pub state: ExecutionState,
    pub budget_usage: BudgetUsage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_digest: Option<String>,
    pub previous_digest: String,
    pub event_digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SignedDeliveryEvidence {
    pub provider_id: String,
    pub candidate_digest: String,
    pub plan_digest: String,
    pub delivered_head_sha: String,
    pub transaction_id: String,
    pub policy_digest: String,
    pub fetched_at_epoch_seconds: u64,
    pub required_checks_passed: bool,
    pub review_satisfied: bool,
    pub conversations_resolved: bool,
    pub evidence_mac: String,
}

#[derive(Clone)]
pub struct ExecutorSealKey {
    key_id: String,
    secret: Vec<u8>,
}

#[derive(Clone)]
pub struct DeliveryEvidenceKey {
    provider_id: String,
    secret: Vec<u8>,
}

pub struct TrustedDeliveryEvidenceProvider {
    key: DeliveryEvidenceKey,
    max_age_seconds: u64,
}

impl ExecutorSealKey {
    pub fn from_secret(key_id: &str, secret: &[u8]) -> Result<Self, String> {
        validate_key(key_id, secret)?;
        Ok(Self {
            key_id: key_id.into(),
            secret: secret.into(),
        })
    }
    pub fn key_id(&self) -> &str {
        &self.key_id
    }
}

impl DeliveryEvidenceKey {
    pub fn from_secret(provider_id: &str, secret: &[u8]) -> Result<Self, String> {
        validate_key(provider_id, secret)?;
        Ok(Self {
            provider_id: provider_id.into(),
            secret: secret.into(),
        })
    }
    pub fn sign(
        &self,
        mut evidence: SignedDeliveryEvidence,
    ) -> Result<SignedDeliveryEvidence, String> {
        if evidence.provider_id != self.provider_id {
            return Err("delivery provider ID mismatch".into());
        }
        evidence.evidence_mac.clear();
        let bytes = serde_json::to_vec(&evidence).map_err(|error| error.to_string())?;
        evidence.evidence_mac = hmac_digest(&self.secret, b"axiom-delivery-evidence-v0", &bytes);
        Ok(evidence)
    }
}

impl TrustedDeliveryEvidenceProvider {
    pub fn new(key: DeliveryEvidenceKey, max_age_seconds: u64) -> Result<Self, String> {
        if max_age_seconds == 0 {
            return Err("delivery evidence freshness window must be positive".into());
        }
        Ok(Self {
            key,
            max_age_seconds,
        })
    }
    fn verify(&self, evidence: &SignedDeliveryEvidence) -> Result<(), String> {
        if evidence.provider_id != self.key.provider_id {
            return Err("untrusted delivery evidence provider".into());
        }
        let mut unsigned = evidence.clone();
        unsigned.evidence_mac.clear();
        let bytes = serde_json::to_vec(&unsigned).map_err(|error| error.to_string())?;
        let expected = hmac_digest(&self.key.secret, b"axiom-delivery-evidence-v0", &bytes);
        if !constant_time_eq(expected.as_bytes(), evidence.evidence_mac.as_bytes()) {
            return Err("delivery evidence MAC mismatch".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "system clock is before the Unix epoch")?
            .as_secs();
        if evidence.fetched_at_epoch_seconds > now
            || now - evidence.fetched_at_epoch_seconds > self.max_age_seconds
        {
            return Err("delivery evidence is stale or future-dated".into());
        }
        if !evidence.required_checks_passed
            || !evidence.review_satisfied
            || !evidence.conversations_resolved
        {
            return Err("delivery evidence is not satisfactory".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CandidateBinding {
    pub candidate_digest: String,
    pub delivered_head_sha: String,
    pub transaction_id: String,
    pub policy_digest: String,
    pub plan_digest: String,
    pub binding_digest: String,
}

impl CandidateBinding {
    fn new(
        candidate_digest: &str,
        delivered_head_sha: &str,
        transaction_id: &str,
        policy_digest: &str,
        plan_digest: &str,
    ) -> Self {
        let binding_digest = candidate_binding_digest(
            candidate_digest,
            delivered_head_sha,
            transaction_id,
            policy_digest,
            plan_digest,
        );
        Self {
            candidate_digest: candidate_digest.into(),
            delivered_head_sha: delivered_head_sha.into(),
            transaction_id: transaction_id.into(),
            policy_digest: policy_digest.into(),
            plan_digest: plan_digest.into(),
            binding_digest,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RetryPolicy {
    pub approved_causes: Vec<FailureCause>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutorRequest {
    pub schema_version: String,
    pub task_contract_digest: String,
    pub transaction_id: String,
    pub transaction_digest: String,
    pub base_sha: String,
    pub mode: ExecutionMode,
    pub dry_run: bool,
    pub budgets: Budgets,
    pub retry_policy: RetryPolicy,
    pub proposal: Option<ReplaceTextProposal>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutorResumeRequest {
    pub schema_version: String,
    pub executor_state_mac: String,
    pub seal_key_id: String,
    pub task_contract_digest: String,
    pub transaction_id: String,
    pub transaction_digest: String,
    pub candidate_digest: Option<String>,
    pub remaining_budgets: BudgetUsage,
    pub next_event_sequence: u64,
}

impl ExecutorRequest {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let request: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid executor request: {error}"))?;
        if request.schema_version != REQUEST_SCHEMA_VERSION {
            return Err("unsupported executor request schema".into());
        }
        Ok(request)
    }
}

impl ExecutorResumeRequest {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let request: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid executor resume request: {error}"))?;
        if request.schema_version != RESUME_SCHEMA_VERSION {
            return Err("unsupported executor resume schema".into());
        }
        Ok(request)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RollbackOutcome {
    NotRequired,
    Succeeded,
    Failed,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundedExecutor {
    schema_version: String,
    task_contract_digest: String,
    base_sha: String,
    dry_run: bool,
    state: ExecutionState,
    budgets: ExecutorBudgets,
    #[serde(skip_serializing_if = "Option::is_none")]
    proposal: Option<ReplaceTextProposal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidate_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    verification: Option<VerificationVerdict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidate_binding: Option<CandidateBinding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    delivery: Option<SignedDeliveryEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transaction_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_digest: Option<String>,
    rollback: RollbackOutcome,
    failures: Vec<ExecutionFailure>,
    events: Vec<ExecutionEvent>,
    state_digest: String,
    seal_key_id: String,
    seal_mac: String,
    #[serde(skip)]
    seal_key: Option<ExecutorSealKey>,
    retry_policy: RetryPolicy,
    allowed_files: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repair_allowed_files: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    interrupted_from: Option<ExecutionState>,
}

impl BoundedExecutor {
    pub fn from_request(
        request: ExecutorRequest,
        contract: &AgentTaskContract,
        workspace: &TransactionalWorkspace,
        current: Option<&[u8]>,
        seal_key: &ExecutorSealKey,
    ) -> Result<Self, String> {
        if request.schema_version != REQUEST_SCHEMA_VERSION
            || request.task_contract_digest != contract.contract_digest
            || request.transaction_id != workspace.state().transaction_id
            || request.transaction_digest != workspace.state().policy_digest
            || request.base_sha != workspace.state().base_sha
            || request.budgets != contract.budgets
            || request.mode != ExecutionMode::Deterministic
        {
            return Err("executor request exceeds or mismatches approved authority".into());
        }
        let mut executor = Self::new_with_retry_policy(
            contract,
            &request.base_sha,
            request.dry_run,
            request.retry_policy,
            seal_key,
        )?;
        executor.bind_transaction(workspace)?;
        match (request.proposal, current) {
            (Some(proposal), Some(bytes)) => {
                let validated = executor.propose_replace(
                    &proposal.path,
                    bytes,
                    &proposal.find,
                    &proposal.replacement,
                )?;
                if validated != proposal {
                    return Err(
                        "executor request proposal digest does not match observed bytes".into(),
                    );
                }
            }
            (None, None) => {}
            (Some(_), None) => {
                return Err("replace-text request requires observed current bytes".into());
            }
            (None, Some(_)) => return Err("current bytes supplied without a typed proposal".into()),
        }
        Ok(executor)
    }

    pub fn new(
        contract: &AgentTaskContract,
        base_sha: &str,
        dry_run: bool,
        seal_key: &ExecutorSealKey,
    ) -> Result<Self, String> {
        Self::new_with_retry_policy(
            contract,
            base_sha,
            dry_run,
            RetryPolicy::default(),
            seal_key,
        )
    }

    pub fn new_with_retry_policy(
        contract: &AgentTaskContract,
        base_sha: &str,
        dry_run: bool,
        retry_policy: RetryPolicy,
        seal_key: &ExecutorSealKey,
    ) -> Result<Self, String> {
        validate_sha(base_sha)?;
        validate_contract(contract)?;
        validate_retry_policy(&retry_policy)?;
        let authorized = authorized_retry_causes(contract);
        if retry_policy
            .approved_causes
            .iter()
            .any(|cause| !authorized.contains(cause))
        {
            return Err("retry policy is not authorized by the signed task contract".into());
        }
        if contract.authority.source_revision != base_sha {
            return Err("task authority source_revision does not match executor base SHA".into());
        }
        let mut executor = Self {
            schema_version: EXECUTOR_SCHEMA_VERSION.into(),
            task_contract_digest: contract.contract_digest.clone(),
            base_sha: base_sha.into(),
            dry_run,
            state: ExecutionState::Planned,
            budgets: ExecutorBudgets {
                limits: contract.budgets.clone(),
                consumed: BudgetUsage::default(),
            },
            proposal: None,
            candidate_digest: None,
            verification: None,
            candidate_binding: None,
            delivery: None,
            transaction_id: None,
            policy_digest: None,
            rollback: RollbackOutcome::NotRequired,
            failures: Vec::new(),
            events: Vec::new(),
            state_digest: empty_digest(),
            seal_key_id: seal_key.key_id.clone(),
            seal_mac: String::new(),
            seal_key: Some(seal_key.clone()),
            retry_policy,
            allowed_files: contract.scope.allowed_files.clone(),
            repair_allowed_files: contract
                .repair_plan
                .as_ref()
                .map(|repair| repair.allowed_files.clone()),
            interrupted_from: None,
        };
        executor.record("planned", &contract.id)?;
        Ok(executor)
    }

    pub fn state(&self) -> ExecutionState {
        self.state
    }
    pub fn budgets(&self) -> &ExecutorBudgets {
        &self.budgets
    }
    pub fn rollback_outcome(&self) -> RollbackOutcome {
        self.rollback
    }
    pub fn candidate_digest(&self) -> Option<&str> {
        self.candidate_digest.as_deref()
    }
    pub fn task_contract_digest(&self) -> &str {
        &self.task_contract_digest
    }
    pub fn seal_mac(&self) -> &str {
        &self.seal_mac
    }
    pub fn failures(&self) -> &[ExecutionFailure] {
        &self.failures
    }
    pub fn events(&self) -> &[ExecutionEvent] {
        &self.events
    }
    pub fn candidate_binding(&self) -> Option<&CandidateBinding> {
        self.candidate_binding.as_ref()
    }

    pub fn resume_request(&self) -> Result<ExecutorResumeRequest, String> {
        self.validate_integrity()?;
        Ok(ExecutorResumeRequest {
            schema_version: RESUME_SCHEMA_VERSION.into(),
            executor_state_mac: self.seal_mac.clone(),
            seal_key_id: self.seal_key_id.clone(),
            task_contract_digest: self.task_contract_digest.clone(),
            transaction_id: self
                .transaction_id
                .clone()
                .ok_or("executor is not transaction-bound")?,
            transaction_digest: self
                .policy_digest
                .clone()
                .ok_or("executor is not policy-bound")?,
            candidate_digest: self.candidate_digest.clone(),
            remaining_budgets: self.remaining_budgets()?,
            next_event_sequence: self.events.len() as u64,
        })
    }

    fn remaining_budgets(&self) -> Result<BudgetUsage, String> {
        Ok(BudgetUsage {
            time_seconds: self
                .budgets
                .limits
                .time_seconds
                .checked_sub(self.budgets.consumed.time_seconds)
                .ok_or("invalid time consumption")?,
            tokens: self
                .budgets
                .limits
                .tokens
                .checked_sub(self.budgets.consumed.tokens)
                .ok_or("invalid token consumption")?,
            retries: self
                .budgets
                .limits
                .retries
                .checked_sub(self.budgets.consumed.retries)
                .ok_or("invalid retry consumption")?,
            cost_usd_micros: self
                .budgets
                .limits
                .cost_usd_micros
                .checked_sub(self.budgets.consumed.cost_usd_micros)
                .ok_or("invalid cost consumption")?,
        })
    }

    /// Restore a serialized executor only when its immutable authority, base,
    /// budgets, and hash chain still match the approved task contract.
    pub fn recover(
        bytes: &[u8],
        request: &ExecutorResumeRequest,
        contract: &AgentTaskContract,
        workspace: &TransactionalWorkspace,
        seal_key: &ExecutorSealKey,
    ) -> Result<Self, String> {
        validate_contract(contract)?;
        let base_sha = &workspace.state().base_sha;
        validate_sha(base_sha)?;
        let mut executor: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid executor state: {error}"))?;
        executor.seal_key = Some(seal_key.clone());
        if request.schema_version != RESUME_SCHEMA_VERSION
            || request.executor_state_mac != executor.seal_mac
            || request.seal_key_id != executor.seal_key_id
            || request.seal_key_id != seal_key.key_id
            || request.task_contract_digest != executor.task_contract_digest
            || request.transaction_id != workspace.state().transaction_id
            || request.transaction_digest != workspace.state().policy_digest
            || request.candidate_digest != executor.candidate_digest
            || request.remaining_budgets != executor.remaining_budgets()?
            || request.next_event_sequence != executor.events.len() as u64
        {
            return Err("resume request does not match sealed executor state".into());
        }
        if executor.schema_version != EXECUTOR_SCHEMA_VERSION
            || executor.task_contract_digest != contract.contract_digest
            || executor.base_sha != *base_sha
            || executor.budgets.limits != contract.budgets
            || executor.allowed_files != contract.scope.allowed_files
            || executor.repair_allowed_files
                != contract
                    .repair_plan
                    .as_ref()
                    .map(|repair| repair.allowed_files.clone())
        {
            return Err(
                "executor state authority does not match the approved task contract".into(),
            );
        }
        if workspace.state().task_contract_digest != executor.task_contract_digest
            || executor.transaction_id.as_deref() != Some(&workspace.state().transaction_id)
            || executor.policy_digest.as_deref() != Some(&workspace.state().policy_digest)
        {
            return Err("executor recovery does not match the transaction binding".into());
        }
        if exceeds(&executor.budgets.consumed, &executor.budgets.limits) {
            return Err("executor state contains widened budget consumption".into());
        }
        if executor.state == ExecutionState::Interrupted {
            if executor.interrupted_from.is_none() {
                return Err("interrupted executor is missing its prior state".into());
            }
        } else if executor.interrupted_from.is_some() {
            return Err("non-interrupted executor contains stale recovery state".into());
        }
        if let Some(proposal) = &executor.proposal {
            validate_digest(&proposal.expected_before_digest)?;
            validate_digest(&proposal.candidate_digest)?;
            if !executor.allowed_files.contains(&proposal.path)
                || executor
                    .repair_allowed_files
                    .as_ref()
                    .is_some_and(|paths| !paths.contains(&proposal.path))
            {
                return Err("serialized proposal escapes approved task scope".into());
            }
        }
        executor.validate_integrity()?;
        Ok(executor)
    }

    /// Validate a typed proposal and compute the exact resulting file digest.
    /// The caller-supplied bytes are observed data, not authorization.
    pub fn propose_replace(
        &mut self,
        path: &str,
        current: &[u8],
        find: &str,
        replacement: &str,
    ) -> Result<ReplaceTextProposal, String> {
        self.guard()?;
        self.require_state(ExecutionState::Planned)?;
        self.authorize_path(path)?;
        if find.is_empty() {
            return self.reject("replacement search text must not be empty");
        }
        let source = std::str::from_utf8(current)
            .map_err(|_| "replace-text proposals require UTF-8 input".to_string())?;
        if source.matches(find).count() != 1 {
            return self.reject("replacement search text must occur exactly once");
        }
        let candidate = source.replacen(find, replacement, 1).into_bytes();
        let proposal = ReplaceTextProposal {
            path: path.into(),
            expected_before_digest: digest(current),
            find: find.into(),
            replacement: replacement.into(),
            candidate_digest: digest(&candidate),
        };
        self.proposal = Some(proposal.clone());
        self.candidate_digest = Some(proposal.candidate_digest.clone());
        if self.dry_run {
            self.state = ExecutionState::DryRun;
        }
        self.record("proposal_validated", path)?;
        Ok(proposal)
    }

    /// Bind resumable state to an existing task transaction without mutating it.
    pub fn bind_transaction(&mut self, workspace: &TransactionalWorkspace) -> Result<(), String> {
        self.guard()?;
        self.require_state(ExecutionState::Planned)?;
        self.validate_workspace(workspace)?;
        self.transaction_id = Some(workspace.state().transaction_id.clone());
        self.policy_digest = Some(workspace.state().policy_digest.clone());
        self.record("transaction_bound", &workspace.state().transaction_id)
    }

    /// Apply only the previously validated proposal through the transaction.
    /// Dry-run returns without reading or writing the workspace.
    pub fn apply_proposal(&mut self, workspace: &mut TransactionalWorkspace) -> Result<(), String> {
        self.guard()?;
        if self.dry_run {
            return Err("dry-run is side-effect free and cannot apply a proposal".into());
        }
        self.require_state(ExecutionState::Planned)?;
        let proposal = self
            .proposal
            .clone()
            .ok_or_else(|| "no validated proposal exists".to_string())?;
        self.validate_workspace(workspace)?;
        let current = workspace.read(&proposal.path).map_err(|error| {
            self.failure_error(FailureCause::Environment, "workspace_read", &error)
        })?;
        if digest(&current) != proposal.expected_before_digest {
            return Err(self.failure_error(
                FailureCause::Conflict,
                "candidate_base_changed",
                "workspace content differs from the proposed base",
            ));
        }
        let source = std::str::from_utf8(&current)
            .map_err(|_| "workspace content is no longer UTF-8".to_string())?;
        if source.matches(&proposal.find).count() != 1 {
            return Err(self.failure_error(
                FailureCause::Conflict,
                "candidate_match_changed",
                "replacement search text no longer occurs exactly once",
            ));
        }
        let candidate = source
            .replacen(&proposal.find, &proposal.replacement, 1)
            .into_bytes();
        if digest(&candidate) != proposal.candidate_digest {
            return Err(self.failure_error(
                FailureCause::Policy,
                "candidate_digest_mismatch",
                "proposal does not reproduce its exact candidate digest",
            ));
        }
        workspace
            .write(&proposal.path, &candidate)
            .map_err(|error| self.failure_error(FailureCause::Policy, "workspace_write", &error))?;
        self.transaction_id = Some(workspace.state().transaction_id.clone());
        self.policy_digest = Some(workspace.state().policy_digest.clone());
        self.state = ExecutionState::Edited;
        self.record("proposal_applied", &proposal.candidate_digest)
    }

    pub fn submit_verification(
        &mut self,
        workspace: &mut TransactionalWorkspace,
        verdict: VerificationVerdict,
    ) -> Result<(), String> {
        self.guard()?;
        self.require_state(ExecutionState::Edited)?;
        self.validate_workspace(workspace)?;
        if verdict.schema_version != VERDICT_SCHEMA_VERSION
            || validate_digest(&verdict.plan_digest).is_err()
            || validate_sha(&verdict.source_head_sha).is_err()
            || validate_sha(&verdict.delivered_head_sha).is_err()
        {
            return Err(self.failure_error(
                FailureCause::Evidence,
                "verification_schema",
                "unsupported verification verdict schema",
            ));
        }
        if verdict.source_head_sha != self.base_sha {
            return Err(self.failure_error(
                FailureCause::Conflict,
                "verification_base",
                "verification verdict is bound to another source head",
            ));
        }
        if verdict.delivered_head_sha == self.base_sha {
            return Err(self.failure_error(
                FailureCause::Evidence,
                "verification_unchanged_head",
                "verification must cover a delivered head containing the candidate edit",
            ));
        }
        let proposal = self
            .proposal
            .clone()
            .ok_or_else(|| "verification requires an applied proposal".to_string())?;
        let delivered = workspace
            .read_at_commit(&proposal.path, &verdict.delivered_head_sha)
            .map_err(|error| {
                self.failure_error(FailureCause::Evidence, "candidate_head_proof", &error)
            })?;
        if digest(&delivered) != proposal.candidate_digest {
            return Err(self.failure_error(
                FailureCause::Evidence,
                "candidate_head_binding",
                "delivered head does not contain the exact candidate bytes",
            ));
        }
        let binding = CandidateBinding::new(
            &proposal.candidate_digest,
            &verdict.delivered_head_sha,
            self.transaction_id
                .as_deref()
                .ok_or("executor is not transaction-bound")?,
            self.policy_digest
                .as_deref()
                .ok_or("executor is not policy-bound")?,
            &verdict.plan_digest,
        );
        self.verification = Some(verdict.clone());
        self.candidate_binding = Some(binding);
        let has_failures = !verdict.missing.is_empty()
            || !verdict.duplicate.is_empty()
            || !verdict.invalid.is_empty()
            || !verdict.failed.is_empty();
        if (verdict.status == VerdictStatus::Passed && has_failures)
            || (verdict.status == VerdictStatus::Failed && !has_failures)
        {
            self.state = ExecutionState::Escalated;
            self.record("verification_contradictory", &verdict.plan_digest)?;
            return Err("contradictory verification evidence requires escalation".into());
        }
        if verdict.status == VerdictStatus::Failed {
            self.state = ExecutionState::EvidenceFailed;
            self.record("verification_failed", &verdict.plan_digest)?;
            return Ok(());
        }
        self.state = ExecutionState::VerificationPassed;
        self.record("verification_passed", &verdict.plan_digest)
    }

    /// Delivery state is supplied as typed evidence. The Class-2 executor has
    /// no delivery mutation primitive and cannot manufacture this verdict.
    pub fn submit_delivery(
        &mut self,
        workspace: &TransactionalWorkspace,
        evidence: SignedDeliveryEvidence,
        provider: &TrustedDeliveryEvidenceProvider,
    ) -> Result<(), String> {
        self.guard()?;
        self.require_state(ExecutionState::VerificationPassed)?;
        self.validate_workspace(workspace)?;
        provider.verify(&evidence).map_err(|error| {
            self.failure_error(FailureCause::Evidence, "delivery_provider", &error)
        })?;
        if self.candidate_digest.as_deref() != Some(&evidence.candidate_digest) {
            return Err(self.failure_error(
                FailureCause::Conflict,
                "delivery_candidate",
                "delivery verdict is bound to another candidate",
            ));
        }
        let verification = self
            .verification
            .as_ref()
            .ok_or_else(|| "delivery recheck requires a verification verdict".to_string())?;
        let binding = self
            .candidate_binding
            .as_ref()
            .ok_or_else(|| "delivery recheck requires a candidate binding".to_string())?;
        if evidence.plan_digest != verification.plan_digest
            || evidence.delivered_head_sha != verification.delivered_head_sha
            || evidence.transaction_id != binding.transaction_id
            || evidence.policy_digest != binding.policy_digest
            || validate_sha(&evidence.delivered_head_sha).is_err()
        {
            return Err(self.failure_error(
                FailureCause::Evidence,
                "delivery_binding",
                "delivery verdict is not bound to the verified plan and head",
            ));
        }
        self.delivery = Some(evidence.clone());
        self.state = ExecutionState::Resolved;
        self.record("resolved", &evidence.candidate_digest)
    }

    pub fn rollback(&mut self, workspace: &mut TransactionalWorkspace) -> Result<(), String> {
        self.guard()?;
        if matches!(
            self.state,
            ExecutionState::Resolved | ExecutionState::DryRun | ExecutionState::RolledBack
        ) {
            return Err("executor state cannot be rolled back".into());
        }
        self.validate_workspace(workspace)?;
        match workspace.abort() {
            Ok(()) => {
                self.rollback = RollbackOutcome::Succeeded;
                self.state = ExecutionState::RolledBack;
                self.record("rolled_back", &workspace.state().transaction_id)
            }
            Err(error) => {
                self.rollback = RollbackOutcome::Failed;
                self.state = ExecutionState::Escalated;
                let _ = self.record("rollback_failed", &error);
                Err(error)
            }
        }
    }

    pub fn charge_budget(&mut self, usage: BudgetUsage) -> Result<(), String> {
        self.guard()?;
        self.ensure_nonterminal()?;
        let next = BudgetUsage {
            time_seconds: self
                .budgets
                .consumed
                .time_seconds
                .checked_add(usage.time_seconds)
                .ok_or_else(|| "time budget overflow".to_string())?,
            tokens: self
                .budgets
                .consumed
                .tokens
                .checked_add(usage.tokens)
                .ok_or_else(|| "token budget overflow".to_string())?,
            retries: self
                .budgets
                .consumed
                .retries
                .checked_add(usage.retries)
                .ok_or_else(|| "retry budget overflow".to_string())?,
            cost_usd_micros: self
                .budgets
                .consumed
                .cost_usd_micros
                .checked_add(usage.cost_usd_micros)
                .ok_or_else(|| "cost budget overflow".to_string())?,
        };
        if next.time_seconds > self.budgets.limits.time_seconds
            || next.tokens > self.budgets.limits.tokens
            || next.retries > self.budgets.limits.retries
            || next.cost_usd_micros > self.budgets.limits.cost_usd_micros
        {
            self.state = ExecutionState::Escalated;
            self.record("budget_exhausted", "execution")?;
            return Err("execution budget exhausted; escalation required".into());
        }
        self.budgets.consumed = next;
        self.record("budget_charged", "execution")
    }

    /// Records a retryable failure. Identical consecutive failures stop, and
    /// unknown/policy failures always escalate without retrying.
    pub fn retry_failure(
        &mut self,
        cause: FailureCause,
        code: &str,
        message: &str,
    ) -> Result<(), String> {
        self.guard()?;
        self.ensure_nonterminal()?;
        let fingerprint = digest(format!("{cause:?}\0{code}\0{message}").as_bytes());
        let repeated = self
            .failures
            .last()
            .is_some_and(|prior| prior.fingerprint == fingerprint);
        let attempt = self.budgets.consumed.retries.saturating_add(1);
        self.failures.push(ExecutionFailure {
            cause,
            fingerprint,
            message: message.into(),
            attempt,
        });
        if matches!(cause, FailureCause::Unknown | FailureCause::Policy)
            || !self.retry_policy.approved_causes.contains(&cause)
            || repeated
        {
            self.state = ExecutionState::Escalated;
            self.record("failure_escalated", code)?;
            return Err("failure requires escalation".into());
        }
        let next = self
            .budgets
            .consumed
            .retries
            .checked_add(1)
            .ok_or_else(|| "retry budget overflow".to_string())?;
        if next > self.budgets.limits.retries {
            self.state = ExecutionState::Escalated;
            self.record("budget_exhausted", "retry")?;
            return Err("execution retry budget exhausted; escalation required".into());
        }
        self.budgets.consumed.retries = next;
        self.record("retry_approved", code)
    }

    pub fn interrupt(&mut self) -> Result<(), String> {
        self.guard()?;
        self.ensure_nonterminal()?;
        if self.state == ExecutionState::Interrupted {
            return Err("executor is already interrupted".into());
        }
        let prior = self.state;
        self.interrupted_from = Some(prior);
        self.state = ExecutionState::Interrupted;
        self.record("interrupted", &format!("{prior:?}"))
    }

    pub fn resume(&mut self) -> Result<(), String> {
        self.guard()?;
        self.require_state(ExecutionState::Interrupted)?;
        let restored = self
            .interrupted_from
            .take()
            .ok_or_else(|| "interrupted executor is missing its prior state".to_string())?;
        self.state = restored;
        self.record("resumed", "execution")
    }

    pub fn validate_chain(&self) -> Result<(), String> {
        let mut previous = empty_digest();
        let mut prior_usage = BudgetUsage::default();
        for (index, event) in self.events.iter().enumerate() {
            if event.sequence != index as u64 || event.previous_digest != previous {
                return Err("executor event chain is discontinuous".into());
            }
            if event.budget_usage.time_seconds < prior_usage.time_seconds
                || event.budget_usage.tokens < prior_usage.tokens
                || event.budget_usage.retries < prior_usage.retries
                || event.budget_usage.cost_usd_micros < prior_usage.cost_usd_micros
            {
                return Err("executor event budgets are not monotonic".into());
            }
            let expected = event_digest(event, &previous)?;
            if event.event_digest != expected {
                return Err("executor event digest mismatch".into());
            }
            previous = expected;
            prior_usage = event.budget_usage.clone();
        }
        if self.state_digest != previous {
            return Err("executor state digest mismatch".into());
        }
        if let Some(last) = self.events.last() {
            if last.state != self.state
                || last.budget_usage != self.budgets.consumed
                || last.candidate_digest != self.candidate_digest
            {
                return Err("executor state does not match its final chained snapshot".into());
            }
        }
        Ok(())
    }

    pub fn validate_integrity(&self) -> Result<(), String> {
        self.validate_chain()?;
        let expected = self.compute_seal()?;
        if !constant_time_eq(self.seal_mac.as_bytes(), expected.as_bytes()) {
            return Err("executor authority-state seal mismatch".into());
        }
        Ok(())
    }

    fn authorize_path(&mut self, path: &str) -> Result<(), String> {
        if !canonical_relative(path) || !self.allowed_files.iter().any(|allowed| allowed == path) {
            return self.reject("proposal path is outside the task allowed_files");
        }
        if self
            .repair_allowed_files
            .as_ref()
            .is_some_and(|allowed| !allowed.iter().any(|candidate| candidate == path))
        {
            return self.reject("proposal path is outside the repair-plan allowed_files");
        }
        Ok(())
    }

    fn validate_workspace(&mut self, workspace: &TransactionalWorkspace) -> Result<(), String> {
        let state = workspace.state();
        if state.task_contract_digest != self.task_contract_digest {
            return self.reject("workspace is bound to another task contract");
        }
        if state.base_sha != self.base_sha {
            return self.reject("workspace is bound to another base SHA");
        }
        if state.phase != TransactionPhase::Active {
            return self.reject("workspace transaction is not active");
        }
        if let Some(transaction_id) = &self.transaction_id {
            if transaction_id != &state.transaction_id
                || self.policy_digest.as_deref() != Some(&state.policy_digest)
            {
                return self.reject("workspace no longer matches the sealed transaction binding");
            }
        }
        if state.policy.allow_network {
            return self.reject("workspace policy grants forbidden network authority");
        }
        if !state.policy.allowed_commands.is_empty() {
            return self.reject("workspace policy grants unsupported command authority");
        }
        if state
            .policy
            .allowed_write_paths
            .iter()
            .any(|path| !self.allowed_files.contains(path))
        {
            return self.reject("workspace write policy exceeds the task allowed_files");
        }
        if self.repair_allowed_files.as_ref().is_some_and(|repair| {
            state
                .policy
                .allowed_write_paths
                .iter()
                .any(|path| !repair.contains(path))
        }) {
            return self.reject("workspace write policy exceeds the repair allowed_files");
        }
        Ok(())
    }

    fn failure_error(&mut self, cause: FailureCause, code: &str, message: &str) -> String {
        let _ = self.retry_failure(cause, code, message);
        message.into()
    }

    fn reject<T>(&mut self, message: &str) -> Result<T, String> {
        self.state = ExecutionState::Rejected;
        self.failures.push(ExecutionFailure {
            cause: FailureCause::Policy,
            fingerprint: digest(message.as_bytes()),
            message: message.into(),
            attempt: self.budgets.consumed.retries,
        });
        let _ = self.record("rejected", message);
        Err(message.into())
    }

    fn require_state(&self, expected: ExecutionState) -> Result<(), String> {
        if self.state == expected {
            Ok(())
        } else {
            Err(format!("executor must be in {expected:?} state"))
        }
    }

    fn ensure_nonterminal(&self) -> Result<(), String> {
        if matches!(
            self.state,
            ExecutionState::Resolved
                | ExecutionState::RolledBack
                | ExecutionState::Rejected
                | ExecutionState::Escalated
        ) {
            Err("executor is terminal".into())
        } else {
            Ok(())
        }
    }

    fn record(&mut self, kind: &str, detail: &str) -> Result<(), String> {
        let previous = self.state_digest.clone();
        let sequence = self.events.len() as u64;
        let mut event = ExecutionEvent {
            sequence,
            kind: kind.into(),
            detail: detail.into(),
            state: self.state,
            budget_usage: self.budgets.consumed.clone(),
            candidate_digest: self.candidate_digest.clone(),
            previous_digest: previous,
            event_digest: String::new(),
        };
        event.event_digest = event_digest(&event, &event.previous_digest)?;
        let event_digest = event.event_digest.clone();
        self.events.push(event);
        self.state_digest = event_digest;
        self.seal_mac = self.compute_seal()?;
        Ok(())
    }

    fn compute_seal(&self) -> Result<String, String> {
        let mut snapshot = self.clone();
        snapshot.seal_mac.clear();
        let key = self
            .seal_key
            .as_ref()
            .ok_or("executor seal key is unavailable")?;
        if key.key_id != self.seal_key_id {
            return Err("executor seal key ID mismatch".into());
        }
        let bytes = serde_json::to_vec(&snapshot).map_err(|error| error.to_string())?;
        Ok(hmac_digest(&key.secret, b"axiom-executor-state-v0", &bytes))
    }

    fn guard(&self) -> Result<(), String> {
        self.validate_integrity()
    }
}

fn validate_contract(contract: &AgentTaskContract) -> Result<(), String> {
    if contract.schema_version != TASK_SCHEMA_VERSION {
        return Err("unsupported agent task contract schema".into());
    }
    validate_digest(&contract.contract_digest)?;
    let mut canonical = contract.clone();
    canonical.contract_digest.clear();
    let encoded = serde_json::to_vec(&canonical).map_err(|error| error.to_string())?;
    if digest(&encoded) != contract.contract_digest {
        return Err("agent task contract digest mismatch".into());
    }
    if contract.scope.allowed_files.is_empty() {
        return Err("task has no allowed files".into());
    }
    if let Some(repair) = &contract.repair_plan {
        if repair.allowed_files.is_empty()
            || repair
                .allowed_files
                .iter()
                .any(|path| !contract.scope.allowed_files.contains(path))
        {
            return Err("repair-plan scope is not a non-empty task-scope subset".into());
        }
    }
    if contract.delivery_permissions.commit
        || contract.delivery_permissions.push
        || contract.delivery_permissions.pull_request
        || contract.delivery_permissions.merge
        || contract.delivery_permissions.review
        || contract.delivery_permissions.deploy
        || contract.delivery_permissions.approve_own_pull_request
        || contract.delivery_permissions.force_push
        || contract.delivery_permissions.direct_push_protected
        || contract.delivery_permissions.irreversible_actions
    {
        return Err("bounded executor refuses delivery or irreversible authority".into());
    }
    Ok(())
}

fn exceeds(usage: &BudgetUsage, limits: &Budgets) -> bool {
    usage.time_seconds > limits.time_seconds
        || usage.tokens > limits.tokens
        || usage.retries > limits.retries
        || usage.cost_usd_micros > limits.cost_usd_micros
}

fn validate_retry_policy(policy: &RetryPolicy) -> Result<(), String> {
    if policy
        .approved_causes
        .iter()
        .any(|cause| matches!(cause, FailureCause::Unknown | FailureCause::Policy))
    {
        Err("unknown and policy failures may never be approved for retry".into())
    } else {
        let mut causes = policy.approved_causes.clone();
        causes.sort_by_key(|cause| format!("{cause:?}"));
        causes.dedup();
        if causes.len() != policy.approved_causes.len() {
            Err("retry policy contains duplicate causes".into())
        } else {
            Ok(())
        }
    }
}

/// Retry authority is derived only from capability names inside the signed
/// task contract; an executor request can narrow this set but never add to it.
fn authorized_retry_causes(contract: &AgentTaskContract) -> Vec<FailureCause> {
    [
        ("executor.retry.code", FailureCause::Code),
        ("executor.retry.evidence", FailureCause::Evidence),
        ("executor.retry.environment", FailureCause::Environment),
        ("executor.retry.conflict", FailureCause::Conflict),
    ]
    .into_iter()
    .filter_map(|(name, cause)| {
        contract
            .capabilities
            .required
            .iter()
            .any(|item| item == name)
            .then_some(cause)
    })
    .collect()
}

fn candidate_binding_digest(
    candidate: &str,
    head: &str,
    transaction: &str,
    policy: &str,
    plan: &str,
) -> String {
    digest(format!("{candidate}\0{head}\0{transaction}\0{policy}\0{plan}").as_bytes())
}

fn canonical_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn validate_sha(value: &str) -> Result<(), String> {
    if value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err("base SHA must be 40 lowercase hexadecimal characters".into())
    }
}

fn validate_digest(value: &str) -> Result<(), String> {
    if value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err("digest must use sha256:<64 lowercase hex> form".into())
    }
}

fn validate_key(key_id: &str, secret: &[u8]) -> Result<(), String> {
    if key_id.is_empty()
        || key_id.len() > 64
        || !key_id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
    {
        return Err("key ID must be 1-64 canonical lowercase ASCII characters".into());
    }
    if secret.len() < 32 {
        return Err("authentication keys must contain at least 32 bytes".into());
    }
    Ok(())
}

fn hmac_digest(key: &[u8], domain: &[u8], payload: &[u8]) -> String {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&sha256_raw(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Vec::with_capacity(64 + domain.len() + 1 + payload.len());
    inner.extend(block.iter().map(|byte| byte ^ 0x36));
    inner.extend_from_slice(domain);
    inner.push(0);
    inner.extend_from_slice(payload);
    let inner_hash = sha256_raw(&inner);
    let mut outer = Vec::with_capacity(96);
    outer.extend(block.iter().map(|byte| byte ^ 0x5c));
    outer.extend_from_slice(&inner_hash);
    format!("hmac-sha256:{}", hex_bytes(&sha256_raw(&outer)))
}

fn sha256_raw(bytes: &[u8]) -> [u8; 32] {
    let encoded = sha256(bytes);
    let mut output = [0u8; 32];
    for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
        output[index] = (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]);
    }
    output
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => unreachable!("SHA output is hex"),
    }
}
fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

fn empty_digest() -> String {
    digest(b"")
}

fn event_digest(event: &ExecutionEvent, previous: &str) -> Result<String, String> {
    let snapshot = serde_json::to_vec(&(
        event.sequence,
        &event.kind,
        &event.detail,
        event.state,
        &event.budget_usage,
        &event.candidate_digest,
        previous,
    ))
    .map_err(|error| error.to_string())?;
    Ok(digest(&snapshot))
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256(bytes))
}

fn sha256(bytes: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut message = bytes.to_vec();
    let bit_len = (message.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());
    let mut h = [
        0x6a09e667u32,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];
    for block in message.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (index, word) in block.chunks_exact(4).enumerate() {
            w[index] = u32::from_be_bytes(word.try_into().expect("SHA word"));
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for index in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for index in 0..8 {
            h[index] = h[index].wrapping_add(v[index]);
        }
    }
    h.iter().map(|word| format!("{word:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_task::*;

    fn seal_key() -> ExecutorSealKey {
        ExecutorSealKey::from_secret("test-seal", &[7u8; 32]).unwrap()
    }

    fn contract() -> AgentTaskContract {
        let mut value = AgentTaskContract {
            schema_version: TASK_SCHEMA_VERSION.into(),
            contract_digest: String::new(),
            id: "task-1422".into(),
            task_kind: TaskKind::Repair,
            objective: "repair one file".into(),
            authority: Authority {
                governing_issue: "https://github.com/OMT-Global/axiomlang/issues/1422".into(),
                repository: "OMT-Global/axiomlang".into(),
                issue: 1422,
                source_revision: "c".repeat(40),
                source_digest: format!("sha256:{}", "b".repeat(64)),
                approval: Approval {
                    state: AuthorityStatus::Approved,
                    approver: "Pheidon".into(),
                    method: "maintainer_review".into(),
                },
            },
            scope: Scope {
                affected_semantic_nodes: vec!["axiom://package/test".into()],
                allowed_files: vec!["src/main.ax".into()],
                denied_files: vec![],
            },
            capabilities: PermissionSet {
                required: vec!["executor.retry.code".into()],
                forbidden: vec!["network".into()],
            },
            commands: PermissionSet {
                required: vec![],
                forbidden: vec!["git push".into()],
            },
            acceptance_criteria: vec![],
            required_evidence: vec![],
            dependencies: vec![],
            autonomy: Autonomy {
                class: 2,
                risk: RiskClass::Medium,
            },
            budgets: Budgets {
                time_seconds: 10,
                tokens: 20,
                retries: 2,
                cost_usd_micros: 30,
            },
            rollback: RollbackPlan {
                checkpoint: "base".into(),
                commands: vec!["rollback".into()],
            },
            terminal_conditions: TerminalConditions {
                success: vec!["verified".into()],
                stop: vec!["budget".into()],
                escalation: vec!["unknown".into()],
            },
            delivery_permissions: DeliveryPermissions {
                commit: false,
                push: false,
                pull_request: false,
                review: false,
                merge: false,
                deploy: false,
                approve_own_pull_request: false,
                force_push: false,
                direct_push_protected: false,
                irreversible_actions: false,
            },
            repair_plan: Some(RepairPlan {
                id: "repair-1".into(),
                reason: "diagnostic".into(),
                target_node: "axiom://package/test".into(),
                allowed_files: vec!["src/main.ax".into()],
                required_evidence: vec![],
                diagnostics: vec![],
            }),
        };
        let bytes = serde_json::to_vec(&value).unwrap();
        value.contract_digest = digest(&bytes);
        value
    }

    #[test]
    fn proposal_is_exact_deterministic_and_dry_run_is_side_effect_free() {
        let mut executor =
            BoundedExecutor::new(&contract(), &"c".repeat(40), true, &seal_key()).unwrap();
        let proposal = executor
            .propose_replace("src/main.ax", b"old\n", "old", "new")
            .unwrap();
        assert_eq!(proposal.candidate_digest, digest(b"new\n"));
        assert_eq!(executor.state, ExecutionState::DryRun);
        assert!(executor.validate_chain().is_ok());
    }

    #[test]
    fn rejects_scope_escape_and_repair_scope_widening() {
        let mut executor =
            BoundedExecutor::new(&contract(), &"c".repeat(40), false, &seal_key()).unwrap();
        assert!(
            executor
                .propose_replace("../main.ax", b"old", "old", "new")
                .is_err()
        );
        assert_eq!(executor.state, ExecutionState::Rejected);
        let mut invalid = contract();
        invalid
            .repair_plan
            .as_mut()
            .unwrap()
            .allowed_files
            .push("other.ax".into());
        assert!(BoundedExecutor::new(&invalid, &"c".repeat(40), false, &seal_key()).is_err());
    }

    #[test]
    fn rejects_tampered_contract_digest_and_delivery_authority() {
        let mut invalid = contract();
        invalid.objective.push('!');
        assert!(BoundedExecutor::new(&invalid, &"c".repeat(40), false, &seal_key()).is_err());
        assert!(BoundedExecutor::new(&contract(), &"d".repeat(40), false, &seal_key()).is_err());
        let mut invalid = contract();
        invalid.delivery_permissions.merge = true;
        let bytes = {
            invalid.contract_digest.clear();
            serde_json::to_vec(&invalid).unwrap()
        };
        invalid.contract_digest = digest(&bytes);
        assert!(BoundedExecutor::new(&invalid, &"c".repeat(40), false, &seal_key()).is_err());
    }

    #[test]
    fn budgets_are_monotonic_and_exhaustion_escalates() {
        let mut executor =
            BoundedExecutor::new(&contract(), &"c".repeat(40), false, &seal_key()).unwrap();
        executor
            .charge_budget(BudgetUsage {
                tokens: 20,
                ..BudgetUsage::default()
            })
            .unwrap();
        assert!(
            executor
                .charge_budget(BudgetUsage {
                    tokens: 1,
                    ..BudgetUsage::default()
                })
                .is_err()
        );
        assert_eq!(executor.budgets.consumed.tokens, 20);
        assert_eq!(executor.state, ExecutionState::Escalated);
    }

    #[test]
    fn identical_failure_and_unknown_failure_escalate() {
        let mut executor = BoundedExecutor::new_with_retry_policy(
            &contract(),
            &"c".repeat(40),
            false,
            RetryPolicy {
                approved_causes: vec![FailureCause::Code],
            },
            &seal_key(),
        )
        .unwrap();
        executor
            .retry_failure(FailureCause::Code, "E1", "same")
            .unwrap();
        assert!(
            executor
                .retry_failure(FailureCause::Code, "E1", "same")
                .is_err()
        );
        assert_eq!(executor.state, ExecutionState::Escalated);
        let mut executor =
            BoundedExecutor::new(&contract(), &"c".repeat(40), false, &seal_key()).unwrap();
        assert!(
            executor
                .retry_failure(FailureCause::Unknown, "E2", "unknown")
                .is_err()
        );
    }

    #[test]
    fn interruption_resume_preserves_hash_chain_and_state() {
        let mut executor =
            BoundedExecutor::new(&contract(), &"c".repeat(40), true, &seal_key()).unwrap();
        executor
            .propose_replace("src/main.ax", b"old", "old", "new")
            .unwrap();
        executor.interrupt().unwrap();
        executor.resume().unwrap();
        assert_eq!(executor.state, ExecutionState::DryRun);
        assert!(executor.validate_chain().is_ok());
        executor.events[1].budget_usage.tokens = 1;
        assert!(executor.validate_chain().is_err());
    }

    #[test]
    fn seal_rejects_tampering_with_every_resumable_authority_family() {
        let mut original =
            BoundedExecutor::new(&contract(), &"c".repeat(40), true, &seal_key()).unwrap();
        original
            .propose_replace("src/main.ax", b"old", "old", "new")
            .unwrap();
        assert!(original.validate_integrity().is_ok());

        let mut tampered = original.clone();
        tampered.proposal.as_mut().unwrap().replacement.push('!');
        assert!(tampered.validate_integrity().is_err());
        let mut tampered = original.clone();
        tampered.transaction_id = Some("txn-0000000000000000".into());
        assert!(tampered.validate_integrity().is_err());
        let mut tampered = original.clone();
        tampered.policy_digest = Some(digest(b"widened"));
        assert!(tampered.validate_integrity().is_err());
        let mut tampered = original.clone();
        tampered.rollback = RollbackOutcome::Succeeded;
        assert!(tampered.validate_integrity().is_err());
        let mut tampered = original.clone();
        tampered.interrupted_from = Some(ExecutionState::Planned);
        assert!(tampered.validate_integrity().is_err());
        let mut tampered = original.clone();
        tampered.failures.push(ExecutionFailure {
            cause: FailureCause::Code,
            fingerprint: digest(b"x"),
            message: "x".into(),
            attempt: 1,
        });
        assert!(tampered.validate_integrity().is_err());
        let mut tampered = original.clone();
        tampered.verification = Some(VerificationVerdict {
            schema_version: VERDICT_SCHEMA_VERSION.into(),
            plan_digest: digest(b"plan"),
            status: VerdictStatus::Passed,
            source_head_sha: "c".repeat(40),
            delivered_head_sha: "d".repeat(40),
            missing: vec![],
            duplicate: vec![],
            invalid: vec![],
            failed: vec![],
        });
        assert!(tampered.validate_integrity().is_err());
        let mut tampered = original.clone();
        tampered.delivery = Some(SignedDeliveryEvidence {
            provider_id: "test-provider".into(),
            candidate_digest: digest(b"new"),
            plan_digest: digest(b"plan"),
            delivered_head_sha: "d".repeat(40),
            transaction_id: "txn-0000000000000000".into(),
            policy_digest: digest(b"policy"),
            fetched_at_epoch_seconds: 1,
            required_checks_passed: true,
            review_satisfied: true,
            conversations_resolved: true,
            evidence_mac: "hmac-sha256:00".into(),
        });
        assert!(tampered.validate_integrity().is_err());
    }

    #[test]
    fn authenticated_seal_rejects_plain_rehash_and_wrong_key() {
        let mut executor =
            BoundedExecutor::new(&contract(), &"c".repeat(40), true, &seal_key()).unwrap();
        executor
            .propose_replace("src/main.ax", b"old", "old", "new")
            .unwrap();
        let encoded = serde_json::to_vec(&executor).unwrap();
        assert!(!encoded.windows(32).any(|window| window == [7u8; 32]));

        executor.proposal.as_mut().unwrap().replacement = "attacker".into();
        let mut unkeyed = executor.clone();
        unkeyed.seal_mac.clear();
        executor.seal_mac = digest(&serde_json::to_vec(&unkeyed).unwrap());
        assert!(executor.validate_integrity().is_err());

        let mut wrong_key =
            BoundedExecutor::new(&contract(), &"c".repeat(40), true, &seal_key()).unwrap();
        wrong_key.seal_key = Some(ExecutorSealKey::from_secret("test-seal", &[8u8; 32]).unwrap());
        assert!(wrong_key.validate_integrity().is_err());
    }

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hmac_digest(&[b'k'; 32], b"domain", b"payload"),
            "hmac-sha256:865e52dc58092803db170c127fb28f8387e65bd82a65417a0971fa350ca011b6"
        );
    }
}
