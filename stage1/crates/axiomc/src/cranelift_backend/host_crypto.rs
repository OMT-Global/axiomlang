//! Direct-native i64 runtime lowering — host_crypto group.
//! Extracted from cranelift_backend.rs under the compiler-source
//! decomposition ratchet (#1254). Shared IR types and helpers stay in
//! the parent module and are visible here through `use super::*`.

use super::*;

pub(crate) fn i64_audited_known_crypto_condition(
    expr: &Expr,
    result: bool,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Condition> {
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    let (intrinsic, inputs) = if is_i64_crypto_sha256_name(name, static_bindings) {
        let [input] = args.as_slice() else {
            return None;
        };
        let _ = i64_string_text(input, static_bindings)?;
        ("crypto_sha256", "strings:1")
    } else if is_i64_crypto_hmac_sha256_name(name, static_bindings)
        || is_i64_crypto_hmac_sha512_name(name, static_bindings)
    {
        let [key, message] = args.as_slice() else {
            return None;
        };
        let _ = i64_string_text(key, static_bindings)?;
        let _ = i64_string_text(message, static_bindings)?;
        let intrinsic = if is_i64_crypto_hmac_sha256_name(name, static_bindings) {
            "crypto_hmac_sha256"
        } else {
            "crypto_hmac_sha512"
        };
        (intrinsic, "strings:2")
    } else {
        return None;
    };
    let audited_result = i64_audited_crypto_expr(
        intrinsic,
        "inputs",
        inputs.to_string(),
        CraneliftI64Expr::Literal(i64::from(result)),
        static_bindings,
        CraneliftI64AuditSuccess::NonNegative,
    )?;
    Some(CraneliftI64Condition::Compare(CraneliftI64Compare {
        op: CraneliftI64CompareOp::Eq,
        lhs: audited_result,
        rhs: CraneliftI64Expr::Literal(1),
    }))
}

pub(crate) fn i64_audited_crypto_expr(
    intrinsic: &str,
    arg_name: &str,
    arg_value: String,
    result: CraneliftI64Expr,
    static_bindings: &I64StaticBindings,
    success: CraneliftI64AuditSuccess,
) -> Option<CraneliftI64Expr> {
    let package = static_bindings.package_root.as_deref()?;
    Some(CraneliftI64Expr::AuditCrypto {
        intrinsic: intrinsic.to_string(),
        package: package.display().to_string(),
        arg_name: arg_name.to_string(),
        arg_value,
        success,
        result: Box::new(result),
    })
}

pub(crate) fn lower_i64_crypto_random_intrinsic_expr(
    name: &str,
    args: &[Expr],
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    if !is_i64_crypto_random_u64_name(name, static_bindings) || !args.is_empty() {
        return None;
    }
    let package = static_bindings.package_root.as_deref()?;
    Some(CraneliftI64Expr::RandomU64 {
        intrinsic: "crypto_rand_u64".to_string(),
        package: package.display().to_string(),
    })
}

pub(crate) fn lower_i64_crypto_random_bytes_len_expr(
    expr: &Expr,
    local_indexes: &HashMap<String, usize>,
    local_conditions: &HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<CraneliftI64Expr> {
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if !is_i64_crypto_random_bytes_name(name, static_bindings) {
        return None;
    }
    let [length] = args.as_slice() else {
        return None;
    };
    let arg_value = i64_static_scalar_value(length, static_bindings)
        .map(|length| format!("int:{length}"))
        .unwrap_or_else(|| "int".to_string());
    let length = lower_i64_expr(
        length,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?;
    i64_audited_crypto_expr(
        "crypto_rand_bytes",
        "length",
        arg_value,
        CraneliftI64Expr::RandomBytesLen {
            length: Box::new(length),
        },
        static_bindings,
        CraneliftI64AuditSuccess::NonNegative,
    )
}

pub(crate) fn is_i64_std_crypto_wrapper(function: &Function, source_name: &str) -> bool {
    matches!(
        function.path.as_str(),
        "<stdlib>/crypto_hash.ax"
            | "<stdlib>/crypto_mac.ax"
            | "<stdlib>/crypto_rand.ax"
            | "<stdlib>/crypto.ax"
    ) && function.source_name == source_name
}

pub(crate) fn is_i64_crypto_sha256_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "crypto_sha256" || static_bindings.crypto_sha256_wrappers.contains(name)
}

pub(crate) fn is_i64_crypto_hmac_sha256_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "crypto_hmac_sha256" || static_bindings.crypto_hmac_sha256_wrappers.contains(name)
}

pub(crate) fn is_i64_crypto_hmac_sha512_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "crypto_hmac_sha512" || static_bindings.crypto_hmac_sha512_wrappers.contains(name)
}

