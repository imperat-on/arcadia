//! Chaves confiáveis e assinaturas Ed25519 de plugins locais — port 1:1 de
//! `app/electron/plugins/trust.js`.
//!
//! Só metadados são manipulados: o módulo nunca baixa, importa ou executa o
//! `entry` de um plugin. As chaves são persistidas como PEM SPKI (formato que o
//! cliente Electron grava) em `$ARCADIA_DATA_DIR/plugins/trusted-keys.json`,
//! com escrita atômica e modo 0600 em sistemas POSIX.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ed25519_dalek::{Signature, VerifyingKey};
use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::manifest::PluginManifest;

pub const TRUST_STORE_VERSION: u64 = 1;
pub const TRUST_STORE_FILENAME: &str = "trusted-keys.json";
/// Alias histórico exportado pelo cliente Electron.
pub const TRUST_STORE_FILE: &str = TRUST_STORE_FILENAME;
pub const MAX_TRUSTED_KEYS: usize = 256;
pub const MAX_KEY_ID_LENGTH: usize = 128;
pub const SIGNATURE_LENGTH: usize = 64;

/// Prefixo DER de uma chave pública Ed25519 em SPKI.
const ED25519_SPKI_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

static KEY_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z0-9](?:[A-Za-z0-9._:-]{0,126}[A-Za-z0-9])?$").expect("regex key id")
});
static BASE64_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$")
        .expect("regex base64")
});
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn normalize_key_id(value: &str) -> String {
    let id = value.trim();
    if id.is_empty() || id.len() > MAX_KEY_ID_LENGTH || id.contains('\0') || !KEY_ID_RE.is_match(id)
    {
        return String::new();
    }
    id.to_string()
}

fn is_standard_base64(value: &str) -> bool {
    !value.is_empty() && value.len().is_multiple_of(4) && BASE64_RE.is_match(value)
}

fn import_spki_der(bytes: &[u8]) -> Option<VerifyingKey> {
    if bytes.len() != ED25519_SPKI_PREFIX.len() + 32 || !bytes.starts_with(&ED25519_SPKI_PREFIX) {
        return None;
    }
    let mut key = [0_u8; 32];
    key.copy_from_slice(&bytes[ED25519_SPKI_PREFIX.len()..]);
    VerifyingKey::from_bytes(&key).ok()
}

/// Importa uma chave pública Ed25519 em PEM SPKI ou base64 do DER SPKI.
pub fn import_public_key(value: &str) -> Option<VerifyingKey> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains("BEGIN") {
        let mut body = String::new();
        let mut in_body = false;
        let mut found_begin = false;
        let mut found_end = false;
        for line in trimmed.lines() {
            let line = line.trim();
            if line == "-----BEGIN PUBLIC KEY-----" {
                in_body = true;
                found_begin = true;
                continue;
            }
            if line == "-----END PUBLIC KEY-----" {
                found_end = true;
                break;
            }
            if in_body {
                if line.contains("-----") {
                    return None;
                }
                body.push_str(line);
            }
        }
        if !found_begin || !found_end || body.is_empty() || !is_standard_base64(&body) {
            return None;
        }
        let der = STANDARD.decode(body).ok()?;
        return import_spki_der(&der);
    }
    if !is_standard_base64(trimmed) {
        return None;
    }
    let der = STANDARD.decode(trimmed).ok()?;
    import_spki_der(&der)
}

/// Exporta a chave como PEM SPKI canônico (mesmo formato do Node/Electron).
pub fn export_public_key(key: &VerifyingKey) -> String {
    let mut der = Vec::with_capacity(ED25519_SPKI_PREFIX.len() + 32);
    der.extend_from_slice(&ED25519_SPKI_PREFIX);
    der.extend_from_slice(key.as_bytes());
    let encoded = STANDARD.encode(der);
    format!("-----BEGIN PUBLIC KEY-----\n{encoded}\n-----END PUBLIC KEY-----\n")
}

/// Bytes exatos cobertos pela assinatura: `{id, version, entrySha256}` em JSON
/// compacto, na mesma ordem do `JSON.stringify` do cliente Electron.
pub fn canonical_payload(id: &str, version: &str, entry_sha256: &str) -> String {
    let mut payload = Map::new();
    payload.insert("id".to_string(), Value::String(id.to_string()));
    payload.insert("version".to_string(), Value::String(version.to_string()));
    payload.insert(
        "entrySha256".to_string(),
        Value::String(entry_sha256.to_string()),
    );
    Value::Object(payload).to_string()
}

