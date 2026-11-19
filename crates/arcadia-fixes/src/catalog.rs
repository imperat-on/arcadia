//! Catálogo de fixes do cliente congelado.
//!
//! Oráculo: `app/electron/fixes.js` (índice de luatools.work + catálogo Ryuu) e
//! `app/electron/main.js` (`fixes:check`, que valida os mesmos formatos antes
//! de gravar o cache).
//!
//! O crate não faz rede: os JSONs são entregues já baixados pela borda e
//! passados a [`FixCatalog`]. Toda a semântica de `Number(appid)`,
//! `pickEntry`, `entryBadge`, `isHypervisor`, `urlEncode` e `sanitizeUrl` é
//! preservada, inclusive os códigos de status (0 = índice indisponível,
//! 404 = appid ausente, 200 = disponível).
//!
//! Além dos campos usados pela UI (`url`, `file`, `badge`, `requiresAuth`,
//! `gameName`), entradas Ryuu aceitam metadados opcionais de release
//! (`size`/`sizeBytes`, `sha256`/`hash`/`hashes`, `version`, `released`) para
//! que o plano de download possa validar tamanho/hash quando a fonte os
//! publica.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Índice de fixes do luatools.work.
pub const FIXES_INDEX_URL: &str = "https://index.luatools.work/fixes-index.json";
/// Base dos bypasses genéricos.
pub const GENERIC_BASE: &str = "https://files.luatools.work/GameBypasses";
/// Base dos online fixes.
pub const ONLINE_BASE: &str = "https://files.luatools.work/OnlineFix1";
/// Catálogo de cracks/bypasses do Ryuu.
pub const RYUU_CATALOG_URL: &str = "https://generator.ryuu.lol/files/fixes.json";
/// Base dos arquivos do Ryuu (a única fonte que exige autenticação).
pub const RYUU_BASE: &str = "https://generator.ryuu.lol/fixes/";

/// Fontes que `fixes:apply` aceita, na ordem do handler IPC.
pub const FIX_URL_BASES: [&str; 3] = [
    "https://files.luatools.work/GameBypasses/",
    "https://files.luatools.work/OnlineFix1/",
    "https://generator.ryuu.lol/fixes/",
];

/// TTL do cache de índices (6 h no JS).
pub const REFRESH_TTL_SECS: u64 = 6 * 60 * 60;
/// Nome do cache do índice luatools (`DATA_DIR/cache`).
pub const FIXES_INDEX_CACHE_FILE: &str = "fixes-index.json";
/// Nome do cache do catálogo Ryuu (`DATA_DIR/cache`).
pub const RYUU_INDEX_CACHE_FILE: &str = "ryuu-index.json";

/// `isFresh(path)`: mtime > 0 e idade < TTL.
pub fn cache_is_fresh(modified_ms: u64, now_ms: u64) -> bool {
    modified_ms > 0 && now_ms.saturating_sub(modified_ms) < REFRESH_TTL_SECS.saturating_mul(1000)
}

/// Tipo de fix, com os mesmos nomes usados em `type` no IPC.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FixType {
    #[default]
    Generic,
    Online,
    Crack,
}

impl FixType {
    pub const ALL: [FixType; 3] = [FixType::Generic, FixType::Online, FixType::Crack];

    pub fn as_str(self) -> &'static str {
        match self {
            FixType::Generic => "generic",
            FixType::Online => "online",
            FixType::Crack => "crack",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "generic" => Some(FixType::Generic),
            "online" => Some(FixType::Online),
            "crack" => Some(FixType::Crack),
            _ => None,
        }
    }
}

impl std::str::FromStr for FixType {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value).ok_or(())
    }
}

impl std::fmt::Display for FixType {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

// O `type || "generic"` do JS aceita qualquer string; tipos desconhecidos caem
// no fluxo genérico em vez de quebrar a desserialização do pedido.
impl<'de> Deserialize<'de> for FixType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self::parse(&value).unwrap_or(FixType::Generic))
    }
}

/// Erro de formato de índice. A borda trata índice inválido como cache
/// ausente (status 0), exatamente como `loadFixesIndex`/`loadRyuuIndex`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogError(pub String);

impl std::fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CatalogError {}

impl CatalogError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// Hash publicado junto de uma entrada do catálogo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileHash {
    pub algorithm: String,
    pub value: String,
}

impl FileHash {
    pub fn new(algorithm: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            algorithm: algorithm.into(),
            value: value.into(),
        }
    }
}

/// Metadados opcionais de release de um fix.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixReleaseInfo {
    #[serde(rename = "sizeBytes", skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hashes: Vec<FileHash>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub released: Option<String>,
}

impl FixReleaseInfo {
    pub fn sha256(&self) -> Option<&str> {
        self.hash("sha256")
    }

    pub fn hash(&self, algorithm: &str) -> Option<&str> {
        self.hashes
            .iter()
            .find(|hash| hash.algorithm.eq_ignore_ascii_case(algorithm))
            .map(|hash| hash.value.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.size_bytes.is_none()
            && self.hashes.is_empty()
            && self.version.is_none()
            && self.released.is_none()
    }
}

/// Entrada crua do catálogo Ryuu, já normalizada.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub badges: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(flatten)]
    pub release: FixReleaseInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub magnet: Option<String>,
}

impl FixEntry {
    /// `const file = entry.file || entry.filename || entry.href`.
    pub fn file_name(&self) -> Option<&str> {
        self.file
            .as_deref()
            .filter(|value| !value.is_empty())
            .or_else(|| self.filename.as_deref().filter(|value| !value.is_empty()))
            .or_else(|| self.href.as_deref().filter(|value| !value.is_empty()))
    }

    /// `entry.file || entry.filename` (o `href` só entra na URL, como no JS).
    pub fn file_or_filename(&self) -> Option<&str> {
        self.file
            .as_deref()
            .filter(|value| !value.is_empty())
            .or_else(|| self.filename.as_deref().filter(|value| !value.is_empty()))
    }

