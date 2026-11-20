//! Ponte entre o frontend iced e o dispatcher do `arcadia-ipc`.
//!
//! Toda chamada de IPC roda no runtime tokio dedicado do app, nunca na thread da
//! UI. O JSON devolvido por `library:get` é convertido aqui em [`Game`] — o view
//! model que as telas consomem. Capas são baixadas e decodificadas neste módulo
//! também, mas o cache fica em [`crate::covers`].

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use arcadia_ipc::{dispatch_async, AppState};
use iced::widget::image::Handle;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::shell::NativeShell;

/// Jogo já normalizado para a UI nativa.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    pub id: String,
    pub title: String,
    pub launcher: String,
    pub cover_url: Option<String>,
    pub hero_url: Option<String>,
    pub description: Option<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub genre: Option<String>,
    pub year: Option<String>,
    pub playtime_minutes: u64,
    pub installed: bool,
    pub hidden: bool,
    pub favorite: bool,
    pub categories: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawGame {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    launcher: Option<String>,
    #[serde(default)]
    cover: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    hero: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    developer: Option<String>,
    #[serde(default)]
    publisher: Option<String>,
    #[serde(default)]
    genre: Option<String>,
    #[serde(default)]
    year: Option<Value>,
    #[serde(default)]
    playtime_minutes: Option<i64>,
    #[serde(default)]
    installed: Option<bool>,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    favorite: bool,
    #[serde(default)]
    categories: Vec<String>,
}

impl Game {
    pub fn from_value(value: Value) -> Option<Self> {
        let raw: RawGame = serde_json::from_value(value).ok()?;
        let id = clean(raw.id)?;
        let title = clean(raw.title)?;

        let cover_url = clean(raw.cover)
            .or_else(|| clean(raw.icon))
            .or_else(|| steam_cover_url(&id));
        let hero_url = clean(raw.hero);

        Some(Self {
            launcher: clean(raw.launcher).unwrap_or_else(|| "custom".to_string()),
            cover_url,
            hero_url,
            description: clean(raw.description)
                .map(|text| strip_html(&text))
                .filter(|text| !text.is_empty()),
            developer: clean(raw.developer),
            publisher: clean(raw.publisher),
            genre: clean(raw.genre),
            year: raw.year.and_then(value_to_year),
            playtime_minutes: raw.playtime_minutes.unwrap_or(0).max(0) as u64,
            installed: raw.installed.unwrap_or(true),
            hidden: raw.hidden,
            favorite: raw.favorite,
            categories: raw
                .categories
                .into_iter()
                .map(|category| category.trim().to_string())
                .filter(|category| !category.is_empty())
                .collect(),
            id,
            title,
        })
    }

    /// URLs que a UI deve carregar para este jogo (sem duplicatas).
    pub fn image_urls(&self) -> Vec<String> {
        let mut urls = Vec::new();
        if let Some(cover) = self.cover_url.as_ref().filter(|url| !url.is_empty()) {
            urls.push(cover.clone());
        }
        if let Some(hero) = self.hero_url.as_ref().filter(|url| !url.is_empty()) {
            if Some(hero) != self.cover_url.as_ref() {
                urls.push(hero.clone());
            }
        }
        urls
    }

    pub fn playtime_label(&self) -> String {
        format_playtime(self.playtime_minutes)
    }

    /// Linha de metadados da página do jogo: ano · desenvolvedor · gênero.
    pub fn metadata_line(&self) -> String {
        let mut parts = Vec::new();
        if let Some(year) = self.year.as_ref().filter(|year| !year.is_empty()) {
            parts.push(year.clone());
        }
        if let Some(developer) = self
            .developer
            .as_ref()
            .or(self.publisher.as_ref())
            .filter(|name| !name.is_empty())
        {
            parts.push(developer.clone());
        }
        if let Some(genre) = self.genre.as_ref().filter(|genre| !genre.is_empty()) {
            parts.push(genre.clone());
        }

        if parts.is_empty() {
            "Metadados não disponíveis".to_string()
        } else {
            parts.join("  ·  ")
        }
    }
}

