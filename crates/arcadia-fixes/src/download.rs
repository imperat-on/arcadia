//! Resolução de fonte (direta/hoster/magnet) e plano de download dos fixes.
//!
//! Oráculo: `app/electron/fixes.js` (nomes dos temporários, headers do Ryuu,
//! estado `downloading`) e `app/electron/hosters.js` (formato de retorno dos
//! resolvedores: `{url, headers?}`, magnet via debrid).
//!
//! Este módulo **não** acessa a rede e **não** spawna processos. A borda
//! injeta um [`DownloadExecutor`] (curl/7zz/aria2, conforme o runtime) e um
//! [`crate::install::ArchiveExtractor`]. Os testes usam executores falsos e
//! nunca tocam em rede/disco externo.
//!
//! Progresso é exposto em [`DownloadEvent`] (campos camelCase, prontos para
//! `webContents.send`) e o polling da UI continua usando os envelopes
//! `{status, bytesRead, totalBytes}` de `plan.rs`.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::catalog::{appid_path_component, FixCandidate, FixReleaseInfo, FixType, RYUU_BASE};
use crate::install::PathRoots;

pub const DEFAULT_USER_AGENT: &str = "arcadia";
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
pub const SPEED_LIMIT_KBPS: u32 = 1024;
pub const SPEED_TIME_SECS: u32 = 45;
pub const FIX_TMP_PREFIX: &str = "fix";

/// Códigos do `fix_downloader.sh` (`badurl`, `stalled`, `authentication`).
pub const ERROR_BAD_URL: &str = "badurl";
pub const ERROR_STALLED: &str = "stalled";
pub const ERROR_AUTHENTICATION: &str = "authentication";

/// Tipo de fonte resolvida.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Direct,
    Hoster,
    Magnet,
}

/// Classifica uma URL sem resolver nada: magnet, hoster de página conhecido
/// ou download direto. `None` para vazio/esquema inválido.
pub fn classify_download_source(url: &str) -> Option<SourceKind> {
    if url.starts_with("magnet:?") {
        Some(SourceKind::Magnet)
    } else if !url.starts_with("https://") && !url.starts_with("http://") {
        None
    } else if is_known_hoster(url) {
        Some(SourceKind::Hoster)
    } else {
        Some(SourceKind::Direct)
    }
}

/// Magnet já validado (o debrid/P2P consome esta forma).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MagnetLink {
    pub uri: String,
    #[serde(rename = "infoHash")]
    pub info_hash: String,
    #[serde(rename = "displayName", skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trackers: Vec<String>,
}

impl MagnetLink {
    /// Valida `magnet:?xt=urn:btih:<hex40|base32>`.
    pub fn parse(uri: &str) -> Result<Self, DownloadError> {
        if !uri.starts_with("magnet:?") {
            return Err(DownloadError::new("not a magnet link"));
        }
        if uri.contains('\r') || uri.contains('\n') || uri.contains('\0') {
            return Err(DownloadError::new("invalid magnet link"));
        }
        let mut info_hash = None;
        let mut display_name = None;
        let mut trackers = Vec::new();
        for pair in uri["magnet:?".len()..].split('&') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            let key = percent_decode(key);
            let value = percent_decode(value);
            match key.as_str() {
                "xt" => {
                    let Some(hash) = value.strip_prefix("urn:btih:") else {
                        continue;
                    };
                    if !valid_info_hash(hash) {
                        return Err(DownloadError::new("invalid magnet info hash"));
                    }
                    info_hash = Some(normalize_info_hash(hash));
                }
                "dn" if !value.is_empty() => display_name = Some(value),
                "tr" if !value.is_empty() => trackers.push(value),
                _ => {}
            }
        }
        let Some(info_hash) = info_hash else {
            return Err(DownloadError::new("magnet link has no info hash"));
        };
        Ok(Self {
            uri: uri.to_string(),
            info_hash,
            display_name,
            trackers,
        })
    }
}

fn valid_info_hash(hash: &str) -> bool {
    if hash.len() == 40 {
        hash.chars().all(|character| character.is_ascii_hexdigit())
    } else if hash.len() == 32 {
        hash.chars()
            .all(|character| matches!(character, 'A'..='Z' | 'a'..='z' | '2'..='7'))
    } else {
        false
    }
}

