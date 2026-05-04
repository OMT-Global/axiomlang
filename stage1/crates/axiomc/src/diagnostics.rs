use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DiagnosticRepair {
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repair: Option<DiagnosticRepair>,
}

impl Diagnostic {
    pub fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            code: None,
            message: message.into(),
            path: None,
            line: None,
            column: None,
            related: Vec::new(),
            repair: None,
        }
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_span(mut self, line: usize, column: usize) -> Self {
        self.line = Some(line);
        self.column = Some(column);
        self
    }

    pub fn with_related(mut self, related: Vec<Diagnostic>) -> Self {
        self.related = related;
        self
    }

    pub fn with_repair(
        mut self,
        action: impl Into<String>,
        edit: Option<impl Into<String>>,
        command: Option<impl Into<String>>,
    ) -> Self {
        self.repair = Some(DiagnosticRepair {
            action: action.into(),
            edit: edit.map(Into::into),
            command: command.map(Into::into),
        });
        self
    }

    pub fn normalized_for_json(&self) -> Self {
        let mut normalized = self.clone();
        if normalized.code.is_none() {
            normalized.code = stable_diagnostic_code(&normalized.kind, &normalized.message);
        }
        if normalized.repair.is_none() {
            normalized.repair = repair_hint(&normalized.kind, &normalized.message);
        }
        normalized.related = normalized
            .related
            .iter()
            .map(Diagnostic::normalized_for_json)
            .collect();
        normalized
    }
}

fn stable_diagnostic_code(kind: &str, message: &str) -> Option<String> {
    let code = match kind {
        "parse" if message.contains("missing closing brace") => "parse.missing_closing_brace",
        "parse" if message.contains("unexpected closing brace") => "parse.unexpected_closing_brace",
        "parse" if message.contains("not supported") => "parse.unsupported_syntax",
        "parse" if message.contains("missing") => "parse.missing_token",
        "parse" => "parse.invalid_syntax",
        "manifest" if message.contains("capability") => "manifest.invalid_capability",
        "manifest" => "manifest.invalid",
        "import" if message.contains("not found") || message.contains("failed to read") => {
            "import.unresolved"
        }
        "import" => "import.invalid",
        "capability" if message.contains("requires") || message.contains("not enabled") => {
            "capability.denied"
        }
        "capability" => "capability.invalid",
        "type" if message.contains("undefined") || message.contains("unknown") => {
            "type.undefined_symbol"
        }
        "type" if message.contains("expected") || message.contains("mismatch") => "type.mismatch",
        "type" => "type.invalid",
        "ownership" => "ownership.invalid",
        "codegen" | "build" => "build.failed",
        "runtime" => "runtime.failed",
        "fmt" => "fmt.failed",
        "source" => "source.invalid",
        "json" => "json.serialization_failed",
        _ => return None,
    };
    Some(code.to_string())
}

fn repair_hint(kind: &str, _message: &str) -> Option<DiagnosticRepair> {
    let (action, edit, command) = match kind {
        "parse" | "type" | "ownership" => (
            "edit_source",
            Some("Update the source at the reported span and rerun `axiomc check --json`."),
            None,
        ),
        "manifest" | "capability" => (
            "edit_manifest",
            Some("Update axiom.toml with the narrowest required package, dependency, or capability change."),
            None,
        ),
        "import" => (
            "edit_import",
            Some("Fix the quoted relative import path or add the missing imported source file."),
            None,
        ),
        "fmt" => (
            "run_command",
            None,
            Some("axiomc fmt <path>"),
        ),
        "source" => (
            "check_path",
            Some("Use an existing .ax source path or package directory."),
            None,
        ),
        "build" | "codegen" | "runtime" => (
            "rerun_command",
            Some("Review the compiler message and rerun the failed command after source changes."),
            None,
        ),
        _ => return None,
    };
    Some(DiagnosticRepair {
        action: action.to_string(),
        edit: edit.map(str::to_string),
        command: command.map(str::to_string),
    })
}

pub fn message_with_suggestion(
    message: impl Into<String>,
    needle: &str,
    candidates: impl IntoIterator<Item = impl AsRef<str>>,
) -> String {
    let message = message.into();
    match closest_name(needle, candidates) {
        Some(candidate) => format!("{message}; did you mean {candidate:?}?"),
        None => message,
    }
}

fn closest_name(
    needle: &str,
    candidates: impl IntoIterator<Item = impl AsRef<str>>,
) -> Option<String> {
    candidates
        .into_iter()
        .filter_map(|candidate| {
            let candidate = candidate.as_ref();
            let distance = edit_distance(needle, candidate);
            if is_plausible_suggestion(needle, candidate, distance) {
                Some((distance, candidate.to_string()))
            } else {
                None
            }
        })
        .min_by(|(left_distance, left_name), (right_distance, right_name)| {
            left_distance
                .cmp(right_distance)
                .then_with(|| left_name.cmp(right_name))
        })
        .map(|(_, candidate)| candidate)
}

fn is_plausible_suggestion(needle: &str, candidate: &str, distance: usize) -> bool {
    if needle.is_empty() || candidate.is_empty() {
        return false;
    }
    let max_len = needle.chars().count().max(candidate.chars().count());
    distance <= 2 || (max_len >= 6 && distance <= 3)
}

fn edit_distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0; right.len() + 1];

    for (left_index, left_char) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_char) in right.iter().enumerate() {
            let substitution = usize::from(left_char != right_char);
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + substitution);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[right.len()]
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.path, self.line, self.column) {
            (Some(path), Some(line), Some(column)) => {
                write!(f, "{}:{}:{}: {}", path, line, column, self.message)
            }
            (Some(path), _, _) => write!(f, "{}: {}", path, self.message),
            _ => write!(f, "{}", self.message),
        }
    }
}

impl std::error::Error for Diagnostic {}
