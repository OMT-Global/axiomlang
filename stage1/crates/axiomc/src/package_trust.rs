//! Package Trust v1 verification.
//!
//! This module is deliberately side-effect free. It accepts already-delivered
//! metadata and package evidence, authenticates it, and returns a deterministic
//! verdict. Callers remain responsible for obtaining bytes and persisting an
//! accepted trusted-state update.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::sync::OnceLock;

use curve25519_dalek::{edwards::CompressedEdwardsY, scalar::Scalar};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

pub const PACKAGE_DOMAIN: &str = "AXIOM-PACKAGE-TRUST-V1";
pub const ROOT_DOMAIN: &str = "AXIOM-TRUST-ROOT-V1";
pub const INDEX_DOMAIN: &str = "AXIOM-REGISTRY-INDEX-V2";
pub const MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SIGNATURES: usize = 16;
pub const MAX_ROOT_KEYS: usize = 128;
pub const MAX_ROLES: usize = 64;
pub const MAX_NAMESPACE_GRANTS: usize = 2_048;
pub const MAX_RELEASES: usize = 1_024;
pub const MAX_ROLE_KEY_IDS: usize = 128;
pub const MAX_REQUIRED_KEY_IDS: usize = 16;
pub const MAX_THRESHOLD: u64 = 16;
pub const MAX_SLSA_COLLECTION_ITEMS: usize = 1_024;
pub const MAX_SEEN_SNAPSHOTS: usize = 10_000;
pub const MAX_MAP_ENTRIES: usize = 128;
const MAX_DOCUMENT_DEPTH: usize = 128;

pub const PACKAGE_FIELDS: [&str; 23] = [
    "transcript_format_version",
    "signature_algorithm",
    "signature_version",
    "signature_message_mode",
    "archive_digest_algorithm",
    "archive_digest",
    "archive_length",
    "manifest_digest",
    "package_namespace",
    "package_name",
    "package_version",
    "target_path",
    "registry_identity",
    "source_identity",
    "publisher_identity",
    "provenance_statement_digest",
    "provenance_statement_type",
    "provenance_predicate_type",
    "provenance_subject_name",
    "provenance_subject_digest",
    "index_generation",
    "index_sequence",
    "package_signature_threshold",
];

pub const REASON_PRECEDENCE: [&str; 44] = [
    "OFFLINE_INPUT_MISSING",
    "ROOT_BOOTSTRAP_MISMATCH",
    "METADATA_EXPIRED",
    "ROOT_ROTATION_INVALID",
    "ROOT_DIGEST_MISMATCH",
    "ROOT_SIGNATURE_INVALID",
    "ROOT_THRESHOLD_NOT_MET",
    "ROOT_ROLLBACK",
    "ROLLBACK_DETECTED",
    "METADATA_REPLAYED",
    "VERSION_DOWNGRADE",
    "INDEX_DIGEST_MISMATCH",
    "INDEX_SIGNATURE_INVALID",
    "INDEX_THRESHOLD_NOT_MET",
    "DUPLICATE_RELEASE",
    "DUPLICATE_TARGET_PATH",
    "DUPLICATE_PACKAGE_COORDINATE",
    "TARGET_PATH_INVALID",
    "ARCHIVE_DIGEST_MISMATCH",
    "MANIFEST_DIGEST_MISMATCH",
    "PROVENANCE_STATEMENT_MISMATCH",
    "PROVENANCE_PREDICATE_MISMATCH",
    "PROVENANCE_SUBJECT_MISMATCH",
    "DELEGATION_INVALID",
    "NAMESPACE_GRANT_MISMATCH",
    "DUPLICATE_KEY",
    "KEY_MALFORMED",
    "KEY_ID_MISMATCH",
    "KEY_SUPERSESSION_INVALID",
    "KEY_UNKNOWN",
    "KEY_REVOKED",
    "KEY_RETIRED",
    "KEY_NOT_YET_VALID",
    "SIGNER_PUBLISHER_MISMATCH",
    "PUBLISHER_MISMATCH",
    "NAMESPACE_MISMATCH",
    "PACKAGE_NAME_MISMATCH",
    "PACKAGE_VERSION_MISMATCH",
    "SOURCE_MISMATCH",
    "TARGET_PATH_MISMATCH",
    "SIGNATURE_MALFORMED",
    "SIGNATURE_INVALID",
    "PACKAGE_THRESHOLD_NOT_MET",
    "OFFLINE_LOCK_MISMATCH",
];

/// The four authenticated inputs needed by the verifier.
///
/// Values intentionally retain the published schema shapes. This keeps the
/// verifier independent from transport and storage while avoiding a second,
/// subtly different Rust wire contract.
macro_rules! trust_document {
    ($name:ident) => {
        #[derive(Clone, Debug, Default, Deserialize, Serialize)]
        #[serde(transparent)]
        pub struct $name(pub Value);

        impl std::ops::Deref for $name {
            type Target = Value;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
    };
}

trust_document!(PackageSignatureEnvelope);
trust_document!(TrustRootsEnvelope);
trust_document!(RegistryIndexEnvelope);
trust_document!(VerificationExpectation);

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PackageTrustInput {
    #[serde(default)]
    pub package_signature: PackageSignatureEnvelope,
    #[serde(default)]
    pub trust_roots: TrustRootsEnvelope,
    #[serde(default)]
    pub registry_index: RegistryIndexEnvelope,
    #[serde(default)]
    pub verification_expectation: VerificationExpectation,
}

/// Borrowed package bytes supplied by a registry consumer.
///
/// The verifier never retains or mutates these bytes.
#[derive(Clone, Copy, Debug, Default)]
pub struct PackageArtifacts<'a> {
    pub archive: Option<&'a [u8]>,
    pub manifest: Option<&'a [u8]>,
    /// Exact canonical in-toto statement bytes, not a containing envelope.
    pub provenance: Option<&'a [u8]>,
}

/// A parsed contract bundle. `verification` is comparison evidence, never an
/// input to the trust decision.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PackageTrustContract {
    pub schema_version: String,
    pub contract: String,
    pub contract_status: String,
    pub specification: Value,
    pub package_signature: PackageSignatureEnvelope,
    pub trust_roots: TrustRootsEnvelope,
    pub registry_index: RegistryIndexEnvelope,
    pub verification_expectation: VerificationExpectation,
    pub verification: Value,
    pub positive_vectors: Vec<Value>,
    pub negative_vectors: Vec<Value>,
}

impl From<&PackageTrustContract> for PackageTrustInput {
    fn from(bundle: &PackageTrustContract) -> Self {
        Self {
            package_signature: bundle.package_signature.clone(),
            trust_roots: bundle.trust_roots.clone(),
            registry_index: bundle.registry_index.clone(),
            verification_expectation: bundle.verification_expectation.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct ObservedPackage {
    pub registry_identity: Option<String>,
    pub source_identity: Option<String>,
    pub namespace: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub target_path: Option<String>,
    pub publisher_identity: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct VerifiedSigner {
    pub key_id: String,
    pub public_key_fingerprint: String,
    pub publisher_identity: Option<String>,
    pub role_id: Option<String>,
    pub algorithm: String,
    pub status: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct TrustEvidence {
    pub root_version: Option<u64>,
    pub root_sequence: Option<u64>,
    pub root_transition_from: Option<u64>,
    pub index_generation: u64,
    pub index_sequence: u64,
    pub package_threshold: u64,
    pub package_valid_signers: usize,
    pub index_threshold: Option<u64>,
    pub index_valid_signers: usize,
    pub offline_mode: Option<String>,
    pub network_fallback: Option<bool>,
    pub consistent_snapshot: Option<bool>,
}

/// Typed `axiom.package_verification.v1` output.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct PackageVerification {
    pub schema_version: String,
    pub contract: String,
    pub contract_status: String,
    pub decision: String,
    pub primary_reason_code: String,
    pub reason_codes: Vec<String>,
    pub observed: ObservedPackage,
    pub signers: Vec<VerifiedSigner>,
    pub archive: Value,
    pub manifest_digest: Value,
    pub provenance: Value,
    pub trust: TrustEvidence,
}

/// Classification for a document that could not reach semantic verification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageInputFailure {
    /// A required file was absent or could not be read.
    MissingOrUnreadable,
    /// The verification expectation was not strict, valid JSON.
    VerificationExpectationMalformed,
    /// The trust-root envelope was not strict, valid JSON.
    TrustRootsMalformed,
    /// The registry-index envelope was not strict, valid JSON.
    RegistryIndexMalformed,
    /// The package-signature envelope was not strict, valid JSON.
    PackageSignatureMalformed,
}

/// Authenticated candidate-root authority for a registry-index signing role.
///
/// Registry publication obtains this context before invoking any signer
/// provider. `eligible_key_ids` is sorted and already accounts for key status,
/// candidate-root sequence, and the verification time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryIndexTrustPreflight {
    pub root_version: u64,
    pub root_sequence: u64,
    pub index_role_id: String,
    pub index_threshold: u64,
    pub index_eligible_key_ids: Vec<String>,
    pub package_role_id: String,
    pub package_threshold: u64,
    pub package_eligible_key_ids: Vec<String>,
}

/// Stable fail-closed reasons returned by registry-index trust preflight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustRootPreflightError {
    pub reason_codes: Vec<String>,
}

impl fmt::Display for TrustRootPreflightError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Package Trust root preflight rejected: {}",
            self.reason_codes.join(", ")
        )
    }
}

impl std::error::Error for TrustRootPreflightError {}

/// Authenticated package authority established before registry-index signing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageReleaseTrustPreflight {
    pub package_threshold: u64,
    pub valid_signer_key_ids: Vec<String>,
}

/// Stable fail-closed reasons returned by package-release preflight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageReleasePreflightError {
    pub reason_codes: Vec<String>,
}

impl fmt::Display for PackageReleasePreflightError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Package Trust release preflight rejected: {}",
            self.reason_codes.join(", ")
        )
    }
}

impl std::error::Error for PackageReleasePreflightError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageTrustError {
    message: String,
}

impl PackageTrustError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for PackageTrustError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for PackageTrustError {}

/// Key-storage-agnostic Ed25519 provider.
///
/// Implementations may delegate to an HSM, keychain, or isolated signing
/// service. Secret key material is never accepted by this API.
pub trait Ed25519Signer {
    type Error;

    fn public_key(&self) -> Result<[u8; 32], Self::Error>;
    fn sign(&self, message: &[u8]) -> Result<[u8; 64], Self::Error>;
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct PackageSignatureEntry {
    pub key_id: String,
    pub algorithm: String,
    pub encoding: String,
    pub value: String,
}

#[derive(Debug)]
pub enum PackageSigningError<E> {
    Provider(E),
    Transcript(PackageTrustError),
    PublicKeyRejected,
    SignatureRejected,
}

/// Sign an exact Package Trust TLV transcript using an external key provider.
///
/// The returned key ID is derived from the provider's public key. Provider
/// output is strictly verified before it is returned to publishing code.
pub fn sign_package_transcript<S: Ed25519Signer>(
    package: &PackageSignatureEnvelope,
    package_threshold: u64,
    signer: &S,
) -> Result<PackageSignatureEntry, PackageSigningError<S::Error>> {
    let transcript =
        package_transcript(package, package_threshold).map_err(PackageSigningError::Transcript)?;
    let public_bytes = signer.public_key().map_err(PackageSigningError::Provider)?;
    public_key(&public_bytes).map_err(|_| PackageSigningError::PublicKeyRejected)?;
    let signature = signer
        .sign(&transcript)
        .map_err(PackageSigningError::Provider)?;
    let public_hex = hex_encode(&public_bytes);
    let signature_hex = hex_encode(&signature);
    if signature_status(
        &Value::String(public_hex.clone()),
        &transcript,
        &Value::String(signature_hex.clone()),
    )
    .is_some()
    {
        return Err(PackageSigningError::SignatureRejected);
    }
    let key_material = serde_json::json!({
        "algorithm": "ed25519",
        "public_key_encoding": "lowercase-hex",
        "public_key": public_hex,
    });
    let key_id = derived_key_id(&key_material).ok_or(PackageSigningError::PublicKeyRejected)?;
    Ok(PackageSignatureEntry {
        key_id,
        algorithm: "ed25519".to_owned(),
        encoding: "lowercase-hex".to_owned(),
        value: signature_hex,
    })
}

/// Return whether the authenticated package's signed index coordinates are
/// componentwise publication floors for the current registry index.
///
/// This permits retaining an unchanged package envelope in later indexes while
/// rejecting a package signed for any future generation or sequence.
pub fn package_index_floor_is_satisfied(
    package: &PackageSignatureEnvelope,
    current_generation: u64,
    current_sequence: u64,
) -> bool {
    number(field(field(package, "index"), "generation"))
        .is_some_and(|generation| generation <= current_generation)
        && number(field(field(package, "index"), "sequence"))
            .is_some_and(|sequence| sequence <= current_sequence)
}

fn parse_strict_value(bytes: &[u8]) -> Result<Value, PackageTrustError> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = StrictValueSeed
        .deserialize(&mut deserializer)
        .map_err(|error| PackageTrustError::new(error.to_string()))?;
    deserializer
        .end()
        .map_err(|error| PackageTrustError::new(error.to_string()))?;
    Ok(value)
}

/// Parse arbitrary JSON with duplicate-member rejection and the Package Trust
/// document-size bound. This is suitable for canonical provenance input.
pub fn parse_strict_json_value(bytes: &[u8]) -> Result<Value, PackageTrustError> {
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(PackageTrustError::new(
            "JSON document exceeds the Package Trust work budget",
        ));
    }
    parse_strict_value(bytes)
}

#[cfg(test)]
fn parse_strict_document<T>(bytes: &[u8]) -> Result<T, PackageTrustError>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_value(parse_strict_json_value(bytes)?)
        .map_err(|error| PackageTrustError::new(error.to_string()))
}

fn parse_schema_document<T>(
    bytes: &[u8],
    schema_bytes: &[u8],
    cached: &'static OnceLock<Result<jsonschema::Validator, String>>,
    document_name: &str,
    kind: DocumentKind,
) -> Result<T, PackageTrustError>
where
    T: serde::de::DeserializeOwned,
{
    let value = parse_strict_json_value(bytes)?;
    if !validate_document_work_budget(&value, kind) {
        return Err(PackageTrustError::new(format!(
            "{document_name} exceeds the Package Trust work budget"
        )));
    }
    validate_schema_value(&value, schema_bytes, cached, document_name)?;
    serde_json::from_value(value).map_err(|error| PackageTrustError::new(error.to_string()))
}

