//! Deterministic generator for the checked-in quickstart registry fixtures.
//!
//! Issue #1663 (phase 1): the README quickstart and `docs/package.md` document a
//! signed-registry verification flow against a `./registry/` tree that did not
//! exist on a fresh clone. `scripts/registry-fixtures/generate-registry-fixtures.sh`
//! regenerates that tree end to end; this example produces the two policy inputs
//! that the documented flow requires consumers/operators to supply:
//!
//! * `trust-roots.json` — a Package Trust root envelope (trusted root v1 plus a
//!   candidate root v2 with a signed transition), built with the exact same
//!   canonicalization, transcript, and Ed25519 machinery the validator uses
//!   (`canonical_json`, `metadata_transcript`, `ROOT_DOMAIN`).
//! * `expectation-template.json` / pinned `verification-request.json` — the
//!   offline verification expectation, derived from the checked-in canonical
//!   contract (`stage1/package-trust/contract/package-trust.json`) the same way
//!   `tests/package_trust_cli.rs` derives its fixtures.
//!
//! All signed release artifacts (`packages/**`) and the signed index
//! (`index.json`) are produced by the real `axiomc publish` / `axiomc
//! registry-index` CLI subcommands in the shell driver, never by this example.
//!
//! Modes:
//!
//! ```text
//! registry_quickstart_fixtures inputs  --out DIR [--contract PATH]
//! registry_quickstart_fixtures prepin  --template T --roots R --signature SIG \
//!     --out E --registry-identity RI --source-identity SI --generation G \
//!     --sequence S --issued-at TS --expires-at TS --snapshot-id ID \
//!     --metadata-path P --previous-snapshot-sha256 HEX
//! ```
//!
//! `inputs` writes the trust roots, deterministic test-only signing seeds (as
//! 64-character lowercase hex, the format `load_cli_signers` accepts), and the
//! expectation template. The seeds are throwaway fixture material derived from
//! fixed single-byte constants; they carry no authority anywhere and are not
//! checked in.
//!
//! `prepin` reads the real published `package.axp.sig` envelope, reconstructs
//! the exact v2 index transcript the driver's `registry-index` invocation will
//! produce (same algorithm as `pin_fixture_to_candidate_index` in the CLI
//! tests), and writes the final expectation whose offline lock pins the exact
//! index transcript, release coordinates, and root state. It prints the pinned
//! index transcript sha256 so the driver can assert it against the real index.
//!
//! Determinism: fixed seeds, identities, and timestamps; no wall-clock or
//! randomness anywhere. Regenerating from the same commit is byte-identical.

