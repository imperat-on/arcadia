//! Extratores de produção para os arquivos de fix.
//!
//! O oráculo (`fix_downloader.sh`) prefere `7zz x -bd -y -o<dir>` e cai para
//! `unzip -o -q <zip> -d <dir>` quando o 7-Zip não existe. Depois ele lista os
//! DLLs/launchers, extrai arquivos aninhados (`.zip`/`.rar`/`.7z`) e apaga os
//! volumes secundários. Este módulo reproduz esse fluxo em dois extratores:
//!
//! - [`ZipArchiveExtractor`]: lê ZIPs em memória com a crate `zip`, valida
//!   cada nome com [`crate::install::validate_archive_path`] (traversal,
//!   absoluto, NUL/controle, ADS, nomes reservados) e rejeita symlinks antes
//!   de qualquer escrita. ZIPs aninhados são abertos recursivamente; RAR/7z
//!   aninhados exigem o extrator de processo (erro explícito em vez de instalar
//!   o arquivo compactado no jogo).
//! - [`SevenZipArchiveExtractor`]: usa um [`ProcessRunner`] injetado
//!   (`7z`/`7zz` ou `unzip`), extrai para um diretório temporário único sob o
//!   data root, valida a listagem *antes* do spawn, percorre a árvore e rejeita
//!   symlinks, e processa arquivos aninhados como o script.
//!
//! Nenhum teste faz rede ou spawna processos: o runner é falso.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Cursor, Read, Seek};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use zip::ZipArchive;

use crate::install::{
    is_archive_file, is_secondary_volume, is_within, resolve_relative, validate_archive_path,
    ArchiveEntry, ArchiveExtractor, ArchiveLayout, InstallError, NestedArchive, SafeArchivePath,
    MAX_ARCHIVE_PATH_LEN,
};

/// Mensagem do `write_failed "The archive could not be opened."` do worker.
pub const ARCHIVE_OPEN_FAILED: &str = "The archive could not be opened.";
/// Symlink dentro do arquivo é rejeitado (nunca seguimos link).
pub const ARCHIVE_SYMLINK_REJECTED: &str = "archive contains a symbolic link";
/// Tipo de entrada que não é arquivo/diretório regular.
pub const ARCHIVE_ENTRY_UNSUPPORTED: &str = "archive entry type is unsupported";
/// ZIP aninhado com RAR/7z precisa do extrator de processo.
pub const NESTED_ARCHIVE_UNSUPPORTED: &str = "nested archive requires the 7-Zip extractor";
/// Limite de recursão de arquivos aninhados (proteção contra ciclos/bombas).
pub const ARCHIVE_DEPTH_EXCEEDED: &str = "nested archive depth exceeded";
/// Prefixo dos diretórios temporários de extração.
pub const EXTRACT_TMP_PREFIX: &str = "fix_extract";
/// Profundidade máxima de aninhamento aceita por padrão.
pub const MAX_NESTED_DEPTH: usize = 8;

// ─── processo injetado ─────────────────────────────────────────────────────

/// Programa + argv. Cada argumento é um item separado; nenhuma string de
/// shell é interpolada (mesmo contrato do `spawn(cmd, args)` do Electron).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
}

impl ProcessSpec {
    pub fn new<I, S>(program: impl Into<String>, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            cwd: None,
            env: Vec::new(),
        }
    }

    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

/// Resultado de um processo já aguardado (stdout/stderr capturados).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProcessOutcome {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl ProcessOutcome {
    pub fn success() -> Self {
        Self {
            code: Some(0),
            ..Self::default()
        }
    }

    pub fn failure(code: i32) -> Self {
        Self {
            code: Some(code),
            ..Self::default()
        }
    }

    pub fn is_success(&self) -> bool {
        self.code == Some(0)
    }
}

/// Runner síncrono injetável. Extratores só precisam de `run`; testes
/// registram argv e simulam a árvore extraída sem criar processo real.
pub trait ProcessRunner: Send + Sync {
    fn run(&self, spec: &ProcessSpec) -> Result<ProcessOutcome, String>;
}

/// Runner de produção: `std::process::Command`, sem shell.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemProcessRunner;

