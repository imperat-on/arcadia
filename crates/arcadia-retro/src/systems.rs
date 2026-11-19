//! Registro canônico de plataformas retrô.
//!
//! Port 1:1 de `app/electron/retro-systems.js` (oráculo congelado). A ordem de
//! `SYSTEMS` é a ordem de `listSystems()` no JavaScript. `serialPattern` é
//! mantido como string (o JS guarda um `RegExp`) e compilado sob demanda.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

/// Definição de uma plataforma, com os mesmos nomes de campo do JS.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct System {
    pub id: &'static str,
    pub display_name: &'static str,
    pub libretro_database: &'static str,
    pub thumbnail_collection: &'static str,
    pub emulator_ids: &'static [&'static str],
    pub aliases: &'static [&'static str],
    pub media_type: &'static str,
    pub identity_strategy: &'static [&'static str],
    pub serial_pattern: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retroachievements_console_id: Option<u32>,
}

macro_rules! system {
    (
        $id:literal,
        $display:literal,
        $libretro:literal,
        $thumb:literal,
        [$($emulator:literal),* $(,)?],
        [$($alias:literal),* $(,)?],
        $media:literal,
        [$($strategy:literal),* $(,)?],
        $serial:expr,
        $ra:expr
    ) => {
        System {
            id: $id,
            display_name: $display,
            libretro_database: $libretro,
            thumbnail_collection: $thumb,
            emulator_ids: &[$($emulator),*],
            aliases: &[$($alias),*],
            media_type: $media,
            identity_strategy: &[$($strategy),*],
            serial_pattern: $serial,
            retroachievements_console_id: $ra,
        }
    };
}

const PLAYSTATION_SERIAL: &str = r"(?i)(?-u:\b)([A-Z]{4}[-_]?[0-9]{5})(?-u:\b)";
const PLAYSTATION_3_SERIAL: &str = r"(?i)(?-u:\b)([A-Z]{4}[0-9]{5})(?-u:\b)";
const GAMECUBE_SERIAL: &str = r"(?i)(?-u:\b)(G[A-Z0-9]{3}[0-9]{2})(?-u:\b)";
const WII_SERIAL: &str = r"(?i)(?-u:\b)(R[A-Z0-9]{3}[0-9]{2})(?-u:\b)";

