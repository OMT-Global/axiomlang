//! Bounded JSON scalar parsing and string rendering shared by native lowering.

pub(crate) const JSON_MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const JSON_MAX_DEPTH: usize = 128;
pub(crate) const JSON_MAX_COLLECTION_ITEMS: usize = 100_000;
pub(crate) const JSON_MAX_NUMBER_DIGITS: usize = 1_024;

pub(crate) fn json_parse_int(text: &str) -> Option<i64> {
    text.trim().parse::<i64>().ok()
}

pub(crate) fn json_parse_bool(text: &str) -> Option<bool> {
    match text.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

pub(crate) fn json_parse_string(text: &str) -> Option<String> {
    let text = text.trim();
    if text.len() < 2 || !text.starts_with('"') || !text.ends_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut chars = text[1..text.len() - 1].chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            if ch <= '\u{001f}' {
                return None;
            }
            out.push(ch);
            continue;
        }
        match chars.next()? {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            '/' => out.push('/'),
            'b' => out.push('\u{0008}'),
            'f' => out.push('\u{000C}'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'u' => {
                let mut value = 0u32;
                for _ in 0..4 {
                    value = (value << 4) + chars.next()?.to_digit(16)?;
                }
                if (0xD800..=0xDBFF).contains(&value) {
                    // High surrogate: require a `\uDC00..=\uDFFF` low surrogate
                    // escape and combine, matching the generated-runtime JSON
                    // contract.
                    if chars.next()? != '\\' || chars.next()? != 'u' {
                        return None;
                    }
                    let mut low = 0u32;
                    for _ in 0..4 {
                        low = (low << 4) + chars.next()?.to_digit(16)?;
                    }
                    if !(0xDC00..=0xDFFF).contains(&low) {
                        return None;
                    }
                    let scalar = 0x10000 + ((value - 0xD800) << 10) + (low - 0xDC00);
                    out.push(char::from_u32(scalar)?);
                } else {
                    out.push(char::from_u32(value)?);
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

pub(crate) fn json_skip_ws(text: &str, mut index: usize) -> usize {
    let bytes = text.as_bytes();
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    index
}

pub(crate) fn json_scan_string_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.get(start).copied()? != b'"' {
        return None;
    }
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return Some(index + 1),
            _ => index += 1,
        }
    }
    None
}

pub(crate) fn json_scan_value_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if start >= bytes.len() {
        return None;
    }
    if bytes[start] == b'"' {
        return json_scan_string_end(text, start);
    }
    let mut index = start;
    let mut depth = 0i64;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => index = json_scan_string_end(text, index)?,
            b'{' | b'[' => {
                depth += 1;
                index += 1;
            }
            b'}' | b']' if depth > 0 => {
                depth -= 1;
                index += 1;
            }
            b',' | b'}' if depth == 0 => return Some(index),
            _ => index += 1,
        }
    }
    Some(index)
}

pub(crate) fn json_object_field(text: &str, key: &str) -> Option<String> {
    let text = text.trim();
    let bytes = text.as_bytes();
    if bytes.first().copied()? != b'{' || bytes.last().copied()? != b'}' {
        return None;
    }
    let mut index = 1usize;
    loop {
        index = json_skip_ws(text, index);
        if index >= bytes.len() || bytes[index] == b'}' {
            return None;
        }
        let key_end = json_scan_string_end(text, index)?;
        let found_key = json_parse_string(&text[index..key_end])?;
        index = json_skip_ws(text, key_end);
        if bytes.get(index).copied()? != b':' {
            return None;
        }
        let value_start = json_skip_ws(text, index + 1);
        let value_end = json_scan_value_end(text, value_start)?;
        if found_key == key {
            return Some(text[value_start..value_end].trim().to_string());
        }
        index = json_skip_ws(text, value_end);
        match bytes.get(index).copied()? {
            b',' => index += 1,
            b'}' => return None,
            _ => return None,
        }
    }
}

pub(crate) fn json_parse_value(text: &str) -> Option<String> {
    let text = text.trim();
    let end = json_scan_value_end(text, 0)?;
    if json_skip_ws(text, end) == text.len() {
        Some(text.to_string())
    } else {
        None
    }
}

pub(crate) fn json_escape_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

pub(crate) fn json_escape_string_content(value: &str) -> String {
    let escaped = json_escape_string(value);
    escaped
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(escaped.as_str())
        .to_string()
}
