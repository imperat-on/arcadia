//! Probes injetáveis de filesystem, caminho e relógio.
//!
//! O cliente Electron usa `node:fs` síncrono em todos os módulos de emulador.
//! Aqui o mesmo conjunto de operações é exposto por [`EmulatorFs`], para que os
//! testes usem diretórios temporários ou [`MemoryFs`] sem tocar no home do
//! usuário. As funções de caminho reproduzem `node:path`
//! (`normalize`, `isAbsolute`, `basename`, `extname`, `delimiter`) e a
//! semântica de `String#trim`, `String#slice` e `localeCompare` usada pelo
//! oráculo.

use std::cmp::Ordering;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering as AtomicOrdering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

/// Sistema operacional de destino (equivalente a `process.platform`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlatformKind {
    Windows,
    Linux,
    MacOs,
    Other,
}

impl PlatformKind {
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Other
        }
    }

    /// Converte o valor de `process.platform` (`"win32"`, `"linux"`...).
    pub fn from_js(value: &str) -> Self {
        match value {
            "win32" => Self::Windows,
            "linux" => Self::Linux,
            "darwin" => Self::MacOs,
            _ => Self::Other,
        }
    }

    pub fn is_windows(self) -> bool {
        self == Self::Windows
    }

    pub fn is_linux(self) -> bool {
        self == Self::Linux
    }

    /// Separador de listas de `PATH`. No Node ele é do host, não do parâmetro
    /// `platform`; manter o host evita quebrar testes que exercitam a regra
    /// Linux em uma máquina Windows com caminhos locais.
    pub fn path_delimiter() -> char {
        if cfg!(windows) {
            ';'
        } else {
            ':'
        }
    }

    pub fn path_separator() -> char {
        std::path::MAIN_SEPARATOR
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsKind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FsMeta {
    pub kind: FsKind,
    pub size: u64,
    pub mtime_ms: i64,
    pub mode: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsEntry {
    pub name: String,
    pub kind: FsKind,
}

impl FsEntry {
    pub fn is_symlink(&self) -> bool {
        self.kind == FsKind::Symlink
    }
}

/// Operações síncronas usadas pelo registry/scanner. Through `Arc<dyn>`, the
/// registry owns one probe for its lifetime.
pub trait EmulatorFs: Send + Sync {
    fn lstat(&self, path: &Path) -> Option<FsMeta>;
    /// `fs.statSync`: segue symlinks.
    fn stat(&self, path: &Path) -> Option<FsMeta> {
        self.lstat(path)
    }
    fn read_dir(&self, path: &Path) -> Option<Vec<FsEntry>>;
    fn read(&self, path: &Path) -> Option<Vec<u8>>;
    /// Lê no máximo `max` bytes do início do arquivo, como o `openSync` +
    /// `readSync` do JS.
    fn read_prefix(&self, path: &Path, max: usize) -> Option<Vec<u8>> {
        let mut data = self.read(path)?;
        data.truncate(max);
        Some(data)
    }
    fn read_link(&self, path: &Path) -> Option<PathBuf>;
    fn mkdir_all(&self, path: &Path, mode: u32) -> std::io::Result<()>;
    fn write_new(&self, path: &Path, data: &[u8], mode: u32) -> std::io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()>;
    fn remove_file(&self, path: &Path) -> std::io::Result<()>;
}

/// `process.env` como mapa, ignorando chaves/valores não UTF-8 (o Node os
/// converte; aqui não são usados pelo contrato de emuladores).
pub fn process_env() -> std::collections::BTreeMap<String, String> {
    std::env::vars_os()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .collect()
}

/// Relógio injetável; `Date.now()` no oráculo.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> i64;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        system_time_ms(SystemTime::now())
    }
}

/// Relógio determinístico para testes.
#[derive(Debug, Default)]
pub struct ManualClock {
    now_ms: AtomicI64,
}

impl ManualClock {
    pub fn new(now_ms: i64) -> Self {
        Self {
            now_ms: AtomicI64::new(now_ms),
        }
    }

    pub fn set(&self, now_ms: i64) {
        self.now_ms.store(now_ms, AtomicOrdering::SeqCst);
    }

