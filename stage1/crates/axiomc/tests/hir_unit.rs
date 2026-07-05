use super::*;
use std::path::Path;

fn parse(source: &str) -> syntax::Program {
    syntax::parse_program(source, Path::new("main.ax")).expect("parse fixture")
}

fn collect_borrow_region_facts(stmts: &[Stmt]) -> Vec<BorrowRegionFact> {
    let mut facts = Vec::new();
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                borrow_region_facts,
                ..
            } => facts.extend(borrow_region_facts.iter().cloned()),
            Stmt::If {
                then_block,
                else_block,
                ..
            } => {
                facts.extend(collect_borrow_region_facts(then_block));
                if let Some(else_block) = else_block {
                    facts.extend(collect_borrow_region_facts(else_block));
                }
            }
            Stmt::While { body, .. } => facts.extend(collect_borrow_region_facts(body)),
            Stmt::Match { arms, .. } => {
                for arm in arms {
                    facts.extend(arm.borrow_region_facts.iter().cloned());
                    facts.extend(collect_borrow_region_facts(&arm.body));
                }
            }
            Stmt::Return {
                borrow_region_facts,
                ..
            } => facts.extend(borrow_region_facts.iter().cloned()),
            Stmt::Assign { .. } | Stmt::Print { .. } | Stmt::Panic { .. } | Stmt::Defer { .. } => {}
        }
    }
    facts
}

#[test]
fn hir_records_borrow_region_fact_for_borrowed_slice_binding() {
    let parsed = parse(
        r#"
let arr: [int] = [1, 2, 3]
let s: &[int] = arr[:]
"#,
    );

    let lowered = lower(&parsed).expect("HIR lowering should accept borrowed array slice");
    let facts = collect_borrow_region_facts(&lowered.stmts);

    assert_eq!(
        facts,
        vec![BorrowRegionFact {
            binding: "s".to_string(),
            origin: BorrowRegionOrigin {
                name: "arr".to_string(),
                projection: Vec::new(),
            },
            scope: BorrowRegionScope::Binding("s".to_string()),
            source: BorrowRegionSource::Direct,
        }]
    );
}

#[test]
fn hir_records_borrow_region_fact_for_enum_payload_binding() {
    let parsed = parse(
        r#"
let values: [int] = [1, 2, 3]
let opt: Option<&[int]> = Some(values[:])
match opt {
Some(view) {
print len(view)
}
None {
print 0
}
}
"#,
    );

    let lowered = lower(&parsed).expect("HIR lowering should accept borrowed enum payload");
    let facts = collect_borrow_region_facts(&lowered.stmts);
    let enum_payload_facts = facts
        .into_iter()
        .filter(|fact| fact.binding == "view")
        .collect::<Vec<_>>();

    assert_eq!(
        enum_payload_facts,
        vec![BorrowRegionFact {
            binding: "view".to_string(),
            origin: BorrowRegionOrigin {
                name: "values".to_string(),
                projection: Vec::new(),
            },
            scope: BorrowRegionScope::Binding("view".to_string()),
            source: BorrowRegionSource::EnumPayload {
                enum_origin: BorrowRegionOrigin {
                    name: "opt".to_string(),
                    projection: Vec::new(),
                },
                variant: "Some".to_string(),
                payload_index: 0,
            },
        }]
    );
}

#[test]
fn hir_records_borrow_region_fact_for_aggregate_return() {
    let parsed = parse(
        r#"
fn pair(s: &[int]): (&[int], int) {
return (s, len(s))
}
"#,
    );

    let lowered = lower(&parsed).expect("HIR lowering should accept aggregate borrow return");
    let facts = collect_borrow_region_facts(&lowered.functions[0].body);

    assert_eq!(
        facts,
        vec![BorrowRegionFact {
            binding: "return".to_string(),
            origin: BorrowRegionOrigin {
                name: "s".to_string(),
                projection: Vec::new(),
            },
            scope: BorrowRegionScope::Return {
                function: "pair".to_string(),
                projection: vec![BorrowRegionProjection::TupleIndex(0)],
            },
            source: BorrowRegionSource::AggregateReturn,
        }]
    );
}

