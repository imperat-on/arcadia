//! Redirect do launcher e credencial do Ryuu.
//!
//! Oráculo: `fixes.js` (`buildLauncherRedirect`, `normalizeRyuuAuth`,
//! `ryuuAuthHeaderLine`, `readRyuuAuth`, `writeRyuuAuth`, `setRyuuAuth`,
//! `getRyuuAuthStatus`, `clearRyuuAuth`).
//!
//! Regras preservadas:
//! - o redirect usa **o primeiro** launcher do manifest e só existe se o
//!   arquivo estiver no disco, no formato `"<abs>" %command%`;
//! - Ryuu aceita `X-Auth-Key` ou `Cookie: session=...` (nunca Bearer);
//! - credencial antiga em `ryuu_auth.txt` é migrada como key e apagada;
//! - o arquivo novo é `ryuu_auth.json` com permissão 0600 quando possível.
//!
//! Nenhum segredo sai em `Debug`; o status exposto à UI continua tendo só
//! `configured`/`kind`, e uma variante mascarada é oferecida para diagnósticos.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::install::{is_within, read_manifest, resolve_relative, FixFs, FsError, FsResult};

pub const RYUU_AUTH_FILE: &str = "ryuu_auth.json";
pub const RYUU_AUTH_FILE_LEGACY: &str = "ryuu_auth.txt";
pub const RYUU_AUTH_MAX_LEN: usize = 8192;

/// Tipo da credencial aceita pelo Ryuu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RyuuAuthKind {
    Key,
    Session,
}

impl RyuuAuthKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RyuuAuthKind::Key => "key",
            RyuuAuthKind::Session => "session",
        }
    }
}

/// Credencial normalizada. O `Debug` é redigido.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RyuuAuth {
    pub kind: RyuuAuthKind,
    pub value: String,
}

impl std::fmt::Debug for RyuuAuth {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RyuuAuth")
            .field("kind", &self.kind)
            .field("value", &self.masked())
            .finish()
    }
}

impl RyuuAuth {
    pub fn key(value: impl Into<String>) -> Self {
        Self {
            kind: RyuuAuthKind::Key,
            value: value.into(),
        }
    }

    pub fn session(value: impl Into<String>) -> Self {
        Self {
            kind: RyuuAuthKind::Session,
            value: value.into(),
        }
    }

    /// `ryuuAuthHeaderLine(cred)`: linha de header pronta para o curl.
    pub fn header_line(&self) -> Option<String> {
        let value = self.value.trim();
        if value.is_empty() || value.contains('\r') || value.contains('\n') {
            return None;
        }
        match self.kind {
            RyuuAuthKind::Session => Some(format!("Cookie: session={value}\n")),
            RyuuAuthKind::Key => Some(format!("X-Auth-Key: {value}\n")),
        }
    }

    /// `true` quando a credencial geraria um header válido.
    pub fn is_valid(&self) -> bool {
        self.header_line().is_some()
    }

    /// Máscara estilo `redigirSegredos`: `abc•••xy` ou `•••`.
    pub fn masked(&self) -> String {
        mask_secret(self.value.trim())
    }
}

/// Máscara de segredo (`abc•••xy` para > 8 caracteres).
pub fn mask_secret(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() > 8 {
        let start: String = chars.iter().take(3).collect();
        let end: String = chars.iter().skip(chars.len() - 2).collect();
        format!("{start}•••{end}")
    } else {
        "•••".to_string()
    }
}

/// `normalizeRyuuAuth(raw)`.
///
/// Formatos aceitos: `X-Auth-Key: ...`, `Cookie: ... session=...`, `session=...`
/// ou valor cru (key). `None` para vazio, multilinha, > 8192 ou header
/// desconhecido.
pub fn normalize_ryuu_auth(raw: &str) -> Option<RyuuAuth> {
    let input = raw.trim();
    if input.is_empty() || input.contains('\r') || input.contains('\n') {
        return None;
    }
    if input.chars().count() > RYUU_AUTH_MAX_LEN {
        return None;
    }
    if let Some((name, value)) = input.split_once(':') {
        let name = name.trim();
        if !name.is_empty()
            && name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
        {
            let value = value.trim();
            if value.is_empty() {
                return None;
            }
            let lower = name.to_ascii_lowercase();
            if lower == "x-auth-key" {
                return Some(RyuuAuth::key(value));
            }
            if lower == "cookie" {
                return cookie_session(value).map(RyuuAuth::session);
            }
            return None;
        }
    }
    if let Some(value) = input.strip_prefix("session=") {
        let value = value.trim();
        if !value.is_empty() {
            return Some(RyuuAuth::session(value));
        }
    }
    Some(RyuuAuth::key(input))
}

