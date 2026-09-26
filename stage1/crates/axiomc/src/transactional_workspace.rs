//! Durable, fail-closed workspace transactions for unattended agents.
//!
//! A transaction owns a detached Git worktree at an exact commit. All file
//! mutations are built-ins so their canonical targets can be checked before
//! use. External commands and network remain unavailable until a caller has
//! independently established a verified sandbox.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::ffi::CString;
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::time::{SystemTime, UNIX_EPOCH};

const STATE_FILE: &str = ".axiom-transaction.json";
const LEASE_FILE: &str = ".axiom-transaction.lock";
const AUDIT_SCHEMA: &str = "axiom.transactional_workspace.v0";
#[cfg(any(not(unix), test))]
const ATOMIC_TEMP_ATTEMPTS: u64 = 128;
#[cfg(any(not(unix), test))]
static ATOMIC_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static OWNER_EPOCH_COUNTER: AtomicU64 = AtomicU64::new(0);

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
    #[cfg(unix)]
    root_dir: File,
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
        #[cfg(unix)]
        let root_dir = open_root_directory(&canonical_worktree)?;
        let lease = acquire_lease(
            &canonical_worktree.join(LEASE_FILE),
            #[cfg(unix)]
            &root_dir,
        )?;
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
            #[cfg(unix)]
            root_dir,
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
        #[cfg(unix)]
        let root_dir = open_root_directory(&root)?;
        let lease = acquire_lease(
            &root.join(LEASE_FILE),
            #[cfg(unix)]
            &root_dir,
        )?;
        let bytes = {
            #[cfg(unix)]
            {
                secure_read(&root_dir, STATE_FILE)
                    .map_err(|error| format!("cannot read transaction state: {error}"))?
            }
            #[cfg(not(unix))]
            {
                fs::read(&state_path)
                    .map_err(|error| format!("cannot read transaction state: {error}"))?
            }
        };
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
            #[cfg(unix)]
            &root_dir,
        )?;
        Ok(Self {
            state_path,
            state,
            _lease: lease,
            #[cfg(unix)]
            root_dir,
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
        let result = self.read_authorized(path, &target);
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
        let creates_structure = self.write_creates_structure(path, &target)?;
        let before = self.file_digest_authorized(path, &target)?;
        self.record("checkpoint", path, "before_write")?;
        self.begin_effect(&format!("write:{path}"))?;
        self.write_authorized(path, &target, bytes)?;
        self.finish_effect(
            "write",
            path,
            &format!("{}|{}", optional_digest(&before), sha256_digest(bytes)),
            &[path],
            creates_structure,
        )
    }

    pub fn delete(&mut self, path: &str) -> Result<(), String> {
        self.require_active()?;
        let target = self.authorize_existing(path, &self.state.policy.allowed_write_paths)?;
        let before = self.file_digest_authorized(path, &target)?;
        self.record("checkpoint", path, "before_delete")?;
        self.begin_effect(&format!("delete:{path}"))?;
        self.delete_authorized(path, &target)?;
        // An authorized delete removes an existing scoped file (directory
        // subjects fail closed at the before-digest) and cannot create
        // structure, so the unscoped baseline never changes here.
        self.finish_effect(
            "delete",
            path,
            &format!("{}|-", optional_digest(&before)),
            &[path],
            false,
        )
    }

    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
        self.require_active()?;
        let source = self.authorize_existing(from, &self.state.policy.allowed_write_paths)?;
        let target = self.authorize_write(to)?;
        let before = self.file_digest_authorized(from, &source)?;
        self.record("checkpoint", &format!("{from}->{to}"), "before_rename")?;
        self.begin_effect(&format!("rename:{from}->{to}"))?;
        self.rename_authorized(from, &source, to, &target)?;
        // Both rename endpoints are scoped paths, and the anchored rename
        // requires an existing target parent, so unscoped structure is
        // unchanged.
        self.finish_effect(
            "rename",
            &format!("{from}->{to}"),
            &format!("{}|{}", optional_digest(&before), optional_digest(&before)),
            &[from, to],
            false,
        )
    }

    #[cfg(unix)]
    pub fn chmod(&mut self, path: &str, mode: u32) -> Result<(), String> {
        self.require_active()?;
        if mode & !0o777 != 0 {
            return Err("special permission bits are forbidden".into());
        }
        let target = self.authorize_existing(path, &self.state.policy.allowed_write_paths)?;
        let digest = self.file_digest_authorized(path, &target)?;
        self.record("checkpoint", path, "before_chmod")?;
        self.begin_effect(&format!("chmod:{path}"))?;
        self.chmod_authorized(path, &target, mode)?;
        self.finish_effect(
            "chmod",
            path,
            &format!("{}|{}", optional_digest(&digest), optional_digest(&digest)),
            &[path],
            false,
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
        let digest = self
            .file_digest_authorized(path, &target)?
            .ok_or_else(|| "artifact is not a file".to_string())?;
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
        #[cfg(unix)]
        let root = descriptor_path(&self.root_dir);
        #[cfg(not(unix))]
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
        #[cfg(unix)]
        validate_anchored_write_path(&self.root_dir, path)?;
        #[cfg(not(unix))]
        {
            return Err("transactional path authority is unavailable on this platform".into());
        }
        #[cfg(unix)]
        {
            Ok(self.root().join(path))
        }
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
        #[cfg(unix)]
        {
            validate_anchored_existing_path(&self.root_dir, path)?;
            return Ok(self.root().join(path));
        }
        #[cfg(not(unix))]
        Err("transactional path authority is unavailable on this platform".into())
    }

    fn read_authorized(&self, path: &str, _target: &Path) -> Result<Vec<u8>, String> {
        #[cfg(unix)]
        {
            return secure_read(&self.root_dir, path);
        }
        #[cfg(not(unix))]
        fs::read(_target).map_err(|e| format!("read failed: {e}"))
    }

    fn file_digest_authorized(&self, path: &str, _target: &Path) -> Result<Option<String>, String> {
        #[cfg(unix)]
        {
            return secure_file_digest(&self.root_dir, path);
        }
        #[cfg(not(unix))]
        file_digest(_target)
    }

    fn write_authorized(&self, path: &str, _target: &Path, bytes: &[u8]) -> Result<(), String> {
        #[cfg(unix)]
        {
            return secure_write(&self.root_dir, path, bytes);
        }
        #[cfg(not(unix))]
        {
            if let Some(parent) = _target.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            atomic_write(_target, bytes)
        }
    }

    fn delete_authorized(&self, path: &str, _target: &Path) -> Result<(), String> {
        #[cfg(unix)]
        {
            return secure_delete(&self.root_dir, path);
        }
        #[cfg(not(unix))]
        {
            if _target.is_dir() {
                fs::remove_dir(_target)
            } else {
                fs::remove_file(_target)
            }
            .map_err(|e| format!("delete failed: {e}"))
        }
    }

    fn rename_authorized(
        &self,
        from: &str,
        _source: &Path,
        to: &str,
        _target: &Path,
    ) -> Result<(), String> {
        #[cfg(unix)]
        {
            return secure_rename(&self.root_dir, from, to);
        }
        #[cfg(not(unix))]
        fs::rename(_source, _target).map_err(|e| format!("rename failed: {e}"))
    }

    #[cfg(unix)]
    fn chmod_authorized(&self, path: &str, _target: &Path, mode: u32) -> Result<(), String> {
        secure_chmod(&self.root_dir, path, mode)
    }

    /// Reports whether an authorized write must first create missing parent
    /// directories. Created ancestors are unscoped workspace structure, so
    /// the caller re-journals the unscoped baseline after such an effect.
    fn write_creates_structure(&self, path: &str, target: &Path) -> Result<bool, String> {
        #[cfg(unix)]
        {
            let _ = target;
            return creates_missing_ancestors(&self.root_dir, path);
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Ok(missing_ancestor_dirs(target))
        }
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
        persist_state(
            &self.state_path,
            &mut self.state,
            expected,
            #[cfg(unix)]
            &self.root_dir,
        )
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
        structure_changed: bool,
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
        if structure_changed {
            // Authorized writes can create parent directories, which belong
            // to the unscoped workspace baseline. Re-journal that baseline
            // after structure-creating effects so interrupt/resume and crash
            // recovery compare against post-effect truth instead of
            // rejecting the transaction's own authorized scaffolding as
            // unjournaled drift. Structure-free effects keep the walk-free
            // fast path.
            self.state.unscoped_workspace_fingerprint =
                unscoped_workspace_fingerprint(&root, &self.state.policy)?;
        }
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

#[cfg(not(unix))]
fn canonical_target(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let target = root.join(relative);
    match fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err("transaction target must not be a symlink".into());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!("cannot inspect write target: {error}"));
        }
    }
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

