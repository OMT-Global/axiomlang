#!/usr/bin/env python3
"""Reject legacy fixture tables without rejecting the sealed rejection test.

The exception is the complete, byte-pinned helper + negative test from #1682,
not a line, marker, function name, or caller-controlled skip. Any change to that
contract requires review of this digest as well as the Rust test. All other
source bytes retain the previous literal legacy-table prohibition.
"""
import hashlib
from pathlib import Path
import sys

START = b"// Validate the same bytes that reach disk, so fixture mistakes fail at setup,\n"
END = b"\nfn copy_fixture("
CONTRACT_SHA256 = "23d2c7624d0e64cc939c34315c9fd91444e5ffea1375b5ee905cf662e0eabc35"
LEGACY = b"[unsafe_rationale]"


def check(source: bytes) -> list[str]:
    errors = []
    if source.count(START) != 1:
        return ["expected exactly one manifest-fixture rejection contract"]
    start = source.index(START)
    end = source.find(END, start)
    if end < 0:
        return ["manifest-fixture rejection contract terminator missing"]
    contract = source[start:end]
    if hashlib.sha256(contract).hexdigest() != CONTRACT_SHA256:
        return ["manifest-fixture rejection contract changed; review its test semantics and digest"]
    # Preserve byte offsets and line numbering in diagnostics.
    remaining = source[:start] + bytes(10 if c == 10 else 32 for c in contract) + source[end:]
    for line, content in enumerate(remaining.splitlines(), 1):
        if LEGACY in content:
            errors.append(f"{line}: legacy [unsafe_rationale] outside sealed rejection test")
    return errors


def main() -> int:
    root = Path(__file__).resolve().parents[2]
    source = root / "stage1/crates/axiomc/tests/cranelift_backend.rs"
    for error in (errors := check(source.read_bytes())):
        print(f"{source}: {error}", file=sys.stderr)
    return bool(errors)


if __name__ == "__main__":
    sys.exit(main())
