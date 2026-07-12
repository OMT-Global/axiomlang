//! Fail-closed compilation of approved specifications into bounded agent tasks.

use crate::diagnostics::Diagnostic;
use crate::intent_ir::{IntentIrDocument, emit_intent_ir};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const SOURCE_SCHEMA_VERSION: &str = "axiom.agent_task.spec.v0";
pub const TASK_SCHEMA_VERSION: &str = "axiom.agent_task.v0";

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentTaskSource {
    pub schema_version: String,
    pub task: AgentTaskSpec,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentTaskSpec {
    pub id: String,
    pub kind: TaskKind,
    pub objective: String,
    pub authority: Authority,
    pub scope: Scope,
    pub capabilities: PermissionSet,
    pub commands: PermissionSet,
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    pub required_evidence: Vec<EvidenceRequirement>,
    pub dependencies: Vec<TaskDependency>,
    pub autonomy: Autonomy,
    pub budgets: Budgets,
    pub rollback: RollbackPlan,
    pub terminal_conditions: TerminalConditions,
    pub delivery_permissions: DeliveryPermissions,
    #[serde(default)]
    #[serde(rename = "repair")]
    pub repair_plan: Option<RepairPlan>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Feature,
    Migration,
    Refactor,
    Operational,
    Repair,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Authority {
    pub governing_issue: String,
    pub repository: String,
    pub issue: u64,
    pub source_revision: String,
    pub source_digest: String,
    pub approval: Approval,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityStatus {
    Approved,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub state: AuthorityStatus,
    pub approver: String,
    pub method: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub affected_semantic_nodes: Vec<String>,
    pub allowed_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub denied_files: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PermissionSet {
    pub required: Vec<String>,
    pub forbidden: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceCriterion {
    pub id: String,
    pub requirement: String,
    pub expected: AcceptanceExpectation,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceExpectation {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRequirement {
    pub id: String,
    pub kind: EvidenceKind,
    pub command: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    UnitTest,
    PropertyTest,
    ConformanceFixture,
    CapabilityDenialTest,
    GoldenOutput,
    SchemaValidation,
    SecurityFixture,
    BenchmarkBaseline,
    ManualReview,
    RiskNote,
    CiStatus,
    ReviewState,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskDependency {
    pub id: String,
    pub status: DependencyStatus,
    pub depends_on: Vec<String>,
    pub precondition: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DependencyStatus {
    Satisfied,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Autonomy {
    pub class: u8,
    pub risk: RiskClass,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskClass {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Budgets {
    pub time_seconds: u64,
    pub tokens: u64,
    pub retries: u32,
    pub cost_usd_micros: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RollbackPlan {
    pub checkpoint: String,
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TerminalConditions {
    pub success: Vec<String>,
    pub stop: Vec<String>,
    pub escalation: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeliveryPermissions {
    pub commit: bool,
    pub push: bool,
    pub pull_request: bool,
    pub review: bool,
    pub merge: bool,
    pub deploy: bool,
    pub approve_own_pull_request: bool,
    pub force_push: bool,
    pub direct_push_protected: bool,
    pub irreversible_actions: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RepairPlan {
    pub id: String,
    pub reason: String,
    pub target_node: String,
    pub allowed_files: Vec<String>,
    pub required_evidence: Vec<EvidenceKind>,
    pub diagnostics: Vec<RepairDiagnostic>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RepairDiagnostic {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repair: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AgentTaskContract {
    pub schema_version: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub contract_digest: String,
    pub id: String,
    #[serde(rename = "kind")]
    pub task_kind: TaskKind,
    pub objective: String,
    pub authority: Authority,
    pub scope: Scope,
    pub capabilities: PermissionSet,
    pub commands: PermissionSet,
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    pub required_evidence: Vec<EvidenceRequirement>,
    pub dependencies: Vec<TaskDependency>,
    pub autonomy: Autonomy,
    pub budgets: Budgets,
    pub rollback: RollbackPlan,
    pub terminal_conditions: TerminalConditions,
    pub delivery_permissions: DeliveryPermissions,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "repair")]
    pub repair_plan: Option<RepairPlan>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AgentTaskReport {
    pub schema_version: String,
    pub ok: bool,
    pub command: String,
    pub project: String,
    pub contract: AgentTaskContract,
}

pub fn compile_task_contract(
    project: &Path,
    source_path: &Path,
) -> Result<AgentTaskReport, Diagnostic> {
    let content = fs::read_to_string(source_path).map_err(|error| {
        task_error(
            "agent_task_source_read",
            format!("failed to read {}: {error}", source_path.display()),
        )
    })?;
    let source: AgentTaskSource = serde_json::from_str(&content).map_err(|error| {
        task_error(
            "agent_task_source_invalid",
            format!("invalid task source {}: {error}", source_path.display()),
        )
    })?;
    require(
        source.schema_version == SOURCE_SCHEMA_VERSION,
        "agent_task_schema",
        "unsupported task source schema",
    )?;
    validate_project_paths(project, &source.task)?;
    let intent = emit_intent_ir(project)?;
    let contract = compile_approved_task(source.task, &intent)?;
    Ok(AgentTaskReport {
        schema_version: TASK_SCHEMA_VERSION.into(),
        ok: true,
        command: "task-contract".into(),
        project: ".".into(),
        contract,
    })
}

pub fn compile_approved_task(
    mut source: AgentTaskSpec,
    intent: &IntentIrDocument,
) -> Result<AgentTaskContract, Diagnostic> {
    validate(&source, intent)?;
    canonicalize(&mut source);
    let mut contract = AgentTaskContract {
        schema_version: TASK_SCHEMA_VERSION.into(),
        contract_digest: String::new(),
        id: source.id,
        task_kind: source.kind,
        objective: source.objective,
        authority: source.authority,
        scope: source.scope,
        capabilities: source.capabilities,
        commands: source.commands,
        acceptance_criteria: source.acceptance_criteria,
        required_evidence: source.required_evidence,
        dependencies: source.dependencies,
        autonomy: source.autonomy,
        budgets: source.budgets,
        rollback: source.rollback,
        terminal_conditions: source.terminal_conditions,
        delivery_permissions: source.delivery_permissions,
        repair_plan: source.repair_plan,
    };
    let canonical = serde_json::to_vec(&contract)
        .map_err(|error| task_error("agent_task_digest", error.to_string()))?;
    contract.contract_digest = format!("sha256:{}", stable_digest(&canonical));
    Ok(contract)
}

fn validate(source: &AgentTaskSpec, intent: &IntentIrDocument) -> Result<(), Diagnostic> {
    nonempty(&source.id, "task ID")?;
    require(
        source.id.chars().enumerate().all(|(i, c)| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || (i > 0 && matches!(c, '.' | '_' | '-'))
        }),
        "agent_task_id",
        "task ID must be canonical lowercase ASCII",
    )?;
    nonempty(&source.objective, "objective")?;
    nonempty(&source.authority.governing_issue, "governing issue")?;
    nonempty(&source.authority.repository, "authority repository")?;
    require(
        source.authority.issue > 0,
        "agent_task_authority",
        "authority issue must be positive",
    )?;
    nonempty(
        &source.authority.source_revision,
        "authority source revision",
    )?;
    nonempty(&source.authority.source_digest, "authority source digest")?;
    nonempty(&source.authority.approval.approver, "approver")?;
    nonempty(&source.authority.approval.method, "approval method")?;
    let expected_issue = format!(
        "https://github.com/{}/issues/{}",
        source.authority.repository, source.authority.issue
    );
    require(
        source.authority.governing_issue == expected_issue,
        "agent_task_authority",
        "governing issue, repository, and issue number do not agree",
    )?;
    require(
        source
            .authority
            .repository
            .split_once('/')
            .is_some_and(|(owner, repo)| {
                !owner.is_empty() && !repo.is_empty() && !repo.contains('/')
            }),
        "agent_task_authority",
        "repository must be owner/name",
    )?;
    require(
        source.authority.source_revision.len() == 40
            && source
                .authority
                .source_revision
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "agent_task_authority",
        "source revision must be a full lowercase commit SHA",
    )?;
    require(
        source
            .authority
            .source_digest
            .strip_prefix("sha256:")
            .is_some_and(|digest| {
                digest.len() == 64
                    && digest
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            }),
        "agent_task_authority",
        "source digest must be lowercase SHA-256",
    )?;
    require(
        matches!(
            source.authority.approval.method.as_str(),
            "issue_label" | "issue_comment" | "signed_spec" | "maintainer_review"
        ),
        "agent_task_authority",
        "unsupported approval method",
    )?;
    require(
        !source.scope.affected_semantic_nodes.is_empty(),
        "agent_task_scope",
        "semantic node scope is required",
    )?;
    require(
        !source.scope.allowed_files.is_empty(),
        "agent_task_scope",
        "allowed file scope is required",
    )?;
    for node in &source.scope.affected_semantic_nodes {
        nonempty(node, "semantic node")?;
        require(
            intent.contains_node(node),
            "agent_task_scope",
            &format!("semantic node `{node}` is not in canonical Intent IR"),
        )?;
    }
    validate_paths(&source.scope.allowed_files)?;
    validate_paths(&source.scope.denied_files)?;
    disjoint(
        &source.scope.allowed_files,
        &source.scope.denied_files,
        "file scope",
    )?;
    validate_permissions(&source.capabilities, "capability")?;
    validate_permissions(&source.commands, "command")?;
    validate_support_commands(source)?;
    validate_acceptance(source)?;
    validate_dependencies(&source.dependencies)?;
    require(
        (1..=3).contains(&source.autonomy.class),
        "agent_task_autonomy",
        "autonomy class must be between 1 and 3",
    )?;
    require(
        source.budgets.time_seconds > 0 && source.budgets.tokens > 0,
        "agent_task_budget",
        "time and token budgets must be positive",
    )?;
    require(
        source.budgets.time_seconds <= 86_400
            && source.budgets.tokens <= 10_000_000
            && source.budgets.retries <= 20
            && source.budgets.cost_usd_micros <= 1_000_000_000,
        "agent_task_budget",
        "budget exceeds the supported bound",
    )?;
    nonempty(&source.rollback.checkpoint, "rollback checkpoint")?;
    nonempty_list(&source.rollback.commands, "rollback commands")?;
    nonempty_list(&source.terminal_conditions.success, "success conditions")?;
    nonempty_list(&source.terminal_conditions.stop, "stop conditions")?;
    nonempty_list(
        &source.terminal_conditions.escalation,
        "escalation conditions",
    )?;
    let delivery = &source.delivery_permissions;
    require(
        !delivery.approve_own_pull_request,
        "agent_task_delivery",
        "self-approval is always denied",
    )?;
    require(
        !delivery.force_push,
        "agent_task_delivery",
        "force push is always denied",
    )?;
    require(
        !delivery.direct_push_protected,
        "agent_task_delivery",
        "direct protected-branch push is always denied",
    )?;
    require(
        !delivery.irreversible_actions,
        "agent_task_irreversible",
        "irreversible actions are unsupported",
    )?;
    let remote_or_privileged = delivery.push
        || delivery.pull_request
        || delivery.review
        || delivery.merge
        || delivery.deploy;
    require(
        source.autonomy.class != 0 || (!delivery.commit && !remote_or_privileged),
        "agent_task_delivery",
        "class 0 tasks cannot mutate or deliver",
    )?;
    require(
        source.autonomy.class != 1 || !remote_or_privileged,
        "agent_task_delivery",
        "class 1 tasks are local-only",
    )?;
    require(
        source.autonomy.class != 2 || (!delivery.review && !delivery.merge && !delivery.deploy),
        "agent_task_delivery",
        "class 2 tasks cannot review, merge, or deploy",
    )?;
    match (&source.kind, &source.repair_plan) {
        (TaskKind::Repair, Some(repair)) => validate_repair(repair, source)?,
        (TaskKind::Repair, None) => {
            return Err(task_error(
                "agent_task_repair",
                "repair task must embed its repair plan",
            ));
        }
        (_, Some(_)) => {
            return Err(task_error(
                "agent_task_repair",
                "only repair tasks may embed a repair plan",
            ));
        }
        (_, None) => {}
    }
    Ok(())
}

fn validate_acceptance(source: &AgentTaskSpec) -> Result<(), Diagnostic> {
    require(
        !source.acceptance_criteria.is_empty(),
        "agent_task_acceptance",
        "acceptance criteria are required",
    )?;
    require(
        !source.required_evidence.is_empty(),
        "agent_task_evidence",
        "required evidence is missing",
    )?;
    let evidence: BTreeSet<_> = source
        .required_evidence
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    require(
        evidence.len() == source.required_evidence.len(),
        "agent_task_evidence",
        "evidence IDs must be unique",
    )?;
    let mut expectations = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for criterion in &source.acceptance_criteria {
        nonempty(&criterion.id, "acceptance ID")?;
        nonempty(&criterion.requirement, "acceptance requirement")?;
        require(
            ids.insert(criterion.id.as_str()),
            "agent_task_acceptance",
            "acceptance IDs must be unique",
        )?;
        if expectations
            .insert(criterion.requirement.trim(), criterion.expected)
            .is_some_and(|old| old != criterion.expected)
        {
            return Err(task_error(
                "agent_task_acceptance_conflict",
                format!(
                    "conflicting acceptance criterion `{}`",
                    criterion.requirement
                ),
            ));
        }
        require(
            !criterion.evidence_refs.is_empty(),
            "agent_task_evidence",
            "each acceptance criterion requires evidence",
        )?;
        for reference in &criterion.evidence_refs {
            require(
                evidence.contains(reference.as_str()),
                "agent_task_evidence",
                &format!("dangling evidence reference `{reference}`"),
            )?;
        }
    }
    for item in &source.required_evidence {
        nonempty(&item.id, "evidence ID")?;
        nonempty(&item.command, "evidence command")?;
    }
    Ok(())
}

fn validate_dependencies(items: &[TaskDependency]) -> Result<(), Diagnostic> {
    let ids: BTreeSet<_> = items.iter().map(|item| item.id.as_str()).collect();
    require(
        ids.len() == items.len(),
        "agent_task_dependency",
        "dependency IDs must be unique",
    )?;
    for item in items {
        nonempty(&item.id, "dependency ID")?;
        if !item.precondition.is_empty() {
            nonempty(&item.precondition, "dependency precondition")?;
        }
        for dep in &item.depends_on {
            require(
                dep != &item.id && ids.contains(dep.as_str()),
                "agent_task_dependency",
                &format!("dangling or self dependency `{dep}`"),
            )?;
        }
    }
    fn visit<'a>(
        id: &'a str,
        map: &BTreeMap<&'a str, &'a TaskDependency>,
        visiting: &mut BTreeSet<&'a str>,
        done: &mut BTreeSet<&'a str>,
    ) -> bool {
        if done.contains(id) {
            return false;
        }
        if !visiting.insert(id) {
            return true;
        }
        let cycle = map[id]
            .depends_on
            .iter()
            .any(|next| visit(next, map, visiting, done));
        visiting.remove(id);
        done.insert(id);
        cycle
    }
    let map: BTreeMap<_, _> = items.iter().map(|item| (item.id.as_str(), item)).collect();
    let mut visiting = BTreeSet::new();
    let mut done = BTreeSet::new();
    for id in ids {
        require(
            !visit(id, &map, &mut visiting, &mut done),
            "agent_task_dependency_cycle",
            "dependency graph contains a cycle",
        )?;
    }
    Ok(())
}

fn validate_repair(repair: &RepairPlan, source: &AgentTaskSpec) -> Result<(), Diagnostic> {
    require(
        matches!(source.kind, TaskKind::Repair),
        "agent_task_repair",
        "embedded repair plan requires repair task kind",
    )?;
    nonempty(&repair.id, "repair ID")?;
    nonempty(&repair.reason, "repair reason")?;
    require(
        source
            .scope
            .affected_semantic_nodes
            .contains(&repair.target_node),
        "agent_task_repair_widening",
        "repair target widens semantic node scope",
    )?;
    nonempty_list(&repair.allowed_files, "repair files")?;
    require(
        !repair.required_evidence.is_empty(),
        "agent_task_repair",
        "repair evidence is required",
    )?;
    require(
        !repair.diagnostics.is_empty(),
        "agent_task_repair",
        "repair diagnostics are required",
    )?;
    validate_paths(&repair.allowed_files)?;
    let files: BTreeSet<_> = source.scope.allowed_files.iter().collect();
    let evidence: BTreeSet<_> = source
        .required_evidence
        .iter()
        .map(|item| item.kind)
        .collect();
    require(
        repair.allowed_files.iter().all(|path| files.contains(path)),
        "agent_task_repair_widening",
        "repair plan widens allowed files",
    )?;
    require(
        repair
            .required_evidence
            .iter()
            .all(|kind| evidence.contains(kind)),
        "agent_task_repair_widening",
        "repair plan widens required evidence",
    )
}

fn validate_paths(paths: &[String]) -> Result<(), Diagnostic> {
    for path in paths {
        nonempty(path, "file path")?;
        let value = Path::new(path);
        require(
            !value.is_absolute()
                && !path.contains('*')
                && !path.contains('?')
                && !path.ends_with('/'),
            "agent_task_path",
            &format!("path `{path}` must be an exact package-relative file"),
        )?;
        require(
            value
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
            "agent_task_path",
            &format!("path `{path}` is not normalized"),
        )?;
    }
    Ok(())
}

fn validate_permissions(set: &PermissionSet, label: &str) -> Result<(), Diagnostic> {
    for value in &set.required {
        nonempty(value, label)?;
    }
    for value in &set.forbidden {
        nonempty(value, label)?;
    }
    disjoint(&set.required, &set.forbidden, label)
}

fn validate_support_commands(source: &AgentTaskSpec) -> Result<(), Diagnostic> {
    for (kind, command) in source
        .required_evidence
        .iter()
        .map(|item| ("evidence", item.command.as_str()))
        .chain(
            source
                .rollback
                .commands
                .iter()
                .map(|command| ("rollback", command.as_str())),
        )
    {
        require(
            !source
                .commands
                .forbidden
                .iter()
                .any(|denied| command_invokes(command, denied)),
            "agent_task_forbidden_command",
            &format!("{kind} command `{command}` invokes a forbidden command"),
        )?;
        require(
            !source
                .capabilities
                .forbidden
                .iter()
                .any(|denied| command_mentions_capability(command, denied)),
            "agent_task_forbidden_capability",
            &format!("{kind} command `{command}` requests a forbidden capability"),
        )?;
    }
    Ok(())
}

fn command_invokes(command: &str, denied: &str) -> bool {
    let command = command.trim();
    let denied = denied.trim();
    command == denied
        || command
            .strip_prefix(denied)
            .is_some_and(|rest| rest.chars().next().is_some_and(char::is_whitespace))
}

fn command_mentions_capability(command: &str, denied: &str) -> bool {
    command
        .split(|c: char| c.is_whitespace() || matches!(c, '=' | ',' | ':' | '"' | '\''))
        .any(|token| token == denied)
}

fn validate_project_paths(project: &Path, source: &AgentTaskSpec) -> Result<(), Diagnostic> {
    let root = project.canonicalize().map_err(|error| {
        task_error(
            "agent_task_project_path",
            format!(
                "failed to canonicalize project {}: {error}",
                project.display()
            ),
        )
    })?;
    require(
        root.is_dir(),
        "agent_task_project_path",
        "task project root must be a directory",
    )?;
    for path in source.scope.allowed_files.iter().chain(
        source
            .repair_plan
            .iter()
            .flat_map(|repair| &repair.allowed_files),
    ) {
        validate_path_boundary(&root, path)?;
    }
    Ok(())
}

fn validate_path_boundary(root: &Path, relative: &str) -> Result<(), Diagnostic> {
    let mut candidate = root.join(relative);
    let mut unresolved = Vec::new();
    while !candidate.exists() {
        let name = candidate.file_name().ok_or_else(|| {
            task_error(
                "agent_task_path",
                format!("path `{relative}` has no existing parent"),
            )
        })?;
        unresolved.push(name.to_owned());
        candidate = candidate.parent().map(Path::to_owned).ok_or_else(|| {
            task_error(
                "agent_task_path",
                format!("path `{relative}` has no existing parent"),
            )
        })?;
    }
    let resolved = candidate.canonicalize().map_err(|error| {
        task_error(
            "agent_task_path",
            format!("failed to resolve scope path `{relative}`: {error}"),
        )
    })?;
    require(
        resolved.starts_with(root),
        "agent_task_path_escape",
        &format!("scope path `{relative}` escapes the canonical project root"),
    )?;
    // Reconstructing the unresolved suffix documents that validation is against
    // the nearest existing parent, including a symlink at any existing prefix.
    let _bounded: PathBuf = unresolved
        .iter()
        .rev()
        .fold(resolved, |path, component| path.join(component));
    Ok(())
}

fn disjoint(left: &[String], right: &[String], label: &str) -> Result<(), Diagnostic> {
    let denied: BTreeSet<_> = right.iter().collect();
    require(
        !left.iter().any(|item| denied.contains(item)),
        "agent_task_permission_conflict",
        &format!("{label} allow and deny sets overlap"),
    )
}

fn canonicalize(source: &mut AgentTaskSpec) {
    fn sort(values: &mut Vec<String>) {
        values.sort();
        values.dedup();
    }
    sort(&mut source.scope.affected_semantic_nodes);
    sort(&mut source.scope.allowed_files);
    sort(&mut source.scope.denied_files);
    sort(&mut source.capabilities.required);
    sort(&mut source.capabilities.forbidden);
    sort(&mut source.commands.required);
    sort(&mut source.commands.forbidden);
    source.acceptance_criteria.sort_by(|a, b| a.id.cmp(&b.id));
    for item in &mut source.acceptance_criteria {
        sort(&mut item.evidence_refs);
    }
    source.required_evidence.sort_by(|a, b| a.id.cmp(&b.id));
    source.dependencies.sort_by(|a, b| a.id.cmp(&b.id));
    for item in &mut source.dependencies {
        sort(&mut item.depends_on);
    }
    sort(&mut source.rollback.commands);
    sort(&mut source.terminal_conditions.success);
    sort(&mut source.terminal_conditions.stop);
    sort(&mut source.terminal_conditions.escalation);
    if let Some(repair) = &mut source.repair_plan {
        sort(&mut repair.allowed_files);
        repair.required_evidence.sort();
        repair.required_evidence.dedup();
    }
}

fn nonempty(value: &str, label: &str) -> Result<(), Diagnostic> {
    require(
        !value.trim().is_empty(),
        "agent_task_missing",
        &format!("{label} must not be empty"),
    )
}
fn nonempty_list(values: &[String], label: &str) -> Result<(), Diagnostic> {
    require(
        !values.is_empty(),
        "agent_task_missing",
        &format!("{label} are required"),
    )?;
    for value in values {
        nonempty(value, label)?;
    }
    Ok(())
}
fn require(condition: bool, code: &str, message: &str) -> Result<(), Diagnostic> {
    if condition {
        Ok(())
    } else {
        Err(task_error(code, message))
    }
}
fn task_error(code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("agent_task", message).with_code(code)
}

fn stable_digest(bytes: &[u8]) -> String {
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
    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    let mut message = bytes.to_vec();
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
            w[index] = u32::from_be_bytes(word.try_into().expect("four-byte SHA word"));
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (state, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *state = state.wrapping_add(value);
        }
    }
    h.iter().map(|word| format!("{word:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_ir::{IntentIrNode, IntentIrProvenance};
    use serde_json::Map;

    fn intent() -> IntentIrDocument {
        IntentIrDocument {
            schema_version: "axiom.intent_ir.v0".into(),
            graph_id: "g".into(),
            package: "pkg".into(),
            provenance: IntentIrProvenance::default(),
            diagnostics: vec![],
            nodes: vec![IntentIrNode {
                id: "pkg::main".into(),
                kind: "Module".into(),
                name: "main".into(),
                description: None,
                source_span: None,
                metadata: Map::new(),
            }],
            edges: vec![],
        }
    }
    fn source() -> AgentTaskSpec {
        serde_json::from_value(serde_json::json!({
        "id":"task-1419", "kind":"feature", "objective": "Implement feature",
        "authority": {"governing_issue":"https://github.com/OMT-Global/axiomlang/issues/1419","repository":"OMT-Global/axiomlang","issue":1419,"source_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","source_digest":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","approval":{"state":"approved","approver":"Pheidon","method":"maintainer_review"}},
        "scope":{"affected_semantic_nodes":["pkg::main"],"allowed_files":["src/main.ax"],"denied_files":["secrets.env"]},
        "capabilities":{"required":["filesystem.write"],"forbidden":["credentials.read"]},
        "commands":{"required":["axiomc check"],"forbidden":["git push --force"]},
        "acceptance_criteria":[{"id":"a","requirement":"checks pass","expected":"pass","evidence_refs":["e"]}],
        "required_evidence":[{"id":"e","kind":"unit_test","command":"axiomc check"}],
        "dependencies":[{"id":"spec","status":"satisfied","depends_on":[],"precondition":"accepted"}],
        "autonomy":{"class":2,"risk":"medium"}, "budgets":{"time_seconds":3600,"tokens":10000,"retries":3,"cost_usd_micros":1000},
        "rollback":{"checkpoint":"before edit","commands":["git revert HEAD"]},
        "terminal_conditions":{"success":["criteria pass"],"stop":["budget exhausted"],"escalation":["scope change"]},
        "delivery_permissions":{"commit":true,"push":true,"pull_request":true,"review":false,"merge":false,"deploy":false,"approve_own_pull_request":false,"force_push":false,"direct_push_protected":false,"irreversible_actions":false}
    })).unwrap()
    }

    #[test]
    fn compiles_deterministically() {
        let a = compile_approved_task(source(), &intent()).unwrap();
        let b = compile_approved_task(source(), &intent()).unwrap();
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
        assert_eq!(a.id, "task-1419");
    }
    #[test]
    fn contract_digest_uses_sha256() {
        assert_eq!(
            stable_digest(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(
            compile_approved_task(source(), &intent())
                .unwrap()
                .contract_digest
                .starts_with("sha256:")
        );
    }
    #[test]
    fn rejects_ambiguous_authority_and_unknown_nodes() {
        let mut s = source();
        s.authority.approval.approver.clear();
        assert_eq!(
            compile_approved_task(s, &intent())
                .unwrap_err()
                .code
                .as_deref(),
            Some("agent_task_missing")
        );
        let mut s = source();
        s.scope.affected_semantic_nodes[0] = "missing".into();
        assert_eq!(
            compile_approved_task(s, &intent())
                .unwrap_err()
                .code
                .as_deref(),
            Some("agent_task_scope")
        );
    }
    #[test]
    fn rejects_scope_and_acceptance_conflicts() {
        let mut s = source();
        s.scope.denied_files.push("src/main.ax".into());
        assert_eq!(
            compile_approved_task(s, &intent())
                .unwrap_err()
                .code
                .as_deref(),
            Some("agent_task_permission_conflict")
        );
        let mut s = source();
        let mut other = s.acceptance_criteria[0].clone();
        other.id = "b".into();
        other.expected = AcceptanceExpectation::Fail;
        s.acceptance_criteria.push(other);
        assert_eq!(
            compile_approved_task(s, &intent())
                .unwrap_err()
                .code
                .as_deref(),
            Some("agent_task_acceptance_conflict")
        );
    }
    #[test]
    fn rejects_dangling_and_cyclic_dependencies() {
        let mut s = source();
        s.dependencies[0].depends_on.push("missing".into());
        assert_eq!(
            compile_approved_task(s, &intent())
                .unwrap_err()
                .code
                .as_deref(),
            Some("agent_task_dependency")
        );
        let mut s = source();
        s.dependencies.push(TaskDependency {
            id: "other".into(),
            status: DependencyStatus::Satisfied,
            depends_on: vec!["spec".into()],
            precondition: "ready".into(),
        });
        s.dependencies[0].depends_on.push("other".into());
        assert_eq!(
            compile_approved_task(s, &intent())
                .unwrap_err()
                .code
                .as_deref(),
            Some("agent_task_dependency_cycle")
        );
    }
    #[test]
    fn denies_unsafe_delivery_and_irreversible_work() {
        let mut s = source();
        s.delivery_permissions.force_push = true;
        assert_eq!(
            compile_approved_task(s, &intent())
                .unwrap_err()
                .code
                .as_deref(),
            Some("agent_task_delivery")
        );
        s = source();
        s.delivery_permissions.irreversible_actions = true;
        assert_eq!(
            compile_approved_task(s, &intent())
                .unwrap_err()
                .code
                .as_deref(),
            Some("agent_task_irreversible")
        );
    }
    #[test]
    fn repair_plan_cannot_widen_task() {
        let mut s = source();
        s.kind = TaskKind::Repair;
        s.repair_plan = Some(RepairPlan {
            id: "repair-1".into(),
            reason: "diagnostic".into(),
            target_node: "pkg::main".into(),
            allowed_files: vec!["other.ax".into()],
            required_evidence: vec![EvidenceKind::UnitTest],
            diagnostics: vec![RepairDiagnostic {
                kind: "type".into(), code: None, message: "bad".into(), help: None,
                path: None, line: None, column: None, related: vec![], repair: None,
            }],
        });
        assert_eq!(
            compile_approved_task(s, &intent())
                .unwrap_err()
                .code
                .as_deref(),
            Some("agent_task_repair_widening")
        );
    }

    #[test]
    fn compiles_public_feature_and_repair_fixtures() {
        let stage1 = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let project = stage1.join("examples/agent_native_authorize");
        for fixture in ["feature-approved.spec.json", "repair-approved.spec.json"] {
            let report = compile_task_contract(
                &project,
                &stage1.join("json-fixtures/task-contract").join(fixture),
            )
            .unwrap_or_else(|error| panic!("compile {fixture}: {error:?}"));
            assert!(report.ok);
            assert_eq!(report.project, ".");
            assert!(report.contract.contract_digest.starts_with("sha256:"));
        }
    }

    #[test]
    fn evidence_and_rollback_cannot_bypass_denials() {
        let mut task = source();
        task.commands.forbidden.push("curl".into());
        task.required_evidence[0].command = "curl https://example.invalid".into();
        assert_eq!(
            compile_approved_task(task, &intent())
                .unwrap_err()
                .code
                .as_deref(),
            Some("agent_task_forbidden_command")
        );

        let mut task = source();
        task.commands.forbidden.push("git reset".into());
        task.rollback.commands = vec!["git reset --hard HEAD".into()];
        assert_eq!(
            compile_approved_task(task, &intent())
                .unwrap_err()
                .code
                .as_deref(),
            Some("agent_task_forbidden_command")
        );

        let mut task = source();
        task.capabilities.forbidden.push("credentials.read".into());
        task.required_evidence[0].command = "axiomc check --capability credentials.read".into();
        assert_eq!(
            compile_approved_task(task, &intent())
                .unwrap_err()
                .code
                .as_deref(),
            Some("agent_task_forbidden_capability")
        );
    }

    #[test]
    fn serde_requirements_match_schema_and_preserve_diagnostics() {
        let mut value = serde_json::to_value(AgentTaskSource {
            schema_version: SOURCE_SCHEMA_VERSION.into(),
            task: source(),
        })
        .unwrap();
        value["task"]["commands"]
            .as_object_mut()
            .unwrap()
            .remove("forbidden");
        assert!(serde_json::from_value::<AgentTaskSource>(value).is_err());

        let mut value = serde_json::to_value(AgentTaskSource {
            schema_version: SOURCE_SCHEMA_VERSION.into(),
            task: source(),
        })
        .unwrap();
        value["task"]["dependencies"][0]
            .as_object_mut()
            .unwrap()
            .remove("precondition");
        value["task"]["repair"] = serde_json::json!({
            "id":"repair-1", "reason":"legacy", "target_node":"pkg::main",
            "allowed_files":["src/main.ax"], "required_evidence":["unit_test"],
            "diagnostics":["opaque legacy repair diagnostic"]
        });
        value["task"]["kind"] = "repair".into();
        assert!(serde_json::from_value::<AgentTaskSource>(value).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn project_paths_reject_existing_and_parent_symlink_escapes() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/main.ax"), "fn main() {}\n").unwrap();
        fs::write(outside.path().join("escaped.ax"), "fn escaped() {}\n").unwrap();

        let mut task = source();
        validate_project_paths(root.path(), &task).unwrap();

        symlink(
            outside.path().join("escaped.ax"),
            root.path().join("src/link.ax"),
        )
        .unwrap();
        task.scope.allowed_files = vec!["src/link.ax".into()];
        assert_eq!(
            validate_project_paths(root.path(), &task)
                .unwrap_err()
                .code
                .as_deref(),
            Some("agent_task_path_escape")
        );

        symlink(outside.path(), root.path().join("external")).unwrap();
        task.scope.allowed_files = vec!["external/new/nested.ax".into()];
        assert_eq!(
            validate_project_paths(root.path(), &task)
                .unwrap_err()
                .code
                .as_deref(),
            Some("agent_task_path_escape")
        );
    }
}
