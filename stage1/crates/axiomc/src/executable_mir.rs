//! A small, backend-neutral executable MIR for the first runtime vertical slice.
//!
//! This is intentionally narrower than the full Semantic MIR contract.  It
//! captures scalar values, runtime stdin length, direct calls, arithmetic,
//! comparisons, conditions, and terminal control-flow blocks.  Unsupported
//! source shapes return `None` so the existing broader lowering can continue
//! serving compatibility fixtures while this boundary grows deliberately.

use crate::mir;
use std::collections::{HashMap, HashSet};

pub type BlockId = usize;
pub type ValueId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ScalarType {
    Int,
    Bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Program {
    pub schema_version: &'static str,
    pub entrypoint: String,
    pub functions: Vec<Function>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: ScalarType,
    pub entry_block: BlockId,
    pub blocks: Vec<BasicBlock>,
    pub span: mir::SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Param {
    pub name: String,
    pub value: ValueId,
    pub ty: ScalarType,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BasicBlock {
    pub id: BlockId,
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
    pub span: mir::SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum Instruction {
    Parameter {
        result: ValueId,
        index: usize,
        ty: ScalarType,
        span: mir::SourceSpan,
    },
    Const {
        result: ValueId,
        value: i64,
        ty: ScalarType,
        span: mir::SourceSpan,
    },
    ReadStdin {
        result: ValueId,
        max_bytes: u32,
        effects: Vec<String>,
        span: mir::SourceSpan,
    },
    Binary {
        result: ValueId,
        op: mir::ArithmeticOp,
        lhs: ValueId,
        rhs: ValueId,
        span: mir::SourceSpan,
    },
    Compare {
        result: ValueId,
        op: mir::CompareOp,
        lhs: ValueId,
        rhs: ValueId,
        span: mir::SourceSpan,
    },
    Logic {
        result: ValueId,
        op: mir::LogicOp,
        lhs: ValueId,
        rhs: ValueId,
        span: mir::SourceSpan,
    },
    Call {
        result: ValueId,
        function: String,
        args: Vec<ValueId>,
        effects: Vec<String>,
        span: mir::SourceSpan,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum Terminator {
    Return {
        value: ValueId,
        span: mir::SourceSpan,
    },
    Branch {
        condition: ValueId,
        then_block: BlockId,
        else_block: BlockId,
        span: mir::SourceSpan,
    },
    Unreachable,
}

#[derive(Debug, Clone, Copy)]
struct ValueInfo {
    id: ValueId,
    ty: ScalarType,
}

struct FunctionLowerer<'a> {
    function: &'a mir::Function,
    known_functions: &'a HashSet<String>,
    next_value: ValueId,
    blocks: Vec<BasicBlock>,
    variables: HashMap<String, ValueInfo>,
}

impl<'a> FunctionLowerer<'a> {
    fn new(function: &'a mir::Function, known_functions: &'a HashSet<String>) -> Option<Self> {
        scalar_type(&function.return_ty)?;
        if function.is_property || function.is_async || function.is_extern {
            return None;
        }
        let mut lowerer = Self {
            function,
            known_functions,
            next_value: 0,
            blocks: vec![BasicBlock {
                id: 0,
                instructions: Vec::new(),
                terminator: Terminator::Unreachable,
                span: mir::SourceSpan {
                    line: function.line,
                    column: function.column,
                },
            }],
            variables: HashMap::new(),
        };
        for (index, param) in function.params.iter().enumerate() {
            let ty = scalar_type(&param.ty)?;
            let value = lowerer.fresh_value();
            lowerer
                .variables
                .insert(param.name.clone(), ValueInfo { id: value, ty });
            let span = lowerer.blocks[0].span;
            lowerer.blocks[0].instructions.push(Instruction::Parameter {
                result: value,
                index,
                ty,
                span,
            });
        }
        Some(lowerer)
    }

    fn fresh_value(&mut self) -> ValueId {
        let value = self.next_value;
        self.next_value += 1;
        value
    }

    fn lower(mut self) -> Option<Function> {
        let entry_variables = self.variables.clone();
        let body = self.function.body.clone();
        self.lower_statements(0, &body, entry_variables)?;
        let params = self
            .function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                Some(Param {
                    name: param.name.clone(),
                    value: self.blocks[0].instructions.iter().find_map(|instruction| {
                        match instruction {
                            Instruction::Parameter {
                                result,
                                index: found,
                                ..
                            } if *found == index => Some(*result),
                            _ => None,
                        }
                    })?,
                    ty: scalar_type(&param.ty)?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Function {
            name: self.function.name.clone(),
            params,
            return_type: scalar_type(&self.function.return_ty)?,
            entry_block: 0,
            blocks: self.blocks,
            span: mir::SourceSpan {
                line: self.function.line,
                column: self.function.column,
            },
        })
    }

    fn lower_statements(
        &mut self,
        block_id: BlockId,
        statements: &[mir::Stmt],
        mut variables: HashMap<String, ValueInfo>,
    ) -> Option<()> {
        for (index, statement) in statements.iter().enumerate() {
            match statement {
                mir::Stmt::Let {
                    name,
                    ty,
                    expr,
                    span,
                } => {
                    let expected = scalar_type(ty)?;
                    let value = self.lower_expr(block_id, expr, &variables, *span)?;
                    if value.ty != expected {
                        return None;
                    }
                    variables.insert(name.clone(), value);
                }
                mir::Stmt::Assign { target, expr, span } => {
                    let mir::Expr::VarRef { name, ty } = target else {
                        return None;
                    };
                    let expected = scalar_type(ty)?;
                    let value = self.lower_expr(block_id, expr, &variables, *span)?;
                    if value.ty != expected {
                        return None;
                    }
                    variables.insert(name.clone(), value);
                }
                mir::Stmt::Return { expr, span } => {
                    let value = self.lower_expr(block_id, expr, &variables, *span)?;
                    if value.ty != scalar_type(&self.function.return_ty)? {
                        return None;
                    }
                    self.blocks[block_id].terminator = Terminator::Return {
                        value: value.id,
                        span: *span,
                    };
                    return (index + 1 == statements.len()).then_some(());
                }
                mir::Stmt::If {
                    cond,
                    then_block,
                    else_block: Some(else_block),
                    span,
                } => {
                    if index + 1 != statements.len() {
                        return None;
                    }
                    let condition = self.lower_expr(block_id, cond, &variables, *span)?;
                    if condition.ty != ScalarType::Bool {
                        return None;
                    }
                    let then_id = self.new_block(*span);
                    let else_id = self.new_block(*span);
                    self.blocks[block_id].terminator = Terminator::Branch {
                        condition: condition.id,
                        then_block: then_id,
                        else_block: else_id,
                        span: *span,
                    };
                    self.lower_statements(then_id, then_block, variables.clone())?;
                    self.lower_statements(else_id, else_block, variables)?;
                    return Some(());
                }
                _ => return None,
            }
        }
        None
    }

    fn new_block(&mut self, span: mir::SourceSpan) -> BlockId {
        let id = self.blocks.len();
        self.blocks.push(BasicBlock {
            id,
            instructions: Vec::new(),
            terminator: Terminator::Unreachable,
            span,
        });
        id
    }

    fn lower_expr(
        &mut self,
        block_id: BlockId,
        expr: &mir::Expr,
        variables: &HashMap<String, ValueInfo>,
        span: mir::SourceSpan,
    ) -> Option<ValueInfo> {
        let ty = scalar_type(&expr.ty())?;
        let result = self.fresh_value();
        let instruction = match expr {
            mir::Expr::Literal(mir::LiteralValue::Int(value)) => Instruction::Const {
                result,
                value: *value,
                ty,
                span,
            },
            mir::Expr::Literal(mir::LiteralValue::Bool(value)) => Instruction::Const {
                result,
                value: i64::from(*value),
                ty,
                span,
            },
            mir::Expr::VarRef { name, .. } => return variables.get(name).copied(),
            mir::Expr::Call { name, args, .. }
                if name == "len"
                    && args.len() == 1
                    && matches!(
                        &args[0],
                        mir::Expr::Call { name, args, .. }
                            if matches!(name.as_str(), "io_read_to_string" | "std_io_read_to_string")
                                && args.is_empty()
                    ) =>
            {
                Instruction::ReadStdin {
                    result,
                    max_bytes: axiomc_backend_cranelift::I64_STDIN_BUFFER_BYTES,
                    effects: vec![String::from("stdin.read")],
                    span,
                }
            }
            mir::Expr::Call { name, args, .. } => {
                if !self.known_functions.contains(name) {
                    return None;
                }
                let args = args
                    .iter()
                    .map(|arg| {
                        self.lower_expr(block_id, arg, variables, span)
                            .map(|value| value.id)
                    })
                    .collect::<Option<Vec<_>>>()?;
                Instruction::Call {
                    result,
                    function: name.clone(),
                    args,
                    effects: Vec::new(),
                    span,
                }
            }
            mir::Expr::BinaryAdd { op, lhs, rhs, .. } => Instruction::Binary {
                result,
                op: *op,
                lhs: self.lower_expr(block_id, lhs, variables, span)?.id,
                rhs: self.lower_expr(block_id, rhs, variables, span)?.id,
                span,
            },
            mir::Expr::BinaryCompare { op, lhs, rhs, .. } => Instruction::Compare {
                result,
                op: *op,
                lhs: self.lower_expr(block_id, lhs, variables, span)?.id,
                rhs: self.lower_expr(block_id, rhs, variables, span)?.id,
                span,
            },
            mir::Expr::BinaryLogic { op, lhs, rhs, .. } => Instruction::Logic {
                result,
                op: *op,
                lhs: self.lower_expr(block_id, lhs, variables, span)?.id,
                rhs: self.lower_expr(block_id, rhs, variables, span)?.id,
                span,
            },
            _ => return None,
        };
        self.blocks[block_id].instructions.push(instruction);
        Some(ValueInfo { id: result, ty })
    }
}

pub fn lower_scalar_program(source: &mir::Program) -> Option<Program> {
    if !source.stmts.is_empty() {
        return None;
    }
    let user_functions = source
        .functions
        .iter()
        .filter(|function| !function.path.starts_with("<stdlib>"))
        .filter(|function| !function.is_property && !function.is_async && !function.is_extern)
        .collect::<Vec<_>>();
    let main = user_functions.iter().find(|function| {
        function.source_name == "main"
            && function.params.is_empty()
            && scalar_type(&function.return_ty).is_some()
    })?;
    let mut known_functions = user_functions
        .iter()
        .filter(|function| {
            scalar_type(&function.return_ty).is_some()
                && function
                    .params
                    .iter()
                    .all(|param| scalar_type(&param.ty).is_some())
        })
        .map(|function| function.name.clone())
        .collect::<HashSet<_>>();
    known_functions.remove(&main.name);

    let mut lowered = user_functions
        .iter()
        .filter(|function| function.name != main.name)
        .filter_map(|function| {
            FunctionLowerer::new(function, &known_functions).and_then(FunctionLowerer::lower)
        })
        .collect::<Vec<_>>();
    let main = FunctionLowerer::new(main, &known_functions)?.lower()?;
    let lowered_names = lowered
        .iter()
        .map(|function| function.name.clone())
        .collect::<HashSet<_>>();
    if !calls_are_resolved(&main, &lowered_names)
        || lowered
            .iter()
            .any(|function| !calls_are_resolved(function, &lowered_names))
    {
        return None;
    }
    lowered.insert(0, main);
    Some(Program {
        schema_version: "axiom.executable_mir.v0",
        entrypoint: String::from("main"),
        functions: lowered,
    })
}

fn calls_are_resolved(function: &Function, known_functions: &HashSet<String>) -> bool {
    function.blocks.iter().all(|block| {
        block
            .instructions
            .iter()
            .all(|instruction| match instruction {
                Instruction::Call { function, .. } => known_functions.contains(function),
                _ => true,
            })
    })
}

fn scalar_type(ty: &mir::Type) -> Option<ScalarType> {
    match ty {
        mir::Type::Int => Some(ScalarType::Int),
        mir::Type::Bool => Some(ScalarType::Bool),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowers_runtime_scalar_branch_into_explicit_blocks() {
        let source = mir::Program {
            path: String::from("main.ax"),
            structs: Vec::new(),
            enums: Vec::new(),
            statics: Vec::new(),
            functions: vec![mir::Function {
                name: String::from("main"),
                source_name: String::from("main"),
                path: String::from("main.ax"),
                params: Vec::new(),
                return_ty: mir::Type::Int,
                body: vec![mir::Stmt::If {
                    cond: mir::Expr::BinaryCompare {
                        op: mir::CompareOp::Gt,
                        lhs: Box::new(mir::Expr::Literal(mir::LiteralValue::Int(2))),
                        rhs: Box::new(mir::Expr::Literal(mir::LiteralValue::Int(1))),
                        ty: mir::Type::Bool,
                    },
                    then_block: vec![mir::Stmt::Return {
                        expr: mir::Expr::Literal(mir::LiteralValue::Int(7)),
                        span: mir::SourceSpan { line: 2, column: 1 },
                    }],
                    else_block: Some(vec![mir::Stmt::Return {
                        expr: mir::Expr::Literal(mir::LiteralValue::Int(1)),
                        span: mir::SourceSpan { line: 4, column: 1 },
                    }]),
                    span: mir::SourceSpan { line: 1, column: 1 },
                }],
                is_property: false,
                is_async: false,
                is_extern: false,
                extern_abi: None,
                extern_library: None,
                line: 1,
                column: 1,
            }],
            stmts: Vec::new(),
        };

        let lowered = lower_scalar_program(&source).expect("scalar MIR should lower");
        assert_eq!(lowered.schema_version, "axiom.executable_mir.v0");
        assert_eq!(lowered.functions[0].blocks.len(), 3);
        assert!(matches!(
            lowered.functions[0].blocks[0].terminator,
            Terminator::Branch { .. }
        ));
    }
}