    /// `isHypervisor(entry)` do JS.
    pub fn is_hypervisor(&self) -> bool {
        let badge = self
            .badge
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if badge == "hypervisor" {
            return true;
        }
        let file = self
            .file
            .as_deref()
            .or(self.filename.as_deref())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if file.contains("hypervisor") {
            return true;
        }
        self.badges
            .iter()
            .any(|badge| badge.eq_ignore_ascii_case("hypervisor"))
    }

    /// `entryBadge(entry)`: badge explícito ou o primeiro badge, com
    /// preferência por "bypass".
    pub fn badge_label(&self) -> Option<String> {
        let badge = self
            .badge
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !badge.is_empty() {
            return Some(badge);
        }
        let mut first = None;
        for candidate in &self.badges {
            let candidate = candidate.to_ascii_lowercase();
            if candidate.is_empty() {
                continue;
            }
            if candidate == "bypass" {
                return Some(candidate);
            }
            if first.is_none() {
                first = Some(candidate);
            }
        }
        first
    }

    /// URL final: arquivo no Ryuu (percent-encoded) ou `href` saneado.
    pub fn resolved_url(&self) -> Option<String> {
        if let Some(file) = self.file.as_deref().filter(|value| !value.is_empty()) {
            return Some(format!("{RYUU_BASE}{}", url_encode(file)));
        }
        if let Some(file) = self.filename.as_deref().filter(|value| !value.is_empty()) {
            return Some(format!("{RYUU_BASE}{}", url_encode(file)));
        }
        sanitize_url(self.href.as_deref().unwrap_or_default())
    }

    /// `requiresAuth`: só URLs dentro da base do Ryuu exigem credencial.
    pub fn requires_auth(&self) -> bool {
        self.resolved_url()
            .is_some_and(|url| url.starts_with(RYUU_BASE))
    }

    fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let file = text_field(object.get("file"));
        let filename = text_field(object.get("filename"));
        let href = text_field(object.get("href"));
        let badge = text_field(object.get("badge"));
        let badges = object
            .get("badges")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(text_value).collect())
            .unwrap_or_default();
        let name = text_field(object.get("name"));
        let magnet = text_field(object.get("magnet"))
            .or_else(|| text_field(object.get("magnetUri")))
            .or_else(|| text_field(object.get("magnet_uri")));
        let release = parse_release(object);
        Some(Self {
            file,
            filename,
            href,
            badge,
            badges,
            name,
            release,
            magnet,
        })
    }
}

/// Jogo do catálogo Ryuu (nome + entradas na ordem publicada).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RyuuGame {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<FixEntry>,
}

impl RyuuGame {
    /// `pickEntry(entries)`: primeiro "bypass", ignorando hypervisors; senão
    /// a primeira entrada utilizável.
    pub fn pick_entry(&self) -> Option<&FixEntry> {
        let mut first = None;
        for entry in &self.entries {
            if entry.file_name().is_none() {
                continue;
            }
            if entry.is_hypervisor() {
                continue;
            }
            if entry.badge_label().as_deref() == Some("bypass") {
                return Some(entry);
            }
            if first.is_none() {
                first = Some(entry);
            }
        }
        first
    }
}

/// Item de `fixes:check` no formato exato consumido pela `FixesPanel`.
///
/// Genérico/online serializam somente `available`, `status` e (quando
/// disponível) `url`. Crack acrescenta `file`, `badge`, `requiresAuth` e
/// `gameName`, como no `checkCrackFix` do JS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixCheckItem {
    pub available: bool,
    pub status: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    #[serde(
        default,
        rename = "requiresAuth",
        skip_serializing_if = "Option::is_none"
    )]
    pub requires_auth: Option<bool>,
    #[serde(default, rename = "gameName", skip_serializing_if = "Option::is_none")]
    pub game_name: Option<String>,
}

impl FixCheckItem {
    pub fn unavailable(status: u16) -> Self {
        Self {
            available: false,
            status,
            url: None,
            file: None,
            badge: None,
            requires_auth: None,
            game_name: None,
        }
    }

    pub fn unavailable_with_name(status: u16, game_name: Option<String>) -> Self {
        Self {
            game_name,
            ..Self::unavailable(status)
        }
    }

    pub fn direct(url: impl Into<String>) -> Self {
        Self {
            available: true,
            status: 200,
            url: Some(url.into()),
            ..Self::unavailable(200)
        }
    }

    pub fn crack(candidate: &FixCandidate) -> Self {
        Self {
            available: true,
            status: 200,
            url: Some(candidate.url.clone()),
            file: candidate.file.clone(),
            badge: Some(candidate.badge.clone().unwrap_or_default()),
            requires_auth: Some(candidate.requires_auth),
            game_name: candidate.game_name.clone(),
        }
    }
}

/// Resultado dos três checks de um appid (sem o `ok` do envelope).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixCheck {
    pub generic: FixCheckItem,
    pub online: FixCheckItem,
    pub crack: FixCheckItem,
}

/// Candidato aplicável, com a fonte e os metadados de release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixCandidate {
    pub appid: String,
    #[serde(rename = "type")]
    pub fix_type: FixType,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    #[serde(rename = "requiresAuth")]
    pub requires_auth: bool,
    #[serde(rename = "gameName", default, skip_serializing_if = "Option::is_none")]
    pub game_name: Option<String>,
    #[serde(default, skip_serializing_if = "FixReleaseInfo::is_empty")]
    pub release: FixReleaseInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub magnet: Option<String>,
}

impl FixCandidate {
    /// Gate de aplicação: crack do Ryuu só pode ser baixado com auth.
    pub fn gate(&self, auth_configured: bool) -> FixGate {
        FixGate {
            allowed: !self.requires_auth || auth_configured,
            requires_auth: self.requires_auth,
            auth_configured,
            reason: if self.requires_auth && !auth_configured {
                Some("authentication".to_string())
            } else {
                None
            },
        }
    }
}

