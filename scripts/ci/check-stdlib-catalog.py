#!/usr/bin/env python3
"""Generate and validate the typed, provider-owned stdlib catalog."""
import argparse
import hashlib
import json
import math
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LEDGER = ROOT / "stage1/compiler-contracts/snapshots/capability-ledger.json"
SNAPSHOT = ROOT / "stage1/compiler-contracts/snapshots/stdlib-catalog.json"
SCHEMA = ROOT / "stage1/compiler-contracts/schemas/axiom.compiler.stdlib_catalog.v1.schema.json"
AUTHORITY = ROOT / "stage1/compiler-contracts/sources/stdlib-catalog-authority-v1.json"
STDLIB = ROOT / "stage1/crates/axiomc/src/stdlib.rs"
PROJECT = ROOT / "stage1/crates/axiomc/src/project.rs"
CATALOG_SOURCE = "stage1/compiler-contracts/sources/stdlib-catalog-authority-v1.json"
ROLLBACK = "stage1/crates/axiomc/src/stdlib.rs remains the bootstrap loader until #1436 qualifies a catalog consumer."
ACCEPTANCE = {"status": "pending", "governing_issue": 1436, "qualified_consumer": "compiler.mir_backend"}
CAPABILITY_IDS = {"async", "clock", "crypto", "env", "ffi", "fs", "fs:write", "net", "process"}


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


def rust_tokens(source):
    """Tokenize the constrained Rust capability function with literal context."""
    tokens = []
    index = 0
    while index < len(source):
        if source[index].isspace():
            index += 1
            continue
        if source.startswith("//", index):
            if source.startswith("///", index) and not source.startswith("////", index):
                tokens.append(("outer_doc_attribute", "#"))
            end = source.find("\n", index + 2)
            index = len(source) if end == -1 else end + 1
            continue
        if source.startswith("/*", index):
            if source.startswith("/**", index) and not source.startswith("/***", index):
                tokens.append(("outer_doc_attribute", "#"))
            depth = 1
            index += 2
            while index < len(source) and depth:
                if source.startswith("/*", index):
                    depth += 1
                    index += 2
                elif source.startswith("*/", index):
                    depth -= 1
                    index += 2
                else:
                    index += 1
            require(depth == 0, "bootstrap runtime capability table has an unterminated block comment")
            continue

        char_literal = re.match(
            r"(?:b)?'(?:\\(?:x[0-9A-Fa-f]{2}|u\{[0-9A-Fa-f_]+\}|.)|[^\\'\n])'",
            source[index:],
        )
        if char_literal is not None:
            tokens.append(("char_literal", ""))
            index += len(char_literal.group(0))
            continue

        raw_prefix = next(
            (prefix for prefix in ("br", "cr", "r") if source.startswith(prefix, index)),
            None,
        )
        raw_start = index + 1 if raw_prefix in {"br", "cr"} else index
        if raw_start < len(source) and source[raw_start] == "r":
            marker = raw_start + 1
            while marker < len(source) and source[marker] == "#":
                marker += 1
            if marker < len(source) and source[marker] == '"':
                hashes = source[raw_start + 1 : marker]
                terminator = '"' + hashes
                end = source.find(terminator, marker + 1)
                require(end != -1, "bootstrap runtime capability table has an unterminated raw string")
                token_kind = {
                    "br": "raw_byte_string",
                    "cr": "raw_c_string",
                    "r": "raw_string",
                }[raw_prefix]
                tokens.append((token_kind, source[marker + 1 : end]))
                index = end + len(terminator)
                continue

        if source[index] == '"':
            end = index + 1
            escaped = False
            while end < len(source):
                if not escaped and source[end] == '"':
                    break
                if not escaped and source[end] == "\\":
                    escaped = True
                else:
                    escaped = False
                end += 1
            require(end < len(source), "bootstrap runtime capability table has an unterminated string")
            contents = source[index + 1 : end]
            token_kind = "string" if "\\" not in contents else "escaped_string"
            tokens.append((token_kind, contents))
            index = end + 1
            continue

        if source[index].isdigit():
            end = index + 1
            while end < len(source) and (source[end].isalnum() or source[end] in "_."):
                end += 1
            tokens.append(("number", source[index:end]))
            index = end
            continue

        if source[index].isalpha() or source[index] == "_":
            end = index + 1
            while end < len(source) and (source[end].isalnum() or source[end] == "_"):
                end += 1
            tokens.append(("identifier", source[index:end]))
            index = end
            continue

        matched = next((value for value in ("=>", "::", "->") if source.startswith(value, index)), None)
        if matched is not None:
            tokens.append(("punctuation", matched))
            index += len(matched)
            continue
        tokens.append(("punctuation", source[index]))
        index += 1
    return tokens


