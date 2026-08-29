pub mod agent_task;
pub(crate) mod borrowck;
pub mod bounded_executor;
#[path = "project/build_contract.rs"]
pub mod build_contract;
pub mod codegen;
pub(crate) mod cranelift_backend;
pub mod dap;
pub mod diagnostic_catalog;
pub mod diagnostics;
pub mod doctor;
pub mod hir;
pub mod intent_ir;
pub mod json_contract;
pub mod lockfile;
pub mod lsp;
pub mod manifest;
pub mod migration_plan;
pub mod mir;
pub mod new_project;
pub mod package_archive;
pub mod package_manager;
pub mod package_resolver;
pub mod package_store;
pub mod package_trust;
pub mod package_version;
pub mod project;
mod protocol_framing;
pub mod registry;
pub mod registry_client;
pub mod runtime_lifecycle;
pub mod stdlib;
pub mod syntax;
#[cfg(test)]
#[path = "../tests/support/lib_unit.rs"]
mod tests;
pub mod transactional_workspace;
pub mod verification_planner;
