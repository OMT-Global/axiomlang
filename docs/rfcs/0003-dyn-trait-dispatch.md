# RFC 0003: Dynamic trait dispatch

Status: Draft
Governing issue: [OMT-Global/axiomlang#626](https://github.com/OMT-Global/axiomlang/issues/626)

## Summary

Add dynamic dispatch through `dyn Trait` values after static trait dispatch has
landed. The dynamic surface is deliberately small: a `dyn Trait` value is an
explicit borrowed trait object that pairs a data pointer with a trait-specific
method table. It is nullable only when wrapped in `Option<dyn Trait>`, cannot
cross host boundaries in stage1, and only supports object-safe trait methods.

This design defines the semantic contract that implementation PRs must follow.
It does not make `dyn Trait` syntax available by itself; until the
implementation lands, the existing diagnostic remains correct.

## Goals

- Define the stage1 `dyn Trait` source shape before implementation.
- Keep dynamic dispatch Axiom-neutral and separate from generated-Rust details.
- Preserve the existing static trait dispatch behavior.
- Make nullability, ownership, vtable layout, and host-boundary behavior
  reviewable before any runtime representation ships.
- Define testable gates for parser, checker, MIR, codegen, and conformance.

## Non-goals

- Blanket impls, supertraits, associated types, trait aliases, or operator
  traits.
- Dynamic dispatch for generic trait methods.
- Passing trait objects through FFI, process, network, or serialized artifact
  boundaries.
- Making `dyn Trait` the default dispatch mode for bounded generics.
- Exposing the vtable layout as a stable public ABI.

## Source Model

The implementation will accept `dyn Trait` only in type position. A trait object
value borrows an existing concrete value that implements the trait:

```axiom
trait Render {
fn render(&self): string
}

struct Label {
text: string
}

impl Render for Label {
fn render(&self): string {
return self.text
}
}

fn show(item: dyn Render): string {
return item.render()
}

let label: Label = Label { text: "ready" }
print show(label as dyn Render)
```

The cast form is explicit because dynamic dispatch changes representation and
runtime cost. Implicit conversion from a concrete value to `dyn Trait` is a
non-goal for stage1.

## Ownership And Reference Shape

`dyn Trait` is a borrowed object reference in stage1. It does not own the
concrete value. Moving or dropping the concrete value while a `dyn Trait`
borrow is live is rejected by the existing borrow-state machinery.

The first implementation should model the source type as:

- `dyn Trait`: immutable borrowed trait object.
- `&mut dyn Trait`: deferred until mutable reference receiver rules are stable.
- `Box<dyn Trait>` or owned trait objects: deferred until Axiom has an owned
  heap allocation surface.

The borrowed object has the same lifetime boundary as other borrowed slices and
borrowed aggregate fields in stage1: it must be derived from a local value or
borrowed parameter whose origin is still live, and it cannot escape unless the
function return can be tied to a borrowed input.

## Object Safety

A trait is object-safe in stage1 when every required method satisfies all of the
following rules:

- The method has a non-consuming immutable receiver represented by `&self`.
- The method is not generic.
- The method does not mention `Self` except in the receiver.
- Parameters and return types are concrete stage1 types, borrowed slices, or
  other already object-safe `dyn Trait` references.
- The method does not require compile-time const evaluation.
- The method is not `property`, `async`, or `extern`.

Current stage1 `self` receivers are by-value receivers. A by-value `self`
method is not object-safe for borrowed `dyn Trait` because dispatch would have
to move out of the opaque borrowed data pointer. The first dynamic-dispatch
implementation must land borrowed receiver syntax and checking before allowing
object-safe method calls. By-value receivers can only become object-safe later
if Axiom adds an owned trait-object representation such as `Box<dyn Trait>`.

The checker rejects `dyn Trait` creation for non-object-safe traits with a
diagnostic naming the first failing method and rule.

## Vtable Contract

A `dyn Trait` value has two implementation-level fields:

- `data`: an opaque pointer to the borrowed concrete value.
- `vtable`: a trait-specific method table for that concrete implementation.

The vtable contains one function entry per object-safe trait method in
declaration order. Each entry receives an immutable erased borrow of the
concrete value plus the method's explicit arguments and returns the declared
result. Vtable entries must not move from borrowed storage and must not
reinterpret a consuming by-value receiver as a borrowed receiver. The method
table is generated per `(Trait, ConcreteType)` implementation.

The declaration order is part of the compiler contract so method lookup is
deterministic, but the physical layout is not a public ABI. Generated Rust,
Cranelift, or a future native backend may represent the pair differently as long
as the Axiom semantics above are preserved.

## Nullability

Plain `dyn Trait` is non-null. Optional trait objects use the existing option
surface:

```axiom
let maybe: Option<dyn Render> = Some(label as dyn Render)
```

There is no implicit null, sentinel data pointer, or nullable vtable. A missing
object must be represented as `None`.

## Host Boundaries And Capabilities

Trait objects are process-local runtime values in stage1. They cannot cross:

- FFI boundaries.
- Network or HTTP request/response surfaces.
- Process command argument or environment surfaces.
- Registry package metadata, lockfiles, generated schemas, or other artifacts.

This rule is independent of capability grants. A package with `ffi = true` or
`net = true` still cannot pass a `dyn Trait` object through those host
boundaries. The diagnostic should say that trait objects are not serializable
or host-stable in stage1.

## MIR And Backend Requirements

The MIR representation should make dynamic dispatch explicit rather than hiding
it behind ordinary function calls. The minimum new shapes are:

- A type form for `DynTrait(TraitId)`.
- An expression or cast node for `Concrete as dyn Trait`.
- A method-call node that records trait, method, data value, and vtable source.

The generated-Rust backend may lower the object pair to Rust structs and
function pointers, but Rust's `dyn Trait` syntax must not define the Axiom
contract. Future native backends must be able to lower the same MIR without a
semantic rewrite.

## Diagnostics

The implementation must add pass and fail coverage for:

- Creating a `dyn Trait` from a concrete type with a matching impl.
- Calling an object-safe method through `dyn Trait`.
- Rejecting `dyn Trait` for unknown traits.
- Rejecting concrete values that do not implement the trait.
- Rejecting non-object-safe traits.
- Rejecting trait-object escape after the borrowed concrete value is no longer
  live.
- Rejecting host-boundary use of trait objects.
- Preserving the current clear diagnostic until the implementation lands.

## Implementation Slices

Implementation should land in small PRs:

1. Parser and type representation for `dyn Trait` plus object-safety checks.
2. Borrowed `&self` receiver syntax, lowering, and diagnostics for rejecting
   by-value receivers in borrowed trait-object dispatch.
3. Explicit `as dyn Trait` construction and borrow-origin validation.
4. MIR representation for dynamic method calls.
5. Generated-Rust lowering through opaque data pointers and method tables.
6. Conformance pass/fail fixtures and docs updates.

The issue should not close until all acceptance criteria in #626 are satisfied:
design, diagnostics, implementation, and conformance coverage.