/// Resultado do gate (equivalente ao `authConfigured` que a UI consulta).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixGate {
    pub allowed: bool,
    #[serde(rename = "requiresAuth")]
    pub requires_auth: bool,
    #[serde(rename = "authConfigured")]
    pub auth_configured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Índice do luatools.work (`genericFixes`/`onlineFixes`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixesIndex {
    #[serde(default, rename = "genericFixes")]
    pub generic_fixes: Vec<u64>,
    #[serde(default, rename = "onlineFixes")]
    pub online_fixes: Vec<u64>,
}

impl FixesIndex {
    /// `valid` do `main.js`: pelo menos um dos arrays existe.
    pub fn validate(value: &Value) -> bool {
        let Some(object) = value.as_object() else {
            return false;
        };
        object.get("genericFixes").is_some_and(Value::is_array)
            || object.get("onlineFixes").is_some_and(Value::is_array)
    }

    pub fn parse(value: &Value) -> Result<Self, CatalogError> {
        if !Self::validate(value) {
            return Err(CatalogError::new(
                "índice de fixes inválido: genericFixes/onlineFixes ausentes",
            ));
        }
        let object = value
            .as_object()
            .ok_or_else(|| CatalogError::new("índice de fixes não é um objeto"))?;
        Ok(Self {
            generic_fixes: parse_appid_list(object.get("genericFixes")),
            online_fixes: parse_appid_list(object.get("onlineFixes")),
        })
    }
}

/// Catálogo completo em memória.
///
/// Os índices são independentes: um catálogo pode ter só o índice genérico,
/// só o Ryuu, ou ambos. Índice nunca carregado devolve `status: 0` (igual ao
/// `loadFixesIndex`/`loadRyuuIndex` retornando `null`); índice carregado sem
/// o appid devolve `404`.
#[derive(Debug, Clone, Default)]
pub struct FixCatalog {
    generic_fixes: BTreeSet<u64>,
    online_fixes: BTreeSet<u64>,
    fixes_index_loaded: bool,
    ryuu: BTreeMap<String, RyuuGame>,
    ryuu_index_loaded: bool,
    fix_entries_by_appid: BTreeMap<String, RyuuGame>,
}

impl FixCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Constrói a partir dos JSONs já baixados. Índices inválidos são
    /// ignorados (equivalente ao `load*Index` devolver `null`).
    pub fn from_values(fixes_index: Option<&Value>, ryuu_index: Option<&Value>) -> Self {
        let mut catalog = Self::new();
        if let Some(value) = fixes_index {
            let _ = catalog.set_fixes_index(value);
        }
        if let Some(value) = ryuu_index {
            let _ = catalog.set_ryuu_index(value);
        }
        catalog
    }

    /// Igual a [`Self::from_values`], mas propaga formato inválido.
    pub fn try_from_values(
        fixes_index: Option<&Value>,
        ryuu_index: Option<&Value>,
    ) -> Result<Self, CatalogError> {
        let mut catalog = Self::new();
        if let Some(value) = fixes_index {
            catalog.set_fixes_index(value)?;
        }
        if let Some(value) = ryuu_index {
            catalog.set_ryuu_index(value)?;
        }
        Ok(catalog)
    }

    pub fn set_fixes_index(&mut self, value: &Value) -> Result<(), CatalogError> {
        if !FixesIndex::validate(value) {
            return Err(CatalogError::new(
                "índice de fixes inválido: genericFixes/onlineFixes ausentes",
            ));
        }
        let index = FixesIndex::parse(value)?;
        self.generic_fixes = index.generic_fixes.into_iter().collect();
        self.online_fixes = index.online_fixes.into_iter().collect();
        self.fixes_index_loaded = true;
        Ok(())
    }

    pub fn set_ryuu_index(&mut self, value: &Value) -> Result<(), CatalogError> {
        if !RyuuIndex::validate(value) {
            return Err(CatalogError::new("índice Ryuu inválido"));
        }
        let index = RyuuIndex::parse(value)?;
        self.ryuu = index.fixes;
        self.fix_entries_by_appid = index.fixes_by_appid;
        self.ryuu_index_loaded = true;
        Ok(())
    }

    pub fn fixes_index_loaded(&self) -> bool {
        self.fixes_index_loaded
    }

    pub fn ryuu_index_loaded(&self) -> bool {
        self.ryuu_index_loaded
    }

    pub fn generic_appids(&self) -> Vec<u64> {
        self.generic_fixes.iter().copied().collect()
    }

    pub fn online_appids(&self) -> Vec<u64> {
        self.online_fixes.iter().copied().collect()
    }

    /// Entradas de um appid no formato de mapa `{"fixes": {"<appid>": [...]}}`.
    pub fn ryuu_game(&self, appid: &str) -> Option<&RyuuGame> {
        let number = js_number(appid)?;
        if number == 0.0 || !number.is_finite() {
            return None;
        }
        let canonical = canonical_appid(number);
        self.ryuu
            .get(&canonical)
            .or_else(|| self.fix_entries_by_appid.get(&canonical))
    }

    /// `checkGenericFix(appid)`.
    pub fn check_generic(&self, appid: &str) -> FixCheckItem {
        if !self.fixes_index_loaded {
            return FixCheckItem::unavailable(0);
        }
        let Some(number) = js_number(appid) else {
            return FixCheckItem::unavailable(404);
        };
        let Some(id) = canonical_appid_u64(number) else {
            return FixCheckItem::unavailable(404);
        };
        if self.generic_fixes.contains(&id) {
            FixCheckItem::direct(format!("{GENERIC_BASE}/{}.zip", canonical_appid(number)))
        } else {
            FixCheckItem::unavailable(404)
        }
    }

    /// `checkOnlineFix(appid)`.
    pub fn check_online(&self, appid: &str) -> FixCheckItem {
        if !self.fixes_index_loaded {
            return FixCheckItem::unavailable(0);
        }
        let Some(number) = js_number(appid) else {
            return FixCheckItem::unavailable(404);
        };
        let Some(id) = canonical_appid_u64(number) else {
            return FixCheckItem::unavailable(404);
        };
        if self.online_fixes.contains(&id) {
            FixCheckItem::direct(format!("{ONLINE_BASE}/{}.zip", canonical_appid(number)))
        } else {
            FixCheckItem::unavailable(404)
        }
    }

    /// `checkCrackFix(appid)`.
    pub fn check_crack(&self, appid: &str) -> FixCheckItem {
        let Some(number) = js_number(appid) else {
            return FixCheckItem::unavailable(0);
        };
        if number == 0.0 || !number.is_finite() {
            return FixCheckItem::unavailable(0);
        }
        if !self.ryuu_index_loaded {
            return FixCheckItem::unavailable(0);
        }
        let Some(game) = self.ryuu_game(appid) else {
            return FixCheckItem::unavailable(404);
        };
        match self.candidate(appid, FixType::Crack) {
            Some(candidate) => FixCheckItem::crack(&candidate),
            None => FixCheckItem::unavailable_with_name(404, game.name.clone()),
        }
    }

    /// Os três checks de uma vez (ordem do `Promise.all` do IPC).
    pub fn check(&self, appid: &str) -> FixCheck {
        FixCheck {
            generic: self.check_generic(appid),
            online: self.check_online(appid),
            crack: self.check_crack(appid),
        }
    }

    /// Candidato aplicável para um tipo. `None` se indisponível ou entrada
    /// filtrada (ex.: hypervisor).
    pub fn candidate(&self, appid: &str, fix_type: FixType) -> Option<FixCandidate> {
        let number = js_number(appid)?;
        match fix_type {
            FixType::Generic => {
                let id = canonical_appid_u64(number)?;
                if !self.fixes_index_loaded || !self.generic_fixes.contains(&id) {
                    return None;
                }
                Some(FixCandidate {
                    appid: canonical_appid(number),
                    fix_type,
                    url: format!("{GENERIC_BASE}/{}.zip", canonical_appid(number)),
                    file: Some(format!("{}.zip", canonical_appid(number))),
                    badge: None,
                    requires_auth: false,
                    game_name: None,
                    release: FixReleaseInfo::default(),
                    magnet: None,
                })
            }
            FixType::Online => {
                let id = canonical_appid_u64(number)?;
                if !self.fixes_index_loaded || !self.online_fixes.contains(&id) {
                    return None;
                }
                Some(FixCandidate {
                    appid: canonical_appid(number),
                    fix_type,
                    url: format!("{ONLINE_BASE}/{}.zip", canonical_appid(number)),
                    file: Some(format!("{}.zip", canonical_appid(number))),
                    badge: None,
                    requires_auth: false,
                    game_name: None,
                    release: FixReleaseInfo::default(),
                    magnet: None,
                })
            }
            FixType::Crack => {
                if number == 0.0 || !number.is_finite() || !self.ryuu_index_loaded {
                    return None;
                }
                let game = self.ryuu_game(appid)?;
                let entry = game.pick_entry()?;
                let url = entry.resolved_url()?;
                Some(FixCandidate {
                    appid: canonical_appid(number),
                    fix_type,
                    requires_auth: url.starts_with(RYUU_BASE),
                    url,
                    file: entry.file_or_filename().map(str::to_string),
                    badge: entry.badge_label(),
                    game_name: game.name.clone(),
                    release: entry.release.clone(),
                    magnet: entry.magnet.clone(),
                })
            }
        }
    }
}