impl ProcessRunner for SystemProcessRunner {
    fn run(&self, spec: &ProcessSpec) -> Result<ProcessOutcome, String> {
        if spec.program.trim().is_empty() {
            return Err("process without program".to_string());
        }
        let mut command = Command::new(&spec.program);
        command.args(&spec.args).stdin(std::process::Stdio::null());
        if let Some(cwd) = &spec.cwd {
            command.current_dir(cwd);
        }
        for (key, value) in &spec.env {
            command.env(key, value);
        }
        // O worker limpa o runtime do Steam antes de chamar 7z/unzip.
        for key in [
            "LD_LIBRARY_PATH",
            "LD_PRELOAD",
            "LD_AUDIT",
            "STEAM_RUNTIME_LIBRARY_PATH",
            "STEAM_ZENITY",
        ] {
            command.env_remove(key);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let output = command.output().map_err(|error| error.to_string())?;
        Ok(ProcessOutcome {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

// ─── descoberta de ferramentas (sem spawn) ─────────────────────────────────

fn find_on_path(names: &[&str]) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        for name in names {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// Localiza `7z`/`7zz` como o `find7z()` do JS (sem executar nada).
pub fn find_seven_zip() -> Option<String> {
    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            for name in if cfg!(windows) {
                ["7z.exe", "7zz.exe"].as_slice()
            } else {
                ["7zz", "7z"].as_slice()
            } {
                candidates.push(directory.join("bin").join(name));
                candidates.push(directory.join(name));
            }
        }
    }
    #[cfg(windows)]
    {
        for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(base) = std::env::var_os(variable) {
                candidates.push(PathBuf::from(base).join("7-Zip").join("7z.exe"));
            }
        }
    }
    #[cfg(not(windows))]
    {
        for path in [
            "/usr/bin/7zz",
            "/usr/bin/7z",
            "/usr/local/bin/7zz",
            "/usr/local/bin/7z",
        ] {
            candidates.push(PathBuf::from(path));
        }
    }
    for candidate in candidates {
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    find_on_path(if cfg!(windows) {
        &["7z.exe", "7zz.exe"]
    } else {
        &["7zz", "7z"]
    })
}

/// Localiza o `unzip` de fallback (sem executar nada).
pub fn find_unzip() -> Option<String> {
    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            candidates.push(directory.join("bin").join("unzip"));
        }
    }
    #[cfg(not(windows))]
    {
        for path in ["/usr/bin/unzip", "/bin/unzip"] {
            candidates.push(PathBuf::from(path));
        }
    }
    for candidate in candidates {
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    find_on_path(if cfg!(windows) {
        &["unzip.exe", "unzip"]
    } else {
        &["unzip"]
    })
}

// ─── ferramenta de linha de comando ────────────────────────────────────────

/// Ferramenta usada pelo [`SevenZipArchiveExtractor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveTool {
    SevenZip,
    Unzip,
}

impl ArchiveTool {
    fn list_args(self, archive: &Path) -> Vec<String> {
        match self {
            ArchiveTool::SevenZip => vec![
                "l".into(),
                "-ba".into(),
                "-slt".into(),
                archive.to_string_lossy().into_owned(),
            ],
            ArchiveTool::Unzip => vec!["-Z1".into(), archive.to_string_lossy().into_owned()],
        }
    }

    fn extract_args(self, archive: &Path, destination: &Path) -> Vec<String> {
        let archive = archive.to_string_lossy().into_owned();
        let destination = destination.to_string_lossy().into_owned();
        match self {
            ArchiveTool::SevenZip => vec![
                "x".into(),
                "-bd".into(),
                "-y".into(),
                format!("-o{destination}"),
                archive,
            ],
            ArchiveTool::Unzip => {
                vec!["-o".into(), "-q".into(), archive, "-d".into(), destination]
            }
        }
    }

    fn parse_list(self, stdout: &str) -> Result<Vec<String>, InstallError> {
        match self {
            ArchiveTool::SevenZip => {
                let mut after_separator = false;
                let mut paths = Vec::new();
                let mut all_paths = Vec::new();
                for line in stdout.lines() {
                    let line = line.trim_end_matches('\r');
                    if line.starts_with("----------") {
                        after_separator = true;
                        continue;
                    }
                    if line.starts_with("Symbolic Link = ") {
                        return Err(InstallError::new(ARCHIVE_SYMLINK_REJECTED));
                    }
                    if let Some(path) = line.strip_prefix("Path = ") {
                        all_paths.push(path.to_string());
                        if after_separator {
                            paths.push(path.to_string());
                        }
                    }
                }
                if !after_separator {
                    // Alguns formatos não imprimem o separador; a primeira
                    // entrada é o próprio arquivo.
                    paths = all_paths.into_iter().skip(1).collect();
                }
                Ok(paths)
            }
            ArchiveTool::Unzip => Ok(stdout
                .lines()
                .map(|line| line.trim_end_matches('\r'))
                .filter(|line| !line.trim().is_empty())
                .map(str::to_string)
                .collect()),
        }
    }
}

// ─── diretório temporário de extração ──────────────────────────────────────

static EXTRACT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Diretório único sob o data root; removido no `Drop`, inclusive em erro.
struct TempExtractDir {
    path: PathBuf,
}

impl TempExtractDir {
    fn create(root: &Path) -> Result<Self, InstallError> {
        if !root.is_absolute()
            || root
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(InstallError::new("extract temp root rejected"));
        }
        fs::create_dir_all(root)
            .map_err(|_| InstallError::new("extract temp dir could not be created"))?;
        for _ in 0..64 {
            let sequence = EXTRACT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = root.join(format!(
                "{EXTRACT_TMP_PREFIX}_{}_{}",
                std::process::id(),
                sequence
            ));
            if !is_within(root, &path) {
                continue;
            }
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(InstallError::new("extract temp dir could not be created")),
            }
        }
        Err(InstallError::new("extract temp dir could not be created"))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempExtractDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

// ─── extrator 7z/unzip ─────────────────────────────────────────────────────

/// Extrator por processo (`7z`/`7zz`/`unzip`) com runner injetado.
pub struct SevenZipArchiveExtractor {
    runner: Arc<dyn ProcessRunner>,
    program: String,
    tool: ArchiveTool,
    temp_root: PathBuf,
    max_nested_depth: usize,
}

impl std::fmt::Debug for SevenZipArchiveExtractor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SevenZipArchiveExtractor")
            .field("program", &self.program)
            .field("tool", &self.tool)
            .field("temp_root", &self.temp_root)
            .field("max_nested_depth", &self.max_nested_depth)
            .finish_non_exhaustive()
    }
}

