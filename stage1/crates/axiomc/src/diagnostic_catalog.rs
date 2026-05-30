use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct DiagnosticCodeInfo {
    pub code: &'static str,
    pub kind: &'static str,
    pub title: &'static str,
    pub explanation: &'static str,
    pub example: &'static str,
    pub suggested_fix: &'static str,
}

pub const STABLE_DIAGNOSTIC_CODES: &[DiagnosticCodeInfo] = &[
    DiagnosticCodeInfo {
        code: "use_after_move",
        kind: "ownership",
        title: "Use after move",
        explanation: "A non-copy value was moved into another binding or call and then used again.",
        example: "let alias: string = greeting\nprint greeting",
        suggested_fix: "Keep using the moved-to binding, or redesign the code so the original value is no longer needed after the move.",
    },
    DiagnosticCodeInfo {
        code: "move_while_borrowed",
        kind: "ownership",
        title: "Move while borrowed",
        explanation: "An owned collection root was moved while a live borrowed slice still referred to it.",
        example: "let slice: &[int] = values[0:1]\nlet moved: [int] = values",
        suggested_fix: "End the borrowed-slice scope before moving the owner, or pass the borrowed view instead of moving the owner.",
    },
    DiagnosticCodeInfo {
        code: "loop_move_outer_non_copy",
        kind: "ownership",
        title: "Loop move of outer non-copy value",
        explanation: "A loop body moved a non-copy value declared outside the loop, so later iterations could observe a missing value.",
        example: "let name: string = \"agent\"\nwhile true {\n  let moved: string = name\n}",
        suggested_fix: "Move the value before the loop, create the value inside the loop, or use a copy-compatible value.",
    },
    DiagnosticCodeInfo {
        code: "borrow_return_requires_param_origin",
        kind: "ownership",
        title: "Borrowed return without parameter origin",
        explanation: "A function returned a borrowed value that was not derived from one of its borrowed parameters.",
        example: "fn local(): &[int] {\n  let values: [int] = [1]\n  return values[0:1]\n}",
        suggested_fix: "Return owned data, or derive the borrowed return from a borrowed parameter.",
    },
    DiagnosticCodeInfo {
        code: "borrow_return_origin_ambiguous",
        kind: "type",
        title: "Ambiguous borrowed return origin",
        explanation: "A function returned a borrowed value while more than one borrowed parameter could be the origin.",
        example: "fn pick(a: &[int], b: &[int], use_a: bool): &[int] {\n  if use_a { return a[0:1] }\n  return b[0:1]\n}",
        suggested_fix: "Return an owned value, reduce the borrowed parameters to a single origin, or wait for explicit origin annotation syntax.",
    },
    DiagnosticCodeInfo {
        code: "mutable_borrow_while_shared_live",
        kind: "ownership",
        title: "Mutable borrow while shared borrow is live",
        explanation: "A mutable borrowed slice was created while a shared borrowed slice of the same owner was still live.",
        example: "let shared: &[int] = values[0:1]\nlet mutable: &mut [int] = values[1:2]",
        suggested_fix: "End the shared borrow before creating the mutable borrow.",
    },
    DiagnosticCodeInfo {
        code: "shared_borrow_while_mutable_live",
        kind: "ownership",
        title: "Shared borrow while mutable borrow is live",
        explanation: "A shared borrowed slice was created while a mutable borrowed slice of the same owner was still live.",
        example: "let mutable: &mut [int] = values[0:1]\nlet shared: &[int] = values[1:2]",
        suggested_fix: "End the mutable borrow before creating shared borrows.",
    },
    DiagnosticCodeInfo {
        code: "mutable_borrow_while_mutable_live",
        kind: "ownership",
        title: "Mutable borrow while mutable borrow is live",
        explanation: "A mutable borrowed slice was created while another mutable borrowed slice of the same owner was still live.",
        example: "let first: &mut [int] = values[0:1]\nlet second: &mut [int] = values[1:2]",
        suggested_fix: "End the first mutable borrow before creating another mutable borrow of the same owner.",
    },
    DiagnosticCodeInfo {
        code: "closure_move_captured_non_copy",
        kind: "ownership",
        title: "Closure moves captured non-copy value",
        explanation: "A closure body moved a captured non-copy value, so the captured binding would be unavailable after the closure runs.",
        example: "let name: string = \"agent\"\nlet f = fn() {\n  let moved: string = name\n}",
        suggested_fix: "Pass owned data into the closure deliberately, clone/copy only copy-compatible data, or restructure the closure so it borrows instead of moving the captured value.",
    },
    DiagnosticCodeInfo {
        code: "closure_borrowed_slice_return",
        kind: "ownership",
        title: "Closure returns borrowed slice without safe origin",
        explanation: "A closure returned a borrowed slice whose lifetime could not be tied to a safe borrowed parameter origin.",
        example: "let f = fn(): &[int] {\n  let values: [int] = [1]\n  return values[0:1]\n}",
        suggested_fix: "Return owned data from the closure, or derive the borrowed return from an explicit borrowed parameter.",
    },
    DiagnosticCodeInfo {
        code: "rebind_not_supported",
        kind: "binding",
        title: "Rebinding is not supported",
        explanation: "A binding name was declared more than once in the same unsupported rebinding context.",
        example: "let count: int = 1\nlet count: int = 2",
        suggested_fix: "Use a distinct binding name, or rewrite the code to avoid redeclaring the same local.",
    },
    DiagnosticCodeInfo {
        code: "import_cycle",
        kind: "imports",
        title: "Import cycle detected",
        explanation: "The package import graph contains a cycle, so the compiler cannot establish an acyclic module order.",
        example: "// a.ax imports b.ax\n// b.ax imports a.ax",
        suggested_fix: "Break the cycle by moving shared declarations into a third module or by depending in only one direction.",
    },
    DiagnosticCodeInfo {
        code: "import_cycle_member",
        kind: "imports",
        title: "Import participates in a cycle",
        explanation: "This import is one member of a larger import cycle reported by the project checker.",
        example: "import \"./b.ax\"",
        suggested_fix: "Inspect the full cycle diagnostic, then remove or invert one of the participating imports.",
    },
    DiagnosticCodeInfo {
        code: "generated_rust_compilation_failed",
        kind: "build",
        title: "Generated Rust compilation failed",
        explanation: "The generated Rust backend emitted code that rustc rejected while building the stage1 artifact.",
        example: "axiomc build . --backend generated-rust",
        suggested_fix: "Read the embedded rustc stderr, minimize the Axiom source that triggered it, and fix the backend lowering or source program as appropriate.",
    },
    DiagnosticCodeInfo {
        code: "ICE-001",
        kind: "internal",
        title: "Internal compiler error",
        explanation: "An invalid compiler-internal shape reached generated-Rust codegen and was reported as a structured diagnostic instead of unwinding.",
        example: "axiomc build . --backend generated-rust",
        suggested_fix: "File the source package and command that triggered the diagnostic so the compiler invariant can be fixed.",
    },
];