pub(crate) fn is_i64_crypto_constant_time_eq_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "crypto_constant_time_eq"
        || static_bindings
            .crypto_constant_time_eq_wrappers
            .contains(name)
}

pub(crate) fn is_i64_crypto_constant_time_eq_u8_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "crypto_constant_time_eq_u8"
        || static_bindings
            .crypto_constant_time_eq_u8_wrappers
            .contains(name)
}

pub(crate) fn is_i64_crypto_verify_sha256_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.crypto_verify_sha256_wrappers.contains(name)
}

pub(crate) fn is_i64_crypto_verify_sha512_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    static_bindings.crypto_verify_sha512_wrappers.contains(name)
}

pub(crate) fn is_i64_crypto_random_bytes_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "crypto_rand_bytes" || static_bindings.crypto_random_bytes_wrappers.contains(name)
}

pub(crate) fn is_i64_crypto_random_u64_name(name: &str, static_bindings: &I64StaticBindings) -> bool {
    name == "crypto_rand_u64" || static_bindings.crypto_random_u64_wrappers.contains(name)
}

#[cfg(not(windows))]
pub(crate) fn fill_crypto_random_bytes(bytes: &mut [u8]) -> Result<(), Diagnostic> {
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(bytes))
        .map_err(|err| {
            unsupported(&format!(
                "failed to read random bytes from /dev/urandom: {err}"
            ))
        })
}

#[cfg(windows)]
pub(crate) fn fill_crypto_random_bytes(_bytes: &mut [u8]) -> Result<(), Diagnostic> {
    Err(unsupported(
        "crypto random bytes are not supported by the cranelift spike on Windows",
    ))
}

