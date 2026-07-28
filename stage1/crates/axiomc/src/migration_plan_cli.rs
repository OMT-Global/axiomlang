use axiomc::diagnostics::Diagnostic;
use axiomc::migration_plan::{MigrationPlan, migration_plan_from_slice};
use std::fs;
use std::path::Path;

pub(super) fn run(report: &Path, json: bool) -> i32 {
    match execute(report) {
        Ok(plan) => {
            if json {
                super::print_json("migrate", &plan)
            } else {
                println!(
                    "migration plan {} -> {} ({} actions; no changes applied)",
                    plan.editions.from,
                    plan.editions.to,
                    plan.actions.len()
                );
                for action in &plan.actions {
                    println!("{}. {}: {}", action.sequence, action.id, action.instruction);
                }
                0
            }
        }
        Err(error) => super::print_error("migrate", error, json),
    }
}

fn execute(report: &Path) -> Result<MigrationPlan, Diagnostic> {
    let bytes = fs::read(report).map_err(|error| {
        Diagnostic::new(
            "migration_plan",
            format!(
                "failed to read compatibility report {}: {error}",
                report.display()
            ),
        )
        .with_path(report.display().to_string())
    })?;
    migration_plan_from_slice(&bytes).map_err(|error| {
        Diagnostic::new("migration_plan", error).with_path(report.display().to_string())
    })
}
