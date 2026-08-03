use super::*;

pub(crate) fn lower_i64_top_level_runtime_stmts(source_stmts: &[Stmt]) -> Option<I64ExitProgram> {
    let mut stmts = Vec::new();
    let mut cli_arrays = HashSet::new();
    let mut cli_options = HashMap::new();
    let mut stdin_remaining = HashSet::new();
    for stmt in source_stmts {
        match stmt {
            Stmt::Print {
                expr: Expr::Call { name, args, .. },
                ..
            } if is_i64_top_level_cli_arg_count_name(name) && args.is_empty() => {
                stmts.push(CraneliftI64Stmt::WriteArgCountLine {
                    stream: OutputStream::Stdout,
                });
            }
            Stmt::Match { expr, arms, .. } => {
                if let Some(stmt) = lower_i64_top_level_cli_arg_match(expr, arms) {
                    stmts.push(stmt);
                } else if let Some(stmt) =
                    lower_i64_top_level_cli_option_match(expr, arms, &cli_options)
                {
                    stmts.push(stmt);
                } else if let Some(stmt) = lower_i64_top_level_readline_match(expr, arms) {
                    stmts.push(stmt);
                } else {
                    return None;
                }
            }
            Stmt::Let {
                name,
                expr:
                    Expr::Call {
                        name: call_name,
                        args,
                        ..
                    },
                ..
            } if is_i64_top_level_cli_args_name(call_name) && args.is_empty() => {
                cli_arrays.insert(name.clone());
            }
            Stmt::Let {
                name,
                expr:
                    Expr::Call {
                        name: call_name,
                        args,
                        ..
                    },
                ..
            } if is_i64_top_level_cli_arg_name(call_name) => {
                let [Expr::Literal(LiteralValue::Int(index))] = args.as_slice() else {
                    return None;
                };
                cli_options.insert(name.clone(), usize::try_from(*index).ok()?);
            }
            Stmt::Let {
                name,
                expr:
                    Expr::Call {
                        name: call_name,
                        args,
                        ..
                    },
                ..
            } if is_i64_top_level_io_read_to_string_name(call_name) && args.is_empty() => {
                stdin_remaining.insert(name.clone());
            }
            Stmt::Print {
                expr: Expr::Call { name, args, .. },
                ..
            } if name == "first"
                && matches!(args.as_slice(), [Expr::VarRef { name, .. }] if cli_arrays.contains(name)) =>
            {
                stmts.push(CraneliftI64Stmt::WriteArgLine {
                    stream: OutputStream::Stdout,
                    index: 0,
                    fallback: String::new(),
                });
            }
            Stmt::Print {
                expr: Expr::Call { name, args, .. },
                ..
            } if name == "len"
                && matches!(args.as_slice(), [Expr::VarRef { name, .. }] if cli_arrays.contains(name)) =>
            {
                stmts.push(CraneliftI64Stmt::WriteArgCountLine {
                    stream: OutputStream::Stdout,
                });
            }
            Stmt::Print {
                expr: Expr::VarRef { name, .. },
                ..
            } if stdin_remaining.contains(name) => {
                stmts.push(CraneliftI64Stmt::WriteStdinRemaining {
                    stream: OutputStream::Stdout,
                    max_bytes: I64_STDIN_BUFFER_BYTES,
                    append_newline: true,
                });
            }
            Stmt::Print {
                expr: Expr::Literal(LiteralValue::String(text) | LiteralValue::Str(text)),
                ..
            } => stmts.push(CraneliftI64Stmt::WriteLine {
                stream: OutputStream::Stdout,
                text: text.clone(),
            }),
            Stmt::Print {
                expr: Expr::Literal(LiteralValue::Int(value)),
                ..
            } => stmts.push(CraneliftI64Stmt::WriteI64Line {
                stream: OutputStream::Stdout,
                value: CraneliftI64Expr::Literal(*value),
            }),
            Stmt::Return {
                expr: Expr::Literal(LiteralValue::Int(0)),
                ..
            } => {}
            _ => return None,
        }
    }
    Some(I64ExitProgram {
        functions: Vec::new(),
        locals: Vec::new(),
        stmts,
        body: I64ExitBody::Return(CraneliftI64Expr::Literal(0)),
    })
}

fn lower_i64_top_level_cli_arg_match(expr: &Expr, arms: &[MatchArm]) -> Option<CraneliftI64Stmt> {
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if !is_i64_top_level_cli_arg_name(name) {
        return None;
    }
    let [Expr::Literal(LiteralValue::Int(index))] = args.as_slice() else {
        return None;
    };
    let index = usize::try_from(*index).ok()?;
    let (_some_arm, none_arm) = i64_option_stmt_match_arms(arms)?;
    Some(CraneliftI64Stmt::WriteArgLine {
        stream: OutputStream::Stdout,
        index,
        fallback: i64_single_printed_static_text(&none_arm.body)?,
    })
}

fn lower_i64_top_level_cli_option_match(
    expr: &Expr,
    arms: &[MatchArm],
    cli_options: &HashMap<String, usize>,
) -> Option<CraneliftI64Stmt> {
    let Expr::VarRef { name, .. } = expr else {
        return None;
    };
    let index = *cli_options.get(name)?;
    let (_some_arm, none_arm) = i64_option_stmt_match_arms(arms)?;
    Some(CraneliftI64Stmt::WriteArgLine {
        stream: OutputStream::Stdout,
        index,
        fallback: i64_single_printed_static_text(&none_arm.body)?,
    })
}

fn lower_i64_top_level_readline_match(expr: &Expr, arms: &[MatchArm]) -> Option<CraneliftI64Stmt> {
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if !is_i64_top_level_io_readline_name(name) || !args.is_empty() {
        return None;
    }
    let (_some_arm, none_arm) = i64_option_stmt_match_arms(arms)?;
    Some(CraneliftI64Stmt::WriteStdinLine {
        stream: OutputStream::Stdout,
        fallback: i64_single_printed_static_text(&none_arm.body)?,
        max_bytes: I64_STDIN_BUFFER_BYTES,
    })
}

fn i64_single_printed_static_text(stmts: &[Stmt]) -> Option<String> {
    let [Stmt::Print { expr, .. }] = stmts else {
        return None;
    };
    match expr {
        Expr::Literal(LiteralValue::String(text) | LiteralValue::Str(text)) => Some(text.clone()),
        _ => None,
    }
}

fn is_i64_top_level_cli_args_name(name: &str) -> bool {
    matches!(name, "cli_args" | "std_cli_args")
}

fn is_i64_top_level_cli_arg_count_name(name: &str) -> bool {
    matches!(name, "cli_arg_count" | "std_cli_arg_count")
}

fn is_i64_top_level_cli_arg_name(name: &str) -> bool {
    matches!(name, "cli_arg" | "std_cli_arg")
}

fn is_i64_top_level_io_readline_name(name: &str) -> bool {
    matches!(name, "io_readline" | "std_io_readline")
}

fn is_i64_top_level_io_read_to_string_name(name: &str) -> bool {
    matches!(name, "io_read_to_string" | "std_io_read_to_string")
}
