//! Fresh #1584 residual cases derived from current-main behavior, not PR #1602.
use axiomc::{hir, syntax};
use std::path::Path;

fn assert_hir(source: &str, code: Option<&str>) {
    let parsed = syntax::parse_program(source, Path::new("loop-edge.ax")).expect("parse fresh case");
    match code {
        None => { hir::lower(&parsed).expect("valid loop ownership"); }
        Some(code) => {
            let error = hir::lower(&parsed).expect_err("ownership rejection required");
            assert_eq!(error.kind, "ownership", "{error:?}");
            assert_eq!(error.code.as_deref(), Some(code), "{error:?}");
        }
    }
}

#[test]
fn break_preserves_move_for_reuse() {
    assert_hir(r#"let owned: string = "kept"
while true {
let taken: string = owned
break
}
print owned
"#, Some("use_after_move"));
}

#[test]
fn break_without_reuse_is_valid() {
    assert_hir(r#"let owned: string = "kept"
while true {
let taken: string = owned
break
}
"#, None);
}

#[test]
fn continue_rejects_outer_move() {
    assert_hir(r#"let owned: string = "kept"
while true {
let taken: string = owned
continue
}
"#, Some("loop_move_outer_non_copy"));
}

#[test]
fn fallthrough_rejects_outer_move() {
    assert_hir(r#"let owned: string = "kept"
while true {
let taken: string = owned
print taken
}
"#, Some("loop_move_outer_non_copy"));
}

#[test]
fn if_break_preserves_move() {
    assert_hir(r#"let owned: string = "kept"
while true {
if true {
let taken: string = owned
break
}
}
print owned
"#, Some("use_after_move"));
}

#[test]
fn if_let_break_preserves_move() {
    assert_hir(r#"let owned: string = "kept"
while true {
if let Some(item) = Some(1) {
let taken: string = owned
break
}
}
print owned
"#, Some("use_after_move"));
}

#[test]
fn match_break_preserves_move() {
    assert_hir(r#"let owned: string = "kept"
while true {
match Some(1) {
Some(item) {
let taken: string = owned
break
}
None {
break
}
}
}
print owned
"#, Some("use_after_move"));
}

#[test]
fn const_match_break_preserves_move() {
    assert_hir(r#"let owned: string = "kept"
while true {
match 1 {
1 {
let taken: string = owned
break
}
}
}
print owned
"#, Some("use_after_move"));
}

#[test]
fn if_continue_preserves_move() {
    assert_hir(r#"let owned: string = "kept"
while true {
if true {
let taken: string = owned
continue
}
}
"#, Some("loop_move_outer_non_copy"));
}

#[test]
fn if_let_continue_preserves_move() {
    assert_hir(r#"let owned: string = "kept"
while true {
if let Some(item) = Some(1) {
let taken: string = owned
continue
}
}
"#, Some("loop_move_outer_non_copy"));
}

#[test]
fn match_continue_preserves_move() {
    assert_hir(r#"let owned: string = "kept"
while true {
match Some(1) {
Some(item) {
let taken: string = owned
continue
}
None {
break
}
}
}
"#, Some("loop_move_outer_non_copy"));
}

#[test]
fn const_match_continue_preserves_move() {
    assert_hir(r#"let owned: string = "kept"
while true {
match 1 {
1 {
let taken: string = owned
continue
}
}
}
"#, Some("loop_move_outer_non_copy"));
}

#[test]
fn condition_continue_rejects_consumption() {
    assert_hir(r#"fn predicate(value: string): bool {
return true
}
let owned: string = "kept"
while predicate(owned) {
continue
}
"#, Some("loop_move_outer_non_copy"));
}

#[test]
fn condition_fallthrough_rejects_consumption() {
    assert_hir(r#"fn predicate(value: string): bool {
return true
}
let owned: string = "kept"
while predicate(owned) {
print 1
}
"#, Some("loop_move_outer_non_copy"));
}

#[test]
fn condition_break_has_no_backedge() {
    assert_hir(r#"fn predicate(value: string): bool {
return true
}
let owned: string = "kept"
while predicate(owned) {
break
}
"#, None);
}

#[test]
fn condition_only_move_remains_after_loop() {
    assert_hir(r#"fn predicate(value: string): bool {
return true
}
let owned: string = "kept"
while predicate(owned) {
break
}
print owned
"#, Some("use_after_move"));
}

#[test]
fn condition_return_has_no_backedge() {
    assert_hir(r#"fn predicate(value: string): bool {
return true
}
fn done(owned: string): int {
while predicate(owned) {
return 1
}
return 0
}
print done("x")
"#, None);
}

#[test]
fn break_moved_projection_rejects_reuse() {
    assert_hir(r#"let pair: (string, string) = ("left", "right")
while true {
let taken: string = pair.0
break
}
let again: string = pair.0
"#, Some("use_after_move"));
}

#[test]
fn break_sibling_projection_remains_available() {
    assert_hir(r#"let pair: (string, string) = ("left", "right")
while true {
let taken: string = pair.0
break
}
let other: string = pair.1
"#, None);
}

#[test]
fn continue_rejects_projection_move() {
    assert_hir(r#"let pair: (string, string) = ("left", "right")
while true {
let taken: string = pair.0
continue
}
"#, Some("loop_move_outer_non_copy"));
}

#[test]
fn condition_projection_move_rejects_backedge() {
    assert_hir(r#"fn predicate(value: string): bool {
return true
}
let pair: (string, string) = ("left", "right")
while predicate(pair.0) {
continue
}
"#, Some("loop_move_outer_non_copy"));
}

#[test]
fn inner_break_flows_to_outer_continue() {
    assert_hir(r#"let owned: string = "kept"
while true {
while true {
let taken: string = owned
break
}
continue
}
"#, Some("loop_move_outer_non_copy"));
}

#[test]
fn inner_loop_local_value_recreated() {
    assert_hir(r#"let count: int = 0
while count < 2 {
let local: string = "new"
while true {
let taken: string = local
break
}
count = count + 1
continue
}
"#, None);
}

#[test]
fn return_only_move_does_not_poison_break() {
    assert_hir(r#"fn done(flag: bool, owned: string): int {
while true {
if flag {
let taken: string = owned
return 1
}
break
}
print owned
return 0
}
print done(false, "x")
"#, None);
}

#[test]
fn copy_and_live_assignment() {
    assert_hir(r#"let value: int = 0
while value < 2 {
let copy: int = value
value = value + 1
continue
}
print value
"#, None);
}

#[test]
fn static_false_does_not_move() {
    assert_hir(r#"let owned: string = "kept"
while false {
let taken: string = owned
continue
}
print owned
"#, None);
}

#[test]
fn move_then_assignment_stays_rejected() {
    assert_hir(r#"let owned: string = "kept"
let taken: string = owned
owned = "replacement"
"#, Some("use_after_move"));
}

#[test]
fn partial_move_then_assignment_stays_rejected() {
    assert_hir(r#"let pair: (string, string) = ("left", "right")
let taken: string = pair.0
pair = ("new", "pair")
"#, Some("use_after_move"));
}

#[test]
fn ignored_match_temporary_released() {
    assert_hir(r#"let values: [int] = [1, 2]
while true {
if let None = Some(values[:]) {
break
} else {
break
}
}
let moved: [int] = values
"#, None);
}

#[test]
fn ignored_match_preserves_outer_borrow() {
    assert_hir(r#"let values: [int] = [1, 2]
let outer: &[int] = values[:]
while true {
if let None = Some(values[:]) {
break
} else {
break
}
}
let moved: [int] = values
"#, Some("move_while_borrowed"));
}

#[test]
fn bound_match_temporary_released() {
    assert_hir(r#"let values: [int] = [1, 2]
while true {
match Some(values[:]) {
Some(view) {
print len(view)
break
}
None {
break
}
}
}
let moved: [int] = values
"#, None);
}

#[test]
fn bound_match_preserves_outer_borrow() {
    assert_hir(r#"let values: [int] = [1, 2]
let outer: &[int] = values[:]
while true {
match Some(values[:]) {
Some(view) {
print len(view)
break
}
None {
break
}
}
}
let moved: [int] = values
"#, Some("move_while_borrowed"));
}

#[test]
fn continue_match_temporary_released() {
    assert_hir(r#"let values: [int] = [1, 2]
let count: int = 0
while count < 1 {
count = count + 1
if let None = Some(values[:]) {
continue
} else {
continue
}
}
let moved: [int] = values
"#, None);
}

#[test]
fn loop_local_borrow_released_on_break() {
    assert_hir(r#"let values: [int] = [1, 2]
while true {
let view: &[int] = values[:]
print len(view)
break
}
let moved: [int] = values
"#, None);
}

#[test]
fn loop_local_borrow_does_not_release_outer() {
    assert_hir(r#"let values: [int] = [1, 2]
let outer: &[int] = values[:]
while true {
let local: &[int] = values[:]
print len(local)
break
}
let moved: [int] = values
"#, Some("move_while_borrowed"));
}

fn public_command(project: &Path, command: &str) -> (std::process::Output, serde_json::Value) {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([command, project.to_str().expect("fixture path"), "--json"])
        .output().expect("public compiler command");
    let payload = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!("{command}: invalid envelope {error}: {} {}",
            String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
    });
    (output, payload)
}

#[test]
fn public_default_cranelift_executes_nested_loop_edges() {
    assert!(which::which("cc").is_ok(), "native acceptance requires cc; never skip");
    let temp = tempfile::tempdir().expect("native fixture");
    let project = temp.path().join("edge-native");
    axiomc::new_project::create_project(&project, Some("edge-native")).expect("create project");
    // Fresh deterministic fixture exercises nearest-loop targets and mixed edges.
    let source = r#"let outer: int = 0
let total: int = 0
while outer < 2 {
outer = outer + 1
let inner: int = 0
while inner < 4 {
inner = inner + 1
if inner == 1 {
continue
}
if inner == 3 {
break
}
total = total + outer + inner
}
}
print total
print outer
"#;
    for entry in ["src/main.ax", "src/main_test.ax"] {
        std::fs::write(project.join(entry), source).expect("native source");
    }
    for command in ["check", "build", "run", "test"] {
        let (output, payload) = public_command(&project, command);
        assert!(output.status.success(), "{command}: {payload}");
        assert_eq!(payload["ok"], true, "{command}: {payload}");
        match command {
            "build" | "run" => {
                assert_eq!(payload["backend"], "cranelift");
                assert!(payload["generated_rust"].is_null());
                let binary = payload["binary"].as_str().expect("native binary");
                assert!(Path::new(binary).with_extension("cranelift.o").is_file());
                if command == "run" {
                    assert_eq!(payload["stdout"], "7\n2\n");
                    assert_eq!(payload["exit_code"], 0);
                } else {
                    let actual = std::process::Command::new(binary).output().expect("actual executable");
                    assert!(actual.status.success());
                    assert_eq!(String::from_utf8_lossy(&actual.stdout), "7\n2\n");
                }
            }
            "test" => {
                assert_eq!(payload["passed"], 1);
                assert_eq!(payload["failed"], 0);
                let cases = payload["cases"].as_array().expect("actual test cases");
                assert_eq!(cases.len(), 1);
                assert_eq!(cases[0]["stdout"], "7\n2\n");
                assert!(cases[0]["generated_rust"].is_null());
                let binary = cases[0]["binary"].as_str().expect("test executable");
                assert!(Path::new(binary).with_extension("cranelift.o").is_file());
            }
            _ => {}
        }
    }
}

#[test]
fn public_commands_reject_loop_ownership_before_backend_lowering() {
    for (label, source, code) in [
        ("break", "let value: string = \"x\"\nwhile true {\nlet taken: string = value\nbreak\n}\nprint value\n", "use_after_move"),
        ("continue", "let value: string = \"x\"\nwhile true {\nlet taken: string = value\ncontinue\n}\n", "loop_move_outer_non_copy"),
    ] {
        let temp = tempfile::tempdir().expect("negative fixture");
        let project = temp.path().join(label);
        axiomc::new_project::create_project(&project, Some(label)).expect("create negative project");
        for entry in ["src/main.ax", "src/main_test.ax"] {
            std::fs::write(project.join(entry), source).expect("negative source");
        }
        for command in ["check", "build", "run", "test"] {
            let (output, payload) = public_command(&project, command);
            assert!(!output.status.success(), "{label}/{command} unexpectedly accepted: {payload}");
            assert_eq!(payload["ok"], false);
            let errors: Vec<&serde_json::Value> = if command == "test" {
                payload["cases"].as_array().expect("failed cases").iter().map(|case| &case["error"]).collect()
            } else if payload["errors"].is_array() {
                payload["errors"].as_array().unwrap().iter().collect()
            } else {
                vec![&payload["error"]]
            };
            assert!(errors.iter().any(|error| error["kind"] == "ownership" && error["code"] == code),
                "{label}/{command} missing expected ownership code {code}: {payload}");
            assert!(!payload.to_string().contains("backend.runtime_lowering_required"));
        }
    }
}

#[test]
fn statements_after_loop_transfer_remain_unreachable() {
    for edge in ["break", "continue"] {
        let source = format!("while true {{\n{edge}\nprint 1\n}}\n");
        let parsed = syntax::parse_program(&source, Path::new("unreachable.ax")).expect("parse case");
        let error = hir::lower(&parsed).expect_err("unreachable statement must fail");
        assert_eq!(error.kind, "control");
        assert!(error.message.contains("unreachable statements"));
    }
}
