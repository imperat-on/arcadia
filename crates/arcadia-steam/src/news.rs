//! Notícias da Steam: extração de capa e cliente de `GetNewsForApp`.
//!
//! Porte de `app/electron/steam-news.js` e do handler `news:game` de
//! `app/electron/main.js`:
//!
//! - endpoint `ISteamNews/GetNewsForApp/v2/` com `appid`, `count=6`,
//!   `maxlength=500` e `feeds=steam_community_announcements`;
//! - até 6 itens, `id`/`title`/`source`/`url` com os mesmos fallbacks;
//! - resumo sem BBCode/HTML, com espaços colapsados;
//! - capa extraída do conteúdo (`[img]`, `[previewimg]`, `og:image`, `<img>`)
//!   e, quando falta, da página oficial — somente hosts da Steam e somente
//!   HTTPS, com cache de 6 h;
//! - `date` no formato ISO-8601 de `new Date(date * 1000).toISOString()`.
//!
//! O cliente HTTP é o [`arcadia_api::HttpClient`] (mesma forma do resto do
//! workspace); os testes usam um [`PageFetcher`] injetado ou `wiremock`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex as SyncMutex, OnceLock};
use std::time::{Duration, Instant};

use arcadia_api::{HttpClient, HttpRequest, TransportError};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{watch, Mutex as AsyncMutex};
use url::Url;

use crate::js::{js_number_value, js_string_value, js_trim, js_truthy_value};

pub const NEWS_ENDPOINT: &str = "https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/";
pub const NEWS_COUNT: usize = 6;
pub const NEWS_MAX_LENGTH: u32 = 500;
pub const NEWS_FEED: &str = "steam_community_announcements";
pub const NEWS_IMAGE_TTL: Duration = Duration::from_secs(6 * 60 * 60);
pub const NEWS_PAGE_TIMEOUT: Duration = Duration::from_secs(8);
pub const STEAM_NEWS_HOSTS: [&str; 3] = [
    "steamstore-a.akamaihd.net",
    "store.steampowered.com",
    "steamcommunity.com",
];

