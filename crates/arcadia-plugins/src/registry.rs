//! Registro local de plugins — port 1:1 de `app/electron/plugins/registry.js`.
//!
//! O registro guarda somente referências e estado de ativação; nunca baixa,
//! descompacta, importa ou executa código de terceiros. O caminho privado do
//! pacote fica apenas no `registry.json` e nunca aparece nos descritores
//! públicos devolvidos à UI.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::manifest::{
    compute_entry_sha256, is_inside, normalize_id, public_manifest, read_manifest, resolve_path,
    resolve_plugin_entry, validate_manifest_with, PluginManifest, ValidateManifestOptions,
    ENTRY_DIGEST_ALIASES, MANIFEST_FILE, MANIFEST_VERSION, SDK_API_VERSION,
};
use crate::trust::{
    atomic_write_json, read_json_file, verify_manifest_signature, TrustProvider, TrustStore,
    TRUST_STORE_FILENAME,
};

pub const REGISTRY_VERSION: u64 = 1;
pub const REGISTRY_DIRNAME: &str = "plugins";
pub const REGISTRY_FILENAME: &str = "registry.json";
pub const LEGACY_REGISTRY_RELATIVE: &str = "bin/plugins.json";
pub const MAX_REGISTRY_ENTRIES: usize = 256;

const RECORD_KNOWN_KEYS: [&str; 11] = [
    "id",
    "path",
    "enabled",
    "registeredAt",
    "updatedAt",
    "entrySha256",
    "entry_sha256",
    "entryDigest",
    "entry_digest",
    "digest",
    "sha256",
];

pub type NowFn = dyn Fn() -> i64 + Send + Sync;

/// Descritor do registro (mesmo shape de `registry.get`/`listDetailed`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDescriptor {
    pub id: String,
    pub manifest: Option<PluginManifest>,
    pub installed: bool,
    pub enabled: bool,
    pub valid: bool,
    pub error: String,
    pub source: String,
}

/// Shape histórico consumido por renderers antigos (`plugins:list`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginListItem {
    pub id: String,
    pub name: String,
    pub desc_key: String,
    pub installed: bool,
    pub enabled: bool,
}

