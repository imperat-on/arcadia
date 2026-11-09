//! Estado instalado, manifests, backups e rollback dos fixes.
//!
//! Oráculo: `app/electron/fixes.js` (`manifestPath`, `readManifest`,
//! `isFixed`, `unfix`) e `app/electron/fix_downloader.sh` (nomes e conteúdo
//! dos manifests `.slssteam_fix_dlls`/`.slssteam_fix_launchers`).
//!
//! Todo acesso a disco passa por [`FixFs`]. Os testes usam [`StdFixFs`] sobre
//! um `tempfile::TempDir`; a borda pode injetar a mesma implementação. Nenhum
//! caminho sai das raízes informadas em [`PathRoots`]: entradas de arquivo com
//! `..`, caminho absoluto, unidade Windows, ADS (`:`) ou NUL são rejeitadas
//! antes de qualquer escrita.

use std::collections::HashSet;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalog::{appid_path_component, FixType};

/// Manifest dos DLLs instalados (existência = fix aplicado no JS).
pub const DLL_MANIFEST: &str = ".slssteam_fix_dlls";
/// Manifest dos launchers do crack (usado pelo redirect do Play).
pub const LAUNCHER_MANIFEST: &str = ".slssteam_fix_launchers";
/// Estado interno do crate (necessário para backup/rollback). Não substitui
/// o teste de `isFixed`: a UI continua vendo `.slssteam_fix_dlls`.
pub const INSTALL_STATE_FILE: &str = ".arcadia_fix_state.json";
/// Versão do estado interno.
pub const INSTALL_STATE_VERSION: u32 = 1;
/// Profundidade/tamanho máximo de um caminho de entrada de arquivo.
pub const MAX_ARCHIVE_PATH_LEN: usize = 4096;

// ─── filesystem injetado ────────────────────────────────────────────────────

/// Erro de filesystem com mensagem estável (sem caminho absoluto).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsError {
    pub message: String,
}

impl FsError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for FsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for FsError {}

impl From<io::Error> for FsError {
    fn from(error: io::Error) -> Self {
        Self::new(error.kind().to_string())
    }
}

pub type FsResult<T> = Result<T, FsError>;

/// Operações de disco usadas pelo instalador. Implementações podem ser
/// temporárias, em memória ou com falha injetada (rollback).
pub trait FixFs: Send + Sync {
    fn exists(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    fn is_file(&self, path: &Path) -> bool;
    fn read(&self, path: &Path) -> FsResult<Vec<u8>>;
    fn write(&self, path: &Path, data: &[u8]) -> FsResult<()>;
    fn create_dir_all(&self, path: &Path) -> FsResult<()>;
    fn remove_file(&self, path: &Path) -> FsResult<()>;
    fn remove_dir(&self, path: &Path) -> FsResult<()>;
    fn remove_dir_all(&self, path: &Path) -> FsResult<()>;

    /// Aplica permissão 0600 quando a plataforma suporta. O default é no-op.
    fn set_private(&self, _path: &Path) -> FsResult<()> {
        Ok(())
    }
}

/// Implementação de produção sobre `std::fs`.
#[derive(Debug, Clone, Copy, Default)]
pub struct StdFixFs;

impl FixFs for StdFixFs {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn read(&self, path: &Path) -> FsResult<Vec<u8>> {
        std::fs::read(path).map_err(FsError::from)
    }

    fn write(&self, path: &Path, data: &[u8]) -> FsResult<()> {
        std::fs::write(path, data).map_err(FsError::from)
    }

    fn create_dir_all(&self, path: &Path) -> FsResult<()> {
        std::fs::create_dir_all(path).map_err(FsError::from)
    }

    fn remove_file(&self, path: &Path) -> FsResult<()> {
        std::fs::remove_file(path).map_err(FsError::from)
    }

    fn remove_dir(&self, path: &Path) -> FsResult<()> {
        std::fs::remove_dir(path).map_err(FsError::from)
    }

    fn remove_dir_all(&self, path: &Path) -> FsResult<()> {
        std::fs::remove_dir_all(path).map_err(FsError::from)
    }

    #[cfg(unix)]
    fn set_private(&self, path: &Path) -> FsResult<()> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(FsError::from)
    }
}

// ─── raízes e contenção de caminhos ─────────────────────────────────────────

/// Erro de caminho rejeitado (mesmas mensagens do IPC quando aplicável).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathError(pub String);

impl std::fmt::Display for PathError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PathError {}

/// Raízes permitidas para instalação, prefixo, backups e dados.
///
/// `game_roots`/`prefix_roots` vazios significam "sem allowlist" (o
/// `installPath` ainda precisa ser absoluto e sem `..`, como no IPC). Quando
/// informadas, o `installPath` precisa estar lexicamente dentro de uma delas.
#[derive(Debug, Clone)]
pub struct PathRoots {
    pub game_roots: Vec<PathBuf>,
    pub prefix_roots: Vec<PathBuf>,
    pub backup_root: PathBuf,
    pub data_root: PathBuf,
}

impl PathRoots {
    pub fn new(
        game_roots: Vec<PathBuf>,
        prefix_roots: Vec<PathBuf>,
        backup_root: PathBuf,
        data_root: PathBuf,
    ) -> Self {
        Self {
            game_roots,
            prefix_roots,
            backup_root,
            data_root,
        }
    }

    /// Sem allowlist de jogos; backups/dados continuam confinados.
    pub fn permissive(backup_root: PathBuf, data_root: PathBuf) -> Self {
        Self::new(Vec::new(), Vec::new(), backup_root, data_root)
    }

    /// `path.isAbsolute(installPath) && !installPath.includes("..")` + raízes.
    pub fn validate_install_path(&self, raw: &str) -> Result<PathBuf, PathError> {
        if raw.is_empty() || !Path::new(raw).is_absolute() || raw.contains("..") {
            return Err(PathError("installPath rejeitado".into()));
        }
        let path = Path::new(raw);
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(PathError("installPath rejeitado".into()));
        }
        if !self.game_roots.is_empty() || !self.prefix_roots.is_empty() {
            let allowed = self
                .game_roots
                .iter()
                .chain(self.prefix_roots.iter())
                .any(|root| is_within(root, path));
            if !allowed {
                return Err(PathError(
                    "installPath fora das raízes de jogo/prefixo".into(),
                ));
            }
        }
        Ok(path.to_path_buf())
    }

    pub fn backup_dir(&self, appid: &str) -> PathBuf {
        self.backup_root.join(appid)
    }

    pub fn data_tmp_dir(&self) -> PathBuf {
        self.data_root.join("tmp")
    }

    pub fn ensure_backup_root(&self) -> Result<(), PathError> {
        if !self.backup_root.is_absolute() || !self.data_root.is_absolute() {
            return Err(PathError("raízes internas precisam ser absolutas".into()));
        }
        Ok(())
    }
}

/// Contenção lexical por componentes (não segue symlinks).
pub fn is_within(root: &Path, path: &Path) -> bool {
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return false;
    }
    path.starts_with(root)
}

// ─── layout de arquivo e regras de extração ─────────────────────────────────

/// Regras equivalentes ao `fix_downloader.sh` (7zz `-y` ou `unzip -o`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionRules {
    pub overwrite: bool,
    pub extract_nested: bool,
    pub delete_nested_archives: bool,
    pub skip_secondary_volumes: bool,
    pub dll_manifest: String,
    pub launcher_manifest: String,
}

