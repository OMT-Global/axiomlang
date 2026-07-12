use std::path::Path;

pub(super) fn run(path: &Path, json: bool) -> i32 {
    match axiomc::intent_ir::emit_intent_ir(path) {
        Ok(document) => {
            if json {
                super::print_json("inspect intent", &document)
            } else {
                println!(
                    "graph={} nodes={} edges={} diagnostics={}",
                    document.graph_id,
                    document.nodes.len(),
                    document.edges.len(),
                    document.diagnostics.len()
                );
                0
            }
        }
        Err(error) => super::print_error("inspect intent", error, json),
    }
}
