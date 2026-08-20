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
requires exact-head evidence from Linux x86-64 and macOS arm64 runners. Until
that evidence passes for the same revision, target support remains
`supported-host-only` rather than cross-target or release qualification.

The `Extended Validation` workflow produces one
`axiom.target_support_evidence.v1` artifact per host on pushes to `main`, the
nightly schedule, and manual dispatches of the protected `main` ref. Each
artifact verifies the checkout against `github.sha`, rejects tracked,
untracked, ignored, or staged inputs before execution, pins and records the
Rust/Cargo/lockfile identity, and rechecks tracked state and lockfile identity
after execution. It builds debug and release
compiler binaries for the explicit target, inspects target-specific ELF or
Mach-O architecture fields, and executes the exact explicitly targeted smoke
artifact only after its canonical project path, no-symlink boundary, target
architecture, and content identity are established. CLI/worker/HTTP proof
workloads, doctor output, and unsupported-target rejection are recorded
separately. Each compiler binary record includes its SHA-256 identity; hashes
are diagnostic identities and are not required to match across runners because
runner-specific paths may affect output. Provider-specific examples remain
outside this host contract and require their own qualification. The macOS job
uses the trusted private native runner pool, and the main-ref gate prevents
unreviewed branch dispatches from reaching it.

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
