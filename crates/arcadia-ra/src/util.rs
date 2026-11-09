//! Coerções fiéis ao JavaScript usadas pelo cliente congelado.
//!
//! O cliente Electron usa `String(x || "")`, `Number(x || 0)`,
//! `Boolean(x)` e `String.prototype.trim()`. Reproduzimos essas semânticas
//! (incluindo `+` para espaço na codificação de formulário) para que os
//! envelopes e os valores normalizados sejam idênticos.

use serde_json::Value;

/// `String.prototype.trim()` do JS. Difere de `str::trim` por não remover
/// U+0085 (NEL) e por remover U+FEFF (BOM).
pub(crate) fn js_trim(value: &str) -> &str {
    value.trim_matches(is_js_whitespace)
}

fn is_js_whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{0009}'
            | '\u{000A}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// `Boolean(value)`.
pub(crate) fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().map(|number| number != 0.0).unwrap_or(false),
        Value::String(value) => !value.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `String(value)` para os tipos que a API devolve.
pub(crate) fn js_to_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => {
            if let Some(integer) = value.as_i64() {
                integer.to_string()
            } else if let Some(integer) = value.as_u64() {
                integer.to_string()
            } else if let Some(number) = value.as_f64() {
                js_number_to_string(number)
            } else {
                "NaN".to_string()
            }
        }
        Value::String(value) => value.clone(),
        Value::Array(_) => String::new(),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

fn js_number_to_string(number: f64) -> String {
    if number.is_nan() {
        return "NaN".to_string();
    }
    if number.is_infinite() {
        return if number.is_sign_negative() {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        };
    }
    if number.fract() == 0.0 && number.abs() < 1e21 {
        format!("{number:.0}")
    } else {
        number.to_string()
    }
}

/// `String(value || "")`.
pub(crate) fn js_or_empty_string(value: Option<&Value>) -> String {
    match value {
        Some(value) if js_truthy(value) => js_to_string(value),
        _ => String::new(),
    }
}

/// `String(object.key || "")`.
pub(crate) fn js_field_string(object: &Value, key: &str) -> String {
    js_or_empty_string(object.get(key))
}

/// `Number(value)`.
pub(crate) fn js_number(value: &Value) -> f64 {
    match value {
        Value::Null => 0.0,
        Value::Bool(value) => {
            if *value {
                1.0
            } else {
                0.0
            }
        }
        Value::Number(value) => value.as_f64().unwrap_or(f64::NAN),
        Value::String(value) => js_number_from_str(value),
        Value::Array(_) | Value::Object(_) => f64::NAN,
    }
}

fn js_number_from_str(value: &str) -> f64 {
    let text = js_trim(value);
    if text.is_empty() {
        return 0.0;
    }
    match text {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        "NaN" | "+NaN" | "-NaN" => return f64::NAN,
        _ => {}
    }
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16)
            .map(|value| value as f64)
            .unwrap_or(f64::NAN);
    }
    if let Some(binary) = text.strip_prefix("0b").or_else(|| text.strip_prefix("0B")) {
        return u64::from_str_radix(binary, 2)
            .map(|value| value as f64)
            .unwrap_or(f64::NAN);
    }
    if let Some(octal) = text.strip_prefix("0o").or_else(|| text.strip_prefix("0O")) {
        return u64::from_str_radix(octal, 8)
            .map(|value| value as f64)
            .unwrap_or(f64::NAN);
    }
    let first = text.chars().next().unwrap_or('\0');
    // `f64::from_str` aceita "inf"/"nan" em minúsculas, que `Number()` do JS
    // rejeita. Só entram strings que começam como um literal numérico.
    if !(first.is_ascii_digit() || first == '+' || first == '-' || first == '.') {
        return f64::NAN;
    }
    text.parse::<f64>().unwrap_or(f64::NAN)
}

/// `Number(value || fallback)`.
pub(crate) fn js_number_or(value: Option<&Value>, fallback: f64) -> f64 {
    match value {
        Some(value) if js_truthy(value) => js_number(value),
        _ => fallback,
    }
}

/// `Number(value || fallback)` truncado para `u32` (contadores da API).
pub(crate) fn js_count(value: Option<&Value>, fallback: usize) -> u32 {
    js_number_to_u32(js_number_or(value, fallback as f64))
}

/// `Number(value)` truncado para `u32`.
pub(crate) fn js_number_to_u32(number: f64) -> u32 {
    if !number.is_finite() || number <= 0.0 {
        return 0;
    }
    number as u32
}

/// `Number(value)` truncado para `i64` (`ID` de jogo/conquista).
pub(crate) fn js_number_to_i64(number: f64) -> i64 {
    if !number.is_finite() {
        return 0;
    }
    number as i64
}

/// `Number(object.key || fallback)` truncado para `u32`.
pub(crate) fn js_field_count(object: &Value, key: &str, fallback: usize) -> u32 {
    js_count(object.get(key), fallback)
}

/// `data?.Error || data?.message || ""`, usado só pela API pública.
pub(crate) fn extract_api_error(data: &Value) -> Option<String> {
    if let Some(error) = data.get("Error").filter(|value| js_truthy(value)) {
        return Some(js_to_string(error));
    }
    data.get("message")
        .filter(|value| js_truthy(value))
        .map(js_to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn trim_segue_o_js() {
        assert_eq!(js_trim("  Fulano \n"), "Fulano");
        assert_eq!(js_trim("\u{FEFF} Fulano \u{FEFF}"), "Fulano");
        assert_eq!(js_trim("\u{0085}x\u{0085}"), "\u{0085}x\u{0085}");
    }

    #[test]
    fn truthiness_segue_o_js() {
        assert!(!js_truthy(&json!(null)));
        assert!(!js_truthy(&json!(false)));
        assert!(!js_truthy(&json!(0)));
        assert!(!js_truthy(&json!("")));
        assert!(js_truthy(&json!(1)));
        assert!(js_truthy(&json!("0")));
        assert!(js_truthy(&json!([])));
        assert!(js_truthy(&json!({})));
    }

    #[test]
    fn number_coercion_segue_o_js() {
        assert_eq!(js_number(&json!(42)), 42.0);
        assert_eq!(js_number(&json!(" 42 ")), 42.0);
        assert_eq!(js_number(&json!("")), 0.0);
        assert_eq!(js_number(&json!("0x10")), 16.0);
        assert_eq!(js_number(&json!(true)), 1.0);
        assert!(js_number(&json!("abc")).is_nan());
        assert!(js_number(&json!("inf")).is_nan());
        assert_eq!(js_number(&json!("Infinity")), f64::INFINITY);
    }

    #[test]
    fn or_empty_string_segue_o_js() {
        assert_eq!(js_or_empty_string(Some(&json!("x"))), "x");
        assert_eq!(js_or_empty_string(Some(&json!(0))), "");
        assert_eq!(js_or_empty_string(Some(&json!(7))), "7");
        assert_eq!(js_or_empty_string(None), "");
    }

    #[test]
    fn api_error_prefere_error_e_depois_message() {
        assert_eq!(
            extract_api_error(&json!({ "Error": "Invalid API Key", "message": "x" })),
            Some("Invalid API Key".to_string())
        );
        assert_eq!(
            extract_api_error(&json!({ "message": "Unauthenticated." })),
            Some("Unauthenticated.".to_string())
        );
        assert_eq!(extract_api_error(&json!({ "Error": "" })), None);
    }
}