impl Default for ExtractionRules {
    fn default() -> Self {
        Self {
            overwrite: true,
            extract_nested: true,
            delete_nested_archives: true,
            skip_secondary_volumes: true,
            dll_manifest: DLL_MANIFEST.to_string(),
            launcher_manifest: LAUNCHER_MANIFEST.to_string(),
        }
    }
}

/// Entrada de arquivo já lida pelo extrator (a borda usa 7zz/unzip; testes
/// usam [`StaticArchiveExtractor`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveEntry {
    pub path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data: Vec<u8>,
    #[serde(default)]
    pub directory: bool,
}

impl ArchiveEntry {
    pub fn file(path: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            data: data.into(),
            directory: false,
        }
    }

    pub fn directory(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            data: Vec::new(),
            directory: true,
        }
    }
}

/// Arquivo compactado dentro do zip principal, com o conteúdo que o 7zz
/// extrairia em seguida.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NestedArchive {
    pub path: String,
    pub entries: Vec<ArchiveEntry>,
}

impl NestedArchive {
    pub fn new(path: impl Into<String>, entries: Vec<ArchiveEntry>) -> Self {
        Self {
            path: path.into(),
            entries,
        }
    }
}

/// Layout completo entregue pelo extrator.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveLayout {
    pub entries: Vec<ArchiveEntry>,
    pub nested: Vec<NestedArchive>,
}

impl ArchiveLayout {
    pub fn new(entries: Vec<ArchiveEntry>) -> Self {
        Self {
            entries,
            nested: Vec::new(),
        }
    }

    pub fn with_nested(mut self, nested: Vec<NestedArchive>) -> Self {
        self.nested = nested;
        self
    }

    pub fn push(&mut self, entry: ArchiveEntry) {
        self.entries.push(entry);
    }

    pub fn push_nested(&mut self, nested: NestedArchive) {
        self.nested.push(nested);
    }
}

/// Extração injetada. `None`/erro interrompe o apply sem tocar no jogo.
pub trait ArchiveExtractor: Send + Sync {
    fn extract(&self, archive: &Path) -> Result<ArchiveLayout, InstallError>;
}

/// Extrator de teste/plano: devolve um layout fixo, sem tocar no arquivo.
#[derive(Debug, Clone, Default)]
pub struct StaticArchiveExtractor {
    pub layout: ArchiveLayout,
}

impl StaticArchiveExtractor {
    pub fn new(layout: ArchiveLayout) -> Self {
        Self { layout }
    }
}

impl ArchiveExtractor for StaticArchiveExtractor {
    fn extract(&self, _archive: &Path) -> Result<ArchiveLayout, InstallError> {
        Ok(self.layout.clone())
    }
}

/// Caminho relativo validado de uma entrada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeArchivePath {
    pub path: PathBuf,
    /// Sempre com `/` (formato dos manifests).
    pub relative: String,
    pub directory: bool,
}

/// Valida uma entrada de arquivo e devolve o caminho relativo normalizado.
///
/// Rejeita, além de `..`/absoluto: unidade (`C:`), UNC (`\\\\`), NUL/controle,
/// ADS (`:`) e os arquivos internos de manifest/estado.
pub fn validate_archive_path(raw: &str) -> Result<SafeArchivePath, InstallError> {
    if raw.is_empty() || raw.len() > MAX_ARCHIVE_PATH_LEN {
        return Err(InstallError::new("archive entry path rejected"));
    }
    if raw.chars().any(char::is_control) {
        return Err(InstallError::new("archive entry path rejected"));
    }
    let directory = raw.ends_with('/') || raw.ends_with('\\');
    let replaced = raw.replace('\\', "/");
    if replaced.starts_with('/') || replaced.contains(':') {
        return Err(InstallError::new("archive entry path rejected"));
    }
    let mut components = Vec::new();
    for component in replaced.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            return Err(InstallError::new("archive entry path rejected"));
        }
        if is_reserved_internal_name(component) || is_windows_reserved_name(component) {
            return Err(InstallError::new("archive entry path rejected"));
        }
        components.push(component);
    }
    if components.is_empty() {
        return Err(InstallError::new("archive entry path rejected"));
    }
    let relative = components.join("/");
    let path = components.iter().collect::<PathBuf>();
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(InstallError::new("archive entry path rejected"));
    }
    Ok(SafeArchivePath {
        path,
        relative,
        directory,
    })
}

fn is_reserved_internal_name(component: &str) -> bool {
    component.eq_ignore_ascii_case(DLL_MANIFEST)
        || component.eq_ignore_ascii_case(LAUNCHER_MANIFEST)
        || component.eq_ignore_ascii_case(INSTALL_STATE_FILE)
}

/// Nomes de dispositivo do Windows e componentes que o Explorer normaliza
/// (`.`/espaço no fim) não podem virar arquivo do jogo.
fn is_windows_reserved_name(component: &str) -> bool {
    if component.ends_with('.') || component.ends_with(' ') {
        return true;
    }
    let base = component
        .split_once('.')
        .map(|(base, _)| base)
        .unwrap_or(component)
        .to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && base.as_bytes()[3].is_ascii_digit()
            && base.as_bytes()[3] != b'0')
}

/// Extensões reconhecidas pelo `find` do `fix_downloader.sh`.
pub fn is_archive_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".rar")
        || lower.ends_with(".zip")
        || lower.ends_with(".7z")
        || is_secondary_volume(path)
}

/// `partN.rar` (N>1), `.rNN` e `.zNN` não são extraídos.
pub fn is_secondary_volume(path: &str) -> bool {
    let base = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    if base.ends_with(".rar") {
        if let Some(index) = base.rfind(".part0") {
            let tail = &base[index + 6..base.len() - 4];
            return tail
                .as_bytes()
                .last()
                .is_some_and(|byte| matches!(*byte, b'2'..=b'9'));
        }
    }
    if let Some((_, extension)) = base.rsplit_once('.') {
        let bytes = extension.as_bytes();
        if bytes.len() == 2 && matches!(bytes[0], b'r' | b'z') && bytes[1].is_ascii_digit() {
            return true;
        }
        if bytes.len() == 3
            && matches!(bytes[0], b'r' | b'z')
            && bytes[1].is_ascii_digit()
            && bytes[2].is_ascii_digit()
        {
            return true;
        }
    }
    false
}

/// Nome do DLL (basename) como o `sed 's#.*[/\\\\]##'` do script.
pub fn dll_basename(path: &str) -> Option<&str> {
    let lower = path.to_ascii_lowercase();
    if !lower.ends_with(".dll") {
        return None;
    }
    Some(path.rsplit(['/', '\\']).next().unwrap_or(path))
}

/// `grep -iE '(^|/)(launcher\.exe|launcher_[^/]+\.exe|[^/]+_launcher\.exe)$'`.
pub fn is_launcher(path: &str) -> bool {
    let lower = path.replace('\\', "/").to_ascii_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    base == "launcher.exe"
        || (base.starts_with("launcher_")
            && base.ends_with(".exe")
            && base.len() > "launcher_.exe".len())
        || (base.ends_with("_launcher.exe") && base.len() > "_launcher.exe".len())
}

