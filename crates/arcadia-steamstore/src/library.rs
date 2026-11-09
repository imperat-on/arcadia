//! Biblioteca Steam: `libraryfolders.vdf`, ACF (`appmanifest_*.acf`),
//! detecção de downloads do Arcadia, ACF mínimo, limpeza segura e registro na
//! SLSsteam (config.yaml/stplug-in/depotcache).
//!
//! Porte de `steamLibraries`, `steamAppsDirs`, `gameInstallDir`,
//! `arcadiaDownloaded`, `removeDownloaded`, `writeAcf`, `addToSteam`,
//! `removeFromSteam`, `registerSlssteam`, `appidsInjetados` e dos stubs
//! `adicionarStubPendente`/`removerStubPendente`/`marcarJogoSteamInstalado` de
//! `app/electron/main.js`.
//!
//! Toda mutação de disco passa por [`LibraryFs`], injetado. Os testes usam
//! diretórios temporários e um FS espião para provar que nada fora de
//! `steamapps/common` é apagado.

use std::collections::{BTreeSet, HashSet};
use std::io;
use std::path::{Component, Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Operações de arquivo usadas pela biblioteca.
pub trait LibraryFs: Send + Sync {
    fn exists(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
    fn read_dir(&self, path: &Path) -> io::Result<Vec<String>>;
    fn write(&self, path: &Path, contents: &str) -> io::Result<()>;
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn remove_dir_all(&self, path: &Path) -> io::Result<()>;
    fn copy(&self, from: &Path, to: &Path) -> io::Result<()>;
}

/// Implementação real (a borda também pode usar um FS em memória nos testes).
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemFs;

impl LibraryFs for SystemFs {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<String>> {
        let mut names = std::fs::read_dir(path)?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect::<Vec<_>>();
        names.sort();
        Ok(names)
    }

    fn write(&self, path: &Path, contents: &str) -> io::Result<()> {
        std::fs::write(path, contents)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        match std::fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn copy(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::copy(from, to).map(|_| ())
    }
}

/// Raiz de uma biblioteca lida de `libraryfolders.vdf` (pasta que contém
/// `steamapps/`, ex.: `D:\SteamLibrary`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LibraryFolder {
    pub path: PathBuf,
}

impl LibraryFolder {
    pub fn steam_apps(&self) -> PathBuf {
        self.path.join("steamapps")
    }
}

/// Biblioteca pronta para a UI (`store:libraries`): `path` é `.../steamapps`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SteamLibrary {
    pub path: PathBuf,
    #[serde(rename = "steamDir")]
    pub steam_dir: PathBuf,
    #[serde(default)]
    pub free: u64,
}

/// ACF lido (`AppState`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcfManifest {
    pub appid: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub installdir: String,
    #[serde(default)]
    pub buildid: String,
    #[serde(default)]
    pub state_flags: u64,
    #[serde(default)]
    pub size_on_disk: u64,
    #[serde(default)]
    pub last_updated: u64,
    #[serde(default)]
    pub last_played: u64,
    /// Marca `"ArcadiaDownload" "1"` — só esses ACFs podem ser removidos.
    #[serde(default)]
    pub arcadia_download: bool,
}

impl AcfManifest {
    pub fn is_installed(&self) -> bool {
        self.state_flags & 4 != 0
    }

    pub fn arcadia_marker(&self) -> bool {
        self.arcadia_download
    }
}

/// Jogo de entrada para `store:installDir`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameRef {
    pub id: String,
    pub launcher: String,
    pub exe: Option<String>,
}

impl GameRef {
    pub fn new(id: impl Into<String>, launcher: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            launcher: launcher.into(),
            exe: None,
        }
    }

    pub fn with_exe(mut self, exe: impl Into<String>) -> Self {
        self.exe = Some(exe.into());
        self
    }
}

/// Download do Arcadia encontrado nas bibliotecas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadedApp {
    pub appid: String,
    pub steamapps: PathBuf,
    pub installdir: String,
}

/// Envelope de `store:addToSteam`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AddToSteamResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub luas: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Envelope das remoções. `removidos` só aparece em `removeDownloaded`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoveResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removidos: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RemoveResult {
    fn ok() -> Self {
        Self {
            ok: true,
            removidos: None,
            error: None,
        }
    }

    fn removed(count: usize) -> Self {
        Self {
            ok: true,
            removidos: Some(count),
            error: None,
        }
    }

    fn failure(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            removidos: None,
            error: Some(message.into()),
        }
    }
}

/// Resultado de uma edição de `pending_games.json` (puro; a borda grava).
#[derive(Debug, Clone, PartialEq)]
pub struct PendingUpdate {
    pub games: Vec<Value>,
    pub changed: bool,
}

// ── libraryfolders.vdf ────────────────────────────────────────────────────

/// Normaliza o caminho do VDF para `/` (o Node faz `\\\\` -> `/`).
fn normalize_library_path(raw: &str) -> String {
    let text = raw.replace("\\\\", "/");
    text.replace('\\', "/")
}

