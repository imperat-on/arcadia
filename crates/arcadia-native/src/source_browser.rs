//! Verificação de fontes em navegador isolado.
//!
//! O processo da UI nunca roda GTK nem interpreta páginas. Quando a fonte
//! responde com desafio Cloudflare, este módulo spawna o binário
//! `arcadia-source-helper`, que abre a janela WebKit em outro processo e
//! devolve o JSON do catálogo por stdout. O helper é opcional: se ele não
//! existir, a fonte com desafio falha e as demais continuam funcionando.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use arcadia_catalog::sources::{validar_downloads, ChallengeSolver};
use serde_json::{json, Value};

use crate::i18n::{I18n, Lang};

const MAX_CATALOG_BYTES: usize = 64 * 1024 * 1024;
const BROWSER_TIMEOUT: Duration = Duration::from_secs(120);
const HELPER_BIN: &str = if cfg!(windows) {
    "arcadia-source-helper.exe"
} else {
    "arcadia-source-helper"
};

pub struct SourceBrowser {
    helper: PathBuf,
    profile: PathBuf,
    language: String,
    gate: tokio::sync::Mutex<()>,
}

impl SourceBrowser {
    /// `executable` é o binário principal; o helper é procurado ao lado dele.
    pub fn new(executable: PathBuf, profile: PathBuf, language: String) -> Self {
        Self {
            helper: executable.with_file_name(HELPER_BIN),
            profile,
            language,
            gate: tokio::sync::Mutex::new(()),
        }
    }

    async fn load(&self, url: &str) -> Result<Value, String> {
        if !safe_browser_url(url) {
            return Err("source_browser_invalid_url".into());
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            let _ = (&self.profile, &self.language);
            return Err("source_browser_unavailable".into());
        }
        // Um processo por vez compartilha os cookies, inclusive durante sync.
        let _guard = self.gate.lock().await;

        let i18n = I18n::new(Lang::from_code(&self.language).unwrap_or_default());
        let title = i18n.t("native.sources.browser_title");
        let hint = i18n.t("native.sources.browser_hint");

        let mut command = tokio::process::Command::new(&self.helper);
        command
            .arg(url)
            .arg(&self.profile)
            .arg(title)
            .arg(hint)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // No AppImage o GStreamer embutido fica no LD_LIBRARY_PATH do processo
        // principal. O helper usa GTK/WebKit do host e não precisa do
        // GStreamer do bundle; removê-lo evita misturar as duas pilhas.
        if std::env::var_os("ARCADIA_BUNDLED_GST").is_some() {
            command
                .env_remove("LD_LIBRARY_PATH")
                .env_remove("GST_PLUGIN_SYSTEM_PATH_1_0")
                .env_remove("GST_PLUGIN_PATH_1_0")
                .env_remove("GST_PLUGIN_SCANNER")
                .env_remove("GST_REGISTRY");
        }

        let child = command
            .spawn()
            .map_err(|error| format!("source_browser_start: {error}"))?;
        let output = tokio::time::timeout(
            BROWSER_TIMEOUT + Duration::from_secs(5),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| "source_browser_timeout")?
        .map_err(|error| format!("source_browser_process: {error}"))?;
        if !output.status.success() {
            return Err("source_browser_process_failed".into());
        }
        parse_reply(&output.stdout)
    }
}

#[async_trait::async_trait]
impl ChallengeSolver for SourceBrowser {
    async fn resolve(&self, url: &str) -> Option<Value> {
        match self.load(url).await {
            Ok(data) => Some(data),
            Err(error) => {
                eprintln!("[arcadia-native] verificação da fonte: {error}");
                None
            }
        }
    }
}

fn safe_browser_url(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    })
}

fn parse_reply(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > MAX_CATALOG_BYTES + 1024 {
        return Err("source_browser_too_large".into());
    }
    let mut reply: Value =
        serde_json::from_slice(bytes).map_err(|_| "source_browser_invalid_reply")?;
    if reply.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(reply
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("source_browser_failed")
            .to_string());
    }
    let data = reply
        .get_mut("data")
        .map(Value::take)
        .ok_or("source_browser_invalid_reply")?;
    if validar_downloads(&data).is_none() {
        return Err("source_browser_invalid_catalog".into());
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_url_rejects_local_schemes_and_credentials() {
        assert!(safe_browser_url(
            "https://hydralinks.cloud/sources/fitgirl.json"
        ));
        assert!(!safe_browser_url("file:///etc/passwd"));
        assert!(!safe_browser_url("javascript:alert(1)"));
        assert!(!safe_browser_url(
            "https://user:pass@example.test/source.json"
        ));
    }

    #[test]
    fn browser_reply_requires_a_valid_catalog() {
        let data =
            json!({"name":"Fonte ação", "downloads":[{"title":"Jogo", "uris":["magnet:test"]}]});
        assert_eq!(
            parse_reply(&serde_json::to_vec(&json!({"ok":true,"data":data})).unwrap()).unwrap(),
            data
        );
        assert!(parse_reply(br#"{"ok":true,"data":{"downloads":[]}}"#).is_err());
        assert!(parse_reply(b"<html>challenge</html>").is_err());
        assert_eq!(
            parse_reply(br#"{"ok":false,"error":"source_browser_cancelled"}"#).unwrap_err(),
            "source_browser_cancelled"
        );
    }

    #[tokio::test]
    async fn browser_spawn_failure_is_not_reported_as_a_source() {
        let browser = SourceBrowser::new(
            PathBuf::from("/nonexistent/arcadia-native"),
            PathBuf::from("/nonexistent/profile"),
            "pt-BR".into(),
        );
        assert!(browser
            .load("https://example.test/source.json")
            .await
            .unwrap_err()
            .starts_with("source_browser_start:"));
        assert!(browser
            .load("file:///etc/passwd")
            .await
            .unwrap_err()
            .contains("source_browser_invalid_url"));
    }
}
