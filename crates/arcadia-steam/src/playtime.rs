//! Horas jogadas: leitura de `localconfig.vdf` e regras do payload do servidor.
//!
//! Porte de `horasDoLocalConfig` (`app/electron/vdf.js`) e de
//! `app/electron/steam-horas-servidor.js`.
//!
//! Caminho real das horas:
//! `UserLocalConfigStore/Software/Valve/Steam/apps/<appid>/Playtime`.
//! O caminho curto `UserLocalConfigStore/apps` continua aceito, como no JS.
//!
//! Regra do servidor: por `(jogo, conta)` vale o MAIOR total visto; contas
//! diferentes somam. Assim reenviar não infla e um total menor lido agora não
//! apaga o que já subiu de outra máquina.

use std::cmp::max;
use std::collections::BTreeMap;
use std::sync::OnceLock;

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

use crate::js::{js_number_value, js_string_value, js_truthy_value};
use crate::vdf::{parse, VdfError, VdfObject, VdfValue};

/// Minutos por appid (chave é o número do appid, sem prefixo).
pub type PlaytimeMap = BTreeMap<String, u64>;
/// `{ appid: { steamid: minutos } }`.
pub type PlaytimeByAccount = BTreeMap<String, BTreeMap<String, u64>>;

/// Uma entrada de `apps` no `localconfig.vdf`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalPlaytime {
    pub appid: String,
    pub minutes: u64,
    /// `LastPlayed` em segundos, quando presente.
    pub last_played: Option<i64>,
}

/// Leitura de UMA conta Steam, no formato que o cliente chama de `{horas,
/// persona, steamid}`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlaytimeReading {
    pub horas: PlaytimeMap,
    pub persona: String,
    pub steamid: String,
}

impl PlaytimeReading {
    pub fn is_empty(&self) -> bool {
        self.horas.is_empty()
    }
}

/// Item enviado ao RPC `push_steam_playtime`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
pub struct SteamPlaytimeItem {
    pub appid: String,
    pub steamid: String,
    pub minutes: u64,
    pub persona: String,
}

/// Linha devolvida por `pull_steam_playtime`.
///
/// `minutes` aceita número ou string (o JS faz `Number(minutos) || 0`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(from = "ServerPlaytimeRaw")]
pub struct ServerPlaytime {
    pub appid: String,
    pub steamid: String,
    pub minutes: u64,
}

#[derive(Debug, Default, Deserialize)]
struct ServerPlaytimeRaw {
    #[serde(default)]
    appid: Value,
    #[serde(default)]
    steamid: Value,
    #[serde(default)]
    minutes: Value,
}

impl From<ServerPlaytimeRaw> for ServerPlaytime {
    fn from(raw: ServerPlaytimeRaw) -> Self {
        Self {
            appid: appid_key(&js_string_value(&raw.appid)),
            steamid: if js_truthy_value(&raw.steamid) {
                js_string_value(&raw.steamid)
            } else {
                String::new()
            },
            minutes: minutes_from_value(&raw.minutes),
        }
    }
}

fn minutes_from_value(value: &Value) -> u64 {
    match js_number_value(value) {
        Some(number) if number.is_finite() && number > 0.0 => {
            let rounded = number.round();
            if rounded <= 0.0 {
                0
            } else {
                rounded as u64
            }
        }
        _ => 0,
    }
}

/// `acharApps`: caminho completo e, por compatibilidade, o curto.
pub fn find_apps(root: &VdfObject) -> Option<&VdfObject> {
    let store = root.get_object("UserLocalConfigStore")?;
    if let Some(apps) = store.get_object("apps") {
        return Some(apps);
    }
    store
        .get_object("Software")?
        .get_object("Valve")?
        .get_object("Steam")?
        .get_object("apps")
}

fn is_appid(value: &str) -> bool {
    // `Number(appid) > 0` do JS: todos os dígitos e pelo menos um não-zero.
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.bytes().any(|byte| byte != b'0')
}

fn minutes_from_vdf(value: &VdfValue) -> Option<u64> {
    let number = value.js_number()?;
    if !number.is_finite() || number <= 0.0 {
        return None;
    }
    let rounded = number.round();
    if rounded <= 0.0 {
        return None;
    }
    Some(rounded as u64)
}

