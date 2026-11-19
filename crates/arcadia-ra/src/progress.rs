//! Normalização do progresso RetroAchievements.
//!
//! Espelha `getGameProgress` de `app/electron/retroachievements/client.js` e
//! acrescenta os agregados que o renderer calculava por conta própria
//! (desbloqueadas/total, percentual) e as datas em ISO-8601 UTC.
//!
//! A resolução de console usa o mapa congelado de `arcadia-retro`
//! (`getRetroachievementsConsoleId`), sem duplicar a tabela.

use serde::Serialize;
use serde_json::Value;

use crate::client::{GameRef, RetroAchievementsClient, MEDIA_HOST};
use crate::error::{RaError, RaResult};
use crate::util::{
    js_field_count, js_field_string, js_number, js_number_to_i64, js_trim, js_truthy,
};

/// Acha o console RA de um `systemId` do Arcadia (ex.: `sony-playstation-2`).
///
/// Delega para [`arcadia_retro::get_retroachievements_console_id`], o mesmo
/// mapa de `getRetroachievementsConsoleId` do Electron.
pub fn console_id_for_system(system_id: &str) -> Option<u32> {
    arcadia_retro::get_retroachievements_console_id(system_id)
}

/// URL de badge, idêntica a `badgeUrl` do JS. Nome vazio devolve `""`.
pub fn badge_url(badge_name: &str, locked: bool) -> String {
    if badge_name.is_empty() {
        return String::new();
    }
    format!(
        "{MEDIA_HOST}/Badge/{badge_name}{}.png",
        if locked { "_lock" } else { "" }
    )
}

/// Percentual de conclusão arredondado a duas casas, limitado a 100%.
///
/// `total == 0` devolve `0.0` (evita `NaN`/`Infinity`).
pub fn completion_percentage(earned: u32, total: u32) -> f64 {
    if total == 0 {
        return 0.0;
    }
    let earned = earned.min(total);
    let percentage = (f64::from(earned) / f64::from(total)) * 100.0;
    (percentage * 100.0).round() / 100.0
}

/// Normaliza `DateEarned`/`DateEarnedHardcore` para ISO-8601 UTC.
///
/// Formatos aceitos: `YYYY-MM-DD HH:MM:SS` (formato da RA), com `T` no lugar
/// do espaço, fração de segundo opcional e sufixo `Z`/fuso opcional. Campos
/// zerados (`0000-00-00 00:00:00`), datas inválidas e texto vazio devolvem
/// `None`. Datas sem fuso são consideradas UTC (a API da RA usa UTC).
pub fn normalize_earned_date(value: &str) -> Option<String> {
    let text = js_trim(value);
    if text.is_empty() {
        return None;
    }
    let separator = text.find([' ', 'T'])?;
    let (date, remainder) = text.split_at(separator);
    let time_and_offset = remainder[1..].trim_start_matches(' ');
    if time_and_offset.is_empty() {
        return None;
    }

    let (date_year, date_month, date_day) = parse_date(date)?;
    let ParsedTime {
        hour,
        minute,
        second,
        fraction,
        offset,
    } = parse_time(time_and_offset)?;

    let mut out =
        format!("{date_year:04}-{date_month:02}-{date_day:02}T{hour:02}:{minute:02}:{second:02}");
    if let Some(fraction) = fraction {
        out.push('.');
        out.push_str(fraction);
    }
    out.push_str(offset.as_deref().unwrap_or("Z"));
    Some(out)
}

fn parse_date(date: &str) -> Option<(u32, u32, u32)> {
    let bytes = date.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year = parse_digits(&date[0..4])?;
    let month = parse_digits(&date[5..7])?;
    let day = parse_digits(&date[8..10])?;
    if year == 0 || !(1..=12).contains(&month) {
        return None;
    }
    let max_day = days_in_month(year, month);
    if day == 0 || day > max_day {
        return None;
    }
    Some((year, month, day))
}

struct ParsedTime<'a> {
    hour: u32,
    minute: u32,
    second: u32,
    fraction: Option<&'a str>,
    offset: Option<String>,
}

fn parse_time(time: &str) -> Option<ParsedTime<'_>> {
    let (clock, offset) = split_offset(time)?;
    let (clock, fraction) = match clock.split_once('.') {
        Some((clock, fraction))
            if !fraction.is_empty()
                && fraction.len() <= 9
                && fraction.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            (clock, Some(fraction))
        }
        Some(_) => return None,
        None => (clock, None),
    };
    let bytes = clock.as_bytes();
    if bytes.len() != 8 || bytes[2] != b':' || bytes[5] != b':' {
        return None;
    }
    let hour = parse_digits(&clock[0..2])?;
    let minute = parse_digits(&clock[3..5])?;
    let second = parse_digits(&clock[6..8])?;
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some(ParsedTime {
        hour,
        minute,
        second,
        fraction,
        offset,
    })
}