fn normalize_info_hash(hash: &str) -> String {
    if hash.len() == 40 {
        hash.to_ascii_lowercase()
    } else {
        hash.to_ascii_uppercase()
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let high = (bytes[index + 1] as char).to_digit(16);
                let low = (bytes[index + 2] as char).to_digit(16);
                if let (Some(high), Some(low)) = (high, low) {
                    output.push(((high << 4) | low) as u8);
                    index += 3;
                } else {
                    output.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

/// Fonte resolvida no formato de `resolverHoster`/`resolverMagnet`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedDownloadSource {
    pub kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub magnet: Option<MagnetLink>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<(String, String)>,
    #[serde(rename = "requiresAuth")]
    pub requires_auth: bool,
}

impl ResolvedDownloadSource {
    /// Classifica uma URL de fonte de fix. Hosters conhecidos do `hosters.js`
    /// são marcados como página; qualquer outra HTTP(S) é direta.
    pub fn from_url(url: &str) -> Result<Self, DownloadError> {
        if url.is_empty() {
            return Err(DownloadError::new("empty download url"));
        }
        let Some(kind) = classify_download_source(url) else {
            return Err(DownloadError::new("unsupported download url"));
        };
        if kind == SourceKind::Magnet {
            let magnet = MagnetLink::parse(url)?;
            return Ok(Self {
                kind,
                url: None,
                magnet: Some(magnet),
                headers: Vec::new(),
                requires_auth: false,
            });
        }
        Ok(Self {
            kind,
            url: Some(url.to_string()),
            magnet: None,
            headers: Vec::new(),
            requires_auth: url.starts_with(RYUU_BASE),
        })
    }

    pub fn direct(url: impl Into<String>) -> Result<Self, DownloadError> {
        Self::from_url(&url.into())
    }

    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    pub fn with_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.headers = headers;
        self
    }
}

/// `true` para os hosters que o `hosters.js` resolve sem janela oculta.
pub fn is_known_hoster(url: &str) -> bool {
    url.contains("gofile.io/d/")
        || url.contains("pixeldra.in/u/")
        || url.contains("pixeldrain.com/u/")
        || url.contains("rootz.so/d/")
}

/// Erro de download com código opcional do worker (`badurl`, `stalled`,
/// `authentication`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadError {
    pub message: String,
    pub code: Option<String>,
}

impl DownloadError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: None,
        }
    }

    pub fn with_code(message: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: Some(code.into()),
        }
    }

    pub fn bad_url() -> Self {
        Self::with_code(
            "The download link from this source is not valid.",
            ERROR_BAD_URL,
        )
    }

    pub fn stalled() -> Self {
        Self::with_code(
            "Download stalled. Try again, or pick another source.",
            ERROR_STALLED,
        )
    }

    pub fn authentication() -> Self {
        Self::with_code(
            "Ryuu authentication was rejected or expired.",
            ERROR_AUTHENTICATION,
        )
    }
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DownloadError {}

/// Plano entregue ao executor. Os caminhos são exatamente os do worker:
/// `tmp/fix_<appid>.zip`, `tmp/fix_<appid>_state.json` e
/// `tmp/fix_<appid>_headers.txt`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadPlan {
    pub appid: String,
    #[serde(rename = "type")]
    pub fix_type: FixType,
    pub source: ResolvedDownloadSource,
    pub destination: PathBuf,
    pub state_path: PathBuf,
    pub headers_path: PathBuf,
    #[serde(rename = "expectedSize", skip_serializing_if = "Option::is_none")]
    pub expected_size: Option<u64>,
    #[serde(rename = "expectedSha256", skip_serializing_if = "Option::is_none")]
    pub expected_sha256: Option<String>,
    /// Linha pronta (`X-Auth-Key: ...\n` ou `Cookie: session=...\n`). Nunca
    /// é serializada no plano público para não vazar a credencial.
    #[serde(skip)]
    pub auth_header: Option<String>,
    pub user_agent: String,
    pub connect_timeout_secs: u64,
    pub speed_limit_kbps: u32,
    pub speed_time_secs: u32,
    pub max_time_secs: Option<u64>,
}