/// `canonicalPayload(manifest)` do JS, incluindo o alias legado
/// `entry_digest` quando `entrySha256` não está presente.
pub fn canonical_payload_value(value: &Value) -> String {
    let Value::Object(map) = value else {
        return canonical_payload("", "", "");
    };
    let id = map.get("id").and_then(Value::as_str).unwrap_or_default();
    let version = map
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let digest = map
        .get("entrySha256")
        .and_then(Value::as_str)
        .or_else(|| map.get("entry_digest").and_then(Value::as_str))
        .unwrap_or_default();
    canonical_payload(id, version, digest)
}

pub fn canonical_payload_for_manifest(manifest: &PluginManifest) -> String {
    canonical_payload(
        &manifest.id,
        &manifest.version,
        manifest.entry_sha256.as_deref().unwrap_or_default(),
    )
}

/// Decodifica estritamente a assinatura base64 padrão de 64 bytes.
pub fn decode_signature(value: &str) -> Option<[u8; SIGNATURE_LENGTH]> {
    if value.is_empty() || !value.len().is_multiple_of(4) || !BASE64_RE.is_match(value) {
        return None;
    }
    let bytes = STANDARD.decode(value).ok()?;
    if bytes.len() != SIGNATURE_LENGTH || STANDARD.encode(&bytes) != value {
        return None;
    }
    let mut signature = [0_u8; SIGNATURE_LENGTH];
    signature.copy_from_slice(&bytes);
    Some(signature)
}

pub fn decode_signature_bytes(value: &[u8]) -> Option<[u8; SIGNATURE_LENGTH]> {
    if value.len() != SIGNATURE_LENGTH {
        return None;
    }
    let mut signature = [0_u8; SIGNATURE_LENGTH];
    signature.copy_from_slice(value);
    Some(signature)
}

pub fn looks_like_signature(value: &str) -> bool {
    decode_signature(value).is_some()
}

/// Verifica Ed25519 sobre uma string de payload.
pub fn verify_signature(payload: &str, signature: &str, public_key: &VerifyingKey) -> bool {
    let Some(bytes) = decode_signature(signature) else {
        return false;
    };
    verify_signature_bytes(payload.as_bytes(), &bytes, public_key)
}

pub fn verify_signature_bytes(
    payload: &[u8],
    signature: &[u8; SIGNATURE_LENGTH],
    public_key: &VerifyingKey,
) -> bool {
    let signature = Signature::from_bytes(signature);
    public_key.verify_strict(payload, &signature).is_ok()
}

/// Verifica a assinatura declarada no manifesto sobre o digest efetivamente
/// calculado pelo chamador (`calculated_entry_sha256`).
pub fn verify_manifest_signature(
    manifest: &PluginManifest,
    calculated_entry_sha256: &str,
    public_key: &VerifyingKey,
) -> bool {
    let Some(signature) = manifest.signature.as_deref() else {
        return false;
    };
    let payload = canonical_payload(&manifest.id, &manifest.version, calculated_entry_sha256);
    verify_signature(&payload, signature, public_key)
}

/// Entrada pública da lista de chaves; nunca inclui caminho do arquivo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedKey {
    pub key_id: String,
    pub public_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustStoreState {
    pub version: u64,
    pub keys: Vec<TrustedKey>,
    pub extra: Map<String, Value>,
}

impl TrustStoreState {
    pub fn empty() -> Self {
        Self {
            version: TRUST_STORE_VERSION,
            keys: Vec::new(),
            extra: Map::new(),
        }
    }

    pub fn get(&self, key_id: &str) -> Option<&str> {
        self.keys
            .iter()
            .find(|key| key.key_id == key_id)
            .map(|key| key.public_key.as_str())
    }
}

fn first_non_null<'a>(value: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .filter_map(|key| value.get(*key))
        .find(|value| !value.is_null())
}

fn public_key_input(value: &Value) -> Option<&str> {
    match value {
        Value::String(text) => Some(text.as_str()),
        Value::Object(map) => {
            first_non_null(map, &["publicKey", "key", "pem"]).and_then(Value::as_str)
        }
        _ => None,
    }
}

