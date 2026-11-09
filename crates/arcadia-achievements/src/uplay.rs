//! Suporte ao loader UPC/voices38 (`upc_r2_loader64.dll`).
//!
//! Porte das partes puras de `app/electron/achievements/uplay.js`:
//! identificação/normalização de IDs, leitura do runtime (`earned` /
//! `earned_time`, segundos ou milissegundos), geração/validação do schema,
//! caminhos de save por plataforma e inspeção da instalação com filesystem
//! injetado.
//!
//! A preparação que grava `achievements_schema.json`/`upc_r2.ini` (com backup,
//! escrita atômica e recusa de symlink) fica fora desta crate pura; aqui só os
//! construtores/validadores e a transformação de INI são expostos.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::fsx::{Platform, WatcherFs};
use crate::js::{js_number_value, js_string_value, js_trim, js_truthy_value};
use crate::r#match::{numeric_achievement_id, numeric_achievement_id_value};

pub const USUARIO_WINE: &str = "steamuser";
pub const UPLAY_SAVE_DIR: &str = "Goldberg UplayEmu Saves";

/// Relações UPC↔Steam confirmadas. Novos jogos entram aqui depois de curados.
pub const KNOWN_UPC_IDS: &[(&str, &str)] = &[("3751950", "66088")];

pub fn known_upc_id(appid: &str) -> Option<&'static str> {
    KNOWN_UPC_IDS
        .iter()
        .find(|(candidate, _)| *candidate == appid)
        .map(|(_, id)| *id)
}

fn plain_object(value: &Value) -> bool {
    matches!(value, Value::Object(_))
}

/// Normaliza ID numérico em string (sem zeros à esquerda). `None` = inválido.
pub fn normalize_numeric_id_str(value: &str) -> Option<String> {
    let text = js_trim(value);
    if text.is_empty() || text.len() > 18 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let stripped = text.trim_start_matches('0');
    if stripped.is_empty() {
        Some("0".to_string())
    } else {
        Some(stripped.to_string())
    }
}

/// Mesma normalização para valores JSON (número seguro >= 0 ou string).
pub fn normalize_numeric_id_value(value: &Value) -> Option<String> {
    match value {
        Value::Number(number) => {
            let value = number.as_f64()?;
            if !value.is_finite()
                || value < 0.0
                || value.fract() != 0.0
                || value > 9_007_199_254_740_991.0
            {
                return None;
            }
            // `String(Number)` usa o double (já arredondado), não o literal.
            if value == 0.0 {
                Some("0".to_string())
            } else {
                Some(format!("{value:.0}"))
            }
        }
        Value::String(text) => normalize_numeric_id_str(text),
        Value::Null => None,
        other => normalize_numeric_id_str(&js_string_value(other)),
    }
}

/// `earnedValue`: booleano/número/string aceitos como desbloqueio.
pub fn earned_value(value: &Value) -> bool {
    match value {
        Value::Bool(value) => *value,
        Value::Number(number) => number
            .as_f64()
            .map(|value| value.is_finite() && value > 0.0)
            .unwrap_or(false),
        Value::String(text) => {
            let text = text.trim().to_lowercase();
            if ["true", "yes", "on", "unlocked"].contains(&text.as_str()) {
                return true;
            }
            if text.is_empty() {
                return false;
            }
            match js_number_value(&Value::String(text)) {
                Some(number) => number.is_finite() && number > 0.0,
                None => false,
            }
        }
        _ => false,
    }
}

/// UPC grava segundos Unix; aceitamos milissegundos e sempre devolvemos ms.
pub fn epoch_milliseconds(value: &Value) -> i64 {
    let Some(number) = js_number_value(value) else {
        return 0;
    };
    if !number.is_finite() || number <= 0.0 {
        return 0;
    }
    let millis = if number > 100_000_000_000.0 {
        number
    } else {
        number * 1000.0
    };
    if millis >= i64::MAX as f64 {
        i64::MAX
    } else {
        millis.floor() as i64
    }
}

/// Uma conquista desbloqueada no runtime UPC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpcUnlock {
    pub id: String,
    pub name: String,
    pub unlock_time: i64,
}

/// Estado de uma entrada do runtime completo (inclusive `earned=0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpcRuntime {
    pub id: String,
    pub earned: bool,
    pub unlock_time: i64,
}

