//! Ponte entre o frontend iced e o dispatcher do `arcadia-ipc`.
//!
//! Toda chamada de IPC roda no runtime tokio dedicado do app, nunca na thread da
//! UI. O JSON devolvido por `library:get` é convertido aqui em [`Game`] — o view
//! model que as telas consomem. Capas são baixadas e decodificadas neste módulo
//! também, mas o cache fica em [`crate::covers`].

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use arcadia_ipc::{dispatch_async, AppState};
use base64::Engine as _;
use iced::widget::image::Handle;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::picker::PickPurpose;
use crate::shell::NativeShell;

/// Jogo já normalizado para a UI nativa.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    pub id: String,
    pub title: String,
    pub launcher: String,
    /// Comando que a UI web manda em `game:launch` (`cmd` do `useGameActions`).
    /// O resolver do IPC prioriza o `gameId`, mas o payload da renderer inclui
    /// o comando — mantemos o mesmo contrato para os modos futuros.
    pub launch_cmd: Vec<String>,
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
    launch_cmd: Vec<String>,
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
            launch_cmd: raw.launch_cmd,
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

    #[allow(dead_code)]
    pub fn playtime_label(&self) -> String {
        format_playtime(self.playtime_minutes)
    }

    /// Versão localizada de [`Game::playtime_label`] para as telas que já
    /// recebem o `I18n` do shell.
    pub fn playtime_label_i18n(&self, i18n: &crate::i18n::I18n) -> String {
        if self.playtime_minutes == 0 {
            return i18n.t("native.game.playtime.never").to_string();
        }
        let minutes = self.playtime_minutes;
        if minutes < 60 {
            return i18n.t_args(
                "native.game.playtime.minutes",
                &[("minutes", &minutes.to_string())],
            );
        }
        let hours = minutes / 60;
        let rest = minutes % 60;
        if rest == 0 {
            i18n.t_args(
                "native.game.playtime.hours",
                &[("hours", &hours.to_string())],
            )
        } else {
            i18n.t_args(
                "native.game.playtime.hours_minutes",
                &[
                    ("hours", &hours.to_string()),
                    ("minutes", &format!("{rest:02}")),
                ],
            )
        }
    }

    /// Appid Steam do jogo (`steam:<appid>` ou id numérico com launcher Steam).
    pub fn steam_appid(&self) -> Option<&str> {
        steam_appid(&self.id).or_else(|| {
            let id = self.id.trim();
            (self.launcher.eq_ignore_ascii_case("steam")
                && !id.is_empty()
                && id.chars().all(|character| character.is_ascii_digit()))
            .then_some(id)
        })
    }

    /// `true` para jogos do catálogo Retro (`retro:<sistema>:…`).
    pub fn is_retro(&self) -> bool {
        self.id.starts_with("retro:")
    }

    /// Sistema RetroAchievements embutido no id (`retro:ps2:SLUS…` → `ps2`).
    pub fn retro_system(&self) -> Option<&str> {
        self.id
            .strip_prefix("retro:")
            .and_then(|rest| rest.split(':').next())
            .map(str::trim)
            .filter(|system| !system.is_empty())
    }

    /// Linha de metadados da página do jogo: ano · desenvolvedor · gênero.
    pub fn metadata_line(&self) -> String {
        self.metadata_line_with_fallback("Metadados não disponíveis")
    }

    /// Versão localizada do fallback de [`Game::metadata_line`].
    pub fn metadata_line_i18n(&self, i18n: &crate::i18n::I18n) -> String {
        self.metadata_line_with_fallback(i18n.t("native.game.metadata_missing"))
    }

    fn metadata_line_with_fallback(&self, fallback: &str) -> String {
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
            fallback.to_string()
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

// ── Loja ────────────────────────────────────────────────────────────────────

/// Jogo normalizado de `store:search`, `store:suggest` e `store:recent`.
///
/// O dispatcher devolve `appid` como string, mas catálogos antigos podem
/// mandar número; o parser aceita os dois. `preco` só existe na busca direta
/// da Steam (`price`); quando ausente a UI simplesmente não mostra preço.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoreGame {
    pub appid: String,
    pub title: String,
    pub cover_url: Option<String>,
    pub hero_url: Option<String>,
    pub portrait_url: Option<String>,
    pub price: Option<String>,
    pub source: Option<String>,
    pub genres: Vec<String>,
    pub manifest: bool,
}

impl StoreGame {
    /// URLs que a UI deve carregar para este item (capa, hero e retrato).
    pub fn image_urls(&self) -> Vec<String> {
        let mut urls = Vec::new();
        for url in [
            self.cover_url.as_ref(),
            self.hero_url.as_ref(),
            self.portrait_url.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if !urls.contains(url) {
                urls.push(url.clone());
            }
        }
        urls
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreSearch {
    pub games: Vec<StoreGame>,
    pub source: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorePage {
    pub games: Vec<StoreGame>,
    pub total: Option<u64>,
    pub has_more: bool,
    pub next_offset: Option<u64>,
}

/// Metadados de `game:sysinfo` usados pela ficha da loja.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreInfo {
    pub description: Option<String>,
    pub header_url: Option<String>,
    pub background_url: Option<String>,
    pub developers: Vec<String>,
    pub publishers: Vec<String>,
    pub release_date: Option<String>,
}

impl StoreInfo {
    pub fn image_urls(&self) -> Vec<String> {
        let mut urls = Vec::new();
        for url in [self.background_url.as_ref(), self.header_url.as_ref()]
            .into_iter()
            .flatten()
        {
            if !urls.contains(url) {
                urls.push(url.clone());
            }
        }
        urls
    }
}

/// Depot de `store:installInfo`, no mesmo formato que o `DepotPicker` da web.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreDepot {
    pub depot_id: String,
    pub manifest_id: String,
    pub key: String,
    pub size: u64,
    pub name: Option<String>,
    pub os: Option<String>,
    pub language: Option<String>,
    pub dlc_appid: Option<String>,
    pub shared: bool,
    pub optional: bool,
}

/// Manifesto de instalação devolvido por `store:installInfo`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreInstallInfo {
    pub appid: Option<String>,
    pub depots: Vec<StoreDepot>,
    pub token: Option<String>,
    pub dlcs: Vec<String>,
    pub source: Option<String>,
    pub install_dir: Option<String>,
}

impl StoreInstallInfo {
    pub fn total_size(&self) -> u64 {
        self.depots.iter().map(|depot| depot.size).sum()
    }

    /// Depots que o `DepotPicker` marca por padrão: base do Windows + idioma
    /// inglês, sem DLCs, shared (runtimes) nem macOS. É a mesma regra do
    /// `padrao()` da UI web; nada aqui inicia download.
    pub fn recommended_depots(&self) -> BTreeSet<String> {
        self.depots
            .iter()
            .filter(|depot| {
                if depot.shared || depot.os.as_deref() == Some("macos") {
                    return false;
                }
                if depot.dlc_appid.is_some() {
                    return false;
                }
                match depot.language.as_deref() {
                    Some("english") => true,
                    Some(_) => false,
                    None => depot.os.as_deref().map_or(true, |os| os == "windows"),
                }
            })
            .map(|depot| depot.depot_id.clone())
            .collect()
    }

    /// Lista mostrada no detalhe: base, idiomas e DLCs; shared e macOS ficam
    /// de fora quando o manifesto tem metadados. Sem metadados, mostra tudo.
    pub fn relevant_depots(&self) -> Vec<&StoreDepot> {
        let no_metadata = self.depots.iter().all(|depot| {
            depot.os.is_none() && depot.language.is_none() && depot.dlc_appid.is_none()
        });
        self.depots
            .iter()
            .filter(|depot| {
                no_metadata
                    || (!depot.shared
                        && depot.os.as_deref() != Some("macos")
                        && (depot.dlc_appid.is_some()
                            || depot.language.is_some()
                            || depot.os.is_some()))
            })
            .collect()
    }
}

/// Biblioteca Steam devolvida por `store:libraries`. `path` aponta para
/// `.../steamapps`; `steam_dir` é a raiz usada pelo `store:install`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreLibrary {
    pub path: String,
    pub steam_dir: String,
    pub free: u64,
}

/// `store:search` — `{ ok, jogos, fonte, avisos, error? }`.
pub fn parse_store_search(value: Value) -> Result<StoreSearch, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "store:search devolveu um payload que não é um objeto".to_string())?;
    check_channel_ok(&value, "store:search")?;

    Ok(StoreSearch {
        games: parse_store_games(object.get("jogos")),
        source: optional_text_field(&value, "fonte"),
        warnings: string_list(object.get("avisos")),
    })
}

/// `store:suggest` — mesmo envelope da busca, sem fonte/avisos.
pub fn parse_store_suggest(value: Value) -> Result<Vec<StoreGame>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "store:suggest devolveu um payload que não é um objeto".to_string())?;
    check_channel_ok(&value, "store:suggest")?;
    Ok(parse_store_games(object.get("jogos")))
}

/// `store:recent` — `{ ok, jogos, total, has_more, next_offset, error? }`.
pub fn parse_store_recent(value: Value) -> Result<StorePage, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "store:recent devolveu um payload que não é um objeto".to_string())?;
    check_channel_ok(&value, "store:recent")?;

    let games = parse_store_games(object.get("jogos"));
    let total = object.get("total").and_then(Value::as_u64);
    let offset = object.get("offset").and_then(Value::as_u64).unwrap_or(0);
    let has_more = object
        .get("has_more")
        .or_else(|| object.get("hasMore"))
        .and_then(Value::as_bool)
        .unwrap_or_else(|| total.is_some_and(|total| offset + (games.len() as u64) < total));
    let next_offset = object
        .get("next_offset")
        .or_else(|| object.get("nextOffset"))
        .and_then(Value::as_u64)
        .or_else(|| has_more.then_some(offset + games.len() as u64));

    Ok(StorePage {
        games,
        total,
        has_more,
        next_offset,
    })
}

fn parse_store_games(value: Option<&Value>) -> Vec<StoreGame> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_store_game).collect())
        .unwrap_or_default()
}

fn parse_store_game(value: &Value) -> Option<StoreGame> {
    let appid = text_field_any(value, "appid").or_else(|| text_field_any(value, "id"))?;
    let title = text_field_any(value, "title")?;
    if appid.is_empty() || title.is_empty() {
        return None;
    }

    let cover_url = optional_image_url(value, "cover")
        .or_else(|| optional_image_url(value, "capa"))
        .or_else(|| steam_store_header(&appid));
    let hero_url = optional_image_url(value, "hero").or_else(|| optional_image_url(value, "heroi"));
    let portrait_url = optional_image_url(value, "capa");
    let source = optional_text_field(value, "fonte")
        .or_else(|| string_list(value.get("fontes")).into_iter().next());
    let mut genres = string_list(value.get("genres"));
    genres.extend(string_list(value.get("generos")));
    genres.dedup();

    Some(StoreGame {
        appid,
        title,
        cover_url,
        hero_url,
        portrait_url,
        price: optional_text_field(value, "preco").or_else(|| optional_text_field(value, "price")),
        source,
        genres,
        manifest: value
            .get("manifest")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// `game:sysinfo` — `{ ok, info }`; `info:null` e `ok:false` viram erro
/// explícito para a ficha mostrar por que não há descrição/backdrop.
pub fn parse_store_info(value: Value) -> Result<StoreInfo, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "game:sysinfo devolveu um payload que não é um objeto".to_string())?;
    check_channel_ok(&value, "game:sysinfo")?;

    let Some(Value::Object(info)) = object.get("info") else {
        return Err(
            "A loja não retornou informações deste jogo (info:null); a ficha segue só com os dados da busca."
                .to_string(),
        );
    };

    let description = optional_text_field_from(info.get("short_description"))
        .or_else(|| optional_text_field_from(info.get("about")))
        .map(|text| strip_html(&text))
        .filter(|text| !text.is_empty());

    Ok(StoreInfo {
        description,
        header_url: optional_image_url_from(info.get("header")),
        background_url: optional_image_url_from(info.get("background")),
        developers: string_list(info.get("developers")),
        publishers: string_list(info.get("publishers")),
        release_date: optional_text_field_from(info.get("release_date")),
    })
}

/// `store:installInfo` — `{ ok, appid, depots, token, dlcs, fonte, installdir }`.
pub fn parse_store_install_info(value: Value) -> Result<StoreInstallInfo, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "store:installInfo devolveu um payload que não é um objeto".to_string())?;
    check_channel_ok(&value, "store:installInfo")?;

    let depots = object
        .get("depots")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_store_depot).collect())
        .unwrap_or_default();

    Ok(StoreInstallInfo {
        appid: optional_text_field(&value, "appid"),
        depots,
        token: optional_text_field(&value, "token"),
        dlcs: string_list(object.get("dlcs")),
        source: optional_text_field(&value, "fonte"),
        install_dir: optional_text_field(&value, "installdir"),
    })
}

fn parse_store_depot(value: &Value) -> Option<StoreDepot> {
    let depot_id = text_field_any(value, "depotId")
        .or_else(|| text_field_any(value, "depot_id"))
        .or_else(|| text_field_any(value, "id"))?;
    if depot_id.is_empty() {
        return None;
    }

    Some(StoreDepot {
        depot_id,
        manifest_id: text_field_any(value, "manifestId")
            .or_else(|| text_field_any(value, "manifest_id"))
            .unwrap_or_default(),
        key: text_field(value, "key"),
        size: value.get("size").and_then(value_to_u64).unwrap_or_default(),
        name: optional_text_field(value, "name"),
        os: optional_text_field(value, "os").map(|os| os.to_ascii_lowercase()),
        language: optional_text_field(value, "language"),
        dlc_appid: optional_text_field(value, "dlcAppid")
            .or_else(|| optional_text_field(value, "dlc_appid")),
        shared: value
            .get("shared")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        optional: value
            .get("optional")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// `store:libraries` devolve um array cru de `{ path, steamDir, free }`.
pub fn parse_store_libraries(value: Value) -> Result<Vec<StoreLibrary>, String> {
    check_channel_ok(&value, "store:libraries")?;

    let items = match &value {
        Value::Array(items) => items,
        Value::Object(object) => object
            .get("libraries")
            .or_else(|| object.get("bibliotecas"))
            .and_then(Value::as_array)
            .ok_or_else(|| {
                "store:libraries devolveu um objeto sem a lista `libraries`".to_string()
            })?,
        _ => return Err("store:libraries devolveu um payload que não é uma lista".to_string()),
    };

    Ok(items.iter().filter_map(parse_store_library).collect())
}

fn parse_store_library(value: &Value) -> Option<StoreLibrary> {
    let steam_dir = text_field_any(value, "steamDir")
        .or_else(|| text_field_any(value, "steam_dir"))
        .or_else(|| text_field_any(value, "path"))?;
    let path = text_field_any(value, "path").unwrap_or_else(|| {
        Path::new(&steam_dir)
            .join("steamapps")
            .to_string_lossy()
            .into_owned()
    });

    Some(StoreLibrary {
        path,
        steam_dir,
        free: value.get("free").and_then(value_to_u64).unwrap_or_default(),
    })
}

/// Envelope de `store:install`: `ok:true`, ou `plugin`/`error` explicando a
/// recusa. O download em si é assíncrono e aparece em `dm:queue`.
pub fn parse_store_install_response(value: Value) -> Result<(), String> {
    if value.get("ok").and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }

    if let Some(plugin) = optional_text_field(&value, "plugin") {
        return Err(format!(
            "store:install requer o plugin {plugin} ativo no dispatcher."
        ));
    }

    match optional_text_field(&value, "error") {
        Some(error) => Err(format!("store:install: {error}")),
        None => Err("store:install falhou sem detalhes do dispatcher.".to_string()),
    }
}

/// Payload aceito por `store:install` (mesmos campos do `confirmarBaixar` da
/// web). `steam_dir` deve ser a raiz da biblioteca escolhida em
/// `store:libraries`; `depots` são os escolhidos na ficha.
pub fn store_install_payload(
    game: &StoreGame,
    info: &StoreInstallInfo,
    steam_dir: &str,
    depots: &[StoreDepot],
) -> Value {
    let appid = game
        .appid
        .trim()
        .strip_prefix("steam:")
        .unwrap_or(game.appid.trim());
    let installdir = info
        .install_dir
        .as_deref()
        .filter(|dir| !dir.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| store_install_dir(&game.title, appid));
    let depots: Vec<Value> = depots
        .iter()
        .map(|depot| {
            json!({
                "depotId": depot.depot_id,
                "manifestId": depot.manifest_id,
                "key": depot.key,
                "size": depot.size,
            })
        })
        .collect();

    json!({
        "appid": appid,
        "title": game.title,
        "cover": game.portrait_url.clone().or_else(|| game.cover_url.clone()).unwrap_or_default(),
        "installdir": installdir,
        "depots": depots,
        "token": info.token.clone().unwrap_or_default(),
        "dlcs": info.dlcs.clone(),
        "steamDir": steam_dir,
    })
}

/// `Half-Life: 2` → `HalfLife2`, como o `jogo.title.replace(/[^A-Za-z0-9]/g, "")`
/// de `useStoreActions`. Sem letras/números, cai no appid.
pub fn store_install_dir(title: &str, appid: &str) -> String {
    let cleaned: String = title
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect();
    if cleaned.is_empty() {
        appid.to_string()
    } else {
        cleaned
    }
}

fn value_to_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64().or_else(|| {
            number
                .as_f64()
                .filter(|number| *number >= 0.0)
                .map(|number| number as u64)
        }),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
}

// ── Catálogo Retro ──────────────────────────────────────────────────────────

/// Sistema do catálogo Retro. A lista abaixo é a mesma do seletor da
/// `RetroStoreView.tsx`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetroSystemInfo {
    pub id: &'static str,
    pub label: &'static str,
}

impl std::fmt::Display for RetroSystemInfo {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label)
    }
}

pub const RETRO_ALL_SYSTEMS: RetroSystemInfo = RetroSystemInfo {
    id: "",
    label: "Todos os sistemas",
};

pub const RETRO_SYSTEMS: [RetroSystemInfo; 14] = [
    RetroSystemInfo {
        id: "sony-playstation",
        label: "PlayStation",
    },
    RetroSystemInfo {
        id: "sony-playstation-2",
        label: "PlayStation 2",
    },
    RetroSystemInfo {
        id: "sony-playstation-3",
        label: "PlayStation 3",
    },
    RetroSystemInfo {
        id: "sony-psp",
        label: "PSP",
    },
    RetroSystemInfo {
        id: "nintendo-nes",
        label: "Nintendo NES",
    },
    RetroSystemInfo {
        id: "nintendo-snes",
        label: "Super Nintendo",
    },
    RetroSystemInfo {
        id: "nintendo-64",
        label: "Nintendo 64",
    },
    RetroSystemInfo {
        id: "nintendo-gamecube",
        label: "Nintendo GameCube",
    },
    RetroSystemInfo {
        id: "nintendo-wii",
        label: "Nintendo Wii",
    },
    RetroSystemInfo {
        id: "nintendo-ds",
        label: "Nintendo DS",
    },
    RetroSystemInfo {
        id: "nintendo-dsi",
        label: "Nintendo DSi",
    },
    RetroSystemInfo {
        id: "nintendo-game-boy",
        label: "Game Boy",
    },
    RetroSystemInfo {
        id: "nintendo-game-boy-color",
        label: "Game Boy Color",
    },
    RetroSystemInfo {
        id: "nintendo-game-boy-advance",
        label: "Game Boy Advance",
    },
];

/// Opções do `pick_list`: "Todos" primeiro, depois os 14 sistemas da web.
pub fn retro_system_filters() -> Vec<RetroSystemInfo> {
    let mut filters = Vec::with_capacity(RETRO_SYSTEMS.len() + 1);
    filters.push(RETRO_ALL_SYSTEMS);
    filters.extend(RETRO_SYSTEMS);
    filters
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RetroGame {
    pub id: String,
    pub title: String,
    pub source_title: Option<String>,
    pub system_id: Option<String>,
    pub platform: Option<String>,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    pub fallback_cover_url: Option<String>,
    pub hero_url: Option<String>,
    pub file_size: Option<String>,
    pub release_year: Option<u32>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub genres: Vec<String>,
    pub offer_count: Option<u64>,
}

impl RetroGame {
    pub fn image_urls(&self) -> Vec<String> {
        let mut urls = Vec::new();
        for url in [
            self.cover_url.as_ref(),
            self.fallback_cover_url.as_ref(),
            self.hero_url.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if !urls.contains(url) {
                urls.push(url.clone());
            }
        }
        urls
    }

    /// Rótulo humano do sistema; cai no id cru quando não é um dos conhecidos.
    #[allow(dead_code)]
    pub fn system_label(&self) -> String {
        self.system_label_with_fallback("Sistema não informado")
    }

    /// Versão localizada do rótulo do sistema.
    pub fn system_label_i18n(&self, i18n: &crate::i18n::I18n) -> String {
        self.system_label_with_fallback(i18n.t("native.retro.system_unknown"))
    }

    fn system_label_with_fallback(&self, fallback: &str) -> String {
        let raw = self
            .system_id
            .as_deref()
            .or(self.platform.as_deref())
            .unwrap_or_default();
        if let Some(system) = RETRO_SYSTEMS.iter().find(|system| system.id == raw) {
            return system.label.to_string();
        }
        if raw.is_empty() {
            fallback.to_string()
        } else {
            raw.to_string()
        }
    }

    /// Linha ano · desenvolvedor · gêneros para o card.
    pub fn meta_line(&self) -> String {
        let mut parts = Vec::new();
        if let Some(year) = self.release_year {
            parts.push(year.to_string());
        }
        if let Some(developer) = self.developer.as_deref().filter(|value| !value.is_empty()) {
            parts.push(developer.to_string());
        } else if let Some(publisher) = self.publisher.as_deref().filter(|value| !value.is_empty())
        {
            parts.push(publisher.to_string());
        }
        if !self.genres.is_empty() {
            parts.push(self.genres.join(", "));
        }
        parts.join("  ·  ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RetroSource {
    pub id: String,
    pub title: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RetroPage {
    pub games: Vec<RetroGame>,
    pub sources: Vec<RetroSource>,
    pub total: Option<u64>,
    pub total_offers: Option<u64>,
    pub offset: u64,
    pub has_more: bool,
}

/// `retro:list` — `{ ok, games, sources, total, totalOffers, hasMore }`.
pub fn parse_retro_page(value: Value) -> Result<RetroPage, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "retro:list devolveu um payload que não é um objeto".to_string())?;
    check_channel_ok(&value, "retro:list")?;

    let games: Vec<RetroGame> = object
        .get("games")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_retro_game).collect())
        .unwrap_or_default();
    let sources: Vec<RetroSource> = object
        .get("sources")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_retro_source).collect())
        .unwrap_or_default();
    let offset = object.get("offset").and_then(Value::as_u64).unwrap_or(0);
    let total = object
        .get("total")
        .or_else(|| object.get("totalGames"))
        .and_then(Value::as_u64);
    let total_offers = object.get("totalOffers").and_then(Value::as_u64);
    let has_more = object
        .get("hasMore")
        .or_else(|| object.get("has_more"))
        .and_then(Value::as_bool)
        .unwrap_or_else(|| total.is_some_and(|total| offset + (games.len() as u64) < total));

    Ok(RetroPage {
        games,
        sources,
        total,
        total_offers,
        offset,
        has_more,
    })
}

fn parse_retro_game(value: &Value) -> Option<RetroGame> {
    let id = text_field_any(value, "id")?;
    let title = text_field_any(value, "title")?;
    if id.is_empty() || title.is_empty() {
        return None;
    }

    let artwork = value.get("artwork");
    let art_url = |key: &str| {
        artwork
            .and_then(|artwork| artwork.get(key))
            .and_then(Value::as_str)
            .and_then(normalize_image_url)
    };
    let first_art_url = |key: &str| {
        artwork
            .and_then(|artwork| artwork.get(key))
            .and_then(Value::as_array)
            .and_then(|items| items.iter().find_map(Value::as_str))
            .and_then(normalize_image_url)
    };

    let cover_url = optional_image_url(value, "cover").or_else(|| art_url("cover"));
    let fallback_cover_url = optional_image_url(value, "fallbackCover")
        .or_else(|| art_url("titleScreen"))
        .or_else(|| first_art_url("titleScreens"))
        .or_else(|| art_url("cachedUrl"));
    let hero_url = optional_image_url(value, "hero").or_else(|| first_art_url("backgrounds"));

    let release_year = value.get("releaseYear").and_then(value_to_year_number);

    Some(RetroGame {
        id,
        title,
        source_title: optional_text_field(value, "sourceTitle"),
        system_id: optional_text_field(value, "systemId"),
        platform: optional_text_field(value, "platform"),
        description: optional_text_field_from(value.get("description"))
            .map(|text| strip_html(&text))
            .filter(|text| !text.is_empty()),
        cover_url,
        fallback_cover_url,
        hero_url,
        file_size: optional_text_field(value, "fileSize"),
        release_year,
        developer: first_string(value.get("developer")),
        publisher: first_string(value.get("publisher")),
        genres: string_list(value.get("genres")),
        offer_count: value.get("offerCount").and_then(Value::as_u64),
    })
}

fn parse_retro_source(value: &Value) -> Option<RetroSource> {
    let id = text_field_any(value, "id")?;
    if id.is_empty() {
        return None;
    }
    let title = text_field_any(value, "title").unwrap_or_else(|| id.clone());
    Some(RetroSource {
        id,
        title,
        url: optional_image_url(value, "url"),
    })
}

/// Oferta resumida de `retro:game` (sem URIs; `hasUris`/`uriCount` indicam se
/// `retro:offer` tem links).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetroOffer {
    pub id: String,
    pub source_id: Option<String>,
    pub source_title: Option<String>,
    pub original_title: Option<String>,
    pub file_size: Option<String>,
    pub upload_date: Option<String>,
    pub has_uris: bool,
    pub uri_count: u64,
    pub personal: bool,
}

/// Resposta de `retro:game`: jogo completo + ofertas + fontes (quando o
/// catálogo remoto as inclui).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetroGameDetail {
    pub game: RetroGame,
    pub offers: Vec<RetroOffer>,
    pub sources: Vec<RetroSource>,
}

impl RetroGameDetail {
    pub fn image_urls(&self) -> Vec<String> {
        self.game.image_urls()
    }
}

/// Resposta de `retro:offer` com as URIs (magnet/HTTP) da oferta.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetroOfferDetail {
    pub id: String,
    pub uris: Vec<String>,
}

/// `retro:game` — `{ ok, game, offers, sources? }`.
pub fn parse_retro_game_detail(value: Value) -> Result<RetroGameDetail, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "retro:game devolveu um payload que não é um objeto".to_string())?;
    check_channel_ok(&value, "retro:game")?;

    let game = object
        .get("game")
        .and_then(parse_retro_game)
        .ok_or_else(|| "retro:game devolveu um jogo sem id ou título".to_string())?;
    let offers = object
        .get("offers")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_retro_offer).collect())
        .unwrap_or_default();
    let sources = object
        .get("sources")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_retro_source).collect())
        .unwrap_or_default();

    Ok(RetroGameDetail {
        game,
        offers,
        sources,
    })
}

fn parse_retro_offer(value: &Value) -> Option<RetroOffer> {
    let id = text_field_any(value, "id")?;
    if id.is_empty() {
        return None;
    }

    Some(RetroOffer {
        id,
        source_id: optional_text_field(value, "sourceId")
            .or_else(|| optional_text_field(value, "source_id")),
        source_title: optional_text_field(value, "sourceTitle")
            .or_else(|| optional_text_field(value, "source_title")),
        original_title: optional_text_field(value, "originalTitle")
            .or_else(|| optional_text_field(value, "original_title")),
        file_size: optional_text_field(value, "fileSize")
            .or_else(|| optional_text_field(value, "file_size")),
        upload_date: optional_text_field(value, "uploadDate")
            .or_else(|| optional_text_field(value, "upload_date")),
        has_uris: value
            .get("hasUris")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        uri_count: value
            .get("uriCount")
            .and_then(value_to_u64)
            .unwrap_or_default(),
        personal: value
            .get("personal")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// `retro:offer` — `{ ok, offer: { id, uris: [...] } }`.
pub fn parse_retro_offer_detail(value: Value) -> Result<RetroOfferDetail, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "retro:offer devolveu um payload que não é um objeto".to_string())?;
    check_channel_ok(&value, "retro:offer")?;

    let offer = object
        .get("offer")
        .and_then(Value::as_object)
        .ok_or_else(|| "retro:offer devolveu um payload sem a oferta".to_string())?;
    let id = offer
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| "retro:offer devolveu uma oferta sem id".to_string())?
        .to_string();
    let uris = offer
        .get("uris")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|uri| !uri.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    Ok(RetroOfferDetail { id, uris })
}

