//! Verificação de fontes em navegador isolado. O processo da UI nunca roda
//! GTK nem interpreta páginas; o helper só devolve o JSON da resposta HTTP.
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use arcadia_catalog::sources::{validar_downloads, ChallengeSolver};
use serde_json::{json, Value};

const HELPER_ARG: &str = "--arcadia-source-browser";
const MAX_CATALOG_BYTES: usize = 64 * 1024 * 1024;
const BROWSER_TIMEOUT: Duration = Duration::from_secs(120);

pub struct SourceBrowser {
    executable: PathBuf,
    profile: PathBuf,
    language: String,
    gate: tokio::sync::Mutex<()>,
}

impl SourceBrowser {
    pub fn new(executable: PathBuf, profile: PathBuf, language: String) -> Self {
        Self { executable, profile, language, gate: tokio::sync::Mutex::new(()) }
    }

    async fn load(&self, url: &str) -> Result<Value, String> {
        if !safe_browser_url(url) {
            return Err("source_browser_invalid_url".into());
        }
        // Um processo por vez compartilha os cookies, inclusive durante sync.
        let _guard = self.gate.lock().await;
        let child = tokio::process::Command::new(&self.executable)
            .arg(HELPER_ARG)
            .arg(url)
            .arg(&self.profile)
            .arg(&self.language)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| format!("source_browser_start: {error}"))?;
        let output = tokio::time::timeout(
            BROWSER_TIMEOUT + Duration::from_secs(5),
            child.wait_with_output(),
        ).await.map_err(|_| "source_browser_timeout")?
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

fn safe_browser_navigation(url: &str) -> bool {
    // A verificação cria frames vazios/srcdoc antes de carregar seu conteúdo.
    // Eles não abrem arquivos nem executam um protocolo externo.
    matches!(url, "about:blank" | "about:srcdoc") || safe_browser_url(url)
}

fn parse_reply(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > MAX_CATALOG_BYTES + 1024 {
        return Err("source_browser_too_large".into());
    }
    let mut reply: Value = serde_json::from_slice(bytes)
        .map_err(|_| "source_browser_invalid_reply")?;
    if reply.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(reply.get("error").and_then(Value::as_str)
            .unwrap_or("source_browser_failed").to_string());
    }
    let data = reply.get_mut("data").map(Value::take).ok_or("source_browser_invalid_reply")?;
    if validar_downloads(&data).is_none() {
        return Err("source_browser_invalid_catalog".into());
    }
    Ok(data)
}

/// Entrada alternativa do próprio executável: evita misturar os loops GTK/iced
/// e não precisa de Python, extensões ou perfil do navegador pessoal.
pub fn run_helper_if_requested() -> bool {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(HELPER_ARG)) {
        return false;
    }
    let result = match (args.next(), args.next(), args.next()) {
        (Some(url), Some(profile), Some(language)) => {
            run_browser(&url.to_string_lossy(), PathBuf::from(profile), &language.to_string_lossy())
        }
        _ => Err("source_browser_invalid_args".into()),
    };
    let reply = match result {
        Ok(data) => json!({"ok": true, "data": data}),
        Err(error) => json!({"ok": false, "error": error}),
    };
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut stdout, &reply);
    let _ = stdout.flush();
    true
}

#[cfg(not(target_os = "linux"))]
fn run_browser(_url: &str, _profile: PathBuf, _language: &str) -> Result<Value, String> {
    Err("source_browser_unavailable".into())
}

