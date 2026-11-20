//! Textos do shell nativo nos mesmos idiomas da UI web.
//!
//! As mensagens vêm de `ui/src/i18n/*.json` (fonte única da tradução). O nativo
//! tem algumas frases que não existem na web — estados de carregamento, notas
//! desta passada —, e elas ficam em `assets/i18n-native.json`, sempre com o
//! pt-BR como último fallback antes da própria chave.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Idioma suportado pelo shell nativo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Lang {
    #[default]
    PtBr,
    EnUs,
    EsEs,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::PtBr, Lang::EnUs, Lang::EsEs];

    /// Converte o valor salvo em `config.language`; códigos desconhecidos
    /// devolvem `None` para o chamador decidir o fallback.
    pub fn from_code(code: &str) -> Option<Self> {
        match code.trim().to_ascii_lowercase().as_str() {
            "pt" | "pt-br" | "pt_br" | "ptbr" => Some(Self::PtBr),
            "en" | "en-us" | "en_us" | "enus" => Some(Self::EnUs),
            "es" | "es-es" | "es_es" | "eses" => Some(Self::EsEs),
            _ => None,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::PtBr => "pt-BR",
            Self::EnUs => "en-US",
            Self::EsEs => "es-ES",
        }
    }

    /// Nome do idioma no próprio idioma, como nos seletores da UI web.
    pub fn label(self) -> &'static str {
        match self {
            Self::PtBr => "Português (Brasil)",
            Self::EnUs => "English (US)",
            Self::EsEs => "Español",
        }
    }

    fn messages(self) -> &'static str {
        match self {
            Self::PtBr => include_str!("../../../ui/src/i18n/pt-BR.json"),
            Self::EnUs => include_str!("../../../ui/src/i18n/en-US.json"),
            Self::EsEs => include_str!("../../../ui/src/i18n/es-ES.json"),
        }
    }
}

/// Traduções que só o shell nativo usa. Mantê-las fora de `ui/` evita que a
/// próxima onda de telas precise mexer no JSON da web.
const NATIVE_OVERRIDES: &str = include_str!("../assets/i18n-native.json");

/// Mensagens carregadas para o idioma ativo.
///
/// Os mapas são estáticos e compartilhados: trocar de idioma apenas aponta
/// `map` para outro `HashMap`, sem reiniciar o app nem recarregar arquivos.
#[derive(Debug, Clone)]
pub struct I18n {
    lang: Lang,
    map: &'static HashMap<String, String>,
}

impl Default for I18n {
    fn default() -> Self {
        Self::new(Lang::default())
    }
}

impl I18n {
    pub fn new(lang: Lang) -> Self {
        Self {
            lang,
            map: map_for(lang),
        }
    }

    pub fn lang(&self) -> Lang {
        self.lang
    }

    /// Troca o idioma em runtime; a próxima pintura já usa o novo mapa.
    pub fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
        self.map = map_for(lang);
    }

    /// Texto da chave no idioma ativo. Fallback: pt-BR → a própria chave.
    pub fn t<'a>(&'a self, key: &'a str) -> &'a str {
        self.map
            .get(key)
            .map(String::as_str)
            .or_else(|| map_for(Lang::PtBr).get(key).map(String::as_str))
            .unwrap_or(key)
    }

    /// Substitui `{nome}`/`{pct}` etc. pelos valores informados.
    pub fn t_args(&self, key: &str, args: &[(&str, &str)]) -> String {
        let mut message = self.t(key).to_string();
        for (name, value) in args {
            message = message.replace(&format!("{{{name}}}"), value);
        }
        message
    }
}

fn map_for(lang: Lang) -> &'static HashMap<String, String> {
    static PT_BR: OnceLock<HashMap<String, String>> = OnceLock::new();
    static EN_US: OnceLock<HashMap<String, String>> = OnceLock::new();
    static ES_ES: OnceLock<HashMap<String, String>> = OnceLock::new();

    match lang {
        Lang::PtBr => PT_BR.get_or_init(|| build_map(Lang::PtBr)),
        Lang::EnUs => EN_US.get_or_init(|| build_map(Lang::EnUs)),
        Lang::EsEs => ES_ES.get_or_init(|| build_map(Lang::EsEs)),
    }
}

fn build_map(lang: Lang) -> HashMap<String, String> {
    let mut map = parse_messages(lang);
    for (key, value) in parse_native(lang) {
        map.insert(key, value);
    }
    map
}

/// Parseia só o JSON da UI web. Separado para os testes poderem conferir os
/// três arquivos sem a camada de frases nativas.
fn parse_messages(lang: Lang) -> HashMap<String, String> {
    serde_json::from_str(lang.messages())
        .unwrap_or_else(|error| panic!("i18n {} inválido: {error}", lang.code()))
}

