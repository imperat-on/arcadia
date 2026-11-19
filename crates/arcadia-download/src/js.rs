//! Semântica mínima de conversões JavaScript usada pelos módulos puros.
//!
//! O oráculo em `app/electron` usa `Number()`, `String()`, `Math.trunc`,
//! `Math.round`, `||` e `.trim()` em valores vindos de JSON/IPC. Esses
//! auxiliares reproduzem esses casos sem depender de Node.

use serde_json::Value;

/// Equivalente a `Number(value)`; `None` representa `NaN`.
pub(crate) fn js_number(value: &Value) -> Option<f64> {
    match value {
        Value::Null => Some(0.0),
        Value::Bool(value) => Some(if *value { 1.0 } else { 0.0 }),
        Value::Number(number) => number.as_f64(),
        Value::String(text) => parse_number(text),
        // Number([]) === 0; Number([x]) === Number(x); arrays maiores são NaN.
        Value::Array(items) => match items.as_slice() {
            [] => Some(0.0),
            [item] => js_number(item),
            _ => None,
        },
        Value::Object(_) => None,
    }
}

fn parse_number(text: &str) -> Option<f64> {
    let trimmed = js_trim(text);
    if trimmed.is_empty() {
        return Some(0.0);
    }
    let lower = trimmed.to_ascii_lowercase();
    match lower.as_str() {
        "infinity" | "+infinity" => return Some(f64::INFINITY),
        "-infinity" => return Some(f64::NEG_INFINITY),
        "nan" => return None,
        _ => {}
    }
    // Number() aceita literais hex/bin/oct sem sinal.
    for (prefix, radix) in [("0x", 16), ("0b", 2), ("0o", 8)] {
        if let Some(digits) = trimmed.strip_prefix(prefix) {
            if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
                return None;
            }
            return u64::from_str_radix(digits, radix).ok().map(|n| n as f64);
        }
        let upper = prefix.to_ascii_uppercase();
        if let Some(digits) = trimmed.strip_prefix(&upper) {
            if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
                return None;
            }
            return u64::from_str_radix(digits, radix).ok().map(|n| n as f64);
        }
    }

    let mut normalized = trimmed.to_string();
    if normalized.starts_with('.') {
        normalized.insert(0, '0');
    }
    if normalized.starts_with("-.") {
        normalized.insert(1, '0');
    }
    if normalized.starts_with("+.") {
        normalized.insert(1, '0');
    }
    if normalized.ends_with('.') {
        normalized.push('0');
    }
    normalized.parse::<f64>().ok().filter(|n| !n.is_nan())
}

/// `Math.trunc`, com saturação na conversão para inteiro (como o cast Rust).
pub(crate) fn js_trunc(value: f64) -> i64 {
    value.trunc() as i64
}

pub(crate) fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// Equivalente a `String(value)` para os tipos que aparecem no oráculo.
pub(crate) fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(true) => "true".to_string(),
        Value::Bool(false) => "false".to_string(),
        Value::Number(number) => number
            .as_f64()
            .map(number_to_string)
            .unwrap_or_else(|| number.to_string()),
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => js_string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

fn number_to_string(number: f64) -> String {
    if number.is_nan() {
        return "NaN".to_string();
    }
    if number == f64::INFINITY {
        return "Infinity".to_string();
    }
    if number == f64::NEG_INFINITY {
        return "-Infinity".to_string();
    }
    if number == 0.0 {
        return "0".to_string();
    }
    if number.fract() == 0.0 && number.abs() < 1e21 {
        return format!("{number:.0}");
    }
    format!("{number}")
}

pub(crate) fn is_js_whitespace(c: char) -> bool {
    c.is_whitespace() || c == '\u{FEFF}'
}

pub(crate) fn js_trim(text: &str) -> String {
    text.trim_matches(is_js_whitespace).to_string()
}

/// `.trim().replace(/\s+/g, " ")` do oráculo.
pub(crate) fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if is_js_whitespace(c) {
            in_space = true;
        } else {
            if in_space && !out.is_empty() {
                out.push(' ');
            }
            in_space = false;
            out.push(c);
        }
    }
    out
}

/// `.slice(0, 300)` — o JS corta em unidades UTF-16, não em chars.
pub(crate) fn truncate_utf16(text: &str, max: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().take(max).collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn number_conversion_matches_javascript_cases() {
        assert_eq!(js_number(&json!("4.9")), Some(4.9));
        assert_eq!(js_number(&json!("")), Some(0.0));
        assert_eq!(js_number(&json!(null)), Some(0.0));
        assert_eq!(js_number(&json!(true)), Some(1.0));
        assert_eq!(js_number(&json!("0x10")), Some(16.0));
        assert_eq!(js_number(&json!("nan")), None);
        assert_eq!(js_number(&json!([7])), Some(7.0));
        assert_eq!(js_number(&json!([])), Some(0.0));
        assert_eq!(js_number(&json!([1, 2])), None);
    }

    #[test]
    fn whitespace_and_truncation_follow_javascript() {
        assert_eq!(collapse_whitespace("  a\n\t b  "), "a b");
        assert_eq!(truncate_utf16("abcdef", 3), "abc");
        // Emoji ocupa duas unidades UTF-16: o corte pode partir o par.
        assert_eq!(truncate_utf16("a😀b", 2), "a\u{FFFD}");
    }
}