/// Extrai todos os `"path"` de `libraryfolders.vdf`, na ordem do arquivo,
/// deduplicados. Usa o parser de VDF do `arcadia-steam`; se o arquivo fugir do
/// formato, cai no regex do oráculo.
pub fn parse_library_folders(text: &str) -> Vec<LibraryFolder> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    if let Ok(root) = arcadia_steam::parse_vdf(text) {
        let mut stack = vec![&root];
        while let Some(object) = stack.pop() {
            // Ordem de inserção preservada; entra nos objetos na ordem inversa
            // para que a pilha devolva a ordem original.
            let mut children = Vec::new();
            for (key, value) in object.iter() {
                if key == "path" {
                    if let Some(path) = value.as_str() {
                        let normalized = normalize_library_path(path);
                        if !normalized.is_empty() && seen.insert(normalized.clone()) {
                            paths.push(LibraryFolder {
                                path: PathBuf::from(normalized),
                            });
                        }
                    }
                } else if let Some(child) = value.as_object() {
                    children.push(child);
                }
            }
            for child in children.into_iter().rev() {
                stack.push(child);
            }
        }
    }
    if paths.is_empty() {
        if let Ok(regex) = Regex::new(r#""path"\s+"([^"]+)""#) {
            for capture in regex.captures_iter(text) {
                let normalized = normalize_library_path(&capture[1]);
                if !normalized.is_empty() && seen.insert(normalized.clone()) {
                    paths.push(LibraryFolder {
                        path: PathBuf::from(normalized),
                    });
                }
            }
        }
    }
    paths
}

/// Bibliotecas de uma lista de raízes de instalação da Steam.
///
/// Cada raiz (`~/.steam/steam`, `D:\Steam`…) tem seu próprio
/// `steamapps/libraryfolders.vdf`; deltas de todas as raízes são unidos na
/// ordem de descoberta. Sem nenhum VDF, cai na biblioteca principal da
/// primeira raiz (equivalente ao `findSteamDir()` do oráculo).
pub fn discover_libraries(
    steam_roots: &[PathBuf],
    fs: &dyn LibraryFs,
    free_space: Option<&dyn Fn(&Path) -> u64>,
) -> Vec<SteamLibrary> {
    let mut libraries: Vec<SteamLibrary> = Vec::new();
    let mut seen = HashSet::new();
    for root in steam_roots {
        let vdf = root.join("steamapps").join("libraryfolders.vdf");
        let Ok(text) = fs.read_to_string(&vdf) else {
            continue;
        };
        for folder in parse_library_folders(&text) {
            let steamapps = folder.steam_apps();
            if seen.insert(steamapps.clone()) {
                libraries.push(SteamLibrary {
                    free: free_space.map(|free| free(&steamapps)).unwrap_or(0),
                    path: steamapps,
                    steam_dir: folder.path,
                });
            }
        }
    }
    if libraries.is_empty() {
        if let Some(root) = steam_roots.first() {
            let steamapps = root.join("steamapps");
            libraries.push(SteamLibrary {
                free: free_space.map(|free| free(&steamapps)).unwrap_or(0),
                path: steamapps,
                steam_dir: root.clone(),
            });
        }
    }
    libraries
}

/// Raízes que o `steamLibraries` do oráculo consulta (Linux/Flatpak).
pub fn steam_libraries(
    home: &Path,
    fs: &dyn LibraryFs,
    free_space: Option<&dyn Fn(&Path) -> u64>,
) -> Vec<SteamLibrary> {
    let roots = [
        home.join(".steam").join("steam"),
        home.join(".local").join("share").join("Steam"),
        home.join(".var")
            .join("app")
            .join("com.valvesoftware.Steam")
            .join(".local")
            .join("share")
            .join("Steam"),
    ];
    discover_libraries(&roots, fs, free_space)
}

/// Pastas `steamapps` de uma instalação, incluindo bibliotecas extras.
pub fn steam_apps_dirs(steam_roots: &[PathBuf], fs: &dyn LibraryFs) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();
    for root in steam_roots {
        let main = root.join("steamapps");
        if seen.insert(main.clone()) {
            dirs.push(main.clone());
        }
        let vdf = main.join("libraryfolders.vdf");
        let Ok(text) = fs.read_to_string(&vdf) else {
            continue;
        };
        for folder in parse_library_folders(&text) {
            let apps = platform_path(folder.steam_apps());
            if seen.insert(apps.clone()) {
                dirs.push(apps);
            }
        }
    }
    dirs
}

/// No Windows o `steamAppsDirs` do oráculo devolve separadores `\`; no resto
/// da plataforma, `/`.
fn platform_path(path: PathBuf) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(path.to_string_lossy().replace('/', "\\"))
    } else {
        path
    }
}

/// `steamBasePath()`: primeira raiz com `steamapps` ou `config`.
pub fn find_steam_base(candidates: &[PathBuf], fs: &dyn LibraryFs) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|path| fs.exists(&path.join("steamapps")) || fs.exists(&path.join("config")))
        .cloned()
}

/// Candidatos do `steamBasePath` congelado (Linux/Flatpak).
pub fn steam_base_candidates(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".steam").join("steam"),
        home.join(".local").join("share").join("Steam"),
        home.join(".steam").join("debian-installation"),
        home.join(".var")
            .join("app")
            .join("com.valvesoftware.Steam")
            .join(".local")
            .join("share")
            .join("Steam"),
    ]
}

// ── ACF ───────────────────────────────────────────────────────────────────

