use axiomc::lockfile::{ParsedLockfile, load_lockfile, write_lockfile_v2_atomic};
use axiomc::package_trust::{
    Ed25519Signer, INDEX_DOMAIN, ROOT_DOMAIN, TrustRootsEnvelope, VerificationExpectation,
    canonical_json, metadata_transcript, parse_package_signature_json,
};
use axiomc::registry::{
    PublishOptions, RegistryIndex, RegistryIndexOptions, build_registry_index, publish_package,
};
use ed25519_dalek::{Signer as _, SigningKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

const REGISTRY_IDENTITY: &str = "registry:test";
const SOURCE_IDENTITY: &str = "registry:test-source";
const PUBLISHER_IDENTITY: &str = "publisher:foundation";
const NAMESPACE: &str = "axiom";

// These cases launch complete compiler processes and generate Ed25519-backed
// registry metadata. Serialize them so constrained runners do not run several
// full package graphs at once.
static PACKAGE_RESOLVER_CLI_PROCESS: Mutex<()> = Mutex::new(());

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
    expectation_template: VerificationExpectation,
    package_signers: [TestSigner; 2],
    index_signers: [TestSigner; 2],
}

struct RegistryVersion {
    index: RegistryIndex,
    expectation: VerificationExpectation,
}

struct HttpRegistry {
    address: SocketAddr,
    routes: Arc<RwLock<BTreeMap<String, Vec<u8>>>>,
    requests: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl HttpRegistry {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| {
            panic!(
                "numeric-loopback HTTP is required for package resolver acceptance coverage; \
                 failed to bind 127.0.0.1:0: {error}"
            )
        });
        listener
            .set_nonblocking(true)
            .expect("set package registry nonblocking");
        let address = listener.local_addr().expect("package registry address");
        let routes = Arc::new(RwLock::new(BTreeMap::new()));
        let requests = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_routes = Arc::clone(&routes);
        let worker_requests = Arc::clone(&requests);
        let worker_stop = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        worker_requests.fetch_add(1, Ordering::AcqRel);
                        serve_http_request(&mut stream, &worker_routes);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("accept package registry request: {error}"),
                }
            }
        });
        Self {
            address,
            routes,
            requests,
            stop,
            worker: Some(worker),
        }
    }

    fn index_url(&self) -> String {
        format!("http://{}/index.json", self.address)
    }

    fn install(&self, registry_root: &Path, index: &RegistryIndex) {
        let mut routes = BTreeMap::from([("/index.json".to_owned(), index.as_bytes().to_vec())]);
        for release in index.envelope()["signed"]["releases"]
            .as_array()
            .expect("registry index releases")
        {
            let target = release["target_path"]
                .as_str()
                .expect("release target path");
            let parent = Path::new(target).parent().expect("release parent");
            for relative in [
                PathBuf::from(target),
                parent.join("axiom.toml"),
                parent.join("provenance.json"),
                parent.join("package.axp.sig"),
            ] {
                let route = format!("/{}", relative.to_string_lossy());
                let bytes = fs::read(registry_root.join(&relative))
                    .unwrap_or_else(|error| panic!("read exact route {route}: {error}"));
                routes.insert(route, bytes);
            }
        }
        *self.routes.write().expect("write package registry routes") = routes;
    }

    fn request_count(&self) -> usize {
        self.requests.load(Ordering::Acquire)
    }

    fn replace_route(&self, route: &str, bytes: &[u8]) {
        let replaced = self
            .routes
            .write()
            .expect("write package registry routes")
            .insert(route.to_owned(), bytes.to_vec());
        assert!(replaced.is_some(), "route {route:?} must already exist");
    }
}

impl Drop for HttpRegistry {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.join().expect("join package registry");
        }
    }
}

