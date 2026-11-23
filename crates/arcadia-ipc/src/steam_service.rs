//! Serviço Steam do IPC: raiz da instalação, horas locais e news por appid.

use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path};

use arcadia_launch::clock::SystemClock;
use arcadia_launch::platform::Platform;
use arcadia_launch::steam_path::{RegistryProbe, SteamPathResolver, SystemPathFs};
#[cfg(not(windows))]
use arcadia_launch::steam_path::NullRegistryProbe;
#[cfg(windows)]
use arcadia_launch::steam_path::WindowsRegistryProbe;
use arcadia_steam::account::{
    local_config_path, parse_login_users, select_active_login_user, SteamAccount,
};
use arcadia_steamstore::library::{scan_manifests, steam_libraries, LibraryFs, SystemFs};
use serde_json::{json, Value};
use arcadia_steam::playtime::{locais_de_contas, playtime_map, PlaytimeReading};

/// Resolve a raiz da Steam com o mesmo resolver do launch.
pub fn steam_root() -> String {
    let platform = Platform::current();
    let home = arcadia_store::paths::home_dir().to_string_lossy().into_owned();
    let env: BTreeMap<String, String> = std::env::vars().collect();
    let fs = SystemPathFs;
    let clock = SystemClock;
    #[cfg(windows)]
    let registry: &dyn RegistryProbe = &WindowsRegistryProbe;
    #[cfg(not(windows))]
    let registry: &dyn RegistryProbe = &NullRegistryProbe;
    let mut resolver =
        SteamPathResolver::new(platform, home, env, &fs, registry, &clock);
    resolver.find_steam_dir()
}

pub fn normalize_appid(value: &str) -> String {
    value
        .trim()
        .strip_prefix("steam:")
        .unwrap_or(value.trim())
        .trim()
        .to_string()
}

/// Minutos por appid somados entre as contas locais da Steam + persona ativa.
pub fn hours_by_appid() -> Result<(BTreeMap<String, u64>, String), String> {
    let root = steam_root();
    if root.trim().is_empty() {
        return Err("steam_ausente".to_string());
    }
    hours_from_root(Path::new(&root))
}

/// Mesma agregação, com raiz injetada — permite testar sem Steam instalada.
pub fn hours_from_root(root: &Path) -> Result<(BTreeMap<String, u64>, String), String> {
    let login_path = root.join("config").join("loginusers.vdf");
    let login_text =
        std::fs::read_to_string(&login_path).map_err(|_| "loginusers_ausente".to_string())?;
    let accounts =
        parse_login_users(&login_text).map_err(|error| format!("loginusers_invalido: {error}"))?;
    let persona = select_active_login_user(&login_text)
        .ok()
        .flatten()
        .map(|selected| selected.account.persona)
        .unwrap_or_default();

    let mut readings = Vec::new();
    for account in accounts {
        let Some(path) = local_config_path(root, &account.steamid) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(map) = playtime_map(&text) else {
            continue;
        };
        readings.push(PlaytimeReading {
            horas: map,
            persona: account.persona,
            steamid: account.steamid,
        });
    }

    let by_account = locais_de_contas(&readings);
    let totals = arcadia_steam::playtime::somar_por_appid(&by_account);
    let totals: BTreeMap<String, u64> = totals
        .into_iter()
        .map(|(appid, minutes)| (appid, minutes.round().max(0.0) as u64))
        .collect();
    Ok((totals, persona))
}

const LINK_FILE: &str = "steam_account.json";

/// Appids Steam da resposta que ainda estão `installed:false` — candidatos a
/// reconhecimento. Sem candidato nenhum, a descoberta não toca o disco.
pub(crate) fn pending_steam_appids(games: &[Value]) -> HashSet<String> {
    games
        .iter()
        .filter(|game| game.get("installed").and_then(Value::as_bool) == Some(false))
        .filter_map(steam_appid)
        .map(str::to_string)
        .collect()
}

/// Promove somente `installed:false` de jogo Steam quando o ACF real em
/// qualquer biblioteca prova instalação. Muda exclusivamente a resposta de
/// `library:get`: nada é persistido e os demais campos ficam intactos.
pub(crate) fn apply_installed_flags(games: &mut [Value], installed: &HashSet<String>) {
    for game in games.iter_mut() {
        if game.get("installed").and_then(Value::as_bool) != Some(false) {
            continue;
        }
        let Some(appid) = steam_appid(game) else {
            continue;
        };
        if !installed.contains(appid) {
            continue;
        }
        if let Some(object) = game.as_object_mut() {
            object.insert("installed".into(), Value::Bool(true));
        }
    }
}