fn regex_field(text: &str, key: &str) -> Option<String> {
    let pattern = format!(r#""{}"\s+"([^"]*)""#, regex::escape(key));
    Regex::new(&pattern)
        .ok()?
        .captures(text)
        .map(|capture| capture[1].to_string())
}

fn number_field(text: &str, key: &str) -> u64 {
    regex_field(text, key)
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

/// Lê `appmanifest_<appid>.acf`. Usa o parser de VDF e cai em regex campo a
/// campo para ACFs reais com formatação inesperada.
pub fn parse_acf(text: &str) -> Option<AcfManifest> {
    if text.trim().is_empty() {
        return None;
    }
    let parsed = arcadia_steam::parse_vdf(text).ok();
    let root = parsed
        .as_ref()
        .and_then(|root| root.get_object("AppState").or(Some(root)));
    let get = |key: &str| {
        root.and_then(|root| root.get_str(key))
            .map(str::to_string)
            .or_else(|| regex_field(text, key))
    };
    let appid = get("appid").unwrap_or_default();
    let installdir = get("installdir").unwrap_or_default();
    let name = get("name").unwrap_or_default();
    let buildid = get("buildid").unwrap_or_default();
    let state_flags = get("StateFlags")
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or_else(|| number_field(text, "StateFlags"));
    let size_on_disk = get("SizeOnDisk")
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or_else(|| number_field(text, "SizeOnDisk"));
    let last_updated = get("LastUpdated")
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or_else(|| number_field(text, "LastUpdated"));
    let last_played = get("LastPlayed")
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or_else(|| number_field(text, "LastPlayed"));
    let arcadia_download = text.contains("\"ArcadiaDownload\"");
    if appid.is_empty()
        && installdir.is_empty()
        && name.is_empty()
        && buildid.is_empty()
        && !arcadia_download
    {
        return None;
    }
    Some(AcfManifest {
        appid,
        name,
        installdir,
        buildid,
        state_flags,
        size_on_disk,
        last_updated,
        last_played,
        arcadia_download,
    })
}

/// Lê todos os ACFs de uma pasta `steamapps`.
pub fn scan_manifests(steamapps: &Path, fs: &dyn LibraryFs) -> Vec<AcfManifest> {
    let Ok(names) = fs.read_dir(steamapps) else {
        return Vec::new();
    };
    let pattern = Regex::new(r"^appmanifest_(\d+)\.acf$").expect("regex de appmanifest");
    let mut manifests = Vec::new();
    for name in names {
        let Some(capture) = pattern.captures(&name) else {
            continue;
        };
        let Ok(text) = fs.read_to_string(&steamapps.join(&name)) else {
            continue;
        };
        let Some(mut manifest) = parse_acf(&text) else {
            continue;
        };
        if manifest.appid.is_empty() {
            manifest.appid = capture[1].to_string();
        }
        manifests.push(manifest);
    }
    manifests
}

/// ACFs instalados (`StateFlags & 4`), em todas as bibliotecas.
pub fn installed_manifests(libraries: &[SteamLibrary], fs: &dyn LibraryFs) -> Vec<AcfManifest> {
    libraries
        .iter()
        .flat_map(|library| scan_manifests(&library.path, fs))
        .filter(AcfManifest::is_installed)
        .collect()
}

/// ACFs com a marca `ArcadiaDownload`, em todas as bibliotecas.
pub fn arcadia_downloaded(libraries: &[SteamLibrary], fs: &dyn LibraryFs) -> Vec<DownloadedApp> {
    let pattern = Regex::new(r"^appmanifest_(\d+)\.acf$").expect("regex de appmanifest");
    let mut found = Vec::new();
    for library in libraries {
        let Ok(names) = fs.read_dir(&library.path) else {
            continue;
        };
        for name in names {
            let Some(capture) = pattern.captures(&name) else {
                continue;
            };
            let Ok(text) = fs.read_to_string(&library.path.join(&name)) else {
                continue;
            };
            let Some(manifest) = parse_acf(&text) else {
                if text.contains("\"ArcadiaDownload\"") {
                    found.push(DownloadedApp {
                        appid: capture[1].to_string(),
                        steamapps: library.path.clone(),
                        installdir: regex_field(&text, "installdir").unwrap_or_default(),
                    });
                }
                continue;
            };
            if !manifest.arcadia_download {
                continue;
            }
            found.push(DownloadedApp {
                appid: capture[1].to_string(),
                steamapps: library.path.clone(),
                installdir: manifest.installdir,
            });
        }
    }
    found
}

fn acf_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// `writeAcf`: `steamapps/appmanifest_<appid>.acf` mínimo com a marca.
pub fn write_acf_at(
    appid: &str,
    title: &str,
    installdir: &str,
    steam_dir: &Path,
    now_seconds: u64,
    fs: &dyn LibraryFs,
) -> io::Result<PathBuf> {
    if !crate::store::valid_appid(appid) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "appid inválido",
        ));
    }
    let installdir = sanitize_install_dir(installdir);
    let apps = steam_dir.join("steamapps");
    let contents = [
        "\"AppState\"".to_string(),
        "{".to_string(),
        format!("\t\"appid\"\t\t\"{}\"", acf_escape(appid)),
        "\t\"Universe\"\t\t\"1\"".to_string(),
        format!("\t\"name\"\t\t\"{}\"", acf_escape(title)),
        "\t\"StateFlags\"\t\t\"4\"".to_string(),
        format!("\t\"installdir\"\t\t\"{}\"", acf_escape(&installdir)),
        format!("\t\"LastUpdated\"\t\t\"{now_seconds}\""),
        "\t\"LastPlayed\"\t\t\"0\"".to_string(),
        "\t\"SizeOnDisk\"\t\t\"1\"".to_string(),
        "\t\"buildid\"\t\t\"1\"".to_string(),
        "\t\"ArcadiaDownload\"\t\t\"1\"".to_string(),
        "}".to_string(),
        String::new(),
    ]
    .join("\n");
    let path = apps.join(format!("appmanifest_{appid}.acf"));
    fs.write(&path, &contents)?;
    Ok(path)
}