pub fn diagnostic_code_info(code: &str) -> Option<&'static DiagnosticCodeInfo> {
    STABLE_DIAGNOSTIC_CODES
        .iter()
        .find(|info| info.code == code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_contains_current_stable_ownership_codes() {
        let codes = STABLE_DIAGNOSTIC_CODES
            .iter()
            .map(|info| info.code)
            .collect::<Vec<_>>();

        assert!(codes.contains(&"use_after_move"));
        assert!(codes.contains(&"move_while_borrowed"));
        assert!(codes.contains(&"loop_move_outer_non_copy"));
        assert!(codes.contains(&"borrow_return_requires_param_origin"));
        assert!(codes.contains(&"borrow_return_origin_ambiguous"));
        assert!(codes.contains(&"mutable_borrow_while_shared_live"));
        assert!(codes.contains(&"shared_borrow_while_mutable_live"));
        assert!(codes.contains(&"mutable_borrow_while_mutable_live"));
        assert!(codes.contains(&"closure_move_captured_non_copy"));
        assert!(codes.contains(&"closure_borrowed_slice_return"));
        assert!(codes.contains(&"rebind_not_supported"));
        assert!(codes.contains(&"import_cycle"));
        assert!(codes.contains(&"import_cycle_member"));
        assert!(codes.contains(&"generated_rust_compilation_failed"));
        assert!(codes.contains(&"ICE-001"));
    }

    #[test]
    fn lookup_returns_explanation_and_fix() {
        let info = diagnostic_code_info("use_after_move").expect("code info");

        assert_eq!(info.kind, "ownership");
        assert!(info.explanation.contains("non-copy value"));
        assert!(info.suggested_fix.contains("moved-to binding"));
    }

    #[test]
    fn stage1_docs_list_stable_ownership_codes() {
        let docs = include_str!("../../../../docs/stage1.md");
        for info in STABLE_DIAGNOSTIC_CODES
            .iter()
            .filter(|info| info.kind == "ownership")
        {
            assert!(
                docs.contains(info.code),
                "docs/stage1.md should list ownership code {}",
                info.code
            );
        }
    }
}
