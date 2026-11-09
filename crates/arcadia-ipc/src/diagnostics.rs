//! Diagnóstico e support bundle — port de `diagnostics.js` + `support-bundle.js`.
//!
//! O bundle redige tokens, caminho do usuário e data dir antes de gravar. Nada
//! é enviado a lugar nenhum; o arquivo fica no diretório escolhido.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{json, Value};

pub const DIAGNOSTICS_VERSION: u32 = 1;
pub const BUNDLE_VERSION: u32 = 1;
pub const MAX_LOG_FILES: usize = 24;
pub const MAX_LOG_BYTES: usize = 512 * 1024;

fn file_info(data_dir: &Path, name: &str) -> Value {
    match fs::metadata(data_dir.join(name)) {
        Ok(meta) => json!({ "present": true, "bytes": meta.len() }),
        Err(_) => json!({ "present": false, "bytes": 0 }),
    }
}

fn count_snapshots(data_dir: &Path) -> usize {
    let root = data_dir.join("snapshots");
    let Ok(games) = fs::read_dir(&root) else {
        return 0;
    };
    games
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .map(|game| {
            fs::read_dir(game.path())
                .map(|entries| {
                    entries
                        .filter_map(Result::ok)
                        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
                        .count()
                })
                .unwrap_or(0)
        })
        .sum()
}

fn download_items(data_dir: &Path) -> Vec<Value> {
    let Ok(raw) = fs::read_to_string(data_dir.join("downloads.json")) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };
    match value {
        Value::Array(items) => items,
        Value::Object(mut object) => object
            .remove("queue")
            .or_else(|| object.remove("items"))
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Relatório consumido pela UI (`DiagnosticsReport`).
pub fn collect(data_dir: &Path, app_version: &str) -> Value {
    let games = fs::read_to_string(data_dir.join("library.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .map(|value| arcadia_store::library::games_from_document(&value))
        .unwrap_or_default();
    let mut by_launcher = serde_json::Map::new();
    for game in &games {
        let launcher = game
            .get("launcher")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let count = by_launcher
            .get(&launcher)
            .and_then(Value::as_u64)
            .unwrap_or(0);
        by_launcher.insert(launcher, json!(count + 1));
    }

    let downloads = download_items(data_dir);
    let mut by_status = serde_json::Map::new();
    for item in &downloads {
        let status = item
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let count = by_status
            .get(&status)
            .and_then(Value::as_u64)
            .unwrap_or(0);
        by_status.insert(status, json!(count + 1));
    }

    let writable = fs::create_dir_all(data_dir).is_ok()
        && fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(data_dir.join(".diagnostics-write-test"))
            .and_then(|mut file| file.write_all(b"ok"))
            .is_ok();
    let _ = fs::remove_file(data_dir.join(".diagnostics-write-test"));

    json!({
        "version": DIAGNOSTICS_VERSION,
        "generated_at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "app": { "version": app_version },
        "runtime": {
            "platform": std::env::consts::OS,
            "release": "unknown",
            "arch": std::env::consts::ARCH,
            "node": "",
            "electron": "",
            "tauri": true
        },
        "storage": {
            "writable": writable,
            "data_dir_configured": std::env::var_os("ARCADIA_DATA_DIR").is_some(),
            "library": file_info(data_dir, "library.json"),
            "downloads": file_info(data_dir, "downloads.json"),
            "snapshots": count_snapshots(data_dir)
        },
        "library": { "total": games.len(), "by_launcher": by_launcher },
        "downloads": { "total": downloads.len(), "by_status": by_status }
    })
}

pub fn redact_text(value: &str, data_dir: &Path, home: &Path) -> String {
    let mut text = value.to_string();
    let mut sources = vec![
        (data_dir.to_string_lossy().into_owned(), "<DATA_DIR>"),
        (home.to_string_lossy().into_owned(), "<HOME>"),
    ];
    sources.sort_by(|left, right| right.0.len().cmp(&left.0.len()));
    for (source, replacement) in sources {
        if source.is_empty() {
            continue;
        }
        text = text.replace(&source, replacement);
        text = text.replace(&source.replace('\\', "/"), replacement);
    }
    // Bearer primeiro: se o regex de chave rodar antes, ele consome só a
    // palavra "Bearer" e deixa o token órfão no texto.
    let bearer = Regex::new(r"(?i)Bearer\s+[A-Za-z0-9._~+/=-]+").expect("regex bearer válido");
    text = bearer.replace_all(&text, "Bearer <REDACTED>").into_owned();
    let secret = Regex::new(
        r#"(?i)((?:access|refresh)[_-]?token|token|password|passwd|api[_-]?key|x-api-key|authorization|cookie|secret)\s*[:=]\s*["']?[^\s,"'}]+["']?"#,
    )
    .expect("regex de segredo válido");
    secret.replace_all(&text, "$1=<REDACTED>").into_owned()
}

fn safe_name(value: &str, fallback: &str) -> String {
    let name: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .take(120)
        .collect();
    if name.is_empty() {
        fallback.to_string()
    } else {
        name
    }
}

fn read_logs(data_dir: &Path, home: &Path) -> Vec<(String, String)> {
    let directory = data_dir.join("logs");
    let Ok(entries) = fs::read_dir(&directory) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|kind| kind.is_file()).unwrap_or(false))
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("log"))
        .collect();
    files.sort();
    files
        .into_iter()
        .take(MAX_LOG_FILES)
        .filter_map(|path| {
            let meta = fs::metadata(&path).ok()?;
            let length = usize::try_from(meta.len().min(MAX_LOG_BYTES as u64)).ok()?;
            let bytes = fs::read(&path).ok()?;
            let start = bytes.len().saturating_sub(length);
            let text = String::from_utf8_lossy(&bytes[start..]);
            Some((
                safe_name(
                    path.file_name().and_then(|name| name.to_str()).unwrap_or("log"),
                    "log",
                ),
                redact_text(&text, data_dir, home),
            ))
        })
        .collect()
}

