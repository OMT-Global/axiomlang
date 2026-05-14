---
title: "stdlib: std/io.ax stdin reading (readline / read_to_string)"
labels: [stage1, area:stdlib, lane:daedalus]
parent: null
---

`std/io.ax` currently exposes `eprintln(text: string): int` only — there is no way for an AxiOM program to read from stdin. CLI proof workload (#620 → #101 → #620) needs interactive input or piped input to be honest about "real CLI".

## Scope

- New ungated host intrinsic `io_readline(): Option<string>` that reads one line from stdin (returns `None` on EOF).
- New ungated host intrinsic `io_read_to_string(): string` that reads stdin to EOF.
- `std/io.ax` wrappers `readline()` and `read_to_string()`.
- Like `eprintln`, no capability gate — stdio is ambient.

## Acceptance

- Pass fixture: piped input → `readline` echoes each line via `print`.
- Pass fixture: EOF-only stdin returns `None`.
- The behavior under a closed stdin (programmatically detached) is deterministic — document it in `docs/stage1.md`.

## Out of scope

- Tokenized / structured input parsing (use `std/json.ax` for that).
- Async stdin — separate follow-up if needed.
