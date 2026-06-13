# Stage1 Debug Map

`axiomc build --debug` writes Axiom source-position sidecars next to the
generated artifact. The generated-Rust backend also asks `rustc` for native
debuginfo; the Cranelift spike currently emits sidecars only and records that
native Axiom DWARF is not present yet.

- `<artifact>.debug-map.json` maps generated Rust statement lines back to
  `.ax` source file, line, and column positions.
- `<artifact>.debug-manifest.json` binds the native binary, generated Rust,
  debug map, backend-native debug settings, source hashes, and mapping counts.
- Direct-native debug maps use
  `axiom.stage1.direct_native.debug_map.v1` and bind Axiom module, function,
  and statement spans directly to the native binary. They do not contain
  `generated_rust` or generated line records.
- Direct-native debug manifests use
  `axiom.stage1.direct_native.debug_manifest.v1`, set
  `artifact_class = "native_binary"`, and omit `generated_rust`,
  `generated_rust_hash`, and `rustc`.

This is an interim sidecar bridge. Generated-Rust DWARF line tables still point
at generated Rust, and Cranelift debug builds do not emit Axiom DWARF yet, so
debugger integrations should translate generated Rust frames through the debug
map instead of assuming the binary contains native `.ax` line records.

`axiom.stage1.debug_manifest.v1` continues to require `generated_rust` and
`generated_rust_hash` because that legacy mapping is mediated through generated
Rust line records. Direct-native builds use the successor direct-native schema
versions above, so generated Rust is not part of their debug integrity
envelope. The provenance sidecar is separate: generated Rust is not a required
direct-native provenance artifact, so provenance for direct-native builds
follows the native artifacts actually emitted by the backend.

## Build

```sh
axiomc build --debug --json
```

The JSON payload includes `binary`, `generated_rust`, `debug_map`, and
`debug_manifest`. `generated_rust` is a path for generated-Rust builds and
`null` for direct-native builds. Use those paths as the source of truth; do not
derive sidecar paths by hand in tooling.

## LLDB

The simplest LLDB workflow is to stop in the generated Rust frame, read the
current generated Rust location, and translate that line through the debug map:

```sh
lldb <binary>
(lldb) breakpoint set --name main
(lldb) run
(lldb) frame info
```

Use the `line_entry.line` from `frame info` as `generated_line`, then resolve
that line through the debug manifest:

```sh
python3 scripts/debug/axiom-debug-map.py resolve \
  --manifest <artifact>.debug-manifest.json \
  --generated-line <line_entry.line>
```

LLDB command scripts should keep this translation explicit. Until the direct
native backend emits Axiom line tables, remapping generated Rust paths to Axiom
paths would overstate the debug format.

## GDB

GDB follows the same generated-line translation model:

```sh
gdb <binary>
(gdb) break main
(gdb) run
(gdb) frame
```

Use the generated Rust line shown by `frame` as `generated_line`, then resolve
it through the checked-in helper:

```sh
python3 scripts/debug/axiom-debug-map.py resolve \
  --debug-map <artifact>.debug-map.json \
  --generated-line <frame-line>
```

A GDB Python command can call `gdb.selected_frame().find_sal().line`, invoke the
same resolver, and print the mapped `.ax` span.

## Tooling Contract

Consumers should treat `debug_manifest` as the integrity envelope:

- `binary_hash` and `generated_rust_hash` identify the exact artifacts.
- `generated_rust` and `generated_rust_hash` remain required in
  `axiom.stage1.debug_manifest.v1`.
- Direct-native manifests identify the native artifact with `binary_hash` and
  `artifact_class = "native_binary"` instead of a generated Rust hash.
- `source_files[*].source_hash` identifies the `.ax` inputs.
- `native_debug.axiom_dwarf` is the backend-neutral signal for whether the
  binary contains native Axiom DWARF line tables.
- `rustc` is retained for generated-Rust compatibility; direct-native debug
  manifests omit it because `rustc` is not the native debug producer.
- `source_files[*].mapping_count` lets tools detect missing or unexpectedly
  sparse source mappings.

If `native_debug.axiom_dwarf` is `false`, tools must report that stepping is
mediated through the sidecar map and not through native Axiom DWARF.

## Native DWARF Readiness

The direct-native DWARF closure check is intentionally fail-closed until the
backend emits Axiom line tables:

```sh
python3 scripts/debug/check-axiom-dwarf.py verify \
  --manifest <artifact>.debug-manifest.json
```

The checker first requires the manifest `binary_hash` to match the bytes at the
manifest `binary` path, then requires both a manifest claim
(`native_debug.axiom_dwarf = true`) and object debug metadata showing non-empty
DWARF line/info sections plus an `.ax` source path reported by either
`llvm-dwarfdump --show-sources` or the generic `dwarfdump` fallback. Current
generated-Rust and Cranelift spike builds are expected to fail this check
honestly. Passing this check is not the whole debugger acceptance bar by
itself, but it is the minimum artifact-level evidence a future #230 closure PR
must satisfy before claiming native Axiom DWARF support.