/// `sort -u -f`: orderna sem diferenciar caixa e remove duplicatas,
/// preservando a grafia da primeira ocorrência.
pub fn sort_unique_case_insensitive(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut unique = Vec::new();
    for value in values {
        let key = value.to_ascii_lowercase();
        if seen.insert(key) {
            unique.push(value);
        }
    }
    unique.sort_by(|left, right| {
        left.to_ascii_lowercase()
            .cmp(&right.to_ascii_lowercase())
            .then_with(|| left.cmp(right))
    });
    unique
}

/// DLLs (basename) de uma lista de caminhos.
pub fn manifest_dlls<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    sort_unique_case_insensitive(
        paths
            .into_iter()
            .filter_map(dll_basename)
            .map(str::to_string),
    )
}

/// Launchers (caminho relativo) de uma lista de caminhos.
pub fn manifest_launchers<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    sort_unique_case_insensitive(
        paths
            .into_iter()
            .filter(|path| is_launcher(path))
            .map(|path| path.replace('\\', "/")),
    )
}

pub fn dll_manifest_path(install_root: &Path) -> PathBuf {
    install_root.join(DLL_MANIFEST)
}

pub fn launcher_manifest_path(install_root: &Path) -> PathBuf {
    install_root.join(LAUNCHER_MANIFEST)
}

pub fn state_path(install_root: &Path) -> PathBuf {
    install_root.join(INSTALL_STATE_FILE)
}

/// `readManifest`: linhas trimadas e não vazias.
pub fn read_manifest(fs: &dyn FixFs, path: &Path) -> Vec<String> {
    match fs.read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Escreve um manifest no formato do `sort -u -f > arquivo` (linha final).
/// Lista vazia não cria o arquivo, igual ao `[ -s "$ACC" ]` do script.
pub fn write_manifest(fs: &dyn FixFs, path: &Path, values: &[String]) -> FsResult<bool> {
    if values.is_empty() {
        return Ok(false);
    }
    let mut content = values.join("\n");
    content.push('\n');
    fs.write(path, content.as_bytes())?;
    Ok(true)
}

/// `isFixed(installPath)`: existência do manifest de DLLs.
pub fn is_fixed(fs: &dyn FixFs, install_root: &Path) -> bool {
    fs.exists(&dll_manifest_path(install_root))
}

// ─── estado instalado ───────────────────────────────────────────────────────

/// O que foi feito com um caminho gerenciado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ManagedKind {
    Written,
    Removed,
}

/// Arquivo gerenciado pelo fix instalado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedFile {
    /// Relativo à raiz do jogo, separadores `/`.
    pub path: String,
    pub kind: ManagedKind,
    /// `true` quando o arquivo não existia antes do fix.
    #[serde(default)]
    pub created: bool,
    /// `true` quando existe backup do conteúdo original em `backup_root`.
    #[serde(default)]
    pub backup: bool,
    #[serde(default)]
    pub size: u64,
}

/// Estado interno gravado em `.arcadia_fix_state.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallState {
    pub version: u32,
    pub appid: String,
    #[serde(rename = "type")]
    pub fix_type: FixType,
    #[serde(rename = "sourceUrl")]
    pub source_url: String,
    #[serde(default)]
    pub files: Vec<ManagedFile>,
    #[serde(default)]
    pub directories: Vec<String>,
}

impl InstallState {
    fn new(appid: &str, fix_type: FixType, source_url: &str) -> Self {
        Self {
            version: INSTALL_STATE_VERSION,
            appid: appid.to_string(),
            fix_type,
            source_url: source_url.to_string(),
            files: Vec::new(),
            directories: Vec::new(),
        }
    }

    fn file(&self, relative: &str) -> Option<&ManagedFile> {
        self.files.iter().find(|file| file.path == relative)
    }

    fn upsert(&mut self, file: ManagedFile) {
        if let Some(existing) = self.files.iter_mut().find(|entry| entry.path == file.path) {
            existing.kind = file.kind;
            existing.created = existing.created || file.created;
            existing.backup = existing.backup || file.backup;
            existing.size = file.size;
        } else {
            self.files.push(file);
        }
    }
}

/// Erro de instalação/rollback (mensagem estável, sem caminho absoluto).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallError(pub String);

impl std::fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for InstallError {}

impl InstallError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl From<FsError> for InstallError {
    fn from(error: FsError) -> Self {
        Self(error.message)
    }
}

/// Lê o estado interno. `Ok(None)` quando o arquivo não existe; estado
/// ilegível é erro (nunca sobrescrevemos backups de um estado desconhecido).
pub fn read_install_state(
    fs: &dyn FixFs,
    install_root: &Path,
) -> Result<Option<InstallState>, InstallError> {
    let path = state_path(install_root);
    if !fs.exists(&path) {
        return Ok(None);
    }
    let bytes = fs.read(&path)?;
    let state: InstallState =
        serde_json::from_slice(&bytes).map_err(|_| InstallError::new("fix state is invalid"))?;
    if state.version != INSTALL_STATE_VERSION {
        return Err(InstallError::new("fix state version is unsupported"));
    }
    Ok(Some(state))
}

fn write_install_state(fs: &dyn FixFs, install_root: &Path, state: &InstallState) -> FsResult<()> {
    let bytes = serde_json::to_vec(state)
        .map_err(|error| FsError::new(format!("fix state serialization: {error}")))?;
    fs.write(&state_path(install_root), &bytes)
}

// ─── plano de instalação ────────────────────────────────────────────────────

/// Arquivo que o apply deve gravar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedWrite {
    pub path: String,
    pub target: PathBuf,
    pub archive_path: String,
    #[serde(default)]
    pub data: Vec<u8>,
}

/// Arquivo que o apply deve remover (nested archive/volume do próprio fix).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedRemoval {
    pub path: String,
    pub target: PathBuf,
    pub archive_path: String,
}

/// Diretório vazio que veio no arquivo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedDirectory {
    pub path: String,
    pub target: PathBuf,
}

/// Backup planejado para um alvo que já existe no jogo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedBackup {
    pub path: String,
    pub target: PathBuf,
    pub backup: PathBuf,
}

/// Manifests calculados a partir do layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestPlan {
    pub dlls: Vec<String>,
    pub launchers: Vec<String>,
    pub dll_manifest: PathBuf,
    pub launcher_manifest: PathBuf,
}

impl ManifestPlan {
    pub fn is_empty(&self) -> bool {
        self.dlls.is_empty() && self.launchers.is_empty()
    }
}

/// Plano que [`apply`] executa. O `ApplyPlan` do módulo `plan` embrulha este
/// com download, pre/pós-condições e envelopes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallPlan {
    pub appid: String,
    #[serde(rename = "type")]
    pub fix_type: FixType,
    #[serde(rename = "sourceUrl")]
    pub source_url: String,
    pub install_root: PathBuf,
    pub backup_root: PathBuf,
    pub backup_dir: PathBuf,
    pub writes: Vec<PlannedWrite>,
    pub removals: Vec<PlannedRemoval>,
    pub directories: Vec<PlannedDirectory>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backups: Vec<PlannedBackup>,
    pub manifests: ManifestPlan,
}

/// Resultado de [`apply`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallOutcome {
    pub already_installed: bool,
    pub state: InstallState,
    pub written: usize,
    pub removed: usize,
}

