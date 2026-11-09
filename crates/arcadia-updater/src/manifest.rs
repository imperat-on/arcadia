//! Manifestos do canal empacotado (`latest.yml`, `latest-linux.yml` e
//! `app-update.yml`).
//!
//! O cliente Electron delega o parse ao `electron-updater`; aqui ele é feito
//! sem dependência de YAML completo, porque o electron-builder emite um
//! subconjunto estável (escalares e a lista `files:`). Campos desconhecidos de
//! `app-update.yml` são preservados em [`AppUpdateConfig::extra`].
//!
//! Erros seguem o envelope `{ "ok": false, "erro": "<código>", "detalhe":
//! "<contexto>" }` (o `detalhe` só aparece quando há contexto).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::version::{is_newer_version, parse_semver};

pub const LATEST_WINDOWS: &str = "latest.yml";
pub const LATEST_LINUX: &str = "latest-linux.yml";
pub const APP_UPDATE: &str = "app-update.yml";

/// Um arquivo oferecido pelo manifesto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateFile {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha512: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Tamanho do `.blockmap` (download diferencial), quando publicado.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_map_size: Option<u64>,
}

impl UpdateFile {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            sha512: None,
            size: None,
            block_map_size: None,
        }
    }

    pub fn with_sha512(mut self, sha512: impl Into<String>) -> Self {
        self.sha512 = Some(sha512.into());
        self
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.size = Some(size);
        self
    }

    pub fn with_block_map_size(mut self, size: u64) -> Self {
        self.block_map_size = Some(size);
        self
    }

    /// `tamanhoDe(info)`: soma só tamanhos finitos e positivos.
    pub fn from_value(value: &Value) -> Self {
        let url = crate::js::string_or_empty(value.get("url"));
        let sha512 = value
            .get("sha512")
            .filter(|value| crate::js::truthy(value))
            .map(crate::js::string);
        let size = value
            .get("size")
            .and_then(crate::js::number)
            .filter(|number| number.is_finite() && *number > 0.0)
            .map(|number| number as u64);
        let block_map_size = value
            .get("blockMapSize")
            .and_then(crate::js::number)
            .filter(|number| number.is_finite() && *number > 0.0)
            .map(|number| number as u64);
        Self {
            url,
            sha512,
            size,
            block_map_size,
        }
    }
}

/// `tamanhoDe(info)`: total dos `files[].size`, ou `None` se nada for positivo.
pub fn size_total(files: &[UpdateFile]) -> Option<u64> {
    let mut total = 0_u64;
    for file in files {
        if let Some(size) = file.size {
            if size > 0 {
                total = total.saturating_add(size);
            }
        }
    }
    if total > 0 {
        Some(total)
    } else {
        None
    }
}

/// Conteúdo de `latest.yml` (Windows/NSIS) ou `latest-linux.yml` (AppImage).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestManifest {
    pub version: String,
    pub path: String,
    pub sha512: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_map_size: Option<u64>,
    pub files: Vec<UpdateFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
}

impl LatestManifest {
    /// Mesma regra do `checar` empacotado.
    pub fn is_newer_than(&self, current: &str) -> bool {
        is_newer_version(&self.version, current)
    }

    pub fn total_size(&self) -> Option<u64> {
        size_total(&self.files).or(self.size)
    }
}

/// Conteúdo de `app-update.yml` (configuração de publish embutida no pacote).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateConfig {
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updater_cache_dir_name: Option<String>,
    /// Campos desconhecidos, preservados como texto YAML já desescapado.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, String>,
}

/// Manifesto reconhecido pelo nome do arquivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Manifest {
    Latest(LatestManifest),
    AppUpdate(AppUpdateConfig),
}

/// Erros de parse/validação dos manifestos.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    #[error("manifesto_vazio")]
    Empty,
    #[error("yaml_invalido: {0}")]
    InvalidYaml(String),
    #[error("versao_ausente")]
    MissingVersion,
    #[error("versao_invalida: {0}")]
    InvalidVersion(String),
    #[error("caminho_ausente")]
    MissingPath,
    #[error("sha512_ausente")]
    MissingSha512,
    #[error("tamanho_invalido: {0}")]
    InvalidSize(String),
    #[error("blockmap_invalido: {0}")]
    InvalidBlockMap(String),
    #[error("provider_ausente")]
    MissingProvider,
    #[error("config_incompleta: {0}")]
    IncompleteConfig(String),
}

