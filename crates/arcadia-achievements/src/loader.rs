//! Leitura do `achievements.json`, índice `block|bit` e rebuild dos itens a
//! partir do schema + progresso do bin da Steam.
//!
//! Porte de `app/electron/achievements/schema.js` e das partes locais de
//! `loader.js` (`loadSchemaForAppid`, `itensSalvosComoIndice`, merge de
//! achieved/unlock e `loadAllSchemas`). A busca do fallback público da Steam
//! (rede) é injetada por [`ScrapeSchemaProvider`]; esta crate não faz HTTP.
//!
//! Diferente do JS — que remonta o item com um objeto fechado e perde campos
//! como `localUnlock`/`revoked` — aqui o item anterior é a base do merge, então
//! campos desconhecidos são preservados. Os campos conhecidos são atualizados
//! com a mesma semântica e prioridade do oráculo.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Number, Value};

use crate::js::{js_number_value, js_string_value, js_trim, js_truthy_value};
use crate::r#match::{aliases_of, build_index, find_item_index, remember_alias};
use crate::schema::{
    load_schema_for_appid, schema_from_saved_items, AchievementSchema, SchemaAchievement,
};
use crate::steam_bin::{json_block_bit_key, load_progress_map};

/// Store completo de `achievements.json`: `{ appid: { items: [...] } }`.
///
/// O objeto raiz e os objetos por appid são preservados por inteiro; só a
/// chave `items` é trocada pelo rebuild.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AchievementsStore {
    root: Map<String, Value>,
}

impl AchievementsStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// JSON inválido, array ou escalar devolve store vazio (nunca lança).
    pub fn from_json(text: &str) -> Self {
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            return Self::default();
        };
        Self::from_value(value)
    }

    pub fn from_value(value: Value) -> Self {
        match value {
            Value::Object(root) => Self { root },
            _ => Self::default(),
        }
    }

    /// Lê o arquivo; ausente/corrompido devolve store vazio.
    pub fn load(path: impl AsRef<Path>) -> Self {
        fs::read_to_string(path)
            .map(|text| Self::from_json(&text))
            .unwrap_or_default()
    }

    pub fn to_value(&self) -> Value {
        Value::Object(self.root.clone())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.to_value()).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn appids(&self) -> Vec<String> {
        self.root.keys().cloned().collect()
    }

    pub fn entry(&self, appid: &str) -> Option<&Map<String, Value>> {
        self.root.get(appid).and_then(Value::as_object)
    }

    pub fn entry_mut(&mut self, appid: &str) -> Option<&mut Map<String, Value>> {
        self.root.get_mut(appid).and_then(Value::as_object_mut)
    }

    /// Itens do appid; entrada ausente ou `items` não-array devolve vazio.
    pub fn items(&self, appid: &str) -> Vec<Value> {
        let Some(entry) = self.entry(appid) else {
            return Vec::new();
        };
        match entry.get("items") {
            Some(Value::Array(items)) => items.clone(),
            _ => Vec::new(),
        }
    }

    /// Troca somente `items`, preservando todos os outros campos da entrada.
    pub fn set_items(&mut self, appid: &str, items: Vec<Value>) {
        if !matches!(self.root.get(appid), Some(Value::Object(_))) {
            self.root
                .insert(appid.to_string(), Value::Object(Map::new()));
        }
        if let Some(Value::Object(entry)) = self.root.get_mut(appid) {
            entry.insert("items".into(), Value::Array(items));
        }
    }

    /// Escrita atômica (tmp + rename), como o `saveAchievements` do JS.
    pub fn save_atomic(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "achievements".to_string());
        let temporary =
            path.with_file_name(format!(".{file_name}.tmp-{}-{nanos}", std::process::id()));
        if let Err(error) = fs::write(&temporary, self.to_json()) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        match fs::rename(&temporary, path) {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                Err(error)
            }
        }
    }
}

pub fn load_achievements(path: impl AsRef<Path>) -> AchievementsStore {
    AchievementsStore::load(path)
}

pub fn save_achievements(path: impl AsRef<Path>, store: &AchievementsStore) -> io::Result<()> {
    store.save_atomic(path)
}

/// `appid -> "block|bit" -> item` usado pelo vigia de bins.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ItemIndex {
    by_appid: BTreeMap<String, BTreeMap<String, Value>>,
}

impl ItemIndex {
    pub fn get(&self, appid: &str, key: &str) -> Option<&Value> {
        self.by_appid.get(appid)?.get(key)
    }

    pub fn has(&self, appid: &str, key: &str) -> bool {
        self.get(appid, key).is_some()
    }

    pub fn appids(&self) -> impl Iterator<Item = &str> {
        self.by_appid.keys().map(String::as_str)
    }

    pub fn keys_for(&self, appid: &str) -> impl Iterator<Item = &str> {
        self.by_appid
            .get(appid)
            .into_iter()
            .flat_map(|map| map.keys().map(String::as_str))
    }

    pub fn is_empty(&self) -> bool {
        self.by_appid.is_empty()
    }

    /// `true` quando há ao menos um `block|bit` para o appid.
    pub fn has_steam_bin_for(&self, appid: &str) -> bool {
        self.by_appid
            .get(appid)
            .map(|map| !map.is_empty())
            .unwrap_or(false)
    }
}

