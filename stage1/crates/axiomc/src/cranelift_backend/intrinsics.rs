//! Pure runtime-intrinsic implementations shared by the cranelift backend's
//! compile-time evaluator and static i64 lowering paths: JSON scalar parsing
//! and stringification, the stage1-safe regex engine, percent encoding, and
//! the crypto primitives. Extracted from cranelift_backend.rs under the
//! compiler-source decomposition ratchet (#1254); everything here is
//! independent of the evaluator and lowering state.

use super::*;

#[cfg(unix)]
#[repr(C)]
pub(crate) struct SpikeEvpCipher {
    _private: [u8; 0],
}
#[cfg(unix)]
#[repr(C)]
pub(crate) struct SpikeEvpCipherCtx {
    _private: [u8; 0],
}
#[cfg(unix)]
#[repr(C)]
pub(crate) struct SpikeEvpPkeyCtx {
    _private: [u8; 0],
}
#[cfg(unix)]
#[repr(C)]
pub(crate) struct SpikeEvpPkey {
    _private: [u8; 0],
}
#[cfg(unix)]
#[repr(C)]
pub(crate) struct SpikeEvpMdCtx {
    _private: [u8; 0],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RegexAtom {
    Literal(char),
    Any,
    Class {
        ranges: Vec<(char, char)>,
        negated: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RegexQuantifier {
    One,
    ZeroOrOne,
    ZeroOrMore,
    OneOrMore,
}

#[derive(Clone, Debug)]
pub(crate) struct RegexToken {
    pub(crate) atom: RegexAtom,
    pub(crate) quantifier: RegexQuantifier,
}

#[derive(Clone, Debug)]
pub(crate) struct RegexProgram {
    pub(crate) tokens: Vec<RegexToken>,
    pub(crate) start_anchor: bool,
    pub(crate) end_anchor: bool,
}

pub(crate) fn json_parse_int(text: &str) -> Option<i64> {
    text.trim().parse::<i64>().ok()
}

pub(crate) fn json_parse_bool(text: &str) -> Option<bool> {
    match text.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

pub(crate) fn json_parse_string(text: &str) -> Option<String> {
    let text = text.trim();
    if text.len() < 2 || !text.starts_with('"') || !text.ends_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut chars = text[1..text.len() - 1].chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next()? {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            '/' => out.push('/'),
            'b' => out.push('\u{0008}'),
            'f' => out.push('\u{000C}'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'u' => {
                let mut value = 0u32;
                for _ in 0..4 {
                    value = (value << 4) + chars.next()?.to_digit(16)?;
                }
                if (0xD800..=0xDBFF).contains(&value) {
                    // High surrogate: require a `\uDC00..=\uDFFF` low surrogate
                    // escape and combine, matching the generated-runtime JSON
                    // contract.
                    if chars.next()? != '\\' || chars.next()? != 'u' {
                        return None;
                    }
                    let mut low = 0u32;
                    for _ in 0..4 {
                        low = (low << 4) + chars.next()?.to_digit(16)?;
                    }
                    if !(0xDC00..=0xDFFF).contains(&low) {
                        return None;
                    }
                    let scalar = 0x10000 + ((value - 0xD800) << 10) + (low - 0xDC00);
                    out.push(char::from_u32(scalar)?);
                } else {
                    out.push(char::from_u32(value)?);
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

pub(crate) fn json_skip_ws(text: &str, mut index: usize) -> usize {
    let bytes = text.as_bytes();
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    index
}

pub(crate) fn json_scan_string_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.get(start).copied()? != b'"' {
        return None;
    }
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return Some(index + 1),
            _ => index += 1,
        }
    }
    None
}

pub(crate) fn json_scan_value_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if start >= bytes.len() {
        return None;
    }
    if bytes[start] == b'"' {
        return json_scan_string_end(text, start);
    }
    let mut index = start;
    let mut depth = 0i64;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => index = json_scan_string_end(text, index)?,
            b'{' | b'[' => {
                depth += 1;
                index += 1;
            }
            b'}' | b']' if depth > 0 => {
                depth -= 1;
                index += 1;
            }
            b',' | b'}' if depth == 0 => return Some(index),
            _ => index += 1,
        }
    }
    Some(index)
}

pub(crate) fn json_object_field(text: &str, key: &str) -> Option<String> {
    let text = text.trim();
    let bytes = text.as_bytes();
    if bytes.first().copied()? != b'{' || bytes.last().copied()? != b'}' {
        return None;
    }
    let mut index = 1usize;
    loop {
        index = json_skip_ws(text, index);
        if index >= bytes.len() || bytes[index] == b'}' {
            return None;
        }
        let key_end = json_scan_string_end(text, index)?;
        let found_key = json_parse_string(&text[index..key_end])?;
        index = json_skip_ws(text, key_end);
        if bytes.get(index).copied()? != b':' {
            return None;
        }
        let value_start = json_skip_ws(text, index + 1);
        let value_end = json_scan_value_end(text, value_start)?;
        if found_key == key {
            return Some(text[value_start..value_end].trim().to_string());
        }
        index = json_skip_ws(text, value_end);
        match bytes.get(index).copied()? {
            b',' => index += 1,
            b'}' => return None,
            _ => return None,
        }
    }
}

pub(crate) fn json_parse_value(text: &str) -> Option<String> {
    let text = text.trim();
    let end = json_scan_value_end(text, 0)?;
    if json_skip_ws(text, end) == text.len() {
        Some(text.to_string())
    } else {
        None
    }
}

pub(crate) fn json_escape_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

pub(crate) fn json_escape_string_content(value: &str) -> String {
    let escaped = json_escape_string(value);
    escaped
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(escaped.as_str())
        .to_string()
}

pub(crate) fn crypto_random_bytes(length: i64) -> Result<Vec<u8>, Diagnostic> {
    if !(0..=65536).contains(&length) {
        return Err(unsupported(
            "crypto_rand_bytes length must be between 0 and 65536",
        ));
    }
    let mut bytes = vec![0; length as usize];
    if bytes.is_empty() {
        return Ok(bytes);
    }
    fill_crypto_random_bytes(&mut bytes)?;
    Ok(bytes)
}

#[cfg(unix)]
pub(crate) fn crypto_ed25519_keygen() -> Result<(Vec<u8>, Vec<u8>), Diagnostic> {
    spike_crypto_ed25519_keygen_inner()
        .ok_or_else(|| unsupported("crypto_ed25519_keygen failed; check OpenSSL Ed25519 support"))
}

#[cfg(not(unix))]
pub(crate) fn crypto_ed25519_keygen() -> Result<(Vec<u8>, Vec<u8>), Diagnostic> {
    Err(unsupported(
        "crypto Ed25519 is not supported by the cranelift spike on Windows",
    ))
}

#[cfg(unix)]
pub(crate) fn crypto_ed25519_sign(secret_key: &[u8], message: &[u8]) -> Result<Vec<u8>, Diagnostic> {
    spike_crypto_ed25519_sign_inner(secret_key, message).ok_or_else(|| {
        unsupported("crypto_ed25519_sign failed; check key length and OpenSSL support")
    })
}

#[cfg(not(unix))]
pub(crate) fn crypto_ed25519_sign(_secret_key: &[u8], _message: &[u8]) -> Result<Vec<u8>, Diagnostic> {
    Err(unsupported(
        "crypto Ed25519 is not supported by the cranelift spike on Windows",
    ))
}

#[cfg(unix)]
pub(crate) fn crypto_ed25519_verify(
    public_key: &[u8],
    message: &[u8],
    signature: &[u8],
) -> Result<bool, Diagnostic> {
    spike_crypto_ed25519_verify_inner(public_key, message, signature)
        .ok_or_else(|| unsupported("crypto_ed25519_verify failed; check OpenSSL support"))
}

#[cfg(not(unix))]
pub(crate) fn crypto_ed25519_verify(
    _public_key: &[u8],
    _message: &[u8],
    _signature: &[u8],
) -> Result<bool, Diagnostic> {
    Err(unsupported(
        "crypto Ed25519 is not supported by the cranelift spike on Windows",
    ))
}

#[cfg(unix)]
pub(crate) fn crypto_aead_seal(
    alg: &str,
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, Diagnostic> {
    spike_crypto_aead_seal_inner(alg, key, nonce, aad, plaintext).ok_or_else(|| {
        unsupported(
            "crypto_aead_seal failed; check algorithm, key length, nonce length, and OpenSSL support",
        )
    })
}

#[cfg(not(unix))]
pub(crate) fn crypto_aead_seal(
    _alg: &str,
    _key: &[u8],
    _nonce: &[u8],
    _aad: &[u8],
    _plaintext: &[u8],
) -> Result<Vec<u8>, Diagnostic> {
    Err(unsupported(
        "crypto AEAD is not supported by the cranelift spike on this platform",
    ))
}

#[cfg(unix)]
pub(crate) fn crypto_aead_open(
    alg: &str,
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Option<Vec<u8>>, Diagnostic> {
    Ok(spike_crypto_aead_open_inner(
        alg, key, nonce, aad, ciphertext,
    ))
}

#[cfg(not(unix))]
pub(crate) fn crypto_aead_open(
    _alg: &str,
    _key: &[u8],
    _nonce: &[u8],
    _aad: &[u8],
    _ciphertext: &[u8],
) -> Result<Option<Vec<u8>>, Diagnostic> {
    Err(unsupported(
        "crypto AEAD is not supported by the cranelift spike on this platform",
    ))
}

#[cfg(unix)]
pub(crate) struct SpikeAeadCipher {
    pub(crate) cipher: *const SpikeEvpCipher,
    pub(crate) key_len: usize,
    pub(crate) nonce_len: usize,
    pub(crate) tag_len: usize,
}

#[cfg(unix)]
pub(crate) struct SpikeEd25519Crypto {
    pub(crate) handle: *mut std::os::raw::c_void,
    pub(crate) evp_pkey_ctx_new_id: unsafe extern "C" fn(
        std::os::raw::c_int,
        *mut std::os::raw::c_void,
    ) -> *mut SpikeEvpPkeyCtx,
    pub(crate) evp_pkey_ctx_free: unsafe extern "C" fn(*mut SpikeEvpPkeyCtx),
    pub(crate) evp_pkey_keygen_init: unsafe extern "C" fn(*mut SpikeEvpPkeyCtx) -> std::os::raw::c_int,
    pub(crate) evp_pkey_keygen:
        unsafe extern "C" fn(*mut SpikeEvpPkeyCtx, *mut *mut SpikeEvpPkey) -> std::os::raw::c_int,
    pub(crate) evp_pkey_free: unsafe extern "C" fn(*mut SpikeEvpPkey),
    pub(crate) evp_pkey_get_raw_public_key:
        unsafe extern "C" fn(*const SpikeEvpPkey, *mut u8, *mut usize) -> std::os::raw::c_int,
    pub(crate) evp_pkey_get_raw_private_key:
        unsafe extern "C" fn(*const SpikeEvpPkey, *mut u8, *mut usize) -> std::os::raw::c_int,
    pub(crate) evp_pkey_new_raw_private_key: unsafe extern "C" fn(
        std::os::raw::c_int,
        *mut std::os::raw::c_void,
        *const u8,
        usize,
    ) -> *mut SpikeEvpPkey,
    pub(crate) evp_pkey_new_raw_public_key: unsafe extern "C" fn(
        std::os::raw::c_int,
        *mut std::os::raw::c_void,
        *const u8,
        usize,
    ) -> *mut SpikeEvpPkey,
    pub(crate) evp_md_ctx_new: unsafe extern "C" fn() -> *mut SpikeEvpMdCtx,
    pub(crate) evp_md_ctx_free: unsafe extern "C" fn(*mut SpikeEvpMdCtx),
    pub(crate) evp_digest_sign_init: unsafe extern "C" fn(
        *mut SpikeEvpMdCtx,
        *mut *mut std::os::raw::c_void,
        *const std::os::raw::c_void,
        *mut std::os::raw::c_void,
        *mut SpikeEvpPkey,
    ) -> std::os::raw::c_int,
    pub(crate) evp_digest_sign: unsafe extern "C" fn(
        *mut SpikeEvpMdCtx,
        *mut u8,
        *mut usize,
        *const u8,
        usize,
    ) -> std::os::raw::c_int,
    pub(crate) evp_digest_verify_init: unsafe extern "C" fn(
        *mut SpikeEvpMdCtx,
        *mut *mut std::os::raw::c_void,
        *const std::os::raw::c_void,
        *mut std::os::raw::c_void,
        *mut SpikeEvpPkey,
    ) -> std::os::raw::c_int,
    pub(crate) evp_digest_verify: unsafe extern "C" fn(
        *mut SpikeEvpMdCtx,
        *const u8,
        usize,
        *const u8,
        usize,
    ) -> std::os::raw::c_int,
}

#[cfg(unix)]
pub(crate) struct SpikeAeadCrypto {
    pub(crate) handle: *mut std::os::raw::c_void,
    pub(crate) evp_aes_128_gcm: unsafe extern "C" fn() -> *const SpikeEvpCipher,
    pub(crate) evp_aes_256_gcm: unsafe extern "C" fn() -> *const SpikeEvpCipher,
    pub(crate) evp_chacha20_poly1305: unsafe extern "C" fn() -> *const SpikeEvpCipher,
    pub(crate) evp_cipher_ctx_new: unsafe extern "C" fn() -> *mut SpikeEvpCipherCtx,
    pub(crate) evp_cipher_ctx_free: unsafe extern "C" fn(*mut SpikeEvpCipherCtx),
    pub(crate) evp_cipher_ctx_ctrl: unsafe extern "C" fn(
        *mut SpikeEvpCipherCtx,
        std::os::raw::c_int,
        std::os::raw::c_int,
        *mut std::os::raw::c_void,
    ) -> std::os::raw::c_int,
    pub(crate) evp_encrypt_init_ex: unsafe extern "C" fn(
        *mut SpikeEvpCipherCtx,
        *const SpikeEvpCipher,
        *mut std::os::raw::c_void,
        *const u8,
        *const u8,
    ) -> std::os::raw::c_int,
    pub(crate) evp_encrypt_update: unsafe extern "C" fn(
        *mut SpikeEvpCipherCtx,
        *mut u8,
        *mut std::os::raw::c_int,
        *const u8,
        std::os::raw::c_int,
    ) -> std::os::raw::c_int,
    pub(crate) evp_encrypt_final_ex: unsafe extern "C" fn(
        *mut SpikeEvpCipherCtx,
        *mut u8,
        *mut std::os::raw::c_int,
    ) -> std::os::raw::c_int,
    pub(crate) evp_decrypt_init_ex: unsafe extern "C" fn(
        *mut SpikeEvpCipherCtx,
        *const SpikeEvpCipher,
        *mut std::os::raw::c_void,
        *const u8,
        *const u8,
    ) -> std::os::raw::c_int,
    pub(crate) evp_decrypt_update: unsafe extern "C" fn(
        *mut SpikeEvpCipherCtx,
        *mut u8,
        *mut std::os::raw::c_int,
        *const u8,
        std::os::raw::c_int,
    ) -> std::os::raw::c_int,
    pub(crate) evp_decrypt_final_ex: unsafe extern "C" fn(
        *mut SpikeEvpCipherCtx,
        *mut u8,
        *mut std::os::raw::c_int,
    ) -> std::os::raw::c_int,
}

#[cfg(unix)]
macro_rules! spike_crypto_aead_load_typed_symbol {
    ($handle:expr, $symbol:literal) => {{
        let value = spike_crypto_aead_load_symbol($handle, $symbol)?;
        unsafe { spike_crypto_aead_cast_typed_symbol(value) }
    }};
}

#[cfg(unix)]
impl SpikeEd25519Crypto {
    pub(crate) fn load() -> Result<Self, String> {
        let handle = spike_crypto_aead_open_library(SPIKE_OPENSSL_CRYPTO_CANDIDATES)?;
        Ok(Self {
            handle,
            evp_pkey_ctx_new_id: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_PKEY_CTX_new_id"
            ),
            evp_pkey_ctx_free: spike_crypto_aead_load_typed_symbol!(handle, "EVP_PKEY_CTX_free"),
            evp_pkey_keygen_init: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_PKEY_keygen_init"
            ),
            evp_pkey_keygen: spike_crypto_aead_load_typed_symbol!(handle, "EVP_PKEY_keygen"),
            evp_pkey_free: spike_crypto_aead_load_typed_symbol!(handle, "EVP_PKEY_free"),
            evp_pkey_get_raw_public_key: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_PKEY_get_raw_public_key"
            ),
            evp_pkey_get_raw_private_key: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_PKEY_get_raw_private_key"
            ),
            evp_pkey_new_raw_private_key: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_PKEY_new_raw_private_key"
            ),
            evp_pkey_new_raw_public_key: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_PKEY_new_raw_public_key"
            ),
            evp_md_ctx_new: spike_crypto_aead_load_typed_symbol!(handle, "EVP_MD_CTX_new"),
            evp_md_ctx_free: spike_crypto_aead_load_typed_symbol!(handle, "EVP_MD_CTX_free"),
            evp_digest_sign_init: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_DigestSignInit"
            ),
            evp_digest_sign: spike_crypto_aead_load_typed_symbol!(handle, "EVP_DigestSign"),
            evp_digest_verify_init: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_DigestVerifyInit"
            ),
            evp_digest_verify: spike_crypto_aead_load_typed_symbol!(handle, "EVP_DigestVerify"),
        })
    }
}

