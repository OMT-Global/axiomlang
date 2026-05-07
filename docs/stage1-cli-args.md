# Stage1 CLI Arguments

`std/cli.ax` exposes arguments forwarded to a generated stage1 binary:

- `args(): [string]` returns all forwarded arguments.
- `arg_count(): int` returns the number of forwarded arguments.
- `arg(index): Option<string>` returns one argument or `None` when the index is
  negative or out of range.

`axiomc run` forwards arguments after `--`:

```bash
axiomc run stage1/examples/stdlib_cli -- alpha beta
```

The surface is ambient process input and does not require a filesystem,
network, process, environment, clock, or crypto capability.
