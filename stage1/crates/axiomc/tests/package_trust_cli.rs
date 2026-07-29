use axiomc::package_trust::{
    Ed25519Signer, INDEX_DOMAIN, ROOT_DOMAIN, TrustRootsEnvelope, VerificationExpectation,
    canonical_json, metadata_transcript, parse_package_signature_json,
};
use axiomc::registry::{
    PublishOptions, RegistryIndex, RegistryIndexOptions, build_registry_index, publish_package,
};
use ed25519_dalek::{Signer as _, SigningKey};
use jsonschema::Validator;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::process::Stdio;
use std::process::{Command, Output};
use std::sync::Mutex;

// Each case launches the full compiler with the Package Trust schema set. Keep
// child processes serialized so constrained self-hosted runners do not
// intermittently kill one while several validators initialize in parallel.
static PACKAGE_TRUST_CLI_PROCESS: Mutex<()> = Mutex::new(());

fn package_trust_cli_process_guard() -> std::sync::MutexGuard<'static, ()> {
    PACKAGE_TRUST_CLI_PROCESS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct PackageTrustFiles {
    archive: PathBuf,
    manifest: PathBuf,
    provenance: PathBuf,
    signature: PathBuf,
    trust_roots: PathBuf,
    registry_index: PathBuf,
    expectation: PathBuf,
}

impl PackageTrustFiles {
    fn args(&self) -> Vec<&str> {
        vec![
            "pkg",
            "verify",
            "--archive",
            self.archive.to_str().expect("archive path"),
            "--manifest",
            self.manifest.to_str().expect("manifest path"),
            "--provenance",
            self.provenance.to_str().expect("provenance path"),
            "--signature",
            self.signature.to_str().expect("signature path"),
            "--trust-roots",
            self.trust_roots.to_str().expect("trust roots path"),
            "--registry-index",
            self.registry_index.to_str().expect("registry index path"),
            "--expectation",
            self.expectation.to_str().expect("expectation path"),
            "--json",
        ]
    }
}

fn stage1_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn contract() -> Value {
    serde_json::from_slice(
        &fs::read(stage1_path("package-trust/contract/package-trust.json"))
            .expect("read checked Package Trust contract"),
    )
    .expect("parse checked Package Trust contract")
}

fn verification_schema() -> Validator {
    let schema: Value = serde_json::from_slice(
        &fs::read(stage1_path(
            "schemas/axiom-package-verification-v1.schema.json",
        ))
        .expect("read package verification schema"),
    )
    .expect("parse package verification schema");
    jsonschema::validator_for(&schema).expect("compile package verification schema")
}

fn write_json(path: &Path, value: &Value) {
    fs::write(
        path,
        serde_json::to_vec_pretty(value).expect("serialize checked metadata"),
    )
    .expect("write checked metadata");
}

fn checked_metadata(temp: &Path) -> PackageTrustFiles {
    let contract = contract();
    let files = PackageTrustFiles {
        archive: temp.join("package.axp"),
        manifest: temp.join("axiom.toml"),
        provenance: temp.join("statement.json"),
        signature: temp.join("package.sig.json"),
        trust_roots: temp.join("roots.json"),
        registry_index: temp.join("index.json"),
        expectation: temp.join("verification-request.json"),
    };
    write_json(&files.signature, &contract["package_signature"]);
    write_json(&files.trust_roots, &contract["trust_roots"]);
    write_json(&files.registry_index, &contract["registry_index"]);
    write_json(&files.expectation, &contract["verification_expectation"]);
    fs::write(&files.manifest, b"tampered manifest bytes\n").expect("write manifest");
    fs::write(
        &files.provenance,
        decode_hex(
            contract["package_signature"]["provenance"]["statement"]["canonical_bytes_hex"]
                .as_str()
                .expect("canonical provenance hex"),
        ),
    )
    .expect("write canonical provenance");
    files
}

fn decode_hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0, "hex fixture length");
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|digits| {
            let digits = std::str::from_utf8(digits).expect("ASCII hex");
            u8::from_str_radix(digits, 16).expect("valid fixture hex")
        })
        .collect()
}

struct TestSigner(SigningKey);