/// `writeAcf` com o relógio do sistema.
pub fn write_acf(
    appid: &str,
    title: &str,
    installdir: &str,
    steam_dir: &Path,
    fs: &dyn LibraryFs,
) -> io::Result<PathBuf> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    write_acf_at(appid, title, installdir, steam_dir, now, fs)
}

/// `sanitizeInstallDir` (duplicado em `depot` por coesão de contrato).
pub fn sanitize_install_dir(installdir: &str) -> String {
    let filtered = installdir
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | ' ' | '-')
        })
        .collect::<String>();
    let without_dots = filtered.replace("..", "");
    let trimmed = without_dots.trim();
    if trimmed.is_empty() {
        "jogo".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Junta `installdir` sob `common` recusando absolutos/`..`/prefixos.
fn install_dir_under(common: &Path, installdir: &str) -> Option<PathBuf> {
    let candidate = Path::new(installdir);
    if candidate.is_absolute() || installdir.trim().is_empty() {
        return None;
    }
    let mut result = common.to_path_buf();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => result.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if result == common {
        None
    } else {
        Some(result)
    }
}

// ── game:installDir ───────────────────────────────────────────────────────

/// `gameInstallDir(game)`: custom -> pasta do exe; Steam -> `common/<installdir>`
/// do ACF; Epic -> `installed.json` do legendary. Devolve `""` se não achar.
pub fn game_install_dir(
    game: &GameRef,
    steam_roots: &[PathBuf],
    legendary_dir: Option<&Path>,
    fs: &dyn LibraryFs,
) -> String {
    let id = game.id.as_str();
    let appid = id.strip_prefix("steam:").unwrap_or(id);
    if game.launcher == "custom" {
        if let Some(exe) = game.exe.as_deref().filter(|exe| !exe.is_empty()) {
            if let Some(parent) = Path::new(exe).parent() {
                return parent.to_string_lossy().into_owned();
            }
        }
    }
    if game.launcher == "steam" || id.starts_with("steam:") {
        if !crate::store::valid_appid(appid) {
            return String::new();
        }
        for dir in steam_apps_dirs(steam_roots, fs) {
            let acf = dir.join(format!("appmanifest_{appid}.acf"));
            if !fs.exists(&acf) {
                continue;
            }
            let Ok(text) = fs.read_to_string(&acf) else {
                continue;
            };
            let installdir = parse_acf(&text)
                .map(|manifest| manifest.installdir)
                .filter(|installdir| !installdir.is_empty())
                .or_else(|| regex_field(&text, "installdir"))
                .unwrap_or_default();
            if !installdir.is_empty() {
                return dir
                    .join("common")
                    .join(installdir)
                    .to_string_lossy()
                    .into_owned();
            }
        }
    }
    if game.launcher == "epic" {
        if let Some(legendary) = legendary_dir {
            if let Ok(text) = fs.read_to_string(&legendary.join("installed.json")) {
                if let Ok(value) = serde_json::from_str::<Value>(&text) {
                    let app = id.strip_prefix("epic:").unwrap_or(id);
                    if let Some(path) = value
                        .get(app)
                        .and_then(|entry| entry.get("install_path"))
                        .and_then(Value::as_str)
                    {
                        return path.to_string();
                    }
                }
            }
        }
    }
    String::new()
}

/// `store:installDir` com o fallback do handler: quando o ACF/launcher não
/// resolve, usa a pasta do exe salvo em `game_settings`. Devolve `""` se nada.
pub fn game_install_dir_or(
    game: &GameRef,
    steam_roots: &[PathBuf],
    legendary_dir: Option<&Path>,
    fallback_exe: Option<&str>,
    fs: &dyn LibraryFs,
) -> String {
    let found = game_install_dir(game, steam_roots, legendary_dir, fs);
    if !found.is_empty() {
        return found;
    }
    fallback_exe
        .filter(|exe| !exe.is_empty())
        .and_then(|exe| Path::new(exe).parent())
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// ── removeDownloaded ──────────────────────────────────────────────────────

/// `removeDownloaded`: apaga somente ACFs marcados com `ArcadiaDownload`, a
/// pasta dentro de `steamapps/common` e a injeção na SLSsteam. ACF de jogo
/// comprado (sem marca) nunca é tocado.
pub fn remove_downloaded(
    appid: &str,
    libraries: &[SteamLibrary],
    home: &Path,
    manifest_dir: Option<&Path>,
    fs: &dyn LibraryFs,
) -> RemoveResult {
    let id = appid.trim();
    if !crate::store::valid_appid(id) {
        return RemoveResult::failure("appid inválido");
    }
    let matches = arcadia_downloaded(libraries, fs)
        .into_iter()
        .filter(|entry| entry.appid == id)
        .collect::<Vec<_>>();
    for entry in &matches {
        let _ = fs.remove_file(&entry.steamapps.join(format!("appmanifest_{id}.acf")));
        if entry.installdir.is_empty() {
            continue;
        }
        let common = entry.steamapps.join("common");
        let Some(directory) = install_dir_under(&common, &entry.installdir) else {
            continue;
        };
        if fs.exists(&directory) {
            let _ = fs.remove_dir_all(&directory);
        }
    }
    let base = matches
        .iter()
        .map(|entry| {
            entry
                .steamapps
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| entry.steamapps.clone())
        })
        .find(|dir| fs.exists(&dir.join("config")))
        .or_else(|| {
            matches.first().map(|entry| {
                entry
                    .steamapps
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| entry.steamapps.clone())
            })
        });
    let _ = remove_from_steam(id, base.as_deref(), home, manifest_dir, fs);
    RemoveResult::removed(matches.len())
}

