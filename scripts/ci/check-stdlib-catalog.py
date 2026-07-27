#!/usr/bin/env python3
"""Generate and validate the typed, provider-owned stdlib catalog."""
import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LEDGER = ROOT / "stage1/compiler-contracts/snapshots/capability-ledger.json"
SNAPSHOT = ROOT / "stage1/compiler-contracts/snapshots/stdlib-catalog.json"
SCHEMA = ROOT / "stage1/compiler-contracts/schemas/axiom.compiler.stdlib_catalog.v1.schema.json"
STDLIB = ROOT / "stage1/crates/axiomc/src/stdlib.rs"
ROLLBACK = "stage1/crates/axiomc/src/stdlib.rs remains the bootstrap loader until #1436 qualifies a catalog consumer."
ACCEPTANCE = {"status": "pending", "governing_issue": 1436, "qualified_consumer": "compiler.mir_backend"}


def embedded_sources():
    source = STDLIB.read_text(encoding="utf-8")
    table = source[source.index("const STDLIB_SOURCES") :]
    pattern = re.compile(r'^    \(\s*"([^"]+\.ax)"\s*,(.*?)(?=^    \(|^\];)', re.MULTILINE | re.DOTALL)
    modules = {}
    for name, body in pattern.findall(table):
        include = re.search(r'include_str!\("([^"]+)"\)', body)
        if include:
            modules[name] = (STDLIB.parent / include.group(1)).read_text(encoding="utf-8")
            continue
        literals = re.findall(r'"((?:\\.|[^"\\])*)"', body, re.DOTALL)
        modules[name] = "".join(bytes(literal, "utf-8").decode("unicode_escape") for literal in literals)
    return modules


def signatures(module_source):
    pattern = re.compile(r'\bpub\s+(async\s+)?fn\s+([a-z][a-z0-9_]*)(<[^>\n]+>)?\s*\(([^)]*)\)\s*:\s*([^\{\n]+)')
    result = {}
    for async_prefix, name, generics, parameters, return_type in pattern.findall(module_source):
        params = re.sub(r'\s+', ' ', parameters.strip())
        result[name] = f"{'async ' if async_prefix else ''}fn {name}{generics}({params}): {return_type.strip()}"
    return result


def build(ledger):
    sources = embedded_sources()
    modules = []
    for row in sorted(ledger["stdlib"], key=lambda item: item["module"]):
        name = row["module"]
        source_name = name.removeprefix("std/")
        module_source = sources[source_name]
        module_signatures = signatures(module_source)
        expected_symbols = sorted(row["functions"])
        if sorted(module_signatures) != expected_symbols:
            raise ValueError(f"stdlib signature parity drift for {name}")
        capabilities = sorted(row["capabilities"])
        effect = "pure" if not capabilities else "capability:" + ",".join(capabilities)
        module_key = source_name.removesuffix(".ax")
        symbols = []
        for symbol in expected_symbols:
            provider_id = f"axiom://provider/stage1-v1/{module_key}/{symbol}"
            symbols.append({
                "name": symbol,
                "signature": module_signatures[symbol],
                "effect": effect,
                "binding": provider_id,
                "binding_kind": "provider_contract",
                "provider": {"id": provider_id, "kind": "declared_provider"},
            })
        modules.append({
            "name": name,
            "module_id": f"axiom://stdlib/stage1-v1/{module_key}",
            "module_loading": {
                "kind": "embedded_source",
                "source_path": "stage1/crates/axiomc/src/stdlib.rs",
                "source_digest": hashlib.sha256(module_source.encode()).hexdigest(),
            },
            "capabilities": capabilities,
            "symbols": symbols,
        })
    material = {"catalog_version": "1.0.0", "modules": modules, "acceptance_boundary": ACCEPTANCE}
    return {
        "schema_version": "axiom.compiler.stdlib_catalog.v1",
        "contract": "compiler.stdlib",
        "catalog_version": "1.0.0",
        "source": "stage1/compiler-contracts/snapshots/capability-ledger.json",
        "modules": modules,
        "release_digest": hashlib.sha256(json.dumps(material, sort_keys=True, separators=(",", ":")).encode()).hexdigest(),
        "rollback_boundary": ROLLBACK,
        "acceptance_boundary": ACCEPTANCE,
    }


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def validate_catalog(catalog, schema):
    require(schema["title"] == "AxiOM compiler standard-library catalog", "schema title mismatch")
    require(set(catalog) == set(schema["properties"]), "catalog/schema field mismatch")
    require(schema.get("additionalProperties") is False, "catalog schema must reject unknown top-level fields")
    module_schema = schema["$defs"]["module"]
    symbol_schema = schema["$defs"]["symbol"]
    require(module_schema.get("additionalProperties") is False, "catalog module schema must be closed")
    require({"name", "module_id", "module_loading", "capabilities", "symbols"}.issubset(module_schema["required"]), "catalog module schema is incomplete")
    require(symbol_schema.get("additionalProperties") is False, "catalog symbol schema must be closed")
    require({"name", "signature", "effect", "binding", "binding_kind", "provider"}.issubset(symbol_schema["required"]), "catalog symbol schema is incomplete")
    require(catalog["acceptance_boundary"] == ACCEPTANCE, "catalog acceptance boundary drift")
    module_ids = set()
    bindings = set()
    for module in catalog["modules"]:
        require(module["module_id"] not in module_ids, "duplicate module identity")
        module_ids.add(module["module_id"])
        require(module["module_loading"]["kind"] == "embedded_source", "invalid module loading mode")
        require(re.fullmatch(r"[0-9a-f]{64}", module["module_loading"]["source_digest"]) is not None, "invalid module source digest")
        for symbol in module["symbols"]:
            require(re.fullmatch(r"(?:async )?fn [a-z][a-z0-9_]*(?:<[^>]+>)?\(.*\): .+", symbol["signature"]) is not None, "invalid canonical signature")
            require(symbol["binding_kind"] == "provider_contract", "invalid provider binding kind")
            require(symbol["binding"] == symbol["provider"]["id"], "provider id/binding mismatch")
            require(symbol["provider"]["kind"] == "declared_provider", "invalid provider declaration kind")
            require(symbol["binding"] not in bindings, "duplicate provider binding")
            bindings.add(symbol["binding"])
            require("rust" not in symbol["binding"].lower(), "host-language provider identifier leaked")


parser = argparse.ArgumentParser()
parser.add_argument("--write", action="store_true")
parser.add_argument("--json", action="store_true")
args = parser.parse_args()
ledger = json.loads(LEDGER.read_text())
expected = build(ledger)
if args.write:
    SNAPSHOT.write_text(json.dumps(expected, indent=2) + "\n")
catalog = json.loads(SNAPSHOT.read_text())
schema = json.loads(SCHEMA.read_text())
require(catalog == expected, "stdlib catalog drift; regenerate with --write")
validate_catalog(catalog, schema)
output = {"ok": True, "modules": len(catalog["modules"]), "symbols": sum(len(module["symbols"]) for module in catalog["modules"]), "release_digest": catalog["release_digest"]}
print(json.dumps(output, sort_keys=True) if args.json else output)
