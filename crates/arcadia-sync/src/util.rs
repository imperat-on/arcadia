//! Helpers de coerção JSON/JS e de arquivo usados pelo sync.
//!
//! A semântica segue o cliente Electron: `String(...)`, `Number(...)`,
//! `.trim()`/truthiness, epoch em segundos e escrita atômica (tmp + rename).

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Number, Value};

use arcadia_store::atomic::write_atomic_text;

pub(crate) fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

pub(crate) fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) fn js_trim(value: &str) -> &str {
    value.trim_matches(|character: char| {
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
    })
}

pub(crate) fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(number) => number.to_string(),
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

/// `Number(value)` para os tipos que aparecem nos arquivos do sync.
pub(crate) fn js_number(value: &Value) -> Option<f64> {
    let parsed = match value {
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
            } else {
                trimmed.parse::<f64>().ok()?
            }
        }
        Value::Array(_) => js_string(value).trim().parse::<f64>().ok()?,
        Value::Object(_) => return None,
    };
    parsed.is_finite().then_some(parsed)
}

/// `Number(value) || 0`.
pub(crate) fn js_number_or_zero(value: Option<&Value>) -> f64 {
    value.and_then(js_number).unwrap_or(0.0)
}

pub(crate) fn number_value(value: f64) -> Value {
    if value.is_finite()
        && value.fract() == 0.0
        && value >= i64::MIN as f64
        && value <= i64::MAX as f64
    {
        return Value::from(value as i64);
    }
    Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

/// Fatia como `String(value).slice(0, max)`, contando unidades UTF-16.
pub(crate) fn js_slice_utf16(value: &str, max: usize) -> String {
    if value.encode_utf16().count() <= max {
        return value.to_string();
    }
    let mut units = 0_usize;
    let mut out = String::new();
    for character in value.chars() {
        let width = character.len_utf16();
        if units + width > max {
            break;
        }
        units += width;
        out.push(character);
    }
    out
}

pub(crate) fn read_json(path: &Path) -> Option<Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub(crate) fn read_object(path: &Path) -> Map<String, Value> {
    match read_json(path) {
        Some(Value::Object(object)) => object,
        _ => Map::new(),
    }
}

pub(crate) fn read_array_or_empty(path: &Path) -> Vec<Value> {
    match read_json(path) {
        Some(Value::Array(items)) => items,
        _ => Vec::new(),
    }
}

pub(crate) fn write_json_atomic(path: &Path, value: &Value, pretty: bool) -> std::io::Result<()> {
    let serialized = if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    }
    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    write_atomic_text(path, &serialized)
}

/// `new Date(seconds * 1000).toISOString()`.
pub(crate) fn iso_from_epoch_seconds(seconds: i64) -> Option<String> {
    if seconds < 0 {
        return None;
    }
    let days = seconds / 86_400;
    let second_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    if !(0..=9999).contains(&year) {
        return None;
    }
    let hour = second_of_day / 3_600;
    let minute = (second_of_day % 3_600) / 60;
    let second = second_of_day % 60;
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.000Z"
    ))
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    (
        if month <= 2 { year + 1 } else { year },
        month as u32,
        day as u32,
    )
}

/// `appid` de uma linha do servidor, sem exigir que seja string.
pub(crate) fn row_appid(row: &Value) -> Option<String> {
    let value = row.get("appid").or_else(|| row.get("id"))?;
    if value.is_null() {
        return None;
    }
    let text = js_string(value);
    (!js_trim(&text).is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn iso_matches_js_to_iso_string() {
        assert_eq!(
            iso_from_epoch_seconds(0).as_deref(),
            Some("1970-01-01T00:00:00.000Z")
        );
        assert_eq!(
            iso_from_epoch_seconds(1_700_000_000).as_deref(),
            Some("2023-11-14T22:13:20.000Z")
        );
        assert_eq!(
            iso_from_epoch_seconds(86_400).as_deref(),
            Some("1970-01-02T00:00:00.000Z")
        );
    }

    #[test]
    fn js_number_coercion_matches_number_constructor() {
        assert_eq!(js_number(&json!(" 30 ")), Some(30.0));
        assert_eq!(js_number(&json!("")), Some(0.0));
        assert_eq!(js_number(&json!(true)), Some(1.0));
        assert_eq!(js_number(&json!(null)), Some(0.0));
        assert_eq!(js_number(&json!("1e2")), Some(100.0));
        assert_eq!(js_number(&json!("abc")), None);
    }
}
