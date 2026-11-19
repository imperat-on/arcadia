//! Contratos compartilhados (port de `contracts/index.js` + `contracts/sync.js`).
//!
//! As funções são puras e preservam os campos desconhecidos que o JavaScript
//! mantém via spread. A semântica de coerção (String/Number/Date.parse,
//! trim, `.length` UTF-16, timestamps) segue o oráculo congelado em `app/` e
//! `contracts/`.

use std::borrow::Borrow;
use std::cmp::Ordering;

use chrono::{DateTime, Days, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone};
use serde_json::{Map, Number, Value};

pub const CONTRACT_VERSION: u32 = 1;
pub const LIBRARY_SCHEMA_VERSION: u32 = 1;
pub const MAX_ID_LENGTH: usize = 512;
pub const MAX_TITLE_LENGTH: usize = 1024;
pub const MAX_COMMAND_ARGS: usize = 64;
pub const MAX_SYNC_ITEMS: usize = 1000;
pub const MAX_SYNC_MINUTES: i64 = 999_999;
/// Ano 2243 em epoch-segundos (mesmo limite de `contracts/sync.js`).
pub const MAX_SYNC_TIMESTAMP: i64 = 8_640_000_000;

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const MAX_SAFE_INTEGER_I64: i64 = 9_007_199_254_740_991;

/// Normaliza um jogo preservando todos os campos desconhecidos do provider.
///
/// Regras (espelho de `normalizeGame`): objeto, `id` e `title` strings não
/// vazias (número é aceito e convertido), limites de tamanho; `launcher`
/// default `"custom"`; `launch_cmd` só strings, até 64.
pub fn normalize_game(value: serde_json::Value) -> Option<serde_json::Value> {
    let object = value.as_object()?;
    let id = js_text(object.get("id"), MAX_ID_LENGTH)?;
    let title = js_text(object.get("title"), MAX_TITLE_LENGTH)?;
    let mut out = object.clone();
    out.insert("id".into(), id.into());
    out.insert("title".into(), title.into());
    let launcher = js_text(object.get("launcher"), 64).unwrap_or_else(|| "custom".to_string());
    out.insert("launcher".into(), launcher.into());
    out.insert(
        "launch_cmd".into(),
        command(object.get("launch_cmd")),
    );
    Some(serde_json::Value::Object(out))
}

pub fn normalize_library(value: serde_json::Value) -> Vec<serde_json::Value> {
    let serde_json::Value::Array(items) = value else {
        return Vec::new();
    };
    items.into_iter().filter_map(normalize_game).collect()
}

/// Sessão segura: só `id`, `email` e `username` atravessam a fronteira IPC.
pub fn safe_account_session(value: &serde_json::Value) -> Option<serde_json::Value> {
    let user = value.get("user")?;
    let id = js_text(user.get("id"), MAX_ID_LENGTH)?;
    let mut safe = Map::new();
    safe.insert("id".into(), id.into());
    if let Some(email) = js_text(user.get("email"), 320) {
        safe.insert("email".into(), email.into());
    }
    // `user.user_metadata?.username ?? user.username`: metadado não-nulo tem
    // precedência; se ele existe mas é inválido, NÃO cai para user.username.
    let metadata_username = match user.get("user_metadata") {
        Some(Value::Object(metadata)) => metadata.get("username"),
        _ => None,
    };
    let username = match metadata_username {
        Some(value) if !value.is_null() => js_text(Some(value), 64),
        _ => js_text(user.get("username"), 64),
    };
    if let Some(username) = username {
        safe.insert("username".into(), username.into());
    }
    let mut out = Map::new();
    out.insert("user".into(), serde_json::Value::Object(safe));
    Some(serde_json::Value::Object(out))
}

/// Resultado público de login/cadastro: tokens ficam somente no main process.
pub fn safe_auth_result(value: impl Borrow<Value>) -> Value {
    let value = value.borrow();
    let Some(object) = value.as_object() else {
        return serde_json::json!({ "ok": false, "error": "resposta_invalida" });
    };
    let mut safe = Map::new();
    safe.insert(
        "ok".into(),
        Value::Bool(object.get("ok") == Some(&Value::Bool(true))),
    );
    if let Some(error) = object.get("error") {
        if !error.is_null() {
            safe.insert("error".into(), Value::String(js_string(error)));
        }
    }
    if let Some(username_real) = object.get("usernameReal") {
        if !username_real.is_null() {
            if let Some(username) = js_text(Some(username_real), 64) {
                safe.insert("usernameReal".into(), Value::String(username));
            }
        }
    }
    Value::Object(safe)
}

pub fn safe_account_status(value: impl Borrow<Value>) -> Value {
    let value = value.borrow();
    let Some(object) = value.as_object() else {
        return serde_json::json!({ "session": null, "error": "resposta_invalida" });
    };
    let session = object
        .get("session")
        .and_then(safe_account_session)
        .unwrap_or(Value::Null);
    let error = match object.get("error") {
        Some(error) if !error.is_null() => Value::String(js_string(error)),
        _ => Value::Null,
    };
    let mut out = Map::new();
    out.insert("session".into(), session);
    out.insert("error".into(), error);
    Value::Object(out)
}

pub fn safe_account_event(event: impl Borrow<Value>, session: impl Borrow<Value>) -> Value {
    let event = event.borrow();
    let session = session.borrow();
    let event = if js_truthy(event) {
        js_string(event)
    } else {
        String::new()
    };
    let mut out = Map::new();
    out.insert("event".into(), Value::String(event));
    out.insert(
        "session".into(),
        safe_account_session(session).unwrap_or(Value::Null),
    );
    Value::Object(out)
}

/// Contrato reduzido usado por push_library/pull_library.
pub fn normalize_library_sync_item(value: impl Borrow<Value>) -> Option<Value> {
    let value = value.borrow();
    let object = value.as_object()?;
    let appid = js_text(object.get("appid"), MAX_ID_LENGTH)?;
    if object.get("removed") == Some(&Value::Bool(true)) {
        let mut out = Map::new();
        out.insert("appid".into(), Value::String(appid));
        out.insert("removed".into(), Value::Bool(true));
        return Some(Value::Object(out));
    }

    let title = js_text(object.get("title"), MAX_TITLE_LENGTH).unwrap_or_else(|| appid.clone());
    let platform = match object.get("platform") {
        Some(Value::String(platform)) if platform == "emulator" => "emulator",
        Some(Value::String(platform)) if platform == "linux" => "linux",
        _ => "windows",
    };
    let mut out = Map::new();
    out.insert("appid".into(), Value::String(appid));
    out.insert("title".into(), Value::String(title));
    out.insert("platform".into(), Value::String(platform.to_string()));

    for key in [
        "cover",
        "hero",
        "logo",
        "icon",
        "description",
        "systemId",
        "developer",
        "publisher",
    ] {
        let max = if key == "description" { 10_000 } else { 2_000 };
        if let Some(item) = js_text(object.get(key), max) {
            out.insert(key.to_string(), Value::String(item));
        }
    }
    if object.get("retro") == Some(&Value::Bool(true)) {
        out.insert("retro".into(), Value::Bool(true));
    }
    if let Some(Value::Array(genres)) = object.get("genres") {
        let genres: Vec<Value> = genres
            .iter()
            .filter_map(|item| match item {
                Value::String(genre) => {
                    let genre = js_trim(genre);
                    if genre.is_empty() {
                        None
                    } else {
                        Some(Value::String(genre.to_string()))
                    }
                }
                _ => None,
            })
            .take(32)
            .collect();
        if !genres.is_empty() {
            out.insert("genres".into(), Value::Array(genres));
        }
    }
    if let Some(release_year) = js_number_is_integer(object.get("releaseYear")) {
        out.insert("releaseYear".into(), Value::Number(release_year));
    }
    Some(Value::Object(out))
}

pub fn normalize_library_sync_items(value: impl Borrow<Value>) -> Vec<Value> {
    let value = value.borrow();
    let Value::Array(items) = value else {
        return Vec::new();
    };
    items
        .iter()
        .take(MAX_SYNC_ITEMS)
        .filter_map(normalize_library_sync_item)
        .collect()
}

