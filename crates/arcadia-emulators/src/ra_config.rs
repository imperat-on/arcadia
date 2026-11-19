//! Credenciais RetroAchievements nos arquivos nativos dos emuladores.
//!
//! Port 1:1 de `app/electron/retroachievements/emulator-config.js`: PCSX2
//! (texto puro em `[Achievements]`), DuckStation (`[Cheevos]` com token
//! AES-128-CBC) e PPSSPP (usuário no ini, token em
//! `ppssp_retroachievements.dat`). Nenhuma rede e nenhum processo: só bytes e
//! caminhos, com filesystem injetado.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::crypto::{aes128_cbc_encrypt_zero_padded, base64_encode, sha256};
use crate::fs::{
    has_symlink_component, is_js_whitespace, is_regular, js_trim, path_string, Clock, EmulatorFs,
    FsKind, SystemClock, SystemFs,
};

pub const LOGIN_KEY_ROUNDS: usize = 100;

/// Resultado `{ ok, files }` / `{ ok: false, error }`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RaConfigResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RaConfigResult {
    pub fn success(files: Vec<String>) -> Self {
        Self {
            ok: true,
            files: Some(files),
            error: None,
        }
    }

    pub fn error(error: &str) -> Self {
        Self {
            ok: false,
            files: None,
            error: Some(error.to_string()),
        }
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// Opções dos escritores. `home` ausente usa o home do processo; `env` ausente
/// captura `process.env` (usado por `XDG_CONFIG_HOME`).
#[derive(Clone, Debug)]
pub struct RaConfigOptions {
    pub username: String,
    pub token: String,
    pub enabled: bool,
    pub home: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub portable: bool,
}

impl RaConfigOptions {
    pub fn new(username: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            token: token.into(),
            enabled: true,
            home: None,
            env: crate::fs::process_env(),
            portable: false,
        }
    }

    pub fn with_home(mut self, home: impl Into<PathBuf>) -> Self {
        self.home = Some(home.into());
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_portable(mut self, portable: bool) -> Self {
        self.portable = portable;
        self
    }

    pub fn with_env(mut self, env: BTreeMap<String, String>) -> Self {
        self.env = env;
        self
    }

    fn resolved_home(&self) -> PathBuf {
        match &self.home {
            Some(home) if !home.as_os_str().is_empty() => home.clone(),
            _ => default_home(),
        }
    }
}

pub fn default_home() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

// ---------------------------------------------------------------------------
// Caminhos por emulador
// ---------------------------------------------------------------------------

/// `pcsx2ConfigPaths`.
pub fn pcsx2_config_paths(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".config")
            .join("PCSX2")
            .join("inis")
            .join("PCSX2.ini"),
        home.join(".var")
            .join("app")
            .join("net.pcsx2.PCSX2")
            .join("config")
            .join("PCSX2")
            .join("inis")
            .join("PCSX2.ini"),
    ]
}

/// `duckstationConfigPaths`.
pub fn duckstation_config_paths(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".local")
            .join("share")
            .join("duckstation")
            .join("settings.ini"),
        home.join(".var")
            .join("app")
            .join("org.duckstation.DuckStation")
            .join("data")
            .join("duckstation")
            .join("settings.ini"),
    ]
}

/// `ppsspConfigDir`: respeita `XDG_CONFIG_HOME` quando não vazio.
pub fn ppssp_config_dir(home: &Path, env: &BTreeMap<String, String>) -> PathBuf {
    let base = env
        .get("XDG_CONFIG_HOME")
        .map(|value| js_trim(value))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    base.join("ppsspp")
}

/// `ppsspIniPath`.
pub fn ppssp_ini_path(home: &Path, env: &BTreeMap<String, String>) -> PathBuf {
    ppssp_config_dir(home, env).join("ppsspp.ini")
}

/// `ppsspSecretPath`: `<memStickDirectory>/PSP/SYSTEM/ppsspp_retroachievements.dat`.
pub fn ppssp_secret_path(home: &Path, env: &BTreeMap<String, String>) -> PathBuf {
    ppssp_config_dir(home, env)
        .join("PSP")
        .join("SYSTEM")
        .join("ppsspp_retroachievements.dat")
}

// ---------------------------------------------------------------------------
// INI
// ---------------------------------------------------------------------------

