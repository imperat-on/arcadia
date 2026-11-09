//! Steam installation detection.
//!
//! Port of `app/electron/steam-path.js`.  The Windows registry is a *read*
//! probe: this crate never writes to it.  Registry and root scans are cached
//! for 30 seconds per resolver instance, with explicit invalidation.

use std::collections::{BTreeMap, HashSet};

use crate::clock::Clock;
use crate::platform::Platform;

pub const TTL_CACHE_MS: i64 = 30_000;
pub const DISK_LETTERS: &str = "CDEFGHIJKLMNOPQRSTUVWXYZ";
pub const DEFAULT_WINDOWS_STEAM: &str = "C:\\Program Files (x86)\\Steam";

/// Registry queries, in trust order.
pub const REGISTRY_QUERIES: [(&str, &str); 3] = [
    ("HKCU\\Software\\Valve\\Steam", "SteamPath"),
    ("HKLM\\SOFTWARE\\WOW6432Node\\Valve\\Steam", "InstallPath"),
    ("HKLM\\SOFTWARE\\Valve\\Steam", "InstallPath"),
];

pub trait PathFs {
    fn exists(&self, path: &str) -> bool;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemPathFs;

impl PathFs for SystemPathFs {
    fn exists(&self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }
}

/// Raw `reg query` probe.  Returning the command output keeps the parsing
/// contract (`REG_SZ <value>`) testable with fixtures.
pub trait RegistryProbe {
    fn query(&self, key: &str, value: &str) -> Option<String>;
}

#[derive(Clone, Debug, Default)]
pub struct NullRegistryProbe;

impl RegistryProbe for NullRegistryProbe {
    fn query(&self, _key: &str, _value: &str) -> Option<String> {
        None
    }
}

/// In-memory registry fixture keyed by `(key, value)` → raw output.
#[derive(Clone, Debug, Default)]
pub struct MapRegistryProbe {
    pub values: BTreeMap<(String, String), String>,
}

impl MapRegistryProbe {
    pub fn insert(&mut self, key: &str, value: &str, output: impl Into<String>) {
        self.values
            .insert((key.to_string(), value.to_string()), output.into());
    }
}

impl RegistryProbe for MapRegistryProbe {
    fn query(&self, key: &str, value: &str) -> Option<String> {
        self.values
            .get(&(key.to_string(), value.to_string()))
            .cloned()
    }
}

/// Read-only Windows `reg query`.
#[cfg(windows)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsRegistryProbe;

#[cfg(windows)]
impl RegistryProbe for WindowsRegistryProbe {
    fn query(&self, key: &str, value: &str) -> Option<String> {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let output = Command::new("reg")
            .args(["query", key, "/v", value])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

/// `REG_SZ\s+(.+?)\s*$` on one query output.
pub fn parse_reg_query_output(output: &str) -> Option<String> {
    let position = output.find("REG_SZ")?;
    let rest = &output[position + "REG_SZ".len()..];
    let rest = rest.trim_start();
    if rest.is_empty() {
        return None;
    }
    let line = rest.split(['\r', '\n']).next().unwrap_or(rest).trim();
    Some(line.trim_end_matches(['\\', '/']).to_string())
}

pub fn registry_roots(probe: &dyn RegistryProbe, platform: Platform) -> Vec<String> {
    if platform != Platform::Windows {
        return Vec::new();
    }
    let mut roots = Vec::new();
    for (key, value) in REGISTRY_QUERIES {
        if let Some(output) = probe.query(key, value) {
            if let Some(root) = parse_reg_query_output(&output) {
                roots.push(root);
            }
        }
    }
    roots
}

/// Candidate roots in trust order.
pub fn steam_candidates(
    platform: Platform,
    home: &str,
    env: &BTreeMap<String, String>,
    registry_roots: &[String],
) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(steam_dir) = env.get("STEAM_DIR").filter(|value| !value.is_empty()) {
        candidates.push(steam_dir.clone());
    }
    match platform {
        Platform::Windows => {
            candidates.extend(registry_roots.iter().cloned());
            for key in ["ProgramFiles(x86)", "ProgramFiles"] {
                if let Some(program_files) = env.get(key).filter(|value| !value.is_empty()) {
                    candidates.push(Platform::Windows.join(&[program_files, "Steam"]));
                }
            }
            for letter in DISK_LETTERS.chars() {
                let root = format!("{letter}:\\");
                candidates.push(Platform::Windows.join(&[&root, "Steam"]));
                candidates.push(Platform::Windows.join(&[&root, "Program Files (x86)", "Steam"]));
                candidates.push(Platform::Windows.join(&[&root, "Program Files", "Steam"]));
                candidates.push(Platform::Windows.join(&[&root, "Games", "Steam"]));
                candidates.push(Platform::Windows.join(&[&root, "SteamLibrary"]));
            }
        }
        Platform::Linux => {
            candidates.push(Platform::Linux.join(&[home, ".steam", "steam"]));
            candidates.push(Platform::Linux.join(&[home, ".local", "share", "Steam"]));
            candidates.push(Platform::Linux.join(&[home, ".steam", "root"]));
            candidates.push(Platform::Linux.join(&[
                home,
                ".var",
                "app",
                "com.valvesoftware.Steam",
                ".local",
                "share",
                "Steam",
            ]));
            candidates.push(Platform::Linux.join(&[
                home,
                ".var",
                "app",
                "com.valvesoftware.Steam",
                "data",
                "Steam",
            ]));
        }
    }
    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|candidate| !candidate.is_empty())
        .filter(|candidate| seen.insert(candidate.clone()))
        .collect()
}

pub fn looks_like_steam_root(fs: &dyn PathFs, dir: &str, platform: Platform) -> bool {
    fs.exists(&platform.join(&[dir, "steam.exe"]))
        || fs.exists(&platform.join(&[dir, "appcache"]))
        || fs.exists(&platform.join(&[dir, "steamapps"]))
}

pub fn find_steam_dir_with(platform: Platform, candidates: &[String], fs: &dyn PathFs) -> String {
    match platform {
        Platform::Windows => candidates
            .iter()
            .find(|candidate| looks_like_steam_root(fs, candidate, platform))
            .cloned()
            .or_else(|| candidates.first().cloned())
            .unwrap_or_else(|| DEFAULT_WINDOWS_STEAM.to_string()),
        Platform::Linux => candidates
            .iter()
            .find(|candidate| fs.exists(&Platform::Linux.join(&[candidate, "steamapps"])))
            .cloned()
            .or_else(|| candidates.first().cloned())
            .unwrap_or_default(),
    }
}

pub fn find_steam_exe_with(platform: Platform, steam_dir: &str, fs: &dyn PathFs) -> String {
    if platform != Platform::Windows {
        return "steam".to_string();
    }
    let exe = Platform::Windows.join(&[steam_dir, "steam.exe"]);
    if fs.exists(&exe) {
        exe
    } else {
        "steam.exe".to_string()
    }
}

/// Stateful resolver with the same short TTL cache as the Electron client.
pub struct SteamPathResolver<'a> {
    pub platform: Platform,
    pub home: String,
    pub env: BTreeMap<String, String>,
    pub fs: &'a dyn PathFs,
    pub registry: &'a dyn RegistryProbe,
    pub clock: &'a dyn Clock,
    pub ttl_ms: i64,
    registry_cache: Option<(i64, Vec<String>)>,
    root_cache: Option<(i64, String)>,
}

