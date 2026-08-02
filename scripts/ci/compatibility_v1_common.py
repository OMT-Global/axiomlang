#!/usr/bin/env python3
"""Shared lexical contracts for Compatibility v1."""

from __future__ import annotations

import re


SEMVER_PATTERN = r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
SEMVER = re.compile(rf"^{SEMVER_PATTERN}$")

# These spellings describe host/Rust representation rather than logical AxiOM
# semantics. Keep this centralized so extraction and comparison cannot drift.
RUST_CAPTURE = re.compile(
    r"""(?ix)
    (?:
        \brust\b
      | \brustc\b
      | \bcargo\b
      | \bcranelift\b
      | \bgenerated[-_\s]*rust\b
      | \bserde\b
      | \bcrate\b
      | \bstd\s*::
      | \bcore\s*::
      | \b[a-z_][a-z0-9_]*\s*::\s*[a-z_][a-z0-9_]*\b
      | \bvec\s*<
      | \busize\b
      | \bisize\b
      | \brepr\s*(?:\(|[_\s]+)
      | \bextern\s*(?:"\s*c\s*"|c\b)
      | \benum[-_\s]+(?:layout|discriminants?)\b
      | \btarget[-_\s]*pointer[-_\s]*width\b
      | \bpointer[-_\s]*width\b
      | \balign(?:ment|[-_\s]*of)?\b
      | \b(?:native|host|physical|memory)(?:[-_\s]+(?:native|host|physical|memory))*[-_\s]+layout\b
    )
    """
)


def captures_rust_detail(value: str) -> bool:
    return RUST_CAPTURE.search(value) is not None


def reject_rust_detail(value: str, label: str) -> None:
    if captures_rust_detail(value):
        raise ValueError(f"{label} captures a Rust implementation detail")
