//! Durable, fail-closed workspace transactions for unattended agents.
//!
//! A transaction owns a detached Git worktree at an exact commit. All file
//! mutations are built-ins so their canonical targets can be checked before
//! use. External commands and network remain unavailable until a caller has
//! independently established a verified sandbox.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const STATE_FILE: &str = ".axiom-transaction.json";
const LEASE_FILE: &str = ".axiom-transaction.lock";
const AUDIT_SCHEMA: &str = "axiom.transactional_workspace.v0";
static OWNER_EPOCH_COUNTER: AtomicU64 = AtomicU64::new(0);
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspacePolicy {
    pub allowed_read_paths: BTreeSet<String>,
    pub allowed_write_paths: BTreeSet<String>,
    pub allowed_commands: BTreeSet<String>,
    pub allow_network: bool,
    pub verified_sandbox: bool,
}

impl Default for WorkspacePolicy {
    fn default() -> Self {
        Self {
            allowed_read_paths: BTreeSet::new(),
            allowed_write_paths: BTreeSet::new(),
            allowed_commands: BTreeSet::new(),
            allow_network: false,
            verified_sandbox: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditEvent {
    pub sequence: u64,
    pub operation: String,
    pub subject: String,
    pub result: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransactionState {
    pub schema_version: String,
    pub base_sha: String,
    pub source_checkout: String,
    pub worktree: String,
    pub policy: WorkspacePolicy,
    pub phase: TransactionPhase,
    pub checkpoint_tree: String,
    pub events: Vec<AuditEvent>,
    pub artifacts: BTreeSet<String>,
    pub rollback_result: Option<String>,
    pub checksum: String,
    pub transaction_id: String,
    pub task_contract_digest: String,
    pub policy_digest: String,
    pub branch: String,
    pub pending_effect: Option<String>,
    pub workspace_fingerprint: String,
    /// Per-path cache for the policy-scoped workspace fingerprint. The cache
    /// is updated only for paths affected by a durable filesystem effect.
    #[serde(default)]
    pub authorized_path_fingerprints: BTreeMap<String, String>,
    /// Full-verification baseline for everything outside the authorized path
    /// scope. Recovery recomputes this; effect completion does not.
    #[serde(default)]
    pub unscoped_workspace_fingerprint: String,
    pub source_fingerprint: String,
    /// A unique durable owner epoch. Recovery claims a new epoch while its
    /// exclusive lease is held, so stale writers cannot be silently merged.
    #[serde(default)]
    pub owner_epoch: String,
    /// Monotonically increasing compare-and-swap generation for the state
    /// file. It advances on every durable state replacement.
    #[serde(default)]
    pub generation: u64,
    /// The one candidate commit proven to contain the executor's exact
    /// authorized bytes. Active recovery may accept this head in addition to
    /// `base_sha`; arbitrary descendants remain forbidden.
    #[serde(default)]
    pub authorized_candidate_head: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionTransactionAudit {
    pub schema_version: String,
    pub transaction_id: String,
    pub task_contract_digest: String,
    pub policy_digest: String,
    pub base_sha: String,
    pub branch: String,
    pub status: String,
    pub checkpoints: Vec<AuditCheckpoint>,
    pub reads: Vec<FileObservation>,
    pub writes: Vec<AuditWrite>,
    pub commands: Vec<AuditCommand>,
    pub artifacts: Vec<AuditArtifact>,
    pub rollback: RollbackAudit,
    pub recovery: RecoveryAudit,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditCheckpoint {
    pub sequence: u64,
    pub kind: String,
    pub tree_sha: String,
    pub created_before: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileObservation {
    pub sequence: u64,
    pub path: String,
    pub digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditWrite {
    pub sequence: u64,
    pub operation: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    pub before_digest: Option<String>,
    pub after_digest: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditCommand {
    pub sequence: u64,
    pub argv: Vec<String>,
    pub capabilities: Vec<String>,
    pub network_hosts: Vec<String>,
    pub outcome: String,
    pub exit_code: i32,
    pub stdout_digest: String,
    pub stderr_digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditArtifact {
    pub sequence: u64,
    pub path: String,
    pub digest: String,
    pub kind: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RollbackAudit {
    pub attempted: bool,
    pub result: String,
    pub restored_checkpoint: Option<u64>,
    pub source_checkout_untouched: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryAudit {
    pub resumable: bool,
    pub rollback_safe: bool,
    pub next_sequence: u64,
    pub journal_digest: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransactionPhase {
    Active,
    Interrupted,
    Committed,
    Aborted,
}

#[derive(Debug)]
pub struct TransactionalWorkspace {
    state_path: PathBuf,
    state: TransactionState,
    /// Held for the complete lifetime of this object. Closing the descriptor
    /// releases the OS-level cross-process lease.
    _lease: File,
}

impl TransactionalWorkspace {
    /// Create a detached, tool-owned worktree at `base_sha` without changing
    /// the source checkout's index, branch, or working files.
    pub fn create(
        source_checkout: &Path,
        worktree: &Path,
        base_sha: &str,
        policy: WorkspacePolicy,
    ) -> Result<Self, String> {
        let policy_bytes = serde_json::to_vec(&policy).map_err(|e| e.to_string())?;
        let policy_digest = sha256_digest(&policy_bytes);
        Self::create_for_task(
            source_checkout,
            worktree,
            base_sha,
            policy,
            &sha256_digest(b"unspecified-task"),
            &policy_digest,
            "detached",
        )
    }

    pub fn create_for_task(
        source_checkout: &Path,
        worktree: &Path,
        base_sha: &str,
        policy: WorkspacePolicy,
        task_contract_digest: &str,
        policy_digest: &str,
        branch: &str,
    ) -> Result<Self, String> {
        validate_sha(base_sha)?;
        validate_policy(&policy)?;
        validate_digest(task_contract_digest)?;
        validate_digest(policy_digest)?;
        if branch.trim().is_empty() {
            return Err("branch must not be empty".into());
        }
        let source = source_checkout
            .canonicalize()
            .map_err(|e| format!("cannot canonicalize source checkout: {e}"))?;
        if !worktree.is_absolute() || worktree.starts_with(&source) {
            return Err(
                "transaction worktree must be an absolute sibling outside the source checkout"
                    .into(),
            );
        }
        if worktree.exists() {
            return Err("transaction worktree already exists".into());
        }
        let verified = git(
            &source,
            &["rev-parse", "--verify", &format!("{base_sha}^{{commit}}")],
        )?;
        let exact_sha = verified.trim();
        if exact_sha != base_sha {
            return Err("base SHA must be the exact full commit SHA".into());
        }
        let worktree_text = worktree
            .to_str()
            .ok_or_else(|| "worktree path is not UTF-8".to_string())?;
        if branch == "detached" {
            git(
                &source,
                &["worktree", "add", "--detach", worktree_text, exact_sha],
            )?;
        } else {
            if branch.starts_with('-')
                || branch.contains("..")
                || branch.contains(char::is_whitespace)
            {
                return Err("invalid transaction-owned branch name".into());
            }
            git(
                &source,
                &["worktree", "add", "-b", branch, worktree_text, exact_sha],
            )?;
        }
        let canonical_worktree = worktree
            .canonicalize()
            .map_err(|e| format!("cannot canonicalize transaction worktree: {e}"))?;
        let lease = acquire_lease(&canonical_worktree.join(LEASE_FILE))?;
        let checkpoint_tree = git(&canonical_worktree, &["rev-parse", "HEAD^{tree}"])?
            .trim()
            .to_string();
        let txn_digest = sha256_digest(
            format!("{exact_sha}\0{task_contract_digest}\0{policy_digest}\0{branch}").as_bytes(),
        );
        let authorized_path_fingerprints =
            authorized_path_fingerprint_cache(&canonical_worktree, &policy)?;
        let workspace_fingerprint =
            policy_scoped_fingerprint(&canonical_worktree, &policy, &authorized_path_fingerprints)?;
        let unscoped_workspace_fingerprint =
            unscoped_workspace_fingerprint(&canonical_worktree, &policy)?;
        let source_fingerprint = source_fingerprint(&source)?;
        let mut this = Self {
            state_path: canonical_worktree.join(STATE_FILE),
            state: TransactionState {
                schema_version: AUDIT_SCHEMA.into(),
                base_sha: exact_sha.into(),
                source_checkout: normalized(&source),
                worktree: normalized(&canonical_worktree),
                policy,
                phase: TransactionPhase::Active,
                checkpoint_tree,
                events: Vec::new(),
                artifacts: BTreeSet::new(),
                rollback_result: None,
                checksum: String::new(),
                transaction_id: format!("txn-{}", &txn_digest[7..23]),
                task_contract_digest: task_contract_digest.into(),
                policy_digest: policy_digest.into(),
                branch: branch.into(),
                pending_effect: None,
                workspace_fingerprint,
                authorized_path_fingerprints,
                unscoped_workspace_fingerprint,
                source_fingerprint,
                owner_epoch: new_owner_epoch(),
                generation: 0,
                authorized_candidate_head: None,
            },
            _lease: lease,
        };
        this.record("checkpoint", exact_sha, "created")?;
        Ok(this)
    }

    /// Open and verify an interrupted or active transaction. Corrupt or
    /// partially written state is rejected instead of guessed at.
    pub fn recover(worktree: &Path) -> Result<Self, String> {
        let root = worktree
            .canonicalize()
            .map_err(|e| format!("cannot canonicalize worktree: {e}"))?;
        let state_path = root.join(STATE_FILE);
        let lease = acquire_lease(&root.join(LEASE_FILE))?;
        let bytes =
            fs::read(&state_path).map_err(|e| format!("cannot read transaction state: {e}"))?;
        let mut state: TransactionState = serde_json::from_slice(&bytes)
            .map_err(|e| format!("invalid transaction state: {e}"))?;
        let expected = state_checksum(&state)?;
        if state.checksum != expected {
            return Err("transaction state checksum mismatch".into());
        }
        if state.worktree != normalized(&root) {
            return Err("transaction state belongs to another worktree".into());
        }
        validate_owner_epoch(&state.owner_epoch)?;
        let observed_head = git(&root, &["rev-parse", "HEAD"])?;
        let observed_head = observed_head.trim();
        let authorized_candidate = state
            .authorized_candidate_head
            .as_deref()
            .is_some_and(|head| head == observed_head);
        if observed_head != state.base_sha
            && state.phase != TransactionPhase::Committed
            && !authorized_candidate
        {
            return Err("transaction HEAD no longer matches its exact base SHA".into());
        }
        if let Some(candidate_head) = &state.authorized_candidate_head {
            validate_sha(candidate_head)?;
        }
        if state
            .events
            .iter()
            .enumerate()
            .any(|(index, event)| event.sequence != index as u64)
        {
            return Err("transaction journal sequence is corrupt".into());
        }
        if state.pending_effect.is_none() && !workspace_matches_state(&root, &state)? {
            return Err("transaction worktree does not match its durable journal".into());
        }
        let previous_epoch = state.owner_epoch.clone();
        let previous_generation = state.generation;
        state.owner_epoch = new_owner_epoch();
        persist_state(
            &state_path,
            &mut state,
            Some((&previous_epoch, previous_generation)),
        )?;
        Ok(Self {
            state_path,
            state,
            _lease: lease,
        })
    }

    pub fn state(&self) -> &TransactionState {
        &self.state
    }

    pub fn mark_interrupted(&mut self) -> Result<(), String> {
        self.require_active()?;
        self.state.phase = TransactionPhase::Interrupted;
        self.record("lifecycle", "transaction", "interrupted")
    }

    pub fn resume(&mut self) -> Result<(), String> {
        if self.state.phase != TransactionPhase::Interrupted {
            return Err("only an interrupted transaction may be resumed".into());
        }
        if self.state.pending_effect.is_some() {
            return Err(
                "an interrupted filesystem effect is ambiguous; rollback is required".into(),
            );
        }
        if !workspace_matches_state(&self.root(), &self.state)? {
            return Err("transaction worktree changed after its last durable event".into());
        }
        self.state.phase = TransactionPhase::Active;
        self.record("lifecycle", "transaction", "resumed")
    }

    pub fn read(&mut self, path: &str) -> Result<Vec<u8>, String> {
        self.require_active()?;
        let target = self.authorize_existing(path, &self.state.policy.allowed_read_paths)?;
        let result = fs::read(&target).map_err(|e| format!("read failed: {e}"));
        let audit_result = result
            .as_ref()
            .map(|bytes| sha256_digest(bytes))
            .unwrap_or_else(|_| "failed".into());
        self.record("read", path, &audit_result)?;
        result
    }

    /// Read the exact Git blob for an authorized path at a full commit SHA.
    /// This is the trusted candidate-to-delivered-head proof used by the
    /// bounded executor; callers cannot substitute self-asserted bytes.
    pub fn read_at_commit(&mut self, path: &str, commit_sha: &str) -> Result<Vec<u8>, String> {
        self.require_active()?;
        validate_sha(commit_sha)?;
        validate_relative(path)?;
        if !self.state.policy.allowed_read_paths.contains(path) {
            return Err("path is outside task read scope".into());
        }
        let root = self.root();
        let verified = git(
            &root,
            &["rev-parse", "--verify", &format!("{commit_sha}^{{commit}}")],
        )?;
        if verified.trim() != commit_sha {
            return Err("delivered head must be an exact full commit SHA".into());
        }
        let ancestry = Command::new("git")
            .args([
                "merge-base",
                "--is-ancestor",
                &self.state.base_sha,
                commit_sha,
            ])
            .current_dir(&root)
            .status()
            .map_err(|error| format!("failed to verify delivered ancestry: {error}"))?;
        if !ancestry.success() {
            return Err("delivered head is not a descendant of the transaction base".into());
        }
        let changed = Command::new("git")
            .args([
                "diff",
                "--name-only",
                "-z",
                &self.state.base_sha,
                commit_sha,
            ])
            .current_dir(&root)
            .output()
            .map_err(|error| format!("failed to inspect delivered scope: {error}"))?;
        if !changed.status.success() {
            return Err("cannot inspect delivered-head file scope".into());
        }
        let mut contains_candidate = false;
        for changed_path in changed
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
        {
            let changed_path = std::str::from_utf8(changed_path)
                .map_err(|_| "delivered-head paths are not UTF-8".to_string())?;
            validate_relative(changed_path)?;
            if !self.state.policy.allowed_write_paths.contains(changed_path) {
                return Err("delivered head changes a path outside task write scope".into());
            }
            contains_candidate |= changed_path == path;
        }
        if !contains_candidate {
            return Err("delivered head does not change the proposed candidate path".into());
        }
        let output = Command::new("git")
            .args(["show", &format!("{commit_sha}:{path}")])
            .current_dir(&root)
            .output()
            .map_err(|error| format!("failed to inspect delivered blob: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "cannot read delivered blob: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let blob_digest = sha256_digest(&output.stdout);
        self.record("read_commit", &format!("{commit_sha}:{path}"), &blob_digest)?;
        Ok(output.stdout)
    }

    /// Durably authorize the exact candidate head after its blob was proven to
    /// match a previously approved candidate digest. This is intentionally
    /// crate-private: only the bounded executor may extend active recovery
    /// beyond `base_sha`, and only for its one verified candidate commit.
    pub(crate) fn authorize_verified_candidate_commit(
        &mut self,
        path: &str,
        commit_sha: &str,
        candidate_digest: &str,
    ) -> Result<(), String> {
        validate_digest(candidate_digest)?;
        let candidate = self.read_at_commit(path, commit_sha)?;
        if sha256_digest(&candidate) != candidate_digest {
            return Err("delivered head does not contain the exact candidate bytes".into());
        }
        let root = self.root();
        if unscoped_workspace_fingerprint(&root, &self.state.policy)?
            != self.state.unscoped_workspace_fingerprint
        {
            return Err("transaction worktree changed outside task scope".into());
        }
        self.state.authorized_candidate_head = Some(commit_sha.into());
        self.state.authorized_path_fingerprints =
            authorized_path_fingerprint_cache(&root, &self.state.policy)?;
        self.state.workspace_fingerprint = policy_scoped_fingerprint(
            &root,
            &self.state.policy,
            &self.state.authorized_path_fingerprints,
        )?;
        self.record("candidate_head_authorized", commit_sha, candidate_digest)
    }

    pub fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.require_active()?;
        let target = self.authorize_write(path)?;
        let before = file_digest(&target)?;
        self.record("checkpoint", path, "before_write")?;
        self.begin_effect(&format!("write:{path}"))?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        atomic_write(&target, bytes)?;
        self.finish_effect(
            "write",
            path,
            &format!("{}|{}", optional_digest(&before), sha256_digest(bytes)),
            &[path],
        )
    }

    pub fn delete(&mut self, path: &str) -> Result<(), String> {
        self.require_active()?;
        let target = self.authorize_existing(path, &self.state.policy.allowed_write_paths)?;
        let before = file_digest(&target)?;
        self.record("checkpoint", path, "before_delete")?;
        self.begin_effect(&format!("delete:{path}"))?;
        if target.is_dir() {
            fs::remove_dir(&target)
        } else {
            fs::remove_file(&target)
        }
        .map_err(|e| format!("delete failed: {e}"))?;
        self.finish_effect(
            "delete",
            path,
            &format!("{}|-", optional_digest(&before)),
            &[path],
        )
    }

    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
        self.require_active()?;
        let source = self.authorize_existing(from, &self.state.policy.allowed_write_paths)?;
        let target = self.authorize_write(to)?;
        let before = file_digest(&source)?;
        self.record("checkpoint", &format!("{from}->{to}"), "before_rename")?;
        self.begin_effect(&format!("rename:{from}->{to}"))?;
        fs::rename(source, target).map_err(|e| format!("rename failed: {e}"))?;
        self.finish_effect(
            "rename",
            &format!("{from}->{to}"),
            &format!("{}|{}", optional_digest(&before), optional_digest(&before)),
            &[from, to],
        )
    }

    #[cfg(unix)]
    pub fn chmod(&mut self, path: &str, mode: u32) -> Result<(), String> {
        use std::os::unix::fs::PermissionsExt;
        self.require_active()?;
        if mode & !0o777 != 0 {
            return Err("special permission bits are forbidden".into());
        }
        let target = self.authorize_existing(path, &self.state.policy.allowed_write_paths)?;
        let digest = file_digest(&target)?;
        self.record("checkpoint", path, "before_chmod")?;
        self.begin_effect(&format!("chmod:{path}"))?;
        fs::set_permissions(target, fs::Permissions::from_mode(mode))
            .map_err(|e| format!("chmod failed: {e}"))?;
        self.finish_effect(
            "chmod",
            path,
            &format!("{}|{}", optional_digest(&digest), optional_digest(&digest)),
            &[path],
        )
    }

    /// Validate authority for an external command. Execution is deliberately
    /// separate so callers cannot confuse an allowlist entry with isolation.
    pub fn authorize_external(&mut self, program: &str, network: bool) -> Result<(), String> {
        self.require_active()?;
        let _ = network;
        self.record("command", program, "denied")?;
        Err("portable v0 denies external commands and network; a verified sandbox executor is required".into())
    }

    pub fn record_artifact(&mut self, path: &str) -> Result<(), String> {
        let target = self.authorize_existing(path, &self.state.policy.allowed_read_paths)?;
        let digest = file_digest(&target)?.ok_or_else(|| "artifact is not a file".to_string())?;
        self.state.artifacts.insert(path.into());
        self.record("artifact", path, &digest)
    }

    /// Restore tracked content to the exact base commit and remove only
    /// untracked files inside the isolated worktree. The source checkout is
    /// never addressed by these commands.
    pub fn abort(&mut self) -> Result<(), String> {
        if matches!(
            self.state.phase,
            TransactionPhase::Committed | TransactionPhase::Aborted
        ) {
            return Err("transaction is already terminal".into());
        }
        let root = PathBuf::from(&self.state.worktree);
        git(&root, &["reset", "--hard", &self.state.base_sha])?;
        git(
            &root,
            &[
                "clean",
                "-fd",
                "--exclude",
                STATE_FILE,
                "--exclude",
                LEASE_FILE,
            ],
        )?;
        self.state.phase = TransactionPhase::Aborted;
        self.state.rollback_result = Some("restored_to_base_sha".into());
        self.state.pending_effect = None;
        self.state.authorized_path_fingerprints =
            authorized_path_fingerprint_cache(&root, &self.state.policy)?;
        self.state.workspace_fingerprint = policy_scoped_fingerprint(
            &root,
            &self.state.policy,
            &self.state.authorized_path_fingerprints,
        )?;
        self.state.unscoped_workspace_fingerprint =
            unscoped_workspace_fingerprint(&root, &self.state.policy)?;
        self.record("rollback", &self.state.base_sha.clone(), "succeeded")
    }

    pub fn commit_local(&mut self) -> Result<(), String> {
        self.require_active()?;
        self.record("checkpoint", "delivery", "before_local_commit")?;
        self.state.phase = TransactionPhase::Committed;
        self.record("lifecycle", "transaction", "committed")
    }

    /// Operations which alter protected history or governance can never be
    /// granted by this Class-2 primitive.
    pub fn reject_delivery_operation(operation: &str) -> Result<(), String> {
        match operation {
            "force_push" | "push_protected_branch" | "self_approve" | "edit_policy" => Err(
                format!("delivery operation {operation} requires separate Class-3 authority"),
            ),
            _ => Ok(()),
        }
    }

    pub fn execution_audit(&self) -> ExecutionTransactionAudit {
        let digest = |value: &str| sha256_digest(value.as_bytes());
        let checkpoints = self
            .state
            .events
            .iter()
            .filter(|e| e.operation == "checkpoint")
            .map(|e| AuditCheckpoint {
                sequence: e.sequence,
                kind: if e.result == "created" {
                    "initial".into()
                } else if e.subject == "delivery" {
                    "before_delivery".into()
                } else {
                    "before_write".into()
                },
                tree_sha: self.state.checkpoint_tree.clone(),
                created_before: e.subject.clone(),
            })
            .collect();
        let reads = self
            .state
            .events
            .iter()
            .filter(|e| e.operation == "read" && e.result.starts_with("sha256:"))
            .map(|e| FileObservation {
                sequence: e.sequence,
                path: e.subject.clone(),
                digest: e.result.clone(),
            })
            .collect();
        let writes = self
            .state
            .events
            .iter()
            .filter(|e| {
                matches!(
                    e.operation.as_str(),
                    "write" | "delete" | "rename" | "chmod"
                )
            })
            .map(|e| {
                let (path, destination) = if e.operation == "rename" {
                    let mut split = e.subject.splitn(2, "->");
                    (
                        split.next().unwrap_or("").into(),
                        split.next().map(str::to_string),
                    )
                } else {
                    (e.subject.clone(), None)
                };
                let mut digests = e.result.splitn(2, '|');
                let before_digest = parse_optional_digest(digests.next());
                let after_digest = parse_optional_digest(digests.next());
                AuditWrite {
                    sequence: e.sequence,
                    operation: if e.operation == "write" && before_digest.is_none() {
                        "create".into()
                    } else if e.operation == "write" {
                        "modify".into()
                    } else {
                        e.operation.clone()
                    },
                    path,
                    destination,
                    before_digest,
                    after_digest,
                }
            })
            .collect();
        let commands = self
            .state
            .events
            .iter()
            .filter(|e| e.operation == "command")
            .map(|e| AuditCommand {
                sequence: e.sequence,
                argv: vec![e.subject.clone()],
                capabilities: vec![],
                network_hosts: vec![],
                outcome: "denied".into(),
                exit_code: 126,
                stdout_digest: digest(""),
                stderr_digest: digest(""),
            })
            .collect();
        let artifacts = self
            .state
            .artifacts
            .iter()
            .enumerate()
            .map(|(i, path)| AuditArtifact {
                sequence: self.state.events.len() as u64 + i as u64,
                path: path.clone(),
                digest: self
                    .state
                    .events
                    .iter()
                    .rev()
                    .find(|event| event.operation == "artifact" && event.subject == *path)
                    .map(|event| event.result.clone())
                    .unwrap_or_else(|| digest(path)),
                kind: "file".into(),
            })
            .collect();
        let attempted = self.state.rollback_result.is_some();
        let rollback = RollbackAudit {
            attempted,
            result: if attempted {
                "succeeded".into()
            } else {
                "not_required".into()
            },
            restored_checkpoint: attempted.then_some(0),
            source_checkout_untouched: source_fingerprint(Path::new(&self.state.source_checkout))
                .is_ok_and(|fingerprint| fingerprint == self.state.source_fingerprint),
        };
        let status = match self.state.phase {
            TransactionPhase::Active => "running",
            TransactionPhase::Interrupted => "interrupted",
            TransactionPhase::Committed => "succeeded",
            TransactionPhase::Aborted => "rolled_back",
        }
        .into();
        let journal = serde_json::to_vec(&self.state.events).unwrap_or_default();
        let workspace_matches =
            workspace_matches_state(&self.root(), &self.state).is_ok_and(|matches| matches);
        ExecutionTransactionAudit {
            schema_version: "axiom.execution_transaction.v0".into(),
            transaction_id: self.state.transaction_id.clone(),
            task_contract_digest: self.state.task_contract_digest.clone(),
            policy_digest: self.state.policy_digest.clone(),
            base_sha: self.state.base_sha.clone(),
            branch: self.state.branch.clone(),
            status,
            checkpoints,
            reads,
            writes,
            commands,
            artifacts,
            rollback,
            recovery: RecoveryAudit {
                resumable: matches!(
                    self.state.phase,
                    TransactionPhase::Active | TransactionPhase::Interrupted
                ) && self.state.pending_effect.is_none()
                    && workspace_matches,
                rollback_safe: !matches!(self.state.phase, TransactionPhase::Committed),
                next_sequence: self.state.events.len() as u64,
                journal_digest: sha256_digest(&journal),
            },
        }
    }

    pub fn deterministic_audit_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&self.execution_audit()).map_err(|e| e.to_string())
    }

    fn require_active(&self) -> Result<(), String> {
        if self.state.phase == TransactionPhase::Active {
            Ok(())
        } else {
            Err("transaction is not active".into())
        }
    }

    fn root(&self) -> PathBuf {
        PathBuf::from(&self.state.worktree)
    }

    fn authorize_write(&self, path: &str) -> Result<PathBuf, String> {
        validate_relative(path)?;
        if !self.state.policy.allowed_write_paths.contains(path) {
            return Err("write path is outside task scope".into());
        }
        canonical_target(&self.root(), path)
    }

    fn authorize_existing(
        &self,
        path: &str,
        allowed: &BTreeSet<String>,
    ) -> Result<PathBuf, String> {
        validate_relative(path)?;
        if !allowed.contains(path) {
            return Err("path is outside task scope".into());
        }
        let target = canonical_target(&self.root(), path)?;
        target
            .canonicalize()
            .map_err(|e| format!("path does not exist: {e}"))
            .and_then(|p| {
                if p.starts_with(self.root()) {
                    Ok(p)
                } else {
                    Err("symlink escapes transaction worktree".into())
                }
            })
    }

    fn record(&mut self, operation: &str, subject: &str, result: &str) -> Result<(), String> {
        self.state.events.push(AuditEvent {
            sequence: self.state.events.len() as u64,
            operation: operation.into(),
            subject: redact(subject),
            result: redact(result),
        });
        self.persist()
    }

    fn persist(&mut self) -> Result<(), String> {
        let expected_owner = self.state.owner_epoch.clone();
        let expected_generation = self.state.generation;
        let expected = if expected_generation == 0 && !self.state_path.exists() {
            None
        } else {
            Some((expected_owner.as_str(), expected_generation))
        };
        persist_state(&self.state_path, &mut self.state, expected)
    }

    fn begin_effect(&mut self, effect: &str) -> Result<(), String> {
        if self.state.pending_effect.is_some() {
            return Err("another filesystem effect is pending".into());
        }
        self.state.pending_effect = Some(effect.into());
        self.persist()
    }

    fn finish_effect(
        &mut self,
        operation: &str,
        subject: &str,
        result: &str,
        changed_paths: &[&str],
    ) -> Result<(), String> {
        self.record(operation, subject, result)?;
        self.state.pending_effect = None;
        let root = self.root();
        update_authorized_path_fingerprint_cache(
            &root,
            &self.state.policy,
            &mut self.state.authorized_path_fingerprints,
            changed_paths,
        )?;
        let head = git(&root, &["rev-parse", "HEAD"])?;
        self.state.workspace_fingerprint = policy_scoped_fingerprint_from_head(
            head.trim(),
            &self.state.policy,
            &self.state.authorized_path_fingerprints,
        )?;
        self.persist()
    }
}

fn validate_policy(policy: &WorkspacePolicy) -> Result<(), String> {
    for path in policy
        .allowed_read_paths
        .iter()
        .chain(&policy.allowed_write_paths)
    {
        validate_relative(path)?;
    }
    for command in &policy.allowed_commands {
        if command.contains('/') || command.trim().is_empty() {
            return Err("commands must be canonical program names".into());
        }
    }
    Ok(())
}

fn validate_sha(sha: &str) -> Result<(), String> {
    if sha.len() == 40
        && sha
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err("base SHA must be a full 40-character hexadecimal commit id".into())
    }
}

fn validate_digest(digest: &str) -> Result<(), String> {
    if digest.len() == 71
        && digest.starts_with("sha256:")
        && digest[7..]
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err("digest must use sha256:<64 lowercase hex> form".into())
    }
}

fn validate_owner_epoch(epoch: &str) -> Result<(), String> {
    if epoch.is_empty() || epoch.len() > 160 || epoch.contains(char::is_whitespace) {
        return Err("transaction owner epoch is invalid".into());
    }
    Ok(())
}

fn new_owner_epoch() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = OWNER_EPOCH_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("owner-{}-{nanos}-{counter}", std::process::id())
}

fn sha256_digest(bytes: &[u8]) -> String {
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
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes(word.try_into().expect("SHA word"));
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
    format!(
        "sha256:{}",
        h.iter()
            .map(|word| format!("{word:08x}"))
            .collect::<String>()
    )
}

fn validate_relative(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if path.is_empty()
        || p.is_absolute()
        || p.components().any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("path must be a normalized relative path without traversal".into());
    }
    if path == STATE_FILE
        || path == LEASE_FILE
        || path.starts_with(".git")
        || path.starts_with(".codex/policies/")
    {
        return Err("protected transaction or policy path".into());
    }
    Ok(())
}

fn canonical_target(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let target = root.join(relative);
    let mut ancestor = target.as_path();
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| "invalid target".to_string())?;
    }
    let canonical = ancestor
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize target ancestor: {e}"))?;
    if !canonical.starts_with(root) {
        return Err("path or symlink escapes transaction worktree".into());
    }
    Ok(target)
}

fn file_digest(path: &Path) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }
    if !path.is_file() {
        return Ok(None);
    }
    fs::read(path)
        .map(|bytes| Some(sha256_digest(&bytes)))
        .map_err(|e| format!("cannot hash {}: {e}", path.display()))
}

fn authorized_path_fingerprint_cache(
    root: &Path,
    policy: &WorkspacePolicy,
) -> Result<BTreeMap<String, String>, String> {
    policy_paths(policy)
        .into_iter()
        .map(|path| {
            let fingerprint = path_fingerprint(root, &path)?;
            Ok((path, fingerprint))
        })
        .collect()
}

fn update_authorized_path_fingerprint_cache(
    root: &Path,
    policy: &WorkspacePolicy,
    cache: &mut BTreeMap<String, String>,
    changed_paths: &[&str],
) -> Result<(), String> {
    for path in policy_paths(policy) {
        if !cache.contains_key(&path)
            || changed_paths
                .iter()
                .any(|changed| paths_overlap(&path, changed))
        {
            cache.insert(path.clone(), path_fingerprint(root, &path)?);
        }
    }
    Ok(())
}

fn policy_paths(policy: &WorkspacePolicy) -> BTreeSet<String> {
    policy
        .allowed_read_paths
        .iter()
        .chain(&policy.allowed_write_paths)
        .cloned()
        .collect()
}

fn paths_overlap(left: &str, right: &str) -> bool {
    left == right
        || left.starts_with(&format!("{right}/"))
        || right.starts_with(&format!("{left}/"))
}

fn policy_scoped_fingerprint(
    root: &Path,
    policy: &WorkspacePolicy,
    cache: &BTreeMap<String, String>,
) -> Result<String, String> {
    let head = git(root, &["rev-parse", "HEAD"])?;
    policy_scoped_fingerprint_from_head(head.trim(), policy, cache)
}

fn policy_scoped_fingerprint_from_head(
    head: &str,
    policy: &WorkspacePolicy,
    cache: &BTreeMap<String, String>,
) -> Result<String, String> {
    let policy_bytes = serde_json::to_vec(policy).map_err(|e| e.to_string())?;
    let rows = cache
        .iter()
        .map(|(path, fingerprint)| format!("{path}\0{fingerprint}"))
        .collect::<Vec<_>>();
    Ok(sha256_digest(
        format!(
            "{head}\0{}\0{}",
            sha256_digest(&policy_bytes),
            rows.join("\n")
        )
        .as_bytes(),
    ))
}

fn path_fingerprint(root: &Path, relative: &str) -> Result<String, String> {
    let target = root.join(relative);
    let mut rows = Vec::new();
    match fs::symlink_metadata(&target) {
        Ok(_) => collect_entry_rows(root, &target, &mut rows)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            rows.push(format!("M\0{relative}"));
        }
        Err(error) => return Err(format!("cannot inspect {}: {error}", target.display())),
    }
    Ok(sha256_digest(rows.join("\n").as_bytes()))
}

fn unscoped_workspace_fingerprint(root: &Path, policy: &WorkspacePolicy) -> Result<String, String> {
    let scopes = policy_paths(policy);
    let mut rows = Vec::new();
    collect_tree_rows(root, root, &mut rows, &|relative| {
        relative == Path::new(".git")
            || relative == Path::new(STATE_FILE)
            || relative == Path::new(LEASE_FILE)
            || scopes
                .iter()
                .any(|scope| path_is_within_scope(relative, scope))
    })?;
    Ok(sha256_digest(rows.join("\n").as_bytes()))
}

fn workspace_matches_state(root: &Path, state: &TransactionState) -> Result<bool, String> {
    let current_cache = authorized_path_fingerprint_cache(root, &state.policy)?;
    let current_scoped = policy_scoped_fingerprint(root, &state.policy, &current_cache)?;
    let current_unscoped = unscoped_workspace_fingerprint(root, &state.policy)?;
    Ok(current_cache == state.authorized_path_fingerprints
        && current_scoped == state.workspace_fingerprint
        && current_unscoped == state.unscoped_workspace_fingerprint)
}

fn path_is_within_scope(path: &Path, scope: &str) -> bool {
    let scope = Path::new(scope);
    path == scope || path.starts_with(scope)
}

fn source_fingerprint(root: &Path) -> Result<String, String> {
    let head = git(root, &["rev-parse", "HEAD"])?;
    Ok(sha256_digest(
        format!("{}\0{}", head.trim(), tree_fingerprint(root)?).as_bytes(),
    ))
}

fn tree_fingerprint(root: &Path) -> Result<String, String> {
    let mut rows = Vec::new();
    collect_tree_rows(root, root, &mut rows, &|relative| {
        relative == Path::new(".git")
            || relative == Path::new(STATE_FILE)
            || relative == Path::new(LEASE_FILE)
    })?;
    Ok(sha256_digest(rows.join("\n").as_bytes()))
}

fn collect_tree_rows<F>(
    root: &Path,
    path: &Path,
    rows: &mut Vec<String>,
    skip: &F,
) -> Result<(), String>
where
    F: Fn(&Path) -> bool,
{
    let mut entries = fs::read_dir(path)
        .map_err(|e| format!("cannot inspect {}: {e}", path.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let child = entry.path();
        let relative = child.strip_prefix(root).map_err(|e| e.to_string())?;
        if skip(relative) {
            continue;
        }
        append_entry_row(root, &child, rows)?;
        if fs::symlink_metadata(&child)
            .map_err(|e| e.to_string())?
            .is_dir()
        {
            collect_tree_rows(root, &child, rows, skip)?;
        }
    }
    Ok(())
}

fn collect_entry_rows(root: &Path, path: &Path, rows: &mut Vec<String>) -> Result<(), String> {
    append_entry_row(root, path, rows)?;
    if fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .is_dir()
    {
        collect_tree_rows(root, path, rows, &|_| false)?;
    }
    Ok(())
}

fn append_entry_row(root: &Path, child: &Path, rows: &mut Vec<String>) -> Result<(), String> {
    let relative = child.strip_prefix(root).map_err(|e| e.to_string())?;
    let metadata = fs::symlink_metadata(child).map_err(|e| e.to_string())?;
    let mode = metadata_mode(&metadata);
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(child).map_err(|e| e.to_string())?;
        rows.push(format!(
            "L\0{}\0{mode:o}\0{}",
            normalized(relative),
            normalized(&target)
        ));
    } else if metadata.is_dir() {
        rows.push(format!("D\0{}\0{mode:o}", normalized(relative)));
    } else if metadata.is_file() {
        let digest = sha256_digest(&fs::read(child).map_err(|e| e.to_string())?);
        rows.push(format!("F\0{}\0{mode:o}\0{digest}", normalized(relative)));
    } else {
        return Err(format!("unsupported filesystem object {}", child.display()));
    }
    Ok(())
}

#[cfg(unix)]
fn metadata_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    metadata.mode()
}

#[cfg(not(unix))]
fn metadata_mode(metadata: &fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

fn optional_digest(digest: &Option<String>) -> &str {
    digest.as_deref().unwrap_or("-")
}

fn parse_optional_digest(value: Option<&str>) -> Option<String> {
    value.filter(|value| *value != "-").map(str::to_owned)
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("cannot execute git: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git operation failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("git output is not UTF-8: {e}"))
}

fn acquire_lease(path: &Path) -> Result<File, String> {
    let parent = path
        .parent()
        .ok_or_else(|| "transaction lease path has no parent".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("cannot create transaction lease parent: {e}"))?;
    let file = open_lease(path).map_err(|e| format!("cannot open transaction lease: {e}"))?;
    lock_lease(&file).map_err(|e| format!("transaction lease unavailable: {e}"))?;
    Ok(file)
}

#[cfg(unix)]
fn open_lease(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
}

#[cfg(windows)]
fn open_lease(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(not(any(unix, windows)))]
fn open_lease(_path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "transaction leases require platform file-lock support",
    ))
}

#[cfg(unix)]
fn lock_lease(file: &File) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;

    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn lock_lease(file: &File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use std::ptr;

    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        event: *mut std::ffi::c_void,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LockFileEx(
            file: *mut std::ffi::c_void,
            flags: u32,
            reserved: u32,
            bytes_low: u32,
            bytes_high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
    }

    const LOCKFILE_EXCLUSIVE_LOCK: u32 = 0x0000_0002;
    const LOCKFILE_FAIL_IMMEDIATELY: u32 = 0x0000_0001;
    let mut overlapped = Overlapped {
        internal: 0,
        internal_high: 0,
        offset: 0,
        offset_high: 0,
        event: ptr::null_mut(),
    };
    if unsafe {
        LockFileEx(
            file.as_raw_handle(),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            u32::MAX,
            u32::MAX,
            &mut overlapped,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
fn lock_lease(_file: &File) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "transaction leases require platform file-lock support",
    ))
}

fn persist_state(
    path: &Path,
    state: &mut TransactionState,
    expected: Option<(&str, u64)>,
) -> Result<(), String> {
    match expected {
        Some((expected_owner, expected_generation)) => {
            let bytes = fs::read(path).map_err(|e| {
                format!("cannot read durable transaction state for compare-and-swap: {e}")
            })?;
            let durable: TransactionState = serde_json::from_slice(&bytes).map_err(|e| {
                format!("invalid durable transaction state for compare-and-swap: {e}")
            })?;
            let durable_checksum = state_checksum(&durable)?;
            if durable.checksum != durable_checksum {
                return Err("transaction state checksum mismatch during compare-and-swap".into());
            }
            if durable.transaction_id != state.transaction_id
                || durable.owner_epoch != expected_owner
                || durable.generation != expected_generation
            {
                return Err(
                    "transaction state owner/generation conflict; refusing to merge stale state"
                        .into(),
                );
            }
        }
        None if path.exists() => {
            return Err("transaction state appeared during creation".into());
        }
        None => {}
    }

    state.generation = match expected {
        Some((_, generation)) => generation
            .checked_add(1)
            .ok_or_else(|| "transaction state generation overflow".to_string())?,
        None => 1,
    };
    state.checksum.clear();
    state.checksum = state_checksum(state)?;
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    atomic_write(path, &bytes)
}

fn state_checksum(state: &TransactionState) -> Result<String, String> {
    let mut copy = state.clone();
    copy.checksum.clear();
    let bytes = serde_json::to_vec(&copy).map_err(|e| e.to_string())?;
    Ok(sha256_digest(&bytes))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "state path has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("transaction");
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    for _ in 0..32 {
        let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = parent.join(format!(".{name}.{pid}.{nanos}.{counter}.tmp"));
        let mut file = match OpenOptions::new().create_new(true).write(true).open(&tmp) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "cannot create unique transaction temp file: {error}"
                ))
            }
        };
        let result = file
            .write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string());
        drop(file);
        if let Err(error) = result {
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }
        let result = fs::rename(&tmp, path)
            .and_then(|()| File::open(parent).and_then(|file| file.sync_all()))
            .map_err(|e| e.to_string());
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        return result;
    }
    Err("could not allocate a unique transaction temp file".into())
}