impl SevenZipArchiveExtractor {
    /// Extrator 7-Zip (`x -bd -y -o...`).
    pub fn new(
        runner: Arc<dyn ProcessRunner>,
        program: impl Into<String>,
        temp_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            runner,
            program: program.into(),
            tool: ArchiveTool::SevenZip,
            temp_root: temp_root.into(),
            max_nested_depth: MAX_NESTED_DEPTH,
        }
    }

    /// Extrator `unzip -o -q ... -d ...` (fallback do worker).
    pub fn unzip(
        runner: Arc<dyn ProcessRunner>,
        program: impl Into<String>,
        temp_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            runner,
            program: program.into(),
            tool: ArchiveTool::Unzip,
            temp_root: temp_root.into(),
            max_nested_depth: MAX_NESTED_DEPTH,
        }
    }

    /// Produção quando há 7z/7zz; `None` sem a ferramenta (nunca spawna para
    /// descobrir).
    pub fn production(temp_root: impl Into<PathBuf>) -> Option<Self> {
        find_seven_zip()
            .map(|program| Self::new(Arc::new(SystemProcessRunner), program, temp_root.into()))
    }

    /// Produção com `unzip`, quando não há 7-Zip.
    pub fn unzip_production(temp_root: impl Into<PathBuf>) -> Option<Self> {
        find_unzip()
            .map(|program| Self::unzip(Arc::new(SystemProcessRunner), program, temp_root.into()))
    }

    pub fn with_max_nested_depth(mut self, depth: usize) -> Self {
        self.max_nested_depth = depth;
        self
    }

    pub fn program(&self) -> &str {
        &self.program
    }

    pub fn tool(&self) -> ArchiveTool {
        self.tool
    }

    pub fn temp_root(&self) -> &Path {
        &self.temp_root
    }

    fn list_archive(&self, archive: &Path) -> Result<Vec<SafeArchivePath>, InstallError> {
        let spec = ProcessSpec::new(&self.program, self.tool.list_args(archive));
        let outcome = self
            .runner
            .run(&spec)
            .map_err(|error| InstallError::new(format!("archive tool: {error}")))?;
        if !outcome.is_success() {
            return Err(InstallError::new(ARCHIVE_OPEN_FAILED));
        }
        let paths = self.tool.parse_list(&outcome.stdout)?;
        let mut listed = Vec::with_capacity(paths.len());
        for raw in paths {
            if raw.len() > MAX_ARCHIVE_PATH_LEN {
                return Err(InstallError::new("archive entry path rejected"));
            }
            listed.push(validate_archive_path(&raw)?);
        }
        Ok(listed)
    }

    fn run_extract(&self, archive: &Path, destination: &Path) -> Result<(), InstallError> {
        let archive =
            std::path::absolute(archive).map_err(|_| InstallError::new(ARCHIVE_OPEN_FAILED))?;
        let spec = ProcessSpec::new(&self.program, self.tool.extract_args(&archive, destination));
        let outcome = self
            .runner
            .run(&spec)
            .map_err(|error| InstallError::new(format!("archive tool: {error}")))?;
        if !outcome.is_success() {
            return Err(InstallError::new(ARCHIVE_OPEN_FAILED));
        }
        Ok(())
    }

    fn extract_into(
        &self,
        archive: &Path,
        destination: &Path,
    ) -> Result<ArchiveLayout, InstallError> {
        let listing = self.list_archive(archive)?;
        let nested_top: Vec<String> = listing
            .iter()
            .filter(|entry| is_nested_candidate(entry))
            .map(|entry| entry.relative.clone())
            .collect();
        self.run_extract(archive, destination)?;

        let skip_data: HashSet<String> = listing
            .iter()
            .filter(|entry| !entry.directory && is_archive_file(&entry.relative))
            .map(|entry| entry.relative.clone())
            .collect();
        let mut entries = walk_tree(destination, &skip_data)?;

        // Volumes secundários nunca são instalados (o worker os apaga).
        entries.retain(|entry| entry.directory || !is_secondary_volume(&entry.path));

        let mut nested = Vec::new();
        let mut seen = HashSet::new();
        for path in nested_top {
            self.process_nested(destination, &path, 0, &mut nested, &mut seen)?;
        }
        Ok(ArchiveLayout { entries, nested })
    }

    fn process_nested(
        &self,
        root: &Path,
        relative: &str,
        depth: usize,
        nested: &mut Vec<NestedArchive>,
        seen: &mut HashSet<String>,
    ) -> Result<(), InstallError> {
        if !seen.insert(relative.to_string()) {
            return Ok(());
        }
        if depth >= self.max_nested_depth {
            return Err(InstallError::new(ARCHIVE_DEPTH_EXCEEDED));
        }
        let Some(archive) = resolve_relative(root, relative) else {
            return Err(InstallError::new("archive entry path rejected"));
        };
        if !archive.is_file() {
            return Ok(());
        }
        let listing = match self.list_archive(&archive) {
            Ok(listing) => listing,
            // O worker ignora falha ao abrir/extrair aninhado.
            Err(error) if error.0 == ARCHIVE_OPEN_FAILED => return Ok(()),
            Err(error) => return Err(error),
        };
        let child_archives: Vec<String> = listing
            .iter()
            .filter(|entry| is_nested_candidate(entry))
            .map(|entry| entry.relative.clone())
            .collect();
        if self.run_extract(&archive, root).is_err() {
            return Ok(());
        }

        let mut entries = Vec::new();
        for entry in &listing {
            if entry.directory {
                entries.push(ArchiveEntry::directory(entry.relative.clone()));
                continue;
            }
            if is_secondary_volume(&entry.relative) || is_archive_file(&entry.relative) {
                continue;
            }
            let Some(target) = resolve_relative(root, &entry.relative) else {
                return Err(InstallError::new("archive entry path rejected"));
            };
            let data = fs::read(&target).map_err(|_| InstallError::new(ARCHIVE_OPEN_FAILED))?;
            entries.push(ArchiveEntry::file(entry.relative.clone(), data));
        }
        nested.push(NestedArchive::new(relative.to_string(), entries));

        for child in child_archives {
            self.process_nested(root, &child, depth + 1, nested, seen)?;
        }
        Ok(())
    }
}