fn cookie_session(cookie: &str) -> Option<String> {
    for part in cookie.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("session=") {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Status exato consumido pela UI (`{configured, kind}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RyuuAuthStatus {
    pub configured: bool,
    pub kind: Option<String>,
}

impl RyuuAuthStatus {
    pub fn empty() -> Self {
        Self {
            configured: false,
            kind: None,
        }
    }

    pub fn from_auth(auth: Option<&RyuuAuth>) -> Self {
        Self {
            configured: auth.is_some(),
            kind: auth.map(|auth| auth.kind.as_str().to_string()),
        }
    }
}

/// Status mascarado para diagnóstico (nunca contém o segredo).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaskedRyuuAuthStatus {
    pub configured: bool,
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub masked: Option<String>,
}

/// Erro de `setRyuuAuth`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RyuuAuthError {
    InvalidCredential,
    Storage(FsError),
}

impl std::fmt::Display for RyuuAuthError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RyuuAuthError::InvalidCredential => formatter.write_str("invalid_credential"),
            RyuuAuthError::Storage(error) => formatter.write_str(&error.message),
        }
    }
}

impl std::error::Error for RyuuAuthError {}

impl From<FsError> for RyuuAuthError {
    fn from(error: FsError) -> Self {
        Self::Storage(error)
    }
}

/// Armazenamento de `ryuu_auth.json`/`ryuu_auth.txt`, sempre dentro de
/// `data_dir`. Usa o filesystem injetado.
pub struct RyuuAuthStore {
    fs: Arc<dyn FixFs>,
    data_dir: PathBuf,
}

impl RyuuAuthStore {
    pub fn new(fs: Arc<dyn FixFs>, data_dir: impl Into<PathBuf>) -> Self {
        Self {
            fs,
            data_dir: data_dir.into(),
        }
    }

    pub fn auth_path(&self) -> PathBuf {
        self.data_dir.join(RYUU_AUTH_FILE)
    }

    pub fn legacy_path(&self) -> PathBuf {
        self.data_dir.join(RYUU_AUTH_FILE_LEGACY)
    }

    fn ensure_allowed(&self, path: &Path) -> Result<(), FsError> {
        let data_dir = &self.data_dir;
        let sane_root = data_dir.is_absolute()
            && !data_dir
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir));
        if !sane_root || !is_within(data_dir, path) {
            return Err(FsError::new("ryuu auth path rejected"));
        }
        Ok(())
    }

    /// `readRyuuAuth`: JSON válido, senão migra o legado como key.
    pub fn read(&self) -> Option<RyuuAuth> {
        let auth_path = self.auth_path();
        if self.ensure_allowed(&auth_path).is_err() {
            return None;
        }
        if let Ok(bytes) = self.fs.read(&auth_path) {
            if let Ok(auth) = serde_json::from_slice::<RyuuAuth>(&bytes) {
                if auth.is_valid() {
                    return Some(auth);
                }
            }
        }
        let legacy_path = self.legacy_path();
        if self.ensure_allowed(&legacy_path).is_err() {
            return None;
        }
        let legacy = self
            .fs
            .read(&legacy_path)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string())
            .filter(|value| !value.is_empty())?;
        let auth = RyuuAuth::key(legacy);
        // Migração transparente; falha de escrita não impede o uso imediato.
        let _ = self.write(&auth);
        let _ = self.fs.remove_file(&legacy_path);
        Some(auth)
    }

    /// `writeRyuuAuth`: JSON compacto + modo 0600.
    pub fn write(&self, auth: &RyuuAuth) -> Result<(), RyuuAuthError> {
        let auth_path = self.auth_path();
        self.ensure_allowed(&auth_path)?;
        let bytes = serde_json::to_vec(auth)
            .map_err(|error| RyuuAuthError::Storage(FsError::new(error.to_string())))?;
        self.fs.create_dir_all(&self.data_dir)?;
        self.fs.write(&auth_path, &bytes)?;
        self.fs.set_private(&auth_path)?;
        Ok(())
    }

    /// `setRyuuAuth(key)`.
    pub fn set(&self, raw: &str) -> Result<RyuuAuth, RyuuAuthError> {
        let auth = normalize_ryuu_auth(raw).ok_or(RyuuAuthError::InvalidCredential)?;
        self.write(&auth)?;
        Ok(auth)
    }

    /// `getRyuuAuthStatus()`.
    pub fn status(&self) -> RyuuAuthStatus {
        RyuuAuthStatus::from_auth(self.read().as_ref())
    }

    /// Status com valor mascarado (não usado pela UI; não vaza o segredo).
    pub fn masked_status(&self) -> MaskedRyuuAuthStatus {
        let auth = self.read();
        MaskedRyuuAuthStatus {
            configured: auth.is_some(),
            kind: auth.as_ref().map(|auth| auth.kind.as_str().to_string()),
            masked: auth.as_ref().map(RyuuAuth::masked),
        }
    }

    /// `clearRyuuAuth()`.
    pub fn clear(&self) -> FsResult<()> {
        let auth_path = self.auth_path();
        let legacy_path = self.legacy_path();
        self.ensure_allowed(&auth_path)?;
        self.ensure_allowed(&legacy_path)?;
        if self.fs.exists(&auth_path) {
            let _ = self.fs.remove_file(&auth_path);
        }
        if self.fs.exists(&legacy_path) {
            let _ = self.fs.remove_file(&legacy_path);
        }
        Ok(())
    }
}

/// `buildLauncherRedirect(installPath)`.
///
/// Usa o primeiro launcher do manifest. Caminho absoluto só é aceito quando
/// está dentro da instalação (defesa contra manifest adulterado); relativo é
/// resolvido sob a raiz e rejeita `..`.
pub fn build_launcher_redirect(fs: &dyn FixFs, install_root: &Path) -> Option<String> {
    let manifest = crate::install::launcher_manifest_path(install_root);
    let launcher = read_manifest(fs, &manifest).into_iter().next()?;
    let launcher_path = Path::new(&launcher);
    let absolute = if launcher_path.is_absolute() {
        let path = launcher_path.to_path_buf();
        if !is_within(install_root, &path) {
            return None;
        }
        path
    } else {
        resolve_relative(install_root, &launcher)?
    };
    if !fs.exists(&absolute) {
        return None;
    }
    Some(format!("\"{}\" %command%", absolute.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::{launcher_manifest_path, FixFs, StdFixFs};

    fn store(temp: &tempfile::TempDir) -> RyuuAuthStore {
        RyuuAuthStore::new(Arc::new(StdFixFs), temp.path())
    }

    #[test]
    fn normalizes_raw_key_header_and_cookie() {
        assert_eq!(
            normalize_ryuu_auth("  abc123  "),
            Some(RyuuAuth::key("abc123"))
        );
        assert_eq!(
            normalize_ryuu_auth("X-Auth-Key: abc123"),
            Some(RyuuAuth::key("abc123"))
        );
        assert_eq!(
            normalize_ryuu_auth("x-auth-key:   abc"),
            Some(RyuuAuth::key("abc"))
        );
        assert_eq!(
            normalize_ryuu_auth("Cookie: foo=1; session=xyz; bar=2"),
            Some(RyuuAuth::session("xyz"))
        );
        assert_eq!(
            normalize_ryuu_auth("session=xyz"),
            Some(RyuuAuth::session("xyz"))
        );
    }

    #[test]
    fn rejects_empty_multiline_oversized_and_unknown_headers() {
        assert_eq!(normalize_ryuu_auth(""), None);
        assert_eq!(normalize_ryuu_auth("a\nb"), None);
        assert_eq!(
            normalize_ryuu_auth(&"a".repeat(RYUU_AUTH_MAX_LEN + 1)),
            None
        );
        assert_eq!(normalize_ryuu_auth("Authorization: Bearer x"), None);
        assert_eq!(normalize_ryuu_auth("Cookie: foo=1"), None);
        assert_eq!(normalize_ryuu_auth("X-Auth-Key:"), None);
    }

    #[test]
    fn header_lines_never_use_bearer() {
        let key = RyuuAuth::key("abc");
        assert_eq!(key.header_line().as_deref(), Some("X-Auth-Key: abc\n"));
        let session = RyuuAuth::session("xyz");
        assert_eq!(
            session.header_line().as_deref(),
            Some("Cookie: session=xyz\n")
        );
        assert_eq!(RyuuAuth::key("  ").header_line(), None);
        assert_eq!(RyuuAuth::key("a\nb").header_line(), None);
    }

    #[test]
    fn secret_mask_and_debug_do_not_leak() {
        let auth = RyuuAuth::key("supersecret-token");
        assert_eq!(auth.masked(), "sup•••en");
        assert!(!format!("{auth:?}").contains("supersecret-token"));
        assert_eq!(mask_secret("short"), "•••");
        let json = serde_json::to_string(&auth).unwrap();
        assert!(json.contains("supersecret-token"));
        let masked = serde_json::to_string(&MaskedRyuuAuthStatus {
            configured: true,
            kind: Some("key".into()),
            masked: Some(auth.masked()),
        })
        .unwrap();
        assert!(!masked.contains("supersecret-token"));
    }

    #[test]
    fn store_roundtrip_status_and_masked_status() {
        let temp = tempfile::tempdir().unwrap();
        let store = store(&temp);
        assert_eq!(store.status(), RyuuAuthStatus::empty());
        let auth = store.set("session=abc").unwrap();
        assert_eq!(auth.kind, RyuuAuthKind::Session);
        assert_eq!(
            store.status(),
            RyuuAuthStatus {
                configured: true,
                kind: Some("session".into())
            }
        );
        assert_eq!(store.read(), Some(RyuuAuth::session("abc")));
        let masked = store.masked_status();
        assert!(masked.configured);
        assert_eq!(masked.masked.as_deref(), Some("•••"));
        let raw = std::fs::read_to_string(temp.path().join(RYUU_AUTH_FILE)).unwrap();
        assert_eq!(raw, r#"{"kind":"session","value":"abc"}"#);
        assert!(!masked_json(&store).contains("abc"));
    }

    fn masked_json(store: &RyuuAuthStore) -> String {
        serde_json::to_string(&store.masked_status()).unwrap()
    }

    #[test]
    fn legacy_text_is_migrated_as_key_and_removed() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join(RYUU_AUTH_FILE_LEGACY), b" old-key \n").unwrap();
        let store = store(&temp);
        assert_eq!(store.read(), Some(RyuuAuth::key("old-key")));
        assert!(!temp.path().join(RYUU_AUTH_FILE_LEGACY).exists());
        let json = std::fs::read_to_string(temp.path().join(RYUU_AUTH_FILE)).unwrap();
        assert_eq!(json, r#"{"kind":"key","value":"old-key"}"#);
    }

    #[test]
    fn invalid_set_returns_invalid_credential_and_leaves_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let store = store(&temp);
        assert_eq!(
            store.set("Authorization: Bearer x"),
            Err(RyuuAuthError::InvalidCredential)
        );
        assert!(!temp.path().join(RYUU_AUTH_FILE).exists());
        assert_eq!(store.status(), RyuuAuthStatus::empty());
    }

    #[test]
    fn clear_removes_both_files() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join(RYUU_AUTH_FILE_LEGACY), b"legacy").unwrap();
        let store = store(&temp);
        store.set("key123").unwrap();
        store.clear().unwrap();
        assert!(!temp.path().join(RYUU_AUTH_FILE).exists());
        assert!(!temp.path().join(RYUU_AUTH_FILE_LEGACY).exists());
        assert_eq!(store.status(), RyuuAuthStatus::empty());
    }

    #[test]
    fn launcher_redirect_uses_first_existing_launcher() {
        let temp = tempfile::tempdir().unwrap();
        let install = temp.path().join("game");
        std::fs::create_dir_all(install.join("bin")).unwrap();
        let fs = StdFixFs;
        std::fs::write(install.join("bin/Launcher.exe"), b"exe").unwrap();
        write_launcher_manifest(&fs, &install, &["bin/Launcher.exe", "other.exe"]);
        let redirect = build_launcher_redirect(&fs, &install).unwrap();
        let expected = format!(
            "\"{}\" %command%",
            install.join("bin").join("Launcher.exe").to_string_lossy()
        );
        assert_eq!(redirect, expected);
    }

    #[test]
    fn launcher_redirect_requires_file_and_safe_manifest_entries() {
        let temp = tempfile::tempdir().unwrap();
        let install = temp.path().join("game");
        std::fs::create_dir_all(&install).unwrap();
        let fs = StdFixFs;
        assert_eq!(build_launcher_redirect(&fs, &install), None);
        write_launcher_manifest(&fs, &install, &["missing.exe"]);
        assert_eq!(build_launcher_redirect(&fs, &install), None);
        write_launcher_manifest(&fs, &install, &["../outside.exe"]);
        std::fs::write(temp.path().join("outside.exe"), b"x").unwrap();
        assert_eq!(build_launcher_redirect(&fs, &install), None);
        let outside = temp.path().join("outside.exe");
        write_launcher_manifest(&fs, &install, &[outside.to_str().unwrap()]);
        assert_eq!(build_launcher_redirect(&fs, &install), None);
    }

    fn write_launcher_manifest(fs: &dyn FixFs, install: &Path, launchers: &[&str]) {
        let lines: Vec<String> = launchers.iter().map(|line| (*line).to_string()).collect();
        crate::install::write_manifest(fs, &launcher_manifest_path(install), &lines).unwrap();
    }
}