pub fn normalize_playtime_item(value: impl Borrow<Value>) -> Option<Value> {
    let value = value.borrow();
    let object = value.as_object()?;
    let appid = js_text(object.get("appid"), MAX_ID_LENGTH)?;
    let raw_minutes = object.get("minutes")?;
    let minutes = match raw_minutes {
        Value::Number(_) => {
            let number = raw_minutes.as_f64()?;
            let integer = js_safe_integer(number)?;
            if integer <= 0 || integer > MAX_SYNC_MINUTES {
                return None;
            }
            integer
        }
        Value::String(raw) => {
            let trimmed = js_trim(raw);
            if !is_js_integer_string(trimmed) {
                return None;
            }
            let minutes: i64 = trimmed.parse().ok()?;
            if minutes <= 0 || minutes > MAX_SYNC_MINUTES {
                return None;
            }
            minutes
        }
        _ => return None,
    };
    let mut out = Map::new();
    out.insert("appid".into(), Value::String(appid));
    out.insert("minutes".into(), Value::Number(Number::from(minutes)));
    Some(Value::Object(out))
}

pub fn normalize_playtime_items(value: impl Borrow<Value>) -> Vec<Value> {
    let value = value.borrow();
    let Value::Array(items) = value else {
        return Vec::new();
    };
    items
        .iter()
        .take(MAX_SYNC_ITEMS)
        .filter_map(normalize_playtime_item)
        .collect()
}

/// Epoch em segundos; aceita segundos, milissegundos e datas ISO.
pub fn normalize_sync_timestamp(value: impl Borrow<Value>) -> Option<i64> {
    let value = value.borrow();
    if value.is_null() || value == &Value::String(String::new()) {
        return None;
    }
    let mut number = match finite_number(value) {
        Some(number) => number,
        None => match value {
            Value::String(text) => parse_js_date(text)?,
            _ => return None,
        },
    };
    if number <= 0.0 {
        return None;
    }
    // Datas em milissegundos têm pelo menos 12 dígitos hoje. O limite evita
    // converter um timestamp obviamente corrompido em um valor válido.
    if number > 1e12 {
        number = (number / 1000.0).floor();
    }
    number = number.floor();
    if number > 0.0
        && number <= MAX_SYNC_TIMESTAMP as f64
        && number.fract() == 0.0
        && number.abs() <= MAX_SAFE_INTEGER
    {
        Some(number as i64)
    } else {
        None
    }
}

/// Merge de conquistas: desbloqueio é monotônico e o menor timestamp vence.
pub fn resolve_achievement_conflict(
    local: impl Borrow<Value>,
    remote: impl Borrow<Value>,
) -> Option<Value> {
    let left = achievement_value(local.borrow());
    let right = achievement_value(remote.borrow());
    let (left, right) = match (left, right) {
        (None, None) => return None,
        (None, Some(right)) => return Some(right.value),
        (Some(left), None) => return Some(left.value),
        (Some(left), Some(right)) => (left, right),
    };
    if left.appid != right.appid || left.apiname != right.apiname {
        return None;
    }

    let left_ts = left.unlocked_at;
    let right_ts = right.unlocked_at;
    let winner = if left_ts.is_none() && right_ts.is_some() {
        &right
    } else if right_ts.is_none() && left_ts.is_some() {
        &left
    } else if let (Some(left_ts), Some(right_ts)) = (left_ts, right_ts) {
        if left_ts != right_ts {
            // Para conquistas desbloqueadas (ts > 0), menor vence.
            // Para bloqueadas (ts = 0), qualquer valor não-zero vence.
            if left_ts > 0 && right_ts > 0 {
                if left_ts < right_ts {
                    &left
                } else {
                    &right
                }
            } else if left_ts > 0 {
                &left
            } else if right_ts > 0 {
                &right
            } else {
                metadata_winner_ref(&left, &right)
            }
        } else {
            metadata_winner_ref(&left, &right)
        }
    } else {
        metadata_winner_ref(&left, &right)
    };

    // Merge de timestamps: ignorar 0 (locked) ao calcular o menor timestamp.
    let non_zero: Vec<i64> = [left_ts, right_ts]
        .into_iter()
        .flatten()
        .filter(|timestamp| *timestamp > 0)
        .collect();
    let timestamp = non_zero.iter().copied().min().or(left_ts).or(right_ts);
    let achieved = left.achieved
        || right.achieved
        || timestamp.map(|timestamp| timestamp > 0).unwrap_or(false);

    let mut out = winner.value.as_object().cloned().unwrap_or_default();
    out.insert("appid".into(), Value::String(left.appid.clone()));
    out.insert("apiname".into(), Value::String(left.apiname.clone()));
    out.insert("achieved".into(), Value::Bool(achieved));
    out.insert(
        "unlocked_at".into(),
        timestamp
            .map(|timestamp| Value::Number(Number::from(timestamp)))
            .unwrap_or(Value::Null),
    );
    Some(Value::Object(out))
}

/// Merge de biblioteca por appid (ver comentários em `contracts/sync.js`).
pub fn resolve_library_conflict(
    local: impl Borrow<Value>,
    remote: impl Borrow<Value>,
) -> Option<Value> {
    let left = library_value(local.borrow());
    let right = library_value(remote.borrow());
    let (left, right) = match (left, right) {
        (None, None) => return None,
        (None, Some(right)) => return Some(right.value),
        (Some(left), None) => return Some(left.value),
        (Some(left), Some(right)) => (left, right),
    };
    if left.appid != right.appid {
        return None;
    }

    let left_revision = operation_revision(&left.value);
    let right_revision = operation_revision(&right.value);
    let winner =
        if let (Some(left_revision), Some(right_revision)) = (left_revision, right_revision) {
            if left_revision != right_revision {
                if left_revision > right_revision {
                    &left
                } else {
                    &right
                }
            } else {
                library_tiebreak(&left, &right)
            }
        } else if left_revision.is_some() {
            &left
        } else if right_revision.is_some() {
            &right
        } else {
            library_tiebreak(&left, &right)
        };

    let mut out = winner.value.as_object().cloned().unwrap_or_default();
    out.insert("appid".into(), Value::String(left.appid.clone()));
    out.insert("removed".into(), Value::Bool(winner.removed));
    Some(Value::Object(out))
}

/// Horas são totais monotônicos no cliente: o maior total vence.
pub fn resolve_playtime_conflict(local: impl Borrow<Value>, remote: impl Borrow<Value>) -> i64 {
    playtime_minutes(local.borrow()).max(playtime_minutes(remote.borrow()))
}