#[cfg(unix)]
pub(crate) fn spike_crypto_aead_seal_inner(
    alg: &str,
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    plaintext: &[u8],
) -> Option<Vec<u8>> {
    let crypto = SpikeAeadCrypto::load().ok()?;
    let cipher = spike_crypto_aead_cipher(&crypto, alg)?;
    if key.len() != cipher.key_len || nonce.len() != cipher.nonce_len {
        return None;
    }
    if plaintext.len() > std::os::raw::c_int::MAX as usize
        || aad.len() > std::os::raw::c_int::MAX as usize
    {
        return None;
    }
    let ctx = SpikeAeadCtxGuard::new(unsafe { (crypto.evp_cipher_ctx_new)() }, &crypto)?;
    if unsafe {
        (crypto.evp_encrypt_init_ex)(
            ctx.ctx,
            cipher.cipher,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    } <= 0
    {
        return None;
    }
    if unsafe {
        (crypto.evp_cipher_ctx_ctrl)(
            ctx.ctx,
            SPIKE_EVP_CTRL_AEAD_SET_IVLEN,
            cipher.nonce_len as std::os::raw::c_int,
            std::ptr::null_mut(),
        )
    } <= 0
    {
        return None;
    }
    if unsafe {
        (crypto.evp_encrypt_init_ex)(
            ctx.ctx,
            std::ptr::null(),
            std::ptr::null_mut(),
            key.as_ptr(),
            nonce.as_ptr(),
        )
    } <= 0
    {
        return None;
    }
    let mut chunk_len = 0 as std::os::raw::c_int;
    if !aad.is_empty()
        && unsafe {
            (crypto.evp_encrypt_update)(
                ctx.ctx,
                std::ptr::null_mut(),
                &mut chunk_len,
                aad.as_ptr(),
                aad.len() as std::os::raw::c_int,
            )
        } <= 0
    {
        return None;
    }
    let mut output = vec![0u8; plaintext.len() + cipher.tag_len];
    let mut written = 0usize;
    if !plaintext.is_empty() {
        if unsafe {
            (crypto.evp_encrypt_update)(
                ctx.ctx,
                output.as_mut_ptr(),
                &mut chunk_len,
                plaintext.as_ptr(),
                plaintext.len() as std::os::raw::c_int,
            )
        } <= 0
        {
            return None;
        }
        written += chunk_len as usize;
    }
    if unsafe {
        (crypto.evp_encrypt_final_ex)(ctx.ctx, output[written..].as_mut_ptr(), &mut chunk_len)
    } <= 0
    {
        return None;
    }
    written += chunk_len as usize;
    output.truncate(written);
    let mut tag = vec![0u8; cipher.tag_len];
    if unsafe {
        (crypto.evp_cipher_ctx_ctrl)(
            ctx.ctx,
            SPIKE_EVP_CTRL_AEAD_GET_TAG,
            cipher.tag_len as std::os::raw::c_int,
            tag.as_mut_ptr().cast::<std::os::raw::c_void>(),
        )
    } <= 0
    {
        return None;
    }
    output.extend_from_slice(&tag);
    Some(output)
}

#[cfg(unix)]
pub(crate) fn spike_crypto_aead_open_inner(
    alg: &str,
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
) -> Option<Vec<u8>> {
    let crypto = SpikeAeadCrypto::load().ok()?;
    let cipher = spike_crypto_aead_cipher(&crypto, alg)?;
    if key.len() != cipher.key_len
        || nonce.len() != cipher.nonce_len
        || ciphertext.len() < cipher.tag_len
    {
        return None;
    }
    let encrypted_len = ciphertext.len() - cipher.tag_len;
    if encrypted_len > std::os::raw::c_int::MAX as usize
        || aad.len() > std::os::raw::c_int::MAX as usize
    {
        return None;
    }
    let (encrypted, tag) = ciphertext.split_at(encrypted_len);
    let ctx = SpikeAeadCtxGuard::new(unsafe { (crypto.evp_cipher_ctx_new)() }, &crypto)?;
    if unsafe {
        (crypto.evp_decrypt_init_ex)(
            ctx.ctx,
            cipher.cipher,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    } <= 0
    {
        return None;
    }
    if unsafe {
        (crypto.evp_cipher_ctx_ctrl)(
            ctx.ctx,
            SPIKE_EVP_CTRL_AEAD_SET_IVLEN,
            cipher.nonce_len as std::os::raw::c_int,
            std::ptr::null_mut(),
        )
    } <= 0
    {
        return None;
    }
    if unsafe {
        (crypto.evp_decrypt_init_ex)(
            ctx.ctx,
            std::ptr::null(),
            std::ptr::null_mut(),
            key.as_ptr(),
            nonce.as_ptr(),
        )
    } <= 0
    {
        return None;
    }
    let mut chunk_len = 0 as std::os::raw::c_int;
    if !aad.is_empty()
        && unsafe {
            (crypto.evp_decrypt_update)(
                ctx.ctx,
                std::ptr::null_mut(),
                &mut chunk_len,
                aad.as_ptr(),
                aad.len() as std::os::raw::c_int,
            )
        } <= 0
    {
        return None;
    }
    let mut output = vec![0u8; encrypted_len + cipher.tag_len];
    let mut written = 0usize;
    if !encrypted.is_empty() {
        if unsafe {
            (crypto.evp_decrypt_update)(
                ctx.ctx,
                output.as_mut_ptr(),
                &mut chunk_len,
                encrypted.as_ptr(),
                encrypted.len() as std::os::raw::c_int,
            )
        } <= 0
        {
            return None;
        }
        written += chunk_len as usize;
    }
    if unsafe {
        (crypto.evp_cipher_ctx_ctrl)(
            ctx.ctx,
            SPIKE_EVP_CTRL_AEAD_SET_TAG,
            cipher.tag_len as std::os::raw::c_int,
            tag.as_ptr() as *mut std::os::raw::c_void,
        )
    } <= 0
    {
        return None;
    }
    if unsafe {
        (crypto.evp_decrypt_final_ex)(ctx.ctx, output[written..].as_mut_ptr(), &mut chunk_len)
    } <= 0
    {
        return None;
    }
    written += chunk_len as usize;
    output.truncate(written);
    Some(output)
}

#[cfg(unix)]
pub(crate) fn spike_crypto_aead_cipher(crypto: &SpikeAeadCrypto, alg: &str) -> Option<SpikeAeadCipher> {
    let (cipher, key_len) = match alg {
        "AES-128-GCM" => (unsafe { (crypto.evp_aes_128_gcm)() }, 16),
        "AES-256-GCM" => (unsafe { (crypto.evp_aes_256_gcm)() }, 32),
        "CHACHA20-POLY1305" => (unsafe { (crypto.evp_chacha20_poly1305)() }, 32),
        _ => return None,
    };
    if cipher.is_null() {
        return None;
    }
    Some(SpikeAeadCipher {
        cipher,
        key_len,
        nonce_len: 12,
        tag_len: 16,
    })
}

#[cfg(unix)]
pub(crate) fn spike_crypto_aead_open_library(
    candidates: &[&str],
) -> Result<*mut std::os::raw::c_void, String> {
    for candidate in candidates {
        let name = match std::ffi::CString::new(*candidate) {
            Ok(name) => name,
            Err(_) => continue,
        };
        let handle = unsafe { spike_crypto_aead_dlopen(name.as_ptr(), 2) };
        if !handle.is_null() {
            return Ok(handle);
        }
    }
    Err(format!(
        "AEAD support requires one of {}",
        candidates.join(", ")
    ))
}

#[cfg(unix)]
pub(crate) fn spike_crypto_aead_load_symbol(
    handle: *mut std::os::raw::c_void,
    symbol: &str,
) -> Result<*mut std::os::raw::c_void, String> {
    let name = std::ffi::CString::new(symbol).map_err(|_| String::from("invalid symbol name"))?;
    let value = unsafe { spike_crypto_aead_dlsym(handle, name.as_ptr()) };
    if value.is_null() {
        return Err(format!("AEAD support missing OpenSSL symbol {symbol}"));
    }
    Ok(value)
}

#[cfg(unix)]
#[cfg_attr(not(target_os = "macos"), link(name = "dl"))]
unsafe extern "C" {
    #[link_name = "dlopen"]
    fn spike_crypto_aead_dlopen(
        filename: *const std::os::raw::c_char,
        flags: std::os::raw::c_int,
    ) -> *mut std::os::raw::c_void;
    #[link_name = "dlsym"]
    fn spike_crypto_aead_dlsym(
        handle: *mut std::os::raw::c_void,
        symbol: *const std::os::raw::c_char,
    ) -> *mut std::os::raw::c_void;
    #[link_name = "dlclose"]
    fn spike_crypto_aead_dlclose(handle: *mut std::os::raw::c_void) -> std::os::raw::c_int;
}

#[cfg(unix)]
pub(crate) fn spike_crypto_ed25519_keygen_inner() -> Option<(Vec<u8>, Vec<u8>)> {
    let crypto = SpikeEd25519Crypto::load().ok()?;
    let ctx = SpikeEd25519PkeyCtxGuard::new(
        unsafe { (crypto.evp_pkey_ctx_new_id)(SPIKE_EVP_PKEY_ED25519, std::ptr::null_mut()) },
        &crypto,
    )?;
    if unsafe { (crypto.evp_pkey_keygen_init)(ctx.ctx) } <= 0 {
        return None;
    }
    let mut pkey = std::ptr::null_mut();
    if unsafe { (crypto.evp_pkey_keygen)(ctx.ctx, &mut pkey) } <= 0 || pkey.is_null() {
        return None;
    }
    let pkey = SpikeEd25519PkeyGuard::new(pkey, &crypto)?;
    let mut public_key = vec![0u8; 32];
    let mut public_len = public_key.len();
    if unsafe {
        (crypto.evp_pkey_get_raw_public_key)(pkey.pkey, public_key.as_mut_ptr(), &mut public_len)
    } <= 0
    {
        return None;
    }
    let mut private_key = vec![0u8; 32];
    let mut private_len = private_key.len();
    if unsafe {
        (crypto.evp_pkey_get_raw_private_key)(pkey.pkey, private_key.as_mut_ptr(), &mut private_len)
    } <= 0
    {
        return None;
    }
    if public_len != 32 || private_len != 32 {
        return None;
    }
    public_key.truncate(public_len);
    private_key.truncate(private_len);
    private_key.extend_from_slice(&public_key);
    Some((public_key, private_key))
}

#[cfg(unix)]
pub(crate) fn spike_crypto_ed25519_sign_inner(secret_key: &[u8], message: &[u8]) -> Option<Vec<u8>> {
    let crypto = SpikeEd25519Crypto::load().ok()?;
    let signing_key = spike_crypto_ed25519_signing_key(secret_key)?;
    let pkey = unsafe {
        (crypto.evp_pkey_new_raw_private_key)(
            SPIKE_EVP_PKEY_ED25519,
            std::ptr::null_mut(),
            signing_key.as_ptr(),
            signing_key.len(),
        )
    };
    let pkey = SpikeEd25519PkeyGuard::new(pkey, &crypto)?;
    let ctx = SpikeEd25519MdCtxGuard::new(unsafe { (crypto.evp_md_ctx_new)() }, &crypto)?;
    if unsafe {
        (crypto.evp_digest_sign_init)(
            ctx.ctx,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null_mut(),
            pkey.pkey,
        )
    } <= 0
    {
        return None;
    }
    let mut signature_len = 0usize;
    if unsafe {
        (crypto.evp_digest_sign)(
            ctx.ctx,
            std::ptr::null_mut(),
            &mut signature_len,
            message.as_ptr(),
            message.len(),
        )
    } <= 0
        || signature_len == 0
        || signature_len > 1024
    {
        return None;
    }
    let mut signature = vec![0u8; signature_len];
    if unsafe {
        (crypto.evp_digest_sign)(
            ctx.ctx,
            signature.as_mut_ptr(),
            &mut signature_len,
            message.as_ptr(),
            message.len(),
        )
    } <= 0
    {
        return None;
    }
    signature.truncate(signature_len);
    Some(signature)
}

#[cfg(unix)]
pub(crate) fn spike_crypto_ed25519_verify_inner(
    public_key: &[u8],
    message: &[u8],
    signature: &[u8],
) -> Option<bool> {
    if public_key.len() != 32 || signature.len() != 64 {
        return Some(false);
    }
    let crypto = SpikeEd25519Crypto::load().ok()?;
    let pkey = unsafe {
        (crypto.evp_pkey_new_raw_public_key)(
            SPIKE_EVP_PKEY_ED25519,
            std::ptr::null_mut(),
            public_key.as_ptr(),
            public_key.len(),
        )
    };
    let pkey = SpikeEd25519PkeyGuard::new(pkey, &crypto)?;
    let ctx = SpikeEd25519MdCtxGuard::new(unsafe { (crypto.evp_md_ctx_new)() }, &crypto)?;
    if unsafe {
        (crypto.evp_digest_verify_init)(
            ctx.ctx,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null_mut(),
            pkey.pkey,
        )
    } <= 0
    {
        return None;
    }
    let result = unsafe {
        (crypto.evp_digest_verify)(
            ctx.ctx,
            signature.as_ptr(),
            signature.len(),
            message.as_ptr(),
            message.len(),
        )
    };
    if result == 1 {
        Some(true)
    } else if result == 0 {
        Some(false)
    } else {
        None
    }
}

#[cfg(unix)]
pub(crate) fn spike_crypto_ed25519_signing_key(secret_key: &[u8]) -> Option<&[u8]> {
    match secret_key.len() {
        32 => Some(secret_key),
        64 => Some(&secret_key[..32]),
        _ => None,
    }
}

#[cfg(unix)]
const SPIKE_EVP_CTRL_AEAD_SET_IVLEN: std::os::raw::c_int = 0x9;
#[cfg(unix)]
const SPIKE_EVP_CTRL_AEAD_GET_TAG: std::os::raw::c_int = 0x10;
#[cfg(unix)]
const SPIKE_EVP_CTRL_AEAD_SET_TAG: std::os::raw::c_int = 0x11;

#[cfg(unix)]
const SPIKE_EVP_PKEY_ED25519: std::os::raw::c_int = 1087;

#[cfg(unix)]
impl Drop for SpikeEd25519Crypto {
    fn drop(&mut self) {
        unsafe {
            let _ = spike_crypto_aead_dlclose(self.handle);
        }
    }
}

#[cfg(unix)]
impl Drop for SpikeAeadCrypto {
    fn drop(&mut self) {
        unsafe {
            let _ = spike_crypto_aead_dlclose(self.handle);
        }
    }
}

#[cfg(unix)]
pub(crate) struct SpikeEd25519PkeyCtxGuard<'a> {
    ctx: *mut SpikeEvpPkeyCtx,
    crypto: &'a SpikeEd25519Crypto,
}

#[cfg(unix)]
impl<'a> SpikeEd25519PkeyCtxGuard<'a> {
    fn new(ctx: *mut SpikeEvpPkeyCtx, crypto: &'a SpikeEd25519Crypto) -> Option<Self> {
        (!ctx.is_null()).then_some(Self { ctx, crypto })
    }
}

#[cfg(unix)]
impl Drop for SpikeEd25519PkeyCtxGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.crypto.evp_pkey_ctx_free)(self.ctx);
        }
    }
}