/// Todas as plataformas, na mesma ordem do objeto `SYSTEMS` do JS.
pub static SYSTEMS: &[System] = &[
    system!(
        "sony-playstation",
        "PlayStation",
        "Sony - PlayStation",
        "Sony - PlayStation",
        ["duckstation"],
        [
            "ps1",
            "psx",
            "playstation",
            "playstation 1",
            "sony playstation"
        ],
        "disc",
        ["serial", "sha1", "title"],
        Some(PLAYSTATION_SERIAL),
        Some(12)
    ),
    system!(
        "sony-playstation-2",
        "PlayStation 2",
        "Sony - PlayStation 2",
        "Sony - PlayStation 2",
        ["pcsx2"],
        ["ps2", "playstation 2", "sony ps2"],
        "disc",
        ["serial", "sha1", "title"],
        Some(PLAYSTATION_SERIAL),
        Some(21)
    ),
    system!(
        "sony-playstation-3",
        "PlayStation 3",
        "Sony - PlayStation 3",
        "Sony - PlayStation 3",
        ["rpcs3"],
        ["ps3", "playstation 3", "sony ps3"],
        "mixed",
        ["serial", "title"],
        Some(PLAYSTATION_3_SERIAL),
        None
    ),
    system!(
        "sony-psp",
        "PlayStation Portable",
        "Sony - PlayStation Portable",
        "Sony - PlayStation Portable",
        ["ppsspp"],
        ["psp", "playstation portable", "sony psp"],
        "mixed",
        ["serial", "sha1", "title"],
        Some(PLAYSTATION_SERIAL),
        Some(41)
    ),
    system!(
        "nintendo-gamecube",
        "GameCube",
        "Nintendo - GameCube",
        "Nintendo - GameCube",
        ["dolphin"],
        ["gc", "gcn", "gamecube", "nintendo gamecube"],
        "disc",
        ["serial", "sha1", "title"],
        Some(GAMECUBE_SERIAL),
        Some(16)
    ),
    system!(
        "nintendo-wii",
        "Wii",
        "Nintendo - Wii",
        "Nintendo - Wii",
        ["dolphin"],
        ["wii", "nintendo wii"],
        "disc",
        ["serial", "sha1", "title"],
        Some(WII_SERIAL),
        Some(19)
    ),
    system!(
        "nintendo-ds",
        "Nintendo DS",
        "Nintendo - Nintendo DS",
        "Nintendo - Nintendo DS",
        ["melonds", "desmume"],
        ["nds", "ds", "nintendo ds"],
        "cartridge",
        ["crc32", "sha1", "title"],
        None,
        Some(18)
    ),
    system!(
        "nintendo-dsi",
        "Nintendo DSi",
        "Nintendo - Nintendo DSi",
        "Nintendo - Nintendo DSi",
        ["melonds", "desmume"],
        ["dsi", "nintendo dsi"],
        "digital",
        ["crc32", "sha1", "title"],
        None,
        Some(78)
    ),
    system!(
        "nintendo-nes",
        "NES",
        "Nintendo - Nintendo Entertainment System",
        "Nintendo - Nintendo Entertainment System",
        ["retroarch"],
        ["nes", "famicom", "nintendo entertainment system"],
        "cartridge",
        ["crc32", "sha1", "title"],
        None,
        Some(7)
    ),
    system!(
        "nintendo-snes",
        "SNES",
        "Nintendo - Super Nintendo Entertainment System",
        "Nintendo - Super Nintendo Entertainment System",
        ["retroarch"],
        ["snes", "sfc", "super famicom", "super nintendo"],
        "cartridge",
        ["crc32", "sha1", "title"],
        None,
        Some(3)
    ),
    system!(
        "nintendo-game-boy",
        "Game Boy",
        "Nintendo - Game Boy",
        "Nintendo - Game Boy",
        ["retroarch"],
        ["gb", "game boy", "gameboy"],
        "cartridge",
        ["crc32", "sha1", "title"],
        None,
        Some(4)
    ),
    system!(
        "nintendo-game-boy-color",
        "Game Boy Color",
        "Nintendo - Game Boy Color",
        "Nintendo - Game Boy Color",
        ["retroarch"],
        ["gbc", "game boy color", "gameboy color"],
        "cartridge",
        ["crc32", "sha1", "title"],
        None,
        Some(6)
    ),
    system!(
        "nintendo-game-boy-advance",
        "Game Boy Advance",
        "Nintendo - Game Boy Advance",
        "Nintendo - Game Boy Advance",
        ["retroarch"],
        ["gba", "game boy advance", "gameboy advance"],
        "cartridge",
        ["crc32", "sha1", "title"],
        None,
        Some(5)
    ),
    system!(
        "nintendo-64",
        "Nintendo 64",
        "Nintendo - Nintendo 64",
        "Nintendo - Nintendo 64",
        ["retroarch"],
        ["n64", "nintendo 64"],
        "cartridge",
        ["crc32", "sha1", "title"],
        None,
        Some(2)
    ),
];

static SYSTEM_SERIAL_REGEXES: LazyLock<Vec<Option<Regex>>> = LazyLock::new(|| {
    SYSTEMS
        .iter()
        .map(|system| {
            system
                .serial_pattern
                .map(|pattern| Regex::new(pattern).expect("serial pattern válido"))
        })
        .collect()
});

fn system_index(system_id: &str) -> Option<usize> {
    SYSTEMS.iter().position(|system| system.id == system_id)
}