/// Shape detalhado da fachada (`plugins:details`/`plugins:get`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDetailedItem {
    pub id: String,
    pub name: String,
    pub desc_key: String,
    pub installed: bool,
    pub enabled: bool,
    pub manifest: Option<PluginManifest>,
    pub valid: bool,
    pub error: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRecord {
    pub id: String,
    pub path: String,
    pub enabled: bool,
    pub registered_at: i64,
    pub updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_sha256: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryState {
    pub version: u64,
    pub plugins: Vec<PluginRecord>,
    pub extra: Map<String, Value>,
}

impl RegistryState {
    pub fn empty() -> Self {
        Self {
            version: REGISTRY_VERSION,
            plugins: Vec::new(),
            extra: Map::new(),
        }
    }

    pub fn get(&self, id: &str) -> Option<&PluginRecord> {
        self.plugins.iter().find(|record| record.id == id)
    }

    fn get_mut(&mut self, id: &str) -> Option<&mut PluginRecord> {
        self.plugins.iter_mut().find(|record| record.id == id)
    }

    fn insert(&mut self, record: PluginRecord) {
        if let Some(existing) = self.get_mut(&record.id) {
            *existing = record;
        } else {
            self.plugins.push(record);
        }
    }
}

/// Built-in é uma integração do host representada apenas por flags; `path` é
/// informativo e nunca é lido/executado pelo registro.
#[derive(Debug, Clone)]
pub struct BuiltinDefinition {
    pub id: String,
    pub manifest: PluginManifest,
    pub installed: bool,
    /// Chave de localização usada pelo shape histórico da UI.
    pub name_key: Option<String>,
    pub desc_key: String,
    pub path: String,
}

impl BuiltinDefinition {
    pub fn new(manifest: PluginManifest, installed: bool) -> Self {
        Self {
            id: manifest.id.clone(),
            manifest,
            installed,
            name_key: None,
            desc_key: String::new(),
            path: String::new(),
        }
    }

    pub fn with_display(mut self, name_key: Option<&str>, desc_key: &str, path: &str) -> Self {
        self.name_key = name_key.map(str::to_string);
        self.desc_key = desc_key.to_string();
        self.path = path.to_string();
        self
    }

    /// Constrói um built-in a partir de um objeto no formato aceito pelo
    /// `builtinManifest` do cliente Electron.
    pub fn from_value(definition: &Value, installed: bool) -> Option<Self> {
        let Value::Object(definition_map) = definition else {
            return None;
        };
        let source = match definition_map.get("manifest") {
            Some(Value::Object(_)) => definition_map.get("manifest").unwrap(),
            _ => definition,
        };
        let Value::Object(source) = source else {
            return None;
        };
        fn string(value: Option<&Value>) -> Option<&str> {
            value.and_then(Value::as_str)
        }
        /// `a ?? b ?? c`: somente null/undefined são ignorados.
        fn coalesce<'a>(values: impl IntoIterator<Item = Option<&'a Value>>) -> Option<&'a Value> {
            values.into_iter().flatten().find(|value| !value.is_null())
        }
        /// `value || fallback` do JS para os tipos JSON.
        fn js_truthy(value: &Value) -> bool {
            match value {
                Value::Null => false,
                Value::Bool(value) => *value,
                Value::Number(number) => number
                    .as_f64()
                    .is_some_and(|value| value != 0.0 && !value.is_nan()),
                Value::String(value) => !value.is_empty(),
                Value::Array(_) | Value::Object(_) => true,
            }
        }

        let id_value = coalesce([source.get("id"), definition_map.get("id")])
            .cloned()
            .unwrap_or(Value::Null);
        let name_value = coalesce([
            source.get("name"),
            definition_map.get("name"),
            definition_map.get("id"),
        ])
        .cloned()
        .unwrap_or(Value::Null);

        let mut input = Map::new();
        input.insert(
            "manifestVersion".to_string(),
            coalesce([source.get("manifestVersion")])
                .cloned()
                .unwrap_or_else(|| Value::Number(MANIFEST_VERSION.into())),
        );
        input.insert(
            "apiVersion".to_string(),
            coalesce([source.get("apiVersion")])
                .cloned()
                .unwrap_or_else(|| Value::Number(SDK_API_VERSION.into())),
        );
        input.insert("id".to_string(), id_value);
        input.insert("name".to_string(), name_value);
        input.insert(
            "version".to_string(),
            coalesce([source.get("version")])
                .cloned()
                .unwrap_or_else(|| Value::String("0.0.0".to_string())),
        );
        input.insert(
            "description".to_string(),
            coalesce([source.get("description")])
                .cloned()
                .unwrap_or_else(|| Value::String(String::new())),
        );
        if let Some(entry) = source.get("entry") {
            input.insert("entry".to_string(), entry.clone());
        }
        if let Some(digest) = coalesce(ENTRY_DIGEST_ALIASES.iter().map(|key| source.get(*key))) {
            input.insert("entrySha256".to_string(), digest.clone());
        }
        input.insert(
            "permissions".to_string(),
            match coalesce([source.get("permissions")]) {
                Some(value) if js_truthy(value) => value.clone(),
                _ => Value::Array(Vec::new()),
            },
        );

        let result = validate_manifest_with(
            &Value::Object(input),
            ValidateManifestOptions::without_entry(),
        );
        let manifest = result.manifest?;
        Some(Self {
            id: manifest.id.clone(),
            manifest,
            installed,
            name_key: string(definition_map.get("name"))
                .filter(|name| !name.is_empty())
                .map(str::to_string),
            desc_key: string(definition_map.get("descKey"))
                .unwrap_or_default()
                .to_string(),
            path: string(definition_map.get("path"))
                .unwrap_or_default()
                .to_string(),
        })
    }

    /// Built-ins históricos do Arcadia (SLSsteam e LuaTools), como flags.
    pub fn standard(slssteam_installed: bool, luatools_installed: bool) -> Vec<Self> {
        let slssteam = PluginManifest {
            manifest_version: MANIFEST_VERSION,
            api_version: SDK_API_VERSION,
            id: "slssteam".to_string(),
            name: "SLSsteam".to_string(),
            version: "0.0.0".to_string(),
            description: "Integração opcional com o cliente Steam.".to_string(),
            entry: String::new(),
            permissions: [
                "library:read",
                "library:write",
                "games:launch",
                "filesystem:read",
                "process:spawn",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            entry_sha256: None,
            signing_key_id: None,
            signature: None,
        };
        let luatools = PluginManifest {
            manifest_version: MANIFEST_VERSION,
            api_version: SDK_API_VERSION,
            id: "luatools-fixes".to_string(),
            name: "LuaTools fixes".to_string(),
            version: "0.0.0".to_string(),
            description: "Correções opcionais de integração LuaTools.".to_string(),
            entry: String::new(),
            permissions: ["library:read", "library:write"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            entry_sha256: None,
            signing_key_id: None,
            signature: None,
        };
        vec![
            Self::new(slssteam, slssteam_installed).with_display(
                Some("plugins.slssteam_nome"),
                "plugins.slssteam_desc",
                "",
            ),
            Self::new(luatools, luatools_installed).with_display(
                Some("plugins.luatools_nome"),
                "plugins.luatools_desc",
                "",
            ),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryError {
    DataDirRequired,
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryError::DataDirRequired => write!(formatter, "dataDir é obrigatório"),
        }
    }
}

impl std::error::Error for RegistryError {}

pub struct RegistryOptions {
    pub data_dir: PathBuf,
    pub builtins: Vec<BuiltinDefinition>,
    pub legacy_registry_path: Option<PathBuf>,
    pub trust_store_path: Option<PathBuf>,
    pub trust_provider: Option<Arc<dyn TrustProvider>>,
    pub now: Option<Arc<NowFn>>,
}

impl RegistryOptions {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            builtins: Vec::new(),
            legacy_registry_path: None,
            trust_store_path: None,
            trust_provider: None,
            now: None,
        }
    }

    pub fn builtins(mut self, builtins: Vec<BuiltinDefinition>) -> Self {
        self.builtins = builtins;
        self
    }

    pub fn now(mut self, now: Arc<NowFn>) -> Self {
        self.now = Some(now);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryPaths {
    pub root: String,
    pub registry_path: String,
    pub legacy_path: String,
    pub trust_store_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
}

impl MutationResult {
    pub fn success() -> Self {
        Self {
            ok: true,
            error: String::new(),
        }
    }

    pub fn failure(error: &str) -> Self {
        Self {
            ok: false,
            error: error.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<PluginDescriptor>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

impl RegisterResult {
    fn success(plugin: Option<PluginDescriptor>) -> Self {
        Self {
            ok: true,
            plugin,
            error: String::new(),
            errors: Vec::new(),
        }
    }

    fn failure(error: &str) -> Self {
        Self {
            ok: false,
            plugin: None,
            error: error.to_string(),
            errors: Vec::new(),
        }
    }

    fn failures(errors: Vec<String>) -> Self {
        let error = errors
            .first()
            .cloned()
            .unwrap_or_else(|| "manifest_invalido".to_string());
        Self {
            ok: false,
            plugin: None,
            error,
            errors,
        }
    }
}

/// Resultado de verificação; nenhum caminho privado é exposto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationResult {
    pub id: String,
    pub ok: bool,
    pub valid: bool,
    pub verified: bool,
    pub declared: bool,
    pub algorithm: String,
    pub expected_digest: String,
    pub actual_digest: String,
    pub digest: String,
    pub source: String,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signing_key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_algorithm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_declared: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_verified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_valid: Option<bool>,
}

/// Informação privada de leitura de pacote (equivalente a `readPackage`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInfo {
    pub ok: bool,
    pub manifest: Option<PluginManifest>,
    pub path: String,
    pub entry: String,
    pub entry_sha256: String,
    pub expected_entry_sha256: String,
    pub signature_declared: bool,
    pub signature_valid: bool,
    pub signature_error: String,
    pub errors: Vec<String>,
}

impl PackageInfo {
    fn failed(errors: impl IntoIterator<Item = String>) -> Self {
        Self {
            ok: false,
            manifest: None,
            path: String::new(),
            entry: String::new(),
            entry_sha256: String::new(),
            expected_entry_sha256: String::new(),
            signature_declared: false,
            signature_valid: false,
            signature_error: String::new(),
            errors: errors.into_iter().collect(),
        }
    }
}

#[derive(Debug, Clone)]
struct BuiltinState {
    manifest: PluginManifest,
    installed: bool,
    name_key: Option<String>,
    desc_key: String,
    #[allow(dead_code)]
    path: String,
}

pub struct PluginRegistry {
    root: PathBuf,
    registry_path: PathBuf,
    legacy_path: PathBuf,
    trust_store_path: PathBuf,
    trust: Arc<dyn TrustProvider>,
    builtins: Vec<BuiltinState>,
    builtin_index: BTreeMap<String, usize>,
    now: Arc<NowFn>,
    state: RefCell<Option<RegistryState>>,
}

impl PluginRegistry {
    pub fn new(data_dir: impl Into<PathBuf>) -> Result<Self, RegistryError> {
        Self::with_options(RegistryOptions::new(data_dir))
    }

    /// Conveniência para o host que quer os dois built-ins históricos como
    /// flags detectadas fora do crate (SLSsteam e LuaTools nunca são lidos).
    pub fn with_standard_builtins(
        data_dir: impl Into<PathBuf>,
        slssteam_installed: bool,
        luatools_installed: bool,
    ) -> Result<Self, RegistryError> {
        Self::with_options(
            RegistryOptions::new(data_dir)
                .builtins(standard_builtins(slssteam_installed, luatools_installed)),
        )
    }

    pub fn with_options(options: RegistryOptions) -> Result<Self, RegistryError> {
        let data_dir_display = options.data_dir.to_string_lossy();
        if data_dir_display.trim().is_empty() {
            return Err(RegistryError::DataDirRequired);
        }
        let root = resolve_path(&options.data_dir);
        let registry_dir = root.join(REGISTRY_DIRNAME);
        let registry_path = registry_dir.join(REGISTRY_FILENAME);
        let legacy_path = options
            .legacy_registry_path
            .map(|path| resolve_path(&path))
            .unwrap_or_else(|| root.join("bin").join("plugins.json"));
        let trust_store_path = options
            .trust_store_path
            .map(|path| resolve_path(&path))
            .unwrap_or_else(|| registry_dir.join(TRUST_STORE_FILENAME));
        let trust: Arc<dyn TrustProvider> = options
            .trust_provider
            .unwrap_or_else(|| Arc::new(TrustStore::new(trust_store_path.clone())));
        let now: Arc<NowFn> = options.now.unwrap_or_else(|| Arc::new(current_millis));

        let mut builtins = Vec::new();
        let mut builtin_index = BTreeMap::new();
        for definition in options.builtins {
            let value = serde_json::to_value(&definition.manifest).unwrap_or(Value::Null);
            let result = validate_manifest_with(&value, ValidateManifestOptions::without_entry());
            let Some(manifest) = result.manifest else {
                continue;
            };
            if builtin_index.contains_key(&manifest.id) {
                continue;
            }
            builtin_index.insert(manifest.id.clone(), builtins.len());
            builtins.push(BuiltinState {
                manifest,
                installed: definition.installed,
                name_key: definition.name_key,
                desc_key: definition.desc_key,
                path: definition.path,
            });
        }

        Ok(Self {
            root,
            registry_path,
            legacy_path,
            trust_store_path,
            trust,
            builtins,
            builtin_index,
            now,
            state: RefCell::new(None),
        })
    }

    fn timestamp(&self) -> i64 {
        let value = (self.now)();
        if value >= 0 {
            value
        } else {
            current_millis()
        }
    }

    fn ensure_state(&self) {
        if self.state.borrow().is_some() {
            return;
        }
        let canonical_value = read_json_file(&self.registry_path);
        let mut canonical = match canonical_value.as_ref() {
            Some(value) => parse_state(value),
            None => RegistryState::empty(),
        };
        let legacy = read_json_file(&self.legacy_path)
            .map(|value| parse_legacy_state(&value))
            .unwrap_or_default();
        for record in legacy {
            if let Some(existing) = canonical.get_mut(&record.id) {
                if record.updated_at > existing.updated_at {
                    existing.enabled = record.enabled;
                    existing.updated_at = record.updated_at;
                }
            } else if self.builtin_index.contains_key(&record.id) {
                canonical.insert(record);
            }
        }
        *self.state.borrow_mut() = Some(canonical);
    }

    pub fn read_state(&self) -> RegistryState {
        self.ensure_state();
        self.state
            .borrow()
            .as_ref()
            .cloned()
            .unwrap_or_else(RegistryState::empty)
    }

    pub fn invalidate(&self) {
        *self.state.borrow_mut() = None;
    }

    fn write_state(&self) -> Result<(), String> {
        let state = self.read_state();
        let mut plugins = Map::new();
        for record in &state.plugins {
            plugins.insert(
                record.id.clone(),
                serde_json::to_value(record).unwrap_or(Value::Null),
            );
        }
        let mut root = Map::new();
        root.insert(
            "version".to_string(),
            Value::Number(REGISTRY_VERSION.into()),
        );
        root.insert("plugins".to_string(), Value::Object(plugins));
        for (key, value) in &state.extra {
            root.insert(key.clone(), value.clone());
        }
        atomic_write_json(
            &self.registry_path,
            &Value::Object(root),
            "diretorio_de_registro_invalido",
        )?;

        // Espelho legado: apenas flags, sem paths/manifests. Uma falha nele não
        // invalida o estado canônico.
        let mut legacy = Map::new();
        for record in &state.plugins {
            let mut item = Map::new();
            item.insert("enabled".to_string(), Value::Bool(record.enabled));
            item.insert(
                "updatedAt".to_string(),
                Value::Number(record.updated_at.into()),
            );
            legacy.insert(record.id.clone(), Value::Object(item));
        }
        let _ = atomic_write_json(
            &self.legacy_path,
            &Value::Object(legacy),
            "diretorio_de_registro_invalido",
        );
        Ok(())
    }

    pub fn read_package(&self, plugin_path: impl AsRef<Path>) -> PackageInfo {
        let plugin_path = plugin_path.as_ref();
        let display = plugin_path.to_string_lossy();
        if display.trim().is_empty() || display.contains('\0') {
            return PackageInfo::failed(["caminho_invalido".to_string()]);
        }
        let mut resolved = resolve_path(plugin_path);
        let metadata = match std::fs::symlink_metadata(&resolved) {
            Ok(metadata) => metadata,
            Err(_) => {
                let mut info = PackageInfo::failed(["diretorio_nao_encontrado".to_string()]);
                info.path = path_string(&resolved);
                return info;
            }
        };
        if metadata.is_file()
            && resolved.file_name().and_then(|name| name.to_str()) == Some(MANIFEST_FILE)
        {
            resolved = resolved.parent().unwrap_or(Path::new("")).to_path_buf();
        } else if !metadata.is_dir() || metadata.file_type().is_symlink() {
            let mut info = PackageInfo::failed(["diretorio_invalido".to_string()]);
            info.path = path_string(&resolved);
            return info;
        }
        let root_metadata = match std::fs::symlink_metadata(&resolved) {
            Ok(metadata) => metadata,
            Err(_) => {
                let mut info = PackageInfo::failed(["diretorio_nao_encontrado".to_string()]);
                info.path = path_string(&resolved);
                return info;
            }
        };
        if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
            let mut info = PackageInfo::failed(["diretorio_invalido".to_string()]);
            info.path = path_string(&resolved);
            return info;
        }

        let parsed = read_manifest(&resolved.join(MANIFEST_FILE));
        let Some(manifest) = parsed.manifest else {
            let mut info = PackageInfo::failed(parsed.errors);
            info.path = path_string(&resolved);
            return info;
        };
        let mut info = PackageInfo {
            ok: false,
            manifest: Some(manifest.clone()),
            path: path_string(&resolved),
            entry: String::new(),
            entry_sha256: String::new(),
            expected_entry_sha256: manifest.entry_sha256.clone().unwrap_or_default(),
            signature_declared: manifest.is_signed(),
            signature_valid: false,
            signature_error: String::new(),
            errors: Vec::new(),
        };

        let checked_entry = resolve_plugin_entry(&resolved, &manifest.entry);
        let Some(entry_path) = checked_entry.path else {
            info.errors = vec![checked_entry.error];
            return info;
        };
        info.entry = path_string(&entry_path);

        let calculated = compute_entry_sha256(&entry_path, Some(&resolved));
        if !calculated.ok {
            info.errors = vec![calculated.error];
            return info;
        }
        info.entry_sha256 = calculated.digest.clone();

        let expected = manifest.entry_sha256.clone().unwrap_or_default();
        if !expected.is_empty() && !digest_matches(&expected, &calculated.digest) {
            info.errors = vec!["entry_digest_mismatch".to_string()];
            return info;
        }

        if info.signature_declared {
            let key_id = manifest.signing_key_id.as_deref().unwrap_or_default();
            match self.trust.get_public_key(key_id) {
                None => {
                    info.signature_error = "assinatura_chave_nao_confiavel".to_string();
                }
                Some(public_key) => {
                    if verify_manifest_signature(&manifest, &calculated.digest, &public_key) {
                        info.signature_valid = true;
                    } else {
                        info.signature_error = "assinatura_invalida".to_string();
                    }
                }
            }
            if !info.signature_valid {
                info.errors = vec![info.signature_error.clone()];
                return info;
            }
        }

        info.ok = true;
        info
    }

    fn record_for(&self, id: &str) -> Option<PluginRecord> {
        let normalized = normalize_id(id);
        if normalized.is_empty() {
            return None;
        }
        self.read_state().get(&normalized).cloned()
    }

    fn package_descriptor(&self, id: &str, record: &PluginRecord) -> Option<PluginDescriptor> {
        let normalized = normalize_id(id);
        if normalized.is_empty() || record.path.is_empty() {
            return None;
        }
        let package_info = self.read_package(Path::new(&record.path));
        if !package_info.ok {
            return Some(PluginDescriptor {
                id: normalized,
                manifest: None,
                installed: false,
                enabled: false,
                valid: false,
                error: package_info
                    .errors
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "manifest_invalido".to_string()),
                source: "local".to_string(),
            });
        }
        let Some(manifest) = package_info.manifest.as_ref() else {
            return Some(PluginDescriptor {
                id: normalized,
                manifest: None,
                installed: false,
                enabled: false,
                valid: false,
                error: "manifest_invalido".to_string(),
                source: "local".to_string(),
            });
        };
        if manifest.id != normalized {
            return Some(PluginDescriptor {
                id: normalized,
                manifest: None,
                installed: false,
                enabled: false,
                valid: false,
                error: "id_divergente".to_string(),
                source: "local".to_string(),
            });
        }
        if let Some(snapshot) = record.entry_sha256.as_deref() {
            if !digest_matches(snapshot, &package_info.entry_sha256) {
                return Some(PluginDescriptor {
                    id: normalized,
                    manifest: Some(public_manifest(manifest)),
                    installed: false,
                    enabled: false,
                    valid: false,
                    error: "entry_digest_mismatch".to_string(),
                    source: "local".to_string(),
                });
            }
        }
        Some(PluginDescriptor {
            id: normalized,
            manifest: Some(public_manifest(manifest)),
            installed: true,
            enabled: record.enabled,
            valid: true,
            error: String::new(),
            source: "local".to_string(),
        })
    }

    fn builtin_descriptor(&self, id: &str) -> Option<PluginDescriptor> {
        let index = *self.builtin_index.get(id)?;
        let definition = &self.builtins[index];
        let record = self.record_for(id);
        let enabled = definition.installed && record.as_ref().is_none_or(|record| record.enabled);
        Some(PluginDescriptor {
            id: id.to_string(),
            manifest: Some(public_manifest(&definition.manifest)),
            installed: definition.installed,
            enabled,
            valid: true,
            error: String::new(),
            source: "builtin".to_string(),
        })
    }

    pub fn get(&self, id: &str) -> Option<PluginDescriptor> {
        let normalized = normalize_id(id);
        if normalized.is_empty() {
            return None;
        }
        if self.builtin_index.contains_key(&normalized) {
            return self.builtin_descriptor(&normalized);
        }
        let record = self.record_for(&normalized)?;
        self.package_descriptor(&normalized, &record)
    }

    pub fn list_detailed(&self) -> Vec<PluginDescriptor> {
        let mut output = Vec::new();
        for index in 0..self.builtins.len() {
            let id = self.builtins[index].manifest.id.clone();
            if let Some(descriptor) = self.builtin_descriptor(&id) {
                output.push(descriptor);
            }
        }
        for record in self.read_state().plugins {
            if self.builtin_index.contains_key(&record.id) {
                continue;
            }
            if let Some(descriptor) = self.package_descriptor(&record.id, &record) {
                output.push(descriptor);
            }
        }
        output
    }

    fn display_fields(&self, descriptor: &PluginDescriptor) -> (String, String) {
        if let Some(index) = self.builtin_index.get(&descriptor.id) {
            let definition = &self.builtins[*index];
            let name = definition
                .name_key
                .clone()
                .filter(|name| !name.is_empty())
                .or_else(|| {
                    descriptor
                        .manifest
                        .as_ref()
                        .map(|manifest| manifest.name.clone())
                })
                .unwrap_or_else(|| descriptor.id.clone());
            return (name, definition.desc_key.clone());
        }
        (
            descriptor
                .manifest
                .as_ref()
                .map(|manifest| manifest.name.clone())
                .unwrap_or_else(|| descriptor.id.clone()),
            String::new(),
        )
    }

    /// Shape histórico de `plugins:list`.
    pub fn list(&self) -> Vec<PluginListItem> {
        self.list_detailed()
            .into_iter()
            .map(|descriptor| {
                let (name, desc_key) = self.display_fields(&descriptor);
                PluginListItem {
                    id: descriptor.id,
                    name,
                    desc_key,
                    installed: descriptor.installed,
                    enabled: descriptor.enabled,
                }
            })
            .collect()
    }

    /// Shape detalhado de `plugins:details`/`plugins:get`.
    pub fn detailed_items(&self) -> Vec<PluginDetailedItem> {
        self.list_detailed()
            .into_iter()
            .map(|descriptor| self.detailed_item(descriptor))
            .collect()
    }

    pub fn get_detailed(&self, id: &str) -> Option<PluginDetailedItem> {
        self.get(id)
            .map(|descriptor| self.detailed_item(descriptor))
    }

    fn detailed_item(&self, descriptor: PluginDescriptor) -> PluginDetailedItem {
        let (name, desc_key) = self.display_fields(&descriptor);
        PluginDetailedItem {
            id: descriptor.id,
            name,
            desc_key,
            installed: descriptor.installed,
            enabled: descriptor.enabled,
            manifest: descriptor.manifest,
            valid: descriptor.valid,
            error: descriptor.error,
            source: descriptor.source,
        }
    }

    pub fn register(&self, plugin_path: impl AsRef<Path>) -> RegisterResult {
        self.register_with_enabled(plugin_path, false)
    }

    pub fn register_with_enabled(
        &self,
        plugin_path: impl AsRef<Path>,
        enabled: bool,
    ) -> RegisterResult {
        let package_info = self.read_package(plugin_path.as_ref());
        if !package_info.ok {
            return RegisterResult::failures(package_info.errors);
        }
        let Some(manifest) = package_info.manifest.as_ref() else {
            return RegisterResult::failure("manifest_invalido");
        };
        let id = manifest.id.clone();
        if self.builtin_index.contains_key(&id) {
            return RegisterResult::failure("id_reservado");
        }
        let existing = self.record_for(&id);
        if let Some(existing) = existing.as_ref() {
            if existing.path != package_info.path {
                return RegisterResult::failure("id_ja_registrado");
            }
        }
        let timestamp = self.timestamp();
        let record = PluginRecord {
            id: id.clone(),
            path: package_info.path.clone(),
            enabled,
            registered_at: existing
                .as_ref()
                .map(|record| record.registered_at)
                .unwrap_or(timestamp),
            updated_at: timestamp,
            entry_sha256: Some(package_info.entry_sha256.clone()),
            // Campos desconhecidos de um registro existente sobrevivem a um
            // re-registro do mesmo pacote.
            extra: existing
                .as_ref()
                .map(|record| record.extra.clone())
                .unwrap_or_default(),
        };
        {
            self.ensure_state();
            let mut cache = self.state.borrow_mut();
            if let Some(state) = cache.as_mut() {
                state.insert(record);
            }
        }
        if self.write_state().is_err() {
            self.invalidate();
            return RegisterResult::failure("registro_nao_gravavel");
        }
        self.invalidate();
        let plugin = self.get(&id);
        RegisterResult::success(plugin)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> MutationResult {
        self.set_enabled_with(id, enabled, false)
    }

    pub fn set_enabled_with(
        &self,
        id: &str,
        enabled: bool,
        allow_undetected: bool,
    ) -> MutationResult {
        let normalized = normalize_id(id);
        if normalized.is_empty() {
            return MutationResult::failure("plugin_invalido");
        }
        if let Some(index) = self.builtin_index.get(&normalized).copied() {
            let definition = &self.builtins[index];
            if !allow_undetected && !definition.installed {
                return MutationResult::failure("not_detected");
            }
        }
        let builtin = self.builtin_index.contains_key(&normalized);
        let record = self.record_for(&normalized);
        if !builtin && record.is_none() {
            return MutationResult::failure("plugin_desconhecido");
        }
        if let Some(record) = record {
            if !builtin {
                let Some(item) = self.package_descriptor(&normalized, &record) else {
                    return MutationResult::failure("manifest_invalido");
                };
                if !item.valid || !item.installed {
                    return MutationResult::failure(if item.error.is_empty() {
                        "manifest_invalido"
                    } else {
                        &item.error
                    });
                }
            }
            let timestamp = self.timestamp();
            self.ensure_state();
            let mut cache = self.state.borrow_mut();
            if let Some(state) = cache.as_mut() {
                if let Some(stored) = state.get_mut(&normalized) {
                    stored.enabled = enabled;
                    stored.updated_at = timestamp;
                }
            }
        } else {
            let timestamp = self.timestamp();
            let record = PluginRecord {
                id: normalized.clone(),
                path: String::new(),
                enabled,
                registered_at: timestamp,
                updated_at: timestamp,
                entry_sha256: None,
                extra: Map::new(),
            };
            self.ensure_state();
            let mut cache = self.state.borrow_mut();
            if let Some(state) = cache.as_mut() {
                state.insert(record);
            }
        }
        if self.write_state().is_err() {
            self.invalidate();
            return MutationResult::failure("registro_nao_gravavel");
        }
        self.invalidate();
        MutationResult::success()
    }

    pub fn enable(&self, id: &str) -> MutationResult {
        self.set_enabled(id, true)
    }

    /// Atualiza apenas a disponibilidade detectada pelo host; preserva a opção salva.
    pub fn set_builtin_installed(&mut self, id: &str, installed: bool) {
        if let Some(index) = self.builtin_index.get(id).copied() {
            self.builtins[index].installed = installed;
        }
    }

    pub fn disable(&self, id: &str) -> MutationResult {
        self.set_enabled(id, false)
    }

    pub fn unregister(&self, id: &str) -> MutationResult {
        let normalized = normalize_id(id);
        if normalized.is_empty() {
            return MutationResult::failure("plugin_invalido");
        }
        if self.builtin_index.contains_key(&normalized) {
            return MutationResult::failure("plugin_embutido");
        }
        self.ensure_state();
        {
            let mut cache = self.state.borrow_mut();
            let Some(state) = cache.as_mut() else {
                return MutationResult::failure("plugin_desconhecido");
            };
            let Some(index) = state
                .plugins
                .iter()
                .position(|record| record.id == normalized)
            else {
                return MutationResult::failure("plugin_desconhecido");
            };
            state.plugins.remove(index);
        }
        if self.write_state().is_err() {
            self.invalidate();
            return MutationResult::failure("registro_nao_gravavel");
        }
        self.invalidate();
        MutationResult::success()
    }

    pub fn verify(&self, id: &str) -> VerificationResult {
        let normalized = normalize_id(id);
        if normalized.is_empty() {
            return verification_result(
                "",
                &PackageInfo::failed(["plugin_invalido".to_string()]),
                None,
            );
        }
        if self.builtin_index.contains_key(&normalized) {
            return VerificationResult {
                id: normalized,
                ok: true,
                valid: true,
                verified: false,
                declared: false,
                algorithm: "sha256".to_string(),
                expected_digest: String::new(),
                actual_digest: String::new(),
                digest: String::new(),
                source: "builtin".to_string(),
                error: String::new(),
                signing_key_id: None,
                signature: None,
                signature_algorithm: None,
                signature_declared: None,
                signature_verified: None,
                signature_valid: None,
            };
        }
        let Some(record) = self.record_for(&normalized) else {
            return verification_result(
                &normalized,
                &PackageInfo::failed(["plugin_desconhecido".to_string()]),
                None,
            );
        };
        let package_info = self.read_package(Path::new(&record.path));
        verification_result(&normalized, &package_info, Some(&record))
    }

    pub fn verify_integrity(&self, id: &str) -> VerificationResult {
        self.verify(id)
    }

    pub fn verify_plugin(&self, id: &str) -> VerificationResult {
        self.verify(id)
    }

    pub fn verify_package(&self, plugin_path: impl AsRef<Path>) -> VerificationResult {
        let package_info = self.read_package(plugin_path);
        let id = package_info
            .manifest
            .as_ref()
            .map(|manifest| manifest.id.clone())
            .unwrap_or_default();
        verification_result(&id, &package_info, None)
    }

    pub fn has_permission(&self, id: &str, permission: &str) -> bool {
        let Some(item) = self.get(id) else {
            return false;
        };
        item.valid
            && item.enabled
            && item
                .manifest
                .as_ref()
                .is_some_and(|manifest| manifest.permissions.iter().any(|item| item == permission))
    }

    pub fn trust_provider(&self) -> &dyn TrustProvider {
        self.trust.as_ref()
    }

    /// Diagnóstico interno; nunca deve ser enviado por IPC.
    pub fn paths(&self) -> RegistryPaths {
        RegistryPaths {
            root: path_string(&self.root),
            registry_path: path_string(&self.registry_path),
            legacy_path: path_string(&self.legacy_path),
            trust_store_path: path_string(&self.trust_store_path),
        }
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn current_millis() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    i64::try_from(millis).unwrap_or(i64::MAX)
}

fn js_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Some(0.0);
            }
            if let Some(hex) = trimmed
                .strip_prefix("0x")
                .or_else(|| trimmed.strip_prefix("0X"))
            {
                return i64::from_str_radix(hex, 16).ok().map(|value| value as f64);
            }
            trimmed.parse::<f64>().ok()
        }
        Value::Bool(value) => Some(if *value { 1.0 } else { 0.0 }),
        Value::Null => Some(0.0),
        _ => None,
    }
}