#[cfg(target_os = "linux")]
fn run_browser(url: &str, profile: PathBuf, language: &str) -> Result<Value, String> {
    use std::cell::{Cell, RefCell};
    use std::os::unix::fs::PermissionsExt;
    use std::rc::Rc;
    use gtk::prelude::*;
    use webkit2gtk::{
        CookieManagerExt, CookiePersistentStorage, DownloadExt, LoadEvent,
        NavigationPolicyDecision, NavigationPolicyDecisionExt, PermissionRequestExt,
        PolicyDecisionExt, PolicyDecisionType, URIRequestExt, URIResponseExt,
        WebContextExt, WebResourceExt, WebViewExt,
    };

    if !safe_browser_url(url) {
        return Err("source_browser_invalid_url".into());
    }
    gtk::init().map_err(|_| "source_browser_display_unavailable")?;
    std::fs::create_dir_all(&profile).map_err(|_| "source_browser_profile_failed")?;
    std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| "source_browser_profile_failed")?;
    let manager = webkit2gtk::WebsiteDataManager::builder()
        .base_data_directory(profile.join("data").to_string_lossy().as_ref())
        .base_cache_directory(profile.join("cache").to_string_lossy().as_ref())
        .build();
    let context = webkit2gtk::WebContext::with_website_data_manager(&manager);
    if let Some(cookies) = context.cookie_manager() {
        cookies.set_persistent_storage(
            &profile.join("cookies.sqlite").to_string_lossy(),
            CookiePersistentStorage::Sqlite,
        );
    }
    context.connect_download_started(|_, download| download.cancel());
    let view = webkit2gtk::WebView::with_context(&context);
    view.connect_permission_request(|_, request| { request.deny(); true });
    view.connect_create(|_, _| None);
    view.connect_decide_policy(|_, decision, kind| {
        if kind == PolicyDecisionType::NewWindowAction {
            decision.ignore();
            return true;
        }
        if kind == PolicyDecisionType::NavigationAction {
            if let Some(navigation) = decision.downcast_ref::<NavigationPolicyDecision>() {
                let allowed = navigation.navigation_action()
                    .and_then(|action| action.request()).and_then(|request| request.uri())
                    .is_some_and(|uri| safe_browser_navigation(&uri));
                if !allowed {
                    decision.ignore();
                    return true;
                }
            }
        }
        false
    });

    let i18n = crate::i18n::I18n::new(crate::i18n::Lang::from_code(language).unwrap_or_default());
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title(i18n.t("native.sources.browser_title"));
    window.set_default_size(800, 600);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let hint = gtk::Label::new(Some(i18n.t("native.sources.browser_hint")));
    hint.set_line_wrap(true);
    hint.set_margin_start(12);
    hint.set_margin_end(12);
    hint.set_margin_top(12);
    content.pack_start(&hint, false, false, 0);
    content.pack_start(&view, true, true, 0);
    window.add(&content);

    let result = Rc::new(RefCell::new(None));
    let reading = Rc::new(Cell::new(false));
    view.connect_load_changed({
        let result = Rc::clone(&result);
        let reading = Rc::clone(&reading);
        move |view, event| {
            if event != LoadEvent::Finished || reading.get() { return; }
            let Some(resource) = view.main_resource() else { return; };
            let Some(response) = resource.response() else { return; };
            if !(200..300).contains(&response.status_code()) { return; }
            if response.content_length() > MAX_CATALOG_BYTES as u64 {
                *result.borrow_mut() = Some(Err("source_browser_too_large".into()));
                gtk::main_quit();
                return;
            }
            reading.set(true);
            let result = Rc::clone(&result);
            let reading = Rc::clone(&reading);
            WebResourceExt::data(&resource, None::<&gtk::gio::Cancellable>, move |data| {
                reading.set(false);
                let Ok(bytes) = data else { return; };
                if bytes.len() > MAX_CATALOG_BYTES {
                    *result.borrow_mut() = Some(Err("source_browser_too_large".into()));
                    gtk::main_quit();
                    return;
                }
                // Bytes HTTP, não innerText: preserva o UTF-8 do catálogo.
                if let Ok(data) = serde_json::from_slice::<Value>(&bytes) {
                    if validar_downloads(&data).is_some() {
                        *result.borrow_mut() = Some(Ok(data));
                        gtk::main_quit();
                    }
                }
            });
        }
    });
    window.connect_delete_event({
        let result = Rc::clone(&result);
        move |_, _| {
            *result.borrow_mut() = Some(Err("source_browser_cancelled".into()));
            gtk::main_quit();
            gtk::glib::Propagation::Proceed
        }
    });
    view.connect_web_process_terminated({
        let result = Rc::clone(&result);
        move |_, _| {
            *result.borrow_mut() = Some(Err("source_browser_process_failed".into()));
            gtk::main_quit();
        }
    });
    gtk::glib::timeout_add_local_once(BROWSER_TIMEOUT, {
        let result = Rc::clone(&result);
        move || {
            *result.borrow_mut() = Some(Err("source_browser_timeout".into()));
            gtk::main_quit();
        }
    });
    window.show_all();
    view.load_uri(url);
    gtk::main();
    result.take().unwrap_or_else(|| Err("source_browser_cancelled".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_url_rejects_local_schemes_and_credentials() {
        assert!(safe_browser_url("https://hydralinks.cloud/sources/fitgirl.json"));
        assert!(!safe_browser_url("file:///etc/passwd"));
        assert!(!safe_browser_url("javascript:alert(1)"));
        assert!(!safe_browser_url("https://user:pass@example.test/source.json"));
        for frame in ["about:blank", "about:srcdoc"] {
            assert!(!safe_browser_url(frame), "frame não pode ser a URL da fonte");
            assert!(safe_browser_navigation(frame));
        }
        for blocked in ["file:///etc/passwd", "javascript:alert(1)", "about:config", "data:text/html,test"] {
            assert!(!safe_browser_navigation(blocked));
        }
    }

    #[test]
    fn browser_reply_requires_a_valid_catalog() {
        let data = json!({"name":"Fonte ação", "downloads":[{"title":"Jogo", "uris":["magnet:test"]}]});
        assert_eq!(parse_reply(&serde_json::to_vec(&json!({"ok":true,"data":data})).unwrap()).unwrap(), data);
        assert!(parse_reply(br#"{"ok":true,"data":{"downloads":[]}}"#).is_err());
        assert!(parse_reply(b"<html>challenge</html>").is_err());
        assert_eq!(parse_reply(br#"{"ok":false,"error":"source_browser_cancelled"}"#).unwrap_err(), "source_browser_cancelled");
    }

    #[tokio::test]
    async fn browser_spawn_failure_is_not_reported_as_a_source() {
        let browser = SourceBrowser::new(
            PathBuf::from("/nonexistent/arcadia-source-browser"),
            PathBuf::from("/nonexistent/profile"),
            "pt-BR".into(),
        );
        assert!(browser.load("https://example.test/source.json").await.unwrap_err()
            .starts_with("source_browser_start:"));
        assert!(browser.load("file:///etc/passwd").await.unwrap_err()
            .contains("source_browser_invalid_url"));
    }
}