fn serve_http_request(stream: &mut TcpStream, routes: &Arc<RwLock<BTreeMap<String, Vec<u8>>>>) {
    stream
        .set_read_timeout(Some(Duration::from_millis(250)))
        .expect("set registry read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .expect("set registry write timeout");
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    let request_deadline = Instant::now() + Duration::from_secs(5);
    while request.len() < 8192 && !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = match stream.read(&mut buffer) {
            Ok(read) => read,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                assert!(
                    Instant::now() < request_deadline,
                    "timed out waiting for complete registry request"
                );
                continue;
            }
            Err(error) => panic!("read registry request: {error}"),
        };
        if read == 0 {
            return;
        }
        request.extend_from_slice(&buffer[..read]);
    }
    let request = String::from_utf8(request).expect("registry request is ASCII");
    let mut words = request
        .lines()
        .next()
        .expect("registry request line")
        .split_ascii_whitespace();
    let method = words.next().unwrap_or_default();
    let path = words.next().unwrap_or_default();
    assert_eq!(method, "GET", "registry transport uses only GET");
    let route = routes
        .read()
        .expect("read package registry routes")
        .get(path)
        .cloned();
    let (status, body) = match route {
        Some(body) => ("200 OK", body),
        None => ("404 Not Found", b"not found\n".to_vec()),
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .expect("write registry response head");
    stream
        .write_all(&body)
        .expect("write exact registry response body");
    stream.flush().expect("flush exact registry response");
    stream
        .shutdown(Shutdown::Write)
        .expect("finish exact registry response");
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
    let material = json!({
        "algorithm": "ed25519",
        "public_key_encoding": "lowercase-hex",
        "public_key": hex_encode(&signer.public_key().expect("public key"))
    });
    json!({
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
    json!({
        "key_id": signer_key_id(signer),
        "algorithm": "ed25519",
        "encoding": "lowercase-hex",
        "value": hex_encode(&signer.sign(transcript).expect("sign metadata"))
    })
}

fn root_envelope(signed: Value, signers: &[TestSigner]) -> Value {
    let transcript = metadata_transcript(ROOT_DOMAIN, &signed).expect("root transcript");
    json!({
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

fn stage1_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
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
    let old_signed = json!({
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
            "registry_identities": [REGISTRY_IDENTITY],
            "source_identities": [SOURCE_IDENTITY],
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
    let candidate_signed = json!({
        "specification": "axiom-package-trust-root-v1",
        "root_version": 2,
        "sequence": 2,
        "issued_at": "2026-07-01T00:00:00Z",
        "expires_at": "2027-01-01T00:00:00Z",
        "consistent_snapshot": true,
        "keys": new_root.iter().map(|signer| trust_key(signer, "axiom://trust/root"))
            .chain(index_signers.iter().map(|signer| trust_key(signer, "axiom://registry/official")))
            .chain(package_signers.iter().map(|signer| trust_key(signer, PUBLISHER_IDENTITY)))
            .collect::<Vec<_>>(),
        "publisher_identities": [{
            "publisher_identity": PUBLISHER_IDENTITY,
            "display_name": "Test publisher"
        }],
        "namespace_grants": [{
            "publisher_identity": PUBLISHER_IDENTITY,
            "namespace": NAMESPACE,
            "package_names": ["core", "support"],
            "registry_identities": [REGISTRY_IDENTITY],
            "source_identities": [SOURCE_IDENTITY],
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
    let roots = TrustRootsEnvelope(json!({
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
    let contract: Value = serde_json::from_slice(
        &fs::read(stage1_path("package-trust/contract/package-trust.json"))
            .expect("read checked Package Trust contract"),
    )
    .expect("parse checked Package Trust contract");
    let mut expectation = VerificationExpectation(contract["verification_expectation"].clone());
    expectation.0["contract_status"] = json!("implemented");
    expectation.0["verification_time"] = json!("2026-07-29T12:00:00Z");
    expectation.0["required_signers"]["index_role_id"] = json!("registry-index");
    expectation.0["required_signers"]["index_threshold"] = json!(2);
    expectation.0["required_signers"]["package_role_id"] = json!("targets:axiom");
    expectation.0["required_signers"]["package_threshold"] = json!(2);
    expectation.0["required_signers"]["required_key_ids"] = json!(package_ids);
    expectation.0["trusted_state"]["trusted_root_anchor"] = json!({
        "root_version": 1,
        "root_sequence": 1,
        "root_transcript_sha256": roots["trusted_root"]["transcript"]["sha256"]
    });
    expectation.0["trusted_state"]["highest_root_version"] = json!(2);
    expectation.0["trusted_state"]["highest_root_sequence"] = json!(2);
    expectation.0["trusted_state"]["highest_index_generation"] = json!(1);
    expectation.0["trusted_state"]["highest_index_sequence"] = json!(1);
    expectation.0["trusted_state"]["minimum_package_version"] = json!("1.2.3");
    expectation.0["trusted_state"]["seen_snapshots"] = json!([{
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
        expectation_template: expectation,
        package_signers,
        index_signers,
    }
}

fn provenance_statement(target: &str, archive_hash: &str) -> Value {
    json!({
        "_type": "https://in-toto.io/Statement/v1",
        "subject": [{"name": target, "digest": {"sha256": archive_hash}}],
        "predicateType": "https://slsa.dev/provenance/v1",
        "predicate": {
            "buildDefinition": {
                "buildType": "axiom:build/package-v1",
                "externalParameters": {},
                "internalParameters": {},
                "resolvedDependencies": [{
                    "uri": SOURCE_IDENTITY,
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

fn package_manifest(name: &str, version: &str) -> String {
    format!(
        "[package]\nname = {name:?}\nversion = {version:?}\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n"
    )
}

fn package_lock(name: &str, version: &str) -> String {
    format!(
        "version = 1\n\n[[package]]\nname = {name:?}\nversion = {version:?}\nsource = \"path\"\n"
    )
}

fn render_archive(manifest: &[u8], lock: &[u8], source: &[u8]) -> Vec<u8> {
    let mut archive = b"AXIOM_PACKAGE_ARCHIVE_V1\n".to_vec();
    for (path, content) in [
        ("axiom.lock", lock),
        ("axiom.toml", manifest),
        ("src/main.ax", source),
    ] {
        archive.extend_from_slice(format!("--- file {path} {} ---\n", content.len()).as_bytes());
        archive.extend_from_slice(content);
        if !content.ends_with(b"\n") {
            archive.push(b'\n');
        }
    }
    archive
}

fn publish_fixture_package(
    root: &Path,
    registry_root: &Path,
    fixture: &TrustFixture,
    name: &str,
    version: &str,
    index_sequence: u64,
    allow_overwrite: bool,
    source_marker: &str,
) {
    let project = root.join(format!("{name}-{version}-{index_sequence}-{source_marker}"));
    fs::create_dir_all(project.join("src")).expect("create package source");
    let manifest = package_manifest(name, version);
    let lock = package_lock(name, version);
    let source = format!("print \"{name} {version} {source_marker}\"\n");
    fs::write(project.join("axiom.toml"), &manifest).expect("write package manifest");
    fs::write(project.join("axiom.lock"), &lock).expect("write package lock");
    fs::write(project.join("src/main.ax"), &source).expect("write package source");
    let archive = render_archive(manifest.as_bytes(), lock.as_bytes(), source.as_bytes());
    let target = format!("{NAMESPACE}/{name}/{version}/package.axp");
    let statement = provenance_statement(&target, &sha256(&archive));
    let published = publish_package(
        &project,
        registry_root,
        &PublishOptions {
            allow_overwrite,
            namespace: NAMESPACE,
            registry_identity: REGISTRY_IDENTITY,
            source_identity: SOURCE_IDENTITY,
            publisher_identity: PUBLISHER_IDENTITY,
            index_generation: 1,
            index_sequence,
            provenance_statement: &statement,
            trust_roots: &fixture.roots,
            verification_expectation: &fixture.expectation_template,
            signers: &fixture.package_signers,
        },
    )
    .expect("publish runtime-generated signed package");
    assert_eq!(
        fs::read(&published.archive).expect("read published archive"),
        archive,
        "the registry must serve the exact archive bytes generated at runtime"
    );
}

fn publish_core(
    root: &Path,
    registry_root: &Path,
    fixture: &TrustFixture,
    version: &str,
    index_sequence: u64,
) {
    publish_fixture_package(
        root,
        registry_root,
        fixture,
        "core",
        version,
        index_sequence,
        false,
        "original",
    );
}

fn release_values(registry_root: &Path) -> Vec<Value> {
    let mut signature_paths = Vec::new();
    collect_named_files(registry_root, "package.axp.sig", &mut signature_paths);
    let mut releases = signature_paths
        .into_iter()
        .map(|signature_path| {
            let signature_bytes =
                fs::read(&signature_path).expect("read published package signature");
            let signature = parse_package_signature_json(&signature_bytes)
                .expect("parse published package signature");
            let canonical_signature =
                canonical_json(&signature).expect("canonicalize published package signature");
            let target = signature["package"]["target_path"]
                .as_str()
                .expect("signature target");
            let release_dir =
                registry_root.join(Path::new(target).parent().expect("release parent"));
            json!({
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
                "yanked": release_dir.join("axiom-registry.toml").exists()
            })
        })
        .collect::<Vec<_>>();
    releases.sort_by(|left, right| {
        left["namespace"]
            .as_str()
            .cmp(&right["namespace"].as_str())
            .then(left["name"].as_str().cmp(&right["name"].as_str()))
            .then(left["version"].as_str().cmp(&right["version"].as_str()))
    });
    releases
}

fn collect_named_files(root: &Path, name: &str, output: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("read fixture directory entry").path();
        if path.is_dir() {
            collect_named_files(&path, name, output);
        } else if path.file_name().and_then(|value| value.to_str()) == Some(name) {
            output.push(path);
        }
    }
}

fn expectation_for_index(
    fixture: &TrustFixture,
    releases: &[Value],
    generation: u64,
    sequence: u64,
    snapshot_id: &str,
    previous_snapshot_sha256: &str,
    trusted_index: Option<&RegistryIndex>,
) -> VerificationExpectation {
    let signed = json!({
        "metadata_version": 2,
        "registry_identity": REGISTRY_IDENTITY,
        "source_identity": SOURCE_IDENTITY,
        "generation": generation,
        "sequence": sequence,
        "issued_at": "2026-07-29T11:00:00Z",
        "expires_at": "2026-12-31T00:00:00Z",
        "consistent_snapshot": {
            "enabled": true,
            "snapshot_id": snapshot_id,
            "metadata_path": format!("{generation}/{sequence}/index.v2.json"),
            "previous_snapshot_sha256": previous_snapshot_sha256
        },
        "signature_role": "registry-index",
        "releases": releases
    });
    let transcript =
        metadata_transcript(INDEX_DOMAIN, &signed).expect("prospective index transcript");
    let transcript_sha256 = sha256(&transcript);
    let release = releases.first().expect("at least one registry release");
    let mut expectation = fixture.expectation_template.0.clone();
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
    if let Some(trusted) = trusted_index {
        expectation["trusted_state"]["highest_index_generation"] =
            trusted.envelope()["signed"]["generation"].clone();
        expectation["trusted_state"]["highest_index_sequence"] =
            trusted.envelope()["signed"]["sequence"].clone();
        expectation["trusted_state"]["seen_snapshots"] = json!([{
            "generation": trusted.envelope()["signed"]["generation"],
            "sequence": trusted.envelope()["signed"]["sequence"],
            "snapshot_id": trusted.envelope()["signed"]["consistent_snapshot"]["snapshot_id"],
            "index_transcript_sha256": trusted.envelope()["transcript"]["sha256"]
        }]);
    } else {
        expectation["trusted_state"]["highest_index_generation"] = json!(generation);
        expectation["trusted_state"]["highest_index_sequence"] = json!(sequence);
        expectation["trusted_state"]["seen_snapshots"] = json!([{
            "generation": generation,
            "sequence": sequence,
            "snapshot_id": snapshot_id,
            "index_transcript_sha256": transcript_sha256
        }]);
    }
    expectation["offline_lock"] = json!({
        "mode": "offline_locked",
        "network_fallback": false,
        "root_version": fixture.roots["candidate_root"]["signed"]["root_version"],
        "root_sequence": fixture.roots["candidate_root"]["signed"]["sequence"],
        "root_transcript_sha256": fixture.roots["candidate_root"]["transcript"]["sha256"],
        "index_generation": generation,
        "index_sequence": sequence,
        "index_transcript_sha256": transcript_sha256,
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
    VerificationExpectation(expectation)
}

fn build_index(
    fixture: &TrustFixture,
    registry_root: &Path,
    sequence: u64,
    previous: Option<&RegistryIndex>,
) -> RegistryVersion {
    let releases = release_values(registry_root);
    let snapshot_id = format!("registry.test.1.{sequence}");
    let previous_sha256 = previous
        .map(|index| {
            index.envelope()["transcript"]["sha256"]
                .as_str()
                .expect("previous index transcript")
                .to_owned()
        })
        .unwrap_or_else(|| "00".repeat(32));
    let build_expectation = expectation_for_index(
        fixture,
        &releases,
        1,
        sequence,
        &snapshot_id,
        &previous_sha256,
        previous,
    );
    let index = build_registry_index(
        registry_root,
        &RegistryIndexOptions {
            registry_identity: REGISTRY_IDENTITY,
            source_identity: SOURCE_IDENTITY,
            generation: 1,
            sequence,
            issued_at: "2026-07-29T11:00:00Z",
            expires_at: "2026-12-31T00:00:00Z",
            snapshot_id: &snapshot_id,
            metadata_path: &format!("1/{sequence}/index.v2.json"),
            previous_snapshot_sha256: &previous_sha256,
            trust_roots: &fixture.roots,
            verification_expectation: &build_expectation,
            signers: &fixture.index_signers,
        },
    )
    .expect("build runtime-generated signed registry index");
    let expectation = expectation_for_index(
        fixture,
        &releases,
        1,
        sequence,
        &snapshot_id,
        &previous_sha256,
        previous,
    );
    RegistryVersion { index, expectation }
}

fn write_project(
    root: &Path,
    index_url: &str,
    expectation: &VerificationExpectation,
    root_requirement: &str,
    local_requirement: &str,
) -> PathBuf {
    let project = root.join("app");
    let local = project.join("deps/local-util");
    fs::create_dir_all(project.join("src")).expect("create app source");
    fs::create_dir_all(local.join("src")).expect("create path dependency source");
    fs::create_dir_all(project.join("trust")).expect("create app trust directory");
    fs::write(
        project.join("axiom.toml"),
        format!(
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n\
             [build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n\
             [registry]\nname = \"fixture\"\nindex = {index_url:?}\n\
             trust_roots = \"trust/roots.json\"\nexpectation = \"trust/expectation.json\"\n\
             cache = \".axiom/cache\"\nvendor = \"vendor\"\n\n\
             [dependencies.local_util]\npath = \"deps/local-util\"\nversion = \"^0.4.0\"\n\n\
             [dependencies.core]\nregistry = \"fixture\"\nnamespace = \"axiom\"\n\
             package = \"core\"\nversion = {root_requirement:?}\n"
        ),
    )
    .expect("write app manifest");
    fs::write(
        project.join("src/main.ax"),
        "print \"package resolver app\"\n",
    )
    .expect("write app source");
    fs::write(
        project.join("src/smoke_test.ax"),
        "print \"resolver test\"\n",
    )
    .expect("write app test source");
    let mut app_manifest =
        fs::read_to_string(project.join("axiom.toml")).expect("read app manifest for test target");
    app_manifest.push_str(
        "\n[[tests]]\nname = \"resolver-smoke\"\nentry = \"src/smoke_test.ax\"\n\
         stdout = \"resolver test\\n\"\n",
    );
    fs::write(project.join("axiom.toml"), app_manifest).expect("write app test target");
    fs::write(
        local.join("axiom.toml"),
        format!(
            "[package]\nname = \"local-util\"\nversion = \"0.4.0\"\n\n\
             [build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n\
             [registry]\nname = \"fixture\"\nindex = {index_url:?}\n\
             trust_roots = \"trust/roots.json\"\nexpectation = \"trust/expectation.json\"\n\
             cache = \".axiom/cache\"\nvendor = \"vendor\"\n\n\
             [dependencies.core]\nregistry = \"fixture\"\nnamespace = \"axiom\"\n\
             package = \"core\"\nversion = {local_requirement:?}\n"
        ),
    )
    .expect("write path dependency manifest");
    fs::write(local.join("src/main.ax"), "print \"local util\"\n")
        .expect("write path dependency source");
    write_expectation(&project, expectation);
    project
}

fn add_registry_dependency(project: &Path, alias: &str, package: &str, requirement: &str) {
    let manifest_path = project.join("axiom.toml");
    let mut manifest = fs::read_to_string(&manifest_path).expect("read root manifest");
    manifest.push_str(&format!(
        "\n[dependencies.{alias}]\nregistry = \"fixture\"\nnamespace = \"axiom\"\n\
         package = {package:?}\nversion = {requirement:?}\n"
    ));
    fs::write(manifest_path, manifest).expect("add root registry dependency");
}

fn write_path_only_manifests(project: &Path) {
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n\
         [build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n\
         [dependencies.local_util]\npath = \"deps/local-util\"\nversion = \"^0.4.0\"\n\n\
         [[tests]]\nname = \"resolver-smoke\"\nentry = \"src/smoke_test.ax\"\n\
         stdout = \"resolver test\\n\"\n",
    )
    .expect("write path-only app manifest");
    fs::write(
        project.join("deps/local-util/axiom.toml"),
        "[package]\nname = \"local-util\"\nversion = \"0.4.0\"\n\n\
         [build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n",
    )
    .expect("write path-only dependency manifest");
}

fn write_v1_path_lock(project: &Path, root_version: &str) {
    fs::write(
        project.join("axiom.lock"),
        format!(
            "version = 1\n\n\
             [[package]]\nname = \"app\"\nversion = {root_version:?}\nsource = \"path\"\n\n\
             [[package]]\nname = \"local-util\"\nversion = \"0.4.0\"\n\
             source = \"path:deps/local-util\"\n"
        ),
    )
    .expect("write path-only v1 lock");
}

fn prune_lock_to_path_only(project: &Path) {
    let ParsedLockfile::V2(mut lockfile) = load_lockfile(project).expect("load fetched lock v2")
    else {
        panic!("fetch must produce axiom.lock v2");
    };
    lockfile.registry.clear();
    lockfile
        .package
        .retain(|package| package.registry.is_none());
    lockfile.edge.retain(|edge| {
        lockfile
            .package
            .iter()
            .any(|package| package.id == edge.from)
            && lockfile.package.iter().any(|package| package.id == edge.to)
    });
    write_lockfile_v2_atomic(project, &lockfile).expect("write path-only v2 lock");
}

fn write_trust_roots(project: &Path, roots: &TrustRootsEnvelope) {
    fs::write(
        project.join("trust/roots.json"),
        canonical_json(roots).expect("canonical trust roots"),
    )
    .expect("write trust roots");
}

fn write_expectation(project: &Path, expectation: &VerificationExpectation) {
    fs::write(
        project.join("trust/expectation.json"),
        canonical_json(expectation).expect("canonical verification expectation"),
    )
    .expect("write verification expectation");
}

fn run_axiomc(project: &Path, args: &[&str]) -> Output {
    let project_arg = project.to_str().expect("UTF-8 project path");
    Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args(args.iter().map(|argument| {
            if *argument == "." {
                project_arg
            } else {
                argument
            }
        }))
        .current_dir(project)
        .output()
        .expect("run axiomc package resolver command")
}

fn assert_json_success(output: &Output, operation: &str) -> Value {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{operation} failed; stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let mut documents = serde_json::Deserializer::from_slice(&output.stdout).into_iter::<Value>();
    let value = documents
        .next()
        .expect("one JSON document")
        .expect("valid JSON document");
    assert!(
        documents.next().is_none(),
        "{operation} stdout must contain exactly one JSON document"
    );
    value
}

fn assert_json_failure(output: &Output, operation: &str) -> Value {
    assert_eq!(
        output.status.code(),
        Some(1),
        "{operation} should fail closed; stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{operation} failure must be JSON: {error}; stdout={}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn assert_published_schema(value: &Value, relative: &str, operation: &str) {
    let schema: Value = serde_json::from_slice(
        &fs::read(stage1_path(relative))
            .unwrap_or_else(|error| panic!("read {relative} for {operation}: {error}")),
    )
    .unwrap_or_else(|error| panic!("parse {relative} for {operation}: {error}"));
    let validator = jsonschema::validator_for(&schema)
        .unwrap_or_else(|error| panic!("compile {relative} for {operation}: {error}"));
    if let Err(error) = validator.validate(value) {
        panic!("{operation} must validate against {relative}: {error}; value={value}");
    }
}

fn assert_error_code(value: &Value, expected: &str) {
    assert_eq!(
        value["error"]["code"], expected,
        "failure must expose exact structured code {expected:?}: {value}"
    );
}

fn assert_resolver_failure(
    value: &Value,
    expected_error_code: &str,
    expected_kind: &str,
    expected_trace_events: &[&str],
) {
    assert_published_schema(
        value,
        "schemas/axiom.stage1.v1.schema.json",
        "resolver failure envelope",
    );
    assert_error_code(value, expected_error_code);
    let trace = value["trace"]
        .as_array()
        .unwrap_or_else(|| panic!("resolver failure must expose a trace array: {value}"));
    assert!(
        !trace.is_empty(),
        "resolver failure trace must contain decision evidence: {value}"
    );
    for expected in expected_trace_events {
        assert!(
            trace.iter().any(|entry| entry["event"] == *expected),
            "resolver failure trace must contain {expected:?}: {trace:?}"
        );
    }
    let resolver = value["resolver"]
        .as_object()
        .unwrap_or_else(|| panic!("resolver failure must expose its typed payload: {value}"));
    assert_eq!(
        resolver.get("kind"),
        Some(&Value::String(expected_kind.to_owned())),
        "resolver failure must preserve typed kind {expected_kind:?}: {resolver:?}"
    );
    assert_eq!(
        resolver.get("trace"),
        Some(&Value::Array(trace.clone())),
        "top-level and typed resolver traces must describe the same failure"
    );
}

fn assert_verification_rejection_code(
    value: &Value,
    expected_error_code: &str,
    expected_resolver_kind: &str,
    expected_rejection_code: &str,
) {
    assert_resolver_failure(
        value,
        expected_error_code,
        expected_resolver_kind,
        &["candidate_rejected"],
    );
    let rejection = value["trace"]
        .as_array()
        .expect("resolver trace")
        .iter()
        .find(|entry| entry["event"] == "candidate_rejected")
        .unwrap_or_else(|| panic!("resolver trace must contain a candidate rejection: {value}"));
    assert_eq!(
        rejection["reason"]["reason"], "verification_failed",
        "candidate rejection must be attributed to Package Trust: {rejection}"
    );
    assert_eq!(
        rejection["reason"]["code"], expected_rejection_code,
        "candidate rejection must preserve exact Package Trust code {expected_rejection_code:?}: {rejection}"
    );
}

fn assert_trace_events(value: &Value, operation: &str, expected: &[&str]) {
    let trace = value["trace"]
        .as_array()
        .unwrap_or_else(|| panic!("{operation} report must expose a trace array: {value}"));
    for event in expected {
        assert!(
            trace.iter().any(|entry| entry["event"] == *event),
            "{operation} trace must contain {event:?}: {trace:?}"
        );
    }
}

fn locked_registry_version(project: &Path) -> String {
    let ParsedLockfile::V2(lockfile) = load_lockfile(project).expect("load lockfile v2") else {
        panic!("registry fetch must create axiom.lock v2");
    };
    lockfile
        .package
        .iter()
        .find(|package| package.registry.as_deref() == Some("fixture"))
        .expect("locked registry package")
        .version
        .clone()
}

fn locked_registry_digest(project: &Path) -> String {
    let ParsedLockfile::V2(lockfile) = load_lockfile(project).expect("load lockfile v2") else {
        panic!("registry fetch must create axiom.lock v2");
    };
    lockfile
        .package
        .iter()
        .find(|package| package.registry.as_deref() == Some("fixture"))
        .and_then(|package| package.archive_sha256.clone())
        .expect("locked registry archive digest")
}

fn first_file(root: &Path) -> PathBuf {
    let mut files = Vec::new();
    collect_all_files(root, &mut files);
    files.sort();
    files
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("expected a materialized file below {}", root.display()))
}

fn collect_all_files(root: &Path, output: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("read materialization entry").path();
        if path.is_dir() {
            collect_all_files(&path, output);
        } else {
            output.push(path);
        }
    }
}

fn first_directory(root: &Path) -> PathBuf {
    let mut directories = fs::read_dir(root)
        .unwrap_or_else(|error| panic!("read directory {}: {error}", root.display()))
        .map(|entry| entry.expect("read directory entry").path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    directories.sort();
    directories
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("expected a directory below {}", root.display()))
}

fn tamper_cache_blob(cache: &Path, digest: &str) {
    fs::write(cache.join("blobs/sha256").join(digest), b"tampered blob")
        .expect("tamper cache blob");
}

fn tamper_cache_tree(cache: &Path, digest: &str) {
    let tree = cache
        .join("trees/axiom-package-extractor-v1/sha256")
        .join(digest);
    fs::write(first_file(&tree), b"tampered extracted tree").expect("tamper cache tree");
}

fn tamper_cache_integrity(cache: &Path, digest: &str) {
    let mut paths = Vec::new();
    collect_named_files(
        &cache.join("evidence/sha256").join(digest),
        "integrity.json",
        &mut paths,
    );
    paths.sort();
    fs::write(paths.first().expect("cache integrity manifest"), b"{}")
        .expect("tamper cache integrity manifest");
}

fn tamper_cache_evidence(cache: &Path, digest: &str) {
    let mut paths = Vec::new();
    collect_named_files(
        &cache.join("evidence/sha256").join(digest),
        "manifest",
        &mut paths,
    );
    paths.sort();
    fs::write(
        paths.first().expect("cache manifest evidence"),
        b"tampered evidence",
    )
    .expect("tamper cache evidence");
}

fn tamper_cache_commit(cache: &Path, digest: &str) {
    let versioned = cache.join("commits/sha256").join(digest);
    let path = if versioned.is_dir() {
        first_file(&versioned)
    } else {
        cache.join("commits/sha256").join(format!("{digest}.json"))
    };
    fs::write(path, b"{}").expect("tamper cache commit");
}

fn vendor_snapshot_root(vendor: &Path) -> PathBuf {
    let current = fs::read_to_string(vendor.join("CURRENT")).expect("read vendor CURRENT");
    let digest = current
        .strip_suffix('\n')
        .expect("vendor CURRENT has one newline");
    vendor.join("snapshots/sha256").join(digest)
}

fn tamper_vendor_tree(vendor: &Path) {
    let package = first_directory(&vendor_snapshot_root(vendor).join("packages/sha256"));
    fs::write(first_file(&package.join("tree")), b"tampered vendor tree")
        .expect("tamper vendor tree");
}

fn tamper_vendor_manifest(vendor: &Path) {
    fs::write(
        vendor_snapshot_root(vendor).join("vendor-manifest.json"),
        b"{}",
    )
    .expect("tamper vendor manifest");
}

fn tamper_vendor_current(vendor: &Path) {
    fs::write(vendor.join("CURRENT"), format!("{}\n", "0".repeat(64)))
        .expect("tamper vendor CURRENT");
}

fn remove_vendor_current(vendor: &Path) {
    fs::remove_file(vendor.join("CURRENT")).expect("remove vendor CURRENT");
}

fn copy_directory(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap_or_else(|error| {
        panic!(
            "create copied directory {} from {}: {error}",
            destination.display(),
            source.display()
        )
    });
    for entry in fs::read_dir(source).expect("read source directory") {
        let entry = entry.expect("read source entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_directory(&source_path, &destination_path);
        } else {
            fs::copy(&source_path, &destination_path).unwrap_or_else(|error| {
                panic!(
                    "copy {} to {}: {error}",
                    source_path.display(),
                    destination_path.display()
                )
            });
        }
    }
}

fn restore_directory(source: &Path, destination: &Path) {
    if destination.exists() {
        fs::remove_dir_all(destination).expect("remove mutated directory");
    }
    copy_directory(source, destination);
}

fn build_locked_offline(project: &Path) -> Output {
    run_axiomc(
        project,
        &[
            "build",
            ".",
            "--backend",
            "cranelift",
            "--locked",
            "--offline",
            "--json",
        ],
    )
}

#[test]
fn fetch_cache_vendor_and_offline_build_fail_closed_round_trip() {
    let _guard = PACKAGE_RESOLVER_CLI_PROCESS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let server = HttpRegistry::start();
    let temp = tempfile::tempdir().expect("create package resolver tempdir");
    let fixture = trust_fixture();
    let registry_root = temp.path().join("registry");
    publish_core(temp.path(), &registry_root, &fixture, "1.2.3", 1);
    let initial = build_index(&fixture, &registry_root, 1, None);
    server.install(&registry_root, &initial.index);
    let project = write_project(
        temp.path(),
        &server.index_url(),
        &initial.expectation,
        "^1.2.3",
        "^1.2.3",
    );
    write_trust_roots(&project, &fixture.roots);

    let fetch = run_axiomc(&project, &["pkg", "fetch", ".", "--json"]);
    let fetch = assert_json_success(&fetch, "pkg fetch");
    assert_eq!(fetch["schema_version"], "axiom.package_operation_report.v1");
    assert_eq!(fetch["operation"], "fetch");
    assert_eq!(fetch["transport_used"], true);
    assert_trace_events(
        &fetch,
        "pkg fetch",
        &[
            "path_preserved",
            "constraint_added",
            "catalog_authenticated",
            "candidate_considered",
            "candidate_verified",
            "selected",
        ],
    );
    assert_eq!(locked_registry_version(&project), "1.2.3");
    assert!(
        fs::read_to_string(project.join("axiom.lock"))
            .expect("read lockfile")
            .contains("deps/local-util"),
        "the path dependency remains a path package in lock v2"
    );
    assert!(
        server.request_count() >= 5,
        "index and exact release bytes fetched"
    );

    let requests_before_graph = server.request_count();
    let graph = assert_json_success(
        &run_axiomc(&project, &["pkg", "graph", ".", "--json"]),
        "pkg graph",
    );
    assert_published_schema(
        &graph,
        "compiler-contracts/schemas/axiom.compiler.package_graph.runtime.v1.schema.json",
        "runtime package graph",
    );
    assert_eq!(
        graph["schema_version"],
        "axiom.compiler.package_graph.runtime.v1"
    );
    assert_eq!(graph["contract"], "compiler.package_graph.runtime");
    let graph_packages = graph["packages"].as_array().expect("graph packages");
    assert_eq!(graph_packages.len(), 3, "root, path, and registry packages");
    let app = graph_packages
        .iter()
        .find(|package| package["name"] == "app")
        .expect("app graph package");
    assert_eq!(app["lockfile"]["version"], 2);
    assert_eq!(app["lockfile"]["status"], "current");
    assert!(
        app["dependencies"]
            .as_array()
            .expect("app dependencies")
            .iter()
            .any(|dependency| {
                dependency["name"] == "local_util" && dependency["source_kind"] == "path"
            }),
        "pkg graph preserves the local path edge"
    );
    let core = graph_packages
        .iter()
        .find(|package| package["name"] == "core")
        .expect("core graph package");
    assert_eq!(core["source"], "registry:fixture/axiom/core");
    assert_eq!(
        app["dependencies"]
            .as_array()
            .expect("app dependencies")
            .iter()
            .find(|dependency| dependency["name"] == "core")
            .expect("registry dependency")["selected_version"],
        "1.2.3"
    );
    assert!(core["trust"].is_object(), "registry package exposes trust");
    assert_eq!(
        core["materialization"]["package_trust_verified"], true,
        "pkg graph exposes verified materialization evidence"
    );
    let ParsedLockfile::V2(graph_lockfile) = load_lockfile(&project).expect("load graph lock v2")
    else {
        panic!("pkg graph registry project must retain lockfile v2");
    };
    let locked_registry = graph_lockfile
        .registry
        .iter()
        .find(|registry| registry.name == "fixture")
        .expect("locked fixture registry");
    let locked_core = graph_lockfile
        .package
        .iter()
        .find(|package| package.id == core["id"].as_str().expect("core package id"))
        .expect("locked core package");
    assert_eq!(
        core["trust"]["index_sha256"], locked_registry.index_sha256,
        "pkg graph binds trust evidence to exact authenticated index bytes"
    );
    assert_eq!(
        core["trust"]["verification_sha256"],
        locked_core
            .verification_sha256
            .as_deref()
            .expect("locked registry package verification digest"),
        "pkg graph binds trust evidence to exact verification bytes"
    );
    assert_eq!(
        server.request_count(),
        requests_before_graph,
        "pkg graph materializes from locked local state without transport"
    );

    let check = assert_json_success(
        &run_axiomc(&project, &["check", ".", "--json"]),
        "signed registry graph check",
    );
    assert_eq!(check["command"], "check");
    assert_eq!(
        check["packages"].as_array().expect("check packages").len(),
        1,
        "check reports the selected workspace package after loading the signed graph"
    );
    let test = assert_json_success(
        &run_axiomc(&project, &["test", ".", "--json"]),
        "signed registry graph test",
    );
    assert_eq!(test["command"], "test");
    let run = assert_json_success(
        &run_axiomc(&project, &["run", ".", "--backend", "cranelift", "--json"]),
        "signed registry graph run",
    );
    assert_eq!(run["command"], "run");
    assert_eq!(run["exit_code"], 0);
    let sbom = assert_json_success(
        &run_axiomc(&project, &["caps", ".", "--format", "sbom-json"]),
        "signed registry graph capability SBOM",
    );
    let sbom_packages = sbom["packages"].as_array().expect("SBOM packages");
    assert_eq!(
        sbom_packages.len(),
        3,
        "capability SBOM consumes root, path, and registry packages"
    );
    let sbom_core = sbom_packages
        .iter()
        .find(|package| package["name"] == "core")
        .expect("registry package in capability SBOM");
    assert!(sbom_core["trust"].is_object());
    assert_eq!(sbom_core["materialization"]["package_trust_verified"], true);
    assert_eq!(
        server.request_count(),
        requests_before_graph,
        "check, test, run, graph, and capability SBOM reuse locked local state"
    );

    let requests_after_online_fetch = server.request_count();
    assert_json_success(&build_locked_offline(&project), "locked offline build");
    assert_eq!(
        server.request_count(),
        requests_after_online_fetch,
        "offline build must not use the loopback transport"
    );

    let cache = project.join(".axiom/cache");
    let digest = locked_registry_digest(&project);
    let cache_mutations: [(&str, fn(&Path, &str)); 5] = [
        ("blob", tamper_cache_blob),
        ("tree", tamper_cache_tree),
        ("integrity", tamper_cache_integrity),
        ("evidence", tamper_cache_evidence),
        ("commit", tamper_cache_commit),
    ];
    for (label, mutate) in cache_mutations {
        if cache.exists() {
            fs::remove_dir_all(&cache).expect("remove prior cache fixture");
        }
        assert_json_success(
            &run_axiomc(&project, &["pkg", "fetch", ".", "--json"]),
            &format!("restore cache before {label} tamper"),
        );
        mutate(&cache, &digest);
        let requests_before_offline_rejection = server.request_count();
        let rejected = build_locked_offline(&project);
        let rejected = assert_json_failure(&rejected, &format!("{label}-tampered offline build"));
        assert!(
            rejected.to_string().contains("package")
                || rejected.to_string().contains("cache")
                || rejected.to_string().contains("archive"),
            "{label} tamper rejection is explicit: {rejected}"
        );
        assert_eq!(
            server.request_count(),
            requests_before_offline_rejection,
            "{label} tamper rejection must not use transport"
        );
    }

    fs::remove_dir_all(&cache).expect("remove last tampered cache");
    let requests_before_missing_cache = server.request_count();
    let missing = build_locked_offline(&project);
    let missing_json = assert_json_failure(&missing, "missing-cache offline build");
    assert!(
        missing_json.to_string().contains("cache") || missing_json.to_string().contains("store"),
        "missing cache rejection is explicit: {missing_json}"
    );
    assert_eq!(
        server.request_count(),
        requests_before_missing_cache,
        "missing-cache offline build must not use transport"
    );

    assert_json_success(
        &run_axiomc(&project, &["pkg", "fetch", ".", "--json"]),
        "cache restoration fetch",
    );
    let requests_after_restoration = server.request_count();
    let lock_path = project.join("axiom.lock");
    let original_lock = fs::read_to_string(&lock_path).expect("read clean lockfile");
    let tampered_lock = original_lock.replacen(&digest, &"0".repeat(64), 1);
    assert_ne!(tampered_lock, original_lock, "lock fixture contains digest");
    fs::write(&lock_path, tampered_lock).expect("tamper lockfile digest");
    let lock_rejection = assert_json_failure(
        &build_locked_offline(&project),
        "tampered-lock offline build",
    );
    assert!(
        lock_rejection.to_string().contains("lock")
            || lock_rejection.to_string().contains("digest"),
        "lock tamper rejection is explicit: {lock_rejection}"
    );
    assert_eq!(
        server.request_count(),
        requests_after_restoration,
        "lock tamper rejection must not use transport"
    );
    fs::write(&lock_path, original_lock).expect("restore exact lockfile");

    let requests_before_vendor = server.request_count();
    let vendor = assert_json_success(
        &run_axiomc(&project, &["pkg", "vendor", ".", "--json"]),
        "pkg vendor",
    );
    assert_eq!(vendor["operation"], "vendor");
    assert_eq!(vendor["transport_used"], false);
    assert_eq!(
        server.request_count(),
        requests_before_vendor,
        "pkg vendor has no transport handle and must not use network"
    );
    let vendor_root = project.join("vendor");
    assert!(vendor_root.join("CURRENT").exists());
    let clean_vendor = temp.path().join("clean-vendor");
    copy_directory(&vendor_root, &clean_vendor);
    fs::remove_dir_all(&cache).expect("remove cache after vendoring");
    let vendored_build =
        assert_json_success(&build_locked_offline(&project), "vendor-only offline build");
    assert_eq!(vendored_build["locked"], true);
    assert_eq!(vendored_build["offline"], true);
    assert_eq!(
        server.request_count(),
        requests_after_restoration,
        "vendor and vendor-only offline build must not use transport"
    );

    let vendor_mutations: [(&str, fn(&Path)); 4] = [
        ("tree", tamper_vendor_tree),
        ("manifest", tamper_vendor_manifest),
        ("CURRENT", tamper_vendor_current),
        ("missing CURRENT", remove_vendor_current),
    ];
    for (label, mutate) in vendor_mutations {
        restore_directory(&clean_vendor, &vendor_root);
        mutate(&vendor_root);
        let requests_before_vendor_rejection = server.request_count();
        let rejected = assert_json_failure(
            &build_locked_offline(&project),
            &format!("{label}-tampered vendor-only build"),
        );
        assert!(
            rejected.to_string().contains("vendor")
                || rejected.to_string().contains("package")
                || rejected.to_string().contains("CURRENT"),
            "{label} vendor rejection is explicit: {rejected}"
        );
        assert_eq!(
            server.request_count(),
            requests_before_vendor_rejection,
            "{label} vendor rejection must not use transport"
        );
    }
}

#[test]
fn update_exact_caret_yank_and_replay_are_deterministic() {
    let _guard = PACKAGE_RESOLVER_CLI_PROCESS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let server = HttpRegistry::start();
    let temp = tempfile::tempdir().expect("create package resolver tempdir");
    let fixture = trust_fixture();
    let registry_root = temp.path().join("registry");
    publish_core(temp.path(), &registry_root, &fixture, "1.2.3", 1);
    let initial = build_index(&fixture, &registry_root, 1, None);
    server.install(&registry_root, &initial.index);
    let project = write_project(
        temp.path(),
        &server.index_url(),
        &initial.expectation,
        "^1.2.3",
        "^1.2.3",
    );
    write_trust_roots(&project, &fixture.roots);
    assert_json_success(
        &run_axiomc(&project, &["pkg", "fetch", ".", "--json"]),
        "initial fetch",
    );
    assert_eq!(locked_registry_version(&project), "1.2.3");

    publish_core(temp.path(), &registry_root, &fixture, "1.2.4", 2);
    let newer = build_index(&fixture, &registry_root, 2, Some(&initial.index));
    server.install(&registry_root, &newer.index);
    let update = assert_json_success(
        &run_axiomc(
            &project,
            &["pkg", "update", ".", "--package", "core", "--json"],
        ),
        "targeted caret update",
    );
    assert_eq!(update["operation"], "update");
    assert_trace_events(
        &update,
        "pkg update",
        &[
            "path_preserved",
            "constraint_added",
            "catalog_authenticated",
            "candidate_considered",
            "candidate_verified",
            "selected",
        ],
    );
    assert_eq!(locked_registry_version(&project), "1.2.4");

    let exact_project = write_project(
        &temp.path().join("exact"),
        &server.index_url(),
        &newer.expectation,
        "1.2.3",
        "1.2.3",
    );
    write_trust_roots(&exact_project, &fixture.roots);
    assert_json_success(
        &run_axiomc(&exact_project, &["pkg", "fetch", ".", "--json"]),
        "exact fetch",
    );
    assert_eq!(locked_registry_version(&exact_project), "1.2.3");

    fs::write(
        registry_root.join("axiom/core/1.2.4/axiom-registry.toml"),
        "yanked = true\nyank_reason = \"fixture withdrawal\"\n",
    )
    .expect("yank core 1.2.4");
    let yanked = build_index(&fixture, &registry_root, 3, Some(&newer.index));
    server.install(&registry_root, &yanked.index);
    assert_json_success(
        &run_axiomc(&project, &["pkg", "update", ".", "--json"]),
        "update away from yank",
    );
    assert_eq!(locked_registry_version(&project), "1.2.3");

    server.install(&registry_root, &initial.index);
    let first_replay = run_axiomc(&project, &["pkg", "update", ".", "--json"]);
    let first_replay_json = assert_json_failure(&first_replay, "registry replay");
    let second_replay = run_axiomc(&project, &["pkg", "update", ".", "--json"]);
    let second_replay_json = assert_json_failure(&second_replay, "registry replay repeat");
    assert_eq!(
        first_replay_json, second_replay_json,
        "registry replay rejection is byte-stable at the JSON value level"
    );
    assert_error_code(&first_replay_json, "registry_catalog_rejected");
    assert!(
        first_replay_json.to_string().contains("ROLLBACK_DETECTED")
            || first_replay_json.to_string().contains("METADATA_REPLAYED"),
        "replay rejection preserves Package Trust reason codes: {first_replay_json}"
    );
}

#[test]
fn path_only_v2_survives_removing_the_last_registry_dependency_and_config() {
    let _guard = PACKAGE_RESOLVER_CLI_PROCESS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let server = HttpRegistry::start();
    let temp = tempfile::tempdir().expect("create package resolver tempdir");
    let fixture = trust_fixture();
    let registry_root = temp.path().join("registry");
    publish_core(temp.path(), &registry_root, &fixture, "1.2.3", 1);
    let initial = build_index(&fixture, &registry_root, 1, None);
    server.install(&registry_root, &initial.index);
    let project = write_project(
        temp.path(),
        &server.index_url(),
        &initial.expectation,
        "^1.2.3",
        "^1.2.3",
    );
    write_trust_roots(&project, &fixture.roots);
    assert_json_success(
        &run_axiomc(&project, &["pkg", "fetch", ".", "--json"]),
        "registry-to-path-only setup fetch",
    );

    write_path_only_manifests(&project);
    prune_lock_to_path_only(&project);
    fs::remove_dir_all(project.join("trust")).expect("remove obsolete trust configuration");
    fs::remove_dir_all(project.join(".axiom/cache")).expect("remove obsolete registry cache");
    let ParsedLockfile::V2(path_only_lock) =
        load_lockfile(&project).expect("load path-only lock v2")
    else {
        panic!("path-only lock must remain v2");
    };
    assert!(path_only_lock.registry.is_empty());
    assert_eq!(path_only_lock.package.len(), 2);
    assert!(
        path_only_lock
            .package
            .iter()
            .all(|package| package.registry.is_none())
    );
    let requests_before_locked_commands = server.request_count();

    let graph = assert_json_success(
        &run_axiomc(&project, &["pkg", "graph", ".", "--json"]),
        "path-only v2 graph",
    );
    assert_eq!(
        graph["packages"]
            .as_array()
            .expect("path-only graph packages")
            .len(),
        2
    );
    assert_json_success(
        &run_axiomc(&project, &["check", ".", "--json"]),
        "path-only v2 check",
    );
    assert_json_success(
        &run_axiomc(&project, &["test", ".", "--json"]),
        "path-only v2 test",
    );
    assert_json_success(
        &run_axiomc(&project, &["run", ".", "--backend", "cranelift", "--json"]),
        "path-only v2 run",
    );
    let sbom = assert_json_success(
        &run_axiomc(&project, &["caps", ".", "--format", "sbom-json"]),
        "path-only v2 capability SBOM",
    );
    assert_eq!(
        sbom["packages"]
            .as_array()
            .expect("path-only SBOM packages")
            .len(),
        2
    );
    assert_json_success(
        &build_locked_offline(&project),
        "path-only v2 offline build",
    );
    let vendor = assert_json_success(
        &run_axiomc(&project, &["pkg", "vendor", ".", "--json"]),
        "path-only v2 vendor",
    );
    assert_eq!(vendor["transport_used"], false);
    assert_eq!(
        server.request_count(),
        requests_before_locked_commands,
        "path-only v2 commands require neither registry transport nor trust/cache files"
    );
}

#[test]
fn valid_v1_migrates_to_v2_but_stale_v1_is_rejected_without_rewrite() {
    let _guard = PACKAGE_RESOLVER_CLI_PROCESS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let server = HttpRegistry::start();
    let temp = tempfile::tempdir().expect("create package resolver tempdir");
    let fixture = trust_fixture();
    let registry_root = temp.path().join("registry");
    publish_core(temp.path(), &registry_root, &fixture, "1.2.3", 1);
    let initial = build_index(&fixture, &registry_root, 1, None);
    server.install(&registry_root, &initial.index);

    let current = write_project(
        &temp.path().join("current-v1"),
        &server.index_url(),
        &initial.expectation,
        "^1.2.3",
        "^1.2.3",
    );
    write_trust_roots(&current, &fixture.roots);
    write_v1_path_lock(&current, "0.1.0");
    let migrated = assert_json_success(
        &run_axiomc(&current, &["pkg", "fetch", ".", "--json"]),
        "current v1 to v2 migration",
    );
    assert_eq!(migrated["operation"], "fetch");
    assert_trace_events(
        &migrated,
        "v1 migration fetch",
        &["path_preserved", "catalog_authenticated", "selected"],
    );
    assert!(
        matches!(
            load_lockfile(&current).expect("load migrated lock"),
            ParsedLockfile::V2(_)
        ),
        "successful trusted fetch atomically migrates v1 to v2"
    );

    let stale = write_project(
        &temp.path().join("stale-v1"),
        &server.index_url(),
        &initial.expectation,
        "^1.2.3",
        "^1.2.3",
    );
    write_trust_roots(&stale, &fixture.roots);
    write_v1_path_lock(&stale, "9.9.9");
    let stale_lock = fs::read(stale.join("axiom.lock")).expect("read stale v1 lock");
    let rejected = assert_json_failure(
        &run_axiomc(&stale, &["pkg", "fetch", ".", "--json"]),
        "stale v1 migration",
    );
    assert_error_code(&rejected, "stale_v1_lockfile");
    assert_eq!(
        fs::read(stale.join("axiom.lock")).expect("reread stale v1 lock"),
        stale_lock,
        "stale v1 rejection must not rewrite the previous lock"
    );
    assert!(
        !stale.join(".axiom/cache/commits/sha256").exists(),
        "stale v1 rejection must occur before cache admission"
    );
}

#[test]
fn targeted_update_rejects_same_version_non_target_artifact_overwrite() {
    let _guard = PACKAGE_RESOLVER_CLI_PROCESS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let server = HttpRegistry::start();
    let temp = tempfile::tempdir().expect("create package resolver tempdir");
    let fixture = trust_fixture();
    let registry_root = temp.path().join("registry");
    publish_core(temp.path(), &registry_root, &fixture, "1.2.3", 1);
    publish_fixture_package(
        temp.path(),
        &registry_root,
        &fixture,
        "support",
        "1.2.3",
        1,
        false,
        "original",
    );
    let initial = build_index(&fixture, &registry_root, 1, None);
    server.install(&registry_root, &initial.index);
    let project = write_project(
        temp.path(),
        &server.index_url(),
        &initial.expectation,
        "^1.2.3",
        "^1.2.3",
    );
    add_registry_dependency(&project, "support", "support", "^1.2.3");
    write_trust_roots(&project, &fixture.roots);
    assert_json_success(
        &run_axiomc(&project, &["pkg", "fetch", ".", "--json"]),
        "two-package initial fetch",
    );
    let initial_lock = fs::read(project.join("axiom.lock")).expect("read two-package lock");

    publish_core(temp.path(), &registry_root, &fixture, "1.2.4", 2);
    publish_fixture_package(
        temp.path(),
        &registry_root,
        &fixture,
        "support",
        "1.2.3",
        2,
        true,
        "overwritten",
    );
    let overwritten = build_index(&fixture, &registry_root, 2, Some(&initial.index));
    server.install(&registry_root, &overwritten.index);
    let rejected = assert_json_failure(
        &run_axiomc(
            &project,
            &["pkg", "update", ".", "--package", "core", "--json"],
        ),
        "targeted update with overwritten frozen artifact",
    );
    assert_error_code(&rejected, "frozen_package_identity_changed");
    assert_eq!(
        fs::read(project.join("axiom.lock")).expect("reread two-package lock"),
        initial_lock,
        "targeted update must not rewrite any lock entry when a frozen artifact changed"
    );
}

#[test]
fn registry_artifact_and_signature_tampering_fail_before_lock_admission() {
    let _guard = PACKAGE_RESOLVER_CLI_PROCESS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let server = HttpRegistry::start();
    let temp = tempfile::tempdir().expect("create package resolver tempdir");
    let fixture = trust_fixture();
    let registry_root = temp.path().join("registry");
    publish_core(temp.path(), &registry_root, &fixture, "1.2.3", 1);
    let index = build_index(&fixture, &registry_root, 1, None);
    let project = write_project(
        temp.path(),
        &server.index_url(),
        &index.expectation,
        "^1.2.3",
        "^1.2.3",
    );
    write_trust_roots(&project, &fixture.roots);

    for (label, route, bytes, expected) in [
        (
            "archive",
            "/axiom/core/1.2.3/package.axp",
            b"tampered archive bytes".as_slice(),
            "archive_length_mismatch",
        ),
        (
            "signature",
            "/axiom/core/1.2.3/package.axp.sig",
            b"{\"invalid\":true}".as_slice(),
            "package_signature_invalid",
        ),
    ] {
        server.install(&registry_root, &index.index);
        server.replace_route(route, bytes);
        let rejected = assert_json_failure(
            &run_axiomc(&project, &["pkg", "fetch", ".", "--json"]),
            &format!("{label}-tampered registry fetch"),
        );
        assert_verification_rejection_code(&rejected, "package_source_failed", "source", expected);
        assert!(
            !project.join("axiom.lock").exists(),
            "{label} rejection must occur before lock admission"
        );
        assert!(
            !project.join(".axiom/cache/commits/sha256").exists(),
            "{label} rejection must occur before cache commit admission"
        );
    }
}

#[test]
fn incompatible_direct_and_path_transitive_constraints_report_stable_conflict() {
    let _guard = PACKAGE_RESOLVER_CLI_PROCESS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let server = HttpRegistry::start();
    let temp = tempfile::tempdir().expect("create package resolver tempdir");
    let fixture = trust_fixture();
    let registry_root = temp.path().join("registry");
    publish_core(temp.path(), &registry_root, &fixture, "1.2.3", 1);
    publish_core(temp.path(), &registry_root, &fixture, "1.2.4", 1);
    let index = build_index(&fixture, &registry_root, 1, None);
    server.install(&registry_root, &index.index);
    let project = write_project(
        temp.path(),
        &server.index_url(),
        &index.expectation,
        "1.2.4",
        "1.2.3",
    );
    write_trust_roots(&project, &fixture.roots);

    let first = run_axiomc(&project, &["pkg", "fetch", ".", "--json"]);
    let first_json = assert_json_failure(&first, "conflicting fetch");
    let second = run_axiomc(&project, &["pkg", "fetch", ".", "--json"]);
    let second_json = assert_json_failure(&second, "conflicting fetch repeat");
    assert_eq!(
        first_json, second_json,
        "constraint conflict is deterministic across identical CLI attempts"
    );
    assert_resolver_failure(
        &first_json,
        "resolution_conflict",
        "conflict",
        &["constraint_added", "conflict"],
    );
    assert!(
        !project.join("axiom.lock").exists(),
        "failed resolution must not partially write lock v2"
    );
}