/// Resolve um alias de plataforma para o `systemId` canônico.
///
/// Espelha `resolveSystem`: compara em minúsculas, com `trim`, e devolve o
/// primeiro alias registrado (a busca percorre os sistemas na ordem do JS).
pub fn resolve_system(alias: &str) -> Option<&'static str> {
    if alias.is_empty() {
        return None;
    }
    let normalized = alias.to_lowercase();
    let normalized = normalized.trim();
    if normalized.is_empty() {
        return None;
    }
    SYSTEMS
        .iter()
        .find_map(|system| system.aliases.contains(&normalized).then_some(system.id))
}

/// Retorna a definição completa de um sistema pelo ID canônico (busca exata).
pub fn get_system(system_id: &str) -> Option<&'static System> {
    system_index(system_id).map(|index| &SYSTEMS[index])
}

/// Lista todos os sistemas registrados, na ordem de declaração.
pub fn list_systems() -> &'static [System] {
    SYSTEMS
}

/// Extrai o serial de um texto usando o pattern do sistema.
///
/// O retorno remove `-`, `_` e ` ` e converte para maiúsculas, como
/// `extractSerial`. Sistemas sem `serialPattern` devolvem `None`.
pub fn extract_serial(text: &str, system_id: &str) -> Option<String> {
    let index = system_index(system_id)?;
    let regex = SYSTEM_SERIAL_REGEXES[index].as_ref()?;
    let captures = regex.captures(text)?;
    let matched = captures.get(1)?.as_str();
    Some(matched.replace(['-', '_', ' '], "").to_uppercase())
}

/// Normaliza um serial para comparação: remove `-`, `_`, espaços e sobe a caixa.
pub fn normalize_serial(serial: &str) -> String {
    serial.replace(['-', '_', ' '], "").to_uppercase()
}

/// Informa se o ID é um sistema built-in (não pode ser sobrescrito por plugin).
pub fn is_builtin_system(system_id: &str) -> bool {
    system_index(system_id).is_some()
}

