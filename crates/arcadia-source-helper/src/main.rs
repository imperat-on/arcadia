#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

//! Helper isolado de verificação de fontes.
//!
//! O binário principal do Arcadia Native é iced/wgpu e não carrega GTK nem
//! WebKit. Quando uma fonte responde com desafio Cloudflare, o processo
//! principal spawna este helper, que abre um navegador mínimo e isolado,
//! deixa o JavaScript do desafio rodar e devolve o JSON do catálogo por
//! stdout. O pai valida o payload antes de usá-lo.
//!
//! No Linux o navegador é o WebKitGTK; no Windows é o Edge/Chrome instalado,
//! controlado pelo DevTools Protocol com um perfil próprio do Arcadia.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
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

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
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

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn run_browser(_url: &str, _profile: PathBuf, _title: &str, _hint: &str) -> Result<Value, String> {
    Err("source_browser_unavailable".into())
}

#[cfg(target_os = "windows")]
fn run_browser(url: &str, profile: PathBuf, _title: &str, _hint: &str) -> Result<Value, String> {
    use std::process::Command;

    if !safe_browser_url(url) {
        return Err("source_browser_invalid_url".into());
    }
    std::fs::create_dir_all(&profile).map_err(|_| "source_browser_profile_failed")?;
    let user_data = profile.join("chromium");
    std::fs::create_dir_all(&user_data).map_err(|_| "source_browser_profile_failed")?;

    // O Edge vem primeiro porque existe por padrão no Windows 10/11. O perfil
    // fica dentro do data dir do Arcadia: nada do navegador pessoal é usado.
    let browser = windows_browser().ok_or("source_browser_unavailable")?;
    let mut child = Command::new(&browser)
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        // Sem isso o Edge se apresenta como automação e a Cloudflare mantém
        // o desafio para sempre, mesmo com a janela fora da tela.
        .arg("--disable-blink-features=AutomationControlled")
        .arg("--disable-backgrounding-occluded-windows")
        .arg("--disable-renderer-backgrounding")
        .arg("--remote-debugging-port=0")
        .arg(format!("--user-data-dir={}", user_data.display()))
        .arg("--window-size=960,720")
        // Fora da tela: o desafio roda como em uma janela oculta do Electron,
        // sem interromper o usuário. Se o cookie ainda estiver válido, o
        // catálogo carrega direto.
        // Fora da tela: o desafio roda como em uma janela oculta do Electron,
        // sem interromper o usuário. Se o cookie ainda estiver válido, o
        // catálogo carrega direto.
        .arg("--window-position=-32000,-32000")
        .arg(url)
        .spawn()
        .map_err(|error| format!("source_browser_start: {error}"))?;

    let pid = child.id();
    let result = windows_wait_for_catalog(&user_data, url);
    if let Some(port) = windows_devtools_port(&user_data, std::time::Instant::now()) {
        windows_close_browser(port);
    }
    windows_terminate_browser(pid);
    let _ = child.wait();
    result
}

#[cfg(target_os = "windows")]
fn windows_browser() -> Option<PathBuf> {
    let mut roots = Vec::new();
    for variable in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
        if let Some(root) = std::env::var_os(variable) {
            roots.push(PathBuf::from(root));
        }
    }

    let mut candidates = Vec::new();
    for root in &roots {
        candidates.push(
            root.join("Microsoft")
                .join("Edge")
                .join("Application")
                .join("msedge.exe"),
        );
    }
    for root in &roots {
        candidates.push(
            root.join("Google")
                .join("Chrome")
                .join("Application")
                .join("chrome.exe"),
        );
    }
    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(target_os = "windows")]