    pub fn advance(&self, delta_ms: i64) {
        self.now_ms.fetch_add(delta_ms, AtomicOrdering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> i64 {
        self.now_ms.load(AtomicOrdering::SeqCst)
    }
}

fn system_time_ms(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
        Err(error) => -i64::try_from(error.duration().as_millis()).unwrap_or(i64::MAX),
    }
}

fn metadata_kind(file_type: std::fs::FileType) -> FsKind {
    if file_type.is_symlink() {
        FsKind::Symlink
    } else if file_type.is_file() {
        FsKind::File
    } else if file_type.is_dir() {
        FsKind::Directory
    } else {
        FsKind::Other
    }
}

fn system_meta(meta: &std::fs::Metadata, kind: FsKind) -> FsMeta {
    let mtime_ms = meta.modified().map(system_time_ms).unwrap_or(0);
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode()
    };
    #[cfg(not(unix))]
    let mode = match kind {
        FsKind::Directory => 0o777,
        _ => 0o666,
    };
    FsMeta {
        kind,
        size: meta.len(),
        mtime_ms,
        mode,
    }
}

/// Implementação real usada pelo app. A camada de IPC constrói o registry com
/// esta probe; os testes preferem diretórios temporários ou [`MemoryFs`].
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemFs;

impl EmulatorFs for SystemFs {
    fn lstat(&self, path: &Path) -> Option<FsMeta> {
        let meta = std::fs::symlink_metadata(path).ok()?;
        Some(system_meta(&meta, metadata_kind(meta.file_type())))
    }

    fn stat(&self, path: &Path) -> Option<FsMeta> {
        let meta = std::fs::metadata(path).ok()?;
        Some(system_meta(&meta, metadata_kind(meta.file_type())))
    }

    fn read_dir(&self, path: &Path) -> Option<Vec<FsEntry>> {
        let entries = std::fs::read_dir(path).ok()?;
        let mut result = Vec::new();
        for entry in entries.flatten() {
            let name = match entry.file_name().into_string() {
                Ok(name) => name,
                Err(_) => continue,
            };
            if name.is_empty() || name.contains('\0') {
                continue;
            }
            let kind = entry
                .file_type()
                .map(metadata_kind)
                .unwrap_or(FsKind::Other);
            result.push(FsEntry { name, kind });
        }
        Some(result)
    }

    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }

    fn read_prefix(&self, path: &Path, max: usize) -> Option<Vec<u8>> {
        use std::io::Read;
        let file = std::fs::File::open(path).ok()?;
        let mut data = Vec::with_capacity(max.min(0x10000));
        file.take(max as u64).read_to_end(&mut data).ok()?;
        Some(data)
    }

    fn read_link(&self, path: &Path) -> Option<PathBuf> {
        std::fs::read_link(path).ok()
    }

    fn mkdir_all(&self, path: &Path, mode: u32) -> std::io::Result<()> {
        // `mkdirSync(..., { recursive: true, mode })` cria os componentes que
        // faltam com o modo pedido e não altera diretórios já existentes.
        let mut missing = Vec::new();
        let mut current = path.to_path_buf();
        while !current.as_os_str().is_empty() && !current.exists() {
            missing.push(current.clone());
            let Some(parent) = current.parent() else {
                break;
            };
            if parent == current {
                break;
            }
            current = parent.to_path_buf();
        }
        for directory in missing.into_iter().rev() {
            let builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            let builder = {
                use std::os::unix::fs::DirBuilderExt;
                let mut builder = builder;
                builder.mode(mode);
                builder
            };
            #[cfg(not(unix))]
            let _ = mode;
            match builder.create(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn write_new(&self, path: &Path, data: &[u8], mode: u32) -> std::io::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(mode);
        }
        #[cfg(not(unix))]
        let _ = mode;
        let mut file = options.open(path)?;
        file.write_all(data)?;
        file.flush()
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        std::fs::remove_file(path)
    }
}

#[derive(Clone, Debug)]
struct MemFile {
    data: Vec<u8>,
    mode: u32,
    mtime_ms: i64,
}

/// Filesystem em memória para testes de decisão (detecção, permissões POSIX,
/// `/proc`) sem tocar no disco. Caminhos são normalizados como no Node; o nó
/// raiz `"/"` existe sempre.
#[derive(Debug, Default)]
pub struct MemoryFs {
    files: Mutex<std::collections::BTreeMap<PathBuf, MemFile>>,
    dirs: Mutex<std::collections::BTreeSet<PathBuf>>,
    links: Mutex<std::collections::BTreeMap<PathBuf, PathBuf>>,
}

