//! Ponte entre o frontend iced e o dispatcher do `arcadia-ipc`.
//!
//! Toda chamada de IPC roda no runtime tokio dedicado do app, nunca na thread da
//! UI. O JSON devolvido por `library:get` é convertido aqui em [`Game`] — o view
//! model que as telas consomem. Capas são baixadas e decodificadas neste módulo
//! também, mas o cache fica em [`crate::covers`].

use std::collections::BTreeSet;
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

    pub fn playtime_label(&self) -> String {
        format_playtime(self.playtime_minutes)
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
    pub fn system_label(&self) -> String {
        let raw = self
            .system_id
            .as_deref()
            .or(self.platform.as_deref())
            .unwrap_or_default();
        retro_system_label(raw)
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
    pub fn date_label(&self) -> String {
        let raw = self.date.trim();
        if raw.is_empty() {
            return "Data não informada".to_string();
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

/// Snapshot de `LaunchRuntime::state_json` — a fonte de verdade do processo.
///
/// O shell nativo ainda não entrega os eventos `game:running`/`game:active`;
/// enquanto isso, o app consulta este estado em polling de baixa frequência.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LaunchSnapshot {
    pub running: bool,
    pub game_id: Option<String>,
    pub pid: Option<u32>,
}

pub fn parse_launch_snapshot(value: &Value) -> LaunchSnapshot {
    if value.get("running").and_then(Value::as_bool) != Some(true) {
        return LaunchSnapshot::default();
    }

    LaunchSnapshot {
        running: true,
        game_id: optional_text_field(value, "gameId"),
        pid: value
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|pid| u32::try_from(pid).ok()),
    }
}

/// Payload de `game:launch`, igual ao do `useGameActions.ts`:
/// `{ cmd, gameId, mode? }`. `mode` só entra quando a UI oferecer a escolha
/// Steam/exe (a próxima onda); launch options de Wine/Proton continuam
/// resolvidas pelo `arcadia-ipc`, não pelo frontend.
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

    /// Estado atual do processo, direto do runtime de launch.
    ///
    /// `state_json` já faz `try_wait`, então a consulta reflete um jogo que
    /// terminou sozinho — não é só o último resultado do nosso `game:launch`.
    pub fn launch_state(&self) -> LaunchSnapshot {
        parse_launch_snapshot(&self.state.launch.state_json())
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
        // Nenhum campo de launch options é inventado nesta onda.
        assert_eq!(
            launch_payload(&game, None),
            json!({
                "cmd": ["steam", "steam://run/10"],
                "gameId": "steam:10"
            })
        );

        // Ponto de extensão do seletor Steam/exe; ainda não usado pela UI.
        assert_eq!(
            launch_payload(&game, Some("exe")),
            json!({
                "cmd": ["steam", "steam://run/10"],
                "gameId": "steam:10",
                "mode": "exe"
            })
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
    fn launch_snapshot_reads_the_runtime_state() {
        assert_eq!(
            parse_launch_snapshot(&json!({ "running": false })),
            LaunchSnapshot::default()
        );

        assert_eq!(
            parse_launch_snapshot(&json!({
                "running": true,
                "gameId": "steam:10",
                "pid": 4242,
                "seconds": 12
            })),
            LaunchSnapshot {
                running: true,
                game_id: Some("steam:10".to_string()),
                pid: Some(4242),
            }
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
}