/// `loadItemIndex`: só itens com as DUAS chaves `block`/`bit` presentes.
pub fn load_item_index(store: &AchievementsStore) -> ItemIndex {
    let mut index = ItemIndex::default();
    for appid in store.appids() {
        let mut map = BTreeMap::new();
        for item in store.items(&appid) {
            let Some(object) = item.as_object() else {
                continue;
            };
            let (Some(block), Some(bit)) = (object.get("block"), object.get("bit")) else {
                continue;
            };
            let key = format!("{}|{}", js_string_value(block), js_string_value(bit));
            map.insert(key, item);
        }
        if !map.is_empty() {
            index.by_appid.insert(appid, map);
        }
    }
    index
}

/// `loadItemIndex()` a partir do caminho do `achievements.json`.
pub fn load_item_index_from_path(path: impl AsRef<Path>) -> ItemIndex {
    load_item_index(&AchievementsStore::load(path))
}

/// Origem do schema usado no rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaOrigin {
    Binary,
    SavedItems,
    Scrape,
    Missing,
}

/// Schema resolvido + progresso do bin para um appid.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedAppSchema {
    pub appid: String,
    pub origin: SchemaOrigin,
    pub schema: AchievementSchema,
    pub progress: BTreeMap<String, i64>,
}

/// Fallback público (loja/API) injetado; a crate não faz rede.
pub trait ScrapeSchemaProvider {
    fn schema_for_appid(&self, appid: &str) -> Option<AchievementSchema>;
}

pub struct NoScrapeProvider;

impl ScrapeSchemaProvider for NoScrapeProvider {
    fn schema_for_appid(&self, _appid: &str) -> Option<AchievementSchema> {
        None
    }
}

/// `UserGameStats_<steamid>_<appid>.bin` por appid (último arquivo vence, como
/// o `Map.set` do JS).
pub fn list_progress_files(stats_dir: impl AsRef<Path>) -> BTreeMap<String, PathBuf> {
    let Ok(entries) = fs::read_dir(stats_dir) else {
        return BTreeMap::new();
    };
    let mut output = BTreeMap::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some((_, appid)) = crate::schema::parse_progress_filename(&name) {
            output.insert(appid.to_string(), entry.path());
        }
    }
    output
}

/// Hierarquia do loader: bin da Steam → itens já salvos → fallback de scrape.
pub fn resolve_schema_for_appid(
    store: &AchievementsStore,
    stats_dir: impl AsRef<Path>,
    appid: &str,
    permission_allowed: bool,
    scrape: Option<&dyn ScrapeSchemaProvider>,
) -> ResolvedAppSchema {
    let stats_dir = stats_dir.as_ref();
    let mut schema = load_schema_for_appid(stats_dir, appid);
    let mut origin = if schema.is_empty() {
        SchemaOrigin::Missing
    } else {
        SchemaOrigin::Binary
    };
    if schema.is_empty() {
        schema = schema_from_saved_items(&store.items(appid));
        if !schema.is_empty() {
            schema.appid = appid.to_string();
            origin = SchemaOrigin::SavedItems;
        }
    }
    if schema.is_empty() {
        if let Some(provider) = scrape {
            if let Some(provided) = provider.schema_for_appid(appid) {
                schema = provided;
                if schema.appid.is_empty() {
                    schema.appid = appid.to_string();
                }
                if !schema.is_empty() {
                    origin = SchemaOrigin::Scrape;
                }
            }
        }
    }
    let progress = if permission_allowed {
        list_progress_files(stats_dir)
            .get(appid)
            .map(load_progress_map)
            .unwrap_or_default()
    } else {
        BTreeMap::new()
    };
    ResolvedAppSchema {
        appid: appid.to_string(),
        origin,
        schema,
        progress,
    }
}

/// Resolver de ícones do loader: cópia do cache da Steam e fallback CDN.
pub trait IconResolver {
    fn resolve_icon(&self, appid: &str, hash: &str) -> Option<String>;
}

/// Resolvedor sem I/O: só aceita URL pronta ou monta a URL do CDN.
#[derive(Debug, Clone, Copy, Default)]
pub struct CdnIcons;

impl IconResolver for CdnIcons {
    fn resolve_icon(&self, appid: &str, hash: &str) -> Option<String> {
        if hash.is_empty() {
            return None;
        }
        if hash.starts_with("http") {
            return Some(hash.to_string());
        }
        steam_icon_url(appid, hash)
    }
}

/// Resolvedor com o cache local (`appcache/stats/achievement_images`).
#[derive(Debug, Clone)]
pub struct SteamIcons {
    pub icon_dir: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
}

impl SteamIcons {
    pub fn new(icon_dir: impl Into<PathBuf>, cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            icon_dir: Some(icon_dir.into()),
            cache_dir: Some(cache_dir.into()),
        }
    }

    pub fn cdn_only() -> Self {
        Self {
            icon_dir: None,
            cache_dir: None,
        }
    }
}

impl IconResolver for SteamIcons {
    fn resolve_icon(&self, appid: &str, hash: &str) -> Option<String> {
        if hash.is_empty() {
            return None;
        }
        if hash.starts_with("http") {
            return Some(hash.to_string());
        }
        if let (Some(icon_dir), Some(cache_dir)) = (&self.icon_dir, &self.cache_dir) {
            if let Some(path) = copy_icon_to_cache(appid, hash, icon_dir, cache_dir) {
                return Some(path);
            }
        }
        steam_icon_url(appid, hash)
    }
}