def punctuation_value(token):
    """Return punctuation text without treating literal contents as syntax."""
    return token[1] if token[0] == "punctuation" else None


def token_is(token, kind, value):
    return token == (kind, value)


def tokens_match(tokens, expected):
    return len(tokens) == len(expected) and all(
        token_is(token, kind, value)
        for token, (kind, value) in zip(tokens, expected)
    )


def split_rust_match_arms(tokens):
    arms = []
    current = []
    depths = {"(": 0, "[": 0, "{": 0}
    closing = {")": "(", "]": "[", "}": "{"}
    for token in tokens:
        value = punctuation_value(token)
        if value in depths:
            depths[value] += 1
        elif value in closing:
            opener = closing[value]
            require(depths[opener] > 0, "bootstrap runtime capability arm delimiters are unbalanced")
            depths[opener] -= 1
        if value == "," and not any(depths.values()):
            if current:
                arms.append(current)
                current = []
            continue
        current.append(token)
        if value == "}" and not any(depths.values()):
            arms.append(current)
            current = []
    require(not any(depths.values()), "bootstrap runtime capability arm delimiters are unbalanced")
    if current:
        arms.append(current)
    return arms


def parse_capability_arm(tokens, capability_names):
    if tokens_match(
        tokens,
        [("identifier", "_"), ("punctuation", "=>"), ("identifier", "None")],
    ):
        return None

    patterns = []
    index = 0
    while index < len(tokens) and punctuation_value(tokens[index]) == "(":
        index += 1
        require(index < len(tokens) and tokens[index][0] == "string", "invalid runtime capability module pattern")
        module_file = tokens[index][1]
        index += 1
        require(
            index < len(tokens) and punctuation_value(tokens[index]) == ",",
            "invalid runtime capability tuple",
        )
        index += 1
        selectors = []
        if index < len(tokens) and token_is(tokens[index], "identifier", "_"):
            index += 1
        else:
            while True:
                require(index < len(tokens) and tokens[index][0] == "string", "invalid runtime capability selector")
                selectors.append(tokens[index][1])
                index += 1
                if (
                    index + 1 < len(tokens)
                    and punctuation_value(tokens[index]) == "|"
                    and tokens[index + 1][0] == "string"
                ):
                    index += 1
                    continue
                break
        require(
            index < len(tokens) and punctuation_value(tokens[index]) == ")",
            "invalid runtime capability tuple",
        )
        index += 1
        patterns.append((module_file, selectors))
        if (
            index + 1 < len(tokens)
            and punctuation_value(tokens[index]) == "|"
            and punctuation_value(tokens[index + 1]) == "("
        ):
            index += 1
            continue
        break

    require(
        patterns and index < len(tokens) and punctuation_value(tokens[index]) == "=>",
        "invalid runtime capability arm",
    )
    index += 1
    result = tokens[index:]
    if result and punctuation_value(result[0]) == "{":
        require(punctuation_value(result[-1]) == "}", "invalid runtime capability result block")
        result = result[1:-1]
    require(
        tokens_match(
            result[:8],
            [
                ("identifier", "Some"),
                ("punctuation", "("),
                ("punctuation", "::"),
                ("identifier", "std"),
                ("punctuation", "::"),
                ("identifier", "vec"),
                ("punctuation", "!"),
                ("punctuation", "["),
            ],
        ),
        "invalid runtime capability result",
    )
    require(
        tokens_match(result[-2:], [("punctuation", "]"), ("punctuation", ")")]),
        "invalid runtime capability result",
    )
    capability_tokens = result[8:-2]
    kinds = []
    index = 0
    while index < len(capability_tokens):
        require(
            index + 2 < len(capability_tokens)
            and token_is(capability_tokens[index], "identifier", "CapabilityKind")
            and punctuation_value(capability_tokens[index + 1]) == "::"
            and capability_tokens[index + 2][0] == "identifier",
            "invalid runtime capability kind",
        )
        kinds.append(capability_tokens[index + 2][1])
        index += 3
        if index < len(capability_tokens):
            require(
                punctuation_value(capability_tokens[index]) == ",",
                "invalid runtime capability list",
            )
            index += 1
    require(kinds and all(kind in capability_names for kind in kinds), "bootstrap runtime capability kind drift")
    return patterns, sorted(capability_names[kind] for kind in kinds)