/// Resultado de [`unfix`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnfixOutcome {
    pub restored: usize,
    pub removed: usize,
    /// Arquivos que tinham backup mas o backup sumiu.
    pub unrecovered: Vec<String>,
}

#[derive(Debug, Clone)]
enum Undo {
    RestoreFromBackup { target: PathBuf, backup: PathBuf },
    RestoreBytes { target: PathBuf, bytes: Vec<u8> },
    RemoveFile { target: PathBuf },
    RemoveDir { target: PathBuf },
}

#[derive(Default)]
struct Transaction {
    undo: Vec<Undo>,
}

impl Transaction {
    fn rollback(&self, fs: &dyn FixFs) {
        for undo in self.undo.iter().rev() {
            match undo {
                Undo::RestoreFromBackup { target, backup } => {
                    if let Ok(bytes) = fs.read(backup) {
                        let _ = fs.write(target, &bytes);
                    }
                }
                Undo::RestoreBytes { target, bytes } => {
                    let _ = fs.write(target, bytes);
                }
                Undo::RemoveFile { target } => {
                    if fs.exists(target) {
                        let _ = fs.remove_file(target);
                    }
                }
                Undo::RemoveDir { target } => {
                    let _ = fs.remove_dir(target);
                }
            }
        }
    }
}

/// Aplica um plano de forma transacional.
///
/// Idempotente: se o estado já descreve todos os arquivos e eles existem, não
/// reescreve nada e devolve `already_installed = true`. Falhas no meio do
/// caminho restauram backups e removem arquivos criados nesta execução.
pub fn apply(fs: &dyn FixFs, plan: &InstallPlan) -> Result<InstallOutcome, InstallError> {
    validate_plan_paths(plan)?;
    if !fs.exists(&plan.install_root) || !fs.is_dir(&plan.install_root) {
        return Err(InstallError::new("install path not found"));
    }

    let existing = read_install_state(fs, &plan.install_root)?;
    if let Some(state) = &existing {
        if state.appid != plan.appid {
            return Err(InstallError::new(
                "another fix is already installed; unfix it first",
            ));
        }
        if state_is_applied(fs, plan, state) {
            return Ok(InstallOutcome {
                already_installed: true,
                state: state.clone(),
                written: 0,
                removed: 0,
            });
        }
    }

    let mut state =
        existing.unwrap_or_else(|| InstallState::new(&plan.appid, plan.fix_type, &plan.source_url));
    state.appid = plan.appid.clone();
    state.fix_type = plan.fix_type;
    state.source_url = plan.source_url.clone();

    let mut transaction = Transaction::default();
    let mut written = 0;
    let mut removed = 0;

    let result = (|| -> Result<(), InstallError> {
        for directory in &plan.directories {
            ensure_directory(
                fs,
                &plan.install_root,
                &directory.target,
                &mut state.directories,
                &mut transaction,
            )?;
        }

        for write in &plan.writes {
            let parent = write
                .target
                .parent()
                .ok_or_else(|| InstallError::new("archive entry path rejected"))?;
            ensure_directory(
                fs,
                &plan.install_root,
                parent,
                &mut state.directories,
                &mut transaction,
            )?;
            if fs.exists(&write.target) && !fs.is_file(&write.target) {
                return Err(InstallError::new("target is not a regular file"));
            }
            let managed = state.file(&write.path).cloned();
            let (created, backup) = prepare_managed_target(
                fs,
                plan,
                &write.path,
                &write.target,
                managed.as_ref(),
                &mut transaction,
            )?;
            fs.write(&write.target, &write.data)?;
            state.upsert(ManagedFile {
                path: write.path.clone(),
                kind: ManagedKind::Written,
                created,
                backup,
                size: write.data.len() as u64,
            });
            written += 1;
        }

        for removal in &plan.removals {
            if fs.exists(&removal.target) && !fs.is_file(&removal.target) {
                continue;
            }
            let managed = state.file(&removal.path).cloned();
            let (created, backup) = prepare_managed_target(
                fs,
                plan,
                &removal.path,
                &removal.target,
                managed.as_ref(),
                &mut transaction,
            )?;
            if fs.exists(&removal.target) {
                fs.remove_file(&removal.target)?;
                removed += 1;
            }
            state.upsert(ManagedFile {
                path: removal.path.clone(),
                kind: ManagedKind::Removed,
                created,
                backup,
                size: 0,
            });
        }

        write_manifest_with_undo(
            fs,
            &plan.manifests.dll_manifest,
            &plan.manifests.dlls,
            &mut transaction,
        )?;
        write_manifest_with_undo(
            fs,
            &plan.manifests.launcher_manifest,
            &plan.manifests.launchers,
            &mut transaction,
        )?;

        state
            .files
            .sort_by(|left, right| left.path.cmp(&right.path));
        state.directories.sort();
        state.directories.dedup();

        if fs.exists(&state_path(&plan.install_root)) {
            transaction.undo.push(Undo::RestoreBytes {
                target: state_path(&plan.install_root),
                bytes: fs.read(&state_path(&plan.install_root))?,
            });
        } else {
            transaction.undo.push(Undo::RemoveFile {
                target: state_path(&plan.install_root),
            });
        }
        write_install_state(fs, &plan.install_root, &state)?;
        Ok(())
    })();

    match result {
        Ok(()) => Ok(InstallOutcome {
            already_installed: false,
            state,
            written,
            removed,
        }),
        Err(error) => {
            transaction.rollback(fs);
            Err(error)
        }
    }
}

fn state_is_applied(fs: &dyn FixFs, plan: &InstallPlan, state: &InstallState) -> bool {
    if state.appid != plan.appid {
        return false;
    }
    let mut planned = HashSet::new();
    for write in &plan.writes {
        planned.insert(write.path.as_str());
        let Some(file) = state.file(&write.path) else {
            return false;
        };
        let Some(target) = resolve_relative(&plan.install_root, &write.path) else {
            return false;
        };
        if file.kind != ManagedKind::Written || !fs.exists(&target) {
            return false;
        }
    }
    for removal in &plan.removals {
        planned.insert(removal.path.as_str());
        let Some(file) = state.file(&removal.path) else {
            return false;
        };
        let Some(target) = resolve_relative(&plan.install_root, &removal.path) else {
            return false;
        };
        if file.kind != ManagedKind::Removed || fs.exists(&target) {
            return false;
        }
    }
    for directory in &plan.directories {
        let Some(target) = resolve_relative(&plan.install_root, &directory.path) else {
            return false;
        };
        if !fs.is_dir(&target) {
            return false;
        }
    }
    // O plano precisa cobrir exatamente o estado; um estado com arquivos
    // extras pertence a outra variante do fix e não pode virar no-op.
    if state
        .files
        .iter()
        .any(|file| !planned.contains(file.path.as_str()))
    {
        return false;
    }
    if !plan.manifests.dlls.is_empty() && !fs.exists(&plan.manifests.dll_manifest) {
        return false;
    }
    if !plan.manifests.launchers.is_empty() && !fs.exists(&plan.manifests.launcher_manifest) {
        return false;
    }
    true
}

