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

// ── Downloads ───────────────────────────────────────────────────────────────

/// Origem do item: fila Epic/Steam (`dm:*`) ou subsistema torrent (`torrent:*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadKind {
    Epic,
    Steam,
    Torrent,
    Http,
    Debrid,
}

impl DownloadKind {
    pub fn label(self) -> &'static str {
        match self {
            DownloadKind::Epic => "Epic",
            DownloadKind::Steam => "Steam",
            DownloadKind::Torrent => "Torrent",
            DownloadKind::Http => "HTTP",
            DownloadKind::Debrid => "Debrid",
        }
    }
}

/// Estado unificado dos dois subsistemas, como em `downloads/normalize.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadStatus {
    Active,
    Queued,
    Paused,
    Caching,
    Done,
    Error,
    Canceled,
}

impl DownloadStatus {
    pub fn label(self) -> &'static str {
        match self {
            DownloadStatus::Active => "Baixando",
            DownloadStatus::Queued => "Na fila",
            DownloadStatus::Paused => "Pausado",
            DownloadStatus::Caching => "Cacheando",
            DownloadStatus::Done => "Concluído",
            DownloadStatus::Error => "Falhou",
            DownloadStatus::Canceled => "Cancelado",
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            DownloadStatus::Active => 0,
            DownloadStatus::Caching => 1,
            DownloadStatus::Queued => 2,
            DownloadStatus::Paused => 3,
            DownloadStatus::Error => 4,
            DownloadStatus::Done => 5,
            DownloadStatus::Canceled => 6,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadAction {
    Pause,
    Resume,
    Cancel,
    Retry,
    Dismiss,
}

impl DownloadAction {
    pub fn label(self) -> &'static str {
        match self {
            DownloadAction::Pause => "Pausar",
            DownloadAction::Resume => "Retomar",
            DownloadAction::Cancel => "Cancelar",
            DownloadAction::Retry => "Tentar novamente",
            DownloadAction::Dismiss => "Remover",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DownloadItem {
    pub id: String,
    pub title: String,
    pub kind: DownloadKind,
    pub status: DownloadStatus,
    pub percent: f32,
    pub detail: String,
    pub live: String,
    pub error: String,
    pub is_torrent: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DownloadsData {
    pub items: Vec<DownloadItem>,
    /// Falha parcial: uma fonte pode responder e a outra não.
    pub warnings: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDmItem {
    #[serde(default)]
    appid: String,
    #[serde(default)]
    app_name: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    percent: f64,
    #[serde(default)]
    done: f64,
    #[serde(default)]
    total: f64,
    #[serde(default)]
    eta: String,
    #[serde(default)]
    speed: f64,
    #[serde(default)]
    error: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTorrentItem {
    #[serde(default)]
    game_id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    folder_name: String,
    #[serde(default)]
    engine: String,
    #[serde(default)]
    pausado: bool,
    #[serde(default)]
    completo: bool,
    #[serde(default)]
    cacheando: bool,
    #[serde(default)]
    erro: String,
    #[serde(default)]
    progress: f64,
    #[serde(default)]
    download_speed: f64,
    #[serde(default)]
    num_peers: i64,
    #[serde(default)]
    num_seeds: i64,
    #[serde(default)]
    bytes_downloaded: f64,
    #[serde(default)]
    file_size: f64,
}

fn parse_dm_queue(value: Value) -> Result<Vec<DownloadItem>, String> {
    let Value::Array(items) = value else {
        return Err("dm:queue devolveu um payload que não é uma lista".to_string());
    };
    Ok(items
        .into_iter()
        .filter_map(|value| serde_json::from_value::<RawDmItem>(value).ok())
        .filter(|item| !item.appid.trim().is_empty())
        .map(|item| {
            let status = match item.status.as_str() {
                "downloading" => DownloadStatus::Active,
                "paused" => DownloadStatus::Paused,
                "done" => DownloadStatus::Done,
                "error" => DownloadStatus::Error,
                "canceled" => DownloadStatus::Canceled,
                _ => DownloadStatus::Queued,
            };
            let percent = clamp_percent(item.percent);
            let detail = if item.total > 0.0 {
                format!(
                    "{} / {} · {:.0}%",
                    format_mib(item.done),
                    format_mib(item.total),
                    percent
                )
            } else if status == DownloadStatus::Queued {
                "Aguardando slot".to_string()
            } else {
                format!("{percent:.0}%")
            };
            let mut live = Vec::new();
            if status == DownloadStatus::Active {
                if item.speed > 0.0 {
                    live.push(format!("{}/s", format_mib(item.speed)));
                }
                if !item.eta.trim().is_empty() {
                    live.push(format!("ETA {}", item.eta.trim()));
                }
            }
            DownloadItem {
                title: first_non_empty([&item.title, &item.app_name, &item.appid]),
                kind: if item.appid.to_ascii_lowercase().starts_with("steam:") {
                    DownloadKind::Steam
                } else {
                    DownloadKind::Epic
                },
                status,
                percent,
                detail,
                live: live.join(" · "),
                error: item.error.trim().to_string(),
                is_torrent: false,
                id: item.appid,
            }
        })
        .collect())
}

fn parse_torrent_list(value: Value) -> Result<Vec<DownloadItem>, String> {
    if let Some(false) = value.get("ok").and_then(Value::as_bool) {
        return Err(envelope_error(&value, "torrent:list"));
    }
    let Some(downloads) = value.get("downloads") else {
        return Err("torrent:list não devolveu downloads".to_string());
    };
    let Value::Array(items) = downloads else {
        return Err("torrent:list devolveu downloads que não são uma lista".to_string());
    };
    Ok(items
        .iter()
        .filter_map(|value| {
            let item: RawTorrentItem = serde_json::from_value(value.clone()).ok()?;
            (!item.game_id.trim().is_empty()).then_some(item)
        })
        .map(|item| {
            let status = if !item.erro.trim().is_empty() {
                DownloadStatus::Error
            } else if item.completo {
                DownloadStatus::Done
            } else if item.pausado {
                DownloadStatus::Paused
            } else if item.cacheando {
                DownloadStatus::Caching
            } else {
                DownloadStatus::Active
            };
            let kind = if item.engine == "http" {
                DownloadKind::Http
            } else if item.engine == "debrid" || item.cacheando {
                DownloadKind::Debrid
            } else {
                DownloadKind::Torrent
            };
            let percent = if item.completo {
                100.0
            } else {
                clamp_percent(item.progress * 100.0)
            };
            let bytes_done = if item.completo {
                item.file_size
            } else {
                item.bytes_downloaded
            };
            let detail = if status == DownloadStatus::Caching {
                "Cacheando no debrid".to_string()
            } else {
                format!(
                    "{} / {} · {:.0}%",
                    format_bytes(bytes_done),
                    format_bytes(item.file_size),
                    percent
                )
            };
            let mut live = Vec::new();
            if status == DownloadStatus::Active {
                if item.download_speed > 0.0 {
                    live.push(format!("{}/s", format_bytes(item.download_speed)));
                }
                if item.num_peers > 0 {
                    live.push(format!("{} peers", item.num_peers));
                }
                if item.num_seeds > 0 {
                    live.push(format!("{} seeds", item.num_seeds));
                }
            }
            DownloadItem {
                id: item.game_id.clone(),
                title: first_non_empty([&item.title, &item.folder_name, &item.game_id]),
                kind,
                status,
                percent,
                detail,
                live: live.join(" · "),
                error: item.erro.trim().to_string(),
                is_torrent: true,
            }
        })
        .collect())
}

pub fn parse_downloads(dm: Option<Value>, torrents: Option<Value>) -> DownloadsData {
    let mut items = Vec::new();
    let mut warnings = Vec::new();

    match dm {
        Some(value) => match parse_dm_queue(value) {
            Ok(mut parsed) => items.append(&mut parsed),
            Err(error) => warnings.push(error),
        },
        None => {}
    }
    match torrents {
        Some(value) => match parse_torrent_list(value) {
            Ok(mut parsed) => items.append(&mut parsed),
            Err(error) => warnings.push(error),
        },
        None => {}
    }
    items.sort_by_key(|item| item.status.rank());
    DownloadsData { items, warnings }
}

fn clamp_percent(value: f64) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 100.0) as f32
    } else {
        0.0
    }
}

fn compact_decimal(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    // Só remove zeros à direita quando há casa decimal: "100" é inteiro, não "1".
    let trimmed = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text.as_str()
    };
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn format_mib(mib: f64) -> String {
    if !mib.is_finite() || mib <= 0.0 {
        return "—".to_string();
    }
    if mib >= 1024.0 {
        format!("{} GiB", compact_decimal(mib / 1024.0, 1))
    } else {
        format!("{} MiB", compact_decimal(mib, 0))
    }
}

fn format_bytes(bytes: f64) -> String {
    if !bytes.is_finite() || bytes <= 0.0 {
        "—".to_string()
    } else {
        format_mib(bytes / (1024.0 * 1024.0))
    }
}

// ── Configurações ───────────────────────────────────────────────────────────

/// Retrato tipado de `config:get`, só com chaves que a UI pode mostrar.
/// Segredos ficam de fora por construção — o dispatcher já os mascara.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsConfig {
    pub language: String,
    pub theme_name: String,
    pub accent: String,
    pub default_install_path: String,
    pub default_wine_prefix_path: String,
    pub steam_path: String,
    pub check_updates_on_start: bool,
    pub start_in_console_mode: bool,
    pub hide_changelog_on_start: bool,
    pub minimize_on_game_launch: bool,
    pub disable_playtime_tracking: bool,
    pub discord_rich_presence: bool,
    pub discord_client_id: String,
    pub download_cpu_cores: u32,
    pub always_titles: bool,
    pub no_anim: bool,
    pub trailer_auto: bool,
    pub notifications_enabled: bool,
    pub music_enabled: bool,
}

impl Default for SettingsConfig {
    fn default() -> Self {
        Self {
            language: "pt-BR".to_string(),
            theme_name: "midnight".to_string(),
            accent: "#22d3ee".to_string(),
            default_install_path: String::new(),
            default_wine_prefix_path: String::new(),
            steam_path: String::new(),
            check_updates_on_start: true,
            start_in_console_mode: false,
            hide_changelog_on_start: false,
            minimize_on_game_launch: false,
            disable_playtime_tracking: false,
            discord_rich_presence: false,
            discord_client_id: String::new(),
            download_cpu_cores: 0,
            always_titles: true,
            no_anim: false,
            trailer_auto: false,
            notifications_enabled: true,
            music_enabled: false,
        }
    }
}

impl SettingsConfig {
    pub fn from_value(value: &Value) -> Self {
        let default = Self::default();
        let text = |key: &str, fallback: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| fallback.to_string())
        };
        let flag =
            |key: &str, fallback: bool| value.get(key).and_then(Value::as_bool).unwrap_or(fallback);
        let cores = value
            .get("download_cpu_cores")
            .and_then(Value::as_u64)
            .map(|cores| cores.min(64) as u32)
            .unwrap_or(default.download_cpu_cores);

        Self {
            language: text("language", &default.language),
            theme_name: text("theme_name", &default.theme_name),
            accent: text("accent", &default.accent),
            default_install_path: text("default_install_path", ""),
            default_wine_prefix_path: text("default_wine_prefix_path", ""),
            steam_path: text("steam_path", ""),
            check_updates_on_start: flag("check_updates_on_start", default.check_updates_on_start),
            start_in_console_mode: flag("start_in_console_mode", default.start_in_console_mode),
            hide_changelog_on_start: flag(
                "hide_changelog_on_start",
                default.hide_changelog_on_start,
            ),
            minimize_on_game_launch: flag(
                "minimize_on_game_launch",
                default.minimize_on_game_launch,
            ),
            disable_playtime_tracking: flag(
                "disable_playtime_tracking",
                default.disable_playtime_tracking,
            ),
            discord_rich_presence: flag("discord_rich_presence", default.discord_rich_presence),
            discord_client_id: text("discord_client_id", ""),
            download_cpu_cores: cores,
            always_titles: flag("always_titles", default.always_titles),
            no_anim: flag("no_anim", default.no_anim),
            trailer_auto: flag("trailer_auto", default.trailer_auto),
            notifications_enabled: flag("notifications_enabled", default.notifications_enabled),
            music_enabled: flag("music_enabled", default.music_enabled),
        }
    }

    pub fn language_label(&self) -> &'static str {
        match self.language.as_str() {
            "pt-BR" => "Português (Brasil)",
            "en-US" => "English (US)",
            "es-ES" => "Español",
            _ => "Idioma personalizado",
        }
    }
}

