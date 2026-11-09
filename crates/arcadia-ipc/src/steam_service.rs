//! Serviço Steam do IPC: raiz da instalação, horas locais e news por appid.

use std::collections::BTreeMap;
use std::path::Path;

use arcadia_launch::clock::SystemClock;
use arcadia_launch::platform::Platform;
use arcadia_launch::steam_path::{RegistryProbe, SteamPathResolver, SystemPathFs};
#[cfg(not(windows))]
use arcadia_launch::steam_path::NullRegistryProbe;
#[cfg(windows)]
use arcadia_launch::steam_path::WindowsRegistryProbe;
use arcadia_steam::account::{local_config_path, parse_login_users, select_active_login_user};
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
    let login_path = Path::new(&root).join("config").join("loginusers.vdf");
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
        let Some(path) = local_config_path(Path::new(&root), &account.steamid) else {
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
    Ok((totals.into_iter().collect(), persona))
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
    use super::normalize_appid;

    #[test]
    fn appid_prefix_is_removed() {
        assert_eq!(normalize_appid("steam:990080"), "990080");
        assert_eq!(normalize_appid(" 990080 "), "990080");
        assert_eq!(normalize_appid("steam:"), "");
    }
}