#[cfg(unix)]
impl SpikeAeadCrypto {
    pub(crate) fn load() -> Result<Self, String> {
        let handle = spike_crypto_aead_open_library(SPIKE_OPENSSL_CRYPTO_CANDIDATES)?;
        Ok(Self {
            handle,
            evp_aes_128_gcm: spike_crypto_aead_load_typed_symbol!(handle, "EVP_aes_128_gcm"),
            evp_aes_256_gcm: spike_crypto_aead_load_typed_symbol!(handle, "EVP_aes_256_gcm"),
            evp_chacha20_poly1305: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_chacha20_poly1305"
            ),
            evp_cipher_ctx_new: spike_crypto_aead_load_typed_symbol!(handle, "EVP_CIPHER_CTX_new"),
            evp_cipher_ctx_free: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_CIPHER_CTX_free"
            ),
            evp_cipher_ctx_ctrl: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_CIPHER_CTX_ctrl"
            ),
            evp_encrypt_init_ex: spike_crypto_aead_load_typed_symbol!(handle, "EVP_EncryptInit_ex"),
            evp_encrypt_update: spike_crypto_aead_load_typed_symbol!(handle, "EVP_EncryptUpdate"),
            evp_encrypt_final_ex: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_EncryptFinal_ex"
            ),
            evp_decrypt_init_ex: spike_crypto_aead_load_typed_symbol!(handle, "EVP_DecryptInit_ex"),
            evp_decrypt_update: spike_crypto_aead_load_typed_symbol!(handle, "EVP_DecryptUpdate"),
            evp_decrypt_final_ex: spike_crypto_aead_load_typed_symbol!(
                handle,
                "EVP_DecryptFinal_ex"
            ),
        })
    }
}