impl DownloadPlan {
    pub fn new(
        appid: &str,
        fix_type: FixType,
        source: ResolvedDownloadSource,
        roots: &PathRoots,
        release: Option<&FixReleaseInfo>,
    ) -> Result<Self, DownloadError> {
        let appid =
            appid_path_component(appid).ok_or_else(|| DownloadError::new("invalid appid"))?;
        let tmp = roots.data_tmp_dir();
        if !tmp.is_absolute()
            || tmp
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(DownloadError::new("invalid data root"));
        }
        let expected_size = release.and_then(|release| release.size_bytes);
        let expected_sha256 = release
            .and_then(|release| release.sha256())
            .map(str::to_string);
        Ok(Self {
            destination: tmp.join(format!("{FIX_TMP_PREFIX}_{appid}.zip")),
            state_path: tmp.join(format!("{FIX_TMP_PREFIX}_{appid}_state.json")),
            headers_path: tmp.join(format!("{FIX_TMP_PREFIX}_{appid}_headers.txt")),
            appid,
            fix_type,
            source,
            expected_size,
            expected_sha256,
            auth_header: None,
            user_agent: DEFAULT_USER_AGENT.to_string(),
            connect_timeout_secs: CONNECT_TIMEOUT.as_secs(),
            speed_limit_kbps: SPEED_LIMIT_KBPS,
            speed_time_secs: SPEED_TIME_SECS,
            max_time_secs: None,
        })
    }

    /// Constrói a partir de um candidato do catálogo.
    pub fn from_candidate(
        candidate: &FixCandidate,
        roots: &PathRoots,
    ) -> Result<Self, DownloadError> {
        let source = match candidate.magnet.as_deref() {
            Some(magnet) => ResolvedDownloadSource::from_url(magnet)?,
            None => ResolvedDownloadSource::from_url(&candidate.url)?,
        };
        Self::new(
            &candidate.appid,
            candidate.fix_type,
            source,
            roots,
            if candidate.release.is_empty() {
                None
            } else {
                Some(&candidate.release)
            },
        )
    }

    /// Injeta a credencial do Ryuu (o executor grava `headers_path` 0600).
    pub fn with_auth_header(mut self, header: impl Into<String>) -> Self {
        self.auth_header = Some(header.into());
        self
    }

    pub fn requires_auth(&self) -> bool {
        self.source.requires_auth
    }
}

/// Evento de progresso (campos camelCase; `type` é o discriminante).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DownloadEvent {
    Downloading {
        #[serde(rename = "bytesRead")]
        bytes_read: u64,
        #[serde(rename = "totalBytes")]
        total_bytes: u64,
    },
    Extracting,
    Done,
    Failed {
        error: String,
        #[serde(rename = "errorCode", default, skip_serializing_if = "Option::is_none")]
        error_code: Option<String>,
    },
}

impl DownloadEvent {
    pub fn downloading(bytes_read: u64, total_bytes: u64) -> Self {
        Self::Downloading {
            bytes_read,
            total_bytes,
        }
    }

    pub fn failed(error: impl Into<String>, error_code: Option<String>) -> Self {
        Self::Failed {
            error: error.into(),
            error_code,
        }
    }

    /// Status equivalente no envelope de polling (`fixes:status`).
    pub fn status(&self) -> &'static str {
        match self {
            DownloadEvent::Downloading { .. } => "downloading",
            DownloadEvent::Extracting => "extracting",
            DownloadEvent::Done => "extracted",
            DownloadEvent::Failed { .. } => "failed",
        }
    }
}

/// Arquivo baixado; o hash é calculado pela borda no mesmo stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadedArchive {
    pub path: PathBuf,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

impl DownloadedArchive {
    pub fn new(path: impl Into<PathBuf>, bytes: u64, sha256: Option<String>) -> Self {
        Self {
            path: path.into(),
            bytes,
            sha256,
        }
    }

    /// Confere tamanho/hash publicados no catálogo (quando presentes).
    pub fn verify(&self, plan: &DownloadPlan) -> Result<(), DownloadError> {
        if let Some(expected) = plan.expected_size {
            if self.bytes != expected {
                return Err(DownloadError::new(
                    "Downloaded archive size does not match the catalog metadata.",
                ));
            }
        }
        if let Some(expected) = &plan.expected_sha256 {
            let Some(actual) = &self.sha256 else {
                return Err(DownloadError::new("Downloaded archive hash is missing."));
            };
            if !actual.trim().eq_ignore_ascii_case(expected.trim()) {
                return Err(DownloadError::new(
                    "Downloaded archive hash does not match the catalog metadata.",
                ));
            }
        }
        Ok(())
    }
}