impl MemoryFs {
    pub fn new() -> Self {
        let fs = Self::default();
        fs.dirs
            .lock()
            .expect("dirs")
            .insert(normalize_path(Path::new("/")));
        fs
    }

    pub fn add_dir(&self, path: &Path) {
        let path = normalize_path(path);
        self.mkdir_all(&path, 0o755).expect("mkdir memory");
    }

    pub fn add_file(&self, path: &Path, data: &[u8]) {
        self.add_file_mode(path, data, 0o644, 0);
    }

    pub fn add_file_mode(&self, path: &Path, data: &[u8], mode: u32, mtime_ms: i64) {
        let path = normalize_path(path);
        if let Some(parent) = path.parent() {
            self.mkdir_all(parent, 0o755).expect("parent memory");
        }
        self.files.lock().expect("files").insert(
            path,
            MemFile {
                data: data.to_vec(),
                mode,
                mtime_ms,
            },
        );
    }

    pub fn add_symlink(&self, path: &Path, target: &Path) {
        let path = normalize_path(path);
        if let Some(parent) = path.parent() {
            self.mkdir_all(parent, 0o755).expect("parent memory");
        }
        self.links
            .lock()
            .expect("links")
            .insert(path, target.to_path_buf());
    }

    fn resolve(&self, path: &Path) -> PathBuf {
        let mut current = normalize_path(path);
        for _ in 0..16 {
            let target = self.links.lock().expect("links").get(&current).cloned();
            let Some(target) = target else {
                return current;
            };
            current = if target.has_root() {
                normalize_path(&target)
            } else {
                normalize_path(&current.parent().unwrap_or(Path::new("")).join(target))
            };
        }
        current
    }

    fn meta_for(&self, path: &Path) -> Option<FsMeta> {
        if let Some(link_target) = self.links.lock().expect("links").get(path) {
            let _ = link_target;
            return Some(FsMeta {
                kind: FsKind::Symlink,
                size: 0,
                mtime_ms: 0,
                mode: 0o777,
            });
        }
        if let Some(file) = self.files.lock().expect("files").get(path) {
            return Some(FsMeta {
                kind: FsKind::File,
                size: file.data.len() as u64,
                mtime_ms: file.mtime_ms,
                mode: file.mode,
            });
        }
        if self.dirs.lock().expect("dirs").contains(path) {
            return Some(FsMeta {
                kind: FsKind::Directory,
                size: 0,
                mtime_ms: 0,
                mode: 0o755,
            });
        }
        None
    }
}

impl EmulatorFs for MemoryFs {
    fn lstat(&self, path: &Path) -> Option<FsMeta> {
        self.meta_for(&normalize_path(path))
    }

    fn stat(&self, path: &Path) -> Option<FsMeta> {
        self.meta_for(&self.resolve(path))
    }

