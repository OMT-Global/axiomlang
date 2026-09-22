//! Generated-Rust JSON serialization support, kept separate from backend orchestration.

pub(super) fn render_json_serdes_support(out: &mut String) {
    out.push_str(
        r##"#[allow(dead_code)]
const AXIOM_JSON_MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;
const AXIOM_JSON_MAX_DEPTH: usize = 128;
const AXIOM_JSON_MAX_COLLECTION_ITEMS: usize = 100_000;
const AXIOM_JSON_MAX_NUMBER_DIGITS: usize = 1_024;

fn axiom_json_serdes_float_to_json(value: f64) -> String {
    if !value.is_finite() {
        return String::from("null");
    }
    let mut rendered = value.to_string();
    if !rendered.contains('.') && !rendered.contains('e') && !rendered.contains('E') {
        rendered.push_str(".0");
    }
    rendered
}

#[allow(dead_code)]
fn axiom_json_serdes_string_to_json(value: String) -> String {
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
            ch if (ch as u32) <= 0x7f => out.push(ch),
            ch if (ch as u32) <= 0xffff => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => {
                let scalar = (ch as u32) - 0x10000;
                let high = 0xd800 + (scalar >> 10);
                let low = 0xdc00 + (scalar & 0x3ff);
                out.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
            }
        }
    }
    out.push('"');
    out
}

#[allow(dead_code)]
fn axiom_json_serdes_value_to_json(value: std_serdes_Value) -> String {
    match value {
        std_serdes_Value::Null => String::from("null"),
        std_serdes_Value::Bool(value) => axiom_json_stringify_bool(value),
        std_serdes_Value::Int(value) => axiom_json_stringify_int(value),
        std_serdes_Value::Float(value) => axiom_json_serdes_float_to_json(value),
        std_serdes_Value::Text(value) => axiom_json_serdes_string_to_json(value),
        std_serdes_Value::Array(values) => {
            let rendered = values
                .into_iter()
                .map(axiom_json_serdes_value_to_json)
                .collect::<Vec<_>>()
                .join(",");
            format!("[{rendered}]")
        }
        std_serdes_Value::Object(values) => axiom_json_serdes_to_json_object(values),
    }
}