/// Lê o envelope `trusted-keys.json`, aceitando também as grafias
/// `trustedKeys`/`trusted_keys` e listas de `{keyId, publicKey}`.
pub fn parse_store(value: &Value) -> TrustStoreState {
    let Value::Object(root) = value else {
        return TrustStoreState::empty();
    };
    if let Some(version) = root.get("version") {
        let supported = match version {
            Value::Number(number) => number.as_f64() == Some(TRUST_STORE_VERSION as f64),
            Value::String(text) => text == &TRUST_STORE_VERSION.to_string(),
            _ => false,
        };
        if !supported {
            return TrustStoreState::empty();
        }
    }

    let mut state = TrustStoreState::empty();
    let source = first_non_null(root, &["keys", "trustedKeys", "trusted_keys"]);
    match source {
        Some(Value::Array(items)) => {
            for item in items.iter().take(MAX_TRUSTED_KEYS) {
                let Value::Object(item) = item else {
                    continue;
                };
                let Some(raw_id) = first_non_null(item, &["keyId", "id"]).and_then(Value::as_str)
                else {
                    continue;
                };
                let Some(public_key) =
                    first_non_null(item, &["publicKey", "key", "pem"]).and_then(public_key_input)
                else {
                    continue;
                };
                insert_key(&mut state, raw_id, public_key);
            }
        }
        Some(Value::Object(map)) => {
            for (raw_id, raw_value) in map.iter().take(MAX_TRUSTED_KEYS) {
                let Some(public_key) = public_key_input(raw_value) else {
                    continue;
                };
                insert_key(&mut state, raw_id, public_key);
            }
        }
        _ => {}
    }

    for (key, value) in root {
        if !matches!(
            key.as_str(),
            "version" | "keys" | "trustedKeys" | "trusted_keys"
        ) {
            state.extra.insert(key.clone(), value.clone());
        }
    }
    state
}

fn insert_key(state: &mut TrustStoreState, raw_id: &str, raw_public_key: &str) {
    let key_id = normalize_key_id(raw_id);
    let Some(public_key) = import_public_key(raw_public_key).map(|key| export_public_key(&key))
    else {
        return;
    };
    if key_id.is_empty() {
        return;
    }
    if let Some(existing) = state.keys.iter_mut().find(|key| key.key_id == key_id) {
        existing.public_key = public_key;
    } else if state.keys.len() < MAX_TRUSTED_KEYS {
        state.keys.push(TrustedKey { key_id, public_key });
    }
}

pub fn read_json_file(path: &Path) -> Option<Value> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Escrita atômica tmp + fsync + rename. `dir_error` mantém o código histórico
/// do chamador (`trust_store_diretorio_invalido` ou
/// `diretorio_de_registro_invalido`).
pub fn atomic_write_json(path: &Path, value: &Value, dir_error: &str) -> Result<(), String> {
    let Some(directory) = path.parent() else {
        return Err(dir_error.to_string());
    };
    #[cfg(unix)]
    let was_present = directory.exists();
    fs::create_dir_all(directory).map_err(|_| dir_error.to_string())?;
    #[cfg(unix)]
    if !was_present {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(directory, fs::Permissions::from_mode(0o700));
    }
    let metadata = fs::symlink_metadata(directory).map_err(|_| dir_error.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(dir_error.to_string());
    }

    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "trusted-keys".to_string());
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_file_name(format!(
        ".{file_name}.{}.{nanos}.{unique}.tmp",
        std::process::id()
    ));

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let result = (|| -> std::io::Result<()> {
        let mut file = options.open(&temporary)?;
        file.write_all(format!("{text}\n").as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| error.to_string())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn store_path_for_data_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("plugins").join(TRUST_STORE_FILENAME)
}

pub fn resolve_store_path(explicit: Option<&Path>, data_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(crate::manifest::resolve_path(path));
    }
    data_dir.map(store_path_for_data_dir)
}

/// Resultado de `add`/`remove` no mesmo envelope do JS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustOpResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
}

impl TrustOpResult {
    fn success(key_id: String) -> Self {
        Self {
            ok: true,
            key_id: Some(key_id),
            error: String::new(),
        }
    }

    fn failure(error: &str) -> Self {
        Self {
            ok: false,
            key_id: None,
            error: error.to_string(),
        }
    }
}

/// Abstração mínima usada pelo registro, permitindo injetar uma política de
/// confiança em testes/host sem tocar no arquivo.
pub trait TrustProvider: Send + Sync {
    fn get_public_key(&self, key_id: &str) -> Option<VerifyingKey>;
}

/// Store local de chaves confiáveis. Lê o arquivo a cada operação para que uma
/// alteração feita por outro processo seja observada sem reiniciar o host.
#[derive(Debug, Clone)]
pub struct TrustStore {
    store_path: PathBuf,
}

impl TrustStore {
    pub fn new(store_path: impl Into<PathBuf>) -> Self {
        Self {
            store_path: store_path.into(),
        }
    }