pub const STEAM_ICON_HOST: &str = "https://cdn.cloudflare.steamstatic.com";

pub fn steam_icon_url(appid: &str, hash: &str) -> Option<String> {
    if hash.is_empty() {
        return None;
    }
    Some(format!(
        "{STEAM_ICON_HOST}/steamcommunity/public/images/apps/{appid}/{hash}"
    ))
}

/// Copia `<icon_dir>/<hash>` para `<cache_dir>/<appid>_<hash>` e devolve
/// `file://`. Falha de cópia devolve `None` (o chamador cai para o CDN).
pub fn copy_icon_to_cache(
    appid: &str,
    hash: &str,
    icon_dir: &Path,
    cache_dir: &Path,
) -> Option<String> {
    if hash.is_empty() {
        return None;
    }
    let source = icon_dir.join(hash);
    if !source.is_file() {
        return None;
    }
    fs::create_dir_all(cache_dir).ok()?;
    let destination = cache_dir.join(format!("{appid}_{hash}"));
    if !destination.exists() {
        fs::copy(&source, &destination).ok()?;
    }
    Some(file_url(&destination))
}

/// `file:///C:/...` no Windows e `file:///home/...` no Unix.
pub fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy();
    if cfg!(windows) {
        format!("file:///{}", text.replace('\\', "/"))
    } else {
        format!("file://{text}")
    }
}

/// Registro de push gerado pelo rebuild (a fila/dedupe fica em `manager`).
#[derive(Debug, Clone, PartialEq)]
pub struct SyncRecord {
    pub appid: String,
    pub apiname: String,
    pub unlocked_at: i64,
    pub achieved: bool,
    pub title: String,
    pub icon: Option<String>,
    pub percent: Value,
}

impl SyncRecord {
    pub fn to_value(&self) -> Value {
        let mut object = Map::new();
        object.insert("appid".into(), Value::String(self.appid.clone()));
        object.insert("apiname".into(), Value::String(self.apiname.clone()));
        object.insert("unlocked_at".into(), Value::from(self.unlocked_at));
        object.insert("achieved".into(), Value::Bool(self.achieved));
        object.insert("title".into(), Value::String(self.title.clone()));
        object.insert(
            "icon".into(),
            self.icon.clone().map(Value::String).unwrap_or(Value::Null),
        );
        object.insert("percent".into(), self.percent.clone());
        Value::Object(object)
    }
}

/// Resultado do rebuild de um appid.
#[derive(Debug, Clone, PartialEq)]
pub struct RebuildReport {
    pub appid: String,
    pub updated: bool,
    pub icons_resolved: usize,
    pub items_before: usize,
    pub items_after: usize,
    pub sync_records: Vec<SyncRecord>,
}