fn windows_devtools_port(user_data: &std::path::Path, deadline: std::time::Instant) -> Option<u16> {
    let path = user_data.join("DevToolsActivePort");
    loop {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Some(port) = text
                .lines()
                .next()
                .and_then(|line| line.trim().parse().ok())
            {
                return Some(port);
            }
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

#[cfg(target_os = "windows")]
fn windows_page_target(
    client: &reqwest::blocking::Client,
    port: u16,
    host: &str,
) -> Option<String> {
    let targets: Vec<Value> = client
        .get(format!("http://127.0.0.1:{port}/json/list"))
        .send()
        .ok()?
        .json()
        .ok()?;
    targets.into_iter().rev().find_map(|target| {
        if target.get("type").and_then(Value::as_str) != Some("page") {
            return None;
        }
        let target_url = target
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !(target_url.starts_with("http://") || target_url.starts_with("https://")) {
            return None;
        }
        if !host.is_empty() && !target_url.contains(host) {
            return None;
        }
        target
            .get("webSocketDebuggerUrl")
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

#[cfg(target_os = "windows")]
fn windows_wait_for_catalog(user_data: &std::path::Path, url: &str) -> Result<Value, String> {
    use std::time::Instant;

    let deadline = Instant::now() + BROWSER_TIMEOUT;
    windows_devtools_port(user_data, Instant::now() + Duration::from_secs(20))
        .ok_or("source_browser_start_failed")?;
    let host = url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_default();
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|_| "source_browser_devtools_unavailable")?;

    loop {
        if Instant::now() >= deadline {
            return Err("source_browser_timeout".into());
        }
        // O Edge pode reiniciar o processo do navegador e gravar outra porta
        // em DevToolsActivePort. Reler a cada tentativa evita ficar preso no
        // navegador antigo, que ainda mostra a página do desafio.
        let Some(port) = windows_devtools_port(user_data, Instant::now()) else {
            std::thread::sleep(Duration::from_secs(1));
            continue;
        };
        if let Some(data) = windows_evaluate_catalog(&client, port, &host) {
            return Ok(data);
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Consulta o target **mais recente** a cada tentativa. O Edge pode trocar o
/// target durante o desafio; manter um WebSocket do primeiro target deixava a
/// leitura presa na página antiga.
#[cfg(target_os = "windows")]
fn windows_evaluate_catalog(
    client: &reqwest::blocking::Client,
    port: u16,
    host: &str,
) -> Option<Value> {
    let ws_url = windows_page_target(client, port, host)?;
    if std::env::var_os("ARCADIA_SOURCE_HELPER_DEBUG").is_some() {
        eprintln!("[source-helper] porta={port} target={ws_url}");
    }
    let stream = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
    let (mut socket, _) = tungstenite::client::client(ws_url.as_str(), stream).ok()?;

    let probe = windows_cdp_value(
        &mut socket,
        1,
        "(() => { const t = document.body ? document.body.innerText : ''; const s = t.trimStart(); return {len: t.length, json: s.startsWith('{') || s.startsWith('[')}; })()",
    )?;
    let len = probe.get("len").and_then(Value::as_u64)? as usize;
    if len == 0 || probe.get("json").and_then(Value::as_bool) != Some(true) {
        return None;
    }

    // O CDP trunca respostas grandes de Runtime.evaluate por volta de 1 MB.
    // Lê o corpo em blocos pequenos e remonta no processo pai.
    const CHUNK: usize = 256 * 1024;
    let mut text = String::with_capacity(len);
    let mut start = 0_usize;
    let mut id = 2_u64;
    while start < len {
        let expression = format!(
            "(() => {{ const t = document.body ? document.body.innerText : ''; let end = Math.min({start} + {CHUNK}, t.length); if (end < t.length) {{ const c = t.charCodeAt(end - 1); if (c >= 0xD800 && c <= 0xDBFF) end -= 1; }} return {{end, text: t.slice({start}, end)}}; }})()",
            start = start,
        );
        let value = windows_cdp_value(&mut socket, id, &expression)?;
        id += 1;
        let end = value.get("end").and_then(Value::as_u64)? as usize;
        let chunk = value.get("text").and_then(Value::as_str)?;
        if end <= start {
            return None;
        }
        text.push_str(chunk);
        start = end;
    }

    match serde_json::from_str::<Value>(&text) {
        Ok(data) if looks_like_catalog(&data) => Some(data),
        _ => {
            if std::env::var_os("ARCADIA_SOURCE_HELPER_DEBUG").is_some() {
                let preview: String = text.chars().take(160).collect();
                eprintln!(
                    "[source-helper] corpo len={} ainda não é JSON: {preview:?}",
                    text.len()
                );
            }
            None
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_cdp_value(
    socket: &mut tungstenite::WebSocket<std::net::TcpStream>,
    id: u64,
    expression: &str,
) -> Option<Value> {
    use tungstenite::Message;

    let request = json!({
        "id": id,
        "method": "Runtime.evaluate",
        "params": {
            "expression": expression,
            "returnByValue": true,
        }
    });
    socket
        .send(Message::Text(request.to_string().into()))
        .ok()?;

    loop {
        match socket.read() {
            Ok(Message::Text(text)) => {
                let Ok(reply) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                if reply.get("id").and_then(Value::as_u64) != Some(id) {
                    continue;
                }
                return reply.pointer("/result/result/value").cloned();
            }
            Ok(Message::Ping(payload)) => {
                let _ = socket.send(Message::Pong(payload));
            }
            Ok(Message::Close(_)) => return None,
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return None
            }
            Err(_) => return None,
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_close_browser(port: u16) {
    use tungstenite::Message;

    let Ok(client) = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
    else {
        return;
    };
    let Ok(response) = client
        .get(format!("http://127.0.0.1:{port}/json/version"))
        .send()
    else {
        return;
    };
    let Ok(version) = response.json::<Value>() else {
        return;
    };
    let Some(ws_url) = version.get("webSocketDebuggerUrl").and_then(Value::as_str) else {
        return;
    };
    let Ok(stream) = std::net::TcpStream::connect(("127.0.0.1", port)) else {
        return;
    };
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
    let Ok((mut socket, _)) = tungstenite::client::client(ws_url, stream) else {
        return;
    };
    if socket
        .send(Message::Text(r#"{"id":1,"method":"Browser.close"}"#.into()))
        .is_ok()
    {
        let _ = socket.read();
    }
}

#[cfg(target_os = "windows")]
fn windows_terminate_browser(pid: u32) {
    use std::os::windows::process::CommandExt;

    // taskkill é um programa de console; CREATE_NO_WINDOW evita o flash de
    // console no processo principal, que é GUI.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
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
