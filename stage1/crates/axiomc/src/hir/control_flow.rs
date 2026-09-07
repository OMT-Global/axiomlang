use super::Stmt;
use super::expressions::static_bool_value;

impl Stmt {
    pub(super) fn always_returns(&self) -> bool {
        self.always_exits(false)
    }

    pub(super) fn always_terminates(&self) -> bool {
        self.always_exits(true)
    }

    fn always_exits(&self, include_loop_control: bool) -> bool {
        let block_always_exits = |block: &[Stmt]| {
            block
                .last()
                .is_some_and(|stmt| stmt.always_exits(include_loop_control))
        };
        match self {
            Stmt::Return { .. } | Stmt::Panic { .. } => true,
            Stmt::Defer { .. } | Stmt::Assign { .. } => false,
            Stmt::Break { .. } | Stmt::Continue { .. } => include_loop_control,
            Stmt::If {
                cond,
                then_block,
                else_block: Some(else_block),
                ..
            } => match static_bool_value(cond) {
                Some(true) => block_always_exits(then_block),
                Some(false) => block_always_exits(else_block),
                None => block_always_exits(then_block) && block_always_exits(else_block),
            },
            Stmt::If {
                cond,
                then_block,
                else_block: None,
                ..
            } => {
                static_bool_value(cond).is_some_and(|value| value) && block_always_exits(then_block)
            }
            Stmt::Match { arms, .. } => arms.iter().all(|arm| block_always_exits(&arm.body)),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Stmt, lower};
    use crate::syntax;
    use std::path::Path;

    fn source_with_loop(body: &str) -> String {
        format!(
            "enum Control {{\nStop\nNext\n}}\nfn run(flag: bool): int {{\nwhile true {{\nlet choice: Control = Stop\n{body}\n}}\nreturn 0\n}}\n"
        )
    }

    #[test]
    fn nested_loop_control_rejects_unreachable_statements() {
        for terminating in [
            "break",
            "continue",
            "if true {\nbreak\n}",
            "if flag {\nbreak\n} else {\ncontinue\n}",
            "match choice {\nStop {\nbreak\n}\nNext {\ncontinue\n}\n}",
        ] {
            let source = source_with_loop(&format!("{terminating}\nprint 99"));
            let parsed = syntax::parse_program(&source, Path::new("main.ax")).expect("parse");
            let error = lower(&parsed).expect_err("statement after loop control is unreachable");
            assert_eq!(error.kind, "control", "{terminating}: {error:?}");
            assert!(
                error.message.contains("unreachable statements"),
                "{error:?}"
            );
        }
    }

    #[test]
    fn loop_control_terminates_scope_without_returning_from_function() {
        for terminating in [
            "break",
            "continue",
            "if flag {\nbreak\n} else {\ncontinue\n}",
            "match choice {\nStop {\nbreak\n}\nNext {\ncontinue\n}\n}",
        ] {
            let source = source_with_loop(terminating);
            let parsed = syntax::parse_program(&source, Path::new("main.ax")).expect("parse");
            let lowered = lower(&parsed).expect("loop followed by return should lower");
            let Stmt::While { body, .. } = &lowered.functions[0].body[0] else {
                panic!("expected loop");
            };
            let exit = body.last().expect("loop terminator");
            assert!(exit.always_terminates(), "{terminating}");
            assert!(!exit.always_returns(), "{terminating}");
        }
    }

    #[test]
    fn nested_break_preserves_existing_loop_exit_ownership_join() {
        let source = "fn run(flag: bool): int {\nlet label: string = \"x\"\nwhile true {\nlet sink: string = label\nprint sink\nif flag {\nbreak\n} else {\nbreak\n}\n}\nreturn 0\n}\n";
        let parsed = syntax::parse_program(source, Path::new("main.ax")).expect("parse");
        lower(&parsed).expect("both branches break before the moved value can be reused");
    }

    #[test]
    fn conditional_loop_control_preserves_reachable_fallthrough() {
        for conditional in ["if flag {\nbreak\n}", "if false {\ncontinue\n}"] {
            let source = source_with_loop(&format!("{conditional}\nprint 99"));
            let parsed = syntax::parse_program(&source, Path::new("main.ax")).expect("parse");
            lower(&parsed).expect("fallthrough remains reachable");
        }
    }
}