use axiomc::package_trust::{
    INDEX_DOMAIN, ROOT_DOMAIN, canonical_json, metadata_transcript, parse_package_signature_json,
};
use ed25519_dalek::{Signer as _, SigningKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::ExitCode;

/// Registry identity used across the fixture set (matches docs/package.md).
const FIXTURE_REGISTRY_IDENTITY: &str = "axiom-registry-production";
/// Source identity used across the fixture set (matches docs/package.md).
const FIXTURE_SOURCE_IDENTITY: &str = "registry:axiom-production";
/// Publisher identity granted the fixture namespace (matches docs/package.md).
const FIXTURE_PUBLISHER_IDENTITY: &str = "https://publishers.example/foundation";
/// Namespace the fixture release is published under.
const FIXTURE_NAMESPACE: &str = "axiom";
/// Package name; must match `stage1/examples/hello/axiom.toml`. A mismatch
/// fails closed: the root grant no longer covers the publish and the driver
/// aborts before any fixture is written.
const FIXTURE_PACKAGE_NAME: &str = "hello-stage1";
/// Package version; must match `stage1/examples/hello/axiom.toml`.
const FIXTURE_PACKAGE_VERSION: &str = "0.1.0";
/// Static verification time for the offline expectation. The Package Trust
/// validator compares metadata validity windows against this value only; it
/// never reads a wall clock, so the checked-in fixtures stay executable.
const FIXTURE_VERIFICATION_TIME: &str = "2026-09-24T12:00:00Z";
/// Snapshot id for generation 1 / sequence 1 of the fixture index.
const FIXTURE_SNAPSHOT_ID: &str = "axiom-registry-production.1.1";
/// Root publisher identity for bootstrap keys (mirrors the CLI test fixture).
const ROOT_PUBLISHER_IDENTITY: &str = "axiom://trust/root";
/// Publisher identity for registry-index role keys.
const REGISTRY_PUBLISHER_IDENTITY: &str = "axiom://registry/official";
/// Default contract path (repo-relative) for the expectation template.
const DEFAULT_CONTRACT_PATH: &str = "stage1/package-trust/contract/package-trust.json";

struct FixtureSigner(SigningKey);

impl FixtureSigner {
    /// Test-only signer from a fixed single-byte seed, exactly like
    /// `TestSigner::new` in `tests/package_trust_cli.rs`.
    fn new(seed: u8) -> Self {
        Self(SigningKey::from_bytes(&[seed; 32]))
    }

    fn public_key_hex(&self) -> String {
        hex_encode(&self.0.verifying_key().to_bytes())
    }

    fn key_material(&self) -> Value {
        json!({
            "algorithm": "ed25519",
            "public_key_encoding": "lowercase-hex",
            "public_key": self.public_key_hex()
        })
    }

    fn key_id(&self) -> String {
        let material = canonical_json(&self.key_material()).expect("canonical key material");
        format!("sha256:{}", sha256_hex(&material))
    }

    fn trust_key(&self, publisher: &str) -> Value {
        json!({
            "key_id": self.key_id(),
            "key_material": self.key_material(),
            "publisher_identity": publisher,
            "status": "active",
            "valid_from_sequence": 1,
            "supersedes_key_ids": [],
            "revocation": null
        })
    }

    fn signature_over(&self, transcript: &[u8]) -> Value {
        json!({
            "key_id": self.key_id(),
            "algorithm": "ed25519",
            "encoding": "lowercase-hex",
            "value": hex_encode(&self.0.sign(transcript).to_bytes())
        })
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn root_envelope(signed: Value, signers: &[FixtureSigner]) -> Value {
    let transcript = metadata_transcript(ROOT_DOMAIN, &signed).expect("root transcript");
    json!({
        "signed": signed,
        "transcript": {
            "encoding": "axiom-canonical-json-v1",
            "domain": ROOT_DOMAIN,
            "bytes_hex": hex_encode(&transcript),
            "sha256": sha256_hex(&transcript)
        },
        "signatures": signers
            .iter()
            .map(|signer| signer.signature_over(&transcript))
            .collect::<Vec<_>>()
    })
}

fn root_policy() -> Value {
    json!({
        "rollback_protection": "reject rollback",
        "freeze_protection": "reject expiry",
        "downgrade_protection": "reject downgrade",
        "offline_locked": "require exact pins",
        "metadata_expiry_required": true,
        "registry_index_equivalence": "combined metadata role"
    })
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(value).expect("serialize fixture metadata");
    fs::write(path, bytes).map_err(|error| format!("write {}: {error}", path.display()))
}

struct InputsArgs {
    out: String,
    contract: String,
}

struct PrepinArgs {
    template: String,
    roots: String,
    signature: String,
    out: String,
    registry_identity: String,
    source_identity: String,
    generation: u64,
    sequence: u64,
    issued_at: String,
    expires_at: String,
    snapshot_id: String,
    metadata_path: String,
    previous_snapshot_sha256: String,
}

fn parse_flag(args: &[String], flag: &str) -> Result<String, String> {
    args.iter()
        .position(|argument| argument == flag)
        .and_then(|position| args.get(position + 1))
        .cloned()
        .ok_or_else(|| format!("missing required flag {flag}"))
}

fn run_inputs(args: InputsArgs) -> Result<(), String> {
    // Fixed test-only key material, mirroring the CLI test fixture roles:
    // old root = [1, 2, 9], candidate root = [3, 4], registry-index = [5, 6],
    // package targets = [7, 8]. Threshold 2 everywhere.
    let old_root = [
        FixtureSigner::new(1),
        FixtureSigner::new(2),
        FixtureSigner::new(9),
    ];
    let new_root = [FixtureSigner::new(3), FixtureSigner::new(4)];
    let index_signers = [FixtureSigner::new(5), FixtureSigner::new(6)];
    let package_signers = [FixtureSigner::new(7), FixtureSigner::new(8)];
    let old_root_ids = old_root
        .iter()
        .map(|signer| signer.key_id())
        .collect::<Vec<_>>();
    let new_root_ids = new_root
        .iter()
        .map(|signer| signer.key_id())
        .collect::<Vec<_>>();
    let index_ids = index_signers
        .iter()
        .map(|signer| signer.key_id())
        .collect::<Vec<_>>();
    let package_ids = package_signers
        .iter()
        .map(|signer| signer.key_id())
        .collect::<Vec<_>>();

    let old_signed = json!({
        "specification": "axiom-package-trust-root-v1",
        "root_version": 1,
        "sequence": 1,
        "issued_at": "2026-01-01T00:00:00Z",
        "expires_at": "2036-01-01T00:00:00Z",
        "consistent_snapshot": true,
        "keys": old_root
            .iter()
            .map(|signer| signer.trust_key(ROOT_PUBLISHER_IDENTITY))
            .collect::<Vec<_>>(),
        "publisher_identities": [{
            "publisher_identity": ROOT_PUBLISHER_IDENTITY,
            "display_name": "Quickstart fixture bootstrap root"
        }],
        "namespace_grants": [{
            "publisher_identity": ROOT_PUBLISHER_IDENTITY,
            "namespace": "bootstrap",
            "package_names": ["bootstrap"],
            "registry_identities": [FIXTURE_REGISTRY_IDENTITY],
            "source_identities": [FIXTURE_SOURCE_IDENTITY],
            "role_id": "registry-index"
        }],
        "roles": [
            {"role_id": "root", "threshold": 2, "key_ids": old_root_ids.clone(), "delegated_by": null},
            {"role_id": "timestamp", "threshold": 2, "key_ids": old_root_ids.clone(), "delegated_by": "root"},
            {"role_id": "snapshot", "threshold": 2, "key_ids": old_root_ids.clone(), "delegated_by": "timestamp"},
            {"role_id": "registry-index", "threshold": 2, "key_ids": old_root_ids, "delegated_by": "snapshot"}
        ],
        "policy": root_policy()
    });
    let candidate_signed = json!({
        "specification": "axiom-package-trust-root-v1",
        "root_version": 2,
        "sequence": 2,
        "issued_at": "2026-09-01T00:00:00Z",
        "expires_at": "2036-09-01T00:00:00Z",
        "consistent_snapshot": true,
        "keys": new_root
            .iter()
            .map(|signer| signer.trust_key(ROOT_PUBLISHER_IDENTITY))
            .chain(
                index_signers
                    .iter()
                    .map(|signer| signer.trust_key(REGISTRY_PUBLISHER_IDENTITY)),
            )
            .chain(
                package_signers
                    .iter()
                    .map(|signer| signer.trust_key(FIXTURE_PUBLISHER_IDENTITY)),
            )
            .collect::<Vec<_>>(),
        "publisher_identities": [{
            "publisher_identity": FIXTURE_PUBLISHER_IDENTITY,
            "display_name": "Quickstart fixture publisher"
        }],
        "namespace_grants": [{
            "publisher_identity": FIXTURE_PUBLISHER_IDENTITY,
            "namespace": FIXTURE_NAMESPACE,
            "package_names": [FIXTURE_PACKAGE_NAME],
            "registry_identities": [FIXTURE_REGISTRY_IDENTITY],
            "source_identities": [FIXTURE_SOURCE_IDENTITY],
            "role_id": "targets:axiom"
        }],
        "roles": [
            {"role_id": "root", "threshold": 2, "key_ids": new_root_ids, "delegated_by": null},
            {"role_id": "timestamp", "threshold": 2, "key_ids": index_ids.clone(), "delegated_by": "root"},
            {"role_id": "snapshot", "threshold": 2, "key_ids": index_ids.clone(), "delegated_by": "timestamp"},
            {"role_id": "registry-index", "threshold": 2, "key_ids": index_ids, "delegated_by": "snapshot"},
            {"role_id": "targets", "threshold": 2, "key_ids": package_ids.clone(), "delegated_by": "root"},
            {"role_id": "targets:axiom", "threshold": 2, "key_ids": package_ids.clone(), "delegated_by": "targets"}
        ],
        "policy": root_policy()
    });
    let trusted_root = root_envelope(old_signed, &old_root);
    let candidate_root = root_envelope(candidate_signed, &new_root);
    let candidate_transcript =
        metadata_transcript(ROOT_DOMAIN, &candidate_root["signed"]).expect("candidate transcript");
    let roots = json!({
        "schema_version": "axiom.package_trust_roots.v1",
        "contract": "package.trust_roots",
        "contract_status": "implemented",
        "trusted_root": trusted_root,
        "candidate_root": candidate_root,
        "transition": {
            "from_version": 1,
            "to_version": 2,
            "transition_time": "2026-09-02T00:00:00Z",
            "candidate_signatures_by_old_root": old_root
                .iter()
                .map(|signer| signer.signature_over(&candidate_transcript))
                .collect::<Vec<_>>(),
            "candidate_signatures_by_new_root": new_root
                .iter()
                .map(|signer| signer.signature_over(&candidate_transcript))
                .collect::<Vec<_>>()
        }
    });

    let contract_bytes =
        fs::read(&args.contract).map_err(|error| format!("read {}: {error}", args.contract))?;
    let contract: Value = serde_json::from_slice(&contract_bytes)
        .map_err(|error| format!("parse {}: {error}", args.contract))?;
    let mut template = contract
        .get("verification_expectation")
        .cloned()
        .ok_or_else(|| "contract has no verification_expectation".to_owned())?;
    template["contract_status"] = json!("implemented");
    template["verification_time"] = json!(FIXTURE_VERIFICATION_TIME);
    template["request"]["registry_identity"] = json!(FIXTURE_REGISTRY_IDENTITY);
    template["request"]["source_identity"] = json!(FIXTURE_SOURCE_IDENTITY);
    template["required_signers"]["index_role_id"] = json!("registry-index");
    template["required_signers"]["index_threshold"] = json!(2);
    template["required_signers"]["package_role_id"] = json!("targets:axiom");
    template["required_signers"]["package_threshold"] = json!(2);
    template["required_signers"]["required_key_ids"] = json!(package_ids);
    template["trusted_state"]["trusted_root_anchor"] = json!({
        "root_version": 1,
        "root_sequence": 1,
        "root_transcript_sha256": roots["trusted_root"]["transcript"]["sha256"]
    });
    template["trusted_state"]["highest_root_version"] = json!(2);
    template["trusted_state"]["highest_root_sequence"] = json!(2);
    template["trusted_state"]["highest_index_generation"] = json!(1);
    template["trusted_state"]["highest_index_sequence"] = json!(1);
    template["trusted_state"]["minimum_package_version"] = json!(FIXTURE_PACKAGE_VERSION);
    template["trusted_state"]["seen_snapshots"] = json!([{
        "generation": 1,
        "sequence": 1,
        "snapshot_id": FIXTURE_SNAPSHOT_ID,
        "index_transcript_sha256": "00".repeat(32)
    }]);
    template["offline_lock"]["root_version"] =
        roots["candidate_root"]["signed"]["root_version"].clone();
    template["offline_lock"]["root_sequence"] =
        roots["candidate_root"]["signed"]["sequence"].clone();
    template["offline_lock"]["root_transcript_sha256"] =
        roots["candidate_root"]["transcript"]["sha256"].clone();

    let out = Path::new(&args.out);
    write_json(&out.join("trust-roots.json"), &roots)?;
    write_json(&out.join("expectation-template.json"), &template)?;
    for (name, signer) in [
        ("package-a", &package_signers[0]),
        ("package-b", &package_signers[1]),
        ("index-a", &index_signers[0]),
        ("index-b", &index_signers[1]),
    ] {
        let seed_path = out.join("seeds").join(format!("{name}.seed"));
        if let Some(parent) = seed_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create {}: {error}", parent.display()))?;
        }
        // 64 lowercase hex characters: one accepted `signing_seed` encoding.
        fs::write(&seed_path, hex_encode(&signer.0.to_bytes()))
            .map_err(|error| format!("write {}: {error}", seed_path.display()))?;
    }
    println!("wrote trust inputs under {}", out.display());
    Ok(())
}

fn run_prepin(args: PrepinArgs) -> Result<(), String> {
    let signature_bytes =
        fs::read(&args.signature).map_err(|error| format!("read {}: {error}", args.signature))?;
    let parsed = parse_package_signature_json(&signature_bytes)
        .map_err(|error| format!("parse published package signature: {error}"))?;
    let signature = &parsed.0;
    let canonical_signature =
        canonical_json(signature).map_err(|error| format!("canonicalize signature: {error}"))?;
    let release = json!({
        "namespace": signature["package"]["namespace"],
        "name": signature["package"]["name"],
        "version": signature["package"]["version"],
        "target_path": signature["package"]["target_path"],
        "registry_identity": signature["registry"]["registry_identity"],
        "source_identity": signature["registry"]["source_identity"],
        "publisher_identity": signature["publisher"]["publisher_identity"],
        "archive": {
            "length": signature["archive"]["size"],
            "digest": signature["archive"]["digest"]
        },
        "manifest": signature["manifest"],
        "provenance": signature["provenance"],
        "package_signature_sha256": sha256_hex(&canonical_signature),
        "yanked": false
    });
    // Reconstruct the exact v2 index transcript `registry-index` will sign for
    // these coordinates (mirrors `pin_fixture_to_candidate_index`).
    let signed = json!({
        "metadata_version": 2,
        "registry_identity": args.registry_identity,
        "source_identity": args.source_identity,
        "generation": args.generation,
        "sequence": args.sequence,
        "issued_at": args.issued_at,
        "expires_at": args.expires_at,
        "consistent_snapshot": {
            "enabled": true,
            "snapshot_id": args.snapshot_id,
            "metadata_path": args.metadata_path,
            "previous_snapshot_sha256": args.previous_snapshot_sha256
        },
        "signature_role": "registry-index",
        "releases": [release.clone()]
    });
    let transcript =
        metadata_transcript(INDEX_DOMAIN, &signed).map_err(|error| error.to_string())?;
    let index_transcript_sha256 = sha256_hex(&transcript);

    let roots_bytes =
        fs::read(&args.roots).map_err(|error| format!("read {}: {error}", args.roots))?;
    let roots: Value = serde_json::from_slice(&roots_bytes)
        .map_err(|error| format!("parse {}: {error}", args.roots))?;
    let template_bytes =
        fs::read(&args.template).map_err(|error| format!("read {}: {error}", args.template))?;
    let mut expectation: Value = serde_json::from_slice(&template_bytes)
        .map_err(|error| format!("parse {}: {error}", args.template))?;

    expectation["request"] = json!({
        "registry_identity": release["registry_identity"],
        "source_identity": release["source_identity"],
        "namespace": release["namespace"],
        "name": release["name"],
        "version": release["version"],
        "target_path": release["target_path"],
        "publisher_identity": release["publisher_identity"],
        "archive": release["archive"],
        "manifest": release["manifest"],
        "provenance": release["provenance"]
    });
    expectation["required_signers"]["required_key_ids"] = Value::Array(
        signature["signatures"]
            .as_array()
            .ok_or_else(|| "published signature has no signatures array".to_owned())?
            .iter()
            .filter_map(|entry| entry.get("key_id").cloned())
            .collect(),
    );
    expectation["trusted_state"]["highest_index_generation"] = json!(args.generation);
    expectation["trusted_state"]["highest_index_sequence"] = json!(args.sequence);
    expectation["trusted_state"]["minimum_package_version"] = release["version"].clone();
    expectation["trusted_state"]["seen_snapshots"] = json!([{
        "generation": args.generation,
        "sequence": args.sequence,
        "snapshot_id": args.snapshot_id,
        "index_transcript_sha256": index_transcript_sha256
    }]);
    expectation["offline_lock"] = json!({
        "mode": "offline_locked",
        "network_fallback": false,
        "root_version": roots["candidate_root"]["signed"]["root_version"],
        "root_sequence": roots["candidate_root"]["signed"]["sequence"],
        "root_transcript_sha256": roots["candidate_root"]["transcript"]["sha256"],
        "index_generation": args.generation,
        "index_sequence": args.sequence,
        "index_transcript_sha256": index_transcript_sha256,
        "release": {
            "registry_identity": release["registry_identity"],
            "source_identity": release["source_identity"],
            "namespace": release["namespace"],
            "name": release["name"],
            "version": release["version"],
            "target_path": release["target_path"],
            "publisher_identity": release["publisher_identity"],
            "archive": release["archive"],
            "manifest": release["manifest"],
            "provenance_statement_sha256": release["provenance"]["statement"]["digest"]["value"],
            "provenance_predicate_type": release["provenance"]["statement"]["value"]["predicateType"],
            "provenance_subject": release["provenance"]["selected_subject"],
            "package_signature_sha256": release["package_signature_sha256"]
        }
    });

    write_json(Path::new(&args.out), &expectation)?;
    // stdout: the pinned index transcript hash, for the driver's cross-check
    // against the real `registry-index` output.
    println!("{index_transcript_sha256}");
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("inputs") => {
            let rest = &args[1..];
            let parsed = parse_flag(rest, "--out").and_then(|out| {
                Ok(InputsArgs {
                    out,
                    contract: parse_flag(rest, "--contract")
                        .unwrap_or_else(|_| DEFAULT_CONTRACT_PATH.to_owned()),
                })
            });
            parsed.and_then(run_inputs)
        }
        Some("prepin") => {
            let rest = args[1..].to_vec();
            let parsed = || -> Result<PrepinArgs, String> {
                Ok(PrepinArgs {
                    template: parse_flag(&rest, "--template")?,
                    roots: parse_flag(&rest, "--roots")?,
                    signature: parse_flag(&rest, "--signature")?,
                    out: parse_flag(&rest, "--out")?,
                    registry_identity: parse_flag(&rest, "--registry-identity")?,
                    source_identity: parse_flag(&rest, "--source-identity")?,
                    generation: parse_flag(&rest, "--generation")?
                        .parse()
                        .map_err(|error| format!("--generation: {error}"))?,
                    sequence: parse_flag(&rest, "--sequence")?
                        .parse()
                        .map_err(|error| format!("--sequence: {error}"))?,
                    issued_at: parse_flag(&rest, "--issued-at")?,
                    expires_at: parse_flag(&rest, "--expires-at")?,
                    snapshot_id: parse_flag(&rest, "--snapshot-id")?,
                    metadata_path: parse_flag(&rest, "--metadata-path")?,
                    previous_snapshot_sha256: parse_flag(&rest, "--previous-snapshot-sha256")?,
                })
            };
            parsed().and_then(run_prepin)
        }
        _ => Err(usage()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("registry_quickstart_fixtures: {message}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> String {
    "usage: registry_quickstart_fixtures inputs --out DIR [--contract PATH]\n\
     \x20      registry_quickstart_fixtures prepin --template T --roots R --signature SIG \
     --out E --registry-identity RI --source-identity SI --generation G --sequence S \
     --issued-at TS --expires-at TS --snapshot-id ID --metadata-path P \
     --previous-snapshot-sha256 HEX"
        .to_owned()
}
