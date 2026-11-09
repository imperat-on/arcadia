//! Helpers que reproduzem as coerções numéricas/string do JavaScript usadas
//! pelos módulos puros.
//!
//! O cliente congelado em `app/electron` lê strings de VDF e as converte com
//! `Number(...)`, `String(...)`, `||` e `.trim()`. Estes auxiliares reproduzem
//! os casos que aparecem nos arquivos da Steam (e nos payloads do servidor)
//! sem depender de Node.
//!
//! Este módulo é usado por [`crate::paths`]; os demais módulos têm a própria
//! camada `js` com a mesma semântica. `#[allow(dead_code)]` cobre os helpers
//! que só os testes exercitam.

use serde_json::Value;

/// `Number(texto)` para os formatos que aparecem em VDF e nas linhas do
/// servidor. Devolve `None` para o que o JS chamaria de `NaN`.
pub(crate) fn js_number_str(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() {
        return Some(0.0);
    }

    let (negative, unsigned) = if let Some(rest) = text.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = text.strip_prefix('+') {
        (false, rest)
    } else {
        (false, text)
    };

    if unsigned.is_empty() {
        return None;
    }

    let (radix, digits) = if let Some(rest) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        (16_u32, rest)
    } else if let Some(rest) = unsigned
        .strip_prefix("0b")
        .or_else(|| unsigned.strip_prefix("0B"))
    {
        (2_u32, rest)
    } else if let Some(rest) = unsigned
        .strip_prefix("0o")
        .or_else(|| unsigned.strip_prefix("0O"))
    {
        (8_u32, rest)
    } else {
        (0_u32, unsigned)
    };

    if radix != 0 {
        // O JS não aceita sinal em literal hexadecimal/binário/octal.
        if text.starts_with('-') || text.starts_with('+') || digits.is_empty() {
            return None;
        }
        let value = u128::from_str_radix(digits, radix).ok()?;
        return Some(value as f64);
    }

    if unsigned == "Infinity" {
        return Some(f64::INFINITY);
    }
    if unsigned.eq_ignore_ascii_case("inf") || unsigned.eq_ignore_ascii_case("infinity") {
        return None;
    }
    if unsigned.eq_ignore_ascii_case("nan") {
        return None;
    }

    let value = unsigned.parse::<f64>().ok()?;
    Some(if negative { -value } else { value })
}

/// `Number(value)` para valores JSON. `null` vira `0`; objetos/arrays viram
/// `None` (o JS daria `NaN`, exceto arrays de um elemento — caso que não
/// aparece nos payloads da Steam).
pub(crate) fn js_number_value(value: &Value) -> Option<f64> {
    match value {
        Value::Null => Some(0.0),
        Value::Bool(true) => Some(1.0),
        Value::Bool(false) => Some(0.0),
        Value::Number(number) => number.as_f64(),
        Value::String(text) => js_number_str(text),
        Value::Array(_) | Value::Object(_) => None,
    }
}

/// `String(value || fallback)` para os campos de notícia.
pub(crate) fn js_string_or(value: &Value, fallback: &str) -> String {
    if js_truthy(value) {
        js_string(value)
    } else {
        fallback.to_string()
    }
}

/// `String(value)` para os tipos que a API da Steam devolve.
pub(crate) fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(true) => "true".to_string(),
        Value::Bool(false) => "false".to_string(),
        Value::Number(number) => number.to_string(),
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

/// `Boolean(value)` para os tipos JSON (0, "", null, false e NaN são falsos).
pub(crate) fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64().is_some_and(|value| value != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `JSON.stringify(number)`: inteiros saem sem `.0`; NaN/Infinity viram
/// `null` (mesmo comportamento do `JSON.stringify`).
pub(crate) fn js_number_json(value: f64) -> Value {
    if !value.is_finite() {
        return Value::Null;
    }
    if value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
        return Value::Number(serde_json::Number::from(value as i64));
    }
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

/// Serializa um `f64` como número JSON no formato do JS (sem `.0` quando
/// inteiro). Usado pelo `Serialize` dos itens de horas.
pub(crate) fn serialize_js_number<S>(value: &f64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::Serialize as _;
    js_number_json(*value).serialize(serializer)
}

/// Diretório temporário com limpeza no `Drop`, para os testes de I/O não
/// precisarem da Steam instalada nem de uma dependência extra.
#[cfg(test)]
pub(crate) struct TestDir {
    path: std::path::PathBuf,
}

#[cfg(test)]
impl TestDir {
    pub(crate) fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "arcadia-steam-{tag}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("criar diretório temporário");
        Self { path }
    }

    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
    }
}

#[cfg(test)]
impl AsRef<std::path::Path> for TestDir {
    fn as_ref(&self) -> &std::path::Path {
        &self.path
    }
}

#[cfg(test)]
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn number_parser_matches_javascript_coercions() {
        assert_eq!(js_number_str(""), Some(0.0));
        assert_eq!(js_number_str(" 4907 "), Some(4907.0));
        assert_eq!(js_number_str("4907.5"), Some(4907.5));
        assert_eq!(js_number_str("1e3"), Some(1000.0));
        assert_eq!(js_number_str("0x10"), Some(16.0));
        assert_eq!(js_number_str("0b101"), Some(5.0));
        assert_eq!(js_number_str("0o17"), Some(15.0));
        assert_eq!(js_number_str("-12"), Some(-12.0));
        assert_eq!(js_number_str("+12"), Some(12.0));
        assert_eq!(js_number_str("Infinity"), Some(f64::INFINITY));
        assert_eq!(js_number_str("infinity"), None);
        assert_eq!(js_number_str("NaN"), None);
        assert_eq!(js_number_str("lixo"), None);
        assert_eq!(js_number_str("+"), None);
        assert_eq!(js_number_str("-0x10"), None);
    }

    #[test]
    fn truthiness_and_string_or_follow_the_js_or_operator() {
        assert!(!js_truthy(&Value::Null));
        assert!(!js_truthy(&json!(0)));
        assert!(!js_truthy(&json!("")));
        assert!(js_truthy(&json!([])));
        assert_eq!(js_string_or(&json!(0), "Steam"), "Steam");
        assert_eq!(js_string_or(&json!(""), "Steam"), "Steam");
        assert_eq!(js_string_or(&json!(123), "Steam"), "123");
        assert_eq!(js_string(&json!(null)), "null");
        assert_eq!(js_string(&json!(true)), "true");
        assert_eq!(js_string(&json!([1, 2])), "1,2");
    }

    #[test]
    fn numbers_serialize_like_json_stringify() {
        assert_eq!(js_number_json(4907.0), json!(4907));
        assert_eq!(js_number_json(4907.5), json!(4907.5));
        assert_eq!(js_number_json(f64::NAN), Value::Null);
    }
}
