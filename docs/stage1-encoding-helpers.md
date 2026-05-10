# Stage1 Encoding Helpers

`std/encoding.ax` provides deterministic percent-encoding helpers:

- `url_component_encode(value)` encodes UTF-8 bytes outside the unreserved
  URL component set.
- `url_component_decode(value)` returns `Option<string>` and rejects malformed
  percent escapes or invalid UTF-8.
- `path_segment_encode(value)` uses the same percent-encoding contract for one
  path segment, including escaping `/`.
- `query_pair_encode(name, value)` returns `name=value` with both sides encoded
  as URL components.
- `path_join_segment(base, segment)` appends one encoded segment to a base path,
  inserting exactly one `/` separator when the base is non-empty.

These helpers are pure string utilities and do not grant network or filesystem
capabilities.