/// Appids entre `candidates` com ACF instalado (`StateFlags & 4`) e diretório
/// `common/<installdir>` real na MESMA biblioteca. Cobre a principal, as
/// secundárias e o Flatpak pelo mesmo `steam_libraries` do resto do app.
pub(crate) fn installed_appids_in(home: &Path, candidates: &HashSet<String>) -> HashSet<String> {
    let mut installed = HashSet::new();
    for library in steam_libraries(home, &SystemFs, None) {
        for manifest in scan_manifests(&library.path, &SystemFs) {
            if !candidates.contains(&manifest.appid)
                || !manifest.is_installed()
                || !installdir_exists(&library.path, &manifest.installdir)
            {
                continue;
            }
            installed.insert(manifest.appid);
        }
    }
    installed
}

/// `common/<installdir>` precisa ser um diretório real dentro da biblioteca.
/// Entradas vazias, absolutas ou com `..` nunca valem como instalação.
fn installdir_exists(steamapps: &Path, installdir: &str) -> bool {
    let dir = Path::new(installdir);
    if installdir.trim().is_empty()
        || dir.is_absolute()
        || dir
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return false;
    }
    SystemFs.is_dir(&steamapps.join("common").join(dir))
}

/// `steam:<appid>` ou id numérico com launcher Steam.
fn steam_appid(game: &Value) -> Option<&str> {
    let id = game.get("id").and_then(Value::as_str)?.trim();
    let candidate = match id.strip_prefix("steam:") {
        Some(appid) => appid,
        None if game
            .get("launcher")
            .and_then(Value::as_str)
            .is_some_and(|launcher| launcher.eq_ignore_ascii_case("steam")) =>
        {
            id
        }
        None => return None,
    };
    (!candidate.is_empty() && candidate.chars().all(|character| character.is_ascii_digit()))
        .then_some(candidate)
}

fn read_link(data_dir: &Path, account: Option<&str>) -> Option<Value> {
    let raw = std::fs::read_to_string(arcadia_store::accounts::account_file(
        data_dir, account, LINK_FILE,
    ))
    .ok()?;
    serde_json::from_str::<Value>(&raw).ok().filter(|value| {
        value.get("steamid").and_then(Value::as_str).is_some()
    })
}

fn write_link(data_dir: &Path, account: Option<&str>, value: &Value) -> bool {
    let path = arcadia_store::accounts::account_file(data_dir, account, LINK_FILE);
    let text = match serde_json::to_string_pretty(value) {
        Ok(text) => text,
        Err(_) => return false,
    };
    arcadia_store::atomic::write_atomic_text(&path, &text).is_ok()
}

fn accounts_json(accounts: &[SteamAccount]) -> Value {
    Value::Array(
        accounts
            .iter()
            .map(|account| json!({
                "steamid": account.steamid,
                "persona": account.persona
            }))
            .collect(),
    )
}

/// Status de vínculo/captura, com a mesma regra do `steam-account.js`.
pub fn status_from_root(
    root: &Path,
    data_dir: &Path,
    account: Option<&str>,
    auto: bool,
) -> Value {
    let login_path = root.join("config").join("loginusers.vdf");
    let Ok(login_text) = std::fs::read_to_string(login_path) else {
        return json!({
            "vinculo": null,
            "contaAtual": null,
            "permitido": auto,
            "forcado": false,
            "vinculoOk": false,
            "semSteam": true,
            "auto": auto,
            "contasSteam": []
        });
    };
    let accounts = parse_login_users(&login_text).unwrap_or_default();
    let active = select_active_login_user(&login_text)
        .ok()
        .flatten()
        .map(|selected| selected.account);
    let mut link = read_link(data_dir, account);
    if link.is_none() {
        if let Some(active) = active.as_ref() {
            let value = json!({
                "steamid": active.steamid,
                "persona": active.persona,
                "vinculadoEm": chrono::Utc::now().timestamp_millis()
            });
            if write_link(data_dir, account, &value) {
                link = Some(value);
            }
        }
    }
    let conta_atual = active.as_ref().map(|active| json!({
        "steamid": active.steamid,
        "persona": active.persona
    }));
    let bate = match (active.as_ref(), link.as_ref()) {
        (Some(active), Some(link)) => {
            link.get("steamid").and_then(Value::as_str) == Some(active.steamid.as_str())
        }
        _ => false,
    };
    let sem_steam = active.is_none();
    json!({
        "vinculo": link.map(|value| json!({
            "steamid": value.get("steamid").cloned().unwrap_or(Value::Null),
            "persona": value.get("persona").cloned().unwrap_or(Value::Null)
        })),
        "contaAtual": conta_atual,
        "permitido": auto && (sem_steam || bate),
        "forcado": false,
        "vinculoOk": bate,
        "semSteam": sem_steam,
        "auto": auto,
        "contasSteam": accounts_json(&accounts)
    })
}

