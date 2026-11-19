//! Decisões puras do vigia de conquistas de jogos crackeados/emulados.
//!
//! Porte de `app/electron/achievements/cracked_watcher.js` sem timers, sem
//! `fs.watch` e sem `setInterval`: parsers de cada cracker, descoberta de
//! pastas/prefixos, schema do próprio emulador (`steam_settings`), casamento
//! com os itens do `achievements.json`, payload do toast e reconciliação UPC.
//!
//! Todo I/O passa por [`WatcherFs`] e todo "agora" passa por [`Clock`]; os
//! testes usam filesystem em memória e não observam nem dormem.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::fsx::{Clock, Platform, WatcherFs};
use crate::js::{
    first_truthy, js_number_str, js_number_value, js_string_value, js_trim, js_truthy_value,
};
use crate::r#match::{
    aliases_of, build_index, extract_appid_value, find_item_index, numeric_achievement_id,
    remember_alias,
};
use crate::uplay::{
    normalize_numeric_id_str, parse_upc, resolve_uplay_id, upc_runtime_map, UPLAY_SAVE_DIR,
    USUARIO_WINE,
};

pub const INTERVALO_POLL: i64 = 3000;
pub const ROOT_SECTION: &str = "__root__";
pub const IDIOMAS_SCHEMA: [&str; 5] = ["brazilian", "english", "spanish", "latam", "LATAM"];

/// Grupo/cracker que produziu o arquivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CrackGroup {
    Upc,
    Goldberg,
    Codex,
    Rune,
    Skidrow,
    Empress,
    OnlineFix,
    CreamApi,
    SmartSteamEmu,
    Razor1911,
    Rld,
    Flt,
    SteamData,
    ThreeDmGame,
}

impl CrackGroup {
    /// Nome de `provider` usado no payload/callback do oráculo.
    pub fn provider_name(self) -> &'static str {
        match self {
            Self::Upc => "upc",
            Self::Goldberg => "goldberg",
            Self::Codex => "codex",
            Self::Rune => "rune",
            Self::Skidrow => "skidrow",
            Self::Empress => "empress",
            Self::OnlineFix => "onlinefix",
            Self::CreamApi => "creamapi",
            Self::SmartSteamEmu => "sse",
            Self::Razor1911 => "razor",
            Self::Rld => "rld",
            Self::Flt => "flt",
            Self::SteamData => "steamdata",
            Self::ThreeDmGame => "3dmgame",
        }
    }

    pub fn from_provider_name(name: &str) -> Option<Self> {
        [
            Self::Upc,
            Self::Goldberg,
            Self::Codex,
            Self::Rune,
            Self::Skidrow,
            Self::Empress,
            Self::OnlineFix,
            Self::CreamApi,
            Self::SmartSteamEmu,
            Self::Razor1911,
            Self::Rld,
            Self::Flt,
            Self::SteamData,
            Self::ThreeDmGame,
        ]
        .into_iter()
        .find(|group| group.provider_name() == name)
    }
}

/// Detecta o cracker pelo caminho (mesma árvore de `caminhosPrefixados`).
pub fn detect_crack_group(path: &Path) -> Option<CrackGroup> {
    for component in path.components() {
        let text = component.as_os_str().to_string_lossy();
        let text = text.trim().to_lowercase();
        let group = match text.as_str() {
            "goldberg uplayemu saves" => CrackGroup::Upc,
            "gse saves" | "goldberg steamemu saves" => CrackGroup::Goldberg,
            "codex" => CrackGroup::Codex,
            "rune" => CrackGroup::Rune,
            "skidrow" => CrackGroup::Skidrow,
            "empress" => CrackGroup::Empress,
            "onlinefix" => CrackGroup::OnlineFix,
            "creamapi" => CrackGroup::CreamApi,
            "smartsteamemu" => CrackGroup::SmartSteamEmu,
            ".1911" => CrackGroup::Razor1911,
            "rld!" => CrackGroup::Rld,
            "flt" => CrackGroup::Flt,
            "steamdata" => CrackGroup::SteamData,
            "3dmgame" => CrackGroup::ThreeDmGame,
            _ => continue,
        };
        return Some(group);
    }
    None
}

// ───────────────────────────────────────────────────────────────────────────
// INI
// ───────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IniSection {
    pub name: String,
    pub entries: Vec<(String, String)>,
}

impl IniSection {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value.as_str())
    }

    fn set(&mut self, key: String, value: String) {
        if let Some((_, current)) = self
            .entries
            .iter_mut()
            .find(|(candidate, _)| *candidate == key)
        {
            *current = value;
        } else {
            self.entries.push((key, value));
        }
    }
}

/// INI com seções na ordem do arquivo e `__root__` para linhas sem seção.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IniFile {
    pub sections: Vec<IniSection>,
}

impl IniFile {
    pub fn parse(text: &str) -> Self {
        let mut file = IniFile::default();
        let mut current = ROOT_SECTION.to_string();
        for raw_line in text.split('\n') {
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line).trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line
                .strip_prefix('[')
                .and_then(|line| line.strip_suffix(']'))
            {
                // O JS só cria a seção quando aparece a primeira chave; uma
                // seção vazia não substitui o fallback root.
                current = name.to_string();
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim().to_string();
            let value = value.trim().to_string();
            if file.section(&current).is_none() {
                file.sections.push(IniSection {
                    name: current.clone(),
                    entries: Vec::new(),
                });
            }
            if let Some(section) = file.section_mut(&current) {
                section.set(key, value);
            }
        }
        file
    }

    pub fn section(&self, name: &str) -> Option<&IniSection> {
        self.sections.iter().find(|section| section.name == name)
    }

    pub fn section_mut(&mut self, name: &str) -> Option<&mut IniSection> {
        self.sections
            .iter_mut()
            .find(|section| section.name == name)
    }

    pub fn root(&self) -> Option<&IniSection> {
        self.section(ROOT_SECTION)
    }

