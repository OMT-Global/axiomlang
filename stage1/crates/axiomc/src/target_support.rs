//! Host-native target support and fail-closed target selection.
//!
//! The direct-native backend currently links for the compiler's Rust host
//! target. Keeping target selection here makes that boundary explicit instead
//! of allowing a requested target to silently fall back to host behavior.

use crate::diagnostics::Diagnostic;
use serde::Serialize;
use std::process::Command;

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
    pub libc: &'static str,
    pub runtime: &'static str,
    pub provider_policy: &'static str,
    pub profiles: Vec<&'static str>,
    pub unsupported_features: Vec<&'static str>,
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
            libc: "glibc-compatible",
            runtime: "glibc-or-compatible-linux-runtime",
            provider_policy: "capability-providers-qualified-separately",
            profiles: vec!["debug", "release"],
            unsupported_features: vec!["cross-compilation", "direct-native-wasm", "windows-native"],
            status: "supported-host-only",
        },
        SupportedTarget {
            target: "aarch64-apple-darwin",
            platform: "macos-arm64",
            object_format: "mach-o",
            abi: "darwin-aarch64",
            linker: "host-linker",
            libc: "darwin-libsystem",
            runtime: "darwin-system-runtime",
            provider_policy: "capability-providers-qualified-separately",
            profiles: vec!["debug", "release"],
            unsupported_features: vec!["cross-compilation", "direct-native-wasm", "windows-native"],
            status: "supported-host-only",
        },
    ]
}

pub fn host_target() -> Option<String> {
    let output = Command::new("rustc").args(["-vV"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_rustc_host_target(&String::from_utf8_lossy(&output.stdout))
}

pub fn parse_rustc_host_target(version: &str) -> Option<String> {
    version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_string)
}

pub fn is_known_supported_target(target: &str) -> bool {
    matches!(target, "x86_64-unknown-linux-gnu" | "aarch64-apple-darwin")
}

pub fn is_host_target(target: &str) -> bool {
    host_target().as_deref() == Some(target)
}

pub fn resolve_requested_target(target: Option<&str>) -> Result<Option<String>, Diagnostic> {
    let host = host_target();
    resolve_requested_target_for_host(target, host.as_deref())
}

fn resolve_requested_target_for_host(
    target: Option<&str>,
    host: Option<&str>,
) -> Result<Option<String>, Diagnostic> {
    let host = host.ok_or_else(|| {
        Diagnostic::new(
            "target",
            "cannot resolve the Rust host target for direct-native compilation",
        )
        .with_code("target.host_unavailable")
        .with_help("install a usable Rust toolchain for a supported direct-native host")
    })?;
    let Some(target) = target else {
        if !is_known_supported_target(host) {
            return Err(Diagnostic::new(
                "target",
                format!(
                    "host target {host:?} is not in the direct-native supported-target catalog"
                ),
            )
            .with_code("target.unsupported")
            .with_help(
                "direct-native builds currently support only x86_64-unknown-linux-gnu and aarch64-apple-darwin",
            ));
        }
        return Ok(None);
    };
    let target = match target {
        "wasm32" | "wasm32-wasi" => "wasm32-wasip1",
        target => target,
    };
    if !is_known_supported_target(target) {
        return Err(Diagnostic::new(
            "target",
            format!("target {target:?} is not in the direct-native supported-target catalog"),
        )
        .with_code("target.unsupported")
        .with_help(
            "direct-native builds currently support only x86_64-unknown-linux-gnu and aarch64-apple-darwin on the exact matching host",
        ));
    }
    if target != host {
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
    Ok(Some(host.to_string()))
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
        resolve_requested_target, resolve_requested_target_for_host, supported_targets,
    };

    #[test]
    fn target_catalog_declares_initial_linux_and_macos_hosts() {
        let targets = supported_targets();
        assert_eq!(targets.len(), 2);
        let linux = targets
            .iter()
            .find(|target| target.target == "x86_64-unknown-linux-gnu")
            .expect("Linux x86-64 target");
        assert_eq!(linux.platform, "linux-x86-64");
        assert_eq!(linux.object_format, "elf");
        assert_eq!(linux.abi, "sysv-amd64");
        assert_eq!(linux.libc, "glibc-compatible");
        let macos = targets
            .iter()
            .find(|target| target.target == "aarch64-apple-darwin")
            .expect("macOS arm64 target");
        assert_eq!(macos.platform, "macos-arm64");
        assert_eq!(macos.object_format, "mach-o");
        assert_eq!(macos.abi, "darwin-aarch64");
        assert_eq!(macos.libc, "darwin-libsystem");
        for target in targets {
            assert_eq!(
                target.provider_policy,
                "capability-providers-qualified-separately"
            );
            assert_eq!(target.profiles, ["debug", "release"]);
            assert_eq!(
                target.unsupported_features,
                ["cross-compilation", "direct-native-wasm", "windows-native"]
            );
        }
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
    fn unknown_hosts_are_not_reported_as_supported() {
        assert!(!is_known_supported_target("x86_64-unknown-linux-musl"));
        assert!(!report(Some(String::from("x86_64-unknown-linux-musl"))).host_supported);
    }

    #[test]
    fn explicit_host_target_is_accepted_and_non_host_target_fails_closed() {
        let host = host_target().expect("test environment must expose a rustc host target");
        assert_eq!(resolve_requested_target(Some(&host)), Ok(Some(host)));
        let error = resolve_requested_target(Some("wasm32"))
            .expect_err("non-host targets must not silently use host codegen");
        assert_eq!(error.code.as_deref(), Some("target.unsupported"));
    }

    #[test]
    fn unsupported_exact_host_is_rejected_before_host_equality_can_authorize_it() {
        let unsupported_host = "x86_64-apple-darwin";
        let error = resolve_requested_target_for_host(
            Some(unsupported_host),
            Some(unsupported_host),
        )
        .expect_err("an unknown exact host must not bypass the supported-target catalog");
        assert_eq!(error.code.as_deref(), Some("target.unsupported"));
    }

    #[test]
    fn implicit_target_requires_a_supported_resolvable_host() {
        assert_eq!(
            resolve_requested_target_for_host(None, Some("aarch64-apple-darwin")),
            Ok(None)
        );

        let unsupported = resolve_requested_target_for_host(
            None,
            Some("x86_64-unknown-linux-musl"),
        )
        .expect_err("an unsupported implicit host must fail closed");
        assert_eq!(unsupported.code.as_deref(), Some("target.unsupported"));

        let unavailable = resolve_requested_target_for_host(None, None)
            .expect_err("an unavailable implicit host must fail closed");
        assert_eq!(unavailable.code.as_deref(), Some("target.host_unavailable"));
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

        let mut earlier_v1 = value.clone();
        for row in earlier_v1["supported_targets"]
            .as_array_mut()
            .expect("supported targets are an array")
        {
            let row = row.as_object_mut().expect("target row is an object");
            for field in ["libc", "provider_policy", "profiles", "unsupported_features"] {
                row.remove(field);
            }
        }
        assert!(
            validator.is_valid(&earlier_v1),
            "additive v1 qualification fields must not invalidate earlier v1 producers"
        );
    }
}
