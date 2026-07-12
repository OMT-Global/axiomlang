//! Lossless lexical formatting for Axiom source.
//!
//! The formatter deliberately stays below the parser: malformed input is still
//! useful in an editor, and literal, comment, and macro token-tree bytes must
//! never be interpreted as syntax.

pub(super) fn format(source: &str) -> String {
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let mut output = Vec::new();
    let mut previous_blank = false;
    let mut indent = 0usize;
    let mut macro_depth = 0isize;
    for line in normalized.lines() {
        let trimmed = line.trim();
        let macro_start = macro_depth == 0 && starts_macro_declaration(trimmed);
        if macro_depth > 0 || macro_start {
            output.push(line.to_string());
            macro_depth = (macro_depth + brace_delta(line)).max(0);
            previous_blank = false;
            continue;
        }
        if trimmed.is_empty() {
            if !previous_blank {
                output.push(String::new());
            }
            previous_blank = true;
            continue;
        }
        previous_blank = false;
        let leading_closes = trimmed
            .chars()
            .take_while(|character| *character == '}')
            .count();
        let line_indent = indent.saturating_sub(leading_closes);
        output.push(format!(
            "{}{}",
            "    ".repeat(line_indent),
            format_code_line(trimmed)
        ));
        let delta = brace_delta(trimmed);
        indent = if delta < 0 {
            indent.saturating_sub(delta.unsigned_abs())
        } else {
            indent.saturating_add(delta as usize)
        };
    }
    sort_import_groups(&mut output);
    while output.last().is_some_and(String::is_empty) {
        output.pop();
    }
    format!("{}\n", output.join("\n"))
}

fn starts_macro_declaration(line: &str) -> bool {
    line.strip_prefix("pub ")
        .unwrap_or(line)
        .starts_with("macro ")
}

fn brace_delta(line: &str) -> isize {
    let (code, _) = split_comment(line);
    let mut delta = 0;
    let mut quote = None;
    let mut escaped = false;
    for character in code.chars() {
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                quote = None;
            }
        } else if matches!(character, '"' | '\'') {
            quote = Some(character);
        } else if character == '{' {
            delta += 1;
        } else if character == '}' {
            delta -= 1;
        }
    }
    delta
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    Literal,
    Operator,
    Punctuation,
}

struct Lexeme<'a> {
    text: &'a str,
    kind: Kind,
}

fn format_code_line(line: &str) -> String {
    if line.starts_with('#') || line.starts_with("//") {
        return line.to_string();
    }
    let (code, comment) = split_comment(line);
    let lexemes = lex(code);
    let mut rendered = String::new();
    for (index, current) in lexemes.iter().enumerate() {
        if index > 0 && needs_space(&lexemes[index - 1], current) {
            rendered.push(' ');
        }
        rendered.push_str(current.text);
    }
    if let Some(comment) = comment {
        if !rendered.is_empty() {
            rendered.push(' ');
        }
        rendered.push_str(comment);
    }
    rendered
}

fn split_comment(line: &str) -> (&str, Option<&str>) {
    let mut quote = None;
    let mut escaped = false;
    let mut chars = line.char_indices().peekable();
    while let Some((index, character)) = chars.next() {
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                quote = None;
            }
        } else if matches!(character, '"' | '\'') {
            quote = Some(character);
        } else if character == '#'
            || (character == '/' && chars.peek().is_some_and(|(_, next)| *next == '/'))
        {
            return (&line[..index], Some(&line[index..]));
        }
    }
    (line, None)
}