#[test]
fn hir_lowering_drops_parser_import_syntax() {
    let parsed = parse(
        r#"
import "./other.ax"
fn main(): int {
return 1
}
let value: int = main()
"#,
    );

    assert_eq!(parsed.imports.len(), 1, "parser owns import syntax");
    let lowered = lower(&parsed).expect("HIR lowering should ignore unresolved unused imports");

    assert_eq!(lowered.path, "main.ax");
    assert_eq!(lowered.functions.len(), 1);
    assert_eq!(lowered.stmts.len(), 1);
}

#[test]
fn hir_lowering_owns_duplicate_symbol_validation() {
    let parsed = parse(
        r#"
fn value(): int {
return 1
}
fn value(): int {
return 2
}
"#,
    );

    let error = lower(&parsed).expect_err("HIR lowering should reject duplicate symbols");
    assert_eq!(error.kind, "type");
    assert!(error.message.contains("duplicate function"));
}

#[test]
fn hir_lowering_owns_type_name_resolution() {
    let parsed = parse(
        r#"
fn main(): MissingType {
return 1
}
"#,
    );

    let error = lower(&parsed).expect_err("HIR lowering should reject unknown type names");
    assert_eq!(error.kind, "type");
    assert!(error.message.contains("unknown type \"MissingType\""));
}

#[test]
fn hir_lowers_trait_declaration_signatures() {
    let parsed = parse(
        r#"
trait Display {
fn render(self): string
}
"#,
    );

    assert_eq!(parsed.traits.len(), 1);
    assert_eq!(parsed.traits[0].methods.len(), 1);
    assert!(parsed.traits[0].methods[0].has_self);

    let lowered = lower(&parsed).expect("HIR lowering should preserve trait declarations");
    assert_eq!(lowered.traits.len(), 1);
    assert_eq!(lowered.traits[0].name, "Display");
    assert_eq!(lowered.traits[0].methods[0].name, "render");
    assert_eq!(
        lowered.traits[0].methods[0].return_ty,
        syntax::TypeName::String
    );
}

#[test]
fn hir_rejects_trait_names_in_type_positions_until_dispatch_lands() {
    let parsed = parse(
        r#"
trait Display {
fn render(self): string
}
fn show(value: Display): string {
return ""
}
"#,
    );

    let error = lower(&parsed).expect_err("trait type positions are intentionally gated");
    assert_eq!(error.kind, "type");
    assert!(
        error
            .message
            .contains("trait dispatch is not yet implemented for trait \"Display\"")
    );
}

#[test]
fn hir_rejects_trait_names_inside_trait_method_signatures_until_dispatch_lands() {
    let parsed = parse(
        r#"
trait Display {
fn render(self): string
}
trait Formatter {
fn format(value: Display): string
}
"#,
    );

    let error = lower(&parsed).expect_err("trait method trait references are gated");
    assert_eq!(error.kind, "type");
    assert!(
        error
            .message
            .contains("trait dispatch is not yet implemented for trait \"Display\"")
    );
}

#[test]
fn hir_lowers_static_trait_impl_and_bounded_generic_call() {
    let parsed = parse(
        r#"
trait Eq {
fn eq(self, other: Self): bool
}
struct Point {
x: int
y: int
}
impl Eq for Point {
fn eq(self, other: Point): bool {
return self.x == other.x
}
}
fn same<T: Eq>(left: T, right: T): bool {
return left.eq(right)
}
let a: Point = Point { x: 1, y: 2 }
let b: Point = Point { x: 1, y: 3 }
print same<Point>(a, b)
"#,
    );

    let lowered = lower(&parsed).expect("static trait impl should lower");
    assert!(
        lowered
            .functions
            .iter()
            .any(|function| function.name == "same__Point")
    );
    assert!(
        lowered
            .functions
            .iter()
            .any(|function| function.name == "Point__eq")
    );
}

#[test]
fn hir_rejects_unsatisfied_trait_bound_on_generic_instantiation() {
    let parsed = parse(
        r#"
trait Eq {
fn eq(self, other: Self): bool
}
struct Point {
x: int
}
fn same<T: Eq>(left: T, right: T): bool {
return left.eq(right)
}
let a: Point = Point { x: 1 }
let b: Point = Point { x: 1 }
print same<Point>(a, b)
"#,
    );

    let error = lower(&parsed).expect_err("missing trait impl should fail");
    assert_eq!(error.kind, "type");
    assert!(error.message.contains(
        "trait bound not satisfied: type Named(\"Point\", []) does not implement \"Eq\""
    ));
}