#[allow(dead_code)]
fn axiom_json_serdes_to_json_object(values: HashMap<String, std_serdes_Value>) -> String {
    let mut entries = values.into_iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let rendered = entries
        .into_iter()
        .map(|(key, value)| {
            format!(
                "{}:{}",
                axiom_json_serdes_string_to_json(key),
                axiom_json_serdes_value_to_json(value)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{rendered}}}")
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
struct AxiomJsonSerdesError {
    message: String,
    offset: usize,
    path: String,
}

#[allow(dead_code)]
fn axiom_json_serdes_error(message: impl Into<String>, offset: usize, path: &str) -> AxiomJsonSerdesError {
    AxiomJsonSerdesError {
        message: message.into(),
        offset,
        path: path.to_string(),
    }
}

#[allow(dead_code)]
fn axiom_json_serdes_field_path(path: &str, key: &str) -> String {
    if !key.is_empty()
        && key.chars().enumerate().all(|(index, ch)| {
            ch == '_'
                || ch.is_ascii_alphanumeric()
                    && (index > 0 || ch.is_ascii_alphabetic() || ch == '_')
        })
    {
        format!("{path}.{key}")
    } else {
        format!("{path}[{}]", axiom_json_serdes_string_to_json(key.to_string()))
    }
}

#[allow(dead_code)]
fn axiom_json_serdes_index_path(path: &str, index: usize) -> String {
    format!("{path}[{index}]")
}

#[allow(dead_code)]
struct AxiomJsonSerdesParser<'a> {
    text: &'a str,
    index: usize,
}

#[allow(dead_code)]
impl<'a> AxiomJsonSerdesParser<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, index: 0 }
    }

    fn is_end(&self) -> bool {
        self.index >= self.text.len()
    }

    fn peek_byte(&self) -> Option<u8> {
        self.text.as_bytes().get(self.index).copied()
    }

    fn next_byte(&mut self) -> Option<u8> {
        let byte = self.peek_byte()?;
        self.index += 1;
        Some(byte)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek_byte(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.index += 1;
        }
    }

    fn consume_literal(&mut self, literal: &str) -> bool {
        if self.text[self.index..].starts_with(literal) {
            self.index += literal.len();
            true
        } else {
            false
        }
    }

    fn parse_value(
        &mut self,
        depth: usize,
        path: &str,
    ) -> Result<std_serdes_Value, AxiomJsonSerdesError> {
        self.skip_ws();
        match self.peek_byte() {
            Some(b'n') if self.consume_literal("null") => Ok(std_serdes_Value::Null),
            Some(b't') if self.consume_literal("true") => Ok(std_serdes_Value::Bool(true)),
            Some(b'f') if self.consume_literal("false") => Ok(std_serdes_Value::Bool(false)),
            Some(b'"') => Ok(std_serdes_Value::Text(self.parse_string(path)?)),
            Some(b'[') => self.parse_array(depth, path),
            Some(b'{') => self.parse_object(depth, path),
            Some(b'-' | b'0'..=b'9') => self.parse_number(path),
            Some(_) => Err(axiom_json_serdes_error(
                "unexpected JSON token",
                self.index,
                path,
            )),
            None => Err(axiom_json_serdes_error("empty JSON input", self.index, path)),
        }
    }

    fn parse_array(
        &mut self,
        depth: usize,
        path: &str,
    ) -> Result<std_serdes_Value, AxiomJsonSerdesError> {
        if depth >= AXIOM_JSON_MAX_DEPTH {
            return Err(axiom_json_serdes_error(
                format!("JSON nesting exceeds {} level limit", AXIOM_JSON_MAX_DEPTH),
                self.index,
                path,
            ));
        }
        self.expect_byte(b'[', "array", path)?;
        self.skip_ws();
        let mut values = Vec::new();
        if self.peek_byte() == Some(b']') {
            self.index += 1;
            return Ok(std_serdes_Value::Array(values));
        }
        loop {
            if values.len() >= AXIOM_JSON_MAX_COLLECTION_ITEMS {
                return Err(axiom_json_serdes_error(
                    format!(
                        "JSON collection exceeds {} item limit",
                        AXIOM_JSON_MAX_COLLECTION_ITEMS
                    ),
                    self.index,
                    path,
                ));
            }
            let value_path = axiom_json_serdes_index_path(path, values.len());
            values.push(self.parse_value(depth + 1, &value_path)?);
            self.skip_ws();
            let delimiter_offset = self.index;
            match self.next_byte() {
                Some(b',') => {
                    self.skip_ws();
                }
                Some(b']') => return Ok(std_serdes_Value::Array(values)),
                _ => {
                    return Err(axiom_json_serdes_error(
                        "array expects ',' or ']'",
                        delimiter_offset,
                        path,
                    ));
                }
            }
        }
    }

    fn parse_object(
        &mut self,
        depth: usize,
        path: &str,
    ) -> Result<std_serdes_Value, AxiomJsonSerdesError> {
        if depth >= AXIOM_JSON_MAX_DEPTH {
            return Err(axiom_json_serdes_error(
                format!("JSON nesting exceeds {} level limit", AXIOM_JSON_MAX_DEPTH),
                self.index,
                path,
            ));
        }
        self.expect_byte(b'{', "object", path)?;
        self.skip_ws();
        let mut values = HashMap::new();
        if self.peek_byte() == Some(b'}') {
            self.index += 1;
            return Ok(std_serdes_Value::Object(values));
        }
        loop {
            if values.len() >= AXIOM_JSON_MAX_COLLECTION_ITEMS {
                return Err(axiom_json_serdes_error(
                    format!(
                        "JSON collection exceeds {} item limit",
                        AXIOM_JSON_MAX_COLLECTION_ITEMS
                    ),
                    self.index,
                    path,
                ));
            }
            self.skip_ws();
            let key = self.parse_string(path)?;
            let value_path = axiom_json_serdes_field_path(path, &key);
            self.skip_ws();
            self.expect_byte(b':', "object field", &value_path)?;
            let value = self.parse_value(depth + 1, &value_path)?;
            values.insert(key, value);
            self.skip_ws();
            let delimiter_offset = self.index;
            match self.next_byte() {
                Some(b',') => {
                    self.skip_ws();
                }
                Some(b'}') => return Ok(std_serdes_Value::Object(values)),
                _ => {
                    return Err(axiom_json_serdes_error(
                        "object expects ',' or '}'",
                        delimiter_offset,
                        path,
                    ));
                }
            }
        }
    }

    fn parse_number(
        &mut self,
        path: &str,
    ) -> Result<std_serdes_Value, AxiomJsonSerdesError> {
        let start = self.index;
        if self.peek_byte() == Some(b'-') {
            self.index += 1;
        }
        match self.peek_byte() {
            Some(b'0') => {
                self.index += 1;
            }
            Some(b'1'..=b'9') => {
                self.index += 1;
                self.consume_digits();
            }
            _ => return Err(axiom_json_serdes_error("invalid JSON number", start, path)),
        }
        let mut is_float = false;
        if self.peek_byte() == Some(b'.') {
            is_float = true;
            self.index += 1;
            if self.consume_digits() == 0 {
                return Err(axiom_json_serdes_error("invalid JSON fraction", start, path));
            }
        }
        if matches!(self.peek_byte(), Some(b'e' | b'E')) {
            is_float = true;
            self.index += 1;
            if matches!(self.peek_byte(), Some(b'+' | b'-')) {
                self.index += 1;
            }
            if self.consume_digits() == 0 {
                return Err(axiom_json_serdes_error("invalid JSON exponent", start, path));
            }
        }
        let raw = &self.text[start..self.index];
        if raw.bytes().filter(u8::is_ascii_digit).count() > AXIOM_JSON_MAX_NUMBER_DIGITS {
            return Err(axiom_json_serdes_error(
                format!(
                    "JSON number exceeds {} digit limit",
                    AXIOM_JSON_MAX_NUMBER_DIGITS
                ),
                start,
                path,
            ));
        }
        if is_float {
            let value = raw
                .parse::<f64>()
                .map_err(|_| axiom_json_serdes_error("invalid JSON float", start, path))?;
            if !value.is_finite() {
                return Err(axiom_json_serdes_error("non-finite JSON float", start, path));
            }
            Ok(std_serdes_Value::Float(value))
        } else {
            raw.parse::<i64>()
                .map(std_serdes_Value::Int)
                .map_err(|_| axiom_json_serdes_error("invalid JSON int", start, path))
        }
    }

    fn parse_string(&mut self, path: &str) -> Result<String, AxiomJsonSerdesError> {
        self.expect_byte(b'"', "string", path)?;
        let mut value = String::new();
        loop {
            let Some(ch) = self.text[self.index..].chars().next() else {
                return Err(axiom_json_serdes_error(
                    "unterminated JSON string",
                    self.index,
                    path,
                ));
            };
            self.index += ch.len_utf8();
            match ch {
                '"' => return Ok(value),
                '\\' => self.parse_escape(&mut value, path)?,
                ch if ch <= '\u{1f}' => {
                    return Err(axiom_json_serdes_error(
                        "control character in JSON string",
                        self.index.saturating_sub(ch.len_utf8()),
                        path,
                    ));
                }
                ch => value.push(ch),
            }
        }
    }

    fn parse_escape(
        &mut self,
        value: &mut String,
        path: &str,
    ) -> Result<(), AxiomJsonSerdesError> {
        match self.next_byte() {
            Some(b'"') => value.push('"'),
            Some(b'\\') => value.push('\\'),
            Some(b'/') => value.push('/'),
            Some(b'b') => value.push('\u{0008}'),
            Some(b'f') => value.push('\u{000C}'),
            Some(b'n') => value.push('\n'),
            Some(b'r') => value.push('\r'),
            Some(b't') => value.push('\t'),
            Some(b'u') => {
                let high = self.parse_hex4(path)?;
                if (0xD800..=0xDBFF).contains(&high) {
                    if self.next_byte() != Some(b'\\') || self.next_byte() != Some(b'u') {
                        return Err(axiom_json_serdes_error(
                            "missing low surrogate escape",
                            self.index,
                            path,
                        ));
                    }
                    let low = self.parse_hex4(path)?;
                    if !(0xDC00..=0xDFFF).contains(&low) {
                        return Err(axiom_json_serdes_error(
                            "invalid low surrogate escape",
                            self.index,
                            path,
                        ));
                    }
                    let scalar =
                        0x10000 + (((high as u32) - 0xD800) << 10) + ((low as u32) - 0xDC00);
                    value.push(
                        char::from_u32(scalar)
                            .ok_or_else(|| axiom_json_serdes_error("invalid unicode scalar", self.index, path))?,
                    );
                } else if (0xDC00..=0xDFFF).contains(&high) {
                    return Err(axiom_json_serdes_error(
                        "unpaired low surrogate escape",
                        self.index,
                        path,
                    ));
                } else {
                    value.push(
                        char::from_u32(high as u32)
                            .ok_or_else(|| axiom_json_serdes_error("invalid unicode escape", self.index, path))?,
                    );
                }
            }
            _ => return Err(axiom_json_serdes_error("invalid JSON string escape", self.index, path)),
        }
        Ok(())
    }

    fn parse_hex4(&mut self, path: &str) -> Result<u16, AxiomJsonSerdesError> {
        let mut value = 0u16;
        for _ in 0..4 {
            let digit = self
                .next_byte()
                .and_then(|byte| (byte as char).to_digit(16))
                .ok_or_else(|| axiom_json_serdes_error("invalid unicode escape", self.index, path))?;
            value = (value << 4) + digit as u16;
        }
        Ok(value)
    }

    fn expect_byte(
        &mut self,
        expected: u8,
        context: &str,
        path: &str,
    ) -> Result<(), AxiomJsonSerdesError> {
        let offset = self.index;
        match self.next_byte() {
            Some(actual) if actual == expected => Ok(()),
            _ => Err(axiom_json_serdes_error(
                format!("{context} expects '{}'", expected as char),
                offset,
                path,
            )),
        }
    }

    fn consume_digits(&mut self) -> usize {
        let start = self.index;
        while matches!(self.peek_byte(), Some(b'0'..=b'9')) {
            self.index += 1;
        }
        self.index - start
    }
}