#[allow(dead_code)]
fn retro_system_label(raw: &str) -> String {
    if let Some(system) = RETRO_SYSTEMS.iter().find(|system| system.id == raw) {
        return system.label.to_string();
    }
    if raw.is_empty() {
        "Sistema não informado".to_string()
    } else {
        raw.to_string()
    }
}

// ── Mídia da ficha do jogo ──────────────────────────────────────────────────

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

/// Captura de tela de `game:sysinfo` (`info.screenshots[]`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Screenshot {
    pub thumb: Option<String>,
    pub full: Option<String>,
}

impl Screenshot {
    /// Imagem grande; cai para a miniatura quando a loja não mandou a versão
    /// full.
    pub fn full_or_thumb(&self) -> Option<&str> {
        self.full.as_deref().or(self.thumb.as_deref())
    }

    /// Miniatura da faixa; cai para a imagem grande quando não há thumb.
    pub fn thumb_or_full(&self) -> Option<&str> {
        self.thumb.as_deref().or(self.full.as_deref())
    }
}

/// Dados de mídia que a página do jogo precisa para tocar o trailer e montar
/// a galeria de screenshots.
///
/// `notice` é sempre explícito: falha de `game:sysinfo` (`ok:false`),
/// `info:null`, jogo sem trailer ou trailers sem fonte utilizável. A UI usa
/// isso no lugar de uma área vazia sem contexto.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameMedia {
    pub movies: Vec<Movie>,
    pub screenshots: Vec<Screenshot>,
    /// HTML de `about`/`short_description` da loja. O renderizador nativo
    /// transforma isso em spans; nada de HTML é executado.
    pub about: Option<String>,
    pub notice: Option<String>,
}

/// Interpreta o envelope `{ ok, info }` de `game:sysinfo`.
pub fn parse_game_media(value: Value) -> GameMedia {
    let Some(object) = value.as_object() else {
        return GameMedia {
            notice: Some("game:sysinfo devolveu um payload que não é um objeto.".to_string()),
            ..GameMedia::default()
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
            notice: Some(format!(
                "Falha ao consultar a loja (game:sysinfo: {detail})."
            )),
            ..GameMedia::default()
        };
    }

    let info = match object.get("info") {
        Some(Value::Object(info)) => info,
        _ => {
            return GameMedia {
                notice: Some(
                    "A loja não retornou informações deste jogo (info:null); sem trailer ou capturas para exibir."
                        .to_string(),
                ),
                ..GameMedia::default()
            };
        }
    };

    let movies: Vec<Movie> = info
        .get("movies")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(movie_from_value).collect())
        .unwrap_or_default();
    let screenshots: Vec<Screenshot> = info
        .get("screenshots")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(screenshot_from_value).collect())
        .unwrap_or_default();
    let about = optional_map_text_field(info, "about")
        .or_else(|| optional_map_text_field(info, "short_description"));

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
            screenshots,
            about,
            notice: Some(notice.to_string()),
        };
    }

    GameMedia {
        movies,
        screenshots,
        about,
        notice: None,
    }
}

fn screenshot_from_value(value: &Value) -> Option<Screenshot> {
    let thumb = optional_text_field(value, "thumb")
        .or_else(|| optional_text_field(value, "path_thumbnail"))
        .and_then(|url| normalize_image_url(&url));
    let full = optional_text_field(value, "full")
        .or_else(|| optional_text_field(value, "path_full"))
        .and_then(|url| normalize_image_url(&url));

    match (thumb, full) {
        (None, None) => None,
        (thumb, full) => Some(Screenshot {
            thumb: thumb.clone().or_else(|| full.clone()),
            full: full.or(thumb),
        }),
    }
}

fn movie_from_value(value: &Value) -> Option<Movie> {
    let movie = Movie {
        name: optional_text_field(value, "name").unwrap_or_else(|| "Trailer".to_string()),
        thumb: optional_text_field(value, "thumb")
            .or_else(|| optional_text_field(value, "thumbnail")),
        hls: stream_field(value, "hls").or_else(|| stream_field(value, "hls_h264")),
        mp4: stream_field(value, "mp4"),
        webm: stream_field(value, "webm"),
    };
    movie.has_source().then_some(movie)
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

// ── Painéis da ficha: conquistas, ProtonDB e stats/reviews ─────────────────

/// Conquista normalizada de Steam (`achievements:get`) ou RetroAchievements
/// (`retroachievements:gameProgress`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Achievement {
    pub title: String,
    pub description: Option<String>,
    /// Ícone já escolhido: colorido quando desbloqueada, cinza quando não.
    pub icon: Option<String>,
    pub unlocked: bool,
    /// Percentual global de jogadores que desbloquearam (Steam).
    pub percent: Option<u32>,
    /// Pontos da conquista (RetroAchievements).
    pub points: Option<u32>,
}

/// Resultado do painel de conquistas. `note` explica ausência de dados
/// (jogo não encontrado no RA) e é exibido mesmo com a lista vazia.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AchievementsData {
    pub items: Vec<Achievement>,
    pub note: Option<String>,
}

/// Resumo público do ProtonDB para um appid.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProtonDbInfo {
    /// Tier em minúsculas (`platinum`, `gold`, …), como a API devolve.
    pub tier: Option<String>,
    /// Score já convertido para percentual (0–100).
    pub score_pct: Option<u32>,
    /// Compatibilidade com Steam Deck (`verified`, `playable`, …).
    pub deck: Option<String>,
    pub total: Option<u64>,
    pub url: Option<String>,
}

/// Avaliação resumida de `game:stats`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameReview {
    pub author: String,
    pub text: String,
    pub positive: bool,
    pub hours: u64,
    pub helpful: u64,
}

/// Owners/CCU e reviews resumidas de `game:stats`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameStats {
    pub owners: Option<String>,
    pub ccu: Option<u64>,
    pub review_desc: Option<String>,
    pub positive_pct: Option<u32>,
    pub total_reviews: Option<u64>,
    pub reviews: Vec<GameReview>,
}

impl GameStats {
    pub fn is_empty(&self) -> bool {
        self.owners.is_none()
            && self.ccu.is_none()
            && self.review_desc.is_none()
            && self.positive_pct.is_none()
            && self.total_reviews.is_none()
            && self.reviews.is_empty()
    }
}

/// `achievements:get` responde um array de itens (ou um objeto com
/// `achievements`); `ok:false` vira erro em vez de lista vazia.
pub fn parse_steam_achievements(value: Value) -> Result<AchievementsData, String> {
    let items = match &value {
        Value::Null => return Ok(AchievementsData::default()),
        Value::Array(items) => items,
        Value::Object(object) => {
            check_channel_ok(&value, "achievements:get")?;
            object
                .get("achievements")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        }
        _ => {
            return Err(
                "achievements:get devolveu um payload que não é uma lista nem um objeto."
                    .to_string(),
            )
        }
    };

    let items = items.iter().filter_map(steam_achievement).collect();
    Ok(AchievementsData { items, note: None })
}

fn steam_achievement(value: &Value) -> Option<Achievement> {
    let title = optional_text_field(value, "title")
        .or_else(|| optional_text_field(value, "apiname"))
        .map(|title| strip_html(&title))?;
    if title.is_empty() {
        return None;
    }

    let unlocked = value
        .get("achieved")
        .and_then(Value::as_bool)
        .or_else(|| value.get("unlocked").and_then(Value::as_bool))
        .unwrap_or_else(|| value_u64(value.get("unlock")).unwrap_or(0) > 0);
    let description = optional_text_field(value, "desc")
        .or_else(|| optional_text_field(value, "description"))
        .map(|description| strip_html(&description))
        .filter(|description| !description.is_empty());
    let icon = if unlocked {
        optional_text_field(value, "icon").or_else(|| optional_text_field(value, "icongray"))
    } else {
        optional_text_field(value, "icongray").or_else(|| optional_text_field(value, "icon"))
    };

    Some(Achievement {
        title,
        description,
        icon: icon.and_then(|icon| normalize_image_url(&icon)),
        unlocked,
        percent: value_f64(value.get("percent"))
            .map(|percent| percent.round().clamp(0.0, 100.0) as u32),
        points: None,
    })
}

/// `retroachievements:gameProgress` — `{ ok, game, achievements }`.
/// `game:null` não é erro: vira uma nota explícita para a UI.
pub fn parse_retro_achievements(value: Value) -> Result<AchievementsData, String> {
    let Some(object) = value.as_object() else {
        return Err(
            "retroachievements:gameProgress devolveu um payload que não é um objeto."
                .to_string(),
        );
    };

    if object.get("ok").and_then(Value::as_bool) == Some(false) {
        let code = optional_text_field(&value, "error")
            .unwrap_or_else(|| "erro desconhecido".to_string());
        return Err(retro_error_message(&code));
    }

    let game_missing = object.get("game").is_none_or(Value::is_null);
    let items: Vec<Achievement> = object
        .get("achievements")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(retro_achievement).collect())
        .unwrap_or_default();

    let note = if game_missing {
        Some("O RetroAchievements não encontrou este jogo para a conta conectada.".to_string())
    } else if items.is_empty() {
        Some("O RetroAchievements não retornou conquistas para este jogo.".to_string())
    } else {
        None
    };

    Ok(AchievementsData { items, note })
}

fn retro_achievement(value: &Value) -> Option<Achievement> {
    let title = optional_text_field(value, "title").map(|title| strip_html(&title))?;
    if title.is_empty() {
        return None;
    }

    let unlocked = value
        .get("unlocked")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let icon = if unlocked {
        optional_text_field(value, "badgeUrl")
            .or_else(|| optional_text_field(value, "badgeLockedUrl"))
    } else {
        optional_text_field(value, "badgeLockedUrl")
            .or_else(|| optional_text_field(value, "badgeUrl"))
    };

    Some(Achievement {
        title,
        description: optional_text_field(value, "description")
            .map(|description| strip_html(&description))
            .filter(|description| !description.is_empty()),
        icon: icon.and_then(|icon| normalize_image_url(&icon)),
        unlocked,
        percent: None,
        points: value_u64(value.get("points"))
            .map(|points| points.min(u64::from(u32::MAX)) as u32)
            .filter(|points| *points > 0),
    })
}

fn retro_error_message(code: &str) -> String {
    match code {
        "nao_logado" | "sem_web_api_key" | "chave_invalida" | "credenciais_invalidas"
        | "credenciais_vazias" => {
            "RetroAchievements: conecte a conta nas Configurações para ver as conquistas."
                .to_string()
        }
        "tempo_esgotado" | "falha_rede" => {
            "RetroAchievements: a consulta falhou por rede/tempo. Tente novamente.".to_string()
        }
        "sistema_nao_suportado" => {
            "RetroAchievements não reconhece o sistema deste jogo.".to_string()
        }
        other => format!("retroachievements:gameProgress: {other}"),
    }
}

/// `game:protondb` — `{ ok, info }`; `info:null` vira `Ok(None)`.
pub fn parse_protondb(value: Value, appid: &str) -> Result<Option<ProtonDbInfo>, String> {
    let Some(object) = value.as_object() else {
        return Err("game:protondb devolveu um payload que não é um objeto.".to_string());
    };
    check_channel_ok(&value, "game:protondb")?;

    let fallback_url = {
        let appid = appid.trim().trim_start_matches("steam:");
        (!appid.is_empty() && appid.chars().all(|character| character.is_ascii_digit()))
            .then(|| format!("https://www.protondb.com/app/{appid}"))
    };

    let Some(info) = object.get("info").and_then(Value::as_object) else {
        return Ok(None);
    };

    let tier = optional_map_text_field(info, "tier")
        .map(|tier| tier.to_ascii_lowercase())
        .filter(|tier| !tier.is_empty());
    let score_pct = value_f64(info.get("score"))
        .filter(|score| score.is_finite() && *score >= 0.0)
        .map(|score| {
            let percent = if score <= 1.0 { score * 100.0 } else { score };
            percent.round().clamp(0.0, 100.0) as u32
        });
    let deck = optional_map_text_field(info, "deckCompatibility")
        .map(|deck| deck.to_ascii_lowercase())
        .filter(|deck| !deck.is_empty());
    let total = value_u64(info.get("total")).filter(|total| *total > 0);
    let url = optional_map_text_field(info, "url").or(fallback_url);

    if tier.is_none() && score_pct.is_none() && total.is_none() {
        return Ok(None);
    }

    Ok(Some(ProtonDbInfo {
        tier,
        score_pct,
        deck,
        total,
        url,
    }))
}

/// `game:stats` — `{ ok, info }` com owners/CCU e reviews resumidas.
pub fn parse_game_stats(value: Value) -> Result<Option<GameStats>, String> {
    let Some(object) = value.as_object() else {
        return Err("game:stats devolveu um payload que não é um objeto.".to_string());
    };
    check_channel_ok(&value, "game:stats")?;

    let Some(info) = object.get("info").and_then(Value::as_object) else {
        return Ok(None);
    };

    let owners = match info.get("owners") {
        Some(Value::String(owners)) => {
            let owners = owners.trim();
            (!owners.is_empty()).then(|| owners.to_string())
        }
        Some(Value::Number(owners)) => Some(owners.to_string()),
        _ => None,
    };
    let stats = GameStats {
        owners,
        ccu: value_u64(info.get("ccu")),
        review_desc: optional_map_text_field(info, "reviewDesc"),
        positive_pct: value_f64(info.get("reviewPositivePct"))
            .map(|percent| percent.round().clamp(0.0, 100.0) as u32),
        total_reviews: value_u64(info.get("totalReviews")),
        reviews: info
            .get("comments")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(game_review).collect())
            .unwrap_or_default(),
    };

    if stats.is_empty() {
        Ok(None)
    } else {
        Ok(Some(stats))
    }
}

fn game_review(value: &Value) -> Option<GameReview> {
    let text = optional_text_field(value, "text")?;
    let author = optional_text_field(value, "author").unwrap_or_else(|| "Steam".to_string());
    Some(GameReview {
        author,
        text,
        positive: value
            .get("positive")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        hours: value_u64(value.get("hours")).unwrap_or(0),
        helpful: value_u64(value.get("helpful")).unwrap_or(0),
    })
}

fn value_u64(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::Number(number) => number.as_u64().or_else(|| {
            number
                .as_f64()
                .filter(|number| number.is_finite() && *number >= 0.0)
                .map(|number| number.round() as u64)
        }),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
}

fn value_f64(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

// ── Seletor de trailers do YouTube ─────────────────────────────────────────

/// Resultado de `trailer:search` — um vídeo do YouTube para escolher como
/// trailer do jogo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrailerResult {
    pub id: String,
    pub title: String,
    pub thumbnail: Option<String>,
    /// Duração em segundos; `0` quando o yt-dlp não informou.
    pub duration: u64,
    pub channel: String,
    pub url: String,
}

/// Interpreta o envelope `{ ok, results }` de `trailer:search`.
///
/// `ok:false` (yt-dlp ausente, falha de rede, vídeo bloqueado) vira `Err` com a
/// mensagem do dispatcher. Assim a tela nunca troca uma falha real por "nenhum
/// vídeo encontrado".
pub fn parse_trailer_search(value: Value) -> Result<Vec<TrailerResult>, String> {
    let Some(object) = value.as_object() else {
        return Err("trailer:search devolveu um payload que não é um objeto.".to_string());
    };

    if object.get("ok").and_then(Value::as_bool) == Some(false) {
        return Err(trailer_error_message(&value, "trailer:search"));
    }

    let Some(items) = object.get("results").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };

    Ok(items.iter().filter_map(trailer_result_from_value).collect())
}

fn trailer_result_from_value(value: &Value) -> Option<TrailerResult> {
    let id = optional_text_field(value, "id").unwrap_or_default();
    let url = optional_text_field(value, "url").or_else(|| {
        (!id.is_empty()).then(|| format!("https://www.youtube.com/watch?v={id}"))
    })?;

    Some(TrailerResult {
        id: if id.is_empty() { url.clone() } else { id },
        title: optional_text_field(value, "title")
            .unwrap_or_else(|| "Vídeo do YouTube".to_string()),
        thumbnail: optional_text_field(value, "thumbnail"),
        duration: trailer_duration(value.get("duration")),
        channel: optional_text_field(value, "channel").unwrap_or_default(),
        url,
    })
}

fn trailer_duration(value: Option<&Value>) -> u64 {
    let seconds = match value {
        Some(Value::Number(number)) => number.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        _ => None,
    };

    seconds
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .map(|seconds| seconds.round() as u64)
        .unwrap_or(0)
}

/// Caminho de `trailer:path`; string vazia (trailer ausente) vira `None`.
pub fn parse_trailer_path(value: &Value) -> Option<String> {
    optional_text_field(value, "path")
}

/// Interpreta `trailer:downloadUrl` e devolve o `file://` do arquivo baixado.
pub fn parse_trailer_download(value: Value) -> Result<String, String> {
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        return Err(trailer_error_message(&value, "trailer:downloadUrl"));
    }

    optional_text_field(&value, "path").ok_or_else(|| {
        "trailer:downloadUrl não devolveu o caminho do trailer baixado.".to_string()
    })
}

/// Mensagem legível para os envelopes de falha de `trailer:*`.
///
/// O dispatcher manda `error` (código curto ou stderr do yt-dlp) e, quando
/// existe, `mensagem` com a explicação completa. `age` tem texto próprio para
/// o usuário saber que o caminho é configurar cookies, não tentar de novo.
fn trailer_error_message(value: &Value, channel: &str) -> String {
    let code = optional_text_field(value, "error");
    if code.as_deref() == Some("age") {
        return "Vídeo com restrição de idade: configure os cookies do YouTube nas Configurações e tente de novo."
            .to_string();
    }

    optional_text_field(value, "mensagem")
        .or(code)
        .map(|message| format!("{channel}: {message}"))
        .unwrap_or_else(|| format!("{channel}: erro desconhecido"))
}

// ── Notícias ────────────────────────────────────────────────────────────────

/// Item devolvido por `news:get` (feed RSS agregado pelo IPC).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewsItem {
    pub id: String,
    pub title: String,
    pub source: String,
    pub date: String,
    pub url: String,
    pub summary: String,
    pub image: Option<String>,
}

impl NewsItem {
    /// Data legível; feeds sem data válida mostram um rótulo explícito ou o
    /// texto bruto quando ele não é RFC 3339.
    #[allow(dead_code)]
    pub fn date_label(&self) -> String {
        self.date_label_with_empty("Data não informada")
    }

    /// Versão localizada do rótulo de data do feed.
    pub fn date_label_i18n(&self, i18n: &crate::i18n::I18n) -> String {
        self.date_label_with_empty(i18n.t("native.console.date_missing"))
    }

    fn date_label_with_empty(&self, empty: &str) -> String {
        let raw = self.date.trim();
        if raw.is_empty() {
            return empty.to_string();
        }
        chrono::DateTime::parse_from_rfc3339(raw)
            .map(|date| {
                date.with_timezone(&chrono::Local)
                    .format("%d/%m/%Y %H:%M")
                    .to_string()
            })
            .unwrap_or_else(|_| raw.to_string())
    }
}

/// `news:get` devolve o array já agregado e ordenado; itens sem título/URL
/// são descartados em vez de virar linhas vazias na aba.
pub fn parse_news(value: Value) -> Result<Vec<NewsItem>, String> {
    let Value::Array(items) = value else {
        return Err("news:get devolveu um payload que não é uma lista".to_string());
    };

    Ok(items
        .into_iter()
        .filter_map(|value| {
            let title = text_field_any(&value, "title")?;
            let url = text_field_any(&value, "url")?;
            if title.is_empty() || url.is_empty() {
                return None;
            }
            Some(NewsItem {
                id: text_field_any(&value, "id").unwrap_or_else(|| url.clone()),
                title,
                source: text_field_any(&value, "source")
                    .unwrap_or_else(|| "Feed".to_string()),
                date: text_field_any(&value, "date").unwrap_or_default(),
                url,
                summary: text_field_any(&value, "summary").unwrap_or_default(),
                image: optional_image_url(&value, "image"),
            })
        })
        .collect())
}

// ── Lançamento de jogos ─────────────────────────────────────────────────────

/// Fase reportada por `game:launchState` (`LaunchRuntime::spawn_plan` e o
/// monitor de saída do IPC).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchPhase {
    Starting,
    Running,
    Stopping,
    Idle,
}

/// Evento de lançamento já validado; a UI não olha mais o runtime em polling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchEvent {
    /// `game:running` — booleano sem `gameId`.
    Running(bool),
    /// `game:active` — `{ rodando, gameId }`.
    Active {
        running: bool,
        game_id: Option<String>,
    },
    /// `game:launchState` — `{ state, gameId, token }`.
    Phase {
        phase: LaunchPhase,
        game_id: Option<String>,
    },
    /// `game:launchWarning` — avisos do planejador (wrapper ausente etc.).
    Warning {
        game_id: Option<String>,
        warnings: Vec<String>,
    },
    /// `game:launchError` — falha de spawn antes de existir processo.
    Error {
        game_id: Option<String>,
        error: String,
    },
}

/// Converte um evento cru do `NativeShell`/`LaunchRuntime` no contrato da UI.
///
/// Payloads desconhecidos devolvem `None`: um evento novo do backend não pode
/// derrubar a tela do jogo nem inventar estado.
pub fn parse_launch_event(event: &str, payload: &Value) -> Option<LaunchEvent> {
    match event {
        "game:running" => payload.as_bool().map(LaunchEvent::Running),
        "game:active" => {
            let running = payload.get("rodando").and_then(Value::as_bool)?;
            Some(LaunchEvent::Active {
                running,
                game_id: optional_text_field(payload, "gameId"),
            })
        }
        "game:launchState" => {
            let phase = match payload.get("state").and_then(Value::as_str)? {
                "starting" => LaunchPhase::Starting,
                "running" => LaunchPhase::Running,
                "stopping" => LaunchPhase::Stopping,
                "idle" => LaunchPhase::Idle,
                _ => return None,
            };
            Some(LaunchEvent::Phase {
                phase,
                game_id: optional_text_field(payload, "gameId"),
            })
        }
        "game:launchWarning" => {
            let warnings = payload
                .get("warnings")
                .and_then(Value::as_array)?
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|warning| !warning.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            if warnings.is_empty() {
                return None;
            }
            Some(LaunchEvent::Warning {
                game_id: optional_text_field(payload, "gameId"),
                warnings,
            })
        }
        "game:launchError" => {
            let error = payload
                .get("error")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|error| !error.is_empty())
                .unwrap_or("falha desconhecida no lançamento")
                .to_string();
            Some(LaunchEvent::Error {
                game_id: optional_text_field(payload, "gameId"),
                error,
            })
        }
        _ => None,
    }
}

/// Payload de `game:launch`, igual ao do `useGameActions.ts`:
/// `{ cmd, gameId, mode? }`. As opções de Wine/Proton não vão no payload: o
/// dispatcher lê `game_settings.json` via `gamesettings:get`/`RuntimeDeps`.
pub fn launch_payload(game: &Game, mode: Option<&str>) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert("cmd".to_string(), json!(game.launch_cmd));
    payload.insert("gameId".to_string(), json!(game.id));
    if let Some(mode) = mode {
        payload.insert("mode".to_string(), json!(mode));
    }
    Value::Object(payload)
}

/// `{ok:false,error}` do `game:launch` vira um erro legível para a UI.
fn check_game_launch(value: &Value) -> Result<(), String> {
    check_envelope(value, "game:launch")
}

// ── Configurações de jogo (game_settings.json) ──────────────────────────────

/// Variável de ambiente extra no shape do resolver (`EnvVar`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnvVarSetting {
    pub name: String,
    pub value: String,
}

/// Subconjunto editável de `GameSettings` no diálogo nativo.
///
/// Só entram campos que o `plan_launch` do `arcadia-launch` consome de fato no
/// caminho nativo. `wineVersion`, `autoDXVK`, `autoVKD3D` e os scripts são
/// apenas lidos para exibir o valor salvo: o dispatcher atual não os aplica a
/// jogos locais (ver `views::game`), então o diálogo os mostra desabilitados.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSettingsView {
    pub wine_version: Option<String>,
    pub prefix_path: String,
    pub exe_path: String,
    pub game_args: String,
    pub dxvk_hud: String,
    pub env_vars: Vec<EnvVarSetting>,
    pub esync: bool,
    pub fsync: bool,
    pub wine_wayland: bool,
    pub wow64: bool,
    pub fsr_hack: bool,
    pub auto_nvapi: bool,
    pub auto_dxvk: bool,
    pub auto_vkd3d: bool,
    pub mangohud: bool,
    pub gamemode: bool,
    pub gamescope: bool,
    pub gs_hdr: bool,
    pub verbose_logs: bool,
    pub script_pre: String,
    pub script_post: String,
    pub wrapper_count: usize,
    pub emulator_id: Option<String>,
    pub rom_path: String,
    pub emulator_args: Vec<String>,
    pub emulator_core_path: String,
}

impl Default for GameSettingsView {
    fn default() -> Self {
        Self {
            wine_version: None,
            prefix_path: String::new(),
            exe_path: String::new(),
            game_args: String::new(),
            dxvk_hud: String::new(),
            env_vars: Vec::new(),
            esync: false,
            fsync: false,
            wine_wayland: false,
            wow64: false,
            fsr_hack: false,
            auto_nvapi: false,
            // `GameSettings::auto_dxvk` é `Option<bool>` e o resolver trata
            // ausência como ligado; o formulário reflete esse padrão.
            auto_dxvk: true,
            auto_vkd3d: false,
            mangohud: false,
            gamemode: false,
            gamescope: false,
            gs_hdr: false,
            verbose_logs: false,
            script_pre: String::new(),
            script_post: String::new(),
            wrapper_count: 0,
            emulator_id: None,
            rom_path: String::new(),
            emulator_args: Vec::new(),
            emulator_core_path: String::new(),
        }
    }
}

/// Resultado de `gamesettings:get`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GameSettingsEnvelope {
    pub settings: GameSettingsView,
    pub default_prefix: String,
}

/// Toggles suportados pelo diálogo; o `App` traduz cada um para o campo
/// camelCase do contrato.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchToggle {
    Esync,
    Fsync,
    WineWayland,
    Wow64,
    FsrHack,
    AutoNvapi,
    Mangohud,
    Gamemode,
    Gamescope,
    GsHdr,
    VerboseLogs,
}

impl GameSettingsView {
    pub fn set_toggle(&mut self, toggle: LaunchToggle, value: bool) {
        match toggle {
            LaunchToggle::Esync => self.esync = value,
            LaunchToggle::Fsync => self.fsync = value,
            LaunchToggle::WineWayland => self.wine_wayland = value,
            LaunchToggle::Wow64 => self.wow64 = value,
            LaunchToggle::FsrHack => self.fsr_hack = value,
            LaunchToggle::AutoNvapi => self.auto_nvapi = value,
            LaunchToggle::Mangohud => self.mangohud = value,
            LaunchToggle::Gamemode => self.gamemode = value,
            LaunchToggle::Gamescope => self.gamescope = value,
            LaunchToggle::GsHdr => self.gs_hdr = value,
            LaunchToggle::VerboseLogs => self.verbose_logs = value,
        }
    }

    /// Patch enviado a `gamesettings:set`.
    ///
    /// Todos os campos abaixo estão em `ALLOWED_GAME_SETTINGS` no
    /// `arcadia-ipc`; os não editáveis não entram para não sobrescrever
    /// configuração feita na UI web.
    pub fn to_patch(&self) -> Value {
        let env_vars = self
            .env_vars
            .iter()
            .filter(|variable| !variable.name.trim().is_empty())
            .map(|variable| {
                json!({
                    "name": variable.name.trim(),
                    "value": variable.value,
                })
            })
            .collect::<Vec<_>>();

        json!({
            "prefixPath": self.prefix_path.trim(),
            "exePath": self.exe_path.trim(),
            "gameArgs": self.game_args.trim(),
            "dxvkHud": self.dxvk_hud.trim(),
            "envVars": env_vars,
            "esync": self.esync,
            "fsync": self.fsync,
            "wineWayland": self.wine_wayland,
            "wow64": self.wow64,
            "fsrHack": self.fsr_hack,
            "autoNVAPI": self.auto_nvapi,
            "mangohud": self.mangohud,
            "gamemode": self.gamemode,
            "gamescope": self.gamescope,
            "gsHdr": self.gs_hdr,
            "verboseLogs": self.verbose_logs,
            "emulatorId": self.emulator_id,
            "romPath": self.rom_path.trim(),
            "emulatorArgs": self.emulator_args,
            "emulatorCorePath": self.emulator_core_path.trim(),
        })
    }