/// `parseUPC`: objeto id → estado; `earned=0` nunca desbloqueia.
pub fn parse_upc(content: &str) -> Vec<UpcUnlock> {
    let Ok(root) = serde_json::from_str::<Value>(content) else {
        return Vec::new();
    };
    let Value::Object(root) = root else {
        return Vec::new();
    };
    let mut output = Vec::new();
    for (raw_id, value) in root {
        let Some(object) = value.as_object() else {
            continue;
        };
        if !object.get("earned").map(earned_value).unwrap_or(false) {
            continue;
        }
        let id = normalize_numeric_id_str(&raw_id).unwrap_or_else(|| raw_id.trim().to_string());
        if id.is_empty() {
            continue;
        }
        let name = ["apiname", "name", "displayName"]
            .iter()
            .find_map(|key| object.get(*key).filter(|value| js_truthy_value(value)))
            .map(js_string_value)
            .unwrap_or_else(|| id.clone());
        output.push(UpcUnlock {
            id,
            name,
            unlock_time: object
                .get("earned_time")
                .map(epoch_milliseconds)
                .unwrap_or(0),
        });
    }
    output
}

/// Runtime completo por displayName normalizado (a fonte da reconciliação).
pub fn upc_runtime_map(content: &str) -> Option<BTreeMap<String, UpcRuntime>> {
    let root: Value = serde_json::from_str(content).ok()?;
    let Value::Object(root) = root else {
        return None;
    };
    let mut map = BTreeMap::new();
    for (raw_id, value) in root {
        let Some(object) = value.as_object() else {
            continue;
        };
        let id = normalize_numeric_id_str(&raw_id).unwrap_or_else(|| raw_id.trim().to_string());
        if id.is_empty() {
            continue;
        }
        let name = ["apiname", "name", "displayName", "description", "title"]
            .iter()
            .find_map(|key| object.get(*key).filter(|value| js_truthy_value(value)))
            .map(js_string_value)
            .unwrap_or_default();
        let name = name.trim().to_lowercase();
        map.insert(
            name,
            UpcRuntime {
                id,
                earned: object.get("earned").map(earned_value).unwrap_or(false),
                unlock_time: object
                    .get("earned_time")
                    .map(epoch_milliseconds)
                    .unwrap_or(0),
            },
        );
    }
    Some(map)
}

/// `numericAchievementId` (reexporta a regra do match).
pub fn numeric_achievement_id_str(value: &str) -> Option<String> {
    numeric_achievement_id(value)
}

/// ID UPC de um item do catálogo: campos explícitos, apiname/name e, quando
/// permitido, a posição (1-based).
pub fn item_uplay_id(item: &Value, index: Option<usize>, allow_positional: bool) -> Option<String> {
    let object = item.as_object()?;
    for key in ["uplayId", "upcId", "objectiveId", "achievementId"] {
        if let Some(id) = object.get(key).and_then(numeric_achievement_id_value) {
            return Some(id);
        }
    }
    let from_name = object
        .get("apiname")
        .filter(|value| js_truthy_value(value))
        .or_else(|| object.get("name").filter(|value| js_truthy_value(value)))
        .and_then(numeric_achievement_id_value);
    if from_name.is_some() {
        return from_name;
    }
    if allow_positional {
        return index.map(|index| (index + 1).to_string());
    }
    None
}

/// Erros de geração do schema UPC (mensagens do oráculo JS).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UplaySchemaError(pub String);

impl fmt::Display for UplaySchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for UplaySchemaError {}