#[cfg(unix)]
pub(crate) struct SpikeAeadCtxGuard<'a> {
    pub(crate) ctx: *mut SpikeEvpCipherCtx,
    pub(crate) crypto: &'a SpikeAeadCrypto,
}

#[cfg(unix)]
impl<'a> SpikeAeadCtxGuard<'a> {
    pub(crate) fn new(ctx: *mut SpikeEvpCipherCtx, crypto: &'a SpikeAeadCrypto) -> Option<Self> {
        (!ctx.is_null()).then_some(Self { ctx, crypto })
    }
}

pub(crate) fn regex_escape_char(ch: char) -> char {
    match ch {
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        other => other,
    }
}

pub(crate) fn regex_parse_atom(chars: &[char], pos: &mut usize) -> Option<RegexAtom> {
    if *pos >= chars.len() {
        return None;
    }
    let ch = chars[*pos];
    *pos += 1;
    match ch {
        '.' => Some(RegexAtom::Any),
        '\\' => {
            if *pos >= chars.len() {
                Some(RegexAtom::Literal('\\'))
            } else {
                let escaped = regex_escape_char(chars[*pos]);
                *pos += 1;
                Some(RegexAtom::Literal(escaped))
            }
        }
        '[' => {
            let mut negated = false;
            if *pos < chars.len() && chars[*pos] == '^' {
                negated = true;
                *pos += 1;
            }
            let mut ranges = Vec::new();
            let mut first = true;
            while *pos < chars.len() {
                if chars[*pos] == ']' && !first {
                    *pos += 1;
                    return Some(RegexAtom::Class { ranges, negated });
                }
                first = false;
                let start = if chars[*pos] == '\\' {
                    *pos += 1;
                    if *pos >= chars.len() {
                        return None;
                    }
                    let escaped = regex_escape_char(chars[*pos]);
                    *pos += 1;
                    escaped
                } else {
                    let value = chars[*pos];
                    *pos += 1;
                    value
                };
                if *pos + 1 < chars.len() && chars[*pos] == '-' && chars[*pos + 1] != ']' {
                    *pos += 1;
                    let end = if chars[*pos] == '\\' {
                        *pos += 1;
                        if *pos >= chars.len() {
                            return None;
                        }
                        let escaped = regex_escape_char(chars[*pos]);
                        *pos += 1;
                        escaped
                    } else {
                        let value = chars[*pos];
                        *pos += 1;
                        value
                    };
                    if start <= end {
                        ranges.push((start, end));
                    } else {
                        ranges.push((end, start));
                    }
                } else {
                    ranges.push((start, start));
                }
            }
            None
        }
        '(' | ')' | '|' => None,
        other => Some(RegexAtom::Literal(other)),
    }
}