struct RyuuIndex {
    fixes: BTreeMap<String, RyuuGame>,
    fixes_by_appid: BTreeMap<String, RyuuGame>,
}

impl RyuuIndex {
    /// `valid` do `main.js`: array raiz ou objeto com `fixes` objeto.
    ///
    /// O JS usa `typeof d.fixes === "object"`, que também aceita arrays;
    /// `{"fixes":[]}` é considerado válido para o cache (embora não ache
    /// nenhum appid).
    pub fn validate(value: &Value) -> bool {
        match value {
            Value::Array(_) => true,
            Value::Object(object) => matches!(
                object.get("fixes"),
                Some(Value::Object(_) | Value::Array(_))
            ),
            _ => false,
        }
    }

    fn parse(value: &Value) -> Result<Self, CatalogError> {
        let mut index = Self {
            fixes: BTreeMap::new(),
            fixes_by_appid: BTreeMap::new(),
        };
        match value {
            Value::Object(object) => match object.get("fixes") {
                Some(Value::Object(fixes)) => {
                    for (appid, entries) in fixes {
                        if let Some(game) = parse_ryuu_game(None, entries) {
                            index.fixes.insert(appid.clone(), game);
                        }
                    }
                }
                // `index.fixes[String(n)]` num array usa o índice numérico.
                Some(Value::Array(entries)) => {
                    for (position, entry) in entries.iter().enumerate() {
                        if let Some(game) = parse_ryuu_game(None, entry) {
                            index.fixes.insert(position.to_string(), game);
                        }
                    }
                }
                _ => return Err(CatalogError::new("índice Ryuu sem objeto fixes")),
            },
            Value::Array(games) => {
                for game in games {
                    let Some(object) = game.as_object() else {
                        continue;
                    };
                    let Some(number) = object.get("appid").and_then(js_number_value) else {
                        continue;
                    };
                    let Some(id) = canonical_appid_u64(number) else {
                        continue;
                    };
                    if index.fixes_by_appid.contains_key(&id.to_string()) {
                        continue;
                    }
                    let name = text_field(object.get("name"));
                    let game = parse_ryuu_game(name, object.get("fixes").unwrap_or(&Value::Null));
                    if let Some(game) = game {
                        index.fixes_by_appid.insert(id.to_string(), game);
                    }
                }
            }
            _ => return Err(CatalogError::new("índice Ryuu inválido")),
        }
        Ok(index)
    }
}

