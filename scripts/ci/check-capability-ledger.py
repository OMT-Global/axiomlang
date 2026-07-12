#!/usr/bin/env python3
"""Generate and validate the checked Axiom capability ledger."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


SCHEMA_VERSION = "axiom.capability_ledger.v1"
EVIDENCE_TIERS = {
    "direct_runtime",
    "static_spike",
    "scaffold",
    "unsupported",
    "production_qualified",
}
DEFAULT_DOCS = [
    "README.md",
    "docs/stage1.md",
    "docs/stage1-stdlib-status.md",
    "docs/stage1-agent-grade-compiler.md",
    "docs/stage1-language-issue-disposition.md",
    "docs/book.md",
    "docs/performance-benchmarks.md",
    "docs/bootstrap/versioning.md",
]

COMMAND_TIERS = {
    "new": "static_spike",
    "parse": "static_spike",
    "check": "static_spike",
    "build": "static_spike",
    "run": "static_spike",
    "trace": "static_spike",
    "test": "static_spike",
    "caps": "static_spike",
    "repair-plan": "static_spike",
    "task-contract": "static_spike",
    "verification-plan": "static_spike",
    "evidence": "static_spike",
    "verify": "static_spike",
    "semantic-diff": "static_spike",
    "inspect": "static_spike",
    "generate": "static_spike",
    "pkg": "static_spike",
    "explain": "static_spike",
    "doctor": "static_spike",
    "fmt": "static_spike",
    "doc": "static_spike",
    "bench": "static_spike",
    "mutation-report": "static_spike",
    "repl": "static_spike",
    "publish": "static_spike",
    "registry-index": "static_spike",
    "registry-validate": "static_spike",
    "registry-serve": "static_spike",
    "lsp": "scaffold",
    "dap": "scaffold",
}

LANGUAGE_SURFACES = {
    "Stmt": {
        "Let", "Assign", "Print", "Panic", "Defer", "If", "IfLet", "While",
        "Match", "Return",
    },
    "Expr": {
        "Literal", "VarRef", "Call", "MethodCall", "BinaryAdd", "BinaryCompare",
        "BinaryLogic", "Cast", "MutBorrow", "Deref", "Try", "Await",
        "StructLiteral", "FieldAccess", "TupleLiteral", "TupleIndex", "MapLiteral",
        "ArrayLiteral", "Slice", "Index", "Closure", "Match",
    },
    "TypeName": {
        "Int", "Numeric", "Bool", "String", "Str", "Named", "Ptr", "MutPtr",
        "MutRef", "Slice", "MutSlice", "LifetimeSlice", "LifetimeMutSlice",
        "Option", "Result", "Tuple", "Map", "Array", "Fn",
    },
}

STDLIB_MODULES = {
    "traits.ax", "time.ax", "env.ax", "fs.ax", "net.ax", "net_tcp.ax",
    "net_udp.ax", "process.ax", "crypto_hash.ax", "crypto_mac.ax",
    "crypto_rand.ax", "crypto_aead.ax", "crypto_sign.ax", "crypto.ax", "io.ax",
    "json.ax", "serdes.ax", "collections.ax", "string.ax", "string_builder.ax",
    "log.ax", "sync.ax", "async.ax", "async_time.ax", "async_net.ax",
    "testing.ax", "doc.ax", "lsp.ax", "http.ax", "http_async.ax", "regex.ax",
    "encoding.ax", "outcome.ax", "cli.ax",
}
SCAFFOLD_MODULES = {"doc.ax", "lsp.ax"}
CAPABILITY_SURFACES = {
    "fs", "fs:write", "net", "process", "env", "clock", "crypto", "ffi", "async"
}
MODULE_CAPABILITIES = {
    "time.ax": ["clock"],
    "env.ax": ["env"],
    "fs.ax": ["fs", "fs:write"],
    "net.ax": ["net"],
    "net_tcp.ax": ["net"],
    "net_udp.ax": ["net"],
    "process.ax": ["process"],
    "crypto_hash.ax": ["crypto"],
    "crypto_mac.ax": ["crypto"],
    "crypto_rand.ax": ["crypto"],
    "crypto_aead.ax": ["crypto"],
    "crypto_sign.ax": ["crypto"],
    "crypto.ax": ["crypto"],
    "async.ax": ["async"],
    "async_time.ax": ["async", "clock"],
    "async_net.ax": ["async", "net"],
    "http.ax": ["net"],
    "http_async.ax": ["async", "net"],
}
HISTORICAL_BOOTSTRAP_ISSUES = {
    216, 217, 218, 219, 220, 221, 222, 223, 224, 225, 226,
    232, 233, 234, 236, 237, 238, 239, 240,
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checkout-root", default=".")
    parser.add_argument(
        "--snapshot",
        default="stage1/compiler-contracts/snapshots/capability-ledger.json",
    )
    parser.add_argument(
        "--schema-file",
        default="stage1/schemas/axiom-capability-ledger-v1.schema.json",
    )
    parser.add_argument("--main-source")
    parser.add_argument("--check-docs", action="store_true")
    parser.add_argument("--docs", nargs="*")
    parser.add_argument("--render", action="store_true")
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def camel_to_kebab(value: str) -> str:
    return re.sub(r"(?<!^)(?=[A-Z])", "-", value).lower()


def enum_body(source: str, name: str) -> str:
    match = re.search(rf"\benum\s+{re.escape(name)}\s*\{{", source)
    if match is None:
        raise ValueError(f"missing compiler-owned enum {name}")
    start = source.index("{", match.start())
    depth = 0
    for index in range(start, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                return source[start + 1 : index]
    raise ValueError(f"unterminated compiler-owned enum {name}")


def enum_variants(source: str, name: str) -> list[str]:
    variants: list[str] = []
    depth = 0
    for line in enum_body(source, name).splitlines():
        if depth == 0:
            match = re.match(r"\s*([A-Z][A-Za-z0-9_]*)\b", line)
            if match:
                variants.append(match.group(1))
        depth += line.count("{") - line.count("}")
    return variants


def stdlib_rows(root: Path) -> list[dict[str, Any]]:
    source_path = root / "stage1/crates/axiomc/src/stdlib.rs"
    source = source_path.read_text(encoding="utf-8")
    table = source[source.index("const STDLIB_SOURCES") :]
    pattern = re.compile(
        r'^    \(\s*"([^"]+\.ax)"\s*,(.*?)(?=^    \(|^\];)', re.MULTILINE | re.DOTALL
    )
    extracted = pattern.findall(table)
    names = {name for name, _ in extracted}
    if names != STDLIB_MODULES:
        unclassified = sorted(names - STDLIB_MODULES)
        stale = sorted(STDLIB_MODULES - names)
        raise ValueError(
            f"stdlib classification drift: unclassified={unclassified}, stale={stale}"
        )

    rows = []
    for name, body in extracted:
        functions = re.findall(r"\bpub (?:async )?fn ([a-z][a-z0-9_]*)", body)
        include = re.search(r'include_str!\("([^"]+)"\)', body)
        if include:
            included = (source_path.parent / include.group(1)).resolve()
            included_source = included.read_text(encoding="utf-8")
            functions.extend(
                re.findall(r"\bpub (?:async )?fn ([a-z][a-z0-9_]*)", included_source)
            )
        if len(functions) != len(set(functions)):
            raise ValueError(f"duplicate exported function in std/{name}")
        if name in SCAFFOLD_MODULES:
            tier = "scaffold"
            status = "scaffold"
        else:
            tier = "static_spike"
            status = "partial"
        rows.append(
            {
                "module": f"std/{name}",
                "functions": sorted(functions),
                "capabilities": MODULE_CAPABILITIES.get(name, []),
                "status": status,
                "evidenceTier": tier,
                "source": "stage1/crates/axiomc/src/stdlib.rs",
            }
        )
    return sorted(rows, key=lambda row: row["module"])


def capability_names(source: str) -> list[str]:
    body_match = re.search(
        r"KNOWN_CAPABILITIES:[^=]+\=\s*\[(.*?)\];", source, re.DOTALL
    )
    if body_match is None:
        raise ValueError("missing KNOWN_CAPABILITIES compiler table")
    variants = re.findall(r"CapabilityKind::([A-Za-z0-9_]+)", body_match.group(1))
    return ["fs:write" if item == "FsWrite" else camel_to_kebab(item) for item in variants]


def schema_rows(root: Path) -> list[dict[str, str]]:
    paths = sorted((root / "stage1/schemas").glob("*.schema.json"))
    paths += sorted((root / "stage1/compiler-contracts/schemas").glob("*.schema.json"))
    rows = []
    for path in paths:
        payload = json.loads(path.read_text(encoding="utf-8"))
        schema_id = payload.get("$id") or payload.get("title") or path.name
        rows.append(
            {
                "name": str(schema_id),
                "status": "checked",
                "evidenceTier": "static_spike",
                "source": path.relative_to(root).as_posix(),
            }
        )
    return rows


def runtime_abi_rows(root: Path) -> list[dict[str, str]]:
    path = root / "stage1/runtime-abi/direct-native-v0.json"
    payload = json.loads(path.read_text(encoding="utf-8"))
    rows = []
    for row in payload.get("value_features", []) + payload.get("capability_shims", []):
        rows.append(
            {
                "name": row["id"],
                "status": row["status"],
                # Runtime-ABI status records coverage intent, not proof that the
                # behavior originated in the built binary. Promote rows only
                # after the contract carries machine-linked runtime evidence.
                "evidenceTier": "static_spike",
                "source": "stage1/runtime-abi/direct-native-v0.json",
            }
        )
    return sorted(rows, key=lambda row: row["name"])


def fact(name: str, status: str, tier: str, source: str) -> dict[str, str]:
    return {"name": name, "status": status, "evidenceTier": tier, "source": source}


def build_ledger(root: Path, main_source: Path | None = None) -> dict[str, Any]:
    main_path = main_source or root / "stage1/crates/axiomc/src/main.rs"
    main = main_path.read_text(encoding="utf-8")
    syntax = (root / "stage1/crates/axiomc/src/syntax.rs").read_text(encoding="utf-8")
    manifest = (root / "stage1/crates/axiomc/src/manifest.rs").read_text(encoding="utf-8")
    codegen = (root / "stage1/crates/axiomc/src/codegen.rs").read_text(encoding="utf-8")
    project = (root / "stage1/crates/axiomc/src/project.rs").read_text(encoding="utf-8")
    registry = (root / "stage1/crates/axiomc/src/registry.rs").read_text(encoding="utf-8")

    command_names = [camel_to_kebab(item) for item in enum_variants(main, "Command")]
    if set(command_names) != set(COMMAND_TIERS):
        raise ValueError(
            "command classification drift: "
            f"unclassified={sorted(set(command_names) - set(COMMAND_TIERS))}, "
            f"stale={sorted(set(COMMAND_TIERS) - set(command_names))}"
        )
    commands = [
        fact(name, "implemented", COMMAND_TIERS[name], "stage1/crates/axiomc/src/main.rs")
        for name in sorted(command_names)
    ]

    language: dict[str, list[dict[str, str]]] = {}
    language_names = {
        "Stmt": "statementForms",
        "Expr": "expressionForms",
        "TypeName": "types",
    }
    for enum_name, expected in LANGUAGE_SURFACES.items():
        actual = set(enum_variants(syntax, enum_name))
        if actual != expected:
            raise ValueError(
                f"{enum_name} classification drift: "
                f"unclassified={sorted(actual - expected)}, stale={sorted(expected - actual)}"
            )
        language[language_names[enum_name]] = [
            fact(camel_to_kebab(name), "partial", "static_spike", "stage1/crates/axiomc/src/syntax.rs")
            for name in sorted(actual)
        ]

    stdlib = stdlib_rows(root)
    discovered_capabilities = capability_names(manifest)
    if set(discovered_capabilities) != CAPABILITY_SURFACES:
        raise ValueError(
            "capability classification drift: "
            f"unclassified={sorted(set(discovered_capabilities) - CAPABILITY_SURFACES)}, "
            f"stale={sorted(CAPABILITY_SURFACES - set(discovered_capabilities))}"
        )
    capabilities = [
        fact(name, "partial", "static_spike", "stage1/crates/axiomc/src/manifest.rs")
        for name in discovered_capabilities
    ]
    supported_backend = re.search(
        r'SUPPORTED_NATIVE_BACKENDS:\s*&str\s*=\s*"([^"]+)"', codegen
    )
    backend_variants = set(enum_variants(codegen, "NativeBackendKind"))
    if supported_backend is None or supported_backend.group(1) != "cranelift" or backend_variants != {"Cranelift", "GeneratedRust"}:
        raise ValueError("backend classification drift in compiler-owned backend tables")
    target_match = re.search(
        r"fn resolved_build_target\(.*?\n\}", project, re.DOTALL
    )
    if target_match is None:
        raise ValueError("missing compiler-owned target resolution table")
    target_aliases = set(re.findall(r'Some\("([^"]+)"\)', target_match.group(0)))
    if target_aliases != {"wasm32", "wasm32-wasi"}:
        raise ValueError(f"target classification drift: aliases={sorted(target_aliases)}")
    package_markers = (
        "pub struct PackageSection", "pub struct DependencySpec",
        "pub struct WorkspaceSection", "pub fn publish_package",
        "pub fn render_registry_index", "pub fn serve_registry",
    )
    combined_package_sources = manifest + registry
    missing_package_markers = [
        marker for marker in package_markers if marker not in combined_package_sources
    ]
    if missing_package_markers:
        raise ValueError(f"package classification drift: missing={missing_package_markers}")
    runtime_abi = runtime_abi_rows(root)
    schemas = schema_rows(root)
    function_count = sum(len(row["functions"]) for row in stdlib)

    ledger = {
        "schemaVersion": SCHEMA_VERSION,
        "governingIssue": 1435,
        "generatedFrom": [
            "stage1/crates/axiomc/src/main.rs",
            "stage1/crates/axiomc/src/syntax.rs",
            "stage1/crates/axiomc/src/stdlib.rs",
            "stage1/crates/axiomc/src/manifest.rs",
            "stage1/crates/axiomc/src/codegen.rs",
            "stage1/crates/axiomc/src/project.rs",
            "stage1/crates/axiomc/src/registry.rs",
            "stage1/runtime-abi/direct-native-v0.json",
            "stage1/schemas",
            "stage1/compiler-contracts/schemas",
        ],
        "summary": {
            "commands": len(commands),
            "stdlibModules": len(stdlib),
            "stdlibFunctions": function_count,
            "capabilities": len(capabilities),
            "runtimeAbiRows": len(runtime_abi),
            "schemas": len(schemas),
            "supportedNativeBackend": "cranelift",
            "evidenceTiers": {},
        },
        "language": language,
        "commands": commands,
        "stdlib": stdlib,
        "capabilities": capabilities,
        "backends": [
            fact("cranelift", "partial", "direct_runtime", "stage1/crates/axiomc/src/codegen.rs"),
            fact("generated-rust", "internal-only", "unsupported", "stage1/crates/axiomc/src/codegen.rs"),
        ],
        "targets": [
            fact("host", "partial", "direct_runtime", "stage1/crates/axiomc/src/project.rs"),
            fact("wasm32-wasip1", "unsupported", "unsupported", "stage1/crates/axiomc/src/project.rs"),
        ],
        "packages": [
            fact("local-package", "implemented", "static_spike", "stage1/crates/axiomc/src/manifest.rs"),
            fact("local-path-dependency", "implemented", "static_spike", "stage1/crates/axiomc/src/manifest.rs"),
            fact("workspace", "implemented", "static_spike", "stage1/crates/axiomc/src/manifest.rs"),
            fact("registry-publication", "partial", "static_spike", "stage1/crates/axiomc/src/registry.rs"),
            fact("remote-registry-resolution", "unsupported", "unsupported", "stage1/crates/axiomc/src/registry.rs"),
        ],
        "runtimeAbi": runtime_abi,
        "schemas": schemas,
        "toolingDepth": [
            fact("native-axiom-dwarf", "scaffold", "scaffold", "docs/stage1-debug-map.md"),
            fact("dynamic-trait-dispatch", "unsupported", "unsupported", "docs/rfcs/0003-dyn-trait-dispatch.md"),
            fact("general-borrow-checker", "unsupported", "unsupported", "stage1/crates/axiomc/src/borrowck.rs"),
        ],
        "historicalEvidence": [
            {
                "issues": [216, 217, 218, 219, 220, 221, 222, 223, 224, 225, 226, 232, 233, 234, 236, 237, 238, 239, 240],
                "note": "Issue state is historical evidence only; current support is classified by compiler-owned surfaces and executable evidence.",
            }
        ],
    }
    tier_counts = {tier: 0 for tier in sorted(EVIDENCE_TIERS)}
    for _, _, row in iter_rows(ledger):
        tier_counts[row["evidenceTier"]] += 1
    ledger["summary"]["evidenceTiers"] = tier_counts
    return ledger


def iter_rows(ledger: dict[str, Any]):
    for section in ("commands", "capabilities", "backends", "targets", "packages", "runtimeAbi", "schemas", "toolingDepth"):
        for row in ledger.get(section, []):
            yield section, row.get("name"), row
    for section, rows in ledger.get("language", {}).items():
        for row in rows:
            yield f"language.{section}", row.get("name"), row
    for row in ledger.get("stdlib", []):
        yield "stdlib", row.get("module"), row


def validate_ledger(ledger: Any, root: Path) -> list[str]:
    errors: list[str] = []
    if not isinstance(ledger, dict):
        return ["ledger root must be an object"]
    if ledger.get("schemaVersion") != SCHEMA_VERSION:
        errors.append(f"schemaVersion must be {SCHEMA_VERSION}")
    required_sections = {
        "schemaVersion", "governingIssue", "generatedFrom", "summary", "language",
        "commands", "stdlib", "capabilities", "backends", "targets", "packages",
        "runtimeAbi", "schemas", "toolingDepth", "historicalEvidence",
    }
    if set(ledger) != required_sections:
        errors.append(
            "ledger top-level fields drifted: "
            f"missing={sorted(required_sections - set(ledger))}, "
            f"unexpected={sorted(set(ledger) - required_sections)}"
        )
    for source in ledger.get("generatedFrom", []):
        if not isinstance(source, str) or not (root / source).exists():
            errors.append(f"generatedFrom references missing compiler source {source!r}")
    seen: set[tuple[str, str]] = set()
    for section, identity, row in iter_rows(ledger):
        if not isinstance(identity, str) or not identity:
            errors.append(f"{section} row is missing an identity")
            continue
        key = (section, identity)
        if key in seen:
            errors.append(f"duplicate ledger row {section}:{identity}")
        seen.add(key)
        tier = row.get("evidenceTier")
        if tier not in EVIDENCE_TIERS:
            errors.append(f"{section}:{identity} has invalid evidence tier {tier!r}")
        source = row.get("source")
        if not isinstance(source, str) or not (root / source).exists():
            errors.append(f"{section}:{identity} references missing source {source!r}")
        if section == "stdlib":
            functions = row.get("functions")
            if not isinstance(functions, list) or len(functions) != len(set(functions)):
                errors.append(f"stdlib:{identity} functions must be a unique array")
    summary = ledger.get("summary", {})
    if isinstance(summary, dict):
        expected_counts = {
            "commands": len(ledger.get("commands", [])),
            "stdlibModules": len(ledger.get("stdlib", [])),
            "stdlibFunctions": sum(
                len(row.get("functions", [])) for row in ledger.get("stdlib", [])
                if isinstance(row, dict)
            ),
            "capabilities": len(ledger.get("capabilities", [])),
            "runtimeAbiRows": len(ledger.get("runtimeAbi", [])),
            "schemas": len(ledger.get("schemas", [])),
        }
        for field, expected in expected_counts.items():
            if summary.get(field) != expected:
                errors.append(
                    f"summary.{field} is {summary.get(field)!r}; expected {expected}"
                )
        actual_tiers = {tier: 0 for tier in sorted(EVIDENCE_TIERS)}
        for _, _, row in iter_rows(ledger):
            tier = row.get("evidenceTier")
            if tier in actual_tiers:
                actual_tiers[tier] += 1
        if summary.get("evidenceTiers") != actual_tiers:
            errors.append("summary.evidenceTiers does not match ledger rows")
    return errors


def expected_doc_marker(ledger: dict[str, Any]) -> str:
    summary = ledger["summary"]
    return (
        "<!-- capability-ledger:v1 "
        f"commands={summary['commands']} "
        f"stdlib_modules={summary['stdlibModules']} "
        f"stdlib_functions={summary['stdlibFunctions']} "
        f"capabilities={summary['capabilities']} "
        f"backend={summary['supportedNativeBackend']} -->"
    )


def validate_docs(root: Path, docs: list[str], ledger: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    marker = expected_doc_marker(ledger)
    stale_patterns = {
        r"\b(?:fifteen|sixteen|thirty-one|31)\s+(?:landed\s+)?stdlib modules\b": "stale stdlib module count",
        r"\bNo closures\b": "closures are implemented syntax and must not be listed as absent",
        r"Only `std/fs\.ax read_file` is supported": "filesystem writes are implemented",
        r"Ed25519[^\n|.]*remain(?:s)? open": "Ed25519 is implemented",
        r"explicit compatibility backend": "generated Rust is not a supported CLI backend",
        r"--backend generated-rust[^\n]*(?:supported|available|compatibility backend)": "generated Rust is not a supported CLI backend",
    }
    for relative in docs:
        path = root / relative
        if not path.is_file():
            errors.append(f"missing checked status document {relative}")
            continue
        text = path.read_text(encoding="utf-8")
        if marker not in text:
            errors.append(f"{relative} is missing capability ledger marker {marker}")
        for pattern, message in stale_patterns.items():
            if re.search(pattern, text, re.IGNORECASE):
                errors.append(f"{relative}: {message}")
        for line_number, line in enumerate(text.splitlines(), start=1):
            references = {
                int(issue)
                for issue in re.findall(r"(?:issues/|#)(\d+)", line)
            }
            if references & HISTORICAL_BOOTSTRAP_ISSUES and re.search(
                r"\b(?:current(?:ly)?|active|production)\b[^\n]*"
                r"\b(?:closure|closed|open|blocker|complete|completed)\b|"
                r"\b(?:keep|remain(?:s)?)\s+open\b",
                line,
                re.IGNORECASE,
            ):
                errors.append(
                    f"{relative}:{line_number}: closed bootstrap issue state is "
                    "historical evidence, not a current support claim"
                )
        table_rows = [line.strip() for line in text.splitlines() if line.lstrip().startswith("|")]
        duplicates = sorted({line for line in table_rows if table_rows.count(line) > 1 and "---" not in line})
        if duplicates:
            errors.append(f"{relative}: duplicate table row(s): {duplicates}")
        if relative == "README.md":
            commands = [line.strip() for line in text.splitlines() if line.startswith("cargo run ")]
            duplicate_commands = sorted({line for line in commands if commands.count(line) > 1})
            if duplicate_commands:
                errors.append(f"README.md: duplicate command(s): {duplicate_commands}")
    return errors


def emit_report(report: dict[str, Any], as_json: bool) -> None:
    if as_json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        status = "pass" if report["ok"] else "fail"
        print(f"capability ledger: {status}")
        for error in report["errors"]:
            print(f"- {error}", file=sys.stderr)


def main() -> int:
    args = parse_args()
    root = Path(args.checkout_root).resolve()
    snapshot_path = root / args.snapshot
    schema_path = root / args.schema_file
    main_source = Path(args.main_source).resolve() if args.main_source else None
    errors: list[str] = []
    try:
        generated = build_ledger(root, main_source)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        generated = {}
        errors.append(str(error))

    if args.render:
        if errors:
            emit_report({"schema": SCHEMA_VERSION, "ok": False, "errors": errors}, args.json)
            return 1
        print(json.dumps(generated, indent=2, sort_keys=True))
        return 0

    schema: Any = None
    if not schema_path.is_file():
        errors.append(f"missing published schema {args.schema_file}")
    else:
        try:
            schema = json.loads(schema_path.read_text(encoding="utf-8"))
            if schema.get("properties", {}).get("schemaVersion", {}).get("const") != SCHEMA_VERSION:
                errors.append("published schema does not pin the capability ledger version")
        except (OSError, json.JSONDecodeError) as error:
            errors.append(f"invalid published schema: {error}")

    checked: Any = None
    if not snapshot_path.is_file():
        errors.append(f"missing checked snapshot {args.snapshot}")
    else:
        try:
            checked = json.loads(snapshot_path.read_text(encoding="utf-8"))
            errors.extend(validate_ledger(checked, root))
        except (OSError, json.JSONDecodeError) as error:
            errors.append(f"invalid checked snapshot: {error}")

    if generated and checked is not None and checked != generated:
        errors.append("checked capability ledger is stale; regenerate it from compiler-owned tables")
    if generated:
        errors.extend(validate_ledger(generated, root))
        if args.check_docs:
            errors.extend(validate_docs(root, args.docs or DEFAULT_DOCS, generated))

    report = {
        "schema": SCHEMA_VERSION,
        "ok": not errors,
        "errors": errors,
        "summary": generated.get("summary", {}),
        "snapshot": args.snapshot,
        "docsChecked": bool(args.check_docs),
    }
    emit_report(report, args.json)
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