impl<'a> SteamPathResolver<'a> {
    pub fn new(
        platform: Platform,
        home: impl Into<String>,
        env: BTreeMap<String, String>,
        fs: &'a dyn PathFs,
        registry: &'a dyn RegistryProbe,
        clock: &'a dyn Clock,
    ) -> Self {
        Self {
            platform,
            home: home.into(),
            env,
            fs,
            registry,
            clock,
            ttl_ms: TTL_CACHE_MS,
            registry_cache: None,
            root_cache: None,
        }
    }

    pub fn invalidate(&mut self) {
        self.registry_cache = None;
        self.root_cache = None;
    }

    pub fn registry_roots(&mut self) -> Vec<String> {
        if self.platform != Platform::Windows {
            return Vec::new();
        }
        let now = self.clock.now_ms();
        if let Some((at, value)) = &self.registry_cache {
            if now - at < self.ttl_ms {
                return value.clone();
            }
        }
        let value = registry_roots(self.registry, self.platform);
        self.registry_cache = Some((now, value.clone()));
        value
    }

    pub fn candidates(&mut self) -> Vec<String> {
        let registry = self.registry_roots();
        steam_candidates(self.platform, &self.home, &self.env, &registry)
    }

    pub fn find_steam_dir(&mut self) -> String {
        let now = self.clock.now_ms();
        if let Some((at, value)) = &self.root_cache {
            if now - at < self.ttl_ms {
                return value.clone();
            }
        }
        let candidates = self.candidates();
        let value = find_steam_dir_with(self.platform, &candidates, self.fs);
        self.root_cache = Some((now, value.clone()));
        value
    }