fn split_js_lines(text: &str) -> Vec<String> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect()
}

fn parse_ini_header(line: &str) -> Option<String> {
    let trimmed = js_trim(line);
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') || trimmed.len() < 3 {
        return None;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    if inner.is_empty() || inner.contains(']') {
        return None;
    }
    Some(js_trim(inner).to_string())
}

/// `^\s*([^=\s][^=]*?)\s*=` -> chave como o JS captura (trim final, espaços
/// internos preservados).
fn parse_ini_key(line: &str) -> Option<String> {
    let trimmed = line.trim_start_matches(is_js_whitespace);
    let equals = trimmed.find('=')?;
    let before = &trimmed[..equals];
    if before.is_empty() {
        return None;
    }
    let key = before.trim_end_matches(is_js_whitespace);
    if key.is_empty() || key.starts_with('=') {
        return None;
    }
    Some(key.to_string())
}

/// `upsertIniSection`: altera só as chaves pedidas na seção, preservando todo o
/// resto do arquivo (outras seções, comentários, ordem).
pub fn upsert_ini_section(text: &str, section_name: &str, key_values: &[(&str, &str)]) -> String {
    let lines = split_js_lines(text);
    let header = format!("[{section_name}]");
    let mut section_start: Option<usize> = None;
    let mut section_end = lines.len();
    for (index, line) in lines.iter().enumerate() {
        let Some(found) = parse_ini_header(line) else {
            continue;
        };
        if section_start.is_none() && found == section_name {
            section_start = Some(index);
        } else if section_start.is_some() && index > section_start.unwrap_or(0) {
            section_end = index;
            break;
        }
    }
    let mut pending: Vec<(String, String)> = key_values
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();

    let Some(start) = section_start else {
        let mut block = vec![header];
        block.extend(
            pending
                .iter()
                .map(|(key, value)| format!("{key} = {value}")),
        );
        let mut prefix = lines;
        if !prefix.is_empty() && prefix.last().map(String::as_str) != Some("") {
            prefix.push(String::new());
        }
        let mut output = prefix;
        output.extend(block);
        output.push(String::new());
        return output.join("\n");
    };

    let mut output: Vec<String> = lines[..=start].to_vec();
    for line in &lines[start + 1..section_end] {
        let key = parse_ini_key(line);
        if let Some(key) = key {
            if let Some(index) = pending
                .iter()
                .position(|(pending_key, _)| *pending_key == key)
            {
                let value = pending.remove(index).1;
                output.push(format!("{key} = {value}"));
                continue;
            }
        }
        output.push(line.clone());
    }
    output.extend(
        pending
            .iter()
            .map(|(key, value)| format!("{key} = {value}")),
    );
    output.extend(lines[section_end..].iter().cloned());
    output.join("\n")
}

fn is_symlink(file: &Path, fs: &dyn EmulatorFs) -> bool {
    fs.lstat(file)
        .map(|meta| meta.kind == FsKind::Symlink)
        .unwrap_or(false)
}

fn temporary_path(file: &Path, clock: &dyn Clock) -> PathBuf {
    PathBuf::from(format!(
        "{}.{}.{}.tmp",
        path_string(file),
        std::process::id(),
        clock.now_ms()
    ))
}

/// Escrita atômica (tmp + rename) do config, sem seguir symlink.
fn atomic_write_text(
    file: &Path,
    text: &str,
    fs: &dyn EmulatorFs,
    clock: &dyn Clock,
) -> Result<(), String> {
    let directory = file.parent().unwrap_or(Path::new("."));
    fs.mkdir_all(directory, 0o700)
        .map_err(|error| error.to_string())?;
    if is_symlink(file, fs) {
        return Err("arquivo_config_symlink".to_string());
    }
    let temporary = temporary_path(file, clock);
    if is_symlink(&temporary, fs) {
        return Err("arquivo_temp_symlink".to_string());
    }
    fs.write_new(&temporary, text.as_bytes(), 0o600)
        .map_err(|error| error.to_string())?;
    if is_symlink(&temporary, fs) {
        return Err("arquivo_temp_symlink".to_string());
    }
    fs.rename(&temporary, file)
        .map_err(|error| error.to_string())
}