impl TestSigner {
    fn new(seed: u8) -> Self {
        Self(SigningKey::from_bytes(&[seed; 32]))
    }
}

impl Ed25519Signer for TestSigner {
    type Error = std::convert::Infallible;

    fn public_key(&self) -> Result<[u8; 32], Self::Error> {
        Ok(self.0.verifying_key().to_bytes())
    }

    fn sign(&self, message: &[u8]) -> Result<[u8; 64], Self::Error> {
        Ok(self.0.sign(message).to_bytes())
    }
}

struct TrustFixture {
    roots: TrustRootsEnvelope,
    expectation: VerificationExpectation,
    package_signers: [TestSigner; 2],
    index_signers: [TestSigner; 2],
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

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn signer_key_id(signer: &TestSigner) -> String {
    trust_key(signer, "unused")["key_id"]
        .as_str()
        .expect("signer key id")
        .to_owned()
}

fn trust_key(signer: &TestSigner, publisher: &str) -> Value {
    let material = serde_json::json!({
        "algorithm": "ed25519",
        "public_key_encoding": "lowercase-hex",
        "public_key": hex_encode(&signer.public_key().expect("public key"))
    });
    serde_json::json!({
        "key_id": format!(
            "sha256:{}",
            sha256(&canonical_json(&material).expect("canonical key material"))
        ),
        "key_material": material,
        "publisher_identity": publisher,
        "status": "active",
        "valid_from_sequence": 1,
        "supersedes_key_ids": [],
        "revocation": null
    })
}

fn metadata_signature(transcript: &[u8], signer: &TestSigner) -> Value {
    serde_json::json!({
        "key_id": signer_key_id(signer),
        "algorithm": "ed25519",
        "encoding": "lowercase-hex",
        "value": hex_encode(&signer.sign(transcript).expect("sign metadata"))
    })
}

fn root_envelope(signed: Value, signers: &[TestSigner]) -> Value {
    let transcript = metadata_transcript(ROOT_DOMAIN, &signed).expect("root transcript");
    serde_json::json!({
        "signed": signed,
        "transcript": {
            "encoding": "axiom-canonical-json-v1",
            "domain": ROOT_DOMAIN,
            "bytes_hex": hex_encode(&transcript),
            "sha256": sha256(&transcript)
        },
        "signatures": signers
            .iter()
            .map(|signer| metadata_signature(&transcript, signer))
            .collect::<Vec<_>>()
    })
}

fn trust_fixture() -> TrustFixture {
    let old_root = [TestSigner::new(1), TestSigner::new(2), TestSigner::new(9)];
    let new_root = [TestSigner::new(3), TestSigner::new(4)];
    let index_signers = [TestSigner::new(5), TestSigner::new(6)];
    let package_signers = [TestSigner::new(7), TestSigner::new(8)];
    let old_root_ids = old_root.iter().map(signer_key_id).collect::<Vec<_>>();
    let new_root_ids = new_root.iter().map(signer_key_id).collect::<Vec<_>>();
    let index_ids = index_signers.iter().map(signer_key_id).collect::<Vec<_>>();
    let package_ids = package_signers
        .iter()
        .map(signer_key_id)
        .collect::<Vec<_>>();
    let old_signed = serde_json::json!({
        "specification": "axiom-package-trust-root-v1",
        "root_version": 1,
        "sequence": 1,
        "issued_at": "2026-01-01T00:00:00Z",
        "expires_at": "2027-01-01T00:00:00Z",
        "consistent_snapshot": true,
        "keys": old_root
            .iter()
            .map(|signer| trust_key(signer, "axiom://trust/root"))
            .collect::<Vec<_>>(),
        "publisher_identities": [{
            "publisher_identity": "axiom://trust/root",
            "display_name": "Old root"
        }],
        "namespace_grants": [{
            "publisher_identity": "axiom://trust/root",
            "namespace": "bootstrap",
            "package_names": ["bootstrap"],
            "registry_identities": ["registry:test"],
            "source_identities": ["registry:test-source"],
            "role_id": "registry-index"
        }],
        "roles": [
            {"role_id":"root","threshold":2,"key_ids":old_root_ids.clone(),"delegated_by":null},
            {"role_id":"timestamp","threshold":2,"key_ids":old_root_ids.clone(),"delegated_by":"root"},
            {"role_id":"snapshot","threshold":2,"key_ids":old_root_ids.clone(),"delegated_by":"timestamp"},
            {"role_id":"registry-index","threshold":2,"key_ids":old_root_ids,"delegated_by":"snapshot"}
        ],
        "policy": {
            "rollback_protection": "reject rollback",
            "freeze_protection": "reject expiry",
            "downgrade_protection": "reject downgrade",
            "offline_locked": "require exact pins",
            "metadata_expiry_required": true,
            "registry_index_equivalence": "combined metadata role"
        }
    });
    let candidate_signed = serde_json::json!({
        "specification": "axiom-package-trust-root-v1",
        "root_version": 2,
        "sequence": 2,
        "issued_at": "2026-07-01T00:00:00Z",
        "expires_at": "2027-01-01T00:00:00Z",
        "consistent_snapshot": true,
        "keys": new_root.iter().map(|signer| trust_key(signer, "axiom://trust/root"))
            .chain(index_signers.iter().map(|signer| trust_key(signer, "axiom://registry/official")))
            .chain(package_signers.iter().map(|signer| trust_key(signer, "publisher:foundation")))
            .collect::<Vec<_>>(),
        "publisher_identities": [{
            "publisher_identity": "publisher:foundation",
            "display_name": "Test publisher"
        }],
        "namespace_grants": [{
            "publisher_identity": "publisher:foundation",
            "namespace": "axiom",
            "package_names": ["core"],
            "registry_identities": ["registry:test"],
            "source_identities": ["registry:test-source"],
            "role_id": "targets:axiom"
        }],
        "roles": [
            {"role_id":"root","threshold":2,"key_ids":new_root_ids,"delegated_by":null},
            {"role_id":"timestamp","threshold":2,"key_ids":index_ids,"delegated_by":"root"},
            {"role_id":"snapshot","threshold":2,"key_ids":index_ids,"delegated_by":"timestamp"},
            {"role_id":"registry-index","threshold":2,"key_ids":index_ids,"delegated_by":"snapshot"},
            {"role_id":"targets","threshold":2,"key_ids":package_ids.clone(),"delegated_by":"root"},
            {"role_id":"targets:axiom","threshold":2,"key_ids":package_ids.clone(),"delegated_by":"targets"}
        ],
        "policy": {
            "rollback_protection": "reject rollback",
            "freeze_protection": "reject expiry",
            "downgrade_protection": "reject downgrade",
            "offline_locked": "require exact pins",
            "metadata_expiry_required": true,
            "registry_index_equivalence": "combined metadata role"
        }
    });
    let trusted_root = root_envelope(old_signed, &old_root);
    let candidate_root = root_envelope(candidate_signed, &new_root);
    let candidate_transcript =
        metadata_transcript(ROOT_DOMAIN, &candidate_root["signed"]).expect("candidate transcript");
    let roots = TrustRootsEnvelope(serde_json::json!({
        "schema_version": "axiom.package_trust_roots.v1",
        "contract": "package.trust_roots",
        "contract_status": "implemented",
        "trusted_root": trusted_root,
        "candidate_root": candidate_root,
        "transition": {
            "from_version": 1,
            "to_version": 2,
            "transition_time": "2026-07-02T00:00:00Z",
            "candidate_signatures_by_old_root": old_root
                .iter()
                .map(|signer| metadata_signature(&candidate_transcript, signer))
                .collect::<Vec<_>>(),
            "candidate_signatures_by_new_root": new_root
                .iter()
                .map(|signer| metadata_signature(&candidate_transcript, signer))
                .collect::<Vec<_>>()
        }
    }));
    let mut expectation = VerificationExpectation(contract()["verification_expectation"].clone());
    expectation.0["contract_status"] = serde_json::json!("implemented");
    expectation.0["verification_time"] = serde_json::json!("2026-07-29T12:00:00Z");
    expectation.0["required_signers"]["index_role_id"] = serde_json::json!("registry-index");
    expectation.0["required_signers"]["index_threshold"] = serde_json::json!(2);
    expectation.0["required_signers"]["package_role_id"] = serde_json::json!("targets:axiom");
    expectation.0["required_signers"]["package_threshold"] = serde_json::json!(2);
    expectation.0["required_signers"]["required_key_ids"] = serde_json::json!(package_ids);
    expectation.0["trusted_state"]["trusted_root_anchor"] = serde_json::json!({
        "root_version": 1,
        "root_sequence": 1,
        "root_transcript_sha256": roots["trusted_root"]["transcript"]["sha256"]
    });
    expectation.0["trusted_state"]["highest_root_version"] = serde_json::json!(2);
    expectation.0["trusted_state"]["highest_root_sequence"] = serde_json::json!(2);
    expectation.0["trusted_state"]["highest_index_generation"] = serde_json::json!(1);
    expectation.0["trusted_state"]["highest_index_sequence"] = serde_json::json!(1);
    expectation.0["trusted_state"]["minimum_package_version"] = serde_json::json!("1.2.3");
    expectation.0["trusted_state"]["seen_snapshots"] = serde_json::json!([{
        "generation": 1,
        "sequence": 1,
        "snapshot_id": "registry.test.publication-bootstrap",
        "index_transcript_sha256": "00".repeat(32)
    }]);
    expectation.0["offline_lock"]["root_version"] =
        roots["candidate_root"]["signed"]["root_version"].clone();
    expectation.0["offline_lock"]["root_sequence"] =
        roots["candidate_root"]["signed"]["sequence"].clone();
    expectation.0["offline_lock"]["root_transcript_sha256"] =
        roots["candidate_root"]["transcript"]["sha256"].clone();
    TrustFixture {
        roots,
        expectation,
        package_signers,
        index_signers,
    }
}

fn provenance_statement(target: &str, archive_hash: &str) -> Value {
    serde_json::json!({
        "_type": "https://in-toto.io/Statement/v1",
        "subject": [{"name": target, "digest": {"sha256": archive_hash}}],
        "predicateType": "https://slsa.dev/provenance/v1",
        "predicate": {
            "buildDefinition": {
                "buildType": "axiom:build/package-v1",
                "externalParameters": {},
                "internalParameters": {},
                "resolvedDependencies": [{
                    "uri": "registry:test-source",
                    "digest": {"sha256": "11".repeat(32)}
                }]
            },
            "runDetails": {
                "builder": {
                    "id": "axiom:builder/test",
                    "builderDependencies": [],
                    "version": {"axiomc": "test"}
                },
                "metadata": {
                    "invocationId": "urn:uuid:00000000-0000-4000-8000-000000000001",
                    "startedOn": "2026-07-29T10:00:00Z",
                    "finishedOn": "2026-07-29T10:00:01Z"
                },
                "byproducts": []
            }
        }
    })
}

fn pin_fixture_to_candidate_index(fixture: &mut TrustFixture, signature_path: &Path) {
    let signature_bytes = fs::read(signature_path).expect("read published package signature");
    let signature =
        parse_package_signature_json(&signature_bytes).expect("parse published package signature");
    let canonical_signature =
        canonical_json(&signature).expect("canonicalize published package signature");
    let release = serde_json::json!({
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
        "package_signature_sha256": sha256(&canonical_signature),
        "yanked": false
    });
    let signed = serde_json::json!({
        "metadata_version": 2,
        "registry_identity": "registry:test",
        "source_identity": "registry:test-source",
        "generation": 1,
        "sequence": 1,
        "issued_at": "2026-07-29T11:00:00Z",
        "expires_at": "2026-12-31T00:00:00Z",
        "consistent_snapshot": {
            "enabled": true,
            "snapshot_id": "registry.test.1.1",
            "metadata_path": "1/1/index.v2.json",
            "previous_snapshot_sha256": "00".repeat(32)
        },
        "signature_role": "registry-index",
        "releases": [release.clone()]
    });
    let transcript = metadata_transcript(INDEX_DOMAIN, &signed).expect("index transcript");
    let index_hash = sha256(&transcript);
    fixture.expectation.0["trusted_state"]["seen_snapshots"] = serde_json::json!([{
        "generation": 1,
        "sequence": 1,
        "snapshot_id": "registry.test.1.1",
        "index_transcript_sha256": index_hash
    }]);
    fixture.expectation.0["offline_lock"] = serde_json::json!({
        "mode": "offline_locked",
        "network_fallback": false,
        "root_version": fixture.roots["candidate_root"]["signed"]["root_version"],
        "root_sequence": fixture.roots["candidate_root"]["signed"]["sequence"],
        "root_transcript_sha256": fixture.roots["candidate_root"]["transcript"]["sha256"],
        "index_generation": 1,
        "index_sequence": 1,
        "index_transcript_sha256": index_hash,
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
}

fn exact_expectation(
    template: &VerificationExpectation,
    roots: &TrustRootsEnvelope,
    index: &RegistryIndex,
    release: &Value,
    package_signature: &Value,
) -> VerificationExpectation {
    let mut value = template.0.clone();
    value["request"] = serde_json::json!({
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
    let generation = index.envelope()["signed"]["generation"].clone();
    let sequence = index.envelope()["signed"]["sequence"].clone();
    let index_hash = index.envelope()["transcript"]["sha256"].clone();
    value["trusted_state"]["highest_index_generation"] = generation.clone();
    value["trusted_state"]["highest_index_sequence"] = sequence.clone();
    value["trusted_state"]["minimum_package_version"] = release["version"].clone();
    value["trusted_state"]["seen_snapshots"] = serde_json::json!([{
        "generation": generation,
        "sequence": sequence,
        "snapshot_id": index.envelope()["signed"]["consistent_snapshot"]["snapshot_id"],
        "index_transcript_sha256": index_hash
    }]);
    value["offline_lock"]["index_generation"] = index.envelope()["signed"]["generation"].clone();
    value["offline_lock"]["index_sequence"] = index.envelope()["signed"]["sequence"].clone();
    value["offline_lock"]["index_transcript_sha256"] =
        index.envelope()["transcript"]["sha256"].clone();
    value["offline_lock"]["network_fallback"] = Value::Bool(false);
    value["offline_lock"]["release"] = serde_json::json!({
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
    });
    value["offline_lock"]["root_version"] =
        roots["candidate_root"]["signed"]["root_version"].clone();
    value["offline_lock"]["root_sequence"] = roots["candidate_root"]["signed"]["sequence"].clone();
    value["offline_lock"]["root_transcript_sha256"] =
        roots["candidate_root"]["transcript"]["sha256"].clone();
    value["required_signers"]["required_key_ids"] = Value::Array(
        package_signature["signatures"]
            .as_array()
            .expect("package signatures")
            .iter()
            .filter_map(|signature| signature.get("key_id").cloned())
            .collect(),
    );
    VerificationExpectation(value)
}

const TEST_MANIFEST: &str = "[package]\nname = \"core\"\nversion = \"1.2.3\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n";
const TEST_LOCK: &str =
    "version = 1\n\n[[package]]\nname = \"core\"\nversion = \"1.2.3\"\nsource = \"path\"\n";
const TEST_SOURCE: &str = "print \"hello\"\n";

fn render_test_archive() -> Vec<u8> {
    let mut archive = b"AXIOM_PACKAGE_ARCHIVE_V1\n".to_vec();
    for (path, content) in [
        ("axiom.lock", TEST_LOCK.as_bytes()),
        ("axiom.toml", TEST_MANIFEST.as_bytes()),
        ("src/main.ax", TEST_SOURCE.as_bytes()),
    ] {
        archive.extend_from_slice(format!("--- file {path} {} ---\n", content.len()).as_bytes());
        archive.extend_from_slice(content);
        if !content.ends_with(b"\n") {
            archive.push(b'\n');
        }
    }
    archive
}

fn run(files: &PackageTrustFiles) -> Output {
    let _guard = package_trust_cli_process_guard();
    Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(files.args())
        .output()
        .expect("run axiomc pkg verify --json")
}

fn assert_single_result(output: &Output, exit_code: i32, decision: &str) -> Value {
    assert_eq!(
        output.status.code(),
        Some(exit_code),
        "{decision} package exits {exit_code}; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut documents = serde_json::Deserializer::from_slice(&output.stdout).into_iter::<Value>();
    let result = documents
        .next()
        .expect("one verification result")
        .expect("verification result is JSON");
    assert!(
        documents.next().is_none(),
        "stdout must contain exactly one JSON document"
    );
    verification_schema()
        .validate(&result)
        .expect("CLI result satisfies axiom.package_verification.v1");
    assert_eq!(result["schema_version"], "axiom.package_verification.v1");
    assert_eq!(result["contract"], "package.verification");
    assert_eq!(result["contract_status"], "implemented");
    assert_eq!(result["decision"], decision);
    result
}

fn assert_single_rejected_result(output: &Output) -> Value {
    assert_single_result(output, 1, "rejected")
}

#[test]
fn deterministic_signed_package_returns_one_trusted_result() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let project = temp.path().join("project");
    fs::create_dir_all(project.join("src")).expect("create project source directory");
    fs::write(project.join("axiom.toml"), TEST_MANIFEST).expect("write manifest");
    fs::write(project.join("axiom.lock"), TEST_LOCK).expect("write lockfile");
    fs::write(project.join("src/main.ax"), TEST_SOURCE).expect("write source");

    let mut fixture = trust_fixture();
    let archive = render_test_archive();
    let statement = provenance_statement("axiom/core/1.2.3/package.axp", &sha256(&archive));
    let registry = temp.path().join("registry");
    let published = publish_package(
        &project,
        &registry,
        &PublishOptions {
            allow_overwrite: false,
            namespace: "axiom",
            registry_identity: "registry:test",
            source_identity: "registry:test-source",
            publisher_identity: "publisher:foundation",
            index_generation: 1,
            index_sequence: 1,
            provenance_statement: &statement,
            trust_roots: &fixture.roots,
            verification_expectation: &fixture.expectation,
            signers: &fixture.package_signers,
        },
    )
    .expect("publish signed package");
    pin_fixture_to_candidate_index(&mut fixture, Path::new(&published.signature));
    let package_signature_bytes =
        fs::read(&published.signature).expect("read published package signature");
    let package_signature =
        parse_package_signature_json(&package_signature_bytes).expect("parse package signature");
    let previous_snapshot = "00".repeat(32);
    let index = build_registry_index(
        &registry,
        &RegistryIndexOptions {
            registry_identity: "registry:test",
            source_identity: "registry:test-source",
            generation: 1,
            sequence: 1,
            issued_at: "2026-07-29T11:00:00Z",
            expires_at: "2026-12-31T00:00:00Z",
            snapshot_id: "registry.test.1.1",
            metadata_path: "1/1/index.v2.json",
            previous_snapshot_sha256: &previous_snapshot,
            trust_roots: &fixture.roots,
            verification_expectation: &fixture.expectation,
            signers: &fixture.index_signers,
        },
    )
    .expect("build signed registry index");
    let release = &index.envelope()["signed"]["releases"][0];
    let expectation = exact_expectation(
        &fixture.expectation,
        &fixture.roots,
        &index,
        release,
        &package_signature.0,
    );
    let files = PackageTrustFiles {
        archive: PathBuf::from(&published.archive),
        manifest: PathBuf::from(&published.manifest),
        provenance: PathBuf::from(&published.provenance),
        signature: PathBuf::from(&published.signature),
        trust_roots: temp.path().join("roots.json"),
        registry_index: temp.path().join("index.json"),
        expectation: temp.path().join("verification-request.json"),
    };
    write_json(&files.trust_roots, &fixture.roots.0);
    fs::write(&files.registry_index, index.as_bytes()).expect("write registry index");
    write_json(&files.expectation, &expectation.0);

    let output = run(&files);
    let result = assert_single_result(&output, 0, "trusted");

    assert_eq!(result["primary_reason_code"], "OK");
    assert_eq!(result["reason_codes"], serde_json::json!(["OK"]));
    assert!(
        output.stderr.is_empty(),
        "trusted verification should not emit diagnostics: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn checked_metadata_with_missing_archive_returns_one_rejected_result() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let files = checked_metadata(temp.path());

    let output = run(&files);
    let result = assert_single_rejected_result(&output);

    assert_eq!(result["primary_reason_code"], "OFFLINE_INPUT_MISSING");
    assert_eq!(
        result["reason_codes"],
        serde_json::json!(["OFFLINE_INPUT_MISSING"])
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("failed to open package archive"),
        "missing input should have an explicit bounded-read error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn checked_metadata_with_tampered_artifacts_returns_digest_rejection() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let files = checked_metadata(temp.path());
    fs::write(&files.archive, b"tampered archive bytes").expect("write tampered archive");

    let output = run(&files);
    let result = assert_single_rejected_result(&output);

    let reasons = result["reason_codes"]
        .as_array()
        .expect("reason codes array");
    assert!(
        reasons.contains(&serde_json::json!("ARCHIVE_DIGEST_MISMATCH")),
        "{reasons:?}"
    );
    assert!(
        reasons.contains(&serde_json::json!("MANIFEST_DIGEST_MISMATCH")),
        "{reasons:?}"
    );
    assert!(
        !reasons.contains(&serde_json::json!("PROVENANCE_STATEMENT_MISMATCH")),
        "canonical provenance bytes should remain intact: {reasons:?}"
    );
    assert!(
        output.stderr.is_empty(),
        "verification rejection is the result, not an operational error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_malformed_document_rejection(
    select_path: impl FnOnce(&PackageTrustFiles) -> &Path,
    malformed: &[u8],
    expected_reason: &str,
    expected_error: &str,
) {
    let temp = tempfile::tempdir().expect("create tempdir");
    let files = checked_metadata(temp.path());
    fs::write(&files.archive, b"tampered archive bytes").expect("write archive");
    fs::write(select_path(&files), malformed).expect("write malformed metadata");

    let output = run(&files);
    let result = assert_single_rejected_result(&output);

    assert_eq!(result["primary_reason_code"], expected_reason);
    assert_eq!(result["reason_codes"], serde_json::json!([expected_reason]));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(expected_error),
        "strict parser error should be explicit: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn malformed_signature_maps_to_signature_malformed() {
    assert_malformed_document_rejection(
        |files| &files.signature,
        br#"{"contract":"package.signature","contract":"duplicate"}"#,
        "SIGNATURE_MALFORMED",
        "invalid package signature",
    );
}

#[test]
fn malformed_trust_roots_maps_to_root_digest_mismatch() {
    assert_malformed_document_rejection(
        |files| &files.trust_roots,
        br#"{"contract":"package.trust_roots","contract":"duplicate"}"#,
        "ROOT_DIGEST_MISMATCH",
        "invalid trust roots",
    );
}

#[test]
fn malformed_registry_index_maps_to_index_digest_mismatch() {
    assert_malformed_document_rejection(
        |files| &files.registry_index,
        br#"{"contract":"package.registry_index","contract":"duplicate"}"#,
        "INDEX_DIGEST_MISMATCH",
        "invalid registry index",
    );
}

#[test]
fn malformed_expectation_maps_to_offline_input_missing() {
    assert_malformed_document_rejection(
        |files| &files.expectation,
        br#"{"contract":"package.verification_expectation","contract":"duplicate"}"#,
        "OFFLINE_INPUT_MISSING",
        "invalid verification expectation",
    );
}

#[test]
fn oversized_metadata_maps_to_offline_input_missing() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let files = checked_metadata(temp.path());
    let signature = File::create(&files.signature).expect("create sparse signature file");
    signature
        .set_len(9 * 1024 * 1024)
        .expect("size sparse signature file");

    let output = run(&files);
    let result = assert_single_rejected_result(&output);

    assert_eq!(result["primary_reason_code"], "OFFLINE_INPUT_MISSING");
    assert_eq!(
        result["reason_codes"],
        serde_json::json!(["OFFLINE_INPUT_MISSING"])
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("exceeds the 8388608-byte limit"),
        "oversized bounded-read error should be explicit: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(target_os = "linux")]
#[test]
fn stdout_write_failure_exits_two() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let files = checked_metadata(temp.path());
    let full = fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .expect("open /dev/full");
    let _guard = package_trust_cli_process_guard();

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(files.args())
        .stdout(Stdio::from(full))
        .stderr(Stdio::piped())
        .output()
        .expect("run axiomc pkg verify with failing stdout");

    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout I/O failure is operational; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("failed to write package verification result"),
        "stdout I/O failure should be explicit: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
