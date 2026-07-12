use std::path::Path;

pub(super) fn run(project: &Path, spec: &Path, json: bool) -> i32 {
    match axiomc::agent_task::compile_task_contract(project, spec) {
        Ok(report) => {
            if json {
                super::print_json("task-contract", &report)
            } else {
                println!(
                    "task={} kind={:?} scope_nodes={} allowed_files={}",
                    report.contract.id,
                    report.contract.task_kind,
                    report.contract.scope.affected_semantic_nodes.len(),
                    report.contract.scope.allowed_files.len()
                );
                0
            }
        }
        Err(error) => super::print_error("task-contract", error, json),
    }
}