    /// Payload de `emulators:resolve` para a configuração atual do jogo.
    pub fn emulator_resolve_payload(&self, launch_mode: &str) -> Value {
        let mut payload = serde_json::Map::new();
        if let Some(emulator_id) = self
            .emulator_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            payload.insert("emulatorId".to_string(), json!(emulator_id));
        }
        payload.insert("romPath".to_string(), json!(self.rom_path.trim()));
        if !self.emulator_args.is_empty() {
            payload.insert("extraArgs".to_string(), json!(self.emulator_args));
        }
        if !self.emulator_core_path.trim().is_empty() {
            payload.insert(
                "corePath".to_string(),
                json!(self.emulator_core_path.trim()),
            );
        }
        payload.insert("launchMode".to_string(), json!(launch_mode));
        Value::Object(payload)
    }
}

fn optional_string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|text| !text.trim().is_empty())
}

fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn bool_field(value: &Value, key: &str, default: bool) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(default)
}

/// `emulatorArgs` chega como array na UI web; respostas antigas podem trazer a
/// string única que o resolvedor de launch ainda lê.
fn string_or_args_field(value: &Value, key: &str) -> Vec<String> {
    match value.get(key) {
        Some(Value::Array(_)) => string_list(value.get(key)),
        Some(Value::String(text)) => parse_emulator_args(text),
        _ => Vec::new(),
    }
}

/// Lê o objeto de settings devolvido por `gamesettings:get`/`set`.
pub fn parse_game_settings(value: &Value) -> GameSettingsView {
    let env_vars = value
        .get("envVars")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let name = item.get("name").and_then(Value::as_str)?.trim();
                    if name.is_empty() {
                        return None;
                    }
                    Some(EnvVarSetting {
                        name: name.to_string(),
                        value: string_field(item, "value"),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    GameSettingsView {
        wine_version: optional_string_field(value, "wineVersion"),
        prefix_path: string_field(value, "prefixPath"),
        exe_path: string_field(value, "exePath"),
        game_args: string_field(value, "gameArgs"),
        dxvk_hud: string_field(value, "dxvkHud"),
        env_vars,
        esync: bool_field(value, "esync", false),
        fsync: bool_field(value, "fsync", false),
        wine_wayland: bool_field(value, "wineWayland", false),
        wow64: bool_field(value, "wow64", false),
        fsr_hack: bool_field(value, "fsrHack", false),
        auto_nvapi: bool_field(value, "autoNVAPI", false),
        auto_dxvk: bool_field(value, "autoDXVK", true),
        auto_vkd3d: bool_field(value, "autoVKD3D", false),
        mangohud: bool_field(value, "mangohud", false),
        gamemode: bool_field(value, "gamemode", false),
        gamescope: bool_field(value, "gamescope", false),
        gs_hdr: bool_field(value, "gsHdr", false),
        verbose_logs: bool_field(value, "verboseLogs", false),
        script_pre: string_field(value, "scriptPre"),
        script_post: string_field(value, "scriptPost"),
        wrapper_count: value
            .get("wrappers")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0),
        emulator_id: optional_string_field(value, "emulatorId"),
        rom_path: string_field(value, "romPath"),
        emulator_args: string_or_args_field(value, "emulatorArgs"),
        emulator_core_path: string_field(value, "emulatorCorePath"),
    }
}

pub fn parse_game_settings_envelope(value: &Value) -> GameSettingsEnvelope {
    let settings = value.get("settings").unwrap_or(value);
    GameSettingsEnvelope {
        settings: parse_game_settings(settings),
        default_prefix: string_field(value, "defaultPrefix"),
    }
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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

// ── Conta ───────────────────────────────────────────────────────────────────

/// Sessão segura devolvida por `account:status` (sem tokens).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AccountSession {
    pub id: String,
    pub email: Option<String>,
    pub username: Option<String>,
}

/// Perfil público de `account:profile`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AccountProfile {
    pub username: String,
    pub avatar_url: Option<String>,
    pub display_name: Option<String>,
    pub summary: Option<String>,
    pub country: Option<String>,
    pub city: Option<String>,
    pub showcase: Vec<String>,
    pub background_url: Option<String>,
    pub banner_url: Option<String>,
}

impl AccountProfile {
    /// `display_name` quando preenchido; senão o username.
    pub fn display(&self) -> &str {
        self.display_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&self.username)
    }

    /// "Cidade, País" com as partes disponíveis.
    pub fn location(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(city) = self.city.as_deref().filter(|city| !city.trim().is_empty()) {
            parts.push(city.trim().to_string());
        }
        if let Some(country) = self
            .country
            .as_deref()
            .filter(|country| !country.trim().is_empty())
        {
            parts.push(country.trim().to_string());
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }
}

/// Resultado de login/cadastro: `ok:true` (com eventual `usernameReal`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuthSuccess {
    pub username_real: Option<String>,
}

/// `account:status` — `{ session: <safe session|null>, error: null }`.
pub fn parse_account_status(value: Value) -> Result<Option<AccountSession>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "account:status devolveu um payload que não é um objeto".to_string())?;

    if let Some(error) = object
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|error| !error.is_empty())
    {
        return Err(error.to_string());
    }

    let Some(session) = object.get("session") else {
        return Ok(None);
    };
    if session.is_null() {
        return Ok(None);
    }
    let user = session
        .get("user")
        .and_then(Value::as_object)
        .ok_or_else(|| "account:status devolveu uma sessão sem user".to_string())?;
    let id = optional_map_text_field(user, "id")
        .ok_or_else(|| "account:status devolveu uma sessão sem id".to_string())?;

    Ok(Some(AccountSession {
        id,
        email: optional_map_text_field(user, "email"),
        username: optional_map_text_field(user, "username"),
    }))
}

/// `account:profile` — `{ ok, profile: {…}, error? }`.
pub fn parse_account_profile(value: Value) -> Result<AccountProfile, String> {
    if let Some(code) = action_error_code(&value) {
        return Err(code);
    }
    let profile = value
        .get("profile")
        .and_then(Value::as_object)
        .ok_or_else(|| "account:profile não devolveu profile".to_string())?;

    Ok(AccountProfile {
        username: optional_map_text_field(profile, "username").unwrap_or_default(),
        avatar_url: optional_image_url_from(profile.get("avatar_url")),
        display_name: optional_map_text_field(profile, "display_name"),
        summary: optional_map_text_field(profile, "summary"),
        country: optional_map_text_field(profile, "country"),
        city: optional_map_text_field(profile, "city"),
        showcase: string_list(profile.get("showcase")),
        background_url: optional_image_url_from(profile.get("background_url")),
        banner_url: optional_image_url_from(profile.get("banner_url")),
    })
}

/// `account:signIn`/`account:signUp` — `{ ok, usernameReal?, error? }`.
pub fn parse_auth_result(value: Value) -> Result<AuthSuccess, String> {
    if let Some(code) = action_error_code(&value) {
        return Err(code);
    }
    if !value.is_object() {
        return Err("resposta de autenticação inválida".to_string());
    }
    Ok(AuthSuccess {
        username_real: optional_text_field(&value, "usernameReal"),
    })
}

/// Respostas de ação de conta (`signOut`, `updateProfile`, `setAvatar`).
pub fn parse_account_action(value: Value) -> Result<(), String> {
    if let Some(code) = action_error_code(&value) {
        return Err(code);
    }
    if !value.is_object() {
        return Err("resposta de conta inválida".to_string());
    }
    Ok(())
}

/// URL pública devolvida por `account:setAvatar`/`account:setAvatarBytes`.
///
/// O IPC atual responde `avatar_url` (o `url` existe em `account:setBackground`);
/// aceitar as duas chaves mantém o parser alinhado ao contrato documentado.
pub fn parse_avatar_upload(value: Value) -> Result<String, String> {
    if let Some(code) = action_error_code(&value) {
        return Err(code);
    }
    optional_text_field(&value, "avatar_url")
        .or_else(|| optional_text_field(&value, "url"))
        .ok_or_else(|| "account:setAvatar não devolveu a URL do avatar".to_string())
}

/// URL pública devolvida por `account:setBackground` (`{ ok, url, error? }`).
pub fn parse_background_upload(value: Value) -> Result<String, String> {
    if let Some(code) = action_error_code(&value) {
        return Err(code);
    }
    optional_text_field(&value, "url")
        .or_else(|| optional_text_field(&value, "background_url"))
        .ok_or_else(|| "account:setBackground não devolveu a URL da imagem".to_string())
}

/// Decodifica o data URL de `avatar:load` em um `Handle` do iced.
pub fn avatar_handle_from_data_url(src: &str) -> Result<Handle, String> {
    let (_, encoded) = src
        .split_once(',')
        .ok_or_else(|| "avatar:load devolveu um data URL inválido".to_string())?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|error| format!("avatar:load devolveu base64 inválido: {error}"))?;
    decode_cover(&bytes)
        .map(|(width, height, pixels)| Handle::from_rgba(width, height, pixels))
        .ok_or_else(|| "avatar:load devolveu uma imagem inválida".to_string())
}

fn action_error_code(value: &Value) -> Option<String> {
    (value.get("ok").and_then(Value::as_bool) == Some(false)).then(|| {
        value
            .get("error")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|error| !error.is_empty())
            .unwrap_or("erro_desconhecido")
            .to_string()
    })
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

    #[allow(dead_code)]
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

/// Item de `friends:search`, com a relação atual (`status`/`incoming`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FriendSearchResult {
    pub id: String,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub status: Option<String>,
    pub incoming: bool,
}

impl FriendSearchResult {
    pub fn display(&self) -> &str {
        self.display_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&self.username)
    }
}

/// `friends:search` — `{ ok, results: [...] }`.
pub fn parse_friends_search(value: Value) -> Result<Vec<FriendSearchResult>, String> {
    if let Some(code) = action_error_code(&value) {
        return Err(friends_error_message(&code));
    }
    let results = value
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| "friends:search não devolveu results".to_string())?;

    Ok(results
        .iter()
        .filter_map(|item| {
            let id = text_field(item, "id");
            (!id.is_empty()).then(|| {
                let username = text_field(item, "username");
                FriendSearchResult {
                    username: if username.is_empty() {
                        "?".to_string()
                    } else {
                        username
                    },
                    display_name: optional_text_field(item, "display_name"),
                    avatar_url: optional_text_field(item, "avatar_url"),
                    status: optional_text_field(item, "status"),
                    incoming: item
                        .get("incoming")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    id,
                }
            })
        })
        .collect())
}

/// Perfil resumido devolvido por `friends:profile` (RPC `friend_profile`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FriendProfile {
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub summary: Option<String>,
    pub country: Option<String>,
    pub city: Option<String>,
    pub background_url: Option<String>,
    pub banner_url: Option<String>,
    pub games: Option<u64>,
    pub playtime_hours: Option<u64>,
    pub friends: Option<u64>,
}

impl FriendProfile {
    pub fn display(&self) -> &str {
        self.display_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&self.username)
    }

    pub fn initial(&self) -> String {
        self.display()
            .chars()
            .next()
            .map(|character| character.to_uppercase().to_string())
            .unwrap_or_else(|| "?".to_string())
    }

    pub fn location(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(city) = self.city.as_deref().filter(|city| !city.trim().is_empty()) {
            parts.push(city.trim().to_string());
        }
        if let Some(country) = self
            .country
            .as_deref()
            .filter(|country| !country.trim().is_empty())
        {
            parts.push(country.trim().to_string());
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }
}

/// `friends:profile` — `{ ok, profile, games?, friends?, stats? }`.
pub fn parse_friend_profile(value: Value) -> Result<FriendProfile, String> {
    if let Some(code) = action_error_code(&value) {
        return Err(friends_error_message(&code));
    }
    let profile = value
        .get("profile")
        .and_then(Value::as_object)
        .ok_or_else(|| "friends:profile não devolveu profile".to_string())?;
    let stats = value.get("stats").and_then(Value::as_object);

    Ok(FriendProfile {
        username: optional_map_text_field(profile, "username").unwrap_or_default(),
        display_name: optional_map_text_field(profile, "display_name"),
        avatar_url: optional_image_url_from(profile.get("avatar_url")),
        summary: optional_map_text_field(profile, "summary"),
        country: optional_map_text_field(profile, "country"),
        city: optional_map_text_field(profile, "city"),
        background_url: optional_image_url_from(profile.get("background_url")),
        banner_url: optional_image_url_from(profile.get("banner_url")),
        games: stats
            .and_then(|stats| stats.get("games"))
            .and_then(value_to_u64),
        playtime_hours: stats
            .and_then(|stats| stats.get("playtime_hours"))
            .and_then(|value| value_f64(Some(value)))
            .map(|hours| hours.max(0.0).round() as u64),
        friends: value
            .get("friends")
            .and_then(Value::as_array)
            .map(|friends| friends.len() as u64),
    })
}

/// Ação de amizade (`send`/`accept`/`cancel`/`remove`).
pub fn parse_friends_action(value: Value) -> Result<(), String> {
    if let Some(code) = action_error_code(&value) {
        return Err(friends_error_message(&code));
    }
    if !value.is_object() {
        return Err("resposta de amigos inválida".to_string());
    }
    Ok(())
}

fn friends_error_message(code: &str) -> String {
    match code {
        "nao_logado" => "Entre na sua conta para usar amigos.".to_string(),
        "token_invalido" => "Sua sessão expirou. Entre novamente.".to_string(),
        "conta_trocada" => "A conta ativa mudou durante a consulta. Tente novamente.".to_string(),
        "rede_indisponivel" => "Sem conexão com o serviço de amigos. Tente novamente.".to_string(),
        "destino_invalido" => "Destino do pedido inválido.".to_string(),
        "perfil_indisponivel" => "O perfil deste amigo não está disponível.".to_string(),
        other => format!("friends: {other}"),
    }
}

/// Conquista pública de um amigo (`friends:achievements`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FriendAchievement {
    pub appid: String,
    pub apiname: String,
    pub title: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub percent: Option<f64>,
    pub unlocked: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FriendAchievements {
    pub items: Vec<FriendAchievement>,
    pub from_cache: bool,
}

/// Resumo exibido no cartão do amigo; mesma leitura da UI web.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FriendAchievementsSummary {
    pub total: usize,
    pub platinum: usize,
    pub games: usize,
    /// `None` quando não há conquista 100% (a web mostra "—").
    pub platinum_percent: Option<u32>,
}

impl FriendAchievements {
    pub fn summary(&self) -> FriendAchievementsSummary {
        let total = self.items.len();
        let platinum = self
            .items
            .iter()
            .filter(|item| item.percent.is_some_and(|percent| percent >= 100.0))
            .count();
        let mut games: Vec<&str> = self
            .items
            .iter()
            .map(|item| item.appid.as_str())
            .filter(|appid| !appid.is_empty())
            .collect();
        games.sort_unstable();
        games.dedup();
        let platinum_percent = (total > 0 && platinum > 0).then(|| {
            ((platinum as f64 / total as f64) * 100.0).round() as u32
        });
        FriendAchievementsSummary {
            total,
            platinum,
            games: games.len(),
            platinum_percent,
        }
    }
}

/// `friends:achievements` — `{ ok, achievements, deCache? }`.
pub fn parse_friend_achievements(value: Value) -> Result<FriendAchievements, String> {
    if let Some(code) = action_error_code(&value) {
        return Err(friends_error_message(&code));
    }
    let raw = match value.get("achievements") {
        Some(Value::Array(items)) => items,
        Some(Value::Null) | None => {
            // `null` é lista vazia; ausência do campo também não inventa dados.
            return Ok(FriendAchievements {
                items: Vec::new(),
                from_cache: value
                    .get("deCache")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            });
        }
        Some(other) => {
            other
                .get("items")
                .or_else(|| other.get("achievements"))
                .and_then(Value::as_array)
                .ok_or_else(|| "friends:achievements devolveu uma lista inválida".to_string())?
        }
    };
    let items = raw.iter().filter_map(friend_achievement).collect();
    Ok(FriendAchievements {
        items,
        from_cache: value
            .get("deCache")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn friend_achievement(value: &Value) -> Option<FriendAchievement> {
    let appid = text_field_any(value, "appid")?;
    let apiname = text_field_any(value, "apiname")
        .or_else(|| text_field_any(value, "key"))
        .or_else(|| text_field_any(value, "apiName"))?;
    let title = optional_text_field(value, "title")
        .or_else(|| optional_text_field(value, "name"))
        .unwrap_or_else(|| apiname.clone());
    let unlocked = value
        .get("unlock")
        .or_else(|| value.get("unlocked"))
        .and_then(value_truthy)
        .unwrap_or(false);
    Some(FriendAchievement {
        appid,
        apiname,
        title,
        description: optional_text_field(value, "desc")
            .or_else(|| optional_text_field(value, "description")),
        icon: optional_image_url_from(value.get("icon")),
        percent: value_f64(value.get("percent")),
        unlocked,
    })
}

/// `unlock` chega como bool, número (timestamp) ou string nas variações do
/// backend; só o que é inequivocamente verdadeiro conta como desbloqueado.
fn value_truthy(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        Value::Number(number) => number.as_f64().map(|number| number != 0.0),
        Value::String(text) => match text.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "sim" | "yes" => Some(true),
            "false" | "0" | "nao" | "não" | "no" | "" => Some(false),
            _ => Some(true),
        },
        _ => None,
    }
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

/// Ações com semântica própria do registro de plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginAction {
    Enable,
    Disable,
    Install,
    Remove,
    Unregister,
}

impl PluginAction {
    pub fn channel(self) -> &'static str {
        match self {
            Self::Enable => "plugins:enable",
            Self::Disable => "plugins:disable",
            Self::Install => "plugins:install",
            Self::Remove => "plugins:remove",
            Self::Unregister => "plugins:unregister",
        }
    }

    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            Self::Enable => "Ativar",
            Self::Disable => "Desativar",
            Self::Install => "Instalar",
            Self::Remove => "Remover",
            Self::Unregister => "Desregistrar",
        }
    }

    /// Rótulo da ação na UI, no idioma ativo.
    pub fn label_i18n<'a>(self, i18n: &'a crate::i18n::I18n) -> &'a str {
        match self {
            Self::Enable => i18n.t("native.components.enable"),
            Self::Disable => i18n.t("native.components.disable"),
            Self::Install => i18n.t("native.components.install"),
            Self::Remove => i18n.t("common.remover"),
            Self::Unregister => i18n.t("native.components.unregister"),
        }
    }

    #[allow(dead_code)]
    pub fn success_message(self) -> &'static str {
        match self {
            Self::Enable => "Componente ativado.",
            Self::Disable => "Componente desativado.",
            Self::Install => "Componente detectado e ativado.",
            Self::Remove => "Componente desativado; o arquivo do sistema foi preservado.",
            Self::Unregister => "Componente removido do registro.",
        }
    }

    /// Aviso de sucesso no idioma ativo.
    pub fn success_message_i18n<'a>(self, i18n: &'a crate::i18n::I18n) -> &'a str {
        match self {
            Self::Enable => i18n.t("native.components.success.enable"),
            Self::Disable => i18n.t("native.components.success.disable"),
            Self::Install => i18n.t("native.components.success.install"),
            Self::Remove => i18n.t("native.components.success.remove"),
            Self::Unregister => i18n.t("native.components.success.unregister"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginActionOutcome {
    Success,
    /// `plugins:install`/`enable` recusaram porque o arquivo não existe.
    NotDetected,
}

pub fn parse_plugin_action(
    value: &Value,
    action: PluginAction,
) -> Result<PluginActionOutcome, String> {
    match value.get("ok").and_then(Value::as_bool) {
        Some(true) => Ok(PluginActionOutcome::Success),
        Some(false) => {
            let code = optional_text_field(value, "error").unwrap_or_default();
            if code == "not_detected" {
                Ok(PluginActionOutcome::NotDetected)
            } else {
                Err(plugin_error_message(&code, action.channel()))
            }
        }
        None => Err(format!(
            "{} devolveu uma resposta sem `ok`",
            action.channel()
        )),
    }
}

/// Resultado de `plugins:register`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginRegistration {
    pub id: String,
    pub name: String,
}

pub fn parse_plugin_register(value: Value) -> Result<PluginRegistration, String> {
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        let mut code = optional_text_field(&value, "error").unwrap_or_default();
        if code.is_empty() {
            code = value
                .get("errors")
                .and_then(Value::as_array)
                .and_then(|errors| errors.first())
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
        }
        return Err(plugin_error_message(&code, "plugins:register"));
    }

    let plugin = value
        .get("plugin")
        .filter(|plugin| plugin.is_object())
        .ok_or_else(|| "plugins:register não devolveu o plugin registrado".to_string())?;
    let id = text_field(plugin, "id");
    if id.is_empty() {
        return Err("plugins:register devolveu um plugin sem id".to_string());
    }
    let name = plugin
        .get("manifest")
        .map(|manifest| text_field(manifest, "name"))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| id.clone());
    Ok(PluginRegistration { id, name })
}

/// Resultado resumido de `plugins:verify` (sem caminhos privados).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginVerification {
    pub id: String,
    pub ok: bool,
    pub valid: bool,
    pub verified: bool,
    pub declared: bool,
    pub source: String,
    pub error: String,
}

pub fn parse_plugin_verification(value: Value) -> Result<PluginVerification, String> {
    let id = text_field(&value, "id");
    if id.is_empty() {
        return Err("plugins:verify devolveu um resultado sem id".to_string());
    }
    Ok(PluginVerification {
        id,
        ok: value.get("ok").and_then(Value::as_bool).unwrap_or(false),
        valid: value
            .get("valid")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        verified: value
            .get("verified")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        declared: value
            .get("declared")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        source: text_field(&value, "source"),
        error: optional_text_field(&value, "error").unwrap_or_default(),
    })
}

/// Detalhes públicas de um plugin (`plugins:get`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginDetails {
    pub id: String,
    pub name: String,
    pub description: String,
    pub installed: bool,
    pub enabled: bool,
    pub valid: bool,
    pub source: String,
    pub error: String,
    pub version: Option<String>,
    pub permissions: Vec<String>,
    pub signed: bool,
}

pub fn parse_plugin_details(value: Value) -> Result<PluginDetails, String> {
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        let code = envelope_error(&value, "plugins:get");
        return Err(plugin_error_message(&code, "plugins:get"));
    }
    let plugin = value
        .get("plugin")
        .filter(|plugin| plugin.is_object())
        .ok_or_else(|| "plugins:get não devolveu plugin".to_string())?;
    let id = text_field(plugin, "id");
    if id.is_empty() {
        return Err("plugins:get devolveu um plugin sem id".to_string());
    }
    let manifest = plugin.get("manifest").filter(|manifest| manifest.is_object());
    let manifest_name = manifest.map(|manifest| text_field(manifest, "name"));
    let name = manifest_name
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| plugin_name(&id, &text_field(plugin, "name")));
    let description = manifest
        .and_then(|manifest| optional_text_field(manifest, "description"))
        .or_else(|| optional_text_field(plugin, "description"))
        .unwrap_or_else(|| plugin_description(&id, &text_field(plugin, "descKey")));
    let version = manifest.and_then(|manifest| optional_text_field(manifest, "version"));
    let permissions = manifest
        .map(|manifest| string_list(manifest.get("permissions")))
        .unwrap_or_default();
    let signed = manifest.is_some_and(|manifest| {
        optional_text_field(manifest, "signature").is_some()
            && optional_text_field(manifest, "signingKeyId").is_some()
    });

    Ok(PluginDetails {
        id,
        name,
        description,
        installed: plugin
            .get("installed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        enabled: plugin
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        valid: plugin.get("valid").and_then(Value::as_bool).unwrap_or(false),
        source: text_field(plugin, "source"),
        error: optional_text_field(plugin, "error").unwrap_or_default(),
        version,
        permissions,
        signed,
    })
}

fn plugin_error_message(code: &str, channel: &str) -> String {
    match code {
        "not_detected" => {
            "Arquivo do componente não encontrado no sistema; nada foi baixado.".to_string()
        }
        "plugin_desconhecido" => "Componente desconhecido pelo registro.".to_string(),
        "plugin_embutido" => "Componentes embutidos não podem ser desregistrados.".to_string(),
        "plugin_invalido" => "Id de componente inválido.".to_string(),
        "manifest_invalido" => "Manifesto do componente inválido.".to_string(),
        "registro_nao_gravavel" => {
            "Não foi possível gravar o registro de componentes.".to_string()
        }
        "id_reservado" => "O id do componente pertence a um componente embutido.".to_string(),
        "" => format!("{channel} falhou sem detalhes."),
        other => format!("{channel}: {other}"),
    }
}

// ── Comunidade ─────────────────────────────────────────────────────────────

/// Review pública já normalizada para a UI.
#[derive(Debug, Clone, PartialEq)]
pub struct CommunityReview {
    pub id: String,
    pub author: String,
    pub title: Option<String>,
    pub text: String,
    pub rating: Option<f64>,
    pub positive: Option<bool>,
    pub hours: Option<f64>,
    pub created_at: Option<String>,
    /// Autor da review; usado para habilitar edição/remoção só na própria.
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunityCollectionItem {
    pub appid: String,
    pub title: Option<String>,
}

/// Coleção pública já normalizada para a UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunityCollection {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub visibility: Option<String>,
    pub owner: String,
    pub item_count: u64,
    pub items: Vec<CommunityCollectionItem>,
    /// Dono da coleção; `mine=true` também habilita as mutações.
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommunityPagination {
    pub limit: u64,
    pub offset: u64,
    pub has_more: bool,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommunityReviewsPage {
    pub reviews: Vec<CommunityReview>,
    pub pagination: CommunityPagination,
    pub offline: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommunityCollectionsPage {
    pub collections: Vec<CommunityCollection>,
    pub pagination: CommunityPagination,
    pub offline: bool,
}

/// Falha de leitura da comunidade; `offline` mantém o estado explícito mesmo
/// quando o serviço não tinha cache para devolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunityError {
    pub message: String,
    pub offline: bool,
    pub code: Option<String>,
}

/// Appid usado pelas rotas `/community/v1`: sempre sem o prefixo `steam:`.
pub fn community_appid(appid: &str) -> String {
    let trimmed = appid.trim();
    trimmed
        .strip_prefix("steam:")
        .unwrap_or(trimmed)
        .to_string()
}

pub fn parse_community_reviews(value: Value) -> Result<CommunityReviewsPage, CommunityError> {
    let offline = value
        .get("offline")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        return Err(community_envelope_error(
            &value,
            "community:reviews",
            offline,
        ));
    }
    let Some(items) = value
        .get("reviews")
        .filter(|items| items.is_array())
        .or_else(|| value.get("items").filter(|items| items.is_array()))
    else {
        return Err(CommunityError {
            message: "community:reviews não devolveu reviews.".to_string(),
            offline,
            code: None,
        });
    };
    let reviews: Vec<CommunityReview> = items
        .as_array()
        .map(|items| items.iter().filter_map(community_review).collect())
        .unwrap_or_default();
    let pagination = community_pagination(&value, reviews.len());
    Ok(CommunityReviewsPage {
        reviews,
        pagination,
        offline,
    })
}

pub fn parse_community_collections(
    value: Value,
) -> Result<CommunityCollectionsPage, CommunityError> {
    let offline = value
        .get("offline")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        return Err(community_envelope_error(
            &value,
            "community:collections",
            offline,
        ));
    }
    let Some(items) = value
        .get("collections")
        .filter(|items| items.is_array())
        .or_else(|| value.get("lists").filter(|items| items.is_array()))
        .or_else(|| value.get("items").filter(|items| items.is_array()))
    else {
        return Err(CommunityError {
            message: "community:collections não devolveu coleções.".to_string(),
            offline,
            code: None,
        });
    };
    let collections: Vec<CommunityCollection> = items
        .as_array()
        .map(|items| items.iter().filter_map(community_collection).collect())
        .unwrap_or_default();
    let pagination = community_pagination(&value, collections.len());
    Ok(CommunityCollectionsPage {
        collections,
        pagination,
        offline,
    })
}

fn community_review(value: &Value) -> Option<CommunityReview> {
    let id = text_field_any(value, "id").unwrap_or_default();
    let author = community_author(value);
    let text = optional_text_field(value, "text").unwrap_or_default();
    if id.is_empty() && author.is_empty() && text.is_empty() {
        return None;
    }
    let positive = match value.get("positive") {
        Some(Value::Bool(positive)) => Some(*positive),
        Some(other) => value_f64(Some(other)).map(|positive| positive != 0.0),
        None => None,
    };
    Some(CommunityReview {
        id,
        author,
        title: optional_text_field(value, "title"),
        text,
        rating: value_f64(value.get("rating")),
        positive,
        hours: value_f64(value.get("hours")),
        created_at: optional_text_field(value, "created_at")
            .or_else(|| optional_text_field(value, "createdAt")),
        user_id: text_field_any(value, "user_id").or_else(|| {
            value
                .get("author")
                .and_then(|author| text_field_any(author, "id"))
        }),
    })
}