#[cfg(unix)]
static SECURE_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(unix)]
const PATH_AUTHORITY_SYMLINK: &str = "path contains a symlink or reparse component";

#[cfg(unix)]
fn path_authority_error(prefix: &str, error: io::Error) -> String {
    if error.raw_os_error() == Some(libc::ELOOP) {
        PATH_AUTHORITY_SYMLINK.into()
    } else if error.kind() == io::ErrorKind::NotFound {
        "path does not exist".into()
    } else {
        format!("{prefix}: {error}")
    }
}

#[cfg(unix)]
fn open_root_directory(root: &Path) -> Result<File, String> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root)
        .map_err(|error| format!("cannot safely open transaction worktree root: {error}"))
}

#[cfg(unix)]
fn descriptor_path(root: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", root.as_raw_fd()))
}

#[cfg(unix)]
fn relative_components(relative: &str) -> Result<Vec<CString>, String> {
    let path = Path::new(relative);
    if relative.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("path must be a normalized relative path without traversal".into());
    }
    Path::new(relative)
        .components()
        .map(|component| match component {
            Component::Normal(name) => CString::new(name.as_bytes())
                .map_err(|_| "path contains an embedded NUL".to_string()),
            _ => Err("path must contain only normal components".into()),
        })
        .collect()
}