pub fn resolve_sync_conflict(
    kind: impl Borrow<Value>,
    local: impl Borrow<Value>,
    remote: impl Borrow<Value>,
) -> Option<Value> {
    let kind = kind.borrow();
    let kind = if js_truthy(kind) {
        js_string(kind)
    } else {
        String::new()
    };
    match kind.to_lowercase().as_str() {
        "achievement" | "achievements" => resolve_achievement_conflict(local, remote),
        "library" | "game" => resolve_library_conflict(local, remote),
        "playtime" | "minutes" => Some(Value::Number(Number::from(resolve_playtime_conflict(
            local, remote,
        )))),
        _ => None,
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Helpers de semântica JavaScript
// ───────────────────────────────────────────────────────────────────────────

fn is_js_whitespace(character: char) -> bool {
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

fn js_trim(value: &str) -> &str {
    value.trim_matches(is_js_whitespace)
}

fn js_utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

fn js_cmp(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

fn js_number_to_string(number: &Number) -> String {
    if let Some(value) = number.as_i64() {
        return value.to_string();
    }
    if let Some(value) = number.as_u64() {
        return value.to_string();
    }
    match number.as_f64() {
        Some(value) => js_f64_to_string(value),
        None => number.to_string(),
    }
}

fn js_f64_to_string(value: f64) -> String {
    if value == 0.0 {
        // Cobre -0.0: String(-0) === "0".
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
    if absolute >= 1e21 || absolute < 1e-6 {
        let formatted = format!("{value:e}");
        if let Some((mantissa, exponent)) = formatted.split_once('e') {
            if let Ok(exponent) = exponent.parse::<i32>() {
                let sign = if exponent >= 0 { '+' } else { '-' };
                return format!("{mantissa}e{sign}{}", exponent.abs());
            }
        }
        return formatted;
    }
    format!("{value}")
}

fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(number) => js_number_to_string(number),
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

fn js_truthy(value: &Value) -> bool {
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

fn js_parse_number(value: &str) -> Option<f64> {
    let value = js_trim(value);
    if value.is_empty() {
        return Some(0.0);
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = value.strip_prefix(prefix) {
            if digits.is_empty() {
                return None;
            }
            let mut parsed = 0.0;
            for character in digits.chars() {
                let digit = character.to_digit(radix)?;
                parsed = parsed * f64::from(radix) + f64::from(digit);
            }
            return Some(parsed);
        }
    }
    value.parse::<f64>().ok()
}

fn finite_number(value: &Value) -> Option<f64> {
    match value {
        Value::Bool(_) | Value::Null => None,
        Value::Number(number) => number.as_f64().filter(|value| value.is_finite()),
        Value::String(value) => js_parse_number(value).filter(|value| value.is_finite()),
        Value::Array(_) => {
            let joined = js_string(value);
            js_parse_number(&joined).filter(|value| value.is_finite())
        }
        Value::Object(_) => None,
    }
}

fn js_text(value: Option<&Value>, max_length: usize) -> Option<String> {
    let value = value?;
    let raw = match value {
        Value::String(value) => value.clone(),
        Value::Number(_) => js_string(value),
        _ => return None,
    };
    let trimmed = js_trim(&raw);
    if trimmed.is_empty() || js_utf16_len(trimmed) > max_length {
        return None;
    }
    Some(trimmed.to_string())
}

fn command(value: Option<&serde_json::Value>) -> serde_json::Value {
    let Some(serde_json::Value::Array(items)) = value else {
        return serde_json::Value::Array(Vec::new());
    };
    serde_json::Value::Array(
        items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .take(MAX_COMMAND_ARGS)
            .map(serde_json::Value::String)
            .collect(),
    )
}

fn js_safe_integer(value: f64) -> Option<i64> {
    if value.is_finite() && value.fract() == 0.0 && value.abs() <= MAX_SAFE_INTEGER {
        Some(value as i64)
    } else {
        None
    }
}

fn number_value(value: &Value, min: i64, max: i64) -> Option<i64> {
    let number = js_safe_integer(finite_number(value)?)?;
    if number < min || number > max {
        None
    } else {
        Some(number)
    }
}

fn js_number_is_integer(value: Option<&Value>) -> Option<Number> {
    let Value::Number(number) = value? else {
        return None;
    };
    let value = number.as_f64()?;
    if !value.is_finite() || value.fract() != 0.0 {
        return None;
    }
    if let Some(value) = number.as_i64() {
        return Some(Number::from(value));
    }
    if let Some(value) = number.as_u64() {
        return Some(Number::from(value));
    }
    if value >= i64::MIN as f64 && value <= i64::MAX as f64 {
        return Some(Number::from(value as i64));
    }
    if value >= 0.0 && value <= u64::MAX as f64 {
        return Some(Number::from(value as u64));
    }
    Number::from_f64(value)
}

fn is_js_integer_string(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

// ───────────────────────────────────────────────────────────────────────────
// Date.parse (subconjunto ISO 8601 + RFC 2822 usado pelo sync)
// ───────────────────────────────────────────────────────────────────────────

fn parse_js_date(value: &str) -> Option<f64> {
    if let Some(milliseconds) = parse_iso_date(value) {
        return Some(milliseconds as f64);
    }
    if let Ok(parsed) = DateTime::parse_from_rfc2822(value) {
        return Some(parsed.timestamp_millis() as f64);
    }
    None
}

fn parse_fixed_digits(bytes: &[u8], index: &mut usize, count: usize) -> Option<u32> {
    if *index + count > bytes.len() {
        return None;
    }
    let mut value = 0_u32;
    for _ in 0..count {
        let byte = bytes[*index];
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value * 10 + u32::from(byte - b'0');
        *index += 1;
    }
    Some(value)
}

fn parse_digit_range(
    bytes: &[u8],
    index: &mut usize,
    min: usize,
    max: usize,
) -> Option<(u32, usize)> {
    let start = *index;
    let mut value = 0_u32;
    while *index < bytes.len() && *index - start < max && bytes[*index].is_ascii_digit() {
        value = value * 10 + u32::from(bytes[*index] - b'0');
        *index += 1;
    }
    let width = *index - start;
    if width < min {
        return None;
    }
    Some((value, width))
}

enum Zone {
    Utc,
    Offset(i64),
    Local,
}

fn parse_iso_date(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut index = 0_usize;

    let year = if matches!(bytes.first(), Some(b'+') | Some(b'-')) {
        let negative = bytes[0] == b'-';
        index += 1;
        let year = parse_fixed_digits(bytes, &mut index, 6)? as i32;
        if negative {
            -year
        } else {
            year
        }
    } else {
        parse_fixed_digits(bytes, &mut index, 4)? as i32
    };

    let mut month = 1_u32;
    let mut day = 1_u32;
    let mut month_width = 0_usize;
    let mut day_width = 0_usize;
    if index < bytes.len() && bytes[index] == b'-' {
        index += 1;
        let (parsed_month, width) = parse_digit_range(bytes, &mut index, 1, 2)?;
        month = parsed_month;
        month_width = width;
        if index < bytes.len() && bytes[index] == b'-' {
            index += 1;
            let (parsed_day, width) = parse_digit_range(bytes, &mut index, 1, 2)?;
            day = parsed_day;
            day_width = width;
        }
    }

    let has_time = if index < bytes.len() {
        match bytes[index] {
            b'T' | b't' | b' ' => {
                index += 1;
                true
            }
            _ => return None,
        }
    } else {
        false
    };

    if !has_time {
        if index != bytes.len() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        let date = normalized_date(year, month, day)?;
        // ISO estrito (componentes com largura fixa) é UTC; formas legadas
        // não preenchidas são interpretadas no fuso local pelo V8.
        let strict = month_width == 2 && (day_width == 0 || day_width == 2);
        let naive = date.and_hms_opt(0, 0, 0)?;
        return if strict {
            Some(naive.and_utc().timestamp_millis())
        } else {
            local_to_millis(naive)
        };
    }

    if month_width != 2 || day_width != 2 {
        return None;
    }
    let hour = parse_fixed_digits(bytes, &mut index, 2)?;
    if bytes.get(index) != Some(&b':') {
        return None;
    }
    index += 1;
    let minute = parse_fixed_digits(bytes, &mut index, 2)?;
    let mut second = 0_u32;
    if bytes.get(index) == Some(&b':') {
        index += 1;
        second = parse_fixed_digits(bytes, &mut index, 2)?;
    }
    let mut milliseconds = 0_u32;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == start {
            return None;
        }
        let mut fraction = 0_u32;
        for offset in 0..3 {
            fraction *= 10;
            if let Some(byte) = bytes.get(start + offset) {
                if byte.is_ascii_digit() {
                    fraction += u32::from(byte - b'0');
                }
            }
        }
        milliseconds = fraction;
    }

    let zone = if index == bytes.len() {
        Zone::Local
    } else {
        match bytes[index] {
            b'Z' | b'z' => {
                index += 1;
                Zone::Utc
            }
            sign @ (b'+' | b'-') => {
                index += 1;
                let hours = parse_fixed_digits(bytes, &mut index, 2)?;
                let minutes = if bytes.get(index) == Some(&b':') {
                    index += 1;
                    parse_fixed_digits(bytes, &mut index, 2)?
                } else {
                    parse_fixed_digits(bytes, &mut index, 2)?
                };
                if hours > 23 || minutes > 59 {
                    return None;
                }
                let offset = i64::from(hours) * 3600 + i64::from(minutes) * 60;
                Zone::Offset(if sign == b'-' { -offset } else { offset })
            }
            _ => return None,
        }
    };
    if index != bytes.len() {
        return None;
    }

    if hour > 24 || minute > 59 || second > 59 {
        return None;
    }
    if hour == 24 && (minute != 0 || second != 0 || milliseconds != 0) {
        return None;
    }
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let mut date = normalized_date(year, month, day)?;
    let mut hour = hour;
    if hour == 24 {
        date = date.succ_opt()?;
        hour = 0;
    }
    let naive = date.and_hms_milli_opt(hour, minute, second, milliseconds)?;
    let naive_millis = naive.and_utc().timestamp_millis();
    match zone {
        Zone::Utc => Some(naive_millis),
        Zone::Offset(offset) => Some(naive_millis - offset * 1000),
        Zone::Local => local_to_millis(naive),
    }
}

fn normalized_date(year: i32, month: u32, day: u32) -> Option<NaiveDate> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    // O V8 aceita dias até 31 e normaliza o excedente (2024-02-30 → 2024-03-01).
    first.checked_add_days(Days::new(u64::from(day - 1)))
}

fn local_to_millis(naive: NaiveDateTime) -> Option<i64> {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(value) => Some(value.timestamp_millis()),
        LocalResult::Ambiguous(first, _second) => Some(first.timestamp_millis()),
        LocalResult::None => {
            // Horário inexistente em transição de DST: o V8 desloca para frente.
            let shifted = naive.checked_add_signed(chrono::Duration::hours(1))?;
            match Local.from_local_datetime(&shifted) {
                LocalResult::Single(value) | LocalResult::Ambiguous(value, _) => {
                    Some(value.timestamp_millis())
                }
                LocalResult::None => None,
            }
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Normalização de registros de sync
// ───────────────────────────────────────────────────────────────────────────

struct AchievementValue {
    value: Value,
    appid: String,
    apiname: String,
    achieved: bool,
    unlocked_at: Option<i64>,
}

fn achievement_value(value: &Value) -> Option<AchievementValue> {
    let object = value.as_object()?;
    let appid = js_text(object.get("appid"), MAX_ID_LENGTH)?;
    let apiname = js_text(object.get("apiname"), MAX_ID_LENGTH)?;
    // unlocked_at=0 é válido para conquistas bloqueadas.
    let raw_timestamp = match object.get("unlocked_at") {
        Some(value) if !value.is_null() => value,
        _ => object.get("unlock").unwrap_or(&Value::Null),
    };
    let unlocked_at = match raw_timestamp {
        Value::Number(number) if number.as_f64() == Some(0.0) => Some(0),
        _ => normalize_sync_timestamp(raw_timestamp),
    };
    let achieved = object.get("achieved") == Some(&Value::Bool(true))
        || unlocked_at.map(|timestamp| timestamp > 0).unwrap_or(false)
        || object.get("unlocked") == Some(&Value::Bool(true));

    let mut out = object.clone();
    out.insert("appid".into(), Value::String(appid.clone()));
    out.insert("apiname".into(), Value::String(apiname.clone()));
    out.insert("achieved".into(), Value::Bool(achieved));
    out.insert(
        "unlocked_at".into(),
        unlocked_at
            .map(|timestamp| Value::Number(Number::from(timestamp)))
            .unwrap_or(Value::Null),
    );
    Some(AchievementValue {
        value: Value::Object(out),
        appid,
        apiname,
        achieved,
        unlocked_at,
    })
}

struct LibraryValue {
    value: Value,
    appid: String,
    removed: bool,
}

fn library_value(value: &Value) -> Option<LibraryValue> {
    let object = value.as_object()?;
    let raw_appid = match object.get("appid") {
        Some(value) if !value.is_null() => value,
        _ => object.get("id").unwrap_or(&Value::Null),
    };
    let appid = js_text(Some(raw_appid), MAX_ID_LENGTH)?;
    let removed = object.get("removed") == Some(&Value::Bool(true));
    let title = js_text(object.get("title"), MAX_TITLE_LENGTH).unwrap_or_else(|| appid.clone());
    let platform = match object.get("platform") {
        Some(Value::String(platform)) if platform == "emulator" => "emulator",
        Some(Value::String(platform)) if platform == "linux" => "linux",
        _ => "windows",
    };
    let mut out = object.clone();
    out.insert("appid".into(), Value::String(appid.clone()));
    out.insert("title".into(), Value::String(title));
    out.insert("platform".into(), Value::String(platform.to_string()));
    out.insert("removed".into(), Value::Bool(removed));
    Some(LibraryValue {
        value: Value::Object(out),
        appid,
        removed,
    })
}

fn operation_revision(value: &Value) -> Option<i64> {
    let object = value.as_object()?;
    for key in ["revision", "lamport", "version"] {
        if let Some(number) = object.get(key) {
            if let Some(number) = number_value(number, 0, MAX_SAFE_INTEGER_I64) {
                return Some(number);
            }
        }
    }
    for key in [
        "updated_at",
        "updatedAt",
        "operation_at",
        "operationAt",
        "created_at",
        "createdAt",
    ] {
        if let Some(timestamp) = object.get(key) {
            if let Some(timestamp) = normalize_sync_timestamp(timestamp) {
                return Some(timestamp);
            }
        }
    }
    None
}

fn title_quality(value: &Value) -> u8 {
    let title = match value.get("title") {
        Some(title) if js_truthy(title) => js_string(title),
        _ => String::new(),
    };
    let appid = match value.get("appid") {
        Some(appid) if js_truthy(appid) => js_string(appid),
        _ => String::new(),
    };
    let title = js_trim(&title);
    let appid = js_trim(&appid);
    if title.is_empty() || title == appid || is_steam_placeholder(title) {
        0
    } else {
        1
    }
}

fn is_steam_placeholder(title: &str) -> bool {
    let lower = title.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("steam ") else {
        return false;
    };
    !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit())
}

fn library_tiebreak<'a>(left: &'a LibraryValue, right: &'a LibraryValue) -> &'a LibraryValue {
    if left.removed != right.removed {
        // Uma remoção explícita vence o empate para impedir que um pull
        // atrasado ressuscite o jogo apagado offline.
        if left.removed {
            left
        } else {
            right
        }
    } else if title_quality(&left.value) != title_quality(&right.value) {
        if title_quality(&left.value) > title_quality(&right.value) {
            left
        } else {
            right
        }
    } else {
        metadata_winner_ref(left, right)
    }
}

fn playtime_minutes(value: &Value) -> i64 {
    let raw = if value.is_object() || value.is_array() {
        value.get("minutes").unwrap_or(&Value::Null)
    } else {
        value
    };
    number_value(raw, 0, MAX_SYNC_MINUTES).unwrap_or(0)
}

trait SyncValue {
    fn sync_value(&self) -> &Value;
}

impl SyncValue for AchievementValue {
    fn sync_value(&self) -> &Value {
        &self.value
    }
}

impl SyncValue for LibraryValue {
    fn sync_value(&self) -> &Value {
        &self.value
    }
}

fn metadata_winner_ref<'a, T: SyncValue>(left: &'a T, right: &'a T) -> &'a T {
    let left_json = stable_string(left.sync_value());
    let right_json = stable_string(right.sync_value());
    if js_cmp(&left_json, &right_json) != Ordering::Greater {
        left
    } else {
        right
    }
}

fn stable_string(value: &Value) -> String {
    let mut out = String::new();
    write_stable(value, &mut out);
    out
}

fn write_stable(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Value::Number(number) => out.push_str(&js_number_to_string(number)),
        Value::String(value) => {
            out.push_str(&serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string()));
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_stable(item, out);
            }
            out.push(']');
        }
        Value::Object(object) => {
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort_by(|left, right| js_cmp(left, right));
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_string()));
                out.push(':');
                if let Some(item) = object.get(key) {
                    write_stable(item, out);
                }
            }
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text(value: &str, count: usize) -> String {
        value.repeat(count)
    }

    // ── normalizeGame / normalizeLibrary (fixtures existentes) ─────────────

    #[test]
    fn game_keeps_unknown_fields_and_normalizes() {
        let game = json!({
            "id": " 10 ",
            "title": " Half-Life ",
            "launcher": "steam",
            "launch_cmd": ["steam", 1, "run"],
            "provider_field": { "keep": true }
        });
        let out = normalize_game(game).unwrap();
        assert_eq!(out["id"], "10");
        assert_eq!(out["title"], "Half-Life");
        assert_eq!(out["launch_cmd"], json!(["steam", "run"]));
        assert_eq!(out["provider_field"], json!({ "keep": true }));
    }

    #[test]
    fn game_rejects_missing_id_or_title() {
        assert!(normalize_game(json!({ "title": "x" })).is_none());
        assert!(normalize_game(json!({ "id": "x" })).is_none());
        assert!(normalize_game(json!({ "id": "  ", "title": "x" })).is_none());
    }

    #[test]
    fn launcher_defaults_to_custom() {
        let out = normalize_game(json!({ "id": "a", "title": "b" })).unwrap();
        assert_eq!(out["launcher"], "custom");
        assert_eq!(out["launch_cmd"], json!([]));
    }

    #[test]
    fn library_fixture_from_js_contract_test() {
        let games = normalize_library(json!([
            {
                "id": "steam:10",
                "title": "  Portal 2  ",
                "launcher": "steam",
                "launch_cmd": ["steam", 10, null, "steam://rungameid/10"],
                "customProviderFlag": true
            },
            { "id": "custom:1", "title": "Jogo local", "exe": "/tmp/game.exe" },
            { "id": "", "title": "invalido" },
            null
        ]));
        assert_eq!(CONTRACT_VERSION, 1);
        assert_eq!(games.len(), 2);
        assert_eq!(games[0]["title"], "Portal 2");
        assert_eq!(
            games[0]["launch_cmd"],
            json!(["steam", "steam://rungameid/10"])
        );
        assert_eq!(games[0]["customProviderFlag"], true);
        assert_eq!(games[1]["launcher"], "custom");
        assert_eq!(games[1]["launch_cmd"], json!([]));
    }

    // ── normalizeLibrarySyncItem(s) ────────────────────────────────────────

    #[test]
    fn library_sync_fixture_from_js_contract_test() {
        assert_eq!(
            normalize_library_sync_items(json!([
                { "appid": "steam:10", "title": "Portal", "platform": "linux" },
                { "appid": "custom:rom", "title": "ROM local", "platform": "emulator" },
                { "appid": "steam:11", "removed": true, "title": "ignorado" },
                { "appid": "steam:12", "removed": "false", "title": "não remover" },
                { "appid": "", "title": "descartado" },
                { "appid": "steam:13", "title": "sem plataforma", "platform": "unknown" }
            ])),
            vec![
                json!({ "appid": "steam:10", "title": "Portal", "platform": "linux" }),
                json!({ "appid": "custom:rom", "title": "ROM local", "platform": "emulator" }),
                json!({ "appid": "steam:11", "removed": true }),
                json!({ "appid": "steam:12", "title": "não remover", "platform": "windows" }),
                json!({ "appid": "steam:13", "title": "sem plataforma", "platform": "windows" })
            ]
        );
    }

    #[test]
    fn library_sync_item_removed_short_circuits_all_other_fields() {
        let item = normalize_library_sync_item(json!({
            "appid": "steam:10",
            "removed": true,
            "title": "não deve aparecer",
            "platform": "emulator",
            "cover": "https://x",
            "genres": ["A"],
            "releaseYear": 2020,
            "retro": true,
            "unknown": 1
        }))
        .unwrap();
        assert_eq!(item, json!({ "appid": "steam:10", "removed": true }));
    }

    #[test]
    fn library_sync_item_text_limits_and_unknown_fields() {
        let description = text("d", 10_000);
        let accepted = normalize_library_sync_item(json!({
            "appid": "x",
            "description": description,
            "cover": text("c", 2_000),
            "unknown": "drop"
        }))
        .unwrap();
        assert_eq!(accepted["description"], text("d", 10_000));
        assert_eq!(accepted["cover"], text("c", 2_000));
        assert!(accepted.get("unknown").is_none());

        let rejected = normalize_library_sync_item(json!({
            "appid": "x",
            "description": text("d", 10_001),
            "cover": text("c", 2_001)
        }))
        .unwrap();
        assert!(rejected.get("description").is_none());
        assert!(rejected.get("cover").is_none());
    }

    #[test]
    fn library_sync_item_genres_trim_filter_and_slice_without_dedupe() {
        let mut genres = vec![
            json!("  Ação  "),
            json!(""),
            json!("   "),
            json!(1),
            json!(null),
        ];
        for index in 0..40 {
            genres.push(json!(format!("g{index}")));
        }
        let item = normalize_library_sync_item(json!({ "appid": "x", "genres": genres })).unwrap();
        let out = item["genres"].as_array().unwrap();
        assert_eq!(out.len(), 32);
        assert_eq!(out[0], "Ação");
        assert_eq!(out[1], "g0");
        assert_eq!(out[31], "g30");
    }

    #[test]
    fn library_sync_item_release_year_retro_and_numeric_text() {
        let item = normalize_library_sync_item(json!({
            "appid": 10,
            "title": 20.5,
            "releaseYear": 2020.0,
            "retro": true
        }))
        .unwrap();
        assert_eq!(item["appid"], "10");
        assert_eq!(item["title"], "20.5");
        assert_eq!(item["releaseYear"], json!(2020));
        assert_eq!(item["retro"], true);

        let string_year = normalize_library_sync_item(json!({
            "appid": "x",
            "releaseYear": "2020",
            "retro": "true"
        }))
        .unwrap();
        assert!(string_year.get("releaseYear").is_none());
        assert!(string_year.get("retro").is_none());
    }

    #[test]
    fn library_sync_items_limit_before_filtering_and_reject_non_arrays() {
        let mut items: Vec<Value> = (0..1_002)
            .map(|index| json!({ "appid": format!("id:{index}") }))
            .collect();
        items[0] = Value::Null;
        assert_eq!(normalize_library_sync_items(Value::Array(items)).len(), 999);
        assert_eq!(
            normalize_library_sync_items(json!({ "appid": "x" })),
            Vec::<Value>::new()
        );
        assert!(normalize_library_sync_item(Value::Null).is_none());
        assert!(normalize_library_sync_item(json!(["x"])).is_none());
    }

    // ── normalizePlaytimeItem(s) ────────────────────────────────────────────

    #[test]
    fn playtime_fixture_from_js_contract_test() {
        assert_eq!(
            normalize_playtime_items(json!([
                { "appid": "steam:10", "minutes": "30" },
                { "appid": "steam:11", "minutes": 0 },
                { "appid": "steam:12", "minutes": true },
                { "appid": "steam:13", "minutes": 999999 }
            ])),
            vec![
                json!({ "appid": "steam:10", "minutes": 30 }),
                json!({ "appid": "steam:13", "minutes": 999999 })
            ]
        );
    }

    #[test]
    fn playtime_numeric_string_coercion_and_whitespace() {
        assert_eq!(
            normalize_playtime_item(json!({ "appid": "x", "minutes": " 30 " })).unwrap(),
            json!({ "appid": "x", "minutes": 30 })
        );
        assert_eq!(
            normalize_playtime_item(json!({ "appid": "x", "minutes": "00030\n" })).unwrap(),
            json!({ "appid": "x", "minutes": 30 })
        );
        assert_eq!(
            normalize_playtime_item(json!({ "appid": 10, "minutes": 30.0 })).unwrap(),
            json!({ "appid": "10", "minutes": 30 })
        );
    }

    #[test]
    fn playtime_rejects_out_of_range_non_integers_and_invalid_shapes() {
        for minutes in [
            json!("+30"),
            json!("1e2"),
            json!("0x10"),
            json!("-0"),
            json!(0),
            json!(-1),
        ] {
            assert!(
                normalize_playtime_item(json!({ "appid": "x", "minutes": minutes })).is_none(),
                "minutes should be rejected: {minutes}"
            );
        }
        for minutes in [
            json!(30.5),
            json!(1_000_000),
            json!(999_999.5),
            json!("1000000"),
        ] {
            assert!(
                normalize_playtime_item(json!({ "appid": "x", "minutes": minutes })).is_none(),
                "minutes should be rejected: {minutes}"
            );
        }
        assert!(normalize_playtime_item(json!({ "appid": "x", "minutes": null })).is_none());
        assert!(normalize_playtime_item(json!({ "appid": "x", "minutes": [30] })).is_none());
        assert!(normalize_playtime_item(json!({ "minutes": 30 })).is_none());
    }

    #[test]
    fn playtime_items_limit_and_reject_non_arrays() {
        let mut items: Vec<Value> = (0..1_002)
            .map(|index| json!({ "appid": format!("id:{index}"), "minutes": 1 }))
            .collect();
        items[0] = Value::Null;
        assert_eq!(normalize_playtime_items(Value::Array(items)).len(), 999);
        assert_eq!(normalize_playtime_items(Value::Null), Vec::<Value>::new());
    }

    // ── safeAuthResult / safeAccountStatus / safeAccountEvent ──────────────

    #[test]
    fn safe_session_strips_tokens_and_metadata() {
        let session = json!({
            "access_token": "secret",
            "user": {
                "id": "u1",
                "email": "a@b.c",
                "user_metadata": { "username": "zyon" },
                "raw": "drop"
            }
        });
        let safe = safe_account_session(&session).unwrap();
        assert_eq!(
            safe,
            json!({ "user": { "id": "u1", "email": "a@b.c", "username": "zyon" } })
        );
    }

    #[test]
    fn safe_auth_status_event_fixture_from_js_contract_test() {
        let session = safe_account_session(&json!({
            "access_token": "access-secret",
            "refresh_token": "refresh-secret",
            "user": {
                "id": "user-1",
                "email": "user@example.com",
                "user_metadata": { "username": "player", "secret": "não expor" }
            }
        }));
        assert_eq!(
            session,
            Some(
                json!({ "user": { "id": "user-1", "email": "user@example.com", "username": "player" } })
            )
        );

        let result = safe_auth_result(json!({
            "ok": true,
            "session": { "access_token": "access-secret", "refresh_token": "refresh-secret" },
            "user": { "id": "user-1" },
            "usernameReal": "player"
        }));
        assert_eq!(result, json!({ "ok": true, "usernameReal": "player" }));
        let serialized = serde_json::to_string(&result).unwrap();
        assert!(!serialized.contains("access-secret"));
        assert!(!serialized.contains("refresh-secret"));

        assert_eq!(
            safe_account_status(json!({
                "session": { "access_token": "access-secret", "user": { "id": "user-1" } },
                "error": null,
                "leaked": "never"
            })),
            json!({ "session": { "user": { "id": "user-1" } }, "error": null })
        );
        assert_eq!(
            safe_account_event(
                json!("SIGNED_IN"),
                json!({
                    "refresh_token": "refresh-secret",
                    "user": { "id": "user-1", "user_metadata": { "username": "player" } }
                })
            ),
            json!({ "event": "SIGNED_IN", "session": { "user": { "id": "user-1", "username": "player" } } })
        );
    }

    #[test]
    fn safe_auth_result_invalid_shapes_and_error_coercion() {
        for invalid in [Value::Null, json!([]), json!("x"), json!(7)] {
            assert_eq!(
                safe_auth_result(invalid),
                json!({ "ok": false, "error": "resposta_invalida" })
            );
        }
        assert_eq!(
            safe_auth_result(json!({ "ok": 1, "error": true })),
            json!({ "ok": false, "error": "true" })
        );
        assert_eq!(
            safe_auth_result(json!({ "ok": true, "error": 123 })),
            json!({ "ok": true, "error": "123" })
        );
        assert_eq!(
            safe_auth_result(json!({ "ok": true, "error": [1, null, "x", [2, 3]] })),
            json!({ "ok": true, "error": "1,,x,2,3" })
        );
        assert_eq!(
            safe_auth_result(json!({ "ok": true, "error": { "a": 1 } })),
            json!({ "ok": true, "error": "[object Object]" })
        );
        assert_eq!(
            safe_auth_result(json!({ "ok": true, "error": null })),
            json!({ "ok": true })
        );
    }

    #[test]
    fn safe_auth_result_username_real_enforces_js_text_limit() {
        assert_eq!(
            safe_auth_result(json!({ "ok": true, "usernameReal": "  player  " })),
            json!({ "ok": true, "usernameReal": "player" })
        );
        assert_eq!(
            safe_auth_result(json!({ "ok": true, "usernameReal": 42 })),
            json!({ "ok": true, "usernameReal": "42" })
        );
        assert_eq!(
            safe_auth_result(json!({ "ok": true, "usernameReal": text("a", 64) })),
            json!({ "ok": true, "usernameReal": text("a", 64) })
        );
        assert_eq!(
            safe_auth_result(json!({ "ok": true, "usernameReal": text("a", 65) })),
            json!({ "ok": true })
        );
        assert_eq!(
            safe_auth_result(json!({ "ok": true, "usernameReal": { "x": 1 } })),
            json!({ "ok": true })
        );
    }

    #[test]
    fn safe_account_status_metadata_precedence_and_error_coercion() {
        assert_eq!(
            safe_account_status(json!({
                "session": {
                    "user": { "id": "u", "username": "direct", "user_metadata": { "username": "meta" } }
                },
                "error": 7
            })),
            json!({ "session": { "user": { "id": "u", "username": "meta" } }, "error": "7" })
        );
        assert_eq!(
            safe_account_status(json!({
                "session": {
                    "user": { "id": "u", "username": "direct", "user_metadata": { "username": null } }
                }
            })),
            json!({ "session": { "user": { "id": "u", "username": "direct" } }, "error": null })
        );
        assert_eq!(
            safe_account_status(json!({
                "session": {
                    "user": { "id": "u", "username": "direct", "user_metadata": { "username": "" } }
                }
            })),
            json!({ "session": { "user": { "id": "u" } }, "error": null })
        );
        assert_eq!(
            safe_account_status(json!({
                "session": {
                    "user": { "id": "u", "username": "direct", "user_metadata": { "username": 5 } }
                }
            })),
            json!({ "session": { "user": { "id": "u", "username": "5" } }, "error": null })
        );
        assert_eq!(
            safe_account_status(json!({ "session": null, "error": false })),
            json!({ "session": null, "error": "false" })
        );
        assert_eq!(
            safe_account_status(json!("nope")),
            json!({ "session": null, "error": "resposta_invalida" })
        );
    }

    #[test]
    fn safe_account_event_event_coercion_and_invalid_shapes() {
        let session = json!({ "user": { "id": "u" } });
        assert_eq!(
            safe_account_event(json!(0), Value::Null),
            json!({ "event": "", "session": null })
        );
        assert_eq!(
            safe_account_event(json!(false), session.clone()),
            json!({ "event": "", "session": { "user": { "id": "u" } } })
        );
        assert_eq!(
            safe_account_event(json!([1, 2]), session.clone()),
            json!({ "event": "1,2", "session": { "user": { "id": "u" } } })
        );
        assert_eq!(
            safe_account_event(json!({ "a": 1 }), session.clone()),
            json!({ "event": "[object Object]", "session": { "user": { "id": "u" } } })
        );
        assert_eq!(
            safe_account_event(json!(true), json!([])),
            json!({ "event": "true", "session": null })
        );
    }

    // ── normalizeSyncTimestamp ──────────────────────────────────────────────

    #[test]
    fn timestamp_fixture_from_js_sync_resolver_test() {
        assert_eq!(
            normalize_sync_timestamp(json!("2024-01-01T00:00:00Z")),
            Some(1_704_067_200)
        );
        assert_eq!(
            normalize_sync_timestamp(json!(1_704_067_200_000_i64)),
            Some(1_704_067_200)
        );
        assert_eq!(
            normalize_sync_timestamp(json!("1704067200")),
            Some(1_704_067_200)
        );
        assert_eq!(normalize_sync_timestamp(json!("nao-e-data")), None);
    }

    #[test]
    fn timestamp_rejects_null_boolean_zero_and_empty() {
        for value in [
            Value::Null,
            json!(true),
            json!(false),
            json!(0),
            json!(-5),
            json!(""),
            json!("0"),
            json!("-0"),
        ] {
            assert_eq!(
                normalize_sync_timestamp(value.clone()),
                None,
                "value: {value}"
            );
        }
        assert_eq!(normalize_sync_timestamp(json!([])), None);
        assert_eq!(normalize_sync_timestamp(json!({})), None);
    }

    #[test]
    fn timestamp_milliseconds_threshold_and_bounds() {
        assert_eq!(normalize_sync_timestamp(json!(1e12)), None);
        assert_eq!(
            normalize_sync_timestamp(json!(1_000_000_000_001_i64)),
            Some(1_000_000_000)
        );
        assert_eq!(
            normalize_sync_timestamp(json!("1000000000001")),
            Some(1_000_000_000)
        );
        assert_eq!(
            normalize_sync_timestamp(json!(8_640_000_000_i64)),
            Some(8_640_000_000)
        );
        assert_eq!(normalize_sync_timestamp(json!(8_640_000_001_i64)), None);
        assert_eq!(normalize_sync_timestamp(json!(1.9)), Some(1));
        assert_eq!(normalize_sync_timestamp(json!("1.9")), Some(1));
    }

    #[test]
    fn timestamp_iso_dates_offsets_and_fractions() {
        assert_eq!(
            normalize_sync_timestamp(json!("2024-01-01")),
            Some(1_704_067_200)
        );
        assert_eq!(
            normalize_sync_timestamp(json!("2024-01-01T00:00:00.999Z")),
            Some(1_704_067_200)
        );
        assert_eq!(
            normalize_sync_timestamp(json!("2024-01-01T00:00:00+05:30")),
            Some(1_704_047_400)
        );
        assert_eq!(
            normalize_sync_timestamp(json!("2024-01-01T00:00:00-05:30")),
            Some(1_704_087_000)
        );
        assert_eq!(
            normalize_sync_timestamp(json!("2024-01-01T00:00:00+0530")),
            Some(1_704_047_400)
        );
        assert_eq!(
            normalize_sync_timestamp(json!("2024-02-30T00:00:00Z")),
            Some(1_709_251_200)
        );
        assert_eq!(
            normalize_sync_timestamp(json!("2024-01-01T24:00:00Z")),
            Some(1_704_153_600)
        );
        assert_eq!(
            normalize_sync_timestamp(json!("2024-13-01T00:00:00Z")),
            None
        );
        assert_eq!(
            normalize_sync_timestamp(json!("2024-01-01T25:00:00Z")),
            None
        );
        assert_eq!(
            normalize_sync_timestamp(json!("2024-01-01T00:00:00+05")),
            None
        );
    }

    #[test]
    fn timestamp_numeric_string_variants() {
        assert_eq!(
            normalize_sync_timestamp(json!(" 1704067200 ")),
            Some(1_704_067_200)
        );
        assert_eq!(normalize_sync_timestamp(json!("0x10")), Some(16));
        assert_eq!(normalize_sync_timestamp(json!("0b101")), Some(5));
        assert_eq!(normalize_sync_timestamp(json!("0o10")), Some(8));
        assert_eq!(normalize_sync_timestamp(json!("1e3")), Some(1_000));
        assert_eq!(normalize_sync_timestamp(json!("5.")), Some(5));
        assert_eq!(normalize_sync_timestamp(json!(".5")), None);
        assert_eq!(normalize_sync_timestamp(json!("1_000")), None);
        assert_eq!(
            normalize_sync_timestamp(json!([1_704_067_200_i64])),
            Some(1_704_067_200)
        );
        assert_eq!(normalize_sync_timestamp(json!([1, 2])), None);
    }

    // ── resolveAchievementConflict ──────────────────────────────────────────

    #[test]
    fn achievement_fixture_earliest_wins_and_preserves_metadata() {
        let a =
            json!({ "appid": "730", "apiname": "ach", "unlocked_at": 1_700_000_200, "title": "A" });
        let b =
            json!({ "appid": "730", "apiname": "ach", "unlocked_at": 1_700_000_100, "title": "B" });
        let ab = resolve_achievement_conflict(&a, &b).unwrap();
        let ba = resolve_achievement_conflict(&b, &a).unwrap();
        assert_eq!(ab, ba);
        assert_eq!(ab["unlocked_at"], 1_700_000_100);
        assert_eq!(ab["achieved"], true);
        assert_eq!(ab["title"], "B");
    }

    #[test]
    fn achievement_locked_and_missing_timestamps() {
        assert_eq!(
            resolve_achievement_conflict(
                json!({ "appid": "1", "apiname": "a" }),
                json!({ "appid": "1", "apiname": "a" })
            )
            .unwrap(),
            json!({ "appid": "1", "apiname": "a", "achieved": false, "unlocked_at": null })
        );
        assert_eq!(
            resolve_achievement_conflict(
                json!({ "appid": "1", "apiname": "a", "unlocked_at": 0 }),
                json!({ "appid": "1", "apiname": "a", "unlocked_at": 100 })
            )
            .unwrap()["unlocked_at"],
            100
        );
        assert_eq!(
            resolve_achievement_conflict(
                json!({ "appid": "1", "apiname": "a" }),
                json!({ "appid": "1", "apiname": "a", "unlocked_at": 0 })
            )
            .unwrap(),
            json!({ "appid": "1", "apiname": "a", "unlocked_at": 0, "achieved": false })
        );
    }

    #[test]
    fn achievement_unlock_alias_and_monotonic_achieved() {
        assert_eq!(
            resolve_achievement_conflict(
                json!({ "appid": "1", "apiname": "a", "unlock": 50 }),
                json!({ "appid": "1", "apiname": "a", "unlocked_at": 100 })
            )
            .unwrap(),
            json!({ "appid": "1", "apiname": "a", "unlock": 50, "achieved": true, "unlocked_at": 50 })
        );
        assert_eq!(
            resolve_achievement_conflict(
                json!({ "appid": "1", "apiname": "a", "achieved": false, "unlocked_at": 100 }),
                json!({ "appid": "1", "apiname": "a", "achieved": true, "unlocked_at": null })
            )
            .unwrap()["achieved"],
            true
        );
        assert_eq!(
            resolve_achievement_conflict(
                json!({ "appid": "1", "apiname": "a", "unlocked_at": 0 }),
                json!({ "appid": "1", "apiname": "a", "unlocked": true })
            )
            .unwrap()["achieved"],
            true
        );
    }

    #[test]
    fn achievement_mismatch_and_invalid_sides() {
        assert!(resolve_achievement_conflict(
            json!({ "appid": "1", "apiname": "a" }),
            json!({ "appid": "2", "apiname": "a" })
        )
        .is_none());
        assert_eq!(
            resolve_achievement_conflict(
                Value::Null,
                json!({ "appid": "1", "apiname": "a", "unlocked_at": "2024-01-01T00:00:00Z" })
            )
            .unwrap(),
            json!({ "appid": "1", "apiname": "a", "unlocked_at": 1_704_067_200, "achieved": true })
        );
        assert!(resolve_achievement_conflict(Value::Null, Value::Null).is_none());
    }

    #[test]
    fn achievement_duplicate_timestamps_use_canonical_json() {
        let left =
            json!({ "appid": "1", "apiname": "a", "unlocked_at": 100, "title": "B", "x": 1 });
        let right =
            json!({ "appid": "1", "apiname": "a", "unlocked_at": 100, "title": "A", "x": 2 });
        assert_eq!(
            resolve_achievement_conflict(&left, &right).unwrap(),
            resolve_achievement_conflict(&right, &left).unwrap()
        );
        assert_eq!(
            resolve_achievement_conflict(&left, &right).unwrap()["title"],
            "A"
        );
        let fractional = resolve_achievement_conflict(
            json!({ "appid": "1", "apiname": "a", "unlocked_at": 100.9 }),
            json!({ "appid": "1", "apiname": "a", "unlocked_at": 101 }),
        )
        .unwrap();
        assert_eq!(fractional["unlocked_at"], 100);
    }

    // ── resolveLibraryConflict ──────────────────────────────────────────────

    #[test]
    fn library_fixture_revision_wins_and_is_commutative() {
        let add = json!({ "appid": "steam:10", "title": "Portal", "revision": 4 });
        let rename = json!({ "appid": "steam:10", "title": "Portal 2", "revision": 5 });
        assert_eq!(
            resolve_library_conflict(&add, &rename),
            resolve_library_conflict(&rename, &add)
        );
        assert_eq!(
            resolve_library_conflict(&add, &rename).unwrap()["title"],
            "Portal 2"
        );

        let removed = json!({ "appid": "steam:10", "removed": true, "revision": 5 });
        let stale_add = json!({ "appid": "steam:10", "title": "Portal", "revision": 5 });
        let merged = resolve_library_conflict(&removed, &stale_add).unwrap();
        assert_eq!(merged["removed"], true);
        assert_eq!(
            resolve_sync_conflict(json!("library"), &stale_add, &removed).unwrap()["removed"],
            true
        );
    }

    #[test]
    fn library_fixture_real_title_beats_placeholder_without_revision() {
        let merged = resolve_library_conflict(
            json!({ "appid": "steam:10", "title": "Steam 10" }),
            json!({ "appid": "steam:10", "title": "Portal 2" }),
        )
        .unwrap();
        assert_eq!(merged["title"], "Portal 2");
    }

    #[test]
    fn library_fixture_preserves_emulator_platform() {
        let merged = resolve_library_conflict(
            json!({ "appid": "custom:rom", "title": "ROM", "platform": "emulator" }),
            json!({ "appid": "custom:rom", "title": "ROM", "platform": "emulator" }),
        )
        .unwrap();
        assert_eq!(merged["platform"], "emulator");
    }

    #[test]
    fn library_revision_aliases_and_timestamp_fallback() {
        assert_eq!(
            resolve_library_conflict(
                json!({ "appid": "x", "title": "A", "lamport": 2 }),
                json!({ "appid": "x", "title": "B", "lamport": 1 })
            )
            .unwrap()["title"],
            "A"
        );
        assert_eq!(
            resolve_library_conflict(
                json!({ "appid": "x", "title": "A", "version": 2 }),
                json!({ "appid": "x", "title": "B", "version": 3 })
            )
            .unwrap()["title"],
            "B"
        );
        assert_eq!(
            resolve_library_conflict(
                json!({ "appid": "x", "title": "A", "updated_at": "2024-01-01T00:00:00Z" }),
                json!({ "appid": "x", "title": "B", "updated_at": 1_704_067_201_i64 })
            )
            .unwrap()["title"],
            "B"
        );
        assert_eq!(
            resolve_library_conflict(
                json!({ "appid": "x", "title": "A", "revision": 1, "updated_at": 999 }),
                json!({ "appid": "x", "title": "B", "updated_at": 2 })
            )
            .unwrap()["title"],
            "B"
        );
    }

    #[test]
    fn library_removed_false_and_appid_fallback() {
        let by_id = resolve_library_conflict(
            json!({ "id": "x", "title": "A" }),
            json!({ "appid": "x", "title": "B" }),
        )
        .unwrap();
        assert_eq!(by_id["appid"], "x");
        assert_eq!(by_id["id"], "x");
        assert_eq!(by_id["title"], "A");

        assert_eq!(
            resolve_library_conflict(
                json!({ "appid": "", "id": "x", "title": "A" }),
                json!({ "appid": "x", "title": "B" })
            )
            .unwrap()["title"],
            "B"
        );
        assert_eq!(
            resolve_library_conflict(
                json!({ "appid": "x", "title": "A", "removed": false }),
                json!({ "appid": "x", "title": "B" })
            )
            .unwrap()["removed"],
            false
        );
    }

    #[test]
    fn library_ignores_invalid_revision_values_and_mismatched_appids() {
        assert_eq!(
            resolve_library_conflict(
                json!({ "appid": "x", "title": "A", "revision": -1 }),
                json!({ "appid": "x", "title": "B", "revision": 0 })
            )
            .unwrap()["title"],
            "B"
        );
        assert_eq!(
            resolve_library_conflict(
                json!({ "appid": "x", "title": "A", "revision": 1.5 }),
                json!({ "appid": "x", "title": "B", "revision": 1 })
            )
            .unwrap()["title"],
            "B"
        );
        assert_eq!(
            resolve_library_conflict(
                json!({ "appid": "x", "title": "A", "revision": true }),
                json!({ "appid": "x", "title": "B", "revision": "2" })
            )
            .unwrap()["title"],
            "B"
        );
        assert!(resolve_library_conflict(
            json!({ "appid": "x", "title": "A" }),
            json!({ "appid": "y", "title": "B" })
        )
        .is_none());
    }

    #[test]
    fn library_unknown_fields_and_duplicate_revisions_follow_canonical_winner() {
        let merged = resolve_library_conflict(
            json!({ "appid": "x", "title": "A", "revision": 2, "foo": 1 }),
            json!({ "appid": "x", "title": "B", "revision": 2, "foo": 2 }),
        )
        .unwrap();
        assert_eq!(merged["title"], "A");
        assert_eq!(merged["foo"], 1);
        assert_eq!(merged["platform"], "windows");
        assert_eq!(merged["removed"], false);
    }

    // ── resolvePlaytimeConflict ─────────────────────────────────────────────

    #[test]
    fn playtime_fixture_is_monotonic() {
        assert_eq!(
            resolve_playtime_conflict(json!({ "minutes": 90 }), json!({ "minutes": "120" })),
            120
        );
        assert_eq!(
            resolve_playtime_conflict(json!({ "minutes": 300 }), json!({ "minutes": 2 })),
            300
        );
        assert_eq!(
            resolve_sync_conflict(json!("playtime"), json!(12), json!(30)),
            Some(json!(30))
        );
        assert_eq!(
            resolve_playtime_conflict(json!({ "minutes": -1 }), json!({ "minutes": "ruim" })),
            0
        );
    }

    #[test]
    fn playtime_ignores_out_of_range_fractional_and_boolean_values() {
        assert_eq!(
            resolve_playtime_conflict(json!({ "minutes": 1_000_000 }), json!(5)),
            5
        );
        assert_eq!(
            resolve_playtime_conflict(json!({ "minutes": 999_999 }), json!(0)),
            999_999
        );
        assert_eq!(
            resolve_playtime_conflict(json!({ "minutes": 999_999.9 }), json!(5)),
            5
        );
        assert_eq!(
            resolve_playtime_conflict(json!({ "minutes": true }), json!(5)),
            5
        );
        assert_eq!(resolve_playtime_conflict(json!(null), json!(5)), 5);
        assert_eq!(resolve_playtime_conflict(json!([10]), json!(5)), 5);
        assert_eq!(
            resolve_playtime_conflict(json!({ "minutes": [30] }), json!(0)),
            30
        );
    }

    #[test]
    fn playtime_numeric_strings_whitespace_and_exponent() {
        assert_eq!(
            resolve_playtime_conflict(json!({ "minutes": " 30 " }), json!(0)),
            30
        );
        assert_eq!(
            resolve_playtime_conflict(json!({ "minutes": "1e2" }), json!(0)),
            100
        );
        assert_eq!(
            resolve_playtime_conflict(json!({ "minutes": "5." }), json!(0)),
            5
        );
        assert_eq!(
            resolve_playtime_conflict(json!({ "minutes": "0x10" }), json!(0)),
            16
        );
        assert_eq!(
            resolve_playtime_conflict(json!({ "minutes": "" }), json!(0)),
            0
        );
        assert_eq!(
            resolve_playtime_conflict(json!({ "minutes": "5.5" }), json!(0)),
            0
        );
    }

    #[test]
    fn playtime_duplicate_totals_are_stable_and_scalar_inputs_work() {
        assert_eq!(resolve_playtime_conflict(json!(120), json!(120)), 120);
        assert_eq!(
            resolve_sync_conflict(json!("minutes"), json!({ "minutes": 10 }), json!(20)),
            Some(json!(20))
        );
    }

    // ── resolveSyncConflict ─────────────────────────────────────────────────

    #[test]
    fn sync_dispatch_aliases_and_case_insensitivity() {
        let achievement = json!({ "appid": "1", "apiname": "a", "unlocked_at": 2 });
        assert_eq!(
            resolve_sync_conflict(
                json!("ACHIEVEMENT"),
                achievement.clone(),
                json!({ "appid": "1", "apiname": "a", "unlocked_at": 1 })
            )
            .unwrap()["unlocked_at"],
            1
        );
        assert_eq!(
            resolve_sync_conflict(
                json!("Achievements"),
                achievement,
                json!({ "appid": "1", "apiname": "a", "unlocked_at": 1 })
            )
            .unwrap()["unlocked_at"],
            1
        );
        assert_eq!(
            resolve_sync_conflict(
                json!("game"),
                json!({ "appid": "x", "title": "A", "revision": 1 }),
                json!({ "appid": "x", "title": "B", "revision": 2 })
            )
            .unwrap()["title"],
            "B"
        );
        assert_eq!(
            resolve_sync_conflict(
                json!(["library"]),
                json!({ "appid": "x", "title": "A" }),
                json!({ "appid": "x", "title": "B" })
            )
            .unwrap()["title"],
            "A"
        );
    }

    #[test]
    fn sync_dispatch_rejects_null_falsy_and_unknown_kinds() {
        for kind in [
            Value::Null,
            json!(0),
            json!(false),
            json!(""),
            json!({}),
            json!("unknown"),
        ] {
            assert_eq!(
                resolve_sync_conflict(kind.clone(), json!(1), json!(2)),
                None,
                "kind: {kind}"
            );
        }
        assert_eq!(
            resolve_sync_conflict(json!("PLAYTIME"), json!(1), json!(2)),
            Some(json!(2))
        );
    }
}