pub fn as_timestamp(value: Option<&Value>, fallback: i64) -> i64 {
    let Some(value) = value else {
        return fallback;
    };
    let Some(number) = js_number(value) else {
        return fallback;
    };
    if !number.is_finite() || number < 0.0 {
        return fallback;
    }
    number.trunc() as i64
}

pub fn normalize_stored_digest(value: &str) -> String {
    let digest = value.trim().to_lowercase();
    if digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        digest
    } else {
        String::new()
    }
}

pub fn stored_entry_digest(raw: &Value) -> Option<String> {
    let Value::Object(map) = raw else {
        return None;
    };
    for key in ENTRY_DIGEST_ALIASES {
        let Some(value) = map.get(key).and_then(Value::as_str) else {
            continue;
        };
        let digest = normalize_stored_digest(value);
        if !digest.is_empty() {
            return Some(digest);
        }
    }
    None
}

pub fn digest_matches(expected: &str, actual: &str) -> bool {
    let left = normalize_stored_digest(expected);
    let right = normalize_stored_digest(actual);
    if left.is_empty() || right.is_empty() {
        return false;
    }
    let mut difference = 0_u8;
    for (left, right) in left.bytes().zip(right.bytes()) {
        difference |= left ^ right;
    }
    difference == 0
}