#[cfg(unix)]
fn open_anchored_parent(
    root: &File,
    relative: &str,
    create_missing: bool,
) -> Result<(File, CString), String> {
    let mut components = relative_components(relative)?;
    let leaf = components
        .pop()
        .ok_or_else(|| "path has no leaf component".to_string())?;
    let mut parent = root
        .try_clone()
        .map_err(|error| format!("cannot duplicate worktree root descriptor: {error}"))?;
    for component in components {
        let existing = anchored_entry_stat(&parent, &component)?;
        if existing.is_none() && !create_missing {
            return Err("path does not exist".into());
        }
        if existing.is_none() {
            let result = unsafe { libc::mkdirat(parent.as_raw_fd(), component.as_ptr(), 0o755) };
            if result < 0 {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(format!("cannot create transaction path component: {error}"));
                }
            }
        }
        let descriptor = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                component.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(path_authority_error(
                "path authority rejected parent component",
                io::Error::last_os_error(),
            ));
        }
        parent = unsafe { File::from_raw_fd(descriptor) };
    }
    Ok((parent, leaf))
}

#[cfg(unix)]
fn anchored_entry_stat(parent: &File, leaf: &CString) -> Result<Option<libc::stat>, String> {
    let mut value = std::mem::MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            leaf.as_ptr(),
            value.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(format!("cannot inspect authorized path: {error}"));
    }
    let value = unsafe { value.assume_init() };
    if (value.st_mode & libc::S_IFMT) == libc::S_IFLNK {
        return Err(PATH_AUTHORITY_SYMLINK.into());
    }
    if (value.st_mode & libc::S_IFMT) == libc::S_IFREG && value.st_nlink > 1 {
        return Err("path authority rejects multiply-linked file identity".into());
    }
    Ok(Some(value))
}

#[cfg(unix)]
fn validate_anchored_existing_path(root: &File, relative: &str) -> Result<(), String> {
    let (parent, leaf) = open_anchored_parent(root, relative, false)?;
    anchored_entry_stat(&parent, &leaf)?.ok_or_else(|| "path does not exist".to_string())?;
    Ok(())
}