def bootstrap_runtime_effects(module_symbols):
    source = PROJECT.read_text(encoding="utf-8")
    source_tokens = rust_tokens(source)
    candidates = []
    depth_before = []
    depths = {"(": 0, "[": 0, "{": 0}
    closing = {")": "(", "]": "[", "}": "{"}
    for index, token in enumerate(source_tokens):
        depth_before.append(dict(depths))
        value = punctuation_value(token)
        if (
            not any(depths.values())
            and index + 1 < len(source_tokens)
            and token == ("identifier", "fn")
            and source_tokens[index + 1] == ("identifier", "stdlib_wrapper_capabilities")
        ):
            candidates.append(index)
        if value in depths:
            depths[value] += 1
        elif value in closing:
            opener = closing[value]
            require(depths[opener] > 0, "bootstrap runtime source delimiters are unbalanced")
            depths[opener] -= 1
    require(not any(depths.values()), "bootstrap runtime source delimiters are unbalanced")
    require(
        len(candidates) == 1,
        "bootstrap runtime capability function must have exactly one active top-level definition",
    )
    function_start = candidates[0]
    item_start = function_start
    for index in range(function_start - 1, -1, -1):
        at_top_level_semicolon = (
            punctuation_value(source_tokens[index]) == ";" and not any(depth_before[index].values())
        )
        closes_top_level_item = (
            punctuation_value(source_tokens[index]) == "}"
            and depth_before[index] == {"(": 0, "[": 0, "{": 1}
        )
        if at_top_level_semicolon or closes_top_level_item:
            item_start = index + 1
            break
        item_start = index
    require(
        not source_tokens[item_start:function_start],
        "bootstrap runtime capability function must not have item attributes or qualifiers",
    )
    tokens = source_tokens[function_start:]
    capability_names = {
        "Fs": "fs",
        "FsWrite": "fs:write",
        "Net": "net",
        "Process": "process",
        "Env": "env",
        "Clock": "clock",
        "Crypto": "crypto",
        "Ffi": "ffi",
        "Async": "async",
    }
    signature = [
        ("identifier", "fn"),
        ("identifier", "stdlib_wrapper_capabilities"),
        ("punctuation", "("),
        ("identifier", "module_path"),
        ("punctuation", ":"),
        ("punctuation", "&"),
        ("identifier", "Path"),
        ("punctuation", ","),
        ("identifier", "function_name"),
        ("punctuation", ":"),
        ("punctuation", "&"),
        ("identifier", "str"),
        ("punctuation", ","),
        ("punctuation", ")"),
        ("punctuation", "->"),
        ("identifier", "Option"),
        ("punctuation", "<"),
        ("identifier", "Vec"),
        ("punctuation", "<"),
        ("identifier", "CapabilityKind"),
        ("punctuation", ">"),
        ("punctuation", ">"),
        ("punctuation", "{"),
    ]
    require(
        tokens_match(tokens[: len(signature)], signature),
        "bootstrap runtime capability function signature drift",
    )
    function_open = len(signature) - 1
    depth = 0
    function_close = None
    for index in range(function_open, len(tokens)):
        if punctuation_value(tokens[index]) == "{":
            depth += 1
        elif punctuation_value(tokens[index]) == "}":
            depth -= 1
            if depth == 0:
                function_close = index
                break
    require(function_close is not None, "bootstrap runtime capability function syntax drift")
    tokens = tokens[: function_close + 1]
    function_close = len(tokens) - 1
    body = tokens[function_open + 1 : function_close]
    prelude = [
        ("identifier", "let"), ("identifier", "module"), ("punctuation", "="),
        ("identifier", "stdlib_module_file"), ("punctuation", "("),
        ("identifier", "module_path"), ("punctuation", ")"), ("punctuation", "?"),
        ("punctuation", ";"), ("identifier", "match"), ("punctuation", "("),
        ("identifier", "module"), ("punctuation", "."), ("identifier", "as_str"),
        ("punctuation", "("), ("punctuation", ")"), ("punctuation", ","),
        ("identifier", "function_name"), ("punctuation", ")"), ("punctuation", "{"),
    ]
    require(tokens_match(body[: len(prelude)], prelude), "bootstrap runtime capability function syntax drift")
    match_open = len(prelude) - 1
    depth = 1
    match_close = None
    for index in range(match_open + 1, len(body)):
        if punctuation_value(body[index]) == "{":
            depth += 1
        elif punctuation_value(body[index]) == "}":
            depth -= 1
            if depth == 0:
                match_close = index
                break
    require(match_close == len(body) - 1, "bootstrap runtime capability match must be the direct tail expression")
    arms = split_rust_match_arms(body[match_open + 1 : match_close])
    require(arms, "bootstrap runtime capability table could not be parsed")
    effects = {
        (module, symbol): "pure"
        for module, symbols in module_symbols.items()
        for symbol in symbols
    }
    assigned = set()
    default_seen = False
    for position, arm in enumerate(arms):
        parsed = parse_capability_arm(arm, capability_names)
        if parsed is None:
            require(not default_seen and position == len(arms) - 1, "runtime capability default arm must be unique and last")
            default_seen = True
            continue
        require(not default_seen, "runtime capability arm follows the default")
        patterns, capabilities = parsed
        effect = "capability:" + ",".join(capabilities)
        for module_file, selectors in patterns:
            module = f"std/{module_file}"
            require(module in module_symbols, f"runtime capability table references unknown {module}")
            symbols = module_symbols[module] if not selectors else selectors
            require(symbols, f"runtime capability selector is empty for {module}")
            for symbol in symbols:
                key = (module, symbol)
                require(key in effects, f"runtime capability table references unknown {module}::{symbol}")
                if key not in assigned:
                    effects[key] = effect
                    assigned.add(key)
    require(default_seen, "bootstrap runtime capability table is missing its default arm")
    return effects