/// Converte o array bruto devolvido por `library:get`.
pub fn parse_library(value: Value) -> Result<Vec<Game>, String> {
    let Value::Array(items) = value else {
        return Err("library:get devolveu um payload que não é uma lista".to_string());
    };

    Ok(items
        .into_iter()
        .filter_map(Game::from_value)
        .collect::<Vec<_>>())
}

/// Trailer de `game:sysinfo` (`info.movies[]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Movie {
    pub name: String,
    pub thumb: Option<String>,
    pub hls: Option<String>,
    pub mp4: Option<String>,
    pub webm: Option<String>,
}

impl Movie {
    /// `true` quando há ao menos uma URI que o player consegue abrir.
    pub fn has_source(&self) -> bool {
        self.hls.is_some() || self.mp4.is_some() || self.webm.is_some()
    }
}

/// Dados de mídia que a página do jogo precisa para tocar o trailer.
///
/// `notice` é sempre explícito: falha de `game:sysinfo` (`ok:false`),
/// `info:null`, jogo sem trailer ou trailers sem fonte utilizável. A UI usa
/// isso no lugar de uma área vazia sem contexto.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameMedia {
    pub movies: Vec<Movie>,
    pub notice: Option<String>,
}

/// Interpreta o envelope `{ ok, info }` de `game:sysinfo`.
pub fn parse_game_media(value: Value) -> GameMedia {
    let Some(object) = value.as_object() else {
        return GameMedia {
            movies: Vec::new(),
            notice: Some("game:sysinfo devolveu um payload que não é um objeto.".to_string()),
        };
    };

    if object.get("ok").and_then(Value::as_bool) == Some(false) {
        let detail = object
            .get("error")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|error| !error.is_empty())
            .unwrap_or("sem detalhes");
        return GameMedia {
            movies: Vec::new(),
            notice: Some(format!(
                "Falha ao consultar o trailer (game:sysinfo: {detail})."
            )),
        };
    }

    let info = match object.get("info") {
        Some(Value::Object(info)) => info,
        _ => {
            return GameMedia {
                movies: Vec::new(),
                notice: Some(
                    "A loja não retornou informações deste jogo (info:null); sem trailer para exibir."
                        .to_string(),
                ),
            };
        }
    };

    let movies: Vec<Movie> = info
        .get("movies")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(movie_from_value).collect())
        .unwrap_or_default();

    if movies.is_empty() {
        let had_movies = info
            .get("movies")
            .and_then(Value::as_array)
            .is_some_and(|values| !values.is_empty());
        let notice = if had_movies {
            "A loja informou trailers, mas nenhum traz hls/mp4/webm reproduzível."
        } else {
            "Este jogo não tem trailer informado pela loja."
        };
        return GameMedia {
            movies: Vec::new(),
            notice: Some(notice.to_string()),
        };
    }

    GameMedia {
        movies,
        notice: None,
    }
}

fn movie_from_value(value: &Value) -> Option<Movie> {
    let movie = Movie {
        name: text_field(value, "name").unwrap_or_else(|| "Trailer".to_string()),
        thumb: text_field(value, "thumb").or_else(|| text_field(value, "thumbnail")),
        hls: stream_field(value, "hls").or_else(|| stream_field(value, "hls_h264")),
        mp4: stream_field(value, "mp4"),
        webm: stream_field(value, "webm"),
    };
    movie.has_source().then_some(movie)
}

fn text_field(value: &Value, key: &str) -> Option<String> {
    clean(value.get(key).and_then(Value::as_str).map(str::to_string))
}

/// Aceita string direta (`game:sysinfo` normalizado) ou objeto `{ max, 480 }`
/// do `appdetails` cru da Steam.
fn stream_field(value: &Value, key: &str) -> Option<String> {
    let raw = value.get(key)?;
    let url = match raw {
        Value::String(text) => text.as_str(),
        Value::Object(map) => map
            .get("max")
            .or_else(|| map.get("480"))
            .and_then(Value::as_str)?,
        _ => return None,
    };
    media_url(url)
}