pub(crate) fn regex_parse(pattern: &str) -> Option<RegexProgram> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut pos = 0usize;
    let mut start_anchor = false;
    let mut end_anchor = false;
    if pos < chars.len() && chars[pos] == '^' {
        start_anchor = true;
        pos += 1;
    }
    let mut parse_end = chars.len();
    if parse_end > pos && chars[parse_end - 1] == '$' {
        let escaped = parse_end >= 2 && chars[parse_end - 2] == '\\';
        if !escaped {
            end_anchor = true;
            parse_end -= 1;
        }
    }
    let mut tokens = Vec::new();
    while pos < parse_end {
        let mut atom_pos = pos;
        let atom = regex_parse_atom(&chars[..parse_end], &mut atom_pos)?;
        pos = atom_pos;
        let quantifier = if pos < parse_end {
            match chars[pos] {
                '?' => {
                    pos += 1;
                    RegexQuantifier::ZeroOrOne
                }
                '*' => {
                    pos += 1;
                    RegexQuantifier::ZeroOrMore
                }
                '+' => {
                    pos += 1;
                    RegexQuantifier::OneOrMore
                }
                _ => RegexQuantifier::One,
            }
        } else {
            RegexQuantifier::One
        };
        tokens.push(RegexToken { atom, quantifier });
    }
    Some(RegexProgram {
        tokens,
        start_anchor,
        end_anchor,
    })
}

