use axiomc::project::{TestCaseResult, TestOutput};
use std::fmt::Write as _;
use std::path::Path;

/// Render a deterministic JUnit XML report for a completed test run.
///
/// Cases are sorted by their stable package/name/entry identity so report
/// ordering does not depend on filesystem traversal order.
pub fn render_test_output(project: &Path, output: &TestOutput) -> String {
    let mut cases = output.cases.iter().collect::<Vec<_>>();
    cases.sort_by(|left, right| {
        (&left.package_root, &left.name, &left.entry).cmp(&(
            &right.package_root,
            &right.name,
            &right.entry,
        ))
    });

    let failures = cases.iter().filter(|case| !case.ok).count();
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        xml,
        "<testsuite name=\"axiomc test\" project=\"{}\" tests=\"{}\" failures=\"{}\" skipped=\"{}\" time=\"{:.3}\">",
        escape_xml(&project.display().to_string()),
        cases.len(),
        failures,
        output.skipped,
        output.duration_ms as f64 / 1000.0,
    );
    for case in cases {
        render_case(&mut xml, case);
    }
    xml.push_str("</testsuite>\n");
    xml
}

fn render_case(xml: &mut String, case: &TestCaseResult) {
    let _ = writeln!(
        xml,
        "  <testcase classname=\"{}\" name=\"{}\" file=\"{}\" time=\"{:.3}\">",
        escape_xml(&case.package_root),
        escape_xml(&case.name),
        escape_xml(&case.entry),
        case.duration_ms as f64 / 1000.0,
    );
    if !case.ok {
        let message = case
            .error
            .as_ref()
            .map(|error| error.message.as_str())
            .unwrap_or("test case failed");
        let kind = case
            .error
            .as_ref()
            .map(|error| error.kind.as_str())
            .unwrap_or("test_failure");
        let _ = writeln!(
            xml,
            "    <failure type=\"{}\" message=\"{}\">{}</failure>",
            escape_xml(kind),
            escape_xml(message),
            escape_xml(&case.stderr),
        );
    }
    if !case.stdout.is_empty() {
        let _ = writeln!(
            xml,
            "    <system-out>{}</system-out>",
            escape_xml(&case.stdout)
        );
    }
    if !case.stderr.is_empty() && case.ok {
        let _ = writeln!(
            xml,
            "    <system-err>{}</system-err>",
            escape_xml(&case.stderr)
        );
    }
    xml.push_str("  </testcase>\n");
}

fn escape_xml(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if !is_xml_char(character) {
            escaped.push('\u{FFFD}');
            continue;
        }
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            character => escaped.push(character),
        }
    }
    escaped
}

fn is_xml_char(character: char) -> bool {
    matches!(
        character as u32,
        0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF
    )
}

#[cfg(test)]
mod tests {
    use super::render_test_output;
    use axiomc::codegen::NativeBackendKind;
    use axiomc::manifest::TestKind;
    use axiomc::project::{TestCaseResult, TestOutput};
    use std::collections::BTreeMap;
    use std::path::Path;

    fn case(name: &str, ok: bool, stdout: &str, stderr: &str) -> TestCaseResult {
        TestCaseResult {
            package_root: "pkg<&".to_string(),
            name: name.to_string(),
            kind: TestKind::Property,
            entry: format!("src/{name}.ax"),
            ok,
            binary: None,
            generated_rust: None,
            exit_code: Some(if ok { 0 } else { 1 }),
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            expected_stdout: None,
            expected_stderr: None,
            expected_error: None,
            lowering: None,
            duration_ms: 12,
            error: (!ok).then(|| axiomc::diagnostics::Diagnostic::new("property", "value < 2 & 3")),
        }
    }

    #[test]
    fn report_is_sorted_and_escapes_xml() {
        let output = TestOutput {
            backend: NativeBackendKind::Cranelift,
            manifest: "axiom.toml".to_string(),
            packages: vec!["pkg<&".to_string()],
            cases: vec![
                case("z_case", true, "ok & done\u{0}", ""),
                case("a_case", false, "", "failure <details>"),
            ],
            passed: 1,
            failed: 1,
            skipped: 0,
            kinds: BTreeMap::new(),
            duration_ms: 25,
            execution: None,
        };

        let xml = render_test_output(Path::new("project<&"), &output);
        assert!(xml.contains("project=\"project&lt;&amp;\""));
        assert!(xml.contains("failures=\"1\""));
        assert!(xml.find("name=\"a_case\"").unwrap() < xml.find("name=\"z_case\"").unwrap());
        assert!(xml.contains("value &lt; 2 &amp; 3"));
        assert!(xml.contains("ok &amp; done"));
        assert!(xml.contains("ok &amp; done�"));
        assert!(!xml.contains('\u{0}'));
        assert!(xml.contains("failure &lt;details&gt;"));
    }
}
