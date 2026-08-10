use super::Stmt;
use super::expressions::static_bool_value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ControlFlow {
    pub(super) fallthrough: bool,
    pub(super) returns: bool,
    pub(super) breaks: bool,
    pub(super) continues: bool,
}

impl ControlFlow {
    pub(super) const fn fallthrough() -> Self {
        Self {
            fallthrough: true,
            returns: false,
            breaks: false,
            continues: false,
        }
    }

    pub(super) const fn return_value() -> Self {
        Self {
            fallthrough: false,
            returns: true,
            breaks: false,
            continues: false,
        }
    }

    pub(super) const fn break_value() -> Self {
        Self {
            fallthrough: false,
            returns: false,
            breaks: true,
            continues: false,
        }
    }

    pub(super) const fn continue_value() -> Self {
        Self {
            fallthrough: false,
            returns: false,
            breaks: false,
            continues: true,
        }
    }

    pub(super) const fn union(self, other: Self) -> Self {
        Self {
            fallthrough: self.fallthrough || other.fallthrough,
            returns: self.returns || other.returns,
            breaks: self.breaks || other.breaks,
            continues: self.continues || other.continues,
        }
    }

    pub(super) const fn always_returns(self) -> bool {
        self.returns && !self.fallthrough && !self.breaks && !self.continues
    }
}

impl Stmt {
    pub(super) fn control_flow(&self) -> ControlFlow {
        match self {
            Stmt::Return { .. } | Stmt::Panic { .. } => ControlFlow::return_value(),
            Stmt::Defer { .. } | Stmt::Assign { .. } => ControlFlow::fallthrough(),
            Stmt::Break { .. } => ControlFlow::break_value(),
            Stmt::Continue { .. } => ControlFlow::continue_value(),
            Stmt::If {
                cond,
                then_block,
                else_block: Some(else_block),
                ..
            } => match static_bool_value(cond) {
                Some(true) => block_control_flow(then_block),
                Some(false) => block_control_flow(else_block),
                None => block_control_flow(then_block).union(block_control_flow(else_block)),
            },
            Stmt::If {
                cond,
                then_block,
                else_block: None,
                ..
            } => match static_bool_value(cond) {
                Some(true) => block_control_flow(then_block),
                _ => ControlFlow::fallthrough(),
            },
            Stmt::While { .. } => ControlFlow::fallthrough(),
            Stmt::Match { arms, .. } => arms
                .iter()
                .map(|arm| block_control_flow(&arm.body))
                .fold(ControlFlow::default(), ControlFlow::union),
            _ => ControlFlow::fallthrough(),
        }
    }
}

fn block_control_flow(block: &[Stmt]) -> ControlFlow {
    let mut flow = ControlFlow::fallthrough();
    for stmt in block {
        if !flow.fallthrough {
            break;
        }
        flow = ControlFlow {
            fallthrough: false,
            returns: flow.returns,
            breaks: flow.breaks,
            continues: flow.continues,
        }
        .union(stmt.control_flow());
    }
    flow
}
