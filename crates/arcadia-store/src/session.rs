//! Sessão do backend persistida em `session.json` — espelho de
//! `app/electron/supabase/session.js`.
//!
//! Formatos lidos (os envelopes v2 que `session.js` aceita):
//!
//! - envelope v2 em texto puro:
//!   `{"v":2,"enc":false,"data":"<base64(JSON da sessão)>"}`
//! - envelope v2 criptografado:
//!   `{"v":2,"enc":true,"data":"<base64(ciphertext)>"}`.
//!   No Windows o ciphertext é DPAPI puro (`CryptProtectData`, escopo do
//!   usuário), o formato do `safeStorage` legado. Blobs modernos `v10`/`v11`
//!   (AES-256-GCM com a chave no `Local State`, protegida por DPAPI) são
//!   reconhecidos, mas devolvem erro tipado e o arquivo é preservado.
//!
//! `session.js` **não** aceita um objeto de sessão "solto" (sem envelope): a
//! checagem `box.v !== 2` devolve `null` sem apagar o arquivo. O mesmo vale
//! aqui. Arquivo corrompido ou que falha ao decriptar é apagado e a leitura
//! devolve `None` (auditoria A-13), como no JS.
//!
//! Em plataformas sem keyring implementado (tudo que não é Windows) a escrita
//! usa o fallback de texto puro/base64. Um envelope criptografado encontrado
//! nessas plataformas devolve erro tipado e o arquivo é preservado.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use base64::alphabet;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::{DecodePaddingMode, Engine};
use serde_json::{Map, Value};

use crate::atomic::write_atomic;

pub const SESSION_FILE: &str = "session.json";
pub const SESSION_ENVELOPE_VERSION: u64 = 2;

/// Base64 padrão com padding na escrita. Na leitura aceita as variações que o
/// `Buffer.from(s, "base64")` do Node aceita: sem padding, URL-safe, espaços e
/// caracteres inválidos são ignorados.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_encode_padding(true)
        .with_decode_allow_trailing_bits(true)
        .with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

#[derive(Debug)]
pub enum SessionError {
    Io(io::Error),
    Json(serde_json::Error),
    /// Falha do DPAPI (ou do futuro keyring de outra plataforma).
    Crypto(String),
    /// Envelope criptografado encontrado numa plataforma sem implementação de
    /// decriptação. O arquivo é mantido intacto.
    UnsupportedEncryptedSession {
        platform: &'static str,
    },
}

impl SessionError {
    pub fn is_unsupported(&self) -> bool {
        matches!(self, SessionError::UnsupportedEncryptedSession { .. })
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::Io(error) => write!(formatter, "falha de I/O na sessão: {error}"),
            SessionError::Json(error) => write!(formatter, "sessão não é JSON válido: {error}"),
            SessionError::Crypto(message) => {
                write!(formatter, "falha de criptografia da sessão: {message}")
            }
            SessionError::UnsupportedEncryptedSession { platform } => write!(
                formatter,
                "formato de sessão criptografada não suportado nesta plataforma ({platform}); \
                 refaça o login ou use uma versão com o keyring compatível"
            ),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SessionError::Io(error) => Some(error),
            SessionError::Json(error) => Some(error),
            SessionError::Crypto(_) | SessionError::UnsupportedEncryptedSession { .. } => None,
        }
    }
}

impl From<io::Error> for SessionError {
    fn from(error: io::Error) -> Self {
        SessionError::Io(error)
    }
}

impl From<serde_json::Error> for SessionError {
    fn from(error: serde_json::Error) -> Self {
        SessionError::Json(error)
    }
}

pub fn session_path(data_dir: &Path) -> PathBuf {
    data_dir.join(SESSION_FILE)
}

/// Salva a sessão no formato que o Electron entende.
///
/// Igual a `saveSession(null)`: um valor `null` não cria arquivo. No Windows
/// grava envelope DPAPI; nas demais plataformas grava o envelope base64 em
/// texto puro (a menos que `ARCADIA_DISABLE_PLAINTEXT_SESSION=1`, caso em que
/// nada é gravado, como no JS sem keyring).
pub fn save_session(data_dir: &Path, session: &Value) -> Result<(), SessionError> {
    save_session_with_mode(data_dir, session, EnvelopeMode::Auto)
}