fn split_offset(time: &str) -> Option<(&str, Option<String>)> {
    if let Some(clock) = time.strip_suffix('Z').or_else(|| time.strip_suffix('z')) {
        return Some((clock, Some("Z".to_string())));
    }
    if let Some(index) = time.rfind(['+', '-']) {
        let (clock, offset) = time.split_at(index);
        let bytes = offset.as_bytes();
        if bytes.len() != 6 || bytes[3] != b':' {
            return None;
        }
        let hour = parse_digits(&offset[1..3])?;
        let minute = parse_digits(&offset[4..6])?;
        if hour > 23 || minute > 59 {
            return None;
        }
        return Some((clock, Some(offset.to_string())));
    }
    Some((time, None))
}

fn parse_digits(value: &str) -> Option<u32> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: u32) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

/// Jogo como a API pública devolve em `API_GetGameInfoAndUserProgress`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameInfo {
    pub id: i64,
    pub title: String,
    pub console_name: String,
    pub num_achievements: u32,
    pub num_awarded_to_user: u32,
    pub num_awarded_to_user_hardcore: u32,
}

/// Conquista normalizada.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Achievement {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub points: u32,
    pub badge_name: String,
    pub badge_url: String,
    pub badge_locked_url: String,
    pub unlocked: bool,
    pub unlocked_hardcore: bool,
    /// `DateEarned` em ISO-8601 UTC, ou `None` quando ausente/inválida.
    pub date_earned: Option<String>,
    /// `DateEarnedHardcore` em ISO-8601 UTC, ou `None`.
    pub date_earned_hardcore: Option<String>,
}

/// Progresso completo de um jogo para o usuário.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameProgress {
    pub game: GameInfo,
    pub achievements: Vec<Achievement>,
    /// Conquistas desbloqueadas na lista (mesma regra de `unlocked`).
    pub earned: u32,
    /// Total de conquistas na lista.
    pub total: u32,
    /// `earned / total`, arredondado a duas casas.
    pub completion_percentage: f64,
    /// Conquistas desbloqueadas em hardcore na lista.
    pub hardcore_earned: u32,
    /// `hardcoreEarned / total`, arredondado a duas casas.
    pub hardcore_completion_percentage: f64,
}

impl GameProgress {
    /// `true` quando a lista não está vazia e todas foram desbloqueadas.
    pub fn is_complete(&self) -> bool {
        self.total > 0 && self.earned >= self.total
    }
}

/// Normaliza o JSON de `API_GetGameInfoAndUserProgress.php`.
///
/// Campos ausentes seguem as coerções do JS (`Number(x || fallback)`); o
/// objeto `Achievements` pode vir como objeto indexado por id (formato real)
/// ou array. Entradas que não são objetos são ignoradas.
pub fn parse_game_progress(data: &Value) -> GameProgress {
    let achievements = parse_achievements(data.get("Achievements"));
    let total = u32::try_from(achievements.len()).unwrap_or(u32::MAX);
    let earned =
        u32::try_from(achievements.iter().filter(|item| item.unlocked).count()).unwrap_or(u32::MAX);
    let hardcore_earned = u32::try_from(
        achievements
            .iter()
            .filter(|item| item.unlocked_hardcore)
            .count(),
    )
    .unwrap_or(u32::MAX);

    let game = GameInfo {
        id: js_number_to_i64(js_number(data.get("ID").unwrap_or(&Value::Null))),
        title: js_field_string(data, "Title"),
        console_name: js_field_string(data, "ConsoleName"),
        num_achievements: js_field_count(data, "NumAchievements", achievements.len()),
        num_awarded_to_user: js_field_count(data, "NumAwardedToUser", 0),
        num_awarded_to_user_hardcore: js_field_count(data, "NumAwardedToUserHardcore", 0),
    };

    GameProgress {
        game,
        achievements,
        earned,
        total,
        completion_percentage: completion_percentage(earned, total),
        hardcore_earned,
        hardcore_completion_percentage: completion_percentage(hardcore_earned, total),
    }
}