#[cfg(unix)]
pub(crate) struct SpikeEd25519PkeyGuard<'a> {
    pkey: *mut SpikeEvpPkey,
    crypto: &'a SpikeEd25519Crypto,
}

#[cfg(unix)]
impl<'a> SpikeEd25519PkeyGuard<'a> {
    fn new(pkey: *mut SpikeEvpPkey, crypto: &'a SpikeEd25519Crypto) -> Option<Self> {
        (!pkey.is_null()).then_some(Self { pkey, crypto })
    }
}

#[cfg(unix)]
impl Drop for SpikeEd25519PkeyGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.crypto.evp_pkey_free)(self.pkey);
        }
    }
}

#[cfg(unix)]
pub(crate) struct SpikeEd25519MdCtxGuard<'a> {
    ctx: *mut SpikeEvpMdCtx,
    crypto: &'a SpikeEd25519Crypto,
}

#[cfg(unix)]
impl<'a> SpikeEd25519MdCtxGuard<'a> {
    fn new(ctx: *mut SpikeEvpMdCtx, crypto: &'a SpikeEd25519Crypto) -> Option<Self> {
        (!ctx.is_null()).then_some(Self { ctx, crypto })
    }
}

#[cfg(unix)]
impl Drop for SpikeEd25519MdCtxGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.crypto.evp_md_ctx_free)(self.ctx);
        }
    }
}