impl ManifestError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Empty => "manifesto_vazio",
            Self::InvalidYaml(_) => "yaml_invalido",
            Self::MissingVersion => "versao_ausente",
            Self::InvalidVersion(_) => "versao_invalida",
            Self::MissingPath => "caminho_ausente",
            Self::MissingSha512 => "sha512_ausente",
            Self::InvalidSize(_) => "tamanho_invalido",
            Self::InvalidBlockMap(_) => "blockmap_invalido",
            Self::MissingProvider => "provider_ausente",
            Self::IncompleteConfig(_) => "config_incompleta",
        }
    }

    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::InvalidYaml(detail)
            | Self::InvalidVersion(detail)
            | Self::InvalidSize(detail)
            | Self::InvalidBlockMap(detail)
            | Self::IncompleteConfig(detail) => Some(detail),
            _ => None,
        }
    }

    /// Envelope de erro pronto para IPC.
    pub fn to_value(&self) -> Value {
        let mut map = serde_json::Map::new();
        map.insert("ok".into(), Value::Bool(false));
        map.insert("erro".into(), Value::String(self.code().to_string()));
        if let Some(detail) = self.detail() {
            map.insert("detalhe".into(), Value::String(detail.to_string()));
        }
        Value::Object(map)
    }
}

impl Serialize for ManifestError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_value().serialize(serializer)
    }
}

/// Parse do `latest.yml`/`latest-linux.yml`.
///
/// Validação: `version` semver obrigatória; pelo menos um arquivo (a lista
/// `files:` ou `path`/`sha512` no topo); `url` e `sha512` por arquivo; `size` e
/// `blockMapSize`, quando presentes, precisam ser inteiros positivos.
pub fn parse_latest(contents: &str) -> Result<LatestManifest, ManifestError> {
    let parsed = parse_yaml(contents)?;

    let version = parsed
        .top
        .get("version")
        .map(|version| version.trim())
        .filter(|version| !version.is_empty())
        .ok_or(ManifestError::MissingVersion)?;
    parse_semver(version).map_err(|_| ManifestError::InvalidVersion(version.to_string()))?;

    let top_sha = parsed
        .top
        .get("sha512")
        .map(|sha| sha.trim())
        .filter(|sha| !sha.is_empty());
    let top_path = parsed
        .top
        .get("path")
        .map(|path| path.trim())
        .filter(|path| !path.is_empty());
    let top_size = parse_optional_size(&parsed.top, &["size"], SizeKind::Size)?;
    let top_block_map = parse_optional_size(
        &parsed.top,
        &["blockMapSize", "blockmapSize", "blockmap"],
        SizeKind::BlockMap,
    )?;

    let mut files = Vec::new();
    for entry in &parsed.files {
        let url = entry
            .get("url")
            .map(|url| url.trim())
            .filter(|url| !url.is_empty())
            .ok_or(ManifestError::MissingPath)?;
        let sha512 = entry
            .get("sha512")
            .map(|sha| sha.trim())
            .filter(|sha| !sha.is_empty())
            .or(top_sha)
            .ok_or(ManifestError::MissingSha512)?;
        let size = parse_optional_size(entry, &["size"], SizeKind::Size)?;
        let block_map_size = parse_optional_size(
            entry,
            &["blockMapSize", "blockmapSize", "blockmap"],
            SizeKind::BlockMap,
        )?;
        files.push(UpdateFile {
            url: url.to_string(),
            sha512: Some(sha512.to_string()),
            size,
            block_map_size,
        });
    }

    if files.is_empty() {
        let path = top_path.ok_or(ManifestError::MissingPath)?;
        let sha512 = top_sha.ok_or(ManifestError::MissingSha512)?;
        files.push(UpdateFile {
            url: path.to_string(),
            sha512: Some(sha512.to_string()),
            size: top_size,
            block_map_size: top_block_map,
        });
    }

    let path = top_path
        .map(str::to_string)
        .unwrap_or_else(|| files[0].url.clone());
    let sha512 = top_sha
        .map(str::to_string)
        .or_else(|| files[0].sha512.clone())
        .ok_or(ManifestError::MissingSha512)?;

    // Consistência dos campos "legados" do topo com a lista `files`, quando os
    // dois aparecem. Um `size`/`blockMapSize` só no topo é aceito e propagado.
    for file in &mut files {
        if file.url == path {
            if file.size.is_none() {
                file.size = top_size;
            }
            if file.block_map_size.is_none() {
                file.block_map_size = top_block_map;
            }
        }
    }
    let size = top_size.or_else(|| size_for(&files, &path));
    let block_map_size = top_block_map.or_else(|| block_map_for(&files, &path));

    Ok(LatestManifest {
        version: version.to_string(),
        path,
        sha512,
        size,
        block_map_size,
        files,
        release_date: parsed.top.get("releaseDate").cloned(),
    })
}