/// `buildUplaySchema`: gera somente o conteúdo (não escreve em pasta).
pub fn build_uplay_schema(
    items: &[Value],
    allow_positional: bool,
) -> Result<Map<String, Value>, UplaySchemaError> {
    if items.is_empty() {
        return Err(UplaySchemaError("catálogo UPC vazio".to_string()));
    }
    let mut schema = Map::new();
    for (index, item) in items.iter().enumerate() {
        let id = item_uplay_id(item, Some(index), allow_positional)
            .ok_or_else(|| UplaySchemaError(format!("ID UPC ausente no item {}", index + 1)))?;
        if schema.contains_key(&id) {
            return Err(UplaySchemaError(format!("ID UPC duplicado: {id}")));
        }
        let object = item.as_object();
        let first = |keys: &[&str]| {
            keys.iter()
                .find_map(|key| {
                    object
                        .and_then(|object| object.get(*key))
                        .filter(|value| js_truthy_value(value))
                })
                .map(js_string_value)
                .unwrap_or_default()
        };
        let title = first(&["title", "displayName", "name"]);
        let description = {
            let description = first(&["desc", "description"]);
            if description.is_empty() {
                title.clone()
            } else {
                description
            }
        };
        let mut entry = Map::new();
        entry.insert("displayName".into(), Value::String(title));
        entry.insert("description".into(), Value::String(description));
        entry.insert("earned".into(), Value::from(0));
        schema.insert(id, Value::Object(entry));
    }
    Ok(schema)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UplayValidateOptions {
    pub require_contiguous: bool,
    pub require_metadata: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UplayValidation {
    pub ok: bool,
    pub error: Option<String>,
    pub count: usize,
    pub ids: Vec<String>,
}

impl UplayValidation {
    fn failure(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(error.into()),
            count: 0,
            ids: Vec::new(),
        }
    }
}

/// `validateUplaySchema` do JS, incluindo o modo contíguo (1..N).
pub fn validate_uplay_schema(schema: &Value, options: UplayValidateOptions) -> UplayValidation {
    let Value::Object(root) = schema else {
        return UplayValidation::failure("schema-raiz-nao-e-objeto");
    };
    let ids: Vec<String> = root.keys().cloned().collect();
    if ids.is_empty() {
        return UplayValidation::failure("schema-vazio");
    }
    for raw_id in &ids {
        if normalize_numeric_id_str(raw_id).as_deref() != Some(raw_id.as_str()) {
            return UplayValidation::failure(format!("id-upc-invalido:{raw_id}"));
        }
        let Some(entry) = root.get(raw_id) else {
            continue;
        };
        if !plain_object(entry) {
            return UplayValidation::failure(format!("entrada-invalida:{raw_id}"));
        }
        if options.require_metadata {
            let display = entry
                .get("displayName")
                .map(js_string_value)
                .unwrap_or_default();
            let description = entry
                .get("description")
                .map(js_string_value)
                .unwrap_or_default();
            if js_trim(&display).is_empty() || js_trim(&description).is_empty() {
                return UplayValidation::failure(format!("metadata-ausente:{raw_id}"));
            }
        }
    }
    if options.require_contiguous {
        let mut sorted: Vec<u64> = ids.iter().filter_map(|id| id.parse().ok()).collect();
        sorted.sort_unstable();
        let expected: Vec<String> = (1..=ids.len()).map(|value| value.to_string()).collect();
        let actual: Vec<String> = sorted.iter().map(u64::to_string).collect();
        if actual != expected {
            return UplayValidation::failure("ids-upc-nao-contiguos");
        }
    }
    UplayValidation {
        ok: true,
        error: None,
        count: ids.len(),
        ids,
    }
}

/// `resolveUplayId`: configuração/entrada e, por último, o mapa conhecido.
///
/// Quando existe mapeamento conhecido para o appid, um ID divergente é
/// ignorado (evita apontar para o runtime de outro jogo).
pub fn resolve_uplay_id(appid: &str, settings: &Value, entry: &Value) -> Option<String> {
    let settings_object = settings.as_object();
    let entry_object = entry.as_object();
    let get = |object: Option<&Map<String, Value>>, key: &str| {
        object.and_then(|object| object.get(key)).cloned()
    };
    let candidates = [
        get(settings_object, "uplayId"),
        get(settings_object, "upcId"),
        get(settings_object, "uplayAppId"),
        get(entry_object, "uplayId"),
        get(entry_object, "upcId"),
    ];
    let known = known_upc_id(appid);
    for candidate in candidates.into_iter().flatten() {
        if let Some(id) = normalize_numeric_id_value(&candidate) {
            if known.is_some() && known != Some(id.as_str()) {
                continue;
            }
            return Some(id);
        }
    }
    known.map(str::to_string)
}

pub fn uplay_save_root(
    platform: Platform,
    prefix: Option<&Path>,
    appdata: &Path,
) -> Option<PathBuf> {
    if platform.is_windows() {
        return Some(appdata.join(UPLAY_SAVE_DIR));
    }
    let prefix = prefix.filter(|path| !path.as_os_str().is_empty())?;
    Some(
        prefix
            .join("drive_c")
            .join("users")
            .join(USUARIO_WINE)
            .join("AppData")
            .join("Roaming")
            .join(UPLAY_SAVE_DIR),
    )
}

pub fn uplay_runtime_path(
    platform: Platform,
    prefix: Option<&Path>,
    uplay_id: &str,
    appdata: &Path,
) -> Option<PathBuf> {
    let id = normalize_numeric_id_str(uplay_id)?;
    Some(
        uplay_save_root(platform, prefix, appdata)?
            .join(id)
            .join("achievements.json"),
    )
}

pub fn uplay_schema_path(game_dir: impl AsRef<Path>) -> Option<PathBuf> {
    let game_dir = game_dir.as_ref();
    if game_dir.as_os_str().is_empty() {
        return None;
    }
    Some(game_dir.join("achievements_schema.json"))
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UplayInstallation {
    pub detected: bool,
    pub game_dir: PathBuf,
    pub ini_path: Option<PathBuf>,
    pub loader_path: Option<PathBuf>,
    pub voices_path: Option<PathBuf>,
    pub schema_path: Option<PathBuf>,
}

/// Inspeção sem escrita; arquivos são resolvidos pelo filesystem injetado.
pub fn inspect_uplay_installation(fs: &dyn WatcherFs, game_dir: &str) -> UplayInstallation {
    let root = PathBuf::from(game_dir);
    if game_dir.is_empty() {
        return UplayInstallation::default();
    }
    let ini_path = first_existing(fs, &root, &["upc_r2.ini"]);
    let loader_path = first_existing(fs, &root, &["upc_r2_loader64.dll", "upc_r2_loader.dll"]);
    let voices_path = first_existing(fs, &root, &["voices38.dll"]);
    let schema_path = uplay_schema_path(&root);
    UplayInstallation {
        detected: ini_path.is_some() && loader_path.is_some(),
        game_dir: root,
        ini_path,
        loader_path,
        voices_path,
        schema_path,
    }
}

fn first_existing(fs: &dyn WatcherFs, dir: &Path, names: &[&str]) -> Option<PathBuf> {
    for name in names {
        let file = dir.join(name);
        if fs.is_file(&file) {
            return Some(file);
        }
    }
    None
}

fn ini_key_rest<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let trimmed = line.trim_start();
    let head = trimmed.get(..key.len())?;
    if !head.eq_ignore_ascii_case(key) {
        return None;
    }
    trimmed.get(key.len()..)
}

/// Lê uma chave do INI (case-insensitive, valor até `;`/`#`).
pub fn ini_setting(text: &str, key: &str) -> String {
    for raw_line in text.split('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let Some(rest) = ini_key_rest(line, key) else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(after) = rest.strip_prefix('=') else {
            continue;
        };
        let value = after.split([';', '#']).next().unwrap_or("");
        return value.trim().to_string();
    }
    String::new()
}

