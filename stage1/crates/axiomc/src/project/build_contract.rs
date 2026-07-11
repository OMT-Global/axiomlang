//! Stable, agent-facing evidence for how a native build was produced.

use crate::codegen::NativeBackendKind;
use crate::cranelift_backend::CraneliftCompilationMode;
use crate::mir::Program;
use serde::{Deserialize, Serialize};

pub const BUILD_LOWERING_EVIDENCE_SCHEMA_VERSION: &str = "axiom.build-lowering-evidence.v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuildExecutionMode {
    DirectNativeRuntime,
    BoundedStaticOutput,
    GeneratedRustRuntime,
    NotProduced,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuildLoweringMode {
    DirectNativeRuntime,
    DirectNativeRuntimeWithStaticFolds,
    GeneratedRustCompatibility,
    RuntimeLoweringRequired,
    BoundedStaticOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildLoweringEvidence {
    pub schema_version: String,
    pub execution_mode: BuildExecutionMode,
    pub lowering_mode: BuildLoweringMode,
    pub direct_native_runtime: bool,
    pub known_value_static_folds: bool,
    pub legacy_fallback_attempted: bool,
}

impl BuildLoweringEvidence {
    pub fn generated_rust() -> Self {
        Self {
            schema_version: BUILD_LOWERING_EVIDENCE_SCHEMA_VERSION.to_string(),
            execution_mode: BuildExecutionMode::GeneratedRustRuntime,
            lowering_mode: BuildLoweringMode::GeneratedRustCompatibility,
            direct_native_runtime: false,
            known_value_static_folds: false,
            legacy_fallback_attempted: false,
        }
    }

    pub fn for_program(backend: NativeBackendKind, program: &Program) -> Self {
        match backend {
            NativeBackendKind::GeneratedRust => Self::generated_rust(),
            NativeBackendKind::Cranelift => {
                // Static declarations are compile-time-known inputs to the direct-native
                // lowerer. Their presence does not turn the binary into evaluator replay.
                let known_value_static_folds = !program.statics.is_empty();
                Self {
                    schema_version: BUILD_LOWERING_EVIDENCE_SCHEMA_VERSION.to_string(),
                    execution_mode: BuildExecutionMode::DirectNativeRuntime,
                    lowering_mode: if known_value_static_folds {
                        BuildLoweringMode::DirectNativeRuntimeWithStaticFolds
                    } else {
                        BuildLoweringMode::DirectNativeRuntime
                    },
                    direct_native_runtime: true,
                    known_value_static_folds,
                    legacy_fallback_attempted: false,
                }
            }
        }
    }

    /// Evidence emitted when the legacy evaluator fallback selection point was
    /// reached and rejected before evaluator execution.
    pub fn blocked_legacy_fallback() -> Self {
        Self {
            schema_version: BUILD_LOWERING_EVIDENCE_SCHEMA_VERSION.to_string(),
            execution_mode: BuildExecutionMode::NotProduced,
            lowering_mode: BuildLoweringMode::RuntimeLoweringRequired,
            direct_native_runtime: false,
            known_value_static_folds: false,
            legacy_fallback_attempted: true,
        }
    }

    pub fn from_cranelift_mode(mode: CraneliftCompilationMode) -> Self {
        let (execution_mode, lowering_mode, direct_native_runtime, known_value_static_folds) =
            match mode {
                CraneliftCompilationMode::DirectNativeRuntime => (
                    BuildExecutionMode::DirectNativeRuntime,
                    BuildLoweringMode::DirectNativeRuntime,
                    true,
                    false,
                ),
                CraneliftCompilationMode::DirectNativeRuntimeWithStaticFolds => (
                    BuildExecutionMode::DirectNativeRuntime,
                    BuildLoweringMode::DirectNativeRuntimeWithStaticFolds,
                    true,
                    true,
                ),
                CraneliftCompilationMode::BoundedStaticOutput => (
                    BuildExecutionMode::BoundedStaticOutput,
                    BuildLoweringMode::BoundedStaticOutput,
                    false,
                    true,
                ),
            };
        Self {
            schema_version: BUILD_LOWERING_EVIDENCE_SCHEMA_VERSION.to_string(),
            execution_mode,
            lowering_mode,
            direct_native_runtime,
            known_value_static_folds,
            legacy_fallback_attempted: false,
        }
    }
}
