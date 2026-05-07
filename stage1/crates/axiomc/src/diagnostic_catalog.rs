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
        assert!(codes.contains(&"mutable_borrow_while_shared_live"));
        assert!(codes.contains(&"shared_borrow_while_mutable_live"));
        assert!(codes.contains(&"mutable_borrow_while_mutable_live"));
    }

    #[test]
    fn lookup_returns_explanation_and_fix() {
        let info = diagnostic_code_info("use_after_move").expect("code info");

        assert_eq!(info.kind, "ownership");
        assert!(info.explanation.contains("non-copy value"));
        assert!(info.suggested_fix.contains("moved-to binding"));
    }
}