/// `setIniSetting`: atualiza a chave, insere em `[Settings]` ou cria a seção.
pub fn set_ini_setting(text: &str, key: &str, value: &str) -> String {
    // 1) Substitui a linha existente preservando o prefixo até o `=`.
    let mut offset = 0usize;
    for segment in text.split_inclusive('\n') {
        let (line, ending) = match segment.strip_suffix('\n') {
            Some(line) => (
                line.strip_suffix('\r').unwrap_or(line),
                &segment[line.len()..],
            ),
            None => (segment, ""),
        };
        if let Some(rest) = ini_key_rest(line, key) {
            let rest = rest.trim_start();
            if let Some(after_eq) = rest.strip_prefix('=') {
                let after_spaces = after_eq.len() - after_eq.trim_start().len();
                let prefix_end = line.len() - rest.len() + 1 + after_spaces;
                let mut output = String::new();
                output.push_str(&text[..offset]);
                output.push_str(&line[..prefix_end]);
                output.push_str(value);
                output.push_str(ending);
                output.push_str(&text[offset + segment.len()..]);
                return output;
            }
        }
        offset += segment.len();
    }

    // 2) Insere logo depois do cabeçalho [Settings].
    offset = 0;
    for segment in text.split_inclusive('\n') {
        let line = segment.trim_end_matches(['\r', '\n']);
        if line.trim().eq_ignore_ascii_case("[Settings]") {
            let insert_at = offset + segment.len();
            let mut output = String::new();
            output.push_str(&text[..insert_at]);
            output.push_str(key);
            output.push_str(" = ");
            output.push_str(value);
            output.push('\n');
            output.push_str(&text[insert_at..]);
            return output;
        }
        offset += segment.len();
    }

    // 3) Cria a seção no topo.
    format!("[Settings]\n{key} = {value}\n{text}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsx::WatcherFs;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct FakeFs {
        files: BTreeMap<PathBuf, String>,
        dirs: BTreeMap<PathBuf, Vec<String>>,
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
        fn modified_ms(&self, _path: &Path) -> Option<i64> {
            Some(1)
        }
    }

    fn black_flag_items() -> Vec<Value> {
        vec![
            json!({
                "apiname": "ACObsidian_Ach_1",
                "title": "Último a Sair",
                "desc": "Sente-se em um navio em chamas",
                "block": 1,
                "bit": 0
            }),
            json!({
                "apiname": "ACObsidian_Ach_40",
                "title": "Artesanato de Primeira",
                "desc": "Crie um item",
                "block": 2,
                "bit": 7
            }),
        ]
    }

    #[test]
    fn parse_upc_reads_numeric_map_and_ignores_zero_earned() {
        let found = parse_upc(&json!({
            "1": { "displayName": "One", "description": "D1", "earned": 0 },
            "2": { "displayName": "Two", "description": "D2", "earned": true, "earned_time": 1788096412 },
            "3": { "displayName": "Three", "description": "D3", "earned": 1, "earned_time": 1788096413000_u64 },
            "4": { "displayName": "Four", "description": "D4" }
        }).to_string());
        assert_eq!(
            found,
            vec![
                UpcUnlock {
                    id: "2".into(),
                    name: "Two".into(),
                    unlock_time: 1_788_096_412_000
                },
                UpcUnlock {
                    id: "3".into(),
                    name: "Three".into(),
                    unlock_time: 1_788_096_413_000
                }
            ]
        );
    }

    #[test]
    fn parse_upc_rejects_schema_array_and_invalid_json() {
        assert!(parse_upc("[]").is_empty());
        assert!(parse_upc("not-json").is_empty());
        assert!(parse_upc(&json!({ "1": { "earned": 0 } }).to_string()).is_empty());
    }

    #[test]
    fn upc_runtime_map_keys_by_preferred_name_and_keeps_locked_entries() {
        let map = upc_runtime_map(
            &json!({
                "1": { "displayName": "Um", "earned": 0 },
                "2": { "apiname": "ACH02", "displayName": "Dois", "earned": 1, "earned_time": 5 },
                "3": "invalid"
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map["um"].id, "1");
        assert!(!map["um"].earned);
        assert!(map["ach02"].earned);
        assert_eq!(map["ach02"].unlock_time, 5000);
        assert!(upc_runtime_map("[]").is_none());
        assert!(upc_runtime_map("{").is_none());
    }

    #[test]
    fn earned_and_epoch_normalization_follow_the_js_rules() {
        assert!(earned_value(&json!(true)));
        assert!(earned_value(&json!(1)));
        assert!(!earned_value(&json!(0)));
        assert!(earned_value(&json!("YES")));
        assert!(earned_value(&json!("2")));
        assert!(!earned_value(&json!("")));
        assert!(!earned_value(&json!("no")));
        assert_eq!(epoch_milliseconds(&json!(1_788_096_412)), 1_788_096_412_000);
        assert_eq!(
            epoch_milliseconds(&json!(1_788_096_412_000_u64)),
            1_788_096_412_000
        );
        assert_eq!(epoch_milliseconds(&json!(0)), 0);
        assert_eq!(epoch_milliseconds(&json!(-5)), 0);
        assert_eq!(epoch_milliseconds(&json!("abc")), 0);
    }

    #[test]
    fn numeric_ids_accept_safe_numbers_and_trim_leading_zeros() {
        assert_eq!(normalize_numeric_id_str("00040"), Some("40".into()));
        assert_eq!(normalize_numeric_id_str("0"), Some("0".into()));
        assert_eq!(normalize_numeric_id_str("000"), Some("0".into()));
        assert_eq!(normalize_numeric_id_str("../x"), None);
        assert_eq!(normalize_numeric_id_str("1234567890123456789"), None);
        assert_eq!(normalize_numeric_id_value(&json!(40)), Some("40".into()));
        assert_eq!(
            normalize_numeric_id_value(&json!(1_788_096_412_u64)),
            Some("1788096412".into())
        );
        assert_eq!(normalize_numeric_id_value(&json!(-1)), None);
        assert_eq!(normalize_numeric_id_value(&json!(1.5)), None);
        assert_eq!(
            numeric_achievement_id_str("ACObsidian_Ach_40"),
            Some("40".into())
        );
    }

    #[test]
    fn item_uplay_id_prefers_explicit_fields_and_positional_only_when_allowed() {
        let item = json!({ "apiname": "ACObsidian_Ach_40", "title": "X" });
        assert_eq!(item_uplay_id(&item, Some(0), false), Some("40".into()));
        let item = json!({ "title": "Sem id" });
        assert_eq!(item_uplay_id(&item, Some(2), false), None);
        assert_eq!(item_uplay_id(&item, Some(2), true), Some("3".into()));
        assert_eq!(
            item_uplay_id(
                &json!({ "uplayId": "70", "apiname": "Ach_1" }),
                Some(0),
                false
            ),
            Some("70".into())
        );
    }

    #[test]
    fn build_schema_uses_decimal_ids_and_description_fallback() {
        let schema = build_uplay_schema(&black_flag_items(), false).unwrap();
        assert_eq!(schema["1"]["displayName"], json!("Último a Sair"));
        assert_eq!(
            schema["1"]["description"],
            json!("Sente-se em um navio em chamas")
        );
        assert_eq!(schema["40"]["earned"], json!(0));
        assert_eq!(
            build_uplay_schema(&[], false).unwrap_err().0,
            "catálogo UPC vazio"
        );
        let missing = build_uplay_schema(&[json!({ "title": "X" })], false).unwrap_err();
        assert_eq!(missing.0, "ID UPC ausente no item 1");
        let duplicate = build_uplay_schema(
            &[
                json!({ "uplayId": "1", "title": "A" }),
                json!({ "uplayId": "01", "title": "B" }),
            ],
            false,
        )
        .unwrap_err();
        assert_eq!(duplicate.0, "ID UPC duplicado: 1");
        let positional = build_uplay_schema(
            &[
                json!({ "title": "A", "desc": "" }),
                json!({ "title": "B", "desc": "" }),
            ],
            true,
        )
        .unwrap();
        assert_eq!(positional["2"]["description"], json!("B"));
    }

    #[test]
    fn validate_schema_detects_invalid_ids_entries_and_metadata() {
        let schema = json!({
            "1": { "displayName": "A", "description": "D" },
            "2": { "displayName": "B", "description": "D" }
        });
        assert!(validate_uplay_schema(&schema, UplayValidateOptions::default()).ok);
        assert!(
            validate_uplay_schema(
                &schema,
                UplayValidateOptions {
                    require_contiguous: true,
                    require_metadata: true
                }
            )
            .ok
        );
        let gap = json!({ "1": { "displayName": "A", "description": "D" }, "3": { "displayName": "B", "description": "D" } });
        assert_eq!(
            validate_uplay_schema(
                &gap,
                UplayValidateOptions {
                    require_contiguous: true,
                    ..UplayValidateOptions::default()
                }
            )
            .error
            .as_deref(),
            Some("ids-upc-nao-contiguos")
        );
        assert_eq!(
            validate_uplay_schema(&json!([]), UplayValidateOptions::default())
                .error
                .as_deref(),
            Some("schema-raiz-nao-e-objeto")
        );
        assert_eq!(
            validate_uplay_schema(&json!({}), UplayValidateOptions::default())
                .error
                .as_deref(),
            Some("schema-vazio")
        );
        assert_eq!(
            validate_uplay_schema(&json!({ "01": {} }), UplayValidateOptions::default())
                .error
                .as_deref(),
            Some("id-upc-invalido:01")
        );
        assert_eq!(
            validate_uplay_schema(&json!({ "1": "x" }), UplayValidateOptions::default())
                .error
                .as_deref(),
            Some("entrada-invalida:1")
        );
        assert_eq!(
            validate_uplay_schema(
                &json!({ "1": {} }),
                UplayValidateOptions {
                    require_metadata: true,
                    ..UplayValidateOptions::default()
                }
            )
            .error
            .as_deref(),
            Some("metadata-ausente:1")
        );
    }

    #[test]
    fn resolve_uplay_id_uses_config_and_known_mapping_safely() {
        assert_eq!(
            resolve_uplay_id("3751950", &json!({}), &json!({})),
            Some("66088".into())
        );
        assert_eq!(
            resolve_uplay_id("999", &json!({ "upcId": "1234" }), &json!({})),
            Some("1234".into())
        );
        assert_eq!(
            resolve_uplay_id("999", &json!({ "uplayId": "../unsafe" }), &json!({})),
            None
        );
        assert_eq!(
            resolve_uplay_id(
                "3751950",
                &json!({ "uplayId": "999" }),
                &json!({ "uplayId": "66088" })
            ),
            Some("66088".into()),
            "candidato divergente é ignorado e o conhecido vale"
        );
        assert_eq!(
            resolve_uplay_id("999", &json!({}), &json!({ "uplayId": "007" })),
            Some("7".into())
        );
    }

    #[test]
    fn runtime_and_save_paths_respect_platform_and_prefix() {
        let appdata = PathBuf::from("C:/Users/x/AppData/Roaming");
        let windows = uplay_runtime_path(
            Platform::Windows,
            Some(Path::new("ignored")),
            "66088",
            &appdata,
        )
        .unwrap();
        assert!(windows.ends_with(Path::new("Goldberg UplayEmu Saves/66088/achievements.json")));

        let unix = uplay_runtime_path(
            Platform::Unix,
            Some(Path::new("C:/prefix")),
            "66088",
            &appdata,
        )
        .unwrap();
        assert!(unix.ends_with(Path::new(
            "drive_c/users/steamuser/AppData/Roaming/Goldberg UplayEmu Saves/66088/achievements.json"
        )));
        assert!(uplay_runtime_path(Platform::Unix, None, "1", &appdata).is_none());
        assert!(uplay_runtime_path(Platform::Windows, None, "01x", &appdata).is_none());
        assert!(uplay_save_root(Platform::Unix, Some(Path::new("")), &appdata).is_none());
        assert_eq!(
            uplay_schema_path("C:/game").unwrap(),
            PathBuf::from("C:/game/achievements_schema.json")
        );
        assert!(uplay_schema_path("").is_none());
    }

    #[test]
    fn inspect_installation_requires_ini_and_loader() {
        let mut fs = FakeFs::default();
        let root = PathBuf::from("C:/game");
        fs.files.insert(root.join("upc_r2.ini"), "x".into());
        fs.files
            .insert(root.join("upc_r2_loader64.dll"), "x".into());
        let info = inspect_uplay_installation(&fs, "C:/game");
        assert!(info.detected);
        assert_eq!(info.voices_path, None);
        assert_eq!(info.schema_path, uplay_schema_path("C:/game"));
        assert!(!inspect_uplay_installation(&fs, "C:/other").detected);
        assert!(!inspect_uplay_installation(&fs, "").detected);
    }

    #[test]
    fn ini_setting_and_set_ini_setting_match_the_js_transformations() {
        let text = "[Settings]\nAchievements = 0\nLogging = 1\n";
        assert_eq!(ini_setting(text, "achievements"), "0");
        assert_eq!(
            set_ini_setting(text, "Achievements", "1"),
            "[Settings]\nAchievements = 1\nLogging = 1\n"
        );
        assert_eq!(
            set_ini_setting("[Settings]\nLogging = 1\n", "Achievements", "1"),
            "[Settings]\nAchievements = 1\nLogging = 1\n"
        );
        assert_eq!(
            set_ini_setting("Logging = 1\n", "Achievements", "1"),
            "[Settings]\nAchievements = 1\nLogging = 1\n"
        );
        assert_eq!(
            set_ini_setting("A=1\r\n", "B", "2"),
            "[Settings]\nB = 2\nA=1\r\n"
        );
        assert_eq!(ini_setting("X=1 ; c\n", "X"), "1");
    }
}
