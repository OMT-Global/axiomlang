# RFC 0001: Traits, bounded generics, and dynamic dispatch

Status: Draft
Governing issue: [OMT-Global/axiom#216](https://github.com/OMT-Global/axiom/issues/216)
Closure: This RFC scopes follow-up implementation work and deliberately does not close #216.

## Summary

Introduce traits as Axiom's named interface mechanism, but land the feature as
small, independently testable slices instead of one Rust-parity jump. The first
implementation should support trait declarations, explicit generic bounds, and
monomorphized static dispatch. Dynamic dispatch, coherence/orphan rules,
blanket impls, supertraits, and stdlib trait stabilization should follow only
after the static surface has conformance coverage.

## Problem

Stage1 already supports explicit generic functions, structs, and enums, but all
generic parameters are unconstrained. Library authors cannot express that a type
argument must support equality, formatting, hashing, iteration, serialization,
or any other operation. This blocks operator overloading, iteration protocols,
real collection constraints, and typed dispatch surfaces.

The original issue also asks for `dyn Trait`, coherence, blanket impls, and a
stdlib trait. Those are related, but they touch parser syntax, type checking,
method lookup, monomorphization, runtime representation, codegen, and standard
library compatibility. Treating all of that as one implementation PR would make
review and rollback too risky.

## Goals

- Add a testable trait declaration syntax.
- Allow explicit bounds on generic declarations, e.g. `fn f<T: Eq>(x: T): bool`.
- Allow `impl Trait for Type` blocks for named local types.
- Type-check calls through bounded generic parameters for methods required by
  the declared trait.
- Keep first-slice dispatch static and monomorphized, matching current generic
  lowering.
- Produce clear diagnostics for unsupported stretch forms until they are
  intentionally implemented.

## Non-goals for the first implementation slice

- `dyn Trait` values, vtables, trait objects, object safety, or boxed dispatch.
- Blanket impls such as `impl<T: Eq> Hash for T`.
- Supertraits and associated types.
- Orphan/coherence rules beyond local `impl Trait for LocalType` validation.
- Operator overloading or compiler-known operator traits.
- Stabilizing a broad stdlib trait hierarchy.

## Proposed staged slices

### Slice 1: syntax and explicit static bounds

Parser and AST:

```axiom
trait Eq {
fn eq(self, other: Self): bool
}

struct Point {
x: int
y: int
}

impl Eq for Point {
fn eq(self, other: Point): bool {
return self.x == other.x
}
}

fn same<T: Eq>(left: T, right: T): bool {
return left.eq(right)
}
```

Requirements:

- `trait Name { fn method(...): Type }` parses as a top-level item.
- Default method bodies are rejected with an explicit diagnostic in slice 1.
- Bounds parse on function, struct, enum, and type alias generic parameters,
  but only function bounds are semantically active in slice 1.
- `Self` is valid in trait method signatures and is rewritten to the concrete
  impl type when checking an implementation.
- `impl Trait for Type { ... }` parses separately from existing inherent
  `impl Type { ... }` blocks.

### Slice 2: trait impl checking and bounded method calls

Checker/HIR:

- Validate that every required trait method has an implementation with the
  expected name, arity, parameter types, and return type after `Self`
  substitution.
- Reject impls for unknown traits or unknown concrete types.
- Reject `impl Trait for ImportedType` until coherence is designed.
- During generic function monomorphization, require each explicit type argument
  to have an impl for every bound on the type parameter.
- Permit method calls on generic parameters only when the method is available
  through an active bound.

### Slice 3: stdlib seed trait and example

Stdlib/docs/examples:

- Add a minimal trait, preferably `Eq`, because scalar equality already exists
  and no formatting protocol is needed to prove the type-system path.
- Add one example and one conformance pass fixture that call a bounded generic
  function with at least one user-defined type.
- Add compile-fail fixtures for missing impls, missing trait methods, signature
  mismatches, and method calls with no satisfying bound.

### Slice 4: dynamic dispatch RFC/implementation

`dyn Trait` needs a separate accepted design before code lands. That design must
settle object safety, ownership/reference shape (`dyn Trait` by value versus
behind `ptr`/`Box`), vtable layout, codegen representation, nullability, and the
capability/runtime implications of crossing host boundaries with trait objects.

## Diagnostics required before stretch work

Until later slices land, the compiler should reject these forms with explicit
messages rather than parse ambiguity:

- trait default method bodies;
- `dyn Trait` type expressions;
- blanket impls;
- supertraits;
- associated types;
- trait aliases;
- impls for imported or primitive types.

## Validation plan

Implementation PRs should add tests in the smallest relevant layer:

- Rust parser tests or conformance fail fixtures for malformed trait syntax and
  unsupported stretch forms.
- HIR/checker tests or conformance fail fixtures for missing impls and signature
  mismatches.
- A conformance pass fixture demonstrating `Eq` on a named user type through a
  bounded generic function.
- Generated-Rust smoke coverage through the existing `axiomc test` or project
  conformance runner.

## Compatibility and migration

Existing unbounded generics remain valid. Existing inherent `impl Type { ... }`
method blocks keep their meaning. The new `impl Trait for Type { ... }` form is
additive and deliberately syntactically distinct from inherent impls.

No source migration is required until stdlib traits become stable enough for
operator protocols or collection APIs to depend on them.

## Security, determinism, and host boundaries

Static trait bounds do not introduce new runtime authority. Dynamic dispatch may
change runtime representation and host ABI behavior, so it remains outside the
first slice and must receive a dedicated RFC section before implementation.

## Alternatives considered

- **Implement the full issue at once.** Rejected because it couples parser,
  checker, runtime representation, stdlib, and coherence policy into one large
  unreviewable change.
- **Use ad hoc compiler-known interfaces only.** Rejected because it would solve
  immediate operator or collection cases while postponing the general interface
  mechanism the language needs.
- **Delay all trait parsing until dynamic dispatch is designed.** Rejected
  because static bounded generics are useful on their own and align with the
  current monomorphized generic architecture.

## Unresolved questions

- Should trait methods require explicit `self`, or should receiver syntax be a
  distinct grammar form?
- What is the final owned/borrowed receiver model once full `&mut T` and string
  view semantics land?
- What coherence rule is acceptable for packages once a registry exists?
- What minimal boxed/pointer type should carry `dyn Trait` in stage1?