    pub fn for_data_dir(data_dir: impl AsRef<Path>) -> Self {
        Self::new(store_path_for_data_dir(data_dir.as_ref()))
    }

    pub fn store_path(&self) -> &Path {
        &self.store_path
    }

    pub fn read_state(&self) -> TrustStoreState {
        parse_store(&read_json_file(&self.store_path).unwrap_or(Value::Null))
    }

    fn write_state(&self, state: &TrustStoreState) -> Result<(), String> {
        let mut keys = Map::new();
        for key in &state.keys {
            keys.insert(key.key_id.clone(), Value::String(key.public_key.clone()));
        }
        let mut root = Map::new();
        root.insert(
            "version".to_string(),
            Value::Number(TRUST_STORE_VERSION.into()),
        );
        root.insert("keys".to_string(), Value::Object(keys));
        for (key, value) in &state.extra {
            root.insert(key.clone(), value.clone());
        }
        atomic_write_json(
            &self.store_path,
            &Value::Object(root),
            "trust_store_diretorio_invalido",
        )
    }

    pub fn add(&self, key_id: &str, public_key: &VerifyingKey) -> TrustOpResult {
        let normalized = normalize_key_id(key_id);
        if normalized.is_empty() {
            return TrustOpResult::failure("chave_invalida");
        }
        let mut state = self.read_state();
        let exists = state.keys.iter().any(|key| key.key_id == normalized);
        if !exists && state.keys.len() >= MAX_TRUSTED_KEYS {
            return TrustOpResult::failure("trust_store_cheio");
        }
        let pem = export_public_key(public_key);
        if let Some(existing) = state.keys.iter_mut().find(|key| key.key_id == normalized) {
            existing.public_key = pem;
        } else {
            state.keys.push(TrustedKey {
                key_id: normalized.clone(),
                public_key: pem,
            });
        }
        match self.write_state(&state) {
            Ok(()) => TrustOpResult::success(normalized),
            Err(_) => TrustOpResult::failure("trust_store_nao_gravavel"),
        }
    }

    /// Conveniência que aceita PEM SPKI ou base64 do DER SPKI.
    pub fn add_pem(&self, key_id: &str, public_key: &str) -> TrustOpResult {
        let Some(key) = import_public_key(public_key) else {
            return TrustOpResult::failure("chave_invalida");
        };
        self.add(key_id, &key)
    }

    pub fn remove(&self, key_id: &str) -> TrustOpResult {
        let normalized = normalize_key_id(key_id);
        if normalized.is_empty() {
            return TrustOpResult::failure("chave_invalida");
        }
        let mut state = self.read_state();
        let Some(index) = state.keys.iter().position(|key| key.key_id == normalized) else {
            return TrustOpResult::failure("chave_desconhecida");
        };
        state.keys.remove(index);
        match self.write_state(&state) {
            Ok(()) => TrustOpResult::success(normalized),
            Err(_) => TrustOpResult::failure("trust_store_nao_gravavel"),
        }
    }

    pub fn list(&self) -> Vec<TrustedKey> {
        self.read_state().keys
    }

    pub fn keys(&self) -> Vec<TrustedKey> {
        self.list()
    }

    pub fn get(&self, key_id: &str) -> Option<String> {
        let normalized = normalize_key_id(key_id);
        if normalized.is_empty() {
            return None;
        }
        self.read_state().get(&normalized).map(str::to_string)
    }

    pub fn get_public_key(&self, key_id: &str) -> Option<VerifyingKey> {
        self.get(key_id).and_then(|pem| import_public_key(&pem))
    }

    pub fn has(&self, key_id: &str) -> bool {
        self.get(key_id).is_some()
    }

    pub fn verify(&self, key_id: &str, payload: &str, signature: &str) -> bool {
        let Some(public_key) = self.get_public_key(key_id) else {
            return false;
        };
        verify_signature(payload, signature, &public_key)
    }

    pub fn verify_manifest(
        &self,
        key_id: &str,
        manifest: &PluginManifest,
        calculated_entry_sha256: &str,
    ) -> bool {
        let Some(public_key) = self.get_public_key(key_id) else {
            return false;
        };
        verify_manifest_signature(manifest, calculated_entry_sha256, &public_key)
    }

    /// Compatibilidade com o cliente Electron: não há cache persistente, mas a
    /// operação existe para hosts que a chamam.
    pub fn invalidate(&self) {}
}

impl TrustProvider for TrustStore {
    fn get_public_key(&self, key_id: &str) -> Option<VerifyingKey> {
        TrustStore::get_public_key(self, key_id)
    }
}