fn timestamp_from_vdf(value: &VdfValue) -> Option<i64> {
    let number = value.js_number()?;
    if !number.is_finite() {
        return None;
    }
    Some(number as i64)
}

/// Entradas válidas de `apps`, na ordem do arquivo.
pub fn playtime_from_root(root: &VdfObject) -> Vec<LocalPlaytime> {
    let Some(apps) = find_apps(root) else {
        return Vec::new();
    };
    apps.iter()
        .filter_map(|(appid, value)| {
            if !is_appid(appid) {
                return None;
            }
            let block = value.as_object()?;
            let minutes = minutes_from_vdf(block.get("Playtime")?)?;
            Some(LocalPlaytime {
                appid: appid.to_string(),
                minutes,
                last_played: block.get("LastPlayed").and_then(timestamp_from_vdf),
            })
        })
        .collect()
}

/// `horasDoLocalConfig`: minutos por appid, ignorando lixo, zero e appid 0.
pub fn playtime_map_from_root(root: &VdfObject) -> PlaytimeMap {
    playtime_from_root(root)
        .into_iter()
        .map(|entry| (entry.appid, entry.minutes))
        .collect()
}

pub fn parse_local_config(text: &str) -> Result<Vec<LocalPlaytime>, VdfError> {
    Ok(playtime_from_root(&parse(text)?))
}

pub fn playtime_map(text: &str) -> Result<PlaytimeMap, VdfError> {
    Ok(playtime_map_from_root(&parse(text)?))
}

pub fn reading_from_root(root: &VdfObject, steamid: &str, persona: &str) -> PlaytimeReading {
    PlaytimeReading {
        horas: playtime_map_from_root(root),
        persona: persona.to_string(),
        steamid: steamid.to_string(),
    }
}

pub fn parse_reading(
    text: &str,
    steamid: &str,
    persona: &str,
) -> Result<PlaytimeReading, VdfError> {
    Ok(reading_from_root(&parse(text)?, steamid, persona))
}

/// `chaveAppid`: primeiro número de 2+ dígitos dentro do id (`steam:990080` →
/// `990080`). Ids sem número devolvem `""`.
pub fn appid_key(value: &str) -> String {
    static APPID: OnceLock<Regex> = OnceLock::new();
    let regex = APPID.get_or_init(|| Regex::new("[0-9]{2,}").expect("regex de appid"));
    regex
        .find(value)
        .map(|found| found.as_str().to_string())
        .unwrap_or_default()
}

/// Minutos locais de uma conta no formato `{ appid: { steamid: minutos } }`.
pub fn locais_por_conta(locais: &PlaytimeMap, steamid: &str) -> PlaytimeByAccount {
    let mut output = PlaytimeByAccount::new();
    if steamid.is_empty() {
        return output;
    }
    for (appid, minutes) in locais {
        let key = appid_key(appid);
        if key.is_empty() || *minutes == 0 {
            continue;
        }
        output
            .entry(key)
            .or_default()
            .insert(steamid.to_string(), *minutes);
    }
    output
}

/// Junta leituras de várias contas locais no mesmo mapa por conta.
pub fn locais_de_contas(leituras: &[PlaytimeReading]) -> PlaytimeByAccount {
    let mut output = PlaytimeByAccount::new();
    for reading in leituras {
        for (appid, contas) in locais_por_conta(&reading.horas, &reading.steamid) {
            let target = output.entry(appid).or_default();
            for (steamid, minutes) in contas {
                target
                    .entry(steamid)
                    .and_modify(|current| *current = max(*current, minutes))
                    .or_insert(minutes);
            }
        }
    }
    output
}

fn guardar(output: &mut PlaytimeByAccount, appid: &str, steamid: &str, minutes: u64) {
    let key = appid_key(appid);
    if key.is_empty() || steamid.is_empty() || minutes == 0 {
        return;
    }
    output
        .entry(key)
        .or_default()
        .entry(steamid.to_string())
        .and_modify(|current| *current = max(*current, minutes))
        .or_insert(minutes);
}

