use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const COMPATIBILITY_REPORT_SCHEMA: &str = "axiom.compatibility_report.v1";
pub const MIGRATION_PLAN_SCHEMA: &str = "axiom.migration_plan.v1";

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Breaking,
    Additive,
    Deprecated,
    Compatible,
}

impl Severity {
    fn rank(self) -> u8 {
        match self {
            Self::Breaking => 0,
            Self::Deprecated => 1,
            Self::Additive => 2,
            Self::Compatible => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SurfaceKind {
    Compiler,
    Language,
    Stdlib,
    Cli,
    Package,
    Abi,
    Schema,
    Artifact,
}

impl SurfaceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Compiler => "compiler",
            Self::Language => "language",
            Self::Stdlib => "stdlib",
            Self::Cli => "cli",
            Self::Package => "package",
            Self::Abi => "abi",
            Self::Schema => "schema",
            Self::Artifact => "artifact",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum ChangeKind {
    Added,
    Removed,
    Modified,
    Deprecated,
}

impl ChangeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Modified => "modified",
            Self::Deprecated => "deprecated",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompatibilityReport {
    schema_version: String,
    ok: bool,
    command: String,
    old: String,
    new: String,
    edition: EditionChange,
    summary: CompatibilitySummary,
    changes: Vec<CompatibilityChange>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditionChange {
    old: String,
    new: String,
    severity: Severity,
    migration: RequiredNullableString,
    #[serde(default)]
    replacement: OptionalNonNullString,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompatibilitySummary {
    breaking: usize,
    additive: usize,
    deprecated: usize,
    compatible: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompatibilityChange {
    #[serde(rename = "change")]
    change_kind: ChangeKind,
    severity: Severity,
    surface_kind: SurfaceKind,
    surface_id: String,
    #[serde(default)]
    old_version: OptionalNonNullString,
    #[serde(default)]
    new_version: OptionalNonNullString,
    description: String,
    migration: RequiredNullableString,
    #[serde(default)]
    replacement: OptionalNonNullString,
}

#[derive(Debug, Deserialize)]
#[serde(transparent)]
struct RequiredNullableString(Option<String>);

#[derive(Debug, Default)]
struct OptionalNonNullString(Option<String>);

impl<'de> Deserialize<'de> for OptionalNonNullString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer).map(|value| Self(Some(value)))
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MigrationPlan {
    pub schema_version: &'static str,
    pub ok: bool,
    pub command: &'static str,
    pub mode: &'static str,
    pub source: MigrationSource,
    pub editions: EditionBinding,
    pub actions: Vec<MigrationAction>,
    pub effects: MigrationEffects,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MigrationSource {
    pub schema_version: &'static str,
    pub old_contract: String,
    pub new_contract: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EditionBinding {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MigrationActionKind {
    Edition,
    Breaking,
    Deprecated,
    Replacement,
}

impl MigrationActionKind {
    fn rank(self) -> u8 {
        match self {
            Self::Edition => 0,
            Self::Breaking => 1,
            Self::Deprecated => 2,
            Self::Replacement => 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MigrationAction {
    pub sequence: usize,
    pub id: String,
    pub kind: MigrationActionKind,
    pub severity: Severity,
    pub surface_kind: Option<SurfaceKind>,
    pub surface_id: Option<String>,
    pub instruction: String,
    pub replacement: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MigrationEffects {
    pub source_rewriting: bool,
    pub package_resolution: bool,
    pub release_publication: bool,
    pub policy_changes: bool,
}

#[derive(Debug)]
struct ActionDraft {
    id: String,
    kind: MigrationActionKind,
    severity: Severity,
    surface_kind: Option<SurfaceKind>,
    surface_id: Option<String>,
    instruction: String,
    replacement: Option<String>,
}

pub fn migration_plan_from_slice(bytes: &[u8]) -> Result<MigrationPlan, String> {
    let report: CompatibilityReport = serde_json::from_slice(bytes)
        .map_err(|error| format!("invalid compatibility report: {error}"))?;
    validate_report(&report)?;
    build_plan(report)
}

fn validate_report(report: &CompatibilityReport) -> Result<(), String> {
    if report.schema_version != COMPATIBILITY_REPORT_SCHEMA {
        return Err(format!(
            "compatibility report must use {COMPATIBILITY_REPORT_SCHEMA}"
        ));
    }
    if !report.ok {
        return Err("compatibility report must be successful (ok=true)".to_owned());
    }
    if report.command != "compatibility-report" {
        return Err("compatibility report command must be compatibility-report".to_owned());
    }
    require_text(&report.old, "compatibility report old contract")?;
    require_text(&report.new, "compatibility report new contract")?;
    validate_edition(&report.edition)?;

    let mut observed = [0usize; 4];
    let mut surface_ids = BTreeSet::new();
    let mut previous_key: Option<(u8, &str, &str, &str)> = None;
    for change in &report.changes {
        validate_change(change)?;
        if !surface_ids.insert(change.surface_id.as_str()) {
            return Err(format!(
                "compatibility report duplicates surface change {}",
                change.surface_id
            ));
        }
        match change.severity {
            Severity::Breaking => observed[0] += 1,
            Severity::Additive => observed[1] += 1,
            Severity::Deprecated => observed[2] += 1,
            Severity::Compatible => observed[3] += 1,
        }
        let key = (
            change.severity.rank(),
            change.surface_kind.as_str(),
            change.surface_id.as_str(),
            change.change_kind.as_str(),
        );
        if previous_key.is_some_and(|previous| previous > key) {
            return Err("compatibility report changes are not deterministically sorted".to_owned());
        }
        previous_key = Some(key);
    }
    let expected = [
        report.summary.breaking,
        report.summary.additive,
        report.summary.deprecated,
        report.summary.compatible,
    ];
    if observed != expected {
        return Err(format!(
            "compatibility report summary does not match changes: expected {expected:?}, observed {observed:?}"
        ));
    }
    Ok(())
}

fn validate_edition(edition: &EditionChange) -> Result<(), String> {
    if !valid_edition(&edition.old) || !valid_edition(&edition.new) {
        return Err("compatibility report editions must be four ASCII digits".to_owned());
    }
    if edition.old != edition.new && edition.severity != Severity::Breaking {
        return Err("an edition change must have breaking severity".to_owned());
    }
    if edition.old == edition.new && edition.severity == Severity::Breaking {
        return Err("breaking edition severity requires different editions".to_owned());
    }
    if matches!(edition.severity, Severity::Breaking | Severity::Deprecated) {
        require_optional_text(
            edition.migration.0.as_deref(),
            "breaking or deprecated edition migration action",
        )?;
    }
    if matches!(edition.severity, Severity::Additive) {
        return Err("edition severity cannot be additive".to_owned());
    }
    if edition.severity == Severity::Deprecated {
        let replacement = edition
            .replacement
            .0
            .as_deref()
            .ok_or_else(|| "deprecated edition replacement is required".to_owned())?;
        if !valid_edition(replacement) || replacement == edition.new {
            return Err(
                "deprecated edition replacement must be a different four-digit edition".to_owned(),
            );
        }
    } else if edition.replacement.0.is_some() {
        return Err("edition replacement is only valid for deprecated editions".to_owned());
    }
    Ok(())
}

fn validate_change(change: &CompatibilityChange) -> Result<(), String> {
    require_axiom_id(&change.surface_id, "surface_id")?;
    require_text(&change.description, "change description")?;
    if let Some(version) = change.old_version.0.as_deref() {
        require_semver(version, "old_version")?;
    }
    if let Some(version) = change.new_version.0.as_deref() {
        require_semver(version, "new_version")?;
    }
    match change.change_kind {
        ChangeKind::Added => {
            if change.severity != Severity::Additive
                || change.old_version.0.is_some()
                || change.new_version.0.is_none()
            {
                return Err(format!(
                    "added surface {} must be additive with only new_version",
                    change.surface_id
                ));
            }
        }
        ChangeKind::Removed => {
            if change.severity != Severity::Breaking
                || change.old_version.0.is_none()
                || change.new_version.0.is_some()
            {
                return Err(format!(
                    "removed surface {} must be breaking with only old_version",
                    change.surface_id
                ));
            }
        }
        ChangeKind::Deprecated => {
            if change.severity != Severity::Deprecated
                || change.old_version.0.is_none()
                || change.new_version.0.is_none()
            {
                return Err(format!(
                    "deprecated surface {} must be deprecated with old_version and new_version",
                    change.surface_id
                ));
            }
        }
        ChangeKind::Modified => {
            if change.old_version.0.is_none()
                || change.new_version.0.is_none()
                || change.severity == Severity::Deprecated
            {
                return Err(format!(
                    "modified surface {} must have old_version and new_version with non-deprecated severity",
                    change.surface_id
                ));
            }
        }
    }
    if matches!(change.severity, Severity::Breaking | Severity::Deprecated) {
        require_optional_text(
            change.migration.0.as_deref(),
            &format!("{} surface migration action", change.surface_id),
        )?;
    }
    if change.severity == Severity::Deprecated {
        require_optional_axiom_id(
            change.replacement.0.as_deref(),
            &format!("deprecated surface {} replacement", change.surface_id),
        )?;
    }
    if change.replacement.0.is_some()
        && !matches!(change.severity, Severity::Breaking | Severity::Deprecated)
    {
        return Err(format!(
            "non-actionable surface {} cannot declare a replacement",
            change.surface_id
        ));
    }
    if let Some(replacement) = change.replacement.0.as_deref() {
        require_axiom_id(replacement, "replacement")?;
        if replacement == change.surface_id {
            return Err(format!(
                "surface {} cannot replace itself",
                change.surface_id
            ));
        }
    }
    Ok(())
}

fn build_plan(report: CompatibilityReport) -> Result<MigrationPlan, String> {
    let mut drafts = Vec::new();
    if matches!(
        report.edition.severity,
        Severity::Breaking | Severity::Deprecated
    ) {
        drafts.push(ActionDraft {
            id: format!("edition:{}->{}", report.edition.old, report.edition.new),
            kind: MigrationActionKind::Edition,
            severity: report.edition.severity,
            surface_kind: None,
            surface_id: None,
            instruction: report
                .edition
                .migration
                .0
                .clone()
                .expect("validated edition migration"),
            replacement: report.edition.replacement.0.clone(),
        });
    }
    for change in &report.changes {
        let kind = match change.severity {
            Severity::Breaking => Some(MigrationActionKind::Breaking),
            Severity::Deprecated => Some(MigrationActionKind::Deprecated),
            Severity::Additive | Severity::Compatible => None,
        };
        if let Some(kind) = kind {
            drafts.push(ActionDraft {
                id: format!("{}:{}", action_kind_name(kind), change.surface_id),
                kind,
                severity: change.severity,
                surface_kind: Some(change.surface_kind),
                surface_id: Some(change.surface_id.clone()),
                instruction: change
                    .migration
                    .0
                    .clone()
                    .expect("validated surface migration"),
                replacement: (kind == MigrationActionKind::Deprecated)
                    .then(|| change.replacement.0.clone())
                    .flatten(),
            });
        }
        if let Some(replacement) = change.replacement.0.as_deref() {
            drafts.push(ActionDraft {
                id: format!("replacement:{}", change.surface_id),
                kind: MigrationActionKind::Replacement,
                severity: change.severity,
                surface_kind: Some(change.surface_kind),
                surface_id: Some(change.surface_id.clone()),
                instruction: format!("Replace {} with {replacement}.", change.surface_id),
                replacement: Some(replacement.to_owned()),
            });
        }
    }
    drafts.sort_by(|left, right| {
        (
            left.kind.rank(),
            left.surface_kind.map(SurfaceKind::as_str).unwrap_or(""),
            left.surface_id.as_deref().unwrap_or(""),
            left.id.as_str(),
        )
            .cmp(&(
                right.kind.rank(),
                right.surface_kind.map(SurfaceKind::as_str).unwrap_or(""),
                right.surface_id.as_deref().unwrap_or(""),
                right.id.as_str(),
            ))
    });
    if drafts.is_empty() {
        return Err("compatibility report contains no migration actions".to_owned());
    }
    let actions = drafts
        .into_iter()
        .enumerate()
        .map(|(index, draft)| MigrationAction {
            sequence: index + 1,
            id: draft.id,
            kind: draft.kind,
            severity: draft.severity,
            surface_kind: draft.surface_kind,
            surface_id: draft.surface_id,
            instruction: draft.instruction,
            replacement: draft.replacement,
        })
        .collect();
    Ok(MigrationPlan {
        schema_version: MIGRATION_PLAN_SCHEMA,
        ok: true,
        command: "migrate",
        mode: "plan-only",
        source: MigrationSource {
            schema_version: COMPATIBILITY_REPORT_SCHEMA,
            old_contract: report.old,
            new_contract: report.new,
        },
        editions: EditionBinding {
            from: report.edition.old,
            to: report.edition.new,
        },
        actions,
        effects: MigrationEffects {
            source_rewriting: false,
            package_resolution: false,
            release_publication: false,
            policy_changes: false,
        },
    })
}

fn action_kind_name(kind: MigrationActionKind) -> &'static str {
    match kind {
        MigrationActionKind::Edition => "edition",
        MigrationActionKind::Breaking => "breaking",
        MigrationActionKind::Deprecated => "deprecated",
        MigrationActionKind::Replacement => "replacement",
    }
}

fn require_text(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label} must be a non-empty string"))
    } else {
        Ok(())
    }
}

fn require_optional_text(value: Option<&str>, label: &str) -> Result<(), String> {
    value
        .ok_or_else(|| format!("{label} is required"))
        .and_then(|value| require_text(value, label))
}

fn valid_edition(value: &str) -> bool {
    value.len() == 4 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn require_semver(value: &str, label: &str) -> Result<(), String> {
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
        })
    {
        return Err(format!("{label} must be canonical SemVer"));
    }
    Ok(())
}

fn require_optional_axiom_id(value: Option<&str>, label: &str) -> Result<(), String> {
    value
        .ok_or_else(|| format!("{label} is required"))
        .and_then(|value| require_axiom_id(value, label))
}

fn require_axiom_id(value: &str, label: &str) -> Result<(), String> {
    let Some(rest) = value.strip_prefix("axiom://") else {
        return Err(format!("{label} must be an axiom:// identifier"));
    };
    if rest.is_empty()
        || !rest.chars().all(|character| {
            character.is_ascii_alphanumeric() || "._~:/#@!$&'()*+,;=%-".contains(character)
        })
    {
        return Err(format!("{label} must be an axiom:// identifier"));
    }
    Ok(())
}