fn validate_schema_value(
    value: &Value,
    schema_bytes: &[u8],
    cached: &'static OnceLock<Result<jsonschema::Validator, String>>,
    document_name: &str,
) -> Result<(), PackageTrustError> {
    let validator = cached
        .get_or_init(|| {
            let schema: Value =
                serde_json::from_slice(schema_bytes).map_err(|error| error.to_string())?;
            jsonschema::options()
                .with_draft(jsonschema::Draft::Draft202012)
                .build(&schema)
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|_| PackageTrustError::new(format!("{document_name} schema is unavailable")))?;
    validator.validate(value).map_err(|_| {
        PackageTrustError::new(format!(
            "{document_name} does not match its published schema"
        ))
    })
}

#[derive(Clone, Copy)]
enum DocumentKind {
    Package,
    Roots,
    Index,
    Expectation,
}

fn within_limit(value: &Value, field_name: &str, limit: usize) -> bool {
    field(value, field_name)
        .as_array()
        .is_none_or(|items| items.len() <= limit)
}

fn validate_document_work_budget(value: &Value, kind: DocumentKind) -> bool {
    let mut estimated_bytes = 0_usize;
    let mut stack = vec![(value, "", 0_usize)];
    while let Some((node, field_name, depth)) = stack.pop() {
        if depth > MAX_DOCUMENT_DEPTH {
            return false;
        }
        // One byte per value plus the unescaped key/string bytes is a lower
        // bound on the corresponding JSON encoding. This lets programmatic
        // callers fail closed above 8 MiB without rejecting a separately
        // parsed document that was within the exact raw-byte cap.
        estimated_bytes = match estimated_bytes.checked_add(1) {
            Some(value) if value <= MAX_DOCUMENT_BYTES => value,
            _ => return false,
        };
        match node {
            Value::String(text) => {
                let limit = if matches!(field_name, "bytes_hex" | "canonical_bytes_hex") {
                    MAX_DOCUMENT_BYTES
                } else if field_name.contains("path")
                    || field_name.contains("text")
                    || field_name.contains("message")
                    || field_name.contains("reason")
                    || field_name.contains("invocation")
                    || field_name.contains("snapshot")
                {
                    4_096
                } else if field_name.contains("uri")
                    || field_name.contains("source")
                    || field_name.contains("publisher")
                    || field_name.contains("registry")
                    || field_name.contains("identity")
                    || matches!(field_name, "buildType" | "build_type" | "builder")
                {
                    2_048
                } else if matches!(
                    field_name,
                    "namespace"
                        | "version"
                        | "role_id"
                        | "delegated_by"
                        | "display_name"
                        | "field_order"
                ) {
                    256
                } else {
                    // The authoritative schemas narrow package identifiers to
                    // 256 and external identities to 2048. The remaining
                    // schema-valid free-text and SLSA map values are bounded
                    // at 4096, so the coarse pre-schema budget must not reject
                    // them more aggressively.
                    4_096
                };
                if text.len() > limit {
                    return false;
                }
                estimated_bytes = match estimated_bytes.checked_add(text.len()) {
                    Some(value) if value <= MAX_DOCUMENT_BYTES => value,
                    _ => return false,
                };
            }
            Value::Array(items) => {
                let limit = match field_name {
                    "signatures"
                    | "candidate_signatures_by_old_root"
                    | "candidate_signatures_by_new_root" => MAX_SIGNATURES,
                    "required_key_ids" => MAX_REQUIRED_KEY_IDS,
                    "key_ids" | "supersedes_key_ids" => MAX_ROLE_KEY_IDS,
                    "keys" => MAX_ROOT_KEYS,
                    "roles" => MAX_ROLES,
                    "namespace_grants" => MAX_NAMESPACE_GRANTS,
                    "releases" => MAX_RELEASES,
                    "subject" | "resolvedDependencies" | "builderDependencies" | "byproducts" => {
                        MAX_SLSA_COLLECTION_ITEMS
                    }
                    "seen_snapshots" => MAX_SEEN_SNAPSHOTS,
                    _ => MAX_DOCUMENT_BYTES,
                };
                if items.len() > limit {
                    return false;
                }
                stack.extend(items.iter().map(|item| (item, field_name, depth + 1)));
            }
            Value::Object(items) => {
                if items.len() > MAX_MAP_ENTRIES {
                    return false;
                }
                for (name, item) in items {
                    if name.len() > 256 {
                        return false;
                    }
                    estimated_bytes = match estimated_bytes.checked_add(name.len()) {
                        Some(value) if value <= MAX_DOCUMENT_BYTES => value,
                        _ => return false,
                    };
                    stack.push((item, name, depth + 1));
                }
            }
            Value::Number(value)
                if field_name.contains("threshold")
                    && value.as_u64().is_none_or(|value| value > MAX_THRESHOLD) =>
            {
                return false;
            }
            _ => {}
        }
    }

    match kind {
        DocumentKind::Package => within_limit(value, "signatures", MAX_SIGNATURES),
        DocumentKind::Index => {
            within_limit(value, "signatures", MAX_SIGNATURES)
                && within_limit(field(value, "signed"), "releases", MAX_RELEASES)
        }
        DocumentKind::Expectation => within_limit(
            field(value, "required_signers"),
            "required_key_ids",
            MAX_REQUIRED_KEY_IDS,
        ),
        DocumentKind::Roots => {
            let trusted = field(field(value, "trusted_root"), "signed");
            let candidate = field(field(value, "candidate_root"), "signed");
            let transition = field(value, "transition");
            [trusted, candidate].iter().all(|signed| {
                within_limit(signed, "keys", MAX_ROOT_KEYS)
                    && within_limit(signed, "roles", MAX_ROLES)
                    && within_limit(signed, "namespace_grants", MAX_NAMESPACE_GRANTS)
                    && field(signed, "roles").as_array().is_none_or(|roles| {
                        roles
                            .iter()
                            .all(|role| within_limit(role, "key_ids", MAX_ROLE_KEY_IDS))
                    })
            }) && within_limit(field(value, "trusted_root"), "signatures", MAX_SIGNATURES)
                && within_limit(field(value, "candidate_root"), "signatures", MAX_SIGNATURES)
                && within_limit(
                    transition,
                    "candidate_signatures_by_old_root",
                    MAX_SIGNATURES,
                )
                && within_limit(
                    transition,
                    "candidate_signatures_by_new_root",
                    MAX_SIGNATURES,
                )
        }
    }
}

fn input_work_budget_failure(input: &PackageTrustInput) -> Option<PackageInputFailure> {
    [
        (
            &input.package_signature.0,
            DocumentKind::Package,
            PackageInputFailure::PackageSignatureMalformed,
        ),
        (
            &input.trust_roots.0,
            DocumentKind::Roots,
            PackageInputFailure::TrustRootsMalformed,
        ),
        (
            &input.registry_index.0,
            DocumentKind::Index,
            PackageInputFailure::RegistryIndexMalformed,
        ),
        (
            &input.verification_expectation.0,
            DocumentKind::Expectation,
            PackageInputFailure::VerificationExpectationMalformed,
        ),
    ]
    .into_iter()
    .find_map(|(document, kind, failure)| {
        (!validate_document_work_budget(document, kind)).then_some(failure)
    })
}

static PACKAGE_SIGNATURE_SCHEMA: OnceLock<Result<jsonschema::Validator, String>> = OnceLock::new();
static TRUST_ROOTS_SCHEMA: OnceLock<Result<jsonschema::Validator, String>> = OnceLock::new();
static REGISTRY_INDEX_SCHEMA: OnceLock<Result<jsonschema::Validator, String>> = OnceLock::new();
static VERIFICATION_EXPECTATION_SCHEMA: OnceLock<Result<jsonschema::Validator, String>> =
    OnceLock::new();
static VERIFICATION_RESULT_SCHEMA: OnceLock<Result<jsonschema::Validator, String>> =
    OnceLock::new();

fn input_schema_failure(input: &PackageTrustInput) -> Option<PackageInputFailure> {
    [
        (
            &input.package_signature.0,
            include_bytes!("../../../schemas/axiom-package-signature-v1.schema.json").as_slice(),
            &PACKAGE_SIGNATURE_SCHEMA,
            "package signature",
            PackageInputFailure::PackageSignatureMalformed,
        ),
        (
            &input.trust_roots.0,
            include_bytes!("../../../schemas/axiom-trust-roots-v1.schema.json").as_slice(),
            &TRUST_ROOTS_SCHEMA,
            "trust roots",
            PackageInputFailure::TrustRootsMalformed,
        ),
        (
            &input.registry_index.0,
            include_bytes!("../../../schemas/axiom-registry-index-v2.schema.json").as_slice(),
            &REGISTRY_INDEX_SCHEMA,
            "registry index",
            PackageInputFailure::RegistryIndexMalformed,
        ),
        (
            &input.verification_expectation.0,
            include_bytes!(
                "../../../schemas/axiom-package-verification-expectation-v1.schema.json"
            )
            .as_slice(),
            &VERIFICATION_EXPECTATION_SCHEMA,
            "verification expectation",
            PackageInputFailure::VerificationExpectationMalformed,
        ),
    ]
    .into_iter()
    .find_map(|(document, schema, cached, name, failure)| {
        validate_schema_value(document, schema, cached, name)
            .is_err()
            .then_some(failure)
    })
}

/// Parse JSON while rejecting duplicate object members at every depth.
#[cfg(test)]
pub(crate) fn parse_contract_json(bytes: &[u8]) -> Result<PackageTrustContract, PackageTrustError> {
    parse_strict_document(bytes)
}

/// Parse a metadata-only test-oracle envelope and validate every production
/// member against its authoritative schema.
#[cfg(test)]
pub(crate) fn parse_input_json(bytes: &[u8]) -> Result<PackageTrustInput, PackageTrustError> {
    let value = parse_strict_json_value(bytes)?;
    let package_signature = field(&value, "package_signature").clone();
    let trust_roots = field(&value, "trust_roots").clone();
    let registry_index = field(&value, "registry_index").clone();
    let verification_expectation = field(&value, "verification_expectation").clone();
    for (document, kind, name) in [
        (
            &package_signature,
            DocumentKind::Package,
            "package signature",
        ),
        (&trust_roots, DocumentKind::Roots, "trust roots"),
        (&registry_index, DocumentKind::Index, "registry index"),
        (
            &verification_expectation,
            DocumentKind::Expectation,
            "verification expectation",
        ),
    ] {
        if !validate_document_work_budget(document, kind) {
            return Err(PackageTrustError::new(format!(
                "{name} exceeds the Package Trust work budget"
            )));
        }
    }
    validate_schema_value(
        &package_signature,
        include_bytes!("../../../schemas/axiom-package-signature-v1.schema.json"),
        &PACKAGE_SIGNATURE_SCHEMA,
        "package signature",
    )?;
    validate_schema_value(
        &trust_roots,
        include_bytes!("../../../schemas/axiom-trust-roots-v1.schema.json"),
        &TRUST_ROOTS_SCHEMA,
        "trust roots",
    )?;
    validate_schema_value(
        &registry_index,
        include_bytes!("../../../schemas/axiom-registry-index-v2.schema.json"),
        &REGISTRY_INDEX_SCHEMA,
        "registry index",
    )?;
    validate_schema_value(
        &verification_expectation,
        include_bytes!("../../../schemas/axiom-package-verification-expectation-v1.schema.json"),
        &VERIFICATION_EXPECTATION_SCHEMA,
        "verification expectation",
    )?;
    Ok(PackageTrustInput {
        package_signature: PackageSignatureEnvelope(package_signature),
        trust_roots: TrustRootsEnvelope(trust_roots),
        registry_index: RegistryIndexEnvelope(registry_index),
        verification_expectation: VerificationExpectation(verification_expectation),
    })
}

/// Strictly parse one package-signature document loaded from a separate file.
pub fn parse_package_signature_json(
    bytes: &[u8],
) -> Result<PackageSignatureEnvelope, PackageTrustError> {
    parse_schema_document(
        bytes,
        include_bytes!("../../../schemas/axiom-package-signature-v1.schema.json"),
        &PACKAGE_SIGNATURE_SCHEMA,
        "package signature",
        DocumentKind::Package,
    )
}

/// Strictly parse one trust-roots document loaded from a separate file.
pub fn parse_trust_roots_json(bytes: &[u8]) -> Result<TrustRootsEnvelope, PackageTrustError> {
    parse_schema_document(
        bytes,
        include_bytes!("../../../schemas/axiom-trust-roots-v1.schema.json"),
        &TRUST_ROOTS_SCHEMA,
        "trust roots",
        DocumentKind::Roots,
    )
}

/// Strictly parse one registry-index document loaded from a separate file.
pub fn parse_registry_index_json(bytes: &[u8]) -> Result<RegistryIndexEnvelope, PackageTrustError> {
    parse_schema_document(
        bytes,
        include_bytes!("../../../schemas/axiom-registry-index-v2.schema.json"),
        &REGISTRY_INDEX_SCHEMA,
        "registry index",
        DocumentKind::Index,
    )
}

/// Strictly parse one verification-expectation document loaded from a separate
/// file.
pub fn parse_verification_expectation_json(
    bytes: &[u8],
) -> Result<VerificationExpectation, PackageTrustError> {
    parse_schema_document(
        bytes,
        include_bytes!("../../../schemas/axiom-package-verification-expectation-v1.schema.json"),
        &VERIFICATION_EXPECTATION_SCHEMA,
        "verification expectation",
        DocumentKind::Expectation,
    )
}

/// Validate an outbound runtime result against the fifth Package Trust schema.
pub fn validate_package_verification(
    verification: &PackageVerification,
) -> Result<(), PackageTrustError> {
    let value = serde_json::to_value(verification)
        .map_err(|_| PackageTrustError::new("package verification result cannot be encoded"))?;
    validate_schema_value(
        &value,
        include_bytes!("../../../schemas/axiom-package-verification-v1.schema.json"),
        &VERIFICATION_RESULT_SCHEMA,
        "package verification result",
    )
}

struct StrictValueSeed;

impl<'de> DeserializeSeed<'de> for StrictValueSeed {
    type Value = Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictValueVisitor)
    }
}

struct StrictValueVisitor;

impl<'de> Visitor<'de> for StrictValueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Value, E>
    where
        E: de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Value, E>
    where
        E: de::Error,
    {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(StrictValueSeed)? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some(key) = object.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate JSON member {key:?}")));
            }
            let value = object.next_value_seed(StrictValueSeed)?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

fn object(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

fn field<'a>(value: &'a Value, name: &str) -> &'a Value {
    value.get(name).unwrap_or(&Value::Null)
}

fn text(value: &Value) -> Option<&str> {
    value.as_str()
}

fn number(value: &Value) -> Option<u64> {
    value.as_u64()
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn is_nfc(value: &str) -> bool {
    value.nfc().eq(value.chars())
}

/// Encode the integer-only, NFC `axiom-canonical-json-v1` subset.
pub fn canonical_json(value: &Value) -> Result<Vec<u8>, PackageTrustError> {
    fn encode(value: &Value, output: &mut String, path: &str) -> Result<(), PackageTrustError> {
        match value {
            Value::Null => output.push_str("null"),
            Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
            Value::Number(value) if value.is_i64() || value.is_u64() => {
                output.push_str(&value.to_string())
            }
            Value::Number(_) => {
                return Err(PackageTrustError::new(format!(
                    "{path} contains a non-canonical JSON number"
                )));
            }
            Value::String(value) => {
                if !is_nfc(value) {
                    return Err(PackageTrustError::new(format!("{path} must be NFC")));
                }
                output.push_str(
                    &serde_json::to_string(value)
                        .map_err(|error| PackageTrustError::new(error.to_string()))?,
                );
            }
            Value::Array(values) => {
                output.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    encode(value, output, &format!("{path}[{index}]"))?;
                }
                output.push(']');
            }
            Value::Object(values) => {
                output.push('{');
                let sorted: BTreeMap<_, _> = values.iter().collect();
                for (index, (key, value)) in sorted.into_iter().enumerate() {
                    if !is_nfc(key) {
                        return Err(PackageTrustError::new(format!("{path} key must be NFC")));
                    }
                    if index != 0 {
                        output.push(',');
                    }
                    output.push_str(
                        &serde_json::to_string(key)
                            .map_err(|error| PackageTrustError::new(error.to_string()))?,
                    );
                    output.push(':');
                    encode(value, output, &format!("{path}.{key}"))?;
                }
                output.push('}');
            }
        }
        Ok(())
    }

    let mut output = String::new();
    encode(value, &mut output, "$")?;
    Ok(output.into_bytes())
}

pub fn metadata_transcript(domain: &str, signed: &Value) -> Result<Vec<u8>, PackageTrustError> {
    if !domain.is_ascii() || domain.len() > u16::MAX as usize {
        return Err(PackageTrustError::new("metadata domain is invalid"));
    }
    let payload = canonical_json(signed)?;
    let mut transcript = Vec::with_capacity(2 + domain.len() + 8 + payload.len());
    transcript.extend_from_slice(&(domain.len() as u16).to_be_bytes());
    transcript.extend_from_slice(domain.as_bytes());
    transcript.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    transcript.extend_from_slice(&payload);
    Ok(transcript)
}

enum TranscriptValue {
    Integer(u64),
    Text(String),
    Bytes(Vec<u8>),
}