    pub fn find_steam_exe(&mut self) -> String {
        let dir = self.find_steam_dir();
        find_steam_exe_with(self.platform, &dir, self.fs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;
    use std::cell::{Cell, RefCell};

    struct FakeFs {
        existing: RefCell<HashSet<String>>,
    }

    impl FakeFs {
        fn new(paths: &[&str]) -> Self {
            Self {
                existing: RefCell::new(paths.iter().map(|v| v.to_string()).collect()),
            }
        }
    }

    impl PathFs for FakeFs {
        fn exists(&self, path: &str) -> bool {
            self.existing.borrow().contains(path)
        }
    }

    struct CountingRegistry {
        values: BTreeMap<(String, String), String>,
        calls: Cell<usize>,
    }

    impl RegistryProbe for CountingRegistry {
        fn query(&self, key: &str, value: &str) -> Option<String> {
            self.calls.set(self.calls.get() + 1);
            self.values
                .get(&(key.to_string(), value.to_string()))
                .cloned()
        }
    }

    #[test]
    fn parses_reg_sz_and_strips_trailing_slashes() {
        let output = "\r\nHKEY_CURRENT_USER\\Software\\Valve\\Steam\r\n    SteamPath    REG_SZ    D:\\Steam\\\r\n\r\n";
        assert_eq!(parse_reg_query_output(output).as_deref(), Some("D:\\Steam"));
        assert_eq!(parse_reg_query_output("no value"), None);
        assert_eq!(parse_reg_query_output("REG_SZ   \n"), None);
    }

    #[test]
    fn windows_candidates_start_with_registry_and_scan_all_drives() {
        let env = BTreeMap::from([
            (
                "ProgramFiles(x86)".to_string(),
                "C:\\Program Files (x86)".to_string(),
            ),
            ("ProgramFiles".to_string(), "C:\\Program Files".to_string()),
        ]);
        let candidates = steam_candidates(
            Platform::Windows,
            "C:\\Users\\z",
            &env,
            &["D:\\Steam".to_string()],
        );
        assert_eq!(candidates[0], "D:\\Steam");
        assert!(candidates.contains(&"C:\\Program Files (x86)\\Steam".to_string()));
        assert!(candidates.contains(&"E:\\Games\\Steam".to_string()));
        assert!(candidates.contains(&"Z:\\SteamLibrary".to_string()));
        assert!(candidates.contains(&"C:\\Steam".to_string()));
    }

    #[test]
    fn linux_candidates_are_the_known_layouts() {
        let candidates = steam_candidates(Platform::Linux, "/home/z", &BTreeMap::new(), &[]);
        assert!(candidates.contains(&"/home/z/.steam/steam".to_string()));
        assert!(candidates.contains(&"/home/z/.local/share/Steam".to_string()));
        assert!(candidates
            .contains(&"/home/z/.var/app/com.valvesoftware.Steam/.local/share/Steam".to_string()));
        assert!(!candidates.iter().any(|value| value.starts_with("C:\\")));
    }

    #[test]
    fn steam_dir_prefers_steam_dir_and_falls_back_like_node() {
        let fs = FakeFs::new(&["/tmp/steam/appcache"]);
        let env = BTreeMap::from([("STEAM_DIR".to_string(), "/tmp/steam".to_string())]);
        let candidates = steam_candidates(Platform::Linux, "/home/z", &env, &[]);
        assert_eq!(
            find_steam_dir_with(Platform::Linux, &candidates, &fs),
            "/tmp/steam"
        );
        assert_eq!(
            find_steam_exe_with(Platform::Linux, "/tmp/steam", &fs),
            "steam"
        );

        let empty = FakeFs::new(&[]);
        assert_eq!(
            find_steam_dir_with(Platform::Windows, &[], &empty),
            DEFAULT_WINDOWS_STEAM
        );
        assert_eq!(
            find_steam_exe_with(Platform::Windows, DEFAULT_WINDOWS_STEAM, &empty),
            "steam.exe"
        );
    }

    #[test]
    fn registry_and_root_scans_are_cached_and_invalidatable() {
        let mut registry = CountingRegistry {
            values: BTreeMap::new(),
            calls: Cell::new(0),
        };
        registry.values.insert(
            (
                "HKCU\\Software\\Valve\\Steam".to_string(),
                "SteamPath".to_string(),
            ),
            "    SteamPath    REG_SZ    D:\\Steam".to_string(),
        );
        let fs = FakeFs::new(&["D:\\Steam\\steam.exe"]);
        let clock = ManualClock::new(1_000);
        let mut resolver = SteamPathResolver::new(
            Platform::Windows,
            "C:\\Users\\z",
            BTreeMap::new(),
            &fs,
            &registry,
            &clock,
        );
        assert_eq!(resolver.find_steam_dir(), "D:\\Steam");
        assert_eq!(resolver.find_steam_exe(), "D:\\Steam\\steam.exe");
        let calls_after_first = registry.calls.get();
        assert!(calls_after_first >= 1);
        assert_eq!(resolver.find_steam_dir(), "D:\\Steam");
        assert_eq!(registry.calls.get(), calls_after_first, "cached");

        resolver.invalidate();
        clock.advance(TTL_CACHE_MS + 1);
        assert_eq!(resolver.find_steam_dir(), "D:\\Steam");
        assert!(registry.calls.get() > calls_after_first, "requeried");
    }

    #[test]
    fn no_registry_access_off_windows() {
        let fs = FakeFs::new(&[]);
        let clock = ManualClock::new(0);
        let mut resolver = SteamPathResolver::new(
            Platform::Linux,
            "/home/z",
            BTreeMap::new(),
            &fs,
            &NullRegistryProbe,
            &clock,
        );
        assert!(resolver.registry_roots().is_empty());
    }
}