impl ArchiveExtractor for SevenZipArchiveExtractor {
    fn extract(&self, archive: &Path) -> Result<ArchiveLayout, InstallError> {
        if !archive.is_file() {
            return Err(InstallError::new(ARCHIVE_OPEN_FAILED));
        }
        let temp = TempExtractDir::create(&self.temp_root)?;
        self.extract_into(archive, temp.path())
    }
}

fn is_nested_candidate(entry: &SafeArchivePath) -> bool {
    !entry.directory && is_archive_file(&entry.relative) && !is_secondary_volume(&entry.relative)
}

fn walk_tree(root: &Path, skip_data: &HashSet<String>) -> Result<Vec<ArchiveEntry>, InstallError> {
    let mut entries = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let mut items = fs::read_dir(&directory)
            .map_err(|_| InstallError::new(ARCHIVE_OPEN_FAILED))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| InstallError::new(ARCHIVE_OPEN_FAILED))?;
        items.sort_by_key(|entry| entry.file_name());
        for item in items {
            let path = item.path();
            let metadata =
                fs::symlink_metadata(&path).map_err(|_| InstallError::new(ARCHIVE_OPEN_FAILED))?;
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                return Err(InstallError::new(ARCHIVE_SYMLINK_REJECTED));
            }
            let relative = relative_string(root, &path)?;
            let safe = validate_archive_path(&relative)?;
            if file_type.is_dir() {
                entries.push(ArchiveEntry::directory(safe.relative.clone()));
                pending.push(path);
            } else if file_type.is_file() {
                let data = if skip_data.contains(&safe.relative) {
                    Vec::new()
                } else {
                    fs::read(&path).map_err(|_| InstallError::new(ARCHIVE_OPEN_FAILED))?
                };
                entries.push(ArchiveEntry::file(safe.relative, data));
            } else {
                return Err(InstallError::new(ARCHIVE_ENTRY_UNSUPPORTED));
            }
        }
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

