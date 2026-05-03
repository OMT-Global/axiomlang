pub mod codegen;
pub mod dap;
pub mod diagnostics;
pub mod hir;
pub mod json_contract;
pub mod lockfile;
pub mod lsp;
pub mod manifest;
pub mod mir;
pub mod new_project;
pub mod project;
pub mod registry;
pub mod stdlib;
pub mod syntax;

#[cfg(test)]
mod tests {
    use crate::codegen::{NativeBackendKind, render_rust};
    use crate::hir;
    use crate::json_contract;
    use crate::lockfile::{render_lockfile, render_lockfile_for_project};
    use crate::manifest::{
        CapabilityConfig, TestKind, TestTarget, capability_descriptors, load_manifest,
        render_manifest,
    };
    use crate::mir;
    use crate::new_project::create_project;
    use crate::project::{
        BuildCacheStatus, BuildOptions, CheckOptions, RunOptions, TestOptions, build_project,
        build_project_with_options, check_project, check_project_with_options,
        command_for_build_output, command_for_executable, project_capabilities, run_project_tests,
        run_project_tests_with_options, run_project_with_options,
    };
    use crate::syntax::{Visibility, parse_program, parse_program_with_recovery};
    use serde::Serialize;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use tempfile::tempdir;

    #[cfg(unix)]
    const PROCESS_FIXTURE_EXECUTABLE_MODE: u32 = 0o700;

    fn render_manifest_with_capabilities(
        name: &str,
        fs: bool,
        net: bool,
        process: bool,
        env: bool,
        clock: bool,
        crypto: bool,
    ) -> String {
        format!(
            "[package]\nname = {name:?}\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = {fs}\n\"fs:write\" = {fs}\nnet = {net}\nprocess = {process}\nenv = {env}\nclock = {clock}\ncrypto = {crypto}\n"
        )
    }

    fn write_process_fixture(dir: &Path) -> String {
        #[cfg(windows)]
        {
            let path = dir.join("status.cmd");
            fs::write(&path, "@echo off\r\nexit /b 7\r\n").expect("write process fixture");
            path.to_string_lossy().into_owned()
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;

            let path = dir.join("status.sh");
            fs::write(&path, "#!/bin/sh\nexit 7\n").expect("write process fixture");
            let mut permissions = fs::metadata(&path)
                .expect("read process fixture metadata")
                .permissions();
            // This is test-only fixture setup for a tempdir-owned shell script. Keep the
            // executable bit scoped to the current user; no group/world access is needed.
            permissions.set_mode(PROCESS_FIXTURE_EXECUTABLE_MODE);
            fs::set_permissions(&path, permissions).expect("chmod process fixture");
            path.to_string_lossy().into_owned()
        }
    }

    fn rust_host_target() -> String {
        let output = rustc_command().arg("-vV").output().expect("run rustc -vV");
        assert!(output.status.success(), "rustc -vV failed");
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout
            .lines()
            .find_map(|line| line.strip_prefix("host: "))
            .map(str::to_string)
            .expect("host target")
    }

    fn rust_target_installed(target: &str) -> bool {
        let output = Command::new("rustup")
            .args(["target", "list", "--installed"])
            .output()
            .expect("run rustup target list --installed");
        assert!(
            output.status.success(),
            "rustup target list --installed failed"
        );
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line.trim() == target)
    }

    fn rustc_command() -> Command {
        let rustc = trusted_rustc_path();
        // The test harness resolves rustc to a full path before execution; PATH is trusted only
        // for this one resolution step in the developer or CI environment running the tests.
        Command::new(rustc)
    }

    fn trusted_rustc_path() -> PathBuf {
        which::which("rustc").expect("resolve rustc from trusted PATH before executing")
    }

    fn ownership_failure_fixture(case: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("ownership_failures")
            .join(case)
    }

    fn conformance_fixture() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("conformance")
    }

    fn checked_in_example_fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("examples")
            .join(name)
    }

    fn compiled_binary_command(path: impl AsRef<Path>) -> Command {
        command_for_executable(path).expect("prepare compiled binary command")
    }

    fn loopback_socket_bind_available() -> bool {
        std::net::TcpListener::bind(("127.0.0.1", 0)).is_ok()
            && std::net::UdpSocket::bind(("127.0.0.1", 0)).is_ok()
    }

    #[cfg(unix)]
    #[test]
    fn process_fixture_is_executable_only_by_owner() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("tempdir");
        let fixture = write_process_fixture(dir.path());
        let mode = fs::metadata(fixture)
            .expect("read process fixture metadata")
            .permissions()
            .mode()
            & 0o777;

        assert_eq!(mode, PROCESS_FIXTURE_EXECUTABLE_MODE);
    }

    #[test]
    fn executable_command_resolves_relative_names_against_current_dir() {
        let command =
            command_for_executable("compiled-output").expect("prepare relative executable command");
        let program = Path::new(command.get_program());
        assert!(program.is_absolute());
        assert!(program.ends_with("compiled-output"));
    }

    #[test]
    fn build_output_command_rejects_paths_outside_output_dir() {
        let dir = tempdir().expect("tempdir");
        let output_dir = dir.path().join("dist");
        let outside = dir.path().join("outside");
        let error = match command_for_build_output(&outside, &output_dir) {
            Ok(_) => panic!("outside binary path should be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn build_output_command_accepts_paths_inside_output_dir() {
        let dir = tempdir().expect("tempdir");
        let output_dir = dir.path().join("dist");
        let command = command_for_build_output(output_dir.join("compiled-output"), &output_dir)
            .expect("prepare build output command");
        assert!(Path::new(command.get_program()).starts_with(&output_dir));
    }

    #[test]
    fn new_project_writes_manifest_lockfile_and_source() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("demo");
        create_project(&project, Some("demo-app")).expect("create project");
        assert!(project.join("axiom.toml").exists());
        assert!(project.join("axiom.lock").exists());
        assert!(project.join("src/main.ax").exists());
        assert!(project.join("src/main_test.ax").exists());
        assert!(project.join("src/main_test.stdout").exists());
        let manifest = load_manifest(&project).expect("load manifest");
        assert_eq!(manifest.tests, Vec::<TestTarget>::new());
    }

    #[test]
    fn parser_lowers_functions_calls_and_while() {
        let source = "fn banner(name: string): string {\nreturn \"hello \" + name\n}\n\nfn lucky(base: int): int {\nreturn base + 2\n}\n\nfn is_ready(value: int): bool {\nreturn value == 42\n}\n\nlet answer: int = lucky(40)\nlet ready: bool = is_ready(answer)\nwhile false {\nprint \"never\"\n}\nif ready {\nprint banner(\"from stage1\")\n} else {\nprint \"bad\"\n}\nprint answer\nprint ready\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        assert_eq!(parsed.functions.len(), 3);
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        assert_eq!(mir.functions.len(), 3);
        assert_eq!(mir.statement_count(), 11);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("fn banner(name: String) -> String {"));
        assert!(rendered.contains("return format!(\"{}{}\", String::from(\"hello \"), name);"));
        assert!(rendered.contains("let answer: i64 = lucky(40);"));
        assert!(rendered.contains("let ready: bool = is_ready(answer);"));
        assert!(rendered.contains("while false {"));
        assert!(rendered.contains("if ready {"));
        assert!(rendered.contains("println!(\"{}\", banner(String::from(\"from stage1\")));"));
        assert!(rendered.contains("println!(\"{}\", ready);"));
    }

    #[test]
    fn parser_expands_declarative_statement_macros_before_lowering() {
        let source = r#"macro_rules! answer {
($value:expr) => {
return $value + 1
}
}

fn compute(): int {
answer!(41)
}

print compute()
"#;
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        assert_eq!(parsed.functions.len(), 1);
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("return 41 + 1;"));
        assert!(!rendered.contains("answer!"));
    }

    #[test]
    fn parser_expands_declarative_expression_macros_before_lowering() {
        let source = r#"macro_rules! add_one {
($value:expr) => {
$value + 1
}
}

let answer: int = add_one!(41)
print answer
"#;
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("let answer: i64 = 41 + 1;"));
    }

    #[test]
    fn check_project_expands_declarative_macros_before_typecheck() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("axiom.toml"), render_manifest("macro-demo"))
            .expect("write manifest");
        let manifest = load_manifest(dir.path()).expect("load manifest");
        fs::write(
            dir.path().join("axiom.lock"),
            render_lockfile_for_project(dir.path(), &manifest).expect("render lockfile"),
        )
        .expect("write lockfile");
        fs::create_dir_all(dir.path().join("src")).expect("create src");
        fs::write(
            dir.path().join("src/main.ax"),
            r#"macro_rules! keep_int {
($value:expr) => {
$value
}
}

let answer: int = keep_int!(42)
print answer
"#,
        )
        .expect("write source");

        let checked = check_project(dir.path()).expect("check project");
        assert_eq!(checked.statement_count, 2);
    }

    #[test]
    fn parser_does_not_expand_macro_text_inside_string_literals() {
        let source = r#"macro_rules! add_one {
($value:expr) => {
$value + 1
}
}

print "add_one!(41)"
"#;
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("add_one!(41)"));
        assert!(!rendered.contains("41 + 1"));
    }

    #[test]
    fn parser_does_not_expand_macro_suffix_of_longer_invocation_name() {
        let source = r#"macro_rules! add {
($value:expr) => {
$value + 1
}
}

let my41: int = 10
let answer: int = myadd!(41)
"#;
        let error = parse_program(source, Path::new("main.ax"))
            .and_then(|parsed| hir::lower(&parsed))
            .expect_err("longer macro invocation name should not match add! suffix");
        assert!(
            error.message.contains("unknown function")
                || error.message.contains("unknown value")
                || error.message.contains("invalid identifier"),
            "unexpected diagnostic: {error:?}",
        );
    }

    #[test]
    fn parser_rejects_nested_macro_rules_definitions() {
        let source = r#"fn compute(): int {
macro_rules! add_one {
($value:expr) => {
$value + 1
}
}
return add_one!(41)
}
"#;
        let error = parse_program(source, Path::new("main.ax"))
            .expect_err("nested macro definitions should be rejected");
        assert!(
            error.message.contains("top level"),
            "unexpected diagnostic: {error:?}",
        );
    }

    #[test]
    fn parser_expands_macro_parameters_with_shared_prefixes() {
        let source = r#"macro_rules! pick_second {
($a:expr, $ab:expr) => {
$ab
}
}

let answer: int = pick_second!(1, 2)
print answer
"#;
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("let answer: i64 = 2;"));
        assert!(!rendered.contains("1b;"));
    }

    #[test]
    fn parser_does_not_substitute_macro_parameters_inside_template_strings() {
        let source = r#"macro_rules! label {
($value:expr) => {
print "$value"
}
}

label!(41)
"#;
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("$value"));
        assert!(!rendered.contains("41"));
    }

    #[test]
    fn parser_ignores_string_braces_when_collecting_top_level_macros() {
        let source = r#"print "{"

macro_rules! add_one {
($value:expr) => {
$value + 1
}
}

let answer: int = add_one!(41)
"#;
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("let answer: i64 = 41 + 1;"));
    }

    #[test]
    fn parser_rejects_nested_macro_rules_even_after_string_close_brace() {
        let source = r#"fn compute(): int {
print "}"
macro_rules! add_one {
($value:expr) => {
$value + 1
}
}
return add_one!(41)
}
"#;
        let error = parse_program(source, Path::new("main.ax"))
            .expect_err("nested macro definitions should be rejected");
        assert!(
            error.message.contains("top level"),
            "unexpected diagnostic: {error:?}",
        );
    }

    #[test]
    fn parser_bounds_recursive_declarative_macro_expansion() {
        let source = r#"macro_rules! spin {
() => {
spin!()
}
}

spin!()
"#;
        let error = parse_program(source, Path::new("main.ax"))
            .expect_err("recursive macro expansion should be bounded");
        assert!(error.message.contains("exceeded bounded depth"));
    }

    #[test]
    fn parser_lowers_panic_statement() {
        let source = "fn fail(): int {\npanic(\"boom\")\n}\n\nprint 0\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("fn axiom_panic(message: String) -> ! {"));
        assert!(rendered.contains("axiom_runtime_error(\"panic\", &message)"));
        assert!(rendered.contains("axiom_panic(String::from(\"boom\"));"));
    }

    #[test]
    fn parser_lowers_panic_statement_with_whitespace_before_paren() {
        let source = "fn fail(): int {\npanic (\"boom\")\n}\n\nprint 0\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("axiom_panic(String::from(\"boom\"));"));
    }

    #[test]
    fn parser_lowers_panic_statement_with_tab_before_paren() {
        let source = "fn fail(): int {\npanic\t(\"boom\")\n}\n\nprint 0\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("axiom_panic(String::from(\"boom\"));"));
    }

    #[test]
    fn parser_lowers_panic_statement_with_generic_call_argument() {
        let source = "fn label<T>(value: T): string {\nreturn \"boom\"\n}\n\nfn require<T>(flag: bool, value: T): T {\nif flag {\nreturn value\n} else {\npanic(label<T>(value))\n}\n}\n\nlet answer: int = require<int>(true, 7)\nprint answer\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("fn label__int(value: i64) -> String {"));
        assert!(rendered.contains("fn require__int(flag: bool, value: i64) -> i64 {"));
        assert!(rendered.contains("axiom_panic(label__int(value));"));
        assert!(rendered.contains("let answer: i64 = require__int(true, 7);"));
    }

    #[test]
    fn parser_lowers_defer_statement() {
        let source = r#"fn trace(label: string): int {
print label
return 0
}

fn demo(): int {
defer trace("cleanup")
return 7
}

print demo()
"#;
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        let cleanup = rendered
            .find("let _ = trace(String::from(\"cleanup\"));")
            .expect("defer cleanup rendered");
        let ret = rendered.find("return 7;").expect("return rendered");
        assert!(
            cleanup < ret,
            "defer should render before return: {rendered}"
        );
    }

    #[test]
    fn run_project_executes_defer_on_return_panic_and_nested_scope() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("axiom.toml"), render_manifest("defer-demo"))
            .expect("write manifest");
        let manifest = load_manifest(dir.path()).expect("load manifest");
        fs::write(
            dir.path().join("axiom.lock"),
            render_lockfile_for_project(dir.path(), &manifest).expect("render lockfile"),
        )
        .expect("write lockfile");
        fs::create_dir_all(dir.path().join("src")).expect("create src");
        fs::write(
            dir.path().join("src/main.ax"),
            r#"fn trace(label: string): int {
print label
return 0
}

fn nested(flag: bool): int {
defer trace("outer-1")
defer trace("outer-2")
if flag {
defer trace("inner")
return 10
}
return 20
}

fn fail(): int {
defer trace("panic-cleanup")
panic("boom")
}

print nested(true)
print nested(false)
if false {
print fail()
}
"#,
        )
        .expect("write source");

        let built = build_project(dir.path()).expect("build project");
        let output = compiled_binary_command(Path::new(&built.binary))
            .output()
            .expect("run binary");
        assert!(output.status.success(), "binary failed: {output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(
            stdout,
            "inner
outer-2
outer-1
10
outer-2
outer-1
20
"
        );

        fs::write(
            dir.path().join("src/main.ax"),
            r#"fn trace(label: string): int {
print label
return 0
}

fn fail(): int {
defer trace("panic-cleanup")
panic("boom")
}