pub fn safe_state_record(id: &str, raw: &Value, fallback_path: &str) -> Option<PluginRecord> {
    let normalized = normalize_id(id);
    let Value::Object(raw) = raw else {
        return None;
    };
    if normalized.is_empty() {
        return None;
    }
    let plugin_path = match raw.get("path") {
        Some(Value::String(path)) => path.trim().to_string(),
        Some(_) => fallback_path.to_string(),
        None => fallback_path.to_string(),
    };
    if plugin_path.contains('\0') {
        return None;
    }
    let updated = as_timestamp(raw.get("updatedAt"), 0);
    let registered = as_timestamp(raw.get("registeredAt"), updated);
    let mut extra = Map::new();
    for (key, value) in raw {
        if !RECORD_KNOWN_KEYS.contains(&key.as_str()) {
            extra.insert(key.clone(), value.clone());
        }
    }
    Some(PluginRecord {
        id: normalized,
        path: if plugin_path.is_empty() {
            String::new()
        } else {
            path_string(&resolve_path(Path::new(&plugin_path)))
        },
        enabled: raw.get("enabled") == Some(&Value::Bool(true)),
        registered_at: registered,
        updated_at: updated,
        entry_sha256: stored_entry_digest(&Value::Object(raw.clone())),
        extra,
    })
}