fn read_text_if_exists(file: &Path, fs: &dyn EmulatorFs) -> Option<String> {
    if is_symlink(file, fs) {
        return None;
    }
    fs.read(file)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

fn apply_to_existing_dirs(
    candidates: &[PathBuf],
    fs: &dyn EmulatorFs,
    write: &mut dyn FnMut(&Path) -> Result<(), String>,
) -> Result<Vec<String>, String> {
    let mut touched = Vec::new();
    for file in candidates {
        let directory_exists = file
            .parent()
            .and_then(|directory| fs.lstat(directory))
            .map(|meta| meta.kind == FsKind::Directory)
            .unwrap_or(false);
        if !directory_exists {
            continue;
        }
        write(file)?;
        touched.push(path_string(file));
    }
    Ok(touched)
}

// ---------------------------------------------------------------------------
// DuckStation: derivação de chave e criptografia
// ---------------------------------------------------------------------------

fn read_machine_key(fs: &dyn EmulatorFs) -> Vec<u8> {
    let Some(bytes) = fs.read(Path::new("/etc/machine-id")) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    js_trim(&text).as_bytes().to_vec()
}

/// `duckstationLoginKey`: `SHA256(machineKey || username)` + 100 rounds.
pub fn duckstation_login_key(username: &str, portable: bool, fs: &dyn EmulatorFs) -> [u8; 32] {
    let machine_key = if portable {
        Vec::new()
    } else {
        read_machine_key(fs)
    };
    let mut buffer = machine_key;
    buffer.extend_from_slice(username.as_bytes());
    let mut key = sha256(&buffer);
    for _ in 0..LOGIN_KEY_ROUNDS {
        key = sha256(&key);
    }
    key
}

/// `encryptDuckstationToken`: AES-128-CBC com padding de zeros + base64.
pub fn encrypt_duckstation_token(
    token: &str,
    username: &str,
    portable: bool,
    fs: &dyn EmulatorFs,
) -> String {
    let key = duckstation_login_key(username, portable, fs);
    let mut aes_key = [0u8; 16];
    let mut iv = [0u8; 16];
    aes_key.copy_from_slice(&key[..16]);
    iv.copy_from_slice(&key[16..32]);
    let encrypted = aes128_cbc_encrypt_zero_padded(&aes_key, &iv, token.as_bytes());
    base64_encode(&encrypted)
}

// ---------------------------------------------------------------------------
// Escritores
// ---------------------------------------------------------------------------

/// `configurePcsx2`: token em texto puro na seção `[Achievements]`.
pub fn configure_pcsx2(
    options: &RaConfigOptions,
    fs: &dyn EmulatorFs,
    clock: &dyn Clock,
) -> Result<RaConfigResult, String> {
    let home = options.resolved_home();
    let candidates = pcsx2_config_paths(&home);
    let enabled = if options.enabled { "true" } else { "false" };
    let mut write = |file: &Path| {
        let current = read_text_if_exists(file, fs).unwrap_or_default();
        let next = upsert_ini_section(
            &current,
            "Achievements",
            &[
                ("Enabled", enabled),
                ("Username", &options.username),
                ("Token", &options.token),
            ],
        );
        atomic_write_text(file, &next, fs, clock)
    };
    let mut touched = apply_to_existing_dirs(&candidates, fs, &mut write)?;
    if touched.is_empty() {
        write(&candidates[0])?;
        touched.push(path_string(&candidates[0]));
    }
    Ok(RaConfigResult::success(touched))
}

/// `configureDuckstation`: token criptografado na seção `[Cheevos]`.
pub fn configure_duckstation(
    options: &RaConfigOptions,
    fs: &dyn EmulatorFs,
    clock: &dyn Clock,
) -> Result<RaConfigResult, String> {
    let home = options.resolved_home();
    let candidates = duckstation_config_paths(&home);
    let enabled = if options.enabled { "true" } else { "false" };
    let encrypted =
        encrypt_duckstation_token(&options.token, &options.username, options.portable, fs);
    let mut write = |file: &Path| {
        let current = read_text_if_exists(file, fs).unwrap_or_default();
        let next = upsert_ini_section(
            &current,
            "Cheevos",
            &[
                ("Enabled", enabled),
                ("Username", &options.username),
                ("Token", &encrypted),
            ],
        );
        atomic_write_text(file, &next, fs, clock)
    };
    let mut touched = apply_to_existing_dirs(&candidates, fs, &mut write)?;
    if touched.is_empty() {
        write(&candidates[0])?;
        touched.push(path_string(&candidates[0]));
    }
    Ok(RaConfigResult::success(touched))
}

/// `configurePpsspp`: usuário no `ppsspp.ini`, token no secret fora do ini.
pub fn configure_ppsspp(
    options: &RaConfigOptions,
    fs: &dyn EmulatorFs,
    clock: &dyn Clock,
) -> Result<RaConfigResult, String> {
    let home = options.resolved_home();
    let ini_file = ppssp_ini_path(&home, &options.env);
    let secret_file = ppssp_secret_path(&home, &options.env);
    let enabled = if options.enabled { "true" } else { "false" };
    let current = read_text_if_exists(&ini_file, fs).unwrap_or_default();
    let next = upsert_ini_section(
        &current,
        "Achievements",
        &[
            ("AchievementsEnable", enabled),
            ("AchievementsUserName", &options.username),
        ],
    );
    atomic_write_text(&ini_file, &next, fs, clock)?;
    atomic_write_text(&secret_file, &options.token, fs, clock)?;
    Ok(RaConfigResult::success(vec![
        path_string(&ini_file),
        path_string(&secret_file),
    ]))
}

/// `configureEmulatorCredentials`: escolhe o escritor pelo id exato.
pub fn configure_emulator_credentials(
    emulator_id: &str,
    options: &RaConfigOptions,
    fs: &dyn EmulatorFs,
    clock: &dyn Clock,
) -> Result<RaConfigResult, String> {
    if options.username.is_empty() || options.token.is_empty() {
        return Ok(RaConfigResult::error("credenciais_vazias"));
    }
    match emulator_id {
        "pcsx2" => configure_pcsx2(options, fs, clock),
        "duckstation" => configure_duckstation(options, fs, clock),
        "ppsspp" => configure_ppsspp(options, fs, clock),
        _ => Ok(RaConfigResult::error(
            "emulador_sem_suporte_retroachievements",
        )),
    }
}

/// Conveniência com filesystem/relógio reais (uso no app).
pub fn configure_pcsx2_system(options: &RaConfigOptions) -> Result<RaConfigResult, String> {
    configure_pcsx2(options, &SystemFs, &SystemClock)
}

pub fn configure_duckstation_system(options: &RaConfigOptions) -> Result<RaConfigResult, String> {
    configure_duckstation(options, &SystemFs, &SystemClock)
}

pub fn configure_ppsspp_system(options: &RaConfigOptions) -> Result<RaConfigResult, String> {
    configure_ppsspp(options, &SystemFs, &SystemClock)
}

pub fn apply_emulator_credentials(
    emulator_id: &str,
    options: &RaConfigOptions,
) -> Result<RaConfigResult, String> {
    configure_emulator_credentials(emulator_id, options, &SystemFs, &SystemClock)
}

/// `true` quando o caminho é absoluto e normalizável (helper de IPC).
pub fn is_absolute_config_path(value: &str) -> bool {
    crate::fs::normalize_absolute(value, "path").is_ok()
}

/// Basename usado em mensagens de erro/testes de caminho.
pub fn config_file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Remove apenas componentes `CurDir` (equivalente ao `normalize` mais simples
/// usado por `normalizeAbsolute` do oráculo).
pub fn normalized_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// `true` se o arquivo existe e não é symlink (helper de UI).
pub fn config_file_is_regular(path: &Path, fs: &dyn EmulatorFs) -> bool {
    !has_symlink_component(path, fs) && is_regular(path, fs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MemoryFs;
    use crate::test_support::TempDir;

    fn options(username: &str, token: &str, home: &Path) -> RaConfigOptions {
        RaConfigOptions::new(username, token).with_home(home)
    }

    #[test]
    fn upsert_cria_secao_preservando_resto() {
        assert_eq!(
            upsert_ini_section(
                "",
                "Achievements",
                &[("Enabled", "true"), ("Username", "u")]
            ),
            "\n[Achievements]\nEnabled = true\nUsername = u\n"
        );
        assert_eq!(
            upsert_ini_section(
                "[UI]\nfoo = bar\n",
                "Achievements",
                &[("Enabled", "true"), ("Username", "u")]
            ),
            "[UI]\nfoo = bar\n\n[Achievements]\nEnabled = true\nUsername = u\n"
        );
        assert_eq!(
            upsert_ini_section(
                "[UI]\nfoo = bar",
                "Achievements",
                &[("Enabled", "true"), ("Username", "u")]
            ),
            "[UI]\nfoo = bar\n\n[Achievements]\nEnabled = true\nUsername = u\n"
        );
    }

    #[test]
    fn upsert_atualiza_chaves_sem_tocar_em_outras() {
        let existing = "[UI]\nfoo=bar\n\n[Achievements]\nEnabled = false\nUsername = old\nSomeOtherKey = keep_me\n\n[Folders]\nBios = /bios\n";
        let out = upsert_ini_section(
            existing,
            "Achievements",
            &[("Enabled", "true"), ("Username", "joao")],
        );
        assert_eq!(
            out,
            "[UI]\nfoo=bar\n\n[Achievements]\nEnabled = true\nUsername = joao\nSomeOtherKey = keep_me\n\n[Folders]\nBios = /bios\n"
        );
        let existing = "[Achievements]\nEnabled = false\nKeep = yes\n\n[Next]\nx=1\n";
        assert_eq!(
            upsert_ini_section(
                existing,
                "Achievements",
                &[("Enabled", "true"), ("Username", "u")]
            ),
            "[Achievements]\nEnabled = true\nKeep = yes\n\nUsername = u\n[Next]\nx=1\n"
        );
    }

    #[test]
    fn encrypt_duckstation_bate_com_node_em_modo_portatil() {
        let fs = MemoryFs::new();
        assert_eq!(
            encrypt_duckstation_token("mytoken123", "joao", true, &fs),
            "gqVCEi04859wa6kxIf9dmA=="
        );
        assert_eq!(
            encrypt_duckstation_token("tok", "userA", true, &fs),
            "WwVXGqesYnd5NzK9nfhjYw=="
        );
        assert_eq!(
            encrypt_duckstation_token("tok", "userB", true, &fs),
            "ukFKg7MA2uSRx8rULboR+Q=="
        );
    }

    #[test]
    fn encrypt_duckstation_bate_com_node_com_machine_id() {
        let fs = MemoryFs::new();
        fs.add_file(Path::new("/etc/machine-id"), b"machine-123\n");
        assert_eq!(
            encrypt_duckstation_token("token-do-usuario", "joao", false, &fs),
            "SHKcoJfnlByd49XNmaXG1Q=="
        );
    }

    #[test]
    fn configure_pcsx2_escreve_texto_puro_e_preserva_arquivo() {
        let temp = TempDir::new("arcadia-ra-home");
        let config_file = pcsx2_config_paths(temp.path())[0].clone();
        std::fs::create_dir_all(config_file.parent().unwrap()).unwrap();
        std::fs::write(&config_file, "[UI]\nfoo = bar\n").unwrap();
        let result = configure_pcsx2(
            &options("joao", "tok-abc", temp.path()),
            &SystemFs,
            &SystemClock,
        )
        .unwrap();
        assert!(result.ok);
        assert_eq!(result.files, Some(vec![path_string(&config_file)]));
        let content = std::fs::read_to_string(&config_file).unwrap();
        assert_eq!(
            content,
            "[UI]\nfoo = bar\n\n[Achievements]\nEnabled = true\nUsername = joao\nToken = tok-abc\n"
        );
    }

    #[test]
    fn configure_pcsx2_cria_arquivo_sem_instalacao() {
        let temp = TempDir::new("arcadia-ra-home");
        let result = configure_pcsx2(
            &options("joao", "tok-abc", temp.path()),
            &SystemFs,
            &SystemClock,
        )
        .unwrap();
        assert!(result.ok);
        assert_eq!(
            result.files,
            Some(vec![path_string(&pcsx2_config_paths(temp.path())[0])])
        );
        assert!(pcsx2_config_paths(temp.path())[0].exists());
    }

    #[test]
    fn configure_duckstation_grava_token_criptografado() {
        let temp = TempDir::new("arcadia-ra-home");
        let result = configure_duckstation(
            &options("joao", "segredo-claro", temp.path()).with_portable(true),
            &SystemFs,
            &SystemClock,
        )
        .unwrap();
        assert!(result.ok);
        let files = result.files.unwrap();
        let content = std::fs::read_to_string(&files[0]).unwrap();
        assert!(content.contains("[Cheevos]"));
        assert!(content.contains("Username = joao"));
        assert!(!content.contains("segredo-claro"));
        assert!(content.contains("Token = I72pc2Qg71G9z6+nR9255g=="));
    }

    #[test]
    fn configure_ppsspp_separa_usuario_e_secret() {
        let temp = TempDir::new("arcadia-ra-home");
        let mut env = BTreeMap::new();
        env.insert(
            "XDG_CONFIG_HOME".to_string(),
            path_string(&temp.path().join(".config")),
        );
        let options = options("joao", "tok-secret", temp.path()).with_env(env.clone());
        let result = configure_ppsspp(&options, &SystemFs, &SystemClock).unwrap();
        assert!(result.ok);
        let ini = std::fs::read_to_string(ppssp_ini_path(temp.path(), &env)).unwrap();
        assert!(ini.contains("AchievementsUserName = joao"));
        assert!(!ini.contains("tok-secret"));
        let secret = std::fs::read_to_string(ppssp_secret_path(temp.path(), &env)).unwrap();
        assert_eq!(secret, "tok-secret");
        assert_eq!(
            result.files,
            Some(vec![
                path_string(&ppssp_ini_path(temp.path(), &env)),
                path_string(&ppssp_secret_path(temp.path(), &env)),
            ])
        );
    }

    #[test]
    fn apply_rejeita_emulador_sem_suporte_e_credenciais_vazias() {
        let temp = TempDir::new("arcadia-ra-home");
        let fs = MemoryFs::new();
        let clock = SystemClock;
        let result = configure_emulator_credentials(
            "rpcs3",
            &options("joao", "tok", temp.path()),
            &fs,
            &clock,
        )
        .unwrap();
        assert!(!result.ok);
        assert_eq!(
            result.error.as_deref(),
            Some("emulador_sem_suporte_retroachievements")
        );
        let result =
            configure_emulator_credentials("pcsx2", &options("", "", temp.path()), &fs, &clock)
                .unwrap();
        assert!(!result.ok);
        assert_eq!(result.error.as_deref(), Some("credenciais_vazias"));
    }

    #[test]
    fn configure_pcsx2_atualiza_todas_as_instalacoes_existentes() {
        let temp = TempDir::new("arcadia-ra-home");
        let native = pcsx2_config_paths(temp.path())[0].clone();
        let flatpak = pcsx2_config_paths(temp.path())[1].clone();
        std::fs::create_dir_all(native.parent().unwrap()).unwrap();
        std::fs::create_dir_all(flatpak.parent().unwrap()).unwrap();
        std::fs::write(&native, "[UI]\nfoo = bar\n").unwrap();
        let result = configure_pcsx2(
            &options("joao", "tok", temp.path()).with_enabled(false),
            &SystemFs,
            &SystemClock,
        )
        .unwrap();
        assert_eq!(
            result.files,
            Some(vec![path_string(&native), path_string(&flatpak)])
        );
        for file in [&native, &flatpak] {
            let content = std::fs::read_to_string(file).unwrap();
            assert!(content.contains("Enabled = false"));
            assert!(content.contains("Username = joao"));
        }
        assert!(std::fs::read_to_string(&native)
            .unwrap()
            .contains("[UI]\nfoo = bar"));
    }

    #[test]
    fn config_symlink_nao_e_seguido() {
        let fs = MemoryFs::new();
        fs.add_file(
            Path::new("/home/.config/PCSX2/inis/outside.ini"),
            b"outside",
        );
        fs.add_symlink(
            Path::new("/home/.config/PCSX2/inis/PCSX2.ini"),
            Path::new("/home/.config/PCSX2/inis/outside.ini"),
        );
        let result = configure_pcsx2(
            &options("joao", "tok", Path::new("/home")),
            &fs,
            &SystemClock,
        );
        assert!(result.is_err());
        assert_eq!(
            fs.read(Path::new("/home/.config/PCSX2/inis/outside.ini"))
                .unwrap(),
            b"outside"
        );
    }
}
