pub(crate) mod borrowck;
pub mod codegen;
pub(crate) mod cranelift_backend;
pub mod dap;
pub mod diagnostic_catalog;
pub mod diagnostics;
pub mod hir;
pub mod json_contract;
pub mod lockfile;
pub mod lsp;
pub mod manifest;
pub mod mir;
pub mod new_project;
pub mod project;
pub mod registry;
pub mod stdlib;
pub mod syntax;

#[cfg(test)]
#[path = "../tests/lib_unit.rs"]
mod tests;