fn json_is_one_number(value: &Value) -> bool {
    matches!(value, Value::Number(number) if number.as_f64() == Some(1.0))
}

pub fn parse_state(value: &Value) -> RegistryState {
    let Value::Object(root) = value else {
        return RegistryState::empty();
    };
    if !root.get("version").is_some_and(json_is_one_number) {
        return RegistryState::empty();
    }
    let Some(Value::Object(plugins)) = root.get("plugins") else {
        return RegistryState::empty();
    };
    let mut state = RegistryState::empty();
    for (id, raw) in plugins.iter().take(MAX_REGISTRY_ENTRIES) {
        if let Some(record) = safe_state_record(id, raw, "") {
            state.insert(record);
        }
    }
    for (key, value) in root {
        if key != "version" && key != "plugins" {
            state.extra.insert(key.clone(), value.clone());
        }
    }
    state
}

pub fn parse_legacy_state(value: &Value) -> Vec<PluginRecord> {
    let Value::Object(root) = value else {
        return Vec::new();
    };
    let mut records: Vec<PluginRecord> = Vec::new();
    for (id, raw) in root.iter().take(MAX_REGISTRY_ENTRIES) {
        let normalized = normalize_id(id);
        let Value::Object(raw) = raw else {
            continue;
        };
        if normalized.is_empty() {
            continue;
        }
        let updated = as_timestamp(raw.get("updatedAt"), 0);
        let record = PluginRecord {
            id: normalized.clone(),
            path: String::new(),
            enabled: raw.get("enabled") == Some(&Value::Bool(true)),
            registered_at: updated,
            updated_at: updated,
            entry_sha256: None,
            extra: Map::new(),
        };
        if let Some(existing) = records.iter_mut().find(|item| item.id == normalized) {
            *existing = record;
        } else {
            records.push(record);
        }
    }
    records
}