pub fn status(data_dir: &Path, account: Option<&str>, auto: bool) -> Value {
    let root = steam_root();
    if root.trim().is_empty() {
        return status_from_root(Path::new(""), data_dir, account, auto);
    }
    status_from_root(Path::new(&root), data_dir, account, auto)
}

pub fn link_now(data_dir: &Path, account: Option<&str>) -> Value {
    let root = steam_root();
    let login_path = Path::new(&root).join("config").join("loginusers.vdf");
    let Ok(text) = std::fs::read_to_string(login_path) else {
        return json!({ "ok": false, "motivo": "steam_ausente" });
    };
    let Some(active) = select_active_login_user(&text).ok().flatten() else {
        return json!({ "ok": false, "motivo": "sem_conta_steam" });
    };
    let value = json!({
        "steamid": active.account.steamid,
        "persona": active.account.persona,
        "vinculadoEm": chrono::Utc::now().timestamp_millis()
    });
    if !write_link(data_dir, account, &value) {
        return json!({ "ok": false, "motivo": "falha_ao_gravar" });
    }
    json!({ "ok": true, "vinculo": value })
}

pub fn hours_json() -> serde_json::Value {
    match hours_by_appid() {
        Ok((hours, persona)) => {
            let map: serde_json::Map<String, serde_json::Value> = hours
                .into_iter()
                .map(|(appid, minutes)| (appid, serde_json::json!(minutes)))
                .collect();
            serde_json::json!({ "ok": true, "horas": map, "persona": persona })
        }
        Err(motivo) => serde_json::json!({ "ok": false, "horas": {}, "motivo": motivo }),
    }
}

