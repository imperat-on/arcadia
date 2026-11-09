//! Normalização do schema de conquistas da Steam.
//!
//! Porte de `loadSchemaForAppid`/`loadSchemaFallback`/`itensSalvosComoIndice`
//! de `app/electron/achievements/loader.js` e do mapa de campos usado pelo
//! `steam_bin.js`: cada `stats.<bloco>.bits.<bit>` vira uma conquista com
//! `apiname`, nome/descrição localizados e hashes de ícone. `hidden` e
//! `permission` são preservados como bits de apresentação/permissão.
//!
//! O JS não tem fallback de schema em VDF de texto — o único formato lido dos
//! arquivos da Steam é o KeyValues binário.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::js::{js_number_value, js_string_value, js_trim, js_truthy_value};
use crate::steam_bin::{load_kv_bin, KvObject, KvValue};

/// Cadeia de idiomas do `pickLocalized` (o primeiro não vazio vence).
pub const LOCALE_CHAIN: [&str; 3] = ["brazilian", "english", "spanish"];

/// Uma conquista normalizada do schema.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SchemaAchievement {
    /// Chave do bloco no bin (string, como no objeto JS).
    pub block: String,
    /// Chave do bit no bin.
    pub bit: String,
    pub apiname: String,
    pub name: String,
    pub desc: String,
    pub icon_hash: String,
    pub icongray_hash: String,
    /// Flag `hidden` do schema (0/1 ou booleano).
    pub hidden: bool,
    /// Bits de permissão (`permission`), 0 quando ausente.
    pub permission: u32,
}

impl SchemaAchievement {
    pub fn key(&self) -> String {
        format!("{}|{}", self.block, self.bit)
    }
}

/// Schema de um appid, na ordem em que o bin apresenta os blocos/bits.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AchievementSchema {
    pub appid: String,
    pub achievements: Vec<SchemaAchievement>,
}