#[cfg(unix)]
impl Drop for SpikeAeadCtxGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.crypto.evp_cipher_ctx_free)(self.ctx);
        }
    }
}

#[cfg(unix)]
pub(crate) const SPIKE_OPENSSL_CRYPTO_CANDIDATES: &[&str] = &[
    "/usr/lib/x86_64-linux-gnu/libcrypto.so.3",
    "/lib/x86_64-linux-gnu/libcrypto.so.3",
    "/usr/lib/aarch64-linux-gnu/libcrypto.so.3",
    "/lib/aarch64-linux-gnu/libcrypto.so.3",
    "/usr/lib64/libcrypto.so.3",
    "/lib64/libcrypto.so.3",
    "/usr/lib/libcrypto.so.3",
    "/lib/libcrypto.so.3",
    "/opt/homebrew/opt/openssl@3/lib/libcrypto.3.dylib",
    "/usr/local/opt/openssl@3/lib/libcrypto.3.dylib",
    "/usr/lib/x86_64-linux-gnu/libcrypto.so.1.1",
    "/lib/x86_64-linux-gnu/libcrypto.so.1.1",
    "/usr/lib/aarch64-linux-gnu/libcrypto.so.1.1",
    "/lib/aarch64-linux-gnu/libcrypto.so.1.1",
    "/usr/lib64/libcrypto.so.1.1",
    "/lib64/libcrypto.so.1.1",
    "/usr/lib/libcrypto.so.1.1",
    "/lib/libcrypto.so.1.1",
];

#[cfg(unix)]
pub(crate) unsafe fn spike_crypto_aead_cast_typed_symbol<T: Copy>(value: *mut std::os::raw::c_void) -> T {
    debug_assert_eq!(
        std::mem::size_of::<T>(),
        std::mem::size_of::<*mut std::os::raw::c_void>()
    );
    let mut output = std::mem::MaybeUninit::<T>::uninit();
    unsafe {
        std::ptr::copy_nonoverlapping(
            (&value as *const *mut std::os::raw::c_void).cast::<u8>(),
            output.as_mut_ptr().cast::<u8>(),
            std::mem::size_of::<T>(),
        );
        output.assume_init()
    }
}