/// Combina as linhas do servidor com as locais; por `(jogo, conta)` vence o
/// maior.
pub fn mesclar_horas(
    do_servidor: &[ServerPlaytime],
    locais: &PlaytimeByAccount,
) -> PlaytimeByAccount {
    let mut output = PlaytimeByAccount::new();
    for row in do_servidor {
        guardar(&mut output, &row.appid, &row.steamid, row.minutes);
    }
    for (appid, contas) in locais {
        for (steamid, minutes) in contas {
            guardar(&mut output, appid, steamid, *minutes);
        }
    }
    output
}

/// Soma as contas de cada jogo: `{ appid: minutos }` — o que a tela consome.
pub fn somar_por_appid(mapa: &PlaytimeByAccount) -> PlaytimeMap {
    let mut output = PlaytimeMap::new();
    for (appid, contas) in mapa {
        let total = contas
            .values()
            .fold(0u64, |total, minutes| total.saturating_add(*minutes));
        if total > 0 {
            output.insert(appid.clone(), total);
        }
    }
    output
}

/// Itens para `push_steam_playtime`: total absoluto por (jogo, conta).
pub fn itens_para_enviar(
    locais: &PlaytimeMap,
    steamid: &str,
    persona: &str,
) -> Vec<SteamPlaytimeItem> {
    if steamid.is_empty() {
        return Vec::new();
    }
    let mut items = Vec::new();
    for (appid, minutes) in locais {
        let key = appid_key(appid);
        if key.is_empty() || *minutes == 0 {
            continue;
        }
        items.push(SteamPlaytimeItem {
            appid: key,
            steamid: steamid.to_string(),
            minutes: *minutes,
            persona: persona.to_string(),
        });
    }
    items
}