/// `loadSchemaForAppid` + merge + `consolidarItens` + ordenação.
pub fn rebuild_app(
    store: &mut AchievementsStore,
    appid: &str,
    schema: &AchievementSchema,
    progress: &BTreeMap<String, i64>,
    icons: &dyn IconResolver,
) -> RebuildReport {
    let old: Vec<Value> = store
        .items(appid)
        .into_iter()
        .filter(|item| !item.is_null())
        .collect();
    let items_before = old.len();
    if schema.is_empty() {
        return RebuildReport {
            appid: appid.to_string(),
            updated: false,
            icons_resolved: 0,
            items_before,
            items_after: items_before,
            sync_records: Vec::new(),
        };
    }

    let index_old = build_index(&old);
    let mut old_by_key: HashMap<String, usize> = HashMap::new();
    for (position, item) in old.iter().enumerate() {
        let Some(object) = item.as_object() else {
            continue;
        };
        if let Some(apiname) = object.get("apiname").filter(|value| js_truthy_value(value)) {
            old_by_key.insert(format!("apiname:{}", js_string_value(apiname)), position);
        }
        if let (Some(block), Some(bit)) = (object.get("block"), object.get("bit")) {
            if !block.is_null() && !bit.is_null() {
                if let Some(key) = json_block_bit_key(block, bit) {
                    old_by_key.insert(format!("bb:{key}"), position);
                }
            }
        }
    }

    let reais: HashSet<String> = schema
        .iter()
        .filter(|achievement| !achievement.apiname.is_empty())
        .map(|achievement| achievement.apiname.clone())
        .collect();
    let mut used = HashSet::new();
    let mut items = Vec::new();
    let mut sync_records = Vec::new();
    let mut icons_resolved = 0usize;

    for achievement in schema.iter() {
        let key = achievement.key();
        let prev_position = old_by_key
            .get(&format!("apiname:{}", achievement.apiname))
            .copied()
            .or_else(|| old_by_key.get(&format!("bb:{key}")).copied())
            .or_else(|| {
                find_item_index(
                    &index_old,
                    &old,
                    Some(&achievement.apiname),
                    Some(&achievement.name),
                )
            });
        let prev = prev_position.and_then(|position| old.get(position));
        if let Some(position) = prev_position {
            used.insert(position);
        }

        let bin_unlock = progress.get(&key).copied().unwrap_or(0);
        let prev_achieved = prev
            .and_then(|item| item.get("achieved"))
            .map(js_truthy_value)
            .unwrap_or(false);
        let achieved = bin_unlock > 0 || prev_achieved;

        let icon = icons.resolve_icon(appid, &achievement.icon_hash);
        let icongray = icons.resolve_icon(appid, &achievement.icongray_hash);
        if icon.is_some()
            && prev
                .and_then(|item| item.get("icon"))
                .map(|value| value != &Value::String(icon.clone().unwrap_or_default()))
                .unwrap_or(true)
        {
            icons_resolved += 1;
        }

        let mut item = prev
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_else(Map::new);
        item.insert("apiname".into(), Value::String(achievement.apiname.clone()));
        item.insert("title".into(), Value::String(achievement.name.clone()));
        item.insert("desc".into(), Value::String(achievement.desc.clone()));
        item.insert(
            "block".into(),
            number_from_text(&achievement.block).unwrap_or(Value::Null),
        );
        item.insert(
            "bit".into(),
            number_from_text(&achievement.bit).unwrap_or(Value::Null),
        );
        item.insert(
            "icon".into(),
            icon.clone()
                .map(Value::String)
                .or_else(|| prev.and_then(|item| item.get("icon")).cloned())
                .unwrap_or_else(|| Value::String(String::new())),
        );
        item.insert(
            "icongray".into(),
            icongray
                .clone()
                .map(Value::String)
                .or_else(|| prev.and_then(|item| item.get("icongray")).cloned())
                .unwrap_or_else(|| Value::String(String::new())),
        );
        item.insert("achieved".into(), Value::Bool(achieved));
        let unlock_value = if bin_unlock > 0 {
            Value::from(bin_unlock)
        } else {
            prev.and_then(|item| item.get("unlock"))
                .filter(|value| js_truthy_value(value))
                .cloned()
                .unwrap_or_else(|| Value::from(0))
        };
        item.insert("unlock".into(), unlock_value.clone());
        item.insert(
            "percent".into(),
            prev.and_then(|item| item.get("percent"))
                .filter(|value| js_truthy_value(value))
                .cloned()
                .unwrap_or_else(|| Value::from(0)),
        );

        // `aliases` é um campo derivado: o rebuild o reconstitui a partir do
        // item anterior, sem manter o apiname canônico dentro da lista.
        item.remove("aliases");
        let mut item = Value::Object(item);
        if let Some(previous) = prev {
            for alias in aliases_of(previous) {
                remember_alias(&mut item, &alias);
            }
            if let Some(remote) = previous
                .get("remoteApiname")
                .filter(|value| js_truthy_value(value))
            {
                if let Some(object) = item.as_object_mut() {
                    object.insert("remoteApiname".into(), remote.clone());
                }
            }
        }

        let unlock_changed = prev
            .and_then(|item| item.get("unlock"))
            .map(|value| value != &unlock_value)
            .unwrap_or(true);
        let achieved_changed = prev
            .and_then(|item| item.get("achieved"))
            .map(|value| value != &Value::Bool(achieved))
            .unwrap_or(true);
        let prev_value = prev.unwrap_or(&Value::Null);
        if !achievement.apiname.is_empty() && (achieved_changed || unlock_changed) {
            let apiname_sync = prev
                .and_then(|item| item.get("remoteApiname"))
                .filter(|value| js_truthy_value(value))
                .map(js_string_value)
                .unwrap_or_else(|| achievement.apiname.clone());
            sync_records.push(SyncRecord {
                appid: appid.to_string(),
                apiname: apiname_sync,
                unlocked_at: js_number_value(&unlock_value)
                    .filter(|value| value.is_finite())
                    .map(|value| value.trunc() as i64)
                    .unwrap_or(0),
                achieved,
                title: achievement.name.clone(),
                icon: icon.clone(),
                percent: prev_value
                    .get("percent")
                    .filter(|value| js_truthy_value(value))
                    .cloned()
                    .unwrap_or_else(|| Value::from(0)),
            });
        }

        items.push(item);
    }

    for (position, item) in old.iter().enumerate() {
        if !used.contains(&position) {
            items.push(item.clone());
        }
    }

    let (mut items, _) = crate::consolidar::consolidar_items_with_report(items, &reais);
    sort_items(&mut items);
    let items_after = items.len();
    store.set_items(appid, items);

    RebuildReport {
        appid: appid.to_string(),
        updated: true,
        icons_resolved,
        items_before,
        items_after,
        sync_records,
    }
}

/// `number_from_text` equivalente ao `Number(...)` usado em `block`/`bit`.
pub fn number_from_text(text: &str) -> Option<Value> {
    let number = js_number_value(&Value::String(text.to_string()))?;
    number_to_value(number)
}

pub fn number_to_value(number: f64) -> Option<Value> {
    if !number.is_finite() {
        return None;
    }
    if number.fract() == 0.0 && number >= i64::MIN as f64 && number <= i64::MAX as f64 {
        return Some(Value::from(number as i64));
    }
    Number::from_f64(number).map(Value::Number)
}