impl AchievementSchema {
    pub fn new(appid: impl Into<String>) -> Self {
        Self {
            appid: appid.into(),
            achievements: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.achievements.len()
    }

    pub fn is_empty(&self) -> bool {
        self.achievements.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &SchemaAchievement> {
        self.achievements.iter()
    }

    pub fn get(&self, key: &str) -> Option<&SchemaAchievement> {
        self.achievements
            .iter()
            .find(|achievement| achievement.key() == key)
    }

    /// `Object.entries` do índice `block|bit`.
    pub fn entries(&self) -> impl Iterator<Item = (String, &SchemaAchievement)> {
        self.achievements
            .iter()
            .map(|achievement| (achievement.key(), achievement))
    }
}

/// Escolhe o texto localizado: pt-BR, inglês, espanhol e depois a primeira
/// string que não seja a chave `token`.
pub fn pick_localized(map: Option<&KvObject>) -> String {
    let Some(map) = map else {
        return String::new();
    };
    for locale in LOCALE_CHAIN {
        if let Some(value) = map.get_str(locale) {
            if !js_trim(value).is_empty() {
                return value.to_string();
            }
        }
    }
    for (key, value) in map.iter() {
        if key == "token" {
            continue;
        }
        if let KvValue::String(text) = value {
            if !js_trim(text).is_empty() {
                return text.clone();
            }
        }
    }
    String::new()
}

/// Normaliza o objeto raiz de um `UserGameStatsSchema_*.bin`.
pub fn schema_from_kv(appid: impl Into<String>, root: &KvObject) -> AchievementSchema {
    let mut schema = AchievementSchema::new(appid);
    let Some(stats) = root.get_object("stats") else {
        return schema;
    };
    for (block_key, block_value) in stats.iter() {
        let Some(block) = block_value.as_object() else {
            continue;
        };
        let Some(bits) = block.get_object("bits") else {
            continue;
        };
        for (bit_key, item_value) in bits.iter() {
            let Some(item) = item_value.as_object() else {
                continue;
            };
            let apiname = match item.get("name") {
                Some(value) if js_truthy_value_kv(value) => value.js_string(),
                _ => continue,
            };
            if apiname.is_empty() {
                continue;
            }
            let display = item.get_object("display");
            let icon = display
                .and_then(|display| display.get_str("icon"))
                .unwrap_or("");
            let icon_gray = display
                .and_then(|display| display.get_str("icon_gray"))
                .filter(|value| !value.is_empty())
                .unwrap_or(icon);
            let hidden = kv_flag(item.get("hidden"))
                || kv_flag(display.and_then(|display| display.get("hidden")));
            let permission = kv_u32(
                item.get("permission")
                    .or_else(|| display.and_then(|display| display.get("permission"))),
            );
            schema.achievements.push(SchemaAchievement {
                block: block_key.to_string(),
                bit: bit_key.to_string(),
                apiname,
                name: pick_localized(display.and_then(|display| display.get_object("name"))),
                desc: pick_localized(display.and_then(|display| display.get_object("desc"))),
                icon_hash: icon.to_string(),
                icongray_hash: icon_gray.to_string(),
                hidden,
                permission,
            });
        }
    }
    schema
}

/// Normaliza diretamente os bytes de um `UserGameStatsSchema_*.bin`.
pub fn schema_from_bytes(appid: impl Into<String>, bytes: &[u8]) -> AchievementSchema {
    match crate::steam_bin::parse_binary(bytes) {
        Ok(root) => schema_from_kv(appid, &root),
        Err(_) => AchievementSchema::new(appid),
    }
}

/// Lê e normaliza o schema de um appid. Arquivo ausente/inválido devolve vazio.
pub fn load_schema_for_appid(stats_dir: impl AsRef<Path>, appid: &str) -> AchievementSchema {
    let path = schema_path(stats_dir, appid);
    let Some(root) = load_kv_bin(&path).and_then(|value| value.as_object().cloned()) else {
        return AchievementSchema::new(appid);
    };
    schema_from_kv(appid, &root)
}

/// Catálogo completo da pasta `appcache/stats`: appid → schema normalizado.
pub fn load_schema_catalog(stats_dir: impl AsRef<Path>) -> BTreeMap<String, AchievementSchema> {
    let stats_dir = stats_dir.as_ref();
    list_schema_files(stats_dir)
        .into_iter()
        .map(|(appid, _)| {
            let schema = load_schema_for_appid(stats_dir, &appid);
            (appid, schema)
        })
        .collect()
}

pub fn schema_filename(appid: &str) -> String {
    format!("UserGameStatsSchema_{appid}.bin")
}

pub fn schema_path(stats_dir: impl AsRef<Path>, appid: &str) -> PathBuf {
    stats_dir.as_ref().join(schema_filename(appid))
}

/// `^UserGameStatsSchema_(\d+)\.bin$` — devolve o appid do nome.
pub fn appid_from_schema_filename(filename: &str) -> Option<&str> {
    let rest = filename.strip_prefix("UserGameStatsSchema_")?;
    let digits = rest.strip_suffix(".bin")?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(digits)
}

/// `^UserGameStats_(\d+)_(\d+)\.bin$` — devolve `(steamid, appid)`.
pub fn parse_progress_filename(filename: &str) -> Option<(&str, &str)> {
    let rest = filename.strip_prefix("UserGameStats_")?;
    let rest = rest.strip_suffix(".bin")?;
    let (steam_id, appid) = rest.split_once('_')?;
    let numeric =
        |value: &str| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit());
    if numeric(steam_id) && numeric(appid) {
        Some((steam_id, appid))
    } else {
        None
    }
}

/// Lista `(appid, caminho)` dos schemas de uma pasta `appcache/stats`.
pub fn list_schema_files(stats_dir: impl AsRef<Path>) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(stats_dir) else {
        return Vec::new();
    };
    let mut output = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(appid) = appid_from_schema_filename(&name) {
            output.push((appid.to_string(), entry.path()));
        }
    }
    output.sort_by(|left, right| left.0.cmp(&right.0));
    output
}

/// Índice a partir dos itens JÁ SALVOS no `achievements.json`.
///
/// Espelha `itensSalvosComoIndice`: exige `apiname` e `block`/`bit` inteiros
/// (o JS usa `Number(...)` + `Number.isInteger`). Ícones ficam vazios para o
/// merge usar o item anterior como dono real da imagem.
pub fn schema_from_saved_items(items: &[Value]) -> AchievementSchema {
    let mut schema = AchievementSchema::default();
    for item in items {
        let Some(object) = item.as_object() else {
            continue;
        };
        let apiname = object
            .get("apiname")
            .filter(|value| js_truthy_value(value))
            .map(js_string_value)
            .unwrap_or_default();
        if apiname.is_empty() {
            continue;
        }
        let Some(block) = integer_text(object.get("block")) else {
            continue;
        };
        let Some(bit) = integer_text(object.get("bit")) else {
            continue;
        };
        let text = |key: &str| {
            object
                .get(key)
                .filter(|value| js_truthy_value(value))
                .map(js_string_value)
                .unwrap_or_default()
        };
        schema.achievements.push(SchemaAchievement {
            block,
            bit,
            apiname,
            name: text("title"),
            desc: text("desc"),
            icon_hash: String::new(),
            icongray_hash: String::new(),
            hidden: false,
            permission: 0,
        });
    }
    schema
}

