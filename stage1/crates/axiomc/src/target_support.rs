//! Host-native target support and fail-closed target selection.
//!
//! The direct-native backend currently links for the compiler's Rust host
//! target. Keeping target selection here makes that boundary explicit instead
//! of allowing a requested target to silently fall back to host behavior.

use crate::diagnostics::Diagnostic;
use serde::Serialize;

pub const TARGET_SUPPORT_SCHEMA_VERSION: &str = "axiom.target_support.v1";
pub const SUPPORTED_NATIVE_BACKEND: &str = "cranelift";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TargetSupportReport {
    pub schema_version: &'static str,
    pub backend: &'static str,
    pub host_target: Option<String>,
    pub host_supported: bool,
    pub target_selection: &'static str,
    pub supported_targets: Vec<SupportedTarget>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SupportedTarget {
    pub target: &'static str,
    pub platform: &'static str,
    pub object_format: &'static str,
    pub abi: &'static str,
    pub linker: &'static str,
    pub runtime: &'static str,
    pub status: &'static str,
}

pub fn supported_targets() -> Vec<SupportedTarget> {
    vec![
        SupportedTarget {
            target: "x86_64-unknown-linux-gnu",
            platform: "linux-x86-64",
            object_format: "elf",
            abi: "sysv-amd64",
            linker: "host-linker",
            runtime: "glibc-or-compatible-linux-runtime",
            status: "supported-host-only",
        },
        SupportedTarget {
            target: "aarch64-apple-darwin",
            platform: "macos-arm64",
            object_format: "mach-o",
            abi: "darwin-aarch64",
            linker: "host-linker",
            runtime: "darwin-system-runtime",
            status: "supported-host-only",
        },
    ]
}

pub fn host_target() -> Option<String> {
    compiled_host_target().map(str::to_owned)
}

#[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
fn compiled_host_target() -> Option<&'static str> {
    Some("x86_64-unknown-linux-gnu")
}

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
fn compiled_host_target() -> Option<&'static str> {
    Some("aarch64-apple-darwin")
}

#[cfg(not(any(
    all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"),
    all(target_arch = "aarch64", target_os = "macos")
)))]
fn compiled_host_target() -> Option<&'static str> {
    None
}

#[cfg(test)]
fn parse_rustc_host_target(version: &str) -> Option<String> {
    version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_string)
}

pub fn is_known_supported_target(target: &str) -> bool {
    supported_targets()
        .iter()
        .any(|supported| supported.target == target)
}

pub fn is_host_target(target: &str) -> bool {
    host_target().as_deref() == Some(target)
}

pub fn resolve_requested_target(target: Option<&str>) -> Result<Option<String>, Diagnostic> {
    let host = host_target().ok_or_else(|| {
        Diagnostic::new(
            "target",
            "the compiler host is not one of the supported direct-native targets",
        )
        .with_code("target.unsupported")
        .with_help("use a prebuilt compiler for a supported Linux x86_64 or macOS arm64 host")
    })?;
    let target = target.unwrap_or(&host);
    let target = match target {
        "wasm32" | "wasm32-wasi" => "wasm32-wasip1",
        target => target,
    };
    if !is_known_supported_target(target) || target != host {
        return Err(Diagnostic::new(
            "target",
            format!(
                "target {target:?} is unsupported by the direct-native backend; host target is {host:?}"
            ),
        )
        .with_code("target.unsupported")
        .with_help(
            "direct-native builds currently accept only the exact host target; cross-target support requires target-specific linker evidence",
        ));
    }
    Ok(Some(host))
}

pub fn resolve_build_target(
    target: Option<&str>,
    direct_native: bool,
) -> Result<Option<String>, Diagnostic> {
    if direct_native {
        return resolve_requested_target(target);
    }
    Ok(match target {
        Some("wasm32") | Some("wasm32-wasi") => Some(String::from("wasm32-wasip1")),
        Some(target) => Some(target.to_string()),
        None => None,
    })
}

pub fn report(host_target: Option<String>) -> TargetSupportReport {
    let host_supported = host_target
        .as_deref()
        .is_some_and(is_known_supported_target);
    TargetSupportReport {
        schema_version: TARGET_SUPPORT_SCHEMA_VERSION,
        backend: SUPPORTED_NATIVE_BACKEND,
        host_target,
        host_supported,
        target_selection: "exact-host-only",
        supported_targets: supported_targets(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        host_target, is_known_supported_target, parse_rustc_host_target, report,
        resolve_requested_target, supported_targets,
    };

    #[test]
    fn target_catalog_declares_initial_linux_and_macos_hosts() {
        let targets = supported_targets();
        assert_eq!(targets.len(), 2);
        assert!(
            targets
                .iter()
                .any(|target| target.target == "x86_64-unknown-linux-gnu")
        );
        assert!(
            targets
                .iter()
                .any(|target| target.target == "aarch64-apple-darwin")
        );
    }

    #[test]
    fn rustc_host_output_is_parsed_without_guessing_from_os() {
        assert_eq!(
            parse_rustc_host_target("rustc 1.90.0\nhost: aarch64-apple-darwin\nrelease: 1.90.0"),
            Some(String::from("aarch64-apple-darwin"))
        );
        assert_eq!(
            parse_rustc_host_target("rustc 1.90.0\nrelease: 1.90.0"),
            None
        );
    }

    #[test]
    fn compiled_host_identity_matches_the_supported_catalog() {
        let host = host_target().expect("test target must be a supported host");
        assert!(is_known_supported_target(&host));
        assert_eq!(report(Some(host.clone())).host_target, Some(host));
    }

    #[test]
    fn unknown_hosts_are_not_reported_as_supported() {
        assert!(!is_known_supported_target("x86_64-unknown-linux-musl"));
        assert!(!report(Some(String::from("x86_64-unknown-linux-musl"))).host_supported);
    }

    #[test]
    fn explicit_host_target_is_accepted_and_non_host_target_fails_closed() {
        let host = host_target().expect("test environment must expose a rustc host target");
        assert_eq!(resolve_requested_target(None), Ok(Some(host.clone())));
        assert_eq!(resolve_requested_target(Some(&host)), Ok(Some(host)));
        let error = resolve_requested_target(Some("wasm32"))
            .expect_err("non-host targets must not silently use host codegen");
        assert_eq!(error.code.as_deref(), Some("target.unsupported"));
        let malformed = resolve_requested_target(Some("not-a-target"))
            .expect_err("malformed targets must fail closed");
        assert_eq!(malformed.code.as_deref(), Some("target.unsupported"));
    }

    #[test]
    fn target_support_report_matches_its_published_schema() {
        let schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../schemas/axiom-target-support-v1.schema.json");
        let schema: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(schema_path).expect("read target support schema"),
        )
        .expect("target support schema is valid JSON");
        let validator = jsonschema::validator_for(&schema).expect("compile target support schema");
        let value = serde_json::to_value(report(host_target())).expect("serialize target report");
        assert!(
            validator.is_valid(&value),
            "target report must validate: {value}"
        );
    }
}