fn relative_string(root: &Path, path: &Path) -> Result<String, InstallError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| InstallError::new("archive entry path rejected"))?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_string_lossy().into_owned()),
            _ => return Err(InstallError::new("archive entry path rejected")),
        }
    }
    if parts.is_empty() {
        return Err(InstallError::new("archive entry path rejected"));
    }
    Ok(parts.join("/"))
}

// ─── extrator ZIP puro ─────────────────────────────────────────────────────

/// Extrator ZIP em memória (fallback sem 7-Zip) com validação prévia.
#[derive(Debug, Clone, Copy)]
pub struct ZipArchiveExtractor {
    max_nested_depth: usize,
}

impl Default for ZipArchiveExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl ZipArchiveExtractor {
    pub fn new() -> Self {
        Self {
            max_nested_depth: MAX_NESTED_DEPTH,
        }
    }

    pub fn with_max_nested_depth(mut self, depth: usize) -> Self {
        self.max_nested_depth = depth;
        self
    }

    fn read_archive<R: Read + Seek>(
        &self,
        reader: R,
        depth: usize,
    ) -> Result<ArchiveLayout, InstallError> {
        let mut archive =
            ZipArchive::new(reader).map_err(|_| InstallError::new(ARCHIVE_OPEN_FAILED))?;
        let mut entries = Vec::new();
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .map_err(|_| InstallError::new(ARCHIVE_OPEN_FAILED))?;
            if entry.name_raw().contains(&0) {
                return Err(InstallError::new("archive entry path rejected"));
            }
            if entry.is_symlink() {
                return Err(InstallError::new(ARCHIVE_SYMLINK_REJECTED));
            }
            if let Some(mode) = entry.unix_mode() {
                let kind = mode & 0o170000;
                if kind != 0 && kind != 0o040000 && kind != 0o100000 {
                    return Err(InstallError::new(ARCHIVE_ENTRY_UNSUPPORTED));
                }
            }
            let safe = validate_archive_path(entry.name())?;
            if entry.is_dir() || safe.directory {
                entries.push(ArchiveEntry::directory(safe.relative));
                continue;
            }
            let mut data = Vec::new();
            entry
                .read_to_end(&mut data)
                .map_err(|_| InstallError::new(ARCHIVE_OPEN_FAILED))?;
            entries.push(ArchiveEntry::file(safe.relative, data));
        }

        let mut nested = Vec::new();
        for entry in &mut entries {
            if entry.directory || !is_archive_file(&entry.path) || is_secondary_volume(&entry.path)
            {
                continue;
            }
            if !entry.path.to_ascii_lowercase().ends_with(".zip") {
                return Err(InstallError::new(NESTED_ARCHIVE_UNSUPPORTED));
            }
            if depth >= self.max_nested_depth {
                return Err(InstallError::new(ARCHIVE_DEPTH_EXCEEDED));
            }
            let data = std::mem::take(&mut entry.data);
            let inner = self.read_archive(Cursor::new(data), depth + 1)?;
            let inner_entries = inner
                .entries
                .into_iter()
                .filter(|nested_entry| {
                    nested_entry.directory || !is_secondary_volume(&nested_entry.path)
                })
                .collect();
            nested.push(NestedArchive::new(entry.path.clone(), inner_entries));
            nested.extend(inner.nested);
        }
        Ok(ArchiveLayout { entries, nested })
    }
}

