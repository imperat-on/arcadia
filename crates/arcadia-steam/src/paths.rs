//! Caminhos da instalação da Steam — a parte de `app/electron/steam-path.js`
//! que alimenta conta e horas.
//!
//! A consulta ao registro do Windows **não** é feita aqui: ela já existe em
//! `arcadia-launch`. [`SteamPathEnv::registry_roots`] recebe as raízes
//! registradas pelo chamador, na mesma ordem de confiança do cliente congelado
//! (`SteamPath`, `WOW6432Node/InstallPath`, `InstallPath`).
//!
//! A lista de candidatos é pura; só [`find_steam_root`] e [`is_steam_root`]
//! tocam o disco.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Plataforma que define a lista de pastas candidatas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteamPlatform {
    Windows,
    Unix,
}

/// Entradas do ambiente usadas por `candidatosSteam()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamPathEnv {
    pub platform: SteamPlatform,
    /// `STEAM_DIR` (tem prioridade sobre tudo).
    pub steam_dir: Option<String>,
    pub program_files_x86: Option<String>,
    pub program_files: Option<String>,
    pub home: Option<String>,
    /// Raízes lidas do registro (somente Windows).
    pub registry_roots: Vec<String>,
}

impl SteamPathEnv {
    /// Ambiente do processo atual. As raízes do registro precisam ser
    /// injetadas (o `arcadia-launch` é a fonte delas).
    pub fn current(registry_roots: Vec<String>) -> Self {
        #[cfg(windows)]
        let platform = SteamPlatform::Windows;
        #[cfg(not(windows))]
        let platform = SteamPlatform::Unix;

        Self {
            platform,
            steam_dir: std::env::var("STEAM_DIR")
                .ok()
                .filter(|value| !value.is_empty()),
            program_files_x86: std::env::var("ProgramFiles(x86)")
                .ok()
                .filter(|value| !value.is_empty()),
            program_files: std::env::var("ProgramFiles")
                .ok()
                .filter(|value| !value.is_empty()),
            home: std::env::var("HOME").ok().filter(|value| !value.is_empty()),
            registry_roots,
        }
    }
}

impl Default for SteamPathEnv {
    fn default() -> Self {
        #[cfg(windows)]
        let platform = SteamPlatform::Windows;
        #[cfg(not(windows))]
        let platform = SteamPlatform::Unix;
        Self {
            platform,
            steam_dir: None,
            program_files_x86: None,
            program_files: None,
            home: None,
            registry_roots: Vec::new(),
        }
    }
}

/// Pastas candidatas a raiz da Steam, na plataforma atual, em ordem de
/// confiança. A lista é deduplicada preservando a ordem, como no JS.
pub fn candidate_roots(env: &SteamPathEnv) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    if let Some(steam_dir) = env.steam_dir.as_deref() {
        push_unique(&mut candidates, &mut seen, steam_dir.trim());
    }

    if env.platform == SteamPlatform::Windows {
        for root in &env.registry_roots {
            push_unique(&mut candidates, &mut seen, root);
        }
        for base in [
            env.program_files_x86.as_deref(),
            env.program_files.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            push_unique(&mut candidates, &mut seen, &join_windows(base, "Steam"));
        }
        for letter in 'C'..='Z' {
            let drive = format!("{letter}:\\");
            for tail in [
                "Steam",
                "Program Files (x86)\\Steam",
                "Program Files\\Steam",
                "Games\\Steam",
                "SteamLibrary",
            ] {
                push_unique(&mut candidates, &mut seen, &join_windows(&drive, tail));
            }
        }
    } else if let Some(home) = env.home.as_deref() {
        for tail in [
            ".steam/steam",
            ".local/share/Steam",
            ".steam/root",
            ".var/app/com.valvesoftware.Steam/.local/share/Steam",
            ".var/app/com.valvesoftware.Steam/data/Steam",
        ] {
            push_unique(&mut candidates, &mut seen, &join_unix(home, tail));
        }
    }

    candidates
}

