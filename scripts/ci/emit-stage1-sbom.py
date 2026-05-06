#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Emit an SPDX JSON SBOM for the stage1 Rust workspace.")
    parser.add_argument("--manifest-path", required=True, help="Path to the stage1 Cargo.toml manifest")
    parser.add_argument("--output", required=True, help="Output path for the SPDX JSON document")
    return parser.parse_args()


def run_metadata(manifest_path: str) -> dict[str, Any]:
    proc = subprocess.run(
        [
            "cargo",
            "metadata",
            "--manifest-path",
            manifest_path,
            "--format-version",
            "1",
            "--locked",
            "--offline",
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(proc.stdout)


def created_timestamp() -> str:
    source_date_epoch = os.environ.get("SOURCE_DATE_EPOCH")
    if source_date_epoch:
        instant = datetime.fromtimestamp(int(source_date_epoch), tz=timezone.utc)
    else:
        instant = datetime.now(timezone.utc)
    return instant.replace(microsecond=0).isoformat().replace("+00:00", "Z")


def spdx_id(prefix: str, value: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9.-]+", "-", value).strip("-") or "item"
    return f"SPDXRef-{prefix}-{cleaned}"


def package_purl(name: str, version: str) -> str:
    return f"pkg:cargo/{name}@{version}"


def main() -> int:
    args = parse_args()
    metadata = run_metadata(args.manifest_path)
    manifest_path = Path(args.manifest_path).resolve()
    workspace_root = Path(metadata["workspace_root"]).resolve()
    output_path = Path(args.output).resolve()
    output_path.parent.mkdir(parents=True, exist_ok=True)

    package_by_id = {package["id"]: package for package in metadata["packages"]}
    root_ids = set(metadata.get("workspace_members", []))
    packages = []
    package_ids = {}

    sorted_packages = sorted(
        metadata["packages"],
        key=lambda pkg: (pkg["name"], pkg["version"], pkg.get("source") or "path"),
    )

    for package in sorted_packages:
        package_id = spdx_id("Package", f"{package['name']}-{package['version']}")
        package_ids[package["id"]] = package_id
        checksum = package.get("checksum")
        download_location = package.get("source") or "NOASSERTION"
        package_entry: dict[str, Any] = {
            "SPDXID": package_id,
            "name": package["name"],
            "versionInfo": package["version"],
            "downloadLocation": download_location,
            "filesAnalyzed": False,
            "licenseConcluded": "NOASSERTION",
            "licenseDeclared": "NOASSERTION",
            "externalRefs": [
                {
                    "referenceCategory": "PACKAGE-MANAGER",
                    "referenceType": "purl",
                    "referenceLocator": package_purl(package["name"], package["version"]),
                }
            ],
        }
        if checksum:
            package_entry["checksums"] = [{"algorithm": "SHA256", "checksumValue": checksum}]
        manifest_file = Path(package["manifest_path"]).resolve()
        package_entry["primaryPackagePurpose"] = "LIBRARY"
        try:
            relative_manifest = manifest_file.relative_to(workspace_root)
            package_entry["summary"] = f"Cargo package manifest at {relative_manifest}"
        except ValueError:
            package_entry["summary"] = "Third-party Cargo package"
        packages.append(package_entry)

    relationships = []
    relationships.append(
        {
            "spdxElementId": "SPDXRef-DOCUMENT",
            "relationshipType": "DESCRIBES",
            "relatedSpdxElement": spdx_id("Workspace", workspace_root.name),
        }
    )

    workspace_package_id = spdx_id("Workspace", workspace_root.name)
    packages.insert(
        0,
        {
            "SPDXID": workspace_package_id,
            "name": workspace_root.name,
            "versionInfo": "NOASSERTION",
            "downloadLocation": "NOASSERTION",
            "filesAnalyzed": False,
            "licenseConcluded": "NOASSERTION",
            "licenseDeclared": "NOASSERTION",
            "primaryPackagePurpose": "APPLICATION",
            "summary": "Cargo workspace root for the stage1 toolchain",
        },
    )

    for root_id in sorted(root_ids):
        if root_id in package_ids:
            relationships.append(
                {
                    "spdxElementId": workspace_package_id,
                    "relationshipType": "CONTAINS",
                    "relatedSpdxElement": package_ids[root_id],
                }
            )

    resolve = metadata.get("resolve") or {}
    for node in resolve.get("nodes", []):
        source_id = package_ids.get(node["id"])
        if source_id is None:
            continue
        for dependency in sorted(node.get("deps", []), key=lambda dep: dep["pkg"]):
            target_id = package_ids.get(dependency["pkg"])
            if target_id is None:
                continue
            relationships.append(
                {
                    "spdxElementId": source_id,
                    "relationshipType": "DEPENDS_ON",
                    "relatedSpdxElement": target_id,
                }
            )

    namespace_seed = json.dumps(
        {
            "workspace_root": str(workspace_root),
            "workspace_members": sorted(root_ids),
            "resolve": resolve,
        },
        sort_keys=True,
    ).encode("utf-8")
    namespace_hash = hashlib.sha256(namespace_seed).hexdigest()

    document = {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": "axiom-stage1-sbom",
        "documentNamespace": f"https://github.com/OMT-Global/axiom/sbom/stage1/{namespace_hash}",
        "creationInfo": {
            "created": created_timestamp(),
            "creators": ["Tool: scripts/ci/emit-stage1-sbom.py"],
        },
        "documentDescribes": [workspace_package_id],
        "packages": packages,
        "relationships": relationships,
    }

    output_path.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(output_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