fn community_author(value: &Value) -> String {
    let nested = value.get("author").filter(|author| author.is_object());
    optional_text_field(value, "display_name")
        .or_else(|| nested.and_then(|author| optional_text_field(author, "display_name")))
        .or_else(|| optional_text_field(value, "username"))
        .or_else(|| nested.and_then(|author| optional_text_field(author, "username")))
        .unwrap_or_else(|| "Membro da comunidade".to_string())
}

fn community_collection(value: &Value) -> Option<CommunityCollection> {
    let id = text_field_any(value, "id").unwrap_or_default();
    let title = optional_text_field(value, "title");
    if id.is_empty() && title.is_none() {
        return None;
    }
    let items: Vec<CommunityCollectionItem> = value
        .get("items")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(community_collection_item).collect())
        .unwrap_or_default();
    let item_count = value_u64(value.get("item_count"))
        .or_else(|| value_u64(value.get("itemCount")))
        .unwrap_or(items.len() as u64);
    let owner = {
        let owner = value.get("owner").filter(|owner| owner.is_object());
        optional_text_field(value, "display_name")
            .or_else(|| owner.and_then(|owner| optional_text_field(owner, "display_name")))
            .or_else(|| optional_text_field(value, "username"))
            .or_else(|| owner.and_then(|owner| optional_text_field(owner, "username")))
            .unwrap_or_else(|| "Membro da comunidade".to_string())
    };
    Some(CommunityCollection {
        id,
        title: title.unwrap_or_else(|| "Coleção sem título".to_string()),
        description: optional_text_field(value, "description"),
        visibility: optional_text_field(value, "visibility"),
        owner,
        item_count,
        items,
        user_id: text_field_any(value, "user_id").or_else(|| {
            value
                .get("owner")
                .and_then(|owner| text_field_any(owner, "id"))
        }),
    })
}

fn community_collection_item(value: &Value) -> Option<CommunityCollectionItem> {
    let appid = text_field_any(value, "appid")?;
    Some(CommunityCollectionItem {
        appid,
        title: optional_text_field(value, "title"),
    })
}

fn community_pagination(value: &Value, item_count: usize) -> CommunityPagination {
    let page = value.get("pagination");
    let limit = page
        .and_then(|page| value_u64(page.get("limit")))
        .unwrap_or(item_count as u64);
    let offset = page
        .and_then(|page| value_u64(page.get("offset")))
        .unwrap_or(0);
    let total = page
        .and_then(|page| value_u64(page.get("total")))
        .or_else(|| value_u64(value.get("total")));
    let explicit_has_more = page
        .and_then(|page| page.get("has_more").or_else(|| page.get("hasMore")))
        .or_else(|| value.get("has_more").or_else(|| value.get("hasMore")))
        .and_then(Value::as_bool);
    let has_more = explicit_has_more
        .unwrap_or_else(|| total.is_some_and(|total| offset + (item_count as u64) < total));
    CommunityPagination {
        limit,
        offset,
        has_more,
        total,
    }
}

fn community_envelope_error(value: &Value, channel: &str, offline: bool) -> CommunityError {
    let offline = offline
        || value
            .get("offline")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    let code =
        optional_text_field(value, "code").or_else(|| optional_text_field(value, "error"));
    let raw = optional_text_field(value, "message")
        .or_else(|| optional_text_field(value, "error"))
        .unwrap_or_else(|| channel.to_string());
    let message = match code.as_deref() {
        Some("nao_logado") | Some("token_invalido") => {
            "Entre na sua conta Arcadia para acessar a comunidade.".to_string()
        }
        Some("conta_trocada") => {
            "A conta ativa mudou durante a consulta. Tente novamente.".to_string()
        }
        Some("offline") if offline => {
            if raw == "offline" {
                "Sem conexão com a comunidade.".to_string()
            } else {
                format!("Sem conexão com a comunidade: {raw}")
            }
        }
        _ if raw == "nao_logado" => {
            "Entre na sua conta Arcadia para acessar a comunidade.".to_string()
        }
        _ => raw,
    };
    CommunityError {
        message,
        offline,
        code,
    }
}

/// Resultado de uma mutação de comunidade. O objeto alterado vem preenchido
/// quando o envelope o devolve; remoções/denúncias só confirmam `ok`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CommunityMutation {
    pub review: Option<CommunityReview>,
    pub collection: Option<CommunityCollection>,
}

pub fn parse_community_review_mutation(
    value: Value,
) -> Result<CommunityMutation, CommunityError> {
    let offline = community_offline(&value);
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        return Err(community_envelope_error(
            &value,
            "community:review",
            offline,
        ));
    }
    let review = value
        .get("review")
        .or_else(|| value.get("data"))
        .and_then(community_review)
        .or_else(|| {
            value
                .get("data")
                .and_then(|data| data.get("review"))
                .and_then(community_review)
        })
        .ok_or_else(|| CommunityError {
            message: "community:review não devolveu a avaliação salva.".to_string(),
            offline,
            code: None,
        })?;
    Ok(CommunityMutation {
        review: Some(review),
        collection: None,
    })
}

pub fn parse_community_collection_mutation(
    value: Value,
) -> Result<CommunityMutation, CommunityError> {
    let offline = community_offline(&value);
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        return Err(community_envelope_error(
            &value,
            "community:collection",
            offline,
        ));
    }
    let collection = value
        .get("collection")
        .or_else(|| value.get("data"))
        .and_then(community_collection)
        .or_else(|| {
            value
                .get("data")
                .and_then(|data| data.get("collection"))
                .and_then(community_collection)
        })
        .ok_or_else(|| CommunityError {
            message: "community:collection não devolveu a coleção salva.".to_string(),
            offline,
            code: None,
        })?;
    Ok(CommunityMutation {
        review: None,
        collection: Some(collection),
    })
}

/// Remoções, denúncias e mutações de item confirmam com `ok` e o chamador
/// recarrega a lista; não há objeto obrigatório no envelope.
pub fn parse_community_action(
    value: Value,
    channel: &str,
) -> Result<CommunityMutation, CommunityError> {
    let offline = community_offline(&value);
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        return Err(community_envelope_error(&value, channel, offline));
    }
    Ok(CommunityMutation::default())
}

fn community_offline(value: &Value) -> bool {
    value
        .get("offline")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn community_dispatch_error(message: String) -> CommunityError {
    CommunityError {
        message,
        offline: false,
        code: None,
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

fn optional_map_text_field(
    map: &serde_json::Map<String, Value>,
    key: &str,
) -> Option<String> {
    map.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn optional_text_field_from(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// Campo textual que aceita string ou número — `appid`/`id` variam entre as
/// respostas da Steam, do Hubcap e de catálogos antigos.
fn text_field_any(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::String(text) => {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        }
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item {
                    Value::String(text) => {
                        let text = text.trim();
                        (!text.is_empty()).then(|| text.to_string())
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn first_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        }
        Value::Array(items) => items.iter().find_map(|item| match item {
            Value::String(text) => {
                let text = text.trim();
                (!text.is_empty()).then(|| text.to_string())
            }
            _ => None,
        }),
        _ => None,
    }
}

fn optional_image_url(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .and_then(normalize_image_url)
}

fn optional_image_url_from(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).and_then(normalize_image_url)
}

/// Normaliza URLs de arte como o `media_url` do trailer: aceita `//`,
/// `http(s)` e descarta qualquer outro esquema que o cliente HTTP não abre.
fn normalize_image_url(input: &str) -> Option<String> {
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

fn steam_store_header(appid: &str) -> Option<String> {
    let appid = appid.trim().strip_prefix("steam:").unwrap_or(appid.trim());
    (!appid.is_empty() && appid.chars().all(|character| character.is_ascii_digit()))
        .then(|| format!("https://cdn.akamai.steamstatic.com/steam/apps/{appid}/header.jpg"))
}

fn check_channel_ok(value: &Value, channel: &str) -> Result<(), String> {
    match value.get("ok").and_then(Value::as_bool) {
        Some(false) => {
            let error = match value.get("error") {
                Some(Value::String(text)) if !text.trim().is_empty() => text.trim().to_string(),
                Some(other) if !other.is_null() => other.to_string(),
                _ => "erro desconhecido".to_string(),
            };
            Err(format!("{channel}: {error}"))
        }
        _ => Ok(()),
    }
}

fn value_to_year_number(value: &Value) -> Option<u32> {
    match value {
        Value::Number(number) => number.as_u64().and_then(|year| u32::try_from(year).ok()),
        Value::String(text) => text.trim().parse::<u32>().ok(),
        _ => None,
    }
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
#[allow(dead_code)]
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

// ── Emuladores ──────────────────────────────────────────────────────────────

/// Pasta de ROM do perfil (`emulators:profile:set`/`emulators:profiles`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorRomFolder {
    pub path: String,
    #[serde(default = "default_true")]
    pub recursive: bool,
}

fn default_true() -> bool {
    true
}

/// Perfil persistido em `emulators.json`, como devolvido por
/// `emulators:list` (`profile`) e `emulators:profiles`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EmulatorProfile {
    pub id: String,
    pub executable: String,
    pub core_path: String,
    pub bios_path: String,
    pub rom_folders: Vec<EmulatorRomFolder>,
    pub args: Vec<String>,
    pub updated_at: i64,
}

/// Item do catálogo de `emulators:list`/`emulators:detect`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EmulatorInfo {
    pub id: String,
    pub name: String,
    pub systems: Vec<String>,
    pub description: String,
    pub candidates: Vec<String>,
    pub executable: String,
    pub detected_args: Vec<String>,
    pub available: bool,
    /// `builtin`, `detected` ou `configured`, direto do contrato.
    pub source: String,
    pub requires_core: bool,
    pub rom_extensions: Vec<String>,
    pub rom_directory_markers: Vec<String>,
    pub profile: Option<EmulatorProfile>,
}

/// Item de `emulators:status`: BIOS/firmware, processo e código de erro.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EmulatorStatus {
    pub ok: bool,
    pub emulator_id: String,
    pub kind: Option<String>,
    pub required: bool,
    pub installed: bool,
    pub detected_path: Option<String>,
    pub error: Option<String>,
    pub code: Option<String>,
    pub running: bool,
    pub running_pid: Option<u64>,
}

/// ROM encontrada pelo scanner e persistida no índice.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EmulatorRomEntry {
    pub path: String,
    pub name: String,
    pub relative_path: String,
    pub extension: String,
    pub kind: String,
    pub size_bytes: u64,
    pub mtime_ms: i64,
    pub sidecars: Vec<String>,
}

/// Entrada de `emulators:roms:index` por emulador.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EmulatorRomIndexEntry {
    pub updated_at: i64,
    pub directory: String,
    pub folders: Vec<EmulatorRomFolder>,
    pub truncated: bool,
    pub roms: Vec<EmulatorRomEntry>,
}

/// Resultado de `emulators:roms`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EmulatorScanResult {
    pub emulator_id: String,
    pub directory: Option<String>,
    pub recursive: bool,
    pub max_depth: u64,
    pub folders: Vec<EmulatorRomFolder>,
    pub truncated: bool,
    pub persisted: bool,
    pub roms: Vec<EmulatorRomEntry>,
}

/// Carga paralela da aba Emulação.
#[derive(Debug, Clone, Default)]
pub struct EmulationLoad {
    pub emulators: Vec<EmulatorInfo>,
    pub statuses: Vec<EmulatorStatus>,
    pub rom_index: std::collections::BTreeMap<String, EmulatorRomIndexEntry>,
    /// Falhas parciais de status/índice/perfis; o catálogo continua visível.
    pub warnings: Vec<String>,
}

/// Catálogo/status usados pelo seletor de emulador das opções do jogo.
#[derive(Debug, Clone, Default)]
pub struct GameEmulatorOptions {
    pub emulators: Vec<EmulatorInfo>,
    pub statuses: Vec<EmulatorStatus>,
}

/// Resultado de `emulators:resolve` (`ok`, `cmd`, `error`, `code`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmulatorResolveResult {
    pub ok: bool,
    pub cmd: Vec<String>,
    pub error: Option<String>,
    pub code: Option<String>,
}

impl EmulatorResolveResult {
    /// Mensagem do contrato para o banner; não inventa sucesso nem esconde o
    /// código estruturado quando os dois vêm preenchidos.
    pub fn error_text(&self) -> String {
        match (self.error.as_deref(), self.code.as_deref()) {
            (Some(error), Some(code)) if !code.is_empty() && code != error => {
                format!("{error} ({code})")
            }
            (Some(error), _) => error.to_string(),
            (None, Some(code)) if !code.is_empty() => code.to_string(),
            _ => "configuração do emulador inválida".to_string(),
        }
    }
}

/// Pedido de importação de uma ROM do índice como jogo customizado.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmulatorRomImport {
    pub emulator_id: String,
    pub rom_path: String,
    pub rom_name: String,
    pub extra_args: Vec<String>,
    pub core_path: Option<String>,
    /// Ids já presentes na biblioteca; a geração do slug os evita.
    pub occupied_ids: Vec<String>,
}

impl EmulatorRomImport {
    pub fn title(&self) -> String {
        custom_game_title(&self.rom_name)
    }

    pub fn game_id(&self) -> String {
        custom_emulator_game_id(&self.emulator_id, &self.title(), &self.occupied_ids)
    }

    /// Payload de `customgame:add` para `platform: "emulator"`.
    pub fn custom_game_payload(&self) -> Value {
        let id = self.game_id();
        json!({
            "id": id,
            "title": self.title(),
            "platform": "emulator",
            "emulatorId": self.emulator_id.trim(),
            "romPath": self.rom_path.trim(),
            "emulatorArgs": self.extra_args,
            "emulatorCorePath": self.core_path.as_deref().unwrap_or("").trim(),
        })
    }

    /// Patch de `gamesettings:set` para o `plan_launch` montar o argv.
    pub fn settings_patch(&self) -> Value {
        json!({
            "emulatorId": self.emulator_id.trim(),
            "romPath": self.rom_path.trim(),
            "emulatorArgs": self.extra_args,
            "emulatorCorePath": self.core_path.as_deref().unwrap_or("").trim(),
        })
    }

    pub fn resolve_payload(&self) -> Value {
        let mut payload = serde_json::Map::new();
        payload.insert("emulatorId".to_string(), json!(self.emulator_id.trim()));
        payload.insert("romPath".to_string(), json!(self.rom_path.trim()));
        if !self.extra_args.is_empty() {
            payload.insert("extraArgs".to_string(), json!(self.extra_args));
        }
        if let Some(core) = self.core_path.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
            payload.insert("corePath".to_string(), json!(core));
        }
        payload.insert("launchMode".to_string(), json!("hydra"));
        Value::Object(payload)
    }
}

/// Jogo importado: a biblioteca recarregada já inclui o novo `custom:<slug>`.
#[derive(Debug, Clone, Default)]
pub struct ImportedEmulatorRom {
    pub title: String,
    pub games: Vec<Game>,
}

/// Título do jogo a partir do nome do arquivo da ROM (sem extensão, máx. 200).
pub fn custom_game_title(rom_name: &str) -> String {
    let file = rom_name.rsplit(['/', '\\']).next().unwrap_or(rom_name);
    let stem = match file.rsplit_once('.') {
        Some((stem, _)) if !stem.trim().is_empty() => stem,
        _ => file,
    };
    let title = if stem.trim().is_empty() {
        file.trim()
    } else {
        stem.trim()
    };
    title.chars().take(200).collect()
}

fn slugify(value: &str) -> String {
    let mut slug = String::with_capacity(value.len());
    for character in value.to_lowercase().chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            slug.push(character);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_string()
}

/// Id `custom:<slug>` estável a partir do emulador + título.
///
/// O slug é limitado a 90 caracteres e, se o id já existir, recebe um sufixo
/// numérico em vez de sobrescrever o jogo existente.
pub fn custom_emulator_game_id(emulator_id: &str, title: &str, occupied: &[String]) -> String {
    let emulator = slugify(emulator_id);
    let title = slugify(title);
    let base = [emulator.as_str(), title.as_str()]
        .iter()
        .filter(|part| !part.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("-");
    let base = if base.is_empty() {
        "rom".to_string()
    } else {
        base.chars().take(90).collect::<String>()
    };
    let base = base.trim_matches('-').to_string();

    let candidate = format!("custom:{base}");
    if !occupied.iter().any(|id| id == &candidate) {
        return candidate;
    }
    for suffix_number in 2..=999u32 {
        let suffix = format!("-{suffix_number}");
        let keep = 90usize.saturating_sub(suffix.len());
        let shortened = base.chars().take(keep).collect::<String>();
        let shortened = shortened.trim_end_matches('-');
        let candidate = format!("custom:{shortened}{suffix}");
        if !occupied.iter().any(|id| id == &candidate) {
            return candidate;
        }
    }
    // Fallback determinístico para catálogos anormalmente grandes.
    format!("custom:{base}-{}", occupied.len())
}

/// Lê o envelope de `emulators:resolve`; `ok:false` é dado do contrato, não
/// erro de transporte.
pub fn parse_emulator_resolve(value: &Value) -> EmulatorResolveResult {
    EmulatorResolveResult {
        ok: value.get("ok").and_then(Value::as_bool).unwrap_or(false),
        cmd: string_list(value.get("cmd")),
        error: optional_text_field(value, "error"),
        code: optional_text_field(value, "code"),
    }
}

fn value_texts(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_emulator_rom_folder(value: &Value) -> Option<EmulatorRomFolder> {
    if let Some(path) = value.as_str() {
        let path = path.trim();
        return (!path.is_empty()).then(|| EmulatorRomFolder {
            path: path.to_string(),
            recursive: true,
        });
    }
    let path = text_field(value, "path");
    if path.is_empty() {
        return None;
    }
    Some(EmulatorRomFolder {
        path,
        recursive: value
            .get("recursive")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    })
}

fn parse_emulator_rom_folders(value: Option<&Value>) -> Vec<EmulatorRomFolder> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(parse_emulator_rom_folder)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_emulator_profile(value: &Value) -> Option<EmulatorProfile> {
    let id = text_field(value, "id");
    if id.is_empty() {
        return None;
    }
    Some(EmulatorProfile {
        id,
        executable: text_field(value, "executable"),
        core_path: text_field(value, "corePath"),
        bios_path: text_field(value, "biosPath"),
        rom_folders: parse_emulator_rom_folders(value.get("romFolders")),
        args: value
            .get("args")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        updated_at: value.get("updatedAt").and_then(Value::as_i64).unwrap_or(0),
    })
}

fn parse_emulator_info(value: &Value) -> Option<EmulatorInfo> {
    let id = text_field(value, "id");
    if id.is_empty() {
        return None;
    }
    let name = text_field(value, "name");
    Some(EmulatorInfo {
        name: if name.is_empty() { id.clone() } else { name },
        id,
        systems: value_texts(value.get("systems")),
        description: text_field(value, "description"),
        candidates: value_texts(value.get("candidates")),
        executable: text_field(value, "executable"),
        detected_args: value_texts(value.get("detectedArgs")),
        available: value.get("available").and_then(Value::as_bool).unwrap_or(false),
        source: {
            let source = text_field(value, "source");
            if source.is_empty() {
                "builtin".to_string()
            } else {
                source.to_ascii_lowercase()
            }
        },
        requires_core: value
            .get("requiresCore")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        rom_extensions: value_texts(value.get("romExtensions")),
        rom_directory_markers: value_texts(value.get("romDirectoryMarkers")),
        profile: value.get("profile").and_then(parse_emulator_profile),
    })
}

fn parse_emulator_status(value: &Value) -> Option<EmulatorStatus> {
    let emulator_id = text_field(value, "emulatorId");
    if emulator_id.is_empty() {
        return None;
    }
    Some(EmulatorStatus {
        ok: value.get("ok").and_then(Value::as_bool).unwrap_or(false),
        emulator_id,
        kind: optional_text_field(value, "kind"),
        required: value.get("required").and_then(Value::as_bool).unwrap_or(false),
        installed: value
            .get("installed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        detected_path: optional_text_field(value, "detectedPath"),
        error: optional_text_field(value, "error"),
        code: optional_text_field(value, "code"),
        running: value.get("running").and_then(Value::as_bool).unwrap_or(false),
        running_pid: value.get("runningPid").and_then(Value::as_u64),
    })
}

fn parse_emulator_rom_entry(value: &Value) -> Option<EmulatorRomEntry> {
    let path = text_field(value, "path");
    if path.is_empty() {
        return None;
    }
    let name = {
        let name = text_field(value, "name");
        if name.is_empty() {
            path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string()
        } else {
            name
        }
    };
    let relative_path = {
        let relative = text_field(value, "relativePath");
        if relative.is_empty() {
            name.clone()
        } else {
            relative
        }
    };
    Some(EmulatorRomEntry {
        path,
        name,
        relative_path,
        extension: text_field(value, "extension"),
        kind: {
            let kind = text_field(value, "kind");
            if kind.is_empty() {
                "file".to_string()
            } else {
                kind
            }
        },
        size_bytes: value.get("sizeBytes").and_then(Value::as_u64).unwrap_or(0),
        mtime_ms: value.get("mtimeMs").and_then(Value::as_i64).unwrap_or(0),
        sidecars: value_texts(value.get("sidecars")),
    })
}

fn parse_emulator_rom_entries(value: Option<&Value>) -> Vec<EmulatorRomEntry> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(parse_emulator_rom_entry)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_emulator_rom_index_entry(value: &Value) -> EmulatorRomIndexEntry {
    EmulatorRomIndexEntry {
        updated_at: value.get("updatedAt").and_then(Value::as_i64).unwrap_or(0),
        directory: text_field(value, "directory"),
        folders: parse_emulator_rom_folders(value.get("folders")),
        truncated: value.get("truncated").and_then(Value::as_bool).unwrap_or(false),
        roms: parse_emulator_rom_entries(value.get("roms")),
    }
}

/// `emulators:list`/`emulators:detect` → catálogo tipado.
pub fn parse_emulators_list(value: Value) -> Result<Vec<EmulatorInfo>, String> {
    check_envelope(&value, "emulators:list")?;
    let items = value
        .get("emulators")
        .and_then(Value::as_array)
        .ok_or_else(|| "emulators:list não devolveu a lista de emuladores".to_string())?;
    Ok(items.iter().filter_map(parse_emulator_info).collect())
}

/// `emulators:status` → BIOS/firmware/execução por emulador.
pub fn parse_emulators_status(value: Value) -> Result<Vec<EmulatorStatus>, String> {
    check_envelope(&value, "emulators:status")?;
    let items = value
        .get("statuses")
        .and_then(Value::as_array)
        .ok_or_else(|| "emulators:status não devolveu a lista de status".to_string())?;
    Ok(items.iter().filter_map(parse_emulator_status).collect())
}

/// `emulators:profiles` → perfis indexados pelo id do emulador.
pub fn parse_emulators_profiles(
    value: Value,
) -> Result<std::collections::BTreeMap<String, EmulatorProfile>, String> {
    check_envelope(&value, "emulators:profiles")?;
    let profiles = value
        .get("profiles")
        .and_then(Value::as_object)
        .ok_or_else(|| "emulators:profiles não devolveu o mapa de perfis".to_string())?;
    Ok(profiles
        .iter()
        .filter_map(|(id, profile)| {
            parse_emulator_profile(profile).map(|parsed| (id.clone(), parsed))
        })
        .collect())
}

/// `emulators:roms` → resultado do scanner configurado.
pub fn parse_emulator_roms(value: Value) -> Result<EmulatorScanResult, String> {
    check_envelope(&value, "emulators:roms")?;
    Ok(EmulatorScanResult {
        emulator_id: text_field(&value, "emulatorId"),
        directory: optional_text_field(&value, "directory"),
        recursive: value.get("recursive").and_then(Value::as_bool).unwrap_or(true),
        max_depth: value.get("maxDepth").and_then(Value::as_u64).unwrap_or(0),
        folders: parse_emulator_rom_folders(value.get("folders")),
        truncated: value.get("truncated").and_then(Value::as_bool).unwrap_or(false),
        persisted: value.get("persisted").and_then(Value::as_bool).unwrap_or(false),
        roms: parse_emulator_rom_entries(value.get("roms")),
    })
}

/// `emulators:roms:index` → índice persistido por emulador.
pub fn parse_emulators_rom_index(
    value: Value,
) -> Result<std::collections::BTreeMap<String, EmulatorRomIndexEntry>, String> {
    check_envelope(&value, "emulators:roms:index")?;
    let emulators = value
        .get("emulators")
        .and_then(Value::as_object)
        .ok_or_else(|| "emulators:roms:index não devolveu o índice".to_string())?;
    Ok(emulators
        .iter()
        .map(|(id, entry)| (id.clone(), parse_emulator_rom_index_entry(entry)))
        .collect())
}

/// Limite de argumentos aceito pelo registry (`MAX_ARGS`).
pub const MAX_EMULATOR_ARGS: usize = 32;

/// Limite de pastas de ROM aceito pelo registry (`MAX_ROM_FOLDERS`).
pub const MAX_EMULATOR_ROM_FOLDERS: usize = 32;

/// Executável do perfil: caminho/nome não vazio. Nada é executado aqui.
pub fn valid_emulator_executable(executable: &str) -> bool {
    !executable.trim().is_empty()
}

/// Divide os argumentos por espaços, sem interpretar aspas ou shell, e limita
/// a [`MAX_EMULATOR_ARGS`] entradas.
pub fn parse_emulator_args(text: &str) -> Vec<String> {
    text.split_whitespace()
        .take(MAX_EMULATOR_ARGS)
        .map(str::to_string)
        .collect()
}

/// Argumentos do editor: se o texto não mudou em relação ao perfil salvo,
/// devolve a lista original intacta (um argv existente pode conter espaços).
pub fn emulator_args_from_text(text: &str, existing: Option<&EmulatorProfile>) -> Vec<String> {
    match existing {
        Some(profile) if text == profile.args.join(" ") => profile.args.clone(),
        _ => parse_emulator_args(text),
    }
}

/// Campos editáveis de um perfil; `None` em core/bios significa "preserve o
/// valor do perfil existente" (campo fora do formulário).
pub struct EmulatorProfileDraft<'a> {
    pub id: &'a str,
    pub executable: &'a str,
    pub core_path: Option<&'a str>,
    pub bios_path: Option<&'a str>,
    pub rom_folders: &'a [EmulatorRomFolder],
    pub args: &'a [String],
    pub existing: Option<&'a EmulatorProfile>,
}

/// Payload de `emulators:profile:set` preservando core/bios fora do formulário.
pub fn emulator_profile_payload(draft: &EmulatorProfileDraft<'_>) -> Value {
    let preserved_core = draft
        .existing
        .map(|profile| profile.core_path.as_str())
        .unwrap_or("");
    let preserved_bios = draft
        .existing
        .map(|profile| profile.bios_path.as_str())
        .unwrap_or("");
    json!({
        "id": draft.id,
        "executable": draft.executable.trim(),
        "corePath": draft.core_path.unwrap_or(preserved_core),
        "biosPath": draft.bios_path.unwrap_or(preserved_bios),
        "romFolders": draft.rom_folders,
        "args": draft.args,
    })
}

// ── Atualizações ───────────────────────────────────────────────────────────

/// Fase do updater empacotado (`update:packaged:state.fase`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PackagedPhase {
    #[default]
    Ocioso,
    Disponivel,
    Baixando,
    Pronto,
    Erro,
    SemSuporte,
}

impl PackagedPhase {
    pub fn from_str(raw: &str) -> Option<Self> {
        match raw.trim() {
            "ocioso" => Some(Self::Ocioso),
            "disponivel" => Some(Self::Disponivel),
            "baixando" => Some(Self::Baixando),
            "pronto" => Some(Self::Pronto),
            "erro" => Some(Self::Erro),
            "sem_suporte" => Some(Self::SemSuporte),
            _ => None,
        }
    }

    pub fn is_busy(self) -> bool {
        matches!(self, Self::Baixando)
    }
}

/// Snapshot de `update:packaged:state` no formato camelCase do IPC.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackagedState {
    pub canal: String,
    pub suportado: bool,
    pub versao_atual: String,
    pub versao_nova: Option<String>,
    pub tamanho: Option<u64>,
    pub fase: PackagedPhase,
    pub progresso: u8,
    pub erro: Option<String>,
    pub erro_de_fundo: bool,
    pub erro_acao: Option<String>,
    pub ja_avisado: bool,
    pub jogo_rodando: bool,
}

impl PackagedState {
    /// Versão oferecida (nova quando existe, senão a atual).
    pub fn versao_oferecida(&self) -> &str {
        self.versao_nova
            .as_deref()
            .filter(|versao| !versao.is_empty())
            .unwrap_or(&self.versao_atual)
    }

    pub fn is_unsupported(&self) -> bool {
        !self.suportado || self.fase == PackagedPhase::SemSuporte
    }
}

/// Resumo de `update:state` (canal fonte/git e disponibilidade).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitUpdateState {
    pub canal: String,
    pub suportado: bool,
    pub versao: String,
    pub disponivel: bool,
}