fn prepare_managed_target(
    fs: &dyn FixFs,
    plan: &InstallPlan,
    relative: &str,
    target: &Path,
    managed: Option<&ManagedFile>,
    transaction: &mut Transaction,
) -> Result<(bool, bool), InstallError> {
    if let Some(managed) = managed {
        if managed.backup {
            let backup_path = backup_path_for(plan, relative)?;
            if !fs.exists(&backup_path) {
                if fs.exists(target) {
                    let bytes = fs.read(target)?;
                    if let Some(parent) = backup_path.parent() {
                        fs.create_dir_all(parent)?;
                    }
                    fs.write(&backup_path, &bytes)?;
                } else {
                    return Err(InstallError::new("fix backup is missing"));
                }
            }
            transaction.undo.push(Undo::RestoreFromBackup {
                target: target.to_path_buf(),
                backup: backup_path,
            });
        } else if managed.created {
            transaction.undo.push(Undo::RemoveFile {
                target: target.to_path_buf(),
            });
        }
        return Ok((managed.created, managed.backup));
    }

    if fs.exists(target) {
        let backup_path = backup_path_for(plan, relative)?;
        let bytes = fs.read(target)?;
        if let Some(parent) = backup_path.parent() {
            fs.create_dir_all(parent)?;
        }
        if !fs.exists(&backup_path) {
            fs.write(&backup_path, &bytes)?;
        }
        transaction.undo.push(Undo::RestoreFromBackup {
            target: target.to_path_buf(),
            backup: backup_path,
        });
        Ok((false, true))
    } else {
        transaction.undo.push(Undo::RemoveFile {
            target: target.to_path_buf(),
        });
        Ok((true, false))
    }
}

fn write_manifest_with_undo(
    fs: &dyn FixFs,
    path: &Path,
    values: &[String],
    transaction: &mut Transaction,
) -> Result<(), InstallError> {
    if values.is_empty() {
        return Ok(());
    }
    if fs.exists(path) {
        transaction.undo.push(Undo::RestoreBytes {
            target: path.to_path_buf(),
            bytes: fs.read(path)?,
        });
    } else {
        transaction.undo.push(Undo::RemoveFile {
            target: path.to_path_buf(),
        });
    }
    write_manifest(fs, path, values)?;
    Ok(())
}

fn ensure_directory(
    fs: &dyn FixFs,
    install_root: &Path,
    directory: &Path,
    created_directories: &mut Vec<String>,
    transaction: &mut Transaction,
) -> Result<(), InstallError> {
    if !is_within(install_root, directory) {
        return Err(InstallError::new("archive entry path rejected"));
    }
    if fs.exists(directory) {
        if fs.is_dir(directory) {
            return Ok(());
        }
        return Err(InstallError::new("target is not a directory"));
    }
    if directory == install_root {
        return Ok(());
    }
    if let Some(parent) = directory.parent() {
        ensure_directory(fs, install_root, parent, created_directories, transaction)?;
    }
    fs.create_dir_all(directory)?;
    transaction.undo.push(Undo::RemoveDir {
        target: directory.to_path_buf(),
    });
    if let Some(relative) = relative_to(install_root, directory) {
        if !relative.is_empty() && !created_directories.iter().any(|entry| entry == &relative) {
            created_directories.push(relative);
        }
    }
    Ok(())
}

fn backup_path_for(plan: &InstallPlan, relative: &str) -> Result<PathBuf, InstallError> {
    let path = resolve_relative(&plan.backup_dir, relative)
        .ok_or_else(|| InstallError::new("fix backup path rejected"))?;
    if !is_within(&plan.backup_dir, &path) || !is_within(&plan.backup_root, &path) {
        return Err(InstallError::new("fix backup path rejected"));
    }
    Ok(path)
}

fn validate_plan_paths(plan: &InstallPlan) -> Result<(), InstallError> {
    if appid_path_component(&plan.appid).is_none() {
        return Err(InstallError::new("invalid appid"));
    }
    if !plan.install_root.is_absolute()
        || !plan.backup_root.is_absolute()
        || !plan.backup_dir.is_absolute()
        || plan
            .backup_root
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(InstallError::new("install path rejected"));
    }
    if !is_within(&plan.backup_root, &plan.backup_dir) {
        return Err(InstallError::new("fix backup path rejected"));
    }
    for write in &plan.writes {
        if !is_within(&plan.install_root, &write.target)
            || validate_archive_path(&write.path).is_err()
        {
            return Err(InstallError::new("archive entry path rejected"));
        }
    }
    for removal in &plan.removals {
        if !is_within(&plan.install_root, &removal.target)
            || validate_archive_path(&removal.path).is_err()
        {
            return Err(InstallError::new("archive entry path rejected"));
        }
    }
    for directory in &plan.directories {
        if !is_within(&plan.install_root, &directory.target)
            || validate_archive_path(&directory.path).is_err()
        {
            return Err(InstallError::new("archive entry path rejected"));
        }
    }
    for backup in &plan.backups {
        if !is_within(&plan.backup_root, &backup.backup)
            || !is_within(&plan.backup_dir, &backup.backup)
            || !is_within(&plan.install_root, &backup.target)
            || validate_archive_path(&backup.path).is_err()
        {
            return Err(InstallError::new("fix backup path rejected"));
        }
    }
    Ok(())
}

// ─── unfix ──────────────────────────────────────────────────────────────────

/// Remove o fix instalado: restaura backups, apaga arquivos criados e os
/// manifests. Sem estado (fix antigo), remove somente os manifests — igual
/// ao `unfix` do JS.
pub fn unfix(
    fs: &dyn FixFs,
    install_root: &Path,
    backup_root: &Path,
    appid_hint: Option<&str>,
) -> Result<UnfixOutcome, InstallError> {
    if !install_root.is_absolute() {
        return Err(InstallError::new("install path rejected"));
    }
    if !backup_root.is_absolute()
        || backup_root
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(InstallError::new("fix backup path rejected"));
    }
    let mut outcome = UnfixOutcome::default();
    let state = read_install_state(fs, install_root).unwrap_or_default();
    let mut cleanup_backup_dir = None;
    if let Some(state) = &state {
        let appid = if state.appid.is_empty() || state.appid == "." || state.appid == ".." {
            appid_hint.unwrap_or_default()
        } else {
            state.appid.as_str()
        };
        if appid.is_empty() {
            return Err(InstallError::new("invalid appid"));
        }
        appid_path_component(appid).ok_or_else(|| InstallError::new("invalid appid"))?;
        let backup_dir = backup_root.join(appid);
        if !is_within(backup_root, &backup_dir) {
            return Err(InstallError::new("fix backup path rejected"));
        }
        cleanup_backup_dir = Some(backup_dir.clone());
        for file in &state.files {
            let Some(target) = resolve_relative(install_root, &file.path) else {
                continue;
            };
            if !is_within(install_root, &target) {
                continue;
            }
            let restored = if file.backup {
                let Some(backup) = resolve_relative(&backup_dir, &file.path) else {
                    outcome.unrecovered.push(file.path.clone());
                    continue;
                };
                if !is_within(backup_root, &backup) || !fs.exists(&backup) {
                    outcome.unrecovered.push(file.path.clone());
                    false
                } else {
                    let bytes = fs.read(&backup)?;
                    if let Some(parent) = target.parent() {
                        fs.create_dir_all(parent)?;
                    }
                    fs.write(&target, &bytes)?;
                    outcome.restored += 1;
                    true
                }
            } else {
                false
            };
            if !restored
                && file.kind == ManagedKind::Written
                && file.created
                && fs.exists(&target)
                && fs.remove_file(&target).is_ok()
            {
                outcome.removed += 1;
            }
        }
        let mut directories: Vec<&String> = state.directories.iter().collect();
        directories.sort_by_key(|directory| {
            std::cmp::Reverse(directory.split('/').count() * 1000 + directory.len())
        });
        for directory in directories {
            let Some(target) = resolve_relative(install_root, directory) else {
                continue;
            };
            if is_within(install_root, &target) {
                let _ = fs.remove_dir(&target);
            }
        }
    }

    // O JS sempre tenta apagar os dois manifests, exista estado ou não.
    for path in [
        dll_manifest_path(install_root),
        launcher_manifest_path(install_root),
    ] {
        if fs.exists(&path) {
            let _ = fs.remove_file(&path);
        }
    }
    let state_file = state_path(install_root);
    if fs.exists(&state_file) {
        let _ = fs.remove_file(&state_file);
    }
    if let Some(backup_dir) = cleanup_backup_dir {
        if fs.exists(&backup_dir) && is_within(backup_root, &backup_dir) {
            let _ = fs.remove_dir_all(&backup_dir);
        }
    }
    Ok(outcome)
}