#[cfg(unix)]
fn creates_missing_ancestors(root: &File, relative: &str) -> Result<bool, String> {
    let mut components = relative_components(relative)?;
    // The leaf itself never contributes an unscoped row; only ancestors can.
    components.pop();
    let mut parent = root
        .try_clone()
        .map_err(|error| format!("cannot duplicate worktree root descriptor: {error}"))?;
    for component in components {
        match anchored_entry_stat(&parent, &component)? {
            None => return Ok(true),
            Some(stat) if (stat.st_mode & libc::S_IFMT) == libc::S_IFDIR => {}
            // A non-directory ancestor makes the anchored effect fail closed;
            // there is no structure to journal.
            Some(_) => return Ok(false),
        }
        let descriptor = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                component.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(path_authority_error(
                "path authority rejected parent component",
                io::Error::last_os_error(),
            ));
        }
        parent = unsafe { File::from_raw_fd(descriptor) };
    }
    Ok(false)
}

#[cfg(not(unix))]
fn missing_ancestor_dirs(target: &Path) -> bool {
    let mut ancestor = target.parent();
    while let Some(dir) = ancestor {
        if !dir.is_dir() {
            return true;
        }
        ancestor = dir.parent();
    }
    false
}

#[cfg(unix)]
fn validate_anchored_write_path(root: &File, relative: &str) -> Result<(), String> {
    match open_anchored_parent(root, relative, false) {
        Ok((parent, leaf)) => {
            let _ = anchored_entry_stat(&parent, &leaf)?;
            Ok(())
        }
        Err(error) if error == "path does not exist" || error.contains("not found") => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn open_anchored_file(root: &File, relative: &str) -> Result<File, String> {
    let (parent, leaf) = open_anchored_parent(root, relative, false)?;
    anchored_entry_stat(&parent, &leaf)?.ok_or_else(|| "path does not exist".to_string())?;
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            leaf.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(path_authority_error(
            "path authority rejected final component",
            io::Error::last_os_error(),
        ));
    }
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(unix)]
fn secure_read(root: &File, relative: &str) -> Result<Vec<u8>, String> {
    let mut file = open_anchored_file(root, relative)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| format!("read failed: {error}"))?;
    Ok(bytes)
}

#[cfg(unix)]
fn secure_file_digest(root: &File, relative: &str) -> Result<Option<String>, String> {
    let mut file = match open_anchored_file(root, relative) {
        Ok(file) => file,
        Err(error) if error == "path does not exist" => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| format!("cannot hash authorized path: {error}"))?;
    Ok(Some(sha256_digest(&bytes)))
}

#[cfg(unix)]
fn secure_write(root: &File, relative: &str, bytes: &[u8]) -> Result<(), String> {
    let (parent, leaf) = open_anchored_parent(root, relative, true)?;
    let _ = anchored_entry_stat(&parent, &leaf)?;
    let pid = std::process::id();
    let mut last_error = None;
    for attempt in 0..32u64 {
        let sequence = SECURE_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = CString::new(format!(
            ".{}.axiom-tmp-{pid}-{sequence}-{attempt}",
            leaf.to_string_lossy()
        ))
        .map_err(|_| "path contains an embedded NUL".to_string())?;
        let descriptor = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if descriptor < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::AlreadyExists {
                last_error = Some(error);
                continue;
            }
            return Err(format!(
                "cannot create exclusive transaction temp file: {error}"
            ));
        }
        let mut file = unsafe { File::from_raw_fd(descriptor) };
        let result = file
            .write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("cannot persist authorized write: {error}"));
        drop(file);
        if let Err(error) = result {
            let _ = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) };
            return Err(error);
        }
        if let Err(error) = anchored_entry_stat(&parent, &leaf) {
            let _ = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) };
            return Err(error);
        }
        let renamed = unsafe {
            libc::renameat(
                parent.as_raw_fd(),
                name.as_ptr(),
                parent.as_raw_fd(),
                leaf.as_ptr(),
            )
        };
        if renamed < 0 {
            let error = io::Error::last_os_error();
            let _ = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) };
            return Err(format!(
                "cannot atomically publish authorized write: {error}"
            ));
        }
        if unsafe { libc::fsync(parent.as_raw_fd()) } < 0 {
            return Err(format!(
                "authorized write published but parent sync failed: {}",
                io::Error::last_os_error()
            ));
        }
        return Ok(());
    }
    Err(format!(
        "cannot allocate exclusive transaction temp file: {}",
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "temporary file collision".into())
    ))
}