def ledger_module_index(ledger):
    rows = ledger.get("stdlib")
    require(isinstance(rows, list), "capability ledger stdlib rows must be an array")
    names = [row.get("module") for row in rows if isinstance(row, dict)]
    require(len(names) == len(rows) and len(names) == len(set(names)), "duplicate capability ledger module")
    return {row["module"]: row for row in rows}


def build(ledger, authority):
    sources = embedded_sources()
    embedded_modules = {f"std/{name}" for name in sources}
    authority_modules = {row["name"] for row in authority["modules"]}
    require(
        embedded_modules == authority_modules,
        "authority/embedded source module parity drift",
    )
    ledger_modules = ledger_module_index(ledger)
    modules = []
    for row in authority["modules"]:
        name = row["name"]
        source_name = name.removeprefix("std/")
        module_source = sources[source_name]
        module_signatures = signatures(module_source)
        expected_symbols = sorted(module_signatures)
        ledger_symbols = sorted(ledger_modules[name]["functions"])
        if expected_symbols != ledger_symbols:
            raise ValueError(f"stdlib signature parity drift for {name}")
        capabilities = sorted(row["capabilities"])
        policy = row.get("symbol_capability_policy", {})
        default_capabilities = policy.get("default", capabilities)
        overrides = policy.get("overrides", {})
        module_key = source_name.removesuffix(".ax")
        symbols = []
        for symbol in expected_symbols:
            symbol_capabilities = overrides.get(symbol, default_capabilities)
            effect = (
                "pure"
                if not symbol_capabilities
                else "capability:" + ",".join(symbol_capabilities)
            )
            provider_id = f"{row['binding_namespace']}/{symbol}"
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
    runtime_effects = bootstrap_runtime_effects(
        {module["name"]: [symbol["name"] for symbol in module["symbols"]] for module in modules}
    )
    for module in modules:
        for symbol in module["symbols"]:
            require(
                symbol["effect"] == runtime_effects[(module["name"], symbol["name"])],
                f"bootstrap runtime capability parity drift for {module['name']}::{symbol['name']}",
            )
    material = {"catalog_version": "2.0.0", "modules": modules, "acceptance_boundary": ACCEPTANCE}
    return {
        "schema_version": "axiom.compiler.stdlib_catalog.v1",
        "contract": "compiler.stdlib",
        "catalog_version": "2.0.0",
        "source": CATALOG_SOURCE,
        "modules": modules,
        "release_digest": hashlib.sha256(json.dumps(material, sort_keys=True, separators=(",", ":")).encode()).hexdigest(),
        "rollback_boundary": ROLLBACK,
        "acceptance_boundary": ACCEPTANCE,
    }


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def validate_capability_list(values, label):
    require(isinstance(values, list), f"{label} must be an array")
    singular = f"{label[:-3]}y" if label.endswith("ies") else label
    require(
        all(isinstance(capability, str) and capability in CAPABILITY_IDS for capability in values),
        f"invalid {singular}",
    )
    require(values == sorted(set(values)), f"{label} must be sorted and unique")