fn lex(code: &str) -> Vec<Lexeme<'_>> {
    const MULTI: [&str; 13] = [
        "::", "->", "=>", "==", "!=", "<=", ">=", "&&", "||", "+=", "-=", "*=", "/=",
    ];
    let mut output = Vec::new();
    let mut offset = 0;
    while offset < code.len() {
        let tail = &code[offset..];
        let character = tail.chars().next().expect("non-empty tail");
        if character.is_whitespace() {
            offset += character.len_utf8();
        } else if matches!(character, '"' | '\'') {
            let end = quoted_end(tail, character);
            output.push(Lexeme {
                text: &tail[..end],
                kind: Kind::Literal,
            });
            offset += end;
        } else if let Some(operator) = MULTI.iter().find(|operator| tail.starts_with(**operator)) {
            output.push(Lexeme {
                text: operator,
                kind: Kind::Operator,
            });
            offset += operator.len();
        } else if "{}()[],;:.".contains(character) {
            let end = character.len_utf8();
            output.push(Lexeme {
                text: &tail[..end],
                kind: Kind::Punctuation,
            });
            offset += end;
        } else if "+-*/%=<>!&|?".contains(character) {
            let end = character.len_utf8();
            output.push(Lexeme {
                text: &tail[..end],
                kind: Kind::Operator,
            });
            offset += end;
        } else {
            let end = tail
                .char_indices()
                .skip(1)
                .find(|(_, current)| {
                    current.is_whitespace() || "{}()[],;:.+-*/%=<>!&|?\"'".contains(*current)
                })
                .map(|(index, _)| index)
                .unwrap_or(tail.len());
            output.push(Lexeme {
                text: &tail[..end],
                kind: Kind::Word,
            });
            offset += end;
        }
    }
    output
}

fn quoted_end(text: &str, delimiter: char) -> usize {
    let mut escaped = false;
    for (index, character) in text.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == delimiter {
            return index + character.len_utf8();
        }
    }
    text.len()
}

fn needs_space(previous: &Lexeme<'_>, current: &Lexeme<'_>) -> bool {
    if current.kind == Kind::Operator || previous.kind == Kind::Operator {
        return !matches!(current.text, "!" | "?") && previous.text != "!";
    }
    if matches!(current.text, ")" | "]" | "}" | "," | ";" | "." | ":")
        || matches!(previous.text, "(" | "[" | ".")
    {
        return false;
    }
    if matches!(previous.text, ":" | ",") {
        return true;
    }
    if current.text == "(" && matches!(previous.text, "if" | "while" | "match" | "for") {
        return true;
    }
    if previous.text == "}" && current.kind == Kind::Word {
        return true;
    }
    if current.text == "{" {
        return !matches!(previous.text, "(" | "[" | "{");
    }
    if previous.text == "{" {
        return false;
    }
    matches!(previous.kind, Kind::Word | Kind::Literal)
        && matches!(current.kind, Kind::Word | Kind::Literal)
}

fn sort_import_groups(lines: &mut [String]) {
    let mut index = 0;
    while index < lines.len() {
        let Some((_, first_end)) = import_block(lines, index) else {
            index += 1;
            continue;
        };
        let start = index;
        index = first_end;
        let mut ranges = vec![(start, first_end)];
        while let Some((block_start, block_end)) = import_block(lines, index) {
            ranges.push((block_start, block_end));
            index = block_end;
        }
        if ranges.len() < 2 {
            continue;
        }
        let mut blocks = ranges
            .iter()
            .map(|(block_start, block_end)| lines[*block_start..*block_end].to_vec())
            .collect::<Vec<_>>();
        blocks.sort_by(|left, right| {
            left.last()
                .expect("import block")
                .trim()
                .cmp(right.last().expect("import block").trim())
        });
        for (target, value) in lines[start..index]
            .iter_mut()
            .zip(blocks.into_iter().flatten())
        {
            *target = value;
        }
    }
}

fn import_block(lines: &[String], start: usize) -> Option<(usize, usize)> {
    let mut cursor = start;
    while cursor < lines.len() && is_comment(&lines[cursor]) {
        cursor += 1;
    }
    (cursor < lines.len() && is_import(&lines[cursor])).then_some((start, cursor + 1))
}

fn is_comment(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('#') || trimmed.starts_with("//")
}

fn is_import(line: &str) -> bool {
    let code = split_comment(line.trim()).0.trim_end();
    code.starts_with("import \"") && code.ends_with('"')
}
