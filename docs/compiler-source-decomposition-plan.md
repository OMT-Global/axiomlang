# Compiler Source Decomposition Plan

This plan turns the self-hosting source-layout boundary in
[AxiOM Compiler Source Layout and Self-Hosting Boundary](axiom-compiler-source-layout.md)
into a measurable migration plan for the largest Rust-hosted compiler files.
It is the first remediation slice for #1162 and supports the Rust-exit
bootstrap path by making compiler source ownership smaller, reviewable, and
package-aligned before AxiOM-owned packages replace the Rust host.

The plan does not make Rust module paths canonical. Rust files are migration
references only; the future ownership boundary remains the AxiOM package map in
`docs/axiom-compiler-source-layout.md`.

## Measurement

Run the blocking ratchet report with:

```bash
make stage1-compiler-source-monoliths
```

Validate that this plan still covers the current largest files with:

```bash
make stage1-compiler-source-monoliths-test
```

The report records:

- total hand-written Rust lines under `stage1/crates/axiomc/src`;
- the largest single compiler file;
- the total line count for the top files;
- the top-file share of the compiler source tree;
- the self-hosted package boundary each large file must move toward.

This is now a ratcheted gate. The target is that the largest files and
top-seven file share move downward release over release as child extraction PRs
land. The current ceilings are the maximum allowed values; extraction PRs should
lower the relevant ceiling in this plan when they remove lines from a tracked
monolith.

The `lib.rs` test-module extraction lowered the absolute top-seven source line
count. It also raised the top-seven share because test code left
`stage1/crates/axiomc/src`, reducing the source denominator. The HIR generic
analysis extraction then split generic inference and monomorphization into a
tracked `compiler.hir` module. The HIR model extraction then moved public HIR
types and type-display helpers into `stage1/crates/axiomc/src/hir/model.rs`,
and the HIR type-lowering extraction moved syntax-to-HIR literal, type, and
operator lowering into `stage1/crates/axiomc/src/hir/types.rs`. The HIR
definitions extraction then moved type-name collection, aggregate definitions,
trait type-use validation, and recursive aggregate checks into
`stage1/crates/axiomc/src/hir/definitions.rs`. The HIR signature extraction
then moved function/method signature collection, trait impl signature
validation, and HIR symbol-name resolution into
`stage1/crates/axiomc/src/hir/signatures.rs`. The HIR capability extraction
then moved FFI validation, capability checks, network/process allowlist
validation, and capability-focused tests into
`stage1/crates/axiomc/src/hir/capabilities.rs`. The HIR expression typing
extraction then moved numeric type predicates, method-owner typing, string
borrow coercion, binary-add typing, and static expression value helpers into
`stage1/crates/axiomc/src/hir/expressions.rs`. The HIR ownership extraction
then moved move/projection checks, borrow-region origin tracing, borrowed-slice
type detection, and active borrow counters into
`stage1/crates/axiomc/src/hir/ownership.rs`. The HIR property extraction then
moved property signature validation, static verdict detection, and property
diagnostic sample/help text into `stage1/crates/axiomc/src/hir/properties.rs`,
lowering both absolute top-file lines and share. The HIR reachability
extraction then moved stdlib reachability and call-graph discovery into
`stage1/crates/axiomc/src/hir/reachability.rs`, further lowering the main HIR
facade without making the Rust helper layout canonical. The HIR diagnostic
recovery extraction then moved primary/related diagnostic selection, flattening,
and deterministic sorting into `stage1/crates/axiomc/src/hir/diagnostics.rs`.
The HIR symbol extraction then moved monomorphized symbol naming and async or
collection intrinsic classification into `stage1/crates/axiomc/src/hir/symbols.rs`.
The HIR boundary-test extraction then moved the inline HIR lowering regression
module out to `stage1/crates/axiomc/tests/hir_unit.rs`, keeping the private
module boundary while shrinking the Rust-hosted HIR facade.
The HIR source-location extraction then moved syntax statement/expression span
accessors into `stage1/crates/axiomc/src/hir/source_locations.rs`, further
shrinking the facade while keeping span logic inside the `compiler.hir`
boundary.
The HIR control-flow extraction then moved return-flow analysis into
`stage1/crates/axiomc/src/hir/control_flow.rs`, keeping block return
classification inside the `compiler.hir` boundary while further shrinking the
facade.
The HIR const-array extraction then moved const integer evaluation and const
array length validation into `stage1/crates/axiomc/src/hir/const_arrays.rs`,
keeping compile-time array shape checks inside the `compiler.hir` boundary.
The HIR match-lowering extraction then moved enum/const match statement and
match expression lowering into `stage1/crates/axiomc/src/hir/matches.rs`,
keeping pattern validation and match-arm borrow handling inside the
`compiler.hir` boundary.
The HIR variant-constructor extraction then moved enum variant resolution and
positional/named payload constructor lowering into
`stage1/crates/axiomc/src/hir/variants.rs`, keeping variant payload validation
inside the `compiler.hir` boundary.
The HIR async-runtime extraction then moved async runtime intrinsic capability
and type lowering into `stage1/crates/axiomc/src/hir/async_runtime.rs`,
keeping async capability validation inside the `compiler.hir` boundary.
The HIR map-intrinsic extraction then moved map lookup/key/default intrinsic
type and ownership lowering into `stage1/crates/axiomc/src/hir/maps.rs`,
keeping collection intrinsic validation inside the `compiler.hir` boundary.
The HIR const-function extraction then moved const function body and expression
validation into `stage1/crates/axiomc/src/hir/const_functions.rs`, keeping
compile-time function restrictions inside the `compiler.hir` boundary.
The direct-native runtime-serving stack then raised the native backend baseline
before this ratchet merged; the ceilings below reflect that post-merge snapshot
so future backend growth must be paid down or accompanied by an explicit
ratchet update.