/// Uma pasta é raiz da Steam se tem o executável, a appcache ou a steamapps.
pub fn is_steam_root(dir: &Path) -> bool {
    dir.join("steam.exe").exists()
        || dir.join("appcache").exists()
        || dir.join("steamapps").exists()
}

/// Primeira raiz válida da lista.
///
/// Windows: qualquer raiz reconhecida; sem nenhuma, devolve o primeiro
/// candidato. Linux: só aceita pasta com `steamapps`, senão o primeiro
/// candidato.
pub fn find_steam_root(env: &SteamPathEnv) -> Option<PathBuf> {
    let candidates = candidate_roots(env);
    let found = match env.platform {
        SteamPlatform::Windows => candidates
            .iter()
            .find(|candidate| is_steam_root(Path::new(candidate))),
        SteamPlatform::Unix => candidates
            .iter()
            .find(|candidate| Path::new(candidate).join("steamapps").exists()),
    };
    found
        .or_else(|| candidates.first())
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

pub fn config_dir(root: impl AsRef<Path>) -> PathBuf {
    root.as_ref().join("config")
}

pub fn login_users_path(root: impl AsRef<Path>) -> PathBuf {
    config_dir(root).join("loginusers.vdf")
}

pub fn config_vdf_path(root: impl AsRef<Path>) -> PathBuf {
    config_dir(root).join("config.vdf")
}

pub fn userdata_dir(root: impl AsRef<Path>, account_id: &str) -> PathBuf {
    root.as_ref().join("userdata").join(account_id)
}

pub fn local_config_path(root: impl AsRef<Path>, account_id: &str) -> PathBuf {
    userdata_dir(root, account_id)
        .join("config")
        .join("localconfig.vdf")
}

fn push_unique(candidates: &mut Vec<String>, seen: &mut HashSet<String>, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    let normalized = value.to_string();
    if seen.insert(normalized.clone()) {
        candidates.push(normalized);
    }
}

fn join_windows(base: &str, tail: &str) -> String {
    let base = base.trim_end_matches(['\\', '/']);
    if base.is_empty() {
        tail.to_string()
    } else {
        format!("{base}\\{tail}")
    }
}

fn join_unix(base: &str, tail: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.is_empty() {
        tail.to_string()
    } else {
        format!("{base}/{tail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::TestDir;

    #[test]
    fn windows_candidates_follow_steam_dir_registry_program_files_and_drives() {
        let env = SteamPathEnv {
            platform: SteamPlatform::Windows,
            steam_dir: Some("D:\\MeuSteam".into()),
            program_files_x86: Some("C:\\Program Files (x86)".into()),
            program_files: Some("C:\\Program Files\\".into()),
            home: Some("C:\\Users\\zes".into()),
            registry_roots: vec!["E:\\Steam".into(), "C:\\Program Files (x86)\\Steam".into()],
        };
        let candidates = candidate_roots(&env);
        assert_eq!(candidates[0], "D:\\MeuSteam");
        assert_eq!(candidates[1], "E:\\Steam");
        assert_eq!(candidates[2], "C:\\Program Files (x86)\\Steam");
        assert_eq!(candidates[3], "C:\\Program Files\\Steam");
        assert_eq!(candidates[4], "C:\\Steam");
        assert_eq!(candidates[5], "C:\\Games\\Steam");
        // dedup: a entrada do registro não se repete na varredura de discos
        assert_eq!(
            candidates
                .iter()
                .filter(|path| path.as_str() == "C:\\Program Files (x86)\\Steam")
                .count(),
            1
        );
        assert!(candidates.contains(&"Z:\\SteamLibrary".to_string()));
    }

    #[test]
    fn unix_candidates_are_the_frozen_list() {
        let env = SteamPathEnv {
            platform: SteamPlatform::Unix,
            home: Some("/home/zes/".into()),
            ..SteamPathEnv::default()
        };
        assert_eq!(
            candidate_roots(&env),
            vec![
                "/home/zes/.steam/steam".to_string(),
                "/home/zes/.local/share/Steam".to_string(),
                "/home/zes/.steam/root".to_string(),
                "/home/zes/.var/app/com.valvesoftware.Steam/.local/share/Steam".to_string(),
                "/home/zes/.var/app/com.valvesoftware.Steam/data/Steam".to_string(),
            ]
        );
    }

    #[test]
    fn empty_env_has_no_candidates_and_no_duplicates() {
        let env = SteamPathEnv {
            platform: SteamPlatform::Unix,
            ..SteamPathEnv::default()
        };
        assert!(candidate_roots(&env).is_empty());

        let env = SteamPathEnv {
            platform: SteamPlatform::Windows,
            steam_dir: Some("C:\\Steam".into()),
            registry_roots: vec!["C:\\Steam".into()],
            ..SteamPathEnv::default()
        };
        assert_eq!(
            candidate_roots(&env)
                .iter()
                .filter(|path| *path == "C:\\Steam")
                .count(),
            1
        );
    }

    #[test]
    fn recognizes_steam_roots_by_exe_appcache_or_steamapps() {
        let temp = TestDir::new("paths-root");
        assert!(!is_steam_root(temp.path()));
        std::fs::create_dir_all(temp.path().join("appcache")).unwrap();
        assert!(is_steam_root(temp.path()));

        let temp = TestDir::new("paths-steamapps");
        std::fs::create_dir_all(temp.path().join("steamapps")).unwrap();
        assert!(is_steam_root(temp.path()));

        let temp = TestDir::new("paths-exe");
        std::fs::write(temp.path().join("steam.exe"), b"x").unwrap();
        assert!(is_steam_root(temp.path()));
    }

    #[test]
    fn find_uses_the_fake_steam_dir_from_env_first() {
        let temp = TestDir::new("paths-find");
        let steam = temp.path().join("Steam");
        std::fs::create_dir_all(steam.join("steamapps")).unwrap();
        let env = SteamPathEnv {
            platform: SteamPlatform::Unix,
            steam_dir: Some(steam.to_string_lossy().into_owned()),
            home: Some(temp.path().join("home").to_string_lossy().into_owned()),
            ..SteamPathEnv::default()
        };
        assert_eq!(find_steam_root(&env), Some(steam));
    }

    #[test]
    fn find_falls_back_to_the_first_candidate() {
        let env = SteamPathEnv {
            platform: SteamPlatform::Unix,
            home: Some("/nao/existe".into()),
            ..SteamPathEnv::default()
        };
        assert_eq!(
            find_steam_root(&env),
            Some(PathBuf::from("/nao/existe/.steam/steam"))
        );

        let env = SteamPathEnv {
            platform: SteamPlatform::Windows,
            program_files_x86: Some("C:\\Program Files (x86)".into()),
            ..SteamPathEnv::default()
        };
        assert_eq!(
            find_steam_root(&env),
            Some(PathBuf::from("C:\\Program Files (x86)\\Steam"))
        );
    }

    #[test]
    fn convenience_paths_match_the_steam_layout() {
        assert_eq!(
            login_users_path("D:\\Steam"),
            PathBuf::from("D:\\Steam")
                .join("config")
                .join("loginusers.vdf")
        );
        assert_eq!(
            config_vdf_path("D:\\Steam"),
            PathBuf::from("D:\\Steam").join("config").join("config.vdf")
        );
        assert_eq!(
            local_config_path("D:\\Steam", "39738813"),
            PathBuf::from("D:\\Steam")
                .join("userdata")
                .join("39738813")
                .join("config")
                .join("localconfig.vdf")
        );
        assert_eq!(
            userdata_dir("D:\\Steam", "39738813"),
            PathBuf::from("D:\\Steam").join("userdata").join("39738813")
        );
    }
}
