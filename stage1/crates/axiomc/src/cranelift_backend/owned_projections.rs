//! Runtime slot ownership for fixed-array and struct projections.

use super::{
    CraneliftI64Condition, CraneliftI64Expr, CraneliftI64Stmt, Expr, HashMap, I64HelperSignature,
    I64StaticBindings, Stmt, Type, i64_array_projection_key, i64_struct_projection_key,
    i64_tuple_projection_key, lower_i64_literal_index, lower_i64_runtime_projection_assign,
};

pub(super) fn lower_i64_runtime_struct_projection_let_stmts(
    name: &str,
    fields: &[crate::mir::StructFieldValue],
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    let mut stmts = Vec::new();
    for field in fields {
        stmts.extend(lower_i64_runtime_projection_value_stmts(
            i64_struct_projection_key(name, &field.name),
            &field.expr,
            locals,
            local_indexes,
            local_conditions,
            helper_signatures,
            static_bindings,
        )?);
    }
    Some(stmts)
}

fn lower_i64_runtime_projection_value_stmts(
    key: String,
    expr: &Expr,
    locals: &mut Vec<CraneliftI64Expr>,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
    helper_signatures: &HashMap<&str, I64HelperSignature>,
    static_bindings: &I64StaticBindings,
) -> Option<Vec<CraneliftI64Stmt>> {
    if let Expr::ArrayLiteral { elements, .. } = expr {
        let mut stmts = Vec::new();
        for (index, element) in elements.iter().enumerate() {
            stmts.extend(lower_i64_runtime_projection_value_stmts(
                i64_array_projection_key(&key, index),
                element,
                locals,
                local_indexes,
                local_conditions,
                helper_signatures,
                static_bindings,
            )?);
        }
        return Some(stmts);
    }
    Some(vec![lower_i64_runtime_projection_assign(
        key,
        expr,
        locals,
        local_indexes,
        local_conditions,
        helper_signatures,
        static_bindings,
    )?])
}

pub(super) fn lower_i64_runtime_fixed_array_projection_move_stmts(
    stmt: &Stmt,
    local_indexes: &mut HashMap<String, usize>,
    local_conditions: &mut HashMap<String, CraneliftI64Condition>,
) -> Option<Vec<CraneliftI64Stmt>> {
    let Stmt::Let {
        name,
        ty: Type::Array(_, Some(size)),
        expr,
        ..
    } = stmt
    else {
        return None;
    };
    let source = i64_runtime_projection_key(expr)?;
    let existing_slots = local_indexes
        .iter()
        .map(|(key, local)| (key.clone(), *local))
        .collect::<Vec<_>>();
    let mut moved_slots = Vec::new();
    for index in 0..*size {
        let source_element = i64_array_projection_key(&source, index);
        let target_element = i64_array_projection_key(name, index);
        let mut element_slots = existing_slots
            .iter()
            .filter_map(|(source_key, local)| {
                let suffix = source_key.strip_prefix(source_element.as_str())?;
                (suffix.is_empty() || suffix.starts_with('[')).then(|| {
                    (
                        source_key.clone(),
                        format!("{target_element}{suffix}"),
                        *local,
                    )
                })
            })
            .collect::<Vec<_>>();
        if element_slots.is_empty() {
            return None;
        }
        element_slots.sort_by(|left, right| left.0.cmp(&right.0));
        moved_slots.extend(element_slots);
    }
    for (source_key, target_key, local) in moved_slots {
        local_indexes.remove(source_key.as_str());
        local_indexes.insert(target_key.clone(), local);
        if let Some(condition) = local_conditions.remove(source_key.as_str()) {
            local_conditions.insert(target_key, condition);
        }
    }
    Some(Vec::new())
}

fn i64_runtime_projection_key(expr: &Expr) -> Option<String> {
    match expr {
        Expr::VarRef { name, .. } => Some(name.clone()),
        Expr::FieldAccess { base, field, .. } => Some(i64_struct_projection_key(
            &i64_runtime_projection_key(base)?,
            field,
        )),
        Expr::TupleIndex { base, index, .. } => Some(i64_tuple_projection_key(
            &i64_runtime_projection_key(base)?,
            *index,
        )),
        Expr::Index { base, index, .. } => Some(i64_array_projection_key(
            &i64_runtime_projection_key(base)?,
            lower_i64_literal_index(index)?,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn array_move(name: &str, source: &str, size: usize) -> Stmt {
        let ty = Type::Array(Box::new(Type::Bool), Some(size));
        Stmt::Let {
            name: name.into(),
            ty: ty.clone(),
            expr: Expr::VarRef {
                name: source.into(),
                ty,
            },
            span: crate::mir::SourceSpan { line: 1, column: 1 },
        }
    }

    #[test]
    fn owned_projection_move_transfers_nested_slots_and_conditions_only() {
        let mut slots = HashMap::from([
            ("source[0][0]".into(), 0),
            ("source[0][1]".into(), 1),
            ("source[1][0]".into(), 2),
            ("source[1][1]".into(), 3),
            ("source[10]".into(), 4),
            ("sibling".into(), 5),
        ]);
        let mut conditions = HashMap::from([
            ("source[0][0]".into(), CraneliftI64Condition::Literal(true)),
            ("source[1][1]".into(), CraneliftI64Condition::Literal(false)),
            ("sibling".into(), CraneliftI64Condition::Literal(true)),
        ]);
        let lowered = lower_i64_runtime_fixed_array_projection_move_stmts(
            &array_move("moved", "source", 2),
            &mut slots,
            &mut conditions,
        );
        assert_eq!(lowered, Some(Vec::new()));
        assert_eq!(
            slots,
            HashMap::from([
                ("moved[0][0]".into(), 0),
                ("moved[0][1]".into(), 1),
                ("moved[1][0]".into(), 2),
                ("moved[1][1]".into(), 3),
                ("source[10]".into(), 4),
                ("sibling".into(), 5),
            ])
        );
        assert_eq!(
            conditions,
            HashMap::from([
                ("moved[0][0]".into(), CraneliftI64Condition::Literal(true)),
                ("moved[1][1]".into(), CraneliftI64Condition::Literal(false)),
                ("sibling".into(), CraneliftI64Condition::Literal(true)),
            ])
        );
    }

    #[test]
    fn owned_projection_move_missing_element_does_not_mutate_state() {
        let mut slots = HashMap::from([("source[0]".into(), 7)]);
        let mut conditions =
            HashMap::from([("source[0]".into(), CraneliftI64Condition::Literal(true))]);
        let original_slots = slots.clone();
        let original_conditions = conditions.clone();
        assert_eq!(
            lower_i64_runtime_fixed_array_projection_move_stmts(
                &array_move("moved", "source", 2),
                &mut slots,
                &mut conditions,
            ),
            None
        );
        assert_eq!(slots, original_slots);
        assert_eq!(conditions, original_conditions);
    }
}
