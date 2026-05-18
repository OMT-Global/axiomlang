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
| Direct native backend | #105 | #627, #628, #629, #630, #652, #653, #654, #656, #691, #692, #693, #694 | supersede older slices unless explicitly assigned | Treat #105 as the umbrella. Use the newest lettered slice only when assigned; do not start a backend rewrite from an older duplicate. |
| Rust bootstrap removal / self-hosting | #721 | #559, #562, #643, #644, #645, #682, #684, #687, #717, #719 | supersede older phase duplicates | Keep work tied to the current Phase-J family. Do not remove Rust or Cargo behavior unless the assigned issue explicitly permits it. |
| Property testing | #715 | #560, #561, #637, #639, #640, #641, #642, #672, #676, #678, #679, #680, #706, #711, #712, #714 | supersede older phase duplicates | Use #715 for first-class property tests; use #711 only for the runner flag if assigned. |
| Doc/LSP self-hosting | #731 | #563, #564, #646, #647, #648, #649, #651, #689, #723, #725, #727, #728 | keep open by slice | Work the current Phase-K/Phase-L issues directly. Do not infer self-hosting completion from stdlib/doc helper work. |
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
| Backend target interface | #786 | #105, #777, #783 | keep open | Define target contracts before expanding non-Rust artifact targets. |
| Agent-native proof demo | #787 | #777, #778, #779, #780, #781, #782, #783 | defer until component APIs exist | Demo should prove intent -> graph -> effects -> evidence -> artifacts. |
| Agent execution contract | #788 | #774, #775, #776 | keep open until merged | Keep AGENTS and PR guidance aligned with semantic-layer governance. |

## Standing Agent Rules

- Start from the canonical issue unless the user explicitly assigns a duplicate
  or a related issue.
- Keep one issue or tightly coupled foundation slice per branch.
- Do not close issues from this ledger automatically.
- Link PRs to the governing issue with `Closes #...` when the acceptance
  criteria are actually satisfied.
- For semantic-layer changes, include docs, schema or schema delta, fixture, and
  validation evidence in the same PR unless the issue is explicitly docs-only.
