//! Semântica mínima de conversões JavaScript usada pelos módulos puros.
//!
//! O cliente congelado em `app/electron` lê strings de VDF e as converte com
//! `Number(...)`, `String(...)`, `||` e `.trim()`. Esses auxiliares reproduzem
//! os casos que aparecem nos arquivos da Steam (e nos payloads do servidor)
//! sem depender de Node.

use serde_json::Value;

/// `String.prototype.trim` (inclui o BOM, que o `trim` do JS remove).
pub(crate) fn js_trim(value: &str) -> &str {
    value.trim_matches(|character: char| character.is_whitespace() || character == '\u{feff}')
}

/// Equivalente a `Number(texto)`; `None` representa `NaN`.
pub(crate) fn js_number_str(value: &str) -> Option<f64> {
    let trimmed = js_trim(value);
    if trimmed.is_empty() {
        // Number("") === 0
        return Some(0.0);
    }
    // Number("Infinity") é case-sensitive; "INFINITY"/"infinity" são NaN.
    match trimmed {
        "Infinity" | "+Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    if trimmed.eq_ignore_ascii_case("nan") {
        return None;
    }
    // O parser de f64 do Rust aceita "inf"/"infinity" em qualquer caixa;
    // o Number() do JS não.
    let unsigned = trimmed
        .strip_prefix('-')
        .or_else(|| trimmed.strip_prefix('+'))
        .unwrap_or(trimmed);
    if unsigned.eq_ignore_ascii_case("inf") || unsigned.eq_ignore_ascii_case("infinity") {
        return None;
    }

    // Number() aceita literais hex/bin/oct sem sinal.
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        for candidate in [prefix.to_string(), prefix.to_ascii_uppercase()] {
            if let Some(digits) = trimmed.strip_prefix(candidate.as_str()) {
                if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
                    return None;
                }
                return u64::from_str_radix(digits, radix).ok().map(|n| n as f64);
            }
        }
    }

    let mut normalized = trimmed.to_string();
    if normalized.starts_with('.') {
        normalized.insert(0, '0');
    } else if normalized.starts_with("-.") || normalized.starts_with("+.") {
        normalized.insert(1, '0');
    }
    if normalized.ends_with('.') {
        normalized.push('0');
    }
    normalized
        .parse::<f64>()
        .ok()
        .filter(|number| !number.is_nan())
}

/// Equivalente a `Number(value)` para os tipos que aparecem em JSON.
pub(crate) fn js_number_value(value: &Value) -> Option<f64> {
    match value {
        Value::Null => Some(0.0),
        Value::Bool(value) => Some(if *value { 1.0 } else { 0.0 }),
        Value::Number(number) => number.as_f64(),
        Value::String(text) => js_number_str(text),
        Value::Array(items) => js_number_str(&js_array_join(items)),
        Value::Object(_) => None,
    }
}

/// Equivalente a `String(value)`; arrays usam a regra de `Array.join(",")`.
pub(crate) fn js_string_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(true) => "true".to_string(),
        Value::Bool(false) => "false".to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Array(items) => js_array_join(items),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// `Array.prototype.join(",")`: `null`/`undefined` viram vazio.
fn js_array_join(items: &[Value]) -> String {
    items
        .iter()
        .map(|item| match item {
            Value::Null => String::new(),
            other => js_string_value(other),
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Equivalente ao teste de veracidade do JS.
pub(crate) fn js_truthy_value(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number
            .as_f64()
            .map(|value| value != 0.0 && !value.is_nan())
            .unwrap_or(true),
        Value::String(value) => !value.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn number_coercion_matches_the_number_constructor() {
        assert_eq!(js_number_str(""), Some(0.0));
        assert_eq!(js_number_str("  42  "), Some(42.0));
        assert_eq!(js_number_str("+7"), Some(7.0));
        assert_eq!(js_number_str("0x10"), Some(16.0));
        assert_eq!(js_number_str("0b101"), Some(5.0));
        assert_eq!(js_number_str("0o17"), Some(15.0));
        assert_eq!(js_number_str("1e3"), Some(1000.0));
        assert_eq!(js_number_str(".5"), Some(0.5));
        assert_eq!(js_number_str("5."), Some(5.0));
        assert_eq!(js_number_str("Infinity"), Some(f64::INFINITY));
        assert_eq!(js_number_str("-Infinity"), Some(f64::NEG_INFINITY));
        assert_eq!(js_number_str("infinity"), None);
        assert_eq!(js_number_str("INFINITY"), None);
        assert_eq!(js_number_str("abc"), None);
        assert_eq!(js_number_str("0x"), None);
        assert_eq!(js_number_str("NaN"), None);
    }

    #[test]
    fn strings_and_truthiness_follow_javascript() {
        assert_eq!(js_string_value(&json!(null)), "null");
        assert_eq!(js_string_value(&json!(1.5)), "1.5");
        assert_eq!(js_string_value(&json!(["a", null, "b"])), "a,,b");
        assert_eq!(js_string_value(&json!({ "a": 1 })), "[object Object]");
        assert!(!js_truthy_value(&json!(0)));
        assert!(!js_truthy_value(&json!("")));
        assert!(js_truthy_value(&json!([])));
        assert_eq!(js_number_value(&json!(["5"])), Some(5.0));
        assert_eq!(js_number_value(&json!(["5", "6"])), None);
        assert_eq!(js_number_value(&json!({})), None);
    }

    #[test]
    fn trim_removes_the_bom_like_javascript() {
        assert_eq!(js_trim("\u{feff} olá \u{feff}"), "olá");
    }
}