fn media_url(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.replace("&amp;", "&");
    if let Some(rest) = normalized.strip_prefix("//") {
        return Some(format!("https://{rest}"));
    }
    let lower = normalized.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://")).then_some(normalized)
}

/// Remove tags HTML e normaliza quebras de linha, como o `stripHtml` da UI web.
pub fn strip_html(input: &str) -> String {
    let mut without_tags = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(start) = rest.find('<') {
        without_tags.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>') else {
            break;
        };
        let tag = rest[start + 1..start + end].trim().to_ascii_lowercase();
        if tag.starts_with("br") || tag.starts_with("/p") || tag.starts_with("/li") {
            without_tags.push('\n');
        }
        rest = &rest[start + end + 1..];
    }
    without_tags.push_str(rest);

    let decoded = decode_entities(&without_tags);
    let mut collapsed = String::with_capacity(decoded.len());
    let mut newlines = 0;
    for character in decoded.chars() {
        if character == '\n' {
            newlines += 1;
            if newlines <= 2 {
                collapsed.push(character);
            }
        } else {
            newlines = 0;
            collapsed.push(character);
        }
    }

    collapsed.trim().to_string()
}

/// `0` => "Nunca jogado"; abaixo de 1 h mostra minutos; acima, horas e minutos.
pub fn format_playtime(minutes: u64) -> String {
    match minutes {
        0 => "Nunca jogado".to_string(),
        minutes if minutes < 60 => format!("{minutes} min"),
        minutes => {
            let hours = minutes / 60;
            let rest = minutes % 60;
            if rest == 0 {
                format!("{hours} h")
            } else {
                format!("{hours}h {rest:02}min")
            }
        }
    }
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn value_to_year(value: Value) -> Option<String> {
    match value {
        Value::String(text) => clean(Some(text)),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn steam_appid(id: &str) -> Option<&str> {
    id.strip_prefix("steam:")
        .filter(|appid| !appid.is_empty() && appid.chars().all(|c| c.is_ascii_digit()))
}

fn steam_cover_url(id: &str) -> Option<String> {
    steam_appid(id).map(|appid| {
        format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/library_600x900.jpg")
    })
}

fn decode_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

fn decode_cover(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let image = image::load_from_memory(bytes).ok()?;
    // Limita o custo de memória: 1200 px já cobre card, hero e página do jogo.
    // Imagens menores não são ampliadas.
    let thumbnail = if image.width() > 1200 || image.height() > 1200 {
        image.thumbnail(1200, 1200)
    } else {
        image
    };
    let rgba = thumbnail.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some((width, height, rgba.into_raw()))
}

async fn fetch_cover_http(http: reqwest::Client, url: String) -> Option<Handle> {
    let response = http.get(url).send().await.ok()?.error_for_status().ok()?;
    let bytes = response.bytes().await.ok()?;
    decode_cover(&bytes).map(|(width, height, pixels)| Handle::from_rgba(width, height, pixels))
}

/// Fachada compartilhada pelas tasks do iced.
#[derive(Clone)]
pub struct Backend {
    state: Arc<AppState>,
    shell: Arc<NativeShell>,
    runtime: tokio::runtime::Handle,
    http: reqwest::Client,
}

impl Backend {
    pub fn new(state: AppState, runtime: tokio::runtime::Handle) -> Self {
        // O builder do reqwest cria recursos que dependem do reactor do tokio;
        // construa dentro do contexto do runtime mesmo estando na thread do main.
        let http = {
            let _guard = runtime.enter();
            reqwest::Client::builder()
                .user_agent(concat!("arcadia-native/", env!("CARGO_PKG_VERSION")))
                .timeout(Duration::from_secs(20))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new())
        };

        Self {
            state: Arc::new(state),
            shell: Arc::new(NativeShell::new()),
            runtime,
            http,
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.state.data_dir
    }

    pub async fn load_library(&self) -> Result<Vec<Game>, String> {
        let value = self.dispatch("library:get", Vec::new()).await?;
        parse_library(value)
    }

    /// Busca `info.movies[]` em `game:sysinfo` sem bloquear a UI.
    ///
    /// A chamada roda no runtime tokio do app. Falha de IPC vira `notice` na
    /// [`GameMedia`] — a página do jogo continua utilizável sem trailer.
    pub async fn load_game_media(&self, id: String, title: String) -> GameMedia {
        let payload = json!({ "id": id, "title": title });
        match self.dispatch("game:sysinfo", vec![payload]).await {
            Ok(value) => parse_game_media(value),
            Err(error) => GameMedia {
                movies: Vec::new(),
                notice: Some(format!("Falha ao consultar o trailer (game:sysinfo): {error}")),
            },
        }
    }

    /// Baixa, decodifica e reduz a imagem para um `Handle` pronto para o iced.
    /// Falha de rede/decodificação vira `None` — a view usa o placeholder.
    pub async fn fetch_cover(&self, url: String) -> Option<Handle> {
        let http = self.http.clone();
        let join = self
            .runtime
            .spawn(async move { fetch_cover_http(http, url).await });

        join.await.ok().flatten()
    }

    async fn dispatch(&self, channel: &str, args: Vec<Value>) -> Result<Value, String> {
        let state = Arc::clone(&self.state);
        let shell = Arc::clone(&self.shell);
        let channel = channel.to_string();

        let join = self.runtime.spawn(async move {
            dispatch_async(state.as_ref(), shell.as_ref(), &channel, args).await
        });

        match join.await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => Err(error.to_string()),
            Err(error) => Err(format!("tarefa IPC falhou: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_library_into_typed_games() {
        let value = json!([
            {
                "id": "steam:10",
                "title": "Half-Life",
                "launcher": "steam",
                "cover": "https://example.test/hl.jpg",
                "hero": "https://example.test/hl-hero.jpg",
                "description": "<p>Hello<br>world &amp; friends</p>",
                "developer": "Valve",
                "publisher": "Valve",
                "genre": "FPS",
                "year": 1998,
                "playtime_minutes": 90,
                "categories": ["Clássicos", " "]
            },
            {
                "id": "steam:20",
                "title": "Sem capa explícita",
                "installed": false
            },
            {
                "id": "custom:1",
                "title": "Jogo custom",
                "hidden": true
            }
        ]);

        let games = parse_library(value).expect("lista válida");
        assert_eq!(games.len(), 3);

        let half_life = &games[0];
        assert_eq!(half_life.id, "steam:10");
        assert_eq!(half_life.year.as_deref(), Some("1998"));
        assert_eq!(
            half_life.description.as_deref(),
            Some("Hello\nworld & friends")
        );
        assert_eq!(half_life.playtime_label(), "1h 30min");
        assert_eq!(half_life.metadata_line(), "1998  ·  Valve  ·  FPS");
        assert_eq!(half_life.categories, vec!["Clássicos"]);

        let sem_capa = &games[1];
        assert_eq!(
            sem_capa.cover_url.as_deref(),
            Some("https://cdn.cloudflare.steamstatic.com/steam/apps/20/library_600x900.jpg")
        );
        assert!(!sem_capa.installed);
        assert_eq!(sem_capa.playtime_label(), "Nunca jogado");

        assert!(games[2].hidden);
        assert!(games[2].cover_url.is_none());
    }

    #[test]
    fn rejects_payload_that_is_not_a_list() {
        assert!(parse_library(json!({ "games": [] })).is_err());
    }

    #[test]
    fn strip_html_matches_web_behavior() {
        assert_eq!(
            strip_html("<p>Linha 1</p><p>Linha 2<br/>fim &amp; mais</p>"),
            "Linha 1\nLinha 2\nfim & mais"
        );
        assert_eq!(strip_html("<div>sem fechamento"), "sem fechamento");
        assert_eq!(strip_html("   "), "");
    }

    #[test]
    fn formats_playtime_in_human_units() {
        assert_eq!(format_playtime(0), "Nunca jogado");
        assert_eq!(format_playtime(45), "45 min");
        assert_eq!(format_playtime(60), "1 h");
        assert_eq!(format_playtime(61), "1h 01min");
        assert_eq!(format_playtime(150), "2h 30min");
    }

    #[test]
    fn image_urls_are_unique_and_keep_cover_first() {
        let game = Game {
            id: "steam:1".to_string(),
            title: "Teste".to_string(),
            launcher: "steam".to_string(),
            cover_url: Some("https://example.test/cover.jpg".to_string()),
            hero_url: Some("https://example.test/cover.jpg".to_string()),
            description: None,
            developer: None,
            publisher: None,
            genre: None,
            year: None,
            playtime_minutes: 0,
            installed: true,
            hidden: false,
            favorite: false,
            categories: Vec::new(),
        };
        assert_eq!(
            game.image_urls(),
            vec!["https://example.test/cover.jpg".to_string()]
        );
    }

    #[test]
    fn decodes_png_bytes_to_rgba() {
        let image = image::RgbaImage::from_pixel(2, 3, image::Rgba([255, 0, 0, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("encode png");

        let (width, height, pixels) = decode_cover(&bytes).expect("decode png");
        assert_eq!((width, height), (2, 3));
        assert_eq!(pixels.len(), 2 * 3 * 4);
        assert_eq!(&pixels[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn parses_movies_skipping_entries_without_usable_source() {
        let value = json!({
            "ok": true,
            "info": {
                "movies": [
                    {
                        "name": "Trailer oficial",
                        "thumb": "https://cdn.test/thumb.jpg",
                        "hls_h264": "https://cdn.test/trailer.m3u8",
                        "mp4": { "max": "https://cdn.test/max.mp4", "480": "https://cdn.test/480.mp4" }
                    },
                    { "name": "Só pôster", "thumbnail": "//cdn.test/poster.jpg" },
                    {
                        "name": "WebM",
                        "webm": "//cdn.test/trailer.webm"
                    }
                ]
            }
        });

        let media = parse_game_media(value);
        assert_eq!(media.movies.len(), 2);
        assert!(media.notice.is_none());

        let first = &media.movies[0];
        assert_eq!(first.name, "Trailer oficial");
        assert_eq!(
            first.hls.as_deref(),
            Some("https://cdn.test/trailer.m3u8")
        );
        assert_eq!(first.mp4.as_deref(), Some("https://cdn.test/max.mp4"));
        assert_eq!(first.thumb.as_deref(), Some("https://cdn.test/thumb.jpg"));

        let second = &media.movies[1];
        assert_eq!(second.name, "WebM");
        assert_eq!(
            second.webm.as_deref(),
            Some("https://cdn.test/trailer.webm")
        );
        assert!(second.thumb.is_none());
    }

    #[test]
    fn game_media_reports_ok_false_info_null_and_missing_trailer() {
        let failed = parse_game_media(json!({ "ok": false, "error": "api_indisponivel" }));
        let failed_notice = failed.notice.expect("mensagem de falha");
        assert!(failed_notice.contains("api_indisponivel"), "{failed_notice}");
        assert!(failed.movies.is_empty());

        let empty_info = parse_game_media(json!({ "ok": true, "info": null }));
        let empty_notice = empty_info.notice.expect("mensagem de info:null");
        assert!(empty_notice.contains("info:null"), "{empty_notice}");

        let no_trailer = parse_game_media(json!({ "ok": true, "info": { "movies": [] } }));
        let no_trailer_notice = no_trailer.notice.expect("mensagem de sem trailer");
        assert!(
            no_trailer_notice.contains("não tem trailer"),
            "{no_trailer_notice}"
        );

        let unusable = parse_game_media(json!({
            "ok": true,
            "info": { "movies": [{ "name": "Vazio", "mp4": "", "hls": "ftp://cdn.test/a" }] }
        }));
        let unusable_notice = unusable.notice.expect("mensagem de fonte inválida");
        assert!(unusable_notice.contains("reproduzível"), "{unusable_notice}");
    }

    #[test]
    fn malformed_game_media_payload_is_reported() {
        let media = parse_game_media(json!(["não é objeto"]));
        assert!(media.movies.is_empty());
        assert!(media.notice.is_some());
    }
}