The cranelift intrinsics extraction then moved the pure runtime-intrinsic
implementations (JSON scalar parse/stringify, the stage1-safe regex engine,
percent encoding, and the crypto primitives) into
`stage1/crates/axiomc/src/cranelift_backend/intrinsics.rs`, lowering the
`cranelift_backend.rs` ceiling below its pre-language-slice level. The small
ceiling raises for `codegen.rs`, `hir.rs`, `hir/matches.rs`, `hir/variants.rs`,
`main.rs`, `project.rs`, and `syntax.rs` record feature growth from merged
work (#1355-#1376 era) that landed while the ratchet was still advisory, which landed while
the ratchet was still advisory; the ratchet now runs in the fast PR lane via
`run-fast-checks.sh`, so future growth fails CI unless the ceiling change is
explicit in the same PR.

The cranelift evaluator extraction then moved the compile-time program
evaluator (the `SpikeValue` interpreter and its host-capability call
dispatchers for fs, net, http, async, process, clock, crypto, json/serdes,
regex, and encoding) into
`stage1/crates/axiomc/src/cranelift_backend/evaluator.rs`, a sibling of the
intrinsics module. Shared value types and helpers reused by the i64 lowering
path stay in the parent and are visible to the submodule through
`use super::*`. This drops `cranelift_backend.rs` below 24k lines and the
top-file share below 0.78.

The cranelift lowering host-capability extraction then began peeling the
direct-native i64 lowering by capability group, starting with the cleanest
near-leaf: the filesystem family (`fs_read`/`fs_write` intrinsic lowering,
path-guard and audit helpers, and the compile-time `spike_fs_*` scope
resolvers) now lives in
`stage1/crates/axiomc/src/cranelift_backend/host_fs.rs`. This family has zero
callbacks into the recursive expr/stmt lowering hub, so the move is pure
relocation. Remaining host families (crypto, net/http, env/process/clock,
json/serdes) follow as their own slices before the mutually-recursive
value/control core is sub-partitioned by value shape.

The cranelift lowering host-capability extraction then moved the crypto
family (`crypto_sha256`/HMAC/constant-time-eq audited condition and helper
lowering, the compile-time `spike_crypto_*` AEAD and Ed25519 dispatchers, and
their OpenSSL FFI Drop-guard structs and dlopen/dlsym/dlclose bindings) into
`stage1/crates/axiomc/src/cranelift_backend/host_crypto.rs`. This family has
one callback into the recursive expr/stmt lowering hub (`lower_i64_expr`);
the FFI Guard structs, Drop impls, and OpenSSL symbol-loading helpers moved
alongside their sole callers since they are crypto-only, while the shared
`crypto_*` wrapper functions the compile-time evaluator calls stayed in
`cranelift_backend/intrinsics.rs`, reachable from the new sibling module
through the parent's `pub(crate) use host_crypto::*` re-export.

The cranelift lowering host-capability extraction then moved the net/http
family (TCP/UDP resolve and loopback-listen helpers, the HTTP request/response
parsing and server intrinsic lowering, and their `is_i64_*`/`lower_i64_*`
name predicates and audited-expression helpers) into
`stage1/crates/axiomc/src/cranelift_backend/host_net_http.rs`. Unlike the
crypto slice, the `Spike{Http,Tcp,Udp}*` structs and their `SPIKE_TCP_*`/
`SPIKE_UDP_*` registry statics stay in the parent because the compile-time
evaluator also drives them; the extraction is therefore a pure function move
(no struct relocation), with the moved functions reaching the shared structs
and registries through `use super::*` and the fifteen functions the evaluator
calls back into resolving through the parent's `pub(crate) use
host_net_http::*` re-export. Remaining host families (env/process/clock,
json/serdes) follow as their own slices before the mutually-recursive
value/control core is sub-partitioned by value shape.

The build-purity slice then isolated the bounded, effect-free static-output
classifier in `stage1/crates/axiomc/src/cranelift_backend/static_output_purity.rs`
and the backend-neutral build lowering evidence contract in
`stage1/crates/axiomc/src/project/build_contract.rs`. Unsupported or effectful
programs now fail closed instead of using compiler-time host state to create a
frozen artifact.

## Current Top Files

Snapshot updated 2026-07-12 after the #1254 completion audit:

| Rank | Current Rust file | Lines | Target package boundary | First extraction slice |
| ---: | --- | ---: | --- | --- |
| 1 | `stage1/crates/axiomc/src/cranelift_backend.rs` | 20,076 | `compiler.backend.native` | Runtime-intrinsic implementations live in `.../cranelift_backend/intrinsics.rs`, the compile-time evaluator in `.../cranelift_backend/evaluator.rs`, host-capability lowering in the `host_*` siblings, and static-output eligibility in `.../cranelift_backend/static_output_purity.rs`; the remaining work is sub-partitioning the mutually-recursive value/control core by value shape. |
| 2 | `stage1/crates/axiomc/src/project.rs` | 11,443 | `compiler.package_graph`, `compiler.commands`, `compiler.evidence` | Build lowering evidence now lives in `stage1/crates/axiomc/src/project/build_contract.rs`; continue splitting manifest/workspace loading, command orchestration, provenance/debug records, and artifact planning along package ownership. |
| 3 | `stage1/crates/axiomc/src/main.rs` | 10,401 | `compiler.commands` | Formatter reporting and edit planning now live in `stage1/crates/axiomc/src/formatter.rs`; continue moving command parsing, JSON envelope construction, check/build/run/test/doc/trace orchestration, and exit handling behind `docs/compiler-command-lsp-packages.md` APIs. |
| 4 | `stage1/crates/axiomc/src/codegen.rs` | 7,919 | `compiler.backend.generated_rust`, `compiler.backend.contracts` | Isolate generated-Rust compatibility emission from backend target selection and unsupported-feature contracts. |
| 5 | `stage1/crates/axiomc/src/syntax.rs` | 6,370 | `compiler.syntax`, `compiler.diagnostics` | Split lexer/parser, parse recovery, source spans, macros, and syntax diagnostics behind the syntax boundary. |
| 6 | `stage1/crates/axiomc/src/hir.rs` | 5,849 | `compiler.hir` | Generic inference and monomorphization now live in `stage1/crates/axiomc/src/hir/generics.rs`; public HIR model types now live in `stage1/crates/axiomc/src/hir/model.rs`; syntax-to-HIR type/literal lowering now lives in `stage1/crates/axiomc/src/hir/types.rs`; type-name, aggregate, and trait-use definition checks now live in `stage1/crates/axiomc/src/hir/definitions.rs`; function/method signatures and trait impl signature validation now live in `stage1/crates/axiomc/src/hir/signatures.rs`; capability analysis now lives in `stage1/crates/axiomc/src/hir/capabilities.rs`; expression typing helpers now live in `stage1/crates/axiomc/src/hir/expressions.rs`; ownership and borrow-state helpers now live in `stage1/crates/axiomc/src/hir/ownership.rs`; property clause checks now live in `stage1/crates/axiomc/src/hir/properties.rs`; reachability/call-graph discovery now lives in `stage1/crates/axiomc/src/hir/reachability.rs`; diagnostic recovery helpers now live in `stage1/crates/axiomc/src/hir/diagnostics.rs`; monomorphized symbol and intrinsic helpers now live in `stage1/crates/axiomc/src/hir/symbols.rs`; source-location helpers now live in `stage1/crates/axiomc/src/hir/source_locations.rs`; return-flow analysis now lives in `stage1/crates/axiomc/src/hir/control_flow.rs`; const-array length validation now lives in `stage1/crates/axiomc/src/hir/const_arrays.rs`; const-function validation now lives in `stage1/crates/axiomc/src/hir/const_functions.rs`; match lowering now lives in `stage1/crates/axiomc/src/hir/matches.rs`; enum variant constructor helpers now live in `stage1/crates/axiomc/src/hir/variants.rs`; async runtime intrinsic lowering now lives in `stage1/crates/axiomc/src/hir/async_runtime.rs`; map intrinsic lowering now lives in `stage1/crates/axiomc/src/hir/maps.rs`; HIR boundary regression tests now live in `stage1/crates/axiomc/tests/hir_unit.rs`; continue splitting remaining HIR helper clusters behind the package APIs in `docs/compiler-hir-ownership-capability.md`. |
| 7 | `stage1/crates/axiomc/src/hir/generics.rs` | 4,208 | `compiler.hir` | Keep generic call inference, trait-bound validation, aggregate monomorphization, and generic call rewriting isolated from the main HIR lowering facade. |

## Ratchet Ceilings

These ceilings are consumed by
`scripts/ci/report-compiler-source-monoliths.py --check-ratchet`. A PR that
adds lines above any ceiling fails `make stage1-compiler-source-monoliths`.
When an extraction PR shrinks a tracked monolith or top-file share, lower the
matching ceiling in this table in the same PR.

| Tracked item | Ceiling |
| --- | ---: |
| `summary.top_file_line_share` | 0.7275 |
| `summary.top_file_lines` | 66266 |
| `stage1/crates/axiomc/src/cranelift_backend.rs` | 20076 |
| `stage1/crates/axiomc/src/cranelift_backend/static_output_purity.rs` | 279 |
| `stage1/crates/axiomc/src/cranelift_backend/host_env_proc_clock.rs` | 586 |
| `stage1/crates/axiomc/src/cranelift_backend/host_json_serdes.rs` | 258 |
| `stage1/crates/axiomc/src/cranelift_backend/intrinsics.rs` | 917 |
| `stage1/crates/axiomc/src/cranelift_backend/evaluator.rs` | 4135 |
| `stage1/crates/axiomc/src/cranelift_backend/host_fs.rs` | 984 |
| `stage1/crates/axiomc/src/cranelift_backend/host_crypto.rs` | 783 |
| `stage1/crates/axiomc/src/cranelift_backend/host_net_http.rs` | 1043 |
| `stage1/crates/axiomc/src/hir.rs` | 5849 |
| `stage1/crates/axiomc/src/project.rs` | 11443 |
| `stage1/crates/axiomc/src/project/build_contract.rs` | 118 |
| `stage1/crates/axiomc/src/main.rs` | 10401 |
| `stage1/crates/axiomc/src/formatter.rs` | 219 |
| `stage1/crates/axiomc/src/formatter_tests.rs` | 137 |
| `stage1/crates/axiomc/src/codegen.rs` | 7919 |
| `stage1/crates/axiomc/src/syntax.rs` | 6370 |
| `stage1/crates/axiomc/src/hir/async_runtime.rs` | 188 |
| `stage1/crates/axiomc/src/hir/capabilities.rs` | 773 |
| `stage1/crates/axiomc/src/hir/const_arrays.rs` | 330 |
| `stage1/crates/axiomc/src/hir/const_functions.rs` | 117 |
| `stage1/crates/axiomc/src/hir/control_flow.rs` | 36 |
| `stage1/crates/axiomc/src/hir/definitions.rs` | 684 |
| `stage1/crates/axiomc/src/hir/diagnostics.rs` | 28 |
| `stage1/crates/axiomc/src/hir/expressions.rs` | 205 |
| `stage1/crates/axiomc/src/hir/generics.rs` | 4208 |
| `stage1/crates/axiomc/src/hir/maps.rs` | 124 |
| `stage1/crates/axiomc/src/hir/matches.rs` | 737 |
| `stage1/crates/axiomc/src/hir/model.rs` | 607 |
| `stage1/crates/axiomc/src/hir/ownership.rs` | 995 |
| `stage1/crates/axiomc/src/hir/properties.rs` | 167 |
| `stage1/crates/axiomc/src/hir/reachability.rs` | 161 |
| `stage1/crates/axiomc/src/hir/signatures.rs` | 471 |
| `stage1/crates/axiomc/src/hir/source_locations.rs` | 89 |
| `stage1/crates/axiomc/src/hir/symbols.rs` | 134 |
| `stage1/crates/axiomc/src/hir/types.rs` | 241 |
| `stage1/crates/axiomc/src/hir/variants.rs` | 188 |
| `stage1/crates/axiomc/src/registry.rs` | 2234 |
| `stage1/crates/axiomc/src/lib.rs` | 23 |

## Extraction Order

1. `compiler.backend.native`: start with helpers that are already aligned to
   `stage1/runtime-abi/direct-native-v0.json` rows. Each extraction should keep
   `make stage1-direct-native-runtime-abi-test` passing.
2. `compiler.backend.contracts`: move target selection and unsupported-feature
   contracts out of generated-Rust code before the final generated-Rust removal
   gate.
3. `compiler.hir`: generic inference/monomorphization, public HIR model types,
   syntax-to-HIR type/literal lowering, and type/aggregate definition collection
   are split; function/method signatures, trait impl signature validation,
   capability analysis, expression typing helpers, ownership/borrow helpers,
   property checks, reachability/call-graph discovery, diagnostic recovery
   helpers, monomorphized symbol/intrinsic helpers, source-location helpers,
   return-flow analysis, const-array validation, const-function validation,
   match lowering, enum variant constructor lowering, async runtime intrinsic
   lowering, and map intrinsic lowering are split; continue with remaining HIR
   helper clusters.
4. `compiler.commands` and `compiler.package_graph`: separate command envelopes
   from package loading so the snapshot bootstrap can invoke package APIs
   without Cargo assumptions.
5. `compiler.syntax` and `compiler.diagnostics`: keep public syntax and
   diagnostic fixtures stable while implementation files shrink.

## Host-Capability Slice Recipe (`compiler.backend.native`)

This is the durable handoff for the `cranelift_backend.rs` i64-lowering
decomposition so any agent can pick up the next slice without prior context.
The lowering is being peeled one host-capability family at a time into
`stage1/crates/axiomc/src/cranelift_backend/<name>.rs` siblings.

Slice status (update as PRs land):

- runtime intrinsics -> `intrinsics.rs` — #1377, merged
- compile-time evaluator -> `evaluator.rs` — #1378, merged
- filesystem -> `host_fs.rs` — #1379, merged
- crypto -> `host_crypto.rs` — #1380, merged
- net/http -> `host_net_http.rs` — #1381, merged
- env/process/clock -> `host_env_proc_clock.rs` — #1383, merged
- json/serdes -> `host_json_serdes.rs` — #1384, merged

All host-capability families are now extracted. The remaining work is
sub-partitioning the mutually-recursive value/control core by value shape
(see the recipe below).

### Value/Control-Core Sub-Partition Recipe

All host families are merged, so `cranelift_backend.rs` is now ~20,085 lines
(current ceiling 20,085) and the only un-extracted code is the value/control
core. Partition it by MIR **value/expr shape**, not by the call-graph hub. A
read-only `grep -oE 'fn lower_i64_[a-z_]+' cranelift_backend.rs | sort | uniq
-c` on origin/main shows the remaining 177 `lower_i64_*` fns cluster by shape,
e.g.: `option` (14), `result` (13), `map` (11), `enum` (11), `aggregate` (9),
`array` (7), `struct` (5), `string` (5), `tuple` (3), `literal` (2), `bool`
(4), `numeric` (1), `slice` (3), `projection` (3), `tagged` (1). These are the
candidate sub-slice seeds.

Reuse the host-family rules above (closure-selection, grep-siblings, wiring,
`private_interfaces`, validate + ratchet), with two extra constraints:

1. **Never move the recursive hub.** `lower_i64_expr`, `lower_i64_runtime_stmt`, and
   `lower_i64_return_value_expr` are the mutually-recursive entry points and stay in the
   parent. Only leaf value-shape lowerers that are *called by* the hub (not
   callers *of* the hub) may move. A fn whose body calls back into
   `lower_i64_expr`/`lower_i64_runtime_stmt`/`lower_i64_return_value_expr` is part of the hub and
   stays.
2. **Do not pick the literal/bool/numeric shape first.** The constants/literals
   are *not* a safe first extraction: the leaf helpers for that shape are shared
   across other value shapes and the hub paths, so moving them would orphan
   callers. The real helpers are `lower_i64_literal_value`,
   `lower_i64_literal_index`, `lower_i64_bool_value_expr`,
   `lower_i64_bool_value_compare`, `lower_i64_bool_literal_compare`,
   `lower_i64_bool_argument_expr`, and `lower_i64_numeric_literal` — all of which
   are referenced by functions outside the literal/bool/numeric cluster:

   - `lower_i64_literal_index` is referenced from at least 13 enclosing functions
     across slice/projection/map/string/expr/condition paths.
   - `lower_i64_bool_value_expr` is referenced from at least 16 enclosing functions
     across aggregate/body/runtime-let/assign/projection/return-value/option paths.
   - `lower_i64_bool_value_compare` and `lower_i64_bool_literal_compare` feed
     `lower_i64_condition`.
   - `lower_i64_bool_argument_expr` feeds call-arg/projection paths.
   - `lower_i64_numeric_literal` feeds both `lower_i64_literal_value` and
     `lower_i64_expr`.

   Before executing any value-shape slice, run the grep-siblings check: confirm
   every caller of each candidate fn is already in the group (or is the hub, which
   stays) and that no sibling — especially `evaluator.rs` — or the hub references a
   moved private type; if any candidate fails, fall back to the next leaf-most
   group (`tuple` or `struct`). Keep each value-shape slice to a single shape so the
   diff stays reviewable and the ratchet ceiling for `cranelift_backend.rs` only
   drops by that shape's lines.

The next code PR should choose a different verified small closure such as
`tuple` or `struct` only after the caller/sibling checks pass, then (a) move that
verified closure into a new `cranelift_backend/value_<shape>.rs` sibling (e.g.
`value_tuple.rs`) following the wiring + `private_interfaces` steps, (b) lower the
`cranelift_backend.rs` ceiling by the moved line count and add the new file's
ceiling in the Ratchet Ceilings table, and (c) mark that slice as merged in the
status list above.

To execute one slice:

1. **Find the closure.** Seed with the family's `fn` name tokens (for the next
   slice: `env`, `process`, `clock`; the one after: `json`, `serdes`). A
   private fn joins the slice iff *every* one of its callers is already in the
   slice, so nothing outside is orphaned. Exclude fns whose name matches a
   token but that sit in the recursive expr/stmt hub (anything that is a
   lowering entry point calling back through `lower_i64_expr` as a hub rather
   than a leaf) — those stay in the parent.
2. **Decide what moves.** Always move the closure `fn`s. For each non-`fn` item
   the family appears to own (structs, `SPIKE_*` statics, FFI guards), `grep`
   it across every `cranelift_backend/*.rs` sibling first: if only this family
   uses it, move it alongside its callers; **if any sibling — especially
   `evaluator.rs` — also uses it, leave it in the parent**, where the moved fns
   still reach it through `use super::*` (a child module sees parent-private
   items, including private statics). `host_crypto` moved its OpenSSL FFI Drop
   guards because they were crypto-exclusive; `host_net_http` left all
   `Spike{Http,Tcp,Udp}*` structs and `SPIKE_TCP_*`/`SPIKE_UDP_*` registries in
   the parent because the evaluator shares them, making that slice a pure
   function move.
3. **Wire it.** The new `cranelift_backend/<name>.rs` starts with `use
   super::*;`. Make every moved fn `pub(crate)`. Add `mod <name>;` and
   `pub(crate) use <name>::*;` immediately after the last existing sibling
   re-export in `cranelift_backend.rs` — this re-export is required so sibling
   modules that call back into the moved fns (e.g. `evaluator.rs` calling
   `http_get`/`spike_fs_*`/`spike_crypto_*`) keep resolving.
4. **Fix `private_interfaces`.** If a moved `pub(crate)` fn's *signature* names
   a parent-private type, raise that type to `pub(crate)` (as was done for
   `I64StaticBindings`, `I64HelperSignature`, and `I64NetResolveHost`). A
   parent-private type referenced only in a fn *body* needs no change.
5. **Validate and ratchet.** Run `cargo build -p axiomc` (must be
   warning-clean), `cargo test -p axiomc --lib` (expect only the pre-existing
   environment-dependent failures on developer machines: the `wasm32-wasip1`
   alias build and the socket-binding stdlib-http / proof-workload tests —
   confirm the count and names match a clean `main`), `make
   stage1-direct-native-runtime-abi-test`, `make self-hosting-spike-parity`,
   and `make stage1-compiler-source-monoliths-test`. In the same PR, lower the
   `cranelift_backend.rs`, `summary.top_file_lines`, and
   `summary.top_file_line_share` ceilings and add the new file's ceiling in the
   Ratchet Ceilings table above, or the fast lane fails.

Do not run `cargo fmt` on these backend files: formatting is not CI-enforced in
this repo, and reformatting the large files would swamp the code-motion diff.

## PR Rules

- Each extraction PR must cite the target AxiOM package boundary, not only the
  Rust module being split.
- Each PR must preserve the existing command JSON envelopes or list the
  intentional envelope delta.
- Each PR must run the package boundary command named in
  `docs/axiom-compiler-source-layout.md`.
- Direct-native backend extractions must also run
  `make stage1-direct-native-runtime-abi-test`.
- Generated-Rust compatibility extractions must not make `rust_source`
  required evidence for direct-native behavior.

## Rust Capture Check

This plan is about migration mechanics only. It does not define Axiom semantics
in terms of Rust files, Rust modules, Cargo, or Cranelift internals. AxiOM
package names and backend-neutral contracts remain the durable self-hosting
boundary.