    fn read_dir(&self, path: &Path) -> Option<Vec<FsEntry>> {
        let path = normalize_path(path);
        if !self.dirs.lock().expect("dirs").contains(&path) {
            return None;
        }
        let mut by_name: std::collections::BTreeMap<String, FsKind> =
            std::collections::BTreeMap::new();
        for (entry_path, kind) in self
            .files
            .lock()
            .expect("files")
            .keys()
            .map(|p| (p, FsKind::File))
            .chain(
                self.dirs
                    .lock()
                    .expect("dirs")
                    .iter()
                    .map(|p| (p, FsKind::Directory)),
            )
            .chain(
                self.links
                    .lock()
                    .expect("links")
                    .keys()
                    .map(|p| (p, FsKind::Symlink)),
            )
        {
            if entry_path.parent() != Some(path.as_path()) {
                continue;
            }
            if let Some(name) = entry_path.file_name().and_then(|name| name.to_str()) {
                by_name.insert(name.to_string(), kind);
            }
        }
        Some(
            by_name
                .into_iter()
                .map(|(name, kind)| FsEntry { name, kind })
                .collect(),
        )
    }

    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        let path = self.resolve(path);
        self.files
            .lock()
            .expect("files")
            .get(&path)
            .map(|file| file.data.clone())
    }

    fn read_prefix(&self, path: &Path, max: usize) -> Option<Vec<u8>> {
        let mut data = self.read(path)?;
        data.truncate(max);
        Some(data)
    }

    fn read_link(&self, path: &Path) -> Option<PathBuf> {
        self.links
            .lock()
            .expect("links")
            .get(&normalize_path(path))
            .cloned()
    }

    fn mkdir_all(&self, path: &Path, mode: u32) -> std::io::Result<()> {
        let path = normalize_path(path);
        let mut current = PathBuf::new();
        let mut dirs = self.dirs.lock().expect("dirs");
        for component in path.components() {
            current.push(component.as_os_str());
            if matches!(component, Component::Prefix(_) | Component::RootDir) {
                continue;
            }
            dirs.insert(current.clone());
        }
        if dirs.contains(&path) {
            let _ = mode;
        }
        Ok(())
    }

    fn write_new(&self, path: &Path, data: &[u8], mode: u32) -> std::io::Result<()> {
        let path = normalize_path(path);
        if let Some(parent) = path.parent() {
            if !self.dirs.lock().expect("dirs").contains(parent) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "parent missing",
                ));
            }
        }
        let mut files = self.files.lock().expect("files");
        if files.contains_key(&path) || self.links.lock().expect("links").contains_key(&path) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "exists",
            ));
        }
        files.insert(
            path,
            MemFile {
                data: data.to_vec(),
                mode,
                mtime_ms: 0,
            },
        );
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        let from = normalize_path(from);
        let to = normalize_path(to);
        let removed_file = { self.files.lock().expect("files").remove(&from) };
        if let Some(file) = removed_file {
            self.files.lock().expect("files").insert(to, file);
            return Ok(());
        }
        let removed_link = { self.links.lock().expect("links").remove(&from) };
        if let Some(link) = removed_link {
            self.links.lock().expect("links").insert(to, link);
            return Ok(());
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "source missing",
        ))
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        let path = normalize_path(path);
        if self.files.lock().expect("files").remove(&path).is_some() {
            return Ok(());
        }
        if self.links.lock().expect("links").remove(&path).is_some() {
            return Ok(());
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file missing",
        ))
    }
}

// ---------------------------------------------------------------------------
// Semântica de string do JS
// ---------------------------------------------------------------------------

/// `\s` do `String#trim`/regex JS (inclui BOM e separadores Unicode).
pub fn is_js_whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{0009}'..='\u{000d}'
            | '\u{0020}'
            | '\u{00a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    )
}

pub fn js_trim(value: &str) -> &str {
    value.trim_matches(is_js_whitespace)
}

/// `String#trimEnd` do JS.
pub fn js_trim_end(value: &str) -> &str {
    value.trim_end_matches(is_js_whitespace)
}

/// `String#length`: unidades UTF-16.
pub fn js_len(value: &str) -> usize {
    value.encode_utf16().count()
}

/// `String#slice(0, max)` em unidades UTF-16.
pub fn js_slice(value: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let mut units = 0usize;
    let mut end = 0usize;
    for (index, character) in value.char_indices() {
        let width = character.len_utf16();
        if units + width > max {
            break;
        }
        units += width;
        end = index + character.len_utf8();
    }
    value[..end].to_string()
}

/// `String#toLowerCase`.
pub fn js_lower(value: &str) -> String {
    value.to_lowercase()
}

/// Aproximação determinística de `String#localeCompare` para ordenar caminhos.
/// Compara primeiro a forma minúscula e depois a original; cobre o caso real
/// (nomes de arquivo com caixas mistas) sem depender do locale do processo.
pub fn js_locale_cmp(left: &str, right: &str) -> Ordering {
    left.to_lowercase()
        .cmp(&right.to_lowercase())
        .then_with(|| left.cmp(right))
}

fn is_path_separator(character: char) -> bool {
    if cfg!(windows) {
        character == '/' || character == '\\'
    } else {
        character == '/'
    }
}

fn ends_with_separator(value: &str) -> bool {
    value.chars().last().map(is_path_separator).unwrap_or(false)
}