pub(crate) fn regex_atom_matches(atom: &RegexAtom, ch: char) -> bool {
    match atom {
        RegexAtom::Literal(expected) => *expected == ch,
        RegexAtom::Any => true,
        RegexAtom::Class { ranges, negated } => {
            let found = ranges.iter().any(|(start, end)| *start <= ch && ch <= *end);
            if *negated { !found } else { found }
        }
    }
}

pub(crate) fn regex_add_state(program: &RegexProgram, states: &mut Vec<usize>, state: usize) {
    if states.contains(&state) {
        return;
    }
    states.push(state);
    if state >= program.tokens.len() {
        return;
    }
    match program.tokens[state].quantifier {
        RegexQuantifier::ZeroOrOne | RegexQuantifier::ZeroOrMore => {
            regex_add_state(program, states, state + 1);
        }
        RegexQuantifier::One | RegexQuantifier::OneOrMore => {}
    }
}

pub(crate) fn regex_accepts(program: &RegexProgram, states: &[usize], at_text_end: bool) -> bool {
    states
        .iter()
        .any(|state| *state == program.tokens.len() && (!program.end_anchor || at_text_end))
}

pub(crate) fn regex_match_from(program: &RegexProgram, text: &[char], start: usize) -> Option<usize> {
    let mut states = Vec::new();
    regex_add_state(program, &mut states, 0);
    let mut last_accept = if regex_accepts(program, &states, start == text.len()) {
        Some(start)
    } else {
        None
    };
    let mut pos = start;
    while pos < text.len() {
        let ch = text[pos];
        let mut next = Vec::new();
        for state in states.iter().copied() {
            if state >= program.tokens.len() {
                continue;
            }
            let token = &program.tokens[state];
            if !regex_atom_matches(&token.atom, ch) {
                continue;
            }
            match token.quantifier {
                RegexQuantifier::One | RegexQuantifier::ZeroOrOne => {
                    regex_add_state(program, &mut next, state + 1);
                }
                RegexQuantifier::ZeroOrMore => {
                    regex_add_state(program, &mut next, state);
                    regex_add_state(program, &mut next, state + 1);
                }
                RegexQuantifier::OneOrMore => {
                    regex_add_state(program, &mut next, state);
                    regex_add_state(program, &mut next, state + 1);
                }
            }
        }
        pos += 1;
        if regex_accepts(program, &next, pos == text.len()) {
            last_accept = Some(pos);
        }
        states = next;
        if states.is_empty() {
            return last_accept;
        }
    }
    last_accept
}