#[cfg(unix)]
fn secure_delete(root: &File, relative: &str) -> Result<(), String> {
    let (parent, leaf) = open_anchored_parent(root, relative, false)?;
    let value =
        anchored_entry_stat(&parent, &leaf)?.ok_or_else(|| "path does not exist".to_string())?;
    let flags = if (value.st_mode & libc::S_IFMT) == libc::S_IFDIR {
        libc::AT_REMOVEDIR
    } else {
        0
    };
    let result = unsafe { libc::unlinkat(parent.as_raw_fd(), leaf.as_ptr(), flags) };
    if result < 0 {
        return Err(format!("delete failed: {}", io::Error::last_os_error()));
    }
    if unsafe { libc::fsync(parent.as_raw_fd()) } < 0 {
        return Err(format!(
            "delete completed but parent sync failed: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn secure_rename(root: &File, from: &str, to: &str) -> Result<(), String> {
    let (source_parent, source_leaf) = open_anchored_parent(root, from, false)?;
    let _ = anchored_entry_stat(&source_parent, &source_leaf)?
        .ok_or_else(|| "rename source does not exist".to_string())?;
    let (target_parent, target_leaf) = open_anchored_parent(root, to, false)?;
    let _ = anchored_entry_stat(&target_parent, &target_leaf)?;
    let result = unsafe {
        libc::renameat(
            source_parent.as_raw_fd(),
            source_leaf.as_ptr(),
            target_parent.as_raw_fd(),
            target_leaf.as_ptr(),
        )
    };
    if result < 0 {
        return Err(format!("rename failed: {}", io::Error::last_os_error()));
    }
    if unsafe { libc::fsync(source_parent.as_raw_fd()) } < 0
        || unsafe { libc::fsync(target_parent.as_raw_fd()) } < 0
    {
        return Err(format!(
            "rename completed but parent sync failed: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn secure_chmod(root: &File, relative: &str, mode: u32) -> Result<(), String> {
    let file = open_anchored_file(root, relative)?;
    if unsafe { libc::fchmod(file.as_raw_fd(), mode as libc::mode_t) } < 0 {
        return Err(format!("chmod failed: {}", io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(not(unix))]
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

fn acquire_lease(path: &Path, #[cfg(unix)] root: &File) -> Result<File, String> {
    let parent = path
        .parent()
        .ok_or_else(|| "transaction lease path has no parent".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("cannot create transaction lease parent: {e}"))?;
    #[cfg(unix)]
    let opened = open_lease(root);
    #[cfg(not(unix))]
    let opened = open_lease(path);
    let file = opened.map_err(|e| format!("cannot open transaction lease: {e}"))?;
    lock_lease(&file).map_err(|e| format!("transaction lease unavailable: {e}"))?;
    Ok(file)
}

#[cfg(unix)]
fn open_lease(root: &File) -> io::Result<File> {
    let name = CString::new(LEASE_FILE).expect("constant lease name");
    // Anchor ownership to the same directory as all durable state I/O.
    let fd = unsafe {
        libc::openat(
            root.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            0o600,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(io::Error::other(
            "transaction lease must be a single-link regular file",
        ));
    }
    Ok(file)
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
    use std::time::{Duration, Instant};

    // flock rides the open file description, and O_CLOEXEC descriptors only
    // close at execve. A subprocess spawned anywhere in this process
    // therefore references every live lease descriptor between fork and
    // exec; if a transaction is dropped inside that window, the inherited
    // reference keeps the lock alive past the parent's close until the child
    // execs (observed: ~1ms). A bounded retry absorbs this same-process
    // spawn artifact. A genuine live owner, in this process or another,
    // still holds the lease after the deadline and fails closed with the
    // same error.
    const ACQUIRE_DEADLINE: Duration = Duration::from_millis(250);
    const ACQUIRE_INTERVAL: Duration = Duration::from_millis(1);

    let deadline = Instant::now() + ACQUIRE_DEADLINE;
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EWOULDBLOCK) || Instant::now() >= deadline {
            return Err(error);
        }
        std::thread::sleep(ACQUIRE_INTERVAL);
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
    #[cfg(unix)] root: &File,
) -> Result<(), String> {
    match expected {
        Some((expected_owner, expected_generation)) => {
            #[cfg(unix)]
            let current = secure_read(root, STATE_FILE);
            #[cfg(not(unix))]
            let current = fs::read(path);
            let bytes = current.map_err(|e| {
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

    let mut next = state.clone();
    next.generation = match expected {
        Some((_, generation)) => generation
            .checked_add(1)
            .ok_or_else(|| "transaction state generation overflow".to_string())?,
        None => 1,
    };
    next.checksum.clear();
    next.checksum = state_checksum(&next)?;
    let bytes = serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    secure_write(root, STATE_FILE, &bytes)?;
    #[cfg(not(unix))]
    atomic_write(path, &bytes)?;
    *state = next;
    Ok(())
}

fn state_checksum(state: &TransactionState) -> Result<String, String> {
    let mut copy = state.clone();
    copy.checksum.clear();
    let bytes = serde_json::to_vec(&copy).map_err(|e| e.to_string())?;
    Ok(sha256_digest(&bytes))
}

#[cfg(any(not(unix), test))]
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let first_nonce = ATOMIC_TEMP_SEQUENCE.fetch_add(ATOMIC_TEMP_ATTEMPTS, Ordering::Relaxed);
    atomic_write_from_nonce(path, bytes, first_nonce)
}

#[cfg(any(not(unix), test))]
fn atomic_write_from_nonce(path: &Path, bytes: &[u8], first_nonce: u64) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "state path has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let (tmp, mut file) = create_atomic_temp(parent, first_nonce)?;
    let mut cleanup = AtomicTempCleanup::new(tmp.clone());

    if let Err(error) = file.write_all(bytes) {
        drop(file);
        return Err(cleanup.after_failure(format!("cannot write atomic temp file: {error}")));
    }
    if let Err(error) = file.sync_all() {
        drop(file);
        return Err(cleanup.after_failure(format!("cannot sync atomic temp file: {error}")));
    }
    drop(file);

    if let Err(error) = fs::rename(&tmp, path) {
        return Err(
            cleanup.after_failure(format!("cannot replace destination atomically: {error}"))
        );
    }
    cleanup.disarm();
    sync_parent(parent)
}

#[cfg(any(not(unix), test))]
fn atomic_temp_path(parent: &Path, nonce: u64) -> PathBuf {
    parent.join(format!(
        ".axiom-atomic-{}-{nonce:016x}.tmp",
        std::process::id()
    ))
}

#[cfg(any(not(unix), test))]
fn create_atomic_temp(parent: &Path, first_nonce: u64) -> Result<(PathBuf, File), String> {
    for offset in 0..ATOMIC_TEMP_ATTEMPTS {
        let path = atomic_temp_path(parent, first_nonce.wrapping_add(offset));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        match options.open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("cannot create atomic temp file: {error}")),
        }
    }
    Err("cannot create atomic temp file after collision retries".into())
}

#[cfg(any(not(unix), test))]
struct AtomicTempCleanup {
    path: PathBuf,
    armed: bool,
}

#[cfg(any(not(unix), test))]
impl AtomicTempCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }

    fn after_failure(&mut self, primary: String) -> String {
        match fs::remove_file(&self.path) {
            Ok(()) => {
                self.armed = false;
                primary
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.armed = false;
                primary
            }
            Err(error) => format!("{primary}; cannot remove atomic temp file: {error}"),
        }
    }
}

#[cfg(any(not(unix), test))]
impl Drop for AtomicTempCleanup {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(unix)]
#[cfg(any(not(unix), test))]
fn sync_parent(parent: &Path) -> Result<(), String> {
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("cannot sync atomic destination directory: {error}"))
}

#[cfg(not(unix))]
#[cfg(any(not(unix), test))]
fn sync_parent(_parent: &Path) -> Result<(), String> {
    Ok(())
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
    fn atomic_write_replaces_file_and_removes_its_temp_entry() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("allowed.txt");
        fs::write(&target, b"old").unwrap();
        let first_nonce = 0x1000;

        atomic_write_from_nonce(&target, b"new", first_nonce).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert!(!atomic_temp_path(dir.path(), first_nonce).exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_does_not_follow_the_old_predictable_temp_symlink() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new().unwrap();
        let worktree = dir.path().join("worktree");
        fs::create_dir(&worktree).unwrap();
        let target = worktree.join("allowed.txt");
        let sentinel = dir.path().join("external-sentinel.txt");
        let old_temp = worktree.join(".allowed.txt.tmp");
        fs::write(&sentinel, b"outside").unwrap();
        symlink(&sentinel, &old_temp).unwrap();

        atomic_write(&target, b"authorized").unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"authorized");
        assert_eq!(fs::read(&sentinel).unwrap(), b"outside");
        assert_eq!(fs::read_link(&old_temp).unwrap(), sentinel);
    }

    #[test]
    fn atomic_write_retries_without_opening_a_colliding_entry() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("allowed.txt");
        let first_nonce = 0x2000;
        let collision = atomic_temp_path(dir.path(), first_nonce);
        fs::write(&collision, b"do not truncate").unwrap();

        atomic_write_from_nonce(&target, b"authorized", first_nonce).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"authorized");
        assert_eq!(fs::read(&collision).unwrap(), b"do not truncate");
        assert!(!atomic_temp_path(dir.path(), first_nonce + 1).exists());
    }

    #[test]
    fn atomic_write_cleans_temp_entry_when_rename_fails() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("destination-directory");
        fs::create_dir(&target).unwrap();
        let first_nonce = 0x3000;

        assert!(
            atomic_write_from_nonce(&target, b"cannot replace a directory", first_nonce).is_err()
        );

        assert!(target.is_dir());
        assert!(!atomic_temp_path(dir.path(), first_nonce).exists());
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

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_write_target_fails_closed() {
        use std::os::unix::fs::symlink;

        let (repo, source, sha) = fixture();
        let worktree = repo.path().join("txn");
        let mut txn = TransactionalWorkspace::create(&source, &worktree, &sha, policy()).unwrap();
        let missing = repo.path().join("missing-external-target");
        let target = worktree.join("allowed.txt");
        fs::remove_file(&target).unwrap();
        symlink(&missing, &target).unwrap();

        let error = txn.write("allowed.txt", b"bad").unwrap_err();

        assert_eq!(error, "path contains a symlink or reparse component");
        assert_eq!(fs::read_link(&target).unwrap(), missing);
        assert!(!missing.exists());
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

    fn nested_policy() -> WorkspacePolicy {
        WorkspacePolicy {
            allowed_read_paths: BTreeSet::from([
                "allowed.txt".into(),
                "out/deep/result.json".into(),
            ]),
            allowed_write_paths: BTreeSet::from([
                "allowed.txt".into(),
                "out/deep/result.json".into(),
            ]),
            ..WorkspacePolicy::default()
        }
    }

    #[test]
    fn nested_scope_write_survives_interrupt_resume_and_recovery() {
        let (repo, source, sha) = fixture();
        let worktree = repo.path().join("txn");
        let mut txn =
            TransactionalWorkspace::create(&source, &worktree, &sha, nested_policy()).unwrap();
        txn.write("out/deep/result.json", b"{\"ok\":true}")
            .unwrap();
        assert!(worktree.join("out/deep").is_dir());
        txn.mark_interrupted().unwrap();
        drop(txn);
        let mut recovered = TransactionalWorkspace::recover(&worktree).unwrap();
        assert_eq!(recovered.state().phase, TransactionPhase::Interrupted);
        recovered.resume().unwrap();
        recovered
            .write("out/deep/result.json", b"{\"ok\":false}")
            .unwrap();
        recovered.abort().unwrap();
        assert_eq!(recovered.state().phase, TransactionPhase::Aborted);
        assert!(!worktree.join("out").exists());
    }

    #[test]
    fn unscoped_drift_under_created_ancestors_is_still_rejected() {
        let (repo, source, sha) = fixture();
        let worktree = repo.path().join("txn");
        let mut txn =
            TransactionalWorkspace::create(&source, &worktree, &sha, nested_policy()).unwrap();
        txn.write("out/deep/result.json", b"{}").unwrap();
        txn.mark_interrupted().unwrap();
        drop(txn);
        fs::write(worktree.join("out/evil.txt"), b"unjournaled").unwrap();
        let error = TransactionalWorkspace::recover(&worktree).unwrap_err();
        assert!(
            error.contains("does not match its durable journal"),
            "{error}"
        );
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
