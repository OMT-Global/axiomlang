# RFC 0002: Write-Capability Boundary

Status: Draft

Governing issue: #391

## Summary

Axiom should expose filesystem mutation only through an explicit, package-root-scoped `fs:write` capability. The initial write surface is intentionally narrow: create/write/append/replace file content and create directories inside the package capability root. Copy semantics are specified as a future helper over the same boundary, but this RFC does not broaden host filesystem mutation or add ambient delete/rename authority.

## Motivation

Agentic workloads need to inspect compiler contracts before they run and rely on a stable answer to "may this package write here?" Existing read-side `fs` policy already scopes reads to the package root or `[capabilities].fs_root`; write access needs the same inspectable boundary without turning Axiom into a general host filesystem API.

The target user story is an agent package that can safely materialize generated files, cache artifacts, or copy declared package resources while reviewers and schedulers can reject workloads that request mutation outside their package-owned tree.

## Non-Goals

- No ambient filesystem mutation without `fs:write`.
- No absolute-path writes, parent-directory traversal, symlink escape, or writes outside the effective package capability root.
- No recursive delete, rename, chmod/chown, metadata mutation, temporary-directory allocation, or arbitrary host copy API in this slice.
- No weakening of AG0-AG5 gates, Python-exit blockers, generated-native safety checks, or existing read-side `fs` enforcement.

## Design

### Manifest contract

Filesystem reads and writes remain separate capability grants:

```toml
[capabilities]
fs = true          # read helpers such as read_file
"fs:write" = true # write/copy/mkdir helpers
fs_root = "data"  # optional, relative to the package root
```

If `fs_root` is absent, the effective filesystem capability root is the package root. If present, it must be a relative path that resolves inside the package root. The same root applies to both `fs` and `fs:write` so package authors cannot accidentally grant broader writes than reads.

`axiomc caps --json` must report `fs:write` as a distinct capability with the configured and effective root so agents can preflight mutation authority without parsing generated code.

### Allowed write-side operations

The stage1 write boundary permits only path-local operations whose target resolves inside the effective root:

- `write_file(path, content)` / `fs_write(path, content)` creates or truncates a file.
- `append_file(path, content)` / `fs_append(path, content)` appends to a file.
- `replace_file(path, content)` / `fs_replace(path, content)` atomically replaces file content when supported by the backend, otherwise behaves as a checked truncate/write.
- `create_file(path)` / `fs_create(path)` creates an empty file.
- `mkdir(path)` / `fs_mkdir(path)` creates one directory.
- `mkdir_all(path)` / `fs_mkdir_all(path)` creates a relative directory chain under the effective root.

A future `copy_file(src, dst)` helper is allowed only if both source and destination resolve inside the effective root. Copying from outside the root into the root, or from the root to outside, is denied. Until implemented, fixtures and contracts should reserve this behavior rather than implying broad host copy access.

### Denied behavior

The compiler/runtime boundary must deny:

- calling any write helper when `[capabilities].fs:write` is false or missing;
- absolute write targets;
- `..` traversal outside the effective root;
- symlink escapes from the package root or effective root;
- writes or copies whose parent directory resolves outside the effective root;
- writes larger than the stage1 filesystem size ceiling;
- any undeclared broad mutation helper.

Compile-time capability denial should use the existing structured capability diagnostic. Runtime path-policy denial should return the filesystem helper failure sentinel (`-1`) rather than mutating the host.

## Examples

Allowed package-local materialization:

```axiom
import "std/fs.ax"

print mkdir_all("scratch/generated")
print write_file("scratch/generated/out.txt", "ok")
```

Denied manifest omission:

```axiom
import "std/fs.ax"

print write_file("scratch/out.txt", "blocked")
```

Expected diagnostic:

```json
{
  "kind": "capability",
  "message": "call to \"fs_write\" requires [capabilities].fs:write = true"
}
```

Denied root escape, even with `fs:write`:

```axiom
import "std/fs.ax"

print write_file("../outside.txt", "blocked") == -1
```

Future copy semantics should follow the same shape:

```axiom
import "std/fs.ax"

print copy_file("templates/base.txt", "scratch/base.txt")
print copy_file("../host-secret.txt", "scratch/secret.txt") == -1
```

## Implementation Plan

1. Keep the manifest field named `"fs:write"` and map compiler-known write intrinsics to that capability.
2. Ensure `std/fs.ax` write helpers require `fs:write` while read helpers require only `fs`.
3. Share package-root/effective-root canonicalization between read and write paths.
4. Keep generated runtime helpers returning `-1` on path-policy denial.
5. Add/maintain compile-fail fixtures for missing `fs:write` and executable fixtures for root-scoped allowed/denied path behavior.
6. Reserve `copy_file` in docs/contracts until the helper is implemented with both endpoints scoped to the effective root.

## Validation

- `stage1/conformance/fail/stdlib_fs_write_without_capability` proves `std/fs.ax` write helpers are denied without `fs:write`.
- `stage1/examples/stdlib_fs_write` proves allowed package-local write/mkdir behavior.
- Rust coverage should continue to prove scoped runtime denials such as writes outside `fs_root` returning `-1`.
- Relevant gates:
  - `make stage1-conformance`
  - `cargo test --manifest-path stage1/Cargo.toml -p axiomc fs_write`

## Compatibility

Existing read-only packages are unchanged. Packages that already declare `"fs:write" = true` keep their current helper access, subject to the documented root boundary. Any future removal of broad delete helpers should be staged separately with explicit diagnostics and migration notes.

## Security And Capability Impact

This RFC tightens the public contract around host mutation: write authority is explicit, inspectable, package-root-scoped, and denied by default. It does not grant arbitrary host filesystem access and does not weaken deterministic compiler gates. Agent schedulers can treat `fs:write` as a high-signal mutation capability and reject workloads whose requested root or helpers exceed policy.

## Alternatives

- Reuse `[capabilities].fs = true` for both reads and writes. Rejected because read access is lower risk than mutation and agents need to distinguish them.
- Allow absolute host paths behind `fs:write`. Rejected because package-root scoping is the safety boundary.
- Add broad copy/delete/rename APIs now. Rejected because the issue asks for a narrow RFC/fixture slice, not broad filesystem mutation.

## Open Questions

- Should delete helpers be moved behind a later `fs:delete` capability or retained as `fs:write` operations with stricter docs?
- Should `copy_file` be implemented as a stdlib helper over `read_file` + `write_file`, or as a dedicated intrinsic with size and metadata semantics?
- Should package registries be allowed to declare a narrower generated-artifact subroot by convention, such as `fs_root = "scratch"`?