pub fn game_hours_json(appid: &str) -> serde_json::Value {
    let key = normalize_appid(appid);
    match hours_by_appid() {
        Ok((hours, persona)) => serde_json::json!({
            "ok": true,
            "minutos": hours.get(&key).copied().unwrap_or(0),
            "persona": persona
        }),
        Err(motivo) => serde_json::json!({ "ok": false, "minutos": 0, "motivo": motivo }),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_installed_flags, hours_from_root, installed_appids_in, normalize_appid,
        pending_steam_appids, status_from_root,
    };
    use serde_json::json;
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    fn temp_home(tag: &str) -> PathBuf {
        let home = std::env::temp_dir().join(format!(
            "arcadia-steam-installed-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&home).unwrap();
        home
    }

    fn write_acf(steamapps: &Path, appid: &str, state_flags: u64, installdir: &str) {
        std::fs::create_dir_all(steamapps).unwrap();
        let text = format!(
            "\"AppState\"\n{{\n\t\"appid\" \"{appid}\"\n\t\"name\" \"Fixture {appid}\"\n\t\"StateFlags\" \"{state_flags}\"\n\t\"installdir\" \"{installdir}\"\n}}\n"
        );
        std::fs::write(steamapps.join(format!("appmanifest_{appid}.acf")), text).unwrap();
    }

    fn write_install(steamapps: &Path, installdir: &str) {
        std::fs::create_dir_all(steamapps.join("common").join(installdir)).unwrap();
    }

    #[test]
    fn installed_requires_acf_flag_and_real_folder_including_secondary_library() {
        let home = temp_home("discovery");
        let primary = home.join(".steam").join("steam");
        let secondary = home.join("SteamLibrary");
        let primary_apps = primary.join("steamapps");
        let secondary_apps = secondary.join("steamapps");

        std::fs::create_dir_all(&primary_apps).unwrap();
        std::fs::write(
            primary_apps.join("libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\" \"{}\"\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\" \"{}\"\n\t}}\n}}\n",
                primary.display(),
                secondary.display()
            ),
        )
        .unwrap();

        // Principal: instalado de verdade (installdir com espaço, como
        // "Stardew Valley").
        write_acf(&primary_apps, "100", 4, "Game A");
        write_install(&primary_apps, "Game A");
        // Secundária: instalado de verdade.
        write_acf(&secondary_apps, "200", 4, "GameB");
        write_install(&secondary_apps, "GameB");
        // Download incompleto: flag de instalado ausente, pasta presente.
        write_acf(&primary_apps, "300", 1026, "GameC");
        write_install(&primary_apps, "GameC");
        // ACF instalado, mas sem a pasta real em common/.
        write_acf(&primary_apps, "400", 4, "GameD");
        // Sem ACF: pasta sozinha não é evidência.
        write_install(&primary_apps, "GameE");

        let candidates: HashSet<String> = ["100", "200", "300", "400", "700"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let installed = installed_appids_in(&home, &candidates);
        assert_eq!(
            installed,
            ["100".to_string(), "200".to_string()]
                .into_iter()
                .collect::<HashSet<_>>()
        );

        let mut games = vec![
            json!({
                "id": "steam:100", "title": "Game A", "launcher": "steam",
                "launch_cmd": ["steam", "steam://rungameid/100"],
                "installed": false, "pendente": true, "playtime_minutes": 42,
                "cover": "https://example.test/custom-cover.jpg"
            }),
            json!({ "id": "steam:200", "title": "Game B", "launcher": "steam", "installed": false }),
            json!({ "id": "steam:300", "title": "Game C", "launcher": "steam", "installed": false }),
            json!({ "id": "steam:400", "title": "Game D", "launcher": "steam", "installed": false }),
            json!({ "id": "steam:700", "title": "Game E", "launcher": "steam", "installed": false }),
            json!({
                "id": "custom:hero", "title": "Custom", "launcher": "custom",
                "installed": false, "exe": "/games/hero", "playtime_minutes": 7
            }),
        ];
        apply_installed_flags(&mut games, &installed);

        assert_eq!(games[0]["installed"], true);
        assert_eq!(games[0]["playtime_minutes"], 42);
        assert_eq!(games[0]["cover"], "https://example.test/custom-cover.jpg");
        assert_eq!(games[0]["launch_cmd"][0], "steam");
        assert_eq!(games[1]["installed"], true, "biblioteca secundária ignorada");
        assert_eq!(games[2]["installed"], false, "download incompleto virou instalado");
        assert_eq!(games[3]["installed"], false, "pasta ausente virou instalada");
        assert_eq!(games[4]["installed"], false, "ACF ausente virou instalado");
        assert_eq!(games[5]["installed"], false, "custom foi alterado");
        assert_eq!(games[5]["exe"], "/games/hero");

        // A varredura só acontece quando existe candidato Steam pendente.
        assert!(pending_steam_appids(&[]).is_empty());
        assert!(pending_steam_appids(&[json!({
            "id": "steam:100", "launcher": "steam", "installed": true
        })])
        .is_empty());

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn appid_prefix_is_removed() {
        assert_eq!(normalize_appid("steam:990080"), "990080");
        assert_eq!(normalize_appid(" 990080 "), "990080");
        assert_eq!(normalize_appid("steam:"), "");
    }

    #[test]
    fn sums_hours_from_login_users_and_localconfig() {
        let root = std::env::temp_dir().join(format!(
            "arcadia-steam-root-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(
            root.join("config").join("loginusers.vdf"),
            r#""users"
{
    "76561198000000000"
    {
        "AccountName" "zes"
        "PersonaName" "Zyon"
        "RememberPassword" "1"
        "MostRecent" "1"
        "Timestamp" "1700000000"
    }
}
"#,
        )
        .unwrap();
        let account_id = arcadia_steam::account::steamid_to_account_id("76561198000000000");
        let userdata = root.join("userdata").join(account_id);
        std::fs::create_dir_all(userdata.join("config")).unwrap();
        std::fs::write(
            userdata.join("config").join("localconfig.vdf"),
            r#""UserLocalConfigStore"
{
    "Software"
    {
        "Valve"
        {
            "Steam"
            {
                "apps"
                {
                    "440" { "Playtime" "120" "LastPlayed" "1700000000" }
                    "570" { "Playtime" "45" }
                }
            }
        }
    }
}
"#,
        )
        .unwrap();

        let (hours, persona) = hours_from_root(&root).expect("horas");
        assert_eq!(persona, "Zyon");
        assert_eq!(hours.get("440"), Some(&120));
        assert_eq!(hours.get("570"), Some(&45));

        let data = root.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let status = status_from_root(&root, &data, None, true);
        assert_eq!(status["contaAtual"]["persona"], "Zyon");
        assert_eq!(status["vinculoOk"], true);
        assert_eq!(status["permitido"], true);
        assert!(data.join("steam_account.json").is_file());
        let _ = std::fs::remove_dir_all(PathBuf::from(root));
    }
}