fn decode_hex(value: &Value) -> Result<Vec<u8>, PackageTrustError> {
    let value = value
        .as_str()
        .ok_or_else(|| PackageTrustError::new("expected hexadecimal string"))?;
    let encoded = value.as_bytes();
    if encoded.len() % 2 != 0 {
        return Err(PackageTrustError::new(
            "hexadecimal string has odd byte length",
        ));
    }
    fn nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        }
    }
    encoded
        .chunks_exact(2)
        .map(|pair| {
            let high = nibble(pair[0])
                .ok_or_else(|| PackageTrustError::new("invalid lowercase hexadecimal string"))?;
            let low = nibble(pair[1])
                .ok_or_else(|| PackageTrustError::new("invalid lowercase hexadecimal string"))?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn canonical_key_id(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value.as_bytes()[7..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

/// Build the exact `axiom-tlv-v1` package-signing transcript.
pub fn package_transcript(
    package: &Value,
    package_threshold: u64,
) -> Result<Vec<u8>, PackageTrustError> {
    let provenance = field(package, "provenance");
    let statement = field(provenance, "statement");
    let selected = field(provenance, "selected_subject");
    let values = [
        TranscriptValue::Integer(1),
        TranscriptValue::Text(required_text(field(field(package, "scheme"), "algorithm"))?),
        TranscriptValue::Integer(required_u64(field(field(package, "scheme"), "version"))?),
        TranscriptValue::Text(required_text(field(
            field(package, "scheme"),
            "message_mode",
        ))?),
        TranscriptValue::Text(required_text(field(
            field(field(package, "archive"), "digest"),
            "algorithm",
        ))?),
        TranscriptValue::Bytes(decode_hex(field(
            field(field(package, "archive"), "digest"),
            "value",
        ))?),
        TranscriptValue::Integer(required_u64(field(field(package, "archive"), "size"))?),
        TranscriptValue::Bytes(decode_hex(field(field(package, "manifest"), "value"))?),
        TranscriptValue::Text(required_text(field(
            field(package, "package"),
            "namespace",
        ))?),
        TranscriptValue::Text(required_text(field(field(package, "package"), "name"))?),
        TranscriptValue::Text(required_text(field(field(package, "package"), "version"))?),
        TranscriptValue::Text(required_text(field(
            field(package, "package"),
            "target_path",
        ))?),
        TranscriptValue::Text(required_text(field(
            field(package, "registry"),
            "registry_identity",
        ))?),
        TranscriptValue::Text(required_text(field(
            field(package, "registry"),
            "source_identity",
        ))?),
        TranscriptValue::Text(required_text(field(
            field(package, "publisher"),
            "publisher_identity",
        ))?),
        TranscriptValue::Bytes(decode_hex(field(field(statement, "digest"), "value"))?),
        TranscriptValue::Text(required_text(field(field(statement, "value"), "_type"))?),
        TranscriptValue::Text(required_text(field(
            field(statement, "value"),
            "predicateType",
        ))?),
        TranscriptValue::Text(required_text(field(selected, "name"))?),
        TranscriptValue::Bytes(decode_hex(field(field(selected, "digest"), "sha256"))?),
        TranscriptValue::Integer(required_u64(field(field(package, "index"), "generation"))?),
        TranscriptValue::Integer(required_u64(field(field(package, "index"), "sequence"))?),
        TranscriptValue::Integer(package_threshold),
    ];
    let mut transcript = Vec::new();
    transcript.extend_from_slice(&(PACKAGE_DOMAIN.len() as u16).to_be_bytes());
    transcript.extend_from_slice(PACKAGE_DOMAIN.as_bytes());
    transcript.extend_from_slice(&(PACKAGE_FIELDS.len() as u16).to_be_bytes());
    for (name, value) in PACKAGE_FIELDS.iter().zip(values) {
        let encoded = match value {
            TranscriptValue::Integer(value) => value.to_be_bytes().to_vec(),
            TranscriptValue::Text(value) => {
                if !is_nfc(&value) {
                    return Err(PackageTrustError::new(format!("{name} must be NFC")));
                }
                value.into_bytes()
            }
            TranscriptValue::Bytes(value) => value,
        };
        transcript.extend_from_slice(&(name.len() as u16).to_be_bytes());
        transcript.extend_from_slice(name.as_bytes());
        transcript.extend_from_slice(&(encoded.len() as u64).to_be_bytes());
        transcript.extend_from_slice(&encoded);
    }
    Ok(transcript)
}

fn required_text(value: &Value) -> Result<String, PackageTrustError> {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| PackageTrustError::new("expected string"))
}

fn required_u64(value: &Value) -> Result<u64, PackageTrustError> {
    value
        .as_u64()
        .ok_or_else(|| PackageTrustError::new("expected unsigned integer"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SignatureFailure {
    KeyMalformed,
    SignatureMalformed,
    SignatureInvalid,
}

fn public_key(bytes: &[u8]) -> Result<VerifyingKey, SignatureFailure> {
    let encoded: [u8; 32] = bytes
        .try_into()
        .map_err(|_| SignatureFailure::KeyMalformed)?;
    let point = CompressedEdwardsY(encoded)
        .decompress()
        .filter(|point| point.compress().to_bytes() == encoded)
        .filter(|point| !point.is_small_order() && point.is_torsion_free())
        .ok_or(SignatureFailure::KeyMalformed)?;
    let key = VerifyingKey::from_bytes(&encoded).map_err(|_| SignatureFailure::KeyMalformed)?;
    if key.is_weak() || point.compress().to_bytes() != key.to_bytes() {
        return Err(SignatureFailure::KeyMalformed);
    }
    Ok(key)
}

fn signature_status(
    public_key_hex: &Value,
    message: &[u8],
    signature_hex: &Value,
) -> Option<SignatureFailure> {
    let public_bytes = match decode_hex(public_key_hex) {
        Ok(value) => value,
        Err(_) => return Some(SignatureFailure::KeyMalformed),
    };
    let key = match public_key(&public_bytes) {
        Ok(value) => value,
        Err(error) => return Some(error),
    };
    let signature_bytes = match decode_hex(signature_hex) {
        Ok(value) if value.len() == 64 => value,
        _ => return Some(SignatureFailure::SignatureMalformed),
    };
    let r: [u8; 32] = signature_bytes[..32]
        .try_into()
        .expect("slice length checked");
    let s: [u8; 32] = signature_bytes[32..]
        .try_into()
        .expect("slice length checked");
    let r_point = match CompressedEdwardsY(r).decompress() {
        Some(point)
            if point.compress().to_bytes() == r
                && !point.is_small_order()
                && point.is_torsion_free() =>
        {
            point
        }
        _ => return Some(SignatureFailure::SignatureMalformed),
    };
    if r_point.compress().to_bytes() != r || bool::from(Scalar::from_canonical_bytes(s).is_none()) {
        return Some(SignatureFailure::SignatureMalformed);
    }
    let signature = match Signature::from_slice(&signature_bytes) {
        Ok(value) => value,
        Err(_) => return Some(SignatureFailure::SignatureMalformed),
    };
    match key.verify_strict(message, &signature) {
        Ok(()) => None,
        Err(_) => Some(SignatureFailure::SignatureInvalid),
    }
}

fn derived_key_id(material: &Value) -> Option<String> {
    object(material)?;
    canonical_json(material)
        .ok()
        .map(|encoded| format!("sha256:{}", sha256(&encoded)))
}

fn canonical_public_key(material: &Value) -> Option<[u8; 32]> {
    let material_object = object(material)?;
    if material_object.len() != 3
        || text(field(material, "algorithm")) != Some("ed25519")
        || text(field(material, "public_key_encoding")) != Some("lowercase-hex")
    {
        return None;
    }
    let bytes = decode_hex(field(material, "public_key")).ok()?;
    let bytes: [u8; 32] = bytes.try_into().ok()?;
    public_key(&bytes).ok()?;
    Some(bytes)
}

type KeyMap = HashMap<String, Value>;
type RoleMap = HashMap<String, Value>;

fn key_maps(root: &Value, failures: &mut BTreeSet<String>) -> (KeyMap, HashMap<String, String>) {
    let mut keys = HashMap::new();
    let mut fingerprints = HashMap::new();
    let mut seen_public = HashSet::new();
    for key in field(root, "keys").as_array().into_iter().flatten() {
        let Some(key_object) = object(key) else {
            add(failures, "KEY_MALFORMED");
            continue;
        };
        let key_id = key_object.get("key_id").and_then(Value::as_str);
        let material = key_object.get("key_material").unwrap_or(&Value::Null);
        let canonical_public = canonical_public_key(material);
        if canonical_public.is_none() {
            add(failures, "KEY_MALFORMED");
        }
        let derived = canonical_public.and_then(|_| derived_key_id(material));
        if key_id.is_none() || !key_id.is_some_and(canonical_key_id) || derived.as_deref() != key_id
        {
            add(failures, "KEY_ID_MISMATCH");
        }
        if let Some(public) = canonical_public
            && !seen_public.insert(public)
        {
            add(failures, "DUPLICATE_KEY");
        }
        if let Some(key_id) = key_id {
            if keys.insert(key_id.to_owned(), key.clone()).is_some() {
                add(failures, "DUPLICATE_KEY");
            }
            if let Some(derived) = derived {
                fingerprints.insert(key_id.to_owned(), derived);
            }
        }
    }
    (keys, fingerprints)
}

fn validate_key_supersession(keys: &KeyMap, failures: &mut BTreeSet<String>) {
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    for (key_id, key) in keys {
        let supersedes = field(key, "supersedes_key_ids").as_array();
        let valid_supersedes = supersedes.is_some_and(|items| {
            let strings: Vec<_> = items.iter().filter_map(Value::as_str).collect();
            strings.len() == items.len()
                && strings.iter().copied().collect::<HashSet<_>>().len() == strings.len()
        });
        if !valid_supersedes {
            add(failures, "KEY_SUPERSESSION_INVALID");
        }
        let predecessors: Vec<String> = supersedes
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect();
        graph.insert(key_id.clone(), predecessors.clone());
        let status = text(field(key, "status"));
        let revocation = field(key, "revocation");
        let valid_from = number(field(key, "valid_from_sequence"));
        if status == Some("revoked") {
            if object(revocation).is_none()
                || valid_from.is_none()
                || number(field(revocation, "effective_sequence")).is_none()
                || number(field(revocation, "effective_sequence")) < valid_from
            {
                add(failures, "KEY_SUPERSESSION_INVALID");
            }
        } else if !revocation.is_null() {
            add(failures, "KEY_SUPERSESSION_INVALID");
        }
        if !predecessors.is_empty() && status != Some("active") {
            add(failures, "KEY_SUPERSESSION_INVALID");
        }
        for predecessor_id in predecessors {
            let predecessor = keys.get(&predecessor_id);
            if predecessor.is_none()
                || predecessor_id == *key_id
                || predecessor.and_then(|value| text(field(value, "publisher_identity")))
                    != text(field(key, "publisher_identity"))
                || !matches!(
                    predecessor.and_then(|value| text(field(value, "status"))),
                    Some("retired" | "revoked")
                )
                || valid_from.is_none()
                || predecessor
                    .and_then(|value| number(field(value, "valid_from_sequence")))
                    .is_none()
                || valid_from
                    <= predecessor.and_then(|value| number(field(value, "valid_from_sequence")))
            {
                add(failures, "KEY_SUPERSESSION_INVALID");
            }
        }
    }
    // Kahn's algorithm bounds work to the graph size and avoids recursively
    // traversing attacker-controlled supersession chains.
    let mut incoming: HashMap<&str, usize> =
        graph.keys().map(|key_id| (key_id.as_str(), 0)).collect();
    for predecessors in graph.values() {
        for predecessor in predecessors {
            if let Some(count) = incoming.get_mut(predecessor.as_str()) {
                *count += 1;
            }
        }
    }
    let mut ready: Vec<&str> = incoming
        .iter()
        .filter_map(|(key_id, count)| (*count == 0).then_some(*key_id))
        .collect();
    let mut visited = 0_usize;
    while let Some(key_id) = ready.pop() {
        visited += 1;
        for predecessor in graph.get(key_id).into_iter().flatten() {
            let Some(count) = incoming.get_mut(predecessor.as_str()) else {
                continue;
            };
            *count -= 1;
            if *count == 0 {
                ready.push(predecessor);
            }
        }
    }
    if visited != graph.len() {
        add(failures, "KEY_SUPERSESSION_INVALID");
    }
}

fn role_maps(root: &Value, keys: &KeyMap, failures: &mut BTreeSet<String>) -> RoleMap {
    let mut roles = HashMap::new();
    for role in field(root, "roles").as_array().into_iter().flatten() {
        let Some(role_id) = text(field(role, "role_id")) else {
            add(failures, "DELEGATION_INVALID");
            continue;
        };
        if roles.insert(role_id.to_owned(), role.clone()).is_some() {
            add(failures, "DELEGATION_INVALID");
        }
        let ids = field(role, "key_ids").as_array();
        let threshold = number(field(role, "threshold"));
        let valid = ids.is_some_and(|ids| {
            let strings: Vec<_> = ids.iter().filter_map(Value::as_str).collect();
            strings.len() == ids.len()
                && strings.iter().copied().collect::<HashSet<_>>().len() == strings.len()
                && strings.iter().all(|id| canonical_key_id(id))
                && threshold
                    .is_some_and(|threshold| threshold >= 1 && threshold <= ids.len() as u64)
                && strings.iter().all(|id| keys.contains_key(*id))
        });
        if !valid {
            add(failures, "DELEGATION_INVALID");
        }
    }
    for (role_id, role) in &roles {
        let mut seen = HashSet::new();
        let mut current = role_id.as_str();
        while current != "root" {
            if !seen.insert(current.to_owned()) {
                add(failures, "DELEGATION_INVALID");
                break;
            }
            let Some(current_role) = roles.get(current) else {
                add(failures, "DELEGATION_INVALID");
                break;
            };
            let Some(parent) = text(field(current_role, "delegated_by")) else {
                add(failures, "DELEGATION_INVALID");
                break;
            };
            current = parent;
        }
        if role_id == "root" && !field(role, "delegated_by").is_null() {
            add(failures, "DELEGATION_INVALID");
        }
    }
    roles
}

fn add(failures: &mut BTreeSet<String>, code: &str) {
    failures.insert(code.to_owned());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Timestamp {
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
}

fn timestamp(value: &Value) -> Option<Timestamp> {
    let value = value.as_str()?;
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return None;
    }
    let part = |start: usize, end: usize| value[start..end].parse::<u16>().ok();
    let year = part(0, 4)?;
    let month = part(5, 7)? as u8;
    let day = part(8, 10)? as u8;
    let hour = part(11, 13)? as u8;
    let minute = part(14, 16)? as u8;
    let second = part(17, 19)? as u8;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if day == 0 || day > days || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some(Timestamp {
        year,
        month,
        day,
        hour,
        minute,
        second,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PreRelease {
    Numeric(u64),
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Semver {
    core: (u64, u64, u64),
    prerelease: Vec<PreRelease>,
}

fn semver(value: &Value) -> Option<Semver> {
    let value = value.as_str()?;
    let without_build = value.split_once('+').map_or(value, |(left, _)| left);
    let (core, prerelease) = without_build
        .split_once('-')
        .map_or((without_build, None), |(left, right)| (left, Some(right)));
    let core_parts: Vec<_> = core.split('.').collect();
    if core_parts.len() != 3
        || core_parts
            .iter()
            .any(|part| part.is_empty() || (part.len() > 1 && part.starts_with('0')))
    {
        return None;
    }
    let core = (
        core_parts[0].parse().ok()?,
        core_parts[1].parse().ok()?,
        core_parts[2].parse().ok()?,
    );
    let prerelease = match prerelease {
        Some(prerelease) => prerelease
            .split('.')
            .map(|item| {
                if item.is_empty()
                    || !item
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '-')
                {
                    return None;
                }
                if item.chars().all(|character| character.is_ascii_digit()) {
                    if item.len() > 1 && item.starts_with('0') {
                        return None;
                    }
                    item.parse().ok().map(PreRelease::Numeric)
                } else {
                    Some(PreRelease::Text(item.to_owned()))
                }
            })
            .collect::<Option<Vec<_>>>()?,
        None => Vec::new(),
    };
    Some(Semver { core, prerelease })
}

fn compare_semver(left: &Value, right: &Value) -> std::cmp::Ordering {
    let (Some(left), Some(right)) = (semver(left), semver(right)) else {
        return std::cmp::Ordering::Less;
    };
    let ordering = left.core.cmp(&right.core);
    if ordering != std::cmp::Ordering::Equal {
        return ordering;
    }
    if left.prerelease.is_empty() && right.prerelease.is_empty() {
        return std::cmp::Ordering::Equal;
    }
    if left.prerelease.is_empty() {
        return std::cmp::Ordering::Greater;
    }
    if right.prerelease.is_empty() {
        return std::cmp::Ordering::Less;
    }
    for (left, right) in left.prerelease.iter().zip(&right.prerelease) {
        let ordering = match (left, right) {
            (PreRelease::Numeric(left), PreRelease::Numeric(right)) => left.cmp(right),
            (PreRelease::Numeric(_), PreRelease::Text(_)) => std::cmp::Ordering::Less,
            (PreRelease::Text(_), PreRelease::Numeric(_)) => std::cmp::Ordering::Greater,
            (PreRelease::Text(left), PreRelease::Text(right)) => left.cmp(right),
        };
        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
    }
    left.prerelease.len().cmp(&right.prerelease.len())
}

fn safe_target_path(path: &Value) -> bool {
    let Some(path) = path.as_str() else {
        return false;
    };
    if !is_nfc(path)
        || path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
    {
        return false;
    }
    path.split('/')
        .all(|component| !matches!(component, "" | "." | ".."))
}

fn valid_type_uri(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|value| value.contains(':') && !value.chars().any(char::is_whitespace))
}

fn exact_fields(value: &Value, expected: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == expected.len() && expected.iter().all(|field| object.contains_key(*field))
    })
}

fn valid_slsa_resource(value: &Value) -> bool {
    if !exact_fields(value, &["uri", "digest"]) || !valid_type_uri(field(value, "uri")) {
        return false;
    }
    field(value, "digest").as_object().is_some_and(|digests| {
        !digests.is_empty()
            && digests.iter().all(|(algorithm, encoded)| {
                !algorithm.is_empty()
                    && encoded.as_str().is_some_and(|encoded| {
                        encoded.len() == 64
                            && encoded
                                .bytes()
                                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                    })
            })
    })
}

fn valid_slsa_predicate(value: &Value) -> bool {
    if !exact_fields(value, &["buildDefinition", "runDetails"]) {
        return false;
    }
    let build = field(value, "buildDefinition");
    if !exact_fields(
        build,
        &[
            "buildType",
            "externalParameters",
            "internalParameters",
            "resolvedDependencies",
        ],
    ) || !valid_type_uri(field(build, "buildType"))
        || object(field(build, "externalParameters")).is_none()
        || object(field(build, "internalParameters")).is_none()
        || !field(build, "resolvedDependencies")
            .as_array()
            .is_some_and(|items| !items.is_empty() && items.iter().all(valid_slsa_resource))
    {
        return false;
    }
    let run = field(value, "runDetails");
    if !exact_fields(run, &["builder", "metadata", "byproducts"]) {
        return false;
    }
    let builder = field(run, "builder");
    if !exact_fields(builder, &["id", "builderDependencies", "version"])
        || !valid_type_uri(field(builder, "id"))
        || !field(builder, "builderDependencies")
            .as_array()
            .is_some_and(|items| items.iter().all(valid_slsa_resource))
        || !field(builder, "version")
            .as_object()
            .is_some_and(|versions| {
                !versions.is_empty()
                    && versions.iter().all(|(name, version)| {
                        !name.is_empty() && version.as_str().is_some_and(|value| !value.is_empty())
                    })
            })
        || !field(run, "byproducts")
            .as_array()
            .is_some_and(|items| items.iter().all(valid_slsa_resource))
    {
        return false;
    }
    let metadata = field(run, "metadata");
    if !exact_fields(metadata, &["invocationId", "startedOn", "finishedOn"]) {
        return false;
    }
    let (Some(started), Some(finished)) = (
        timestamp(field(metadata, "startedOn")),
        timestamp(field(metadata, "finishedOn")),
    ) else {
        return false;
    };
    field(metadata, "invocationId")
        .as_str()
        .is_some_and(|value| !value.is_empty())
        && started <= finished
}

fn statement_failures(
    provenance: &Value,
    archive: &Value,
    target_path: &Value,
) -> BTreeSet<String> {
    let mut failures = BTreeSet::new();
    if object(provenance).is_none() {
        add(&mut failures, "PROVENANCE_STATEMENT_MISMATCH");
        add(&mut failures, "PROVENANCE_SUBJECT_MISMATCH");
        return failures;
    }
    let statement = field(provenance, "statement");
    let selected = field(provenance, "selected_subject");
    let value = field(statement, "value");
    if object(statement).is_none() || object(value).is_none() {
        add(&mut failures, "PROVENANCE_STATEMENT_MISMATCH");
        return failures;
    }
    let encoded = match canonical_json(value) {
        Ok(value) => value,
        Err(_) => {
            add(&mut failures, "PROVENANCE_STATEMENT_MISMATCH");
            return failures;
        }
    };
    if text(field(statement, "canonical_bytes_hex")) != Some(hex_encode(&encoded)).as_deref()
        || text(field(field(statement, "digest"), "value")) != Some(sha256(&encoded)).as_deref()
        || text(field(value, "_type")) != Some("https://in-toto.io/Statement/v1")
    {
        add(&mut failures, "PROVENANCE_STATEMENT_MISMATCH");
    }
    if text(field(value, "predicateType")) != Some("https://slsa.dev/provenance/v1")
        || !valid_slsa_predicate(field(value, "predicate"))
    {
        add(&mut failures, "PROVENANCE_PREDICATE_MISMATCH");
    }
    let selected_is_subject = field(value, "subject")
        .as_array()
        .is_some_and(|subjects| subjects.contains(selected));
    let archive_digest = field(field(archive, "digest"), "value");
    if !selected_is_subject
        || field(selected, "name") != target_path
        || field(field(selected, "digest"), "sha256") != archive_digest
    {
        add(&mut failures, "PROVENANCE_SUBJECT_MISMATCH");
    }
    failures
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

fn key_eligibility(
    key: &Value,
    sequence: u64,
    verification_time: Timestamp,
) -> Option<&'static str> {
    if number(field(key, "valid_from_sequence")).unwrap_or(sequence.saturating_add(1)) > sequence {
        return Some("KEY_NOT_YET_VALID");
    }
    match text(field(key, "status")) {
        Some("retired") => Some("KEY_RETIRED"),
        Some("revoked") => {
            let revocation = field(key, "revocation");
            if object(revocation).is_none()
                || number(field(revocation, "effective_sequence"))
                    .unwrap_or(sequence.saturating_add(1))
                    <= sequence
                || timestamp(field(revocation, "effective_time"))
                    .is_some_and(|time| time <= verification_time)
            {
                Some("KEY_REVOKED")
            } else {
                None
            }
        }
        _ => None,
    }
}

struct SignatureEvidence<'a> {
    role: Option<&'a Value>,
    keys: &'a KeyMap,
    message: &'a [u8],
    sequence: u64,
    verification_time: Timestamp,
    context: &'static str,
    required_key_ids: Option<HashSet<String>>,
    expected_publisher: Option<&'a str>,
    publisher_grant_authorized: bool,
}

fn signature_evidence(
    signatures: &Value,
    evidence: SignatureEvidence<'_>,
    failures: &mut BTreeSet<String>,
) -> (BTreeSet<String>, usize) {
    let mut valid_key_ids = BTreeSet::new();
    let mut valid_fingerprints = HashSet::new();
    let Some(role) = evidence.role else {
        add(failures, &format!("{}_THRESHOLD_NOT_MET", evidence.context));
        return (valid_key_ids, 0);
    };
    let Some(signatures) = signatures.as_array() else {
        add(failures, &format!("{}_THRESHOLD_NOT_MET", evidence.context));
        return (valid_key_ids, 0);
    };
    for signature in signatures {
        if object(signature).is_none() {
            add(
                failures,
                if evidence.context == "PACKAGE" {
                    "SIGNATURE_MALFORMED"
                } else if evidence.context == "ROOT" {
                    "ROOT_SIGNATURE_INVALID"
                } else {
                    "INDEX_SIGNATURE_INVALID"
                },
            );
            continue;
        }
        if text(field(signature, "algorithm")) != Some("ed25519")
            || text(field(signature, "encoding")) != Some("lowercase-hex")
        {
            add(
                failures,
                if evidence.context == "PACKAGE" {
                    "SIGNATURE_MALFORMED"
                } else if evidence.context == "ROOT" {
                    "ROOT_SIGNATURE_INVALID"
                } else {
                    "INDEX_SIGNATURE_INVALID"
                },
            );
            continue;
        }
        let Some(key_id) = text(field(signature, "key_id")) else {
            add(
                failures,
                if evidence.context == "PACKAGE" {
                    "KEY_UNKNOWN"
                } else if evidence.context == "ROOT" {
                    "ROOT_SIGNATURE_INVALID"
                } else {
                    "INDEX_SIGNATURE_INVALID"
                },
            );
            continue;
        };
        if !canonical_key_id(key_id) {
            add(
                failures,
                if evidence.context == "PACKAGE" {
                    "KEY_ID_MISMATCH"
                } else if evidence.context == "ROOT" {
                    "ROOT_SIGNATURE_INVALID"
                } else {
                    "INDEX_SIGNATURE_INVALID"
                },
            );
            continue;
        }
        let Some(key) = evidence.keys.get(key_id) else {
            add(
                failures,
                if evidence.context == "PACKAGE" {
                    "KEY_UNKNOWN"
                } else if evidence.context == "ROOT" {
                    "ROOT_SIGNATURE_INVALID"
                } else {
                    "INDEX_SIGNATURE_INVALID"
                },
            );
            continue;
        };
        let material = field(key, "key_material");
        if canonical_public_key(material).is_none() {
            add(
                failures,
                if evidence.context == "PACKAGE" {
                    "KEY_MALFORMED"
                } else if evidence.context == "ROOT" {
                    "ROOT_SIGNATURE_INVALID"
                } else {
                    "INDEX_SIGNATURE_INVALID"
                },
            );
            continue;
        }
        if derived_key_id(material).as_deref() != Some(key_id) {
            add(
                failures,
                if evidence.context == "PACKAGE" {
                    "KEY_ID_MISMATCH"
                } else if evidence.context == "ROOT" {
                    "ROOT_SIGNATURE_INVALID"
                } else {
                    "INDEX_SIGNATURE_INVALID"
                },
            );
            continue;
        }
        let role_contains = field(role, "key_ids")
            .as_array()
            .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(key_id)));
        if !role_contains {
            add(
                failures,
                if evidence.context == "PACKAGE" {
                    "DELEGATION_INVALID"
                } else if evidence.context == "ROOT" {
                    "ROOT_SIGNATURE_INVALID"
                } else {
                    "INDEX_SIGNATURE_INVALID"
                },
            );
            continue;
        }
        if evidence.context == "PACKAGE"
            && (!evidence.publisher_grant_authorized
                || text(field(key, "publisher_identity")) != evidence.expected_publisher)
        {
            add(failures, "SIGNER_PUBLISHER_MISMATCH");
            continue;
        }
        if let Some(eligibility) =
            key_eligibility(key, evidence.sequence, evidence.verification_time)
        {
            add(
                failures,
                if evidence.context == "PACKAGE" {
                    eligibility
                } else if evidence.context == "ROOT" {
                    "ROOT_SIGNATURE_INVALID"
                } else {
                    "INDEX_SIGNATURE_INVALID"
                },
            );
            continue;
        }
        let status = signature_status(
            field(material, "public_key"),
            evidence.message,
            field(signature, "value"),
        );
        if let Some(status) = status {
            add(
                failures,
                match (evidence.context, status) {
                    ("PACKAGE", SignatureFailure::KeyMalformed) => "KEY_MALFORMED",
                    ("PACKAGE", SignatureFailure::SignatureMalformed) => "SIGNATURE_MALFORMED",
                    ("PACKAGE", SignatureFailure::SignatureInvalid) => "SIGNATURE_INVALID",
                    ("ROOT", _) => "ROOT_SIGNATURE_INVALID",
                    _ => "INDEX_SIGNATURE_INVALID",
                },
            );
            continue;
        }
        let Some(fingerprint) = derived_key_id(material) else {
            add(
                failures,
                if evidence.context == "PACKAGE" {
                    "KEY_MALFORMED"
                } else if evidence.context == "ROOT" {
                    "ROOT_SIGNATURE_INVALID"
                } else {
                    "INDEX_SIGNATURE_INVALID"
                },
            );
            continue;
        };
        valid_key_ids.insert(key_id.to_owned());
        valid_fingerprints.insert(fingerprint);
    }
    let threshold = number(field(role, "threshold")).unwrap_or(1);
    if valid_fingerprints.len() < threshold as usize
        || evidence
            .required_key_ids
            .as_ref()
            .is_some_and(|required| !required.iter().all(|id| valid_key_ids.contains(id)))
    {
        add(failures, &format!("{}_THRESHOLD_NOT_MET", evidence.context));
    }
    (valid_key_ids, valid_fingerprints.len())
}

fn max_timestamp() -> Timestamp {
    Timestamp {
        year: u16::MAX,
        month: 12,
        day: 31,
        hour: 23,
        minute: 59,
        second: 59,
    }
}

fn transcript_matches(envelope: &Value, raw: &[u8]) -> bool {
    let transcript = field(envelope, "transcript");
    text(field(transcript, "bytes_hex")) == Some(hex_encode(raw)).as_deref()
        && text(field(transcript, "sha256")) == Some(sha256(raw)).as_deref()
}

fn optional_string(value: &Value) -> Option<String> {
    value.as_str().map(ToOwned::to_owned)
}

fn contains_text(array: &Value, expected: Option<&str>) -> bool {
    expected.is_some_and(|expected| {
        array
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(expected)))
    })
}

struct RootValidation {
    verification_time: Timestamp,
    candidate_keys: KeyMap,
    fingerprints: HashMap<String, String>,
    candidate_roles: RoleMap,
    old_version: Option<u64>,
    candidate_version: Option<u64>,
    candidate_sequence: Option<u64>,
}

fn validate_trust_root_transition(
    roots: &TrustRootsEnvelope,
    expectation: &VerificationExpectation,
    failures: &mut BTreeSet<String>,
) -> RootValidation {
    let verification_time =
        timestamp(field(expectation, "verification_time")).unwrap_or_else(max_timestamp);
    let trusted_state = field(expectation, "trusted_state");
    let offline = field(expectation, "offline_lock");
    let trusted_root = field(roots, "trusted_root");
    let candidate_root = field(roots, "candidate_root");
    let transition = field(roots, "transition");
    let old_signed = field(trusted_root, "signed");
    let candidate_signed = field(candidate_root, "signed");
    let (old_keys, _) = key_maps(old_signed, failures);
    let (candidate_keys, fingerprints) = key_maps(candidate_signed, failures);
    validate_key_supersession(&old_keys, failures);
    validate_key_supersession(&candidate_keys, failures);
    let old_roles = role_maps(old_signed, &old_keys, failures);
    let candidate_roles = role_maps(candidate_signed, &candidate_keys, failures);

    let old_raw = match metadata_transcript(ROOT_DOMAIN, old_signed) {
        Ok(value) => value,
        Err(_) => {
            add(failures, "ROOT_DIGEST_MISMATCH");
            Vec::new()
        }
    };
    let candidate_raw = match metadata_transcript(ROOT_DOMAIN, candidate_signed) {
        Ok(value) => value,
        Err(_) => {
            add(failures, "ROOT_DIGEST_MISMATCH");
            Vec::new()
        }
    };
    if !transcript_matches(trusted_root, &old_raw)
        || !transcript_matches(candidate_root, &candidate_raw)
    {
        add(failures, "ROOT_DIGEST_MISMATCH");
    }

    let old_version = number(field(old_signed, "root_version"));
    let old_sequence = number(field(old_signed, "sequence"));
    let candidate_version = number(field(candidate_signed, "root_version"));
    let candidate_sequence = number(field(candidate_signed, "sequence"));
    let bootstrap_matches =
        object(field(trusted_state, "trusted_root_anchor")).is_some_and(|anchor| {
            anchor.len() == 3
                && number(anchor.get("root_version").unwrap_or(&Value::Null)) == old_version
                && number(anchor.get("root_sequence").unwrap_or(&Value::Null)) == old_sequence
                && text(anchor.get("root_transcript_sha256").unwrap_or(&Value::Null))
                    == Some(sha256(&old_raw)).as_deref()
        });
    if !bootstrap_matches {
        add(failures, "ROOT_BOOTSTRAP_MISMATCH");
    }
    let old_issued = timestamp(field(old_signed, "issued_at"));
    let old_expiry = timestamp(field(old_signed, "expires_at"));
    let candidate_issued = timestamp(field(candidate_signed, "issued_at"));
    let candidate_expiry = timestamp(field(candidate_signed, "expires_at"));
    let valid_rotation = matches!(
        (
            old_version,
            old_sequence,
            candidate_version,
            candidate_sequence,
            old_issued,
            old_expiry,
            candidate_issued,
            candidate_expiry,
        ),
        (
            Some(old_version),
            Some(old_sequence),
            Some(candidate_version),
            Some(candidate_sequence),
            Some(old_issued),
            Some(old_expiry),
            Some(candidate_issued),
            Some(candidate_expiry),
        ) if old_version.checked_add(1) == Some(candidate_version)
            && candidate_sequence > old_sequence
            && number(field(transition, "from_version")) == Some(old_version)
            && number(field(transition, "to_version")) == Some(candidate_version)
            && old_issued <= candidate_issued
            && candidate_issued < old_expiry
            && candidate_issued < candidate_expiry
            && candidate_issued <= verification_time
    );
    if !valid_rotation {
        add(failures, "ROOT_ROTATION_INVALID");
    }
    if old_expiry
        .zip(candidate_issued)
        .is_some_and(|(expiry, issued)| expiry <= issued)
        || candidate_expiry.is_some_and(|expiry| expiry <= verification_time)
    {
        add(failures, "METADATA_EXPIRED");
    }

    let old_root_role = old_roles.get("root");
    let new_root_role = candidate_roles.get("root");
    signature_evidence(
        field(trusted_root, "signatures"),
        SignatureEvidence {
            role: old_root_role,
            keys: &old_keys,
            message: &old_raw,
            sequence: old_sequence.unwrap_or(0),
            verification_time,
            context: "ROOT",
            required_key_ids: None,
            expected_publisher: None,
            publisher_grant_authorized: true,
        },
        failures,
    );
    let (old_valid, _) = signature_evidence(
        field(transition, "candidate_signatures_by_old_root"),
        SignatureEvidence {
            role: old_root_role,
            keys: &old_keys,
            message: &candidate_raw,
            sequence: candidate_sequence.unwrap_or(0),
            verification_time,
            context: "ROOT",
            required_key_ids: None,
            expected_publisher: None,
            publisher_grant_authorized: true,
        },
        failures,
    );
    let (new_valid, _) = signature_evidence(
        field(transition, "candidate_signatures_by_new_root"),
        SignatureEvidence {
            role: new_root_role,
            keys: &candidate_keys,
            message: &candidate_raw,
            sequence: candidate_sequence.unwrap_or(0),
            verification_time,
            context: "ROOT",
            required_key_ids: None,
            expected_publisher: None,
            publisher_grant_authorized: true,
        },
        failures,
    );
    let (candidate_valid, _) = signature_evidence(
        field(candidate_root, "signatures"),
        SignatureEvidence {
            role: new_root_role,
            keys: &candidate_keys,
            message: &candidate_raw,
            sequence: candidate_sequence.unwrap_or(0),
            verification_time,
            context: "ROOT",
            required_key_ids: None,
            expected_publisher: None,
            publisher_grant_authorized: true,
        },
        failures,
    );
    if new_valid != candidate_valid || old_valid.is_empty() || new_valid.is_empty() {
        add(failures, "ROOT_THRESHOLD_NOT_MET");
    }

    if candidate_version.is_some_and(|candidate| {
        candidate
            < number(field(trusted_state, "highest_root_version"))
                .unwrap_or(candidate)
                .max(number(field(offline, "root_version")).unwrap_or(candidate))
    }) || candidate_sequence.is_some_and(|candidate| {
        candidate
            < number(field(trusted_state, "highest_root_sequence"))
                .unwrap_or(candidate)
                .max(number(field(offline, "root_sequence")).unwrap_or(candidate))
    }) {
        add(failures, "ROOT_ROLLBACK");
    }

    RootValidation {
        verification_time,
        candidate_keys,
        fingerprints,
        candidate_roles,
        old_version,
        candidate_version,
        candidate_sequence,
    }
}

fn ordered_reason_codes(failures: &BTreeSet<String>) -> Vec<String> {
    REASON_PRECEDENCE
        .iter()
        .filter(|reason| failures.contains(**reason))
        .map(|reason| (*reason).to_owned())
        .collect()
}

fn eligible_signing_role(
    validation: &RootValidation,
    role_id: Option<&str>,
    expected_threshold: Option<u64>,
    threshold_failure: &str,
    failures: &mut BTreeSet<String>,
) -> Vec<String> {
    let role = role_id.and_then(|role_id| validation.candidate_roles.get(role_id));
    if role.and_then(|role| number(field(role, "threshold"))) != expected_threshold {
        add(failures, threshold_failure);
    }
    let sequence = validation.candidate_sequence.unwrap_or(0);
    let mut eligible_key_ids = role
        .into_iter()
        .flat_map(|role| field(role, "key_ids").as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .filter(|key_id| {
            validation.candidate_keys.get(*key_id).is_some_and(|key| {
                key_eligibility(key, sequence, validation.verification_time).is_none()
            })
        })
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    eligible_key_ids.sort();
    eligible_key_ids.dedup();
    if expected_threshold.is_none_or(|threshold| eligible_key_ids.len() < threshold as usize) {
        add(failures, threshold_failure);
    }
    eligible_key_ids
}

/// Authenticate the root anchor, transition, candidate root, rollback state,
/// and registry-index signing authority without invoking a signing provider.
pub fn preflight_registry_index_trust(
    roots: &TrustRootsEnvelope,
    expectation: &VerificationExpectation,
) -> Result<RegistryIndexTrustPreflight, TrustRootPreflightError> {
    let mut failures = BTreeSet::new();
    if !validate_document_work_budget(roots, DocumentKind::Roots) {
        add(&mut failures, "ROOT_DIGEST_MISMATCH");
    }
    if !validate_document_work_budget(expectation, DocumentKind::Expectation) {
        add(&mut failures, "OFFLINE_INPUT_MISSING");
    }
    if failures.is_empty()
        && validate_schema_value(
            roots,
            include_bytes!("../../../schemas/axiom-trust-roots-v1.schema.json"),
            &TRUST_ROOTS_SCHEMA,
            "trust roots",
        )
        .is_err()
    {
        add(&mut failures, "ROOT_DIGEST_MISMATCH");
    }
    if failures.is_empty()
        && validate_schema_value(
            expectation,
            include_bytes!(
                "../../../schemas/axiom-package-verification-expectation-v1.schema.json"
            ),
            &VERIFICATION_EXPECTATION_SCHEMA,
            "verification expectation",
        )
        .is_err()
    {
        add(&mut failures, "OFFLINE_INPUT_MISSING");
    }
    if !failures.is_empty() {
        return Err(TrustRootPreflightError {
            reason_codes: ordered_reason_codes(&failures),
        });
    }

    let validation = validate_trust_root_transition(roots, expectation, &mut failures);
    let required_signers = field(expectation, "required_signers");
    let index_role_id = text(field(required_signers, "index_role_id"));
    let index_threshold = number(field(required_signers, "index_threshold"));
    let index_eligible_key_ids = eligible_signing_role(
        &validation,
        index_role_id,
        index_threshold,
        "INDEX_THRESHOLD_NOT_MET",
        &mut failures,
    );
    let package_role_id = text(field(required_signers, "package_role_id"));
    let package_threshold = number(field(required_signers, "package_threshold"));
    let package_eligible_key_ids = eligible_signing_role(
        &validation,
        package_role_id,
        package_threshold,
        "PACKAGE_THRESHOLD_NOT_MET",
        &mut failures,
    );
    if !failures.is_empty() {
        return Err(TrustRootPreflightError {
            reason_codes: ordered_reason_codes(&failures),
        });
    }

    Ok(RegistryIndexTrustPreflight {
        root_version: validation.candidate_version.unwrap_or(0),
        root_sequence: validation.candidate_sequence.unwrap_or(0),
        index_role_id: index_role_id.unwrap_or_default().to_owned(),
        index_threshold: index_threshold.unwrap_or(0),
        index_eligible_key_ids,
        package_role_id: package_role_id.unwrap_or_default().to_owned(),
        package_threshold: package_threshold.unwrap_or(0),
        package_eligible_key_ids,
    })
}

/// Validate canonical in-toto/SLSA semantics before invoking a package signer.
///
/// The envelope may still be unsigned; this checks the selected subject,
/// archive binding, canonical statement digest, supported predicate shape, and
/// monotonic build timestamps using the same normative verifier routine.
pub fn validate_package_provenance_semantics(
    package: &PackageSignatureEnvelope,
) -> Result<(), PackageReleasePreflightError> {
    let archive = serde_json::json!({
        "length": field(field(package, "archive"), "size"),
        "digest": field(field(package, "archive"), "digest"),
    });
    let failures = statement_failures(
        field(package, "provenance"),
        &archive,
        field(field(package, "package"), "target_path"),
    );
    if failures.is_empty() {
        Ok(())
    } else {
        Err(PackageReleasePreflightError {
            reason_codes: ordered_reason_codes(&failures),
        })
    }
}

/// Authenticate one signed package and its exact captured artifacts before a
/// registry index exists or any registry-index signer is invoked.
///
/// Registry-index signatures, snapshot replay, and current offline index pins
/// are intentionally outside this preflight.
pub fn preflight_package_release_trust(
    package: &PackageSignatureEnvelope,
    roots: &TrustRootsEnvelope,
    expectation: &VerificationExpectation,
    artifacts: PackageArtifacts<'_>,
    expected_index_generation: u64,
    expected_index_sequence: u64,
) -> Result<PackageReleaseTrustPreflight, PackageReleasePreflightError> {
    let mut failures = BTreeSet::new();
    if !validate_document_work_budget(package, DocumentKind::Package)
        || validate_schema_value(
            package,
            include_bytes!("../../../schemas/axiom-package-signature-v1.schema.json"),
            &PACKAGE_SIGNATURE_SCHEMA,
            "package signature",
        )
        .is_err()
    {
        add(&mut failures, "SIGNATURE_MALFORMED");
    }
    if !failures.is_empty() {
        return Err(PackageReleasePreflightError {
            reason_codes: ordered_reason_codes(&failures),
        });
    }
    if let Err(error) = preflight_registry_index_trust(roots, expectation) {
        return Err(PackageReleasePreflightError {
            reason_codes: error.reason_codes,
        });
    }

    let verification_time =
        timestamp(field(expectation, "verification_time")).unwrap_or_else(max_timestamp);
    let request = field(expectation, "request");
    let required_signers = field(expectation, "required_signers");
    let candidate_signed = field(field(roots, "candidate_root"), "signed");
    let candidate_sequence = number(field(candidate_signed, "sequence")).unwrap_or(0);
    let (candidate_keys, _) = key_maps(candidate_signed, &mut failures);
    let candidate_roles = role_maps(candidate_signed, &candidate_keys, &mut failures);
    let package_role_id = text(field(required_signers, "package_role_id"));
    let package_role = package_role_id.and_then(|role| candidate_roles.get(role));
    let package_threshold = number(field(required_signers, "package_threshold")).unwrap_or(0);
    if package_role.and_then(|role| number(field(role, "threshold"))) != Some(package_threshold) {
        add(&mut failures, "PACKAGE_THRESHOLD_NOT_MET");
    }

    let package_path = field(field(package, "package"), "target_path");
    if !safe_target_path(package_path) {
        add(&mut failures, "TARGET_PATH_INVALID");
    }
    if compare_semver(
        field(field(package, "package"), "version"),
        field(
            field(expectation, "trusted_state"),
            "minimum_package_version",
        ),
    ) == std::cmp::Ordering::Less
    {
        add(&mut failures, "VERSION_DOWNGRADE");
    }
    let exact_grant = field(candidate_signed, "namespace_grants")
        .as_array()
        .is_some_and(|grants| {
            grants.iter().any(|grant| {
                field(grant, "publisher_identity") == field(request, "publisher_identity")
                    && field(grant, "namespace") == field(request, "namespace")
                    && contains_text(field(grant, "package_names"), text(field(request, "name")))
                    && contains_text(
                        field(grant, "registry_identities"),
                        text(field(request, "registry_identity")),
                    )
                    && contains_text(
                        field(grant, "source_identities"),
                        text(field(request, "source_identity")),
                    )
                    && text(field(grant, "role_id")) == package_role_id
            })
        });
    if !exact_grant {
        add(&mut failures, "NAMESPACE_GRANT_MISMATCH");
    }

    let package_raw = match package_transcript(package, package_threshold) {
        Ok(value) => value,
        Err(_) => {
            add(&mut failures, "SIGNATURE_INVALID");
            Vec::new()
        }
    };
    let field_order_matches = field(field(package, "transcript"), "field_order")
        .as_array()
        .is_some_and(|items| {
            items.len() == PACKAGE_FIELDS.len()
                && items
                    .iter()
                    .zip(PACKAGE_FIELDS)
                    .all(|(item, expected)| item.as_str() == Some(expected))
        });
    if !field_order_matches || !transcript_matches(package, &package_raw) {
        add(&mut failures, "SIGNATURE_INVALID");
    }
    let required_key_ids = field(required_signers, "required_key_ids")
        .as_array()
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    let (valid_signers, valid_signer_count) = signature_evidence(
        field(package, "signatures"),
        SignatureEvidence {
            role: package_role,
            keys: &candidate_keys,
            message: &package_raw,
            sequence: candidate_sequence,
            verification_time,
            context: "PACKAGE",
            required_key_ids: Some(required_key_ids.clone()),
            expected_publisher: text(field(request, "publisher_identity")),
            publisher_grant_authorized: exact_grant,
        },
        &mut failures,
    );
    if valid_signer_count < package_threshold as usize
        || !required_key_ids
            .iter()
            .all(|key_id| valid_signers.contains(key_id))
    {
        add(&mut failures, "PACKAGE_THRESHOLD_NOT_MET");
    }
    if !package_index_floor_is_satisfied(
        package,
        expected_index_generation,
        expected_index_sequence,
    ) {
        add(&mut failures, "OFFLINE_LOCK_MISMATCH");
    }

    let package_archive = serde_json::json!({
        "length": field(field(package, "archive"), "size"),
        "digest": field(field(package, "archive"), "digest"),
    });
    if package_archive != *field(request, "archive") {
        add(&mut failures, "ARCHIVE_DIGEST_MISMATCH");
    }
    if field(package, "manifest") != field(request, "manifest") {
        add(&mut failures, "MANIFEST_DIGEST_MISMATCH");
    }
    failures.extend(statement_failures(
        field(package, "provenance"),
        &package_archive,
        package_path,
    ));
    let request_provenance = field(request, "provenance");
    if field(field(field(package, "provenance"), "statement"), "digest")
        != field(field(request_provenance, "statement"), "digest")
    {
        add(&mut failures, "PROVENANCE_STATEMENT_MISMATCH");
    }
    if field(
        field(field(field(package, "provenance"), "statement"), "value"),
        "predicateType",
    ) != field(
        field(field(request_provenance, "statement"), "value"),
        "predicateType",
    ) {
        add(&mut failures, "PROVENANCE_PREDICATE_MISMATCH");
    }
    if field(field(package, "provenance"), "selected_subject")
        != field(request_provenance, "selected_subject")
    {
        add(&mut failures, "PROVENANCE_SUBJECT_MISMATCH");
    }
    if field(field(package, "publisher"), "publisher_identity")
        != field(request, "publisher_identity")
    {
        add(&mut failures, "PUBLISHER_MISMATCH");
    }
    if field(field(package, "package"), "namespace") != field(request, "namespace") {
        add(&mut failures, "NAMESPACE_MISMATCH");
    }
    if field(field(package, "package"), "name") != field(request, "name") {
        add(&mut failures, "PACKAGE_NAME_MISMATCH");
    }
    if field(field(package, "package"), "version") != field(request, "version") {
        add(&mut failures, "PACKAGE_VERSION_MISMATCH");
    }
    if field(field(package, "registry"), "registry_identity") != field(request, "registry_identity")
        || field(field(package, "registry"), "source_identity") != field(request, "source_identity")
    {
        add(&mut failures, "SOURCE_MISMATCH");
    }
    if package_path != field(request, "target_path") {
        add(&mut failures, "TARGET_PATH_MISMATCH");
    }

    match artifacts.archive {
        Some(bytes)
            if number(field(field(package, "archive"), "size")) == Some(bytes.len() as u64)
                && text(field(
                    field(field(package, "archive"), "digest"),
                    "algorithm",
                )) == Some("sha-256")
                && text(field(field(field(package, "archive"), "digest"), "value"))
                    == Some(sha256(bytes)).as_deref() => {}
        Some(_) => add(&mut failures, "ARCHIVE_DIGEST_MISMATCH"),
        None => add(&mut failures, "OFFLINE_INPUT_MISSING"),
    }
    match artifacts.manifest {
        Some(bytes)
            if text(field(field(package, "manifest"), "algorithm")) == Some("sha-256")
                && text(field(field(package, "manifest"), "value"))
                    == Some(sha256(bytes)).as_deref() => {}
        Some(_) => add(&mut failures, "MANIFEST_DIGEST_MISMATCH"),
        None => add(&mut failures, "OFFLINE_INPUT_MISSING"),
    }
    match artifacts.provenance {
        Some(bytes) => {
            let statement = field(field(package, "provenance"), "statement");
            let matches = canonical_json(field(statement, "value")).is_ok_and(|canonical| {
                bytes == canonical
                    && text(field(statement, "canonical_bytes_hex"))
                        == Some(hex_encode(bytes)).as_deref()
                    && text(field(field(statement, "digest"), "algorithm")) == Some("sha-256")
                    && text(field(field(statement, "digest"), "value"))
                        == Some(sha256(bytes)).as_deref()
            });
            if !matches {
                add(&mut failures, "PROVENANCE_STATEMENT_MISMATCH");
            }
        }
        None => add(&mut failures, "OFFLINE_INPUT_MISSING"),
    }

    if !failures.is_empty() {
        return Err(PackageReleasePreflightError {
            reason_codes: ordered_reason_codes(&failures),
        });
    }
    let mut valid_signer_key_ids = valid_signers.into_iter().collect::<Vec<_>>();
    valid_signer_key_ids.sort();
    Ok(PackageReleaseTrustPreflight {
        package_threshold,
        valid_signer_key_ids,
    })
}

/// Evaluate authenticated metadata for contract-oracle compatibility.
///
/// This metadata-only entry point cannot admit package consumption because it
/// does not observe artifact bytes. Registry and CLI consume paths must call
/// [`verify_package_with_artifacts`].
pub(crate) fn verify_package(input: &PackageTrustInput) -> PackageVerification {
    if let Some(failure) = input_work_budget_failure(input) {
        return input_failure_verification(failure);
    }
    let mut failures = BTreeSet::new();
    let expectation = &input.verification_expectation;
    let package = &input.package_signature;
    let index = &input.registry_index;
    let roots = &input.trust_roots;
    if object(expectation).is_none()
        || object(package).is_none()
        || object(index).is_none()
        || object(roots).is_none()
    {
        return offline_input_missing_verification();
    }
    let request = field(expectation, "request");
    let required_signers = field(expectation, "required_signers");
    let trusted_state = field(expectation, "trusted_state");
    let offline = field(expectation, "offline_lock");

    let RootValidation {
        verification_time,
        candidate_keys,
        fingerprints,
        candidate_roles,
        old_version,
        candidate_version,
        candidate_sequence,
    } = validate_trust_root_transition(roots, expectation, &mut failures);
    let candidate_root = field(roots, "candidate_root");
    let candidate_signed = field(candidate_root, "signed");

    let index_signed = field(index, "signed");
    let index_raw = match metadata_transcript(INDEX_DOMAIN, index_signed) {
        Ok(value) => value,
        Err(_) => {
            add(&mut failures, "INDEX_DIGEST_MISMATCH");
            Vec::new()
        }
    };
    let index_transcript_matches = transcript_matches(index, &index_raw);
    if !index_transcript_matches {
        add(&mut failures, "INDEX_DIGEST_MISMATCH");
    }
    if timestamp(field(index_signed, "expires_at")).is_none_or(|expiry| expiry <= verification_time)
        || timestamp(field(index_signed, "issued_at"))
            .is_none_or(|issued| issued > verification_time)
    {
        add(&mut failures, "METADATA_EXPIRED");
    }
    let generation = number(field(index_signed, "generation")).unwrap_or(0);
    let sequence = number(field(index_signed, "sequence")).unwrap_or(0);
    let index_role_id = text(field(required_signers, "index_role_id"));
    let signed_index_role_id = text(field(index_signed, "signature_role"));
    if signed_index_role_id != index_role_id {
        add(&mut failures, "INDEX_THRESHOLD_NOT_MET");
    }
    let index_role = signed_index_role_id
        .filter(|role| Some(*role) == index_role_id)
        .and_then(|role| candidate_roles.get(role));
    if field(index_signed, "registry_identity") != field(request, "registry_identity")
        || field(index_signed, "source_identity") != field(request, "source_identity")
        || field(index_signed, "registry_identity")
            != field(field(package, "registry"), "registry_identity")
        || field(index_signed, "source_identity")
            != field(field(package, "registry"), "source_identity")
    {
        add(&mut failures, "SOURCE_MISMATCH");
    }
    let (index_valid, index_valid_count) = signature_evidence(
        field(index, "signatures"),
        SignatureEvidence {
            role: index_role,
            keys: &candidate_keys,
            message: &index_raw,
            sequence,
            verification_time,
            context: "INDEX",
            required_key_ids: None,
            expected_publisher: None,
            publisher_grant_authorized: true,
        },
        &mut failures,
    );
    let expected_index_threshold = number(field(required_signers, "index_threshold"));
    if index_role.and_then(|role| number(field(role, "threshold"))) != expected_index_threshold {
        add(&mut failures, "INDEX_THRESHOLD_NOT_MET");
    }
    let index_authenticated = index_transcript_matches
        && index_role.is_some()
        && index_role.and_then(|role| number(field(role, "threshold"))) == expected_index_threshold
        && index_role
            .and_then(|role| number(field(role, "threshold")))
            .is_some_and(|threshold| index_valid_count >= threshold as usize);
    let _ = index_valid;
    if index_authenticated {
        let highest_generation =
            number(field(trusted_state, "highest_index_generation")).unwrap_or(generation);
        let highest_sequence =
            number(field(trusted_state, "highest_index_sequence")).unwrap_or(sequence);
        if generation < highest_generation || sequence < highest_sequence {
            add(&mut failures, "ROLLBACK_DETECTED");
        }
        let seen_snapshots = field(trusted_state, "seen_snapshots").as_array();
        if seen_snapshots.is_none() {
            add(&mut failures, "OFFLINE_INPUT_MISSING");
        }
        let empty = Vec::new();
        let seen_snapshots = seen_snapshots.unwrap_or(&empty);
        let snapshot_state = serde_json::json!({
            "generation": generation,
            "sequence": sequence,
            "snapshot_id": field(field(index_signed, "consistent_snapshot"), "snapshot_id"),
            "index_transcript_sha256": sha256(&index_raw),
        });
        let exact_repeat = seen_snapshots.contains(&snapshot_state);
        let rebound = seen_snapshots.iter().any(|seen| {
            object(seen).is_some()
                && seen != &snapshot_state
                && ((number(field(seen, "generation")) == Some(generation)
                    && number(field(seen, "sequence")) == Some(sequence))
                    || field(seen, "snapshot_id") == field(&snapshot_state, "snapshot_id")
                    || field(seen, "index_transcript_sha256")
                        == field(&snapshot_state, "index_transcript_sha256"))
        });
        let highest_position_seen = seen_snapshots.iter().any(|seen| {
            number(field(seen, "generation")) == Some(highest_generation)
                && number(field(seen, "sequence")) == Some(highest_sequence)
        });
        if !highest_position_seen {
            add(&mut failures, "OFFLINE_INPUT_MISSING");
        }
        if rebound
            || (generation == highest_generation
                && sequence == highest_sequence
                && highest_position_seen
                && !exact_repeat)
        {
            add(&mut failures, "METADATA_REPLAYED");
        }
    }

    let releases = field(index_signed, "releases")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut release_tuples = Vec::new();
    let mut release_coordinates = Vec::new();
    let mut release_target_paths = Vec::new();
    for release in &releases {
        if object(release).is_none() {
            continue;
        }
        let coordinate = serde_json::json!([
            field(release, "registry_identity"),
            field(release, "source_identity"),
            field(release, "namespace"),
            field(release, "name"),
            field(release, "version"),
        ]);
        let full = serde_json::json!([
            field(release, "registry_identity"),
            field(release, "source_identity"),
            field(release, "namespace"),
            field(release, "name"),
            field(release, "version"),
            field(release, "target_path"),
        ]);
        release_coordinates.push(canonical_json(&coordinate).unwrap_or_default());
        release_tuples.push(canonical_json(&full).unwrap_or_default());
        release_target_paths.push(field(release, "target_path").clone());
    }
    if release_tuples.iter().collect::<HashSet<_>>().len() != release_tuples.len() {
        add(&mut failures, "DUPLICATE_RELEASE");
    }
    if release_target_paths.iter().collect::<HashSet<_>>().len() != release_target_paths.len() {
        add(&mut failures, "DUPLICATE_TARGET_PATH");
    }
    if release_coordinates.iter().collect::<HashSet<_>>().len() != release_coordinates.len() {
        add(&mut failures, "DUPLICATE_PACKAGE_COORDINATE");
    }
    let selected_release = releases.iter().find(|release| {
        [
            "registry_identity",
            "source_identity",
            "namespace",
            "name",
            "version",
            "target_path",
        ]
        .iter()
        .all(|name| field(release, name) == field(request, name))
    });
    if selected_release.is_none() {
        add(&mut failures, "OFFLINE_INPUT_MISSING");
    } else if selected_release.is_some_and(|release| {
        field(release, "registry_identity") != field(index_signed, "registry_identity")
            || field(release, "source_identity") != field(index_signed, "source_identity")
    }) {
        add(&mut failures, "SOURCE_MISMATCH");
    }

    let package_path = field(field(package, "package"), "target_path");
    if !safe_target_path(package_path) {
        add(&mut failures, "TARGET_PATH_INVALID");
    }
    if compare_semver(
        field(field(package, "package"), "version"),
        field(trusted_state, "minimum_package_version"),
    ) == std::cmp::Ordering::Less
    {
        add(&mut failures, "VERSION_DOWNGRADE");
    }

    let package_role_id = text(field(required_signers, "package_role_id"));
    let exact_grant = field(candidate_signed, "namespace_grants")
        .as_array()
        .is_some_and(|grants| {
            grants.iter().any(|grant| {
                field(grant, "publisher_identity") == field(request, "publisher_identity")
                    && field(grant, "namespace") == field(request, "namespace")
                    && contains_text(field(grant, "package_names"), text(field(request, "name")))
                    && contains_text(
                        field(grant, "registry_identities"),
                        text(field(request, "registry_identity")),
                    )
                    && contains_text(
                        field(grant, "source_identities"),
                        text(field(request, "source_identity")),
                    )
                    && text(field(grant, "role_id")) == package_role_id
            })
        });
    if !exact_grant {
        add(&mut failures, "NAMESPACE_GRANT_MISMATCH");
    }

    let package_threshold = number(field(required_signers, "package_threshold")).unwrap_or(1);
    let package_raw = match package_transcript(package, package_threshold) {
        Ok(value) => value,
        Err(_) => {
            add(&mut failures, "SIGNATURE_INVALID");
            Vec::new()
        }
    };
    let package_transcript_value = field(package, "transcript");
    let field_order_matches = field(package_transcript_value, "field_order")
        .as_array()
        .is_some_and(|items| {
            items.len() == PACKAGE_FIELDS.len()
                && items
                    .iter()
                    .zip(PACKAGE_FIELDS)
                    .all(|(item, expected)| item.as_str() == Some(expected))
        });
    let package_transcript_matches =
        field_order_matches && transcript_matches(package, &package_raw);
    if !package_transcript_matches {
        add(&mut failures, "SIGNATURE_INVALID");
    }
    let package_role = package_role_id.and_then(|role| candidate_roles.get(role));
    let required_key_ids = field(required_signers, "required_key_ids")
        .as_array()
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    let (package_valid, package_valid_count) = signature_evidence(
        field(package, "signatures"),
        SignatureEvidence {
            role: package_role,
            keys: &candidate_keys,
            message: &package_raw,
            sequence,
            verification_time,
            context: "PACKAGE",
            required_key_ids: Some(required_key_ids.clone()),
            expected_publisher: text(field(request, "publisher_identity")),
            publisher_grant_authorized: exact_grant,
        },
        &mut failures,
    );
    if package_role.and_then(|role| number(field(role, "threshold"))) != Some(package_threshold) {
        add(&mut failures, "PACKAGE_THRESHOLD_NOT_MET");
    }
    let package_authenticated = package_transcript_matches
        && package_role.and_then(|role| number(field(role, "threshold")))
            == Some(package_threshold)
        && package_valid_count >= package_threshold as usize
        && required_key_ids
            .iter()
            .all(|key_id| package_valid.contains(key_id));
    let package_index_matches = package_index_floor_is_satisfied(package, generation, sequence);
    if !package_index_matches {
        add(&mut failures, "OFFLINE_LOCK_MISMATCH");
        if index_authenticated && package_authenticated {
            add(&mut failures, "METADATA_REPLAYED");
        }
    }

    let package_archive = serde_json::json!({
        "length": field(field(package, "archive"), "size"),
        "digest": field(field(package, "archive"), "digest"),
    });
    if package_archive != *field(request, "archive") {
        add(&mut failures, "ARCHIVE_DIGEST_MISMATCH");
    }
    if field(package, "manifest") != field(request, "manifest") {
        add(&mut failures, "MANIFEST_DIGEST_MISMATCH");
    }
    let package_provenance = field(package, "provenance");
    failures.extend(statement_failures(
        package_provenance,
        &package_archive,
        package_path,
    ));
    let request_provenance = field(request, "provenance");
    if field(field(package_provenance, "statement"), "digest")
        != field(field(request_provenance, "statement"), "digest")
    {
        add(&mut failures, "PROVENANCE_STATEMENT_MISMATCH");
    }
    if field(
        field(field(package_provenance, "statement"), "value"),
        "predicateType",
    ) != field(
        field(field(request_provenance, "statement"), "value"),
        "predicateType",
    ) {
        add(&mut failures, "PROVENANCE_PREDICATE_MISMATCH");
    }
    if field(package_provenance, "selected_subject")
        != field(request_provenance, "selected_subject")
    {
        add(&mut failures, "PROVENANCE_SUBJECT_MISMATCH");
    }
    if field(field(package, "publisher"), "publisher_identity")
        != field(request, "publisher_identity")
    {
        add(&mut failures, "PUBLISHER_MISMATCH");
    }
    if field(field(package, "package"), "namespace") != field(request, "namespace") {
        add(&mut failures, "NAMESPACE_MISMATCH");
    }
    if field(field(package, "package"), "name") != field(request, "name") {
        add(&mut failures, "PACKAGE_NAME_MISMATCH");
    }
    if field(field(package, "package"), "version") != field(request, "version") {
        add(&mut failures, "PACKAGE_VERSION_MISMATCH");
    }
    if field(field(package, "registry"), "registry_identity") != field(request, "registry_identity")
        || field(field(package, "registry"), "source_identity") != field(request, "source_identity")
    {
        add(&mut failures, "SOURCE_MISMATCH");
    }
    if package_path != field(request, "target_path") {
        add(&mut failures, "TARGET_PATH_MISMATCH");
    }

    if let Some(release) = selected_release {
        if field(release, "archive") != field(request, "archive") {
            add(&mut failures, "ARCHIVE_DIGEST_MISMATCH");
        }
        if field(release, "manifest") != field(request, "manifest") {
            add(&mut failures, "MANIFEST_DIGEST_MISMATCH");
        }
        let release_provenance = field(release, "provenance");
        if field(field(release_provenance, "statement"), "digest")
            != field(field(request_provenance, "statement"), "digest")
        {
            add(&mut failures, "PROVENANCE_STATEMENT_MISMATCH");
        }
        if field(
            field(field(release_provenance, "statement"), "value"),
            "predicateType",
        ) != field(
            field(field(request_provenance, "statement"), "value"),
            "predicateType",
        ) {
            add(&mut failures, "PROVENANCE_PREDICATE_MISMATCH");
        }
        if field(release_provenance, "selected_subject")
            != field(request_provenance, "selected_subject")
        {
            add(&mut failures, "PROVENANCE_SUBJECT_MISMATCH");
        }
        if field(release, "publisher_identity") != field(request, "publisher_identity") {
            add(&mut failures, "PUBLISHER_MISMATCH");
        }
    }

    let package_signature_hash = canonical_json(package)
        .map(|encoded| sha256(&encoded))
        .unwrap_or_default();
    let observed_lock_release = selected_release.map(|release| {
        serde_json::json!({
            "registry_identity": field(release, "registry_identity"),
            "source_identity": field(release, "source_identity"),
            "namespace": field(release, "namespace"),
            "name": field(release, "name"),
            "version": field(release, "version"),
            "target_path": field(release, "target_path"),
            "publisher_identity": field(release, "publisher_identity"),
            "archive": field(release, "archive"),
            "manifest": field(release, "manifest"),
            "provenance_statement_sha256": field(field(field(release, "provenance"), "statement"), "digest").get("value").unwrap_or(&Value::Null),
            "provenance_predicate_type": field(field(field(field(release, "provenance"), "statement"), "value"), "predicateType"),
            "provenance_subject": field(field(release, "provenance"), "selected_subject"),
            "package_signature_sha256": field(release, "package_signature_sha256"),
        })
    });
    let lock_mismatch = field(offline, "network_fallback").as_bool() != Some(false)
        || number(field(offline, "root_version")) != candidate_version
        || number(field(offline, "root_sequence")) != candidate_sequence
        || field(offline, "root_transcript_sha256")
            != field(field(candidate_root, "transcript"), "sha256")
        || number(field(offline, "index_generation")) != Some(generation)
        || number(field(offline, "index_sequence")) != Some(sequence)
        || field(offline, "index_transcript_sha256") != field(field(index, "transcript"), "sha256")
        || observed_lock_release
            .as_ref()
            .is_none_or(|release| field(offline, "release") != release)
        || selected_release.is_some_and(|release| {
            text(field(release, "package_signature_sha256"))
                != Some(package_signature_hash.as_str())
        });
    if lock_mismatch {
        add(&mut failures, "OFFLINE_LOCK_MISMATCH");
    }

    let precedence_matches = field(expectation, "reason_precedence")
        .as_array()
        .is_some_and(|items| {
            items.len() == REASON_PRECEDENCE.len()
                && items
                    .iter()
                    .zip(REASON_PRECEDENCE)
                    .all(|(item, expected)| item.as_str() == Some(expected))
        });
    if !precedence_matches {
        add(&mut failures, "OFFLINE_INPUT_MISSING");
    }
    let mut reason_codes: Vec<String> = REASON_PRECEDENCE
        .iter()
        .filter(|code| failures.contains(**code))
        .map(|code| (*code).to_owned())
        .collect();
    if reason_codes.is_empty() {
        reason_codes.push("OK".to_owned());
    }
    let signers = package_valid
        .iter()
        .filter_map(|key_id| {
            let key = candidate_keys.get(key_id)?;
            Some(VerifiedSigner {
                key_id: key_id.clone(),
                public_key_fingerprint: fingerprints
                    .get(key_id)
                    .cloned()
                    .unwrap_or_else(|| key_id.clone()),
                publisher_identity: optional_string(field(key, "publisher_identity")),
                role_id: package_role_id.map(ToOwned::to_owned),
                algorithm: "ed25519".to_owned(),
                status: optional_string(field(key, "status")),
            })
        })
        .collect();
    let provenance_evidence = if [
        "PROVENANCE_STATEMENT_MISMATCH",
        "PROVENANCE_PREDICATE_MISMATCH",
        "PROVENANCE_SUBJECT_MISMATCH",
    ]
    .iter()
    .any(|reason| failures.contains(*reason))
    {
        Value::Null
    } else {
        package_provenance.clone()
    };
    PackageVerification {
        schema_version: "axiom.package_verification.v1".to_owned(),
        contract: "package.verification".to_owned(),
        contract_status: "implemented".to_owned(),
        decision: if reason_codes == ["OK"] {
            "trusted".to_owned()
        } else {
            "rejected".to_owned()
        },
        primary_reason_code: reason_codes[0].clone(),
        reason_codes,
        observed: ObservedPackage {
            registry_identity: optional_string(field(
                field(package, "registry"),
                "registry_identity",
            )),
            source_identity: optional_string(field(field(package, "registry"), "source_identity")),
            namespace: optional_string(field(field(package, "package"), "namespace")),
            name: optional_string(field(field(package, "package"), "name")),
            version: optional_string(field(field(package, "package"), "version")),
            target_path: optional_string(package_path),
            publisher_identity: optional_string(field(
                field(package, "publisher"),
                "publisher_identity",
            )),
        },
        signers,
        archive: package_archive,
        manifest_digest: field(package, "manifest").clone(),
        provenance: provenance_evidence,
        trust: TrustEvidence {
            root_version: candidate_version,
            root_sequence: candidate_sequence,
            root_transition_from: old_version,
            index_generation: generation,
            index_sequence: sequence,
            package_threshold,
            package_valid_signers: package_valid_count,
            index_threshold: expected_index_threshold,
            index_valid_signers: index_valid_count,
            offline_mode: optional_string(field(offline, "mode")),
            network_fallback: field(offline, "network_fallback").as_bool(),
            consistent_snapshot: field(field(index_signed, "consistent_snapshot"), "enabled")
                .as_bool(),
        },
    }
}

/// Authenticate metadata and exact delivered artifact bytes.
pub fn verify_package_with_artifacts(
    input: &PackageTrustInput,
    artifacts: PackageArtifacts<'_>,
) -> PackageVerification {
    if let Some(failure) = input_work_budget_failure(input) {
        return input_failure_verification(failure);
    }
    if let Some(failure) = input_schema_failure(input) {
        return input_failure_verification(failure);
    }
    let mut verdict = verify_package(input);
    let mut failures: BTreeSet<String> = verdict
        .reason_codes
        .iter()
        .filter(|reason| reason.as_str() != "OK")
        .cloned()
        .collect();

    if object(&input.package_signature).is_some() {
        match artifacts.archive {
            Some(bytes) => {
                let archive = field(&input.package_signature, "archive");
                if number(field(archive, "size")) != Some(bytes.len() as u64)
                    || text(field(field(archive, "digest"), "algorithm")) != Some("sha-256")
                    || text(field(field(archive, "digest"), "value"))
                        != Some(sha256(bytes)).as_deref()
                {
                    add(&mut failures, "ARCHIVE_DIGEST_MISMATCH");
                }
            }
            None => add(&mut failures, "OFFLINE_INPUT_MISSING"),
        }

        match artifacts.manifest {
            Some(bytes) => {
                let manifest = field(&input.package_signature, "manifest");
                if text(field(manifest, "algorithm")) != Some("sha-256")
                    || text(field(manifest, "value")) != Some(sha256(bytes)).as_deref()
                {
                    add(&mut failures, "MANIFEST_DIGEST_MISMATCH");
                }
            }
            None => add(&mut failures, "OFFLINE_INPUT_MISSING"),
        }

        match artifacts.provenance {
            Some(bytes) => {
                let statement = field(field(&input.package_signature, "provenance"), "statement");
                let canonical = canonical_json(field(statement, "value"));
                let matches = canonical.is_ok_and(|canonical| {
                    bytes == canonical
                        && text(field(statement, "canonical_bytes_hex"))
                            == Some(hex_encode(bytes)).as_deref()
                        && text(field(field(statement, "digest"), "algorithm")) == Some("sha-256")
                        && text(field(field(statement, "digest"), "value"))
                            == Some(sha256(bytes)).as_deref()
                });
                if !matches {
                    add(&mut failures, "PROVENANCE_STATEMENT_MISMATCH");
                }
            }
            None => add(&mut failures, "OFFLINE_INPUT_MISSING"),
        }
    } else {
        add(&mut failures, "OFFLINE_INPUT_MISSING");
    }

    verdict.reason_codes = REASON_PRECEDENCE
        .iter()
        .filter(|reason| failures.contains(**reason))
        .map(|reason| (*reason).to_owned())
        .collect();
    if verdict.reason_codes.is_empty() {
        verdict.reason_codes.push("OK".to_owned());
    }
    verdict.primary_reason_code = verdict.reason_codes[0].clone();
    verdict.decision = if verdict.reason_codes == ["OK"] {
        "trusted".to_owned()
    } else {
        "rejected".to_owned()
    };
    verdict
}

/// Construct the schema-shaped fail-closed result used when a required input
/// document is absent or unreadable.
pub fn offline_input_missing_verification() -> PackageVerification {
    input_failure_verification(PackageInputFailure::MissingOrUnreadable)
}

/// Construct a schema-valid, implemented, null-evidence rejection for a
/// document that failed before semantic verification.
pub fn input_failure_verification(failure: PackageInputFailure) -> PackageVerification {
    let reason = match failure {
        PackageInputFailure::MissingOrUnreadable
        | PackageInputFailure::VerificationExpectationMalformed => "OFFLINE_INPUT_MISSING",
        PackageInputFailure::TrustRootsMalformed => "ROOT_DIGEST_MISMATCH",
        PackageInputFailure::RegistryIndexMalformed => "INDEX_DIGEST_MISMATCH",
        PackageInputFailure::PackageSignatureMalformed => "SIGNATURE_MALFORMED",
    };
    PackageVerification {
        schema_version: "axiom.package_verification.v1".to_owned(),
        contract: "package.verification".to_owned(),
        contract_status: "implemented".to_owned(),
        decision: "rejected".to_owned(),
        primary_reason_code: reason.to_owned(),
        reason_codes: vec![reason.to_owned()],
        observed: ObservedPackage {
            registry_identity: None,
            source_identity: None,
            namespace: None,
            name: None,
            version: None,
            target_path: None,
            publisher_identity: None,
        },
        signers: Vec::new(),
        archive: Value::Null,
        manifest_digest: Value::Null,
        provenance: Value::Null,
        trust: TrustEvidence {
            root_version: None,
            root_sequence: None,
            root_transition_from: None,
            index_generation: 0,
            index_sequence: 0,
            package_threshold: 0,
            package_valid_signers: 0,
            index_threshold: None,
            index_valid_signers: 0,
            offline_mode: None,
            network_fallback: None,
            consistent_snapshot: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    const CONTRACT: &[u8] = include_bytes!("../../../package-trust/contract/package-trust.json");
    const VERIFICATION_SCHEMA: &[u8] =
        include_bytes!("../../../schemas/axiom-package-verification-v1.schema.json");

    fn bundle() -> PackageTrustContract {
        parse_contract_json(CONTRACT).expect("checked Package Trust contract parses strictly")
    }

    fn projected(verdict: &PackageVerification) -> Value {
        serde_json::json!({
            "decision": verdict.decision,
            "primary_reason_code": verdict.primary_reason_code,
            "reason_codes": verdict.reason_codes,
        })
    }

    fn pointer<'a>(document: &'a Value, path: &str) -> &'a Value {
        document
            .pointer(path)
            .unwrap_or_else(|| panic!("fixture pointer {path} exists"))
    }

    fn apply_mutations(document: &mut Value, mutations: &Value) {
        for mutation in mutations.as_array().expect("vector mutations are an array") {
            let path = field(mutation, "path")
                .as_str()
                .expect("mutation path is a string");
            let operation = field(mutation, "operation")
                .as_str()
                .expect("mutation operation is a string");
            if operation == "append_copy" {
                let copied = pointer(
                    document,
                    field(mutation, "from")
                        .as_str()
                        .expect("append_copy source is a string"),
                )
                .clone();
                document
                    .pointer_mut(path)
                    .and_then(Value::as_array_mut)
                    .expect("append_copy target is an array")
                    .push(copied);
                continue;
            }
            let (parent_path, final_segment) = path
                .rsplit_once('/')
                .expect("mutation does not target root");
            let final_segment = final_segment.replace("~1", "/").replace("~0", "~");
            let parent = document
                .pointer_mut(parent_path)
                .unwrap_or_else(|| panic!("mutation parent {parent_path} exists"));
            match (operation, parent) {
                ("replace", Value::Array(items)) => {
                    items[final_segment.parse::<usize>().expect("array index")] =
                        field(mutation, "value").clone();
                }
                ("replace", Value::Object(items)) => {
                    items.insert(final_segment, field(mutation, "value").clone());
                }
                ("remove", Value::Array(items)) => {
                    items.remove(final_segment.parse::<usize>().expect("array index"));
                }
                ("remove", Value::Object(items)) => {
                    items.remove(&final_segment);
                }
                _ => panic!("unsupported fixture mutation {operation}"),
            }
        }
    }

    #[test]
    fn checked_contract_and_positive_vectors_verify() {
        let bundle = bundle();
        assert_eq!(bundle.contract_status, "contract_only");
        let computed_transcript = package_transcript(
            &bundle.package_signature,
            number(field(
                field(&bundle.verification_expectation, "required_signers"),
                "package_threshold",
            ))
            .unwrap_or(0),
        )
        .expect("package transcript");
        assert_eq!(
            hex_encode(&computed_transcript),
            text(field(
                field(&bundle.package_signature, "transcript"),
                "bytes_hex"
            ))
            .expect("stored transcript")
        );
        let verdict = verify_package(&PackageTrustInput::from(&bundle));
        assert_eq!(verdict.decision, "trusted", "{verdict:#?}");
        assert_eq!(verdict.primary_reason_code, "OK");
        assert_eq!(verdict.reason_codes, ["OK"]);
        assert_eq!(verdict.contract_status, "implemented");
        let mut expected_verification = bundle.verification.clone();
        expected_verification["contract_status"] = Value::String("implemented".to_owned());
        assert_eq!(
            serde_json::to_value(&verdict).expect("serialize verdict"),
            expected_verification,
            "typed production result stays field-for-field compatible except runtime status"
        );
        for vector in &bundle.positive_vectors {
            assert_eq!(
                projected(&verdict),
                field(vector, "expected").clone(),
                "positive vector {}",
                text(field(vector, "id")).unwrap_or("<missing>")
            );
        }
    }

    #[test]
    fn published_negative_vectors_have_complete_ordered_reason_sets() {
        let bundle = bundle();
        let contract_value = serde_json::to_value(&bundle).expect("serialize fixture");
        let result_schema: Value =
            serde_json::from_slice(VERIFICATION_SCHEMA).expect("verification schema parses");
        let result_validator =
            jsonschema::validator_for(&result_schema).expect("verification schema compiles");
        let mut observed = HashSet::new();
        for vector in &bundle.negative_vectors {
            let mut mutated = contract_value.clone();
            apply_mutations(&mut mutated, field(vector, "mutations"));
            let input: PackageTrustInput =
                serde_json::from_value(mutated).expect("mutated bundle retains verifier inputs");
            let verdict = verify_package(&input);
            result_validator
                .validate(&serde_json::to_value(&verdict).expect("serialize rejected result"))
                .unwrap_or_else(|error| {
                    panic!(
                        "negative vector {} must produce schema-valid rejection: {error}",
                        text(field(vector, "id")).unwrap_or("<missing>")
                    )
                });
            if verdict.reason_codes.iter().any(|reason| {
                matches!(
                    reason.as_str(),
                    "PROVENANCE_STATEMENT_MISMATCH"
                        | "PROVENANCE_PREDICATE_MISMATCH"
                        | "PROVENANCE_SUBJECT_MISMATCH"
                )
            }) {
                assert!(
                    verdict.provenance.is_null(),
                    "invalid provenance must not be copied into rejected evidence for {}",
                    text(field(vector, "id")).unwrap_or("<missing>")
                );
            }
            assert_eq!(
                projected(&verdict),
                field(vector, "expected").clone(),
                "negative vector {}",
                text(field(vector, "id")).unwrap_or("<missing>")
            );
            assert_eq!(
                verdict.primary_reason_code, verdict.reason_codes[0],
                "primary follows published precedence"
            );
            observed.extend(verdict.reason_codes);
        }
        assert!(
            REASON_PRECEDENCE
                .iter()
                .all(|reason| observed.contains(*reason)),
            "negative vectors cover every stable reason"
        );
    }

    #[test]
    fn rfc_8032_empty_message_signature_verifies_strictly() {
        let public_key = Value::String(
            "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a".into(),
        );
        let signature = Value::String(
            concat!(
                "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155",
                "5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
            )
            .into(),
        );
        assert_eq!(signature_status(&public_key, b"", &signature), None);
    }

    #[test]
    fn duplicate_json_members_are_rejected_at_any_depth() {
        let duplicate = br#"{
            "package_signature": {},
            "trust_roots": {"candidate_root": 1, "candidate_root": 2},
            "registry_index": {},
            "verification_expectation": {}
        }"#;
        let error = parse_input_json(duplicate).expect_err("duplicate member must fail closed");
        assert!(
            error
                .to_string()
                .contains("duplicate JSON member \"candidate_root\""),
            "{error}"
        );
    }

    #[test]
    fn separate_document_parsers_reject_duplicate_members() {
        let bundle = bundle();
        let package = serde_json::to_vec(&bundle.package_signature).expect("serialize package");
        let roots = serde_json::to_vec(&bundle.trust_roots).expect("serialize roots");
        let index = serde_json::to_vec(&bundle.registry_index).expect("serialize index");
        let expectation =
            serde_json::to_vec(&bundle.verification_expectation).expect("serialize expectation");
        parse_package_signature_json(&package).expect("strict package parser");
        parse_trust_roots_json(&roots).expect("strict roots parser");
        parse_registry_index_json(&index).expect("strict index parser");
        parse_verification_expectation_json(&expectation).expect("strict expectation parser");

        let duplicate = br#"{"contract":"package.signature","contract":"shadow"}"#;
        assert!(
            parse_package_signature_json(duplicate)
                .expect_err("separate parser must reject duplicate")
                .to_string()
                .contains("duplicate JSON member")
        );

        let mut unknown_package = bundle.package_signature.0.clone();
        unknown_package["unexpected"] = Value::Bool(true);
        assert!(
            parse_package_signature_json(
                &serde_json::to_vec(&unknown_package).expect("serialize unknown package field")
            )
            .is_err()
        );

        let mut unknown_nested_roots = bundle.trust_roots.0.clone();
        unknown_nested_roots["candidate_root"]["signed"]["unexpected"] = Value::Bool(true);
        assert!(
            parse_trust_roots_json(
                &serde_json::to_vec(&unknown_nested_roots).expect("serialize unknown root field")
            )
            .is_err()
        );

        let mut invalid_index_status = bundle.registry_index.0.clone();
        invalid_index_status["contract_status"] = Value::String("unreviewed".to_owned());
        assert!(
            parse_registry_index_json(
                &serde_json::to_vec(&invalid_index_status).expect("serialize invalid index status")
            )
            .is_err()
        );

        let mut missing_expectation_request = bundle.verification_expectation.0.clone();
        missing_expectation_request
            .as_object_mut()
            .expect("expectation object")
            .remove("request");
        assert!(
            parse_verification_expectation_json(
                &serde_json::to_vec(&missing_expectation_request)
                    .expect("serialize missing expectation field")
            )
            .is_err()
        );
    }

    #[test]
    fn document_work_budgets_enforce_published_collection_and_string_caps() {
        let package_at_limit = serde_json::json!({
            "signatures": vec![Value::Null; MAX_SIGNATURES],
        });
        assert!(validate_document_work_budget(
            &package_at_limit,
            DocumentKind::Package
        ));
        let package_over_limit = serde_json::json!({
            "signatures": vec![Value::Null; MAX_SIGNATURES + 1],
        });
        assert!(!validate_document_work_budget(
            &package_over_limit,
            DocumentKind::Package
        ));

        let index_over_limit = serde_json::json!({
            "signed": {"releases": vec![Value::Null; MAX_RELEASES + 1]},
        });
        assert!(!validate_document_work_budget(
            &index_over_limit,
            DocumentKind::Index
        ));
        let roots_over_limit = serde_json::json!({
            "candidate_root": {
                "signed": {"keys": vec![Value::Null; MAX_ROOT_KEYS + 1]},
            },
        });
        assert!(!validate_document_work_budget(
            &roots_over_limit,
            DocumentKind::Roots
        ));
        let supersession_over_limit = serde_json::json!({
            "supersedes_key_ids": vec![Value::Null; MAX_ROLE_KEY_IDS + 1],
        });
        assert!(!validate_document_work_budget(
            &supersession_over_limit,
            DocumentKind::Roots
        ));
        let expectation_over_limit = serde_json::json!({
            "required_signers": {
                "required_key_ids": vec![Value::Null; MAX_REQUIRED_KEY_IDS + 1],
            },
        });
        assert!(!validate_document_work_budget(
            &expectation_over_limit,
            DocumentKind::Expectation
        ));
        let identifier_over_limit = serde_json::json!({
            "namespace": "x".repeat(257),
        });
        assert!(!validate_document_work_budget(
            &identifier_over_limit,
            DocumentKind::Package
        ));
        let external_identity_at_limit = serde_json::json!({
            "publisher_identity": "x".repeat(2_048),
        });
        assert!(validate_document_work_budget(
            &external_identity_at_limit,
            DocumentKind::Package
        ));
        let external_identity_over_limit = serde_json::json!({
            "publisher_identity": "x".repeat(2_049),
        });
        assert!(!validate_document_work_budget(
            &external_identity_over_limit,
            DocumentKind::Package
        ));
        let free_text_over_limit = serde_json::json!({
            "reason": "x".repeat(4_097),
        });
        assert!(!validate_document_work_budget(
            &free_text_over_limit,
            DocumentKind::Roots
        ));
        let threshold_over_limit = serde_json::json!({
            "package_threshold": MAX_THRESHOLD + 1,
        });
        assert!(!validate_document_work_budget(
            &threshold_over_limit,
            DocumentKind::Expectation
        ));
        let slsa_collection_over_limit = serde_json::json!({
            "subject": vec![Value::Null; MAX_SLSA_COLLECTION_ITEMS + 1],
        });
        assert!(!validate_document_work_budget(
            &slsa_collection_over_limit,
            DocumentKind::Package
        ));
        let seen_snapshots_over_limit = serde_json::json!({
            "seen_snapshots": vec![Value::Null; MAX_SEEN_SNAPSHOTS + 1],
        });
        assert!(!validate_document_work_budget(
            &seen_snapshots_over_limit,
            DocumentKind::Expectation
        ));
        let oversized_map = Value::Object(
            (0..=MAX_MAP_ENTRIES)
                .map(|index| (format!("key{index}"), Value::Null))
                .collect(),
        );
        assert!(!validate_document_work_budget(
            &oversized_map,
            DocumentKind::Package
        ));

        let oversized_json = vec![b' '; MAX_DOCUMENT_BYTES + 1];
        assert!(
            parse_strict_json_value(&oversized_json)
                .expect_err("raw input above 8 MiB must fail before parsing")
                .to_string()
                .contains("work budget")
        );
    }

    #[test]
    fn programmatic_inputs_over_work_budgets_fail_closed_with_null_evidence() {
        let bundle = bundle();
        let cases = [
            (
                {
                    let mut input = PackageTrustInput::from(&bundle);
                    let signature = field(&input.package_signature, "signatures")
                        .as_array()
                        .and_then(|items| items.first())
                        .cloned()
                        .expect("package fixture has a signature");
                    input.package_signature.0["signatures"] =
                        Value::Array(vec![signature; MAX_SIGNATURES + 1]);
                    input
                },
                "SIGNATURE_MALFORMED",
            ),
            (
                {
                    let mut input = PackageTrustInput::from(&bundle);
                    let release = field(field(&input.registry_index, "signed"), "releases")
                        .as_array()
                        .and_then(|items| items.first())
                        .cloned()
                        .expect("index fixture has a release");
                    input.registry_index.0["signed"]["releases"] =
                        Value::Array(vec![release; MAX_RELEASES + 1]);
                    input
                },
                "INDEX_DIGEST_MISMATCH",
            ),
            (
                {
                    let mut input = PackageTrustInput::from(&bundle);
                    let key = field(
                        field(field(&input.trust_roots, "candidate_root"), "signed"),
                        "keys",
                    )
                    .as_array()
                    .and_then(|items| items.first())
                    .cloned()
                    .expect("root fixture has a key");
                    input.trust_roots.0["candidate_root"]["signed"]["keys"] =
                        Value::Array(vec![key; MAX_ROOT_KEYS + 1]);
                    input
                },
                "ROOT_DIGEST_MISMATCH",
            ),
            (
                {
                    let mut input = PackageTrustInput::from(&bundle);
                    input.verification_expectation.0["required_signers"]["required_key_ids"] =
                        Value::Array(vec![
                            Value::String("sha256:00".to_owned());
                            MAX_REQUIRED_KEY_IDS + 1
                        ]);
                    input
                },
                "OFFLINE_INPUT_MISSING",
            ),
        ];

        for (input, expected_reason) in cases {
            let verdict = verify_package_with_artifacts(&input, PackageArtifacts::default());
            assert_eq!(verdict.decision, "rejected");
            assert_eq!(verdict.primary_reason_code, expected_reason);
            assert_eq!(verdict.reason_codes, [expected_reason]);
            assert!(verdict.archive.is_null());
            assert!(verdict.manifest_digest.is_null());
            assert!(verdict.provenance.is_null());
            validate_package_verification(&verdict)
                .expect("resource-limit rejection must match the result schema");
        }
    }

    #[test]
    fn lowercase_hex_decoder_never_panics_on_unicode_or_accepts_uppercase() {
        assert!(decode_hex(&Value::String("0é0".to_owned())).is_err());
        assert!(decode_hex(&Value::String("AA".to_owned())).is_err());
        assert_eq!(
            decode_hex(&Value::String("aa".to_owned())).expect("canonical lowercase hex"),
            [0xaa]
        );
    }

    #[test]
    fn uppercase_public_key_alias_cannot_satisfy_threshold() {
        let bundle = bundle();
        let candidate_signed = field(field(&bundle.trust_roots, "candidate_root"), "signed");
        let first_signature = field(&bundle.package_signature, "signatures")
            .as_array()
            .and_then(|signatures| signatures.first())
            .expect("first package signature")
            .clone();
        let original_key_id = text(field(&first_signature, "key_id")).expect("signature key id");
        let original_key = field(candidate_signed, "keys")
            .as_array()
            .and_then(|keys| {
                keys.iter()
                    .find(|key| text(field(key, "key_id")) == Some(original_key_id))
            })
            .expect("signing key");

        let mut alias = original_key.clone();
        let uppercase_public = text(field(field(&alias, "key_material"), "public_key"))
            .expect("public key")
            .to_ascii_uppercase();
        alias["key_material"]["public_key"] = Value::String(uppercase_public);
        let alias_id = derived_key_id(field(&alias, "key_material")).expect("textual alias key id");
        alias["key_id"] = Value::String(alias_id.clone());
        let mut aliased_root = candidate_signed.clone();
        aliased_root["keys"]
            .as_array_mut()
            .expect("root keys")
            .push(alias);

        let mut failures = BTreeSet::new();
        let (keys, _) = key_maps(&aliased_root, &mut failures);
        assert!(failures.contains("KEY_MALFORMED"));
        let package_role_id = text(field(
            field(&bundle.verification_expectation, "required_signers"),
            "package_role_id",
        ))
        .expect("package role id");
        let mut role = field(candidate_signed, "roles")
            .as_array()
            .and_then(|roles| {
                roles
                    .iter()
                    .find(|role| text(field(role, "role_id")) == Some(package_role_id))
            })
            .expect("package role")
            .clone();
        role["key_ids"]
            .as_array_mut()
            .expect("role key ids")
            .push(Value::String(alias_id.clone()));
        role["threshold"] = Value::from(3_u64);

        let mut signatures = field(&bundle.package_signature, "signatures").clone();
        let mut alias_signature = first_signature;
        alias_signature["key_id"] = Value::String(alias_id);
        signatures
            .as_array_mut()
            .expect("package signatures")
            .push(alias_signature);
        let message = package_transcript(&bundle.package_signature, 2).expect("package transcript");
        let (_, count) = signature_evidence(
            &signatures,
            SignatureEvidence {
                role: Some(&role),
                keys: &keys,
                message: &message,
                sequence: 1042,
                verification_time: timestamp(field(
                    &bundle.verification_expectation,
                    "verification_time",
                ))
                .expect("verification time"),
                context: "PACKAGE",
                required_key_ids: Some(HashSet::new()),
                expected_publisher: text(field(
                    field(&bundle.verification_expectation, "request"),
                    "publisher_identity",
                )),
                publisher_grant_authorized: true,
            },
            &mut failures,
        );
        assert_eq!(count, 2, "uppercase alias must not become a third signer");
        assert!(failures.contains("PACKAGE_THRESHOLD_NOT_MET"));
    }

    #[test]
    fn deep_and_cyclic_supersession_graphs_are_bounded_and_rejected() {
        let mut deep = KeyMap::new();
        for index in 0_u64..20_000 {
            deep.insert(
                format!("key-{index}"),
                serde_json::json!({
                    "publisher_identity": "publisher",
                    "status": if index == 0 { "retired" } else { "active" },
                    "valid_from_sequence": index + 1,
                    "revocation": null,
                    "supersedes_key_ids": if index == 0 {
                        Vec::<String>::new()
                    } else {
                        vec![format!("key-{}", index - 1)]
                    },
                }),
            );
        }
        let mut deep_failures = BTreeSet::new();
        validate_key_supersession(&deep, &mut deep_failures);
        assert!(deep_failures.contains("KEY_SUPERSESSION_INVALID"));

        let mut cycle = KeyMap::new();
        for (key, predecessor) in [("a", "b"), ("b", "c"), ("c", "a")] {
            cycle.insert(
                key.to_owned(),
                serde_json::json!({
                    "publisher_identity": "publisher",
                    "status": "active",
                    "valid_from_sequence": 2,
                    "revocation": null,
                    "supersedes_key_ids": [predecessor],
                }),
            );
        }
        let mut cycle_failures = BTreeSet::new();
        validate_key_supersession(&cycle, &mut cycle_failures);
        assert!(cycle_failures.contains("KEY_SUPERSESSION_INVALID"));
    }

    #[test]
    fn authenticated_times_are_enforced_and_unsigned_transition_time_is_informational() {
        let bundle = bundle();

        let mut informational = PackageTrustInput::from(&bundle);
        informational.trust_roots.0["transition"]["transition_time"] =
            Value::String("2099-01-01T00:00:00Z".to_owned());
        assert_eq!(
            verify_package(&informational).reason_codes,
            ["OK"],
            "unsigned transition_time must not influence trust"
        );

        let mut future_root = PackageTrustInput::from(&bundle);
        future_root.trust_roots.0["candidate_root"]["signed"]["issued_at"] =
            Value::String("2026-08-01T00:00:00Z".to_owned());
        assert!(
            verify_package(&future_root)
                .reason_codes
                .contains(&"ROOT_ROTATION_INVALID".to_owned())
        );

        let mut future_index = PackageTrustInput::from(&bundle);
        future_index.registry_index.0["signed"]["issued_at"] =
            Value::String("2026-08-01T00:00:00Z".to_owned());
        assert_eq!(
            verify_package(&future_index).primary_reason_code,
            "METADATA_EXPIRED"
        );

        let mut overflow = PackageTrustInput::from(&bundle);
        overflow.trust_roots.0["trusted_root"]["signed"]["root_version"] = Value::from(u64::MAX);
        overflow.trust_roots.0["candidate_root"]["signed"]["root_version"] = Value::from(u64::MAX);
        overflow.trust_roots.0["transition"]["from_version"] = Value::from(u64::MAX);
        overflow.trust_roots.0["transition"]["to_version"] = Value::from(u64::MAX);
        overflow.verification_expectation.0["trusted_state"]["trusted_root_anchor"]["root_version"] =
            Value::from(u64::MAX);
        assert!(
            verify_package(&overflow)
                .reason_codes
                .contains(&"ROOT_ROTATION_INVALID".to_owned())
        );
    }

    #[test]
    fn registry_index_trust_preflight_returns_authenticated_eligible_roles() {
        let bundle = bundle();
        let trusted =
            preflight_registry_index_trust(&bundle.trust_roots, &bundle.verification_expectation)
                .expect("published roots pass signing preflight");
        assert_eq!(trusted.root_version, 4);
        assert_eq!(trusted.root_sequence, 1040);
        assert_eq!(trusted.index_role_id, "registry-index");
        assert_eq!(trusted.index_threshold, 2);
        assert_eq!(trusted.index_eligible_key_ids.len(), 2);
        assert_eq!(trusted.package_role_id, "targets:axiom");
        assert_eq!(trusted.package_threshold, 2);

        let candidate_keys = field(
            field(field(&bundle.trust_roots, "candidate_root"), "signed"),
            "keys",
        )
        .as_array()
        .expect("candidate keys");
        let revoked_id = candidate_keys
            .iter()
            .find(|key| text(field(key, "status")) == Some("revoked"))
            .and_then(|key| text(field(key, "key_id")))
            .expect("revoked fixture key");
        let premature_id = candidate_keys
            .iter()
            .find(|key| number(field(key, "valid_from_sequence")) == Some(1100))
            .and_then(|key| text(field(key, "key_id")))
            .expect("premature fixture key");
        assert!(
            !trusted
                .package_eligible_key_ids
                .iter()
                .any(|id| id == revoked_id)
        );
        assert!(
            !trusted
                .package_eligible_key_ids
                .iter()
                .any(|id| id == premature_id)
        );

        let mut before_revocation = bundle.verification_expectation.clone();
        before_revocation.0["verification_time"] = Value::String("2026-07-10T00:00:00Z".to_owned());
        let before_revocation =
            preflight_registry_index_trust(&bundle.trust_roots, &before_revocation)
                .expect("future-effective revocation remains eligible");
        assert!(
            before_revocation
                .package_eligible_key_ids
                .iter()
                .any(|id| id == revoked_id),
            "revocation is not effective until its sequence or time boundary"
        );
        assert!(
            !before_revocation
                .package_eligible_key_ids
                .iter()
                .any(|id| id == premature_id),
            "valid_from_sequence remains independently enforced"
        );
    }

    #[test]
    fn registry_index_trust_preflight_rejects_adversarial_root_state() {
        let bundle = bundle();
        let assert_rejected =
            |roots: &TrustRootsEnvelope, expectation: &VerificationExpectation, expected: &str| {
                let error = preflight_registry_index_trust(roots, expectation)
                    .expect_err("adversarial root state must fail before signing");
                assert!(
                    error.reason_codes.iter().any(|reason| reason == expected),
                    "expected {expected}, observed {:?}",
                    error.reason_codes
                );
            };

        let mut bad_anchor = bundle.verification_expectation.clone();
        bad_anchor.0["trusted_state"]["trusted_root_anchor"]["root_transcript_sha256"] =
            Value::String("0".repeat(64));
        assert_rejected(&bundle.trust_roots, &bad_anchor, "ROOT_BOOTSTRAP_MISMATCH");

        let mut expired = bundle.verification_expectation.clone();
        expired.0["verification_time"] = Value::String("2028-01-01T00:00:00Z".to_owned());
        assert_rejected(&bundle.trust_roots, &expired, "METADATA_EXPIRED");

        let mut rollback = bundle.verification_expectation.clone();
        rollback.0["trusted_state"]["highest_root_version"] = Value::from(9_u64);
        assert_rejected(&bundle.trust_roots, &rollback, "ROOT_ROLLBACK");

        let mut bad_transition = bundle.trust_roots.clone();
        bad_transition.0["transition"]["from_version"] = Value::from(99_u64);
        assert_rejected(
            &bad_transition,
            &bundle.verification_expectation,
            "ROOT_ROTATION_INVALID",
        );

        let mut bad_signature = bundle.trust_roots.clone();
        bad_signature.0["candidate_root"]["signatures"][0]["value"] =
            Value::String("0".repeat(128));
        assert_rejected(
            &bad_signature,
            &bundle.verification_expectation,
            "ROOT_SIGNATURE_INVALID",
        );

        let mut bad_key_id = bundle.trust_roots.clone();
        bad_key_id.0["candidate_root"]["signed"]["keys"][0]["key_id"] =
            Value::String(format!("sha256:{}", "0".repeat(64)));
        assert_rejected(
            &bad_key_id,
            &bundle.verification_expectation,
            "KEY_ID_MISMATCH",
        );

        let mut bad_supersession = bundle.trust_roots.clone();
        let self_id = bad_supersession.0["candidate_root"]["signed"]["keys"][0]["key_id"].clone();
        bad_supersession.0["candidate_root"]["signed"]["keys"][0]["supersedes_key_ids"] =
            Value::Array(vec![self_id]);
        assert_rejected(
            &bad_supersession,
            &bundle.verification_expectation,
            "KEY_SUPERSESSION_INVALID",
        );

        let mut threshold_mismatch = bundle.verification_expectation.clone();
        threshold_mismatch.0["required_signers"]["index_threshold"] = Value::from(3_u64);
        assert_rejected(
            &bundle.trust_roots,
            &threshold_mismatch,
            "INDEX_THRESHOLD_NOT_MET",
        );
    }

    #[test]
    fn package_index_and_index_identity_substitutions_fail_closed() {
        let bundle = bundle();

        let mut old_envelope = PackageTrustInput::from(&bundle);
        old_envelope.package_signature.0["index"]["sequence"] = Value::from(1041_u64);
        let old_envelope_verdict = verify_package(&old_envelope);
        assert_eq!(old_envelope_verdict.decision, "rejected");
        assert!(
            old_envelope_verdict
                .reason_codes
                .contains(&"OFFLINE_LOCK_MISMATCH".to_owned())
        );

        let mut registry_substitution = PackageTrustInput::from(&bundle);
        registry_substitution.registry_index.0["signed"]["registry_identity"] =
            Value::String("attacker-registry".to_owned());
        assert!(
            verify_package(&registry_substitution)
                .reason_codes
                .contains(&"SOURCE_MISMATCH".to_owned())
        );

        let mut role_substitution = PackageTrustInput::from(&bundle);
        role_substitution.registry_index.0["signed"]["signature_role"] =
            Value::String("root".to_owned());
        assert!(
            verify_package(&role_substitution)
                .reason_codes
                .contains(&"INDEX_THRESHOLD_NOT_MET".to_owned())
        );
    }

    #[test]
    fn authenticated_newer_index_accepts_retained_package_publication_floor() {
        let bundle = bundle();
        let vector = bundle
            .negative_vectors
            .iter()
            .find(|vector| text(field(vector, "id")) == Some("duplicate-target-path-resigned"))
            .expect("authenticated newer-index vector");
        let replacement = field(vector, "mutations")
            .as_array()
            .and_then(|mutations| {
                mutations.iter().find(|mutation| {
                    text(field(mutation, "operation")) == Some("replace")
                        && text(field(mutation, "path")) == Some("/registry_index")
                })
            })
            .map(|mutation| field(mutation, "value").clone())
            .expect("vector replaces the complete authenticated index");
        let mut retained = PackageTrustInput::from(&bundle);
        retained.registry_index = RegistryIndexEnvelope(replacement);
        let generation = number(field(
            field(&retained.registry_index, "signed"),
            "generation",
        ))
        .expect("new generation");
        let sequence = number(field(field(&retained.registry_index, "signed"), "sequence"))
            .expect("new sequence");
        retained.verification_expectation.0["offline_lock"]["index_generation"] =
            Value::from(generation);
        retained.verification_expectation.0["offline_lock"]["index_sequence"] =
            Value::from(sequence);
        retained.verification_expectation.0["offline_lock"]["index_transcript_sha256"] =
            field(field(&retained.registry_index, "transcript"), "sha256").clone();

        assert!(package_index_floor_is_satisfied(
            &retained.package_signature,
            generation,
            sequence
        ));
        let verdict = verify_package(&retained);
        assert_eq!(
            verdict.reason_codes,
            ["DUPLICATE_TARGET_PATH"],
            "the authenticated newer index is rejected only for its deliberate duplicate"
        );
        assert!(
            !verdict
                .reason_codes
                .contains(&"METADATA_REPLAYED".to_owned())
        );
        assert!(
            !verdict
                .reason_codes
                .contains(&"OFFLINE_LOCK_MISMATCH".to_owned())
        );
    }

    #[test]
    fn resigned_future_package_coordinates_fail_componentwise_floor_parity() {
        let bundle = bundle();
        let current_generation =
            number(field(field(&bundle.registry_index, "signed"), "generation"))
                .expect("current generation");
        let current_sequence = number(field(field(&bundle.registry_index, "signed"), "sequence"))
            .expect("current sequence");
        let threshold = number(field(
            field(&bundle.verification_expectation, "required_signers"),
            "package_threshold",
        ))
        .expect("package threshold");
        let signer = TestSigner(SigningKey::from_bytes(&[19_u8; 32]));

        for (name, generation, sequence) in [
            (
                "future generation",
                current_generation + 1,
                current_sequence,
            ),
            ("future sequence", current_generation, current_sequence + 1),
        ] {
            let mut future = bundle.package_signature.clone();
            future.0["index"]["generation"] = Value::from(generation);
            future.0["index"]["sequence"] = Value::from(sequence);
            let signature = sign_package_transcript(&future, threshold, &signer)
                .expect("future coordinate transcript can be re-signed");
            let transcript =
                package_transcript(&future, threshold).expect("future package transcript");
            assert_eq!(
                signature_status(
                    &Value::String(hex_encode(&signer.0.verifying_key().to_bytes())),
                    &transcript,
                    &Value::String(signature.value.clone()),
                ),
                None,
                "{name} re-signature is cryptographically valid"
            );
            future.0["transcript"]["bytes_hex"] = Value::String(hex_encode(&transcript));
            future.0["transcript"]["sha256"] = Value::String(sha256(&transcript));
            future.0["signatures"] = Value::Array(vec![
                serde_json::to_value(signature).expect("serialize re-signature"),
            ]);
            assert!(
                !package_index_floor_is_satisfied(&future, current_generation, current_sequence),
                "{name} must fail the shared publication-floor predicate"
            );
            let mut input = PackageTrustInput::from(&bundle);
            input.package_signature = future;
            assert!(
                verify_package(&input)
                    .reason_codes
                    .contains(&"OFFLINE_LOCK_MISMATCH".to_owned()),
                "{name} must produce the same verifier rejection"
            );
        }
    }

    #[test]
    fn unsigned_provenance_semantics_reject_reversed_build_timestamps() {
        let bundle = bundle();
        validate_package_provenance_semantics(&bundle.package_signature)
            .expect("published provenance semantics are valid");

        let mut reversed = bundle.package_signature.clone();
        reversed.0["provenance"]["statement"]["value"]["predicate"]["runDetails"]["metadata"]["startedOn"] =
            Value::String("2026-07-29T10:06:00Z".to_owned());
        let error = validate_package_provenance_semantics(&reversed)
            .expect_err("reversed build timestamps must fail before signing");
        assert!(
            error
                .reason_codes
                .contains(&"PROVENANCE_PREDICATE_MISMATCH".to_owned()),
            "{:?}",
            error.reason_codes
        );
    }

    #[test]
    fn artifact_verification_fails_closed_for_missing_and_tampered_bytes() {
        let bundle = bundle();
        let input = PackageTrustInput::from(&bundle);
        let missing = verify_package_with_artifacts(&input, PackageArtifacts::default());
        assert_eq!(missing.primary_reason_code, "OFFLINE_INPUT_MISSING");
        assert!(
            missing
                .reason_codes
                .contains(&"OFFLINE_INPUT_MISSING".to_owned())
        );

        let canonical_provenance = decode_hex(field(
            field(field(&bundle.package_signature, "provenance"), "statement"),
            "canonical_bytes_hex",
        ))
        .expect("fixture provenance bytes");
        let tampered = verify_package_with_artifacts(
            &input,
            PackageArtifacts {
                archive: Some(b"tampered archive"),
                manifest: Some(b"tampered manifest"),
                provenance: Some(&canonical_provenance),
            },
        );
        assert!(
            tampered
                .reason_codes
                .contains(&"ARCHIVE_DIGEST_MISMATCH".to_owned())
        );
        assert!(
            tampered
                .reason_codes
                .contains(&"MANIFEST_DIGEST_MISMATCH".to_owned())
        );
        assert!(
            !tampered
                .reason_codes
                .contains(&"PROVENANCE_STATEMENT_MISMATCH".to_owned())
        );
    }

    #[test]
    fn artifact_verification_rejects_noncanonical_provenance_bytes() {
        let bundle = bundle();
        let input = PackageTrustInput::from(&bundle);
        let statement = field(
            field(field(&bundle.package_signature, "provenance"), "statement"),
            "value",
        );
        let noncanonical =
            serde_json::to_vec_pretty(statement).expect("render semantically equal statement");
        assert_ne!(
            noncanonical,
            canonical_json(statement).expect("canonical statement")
        );
        let verdict = verify_package_with_artifacts(
            &input,
            PackageArtifacts {
                archive: Some(&[]),
                manifest: Some(&[]),
                provenance: Some(&noncanonical),
            },
        );
        assert!(
            verdict
                .reason_codes
                .contains(&"PROVENANCE_STATEMENT_MISMATCH".to_owned())
        );
    }

    struct TestSigner(SigningKey);

    impl Ed25519Signer for TestSigner {
        type Error = std::convert::Infallible;

        fn public_key(&self) -> Result<[u8; 32], Self::Error> {
            Ok(self.0.verifying_key().to_bytes())
        }

        fn sign(&self, message: &[u8]) -> Result<[u8; 64], Self::Error> {
            Ok(self.0.sign(message).to_bytes())
        }
    }

    #[test]
    fn external_signer_helper_derives_key_id_and_self_verifies() {
        let bundle = bundle();
        let signer = TestSigner(SigningKey::from_bytes(&[7_u8; 32]));
        let threshold = number(field(
            field(&bundle.verification_expectation, "required_signers"),
            "package_threshold",
        ))
        .expect("package threshold");
        let entry = sign_package_transcript(&bundle.package_signature, threshold, &signer)
            .expect("external signer output");
        assert!(entry.key_id.starts_with("sha256:"));
        assert_eq!(entry.algorithm, "ed25519");
        assert_eq!(entry.encoding, "lowercase-hex");
        let transcript =
            package_transcript(&bundle.package_signature, threshold).expect("package transcript");
        assert_eq!(
            signature_status(
                &Value::String(hex_encode(&signer.0.verifying_key().to_bytes())),
                &transcript,
                &Value::String(entry.value),
            ),
            None
        );
    }

    #[test]
    fn missing_input_result_is_implemented_and_uses_null_evidence() {
        let verdict = offline_input_missing_verification();
        assert_eq!(verdict.contract_status, "implemented");
        assert_eq!(verdict.primary_reason_code, "OFFLINE_INPUT_MISSING");
        assert!(verdict.archive.is_null());
        assert!(verdict.manifest_digest.is_null());
        assert!(verdict.provenance.is_null());
    }

    #[test]
    fn runtime_and_missing_results_match_the_published_result_schema() {
        let schema: Value =
            serde_json::from_slice(VERIFICATION_SCHEMA).expect("verification schema parses");
        let validator = jsonschema::validator_for(&schema).expect("verification schema compiles");
        let bundle = bundle();
        let runtime = verify_package(&PackageTrustInput::from(&bundle));
        validator
            .validate(&serde_json::to_value(runtime).expect("serialize runtime result"))
            .expect("runtime result is schema-valid");
        validator
            .validate(
                &serde_json::to_value(offline_input_missing_verification())
                    .expect("serialize missing-input result"),
            )
            .expect("missing-input result is schema-valid");
    }

    #[test]
    fn outbound_result_validator_rejects_cross_field_mismatch_and_unknown_fields() {
        let mut mismatch = offline_input_missing_verification();
        mismatch.decision = "trusted".to_owned();
        assert!(
            validate_package_verification(&mismatch)
                .expect_err("trusted decision cannot carry rejection evidence")
                .to_string()
                .contains("published schema")
        );

        let mut unknown =
            serde_json::to_value(offline_input_missing_verification()).expect("serialize result");
        unknown["unexpected"] = Value::Bool(true);
        assert!(
            validate_schema_value(
                &unknown,
                VERIFICATION_SCHEMA,
                &VERIFICATION_RESULT_SCHEMA,
                "package verification result",
            )
            .expect_err("unknown result members must fail closed")
            .to_string()
            .contains("published schema")
        );
    }

    #[test]
    fn typed_input_failures_have_stable_schema_valid_reasons() {
        let schema: Value =
            serde_json::from_slice(VERIFICATION_SCHEMA).expect("verification schema parses");
        let validator = jsonschema::validator_for(&schema).expect("verification schema compiles");
        let cases = [
            (
                PackageInputFailure::MissingOrUnreadable,
                "OFFLINE_INPUT_MISSING",
            ),
            (
                PackageInputFailure::VerificationExpectationMalformed,
                "OFFLINE_INPUT_MISSING",
            ),
            (
                PackageInputFailure::TrustRootsMalformed,
                "ROOT_DIGEST_MISMATCH",
            ),
            (
                PackageInputFailure::RegistryIndexMalformed,
                "INDEX_DIGEST_MISMATCH",
            ),
            (
                PackageInputFailure::PackageSignatureMalformed,
                "SIGNATURE_MALFORMED",
            ),
        ];
        for (failure, expected_reason) in cases {
            let verdict = input_failure_verification(failure);
            assert_eq!(verdict.contract_status, "implemented");
            assert_eq!(verdict.decision, "rejected");
            assert_eq!(verdict.primary_reason_code, expected_reason);
            assert_eq!(verdict.reason_codes, [expected_reason]);
            assert!(verdict.archive.is_null());
            assert!(verdict.manifest_digest.is_null());
            assert!(verdict.provenance.is_null());
            validator
                .validate(&serde_json::to_value(verdict).expect("serialize failure result"))
                .unwrap_or_else(|error| {
                    panic!("{failure:?} result must match verification schema: {error}")
                });
        }
    }
}
