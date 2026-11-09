//! Manifest v1 de plugins locais — port 1:1 de `app/electron/plugins/manifest.js`.
//!
//! A validação é puramente declarativa: nenhum `entry` é importado, avaliado ou
//! executado. O manifesto canônico usa camelCase; grafias snake_case legadas são
//! aceitas apenas na leitura. Campos desconhecidos são rejeitados para que o
//! manifesto continue sendo uma declaração de capacidades, e não uma forma de
//! configurar o processo principal.

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub const MANIFEST_VERSION: u64 = 1;
pub const SDK_API_VERSION: u64 = 1;
pub const MANIFEST_FILE: &str = "plugin.json";
pub const MAX_ID_LENGTH: usize = 64;
pub const MAX_NAME_LENGTH: usize = 120;
pub const MAX_DESCRIPTION_LENGTH: usize = 1000;
pub const MAX_VERSION_LENGTH: usize = 64;
pub const MAX_ENTRY_LENGTH: usize = 240;
pub const MAX_PERMISSIONS: usize = 32;
pub const ENTRY_DIGEST_LENGTH: usize = 64;
pub const SIGNATURE_LENGTH: usize = 64;
pub const SIGNING_KEY_ID_MAX_LENGTH: usize = 128;

/// Aliases de leitura para o digest do entry. O objeto canônico sempre expõe
/// somente `entrySha256` (e as versões anteriores, grafias alternativas).
pub const ENTRY_DIGEST_ALIASES: [&str; 6] = [
    "entrySha256",
    "entry_sha256",
    "entryDigest",
    "entry_digest",
    "digest",
    "sha256",
];

/// Allowlist v1 de capacidades. Não existem globais/wildcards: cada operação
/// futura precisa de um nome explícito e de uma checagem no SDK.
pub const PLUGIN_PERMISSIONS: [&str; 13] = [
    "library:read",
    "library:write",
    "games:read",
    "games:launch",
    "events:subscribe",
    "commands:register",
    "network",
    "filesystem:read",
    "filesystem:write",
    "process:spawn",
    "notifications",
    "settings:read",
    "settings:write",
];

static ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9](?:[a-z0-9._-]{0,62}[a-z0-9])?$").expect("regex de id"));
static SEMVER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$",
    )
    .expect("regex semver")
});
static ENTRY_DIGEST_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9a-fA-F]{64}$").expect("regex digest"));
static SIGNING_KEY_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z0-9](?:[A-Za-z0-9._:-]{0,126}[A-Za-z0-9])?$")
        .expect("regex signing key id")
});
static SIGNATURE_BASE64_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$")
        .expect("regex assinatura base64")
});

/// Manifesto validado e canônico. A ordem dos campos coincide com o objeto
/// devolvido pelo cliente Electron.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub manifest_version: u64,
    pub api_version: u64,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub entry: String,
    pub permissions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl PluginManifest {
    /// Cópia independente para a UI: sem caminho de pacote/registro.
    pub fn public(&self) -> Self {
        self.clone()
    }

    /// Alias de leitura histórico (`entryDigest`, `entry_digest`, ...).
    pub fn entry_digest(&self) -> Option<&str> {
        self.entry_sha256.as_deref()
    }

    /// Alias curto usado por chamadores que só querem o digest.
    pub fn digest(&self) -> Option<&str> {
        self.entry_sha256.as_deref()
    }

    pub fn is_signed(&self) -> bool {
        self.signing_key_id.is_some() && self.signature.is_some()
    }
}

/// Resultado de validação no mesmo formato do JS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestValidation {
    pub ok: bool,
    pub manifest: Option<PluginManifest>,
    pub errors: Vec<String>,
}

impl ManifestValidation {
    pub fn error(&self) -> Option<&str> {
        self.errors.first().map(String::as_str)
    }

    fn failure(errors: impl IntoIterator<Item = String>) -> Self {
        Self {
            ok: false,
            manifest: None,
            errors: dedupe(errors.into_iter().collect()),
        }
    }

