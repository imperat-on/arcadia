//! Semântica mínima de conversões JavaScript usada pelo oráculo.
//!
//! O cliente Electron compara versões com `parseInt`/`||`, extrai tamanhos com
//! `Number(...)` e serializa mensagens de erro com `String(e?.message || e ||
//! "erro")`. Estes auxiliares reproduzem esses casos sem depender de Node.

use serde_json::Value;

/// `String(value)` para os tipos que aparecem no oráculo.
pub(crate) fn string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(number) => number
            .as_f64()
            .map(number_to_string)
            .unwrap_or_else(|| number.to_string()),
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

fn number_to_string(number: f64) -> String {
    if number == 0.0 {
        return "0".to_string();
    }
    if number.is_finite() && number.fract() == 0.0 && number.abs() < 1e21 {
        return format!("{}", number as i128);
    }
    number.to_string()
}

/// `String(value || "")`.
pub(crate) fn string_or_empty(value: Option<&Value>) -> String {
    match value {
        Some(value) if truthy(value) => string(value),
        _ => String::new(),
    }
}

/// `Boolean(value)`.
pub(crate) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `Number(value)`; `None` representa `NaN`.
pub(crate) fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Null => Some(0.0),
        Value::Bool(value) => Some(if *value { 1.0 } else { 0.0 }),
        Value::Number(number) => number.as_f64(),
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                Some(0.0)
            } else {
                trimmed
                    .parse::<f64>()
                    .ok()
                    .filter(|number| !number.is_nan())
            }
        }
        Value::Array(items) => match items.as_slice() {
            [] => Some(0.0),
            [item] => number(item),
            _ => None,
        },
        Value::Object(_) => None,
    }
}