#[test]
fn hir_rejects_unbounded_generic_method_call_before_instantiation() {
    let parsed = parse(
        r#"
trait Eq {
fn eq(self, other: Self): bool
}
struct Point {
x: int
}
impl Eq for Point {
fn eq(self, other: Point): bool {
return self.x == other.x
}
}
fn same<T>(left: T, right: T): bool {
return left.eq(right)
}
"#,
    );

    let error = lower(&parsed).expect_err("generic method call requires a bound");
    assert_eq!(error.kind, "type");
    assert!(error.message.contains(
        "method call \"eq\" on generic parameter \"T\" requires an explicit trait bound"
    ));
}

#[test]
fn hir_rejects_unbounded_generic_method_call_through_local() {
    let parsed = parse(
        r#"
trait Eq {
fn eq(self, other: Self): bool
}
struct Point {
x: int
}
impl Eq for Point {
fn eq(self, other: Point): bool {
return self.x == other.x
}
}
fn same<T>(left: T, right: T): bool {
let tmp: T = left
return tmp.eq(right)
}
"#,
    );

    let error = lower(&parsed).expect_err("generic local method call requires a bound");
    assert_eq!(error.kind, "type");
    assert!(error.message.contains(
        "method call \"eq\" on generic parameter \"T\" requires an explicit trait bound"
    ));
}

#[test]
fn hir_rejects_unbounded_generic_method_call_through_projected_field() {
    let parsed = parse(
        r#"
trait Eq {
fn eq(self, other: Self): bool
}
struct Point {
x: int
}
struct Box<T> {
item: T
}
impl Eq for Point {
fn eq(self, other: Point): bool {
return self.x == other.x
}
}
fn same<T>(holder: Box<T>, right: T): bool {
return holder.item.eq(right)
}
"#,
    );

    let error = lower(&parsed).expect_err("generic field method call requires an explicit bound");
    assert_eq!(error.kind, "type");
    assert!(error.message.contains(
        "method call \"eq\" on generic parameter \"T\" requires an explicit trait bound"
    ));
}

#[test]
fn hir_rejects_trait_impl_missing_required_method() {
    let parsed = parse(
        r#"
trait Eq {
fn eq(self, other: Self): bool
}
struct Point {
x: int
}
impl Eq for Point {
fn same(self, other: Point): bool {
return self.x == other.x
}
}
"#,
    );

    let error = lower(&parsed).expect_err("wrong trait method should fail");
    assert_eq!(error.kind, "type");
    assert!(
        error
            .message
            .contains("impl Eq for Point is missing required method \"eq\"")
    );
}

#[test]
fn hir_rejects_trait_impl_signature_mismatch() {
    let parsed = parse(
        r#"
trait Eq {
fn eq(self, other: Self): bool
}
struct Point {
x: int
}
impl Eq for Point {
fn eq(self, other: int): bool {
return true
}
}
"#,
    );

    let error = lower(&parsed).expect_err("trait method signature mismatch should fail");
    assert_eq!(error.kind, "type");
    assert!(
        error
            .message
            .contains("method \"eq\" has parameter type int, expected Point")
    );
}

#[test]
fn parser_rejects_dyn_trait_type_with_explicit_diagnostic() {
    let error = syntax::parse_program(
        r#"
trait Display {
fn render(self): string
}
fn show(value: dyn Display): string {
return ""
}
"#,
        Path::new("main.ax"),
    )
    .expect_err("dyn Trait should stay gated");

    assert_eq!(error.kind, "parse");
    assert!(
        error
            .message
            .contains("dyn Trait type expressions require an accepted dynamic-dispatch design")
    );
}

#[test]
fn never_type_is_assignable_to_concrete_types() {
    assert!(type_assignable_to(&Type::Never, &Type::Int));
    assert!(type_assignable_to(
        &Type::Never,
        &Type::Option(Box::new(Type::String))
    ));
    assert!(!type_assignable_to(&Type::Int, &Type::Never));
}

