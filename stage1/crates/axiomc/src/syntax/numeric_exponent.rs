//! Tells a float exponent sign (`1e-3`, `2.5E+4`) apart from a binary `+`/`-`
//! that follows an identifier ending in `e`/`E`, such as `value-1` (#1760).

/// Whether the `+` or `-` at byte offset `index` in `raw` is the sign of a
/// float exponent. It must directly follow a decimal mantissa and `e`/`E`,
/// with no whitespace, and be directly followed by a digit.
pub(super) fn is_exponent_sign(raw: &str, index: usize, sign: char) -> bool {
    if !matches!(sign, '+' | '-') {
        return false;
    }
    let Some(mantissa) = raw[..index].strip_suffix(['e', 'E']) else {
        return false;
    };
    if !raw[index + sign.len_utf8()..].starts_with(|ch: char| ch.is_ascii_digit()) {
        return false;
    }
    let start = mantissa
        .char_indices()
        .rev()
        .find(|(_, ch)| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.')))
        .map_or(0, |(position, ch)| position + ch.len_utf8());
    let token = &mantissa[start..];
    token.starts_with(|ch: char| ch.is_ascii_digit())
        && token
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '_' | '.'))
}

#[cfg(test)]
mod tests {
    use super::is_exponent_sign;

    fn sign_at(raw: &str) -> bool {
        let index = raw.rfind(['+', '-']).expect("test input has a sign");
        is_exponent_sign(raw, index, raw[index..].chars().next().unwrap())
    }

    #[test]
    fn exponent_signs_after_decimal_mantissas() {
        for raw in ["1e-3", "1e+2", "2.5E-4", "1_000e+3", "x * 1.0e-2"] {
            assert!(sign_at(raw), "{raw:?} should be an exponent sign");
        }
    }

    #[test]
    fn binary_operators_after_identifiers_or_spacing() {
        for raw in [
            "value-1",
            "value - 1",
            "e-1",
            "E+2",
            "line-2",
            "size_e-1",
            "1e - 2",
            "1e+f64",
            "1e-",
            "t.e-1",
            "0x1e-1",
        ] {
            assert!(!sign_at(raw), "{raw:?} should not be an exponent sign");
        }
    }
}
