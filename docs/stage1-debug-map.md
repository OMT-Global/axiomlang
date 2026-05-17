# Stage1 Debug Map

`axiomc build --debug` currently emits Rust debuginfo for the generated Rust
shim and writes Axiom source-position sidecars next to the generated artifact:

- `<artifact>.debug-map.json` maps generated Rust statement lines back to
  `.ax` source file, line, and column positions.
- `<artifact>.debug-manifest.json` binds the native binary, generated Rust,
  debug map, rustc debug settings, source hashes, and mapping counts.

This is an interim generated-Rust bridge. Native DWARF line tables still point
at generated Rust, so debugger integrations should translate generated Rust
frames through the debug map instead of assuming the binary contains native
`.ax` line records.

## Build

```sh
axiomc build --debug --json
```

The JSON payload includes `binary`, `generated_rust`, `debug_map`, and
`debug_manifest`. Use those paths as the source of truth; do not derive sidecar
paths by hand in tooling.

## LLDB

The simplest LLDB workflow is to stop in the generated Rust frame, read the
current generated Rust location, and translate that line through the debug map:

```sh
lldb <binary>
(lldb) breakpoint set --name main
(lldb) run
(lldb) frame info
```

Use the `line_entry.line` from `frame info` as `generated_line`. A helper script
can then load the debug map and print the matching Axiom span:

```python
import json

def axiom_span(debug_map_path, generated_line):
    with open(debug_map_path, "r", encoding="utf-8") as handle:
        payload = json.load(handle)
    for mapping in payload["mappings"]:
        if mapping["generated_line"] == generated_line:
            return f'{mapping["source"]}:{mapping["line"]}:{mapping["column"]}'
    return None
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
it through `debug_map` with the same JSON lookup shown above. A GDB Python
command can call `gdb.selected_frame().find_sal().line`, load the debug map,
and print the mapped `.ax` span.

## Tooling Contract

Consumers should treat `debug_manifest` as the integrity envelope:

- `binary_hash` and `generated_rust_hash` identify the exact artifacts.
- `source_files[*].source_hash` identifies the `.ax` inputs.
- `rustc.axiom_dwarf` is `false` for the generated-Rust backend.
- `source_files[*].mapping_count` lets tools detect missing or unexpectedly
  sparse source mappings.

If `rustc.axiom_dwarf` is `false`, tools must report that stepping is mediated
through the sidecar map and not through native Axiom DWARF.
