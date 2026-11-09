//! Normalização de progresso sobre os fixtures reais da API.

use arcadia_ra::{
    completion_percentage, console_id_for_system, parse_game_progress, Achievement, GameInfo,
};
use serde_json::{json, Value};

const GAME_PROGRESS: &str = include_str!("fixtures/game_progress.json");
const GAME_PROGRESS_EDGE: &str = include_str!("fixtures/game_progress_edge.json");

fn fixture(raw: &str) -> Value {
    serde_json::from_str(raw).expect("fixture JSON")
}

#[test]
fn fixture_completo_normaliza_contadores_percentual_e_datas() {
    let progress = parse_game_progress(&fixture(GAME_PROGRESS));
    assert_eq!(
        progress.game,
        GameInfo {
            id: 21,
            title: "GTA: SA".to_string(),
            console_name: "PlayStation 2".to_string(),
            num_achievements: 3,
            num_awarded_to_user: 2,
            num_awarded_to_user_hardcore: 1,
        }
    );
    assert_eq!(progress.earned, 2);
    assert_eq!(progress.total, 3);
    assert_eq!(progress.completion_percentage, 66.67);
    assert_eq!(progress.hardcore_earned, 1);
    assert_eq!(progress.hardcore_completion_percentage, 33.33);
    assert!(!progress.is_complete());

    assert_eq!(
        progress.achievements[0],
        Achievement {
            id: 9,
            title: "Conquista A".to_string(),
            description: "Desc A".to_string(),
            points: 10,
            badge_name: "325108".to_string(),
            badge_url: "https://media.retroachievements.org/Badge/325108.png".to_string(),
            badge_locked_url: "https://media.retroachievements.org/Badge/325108_lock.png"
                .to_string(),
            unlocked: true,
            unlocked_hardcore: true,
            date_earned: Some("2024-01-01T00:00:00Z".to_string()),
            date_earned_hardcore: Some("2024-01-02T03:04:05Z".to_string()),
        }
    );
    assert_eq!(
        progress.achievements[1].date_earned.as_deref(),
        Some("2024-02-29T23:59:59Z")
    );
    assert!(!progress.achievements[1].unlocked_hardcore);
    assert_eq!(progress.achievements[2].badge_url, "");
    assert_eq!(progress.achievements[2].badge_locked_url, "");
}

#[test]
fn fixture_edge_data_zerada_e_contador_com_fallback() {
    let progress = parse_game_progress(&fixture(GAME_PROGRESS_EDGE));
    assert_eq!(progress.game.id, 77);
    // NumAchievements 0 -> fallback `achievements.length`, como `||` no JS.
    assert_eq!(progress.game.num_achievements, 1);
    assert_eq!(progress.game.num_awarded_to_user, 0);
    assert_eq!(progress.total, 1);
    assert_eq!(progress.earned, 1, "Boolean('0000-00-00 00:00:00') é true");
    assert_eq!(progress.achievements[0].points, 15);
    assert_eq!(progress.achievements[0].date_earned, None);
    assert_eq!(
        progress.achievements[0].date_earned_hardcore.as_deref(),
        Some("2024-01-01T00:00:00Z")
    );
    assert_eq!(progress.completion_percentage, 100.0);
}

#[test]
fn percentuais_tem_bordas_seguras() {
    assert_eq!(completion_percentage(0, 0), 0.0);
    assert_eq!(completion_percentage(1, 0), 0.0);
    assert_eq!(completion_percentage(0, 1), 0.0);
    assert_eq!(completion_percentage(1, 1), 100.0);
    assert_eq!(completion_percentage(1, 6), 16.67);
    assert_eq!(completion_percentage(2, 6), 33.33);
    assert_eq!(completion_percentage(5, 6), 83.33);
    assert_eq!(completion_percentage(6, 6), 100.0);
    assert_eq!(completion_percentage(7, 6), 100.0);
}

#[test]
fn ids_de_console_seguem_o_mapa_do_arcadia_retro() {
    assert_eq!(console_id_for_system("sony-playstation"), Some(12));
    assert_eq!(console_id_for_system("sony-playstation-2"), Some(21));
    assert_eq!(console_id_for_system("sony-psp"), Some(41));
    assert_eq!(console_id_for_system("nintendo-gamecube"), Some(16));
    assert_eq!(console_id_for_system("sony-playstation-3"), None);
    assert_eq!(console_id_for_system("sistema-inexistente"), None);
}

#[test]
fn json_sem_campos_nao_quebra() {
    let progress = parse_game_progress(&json!({}));
    assert_eq!(progress.game.id, 0);
    assert_eq!(progress.game.title, "");
    assert_eq!(progress.total, 0);
    assert_eq!(progress.earned, 0);
    assert_eq!(progress.completion_percentage, 0.0);
    assert!(progress.achievements.is_empty());
    assert!(!progress.is_complete());
}
