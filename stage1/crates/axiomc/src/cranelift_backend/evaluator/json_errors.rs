//! Structured JSON error values and document boundary validation.

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JsonSerdesError {
    pub(crate) message: String,
    pub(crate) offset: usize,
    pub(crate) path: String,
}

pub(crate) fn json_serdes_error(
    message: impl Into<String>,
    offset: usize,
    path: &str,
) -> JsonSerdesError {
    JsonSerdesError {
        message: message.into(),
        offset,
        path: path.to_string(),
    }
}

pub(crate) fn json_serdes_field_path(path: &str, key: &str) -> String {
    if !key.is_empty()
        && key.chars().enumerate().all(|(index, ch)| {
            ch == '_'
                || ch.is_ascii_alphanumeric()
                    && (index > 0 || ch.is_ascii_alphabetic() || ch == '_')
        })
    {
        format!("{path}.{key}")
    } else {
        format!("{path}[{}]", json_serdes_escape_string(key))
    }
}

pub(crate) fn json_serdes_index_path(path: &str, index: usize) -> String {
    format!("{path}[{index}]")
}

pub(crate) fn json_serdes_result(value: Result<SpikeValue, JsonSerdesError>) -> SpikeValue {
    match value {
        Ok(value) => SpikeValue::Enum {
            enum_name: String::from("Result"),
            variant: String::from("Ok"),
            field_names: Vec::new(),
            payloads: vec![value],
        },
        Err(error) => SpikeValue::Enum {
            enum_name: String::from("Result"),
            variant: String::from("Err"),
            field_names: Vec::new(),
            payloads: vec![SpikeValue::Struct {
                name: String::from("std_serdes_ParseError"),
                fields: vec![
                    (String::from("message"), SpikeValue::Text(error.message)),
                    (String::from("offset"), SpikeValue::Int(error.offset as i64)),
                    (String::from("path"), SpikeValue::Text(error.path)),
                ],
            }],
        },
    }
}

pub(crate) fn json_serdes_parse_document(text: &str) -> Result<SpikeValue, JsonSerdesError> {
    if text.len() > JSON_MAX_DOCUMENT_BYTES {
        return Err(json_serdes_error(
            format!("JSON document exceeds {} byte limit", JSON_MAX_DOCUMENT_BYTES),
            JSON_MAX_DOCUMENT_BYTES,
            "$",
        ));
    }
    let (value, index) = json_serdes_parse_value(text, json_skip_ws(text, 0), 0, "$")?;
    if json_skip_ws(text, index) == text.len() {
        Ok(value)
    } else {
        Err(json_serdes_error(
            "trailing characters after JSON value",
            json_skip_ws(text, index),
            "$",
        ))
    }
}

