# Agent-Native Roadmap Ledger

This ledger is the execution map for agent-native and adjacent compiler-roadmap
work. GitHub issues remain the source of record; this file tells agents which
issue to treat as canonical when several open issues describe the same family.

Agents must not close duplicate issues from this ledger without explicit human
approval. When assigned a duplicate or related issue, first confirm whether the
canonical issue has already shipped or has an active PR.

## Disposition Terms

- `keep open`: valid work remains and the issue can be assigned directly.
- `close as duplicate`: likely duplicate, but leave open until a maintainer
  approves closure.
- `supersede`: newer issue should drive future work; older issue remains only
  for historical context until a maintainer closes it.
- `defer`: valid work, but not the next agent execution target.
- `needs human approval`: requires product, architecture, or maintainer
  decision before implementation.

## Existing Roadmap Families

| Family | Canonical issue | Duplicate / related issues | Disposition | Agent instruction |
|---|---:|---|---|---|
| Direct native backend | #1124 | #105, #627, #628, #629, #630, #652, #653, #654, #656, #691, #692, #693, #694, #927, #928, #929 | use #1124 for remaining direct-native ABI completion; treat #693/#694 as closed historical planning slices | Start with the runtime ABI matrix before backend replacement work. Do not remove generated Rust from the supported toolchain until #1124 is ready and #1191 removes the compatibility backend from supported builds. |
| Rust bootstrap removal / self-hosting | #721 | #559, #562, #643, #644, #645, #682, #684, #687, #717, #719, #930, #931, #932, #936, #937, #938, #939, #940 | supersede older phase duplicates; #721 is the current final Rust-bootstrap removal gate | Keep work tied to the current Phase-J family. Do not remove Rust or Cargo behavior unless the assigned issue explicitly permits it and `make rust-exit-readiness` is green. |
| Property testing | #715 | #560, #561, #637, #639, #640, #641, #642, #672, #676, #678, #679, #680, #706, #711, #712, #714 | shipped as the Phase-I property gate | #715, #714, and #712 are the shipped property-gate record. Close older Phase-I duplicate trackers such as #561 against this evidence when explicitly assigned; route remaining Cargo/Rust-bootstrap removal to #719 and #721. |
| Doc/LSP self-hosting | #731 | #563, #564, #646, #647, #648, #649, #651, #689, #723, #725, #727, #728 | use #731 for the remaining doc/LSP-owned readiness gate; treat #563/#564 as historical slices | Work the current Phase-K/Phase-L issue directly. Do not infer self-hosting completion from stdlib/doc helper work. |
| Numeric tower | #716 | #681, #683, #685, #686, #688, #690, #718, #720, #722, #724, #726 | supersede older duplicate stack | Prefer the newest Numeric tower A-F sequence. Do not implement multiple numeric slices in one PR unless assigned. |
| Traits and macros | #695 | #623, #624, #625, #626, #631, #632, #633, #634, #658, #659, #660, #662, #663, #665, #666, #667, #697, #698, #699, #700, #701, #702 | keep open by slice | Keep traits and macros separate. Syntax-only slices should not add codegen behavior unless the issue says so. |
| Mutable borrow work | #713 | #328, #330, #332, #611, #612, #613, #614, #615, #616, #618, #619, #670, #671, #673, #705, #707, #709, #710 | keep open by slice | Use active AG1.2 issues for current borrow behavior. Treat older AG1 issues as historical unless assigned. |
| Crypto runtime capability | #743 | #740, #741, #742 | keep open by slice | Keep AEAD, signing, random, and constant-time primitives separate; each needs capability and conformance evidence. |
| Socket/runtime capability | #738 | #608, #611, #735, #736, #737 | keep open by slice | Do not broaden network behavior without policy fixtures. Host/port policy belongs with #737. Async integration belongs with #738. |

## Agent-Native Semantic Layer