/// Desserializa uma lista de linhas do servidor sem falhar por item inválido
/// (os inválidos viram minutos 0 e são ignorados no merge).
pub fn parse_server_rows(value: &Value) -> Vec<ServerPlaytime> {
    let Some(rows) = value.as_array() else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| serde_json::from_value::<ServerPlaytime>(row.clone()).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const LOCAL_CONFIG: &str = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"990080\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LastPlayed\"\t\t\"1750000000\"\n\t\t\t\t\t\t\"Playtime\"\t\t\"4907\"\n\t\t\t\t\t\t\"Playtime2wks\"\t\t\"0\"\n\t\t\t\t\t}\n\t\t\t\t\t\"1091500\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"Playtime\"\t\t\"120\"\n\t\t\t\t\t}\n\t\t\t\t\t\"0\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"Playtime\"\t\t\"999\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n";

    #[test]
    fn extracts_real_nested_playtime_and_last_played() {
        let entries = parse_local_config(LOCAL_CONFIG).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].appid, "990080");
        assert_eq!(entries[0].minutes, 4907);
        assert_eq!(entries[0].last_played, Some(1_750_000_000));
        assert_eq!(entries[1].appid, "1091500");
        assert_eq!(entries[1].minutes, 120);
        assert_eq!(entries[1].last_played, None);

        let map = playtime_map(LOCAL_CONFIG).unwrap();
        assert_eq!(map.get("990080"), Some(&4907));
        assert_eq!(map.get("1091500"), Some(&120));
        assert!(!map.contains_key("0"), "appid 0 não é jogo");
    }

    #[test]
    fn short_apps_path_is_accepted_for_compatibility() {
        let text = "\"UserLocalConfigStore\" { \"apps\" { \"220\" { \"Playtime\" \"60\" } } }";
        let map = playtime_map(text).unwrap();
        assert_eq!(map.get("220"), Some(&60));
    }

    #[test]
    fn appid_zero_letters_and_non_object_entries_are_ignored() {
        let text = "\"UserLocalConfigStore\" { \"apps\" { \"0\" { \"Playtime\" \"1\" } \"00\" { \"Playtime\" \"1\" } \"abc\" { \"Playtime\" \"1\" } \"220\" \"texto\" \"730\" { \"Playtime\" \"0\" } } }";
        assert!(playtime_map(text).unwrap().is_empty());
    }

    #[test]
    fn missing_zero_negative_and_non_numeric_playtime_are_ignored() {
        let text = "\"UserLocalConfigStore\" { \"apps\" { \"1\" { } \"2\" { \"Playtime\" \"0\" } \"3\" { \"Playtime\" \"-10\" } \"4\" { \"Playtime\" \"abc\" } \"5\" { \"Playtime\" \"\" } \"6\" { \"Playtime\" { \"x\" \"1\" } } } }";
        assert!(playtime_map(text).unwrap().is_empty());
    }

    #[test]
    fn playtime_uses_javascript_number_coercion() {
        let text = "\"UserLocalConfigStore\" { \"apps\" { \"1\" { \"Playtime\" \" 42 \" } \"2\" { \"Playtime\" \"+7\" } \"3\" { \"Playtime\" \"0x10\" } \"4\" { \"Playtime\" \"1e3\" } \"5\" { \"Playtime\" \"Infinity\" } \"6\" { \"Playtime\" \"0.4\" } \"7\" { \"Playtime\" \"4907.6\" } } }";
        let map = playtime_map(text).unwrap();
        assert_eq!(map.get("1"), Some(&42));
        assert_eq!(map.get("2"), Some(&7));
        assert_eq!(map.get("3"), Some(&16));
        assert_eq!(map.get("4"), Some(&1000));
        assert!(!map.contains_key("5"));
        // Minutos inteiros: 0.4 arredonda para 0 e fica fora (contrato do
        // servidor só aceita > 0); 4907.6 arredonda para 4908.
        assert!(!map.contains_key("6"));
        assert_eq!(map.get("7"), Some(&4908));
    }

    #[test]
    fn duplicate_playtime_follows_number_of_the_joined_list() {
        let one = "\"UserLocalConfigStore\" { \"apps\" { \"220\" { \"Playtime\" \"60\" \"Playtime\" \"60\" } } }";
        // ["60","60"] -> Number("60,60") === NaN; a chave fica de fora.
        assert!(playtime_map(one).unwrap().is_empty());

        let single = "\"UserLocalConfigStore\" { \"apps\" { \"220\" { \"Playtime\" \"60\" } } }";
        assert_eq!(playtime_map(single).unwrap().get("220"), Some(&60));
    }

    #[test]
    fn reading_keeps_identity_and_minutes() {
        let reading = parse_reading(LOCAL_CONFIG, "76561198000004541", "Kk").unwrap();
        assert_eq!(reading.steamid, "76561198000004541");
        assert_eq!(reading.persona, "Kk");
        assert_eq!(reading.horas.get("990080"), Some(&4907));
        assert!(!reading.is_empty());
    }

    #[test]
    fn appid_key_normalizes_library_and_store_ids() {
        assert_eq!(appid_key("steam:990080"), "990080");
        assert_eq!(appid_key("990080"), "990080");
        assert_eq!(appid_key("epic:abc"), "");
        assert_eq!(appid_key(""), "");
        assert_eq!(appid_key("a1b22c"), "22");
    }

    #[test]
    fn different_accounts_sum_but_same_account_never_double_counts() {
        let leituras = vec![
            PlaytimeReading {
                steamid: "76561197960265728".into(),
                persona: "NEGO EJACULADO".into(),
                horas: BTreeMap::from([
                    ("steam:990080".to_string(), 4907),
                    ("steam:220".to_string(), 120),
                ]),
            },
            PlaytimeReading {
                steamid: "76561198000004541".into(),
                persona: "Kk".into(),
                horas: BTreeMap::from([
                    ("steam:990080".to_string(), 1800),
                    ("steam:730".to_string(), 5000),
                ]),
            },
        ];
        let somado = somar_por_appid(&mesclar_horas(&[], &locais_de_contas(&leituras)));
        assert_eq!(somado.get("990080"), Some(&6707));
        assert_eq!(somado.get("220"), Some(&120));
        assert_eq!(somado.get("730"), Some(&5000));

        let locais = locais_por_conta(
            &BTreeMap::from([("steam:990080".to_string(), 4907)]),
            "76561198000004541",
        );
        let mapa = mesclar_horas(
            &[ServerPlaytime {
                appid: "990080".into(),
                steamid: "76561198000004541".into(),
                minutes: 5000,
            }],
            &locais,
        );
        assert_eq!(somar_por_appid(&mapa).get("990080"), Some(&5000));
    }

    #[test]
    fn smaller_local_total_does_not_erase_what_already_uploaded() {
        let locais = locais_por_conta(
            &BTreeMap::from([("steam:990080".to_string(), 100)]),
            "76561198000004541",
        );
        let mapa = mesclar_horas(
            &[ServerPlaytime {
                appid: "990080".into(),
                steamid: "76561198000004541".into(),
                minutes: 4907,
            }],
            &locais,
        );
        assert_eq!(somar_por_appid(&mapa).get("990080"), Some(&4907));
    }

    #[test]
    fn zero_negative_and_garbage_rows_create_nothing() {
        let mapa = mesclar_horas(
            &[
                ServerPlaytime {
                    appid: "990080".into(),
                    steamid: "76561198000004541".into(),
                    minutes: 0,
                },
                ServerPlaytime {
                    appid: "990080".into(),
                    steamid: "76561198000004542".into(),
                    minutes: 0, // linha negativa vira 0 na desserialização
                },
                ServerPlaytime {
                    appid: "990080".into(),
                    steamid: String::new(),
                    minutes: 60,
                },
                ServerPlaytime {
                    appid: "lixo".into(),
                    steamid: "76561198000004541".into(),
                    minutes: 60,
                },
            ],
            &PlaytimeByAccount::new(),
        );
        assert!(mapa.is_empty());
    }

    #[test]
    fn server_rows_accept_number_and_numeric_string_minutes() {
        let rows = parse_server_rows(&json!([
            { "appid": "990080", "steamid": "76561198000004541", "minutes": 4907 },
            { "appid": "steam:220", "steamid": "76561198000004542", "minutes": "120" },
            { "appid": "730", "steamid": "76561198000004541", "minutes": -10 },
            { "appid": "400", "steamid": "76561198000004541", "minutes": "ruim" },
            { "appid": null, "steamid": null, "minutes": null },
        ]));
        assert_eq!(rows[0].minutes, 4907);
        assert_eq!(rows[1].appid, "220");
        assert_eq!(rows[1].minutes, 120);
        assert_eq!(rows[2].minutes, 0);
        assert_eq!(rows[3].minutes, 0);
        assert_eq!(rows[4].appid, "");
    }

    #[test]
    fn upload_items_are_absolute_totals_with_the_numeric_key() {
        let items = itens_para_enviar(
            &BTreeMap::from([
                ("steam:990080".to_string(), 4907),
                ("steam:400".to_string(), 0),
            ]),
            "76561198000004541",
            "Kk",
        );
        assert_eq!(
            items,
            vec![SteamPlaytimeItem {
                appid: "990080".into(),
                steamid: "76561198000004541".into(),
                minutes: 4907,
                persona: "Kk".into(),
            }]
        );
        assert!(
            itens_para_enviar(&BTreeMap::from([("steam:990080".to_string(), 10)]), "", "")
                .is_empty()
        );

        let json = serde_json::to_value(&items[0]).unwrap();
        assert_eq!(
            json,
            json!({
                "appid": "990080",
                "steamid": "76561198000004541",
                "minutes": 4907,
                "persona": "Kk"
            })
        );
    }

    #[test]
    fn final_sum_matches_the_server_rule() {
        let locais = locais_por_conta(
            &BTreeMap::from([
                ("steam:990080".to_string(), 4907),
                ("steam:220".to_string(), 120),
            ]),
            "76561198000004541",
        );
        let do_servidor = vec![
            ServerPlaytime {
                appid: "990080".into(),
                steamid: "76561198000004541".into(),
                minutes: 4907,
            },
            ServerPlaytime {
                appid: "990080".into(),
                steamid: "76561198000004542".into(),
                minutes: 1800,
            },
            ServerPlaytime {
                appid: "990080".into(),
                steamid: "76561198000009999".into(),
                minutes: 60,
            },
        ];
        let somado = somar_por_appid(&mesclar_horas(&do_servidor, &locais));
        assert_eq!(somado.get("990080"), Some(&(4907 + 1800 + 60)));
        assert_eq!(somado.get("220"), Some(&120));
    }
}