/// Cria `arcadia-support-<id>/` com diagnóstico redigido e logs.
pub fn create_support_bundle(data_dir: &Path, output_dir: &Path, report: &Value) -> Value {
    if output_dir.as_os_str().is_empty() {
        return json!({ "ok": false, "error": "destino_invalido" });
    }
    let millis = chrono::Utc::now().timestamp_millis();
    let suffix = format!("{:08x}", std::process::id() ^ (millis as u32));
    let bundle_dir = output_dir.join(format!("arcadia-support-{millis}-{suffix}"));
    let temporary = output_dir.join(format!("arcadia-support-{millis}-{suffix}.tmp"));

    let result = (|| -> std::io::Result<Value> {
        fs::create_dir_all(output_dir)?;
        let _ = fs::remove_dir_all(&temporary);
        fs::create_dir_all(temporary.join("logs"))?;

        let home = arcadia_store::paths::home_dir();
        let report_text = serde_json::to_string_pretty(report).unwrap_or_default();
        let redacted = redact_text(&report_text, data_dir, &home);
        let report_value: Value = serde_json::from_str(&redacted).unwrap_or_else(|_| json!({}));
        fs::write(
            temporary.join("diagnostics.json"),
            serde_json::to_string_pretty(&report_value).unwrap_or_default(),
        )?;

        let mut files = vec!["diagnostics.json".to_string()];
        for (name, text) in read_logs(data_dir, &home) {
            let target = temporary.join("logs").join(&name);
            fs::write(&target, text)?;
            files.push(format!("logs/{name}"));
        }
        let manifest = json!({
            "version": BUNDLE_VERSION,
            "created_at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "files": files,
            "redacted": true
        });
        fs::write(
            temporary.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap_or_default(),
        )?;
        fs::rename(&temporary, &bundle_dir)?;
        Ok(json!({ "ok": true, "path": bundle_dir.to_string_lossy(), "files": files }))
    })();

    match result {
        Ok(value) => value,
        Err(error) => {
            let _ = fs::remove_dir_all(&temporary);
            json!({ "ok": false, "error": error.to_string() })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-diagnostics-{tag}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_millis()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn redacts_secrets_and_paths() {
        let data = Path::new(r"C:\Users\zes\AppData\Local\arcadia");
        let home = Path::new(r"C:\Users\zes");
        let text = r#"access_token=abcd1234 Authorization: Bearer eyJhbGci.abc.def path=C:\Users\zes\AppData\Local\arcadia\config.json"#;
        let redacted = redact_text(text, data, home);
        assert!(!redacted.contains("abcd1234"));
        assert!(!redacted.contains("eyJhbGci"));
        assert!(redacted.contains("<DATA_DIR>"), "redacted: {redacted}");
    }

    #[test]
    fn collect_reports_library_and_storage() {
        let dir = temp_dir("collect");
        fs::write(
            dir.join("library.json"),
            r#"{"version":1,"games":[{"id":"1","title":"A","launcher":"steam"},{"id":"2","title":"B","launcher":"custom"}]}"#,
        )
        .unwrap();
        let report = collect(&dir, "1.5.5");
        assert_eq!(report["library"]["total"], 2);
        assert_eq!(report["library"]["by_launcher"]["steam"], 1);
        assert_eq!(report["storage"]["writable"], true);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bundle_includes_redacted_logs_and_manifest() {
        let data = temp_dir("data");
        fs::create_dir_all(data.join("logs")).unwrap();
        fs::write(
            data.join("logs").join("debug.log"),
            "access_token=segredo123\nBearer abc.def.ghi\n",
        )
        .unwrap();
        let output = temp_dir("out");
        let report = collect(&data, "1.5.5");
        let result = create_support_bundle(&data, &output, &report);
        assert_eq!(result["ok"], true, "bundle: {result}");
        let path = PathBuf::from(result["path"].as_str().unwrap());
        assert!(path.join("manifest.json").is_file());
        let log = fs::read_to_string(path.join("logs").join("debug.log")).unwrap();
        assert!(!log.contains("segredo123"));
        assert!(log.contains("REDACTED"));
        let _ = fs::remove_dir_all(&data);
        let _ = fs::remove_dir_all(&output);
    }
}