// ── SLSsteam config.yaml ──────────────────────────────────────────────────

/// Corpo de uma seção de topo (até a próxima linha sem indentação).
fn section_body<'a>(yaml: &'a str, section: &str) -> Vec<&'a str> {
    let lines = yaml.split('\n').collect::<Vec<_>>();
    let Some(start) = lines
        .iter()
        .position(|line| line.starts_with(&format!("{section}:")))
    else {
        return Vec::new();
    };
    let mut body = Vec::new();
    for line in &lines[start + 1..] {
        if !line.is_empty()
            && !line.starts_with(|c: char| c.is_whitespace())
            && !line.starts_with('#')
        {
            break;
        }
        body.push(*line);
    }
    body
}

/// `temNaSecao`: item de lista `- chave` ou chave de mapa `chave:`.
fn has_in_section(yaml: &str, section: &str, key: &str) -> bool {
    // O oráculo só olha o corpo (linhas seguintes); listas inline
    // (`AdditionalApps: [1, 2]`) ficam na mesma linha e precisam ser
    // reconhecidas para o registro não duplicar o appid.
    let header = format!("{section}:");
    for line in yaml.lines() {
        let Some(rest) = line.strip_prefix(&header) else {
            continue;
        };
        let rest = rest.trim();
        if let Some(inner) = rest
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            if inner.split(',').map(str::trim).any(|item| item == key) {
                return true;
            }
        }
    }
    let body = section_body(yaml, section).join("\n");
    if body.is_empty() {
        return false;
    }
    let key = regex::escape(key);
    let list_or_map = Regex::new(&format!(r"(?m)^\s*(?:-\s*{key}\s*(?:#.*)?|{key}\s*:)\s*$")).ok();
    let map_value = Regex::new(&format!(r"(?m)^\s*{key}\s*:\s*\S")).ok();
    list_or_map.is_some_and(|regex| regex.is_match(&body))
        || map_value.is_some_and(|regex| regex.is_match(&body))
}

/// Remove uma chave de mapa e todas as linhas mais indentadas que a seguem.
fn remove_key_with_children(yaml: &str, section: &str, key: &str) -> String {
    let lines = yaml.split('\n').map(str::to_string).collect::<Vec<_>>();
    let marker = Regex::new(&format!(r"(?m)^{}:", regex::escape(section))).ok();
    let Some(marker) = marker else {
        return yaml.to_string();
    };
    let Some(start) = lines.iter().position(|line| marker.is_match(line)) else {
        return yaml.to_string();
    };
    let key = regex::escape(key);
    let key_re = Regex::new(&format!(r"^(\s+){key}\s*:")).ok();
    let Some(key_re) = key_re else {
        return yaml.to_string();
    };
    for index in start + 1..lines.len() {
        let line = &lines[index];
        if !line.is_empty()
            && !line.starts_with(|c: char| c.is_whitespace())
            && !line.starts_with('#')
        {
            break;
        }
        let Some(capture) = key_re.captures(line) else {
            continue;
        };
        let indent = capture[1].chars().count();
        let mut end = index + 1;
        while end < lines.len() {
            let candidate = &lines[end];
            if candidate.trim().is_empty() {
                break;
            }
            let child_indent = candidate
                .chars()
                .take_while(|character| character.is_whitespace())
                .count();
            if child_indent <= indent {
                break;
            }
            end += 1;
        }
        let mut updated = lines.clone();
        updated.splice(index..end, std::iter::empty());
        return updated.join("\n");
    }
    yaml.to_string()
}

/// Insere `<dlc>: "DLC"` sob `<appid>:` em `DlcData` (mapa de mapas).
fn add_dlc(yaml: &str, appid: &str, dlc: &str) -> String {
    let mut lines = yaml.split('\n').map(str::to_string).collect::<Vec<_>>();
    let Some(section) = lines.iter().position(|line| line.starts_with("DlcData:")) else {
        return format!("{yaml}\nDlcData:\n  {appid}:\n    {dlc}: \"DLC\"\n");
    };
    let appid_re = Regex::new(&format!(r"^(\s+){}\s*:", regex::escape(appid))).ok();
    if let Some(appid_re) = appid_re {
        for index in section + 1..lines.len() {
            let line = lines[index].clone();
            if !line.is_empty()
                && !line.starts_with(|c: char| c.is_whitespace())
                && !line.starts_with('#')
            {
                break;
            }
            if let Some(capture) = appid_re.captures(&line) {
                let indent = capture[1].to_string();
                lines.insert(index + 1, format!("{indent}  {dlc}: \"DLC\""));
                return lines.join("\n");
            }
        }
    }
    lines.insert(section + 1, format!("  {appid}:"));
    lines.insert(section + 2, format!("    {dlc}: \"DLC\""));
    lines.join("\n")
}

