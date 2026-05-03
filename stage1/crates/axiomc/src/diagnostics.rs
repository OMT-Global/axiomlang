use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<Diagnostic>,
}

impl Diagnostic {
    pub fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            code: None,
            message: message.into(),
            help: None,
            path: None,
            line: None,
            column: None,
            related: Vec::new(),
        }
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
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