fn parse_ryuu_game(name: Option<String>, entries: &Value) -> Option<RyuuGame> {
    let entries = entries.as_array()?;
    Some(RyuuGame {
        name,
        entries: entries.iter().filter_map(FixEntry::from_value).collect(),
    })
}

/// `validate_fixes_index` usado pelo IPC antes de gravar o cache.
pub fn valid_fixes_index(value: &Value) -> bool {
    FixesIndex::validate(value)
}

/// `validate_ryuu_index` usado pelo IPC antes de gravar o cache.
pub fn valid_ryuu_index(value: &Value) -> bool {
    RyuuIndex::validate(value)
}

/// `String(appid || "").replace(/^steam:/, "")`.
pub fn normalize_appid(raw: &str) -> String {
    raw.strip_prefix("steam:").unwrap_or(raw).to_string()
}

/// Componente seguro para nomes de arquivo de estado/temporários.
///
/// Appids reais são numéricos (ou `steam_123`); qualquer separador, `..`,
/// caractere de controle ou nome reservado é rejeitado antes de virar caminho.
pub fn appid_path_component(raw: &str) -> Option<String> {
    if raw.is_empty()
        || raw.len() > 128
        || raw == "."
        || raw == ".."
        || !raw.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return None;
    }
    Some(raw.to_string())
}

/// `true` se a URL pertence a uma das fontes aceitas por `fixes:apply`.
pub fn fix_url_allowed(url: &str) -> bool {
    FIX_URL_BASES.iter().any(|base| url.starts_with(base))
}

/// Tipo da fonte reconhecida pela URL (última base vence, como no JS).
pub fn fix_url_type(url: &str) -> Option<FixType> {
    if url.starts_with(FIX_URL_BASES[0]) {
        Some(FixType::Generic)
    } else if url.starts_with(FIX_URL_BASES[1]) {
        Some(FixType::Online)
    } else if url.starts_with(FIX_URL_BASES[2]) {
        Some(FixType::Crack)
    } else {
        None
    }
}

/// Percent-encode do `urlEncode` do JS. O oráculo itera unidades UTF-16
/// (`charCodeAt(0)`) e usa `padStart(2, "0")`; repetimos isso para nomes com
/// acento/emoji ficarem byte a byte iguais aos do cliente congelado.
pub fn url_encode(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for unit in value.encode_utf16() {
        let allowed = unit <= 0x7F
            && ((unit as u8).is_ascii_alphanumeric()
                || matches!(unit as u8, b'-' | b'.' | b'_' | b'~'));
        if allowed {
            output.push(unit as u8 as char);
        } else {
            output.push('%');
            output.push_str(&format!("{unit:02X}"));
        }
    }
    output
}

/// `sanitizeUrl(href)`: percent-encode espaços e metacaracteres.
pub fn sanitize_url(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    let mut output = String::with_capacity(raw.len());
    for character in raw.chars() {
        if character.is_whitespace()
            || matches!(
                character,
                '"' | '<' | '>' | '\\' | '^' | '`' | '{' | '|' | '}'
            )
        {
            for unit in character.encode_utf16(&mut [0u16; 2]) {
                output.push('%');
                output.push_str(&format!("{unit:02X}"));
            }
        } else {
            output.push(character);
        }
    }
    Some(output)
}

/// `Number(value)` para uma string, com o mesmo tratamento de vazio,
/// hexadecimal e infinito que os checks de appid precisam.
pub fn js_number(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Some(0.0);
    }
    let lower = trimmed.to_ascii_lowercase();
    // Prefixos numéricos não aceitam sinal no `Number` do JS ("-0x10" é NaN).
    if let Some(hex) = lower.strip_prefix("0x") {
        return u64::from_str_radix(hex, 16).ok().map(|value| value as f64);
    }
    if let Some(binary) = lower.strip_prefix("0b") {
        return u64::from_str_radix(binary, 2)
            .ok()
            .map(|value| value as f64);
    }
    if let Some(octal) = lower.strip_prefix("0o") {
        return u64::from_str_radix(octal, 8).ok().map(|value| value as f64);
    }
    let (sign, digits) = match lower.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, lower.strip_prefix('+').unwrap_or(&lower)),
    };
    if digits == "infinity" {
        return Some(sign * f64::INFINITY);
    }
    trimmed.parse::<f64>().ok()
}

fn js_number_value(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => js_number(text),
        Value::Bool(boolean) => Some(if *boolean { 1.0 } else { 0.0 }),
        Value::Null => Some(0.0),
        Value::Array(items) => {
            if items.is_empty() {
                Some(0.0)
            } else if items.len() == 1 {
                js_number_value(&items[0])
            } else {
                None
            }
        }
        Value::Object(_) => None,
    }
}

fn canonical_appid(number: f64) -> String {
    if number == 0.0 {
        return "0".to_string();
    }
    if number.fract() == 0.0 && number.abs() < 1e21 {
        format!("{number:.0}")
    } else {
        format!("{number}")
    }
}

fn canonical_appid_u64(number: f64) -> Option<u64> {
    if !number.is_finite() || number < 0.0 || number.fract() != 0.0 || number > u64::MAX as f64 {
        return None;
    }
    Some(number as u64)
}

fn parse_appid_list(value: Option<&Value>) -> Vec<u64> {
    let Some(items) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut appids = BTreeSet::new();
    for item in items {
        if let Some(number) = js_number_value(item) {
            if let Some(id) = canonical_appid_u64(number) {
                appids.insert(id);
            }
        }
    }
    appids.into_iter().collect()
}

fn text_field(value: Option<&Value>) -> Option<String> {
    value.and_then(text_value)
}

