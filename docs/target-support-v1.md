# Target Support v1

AxiOM direct-native builds currently support exact-host execution on two
initial targets:

| Target triple | Platform | Object | ABI | libc/runtime |
| --- | --- | --- | --- | --- |
| `x86_64-unknown-linux-gnu` | `linux-x86-64` | ELF | SysV AMD64 | glibc-compatible Linux runtime |
| `aarch64-apple-darwin` | `macos-arm64` | Mach-O | Darwin arm64 | Darwin libSystem runtime |

The contract is host-only. Supplying the exact active host triple is accepted;
requesting any other target fails with `target.unsupported` instead of silently
using host code generation. `wasm32`, Windows-native compilation, and all
cross-compilation remain explicit unsupported features.

Both debug and release profiles are in the target contract. The compiler uses
the host linker and runtime for each row. Capability providers such as crypto,
TLS, databases, and external native extensions require their own qualification;
being on a supported host does not automatically qualify one of those
providers.

`axiomc doctor --json` publishes the active host, support decision, target
selection rule, and the full target catalog under `target_support`. The report
conforms to `stage1/schemas/axiom-target-support-v1.schema.json`.
The `libc`, `provider_policy`, `profiles`, and `unsupported_features` row fields
are additive v1 metadata: current `axiomc` always emits them, while the v1 schema
keeps them optional so reports from earlier v1 producers remain valid. A
qualification consumer must require those fields before relying on their newer
claims.

This declaration is not itself two-host proof. Authoritative qualification
requires exact-head evidence from both supported hosts: Linux x86-64 and
macOS arm64. Until that evidence passes for the same revision, target support
remains `supported-host-only` rather than cross-target or release
qualification.

The evidence contract keeps ordinary CI on standard hosted `ubuntu-24.04`
runners and adds no macOS CI job:

- Linux x86-64: the `Extended Validation` workflow's `target-support-evidence`
  job produces one `axiom.target_support_evidence.v1` artifact on pushes to
  `main`, on the nightly schedule, and on manual dispatches of the protected
  `main` ref. For pre-merge exact-PR-head evidence, the approval-gated
  `workflow_dispatch` input `evidence_pr_sha` accepts exactly one 40-hex
  commit SHA, validated fail-closed before checkout; the job then checks out
  that exact SHA and produces the same artifact bound to it. Dispatching
  requires write access to the governed repository, which is the approval
  gate for this path.
- macOS arm64: produced only by the explicitly reviewed specialized macOS
  evidence lane, never by a CI runner. The lane executes the identical
  fail-closed `scripts/ci/run-target-support-evidence-v1.py run` command on a
  real gateway-managed macOS arm64 node against a clean, isolated checkout of
  the exact PR head. The resulting evidence JSON, run log, node identity,
  commands, timestamps, and SHA-256 checksums are committed into the PR
  branch under an EVIDENCE manifest that binds them to the exact tested
  head, and an independent reviewer verifies that binding before merge.

Each artifact verifies the checkout against the expected head SHA, rejects
tracked, untracked, ignored, or staged inputs before execution, pins and
records the Rust/Cargo/lockfile identity, and rechecks tracked state and
lockfile identity after execution. It builds debug and release compiler
binaries for the explicit target, inspects target-specific ELF or Mach-O
architecture fields, and executes the exact explicitly targeted smoke
artifact only after its canonical project path, no-symlink boundary, target
architecture, and content identity are established. CLI/worker/HTTP proof
workloads, doctor output, and unsupported-target rejection are recorded
separately. Each compiler binary record includes its SHA-256 identity; hashes
are diagnostic identities and are not required to match across hosts because
host-specific paths may affect output. Provider-specific examples remain
outside this host contract and require their own qualification.

Run the contract self-tests locally with `make target-support-v1-test`. A local
host evidence artifact can be produced after committing the exact tree with:

```sh
python3 scripts/ci/run-target-support-evidence-v1.py run \
  --expected-target "$(rustc -vV | sed -n 's/^host: //p')" \
  --head-sha "$(git rev-parse HEAD)" \
  --trigger local \
  --runner-label local \
  --output /tmp/axiom-target-support-evidence.json
```