#[derive(Debug, Error)]
pub enum NewsError {
    #[error("URL inválida: {0}")]
    InvalidUrl(String),
    #[error("falha de rede: {0}")]
    Transport(#[from] TransportError),
    #[error("HTTP {status}")]
    Http { status: u16 },
    #[error("resposta JSON inválida: {0}")]
    Json(#[from] serde_json::Error),
    #[error("data de notícia inválida: {0}")]
    InvalidDate(String),
    #[error("item de notícia inválido")]
    InvalidItem,
}

/// Item devolvido pelo IPC `news:game`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SteamNewsItem {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub source: String,
    pub url: String,
    pub image: String,
    pub date: String,
}

// --- extração de imagens ----------------------------------------------------

fn decode_attribute(value: &str) -> String {
    static AMP: OnceLock<Regex> = OnceLock::new();
    static QUOT: OnceLock<Regex> = OnceLock::new();
    static APOS: OnceLock<Regex> = OnceLock::new();

    let amp = AMP.get_or_init(|| Regex::new("(?i)&amp;").expect("regex amp"));
    let quot = QUOT.get_or_init(|| Regex::new("(?i)&quot;").expect("regex quot"));
    let apos = APOS.get_or_init(|| Regex::new("(?i)&#39;|&apos;").expect("regex apos"));

    let value = amp.replace_all(value, "&");
    let value = quot.replace_all(&value, "\"");
    apos.replace_all(&value, "'").into_owned()
}

/// Normaliza uma URL de imagem da Steam: expande `{STEAM_CLAN_IMAGE}`,
/// aceita `//host/...` e só devolve HTTPS absoluto.
pub fn normalize_steam_image_url(value: &str) -> String {
    static CLAN: OnceLock<Regex> = OnceLock::new();

    let decoded = decode_attribute(value);
    let raw = js_trim(&decoded);
    if raw.is_empty() {
        return String::new();
    }
    let clan = CLAN.get_or_init(|| {
        Regex::new(r"(?i)^\{STEAM_CLAN_IMAGE\}/+([0-9]+)/(.+)$").expect("regex clan image")
    });
    let expanded = clan
        .replace(raw, "https://clan.cloudflare.steamstatic.com/images/$1/$2")
        .into_owned();
    let absolute = if let Some(rest) = expanded.strip_prefix("//") {
        format!("https:{rest}")
    } else {
        expanded
    };
    match Url::parse(&absolute) {
        Ok(url) if url.scheme() == "https" => url.to_string(),
        _ => String::new(),
    }
}

fn extract_attribute(tag: &str, name: &str) -> String {
    let pattern = format!(
        r#"(?i)\b{}\s*=\s*(?:["']([^"']*)["']|([^\s>]+))"#,
        regex::escape(name)
    );
    let Ok(regex) = Regex::new(&pattern) else {
        return String::new();
    };
    let Some(captures) = regex.captures(tag) else {
        return String::new();
    };
    let raw = captures
        .get(1)
        .or_else(|| captures.get(2))
        .map(|capture| capture.as_str())
        .unwrap_or("");
    decode_attribute(raw)
}

/// Primeira imagem encontrada no markup de uma notícia.
pub fn extract_steam_news_image(markup: &str) -> String {
    static BBCODE_IMG: OnceLock<Regex> = OnceLock::new();
    static BBCODE_PREVIEW: OnceLock<Regex> = OnceLock::new();
    static TAG: OnceLock<Regex> = OnceLock::new();
    static HTML_IMG: OnceLock<Regex> = OnceLock::new();

    let bbcode_img = BBCODE_IMG.get_or_init(|| {
        Regex::new(r#"(?i)\[img(?:\s+src\s*=\s*["']?|\s*=\s*["']?)?([^\]"']+)["']?\]\s*\[/img\]"#)
            .expect("regex bbcode img")
    });
    let bbcode_preview = BBCODE_PREVIEW.get_or_init(|| {
        Regex::new(r#"(?i)\[previewimg(?:\s*=\s*["']?)?([^\]"']+)["']?"#)
            .expect("regex bbcode preview")
    });
    let tag = TAG.get_or_init(|| Regex::new(r"(?i)<(?:meta|link)\b[^>]*>").expect("regex tag"));
    let html_img = HTML_IMG.get_or_init(|| {
        Regex::new(r#"(?i)<img\b[^>]*\bsrc\s*=\s*(?:["']([^"']+)["']|([^\s>]+))"#)
            .expect("regex html img")
    });

    let bbcode = bbcode_img
        .captures(markup)
        .and_then(|captures| captures.get(1))
        .map(|capture| capture.as_str())
        .or_else(|| {
            bbcode_preview
                .captures(markup)
                .and_then(|captures| captures.get(1))
                .map(|capture| capture.as_str())
        });
    let normalized_bbcode = normalize_steam_image_url(bbcode.unwrap_or(""));
    if !normalized_bbcode.is_empty() {
        return normalized_bbcode;
    }

    for found in tag.find_iter(markup) {
        let tag_text = found.as_str();
        let mut kind = extract_attribute(tag_text, "property");
        if kind.is_empty() {
            kind = extract_attribute(tag_text, "name");
        }
        if kind.is_empty() {
            kind = extract_attribute(tag_text, "rel");
        }
        let kind = kind.to_ascii_lowercase();
        if !matches!(
            kind.as_str(),
            "og:image" | "og:image:url" | "twitter:image" | "twitter:image:src" | "image_src"
        ) {
            continue;
        }
        let mut source = extract_attribute(tag_text, "content");
        if source.is_empty() {
            source = extract_attribute(tag_text, "href");
        }
        let image = normalize_steam_image_url(&source);
        if !image.is_empty() {
            return image;
        }
    }

    let image = html_img
        .captures(markup)
        .and_then(|captures| {
            captures
                .get(1)
                .or_else(|| captures.get(2))
                .map(|capture| capture.as_str())
        })
        .unwrap_or("");
    normalize_steam_image_url(image)
}

/// `true` somente para HTTPS em um dos três hosts de notícias da Steam.
pub fn is_steam_news_url(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    if url.scheme() != "https" {
        return false;
    }
    url.host_str()
        .map(|host| {
            STEAM_NEWS_HOSTS
                .iter()
                .any(|allowed| host.eq_ignore_ascii_case(allowed))
        })
        .unwrap_or(false)
}

fn summarize(markup: &str) -> String {
    static IMG_BLOCK: OnceLock<Regex> = OnceLock::new();
    static BBCODE: OnceLock<Regex> = OnceLock::new();
    static HTML_TAG: OnceLock<Regex> = OnceLock::new();
    static SPACES: OnceLock<Regex> = OnceLock::new();

    let img_block = IMG_BLOCK
        .get_or_init(|| Regex::new(r"(?i)\[img\][\s\S]*?\[/img\]").expect("regex img block"));
    let bbcode = BBCODE.get_or_init(|| Regex::new(r"\[[^\]]+\]").expect("regex bbcode summary"));
    let html = HTML_TAG.get_or_init(|| Regex::new(r"<[^>]+>").expect("regex html summary"));
    let spaces = SPACES.get_or_init(|| Regex::new(r"\s+").expect("regex spaces"));

    let text = img_block.replace_all(markup, " ");
    let text = bbcode.replace_all(&text, " ");
    let text = html.replace_all(&text, " ");
    let text = spaces.replace_all(&text, " ");
    js_trim(&text).to_string()
}

// --- página da notícia (capa via og:image) ---------------------------------

pub type PageFuture<'a> = Pin<Box<dyn Future<Output = Option<String>> + Send + 'a>>;

/// Busca uma página HTML. Existe para os testes não tocarem na rede.
pub trait PageFetcher: Send + Sync {
    fn fetch_page<'a>(&'a self, url: &'a str) -> PageFuture<'a>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoPageFetcher;

impl PageFetcher for NoPageFetcher {
    fn fetch_page<'a>(&'a self, _url: &'a str) -> PageFuture<'a> {
        Box::pin(async { None })
    }
}

impl PageFetcher for HttpClient {
    fn fetch_page<'a>(&'a self, url: &'a str) -> PageFuture<'a> {
        Box::pin(async move {
            let request = HttpRequest::get(url)
                .header("User-Agent", "arcadia")
                .header("Accept", "text/html");
            let response = self.send(&request).await.ok()?;
            if !response.is_success() {
                return None;
            }
            Some(response.text())
        })
    }
}

impl<T: PageFetcher + ?Sized> PageFetcher for Arc<T> {
    fn fetch_page<'a>(&'a self, url: &'a str) -> PageFuture<'a> {
        (**self).fetch_page(url)
    }
}

/// Busca HTML com o mesmo timeout de 8 s do `AbortSignal.timeout` do JS.
#[derive(Debug, Clone)]
pub struct HttpPageFetcher {
    pub http: HttpClient,
    pub timeout: Duration,
}

impl HttpPageFetcher {
    pub fn new(http: HttpClient) -> Self {
        Self {
            http,
            timeout: NEWS_PAGE_TIMEOUT,
        }
    }

    pub fn with_timeout(http: HttpClient, timeout: Duration) -> Self {
        Self { http, timeout }
    }
}

impl PageFetcher for HttpPageFetcher {
    fn fetch_page<'a>(&'a self, url: &'a str) -> PageFuture<'a> {
        Box::pin(async move {
            let request = HttpRequest::get(url)
                .header("User-Agent", "arcadia")
                .header("Accept", "text/html");
            let response = tokio::time::timeout(self.timeout, self.http.send(&request))
                .await
                .ok()?
                .ok()?;
            if !response.is_success() {
                return None;
            }
            Some(response.text())
        })
    }
}

enum ImageState {
    Pending,
    Ready(String, Instant),
}

struct ImageCell {
    state: SyncMutex<ImageState>,
    updates: watch::Sender<u64>,
}

impl ImageCell {
    fn pending() -> Arc<Self> {
        let (updates, _) = watch::channel(0u64);
        Arc::new(Self {
            state: SyncMutex::new(ImageState::Pending),
            updates,
        })
    }
}

enum Lookup {
    Hit(String),
    Expired,
    Wait(Arc<ImageCell>, watch::Receiver<u64>),
}

/// Resolve a capa de uma notícia uma única vez por URL, com cache e
/// deduplicação de chamadas concorrentes (equivalente ao `cache`/`inFlight`
/// do JS).
pub struct SteamNewsImageResolver<F: PageFetcher> {
    fetcher: F,
    ttl: Duration,
    cache: AsyncMutex<HashMap<String, Arc<ImageCell>>>,
}

impl<F: PageFetcher> SteamNewsImageResolver<F> {
    pub fn new(fetcher: F) -> Self {
        Self::with_ttl(fetcher, NEWS_IMAGE_TTL)
    }

    pub fn with_ttl(fetcher: F, ttl: Duration) -> Self {
        Self {
            fetcher,
            ttl,
            cache: AsyncMutex::new(HashMap::new()),
        }
    }

    pub fn fetcher(&self) -> &F {
        &self.fetcher
    }

    pub async fn resolve(&self, contents: &str, page_url: &str) -> String {
        let inline = extract_steam_news_image(contents);
        if !inline.is_empty() {
            return inline;
        }
        if !is_steam_news_url(page_url) {
            return String::new();
        }

        let cell = loop {
            let mut cache = self.cache.lock().await;
            let lookup = match cache.get(page_url).cloned() {
                Some(cell) => {
                    let state = cell.state.lock().unwrap_or_else(|error| error.into_inner());
                    match &*state {
                        ImageState::Ready(image, expires) if *expires > Instant::now() => {
                            Lookup::Hit(image.clone())
                        }
                        ImageState::Ready(_, _) => Lookup::Expired,
                        ImageState::Pending => {
                            // Assina ANTES de soltar os locks: uma conclusão
                            // concorrente não pode passar despercebida.
                            Lookup::Wait(Arc::clone(&cell), cell.updates.subscribe())
                        }
                    }
                }
                None => Lookup::Expired,
            };
            match lookup {
                Lookup::Hit(image) => return image,
                Lookup::Expired => {
                    cache.remove(page_url);
                }
                Lookup::Wait(_cell, mut updates) => {
                    drop(cache);
                    let _ = updates.changed().await;
                    continue;
                }
            }

            let cell = ImageCell::pending();
            cache.insert(page_url.to_string(), Arc::clone(&cell));
            break cell;
        };

        let body = self.fetcher.fetch_page(page_url).await.unwrap_or_default();
        let image = extract_steam_news_image(&body);
        {
            let mut state = cell.state.lock().unwrap_or_else(|error| error.into_inner());
            *state = ImageState::Ready(image.clone(), Instant::now() + self.ttl);
        }
        let _ = cell.updates.send(1);
        image
    }
}

// --- payload de GetNewsForApp ----------------------------------------------

/// Faz o parse do envelope JSON e devolve os itens já normalizados (a capa é
/// apenas a inline; o cliente busca a página quando ela falta).
pub fn parse_game_news(appid: &str, payload: &Value) -> Result<Vec<SteamNewsItem>, NewsError> {
    let Some(items) = payload
        .get("appnews")
        .and_then(|value| value.get("newsitems"))
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };

    let mut output = Vec::new();
    for item in items.iter().take(NEWS_COUNT) {
        output.push(parse_news_item(appid, item)?);
    }
    Ok(output)
}

pub fn parse_game_news_text(appid: &str, text: &str) -> Result<Vec<SteamNewsItem>, NewsError> {
    let payload: Value = serde_json::from_str(text)?;
    parse_game_news(appid, &payload)
}

fn parse_news_item(appid: &str, item: &Value) -> Result<SteamNewsItem, NewsError> {
    let object = item.as_object().ok_or(NewsError::InvalidItem)?;

    let contents = object
        .get("contents")
        .filter(|value| js_truthy_value(value))
        .map(js_string_value)
        .unwrap_or_default();

    // `item.gid || item.url || item.date` — o último operando é devolvido
    // mesmo quando é falsy (undefined vira "undefined").
    let mut id_value = None;
    for key in ["gid", "url"] {
        if let Some(value) = object.get(key).filter(|value| js_truthy_value(value)) {
            id_value = Some(value);
            break;
        }
    }
    if id_value.is_none() {
        id_value = object.get("date");
    }
    let id = id_value
        .map(js_string_value)
        .unwrap_or_else(|| "undefined".to_string());

    let title = object
        .get("title")
        .filter(|value| js_truthy_value(value))
        .map(js_string_value)
        .unwrap_or_else(|| "Atualização".to_string());
    let source = object
        .get("author")
        .filter(|value| js_truthy_value(value))
        .map(js_string_value)
        .unwrap_or_else(|| "Steam".to_string());
    let url = object
        .get("url")
        .filter(|value| js_truthy_value(value))
        .map(js_string_value)
        .unwrap_or_else(|| format!("https://store.steampowered.com/news/app/{appid}"));

    let seconds = match object.get("date") {
        Some(value) if js_truthy_value(value) => js_number_value(value),
        _ => Some(0.0),
    }
    .ok_or_else(|| NewsError::InvalidDate("date não numérico".to_string()))?;
    let date = format_iso8601(seconds * 1000.0)
        .ok_or_else(|| NewsError::InvalidDate(seconds.to_string()))?;

    Ok(SteamNewsItem {
        id,
        title,
        summary: summarize(&contents),
        source,
        url,
        image: extract_steam_news_image(&contents),
        date,
    })
}

/// `new Date(ms).toISOString()`, inclusive para datas anteriores a 1970.
fn format_iso8601(milliseconds: f64) -> Option<String> {
    // TimeClip do ECMAScript: ±8.64e15 ms.
    if !milliseconds.is_finite() || milliseconds.abs() > 8.64e15 {
        return None;
    }
    let milliseconds = milliseconds.trunc() as i64;
    let days = milliseconds.div_euclid(86_400_000);
    let remainder = milliseconds.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    let hour = remainder / 3_600_000;
    let minute = (remainder % 3_600_000) / 60_000;
    let second = (remainder % 60_000) / 1_000;
    let millis = remainder % 1_000;
    let year_text = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else if year > 0 {
        format!("+{year:06}")
    } else {
        format!("-{:06}", -year)
    };
    Some(format!(
        "{year_text}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z"
    ))
}

/// Howard Hinnant, `civil_from_days` (dias desde 1970-01-01).
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
    (year + i64::from(month <= 2), month as u32, day as u32)
}

fn is_valid_appid(appid: &str) -> bool {
    !appid.is_empty() && appid.len() <= 12 && appid.bytes().all(|byte| byte.is_ascii_digit())
}

/// Cliente de notícias sobre o mesmo [`HttpClient`] do `arcadia-api`.
#[derive(Clone)]
pub struct NewsClient {
    http: HttpClient,
    endpoint: String,
    resolver: Arc<SteamNewsImageResolver<HttpPageFetcher>>,
}

impl NewsClient {
    pub fn new(http: HttpClient) -> Self {
        Self::with_endpoint(http, NEWS_ENDPOINT)
    }

    pub fn with_endpoint(http: HttpClient, endpoint: impl Into<String>) -> Self {
        let resolver = Arc::new(SteamNewsImageResolver::new(HttpPageFetcher::new(
            http.clone(),
        )));
        Self {
            http,
            endpoint: endpoint.into(),
            resolver,
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    fn request_url(&self, appid: &str) -> Result<String, NewsError> {
        let mut url =
            Url::parse(&self.endpoint).map_err(|error| NewsError::InvalidUrl(error.to_string()))?;
        url.query_pairs_mut()
            .append_pair("appid", appid)
            .append_pair("count", &NEWS_COUNT.to_string())
            .append_pair("maxlength", &NEWS_MAX_LENGTH.to_string())
            .append_pair("feeds", NEWS_FEED);
        Ok(url.to_string())
    }

    /// Busca estrita: erro de rede/HTTP/JSON/data é devolvido ao chamador.
    pub async fn fetch_game_news(&self, appid: &str) -> Result<Vec<SteamNewsItem>, NewsError> {
        let appid = js_trim(appid);
        if !is_valid_appid(appid) {
            return Ok(Vec::new());
        }
        let request = HttpRequest::get(self.request_url(appid)?).header("User-Agent", "arcadia");
        let response = self.http.send(&request).await?;
        if !response.is_success() {
            return Err(NewsError::Http {
                status: response.status,
            });
        }
        let payload: Value = serde_json::from_slice(&response.body)?;
        self.fetch_from_payload(appid, &payload).await
    }

    /// Mesmo contrato do IPC congelado: qualquer falha vira lista vazia.
    pub async fn game_news(&self, appid: &str) -> Vec<SteamNewsItem> {
        self.fetch_game_news(appid).await.unwrap_or_default()
    }

    pub async fn fetch_from_payload(
        &self,
        appid: &str,
        payload: &Value,
    ) -> Result<Vec<SteamNewsItem>, NewsError> {
        let mut items = parse_game_news(appid, payload)?;
        for item in &mut items {
            if item.image.is_empty() {
                item.image = self.resolver.resolve("", &item.url).await;
            }
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const NEWS_JSON: &str = r#"{
        "appnews": {
            "appid": 990080,
            "newsitems": [
                {
                    "gid": 111,
                    "title": "Patch notes",
                    "url": "https://store.steampowered.com/news/app/990080/view/1",
                    "author": "Valve",
                    "contents": "[img src=\"{STEAM_CLAN_IMAGE}/35190511/capa.png\"][/img] <b>Hello</b> [b]world[/b]",
                    "date": 1700000000
                },
                {
                    "gid": 0,
                    "title": "",
                    "url": "https://store.steampowered.com/news/app/990080/view/2",
                    "author": "",
                    "contents": "[previewimg=https://clan.akamai.steamstatic.com/images/2/post.png]",
                    "date": 0
                },
                {
                    "gid": 333,
                    "title": "Terceira",
                    "url": "https://steamcommunity.com/games/990080/announcements/detail/3",
                    "author": "Comunidade",
                    "contents": "[img src=\"https://clan.akamai.steamstatic.com/images/3/p.png\"][/img] texto com imagem",
                    "date": 1700000100
                }
            ]
        }
    }"#;

    #[test]
    fn normalizes_steam_clan_images_and_page_metadata() {
        assert_eq!(
            normalize_steam_image_url("{STEAM_CLAN_IMAGE}/35190511/capa.png"),
            "https://clan.cloudflare.steamstatic.com/images/35190511/capa.png"
        );
        assert_eq!(
            normalize_steam_image_url("//clan.akamai.steamstatic.com/images/1/a.png"),
            "https://clan.akamai.steamstatic.com/images/1/a.png"
        );
        assert_eq!(
            normalize_steam_image_url("http://clan.akamai.steamstatic.com/images/1/a.png"),
            "",
            "só HTTPS"
        );
        assert_eq!(normalize_steam_image_url("lixo"), "");
        assert_eq!(normalize_steam_image_url(""), "");
        assert_eq!(
            normalize_steam_image_url("https://x.test/a.png?x=1&amp;y=2"),
            "https://x.test/a.png?x=1&y=2"
        );
    }

    #[test]
    fn extracts_images_like_the_js_oracle() {
        assert_eq!(
            extract_steam_news_image(
                "<meta property=\"og:image\" content=\"https://clan.akamai.steamstatic.com/images/1/news.jpg\">"
            ),
            "https://clan.akamai.steamstatic.com/images/1/news.jpg"
        );
        assert_eq!(
            extract_steam_news_image("[img src=\"{STEAM_CLAN_IMAGE}/22/post.webp\"][/img]"),
            "https://clan.cloudflare.steamstatic.com/images/22/post.webp"
        );
        // O regex do JS exige `src=`/`=` no [img]; o formato "solto" não casa.
        assert_eq!(
            extract_steam_news_image(
                "[img]https://clan.akamai.steamstatic.com/images/2/a.png[/img]"
            ),
            ""
        );
        assert_eq!(
            extract_steam_news_image(
                "[img=https://clan.akamai.steamstatic.com/images/2/a.png][/img]"
            ),
            "https://clan.akamai.steamstatic.com/images/2/a.png"
        );
        assert_eq!(
            extract_steam_news_image(
                "[previewimg=https://clan.akamai.steamstatic.com/images/3/p.png]"
            ),
            "https://clan.akamai.steamstatic.com/images/3/p.png"
        );
        assert_eq!(
            extract_steam_news_image(
                "<link rel=\"image_src\" href=\"https://clan.akamai.steamstatic.com/images/4/p.png\">"
            ),
            "https://clan.akamai.steamstatic.com/images/4/p.png"
        );
        assert_eq!(
            extract_steam_news_image(
                "<meta name=\"twitter:image\" content=\"https://clan.akamai.steamstatic.com/images/5/p.png\">"
            ),
            "https://clan.akamai.steamstatic.com/images/5/p.png"
        );
        assert_eq!(
            extract_steam_news_image(
                "<p>oi</p><img alt=\"x\" src=\"https://clan.akamai.steamstatic.com/images/6/p.png\">"
            ),
            "https://clan.akamai.steamstatic.com/images/6/p.png"
        );
        assert_eq!(extract_steam_news_image("sem imagem"), "");
        assert_eq!(
            extract_steam_news_image(
                "<meta property=\"og:image\" content=\"http://x.test/a.png\">"
            ),
            ""
        );
    }

    #[test]
    fn image_attributes_are_decoded() {
        assert_eq!(
            extract_steam_news_image(
                "<meta property='og:image' content='https://x.test/a.png?x=1&amp;y=2'>"
            ),
            "https://x.test/a.png?x=1&y=2"
        );
        assert_eq!(
            extract_attribute(
                "<meta content=\"diz &quot;oi&quot; &amp; tchau\">",
                "content"
            ),
            "diz \"oi\" & tchau"
        );
    }

    #[test]
    fn only_https_steam_hosts_are_accepted() {
        assert!(is_steam_news_url(
            "https://steamstore-a.akamaihd.net/news/externalpost/steam_community_announcements/123"
        ));
        assert!(is_steam_news_url(
            "https://store.steampowered.com/news/app/1"
        ));
        assert!(is_steam_news_url("https://steamcommunity.com/games/1"));
        assert!(!is_steam_news_url(
            "http://store.steampowered.com/news/app/1"
        ));
        assert!(!is_steam_news_url("https://evil.example/post"));
        assert!(!is_steam_news_url(
            "https://store.steampowered.com.evil.example/"
        ));
        assert!(!is_steam_news_url("lixo"));
    }

    #[test]
    fn summary_strips_bbcode_and_html_and_collapses_spaces() {
        assert_eq!(
            summarize("<p>Hello</p> [b]world[/b] [img]https://x.test/a.png[/img]"),
            "Hello world"
        );
        assert_eq!(summarize("  a\n\n b  "), "a b");
    }

    #[test]
    fn parses_the_json_envelope_with_fallbacks_and_limits() {
        let payload: Value = serde_json::from_str(NEWS_JSON).unwrap();
        let items = parse_game_news("990080", &payload).unwrap();
        assert_eq!(items.len(), 3);

        assert_eq!(items[0].id, "111");
        assert_eq!(items[0].title, "Patch notes");
        assert_eq!(items[0].source, "Valve");
        assert_eq!(items[0].summary, "Hello world");
        assert_eq!(
            items[0].image,
            "https://clan.cloudflare.steamstatic.com/images/35190511/capa.png"
        );
        assert_eq!(items[0].date, "2023-11-14T22:13:20.000Z");

        assert_eq!(
            items[1].id,
            "https://store.steampowered.com/news/app/990080/view/2"
        );
        assert_eq!(items[1].title, "Atualização");
        assert_eq!(items[1].source, "Steam");
        assert_eq!(items[1].date, "1970-01-01T00:00:00.000Z");
        assert_eq!(
            items[1].image,
            "https://clan.akamai.steamstatic.com/images/2/post.png"
        );

        assert_eq!(items[2].id, "333");
    }

    #[test]
    fn newsitems_limits_to_six_and_missing_envelope_is_empty() {
        let items: Vec<Value> = (0..9)
            .map(|index| {
                json!({
                    "gid": index + 1,
                    "title": format!("Notícia {index}"),
                    "url": format!("https://store.steampowered.com/news/app/1/view/{index}"),
                    "date": 1_700_000_000 + index
                })
            })
            .collect();
        let payload = json!({ "appnews": { "newsitems": items } });
        assert_eq!(parse_game_news("1", &payload).unwrap().len(), 6);

        assert!(parse_game_news("1", &json!({})).unwrap().is_empty());
        assert!(
            parse_game_news("1", &json!({ "appnews": { "newsitems": "x" } }))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn id_and_date_follow_javascript_truthiness() {
        let payload = json!({
            "appnews": {
                "newsitems": [
                    { "gid": 0, "date": 0 },
                    { "gid": 0, "url": "https://store.steampowered.com/x", "date": 0 },
                    { "title": "sem campos" }
                ]
            }
        });
        let items = parse_game_news("1", &payload).unwrap();
        // gid=0, url ausente, date=0 -> o último operando (0) vira "0".
        assert_eq!(items[0].id, "0");
        assert_eq!(items[1].id, "https://store.steampowered.com/x");
        // Sem gid/url/date: expressão devolve undefined.
        assert_eq!(items[2].id, "undefined");
        assert_eq!(
            items[2].url, "https://store.steampowered.com/news/app/1",
            "url fallback do appid"
        );
    }

    #[test]
    fn invalid_date_and_malformed_json_are_errors() {
        let payload = json!({
            "appnews": { "newsitems": [{ "date": "nao-e-data" }] }
        });
        assert!(matches!(
            parse_game_news("1", &payload),
            Err(NewsError::InvalidDate(_))
        ));
        assert!(matches!(
            parse_game_news_text("1", "not json"),
            Err(NewsError::Json(_))
        ));
        assert!(matches!(
            parse_game_news("1", &json!({ "appnews": { "newsitems": [null] } })),
            Err(NewsError::InvalidItem)
        ));
    }

    #[test]
    fn iso_dates_match_javascript_for_epoch_and_pre_epoch() {
        assert_eq!(
            format_iso8601(0.0).as_deref(),
            Some("1970-01-01T00:00:00.000Z")
        );
        assert_eq!(
            format_iso8601(-1_000.0).as_deref(),
            Some("1969-12-31T23:59:59.000Z")
        );
        assert_eq!(
            format_iso8601(1_700_000_000_000.0).as_deref(),
            Some("2023-11-14T22:13:20.000Z")
        );
        assert_eq!(format_iso8601(f64::NAN), None);
        assert_eq!(format_iso8601(9e15), None);
    }

    #[derive(Default)]
    struct MockFetcher {
        calls: AtomicUsize,
        body: String,
        seen: SyncMutex<Vec<String>>,
    }

    impl MockFetcher {
        fn with_body(body: &str) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                body: body.to_string(),
                seen: SyncMutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl PageFetcher for MockFetcher {
        fn fetch_page<'a>(&'a self, url: &'a str) -> PageFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.seen.lock().unwrap().push(url.to_string());
                if self.body.is_empty() {
                    None
                } else {
                    Some(self.body.clone())
                }
            })
        }
    }

    #[tokio::test]
    async fn resolver_fetches_the_page_once_and_reuses_the_cache() {
        let fetcher = Arc::new(MockFetcher::with_body(
            "<link rel=\"image_src\" href=\"https://clan.akamai.steamstatic.com/images/7/post.png\">",
        ));
        let resolver = SteamNewsImageResolver::new(Arc::clone(&fetcher));
        let url =
            "https://steamstore-a.akamaihd.net/news/externalpost/steam_community_announcements/123";
        assert_eq!(
            resolver.resolve("", url).await,
            "https://clan.akamai.steamstatic.com/images/7/post.png"
        );
        assert_eq!(
            resolver.resolve("", url).await,
            "https://clan.akamai.steamstatic.com/images/7/post.png"
        );
        assert_eq!(fetcher.calls(), 1);
    }

    #[tokio::test]
    async fn resolver_uses_inline_image_without_fetching() {
        let fetcher = Arc::new(MockFetcher::with_body(
            "<meta property=\"og:image\" content=\"https://x.test/a.png\">",
        ));
        let resolver = SteamNewsImageResolver::new(Arc::clone(&fetcher));
        assert_eq!(
            resolver
                .resolve(
                    "[img src=\"https://clan.akamai.steamstatic.com/images/9/inline.png\"][/img]",
                    "https://store.steampowered.com/news/app/1"
                )
                .await,
            "https://clan.akamai.steamstatic.com/images/9/inline.png"
        );
        assert_eq!(fetcher.calls(), 0);
    }

    #[tokio::test]
    async fn resolver_never_fetches_external_urls() {
        let fetcher = Arc::new(MockFetcher::with_body("x"));
        let resolver = SteamNewsImageResolver::new(Arc::clone(&fetcher));
        assert_eq!(resolver.resolve("", "https://evil.example/post").await, "");
        assert_eq!(
            resolver
                .resolve("", "http://store.steampowered.com/a")
                .await,
            ""
        );
        assert_eq!(fetcher.calls(), 0);
    }

    #[tokio::test]
    async fn resolver_caches_the_empty_result_too() {
        let fetcher = Arc::new(MockFetcher::with_body(""));
        let resolver = SteamNewsImageResolver::new(Arc::clone(&fetcher));
        let url = "https://store.steampowered.com/news/app/1";
        assert_eq!(resolver.resolve("", url).await, "");
        assert_eq!(resolver.resolve("", url).await, "");
        assert_eq!(fetcher.calls(), 1);
    }

    struct SlowFetcher {
        calls: AtomicUsize,
        body: String,
    }

    impl PageFetcher for Arc<SlowFetcher> {
        fn fetch_page<'a>(&'a self, _url: &'a str) -> PageFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(25)).await;
                Some(self.body.clone())
            })
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_requests_share_one_fetch() {
        let fetcher = Arc::new(SlowFetcher {
            calls: AtomicUsize::new(0),
            body: "<meta property=\"og:image\" content=\"https://clan.akamai.steamstatic.com/images/1/p.png\">"
                .to_string(),
        });
        let resolver = Arc::new(SteamNewsImageResolver::new(Arc::clone(&fetcher)));
        let url = "https://store.steampowered.com/news/app/1";
        let first = {
            let resolver = Arc::clone(&resolver);
            tokio::spawn(async move { resolver.resolve("", url).await })
        };
        let second = {
            let resolver = Arc::clone(&resolver);
            tokio::spawn(async move { resolver.resolve("", url).await })
        };
        let (a, b) = tokio::join!(first, second);
        assert_eq!(
            a.unwrap(),
            "https://clan.akamai.steamstatic.com/images/1/p.png"
        );
        assert_eq!(
            b.unwrap(),
            "https://clan.akamai.steamstatic.com/images/1/p.png"
        );
        assert_eq!(fetcher.calls.load(Ordering::SeqCst), 1);
    }

    fn client_for(server: &MockServer) -> NewsClient {
        let config = arcadia_api::ApiConfig::resolve(Some(&server.uri()), None, None, None);
        let http = HttpClient::new(std::sync::Arc::new(config)).unwrap();
        NewsClient::with_endpoint(
            http,
            format!("{}/ISteamNews/GetNewsForApp/v2/", server.uri()),
        )
    }

    #[tokio::test]
    async fn http_contract_uses_the_same_endpoint_query_and_user_agent() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ISteamNews/GetNewsForApp/v2/"))
            .and(query_param("appid", "990080"))
            .and(query_param("count", "6"))
            .and(query_param("maxlength", "500"))
            .and(query_param("feeds", "steam_community_announcements"))
            .and(header("user-agent", "arcadia"))
            .respond_with(ResponseTemplate::new(200).set_body_string(NEWS_JSON))
            .expect(1)
            .mount(&server)
            .await;

        let items = client_for(&server)
            .fetch_game_news(" 990080 ")
            .await
            .unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].title, "Patch notes");
        assert_eq!(
            items[0].image,
            "https://clan.cloudflare.steamstatic.com/images/35190511/capa.png"
        );
        assert_eq!(items[0].summary, "Hello world");
    }

    #[tokio::test]
    async fn invalid_appid_does_not_touch_the_network() {
        let server = MockServer::start().await;
        let client = client_for(&server);
        assert!(client.fetch_game_news("abc").await.unwrap().is_empty());
        assert!(client
            .fetch_game_news("1234567890123")
            .await
            .unwrap()
            .is_empty());
        assert!(client.fetch_game_news("").await.unwrap().is_empty());
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn http_errors_and_bad_json_are_reported() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ISteamNews/GetNewsForApp/v2/"))
            .respond_with(ResponseTemplate::new(500))
            .expect(2)
            .mount(&server)
            .await;
        let client = client_for(&server);
        assert!(matches!(
            client.fetch_game_news("1").await,
            Err(NewsError::Http { status: 500 })
        ));
        assert!(client.game_news("1").await.is_empty());

        let bad = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ISteamNews/GetNewsForApp/v2/"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .expect(1)
            .mount(&bad)
            .await;
        assert!(matches!(
            client_for(&bad).fetch_game_news("1").await,
            Err(NewsError::Json(_))
        ));
    }
}