/// Estado de update que o `App` mantém em memória.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateSnapshot {
    pub packaged: Option<PackagedState>,
    pub git: Option<GitUpdateState>,
    /// Falha parcial de um dos canais; o outro segue utilizável.
    pub error: Option<String>,
}

impl UpdateSnapshot {
    pub fn channel(&self) -> Option<&str> {
        self.packaged
            .as_ref()
            .map(|state| state.canal.as_str())
            .or_else(|| self.git.as_ref().map(|state| state.canal.as_str()))
            .filter(|canal| !canal.is_empty())
    }

    pub fn is_supported(&self) -> bool {
        self.packaged.as_ref().map(|state| state.suportado).unwrap_or_else(|| {
            self.git.as_ref().map(|state| state.suportado).unwrap_or(false)
        })
    }
}

/// Um commit do changelog do canal fonte.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateCommit {
    pub sha: String,
    pub titulo: String,
}

/// Resultado de `update:check` / `update:packaged:check`.
///
/// Os dois canais têm campos em comum; os específicos do git (`atrasado`,
/// `commits`, `depsMudaram`) ficam vazios no canal empacotado.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateCheck {
    pub ok: bool,
    pub disponivel: bool,
    pub atrasado: Option<i64>,
    pub commits: Vec<UpdateCommit>,
    pub deps_mudaram: bool,
    pub erro: Option<String>,
    pub motivo: Option<String>,
    pub versao: Option<String>,
    pub tamanho: Option<u64>,
}

/// Resultado de `update:packaged:download|install` e `update:apply`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateActionResult {
    pub ok: bool,
    pub erro: Option<String>,
}

/// Retrato devolvido pela checagem manual/automática do `App`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCheckReport {
    pub channel: String,
    pub check: UpdateCheck,
    pub snapshot: UpdateSnapshot,
}

/// Vídeo de boot devolvido por `boot:video`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootVideo {
    pub mime: String,
    pub data: Vec<u8>,
    pub codecs: Vec<String>,
}

/// Payload de `achievement:unlocked` já validado para o toast.
#[derive(Debug, Clone, PartialEq)]
pub struct AchievementToast {
    pub appid: String,
    pub key: String,
    pub title: String,
    pub desc: Option<String>,
    pub icon: Option<String>,
    pub percent: Option<f64>,
    pub unlock: Option<i64>,
}

/// Parser de `update:packaged:state` (snapshot camelCase).
pub fn parse_packaged_state(value: &Value) -> Result<PackagedState, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "update:packaged:state não devolveu um objeto".to_string())?;
    let canal = object
        .get("canal")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|canal| !canal.is_empty())
        .ok_or_else(|| "update:packaged:state sem `canal`".to_string())?;
    let fase_raw = object
        .get("fase")
        .and_then(Value::as_str)
        .ok_or_else(|| "update:packaged:state sem `fase`".to_string())?;
    let fase = PackagedPhase::from_str(fase_raw)
        .ok_or_else(|| format!("fase de update desconhecida: {fase_raw}"))?;

    let progresso = object
        .get("progresso")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 100.0)
        .round() as u8;

    Ok(PackagedState {
        canal: canal.to_string(),
        suportado: bool_field(value, "suportado", false),
        versao_atual: string_field(value, "versaoAtual"),
        versao_nova: optional_text_field(value, "versaoNova"),
        tamanho: object.get("tamanho").and_then(value_to_u64),
        fase,
        progresso,
        erro: optional_text_field(value, "erro"),
        erro_de_fundo: bool_field(value, "erroDeFundo", false),
        erro_acao: optional_text_field(value, "erroAcao"),
        ja_avisado: bool_field(value, "jaAvisado", false),
        jogo_rodando: bool_field(value, "jogoRodando", false),
    })
}

/// Parser de `update:state` (`{ canal, suportado, versao, disponivel }`).
pub fn parse_update_state(value: &Value) -> Result<GitUpdateState, String> {
    if !value.is_object() {
        return Err("update:state não devolveu um objeto".to_string());
    }
    Ok(GitUpdateState {
        canal: string_field(value, "canal"),
        suportado: bool_field(value, "suportado", false),
        versao: string_field(value, "versao"),
        disponivel: bool_field(value, "disponivel", false),
    })
}