fn parse_achievements(value: Option<&Value>) -> Vec<Achievement> {
    let values: Vec<&Value> = match value {
        Some(Value::Object(map)) => map.values().collect(),
        Some(Value::Array(items)) => items.iter().collect(),
        _ => Vec::new(),
    };
    values
        .into_iter()
        .filter_map(|item| {
            if !item.is_object() {
                return None;
            }
            let badge_name = js_field_string(item, "BadgeName");
            let date_earned = item
                .get("DateEarned")
                .and_then(Value::as_str)
                .and_then(normalize_earned_date);
            let date_earned_hardcore = item
                .get("DateEarnedHardcore")
                .and_then(Value::as_str)
                .and_then(normalize_earned_date);
            Some(Achievement {
                id: js_number_to_i64(js_number(item.get("ID").unwrap_or(&Value::Null))),
                title: js_field_string(item, "Title"),
                description: js_field_string(item, "Description"),
                points: js_field_count(item, "Points", 0),
                badge_url: badge_url(&badge_name, false),
                badge_locked_url: badge_url(&badge_name, true),
                badge_name,
                unlocked: item.get("DateEarned").map(js_truthy).unwrap_or(false),
                unlocked_hardcore: item
                    .get("DateEarnedHardcore")
                    .map(js_truthy)
                    .unwrap_or(false),
                date_earned,
                date_earned_hardcore,
            })
        })
        .collect()
}

/// Resolve o jogo por título dentro do console do `systemId`.
///
/// Devolve [`RaError::UnsupportedSystem`] quando o sistema não tem console RA
/// (ex.: PlayStation 3), igual ao handler `retroachievements:gameProgress`.
pub async fn find_game_for_system(
    client: &RetroAchievementsClient,
    username: &str,
    api_key: &str,
    title: &str,
    system_id: &str,
) -> RaResult<Option<GameRef>> {
    let console_id = console_id_for_system(system_id).ok_or(RaError::UnsupportedSystem)?;
    client
        .find_game_by_title(username, api_key, title, console_id)
        .await
}

