//! Deterministic, fail-closed evidence planning from canonical Intent IR changes.

use crate::intent_ir::{IntentIrDocument, IntentIrEdge, IntentIrNode};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

pub const PLAN_SCHEMA_VERSION: &str = "axiom.verification_plan.v0";
pub const RESULTS_SCHEMA_VERSION: &str = "axiom.verification_results.v0";
pub const VERDICT_SCHEMA_VERSION: &str = "axiom.verification_verdict.v0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SemanticDiffContract {
    pub schema_version: String,
    pub ok: bool,
    pub command: String,
    pub old: String,
    pub new: String,
    pub summary: SemanticDiffSummary,
    pub changes: Vec<SemanticDiffChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SemanticDiffSummary {
    pub breaking: usize,
    pub additive: usize,
    pub informational: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct SemanticDiffChange {
    pub change: String,
    pub severity: String,
    pub node_kind: String,
    pub node_id: String,
    pub edge_kind: Option<String>,
    pub edge_from: Option<String>,
    pub edge_to: Option<String>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VerificationPlan {
    pub schema_version: String,
    pub plan_digest: String,
    pub bindings: PlanBindings,
    pub changes: Vec<SemanticChange>,
    pub requirements: Vec<EvidenceRequirement>,
    pub coverage: Coverage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanBindings {
    pub before_graph_id: String,
    pub after_graph_id: String,
    pub before_snapshot_digest: String,
    pub after_snapshot_digest: String,
    pub source_head_sha: String,
    pub delivered_head_sha: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct SemanticChange {
    pub id: String,
    pub change_kind: ChangeKind,
    pub node_kind: String,
    pub semantic_id: String,
    pub source_path: Option<String>,
    pub impact: Vec<EvidenceKind>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Removed,
    Modified,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Positive,
    Denial,
    Regression,
    Schema,
    Artifact,
    Security,
    Performance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRequirement {
    pub id: String,
    pub kind: EvidenceKind,
    pub reason: String,
    pub semantic_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    pub confidence: CoverageConfidence,
    pub complete: bool,
    pub explanation: String,
    pub unknown_impacts: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoverageConfidence {
    Complete,
    Conservative,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VerificationResults {
    pub schema_version: String,
    pub plan_digest: String,
    pub source_head_sha: String,
    pub delivered_head_sha: String,
    pub results: Vec<EvidenceResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EvidenceResult {
    pub id: String,
    pub plan_digest: String,
    pub source_head_sha: String,
    pub delivered_head_sha: String,
    pub status: EvidenceStatus,
    pub evidence_digest: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VerificationVerdict {
    pub schema_version: String,
    pub plan_digest: String,
    pub status: VerdictStatus,
    pub source_head_sha: String,
    pub delivered_head_sha: String,
    pub missing: Vec<String>,
    pub duplicate: Vec<String>,
    pub invalid: Vec<String>,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerdictStatus {
    Passed,
    Failed,
}

/// Computes a stable verification plan. Unknown semantic surfaces deliberately
/// request every evidence family rather than guessing a narrower suite.
pub fn plan_verification(
    before: &IntentIrDocument,
    after: &IntentIrDocument,
    source_head_sha: &str,
    delivered_head_sha: &str,
) -> Result<VerificationPlan, String> {
    validate_document(before, "before")?;
    validate_document(after, "after")?;
    validate_sha(source_head_sha)?;
    validate_sha(delivered_head_sha)?;

    let mut unknown = BTreeSet::new();
    let mut changes = node_changes(before, after, &mut unknown);
    changes.extend(edge_changes(before, after, &mut unknown));
    let source_changes = source_changes(before, after, &mut unknown);
    if source_changes.is_empty()
        && before.provenance.source_digest != after.provenance.source_digest
    {
        unknown.insert("provenance:unmapped_source_digest".into());
        changes.push(SemanticChange {
            id: "source:modified:unmapped-provenance".into(),
            change_kind: ChangeKind::Modified,
            node_kind: "SourceInput".into(),
            semantic_id: "source:unmapped-provenance".into(),
            source_path: None,
            impact: all_evidence(),
        });
    }
    changes.extend(source_changes);
    if !before.diagnostics.is_empty() || !after.diagnostics.is_empty() {
        unknown.insert("intent_ir:incomplete_diagnostics".into());
        changes.push(SemanticChange {
            id: "diagnostic:incomplete-intent-ir".into(),
            change_kind: ChangeKind::Modified,
            node_kind: "IntentDiagnostic".into(),
            semantic_id: after.package.clone(),
            source_path: None,
            impact: all_evidence(),
        });
    }
    if before.graph_id != after.graph_id
        || before.package != after.package
        || before.provenance.producer != after.provenance.producer
        || before.provenance.path_policy != after.provenance.path_policy
    {
        unknown.insert("intent_ir:top_level_contract_change".into());
        changes.push(SemanticChange {
            id: "snapshot:modified:top-level-contract".into(),
            change_kind: ChangeKind::Modified,
            node_kind: "IntentSnapshot".into(),
            semantic_id: after.package.clone(),
            source_path: None,
            impact: all_evidence(),
        });
    }
    if changes.is_empty() && before != after {
        unknown.insert("intent_ir:unmapped_snapshot_change".into());
        changes.push(SemanticChange {
            id: "snapshot:modified:unmapped".into(),
            change_kind: ChangeKind::Modified,
            node_kind: "IntentSnapshot".into(),
            semantic_id: after.package.clone(),
            source_path: None,
            impact: all_evidence(),
        });
    }
    changes.sort();
    changes.dedup();

    let mut by_kind: BTreeMap<EvidenceKind, BTreeSet<String>> = BTreeMap::new();
    for change in &changes {
        for kind in &change.impact {
            by_kind
                .entry(*kind)
                .or_default()
                .insert(change.semantic_id.clone());
        }
    }
    let requirements = by_kind
        .into_iter()
        .map(|(kind, semantic_ids)| EvidenceRequirement {
            id: format!("evidence-{}", kind_name(kind)),
            kind,
            reason: requirement_reason(kind).into(),
            semantic_ids: semantic_ids.into_iter().collect(),
        })
        .collect();
    let unknown_impacts: Vec<_> = unknown.into_iter().collect();
    let conservative = !unknown_impacts.is_empty();
    let mut plan = VerificationPlan {
        schema_version: PLAN_SCHEMA_VERSION.into(),
        plan_digest: String::new(),
        bindings: PlanBindings {
            before_graph_id: before.graph_id.clone(),
            after_graph_id: after.graph_id.clone(),
            before_snapshot_digest: snapshot_digest(before)?,
            after_snapshot_digest: snapshot_digest(after)?,
            source_head_sha: source_head_sha.into(),
            delivered_head_sha: delivered_head_sha.into(),
        },
        changes,
        requirements,
        coverage: Coverage {
            confidence: if conservative {
                CoverageConfidence::Conservative
            } else {
                CoverageConfidence::Complete
            },
            complete: !conservative,
            explanation: if conservative {
                "Unknown semantic impact broadened the plan to every evidence family.".into()
            } else {
                "Every changed semantic surface mapped to an explicit evidence family.".into()
            },
            unknown_impacts,
        },
    };
    let bytes = serde_json::to_vec(&plan).map_err(|error| error.to_string())?;
    plan.plan_digest = sha256_digest(&bytes);
    Ok(plan)
}

/// Plans verification only after the established semantic-diff contract is
/// proven to describe the same node and edge changes as the supplied graphs.
pub fn plan_verification_with_diff(
    before: &IntentIrDocument,
    after: &IntentIrDocument,
    diff: &SemanticDiffContract,
    source_head_sha: &str,
    delivered_head_sha: &str,
) -> Result<VerificationPlan, String> {
    validate_semantic_diff(before, after, diff)?;
    plan_verification(before, after, source_head_sha, delivered_head_sha)
}

/// Fails closed unless every planned item has exactly one valid, passing result
/// bound to the plan and both exact commit heads.
pub fn evaluate_verification(
    plan: &VerificationPlan,
    results: &VerificationResults,
    exact_delivered_head_sha: &str,
) -> VerificationVerdict {
    let mut invalid = BTreeSet::new();
    if validate_sha(exact_delivered_head_sha).is_err() {
        invalid.insert("delivered_head_sha:invalid".into());
    }
    if !plan_shape_valid(plan) {
        invalid.insert("plan:invalid".into());
    }
    if results.schema_version != RESULTS_SCHEMA_VERSION {
        invalid.insert("results:schema_version".into());
    }
    if results.plan_digest != plan.plan_digest {
        invalid.insert("results:plan_digest".into());
    }
    if results.source_head_sha != plan.bindings.source_head_sha {
        invalid.insert("results:source_head_sha".into());
    }
    if results.delivered_head_sha != plan.bindings.delivered_head_sha
        || results.delivered_head_sha != exact_delivered_head_sha
    {
        invalid.insert("results:delivered_head_sha".into());
    }

    let required: BTreeSet<_> = plan
        .requirements
        .iter()
        .map(|item| item.id.clone())
        .collect();
    if required.len() != plan.requirements.len() {
        invalid.insert("plan:duplicate_requirement".into());
    }
    let mut counts = BTreeMap::<String, usize>::new();
    let mut failed = BTreeSet::new();
    for result in &results.results {
        *counts.entry(result.id.clone()).or_default() += 1;
        if !required.contains(&result.id) {
            invalid.insert(format!("{}:unexpected", result.id));
        }
        if result.plan_digest != plan.plan_digest {
            invalid.insert(format!("{}:plan_digest", result.id));
        }
        if result.source_head_sha != plan.bindings.source_head_sha {
            invalid.insert(format!("{}:source_head_sha", result.id));
        }
        if result.delivered_head_sha != plan.bindings.delivered_head_sha
            || result.delivered_head_sha != exact_delivered_head_sha
        {
            invalid.insert(format!("{}:delivered_head_sha", result.id));
        }
        if !valid_digest(&result.evidence_digest) {
            invalid.insert(format!("{}:evidence_digest", result.id));
        }
        if result.status == EvidenceStatus::Failed {
            failed.insert(result.id.clone());
        }
    }
    let missing: Vec<_> = required
        .iter()
        .filter(|id| !counts.contains_key(*id))
        .cloned()
        .collect();
    let duplicate: Vec<_> = counts
        .into_iter()
        .filter_map(|(id, count)| (count != 1).then_some(id))
        .collect();
    let invalid: Vec<_> = invalid.into_iter().collect();
    let failed: Vec<_> = failed.into_iter().collect();
    let passed =
        missing.is_empty() && duplicate.is_empty() && invalid.is_empty() && failed.is_empty();
    VerificationVerdict {
        schema_version: VERDICT_SCHEMA_VERSION.into(),
        plan_digest: plan.plan_digest.clone(),
        status: if passed {
            VerdictStatus::Passed
        } else {
            VerdictStatus::Failed
        },
        source_head_sha: plan.bindings.source_head_sha.clone(),
        delivered_head_sha: exact_delivered_head_sha.into(),
        missing,
        duplicate,
        invalid,
        failed,
    }
}

fn validate_document(document: &IntentIrDocument, label: &str) -> Result<(), String> {
    if document.schema_version != crate::intent_ir::SCHEMA_VERSION {
        return Err(format!("{label} document uses unsupported schema_version"));
    }
    if !document.graph_id.starts_with("axiom://graph/") || !valid_graph_id(&document.graph_id) {
        return Err(format!("{label} document has invalid graph_id"));
    }
    if !valid_graph_id(&document.package)
        || document.provenance.producer.is_empty()
        || document.provenance.path_policy != "package_relative"
        || !valid_raw_digest(&document.provenance.source_digest)
        || document.provenance.inputs.is_empty()
        || document.nodes.is_empty()
    {
        return Err(format!("{label} document is not a complete Intent IR snapshot"));
    }
    let node_ids: BTreeSet<_> = document.nodes.iter().map(|node| node.id.as_str()).collect();
    if node_ids.len() != document.nodes.len()
        || document.nodes.iter().any(|node| !valid_graph_id(&node.id) || node.kind.is_empty())
        || document.provenance.inputs.iter().any(|input| {
            !valid_graph_id(&input.package)
                || !valid_relative_path(&input.path)
                || !valid_raw_digest(&input.digest)
        })
        || document.edges.iter().any(|edge| {
            edge.kind.is_empty()
                || !node_ids.contains(edge.from.as_str())
                || !node_ids.contains(edge.to.as_str())
        })
    {
        return Err(format!("{label} document has invalid semantic references or provenance"));
    }
    Ok(())
}

fn validate_semantic_diff(
    before: &IntentIrDocument,
    after: &IntentIrDocument,
    diff: &SemanticDiffContract,
) -> Result<(), String> {
    if diff.schema_version != "axiom.semantic_diff.v0" || !diff.ok || diff.command != "semantic-diff" {
        return Err("semantic diff uses an invalid contract envelope".into());
    }
    let before_nodes: BTreeMap<_, _> = before.nodes.iter().map(|node| (&node.id, node)).collect();
    let after_nodes: BTreeMap<_, _> = after.nodes.iter().map(|node| (&node.id, node)).collect();
    let mut expected = BTreeSet::new();
    for id in before_nodes.keys().chain(after_nodes.keys()).copied().collect::<BTreeSet<_>>() {
        match (before_nodes.get(id), after_nodes.get(id)) {
            (None, Some(_)) => { expected.insert(("added".to_string(), (*id).clone())); }
            (Some(_), None) => { expected.insert(("removed".to_string(), (*id).clone())); }
            (Some(left), Some(right)) if left != right => {
                expected.insert(("modified".to_string(), (*id).clone()));
            }
            _ => {}
        }
    }
    let before_edges: BTreeMap<_, _> = before.edges.iter().map(|edge| (edge_key(edge), edge)).collect();
    let after_edges: BTreeMap<_, _> = after.edges.iter().map(|edge| (edge_key(edge), edge)).collect();
    for key in before_edges.keys().chain(after_edges.keys()).cloned().collect::<BTreeSet<_>>() {
        let change = match (before_edges.get(&key), after_edges.get(&key)) {
            (None, Some(_)) => Some("added"),
            (Some(_), None) => Some("removed"),
            (Some(left), Some(right)) if left != right => Some("modified"),
            _ => None,
        };
        if let Some(change) = change {
            expected.insert((change.into(), semantic_edge_id(&key.1, &key.0, &key.2)));
        }
    }
    let observed: BTreeSet<_> = diff
        .changes
        .iter()
        .map(|change| (change.change.clone(), change.node_id.clone()))
        .collect();
    let summary_total = diff.summary.breaking + diff.summary.additive + diff.summary.informational;
    let record_invalid = diff.changes.iter().any(|change| {
        if change.node_kind == "Edge" {
            let Some(kind) = change.edge_kind.as_deref() else { return true; };
            let Some(from) = change.edge_from.as_deref() else { return true; };
            let Some(to) = change.edge_to.as_deref() else { return true; };
            change.node_id != semantic_edge_id(kind, from, to)
                || change.severity != "breaking"
                || change.description != format!("{} {} edge {} -> {}", change.change, kind, from, to)
        } else {
            if change.edge_kind.is_some() || change.edge_from.is_some() || change.edge_to.is_some() {
                return true;
            }
            let node = match change.change.as_str() {
                "added" => after_nodes.get(&change.node_id).copied(),
                "removed" | "modified" => before_nodes.get(&change.node_id).copied(),
                _ => None,
            };
            node.is_none_or(|node| {
                let severity = match change.change.as_str() {
                    "added" => semantic_added_severity(&node.kind),
                    "removed" => semantic_removed_severity(&node.kind),
                    "modified" => semantic_modified_severity(&node.kind),
                    _ => "invalid",
                };
                change.node_kind != node.kind
                    || change.severity != severity
                    || change.description
                        != format!("{} {} {}", change.change, node.kind, node.name)
            })
        }
    });
    if observed.len() != diff.changes.len()
        || observed != expected
        || summary_total != diff.changes.len()
        || diff.summary.breaking != diff.changes.iter().filter(|change| change.severity == "breaking").count()
        || diff.summary.additive != diff.changes.iter().filter(|change| change.severity == "additive").count()
        || diff.summary.informational != diff.changes.iter().filter(|change| change.severity == "informational").count()
        || record_invalid
        || diff.changes.iter().any(|change| {
            !matches!(change.change.as_str(), "added" | "removed" | "modified")
                || !matches!(change.severity.as_str(), "breaking" | "additive" | "informational")
        })
    {
        return Err(format!(
            "semantic diff does not exactly match the supplied Intent IR snapshots: expected={expected:?} observed={observed:?} summary_total={summary_total} changes={}",
            diff.changes.len()
        ));
    }
    Ok(())
}

fn semantic_added_severity(kind: &str) -> &'static str {
    if matches!(kind, "Capability" | "Effect" | "RuntimeSurface") { "breaking" } else { "additive" }
}

fn semantic_removed_severity(kind: &str) -> &'static str {
    if matches!(kind, "Module" | "Type" | "Function") { "informational" } else { "breaking" }
}

fn semantic_modified_severity(kind: &str) -> &'static str {
    if matches!(kind, "Capability" | "Effect" | "Axiom" | "Artifact" | "RuntimeSurface") {
        "breaking"
    } else {
        "informational"
    }
}

fn semantic_edge_id(kind: &str, from: &str, to: &str) -> String {
    format!(
        "axiom://edge/{}/{}/{}",
        kind,
        from.trim_start_matches("axiom://"),
        to.trim_start_matches("axiom://")
    )
}

fn node_changes(
    before: &IntentIrDocument,
    after: &IntentIrDocument,
    unknown: &mut BTreeSet<String>,
) -> Vec<SemanticChange> {
    let left: BTreeMap<_, _> = before.nodes.iter().map(|node| (&node.id, node)).collect();
    let right: BTreeMap<_, _> = after.nodes.iter().map(|node| (&node.id, node)).collect();
    left.keys()
        .chain(right.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|id| match (left.get(id), right.get(id)) {
            (None, Some(node)) => Some(node_change(ChangeKind::Added, node, unknown)),
            (Some(node), None) => Some(node_change(ChangeKind::Removed, node, unknown)),
            (Some(old), Some(new)) if old != new => Some(modified_node_change(old, new, unknown)),
            _ => None,
        })
        .collect()
}

fn modified_node_change(
    old: &IntentIrNode,
    new: &IntentIrNode,
    unknown: &mut BTreeSet<String>,
) -> SemanticChange {
    let mut impact = node_impact(old, unknown);
    impact.extend(node_impact(new, unknown));
    impact.sort();
    impact.dedup();
    SemanticChange {
        id: format!("node:modified:{}", new.id),
        change_kind: ChangeKind::Modified,
        node_kind: new.kind.clone(),
        semantic_id: new.id.clone(),
        source_path: new
            .source_span
            .as_ref()
            .or(old.source_span.as_ref())
            .map(|span| span.path.clone()),
        impact,
    }
}

fn node_change(
    change_kind: ChangeKind,
    node: &IntentIrNode,
    unknown: &mut BTreeSet<String>,
) -> SemanticChange {
    let impact = node_impact(node, unknown);
    SemanticChange {
        id: format!("node:{}:{}", change_name(change_kind), node.id),
        change_kind,
        node_kind: node.kind.clone(),
        semantic_id: node.id.clone(),
        source_path: node.source_span.as_ref().map(|span| span.path.clone()),
        impact,
    }
}

fn edge_changes(
    before: &IntentIrDocument,
    after: &IntentIrDocument,
    unknown: &mut BTreeSet<String>,
) -> Vec<SemanticChange> {
    let left: BTreeMap<_, _> = before
        .edges
        .iter()
        .map(|edge| (edge_key(edge), edge))
        .collect();
    let right: BTreeMap<_, _> = after
        .edges
        .iter()
        .map(|edge| (edge_key(edge), edge))
        .collect();
    left.keys()
        .chain(right.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|key| match (left.get(&key), right.get(&key)) {
            (None, Some(edge)) => Some(edge_change(ChangeKind::Added, edge, unknown)),
            (Some(edge), None) => Some(edge_change(ChangeKind::Removed, edge, unknown)),
            (Some(old), Some(new)) if old != new => {
                Some(edge_change(ChangeKind::Modified, new, unknown))
            }
            _ => None,
        })
        .collect()
}

fn source_changes(
    before: &IntentIrDocument,
    after: &IntentIrDocument,
    unknown: &mut BTreeSet<String>,
) -> Vec<SemanticChange> {
    let left: BTreeMap<_, _> = before
        .provenance
        .inputs
        .iter()
        .map(|input| ((input.package.clone(), input.path.clone()), input))
        .collect();
    let right: BTreeMap<_, _> = after
        .provenance
        .inputs
        .iter()
        .map(|input| ((input.package.clone(), input.path.clone()), input))
        .collect();
    left.keys()
        .chain(right.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|(package, path)| {
            let change_kind = match (
                left.get(&(package.clone(), path.clone())),
                right.get(&(package.clone(), path.clone())),
            ) {
                (None, Some(_)) => ChangeKind::Added,
                (Some(_), None) => ChangeKind::Removed,
                (Some(old), Some(new)) if old != new => ChangeKind::Modified,
                _ => return None,
            };
            let mut impact = Vec::new();
            for node in before.nodes.iter().chain(&after.nodes).filter(|node| {
                node.source_span
                    .as_ref()
                    .is_some_and(|span| span.path == path)
            }) {
                impact.extend(node_impact(node, unknown));
            }
            if impact.is_empty() {
                unknown.insert(format!("source_path:{path}"));
                impact = all_evidence();
            }
            impact.sort();
            impact.dedup();
            let semantic_id = format!("source:{package}:{path}");
            Some(SemanticChange {
                id: format!("source:{}:{package}:{path}", change_name(change_kind)),
                change_kind,
                node_kind: "SourceInput".into(),
                semantic_id,
                source_path: Some(path),
                impact,
            })
        })
        .collect()
}

fn edge_change(
    change_kind: ChangeKind,
    edge: &IntentIrEdge,
    unknown: &mut BTreeSet<String>,
) -> SemanticChange {
    let semantic_id = format!("{}|{}|{}", edge.from, edge.kind, edge.to);
    SemanticChange {
        id: format!("edge:{}:{semantic_id}", change_name(change_kind)),
        change_kind,
        node_kind: format!("Edge: {}", edge.kind),
        semantic_id,
        source_path: None,
        impact: edge_impact(edge, unknown),
    }
}

fn node_impact(node: &IntentIrNode, unknown: &mut BTreeSet<String>) -> Vec<EvidenceKind> {
    use EvidenceKind::*;
    let mut kinds = match node.kind.as_str() {
        "Capability" | "Effect" | "RuntimeSurface" => vec![Positive, Denial, Regression, Security],
        "Axiom" => vec![Positive, Denial, Regression],
        "Dependency" => vec![Positive, Regression, Schema, Security],
        "Artifact" => vec![Regression, Schema, Artifact],
        "Package" => vec![Regression, Schema, Artifact],
        "Type" => vec![Positive, Regression, Schema],
        "Function" | "Constant" | "Global" | "Macro" => vec![Positive, Regression, Schema],
        "Module" | "Evidence" => vec![Positive, Regression],
        other => {
            unknown.insert(format!("node_kind:{other}:{}", node.id));
            all_evidence()
        }
    };
    let metadata = serde_json::to_string(&node.metadata)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if contains_any(
        &metadata,
        &["performance", "latency", "throughput", "benchmark"],
    ) {
        kinds.push(Performance);
    }
    if contains_any(&metadata, &["target", "backend", "contract"]) {
        kinds.extend([Schema, Artifact]);
    }
    kinds.sort();
    kinds.dedup();
    kinds
}

fn edge_impact(edge: &IntentIrEdge, unknown: &mut BTreeSet<String>) -> Vec<EvidenceKind> {
    use EvidenceKind::*;
    let mut kinds = match edge.kind.as_str() {
        "allows_effect" | "requires" | "uses" | "preserves" => {
            vec![Positive, Denial, Regression, Security]
        }
        "depends_on" => vec![Positive, Regression, Schema, Security],
        "generated_from" => vec![Regression, Schema, Artifact],
        "declares" => vec![Positive, Regression, Schema],
        "verified_by" => vec![Positive, Regression],
        other => {
            unknown.insert(format!("edge_kind:{other}:{}->{}", edge.from, edge.to));
            all_evidence()
        }
    };
    let metadata = serde_json::to_string(&edge.metadata)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if contains_any(
        &metadata,
        &["performance", "latency", "throughput", "benchmark"],
    ) {
        kinds.push(Performance);
    }
    kinds.sort();
    kinds.dedup();
    kinds
}

fn all_evidence() -> Vec<EvidenceKind> {
    use EvidenceKind::*;
    vec![
        Positive,
        Denial,
        Regression,
        Schema,
        Artifact,
        Security,
        Performance,
    ]
}

fn edge_key(edge: &IntentIrEdge) -> (String, String, String) {
    (edge.from.clone(), edge.kind.clone(), edge.to.clone())
}

fn snapshot_digest(document: &IntentIrDocument) -> Result<String, String> {
    let mut canonical = document.clone();
    canonical.nodes.sort_by(|a, b| a.id.cmp(&b.id));
    canonical.edges.sort_by_key(edge_key);
    canonical.diagnostics.sort();
    canonical.provenance.inputs.sort();
    serde_json::to_vec(&canonical)
        .map(|bytes| sha256_digest(&bytes))
        .map_err(|error| error.to_string())
}

fn validate_sha(sha: &str) -> Result<(), String> {
    if sha.len() == 40
        && sha
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err("commit SHA must be exactly 40 lowercase hexadecimal characters".into())
    }
}

fn valid_digest(digest: &str) -> bool {
    digest.len() == 71
        && digest.starts_with("sha256:")
        && digest[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_raw_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn plan_digest_matches(plan: &VerificationPlan) -> bool {
    if !valid_digest(&plan.plan_digest) {
        return false;
    }
    let mut canonical = plan.clone();
    let expected = canonical.plan_digest.clone();
    canonical.plan_digest.clear();
    serde_json::to_vec(&canonical).is_ok_and(|bytes| sha256_digest(&bytes) == expected)
}

fn plan_shape_valid(plan: &VerificationPlan) -> bool {
    let mut expected = BTreeMap::<EvidenceKind, BTreeSet<String>>::new();
    for change in &plan.changes {
        for kind in &change.impact {
            expected.entry(*kind).or_default().insert(change.semantic_id.clone());
        }
    }
    let observed: BTreeMap<_, _> = plan
        .requirements
        .iter()
        .map(|requirement| (requirement.kind, requirement.semantic_ids.iter().cloned().collect()))
        .collect();
    if expected != observed {
        return false;
    }
    if plan.schema_version != PLAN_SCHEMA_VERSION
        || !plan_digest_matches(plan)
        || validate_sha(&plan.bindings.source_head_sha).is_err()
        || validate_sha(&plan.bindings.delivered_head_sha).is_err()
        || !valid_digest(&plan.bindings.before_snapshot_digest)
        || !valid_digest(&plan.bindings.after_snapshot_digest)
        || !valid_graph_id(&plan.bindings.before_graph_id)
        || !valid_graph_id(&plan.bindings.after_graph_id)
        || (plan.changes.is_empty() != plan.requirements.is_empty())
        || plan.coverage.explanation.is_empty()
        || plan.changes.iter().any(|change| {
            change.id.is_empty()
                || change.node_kind.is_empty()
                || change.semantic_id.is_empty()
                || change.impact.is_empty()
                || !all_unique(&change.impact)
                || change
                    .source_path
                    .as_ref()
                    .is_some_and(|path| !valid_relative_path(path))
        })
        || plan.requirements.iter().any(|requirement| {
            requirement.id != format!("evidence-{}", kind_name(requirement.kind))
                || requirement.reason.is_empty()
                || requirement.semantic_ids.is_empty()
                || !all_unique(&requirement.semantic_ids)
        })
        || !all_unique(&plan.coverage.unknown_impacts)
    {
        return false;
    }
    match plan.coverage.confidence {
        CoverageConfidence::Complete => {
            plan.coverage.complete && plan.coverage.unknown_impacts.is_empty()
        }
        CoverageConfidence::Conservative => {
            !plan.coverage.complete
                && !plan.coverage.unknown_impacts.is_empty()
                && expected.keys().copied().collect::<BTreeSet<_>>()
                    == all_evidence().into_iter().collect()
        }
    }
}

fn all_unique<T: Ord + Clone>(values: &[T]) -> bool {
    values.iter().cloned().collect::<BTreeSet<_>>().len() == values.len()
}

fn valid_graph_id(id: &str) -> bool {
    id.strip_prefix("axiom://").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._~:/#@!$&'()*+,;=%-".contains(&byte))
    })
}

fn valid_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn kind_name(kind: EvidenceKind) -> &'static str {
    match kind {
        EvidenceKind::Positive => "positive",
        EvidenceKind::Denial => "denial",
        EvidenceKind::Regression => "regression",
        EvidenceKind::Schema => "schema",
        EvidenceKind::Artifact => "artifact",
        EvidenceKind::Security => "security",
        EvidenceKind::Performance => "performance",
    }
}

fn requirement_reason(kind: EvidenceKind) -> &'static str {
    match kind {
        EvidenceKind::Positive => "Changed behavior must succeed on its declared positive path.",
        EvidenceKind::Denial => "Denied and invalid behavior must remain fail-closed.",
        EvidenceKind::Regression => "Adjacent established behavior must remain unchanged.",
        EvidenceKind::Schema => "Public and serialized contracts must remain schema-valid.",
        EvidenceKind::Artifact => "Generated artifacts must match semantic inputs and provenance.",
        EvidenceKind::Security => {
            "Capability, dependency, and trust-boundary changes require security evidence."
        }
        EvidenceKind::Performance => {
            "Performance-sensitive behavior requires fresh baseline evidence."
        }
    }
}

fn change_name(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "added",
        ChangeKind::Removed => "removed",
        ChangeKind::Modified => "modified",
    }
}

// Local SHA-256 keeps this contract deterministic without introducing a new dependency.
fn sha256_digest(input: &[u8]) -> String {
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
    let mut message = input.to_vec();
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
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(s0.wrapping_add(maj));
        }
        for (state, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *state = state.wrapping_add(value);
        }
    }
    format!(
        "sha256:{}",
        h.iter()
            .map(|word| format!("{word:08x}"))
            .collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_ir::{IntentIrInput, IntentIrProvenance, SourceSpan};
    use serde_json::{Map, json};

    fn document(nodes: Vec<IntentIrNode>) -> IntentIrDocument {
        let mut complete_nodes = vec![node("axiom://package/test", "Package")];
        complete_nodes.extend(nodes);
        IntentIrDocument {
            schema_version: crate::intent_ir::SCHEMA_VERSION.into(),
            graph_id: "axiom://graph/test".into(),
            package: "axiom://package/test".into(),
            provenance: IntentIrProvenance {
                producer: "test".into(),
                source_digest: "0".repeat(64),
                path_policy: "package_relative".into(),
                inputs: vec![IntentIrInput {
                    package: "axiom://package/test".into(),
                    path: "src/main.ax".into(),
                    digest: "0".repeat(64),
                }],
            },
            diagnostics: vec![],
            nodes: complete_nodes,
            edges: vec![],
        }
    }

    fn node(id: &str, kind: &str) -> IntentIrNode {
        let id = if id.starts_with("axiom://") {
            id.to_owned()
        } else {
            format!("axiom://package/test/node/{id}")
        };
        IntentIrNode {
            id: id.clone(),
            kind: kind.into(),
            name: id,
            description: None,
            source_span: Some(SourceSpan {
                path: "src/main.ax".into(),
                line: Some(1),
                column: Some(1),
            }),
            metadata: Map::new(),
        }
    }

    #[test]
    fn capability_change_never_yields_empty_or_only_positive_plan() {
        let before = document(vec![]);
        let after = document(vec![node("axiom://capability/net", "Capability")]);
        let plan = plan_verification(&before, &after, &"a".repeat(40), &"b".repeat(40)).unwrap();
        let kinds: BTreeSet<_> = plan.requirements.iter().map(|item| item.kind).collect();
        assert!(kinds.is_superset(&BTreeSet::from([
            EvidenceKind::Positive,
            EvidenceKind::Denial,
            EvidenceKind::Regression,
            EvidenceKind::Security,
        ])));
    }

    #[test]
    fn unknown_change_broadens_to_every_suite() {
        let before = document(vec![]);
        let after = document(vec![node("axiom://future/x", "FuturePrimitive")]);
        let plan = plan_verification(&before, &after, &"a".repeat(40), &"b".repeat(40)).unwrap();
        assert_eq!(plan.requirements.len(), 7);
        assert_eq!(plan.coverage.confidence, CoverageConfidence::Conservative);
        assert!(!plan.coverage.complete);
    }

    #[test]
    fn ordering_does_not_change_snapshot_or_plan_digest() {
        let a = node("axiom://function/a", "Function");
        let b = node("axiom://function/b", "Function");
        let before = document(vec![]);
        let left = plan_verification(
            &before,
            &document(vec![a.clone(), b.clone()]),
            &"a".repeat(40),
            &"b".repeat(40),
        )
        .unwrap();
        let right = plan_verification(
            &before,
            &document(vec![b, a]),
            &"a".repeat(40),
            &"b".repeat(40),
        )
        .unwrap();
        assert_eq!(left, right);
    }

    #[test]
    fn exact_one_fresh_passing_result_is_required() {
        let plan = plan_verification(
            &document(vec![]),
            &document(vec![node("x", "Module")]),
            &"a".repeat(40),
            &"b".repeat(40),
        )
        .unwrap();
        let results = VerificationResults {
            schema_version: RESULTS_SCHEMA_VERSION.into(),
            plan_digest: plan.plan_digest.clone(),
            source_head_sha: "a".repeat(40),
            delivered_head_sha: "b".repeat(40),
            results: plan
                .requirements
                .iter()
                .map(|requirement| EvidenceResult {
                    id: requirement.id.clone(),
                    plan_digest: plan.plan_digest.clone(),
                    source_head_sha: "a".repeat(40),
                    delivered_head_sha: "b".repeat(40),
                    status: EvidenceStatus::Passed,
                    evidence_digest: sha256_digest(requirement.id.as_bytes()),
                })
                .collect(),
        };
        assert_eq!(
            evaluate_verification(&plan, &results, &"b".repeat(40)).status,
            VerdictStatus::Passed
        );
        let mut duplicate = results.clone();
        duplicate.results.push(duplicate.results[0].clone());
        assert_eq!(
            evaluate_verification(&plan, &duplicate, &"b".repeat(40)).status,
            VerdictStatus::Failed
        );
        assert_eq!(
            evaluate_verification(&plan, &results, &"c".repeat(40)).status,
            VerdictStatus::Failed
        );
        let mut tampered = plan.clone();
        tampered.requirements[0].reason.push_str(" altered");
        assert_eq!(
            evaluate_verification(&tampered, &results, &"b".repeat(40)).status,
            VerdictStatus::Failed
        );
    }

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(
            sha256_digest(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn performance_metadata_adds_baseline_requirement() {
        let mut changed = node("x", "Function");
        changed
            .metadata
            .insert("performance_sensitive".into(), json!(true));
        let plan = plan_verification(
            &document(vec![]),
            &document(vec![changed]),
            &"a".repeat(40),
            &"b".repeat(40),
        )
        .unwrap();
        assert!(
            plan.requirements
                .iter()
                .any(|item| item.kind == EvidenceKind::Performance)
        );
    }

    #[test]
    fn modified_nodes_preserve_removed_security_and_performance_impact() {
        let mut old = node("x", "Capability");
        old.metadata
            .insert("performance_sensitive".into(), json!(true));
        let new = node("x", "Function");
        let plan = plan_verification(
            &document(vec![old]),
            &document(vec![new]),
            &"a".repeat(40),
            &"b".repeat(40),
        )
        .unwrap();
        let kinds: BTreeSet<_> = plan.requirements.iter().map(|item| item.kind).collect();
        assert!(kinds.contains(&EvidenceKind::Denial));
        assert!(kinds.contains(&EvidenceKind::Security));
        assert!(kinds.contains(&EvidenceKind::Performance));
    }

    #[test]
    fn source_digest_change_maps_through_source_spans() {
        let mut before = document(vec![node("x", "Function")]);
        let mut after = before.clone();
        before.provenance.inputs[0].digest = "1".repeat(64);
        after.provenance.inputs[0].digest = "2".repeat(64);
        let plan = plan_verification(&before, &after, &"a".repeat(40), &"b".repeat(40)).unwrap();
        assert!(
            plan.changes
                .iter()
                .any(|change| change.node_kind == "SourceInput")
        );
        assert!(
            plan.requirements
                .iter()
                .any(|item| item.kind == EvidenceKind::Regression)
        );
    }

    #[test]
    fn mapped_and_top_level_unknown_change_still_broadens_every_suite() {
        let before = document(vec![]);
        let mut after = document(vec![node("added", "Function")]);
        after.provenance.producer = "different-producer".into();
        let plan = plan_verification(&before, &after, &"a".repeat(40), &"b".repeat(40)).unwrap();
        assert_eq!(plan.coverage.confidence, CoverageConfidence::Conservative);
        assert_eq!(
            plan.requirements.iter().map(|item| item.kind).collect::<BTreeSet<_>>(),
            all_evidence().into_iter().collect()
        );
    }

    #[test]
    fn conservative_plan_cannot_be_redigested_with_a_narrower_suite() {
        let before = document(vec![]);
        let after = document(vec![node("future", "FuturePrimitive")]);
        let mut plan = plan_verification(&before, &after, &"a".repeat(40), &"b".repeat(40)).unwrap();
        plan.changes[0].impact = vec![EvidenceKind::Positive];
        plan.requirements.retain(|item| item.kind == EvidenceKind::Positive);
        plan.plan_digest.clear();
        plan.plan_digest = sha256_digest(&serde_json::to_vec(&plan).unwrap());
        assert!(!plan_shape_valid(&plan));
    }
}