fn text_value(value: &Value) -> Option<String> {
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(boolean) => boolean.to_string(),
        _ => return None,
    };
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn parse_release(object: &serde_json::Map<String, Value>) -> FixReleaseInfo {
    let size_bytes = object
        .get("sizeBytes")
        .and_then(parse_size_value)
        .or_else(|| object.get("size").and_then(parse_size_value))
        .or_else(|| object.get("bytes").and_then(parse_size_value));
    let mut hashes = Vec::new();
    if let Some(value) = object.get("hashes") {
        match value {
            Value::Object(map) => {
                for (algorithm, hash) in map {
                    if let Some(hash) = text_value(hash) {
                        hashes.push(FileHash::new(algorithm.clone(), hash));
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    match item {
                        Value::String(hash) => {
                            hashes.push(FileHash::new(guess_algorithm(hash), hash.clone()));
                        }
                        Value::Object(map) => {
                            let algorithm = text_field(map.get("algorithm"))
                                .or_else(|| text_field(map.get("algo")))
                                .unwrap_or_else(|| "sha256".to_string());
                            if let Some(hash) =
                                text_field(map.get("value")).or_else(|| text_field(map.get("hash")))
                            {
                                hashes.push(FileHash::new(algorithm, hash));
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(hash) = text_field(object.get("sha256")) {
        hashes.push(FileHash::new("sha256", hash));
    }
    if let Some(hash) = text_field(object.get("md5")) {
        hashes.push(FileHash::new("md5", hash));
    }
    if let Some(hash) = text_field(object.get("hash")) {
        hashes.push(FileHash::new(guess_algorithm(&hash), hash));
    }
    dedupe_hashes(&mut hashes);
    FixReleaseInfo {
        size_bytes,
        hashes,
        version: text_field(object.get("version")),
        released: text_field(object.get("released"))
            .or_else(|| text_field(object.get("releasedAt")))
            .or_else(|| text_field(object.get("updatedAt"))),
    }
}

fn dedupe_hashes(hashes: &mut Vec<FileHash>) {
    let mut seen = BTreeSet::new();
    hashes.retain(|hash| {
        let key = (
            hash.algorithm.to_ascii_lowercase(),
            hash.value.to_ascii_lowercase(),
        );
        seen.insert(key)
    });
}

fn guess_algorithm(hash: &str) -> String {
    match hash.trim().len() {
        32 => "md5".to_string(),
        40 => "sha1".to_string(),
        _ => "sha256".to_string(),
    }
}

fn parse_size_value(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number
            .as_f64()
            .filter(|value| *value >= 0.0)
            .map(|value| value as u64),
        Value::String(text) => parse_size(text),
        _ => None,
    }
}

/// Aceita `"123456"`, `"123,456"` e formatos humanos (`"12.5 MB"`).
pub fn parse_size(raw: &str) -> Option<u64> {
    let cleaned = raw.trim().replace(',', "");
    if cleaned.is_empty() {
        return None;
    }
    if let Ok(bytes) = cleaned.parse::<u64>() {
        return Some(bytes);
    }
    let mut parts = cleaned.split_whitespace();
    let number = parts.next()?.parse::<f64>().ok()?;
    if !number.is_finite() || number < 0.0 {
        return None;
    }
    let unit = parts.next().unwrap_or("B").to_ascii_lowercase();
    let multiplier = match unit.as_str() {
        "b" | "byte" | "bytes" => 1.0,
        "k" | "kb" => 1000.0,
        "kib" => 1024.0,
        "m" | "mb" => 1_000_000.0,
        "mib" => 1_048_576.0,
        "g" | "gb" => 1_000_000_000.0,
        "gib" => 1_073_741_824.0,
        _ => return None,
    };
    let bytes = number * multiplier;
    if bytes > u64::MAX as f64 {
        None
    } else {
        Some(bytes.round() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_fixes() -> Value {
        json!({
            "genericFixes": [100, "200", 300],
            "onlineFixes": [100, 400]
        })
    }

    fn sample_ryuu() -> Value {
        json!({
            "fixes": {
                "100": [
                    {"file": "hypervisor.zip", "badge": "hypervisor"},
                    {"file": "first fix.zip", "badge": "crack"},
                    {"file": "bypass.zip", "badge": "bypass", "size": "12.5 MB", "sha256": "aabb"}
                ],
                "200": [
                    {"href": "https://example.test/fix file.zip", "badge": "bypass"}
                ]
            }
        })
    }

    #[test]
    fn generic_and_online_use_canonical_numeric_appid() {
        let catalog = FixCatalog::from_values(Some(&sample_fixes()), None);
        assert_eq!(
            catalog.check_generic("0100").url.as_deref(),
            Some("https://files.luatools.work/GameBypasses/100.zip")
        );
        assert_eq!(catalog.check_online("100").status, 200);
        assert_eq!(catalog.check_online("200").status, 404);
        assert_eq!(catalog.check_generic("abc").status, 404);
    }

    #[test]
    fn missing_index_reports_status_zero_and_loaded_index_reports_404() {
        let empty = FixCatalog::new();
        assert_eq!(empty.check_generic("100").status, 0);
        assert_eq!(empty.check_online("100").status, 0);
        assert_eq!(empty.check_crack("100").status, 0);

        let loaded = FixCatalog::from_values(Some(&json!({"genericFixes": []})), None);
        assert_eq!(loaded.check_generic("100").status, 404);
        assert_eq!(loaded.check_online("100").status, 404);
    }

    #[test]
    fn crack_prefers_bypass_and_skips_hypervisor() {
        let catalog = FixCatalog::from_values(Some(&sample_fixes()), Some(&sample_ryuu()));
        let check = catalog.check_crack("100");
        assert!(check.available);
        assert_eq!(check.status, 200);
        assert_eq!(check.badge.as_deref(), Some("bypass"));
        assert_eq!(
            check.url.as_deref(),
            Some("https://generator.ryuu.lol/fixes/bypass.zip")
        );
        assert_eq!(check.requires_auth, Some(true));
    }

    #[test]
    fn crack_file_is_percent_encoded_and_href_is_sanitized() {
        let catalog = FixCatalog::from_values(None, Some(&sample_ryuu()));
        let first = catalog.check_crack("100");
        assert!(first
            .url
            .as_deref()
            .is_some_and(|url| url.ends_with("bypass.zip")));
        let second = catalog.check_crack("200");
        assert_eq!(
            second.url.as_deref(),
            Some("https://example.test/fix%20file.zip")
        );
        assert_eq!(second.requires_auth, Some(false));
        // `file` no JS é `entry.file || entry.filename`; href não conta.
        assert_eq!(second.file, None);
    }

    #[test]
    fn crack_array_shape_exposes_game_name() {
        let ryuu = json!([
            {"appid": 7, "name": "Seven", "fixes": [
                {"filename": "seven.7z", "badges": ["crack", "bypass"]}
            ]}
        ]);
        let catalog = FixCatalog::from_values(None, Some(&ryuu));
        let check = catalog.check_crack("7");
        assert_eq!(check.game_name.as_deref(), Some("Seven"));
        assert_eq!(
            check.url.as_deref(),
            Some("https://generator.ryuu.lol/fixes/seven.7z")
        );
    }

    #[test]
    fn unavailable_crack_keeps_game_name_only_in_array_form() {
        let ryuu = json!([
            {"appid": 7, "name": "Seven", "fixes": [
                {"file": "x-hypervisor.zip", "badge": "bypass"}
            ]}
        ]);
        let catalog = FixCatalog::from_values(None, Some(&ryuu));
        let check = catalog.check_crack("7");
        assert!(!check.available);
        assert_eq!(check.status, 404);
        assert_eq!(check.game_name.as_deref(), Some("Seven"));
    }

    #[test]
    fn invalid_crack_appid_is_status_zero() {
        let catalog = FixCatalog::from_values(Some(&sample_fixes()), Some(&sample_ryuu()));
        assert_eq!(catalog.check_crack("").status, 0);
        assert_eq!(catalog.check_crack("0").status, 0);
        assert_eq!(catalog.check_crack("nope").status, 0);
        assert_eq!(catalog.check_crack("100").status, 200);
    }

    #[test]
    fn badge_label_prefers_bypass_in_badges_and_lowercases() {
        let entry = FixEntry {
            badges: vec!["CRACK".into(), "BYPASS".into()],
            ..FixEntry::default()
        };
        assert_eq!(entry.badge_label().as_deref(), Some("bypass"));
        let entry = FixEntry {
            badges: vec!["First".into(), "Second".into()],
            ..FixEntry::default()
        };
        assert_eq!(entry.badge_label().as_deref(), Some("first"));
        let entry = FixEntry {
            badge: Some("Manual".into()),
            ..FixEntry::default()
        };
        assert_eq!(entry.badge_label().as_deref(), Some("manual"));
    }

    #[test]
    fn hypervisor_detection_covers_badge_file_and_badges() {
        assert!(FixEntry {
            badge: Some("Hypervisor".into()),
            ..FixEntry::default()
        }
        .is_hypervisor());
        assert!(FixEntry {
            file: Some("GAME-HYPERVISOR.zip".into()),
            ..FixEntry::default()
        }
        .is_hypervisor());
        assert!(FixEntry {
            badges: vec!["hypervisor".into()],
            ..FixEntry::default()
        }
        .is_hypervisor());
        assert!(!FixEntry {
            file: Some("normal.zip".into()),
            ..FixEntry::default()
        }
        .is_hypervisor());
    }

    #[test]
    fn fix_index_validation_matches_main_js() {
        assert!(valid_fixes_index(&json!({"genericFixes": []})));
        assert!(valid_fixes_index(&json!({"onlineFixes": [1]})));
        assert!(!valid_fixes_index(&json!({"genericFixes": "no"})));
        assert!(!valid_fixes_index(&json!([])));
        assert!(valid_ryuu_index(&json!([])));
        assert!(valid_ryuu_index(&json!({"fixes": {}})));
        // `typeof [] === "object"` no JS: o cache aceita fixes array.
        assert!(valid_ryuu_index(&json!({"fixes": []})));
        assert!(!valid_ryuu_index(&json!({"fixes": "no"})));
        assert!(!valid_ryuu_index(&json!("no")));
    }

    #[test]
    fn invalid_indexes_are_rejected_and_lossy_constructor_ignores_them() {
        assert!(FixCatalog::try_from_values(Some(&json!({})), None).is_err());
        assert!(FixCatalog::try_from_values(None, Some(&json!({"fixes": "no"}))).is_err());
        let catalog = FixCatalog::from_values(Some(&json!({})), Some(&json!({})));
        assert!(!catalog.fixes_index_loaded());
        assert!(!catalog.ryuu_index_loaded());
    }

    #[test]
    fn release_metadata_parses_size_hashes_version_and_date() {
        let entry = FixEntry::from_value(&json!({
            "file": "fix.zip",
            "sizeBytes": "1.5 GiB",
            "hashes": {"sha256": "ABC", "md5": "DEF"},
            "hash": "aabbccddeeff00112233445566778899",
            "version": "1.2.3",
            "releasedAt": "2026-01-02"
        }))
        .unwrap();
        assert_eq!(entry.release.size_bytes, Some(1_610_612_736));
        assert_eq!(entry.release.sha256(), Some("ABC"));
        assert_eq!(entry.release.hash("md5"), Some("DEF"));
        assert_eq!(entry.release.version.as_deref(), Some("1.2.3"));
        assert_eq!(entry.release.released.as_deref(), Some("2026-01-02"));
        assert_eq!(entry.release.hash("md5"), Some("DEF"));
    }

    #[test]
    fn size_parser_accepts_plain_and_human_units() {
        assert_eq!(parse_size("123"), Some(123));
        assert_eq!(parse_size("1,234"), Some(1234));
        assert_eq!(parse_size("12.5 MB"), Some(12_500_000));
        assert_eq!(parse_size("2 MiB"), Some(2_097_152));
        assert_eq!(parse_size("nope"), None);
        assert_eq!(parse_size("-1"), None);
    }

    #[test]
    fn candidate_gate_requires_auth_only_for_ryuu() {
        let catalog = FixCatalog::from_values(Some(&sample_fixes()), Some(&sample_ryuu()));
        let crack = catalog.candidate("100", FixType::Crack).unwrap();
        assert!(crack.requires_auth);
        assert!(!crack.gate(false).allowed);
        assert_eq!(crack.gate(false).reason.as_deref(), Some("authentication"));
        assert!(crack.gate(true).allowed);
        let generic = catalog.candidate("100", FixType::Generic).unwrap();
        assert!(!generic.requires_auth);
        assert!(generic.gate(false).allowed);
    }

    #[test]
    fn candidate_crack_carries_release_metadata() {
        let catalog = FixCatalog::from_values(None, Some(&sample_ryuu()));
        let crack = catalog.candidate("100", FixType::Crack).unwrap();
        assert_eq!(crack.release.size_bytes, Some(12_500_000));
        assert_eq!(crack.release.sha256(), Some("aabb"));
        assert_eq!(crack.file.as_deref(), Some("bypass.zip"));
    }

    #[test]
    fn url_encode_and_sanitize_match_frozen_client() {
        assert_eq!(url_encode("a b/c+d"), "a%20b%2Fc%2Bd");
        assert_eq!(url_encode("a-._~"), "a-._~");
        // Fiel ao `charCodeAt` do oráculo (não é UTF-8).
        assert_eq!(url_encode("é"), "%E9");
        assert_eq!(url_encode("€"), "%20AC");
        assert_eq!(
            sanitize_url("https://x/a b\"c\\d").as_deref(),
            Some("https://x/a%20b%22c%5Cd")
        );
        assert_eq!(sanitize_url(""), None);
    }

    #[test]
    fn normalize_appid_strips_only_steam_prefix() {
        assert_eq!(normalize_appid("steam:440"), "440");
        assert_eq!(normalize_appid("440"), "440");
        assert_eq!(normalize_appid("epic:steam:440"), "epic:steam:440");
    }

    #[test]
    fn fix_url_type_routes_known_bases() {
        assert_eq!(
            fix_url_type("https://files.luatools.work/GameBypasses/1.zip"),
            Some(FixType::Generic)
        );
        assert_eq!(
            fix_url_type("https://files.luatools.work/OnlineFix1/1.zip"),
            Some(FixType::Online)
        );
        assert_eq!(
            fix_url_type("https://generator.ryuu.lol/fixes/1.zip"),
            Some(FixType::Crack)
        );
        assert_eq!(fix_url_type("https://evil.test/1.zip"), None);
        assert!(!fix_url_allowed(
            "https://files.luatools.work/GameBypassesX/1.zip"
        ));
    }

    #[test]
    fn ryuu_map_keys_keep_js_string_semantics() {
        let catalog = FixCatalog::from_values(
            None,
            Some(&json!({"fixes": {"0123": [{"file": "x.zip", "badge": "bypass"}]}})),
        );
        // String(Number("0123")) === "123", então a chave "0123" não casa.
        assert_eq!(catalog.check_crack("123").status, 404);
        assert_eq!(catalog.check_crack("0123").status, 404);
    }

    #[test]
    fn ryuu_fixes_array_is_indexed_like_js_property_access() {
        let catalog = FixCatalog::from_values(
            None,
            Some(&json!({"fixes": [null, [{"file": "indexed.zip", "badge": "bypass"}]]})),
        );
        assert!(catalog.ryuu_index_loaded());
        assert_eq!(catalog.check_crack("1").status, 200);
        assert_eq!(catalog.check_crack("2").status, 404);
    }

    #[test]
    fn available_crack_without_badge_serializes_empty_badge_like_js() {
        let catalog = FixCatalog::from_values(
            None,
            Some(&json!({"fixes": {"5": [{"file": "plain.zip"}]}})),
        );
        let item = catalog.check_crack("5");
        assert!(item.available);
        assert_eq!(item.badge.as_deref(), Some(""));
        let value = serde_json::to_value(&item).unwrap();
        assert_eq!(value["badge"], json!(""));
    }

    #[test]
    fn candidate_keeps_magnet_and_unknown_type_defaults_to_generic() {
        let hash = "0123456789abcdef0123456789abcdef01234567";
        let catalog = FixCatalog::from_values(
            Some(&json!({"genericFixes": [9]})),
            Some(&json!({"fixes": {"9": [{
                "file": "magnet.zip",
                "badge": "bypass",
                "magnet": format!("magnet:?xt=urn:btih:{hash}")
            }]}})),
        );
        let candidate = catalog.candidate("9", FixType::Crack).unwrap();
        let uri = format!("magnet:?xt=urn:btih:{hash}");
        assert_eq!(candidate.magnet.as_deref(), Some(uri.as_str()));
        let unknown: FixType = serde_json::from_str("\"something-new\"").unwrap();
        assert_eq!(unknown, FixType::Generic);
    }

    #[test]
    fn cache_freshness_matches_six_hour_ttl() {
        let now = 10_000_000_000u64;
        assert!(!cache_is_fresh(0, now));
        assert!(cache_is_fresh(now - 1000, now));
        assert!(cache_is_fresh(now - REFRESH_TTL_SECS * 1000 + 1, now));
        assert!(!cache_is_fresh(now - REFRESH_TTL_SECS * 1000, now));
        // mtime no futuro também é "fresco" no JS (idade negativa < TTL).
        assert!(cache_is_fresh(now + 1, now));
    }
}
