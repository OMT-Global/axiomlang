# Stage1 Outcome Helpers

`std/outcome.ax` provides small generic helpers for the existing
`Option<T>` and `Result<T, E>` language forms:

- `option_is_some<T>(value)` and `option_is_none<T>(value)` return booleans.
- `option_unwrap_or<T>(value, fallback)` returns the contained value or the
  supplied fallback.
- `result_is_ok<T, E>(value)` and `result_is_err<T, E>(value)` return
  booleans.
- `result_unwrap_or<T, E>(value, fallback)` returns the `Ok` payload or the
  supplied fallback.

The helpers are implemented in Axiom on top of `match`; they do not introduce
panic-based unwraps or host runtime behavior.