/// `registerSlssteam`: registra appid/token/DLCs no config.yaml (puro).
pub fn register_slssteam(config_yaml: &str, appid: &str, token: &str, dlcs: &[String]) -> String {
    let mut yaml = config_yaml.to_string();
    let appid = appid.trim();

    if !has_in_section(&yaml, "AdditionalApps", appid) {
        if Regex::new(r"(?m)^AdditionalApps:")
            .ok()
            .is_some_and(|re| re.is_match(&yaml))
        {
            if let Ok(re) = Regex::new(r"(?m)^AdditionalApps:\s*$") {
                yaml = re
                    .replacen(&yaml, 1, format!("AdditionalApps:\n  - {appid}"))
                    .into_owned();
            }
            if let Ok(re) = Regex::new(r"(?m)^(AdditionalApps:\s*\[)([^\]]*)\]") {
                yaml = re
                    .replacen(&yaml, 1, |captures: &regex::Captures<'_>| {
                        let prefix = &captures[1];
                        let inner = captures[2].trim();
                        if inner.is_empty() {
                            format!("{prefix}{appid}]")
                        } else {
                            format!("{prefix}{inner}, {appid}]")
                        }
                    })
                    .into_owned();
            }
        } else {
            yaml.push_str(&format!("\nAdditionalApps:\n  - {appid}\n"));
        }
    }

    if !token.is_empty() && !has_in_section(&yaml, "AppTokens", appid) {
        let has_section = Regex::new(r"(?m)^AppTokens:")
            .ok()
            .is_some_and(|re| re.is_match(&yaml));
        if has_section {
            if let Ok(re) = Regex::new(r"(?m)^AppTokens:\s*$") {
                yaml = re
                    .replacen(&yaml, 1, format!("AppTokens:\n  {appid}: {token}"))
                    .into_owned();
            }
            if let Ok(re) = Regex::new(r"(?m)^AppTokens:\s*\{\}\s*$") {
                yaml = re
                    .replacen(&yaml, 1, format!("AppTokens:\n  {appid}: {token}"))
                    .into_owned();
            }
        } else {
            yaml.push_str(&format!("\nAppTokens:\n  {appid}: {token}\n"));
        }
    }

    for dlc in dlcs {
        if !has_in_section(&yaml, "DlcData", dlc) {
            yaml = add_dlc(&yaml, appid, dlc);
        }
    }
    yaml
}

/// `removeFromSteam` da parte YAML: tira o appid de `AdditionalApps`,
/// `AppTokens` e `DlcData` (com os filhos).
pub fn remove_app_from_config(config_yaml: &str, appid: &str) -> String {
    let appid = appid.trim();
    let mut yaml = config_yaml.to_string();
    if let Ok(re) = Regex::new(&format!(
        r"(?m)^\s*-\s*{}\s*(?:#.*)?\n?",
        regex::escape(appid)
    )) {
        yaml = re.replace_all(&yaml, "").into_owned();
    }
    if let Ok(re) = Regex::new(r"(?m)^(AdditionalApps:\s*\[)([^\]]*)\]") {
        yaml = re
            .replacen(&yaml, 1, |captures: &regex::Captures<'_>| {
                let prefix = &captures[1];
                let ids = captures[2]
                    .split(',')
                    .map(str::trim)
                    .filter(|id| !id.is_empty() && *id != appid)
                    .collect::<Vec<_>>();
                if ids.is_empty() {
                    "AdditionalApps: []".to_string()
                } else {
                    format!("{prefix}{}]", ids.join(", "))
                }
            })
            .into_owned();
    }
    yaml = remove_key_with_children(&yaml, "AppTokens", appid);
    yaml = remove_key_with_children(&yaml, "DlcData", appid);
    yaml
}

fn limpa_legado(appid: &str, home: &Path, fs: &dyn LibraryFs) {
    let lua_dir = home
        .join(".config")
        .join("SLSsteam")
        .join("config")
        .join("stplug-in");
    let manifests = home.join(".config").join("SLSsteam").join("manifests");
    let lua = lua_dir.join(format!("{appid}.lua"));
    let mut depots = HashSet::new();
    if let Ok(text) = fs.read_to_string(&lua) {
        if let Ok(re) = Regex::new(r"addappid\s*\(\s*(\d+)") {
            for capture in re.captures_iter(&text) {
                depots.insert(capture[1].to_string());
            }
        }
    }
    let _ = fs.remove_file(&lua);
    if let Ok(names) = fs.read_dir(&manifests) {
        if let Ok(re) = Regex::new(r"^(\d+)_.*\.manifest$") {
            for name in names {
                let Some(capture) = re.captures(&name) else {
                    continue;
                };
                if depots.contains(&capture[1]) {
                    let _ = fs.remove_file(&manifests.join(name));
                }
            }
        }
    }
}