/// Fallback da loja/API: monta o schema a partir das linhas do scrape.
///
/// `apinames` é a lista posicional da API pública. O apiname do próprio item
/// (`it.apiname`) sempre vence; sem ele usa a lista e, por fim, `ach_NN`.
pub fn schema_from_scrape(items: &[Value], apinames: &[String]) -> AchievementSchema {
    let mut schema = AchievementSchema::default();
    for (index, item) in items.iter().enumerate() {
        let Some(object) = item.as_object() else {
            continue;
        };
        let title = object
            .get("title")
            .filter(|value| js_truthy_value(value))
            .map(js_string_value)
            .unwrap_or_default();
        if title.is_empty() {
            continue;
        }
        let own_apiname = object
            .get("apiname")
            .filter(|value| js_truthy_value(value))
            .map(js_string_value)
            .unwrap_or_default();
        let apiname = if !own_apiname.is_empty() {
            own_apiname
        } else if let Some(from_api) = apinames.get(index).filter(|value| !value.is_empty()) {
            from_api.clone()
        } else {
            format!("ach_{:02}", index + 1)
        };
        let icon = object
            .get("icon")
            .filter(|value| js_truthy_value(value))
            .map(js_string_value)
            .unwrap_or_default();
        let icongray = object
            .get("icongray")
            .filter(|value| js_truthy_value(value))
            .map(js_string_value)
            .unwrap_or_else(|| icon.clone());
        schema.achievements.push(SchemaAchievement {
            block: index.to_string(),
            bit: "0".to_string(),
            apiname,
            name: title,
            desc: object
                .get("desc")
                .filter(|value| js_truthy_value(value))
                .map(js_string_value)
                .unwrap_or_default(),
            icon_hash: icon,
            icongray_hash: icongray,
            hidden: false,
            permission: 0,
        });
    }
    schema
}

fn js_truthy_value_kv(value: &KvValue) -> bool {
    match value {
        KvValue::Object(_) => true,
        KvValue::String(text) => !text.is_empty(),
        KvValue::Int32(number) => *number != 0,
        KvValue::UInt64(number) => *number != 0,
        KvValue::Int64(number) => *number != 0,
        KvValue::Float32(number) => *number != 0.0 && !number.is_nan(),
    }
}

/// Flag numérica (`hidden`): string "0" é falsa, string "1"/"true" é verdadeira.
fn kv_flag(value: Option<&KvValue>) -> bool {
    let Some(value) = value else {
        return false;
    };
    match value {
        KvValue::Object(_) => false,
        KvValue::String(text) if text.eq_ignore_ascii_case("true") => true,
        KvValue::String(text) if text.eq_ignore_ascii_case("false") => false,
        KvValue::String(text) => match js_number_value(&Value::String(text.clone())) {
            Some(number) => number.is_finite() && number != 0.0,
            None => !js_trim(text).is_empty(),
        },
        KvValue::Int32(number) => *number != 0,
        KvValue::UInt64(number) => *number != 0,
        KvValue::Int64(number) => *number != 0,
        KvValue::Float32(number) => *number != 0.0 && !number.is_nan(),
    }
}

fn kv_u32(value: Option<&KvValue>) -> u32 {
    let Some(value) = value else {
        return 0;
    };
    let number = match value {
        KvValue::Int32(number) => f64::from(*number),
        KvValue::UInt64(number) => *number as f64,
        KvValue::Int64(number) => *number as f64,
        KvValue::Float32(number) => f64::from(*number),
        KvValue::String(text) => js_number_value(&Value::String(text.clone())).unwrap_or(0.0),
        KvValue::Object(_) => 0.0,
    };
    if !number.is_finite() {
        return 0;
    }
    let modulo = number.trunc().rem_euclid(4_294_967_296.0);
    modulo as u32
}