// ── Fontes ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceItem {
    pub id: String,
    pub name: String,
    pub url: String,
    pub count: Option<u64>,
}

impl SourceItem {
    pub fn status_label(&self) -> String {
        match self.count {
            Some(1) => "1 jogo".to_string(),
            Some(count) => format!("{count} jogos"),
            None => "Índice não carregado".to_string(),
        }
    }
}

pub fn parse_sources(value: Value) -> Result<Vec<SourceItem>, String> {
    if let Some(false) = value.get("ok").and_then(Value::as_bool) {
        return Err(envelope_error(&value, "sources:list"));
    }
    let Some(sources) = value.get("sources") else {
        return Err("sources:list não devolveu sources".to_string());
    };
    let Value::Array(items) = sources else {
        return Err("sources:list devolveu sources que não são uma lista".to_string());
    };
    Ok(items
        .iter()
        .filter_map(|item| {
            let id = text_field(item, "id");
            (!id.is_empty()).then(|| {
                let url = text_field(item, "url");
                SourceItem {
                    name: {
                        let name = text_field(item, "name");
                        if name.is_empty() {
                            if url.is_empty() {
                                id.clone()
                            } else {
                                url.clone()
                            }
                        } else {
                            name
                        }
                    },
                    url,
                    count: item.get("count").and_then(Value::as_u64),
                    id,
                }
            })
        })
        .collect())
}