print fail()
"#,
        )
        .expect("rewrite source");
        let built = build_project(dir.path()).expect("rebuild project");
        let output = compiled_binary_command(Path::new(&built.binary))
            .output()
            .expect("run panic binary");
        assert!(
            !output.status.success(),
            "panic binary unexpectedly succeeded"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            stdout,
            "panic-cleanup
"
        );
        assert!(
            stderr.contains("\"kind\":\"panic\""),
            "stderr missing panic report: {stderr}"
        );
    }

    #[test]
    fn parser_tracks_package_visibility() {
        let source = "pub(pkg) const ANSWER: int = 42\npub(pkg) type Id = int\npub(pkg) struct BuildInfo {\nlabel: string\n}\npub(pkg) enum Status {\nReady\n}\npub(pkg) fn answer(): int {\nreturn ANSWER\n}\npub(pkg) async fn answer_later(): int {\nreturn ANSWER\n}\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        assert_eq!(parsed.consts[0].visibility, Visibility::Package);
        assert_eq!(parsed.type_aliases[0].visibility, Visibility::Package);
        assert_eq!(parsed.structs[0].visibility, Visibility::Package);
        assert_eq!(parsed.enums[0].visibility, Visibility::Package);
        assert_eq!(parsed.functions[0].visibility, Visibility::Package);
        assert_eq!(parsed.functions[1].visibility, Visibility::Package);
        assert!(parsed.functions[1].is_async);
    }

    #[test]
    fn parser_rejects_package_re_exports_explicitly() {
        for source in [
            "pub(pkg) import \"math.ax\"\nprint \"skip\"\n",
            "pub(pkg) use \"math.ax\"\nprint \"skip\"\n",
        ] {
            let error = parse_program(source, Path::new("main.ax"))
                .expect_err("package re-exports should fail during parsing");
            assert_eq!(error.kind, "parse");
            assert!(error.message.contains("does not support re-exports"));
        }
    }

    #[test]
    fn parser_rejects_for_loops_explicitly() {
        let source = "fn main(): int {\nfor value in [1, 2, 3] {\nprint value\n}\nreturn 0\n}\n";
        let error = parse_program(source, Path::new("main.ax"))
            .expect_err("for loops should fail with an explicit parser diagnostic");
        assert_eq!(error.kind, "parse");
        assert_eq!(error.line, Some(2));
        assert_eq!(error.column, Some(1));
        assert!(error.message.contains("does not support `for` loops yet"));
    }

    #[test]
    fn parser_recovery_reports_stable_top_level_errors() {
        let source = "import math.ax\nlet answer int = 42\nprint answer\nelse {\n";
        let diagnostics = parse_program_with_recovery(source, Path::new("main.ax"))
            .expect_err("recovering parser should report all top-level parse errors");

        assert_eq!(diagnostics.len(), 3);
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.line)
                .collect::<Vec<_>>(),
            vec![Some(1), Some(2), Some(4)]
        );
        assert_eq!(
            diagnostics[0].message,
            "import must use a quoted relative path"
        );
        assert_eq!(diagnostics[1].message, "let binding is missing ':'");
        assert_eq!(diagnostics[2].message, "unexpected else block");
    }

    #[test]
    fn parser_recovery_resynchronizes_top_level_statements_from_their_start() {
        let source =
            "if true {\nfor value in [1] {\nprint value\n}\n}\nlet answer int = 42\nprint answer\n";
        let diagnostics = parse_program_with_recovery(source, Path::new("main.ax"))
            .expect_err("recovering parser should skip the failed top-level statement body");

        assert_eq!(diagnostics.len(), 2);
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.line)
                .collect::<Vec<_>>(),
            vec![Some(2), Some(6)]
        );
        assert!(
            diagnostics[0]
                .message
                .contains("does not support `for` loops yet")
        );
        assert_eq!(diagnostics[1].message, "let binding is missing ':'");
    }

    #[test]
    fn parser_error_preserves_related_recovery_diagnostics_for_cli_payloads() {
        let source = "import math.ax\nlet answer int = 42\n";
        let error = parse_program(source, Path::new("main.ax"))
            .expect_err("default parser should fail with primary diagnostic");

        assert_eq!(error.message, "import must use a quoted relative path");
        assert_eq!(error.related.len(), 1);
        assert_eq!(error.related[0].message, "let binding is missing ':'");
        assert_eq!(error.related[0].line, Some(2));
    }

    #[test]
    fn parser_rejects_match_arm_guards() {
        let source = "enum OptionInt {\nSome(int)\nNone\n}\n\nfn describe(value: OptionInt): int {\nmatch value {\nSome(n) if n > 0 {\nreturn n\n}\nNone {\nreturn 0\n}\n}\n}\n";
        let error = parse_program(source, Path::new("main.ax"))
            .expect_err("match arm guards should fail during parsing");
        assert_eq!(error.kind, "parse");
        assert_eq!(error.message, "match arm guards are not supported yet");
        assert_eq!(error.line, Some(8));
        assert_eq!(error.column, Some(9));
    }

    #[test]
    fn parser_accepts_match_arm_identifiers_containing_if() {
        let source = "enum Prize {\nGift(int)\nNone\n}\n\nmatch Gift(3) {\nGift(gift_value) {\nprint gift_value\n}\nNone {\nprint 0\n}\n}\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");

        match &parsed.stmts[0] {
            crate::syntax::Stmt::Match { arms, .. } => {
                assert_eq!(arms.len(), 2);
                assert_eq!(arms[0].variant, "Gift");
                assert_eq!(arms[0].bindings, vec!["gift_value".to_string()]);
            }
            other => panic!("expected match statement, got {other:?}"),
        }
    }

    #[test]
    fn parser_accepts_named_match_arm_identifiers_containing_if() {
        let source = "enum Prize {\nGift { gift_value: int }\nNone\n}\n\nmatch Gift { gift_value: 3 } {\nGift { gift_value } {\nprint gift_value\n}\nNone {\nprint 0\n}\n}\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");

        match &parsed.stmts[0] {
            crate::syntax::Stmt::Match { arms, .. } => {
                assert_eq!(arms.len(), 2);
                assert_eq!(arms[0].variant, "Gift");
                assert!(arms[0].is_named);
                assert_eq!(arms[0].bindings, vec!["gift_value".to_string()]);
            }
            other => panic!("expected match statement, got {other:?}"),
        }
    }

    #[test]
    fn parser_rejects_nested_match_patterns() {
        let source = "enum Pair {\nWrap((int, bool))\n}\n\nmatch Wrap((1, true)) {\nWrap((count, true)) {\nprint count\n}\n}\n";
        let error = parse_program(source, Path::new("main.ax"))
            .expect_err("nested match patterns should fail during parsing");
        assert_eq!(error.kind, "parse");
        assert_eq!(error.message, "nested match patterns are not supported yet");
        assert_eq!(error.line, Some(6));
        assert_eq!(error.column, Some(6));
    }

    #[test]
    fn parser_reports_nested_match_pattern_at_offending_positional_binding() {
        let source = "enum Pair {\nWrap(int, (int, bool))\n}\n\nmatch Wrap(1, (2, true)) {\nWrap(value, (count, true)) {\nprint value\n}\n}\n";
        let error = parse_program(source, Path::new("main.ax"))
            .expect_err("nested positional bindings should report the offending binding");
        assert_eq!(error.kind, "parse");
        assert_eq!(error.message, "nested match patterns are not supported yet");
        assert_eq!(error.line, Some(6));
        assert_eq!(error.column, Some(13));
    }

    #[test]
    fn parser_rejects_nested_named_match_patterns() {
        let source = "enum Event {\nTick { payload: (int, bool) }\n}\n\nmatch Tick { payload: (1, true) } {\nTick { payload: (count, true) } {\nprint count\n}\n}\n";
        let error = parse_program(source, Path::new("main.ax"))
            .expect_err("nested named match patterns should fail during parsing");
        assert_eq!(error.kind, "parse");
        assert_eq!(error.message, "nested match patterns are not supported yet");
        assert_eq!(error.line, Some(6));
        assert_eq!(error.column, Some(15));
    }

    #[test]
    fn parser_reports_nested_match_pattern_at_offending_named_binding() {
        let source = "enum Event {\nTick { tag: int, payload: (int, bool) }\n}\n\nmatch Tick { tag: 1, payload: (2, true) } {\nTick { tag, payload: (count, true) } {\nprint tag\n}\n}\n";
        let error = parse_program(source, Path::new("main.ax"))
            .expect_err("nested named bindings should report the offending binding");
        assert_eq!(error.kind, "parse");
        assert_eq!(error.message, "nested match patterns are not supported yet");
        assert_eq!(error.line, Some(6));
        assert_eq!(error.column, Some(20));
    }

    #[test]
    fn parser_lowers_generic_functions_to_monomorphized_copies() {
        let source = "fn identity<T>(value: T): T {\nreturn value\n}\n\nfn singleton<T>(value: T): [T] {\nreturn [value]\n}\n\nlet answer: int = identity<int>(42)\nlet label: string = identity<string>(\"stage1\")\nlet values: [int] = singleton<int>(answer)\nprint answer\nprint label\nprint len(values)\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        assert_eq!(parsed.functions.len(), 2);
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        assert_eq!(mir.functions.len(), 3);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("fn identity__int(value: i64) -> i64 {"));
        assert!(rendered.contains("fn identity__string(value: String) -> String {"));
        assert!(rendered.contains("fn singleton__int(value: i64) -> Vec<i64> {"));
        assert!(rendered.contains("let answer: i64 = identity__int(42);"));
        assert!(
            rendered.contains("let label: String = identity__string(String::from(\"stage1\"));")
        );
        assert!(rendered.contains("let values: Vec<i64> = singleton__int(answer);"));
        assert!(!rendered.contains("fn identity("));
        assert!(!rendered.contains("fn singleton("));
    }

    #[test]
    fn parser_lowers_nested_generic_instantiations() {
        let source = "fn identity<T>(value: T): T {\nreturn value\n}\n\nfn pair<T>(value: T): (T, T) {\nlet left: T = identity<T>(value)\nreturn (left, left)\n}\n\nlet both: (int, int) = pair<int>(7)\nprint both.0\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("fn pair__int(value: i64) -> (i64, i64) {"));
        assert!(rendered.contains("fn identity__int(value: i64) -> i64 {"));
        assert!(rendered.contains("let left: i64 = identity__int(value);"));
    }

    #[test]
    fn parser_lowers_generic_structs_and_enums_to_monomorphized_copies() {
        let source = "struct Box<T> {\nvalue: T\n}\n\nstruct Buckets<T> {\nitems: [T]\nby_name: {string: T}\n}\n\nenum Outcome<T, E> {\nOkValue(T)\nErrValue(E)\n}\n\nlet values: [int] = [1, 2]\nlet table: {string: int} = {\"one\": 1}\nlet boxed: Box<int> = Box { value: 42 }\nlet buckets: Buckets<int> = Buckets { items: values, by_name: table }\nlet outcome: Outcome<int, string> = OkValue(7)\nprint boxed.value\nprint len(buckets.items)\nmatch outcome {\nOkValue(value) {\nprint value\n}\nErrValue(error) {\nprint error\n}\n}\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        assert_eq!(parsed.structs.len(), 2);
        assert_eq!(parsed.enums.len(), 1);
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("struct Box__int {"));
        assert!(rendered.contains("struct Buckets__int {"));
        assert!(rendered.contains("items: Vec<i64>,"));
        assert!(rendered.contains("by_name: HashMap<String, i64>,"));
        assert!(rendered.contains("enum Outcome__int__string {"));
        assert!(rendered.contains("OkValue(i64),"));
        assert!(rendered.contains("ErrValue(String),"));
        assert!(rendered.contains("let boxed: Box__int = Box__int {"));
        assert!(rendered.contains("let buckets: Buckets__int = Buckets__int {"));
        assert!(
            rendered
                .contains("let outcome: Outcome__int__string = Outcome__int__string::OkValue(7);")
        );
    }

    #[test]
    fn parser_lowers_tuple_generic_type_arguments() {
        let source = "fn identity<T>(value: T): T {\nreturn value\n}\n\nlet pair: (int, int) = identity<(int, int)>((1, 2))\nprint pair.1\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("fn identity__tuple_int_int(value: (i64, i64)) -> (i64, i64) {"));
        assert!(rendered.contains("let pair: (i64, i64) = identity__tuple_int_int((1, 2));"));
    }

    #[test]
    fn render_rust_uses_structured_runtime_error_reporting() {
        let source = "fn crash(values: [int]): int {\nreturn values[1]\n}\n\nprint crash([7])\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("fn axiom_install_panic_hook() {"));
        assert!(rendered.contains("fn axiom_runtime_report(kind: &str, message: &str) {"));
        assert!(rendered.contains("fn axiom_runtime_error(kind: &str, message: &str) -> ! {"));
        assert!(rendered.contains("let result = panic::catch_unwind(|| {"));
        assert!(!rendered.contains(".expect("));
        assert!(!rendered.contains("std::process::exit"));
        assert!(!rendered.contains("assert!("));
        assert!(!rendered.contains("Axiom stack trace"));
    }

    #[test]
    fn panic_statement_requires_single_string_argument() {
        let source = "fn fail(): int {\npanic(1)\n}\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let error = hir::lower(&parsed).expect_err("panic should reject non-string arguments");
        assert_eq!(error.kind, "type");
        assert!(error.message.contains("panic expects a string argument"));
    }

    #[test]
    fn panic_statement_rejects_wrong_arity() {
        let source = "fn fail(): int {\npanic()\n}\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let error = hir::lower(&parsed).expect_err("panic should reject missing arguments");
        assert_eq!(error.kind, "type");
        assert!(error.message.contains("panic expects 1 argument, got 0"));
    }

    #[test]
    fn panic_statement_without_parens_rejects_missing_argument() {
        let source = "fn fail(): int {\npanic\n}\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let error =
            hir::lower(&parsed).expect_err("bare panic should reject the non-call statement form");
        assert_eq!(error.kind, "type");
        assert!(
            error
                .message
                .contains("panic statement expects `panic(\"message\")`")
        );
    }

    #[test]
    fn panic_statement_rejects_multiple_arguments() {
        let source = "fn fail(): int {\npanic(\"boom\", \"again\")\n}\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let error = hir::lower(&parsed).expect_err("panic should reject extra arguments");
        assert_eq!(error.kind, "type");
        assert!(error.message.contains("panic expects 1 argument, got 2"));
    }

    #[test]
    fn panic_statement_rejects_type_arguments() {
        let source = "fn fail(): int {\npanic<string>(\"boom\")\n}\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let error = hir::lower(&parsed).expect_err("panic should reject type arguments");
        assert_eq!(error.kind, "type");
        assert!(
            error
                .message
                .contains("panic does not accept type arguments")
        );
    }

    #[test]
    fn render_rust_uses_checked_slice_access() {
        let source =
            "let values: [int] = [1]\nlet window: &[int] = values[0:1]\nprint len(window)\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("match values.get(start..end) {"));
        assert!(rendered.contains("match values.get_mut(start..end) {"));
        assert!(!rendered.contains("&values[start..end]"));
        assert!(!rendered.contains("&mut values[start..end]"));
        assert!(!rendered.contains("assert!("));
        assert!(!rendered.contains("debug_assert!("));
    }

    #[test]
    fn render_rust_documents_network_address_filtering() {
        let source = "print true\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("fn axiom_resolve_public_socket_addrs("));
        assert!(rendered.contains("Network intrinsics reject private, loopback, link-local,"));
        assert!(rendered.contains("addr.to_ipv4_mapped()"));
    }

    #[test]
    fn render_rust_clamps_socket_timeouts_and_bounds_tcp_request_reads() {
        let source = "print true\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("timeout_ms.clamp(1, 30_000)"));
        assert!(rendered.contains("let mut total_read = 0usize;"));
        assert!(rendered.contains("if total_read >= 65_536"));
        assert!(rendered.contains("stream.shutdown(std::net::Shutdown::Write).ok()?;"));
    }

    #[test]
    fn render_rust_restricts_http_server_binds_to_loopback() {
        let source = "print http_serve_once(\"127.0.0.1:0\", \"ok\")\nprint http_serve_route(\"127.0.0.1:0\", \"/\", \"ok\", 1)\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower_with_capabilities(
            &parsed,
            &CapabilityConfig {
                net: true,
                ..CapabilityConfig::default()
            },
        )
        .expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("fn axiom_http_loopback_bind_addr("));
        assert!(rendered.contains("addr.ip().is_loopback()"));
        assert_eq!(
            rendered
                .matches("axiom_http_loopback_bind_addr(bind.as_str())")
                .count(),
            2
        );
    }

    #[test]
    fn render_rust_keeps_http_response_size_guards() {
        let source = "match http_get(\"http://example.com/\") {\nSome(_body) {\nprint true\n}\nNone {\nprint false\n}\n}\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower_with_capabilities(
            &parsed,
            &CapabilityConfig {
                net: true,
                ..CapabilityConfig::default()
            },
        )
        .expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("const MAX_HEADER_BYTES: usize = 64 * 1024;"));
        assert!(rendered.contains("const MAX_BODY_BYTES: usize = 1024 * 1024;"));
        assert!(rendered.contains("axiom_resolve_public_socket_addrs(clean_host.as_str(), port)?"));
        assert!(rendered.contains("TcpStream::connect_timeout(&addr, Duration::from_secs(5))"));
    }

    #[test]
    fn render_rust_gates_https_tls_runtime_to_linux() {
        let source = "match http_get(\"https://example.com/\") {\nSome(_body) {\nprint true\n}\nNone {\nprint false\n}\n}\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower_with_capabilities(
            &parsed,
            &CapabilityConfig {
                net: true,
                ..CapabilityConfig::default()
            },
        )
        .expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("fn axiom_https_get_native_tls("));
        assert!(rendered.contains("#[cfg(target_os = \"linux\")]"));
        assert!(rendered.contains("https TLS is not supported on this platform in stage1"));
    }

    #[test]
    fn render_rust_strips_crlf_from_http_request_parts() {
        let source = "match http_get(\"http://example.com/\") {\nSome(_body) {\nprint true\n}\nNone {\nprint false\n}\n}\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower_with_capabilities(
            &parsed,
            &CapabilityConfig {
                net: true,
                ..CapabilityConfig::default()
            },
        )
        .expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("fn axiom_http_strip_crlf(value: &str) -> String {"));
        assert!(rendered.contains("*ch != '\\r' && *ch != '\\n'"));
        assert!(rendered.contains("let clean_host = axiom_http_strip_crlf(host);"));
        assert!(rendered.contains("let clean_path = axiom_http_strip_crlf(path);"));
        assert!(rendered.contains("axiom_resolve_public_socket_addrs(clean_host.as_str(), port)?"));
        assert!(rendered.contains("axiom_http_request(clean_host.as_str(), clean_path.as_str())"));
        assert!(!rendered.contains("axiom_resolve_public_socket_addrs(host, port)?"));
        assert!(!rendered.contains("axiom_http_request(host, path)"));
    }

    #[test]
    fn parser_lowers_struct_literals_and_field_access() {
        let source = "struct BuildInfo {\nname: string\ncount: int\n}\n\nfn count_of(info: BuildInfo): int {\nreturn info.count\n}\n\nlet info: BuildInfo = BuildInfo { name: \"stage1\", count: 42 }\nprint count_of(info)\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        assert_eq!(parsed.structs.len(), 1);
        let hir = hir::lower(&parsed).expect("lower");
        assert_eq!(hir.structs.len(), 1);
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("struct BuildInfo {"));
        assert!(rendered.contains("name: String,"));
        assert!(rendered.contains("count: i64,"));
        assert!(rendered.contains(
            "let info: BuildInfo = BuildInfo { name: String::from(\"stage1\"), count: 42 };"
        ));
        assert!(rendered.contains("return (info).count;"));
    }

    #[test]
    fn parser_lowers_arrays_and_indexing() {
        let source = "fn answer(values: [int]): int {\nreturn values[1]\n}\n\nlet values: [int] = [40, 42]\nprint answer(values)\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("fn answer(values: Vec<i64>) -> i64 {"));
        assert!(rendered.contains("return axiom_array_get(&values, 1);"));
        assert!(rendered.contains("let values: Vec<i64> = vec![40, 42];"));
        assert!(rendered.contains("println!(\"{}\", answer(values));"));
    }

    #[test]
    fn parser_lowers_array_slices() {
        let source = "fn tail(values: &[int]): &[int] {\nreturn values[1:]\n}\n\nfn string_tail_len(values: &[string]): int {\nlet rest: &[string] = values[1:]\nreturn len(rest)\n}\n\nlet values: [int] = [3, 7, 9, 11]\nlet window: &[int] = tail(values[:])\nprint first(window)\nprint last(window)\nprint len(window)\nlet labels: [string] = [\"build\", \"test\", \"ship\"]\nprint string_tail_len(labels[:])\nlet words: [string] = [\"alpha\", \"beta\"]\nprint first(words)\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("fn tail<'a>(values: &'a [i64]) -> &'a [i64] {"));
        assert!(rendered.contains("return axiom_slice_view(values, Some(1), None);"));
        assert!(rendered.contains("fn string_tail_len<'a>(values: &'a [String]) -> i64 {"));
        assert!(
            rendered.contains("let window: &[i64] = tail(axiom_slice_view(&values, None, None));")
        );
        assert!(
            rendered
                .contains(
                    "println!(\"{}\", { let values = window; let index = 0; axiom_array_get(values, index) });"
                )
        );
        assert!(
            rendered.contains(
                "println!(\"{}\", { let values = window; let index = axiom_last_index(values.len()); axiom_array_get(values, index) });"
            )
        );
        assert!(rendered.contains("return (rest).len() as i64;"));
        assert!(
            rendered
                .contains(
                    "println!(\"{}\", { let values = words; let index = 0; axiom_array_take(values, index) });"
                )
        );
    }

    #[test]
    fn parser_lowers_mutable_slice_signatures() {
        let source = "fn passthrough(values: &mut [int]): &mut [int] {\nreturn values\n}\n\nfn count(values: &mut [string]): int {\nreturn len(values)\n}\n\nprint 0\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("fn passthrough<'a>(values: &'a mut [i64]) -> &'a mut [i64] {"));
        assert!(rendered.contains("return values;"));
        assert!(rendered.contains("fn count<'a>(values: &'a mut [String]) -> i64 {"));
        assert!(rendered.contains("return (values).len() as i64;"));
    }

    #[test]
    fn parser_lowers_mutable_slice_views() {
        let source = "fn tail(values: &mut [int]): &mut [int] {\nlet rest: &mut [int] = values[1:]\nreturn rest\n}\n\nfn local_tail_len(): int {\nlet values: [int] = [3, 7, 9, 11]\nlet rest: &mut [int] = values[1:]\nreturn len(rest)\n}\n\nprint 0\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(
            rendered
                .contains("let rest: &mut [i64] = axiom_slice_view_mut(values, Some(1), None);")
        );
        assert!(
            rendered.contains(
                "let rest: &mut [i64] = axiom_slice_view_mut(&mut values, Some(1), None);"
            )
        );
        assert!(rendered.contains("fn axiom_slice_view_mut<'a, T>(values: &'a mut [T], start: Option<i64>, end: Option<i64>) -> &'a mut [T] {"));
    }

    #[test]
    fn parser_lowers_borrowed_structs_and_enums() {
        let source = "struct Window {\nview: &[int]\n}\n\nenum Snapshot {\nWindow(Window)\nNamed { window: Window }\n}\n\nfn tail(values: &[int]): Window {\nreturn Window { view: values[1:] }\n}\n\nfn read(snapshot: Snapshot): int {\nmatch snapshot {\nWindow(window) {\nreturn first(window.view)\n}\nNamed { window } {\nreturn last(window.view)\n}\n}\n}\n\nlet numbers: [int] = [3, 7, 9, 11]\nlet window: Window = tail(numbers[:])\nprint first(window.view)\nprint read(Named { window: tail(numbers[:]) })\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("struct Window<'a> {"));
        assert!(rendered.contains("view: &'a [i64],"));
        assert!(rendered.contains("enum Snapshot<'a> {"));
        assert!(rendered.contains("Window(Window<'a>),"));
        assert!(rendered.contains("window: Window<'a>,"));
        assert!(rendered.contains("fn tail<'a>(values: &'a [i64]) -> Window<'a> {"));
        assert!(rendered.contains("fn read<'a>(snapshot: Snapshot<'a>) -> i64 {"));
        assert!(
            rendered
                .contains("let window: Window<'_> = tail(axiom_slice_view(&numbers, None, None));")
        );
        assert!(
            rendered.contains(
                "println!(\"{}\", read(Snapshot::Named { window: tail(axiom_slice_view(&numbers, None, None)) }));"
            )
        );
    }

    #[test]
    fn parser_lowers_tuples_and_tuple_indexing() {
        let source = "fn label(pair: (int, string)): string {\nreturn pair.1\n}\n\nlet pair: (int, string) = (7, \"stage1 tuples\")\nprint pair.0\nprint label(pair)\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("fn label(pair: (i64, String)) -> String {"));
        assert!(rendered.contains("return (pair).1;"));
        assert!(
            rendered.contains("let pair: (i64, String) = (7, String::from(\"stage1 tuples\"));")
        );
        assert!(rendered.contains("println!(\"{}\", (pair).0);"));
        assert!(rendered.contains("println!(\"{}\", label(pair));"));
    }

    #[test]
    fn parser_lowers_maps_and_indexing() {
        let source =
            "let scores: {string: int} = {\"build\": 7, \"deploy\": 9}\nprint scores[\"deploy\"]\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("let scores: HashMap<String, i64> = HashMap::from(["));
        assert!(rendered.contains("(String::from(\"build\"), 7)"));
        assert!(rendered.contains("(String::from(\"deploy\"), 9)"));
        assert!(
            rendered
                .contains("println!(\"{}\", axiom_map_get(&scores, &String::from(\"deploy\")));")
        );
    }

    #[test]
    fn parser_lowers_option_and_result() {
        let source = "struct BuildInfo {\nlabel: string\n}\n\nfn maybe(ready: bool): Option<BuildInfo> {\nif ready {\nreturn Some(BuildInfo { label: \"ok\" })\n}\nreturn None\n}\n\nfn load(ready: bool): Result<BuildInfo, string> {\nif ready {\nreturn Ok(BuildInfo { label: \"built\" })\n}\nreturn Err(\"boom\")\n}\n\nfn describe(value: Option<BuildInfo>): string {\nmatch value {\nSome(info) {\nreturn info.label\n}\nNone {\nreturn \"none\"\n}\n}\n}\n\nfn render(result: Result<BuildInfo, string>): string {\nmatch result {\nOk(info) {\nreturn info.label\n}\nErr(message) {\nreturn message\n}\n}\n}\n\nprint describe(maybe(true))\nprint render(load(false))\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("fn maybe(ready: bool) -> Option<BuildInfo> {"));
        assert!(
            rendered.contains("return Option::Some(BuildInfo { label: String::from(\"ok\") });")
        );
        assert!(rendered.contains("return Option::None;"));
        assert!(rendered.contains("fn load(ready: bool) -> Result<BuildInfo, String> {"));
        assert!(
            rendered.contains("return Result::Ok(BuildInfo { label: String::from(\"built\") });")
        );
        assert!(rendered.contains("return Result::Err(String::from(\"boom\"));"));
        assert!(rendered.contains("Option::Some(info) => {"));
        assert!(rendered.contains("Option::None => {"));
        assert!(rendered.contains("Result::Ok(info) => {"));
        assert!(rendered.contains("Result::Err(message) => {"));
    }

    #[test]
    fn parser_lowers_try_operator() {
        let source = "fn maybe_label(ready: bool): Option<string> {\nif ready {\nreturn Some(\"ready\")\n}\nreturn None\n}\n\nfn load_count(ready: bool): Result<int, string> {\nif ready {\nreturn Ok(7)\n}\nreturn Err(\"boom\")\n}\n\nfn require_label(ready: bool): Option<string> {\nlet label: string = maybe_label(ready)?\nreturn Some(label)\n}\n\nfn next_count(ready: bool): Result<int, string> {\nlet count: int = load_count(ready)?\nreturn Ok(count + 1)\n}\n\nprint \"ready\"\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("(maybe_label(ready))?"));
        assert!(rendered.contains("(load_count(ready))?"));
    }

    #[test]
    fn build_project_runs_try_operator() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("try-operator");
        create_project(&project, Some("try-operator-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn maybe_label(ready: bool): Option<string> {\nif ready {\nreturn Some(\"ready\")\n}\nreturn None\n}\n\nfn load_count(ready: bool): Result<int, string> {\nif ready {\nreturn Ok(7)\n}\nreturn Err(\"boom\")\n}\n\nfn require_label(ready: bool): Option<string> {\nlet label: string = maybe_label(ready)?\nreturn Some(label)\n}\n\nfn next_count(ready: bool): Result<int, string> {\nlet count: int = load_count(ready)?\nreturn Ok(count + 1)\n}\n\nfn render_option(value: Option<string>): string {\nmatch value {\nSome(label) {\nreturn label\n}\nNone {\nreturn \"none\"\n}\n}\n}\n\nfn render_result(value: Result<int, string>): string {\nmatch value {\nOk(count) {\nreturn \"ok\"\n}\nErr(message) {\nreturn message\n}\n}\n}\n\nprint render_option(require_label(true))\nprint render_option(require_label(false))\nprint render_result(next_count(true))\nprint render_result(next_count(false))\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "ready\nnone\nok\nboom\n"
        );
    }

    #[test]
    fn parser_lowers_enums_and_match() {
        let source = "enum Status {\nReady\nFailed\n}\n\nfn label(status: Status): string {\nmatch status {\nReady {\nreturn \"ready\"\n}\nFailed {\nreturn \"failed\"\n}\n}\n}\n\nlet status: Status = Ready\nprint label(status)\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        assert_eq!(parsed.enums.len(), 1);
        let hir = hir::lower(&parsed).expect("lower");
        assert_eq!(hir.enums.len(), 1);
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("enum Status {"));
        assert!(rendered.contains("Ready,"));
        assert!(rendered.contains("Failed,"));
        assert!(rendered.contains("match status {"));
        assert!(rendered.contains("Status::Ready => {"));
        assert!(rendered.contains("Status::Failed => {"));
        assert!(rendered.contains("let status: Status = Status::Ready;"));
    }

    #[test]
    fn parser_lowers_payload_enums_and_match_bindings() {
        let source = "enum Message {\nText(string)\nCount(int)\n}\n\nfn render(message: Message): string {\nmatch message {\nText(text) {\nreturn text\n}\nCount(count) {\nreturn \"count\"\n}\n}\n}\n\nlet message: Message = Text(\"ready\")\nprint render(message)\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        assert_eq!(
            parsed.enums[0].variants[0].payload_tys,
            vec![crate::syntax::TypeName::String]
        );
        let crate::syntax::Stmt::Match { arms, .. } = &parsed.functions[0].body[0] else {
            panic!("expected match statement");
        };
        assert_eq!(arms[0].variant, "Text");
        assert_eq!(arms[0].bindings, vec![String::from("text")]);
        assert_eq!(arms[1].variant, "Count");
        assert_eq!(arms[1].bindings, vec![String::from("count")]);
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("Text(String),"));
        assert!(rendered.contains("Count(i64),"));
        assert!(rendered.contains("Message::Text(text) => {"));
        assert!(
            rendered.contains("let message: Message = Message::Text(String::from(\"ready\"));")
        );
    }

    #[test]
    fn parser_lowers_multi_payload_enums_and_match_bindings() {
        let source = "enum Message {\nPair(int, string)\nText(string)\n}\n\nfn render(message: Message): string {\nmatch message {\nPair(count, label) {\nprint count\nreturn label\n}\nText(text) {\nreturn text\n}\n}\n}\n\nlet message: Message = Pair(7, \"tuple payload\")\nprint render(message)\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        assert_eq!(
            parsed.enums[0].variants[0].payload_tys,
            vec![
                crate::syntax::TypeName::Int,
                crate::syntax::TypeName::String
            ]
        );
        let crate::syntax::Stmt::Match { arms, .. } = &parsed.functions[0].body[0] else {
            panic!("expected match statement");
        };
        assert_eq!(
            arms[0].bindings,
            vec![String::from("count"), String::from("label")]
        );
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("Pair(i64, String),"));
        assert!(rendered.contains("Message::Pair(count, label) => {"));
        assert!(
            rendered.contains(
                "let message: Message = Message::Pair(7, String::from(\"tuple payload\"));"
            )
        );
    }

    #[test]
    fn parser_lowers_named_payload_enums_and_match_bindings() {
        let source = "enum Message {\nJob { id: int, label: string }\nText(string)\n}\n\nfn render(message: Message): string {\nmatch message {\nJob { id, label } {\nprint id\nreturn label\n}\nText(text) {\nreturn text\n}\n}\n}\n\nlet message: Message = Job { id: 7, label: \"named payload\" }\nprint render(message)\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        assert_eq!(
            parsed.enums[0].variants[0].payload_names,
            vec![String::from("id"), String::from("label")]
        );
        let crate::syntax::Stmt::Match { arms, .. } = &parsed.functions[0].body[0] else {
            panic!("expected match statement");
        };
        assert!(arms[0].is_named);
        assert_eq!(
            arms[0].bindings,
            vec![String::from("id"), String::from("label")]
        );
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("Job {"));
        assert!(rendered.contains("id: i64,"));
        assert!(rendered.contains("label: String,"));
        assert!(rendered.contains("Message::Job { id, label } => {"));
        assert!(rendered.contains(
            "let message: Message = Message::Job { id: 7, label: String::from(\"named payload\") };"
        ));
    }

    #[test]
    fn build_project_emits_native_binary() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("native");
        create_project(&project, Some("native-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn banner(name: string): string {\nreturn \"hello \" + name\n}\n\nfn lucky(base: int): int {\nreturn base + 2\n}\n\nfn is_ready(value: int): bool {\nreturn value == 42\n}\n\nlet answer: int = lucky(40)\nlet ready: bool = is_ready(answer)\nwhile false {\nprint \"never\"\n}\nif ready {\nprint banner(\"from stage1\")\n} else {\nprint \"broken\"\n}\nprint answer\nprint ready\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        assert!(Path::new(&built.binary).exists());
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "hello from stage1\n42\ntrue\n"
        );
    }

    #[test]
    fn build_project_emits_native_binary_with_structs() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("structs");
        create_project(&project, Some("structs-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct BuildInfo {\nlabel: string\ncount: int\n}\n\nlet info: BuildInfo = BuildInfo { label: \"hello from stage1\", count: 42 }\nprint info.count\nprint info.label\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "42\nhello from stage1\n"
        );
    }

    #[test]
    fn build_project_emits_native_binary_with_arrays() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("arrays");
        create_project(&project, Some("arrays-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn answer(values: [int]): int {\nreturn values[1]\n}\n\nlet values: [int] = [40, 42]\nprint answer(values)\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");
    }

    #[test]
    fn build_project_emits_native_binary_with_array_slices() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("slices");
        create_project(&project, Some("slices-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn tail(values: &[int]): &[int] {\nreturn values[1:]\n}\n\nfn string_tail_len(values: &[string]): int {\nlet rest: &[string] = values[1:]\nreturn len(rest)\n}\n\nlet values: [int] = [3, 7, 9, 11]\nlet window: &[int] = tail(values[:])\nprint first(window)\nprint last(window)\nprint len(window)\nlet labels: [string] = [\"build\", \"test\", \"ship\"]\nprint string_tail_len(labels[:])\nlet words: [string] = [\"alpha\", \"beta\"]\nprint first(words)\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "7\n11\n3\n2\nalpha\n"
        );
    }

    #[test]
    fn build_project_emits_native_binary_with_wrapped_borrow_returns() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("wrapped-borrow-returns");
        create_project(&project, Some("wrapped-borrow-returns-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn maybe_tail(values: &[int], ready: bool): Option<&[int]> {\nif ready {\nreturn Some(values[1:])\n}\nreturn None\n}\n\nfn describe(values: &[int]): (Option<&[int]>, int) {\nreturn (Some(values[1:]), len(values))\n}\n\nlet numbers: [int] = [3, 7, 9, 11]\nmatch maybe_tail(numbers[:], true) {\nSome(window) {\nprint first(window)\n}\nNone {\nprint 0\n}\n}\nlet summary: (Option<&[int]>, int) = describe(numbers[:])\nmatch summary.0 {\nSome(window) {\nprint last(window)\n}\nNone {\nprint 0\n}\n}\nprint summary.1\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n11\n4\n");
    }

    #[test]
    fn build_project_emits_native_binary_with_match_payload_borrow_returns() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("match-payload-borrow-returns");
        create_project(&project, Some("match-payload-borrow-returns-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn choose(values: &[int]): Option<&[int]> {\nmatch Some(values[1:]) {\nSome(window) {\nreturn Some(window)\n}\nNone {\nreturn None\n}\n}\n}\n\nlet numbers: [int] = [3, 7, 9, 11]\nmatch choose(numbers[:]) {\nSome(window) {\nprint first(window)\n}\nNone {\nprint 0\n}\n}\nprint first(numbers)\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n3\n");
    }

    #[test]
    fn build_project_emits_native_binary_after_match_temporary_borrow_ends() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("match-temporary-borrow-release");
        create_project(&project, Some("match-temporary-borrow-release-app"))
            .expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [string] = [\"alpha\", \"beta\"]\nmatch Some(values[:]) {\nSome(window) {\nprint len(window)\n}\nNone {\nprint 0\n}\n}\nprint first(values)\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "2\nalpha\n");
    }

    #[test]
    fn build_project_emits_native_binary_after_if_false_dead_branch_is_ignored() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("if-false-dead-branch");
        create_project(&project, Some("if-false-dead-branch-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [string] = [\"alpha\", \"beta\"]\nif false {\nlet view: &[string] = values[:]\nprint len(view)\nprint first(values)\n} else {\nprint 0\n}\nprint first(values)\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "0\nalpha\n");
    }

    #[test]
    fn build_project_emits_native_binary_after_while_false_dead_body_is_ignored() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("while-false-dead-body");
        create_project(&project, Some("while-false-dead-body-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [string] = [\"alpha\", \"beta\"]\nwhile false {\nlet view: &[string] = values[:]\nprint len(view)\nprint first(values)\n}\nprint first(values)\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "alpha\n");
    }

    #[test]
    fn build_project_emits_native_binary_with_multi_param_borrow_returns() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("multi-param-borrow-returns");
        create_project(&project, Some("multi-param-borrow-returns-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn choose(left: &[int], right: &[int], pick_left: bool): Option<&[int]> {\nif pick_left {\nreturn Some(left[1:])\n}\nreturn Some(right[1:])\n}\n\nlet left: [int] = [3, 7, 9]\nlet right: [int] = [40, 42, 44]\nmatch choose(left[:], right[:], false) {\nSome(window) {\nprint first(window)\n}\nNone {\nprint 0\n}\n}\nprint first(left)\nprint first(right)\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n3\n40\n");
    }

    #[test]
    fn build_project_emits_native_binary_with_borrowed_named_shapes() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("borrowed-named-shapes");
        create_project(&project, Some("borrowed-named-shapes-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct Window {\nview: &[int]\n}\n\nenum Snapshot {\nWindow(Window)\nNamed { window: Window }\n}\n\nfn tail(values: &[int]): Window {\nreturn Window { view: values[1:] }\n}\n\nfn read(snapshot: Snapshot): int {\nmatch snapshot {\nWindow(window) {\nreturn first(window.view)\n}\nNamed { window } {\nreturn last(window.view)\n}\n}\n}\n\nlet numbers: [int] = [3, 7, 9, 11]\nlet window: Window = tail(numbers[:])\nprint first(window.view)\nprint read(Window(tail(numbers[:])))\nprint read(Named { window: tail(numbers[:]) })\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n7\n11\n");
    }

    #[test]
    fn build_project_emits_native_binary_after_branch_local_slice_borrow_ends() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("borrow-scope");
        create_project(&project, Some("borrow-scope-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [string] = [\"alpha\", \"beta\"]\nif true {\nlet view: &[string] = values[:]\nprint len(view)\n}\nprint first(values)\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "2\nalpha\n");
    }

    #[test]
    fn build_project_emits_native_binary_after_wrapped_borrow_scope_ends() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("wrapped-borrow-scope");
        create_project(&project, Some("wrapped-borrow-scope-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [string] = [\"alpha\", \"beta\"]\nif true {\nlet wrapped: (&[string], int) = (values[:], 1)\nprint len(wrapped.0)\n}\nprint first(values)\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "2\nalpha\n");
    }

    #[test]
    fn build_project_emits_native_binary_with_tuples() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("tuples");
        create_project(&project, Some("tuples-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn label(pair: (int, string)): string {\nreturn pair.1\n}\n\nlet pair: (int, string) = (7, \"stage1 tuples\")\nprint pair.0\nprint label(pair)\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "7\nstage1 tuples\n"
        );
    }

    #[test]
    fn build_project_emits_native_binary_with_maps() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("maps");
        create_project(&project, Some("maps-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let scores: {string: int} = {\"build\": 7, \"deploy\": 9}\nprint scores[\"deploy\"]\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "9\n");
    }

    #[test]
    fn build_project_emits_native_binary_with_option_and_result() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("outcomes");
        create_project(&project, Some("outcomes-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "import \"models.ax\"\n\nfn maybe_job(ready: bool): Option<Job> {\nif ready {\nreturn Some(Job { id: 7, label: \"queued\" })\n}\nreturn None\n}\n\nfn load_job(ready: bool): Result<Job, string> {\nif ready {\nreturn Ok(Job { id: 9, label: \"built\" })\n}\nreturn Err(\"boom\")\n}\n\nfn describe(job: Option<Job>): string {\nmatch job {\nSome(info) {\nreturn info.label\n}\nNone {\nreturn \"none\"\n}\n}\n}\n\nfn render(result: Result<Job, string>): string {\nmatch result {\nOk(info) {\nreturn info.label\n}\nErr(message) {\nreturn message\n}\n}\n}\n\nprint describe(maybe_job(true))\nprint describe(maybe_job(false))\nprint render(load_job(true))\nprint render(load_job(false))\n",
        )
        .expect("write main");
        fs::write(
            project.join("src/models.ax"),
            "pub struct Job {\nid: int\nlabel: string\n}\n",
        )
        .expect("write models");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "queued\nnone\nbuilt\nboom\n"
        );
    }

    #[test]
    fn build_project_emits_native_binary_with_enums() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("enums");
        create_project(&project, Some("enums-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum Status {\nReady\nFailed\n}\n\nfn label(status: Status): string {\nmatch status {\nReady {\nreturn \"ready\"\n}\nFailed {\nreturn \"failed\"\n}\n}\n}\n\nlet status: Status = Ready\nprint label(status)\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "ready\n");
    }

    #[test]
    fn build_project_emits_native_binary_with_enum_field_match() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("enum-field-match");
        create_project(&project, Some("enum-field-match-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum JobState {\nQueued\nRunning\nDone\n}\n\nstruct Job {\nid: int\nstate: JobState\n}\n\nfn label(job: Job): string {\nmatch job.state {\nQueued {\nreturn \"queued\"\n}\nRunning {\nreturn \"running\"\n}\nDone {\nreturn \"done\"\n}\n}\n}\n\nlet job: Job = Job { id: 7, state: Running }\nprint job.id\nprint label(job)\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "7\nrunning\n");
    }

    #[test]
    fn build_project_emits_native_binary_with_payload_enums() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("payload-enums");
        create_project(&project, Some("payload-enums-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum Message {\nText(string)\nCount(int)\n}\n\nfn render(message: Message): string {\nmatch message {\nText(text) {\nreturn text\n}\nCount(count) {\nprint count\nreturn \"count\"\n}\n}\n}\n\nlet message: Message = Text(\"ready\")\nprint render(message)\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "ready\n");
    }

    #[test]
    fn build_project_emits_native_binary_with_multi_payload_enums() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("multi-payload-enums");
        create_project(&project, Some("multi-payload-enums-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum Message {\nPair(int, string)\nText(string)\n}\n\nfn render(message: Message): string {\nmatch message {\nPair(count, label) {\nprint count\nreturn label\n}\nText(text) {\nreturn text\n}\n}\n}\n\nlet first: Message = Pair(7, \"multi payload\")\nprint render(first)\nlet second: Message = Text(\"payload enums\")\nprint render(second)\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "7\nmulti payload\npayload enums\n"
        );
    }

    #[test]
    fn build_project_emits_native_binary_with_named_payload_enums() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("named-payload-enums");
        create_project(&project, Some("named-payload-enums-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum Message {\nJob { id: int, label: string }\nText(string)\n}\n\nfn render(message: Message): string {\nmatch message {\nJob { id, label } {\nprint id\nreturn label\n}\nText(text) {\nreturn text\n}\n}\n}\n\nlet first: Message = Job { id: 7, label: \"named payload\" }\nprint render(first)\nlet second: Message = Text(\"payload enums\")\nprint render(second)\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "7\nnamed payload\npayload enums\n"
        );
    }

    #[test]
    fn stage1_project_supports_local_path_dependencies() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("deps-app");
        let dependency = project.join("deps/core");
        create_project(&project, Some("deps-app")).expect("create project");
        create_project(&dependency, Some("core-lib")).expect("create dependency");
        fs::write(
            dependency.join("src/math.ax"),
            "pub fn answer(): int {\nreturn 42\n}\n",
        )
        .expect("write dependency source");
        let dependency_manifest = load_manifest(&dependency).expect("load dependency manifest");
        fs::write(
            dependency.join("axiom.lock"),
            render_lockfile_for_project(&dependency, &dependency_manifest)
                .expect("dependency lockfile"),
        )
        .expect("write dependency lockfile");
        fs::write(
            project.join("axiom.toml"),
            format!(
                "{}\n[dependencies]\ncore = {{ path = \"deps/core\" }}\n",
                render_manifest("deps-app")
            ),
        )
        .expect("write manifest");
        fs::write(
            project.join("src/main.ax"),
            "import \"core/math.ax\"\nprint answer()\n",
        )
        .expect("write root source");
        fs::write(
            project.join("src/main_test.ax"),
            "import \"core/math.ax\"\nprint answer()\n",
        )
        .expect("write root test");
        fs::write(project.join("src/main_test.stdout"), "42\n").expect("write expected stdout");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");

        check_project(&project).expect("check project");
        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");

        let tests = run_project_tests(&project).expect("run tests");
        assert_eq!(tests.passed, 1);
        assert_eq!(tests.failed, 0);
    }

    #[test]
    fn package_visibility_allows_same_package_module_imports() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("package-visible-module");
        create_project(&project, Some("package-visible-module-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "import \"shared.ax\"\n\nlet answer: Id = helper()\nlet info: BuildInfo = build()\nlet status: Status = ready()\nprint answer\nprint info.label\nmatch status {\nReady {\nprint ANSWER\n}\n}\n",
        )
        .expect("write main");
        fs::write(
            project.join("src/shared.ax"),
            "pub(pkg) const ANSWER: int = 42\npub(pkg) type Id = int\npub(pkg) struct BuildInfo {\nlabel: string\n}\npub(pkg) enum Status {\nReady\n}\npub(pkg) fn helper(): Id {\nreturn ANSWER\n}\npub(pkg) fn build(): BuildInfo {\nreturn BuildInfo { label: \"package\" }\n}\npub(pkg) fn ready(): Status {\nreturn Ready\n}\n",
        )
        .expect("write shared");
        let built = build_project(&project).expect("build package-visible module");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "42\npackage\n42\n");
    }

    #[test]
    fn package_visibility_allows_same_package_async_module_imports() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("package-visible-async-module");
        create_project(&project, Some("package-visible-async-module-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/async.ax\"\nimport \"shared.ax\"\n\nlet task: Task<int> = helper(41)\nprint await task\n",
        )
        .expect("write main");
        fs::write(
            project.join("src/shared.ax"),
            "pub(pkg) async fn helper(value: int): int {\nreturn value + 1\n}\n",
        )
        .expect("write shared");
        let built = build_project(&project).expect("build package-visible async module");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");
    }

    #[test]
    fn package_visibility_rejects_cross_package_imports() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("package-visible-dependency");
        let dependency = project.join("deps/core");
        create_project(&project, Some("package-visible-dependency-app")).expect("create root");
        create_project(&dependency, Some("package-visible-core")).expect("create dependency");

        fs::write(
            dependency.join("src/shared.ax"),
            "pub(pkg) fn helper(): int {\nreturn 42\n}\n",
        )
        .expect("write dependency source");
        let dependency_manifest = load_manifest(&dependency).expect("load dependency manifest");
        fs::write(
            dependency.join("axiom.lock"),
            render_lockfile_for_project(&dependency, &dependency_manifest)
                .expect("dependency lockfile"),
        )
        .expect("write dependency lockfile");

        fs::write(
            project.join("axiom.toml"),
            format!(
                "{}\n[dependencies]\ncore = {{ path = \"deps/core\" }}\n",
                render_manifest("package-visible-dependency-app")
            ),
        )
        .expect("write root manifest");
        fs::write(
            project.join("src/main.ax"),
            "import \"core/shared.ax\"\nprint helper()\n",
        )
        .expect("write root source");
        let manifest = load_manifest(&project).expect("load root manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("root lockfile"),
        )
        .expect("write root lockfile");

        let error =
            check_project(&project).expect_err("package-visible dependency import should fail");
        assert_eq!(error.kind, "import");
        assert!(error.message.contains("is not visible from this module"));
    }

    #[test]
    fn package_visibility_rejects_cross_package_async_function_imports() {
        assert_cross_package_package_visibility_error(
            "package-visible-dependency-async-fn",
            "pub(pkg) async fn helper(): int {\nreturn 42\n}\n",
            "import \"std/async.ax\"\nimport \"core/shared.ax\"\nlet task: Task<int> = helper()\nprint await task\n",
            "function \"helper\"",
        );
    }

    fn assert_cross_package_package_visibility_error(
        case_name: &str,
        dependency_source: &str,
        main_source: &str,
        expected_message: &str,
    ) {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join(case_name);
        let dependency = project.join("deps/core");
        create_project(&project, Some(case_name)).expect("create root");
        create_project(&dependency, Some("package-visible-core")).expect("create dependency");

        fs::write(dependency.join("src/shared.ax"), dependency_source).expect("write dependency");
        let dependency_manifest = load_manifest(&dependency).expect("load dependency manifest");
        fs::write(
            dependency.join("axiom.lock"),
            render_lockfile_for_project(&dependency, &dependency_manifest)
                .expect("dependency lockfile"),
        )
        .expect("write dependency lockfile");

        fs::write(
            project.join("axiom.toml"),
            format!(
                "{}\n[dependencies]\ncore = {{ path = \"deps/core\" }}\n",
                render_manifest(case_name)
            ),
        )
        .expect("write root manifest");
        fs::write(project.join("src/main.ax"), main_source).expect("write root source");
        let manifest = load_manifest(&project).expect("load root manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("root lockfile"),
        )
        .expect("write root lockfile");

        let error =
            check_project(&project).expect_err("package-visible dependency import should fail");
        assert_eq!(error.kind, "import");
        assert!(error.message.contains(expected_message));
        assert!(error.message.contains("is not visible from this module"));
    }

    #[test]
    fn package_visibility_rejects_cross_package_const_imports() {
        assert_cross_package_package_visibility_error(
            "package-visible-dependency-const",
            "pub(pkg) const ANSWER: int = 42\n",
            "import \"core/shared.ax\"\nprint ANSWER\n",
            "const \"ANSWER\"",
        );
    }

    #[test]
    fn package_visibility_rejects_cross_package_type_alias_imports() {
        assert_cross_package_package_visibility_error(
            "package-visible-dependency-type",
            "pub(pkg) type Id = int\n",
            "import \"core/shared.ax\"\nlet answer: Id = 42\nprint answer\n",
            "type \"Id\"",
        );
    }

    #[test]
    fn package_visibility_rejects_cross_package_struct_imports() {
        assert_cross_package_package_visibility_error(
            "package-visible-dependency-struct",
            "pub(pkg) struct BuildInfo {\nlabel: string\n}\n",
            "import \"core/shared.ax\"\nlet info: BuildInfo = BuildInfo { label: \"x\" }\nprint info.label\n",
            "type \"BuildInfo\"",
        );
    }

    #[test]
    fn package_visibility_rejects_cross_package_enum_imports() {
        assert_cross_package_package_visibility_error(
            "package-visible-dependency-enum",
            "pub(pkg) enum Status {\nReady\n}\n",
            "import \"core/shared.ax\"\nlet status: Status = Ready\nmatch status {\nReady {\nprint \"ready\"\n}\n}\n",
            "type \"Status\"",
        );
    }

    #[test]
    fn stage1_project_supports_workspace_members() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("workspace-root");
        let core = project.join("members/core");
        let util = project.join("members/util");
        create_project(&project, Some("workspace-root-app")).expect("create root project");
        create_project(&core, Some("workspace-core")).expect("create core member");
        create_project(&util, Some("workspace-util")).expect("create util member");

        fs::write(
            core.join("src/math.ax"),
            "pub fn answer(): int {\nreturn 42\n}\n",
        )
        .expect("write core source");
        fs::write(
            util.join("src/extra.ax"),
            "pub fn helper(): int {\nreturn 7\n}\n",
        )
        .expect("write util source");

        let core_manifest = load_manifest(&core).expect("load core manifest");
        fs::write(
            core.join("axiom.lock"),
            render_lockfile_for_project(&core, &core_manifest).expect("core lockfile"),
        )
        .expect("write core lockfile");
        let util_manifest = load_manifest(&util).expect("load util manifest");
        fs::write(
            util.join("axiom.lock"),
            render_lockfile_for_project(&util, &util_manifest).expect("util lockfile"),
        )
        .expect("write util lockfile");

        fs::write(
            project.join("axiom.toml"),
            format!(
                "{}\n[workspace]\nmembers = [\"members/core\", \"members/util\"]\n\n[dependencies]\ncore = {{ path = \"members/core\" }}\n",
                render_manifest("workspace-root-app")
            ),
        )
        .expect("write workspace manifest");
        fs::write(
            project.join("src/main.ax"),
            "import \"core/math.ax\"\nprint answer()\n",
        )
        .expect("write root source");
        fs::write(
            project.join("src/main_test.ax"),
            "import \"core/math.ax\"\nprint answer()\n",
        )
        .expect("write root test");
        fs::write(project.join("src/main_test.stdout"), "42\n").expect("write golden");

        let manifest = load_manifest(&project).expect("load root manifest");
        let lockfile = render_lockfile_for_project(&project, &manifest).expect("root lockfile");
        assert!(lockfile.contains("path:members/core"));
        assert!(lockfile.contains("path:members/util"));
        fs::write(project.join("axiom.lock"), lockfile).expect("write root lockfile");

        let checked = check_project(&project).expect("check workspace root");
        assert_eq!(checked.packages.len(), 3);
        let built = build_project(&project).expect("build workspace root");
        assert_eq!(built.packages.len(), 3);
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");

        let tests = run_project_tests(&project).expect("run workspace tests");
        assert_eq!(tests.packages.len(), 3);
        assert_eq!(tests.passed, 3);
        assert_eq!(tests.failed, 0);
    }

    #[test]
    fn workspace_only_manifest_supports_package_selection() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("workspace-only-root");
        let app = project.join("members/app");
        let core = project.join("members/core");
        fs::create_dir_all(project.join("members")).expect("create workspace members dir");
        create_project(&app, Some("workspace-app")).expect("create app member");
        create_project(&core, Some("workspace-core")).expect("create core member");

        fs::write(
            core.join("src/math.ax"),
            "pub fn answer(): int {\nreturn 42\n}\n",
        )
        .expect("write core source");
        let core_manifest = load_manifest(&core).expect("load core manifest");
        fs::write(
            core.join("axiom.lock"),
            render_lockfile_for_project(&core, &core_manifest).expect("core lockfile"),
        )
        .expect("write core lockfile");

        fs::write(
            app.join("axiom.toml"),
            format!(
                "{}\n[dependencies]\ncore = {{ path = \"../core\" }}\n",
                render_manifest("workspace-app")
            ),
        )
        .expect("write app manifest");
        fs::write(
            app.join("src/main.ax"),
            "import \"core/math.ax\"\nprint answer()\n",
        )
        .expect("write app source");
        fs::write(
            app.join("src/main_test.ax"),
            "import \"core/math.ax\"\nprint answer()\n",
        )
        .expect("write app test");
        fs::write(app.join("src/main_test.stdout"), "42\n").expect("write app golden");
        let app_manifest = load_manifest(&app).expect("load app manifest");
        fs::write(
            app.join("axiom.lock"),
            render_lockfile_for_project(&app, &app_manifest).expect("app lockfile"),
        )
        .expect("write app lockfile");

        fs::write(
            project.join("axiom.toml"),
            "[workspace]\nmembers = [\"members/app\", \"members/core\"]\n",
        )
        .expect("write workspace-only manifest");
        let root_manifest = load_manifest(&project).expect("load root manifest");
        assert!(root_manifest.is_workspace_only());
        let root_lockfile =
            render_lockfile_for_project(&project, &root_manifest).expect("root lockfile");
        assert!(root_lockfile.contains("path:members/app"));
        assert!(root_lockfile.contains("path:members/core"));
        fs::write(project.join("axiom.lock"), root_lockfile).expect("write root lockfile");

        let checked = check_project(&project).expect("check workspace-only root");
        assert_eq!(checked.packages.len(), 2);

        let selected = check_project_with_options(
            &project,
            &CheckOptions {
                package: Some(String::from("workspace-app")),
            },
        )
        .expect("check selected workspace package");
        assert_eq!(selected.packages.len(), 1);
        assert!(selected.manifest.ends_with("members/app/axiom.toml"));

        let built = build_project_with_options(
            &project,
            &BuildOptions {
                backend: NativeBackendKind::GeneratedRust,
                target: None,
                package: Some(String::from("workspace-app")),
                debug: false,
                ..BuildOptions::default()
            },
        )
        .expect("build selected workspace package");
        assert_eq!(built.packages.len(), 1);
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run selected workspace binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");

        let tests = run_project_tests_with_options(
            &project,
            &TestOptions {
                filter: None,
                package: Some(String::from("workspace-app")),
                include_benchmarks: false,
            },
        )
        .expect("test selected workspace package");
        assert_eq!(tests.passed, 1);
        assert_eq!(tests.failed, 0);

        let exit = run_project_with_options(
            &project,
            &RunOptions {
                package: Some(String::from("workspace-app")),
            },
        )
        .expect("run selected workspace package");
        assert_eq!(exit, 0);
    }

    #[test]
    fn workspace_only_manifest_requires_package_for_run() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("workspace-only-run");
        let app = project.join("members/app");
        fs::create_dir_all(project.join("members")).expect("create workspace members dir");
        create_project(&app, Some("workspace-runner")).expect("create app member");

        fs::write(
            project.join("axiom.toml"),
            "[workspace]\nmembers = [\"members/app\"]\n",
        )
        .expect("write workspace-only manifest");
        let root_manifest = load_manifest(&project).expect("load root manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &root_manifest).expect("root lockfile"),
        )
        .expect("write root lockfile");

        let error = run_project_with_options(&project, &RunOptions::default())
            .expect_err("workspace-only run should require package selection");
        assert_eq!(error.kind, "run");
        assert!(error.message.contains("require -p/--package"));
    }

    #[test]
    fn build_locked_offline_accepts_local_path_graph() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("offline-root");
        let dependency = dir.path().join("offline-dep");
        create_project(&project, Some("offline-root")).expect("create root project");
        create_project(&dependency, Some("offline-dep")).expect("create dependency project");

        fs::write(
            project.join("axiom.toml"),
            r#"[package]
name = "offline-root"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[dependencies]
dep = { path = "../offline-dep" }

[capabilities]
fs = false
"fs:write" = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
        )
        .expect("write root manifest");
        let manifest = load_manifest(&project).expect("load root manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("root lockfile"),
        )
        .expect("write root lockfile");

        let output = build_project_with_options(
            &project,
            &BuildOptions {
                locked: true,
                offline: true,
                ..BuildOptions::default()
            },
        )
        .expect("locked offline build should accept local path graph");
        assert!(output.locked);
        assert!(output.offline);
        assert!(Path::new(&output.binary).exists());
    }

    #[test]
    fn build_locked_offline_missing_lockfile_fails_without_outputs() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("missing-lock");
        create_project(&project, Some("missing-lock")).expect("create project");
        fs::remove_file(project.join("axiom.lock")).expect("remove lockfile");

        let error = build_project_with_options(
            &project,
            &BuildOptions {
                locked: true,
                offline: true,
                ..BuildOptions::default()
            },
        )
        .expect_err("missing lockfile should fail locked offline build");
        assert_eq!(error.kind, "lockfile");
        assert!(!project.join("axiom.lock").exists());
        assert!(!project.join("dist").exists());
    }

    #[test]
    fn build_locked_offline_stale_lockfile_fails_without_outputs() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stale-lock");
        create_project(&project, Some("stale-lock")).expect("create project");
        let lockfile_path = project.join("axiom.lock");
        let original_lockfile = fs::read_to_string(&lockfile_path).expect("read lockfile");
        let manifest = fs::read_to_string(project.join("axiom.toml")).expect("read manifest");
        fs::write(
            project.join("axiom.toml"),
            manifest.replace("version = \"0.1.0\"", "version = \"0.2.0\""),
        )
        .expect("stale manifest");

        let error = build_project_with_options(
            &project,
            &BuildOptions {
                locked: true,
                offline: true,
                ..BuildOptions::default()
            },
        )
        .expect_err("stale lockfile should fail locked offline build");
        assert_eq!(error.kind, "lockfile");
        assert_eq!(
            fs::read_to_string(&lockfile_path).expect("read unchanged lockfile"),
            original_lockfile
        );
        assert!(!project.join("dist").exists());
    }

    #[test]
    fn workspace_members_must_appear_in_root_lockfile() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("workspace-lock");
        let core = project.join("members/core");
        let util = project.join("members/util");
        create_project(&project, Some("workspace-lock-app")).expect("create root project");
        create_project(&core, Some("workspace-lock-core")).expect("create core member");
        create_project(&util, Some("workspace-lock-util")).expect("create util member");

        fs::write(
            project.join("axiom.toml"),
            format!(
                "{}\n[workspace]\nmembers = [\"members/core\", \"members/util\"]\n\n[dependencies]\ncore = {{ path = \"members/core\" }}\n",
                render_manifest("workspace-lock-app")
            ),
        )
        .expect("write workspace manifest");
        fs::write(
            project.join("src/main.ax"),
            "import \"core/main.ax\"\nprint \"done\"\n",
        )
        .expect("write root source");

        let manifest = load_manifest(&project).expect("load root manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile(&manifest).expect("minimal lockfile"),
        )
        .expect("write incomplete lockfile");

        let error = check_project(&project).expect_err("workspace members should be locked");
        assert_eq!(error.kind, "lockfile");
        assert!(
            error
                .message
                .contains("axiom.lock does not match axiom.toml")
        );
    }

    #[test]
    fn workspace_members_reject_parent_traversal() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("workspace-invalid");
        create_project(&project, Some("workspace-invalid-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            format!(
                "{}\n[workspace]\nmembers = [\"../outside\"]\n",
                render_manifest("workspace-invalid-app")
            ),
        )
        .expect("write manifest");

        let error = check_project(&project).expect_err("workspace member traversal should fail");
        assert_eq!(error.kind, "manifest");
        assert!(error.message.contains("must not use parent traversal"));
    }

    #[test]
    fn dependency_package_must_enable_its_own_capabilities() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("dep-cap-root");
        let dependency = project.join("deps/core");
        create_project(&project, Some("dep-cap-root-app")).expect("create root project");
        create_project(&dependency, Some("dep-cap-core")).expect("create dependency");

        fs::write(
            dependency.join("src/time.ax"),
            "pub fn tick(): int {\nreturn clock_now_ms()\n}\n",
        )
        .expect("write dependency source");
        let dependency_manifest = load_manifest(&dependency).expect("load dependency manifest");
        fs::write(
            dependency.join("axiom.lock"),
            render_lockfile_for_project(&dependency, &dependency_manifest)
                .expect("dependency lockfile"),
        )
        .expect("write dependency lockfile");

        fs::write(
            project.join("axiom.toml"),
            format!(
                "{}\n[dependencies]\ncore = {{ path = \"deps/core\" }}\n",
                render_manifest_with_capabilities(
                    "dep-cap-root-app",
                    false,
                    false,
                    false,
                    false,
                    true,
                    false,
                )
            ),
        )
        .expect("write root manifest");
        fs::write(
            project.join("src/main.ax"),
            "import \"core/time.ax\"\nprint tick()\n",
        )
        .expect("write root source");
        let manifest = load_manifest(&project).expect("load root manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("root lockfile"),
        )
        .expect("write root lockfile");

        let error = check_project(&project).expect_err("dependency capability should be required");
        assert_eq!(error.kind, "capability");
        assert!(
            error
                .path
                .as_ref()
                .is_some_and(|path| path.ends_with("deps/core/src/time.ax"))
        );
        assert!(
            error
                .message
                .contains("requires [capabilities].clock = true")
        );
    }

    #[test]
    fn capability_view_reflects_manifest_flags() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("caps");
        create_project(&project, Some("caps-app")).expect("create project");
        let manifest = load_manifest(&project).expect("load manifest");
        let caps = capability_descriptors(&manifest.capabilities);
        assert_eq!(caps.len(), 8);
        assert!(caps.iter().all(|cap| !cap.enabled));
        let project_caps = project_capabilities(&project).expect("project capabilities");
        assert_eq!(project_caps.len(), 8);
    }

    #[test]
    fn capability_view_includes_env_allowlist_scope() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("caps-env");
        create_project(&project, Some("caps-env-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            "[package]\nname = \"caps-env-app\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nenv = [\"FOO\", \"LOG_LEVEL\"]\n",
        )
        .expect("write manifest");

        let caps = project_capabilities(&project).expect("project capabilities");
        let env = caps
            .iter()
            .find(|cap| cap.name == "env")
            .expect("env capability");
        assert!(env.enabled);
        assert_eq!(env.allowed, vec!["FOO", "LOG_LEVEL"]);
        assert!(!env.unsafe_unrestricted);

        let payload = json_contract::caps_success(&project, &caps);
        assert_eq!(payload["capabilities"][4]["name"], "env");
        assert_eq!(payload["capabilities"][4]["allowed"][0], "FOO");
        assert_eq!(payload["capabilities"][4]["allowed"][1], "LOG_LEVEL");
        assert!(payload["capabilities"][4]["unsafe_unrestricted"].is_null());
    }

    #[test]
    fn check_project_rejects_extern_function_without_ffi_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("ffi-denied");
        create_project(&project, Some("ffi-denied-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            r#"extern fn strlen(value: string): int from "c"
print strlen("hello")
"#,
        )
        .expect("write source");

        let error = check_project(&project).expect_err("ffi capability should be required");
        assert_eq!(error.kind, "capability");
        assert!(error.message.contains("requires [capabilities].ffi = true"));
    }

    #[test]
    fn build_project_runs_c_ffi_strlen_with_ffi_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("ffi-strlen");
        create_project(&project, Some("ffi-strlen-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            r#"[package]
name = "ffi-strlen-app"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
ffi = true
"#,
        )
        .expect("write manifest");
        fs::write(
            project.join("src/main.ax"),
            r#"extern fn strlen(value: string): int from "c"
print strlen("hello")
"#,
        )
        .expect("write source");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "5\n");
    }

    #[test]
    fn parse_extern_function_accepts_pointer_types() {
        let source = r#"extern fn poke(input: ptr<int>, output: mutptr<int>): bool from "c"
"#;
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let function = parsed.functions.first().expect("function");
        assert!(function.is_extern);
        assert_eq!(function.extern_library.as_deref(), Some("c"));
        assert!(matches!(
            function.params[0].ty,
            crate::syntax::TypeName::Ptr(_)
        ));
        assert!(matches!(
            function.params[1].ty,
            crate::syntax::TypeName::MutPtr(_)
        ));
    }

    #[test]
    fn check_project_rejects_clock_intrinsic_without_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("clock-denied");
        create_project(&project, Some("clock-denied-app")).expect("create project");
        fs::write(project.join("src/main.ax"), "print clock_now_ms()\n").expect("write source");

        let error = check_project(&project).expect_err("clock capability should be required");
        assert_eq!(error.kind, "capability");
        assert!(
            error
                .message
                .contains("requires [capabilities].clock = true")
        );
    }

    #[test]
    fn check_project_rejects_env_intrinsic_without_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("env-denied");
        create_project(&project, Some("env-denied-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let value: Option<string> = env_get(\"PATH\")\nprint \"never\"\n",
        )
        .expect("write source");

        let error = check_project(&project).expect_err("env capability should be required");
        assert_eq!(error.kind, "capability");
        assert!(
            error
                .message
                .contains("requires [capabilities].env = [\"NAME\"]")
        );
    }

    #[test]
    fn env_allowlist_scopes_generated_env_get() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("env-scoped");
        create_project(&project, Some("env-scoped-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            "[package]\nname = \"env-scoped-app\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nenv = [\"FOO\"]\n",
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            "match env_get(\"FOO\") {\nSome(value) {\nprint value\n}\nNone {\nprint \"missing foo\"\n}\n}\nmatch env_get(\"BAR\") {\nSome(value) {\nprint value\n}\nNone {\nprint \"none bar\"\n}\n}\nmatch env_get(\"AWS_SECRET_ACCESS_KEY\") {\nSome(value) {\nprint value\n}\nNone {\nprint \"none secret\"\n}\n}\n",
        )
        .expect("write source");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .env("FOO", "allowed")
            .env("BAR", "blocked")
            .env("AWS_SECRET_ACCESS_KEY", "blocked-secret")
            .output()
            .expect("run compiled binary");

        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "allowed\nnone bar\nnone secret\n"
        );
    }

    #[test]
    fn legacy_env_bool_still_checks_with_deprecation_warning() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("legacy-env");
        create_project(&project, Some("legacy-env-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "legacy-env-app",
                false,
                false,
                false,
                true,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            "match env_get(\"FOO\") {\nSome(value) {\nprint value\n}\nNone {\nprint \"none\"\n}\n}\n",
        )
        .expect("write source");

        let checked = check_project(&project).expect("check project");
        assert_eq!(checked.warnings.len(), 1);
        assert!(checked.warnings[0].contains("[capabilities].env = true is deprecated"));
        assert!(checked.warnings[0].contains("unrestricted environment access"));

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .env("FOO", "legacy")
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "legacy\n");
    }

    #[test]
    fn check_project_rejects_fs_intrinsic_without_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("fs-denied");
        create_project(&project, Some("fs-denied-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "match fs_read(\"missing.txt\") {\nSome(value) {\nprint value\n}\nNone {\nprint \"none\"\n}\n}\n",
        )
        .expect("write source");

        let error = check_project(&project).expect_err("fs capability should be required");
        assert_eq!(error.kind, "capability");
        assert!(error.message.contains("requires [capabilities].fs = true"));
    }

    #[test]
    fn check_project_rejects_net_intrinsic_without_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("net-denied");
        create_project(&project, Some("net-denied-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "match net_resolve(\"localhost\") {\nSome(address) {\nprint address\n}\nNone {\nprint \"none\"\n}\n}\n",
        )
        .expect("write source");

        let error = check_project(&project).expect_err("net capability should be required");
        assert_eq!(error.kind, "capability");
        assert!(error.message.contains("requires [capabilities].net = true"));
    }

    #[test]
    fn check_project_rejects_process_intrinsic_without_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("process-denied");
        create_project(&project, Some("process-denied-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "print process_status(\"fixture\")\n",
        )
        .expect("write source");

        let error = check_project(&project).expect_err("process capability should be required");
        assert_eq!(error.kind, "capability");
        assert!(
            error
                .message
                .contains("requires [capabilities].process = true")
        );
    }

    #[test]
    fn check_project_rejects_crypto_intrinsic_without_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("crypto-denied");
        create_project(&project, Some("crypto-denied-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "print crypto_sha256(\"abc\")\n",
        )
        .expect("write source");

        let error = check_project(&project).expect_err("crypto capability should be required");
        assert_eq!(error.kind, "capability");
        assert!(
            error
                .message
                .contains("requires [capabilities].crypto = true")
        );
    }

    #[test]
    fn build_project_emits_native_binary_with_capability_intrinsics() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("capability-intrinsics");
        create_project(&project, Some("capability-intrinsics-app")).expect("create project");
        let fixture_path = project.join("fixture.txt");
        fs::write(&fixture_path, "fs ok\n").expect("write fs fixture");
        let process_path = write_process_fixture(&project);
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "capability-intrinsics-app",
                true,
                true,
                true,
                true,
                true,
                true,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            format!(
                "match fs_read({fixture:?}) {{\nSome(value) {{\nprint value\n}}\nNone {{\nprint \"missing\"\n}}\n}}\nmatch net_resolve(\"localhost\") {{\nSome(_address) {{\nprint true\n}}\nNone {{\nprint false\n}}\n}}\nprint process_status({process:?})\nprint crypto_sha256(\"abc\")\nlet now: int = clock_now_ms()\nprint now > 0\nmatch env_get(\"__AXIOM_STAGE1_MISSING__\") {{\nSome(value) {{\nprint value\n}}\nNone {{\nprint \"none\"\n}}\n}}\n",
                fixture = fixture_path.to_string_lossy(),
                process = process_path,
            ),
        )
        .expect("write source");
        fs::write(
            project.join("src/main_test.ax"),
            format!(
                "match fs_read({fixture:?}) {{\nSome(value) {{\nprint value\n}}\nNone {{\nprint \"missing\"\n}}\n}}\nmatch net_resolve(\"localhost\") {{\nSome(_address) {{\nprint true\n}}\nNone {{\nprint false\n}}\n}}\nprint process_status({process:?})\nprint crypto_sha256(\"abc\")\nlet now: int = clock_now_ms()\nprint now > 0\nmatch env_get(\"__AXIOM_STAGE1_MISSING__\") {{\nSome(value) {{\nprint value\n}}\nNone {{\nprint \"none\"\n}}\n}}\n",
                fixture = fixture_path.to_string_lossy(),
                process = process_path,
            ),
        )
        .expect("write test");
        fs::write(
            project.join("src/main_test.stdout"),
            "fs ok\n\nfalse\n7\nba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\ntrue\nnone\n",
        )
        .expect("write golden");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "fs ok\n\nfalse\n7\nba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\ntrue\nnone\n"
        );

        let tests = run_project_tests(&project).expect("run tests");
        assert_eq!(tests.passed, 1);
        assert_eq!(tests.failed, 0);
    }

    #[cfg(unix)]
    #[test]
    fn build_project_scopes_fs_read_to_manifest_root() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("scoped-fs");
        create_project(&project, Some("scoped-fs-app")).expect("create project");
        let data = project.join("data");
        fs::create_dir_all(&data).expect("create data dir");
        fs::write(data.join("x.txt"), "inside ok").expect("write inside fixture");
        let outside = dir.path().join("outside.txt");
        fs::write(&outside, "outside secret").expect("write outside fixture");
        symlink(&outside, data.join("evil")).expect("create escaping symlink");
        let large = fs::File::create(data.join("large.txt")).expect("create large fixture");
        large
            .set_len(100 * 1024 * 1024)
            .expect("size large fixture");
        fs::write(
            project.join("axiom.toml"),
            "[package]\nname = \"scoped-fs-app\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = true\nfs_root = \"data\"\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        let source = format!(
            "match fs_read(\"data/x.txt\") {{\nSome(value) {{\nprint value\n}}\nNone {{\nprint \"inside denied\"\n}}\n}}\nmatch fs_read({outside:?}) {{\nSome(_value) {{\nprint \"absolute leak\"\n}}\nNone {{\nprint \"absolute denied\"\n}}\n}}\nmatch fs_read(\"data/../../outside.txt\") {{\nSome(_value) {{\nprint \"traversal leak\"\n}}\nNone {{\nprint \"traversal denied\"\n}}\n}}\nmatch fs_read(\"data/evil\") {{\nSome(_value) {{\nprint \"symlink leak\"\n}}\nNone {{\nprint \"symlink denied\"\n}}\n}}\nmatch fs_read(\"data/large.txt\") {{\nSome(_value) {{\nprint \"large leak\"\n}}\nNone {{\nprint \"large denied\"\n}}\n}}\n",
            outside = outside.to_string_lossy(),
        );
        fs::write(project.join("src/main.ax"), &source).expect("write source");
        fs::write(project.join("src/main_test.ax"), source).expect("write test");
        fs::write(
            project.join("src/main_test.stdout"),
            "inside ok\nabsolute denied\ntraversal denied\nsymlink denied\nlarge denied\n",
        )
        .expect("write golden");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "inside ok\nabsolute denied\ntraversal denied\nsymlink denied\nlarge denied\n"
        );

        let tests = run_project_tests(&project).expect("run tests");
        assert_eq!(tests.passed, 1);
        assert_eq!(tests.failed, 0);
    }

    #[test]
    fn stage1_project_imports_synthetic_stdlib_time_module() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-time-app");
        create_project(&project, Some("stdlib-time-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-time-app",
                false,
                false,
                false,
                false,
                true,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/time.ax\"\nlet start: Instant = now()\nlet pause: Duration = duration_ms(0)\nprint start.ms > 0\nprint now_ms() > 0\nprint sleep(pause) == 0\nlet elapsed: int = elapsed_ms(start)\nprint elapsed == elapsed\n",
        )
        .expect("write source");
        fs::write(
            project.join("src/main_test.ax"),
            "import \"std/time.ax\"\nlet start: Instant = now()\nlet pause: Duration = duration_ms(0)\nprint start.ms > 0\nprint now_ms() > 0\nprint sleep(pause) == 0\nlet elapsed: int = elapsed_ms(start)\nprint elapsed == elapsed\n",
        )
        .expect("write test");
        fs::write(
            project.join("src/main_test.stdout"),
            "true\ntrue\ntrue\ntrue\n",
        )
        .expect("write golden");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "true\ntrue\ntrue\ntrue\n"
        );

        let tests = run_project_tests(&project).expect("run tests");
        assert_eq!(tests.passed, 1);
        assert_eq!(tests.failed, 0);
    }

    #[test]
    fn stage1_project_rejects_stdlib_time_without_clock_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-time-denied");
        create_project(&project, Some("stdlib-time-denied")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-time-denied",
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/time.ax\"\nlet start: Instant = now()\nprint sleep(duration_ms(0))\nprint elapsed_ms(start)\n",
        )
        .expect("write source");

        let err = check_project(&project).expect_err("expected capability denial");
        assert!(
            err.message.contains("requires [capabilities].clock = true"),
            "unexpected diagnostic: {err:?}",
        );
    }

    #[test]
    fn stage1_project_imports_synthetic_stdlib_env_module() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-env-app");
        create_project(&project, Some("stdlib-env-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-env-app",
                false,
                false,
                false,
                true,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        let source = "import \"std/env.ax\"\nmatch get_env(\"__AXIOM_STAGE1_MISSING__\") {\nSome(value) {\nprint value\n}\nNone {\nprint \"none\"\n}\n}\n";
        fs::write(project.join("src/main.ax"), source).expect("write source");
        fs::write(project.join("src/main_test.ax"), source).expect("write test");
        fs::write(project.join("src/main_test.stdout"), "none\n").expect("write golden");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .env_remove("__AXIOM_STAGE1_MISSING__")
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "none\n");

        let tests = run_project_tests(&project).expect("run tests");
        assert_eq!(tests.passed, 1);
        assert_eq!(tests.failed, 0);
    }

    #[test]
    fn stage1_project_rejects_stdlib_env_without_env_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-env-denied");
        create_project(&project, Some("stdlib-env-denied")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-env-denied",
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/env.ax\"\nmatch get_env(\"X\") {\nSome(v) {\nprint v\n}\nNone {\nprint \"none\"\n}\n}\n",
        )
        .expect("write source");

        let err = check_project(&project).expect_err("expected capability denial");
        assert!(
            err.message
                .contains("requires [capabilities].env = [\"NAME\"]"),
            "unexpected diagnostic: {err:?}",
        );
    }

    #[test]
    fn stage1_project_imports_synthetic_stdlib_fs_module() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-fs-app");
        create_project(&project, Some("stdlib-fs-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-fs-app",
                true,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        let fixture = project.join("src/fixture.txt");
        fs::write(&fixture, "hello stdlib fs\n").expect("write fixture");
        let fixture_literal = fixture.to_string_lossy().replace('\\', "\\\\");
        let source = format!(
            "import \"std/fs.ax\"\nmatch read_file(\"{fixture_literal}\") {{\nSome(value) {{\nprint value\n}}\nNone {{\nprint \"missing\"\n}}\n}}\n"
        );
        fs::write(project.join("src/main.ax"), &source).expect("write source");
        fs::write(project.join("src/main_test.ax"), &source).expect("write test");
        fs::write(project.join("src/main_test.stdout"), "hello stdlib fs\n\n")
            .expect("write golden");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "hello stdlib fs\n\n"
        );

        let tests = run_project_tests(&project).expect("run tests");
        assert_eq!(tests.passed, 1);
        assert_eq!(tests.failed, 0);
    }

    #[test]
    fn build_project_scopes_fs_write_to_manifest_root_without_read_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("scoped-fs-write-only");
        create_project(&project, Some("scoped-fs-write-only-app")).expect("create project");
        fs::create_dir_all(project.join("data")).expect("create data dir");
        fs::write(
            project.join("axiom.toml"),
            r#"[package]
name = "scoped-fs-write-only-app"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
"fs:write" = true
fs_root = "data"
net = false
process = false
env = false
clock = false
crypto = false
"#,
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            r#"print fs_write("data/inside.txt", "inside") == 0
print fs_write("outside.txt", "outside") == -1
print fs_write("data/../outside.txt", "traversal") == -1
"#,
        )
        .expect("write source");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "true\ntrue\ntrue\n"
        );
        assert_eq!(
            fs::read_to_string(project.join("data/inside.txt")).expect("inside write"),
            "inside",
        );
        assert!(!project.join("outside.txt").exists());
    }

    #[test]
    fn stage1_project_imports_stdlib_fs_read_without_write_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-fs-read-only-app");
        create_project(&project, Some("stdlib-fs-read-only-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            r#"[package]
name = "stdlib-fs-read-only-app"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = true
"fs:write" = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(project.join("src/fixture.txt"), "read only\n").expect("write fixture");
        fs::write(
            project.join("src/main.ax"),
            r#"import "std/fs.ax"
match read_file("src/fixture.txt") {
Some(value) {
print value
}
None {
print "missing"
}
}
"#,
        )
        .expect("write source");

        check_project(&project).expect("read-only std/fs import should not require fs:write");
    }

    #[test]
    fn stage1_project_imports_synthetic_stdlib_fs_write_helpers() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-fs-write-app");
        create_project(&project, Some("stdlib-fs-write-app")).expect("create project");
        fs::create_dir_all(project.join("data/empty")).expect("create empty dir");
        fs::write(
            project.join("axiom.toml"),
            "[package]\nname = \"stdlib-fs-write-app\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = true\n\"fs:write\" = true\nfs_root = \"data\"\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        let source = "import \"std/fs.ax\"\nprint create_file(\"data/new.txt\") == 0\nprint append_file(\"data/new.txt\", \"hello\") == 0\nprint append_file(\"data/new.txt\", \" world\") == 0\nmatch read_file(\"data/new.txt\") {\nSome(value) {\nprint value\n}\nNone {\nprint \"missing\"\n}\n}\nprint write_file(\"data/write.txt\", \"first\") == 0\nprint replace_file(\"data/write.txt\", \"second\") == 0\nmatch read_file(\"data/write.txt\") {\nSome(value) {\nprint value\n}\nNone {\nprint \"missing\"\n}\n}\nprint mkdir_all(\"data/nested/dir\") == 0\nprint write_file(\"data/nested/dir/file.txt\", \"nested\") == 0\nprint remove_file(\"data/nested/dir/file.txt\") == 0\nprint mkdir(\"data/single\") == 0\nprint remove_dir(\"data/single\") == 0\nprint write_file(\"../escape.txt\", \"leak\") == -1\n";
        fs::write(project.join("src/main.ax"), source).expect("write source");
        fs::write(project.join("src/main_test.ax"), source).expect("write test");
        fs::write(
            project.join("src/main_test.stdout"),
            "true\ntrue\ntrue\nhello world\ntrue\ntrue\nsecond\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n",
        )
        .expect("write golden");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "true\ntrue\ntrue\nhello world\ntrue\ntrue\nsecond\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n"
        );
        assert!(!project.join("escape.txt").exists());
        let _ = fs::remove_file(project.join("data/new.txt"));
        let _ = fs::remove_file(project.join("data/write.txt"));
        let _ = fs::remove_dir_all(project.join("data/nested"));

        let tests = run_project_tests(&project).expect("run tests");
        assert_eq!(tests.passed, 1);
        assert_eq!(tests.failed, 0);
    }

    #[test]
    fn stage1_project_rejects_fs_write_without_write_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-fs-write-denied");
        create_project(&project, Some("stdlib-fs-write-denied")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            "[package]\nname = \"stdlib-fs-write-denied\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = true\n\"fs:write\" = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/fs.ax\"\nprint write_file(\"x\", \"content\")\n",
        )
        .expect("write source");

        let err = check_project(&project).expect_err("expected fs write capability denial");
        assert!(
            err.message
                .contains("requires [capabilities].fs:write = true"),
            "unexpected diagnostic: {err:?}",
        );
    }

    #[test]
    fn stage1_project_rejects_stdlib_fs_without_fs_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-fs-denied");
        create_project(&project, Some("stdlib-fs-denied")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-fs-denied",
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/fs.ax\"\nmatch read_file(\"x\") {\nSome(v) {\nprint v\n}\nNone {\nprint \"missing\"\n}\n}\n",
        )
        .expect("write source");

        let err = check_project(&project).expect_err("expected capability denial");
        assert!(
            err.message.contains("requires [capabilities].fs = true"),
            "unexpected diagnostic: {err:?}",
        );
    }

    #[test]
    fn stage1_project_imports_synthetic_stdlib_process_module() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-process-app");
        create_project(&project, Some("stdlib-process-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-process-app",
                false,
                false,
                true,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        let source = "import \"std/process.ax\"\nlet status: int = run_status(\"__axiom_stage1_missing_binary__\")\nprint status\n";
        fs::write(project.join("src/main.ax"), source).expect("write source");
        fs::write(project.join("src/main_test.ax"), source).expect("write test");
        fs::write(project.join("src/main_test.stdout"), "-1\n").expect("write golden");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "-1\n");

        let tests = run_project_tests(&project).expect("run tests");
        assert_eq!(tests.passed, 1);
        assert_eq!(tests.failed, 0);
    }

    #[test]
    fn stage1_project_rejects_stdlib_process_without_process_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-process-denied");
        create_project(&project, Some("stdlib-process-denied")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-process-denied",
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/process.ax\"\nlet status: int = run_status(\"x\")\nprint status\n",
        )
        .expect("write source");

        let err = check_project(&project).expect_err("expected capability denial");
        assert!(
            err.message
                .contains("requires [capabilities].process = true"),
            "unexpected diagnostic: {err:?}",
        );
    }

    #[test]
    fn stage1_project_imports_synthetic_stdlib_crypto_hash_module() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-crypto-hash-app");
        create_project(&project, Some("stdlib-crypto-hash-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-crypto-hash-app",
                false,
                false,
                false,
                false,
                false,
                true,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        let source = "import \"std/crypto_hash.ax\"\nprint sha256(\"abc\")\n";
        fs::write(project.join("src/main.ax"), source).expect("write source");
        fs::write(project.join("src/main_test.ax"), source).expect("write test");
        fs::write(
            project.join("src/main_test.stdout"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n",
        )
        .expect("write golden");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n"
        );

        let tests = run_project_tests(&project).expect("run tests");
        assert_eq!(tests.passed, 1);
        assert_eq!(tests.failed, 0);
    }

    #[test]
    fn stage1_project_rejects_stdlib_crypto_hash_without_crypto_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-crypto-hash-denied");
        create_project(&project, Some("stdlib-crypto-hash-denied")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-crypto-hash-denied",
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/crypto_hash.ax\"\nprint sha256(\"abc\")\n",
        )
        .expect("write source");

        let err = check_project(&project).expect_err("expected capability denial");
        assert!(
            err.message
                .contains("requires [capabilities].crypto = true"),
            "unexpected diagnostic: {err:?}",
        );
    }

    #[test]
    fn stage1_project_imports_synthetic_stdlib_crypto_mac_module() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-crypto-mac-app");
        create_project(&project, Some("stdlib-crypto-mac-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-crypto-mac-app",
                false,
                false,
                false,
                false,
                false,
                true,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        let source = "import \"std/crypto_mac.ax\"\nprint hmac_sha256(\"key\", \"The quick brown fox jumps over the lazy dog\")\nprint constant_time_eq(hmac_sha256(\"key\", \"The quick brown fox jumps over the lazy dog\"), \"f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8\")\nprint constant_time_eq(hmac_sha256(\"key\", \"The quick brown fox jumps over the lazy dog\"), \"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\")\nprint constant_time_eq(\"short\", \"shorter\")\n";
        fs::write(project.join("src/main.ax"), source).expect("write source");
        fs::write(project.join("src/main_test.ax"), source).expect("write test");
        fs::write(
            project.join("src/main_test.stdout"),
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8\ntrue\nfalse\nfalse\n",
        )
        .expect("write golden");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8\ntrue\nfalse\nfalse\n"
        );

        let tests = run_project_tests(&project).expect("run tests");
        assert_eq!(tests.passed, 1);
        assert_eq!(tests.failed, 0);
    }

    #[test]
    fn stage1_project_rejects_stdlib_crypto_mac_without_crypto_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-crypto-mac-denied");
        create_project(&project, Some("stdlib-crypto-mac-denied")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-crypto-mac-denied",
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/crypto_mac.ax\"\nprint hmac_sha256(\"key\", \"message\")\n",
        )
        .expect("write source");

        let err = check_project(&project).expect_err("expected capability denial");
        assert!(
            err.message
                .contains("requires [capabilities].crypto = true"),
            "unexpected diagnostic: {err:?}",
        );
    }

    #[test]
    fn stage1_project_imports_synthetic_stdlib_net_module() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-net-app");
        create_project(&project, Some("stdlib-net-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-net-app",
                false,
                true,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        let source = "import \"std/net.ax\"\nmatch resolve(\"localhost\") {\nSome(_address) {\nprint true\n}\nNone {\nprint false\n}\n}\nmatch tcp_listen_loopback_once(\"tcp pong\", 1000) {\nSome(port) {\nmatch tcp_dial(\"127.0.0.1\", port, \"tcp ping\", 1000) {\nSome(reply) {\nprint reply\n}\nNone {\nprint \"tcp none\"\n}\n}\n}\nNone {\nprint \"tcp listen none\"\n}\n}\nmatch udp_bind_loopback_once(\"udp pong\", 1000) {\nSome(port) {\nmatch udp_send_recv(\"127.0.0.1\", port, \"udp ping\", 1000) {\nSome(reply) {\nprint reply\n}\nNone {\nprint \"udp none\"\n}\n}\n}\nNone {\nprint \"udp bind none\"\n}\n}\n";
        fs::write(project.join("src/main.ax"), source).expect("write source");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        let expected = if loopback_socket_bind_available() {
            "false\ntcp pong\nudp pong\n"
        } else {
            "false\ntcp listen none\nudp bind none\n"
        };
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }

    #[test]
    fn stage1_project_rejects_stdlib_net_without_net_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-net-denied");
        create_project(&project, Some("stdlib-net-denied")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-net-denied",
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/net.ax\"\nmatch tcp_listen_loopback_once(\"pong\", 1000) {\nSome(_port) {\nprint true\n}\nNone {\nprint false\n}\n}\n",
        )
        .expect("write source");

        let err = check_project(&project).expect_err("expected capability denial");
        assert!(
            err.message.contains("requires [capabilities].net = true"),
            "unexpected diagnostic: {err:?}",
        );
    }

    #[test]
    fn stage1_project_imports_synthetic_stdlib_io_module() {
        // `std/io.ax` is the first stdlib module not tied to a capability
        // flag: `io_eprintln` is ungated, matching the ambient status of the
        // `print` statement. All six capabilities stay `false` to prove the
        // ungated path does not require a manifest opt-in.
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-io-app");
        create_project(&project, Some("stdlib-io-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-io-app",
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        let source = "import \"std/io.ax\"\nlet n: int = eprintln(\"hello stderr\")\nprint n > 0\n";
        fs::write(project.join("src/main.ax"), source).expect("write source");
        fs::write(project.join("src/main_test.ax"), source).expect("write test");
        fs::write(project.join("src/main_test.stdout"), "true\n").expect("write golden");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "true\n");
        assert_eq!(String::from_utf8_lossy(&output.stderr), "hello stderr\n");

        let tests = run_project_tests(&project).expect("run tests");
        assert_eq!(tests.passed, 1);
        assert_eq!(tests.failed, 0);
    }

    #[test]
    fn stage1_project_imports_synthetic_stdlib_json_module() {
        // `std/json.ax` stays ungated in stage1: parsing and serialising scalar
        // JSON values does not cross a host capability boundary.
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-json-app");
        create_project(&project, Some("stdlib-json-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-json-app",
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        let source = r#"import "std/json.ax"

match parse_int("42") {
Some(value) {
print value
}
None {
print "none"
}
}

match parse_string("\"agent\"") {
Some(value) {
print value
}
None {
print "none"
}
}

print stringify_bool(true)
print stringify_int(7)
print stringify_string("agent")
print object3(field_string("name", "agent"), field_int("retries", 3), field_bool("ready", true))

let payload_name: string = object3(field_string("name", "agent"), field_int("retries", 3), field_bool("ready", true))
match parse_field_string(payload_name, "name") {
Some(value) {
print value
}
None {
print "missing"
}
}
let payload_retries: string = object3(field_string("name", "agent"), field_int("retries", 3), field_bool("ready", true))
match parse_field_int(payload_retries, "retries") {
Some(value) {
print value
}
None {
print 0
}
}
let payload_ready: string = object3(field_string("name", "agent"), field_int("retries", 3), field_bool("ready", true))
match parse_field_bool(payload_ready, "ready") {
Some(value) {
print value
}
None {
print false
}
}
print schema_object3(schema_field_string("name"), schema_field_int("retries"), schema_field_bool("ready"))

match parse_bool("123") {
Some(_value) {
print "bad"
}
None {
print "none"
}
}
"#;
        fs::write(project.join("src/main.ax"), source).expect("write source");
        fs::write(project.join("src/main_test.ax"), source).expect("write test");
        fs::write(
            project.join("src/main_test.stdout"),
            "42\nagent\ntrue\n7\n\"agent\"\n{\"name\":\"agent\",\"retries\":3,\"ready\":true}\nagent\n3\ntrue\n{\"type\":\"object\",\"properties\":{\"name\":{\"type\":\"string\"},\"retries\":{\"type\":\"integer\"},\"ready\":{\"type\":\"boolean\"}}}\nnone\n",
        )
        .expect("write golden");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "42\nagent\ntrue\n7\n\"agent\"\n{\"name\":\"agent\",\"retries\":3,\"ready\":true}\nagent\n3\ntrue\n{\"type\":\"object\",\"properties\":{\"name\":{\"type\":\"string\"},\"retries\":{\"type\":\"integer\"},\"ready\":{\"type\":\"boolean\"}}}\nnone\n"
        );

        let tests = run_project_tests(&project).expect("run tests");
        assert_eq!(tests.passed, 1);
        assert_eq!(tests.failed, 0);
    }

    #[test]
    fn stage1_project_imports_synthetic_stdlib_regex_module() {
        // `std/regex.ax` stays ungated in stage1: matching runs inside the
        // deterministic generated runtime and does not cross a host capability
        // boundary. The engine uses NFA-state simulation rather than
        // backtracking so agent-provided patterns stay DoS-safe.
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-regex-app");
        create_project(&project, Some("stdlib-regex-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-regex-app",
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        let source = "import \"std/regex.ax\"
print is_match(\"^h.llo$\", \"hello\")
print is_match(\"^[a-z]+$\", \"agent\")
print is_match(\"^[^0-9]+$\", \"agent7\")
match find(\"[0-9]+\", \"issue-238-ready\") {
Some(value) {
print value
}
None {
print \"none\"
}
}
print replace_all(\"[0-9]+\", \"issue-238-ready\", \"#\")
print is_match(\"a*a\", \"aaaaaaaaaaaaaaaa\")
";
        fs::write(project.join("src/main.ax"), source).expect("write source");
        fs::write(project.join("src/main_test.ax"), source).expect("write test");
        fs::write(
            project.join("src/main_test.stdout"),
            "true
true
false
238
issue-#-ready
true
",
        )
        .expect("write golden");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "true
true
false
238
issue-#-ready
true
"
        );

        let tests = run_project_tests(&project).expect("run tests");
        assert_eq!(tests.passed, 1);
        assert_eq!(tests.failed, 0);
    }

    #[test]
    fn stage1_project_imports_synthetic_stdlib_collections_module() {
        // `std/collections.ax` is ungated: it is implemented entirely in Axiom
        // on top of AG2 generic functions and existing collection primitives.
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-collections-app");
        create_project(&project, Some("stdlib-collections-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-collections-app",
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        let source = "import \"std/collections.ax\"\nlet numbers: [int] = [4, 5, 6, 7]\nprint count<int>(numbers[:])\nprint is_empty<int>(numbers[:])\nprint has_items<int>(numbers[:])\nlet middle: &[int] = window<int>(numbers[:], 1, 3)\nprint count<int>(middle)\nprint first(middle)\nprint last(middle)\nlet prefix: &[int] = take<int>(numbers[:], 2)\nprint last(prefix)\nlet suffix: &[int] = skip<int>(numbers[:], 2)\nprint first(suffix)\nlet words: [string] = [\"build\", \"test\", \"ship\"]\nprint count<string>(words[:])\nlet empty_words: &[string] = take<string>(words[:], 0)\nprint is_empty<string>(empty_words)\n";
        fs::write(project.join("src/main.ax"), source).expect("write source");
        fs::write(project.join("src/main_test.ax"), source).expect("write test");
        fs::write(
            project.join("src/main_test.stdout"),
            "4\nfalse\ntrue\n2\n5\n6\n5\n6\n3\ntrue\n",
        )
        .expect("write golden");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "4\nfalse\ntrue\n2\n5\n6\n5\n6\n3\ntrue\n"
        );

        let tests = run_project_tests(&project).expect("run tests");
        assert_eq!(tests.passed, 1);
        assert_eq!(tests.failed, 0);
    }

    #[test]
    fn stage1_project_imports_synthetic_stdlib_string_builder_module() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-string-builder-app");
        create_project(&project, Some("stdlib-string-builder-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-string-builder-app",
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        let source = "import \"std/string_builder.ax\"\nlet empty: StringBuilder = builder()\nlet greeting: StringBuilder = push_str(empty, \"hello\")\nlet spaced: StringBuilder = push_str(greeting, \" \")\nlet finished: StringBuilder = push_str(spaced, \"stdlib\")\nprint finish(finished)\nlet seeded: StringBuilder = from_string(\"first\")\nlet second: StringBuilder = push_line(seeded, \" line\")\nlet third: StringBuilder = push_str(second, \"second line\")\nprint finish(third)\n";
        fs::write(project.join("src/main.ax"), source).expect("write source");
        fs::write(project.join("src/main_test.ax"), source).expect("write test");
        fs::write(
            project.join("src/main_test.stdout"),
            "hello stdlib\nfirst line\nsecond line\n",
        )
        .expect("write golden");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "hello stdlib\nfirst line\nsecond line\n"
        );

        let tests = run_project_tests(&project).expect("run tests");
        assert_eq!(tests.passed, 1);
        assert_eq!(tests.failed, 0);
    }

    #[test]
    fn stage1_project_imports_synthetic_stdlib_log_module() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-log-app");
        create_project(&project, Some("stdlib-log-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-log-app",
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        let source = "import \"std/log.ax\"\nlet attrs: string = fields3(field_string(\"component\", \"worker\"), field_int(\"attempt\", 2), field_bool(\"ready\", true))\nprint event(\"info\", \"started\", attrs)\nlet attrs_for_log: string = fields3(field_string(\"component\", \"worker\"), field_int(\"attempt\", 2), field_bool(\"ready\", true))\nlet written: int = info_attrs(\"started\", attrs_for_log)\nprint written > 0\n";
        fs::write(project.join("src/main.ax"), source).expect("write source");
        fs::write(
            project.join("src/main_test.ax"),
            "import \"std/log.ax\"\nlet attrs: string = fields3(field_string(\"component\", \"worker\"), field_int(\"attempt\", 2), field_bool(\"ready\", true))\nprint event(\"info\", \"started\", attrs)\n",
        )
        .expect("write test");
        fs::write(
            project.join("src/main_test.stdout"),
            "{\"level\":\"info\",\"message\":\"started\",\"attributes\":{\"component\":\"worker\",\"attempt\":2,\"ready\":true}}\n",
        )
        .expect("write golden");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "{\"level\":\"info\",\"message\":\"started\",\"attributes\":{\"component\":\"worker\",\"attempt\":2,\"ready\":true}}\ntrue\n"
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "{\"level\":\"info\",\"message\":\"started\",\"attributes\":{\"component\":\"worker\",\"attempt\":2,\"ready\":true}}\n"
        );

        let tests = run_project_tests(&project).expect("run tests");
        assert_eq!(tests.passed, 1);
        assert_eq!(tests.failed, 0);
    }

    #[test]
    fn stage1_project_imports_synthetic_stdlib_sync_module() {
        // `std/sync.ax` is ungated in stage1 because it is implemented in
        // Axiom using ownership tokens rather than host threads or blocking
        // runtime services. Async-aware channels and wakeups stay AG4.2 work.
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-sync-app");
        create_project(&project, Some("stdlib-sync-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-sync-app",
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        let source = "import \"std/sync.ax\"\nlet counter: Mutex<int> = mutex<int>(1)\nlet guard: MutexGuard<int> = lock<int>(counter)\nlet updated: Mutex<int> = replace<int>(guard, 2)\nlet final_guard: MutexGuard<int> = lock<int>(updated)\nprint into_inner<int>(final_guard)\nlet ready: Once<string> = once_with<string>(\"configured\")\nprint once_is_set<string>(ready)\nlet empty: Once<int> = once<int>(None)\nmatch once_take<int>(empty) {\nSome(value) {\nprint value\n}\nNone {\nprint \"empty\"\n}\n}\nlet channel: Channel<string> = channel<string>(None)\nlet sent: Channel<string> = send<string>(channel, \"message\")\nmatch try_recv<string>(sent) {\nSome(message) {\nprint message\n}\nNone {\nprint \"missing\"\n}\n}\n";
        fs::write(project.join("src/main.ax"), source).expect("write source");
        fs::write(project.join("src/main_test.ax"), source).expect("write test");
        fs::write(
            project.join("src/main_test.stdout"),
            "2\ntrue\nempty\nmessage\n",
        )
        .expect("write golden");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "2\ntrue\nempty\nmessage\n"
        );

        let tests = run_project_tests(&project).expect("run tests");
        assert_eq!(tests.passed, 1);
        assert_eq!(tests.failed, 0);
    }

    #[test]
    fn stage1_project_supports_async_runtime_surface() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-async-app");
        create_project(&project, Some("stdlib-async-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-async-app",
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        let source = "import \"std/async.ax\"\nasync fn compute(value: int): int {\nreturn value + 1\n}\nlet direct: Task<int> = compute(40)\nprint await direct\nlet handle: JoinHandle<int> = spawn<int>(compute(6))\nprint await join<int>(handle)\nlet canceled: Task<int> = cancel<int>(compute(1))\nprint is_canceled<int>(canceled)\nlet maybe: Option<int> = await timeout<int>(compute(5), 100)\nmatch maybe {\nSome(value) {\nprint value\n}\nNone {\nprint 0\n}\n}\nlet messages: AsyncChannel<string> = channel<string>()\nlet sent: AsyncChannel<string> = await send<string>(messages, \"message\")\nlet received: Option<string> = await recv<string>(sent)\nmatch received {\nSome(message) {\nprint message\n}\nNone {\nprint \"missing\"\n}\n}\nlet left_index: Task<Option<string>> = ready<Option<string>>(None)\nlet right_index: Task<Option<string>> = ready<Option<string>>(Some(\"right\"))\nlet picked_index: SelectResult<string> = await select<string>(left_index, right_index)\nprint selected<string>(picked_index)\nlet left_value: Task<Option<string>> = ready<Option<string>>(None)\nlet right_value: Task<Option<string>> = ready<Option<string>>(Some(\"right\"))\nlet picked_value: SelectResult<string> = await select<string>(left_value, right_value)\nmatch selected_value<string>(picked_value) {\nSome(value) {\nprint value\n}\nNone {\nprint \"none\"\n}\n}\n";
        fs::write(project.join("src/main.ax"), source).expect("write source");
        fs::write(project.join("src/main_test.ax"), source).expect("write test");
        fs::write(
            project.join("src/main_test.stdout"),
            "41\n7\ntrue\n6\nmessage\n1\nright\n",
        )
        .expect("write golden");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "41\n7\ntrue\n6\nmessage\n1\nright\n"
        );

        let tests = run_project_tests(&project).expect("run tests");
        assert_eq!(tests.passed, 1);
        assert_eq!(tests.failed, 0);
    }

    #[test]
    fn stage1_project_rejects_stdlib_json_with_wrong_argument_type() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-json-bad-arg");
        create_project(&project, Some("stdlib-json-bad-arg")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-json-bad-arg",
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/json.ax\"\nmatch parse_int(true) {\nSome(value) {\nprint value\n}\nNone {\nprint 0\n}\n}\n",
        )
        .expect("write source");

        let err = check_project(&project).expect_err("expected json type error");
        assert!(
            err.message
                .contains("expects argument type string, got bool"),
            "unexpected diagnostic: {err:?}",
        );
    }

    #[test]
    fn stage1_project_rejects_stdlib_regex_with_wrong_argument_type() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-regex-bad-arg");
        create_project(&project, Some("stdlib-regex-bad-arg")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-regex-bad-arg",
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/regex.ax\"
print is_match(\"[a-z]+\", true)
",
        )
        .expect("write source");

        let err = check_project(&project).expect_err("expected regex type error");
        assert!(
            err.message
                .contains("expects argument type string, got bool"),
            "unexpected diagnostic: {err:?}",
        );
    }

    #[test]
    fn stage1_stdlib_http_rejects_loopback_address() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-http-loopback-denied");
        create_project(&project, Some("stdlib-http-loopback-denied")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-http-loopback-denied",
                false,
                true,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/http.ax\"\nmatch get(\"http://127.0.0.1:1/\") {\nSome(_body) {\nprint \"body\"\n}\nNone {\nprint \"none\"\n}\n}\n",
        )
        .expect("write source");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "none\n");
    }

    #[test]
    fn stage1_stdlib_http_rejects_metadata_address() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-http-metadata-denied");
        create_project(&project, Some("stdlib-http-metadata-denied")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-http-metadata-denied",
                false,
                true,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/http.ax\"\nmatch get(\"http://169.254.169.254/latest/meta-data/\") {\nSome(_body) {\nprint \"body\"\n}\nNone {\nprint \"none\"\n}\n}\n",
        )
        .expect("write source");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "none\n");
    }

    #[test]
    fn stage1_project_rejects_stdlib_http_without_net_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-http-denied");
        create_project(&project, Some("stdlib-http-denied")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-http-denied",
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/http.ax\"\nmatch get(\"http://127.0.0.1:1/\") {\nSome(_b) {\nprint true\n}\nNone {\nprint false\n}\n}\n",
        )
        .expect("write source");

        let err = check_project(&project).expect_err("expected capability denial");
        assert!(
            err.message.contains("requires [capabilities].net = true"),
            "unexpected diagnostic: {err:?}",
        );
    }

    fn find_free_loopback_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind loopback probe")
            .local_addr()
            .expect("probe local addr")
            .port()
    }

    #[test]
    fn stage1_stdlib_http_service_serves_one_request() {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        use std::thread;
        use std::time::{Duration, Instant};

        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-http-service");
        create_project(&project, Some("stdlib-http-service")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-http-service",
                false,
                true,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        let port = find_free_loopback_port();
        fs::write(
            project.join("src/main.ax"),
            format!(
                r#"import "std/http.ax"
print serve_once("127.0.0.1:{port}", "hello from axiom")
"#
            ),
        )
        .expect("write source");

        let built = build_project(&project).expect("build project");
        let child = compiled_binary_command(&built.binary)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn compiled binary");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(stream) => break stream,
                Err(err) if Instant::now() < deadline => {
                    let _ = err;
                    thread::sleep(Duration::from_millis(25));
                }
                Err(err) => panic!("server never became ready: {err}"),
            }
        };
        stream
            .write_all(b"GET / HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n")
            .expect("write request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read response");
        assert!(
            response.starts_with("HTTP/1.0 200 OK\r\n"),
            "unexpected response: {response:?}"
        );
        assert!(
            response.contains("Content-Length: 16\r\n"),
            "unexpected response headers: {response:?}"
        );
        assert!(
            response.ends_with("hello from axiom"),
            "unexpected response body: {response:?}"
        );

        let output = child.wait_with_output().expect("wait for server exit");
        assert!(output.status.success(), "server process failed: {output:?}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "true\n");
        assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    }

    #[test]
    fn stage1_stdlib_http_service_rejects_non_loopback_bind() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-http-service-non-loopback-denied");
        create_project(&project, Some("stdlib-http-service-non-loopback-denied"))
            .expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-http-service-non-loopback-denied",
                false,
                true,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            r#"import "std/http.ax"
print serve_once("0.0.0.0:18080", "hello")
"#,
        )
        .expect("write source");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert!(
            output.status.success(),
            "service process failed: {output:?}"
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "false\n");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("http server bind address must resolve only to loopback"),
            "unexpected stderr: {:?}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn stage1_stdlib_http_routed_service_rejects_non_loopback_bind() {
        let dir = tempdir().expect("tempdir");
        let project = dir
            .path()
            .join("stdlib-http-routed-service-non-loopback-denied");
        create_project(
            &project,
            Some("stdlib-http-routed-service-non-loopback-denied"),
        )
        .expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-http-routed-service-non-loopback-denied",
                false,
                true,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            r#"import "std/http.ax"
let selected_route: HttpRoute = route("/ready", "hello")
print serve("0.0.0.0:18080", selected_route, 1)
"#,
        )
        .expect("write source");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert!(
            output.status.success(),
            "service process failed: {output:?}"
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "false\n");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("http server bind address must resolve only to loopback"),
            "unexpected stderr: {:?}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn stage1_stdlib_http_service_routes_multiple_requests() {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        use std::thread;
        use std::time::{Duration, Instant};

        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-http-routed-service");
        create_project(&project, Some("stdlib-http-routed-service")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-http-routed-service",
                false,
                true,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        let port = find_free_loopback_port();
        fs::write(
            project.join("src/main.ax"),
            format!(
                r#"import "std/http.ax"

let selected_route: HttpRoute = route("/ready", "routed response")
print serve("127.0.0.1:{port}", selected_route, 2)
"#
            ),
        )
        .expect("write source");

        let built = build_project(&project).expect("build project");
        let child = compiled_binary_command(&built.binary)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn compiled binary");

        let connect = || {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match TcpStream::connect(("127.0.0.1", port)) {
                    Ok(stream) => break stream,
                    Err(err) if Instant::now() < deadline => {
                        let _ = err;
                        thread::sleep(Duration::from_millis(25));
                    }
                    Err(err) => panic!("server never became ready: {err}"),
                }
            }
        };
        let mut first = connect();
        let mut second = connect();
        first
            .write_all(b"GET /ready HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n")
            .expect("write first request");
        second
            .write_all(b"GET /missing HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n")
            .expect("write second request");
        let mut first_response = String::new();
        let mut second_response = String::new();
        first
            .read_to_string(&mut first_response)
            .expect("read first response");
        second
            .read_to_string(&mut second_response)
            .expect("read second response");
        assert!(
            first_response.starts_with("HTTP/1.0 200 OK\r\n"),
            "unexpected first response: {first_response:?}"
        );
        assert!(
            first_response.ends_with("routed response"),
            "unexpected first body: {first_response:?}"
        );
        assert!(
            second_response.starts_with("HTTP/1.0 404 Not Found\r\n"),
            "unexpected second response: {second_response:?}"
        );

        let output = child.wait_with_output().expect("wait for server exit");
        assert!(output.status.success(), "server process failed: {output:?}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "true\n");
        assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    }

    #[test]
    fn stage1_project_rejects_stdlib_http_service_without_net_capability() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-http-service-denied");
        create_project(&project, Some("stdlib-http-service-denied")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            render_manifest_with_capabilities(
                "stdlib-http-service-denied",
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        fs::write(
            project.join("axiom.lock"),
            render_lockfile_for_project(&project, &manifest).expect("lockfile"),
        )
        .expect("write lockfile");
        fs::write(
            project.join("src/main.ax"),
            r#"import "std/http.ax"
print serve_once("127.0.0.1:18080", "hello")
"#,
        )
        .expect("write source");

        let err = check_project(&project).expect_err("expected capability denial");
        assert!(
            err.message.contains("requires [capabilities].net = true"),
            "unexpected diagnostic: {err:?}",
        );
    }

    #[test]
    fn stage1_runtime_reports_structured_error_for_index_errors() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("runtime-error-index");
        create_project(&project, Some("runtime-error-index")).expect("create project");
        fs::write(
            project.join("src/math.ax"),
            "pub fn explode(values: [int]): int {\nreturn values[1]\n}\n",
        )
        .expect("write module");
        fs::write(
            project.join("src/main.ax"),
            "import \"math.ax\"\nprint explode([7])\n",
        )
        .expect("write source");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");

        assert!(!output.status.success(), "program should fail");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("{\"kind\":\"runtime\",\"message\":\"array index out of bounds\"}"),
            "unexpected stderr: {stderr}"
        );
        assert!(!stderr.contains("panic:"), "unexpected stderr: {stderr}");
        assert!(
            !stderr.contains("Axiom stack trace"),
            "unexpected stderr: {stderr}"
        );
        assert!(!stderr.contains("explode"), "unexpected stderr: {stderr}");
        assert!(
            !stderr.contains("src/math.ax"),
            "unexpected stderr: {stderr}"
        );
    }

    #[test]
    fn stage1_runtime_reports_structured_error_for_panic_statement() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("panic-statement");
        create_project(&project, Some("panic-statement")).expect("create project");
        fs::write(project.join("src/main.ax"), "panic(\"boom\")\n").expect("write source");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");

        assert!(!output.status.success(), "program should fail");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("{\"kind\":\"panic\",\"message\":\"boom\"}"),
            "unexpected stderr: {stderr}"
        );
        assert!(
            !stderr.contains("runtime panic"),
            "unexpected stderr: {stderr}"
        );
        assert!(!stderr.contains("panic:"), "unexpected stderr: {stderr}");
    }

    #[test]
    fn stage1_runtime_reports_structured_error_for_slice_failures() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("runtime-error-slice");
        create_project(&project, Some("runtime-error-slice")).expect("create project");
        fs::write(
            project.join("src/math.ax"),
            "pub fn window(values: &[int]): &[int] {\nreturn values[0:2]\n}\n",
        )
        .expect("write module");
        fs::write(
            project.join("src/main.ax"),
            "import \"math.ax\"\nlet values: [int] = [7]\nlet tail: &[int] = window(values[:])\nprint len(tail)\n",
        )
        .expect("write source");

        let built = build_project(&project).expect("build project");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");

        assert!(!output.status.success(), "program should fail");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("{\"kind\":\"runtime\",\"message\":\"array slice end out of bounds\"}"),
            "unexpected stderr: {stderr}"
        );
        assert!(!stderr.contains("panic:"), "unexpected stderr: {stderr}");
        assert!(
            !stderr.contains("Axiom stack trace"),
            "unexpected stderr: {stderr}"
        );
        assert!(!stderr.contains("window"), "unexpected stderr: {stderr}");
        assert!(
            !stderr.contains("src/math.ax"),
            "unexpected stderr: {stderr}"
        );
    }

    #[test]
    fn stage1_runtime_reports_structured_slice_error_in_debug_and_release() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("runtime-error-slice-modes");
        create_project(&project, Some("runtime-error-slice-modes")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [int] = [7]\nlet window: &[int] = values[0:2]\nprint len(window)\n",
        )
        .expect("write source");

        for debug in [true, false] {
            let built = build_project_with_options(
                &project,
                &BuildOptions {
                    debug,
                    ..BuildOptions::default()
                },
            )
            .expect("build project");
            let output = compiled_binary_command(&built.binary)
                .output()
                .expect("run compiled binary");

            assert!(
                !output.status.success(),
                "program should fail for debug={debug}"
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                stderr.contains(
                    "{\"kind\":\"runtime\",\"message\":\"array slice end out of bounds\"}"
                ),
                "unexpected stderr for debug={debug}: {stderr}"
            );
            assert!(
                !stderr.contains("panic:"),
                "unexpected stderr for debug={debug}: {stderr}"
            );
            assert!(
                !stderr.contains("Axiom stack trace"),
                "unexpected stderr for debug={debug}: {stderr}"
            );
        }
    }

    #[test]
    fn stage1_project_rejects_unknown_stdlib_module() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-unknown");
        create_project(&project, Some("stdlib-unknown")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/bogus.ax\"\nprint 1\n",
        )
        .expect("write source");

        let err = check_project(&project).expect_err("expected unknown stdlib module error");
        assert!(
            err.message.contains("unknown stdlib module"),
            "unexpected diagnostic: {err:?}",
        );
    }

    #[test]
    fn stage1_project_suggests_similar_stdlib_module() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("stdlib-suggestion");
        create_project(&project, Some("stdlib-suggestion-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "import \"std/tmie.ax\"\nprint \"skip\"\n",
        )
        .expect("write source");

        let err = check_project(&project).expect_err("expected unknown stdlib module error");
        assert!(err.message.contains("unknown stdlib module"));
        assert!(err.message.contains("did you mean \"time.ax\"?"));
        assert_eq!(err.kind, "import");
    }

    #[test]
    fn manifest_parses_test_targets() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("tests");
        create_project(&project, Some("tests-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            format!(
                "{}\n[[tests]]\nname = \"math-smoke\"\nentry = \"src/math_test.ax\"\nstdout = \"42\\n\"\n",
                render_manifest("tests-app")
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&project).expect("load manifest");
        assert_eq!(
            manifest.tests,
            vec![TestTarget {
                name: String::from("math-smoke"),
                entry: String::from("src/math_test.ax"),
                stdout: Some(String::from("42\n")),
                kind: TestKind::Unit,
            }]
        );
    }

    #[test]
    fn manifest_parses_richer_test_kinds() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("typed-tests");
        create_project(&project, Some("typed-tests-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            format!(
                "{}\n[[tests]]\nname = \"json-table\"\nentry = \"src/main_test.ax\"\nkind = \"table\"\nstdout = \"0\\n\"\n",
                render_manifest("typed-tests-app")
            ),
        )
        .expect("write manifest");

        let manifest = load_manifest(&project).expect("load manifest");
        assert_eq!(manifest.tests[0].kind, TestKind::Table);
    }

    #[test]
    fn run_project_tests_executes_manifest_cases() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("runner");
        create_project(&project, Some("runner-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            format!(
                "{}\n[[tests]]\nname = \"math-smoke\"\nentry = \"src/math_test.ax\"\nstdout = \"42\\n\"\n",
                render_manifest("runner-app")
            ),
        )
        .expect("write manifest");
        fs::write(
            project.join("src/math.ax"),
            "pub fn lucky(base: int): int {\nreturn base + 2\n}\n",
        )
        .expect("write module");
        fs::write(
            project.join("src/math_test.ax"),
            "import \"math.ax\"\nprint lucky(40)\n",
        )
        .expect("write test");

        let output = run_project_tests(&project).expect("run tests");
        assert_eq!(output.passed, 2);
        assert_eq!(output.failed, 0);
        assert_eq!(output.cases.len(), 2);
        let math_case = output
            .cases
            .iter()
            .find(|case| case.name == "math-smoke")
            .expect("math case");
        assert_eq!(math_case.stdout, "42\n");
        assert!(math_case.ok);
        assert!(
            output
                .cases
                .iter()
                .any(|case| case.entry == "src/main_test.ax")
        );
    }

    #[test]
    fn run_project_tests_uses_package_expected_output_for_manifest_cases() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("runner-package-golden");
        create_project(&project, Some("runner-package-golden-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            format!(
                "{}\n[[tests]]\nname = \"math-smoke\"\nentry = \"src/math_test.ax\"\n",
                render_manifest("runner-package-golden-app")
            ),
        )
        .expect("write manifest");
        fs::write(project.join("src/math_test.ax"), "print 42\n").expect("write test");
        fs::write(project.join("expected-output.txt"), "42\n").expect("write package golden");

        let output = run_project_tests(&project).expect("run tests");
        assert_eq!(output.passed, 2);
        assert_eq!(output.failed, 0);
        let math_case = output
            .cases
            .iter()
            .find(|case| case.name == "math-smoke")
            .expect("math case");
        assert_eq!(math_case.expected_stdout.as_deref(), Some("42\n"));
        assert!(math_case.ok);
    }

    #[test]
    fn run_project_tests_reports_stdout_mismatch() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("runner-fail");
        create_project(&project, Some("runner-fail-app")).expect("create project");
        fs::write(
            project.join("axiom.toml"),
            format!(
                "{}\n[[tests]]\nname = \"math-smoke\"\nentry = \"src/math_test.ax\"\nstdout = \"99\\n\"\n",
                render_manifest("runner-fail-app")
            ),
        )
        .expect("write manifest");
        fs::write(
            project.join("src/math.ax"),
            "pub fn lucky(base: int): int {\nreturn base + 2\n}\n",
        )
        .expect("write module");
        fs::write(
            project.join("src/math_test.ax"),
            "import \"math.ax\"\nprint lucky(40)\n",
        )
        .expect("write test");

        let output = run_project_tests(&project).expect("run tests");
        assert_eq!(output.passed, 1);
        assert_eq!(output.failed, 1);
        let math_case = output
            .cases
            .iter()
            .find(|case| case.name == "math-smoke")
            .expect("math case");
        assert_eq!(math_case.stdout, "42\n");
        assert!(!math_case.ok);
        assert!(
            math_case
                .error
                .as_ref()
                .expect("error")
                .message
                .contains("expected \"99\\n\", got \"42\\n\"")
        );
    }

    #[test]
    fn run_project_tests_supports_builtin_assertions() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("runner-assertions");
        create_project(&project, Some("runner-assertions-app")).expect("create project");
        fs::write(
            project.join("src/main_test.ax"),
            "let eq_ok: int = assert_eq(40 + 2, 42)\nlet true_ok: int = assert_true(42 == 42)\nlet ne_ok: int = assert_ne(\"alpha\", \"beta\")\nlet contains_ok: int = assert_contains(\"axiom stage1\", \"stage1\")\nprint eq_ok + true_ok + ne_ok + contains_ok\n",
        )
        .expect("write assertion test");
        fs::write(project.join("src/main_test.stdout"), "0\n").expect("write assertion golden");

        let output = run_project_tests(&project).expect("run tests");
        assert_eq!(output.passed, 1);
        assert_eq!(output.failed, 0);
        assert_eq!(output.skipped, 0);
        let case = output.cases.first().expect("test case");
        assert_eq!(case.stdout, "0\n");
        assert!(case.ok);
    }

    #[test]
    fn run_project_tests_supports_std_testing_helpers() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("runner-std-testing");
        create_project(&project, Some("runner-std-testing-app")).expect("create project");
        fs::write(
            project.join("src/main_test.ax"),
            "import \"std/testing.ax\"\nlet int_case: int = table_int(\"double two\", 2 + 2, 4)\nlet bool_case: int = table_bool(\"bool equality\", true, true)\nlet string_case: int = table_string(\"greeting\", \"hello\" + \" world\", \"hello world\")\nlet property_case: int = property(\"addition identity\", 40 + 2 == 42)\nlet snapshot_case: int = snapshot(\"json line\", \"{\\\"ok\\\":true}\", \"{\\\"ok\\\":true}\")\nprint int_case + bool_case + string_case + property_case + snapshot_case\n",
        )
        .expect("write std testing test");
        fs::write(project.join("src/main_test.stdout"), "0\n").expect("write golden");

        let output = run_project_tests(&project).expect("run tests");
        assert_eq!(output.passed, 1);
        assert_eq!(output.failed, 0);
        let case = output.cases.first().expect("test case");
        assert_eq!(case.stdout, "0\n");
        assert!(case.ok);
    }

    #[test]
    fn run_project_tests_reports_assertion_failure_details() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("runner-assertion-fail");
        create_project(&project, Some("runner-assertion-fail-app")).expect("create project");
        fs::write(
            project.join("src/main_test.ax"),
            "let failed: int = assert_eq(41, 42)\nprint failed\n",
        )
        .expect("write failing assertion test");
        fs::remove_file(project.join("src/main_test.stdout")).expect("remove default golden");

        let output = run_project_tests(&project).expect("run tests");
        assert_eq!(output.passed, 0);
        assert_eq!(output.failed, 1);
        assert_eq!(output.skipped, 0);
        let case = output.cases.first().expect("test case");
        assert!(!case.ok);
        assert!(case.stderr.contains(
            "{\"kind\":\"assertion\",\"message\":\"expected left == right, left=41, right=42\"}"
        ));
        assert!(!case.stderr.contains("1:14"));
        assert!(
            case.error
                .as_ref()
                .expect("error")
                .message
                .contains("expected left == right, left=41, right=42")
        );
    }

    #[test]
    fn run_project_tests_discovers_src_suffix_cases() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("runner-discovery");
        create_project(&project, Some("runner-discovery-app")).expect("create project");
        fs::write(
            project.join("src/math.ax"),
            "pub fn lucky(base: int): int {\nreturn base + 2\n}\n",
        )
        .expect("write module");
        fs::write(
            project.join("src/math_test.ax"),
            "import \"math.ax\"\nprint lucky(40)\n",
        )
        .expect("write test");
        fs::write(project.join("src/math_test.stdout"), "42\n").expect("write golden");

        let output = run_project_tests(&project).expect("run tests");
        assert_eq!(output.passed, 2);
        assert_eq!(output.failed, 0);
        assert_eq!(output.cases.len(), 2);
        assert!(
            output
                .cases
                .iter()
                .any(|case| case.entry == "src/main_test.ax")
        );
        let math_case = output
            .cases
            .iter()
            .find(|case| case.entry == "src/math_test.ax")
            .expect("math test");
        assert_eq!(math_case.stdout, "42\n");
        assert!(math_case.ok);
    }

    #[test]
    fn run_project_tests_classifies_richer_fixture_kinds() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("runner-rich-discovery");
        create_project(&project, Some("runner-rich-discovery-app")).expect("create project");
        fs::write(
            project.join("src/cases_table_test.ax"),
            "let ok: int = assert_eq(40 + 2, 42)\nprint ok\n",
        )
        .expect("write table test");
        fs::write(project.join("src/cases_table_test.stdout"), "0\n").expect("write table golden");
        fs::write(
            project.join("src/roundtrip_property.ax"),
            "let ok: int = assert_true(42 == 42)\nprint ok\n",
        )
        .expect("write property test");
        fs::write(project.join("src/roundtrip_property.stdout"), "0\n")
            .expect("write property golden");
        fs::write(
            project.join("src/output_snapshot_test.ax"),
            "print \"snapshot\"\n",
        )
        .expect("write snapshot test");
        fs::write(
            project.join("src/output_snapshot_test.stdout"),
            "snapshot\n",
        )
        .expect("write snapshot golden");

        let output = run_project_tests(&project).expect("run tests");
        assert_eq!(output.failed, 0);
        assert_eq!(output.kinds.get(&TestKind::Table), Some(&1));
        assert_eq!(output.kinds.get(&TestKind::Property), Some(&1));
        assert_eq!(output.kinds.get(&TestKind::Snapshot), Some(&1));
    }

    #[test]
    fn run_project_tests_can_include_benchmark_smoke_fixtures() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("runner-benchmark-discovery");
        create_project(&project, Some("runner-benchmark-discovery-app")).expect("create project");
        fs::write(project.join("src/compute_bench.ax"), "print \"bench\"\n")
            .expect("write benchmark");

        let default_output = run_project_tests(&project).expect("run default tests");
        assert_eq!(default_output.kinds.get(&TestKind::Benchmark), None);

        let output = run_project_tests_with_options(
            &project,
            &TestOptions {
                filter: None,
                package: None,
                include_benchmarks: true,
            },
        )
        .expect("run benchmark smoke tests");
        assert_eq!(output.failed, 0);
        assert_eq!(output.kinds.get(&TestKind::Benchmark), Some(&1));
    }

    #[test]
    fn check_project_rejects_use_after_string_move() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("moves");
        create_project(&project, Some("moves-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let greeting: string = \"hello\"\nlet alias: string = greeting\nprint alias\nprint greeting\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("use after move should fail");
        assert!(error.message.contains("use of moved value"));
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn ownership_compile_fail_corpus_reports_stable_codes() {
        let cases = [
            ("use_after_move", "use_after_move", "use of moved value"),
            (
                "borrow_return_requires_param_origin",
                "borrow_return_requires_param_origin",
                "returning borrowed values requires data derived from one of the borrowed parameters",
            ),
            (
                "generic_borrow_return_requires_param_origin",
                "borrow_return_requires_param_origin",
                "returning borrowed values requires data derived from one of the borrowed parameters",
            ),
            (
                "mutable_borrow_while_shared_live",
                "mutable_borrow_while_shared_live",
                "cannot create mutable borrow of value",
            ),
            (
                "loop_move_outer_non_copy",
                "loop_move_outer_non_copy",
                "cannot move non-copy value",
            ),
        ];

        for (case, code, message) in cases {
            let project = ownership_failure_fixture(case);
            let error = check_project(&project)
                .expect_err(&format!("ownership fixture {case} should fail"));
            assert_eq!(error.kind, "ownership", "fixture {case}");
            assert_eq!(error.code.as_deref(), Some(code), "fixture {case}");
            assert!(
                error.message.contains(message),
                "fixture {case}: {:?}",
                error.message
            );
        }
    }

    #[test]
    fn checked_in_proof_workload_examples_build_run_and_test() {
        for example in ["proof_cli", "proof_worker", "proof_http_service"] {
            let project = checked_in_example_fixture(example);
            check_project(&project).expect("check checked-in proof workload example");

            let built = build_project(&project).expect("build checked-in proof workload example");
            let output = compiled_binary_command(&built.binary)
                .output()
                .expect("run checked-in proof workload example");
            let expected = fs::read_to_string(project.join("src/main_test.stdout"))
                .expect("read expected stdout");
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                expected,
                "example {example}"
            );

            let tests =
                run_project_tests(&project).expect("test checked-in proof workload example");
            let expected_passed = match example {
                "proof_cli" => 2,
                _ => 1,
            };
            assert_eq!(tests.passed, expected_passed, "example {example}");
            assert_eq!(tests.failed, 0, "example {example}");
        }
    }

    #[test]
    fn conformance_corpus_reports_stable_results() {
        let output =
            run_project_tests(&conformance_fixture()).expect("run stage1 conformance corpus");
        assert_eq!(output.cases.len(), 25);
        assert_eq!(output.passed, 25);
        assert_eq!(output.failed, 0);
        assert!(
            output
                .cases
                .iter()
                .filter(|case| case.expected_error.is_some())
                .count()
                == 18
        );
        assert_eq!(
            output
                .cases
                .iter()
                .filter(|case| case.expected_stdout.is_some())
                .count(),
            7
        );
    }

    #[test]
    fn check_project_rejects_branch_move_followed_by_outer_use() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("branch-moves");
        create_project(&project, Some("branch-moves-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let greeting: string = \"hello\"\nlet ready: bool = true\nif ready {\nlet alias: string = greeting\nprint alias\n} else {\nprint \"skip\"\n}\nprint greeting\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("conditional move should fail");
        assert!(error.message.contains("use of moved value"));
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_allows_copy_reuse_after_binding() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("copy");
        create_project(&project, Some("copy-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let count: int = 21\nlet duplicate: int = count\nprint count + duplicate\n",
        )
        .expect("write source");
        let output = check_project(&project).expect("copy values should be reusable");
        assert_eq!(output.statement_count, 3);
    }

    #[test]
    fn check_project_rejects_type_mismatch() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("types");
        create_project(&project, Some("types-app")).expect("create project");
        fs::write(project.join("src/main.ax"), "let count: int = \"nope\"\n")
            .expect("write source");
        let error = check_project(&project).expect_err("type mismatch should fail");
        assert!(error.message.contains("expects int, got string"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_infers_generic_call_from_argument_type() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("generic-inferred-arg");
        create_project(&project, Some("generic-inferred-arg-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn identity<T>(value: T): T {\nreturn value\n}\n\nprint identity(42)\n",
        )
        .expect("write source");
        let output = check_project(&project).expect("generic call should infer type args");
        assert_eq!(output.statement_count, 2);
    }

    #[test]
    fn parser_lowers_inferred_generic_calls_to_monomorphized_copies() {
        let source = "fn identity<T>(value: T): T {\nreturn value\n}\n\nlet answer: int = identity(42)\nlet label: string = identity<string>(\"stage1\")\nprint answer\nprint label\n";
        let parsed = parse_program(source, Path::new("main.ax")).expect("parse");
        let hir = hir::lower(&parsed).expect("lower");
        let mir = mir::lower(&hir);
        let rendered = render_rust(&mir);
        assert!(rendered.contains("fn identity__int(value: i64) -> i64 {"));
        assert!(rendered.contains("fn identity__string(value: String) -> String {"));
        assert!(rendered.contains("let answer: i64 = identity__int(42);"));
        assert!(
            rendered.contains("let label: String = identity__string(String::from(\"stage1\"));")
        );
    }

    #[test]
    fn check_project_infers_generic_call_from_return_context() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("generic-inferred-return");
        create_project(&project, Some("generic-inferred-return-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn none<T>(): Option<T> {\nreturn None\n}\n\nlet missing: Option<int> = none()\n",
        )
        .expect("write source");
        let output = check_project(&project).expect("generic call should infer from expected type");
        assert_eq!(output.statement_count, 2);
    }

    #[test]
    fn check_project_reports_generic_inference_constraint_mismatch() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("generic-inference-mismatch");
        create_project(&project, Some("generic-inference-mismatch-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn first<T>(values: [T]): T {\nreturn values[0]\n}\n\nprint first(42)\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("argument constraint should fail");
        assert!(error.message.contains("argument 1 constraint failed"));
        assert!(error.message.contains("expected generic constraint"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_unconstrained_generic_type_param() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("generic-unconstrained");
        create_project(&project, Some("generic-unconstrained-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn bad<T>(value: int): int {\nreturn value\n}\n\nprint bad<int>(42)\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("unconstrained type params should fail");
        assert!(error.message.contains("unconstrained type parameter"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_generic_instantiation_type_mismatch() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("generic-type-mismatch");
        create_project(&project, Some("generic-type-mismatch-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn identity<T>(value: T): T {\nreturn value\n}\n\nprint identity<int>(\"nope\")\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("instantiated argument type should fail");
        assert!(
            error
                .message
                .contains("expects argument type int, got string")
        );
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_generic_wrapper_instantiation_type_mismatch() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("generic-wrapper-type-mismatch");
        create_project(&project, Some("generic-wrapper-type-mismatch-app"))
            .expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct Window<T> {\nview: &[T]\n}\n\nlet values: [int] = [1, 2, 3]\nlet window: Window<string> = Window { view: values[:] }\nprint len(window.view)\n",
        )
        .expect("write source");
        let error = check_project(&project)
            .expect_err("borrowed generic wrapper instantiation should enforce type args");
        assert!(error.message.contains("field \"view\" expects &[string]"));
        assert!(error.message.contains("got &[int]"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_unconstrained_borrowed_generic_wrapper_type_param() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("generic-borrow-wrapper-unconstrained");
        create_project(&project, Some("generic-borrow-wrapper-unconstrained-app"))
            .expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct Window<T> {\nview: &[int]\n}\n\nlet values: [int] = [1, 2, 3]\nlet window: Window<string> = Window { view: values[:] }\nprint len(window.view)\n",
        )
        .expect("write source");
        let error = check_project(&project)
            .expect_err("borrowed generic wrappers should constrain their type params");
        assert!(error.message.contains("unconstrained type parameter"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn build_project_emits_native_binary_from_generic_structs_and_enums() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("generic-aggregates");
        create_project(&project, Some("generic-aggregates-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct Window<T> {\nview: &[T]\n}\n\nstruct MaybeBox<T> {\nitem: Option<T>\n}\n\nstruct ResultBox<T, E> {\nitem: Result<T, E>\n}\n\nstruct Buckets<T> {\nitems: [T]\nby_name: {string: T}\n}\n\nenum Slot<T> {\nFilled(T)\nEmpty\n}\n\nfn tail<T>(values: &[T]): &[T] {\nreturn values[1:]\n}\n\nfn make_window<T>(values: &[T]): Window<T> {\nreturn Window { view: tail<T>(values) }\n}\n\nlet values: [int] = [4, 5, 6]\nlet window: Window<int> = Window { view: values[:] }\nprint len(window.view)\nlet tail_window: Window<int> = make_window<int>(values[:])\nprint len(tail_window.view)\nlet maybe: MaybeBox<int> = MaybeBox { item: Some(8) }\nmatch maybe.item {\nSome(value) {\nprint value\n}\nNone {\nprint 0\n}\n}\nlet result: ResultBox<string, string> = ResultBox { item: Ok(\"ready\") }\nmatch result.item {\nOk(value) {\nprint value\n}\nErr(error) {\nprint error\n}\n}\nlet bucket_values: [int] = [10, 20]\nlet bucket_lookup: {string: int} = {\"answer\": 42}\nlet buckets: Buckets<int> = Buckets { items: bucket_values, by_name: bucket_lookup }\nprint len(buckets.items)\nlet answers: {string: int} = buckets.by_name\nprint answers[\"answer\"]\nlet number: Slot<int> = Filled(42)\nmatch number {\nFilled(value) {\nprint value\n}\nEmpty {\nprint 0\n}\n}\nlet text: Slot<string> = Filled(\"done\")\nmatch text {\nFilled(value) {\nprint value\n}\nEmpty {\nprint \"empty\"\n}\n}\n",
        )
        .expect("write source");

        let built = build_project(&project).expect("build generic aggregates");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "3\n2\n8\nready\n2\n42\n42\ndone\n"
        );
    }

    #[test]
    fn check_project_rejects_non_bool_if_condition() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("if-types");
        create_project(&project, Some("if-types-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let answer: int = 42\nif answer {\nprint answer\n}\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("if condition should require bool");
        assert!(error.message.contains("if condition expects bool, got int"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_none_without_expected_option_context() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("none-context");
        create_project(&project, Some("none-context-app")).expect("create project");
        fs::write(project.join("src/main.ax"), "print None\n").expect("write source");
        let error = check_project(&project).expect_err("None should require an expected type");
        assert!(
            error
                .message
                .contains("None requires an expected Option<T> context")
        );
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_ok_without_expected_result_context() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("ok-context");
        create_project(&project, Some("ok-context-app")).expect("create project");
        fs::write(project.join("src/main.ax"), "print Ok(7)\n").expect("write source");
        let error = check_project(&project).expect_err("Ok should require an expected type");
        assert!(
            error
                .message
                .contains("Ok requires an expected Result<T, E> context")
        );
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_option_payload_type_mismatch() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("option-mismatch");
        create_project(&project, Some("option-mismatch-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let value: Option<int> = Some(\"nope\")\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("option payload mismatch should fail");
        assert!(
            error
                .message
                .contains("Option::Some expects payload type int, got string")
        );
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_result_payload_type_mismatch() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("result-mismatch");
        create_project(&project, Some("result-mismatch-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let value: Result<int, string> = Err(7)\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("result payload mismatch should fail");
        assert!(
            error
                .message
                .contains("Result::Err expects payload type string, got int")
        );
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_non_exhaustive_option_match() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("option-match");
        create_project(&project, Some("option-match-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn unwrap(value: Option<int>): int {\nmatch value {\nSome(count) {\nreturn count\n}\n}\n}\n\nprint 0\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("non-exhaustive option match should fail");
        assert!(error.message.contains("not exhaustive"));
        assert!(error.message.contains("None"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_return_outside_function() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("return-top");
        create_project(&project, Some("return-top-app")).expect("create project");
        fs::write(project.join("src/main.ax"), "return 42\n").expect("write source");
        let error = check_project(&project).expect_err("top-level return should fail");
        assert!(
            error
                .message
                .contains("return is only valid inside a function")
        );
        assert_eq!(error.kind, "control");
    }

    #[test]
    fn check_project_rejects_undefined_function_call() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("missing-call");
        create_project(&project, Some("missing-call-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let answer: int = lucky(40)\nprint answer\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("missing function should fail");
        assert!(error.message.contains("undefined function"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_suggests_similar_local_for_undefined_variable() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("missing-variable-suggestion");
        create_project(&project, Some("missing-variable-suggestion-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let answer: int = 42\nprint anwser\n",
        )
        .expect("write source");

        let error = check_project(&project).expect_err("missing variable should fail");
        assert!(error.message.contains("undefined variable \"anwser\""));
        assert!(error.message.contains("did you mean \"answer\"?"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_wrong_function_arity() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("arity");
        create_project(&project, Some("arity-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn lucky(base: int): int {\nreturn base + 2\n}\n\nlet answer: int = lucky()\nprint answer\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("wrong arity should fail");
        assert!(error.message.contains("expects 1 arguments, got 0"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_function_return_mismatch() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("return-mismatch");
        create_project(&project, Some("return-mismatch-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn bad(): int {\nreturn \"nope\"\n}\n\nprint \"skip\"\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("return mismatch should fail");
        assert!(error.message.contains("return expects int, got string"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_try_without_option_or_result_return() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("try-return-mismatch");
        create_project(&project, Some("try-return-mismatch-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn maybe_label(): Option<string> {\nreturn Some(\"ready\")\n}\n\nfn bad(): int {\nlet label: string = maybe_label()?\nreturn 0\n}\n\nprint 0\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("type error");
        assert_eq!(error.kind, "type");
        assert!(
            error
                .message
                .contains("`?` on Option<T> requires the enclosing function to return Option<_>")
        );
    }

    #[test]
    fn check_project_rejects_try_result_error_type_mismatch() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("try-result-error-mismatch");
        create_project(&project, Some("try-result-error-mismatch-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn load(): Result<int, string> {\nreturn Err(\"boom\")\n}\n\nfn bad(): Result<int, int> {\nlet count: int = load()?\nreturn Ok(count)\n}\n\nprint 0\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("type error");
        assert_eq!(error.kind, "type");
        assert!(
            error
                .message
                .contains("`?` cannot propagate Result error type string")
        );
    }

    #[test]
    fn check_project_rejects_missing_function_return() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("missing-return");
        create_project(&project, Some("missing-return-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn maybe(value: bool): int {\nif value {\nreturn 1\n}\n}\n\nprint \"skip\"\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("missing return should fail");
        assert!(error.message.contains("does not return along all paths"));
        assert_eq!(error.kind, "control");
    }

    #[test]
    fn check_project_accepts_panic_as_terminating_branch() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("panic-terminates-branch");
        create_project(&project, Some("panic-terminates-branch-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn require(flag: bool): int {\nif flag {\nreturn 7\n} else {\npanic(\"boom\")\n}\n}\n\nprint require(true)\n",
        )
        .expect("write source");
        check_project(&project).expect("panic branch should count as terminating control flow");
    }

    #[test]
    fn check_project_accepts_panic_as_terminating_match_arm() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("panic-terminates-match-arm");
        create_project(&project, Some("panic-terminates-match-arm-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum Status {\nReady\nFailed\n}\n\nfn require(status: Status): int {\nmatch status {\nReady {\nreturn 7\n}\nFailed {\npanic(\"boom\")\n}\n}\n}\n\nprint require(Ready)\n",
        )
        .expect("write source");
        check_project(&project).expect("panic match arm should count as terminating control flow");
    }

    #[test]
    fn check_project_rejects_unreachable_statement_after_panic() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("panic-unreachable");
        create_project(&project, Some("panic-unreachable-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn fail(): int {\npanic(\"boom\")\nprint 1\n}\n\nprint 0\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("unreachable statement should fail");
        assert_eq!(error.kind, "control");
        assert!(
            error
                .message
                .contains("unreachable statements after a terminating control-flow statement")
        );
    }

    #[test]
    fn build_project_emits_native_binary_from_imported_modules() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("modules");
        create_project(&project, Some("modules-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "import \"greetings.ax\"\nimport \"math.ax\"\n\nfn is_ready(value: int): bool {\nreturn value == 42\n}\n\nlet answer: int = lucky(40)\nlet ready: bool = is_ready(answer)\nif ready {\nprint banner(\"from modules\")\n} else {\nprint \"bad\"\n}\nprint answer\nprint ready\n",
        )
        .expect("write main");
        fs::write(
            project.join("src/greetings.ax"),
            "pub fn banner(name: string): string {\nreturn prefix() + name\n}\n\nfn prefix(): string {\nreturn \"hello \"\n}\n",
        )
        .expect("write greetings");
        fs::write(
            project.join("src/math.ax"),
            "pub fn lucky(base: int): int {\nreturn bump(base)\n}\n\nfn bump(base: int): int {\nreturn base + 2\n}\n",
        )
        .expect("write math");
        let built = build_project(&project).expect("build imported modules");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "hello from modules\n42\ntrue\n"
        );
    }

    #[test]
    fn build_project_emits_native_binary_with_local_type_aliases() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("local-type-alias");
        create_project(&project, Some("local-type-alias-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "type Id = int\ntype Labels = [string]\n\nfn echo(value: Id): Id {\nreturn value\n}\n\nlet answer: Id = echo(42)\nlet labels: Labels = [\"alpha\", \"beta\"]\nprint answer\nprint len(labels)\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project with local type aliases");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n2\n");
    }

    #[test]
    fn build_project_emits_native_binary_with_imported_public_type_aliases() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("public-type-alias");
        create_project(&project, Some("public-type-alias-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "import \"types.ax\"\n\nlet answer: Id = 42\nlet labels: Labels = [\"alpha\", \"beta\"]\nprint answer\nprint len(labels)\n",
        )
        .expect("write main");
        fs::write(
            project.join("src/types.ax"),
            "pub type Id = int\npub type Label = string\npub type Labels = [Label]\n",
        )
        .expect("write types");
        let built = build_project(&project).expect("build project with imported type aliases");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n2\n");
    }

    #[test]
    fn build_project_emits_native_binary_with_local_consts() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("local-consts");
        create_project(&project, Some("local-consts-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "const ANSWER: int = 40 + 2\nconst READY: bool = ANSWER == 42\nconst LABEL: string = \"stage\" + \"1\"\nprint ANSWER\nprint READY\nprint LABEL\n",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project with local consts");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "42\ntrue\nstage1\n"
        );
    }

    #[test]
    fn build_project_emits_native_binary_with_imported_public_consts() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("public-consts");
        create_project(&project, Some("public-consts-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "import \"values.ax\"\nprint ANSWER\nprint READY\nprint LABEL\n",
        )
        .expect("write main");
        fs::write(
            project.join("src/values.ax"),
            "pub const ANSWER: int = 40 + 2\npub const READY: bool = ANSWER == 42\npub const LABEL: string = \"stage\" + \"1\"\n",
        )
        .expect("write values");
        let built = build_project(&project).expect("build project with imported consts");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "42\ntrue\nstage1\n"
        );
    }

    #[test]
    fn check_project_rejects_missing_import() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("missing-import");
        create_project(&project, Some("missing-import-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "import \"nope.ax\"\nprint \"skip\"\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("missing import should fail");
        assert!(error.message.contains("missing import"));
        assert_eq!(error.kind, "import");
    }

    #[test]
    fn check_project_rejects_import_aliases_explicitly() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("import-alias");
        create_project(&project, Some("import-alias-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "import \"math.ax\" as math\nprint \"skip\"\n",
        )
        .expect("write source");
        fs::write(
            project.join("src/math.ax"),
            "pub fn answer(): int {\nreturn 42\n}\n",
        )
        .expect("write module");

        let error = check_project(&project).expect_err("import aliases should fail");
        assert_eq!(error.kind, "parse");
        assert!(error.message.contains("does not support import aliases"));
    }

    #[test]
    fn check_project_rejects_re_exports_explicitly() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("re-export");
        create_project(&project, Some("re-export-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "pub use \"math.ax\"\nprint \"skip\"\n",
        )
        .expect("write source");

        let error = check_project(&project).expect_err("re-exports should fail");
        assert_eq!(error.kind, "parse");
        assert!(error.message.contains("does not support re-exports"));
    }

    #[test]
    fn check_project_rejects_package_re_exports_explicitly() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("package-re-export");
        create_project(&project, Some("package-re-export-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "pub(pkg) use \"math.ax\"\nprint \"skip\"\n",
        )
        .expect("write source");

        let error = check_project(&project).expect_err("package re-exports should fail");
        assert_eq!(error.kind, "parse");
        assert!(error.message.contains("does not support re-exports"));
    }

    #[test]
    fn check_project_rejects_package_import_re_exports_explicitly() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("package-import-re-export");
        create_project(&project, Some("package-import-re-export-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "pub(pkg) import \"math.ax\"\nprint \"skip\"\n",
        )
        .expect("write source");

        let error = check_project(&project).expect_err("package import re-exports should fail");
        assert_eq!(error.kind, "parse");
        assert!(error.message.contains("does not support re-exports"));
    }

    #[test]
    fn check_project_rejects_namespace_qualified_calls_explicitly() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("qualified-call");
        create_project(&project, Some("qualified-call-app")).expect("create project");
        fs::write(project.join("src/main.ax"), "print math.answer()\n").expect("write source");

        let error = check_project(&project).expect_err("qualified calls should fail");
        assert_eq!(error.kind, "type");
        assert!(error.message.contains("undefined variable \"math\""));
    }

    #[test]
    fn check_project_rejects_private_imported_type_alias() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("private-type-alias");
        create_project(&project, Some("private-type-alias-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "import \"types.ax\"\nlet answer: Hidden = 42\nprint answer\n",
        )
        .expect("write main");
        fs::write(project.join("src/types.ax"), "type Hidden = int\n").expect("write types");
        let error = check_project(&project).expect_err("private type alias should fail");
        assert!(error.message.contains("is not visible from this module"));
        assert_eq!(error.kind, "import");
    }

    #[test]
    fn check_project_rejects_recursive_type_alias() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("recursive-type-alias");
        create_project(&project, Some("recursive-type-alias-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "type Loop = Loop\nlet value: Loop = 42\nprint value\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("recursive type alias should fail");
        assert!(error.message.contains("is recursive"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_mutually_recursive_structs_without_indirection() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("mutually-recursive-structs");
        create_project(&project, Some("mutually-recursive-structs-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct Node {\nnext: Link\n}\n\nstruct Link {\nnode: Node\n}\n\nprint 0\n",
        )
        .expect("write source");

        let error = check_project(&project).expect_err("mutually recursive structs should fail");
        assert!(error.message.contains("requires indirection"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_mutually_recursive_struct_enum_without_indirection() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("mutually-recursive-struct-enum");
        create_project(&project, Some("mutually-recursive-struct-enum-app"))
            .expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct ExprNode {\nexpr: Expr\n}\n\nenum Expr {\nWrap(ExprNode)\nLit(int)\n}\n\nprint 0\n",
        )
        .expect("write source");

        let error =
            check_project(&project).expect_err("mutually recursive struct and enum should fail");
        assert!(error.message.contains("requires indirection"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_recursive_enum_without_indirection() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("recursive-enum");
        create_project(&project, Some("recursive-enum-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum List {\nCons(List)\nNil\n}\n\nprint 0\n",
        )
        .expect("write source");

        let error = check_project(&project).expect_err("recursive enum should fail");
        assert!(error.message.contains("requires indirection"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn build_project_allows_recursive_struct_through_array_indirection() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("recursive-struct-array");
        create_project(&project, Some("recursive-struct-array-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct Node {\nchildren: [Node]\n}\n\nprint 0\n",
        )
        .expect("write source");

        build_project(&project).expect("recursive struct through array indirection should build");
    }

    #[test]
    fn check_project_rejects_recursive_const() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("recursive-const");
        create_project(&project, Some("recursive-const-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "const LOOP: int = LOOP\nprint LOOP\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("recursive const should fail");
        assert!(error.message.contains("recursive const"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_const_type_mismatch() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("const-type-mismatch");
        create_project(&project, Some("const-type-mismatch-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "const ANSWER: bool = 42\nprint ANSWER\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("const type mismatch should fail");
        assert!(error.message.contains("expects bool, got int"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_type_alias_inside_function_block() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("block-type-alias");
        create_project(&project, Some("block-type-alias-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn main(): int {\ntype Id = int\nreturn 42\n}\n\nprint main()\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("block type alias should fail");
        assert!(
            error
                .message
                .contains("only supports top-level type alias declarations")
        );
        assert_eq!(error.kind, "parse");
    }

    #[test]
    fn check_project_rejects_const_inside_function_block() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("block-const");
        create_project(&project, Some("block-const-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn main(): int {\nconst ANSWER: int = 42\nreturn ANSWER\n}\n\nprint main()\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("block const should fail");
        assert!(
            error
                .message
                .contains("only supports top-level const declarations")
        );
        assert_eq!(error.kind, "parse");
    }

    #[test]
    fn check_project_rejects_private_import_call() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("private-import");
        create_project(&project, Some("private-import-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "import \"greetings.ax\"\nprint prefix()\n",
        )
        .expect("write main");
        fs::write(
            project.join("src/greetings.ax"),
            "pub fn banner(name: string): string {\nreturn prefix() + name\n}\n\nfn prefix(): string {\nreturn \"hello \"\n}\n",
        )
        .expect("write greetings");
        let error = check_project(&project).expect_err("private import should fail");
        assert!(error.message.contains("is not visible from this module"));
        assert_eq!(error.kind, "import");
    }

    #[test]
    fn check_project_rejects_imported_top_level_statements() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("bad-module");
        create_project(&project, Some("bad-module-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "import \"greetings.ax\"\nprint banner(\"x\")\n",
        )
        .expect("write main");
        fs::write(project.join("src/greetings.ax"), "print \"nope\"\n").expect("write greetings");
        let error = check_project(&project).expect_err("module top-level statements should fail");
        assert!(
            error.message.contains(
                "may only contain imports, const declarations, type alias declarations, struct declarations, enum declarations, and function declarations"
            )
        );
        assert_eq!(error.kind, "import");
    }

    #[test]
    fn check_project_rejects_circular_imports() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("cycle");
        create_project(&project, Some("cycle-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "import \"a.ax\"\nprint \"skip\"\n",
        )
        .expect("write main");
        fs::write(
            project.join("src/a.ax"),
            "import \"b.ax\"\npub fn call_a(): int {\nreturn call_b()\n}\n",
        )
        .expect("write a");
        fs::write(
            project.join("src/b.ax"),
            "import \"a.ax\"\npub fn call_b(): int {\nreturn call_a()\n}\n",
        )
        .expect("write b");
        let error = check_project(&project).expect_err("circular imports should fail");
        assert!(error.message.contains("circular import"));
        assert_eq!(error.kind, "import");
    }

    #[test]
    fn build_project_emits_native_binary_from_imported_public_structs() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("struct-modules");
        create_project(&project, Some("struct-modules-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "import \"model.ax\"\n\nlet info: BuildInfo = BuildInfo { label: \"hello from modules\", count: 42 }\nprint info.count\nprint info.label\n",
        )
        .expect("write main");
        fs::write(
            project.join("src/model.ax"),
            "pub struct BuildInfo {\nlabel: string\ncount: int\n}\n",
        )
        .expect("write model");
        let built = build_project(&project).expect("build imported structs");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "42\nhello from modules\n"
        );
    }

    #[test]
    fn check_project_allows_non_copy_struct_field_move_then_sibling_use() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("struct-partial-move-sibling");
        create_project(&project, Some("struct-partial-move-sibling-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct BuildInfo {\nlabel: string\nsummary: string\ncount: int\n}\n\nlet info: BuildInfo = BuildInfo { label: \"deploy\", summary: \"ready\", count: 7 }\nprint info.label\nprint info.count\nprint info.summary\n",
        )
        .expect("write source");
        check_project(&project).expect("moving one struct field should leave siblings available");
    }

    #[test]
    fn check_project_allows_non_copy_struct_field_move_through_call_then_sibling_use() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("struct-call-partial-move-sibling");
        create_project(&project, Some("struct-call-partial-move-sibling-app"))
            .expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct BuildInfo {\nlabel: string\nsummary: string\ncount: int\n}\n\nfn consume(label: string): string {\nreturn label\n}\n\nlet info: BuildInfo = BuildInfo { label: \"deploy\", summary: \"ready\", count: 7 }\nprint consume(info.label)\nprint info.count\nprint info.summary\n",
        )
        .expect("write source");
        check_project(&project)
            .expect("call lowering should move only the projected struct field argument");
    }

    #[test]
    fn check_project_allows_nested_non_copy_struct_field_move_then_sibling_use() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("nested-struct-partial-move-sibling");
        create_project(&project, Some("nested-struct-partial-move-sibling-app"))
            .expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct Details {\nlabel: string\nsummary: string\n}\n\nstruct BuildInfo {\ndetails: Details\ncount: int\n}\n\nlet info: BuildInfo = BuildInfo { details: Details { label: \"deploy\", summary: \"ready\" }, count: 7 }\nprint info.details.label\nprint info.details.summary\nprint info.count\n",
        )
        .expect("write source");
        check_project(&project)
            .expect("moving a nested struct field should leave nested siblings available");
    }

    #[test]
    fn check_project_rejects_whole_struct_use_after_field_move() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("struct-partial-move-whole-use");
        create_project(&project, Some("struct-partial-move-whole-use-app"))
            .expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct BuildInfo {\nlabel: string\nsummary: string\ncount: int\n}\n\nfn consume(info: BuildInfo): string {\nreturn info.summary\n}\n\nlet info: BuildInfo = BuildInfo { label: \"deploy\", summary: \"ready\", count: 7 }\nprint info.label\nprint consume(info)\n",
        )
        .expect("write source");
        let error = check_project(&project)
            .expect_err("partially moved aggregate should not be usable as a whole value");
        assert!(error.message.contains("use of partially moved value"));
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_reusing_moved_struct_field() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("struct-partial-move-field-reuse");
        create_project(&project, Some("struct-partial-move-field-reuse-app"))
            .expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct BuildInfo {\nlabel: string\nsummary: string\ncount: int\n}\n\nlet info: BuildInfo = BuildInfo { label: \"deploy\", summary: \"ready\", count: 7 }\nprint info.label\nprint info.label\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("moved field should not be reusable");
        assert!(error.message.contains("use of moved value"));
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_allows_non_copy_enum_payload_binding_then_sibling_payload_use() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("enum-payload-sibling-move");
        create_project(&project, Some("enum-payload-sibling-move-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum Message {\nJob { label: string, detail: string }\n}\n\nfn consume(value: string): string {\nreturn value\n}\n\nlet message: Message = Job { label: \"deploy\", detail: \"ready\" }\nmatch message {\nJob { label, detail } {\nprint consume(label)\nprint consume(detail)\n}\n}\n",
        )
        .expect("write source");
        check_project(&project)
            .expect("moving one enum payload binding should leave sibling payloads available");
    }

    #[test]
    fn check_project_rejects_missing_struct_field() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("missing-field");
        create_project(&project, Some("missing-field-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct BuildInfo {\nlabel: string\ncount: int\n}\n\nlet info: BuildInfo = BuildInfo { label: \"x\" }\nprint info.count\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("missing field should fail");
        assert!(error.message.contains("is missing field"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_suggests_similar_struct_field() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("struct-field-suggestion");
        create_project(&project, Some("struct-field-suggestion-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct User {\nname: string\ncount: int\n}\n\nlet user: User = User { name: \"agent\", count: 1 }\nprint user.naem\n",
        )
        .expect("write source");

        let error = check_project(&project).expect_err("unknown field should fail");
        assert!(error.message.contains("has no field \"naem\""));
        assert!(error.message.contains("did you mean \"name\"?"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_field_access_on_non_struct() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("bad-field-access");
        create_project(&project, Some("bad-field-access-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let answer: int = 42\nprint answer.count\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("field access should fail");
        assert!(
            error
                .message
                .contains("field access expects a struct value")
        );
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_mixed_array_literal_types() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("bad-array-literal");
        create_project(&project, Some("bad-array-literal-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [int] = [1, true]\n",
        )
        .expect("write source");
        let error =
            check_project(&project).expect_err("array literal should require matching types");
        assert!(
            error
                .message
                .contains("array literal expects matching element types")
        );
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_array_index_on_non_array() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("bad-array-index");
        create_project(&project, Some("bad-array-index-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let answer: int = 42\nprint answer[0]\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("array index should require array");
        assert!(
            error
                .message
                .contains("index expects an array or map value")
        );
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_non_int_array_index() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("bad-array-index-type");
        create_project(&project, Some("bad-array-index-type-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [int] = [1, 2]\nprint values[true]\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("array index should require int");
        assert!(error.message.contains("array index expects int"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_array_slice_on_non_array() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("slice-non-array");
        create_project(&project, Some("slice-non-array-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let value: int = 42\nprint value[1:2]\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("slicing non-array should fail");
        assert!(
            error
                .message
                .contains("slice expects an array or slice value, got int")
        );
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_non_int_array_slice_bound() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("slice-bound-type");
        create_project(&project, Some("slice-bound-type-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [int] = [1, 2, 3]\nprint values[true:2][0]\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("slice bound should require int");
        assert!(
            error
                .message
                .contains("array slice start expects int, got bool")
        );
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_non_copy_slice_index() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("slice-move");
        create_project(&project, Some("slice-move-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [string] = [\"a\", \"b\", \"c\"]\nlet tail: &[string] = values[1:]\nprint tail[0]\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("non-copy slice indexing should fail");
        assert!(
            error
                .message
                .contains("borrowed slice indexing requires a Copy element type")
        );
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_slice_return_without_borrowed_param() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("slice-return-owned");
        create_project(&project, Some("slice-return-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn tail(values: [int]): &[int] {\nreturn values[1:]\n}\n\nprint 0\n",
        )
        .expect("write source");
        let error =
            check_project(&project).expect_err("slice returns should require a borrowed param");
        assert!(
            error
                .message
                .contains("borrowed return functions must take at least one borrowed parameter")
        );
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_slice_return_from_local_value() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("slice-return-local");
        create_project(&project, Some("slice-return-local-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn tail(values: &[int]): &[int] {\nlet local: [int] = [7, 9, 11]\nreturn local[1:]\n}\n\nprint 0\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("local slice return should fail");
        assert!(error.message.contains(
            "returning borrowed values requires data derived from one of the borrowed parameters"
        ));
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_wrapped_borrow_return_from_local_value() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("wrapped-borrow-return-local");
        create_project(&project, Some("wrapped-borrow-return-local-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn wrap(values: &[int]): Option<&[int]> {\nlet local: [int] = [7, 9, 11]\nreturn Some(local[1:])\n}\n\nprint 0\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("local wrapped borrow return should fail");
        assert!(error.message.contains(
            "returning borrowed values requires data derived from one of the borrowed parameters"
        ));
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_wrapped_borrow_return_without_borrowed_params() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("wrapped-borrow-return-no-param");
        create_project(&project, Some("wrapped-borrow-return-no-param-app"))
            .expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn choose(values: [int]): Option<&[int]> {\nreturn Some(values[1:])\n}\n\nprint 0\n",
        )
        .expect("write source");
        let error = check_project(&project)
            .expect_err("borrowed returns should still require at least one borrowed param");
        assert!(
            error
                .message
                .contains("borrowed return functions must take at least one borrowed parameter")
        );
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_moving_owner_inside_match_while_temporary_borrow_is_live() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("match-temporary-borrow-move");
        create_project(&project, Some("match-temporary-borrow-move-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [string] = [\"alpha\", \"beta\"]\nmatch Some(values[:]) {\nSome(window) {\nprint len(window)\nprint first(values)\n}\nNone {\nprint 0\n}\n}\n",
        )
        .expect("write source");
        let error = check_project(&project)
            .expect_err("temporary match borrow should block owner move inside the arm");
        assert!(error.message.contains("cannot move"));
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_moving_owner_in_later_call_arg_after_temporary_borrow_arg() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("call-arg-temporary-borrow-move");
        create_project(&project, Some("call-arg-temporary-borrow-move-app"))
            .expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn consume(view: Option<&[string]>, values: [string]): string {\nreturn first(values)\n}\n\nlet values: [string] = [\"alpha\", \"beta\"]\nprint consume(Some(values[:]), values)\n",
        )
        .expect("write source");
        let error = check_project(&project)
            .expect_err("temporary borrow in an earlier call argument should block moving the owner later in the call");
        assert!(error.message.contains("cannot move"));
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_borrowing_owner_in_later_call_arg_after_move_arg() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("call-arg-move-then-borrow");
        create_project(&project, Some("call-arg-move-then-borrow-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn consume(values: [string], view: Option<&[string]>): string {\nreturn first(values)\n}\n\nlet values: [string] = [\"alpha\", \"beta\"]\nprint consume(values, Some(values[:]))\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err(
            "moving the owner first should still reject borrowing it later in the call",
        );
        assert!(error.message.contains("use of moved value"));
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_moving_owner_inside_while_while_local_borrow_is_live() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("while-live-borrow-move");
        create_project(&project, Some("while-live-borrow-move-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [string] = [\"alpha\", \"beta\"]\nwhile true {\nlet view: &[string] = values[:]\nprint len(view)\nprint first(values)\n}\n",
        )
        .expect("write source");
        let error = check_project(&project)
            .expect_err("loop-local borrow should block owner move inside the loop body");
        assert!(error.message.contains("cannot move"));
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_first_on_non_copy_slice() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("slice-first-non-copy");
        create_project(&project, Some("slice-first-non-copy-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [string] = [\"a\", \"b\", \"c\"]\nlet tail: &[string] = values[1:]\nprint first(tail)\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("first on non-copy slice should fail");
        assert!(
            error
                .message
                .contains("first requires a Copy element type when called on a borrowed slice")
        );
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_moving_owned_array_while_slice_borrow_is_live() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("live-borrow-move");
        create_project(&project, Some("live-borrow-move-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [string] = [\"alpha\", \"beta\"]\nlet view: &[string] = values[:]\nprint len(view)\nprint first(values)\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("moving a borrowed owner should fail");
        assert!(
            error
                .message
                .contains("cannot move value \"values\" while borrowed slices are still live")
        );
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_mutable_borrow_while_shared_borrow_is_live() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("shared-then-mutable-borrow");
        create_project(&project, Some("shared-then-mutable-borrow-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [int] = [1, 2, 3]\nlet shared: &[int] = values[:]\nlet mutable: &mut [int] = values[:]\nprint len(shared)\nprint len(mutable)\n",
        )
        .expect("write source");
        let error = check_project(&project)
            .expect_err("mutable borrow should fail while a shared borrow is live");
        assert!(error.message.contains(
            "cannot create mutable borrow of value \"values\" while a shared borrow is still live"
        ));
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_shared_borrow_while_mutable_borrow_is_live() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("mutable-then-shared-borrow");
        create_project(&project, Some("mutable-then-shared-borrow-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [int] = [1, 2, 3]\nlet mutable: &mut [int] = values[:]\nlet shared: &[int] = values[:]\nprint len(mutable)\nprint len(shared)\n",
        )
        .expect("write source");
        let error = check_project(&project)
            .expect_err("shared borrow should fail while a mutable borrow is live");
        assert!(error.message.contains(
            "cannot create shared borrow of value \"values\" while a mutable borrow is still live"
        ));
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_double_mutable_borrow() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("double-mutable-borrow");
        create_project(&project, Some("double-mutable-borrow-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [int] = [1, 2, 3]\nlet first: &mut [int] = values[:]\nlet second: &mut [int] = values[:]\nprint len(first)\nprint len(second)\n",
        )
        .expect("write source");
        let error = check_project(&project)
            .expect_err("second mutable borrow should fail while the first is live");
        assert!(error.message.contains(
            "cannot create mutable borrow of value \"values\" while another mutable borrow is still live"
        ));
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_moving_owned_array_while_mutable_slice_borrow_is_live() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("live-mut-borrow-move");
        create_project(&project, Some("live-mut-borrow-move-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [string] = [\"alpha\", \"beta\"]\nlet view: &mut [string] = values[:]\nprint len(view)\nprint first(values)\n",
        )
        .expect("write source");
        let error =
            check_project(&project).expect_err("moving a mutably borrowed owner should fail");
        assert!(
            error
                .message
                .contains("cannot move value \"values\" while borrowed slices are still live")
        );
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_moving_owner_while_tuple_wrapped_mut_slice_is_live() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("tuple-wrapped-live-mut-borrow");
        create_project(&project, Some("tuple-wrapped-live-mut-borrow-app"))
            .expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [string] = [\"alpha\", \"beta\"]\nlet wrapped: (&mut [string], int) = (values[:], 1)\nprint len(wrapped.0)\nprint first(values)\n",
        )
        .expect("write source");
        let error = check_project(&project)
            .expect_err("tuple-wrapped mutable borrow should block owner move");
        assert!(
            error
                .message
                .contains("cannot move value \"values\" while borrowed slices are still live")
        );
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_moving_owner_while_option_wrapped_mut_slice_is_live() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("option-wrapped-live-mut-borrow");
        create_project(&project, Some("option-wrapped-live-mut-borrow-app"))
            .expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [string] = [\"alpha\", \"beta\"]\nlet wrapped: Option<&mut [string]> = Some(values[:])\nmatch wrapped {\nSome(view) {\nprint len(view)\n}\nNone {\nprint 0\n}\n}\nprint first(values)\n",
        )
        .expect("write source");
        let error = check_project(&project)
            .expect_err("option-wrapped mutable borrow should block owner move");
        assert!(
            error
                .message
                .contains("cannot move value \"values\" while borrowed slices are still live")
        );
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_moving_owner_while_struct_wrapped_mut_slice_is_live() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("struct-wrapped-live-mut-borrow");
        create_project(&project, Some("struct-wrapped-live-mut-borrow-app"))
            .expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct Window {\nview: &mut [string]\n}\n\nlet values: [string] = [\"alpha\", \"beta\"]\nlet window: Window = Window { view: values[:] }\nprint len(window.view)\nprint first(values)\n",
        )
        .expect("write source");
        let error = check_project(&project)
            .expect_err("struct-wrapped mutable borrow should block owner move");
        assert!(
            error
                .message
                .contains("cannot move value \"values\" while borrowed slices are still live")
        );
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_moving_owner_while_enum_wrapped_mut_slice_is_live() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("enum-wrapped-live-mut-borrow");
        create_project(&project, Some("enum-wrapped-live-mut-borrow-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum Snapshot {\nWindow(&mut [string])\n}\n\nlet values: [string] = [\"alpha\", \"beta\"]\nlet snapshot: Snapshot = Window(values[:])\nmatch snapshot {\nWindow(view) {\nprint len(view)\n}\n}\nprint first(values)\n",
        )
        .expect("write source");
        let error = check_project(&project)
            .expect_err("enum-wrapped mutable borrow should block owner move");
        assert!(
            error
                .message
                .contains("cannot move value \"values\" while borrowed slices are still live")
        );
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_moving_owner_while_tuple_wrapped_slice_is_live() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("tuple-wrapped-live-borrow");
        create_project(&project, Some("tuple-wrapped-live-borrow-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [string] = [\"alpha\", \"beta\"]\nlet wrapped: (&[string], int) = (values[:], 1)\nprint len(wrapped.0)\nprint first(values)\n",
        )
        .expect("write source");
        let error =
            check_project(&project).expect_err("tuple-wrapped borrow should block owner move");
        assert!(
            error
                .message
                .contains("cannot move value \"values\" while borrowed slices are still live")
        );
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_moving_owner_while_option_wrapped_slice_is_live() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("option-wrapped-live-borrow");
        create_project(&project, Some("option-wrapped-live-borrow-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let values: [string] = [\"alpha\", \"beta\"]\nlet wrapped: Option<&[string]> = Some(values[:])\nmatch wrapped {\nSome(view) {\nprint len(view)\n}\nNone {\nprint 0\n}\n}\nprint first(values)\n",
        )
        .expect("write source");
        let error =
            check_project(&project).expect_err("option-wrapped borrow should block owner move");
        assert!(
            error
                .message
                .contains("cannot move value \"values\" while borrowed slices are still live")
        );
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_moving_owner_while_struct_wrapped_slice_is_live() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("struct-wrapped-live-borrow");
        create_project(&project, Some("struct-wrapped-live-borrow-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct Window {\nview: &[string]\n}\n\nlet values: [string] = [\"alpha\", \"beta\"]\nlet window: Window = Window { view: values[:] }\nprint len(window.view)\nprint first(values)\n",
        )
        .expect("write source");
        let error =
            check_project(&project).expect_err("struct-wrapped borrow should block owner move");
        assert!(
            error
                .message
                .contains("cannot move value \"values\" while borrowed slices are still live")
        );
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_moving_owner_while_enum_wrapped_slice_is_live() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("enum-wrapped-live-borrow");
        create_project(&project, Some("enum-wrapped-live-borrow-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum Snapshot {\nWindow(&[string])\n}\n\nlet values: [string] = [\"alpha\", \"beta\"]\nlet snapshot: Snapshot = Window(values[:])\nmatch snapshot {\nWindow(view) {\nprint len(view)\n}\n}\nprint first(values)\n",
        )
        .expect("write source");
        let error =
            check_project(&project).expect_err("enum-wrapped borrow should block owner move");
        assert!(
            error
                .message
                .contains("cannot move value \"values\" while borrowed slices are still live")
        );
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_mixed_map_literal_key_types() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("bad-map-literal-keys");
        create_project(&project, Some("bad-map-literal-keys-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let scores: {string: int} = {\"build\": 7, 9: 10}\n",
        )
        .expect("write source");
        let error =
            check_project(&project).expect_err("map literal should require matching key types");
        assert!(
            error
                .message
                .contains("map literal expects matching key types")
        );
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_unsupported_map_key_type() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("bad-map-key-type");
        create_project(&project, Some("bad-map-key-type-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let bad: {[int]: int} = {[1, 2]: 7}\n",
        )
        .expect("write source");
        let error =
            check_project(&project).expect_err("map type should reject unsupported key type");
        assert!(
            error
                .message
                .contains("map key type [int] is not supported")
        );
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_wrong_map_key_type_on_index() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("bad-map-index-key");
        create_project(&project, Some("bad-map-index-key-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let scores: {string: int} = {\"build\": 7}\nprint scores[0]\n",
        )
        .expect("write source");
        let error =
            check_project(&project).expect_err("map index should require matching key type");
        assert!(
            error
                .message
                .contains("map index expects key type string, got int")
        );
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_use_after_non_copy_map_index() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("map-move");
        create_project(&project, Some("map-move-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let labels: {string: string} = {\"build\": \"green\", \"deploy\": \"ready\"}\nprint labels[\"build\"]\nprint labels[\"deploy\"]\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("non-copy map index should consume owner");
        assert!(error.message.contains("use of moved value"));
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_tuple_index_on_non_tuple() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("bad-tuple-index");
        create_project(&project, Some("bad-tuple-index-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let answer: int = 42\nprint answer.0\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("tuple index should require tuple");
        assert!(error.message.contains("tuple index expects a tuple value"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_out_of_bounds_tuple_index() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("tuple-index-bounds");
        create_project(&project, Some("tuple-index-bounds-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let pair: (int, string) = (7, \"label\")\nprint pair.2\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("tuple index should enforce bounds");
        assert!(error.message.contains("tuple index 2 is out of bounds"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_allows_non_copy_tuple_index_then_sibling_use() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("tuple-partial-move-sibling");
        create_project(&project, Some("tuple-partial-move-sibling-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let pair: (int, string) = (7, \"label\")\nprint pair.1\nprint pair.0\n",
        )
        .expect("write source");
        check_project(&project).expect("moving one tuple slot should leave siblings available");
    }

    #[test]
    fn check_project_rejects_whole_tuple_use_after_non_copy_tuple_index() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("tuple-partial-move-whole-use");
        create_project(&project, Some("tuple-partial-move-whole-use-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn consume(pair: (int, string)): string {\nreturn pair.1\n}\n\nlet pair: (int, string) = (7, \"label\")\nprint pair.1\nprint consume(pair)\n",
        )
        .expect("write source");
        let error = check_project(&project)
            .expect_err("partially moved tuple should not be usable as a whole value");
        assert!(error.message.contains("use of partially moved value"));
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_use_after_non_copy_array_index() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("array-move");
        create_project(&project, Some("array-move-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let labels: [string] = [\"a\", \"b\"]\nprint labels[0]\nprint labels[1]\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("non-copy array index should consume owner");
        assert!(error.message.contains("use of moved value"));
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_rejects_non_exhaustive_match() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("non-exhaustive-match");
        create_project(&project, Some("non-exhaustive-match-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum Status {\nReady\nFailed\n}\n\nfn label(status: Status): string {\nmatch status {\nReady {\nreturn \"ready\"\n}\n}\n}\n\nprint \"skip\"\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("match should be exhaustive");
        assert!(error.message.contains("not exhaustive"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_unknown_match_variant() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("unknown-match-variant");
        create_project(&project, Some("unknown-match-variant-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum Status {\nReady\nFailed\n}\n\nfn label(status: Status): string {\nmatch status {\nUnknown {\nreturn \"nope\"\n}\nReady {\nreturn \"ready\"\n}\nFailed {\nreturn \"failed\"\n}\n}\n}\n\nprint \"skip\"\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("match should reject unknown variant");
        assert!(error.message.contains("has no variant"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_suggests_similar_match_variant() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("match-variant-suggestion");
        create_project(&project, Some("match-variant-suggestion-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum Status {\nReady\nFailed\n}\n\nfn label(status: Status): string {\nmatch status {\nReday {\nreturn \"ready\"\n}\nFailed {\nreturn \"failed\"\n}\n}\n}\n\nprint \"skip\"\n",
        )
        .expect("write source");

        let error = check_project(&project).expect_err("match should reject unknown variant");
        assert!(error.message.contains("has no variant \"Reday\""));
        assert!(error.message.contains("did you mean \"Ready\"?"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_missing_payload_match_binding() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("missing-payload-binding");
        create_project(&project, Some("missing-payload-binding-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum Message {\nText(string)\nCount(int)\n}\n\nfn render(message: Message): string {\nmatch message {\nText {\nreturn \"text\"\n}\nCount(count) {\nreturn \"count\"\n}\n}\n}\n\nprint \"skip\"\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("match should require payload binding");
        assert!(error.message.contains("expects 1 bindings, got 0"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_multi_payload_match_binding_count() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("multi-payload-binding-count");
        create_project(&project, Some("multi-payload-binding-count-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum Message {\nPair(int, string)\nText(string)\n}\n\nfn render(message: Message): string {\nmatch message {\nPair(label) {\nreturn label\n}\nText(text) {\nreturn text\n}\n}\n}\n\nprint \"skip\"\n",
        )
        .expect("write source");
        let error =
            check_project(&project).expect_err("match should enforce payload binding count");
        assert!(error.message.contains("expects 2 bindings, got 1"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_payload_constructor_type_mismatch() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("payload-constructor-type");
        create_project(&project, Some("payload-constructor-type-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum Message {\nText(string)\nCount(int)\n}\n\nlet message: Message = Text(42)\nprint \"skip\"\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("payload constructor should typecheck");
        assert!(error.message.contains("expects payload type string"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_named_payload_constructor_with_positional_args() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("named-payload-constructor-positional");
        create_project(&project, Some("named-payload-constructor-positional-app"))
            .expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum Message {\nJob { id: int, label: string }\n}\n\nlet message: Message = Job(7, \"x\")\nprint \"skip\"\n",
        )
        .expect("write source");
        let error = check_project(&project)
            .expect_err("named payload variant should reject positional args");
        assert!(error.message.contains("requires named payload fields"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_named_payload_constructor_missing_field() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("named-payload-constructor-missing");
        create_project(&project, Some("named-payload-constructor-missing-app"))
            .expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum Message {\nJob { id: int, label: string }\n}\n\nlet message: Message = Job { id: 7 }\nprint \"skip\"\n",
        )
        .expect("write source");
        let error =
            check_project(&project).expect_err("named payload variant should require all fields");
        assert!(error.message.contains("is missing named payload"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_multi_payload_constructor_arity() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("multi-payload-constructor-arity");
        create_project(&project, Some("multi-payload-constructor-arity-app"))
            .expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum Message {\nPair(int, string)\nText(string)\n}\n\nlet message: Message = Pair(7)\nprint \"skip\"\n",
        )
        .expect("write source");
        let error = check_project(&project).expect_err("payload constructor should enforce arity");
        assert!(error.message.contains("expects 2 arguments, got 1"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn check_project_rejects_named_payload_match_with_positional_bindings() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("named-payload-match-positional");
        create_project(&project, Some("named-payload-match-positional-app"))
            .expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "enum Message {\nJob { id: int, label: string }\n}\n\nfn render(message: Message): string {\nmatch message {\nJob(id, label) {\nreturn label\n}\n}\n}\n\nprint \"skip\"\n",
        )
        .expect("write source");
        let error =
            check_project(&project).expect_err("named payload match should require named bindings");
        assert!(error.message.contains("must use named bindings"));
        assert_eq!(error.kind, "type");
    }

    #[test]
    fn build_project_emits_native_binary_from_imported_public_enums() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("enum-modules");
        create_project(&project, Some("enum-modules-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "import \"status.ax\"\n\nfn label(status: Status): string {\nmatch status {\nReady {\nreturn \"ready\"\n}\nFailed {\nreturn \"failed\"\n}\n}\n}\n\nlet status: Status = Ready\nprint label(status)\n",
        )
        .expect("write main");
        fs::write(
            project.join("src/status.ax"),
            "pub enum Status {\nReady\nFailed\n}\n",
        )
        .expect("write status");
        let built = build_project(&project).expect("build imported enums");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "ready\n");
    }

    #[test]
    fn build_project_emits_native_binary_from_imported_payload_enums() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("payload-enum-modules");
        create_project(&project, Some("payload-enum-modules-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "import \"status.ax\"\n\nfn render(status: Status): string {\nmatch status {\nReady(label) {\nreturn label\n}\nFailed(label) {\nreturn label\n}\n}\n}\n\nlet status: Status = Ready(\"from import\")\nprint render(status)\n",
        )
        .expect("write main");
        fs::write(
            project.join("src/status.ax"),
            "pub enum Status {\nReady(string)\nFailed(string)\n}\n",
        )
        .expect("write status");
        let built = build_project(&project).expect("build imported payload enums");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "from import\n");
    }

    #[test]
    fn build_project_emits_native_binary_from_imported_named_payload_enums() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("named-payload-enum-modules");
        create_project(&project, Some("named-payload-enum-modules-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "import \"status.ax\"\n\nfn render(status: Status): string {\nmatch status {\nReady { label } {\nreturn label\n}\nFailed { label } {\nreturn label\n}\n}\n}\n\nlet status: Status = Ready { label: \"from import\" }\nprint render(status)\n",
        )
        .expect("write main");
        fs::write(
            project.join("src/status.ax"),
            "pub enum Status {\nReady { label: string }\nFailed { label: string }\n}\n",
        )
        .expect("write status");
        let built = build_project(&project).expect("build imported named payload enums");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "from import\n");
    }

    #[test]
    fn build_project_emits_native_binary_from_imported_multi_payload_enums() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("multi-payload-enum-modules");
        create_project(&project, Some("multi-payload-enum-modules-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "import \"message.ax\"\n\nfn render(message: Message): string {\nmatch message {\nPair(count, label) {\nprint count\nreturn label\n}\nText(text) {\nreturn text\n}\n}\n}\n\nlet message: Message = Pair(7, \"from import\")\nprint render(message)\n",
        )
        .expect("write main");
        fs::write(
            project.join("src/message.ax"),
            "pub enum Message {\nPair(int, string)\nText(string)\n}\n",
        )
        .expect("write module");
        let built = build_project(&project).expect("build imported multi payload enums");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "7\nfrom import\n");
    }

    // ------------------------------------------------------------------
    // AG1.1: unknown-branch and loop join handling
    // ------------------------------------------------------------------

    #[test]
    fn check_project_rejects_moving_outer_string_inside_while_body() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("loop-move-outer");
        create_project(&project, Some("loop-move-outer-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let label: string = \"hello\"\nlet running: bool = true\nwhile running {\nlet sink: string = label\nprint sink\n}\n",
        )
        .expect("write source");
        let error = check_project(&project)
            .expect_err("moving outer non-copy value inside loop body should fail");
        assert!(
            error.message.contains("cannot move non-copy value")
                && error.message.contains("inside loop body"),
            "unexpected error message: {}",
            error.message
        );
        assert_eq!(error.kind, "ownership");
    }

    #[test]
    fn check_project_allows_copy_move_inside_while_body() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("loop-copy-ok");
        create_project(&project, Some("loop-copy-ok-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let count: int = 42\nlet running: bool = true\nwhile running {\nlet dup: int = count\nprint dup\n}\n",
        )
        .expect("write source");
        check_project(&project).expect("copy values should be reusable inside loop bodies");
    }

    #[test]
    fn check_project_allows_use_after_while_when_body_does_not_move() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("loop-no-move");
        create_project(&project, Some("loop-no-move-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let label: string = \"hello\"\nlet running: bool = false\nwhile running {\nprint label\n}\nprint label\n",
        )
        .expect("write source");
        check_project(&project)
            .expect("values not moved inside loop should remain available after loop");
    }

    #[test]
    fn check_project_allows_local_string_move_inside_while_body() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("loop-local-move");
        create_project(&project, Some("loop-local-move-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let running: bool = true\nwhile running {\nlet inner: string = \"fresh\"\nlet sink: string = inner\nprint sink\n}\n",
        )
        .expect("write source");
        check_project(&project).expect("moving loop-local values should be allowed");
    }

    #[test]
    fn build_project_records_requested_target_triple() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("targeted-build");
        create_project(&project, Some("targeted-build-app")).expect("create project");

        let target = rust_host_target();
        let output = build_project_with_options(
            &project,
            &BuildOptions {
                backend: NativeBackendKind::GeneratedRust,
                target: Some(target.clone()),
                package: None,
                debug: false,
                ..BuildOptions::default()
            },
        )
        .expect("build project with explicit target");

        assert_eq!(output.target.as_deref(), Some(target.as_str()));
        assert!(project.join("dist/targeted-build-app").exists());
    }

    #[test]
    fn build_project_wasm_alias_emits_wasm_artifact() {
        if !rust_target_installed("wasm32-wasip1") {
            eprintln!("skipping wasm build test; wasm32-wasip1 target is not installed");
            return;
        }

        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("targeted-wasm-build");
        create_project(&project, Some("targeted-wasm-build-app")).expect("create project");

        let output = build_project_with_options(
            &project,
            &BuildOptions {
                backend: NativeBackendKind::GeneratedRust,
                target: Some(String::from("wasm32")),
                package: None,
                debug: false,
                ..BuildOptions::default()
            },
        )
        .expect("build project with wasm alias");

        assert_eq!(output.target.as_deref(), Some("wasm32-wasip1"));
        assert!(output.binary.ends_with("targeted-wasm-build-app.wasm"));
        assert!(project.join("dist/targeted-wasm-build-app.wasm").exists());
    }

    #[test]
    fn build_project_debug_mode_emits_source_markers_and_uses_separate_cache_key() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("debug-build");
        create_project(&project, Some("debug-build-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "let answer: int = 42\nprint answer\n",
        )
        .expect("write source");

        let release = build_project(&project).expect("release build");
        assert_eq!(release.cache_misses, 1);
        assert!(!release.debug);
        let release_generated =
            fs::read_to_string(&release.generated_rust).expect("read release generated rust");
        assert!(!release_generated.contains("// axiom-source:"));

        let debug = build_project_with_options(
            &project,
            &BuildOptions {
                backend: NativeBackendKind::GeneratedRust,
                target: None,
                package: None,
                debug: true,
                ..BuildOptions::default()
            },
        )
        .expect("debug build");
        assert!(debug.debug);
        assert!(debug.packages[0].debug);
        let debug_map = PathBuf::from(debug.debug_map.as_ref().expect("debug map path"));
        assert!(debug_map.exists());
        assert_eq!(debug.cache_hits, 0);
        assert_eq!(debug.cache_misses, 1);

        let generated = fs::read_to_string(&debug.generated_rust).expect("read generated rust");
        let source = project
            .join("src/main.ax")
            .canonicalize()
            .expect("canonical source path")
            .display()
            .to_string();
        assert!(generated.contains(&format!("// axiom-source: {source}:1:1")));
        assert!(generated.contains(&format!("// axiom-source: {source}:2:1")));
        let map: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&debug_map).expect("read debug map"))
                .expect("parse debug map");
        assert_eq!(map["schema_version"], "axiom.stage1.debug_map.v1");
        assert_eq!(map["generated_rust"], debug.generated_rust);
        assert_eq!(map["mappings"][0]["source"], source);
        assert_eq!(map["mappings"][0]["line"], 1);
        assert_eq!(map["mappings"][0]["column"], 1);
        assert!(map["mappings"][0]["generated_line"].is_u64());

        fs::remove_file(&debug_map).expect("remove debug map");
        let cached_debug = build_project_with_options(
            &project,
            &BuildOptions {
                backend: NativeBackendKind::GeneratedRust,
                target: None,
                package: None,
                debug: true,
                ..BuildOptions::default()
            },
        )
        .expect("cached debug build");
        assert_eq!(cached_debug.cache_hits, 1);
        assert_eq!(cached_debug.cache_misses, 0);
        assert!(debug_map.exists());
    }

    #[test]
    fn build_project_reuses_incremental_cache_until_module_changes() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("cached-build");
        create_project(&project, Some("cached-build-app")).expect("create project");
        fs::write(
            project.join("src/math.ax"),
            "pub fn answer(): int {\nreturn 41\n}\n",
        )
        .expect("write module");
        fs::write(
            project.join("src/main.ax"),
            "import \"math.ax\"\nprint answer()\n",
        )
        .expect("write source");

        let first = build_project(&project).expect("initial build");
        assert_eq!(first.cache_hits, 0);
        assert_eq!(first.cache_misses, 1);
        assert_eq!(first.packages[0].cache_status, BuildCacheStatus::Miss);
        let generated = fs::read_to_string(&first.generated_rust).expect("read generated rust");

        let second = build_project(&project).expect("cached build");
        assert_eq!(second.cache_hits, 1);
        assert_eq!(second.cache_misses, 0);
        assert_eq!(second.packages[0].cache_status, BuildCacheStatus::Hit);
        assert_eq!(
            fs::read_to_string(&second.generated_rust).expect("read cached generated rust"),
            generated
        );

        fs::write(&second.generated_rust, "// stale generated rust\n")
            .expect("corrupt generated rust");
        let repaired_rust = build_project(&project).expect("repair generated rust");
        assert_eq!(repaired_rust.cache_hits, 0);
        assert_eq!(repaired_rust.cache_misses, 1);
        assert_eq!(
            repaired_rust.packages[0].cache_status,
            BuildCacheStatus::Miss
        );
        assert_eq!(
            fs::read_to_string(&repaired_rust.generated_rust).expect("read repaired rust"),
            generated
        );

        fs::write(&repaired_rust.binary, "not a compiled binary").expect("corrupt binary");
        let repaired_binary = build_project(&project).expect("repair binary");
        assert_eq!(repaired_binary.cache_hits, 0);
        assert_eq!(repaired_binary.cache_misses, 1);
        assert_eq!(
            repaired_binary.packages[0].cache_status,
            BuildCacheStatus::Miss
        );
        let output = compiled_binary_command(&repaired_binary.binary)
            .output()
            .expect("run repaired binary");
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "41\n");

        fs::write(
            project.join("src/math.ax"),
            "pub fn answer(): int {\nreturn 42\n}\n",
        )
        .expect("update module");
        let third = build_project(&project).expect("rebuild after module change");
        assert_eq!(third.cache_hits, 0);
        assert_eq!(third.cache_misses, 1);
        assert_eq!(third.packages[0].cache_status, BuildCacheStatus::Miss);
        let output = compiled_binary_command(&third.binary)
            .output()
            .expect("run rebuilt binary");
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");
    }

    #[test]
    fn run_project_tests_supports_name_filtering() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("filtered-tests");
        create_project(&project, Some("filtered-tests-app")).expect("create project");
        fs::write(project.join("src/math_test.ax"), "print 42\n").expect("write filtered test");
        fs::write(project.join("src/math_test.stdout"), "42\n").expect("write filtered stdout");

        let output = run_project_tests_with_options(
            &project,
            &TestOptions {
                filter: Some(String::from("math")),
                package: None,
                include_benchmarks: false,
            },
        )
        .expect("run filtered tests");

        assert_eq!(output.passed, 1);
        assert_eq!(output.failed, 0);
        assert_eq!(output.cases.len(), 1);
        assert_eq!(output.cases[0].name, "src/math_test");
        assert!(output.duration_ms > 0 || output.cases[0].duration_ms <= output.duration_ms);
    }

    #[test]
    fn json_contract_check_payload_is_versioned() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("json-check");
        create_project(&project, Some("json-check-app")).expect("create project");
        let output = check_project(&project).expect("check project");

        let payload = json_contract::check_success(&project, &output);
        assert_eq!(
            payload["schema_version"],
            json_contract::JSON_SCHEMA_VERSION
        );
        assert_eq!(payload["command"], "check");
        assert_eq!(payload["ok"], true);
        assert!(payload["packages"].is_array());
    }

    #[test]
    fn json_contract_build_payload_includes_target() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("json-build");
        create_project(&project, Some("json-build-app")).expect("create project");
        let output = build_project_with_options(
            &project,
            &BuildOptions {
                backend: NativeBackendKind::GeneratedRust,
                target: Some(rust_host_target()),
                package: None,
                debug: true,
                locked: true,
                offline: true,
                ..BuildOptions::default()
            },
        )
        .expect("build project");

        let payload = json_contract::build_success(&project, &output);
        assert_eq!(
            payload["schema_version"],
            json_contract::JSON_SCHEMA_VERSION
        );
        assert_eq!(payload["command"], "build");
        assert_eq!(payload["backend"], "generated-rust");
        assert_eq!(payload["locked"], true);
        assert_eq!(payload["offline"], true);
        assert!(payload["target"].is_string());
        assert_eq!(payload["debug"], true);
        assert!(payload["debug_map"].is_string());
        assert!(payload["cache_hits"].is_u64());
        assert!(payload["cache_misses"].is_u64());
        assert!(payload["duration_ms"].is_u64());
    }

    #[test]
    fn json_contract_test_payload_includes_filter_and_duration() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("json-test");
        create_project(&project, Some("json-test-app")).expect("create project");

        let output = run_project_tests_with_options(
            &project,
            &TestOptions {
                filter: Some(String::from("main")),
                package: None,
                include_benchmarks: false,
            },
        )
        .expect("test project");
        let payload = json_contract::test_success(&project, Some("main"), &output);

        assert_eq!(
            payload["schema_version"],
            json_contract::JSON_SCHEMA_VERSION
        );
        assert_eq!(payload["command"], "test");
        assert_eq!(payload["filter"], "main");
        assert_eq!(payload["skipped"], 0);
        assert_eq!(payload["cases"][0]["kind"], "unit");
        assert_eq!(payload["kinds"]["unit"], 1);
        assert!(payload["duration_ms"].is_u64());
    }

    #[test]
    fn json_contract_caps_and_error_payloads_are_versioned() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("json-caps");
        create_project(&project, Some("json-caps-app")).expect("create project");
        let caps = project_capabilities(&project).expect("project capabilities");

        let caps_payload = json_contract::caps_success(&project, &caps);
        assert_eq!(
            caps_payload["schema_version"],
            json_contract::JSON_SCHEMA_VERSION
        );
        assert_eq!(caps_payload["command"], "caps");
        assert_eq!(caps_payload["ok"], true);

        let error =
            crate::diagnostics::Diagnostic::new("ownership", "boom").with_code("use_after_move");
        let error_payload = json_contract::error("test", &error);
        assert_eq!(
            error_payload["schema_version"],
            json_contract::JSON_SCHEMA_VERSION
        );
        assert_eq!(error_payload["command"], "test");
        assert_eq!(error_payload["ok"], false);
        assert_eq!(error_payload["error"]["kind"], "ownership");
        assert_eq!(error_payload["error"]["code"], "use_after_move");
        assert_eq!(error_payload["error"]["message"], "boom");
    }

    #[test]
    fn json_contract_pretty_serializer_failure_is_diagnostic() {
        struct FailingPayload;

        impl Serialize for FailingPayload {
            fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                Err(serde::ser::Error::custom("forced serializer failure"))
            }
        }

        let error = json_contract::to_pretty_string(&FailingPayload)
            .expect_err("serializer errors should return diagnostics");

        assert_eq!(error.kind, "json");
        assert!(error.message.contains("failed to serialize JSON output"));
        assert!(error.message.contains("forced serializer failure"));
    }
    #[test]
    fn build_project_supports_impl_methods_and_associated_functions() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("impl-methods");
        create_project(&project, Some("impl-methods-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct Counter {
value: int
}

impl Counter {
fn new(value: int): Counter {
return Counter { value: value }
}

fn bump(self, delta: int): Counter {
return Counter { value: self.value + delta }
}

fn read(self): int {
return self.value
}
}

let counter: Counter = Counter.new(40)
let next: Counter = counter.bump(2)
print next.read()
",
        )
        .expect("write source");
        let built = build_project(&project).expect("build project with impl methods");
        let output = compiled_binary_command(&built.binary)
            .output()
            .expect("run compiled binary");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "42
"
        );
    }

    #[test]
    fn check_project_rejects_self_parameter_outside_impl() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("self-outside-impl");
        create_project(&project, Some("self-outside-impl-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "fn read(self): int {
return 42
}

print 0
",
        )
        .expect("write source");

        let error = check_project(&project).expect_err("self outside impl should fail");
        assert_eq!(error.kind, "parse");
        assert!(
            error
                .message
                .contains("self parameter is only allowed inside impl methods")
        );
    }

    #[test]
    fn check_project_rejects_calling_method_without_receiver() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("method-without-receiver");
        create_project(&project, Some("method-without-receiver-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct Counter {
value: int
}

impl Counter {
fn bump(self, delta: int): Counter {
return Counter { value: self.value + delta }
}
}

let counter: Counter = Counter.bump(2)
print counter.value
",
        )
        .expect("write source");

        let error = check_project(&project).expect_err("method call without receiver should fail");
        assert_eq!(error.kind, "type");
        assert!(error.message.contains("requires a value receiver"));
    }

    #[test]
    fn check_project_rejects_calling_associated_function_as_method() {
        let dir = tempdir().expect("tempdir");
        let project = dir.path().join("associated-as-method");
        create_project(&project, Some("associated-as-method-app")).expect("create project");
        fs::write(
            project.join("src/main.ax"),
            "struct Counter {
value: int
}

impl Counter {
fn new(value: int): Counter {
return Counter { value: value }
}
}

let counter: Counter = Counter { value: 40 }
let next: Counter = counter.new(2)
print next.value
",
        )
        .expect("write source");

        let error = check_project(&project).expect_err("associated function as method should fail");
        assert_eq!(error.kind, "type");
        assert!(error.message.contains("must be called as"));
        assert!(error.message.contains(".new()"));
    }
}