/// Carrega a sessão; `Ok(None)` cobre arquivo ausente, envelope desconhecido e
/// arquivo corrompido. Um envelope criptografado em plataforma sem suporte
/// devolve `Err(SessionError::UnsupportedEncryptedSession)` sem tocar no disco.
pub fn load_session(data_dir: &Path) -> Result<Option<Value>, SessionError> {
    let path = session_path(data_dir);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(SessionError::Io(error)),
    };

    match decode_envelope(&raw) {
        Ok(session) => Ok(Some(session)),
        Err(EnvelopeFailure::Invalid) => Ok(None),
        Err(EnvelopeFailure::Corrupt) => {
            // Mesma política do session.js: não deixar token velho no disco.
            let _ = std::fs::remove_file(&path);
            Ok(None)
        }
        Err(EnvelopeFailure::Unsupported(platform)) => {
            Err(SessionError::UnsupportedEncryptedSession { platform })
        }
    }
}

/// Remove a sessão do disco. Arquivo inexistente não é erro.
pub fn clear_session(data_dir: &Path) -> io::Result<()> {
    match std::fs::remove_file(session_path(data_dir)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// `Plaintext`/`Encrypted` são usados pelos testes para exercitar os dois
// formatos independentemente do keyring disponível.
#[allow(dead_code)]
enum EnvelopeMode {
    Auto,
    Plaintext,
    Encrypted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvelopeFailure {
    /// Envelope que o JS rejeita com `null` sem apagar o arquivo.
    Invalid,
    /// Envelope v2 cujo conteúdo não decodifica/decripta (o JS apaga).
    Corrupt,
    /// Envelope criptografado num formato sem implementação nesta plataforma.
    Unsupported(&'static str),
}

fn save_session_with_mode(
    data_dir: &Path,
    session: &Value,
    mode: EnvelopeMode,
) -> Result<(), SessionError> {
    if session.is_null() {
        return Ok(());
    }

    let payload = serde_json::to_string(session)?;
    let encrypted = match mode {
        EnvelopeMode::Plaintext => false,
        EnvelopeMode::Encrypted => true,
        EnvelopeMode::Auto => {
            if can_encrypt() {
                true
            } else if allow_plaintext_fallback() {
                false
            } else {
                // `saveSession` loga e não persiste quando não há keyring e o
                // fallback foi desativado explicitamente.
                return Ok(());
            }
        }
    };

    let data = if encrypted {
        encode_base64(&encrypt(payload.as_bytes())?)
    } else {
        encode_base64(payload.as_bytes())
    };

    let mut envelope = Map::new();
    envelope.insert("v".into(), Value::from(SESSION_ENVELOPE_VERSION));
    envelope.insert("enc".into(), Value::Bool(encrypted));
    envelope.insert("data".into(), Value::String(data));
    let serialized = serde_json::to_string(&Value::Object(envelope))?;

    write_atomic(&session_path(data_dir), serialized.as_bytes())?;
    Ok(())
}

fn decode_envelope(raw: &str) -> Result<Value, EnvelopeFailure> {
    let boxed: Value = serde_json::from_str(raw).map_err(|_| EnvelopeFailure::Corrupt)?;
    let Some(object) = boxed.as_object() else {
        return Err(EnvelopeFailure::Invalid);
    };

    // `box.v !== 2` no JS é comparação estrita de número; `2.0` também passa.
    if object.get("v").and_then(Value::as_f64) != Some(SESSION_ENVELOPE_VERSION as f64) {
        return Err(EnvelopeFailure::Invalid);
    }

    let data = match object.get("data") {
        Some(Value::String(data)) => data,
        // `Buffer.from(undefined, "base64")` lança no JS: cai no catch e apaga.
        _ => return Err(EnvelopeFailure::Corrupt),
    };
    let decoded = decode_base64(data).ok_or(EnvelopeFailure::Corrupt)?;

    let plaintext = if js_truthy(object.get("enc")) {
        decrypt(&decoded).map_err(|error| match error {
            SessionError::UnsupportedEncryptedSession { platform } => {
                EnvelopeFailure::Unsupported(platform)
            }
            _ => EnvelopeFailure::Corrupt,
        })?
    } else {
        decoded
    };

    // `Buffer.toString("utf8")` substitui bytes inválidos; `from_utf8_lossy`
    // reproduz o mesmo comportamento antes do JSON.parse.
    let text = String::from_utf8_lossy(&plaintext);
    serde_json::from_str(&text).map_err(|_| EnvelopeFailure::Corrupt)
}

fn js_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number
            .as_f64()
            .map(|number| number != 0.0 && !number.is_nan())
            .unwrap_or(true),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

fn encode_base64(bytes: &[u8]) -> String {
    BASE64.encode(bytes)
}

fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let mut cleaned = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' => cleaned.push(byte as char),
            // Node aceita o alfabeto URL-safe em Buffer.from(..., "base64").
            b'-' => cleaned.push('+'),
            b'_' => cleaned.push('/'),
            // Padding é recalculado; espaços e lixo são ignorados pelo Node.
            _ => {}
        }
    }

    match cleaned.len() % 4 {
        0 => {}
        // Um caractere solto é descartado pelo decoder do Node.
        1 => {
            cleaned.pop();
        }
        2 => cleaned.push_str("=="),
        3 => cleaned.push('='),
        _ => unreachable!(),
    }

    if cleaned.is_empty() {
        return Some(Vec::new());
    }
    BASE64.decode(cleaned).ok()
}

fn can_encrypt() -> bool {
    cfg!(windows)
}

fn allow_plaintext_fallback() -> bool {
    if std::env::var("ARCADIA_DISABLE_PLAINTEXT_SESSION").as_deref() == Ok("1") {
        return false;
    }
    // O cliente Electron real tem `process.versions.electron` e portanto
    // sempre permite o fallback; um binário nativo é o equivalente desse
    // runtime. Os opt-ins de teste do JS (NODE_ENV=test etc.) ficam cobertos
    // por esse comportamento.
    true
}

#[cfg(windows)]
fn encrypt(data: &[u8]) -> Result<Vec<u8>, SessionError> {
    dpapi::protect(data).map_err(SessionError::Crypto)
}

#[cfg(windows)]
fn decrypt(data: &[u8]) -> Result<Vec<u8>, SessionError> {
    // Electron/Chromium modernos no Windows gravam "v10"/"v11" + AES-256-GCM
    // com a chave no `Local State` (protegida por DPAPI). Isso não é o blob
    // DPAPI puro que sabemos abrir; devolver erro tipado preserva o arquivo em
    // vez de tratá-lo como corrompido e apagá-lo.
    if data.starts_with(b"v10") || data.starts_with(b"v11") {
        return Err(SessionError::UnsupportedEncryptedSession {
            platform: "windows safeStorage v10/v11 (AES-GCM)",
        });
    }
    dpapi::unprotect(data).map_err(SessionError::Crypto)
}

#[cfg(not(windows))]
fn encrypt(_data: &[u8]) -> Result<Vec<u8>, SessionError> {
    Err(SessionError::UnsupportedEncryptedSession {
        platform: std::env::consts::OS,
    })
}

#[cfg(not(windows))]
fn decrypt(_data: &[u8]) -> Result<Vec<u8>, SessionError> {
    Err(SessionError::UnsupportedEncryptedSession {
        platform: std::env::consts::OS,
    })
}

/// DPAPI do Windows, como o caminho legado do `safeStorage` do Electron:
/// escopo do usuário e flags padrão (0), sem entropy adicional.
#[cfg(windows)]
mod dpapi {
    use std::ptr;

    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
    };

    fn input_blob(data: &[u8]) -> Result<CRYPT_INTEGER_BLOB, String> {
        let length = u32::try_from(data.len())
            .map_err(|_| "payload grande demais para o DPAPI".to_string())?;
        Ok(CRYPT_INTEGER_BLOB {
            cbData: length,
            pbData: data.as_ptr() as *mut u8,
        })
    }

    fn take_output(output: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        let bytes = if output.pbData.is_null() || output.cbData == 0 {
            Vec::new()
        } else {
            // Cópia antes do LocalFree; o buffer pertence ao DPAPI.
            unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() }
        };
        if !output.pbData.is_null() {
            unsafe {
                let _ = LocalFree(output.pbData.cast());
            }
        }
        bytes
    }

    fn last_error() -> u32 {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(0) as u32
    }

    pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
        let input = input_blob(data)?;
        let mut output = CRYPT_INTEGER_BLOB::default();
        let ok = unsafe {
            CryptProtectData(
                &input,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                0,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(format!(
                "CryptProtectData falhou (GetLastError={})",
                last_error()
            ));
        }
        Ok(take_output(output))
    }

    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
        let input = input_blob(data)?;
        let mut output = CRYPT_INTEGER_BLOB::default();
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                0,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(format!(
                "CryptUnprotectData falhou (GetLastError={})",
                last_error()
            ));
        }
        Ok(take_output(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-session-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_session() -> Value {
        json!({
            "access_token": "fake.access.token",
            "refresh_token": "fake.refresh.token",
            "expires_at": 9_999_999_999u64,
            "user": { "id": "uuid-teste", "email": "a@b.com" }
        })
    }

    #[test]
    fn plaintext_envelope_round_trip() {
        let dir = temp_dir("plain");
        let session = sample_session();

        save_session_with_mode(&dir, &session, EnvelopeMode::Plaintext).unwrap();

        let raw = std::fs::read_to_string(session_path(&dir)).unwrap();
        assert!(
            raw.starts_with(r#"{"v":2,"enc":false,"data":""#),
            "formato do envelope divergiu: {raw}"
        );
        let envelope: Value = serde_json::from_str(&raw).unwrap();
        let decoded = decode_base64(envelope["data"].as_str().unwrap()).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&decoded).unwrap(), session);

        assert_eq!(load_session(&dir).unwrap(), Some(session));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plaintext_accepts_node_base64_variants() {
        let dir = temp_dir("base64");
        let payload = br#"{"access_token":"x","user":{"id":"u"}}"#;
        let padded = encode_base64(payload);
        let unpadded = padded.trim_end_matches('=').to_string();
        let url_safe = padded.replace('+', "-").replace('/', "_");

        for data in [padded, unpadded, url_safe] {
            let raw = format!(r#"{{"v":2,"enc":false,"data":"{data}"}}"#);
            std::fs::write(session_path(&dir), raw).unwrap();
            let loaded = load_session(&dir).unwrap().unwrap();
            assert_eq!(loaded["user"]["id"], "u");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_raw_session_object_is_rejected_without_deleting() {
        let dir = temp_dir("legacy");
        let path = session_path(&dir);
        std::fs::write(&path, r#"{"access_token":"x","user":{"id":"u"}}"#).unwrap();

        assert_eq!(load_session(&dir).unwrap(), None);
        assert!(path.exists(), "envelope inválido não deve ser apagado");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn future_or_missing_version_is_ignored_without_deleting() {
        let dir = temp_dir("version");
        let path = session_path(&dir);
        for raw in [
            r#"{"v":1,"enc":false,"data":"e30="}"#,
            r#"{"v":3,"enc":false,"data":"e30="}"#,
            r#"{"enc":false,"data":"e30="}"#,
        ] {
            std::fs::write(&path, raw).unwrap();
            assert_eq!(load_session(&dir).unwrap(), None);
            assert!(path.exists());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_is_deleted_and_returns_none() {
        let dir = temp_dir("corrupt");
        let path = session_path(&dir);
        std::fs::write(&path, "{corrompido!!!").unwrap();
        assert_eq!(load_session(&dir).unwrap(), None);
        assert!(!path.exists(), "arquivo corrompido deveria ser apagado");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_and_clear_are_idempotent() {
        let dir = temp_dir("missing");
        assert_eq!(load_session(&dir).unwrap(), None);
        clear_session(&dir).unwrap();

        save_session_with_mode(&dir, &sample_session(), EnvelopeMode::Plaintext).unwrap();
        assert!(session_path(&dir).exists());
        clear_session(&dir).unwrap();
        assert!(!session_path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_null_does_not_create_file() {
        let dir = temp_dir("null");
        save_session(&dir, &Value::Null).unwrap();
        assert!(!session_path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn session_file_has_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir("mode");
        save_session_with_mode(&dir, &sample_session(), EnvelopeMode::Plaintext).unwrap();
        let mode = std::fs::metadata(session_path(&dir))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_envelope_round_trip() {
        let dir = temp_dir("dpapi");
        let session = sample_session();

        save_session(&dir, &session).unwrap();

        let raw = std::fs::read_to_string(session_path(&dir)).unwrap();
        let envelope: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(envelope["v"], json!(2));
        assert_eq!(envelope["enc"], json!(true));
        assert!(envelope["data"]
            .as_str()
            .is_some_and(|data| !data.is_empty()));

        assert_eq!(load_session(&dir).unwrap(), Some(session));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn undecryptable_envelope_is_deleted() {
        let dir = temp_dir("bad-dpapi");
        let path = session_path(&dir);
        std::fs::write(
            &path,
            format!(
                r#"{{"v":2,"enc":true,"data":"{}"}}"#,
                encode_base64(b"isto nao e um blob DPAPI")
            ),
        )
        .unwrap();

        assert_eq!(load_session(&dir).unwrap(), None);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn electron_v10_envelope_is_preserved_as_unsupported() {
        let dir = temp_dir("v10");
        let path = session_path(&dir);
        std::fs::write(
            &path,
            format!(
                r#"{{"v":2,"enc":true,"data":"{}"}}"#,
                encode_base64(b"v10-nonce-que-nao-e-dpapi")
            ),
        )
        .unwrap();

        let error = load_session(&dir).unwrap_err();
        assert!(error.is_unsupported(), "erro inesperado: {error}");
        assert!(path.exists(), "envelope v10 deve ser preservado");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(windows))]
    #[test]
    fn encrypted_envelope_returns_unsupported_and_keeps_file() {
        let dir = temp_dir("unsupported");
        let path = session_path(&dir);
        std::fs::write(
            &path,
            format!(
                r#"{{"v":2,"enc":true,"data":"{}"}}"#,
                encode_base64(b"ciphertext")
            ),
        )
        .unwrap();

        let error = load_session(&dir).unwrap_err();
        assert!(error.is_unsupported(), "erro inesperado: {error}");
        assert!(path.exists(), "envelope não suportado deve ser preservado");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