fn size_for(files: &[UpdateFile], path: &str) -> Option<u64> {
    files
        .iter()
        .find(|file| file.url == path)
        .and_then(|file| file.size)
}

fn block_map_for(files: &[UpdateFile], path: &str) -> Option<u64> {
    files
        .iter()
        .find(|file| file.url == path)
        .and_then(|file| file.block_map_size)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SizeKind {
    Size,
    BlockMap,
}

fn parse_optional_size(
    map: &BTreeMap<String, String>,
    keys: &[&str],
    kind: SizeKind,
) -> Result<Option<u64>, ManifestError> {
    for key in keys {
        let Some(raw) = map.get(*key) else {
            continue;
        };
        let raw = raw.trim();
        let parsed = raw.parse::<u64>().ok().filter(|size| *size > 0);
        return match parsed {
            Some(size) => Ok(Some(size)),
            None => Err(match kind {
                SizeKind::Size => ManifestError::InvalidSize(raw.to_string()),
                SizeKind::BlockMap => ManifestError::InvalidBlockMap(raw.to_string()),
            }),
        };
    }
    Ok(None)
}

/// Parse do `app-update.yml`.
pub fn parse_app_update(contents: &str) -> Result<AppUpdateConfig, ManifestError> {
    let parsed = parse_yaml(contents)?;
    let provider = parsed
        .top
        .get("provider")
        .map(|provider| provider.trim())
        .filter(|provider| !provider.is_empty())
        .ok_or(ManifestError::MissingProvider)?
        .to_string();

    let owner = non_empty(parsed.top.get("owner"));
    let repo = non_empty(parsed.top.get("repo"));
    let url = non_empty(parsed.top.get("url"));
    let channel = non_empty(parsed.top.get("channel"));
    let updater_cache_dir_name = non_empty(parsed.top.get("updaterCacheDirName"));

    match provider.as_str() {
        "github" => {
            if owner.is_none() || repo.is_none() {
                return Err(ManifestError::IncompleteConfig(
                    "github exige owner e repo".into(),
                ));
            }
        }
        "generic" if url.is_none() => {
            return Err(ManifestError::IncompleteConfig("generic exige url".into()));
        }
        _ => {}
    }

    let known = [
        "provider",
        "owner",
        "repo",
        "url",
        "channel",
        "updaterCacheDirName",
    ];
    let mut extra = BTreeMap::new();
    for (key, value) in &parsed.top {
        if !known.contains(&key.as_str()) {
            extra.insert(key.clone(), value.clone());
        }
    }

    Ok(AppUpdateConfig {
        provider,
        owner,
        repo,
        url,
        channel,
        updater_cache_dir_name,
        extra,
    })
}

fn non_empty(value: Option<&String>) -> Option<String> {
    value
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Escolhe o parser pelo nome do arquivo (`latest*.yml` × `app-update.yml`).
pub fn parse_manifest(filename: &str, contents: &str) -> Result<Manifest, ManifestError> {
    let name = filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(filename)
        .to_ascii_lowercase();
    if name == APP_UPDATE || name.ends_with("app-update.yml") {
        parse_app_update(contents).map(Manifest::AppUpdate)
    } else if name.contains("latest") {
        parse_latest(contents).map(Manifest::Latest)
    } else {
        Err(ManifestError::InvalidYaml(format!(
            "manifesto desconhecido: {filename}"
        )))
    }
}

/// Envelope `{ok:true, manifesto:...}` ou o envelope de erro.
pub fn latest_envelope(contents: &str) -> Value {
    match parse_latest(contents) {
        Ok(manifest) => json!({ "ok": true, "manifesto": manifest }),
        Err(error) => error.to_value(),
    }
}

pub fn app_update_envelope(contents: &str) -> Value {
    match parse_app_update(contents) {
        Ok(manifest) => json!({ "ok": true, "manifesto": manifest }),
        Err(error) => error.to_value(),
    }
}

// ── Parser YAML mínimo ─────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct ParsedYaml {
    top: BTreeMap<String, String>,
    files: Vec<BTreeMap<String, String>>,
}

fn parse_yaml(contents: &str) -> Result<ParsedYaml, ManifestError> {
    let contents = contents.strip_prefix('\u{feff}').unwrap_or(contents);
    if contents.trim().is_empty() {
        return Err(ManifestError::Empty);
    }

    let mut parsed = ParsedYaml::default();
    let mut section: Option<String> = None;
    let mut current_file: Option<BTreeMap<String, String>> = None;

    for (index, raw) in contents.lines().enumerate() {
        let line_number = index + 1;
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed == "---" || trimmed == "..." {
            continue;
        }

        let indent = line.len() - trimmed.len();

        if let Some(rest) = trimmed.strip_prefix('-') {
            let rest = rest.trim_start();
            if section.as_deref() == Some("files") {
                if let Some(file) = current_file.take() {
                    parsed.files.push(file);
                }
                let mut file = BTreeMap::new();
                if !rest.is_empty() {
                    let (key, value) = split_entry(rest, line_number)?;
                    if let Some(value) = value {
                        file.insert(key, value);
                    }
                }
                current_file = Some(file);
            }
            continue;
        }

        let (key, value) = split_entry(trimmed, line_number)?;
        if indent == 0 {
            if let Some(file) = current_file.take() {
                parsed.files.push(file);
            }
            match value {
                Some(value) => {
                    if key == "files" && value.trim() == "[]" {
                        section = Some("files".into());
                    } else {
                        parsed.top.insert(key, value);
                        section = None;
                    }
                }
                None => section = Some(key),
            }
        } else if section.as_deref() == Some("files") {
            if let Some(file) = current_file.as_mut() {
                if let Some(value) = value {
                    file.insert(key, value);
                }
            }
        }
    }
    if let Some(file) = current_file.take() {
        parsed.files.push(file);
    }
    Ok(parsed)
}

fn split_entry(line: &str, line_number: usize) -> Result<(String, Option<String>), ManifestError> {
    let Some(colon) = line.find(':') else {
        return Err(ManifestError::InvalidYaml(format!("linha {line_number}")));
    };
    let key = line[..colon].trim();
    if key.is_empty() {
        return Err(ManifestError::InvalidYaml(format!("linha {line_number}")));
    }
    Ok((key.to_string(), parse_scalar(&line[colon + 1..])))
}

fn parse_scalar(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    if value.starts_with('\'') {
        return Some(unquote_single(value));
    }
    if value.starts_with('"') {
        return Some(unquote_double(value));
    }
    let mut cut = value.len();
    for (index, character) in value.char_indices() {
        if character == '#'
            && (index == 0
                || value[..index]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_whitespace))
        {
            cut = index;
            break;
        }
    }
    let value = value[..cut].trim();
    if value.is_empty() || matches!(value, "~" | "null" | "Null" | "NULL") {
        return None;
    }
    Some(value.to_string())
}