fn integer_text(value: Option<&Value>) -> Option<String> {
    let number = js_number_value(value?)?;
    if !number.is_finite() || number.fract() != 0.0 {
        return None;
    }
    Some(format!("{number:.0}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn kv(entries: Vec<(&str, KvValue)>) -> KvObject {
        let mut object = KvObject::new();
        for (key, value) in entries {
            object.insert(key, value);
        }
        object
    }

    fn display(name: &str, desc: &str, icon: &str, icon_gray: &str) -> KvValue {
        KvValue::Object(kv(vec![
            (
                "name",
                KvValue::Object(kv(vec![("brazilian", KvValue::String(name.into()))])),
            ),
            (
                "desc",
                KvValue::Object(kv(vec![("brazilian", KvValue::String(desc.into()))])),
            ),
            ("icon", KvValue::String(icon.into())),
            ("icon_gray", KvValue::String(icon_gray.into())),
        ]))
    }

    fn root_with_bits(bits: Vec<(&str, KvValue)>) -> KvObject {
        kv(vec![(
            "stats",
            KvValue::Object(kv(vec![(
                "1",
                KvValue::Object(kv(vec![("bits", KvValue::Object(kv(bits)))])),
            )])),
        )])
    }

    #[test]
    fn pick_localized_uses_the_locale_chain_then_first_non_token_string() {
        let map = kv(vec![
            ("token", KvValue::String("#Token".into())),
            ("english", KvValue::String("English".into())),
            ("brazilian", KvValue::String("Português".into())),
        ]);
        assert_eq!(pick_localized(Some(&map)), "Português");
        let map = kv(vec![
            ("token", KvValue::String("#Token".into())),
            ("german", KvValue::String("Deutsch".into())),
        ]);
        assert_eq!(pick_localized(Some(&map)), "Deutsch");
        let map = kv(vec![
            ("brazilian", KvValue::String("   ".into())),
            ("english", KvValue::String("Fallback".into())),
        ]);
        assert_eq!(pick_localized(Some(&map)), "Fallback");
        assert_eq!(pick_localized(None), "");
    }

    #[test]
    fn schema_maps_block_bit_display_icons_hidden_and_permission() {
        let first_item = KvValue::Object(kv(vec![
            ("name", KvValue::String("ACH01".into())),
            (
                "display",
                display("Título", "Descrição", "icon.jpg", "gray.jpg"),
            ),
            ("hidden", KvValue::Int32(1)),
            ("permission", KvValue::Int32(3)),
        ]));
        let second_item = KvValue::Object(kv(vec![
            ("name", KvValue::String("ACH02".into())),
            ("display", display("Sem cinza", "D", "only.jpg", "")),
            ("hidden", KvValue::String("0".into())),
            ("permission", KvValue::String("2".into())),
        ]));
        let schema = schema_from_kv(
            "42",
            &root_with_bits(vec![("0", first_item), ("1", second_item)]),
        );
        assert_eq!(schema.len(), 2);
        let first = schema.get("1|0").unwrap();
        assert_eq!(first.apiname, "ACH01");
        assert_eq!(first.name, "Título");
        assert_eq!(first.desc, "Descrição");
        assert_eq!(first.icon_hash, "icon.jpg");
        assert_eq!(first.icongray_hash, "gray.jpg");
        assert!(first.hidden);
        assert_eq!(first.permission, 3);
        let second = schema.get("1|1").unwrap();
        assert_eq!(second.icongray_hash, "only.jpg");
        assert!(!second.hidden, "hidden \"0\" é falso");
        assert_eq!(second.permission, 2);
    }

    #[test]
    fn schema_skips_entries_without_name_bits_or_stats() {
        let without_name = KvValue::Object(kv(vec![("display", display("x", "y", "i", "g"))]));
        let bits = KvValue::Object(kv(vec![("0", without_name)]));
        let block = KvValue::Object(kv(vec![("bits", bits)]));
        let stats = KvValue::Object(kv(vec![
            ("1", block),
            ("2", KvValue::String("not-an-object".into())),
        ]));
        let root = kv(vec![("stats", stats)]);
        assert!(schema_from_kv("1", &root).is_empty());
        assert!(schema_from_kv("1", &kv(vec![])).is_empty());
    }

    #[test]
    fn schema_filename_helpers_accept_only_numeric_bins() {
        assert_eq!(
            appid_from_schema_filename("UserGameStatsSchema_3751950.bin"),
            Some("3751950")
        );
        assert_eq!(
            appid_from_schema_filename("UserGameStatsSchema_x.bin"),
            None
        );
        assert_eq!(
            appid_from_schema_filename("UserGameStatsSchema_1.txt"),
            None
        );
        assert_eq!(
            parse_progress_filename("UserGameStats_76561198000000000_440.bin"),
            Some(("76561198000000000", "440"))
        );
        assert_eq!(parse_progress_filename("UserGameStats_440.bin"), None);
        assert_eq!(schema_filename("440"), "UserGameStatsSchema_440.bin");
    }

    #[test]
    fn saved_items_index_requires_integral_block_and_bit() {
        let items = vec![
            json!({ "apiname": "A", "title": "Um", "desc": "D", "block": 1, "bit": 0 }),
            json!({ "apiname": "B", "title": "Dois", "block": "2", "bit": "7" }),
            json!({ "apiname": "C", "title": "Sem bit", "block": 2 }),
            json!({ "apiname": "", "title": "Sem nome", "block": 3, "bit": 0 }),
            json!({ "apiname": "D", "title": "Fracionário", "block": 1.5, "bit": 0 }),
        ];
        let schema = schema_from_saved_items(&items);
        assert_eq!(schema.len(), 2);
        assert_eq!(schema.get("1|0").unwrap().apiname, "A");
        assert_eq!(schema.get("2|7").unwrap().apiname, "B");
        assert_eq!(schema.get("1|0").unwrap().icon_hash, "");
    }

    #[test]
    fn scrape_fallback_prefers_own_apiname_then_api_list_then_synthetic() {
        let items = vec![
            json!({ "apiname": "OWN", "title": "Um", "icon": "a.jpg" }),
            json!({ "apiname": "", "title": "Dois", "icon": "b.jpg", "icongray": "bg.jpg" }),
            json!({ "apiname": null, "title": "Três" }),
            json!({ "apiname": "", "title": "" }),
        ];
        let apinames = vec!["API1".to_string()];
        let schema = schema_from_scrape(&items, &apinames);
        assert_eq!(schema.len(), 3);
        assert_eq!(schema.achievements[0].apiname, "OWN");
        assert_eq!(schema.achievements[1].apiname, "ach_02");
        assert_eq!(schema.achievements[1].icongray_hash, "bg.jpg");
        assert_eq!(schema.achievements[2].apiname, "ach_03");
        assert_eq!(schema.achievements[0].block, "0");
        assert_eq!(schema.achievements[0].bit, "0");
    }

    #[test]
    fn empty_api_list_uses_synthetic_apinames_without_padding_limits() {
        let items: Vec<Value> = (0..101)
            .map(|index| json!({ "title": format!("Ach {index}") }))
            .collect();
        let schema = schema_from_scrape(&items, &[]);
        assert_eq!(schema.achievements[0].apiname, "ach_01");
        assert_eq!(schema.achievements[99].apiname, "ach_100");
    }

    #[test]
    fn load_schema_reads_a_binary_fixture_from_disk() {
        use crate::steam_bin::{KV_OBJECT, KV_OBJECT_END, KV_STRING};
        fn name(value: &str) -> Vec<u8> {
            let mut bytes = value.as_bytes().to_vec();
            bytes.push(0);
            bytes
        }
        fn string(name_value: &str, value: &str) -> Vec<u8> {
            let mut bytes = vec![KV_STRING];
            bytes.extend(name(name_value));
            bytes.extend_from_slice(value.as_bytes());
            bytes.push(0);
            bytes
        }
        fn object(name_value: &str, children: &[Vec<u8>]) -> Vec<u8> {
            let mut bytes = vec![KV_OBJECT];
            bytes.extend(name(name_value));
            for child in children {
                bytes.extend_from_slice(child);
            }
            bytes.push(KV_OBJECT_END);
            bytes
        }
        let bytes = object(
            "",
            &[object(
                "stats",
                &[object(
                    "1",
                    &[object("bits", &[object("0", &[string("name", "ACH01")])])],
                )],
            )],
        );
        let dir = std::env::temp_dir().join(format!(
            "arcadia-schema-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(schema_path(&dir, "77"), bytes).unwrap();
        let schema = load_schema_for_appid(&dir, "77");
        assert_eq!(schema.len(), 1);
        assert_eq!(schema.get("1|0").unwrap().apiname, "ACH01");
        assert!(load_schema_for_appid(&dir, "78").is_empty());
        assert_eq!(list_schema_files(&dir).len(), 1);
        let catalog = load_schema_catalog(&dir);
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog.get("77").unwrap().len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