/// `path.normalize` léxico do Node. Preserva a barra final quando o caminho
/// original termina em separador, exatamente como o Node.
pub fn normalize_path(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    let trailing = ends_with_separator(&raw);
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => output.push(prefix.as_os_str()),
            Component::RootDir => output.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                let last = output.components().next_back();
                let can_pop = matches!(last, Some(Component::Normal(_)));
                if can_pop {
                    output.pop();
                } else if !output.has_root() {
                    output.push("..");
                }
            }
            Component::Normal(part) => output.push(part),
        }
    }
    if output.as_os_str().is_empty() {
        output.push(".");
    }
    if trailing {
        let rendered = output.to_string_lossy();
        if !rendered.ends_with(PlatformKind::path_separator()) {
            let mut rendered = rendered.into_owned();
            rendered.push(PlatformKind::path_separator());
            output = PathBuf::from(rendered);
        }
    }
    output
}

/// `path.resolve` para um caminho possivelmente relativo.
pub fn resolve_from_cwd(path: &Path) -> PathBuf {
    if path.has_root() {
        return normalize_path(path);
    }
    match std::env::current_dir() {
        Ok(cwd) => normalize_path(&cwd.join(path)),
        Err(_) => normalize_path(path),
    }
}

/// `path.isAbsolute` (Node considera `/x` absoluto no Windows).
pub fn is_absolute(path: &Path) -> bool {
    path.has_root()
}

/// `path.basename`.
pub fn basename(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `path.basename(path, ext)`.
pub fn basename_without_extension(name: &str, extension: &str) -> String {
    if !extension.is_empty() && name.ends_with(extension) {
        name[..name.len() - extension.len()].to_string()
    } else {
        name.to_string()
    }
}

/// `path.extname` aplicado a um nome/caminho.
pub fn extname(path: &str) -> String {
    let part_start = path
        .char_indices()
        .rev()
        .find(|(_, character)| is_path_separator(*character))
        .map(|(index, character)| index + character.len_utf8())
        .unwrap_or(0);
    let part = &path[part_start..];
    if part == "." || part == ".." {
        return String::new();
    }
    match part.rfind('.') {
        Some(0) | None => String::new(),
        Some(index) => part[index..].to_string(),
    }
}

/// `path.join`/`path.relative` produzem caminhos com o separador do host.
pub fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// `path.join(a, b)` com normalização do Node para caminhos absolutos.
pub fn join_path(base: &Path, name: &str) -> PathBuf {
    normalize_path(&base.join(name))
}

// ---------------------------------------------------------------------------
// Validação de arquivos/caminhos
// ---------------------------------------------------------------------------

/// `hasSymlinkComponent`: percorre cada componente com `lstat`; qualquer erro
/// encerra a checagem (tratado como "sem symlink" pelo JS).
pub fn has_symlink_component(path: &Path, fs: &dyn EmulatorFs) -> bool {
    let absolute = if path.has_root() {
        normalize_path(path)
    } else {
        resolve_from_cwd(path)
    };
    let mut root = PathBuf::new();
    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) if root.as_os_str().is_empty() => {
                root.push(prefix.as_os_str());
            }
            Component::RootDir if parts.is_empty() => {
                root.push(component.as_os_str());
            }
            Component::Normal(part) => parts.push(part.to_os_string()),
            Component::CurDir
            | Component::ParentDir
            | Component::Prefix(_)
            | Component::RootDir => {}
        }
    }
    let mut current = root;
    for part in parts {
        current.push(part);
        match fs.lstat(&current) {
            Some(meta) if meta.kind == FsKind::Symlink => return true,
            Some(_) => {}
            None => return false,
        }
    }
    false
}

/// `isRegular`: arquivo comum e nenhum componente symlink.
pub fn is_regular(path: &Path, fs: &dyn EmulatorFs) -> bool {
    if has_symlink_component(path, fs) {
        return false;
    }
    fs.lstat(path)
        .map(|meta| meta.kind == FsKind::File)
        .unwrap_or(false)
}

/// `isDirectory`: diretório e nenhum componente symlink.
pub fn is_directory(path: &Path, fs: &dyn EmulatorFs) -> bool {
    if has_symlink_component(path, fs) {
        return false;
    }
    fs.lstat(path)
        .map(|meta| meta.kind == FsKind::Directory)
        .unwrap_or(false)
}