pub(crate) fn regex_find_span(pattern: &str, text: &str) -> Option<(usize, usize)> {
    let program = regex_parse(pattern)?;
    let chars: Vec<char> = text.chars().collect();
    let byte_offsets: Vec<usize> = text
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(text.len()))
        .collect();
    let starts: Box<dyn Iterator<Item = usize>> = if program.start_anchor {
        Box::new(std::iter::once(0))
    } else {
        Box::new(0..=chars.len())
    };
    for start in starts {
        if let Some(end) = regex_match_from(&program, &chars, start) {
            return Some((byte_offsets[start], byte_offsets[end]));
        }
    }
    None
}

pub(crate) fn regex_replace_all(pattern: &str, text: &str, replacement: &str) -> String {
    let Some(program) = regex_parse(pattern) else {
        return text.to_string();
    };
    if program.start_anchor {
        let Some((start, end)) = regex_find_span(pattern, text) else {
            return text.to_string();
        };
        let mut out = String::new();
        out.push_str(&text[..start]);
        out.push_str(replacement);
        out.push_str(&text[end..]);
        return out;
    }
    let mut remaining = text;
    let mut out = String::new();
    loop {
        let Some((start, end)) = regex_find_span(pattern, remaining) else {
            out.push_str(remaining);
            break;
        };
        out.push_str(&remaining[..start]);
        out.push_str(replacement);
        if end == 0 {
            if let Some(ch) = remaining.chars().next() {
                out.push(ch);
                remaining = &remaining[ch.len_utf8()..];
            } else {
                break;
            }
        } else {
            remaining = &remaining[end..];
        }
    }
    out
}

pub(crate) fn encoding_is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
}

pub(crate) fn percent_encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::new();
    for byte in value.bytes() {
        if encoding_is_unreserved(byte) {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    out
}

pub(crate) fn percent_decode(value: &str) -> Option<String> {
    pub(crate) fn hex(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    let bytes = value.as_bytes();
    let mut index = 0usize;
    let mut out = Vec::new();
    while index < bytes.len() {
        if bytes[index] != b'%' {
            out.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return None;
        }
        let high = hex(bytes[index + 1])?;
        let low = hex(bytes[index + 2])?;
        out.push((high << 4) | low);
        index += 3;
    }
    String::from_utf8(out).ok()
}