impl ArchiveExtractor for ZipArchiveExtractor {
    fn extract(&self, archive: &Path) -> Result<ArchiveLayout, InstallError> {
        let file = File::open(archive).map_err(|_| InstallError::new(ARCHIVE_OPEN_FAILED))?;
        self.read_archive(file, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Mutex;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            if name.ends_with('/') {
                writer
                    .add_directory(*name, SimpleFileOptions::default())
                    .unwrap();
            } else {
                writer
                    .start_file(*name, SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(data).unwrap();
            }
        }
        writer.finish().unwrap().into_inner()
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        fs::write(path, zip_bytes(entries)).unwrap();
    }

    fn entry<'a>(layout: &'a ArchiveLayout, path: &str) -> Option<&'a ArchiveEntry> {
        layout.entries.iter().find(|entry| entry.path == path)
    }

    fn nested<'a>(layout: &'a ArchiveLayout, path: &str) -> Option<&'a NestedArchive> {
        layout.nested.iter().find(|nested| nested.path == path)
    }

    fn extract_temp_dirs(root: &Path) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(root) else {
            return Vec::new();
        };
        entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(EXTRACT_TMP_PREFIX))
            })
            .collect()
    }

    #[derive(Default)]
    struct FakeRunner {
        calls: Mutex<Vec<ProcessSpec>>,
        traversal: bool,
        symlink: bool,
    }

    impl FakeRunner {
        fn calls(&self) -> Vec<ProcessSpec> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl ProcessRunner for FakeRunner {
        fn run(&self, spec: &ProcessSpec) -> Result<ProcessOutcome, String> {
            self.calls.lock().unwrap().push(spec.clone());
            let first = spec.args.first().map(String::as_str).unwrap_or_default();
            let archive = match first {
                "-Z1" => spec.args.get(1),
                "-o" => spec.args.get(2),
                _ => spec.args.last(),
            }
            .cloned()
            .unwrap_or_default();
            let name = Path::new(&archive)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();

            if first == "l" || first == "-Z1" {
                if self.traversal {
                    return Ok(ProcessOutcome {
                        code: Some(0),
                        stdout: "Path = fix.7z\nType = 7z\n----------\nPath = ../evil.dll\n".into(),
                        stderr: String::new(),
                    });
                }
                if self.symlink {
                    return Ok(ProcessOutcome {
                        code: Some(0),
                        stdout: "Path = fix.7z\n----------\nPath = link.dll\nSymbolic Link = /etc/passwd\n"
                            .into(),
                        stderr: String::new(),
                    });
                }
                let stdout = if first == "-Z1" {
                    match name.as_str() {
                        "fix.zip" => "root.dll\nsub/Game.dll\n",
                        _ => "",
                    }
                } else {
                    match name.as_str() {
                        "fix.7z" => concat!(
                            "Path = fix.7z\nType = 7z\n",
                            "----------\nPath = root.dll\nSize = 4\n",
                            "----------\nPath = sub\\Game.dll\nSize = 4\n",
                            "----------\nPath = pack.zip\nSize = 12\n",
                        ),
                        "pack.zip" => concat!(
                            "Path = pack.zip\nType = zip\n",
                            "----------\nPath = nested\\Hook.dll\nSize = 4\n",
                        ),
                        _ => "",
                    }
                };
                return Ok(ProcessOutcome {
                    code: Some(0),
                    stdout: stdout.into(),
                    stderr: String::new(),
                });
            }

            if first == "x" {
                let destination = spec
                    .args
                    .iter()
                    .find_map(|arg| arg.strip_prefix("-o"))
                    .map(PathBuf::from)
                    .unwrap_or_default();
                match name.as_str() {
                    "fix.7z" => {
                        write_file(&destination, "root.dll", b"root");
                        write_file(&destination, "sub/Game.dll", b"game");
                        write_file(&destination, "pack.zip", b"nested-archive");
                    }
                    "pack.zip" => {
                        write_file(&destination, "nested/Hook.dll", b"hook");
                    }
                    _ => {}
                }
                return Ok(ProcessOutcome::success());
            }

            if first == "-o" {
                let destination = spec.args.get(4).map(PathBuf::from).unwrap_or_default();
                if name.as_str() == "fix.zip" {
                    write_file(&destination, "root.dll", b"root");
                    write_file(&destination, "sub/Game.dll", b"game");
                }
                return Ok(ProcessOutcome::success());
            }

            Ok(ProcessOutcome::failure(1))
        }
    }

    fn write_file(root: &Path, relative: &str, data: &[u8]) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, data).unwrap();
    }

    #[test]
    fn zip_layout_keeps_files_directories_and_nested_zip() {
        let temp = tempfile::tempdir().unwrap();
        let nested_zip = zip_bytes(&[("nested/Hook.dll", b"hook")]);
        let archive = temp.path().join("fix.zip");
        write_zip(
            &archive,
            &[
                ("root.dll", b"root"),
                ("sub/Game.dll", b"game"),
                ("empty-dir/", b""),
                ("pack.zip", nested_zip.as_slice()),
            ],
        );

        let layout = ZipArchiveExtractor::new().extract(&archive).unwrap();
        let root = entry(&layout, "root.dll").unwrap();
        assert_eq!(root.data, b"root");
        let game = entry(&layout, "sub/Game.dll").unwrap();
        assert_eq!(game.data, b"game");
        assert!(entry(&layout, "empty-dir").unwrap().directory);
        let pack = entry(&layout, "pack.zip").unwrap();
        assert!(pack.data.is_empty(), "dados do ZIP aninhado não duplicam");

        let nested = nested(&layout, "pack.zip").unwrap();
        let hook = nested
            .entries
            .iter()
            .find(|entry| entry.path == "nested/Hook.dll")
            .unwrap();
        assert_eq!(hook.data, b"hook");
    }

    #[test]
    fn zip_rejects_traversal_absolute_ads_and_reserved_entries() {
        let temp = tempfile::tempdir().unwrap();
        for (index, name) in [
            "../evil.dll",
            "a/../../evil.dll",
            "/etc/passwd",
            "C:/Windows/x.dll",
            "file.dll:stream",
            "CON",
        ]
        .into_iter()
        .enumerate()
        {
            let archive = temp.path().join(format!("bad-{index}.zip"));
            write_zip(&archive, &[(name, b"x")]);
            let error = ZipArchiveExtractor::new().extract(&archive).unwrap_err();
            assert!(
                error.0.contains("archive entry path rejected"),
                "{name}: {error}"
            );
        }
    }

    #[test]
    fn zip_rejects_symlink_entries() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("link.zip");
        let mut writer = ZipWriter::new(File::create(&archive).unwrap());
        writer
            .add_symlink("link.dll", "/etc/passwd", SimpleFileOptions::default())
            .unwrap();
        writer.finish().unwrap();

        let error = ZipArchiveExtractor::new().extract(&archive).unwrap_err();
        assert!(error.0.contains("symbolic link"), "{error}");
    }

    #[test]
    fn zip_rejects_non_zip_nested_archives_instead_of_installing_them() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("fix.zip");
        write_zip(&archive, &[("pack.rar", b"not really rar")]);
        let error = ZipArchiveExtractor::new().extract(&archive).unwrap_err();
        assert_eq!(error.0, NESTED_ARCHIVE_UNSUPPORTED);
    }

    #[test]
    fn seven_zip_runs_expected_argv_and_builds_layout_with_nested() {
        let temp = tempfile::tempdir().unwrap();
        let runner = Arc::new(FakeRunner::default());
        let temp_root = temp.path().join("data/tmp");
        let extractor = SevenZipArchiveExtractor::new(runner.clone(), "7z", temp_root.clone());
        let archive = temp.path().join("fix.7z");
        fs::write(&archive, b"fake archive").unwrap();

        let layout = extractor.extract(&archive).unwrap();
        assert_eq!(entry(&layout, "root.dll").unwrap().data, b"root");
        assert_eq!(entry(&layout, "sub/Game.dll").unwrap().data, b"game");
        let pack = entry(&layout, "pack.zip").unwrap();
        assert!(pack.data.is_empty());
        let nested = nested(&layout, "pack.zip").unwrap();
        assert_eq!(
            nested
                .entries
                .iter()
                .find(|entry| entry.path == "nested/Hook.dll")
                .unwrap()
                .data,
            b"hook"
        );
        assert!(extract_temp_dirs(&temp_root).is_empty());

        let calls = runner.calls();
        let listing = calls.first().unwrap();
        assert_eq!(listing.program, "7z");
        assert_eq!(listing.args[0], "l");
        assert_eq!(listing.args[1], "-ba");
        assert_eq!(listing.args[2], "-slt");
        assert!(listing.args[3].ends_with("fix.7z"));

        let extract = calls
            .iter()
            .find(|call| call.args.first().map(String::as_str) == Some("x"))
            .unwrap();
        assert_eq!(extract.program, "7z");
        assert_eq!(&extract.args[..3], ["x", "-bd", "-y"]);
        assert!(extract.args[3].starts_with("-o"));
        assert!(extract.args[4].ends_with("fix.7z"));

        let nested_extract = calls
            .iter()
            .filter(|call| call.args.first().map(String::as_str) == Some("x"))
            .find(|call| call.args[4].ends_with("pack.zip"))
            .unwrap();
        assert!(nested_extract.args[3].starts_with("-o"));
        assert_eq!(calls.len(), 4, "list/extract top + list/extract nested");
    }

    #[test]
    fn seven_zip_rejects_traversal_listing_before_spawning_extract() {
        let temp = tempfile::tempdir().unwrap();
        let runner = Arc::new(FakeRunner {
            traversal: true,
            ..FakeRunner::default()
        });
        let extractor =
            SevenZipArchiveExtractor::new(runner.clone(), "7z", temp.path().join("data/tmp"));
        let archive = temp.path().join("fix.7z");
        fs::write(&archive, b"fake archive").unwrap();

        let error = extractor.extract(&archive).unwrap_err();
        assert!(error.0.contains("archive entry path rejected"), "{error}");
        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].args[0], "l");
        assert!(!calls.iter().any(|call| call.args[0] == "x"));
    }

    #[test]
    fn seven_zip_rejects_symlink_listing_before_extract() {
        let temp = tempfile::tempdir().unwrap();
        let runner = Arc::new(FakeRunner {
            symlink: true,
            ..FakeRunner::default()
        });
        let extractor =
            SevenZipArchiveExtractor::new(runner.clone(), "7z", temp.path().join("data/tmp"));
        let archive = temp.path().join("fix.7z");
        fs::write(&archive, b"fake archive").unwrap();

        let error = extractor.extract(&archive).unwrap_err();
        assert!(error.0.contains("symbolic link"), "{error}");
        assert!(!runner.calls().iter().any(|call| call.args[0] == "x"));
    }

    #[test]
    fn unzip_tool_uses_worker_argv_and_layout() {
        let temp = tempfile::tempdir().unwrap();
        let runner = Arc::new(FakeRunner::default());
        let temp_root = temp.path().join("data/tmp");
        let extractor = SevenZipArchiveExtractor::unzip(runner.clone(), "unzip", temp_root.clone());
        let archive = temp.path().join("fix.zip");
        fs::write(&archive, b"fake archive").unwrap();

        let layout = extractor.extract(&archive).unwrap();
        assert_eq!(entry(&layout, "root.dll").unwrap().data, b"root");
        assert_eq!(entry(&layout, "sub/Game.dll").unwrap().data, b"game");
        assert!(extract_temp_dirs(&temp_root).is_empty());

        let calls = runner.calls();
        assert_eq!(calls[0].program, "unzip");
        assert_eq!(calls[0].args[0], "-Z1");
        let extract = calls
            .iter()
            .find(|call| call.args.first().map(String::as_str) == Some("-o"))
            .unwrap();
        assert_eq!(extract.args[0], "-o");
        assert_eq!(extract.args[1], "-q");
        assert!(extract.args[2].ends_with("fix.zip"));
        assert_eq!(extract.args[3], "-d");
        assert!(!extract.args[4].is_empty());
    }

    #[test]
    fn missing_archive_has_stable_error() {
        let temp = tempfile::tempdir().unwrap();
        let extractor = ZipArchiveExtractor::new();
        let error = extractor
            .extract(&temp.path().join("missing.zip"))
            .unwrap_err();
        assert_eq!(error.0, ARCHIVE_OPEN_FAILED);
        let runner = Arc::new(FakeRunner::default());
        let extractor = SevenZipArchiveExtractor::new(runner, "7z", temp.path().join("data/tmp"));
        let error = extractor
            .extract(&temp.path().join("missing.7z"))
            .unwrap_err();
        assert_eq!(error.0, ARCHIVE_OPEN_FAILED);
    }

    #[test]
    fn temp_root_must_be_absolute_and_is_cleaned_on_failure() {
        let temp = tempfile::tempdir().unwrap();
        let runner = Arc::new(FakeRunner::default());
        let extractor = SevenZipArchiveExtractor::new(runner, "7z", PathBuf::from("relative"));
        let archive = temp.path().join("fix.7z");
        fs::write(&archive, b"fake archive").unwrap();
        let error = extractor.extract(&archive).unwrap_err();
        assert_eq!(error.0, "extract temp root rejected");

        let runner = Arc::new(FakeRunner {
            traversal: true,
            ..FakeRunner::default()
        });
        let temp_root = temp.path().join("data/tmp");
        let extractor = SevenZipArchiveExtractor::new(runner, "7z", temp_root.clone());
        let error = extractor.extract(&archive).unwrap_err();
        assert!(error.0.contains("archive entry path rejected"));
        assert!(extract_temp_dirs(&temp_root).is_empty());
    }
}