    pub fn sections(&self) -> impl Iterator<Item = &IniSection> {
        self.sections.iter()
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.section(section)?.get(key)
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Parsers por cracker
// ───────────────────────────────────────────────────────────────────────────

/// Desbloqueio detectado num arquivo de crack (nome/id + ms).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DetectedUnlock {
    pub name: Option<String>,
    pub apiname: Option<String>,
    pub id: Option<String>,
    pub unlock_time_ms: i64,
}

/// `parseGoldbergJSON`: array cru ou objeto `{nome: estado}` com `name` injetado.
pub fn parse_goldberg_json(text: &str) -> Vec<Value> {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    match value {
        Value::Array(items) => items,
        Value::Object(object) => object
            .into_iter()
            .map(|(name, value)| {
                let mut output = Map::new();
                output.insert("name".into(), Value::String(name));
                if let Value::Object(value) = value {
                    for (key, value) in value {
                        output.insert(key, value);
                    }
                }
                Value::Object(output)
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `parseGoldberg`: só `earned` truthy; `earned_time` em segundos vira ms.
pub fn parse_goldberg(text: &str) -> Vec<DetectedUnlock> {
    parse_goldberg_json(text)
        .into_iter()
        .filter(|item| item.get("earned").map(js_truthy_value).unwrap_or(false))
        .map(|item| DetectedUnlock {
            name: item.get("name").and_then(|value| match value {
                Value::Null => None,
                other => Some(js_string_value(other)),
            }),
            apiname: None,
            id: None,
            unlock_time_ms: item
                .get("earned_time")
                .and_then(js_number_value)
                .filter(|value| value.is_finite())
                .map(|value| (value * 1000.0) as i64)
                .unwrap_or(0),
        })
        .collect()
}

fn seconds_or_millis(value: &str) -> i64 {
    let number = js_number_str(value).unwrap_or(0.0);
    if !number.is_finite() {
        return 0;
    }
    let millis = if number > 100_000_000_000.0 {
        number
    } else {
        number * 1000.0
    };
    millis.floor() as i64
}

/// CODEX/RUNE: `[ACH]\nAchieved=1\nUnlockTime=...`.
pub fn parse_codex(text: &str) -> Vec<DetectedUnlock> {
    let file = IniFile::parse(text);
    let mut output = Vec::new();
    for section in file.sections() {
        if section.get("Achieved") != Some("1") {
            continue;
        }
        let timestamp = first_truthy(section.get("UnlockTime"), section.get("TimeUnlocked"));
        output.push(DetectedUnlock {
            name: Some(section.name.clone()),
            apiname: None,
            id: None,
            unlock_time_ms: seconds_or_millis(timestamp),
        });
    }
    output
}

/// Skidrow: `[Achievements]\nNOME=1@NOME@timestamp`.
pub fn parse_skidrow(text: &str) -> Vec<DetectedUnlock> {
    let file = IniFile::parse(text);
    let section = file.section("Achievements").or_else(|| file.root());
    let Some(section) = section else {
        return Vec::new();
    };
    let mut output = Vec::new();
    for (name, value) in &section.entries {
        let parts: Vec<&str> = value.split('@').collect();
        if parts.first().copied() != Some("1") {
            continue;
        }
        let timestamp = parts
            .last()
            .and_then(|value| js_number_str(value))
            .unwrap_or(0.0);
        output.push(DetectedUnlock {
            name: Some(name.clone()),
            apiname: None,
            id: None,
            unlock_time_ms: if timestamp.is_finite() {
                (timestamp * 1000.0) as i64
            } else {
                0
            },
        });
    }
    output
}

/// Extrai `unlocked = true ... time = N` de um valor do SteamData.
fn parse_user_stats_value(value: &str) -> Option<i64> {
    let cleaned: String = value
        .chars()
        .filter(|character| !matches!(character, '"' | '{' | '}'))
        .collect();
    let lower = cleaned.to_lowercase();
    let mut search = 0usize;
    while let Some(offset) = lower[search..].find("unlocked") {
        let mut cursor = search + offset + "unlocked".len();
        if !skip_spaces_equals(&lower, &mut cursor) {
            search = cursor.max(search + 1);
            continue;
        }
        if !lower[cursor..].starts_with("true") {
            search = cursor.max(search + 1);
            continue;
        }
        cursor += 4;
        let rest = &lower[cursor..];
        let time_offset = rest.find("time")?;
        if rest[..time_offset].contains('\n') {
            search = cursor.max(search + 1);
            continue;
        }
        let mut time_cursor = cursor + time_offset + "time".len();
        if skip_spaces_equals(&lower, &mut time_cursor) {
            let digits: String = lower[time_cursor..]
                .chars()
                .take_while(|character| character.is_ascii_digit())
                .collect();
            if !digits.is_empty() {
                return digits.parse::<i64>().ok();
            }
        }
        search = cursor.max(search + 1);
    }
    None
}

fn skip_spaces_equals(text: &str, cursor: &mut usize) -> bool {
    while let Some(character) = text.get(*cursor..).and_then(|rest| rest.chars().next()) {
        if !character.is_whitespace() {
            break;
        }
        *cursor += character.len_utf8();
    }
    if text.get(*cursor..).map(|rest| rest.starts_with('=')) == Some(true) {
        *cursor += 1;
        while let Some(character) = text.get(*cursor..).and_then(|rest| rest.chars().next()) {
            if !character.is_whitespace() {
                break;
            }
            *cursor += character.len_utf8();
        }
        true
    } else {
        false
    }
}

/// SteamData: `[ACHIEVEMENTS]\nNOME={"unlocked = true, time = 123"}`.
pub fn parse_user_stats(text: &str) -> Vec<DetectedUnlock> {
    let file = IniFile::parse(text);
    let section = file
        .section("ACHIEVEMENTS")
        .or_else(|| file.section("achievements"));
    let Some(section) = section else {
        return Vec::new();
    };
    let mut output = Vec::new();
    for (name, value) in &section.entries {
        if let Some(seconds) = parse_user_stats_value(value) {
            output.push(DetectedUnlock {
                name: Some(name.clone()),
                apiname: None,
                id: None,
                unlock_time_ms: seconds * 1000,
            });
        }
    }
    output
}

/// OnlineFix/SmartSteamEmu: `achieved = true` + `timestamp`.
pub fn parse_online_fix(text: &str) -> Vec<DetectedUnlock> {
    let file = IniFile::parse(text);
    let mut output = Vec::new();
    for section in file.sections() {
        let achieved =
            section.get("achieved") == Some("true") || section.get("Achieved") == Some("true");
        if !achieved {
            continue;
        }
        let timestamp = first_truthy(section.get("timestamp"), section.get("TimeUnlocked"));
        output.push(DetectedUnlock {
            name: Some(section.name.clone()),
            apiname: None,
            id: None,
            unlock_time_ms: seconds_or_millis(timestamp),
        });
    }
    output
}

/// CreamAPI: `achieved=true` + `unlocktime`.
pub fn parse_cream_api(text: &str) -> Vec<DetectedUnlock> {
    let file = IniFile::parse(text);
    let mut output = Vec::new();
    for section in file.sections() {
        if section.get("achieved") != Some("true") {
            continue;
        }
        output.push(DetectedUnlock {
            name: Some(section.name.clone()),
            apiname: None,
            id: None,
            unlock_time_ms: seconds_or_millis(section.get("unlocktime").unwrap_or("")),
        });
    }
    output
}

/// RLD!: `State`/`Time` em hex little-endian (os 4 primeiros bytes).
pub fn parse_rld(text: &str) -> Vec<DetectedUnlock> {
    let file = IniFile::parse(text);
    let mut output = Vec::new();
    for section in file.sections() {
        if section.name == "Steam" {
            continue;
        }
        let Some(state) = section.get("State") else {
            continue;
        };
        let state = decode_hex(state);
        if state.len() < 4 || u32::from_le_bytes([state[0], state[1], state[2], state[3]]) != 1 {
            continue;
        }
        let time = section
            .get("Time")
            .map(decode_hex)
            .filter(|bytes| bytes.len() >= 4)
            .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            .unwrap_or(0);
        output.push(DetectedUnlock {
            name: Some(section.name.clone()),
            apiname: None,
            id: None,
            unlock_time_ms: i64::from(time) * 1000,
        });
    }
    output
}

fn decode_hex(text: &str) -> Vec<u8> {
    let clean: Vec<char> = text.trim().chars().collect();
    let mut output = Vec::new();
    let mut index = 0;
    while index + 1 < clean.len() {
        let pair: String = clean[index..index + 2].iter().collect();
        match u8::from_str_radix(&pair, 16) {
            Ok(value) => output.push(value),
            Err(_) => return output,
        }
        index += 2;
    }
    output
}

/// Razor1911: `NOME 1 timestamp` por linha.
pub fn parse_razor1911(text: &str) -> Vec<DetectedUnlock> {
    let mut output = Vec::new();
    for raw_line in text.split('\n') {
        let parts: Vec<&str> = raw_line.split_whitespace().collect();
        if parts.len() < 3 || parts[1] != "1" {
            continue;
        }
        let seconds = js_number_str(parts[2]).unwrap_or(0.0);
        output.push(DetectedUnlock {
            name: Some(parts[0].to_string()),
            apiname: None,
            id: None,
            unlock_time_ms: if seconds.is_finite() {
                (seconds * 1000.0) as i64
            } else {
                0
            },
        });
    }
    output
}

/// FLT: uma pasta com um arquivo por conquista; o mtime não traz carimbo.
pub fn parse_flt_file_names(names: &[String], now_ms: i64) -> Vec<DetectedUnlock> {
    names
        .iter()
        .map(|name| DetectedUnlock {
            name: Some(name.clone()),
            apiname: None,
            id: None,
            unlock_time_ms: now_ms,
        })
        .collect()
}

/// Despacha o parser pelo grupo. UPC usa o parser de `uplay`.
pub fn parse_crack_text(group: CrackGroup, text: &str, now_ms: i64) -> Vec<DetectedUnlock> {
    match group {
        CrackGroup::Upc => parse_upc(text)
            .into_iter()
            .map(|unlock| DetectedUnlock {
                name: Some(unlock.name),
                apiname: None,
                id: Some(unlock.id),
                unlock_time_ms: unlock.unlock_time,
            })
            .collect(),
        CrackGroup::Goldberg | CrackGroup::Empress => parse_goldberg(text),
        CrackGroup::Codex | CrackGroup::Rune | CrackGroup::ThreeDmGame => parse_codex(text),
        CrackGroup::Skidrow => parse_skidrow(text),
        CrackGroup::OnlineFix | CrackGroup::SmartSteamEmu => parse_online_fix(text),
        CrackGroup::CreamApi => parse_cream_api(text),
        CrackGroup::Rld => parse_rld(text),
        CrackGroup::Razor1911 => parse_razor1911(text),
        CrackGroup::SteamData => parse_user_stats(text),
        CrackGroup::Flt => {
            let _ = now_ms;
            Vec::new()
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Schema do próprio emulador (steam_settings/achievements.json)
// ───────────────────────────────────────────────────────────────────────────

/// `apiname minúsculo -> títulos` (todos os idiomas, deduplicados).
pub type EmulatorTitles = BTreeMap<String, Vec<String>>;

pub fn parse_emulator_schema(text: &str) -> Option<EmulatorTitles> {
    let value: Value = serde_json::from_str(text).ok()?;
    let items: Vec<Value> = match value {
        Value::Array(items) => items,
        Value::Object(object) => object.into_values().collect(),
        _ => Vec::new(),
    };
    let mut titles = EmulatorTitles::new();
    for item in items {
        let Some(object) = item.as_object() else {
            continue;
        };
        let name = ["name", "apiname"]
            .iter()
            .find_map(|key| object.get(*key).filter(|value| js_truthy_value(value)))
            .map(js_string_value)
            .unwrap_or_default();
        let name = js_trim(&name).to_string();
        if name.is_empty() {
            continue;
        }
        let display = ["displayName", "display_name"]
            .iter()
            .filter_map(|key| object.get(*key))
            .find(|value| value.is_object())
            .and_then(Value::as_object);
        let mut list: Vec<String> = Vec::new();
        let mut push = |value: Option<&Value>| {
            let Some(value) = value.filter(|value| !value.is_null()) else {
                return;
            };
            let text = js_trim(&js_string_value(value)).to_string();
            if !text.is_empty() && !list.contains(&text) {
                list.push(text);
            }
        };
        if let Some(display) = display {
            for language in IDIOMAS_SCHEMA {
                push(display.get(language));
            }
            for value in display.values() {
                push(Some(value));
            }
        }
        push(object.get("title"));
        if !list.is_empty() {
            titles.insert(name.to_lowercase(), list);
        }
    }
    Some(titles)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmulatorSchemaFile {
    pub path: PathBuf,
    pub mtime_ms: i64,
}

/// Sobe no máximo 5 diretórios a partir do exe procurando
/// `steam_settings/achievements.json` (layout do GSE na raiz do jogo).
pub fn find_emulator_schema(fs: &dyn WatcherFs, exe_dir: &Path) -> Option<EmulatorSchemaFile> {
    let mut dir = exe_dir.to_path_buf();
    for _ in 0..5 {
        let candidate = dir.join("steam_settings").join("achievements.json");
        if fs.is_file(&candidate) {
            let mtime_ms = fs.modified_ms(&candidate).unwrap_or(0);
            return Some(EmulatorSchemaFile {
                path: candidate,
                mtime_ms,
            });
        }
        let parent = dir.parent()?;
        if parent == dir {
            break;
        }
        if parent.parent().is_none() {
            break;
        }
        dir = parent.to_path_buf();
    }
    None
}

// ───────────────────────────────────────────────────────────────────────────
// Raízes/caminhos de crack
// ───────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct CrackEnvironment {
    pub home: PathBuf,
    pub appdata: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    pub public: Option<PathBuf>,
    pub program_data: Option<PathBuf>,
    /// Documents já resolvido (OneDrive/empresa); `None` usa `home/Documents`.
    pub documents: Option<PathBuf>,
}

impl CrackEnvironment {
    pub fn from_current() -> Self {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            home,
            appdata: std::env::var_os("APPDATA").map(PathBuf::from),
            local_app_data: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            public: std::env::var_os("PUBLIC").map(PathBuf::from),
            program_data: std::env::var_os("PROGRAMDATA").map(PathBuf::from),
            documents: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrackRoots {
    pub user: PathBuf,
    pub appdata: PathBuf,
    pub local_app_data: PathBuf,
    pub public_docs: PathBuf,
    pub program_data: PathBuf,
    pub user_docs: PathBuf,
}

/// Documents pode estar redirecionado (OneDrive/empresa); escolhe a primeira
/// pasta existente e cai no caminho clássico.
pub fn pasta_documentos_windows(fs: &dyn WatcherFs, home: &Path) -> PathBuf {
    let candidates = [
        home.join("Documents"),
        home.join("OneDrive").join("Documents"),
        home.join("OneDrive - Personal").join("Documents"),
    ];
    for candidate in &candidates {
        if fs.is_dir(candidate) {
            return candidate.clone();
        }
    }
    candidates[0].clone()
}

/// Mesma árvore do JS: prefixo Wine no Linux e pastas reais no Windows.
pub fn crack_roots(platform: Platform, prefix: &Path, env: &CrackEnvironment) -> CrackRoots {
    if platform.is_windows() {
        let appdata = env
            .appdata
            .clone()
            .unwrap_or_else(|| env.home.join("AppData").join("Roaming"));
        let local_app_data = env
            .local_app_data
            .clone()
            .unwrap_or_else(|| env.home.join("AppData").join("Local"));
        let public_docs = env
            .public
            .clone()
            .map(|path| path.join("Documents"))
            .unwrap_or_else(|| PathBuf::from("C:/Users/Public/Documents"));
        let program_data = env
            .program_data
            .clone()
            .unwrap_or_else(|| PathBuf::from("C:/ProgramData"));
        let user_docs = env
            .documents
            .clone()
            .unwrap_or_else(|| env.home.join("Documents"));
        return CrackRoots {
            user: appdata.clone(),
            appdata,
            local_app_data,
            public_docs,
            program_data,
            user_docs,
        };
    }
    let user = prefix.join("drive_c").join("users").join(USUARIO_WINE);
    CrackRoots {
        user: user.clone(),
        appdata: user.join("AppData").join("Roaming"),
        local_app_data: user.join("AppData").join("Local"),
        public_docs: prefix
            .join("drive_c")
            .join("users")
            .join("Public")
            .join("Documents"),
        program_data: prefix.join("drive_c").join("ProgramData"),
        user_docs: user.join("Documents"),
    }
}

pub fn numeric_uplay_dirs(fs: &dyn WatcherFs, root: &Path) -> Vec<String> {
    let Some(names) = fs.read_dir(root) else {
        return Vec::new();
    };
    names
        .into_iter()
        .filter(|name| {
            (1..=18).contains(&name.len())
                && name.bytes().all(|byte| byte.is_ascii_digit())
                && fs.is_dir(&root.join(name))
        })
        .filter_map(|name| normalize_numeric_id_str(&name))
        .collect()
}

/// IDs UPC a varrer: configurado primeiro; no Windows só enumera quando há ID
/// confirmado (pastas compartilhadas vazariam o ID de outro jogo).
pub fn uplay_ids_for_scan(
    fs: &dyn WatcherFs,
    platform: Platform,
    appid: &str,
    settings: &Value,
    entry: &Value,
    save_root: &Path,
) -> Vec<String> {
    let configured = resolve_uplay_id(appid, settings, entry);
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    if let Some(id) = configured.clone() {
        if seen.insert(id.clone()) {
            output.push(id);
        }
    }
    let enumerate = !platform.is_windows() || configured.is_some();
    if enumerate {
        for id in numeric_uplay_dirs(fs, save_root) {
            if seen.insert(id.clone()) {
                output.push(id);
            }
        }
    }
    output
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrackCandidate {
    pub group: CrackGroup,
    pub path: PathBuf,
}

/// Lista de caminhos na mesma ordem de `caminhosPrefixados` (+ SteamData/3DM).
pub fn crack_candidates(
    roots: &CrackRoots,
    appid: &str,
    uplay_ids: &[String],
    exe_dir: Option<&Path>,
) -> Vec<CrackCandidate> {
    let mut output = Vec::new();
    let mut push = |group: CrackGroup, path: PathBuf| {
        output.push(CrackCandidate { group, path });
    };
    for uplay_id in uplay_ids {
        push(
            CrackGroup::Upc,
            roots
                .appdata
                .join(UPLAY_SAVE_DIR)
                .join(uplay_id)
                .join("achievements.json"),
        );
    }
    let a = &roots.appdata;
    let pubd = &roots.public_docs;
    let docs = &roots.user_docs;
    let local = &roots.local_app_data;
    let prog = &roots.program_data;
    push(
        CrackGroup::Goldberg,
        a.join("GSE Saves").join(appid).join("achievements.json"),
    );
    push(
        CrackGroup::Goldberg,
        a.join("Goldberg SteamEmu Saves")
            .join(appid)
            .join("achievements.json"),
    );
    push(
        CrackGroup::Codex,
        pubd.join("Steam")
            .join("CODEX")
            .join(appid)
            .join("achievements.ini"),
    );
    push(
        CrackGroup::Codex,
        a.join("Steam")
            .join("CODEX")
            .join(appid)
            .join("achievements.ini"),
    );
    push(
        CrackGroup::Rune,
        pubd.join("Steam")
            .join("RUNE")
            .join(appid)
            .join("achievements.ini"),
    );
    push(
        CrackGroup::Skidrow,
        docs.join("SKIDROW")
            .join(appid)
            .join("SteamEmu")
            .join("UserStats")
            .join("achiev.ini"),
    );
    push(
        CrackGroup::Skidrow,
        docs.join("Player")
            .join(appid)
            .join("SteamEmu")
            .join("UserStats")
            .join("achiev.ini"),
    );
    push(
        CrackGroup::Skidrow,
        local
            .join("SKIDROW")
            .join(appid)
            .join("SteamEmu")
            .join("UserStats")
            .join("achiev.ini"),
    );
    push(
        CrackGroup::Empress,
        a.join("EMPRESS")
            .join("remote")
            .join(appid)
            .join("achievements.json"),
    );
    push(
        CrackGroup::Empress,
        pubd.join("EMPRESS")
            .join(appid)
            .join("remote")
            .join(appid)
            .join("achievements.json"),
    );
    push(
        CrackGroup::OnlineFix,
        pubd.join("OnlineFix")
            .join(appid)
            .join("Stats")
            .join("Achievements.ini"),
    );
    push(
        CrackGroup::OnlineFix,
        pubd.join("OnlineFix").join(appid).join("Achievements.ini"),
    );
    push(
        CrackGroup::CreamApi,
        a.join("CreamAPI")
            .join(appid)
            .join("stats")
            .join("CreamAPI.Achievements.cfg"),
    );
    push(
        CrackGroup::SmartSteamEmu,
        a.join("SmartSteamEmu")
            .join(appid)
            .join("User")
            .join("Achievements.ini"),
    );
    push(
        CrackGroup::Razor1911,
        a.join(".1911").join(appid).join("achievement"),
    );
    push(
        CrackGroup::Rld,
        prog.join("RLD!").join(appid).join("achievements.ini"),
    );
    push(
        CrackGroup::Rld,
        prog.join("Steam")
            .join("Player")
            .join(appid)
            .join("stats")
            .join("achievements.ini"),
    );
    push(
        CrackGroup::Rld,
        prog.join("Steam")
            .join("RLD!")
            .join(appid)
            .join("stats")
            .join("achievements.ini"),
    );
    push(CrackGroup::Flt, a.join("FLT").join(appid));
    if let Some(exe_dir) = exe_dir {
        push(
            CrackGroup::SteamData,
            exe_dir.join("SteamData").join("user_stats.ini"),
        );
        push(
            CrackGroup::ThreeDmGame,
            exe_dir
                .join("3DMGAME")
                .join("Player")
                .join("stats")
                .join("achievements.ini"),
        );
    }
    output
}

// ───────────────────────────────────────────────────────────────────────────
// Configuração/biblioteca (sem I/O)
// ───────────────────────────────────────────────────────────────────────────

pub fn settings_for_entry(settings: &Value, entry: &Value, appid: &str) -> Value {
    let key = entry
        .get("id")
        .filter(|value| js_truthy_value(value))
        .map(js_string_value)
        .unwrap_or_else(|| format!("steam:{appid}"));
    let direct = settings
        .get(&key)
        .filter(|value| js_truthy_value(value))
        .or_else(|| settings.get(appid).filter(|value| js_truthy_value(value)));
    direct.cloned().unwrap_or_else(|| Value::Object(Map::new()))
}

pub fn resolve_prefix(
    appid: &str,
    settings: &Value,
    _entry: &Value,
    compatdata: &Path,
    prefix_of: Option<&str>,
) -> PathBuf {
    for key in ["prefixPath", "winePrefixPath"] {
        if let Some(prefix) = settings.get(key).filter(|value| js_truthy_value(value)) {
            return PathBuf::from(js_string_value(prefix));
        }
    }
    if let Some(prefix) = prefix_of.filter(|value| !value.trim().is_empty()) {
        return PathBuf::from(prefix);
    }
    compatdata.join(appid).join("pfx")
}

pub fn resolve_exe_dir(settings: &Value, entry: &Value) -> Option<PathBuf> {
    let exe = settings
        .get("exePath")
        .filter(|value| js_truthy_value(value))
        .or_else(|| entry.get("exe").filter(|value| js_truthy_value(value)))?;
    let exe = PathBuf::from(js_string_value(exe));
    exe.parent().map(Path::to_path_buf)
}

pub fn extrair_appid(entry: &Value) -> Option<String> {
    extract_appid_value(entry)
}

/// `lerLibrary`: une `library.json` e `game_settings.json` (stubs instalados).
pub fn library_games(library: &Value, settings: &Value) -> Vec<Value> {
    let mut games: Vec<Value> = library
        .get("games")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut by_id: HashMap<String, usize> = HashMap::new();
    for (index, game) in games.iter().enumerate() {
        if let Some(id) = game.get("id").filter(|value| js_truthy_value(value)) {
            by_id.insert(js_string_value(id), index);
        }
    }
    let Some(settings) = settings.as_object() else {
        return games;
    };
    for (id, config) in settings {
        let Some(appid) = id.strip_prefix("steam:") else {
            continue;
        };
        if appid.is_empty() || !appid.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let has_config = ["exePath", "prefixPath", "uplayId", "upcId"]
            .iter()
            .any(|key| config.get(*key).map(js_truthy_value).unwrap_or(false));
        if !has_config {
            continue;
        }
        if let Some(index) = by_id.get(id) {
            if let Some(game) = games.get_mut(*index).and_then(Value::as_object_mut) {
                game.insert("installed".into(), Value::Bool(true));
            }
        } else {
            let mut game = Map::new();
            game.insert("id".into(), Value::String(id.clone()));
            game.insert("installed".into(), Value::Bool(true));
            if let Some(exe) = config.get("exePath") {
                game.insert("exe".into(), exe.clone());
            }
            games.push(Value::Object(game));
            by_id.insert(id.clone(), games.len() - 1);
        }
    }
    games
}

// ───────────────────────────────────────────────────────────────────────────
// Casamento com os itens e payload
// ───────────────────────────────────────────────────────────────────────────

/// Resolve o item do Arcadia para um desbloqueio.
///
/// UPC: título exato primeiro e somente depois o ID numérico (a ordem do
/// runtime não corresponde ao apiname). Demais: apiname/apelido; por fim a
/// ponte por título do schema do emulador, que só casa título ÚNICO.
pub fn find_item_for_unlock(
    items: &[Value],
    unlock: &DetectedUnlock,
    group: CrackGroup,
    emulator_titles: Option<&EmulatorTitles>,
) -> Option<usize> {
    if group == CrackGroup::Upc {
        let name = unlock
            .name
            .as_deref()
            .map(|name| name.trim().to_lowercase())
            .unwrap_or_default();
        if !name.is_empty() {
            if let Some(position) = items.iter().position(|item| {
                item.get("title")
                    .filter(|value| js_truthy_value(value))
                    .map(|value| js_string_value(value).trim().to_lowercase() == name)
                    .unwrap_or(false)
            }) {
                return Some(position);
            }
        }
        if let Some(id) = unlock.id.as_deref().and_then(numeric_achievement_id) {
            if let Some(position) = items.iter().position(|item| {
                crate::uplay::item_uplay_id(item, None, false)
                    .map(|item_id| item_id == id)
                    .unwrap_or(false)
            }) {
                return Some(position);
            }
        }
    }
    let index = build_index(items);
    for name in [&unlock.name, &unlock.apiname, &unlock.id]
        .into_iter()
        .flatten()
    {
        // O `byName` do JS só aplica `toLowerCase`, sem trim.
        let key = name.to_lowercase();
        if key.is_empty() {
            continue;
        }
        if let Some(position) = index.by_apiname(&key) {
            return Some(position);
        }
    }
    if let Some(titles) = emulator_titles {
        for name in [&unlock.name, &unlock.apiname, &unlock.id]
            .into_iter()
            .flatten()
        {
            let key = name.trim().to_lowercase();
            if key.is_empty() {
                continue;
            }
            for title in titles.get(&key).into_iter().flatten() {
                if let Some(position) = find_item_index(&index, items, None, Some(title)) {
                    return Some(position);
                }
            }
        }
    }
    None
}

/// Payload entregue ao callback de toast.
#[derive(Debug, Clone, PartialEq)]
pub struct UnlockPayload {
    pub appid: String,
    pub key: String,
    pub apiname: String,
    pub provider: String,
    pub title: String,
    pub desc: String,
    pub icon: String,
    pub percent: f64,
    pub unlock: i64,
}

impl UnlockPayload {
    pub fn to_value(&self) -> Value {
        let mut object = Map::new();
        object.insert("appid".into(), Value::String(self.appid.clone()));
        object.insert("key".into(), Value::String(self.key.clone()));
        object.insert("apiname".into(), Value::String(self.apiname.clone()));
        object.insert("provider".into(), Value::String(self.provider.clone()));
        object.insert("title".into(), Value::String(self.title.clone()));
        object.insert("desc".into(), Value::String(self.desc.clone()));
        object.insert("icon".into(), Value::String(self.icon.clone()));
        object.insert(
            "percent".into(),
            serde_json::Number::from_f64(self.percent)
                .map(Value::Number)
                .unwrap_or_else(|| Value::from(0)),
        );
        object.insert("unlock".into(), Value::from(self.unlock));
        Value::Object(object)
    }
}

pub fn payload_for_unlock(
    appid: &str,
    item: &Value,
    unlock: &DetectedUnlock,
    group: CrackGroup,
    now_ms: i64,
) -> UnlockPayload {
    let key = item
        .get("block")
        .filter(|value| !value.is_null())
        .zip(item.get("bit").filter(|value| !value.is_null()))
        .map(|(block, bit)| format!("{}|{}", js_string_value(block), js_string_value(bit)))
        .or_else(|| {
            item.get("apiname")
                .filter(|value| js_truthy_value(value))
                .map(js_string_value)
        })
        .or_else(|| {
            unlock
                .id
                .as_ref()
                .filter(|value| !value.is_empty())
                .cloned()
        })
        .unwrap_or_default();
    let text = |field: &str| {
        item.get(field)
            .filter(|value| js_truthy_value(value))
            .map(js_string_value)
            .unwrap_or_default()
    };
    let seconds = (unlock.unlock_time_ms as f64 / 1000.0).floor() as i64;
    let unlock_seconds = if seconds != 0 {
        seconds
    } else {
        (now_ms as f64 / 1000.0).floor() as i64
    };
    UnlockPayload {
        appid: appid.to_string(),
        key,
        apiname: text("apiname"),
        provider: group.provider_name().to_string(),
        title: text("title"),
        desc: text("desc"),
        icon: text("icon"),
        percent: item
            .get("percent")
            .and_then(js_number_value)
            .filter(|value| value.is_finite())
            .unwrap_or(0.0),
        unlock: unlock_seconds,
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ApplyResult {
    pub payloads: Vec<UnlockPayload>,
    pub changed: bool,
}

/// Aplica desbloqueios aos itens: memoriza alias/remoteApiname, marca uma vez
/// e devolve um payload por conquista NOVA (itens já achieved não repetem).
pub fn apply_unlocks(
    items: &mut [Value],
    appid: &str,
    group: CrackGroup,
    unlocks: &[DetectedUnlock],
    emulator_titles: Option<&EmulatorTitles>,
    now_ms: i64,
) -> ApplyResult {
    let mut result = ApplyResult::default();
    for unlock in unlocks {
        let Some(position) = find_item_for_unlock(items, unlock, group, emulator_titles) else {
            continue;
        };
        let Some(item) = items.get_mut(position) else {
            continue;
        };
        if let Some(name) = unlock
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            let known = aliases_of(item)
                .iter()
                .any(|alias| alias.to_lowercase() == name.to_lowercase());
            if !known {
                let has_remote = item
                    .get("remoteApiname")
                    .map(js_truthy_value)
                    .unwrap_or(false);
                if !has_remote {
                    if let Some(object) = item.as_object_mut() {
                        object.insert("remoteApiname".into(), Value::String(name.to_string()));
                    }
                }
                remember_alias(item, name);
                result.changed = true;
            }
        }
        if item.get("achieved").map(js_truthy_value).unwrap_or(false) {
            continue;
        }
        let payload = payload_for_unlock(appid, item, unlock, group, now_ms);
        if let Some(object) = item.as_object_mut() {
            object.insert("achieved".into(), Value::Bool(true));
            object.insert("unlock".into(), Value::from(payload.unlock));
        }
        result.changed = true;
        result.payloads.push(payload);
    }
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revocation {
    pub appid: String,
    pub apiname: String,
}

/// Reconciliação UPC: runtime completo e ativo + permissão de revogar + item
/// `localUnlock=true` + título com `earned=false` → desfaz o desbloqueio.
pub fn reconcile_upc(
    items: &mut [Value],
    appid: &str,
    content: &str,
    allow_revoke: bool,
) -> Vec<Revocation> {
    let Some(runtime) = upc_runtime_map(content) else {
        return Vec::new();
    };
    if runtime.len() < items.len() {
        return Vec::new();
    }
    if !runtime.values().any(|entry| entry.earned) {
        return Vec::new();
    }
    if !allow_revoke {
        return Vec::new();
    }
    let mut revocations = Vec::new();
    for item in items.iter_mut() {
        let Some(object) = item.as_object_mut() else {
            continue;
        };
        if !object.get("achieved").map(js_truthy_value).unwrap_or(false) {
            continue;
        }
        if object.get("localUnlock") != Some(&Value::Bool(true)) {
            continue;
        }
        let title = ["title", "name"]
            .iter()
            .find_map(|key| object.get(*key).filter(|value| js_truthy_value(value)))
            .map(js_string_value)
            .unwrap_or_default();
        let title = title.trim().to_lowercase();
        if title.is_empty() {
            continue;
        }
        let Some(runtime_entry) = runtime.get(&title) else {
            continue;
        };
        if runtime_entry.earned {
            continue;
        }
        object.insert("achieved".into(), Value::Bool(false));
        object.insert("unlock".into(), Value::from(0));
        object.insert("revoked".into(), Value::Bool(true));
        if let Some(apiname) = object
            .get("apiname")
            .filter(|value| js_truthy_value(value))
            .map(js_string_value)
        {
            revocations.push(Revocation {
                appid: appid.to_string(),
                apiname,
            });
        }
    }
    revocations
}

// ───────────────────────────────────────────────────────────────────────────
// Estado do watcher (scans sob demanda, sem timers)
// ───────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CrackScanReport {
    pub appid: String,
    pub unlocks: Vec<UnlockPayload>,
    pub revocations: Vec<Revocation>,
    pub changed: bool,
}

/// Entrada de um scan (referências para não acumular clones por jogo).
pub struct ScanGameOptions<'a> {
    pub fs: &'a dyn WatcherFs,
    pub clock: &'a dyn Clock,
    pub appid: &'a str,
    pub entry: &'a Value,
    pub settings: &'a Value,
    pub platform: Platform,
    pub prefix: &'a Path,
    pub env: &'a CrackEnvironment,
    pub allow_revoke: bool,
}

/// Cache de mtime e do schema do emulador. Uma instância equivale a uma conta;
/// `scan_game` pode ser chamado quando o chamador quiser (nada é agendado).
#[derive(Debug, Clone, Default)]
pub struct CrackedWatcher {
    pub account: String,
    mtimes: BTreeMap<String, i64>,
    emulator_cache: BTreeMap<PathBuf, (i64, EmulatorTitles)>,
}

impl CrackedWatcher {
    pub fn new(account: impl Into<String>) -> Self {
        Self {
            account: account.into(),
            ..Self::default()
        }
    }

    fn cache_key(&self, path: &Path) -> String {
        format!("{}\0{}", self.account, path.to_string_lossy())
    }

    /// Primeira leitura sempre processa; depois, só se o mtime mudou.
    pub fn should_process(&mut self, path: &Path, mtime_ms: i64) -> bool {
        let key = self.cache_key(path);
        if self.mtimes.get(&key) == Some(&mtime_ms) {
            return false;
        }
        self.mtimes.insert(key, mtime_ms);
        true
    }

    pub fn emulator_titles(
        &mut self,
        fs: &dyn WatcherFs,
        exe_dir: &Path,
    ) -> Option<EmulatorTitles> {
        let found = find_emulator_schema(fs, exe_dir)?;
        if let Some((mtime, titles)) = self.emulator_cache.get(&found.path) {
            if *mtime == found.mtime_ms {
                return Some(titles.clone());
            }
        }
        let text = fs.read_to_string(&found.path)?;
        let titles = parse_emulator_schema(&text)?;
        self.emulator_cache
            .insert(found.path, (found.mtime_ms, titles.clone()));
        Some(titles)
    }

    /// Varre os candidatos de UM jogo e aplica as decisões aos itens.
    pub fn scan_game(
        &mut self,
        options: &ScanGameOptions<'_>,
        items: &mut [Value],
    ) -> CrackScanReport {
        let fs = options.fs;
        let clock = options.clock;
        let appid = options.appid;
        let entry = options.entry;
        let platform = options.platform;
        let prefix = options.prefix;
        let env = options.env;
        let allow_revoke = options.allow_revoke;
        if items.is_empty() || (!platform.is_windows() && !fs.exists(prefix)) {
            return CrackScanReport {
                appid: appid.to_string(),
                ..CrackScanReport::default()
            };
        }
        // No Windows o JS resolve Documents (OneDrive/empresa) por existência;
        // o chamador pode trazer `documents` pronto, senão resolvemos aqui.
        let mut resolved_env = env.clone();
        if platform.is_windows() && resolved_env.documents.is_none() {
            resolved_env.documents = Some(pasta_documentos_windows(fs, &resolved_env.home));
        }
        let roots = crack_roots(platform, prefix, &resolved_env);
        let settings = settings_for_entry(options.settings, entry, appid);
        let save_root = roots.appdata.join(UPLAY_SAVE_DIR);
        let uplay_ids = uplay_ids_for_scan(fs, platform, appid, &settings, entry, &save_root);
        let exe_dir = resolve_exe_dir(&settings, entry);
        let emulator_titles = exe_dir
            .as_deref()
            .and_then(|dir| self.emulator_titles(fs, dir));
        let candidates = crack_candidates(&roots, appid, &uplay_ids, exe_dir.as_deref());
        let now_ms = clock.now_ms();
        let mut report = CrackScanReport {
            appid: appid.to_string(),
            ..CrackScanReport::default()
        };
        for candidate in candidates {
            if !fs.exists(&candidate.path) {
                continue;
            }
            let Some(mtime) = fs.modified_ms(&candidate.path) else {
                continue;
            };
            if !self.should_process(&candidate.path, mtime) {
                continue;
            }
            if candidate.group == CrackGroup::Flt {
                let Some(names) = fs.read_dir(&candidate.path) else {
                    continue;
                };
                let unlocks = parse_flt_file_names(&names, now_ms);
                let applied = apply_unlocks(items, appid, CrackGroup::Flt, &unlocks, None, now_ms);
                report.changed |= applied.changed;
                report.unlocks.extend(applied.payloads);
                continue;
            }
            let Some(content) = fs.read_to_string(&candidate.path) else {
                continue;
            };
            if candidate.group == CrackGroup::Upc {
                let revocations = reconcile_upc(items, appid, &content, allow_revoke);
                if !revocations.is_empty() {
                    report.changed = true;
                    report.revocations.extend(revocations);
                }
            }
            let unlocks = parse_crack_text(candidate.group, &content, now_ms);
            if unlocks.is_empty() {
                continue;
            }
            // SteamData/3DMGAME são varridos à parte no JS, somente pelo
            // índice de apiname — o schema do emulador vale só para os
            // registros das pastas de crack.
            let titles = match candidate.group {
                CrackGroup::SteamData | CrackGroup::ThreeDmGame => None,
                _ => emulator_titles.as_ref(),
            };
            let applied = apply_unlocks(items, appid, candidate.group, &unlocks, titles, now_ms);
            report.changed |= applied.changed;
            report.unlocks.extend(applied.payloads);
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsx::Clock;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct FakeFs {
        files: BTreeMap<PathBuf, String>,
        dirs: BTreeMap<PathBuf, Vec<String>>,
        mtimes: BTreeMap<PathBuf, i64>,
    }

    impl FakeFs {
        fn file(&mut self, path: impl Into<PathBuf>, content: &str) {
            let path = path.into();
            self.mtimes.insert(path.clone(), 1);
            self.files.insert(path, content.to_string());
        }

        fn dir(&mut self, path: impl Into<PathBuf>, names: &[&str]) {
            let path = path.into();
            if let Some(parent) = path.parent() {
                if let (Some(grand), Some(name)) = (parent.parent(), path.file_name()) {
                    let entries = self.dirs.entry(grand.to_path_buf()).or_default();
                    let name = name.to_string_lossy().into_owned();
                    if !entries.contains(&name) {
                        entries.push(name);
                    }
                }
            }
            self.dirs
                .insert(path, names.iter().map(|name| (*name).to_string()).collect());
        }
    }

    impl WatcherFs for FakeFs {
        fn exists(&self, path: &Path) -> bool {
            self.files.contains_key(path) || self.dirs.contains_key(path)
        }
        fn is_file(&self, path: &Path) -> bool {
            self.files.contains_key(path)
        }
        fn is_dir(&self, path: &Path) -> bool {
            self.dirs.contains_key(path)
        }
        fn read_to_string(&self, path: &Path) -> Option<String> {
            self.files.get(path).cloned()
        }
        fn read_dir(&self, path: &Path) -> Option<Vec<String>> {
            self.dirs.get(path).cloned()
        }
        fn modified_ms(&self, path: &Path) -> Option<i64> {
            self.mtimes.get(path).copied()
        }
    }

    struct FixedClock(i64);
    impl Clock for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    fn items() -> Vec<Value> {
        vec![
            json!({ "apiname": "ACH01", "title": "Primeiros passos", "desc": "d1", "block": 1, "bit": 0, "achieved": false, "unlock": 0, "percent": 0 }),
            json!({ "apiname": "ACH02", "title": "Com molho extra", "desc": "d2", "block": 1, "bit": 1, "achieved": false, "unlock": 0, "percent": 0 }),
        ]
    }

    #[test]
    fn parse_ini_supports_sections_root_comments_and_overwrite() {
        let file = IniFile::parse("root=1\n; c\n# c\n[Sec]\nA=1\nA=2\nNoEqual\n[B]\nX = y\n");
        assert_eq!(file.get(ROOT_SECTION, "root"), Some("1"));
        assert_eq!(file.get("Sec", "A"), Some("2"));
        assert_eq!(file.get("B", "X"), Some("y"));
        assert_eq!(file.get("Sec", "NoEqual"), None);
    }

    #[test]
    fn goldberg_reads_array_and_object_and_injects_the_key_as_name() {
        let array = parse_goldberg(
            &json!([
                { "name": "ACH01", "earned": true, "earned_time": 1700000000 },
                { "name": "ACH02", "earned": 0, "earned_time": 0 }
            ])
            .to_string(),
        );
        assert_eq!(array.len(), 1);
        assert_eq!(array[0].name.as_deref(), Some("ACH01"));
        assert_eq!(array[0].unlock_time_ms, 1_700_000_000_000);

        let object =
            parse_goldberg(&json!({ "ACH19": { "earned": true, "earned_time": 5 } }).to_string());
        assert_eq!(object[0].name.as_deref(), Some("ACH19"));
        assert_eq!(object[0].unlock_time_ms, 5000);

        let overridden =
            parse_goldberg_json(&json!({ "KEY": { "name": "REAL", "earned": true } }).to_string());
        assert_eq!(overridden[0]["name"], json!("REAL"));
        assert!(parse_goldberg_json("not-json").is_empty());
        assert!(parse_goldberg_json("1").is_empty());
    }

    #[test]
    fn codex_and_rune_parse_seconds_and_milliseconds() {
        let text = "[ACH01]\nAchieved=1\nUnlockTime=1700000000\n[ACH02]\nAchieved=0\nUnlockTime=0\n[ACH03]\nAchieved=1\nTimeUnlocked=1700000000000\n";
        let found = parse_codex(text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name.as_deref(), Some("ACH01"));
        assert_eq!(found[0].unlock_time_ms, 1_700_000_000_000);
        assert_eq!(found[1].unlock_time_ms, 1_700_000_000_000);
        assert_eq!(parse_crack_text(CrackGroup::Rune, text, 0), found);
    }

    #[test]
    fn skidrow_reads_section_or_root_and_last_part_timestamp() {
        let found = parse_skidrow("[Achievements]\nACH01=1@ACH01@1700000000\nACH02=0@ACH02@0\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name.as_deref(), Some("ACH01"));
        assert_eq!(found[0].unlock_time_ms, 1_700_000_000_000);
        let root = parse_skidrow("ACH03=1\n");
        assert_eq!(root[0].unlock_time_ms, 1000, "última parte é o próprio 1");
        let root_with_empty_section = parse_skidrow("ACH04=1@ACH04@1700000000\n[Achievements]\n");
        assert_eq!(
            root_with_empty_section.len(),
            1,
            "seção vazia não substitui o root (objeto vazio é ausente)"
        );
    }

    #[test]
    fn user_stats_extracts_unlocked_true_and_time() {
        let text = "[ACHIEVEMENTS]\nACH01={\"unlocked = true, time = 1700000000\"}\nACH02={\"unlocked = false, time = 0\"}\n";
        let found = parse_user_stats(text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name.as_deref(), Some("ACH01"));
        assert_eq!(found[0].unlock_time_ms, 1_700_000_000_000);
        assert!(parse_user_stats("[ACHIEVEMENTS]\nA=unlocked=true\n").is_empty());
    }

    #[test]
    fn onlinefix_and_creamapi_require_exact_true_values() {
        let online = parse_online_fix(
            "[ACH01]\nAchieved=true\nTimeUnlocked=1700000000\n[ACH02]\nAchieved=false\n",
        );
        assert_eq!(online.len(), 1);
        assert_eq!(online[0].name.as_deref(), Some("ACH01"));
        assert_eq!(online[0].unlock_time_ms, 1_700_000_000_000);
        assert!(parse_online_fix("[ACH01]\nAchieved=True\n").is_empty());
        let direct = parse_online_fix("[ACH01]\nachieved=true\ntimestamp=1700000000000\n");
        assert_eq!(direct[0].unlock_time_ms, 1_700_000_000_000);
        let cream = parse_cream_api(
            "[ACH01]\nachieved=true\nunlocktime=1700000000\n[ACH02]\nachieved=false\n",
        );
        assert_eq!(cream.len(), 1);
        assert_eq!(cream[0].unlock_time_ms, 1_700_000_000_000);
        assert!(parse_cream_api("[ACH01]\nachieved=1\n").is_empty());
    }

    #[test]
    fn rld_decodes_state_and_time_little_endian_hex() {
        let state = "01000000";
        let time = "00F15365"; // 1700000000 em little-endian
        let text = format!("[ACH01]\nState={state}\nTime={time}\n[Steam]\nState={state}\n[ACH02]\nState=00000000\n");
        let found = parse_rld(&text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name.as_deref(), Some("ACH01"));
        assert_eq!(found[0].unlock_time_ms, 1_700_000_000_000);
        assert!(parse_rld("[ACH01]\nState=zz\n").is_empty());
        assert!(parse_rld("[ACH01]\nTime=01000000\n").is_empty());
    }

    #[test]
    fn razor_and_flt_parsers_match_the_oracle() {
        let found = parse_razor1911("ACH01 1 1700000000\nACH02 0 0\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].unlock_time_ms, 1_700_000_000_000);
        let flt = parse_flt_file_names(&["ACH01".to_string(), "ACH02".to_string()], 999);
        assert_eq!(flt.len(), 2);
        assert_eq!(flt[0].unlock_time_ms, 999);
    }

    #[test]
    fn emulator_schema_collects_languages_title_and_lowercase_key() {
        let schema = parse_emulator_schema(
            &json!([
                { "name": "ACH19", "displayName": { "brazilian": "Tira meu gatinho da árvore?", "english": "Cat in a Tree" } },
                { "name": "ACH20", "display_name": { "spanish": "Gato" }, "title": "Título" },
                { "apiname": "ACH21" }
            ])
            .to_string(),
        )
        .unwrap();
        assert_eq!(
            schema.get("ach19").unwrap(),
            &vec![
                "Tira meu gatinho da árvore?".to_string(),
                "Cat in a Tree".to_string()
            ]
        );
        assert_eq!(
            schema.get("ach20").unwrap(),
            &vec!["Gato".to_string(), "Título".to_string()]
        );
        assert!(!schema.contains_key("ach21"), "sem título não vira ponte");
        assert!(parse_emulator_schema("not-json").is_none());
    }

    #[test]
    fn find_emulator_schema_walks_up_five_levels_and_stops_at_root() {
        let mut fs = FakeFs::default();
        let game = PathBuf::from("C:/game");
        let exe = game.join("Gameface").join("Binaries").join("Win64");
        fs.file(game.join("steam_settings").join("achievements.json"), "[]");
        let found = find_emulator_schema(&fs, &exe).unwrap();
        assert_eq!(
            found.path,
            game.join("steam_settings").join("achievements.json")
        );
        let empty = FakeFs::default();
        assert!(find_emulator_schema(&empty, &exe).is_none());
    }

    #[test]
    fn crack_roots_and_document_fallback_match_the_platform_layout() {
        let env = CrackEnvironment {
            home: PathBuf::from("C:/Users/x"),
            appdata: Some(PathBuf::from("C:/Users/x/AppData/Roaming")),
            local_app_data: Some(PathBuf::from("C:/Users/x/AppData/Local")),
            public: Some(PathBuf::from("C:/Users/Public")),
            program_data: Some(PathBuf::from("C:/ProgramData")),
            documents: None,
        };
        let windows = crack_roots(Platform::Windows, Path::new("ignored"), &env);
        assert_eq!(windows.appdata, PathBuf::from("C:/Users/x/AppData/Roaming"));
        assert_eq!(
            windows.public_docs,
            PathBuf::from("C:/Users/Public/Documents")
        );
        assert_eq!(windows.user_docs, PathBuf::from("C:/Users/x/Documents"));
        let unix = crack_roots(Platform::Unix, Path::new("C:/prefix"), &env);
        assert!(unix
            .appdata
            .ends_with(Path::new("drive_c/users/steamuser/AppData/Roaming")));
        assert!(unix
            .program_data
            .ends_with(Path::new("drive_c/ProgramData")));
        let mut fs = FakeFs::default();
        fs.dir("C:/Users/x/OneDrive/Documents", &[]);
        assert_eq!(
            pasta_documentos_windows(&fs, Path::new("C:/Users/x")),
            PathBuf::from("C:/Users/x/OneDrive/Documents")
        );
    }

    #[test]
    fn crack_candidates_cover_every_supported_format_in_order() {
        let roots = CrackRoots {
            user: PathBuf::from("u"),
            appdata: PathBuf::from("a"),
            local_app_data: PathBuf::from("l"),
            public_docs: PathBuf::from("p"),
            program_data: PathBuf::from("g"),
            user_docs: PathBuf::from("d"),
        };
        let candidates = crack_candidates(
            &roots,
            "42",
            &["7".to_string()],
            Some(Path::new("C:/game/Win64")),
        );
        let providers: Vec<_> = candidates.iter().map(|candidate| candidate.group).collect();
        assert_eq!(providers[0], CrackGroup::Upc);
        assert_eq!(
            providers
                .iter()
                .filter(|group| **group == CrackGroup::Goldberg)
                .count(),
            2
        );
        assert!(providers.contains(&CrackGroup::Codex));
        assert!(providers.contains(&CrackGroup::Rune));
        assert_eq!(
            providers
                .iter()
                .filter(|group| **group == CrackGroup::Skidrow)
                .count(),
            3
        );
        assert_eq!(
            providers
                .iter()
                .filter(|group| **group == CrackGroup::Rld)
                .count(),
            3
        );
        assert_eq!(providers.last(), Some(&CrackGroup::ThreeDmGame));
        assert_eq!(
            detect_crack_group(&candidates[0].path),
            Some(CrackGroup::Upc)
        );
        assert_eq!(
            detect_crack_group(Path::new("C:/x/GSE Saves/42/achievements.json")),
            Some(CrackGroup::Goldberg)
        );
        assert_eq!(detect_crack_group(Path::new("C:/x/other")), None);
        assert_eq!(CrackGroup::SmartSteamEmu.provider_name(), "sse");
        assert_eq!(
            CrackGroup::from_provider_name("3dmgame"),
            Some(CrackGroup::ThreeDmGame)
        );
    }

    #[test]
    fn uplay_id_scan_enumerates_only_when_allowed() {
        let mut fs = FakeFs::default();
        fs.dir("roaming/Goldberg UplayEmu Saves", &["66088", "777", "abc"]);
        fs.dir("roaming/Goldberg UplayEmu Saves/66088", &[]);
        fs.dir("roaming/Goldberg UplayEmu Saves/777", &[]);
        fs.dir("roaming/Goldberg UplayEmu Saves/abc", &[]);
        let ids = uplay_ids_for_scan(
            &fs,
            Platform::Windows,
            "999",
            &json!({}),
            &json!({}),
            Path::new("roaming/Goldberg UplayEmu Saves"),
        );
        assert!(ids.is_empty(), "Windows sem ID confirmado não enumera");
        let ids = uplay_ids_for_scan(
            &fs,
            Platform::Unix,
            "999",
            &json!({}),
            &json!({}),
            Path::new("roaming/Goldberg UplayEmu Saves"),
        );
        assert!(ids.contains(&"66088".to_string()));
        assert!(ids.contains(&"777".to_string()));
        assert!(!ids.contains(&"abc".to_string()));
        let ids = uplay_ids_for_scan(
            &fs,
            Platform::Windows,
            "3751950",
            &json!({}),
            &json!({}),
            Path::new("roaming/Goldberg UplayEmu Saves"),
        );
        assert_eq!(
            ids,
            ["66088", "777"],
            "configurado primeiro, depois enumeração"
        );
    }

    #[test]
    fn settings_library_prefix_and_exe_resolution() {
        let settings = json!({
            "steam:42": { "exePath": "C:/game/Game.exe", "prefixPath": "C:/prefix" },
            "steam:43": { "exePath": "C:/g2/Game.exe" }
        });
        let entry = json!({ "id": "steam:42", "exe": "C:/ignored.exe" });
        let config = settings_for_entry(&settings, &entry, "42");
        assert_eq!(config["prefixPath"], json!("C:/prefix"));
        assert_eq!(
            resolve_prefix("42", &config, &entry, Path::new("compat"), None),
            PathBuf::from("C:/prefix")
        );
        assert_eq!(
            resolve_prefix("42", &json!({}), &entry, Path::new("compat"), Some("C:/p2")),
            PathBuf::from("C:/p2")
        );
        assert_eq!(
            resolve_prefix("42", &json!({}), &entry, Path::new("compat"), None),
            PathBuf::from("compat/42/pfx")
        );
        assert_eq!(
            resolve_exe_dir(&config, &entry),
            Some(PathBuf::from("C:/game"))
        );
        assert_eq!(extrair_appid(&entry), Some("42".to_string()));
        let library = json!({ "games": [{ "id": "steam:42" }] });
        let games = library_games(&library, &settings);
        assert_eq!(games.len(), 2);
        assert_eq!(games[0]["installed"], json!(true));
        assert_eq!(games[1]["id"], json!("steam:43"));
        assert_eq!(games[1]["exe"], json!("C:/g2/Game.exe"));
    }

    #[test]
    fn matching_uses_upc_title_first_then_numeric_id_and_ambiguous_title_stays_unmatched() {
        let items = items();
        let by_title = find_item_for_unlock(
            &items,
            &DetectedUnlock {
                name: Some("Primeiros passos".into()),
                id: Some("99".into()),
                ..DetectedUnlock::default()
            },
            CrackGroup::Upc,
            None,
        );
        assert_eq!(by_title, Some(0));
        let by_id = find_item_for_unlock(
            &items,
            &DetectedUnlock {
                name: Some("desconhecido".into()),
                id: Some("3".into()),
                ..DetectedUnlock::default()
            },
            CrackGroup::Upc,
            None,
        );
        assert_eq!(by_id, None, "id 3 não existe no catálogo");
        let mut with_id = items.clone();
        with_id[1]["uplayId"] = json!("40");
        assert_eq!(
            find_item_for_unlock(
                &with_id,
                &DetectedUnlock {
                    name: Some("x".into()),
                    id: Some("40".into()),
                    ..DetectedUnlock::default()
                },
                CrackGroup::Upc,
                None,
            ),
            Some(1)
        );
        let ambiguous = vec![
            json!({ "apiname": "ach_15", "title": "Mesmo", "block": 14, "bit": 0 }),
            json!({ "apiname": "ach_16", "title": "Mesmo", "block": 15, "bit": 0 }),
        ];
        let titles = BTreeMap::from([("ach19".to_string(), vec!["Mesmo".to_string()])]);
        assert_eq!(
            find_item_for_unlock(
                &ambiguous,
                &DetectedUnlock {
                    name: Some("ACH19".into()),
                    ..DetectedUnlock::default()
                },
                CrackGroup::Goldberg,
                Some(&titles),
            ),
            None
        );
    }

    #[test]
    fn emulator_title_bridge_finds_synthetic_item_and_apply_records_alias() {
        let mut items = vec![json!({
            "apiname": "ach_15", "title": "Tira meu gatinho da árvore?", "block": 14, "bit": 0,
            "achieved": false, "unlock": 0, "percent": 0
        })];
        let titles = BTreeMap::from([(
            "ach19".to_string(),
            vec!["Tira meu gatinho da árvore?".to_string()],
        )]);
        let unlock = DetectedUnlock {
            name: Some("ACH19".into()),
            unlock_time_ms: 1_700_000_000_000,
            ..DetectedUnlock::default()
        };
        let result = apply_unlocks(
            &mut items,
            "42",
            CrackGroup::Goldberg,
            &[unlock],
            Some(&titles),
            0,
        );
        assert_eq!(result.payloads.len(), 1);
        assert_eq!(result.payloads[0].apiname, "ach_15");
        assert_eq!(result.payloads[0].provider, "goldberg");
        assert_eq!(result.payloads[0].unlock, 1_700_000_000);
        assert_eq!(result.payloads[0].key, "14|0");
        assert_eq!(items[0]["achieved"], json!(true));
        assert_eq!(items[0]["aliases"], json!(["ACH19"]));
        assert_eq!(items[0]["remoteApiname"], json!("ACH19"));
        assert!(result.changed);
        let second = apply_unlocks(
            &mut items,
            "42",
            CrackGroup::Goldberg,
            &[DetectedUnlock {
                name: Some("ACH19".into()),
                unlock_time_ms: 1_700_000_000_000,
                ..DetectedUnlock::default()
            }],
            Some(&titles),
            0,
        );
        assert!(second.payloads.is_empty(), "já achieved não repete toast");
        assert!(!second.changed, "alias já era conhecido");
    }

    #[test]
    fn payload_uses_block_bit_fallback_and_now_for_zero_or_negative_timestamps() {
        let item = json!({ "apiname": "A", "title": "T", "desc": "D", "icon": "I", "block": 1, "bit": 2, "percent": 7 });
        let payload = payload_for_unlock(
            "42",
            &item,
            &DetectedUnlock {
                unlock_time_ms: 0,
                ..DetectedUnlock::default()
            },
            CrackGroup::Codex,
            1_700_000_000_000,
        );
        assert_eq!(payload.key, "1|2");
        assert_eq!(payload.unlock, 1_700_000_000);
        assert_eq!(payload.percent, 7.0);
        let fallback = payload_for_unlock(
            "42",
            &json!({ "apiname": "A" }),
            &DetectedUnlock {
                id: Some("9".into()),
                unlock_time_ms: -1500,
                ..DetectedUnlock::default()
            },
            CrackGroup::Flt,
            0,
        );
        assert_eq!(fallback.key, "A");
        assert_eq!(fallback.unlock, -2, "Math.floor(-1.5)");
    }

    #[test]
    fn upc_reconciliation_revokes_only_allowed_local_unlocks() {
        let runtime = json!({
            "1": { "apiname": "Um", "earned": true, "earned_time": 10 },
            "2": { "apiname": "Dois", "earned": false },
            "3": { "apiname": "Três", "earned": true }
        });
        let mut items = vec![
            json!({ "apiname": "A", "title": "Um", "achieved": true, "localUnlock": true, "unlock": 10 }),
            json!({ "apiname": "B", "title": "Dois", "achieved": true, "localUnlock": true, "unlock": 20 }),
        ];
        let revoked = reconcile_upc(&mut items, "42", &runtime.to_string(), true);
        assert_eq!(revoked.len(), 1);
        assert_eq!(revoked[0].apiname, "B");
        assert_eq!(items[1]["achieved"], json!(false));
        assert_eq!(items[1]["unlock"], json!(0));
        assert_eq!(items[1]["revoked"], json!(true));

        let mut items =
            vec![json!({ "apiname": "B", "title": "Dois", "achieved": true, "localUnlock": true })];
        assert!(reconcile_upc(&mut items, "42", &runtime.to_string(), false).is_empty());
        assert_eq!(items[0]["achieved"], json!(true));
    }

    #[test]
    fn reconciliation_requires_complete_and_active_runtime() {
        let mut items = vec![
            json!({ "apiname": "A", "title": "Um", "achieved": true, "localUnlock": true }),
            json!({ "apiname": "B", "title": "Dois", "achieved": true, "localUnlock": true }),
        ];
        let incomplete = json!({
            "1": { "apiname": "Um", "earned": true }
        });
        assert!(
            reconcile_upc(&mut items, "42", &incomplete.to_string(), true).is_empty(),
            "runtime menor que o catálogo não é autoritativo"
        );
        let inactive = json!({
            "1": { "apiname": "Um", "earned": false },
            "2": { "apiname": "Dois", "earned": false }
        });
        assert!(reconcile_upc(&mut items, "42", &inactive.to_string(), true).is_empty());
    }

    #[test]
    fn mtime_gate_processes_first_read_then_only_changes() {
        let mut watcher = CrackedWatcher::new("zes");
        let path = Path::new("C:/x/file.json");
        assert!(watcher.should_process(path, 100));
        assert!(!watcher.should_process(path, 100));
        assert!(watcher.should_process(path, 101));
        let mut other = CrackedWatcher::new("outra-conta");
        assert!(other.should_process(path, 101), "cache é por conta");
    }

    #[test]
    fn scan_game_detects_cracker_and_uses_injected_clock() {
        let mut fs = FakeFs::default();
        let env = CrackEnvironment {
            home: PathBuf::from("C:/home"),
            appdata: Some(PathBuf::from("roaming")),
            local_app_data: Some(PathBuf::from("local")),
            public: Some(PathBuf::from("public")),
            program_data: Some(PathBuf::from("programdata")),
            documents: Some(PathBuf::from("docs")),
        };
        let path = PathBuf::from("roaming")
            .join("GSE Saves")
            .join("42")
            .join("achievements.json");
        fs.file(
            path,
            &json!({ "ACH01": { "earned": true, "earned_time": 1700000000 } }).to_string(),
        );
        let mut watcher = CrackedWatcher::new("zes");
        let mut items = items();
        let entry = json!({ "id": "steam:42" });
        let settings = json!({});
        let clock = FixedClock(9_999_999);
        let options = ScanGameOptions {
            fs: &fs,
            clock: &clock,
            appid: "42",
            entry: &entry,
            settings: &settings,
            platform: Platform::Windows,
            prefix: Path::new("ignored"),
            env: &env,
            allow_revoke: false,
        };
        let report = watcher.scan_game(&options, &mut items);
        assert_eq!(report.unlocks.len(), 1);
        assert_eq!(report.unlocks[0].provider, "goldberg");
        assert_eq!(report.unlocks[0].unlock, 1_700_000_000);
        assert_eq!(items[0]["achieved"], json!(true));
        let again = watcher.scan_game(&options, &mut items);
        assert!(again.unlocks.is_empty(), "mtime igual não reprocessa");
    }
}