| Family | Canonical issue | Duplicate / related issues | Disposition | Agent instruction |
|---|---:|---|---|---|
| Vision and positioning | #774 | #775, #789 | keep open; #775 and #789 are foundation companions | Complete docs first. Do not implement semantic primitives until the vocabulary and Rust boundary are merged. |
| Roadmap normalization | #776 | this ledger | keep open until merged | Update this ledger when new semantic-layer issues are created or old duplicates are closed. |
| Intent IR / semantic graph | #777 | #778, #779, #781, #785, #787 | keep open; schema-first | Define schema and smoke fixture before CLI APIs or source syntax depend on it. |
| Agent inspection API | #778 | #777, #780, #782, #783 | defer until #777 lands | `axiomc inspect` should emit versioned envelopes backed by schemas and fixtures. |
| Semantic capabilities | #779 | #780, #781, #787 | defer until Intent IR v0 exists | Parser/HIR work must include docs, schema edges, pass/fail fixtures, and inspect output. |
| Effect model | #780 | #779, #778 | defer until Intent IR v0 exists | Map manifest capabilities into semantic effects without changing `axiomc caps` behavior. |
| Axioms/invariants | #781 | #779, #787 | defer until Intent IR v0 exists | v0 axioms are declared and traceable, not formally proven. |
| Evidence model | #782 | #784, #787 | keep open | Evidence reports should connect existing tests/conformance to semantic completion gates. |
| Artifact plan | #783 | #785, #787 | keep open | Represent existing build/test/doc outputs before adding new generators. |
| Repair-plan protocol | #784 | #782, #778 | defer until evidence model shape exists | Emit plans only; no auto-fixer or LLM invocation. |
| Provenance / trace | #785 | #783, #778 | defer until artifact plan and Intent IR shape exist | Connect source spans, semantic nodes, generated artifacts, and evidence. |
| Issue-to-PR traceability | #852 | #785 | shipped as advisory delivery signal | `scripts/ci/issue-pr-traceability.py` emits issue -> PR -> changed-file/semantic-hint reports without adding a second required status check. |
| Backend target interface | #786 | #105, #777, #783 | shipped as docs + schema | `docs/backend-target-interface-v0.md` and `stage1/schemas/axiom-target-v0.schema.json` define target classes, contract slots, and the generated-Rust and direct-native backend mappings; non-Rust artifact targets must declare contracts in this shape before they ship. |
| Agent-native proof demo | #787 | #777, #778, #779, #780, #781, #782, #783 | defer until component APIs exist | Demo should prove intent -> graph -> effects -> evidence -> artifacts. |
| Artifact-target generators | #847 | #848, #849, #850, #851 | defer until one generator exercises the #786 contract | Each generator must declare a target contract in the `backend-target-interface-v0` v0 shape. #847 (OpenAPI) and #848 (policy bundle) are the lowest-risk first targets because their inputs already exist; #849 (SQL) and #850 (Terraform) depend on new declaration surfaces (their Part A) and must not start codegen before that lands; #851 (runbook) composes existing capability/evidence/artifact nodes. |
| Semantic verification | #855 | #781, #782, #786 | defer until evidence model and axioms exist | `axiomc verify` joins declared axioms to backing evidence and target-contract evidence requirements, computing `verified_by`/`violates`. No model invocation, no file edits, no new required `CI Gate` check. |
| Semantic diff / drift gate | #856 | #777, #786 | defer until Intent IR v0 emits real graphs | Classify capability/effect/axiom/contract drift between Intent IR snapshots. Advisory before gating; enforces the no-schema-drift non-goal without becoming a second required check. |
| Decision records | #857 | #777, #778 | defer until Intent IR v0 lands | Make the `Decision` node kind reachable, schema-first and fixture-backed. Do not auto-generate decisions; link existing decisions/RFCs into the graph. |
| PR queue remediation | #853 | #854, #858 | shipped as read-only operator worklist | `scripts/ci/pr-queue-remediation.py` classifies open PRs into deterministic remediation states with a fresh recheck timestamp. It does not auto-merge, rerun workflows, or force-push. |
| CI / review delivery evidence | #854 | #782 | keep open | Record live `CI Gate` and review state as evidence kinds with recheck timestamps. Records the signal only; does not add a required check or auto-merge. |
| Repair executor | #858 | #784, #782, #854 | needs human approval | `docs/repair-executor-v0.md` defines the proposed closed-loop contract. Do not implement executor code until an owner accepts that design or a later revision; edits must stay confined to `allowed_files`, no auto-merge, no force-push, fresh evidence + CI recheck before a task is resolved. |
| Agent execution contract | #788 | #774, #775, #776 | keep open until merged | Keep AGENTS and PR guidance aligned with semantic-layer governance. |

## Self-Hosting Track Reconciliation

"Independence from Rust" covers two distinct programs that the issue history
conflates. This section is the canonical reconciliation; prefer it over the
phase scheme in #565 where they disagree.

- **Backend-exit** — make `rustc`/Cargo unnecessary to build *user programs* by
  defaulting to the direct-native Cranelift backend and removing the
  generated-Rust backend. This is the active track: #1124 (direct-native ABI),
  #1191 (remove generated-Rust), #731 (Axiom-owned doc/LSP), #1255 (suite +
  cross-backend parity gating), gated by #721 / `make rust-exit-readiness`.
  Note: Cranelift is itself a Rust crate linked into `axiomc`, so backend-exit
  removes the `rustc` *step*, not Rust from the toolchain.
- **Host-exit (self-hosting)** — rewrite `axiomc` itself in AxiOM and prove a
  snapshot bootstrap chain (a shipped `axiomc` builds the next without Cargo).
  This is #565's thesis ("Rust is not the product"). It is **early**: the
  compiler is 3 Rust crates with ~91% of source in 7 monolith files, and no
  compiler component is written in AxiOM yet.

| Concern | Canonical issue | Disposition | Agent instruction |
|---|---:|---|---|
| Self-hosting master thesis | #565 | keep open; **historical phase scheme — do not execute from its checkboxes** | #565's Phase-G checkboxes are stale (conformance corpus and `check --properties` shipped) and its phase letters collide with the active `phase-j`/`phase-l` labels. Treat #565 as intent narrative; take execution gates from this ledger. |
| Active rewrite track | #1253 | keep open | Feasibility spike (one component in `.ax`) + snapshot-bootstrap design. The only active issue for the rewrite itself. Do not remove Rust/Cargo under it. |
| Monolith decomposition | #1254 | keep open | Prerequisite for package-by-package porting. #930/#936–#940 closed the *boundary contracts* (snapshot fixtures + validators), **not** source migration — do not infer decomposition is done from their closed state. |
| Self-hosting language readiness | #1256 | needs spec | Minimum AxiOM language + backend surface required before the rewrite can start (e.g. `?`/try is unsupported on the default backend). Gates the rewrite phases. |

## Standing Agent Rules

- Start from the canonical issue unless the user explicitly assigns a duplicate
  or a related issue.
- Keep one issue or tightly coupled foundation slice per branch.
- Do not close issues from this ledger automatically.
- Link PRs to the governing issue with `Closes #...` when the acceptance
  criteria are actually satisfied.
- For semantic-layer changes, include docs, schema or schema delta, fixture, and
  validation evidence in the same PR unless the issue is explicitly docs-only.
