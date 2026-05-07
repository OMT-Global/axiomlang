# Stage1 Collection Lookup Helpers

`std/collections.ax` includes bounded map lookup helpers:

- `contains_key<K, V>(values, key)` returns whether a map contains a key.
- `get<K, V>(values, key)` returns `Option<V>` instead of panicking when the key
  is absent.

Both helpers consume the map in the current stage1 ownership model. They are
intended as a safe lookup surface while the language still lacks borrowed map
views and mutable collection APIs.