/// Parser comum de `update:check` e `update:packaged:check`.
pub fn parse_update_check(value: &Value) -> Result<UpdateCheck, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "checagem de update não devolveu um objeto".to_string())?;

    let atrasado = object.get("atrasado").and_then(Value::as_i64);
    let commits = object
        .get("commits")
        .and_then(Value::as_array)
        .map(|commits| {
            commits
                .iter()
                .filter_map(|commit| {
                    let sha = commit
                        .get("sha")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|sha| !sha.is_empty())?;
                    let titulo = commit
                        .get("titulo")
                        .or_else(|| commit.get("title"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or_default();
                    Some(UpdateCommit {
                        sha: sha.to_string(),
                        titulo: titulo.to_string(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let erro = optional_text_field(value, "erro").or_else(|| optional_text_field(value, "error"));
    let disponivel = object
        .get("disponivel")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| atrasado.unwrap_or(0) > 0 || !commits.is_empty());

    Ok(UpdateCheck {
        ok: bool_field(value, "ok", erro.is_none()),
        disponivel,
        atrasado,
        commits,
        deps_mudaram: bool_field(value, "depsMudaram", false),
        erro,
        motivo: optional_text_field(value, "motivo"),
        versao: optional_text_field(value, "versao"),
        tamanho: object.get("tamanho").and_then(value_to_u64),
    })
}

/// Parser de `{ ok, erro? }` das ações do updater.
pub fn parse_update_action(value: &Value) -> Result<UpdateActionResult, String> {
    if !value.is_object() {
        return Err("ação do updater não devolveu um objeto".to_string());
    }
    Ok(UpdateActionResult {
        ok: bool_field(value, "ok", false),
        erro: optional_text_field(value, "erro")
            .or_else(|| optional_text_field(value, "error"))
            .or_else(|| optional_text_field(value, "motivo")),
    })
}

/// Parser de `boot:video`.
///
/// `{ ok: false }` é o contrato de "sem vídeo" e vira `Ok(None)`; bytes
/// inválidos/vazios também caem no fallback da UI em vez de virar erro fatal.
pub fn parse_boot_video(value: &Value) -> Result<Option<BootVideo>, String> {
    if !bool_field(value, "ok", false) {
        return Ok(None);
    }
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "boot:video ok sem `data`".to_string())?;

    let mut bytes = Vec::with_capacity(data.len());
    for byte in data {
        let number = byte
            .as_u64()
            .filter(|number| *number <= u8::MAX as u64)
            .ok_or_else(|| "boot:video com byte inválido".to_string())?;
        bytes.push(number as u8);
    }
    if bytes.is_empty() {
        return Ok(None);
    }

    Ok(Some(BootVideo {
        mime: optional_text_field(value, "mime")
            .unwrap_or_else(|| "video/mp4".to_string()),
        data: bytes,
        codecs: string_list(value.get("codecs")),
    }))
}

/// Parser de `achievement:unlocked`.
pub fn parse_achievement_unlocked(payload: &Value) -> Option<AchievementToast> {
    let appid = optional_text_field(payload, "appid")?;
    let title = optional_text_field(payload, "title")
        .or_else(|| optional_text_field(payload, "name"))?;
    let key = optional_text_field(payload, "key")
        .or_else(|| optional_text_field(payload, "apiname"))
        .unwrap_or_else(|| title.clone());

    Some(AchievementToast {
        appid,
        key,
        title,
        desc: optional_text_field(payload, "desc")
            .or_else(|| optional_text_field(payload, "description")),
        icon: optional_image_url(payload, "icon"),
        percent: payload.get("percent").and_then(Value::as_f64),
        unlock: payload.get("unlock").and_then(Value::as_i64),
    })
}

/// Evento já convertido que o `App` aplica na UI.
#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    Launch(LaunchEvent),
    Achievement(AchievementToast),
}

/// Fachada compartilhada pelas tasks do iced.
#[derive(Clone)]
pub struct Backend {
    state: Arc<AppState>,
    shell: Arc<NativeShell>,
    runtime: tokio::runtime::Handle,
    http: reqwest::Client,
    achievements_warmed: Arc<AtomicBool>,
    achievements_running: Arc<AtomicBool>,
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

        let shell = Arc::new(NativeShell::with_data_dir(state.data_dir.clone()));
        // O launch roda no `LaunchRuntime` do IPC e publica pelo sink de
        // background, não pelo `Shell::emit`. Os dois caminhos alimentam a
        // mesma fila; o app drena numa Subscription de ~150 ms.
        let events = shell.events();
        state.set_event_sink(Arc::new(move |event: &str, payload: Value| {
            events.push(event, payload);
        }));

        Self {
            state: Arc::new(state),
            shell,
            runtime,
            http,
            achievements_warmed: Arc::new(AtomicBool::new(false)),
            achievements_running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Drena a fila compartilhada já convertida em eventos de UI.
    ///
    /// Eventos de conquista passam pelo mesmo parser do toast; um payload
    /// desconhecido é registrado e descartado sem derrubar os demais.
    pub fn drain_ui_events(&self) -> Vec<UiEvent> {
        let queue = self.shell.events();
        if queue.is_empty() {
            return Vec::new();
        }
        queue
            .drain()
            .into_iter()
            .filter_map(|event| {
                if event.event == "achievement:unlocked" {
                    let parsed = parse_achievement_unlocked(&event.payload);
                    if parsed.is_none() {
                        eprintln!(
                            "[arcadia-native] achievement:unlocked sem payload reconhecido: {}",
                            event.payload
                        );
                    }
                    return parsed.map(UiEvent::Achievement);
                }

                let parsed = parse_launch_event(&event.event, &event.payload);
                if parsed.is_none() {
                    eprintln!(
                        "[arcadia-native] evento de UI sem payload reconhecido: {} {}",
                        event.event, event.payload
                    );
                }
                parsed.map(UiEvent::Launch)
            })
            .collect()
    }

    pub fn data_dir(&self) -> &Path {
        &self.state.data_dir
    }

    /// Snapshot `update:packaged:state` + `update:state` em paralelo.
    ///
    /// Uma falha parcial não apaga o outro canal; só vira erro quando nenhum
    /// dos dois respondeu.
    pub async fn load_update_snapshot(&self) -> Result<UpdateSnapshot, String> {
        let (packaged, git) = tokio::join!(
            self.dispatch("update:packaged:state", Vec::new()),
            self.dispatch("update:state", Vec::new())
        );

        let mut errors = Vec::new();
        let packaged = match packaged {
            Ok(value) => match parse_packaged_state(&value) {
                Ok(state) => Some(state),
                Err(error) => {
                    errors.push(format!("update:packaged:state: {error}"));
                    None
                }
            },
            Err(error) => {
                errors.push(format!("update:packaged:state: {error}"));
                None
            }
        };
        let git = match git {
            Ok(value) => match parse_update_state(&value) {
                Ok(state) => Some(state),
                Err(error) => {
                    errors.push(format!("update:state: {error}"));
                    None
                }
            },
            Err(error) => {
                errors.push(format!("update:state: {error}"));
                None
            }
        };

        if packaged.is_none() && git.is_none() {
            return Err(errors.join(" · "));
        }

        Ok(UpdateSnapshot {
            packaged,
            git,
            error: (!errors.is_empty()).then(|| errors.join(" · ")),
        })
    }

    /// Estado empacotado isolado, usado pelo poll do diálogo em download.
    pub async fn load_packaged_state(&self) -> Result<PackagedState, String> {
        let value = self.dispatch("update:packaged:state", Vec::new()).await?;
        parse_packaged_state(&value)
    }

    /// Checagem manual/automática: escolhe o canal informado por `update:state`
    /// e devolve o snapshot atualizado junto do resultado.
    pub async fn check_updates(&self, manual: bool) -> Result<UpdateCheckReport, String> {
        let snapshot = self
            .load_update_snapshot()
            .await
            .unwrap_or_else(|error| UpdateSnapshot {
                error: Some(error),
                ..UpdateSnapshot::default()
            });
        let channel = snapshot
            .channel()
            .unwrap_or("sem_suporte")
            .to_string();

        let value = if channel == "fonte" {
            self.dispatch("update:check", Vec::new()).await?
        } else {
            self.dispatch(
                "update:packaged:check",
                vec![json!({ "manual": manual })],
            )
            .await?
        };
        let check = parse_update_check(&value)?;

        let packaged = self.load_packaged_state().await.ok();
        let refreshed = UpdateSnapshot {
            packaged,
            git: snapshot.git,
            error: snapshot.error,
        };

        Ok(UpdateCheckReport {
            channel,
            check,
            snapshot: refreshed,
        })
    }

    /// `update:packaged:download` — só deve ser chamado por ação do usuário.
    pub async fn download_packaged_update(&self) -> Result<UpdateActionResult, String> {
        let value = self
            .dispatch("update:packaged:download", Vec::new())
            .await?;
        parse_update_action(&value)
    }

    /// `update:packaged:install` — encerra/reinicia o app; nunca automático.
    pub async fn install_packaged_update(&self) -> Result<UpdateActionResult, String> {
        let value = self
            .dispatch("update:packaged:install", Vec::new())
            .await?;
        parse_update_action(&value)
    }

    /// `update:apply { depsMudaram }` do canal fonte/git.
    pub async fn apply_git_update(&self, deps_mudaram: bool) -> Result<UpdateActionResult, String> {
        let value = self
            .dispatch(
                "update:apply",
                vec![json!({ "depsMudaram": deps_mudaram })],
            )
            .await?;
        parse_update_action(&value)
    }

    /// Persiste que a versão já foi mostrada (`update:packaged:jaAvisado`).
    pub async fn mark_update_advised(&self, versao: String) -> Result<(), String> {
        let value = self
            .dispatch(
                "update:packaged:jaAvisado",
                vec![json!({ "versao": versao })],
            )
            .await?;
        let result = parse_update_action(&value)?;
        if result.ok {
            Ok(())
        } else {
            Err(result
                .erro
                .unwrap_or_else(|| "update:packaged:jaAvisado falhou".to_string()))
        }
    }

    /// Pede `boot:video` e grava os bytes num arquivo temporário do data dir.
    ///
    /// A gravação roda dentro da task do iced (fora da thread de pintura) e o
    /// `MediaPlayer` abre o caminho local depois. Sem vídeo, devolve `Ok(None)`.
    pub async fn load_boot_video(&self) -> Result<Option<PathBuf>, String> {
        let value = self.dispatch("boot:video", Vec::new()).await?;
        let Some(video) = parse_boot_video(&value)? else {
            return Ok(None);
        };

        let dir = self.data_dir().join("temp");
        std::fs::create_dir_all(&dir)
            .map_err(|error| format!("não foi possível criar {}: {error}", dir.display()))?;
        let extension = match video.mime.as_str() {
            "video/webm" => "webm",
            "video/quicktime" => "mov",
            _ => "mp4",
        };
        let path = dir.join(format!("arcadia-boot-{}.{}", std::process::id(), extension));
        std::fs::write(&path, &video.data)
            .map_err(|error| format!("não foi possível gravar {}: {error}", path.display()))?;
        Ok(Some(path))
    }

    /// Aquece os schemas na primeira chamada e roda o tick de conquistas.
    ///
    /// O `App` chama isso num tick de 15 s, como o loop do shell Tauri. Roda em
    /// `spawn_blocking` porque o runtime de conquistas lê arquivos do Steam.
    pub async fn achievements_tick(&self) {
        if self.achievements_running.swap(true, Ordering::SeqCst) {
            return;
        }
        let state = Arc::clone(&self.state);
        let warm = !self.achievements_warmed.swap(true, Ordering::SeqCst);
        let join = self.runtime.spawn_blocking(move || {
            if warm {
                let _ = state.achievements_schemas_load();
            }
            let _ = state.achievements_tick();
        });
        let _ = join.await;
        self.achievements_running.store(false, Ordering::SeqCst);
    }

    /// Prefixo que o `LaunchRuntime::build_environment` usa quando o jogo não
    /// define `prefixPath` (mesma sanitização de lá). O `defaultPrefix` de
    /// `gamesettings:get` é o das ferramentas Wine da UI web e pode diferir.
    pub fn native_default_prefix(&self, id: &str) -> String {
        let safe = id
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>();
        self.data_dir()
            .join("prefixes")
            .join(if safe.is_empty() {
                "game".to_string()
            } else {
                safe
            })
            .to_string_lossy()
            .into_owned()
    }

    pub async fn load_library(&self) -> Result<Vec<Game>, String> {
        let value = self.dispatch("library:get", Vec::new()).await?;
        parse_library(value)
    }

    /// Feed agregado da aba Notícias (`news:get`).
    pub async fn load_news(&self) -> Result<Vec<NewsItem>, String> {
        let value = self.dispatch("news:get", Vec::new()).await?;
        parse_news(value)
    }

    /// Marca/desmarca o favorito e devolve a biblioteca reordenada pelo
    /// dispatcher (`overrides:set` responde a lista inteira).
    pub async fn set_favorite(&self, id: String, favorite: bool) -> Result<Vec<Game>, String> {
        let patch = if favorite {
            json!({ "favorite": true })
        } else {
            json!({ "favorite": null })
        };
        let payload = json!({ "id": id, "patch": patch });
        let value = self.dispatch("overrides:set", vec![payload]).await?;
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
                notice: Some(format!("Falha ao consultar a loja (game:sysinfo): {error}")),
                ..GameMedia::default()
            },
        }
    }

    /// Conquistas Steam do appid (`achievements:get`).
    pub async fn load_steam_achievements(
        &self,
        appid: String,
    ) -> Result<AchievementsData, String> {
        let value = self
            .dispatch("achievements:get", vec![Value::String(appid)])
            .await?;
        parse_steam_achievements(value)
    }

    /// Conquistas RetroAchievements do título/sistema
    /// (`retroachievements:gameProgress`).
    pub async fn load_retro_achievements(
        &self,
        title: String,
        system: String,
    ) -> Result<AchievementsData, String> {
        let value = self
            .dispatch(
                "retroachievements:gameProgress",
                vec![json!({ "title": title, "systemId": system })],
            )
            .await?;
        parse_retro_achievements(value)
    }

    /// Resumo do ProtonDB (`game:protondb`); `Ok(None)` = sem relatórios.
    pub async fn load_protondb(&self, appid: String) -> Result<Option<ProtonDbInfo>, String> {
        let value = self
            .dispatch("game:protondb", vec![Value::String(appid.clone())])
            .await?;
        parse_protondb(value, &appid)
    }

    /// Owners/CCU e reviews de `game:stats`; `Ok(None)` = sem dados.
    pub async fn load_game_stats(&self, appid: String) -> Result<Option<GameStats>, String> {
        let value = self
            .dispatch("game:stats", vec![Value::String(appid)])
            .await?;
        parse_game_stats(value)
    }

    /// Busca vídeos do YouTube (`trailer:search`) para o seletor manual.
    pub async fn search_trailers(&self, query: String) -> Result<Vec<TrailerResult>, String> {
        let value = self
            .dispatch("trailer:search", vec![json!({ "query": query })])
            .await?;
        parse_trailer_search(value)
    }

    /// Baixa um vídeo específico como trailer do jogo (`trailer:downloadUrl`).
    ///
    /// A chamada é longa (yt-dlp + remux), mas roda no runtime do backend; a UI
    /// continua desenhando e recebe o caminho em `TrailerDownloadDone`.
    pub async fn download_trailer(&self, id: String, url: String) -> Result<String, String> {
        let value = self
            .dispatch("trailer:downloadUrl", vec![json!({ "id": id, "url": url })])
            .await?;
        parse_trailer_download(value)
    }

    /// Trailer já baixado do jogo (`trailer:path`), ou `None` se não existir.
    pub async fn trailer_path(&self, id: String) -> Result<Option<String>, String> {
        let value = self
            .dispatch("trailer:path", vec![Value::String(id)])
            .await?;
        Ok(parse_trailer_path(&value))
    }

    /// Inicia um jogo pelo mesmo canal do Electron (`game:launch`).
    ///
    /// O payload é o do [`launch_payload`]; `mode` é o ponto de extensão para o
    /// seletor Steam/exe da UI web, que o nativo ainda não oferece. Erros do
    /// dispatcher (`ok:false`) e do spawn do processo chegam como `Err` para a
    /// UI mostrar na ficha, não só no stderr.
    pub async fn launch_game(&self, game: &Game, mode: Option<&str>) -> Result<(), String> {
        let value = self
            .dispatch("game:launch", vec![launch_payload(game, mode)])
            .await?;
        check_game_launch(&value)
    }

    /// `game:close` — encerra o processo (e a árvore) do jogo atual.
    pub async fn close_game(&self) -> Result<(), String> {
        let value = self.dispatch("game:close", Vec::new()).await?;
        check_envelope(&value, "game:close")
    }

    /// `gamesettings:get` — settings persistidos do jogo + prefixo padrão.
    pub async fn load_game_settings(&self, id: String) -> Result<GameSettingsEnvelope, String> {
        let value = self
            .dispatch("gamesettings:get", vec![Value::String(id)])
            .await?;
        Ok(parse_game_settings_envelope(&value))
    }

    /// `gamesettings:set` — grava o patch e devolve o objeto mesclado.
    ///
    /// O retorno do canal é o próprio entry (sem envelope), então só checamos
    /// `ok:false` por segurança e reinterpretamos como settings.
    pub async fn save_game_settings(
        &self,
        id: String,
        patch: Value,
    ) -> Result<GameSettingsView, String> {
        let value = self
            .dispatch("gamesettings:set", vec![json!({ "id": id, "patch": patch })])
            .await?;
        check_envelope(&value, "gamesettings:set")?;
        Ok(parse_game_settings(&value))
    }

    /// `emulators:list` + `emulators:status` para o seletor da ficha.
    ///
    /// Falha de status não derruba o catálogo; o seletor continua utilizável e
    /// a validação de BIOS apenas fica sem aviso.
    pub async fn load_game_emulator_options(&self) -> Result<GameEmulatorOptions, String> {
        let (catalog, statuses) = tokio::join!(
            self.dispatch("emulators:list", Vec::new()),
            self.dispatch("emulators:status", Vec::new())
        );
        let emulators = parse_emulators_list(catalog?)?;
        let statuses = statuses
            .and_then(parse_emulators_status)
            .unwrap_or_default();
        Ok(GameEmulatorOptions {
            emulators,
            statuses,
        })
    }

    /// `emulators:resolve` — valida emulador/ROM/core antes de salvar ou
    /// importar. O envelope `ok:false` volta como resultado, não como `Err`.
    pub async fn resolve_emulator_launch(
        &self,
        payload: Value,
    ) -> Result<EmulatorResolveResult, String> {
        let value = self.dispatch("emulators:resolve", vec![payload]).await?;
        Ok(parse_emulator_resolve(&value))
    }

    /// Importa uma ROM como `custom:<slug>` e grava os settings que o
    /// `plan_launch` consome. Para no primeiro erro real do contrato.
    pub async fn import_emulator_rom(
        &self,
        request: EmulatorRomImport,
    ) -> Result<ImportedEmulatorRom, String> {
        let resolved = self
            .resolve_emulator_launch(request.resolve_payload())
            .await?;
        if !resolved.ok {
            return Err(resolved.error_text());
        }

        let id = request.game_id();
        let title = request.title();
        let added = self
            .dispatch("customgame:add", vec![request.custom_game_payload()])
            .await?;
        check_envelope(&added, "customgame:add")?;

        let saved = self
            .dispatch(
                "gamesettings:set",
                vec![json!({ "id": id, "patch": request.settings_patch() })],
            )
            .await?;
        if let Err(error) = check_envelope(&saved, "gamesettings:set") {
            return Err(format!(
                "o jogo {id} foi criado, mas os settings do emulador falharam: {error}"
            ));
        }

        let games = self.refresh_library().await?;
        Ok(ImportedEmulatorRom { title, games })
    }

    /// `library:refresh` — relê a biblioteca depois de criar um jogo custom.
    pub async fn refresh_library(&self) -> Result<Vec<Game>, String> {
        let value = self.dispatch("library:refresh", Vec::new()).await?;
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

    /// Carrega catálogo, status, perfis e índice de ROMs em paralelo.
    ///
    /// `detect` força `emulators:detect`; falhas parciais de status/índice/
    /// perfis viram avisos para a tela não perder o catálogo já disponível.
    pub async fn load_emulation(&self, detect: bool) -> Result<EmulationLoad, String> {
        let channel = if detect {
            "emulators:detect"
        } else {
            "emulators:list"
        };
        let (catalog, statuses, index, profiles) = tokio::join!(
            self.dispatch(channel, Vec::new()),
            self.dispatch("emulators:status", Vec::new()),
            self.dispatch("emulators:roms:index", Vec::new()),
            self.dispatch("emulators:profiles", Vec::new())
        );

        let mut emulators = parse_emulators_list(catalog?)?;
        let mut warnings = Vec::new();

        let statuses = match statuses.and_then(parse_emulators_status) {
            Ok(statuses) => statuses,
            Err(error) => {
                warnings.push(format!("emulators:status: {error}"));
                Vec::new()
            }
        };
        let rom_index = match index.and_then(parse_emulators_rom_index) {
            Ok(index) => index,
            Err(error) => {
                warnings.push(format!("emulators:roms:index: {error}"));
                Default::default()
            }
        };
        let profiles = match profiles.and_then(parse_emulators_profiles) {
            Ok(profiles) => profiles,
            Err(error) => {
                warnings.push(format!("emulators:profiles: {error}"));
                Default::default()
            }
        };

        // `emulators:list` já inclui o perfil; o mapa de profiles cobre uma
        // resposta antiga/sem o campo sem exigir outro round-trip na view.
        for emulator in &mut emulators {
            if emulator.profile.is_none() {
                emulator.profile = profiles.get(&emulator.id).cloned();
            }
        }

        Ok(EmulationLoad {
            emulators,
            statuses,
            rom_index,
            warnings,
        })
    }

    pub async fn save_emulator_profile(&self, payload: Value) -> Result<(), String> {
        let value = self
            .dispatch("emulators:profile:set", vec![payload])
            .await?;
        check_envelope(&value, "emulators:profile:set")
    }

    pub async fn remove_emulator_profile(&self, id: String) -> Result<(), String> {
        let value = self
            .dispatch("emulators:profile:remove", vec![Value::String(id)])
            .await?;
        check_envelope(&value, "emulators:profile:remove")
    }

    /// `emulators:roms` com as pastas persistidas do perfil (sem diretório
    /// avulso), limite de 256 resultados e recursão ligada.
    pub async fn scan_configured_emulator_roms(
        &self,
        emulator_id: String,
    ) -> Result<EmulatorScanResult, String> {
        let payload = json!({
            "emulatorId": emulator_id,
            "recursive": true,
            "maxResults": 256,
        });
        let value = self.dispatch("emulators:roms", vec![payload]).await?;
        parse_emulator_roms(value)
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

    /// `friends:search` — busca por prefixo de username na sessão atual.
    pub async fn search_friends(&self, query: String) -> Result<Vec<FriendSearchResult>, String> {
        let value = self
            .dispatch("friends:search", vec![Value::String(query)])
            .await?;
        parse_friends_search(value)
    }

    /// Envia/aceita/cancela/remove uma relação de amizade.
    pub async fn run_friend_action(&self, channel: &str, user_id: String) -> Result<(), String> {
        let value = self.dispatch(channel, vec![Value::String(user_id)]).await?;
        parse_friends_action(value)
    }

    /// `friends:profile` — RPC pública usada pelo cartão do amigo.
    pub async fn load_friend_profile(&self, user_id: String) -> Result<FriendProfile, String> {
        let value = self
            .dispatch("friends:profile", vec![Value::String(user_id)])
            .await?;
        parse_friend_profile(value)
    }

    /// `friends:achievements` — só deve ser chamado para amigo aceito.
    pub async fn load_friend_achievements(
        &self,
        user_id: String,
    ) -> Result<FriendAchievements, String> {
        let value = self
            .dispatch("friends:achievements", vec![Value::String(user_id)])
            .await?;
        parse_friend_achievements(value)
    }

    /// Leitura de `account:status`; `Ok(None)` é sessão ausente (não é erro).
    pub async fn load_account_status(&self) -> Result<Option<AccountSession>, String> {
        let value = self.dispatch("account:status", Vec::new()).await?;
        parse_account_status(value)
    }

    /// Login por username/senha validado no dispatcher.
    pub async fn sign_in(&self, username: String, password: String) -> Result<AuthSuccess, String> {
        let value = self
            .dispatch(
                "account:signIn",
                vec![json!({ "username": username, "password": password })],
            )
            .await?;
        parse_auth_result(value)
    }

    /// Cadastro com email/username/senha.
    pub async fn sign_up(
        &self,
        email: String,
        username: String,
        password: String,
    ) -> Result<AuthSuccess, String> {
        let value = self
            .dispatch(
                "account:signUp",
                vec![json!({ "email": email, "username": username, "password": password })],
            )
            .await?;
        parse_auth_result(value)
    }

    pub async fn sign_out(&self) -> Result<(), String> {
        let value = self.dispatch("account:signOut", Vec::new()).await?;
        parse_account_action(value)
    }

    pub async fn load_account_profile(&self) -> Result<AccountProfile, String> {
        let value = self.dispatch("account:profile", Vec::new()).await?;
        parse_account_profile(value)
    }

    /// Envia somente os campos aceitos por `account:updateProfile`.
    pub async fn update_account_profile(&self, patch: Value) -> Result<(), String> {
        let value = self.dispatch("account:updateProfile", vec![patch]).await?;
        parse_account_action(value)
    }

    /// Envia o caminho de arquivo para `account:setAvatar` e devolve a URL.
    pub async fn set_account_avatar(&self, path: String) -> Result<String, String> {
        let value = self
            .dispatch("account:setAvatar", vec![Value::String(path)])
            .await?;
        parse_avatar_upload(value)
    }

    /// Envia fundo/banner de um caminho local e devolve a URL pública.
    pub async fn set_account_background(&self, path: String, kind: &str) -> Result<String, String> {
        let value = self
            .dispatch(
                "account:setBackground",
                vec![Value::String(path), Value::String(kind.to_string())],
            )
            .await?;
        parse_background_upload(value)
    }

    /// Abre o seletor nativo. `None` significa que o usuário cancelou.
    pub async fn pick_path(&self, purpose: PickPurpose) -> Result<Option<PathBuf>, String> {
        crate::picker::pick(purpose).await
    }

    /// `avatar:load` baixa a URL http(s) e devolve o Handle pronto para o iced.
    pub async fn load_avatar(&self, url: String) -> Result<Handle, String> {
        let value = self
            .dispatch("avatar:load", vec![Value::String(url)])
            .await?;
        if value.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err(value
                .get("error")
                .and_then(Value::as_str)
                .filter(|error| !error.trim().is_empty())
                .unwrap_or("avatar:load não retornou a imagem")
                .to_string());
        }
        let src = value
            .get("src")
            .and_then(Value::as_str)
            .ok_or_else(|| "avatar:load não devolveu src".to_string())?;
        avatar_handle_from_data_url(src)
    }

    pub async fn load_plugins(&self) -> Result<Vec<PluginItem>, String> {
        let value = self.dispatch("plugins:list", Vec::new()).await?;
        parse_plugins(value)
    }

    /// Executa uma mutação do registro e devolve o resultado já normalizado.
    /// A lista não vem no envelope: o chamador recarrega `plugins:list`.
    pub async fn run_plugin_action(
        &self,
        id: String,
        action: PluginAction,
    ) -> Result<PluginActionOutcome, String> {
        let value = self
            .dispatch(action.channel(), vec![Value::String(id)])
            .await?;
        parse_plugin_action(&value, action)
    }

    pub async fn register_plugin(
        &self,
        path: String,
    ) -> Result<PluginRegistration, String> {
        let value = self
            .dispatch("plugins:register", vec![Value::String(path)])
            .await?;
        parse_plugin_register(value)
    }

    pub async fn load_plugin_details(&self, id: String) -> Result<PluginDetails, String> {
        let value = self
            .dispatch("plugins:get", vec![Value::String(id)])
            .await?;
        parse_plugin_details(value)
    }

    pub async fn verify_plugin(&self, id: String) -> Result<PluginVerification, String> {
        let value = self
            .dispatch("plugins:verify", vec![Value::String(id)])
            .await?;
        parse_plugin_verification(value)
    }

    /// Página de reviews públicas de um appid (`community:reviews`).
    pub async fn load_community_reviews(
        &self,
        appid: String,
        offset: u64,
        limit: u64,
    ) -> Result<CommunityReviewsPage, CommunityError> {
        let appid = community_appid(&appid);
        let value = self
            .dispatch(
                "community:reviews",
                vec![Value::String(appid), json!({ "limit": limit, "offset": offset })],
            )
            .await
            .map_err(|message| CommunityError {
                message,
                offline: false,
                code: None,
            })?;
        parse_community_reviews(value)
    }

    /// Página de coleções públicas (`community:collections`); `mine` usa o
    /// contrato existente para listar as coleções da sessão.
    pub async fn load_community_collections(
        &self,
        offset: u64,
        limit: u64,
        mine: bool,
    ) -> Result<CommunityCollectionsPage, CommunityError> {
        let value = self
            .dispatch(
                "community:collections",
                vec![json!({ "limit": limit, "offset": offset, "mine": mine })],
            )
            .await
            .map_err(|message| CommunityError {
                message,
                offline: false,
                code: None,
            })?;
        parse_community_collections(value)
    }

    /// `community:review:create` — payload cru validado pelo chamador.
    pub async fn create_community_review(
        &self,
        payload: Value,
    ) -> Result<CommunityMutation, CommunityError> {
        let value = self
            .dispatch("community:review:create", vec![payload])
            .await
            .map_err(community_dispatch_error)?;
        parse_community_review_mutation(value)
    }

    pub async fn update_community_review(
        &self,
        id: String,
        payload: Value,
    ) -> Result<CommunityMutation, CommunityError> {
        let value = self
            .dispatch("community:review:update", vec![Value::String(id), payload])
            .await
            .map_err(community_dispatch_error)?;
        parse_community_review_mutation(value)
    }

    pub async fn remove_community_review(
        &self,
        id: String,
    ) -> Result<CommunityMutation, CommunityError> {
        let value = self
            .dispatch("community:review:remove", vec![Value::String(id)])
            .await
            .map_err(community_dispatch_error)?;
        parse_community_action(value, "community:review:remove")
    }

    pub async fn report_community_review(
        &self,
        id: String,
        reason: String,
    ) -> Result<CommunityMutation, CommunityError> {
        let value = self
            .dispatch(
                "community:review:report",
                vec![Value::String(id), json!({ "reason": reason })],
            )
            .await
            .map_err(community_dispatch_error)?;
        parse_community_action(value, "community:review:report")
    }

    pub async fn create_community_collection(
        &self,
        payload: Value,
    ) -> Result<CommunityMutation, CommunityError> {
        let value = self
            .dispatch("community:collection:create", vec![payload])
            .await
            .map_err(community_dispatch_error)?;
        parse_community_collection_mutation(value)
    }

    pub async fn update_community_collection(
        &self,
        id: String,
        payload: Value,
    ) -> Result<CommunityMutation, CommunityError> {
        let value = self
            .dispatch(
                "community:collection:update",
                vec![Value::String(id), payload],
            )
            .await
            .map_err(community_dispatch_error)?;
        parse_community_collection_mutation(value)
    }

    pub async fn remove_community_collection(
        &self,
        id: String,
    ) -> Result<CommunityMutation, CommunityError> {
        let value = self
            .dispatch("community:collection:remove", vec![Value::String(id)])
            .await
            .map_err(community_dispatch_error)?;
        parse_community_action(value, "community:collection:remove")
    }

    pub async fn report_community_collection(
        &self,
        id: String,
        reason: String,
    ) -> Result<CommunityMutation, CommunityError> {
        let value = self
            .dispatch(
                "community:collection:report",
                vec![Value::String(id), json!({ "reason": reason })],
            )
            .await
            .map_err(community_dispatch_error)?;
        parse_community_action(value, "community:collection:report")
    }

    pub async fn add_community_collection_item(
        &self,
        id: String,
        appid: String,
    ) -> Result<CommunityMutation, CommunityError> {
        let value = self
            .dispatch(
                "community:collection:item:add",
                vec![Value::String(id), Value::String(appid)],
            )
            .await
            .map_err(community_dispatch_error)?;
        parse_community_action(value, "community:collection:item:add")
    }

    pub async fn remove_community_collection_item(
        &self,
        id: String,
        appid: String,
    ) -> Result<CommunityMutation, CommunityError> {
        let value = self
            .dispatch(
                "community:collection:item:remove",
                vec![Value::String(id), Value::String(appid)],
            )
            .await
            .map_err(community_dispatch_error)?;
        parse_community_action(value, "community:collection:item:remove")
    }

    /// Busca no catálogo da loja (`store:search`), como a aba Lojas da web.
    pub async fn search_store(&self, query: String) -> Result<StoreSearch, String> {
        let value = self
            .dispatch("store:search", vec![Value::String(query)])
            .await?;
        parse_store_search(value)
    }

    /// Sugestões enquanto digita (`store:suggest`).
    pub async fn suggest_store(&self, query: String) -> Result<Vec<StoreGame>, String> {
        let value = self
            .dispatch("store:suggest", vec![Value::String(query)])
            .await?;
        parse_store_suggest(value)
    }

    /// Catálogo navegável paginado (`store:recent` com `lista: "all"`).
    pub async fn load_store_recent(&self, offset: u64, limit: u64) -> Result<StorePage, String> {
        let payload = json!({
            "lista": "all",
            "limite": limit,
            "offset": offset,
        });
        let value = self.dispatch("store:recent", vec![payload]).await?;
        parse_store_recent(value)
    }

    /// Ficha da loja (`game:sysinfo`) com descrição/backdrop do appid.
    pub async fn load_store_info(&self, appid: String, title: String) -> Result<StoreInfo, String> {
        let id = if appid.starts_with("steam:") {
            appid
        } else {
            format!("steam:{appid}")
        };
        let payload = json!({ "id": id, "title": title, "launcher": "steam" });
        let value = self.dispatch("game:sysinfo", vec![payload]).await?;
        parse_store_info(value)
    }

    /// Manifesto de instalação Steam (`store:installInfo`).
    pub async fn load_store_install_info(&self, appid: String) -> Result<StoreInstallInfo, String> {
        let value = self
            .dispatch("store:installInfo", vec![Value::String(appid)])
            .await?;
        parse_store_install_info(value)
    }

    /// Bibliotecas Steam (`store:libraries`) usadas para escolher o `steamDir`.
    pub async fn load_store_libraries(&self) -> Result<Vec<StoreLibrary>, String> {
        let value = self.dispatch("store:libraries", Vec::new()).await?;
        parse_store_libraries(value)
    }

    /// Enfileira a instalação (`store:install`) com um payload já montado por
    /// [`store_install_payload`]. Não baixa nada nesta chamada: o dispatcher
    /// apenas adiciona o item à fila.
    pub async fn install_store_game(&self, payload: Value) -> Result<(), String> {
        let value = self.dispatch("store:install", vec![payload]).await?;
        parse_store_install_response(value)
    }

    /// Página do catálogo Retro (`retro:list`). O filtro usa `system`, a mesma
    /// chave aceita pelo catálogo remoto e pela UI web.
    pub async fn load_retro_page(
        &self,
        query: String,
        system: Option<String>,
        offset: u64,
        limit: u64,
    ) -> Result<RetroPage, String> {
        let mut payload = json!({
            "offset": offset,
            "limit": limit,
            "mode": "all",
            "variants": "all",
        });
        if !query.trim().is_empty() {
            payload["query"] = Value::String(query);
        }
        if let Some(system) = system.filter(|system| !system.trim().is_empty()) {
            // O catálogo remoto lê `system`; o fallback local em disco lê
            // `systems[]`. Mandar as duas formas mantém o filtro nos dois.
            payload["systems"] = json!([system.clone()]);
            payload["system"] = Value::String(system);
        }
        let value = self.dispatch("retro:list", vec![payload]).await?;
        parse_retro_page(value)
    }

    /// Ficha completa do catálogo Retro (`retro:game`).
    pub async fn load_retro_game(&self, id: String) -> Result<RetroGameDetail, String> {
        let value = self.dispatch("retro:game", vec![Value::String(id)]).await?;
        parse_retro_game_detail(value)
    }

    /// URIs de uma oferta Retro (`retro:offer`), somente para exibição.
    pub async fn load_retro_offer(&self, id: String) -> Result<RetroOfferDetail, String> {
        let value = self
            .dispatch("retro:offer", vec![Value::String(id)])
            .await?;
        parse_retro_offer_detail(value)
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
                "launch_cmd": ["steam", "steam://run/10"],
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
            half_life.launch_cmd,
            vec!["steam".to_string(), "steam://run/10".to_string()]
        );
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
    fn parses_news_and_drops_items_without_title_or_url() {
        let news = parse_news(json!([
            {
                "id": "news_1",
                "title": "Jogo novo anunciado",
                "source": "Eurogamer",
                "date": "2026-11-09T10:00:00+00:00",
                "url": "https://example.test/noticia",
                "summary": "Resumo curto",
                "image": "https://example.test/foto.jpg"
            },
            {
                "title": "Sem link"
            },
            {
                "title": "Sem data",
                "url": "https://example.test/sem-data"
            }
        ]))
        .expect("lista de notícias");

        assert_eq!(news.len(), 2);
        assert_eq!(news[0].source, "Eurogamer");
        assert_eq!(news[0].image.as_deref(), Some("https://example.test/foto.jpg"));
        assert!(news[0].date_label().contains('/'));
        assert_eq!(news[1].source, "Feed");
        assert_eq!(news[1].date_label(), "Data não informada");
    }

    #[test]
    fn news_payload_that_is_not_a_list_is_an_error() {
        assert!(parse_news(json!({ "items": [] })).is_err());
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
            launch_cmd: Vec::new(),
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
        assert_eq!(first.hls.as_deref(), Some("https://cdn.test/trailer.m3u8"));
        assert_eq!(first.mp4.as_deref(), Some("https://cdn.test/max.mp4"));
        assert_eq!(first.thumb.as_deref(), Some("https://cdn.test/thumb.jpg"));

        let second = &media.movies[1];
        assert_eq!(second.name, "WebM");
        assert_eq!(second.webm.as_deref(), Some("https://cdn.test/trailer.webm"));
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

    #[test]
    fn game_media_keeps_screenshots_and_about_html() {
        let media = parse_game_media(json!({
            "ok": true,
            "info": {
                "about": "<h2>Sobre</h2><p>Texto <b>rico</b>.</p>",
                "screenshots": [
                    { "thumb": "//cdn.test/a_thumb.jpg", "full": "https://cdn.test/a.jpg" },
                    { "path_thumbnail": "https://cdn.test/b_thumb.jpg" },
                    { "full": "" },
                    { "thumb": "" }
                ],
                "movies": []
            }
        }));

        assert_eq!(media.screenshots.len(), 2);
        assert_eq!(
            media.screenshots[0].thumb.as_deref(),
            Some("https://cdn.test/a_thumb.jpg")
        );
        assert_eq!(
            media.screenshots[0].full.as_deref(),
            Some("https://cdn.test/a.jpg")
        );
        assert_eq!(
            media.screenshots[1].thumb.as_deref(),
            Some("https://cdn.test/b_thumb.jpg")
        );
        assert_eq!(media.screenshots[1].full_or_thumb(), Some("https://cdn.test/b_thumb.jpg"));
        assert!(media.about.as_deref().unwrap_or_default().contains("<b>rico</b>"));
        // Sem trailer continua sendo um estado explícito, mas a galeria ainda
        // tem dados para mostrar.
        assert!(media.notice.is_some());
    }

    #[test]
    fn parses_steam_achievements_with_partial_and_unlocked_items() {
        let data = parse_steam_achievements(json!([
            {
                "title": "Primeira",
                "desc": "Abra a porta",
                "icon": "https://cdn.test/on.png",
                "icongray": "https://cdn.test/off.png",
                "achieved": true,
                "percent": 42.6
            },
            {
                "apiname": "ACH_2",
                "title": "Segunda",
                "icongray": "//cdn.test/gray.png",
                "achieved": false
            },
            { "apiname": "ACH_SEM_TITULO", "achieved": false },
            { "desc": "sem título" },
            { "title": "", "desc": "vazio" }
        ]))
        .expect("lista válida");

        assert_eq!(data.items.len(), 3);
        assert_eq!(data.items[0].title, "Primeira");
        assert!(data.items[0].unlocked);
        assert_eq!(data.items[0].percent, Some(43));
        assert_eq!(
            data.items[0].icon.as_deref(),
            Some("https://cdn.test/on.png")
        );
        assert_eq!(data.items[1].title, "Segunda");
        assert!(!data.items[1].unlocked);
        assert_eq!(
            data.items[1].icon.as_deref(),
            Some("https://cdn.test/gray.png")
        );
        assert_eq!(data.items[2].title, "ACH_SEM_TITULO");
        assert!(data.note.is_none());

        let failed = parse_steam_achievements(json!({ "ok": false, "error": "sem_conta" }))
            .expect_err("ok:false vira erro");
        assert!(failed.contains("sem_conta"), "{failed}");
        assert!(parse_steam_achievements(json!(["não é item"])).expect("itens inválidos").items.is_empty());
    }

    #[test]
    fn parses_retro_achievements_and_missing_game_as_a_note() {
        let data = parse_retro_achievements(json!({
            "ok": true,
            "game": { "title": "Chrono Trigger" },
            "achievements": [
                {
                    "title": "Herói do tempo",
                    "description": "Termine o jogo",
                    "points": 50,
                    "badgeUrl": "https://media.retroachievements.org/Badge/1.png",
                    "badgeLockedUrl": "https://media.retroachievements.org/Badge/1_lock.png",
                    "unlocked": true
                },
                {
                    "title": "Segredo",
                    "description": "",
                    "badgeLockedUrl": "https://media.retroachievements.org/Badge/2_lock.png",
                    "unlocked": false
                }
            ]
        }))
        .expect("progresso válido");

        assert_eq!(data.items.len(), 2);
        assert_eq!(data.items[0].points, Some(50));
        assert!(data.items[0].unlocked);
        assert_eq!(
            data.items[0].icon.as_deref(),
            Some("https://media.retroachievements.org/Badge/1.png")
        );
        assert_eq!(
            data.items[1].icon.as_deref(),
            Some("https://media.retroachievements.org/Badge/2_lock.png")
        );

        let missing = parse_retro_achievements(json!({
            "ok": true,
            "game": null,
            "achievements": []
        }))
        .expect("jogo ausente não é erro");
        assert!(missing.items.is_empty());
        assert!(missing.note.as_deref().unwrap_or_default().contains("não encontrou"));

        let offline = parse_retro_achievements(json!({ "ok": false, "error": "nao_logado" }))
            .expect_err("conta desconectada");
        assert!(offline.contains("conecte a conta"), "{offline}");
    }

    #[test]
    fn parses_protondb_tiers_scores_and_nulls() {
        let info = parse_protondb(
            json!({
                "ok": true,
                "info": {
                    "tier": "GOLD",
                    "score": 0.734,
                    "deckCompatibility": "PLAYABLE",
                    "total": 42,
                    "url": ""
                }
            }),
            "steam:570",
        )
        .expect("payload válido")
        .expect("relatórios presentes");

        assert_eq!(info.tier.as_deref(), Some("gold"));
        assert_eq!(info.score_pct, Some(73));
        assert_eq!(info.deck.as_deref(), Some("playable"));
        assert_eq!(info.total, Some(42));
        assert_eq!(info.url.as_deref(), Some("https://www.protondb.com/app/570"));

        assert_eq!(
            parse_protondb(json!({ "ok": true, "info": null }), "570").unwrap(),
            None
        );
        assert_eq!(
            parse_protondb(json!({ "ok": true, "info": {} }), "570").unwrap(),
            None
        );
        assert!(parse_protondb(json!({ "ok": false, "error": "api_indisponivel" }), "570").is_err());
    }

    #[test]
    fn parses_game_stats_owners_ccu_and_reviews_tolerantly() {
        let stats = parse_game_stats(json!({
            "ok": true,
            "info": {
                "owners": 1_000_000,
                "ccu": "12345",
                "reviewDesc": "Muito positivas",
                "reviewPositivePct": 94.6,
                "totalReviews": "250000",
                "comments": [
                    {
                        "author": "Steam 1234",
                        "text": "Ótimo jogo",
                        "positive": true,
                        "hours": 120,
                        "helpful": 7
                    },
                    {
                        "author": "",
                        "text": "",
                        "positive": false
                    },
                    { "text": "Sem autor", "positive": false }
                ]
            }
        }))
        .expect("stats válidas")
        .expect("dados presentes");

        assert_eq!(stats.owners.as_deref(), Some("1000000"));
        assert_eq!(stats.ccu, Some(12345));
        assert_eq!(stats.positive_pct, Some(95));
        assert_eq!(stats.total_reviews, Some(250000));
        assert_eq!(stats.reviews.len(), 2);
        assert_eq!(stats.reviews[0].author, "Steam 1234");
        assert!(stats.reviews[0].positive);
        assert_eq!(stats.reviews[1].author, "Steam");

        assert_eq!(
            parse_game_stats(json!({ "ok": true, "info": null })).unwrap(),
            None
        );
        assert_eq!(
            parse_game_stats(json!({ "ok": true, "info": {} })).unwrap(),
            None
        );
        assert!(parse_game_stats(json!({ "ok": false, "error": "api_indisponivel" })).is_err());
    }

    #[test]
    fn game_exposes_steam_appid_and_retro_system_safely() {
        let mut game = launch_game_fixture();
        assert_eq!(game.steam_appid(), Some("10"));
        assert!(!game.is_retro());
        assert_eq!(game.retro_system(), None);

        game.id = "retro:ps2:SLUS20312".to_string();
        game.launcher = "retro".to_string();
        assert_eq!(game.steam_appid(), None);
        assert!(game.is_retro());
        assert_eq!(game.retro_system(), Some("ps2"));

        game.id = "12345".to_string();
        game.launcher = "Steam".to_string();
        assert_eq!(game.steam_appid(), Some("12345"));

        game.id = "steam:12x".to_string();
        game.launcher = "steam".to_string();
        assert_eq!(game.steam_appid(), None);
    }

    #[test]
    fn parses_youtube_trailer_search_and_reports_ok_false() {
        let results = parse_trailer_search(json!({
            "ok": true,
            "results": [
                {
                    "id": "abc123",
                    "title": "Trailer oficial",
                    "thumbnail": "https://i.ytimg.test/abc123.jpg",
                    "duration": 95,
                    "channel": "Estúdio",
                    "url": "https://www.youtube.com/watch?v=abc123"
                },
                {
                    "id": "sem-url",
                    "title": "Só id",
                    "duration": "42.6"
                },
                { "title": "Sem id nem URL" }
            ]
        }))
        .expect("busca válida");

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "abc123");
        assert_eq!(results[0].duration, 95);
        assert_eq!(results[0].channel, "Estúdio");
        assert_eq!(results[0].thumbnail.as_deref(), Some("https://i.ytimg.test/abc123.jpg"));

        assert_eq!(results[1].url, "https://www.youtube.com/watch?v=sem-url");
        assert_eq!(results[1].duration, 43);
        assert!(results[1].thumbnail.is_none());

        let failed = parse_trailer_search(json!({
            "ok": false,
            "error": "yt_dlp_ausente",
            "mensagem": "yt-dlp não está instalado"
        }))
        .expect_err("ok:false vira erro");
        assert!(failed.contains("yt-dlp não está instalado"), "{failed}");

        assert!(parse_trailer_search(json!(["não é objeto"])).is_err());
    }

    #[test]
    fn parses_trailer_path_and_download_envelopes() {
        let path = "file:///tmp/trailers/steam_10.mp4";
        assert_eq!(
            parse_trailer_path(&json!({ "path": path })),
            Some(path.to_string())
        );
        assert_eq!(parse_trailer_path(&json!({ "path": "" })), None);

        assert_eq!(
            parse_trailer_download(json!({ "ok": true, "path": path })).unwrap(),
            path
        );
        assert!(parse_trailer_download(json!({ "ok": true })).is_err());

        let age = parse_trailer_download(json!({ "ok": false, "error": "age" }))
            .expect_err("restrição de idade");
        assert!(age.contains("restrição de idade"), "{age}");

        let network = parse_trailer_download(json!({
            "ok": false,
            "error": "ERROR: unable to download video"
        }))
        .expect_err("falha de rede");
        assert!(network.contains("unable to download"), "{network}");
    }

    fn launch_game_fixture() -> Game {
        Game {
            id: "steam:10".to_string(),
            title: "Half-Life".to_string(),
            launcher: "steam".to_string(),
            launch_cmd: vec!["steam".to_string(), "steam://run/10".to_string()],
            cover_url: None,
            hero_url: None,
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
        }
    }

    #[test]
    fn launch_payload_matches_the_web_bridge_contract() {
        let game = launch_game_fixture();

        // useGameActions.launch(command, gameId) → preload `{ cmd, gameId }`.
        // As opções de Wine/Proton ficam no game_settings.json lido pelo IPC.
        assert_eq!(
            launch_payload(&game, None),
            json!({
                "cmd": ["steam", "steam://run/10"],
                "gameId": "steam:10"
            })
        );

        // Seletor do diálogo: mode "steam" | "exe"; "custom" não existe no
        // contrato do resolver (`LaunchRequest.mode`).
        assert_eq!(
            launch_payload(&game, Some("exe")),
            json!({
                "cmd": ["steam", "steam://run/10"],
                "gameId": "steam:10",
                "mode": "exe"
            })
        );
        assert_eq!(
            launch_payload(&game, Some("steam"))["mode"],
            json!("steam")
        );
    }

    #[test]
    fn launch_errors_from_the_dispatcher_are_readable() {
        let error = check_game_launch(&json!({
            "ok": false,
            "error": "Já existe um lançamento ou jogo em execução."
        }))
        .expect_err("ok:false deve virar Err");
        assert!(error.contains("game:launch"), "{error}");
        assert!(error.contains("Já existe"), "{error}");

        assert!(check_game_launch(&json!({ "ok": true, "pid": 42 })).is_ok());
    }

    #[test]
    fn parses_the_launch_event_contract() {
        assert_eq!(
            parse_launch_event("game:running", &json!(true)),
            Some(LaunchEvent::Running(true))
        );
        assert_eq!(
            parse_launch_event("game:running", &json!(false)),
            Some(LaunchEvent::Running(false))
        );
        assert_eq!(
            parse_launch_event("game:active", &json!({ "rodando": true, "gameId": "steam:10" })),
            Some(LaunchEvent::Active {
                running: true,
                game_id: Some("steam:10".to_string()),
            })
        );
        assert_eq!(
            parse_launch_event(
                "game:launchState",
                &json!({ "state": "starting", "gameId": "steam:10", "token": 7 })
            ),
            Some(LaunchEvent::Phase {
                phase: LaunchPhase::Starting,
                game_id: Some("steam:10".to_string()),
            })
        );
        assert_eq!(
            parse_launch_event(
                "game:launchWarning",
                &json!({
                    "gameId": "steam:10",
                    "warnings": ["mangohud não está instalado — iniciando sem ele", " "]
                })
            ),
            Some(LaunchEvent::Warning {
                game_id: Some("steam:10".to_string()),
                warnings: vec!["mangohud não está instalado — iniciando sem ele".to_string()],
            })
        );
        assert_eq!(
            parse_launch_event(
                "game:launchError",
                &json!({ "gameId": "steam:10", "error": "No such file" })
            ),
            Some(LaunchEvent::Error {
                game_id: Some("steam:10".to_string()),
                error: "No such file".to_string(),
            })
        );

        // Eventos fora do contrato ou payloads incompletos são ignorados.
        assert_eq!(parse_launch_event("game:unknown", &json!({})), None);
        assert_eq!(
            parse_launch_event("game:launchState", &json!({ "state": "paused" })),
            None
        );
        assert_eq!(
            parse_launch_event("game:launchWarning", &json!({ "warnings": [] })),
            None
        );
        assert_eq!(parse_launch_event("library:changed", &json!({})), None);
    }

    #[test]
    fn parses_game_settings_and_default_prefix() {
        let envelope = parse_game_settings_envelope(&json!({
            "settings": {
                "wineVersion": "steam:GE-Proton9-20",
                "prefixPath": "/games/pfx",
                "exePath": "/games/game.exe",
                "gameArgs": "--windowed",
                "dxvkHud": "fps",
                "envVars": [
                    { "name": "WINEDLLOVERRIDES", "value": "winmm=n,b" },
                    { "name": "  ", "value": "ignorada" }
                ],
                "esync": true,
                "fsync": true,
                "wineWayland": true,
                "wow64": true,
                "fsrHack": true,
                "autoNVAPI": true,
                "autoDXVK": false,
                "autoVKD3D": true,
                "mangohud": true,
                "gamemode": true,
                "gamescope": true,
                "gsHdr": true,
                "verboseLogs": true,
                "scriptPre": "/tmp/pre.sh",
                "wrappers": [{ "cmd": "mangohud" }],
                "emulatorId": "retroarch",
                "romPath": "/roms/snes/game.sfc",
                "emulatorArgs": ["--fullscreen", "-v"],
                "emulatorCorePath": "/cores/snes9x.so"
            },
            "defaultPrefix": "/home/user/Games/Arcadia/Prefixes/steam_10"
        }));

        let settings = envelope.settings;
        assert_eq!(settings.wine_version.as_deref(), Some("steam:GE-Proton9-20"));
        assert_eq!(settings.prefix_path, "/games/pfx");
        assert_eq!(settings.exe_path, "/games/game.exe");
        assert_eq!(settings.game_args, "--windowed");
        assert_eq!(settings.dxvk_hud, "fps");
        assert_eq!(settings.env_vars.len(), 1);
        assert_eq!(settings.env_vars[0].name, "WINEDLLOVERRIDES");
        assert_eq!(settings.env_vars[0].value, "winmm=n,b");
        assert!(settings.esync && settings.fsync);
        assert!(settings.wine_wayland && settings.wow64 && settings.fsr_hack);
        assert!(settings.auto_nvapi);
        assert!(!settings.auto_dxvk);
        assert!(settings.auto_vkd3d);
        assert!(settings.mangohud && settings.gamemode && settings.gamescope);
        assert!(settings.gs_hdr && settings.verbose_logs);
        assert_eq!(settings.script_pre, "/tmp/pre.sh");
        assert_eq!(settings.wrapper_count, 1);
        assert_eq!(settings.emulator_id.as_deref(), Some("retroarch"));
        assert_eq!(settings.rom_path, "/roms/snes/game.sfc");
        assert_eq!(
            settings.emulator_args,
            vec!["--fullscreen".to_string(), "-v".to_string()]
        );
        assert_eq!(settings.emulator_core_path, "/cores/snes9x.so");
        assert_eq!(
            envelope.default_prefix,
            "/home/user/Games/Arcadia/Prefixes/steam_10"
        );

        // Sem settings salvos: defaults do resolver (autoDXVK ausente = on).
        let empty = parse_game_settings(&json!({}));
        assert_eq!(empty, GameSettingsView::default());
        assert!(empty.auto_dxvk);
    }

    #[test]
    fn settings_patch_uses_only_the_supported_contract_fields() {
        let mut settings = GameSettingsView {
            wine_version: Some("steam:GE-Proton9-20".to_string()),
            prefix_path: "  /games/pfx  ".to_string(),
            exe_path: " /games/game.exe ".to_string(),
            game_args: " --windowed ".to_string(),
            dxvk_hud: " fps ".to_string(),
            script_pre: "/tmp/ignorado.sh".to_string(),
            auto_dxvk: false,
            auto_vkd3d: true,
            emulator_id: Some("retroarch".to_string()),
            rom_path: " /roms/snes/game.sfc ".to_string(),
            emulator_args: vec!["--fullscreen".to_string(), "-v".to_string()],
            emulator_core_path: " /cores/snes9x.so ".to_string(),
            ..GameSettingsView::default()
        };
        settings.env_vars.push(EnvVarSetting {
            name: "WINEDLLOVERRIDES".to_string(),
            value: "winmm=n,b".to_string(),
        });
        settings.set_toggle(LaunchToggle::Esync, true);
        settings.set_toggle(LaunchToggle::Gamescope, true);

        let patch = settings.to_patch();
        assert_eq!(patch["prefixPath"], "/games/pfx");
        assert_eq!(patch["exePath"], "/games/game.exe");
        assert_eq!(patch["gameArgs"], "--windowed");
        assert_eq!(patch["dxvkHud"], "fps");
        assert_eq!(patch["esync"], true);
        assert_eq!(patch["gamescope"], true);
        assert_eq!(patch["envVars"][0]["name"], "WINEDLLOVERRIDES");
        assert_eq!(patch["emulatorId"], "retroarch");
        assert_eq!(patch["romPath"], "/roms/snes/game.sfc");
        assert_eq!(
            patch["emulatorArgs"],
            json!(["--fullscreen", "-v"]),
        );
        assert_eq!(patch["emulatorCorePath"], "/cores/snes9x.so");

        // Campos não editáveis não são sobrescritos pelo diálogo nativo.
        assert!(patch.get("wineVersion").is_none());
        assert!(patch.get("autoDXVK").is_none());
        assert!(patch.get("autoVKD3D").is_none());
        assert!(patch.get("scriptPre").is_none());
    }

    #[test]
    fn game_settings_roundtrip_preserves_fields_outside_the_dialog() {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-native-settings-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let backend = Backend::new(AppState::new(dir.clone()), runtime.handle().clone());

        // Campos fora do diálogo (wineVersion/autoDXVK) chegam pela API real.
        runtime
            .block_on(backend.dispatch(
                "gamesettings:set",
                vec![json!({
                    "id": "steam:10",
                    "patch": {
                        "wineVersion": "steam:GE-Proton9-20",
                        "autoDXVK": false
                    }
                })],
            ))
            .unwrap();

        let mut settings = GameSettingsView::default();
        settings.prefix_path = "/games/pfx".to_string();
        settings.set_toggle(LaunchToggle::Esync, true);
        settings.emulator_id = Some("retroarch".to_string());
        settings.rom_path = "/roms/snes/game.sfc".to_string();
        settings.emulator_args = vec!["--fullscreen".to_string(), "-v".to_string()];
        settings.emulator_core_path = "/cores/snes9x.so".to_string();
        let saved = runtime
            .block_on(backend.save_game_settings("steam:10".to_string(), settings.to_patch()))
            .unwrap();
        assert_eq!(saved.prefix_path, "/games/pfx");
        assert!(saved.esync);
        assert_eq!(saved.emulator_id.as_deref(), Some("retroarch"));
        assert_eq!(saved.rom_path, "/roms/snes/game.sfc");

        let loaded = runtime
            .block_on(backend.load_game_settings("steam:10".to_string()))
            .unwrap();
        assert_eq!(loaded.settings.prefix_path, "/games/pfx");
        assert!(loaded.settings.esync);
        assert_eq!(loaded.settings.emulator_id.as_deref(), Some("retroarch"));
        assert_eq!(loaded.settings.rom_path, "/roms/snes/game.sfc");
        assert_eq!(
            loaded.settings.emulator_args,
            vec!["--fullscreen".to_string(), "-v".to_string()]
        );
        assert_eq!(loaded.settings.emulator_core_path, "/cores/snes9x.so");
        // O patch do diálogo não apaga o que a UI web gravou.
        assert_eq!(
            loaded.settings.wine_version.as_deref(),
            Some("steam:GE-Proton9-20")
        );
        assert!(!loaded.settings.auto_dxvk);
        assert!(!loaded.default_prefix.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn emulator_import_slug_and_payload_are_stable() {
        let request = EmulatorRomImport {
            emulator_id: "retroarch".to_string(),
            rom_path: " /roms/snes/Super Metroid (USA).sfc ".to_string(),
            rom_name: "Super Metroid (USA).sfc".to_string(),
            extra_args: vec!["--fullscreen".to_string()],
            core_path: Some("/cores/snes9x.so".to_string()),
            occupied_ids: Vec::new(),
        };
        assert_eq!(request.title(), "Super Metroid (USA)");
        assert_eq!(
            request.game_id(),
            "custom:retroarch-super-metroid-usa"
        );
        // Estável para a mesma ROM/título.
        assert_eq!(request.game_id(), request.game_id());

        let payload = request.custom_game_payload();
        assert_eq!(payload["id"], "custom:retroarch-super-metroid-usa");
        assert_eq!(payload["title"], "Super Metroid (USA)");
        assert_eq!(payload["platform"], "emulator");
        assert_eq!(payload["emulatorId"], "retroarch");
        assert_eq!(payload["romPath"], "/roms/snes/Super Metroid (USA).sfc");
        assert_eq!(payload["emulatorArgs"], json!(["--fullscreen"]));
        assert_eq!(payload["emulatorCorePath"], "/cores/snes9x.so");
        assert_eq!(
            request.settings_patch(),
            json!({
                "emulatorId": "retroarch",
                "romPath": "/roms/snes/Super Metroid (USA).sfc",
                "emulatorArgs": ["--fullscreen"],
                "emulatorCorePath": "/cores/snes9x.so",
            })
        );

        let mut occupied = vec!["custom:retroarch-super-metroid-usa".to_string()];
        let collided = EmulatorRomImport {
            occupied_ids: occupied.clone(),
            ..request.clone()
        };
        assert_eq!(
            collided.game_id(),
            "custom:retroarch-super-metroid-usa-2"
        );
        occupied.push("custom:retroarch-super-metroid-usa-2".to_string());
        let collided_again = EmulatorRomImport {
            occupied_ids: occupied,
            ..request
        };
        assert_eq!(
            collided_again.game_id(),
            "custom:retroarch-super-metroid-usa-3"
        );
    }

    #[test]
    fn emulator_import_slug_is_bounded_and_valid() {
        let title = "ç".repeat(400);
        let id = custom_emulator_game_id("pcsx2", &title, &[]);
        let slug = id.strip_prefix("custom:").unwrap();
        assert!(slug.len() <= 90, "{id}");
        assert!(slug.chars().next().unwrap().is_ascii_lowercase());
        assert!(slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-')));

        let long_title = "A Very Long Game Title ".repeat(10);
        let base = slugify(&format!("retroarch-{long_title}"))
            .chars()
            .take(90)
            .collect::<String>();
        let base = base.trim_matches('-').to_string();
        let with_suffix =
            custom_emulator_game_id("retroarch", &long_title, &[format!("custom:{base}")]);
        assert!(with_suffix.ends_with("-2"), "{with_suffix}");
        assert!(with_suffix.strip_prefix("custom:").unwrap().len() <= 90);
    }

    #[test]
    fn emulator_resolve_parser_keeps_contract_errors() {
        let ok = parse_emulator_resolve(&json!({
            "ok": true,
            "emulatorId": "pcsx2",
            "system": "playstation-2",
            "cmd": ["pcsx2-qt", "/roms/game.iso"]
        }));
        assert!(ok.ok);
        assert_eq!(ok.cmd, vec!["pcsx2-qt".to_string(), "/roms/game.iso".to_string()]);
        assert_eq!(ok.error_text(), "configuração do emulador inválida");

        let failed = parse_emulator_resolve(&json!({
            "ok": false,
            "error": "ROM inválida",
            "code": "rom_invalida"
        }));
        assert!(!failed.ok);
        assert_eq!(failed.error_text(), "ROM inválida (rom_invalida)");

        let only_code = parse_emulator_resolve(&json!({ "ok": false, "code": "core_ausente" }));
        assert_eq!(only_code.error_text(), "core_ausente");
    }

    #[test]
    fn emulator_settings_resolve_payload_matches_contract() {
        let settings = GameSettingsView {
            emulator_id: Some("retroarch".to_string()),
            rom_path: " /roms/snes/game.sfc ".to_string(),
            emulator_args: vec!["--fullscreen".to_string()],
            emulator_core_path: " /cores/snes9x.so ".to_string(),
            ..GameSettingsView::default()
        };
        assert_eq!(
            settings.emulator_resolve_payload("hydra"),
            json!({
                "emulatorId": "retroarch",
                "romPath": "/roms/snes/game.sfc",
                "extraArgs": ["--fullscreen"],
                "corePath": "/cores/snes9x.so",
                "launchMode": "hydra",
            })
        );
    }

    #[test]
    fn parses_store_search_with_price_source_and_numeric_appid() {
        let search = parse_store_search(json!({
            "ok": true,
            "jogos": [
                {
                    "appid": "10",
                    "title": "Half-Life",
                    "cover": "https://cdn.test/hl.jpg",
                    "heroi": "https://cdn.test/hl-hero.jpg",
                    "capa": "https://cdn.test/hl-portrait.jpg",
                    "preco": "R$ 19,99",
                    "fontes": ["Hubcap"],
                    "manifest": true
                },
                {
                    "appid": 20,
                    "title": "Jogo numérico",
                    "fontes": []
                },
                { "appid": "", "title": "Sem appid" }
            ],
            "fonte": "steam",
            "avisos": ["catálogo parcial"]
        }))
        .expect("busca válida");

        assert_eq!(search.games.len(), 2);
        let first = &search.games[0];
        assert_eq!(first.appid, "10");
        assert_eq!(first.price.as_deref(), Some("R$ 19,99"));
        assert_eq!(first.source.as_deref(), Some("Hubcap"));
        assert!(first.manifest);
        assert_eq!(first.image_urls().len(), 3);
        assert_eq!(search.source.as_deref(), Some("steam"));
        assert_eq!(search.warnings, vec!["catálogo parcial"]);

        let second = &search.games[1];
        assert_eq!(second.appid, "20");
        assert_eq!(
            second.cover_url.as_deref(),
            Some("https://cdn.akamai.steamstatic.com/steam/apps/20/header.jpg")
        );
        assert!(second.image_urls().len() == 1);
    }

    #[test]
    fn store_parsers_report_ok_false_and_accept_null_lists() {
        let error = parse_store_search(json!({ "ok": false, "error": "hubcap_offline" }))
            .expect_err("ok:false deve virar erro");
        assert!(error.contains("hubcap_offline"), "{error}");

        let empty = parse_store_search(json!({ "ok": true, "jogos": null })).expect("null é vazio");
        assert!(empty.games.is_empty());

        let suggest = parse_store_suggest(json!({
            "ok": true,
            "jogos": [{ "appid": "30", "title": "Sugestão" }]
        }))
        .expect("sugestões válidas");
        assert_eq!(suggest.len(), 1);

        assert!(parse_store_suggest(json!({ "ok": false, "error": "sem_rede" })).is_err());
        assert!(parse_store_recent(Value::Null).is_err());
    }

    #[test]
    fn parses_store_recent_pagination_fallbacks() {
        let page = parse_store_recent(json!({
            "ok": true,
            "jogos": [
                { "appid": "1", "title": "A" },
                { "appid": "2", "title": "B" }
            ],
            "offset": 24,
            "total": 100
        }))
        .expect("página válida");
        assert_eq!(page.games.len(), 2);
        assert_eq!(page.total, Some(100));
        assert!(page.has_more);
        assert_eq!(page.next_offset, Some(26));

        let last = parse_store_recent(json!({
            "ok": true,
            "jogos": [],
            "offset": 96,
            "total": 96,
            "has_more": false,
            "next_offset": null
        }))
        .expect("última página válida");
        assert!(!last.has_more);
        assert!(last.next_offset.is_none());
    }

    #[test]
    fn parses_retro_page_and_builds_cover_fallbacks() {
        let page = parse_retro_page(json!({
            "ok": true,
            "games": [
                {
                    "id": "retro:1",
                    "title": "Chrono Trigger",
                    "systemId": "nintendo-snes",
                    "releaseYear": 1995,
                    "developer": ["Square"],
                    "genres": ["RPG"],
                    "offerCount": 3,
                    "artwork": {
                        "cover": "//cdn.test/chrono.jpg",
                        "titleScreen": "https://cdn.test/chrono-title.png",
                        "backgrounds": ["https://cdn.test/chrono-hero.jpg"]
                    }
                },
                {
                    "id": "retro:2",
                    "title": "Sem arte",
                    "platform": "sony-playstation"
                }
            ],
            "sources": [{ "id": "src", "title": "Fonte", "url": "https://example.test/catalog" }],
            "total": 240,
            "totalOffers": 500,
            "offset": 24,
            "hasMore": true
        }))
        .expect("catálogo válido");

        assert_eq!(page.games.len(), 2);
        assert_eq!(page.total, Some(240));
        assert_eq!(page.total_offers, Some(500));
        assert!(page.has_more);
        assert_eq!(page.sources.len(), 1);

        let chrono = &page.games[0];
        assert_eq!(
            chrono.cover_url.as_deref(),
            Some("https://cdn.test/chrono.jpg")
        );
        assert_eq!(chrono.system_label(), "Super Nintendo");
        assert_eq!(chrono.meta_line(), "1995  ·  Square  ·  RPG");
        assert_eq!(chrono.offer_count, Some(3));

        let sem_arte = &page.games[1];
        assert!(sem_arte.cover_url.is_none());
        assert_eq!(sem_arte.system_label(), "PlayStation");

        let error = parse_retro_page(json!({ "ok": false, "error": "catalogo_offline" }))
            .expect_err("ok:false deve virar erro");
        assert!(error.contains("catalogo_offline"), "{error}");
    }

    #[test]
    fn parses_store_info_and_reports_null_info() {
        let info = parse_store_info(json!({
            "ok": true,
            "info": {
                "short_description": "<p>Descrição &amp; detalhes</p>",
                "background": "https://cdn.test/bg.jpg",
                "header": "//cdn.test/header.jpg",
                "developers": ["Valve"],
                "publishers": ["Valve"],
                "release_date": "2004"
            }
        }))
        .expect("ficha válida");
        assert_eq!(info.description.as_deref(), Some("Descrição & detalhes"));
        assert_eq!(info.image_urls().len(), 2);
        assert_eq!(info.developers, vec!["Valve"]);

        let null_info =
            parse_store_info(json!({ "ok": true, "info": null })).expect_err("info:null");
        assert!(null_info.contains("info:null"), "{null_info}");

        let failed = parse_store_info(json!({ "ok": false, "error": "api_indisponivel" }))
            .expect_err("ok:false");
        assert!(failed.contains("api_indisponivel"), "{failed}");
    }

    #[test]
    fn parses_store_install_info_libraries_and_recommended_depots() {
        let info = parse_store_install_info(json!({
            "ok": true,
            "appid": "10",
            "fonte": "Hubcap",
            "installdir": "HalfLife",
            "token": "token-abc",
            "dlcs": ["20"],
            "depots": [
                { "depotId": "11", "manifestId": "111", "key": "k1", "size": 100, "os": "windows" },
                { "depotId": "12", "manifestId": "112", "key": "k2", "size": 200, "os": "windows", "language": "english" },
                { "depotId": "13", "manifestId": "113", "key": "k3", "size": 300, "os": "windows", "language": "brazilian" },
                { "depotId": "14", "manifestId": "114", "key": "k4", "size": 400, "os": "windows", "dlcAppid": "20" },
                { "depotId": "15", "manifestId": "115", "key": "k5", "size": 500, "shared": true },
                { "depotId": "16", "manifestId": "116", "key": "k6", "size": 600, "os": "macos" },
                { "depotId": "17", "manifestId": "117", "key": "k7", "size": "700" }
            ]
        }))
        .expect("manifesto válido");

        assert_eq!(info.appid.as_deref(), Some("10"));
        assert_eq!(info.source.as_deref(), Some("Hubcap"));
        assert_eq!(info.install_dir.as_deref(), Some("HalfLife"));
        assert_eq!(info.token.as_deref(), Some("token-abc"));
        assert_eq!(info.dlcs, vec!["20"]);
        assert_eq!(info.depots.len(), 7);
        assert_eq!(info.total_size(), 2_800);
        assert_eq!(
            info.recommended_depots(),
            ["11", "12", "17"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
        let relevant: Vec<&str> = info
            .relevant_depots()
            .iter()
            .map(|depot| depot.depot_id.as_str())
            .collect();
        assert_eq!(relevant, vec!["11", "12", "13", "14"]);

        let failed = parse_store_install_info(json!({
            "ok": false,
            "error": "nenhum provedor disponível"
        }))
        .expect_err("ok:false deve virar erro");
        assert!(failed.contains("nenhum provedor disponível"), "{failed}");

        let libraries = parse_store_libraries(json!([
            {
                "path": "/games/SteamLibrary/steamapps",
                "steamDir": "/games/SteamLibrary",
                "free": 123456
            }
        ]))
        .expect("bibliotecas válidas");
        assert_eq!(libraries.len(), 1);
        assert_eq!(libraries[0].steam_dir, "/games/SteamLibrary");
        assert_eq!(libraries[0].free, 123_456);
        assert!(parse_store_libraries(json!({ "ok": true, "libraries": [] }))
            .expect("objeto com libraries")
            .is_empty());
        assert!(parse_store_libraries(Value::Null).is_err());
    }

    #[test]
    fn builds_store_install_payload_and_maps_dispatcher_errors() {
        let game = StoreGame {
            appid: "steam:10".to_string(),
            title: "Half-Life: 2".to_string(),
            cover_url: Some("https://cdn.test/header.jpg".to_string()),
            portrait_url: Some("https://cdn.test/portrait.jpg".to_string()),
            ..StoreGame::default()
        };
        let info = parse_store_install_info(json!({
            "ok": true,
            "installdir": "HalfLife2",
            "token": "token",
            "dlcs": ["20"],
            "depots": [
                { "depotId": "11", "manifestId": "111", "key": "k1", "size": 100 }
            ]
        }))
        .expect("manifesto válido");
        let depots: Vec<StoreDepot> = info.depots.clone();
        let payload = store_install_payload(&game, &info, "/games/Steam", &depots);

        assert_eq!(payload["appid"], "10");
        assert_eq!(payload["title"], "Half-Life: 2");
        assert_eq!(payload["cover"], "https://cdn.test/portrait.jpg");
        assert_eq!(payload["installdir"], "HalfLife2");
        assert_eq!(payload["steamDir"], "/games/Steam");
        assert_eq!(payload["token"], "token");
        assert_eq!(payload["dlcs"], json!(["20"]));
        assert_eq!(payload["depots"][0]["depotId"], "11");
        assert_eq!(payload["depots"][0]["manifestId"], "111");
        assert_eq!(store_install_dir("???", "10"), "10");

        assert!(parse_store_install_response(json!({ "ok": true })).is_ok());
        let plugin = parse_store_install_response(json!({
            "ok": false,
            "plugin": "slssteam"
        }))
        .expect_err("plugin desligado");
        assert!(plugin.contains("slssteam"), "{plugin}");
        let failed = parse_store_install_response(json!({
            "ok": false,
            "error": "Steam não encontrada"
        }))
        .expect_err("erro do dispatcher");
        assert!(failed.contains("Steam não encontrada"), "{failed}");
    }

    #[test]
    fn parses_retro_game_detail_offers_and_offer_uris() {
        let detail = parse_retro_game_detail(json!({
            "ok": true,
            "game": {
                "id": "retro:1",
                "title": "Chrono Trigger",
                "systemId": "nintendo-snes",
                "platform": "nintendo-snes",
                "releaseYear": 1995,
                "developer": ["Square"],
                "publisher": ["Square"],
                "genres": ["RPG", "Aventura"],
                "description": "<p>Viagem no tempo</p>",
                "offerCount": 2,
                "fileSize": "4 MiB",
                "artwork": {
                    "cover": "//cdn.test/chrono.jpg",
                    "backgrounds": ["https://cdn.test/chrono-hero.jpg"]
                }
            },
            "offers": [
                {
                    "id": "offer:1",
                    "sourceId": "src",
                    "sourceTitle": "Fonte",
                    "originalTitle": "Chrono Trigger (USA)",
                    "fileSize": "4 MiB",
                    "uploadDate": "2024-01-02",
                    "hasUris": true,
                    "uriCount": 2
                }
            ],
            "sources": [
                { "id": "src", "title": "Fonte", "url": "https://example.test/catalog" }
            ]
        }))
        .expect("ficha Retro válida");

        assert_eq!(detail.game.title, "Chrono Trigger");
        assert_eq!(detail.game.release_year, Some(1995));
        assert_eq!(detail.game.developer.as_deref(), Some("Square"));
        assert_eq!(detail.game.genres, vec!["RPG", "Aventura"]);
        assert_eq!(
            detail.game.description.as_deref(),
            Some("Viagem no tempo")
        );
        assert_eq!(
            detail.game.hero_url.as_deref(),
            Some("https://cdn.test/chrono-hero.jpg")
        );
        assert_eq!(detail.offers.len(), 1);
        assert_eq!(detail.offers[0].uri_count, 2);
        assert!(detail.offers[0].has_uris);
        assert_eq!(detail.sources.len(), 1);

        let offer = parse_retro_offer_detail(json!({
            "ok": true,
            "offer": {
                "id": "offer:1",
                "uris": ["magnet:?xt=urn:btih:abc", "https://example.test/rom.zip", " "]
            }
        }))
        .expect("oferta válida");
        assert_eq!(offer.id, "offer:1");
        assert_eq!(offer.uris.len(), 2);

        let failed = parse_retro_game_detail(json!({
            "ok": false,
            "error": "Game not found"
        }))
        .expect_err("ok:false deve virar erro");
        assert!(failed.contains("Game not found"), "{failed}");
        assert!(parse_retro_game_detail(json!({ "ok": true })).is_err());
        assert!(parse_retro_offer_detail(json!({ "ok": true })).is_err());
    }

    #[test]
    fn parses_emulators_list_with_profiles_and_detected_wrappers() {
        let emulators = parse_emulators_list(json!({
            "ok": true,
            "emulators": [
                {
                    "id": "pcsx2",
                    "name": "PCSX2",
                    "systems": ["PlayStation 2"],
                    "description": "Emulador de PlayStation 2",
                    "candidates": ["pcsx2-qt", "pcsx2"],
                    "romExtensions": [".iso", ".chd"],
                    "romDirectoryMarkers": ["roms"],
                    "executable": "/usr/bin/pcsx2-qt",
                    "available": true,
                    "source": "configured",
                    "requiresCore": false,
                    "profile": {
                        "id": "pcsx2",
                        "executable": "/usr/bin/pcsx2-qt",
                        "corePath": "",
                        "biosPath": "/home/jogador/bios/ps2",
                        "romFolders": [
                            { "path": "/home/jogador/roms/ps2", "recursive": true }
                        ],
                        "args": ["-fullscreen", "-nogui"],
                        "updatedAt": 1_700_000_000_000i64
                    }
                },
                {
                    "id": "duckstation",
                    "name": "DuckStation",
                    "systems": ["PlayStation 1"],
                    "description": "Emulador de PlayStation 1",
                    "executable": "flatpak",
                    "detectedArgs": ["run", "org.duckstation.DuckStation"],
                    "available": true,
                    "source": "detected",
                    "requiresCore": true,
                    "romExtensions": [".cue"],
                    "romDirectoryMarkers": []
                },
                { "id": "", "name": "sem id" }
            ]
        }))
        .expect("catálogo válido");

        assert_eq!(emulators.len(), 2);
        let pcsx2 = &emulators[0];
        assert_eq!(pcsx2.id, "pcsx2");
        assert_eq!(pcsx2.systems, vec!["PlayStation 2"]);
        assert_eq!(pcsx2.source, "configured");
        assert!(pcsx2.available);
        let profile = pcsx2.profile.as_ref().expect("perfil configurado");
        assert_eq!(profile.bios_path, "/home/jogador/bios/ps2");
        assert_eq!(profile.args, vec!["-fullscreen", "-nogui"]);
        assert_eq!(profile.rom_folders[0].path, "/home/jogador/roms/ps2");
        assert!(profile.rom_folders[0].recursive);

        let duckstation = &emulators[1];
        assert_eq!(
            duckstation.detected_args,
            vec!["run", "org.duckstation.DuckStation"]
        );
        assert!(duckstation.requires_core);
        assert!(duckstation.profile.is_none());

        let failed = parse_emulators_list(json!({ "ok": false, "error": "fila ocupada" }))
            .expect_err("ok:false deve virar erro");
        assert!(failed.contains("fila ocupada"), "{failed}");
    }

    #[test]
    fn parses_emulators_status_bios_and_running() {
        let statuses = parse_emulators_status(json!({
            "ok": true,
            "statuses": [
                {
                    "ok": true,
                    "emulatorId": "duckstation",
                    "kind": "bios",
                    "required": true,
                    "installed": true,
                    "detectedPath": "/home/jogador/bios/ps1",
                    "running": false,
                    "runningPid": null
                },
                {
                    "ok": true,
                    "emulatorId": "rpcs3",
                    "kind": "firmware",
                    "required": false,
                    "installed": false,
                    "detectedPath": null,
                    "running": true,
                    "runningPid": 4242
                }
            ]
        }))
        .expect("status válido");

        assert_eq!(statuses.len(), 2);
        assert!(statuses[0].required);
        assert!(statuses[0].installed);
        assert_eq!(
            statuses[0].detected_path.as_deref(),
            Some("/home/jogador/bios/ps1")
        );
        assert_eq!(statuses[1].kind.as_deref(), Some("firmware"));
        assert!(statuses[1].running);
        assert_eq!(statuses[1].running_pid, Some(4242));
        assert!(parse_emulators_status(json!({ "ok": true })).is_err());
    }

    #[test]
    fn parses_emulators_profiles_map() {
        let profiles = parse_emulators_profiles(json!({
            "ok": true,
            "profiles": {
                "retroarch": {
                    "id": "retroarch",
                    "executable": "/usr/bin/retroarch",
                    "corePath": "/home/jogador/cores/snes9x_libretro.so",
                    "biosPath": "",
                    "romFolders": [
                        { "path": "/home/jogador/roms/snes", "recursive": false }
                    ],
                    "args": ["-v"],
                    "updatedAt": 1_700_000_000_001i64
                }
            }
        }))
        .expect("perfis válidos");

        let retroarch = profiles.get("retroarch").expect("perfil");
        assert_eq!(retroarch.executable, "/usr/bin/retroarch");
        assert_eq!(
            retroarch.core_path,
            "/home/jogador/cores/snes9x_libretro.so"
        );
        assert!(!retroarch.rom_folders[0].recursive);
        assert_eq!(retroarch.args, vec!["-v"]);
        assert!(parse_emulators_profiles(json!({ "ok": true })).is_err());
    }

    #[test]
    fn parses_emulators_roms_scan_result() {
        let result = parse_emulator_roms(json!({
            "ok": true,
            "emulatorId": "pcsx2",
            "recursive": true,
            "maxDepth": 6,
            "folders": [{ "path": "/home/jogador/roms/ps2", "recursive": true }],
            "truncated": true,
            "persisted": true,
            "roms": [
                {
                    "path": "/home/jogador/roms/ps2/Game.cue",
                    "name": "Game.cue",
                    "relativePath": "Game.cue",
                    "extension": ".cue",
                    "kind": "file",
                    "sizeBytes": 4096,
                    "mtimeMs": 1_700_000_000_002i64,
                    "sidecars": ["/home/jogador/roms/ps2/Game.bin"]
                }
            ]
        }))
        .expect("varredura válida");

        assert_eq!(result.emulator_id, "pcsx2");
        assert!(result.recursive);
        assert!(result.truncated);
        assert!(result.persisted);
        assert_eq!(result.folders.len(), 1);
        assert_eq!(result.roms.len(), 1);
        assert_eq!(result.roms[0].relative_path, "Game.cue");
        assert_eq!(result.roms[0].size_bytes, 4096);
        assert_eq!(result.roms[0].sidecars.len(), 1);

        let failed = parse_emulator_roms(json!({
            "ok": false,
            "error": "diretorio_rom_invalido"
        }))
        .expect_err("ok:false deve virar erro");
        assert!(failed.contains("diretorio_rom_invalido"), "{failed}");
    }

    #[test]
    fn parses_emulators_rom_index_entries() {
        let index = parse_emulators_rom_index(json!({
            "ok": true,
            "emulators": {
                "pcsx2": {
                    "updatedAt": 1_700_000_000_003i64,
                    "directory": "",
                    "folders": [{ "path": "/roms/ps2", "recursive": true }],
                    "truncated": false,
                    "roms": [
                        {
                            "path": "/roms/ps2/Other.iso",
                            "name": "Other.iso",
                            "relativePath": "Other.iso",
                            "extension": ".iso",
                            "sizeBytes": 1024,
                            "sidecars": []
                        }
                    ]
                }
            }
        }))
        .expect("índice válido");

        let entry = index.get("pcsx2").expect("entrada");
        assert_eq!(entry.updated_at, 1_700_000_000_003);
        assert_eq!(entry.folders[0].path, "/roms/ps2");
        assert!(!entry.truncated);
        assert_eq!(entry.roms[0].name, "Other.iso");
        assert_eq!(entry.roms[0].kind, "file");
    }

    #[test]
    fn emulator_profile_payload_preserves_fields_outside_the_form() {
        let existing = EmulatorProfile {
            id: "retroarch".to_string(),
            executable: "/usr/bin/retroarch".to_string(),
            core_path: "/home/jogador/cores/snes9x_libretro.so".to_string(),
            bios_path: "/home/jogador/bios".to_string(),
            rom_folders: vec![EmulatorRomFolder {
                path: "/home/jogador/roms/antigas".to_string(),
                recursive: true,
            }],
            args: vec!["-v".to_string()],
            updated_at: 1,
        };
        let folders = vec![EmulatorRomFolder {
            path: "/home/jogador/roms/snes".to_string(),
            recursive: false,
        }];
        let args = vec!["-f".to_string()];

        let preserved = emulator_profile_payload(&EmulatorProfileDraft {
            id: "retroarch",
            executable: "  /usr/bin/retroarch  ",
            core_path: None,
            bios_path: None,
            rom_folders: &folders,
            args: &args,
            existing: Some(&existing),
        });
        assert_eq!(preserved["executable"], json!("/usr/bin/retroarch"));
        assert_eq!(
            preserved["corePath"],
            json!("/home/jogador/cores/snes9x_libretro.so")
        );
        assert_eq!(preserved["biosPath"], json!("/home/jogador/bios"));
        assert_eq!(
            preserved["romFolders"],
            json!([{ "path": "/home/jogador/roms/snes", "recursive": false }])
        );
        assert_eq!(preserved["args"], json!(["-f"]));

        let cleared = emulator_profile_payload(&EmulatorProfileDraft {
            id: "retroarch",
            executable: "/usr/bin/retroarch",
            core_path: Some(""),
            bios_path: Some(""),
            rom_folders: &folders,
            args: &args,
            existing: Some(&existing),
        });
        assert_eq!(cleared["corePath"], json!(""));
        assert_eq!(cleared["biosPath"], json!(""));
    }

    #[test]
    fn emulator_args_split_without_shell_and_reject_empty_executable() {
        assert_eq!(
            parse_emulator_args("  --fullscreen\t-L \"core path.so\"  "),
            vec!["--fullscreen", "-L", "\"core", "path.so\""]
        );
        let many = (0..40)
            .map(|index| format!("a{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(parse_emulator_args(&many).len(), MAX_EMULATOR_ARGS);

        let existing = EmulatorProfile {
            id: "retroarch".to_string(),
            args: vec![
                "--config=/home/jogador/config com espaço.cfg".to_string(),
                "-v".to_string(),
            ],
            ..EmulatorProfile::default()
        };
        let untouched = existing.args.join(" ");
        assert_eq!(
            emulator_args_from_text(&untouched, Some(&existing)),
            existing.args
        );
        assert_eq!(
            emulator_args_from_text("--config=/novo.cfg -v", Some(&existing)),
            vec!["--config=/novo.cfg", "-v"]
        );

        assert!(valid_emulator_executable("/usr/bin/pcsx2-qt"));
        assert!(!valid_emulator_executable(""));
        assert!(!valid_emulator_executable("   "));
    }

    #[test]
    fn parses_community_reviews_with_offline_and_pagination() {
        let page = parse_community_reviews(json!({
            "ok": true,
            "offline": true,
            "reviews": [
                {
                    "id": 7,
                    "text": "Muito bom",
                    "rating": 5,
                    "positive": true,
                    "hours": 12.5,
                    "created_at": "2026-01-02T03:04:05Z",
                    "username": "ana"
                },
                {
                    "id": "r2",
                    "text": "Sem nota",
                    "positive": 0,
                    "author": { "display_name": "Beto" }
                }
            ],
            "pagination": { "limit": 2, "offset": 4, "has_more": true, "total": 9 }
        }))
        .expect("reviews válidas");

        assert!(page.offline);
        assert_eq!(page.reviews.len(), 2);
        assert_eq!(page.reviews[0].id, "7");
        assert_eq!(page.reviews[0].author, "ana");
        assert_eq!(page.reviews[0].rating, Some(5.0));
        assert_eq!(page.reviews[0].hours, Some(12.5));
        assert_eq!(page.reviews[1].author, "Beto");
        assert_eq!(page.reviews[1].positive, Some(false));
        assert_eq!(page.pagination.offset, 4);
        assert!(page.pagination.has_more);
        assert_eq!(page.pagination.total, Some(9));

        let fallback = parse_community_reviews(json!({
            "ok": true,
            "items": [{ "id": 1, "text": "x" }]
        }))
        .expect("items funciona como fallback");
        assert_eq!(fallback.pagination.offset, 0);
        assert!(!fallback.pagination.has_more);
        assert!(!fallback.offline);
    }

    #[test]
    fn parses_community_collections_from_lists_and_counts_items() {
        let page = parse_community_collections(json!({
            "ok": true,
            "lists": [{
                "id": "c1",
                "title": "Favoritos",
                "description": "Lista pessoal",
                "visibility": "unlisted",
                "owner": { "username": "ana" },
                "item_count": 3,
                "items": [
                    { "appid": 10, "title": "A" },
                    { "appid": "20" }
                ]
            }],
            "pagination": { "limit": 1, "offset": 0, "has_more": false }
        }))
        .expect("coleções válidas");

        let collection = &page.collections[0];
        assert_eq!(collection.id, "c1");
        assert_eq!(collection.owner, "ana");
        assert_eq!(collection.visibility.as_deref(), Some("unlisted"));
        assert_eq!(collection.item_count, 3);
        assert_eq!(collection.items.len(), 2);
        assert_eq!(collection.items[0].appid, "10");
        assert_eq!(collection.items[1].appid, "20");
        assert!(!page.offline);
    }

    #[test]
    fn community_error_envelope_marks_offline_and_login() {
        let offline = parse_community_reviews(json!({
            "ok": false,
            "offline": true,
            "error": "offline",
            "code": "offline",
            "message": "connect refused"
        }))
        .expect_err("envelope de erro");
        assert!(offline.offline);
        assert!(offline.message.contains("Sem conexão"), "{}", offline.message);

        let login = parse_community_collections(json!({
            "ok": false,
            "error": "nao_logado",
            "code": "nao_logado"
        }))
        .expect_err("sem sessão");
        assert!(!login.offline);
        assert!(
            login.message.contains("Entre na sua conta"),
            "{}",
            login.message
        );
    }

    #[test]
    fn parses_review_and_collection_mutation_success() {
        let review = parse_community_review_mutation(json!({
            "ok": true,
            "review": { "id": 7, "text": "Bom", "rating": 5, "user_id": "u1" },
            "data": { "id": 7 }
        }))
        .expect("review salva");
        let review = review.review.expect("review no envelope");
        assert_eq!(review.id, "7");
        assert_eq!(review.user_id.as_deref(), Some("u1"));

        let collection = parse_community_collection_mutation(json!({
            "ok": true,
            "collection": { "id": "c1", "title": "Lista", "user_id": "u1" },
            "data": { "id": "c1" }
        }))
        .expect("coleção salva");
        let collection = collection.collection.expect("coleção no envelope");
        assert_eq!(collection.id, "c1");
        assert_eq!(collection.user_id.as_deref(), Some("u1"));

        assert!(parse_community_action(
            json!({ "ok": true }),
            "community:collection:item:add"
        )
        .is_ok());
        assert!(parse_community_action(
            json!({ "ok": true }),
            "community:collection:item:remove"
        )
        .is_ok());
        let item_login = parse_community_action(
            json!({ "ok": false, "error": "nao_logado", "code": "nao_logado" }),
            "community:collection:item:add",
        )
        .expect_err("sessão ausente no item");
        assert!(
            item_login.message.contains("Entre na sua conta"),
            "{}",
            item_login.message
        );
    }

    #[test]
    fn community_mutations_surface_login_offline_and_real_errors() {
        let login = parse_community_review_mutation(json!({
            "ok": false,
            "error": "nao_logado",
            "code": "nao_logado"
        }))
        .expect_err("sem sessão");
        assert!(login.message.contains("Entre na sua conta"), "{}", login.message);

        let offline = parse_community_action(
            json!({
                "ok": false,
                "offline": true,
                "error": "offline",
                "code": "offline",
                "message": "connect refused"
            }),
            "community:review:remove",
        )
        .expect_err("offline");
        assert!(offline.offline);
        assert!(offline.message.contains("Sem conexão"), "{}", offline.message);

        let backend = parse_community_collection_mutation(json!({
            "ok": false,
            "error": "sem_permissao",
            "code": "42501",
            "message": "sem permissão"
        }))
        .expect_err("erro real");
        assert_eq!(backend.message, "sem permissão");
        assert!(!backend.offline);
    }

    #[test]
    fn parses_friend_achievements_and_aggregates_summary() {
        let achievements = parse_friend_achievements(json!({
            "ok": true,
            "deCache": true,
            "achievements": [
                {
                    "appid": "10",
                    "apiname": "A",
                    "name": "Um",
                    "desc": "Primeira",
                    "icon": "//cdn.example/a.jpg",
                    "percent": 100,
                    "unlock": true
                },
                {
                    "appid": 20,
                    "key": "B",
                    "title": "Dois",
                    "percent": "50",
                    "unlock": 0
                },
                {
                    "appid": "20",
                    "apiname": "C",
                    "title": "Três",
                    "percent": 0,
                    "unlock": "1"
                },
                { "sem_appid": true }
            ]
        }))
        .expect("conquistas válidas");
        assert!(achievements.from_cache);
        assert_eq!(achievements.items.len(), 3);
        assert_eq!(
            achievements.items[0].icon.as_deref(),
            Some("https://cdn.example/a.jpg")
        );
        assert!(achievements.items[0].unlocked);
        assert!(!achievements.items[1].unlocked);
        assert!(achievements.items[2].unlocked);

        let summary = achievements.summary();
        assert_eq!(summary.total, 3);
        assert_eq!(summary.platinum, 1);
        assert_eq!(summary.games, 2);
        assert_eq!(summary.platinum_percent, Some(33));

        let empty = parse_friend_achievements(json!({
            "ok": true,
            "achievements": []
        }))
        .expect("vazio");
        assert_eq!(empty.summary().total, 0);
        assert_eq!(empty.summary().platinum_percent, None);

        let login = parse_friend_achievements(json!({
            "ok": false,
            "error": "nao_logado"
        }))
        .expect_err("sem sessão");
        assert!(login.contains("Entre na sua conta"), "{login}");

        let offline = parse_friend_achievements(json!({
            "ok": false,
            "error": "rede_indisponivel"
        }))
        .expect_err("offline");
        assert!(offline.contains("Sem conexão"), "{offline}");
    }

    #[test]
    fn parses_plugin_action_success_not_detected_and_error() {
        assert_eq!(
            parse_plugin_action(&json!({ "ok": true }), PluginAction::Install)
                .expect("sucesso"),
            PluginActionOutcome::Success
        );
        assert_eq!(
            parse_plugin_action(
                &json!({ "ok": false, "error": "not_detected" }),
                PluginAction::Install
            )
            .expect("não detectado"),
            PluginActionOutcome::NotDetected
        );
        let error = parse_plugin_action(
            &json!({ "ok": false, "error": "plugin_embutido" }),
            PluginAction::Unregister,
        )
        .expect_err("embutido não desregistra");
        assert!(error.contains("embutidos"), "{error}");
        assert!(parse_plugin_action(&json!({}), PluginAction::Enable).is_err());
    }

    #[test]
    fn parses_plugin_register_verify_and_details() {
        let registration = parse_plugin_register(json!({
            "ok": true,
            "plugin": { "id": "demo", "manifest": { "name": "Demo" } }
        }))
        .expect("registro válido");
        assert_eq!(registration.id, "demo");
        assert_eq!(registration.name, "Demo");

        let invalid = parse_plugin_register(json!({
            "ok": false,
            "errors": ["manifest_invalido"]
        }))
        .expect_err("manifesto inválido");
        assert!(invalid.contains("Manifesto"), "{invalid}");

        let verification = parse_plugin_verification(json!({
            "id": "demo",
            "ok": true,
            "valid": true,
            "verified": true,
            "declared": true,
            "source": "local"
        }))
        .expect("verificação válida");
        assert!(verification.valid);
        assert!(verification.verified);
        assert_eq!(verification.source, "local");

        let details = parse_plugin_details(json!({
            "ok": true,
            "plugin": {
                "id": "demo",
                "name": "Demo",
                "enabled": true,
                "installed": true,
                "valid": true,
                "source": "local",
                "manifest": {
                    "name": "Demo",
                    "version": "1.2.3",
                    "description": "Descrição",
                    "permissions": ["files"],
                    "signature": "sig",
                    "signingKeyId": "key"
                }
            }
        }))
        .expect("detalhes válidos");
        assert_eq!(details.version.as_deref(), Some("1.2.3"));
        assert_eq!(details.permissions, vec!["files"]);
        assert!(details.signed);

        let missing = parse_plugin_details(json!({
            "ok": false,
            "error": "plugin_desconhecido"
        }))
        .expect_err("plugin desconhecido");
        assert!(missing.contains("desconhecido"), "{missing}");
    }

    #[test]
    fn community_appid_strips_steam_prefix() {
        assert_eq!(community_appid("steam:10"), "10");
        assert_eq!(community_appid(" 10 "), "10");
        assert_eq!(community_appid("custom:1"), "custom:1");
    }

    #[test]
    fn parses_account_status_session_and_errors() {
        assert_eq!(
            parse_account_status(json!({ "session": null, "error": null })).unwrap(),
            None
        );

        let session = parse_account_status(json!({
            "session": {
                "user": {
                    "id": "user-1",
                    "email": "ana@example.test",
                    "username": "ana",
                    "access_token": "NAO-DEVE-VAZAR"
                }
            },
            "error": null
        }))
        .expect("sessão válida")
        .expect("sessão presente");
        assert_eq!(session.id, "user-1");
        assert_eq!(session.username.as_deref(), Some("ana"));
        assert_eq!(session.email.as_deref(), Some("ana@example.test"));
        assert_eq!(
            parse_account_status(json!({ "session": null, "error": "rede_indisponivel" }))
                .expect_err("erro explícito"),
            "rede_indisponivel"
        );
        assert!(parse_account_status(json!({ "session": { "user": {} } })).is_err());
    }

    #[test]
    fn parses_account_profile_fields_and_error() {
        let profile = parse_account_profile(json!({
            "ok": true,
            "profile": {
                "username": "ana",
                "avatar_url": "https://cdn.test/ana.png",
                "display_name": "Ana",
                "summary": "Só jogando",
                "country": "Brasil",
                "city": "Recife",
                "showcase": ["steam:10", " "],
                "background_url": null,
                "banner_url": "https://cdn.test/banner.png"
            }
        }))
        .expect("perfil válido");
        assert_eq!(profile.username, "ana");
        assert_eq!(profile.display(), "Ana");
        assert_eq!(profile.location().as_deref(), Some("Recife, Brasil"));
        assert_eq!(profile.showcase, vec!["steam:10"]);
        assert_eq!(
            profile.banner_url.as_deref(),
            Some("https://cdn.test/banner.png")
        );
        assert_eq!(
            parse_account_profile(json!({ "ok": false, "error": "nao_logado" }))
                .expect_err("erro de sessão"),
            "nao_logado"
        );
    }

    #[test]
    fn parses_auth_and_avatar_action_responses() {
        let auth = parse_auth_result(json!({
            "ok": true,
            "usernameReal": "ana_1"
        }))
        .expect("cadastro válido");
        assert_eq!(auth.username_real.as_deref(), Some("ana_1"));
        assert_eq!(
            parse_auth_result(json!({ "ok": false, "error": "credenciais_invalidas" }))
                .expect_err("credenciais"),
            "credenciais_invalidas"
        );
        assert_eq!(
            parse_avatar_upload(json!({ "ok": true, "avatar_url": "https://cdn.test/a.png" }))
                .expect("avatar"),
            "https://cdn.test/a.png"
        );
        assert_eq!(
            parse_avatar_upload(json!({ "ok": true, "url": "https://cdn.test/b.png" }))
                .expect("url alternativa"),
            "https://cdn.test/b.png"
        );
        assert!(parse_avatar_upload(json!({ "ok": false, "error": "avatar_grande" })).is_err());
    }

    #[test]
    fn parses_background_upload_response() {
        assert_eq!(
            parse_background_upload(json!({ "ok": true, "url": "https://cdn.test/bg.png" }))
                .expect("url do fundo"),
            "https://cdn.test/bg.png"
        );
        assert_eq!(
            parse_background_upload(json!({
                "ok": true,
                "background_url": "https://cdn.test/banner.png"
            }))
            .expect("background_url alternativa"),
            "https://cdn.test/banner.png"
        );
        assert_eq!(
            parse_background_upload(json!({ "ok": false, "error": "arquivo_grande" }))
                .expect_err("erro do IPC"),
            "arquivo_grande"
        );
        assert!(parse_background_upload(json!({ "ok": true })).is_err());
    }

    #[test]
    fn parses_friends_search_and_action_errors() {
        let results = parse_friends_search(json!({
            "ok": true,
            "results": [{
                "id": "u2",
                "username": "bia",
                "display_name": "Bia",
                "avatar_url": null,
                "status": "pending",
                "incoming": true
            }]
        }))
        .expect("busca válida");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].display(), "Bia");
        assert!(results[0].incoming);
        assert_eq!(results[0].status.as_deref(), Some("pending"));

        let error = parse_friends_search(json!({ "ok": false, "error": "nao_logado" }))
            .expect_err("sem sessão");
        assert!(error.contains("Entre na sua conta"), "{error}");
        assert!(parse_friends_action(json!({ "ok": true })).is_ok());
        assert!(parse_friends_action(json!({ "ok": false, "error": "destino_invalido" })).is_err());
    }

    #[test]
    fn parses_friend_profile_subset() {
        let profile = parse_friend_profile(json!({
            "ok": true,
            "profile": {
                "username": "bia",
                "display_name": "Bia",
                "summary": "Olá",
                "country": "Brasil",
                "city": "São Paulo"
            },
            "friends": [{ "id": "u1" }],
            "stats": { "games": 12, "playtime_hours": 34.6 }
        }))
        .expect("perfil do amigo");
        assert_eq!(profile.display(), "Bia");
        assert_eq!(profile.location().as_deref(), Some("São Paulo, Brasil"));
        assert_eq!(profile.games, Some(12));
        assert_eq!(profile.playtime_hours, Some(35));
        assert_eq!(profile.friends, Some(1));
    }
}
#[cfg(test)]
mod update_notification_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_packaged_state_snapshot() {
        let state = parse_packaged_state(&json!({
            "canal": "appimage",
            "suportado": true,
            "versaoAtual": "1.5.5",
            "versaoNova": "1.6.0",
            "tamanho": 52428800,
            "fase": "disponivel",
            "progresso": 0,
            "erro": null,
            "erroDeFundo": false,
            "erroAcao": null,
            "jaAvisado": false,
            "jogoRodando": true
        }))
        .expect("snapshot válido");

        assert_eq!(state.canal, "appimage");
        assert!(state.suportado);
        assert_eq!(state.versao_atual, "1.5.5");
        assert_eq!(state.versao_nova.as_deref(), Some("1.6.0"));
        assert_eq!(state.tamanho, Some(52_428_800));
        assert_eq!(state.fase, PackagedPhase::Disponivel);
        assert_eq!(state.versao_oferecida(), "1.6.0");
        assert!(state.jogo_rodando);
        assert!(!state.is_unsupported());

        let downloading = parse_packaged_state(&json!({
            "canal": "nsis",
            "suportado": true,
            "versaoAtual": "2.0.0",
            "versaoNova": "2.1.0",
            "fase": "baixando",
            "progresso": 42.0,
            "erroAcao": "baixar"
        }))
        .expect("download em andamento");
        assert_eq!(downloading.fase, PackagedPhase::Baixando);
        assert_eq!(downloading.progresso, 42);
        assert_eq!(downloading.erro_acao.as_deref(), Some("baixar"));
        assert!(downloading.fase.is_busy());
    }

    #[test]
    fn packaged_state_rejects_unknown_phase_and_missing_channel() {
        let error = parse_packaged_state(&json!({
            "canal": "appimage",
            "fase": "instalando"
        }))
        .unwrap_err();
        assert!(error.contains("fase"), "{error}");

        let error = parse_packaged_state(&json!({ "fase": "ocioso" })).unwrap_err();
        assert!(error.contains("canal"), "{error}");
        assert!(parse_packaged_state(&json!([])).is_err());
    }

    #[test]
    fn parses_update_state_for_git_channel() {
        let state = parse_update_state(&json!({
            "canal": "fonte",
            "suportado": true,
            "versao": "9cea844",
            "disponivel": false
        }))
        .expect("estado git válido");
        assert_eq!(state.canal, "fonte");
        assert!(state.suportado);
        assert_eq!(state.versao, "9cea844");
        assert!(!state.disponivel);
        assert!(parse_update_state(&json!("fonte")).is_err());
    }

    #[test]
    fn parses_git_update_check_with_commits_and_deps() {
        let check = parse_update_check(&json!({
            "ok": true,
            "disponivel": true,
            "atrasado": 2,
            "commits": [
                { "sha": "abc1234", "titulo": "feat: boot" },
                { "sha": "def5678", "titulo": "fix: update" },
                { "sha": " " , "titulo": "ignorado" }
            ],
            "depsMudaram": true
        }))
        .expect("check git válido");

        assert!(check.ok);
        assert!(check.disponivel);
        assert_eq!(check.atrasado, Some(2));
        assert_eq!(check.commits.len(), 2);
        assert_eq!(check.commits[0].sha, "abc1234");
        assert_eq!(check.commits[1].titulo, "fix: update");
        assert!(check.deps_mudaram);
        assert!(check.erro.is_none());

        // `update:check` sem `disponivel` deriva de `atrasado`.
        let derived = parse_update_check(&json!({
            "ok": true,
            "atrasado": 1,
            "commits": [{ "sha": "abc", "titulo": "x" }]
        }))
        .expect("check sem campo disponivel");
        assert!(derived.disponivel);
    }

    #[test]
    fn parses_packaged_check_and_action_errors() {
        let available = parse_update_check(&json!({
            "ok": true,
            "disponivel": true,
            "versao": "1.7.0",
            "tamanho": 100
        }))
        .expect("check empacotado");
        assert_eq!(available.versao.as_deref(), Some("1.7.0"));
        assert_eq!(available.tamanho, Some(100));

        let unsupported = parse_update_check(&json!({
            "ok": false,
            "disponivel": false,
            "motivo": "canal_nao_suportado"
        }))
        .expect("check sem suporte");
        assert!(!unsupported.ok);
        assert_eq!(unsupported.motivo.as_deref(), Some("canal_nao_suportado"));

        let failed = parse_update_check(&json!({
            "ok": false,
            "disponivel": false,
            "erro": "sem rede"
        }))
        .expect("check com erro");
        assert_eq!(failed.erro.as_deref(), Some("sem rede"));

        // O `git_channel` serializa a falha como `error`.
        let git_failed = parse_update_check(&json!({
            "ok": false,
            "error": "sem git"
        }))
        .expect("check git com erro");
        assert_eq!(git_failed.erro.as_deref(), Some("sem git"));

        let action = parse_update_action(&json!({ "ok": false, "erro": "sem_download" }))
            .expect("ação inválida");
        assert!(!action.ok);
        assert_eq!(action.erro.as_deref(), Some("sem_download"));
        assert!(parse_update_action(&json!(null)).is_err());
    }

    #[test]
    fn parses_boot_video_and_falls_back_without_bytes() {
        let video = parse_boot_video(&json!({
            "ok": true,
            "mime": "video/mp4",
            "data": [0, 1, 254, 255],
            "codecs": ["avc1.42E01E", " "]
        }))
        .expect("vídeo válido")
        .expect("vídeo presente");
        assert_eq!(video.mime, "video/mp4");
        assert_eq!(video.data, vec![0, 1, 254, 255]);
        assert_eq!(video.codecs, vec!["avc1.42E01E"]);

        assert!(parse_boot_video(&json!({ "ok": false }))
            .expect("sem vídeo é fallback")
            .is_none());
        assert!(parse_boot_video(&json!({ "ok": true, "data": [] }))
            .expect("bytes vazios são fallback")
            .is_none());
        assert!(parse_boot_video(&json!({ "ok": true, "data": [256] })).is_err());
        assert!(parse_boot_video(&json!({ "ok": true })).is_err());
    }

    #[test]
    fn parses_achievement_unlocked_payload() {
        let toast = parse_achievement_unlocked(&json!({
            "appid": "10",
            "key": "ACH_WIN",
            "title": "Conquista",
            "desc": "Descrição",
            "icon": "//cdn.test/icon.jpg",
            "percent": 12.5,
            "unlock": 1700000000
        }))
        .expect("payload válido");
        assert_eq!(toast.appid, "10");
        assert_eq!(toast.key, "ACH_WIN");
        assert_eq!(toast.title, "Conquista");
        assert_eq!(toast.desc.as_deref(), Some("Descrição"));
        assert_eq!(toast.icon.as_deref(), Some("https://cdn.test/icon.jpg"));
        assert_eq!(toast.percent, Some(12.5));
        assert_eq!(toast.unlock, Some(1_700_000_000));

        let fallback = parse_achievement_unlocked(&json!({
            "appid": "20",
            "apiname": "API_NAME",
            "name": "Sem title",
            "description": "Sem desc",
            "icon": ""
        }))
        .expect("fallback de apiname/name");
        assert_eq!(fallback.key, "API_NAME");
        assert_eq!(fallback.title, "Sem title");
        assert_eq!(fallback.desc.as_deref(), Some("Sem desc"));
        assert!(fallback.icon.is_none());

        assert!(parse_achievement_unlocked(&json!({ "appid": "10", "key": "X" })).is_none());
        assert!(parse_achievement_unlocked(&json!(null)).is_none());
    }
}
