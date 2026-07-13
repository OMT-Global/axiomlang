use super::FormatEdit;

pub(super) fn format_edits(original: &str, formatted: &str) -> Vec<FormatEdit> {
    if original == formatted { return Vec::new(); }
    let lines = source_lines(original);
    let last_content = lines.iter().rposition(|line| !normalized_line(line.text).is_empty());
    let mut expected = Vec::new();
    let mut previous_blank = false;
    for (index, line) in lines.iter().enumerate() {
        let normalized = normalized_line(line.text);
        let blank = normalized.is_empty();
        let emit = index <= last_content.unwrap_or(0) && !(blank && previous_blank);
        expected.push(emit.then(|| format!("{normalized}\n")));
        if emit { previous_blank = blank; }
    }
    let rebuilt = expected.iter().flatten().cloned().collect::<String>();
    if rebuilt != formatted && !(lines.is_empty() && formatted == "\n") {
        return vec![precise_edit("replace_line", 1, 0, original, formatted)];
    }
    let mut edits = Vec::new();
    for (index, (line, after)) in lines.iter().zip(&expected).enumerate() {
        match after {
            Some(after) if line.text != after => edits.push(precise_edit("replace_line", index + 1, line.start_byte, line.text, after)),
            None => edits.push(precise_edit("delete_line", index + 1, line.start_byte, line.text, "")),
            _ => {}
        }
    }
    if lines.is_empty() { edits.push(precise_edit("insert_line", 1, 0, "", formatted)); }
    edits
}

struct SourceLine<'a> { start_byte: usize, text: &'a str }

fn source_lines(source: &str) -> Vec<SourceLine<'_>> {
    let mut start_byte = 0;
    source.split_inclusive('\n').map(|text| {
        let line = SourceLine { start_byte, text };
        start_byte += text.len();
        line
    }).collect()
}

fn normalized_line(line: &str) -> String {
    trim_line_ending(line).replace('\t', "    ").trim_end().to_string()
}

fn precise_edit(action: &'static str, line: usize, base: usize, before: &str, after: &str) -> FormatEdit {
    let prefix = common_prefix_bytes(before, after);
    let suffix = common_suffix_bytes(&before[prefix..], &after[prefix..]);
    FormatEdit {
        action, line,
        before: (action != "insert_line").then(|| trim_line_ending(before).to_string()),
        after: (action != "delete_line").then(|| trim_line_ending(after).to_string()),
        start_byte: base + prefix,
        end_byte: base + before.len() - suffix,
        replacement: after[prefix..after.len() - suffix].to_string(),
    }
}

fn common_prefix_bytes(left: &str, right: &str) -> usize {
    left.chars().zip(right.chars()).take_while(|(left, right)| left == right).map(|(c, _)| c.len_utf8()).sum()
}

fn common_suffix_bytes(left: &str, right: &str) -> usize {
    left.chars().rev().zip(right.chars().rev()).take_while(|(left, right)| left == right).map(|(c, _)| c.len_utf8()).sum()
}

fn trim_line_ending(line: &str) -> &str {
    line.strip_suffix('\n').and_then(|line| line.strip_suffix('\r').or(Some(line))).unwrap_or(line)
}
