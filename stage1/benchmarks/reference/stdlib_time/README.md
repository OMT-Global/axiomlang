# Stage1 benchmark reference: stdlib_time

This host-I/O reference mirrors `stage1/examples/stdlib_time`. The Axiom, Go,
and Rust programs read a wall clock, perform a zero-duration sleep, and emit the
same four boolean lines. The workload is backed by direct-native clock runtime
evidence and does not rely on compiler-time evaluation.
