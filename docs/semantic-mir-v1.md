# Semantic MIR v1

Semantic MIR v1 is the backend-neutral executable contract between typed HIR
and a selected target. It is not Intent IR, a Rust enum serialization, or a
target instruction stream. Targets consume declared MIR features and either
lower them or return a stable Axiom diagnostic; they must not infer semantics
from host-language data structures.

## Package model

A document has `schema_version`, a deterministic `package_id`, source-span
table, semantic-node table, feature requirements, and modules. Identifiers are
stable `axiom://` ids derived from package identity, source identity, and an
ordinal in source order. A consumer must preserve ids and may add target-local
metadata only outside the semantic document.

Each module declares functions, aggregates, and entrypoints. A function owns
ordered blocks. A block has parameters, instructions, exactly one terminator,
and a source span. Values are introduced only by parameters or instructions;
uses refer to value ids. Control-flow edges pass block arguments explicitly,
so joins and loop headers do not depend on an implicit host-language phi node.

## Values, places, and operations

Values have an AxiOM type and provenance link. Places identify local storage or
an explicit projection: field, tuple element, index, dereference, or slice.
Instructions cover constants, copies, moves, borrows, loads, stores,
aggregate construction, calls, capability calls, arithmetic/comparison/logical
operations, casts, await, and explicit drop/defer scope operations. A borrow
records mutability, region/scope id, and its borrowed place. A move or drop
consumes ownership according to the declared type and scope; neither may be
silently replaced by target reference counting or garbage collection semantics.

Calls identify their callee, argument values, result value when any, required
effect kinds, and source/semantic provenance. Capability calls additionally
identify the required capability and may not execute during compilation.

## Control and terminal behavior

Terminators are `goto`, `branch`, `match`, `return`, `panic`, `unwind`, and
`unreachable`. Their successor edges carry ordered block arguments. `?` lowers
to an explicit result-match branch; loops use a header block and back-edge;
early return is `return`; and defer registers a cleanup scope whose actions run
on return, panic, or unwind according to its declared mode. Async boundaries
are explicit `await` and task/cancellation effect operations rather than an
implicit scheduler call.

## Backend support and diagnostics

Every target contract declares supported MIR feature ids, supported effect
kinds, and supported value/type features. A missing declaration is not
permission to lower by static evaluation. The target returns the stable
`backend.unsupported_mir_feature` diagnostic with the requested feature id,
target id, MIR node id, and source span. Build evidence records the selected
lowering mode and the feature decision for every rejected node.

## Provenance and migration

Each function, block, instruction, terminator, value, place, and cleanup scope
links to a source span and one or more semantic-node ids. HIR remains the
typed analysis layer; the existing HIR-shaped `axiomc::mir` JSON is a legacy
inspection projection and is not Semantic MIR v1. During migration it may be
emitted alongside v1, but backends must consume the v1 feature contract before
claiming executable lowering coverage. #1436 owns that consumption change.

## Conformance requirements

The v1 schema freezes the complete feature, terminator, and instruction
vocabularies for this version. The checker derives the expected sets from those
schema enums, and the snapshot covers every declared terminator and instruction
operation. The schema and snapshots must represent scalar calls, branches, loops,
match, `?`, mutation, early return, panic/defer, capability calls, aggregates,
and async boundaries. Fixtures must include valid and rejected documents,
deterministic id ordering, explicit unsupported-feature diagnostics, and no
Rust enum, layout, crate, Cargo, or backend implementation names.