fn normalized(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn redact(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("secret://")
        || lower.starts_with("secret-ref:")
        || lower.contains("credential=")
        || lower.contains("token=")
        || lower.contains("password=")
        || lower.contains("authorization:")
        || lower.contains(&["github_", "pat_"].concat())
        || lower.contains(&["gh", "p_"].concat())
        || lower.contains("bearer ")
    {
        "[secret-reference-redacted]".into()
    } else {
        value.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fixture() -> (TempDir, PathBuf, String) {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("source");
        fs::create_dir(&source).unwrap();
        git(&source, &["init", "-q"]).unwrap();
        git(&source, &["config", "user.email", "test@example.invalid"]).unwrap();
        git(&source, &["config", "user.name", "Test"]).unwrap();
        fs::write(source.join("allowed.txt"), b"original").unwrap();
        fs::write(source.join("outside.txt"), b"owned").unwrap();
        git(&source, &["add", "allowed.txt", "outside.txt"]).unwrap();
        git(
            &source,
            &["-c", "commit.gpgsign=false", "commit", "-qm", "base"],
        )
        .unwrap();
        let sha = git(&source, &["rev-parse", "HEAD"]).unwrap().trim().into();
        (dir, source, sha)
    }

    fn policy() -> WorkspacePolicy {
        WorkspacePolicy {
            allowed_read_paths: BTreeSet::from(["allowed.txt".into()]),
            allowed_write_paths: BTreeSet::from(["allowed.txt".into(), "new.txt".into()]),
            ..WorkspacePolicy::default()
        }
    }

    #[test]
    fn isolated_abort_preserves_dirty_source() {
        let (repo, source, sha) = fixture();
        fs::write(source.join("outside.txt"), b"user dirty").unwrap();
        let worktree = repo.path().join("txn");
        let mut txn = TransactionalWorkspace::create(&source, &worktree, &sha, policy()).unwrap();
        txn.write("allowed.txt", b"changed").unwrap();
        txn.write("new.txt", b"new").unwrap();
        txn.abort().unwrap();
        assert_eq!(fs::read(worktree.join("allowed.txt")).unwrap(), b"original");
        assert!(!worktree.join("new.txt").exists());
        assert_eq!(fs::read(source.join("outside.txt")).unwrap(), b"user dirty");
    }

    #[test]
    fn traversal_and_unapproved_commands_fail_closed() {
        let (repo, source, sha) = fixture();
        let worktree = repo.path().join("txn");
        let mut txn = TransactionalWorkspace::create(&source, &worktree, &sha, policy()).unwrap();
        assert!(txn.write("../outside.txt", b"bad").is_err());
        assert!(txn.write("outside.txt", b"bad").is_err());
        assert!(txn.authorize_external("sh", false).is_err());
        assert!(txn.authorize_external("curl", true).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_fails_closed() {
        use std::os::unix::fs::symlink;
        let (repo, source, sha) = fixture();
        let worktree = repo.path().join("txn");
        let mut scoped = policy();
        scoped.allowed_write_paths.insert("link/stolen.txt".into());
        let mut txn = TransactionalWorkspace::create(&source, &worktree, &sha, scoped).unwrap();
        symlink(&source, worktree.join("link")).unwrap();
        assert!(txn.write("link/stolen.txt", b"bad").is_err());
    }

    #[test]
    fn crash_recovery_verifies_state_and_can_resume() {
        let (repo, source, sha) = fixture();
        let worktree = repo.path().join("txn");
        let mut txn = TransactionalWorkspace::create(&source, &worktree, &sha, policy()).unwrap();
        txn.mark_interrupted().unwrap();
        drop(txn);
        let mut recovered = TransactionalWorkspace::recover(&worktree).unwrap();
        assert_eq!(recovered.state().phase, TransactionPhase::Interrupted);
        recovered.resume().unwrap();
        recovered.write("new.txt", b"resumed").unwrap();
    }

    #[test]
    fn crash_during_effect_requires_rollback() {
        let (repo, source, sha) = fixture();
        let worktree = repo.path().join("txn");
        let mut txn = TransactionalWorkspace::create(&source, &worktree, &sha, policy()).unwrap();
        txn.state.pending_effect = Some("write:allowed.txt".into());
        txn.state.phase = TransactionPhase::Interrupted;
        txn.persist().unwrap();
        drop(txn);
        let mut recovered = TransactionalWorkspace::recover(&worktree).unwrap();
        assert!(!recovered.execution_audit().recovery.resumable);
        assert!(recovered.resume().is_err());
        recovered.abort().unwrap();
        assert_eq!(recovered.state.phase, TransactionPhase::Aborted);
    }

    #[test]
    fn recovery_rejects_unjournaled_workspace_mutation() {
        let (repo, source, sha) = fixture();
        let worktree = repo.path().join("txn");
        let mut txn = TransactionalWorkspace::create(&source, &worktree, &sha, policy()).unwrap();
        txn.mark_interrupted().unwrap();
        drop(txn);
        fs::write(worktree.join("allowed.txt"), b"unjournaled").unwrap();
        assert!(TransactionalWorkspace::recover(&worktree).is_err());
    }

    #[test]
    fn recovery_rejects_an_unjournaled_descendant_head() {
        let (repo, source, sha) = fixture();
        let worktree = repo.path().join("txn");
        let mut txn = TransactionalWorkspace::create(&source, &worktree, &sha, policy()).unwrap();
        txn.write("allowed.txt", b"changed").unwrap();
        git(&worktree, &["add", "allowed.txt"]).unwrap();
        git(
            &worktree,
            &["-c", "commit.gpgsign=false", "commit", "-qm", "candidate"],
        )
        .unwrap();
        drop(txn);

        assert!(TransactionalWorkspace::recover(&worktree).is_err());
    }

    #[test]
    fn audit_is_deterministic_and_redacts_secret_references() {
        let (repo, source, sha) = fixture();
        let worktree = repo.path().join("txn");
        let mut txn = TransactionalWorkspace::create(&source, &worktree, &sha, policy()).unwrap();
        assert!(txn
            .authorize_external("secret://broker/key", false)
            .is_err());
        let first = txn.deterministic_audit_json().unwrap();
        let second = txn.deterministic_audit_json().unwrap();
        assert_eq!(first, second);
        assert!(!first.contains("broker/key"));
        assert!(first.contains("[secret-reference-redacted]"));
    }

    #[test]
    fn class_three_delivery_mutations_are_rejected() {
        for op in [
            "force_push",
            "push_protected_branch",
            "self_approve",
            "edit_policy",
        ] {
            assert!(TransactionalWorkspace::reject_delivery_operation(op).is_err());
        }
    }
}
