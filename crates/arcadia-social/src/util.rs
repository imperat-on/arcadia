//! Helpers de coerção JSON/JS e de arquivos usados por amigos e comunidade.
//!
//! A semântica segue o cliente Electron congelado: `String(...)`, `Number(...)`,
//! truthiness do JavaScript, `encodeURIComponent`, timestamps em milissegundos
//! e escrita atômica (tmp + rename, 0600 no Unix).

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

use arcadia_store::atomic::write_atomic_text;

pub(crate) fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) fn is_js_whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{0009}'
            | '\u{000A}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000D}'
            | ' '
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

pub(crate) fn js_trim(value: &str) -> &str {
    value.trim_matches(is_js_whitespace)
}

pub(crate) fn js_truthy(value: &Value) -> bool {
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

fn js_f64_to_string(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    if !value.is_finite() {
        return if value.is_nan() {
            "NaN".to_string()
        } else if value.is_sign_negative() {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        };
    }
    let absolute = value.abs();
    if value.fract() == 0.0 && absolute < 1e21 {
        return format!("{value:.0}");
    }
    format!("{value}")
}

pub(crate) fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                value.to_string()
            } else if let Some(value) = number.as_u64() {
                value.to_string()
            } else if let Some(value) = number.as_f64() {
                js_f64_to_string(value)
            } else {
                number.to_string()
            }
        }
        Value::String(value) => value.clone(),
        Value::Array(items) => items
            .iter()
            .map(|item| {
                if item.is_null() {
                    String::new()
                } else {
                    js_string(item)
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// `String(item?.id)` — `None` corresponde a `String(undefined)`.
pub(crate) fn js_string_opt(value: Option<&Value>) -> String {
    match value {
        Some(value) => js_string(value),
        None => "undefined".to_string(),
    }
}

/// `Number(value)` para os tipos que aparecem nas opções de paginação.
pub(crate) fn js_number(value: &Value) -> Option<f64> {
    let number = match value {
        Value::Null => 0.0,
        Value::Bool(value) => {
            if *value {
                1.0
            } else {
                0.0
            }
        }
        Value::Number(number) => number.as_f64()?,
        Value::String(value) => {
            let trimmed = js_trim(value);
            if trimmed.is_empty() {
                0.0
            } else if let Some(digits) = trimmed
                .strip_prefix("0x")
                .or_else(|| trimmed.strip_prefix("0X"))
            {
                let mut parsed = 0.0;
                if digits.is_empty() {
                    return None;
                }
                for character in digits.chars() {
                    parsed = parsed * 16.0 + f64::from(character.to_digit(16)?);
                }
                parsed
            } else {
                trimmed.parse::<f64>().ok()?
            }
        }
        Value::Array(items) => match items.len() {
            0 => 0.0,
            1 => js_number(&items[0])?,
            _ => return None,
        },
        Value::Object(_) => return None,
    };
    number.is_finite().then_some(number)
}

pub(crate) fn read_json(path: &Path) -> Option<Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Objeto do arquivo; JSON inválido, ausente ou não-objeto vira objeto vazio.
pub(crate) fn read_json_object(path: &Path) -> Map<String, Value> {
    match read_json(path) {
        Some(Value::Object(object)) => object,
        _ => Map::new(),
    }
}

pub(crate) fn write_json_atomic(path: &Path, value: &Value) -> std::io::Result<()> {
    let serialized = serde_json::to_string(value)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    write_atomic_text(path, &serialized)
}

/// `encodeURIComponent`: mantém `A-Z a-z 0-9 - _ . ! ~ * ' ( )` e codifica os
/// bytes UTF-8 restantes em `%XX` maiúsculo. Espaço vira `%20` (não `+`).
pub(crate) fn encode_uri_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for &byte in value.as_bytes() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            );
        if keep {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
}

/// Objeto espalhado como `{...value}` no JS (arrays viram chaves de índice).
pub(crate) fn spread_into(map: &mut Map<String, Value>, value: &Value) {
    match value {
        Value::Object(object) => {
            for (key, item) in object {
                map.insert(key.clone(), item.clone());
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                map.insert(index.to_string(), item.clone());
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn encode_uri_component_matches_javascript() {
        assert_eq!(encode_uri_component("steam:10"), "steam%3A10");
        assert_eq!(encode_uri_component("id/with space"), "id%2Fwith%20space");
        assert_eq!(
            encode_uri_component("a-b_c.d!e~f*g'h(i)"),
            "a-b_c.d!e~f*g'h(i)"
        );
        assert_eq!(encode_uri_component("ção"), "%C3%A7%C3%A3o");
    }

    #[test]
    fn js_string_coerces_numbers_like_javascript() {
        assert_eq!(js_string(&json!(9)), "9");
        assert_eq!(js_string(&json!(9.0)), "9");
        assert_eq!(js_string(&json!(9.5)), "9.5");
        assert_eq!(js_string(&Value::Null), "null");
        assert_eq!(js_string(&json!(["a", null, "b"])), "a,,b");
        assert_eq!(js_string_opt(None), "undefined");
    }

    #[test]
    fn js_number_handles_arrays_and_hex_like_the_renderer() {
        assert_eq!(js_number(&Value::Null), Some(0.0));
        assert_eq!(js_number(&json!(" 10 ")), Some(10.0));
        assert_eq!(js_number(&json!("")), Some(0.0));
        assert_eq!(js_number(&json!("0x10")), Some(16.0));
        assert_eq!(js_number(&json!([])), Some(0.0));
        assert_eq!(js_number(&json!([5])), Some(5.0));
        assert_eq!(js_number(&json!([1, 2])), None);
        assert_eq!(js_number(&json!({})), None);
    }

    #[test]
    fn spread_matches_object_spread() {
        let mut map = Map::new();
        spread_into(&mut map, &json!({ "a": 1, "b": 2 }));
        assert_eq!(map.get("a"), Some(&json!(1)));
        spread_into(&mut map, &json!(["x", "y"]));
        assert_eq!(map.get("0"), Some(&json!("x")));
        assert_eq!(map.get("1"), Some(&json!("y")));
    }
}