/// ID do console na RetroAchievements para um `systemId`, quando suportado.
pub fn get_retroachievements_console_id(system_id: &str) -> Option<u32> {
    get_system(system_id).and_then(|system| system.retroachievements_console_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_system_reconhece_aliases_playstation() {
        assert_eq!(resolve_system("ps1"), Some("sony-playstation"));
        assert_eq!(resolve_system("psx"), Some("sony-playstation"));
        assert_eq!(resolve_system("playstation"), Some("sony-playstation"));
        assert_eq!(resolve_system("PS1"), Some("sony-playstation"));
        assert_eq!(resolve_system("  psx  "), Some("sony-playstation"));

        assert_eq!(resolve_system("ps2"), Some("sony-playstation-2"));
        assert_eq!(resolve_system("playstation 2"), Some("sony-playstation-2"));

        assert_eq!(resolve_system("ps3"), Some("sony-playstation-3"));
        assert_eq!(resolve_system("playstation 3"), Some("sony-playstation-3"));

        assert_eq!(resolve_system("psp"), Some("sony-psp"));
        assert_eq!(resolve_system("playstation portable"), Some("sony-psp"));
    }

    #[test]
    fn resolve_system_reconhece_aliases_nintendo() {
        assert_eq!(resolve_system("nes"), Some("nintendo-nes"));
        assert_eq!(resolve_system("famicom"), Some("nintendo-nes"));

        assert_eq!(resolve_system("snes"), Some("nintendo-snes"));
        assert_eq!(resolve_system("sfc"), Some("nintendo-snes"));
        assert_eq!(resolve_system("super famicom"), Some("nintendo-snes"));

        assert_eq!(resolve_system("n64"), Some("nintendo-64"));
        assert_eq!(resolve_system("nintendo 64"), Some("nintendo-64"));

        assert_eq!(resolve_system("gb"), Some("nintendo-game-boy"));
        assert_eq!(resolve_system("gbc"), Some("nintendo-game-boy-color"));
        assert_eq!(resolve_system("gba"), Some("nintendo-game-boy-advance"));

        assert_eq!(resolve_system("gc"), Some("nintendo-gamecube"));
        assert_eq!(resolve_system("gcn"), Some("nintendo-gamecube"));
        assert_eq!(resolve_system("gamecube"), Some("nintendo-gamecube"));

        assert_eq!(resolve_system("wii"), Some("nintendo-wii"));

        assert_eq!(resolve_system("nds"), Some("nintendo-ds"));
        assert_eq!(resolve_system("ds"), Some("nintendo-ds"));
        assert_eq!(resolve_system("dsi"), Some("nintendo-dsi"));
    }

    #[test]
    fn resolve_system_devolve_none_para_desconhecidos() {
        assert_eq!(resolve_system("unknown"), None);
        assert_eq!(resolve_system("xbox"), None);
        assert_eq!(resolve_system(""), None);
        assert_eq!(resolve_system("   "), None);
    }

    #[test]
    fn get_system_devolve_definicao_completa() {
        let ps1 = get_system("sony-playstation").expect("PS1 registrado");
        assert_eq!(ps1.id, "sony-playstation");
        assert_eq!(ps1.display_name, "PlayStation");
        assert_eq!(ps1.libretro_database, "Sony - PlayStation");
        assert_eq!(ps1.thumbnail_collection, "Sony - PlayStation");
        assert_eq!(ps1.emulator_ids, &["duckstation"]);
        assert!(ps1.aliases.contains(&"ps1"));
        assert_eq!(ps1.media_type, "disc");
        assert_eq!(ps1.identity_strategy, &["serial", "sha1", "title"]);
        assert!(ps1.serial_pattern.is_some());
    }

    #[test]
    fn get_system_devolve_none_para_desconhecido() {
        assert!(get_system("unknown-system").is_none());
        assert!(get_system("").is_none());
    }

    #[test]
    fn list_systems_devolve_todos() {
        let systems = list_systems();
        assert!(systems.len() >= 14);

        let ids: Vec<&str> = systems.iter().map(|system| system.id).collect();
        assert!(ids.contains(&"sony-playstation"));
        assert!(ids.contains(&"nintendo-64"));
        assert!(ids.contains(&"nintendo-gamecube"));
    }

    #[test]
    fn extract_serial_encontra_serials_playstation() {
        assert_eq!(
            extract_serial("SLUS-20312", "sony-playstation").as_deref(),
            Some("SLUS20312")
        );
        assert_eq!(
            extract_serial("SCUS_94900", "sony-playstation").as_deref(),
            Some("SCUS94900")
        );
        assert_eq!(
            extract_serial("Game Title [SLUS-20312] [USA]", "sony-playstation").as_deref(),
            Some("SLUS20312")
        );
        assert_eq!(
            extract_serial("SLES-12345 Region", "sony-playstation").as_deref(),
            Some("SLES12345")
        );
    }

    #[test]
    fn extract_serial_encontra_serials_ps2() {
        assert_eq!(
            extract_serial("SLUS-20312", "sony-playstation-2").as_deref(),
            Some("SLUS20312")
        );
        assert_eq!(
            extract_serial("SCUS_94900", "sony-playstation-2").as_deref(),
            Some("SCUS94900")
        );
    }

    #[test]
    fn extract_serial_encontra_serials_ps3() {
        assert_eq!(
            extract_serial("BLUS30100", "sony-playstation-3").as_deref(),
            Some("BLUS30100")
        );
        assert_eq!(
            extract_serial("NPUB30001", "sony-playstation-3").as_deref(),
            Some("NPUB30001")
        );
        assert_eq!(
            extract_serial("Game [BLUS30100] Region", "sony-playstation-3").as_deref(),
            Some("BLUS30100")
        );
    }

    #[test]
    fn extract_serial_encontra_serials_gamecube() {
        assert_eq!(
            extract_serial("GALE01", "nintendo-gamecube").as_deref(),
            Some("GALE01")
        );
        assert_eq!(
            extract_serial("Game (GALE01)", "nintendo-gamecube").as_deref(),
            Some("GALE01")
        );
    }

    #[test]
    fn extract_serial_encontra_serials_wii() {
        assert_eq!(
            extract_serial("RMCE01", "nintendo-wii").as_deref(),
            Some("RMCE01")
        );
        assert_eq!(
            extract_serial("Game [RMCE01]", "nintendo-wii").as_deref(),
            Some("RMCE01")
        );
    }

    #[test]
    fn extract_serial_devolve_none_quando_nao_acha() {
        assert_eq!(extract_serial("No Serial Here", "sony-playstation"), None);
        assert_eq!(extract_serial("", "sony-playstation"), None);
        assert_eq!(extract_serial("Game Title", "nintendo-nes"), None);
    }

    #[test]
    fn extract_serial_devolve_none_para_sistemas_sem_pattern() {
        assert_eq!(extract_serial("Some Text", "nintendo-nes"), None);
        assert_eq!(extract_serial("SLUS-20312", "nintendo-64"), None);
        assert_eq!(extract_serial("SLUS-20312", "unknown-system"), None);
    }

    #[test]
    fn normalize_serial_remove_separadores_e_sobe_caixa() {
        assert_eq!(normalize_serial("SLUS-20312"), "SLUS20312");
        assert_eq!(normalize_serial("SCUS_94900"), "SCUS94900");
        assert_eq!(normalize_serial("slus 20312"), "SLUS20312");
        assert_eq!(normalize_serial("slus-20312"), "SLUS20312");
        assert_eq!(normalize_serial(""), "");
    }

    #[test]
    fn is_builtin_system_identifica_sistemas_builtin() {
        assert!(is_builtin_system("sony-playstation"));
        assert!(is_builtin_system("nintendo-64"));
        assert!(is_builtin_system("nintendo-gamecube"));
        assert!(!is_builtin_system("unknown-system"));
        assert!(!is_builtin_system("custom-plugin-system"));
        assert!(!is_builtin_system(""));
    }

    #[test]
    fn resolucao_de_alias_ignora_caixa() {
        assert_eq!(resolve_system("PS1"), Some("sony-playstation"));
        assert_eq!(resolve_system("Ps1"), Some("sony-playstation"));
        assert_eq!(resolve_system("pS1"), Some("sony-playstation"));
        assert_eq!(resolve_system("SNES"), Some("nintendo-snes"));
        assert_eq!(resolve_system("N64"), Some("nintendo-64"));
    }

    #[test]
    fn resolucao_de_alias_ignora_espacos() {
        assert_eq!(resolve_system("  ps1  "), Some("sony-playstation"));
        assert_eq!(resolve_system("\tgba\t"), Some("nintendo-game-boy-advance"));
        assert_eq!(
            resolve_system("  playstation 2  "),
            Some("sony-playstation-2")
        );
    }

    #[test]
    fn todos_os_sistemas_tem_campos_obrigatorios() {
        for system in list_systems() {
            assert!(!system.id.is_empty());
            assert!(!system.display_name.is_empty(), "{}", system.id);
            assert!(!system.libretro_database.is_empty(), "{}", system.id);
            assert!(!system.thumbnail_collection.is_empty(), "{}", system.id);
            assert!(!system.emulator_ids.is_empty(), "{}", system.id);
            assert!(!system.aliases.is_empty(), "{}", system.id);
            assert!(!system.media_type.is_empty(), "{}", system.id);
            assert!(!system.identity_strategy.is_empty(), "{}", system.id);
        }
    }

    #[test]
    fn nao_ha_aliases_duplicados_entre_sistemas() {
        let mut seen: Vec<(String, &str)> = Vec::new();
        for system in list_systems() {
            for alias in system.aliases {
                let normalized = alias.to_lowercase();
                let normalized = normalized.trim().to_string();
                if let Some((_, other)) = seen.iter().find(|(value, _)| *value == normalized) {
                    panic!("alias duplicado {alias:?} em {other} e {}", system.id);
                }
                seen.push((normalized, system.id));
            }
        }
    }

    #[test]
    fn strategias_de_identidade_sao_validas() {
        let valid = ["serial", "sha1", "md5", "crc32", "title"];
        for system in list_systems() {
            for strategy in system.identity_strategy {
                assert!(
                    valid.contains(strategy),
                    "sistema {} tem estratégia inválida: {}",
                    system.id,
                    strategy
                );
            }
        }
    }

    #[test]
    fn tipos_de_midia_sao_validos() {
        let valid = ["cartridge", "disc", "digital", "mixed"];
        for system in list_systems() {
            assert!(
                valid.contains(&system.media_type),
                "sistema {} tem mediaType inválido: {}",
                system.id,
                system.media_type
            );
        }
    }

    #[test]
    fn patterns_de_serial_capturam_corretamente() {
        let cases = [
            ("sony-playstation", "SLUS-20312", "SLUS20312"),
            ("sony-playstation", "Game [SCUS_94900]", "SCUS94900"),
            ("sony-playstation-3", "BLUS30100", "BLUS30100"),
            ("nintendo-gamecube", "GALE01", "GALE01"),
            ("nintendo-wii", "RMCE01", "RMCE01"),
        ];
        for (system_id, text, expected) in cases {
            assert_eq!(
                extract_serial(text, system_id).as_deref(),
                Some(expected),
                "falhou para {text:?} em {system_id}"
            );
        }
    }

    #[test]
    fn definicao_serializa_com_os_nomes_do_cliente() {
        let system = get_system("sony-playstation").expect("PS1");
        let value = serde_json::to_value(system).expect("json");
        assert_eq!(value["id"], "sony-playstation");
        assert_eq!(value["displayName"], "PlayStation");
        assert_eq!(value["libretroDatabase"], "Sony - PlayStation");
        assert_eq!(value["thumbnailCollection"], "Sony - PlayStation");
        assert_eq!(value["emulatorIds"], serde_json::json!(["duckstation"]));
        assert_eq!(value["mediaType"], "disc");
        assert_eq!(value["retroachievementsConsoleId"], 12);
    }

    #[test]
    fn retroachievements_console_id_para_sistemas_suportados() {
        assert_eq!(
            get_retroachievements_console_id("sony-playstation"),
            Some(12)
        );
        assert_eq!(
            get_retroachievements_console_id("sony-playstation-2"),
            Some(21)
        );
        assert_eq!(get_retroachievements_console_id("sony-psp"), Some(41));
        assert_eq!(
            get_retroachievements_console_id("nintendo-gamecube"),
            Some(16)
        );
        assert_eq!(get_retroachievements_console_id("nintendo-wii"), Some(19));
        assert_eq!(get_retroachievements_console_id("nintendo-ds"), Some(18));
        assert_eq!(get_retroachievements_console_id("nintendo-nes"), Some(7));
        assert_eq!(get_retroachievements_console_id("nintendo-snes"), Some(3));
        assert_eq!(
            get_retroachievements_console_id("nintendo-game-boy"),
            Some(4)
        );
        assert_eq!(
            get_retroachievements_console_id("nintendo-game-boy-color"),
            Some(6)
        );
        assert_eq!(
            get_retroachievements_console_id("nintendo-game-boy-advance"),
            Some(5)
        );
        assert_eq!(get_retroachievements_console_id("nintendo-64"), Some(2));
        assert_eq!(get_retroachievements_console_id("nintendo-dsi"), Some(78));
    }

    #[test]
    fn retroachievements_console_id_devolve_none_para_ps3() {
        assert_eq!(get_retroachievements_console_id("sony-playstation-3"), None);
    }

    #[test]
    fn retroachievements_console_id_devolve_none_para_desconhecidos() {
        assert_eq!(get_retroachievements_console_id("unknown-system"), None);
        assert_eq!(get_retroachievements_console_id(""), None);
    }
}
