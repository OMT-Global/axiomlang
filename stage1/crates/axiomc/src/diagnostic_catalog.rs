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
        suggested_fix: "Return owned data, split the function so each borrowed return has one possible source, or pass a single borrowed aggregate that represents the selected origin.",
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
    DiagnosticCodeInfo {
        code: "type.mismatch",
        kind: "type",
        title: "Type mismatch",
        explanation: "An expression, binding, return, or call argument has a different type than the surrounding context requires.",
        example: "Before: let count: int = \"one\"\nAfter: let count: int = 1",
        suggested_fix: "Change the expression type, adjust the annotation, or add an explicit cast when the source and destination types support one.",
    },
    DiagnosticCodeInfo {
        code: "parse.unexpected_token",
        kind: "parse",
        title: "Unexpected token",
        explanation: "The parser reached a token that is valid Axiom syntax in another position but not in the grammar slot currently being parsed.",
        example: "Before: let value: int = }\nAfter: let value: int = 1",
        suggested_fix: "Edit the source around the reported line and column so the token sequence matches the expected statement or expression form.",
    },
    DiagnosticCodeInfo {
        code: "parse.invalid_syntax",
        kind: "parse",
        title: "Invalid syntax",
        explanation: "The parser rejected source that does not fit any supported stage1 grammar form.",
        example: "Before: for value in values { print value }\nAfter: let i: int = 0\nwhile i < len(values) { print values[i]\n  i = i + 1\n}",
        suggested_fix: "Rewrite the construct using syntax supported by the stage1 grammar, then rerun `axiomc check --json`.",
    },
    DiagnosticCodeInfo {
        code: "parse.missing_token",
        kind: "parse",
        title: "Missing token",
        explanation: "A declaration or expression is incomplete because a required token such as a type separator or delimiter is missing.",
        example: "Before: const answer int = 42\nAfter: const answer: int = 42",
        suggested_fix: "Add the missing delimiter, annotation separator, or expression token at the reported source span.",
    },
    DiagnosticCodeInfo {
        code: "parse.unsupported_syntax",
        kind: "parse",
        title: "Unsupported syntax",
        explanation: "The source uses a syntactic form that the current stage1 compiler deliberately does not implement yet.",
        example: "Before: match value { Some(x) if x > 0 => x }\nAfter: bind the guard condition inside a supported branch body.",
        suggested_fix: "Replace the unsupported form with an equivalent supported construct or track the language feature issue before depending on it.",
    },
    DiagnosticCodeInfo {
        code: "manifest.invalid_capability",
        kind: "manifest",
        title: "Invalid capability declaration",
        explanation: "The package manifest declares a capability in a shape the stage1 manifest schema does not accept.",
        example: "Before: process = true\nAfter: process = [\"/usr/bin/true\"]",
        suggested_fix: "Use the narrowest supported capability form, such as a boolean false or a scoped allowlist for capabilities that require one.",
    },
    DiagnosticCodeInfo {
        code: "manifest.bad_dependency_path",
        kind: "manifest",
        title: "Bad dependency path",
        explanation: "A manifest dependency or workspace member path is empty, missing, or resolves outside the package/workspace boundary.",
        example: "Before: [dependencies]\ncore = { path = \"../core\" }\nAfter: [dependencies]\ncore = { path = \"deps/core\" }",
        suggested_fix: "Move the dependency under the workspace boundary or update the manifest path to a checked-in local package directory.",
    },
    DiagnosticCodeInfo {
        code: "import.unresolved",
        kind: "import",
        title: "Unresolved import",
        explanation: "An import path could not be resolved to a source file in the current package or dependency graph.",
        example: "Before: import \"./missing.ax\"\nAfter: import \"./main.ax\"",
        suggested_fix: "Fix the quoted import path, add the missing source file, or declare the dependency that owns the imported module.",
    },
    DiagnosticCodeInfo {
        code: "import.invalid",
        kind: "import",
        title: "Invalid import",
        explanation: "An import is syntactically present but violates the stage1 import contract, visibility rules, or namespace restrictions.",
        example: "Before: import \"../private.ax\"\nAfter: expose the needed API through a package-visible module inside the workspace boundary.",
        suggested_fix: "Keep imports within allowed package boundaries, avoid reserved namespaces, and export only names intended for downstream use.",
    },
    DiagnosticCodeInfo {
        code: "import.cycle",
        kind: "import",
        title: "Import cycle",
        explanation: "A package-local import graph loops back to a module that is already being loaded, so module order is ambiguous.",
        example: "Before: a.ax imports b.ax and b.ax imports a.ax\nAfter: move shared declarations to shared.ax",
        suggested_fix: "Break the cycle by moving shared declarations into a third module or by depending in only one direction.",
    },
    DiagnosticCodeInfo {
        code: "codegen.internal",
        kind: "codegen",
        title: "Internal code generation failure",
        explanation: "The compiler reached a backend path that could not lower a checked Axiom construct into the selected target representation.",
        example: "Before: axiomc build . emits a codegen diagnostic\nAfter: add or correct the backend lowering for that checked construct",
        suggested_fix: "Minimize the source that triggered the diagnostic and fix the backend lowering or route unsupported target behavior through an explicit unsupported-feature diagnostic.",
    },
    DiagnosticCodeInfo {
        code: "capability.denied",
        kind: "capability",
        title: "Capability denied",
        explanation: "Source attempted to use a host capability that is disabled or narrower than the requested operation in `axiom.toml`.",
        example: "Before: call `fs_write` with `[capabilities].fs = false`\nAfter: enable the narrow `fs:write` capability required for that package.",
        suggested_fix: "Declare the smallest required capability allowlist in the package manifest, or remove the host operation from the code path.",
    },
    DiagnosticCodeInfo {
        code: "type.invalid",
        kind: "type",
        title: "Invalid type use",
        explanation: "A type-system rule failed outside a simple expected-versus-actual mismatch, such as invalid recursion, arity, or unsupported type position.",
        example: "Before: struct Node { next: Node }\nAfter: introduce an explicit indirection type once the language supports it.",
        suggested_fix: "Adjust the type shape, arity, or construct to one supported by the current stage1 type checker.",
    },
    DiagnosticCodeInfo {
        code: "control.missing_return",
        kind: "control",
        title: "Missing return path",
        explanation: "A function with a non-unit return type can reach the end of a control-flow path without producing a value.",
        example: "Before: fn choose(flag: bool): int { if flag { return 1 } }\nAfter: add an else branch or trailing return value.",
        suggested_fix: "Make every reachable branch return a value of the declared return type, or change the signature if no value is required.",
    },
    DiagnosticCodeInfo {
        code: "control.unreachable_statement",
        kind: "control",
        title: "Unreachable statement",
        explanation: "A statement appears after a terminating control-flow operation in a context where stage1 does not yet model unreachable code.",
        example: "Before: panic(\"stop\")\nprint \"after\"\nAfter: remove the statement or move it before the terminating operation.",
        suggested_fix: "Remove the unreachable statement or restructure the branch so subsequent statements remain reachable.",
    },
    DiagnosticCodeInfo {
        code: "control.invalid",
        kind: "control",
        title: "Invalid control flow",
        explanation: "The checker found a control-flow shape that cannot be accepted by the current stage1 execution model.",
        example: "Before: a branch exits without satisfying the surrounding expression contract\nAfter: make the branch return or produce the expected value.",
        suggested_fix: "Restructure the branch, loop, or terminating statement so the function body satisfies the declared control-flow contract.",
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
        assert!(codes.contains(&"type.mismatch"));
        assert!(codes.contains(&"parse.unexpected_token"));
        assert!(codes.contains(&"parse.invalid_syntax"));
        assert!(codes.contains(&"parse.missing_token"));
        assert!(codes.contains(&"parse.unsupported_syntax"));
        assert!(codes.contains(&"manifest.invalid_capability"));
        assert!(codes.contains(&"manifest.bad_dependency_path"));
        assert!(codes.contains(&"import.unresolved"));
        assert!(codes.contains(&"import.invalid"));
        assert!(codes.contains(&"import.cycle"));
        assert!(codes.contains(&"codegen.internal"));
        assert!(codes.contains(&"capability.denied"));
        assert!(codes.contains(&"type.invalid"));
        assert!(codes.contains(&"control.missing_return"));
        assert!(codes.contains(&"control.unreachable_statement"));
        assert!(codes.contains(&"control.invalid"));
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

    #[test]
    fn conformance_fail_diagnostics_have_catalog_entries() {
        use crate::project::check_project;
        use std::fs;
        use std::path::Path;

        let fail_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("conformance")
            .join("fail");
        let mut checked = 0usize;
        let mut missing = Vec::new();

        for entry in fs::read_dir(&fail_dir).expect("read conformance fail corpus") {
            let entry = entry.expect("read conformance fail entry");
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            checked += 1;
            let error = check_project(&path)
                .expect_err("conformance fail corpus entries should fail to check")
                .normalized_for_json();
            match error.code.as_deref() {
                Some(code) if diagnostic_code_info(code).is_some() => {}
                Some(code) => missing.push(format!(
                    "{} emitted undocumented code {code}",
                    path.display()
                )),
                None => missing.push(format!(
                    "{} emitted no stable diagnostic code",
                    path.display()
                )),
            }
        }

        assert!(
            checked > 0,
            "conformance fail corpus should contain at least one fixture"
        );
        assert!(
            missing.is_empty(),
            "conformance fail diagnostics without catalog entries:\n{}",
            missing.join("\n")
        );
    }
}