/// `isExecutable`: `statSync` (segue symlink), arquivo comum; no Windows o bit
/// POSIX não existe e qualquer arquivo regular conta.
pub fn is_executable(path: &Path, fs: &dyn EmulatorFs, platform: PlatformKind) -> bool {
    let Some(meta) = fs.stat(path) else {
        return false;
    };
    if meta.kind != FsKind::File {
        return false;
    }
    if platform.is_windows() {
        return true;
    }
    meta.mode & 0o111 != 0
}

/// Validação de caminho absoluto do registry (`normalizeAbsoluteFile`).
///
/// Rejeita vazio/NUL, caminhos relativos e qualquer componente `..` em ambos
/// os separadores (bypass que o oráculo fecha explicitamente).
pub fn normalize_absolute(value: &str, field: &str) -> Result<PathBuf, String> {
    let trimmed = js_trim(value);
    if trimmed.is_empty() || trimmed.contains('\0') {
        return Err(format!("{field}_invalido"));
    }
    if trimmed.split(['/', '\\']).any(|part| part == "..") {
        return Err(format!("{field}_invalido"));
    }
    let path = Path::new(trimmed);
    if !is_absolute(path) {
        return Err(format!("{field}_invalido"));
    }
    let normalized = normalize_path(path);
    if !is_absolute(&normalized) {
        return Err(format!("{field}_invalido"));
    }
    Ok(normalized)
}

// ---------------------------------------------------------------------------
// Conversões de `serde_json::Value` com a semântica do JS
// ---------------------------------------------------------------------------

/// `Boolean(value)` do JS.
pub fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64().map(|n| n != 0.0).unwrap_or(true),
        Value::String(value) => !value.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn parse_js_number(value: &str) -> Option<f64> {
    let trimmed = js_trim(value);
    if trimmed.is_empty() {
        return Some(0.0);
    }
    let (sign, unsigned) = if let Some(rest) = trimmed.strip_prefix('+') {
        (1.0, rest)
    } else if let Some(rest) = trimmed.strip_prefix('-') {
        (-1.0, rest)
    } else {
        (1.0, trimmed)
    };
    if unsigned == "Infinity" {
        return Some(sign * f64::INFINITY);
    }
    let lower = unsigned.to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix("0x") {
        return u128::from_str_radix(hex, 16)
            .ok()
            .map(|value| sign * value as f64);
    }
    if let Some(binary) = lower.strip_prefix("0b") {
        return u128::from_str_radix(binary, 2)
            .ok()
            .map(|value| sign * value as f64);
    }
    if let Some(octal) = lower.strip_prefix("0o") {
        return u128::from_str_radix(octal, 8)
            .ok()
            .map(|value| sign * value as f64);
    }
    if !is_js_decimal(unsigned) {
        return None;
    }
    trimmed.parse::<f64>().ok()
}

/// Gramática decimal aceita por `Number(string)` do JS (sem `Infinity`, que é
/// tratado antes, e sem os prefixos 0x/0o/0b).
fn is_js_decimal(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let mut index = 0usize;
    let mut digits = 0usize;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
        digits += 1;
    }
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return false;
    }
    if index < bytes.len() && (bytes[index] == b'e' || bytes[index] == b'E') {
        index += 1;
        if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
            index += 1;
        }
        let exponent_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == exponent_start {
            return false;
        }
    }
    index == bytes.len()
}

/// `Number(value)` do JS. `None` representa `NaN`.
pub fn js_number(value: &Value) -> Option<f64> {
    match value {
        Value::Null => Some(0.0),
        Value::Bool(value) => Some(if *value { 1.0 } else { 0.0 }),
        Value::Number(number) => number.as_f64(),
        Value::String(value) => parse_js_number(value),
        Value::Array(items) => match items.len() {
            0 => Some(0.0),
            1 => js_number(&items[0]),
            _ => None,
        },
        Value::Object(_) => None,
    }
}

/// `Number.isInteger` + conversão para `i64`. Só aceita números JSON, como o
/// `Number.isInteger` (que não converte strings/booleanos).
pub fn js_integer(value: &Value) -> Option<i64> {
    let Value::Number(number) = value else {
        return None;
    };
    let number = number.as_f64()?;
    if !number.is_finite() || number.fract() != 0.0 {
        return None;
    }
    if number < i64::MIN as f64 || number > i64::MAX as f64 {
        return None;
    }
    Some(number as i64)
}