fn verification_result(
    id: &str,
    package_info: &PackageInfo,
    record: Option<&PluginRecord>,
) -> VerificationResult {
    let declared_digest = package_info
        .manifest
        .as_ref()
        .and_then(|manifest| manifest.entry_sha256.clone())
        .unwrap_or_default();
    let expected_digest = record
        .and_then(|record| record.entry_sha256.clone())
        .filter(|digest| !digest.is_empty())
        .unwrap_or_else(|| declared_digest.clone());
    let actual_digest = package_info.entry_sha256.clone();
    let verified = !expected_digest.is_empty();
    let digest_ok = !verified || digest_matches(&expected_digest, &actual_digest);
    let valid = package_info.ok && digest_ok;
    let error = if !package_info.ok {
        package_info
            .errors
            .first()
            .cloned()
            .unwrap_or_else(|| "entry_nao_verificavel".to_string())
    } else if verified && !digest_matches(&expected_digest, &actual_digest) {
        "entry_digest_mismatch".to_string()
    } else {
        String::new()
    };
    let source = if record
        .and_then(|record| record.entry_sha256.as_deref())
        .is_some_and(|digest| !digest.is_empty())
    {
        "registry"
    } else if !declared_digest.is_empty() {
        "manifest"
    } else {
        "none"
    };

    let mut result = VerificationResult {
        id: normalize_id(id),
        ok: valid,
        valid,
        verified,
        declared: !declared_digest.is_empty(),
        algorithm: "sha256".to_string(),
        expected_digest,
        actual_digest: actual_digest.clone(),
        digest: actual_digest,
        source: source.to_string(),
        error: error.clone(),
        signing_key_id: None,
        signature: None,
        signature_algorithm: None,
        signature_declared: None,
        signature_verified: None,
        signature_valid: None,
    };

    if let Some(manifest) = package_info.manifest.as_ref() {
        if let (Some(key_id), Some(signature)) = (
            manifest.signing_key_id.as_ref(),
            manifest.signature.as_ref(),
        ) {
            let signature_verified = package_info.signature_valid && package_info.ok;
            result.signing_key_id = Some(key_id.clone());
            result.signature = Some(signature.clone());
            result.signature_algorithm = Some("ed25519".to_string());
            result.signature_declared = Some(true);
            result.signature_verified = Some(signature_verified);
            result.signature_valid = Some(signature_verified);
            if !signature_verified && result.error.is_empty() {
                result.error = if package_info.signature_error.is_empty() {
                    "assinatura_invalida".to_string()
                } else {
                    package_info.signature_error.clone()
                };
            }
            if !signature_verified {
                result.ok = false;
                result.valid = false;
            }
        }
    }
    result
}

/// Helper de conveniência para hosts que querem os dois built-ins históricos.
pub fn standard_builtins(
    slssteam_installed: bool,
    luatools_installed: bool,
) -> Vec<BuiltinDefinition> {
    BuiltinDefinition::standard(slssteam_installed, luatools_installed)
}

/// Reexporta `is_inside` para hosts que diagnosticam pacotes sem duplicar a
/// regra de contenção.
pub fn package_is_inside(root: &Path, target: &Path) -> bool {
    is_inside(root, target)
}
