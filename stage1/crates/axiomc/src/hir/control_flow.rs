use super::Stmt;
use super::expressions::static_bool_value;

impl Stmt {
    pub(super) fn always_returns(&self) -> bool {
        match self {
            Stmt::Return { .. } | Stmt::Panic { .. } => true,
            Stmt::Defer { .. } | Stmt::Assign { .. } => false,
            Stmt::If {
                cond,
                then_block,
                else_block: Some(else_block),
                ..
            } => match static_bool_value(cond) {
                Some(true) => block_always_returns(then_block),
                Some(false) => block_always_returns(else_block),
                None => block_always_returns(then_block) && block_always_returns(else_block),
            },
            Stmt::If {
                cond,
                then_block,
                else_block: None,
                ..
            } => {
                static_bool_value(cond).is_some_and(|value| value)
                    && block_always_returns(then_block)
            }
            Stmt::Match { arms, .. } => arms.iter().all(|arm| block_always_returns(&arm.body)),
            _ => false,
        }
    }
}

fn block_always_returns(block: &[Stmt]) -> bool {
    block.last().is_some_and(Stmt::always_returns)
}
