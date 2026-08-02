# Stage1 benchmark reference: stdlib_sync

This ownership-shaped synchronization reference mirrors
`stage1/examples/stdlib_sync`: mutex replacement, once-cell state, and a
single-slot channel. It is categorized under the benchmark's historical
`concurrency` build budget, but it is not evidence of Axiom scheduler, thread,
blocking, or dynamic-channel runtime support. The Axiom workload currently
qualifies as bounded static output without generated Rust.