/// `addToSteam`: copia `.lua` (comentando `setManifestid`) e `.manifest` para
/// a Steam, sem baixar nada. O download fica por conta da Steam.
pub fn add_to_steam(
    appid: &str,
    manifest_dir: &Path,
    steam_base: Option<&Path>,
    home: &Path,
    fs: &dyn LibraryFs,
) -> AddToSteamResponse {
    if !crate::store::valid_appid(appid) {
        return AddToSteamResponse {
            ok: false,
            luas: None,
            error: Some("appid inválido".to_string()),
        };
    }
    let Some(base) = steam_base else {
        return AddToSteamResponse {
            ok: false,
            luas: None,
            error: Some("instalação da Steam não encontrada".to_string()),
        };
    };
    let stplug = base.join("config").join("stplug-in");
    let depot_cache = base.join("depotcache");
    if let Err(error) = fs.create_dir_all(&stplug) {
        return AddToSteamResponse {
            ok: false,
            luas: None,
            error: Some(error.to_string()),
        };
    }
    if let Err(error) = fs.create_dir_all(&depot_cache) {
        return AddToSteamResponse {
            ok: false,
            luas: None,
            error: Some(error.to_string()),
        };
    }
    limpa_legado(appid, home, fs);
    let comment = Regex::new(r"(?m)^(\s*)(setManifestid\()").expect("regex setManifestid");
    let mut luas = 0;
    match fs.read_dir(manifest_dir) {
        Ok(names) => {
            for name in names {
                if name.ends_with(".lua") {
                    let Ok(text) = fs.read_to_string(&manifest_dir.join(&name)) else {
                        continue;
                    };
                    let text = comment.replace_all(&text, "$1-- $2");
                    if fs.write(&stplug.join(&name), &text).is_err() {
                        return AddToSteamResponse {
                            ok: false,
                            luas: None,
                            error: Some("falha ao gravar .lua".to_string()),
                        };
                    }
                    luas += 1;
                } else if name.ends_with(".manifest") {
                    let _ = fs.copy(&manifest_dir.join(&name), &depot_cache.join(&name));
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return AddToSteamResponse {
                ok: false,
                luas: None,
                error: Some(error.to_string()),
            }
        }
    }
    if luas == 0 {
        return AddToSteamResponse {
            ok: false,
            luas: None,
            error: Some(
                "nenhum .lua no manifesto — baixe o manifesto antes (botão Baixar)".to_string(),
            ),
        };
    }
    AddToSteamResponse {
        ok: true,
        luas: Some(luas),
        error: None,
    }
}

/// `removeFromSteam`: apaga `.lua`/manifests do app e limpa o config.yaml.
pub fn remove_from_steam(
    appid: &str,
    steam_base: Option<&Path>,
    home: &Path,
    manifest_dir: Option<&Path>,
    fs: &dyn LibraryFs,
) -> RemoveResult {
    let id = appid.trim();
    if !crate::store::valid_appid(id) {
        return RemoveResult::failure("appid inválido");
    }
    limpa_legado(id, home, fs);
    if let Some(base) = steam_base {
        let stplug = base.join("config").join("stplug-in");
        if fs.exists(&stplug) {
            let mut depots = HashSet::new();
            if let Some(manifest_dir) = manifest_dir {
                if let Ok(names) = fs.read_dir(manifest_dir) {
                    if let Ok(re) = Regex::new(r"^(\d+)_.*\.manifest$") {
                        for name in names {
                            if let Some(capture) = re.captures(&name) {
                                depots.insert(capture[1].to_string());
                            }
                        }
                    }
                }
            }
            if let Ok(names) = fs.read_dir(&stplug) {
                for name in names {
                    if name == format!("{id}.lua") || name.starts_with(&format!("{id}_")) {
                        let _ = fs.remove_file(&stplug.join(name));
                    }
                }
            }
            let manifest_name = Regex::new(r"^(\d+)_.*\.manifest$").expect("regex manifest");
            for store in [
                base.join("depotcache"),
                home.join(".config").join("SLSsteam").join("manifests"),
            ] {
                if !fs.exists(&store) {
                    continue;
                }
                let Ok(names) = fs.read_dir(&store) else {
                    continue;
                };
                for name in names {
                    if let Some(capture) = manifest_name.captures(&name) {
                        if depots.contains(&capture[1]) {
                            let _ = fs.remove_file(&store.join(name));
                        }
                    }
                }
            }
        }
    }
    let config = home.join(".config").join("SLSsteam").join("config.yaml");
    if let Ok(text) = fs.read_to_string(&config) {
        let updated = remove_app_from_config(&text, id);
        let _ = fs.write(&config, &updated);
    }
    RemoveResult::ok()
}

/// `appidsInjetados` a partir de um config.yaml já lido.
pub fn appids_injected_from(
    config_yaml: Option<&str>,
    steam_base: Option<&Path>,
    home: &Path,
    fs: &dyn LibraryFs,
) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    if let Some(yaml) = config_yaml {
        if let Some(block) = yaml.split("AdditionalApps:").nth(1) {
            if let Ok(re) = Regex::new(r"(?m)^\s*-\s*(\d+)") {
                for capture in re.captures_iter(block) {
                    ids.insert(capture[1].to_string());
                }
            }
        }
    }
    let mut plugins = vec![home
        .join(".config")
        .join("SLSsteam")
        .join("config")
        .join("stplug-in")];
    if let Some(base) = steam_base {
        plugins.push(base.join("config").join("stplug-in"));
    }
    let re = Regex::new(r"^(\d+)(?:_.*)?\.lua$").expect("regex stplug");
    for plugin in plugins {
        let Ok(names) = fs.read_dir(&plugin) else {
            continue;
        };
        for name in names {
            if let Some(capture) = re.captures(&name) {
                ids.insert(capture[1].to_string());
            }
        }
    }
    ids
}

/// `appidsInjetados()` lendo o config.yaml padrão.
pub fn appids_injected(
    steam_base: Option<&Path>,
    home: &Path,
    fs: &dyn LibraryFs,
) -> BTreeSet<String> {
    let config = home.join(".config").join("SLSsteam").join("config.yaml");
    let yaml = fs.read_to_string(&config).ok();
    appids_injected_from(yaml.as_deref(), steam_base, home, fs)
}

// ── pending_games.json (store:addToLibrary/removeFromLibrary local) ───────

fn steam_id(appid: &str) -> String {
    format!("steam:{appid}")
}

/// Entry de `pending_games.json` criada pelo Add.
pub fn pending_stub(appid: &str, title: &str) -> Value {
    let base = format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}");
    json!({
        "id": steam_id(appid),
        "title": if title.trim().is_empty() { format!("Steam {appid}") } else { title.trim().to_string() },
        "launcher": "steam",
        "launch_cmd": ["steam", format!("steam://rungameid/{appid}")],
        "installed": false,
        "cover": format!("{base}/library_600x900.jpg"),
        "hero": format!("{base}/library_hero.jpg"),
        "logo": format!("{base}/logo.png"),
        "pendente": true,
    })
}