#[test]
fn never_type_unifies_to_the_other_side() {
    assert_eq!(unify_types(&Type::Never, &Type::Int), Some(Type::Int));
    assert_eq!(unify_types(&Type::Bool, &Type::Never), Some(Type::Bool));
    assert_eq!(unify_types(&Type::Never, &Type::Never), Some(Type::Never));
    assert_eq!(unify_types(&Type::Int, &Type::Bool), None);
}

#[test]
fn hir_lowering_owns_ownership_validation() {
    let parsed = parse(
        r#"
fn consume(value: string): int {
return 1
}
let value: string = "owned"
let first: int = consume(value)
let second: int = consume(value)
"#,
    );

    let error = lower(&parsed).expect_err("HIR lowering should reject use after move");
    assert_eq!(error.kind, "ownership");
    assert!(error.message.contains("use of moved value") || error.message.contains("moved"));
}

#[test]
fn hir_lowers_property_clause_with_bool_logic() {
    let parsed = parse(
        r#"
property fn reverse_double_returns_original(input: [int]): bool {
return input == input && true && true
}
"#,
    );

    let lowered = lower(&parsed).expect("HIR lowering should accept property clauses");
    let property = &lowered.functions[0];

    assert!(property.is_property);
    assert_eq!(property.return_ty, Type::Bool);
    match &property.body[0] {
        Stmt::Return { expr, .. } => match expr {
            Expr::BinaryLogic { op, ty, .. } => {
                assert_eq!(*op, LogicOp::And);
                assert_eq!(ty, &Type::Bool);
            }
            other => panic!("expected boolean property verdict, got {other:?}"),
        },
        other => panic!("expected property return, got {other:?}"),
    }
}

#[test]
fn hir_lowers_long_bool_logic_chain_without_stack_overflow() {
    let chain = std::iter::repeat_n("flag", 32)
        .collect::<Vec<_>>()
        .join(" && ");
    let source = format!(
        r#"
fn main(): int {{
let flag: bool = true
let ok: bool = {chain}
if ok {{
return 48
}} else {{
return 1
}}
}}
"#
    );
    let parsed = parse(&source);

    let lowered = lower(&parsed).expect("HIR lowering should accept long bool logic chains");
    let main = &lowered.functions[0];

    assert_eq!(main.name, "main");
    assert_eq!(main.return_ty, Type::Int);
}

#[test]
fn hir_lowers_property_clause_with_borrowed_input() {
    let parsed = parse(
        r#"
property fn borrowed_input_len(input: [int]): bool {
let view: &[int] = input[:]
return len(view) == len(input)
}
"#,
    );

    let lowered = lower(&parsed).expect("property clauses should typecheck borrowed input reads");
    let property = &lowered.functions[0];

    assert!(property.is_property);
    assert_eq!(property.return_ty, Type::Bool);
}

#[test]
fn hir_lowers_assert_true_as_property_verdict() {
    let parsed = parse(
        r#"
property fn assertion_form(input: int): bool {
return assert_true(false || input == input || false)
}
"#,
    );

    let lowered =
        lower(&parsed).expect("property clauses should accept assert_true verdict syntax");
    let property = &lowered.functions[0];

    match &property.body[0] {
        Stmt::Return { expr, .. } => {
            assert_eq!(expr.ty(), &Type::Bool);
            assert!(
                matches!(
                    expr,
                    Expr::BinaryLogic {
                        op: LogicOp::Or,
                        ..
                    }
                ),
                "assert_true should lower to the inner boolean verdict"
            );
        }
        other => panic!("expected property return, got {other:?}"),
    }
}

#[test]
fn hir_rejects_statically_false_property_verdict() {
    let parsed = parse(
        r#"
property fn broken(input: [int]): bool {
return input != input
}
"#,
    );

    let error = lower(&parsed).expect_err("property lowering should reject false verdicts");

    assert_eq!(error.kind, "property");
    assert_eq!(error.code.as_deref(), Some("property_failed"));
    assert!(error.message.contains("property \"broken\" failed"));
    assert!(error.message.contains("input = []"));
    assert_eq!(error.path.as_deref(), Some("main.ax"));
    assert_eq!(error.line, Some(3));
    assert_eq!(error.column, Some(1));
}
