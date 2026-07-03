use crate::diagnostics::Diagnostic;
use crate::lockfile::validate_lockfile;
use crate::manifest::{
    LOCK_FILENAME, MANIFEST_FILENAME, capability_descriptors, load_manifest, manifest_path,
};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};

const REGISTRY_METADATA_FILENAME: &str = "axiom-registry.toml";
const DEFAULT_ARCHIVE_FILENAME: &str = "package.axp";
const ARCHIVE_AUTH_HEADER: &str = "axiom-hmac-sha256-v1";
const SHA256_BLOCK_LEN: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryIndex {
    pub version: u32,
    pub packages: BTreeMap<String, Vec<RegistryRelease>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryCapability {
    pub name: String,
    pub enabled: bool,
    pub description: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub allowed: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub unsafe_unrestricted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryRelease {
    pub version: String,
    pub source: String,
    pub manifest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    pub yanked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yank_reason: Option<String>,
    pub capabilities: Vec<RegistryCapability>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PublishOutput {
    pub package: String,
    pub version: String,
    pub release_dir: String,
    pub manifest: String,
    pub archive: String,
    pub signature: String,
    pub archive_hash: String,
}

#[derive(Debug, Clone, Default)]
pub struct PublishOptions {
    pub signing_key: Option<String>,
    pub allow_overwrite: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryServeOptions {
    pub addr: String,
    pub base_url: Option<String>,
    pub signing_key: String,
    pub once: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RegistryServeOutput {
    pub addr: String,
    pub base_url: String,
    pub requests: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegistryServeContext {
    packages_root: PathBuf,
    index: RegistryIndex,
    index_body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegistryHttpResponse<'a> {
    status: &'static str,
    content_type: &'static str,
    body: Cow<'a, [u8]>,
}

#[derive(Debug, Default, Deserialize)]
struct RawRegistryMetadata {
    archive: Option<String>,
    signature: Option<String>,
    yanked: Option<bool>,
    yank_reason: Option<String>,
}

pub fn publish_package(
    project_root: &Path,
    registry_root: &Path,
    options: &PublishOptions,
) -> Result<PublishOutput, Diagnostic> {
    let signing_key = options.signing_key.as_deref().ok_or_else(|| {
        Diagnostic::new(
            "publish",
            "publish requires --signing-key; the stage1 registry has no default authentication key",
        )
    })?;
    if signing_key.trim().is_empty() {
        return Err(Diagnostic::new(
            "publish",
            "--signing-key must not be empty",
        ));
    }
    let project_root = fs::canonicalize(project_root).map_err(|err| {
        Diagnostic::new(
            "publish",
            format!(
                "failed to resolve project root {}: {err}",
                project_root.display()
            ),
        )
        .with_path(project_root.display().to_string())
    })?;
    let manifest = load_manifest(&project_root)?;
    validate_lockfile(&project_root, &manifest)?;
    let package = manifest.package.as_ref().ok_or_else(|| {
        Diagnostic::new("publish", "published packages require a [package] section")
            .with_path(manifest_path(&project_root).display().to_string())
    })?;
    let package_segment = safe_registry_path_segment("package name", &package.name)?;
    let version_segment = safe_registry_path_segment("package version", &package.version)?;
    let release_dir = registry_root.join(package_segment).join(version_segment);
    if release_dir.exists() && !options.allow_overwrite {
        return Err(Diagnostic::new(
            "publish",
            format!(
                "registry release {}@{} already exists; pass --allow-overwrite to replace it",
                package.name, package.version
            ),
        )
        .with_path(release_dir.display().to_string()));
    }
    fs::create_dir_all(&release_dir).map_err(|err| {
        Diagnostic::new(
            "publish",
            format!(
                "failed to create release directory {}: {err}",
                release_dir.display()
            ),
        )
        .with_path(release_dir.display().to_string())
    })?;

    let manifest_out = release_dir.join(MANIFEST_FILENAME);
    fs::copy(project_root.join(MANIFEST_FILENAME), &manifest_out).map_err(|err| {
        Diagnostic::new(
            "publish",
            format!("failed to copy {MANIFEST_FILENAME}: {err}"),
        )
        .with_path(manifest_out.display().to_string())
    })?;
    let lock_out = release_dir.join(LOCK_FILENAME);
    fs::copy(project_root.join(LOCK_FILENAME), &lock_out).map_err(|err| {
        Diagnostic::new("publish", format!("failed to copy {LOCK_FILENAME}: {err}"))
            .with_path(lock_out.display().to_string())
    })?;

    let archive_bytes = render_package_archive(&project_root)?;
    let archive_hash = hash_bytes(&archive_bytes);
    let archive_out = release_dir.join(DEFAULT_ARCHIVE_FILENAME);
    fs::write(&archive_out, &archive_bytes).map_err(|err| {
        Diagnostic::new("publish", format!("failed to write package archive: {err}"))
            .with_path(archive_out.display().to_string())
    })?;
    let signature =
        render_archive_signature(&package.name, &package.version, &archive_hash, signing_key);
    let signature_out = release_dir.join(format!("{DEFAULT_ARCHIVE_FILENAME}.sig"));
    fs::write(&signature_out, signature).map_err(|err| {
        Diagnostic::new(
            "publish",
            format!("failed to write package signature: {err}"),
        )
        .with_path(signature_out.display().to_string())
    })?;

    Ok(PublishOutput {
        package: package.name.clone(),
        version: package.version.clone(),
        release_dir: release_dir.display().to_string(),
        manifest: manifest_out.display().to_string(),
        archive: archive_out.display().to_string(),
        signature: signature_out.display().to_string(),
        archive_hash,
    })
}

fn render_package_archive(project_root: &Path) -> Result<Vec<u8>, Diagnostic> {
    let mut files = publishable_files(project_root)?;
    files.sort();
    let mut archive = Vec::new();
    archive.extend_from_slice(b"AXIOM_PACKAGE_ARCHIVE_V1\n");
    for path in files {
        let relative = path.strip_prefix(project_root).unwrap_or(&path);
        let relative = normalize_archive_path(relative)?;
        let content = fs::read(&path).map_err(|err| {
            Diagnostic::new(
                "publish",
                format!("failed to read {}: {err}", path.display()),
            )
            .with_path(path.display().to_string())
        })?;
        archive
            .extend_from_slice(format!("--- file {relative} {} ---\n", content.len()).as_bytes());
        archive.extend_from_slice(&content);
        if !content.ends_with(b"\n") {
            archive.push(b'\n');
        }
    }
    Ok(archive)
}

fn publishable_files(project_root: &Path) -> Result<Vec<PathBuf>, Diagnostic> {
    let mut files = Vec::new();
    collect_publishable_files(project_root, project_root, &mut files)?;
    Ok(files)
}

fn collect_publishable_files(
    project_root: &Path,
    dir: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), Diagnostic> {
    for entry in fs::read_dir(dir).map_err(|err| {
        Diagnostic::new(
            "publish",
            format!("failed to read {}: {err}", dir.display()),
        )
        .with_path(dir.display().to_string())
    })? {
        let entry = entry.map_err(|err| {
            Diagnostic::new("publish", format!("failed to read directory entry: {err}"))
        })?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let metadata = fs::symlink_metadata(&path).map_err(|err| {
            Diagnostic::new(
                "publish",
                format!("failed to stat {}: {err}", path.display()),
            )
            .with_path(path.display().to_string())
        })?;
        if metadata.file_type().is_symlink() {
            return Err(Diagnostic::new(
                "publish",
                format!(
                    "refusing to package symlinked path {} -- publish does not follow symlinks",
                    path.display()
                ),
            )
            .with_path(path.display().to_string()));
        }
        if metadata.is_dir() {
            if matches!(name.as_ref(), ".git" | "target" | "dist") {
                continue;
            }
            collect_publishable_files(project_root, &path, files)?;
        } else if metadata.is_file() && should_publish_file(&path) {
            let canonical = fs::canonicalize(&path).map_err(|err| {
                Diagnostic::new(
                    "publish",
                    format!("failed to resolve {}: {err}", path.display()),
                )
                .with_path(path.display().to_string())
            })?;
            if !canonical.starts_with(project_root) {
                return Err(Diagnostic::new(
                    "publish",
                    format!(
                        "refusing to package {} -- resolves outside the project root {}",
                        path.display(),
                        project_root.display()
                    ),
                )
                .with_path(path.display().to_string()));
            }
            files.push(canonical);
        }
    }
    Ok(())
}

fn should_publish_file(path: &Path) -> bool {
    if path
        .file_name()
        .is_some_and(|name| name == MANIFEST_FILENAME || name == LOCK_FILENAME)
    {
        return true;
    }
    path.extension().is_some_and(|extension| extension == "ax")
}

fn normalize_archive_path(path: &Path) -> Result<String, Diagnostic> {
    let mut out = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => {
                let value = value.to_str().ok_or_else(|| {
                    Diagnostic::new(
                        "publish",
                        format!(
                            "archive path component is not valid UTF-8 in {}",
                            path.display()
                        ),
                    )
                })?;
                if !is_safe_archive_component(value) {
                    return Err(Diagnostic::new(
                        "publish",
                        format!(
                            "unsafe archive path component {value:?} in {}",
                            path.display()
                        ),
                    ));
                }
                out.push(value.to_string());
            }
            _ => {
                return Err(Diagnostic::new(
                    "publish",
                    format!("unsupported archive path component in {}", path.display()),
                ));
            }
        }
    }
    if out.is_empty() {
        return Err(Diagnostic::new(
            "publish",
            format!(
                "archive path must name a descendant file: {}",
                path.display()
            ),
        ));
    }
    Ok(out.join("/"))
}

fn is_safe_archive_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains('\0')
        && !value.contains('/')
        && !value.contains('\\')
        && !looks_like_windows_drive_component(value)
}

fn looks_like_windows_drive_component(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn render_archive_signature(
    package: &str,
    version: &str,
    archive_hash: &str,
    signing_key: &str,
) -> String {
    let integrity = compute_authentication_tag(signing_key, package, version, archive_hash);
    format!(
        "{ARCHIVE_AUTH_HEADER}\npackage={package}\nversion={version}\narchive_hash={archive_hash}\nhmac_sha256={integrity}\n"
    )
}

fn compute_authentication_tag(
    signing_key: &str,
    package: &str,
    version: &str,
    archive_hash: &str,
) -> String {
    hmac_sha256_hex(
        signing_key.as_bytes(),
        format!("{package}\0{version}\0{archive_hash}").as_bytes(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArchiveAuthentication {
    package: String,
    version: String,
    archive_hash: String,
    hmac_sha256: String,
}

fn parse_archive_authentication(payload: &str) -> Result<ArchiveAuthentication, Diagnostic> {
    let mut header = None;
    let mut declared_package = None;
    let mut declared_version = None;
    let mut declared_hash = None;
    let mut declared_hmac = None;
    for line in payload.lines() {
        if header.is_none() {
            header = Some(line);
            continue;
        }
        if line.is_empty() {
            return Err(Diagnostic::new(
                "registry",
                "signature payload contains an empty line",
            ));
        }
        if let Some((key, value)) = line.split_once('=') {
            match key {
                "package" => {
                    if declared_package.is_some() {
                        return Err(Diagnostic::new(
                            "registry",
                            "signature payload repeats package",
                        ));
                    }
                    declared_package = Some(value.to_string())
                }
                "version" => {
                    if declared_version.is_some() {
                        return Err(Diagnostic::new(
                            "registry",
                            "signature payload repeats version",
                        ));
                    }
                    declared_version = Some(value.to_string())
                }
                "archive_hash" => {
                    if declared_hash.is_some() {
                        return Err(Diagnostic::new(
                            "registry",
                            "signature payload repeats archive_hash",
                        ));
                    }
                    declared_hash = Some(value.to_string())
                }
                "hmac_sha256" => {
                    if declared_hmac.is_some() {
                        return Err(Diagnostic::new(
                            "registry",
                            "signature payload repeats hmac_sha256",
                        ));
                    }
                    declared_hmac = Some(value.to_string())
                }
                _ => {
                    return Err(Diagnostic::new(
                        "registry",
                        format!("signature payload contains unexpected field {key}"),
                    ));
                }
            }
        } else {
            return Err(Diagnostic::new(
                "registry",
                "signature payload contains an invalid line",
            ));
        }
    }
    if header != Some(ARCHIVE_AUTH_HEADER) {
        return Err(Diagnostic::new(
            "registry",
            format!("signature payload missing {ARCHIVE_AUTH_HEADER} header"),
        ));
    }
    Ok(ArchiveAuthentication {
        package: declared_package
            .ok_or_else(|| Diagnostic::new("registry", "signature payload missing package"))?,
        version: declared_version
            .ok_or_else(|| Diagnostic::new("registry", "signature payload missing version"))?,
        archive_hash: declared_hash
            .ok_or_else(|| Diagnostic::new("registry", "signature payload missing archive_hash"))?,
        hmac_sha256: declared_hmac
            .ok_or_else(|| Diagnostic::new("registry", "signature payload missing hmac_sha256"))?,
    })
}

/// Re-derive the archive authentication tag and compare it against the emitted
/// `.sig` payload. Returns Ok(()) when the HMAC binds the same archive bytes,
/// package identity, and version under the supplied key.
pub fn verify_archive_integrity(
    package: &str,
    version: &str,
    archive_bytes: &[u8],
    signature_payload: &str,
    signing_key: &str,
) -> Result<(), Diagnostic> {
    let auth = parse_archive_authentication(signature_payload)?;
    if auth.package != package || auth.version != version {
        return Err(Diagnostic::new(
            "registry",
            format!("signature payload does not match package {package}@{version}"),
        ));
    }
    let actual_hash = hash_bytes(archive_bytes);
    if auth.archive_hash != actual_hash {
        return Err(Diagnostic::new(
            "registry",
            format!(
                "archive hash mismatch: payload declares {}, archive hashes to {actual_hash}",
                auth.archive_hash
            ),
        ));
    }
    let expected_hmac = compute_authentication_tag(signing_key, package, version, &actual_hash);
    if !constant_time_eq_hex(&auth.hmac_sha256, &expected_hmac) {
        return Err(Diagnostic::new(
            "registry",
            "archive authentication tag does not match supplied signing key",
        ));
    }
    Ok(())
}

fn verify_archive_attestation(
    package: &str,
    version: &str,
    archive_bytes: &[u8],
    signature_payload: &str,
    signing_key: &str,
) -> Result<(), Diagnostic> {
    if signing_key.trim().is_empty() {
        return Err(Diagnostic::new(
            "registry",
            "--signing-key must not be empty when verifying registry archive authentication",
        ));
    }
    verify_archive_integrity(
        package,
        version,
        archive_bytes,
        signature_payload,
        signing_key,
    )
}

fn hash_bytes(value: &[u8]) -> String {
    hex_bytes(&sha256(value))
}

fn hmac_sha256_hex(key: &[u8], message: &[u8]) -> String {
    let mut normalized_key = [0u8; SHA256_BLOCK_LEN];
    if key.len() > SHA256_BLOCK_LEN {
        normalized_key[..32].copy_from_slice(&sha256(key));
    } else {
        normalized_key[..key.len()].copy_from_slice(key);
    }

    let mut outer_key_pad = [0x5cu8; SHA256_BLOCK_LEN];
    let mut inner_key_pad = [0x36u8; SHA256_BLOCK_LEN];
    for index in 0..SHA256_BLOCK_LEN {
        outer_key_pad[index] ^= normalized_key[index];
        inner_key_pad[index] ^= normalized_key[index];
    }

    let mut inner = Vec::with_capacity(SHA256_BLOCK_LEN + message.len());
    inner.extend_from_slice(&inner_key_pad);
    inner.extend_from_slice(message);
    let inner_hash = sha256(&inner);

    let mut outer = Vec::with_capacity(SHA256_BLOCK_LEN + inner_hash.len());
    outer.extend_from_slice(&outer_key_pad);
    outer.extend_from_slice(&inner_hash);
    hex_bytes(&sha256(&outer))
}

fn constant_time_eq_hex(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut diff = left.len() ^ right.len();
    let max_len = left.len().max(right.len());
    for index in 0..max_len {
        let left_byte = left.get(index).copied().unwrap_or(0);
        let right_byte = right.get(index).copied().unwrap_or(0);
        diff |= usize::from(left_byte ^ right_byte);
    }
    diff == 0
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

fn sha256(input: &[u8]) -> [u8; 32] {
    const INITIAL: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut message = Vec::with_capacity(input.len() + 72);
    message.extend_from_slice(input);
    message.push(0x80);
    while (message.len() % 64) != 56 {
        message.push(0);
    }
    let bit_len = (input.len() as u64).wrapping_mul(8);
    message.extend_from_slice(&bit_len.to_be_bytes());

    let mut state = INITIAL;
    for chunk in message.chunks_exact(64) {
        let mut words = [0u32; 64];
        for (index, word) in words.iter_mut().take(16).enumerate() {
            let offset = index * 4;
            *word = u32::from_be_bytes([
                chunk[offset],
                chunk[offset + 1],
                chunk[offset + 2],
                chunk[offset + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7)
                ^ words[index - 15].rotate_right(18)
                ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17)
                ^ words[index - 2].rotate_right(19)
                ^ (words[index - 2] >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }

        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];

        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(words[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }

    let mut output = [0u8; 32];
    for (index, value) in state.iter().enumerate() {
        output[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    output
}

pub fn build_registry_index(
    packages_root: &Path,
    base_url: &str,
    signing_key: &str,
) -> Result<RegistryIndex, Diagnostic> {
    if signing_key.trim().is_empty() {
        return Err(Diagnostic::new(
            "registry",
            "--signing-key must not be empty when building a registry index",
        ));
    }
    let base_url = normalize_base_url(base_url, packages_root)?;
    let mut packages = BTreeMap::new();
    for package_dir in read_sorted_dirs(packages_root)? {
        let package_name = file_name(&package_dir)?;
        let mut releases = Vec::new();
        for version_dir in read_sorted_dirs(&package_dir)? {
            let release = load_release(&package_name, &version_dir, &base_url, signing_key)?;
            releases.push(release);
        }
        if !releases.is_empty() {
            packages.insert(package_name, releases);
        }
    }
    Ok(RegistryIndex {
        version: 1,
        packages,
    })
}

pub fn render_registry_index(
    packages_root: &Path,
    base_url: &str,
    signing_key: &str,
) -> Result<String, Diagnostic> {
    let index = build_registry_index(packages_root, base_url, signing_key)?;
    render_registry_index_json(&index)
}

fn render_registry_index_json(index: &RegistryIndex) -> Result<String, Diagnostic> {
    serde_json::to_string_pretty(index).map_err(|err| {
        Diagnostic::new(
            "registry",
            format!("failed to render registry index: {err}"),
        )
    })
}

impl RegistryServeContext {
    fn new(packages_root: &Path, base_url: &str, signing_key: &str) -> Result<Self, Diagnostic> {
        // The hosted registry is a static snapshot: startup validates the whole
        // registry once, and requests serve that validated view.
        let index = build_registry_index(packages_root, base_url, signing_key)?;
        let index_body = render_registry_index_json(&index)?.into_bytes();
        Ok(Self {
            packages_root: packages_root.to_path_buf(),
            index,
            index_body,
        })
    }
}

pub fn load_registry_index(path: &Path) -> Result<RegistryIndex, Diagnostic> {
    let content = fs::read_to_string(path).map_err(|err| {
        Diagnostic::new("registry", format!("failed to read registry index: {err}"))
            .with_path(path.display().to_string())
    })?;
    let index: RegistryIndex = serde_json::from_str(&content).map_err(|err| {
        Diagnostic::new("registry", format!("invalid registry index: {err}"))
            .with_path(path.display().to_string())
    })?;
    validate_registry_index(&index, Some(path))?;
    Ok(index)
}

pub fn serve_registry(
    packages_root: &Path,
    options: &RegistryServeOptions,
) -> Result<RegistryServeOutput, Diagnostic> {
    let listener = TcpListener::bind(&options.addr).map_err(|err| {
        Diagnostic::new(
            "registry",
            format!("failed to bind registry server {}: {err}", options.addr),
        )
    })?;
    let local_addr = listener.local_addr().map_err(|err| {
        Diagnostic::new(
            "registry",
            format!("failed to inspect registry bind addr: {err}"),
        )
    })?;
    let addr = local_addr.to_string();
    let base_url = options
        .base_url
        .clone()
        .unwrap_or_else(|| format!("http://{addr}"));
    let context = RegistryServeContext::new(packages_root, &base_url, &options.signing_key)?;

    eprintln!(
        "serving registry {} at {}",
        packages_root.display(),
        base_url
    );

    let mut requests = 0usize;
    for stream in listener.incoming() {
        let mut stream = stream.map_err(|err| {
            Diagnostic::new(
                "registry",
                format!("failed to accept registry request: {err}"),
            )
        })?;
        serve_registry_stream(&context, &mut stream)?;
        requests += 1;
        if options.once {
            break;
        }
    }

    Ok(RegistryServeOutput {
        addr,
        base_url,
        requests,
    })
}

fn serve_registry_stream(
    context: &RegistryServeContext,
    stream: &mut TcpStream,
) -> Result<(), Diagnostic> {
    let mut buffer = [0u8; 16 * 1024];
    let len = stream.read(&mut buffer).map_err(|err| {
        Diagnostic::new(
            "registry",
            format!("failed to read registry request: {err}"),
        )
    })?;
    let request = String::from_utf8_lossy(&buffer[..len]);
    let response = registry_http_response(context, &request)?;
    write_registry_http_response(stream, &response)
}

fn registry_http_response<'a>(
    context: &'a RegistryServeContext,
    request: &str,
) -> Result<RegistryHttpResponse<'a>, Diagnostic> {
    let Some(request_line) = request.lines().next() else {
        return Ok(registry_http_error("400 Bad Request", "empty request"));
    };
    let mut parts = request_line.split_whitespace();
    let Some(method) = parts.next() else {
        return Ok(registry_http_error("400 Bad Request", "missing method"));
    };
    let Some(target) = parts.next() else {
        return Ok(registry_http_error("400 Bad Request", "missing target"));
    };
    let Some(version) = parts.next() else {
        return Ok(registry_http_error("400 Bad Request", "missing version"));
    };
    if !version.starts_with("HTTP/") {
        return Ok(registry_http_error("400 Bad Request", "invalid version"));
    }
    if method != "GET" && method != "HEAD" {
        return Ok(RegistryHttpResponse {
            status: "405 Method Not Allowed",
            content_type: "text/plain; charset=utf-8",
            body: Cow::Borrowed(b"method not allowed\n"),
        });
    }

    let target = target.split('?').next().unwrap_or(target);
    let mut response = if target == "/" || target == "/index.json" {
        RegistryHttpResponse {
            status: "200 OK",
            content_type: "application/json",
            body: Cow::Borrowed(&context.index_body),
        }
    } else {
        registry_release_file_response(context, target)?
    };
    if method == "HEAD" {
        response.body = Cow::Borrowed(b"");
    }
    Ok(response)
}

fn registry_release_file_response<'a>(
    context: &'a RegistryServeContext,
    target: &str,
) -> Result<RegistryHttpResponse<'a>, Diagnostic> {
    let Some(relative) = target.strip_prefix('/') else {
        return Ok(registry_http_error(
            "400 Bad Request",
            "target must be absolute",
        ));
    };
    let segments = relative.split('/').collect::<Vec<_>>();
    if segments.len() != 3 {
        return Ok(registry_http_error("404 Not Found", "not found"));
    }
    let package = match safe_registry_lookup_segment("package name", segments[0]) {
        Ok(value) => value,
        Err(_) => {
            return Ok(registry_http_error(
                "400 Bad Request",
                "unsafe package path segment",
            ));
        }
    };
    let version = match safe_registry_lookup_segment("package version", segments[1]) {
        Ok(value) => value,
        Err(_) => {
            return Ok(registry_http_error(
                "400 Bad Request",
                "unsafe version path segment",
            ));
        }
    };
    let file = match safe_registry_lookup_segment("artifact file name", segments[2]) {
        Ok(value) => value,
        Err(_) => {
            return Ok(registry_http_error(
                "400 Bad Request",
                "unsafe artifact path segment",
            ));
        }
    };
    let Some(release) = context
        .index
        .packages
        .get(&package)
        .and_then(|releases| releases.iter().find(|release| release.version == version))
    else {
        return Ok(registry_http_error("404 Not Found", "not found"));
    };

    if !registry_release_allows_file(release, &file)? {
        return Ok(registry_http_error("404 Not Found", "not found"));
    }

    let path = context
        .packages_root
        .join(&package)
        .join(&version)
        .join(&file);
    let body = read_registry_artifact(&context.packages_root, &path)?;
    Ok(RegistryHttpResponse {
        status: "200 OK",
        content_type: registry_content_type(&file),
        body: Cow::Owned(body),
    })
}

fn read_registry_artifact(packages_root: &Path, path: &Path) -> Result<Vec<u8>, Diagnostic> {
    let root = packages_root.canonicalize().map_err(|err| {
        Diagnostic::new(
            "registry",
            format!(
                "failed to canonicalize registry root {}: {err}",
                packages_root.display()
            ),
        )
        .with_path(packages_root.display().to_string())
    })?;
    let metadata = fs::symlink_metadata(path).map_err(|err| {
        Diagnostic::new(
            "registry",
            format!(
                "failed to inspect registry artifact {}: {err}",
                path.display()
            ),
        )
        .with_path(path.display().to_string())
    })?;
    if metadata.file_type().is_symlink() {
        return Err(Diagnostic::new(
            "registry",
            format!(
                "refusing to serve symlinked registry artifact {}",
                path.display()
            ),
        )
        .with_path(path.display().to_string()));
    }
    let canonical = path.canonicalize().map_err(|err| {
        Diagnostic::new(
            "registry",
            format!(
                "failed to canonicalize registry artifact {}: {err}",
                path.display()
            ),
        )
        .with_path(path.display().to_string())
    })?;
    if !canonical.starts_with(&root) {
        return Err(Diagnostic::new(
            "registry",
            format!(
                "refusing to serve registry artifact outside registry root {}",
                canonical.display()
            ),
        )
        .with_path(canonical.display().to_string()));
    }
    let body = fs::read(&canonical).map_err(|err| {
        Diagnostic::new(
            "registry",
            format!("failed to read registry artifact {}: {err}", path.display()),
        )
        .with_path(canonical.display().to_string())
    })?;
    Ok(body)
}

fn registry_release_allows_file(release: &RegistryRelease, file: &str) -> Result<bool, Diagnostic> {
    if file == MANIFEST_FILENAME || file == LOCK_FILENAME {
        return Ok(true);
    }
    if let Some(archive) = release.archive.as_deref() {
        if registry_artifact_file_name("archive file name", archive)? == file {
            return Ok(true);
        }
    }
    if let Some(signature) = release.signature.as_deref() {
        if registry_artifact_file_name("signature file name", signature)? == file {
            return Ok(true);
        }
    }
    Ok(false)
}

fn registry_content_type(file: &str) -> &'static str {
    match file {
        MANIFEST_FILENAME | LOCK_FILENAME => "application/toml; charset=utf-8",
        "package.axp.sig" => "text/plain; charset=utf-8",
        _ if file.ends_with(".json") => "application/json",
        _ => "application/octet-stream",
    }
}

fn registry_http_error(status: &'static str, message: &str) -> RegistryHttpResponse<'static> {
    RegistryHttpResponse {
        status,
        content_type: "text/plain; charset=utf-8",
        body: Cow::Owned(format!("{message}\n").into_bytes()),
    }
}

fn write_registry_http_response(
    stream: &mut TcpStream,
    response: &RegistryHttpResponse<'_>,
) -> Result<(), Diagnostic> {
    write!(
        stream,
        "HTTP/1.1 {}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        response.status,
        response.content_type,
        response.body.len()
    )
    .map_err(|err| Diagnostic::new("registry", format!("failed to write response: {err}")))?;
    stream
        .write_all(&response.body)
        .map_err(|err| Diagnostic::new("registry", format!("failed to write response: {err}")))
}

pub fn validate_registry_index(
    index: &RegistryIndex,
    path: Option<&Path>,
) -> Result<(), Diagnostic> {
    if index.version != 1 {
        return Err(registry_error(
            path,
            format!(
                "unsupported registry index version {}; expected 1",
                index.version
            ),
        ));
    }
    for (package, releases) in &index.packages {
        if package.trim().is_empty() {
            return Err(registry_error(path, "package names must not be empty"));
        }
        let mut seen_versions = std::collections::BTreeSet::new();
        for release in releases {
            if release.version.trim().is_empty() {
                return Err(registry_error(
                    path,
                    format!("package {package:?} contains an empty version string"),
                ));
            }
            if !seen_versions.insert(release.version.clone()) {
                return Err(registry_error(
                    path,
                    format!(
                        "package {package:?} contains duplicate version {:?}",
                        release.version
                    ),
                ));
            }
            if release.archive.is_some() && release.signature.is_none() {
                return Err(registry_error(
                    path,
                    format!(
                        "package {package:?} version {:?} declares an archive without a signature",
                        release.version
                    ),
                ));
            }
            if release.yank_reason.is_some() && !release.yanked {
                return Err(registry_error(
                    path,
                    format!(
                        "package {package:?} version {:?} has yank_reason but is not yanked",
                        release.version
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Verify local release archives listed by a registry index against their
/// `axiom-hmac-sha256-v1` sidecars using the supplied stage1 authentication key.
pub fn verify_registry_index_integrity(
    index: &RegistryIndex,
    packages_root: &Path,
    signing_key: &str,
) -> Result<(), Diagnostic> {
    if signing_key.trim().is_empty() {
        return Err(Diagnostic::new(
            "registry",
            "--signing-key must not be empty when verifying registry integrity",
        ));
    }
    validate_registry_index(index, None)?;

    for (package, releases) in &index.packages {
        let package_segment = safe_registry_lookup_segment("package name", package)?;
        for release in releases {
            let Some(archive_location) = release.archive.as_deref() else {
                continue;
            };
            let signature_location = release.signature.as_deref().ok_or_else(|| {
                Diagnostic::new(
                    "registry",
                    format!(
                        "package {package:?} version {:?} declares an archive without a signature",
                        release.version
                    ),
                )
            })?;
            let version_segment =
                safe_registry_lookup_segment("package version", &release.version)?;
            let archive_file = registry_artifact_file_name("archive file name", archive_location)?;
            let signature_file =
                registry_artifact_file_name("signature file name", signature_location)?;
            let release_dir = packages_root.join(&package_segment).join(version_segment);
            let archive_path = release_dir.join(archive_file);
            let signature_path = release_dir.join(signature_file);
            let archive_bytes = fs::read(&archive_path).map_err(|err| {
                registry_error(
                    Some(&archive_path),
                    format!(
                        "failed to read registry archive for {package}@{}: {err}",
                        release.version
                    ),
                )
            })?;
            let signature_payload = fs::read_to_string(&signature_path).map_err(|err| {
                registry_error(
                    Some(&signature_path),
                    format!(
                        "failed to read registry signature for {package}@{}: {err}",
                        release.version
                    ),
                )
            })?;
            verify_archive_integrity(
                package,
                &release.version,
                &archive_bytes,
                &signature_payload,
                signing_key,
            )
            .map_err(|error| {
                registry_error(
                    Some(&signature_path),
                    format!(
                        "registry release {package}@{} failed integrity verification: {}",
                        release.version, error.message
                    ),
                )
            })?;
        }
    }

    Ok(())
}

fn load_release(
    package_name: &str,
    version_dir: &Path,
    base_url: &str,
    signing_key: &str,
) -> Result<RegistryRelease, Diagnostic> {
    let version = file_name(version_dir)?;
    let manifest = load_manifest(version_dir)?;
    let manifest_path = manifest_path(version_dir);
    let package = manifest.package.as_ref().ok_or_else(|| {
        Diagnostic::new(
            "registry",
            "registry release manifest requires a [package] section",
        )
        .with_path(manifest_path.display().to_string())
    })?;
    if package.name != package_name {
        return Err(Diagnostic::new(
            "registry",
            format!(
                "package directory {:?} does not match manifest package name {:?}",
                package_name, package.name
            ),
        )
        .with_path(manifest_path.display().to_string()));
    }
    if package.version != version {
        return Err(Diagnostic::new(
            "registry",
            format!(
                "version directory {:?} does not match manifest package version {:?}",
                version, package.version
            ),
        )
        .with_path(manifest_path.display().to_string()));
    }
    let metadata = load_registry_metadata(version_dir)?;
    let yanked = metadata.yanked.unwrap_or(false);
    if metadata.yank_reason.is_some() && !yanked {
        return Err(Diagnostic::new(
            "registry",
            format!(
                "registry release {package_name}@{version} declares yank_reason but is not yanked"
            ),
        )
        .with_path(
            version_dir
                .join(REGISTRY_METADATA_FILENAME)
                .display()
                .to_string(),
        ));
    }
    let archive_file = match metadata.archive {
        // Reduce the untrusted registry.toml artifact name to a validated
        // basename within version_dir (matching the consumer-side verifier) so a
        // crafted name such as "../../../../etc/passwd" cannot escape the package
        // tree when the path is later joined and read.
        Some(value) => Some(registry_artifact_file_name("archive file name", &value)?),
        None => version_dir
            .join(DEFAULT_ARCHIVE_FILENAME)
            .exists()
            .then(|| String::from(DEFAULT_ARCHIVE_FILENAME)),
    };
    let signature_file = match metadata.signature {
        Some(value) => Some(registry_artifact_file_name("signature file name", &value)?),
        None => archive_file.as_ref().and_then(|archive| {
            version_dir
                .join(format!("{archive}.sig"))
                .exists()
                .then(|| format!("{archive}.sig"))
        }),
    };
    if archive_file.is_some() ^ signature_file.is_some() {
        return Err(Diagnostic::new(
            "registry",
            format!(
                "registry release {package_name} must include both archive and signature sidecars"
            ),
        )
        .with_path(version_dir.display().to_string()));
    }
    if let (Some(archive_file), Some(signature_file)) = (&archive_file, &signature_file) {
        let archive_path = version_dir.join(archive_file);
        let signature_path = version_dir.join(signature_file);
        let archive_bytes = fs::read(&archive_path).map_err(|err| {
            Diagnostic::new(
                "registry",
                format!(
                    "failed to read registry archive {}: {err}",
                    archive_path.display()
                ),
            )
            .with_path(archive_path.display().to_string())
        })?;
        let signature_payload = fs::read_to_string(&signature_path).map_err(|err| {
            Diagnostic::new(
                "registry",
                format!(
                    "failed to read registry signature {}: {err}",
                    signature_path.display()
                ),
            )
            .with_path(signature_path.display().to_string())
        })?;
        verify_archive_attestation(
            package_name,
            &version,
            &archive_bytes,
            &signature_payload,
            signing_key,
        )?;
    }
    Ok(RegistryRelease {
        version: package.version.clone(),
        source: format!("registry+{}/{}/{}", base_url, package_name, version),
        manifest: format!("{}/{}/{}/axiom.toml", base_url, package_name, version),
        archive: archive_file
            .map(|file| format!("{}/{}/{}/{}", base_url, package_name, version, file)),
        signature: signature_file
            .map(|file| format!("{}/{}/{}/{}", base_url, package_name, version, file)),
        yanked,
        yank_reason: metadata.yank_reason,
        capabilities: capability_descriptors(&manifest.capabilities)
            .into_iter()
            .map(|capability| RegistryCapability {
                name: capability.name,
                enabled: capability.enabled,
                description: capability.description.to_string(),
                allowed: capability.allowed,
                unsafe_unrestricted: capability.unsafe_unrestricted,
            })
            .collect(),
    })
}

fn load_registry_metadata(version_dir: &Path) -> Result<RawRegistryMetadata, Diagnostic> {
    let path = version_dir.join(REGISTRY_METADATA_FILENAME);
    if !path.exists() {
        return Ok(RawRegistryMetadata::default());
    }
    let content = fs::read_to_string(&path).map_err(|err| {
        Diagnostic::new(
            "registry",
            format!("failed to read {REGISTRY_METADATA_FILENAME}: {err}"),
        )
        .with_path(path.display().to_string())
    })?;
    toml::from_str(&content).map_err(|err| {
        Diagnostic::new(
            "registry",
            format!("invalid {REGISTRY_METADATA_FILENAME}: {err}"),
        )
        .with_path(path.display().to_string())
    })
}

fn read_sorted_dirs(path: &Path) -> Result<Vec<PathBuf>, Diagnostic> {
    let mut dirs = fs::read_dir(path)
        .map_err(|err| {
            Diagnostic::new(
                "registry",
                format!("failed to read {}: {err}", path.display()),
            )
            .with_path(path.display().to_string())
        })?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|entry| entry.is_dir())
        .collect::<Vec<_>>();
    dirs.sort();
    Ok(dirs)
}

fn file_name(path: &Path) -> Result<String, Diagnostic> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .ok_or_else(|| Diagnostic::new("registry", format!("invalid path {}", path.display())))
}

fn normalize_base_url(base_url: &str, packages_root: &Path) -> Result<String, Diagnostic> {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(Diagnostic::new("registry", "--base-url must not be empty")
            .with_path(packages_root.display().to_string()));
    }
    Ok(trimmed.to_string())
}

fn safe_registry_path_segment(kind: &str, value: &str) -> Result<String, Diagnostic> {
    safe_registry_segment("publish", kind, value)
}

fn safe_registry_lookup_segment(kind: &str, value: &str) -> Result<String, Diagnostic> {
    safe_registry_segment("registry", kind, value)
}

fn safe_registry_segment(category: &str, kind: &str, value: &str) -> Result<String, Diagnostic> {
    let trimmed = value.trim();
    if is_unsafe_registry_path_segment(value) {
        return Err(Diagnostic::new(
            category,
            format!("registry {kind} must be a safe path segment: {value:?}"),
        ));
    }
    Ok(trimmed.to_string())
}

fn registry_artifact_file_name(kind: &str, location: &str) -> Result<String, Diagnostic> {
    let file_name = location.rsplit('/').next().unwrap_or_default();
    if file_name.is_empty()
        || file_name
            .chars()
            .any(|ch| matches!(ch, '?' | '#' | ':' | '\0'))
    {
        return Err(Diagnostic::new(
            "registry",
            format!("registry {kind} must be a safe file name: {location:?}"),
        ));
    }
    safe_registry_lookup_segment(kind, file_name)
}

fn is_unsafe_registry_path_segment(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.is_empty()
        || trimmed != value
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || Path::new(trimmed)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
}

fn registry_error(path: Option<&Path>, message: impl Into<String>) -> Diagnostic {
    let diagnostic = Diagnostic::new("registry", message.into());
    if let Some(path) = path {
        diagnostic.with_path(path.display().to_string())
    } else {
        diagnostic
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn sha256_matches_known_test_vector() {
        assert_eq!(
            hash_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn hmac_sha256_matches_rfc4231_test_vector() {
        let key = [0x0bu8; 20];
        assert_eq!(
            hmac_sha256_hex(&key, b"Hi There"),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    fn write_release(root: &Path, package: &str, version: &str, manifest: &str) -> PathBuf {
        let dir = root.join(package).join(version);
        fs::create_dir_all(&dir).expect("create release dir");
        fs::write(dir.join("axiom.toml"), manifest).expect("write manifest");
        dir
    }

    fn write_publishable_project(root: &Path, package: &str, version: &str) -> PathBuf {
        let project = root.join(package);
        fs::create_dir_all(project.join("src")).expect("create project src");
        fs::write(
            project.join("axiom.toml"),
            format!(
                "[package]\nname = {package:?}\nversion = {version:?}\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n"
            ),
        )
        .expect("write manifest");
        fs::write(
            project.join("axiom.lock"),
            format!("version = 1\n\n[[package]]\nname = {package:?}\nversion = {version:?}\nsource = \"path\"\n"),
        )
        .expect("write lockfile");
        fs::write(project.join("src/main.ax"), "print \"hello\"\n").expect("write source");
        project
    }

    #[test]
    fn load_release_confines_traversal_archive_names_to_the_version_dir() {
        let dir = tempdir().expect("tempdir");
        let registry = dir.path().join("registry");
        // Real files outside the registry that a crafted archive/signature name
        // would reach if joined onto version_dir without basename sanitization.
        fs::write(dir.path().join("escape.axp"), b"OUTSIDE ARCHIVE")
            .expect("write outside archive");
        fs::write(dir.path().join("escape.axp.sig"), b"OUTSIDE SIGNATURE")
            .expect("write outside signature");
        // version_dir is <tmp>/registry/demo/1.0.0; "../../../escape.axp" escapes to <tmp>.
        let version_dir = write_release(
            &registry,
            "demo",
            "1.0.0",
            "[package]\nname = \"demo\"\nversion = \"1.0.0\"\n",
        );
        fs::write(
            version_dir.join(REGISTRY_METADATA_FILENAME),
            "archive = \"../../../escape.axp\"\nsignature = \"../../../escape.axp.sig\"\n",
        )
        .expect("write registry metadata");

        let error = build_registry_index(&registry, "https://packages.example.test", "test-key")
            .expect_err("traversal artifact names must be confined to the version dir");
        // The names are reduced to their basenames and read from inside version_dir
        // (which does not contain them), so the build fails at the contained archive
        // read rather than reading the outside files.
        assert!(
            error.message.contains("failed to read registry archive"),
            "expected a contained archive read failure, got: {}",
            error.message
        );
        assert!(
            !error.message.contains("escape.axp.sig"),
            "read must fail at the archive and never reach the outside signature: {}",
            error.message
        );
    }

    #[test]
    fn publishes_package_archive_signature_and_registry_index_release() {
        let dir = tempdir().expect("tempdir");
        let project = write_publishable_project(dir.path(), "core", "1.0.0");
        let registry = dir.path().join("registry");

        let output = publish_package(
            &project,
            &registry,
            &PublishOptions {
                signing_key: Some(String::from("test-key")),
                allow_overwrite: false,
            },
        )
        .expect("publish package");

        assert_eq!(output.package, "core");
        assert_eq!(output.version, "1.0.0");
        let release = registry.join("core").join("1.0.0");
        assert!(release.join("axiom.toml").exists());
        assert!(release.join("axiom.lock").exists());
        let archive = fs::read_to_string(release.join("package.axp")).expect("read archive");
        assert!(archive.contains("AXIOM_PACKAGE_ARCHIVE_V1"));
        assert!(archive.contains("--- file src/main.ax"));
        let signature =
            fs::read_to_string(release.join("package.axp.sig")).expect("read signature");
        assert!(signature.contains(ARCHIVE_AUTH_HEADER));
        assert!(signature.contains(&format!("archive_hash={}", output.archive_hash)));
        let archive_bytes = fs::read(release.join("package.axp")).expect("read archive bytes");
        verify_archive_integrity("core", "1.0.0", &archive_bytes, &signature, "test-key")
            .expect("archive authentication verifies under publishing key");

        let index = build_registry_index(&registry, "https://packages.example.test", "test-key")
            .expect("build registry index");
        let release = &index.packages["core"][0];
        assert_eq!(
            release.archive.as_deref(),
            Some("https://packages.example.test/core/1.0.0/package.axp")
        );
        assert_eq!(
            release.signature.as_deref(),
            Some("https://packages.example.test/core/1.0.0/package.axp.sig")
        );
    }

    #[test]
    fn publish_rejects_existing_release_without_overwrite() {
        let dir = tempdir().expect("tempdir");
        let project = write_publishable_project(dir.path(), "core", "1.0.0");
        let registry = dir.path().join("registry");
        let opts = PublishOptions {
            signing_key: Some(String::from("test-key")),
            allow_overwrite: false,
        };
        publish_package(&project, &registry, &opts).expect("initial publish");

        let error =
            publish_package(&project, &registry, &opts).expect_err("duplicate publish should fail");

        assert_eq!(error.kind, "publish");
        assert!(error.message.contains("already exists"));
    }

    #[test]
    fn safe_registry_segments_share_rule_but_keep_diagnostic_category() {
        assert_eq!(
            safe_registry_path_segment("package name", "core").expect("safe publish segment"),
            "core"
        );
        assert_eq!(
            safe_registry_lookup_segment("package name", "core").expect("safe lookup segment"),
            "core"
        );

        let publish_error = safe_registry_path_segment("package name", "../core")
            .expect_err("publish segment traversal should fail");
        let registry_error = safe_registry_lookup_segment("package name", "../core")
            .expect_err("lookup segment traversal should fail");

        assert_eq!(publish_error.kind, "publish");
        assert_eq!(registry_error.kind, "registry");
        assert_eq!(publish_error.message, registry_error.message);
    }

    #[test]
    fn publish_requires_explicit_signing_key() {
        let dir = tempdir().expect("tempdir");
        let project = write_publishable_project(dir.path(), "core", "1.0.0");
        let registry = dir.path().join("registry");

        let error = publish_package(&project, &registry, &PublishOptions::default())
            .expect_err("missing signing key should fail");

        assert_eq!(error.kind, "publish");
        assert!(error.message.contains("--signing-key"));
        assert!(!registry.exists(), "registry tree must not be created");
    }

    #[test]
    fn registry_index_requires_nonempty_signing_key() {
        let dir = tempdir().expect("tempdir");

        let error = build_registry_index(dir.path(), "https://packages.example.test", "   ")
            .expect_err("empty signing key should fail");

        assert_eq!(error.kind, "registry");
        assert!(error.message.contains("--signing-key"));
    }

    #[test]
    fn verify_archive_integrity_rejects_tampered_archive() {
        let dir = tempdir().expect("tempdir");
        let project = write_publishable_project(dir.path(), "core", "1.0.0");
        let registry = dir.path().join("registry");
        publish_package(
            &project,
            &registry,
            &PublishOptions {
                signing_key: Some(String::from("test-key")),
                allow_overwrite: false,
            },
        )
        .expect("publish package");
        let release = registry.join("core").join("1.0.0");
        let signature = fs::read_to_string(release.join("package.axp.sig")).expect("read sig");

        let tampered = b"AXIOM_PACKAGE_ARCHIVE_V1\n--- file evil.ax 4 ---\nevil\n";
        let error = verify_archive_integrity("core", "1.0.0", tampered, &signature, "test-key")
            .expect_err("tampered archive should fail");
        assert!(error.message.contains("archive hash mismatch"));

        let original = fs::read(release.join("package.axp")).expect("read archive");
        let error = verify_archive_integrity("core", "1.0.0", &original, &signature, "wrong-key")
            .expect_err("wrong key should fail");
        assert!(
            error
                .message
                .contains("does not match supplied signing key")
        );
    }

    #[test]
    fn verifies_registry_index_integrity_for_local_release_artifacts() {
        let dir = tempdir().expect("tempdir");
        let project = write_publishable_project(dir.path(), "core", "1.0.0");
        let registry = dir.path().join("registry");
        publish_package(
            &project,
            &registry,
            &PublishOptions {
                signing_key: Some(String::from("test-key")),
                allow_overwrite: false,
            },
        )
        .expect("publish package");
        let index = build_registry_index(&registry, "https://packages.example.test", "test-key")
            .expect("build registry index");

        verify_registry_index_integrity(&index, &registry, "test-key")
            .expect("registry release artifacts verify");
    }

    #[test]
    fn registry_index_integrity_rejects_tampered_local_archive() {
        let dir = tempdir().expect("tempdir");
        let project = write_publishable_project(dir.path(), "core", "1.0.0");
        let registry = dir.path().join("registry");
        publish_package(
            &project,
            &registry,
            &PublishOptions {
                signing_key: Some(String::from("test-key")),
                allow_overwrite: false,
            },
        )
        .expect("publish package");
        let index = build_registry_index(&registry, "https://packages.example.test", "test-key")
            .expect("build registry index");
        fs::write(
            registry.join("core").join("1.0.0").join("package.axp"),
            b"AXIOM_PACKAGE_ARCHIVE_V1\n--- file tampered.ax 9 ---\ntampered\n",
        )
        .expect("tamper archive");

        let error = verify_registry_index_integrity(&index, &registry, "test-key")
            .expect_err("tampered archive should fail registry integrity validation");

        assert_eq!(error.kind, "registry");
        assert!(
            error
                .message
                .contains("failed integrity verification: archive hash mismatch")
        );
        assert!(
            error
                .path
                .as_deref()
                .is_some_and(|path| path.ends_with("package.axp.sig"))
        );
    }

    #[test]
    fn registry_http_rejects_symlinked_lockfile_artifact() {
        let dir = tempdir().expect("tempdir");
        let registry = dir.path().join("registry");
        let release = write_release(
            &registry,
            "core",
            "1.0.0",
            "[package]\nname = \"core\"\nversion = \"1.0.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n",
        );
        let outside = dir.path().join("outside-lock.toml");
        fs::write(&outside, "secret = true\n").expect("write outside file");
        let lock_path = release.join(LOCK_FILENAME);

        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &lock_path).expect("create symlink");
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&outside, &lock_path).expect("create symlink");

        let context =
            RegistryServeContext::new(&registry, "https://packages.example.test", "test-key")
                .expect("build serve context");
        let error = registry_http_response(&context, "GET /core/1.0.0/axiom.lock HTTP/1.1\r\n\r\n")
            .expect_err("symlinked lockfile should not be served");

        assert_eq!(error.kind, "registry");
        assert!(error.message.contains("symlinked registry artifact"));
    }

    #[test]
    fn registry_index_integrity_rejects_unsafe_artifact_locations() {
        let index = RegistryIndex {
            version: 1,
            packages: BTreeMap::from([(
                String::from("core"),
                vec![RegistryRelease {
                    version: String::from("1.0.0"),
                    source: String::from("registry+https://packages.example.test/core/1.0.0"),
                    manifest: String::from("https://packages.example.test/core/1.0.0/axiom.toml"),
                    archive: Some(String::from(
                        "https://packages.example.test/core/1.0.0/package.axp?download=1",
                    )),
                    signature: Some(String::from(
                        "https://packages.example.test/core/1.0.0/package.axp.sig",
                    )),
                    yanked: false,
                    yank_reason: None,
                    capabilities: Vec::new(),
                }],
            )]),
        };

        let error = verify_registry_index_integrity(&index, Path::new("."), "test-key")
            .expect_err("unsafe artifact location should fail before filesystem access");

        assert_eq!(error.kind, "registry");
        assert!(error.message.contains("archive file name"));
    }

    #[test]
    fn publish_rejects_symlinked_source_file() {
        let dir = tempdir().expect("tempdir");
        let project = write_publishable_project(dir.path(), "core", "1.0.0");
        let outside = dir.path().join("outside-secret.ax");
        fs::write(&outside, "secret\n").expect("write outside file");
        let link_path = project.join("src").join("leaked.ax");

        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link_path).expect("create symlink");
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&outside, &link_path).expect("create symlink");

        let registry = dir.path().join("registry");
        let error = publish_package(
            &project,
            &registry,
            &PublishOptions {
                signing_key: Some(String::from("test-key")),
                allow_overwrite: false,
            },
        )
        .expect_err("symlinked file should be rejected");

        assert_eq!(error.kind, "publish");
        assert!(error.message.contains("symlinked"));
    }

    #[test]
    fn archive_path_normalization_accepts_safe_descendant_paths() {
        assert_eq!(
            normalize_archive_path(Path::new("src/main.ax")).expect("safe archive path"),
            "src/main.ax"
        );
        assert_eq!(
            normalize_archive_path(Path::new("axiom.lock")).expect("safe archive path"),
            "axiom.lock"
        );
    }

    #[test]
    fn archive_path_normalization_rejects_unsafe_components() {
        for path in [
            "dir/../etc/passwd",
            "./foo",
            "/abs/path",
            "C:\\bad",
            "C:bad",
            "link/../escape",
            "bad\0name.ax",
            ".",
        ] {
            let error = normalize_archive_path(Path::new(path))
                .expect_err("unsafe archive path should fail");
            assert_eq!(error.kind, "publish");
            assert!(
                error.message.contains("archive path")
                    || error.message.contains("archive path component"),
                "unexpected error for {path:?}: {}",
                error.message
            );
        }
    }

    #[test]
    fn publish_rejects_traversal_package_name() {
        let dir = tempdir().expect("tempdir");
        let project = write_publishable_project(dir.path(), "../escaped-publish", "1.0.0");
        let registry = dir.path().join("registry");
        let opts = PublishOptions {
            signing_key: Some(String::from("test-key")),
            allow_overwrite: false,
        };

        let error = publish_package(&project, &registry, &opts)
            .expect_err("traversal package name should fail");

        assert_eq!(error.kind, "publish");
        assert!(error.message.contains("package name"));
        assert!(!dir.path().join("escaped-publish").exists());
    }

    #[test]
    fn publish_rejects_traversal_package_version() {
        let dir = tempdir().expect("tempdir");
        let project = write_publishable_project(dir.path(), "core", "../escaped-version");
        let registry = dir.path().join("registry");
        let opts = PublishOptions {
            signing_key: Some(String::from("test-key")),
            allow_overwrite: false,
        };

        let error = publish_package(&project, &registry, &opts)
            .expect_err("traversal package version should fail");

        assert_eq!(error.kind, "publish");
        assert!(error.message.contains("package version"));
        assert!(!dir.path().join("registry").join("escaped-version").exists());
        assert!(!dir.path().join("escaped-version").exists());
    }

    #[test]
    fn builds_static_registry_index_with_capabilities_and_yanks() {
        let dir = tempdir().expect("tempdir");
        let release = write_release(
            dir.path(),
            "core",
            "1.2.3",
            "[package]\nname = \"core\"\nversion = \"1.2.3\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nnet = true\nenv = [\"API_TOKEN\"]\n",
        );
        fs::write(release.join("package.axp"), "archive").expect("write archive");
        let archive_hash = hash_bytes(b"archive");
        fs::write(
            release.join("package.axp.sig"),
            render_archive_signature("core", "1.2.3", &archive_hash, "test-key"),
        )
        .expect("write signature");
        fs::write(
            release.join("axiom-registry.toml"),
            "yanked = true\nyank_reason = \"security fix required\"\n",
        )
        .expect("write metadata");

        let index = build_registry_index(
            dir.path(),
            "https://packages.example.test/registry/",
            "test-key",
        )
        .expect("build index");
        let release = &index.packages["core"][0];
        assert_eq!(
            release.source,
            "registry+https://packages.example.test/registry/core/1.2.3"
        );
        assert_eq!(
            release.archive.as_deref(),
            Some("https://packages.example.test/registry/core/1.2.3/package.axp")
        );
        assert_eq!(
            release.signature.as_deref(),
            Some("https://packages.example.test/registry/core/1.2.3/package.axp.sig")
        );
        assert!(release.yanked);
        assert_eq!(
            release.yank_reason.as_deref(),
            Some("security fix required")
        );
        assert!(
            release
                .capabilities
                .iter()
                .any(|cap| cap.name == "net" && cap.enabled)
        );
        let env = release
            .capabilities
            .iter()
            .find(|cap| cap.name == "env")
            .expect("env cap");
        assert_eq!(env.allowed, vec![String::from("API_TOKEN")]);
    }

    #[test]
    fn rejects_unsigned_archives() {
        let dir = tempdir().expect("tempdir");
        let release = write_release(
            dir.path(),
            "core",
            "1.0.0",
            "[package]\nname = \"core\"\nversion = \"1.0.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n",
        );
        fs::write(release.join("package.axp"), "archive").expect("write archive");
        let error = build_registry_index(dir.path(), "https://packages.example.test", "test-key")
            .expect_err("unsigned archive should fail");
        assert_eq!(error.kind, "registry");
        assert!(error.message.contains("archive"));
        assert!(error.message.contains("signature sidecars"));
    }

    #[test]
    fn rejects_invalid_archive_attestation_payload() {
        let dir = tempdir().expect("tempdir");
        let release = write_release(
            dir.path(),
            "core",
            "1.0.0",
            "[package]\nname = \"core\"\nversion = \"1.0.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n",
        );
        fs::write(release.join("package.axp"), "archive").expect("write archive");
        fs::write(
            release.join("package.axp.sig"),
            "axiom-hmac-sha256-v1\npackage=core\nversion=1.0.0\narchive_hash=deadbeef\nhmac_sha256=ignored\n",
        )
        .expect("write signature");

        let error = build_registry_index(dir.path(), "https://packages.example.test", "test-key")
            .expect_err("mismatched archive hash should fail");
        assert_eq!(error.kind, "registry");
        assert!(error.message.contains("archive hash mismatch"));
    }

    #[test]
    fn rejects_archive_authentication_payload_with_forged_hmac() {
        let dir = tempdir().expect("tempdir");
        let release = write_release(
            dir.path(),
            "core",
            "1.0.0",
            "[package]\nname = \"core\"\nversion = \"1.0.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n",
        );
        fs::write(release.join("package.axp"), "archive").expect("write archive");
        let archive_hash = hash_bytes(b"archive");
        fs::write(
            release.join("package.axp.sig"),
            format!(
                "{ARCHIVE_AUTH_HEADER}\npackage=core\nversion=1.0.0\narchive_hash={archive_hash}\nhmac_sha256=0000000000000000000000000000000000000000000000000000000000000000\n",
            ),
        )
        .expect("write signature");

        let error = build_registry_index(dir.path(), "https://packages.example.test", "test-key")
            .expect_err("forged HMAC should fail");

        assert_eq!(error.kind, "registry");
        assert!(
            error
                .message
                .contains("authentication tag does not match supplied signing key")
        );
    }

    #[test]
    fn rejects_archive_attestation_payload_without_integrity_field() {
        let dir = tempdir().expect("tempdir");
        let release = write_release(
            dir.path(),
            "core",
            "1.0.0",
            "[package]\nname = \"core\"\nversion = \"1.0.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n",
        );
        fs::write(release.join("package.axp"), "archive").expect("write archive");
        let archive_hash = hash_bytes(b"archive");
        fs::write(
            release.join("package.axp.sig"),
            format!(
                "{ARCHIVE_AUTH_HEADER}\npackage=core\nversion=1.0.0\narchive_hash={archive_hash}\n",
            ),
        )
        .expect("write signature");

        let error = build_registry_index(dir.path(), "https://packages.example.test", "test-key")
            .expect_err("missing hmac field should fail");
        assert_eq!(error.kind, "registry");
        assert!(error.message.contains("missing hmac_sha256"));
    }

    #[test]
    fn rejects_archive_attestation_payload_with_unexpected_field() {
        let dir = tempdir().expect("tempdir");
        let release = write_release(
            dir.path(),
            "core",
            "1.0.0",
            "[package]\nname = \"core\"\nversion = \"1.0.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n",
        );
        fs::write(release.join("package.axp"), "archive").expect("write archive");
        let archive_hash = hash_bytes(b"archive");
        fs::write(
            release.join("package.axp.sig"),
            format!(
                "{ARCHIVE_AUTH_HEADER}\npackage=core\nversion=1.0.0\narchive_hash={archive_hash}\nhmac_sha256=ignored\nunknown=field\n",
            ),
        )
        .expect("write signature");

        let error = build_registry_index(dir.path(), "https://packages.example.test", "test-key")
            .expect_err("unexpected field should fail");
        assert_eq!(error.kind, "registry");
        assert!(error.message.contains("unexpected field unknown"));
    }

    #[test]
    fn rejects_yank_reason_without_yanked_metadata() {
        let dir = tempdir().expect("tempdir");
        let release = write_release(
            dir.path(),
            "core",
            "1.0.0",
            "[package]\nname = \"core\"\nversion = \"1.0.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n",
        );
        fs::write(
            release.join("axiom-registry.toml"),
            "yank_reason = \"metadata drift\"\n",
        )
        .expect("write metadata");

        let error = build_registry_index(dir.path(), "https://packages.example.test", "test-key")
            .expect_err("yank_reason without yanked should fail");
        assert_eq!(error.kind, "registry");
        assert!(error.message.contains("yank_reason but is not yanked"));
        assert!(
            error
                .path
                .as_deref()
                .is_some_and(|path| path.ends_with("axiom-registry.toml"))
        );
    }

    #[test]
    fn validates_index_file_contract() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("index.json");
        fs::write(
            &path,
            r#"{"version":1,"packages":{"core":[{"version":"1.0.0","source":"registry+https://packages.example.test/core/1.0.0","manifest":"https://packages.example.test/core/1.0.0/axiom.toml","archive":"https://packages.example.test/core/1.0.0/package.axp","signature":"https://packages.example.test/core/1.0.0/package.axp.sig","yanked":false,"capabilities":[]}]}}"#,
        )
        .expect("write index");
        load_registry_index(&path).expect("valid index");
    }

    #[test]
    fn hosted_registry_serves_index_and_signed_artifacts() {
        let dir = tempdir().expect("tempdir");
        let project = write_publishable_project(dir.path(), "core", "1.0.0");
        let registry = dir.path().join("registry");
        publish_package(
            &project,
            &registry,
            &PublishOptions {
                signing_key: Some(String::from("dev-key")),
                allow_overwrite: false,
            },
        )
        .expect("publish package");
        fs::write(
            registry
                .join("core")
                .join("1.0.0")
                .join("axiom-registry.toml"),
            "yanked = true\nyank_reason = \"superseded\"\n",
        )
        .expect("write registry metadata");

        let context = RegistryServeContext::new(&registry, "http://registry.test", "dev-key")
            .expect("build serve context");
        let index = registry_http_response(
            &context,
            "GET /index.json HTTP/1.1\r\nHost: registry.test\r\n\r\n",
        )
        .expect("index response");
        assert_eq!(index.status, "200 OK");
        assert_eq!(index.content_type, "application/json");
        let index_text = String::from_utf8(index.body.into_owned()).expect("utf8 index");
        assert!(index_text.contains("\"core\""));
        assert!(index_text.contains("\"yanked\": true"));
        assert!(index_text.contains("\"yank_reason\": \"superseded\""));
        assert!(index_text.contains("http://registry.test/core/1.0.0/package.axp"));

        let archive = registry_http_response(
            &context,
            "GET /core/1.0.0/package.axp HTTP/1.1\r\nHost: registry.test\r\n\r\n",
        )
        .expect("archive response");
        assert_eq!(archive.status, "200 OK");
        assert_eq!(archive.content_type, "application/octet-stream");
        assert!(archive.body.starts_with(b"AXIOM_PACKAGE_ARCHIVE_V1\n"));
    }

    #[test]
    fn hosted_registry_rejects_traversal_and_unknown_files() {
        let dir = tempdir().expect("tempdir");
        let project = write_publishable_project(dir.path(), "core", "1.0.0");
        let registry = dir.path().join("registry");
        publish_package(
            &project,
            &registry,
            &PublishOptions {
                signing_key: Some(String::from("dev-key")),
                allow_overwrite: false,
            },
        )
        .expect("publish package");

        let context = RegistryServeContext::new(&registry, "http://registry.test", "dev-key")
            .expect("build serve context");
        let traversal = registry_http_response(
            &context,
            "GET /core/../package.axp HTTP/1.1\r\nHost: registry.test\r\n\r\n",
        )
        .expect("traversal response");
        assert_eq!(traversal.status, "400 Bad Request");
        assert!(String::from_utf8_lossy(&traversal.body).contains("unsafe version"));

        let metadata = registry_http_response(
            &context,
            "GET /core/1.0.0/axiom-registry.toml HTTP/1.1\r\nHost: registry.test\r\n\r\n",
        )
        .expect("metadata response");
        assert_eq!(metadata.status, "404 Not Found");
    }

    #[test]
    fn hosted_registry_serves_startup_snapshot_after_signature_drift() {
        let dir = tempdir().expect("tempdir");
        let project = write_publishable_project(dir.path(), "core", "1.0.0");
        let registry = dir.path().join("registry");
        publish_package(
            &project,
            &registry,
            &PublishOptions {
                signing_key: Some(String::from("dev-key")),
                allow_overwrite: false,
            },
        )
        .expect("publish package");
        let context = RegistryServeContext::new(&registry, "http://registry.test", "dev-key")
            .expect("build serve context");

        fs::write(
            registry.join("core").join("1.0.0").join("package.axp.sig"),
            "corrupted after startup\n",
        )
        .expect("corrupt signature");

        let index = registry_http_response(
            &context,
            "GET /index.json HTTP/1.1\r\nHost: registry.test\r\n\r\n",
        )
        .expect("index response");
        assert_eq!(index.status, "200 OK");
        assert!(String::from_utf8_lossy(&index.body).contains("\"core\""));

        let archive = registry_http_response(
            &context,
            "GET /core/1.0.0/package.axp HTTP/1.1\r\nHost: registry.test\r\n\r\n",
        )
        .expect("archive response");
        assert_eq!(archive.status, "200 OK");
        assert!(archive.body.starts_with(b"AXIOM_PACKAGE_ARCHIVE_V1\n"));
    }
}