def reject_duplicate_keys(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError(f"duplicate JSON key: {key}")
        value[key] = item
    return value


def reject_non_finite_constant(value):
    raise ValueError(f"non-finite JSON constant: {value}")


def reject_non_finite_float(value):
    parsed = float(value)
    if not math.isfinite(parsed):
        raise ValueError(f"non-finite JSON number: {value}")
    return parsed


def load_json(path):
    try:
        return json.loads(
            path.read_text(),
            object_pairs_hook=reject_duplicate_keys,
            parse_constant=reject_non_finite_constant,
            parse_float=reject_non_finite_float,
        )
    except (json.JSONDecodeError, ValueError) as error:
        raise AssertionError(f"invalid JSON in {path}: {error}") from error


def validate_authority(authority, ledger):
    require(
        set(authority) == {"schema_version", "catalog_version", "binding_policy", "modules"},
        "authority envelope must be closed",
    )
    require(
        authority["schema_version"] == "axiom.compiler.stdlib_catalog.authority.v1",
        "authority schema version mismatch",
    )
    require(authority["catalog_version"] == "2.0.0", "authority catalog version mismatch")
    require(
        authority["binding_policy"] == "module_provider_namespace",
        "authority binding policy mismatch",
    )
    require(
        isinstance(authority["modules"], list) and authority["modules"],
        "authority modules must be non-empty",
    )

    names = []
    namespaces = set()
    authority_by_name = {}
    ledger_by_name = ledger_module_index(ledger)
    for module in authority["modules"]:
        base_fields = {"name", "capabilities", "binding_namespace"}
        require(
            frozenset(module)
            in {frozenset(base_fields), frozenset(base_fields | {"symbol_capability_policy"})},
            "authority module must be closed",
        )
        name = module["name"]
        require(
            isinstance(name, str) and re.fullmatch(r"std/[a-z0-9_]+\.ax", name) is not None,
            "invalid authority module name",
        )
        require(name not in authority_by_name, "duplicate authority module")
        require(name in ledger_by_name, "authority module is absent from capability ledger")
        capabilities = module["capabilities"]
        validate_capability_list(capabilities, "authority capabilities")
        policy = module.get("symbol_capability_policy")
        if policy is not None:
            require(
                isinstance(policy, dict) and set(policy) == {"default", "overrides"},
                "symbol capability policy must be closed",
            )
            default = policy["default"]
            overrides = policy["overrides"]
            validate_capability_list(default, "default symbol capabilities")
            require(
                isinstance(overrides, dict)
                and list(overrides) == sorted(overrides)
                and overrides,
                "symbol capability overrides must be non-empty and sorted",
            )
            require(
                all(
                    isinstance(symbol, str)
                    and symbol in ledger_by_name[name]["functions"]
                    and isinstance(values, list)
                    and values == sorted(set(values))
                    and all(capability in CAPABILITY_IDS for capability in values)
                    and values != default
                    for symbol, values in overrides.items()
                ),
                "invalid symbol capability override",
            )
            symbol_capabilities = [
                overrides.get(symbol, default)
                for symbol in ledger_by_name[name]["functions"]
            ]
            require(
                sorted({capability for values in symbol_capabilities for capability in values})
                == capabilities,
                "symbol capabilities must aggregate to module capabilities",
            )
        namespace = module["binding_namespace"]
        require(
            isinstance(namespace, str)
            and re.fullmatch(
                r"axiom://provider/[A-Za-z0-9._~-]+(?:/[A-Za-z0-9._~-]+)*",
                namespace,
            )
            is not None,
            "invalid authority provider namespace",
        )
        require(
            all(segment not in {".", ".."} for segment in namespace.removeprefix("axiom://provider/").split("/")),
            "invalid authority provider namespace segment",
        )
        require("rust" not in namespace.lower(), "host-language provider namespace leaked")
        require(namespace not in namespaces, "duplicate authority provider namespace")
        namespaces.add(namespace)
        names.append(name)
        authority_by_name[name] = module
    require(names == sorted(names), "authority modules must be deterministically sorted")

    require(
        set(authority_by_name) == set(ledger_by_name),
        "authority/ledger module parity drift",
    )
    for name, module in authority_by_name.items():
        require(
            module["capabilities"] == sorted(ledger_by_name[name]["capabilities"]),
            f"authority/ledger capability drift for {name}",
        )


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
ledger = load_json(LEDGER)
authority = load_json(AUTHORITY)
validate_authority(authority, ledger)
expected = build(ledger, authority)
if args.write:
    SNAPSHOT.write_text(json.dumps(expected, indent=2) + "\n")
catalog = load_json(SNAPSHOT)
schema = load_json(SCHEMA)
require(catalog == expected, "stdlib catalog drift; regenerate with --write")
validate_catalog(catalog, schema)
output = {
    "ok": True,
    "modules": len(catalog["modules"]),
    "symbols": sum(len(module["symbols"]) for module in catalog["modules"]),
    "release_digest": catalog["release_digest"],
    "authority_source": CATALOG_SOURCE,
    "authority_digest": hashlib.sha256(AUTHORITY.read_bytes()).hexdigest(),
}
print(json.dumps(output, sort_keys=True) if args.json else output)