/// Ordena por `block` e depois `bit`; itens sem os dois inteiros vão para o
/// fim, preservando a ordem relativa (sort estável).
pub fn sort_items(items: &mut [Value]) {
    items.sort_by(|left, right| {
        let left_key = integer_pair(left);
        let right_key = integer_pair(right);
        match (left_key, right_key) {
            (Some(left_key), Some(right_key)) => left_key
                .0
                .partial_cmp(&right_key.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(
                    left_key
                        .1
                        .partial_cmp(&right_key.1)
                        .unwrap_or(std::cmp::Ordering::Equal),
                ),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
}

fn integer_pair(item: &Value) -> Option<(f64, f64)> {
    let object = item.as_object()?;
    let block = integral_number(object.get("block")?)?;
    let bit = integral_number(object.get("bit")?)?;
    Some((block, bit))
}

fn integral_number(value: &Value) -> Option<f64> {
    // `Number.isInteger`: só números JSON contam; strings/bools ficam de fora.
    let Value::Number(number) = value else {
        return None;
    };
    let number = number.as_f64()?;
    if number.is_finite() && number.fract() == 0.0 {
        Some(number)
    } else {
        None
    }
}

/// Opções do `loadAllSchemas` local.
pub struct LoadOptions<'a> {
    pub stats_dir: &'a Path,
    pub permission_allowed: bool,
    pub icons: &'a dyn IconResolver,
    pub scrape: Option<&'a dyn ScrapeSchemaProvider>,
    /// Appids da biblioteca/configurações (o JS os adiciona à união).
    pub extra_appids: &'a [String],
}

impl<'a> LoadOptions<'a> {
    pub fn new(stats_dir: &'a Path, icons: &'a dyn IconResolver) -> Self {
        Self {
            stats_dir,
            permission_allowed: true,
            icons,
            scrape: None,
            extra_appids: &[],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LoadReport {
    pub updated: usize,
    pub icons_copied: usize,
    /// Appids varridos (store + bins + extras).
    pub total_appids: usize,
    /// `Object.keys(store).length` no fim, como o `total` do JS.
    pub total: usize,
    pub apps: Vec<RebuildReport>,
}

/// União de appids: store + schemas `.bin` + appids extras (biblioteca).
pub fn collect_appids(
    store: &AchievementsStore,
    stats_dir: &Path,
    extra: &[String],
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    let mut push = |appid: String| {
        if seen.insert(appid.clone()) {
            output.push(appid);
        }
    };
    for appid in store.appids() {
        push(appid);
    }
    for (appid, _) in crate::schema::list_schema_files(stats_dir) {
        push(appid);
    }
    for appid in extra {
        if let Some(normalized) = crate::r#match::normalize_appid(appid) {
            push(normalized);
        }
    }
    output
}

/// Rebuild de TODOS os appids com schema local/salvo/fallback injetado.
pub fn load_all_schemas(store: &mut AchievementsStore, options: &LoadOptions<'_>) -> LoadReport {
    let appids = collect_appids(store, options.stats_dir, options.extra_appids);
    let progress_files = list_progress_files(options.stats_dir);
    let mut report = LoadReport {
        total_appids: appids.len(),
        ..LoadReport::default()
    };
    for appid in appids {
        let mut schema = load_schema_for_appid(options.stats_dir, &appid);
        if schema.is_empty() {
            schema = schema_from_saved_items(&store.items(&appid));
            if !schema.is_empty() {
                schema.appid = appid.clone();
            }
        }
        if schema.is_empty() {
            if let Some(provider) = options.scrape {
                if let Some(provided) = provider.schema_for_appid(&appid) {
                    schema = provided;
                    if schema.appid.is_empty() {
                        schema.appid = appid.clone();
                    }
                }
            }
        }
        if schema.is_empty() {
            continue;
        }
        let progress = if options.permission_allowed {
            progress_files
                .get(&appid)
                .map(load_progress_map)
                .unwrap_or_default()
        } else {
            BTreeMap::new()
        };
        let rebuild = rebuild_app(store, &appid, &schema, &progress, options.icons);
        report.icons_copied += rebuild.icons_resolved;
        if rebuild.updated {
            report.updated += 1;
        }
        report.apps.push(rebuild);
    }
    report.total = store.appids().len();
    report
}

/// Appids `steam:<id>` presentes em `library.json` + `game_settings.json`.
pub fn configured_appids(library: &Value, settings: &Value) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    let mut push = |value: &Value| {
        let Some(text) = value.as_str() else {
            return;
        };
        if let Some(appid) = crate::r#match::extract_appid(text) {
            if seen.insert(appid.to_string()) {
                output.push(appid.to_string());
            }
        }
    };
    if let Some(games) = library.get("games").and_then(Value::as_array) {
        for game in games {
            if let Some(id) = game.get("id") {
                push(id);
            }
        }
    }
    if let Some(settings) = settings.as_object() {
        for key in settings.keys() {
            push(&Value::String(key.clone()));
        }
    }
    output
}

/// Primeira string não vazia de um schema vindo de JSON (fallback auxiliar).
pub fn first_text(value: Option<&Value>) -> String {
    value
        .filter(|value| js_truthy_value(value))
        .map(js_string_value)
        .map(|text| js_trim(&text).to_string())
        .unwrap_or_default()
}

/// Extrai ícone/nome/descrição de um `SchemaAchievement` para consumidores
/// externos sem depender dos campos internos do loader.
pub fn schema_item_view(achievement: &SchemaAchievement) -> Map<String, Value> {
    let mut item = Map::new();
    item.insert("apiname".into(), Value::String(achievement.apiname.clone()));
    item.insert("title".into(), Value::String(achievement.name.clone()));
    item.insert("desc".into(), Value::String(achievement.desc.clone()));
    item.insert(
        "block".into(),
        number_from_text(&achievement.block).unwrap_or(Value::Null),
    );
    item.insert(
        "bit".into(),
        number_from_text(&achievement.bit).unwrap_or(Value::Null),
    );
    item
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{AchievementSchema, SchemaAchievement};
    use serde_json::json;

    fn achievement(block: &str, bit: &str, apiname: &str, name: &str) -> SchemaAchievement {
        SchemaAchievement {
            block: block.into(),
            bit: bit.into(),
            apiname: apiname.into(),
            name: name.into(),
            desc: format!("desc {apiname}"),
            icon_hash: format!("{apiname}.jpg"),
            icongray_hash: format!("{apiname}_gray.jpg"),
            hidden: false,
            permission: 0,
        }
    }

    fn schema(items: Vec<SchemaAchievement>) -> AchievementSchema {
        AchievementSchema {
            appid: "42".into(),
            achievements: items,
        }
    }

    fn progress(pairs: &[(&str, i64)]) -> BTreeMap<String, i64> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), *value))
            .collect()
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-loader-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn store_tolerates_invalid_json_and_preserves_entry_fields() {
        assert!(AchievementsStore::from_json("not-json").appids().is_empty());
        assert!(AchievementsStore::from_json("[]").appids().is_empty());
        let store = AchievementsStore::from_json(
            r#"{"42":{"at":123,"custom":{"x":1},"items":[{"apiname":"A","unknown":true}]}}"#,
        );
        assert_eq!(store.appids(), ["42"]);
        assert_eq!(store.entry("42").unwrap()["at"], json!(123));
        assert_eq!(store.items("42")[0]["unknown"], json!(true));
    }

    #[test]
    fn set_items_keeps_unknown_entry_fields_and_creates_missing_entry() {
        let mut store = AchievementsStore::from_json(r#"{"42":{"at":1,"x":2}}"#);
        store.set_items("42", vec![json!({"apiname":"A"})]);
        let entry = store.entry("42").unwrap();
        assert_eq!(entry["at"], json!(1));
        assert_eq!(entry["x"], json!(2));
        assert_eq!(entry["items"][0]["apiname"], json!("A"));
        store.set_items("7", vec![]);
        assert_eq!(store.entry("7").unwrap()["items"], json!([]));
    }

    #[test]
    fn item_index_only_uses_items_with_both_block_and_bit() {
        let store = AchievementsStore::from_json(
            r#"{
                "42":{"items":[
                    {"apiname":"A","block":1,"bit":0},
                    {"apiname":"B","block":1},
                    {"apiname":"C","bit":2},
                    {"apiname":"D","block":1,"bit":1}
                ]},
                "43":{"items":[{"apiname":"E"}]}
            }"#,
        );
        let index = load_item_index(&store);
        assert!(index.has_steam_bin_for("42"));
        assert!(index.has("42", "1|0"));
        assert!(index.has("42", "1|1"));
        assert!(!index.has("42", "1|2"));
        assert!(!index.has_steam_bin_for("43"));
    }

    #[test]
    fn rebuild_merges_schema_progress_and_previous_unlock_state() {
        let mut store = AchievementsStore::from_json(
            r#"{"42":{"at":9,"items":[
                {"apiname":"ACH01","title":"Antigo","desc":"d","block":1,"bit":0,"achieved":false,"unlock":0,"percent":12,"unknown":"keep"},
                {"apiname":"ACH02","title":"Dois","block":1,"bit":1,"achieved":true,"unlock":50}
            ]}}"#,
        );
        let schema = schema(vec![
            achievement("1", "0", "ACH01", "Um"),
            achievement("1", "1", "ACH02", "Dois"),
            achievement("1", "2", "ACH03", "Três"),
        ]);
        let progress = progress(&[("1|0", 100)]);
        let report = rebuild_app(&mut store, "42", &schema, &progress, &CdnIcons);
        assert!(report.updated);
        assert_eq!(report.items_before, 2);
        assert_eq!(report.items_after, 3);
        assert_eq!(store.entry("42").unwrap()["at"], json!(9));
        let items = store.items("42");
        assert_eq!(items[0]["apiname"], json!("ACH01"));
        assert_eq!(items[0]["achieved"], json!(true));
        assert_eq!(items[0]["unlock"], json!(100));
        assert_eq!(items[0]["percent"], json!(12));
        assert_eq!(items[0]["unknown"], json!("keep"));
        assert_eq!(items[0]["title"], json!("Um"));
        assert_eq!(
            items[1]["achieved"],
            json!(true),
            "prev achieved é preservado"
        );
        assert_eq!(items[1]["unlock"], json!(50));
        assert_eq!(items[2]["achieved"], json!(false));
        assert_eq!(items[2]["unlock"], json!(0));
    }

    #[test]
    fn negative_progress_never_unlocks_and_prev_unlock_is_kept() {
        let mut store = AchievementsStore::from_json(
            r#"{"42":{"items":[{"apiname":"A","title":"A","block":1,"bit":0,"achieved":false,"unlock":77}]}}"#,
        );
        let schema = schema(vec![achievement("1", "0", "A", "A")]);
        let report = rebuild_app(
            &mut store,
            "42",
            &schema,
            &progress(&[("1|0", -9)]),
            &CdnIcons,
        );
        assert_eq!(store.items("42")[0]["achieved"], json!(false));
        assert_eq!(store.items("42")[0]["unlock"], json!(77));
        assert_eq!(report.items_after, 1);
    }

    #[test]
    fn pulled_orphans_are_preserved_and_consolidation_merges_duplicates() {
        let mut store = AchievementsStore::from_json(
            r#"{"42":{"items":[
                {"apiname":"ach_01","title":"Um","block":0,"bit":0,"achieved":true,"aliases":["PFA_1"]},
                {"apiname":"ACH99","title":"Da outra máquina","achieved":true,"unlock":123}
            ]}}"#,
        );
        let schema = schema(vec![achievement("1", "0", "PFA_1", "Um")]);
        let report = rebuild_app(&mut store, "42", &schema, &BTreeMap::new(), &CdnIcons);
        assert_eq!(report.sync_records.len(), 1);
        let items = store.items("42");
        assert_eq!(items.len(), 2, "par fundido + órfão preservado");
        assert_eq!(items[0]["apiname"], json!("PFA_1"));
        assert_eq!(items[0]["achieved"], json!(true));
        assert_eq!(items[0]["aliases"], json!(["ach_01"]));
        let orphan = items
            .iter()
            .find(|item| item["apiname"] == json!("ACH99"))
            .unwrap();
        assert_eq!(orphan["achieved"], json!(true));
        assert_eq!(orphan["unlock"], json!(123));
        assert!(orphan.get("block").is_none());
    }

    #[test]
    fn duplicate_apiname_entries_do_not_break_and_consolidate_to_one_item() {
        let mut store = AchievementsStore::from_json(r#"{"42":{"items":[]}}"#);
        let schema = schema(vec![
            achievement("1", "0", "DUP", "Mesma conquista"),
            achievement("1", "1", "DUP", "Mesma conquista"),
        ]);
        let report = rebuild_app(&mut store, "42", &schema, &BTreeMap::new(), &CdnIcons);
        assert!(report.updated);
        let items = store.items("42");
        assert_eq!(items.len(), 1, "duplicata de apiname/título vira um item");
        assert_eq!(items[0]["apiname"], json!("DUP"));
        assert_eq!(items[0]["block"], json!(1));
    }

    #[test]
    fn items_are_sorted_by_block_and_bit_with_unknown_last() {
        let mut store = AchievementsStore::from_json(
            r#"{"42":{"items":[{"apiname":"ORPHAN","title":"Sem bit","achieved":false}]}}"#,
        );
        let schema = schema(vec![
            achievement("2", "0", "B", "B"),
            achievement("1", "5", "A5", "A5"),
            achievement("1", "1", "A1", "A1"),
        ]);
        rebuild_app(&mut store, "42", &schema, &BTreeMap::new(), &CdnIcons);
        let apinames: Vec<_> = store
            .items("42")
            .into_iter()
            .map(|item| item["apiname"].as_str().unwrap_or("").to_string())
            .collect();
        assert_eq!(apinames, ["A1", "A5", "B", "ORPHAN"]);
    }

    #[test]
    fn sync_records_use_remote_apiname_and_only_emit_changes() {
        let mut store = AchievementsStore::from_json(
            r#"{"42":{"items":[{"apiname":"ach_01","remoteApiname":"PFA_1","title":"Um","block":1,"bit":0,"achieved":false,"unlock":0,"percent":44}]}}"#,
        );
        let schema = schema(vec![achievement("1", "0", "PFA_1", "Um")]);
        let report = rebuild_app(
            &mut store,
            "42",
            &schema,
            &progress(&[("1|0", 999)]),
            &CdnIcons,
        );
        assert_eq!(report.sync_records.len(), 1);
        let record = &report.sync_records[0];
        assert_eq!(record.apiname, "PFA_1");
        assert_eq!(record.unlocked_at, 999);
        assert_eq!(record.percent, json!(44));
        assert!(record.icon.as_deref().unwrap().contains("PFA_1.jpg"));

        let report = rebuild_app(
            &mut store,
            "42",
            &schema,
            &progress(&[("1|0", 999)]),
            &CdnIcons,
        );
        assert!(report.sync_records.is_empty(), "reprocessar é no-op");
    }

    #[test]
    fn icons_use_steam_cache_copy_then_cdn_and_keep_http_urls() {
        let dir = temp_dir("icons");
        let icon_dir = dir.join("images");
        let cache_dir = dir.join("cache");
        fs::create_dir_all(&icon_dir).unwrap();
        fs::write(icon_dir.join("ACH01.jpg"), b"jpg").unwrap();
        let icons = SteamIcons::new(&icon_dir, &cache_dir);
        let copied = icons.resolve_icon("42", "ACH01.jpg").unwrap();
        assert!(copied.starts_with("file:///"));
        assert!(cache_dir.join("42_ACH01.jpg").is_file());
        let cdn = icons.resolve_icon("42", "missing.jpg").unwrap();
        assert!(cdn.contains("/apps/42/missing.jpg"));
        assert_eq!(
            icons.resolve_icon("42", "https://cdn/x.jpg").as_deref(),
            Some("https://cdn/x.jpg")
        );
        assert_eq!(icons.resolve_icon("42", ""), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolver_prefers_binary_then_saved_items_then_scrape() {
        let dir = temp_dir("resolve");
        let stats = dir.join("stats");
        fs::create_dir_all(&stats).unwrap();
        let mut store = AchievementsStore::from_json(
            r#"{"42":{"items":[{"apiname":"PFA_1","title":"Um","block":1,"bit":0}]}}"#,
        );
        let saved = resolve_schema_for_appid(&store, &stats, "42", true, None);
        assert_eq!(saved.origin, SchemaOrigin::SavedItems);
        assert_eq!(saved.schema.len(), 1);

        struct Scrape;
        impl ScrapeSchemaProvider for Scrape {
            fn schema_for_appid(&self, appid: &str) -> Option<AchievementSchema> {
                let mut schema = AchievementSchema::new(appid);
                schema
                    .achievements
                    .push(achievement("0", "0", "SCRAPE", "Scrape"));
                Some(schema)
            }
        }
        let scraped =
            resolve_schema_for_appid(&AchievementsStore::new(), &stats, "99", true, Some(&Scrape));
        assert_eq!(scraped.origin, SchemaOrigin::Scrape);
        assert_eq!(scraped.schema.achievements[0].apiname, "SCRAPE");

        store.set_items("42", vec![]);
        let missing = resolve_schema_for_appid(&store, &stats, "42", true, None);
        assert_eq!(missing.origin, SchemaOrigin::Missing);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn end_to_end_synthetic_schema_and_progress_bins_unlock_the_item() {
        let dir = temp_dir("bins");
        let stats = dir.join("stats");
        fs::create_dir_all(&stats).unwrap();
        fs::write(
            crate::schema::schema_path(&stats, "42"),
            crate::test_support::schema_bin(&[(
                "1",
                "0",
                "ACH01",
                &[("brazilian", "Primeiros passos")],
            )]),
        )
        .unwrap();
        fs::write(
            stats.join("UserGameStats_76561198000000000_42.bin"),
            crate::test_support::progress_bin(&[("1", "0", 1_700_000_000)]),
        )
        .unwrap();

        let resolved =
            resolve_schema_for_appid(&AchievementsStore::new(), &stats, "42", true, None);
        assert_eq!(resolved.origin, SchemaOrigin::Binary);
        assert_eq!(resolved.schema.len(), 1);
        assert_eq!(resolved.progress.get("1|0"), Some(&1_700_000_000));

        let without_permission =
            resolve_schema_for_appid(&AchievementsStore::new(), &stats, "42", false, None);
        assert!(
            without_permission.progress.is_empty(),
            "captura pausada não lê bin"
        );

        let mut store = AchievementsStore::new();
        rebuild_app(
            &mut store,
            "42",
            &resolved.schema,
            &resolved.progress,
            &CdnIcons,
        );
        let item = &store.items("42")[0];
        assert_eq!(item["apiname"], json!("ACH01"));
        assert_eq!(item["title"], json!("Primeiros passos"));
        assert_eq!(item["achieved"], json!(true));
        assert_eq!(item["unlock"], json!(1_700_000_000));
        assert_eq!(item["block"], json!(1));
        assert_eq!(item["bit"], json!(0));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn progress_files_are_indexed_by_appid() {
        let dir = temp_dir("progress-files");
        fs::write(dir.join("UserGameStats_765_42.bin"), b"x").unwrap();
        fs::write(dir.join("UserGameStats_765_43.bin"), b"x").unwrap();
        fs::write(dir.join("other.bin"), b"x").unwrap();
        let map = list_progress_files(&dir);
        assert!(map.contains_key("42"));
        assert!(map.contains_key("43"));
        assert_eq!(map.len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_all_schemas_rebuilds_union_and_reports_totals() {
        let dir = temp_dir("all");
        let stats = dir.join("stats");
        fs::create_dir_all(&stats).unwrap();
        let mut store = AchievementsStore::from_json(
            r#"{"42":{"items":[{"apiname":"A","title":"A","block":1,"bit":0,"achieved":false}]}}"#,
        );
        // Constrói um schema binário mínimo para o appid 42.
        let path = crate::schema::schema_path(&stats, "42");
        fs::write(
            &path,
            crate::test_support::schema_bin(&[("1", "0", "A", &[("brazilian", "A")])]),
        )
        .unwrap();
        let report = load_all_schemas(
            &mut store,
            &LoadOptions {
                stats_dir: &stats,
                permission_allowed: true,
                icons: &CdnIcons,
                scrape: None,
                extra_appids: &["steam:43".to_string()],
            },
        );
        assert_eq!(report.total_appids, 2);
        assert_eq!(report.updated, 1, "43 não tem schema nem itens salvos");
        assert_eq!(store.items("42")[0]["title"], json!("A"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn configured_appids_reads_library_and_settings_without_duplicates() {
        let library =
            json!({ "games": [{ "id": "steam:10" }, { "id": "steam:10" }, { "id": "gog:1" }] });
        let settings = json!({ "steam:11": {}, "gog:2": {}, "steam:10": {} });
        assert_eq!(configured_appids(&library, &settings), ["10", "11"]);
    }

    #[test]
    fn store_round_trips_and_saves_atomically() {
        let dir = temp_dir("save");
        let path = dir.join("achievements.json");
        let mut store = AchievementsStore::from_json(r#"{"42":{"at":1,"items":[]}}"#);
        store.set_items("42", vec![json!({"apiname":"A"})]);
        save_achievements(&path, &store).unwrap();
        let loaded = load_achievements(&path);
        assert_eq!(loaded.to_value(), store.to_value());
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