/// `adicionarStubPendente`: não duplica se já está na biblioteca ou pendente.
pub fn add_pending_stub(
    existing: &[Value],
    in_library: bool,
    appid: &str,
    title: &str,
    cover: Option<&str>,
    hero: Option<&str>,
) -> PendingUpdate {
    let id = steam_id(appid);
    if !crate::store::valid_appid(appid)
        || in_library
        || existing
            .iter()
            .any(|game| game.get("id").and_then(Value::as_str) == Some(id.as_str()))
    {
        return PendingUpdate {
            games: existing.to_vec(),
            changed: false,
        };
    }
    let mut stub = pending_stub(appid, title);
    if let Some(cover) = cover.filter(|cover| !cover.is_empty()) {
        stub["cover"] = Value::String(cover.to_string());
    }
    if let Some(hero) = hero.filter(|hero| !hero.is_empty()) {
        stub["hero"] = Value::String(hero.to_string());
    }
    let mut games = existing.to_vec();
    games.push(stub);
    PendingUpdate {
        games,
        changed: true,
    }
}

/// `removerStubPendente`.
pub fn remove_pending_stub(existing: &[Value], appid: &str) -> PendingUpdate {
    let id = steam_id(appid);
    let games = existing
        .iter()
        .filter(|game| game.get("id").and_then(Value::as_str) != Some(id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    PendingUpdate {
        changed: games.len() != existing.len(),
        games,
    }
}

/// `marcarJogoSteamInstalado`: atualiza/cria a entry com `installed: true`.
pub fn mark_steam_installed(
    existing: &[Value],
    appid: &str,
    title: &str,
    cover: Option<&str>,
    hero: Option<&str>,
) -> PendingUpdate {
    let id = steam_id(appid);
    if !Regex::new(r"^steam:\d+$")
        .map(|regex| regex.is_match(&id))
        .unwrap_or(false)
    {
        return PendingUpdate {
            games: existing.to_vec(),
            changed: false,
        };
    }
    let base = format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}");
    let mut games = existing.to_vec();
    let index = games
        .iter()
        .position(|game| game.get("id").and_then(Value::as_str) == Some(id.as_str()));
    let current = index.map(|index| &games[index]);
    let title = if title.trim().is_empty() {
        current
            .and_then(|game| game.get("title"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("Steam {appid}"))
    } else {
        title.trim().to_string()
    };
    let entry = json!({
        "id": id,
        "title": title,
        "launcher": "steam",
        "launch_cmd": ["steam", format!("steam://rungameid/{appid}")],
        "installed": true,
        "cover": cover.filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| current.and_then(|game| game.get("cover")).and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| format!("{base}/library_600x900.jpg")),
        "hero": hero.filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| current.and_then(|game| game.get("hero")).and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| format!("{base}/library_hero.jpg")),
        "logo": current.and_then(|game| game.get("logo")).and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{base}/logo.png")),
    });
    if let Some(index) = index {
        games[index] = entry;
    } else {
        games.push(entry);
    }
    PendingUpdate {
        games,
        changed: true,
    }
}

/// Entradas usadas por `store:status` que vêm de sondagens de processo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreStatusInputs {
    /// `"dotnet"`, caminho local ou `None` (runtime ausente).
    pub dotnet: Option<String>,
    pub depotdownloader: bool,
    pub hubcap_key: bool,
    pub slssteam: bool,
    pub steam_dir: String,
}

/// Envelope de `store:status` (mesmos nomes do oráculo).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoreStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dotnet: Option<String>,
    pub depotdownloader: bool,
    #[serde(rename = "hubcapKey")]
    pub hubcap_key: bool,
    pub slssteam: bool,
    #[serde(rename = "steamDir")]
    pub steam_dir: String,
    pub adicionados: Vec<String>,
}

/// Junta `appidsInjetados` + downloads do Arcadia, como `status()`.
pub fn store_status(
    inputs: StoreStatusInputs,
    libraries: &[SteamLibrary],
    home: &Path,
    steam_base: Option<&Path>,
    fs: &dyn LibraryFs,
) -> StoreStatus {
    let mut added = appids_injected(steam_base, home, fs);
    for download in arcadia_downloaded(libraries, fs) {
        added.insert(download.appid);
    }
    StoreStatus {
        dotnet: inputs.dotnet,
        depotdownloader: inputs.depotdownloader,
        hubcap_key: inputs.hubcap_key,
        slssteam: inputs.slssteam,
        steam_dir: inputs.steam_dir,
        adicionados: added.into_iter().collect(),
    }
}

/// Lê `pending_games.json` (lista; inválido vira vazio, como `readJsonFile`).
pub fn read_pending_games(path: &Path, fs: &dyn LibraryFs) -> Vec<Value> {
    fs.read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
}

/// Grava `pending_games.json` com a mesma indentação de 2 espaços do oráculo.
pub fn write_pending_games(path: &Path, games: &[Value], fs: &dyn LibraryFs) -> io::Result<()> {
    let text = serde_json::to_string_pretty(games)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    fs.write(path, &text)
}

#[cfg(test)]
#[path = "library_tests.rs"]
mod tests;
