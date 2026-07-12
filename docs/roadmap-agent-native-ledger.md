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
| Direct native backend | #1124 | #105, #627-#630, #652-#656, #691-#694, #927-#929, #1191, #1255, #1434 | structurally shipped; runtime-truth correction active | Keep generated Rust outside the CLI and execute the compile-time replay correction from #1434. Existing green ABI rows do not supersede it. |
| Production language readiness | #1432 | #1433-#1467, #1476-#1477, #1481 | active dependency-ordered program | Dispatch from the leaf order in `docs/production-language-roadmap.md`; require the row's evidence tier before closure. |
| Rust bootstrap removal / self-hosting | #721 | #565, #1254, #1366, #1425-#1428, #1434, #1436-#1440, #1468-#1475, #1478-#1479 | active final gate | Execute from the leaf issues. Runtime truth, compiler-source ownership, and snapshot evidence are all required. |
| Property testing | #715 | #560, #561, #637, #639, #640, #641, #642, #672, #676, #678, #679, #680, #706, #711, #712, #714 | shipped as the Phase-I property gate | #715, #714, and #712 are the shipped property-gate record. Close older Phase-I duplicate trackers such as #561 against this evidence when explicitly assigned; route remaining Cargo/Rust-bootstrap removal to #719 and #721. |
| Doc/LSP self-hosting | #731 | #563, #564, #646, #647, #648, #649, #651, #689, #723, #725, #727, #728 | #731 is closed; use it as the completed doc/LSP-owned readiness proof and treat #563/#564 as historical slices | Do not infer full self-hosting completion from doc/LSP readiness. Final compiler-source migration and snapshot bootstrap remain with #721. |
| Numeric tower | #716 | #681, #683, #685, #686, #688, #690, #718, #720, #722, #724, #726 | closed historical family | Open a source-grounded leaf for any missing numeric behavior; do not revive the duplicate phase stack. |
| Traits and macros | #695 | #623-#626, #631-#634, #658-#667, #697-#702 | closed historical family | Keep any new trait and macro gaps separate and fixture-backed. |
| Mutable borrow work | #713 | #328, #330, #332, #611-#619, #670-#673, #705-#710 | closed historical family | New ownership/borrow gaps need a current conformance-backed issue; #1426 owns the self-hosting parameter ABI. |
| Crypto runtime capability | #743 | #740, #741, #742 | closed historical family | New primitives require separate capability, denial, runtime, and constant-time evidence. |
| Socket/runtime capability | #738 | #608, #611, #735-#737 | closed historical family | New network behavior must retain loopback/policy fixtures and receive a current leaf issue. |

## Agent-Native Semantic Layer

| Family | Canonical issue | Duplicate / related issues | Disposition | Agent instruction |
|---|---:|---|---|---|
| Vision and Rust boundary | #774 | #775, #789 | shipped | Keep new semantics Axiom-neutral and preserve the anti-capture checks. |
| Roadmap and execution contract | #776 | #788 | shipped | GitHub issues remain the execution source of record; keep this ledger aligned with live issue state. |
| Intent IR v0 schema | #777 | #778, #779, #781, #785, #787 | shipped foundation | The schema, smoke fixture, and #1418 real-package emitter define one contract. |
| Inspection, capabilities, effects, and axioms | #778 | #779, #780, #781 | shipped | Extend existing versioned envelopes and fixtures rather than creating parallel inspection graphs. |
| Evidence, artifacts, repair plans, and provenance | #782 | #783, #784, #785 | shipped | These remain read-only/verification foundations; bounded execution is #1422. |
| Backend target interface | #786 | #777, #783 | shipped | New targets must declare a v0 contract before implementation. |
| Agent-native proof demo | #787 | #777-#786 | shipped | Preserve the intent -> graph -> effects -> evidence -> artifacts proof as a regression surface. |
| Artifact target generators | #847 | #848, #849, #850, #851 | shipped | OpenAPI, policy, SQL, OpenTofu, and runbook generation must converge on complete Intent IR under #1418. |
| Delivery signals | #852 | #853, #854 | shipped | Signals remain evidence only; autonomous external mutations belong to #1423. |
| Verification, semantic diff, and decisions | #855 | #856, #857 | shipped | Use these surfaces as inputs to the verification planner in #1421. |
| Repair executor design | #858 | #784, #854 | design accepted; implementation open | `docs/repair-executor-v0.md` is the contract; implementation is #1422 and merge-capable delivery is separately gated by #1423. |

## Autonomous Agent Execution

The complete roadmap is [Autonomous Agent Execution Roadmap](autonomous-agent-roadmap.md).