/// Resolvedor injetado de páginas de hoster/magnet (produção: o cliente de
/// debrid de `arcadia-catalog`; testes: falso). O plano só carrega a forma;
/// nenhuma resolução acontece neste crate.
pub trait SourceResolver: Send + Sync {
    fn resolve(
        &self,
        source: &ResolvedDownloadSource,
    ) -> Result<ResolvedDownloadSource, DownloadError>;
}

/// Resolvedor identidade para fontes já diretas (e testes do plano).
#[derive(Debug, Clone, Copy, Default)]
pub struct IdentitySourceResolver;

impl SourceResolver for IdentitySourceResolver {
    fn resolve(
        &self,
        source: &ResolvedDownloadSource,
    ) -> Result<ResolvedDownloadSource, DownloadError> {
        Ok(source.clone())
    }
}

/// Requisição HTTP mínima para a borda resolver páginas de hoster (o
/// `fetchRede` do cliente congelado). Nenhuma chamada real acontece aqui.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub timeout: Duration,
}

impl FetchRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: Vec::new(),
            timeout: CONNECT_TIMEOUT,
        }
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

/// Resposta HTTP já lida por inteiro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl FetchResponse {
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    pub fn json(status: u16, value: &serde_json::Value) -> Self {
        let mut response = Self::new(status, serde_json::to_vec(value).unwrap_or_default());
        response
            .headers
            .push(("content-type".into(), "application/json".into()));
        response
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn json_value(&self) -> Result<serde_json::Value, DownloadError> {
        serde_json::from_slice(&self.body).map_err(|_| DownloadError::new("invalid JSON response"))
    }
}

/// Fetcher HTTP injetado para resolvers de hoster (produção: `fetchRede`;
/// testes: duplo local). O crate não faz rede.
pub trait DownloadFetcher: Send + Sync {
    fn fetch(&self, request: &FetchRequest) -> Result<FetchResponse, DownloadError>;
}

/// Executor injetado (produção: curl/aria2/7zz; testes: falso).
pub trait DownloadExecutor: Send + Sync {
    fn download(
        &self,
        plan: &DownloadPlan,
        progress: &mut dyn FnMut(DownloadEvent),
    ) -> Result<DownloadedArchive, DownloadError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::FixCatalog;
    use serde_json::json;

    fn roots(dir: &std::path::Path) -> PathRoots {
        PathRoots::new(
            vec![dir.join("game")],
            Vec::new(),
            dir.join("backups"),
            dir.join("data"),
        )
    }

    fn candidate() -> FixCandidate {
        let catalog = FixCatalog::from_values(Some(&json!({"genericFixes": [730]})), None);
        catalog.candidate("730", FixType::Generic).unwrap()
    }

    #[test]
    fn classifies_direct_hoster_and_magnet_sources() {
        assert_eq!(
            classify_download_source("https://files.luatools.work/GameBypasses/1.zip"),
            Some(SourceKind::Direct)
        );
        assert_eq!(
            classify_download_source("https://gofile.io/d/abc"),
            Some(SourceKind::Hoster)
        );
        assert_eq!(
            classify_download_source("https://pixeldrain.com/u/abc"),
            Some(SourceKind::Hoster)
        );
        assert_eq!(
            classify_download_source("https://rootz.so/d/abc"),
            Some(SourceKind::Hoster)
        );
        assert_eq!(
            classify_download_source(
                "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567"
            ),
            Some(SourceKind::Magnet)
        );
        assert_eq!(classify_download_source(""), None);
        assert!(ResolvedDownloadSource::from_url("ftp://x").is_err());
    }

    #[test]
    fn parses_magnet_hex_and_base32_and_metadata() {
        let hex = "0123456789abcdef0123456789abcdef01234567";
        let magnet = MagnetLink::parse(&format!(
            "magnet:?xt=urn:btih:{hex}&dn=My%20Game&tr=udp%3A%2F%2Ftracker.test%3A80"
        ))
        .unwrap();
        assert_eq!(magnet.info_hash, hex);
        assert_eq!(magnet.display_name.as_deref(), Some("My Game"));
        assert_eq!(magnet.trackers, vec!["udp://tracker.test:80"]);

        let mut base32 = String::new();
        for _ in 0..32 {
            base32.push('a');
        }
        let magnet = MagnetLink::parse(&format!("magnet:?xt=urn:btih:{base32}")).unwrap();
        assert_eq!(magnet.info_hash, base32.to_ascii_uppercase());
    }

