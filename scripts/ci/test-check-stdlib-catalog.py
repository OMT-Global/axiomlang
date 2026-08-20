#!/usr/bin/env python3
"""Regression coverage for typed stdlib catalog parity and schema guards."""
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/ci/check-stdlib-catalog.py"
SNAPSHOT = ROOT / "stage1/compiler-contracts/snapshots/stdlib-catalog.json"
AUTHORITY = ROOT / "stage1/compiler-contracts/sources/stdlib-catalog-authority-v1.json"
LEDGER = ROOT / "stage1/compiler-contracts/snapshots/capability-ledger.json"
CANONICAL_SNAPSHOT = json.loads(SNAPSHOT.read_text())
CANONICAL_AUTHORITY = json.loads(AUTHORITY.read_text())
CANONICAL_LEDGER = json.loads(LEDGER.read_text())


def run(root):
    return subprocess.run([sys.executable, str(root / "scripts/ci/check-stdlib-catalog.py")], cwd=root, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def restore_inputs(root):
    snapshot = root / "stage1/compiler-contracts/snapshots/stdlib-catalog.json"
    authority = root / "stage1/compiler-contracts/sources/stdlib-catalog-authority-v1.json"
    ledger = root / "stage1/compiler-contracts/snapshots/capability-ledger.json"
    snapshot.write_text(json.dumps(CANONICAL_SNAPSHOT))
    authority.write_text(json.dumps(CANONICAL_AUTHORITY))
    ledger.write_text(json.dumps(CANONICAL_LEDGER))
    shutil.copy2(ROOT / "stage1/crates/axiomc/src/stdlib.rs", root / "stage1/crates/axiomc/src/stdlib.rs")
    shutil.copy2(ROOT / "stage1/crates/axiomc/src/project.rs", root / "stage1/crates/axiomc/src/project.rs")


def require_rejected(root, value, message):
    restore_inputs(root)
    path = root / "stage1/compiler-contracts/snapshots/stdlib-catalog.json"
    path.write_text(json.dumps(value))
    result = run(root)
    if result.returncode == 0:
        raise SystemExit(message)
    if "stdlib catalog drift" not in result.stderr:
        raise SystemExit(f"{message}: wrong failure reason: {result.stderr}")


def require_authority_rejected(root, value, message, expected):
    restore_inputs(root)
    path = root / "stage1/compiler-contracts/sources/stdlib-catalog-authority-v1.json"
    path.write_text(json.dumps(value))
    result = run(root)
    if result.returncode == 0:
        raise SystemExit(message)
    if expected not in result.stderr:
        raise SystemExit(f"{message}: wrong failure reason: {result.stderr}")


def require_raw_authority_rejected(root, value, message, expected):
    restore_inputs(root)
    path = root / "stage1/compiler-contracts/sources/stdlib-catalog-authority-v1.json"
    path.write_text(value)
    result = run(root)
    if result.returncode == 0:
        raise SystemExit(message)
    if expected not in result.stderr:
        raise SystemExit(f"{message}: wrong failure reason: {result.stderr}")


def require_source_rejected(root, relative_path, transform, message, expected):
    restore_inputs(root)
    path = root / relative_path
    path.write_text(transform(path.read_text()))
    result = run(root)
    if result.returncode == 0:
        raise SystemExit(message)
    if expected not in result.stderr:
        raise SystemExit(f"{message}: wrong failure reason: {result.stderr}")


def require_source_accepted(root, relative_path, transform, message):
    restore_inputs(root)
    path = root / relative_path
    path.write_text(transform(path.read_text()))
    result = run(root)
    if result.returncode != 0:
        raise SystemExit(f"{message}: {result.stderr}")


def require_ledger_rejected(root, value, message, expected):
    restore_inputs(root)
    path = root / "stage1/compiler-contracts/snapshots/capability-ledger.json"
    path.write_text(json.dumps(value))
    result = run(root)
    if result.returncode == 0:
        raise SystemExit(message)
    if expected not in result.stderr:
        raise SystemExit(f"{message}: wrong failure reason: {result.stderr}")


def require_raw_ledger_rejected(root, value, message, expected):
    restore_inputs(root)
    path = root / "stage1/compiler-contracts/snapshots/capability-ledger.json"
    path.write_text(value)
    result = run(root)
    if result.returncode == 0:
        raise SystemExit(message)
    if expected not in result.stderr:
        raise SystemExit(f"{message}: wrong failure reason: {result.stderr}")


with tempfile.TemporaryDirectory() as directory:
    root = Path(directory) / "repo"
    shutil.copytree(ROOT / "stage1", root / "stage1")
    (root / "scripts/ci").mkdir(parents=True)
    shutil.copy2(CHECKER, root / "scripts/ci/check-stdlib-catalog.py")
    if run(root).returncode != 0:
        raise SystemExit("valid typed stdlib catalog was rejected")
    value = json.loads(SNAPSHOT.read_text())
    value["modules"][0]["symbols"][0]["signature"] = "fn malformed(): unknown"
    require_rejected(root, value, "stdlib catalog accepted a malformed symbol signature")
    value = json.loads(SNAPSHOT.read_text())
    value["modules"][0]["symbols"][0]["provider"]["id"] = "axiom://provider/other"
    require_rejected(root, value, "stdlib catalog accepted a mismatched provider declaration")
    value = json.loads(SNAPSHOT.read_text())
    value["modules"][0]["module_loading"]["source_digest"] = "0" * 64
    require_rejected(root, value, "stdlib catalog accepted a stale module loading digest")
    value = json.loads(SNAPSHOT.read_text())
    del value["acceptance_boundary"]
    require_rejected(root, value, "stdlib catalog accepted a missing acceptance boundary")
    value = json.loads(SNAPSHOT.read_text())
    value["source"] = "stage1/compiler-contracts/snapshots/capability-ledger.json"
    require_rejected(root, value, "stdlib catalog accepted the derived ledger as semantic authority")

    authority = json.loads(AUTHORITY.read_text())
    authority["modules"][0]["capabilities"] = []
    require_authority_rejected(root, authority, "stdlib authority accepted capability/ledger drift", "authority/ledger capability drift")
    authority = json.loads(AUTHORITY.read_text())
    authority["modules"][1]["binding_namespace"] = authority["modules"][0]["binding_namespace"]
    require_authority_rejected(root, authority, "stdlib authority accepted a duplicate provider namespace", "duplicate authority provider namespace")
    authority = json.loads(AUTHORITY.read_text())
    authority["modules"][0]["binding_namespace"] = "axiom://provider/rust/bootstrap"
    require_authority_rejected(root, authority, "stdlib authority accepted a host-language provider namespace", "host-language provider namespace leaked")
    authority = json.loads(AUTHORITY.read_text())
    authority["modules"][0]["binding_namespace"] += "/"
    require_authority_rejected(root, authority, "stdlib authority accepted an ambiguous provider namespace", "invalid authority provider namespace")
    authority = json.loads(AUTHORITY.read_text())
    authority["modules"][0]["binding_namespace"] = "axiom://provider/stage1-v1//async"
    require_authority_rejected(root, authority, "stdlib authority accepted an empty provider namespace segment", "invalid authority provider namespace")
    authority = json.loads(AUTHORITY.read_text())
    authority["modules"][0], authority["modules"][1] = authority["modules"][1], authority["modules"][0]
    require_authority_rejected(root, authority, "stdlib authority accepted nondeterministic module order", "authority modules must be deterministically sorted")
    authority = json.loads(AUTHORITY.read_text())
    authority["modules"][0]["extension"] = {}
    require_authority_rejected(root, authority, "stdlib authority accepted an unknown module field", "authority module must be closed")
    authority = json.loads(AUTHORITY.read_text())
    authority["modules"][0]["capabilities"] = ["async,net"]
    require_authority_rejected(root, authority, "stdlib authority accepted a non-canonical capability", "invalid authority capability")
    authority = AUTHORITY.read_text().replace(
        '"catalog_version": "2.0.0",',
        '"catalog_version": "2.0.0",\n  "catalog_version": "2.0.0",',
        1,
    )
    require_raw_authority_rejected(root, authority, "stdlib authority accepted a duplicate JSON key", "duplicate JSON key")
    for segment in (".", ".."):
        authority = json.loads(AUTHORITY.read_text())
        authority["modules"][0]["binding_namespace"] += f"/{segment}/module"
        require_authority_rejected(root, authority, "stdlib authority accepted a relative provider namespace segment", "invalid authority provider namespace segment")

    require_source_rejected(
        root,
        "stage1/crates/axiomc/src/stdlib.rs",
        lambda source: source.replace(
            'const STDLIB_SOURCES: &[(&str, &str)] = &[',
            'const STDLIB_SOURCES: &[(&str, &str)] = &[\n    ("review_extra.ax", "pub fn review_extra(): int {\\nreturn 0\\n}\\n"),',
            1,
        ),
        "stdlib authority ignored an extra embedded module",
        "authority/embedded source module parity drift",
    )
    require_source_rejected(
        root,
        "stage1/crates/axiomc/src/project.rs",
        lambda source: source.replace(
            '("async.ax", _) => Some(::std::vec![CapabilityKind::Async]),',
            '/* ("async.ax", _) => Some(::std::vec![CapabilityKind::Async]), */',
            1,
        ),
        "stdlib checker counted a commented-out runtime capability arm",
        "bootstrap runtime capability parity drift",
    )
    require_source_accepted(
        root,
        "stage1/crates/axiomc/src/project.rs",
        lambda source: source.replace(
            "fn stdlib_wrapper_capabilities(",
            "mod delimiter_literal_decoy {\n"
            "    const CLOSE: &str = \"}\";\n"
            "    fn stdlib_wrapper_capabilities() {}\n"
            "    const OPEN: &str = r#\"{\"#;\n"
            "}\n\n"
            "fn stdlib_wrapper_capabilities(",
            1,
        ),
        "stdlib checker treated string contents as Rust delimiters",
    )
    require_source_accepted(
        root,
        "stage1/crates/axiomc/src/project.rs",
        lambda source: source.replace(
            "fn stdlib_wrapper_capabilities(",
            "mod raw_c_literal_decoy {\n"
            "    const VALUE: &core::ffi::CStr = cr##\"first\" } fn stdlib_wrapper_capabilities() { } { \"last\"##;\n"
            "}\n\n"
            "fn stdlib_wrapper_capabilities(",
            1,
        ),
        "stdlib checker exposed raw C-string contents as Rust syntax",
    )
    require_source_rejected(
        root,
        "stage1/crates/axiomc/src/project.rs",
        lambda source: source.replace("_ => None", 'r#"_"# => None', 1),
        "stdlib checker accepted literal text as the default pattern",
        "invalid runtime capability arm",
    )
    require_source_rejected(
        root,
        "stage1/crates/axiomc/src/project.rs",
        lambda source: source.replace(
            ") -> Option<Vec<CapabilityKind>> {",
            ") -> capability_type! { Option<Vec<CapabilityKind>> } {",
            1,
        ),
        "stdlib checker parsed a signature macro token tree as the function body",
        "bootstrap runtime capability function signature drift",
    )
    require_source_rejected(
        root,
        "stage1/crates/axiomc/src/project.rs",
        lambda source: source.replace(
            'let module = stdlib_module_file(module_path)?;',
            'let module = stdlib_module_file(module_path)?;\n    let _fake = r#"("async.ax", _) => Some(vec![CapabilityKind::Async])"#;',
            1,
        ).replace(
            '("async.ax", _) => Some(::std::vec![CapabilityKind::Async]),',
            '/* ("async.ax", _) => Some(::std::vec![CapabilityKind::Async]), */',
            1,
        ),
        "stdlib checker accepted arm-shaped raw-string text",
        "bootstrap runtime capability function syntax drift",
    )
    require_source_rejected(
        root,
        "stage1/crates/axiomc/src/project.rs",
        lambda source: source.replace(
            '("async.ax", _) => Some(::std::vec![CapabilityKind::Async]),',
            '#[cfg(any())]\n        ("async.ax", _) => Some(::std::vec![CapabilityKind::Async]),',
            1,
        ),
        "stdlib checker accepted a configured-out runtime capability arm",
        "invalid runtime capability arm",
    )
    require_source_rejected(
        root,
        "stage1/crates/axiomc/src/project.rs",
        lambda source: source.replace(
            "fn stdlib_wrapper_capabilities(",
            "/* fn stdlib_wrapper_capabilities(\n"
            "    _module_path: &Path,\n"
            "    _function_name: &str,\n"
            ") -> Option<Vec<CapabilityKind>> {\n"
            "    Some(vec![CapabilityKind::Async])\n"
            "}\n"
            "fn validate_program_capabilities( */\n"
            "fn stdlib_wrapper_capabilities(",
            1,
        ).replace(
            '("async.ax", _) => Some(::std::vec![CapabilityKind::Async]),',
            '/* ("async.ax", _) => Some(::std::vec![CapabilityKind::Async]), */',
            1,
        ),
        "stdlib checker selected a commented capability-function decoy",
        "bootstrap runtime capability parity drift",
    )
    require_source_rejected(
        root,
        "stage1/crates/axiomc/src/project.rs",
        lambda source: source.replace(
            "fn stdlib_wrapper_capabilities(",
            "#[cfg(any())]\n"
            "fn stdlib_wrapper_capabilities(\n"
            "    _module_path: &Path,\n"
            "    _function_name: &str,\n"
            ") -> Option<Vec<CapabilityKind>> {\n"
            "    Some(vec![CapabilityKind::Async])\n"
            "}\n\n"
            "fn stdlib_wrapper_capabilities(",
            1,
        ),
        "stdlib checker accepted a cfg-disabled capability-function decoy",
        "bootstrap runtime capability function must have exactly one active top-level definition",
    )
    require_source_rejected(
        root,
        "stage1/crates/axiomc/src/project.rs",
        lambda source: source.replace(
            "fn stdlib_wrapper_capabilities(",
            "#[cfg(any())]\npub(crate) fn stdlib_wrapper_capabilities(",
            1,
        ),
        "stdlib checker accepted an attributed qualified capability function",
        "bootstrap runtime capability function must not have item attributes or qualifiers",
    )
    require_source_rejected(
        root,
        "stage1/crates/axiomc/src/project.rs",
        lambda source: source.replace(
            "fn stdlib_wrapper_capabilities(",
            "/// governed function decoy\npub(crate) fn stdlib_wrapper_capabilities(",
            1,
        ),
        "stdlib checker accepted an outer-doc-attributed capability function",
        "bootstrap runtime capability function must not have item attributes or qualifiers",
    )
    require_source_rejected(
        root,
        "stage1/crates/axiomc/src/project.rs",
        lambda source: source.replace(
            "fn stdlib_wrapper_capabilities(",
            "pub fn stdlib_wrapper_capabilities(",
            1,
        ),
        "stdlib checker accepted a visibility-qualified capability function",
        "bootstrap runtime capability function must not have item attributes or qualifiers",
    )
    require_source_rejected(
        root,
        "stage1/crates/axiomc/src/project.rs",
        lambda source: source.replace(
            "fn stdlib_wrapper_capabilities(",
            "macro_rules! vec { ($($value:expr),* $(,)?) => { ::std::vec::Vec::new() }; }\n\n"
            "fn stdlib_wrapper_capabilities(",
            1,
        ).replace("::std::vec!", "vec!"),
        "stdlib checker accepted a shadowable runtime capability constructor",
        "invalid runtime capability result",
    )
    require_source_rejected(
        root,
        "stage1/crates/axiomc/src/project.rs",
        lambda source: source.replace(
            "fn stdlib_wrapper_capabilities(",
            "mod capability_decoy {\n"
            "    const CLOSE: char = '}';\n"
            "    fn stdlib_wrapper_capabilities(",
            1,
        ).replace(
            "fn validate_program_capabilities(",
            "    const OPEN: char = '{';\n}\n\n"
            "use capability_provider::stdlib_wrapper_capabilities;\n\n"
            "fn validate_program_capabilities(",
            1,
        ),
        "stdlib checker accepted a character-literal delimiter decoy",
        "bootstrap runtime capability function must have exactly one active top-level definition",
    )
    require_source_rejected(
        root,
        "stage1/crates/axiomc/src/project.rs",
        lambda source: source.replace(
            "fn stdlib_wrapper_capabilities(",
            "#[cfg(any())]\n"
            "mod capability_decoy {\n"
            "    fn dummy() {}\n"
            "    fn stdlib_wrapper_capabilities(\n"
            "        _module_path: &Path,\n"
            "        _function_name: &str,\n"
            "    ) -> Option<Vec<CapabilityKind>> {\n"
            "        Some(vec![CapabilityKind::Async])\n"
            "    }\n"
            "}\n\n"
            "use capability_provider::stdlib_wrapper_capabilities;\n\n"
            "fn renamed_stdlib_wrapper_capabilities(",
            1,
        ),
        "stdlib checker accepted a nested disabled capability-function decoy",
        "bootstrap runtime capability function must have exactly one active top-level definition",
    )
    ledger = json.loads(LEDGER.read_text())
    ledger["stdlib"].append(dict(ledger["stdlib"][0]))
    require_ledger_rejected(
        root,
        ledger,
        "stdlib checker accepted duplicate capability-ledger module rows",
        "duplicate capability ledger module",
    )
    for constant in ("NaN", "Infinity", "-Infinity"):
        ledger = LEDGER.read_text().replace("{", f'{{"review_nonfinite": {constant},', 1)
        require_raw_ledger_rejected(
            root,
            ledger,
            f"stdlib checker accepted the non-finite JSON constant {constant}",
            "non-finite JSON constant",
        )
    ledger = LEDGER.read_text().replace("{", '{"review_nonfinite": 1e10000,', 1)
    require_raw_ledger_rejected(
        root,
        ledger,
        "stdlib checker accepted a JSON number that overflows to infinity",
        "non-finite JSON number",
    )

    authority = json.loads(AUTHORITY.read_text())
    fs_module = next(module for module in authority["modules"] if module["name"] == "std/fs.ax")
    fs_module["symbol_capability_policy"]["overrides"]["read_file"] = []
    require_authority_rejected(root, authority, "stdlib authority capability changes bypassed bootstrap parity", "bootstrap runtime capability parity drift")

print("stdlib catalog checker tests passed")