| Family | Canonical issue | Disposition | Agent instruction |
|---|---:|---|---|
| Unattended coding umbrella | #1417 | needs human approval for policy transitions | Compose child gates; do not infer delivery permission from feature completeness. |
| Complete Intent IR emission | #1418 | implemented; review-gated | Preserve deterministic package/workspace emission, provenance, traceable diagnostics, and the shared consumer contract. |
| Typed task contract | #1419 | implemented; review-gated | Preserve approved source authority and compile strict feature/repair specs into deterministic bounded contracts; rejected or ambiguous authority fails closed. |
| Transactional workspace | #1420 | ready for planning | Isolate work, enforce file/command/capability policy, preserve dirty user state, and prove rollback. |
| Verification planner | #1421 | ready for planning | Map semantic drift to exact-head positive, negative, schema, artifact, security, and performance evidence. |
| Bounded executor | #1422 | ready after #1419-#1421 | Implement dry-run, deterministic repair, assisted proposals, budgets, retries, and auditable terminal states. |
| Delivery controller | #1423 | human approval required before merge-capable code | Require independent review; prohibit self-approval, force-push, and policy bypass. |
| Autonomy evaluation | #1424 | ready for planning | Gate promotion on correctness, containment, recovery, escalation, time, and cost, including tasks that must stop. |

## Self-Hosting Track Reconciliation

"Independence from Rust" covers two distinct programs that the issue history
conflates. This section is the canonical reconciliation; prefer it over the
phase scheme in #565 where they disagree.

- **Backend-exit** — make `rustc`/Cargo unnecessary to build *user programs* by
  defaulting to the direct-native Cranelift backend and removing the
  generated-Rust backend. The structural track is shipped, but runtime
  completeness is not: unsupported programs can still fall into compiler-side
  evaluation and replay. #1434 must close before treating backend exit as a
  semantic execution proof.
  Note: Cranelift is itself a Rust crate linked into `axiomc`, so backend-exit
  removes the `rustc` *step*, not Rust from the toolchain.
- **Host-exit (self-hosting)** — rewrite `axiomc` itself in AxiOM and prove a
  snapshot bootstrap chain (a shipped `axiomc` builds the next without Cargo).
  This is #565's thesis ("Rust is not the product"). It is active but early:
  existing AxiOM compiler spikes prove static/bootstrap shapes, while the
  compiler remains roughly 90k lines of hand-written Rust source. The leaf path
  is #1254, #1425-#1428, #1434, #1436-#1440, and #1468-#1479, with #721 as the
  final policy gate.

| Concern | Canonical issue | Disposition | Agent instruction |
|---|---:|---|---|
| Self-hosting master thesis | #565 | keep open; **historical phase scheme — do not execute from its checkboxes** | #565's Phase-G checkboxes are stale (conformance corpus and `check --properties` shipped) and its phase letters collide with the active `phase-j`/`phase-l` labels. Treat #565 as intent narrative; take execution gates from this ledger. |
| Feasibility and design | #1253 | closed as completed | The diagnostics spikes and snapshot-chain design shipped; execute from the leaf issues below. |
| Monolith decomposition | #1254 | keep open | Prerequisite for package-by-package porting. #930/#936–#940 closed the *boundary contracts* (snapshot fixtures + validators), **not** source migration — do not infer decomposition is done from their closed state. |
| Self-hosting language readiness | #1366 | keep open | Defer to `docs/self-hosting-language-readiness.json`: build purity #1434, executable MIR/lifecycle/ownership #1436-#1440, runtime sequences/text #1425-#1426, maps/sets #1476, program host ABI #1477, runtime crypto #1481, and compiler proof #1427 remain before migration entry. |
| Effect-pure compilation | #1434 | first implementation task | Reject unsupported runtime lowering; no environment, filesystem, process, network, clock, randomness, or other runtime effect may execute during build. |
| Executable MIR and lifecycle | #1436-#1440 | design then implementation | Establish backend-neutral control/value/effect/lifecycle semantics before expanding native shapes. |
| Runtime-sized compiler collections | #1425 | ready for planning | Land Axiom-neutral language/stdlib/backend contracts with conformance and runtime evidence. |
| String and slice parameter ABI | #1426 | ready for planning | Prove owned/borrowed values and mutable write-through across direct-native function boundaries. |
| Compiler-scale AxiOM proof | #1427 | ready after runtime foundation | Build once, run with different runtime inputs, and prove output/effects change without compiler-side execution. |
| Compiler source migration | #1468-#1475, #1478-#1479 | blocked on #1427 | Port diagnostics, syntax, packages, HIR, MIR, stdlib, backend contracts/runtime, commands, docs, and LSP with coexistence and rollback. |
| Snapshot bootstrap chain | #1428 | human-gated release work | Publish the genesis snapshot, prove offline build/test, no Cargo after genesis, and the normalized fixpoint. |
| Final host-exit gate | #721 | human decision required | Close only when language, source ownership, snapshot, release, and live evidence are all green for the exact candidate. |

## Standing Agent Rules

- Start from the canonical issue unless the user explicitly assigns a duplicate
  or a related issue.
- Keep one issue or tightly coupled foundation slice per branch.
- Do not close issues from this ledger automatically.
- Link PRs to the governing issue with `Closes #...` when the acceptance
  criteria are actually satisfied.
- For semantic-layer changes, include docs, schema or schema delta, fixture, and
  validation evidence in the same PR unless the issue is explicitly docs-only.
