use crate::diagnostics::Diagnostic;

pub(super) fn primary_diagnostic(mut diagnostics: Vec<Diagnostic>) -> Diagnostic {
    sort_diagnostics(&mut diagnostics);
    let mut first = diagnostics.remove(0);
    first.related = diagnostics;
    first
}

pub(super) fn single_diagnostic(diagnostic: Diagnostic) -> Vec<Diagnostic> {
    vec![diagnostic]
}

pub(super) fn append_diagnostic(diagnostics: &mut Vec<Diagnostic>, mut diagnostic: Diagnostic) {
    diagnostics.append(&mut diagnostic.related);
    diagnostics.push(diagnostic);
}

pub(super) fn sort_diagnostics(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| left.column.cmp(&right.column))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.message.cmp(&right.message))
    });
}