    fn success(manifest: PluginManifest) -> Self {
        Self {
            ok: true,
            manifest: Some(manifest),
            errors: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdValidation {
    pub ok: bool,
    pub id: String,
    pub errors: Vec<String>,
}

/// Opções de validação. Built-ins não têm entry executável, por isso o
/// registro os valida com `require_entry = false`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidateManifestOptions {
    pub require_entry: bool,
}

impl Default for ValidateManifestOptions {
    fn default() -> Self {
        Self {
            require_entry: true,
        }
    }
}

impl ValidateManifestOptions {
    pub fn without_entry() -> Self {
        Self {
            require_entry: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryResolution {
    pub ok: bool,
    pub path: Option<PathBuf>,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryDigest {
    pub ok: bool,
    pub digest: String,
    pub error: String,
}

fn dedupe(errors: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    errors
        .into_iter()
        .filter(|error| seen.insert(error.clone()))
        .collect()
}

/// Comprimento no mesmo espírito de `String.prototype.length` (unidades
/// UTF-16), usado pelos limites do JS.
fn js_len(value: &str) -> usize {
    value.encode_utf16().count()
}

fn truncate_js(value: &str, max: usize) -> String {
    let units: Vec<u16> = value.encode_utf16().take(max).collect();
    String::from_utf16_lossy(&units)
}

fn first<'a>(value: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| value.get(*key))
}

pub fn normalize_id(value: &str) -> String {
    let id = value.trim();
    if id.is_empty() || id != id.to_lowercase() || js_len(id) > MAX_ID_LENGTH || !ID_RE.is_match(id)
    {
        return String::new();
    }
    id.to_string()
}

pub fn validate_id(value: &str) -> IdValidation {
    let id = normalize_id(value);
    if id.is_empty() {
        IdValidation {
            ok: false,
            id: String::new(),
            errors: vec!["id_invalido".to_string()],
        }
    } else {
        IdValidation {
            ok: true,
            id,
            errors: Vec::new(),
        }
    }
}

fn normalize_manifest_version(value: Option<&Value>) -> Option<u64> {
    match value {
        Some(Value::String(text)) if text == "1" => Some(MANIFEST_VERSION),
        Some(Value::Number(number)) if number.as_f64() == Some(1.0) => Some(MANIFEST_VERSION),
        _ => None,
    }
}

fn normalize_api_version(value: Option<&Value>) -> Option<u64> {
    match value {
        None => Some(SDK_API_VERSION),
        Some(Value::String(text)) if text == "1" => Some(SDK_API_VERSION),
        Some(Value::Number(number)) if number.as_f64() == Some(1.0) => Some(SDK_API_VERSION),
        _ => None,
    }
}

fn string_value(
    value: Option<&Value>,
    field: &str,
    max: usize,
    errors: &mut Vec<String>,
    required: bool,
    trim: bool,
) -> String {
    match value {
        Some(Value::String(raw)) => {
            let normalized = if trim {
                raw.trim().to_string()
            } else {
                raw.clone()
            };
            if required && normalized.is_empty() {
                errors.push(format!("{field}_obrigatorio"));
            }
            if js_len(&normalized) > max {
                errors.push(format!("{field}_muito_longo"));
            }
            if normalized.contains('\0') {
                errors.push(format!("{field}_invalido"));
            }
            normalized
        }
        Some(_) => {
            errors.push(format!("{field}_invalido"));
            String::new()
        }
        None => {
            if required {
                errors.push(format!("{field}_invalido"));
            }
            String::new()
        }
    }
}

fn normalize_permissions(value: Option<&Value>, errors: &mut Vec<String>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Value::Array(items) = value else {
        errors.push("permissions_invalido".to_string());
        return Vec::new();
    };
    if items.len() > MAX_PERMISSIONS {
        errors.push("permissions_invalido".to_string());
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for item in items {
        let Value::String(raw) = item else {
            errors.push("permission_invalida".to_string());
            continue;
        };
        let permission = raw.trim();
        if !PLUGIN_PERMISSIONS.contains(&permission) {
            errors.push(format!(
                "permission_nao_suportada:{}",
                truncate_js(permission, 80)
            ));
            continue;
        }
        if !seen.insert(permission.to_string()) {
            errors.push(format!("permission_duplicada:{permission}"));
            continue;
        }
        result.push(permission.to_string());
    }
    result
}

fn entry_is_absolute(entry: &str) -> bool {
    if entry.starts_with('/') {
        return true;
    }
    // No Windows `path.isAbsolute("C:/x")` e `path.isAbsolute("C:\\x")` são
    // verdadeiros; a barra invertida já é rejeitada em qualquer posição.
    let bytes = entry.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

/// Implementação de `path.posix.normalize` suficiente para comparar com a
/// entrada sem tocar no disco.
fn posix_normalize(entry: &str) -> String {
    if entry.is_empty() {
        return ".".to_string();
    }
    let absolute = entry.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in entry.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|last| *last != "..") {
                    parts.pop();
                } else if !absolute {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    if absolute {
        if joined.is_empty() {
            "/".to_string()
        } else {
            format!("/{joined}")
        }
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

fn normalize_entry(
    input: &Map<String, Value>,
    errors: &mut Vec<String>,
    require_entry: bool,
) -> String {
    let value = first(input, &["entry", "entrypoint"]);
    // `undefined` com require_entry=false é o único caso aceito sem validação.
    if value.is_none() && !require_entry {
        return String::new();
    }
    let entry = string_value(
        value,
        "entry",
        MAX_ENTRY_LENGTH,
        errors,
        require_entry,
        true,
    );
    if entry.is_empty() {
        return String::new();
    }
    if entry_is_absolute(&entry) || entry.contains('\0') || entry.contains('\\') {
        errors.push("entry_invalido".to_string());
        return String::new();
    }
    let normalized = posix_normalize(&entry);
    if normalized != entry
        || normalized == "."
        || normalized.starts_with("../")
        || normalized.contains("/../")
        || normalized.ends_with("/..")
    {
        errors.push("entry_invalido".to_string());
        return String::new();
    }
    normalized
}

fn normalize_entry_digest(value: Option<&Value>, errors: &mut Vec<String>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    let Value::String(raw) = value else {
        errors.push("entry_digest_invalido".to_string());
        return String::new();
    };
    let digest = raw.trim().to_lowercase();
    if !ENTRY_DIGEST_RE.is_match(&digest) {
        errors.push("entry_digest_invalido".to_string());
        return String::new();
    }
    digest
}

pub fn normalize_signing_key_id(value: &str) -> String {
    let key_id = value.trim();
    if key_id.is_empty()
        || js_len(key_id) > SIGNING_KEY_ID_MAX_LENGTH
        || key_id.contains('\0')
        || key_id.contains('/')
        || key_id.contains('\\')
        || !SIGNING_KEY_ID_RE.is_match(key_id)
    {
        return String::new();
    }
    key_id.to_string()
}

fn normalize_signing_key_id_value(value: Option<&Value>, errors: &mut Vec<String>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    let Value::String(raw) = value else {
        errors.push("signing_key_id_invalido".to_string());
        return String::new();
    };
    let key_id = normalize_signing_key_id(raw);
    if key_id.is_empty() {
        errors.push("signing_key_id_invalido".to_string());
    }
    key_id
}

fn decode_signature_strict(value: &str) -> Option<Vec<u8>> {
    if value.is_empty() || !value.len().is_multiple_of(4) || !SIGNATURE_BASE64_RE.is_match(value) {
        return None;
    }
    let bytes = STANDARD.decode(value).ok()?;
    if bytes.len() != SIGNATURE_LENGTH || STANDARD.encode(&bytes) != value {
        return None;
    }
    Some(bytes)
}

fn normalize_signature(value: Option<&Value>, errors: &mut Vec<String>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    let Value::String(signature) = value else {
        errors.push("signature_invalida".to_string());
        return String::new();
    };
    // O campo não sofre trim: espaços escondidos fariam a assinatura revisada
    // diferir dos bytes realmente cobertos.
    if signature.len() != 88 || decode_signature_strict(signature).is_none() {
        errors.push("signature_invalida".to_string());
        return String::new();
    }
    signature.clone()
}

/// Valida e canoniza um objeto de manifesto.
pub fn validate_manifest(input: &Value) -> ManifestValidation {
    validate_manifest_with(input, ValidateManifestOptions::default())
}

pub fn validate_manifest_with(
    input: &Value,
    options: ValidateManifestOptions,
) -> ManifestValidation {
    let Value::Object(input) = input else {
        return ManifestValidation::failure(["manifest_invalido".to_string()]);
    };
    let mut errors = Vec::new();

    for key in input.keys() {
        if key == "__proto__" || key == "prototype" || key == "constructor" {
            errors.push("campo_invalido".to_string());
        }
    }

    let alias_groups: [&[&str]; 4] = [
        &["manifestVersion", "manifest_version", "schemaVersion"],
        &["apiVersion", "api_version"],
        &["entry", "entrypoint"],
        &ENTRY_DIGEST_ALIASES,
    ];
    for group in alias_groups {
        let present = group.iter().filter(|key| input.contains_key(**key)).count();
        if present > 1 {
            errors.push(format!("campo_duplicado:{}", group[0]));
        }
    }

    let manifest_version = normalize_manifest_version(first(
        input,
        &["manifestVersion", "manifest_version", "schemaVersion"],
    ));
    if manifest_version.is_none() {
        errors.push("manifest_version_invalida".to_string());
    }

    let api_version = normalize_api_version(first(input, &["apiVersion", "api_version"]));
    if api_version.is_none() {
        errors.push("api_version_invalida".to_string());
    }

    let id_result = validate_id(input.get("id").and_then(Value::as_str).unwrap_or_default());
    if !id_result.ok {
        errors.extend(id_result.errors.iter().cloned());
    }

    let name = string_value(
        input.get("name"),
        "name",
        MAX_NAME_LENGTH,
        &mut errors,
        true,
        true,
    );
    let version = string_value(
        input.get("version"),
        "version",
        MAX_VERSION_LENGTH,
        &mut errors,
        true,
        true,
    );
    if !version.is_empty() && !SEMVER_RE.is_match(&version) {
        errors.push("version_invalida".to_string());
    }
    let description = string_value(
        input.get("description"),
        "description",
        MAX_DESCRIPTION_LENGTH,
        &mut errors,
        false,
        true,
    );
    let entry = normalize_entry(input, &mut errors, options.require_entry);
    let entry_sha256 = normalize_entry_digest(first(input, &ENTRY_DIGEST_ALIASES), &mut errors);

    let has_signing_key_id = input.contains_key("signingKeyId");
    let has_signature = input.contains_key("signature");
    let signing_key_id = normalize_signing_key_id_value(input.get("signingKeyId"), &mut errors);
    let signature = normalize_signature(input.get("signature"), &mut errors);
    if has_signing_key_id != has_signature {
        errors.push("assinatura_incompleta".to_string());
        if !has_signing_key_id {
            errors.push("signing_key_id_obrigatorio".to_string());
        }
        if !has_signature {
            errors.push("signature_obrigatoria".to_string());
        }
    }

    let permissions = normalize_permissions(input.get("permissions"), &mut errors);

    let mut known: HashSet<&str> = HashSet::new();
    for key in [
        "manifestVersion",
        "manifest_version",
        "schemaVersion",
        "apiVersion",
        "api_version",
        "id",
        "name",
        "version",
        "description",
        "entry",
        "entrypoint",
        "signingKeyId",
        "signature",
        "permissions",
    ] {
        known.insert(key);
    }
    for key in ENTRY_DIGEST_ALIASES {
        known.insert(key);
    }
    for key in input.keys() {
        if !known.contains(key.as_str()) {
            errors.push(format!("campo_desconhecido:{}", truncate_js(key, 80)));
        }
    }

    if !errors.is_empty() {
        return ManifestValidation::failure(errors);
    }

    let has_signing_pair = !signing_key_id.is_empty() && !signature.is_empty();
    let manifest = PluginManifest {
        manifest_version: manifest_version.unwrap_or(MANIFEST_VERSION),
        api_version: api_version.unwrap_or(SDK_API_VERSION),
        id: id_result.id,
        name,
        version,
        description,
        entry,
        permissions,
        entry_sha256: if entry_sha256.is_empty() {
            None
        } else {
            Some(entry_sha256)
        },
        signing_key_id: if has_signing_pair {
            Some(signing_key_id)
        } else {
            None
        },
        signature: if has_signing_pair {
            Some(signature)
        } else {
            None
        },
    };
    ManifestValidation::success(manifest)
}

pub fn parse_manifest(text: &str) -> ManifestValidation {
    parse_manifest_with(text, ValidateManifestOptions::default())
}

pub fn parse_manifest_with(text: &str, options: ValidateManifestOptions) -> ManifestValidation {
    let value: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(_) => {
            return ManifestValidation::failure(["json_invalido".to_string()]);
        }
    };
    validate_manifest_with(&value, options)
}

/// Lê `plugin.json` recusando symlinks e arquivos que não sejam regulares.
pub fn read_manifest(file: &Path) -> ManifestValidation {
    read_manifest_with(file, ValidateManifestOptions::default())
}

pub fn read_manifest_with(file: &Path, options: ValidateManifestOptions) -> ManifestValidation {
    let display = file.to_string_lossy();
    if display.trim().is_empty() {
        return ManifestValidation::failure(["caminho_invalido".to_string()]);
    }
    let metadata = match fs::symlink_metadata(file) {
        Ok(metadata) => metadata,
        Err(_) => {
            return ManifestValidation::failure(["manifest_nao_encontrado".to_string()]);
        }
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return ManifestValidation::failure(["manifest_invalido".to_string()]);
    }
    match fs::read_to_string(file) {
        Ok(text) => parse_manifest_with(&text, options),
        Err(_) => ManifestValidation::failure(["manifest_nao_encontrado".to_string()]),
    }
}

/// Cópia segura para a UI (sem caminho de pacote/registro).
pub fn public_manifest(manifest: &PluginManifest) -> PluginManifest {
    manifest.public()
}

/// Resolve o caminho absoluto lexical (sem resolver symlinks), como
/// `path.resolve`.
pub fn resolve_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let is_absolute = absolute.is_absolute();
    let mut result = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => result.push(prefix.as_os_str()),
            Component::RootDir => result.push(Component::RootDir.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() && !is_absolute {
                    result.push("..");
                }
            }
            Component::Normal(part) => result.push(part),
        }
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    result
}

pub fn is_inside(root: &Path, target: &Path) -> bool {
    let base = resolve_path(root);
    let child = resolve_path(target);
    child == base || child.strip_prefix(&base).is_ok()
}

fn io_error_is_loop(error: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        error.raw_os_error() == Some(40)
    }
    #[cfg(not(unix))]
    {
        let _ = error;
        false
    }
}

fn canonicalize_entry(path: &Path) -> Result<PathBuf, &'static str> {
    fs::canonicalize(path).map_err(|error| {
        if io_error_is_loop(&error) {
            "entry_invalido"
        } else {
            "entry_nao_verificavel"
        }
    })
}

/// Resolve o entry sem permitir absolutos, traversal ou symlinks que escapem
/// do diretório do pacote.
pub fn resolve_plugin_entry(plugin_dir: &Path, entry: &str) -> EntryResolution {
    let directory_display = plugin_dir.to_string_lossy();
    if directory_display.trim().is_empty() || entry.trim().is_empty() {
        return EntryResolution {
            ok: false,
            path: None,
            error: "entry_invalido".to_string(),
        };
    }
    if entry_is_absolute(entry) || entry.contains('\0') || entry.contains('\\') {
        return EntryResolution {
            ok: false,
            path: None,
            error: "entry_invalido".to_string(),
        };
    }
    let root = resolve_path(plugin_dir);
    let candidate = resolve_path(&root.join(entry));
    if !is_inside(&root, &candidate) || candidate == root {
        return EntryResolution {
            ok: false,
            path: None,
            error: "entry_fora_do_plugin".to_string(),
        };
    }
    let Ok(root_stat) = fs::symlink_metadata(&root) else {
        return EntryResolution {
            ok: false,
            path: None,
            error: "entry_nao_encontrado".to_string(),
        };
    };
    let Ok(entry_stat) = fs::symlink_metadata(&candidate) else {
        return EntryResolution {
            ok: false,
            path: None,
            error: "entry_nao_encontrado".to_string(),
        };
    };
    if !root_stat.is_dir() || root_stat.file_type().is_symlink() || !entry_stat.is_file() {
        return EntryResolution {
            ok: false,
            path: None,
            error: "entry_invalido".to_string(),
        };
    }
    let real_root = match fs::canonicalize(&root) {
        Ok(path) => path,
        Err(_) => {
            return EntryResolution {
                ok: false,
                path: None,
                error: "entry_nao_encontrado".to_string(),
            };
        }
    };
    let real_entry = match fs::canonicalize(&candidate) {
        Ok(path) => path,
        Err(_) => {
            return EntryResolution {
                ok: false,
                path: None,
                error: "entry_nao_encontrado".to_string(),
            };
        }
    };
    if !is_inside(&real_root, &real_entry) {
        return EntryResolution {
            ok: false,
            path: None,
            error: "entry_fora_do_plugin".to_string(),
        };
    }
    EntryResolution {
        ok: true,
        path: Some(candidate),
        error: String::new(),
    }
}

/// Calcula SHA-256 dos bytes do entry sem executá-lo. Quando `root` é
/// fornecido, o caminho real também precisa permanecer dentro do pacote.
pub fn compute_entry_sha256(file: &Path, root: Option<&Path>) -> EntryDigest {
    let display = file.to_string_lossy();
    if display.trim().is_empty() || display.contains('\0') {
        return EntryDigest {
            ok: false,
            digest: String::new(),
            error: "entry_invalido".to_string(),
        };
    }

    let real_file = if let Some(root) = root {
        let root_display = root.to_string_lossy();
        if root_display.trim().is_empty() || root_display.contains('\0') {
            return EntryDigest {
                ok: false,
                digest: String::new(),
                error: "entry_invalido".to_string(),
            };
        }
        let real_root = match canonicalize_entry(&resolve_path(root)) {
            Ok(path) => path,
            Err(error) => {
                return EntryDigest {
                    ok: false,
                    digest: String::new(),
                    error: error.to_string(),
                };
            }
        };
        let real_file = match canonicalize_entry(&resolve_path(file)) {
            Ok(path) => path,
            Err(error) => {
                return EntryDigest {
                    ok: false,
                    digest: String::new(),
                    error: error.to_string(),
                };
            }
        };
        if !is_inside(&real_root, &real_file) {
            return EntryDigest {
                ok: false,
                digest: String::new(),
                error: "entry_fora_do_plugin".to_string(),
            };
        }
        real_file
    } else {
        match canonicalize_entry(&resolve_path(file)) {
            Ok(path) => path,
            Err(error) => {
                return EntryDigest {
                    ok: false,
                    digest: String::new(),
                    error: error.to_string(),
                };
            }
        }
    };

    let metadata = match fs::symlink_metadata(&real_file) {
        Ok(metadata) => metadata,
        Err(error) => {
            let code = if io_error_is_loop(&error) {
                "entry_invalido"
            } else {
                "entry_nao_verificavel"
            };
            return EntryDigest {
                ok: false,
                digest: String::new(),
                error: code.to_string(),
            };
        }
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return EntryDigest {
            ok: false,
            digest: String::new(),
            error: "entry_invalido".to_string(),
        };
    }

    let mut handle = match fs::File::open(&real_file) {
        Ok(handle) => handle,
        Err(error) => {
            let code = if io_error_is_loop(&error) {
                "entry_invalido"
            } else {
                "entry_nao_verificavel"
            };
            return EntryDigest {
                ok: false,
                digest: String::new(),
                error: code.to_string(),
            };
        }
    };
    match handle.metadata() {
        Ok(opened) if opened.is_file() => {}
        _ => {
            return EntryDigest {
                ok: false,
                digest: String::new(),
                error: "entry_invalido".to_string(),
            };
        }
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        match handle.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => hasher.update(&buffer[..count]),
            Err(_) => {
                return EntryDigest {
                    ok: false,
                    digest: String::new(),
                    error: "entry_nao_verificavel".to_string(),
                };
            }
        }
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    EntryDigest {
        ok: true,
        digest: hex,
        error: String::new(),
    }
}