fn unquote_single(value: &str) -> String {
    let inner = value
        .strip_prefix('\'')
        .and_then(|value| value.rsplit_once('\'').map(|(inner, _)| inner))
        .unwrap_or(value);
    inner.replace("''", "'")
}

fn unquote_double(value: &str) -> String {
    let inner = value
        .strip_prefix('"')
        .and_then(|value| value.rsplit_once('"').map(|(inner, _)| inner))
        .unwrap_or(value);
    let mut output = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        match chars.next() {
            Some('n') => output.push('\n'),
            Some('r') => output.push('\r'),
            Some('t') => output.push('\t'),
            Some(other) => output.push(other),
            None => output.push('\\'),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    const LATEST: &str = include_str!("../tests/fixtures/latest.yml");
    const LATEST_LINUX: &str = include_str!("../tests/fixtures/latest-linux.yml");
    const APP_UPDATE: &str = include_str!("../tests/fixtures/app-update.yml");

    #[test]
    fn parses_windows_latest() {
        let manifest = parse_latest(LATEST).expect("latest.yml");
        assert_eq!(manifest.version, "1.4.2");
        assert_eq!(manifest.path, "Arcadia-Setup-1.4.2-x64.exe");
        assert_eq!(manifest.sha512, format!("{}==", "A".repeat(86)));
        assert_eq!(manifest.size, Some(104_857_600));
        assert_eq!(manifest.block_map_size, None);
        assert_eq!(
            manifest.release_date.as_deref(),
            Some("2026-09-21T12:00:00.000Z")
        );
        assert_eq!(manifest.files.len(), 1);
        assert_eq!(manifest.files[0].url, manifest.path);
        assert!(manifest.is_newer_than("1.4.1"));
        assert!(!manifest.is_newer_than("1.4.2"));
        assert_eq!(manifest.total_size(), Some(104_857_600));
    }

    #[test]
    fn parses_linux_latest_with_blockmap() {
        let manifest = parse_latest(LATEST_LINUX).expect("latest-linux.yml");
        assert_eq!(manifest.version, "1.4.2");
        assert_eq!(manifest.path, "Arcadia-1.4.2-x86_64.AppImage");
        assert_eq!(manifest.size, Some(209_715_200));
        assert_eq!(manifest.block_map_size, Some(65_536));
        assert_eq!(manifest.files[0].block_map_size, Some(65_536));
    }

    #[test]
    fn falls_back_to_top_level_path_and_sha() {
        let contents = "version: 2.0.0\npath: Arcadia.AppImage\nsha512: abcdef\nsize: 42\n";
        let manifest = parse_latest(contents).expect("manifest");
        assert_eq!(manifest.files.len(), 1);
        assert_eq!(manifest.files[0].url, "Arcadia.AppImage");
        assert_eq!(manifest.files[0].sha512.as_deref(), Some("abcdef"));
        assert_eq!(manifest.size, Some(42));
    }

    #[test]
    fn derives_path_from_files_and_propagates_top_size() {
        let contents = "\
version: 2.0.0
files:
  - url: Arcadia-setup.exe
    sha512: abcdef
    size: 100
size: 100
";
        let manifest = parse_latest(contents).expect("manifest");
        assert_eq!(manifest.path, "Arcadia-setup.exe");
        assert_eq!(manifest.sha512, "abcdef");
        assert_eq!(manifest.size, Some(100));
    }

    #[test]
    fn handles_quotes_comments_bom_and_crlf() {
        let contents = "\u{feff}---\r\n# comentário\r\nversion: '1.4.2' # versão\r\npath: \"Arcadia.exe\"\r\nsha512: 'abc#def'\r\n";
        let manifest = parse_latest(contents).expect("manifest");
        assert_eq!(manifest.version, "1.4.2");
        assert_eq!(manifest.path, "Arcadia.exe");
        assert_eq!(manifest.sha512, "abc#def");
    }

    #[test]
    fn rejects_empty_manifest() {
        assert_eq!(parse_latest(" \n"), Err(ManifestError::Empty));
    }

    #[test]
    fn rejects_malformed_yaml_with_line() {
        let error = parse_latest("version: 1.0.0\nisto nao tem dois pontos\n").unwrap_err();
        assert_eq!(error.code(), "yaml_invalido");
        assert_eq!(error.detail(), Some("linha 2"));
    }

    #[test]
    fn rejects_missing_or_invalid_version() {
        assert_eq!(
            parse_latest("path: x\nsha512: y\n").unwrap_err(),
            ManifestError::MissingVersion
        );
        let error = parse_latest("version: um.dois\npath: x\nsha512: y\n").unwrap_err();
        assert_eq!(error.code(), "versao_invalida");
        assert_eq!(error.detail(), Some("um.dois"));
    }

    #[test]
    fn rejects_missing_path_or_sha() {
        assert_eq!(
            parse_latest("version: 1.0.0\nsha512: y\n").unwrap_err(),
            ManifestError::MissingPath
        );
        assert_eq!(
            parse_latest("version: 1.0.0\npath: x\n").unwrap_err(),
            ManifestError::MissingSha512
        );
        assert_eq!(
            parse_latest("version: 1.0.0\nfiles:\n  - sha512: y\n").unwrap_err(),
            ManifestError::MissingPath
        );
        assert_eq!(
            parse_latest("version: 1.0.0\nfiles:\n  - url: x\n").unwrap_err(),
            ManifestError::MissingSha512
        );
    }

    #[test]
    fn rejects_invalid_size_and_blockmap() {
        let error = parse_latest("version: 1.0.0\npath: x\nsha512: y\nsize: banana\n").unwrap_err();
        assert_eq!(error.code(), "tamanho_invalido");
        let error = parse_latest("version: 1.0.0\npath: x\nsha512: y\nsize: 0\n").unwrap_err();
        assert_eq!(error.code(), "tamanho_invalido");
        let error = parse_latest(
            "version: 1.0.0\nfiles:\n  - url: x\n    sha512: y\n    blockMapSize: -1\n",
        )
        .unwrap_err();
        assert_eq!(error.code(), "blockmap_invalido");
    }

    #[test]
    fn sums_file_sizes_ignoring_missing() {
        let files = vec![
            UpdateFile::new("a").with_size(10),
            UpdateFile::new("b"),
            UpdateFile::new("c").with_size(32),
        ];
        assert_eq!(size_total(&files), Some(42));
        assert_eq!(size_total(&[UpdateFile::new("d")]), None);
    }

    #[test]
    fn parses_app_update_github() {
        let config = parse_app_update(APP_UPDATE).expect("app-update.yml");
        assert_eq!(config.provider, "github");
        assert_eq!(config.owner.as_deref(), Some("imperat-on"));
        assert_eq!(config.repo.as_deref(), Some("arcadia"));
        assert_eq!(
            config.updater_cache_dir_name.as_deref(),
            Some("arcadia-updater")
        );
        assert_eq!(
            config.extra.get("vPrefixedTagName").map(String::as_str),
            Some("false")
        );
    }

    #[test]
    fn parses_generic_provider_and_rejects_incomplete() {
        let config = parse_app_update("provider: generic\nurl: https://example.com/arcadia\n")
            .expect("generic");
        assert_eq!(config.url.as_deref(), Some("https://example.com/arcadia"));

        assert_eq!(
            parse_app_update("owner: x\n").unwrap_err(),
            ManifestError::MissingProvider
        );
        let error = parse_app_update("provider: github\nowner: x\n").unwrap_err();
        assert_eq!(error.code(), "config_incompleta");
        let error = parse_app_update("provider: generic\n").unwrap_err();
        assert_eq!(error.code(), "config_incompleta");
    }

    #[test]
    fn dispatch_uses_filename() {
        assert!(matches!(
            parse_manifest("C:/release/latest-linux.yml", LATEST_LINUX),
            Ok(Manifest::Latest(_))
        ));
        assert!(matches!(
            parse_manifest("app-update.yml", APP_UPDATE),
            Ok(Manifest::AppUpdate(_))
        ));
        let error = parse_manifest("other.yml", LATEST).unwrap_err();
        assert_eq!(error.code(), "yaml_invalido");
    }

    #[test]
    fn envelopes_match_contract_shape() {
        let ok = latest_envelope(LATEST);
        assert_eq!(ok["ok"], json!(true));
        assert_eq!(ok["manifesto"]["version"], json!("1.4.2"));

        let error = latest_envelope("");
        assert_eq!(error, json!({ "ok": false, "erro": "manifesto_vazio" }));

        let error = app_update_envelope("provider: github\n");
        assert_eq!(
            error,
            json!({
                "ok": false,
                "erro": "config_incompleta",
                "detalhe": "github exige owner e repo"
            })
        );
    }
}