/// Recorta o objeto do idioma dentro de `assets/i18n-native.json`.
fn parse_native(lang: Lang) -> HashMap<String, String> {
    static ALL: OnceLock<HashMap<String, HashMap<String, String>>> = OnceLock::new();
    let all = ALL.get_or_init(|| {
        serde_json::from_str(NATIVE_OVERRIDES)
            .unwrap_or_else(|error| panic!("i18n nativo inválido: {error}"))
    });

    // pt-BR é base: um idioma sem alguma chave nativa cai na tradução pt-BR,
    // e o `t` ainda tem a própria chave como último recurso.
    let mut messages = all.get(Lang::PtBr.code()).cloned().unwrap_or_default();
    if let Some(translated) = all.get(lang.code()) {
        messages.extend(translated.iter().map(|(key, value)| (key.clone(), value.clone())));
    }
    messages
}

#[cfg(test)]
impl I18n {
    /// Usado apenas em teste para simular um mapa com uma tradução faltando.
    fn from_map(lang: Lang, map: HashMap<String, String>) -> Self {
        Self {
            lang,
            map: Box::leak(Box::new(map)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parseia_os_tres_json_da_ui_web() {
        let pt = parse_messages(Lang::PtBr);
        let en = parse_messages(Lang::EnUs);
        let es = parse_messages(Lang::EsEs);

        assert!(pt.len() > 1000, "pt-BR tem {} chaves", pt.len());
        assert_eq!(pt.len(), en.len(), "en-US perdeu chaves em relação ao pt-BR");
        assert_eq!(pt.len(), es.len(), "es-ES perdeu chaves em relação ao pt-BR");
        assert_eq!(pt.get("settings.title").map(String::as_str), Some("Configurações"));
        assert_eq!(en.get("settings.title").map(String::as_str), Some("Settings"));
        assert_eq!(es.get("settings.title").map(String::as_str), Some("Configuración"));
    }

    #[test]
    fn traduz_por_idioma_e_faz_fallback_para_pt_br() {
        let mut en = parse_messages(Lang::EnUs);
        en.remove("sidebar.biblioteca");
        let i18n = I18n::from_map(Lang::EnUs, en);

        // Ativa não tem a chave → pt-BR.
        assert_eq!(i18n.t("sidebar.biblioteca"), "Biblioteca");
        // Ninguém tem a chave → devolve a própria chave.
        assert_eq!(i18n.t("chave.que.nao.existe"), "chave.que.nao.existe");
    }

    #[test]
    fn interpola_argumentos() {
        let i18n = I18n::new(Lang::EnUs);
        assert_eq!(
            i18n.t_args("about.versao", &[("versao", "0.1.0")]),
            "Arcadia 0.1.0"
        );

        let pt = I18n::new(Lang::PtBr);
        assert_eq!(
            pt.t_args("native.settings.cores", &[("count", "4")]),
            "4 núcleo(s)"
        );
    }

    #[test]
    fn troca_de_idioma_em_runtime() {
        let mut i18n = I18n::new(Lang::PtBr);
        assert_eq!(i18n.t("settings.title"), "Configurações");

        i18n.set_lang(Lang::EnUs);
        assert_eq!(i18n.lang(), Lang::EnUs);
        assert_eq!(i18n.t("settings.title"), "Settings");
        assert_eq!(i18n.t("sidebar.biblioteca"), "Library");

        i18n.set_lang(Lang::EsEs);
        assert_eq!(i18n.t("settings.title"), "Configuración");
        assert_eq!(i18n.t("sidebar.biblioteca"), "Biblioteca");
    }

    #[test]
    fn from_code_aceita_os_codigos_do_config() {
        assert_eq!(Lang::from_code("pt-BR"), Some(Lang::PtBr));
        assert_eq!(Lang::from_code("en-US"), Some(Lang::EnUs));
        assert_eq!(Lang::from_code("es-ES"), Some(Lang::EsEs));
        assert_eq!(Lang::from_code(" en-us "), Some(Lang::EnUs));
        assert_eq!(Lang::from_code("fr-FR"), None);
        assert_eq!(Lang::PtBr.code(), "pt-BR");
        assert_eq!(Lang::EnUs.label(), "English (US)");
    }

    #[test]
    fn chaves_das_telas_migradas_existem_nos_tres_idiomas() {
        const KEYS: &[&str] = &[
            // Sidebar e shell.
            "app.name",
            "sidebar.inicio",
            "sidebar.biblioteca",
            "sidebar.lojas",
            "sidebar.plugins",
            "sidebar.downloads",
            "sidebar.fontes",
            "sidebar.jogos",
            "sidebar.bigpicture",
            "sidebar.modo_desktop",
            "amigos.titulo",
            "settings.title",
            "native.profile.local",
            "native.section.not_ported",
            "native.section.back_library",
            "native.section.stub.home",
            "native.section.stub.library",
            "native.section.stub.stores",
            "native.section.stub.components",
            "native.section.stub.downloads",
            "native.section.stub.sources",
            "native.section.stub.friends",
            "native.section.stub.settings",
            "desktop.big_picture_desc",
            "native.game.not_found",
            "native.game.not_found_desc",
            // Biblioteca.
            "library.buscar",
            "library.todos_jogos",
            "library.vazia",
            "library.vazio",
            "common.carregando",
            "native.library.load_error",
            "downloads.tentar_novamente",
            "ps5.biblioteca.vazia",
            // Home.
            "native.home.continue_playing",
            "native.home.history_loading",
            "native.home.history_error",
            "native.home.library_error",
            "profile.nenhum_jogado",
            "native.home.view_library",
            "native.home.in_library",
            "native.home.grid_empty",
            "native.console.home.subtitle",
            "native.console.home.recent",
            "native.console.home.empty_title",
            "native.console.home.empty_desc",
            "native.console.library.rail",
            "native.console.hint.tabs",
            "native.console.hint.rail",
            "native.console.hint.overview",
            "native.console.hint.menu",
            "native.console.hint.store_search",
            "native.console.hint.store_editing",
            "native.console.hint.store_grid",
            "native.console.hint.store_detail",
            "native.console.store.subtitle",
            "native.console.store.catalog_count",
            "native.console.store.empty_title",
            "native.console.store.empty_desc",
            "native.console.store.error_title",
            "native.console.store.loading",
            "native.console.store.searching",
            "native.console.store.load_more",
            "native.console.store.detail_meta",
            "native.console.store.about",
            "native.console.store.info",
            "native.console.store.install",
            "native.console.store.install_unavailable",
            "native.console.store.install_note",
            "native.console.store.back",
            "native.console.store.clear",
            "native.console.store.price_missing",
            "native.console.store.manifest",
            "native.console.store.developer",
            "native.console.store.publisher",
            "native.console.store.release",
            "native.console.store.genres",
            "native.console.store.info_empty",
            "native.console.store.info_loading",
            "native.console.store.description_loading",
            "native.console.store.description_error",
            "native.console.store.description_empty",
            "native.console.favorite.add",
            "native.console.favorite.remove",
            "native.console.more",
            // Configurações.
            "settings.general",
            "settings.integracoes",
            "settings.paths",
            "settings.behavior",
            "settings.install_path.label",
            "settings.install_path.desc",
            "settings.steam_path.label",
            "settings.steam_path.desc",
            "settings.check_updates.label",
            "settings.check_updates.desc",
            "settings.console_mode.label",
            "settings.console_mode.desc",
            "settings.hide_changelog.label",
            "settings.hide_changelog.desc",
            "settings.minimize_on_launch.label",
            "settings.minimize_on_launch.desc",
            "settings.disable_playtime.label",
            "settings.disable_playtime.desc",
            "settings.discord_rpc.label",
            "settings.discord_rpc.desc",
            "settings.theme.label",
            "settings.theme.desc",
            "settings.download_cores.label",
            "settings.download_cores.desc",
            "settings.trailers",
            "settings.trailers.auto_tocar",
            "settings.salvar",
            "about.titulo",
            "idioma.idioma",
            "accessibility.sempre_titulos",
            "accessibility.desativar_animacoes",
            "common.ativado",
            "common.desativado",
            "common.padrao",
            "common.salvar",
            "native.refresh",
            "native.settings.subtitle",
            "native.settings.load_error",
            "native.settings.empty_title",
            "native.settings.empty_desc",
            "native.settings.saving",
            "native.settings.save_error",
            "native.settings.paths_note",
            "native.settings.active_note",
            "native.settings.discord_client_id_desc",
            "native.settings.invalid_color",
            "native.settings.appearance",
            "native.settings.playback",
            "native.settings.theme_group",
            "native.settings.accent_group",
            "native.settings.accent",
            "native.settings.accent_desc",
            "native.settings.always_titles_desc",
            "native.settings.no_anim_desc",
            "native.settings.destination",
            "native.settings.performance",
            "native.settings.downloads_note",
            "native.settings.auto_max",
            "native.settings.cores",
            "native.settings.notifications_music",
            "native.settings.notifications",
            "native.settings.notifications_desc",
            "native.settings.music",
            "native.settings.music_desc",
            "native.settings.trailer_auto_desc",
            "native.settings.playback_note",
            "native.settings.keys_tokens",
            "native.settings.keys_note",
            "native.settings.discord_on",
            "native.settings.discord_off",
            "native.settings.accounts",
            "native.settings.store_language",
            "native.settings.store_language_desc",
            "native.settings.integrations_note",
            "native.settings.version",
            "native.settings.version_desc",
            "native.settings.data_dir",
            "native.settings.data_dir_desc",
            "native.settings.about_theme_desc",
            "native.settings.language_desc",
            "native.settings.about_note",
        ];

        for lang in Lang::ALL {
            let i18n = I18n::new(lang);
            for key in KEYS {
                assert_ne!(
                    i18n.t(key),
                    *key,
                    "{} não traduz a chave {key}",
                    lang.code()
                );
            }
        }
    }
}