/// `Number.isFinite(Number(value)) ? Math.trunc(Number(value)) : fallback`.
pub fn js_trunc_or(value: &Value, fallback: i64) -> i64 {
    match js_number(value) {
        Some(number) if number.is_finite() => {
            let truncated = number.trunc();
            if truncated < i64::MIN as f64 {
                i64::MIN
            } else if truncated > i64::MAX as f64 {
                i64::MAX
            } else {
                truncated as i64
            }
        }
        _ => fallback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normaliza_caminhos_como_node() {
        assert_eq!(
            normalize_path(Path::new("/a/./b/../c")),
            PathBuf::from("/a/c")
        );
        assert_eq!(normalize_path(Path::new("/a/b/")), PathBuf::from("/a/b/"));
        assert_eq!(normalize_path(Path::new("a/../")), PathBuf::from("./"));
        assert_eq!(normalize_path(Path::new("..")), PathBuf::from(".."));
        assert_eq!(extname("Game.MDS"), ".MDS");
        assert_eq!(extname(".bashrc"), "");
        assert_eq!(extname("file."), ".");
        assert_eq!(basename_without_extension("Game.cue", ".cue"), "Game");
    }

    #[test]
    fn trim_slice_e_comparacao_seguem_o_js() {
        assert_eq!(js_trim("\u{feff} x \n"), "x");
        assert_eq!(js_slice("a😀b", 3), "a😀");
        assert_eq!(js_slice("a😀b", 2), "a");
        assert_eq!(js_len("a😀b"), 4);
        assert!(js_locale_cmp("a.foo", "b.FOO") == Ordering::Less);
        assert!(js_locale_cmp("Z", "a") == Ordering::Greater);
        assert!(js_locale_cmp("Other.ISO", "nested") == Ordering::Greater);
    }

    #[test]
    fn numero_booleano_e_inteiro_como_js() {
        assert_eq!(js_number(&Value::String("0x10".into())), Some(16.0));
        assert_eq!(js_number(&Value::String("".into())), Some(0.0));
        assert_eq!(
            js_number(&Value::String("Infinity".into())),
            Some(f64::INFINITY)
        );
        assert_eq!(js_number(&Value::String("infinity".into())), None);
        assert_eq!(js_number(&Value::String("1e3".into())), Some(1000.0));
        assert_eq!(js_number(&Value::String("1e".into())), None);
        assert_eq!(js_number(&Value::String(".5".into())), Some(0.5));
        assert_eq!(js_number(&Value::String("5.".into())), Some(5.0));
        assert_eq!(js_number(&Value::Array(vec![])), Some(0.0));
        assert_eq!(js_number(&Value::Array(vec![Value::from(2)])), Some(2.0));
        assert_eq!(js_number(&serde_json::json!([1, 2])), None);
        assert_eq!(js_integer(&serde_json::json!(1.0)), Some(1));
        assert_eq!(js_integer(&serde_json::json!(1.5)), None);
        assert_eq!(js_integer(&serde_json::json!("1")), None);
        assert!(js_truthy(&Value::Array(vec![])));
        assert!(!js_truthy(&Value::String(String::new())));
    }

    #[test]
    fn memory_fs_respeita_modo_e_symlink() {
        let fs = MemoryFs::new();
        fs.add_file_mode(Path::new("/bin/emu"), b"x", 0o755, 10);
        assert!(is_executable(
            Path::new("/bin/emu"),
            &fs,
            PlatformKind::Linux
        ));
        fs.add_file_mode(Path::new("/bin/noexec"), b"x", 0o644, 10);
        assert!(!is_executable(
            Path::new("/bin/noexec"),
            &fs,
            PlatformKind::Linux
        ));
        fs.add_symlink(Path::new("/bin/link"), Path::new("/bin/emu"));
        assert!(!is_regular(Path::new("/bin/link"), &fs));
        assert!(has_symlink_component(Path::new("/bin/link"), &fs));
        assert!(is_executable(
            Path::new("/bin/link"),
            &fs,
            PlatformKind::Linux
        ));
    }

    #[test]
    fn normalize_absolute_rejeita_relativo_e_parent() {
        assert!(normalize_absolute("/a/../b", "romPath").is_err());
        assert!(normalize_absolute("relative/x", "romPath").is_err());
        assert!(normalize_absolute("", "romPath").is_err());
        assert!(normalize_absolute("/a/b", "romPath").is_ok());
    }
}
