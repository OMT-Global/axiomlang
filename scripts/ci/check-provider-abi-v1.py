#!/usr/bin/env python3
"""Validate the Provider ABI v1 security contract and reference C fixture."""
import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

R = Path(__file__).resolve().parents[2]
S = R / "stage1/compiler-contracts/schemas/axiom.provider-abi.v1.schema.json"
V = R / "stage1/compiler-contracts/snapshots/provider-abi-v1.json"
C = R / "stage1/compiler-contracts/fixtures/provider-abi-v1/reference-provider.c"

FIXTURES = {
    "c-reference-descriptor-provider": "positive", "version-incompatible": "negative",
    "missing-symbol": "negative", "untrusted-library": "negative",
    "capability-denied": "negative", "null-invalid-stale-handle": "negative",
    "borrowed-buffer-retention": "negative", "provider-fault-quarantine": "fault",
    "owned-buffer-release-leak": "leak", "cancel-drain": "negative",
}

def need(condition, message):
    if not condition:
        print(message, file=sys.stderr)
        raise SystemExit(1)

def exact(value, expected, message):
    need(value == expected, message)

def validate(v, s):
    exact(s.get("type"), "object", "schema envelope drift")
    need(s.get("additionalProperties") is False, "schema permits unknown contract fields")
    required = {"schema_version", "contract", "issue", "negotiation", "safe_surface", "handles", "buffers", "operations", "errors", "loading", "audit", "fixtures"}
    exact(set(s.get("required", [])), required, "schema required surface drift")
    exact(set(s.get("properties", {})), required, "schema property surface drift")
    exact((v.get("schema_version"), v.get("contract"), v.get("issue")), ("axiom.provider-abi.v1", "runtime.provider_abi", 1453), "identity drift")
    n = v.get("negotiation", {})
    exact(n.get("entrypoint"), "axiom_provider_v1", "entrypoint drift")
    exact(n.get("version"), {"major": 1, "minor": "provider_lte_runtime"}, "version negotiation drift")
    exact(n.get("features"), "declared_unique_subset", "feature discovery drift")
    exact(n.get("required_symbols"), ["describe", "call", "release_owned_buffer", "close_handle"], "required symbol contract drift")
    exact(n.get("incompatible"), "fail_closed_before_call", "version incompatibility accepted")
    safe = v.get("safe_surface", {})
    exact(safe.get("exports"), ["provider_descriptor", "opaque_handle_u64", "owned_bytes", "borrowed_bytes", "owned_text_utf8", "borrowed_text_utf8", "structured_error", "cancellation_token"], "safe surface drift")
    exact(safe.get("forbidden"), ["raw_pointer", "address", "allocator_callback", "retained_callback", "unbounded_length"], "raw-pointer escape accepted")
    h = v.get("handles", {})
    exact(h.get("representation"), "nonzero_provider_scoped_generation_tagged_u64", "handle representation drift")
    exact(h.get("create"), "caller_owns", "handle creation ownership drift")
    exact(h.get("close"), "idempotent_invalidates_children", "handle close drift")
    exact(h.get("drop"), "closes_unclosed_owned", "handle drop drift")
    exact(h.get("validation"), ["provider", "kind", "generation", "open"], "invalid handle validation accepted")
    exact(h.get("invalid"), "fail_closed_without_dispatch", "invalid handle dispatch accepted")
    b = v.get("buffers", {})
    exact(b.get("text"), "utf8_explicit_byte_length", "text buffer contract drift")
    exact(b.get("ownership"), {"borrowed_call": "valid_until_call_returns", "borrowed_event": "valid_until_event_acknowledged", "owned_provider": "released_by_negotiated_release"}, "buffer ownership drift")
    exact(b.get("retention"), "forbidden_after_boundary", "borrowed buffer retention accepted")
    exact(b.get("limits"), "validated_before_allocation_or_copy", "buffer limit validation drift")
    exact(b.get("safe_return"), "copy_then_release_or_release_on_failure", "owned buffer release/leak drift")
    o = v.get("operations", {})
    exact(o.get("capability"), "declared_and_checked_before_dispatch", "capability audit/denial drift")
    exact(o.get("effects"), "mapped_to_provider_call", "operation effect mapping drift")
    exact(o.get("cancellation"), "token_then_drain_acknowledge_before_teardown", "cancellation drain drift")
    exact(o.get("events"), "synchronous_acknowledged_only_v1", "event acknowledgement drift")
    exact(o.get("fault"), "quarantine_provider_invalidate_handles", "provider fault quarantine drift")
    e = v.get("errors", {})
    exact(e.get("shape"), ["kind", "operation", "provider_identity", "message"], "structured error shape drift")
    exact(e.get("kinds"), ["incompatible_version", "missing_symbol", "feature_denied", "trust_denied", "invalid_handle", "invalid_buffer", "capability_denied", "cancelled", "provider_fault", "provider_leak"], "required failure kinds drift")
    l = v.get("loading", {})
    exact(l.get("selection"), "target_policy_explicit_candidate", "target selection drift")
    exact(l.get("trust"), "target_signature_or_trust_policy_required", "trust denial drift")
    exact(l.get("search_paths"), {"host_default": "denied", "relative": "denied", "ambient_lookup": "denied", "unsigned": "denied"}, "library search policy drift")
    exact(l.get("resolution"), "required_symbols_before_provider_code", "missing symbol resolution drift")
    a = v.get("audit", {})
    exact(a.get("fields"), ["provider_identity", "operation_class", "capability", "decision"], "capability audit fields drift")
    exact(a.get("forbidden"), ["buffer_contents", "text_contents", "paths", "credentials", "addresses", "raw_handles"], "capability audit leak")
    actual = {x.get("id"): x.get("kind") for x in v.get("fixtures", []) if isinstance(x, dict)}
    exact(actual, FIXTURES, "required failure fixture coverage drift")

def compile_fixture(target):
    cc = shutil.which("cc")
    need(cc, "C compiler unavailable for reference fixture")
    with tempfile.TemporaryDirectory() as directory:
        output = Path(directory) / "provider.o"
        result = subprocess.run([cc, "-std=c11", "-Wall", "-Wextra", "-Werror", "-c", str(C), "-o", str(output)], capture_output=True, text=True)
        need(result.returncode == 0, f"C reference fixture failed for {target}: {result.stderr}")

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", default="host", help="supported CI target label compiling this portable C fixture")
    args = parser.parse_args()
    need(args.target.strip(), "target label is required")
    schema = json.loads(S.read_text())
    contract = json.loads(V.read_text())
    validate(contract, schema)
    compile_fixture(args.target)
    print(json.dumps({"schema": contract["schema_version"], "ok": True, "fixtures": len(FIXTURES), "c_fixture": "compiled", "target": args.target}))

if __name__ == "__main__":
    main()