    #[test]
    fn rejects_malformed_magnets() {
        assert!(MagnetLink::parse("magnet:?dn=x").is_err());
        assert!(MagnetLink::parse("magnet:?xt=urn:sha1:abc").is_err());
        assert!(MagnetLink::parse("magnet:?xt=urn:btih:xyz").is_err());
        assert!(MagnetLink::parse("https://example.test/x").is_err());
        assert!(MagnetLink::parse(
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567\n"
        )
        .is_err());
    }

    #[test]
    fn download_plan_uses_worker_temp_names_and_release_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let mut candidate = candidate();
        candidate.release = FixReleaseInfo {
            size_bytes: Some(1234),
            hashes: vec![crate::catalog::FileHash::new("sha256", "ABCD")],
            version: Some("1".into()),
            released: None,
        };
        let plan = DownloadPlan::from_candidate(&candidate, &roots(temp.path())).unwrap();
        let tmp = temp.path().join("data/tmp");
        assert_eq!(plan.destination, tmp.join("fix_730.zip"));
        assert_eq!(plan.state_path, tmp.join("fix_730_state.json"));
        assert_eq!(plan.headers_path, tmp.join("fix_730_headers.txt"));
        assert_eq!(plan.expected_size, Some(1234));
        assert_eq!(plan.expected_sha256.as_deref(), Some("ABCD"));
        assert_eq!(plan.user_agent, "arcadia");
        assert_eq!(plan.speed_limit_kbps, 1024);
        assert_eq!(plan.connect_timeout_secs, 8);
        assert!(!plan.requires_auth());
    }

    #[test]
    fn ryuu_auth_header_enters_plan_but_not_serialization() {
        let temp = tempfile::tempdir().unwrap();
        let mut candidate = candidate();
        candidate.url = "https://generator.ryuu.lol/fixes/730.zip".into();
        candidate.requires_auth = true;
        let plan = DownloadPlan::from_candidate(&candidate, &roots(temp.path()))
            .unwrap()
            .with_auth_header("X-Auth-Key: secret\n");
        assert!(plan.requires_auth());
        assert_eq!(plan.auth_header.as_deref(), Some("X-Auth-Key: secret\n"));
        let json = serde_json::to_string(&plan).unwrap();
        assert!(!json.contains("secret"));
    }

    #[test]
    fn invalid_appid_cannot_escape_temp_directory() {
        let temp = tempfile::tempdir().unwrap();
        let source = ResolvedDownloadSource::from_url("https://x.test/fix.zip").unwrap();
        assert!(DownloadPlan::new(
            "../evil",
            FixType::Generic,
            source,
            &roots(temp.path()),
            None
        )
        .is_err());
    }

    #[test]
    fn progress_events_have_ui_field_names() {
        assert_eq!(
            serde_json::to_value(DownloadEvent::downloading(10, 20)).unwrap(),
            json!({"type": "downloading", "bytesRead": 10, "totalBytes": 20})
        );
        assert_eq!(
            serde_json::to_value(DownloadEvent::Extracting).unwrap(),
            json!({"type": "extracting"})
        );
        assert_eq!(
            serde_json::to_value(DownloadEvent::Done).unwrap(),
            json!({"type": "done"})
        );
        assert_eq!(
            serde_json::to_value(DownloadEvent::failed("x", Some("badurl".into()))).unwrap(),
            json!({"type": "failed", "error": "x", "errorCode": "badurl"})
        );
        assert_eq!(DownloadEvent::Done.status(), "extracted");
    }

    #[test]
    fn metadata_verification_checks_size_and_sha256() {
        let temp = tempfile::tempdir().unwrap();
        let mut candidate = candidate();
        candidate.release.size_bytes = Some(10);
        candidate.release.hashes = vec![crate::catalog::FileHash::new("sha256", "aabb")];
        let plan = DownloadPlan::from_candidate(&candidate, &roots(temp.path())).unwrap();

        let ok = DownloadedArchive::new(temp.path().join("f.zip"), 10, Some("AABB".into()));
        assert!(ok.verify(&plan).is_ok());
        let wrong_size = DownloadedArchive::new(temp.path().join("f.zip"), 9, Some("aabb".into()));
        assert!(wrong_size.verify(&plan).is_err());
        let missing_hash = DownloadedArchive::new(temp.path().join("f.zip"), 10, None);
        assert!(missing_hash.verify(&plan).is_err());
        let wrong_hash = DownloadedArchive::new(temp.path().join("f.zip"), 10, Some("ccdd".into()));
        assert!(wrong_hash.verify(&plan).is_err());
    }

    #[test]
    fn download_error_codes_match_worker() {
        assert_eq!(DownloadError::bad_url().code.as_deref(), Some("badurl"));
        assert_eq!(DownloadError::stalled().code.as_deref(), Some("stalled"));
        assert_eq!(
            DownloadError::authentication().code.as_deref(),
            Some("authentication")
        );
    }

    struct FakeExecutor;

    impl DownloadExecutor for FakeExecutor {
        fn download(
            &self,
            plan: &DownloadPlan,
            progress: &mut dyn FnMut(DownloadEvent),
        ) -> Result<DownloadedArchive, DownloadError> {
            progress(DownloadEvent::downloading(
                0,
                plan.expected_size.unwrap_or(0),
            ));
            progress(DownloadEvent::downloading(
                plan.expected_size.unwrap_or(0),
                plan.expected_size.unwrap_or(0),
            ));
            Ok(DownloadedArchive::new(
                plan.destination.clone(),
                plan.expected_size.unwrap_or(0),
                plan.expected_sha256.clone(),
            ))
        }
    }

    #[test]
    fn executor_trait_can_be_injected_without_network() {
        let temp = tempfile::tempdir().unwrap();
        let mut candidate = candidate();
        candidate.release.size_bytes = Some(5);
        let plan = DownloadPlan::from_candidate(&candidate, &roots(temp.path())).unwrap();
        let mut events = Vec::new();
        let archive = FakeExecutor
            .download(&plan, &mut |event| events.push(event.clone()))
            .unwrap();
        assert_eq!(events.len(), 2);
        assert!(archive.verify(&plan).is_ok());
    }

    #[test]
    fn resolver_trait_resolves_sources_without_network() {
        let source = ResolvedDownloadSource::from_url("https://gofile.io/d/abc").unwrap();
        let resolved = IdentitySourceResolver.resolve(&source).unwrap();
        assert_eq!(resolved, source);
        assert_eq!(resolved.kind, SourceKind::Hoster);
    }

    struct FakeFetcher;

    impl DownloadFetcher for FakeFetcher {
        fn fetch(&self, request: &FetchRequest) -> Result<FetchResponse, DownloadError> {
            assert_eq!(request.url, "https://example.test/api");
            Ok(FetchResponse::json(
                200,
                &json!({"url": "https://cdn.test/f.zip"}),
            ))
        }
    }

    #[test]
    fn fetcher_trait_can_be_injected_without_network() {
        let response = FakeFetcher
            .fetch(&FetchRequest::get("https://example.test/api"))
            .unwrap();
        assert!(response.is_success());
        assert_eq!(
            response.json_value().unwrap()["url"],
            json!("https://cdn.test/f.zip")
        );
        assert_eq!(FetchResponse::new(200, b"x".to_vec()).text(), "x");
        assert!(!FetchResponse::new(404, Vec::new()).is_success());
    }

    #[test]
    fn candidate_magnet_becomes_magnet_download_plan() {
        let temp = tempfile::tempdir().unwrap();
        let hash = "0123456789abcdef0123456789abcdef01234567";
        let mut candidate = candidate();
        candidate.magnet = Some(format!("magnet:?xt=urn:btih:{hash}&dn=Game"));
        let plan = DownloadPlan::from_candidate(&candidate, &roots(temp.path())).unwrap();
        assert_eq!(plan.source.kind, SourceKind::Magnet);
        let magnet = plan.source.magnet.as_ref().unwrap();
        assert_eq!(magnet.info_hash, hash);
        assert_eq!(magnet.display_name.as_deref(), Some("Game"));
    }
}