#[allow(dead_code)]
fn axiom_json_serdes_parse(text: String) -> Result<std_serdes_Value, std_serdes_ParseError> {
    axiom_json_serdes_parse_str(text.as_str())
}

#[allow(dead_code)]
fn axiom_json_serdes_parse_str(text: &str) -> Result<std_serdes_Value, std_serdes_ParseError> {
    if text.len() > AXIOM_JSON_MAX_DOCUMENT_BYTES {
        return Err(std_serdes_ParseError {
            message: format!(
                "JSON document exceeds {} byte limit",
                AXIOM_JSON_MAX_DOCUMENT_BYTES
            ),
            offset: AXIOM_JSON_MAX_DOCUMENT_BYTES as i64,
            path: String::from("$"),
        });
    }
    let mut parser = AxiomJsonSerdesParser::new(text);
    match parser.parse_value(0, "$") {
        Ok(value) => {
            parser.skip_ws();
            if parser.is_end() {
                Ok(value)
            } else {
                Err(std_serdes_ParseError {
                    message: String::from("trailing characters after JSON value"),
                    offset: parser.index as i64,
                    path: String::from("$"),
                })
            }
        }
        Err(error) => Err(std_serdes_ParseError {
            message: error.message,
            offset: error.offset as i64,
            path: error.path,
        }),
    }
}

"##,
    );
}