// ── Amigos ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendItem {
    pub id: String,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub since: Option<String>,
}

impl FriendItem {
    pub fn display(&self) -> &str {
        self.display_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&self.username)
    }

    pub fn initial(&self) -> String {
        self.username
            .chars()
            .next()
            .map(|character| character.to_uppercase().to_string())
            .unwrap_or_else(|| "?".to_string())
    }

    pub fn since_label(&self) -> Option<String> {
        self.since
            .as_deref()
            .and_then(|value| value.get(..10))
            .and_then(|date| {
                let mut parts = date.split('-');
                let year = parts.next()?;
                let month = parts.next()?;
                let day = parts.next()?;
                Some(format!("{day}/{month}/{year}"))
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendsData {
    pub friends: Vec<FriendItem>,
    pub incoming: Vec<FriendItem>,
    pub outgoing: Vec<FriendItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FriendsLoad {
    NotLoggedIn,
    Ready(FriendsData),
}

pub fn parse_friends(value: Value) -> Result<FriendsLoad, String> {
    if let Some(false) = value.get("ok").and_then(Value::as_bool) {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("erro desconhecido")
            .to_string();
        if error == "nao_logado" {
            return Ok(FriendsLoad::NotLoggedIn);
        }
        return Err(format!("friends:list: {error}"));
    }
    let data = value
        .get("data")
        .ok_or_else(|| "friends:list não devolveu data".to_string())?;
    Ok(FriendsLoad::Ready(FriendsData {
        friends: parse_friend_list(data.get("friends")),
        incoming: parse_friend_list(data.get("incoming")),
        outgoing: parse_friend_list(data.get("outgoing")),
    }))
}

fn parse_friend_list(value: Option<&Value>) -> Vec<FriendItem> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = text_field(item, "id");
                    (!id.is_empty()).then(|| FriendItem {
                        username: {
                            let username = text_field(item, "username");
                            if username.is_empty() {
                                "?".to_string()
                            } else {
                                username
                            }
                        },
                        display_name: optional_text_field(item, "display_name"),
                        avatar_url: optional_text_field(item, "avatar_url"),
                        since: optional_text_field(item, "since"),
                        id,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

// ── Componentes ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub installed: bool,
    pub enabled: bool,
}

pub fn parse_plugins(value: Value) -> Result<Vec<PluginItem>, String> {
    if let Some(false) = value.get("ok").and_then(Value::as_bool) {
        return Err(envelope_error(&value, "plugins:list"));
    }
    let Some(plugins) = value.get("plugins") else {
        return Err("plugins:list não devolveu plugins".to_string());
    };
    let Value::Array(items) = plugins else {
        return Err("plugins:list devolveu plugins que não são uma lista".to_string());
    };
    Ok(items
        .iter()
        .filter_map(|item| {
            let id = text_field(item, "id");
            (!id.is_empty()).then(|| {
                let raw_name = text_field(item, "name");
                let desc_key = text_field(item, "descKey");
                PluginItem {
                    name: plugin_name(&id, &raw_name),
                    description: plugin_description(&id, &desc_key),
                    installed: item
                        .get("installed")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    enabled: item
                        .get("enabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    id,
                }
            })
        })
        .collect())
}

fn plugin_name(id: &str, fallback: &str) -> String {
    match id {
        "slssteam" => "Integração de biblioteca".to_string(),
        "luatools-fixes" => "Ajustes de compatibilidade".to_string(),
        _ if !fallback.trim().is_empty() => fallback.trim().to_string(),
        _ => id.to_string(),
    }
}

fn plugin_description(id: &str, desc_key: &str) -> String {
    match id {
        "slssteam" => {
            "Habilita integração direta com a biblioteca local e adicionar jogos a ela.".to_string()
        }
        "luatools-fixes" => {
            "Ativa recursos extras na página do jogo, se você já os tiver no sistema.".to_string()
        }
        _ if !desc_key.trim().is_empty() => desc_key.trim().to_string(),
        _ => "Componente detectado pelo Arcadia; nada é baixado pela rede.".to_string(),
    }
}

fn envelope_error(value: &Value, channel: &str) -> String {
    let code = value
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("erro desconhecido");
    format!("{channel}: {code}")
}

/// Envelopes de ação (`{ok:false,error}`) viram `Err`; `null` conta como ok.
fn check_envelope(value: &Value, channel: &str) -> Result<(), String> {
    match value.get("ok").and_then(Value::as_bool) {
        Some(false) => Err(envelope_error(value, channel)),
        _ => Ok(()),
    }
}

fn text_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn optional_text_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn first_non_empty<const N: usize>(values: [&String; N]) -> String {
    values
        .into_iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or("Sem título")
        .to_string()
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

    /// `dm:queue` + `torrent:list` em paralelo. Uma falha parcial não apaga a
    /// outra fonte: vira aviso na tela, com os itens que responderam.
    pub async fn load_downloads(&self) -> Result<DownloadsData, String> {
        let (dm, torrents) = tokio::join!(
            self.dispatch("dm:queue", Vec::new()),
            self.dispatch("torrent:list", Vec::new())
        );
        let (dm, dm_error) = match dm {
            Ok(value) => (Some(value), None),
            Err(error) => (None, Some(error)),
        };
        let (torrents, torrent_error) = match torrents {
            Ok(value) => (Some(value), None),
            Err(error) => (None, Some(error)),
        };
        if dm.is_none() && torrents.is_none() {
            return Err(format!(
                "{} · {}",
                dm_error.unwrap_or_else(|| "dm:queue indisponível".to_string()),
                torrent_error.unwrap_or_else(|| "torrent:list indisponível".to_string())
            ));
        }
        Ok(parse_downloads(dm, torrents))
    }

    pub async fn run_download_action(
        &self,
        id: String,
        is_torrent: bool,
        action: DownloadAction,
    ) -> Result<(), String> {
        let (channel, args) = if is_torrent {
            let channel = match action {
                DownloadAction::Pause => "torrent:pause",
                DownloadAction::Resume | DownloadAction::Retry => "torrent:resume",
                DownloadAction::Cancel | DownloadAction::Dismiss => "torrent:cancel",
            };
            (channel, vec![json!({ "gameId": id })])
        } else {
            let channel = match action {
                DownloadAction::Pause => "dm:pause",
                DownloadAction::Resume => "dm:resume",
                DownloadAction::Cancel => "dm:cancel",
                DownloadAction::Retry => "dm:retry",
                DownloadAction::Dismiss => "dm:dismiss",
            };
            (channel, vec![Value::String(id)])
        };

        let value = self.dispatch(channel, args).await?;
        check_envelope(&value, channel)
    }

    pub async fn load_settings(&self) -> Result<SettingsConfig, String> {
        let value = self.dispatch("config:get", Vec::new()).await?;
        if !value.is_object() {
            return Err("config:get devolveu um payload que não é um objeto".to_string());
        }
        Ok(SettingsConfig::from_value(&value))
    }

    /// Aplica um patch permitido e devolve o config redigido recarregado.
    pub async fn save_config(&self, patch: Value) -> Result<SettingsConfig, String> {
        let value = self.dispatch("config:set", vec![patch]).await?;
        check_envelope(&value, "config:set")?;
        let config = value
            .get("config")
            .ok_or_else(|| "config:set não devolveu a configuração".to_string())?;
        Ok(SettingsConfig::from_value(config))
    }

    pub async fn load_sources(&self) -> Result<Vec<SourceItem>, String> {
        let value = self.dispatch("sources:list", Vec::new()).await?;
        parse_sources(value)
    }

    pub async fn add_source(&self, url: String) -> Result<(), String> {
        let value = self
            .dispatch("sources:add", vec![Value::String(url)])
            .await?;
        check_envelope(&value, "sources:add")
    }

    pub async fn remove_source(&self, id: String) -> Result<(), String> {
        let value = self
            .dispatch("sources:remove", vec![Value::String(id)])
            .await?;
        check_envelope(&value, "sources:remove")
    }

    pub async fn sync_sources(&self) -> Result<(), String> {
        let value = self.dispatch("sources:sync", Vec::new()).await?;
        check_envelope(&value, "sources:sync")
    }

    pub async fn load_friends(&self, force: bool) -> Result<FriendsLoad, String> {
        let value = self
            .dispatch("friends:list", vec![json!({ "forcar": force })])
            .await?;
        parse_friends(value)
    }

    pub async fn load_plugins(&self) -> Result<Vec<PluginItem>, String> {
        let value = self.dispatch("plugins:list", Vec::new()).await?;
        parse_plugins(value)
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

    fn download<'a>(data: &'a DownloadsData, id: &str) -> &'a DownloadItem {
        data.items
            .iter()
            .find(|item| item.id == id)
            .unwrap_or_else(|| panic!("item {id} ausente"))
    }

    #[test]
    fn parses_dm_and_torrent_downloads_into_one_feed() {
        let dm = json!([
            {
                "appid": "epic:abc",
                "title": "Jogo Epic",
                "status": "downloading",
                "percent": 42.0,
                "done": 100.0,
                "total": 200.0,
                "speed": 3.5,
                "eta": "2 min"
            },
            {
                "appid": "steam:10",
                "appName": "Jogo Steam",
                "status": "queued"
            }
        ]);
        let torrents = json!({
            "ok": true,
            "downloads": [
                {
                    "gameId": "tor:abc",
                    "title": "Torrent",
                    "progress": 0.5,
                    "downloadSpeed": 1048576.0,
                    "bytesDownloaded": 1073741824.0,
                    "fileSize": 2147483648.0,
                    "numPeers": 3,
                    "numSeeds": 2
                },
                {
                    "gameId": "tor:done",
                    "title": "Completo",
                    "completo": true,
                    "progress": 1.0,
                    "fileSize": 2147483648.0
                },
                {
                    "gameId": "tor:err",
                    "title": "Com erro",
                    "erro": "sem seeds"
                }
            ]
        });

        let data = parse_downloads(Some(dm), Some(torrents));
        assert_eq!(data.items.len(), 5);
        assert!(data.warnings.is_empty());

        let epic = download(&data, "epic:abc");
        assert_eq!(epic.kind, DownloadKind::Epic);
        assert_eq!(epic.status, DownloadStatus::Active);
        assert!(!epic.is_torrent);
        assert_eq!(epic.percent, 42.0);
        assert_eq!(epic.detail, "100 MiB / 200 MiB · 42%");
        assert!(epic.live.contains("4 MiB/s"));
        assert!(epic.live.contains("ETA 2 min"));

        let steam = download(&data, "steam:10");
        assert_eq!(steam.kind, DownloadKind::Steam);
        assert_eq!(steam.status, DownloadStatus::Queued);
        assert_eq!(steam.title, "Jogo Steam");
        assert_eq!(steam.detail, "Aguardando slot");

        let torrent = download(&data, "tor:abc");
        assert_eq!(torrent.kind, DownloadKind::Torrent);
        assert_eq!(torrent.status, DownloadStatus::Active);
        assert!(torrent.is_torrent);
        assert_eq!(torrent.percent, 50.0);
        assert_eq!(torrent.detail, "1 GiB / 2 GiB · 50%");
        assert!(torrent.live.contains("1 MiB/s"));
        assert!(torrent.live.contains("3 peers"));

        assert_eq!(download(&data, "tor:done").status, DownloadStatus::Done);
        assert_eq!(download(&data, "tor:done").percent, 100.0);

        let failed = download(&data, "tor:err");
        assert_eq!(failed.status, DownloadStatus::Error);
        assert_eq!(failed.error, "sem seeds");

        // Ativos vêm antes de falhas e concluídos.
        assert_eq!(data.items[0].status, DownloadStatus::Active);
        assert_eq!(data.items[1].status, DownloadStatus::Active);
    }

    #[test]
    fn partial_download_failure_becomes_warning() {
        let data = parse_downloads(
            Some(json!([{"appid": "epic:ok", "title": "Ok"}])),
            Some(json!({ "ok": false, "error": "torrent_offline" })),
        );
        assert_eq!(data.items.len(), 1);
        assert_eq!(data.warnings.len(), 1);
        assert!(data.warnings[0].contains("torrent_offline"));
    }

    #[test]
    fn settings_ignore_secrets_and_clamp_cores() {
        let config = SettingsConfig::from_value(&json!({
            "language": "en-US",
            "accent": "#ff0000",
            "check_updates_on_start": false,
            "download_cpu_cores": 999,
            "steam_api_key": "SEGREDO-NAO-MOSTRAR",
            "torbox_token": "SEGREDO-NAO-MOSTRAR"
        }));

        assert_eq!(config.language, "en-US");
        assert_eq!(config.language_label(), "English (US)");
        assert_eq!(config.accent, "#ff0000");
        assert!(!config.check_updates_on_start);
        assert_eq!(config.download_cpu_cores, 64);
        let debug = format!("{config:?}");
        assert!(!debug.contains("SEGREDO-NAO-MOSTRAR"));
    }

    #[test]
    fn parses_sources_plugins_and_friends() {
        let sources = parse_sources(json!({
            "ok": true,
            "sources": [{
                "id": "src-1",
                "name": "",
                "url": "https://example.test/catalog.json",
                "count": 3
            }]
        }))
        .expect("fontes válidas");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "https://example.test/catalog.json");
        assert_eq!(sources[0].status_label(), "3 jogos");

        let plugins = parse_plugins(json!({
            "ok": true,
            "plugins": [{
                "id": "slssteam",
                "name": "slssteam",
                "descKey": "plugins.slssteam_desc",
                "installed": true,
                "enabled": false
            }]
        }))
        .expect("plugins válidos");
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "Integração de biblioteca");
        assert!(!plugins[0].description.starts_with("plugins."));
        assert!(plugins[0].installed);
        assert!(!plugins[0].enabled);

        let friends = parse_friends(json!({
            "ok": true,
            "data": {
                "friends": [{
                    "id": "u1",
                    "username": "ana",
                    "display_name": "Ana",
                    "avatar_url": null,
                    "since": "2026-01-02T10:00:00Z"
                }],
                "incoming": [],
                "outgoing": [{ "id": "u2", "username": "bia" }]
            }
        }))
        .expect("amigos válidos");
        let FriendsLoad::Ready(data) = friends else {
            panic!("esperava lista pronta");
        };
        assert_eq!(data.friends.len(), 1);
        assert_eq!(data.friends[0].display(), "Ana");
        assert_eq!(data.friends[0].since_label().as_deref(), Some("02/01/2026"));
        assert_eq!(data.outgoing.len(), 1);

        assert_eq!(
            parse_friends(json!({ "ok": false, "error": "nao_logado" })).unwrap(),
            FriendsLoad::NotLoggedIn
        );
    }
}
