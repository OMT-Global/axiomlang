//! Native compilation-mode classification and fail-closed selection diagnostics.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CraneliftCompilationMode {
    DirectNativeRuntime,
    DirectNativeRuntimeWithStaticFolds,
    BoundedStaticOutput,
}

impl CraneliftCompilationMode {
    pub fn uses_static_folding(self) -> bool {
        matches!(
            self,
            Self::DirectNativeRuntimeWithStaticFolds | Self::BoundedStaticOutput
        )
    }
}

pub(super) fn direct_native_mode(program: &Program) -> CraneliftCompilationMode {
    if program.statics.is_empty() && !program_uses_known_value_folds(program) {
        CraneliftCompilationMode::DirectNativeRuntime
    } else {
        CraneliftCompilationMode::DirectNativeRuntimeWithStaticFolds
    }
}

pub(super) fn known_value_fold_call(name: &str) -> bool {
    name.starts_with("json_")
        || name.starts_with("serdes_")
        || name.starts_with("std_serdes_")
        || name.starts_with("encoding_")
        || name.starts_with("string_")
        || name.starts_with("regex_")
        || matches!(
            name,
            "crypto_sha256"
                | "crypto_hmac_sha256"
                | "crypto_hmac_sha512"
                | "crypto_constant_time_eq"
                | "crypto_constant_time_eq_u8"
                | "crypto_verify_sha256"
                | "crypto_verify_sha512"
        )
}

pub(super) fn runtime_lowering_required() -> Diagnostic {
    Diagnostic::new(
        "build",
        "native build requires runtime lowering row direct-native.program_lowering; compile-time evaluator fallback is forbidden",
    )
    .with_code("backend.runtime_lowering_required")
    .with_help("fallback selection was blocked before evaluator execution")
}
