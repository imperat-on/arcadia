//! Helper isolado de verificação de fontes.
//!
//! O binário principal do Arcadia Native é iced/wgpu e não carrega GTK nem
//! WebKit. Quando uma fonte responde com desafio Cloudflare, o processo
//! principal spawna este helper, que abre uma janela WebKit mínima e isolada,
//! deixa o JavaScript do desafio rodar e devolve o JSON do catálogo por
//! stdout. O pai valida o payload antes de usá-lo.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};

const MAX_CATALOG_BYTES: usize = 64 * 1024 * 1024;
const BROWSER_TIMEOUT: Duration = Duration::from_secs(120);

fn main() {
    let mut args = std::env::args_os().skip(1);
    let result = match (args.next(), args.next(), args.next(), args.next()) {
        (Some(url), Some(profile), Some(title), Some(hint)) => run_browser(
            &url.to_string_lossy(),
            PathBuf::from(profile),
            &title.to_string_lossy(),
            &hint.to_string_lossy(),
        ),
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
}

fn safe_browser_url(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| {
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

/// O helper só precisa reconhecer que a página deixou de ser o desafio e
/// virou um catálogo com downloads. A validação estrita continua no processo
/// principal (`parse_reply`), que tem acesso ao validador completo.
fn looks_like_catalog(data: &Value) -> bool {
    data.get("downloads")
        .and_then(Value::as_array)
        .is_some_and(|downloads| !downloads.is_empty())
}

#[cfg(not(target_os = "linux"))]
fn run_browser(_url: &str, _profile: PathBuf, _title: &str, _hint: &str) -> Result<Value, String> {
    Err("source_browser_unavailable".into())
}

#[cfg(target_os = "linux")]
fn run_browser(url: &str, profile: PathBuf, title: &str, hint: &str) -> Result<Value, String> {
    use std::cell::{Cell, RefCell};
    use std::os::unix::fs::PermissionsExt;
    use std::rc::Rc;

    use gtk::prelude::*;
    use webkit2gtk::{
        CookieManagerExt, CookiePersistentStorage, DownloadExt, LoadEvent,
        NavigationPolicyDecision, NavigationPolicyDecisionExt, PermissionRequestExt,
        PolicyDecisionExt, PolicyDecisionType, URIRequestExt, URIResponseExt, WebContextExt,
        WebResourceExt, WebViewExt,
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
    view.connect_permission_request(|_, request| {
        request.deny();
        true
    });
    view.connect_create(|_, _| None);
    view.connect_decide_policy(|_, decision, kind| {
        if kind == PolicyDecisionType::NewWindowAction {
            decision.ignore();
            return true;
        }
        if kind == PolicyDecisionType::NavigationAction {
            if let Some(navigation) = decision.downcast_ref::<NavigationPolicyDecision>() {
                let allowed = navigation
                    .navigation_action()
                    .and_then(|action| action.request())
                    .and_then(|request| request.uri())
                    .is_some_and(|uri| safe_browser_navigation(&uri));
                if !allowed {
                    decision.ignore();
                    return true;
                }
            }
        }
        false
    });

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title(title);
    window.set_default_size(800, 600);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let label = gtk::Label::new(Some(hint));
    label.set_line_wrap(true);
    label.set_margin_start(12);
    label.set_margin_end(12);
    label.set_margin_top(12);
    content.pack_start(&label, false, false, 0);
    content.pack_start(&view, true, true, 0);
    window.add(&content);

    let result = Rc::new(RefCell::new(None));
    let reading = Rc::new(Cell::new(false));
    view.connect_load_changed({
        let result = Rc::clone(&result);
        let reading = Rc::clone(&reading);
        move |view, event| {
            if event != LoadEvent::Finished || reading.get() {
                return;
            }
            let Some(resource) = view.main_resource() else {
                return;
            };
            let Some(response) = resource.response() else {
                return;
            };
            if !(200..300).contains(&response.status_code()) {
                return;
            }
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
                let Ok(bytes) = data else {
                    return;
                };
                if bytes.len() > MAX_CATALOG_BYTES {
                    *result.borrow_mut() = Some(Err("source_browser_too_large".into()));
                    gtk::main_quit();
                    return;
                }
                // Bytes HTTP, não innerText: preserva o UTF-8 do catálogo.
                if let Ok(data) = serde_json::from_slice::<Value>(&bytes) {
                    if looks_like_catalog(&data) {
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
    result
        .take()
        .unwrap_or_else(|| Err("source_browser_cancelled".into()))
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
        for frame in ["about:blank", "about:srcdoc"] {
            assert!(
                !safe_browser_url(frame),
                "frame não pode ser a URL da fonte"
            );
            assert!(safe_browser_navigation(frame));
        }
        for blocked in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "about:config",
            "data:text/html,test",
        ] {
            assert!(!safe_browser_navigation(blocked));
        }
    }

    #[test]
    fn catalog_detection_requires_a_non_empty_downloads_array() {
        assert!(looks_like_catalog(&json!({
            "name": "Fonte",
            "downloads": [{"title": "Jogo"}]
        })));
        assert!(!looks_like_catalog(&json!({"downloads": []})));
        assert!(!looks_like_catalog(&json!({"name": "sem downloads"})));
        assert!(!looks_like_catalog(&json!({"downloads": "não é lista"})));
    }
}