/// Como [`find_game_for_system`], mas devolve só o id numérico positivo.
pub async fn resolve_game_id(
    client: &RetroAchievementsClient,
    username: &str,
    api_key: &str,
    title: &str,
    system_id: &str,
) -> RaResult<Option<u32>> {
    let game = find_game_for_system(client, username, api_key, title, system_id).await?;
    Ok(game.and_then(|game| {
        if game.id > 0 && game.id <= i64::from(u32::MAX) {
            Some(game.id as u32)
        } else {
            None
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn console_ids_vem_do_arcadia_retro() {
        assert_eq!(console_id_for_system("sony-playstation"), Some(12));
        assert_eq!(console_id_for_system("sony-playstation-2"), Some(21));
        assert_eq!(console_id_for_system("sony-psp"), Some(41));
        assert_eq!(console_id_for_system("nintendo-nes"), Some(7));
        assert_eq!(console_id_for_system("sony-playstation-3"), None);
        assert_eq!(console_id_for_system(""), None);
        // Alias não é id canônico: o mapa do JS também não resolve.
        assert_eq!(console_id_for_system("ps2"), None);
    }

    #[test]
    fn percentual_limita_e_arredonda() {
        assert_eq!(completion_percentage(0, 0), 0.0);
        assert_eq!(completion_percentage(0, 10), 0.0);
        assert_eq!(completion_percentage(10, 10), 100.0);
        assert_eq!(completion_percentage(1, 3), 33.33);
        assert_eq!(completion_percentage(2, 3), 66.67);
        assert_eq!(completion_percentage(15, 10), 100.0);
    }

    #[test]
    fn datas_normalizam_para_iso_utc() {
        assert_eq!(
            normalize_earned_date("2024-01-01 00:00:00").as_deref(),
            Some("2024-01-01T00:00:00Z")
        );
        assert_eq!(
            normalize_earned_date(" 2024-02-29T23:59:59 ").as_deref(),
            Some("2024-02-29T23:59:59Z")
        );
        assert_eq!(
            normalize_earned_date("2023-02-29 00:00:00").as_deref(),
            None,
            "2023 não é bissexto"
        );
        assert_eq!(
            normalize_earned_date("0000-00-00 00:00:00").as_deref(),
            None
        );
        assert_eq!(normalize_earned_date("").as_deref(), None);
        assert_eq!(normalize_earned_date("   ").as_deref(), None);
        assert_eq!(
            normalize_earned_date("2024-13-01 00:00:00").as_deref(),
            None
        );
        assert_eq!(
            normalize_earned_date("2024-01-01 24:00:00").as_deref(),
            None
        );
        assert_eq!(normalize_earned_date("2024-01-01").as_deref(), None);
        assert_eq!(
            normalize_earned_date("2024-01-01 00:00:00.500").as_deref(),
            Some("2024-01-01T00:00:00.500Z")
        );
        assert_eq!(
            normalize_earned_date("2024-01-01 00:00:00+02:30").as_deref(),
            Some("2024-01-01T00:00:00+02:30")
        );
    }

    #[test]
    fn parse_progresso_completo() {
        let data = json!({
            "ID": 21,
            "Title": "GTA: SA",
            "ConsoleName": "PlayStation 2",
            "NumAchievements": 2,
            "NumAwardedToUser": 1,
            "NumAwardedToUserHardcore": 0,
            "Achievements": {
                "9": {
                    "ID": 9,
                    "Title": "Conquista A",
                    "Description": "Desc A",
                    "Points": 10,
                    "BadgeName": "325108",
                    "DateEarned": "2024-01-01 00:00:00"
                },
                "10": {
                    "ID": 10,
                    "Title": "Conquista B",
                    "Description": "Desc B",
                    "Points": 5,
                    "BadgeName": "325109"
                }
            }
        });
        let progress = parse_game_progress(&data);
        assert_eq!(progress.game.id, 21);
        assert_eq!(progress.game.title, "GTA: SA");
        assert_eq!(progress.game.console_name, "PlayStation 2");
        assert_eq!(progress.game.num_achievements, 2);
        assert_eq!(progress.game.num_awarded_to_user, 1);
        assert_eq!(progress.earned, 1);
        assert_eq!(progress.total, 2);
        assert_eq!(progress.completion_percentage, 50.0);
        assert_eq!(progress.hardcore_earned, 0);
        assert!(!progress.is_complete());
        assert_eq!(progress.achievements.len(), 2);
        assert_eq!(
            progress.achievements[0].badge_url,
            "https://media.retroachievements.org/Badge/325108.png"
        );
        assert_eq!(
            progress.achievements[0].badge_locked_url,
            "https://media.retroachievements.org/Badge/325108_lock.png"
        );
        assert_eq!(
            progress.achievements[0].date_earned.as_deref(),
            Some("2024-01-01T00:00:00Z")
        );
        assert!(!progress.achievements[1].unlocked);
        assert_eq!(progress.achievements[1].date_earned, None);
    }

    #[test]
    fn fallback_de_contadores_igual_ao_js() {
        let progress = parse_game_progress(&json!({
            "ID": "12",
            "NumAchievements": 0,
            "Achievements": {
                "1": { "ID": 1, "Title": "A", "DateEarned": "0" },
                "2": { "ID": 2, "Title": "B" }
            }
        }));
        assert_eq!(progress.game.id, 12);
        assert_eq!(progress.game.num_achievements, 2);
        assert_eq!(progress.game.num_awarded_to_user, 0);
        // Boolean("0") é true no JS, mas a data não é válida.
        assert!(progress.achievements[0].unlocked);
        assert_eq!(progress.achievements[0].date_earned, None);
        assert_eq!(progress.total, 2);
    }

    #[test]
    fn achievements_ausentes_ou_invalidos_viram_lista_vazia() {
        let progress = parse_game_progress(&json!({ "Achievements": null }));
        assert!(progress.achievements.is_empty());
        assert_eq!(progress.total, 0);
        assert_eq!(progress.completion_percentage, 0.0);
        assert_eq!(progress.game.num_achievements, 0);

        let progress = parse_game_progress(&json!({ "Achievements": "nope" }));
        assert!(progress.achievements.is_empty());
    }

    #[test]
    fn achievements_em_array_tambem_funcionam() {
        let progress = parse_game_progress(&json!({
            "Achievements": [
                { "ID": 1, "Title": "A", "DateEarnedHardcore": "2024-05-04 03:02:01" },
                { "ID": 2, "Title": "B" }
            ]
        }));
        assert_eq!(progress.total, 2);
        assert_eq!(progress.hardcore_earned, 1);
        assert_eq!(progress.hardcore_completion_percentage, 50.0);
        assert!(progress.achievements[0].unlocked_hardcore);
    }

    #[test]
    fn jogo_com_todas_desbloqueadas_fica_completo() {
        let progress = parse_game_progress(&json!({
            "NumAchievements": 1,
            "Achievements": { "1": { "ID": 1, "DateEarned": "2024-01-01 00:00:00" } }
        }));
        assert!(progress.is_complete());
        assert_eq!(progress.completion_percentage, 100.0);
    }
}