// ─── utilidades de caminho ──────────────────────────────────────────────────

/// Junta um caminho relativo (`a/b/c`) sob `root`, rejeitando traversal.
pub fn resolve_relative(root: &Path, relative: &str) -> Option<PathBuf> {
    let replaced = relative.replace('\\', "/");
    if replaced.is_empty()
        || replaced.starts_with('/')
        || replaced.contains(':')
        || replaced.chars().any(char::is_control)
    {
        return None;
    }
    let mut path = root.to_path_buf();
    for component in replaced.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".."
            || is_reserved_internal_name(component)
            || is_windows_reserved_name(component)
        {
            return None;
        }
        path.push(component);
    }
    if !is_within(root, &path) {
        return None;
    }
    Some(path)
}

fn relative_to(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn test_roots(dir: &Path) -> PathRoots {
        PathRoots::new(
            vec![dir.join("game")],
            vec![dir.join("prefix")],
            dir.join("backups"),
            dir.join("data"),
        )
    }

    fn plan_for(dir: &Path, appid: &str, layout: &ArchiveLayout) -> InstallPlan {
        let install_root = dir.join("game");
        let mut writes = Vec::new();
        let mut removals = Vec::new();
        let mut directories = Vec::new();
        let mut paths = Vec::new();
        for entry in &layout.entries {
            let safe = validate_archive_path(&entry.path).unwrap();
            if entry.directory || safe.directory {
                directories.push(PlannedDirectory {
                    path: safe.relative.clone(),
                    target: install_root.join(&safe.path),
                });
            } else if layout.nested.iter().any(|nested| nested.path == entry.path)
                || is_secondary_volume(&safe.relative)
            {
                removals.push(PlannedRemoval {
                    path: safe.relative.clone(),
                    target: install_root.join(&safe.path),
                    archive_path: entry.path.clone(),
                });
            } else {
                paths.push(safe.relative.clone());
                writes.push(PlannedWrite {
                    path: safe.relative,
                    target: install_root.join(&safe.path),
                    archive_path: entry.path.clone(),
                    data: entry.data.clone(),
                });
            }
        }
        for nested in &layout.nested {
            let safe = validate_archive_path(&nested.path).unwrap();
            removals.push(PlannedRemoval {
                path: safe.relative.clone(),
                target: install_root.join(&safe.path),
                archive_path: nested.path.clone(),
            });
            for entry in &nested.entries {
                let safe = validate_archive_path(&entry.path).unwrap();
                if entry.directory || safe.directory {
                    directories.push(PlannedDirectory {
                        path: safe.relative.clone(),
                        target: install_root.join(&safe.path),
                    });
                } else {
                    paths.push(safe.relative.clone());
                    writes.push(PlannedWrite {
                        path: safe.relative,
                        target: install_root.join(&safe.path),
                        archive_path: entry.path.clone(),
                        data: entry.data.clone(),
                    });
                }
            }
        }
        InstallPlan {
            appid: appid.into(),
            fix_type: FixType::Generic,
            source_url: format!("https://files.luatools.work/GameBypasses/{appid}.zip"),
            install_root: install_root.clone(),
            backup_root: dir.join("backups"),
            backup_dir: dir.join("backups").join(appid),
            manifests: ManifestPlan {
                dlls: manifest_dlls(paths.iter().map(String::as_str)),
                launchers: manifest_launchers(paths.iter().map(String::as_str)),
                dll_manifest: dll_manifest_path(&install_root),
                launcher_manifest: launcher_manifest_path(&install_root),
            },
            writes,
            removals,
            directories,
            backups: Vec::new(),
        }
    }

    fn layout(entries: Vec<ArchiveEntry>) -> ArchiveLayout {
        ArchiveLayout::new(entries)
    }

    fn sample_layout() -> ArchiveLayout {
        layout(vec![
            ArchiveEntry::file("root.dll", b"root".to_vec()),
            ArchiveEntry::file("sub/Game.dll", b"game".to_vec()),
            ArchiveEntry::file("launcher.exe", b"launcher".to_vec()),
            ArchiveEntry::file("empty", Vec::new()),
            ArchiveEntry::directory("empty-dir/"),
        ])
        .with_nested(vec![NestedArchive::new(
            "pack.zip",
            vec![
                ArchiveEntry::file("nested/Hook.dll", b"hook".to_vec()),
                ArchiveEntry::file("nested/Sub_Launcher.EXE", b"nested".to_vec()),
            ],
        )])
    }

    #[test]
    fn path_validation_rejects_traversal_and_absolute_entries() {
        for raw in [
            "../evil.dll",
            "a/../../evil.dll",
            "..\\evil.dll",
            "/etc/passwd",
            "\\\\server\\share\\x.dll",
            "C:\\Windows\\x.dll",
            "C:/Windows/x.dll",
            "file.dll:stream",
            "nul\0.dll",
            "CON",
            "nul.txt",
            "trailing.",
            "trailing ",
            DLL_MANIFEST,
            "sub/.slssteam_fix_launchers",
            INSTALL_STATE_FILE,
        ] {
            assert!(
                validate_archive_path(raw).is_err(),
                "deveria rejeitar {raw:?}"
            );
        }
        let safe = validate_archive_path("sub\\Dir/ok.dll").unwrap();
        assert_eq!(safe.relative, "sub/Dir/ok.dll");
        assert_eq!(safe.path, Path::new("sub").join("Dir").join("ok.dll"));
        assert!(!safe.directory);
        assert!(validate_archive_path("dir/").unwrap().directory);
    }

    #[test]
    fn launcher_and_dll_classification_match_fix_downloader() {
        assert!(is_launcher("launcher.exe"));
        assert!(is_launcher("bin/Launcher.exe"));
        assert!(is_launcher("launcher_custom.exe"));
        assert!(is_launcher("Custom_Launcher.EXE"));
        assert!(!is_launcher("launcher"));
        assert!(!is_launcher("_launcher.exe"));
        assert_eq!(dll_basename("sub\\Game.DLL"), Some("Game.DLL"));
        assert_eq!(dll_basename("not.dll.txt"), None);
        assert!(is_secondary_volume("game.part02.rar"));
        assert!(is_secondary_volume("game.part012.rar"));
        assert!(!is_secondary_volume("part02.rar"));
        assert!(is_secondary_volume("game.part03.rar"));
        assert!(is_secondary_volume("game.r00"));
        assert!(is_secondary_volume("game.z99"));
        assert!(!is_secondary_volume("game.part01.rar"));
        assert!(!is_secondary_volume("game.rar"));
    }

    #[test]
    fn manifests_sort_dedupe_case_insensitively_and_keep_spelling() {
        let dlls = manifest_dlls(["b.DLL", "A.dll", "sub/a.DLL", "note.txt"]);
        assert_eq!(dlls, vec!["A.dll", "b.DLL"]);
        let launchers = manifest_launchers([
            "bin/Launcher.exe",
            "bin/launcher.EXE",
            "SubGame_launcher.exe",
            "readme.txt",
        ]);
        assert_eq!(launchers, vec!["bin/Launcher.exe", "SubGame_launcher.exe"]);
    }

    #[test]
    fn apply_writes_files_and_manifests_and_is_fixed() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("game")).unwrap();
        let fs = StdFixFs;
        let plan = plan_for(temp.path(), "123", &sample_layout());

        let outcome = apply(&fs, &plan).unwrap();
        assert!(!outcome.already_installed);
        assert_eq!(outcome.written, 6);
        assert_eq!(outcome.removed, 0);
        assert_eq!(
            std::fs::read(temp.path().join("game/sub/Game.dll")).unwrap(),
            b"game"
        );
        assert!(is_fixed(&fs, &temp.path().join("game")));
        let dlls = read_manifest(&fs, &plan.manifests.dll_manifest);
        assert_eq!(dlls, vec!["Game.dll", "Hook.dll", "root.dll"]);
        let launchers = read_manifest(&fs, &plan.manifests.launcher_manifest);
        assert_eq!(launchers, vec!["launcher.exe", "nested/Sub_Launcher.EXE"]);
        assert!(temp.path().join("game/empty-dir").is_dir());
        assert!(!temp.path().join("game/pack.zip").exists());
    }

    #[test]
    fn apply_is_idempotent_and_does_not_rewrite() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("game")).unwrap();
        let fs = StdFixFs;
        let plan = plan_for(temp.path(), "123", &sample_layout());
        apply(&fs, &plan).unwrap();
        let first = std::fs::read(temp.path().join("game/root.dll")).unwrap();

        let second = apply(&fs, &plan).unwrap();
        assert!(second.already_installed);
        assert_eq!(second.written, 0);
        assert_eq!(
            std::fs::read(temp.path().join("game/root.dll")).unwrap(),
            first
        );
        assert!(!temp.path().join("backups/123/files").exists());
    }

    #[test]
    fn unfix_restores_backups_and_removes_created_files() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("game/sub")).unwrap();
        std::fs::write(temp.path().join("game/root.dll"), b"original").unwrap();
        let fs = StdFixFs;
        let plan = plan_for(temp.path(), "123", &sample_layout());
        apply(&fs, &plan).unwrap();
        assert_eq!(
            std::fs::read(temp.path().join("game/root.dll")).unwrap(),
            b"root"
        );

        let outcome = unfix(
            &fs,
            &temp.path().join("game"),
            &temp.path().join("backups"),
            Some("123"),
        )
        .unwrap();
        assert_eq!(outcome.restored, 1);
        assert!(outcome.removed >= 4);
        assert_eq!(
            std::fs::read(temp.path().join("game/root.dll")).unwrap(),
            b"original"
        );
        assert!(!temp.path().join("game/sub/Game.dll").exists());
        assert!(!temp.path().join("game/empty-dir").exists());
        assert!(!temp.path().join("game/.slssteam_fix_dlls").exists());
        assert!(!temp.path().join("game/.slssteam_fix_launchers").exists());
        assert!(!temp.path().join("game/.arcadia_fix_state.json").exists());
        assert!(!is_fixed(&fs, &temp.path().join("game")));
    }

    #[test]
    fn unfix_without_state_only_removes_manifests() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("game")).unwrap();
        let fs = StdFixFs;
        std::fs::write(temp.path().join("game/.slssteam_fix_dlls"), b"x.dll\n").unwrap();
        std::fs::write(temp.path().join("game/.slssteam_fix_launchers"), b"l.exe\n").unwrap();
        std::fs::write(temp.path().join("game/keep.dll"), b"keep").unwrap();
        let outcome = unfix(
            &fs,
            &temp.path().join("game"),
            &temp.path().join("backups"),
            None,
        )
        .unwrap();
        assert_eq!(outcome.restored, 0);
        assert!(!temp.path().join("game/.slssteam_fix_dlls").exists());
        assert!(!temp.path().join("game/.slssteam_fix_launchers").exists());
        assert_eq!(
            std::fs::read(temp.path().join("game/keep.dll")).unwrap(),
            b"keep"
        );
    }

    #[test]
    fn unfix_removes_created_directories_and_backup_root() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("game")).unwrap();
        std::fs::write(temp.path().join("game/root.dll"), b"original").unwrap();
        let fs = StdFixFs;
        let layout = layout(vec![
            ArchiveEntry::file("a/b/deep.dll", b"deep".to_vec()),
            ArchiveEntry::file("root.dll", b"new".to_vec()),
        ]);
        let plan = plan_for(temp.path(), "123", &layout);
        apply(&fs, &plan).unwrap();
        assert!(temp.path().join("game/a/b/deep.dll").exists());
        assert!(temp.path().join("backups/123").exists());
        unfix(
            &fs,
            &temp.path().join("game"),
            &temp.path().join("backups"),
            Some("123"),
        )
        .unwrap();
        assert!(!temp.path().join("game/a").exists());
        assert!(!temp.path().join("backups/123").exists());
        assert_eq!(
            std::fs::read(temp.path().join("game/root.dll")).unwrap(),
            b"original"
        );
    }

    #[test]
    fn second_apply_without_dlls_is_still_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("game")).unwrap();
        let fs = StdFixFs;
        let plan = plan_for(
            temp.path(),
            "9",
            &layout(vec![ArchiveEntry::file("readme.txt", b"hello".to_vec())]),
        );
        assert!(!apply(&fs, &plan).unwrap().already_installed);
        assert!(apply(&fs, &plan).unwrap().already_installed);
        assert!(!is_fixed(&fs, &temp.path().join("game")));
    }

    #[test]
    fn state_with_extra_files_is_not_a_noop_for_a_different_plan() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("game")).unwrap();
        let fs = StdFixFs;
        let first = plan_for(
            temp.path(),
            "9",
            &layout(vec![ArchiveEntry::file("one.dll", b"1".to_vec())]),
        );
        apply(&fs, &first).unwrap();
        let second = plan_for(
            temp.path(),
            "9",
            &layout(vec![
                ArchiveEntry::file("one.dll", b"1".to_vec()),
                ArchiveEntry::file("two.dll", b"2".to_vec()),
            ]),
        );
        let outcome = apply(&fs, &second).unwrap();
        assert!(!outcome.already_installed);
        assert_eq!(outcome.written, 2);
        assert!(temp.path().join("game/two.dll").exists());
    }
    #[test]
    fn no_dlls_means_not_fixed_but_state_still_allows_unfix() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("game")).unwrap();
        let fs = StdFixFs;
        let plan = plan_for(
            temp.path(),
            "9",
            &layout(vec![ArchiveEntry::file("readme.txt", b"hello".to_vec())]),
        );
        apply(&fs, &plan).unwrap();
        assert!(!is_fixed(&fs, &temp.path().join("game")));
        assert!(temp.path().join("game/.arcadia_fix_state.json").exists());
        unfix(
            &fs,
            &temp.path().join("game"),
            &temp.path().join("backups"),
            Some("9"),
        )
        .unwrap();
        assert!(!temp.path().join("game/readme.txt").exists());
    }

    #[test]
    fn nested_archive_removal_backs_up_preexisting_file() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("game")).unwrap();
        std::fs::write(temp.path().join("game/pack.zip"), b"old-pack").unwrap();
        let fs = StdFixFs;
        let plan = plan_for(temp.path(), "123", &sample_layout());
        apply(&fs, &plan).unwrap();
        assert!(!temp.path().join("game/pack.zip").exists());
        unfix(
            &fs,
            &temp.path().join("game"),
            &temp.path().join("backups"),
            Some("123"),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(temp.path().join("game/pack.zip")).unwrap(),
            b"old-pack"
        );
    }

    struct FailingFs {
        inner: StdFixFs,
        writes: AtomicUsize,
        fail_on_write: usize,
    }

    impl FixFs for FailingFs {
        fn exists(&self, path: &Path) -> bool {
            self.inner.exists(path)
        }
        fn is_dir(&self, path: &Path) -> bool {
            self.inner.is_dir(path)
        }
        fn is_file(&self, path: &Path) -> bool {
            self.inner.is_file(path)
        }
        fn read(&self, path: &Path) -> FsResult<Vec<u8>> {
            self.inner.read(path)
        }
        fn write(&self, path: &Path, data: &[u8]) -> FsResult<()> {
            let count = self.writes.fetch_add(1, Ordering::SeqCst) + 1;
            if count == self.fail_on_write {
                return Err(FsError::new("injected write failure"));
            }
            self.inner.write(path, data)
        }
        fn create_dir_all(&self, path: &Path) -> FsResult<()> {
            self.inner.create_dir_all(path)
        }
        fn remove_file(&self, path: &Path) -> FsResult<()> {
            self.inner.remove_file(path)
        }
        fn remove_dir(&self, path: &Path) -> FsResult<()> {
            self.inner.remove_dir(path)
        }
        fn remove_dir_all(&self, path: &Path) -> FsResult<()> {
            self.inner.remove_dir_all(path)
        }
    }

    #[test]
    fn failure_mid_apply_rolls_back_backups_and_created_files() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("game")).unwrap();
        std::fs::write(temp.path().join("game/root.dll"), b"original").unwrap();
        let failing = Arc::new(FailingFs {
            inner: StdFixFs,
            writes: AtomicUsize::new(0),
            // 1 = backup de root.dll, 2 = root.dll novo,
            // 3 = sub/Game.dll (primeira escrita que falha)
            fail_on_write: 3,
        });
        let plan = plan_for(temp.path(), "123", &sample_layout());
        let error = apply(failing.as_ref(), &plan).unwrap_err();
        assert_eq!(error.0, "injected write failure");
        assert_eq!(
            std::fs::read(temp.path().join("game/root.dll")).unwrap(),
            b"original"
        );
        assert!(!temp.path().join("game/sub/Game.dll").exists());
        assert!(!temp.path().join("game/.slssteam_fix_dlls").exists());
        assert!(!temp.path().join("game/.arcadia_fix_state.json").exists());
    }

    #[test]
    fn apply_refuses_state_for_another_appid() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("game")).unwrap();
        let fs = StdFixFs;
        apply(&fs, &plan_for(temp.path(), "123", &sample_layout())).unwrap();
        let error = apply(
            &fs,
            &plan_for(
                temp.path(),
                "456",
                &layout(vec![ArchiveEntry::file("x.dll", b"x".to_vec())]),
            ),
        )
        .unwrap_err();
        assert!(error.0.contains("another fix"));
    }

    #[test]
    fn corrupt_state_blocks_apply_but_not_unfix() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("game")).unwrap();
        std::fs::write(
            temp.path().join("game/.arcadia_fix_state.json"),
            b"{not json",
        )
        .unwrap();
        std::fs::write(temp.path().join("game/.slssteam_fix_dlls"), b"x.dll\n").unwrap();
        let fs = StdFixFs;
        let plan = plan_for(temp.path(), "123", &sample_layout());
        assert_eq!(apply(&fs, &plan).unwrap_err().0, "fix state is invalid");
        let outcome = unfix(
            &fs,
            &temp.path().join("game"),
            &temp.path().join("backups"),
            Some("123"),
        )
        .unwrap();
        assert_eq!(outcome.unrecovered.len(), 0);
        assert!(!temp.path().join("game/.slssteam_fix_dlls").exists());
        assert!(!temp.path().join("game/.arcadia_fix_state.json").exists());
    }

    #[test]
    fn tampered_state_paths_cannot_escape_install_root() {
        let temp = tempfile::tempdir().unwrap();
        let game = temp.path().join("game");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(temp.path().join("outside.dll"), b"outside").unwrap();
        let state = serde_json::json!({
            "version": 1,
            "appid": "123",
            "type": "generic",
            "sourceUrl": "https://files.luatools.work/GameBypasses/123.zip",
            "files": [
                {"path": "../outside.dll", "kind": "written", "created": true, "backup": false, "size": 7},
                {"path": "/abs.dll", "kind": "written", "created": true, "backup": false, "size": 7}
            ],
            "directories": ["..", "../outside"]
        });
        std::fs::write(
            game.join(".arcadia_fix_state.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        let fs = StdFixFs;
        let outcome = unfix(&fs, &game, &temp.path().join("backups"), Some("123")).unwrap();
        assert_eq!(outcome.removed, 0);
        assert_eq!(
            std::fs::read(temp.path().join("outside.dll")).unwrap(),
            b"outside"
        );
        assert!(!game.join(".arcadia_fix_state.json").exists());
    }

    #[test]
    fn roots_reject_relative_traversal_and_outside_paths() {
        let temp = tempfile::tempdir().unwrap();
        let roots = test_roots(temp.path());
        assert!(roots
            .validate_install_path(temp.path().join("game").to_str().unwrap())
            .is_ok());
        assert!(roots.validate_install_path("relative/path").is_err());
        assert!(roots
            .validate_install_path(temp.path().join("game/../x").to_str().unwrap())
            .is_err());
        assert!(roots
            .validate_install_path(temp.path().join("other").to_str().unwrap())
            .is_err());
        assert!(roots
            .validate_install_path(temp.path().join("prefix").to_str().unwrap())
            .is_ok());
    }

    #[test]
    fn plan_paths_outside_install_root_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("game")).unwrap();
        let fs = StdFixFs;
        let mut plan = plan_for(temp.path(), "123", &sample_layout());
        plan.writes[0].target = temp.path().join("outside.dll");
        assert_eq!(
            apply(&fs, &plan).unwrap_err().0,
            "archive entry path rejected"
        );
    }
}
