//! Semântica do JavaScript usada pelos módulos retrô.
//!
//! Os helpers abaixo reproduzem `Boolean(value)`, `String(value)`,
//! `Number(value)`, `Number.isSafeInteger`, `JSON.parse` com limite de tamanho
//! e escrita atômica. São internos ao crate para que catálogo, repositório,
//! ofertas e metadados compartilhem exatamente as mesmas regras.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::title::js_trim;

/// `Boolean(value)`.
pub fn js_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(boolean)) => *boolean,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|value| value != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

/// `String(value)`.
pub fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(js_string).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// `String(value || "")`.
pub fn js_or_string(value: Option<&Value>) -> String {
    match value {
        Some(value) if js_truthy(Some(value)) => js_string(value),
        _ => String::new(),
    }
}

/// `Number(value)`.
pub fn js_number(value: Option<&Value>) -> f64 {
    match value {
        None | Some(Value::Null) => 0.0,
        Some(Value::Bool(boolean)) => {
            if *boolean {
                1.0
            } else {
                0.0
            }
        }
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(text)) => {
            let trimmed = js_trim(text);
            if trimmed.is_empty() {
                return 0.0;
            }
            if let Some(hex) = trimmed
                .strip_prefix("0x")
                .or_else(|| trimmed.strip_prefix("0X"))
            {
                return u64::from_str_radix(hex, 16)
                    .map(|value| value as f64)
                    .unwrap_or(f64::NAN);
            }
            if let Some(octal) = trimmed
                .strip_prefix("0o")
                .or_else(|| trimmed.strip_prefix("0O"))
            {
                return u64::from_str_radix(octal, 8)
                    .map(|value| value as f64)
                    .unwrap_or(f64::NAN);
            }
            trimmed.parse::<f64>().unwrap_or(f64::NAN)
        }
        Some(Value::Array(_)) => {
            let text = js_string(value.expect("array presente"));
            if text.is_empty() {
                0.0
            } else {
                text.parse::<f64>().unwrap_or(f64::NAN)
            }
        }
        Some(Value::Object(_)) => f64::NAN,
    }
}

/// `Number.isSafeInteger(Number(value))` devolvendo o inteiro.
pub fn js_safe_integer(value: Option<&Value>) -> Option<i64> {
    let number = js_number(value);
    if !number.is_finite() || number.fract() != 0.0 || number.abs() > 9_007_199_254_740_991.0 {
        return None;
    }
    Some(number as i64)
}

/// `String(value || "")` a partir de um campo.
pub fn field_or_string(value: &Value, key: &str) -> String {
    js_or_string(value.get(key))
}

pub fn sha256_hex(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    encode_hex(&hasher.finalize())
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

/// `Date.now()`.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// `pathHasSymlink`: qualquer componente (inclusive o alvo) é symlink.
pub fn path_has_symlink(target: &Path) -> bool {
    let absolute: PathBuf = if target.is_absolute() {
        target.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(target))
            .unwrap_or_else(|_| target.to_path_buf())
    };
    for ancestor in absolute.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return true;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return true,
        }
    }
    false
}

/// `safeReadJson`: symlink-safe, arquivo regular, limite de tamanho e JSON.
pub fn safe_read_json(path: &Path, max_bytes: u64) -> Option<Value> {
    if path_has_symlink(path) {
        return None;
    }
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() || metadata.len() > max_bytes {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// `safeWriteJson`: escrita atômica, sem seguir symlink. Erros silenciosos.
pub fn safe_write_json(path: &Path, value: &Value, pretty: bool) -> io::Result<()> {
    let Some(directory) = path.parent() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "caminho sem diretório",
        ));
    };
    if path_has_symlink(directory) {
        return Ok(());
    }
    std::fs::create_dir_all(directory)?;
    if path_has_symlink(directory) || path_has_symlink(path) {
        return Ok(());
    }
    let serialized = if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    }
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    arcadia_store::atomic::write_atomic_text(path, &serialized)
}

/// Lê um JSON tipado através de [`safe_read_json`].
pub fn safe_read_typed<T: serde::de::DeserializeOwned>(path: &Path, max_bytes: u64) -> Option<T> {
    let value = safe_read_json(path, max_bytes)?;
    serde_json::from_value(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn js_truthiness_and_number_match_javascript() {
        assert!(!js_truthy(Some(&json!(null))));
        assert!(!js_truthy(Some(&json!(0))));
        assert!(!js_truthy(Some(&json!(""))));
        assert!(js_truthy(Some(&json!([]))));
        assert!(js_truthy(Some(&json!({}))));
        assert_eq!(js_number(Some(&json!("12"))), 12.0);
        assert_eq!(js_number(Some(&json!(""))), 0.0);
        assert!(js_number(Some(&json!("abc"))).is_nan());
        assert_eq!(js_safe_integer(Some(&json!(1.5))), None);
        assert_eq!(js_safe_integer(Some(&json!(-3))), Some(-3));
    }

    #[test]
    fn js_string_matches_string_conversion() {
        assert_eq!(js_string(&json!(null)), "null");
        assert_eq!(js_string(&json!(true)), "true");
        assert_eq!(js_string(&json!([1, 2])), "1,2");
        assert_eq!(js_string(&json!({})), "[object Object]");
        assert_eq!(js_or_string(Some(&json!(""))), "");
        assert_eq!(js_or_string(Some(&json!("x"))), "x");
    }

    #[test]
    fn sha256_hex_matches_known_vector() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
